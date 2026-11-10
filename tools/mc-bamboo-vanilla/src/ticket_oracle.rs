//! 用官方 26.3 的 `DistanceManager` + `TicketStorage` 检查 `TicketTable`。
//!
//! `TicketOracle.java` 在 JVM 里直接驱动官方的距离管理器，不启动服务端：随机加票、撤票、
//! 玩家移动、改视距和模拟距离、推进若干刻，每步报告一个 49×49 窗口里全部区块的两个等级。
//! 这里把同一串操作重放到 `TicketTable`，用同样的 FNV-1a 哈希比较，出错时用 `--detail`
//! 输出的非默认区块列表定位第一个不一致的位置。
//!
//! 官方的 `player_loading` 票增删经过 `ThrottlingChunkTaskDispatcher`（每刻 4 个在途），
//! 所以 Java 侧把每个操作推进到稳定再出题；`TicketTable` 同步给满，稳定状态应逐位一致。
//! 超时票在"区块忙"的分支上也要一致，忙的位置由 Java 侧出题、这里作为 `tick` 的输入。

use std::{
    collections::{BTreeSet, HashMap},
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_data::version;
use mc_bamboo_world::ticket::{
    FULL_CHUNK_LEVEL, SIMULATION_MAX_LEVEL, TicketTable, TicketType, UNLOADED_LEVEL,
};
use serde::Deserialize;

use crate::oracle::collect_log;

const USAGE: &str =
    "usage: ticket-oracle <java> [--scenes N] [--seed S] [--scene I] [--detail] [--show N]";
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;
const DEFAULT_SHOW: usize = 5;
/// 和 Java 的注册顺序一致。
const TYPES: [TicketType; 9] = TicketType::ALL;

#[derive(Debug, Default)]
struct Options {
    forwarded: Vec<String>,
    show: Option<usize>,
    detail: bool,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--scenes" | "--seed" | "--scene" => {
                    let value = args.next().ok_or(USAGE)?;
                    options
                        .forwarded
                        .extend([arg.to_owned(), (*value).to_owned()]);
                }
                "--show" => {
                    let value = args.next().ok_or(USAGE)?;
                    options.show = Some(value.parse().map_err(|_| USAGE)?);
                }
                "--detail" => {
                    options.detail = true;
                    options.forwarded.push(arg.to_owned());
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

#[derive(Debug, Deserialize)]
struct Header {
    version: String,
    protocol: i32,
    radius: i32,
}

#[derive(Debug, Deserialize)]
struct Step {
    scene: u32,
    step: u32,
    op: StepOp,
    hash: String,
    count: usize,
    #[serde(default)]
    levels: Vec<[i32; 4]>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op")]
enum StepOp {
    #[serde(rename = "add")]
    Add {
        r#type: usize,
        x: i32,
        z: i32,
        value: i32,
    },
    #[serde(rename = "remove")]
    Remove {
        r#type: usize,
        x: i32,
        z: i32,
        value: i32,
    },
    #[serde(rename = "addRadius")]
    AddRadius {
        r#type: usize,
        x: i32,
        z: i32,
        value: i32,
    },
    #[serde(rename = "removeRadius")]
    RemoveRadius {
        r#type: usize,
        x: i32,
        z: i32,
        value: i32,
    },
    #[serde(rename = "player")]
    Player { id: u64, has: bool, x: i32, z: i32 },
    #[serde(rename = "view")]
    View { distance: i32 },
    #[serde(rename = "sim")]
    Sim { distance: i32 },
    #[serde(rename = "forced")]
    Forced { x: i32, z: i32, value: bool },
    #[serde(rename = "tick")]
    Tick { busy: Vec<[i32; 2]> },
}

#[derive(Debug, Deserialize)]
struct End {
    scenes: u32,
}

#[derive(Debug, Default)]
struct Report {
    scenes: u32,
    steps: u32,
    mismatches: usize,
    shown: usize,
    show: usize,
    detail: bool,
}

impl Report {
    fn hash(table: &TicketTable, radius: i32) -> (String, usize, Vec<[i32; 4]>) {
        let mut hash = FNV_OFFSET;
        let mut count = 0usize;
        let mut levels = Vec::new();
        for x in -radius..=radius {
            for z in -radius..=radius {
                let pos = ChunkPos::new(x, z);
                let loading = table.loading_level(pos);
                let simulation = table.simulation_level(pos);
                hash = mix(hash, x);
                hash = mix(hash, z);
                hash = mix(hash, loading);
                hash = mix(hash, simulation);
                if loading != UNLOADED_LEVEL || simulation != SIMULATION_MAX_LEVEL {
                    count += 1;
                    levels.push([x, z, loading, simulation]);
                }
            }
        }
        (hash.to_string(), count, levels)
    }

    fn check(&mut self, step: &Step, table: &TicketTable, radius: i32) -> Result<(), String> {
        let (hash, count, levels) = Self::hash(table, radius);
        self.steps += 1;
        if hash == step.hash && count == step.count {
            return Ok(());
        }
        self.mismatches += 1;
        if self.shown < self.show {
            self.shown += 1;
            eprintln!(
                "场景 {} 第 {} 步 {:?}：官方 hash={} count={}，Bamboo hash={hash} count={count}",
                step.scene, step.step, step.op, step.hash, step.count
            );
            if self.detail && !step.levels.is_empty() {
                let ours: HashMap<(i32, i32), (i32, i32)> = levels
                    .into_iter()
                    .map(|[x, z, l, s]| ((x, z), (l, s)))
                    .collect();
                let theirs: HashMap<(i32, i32), (i32, i32)> = step
                    .levels
                    .iter()
                    .map(|&[x, z, l, s]| ((x, z), (l, s)))
                    .collect();
                let keys: BTreeSet<_> = ours.keys().chain(theirs.keys()).copied().collect();
                let mut printed = 0;
                for key in keys {
                    let a = theirs.get(&key).copied();
                    let b = ours.get(&key).copied();
                    if a != b && printed < 12 {
                        eprintln!("  ({},{}) 官方 {a:?} Bamboo {b:?}", key.0, key.1);
                        printed += 1;
                    }
                }
            } else {
                eprintln!("  用 --detail 看具体哪些区块不一致");
            }
        }
        Ok(())
    }
}

fn mix(mut hash: u64, value: i32) -> u64 {
    for i in 0..4 {
        hash ^= u64::from((value >> (i * 8)) as u8);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// 一个场景一张表，按 Java 侧发出的操作顺序重放。
fn apply(table: &mut TicketTable, op: &StepOp) {
    match *op {
        StepOp::Add {
            r#type,
            x,
            z,
            value,
        } => {
            table.add(TYPES[r#type], ChunkPos::new(x, z), value);
        }
        StepOp::Remove {
            r#type,
            x,
            z,
            value,
        } => {
            table.remove(TYPES[r#type], ChunkPos::new(x, z), value);
        }
        StepOp::AddRadius {
            r#type,
            x,
            z,
            value,
        } => {
            table.add(TYPES[r#type], ChunkPos::new(x, z), FULL_CHUNK_LEVEL - value);
        }
        StepOp::RemoveRadius {
            r#type,
            x,
            z,
            value,
        } => {
            table.remove(TYPES[r#type], ChunkPos::new(x, z), FULL_CHUNK_LEVEL - value);
        }
        StepOp::Player { id, has, x, z } => {
            table.set_player(id, has.then_some(ChunkPos::new(x, z)));
        }
        StepOp::View { distance } => table.set_view_distance(distance),
        StepOp::Sim { distance } => table.set_simulation_distance(distance),
        StepOp::Forced { x, z, value } => table.set_forced(ChunkPos::new(x, z), value),
        StepOp::Tick { ref busy } => {
            let busy: BTreeSet<(i32, i32)> = busy.iter().map(|&[x, z]| (x, z)).collect();
            table.tick(&mut |pos| busy.contains(&(pos.x, pos.z)));
        }
    }
    table.run_updates();
}

fn finish_report(report: &Report, ended: bool) -> Result<(), String> {
    if !ended {
        return Err("TicketOracle.java 没写 end 行".to_owned());
    }
    println!(
        "ticket oracle：{} 个场景、{} 步、{} 处不一致",
        report.scenes, report.steps, report.mismatches
    );
    if report.mismatches == 0 {
        Ok(())
    } else {
        Err(format!("{} 步和官方不一致", report.mismatches))
    }
}

pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let mut child = command
        .args(&options.forwarded)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut report = Report {
        show: options.show.unwrap_or(DEFAULT_SHOW),
        detail: options.detail,
        ..Report::default()
    };
    let read = read_lines(stdout, &mut report);
    if read.is_err() {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    if !status.success() {
        return Err(format!("TicketOracle.java failed: {status}\n{log}"));
    }
    let read = read.map_err(|error| format!("{error}\n{log}"))?;
    finish_report(&report, read)
}

fn read_lines(source: impl Read, report: &mut Report) -> Result<bool, String> {
    let mut radius = 0;
    let mut header_ok = false;
    let mut ended = false;
    let mut current_scene: Option<u32> = None;
    let mut table = TicketTable::new();
    for line in BufReader::new(source).lines() {
        let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
        if !line.starts_with('{') {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)
            .map_err(|error| format!("bad ticket oracle line: {error}"))?;
        match value["type"].as_str() {
            Some("header") => {
                let header: Header = serde_json::from_value(value)
                    .map_err(|error| format!("bad header: {error}"))?;
                if header.version != version::GAME_VERSION
                    || header.protocol != version::PROTOCOL_VERSION
                {
                    return Err(format!(
                        "官方是 {} / {}，Bamboo 是 {} / {}",
                        header.version,
                        header.protocol,
                        version::GAME_VERSION,
                        version::PROTOCOL_VERSION
                    ));
                }
                radius = header.radius;
                header_ok = true;
            }
            Some("step") => {
                if !header_ok {
                    return Err("step 出现在 header 之前".to_owned());
                }
                let step: Step =
                    serde_json::from_value(value).map_err(|error| format!("bad step: {error}"))?;
                if current_scene != Some(step.scene) {
                    current_scene = Some(step.scene);
                    table = TicketTable::new();
                    table.set_view_distance(4);
                    table.run_updates();
                    report.scenes += 1;
                }
                apply(&mut table, &step.op);
                report.check(&step, &table, radius)?;
            }
            Some("end") => {
                let end: End =
                    serde_json::from_value(value).map_err(|error| format!("bad end: {error}"))?;
                if end.scenes != report.scenes {
                    return Err(format!(
                        "java wrote {} scenes, {} arrived",
                        end.scenes, report.scenes
                    ));
                }
                ended = true;
            }
            other => return Err(format!("unknown line type: {other:?}")),
        }
    }
    if report.steps == 0 {
        return Err("官方没有出任何一步".to_owned());
    }
    Ok(ended)
}
