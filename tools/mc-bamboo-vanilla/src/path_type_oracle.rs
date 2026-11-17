//! `path-type`：把每个方块状态预算的寻路类型和官方对拍。
//!
//! `PathTypeOracle.java` 用官方 `WalkNodeEvaluator.getPathTypeFromState` 和
//! `checkNeighbourBlocks` 给全部方块状态出题，Bamboo 侧查
//! `mc_bamboo_data` 生成的表逐项比较：`self`、`neighbor` 和动态形状标记。
//! 形状依赖世界或方块实体的状态两边都写 `None`，表示要运行时算。
//! `--break-self-test` 故意改坏一个状态的答案，确认比较器真的会报不一致。
//!
//! `path-type`: compare the precomputed per-state path types with vanilla.
//! `PathTypeOracle.java` asks official `WalkNodeEvaluator.getPathTypeFromState`
//! and `checkNeighbourBlocks` about every block state; Bamboo looks up its
//! generated tables and compares `self`, `neighbor` and the dynamic-shape flag.
//! States whose shape depends on the world or a block entity report `None` on
//! both sides and are computed at runtime. `--break-self-test` deliberately
//! corrupts one answer to confirm the comparator fails.

use std::process::Command;

use mc_bamboo_data::{PathType, blocks, neighbor_path_type_of_state, path_type_of_state, version};
use serde::Deserialize;

const USAGE: &str = "usage: path-type <java> [--break-self-test] [--show N]";
const DEFAULT_SHOW: usize = 10;

#[derive(Debug)]
struct Options {
    break_self_test: bool,
    show: usize,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            break_self_test: false,
            show: DEFAULT_SHOW,
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--break-self-test" => options.break_self_test = true,
                "--show" => {
                    options.show = args
                        .next()
                        .ok_or(USAGE)?
                        .parse()
                        .map_err(|_| USAGE.to_owned())?;
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Line {
    Header {
        version: String,
        protocol: i32,
        states: usize,
    },
    Case {
        #[serde(rename = "self")]
        self_types: Vec<Option<String>>,
        neighbor: Vec<Option<String>>,
        dynamic: Vec<bool>,
    },
    End {
        states: usize,
    },
}

#[derive(Debug, Default)]
struct Report {
    states: usize,
    comparisons: u64,
    dynamic: usize,
    header_seen: bool,
    ended: bool,
    show: usize,
    mismatches: Vec<String>,
}

impl Report {
    fn mismatch(&mut self, text: String) {
        if self.mismatches.len() < self.show {
            self.mismatches.push(text);
        }
    }

    fn compare(&mut self, options: &Options, line: Line) -> Result<(), String> {
        match line {
            Line::Header {
                version: game_version,
                protocol,
                states,
            } => {
                if game_version != version::GAME_VERSION || protocol != version::PROTOCOL_VERSION {
                    return Err(format!(
                        "生成的数据是 {}（协议 {}），官方 jar 是 {}（协议 {}）",
                        version::GAME_VERSION,
                        version::PROTOCOL_VERSION,
                        game_version,
                        protocol
                    ));
                }
                if states != blocks::TOTAL_STATES as usize {
                    return Err(format!(
                        "官方有 {states} 个方块状态，生成的表有 {}",
                        blocks::TOTAL_STATES
                    ));
                }
                self.header_seen = true;
                self.states = states;
            }
            Line::Case {
                self_types,
                neighbor,
                dynamic,
            } => {
                if !self.header_seen {
                    return Err("官方先写了题目，没有 header".to_owned());
                }
                if self_types.len() != self.states
                    || neighbor.len() != self.states
                    || dynamic.len() != self.states
                {
                    return Err(format!(
                        "官方的数组长度不一致：{} / {} / {}，应为 {}",
                        self_types.len(),
                        neighbor.len(),
                        dynamic.len(),
                        self.states
                    ));
                }
                for state in 0..self.states {
                    let id = state as u32;
                    let corrupted = Some("OPEN".to_owned());
                    let official_self = if options.break_self_test && state == 17 {
                        // 石头本来是 BLOCKED，故意换成 OPEN。
                        // Stone is BLOCKED in vanilla; deliberately change it to OPEN.
                        &corrupted
                    } else {
                        &self_types[state]
                    };
                    check(
                        self,
                        state,
                        "self",
                        official_self,
                        path_type_of_state(id),
                        dynamic[state],
                    )?;
                    check(
                        self,
                        state,
                        "neighbor",
                        &neighbor[state],
                        neighbor_path_type_of_state(id),
                        dynamic[state],
                    )?;
                    if dynamic[state] {
                        self.dynamic += 1;
                        if path_type_of_state(id).is_some()
                            || neighbor_path_type_of_state(id).is_some()
                        {
                            self.mismatch(format!("状态 {id}: 动态形状却预算了寻路类型"));
                        }
                    }
                }
            }
            Line::End { states } => {
                if states != self.states {
                    return Err(format!("官方收尾写了 {states} 项，题目是 {}", self.states));
                }
                self.ended = true;
            }
        }
        Ok(())
    }
}

fn check(
    report: &mut Report,
    state: usize,
    field: &str,
    official: &Option<String>,
    ours: Option<PathType>,
    dynamic: bool,
) -> Result<(), String> {
    report.comparisons += 1;
    let expected = official
        .as_deref()
        .map(|name| {
            PathType::by_name(name).ok_or_else(|| format!("官方给了不认识的 PathType: {name}"))
        })
        .transpose()?;
    if dynamic && expected.is_some() {
        report.mismatch(format!("状态 {state} 的 {field}: 动态形状却给了官方答案"));
    }
    if expected != ours {
        report.mismatch(format!(
            "状态 {state} 的 {field}: 官方 {:?}，Bamboo {:?}",
            expected.map(PathType::name),
            ours.map(PathType::name)
        ));
    }
    Ok(())
}

/// 启动官方 `PathTypeOracle.java`，逐状态比较整张表。
pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    command.env(
        "JAVA_TOOL_OPTIONS",
        "-Dstdout.encoding=UTF-8 -Dstderr.encoding=UTF-8",
    );
    let output = command
        .output()
        .map_err(|error| format!("cannot run java: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "PathTypeOracle.java failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|error| format!("oracle stdout is not UTF-8: {error}"))?;
    let mut report = Report {
        show: options.show,
        ..Report::default()
    };
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let parsed: Line = serde_json::from_str(line)
            .map_err(|error| format!("bad path-type oracle line: {error}"))?;
        report.compare(&options, parsed)?;
    }
    if !report.header_seen || !report.ended {
        return Err("官方输出缺 header 或 end".to_owned());
    }
    println!(
        "path-type: {} states, {} comparisons, {} runtime-computed, {} mismatches",
        report.states,
        report.comparisons,
        report.dynamic,
        report.mismatches.len()
    );
    for mismatch in &report.mismatches {
        println!("  {mismatch}");
    }
    if options.break_self_test {
        if report.mismatches.is_empty() {
            return Err("path-type self-test broke an answer but reported 0 mismatches".to_owned());
        }
        println!(
            "path-type self-test: detector works ({} mismatches)",
            report.mismatches.len()
        );
        return Ok(());
    }
    if report.mismatches.is_empty() {
        Ok(())
    } else {
        Err(format!("path-type: {} mismatches", report.mismatches.len()))
    }
}
