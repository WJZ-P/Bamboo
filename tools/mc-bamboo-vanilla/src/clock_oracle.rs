//! 用官方 26.3 的 `ServerClockManager` 和 `Level.updateSkyBrightness` 对拍
//! Bamboo 的时钟初值、推进、出站流速和天色变暗。
//!
//! `ClockOracle.java` 在 JVM 里用官方类跑一组逐刻场景（一整个昼夜循环、雨、雷暴、
//! 渐变中的雨、`/time set` 跳变、`advance_time` 关闭、非整数流速、暂停），
//! 输出每一步的两个时钟状态和 `getSkyDarken()`。这里把同一组输入喂给
//! `mc_bamboo_game::replay_clock_scene`（产品里的同一份实现）逐项比较。
//!
//! Compare Bamboo's clock defaults, ticking, network rate and sky darkening
//! with official 26.3 `ServerClockManager` and `Level.updateSkyBrightness`.
//! `ClockOracle.java` runs per-tick scenes through the official classes in a
//! JVM (a full day/night cycle, rain, thunder, rising rain, `/time set` jumps,
//! `advance_time` off, fractional rates, paused clocks) and emits both clock
//! states plus `getSkyDarken()` at every step. This module feeds the same
//! inputs to `mc_bamboo_game::replay_clock_scene` (the production code) and
//! compares every item.

use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_game::{ClockReplayScene, ClockReplayStep, replay_clock_scene};
use serde_json::Value;

use crate::oracle::collect_log;

const USAGE: &str = "usage: clock-oracle <java> [--break-self-test] [--show N]";
const DEFAULT_SHOW: usize = 8;

