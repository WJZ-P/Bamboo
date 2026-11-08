//! 用官方的 `StatePropertiesPredicate` 和 `BlockPredicate` 检查 `mc-bamboo-blocks::predicate`。
//!
//! `PredicateOracle.java` 抽大量真实的方块状态和随机谓词，按官方 `BlockPredicate.STREAM_CODEC`
//! 编码成十六进制，附上官方的答案写到 stdout。这边逐条解码，用 `predicate::matches` 重放：
//!
//! - 方块列表：不写、空列表、直接列表、方块标签；
//! - 属性：精确值、区间、缺一端、错值、不存在的属性名；
//! - NBT：带 NBT 的谓词官方要求方块实体，没有实体时不匹配。
//!
//! 官方答案取 `BlockPredicate.matches(BlockInWorld)` 在没有方块实体时的结果，和
//! `predicate::matches` 建模的是同一条路径。题目一条都不能少，解码失败也算差异。

use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_blocks::{BlockState, predicate};
use mc_bamboo_data::{blocks, version};
use mc_bamboo_protocol::components::{cursor::Cursor, predicate::BlockPredicate};
use serde::Deserialize;

use crate::oracle::collect_log;

const USAGE: &str = "usage: predicate-oracle <java> [--cases N] [--seed S] [--show N]";
/// `--show` 的默认值。
const DEFAULT_SHOW: usize = 10;

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
                "--cases" | "--seed" => {
                    let value = args.next().ok_or(USAGE)?;
                    options
                        .forwarded
                        .extend([arg.to_owned(), (*value).to_owned()]);
                }
                "--show" => {
                    let value = args.next().ok_or(USAGE)?;
                    options.show = Some(value.parse().map_err(|_| USAGE.to_owned())?);
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

/// 官方那一行 JSON。
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Line {
    Header {
        version: String,
        protocol: i32,
        /// 官方状态注册表里的状态数。
        states: u32,
    },
    Case {
        index: usize,
        /// 全局方块状态 ID，和 Bamboo 的 `BlockState` 是同一套编号。
        state: u32,
        /// 官方 `BlockPredicate.STREAM_CODEC` 编出来的字节。
        predicate: String,
        /// 官方 `BlockPredicate.matches(BlockInWorld)` 的答案。
        matches: bool,
    },
    End {
        cases: usize,
        /// 官方算出来匹配的题数，用来确认题目没有丢。
        matches: usize,
    },
}

/// 一次运行的计数和不一致。
#[derive(Debug, Default)]
struct Report {
    cases: usize,
    expected_matches: usize,
    ours_matches: usize,
    mismatches: Vec<String>,
    show: usize,
    header_seen: bool,
    ended: bool,
}

/// 启动官方进程，一边读题一边比较。`command` 已经是能运行 `PredicateOracle.java` 的命令。
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
        return Err(format!("PredicateOracle.java failed: {status}\n{log}"));
    }
    report.finish()
}

