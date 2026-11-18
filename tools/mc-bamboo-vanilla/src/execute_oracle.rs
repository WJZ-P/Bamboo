//! `execute-oracle`：整条 `/execute` 命令的副作用和反馈对拍。
//!
//! Java 侧 `ExecuteOracle.java` 在假世界里用官方 `Commands.performCommand`
//! 跑 32 条固定脚本，输出每条命令以后的方块、计分板和「翻译键 + 参数 + 子组件」
//! 展开的反馈；Rust 侧用 `mc-bamboo-game::replay_execute` 在同样的小世界里重放并逐项比较。
//! `--break-self-test` 改坏一项副作用；`--break-group NAME` 按类别改坏一个字段。
//!
//! `execute-oracle`: compare whole `/execute` commands by side effects and
//! feedback. `--break-self-test` deliberately corrupts one effect.

use std::process::Command;

use serde_json::Value as Json;

const USAGE: &str =
    "usage: execute-oracle <java> [--show N] [--break-self-test] [--break-group NAME]";

/// 一类改坏确认：名字、命中这类场景的判定、要改坏的字段下标。
type BreakGroup = (&'static str, fn(&str) -> bool, usize);

/// 每类改坏确认：把这一类的一个字段按位翻转，确认对拍会失败。
const BREAK_GROUPS: &[BreakGroup] = &[
    ("run", |line| line.contains("run setblock"), 0),
    (
        "conditions",
        |line| line.contains(" if ") || line.contains(" unless "),
        4,
    ),
    ("as", |line| line.contains(" as "), 0),
    (
        "at",
        |line| line.contains(" at ") || line.contains(" at @s"),
        1,
    ),
    ("positioned", |line| line.contains(" positioned "), 0),
    ("rotated", |line| line.contains(" rotated "), 0),
    ("facing", |line| line.contains(" facing "), 0),
    ("align", |line| line.contains(" align "), 0),
    ("anchored", |line| line.contains(" anchored "), 0),
    ("blocks", |line| line.contains(" blocks "), 0),
    ("store", |line| line.contains(" store "), 3),
];

/// Java 和 Rust 都按这个脚本跑；每条命令从同一份干净场景开始。
const SCENARIOS: &[&str] = &[
    "execute positioned 4 -60 6 run setblock 4 -60 6 stone",
    "execute if block 4 -60 6 stone run setblock 4 -60 6 stone",
    "execute unless block 4 -60 6 stone run setblock 4 -60 6 stone",
    "execute if block 2 -60 2 stone run setblock 4 -60 6 stone",
    "execute unless block 2 -60 2 stone run setblock 4 -60 6 stone",
    "execute if entity @e[type=cow] run setblock 4 -60 6 stone",
    "execute unless entity @e[type=cow] run setblock 4 -60 6 stone",
    "execute unless entity @e[type=slime] run setblock 4 -60 6 stone",
    "execute as @e[type=cow] at @s run setblock ~ ~ ~ stone",
    "execute at @e[type=cow] run setblock ~ ~ ~ stone",
    "execute as @e[type=cow,sort=nearest,limit=2] at @s run setblock ~ ~ ~ stone",
    "execute as @e[type=cow,sort=nearest,limit=1] at @s run setblock 4 -60 6 stone",
    "execute as @e[type=cow,sort=furthest,limit=1] at @s run setblock 4 -60 6 stone",
    "execute as @s at @s run setblock 4 -60 6 stone",
    "execute positioned 2 -60 2 run setblock ~ ~ ~ stone",
    "execute positioned as @e[type=cow,limit=1,sort=nearest] run setblock 4 -60 6 stone",
    "execute rotated 90 0 run setblock 4 -60 6 stone",
    "execute rotated as @s run setblock 4 -60 6 stone",
    "execute facing 4 -60 6 run setblock 4 -60 6 stone",
    "execute facing entity @e[type=cow,limit=1,sort=nearest] eyes run setblock 4 -60 6 stone",
    "execute facing entity @e[type=cow,limit=1,sort=nearest] feet run setblock 4 -60 6 stone",
    "execute align xyz run setblock 4 -60 6 stone",
    "execute anchored eyes run setblock 4 -60 6 stone",
    "execute anchored feet run setblock 4 -60 6 stone",
    "execute if blocks 2 -60 2 2 -60 2 6 -60 6 all run setblock 4 -60 6 stone",
    "execute unless blocks 2 -60 2 2 -60 2 6 -60 6 masked run setblock 4 -60 6 stone",
    "execute store result score #total result run setblock 4 -60 6 stone",
    "execute store success score #total result run setblock 4 -60 6 stone",
    "execute as @e[type=cow] at @s store success score #total result run setblock ~ ~ ~ stone",
    "execute as @e[type=cow] at @s store result score #total result run setblock ~ ~ ~ stone",
    "execute if score #total result matches 0 run setblock 4 -60 6 stone",
    "execute unless score #total result matches 0 run setblock 4 -60 6 stone",
    "execute if score #total result = #total result run setblock 4 -60 6 stone",
    "execute if block 4 -60 6 stone store success score #total result run setblock 4 -60 6 stone",
    "execute unless block 4 -60 6 stone store result score #total result run setblock 4 -60 6 stone",
    "data merge storage minecraft:test {Value:7}",
    "data get storage minecraft:test Value",
    "data remove storage minecraft:test Value",
    "data modify storage minecraft:test Other set value 3",
    "execute if data storage minecraft:test Value run setblock 4 -60 6 stone",
    "execute unless data storage minecraft:test Missing run setblock 4 -60 6 stone",
    "execute store result storage minecraft:test Value int 1 run setblock 4 -60 6 stone",
];

#[derive(Debug)]
struct Options {
    show: usize,
    break_self_test: bool,
    break_group: Option<String>,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            show: 8,
            break_self_test: false,
            break_group: None,
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--show" => {
                    options.show = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?;
                }
                "--break-self-test" => options.break_self_test = true,
                "--break-group" => {
                    options.break_group = Some((*args.next().ok_or(USAGE)?).to_owned());
                }
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
                if let Some(group) = &options.break_group
                    && let Some((_, matches, field)) =
                        BREAK_GROUPS.iter().find(|(name, _, _)| name == group)
                    && matches(input)
                {
                    // 故意改坏这一类的一个字段：正常比较会在下面报出差异。
                    ours[*field] = ours[*field].wrapping_add(1);
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
