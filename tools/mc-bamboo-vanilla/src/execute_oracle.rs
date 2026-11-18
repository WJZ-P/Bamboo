//! `execute-oracle`：整条 `/execute` 命令的副作用和反馈对拍。
//!
//! Java 侧 `ExecuteOracle.java` 在假世界里用官方 `Commands.performPrefixedCommand`
//! 跑固定脚本，输出每条命令以后的方块、计分板和反馈文本；Rust 侧用
//! `mc-bamboo-game::replay_execute` 在同样的小世界里重放并逐项比较。
//! `--break-self-test` 故意改坏一项副作用，确认会被抓到。
//!
//! `execute-oracle`: compare whole `/execute` commands by side effects and
//! feedback. `--break-self-test` deliberately corrupts one effect.

use std::process::Command;

use serde_json::Value as Json;

const USAGE: &str = "usage: execute-oracle <java> [--show N] [--break-self-test]";

/// Java 和 Rust 都按这个脚本跑；每条命令从同一份干净场景开始。
const SCENARIOS: &[&str] = &[
    "execute positioned 4 -60 6 run setblock 4 -60 6 stone",
    "execute if block 4 -60 6 stone run say yes",
    "execute unless block 4 -60 6 stone run setblock 4 -60 6 stone",
    "execute as @s at @s run setblock 4 -60 6 stone",
    "execute store success score #total result run setblock 4 -60 6 stone",
];

#[derive(Debug)]
struct Options {
    show: usize,
    break_self_test: bool,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            show: 8,
            break_self_test: false,
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--show" => {
                    options.show = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?;
                }
                "--break-self-test" => options.break_self_test = true,
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let output = command
        .output()
        .map_err(|error| format!("cannot run java: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ExecuteOracle.java failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|error| format!("oracle stdout is not UTF-8: {error}"))?;
    let mut cases = 0usize;
    let mut mismatches = Vec::new();
    for line in text
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
    {
        let value: Json =
            serde_json::from_str(line).map_err(|error| format!("bad oracle line: {error}"))?;
        match value.get("type").and_then(Json::as_str) {
            Some("header") => {
                if value.get("version").and_then(Json::as_str)
                    != Some(mc_bamboo_data::version::GAME_VERSION)
                {
                    return Err("oracle version does not match the generated data".to_owned());
                }
            }
            Some("case") => {
                let input = value
                    .get("input")
                    .and_then(Json::as_str)
                    .ok_or("case line missing input")?;
                let expected = effects(&value)?;
                let mut ours = mc_bamboo_game::replay_execute(input);
                if options.break_self_test && cases == 0 {
                    ours[0] ^= 1;
                }
                cases += 1;
                if ours != expected {
                    mismatches.push(format!("{input}: java {expected:?}, rust {ours:?}"));
                }
            }
            Some("end") => {}
            other => return Err(format!("unexpected oracle line type {other:?}")),
        }
    }
    for scenario in SCENARIOS {
        if !text.contains(&format!("\"input\":{}", quoted(scenario))) {
            return Err(format!("oracle did not emit scenario {scenario}"));
        }
    }
    for mismatch in mismatches.iter().take(options.show) {
        println!("{mismatch}");
    }
    if mismatches.is_empty() {
        println!("execute-oracle: {cases} commands, all side effects and feedback matched");
        Ok(())
    } else {
        Err(format!(
            "execute-oracle: {} mismatch(es) in {cases} commands",
            mismatches.len()
        ))
    }
}

fn effects(value: &Json) -> Result<[i32; 6], String> {
    let mut out = [0; 6];
    for (index, name) in [
        "stone_4_6",
        "stone_cow_a",
        "stone_cow_b",
        "score",
        "ran",
        "feedback",
    ]
    .iter()
    .enumerate()
    {
        out[index] = value
            .get(name)
            .and_then(Json::as_i64)
            .and_then(|number| i32::try_from(number).ok())
            .ok_or_else(|| format!("case line missing {name}"))?;
    }
    Ok(out)
}

fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}