impl Report {
    fn read_lines(&mut self, source: BufReader<impl Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            // Minecraft 的日志也写到 stdout，题目行都是 JSON 对象。
            if !line.starts_with('{') {
                continue;
            }
            let case: Line = serde_json::from_str(&line)
                .map_err(|error| format!("bad predicate oracle line: {error}"))?;
            match case {
                Line::Header {
                    version,
                    protocol,
                    states,
                } => self.check_header(&version, protocol, states)?,
                Line::Case {
                    index,
                    state,
                    predicate,
                    matches,
                } => self.check_case(index, state, &predicate, matches)?,
                Line::End { cases, matches } => self.finish_end(cases, matches)?,
            }
        }
        Ok(())
    }

    /// 生成的数据和官方是同一个版本，状态数也一样。
    fn check_header(
        &mut self,
        official_version: &str,
        protocol: i32,
        states: u32,
    ) -> Result<(), String> {
        if official_version != version::GAME_VERSION || protocol != version::PROTOCOL_VERSION {
            return Err(format!(
                "official is {} protocol {protocol}, generated data is {} protocol {}",
                official_version,
                version::GAME_VERSION,
                version::PROTOCOL_VERSION
            ));
        }
        if states != blocks::TOTAL_STATES {
            return Err(format!(
                "official has {states} states, generated data has {}",
                blocks::TOTAL_STATES
            ));
        }
        self.header_seen = true;
        Ok(())
    }

    fn check_case(
        &mut self,
        index: usize,
        state: u32,
        predicate: &str,
        expected: bool,
    ) -> Result<(), String> {
        if index != self.cases {
            return Err(format!("case {index} arrived after {} cases", self.cases));
        }
        self.cases += 1;
        if expected {
            self.expected_matches += 1;
        }
        if state >= blocks::TOTAL_STATES {
            return Err(format!(
                "case {index} uses state {state}, generated data has {}",
                blocks::TOTAL_STATES
            ));
        }
        let bytes = unhex(predicate)?;
        let ours = match decode(&bytes) {
            Ok(predicate) => predicate::matches(&predicate, BlockState(state)),
            Err(error) => {
                self.mismatch(format!(
                    "case {index} state {state}: bamboo cannot decode the official predicate: {error}"
                ));
                return Ok(());
            }
        };
        if ours {
            self.ours_matches += 1;
        }
        if ours != expected {
            self.mismatch(format!(
                "case {index} state {state} predicate {predicate}: official {expected}, bamboo {ours}"
            ));
        }
        Ok(())
    }

    fn finish_end(&mut self, cases: usize, matches: usize) -> Result<(), String> {
        if self.ended {
            return Err("PredicateOracle.java wrote two end lines".to_owned());
        }
        self.ended = true;
        if cases != self.cases {
            return Err(format!(
                "java counted {cases} cases, {} arrived",
                self.cases
            ));
        }
        if matches != self.expected_matches {
            return Err(format!(
                "java counted {matches} matches, {} arrived",
                self.expected_matches
            ));
        }
        Ok(())
    }

    fn mismatch(&mut self, message: String) {
        self.mismatches.push(message);
    }

    fn finish(self) -> Result<(), String> {
        if !self.header_seen {
            return Err("PredicateOracle.java wrote no header".to_owned());
        }
        if !self.ended {
            return Err("PredicateOracle.java wrote no end line".to_owned());
        }
        println!(
            "predicate oracle: {} cases ({} official matches, {} bamboo matches), {} differences",
            self.cases,
            self.expected_matches,
            self.ours_matches,
            self.mismatches.len(),
        );
        for message in self.mismatches.iter().take(self.show) {
            println!("  {message}");
        }
        if self.mismatches.len() > self.show {
            println!(
                "  ... {} more differences",
                self.mismatches.len() - self.show
            );
        }
        if self.mismatches.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{} of {} cases differ",
                self.mismatches.len(),
                self.cases
            ))
        }
    }
}

/// 解码一条官方谓词，要求刚好用完全部字节。
fn decode(bytes: &[u8]) -> Result<BlockPredicate, String> {
    let mut cursor = Cursor::new(bytes);
    let predicate = BlockPredicate::read(&mut cursor).map_err(|error| error.to_string())?;
    cursor.finish().map_err(|error| error.to_string())?;
    Ok(predicate)
}

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err(format!("odd hex length {}", text.len()));
    }
    (0..text.len() / 2)
        .map(|index| {
            u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
                .map_err(|error| format!("bad hex: {error}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        assert_eq!(unhex("00ff10"), Ok(vec![0, 255, 16]));
        assert!(unhex("0").is_err());
        assert!(unhex("zz").is_err());
    }

    #[test]
    fn options_are_parsed() {
        assert!(Options::parse(&[]).is_ok());
        assert!(Options::parse(&["--cases", "20", "--seed", "7"]).is_ok());
        assert!(Options::parse(&["--show", "3"]).is_ok());
        assert!(Options::parse(&["--nope"]).is_err());
    }
}
