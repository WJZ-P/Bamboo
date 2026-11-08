//! 用官方的光照引擎检查 `mc-bamboo-world` 的光照。
//!
//! `LightOracle.java` 在 JVM 里铺一片真正的官方区块，按官方流程算好超平坦的光照，再分几个 tick 随机改方块，
//! 每个 tick 之后报告全部光照段、光照变了的段和随机位置上的光照值。这里边读边在 [`ChunkStore`] 里做同样的改动，
//! 等排着的光照全部算完再逐项比较。题目不落盘。
//!
//! 官方那边任务攒够 1000 个就当场算一轮；这边改方块时只排轮次，改完才一轮轮地算，
//! 每一轮读它排好那一刻的方块，而且每次只给一点工作量，一轮要停好几次才算完。
//! 两边一致，说明晚算、分几次算的轮次和当场一口气算完的结果相同。
//!
//! 比较的是官方区块包里的那种数据：一段不存光照、存着但没分配数组（带默认值）、还是一整个数组，
//! 数组按 64 位 FNV-1a 哈希比较。`--detail` 让官方逐段描述，不一致时能看出是哪一段。

use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};
use mc_bamboo_data::version;
use mc_bamboo_world::{
    light::{DataLayer, LIGHT_SECTION_COUNT, MIN_LIGHT_SECTION, SectionPos},
    store::ChunkStore,
};
use serde::Deserialize;

use crate::oracle::collect_log;

const USAGE: &str = "usage: light <java> [--scenes N] [--seed S] [--scene I] [--detail] [--show N]";
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;
const LAYERS: [LightLayer; 2] = [LightLayer::Sky, LightLayer::Block];
/// `--show` 的默认值。
const DEFAULT_SHOW: usize = 5;
/// 每次只给光照这么多工作量，一轮会停在各种地方再接着算。取一个和 1000 对不齐的数。
const WORK_PER_RUN: usize = 997;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Line {
    Header {
        version: String,
        protocol: i32,
        radius: i32,
        template: String,
    },
    Scene {
        index: u32,
        ticks: Vec<Tick>,
    },
    End {
        scenes: u32,
    },
}

#[derive(Debug, Deserialize)]
struct Tick {
    /// x、y、z、状态 ID。
    edits: Vec<[i32; 4]>,
    columns: Vec<Column>,
    /// 区块 x、z、天空光位图、方块光位图。
    changes: Vec<[i64; 4]>,
    /// x、y、z、天空光、方块光。
    samples: Vec<[i32; 5]>,
}

/// 一个区块柱：平时是整柱的哈希，`--detail` 时是每一段的描述。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Column {
    Hash(String),
    Sections(Vec<String>),
}