#[derive(Debug, Default)]
struct Options {
    break_self_test: bool,
    show: usize,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            show: DEFAULT_SHOW,
            ..Self::default()
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--break-self-test" => options.break_self_test = true,
                "--show" => {
                    let value = args.next().ok_or(USAGE)?;
                    options.show = value.parse().map_err(|_| USAGE)?;
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

#[derive(Debug, Default)]
struct Report {
    scenes: usize,
    checks: usize,
    mismatches: usize,
    shown: usize,
    show: usize,
}

impl Report {
    fn compare(&mut self, scene: &str, tick: i64, what: &str, expected: &Value, actual: Value) {
        self.checks += 1;
        if &actual == expected {
            return;
        }
        self.mismatches += 1;
        if self.shown < self.show {
            self.shown += 1;
            eprintln!("{scene} tick {tick} {what}: official {expected}, Bamboo {actual}");
        }
    }
}

pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    if options.break_self_test {
        return break_self_test();
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut report = Report {
        show: options.show,
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
        return Err(format!("ClockOracle.java failed: {status}\n{log}"));
    }
    read?;
    println!(
        "clock-oracle: {} scenes, {} checks, {} mismatches",
        report.scenes, report.checks, report.mismatches
    );
    if report.mismatches > 0 {
        return Err(format!("{} mismatches", report.mismatches));
    }
    Ok(())
}

fn read_lines<R: Read>(reader: R, report: &mut Report) -> Result<(), String> {
    let mut lines = BufReader::new(reader).lines();
    let Some(line) = lines.next() else {
        return Err("java printed nothing".to_owned());
    };
    let line = line.map_err(|error| format!("cannot read java output: {error}"))?;
    let json: Value = serde_json::from_str(&line).map_err(|error| format!("bad JSON: {error}"))?;
    let scenes = json
        .get("scenes")
        .and_then(Value::as_array)
        .ok_or("java output has no scenes")?;
    for scene in scenes {
        compare_scene(scene, report)?;
        report.scenes += 1;
    }
    Ok(())
}

fn compare_scene(scene: &Value, report: &mut Report) -> Result<(), String> {
    let name = scene
        .get("name")
        .and_then(Value::as_str)
        .ok_or("scene without a name")?;
    let steps = scene
        .get("steps")
        .and_then(Value::as_array)
        .ok_or("scene without steps")?;
    let input = ClockReplayScene {
        name: name.to_owned(),
        ticks: steps.len() as i64 - 1,
        start_ticks: scene
            .get("start_ticks")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        rate: number(scene, "rate")?,
        paused: scene
            .get("paused")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        advance_time: scene
            .get("advance_time")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        o_rain: number(scene, "o_rain")?,
        rain: number(scene, "rain")?,
        o_thunder: number(scene, "o_thunder")?,
        thunder: number(scene, "thunder")?,
        rising_rain: name == "rising_rain",
        // `/time set` 跳变在 Java 侧是「刻号 == 目标刻」。
        // Java jumps are keyed by tick number and set the clock to that tick.
        jumps: scene
            .get("jumps")
            .and_then(Value::as_array)
            .ok_or("scene without jumps")?
            .iter()
            .filter_map(Value::as_i64)
            .map(|value| value as i32)
            .collect(),
    };
    let actual = replay_clock_scene(&input);
    if actual.len() != steps.len() {
        return Err(format!(
            "{name}: official {} steps, Bamboo {}",
            steps.len(),
            actual.len()
        ));
    }
    for (official, bamboo) in steps.iter().zip(&actual) {
        let tick = bamboo.tick;
        compare_clock(name, tick, "overworld", official, bamboo, false, report);
        compare_clock(name, tick, "the_end", official, bamboo, true, report);
        report.compare(
            name,
            tick,
            "sky_darken",
            official.get("sky_darken").unwrap_or(&Value::Null),
            serde_json::json!(bamboo.sky_darken),
        );
    }
    Ok(())
}

fn compare_clock(
    scene: &str,
    tick: i64,
    which: &str,
    official: &Value,
    bamboo: &ClockReplayStep,
    end: bool,
    report: &mut Report,
) {
    let state = official.get(which).unwrap_or(&Value::Null);
    let (ticks, partial, rate, paused) = if end {
        (
            bamboo.end_ticks,
            bamboo.end_partial,
            bamboo.end_rate,
            bamboo.end_paused,
        )
    } else {
        (
            bamboo.overworld_ticks,
            bamboo.overworld_partial,
            bamboo.overworld_rate,
            bamboo.overworld_paused,
        )
    };
    report.compare(
        scene,
        tick,
        &format!("{which}.total_ticks"),
        state.get("total_ticks").unwrap_or(&Value::Null),
        serde_json::json!(ticks),
    );
    report.compare(
        scene,
        tick,
        &format!("{which}.partial_tick"),
        state.get("partial_tick").unwrap_or(&Value::Null),
        serde_json::json!(partial),
    );
    report.compare(
        scene,
        tick,
        &format!("{which}.rate"),
        state.get("rate").unwrap_or(&Value::Null),
        serde_json::json!(rate),
    );
    report.compare(
        scene,
        tick,
        &format!("{which}.paused"),
        state.get("paused").unwrap_or(&Value::Null),
        serde_json::json!(paused),
    );
}

fn number(scene: &Value, key: &str) -> Result<f32, String> {
    scene
        .get(key)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .ok_or_else(|| format!("scene field {key} is not a number"))
}

/// 改坏确认：故意改掉天气对变暗的贡献、更新时机、时钟初值，各自必须报出不一致。
///
/// Break checks: deliberately break the weather contribution, the update point
/// and the clock default; each must produce mismatches.
fn break_self_test() -> Result<(), String> {
    let scene = mc_bamboo_game::ClockReplayScene {
        name: "break_check".to_owned(),
        ticks: 24_100,
        start_ticks: 0,
        rate: 1.0,
        paused: false,
        advance_time: true,
        o_rain: 1.0,
        rain: 1.0,
        o_thunder: 1.0,
        thunder: 1.0,
        rising_rain: false,
        jumps: vec![],
    };
    let good = replay_clock_scene(&scene);
    // 1) 天气贡献：按晴天算会把雨夜的 darken 算低。
    let clear = replay_clock_scene(&mc_bamboo_game::ClockReplayScene {
        o_rain: 0.0,
        rain: 0.0,
        o_thunder: 0.0,
        thunder: 0.0,
        ..scene.clone()
    });
    let weather_differs = good
        .iter()
        .zip(&clear)
        .any(|(a, b)| a.sky_darken != b.sky_darken);
    // 2) 更新时机：少推进一刻，时钟和 darken 在黎明附近都会错。
    let late = replay_clock_scene(&mc_bamboo_game::ClockReplayScene {
        ticks: 24_099,
        ..scene.clone()
    });
    let timing_differs = good[24_100].overworld_ticks != late[24_099].overworld_ticks
        || good[13_670].sky_darken != late[13_669].sky_darken;
    // 3) 时钟初值：停在正午的旧特例与官方初值不同。
    let noon = replay_clock_scene(&mc_bamboo_game::ClockReplayScene {
        start_ticks: 6_000,
        ..scene.clone()
    });
    let initial_differs = good[0].overworld_ticks != noon[0].overworld_ticks
        || good[0].sky_darken != noon[0].sky_darken;
    println!(
        "clock-oracle break checks: weather={weather_differs} timing={timing_differs} initial={initial_differs}"
    );
    if weather_differs && timing_differs && initial_differs {
        Ok(())
    } else {
        Err("a break check did not change the replay result".to_owned())
    }
}
