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
    /// 三种改坏分别被抓到的场景数。
    break_weather: usize,
    break_timing: usize,
    break_initial: usize,
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
    let read = read_lines(stdout, &mut report, options.break_self_test);
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
    read.map_err(|error| format!("{error}\n{log}"))?;
    if options.break_self_test {
        println!(
            "clock-oracle break checks: {} varying scenes caught; weather={} timing={} initial={} ({} differing items)",
            report.scenes,
            report.break_weather,
            report.break_timing,
            report.break_initial,
            report.mismatches
        );
        if report.break_weather == 0 || report.break_timing == 0 || report.break_initial == 0 {
            return Err("a break check was not caught by any scene".to_owned());
        }
        return Ok(());
    }
    println!(
        "clock-oracle: {} scenes, {} checks, {} mismatches",
        report.scenes, report.checks, report.mismatches
    );
    if report.mismatches > 0 {
        return Err(format!("{} mismatches", report.mismatches));
    }
    Ok(())
}

fn read_lines<R: Read>(
    reader: R,
    report: &mut Report,
    break_self_test: bool,
) -> Result<(), String> {
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
        if break_self_test {
            run_break_checks(scene, report)?;
        } else {
            compare_scene(scene, report)?;
        }
        report.scenes += usize::from(
            !break_self_test
                || scene
                    .get("steps")
                    .and_then(Value::as_array)
                    .is_some_and(|steps| {
                        steps.iter().any(|step| {
                            step.get("sky_darken").and_then(Value::as_i64)
                                != steps[0].get("sky_darken").and_then(Value::as_i64)
                                || step
                                    .get("overworld")
                                    .and_then(|c| c.get("total_ticks"))
                                    .and_then(Value::as_i64)
                                    != steps[0]
                                        .get("overworld")
                                        .and_then(|c| c.get("total_ticks"))
                                        .and_then(Value::as_i64)
                        })
                    }),
        );
    }
    Ok(())
}

/// 官方答案不动，把 Bamboo 的三处实现分别改坏，逐刻比同一份官方输出；
/// 每处都必须报出不一致，否则说明对拍没有覆盖到那部分逻辑。
///
/// Keep the official answers and mutate three parts of Bamboo's replay; each
/// must differ from official at some tick, or the oracle is not covering it.
fn run_break_checks(scene: &Value, report: &mut Report) -> Result<(), String> {
    // 稳定晴天 / 冻结时钟这类场景本来就没有会变的值，改坏也不会产生不一致；
    // 只要求三种改坏在「有变化」的场景里被抓到（由 `run` 汇总报告）。
    // Scenes with constant weather and a frozen clock have nothing to change;
    // require each mutation to be caught in at least one varying scene.
    let varying = scene
        .get("steps")
        .and_then(Value::as_array)
        .is_some_and(|steps| {
            steps.iter().any(|step| {
                step.get("sky_darken").and_then(Value::as_i64)
                    != steps[0].get("sky_darken").and_then(Value::as_i64)
                    || step
                        .get("overworld")
                        .and_then(|c| c.get("total_ticks"))
                        .and_then(Value::as_i64)
                        != steps[0]
                            .get("overworld")
                            .and_then(|c| c.get("total_ticks"))
                            .and_then(Value::as_i64)
            })
        });
    if !varying {
        return Ok(());
    }
    let name = scene
        .get("name")
        .and_then(Value::as_str)
        .ok_or("scene without a name")?;

    // 1) 天气贡献：清掉天气层，并把天色写死成 0（旧实现的常量）。
    let mut input = scene_input(scene)?;
    input.o_rain = 0.0;
    input.rain = 0.0;
    input.o_thunder = 0.0;
    input.thunder = 0.0;
    input.rising_rain = false;
    let mut broken = replay_clock_scene(&input);
    for step in &mut broken {
        step.sky_darken = 0;
    }
    let mismatches = count_mismatches(scene, &broken);
    println!("break weather {name}: {mismatches} differing items");
    if mismatches > 0 {
        report.break_weather += 1;
    }
    report.checks += mismatches;
    report.mismatches += mismatches;

    // 2) 更新时机：整份逐刻状态晚一刻发布，模拟在错误阶段算
    // `updateSkyBrightness` / 推进时钟。天气稳定的场景里天色不变，只有时钟刻数能
    // 暴露这个错误，所以时钟状态和天色一起错位。
    let mut input = scene_input(scene)?;
    input.jumps.clear();
    let good = replay_clock_scene(&input);
    let mut broken = good.clone();
    for index in (1..broken.len()).rev() {
        broken[index] = good[index - 1].clone();
        broken[index].tick = good[index].tick;
    }
    let mismatches = count_mismatches(scene, &broken);
    println!("break timing {name}: {mismatches} differing items");
    if mismatches > 0 {
        report.break_timing += 1;
    }
    report.checks += mismatches;
    report.mismatches += mismatches;

    // 3) 时钟初值：回到旧的「停在正午且暂停」特例。
    let mut input = scene_input(scene)?;
    input.start_ticks = 6_000;
    input.paused = true;
    input.jumps.clear();
    let broken = replay_clock_scene(&input);
    let mismatches = count_mismatches(scene, &broken);
    println!("break initial {name}: {mismatches} differing items");
    if mismatches > 0 {
        report.break_initial += 1;
    }
    report.checks += mismatches;
    report.mismatches += mismatches;
    Ok(())
}

/// 官方逐刻答案与一份（可能被改坏的）重放结果之间有多少项不同。
fn count_mismatches(scene: &Value, actual: &[ClockReplayStep]) -> usize {
    let Some(steps) = scene.get("steps").and_then(Value::as_array) else {
        return 1;
    };
    let mut mismatches = 0;
    for (official, bamboo) in steps.iter().zip(actual) {
        let state = official.get("overworld").unwrap_or(&Value::Null);
        mismatches += usize::from(clock_differs(
            state,
            bamboo.overworld_ticks,
            bamboo.overworld_partial,
            bamboo.overworld_rate,
            bamboo.overworld_paused,
        ));
        let state = official.get("the_end").unwrap_or(&Value::Null);
        mismatches += usize::from(clock_differs(
            state,
            bamboo.end_ticks,
            bamboo.end_partial,
            bamboo.end_rate,
            bamboo.end_paused,
        ));
        mismatches += usize::from(
            official.get("sky_darken").and_then(Value::as_i64)
                != Some(i64::from(bamboo.sky_darken)),
        );
    }
    mismatches.saturating_add(steps.len().saturating_sub(actual.len()))
}

fn clock_differs(state: &Value, ticks: i64, partial: f32, rate: f32, paused: bool) -> bool {
    state.get("total_ticks").and_then(Value::as_i64) != Some(ticks)
        || state.get("partial_tick").and_then(Value::as_f64) != Some(f64::from(partial))
        || state.get("rate").and_then(Value::as_f64) != Some(f64::from(rate))
        || state.get("paused").and_then(Value::as_bool) != Some(paused)
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
    let actual = replay_clock_scene(&scene_input(scene)?);
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

fn scene_input(scene: &Value) -> Result<ClockReplayScene, String> {
    Ok(ClockReplayScene {
        name: scene
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        ticks: scene
            .get("steps")
            .and_then(Value::as_array)
            .map(|steps| steps.len() as i64 - 1)
            .unwrap_or(0),
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
        rising_rain: scene
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name == "rising_rain"),
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
    })
}

fn number(scene: &Value, key: &str) -> Result<f32, String> {
    scene
        .get(key)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .ok_or_else(|| format!("scene field {key} is not a number"))
}