#[derive(Debug, Default)]
struct Options {
    forwarded: Vec<String>,
    show: Option<usize>,
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
                "--detail" => options.forwarded.push(arg.to_owned()),
                "--show" => {
                    let value = args.next().ok_or(USAGE)?;
                    options.show = Some(value.parse().map_err(|_| USAGE)?);
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

/// 一次运行的计数和不一致。
#[derive(Debug, Default)]
struct Report {
    radius: i32,
    scenes: u32,
    ticks: usize,
    edits: usize,
    columns: usize,
    samples: usize,
    change_sets: usize,
    template_checked: bool,
    mismatches: Vec<String>,
    show: usize,
    ended: bool,
}

/// 启动官方进程，一边读一边比较。`command` 已经是能运行 `LightOracle.java` 的命令。
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
        ..Report::default()
    };
    let read = report.read_lines(BufReader::new(stdout));
    if read.is_err() {
        // 这边不再读了，JVM 会一直等着管道。
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    read.map_err(|error| format!("{error}\n{log}"))?;
    if !status.success() {
        return Err(format!("LightOracle.java failed: {status}\n{log}"));
    }
    report.finish()
}

impl Report {
    fn read_lines(&mut self, source: BufReader<impl Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            let parsed: Line = serde_json::from_str(&line)
                .map_err(|error| format!("bad light oracle line: {error}"))?;
            match parsed {
                Line::Header {
                    version,
                    protocol,
                    radius,
                    template,
                } => self.check_header(&version, protocol, radius, &template)?,
                Line::Scene { index, ticks } => self.replay(index, &ticks),
                Line::End { scenes } => {
                    self.ended = true;
                    if scenes != self.scenes {
                        return Err(format!(
                            "java wrote {scenes} scenes, {} arrived",
                            self.scenes
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn check_header(
        &mut self,
        game_version: &str,
        protocol: i32,
        radius: i32,
        template: &str,
    ) -> Result<(), String> {
        if game_version != version::GAME_VERSION || protocol != version::PROTOCOL_VERSION {
            return Err(format!(
                "the generated data is for {} (protocol {}), the official jar is {game_version} (protocol {protocol})",
                version::GAME_VERSION,
                version::PROTOCOL_VERSION,
            ));
        }
        self.radius = radius;
        let ours = hex(column_hash(&ChunkStore::flat(), ChunkPos::new(0, 0)));
        if ours != template {
            self.mismatch(format!(
                "template column: official {template}, bamboo {ours}"
            ));
        }
        self.template_checked = true;
        Ok(())
    }

    /// 在一个新的超平坦世界里重放一道题的每个 tick，每个 tick 等光照全部算完再比较。
    fn replay(&mut self, index: u32, ticks: &[Tick]) {
        self.scenes += 1;
        let mut store = ChunkStore::flat();
        for (number, tick) in ticks.iter().enumerate() {
            for &[x, y, z, state] in &tick.edits {
                store.set_block(BlockPos::new(x, y, z), state as u32);
            }
            while store.has_pending_light_updates() {
                store.run_light_updates(WORK_PER_RUN);
            }
            let place = format!("scene {index} tick {number}");
            self.compare_columns(&store, tick, &place);
            self.compare_changes(&mut store, tick, &place);
            self.compare_samples(&store, tick, &place);
            self.ticks += 1;
            self.edits += tick.edits.len();
        }
    }

    fn compare_columns(&mut self, store: &ChunkStore, tick: &Tick, place: &str) {
        let range = -self.radius..=self.radius;
        let positions = range
            .clone()
            .flat_map(|x| range.clone().map(move |z| ChunkPos::new(x, z)));
        for (pos, official) in positions.zip(&tick.columns) {
            self.columns += 1;
            match official {
                Column::Hash(hash) => {
                    let ours = hex(column_hash(store, pos));
                    if ours != *hash {
                        self.mismatch(format!(
                            "{place}: column ({}, {}) differs, rerun with --scene and --detail",
                            pos.x, pos.z
                        ));
                    }
                }
                Column::Sections(sections) => self.compare_sections(store, pos, sections, place),
            }
        }
    }

    fn compare_sections(
        &mut self,
        store: &ChunkStore,
        pos: ChunkPos,
        official: &[String],
        place: &str,
    ) {
        let ours = LAYERS.into_iter().flat_map(|layer| {
            (0..LIGHT_SECTION_COUNT).map(move |index| describe(section(store, layer, pos, index)))
        });
        for (slot, (ours, official)) in ours.zip(official).enumerate() {
            if ours != *official {
                let layer = LAYERS[slot / LIGHT_SECTION_COUNT];
                let section_y = MIN_LIGHT_SECTION + (slot % LIGHT_SECTION_COUNT) as i32;
                self.mismatch(format!(
                    "{place}: {layer:?} light of section ({}, {section_y}, {}): official {official}, bamboo {ours}",
                    pos.x, pos.z
                ));
            }
        }
    }

    /// Java 那边按打包后的区块坐标排序，所以两边都排一遍再比。
    fn compare_changes(&mut self, store: &mut ChunkStore, tick: &Tick, place: &str) {
        self.change_sets += 1;
        let mut official = tick.changes.clone();
        official.sort_unstable();
        let mut ours: Vec<[i64; 4]> = store
            .take_light_changes()
            .into_iter()
            .map(|(pos, changes)| {
                [
                    i64::from(pos.x),
                    i64::from(pos.z),
                    i64::from(changes.sky),
                    i64::from(changes.block),
                ]
            })
            .collect();
        ours.sort_unstable();
        if ours != official {
            self.mismatch(format!(
                "{place}: changed sections differ\n  official {official:?}\n  bamboo   {ours:?}"
            ));
        }
    }

    fn compare_samples(&mut self, store: &ChunkStore, tick: &Tick, place: &str) {
        for &[x, y, z, sky, block] in &tick.samples {
            self.samples += 1;
            let pos = BlockPos::new(x, y, z);
            let ours = [
                i32::from(store.light_value(LightLayer::Sky, pos)),
                i32::from(store.light_value(LightLayer::Block, pos)),
            ];
            if ours != [sky, block] {
                self.mismatch(format!(
                    "{place}: light at ({x}, {y}, {z}): official sky {sky} block {block}, bamboo sky {} block {}",
                    ours[0], ours[1]
                ));
            }
        }
    }

    fn mismatch(&mut self, message: String) {
        if self.mismatches.len() < self.show {
            println!("MISMATCH {message}");
        }
        self.mismatches.push(message);
    }

    fn finish(&self) -> Result<(), String> {
        if !self.ended {
            return Err("LightOracle.java stopped before its end line".to_owned());
        }
        let sections = self.columns * LIGHT_SECTION_COUNT * LAYERS.len();
        println!(
            "light: {} scenes, {} ticks, {} edits; compared {} columns ({sections} light sections), {} changed-section sets, {} sampled positions",
            self.scenes, self.ticks, self.edits, self.columns, self.change_sets, self.samples
        );
        println!(
            "template column: {}",
            if self.template_checked && !self.mismatches.iter().any(|m| m.starts_with("template")) {
                "matches official"
            } else {
                "differs"
            }
        );
        println!("LIGHT RESULT: {} mismatches", self.mismatches.len());
        if self.mismatches.is_empty() {
            Ok(())
        } else {
            Err(format!("{} light mismatches", self.mismatches.len()))
        }
    }
}

fn section(
    store: &ChunkStore,
    layer: LightLayer,
    pos: ChunkPos,
    index: usize,
) -> Option<&DataLayer> {
    let section = SectionPos::new(pos.x, MIN_LIGHT_SECTION + index as i32, pos.z);
    store.light().section(layer, section)
}

/// 和 `LightOracle.java` 相同的哈希：天空光再方块光，每层 26 段。
fn column_hash(store: &ChunkStore, pos: ChunkPos) -> u64 {
    let mut hash = FNV_OFFSET;
    for layer in LAYERS {
        for index in 0..LIGHT_SECTION_COUNT {
            hash = match section(store, layer, pos, index) {
                None => mix(hash, 0),
                Some(DataLayer::Filled(value)) => mix(mix(hash, 1), *value),
                Some(DataLayer::Data(data)) => data
                    .iter()
                    .fold(mix(hash, 2), |hash, &byte| mix(hash, byte)),
            };
        }
    }
    hash
}

/// `--detail` 里一段的描述：A 不存，F 加默认值，D 加数组的哈希。
fn describe(layer: Option<&DataLayer>) -> String {
    match layer {
        None => "A".to_owned(),
        Some(DataLayer::Filled(value)) => format!("F{value}"),
        Some(DataLayer::Data(data)) => {
            let hash = data.iter().fold(FNV_OFFSET, |hash, &byte| mix(hash, byte));
            format!("D{}", hex(hash))
        }
    }
}

fn mix(hash: u64, value: u8) -> u64 {
    (hash ^ u64::from(value)).wrapping_mul(FNV_PRIME)
}

fn hex(value: u64) -> String {
    format!("{value:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_the_java_definition() {
        // FNV-1a 64 的公开测试向量：空输入和单个字节 'a'。
        assert_eq!(FNV_OFFSET, 0xcbf2_9ce4_8422_2325);
        assert_eq!(mix(FNV_OFFSET, b'a'), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn options_forward_the_scene_selection() {
        let options =
            Options::parse(&["--scene", "7", "--detail", "--show", "2"]).expect("合法的参数");
        assert_eq!(options.forwarded, ["--scene", "7", "--detail"]);
        assert_eq!(options.show, Some(2));
        assert!(Options::parse(&["--bogus"]).is_err());
    }
}
