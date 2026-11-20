//! 用官方 26.3 的 `BedRule` / `SleepStatus` / `ClockTimeMarker` / `GameRules`
//! 对拍 Bamboo 睡眠系统的判定与跳早数学。
//!
//! `SleepOracle.java` 在 JVM 里枚举天色、玩家人数、`players_sleeping_percentage`
//! 和时间刻，输出官方的 `isDarkOutside` / `sleepersNeeded` /
//! `areEnoughSleeping` / `canSleepThroughNights` / `resolveTimeToMoveTo`。
//! 这里把同一组输入喂给 `mc-bamboo-game` 的产品实现逐项比较，
//! 并用三种改坏确认覆盖（天色阈值、人数公式、跳早目标）。
//!
//! Compare Bamboo's sleep decisions and morning-skip maths with official 26.3
//! `BedRule` / `SleepStatus` / `ClockTimeMarker` / `GameRules`.

use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_game::{SleepReplayFacts, replay_sleep_facts};
use serde_json::Value;

use crate::oracle::collect_log;

const USAGE: &str = "usage: sleep-oracle <java> [--break-self-test] [--show N]";
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
    checks: usize,
    mismatches: usize,
    shown: usize,
    show: usize,
    /// 三种改坏各自被抓到的次数。
    break_dark: usize,
    break_needed: usize,
    break_wake: usize,
}

impl Report {
    fn compare(&mut self, what: &str, expected: &Value, actual: Value) {
        self.checks += 1;
        if &actual == expected {
            return;
        }
        self.mismatches += 1;
        if self.shown < self.show {
            self.shown += 1;
            eprintln!("{what}: official {expected}, Bamboo {actual}");
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
        return Err(format!("SleepOracle.java failed: {status}\n{log}"));
    }
    read.map_err(|error| format!("{error}\n{log}"))?;
    if options.break_self_test {
        println!(
            "sleep-oracle break checks: dark={} needed={} wake={} ({} differing items)",
            report.break_dark, report.break_needed, report.break_wake, report.mismatches
        );
        if report.break_dark == 0 || report.break_needed == 0 || report.break_wake == 0 {
            return Err("a sleep break check was not caught".to_owned());
        }
        return Ok(());
    }
    println!(
        "sleep-oracle: {} checks, {} mismatches",
        report.checks, report.mismatches
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
    for (index, entry) in json
        .get("darkness")
        .and_then(Value::as_array)
        .ok_or("no darkness array")?
        .iter()
        .enumerate()
    {
        let clock_ticks = entry
            .get("clock_ticks")
            .and_then(Value::as_i64)
            .ok_or("darkness without clock_ticks")?;
        let rain = f32::from(u8::from(
            entry.get("rain").and_then(Value::as_bool).unwrap_or(false),
        ));
        let thunder = f32::from(u8::from(
            entry
                .get("thunder")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ));
        let facts = SleepReplayFacts {
            clock_ticks,
            rain,
            thunder,
            ..SleepReplayFacts::default()
        };
        let answer = replay_sleep_facts(&facts);
        let what = format!("darkness[{index}] tick {clock_ticks} rain {rain} thunder {thunder}");
        // 官方 `Level.updateSkyBrightness` 得到的整数天色，和 Bamboo 的 `sky` 模块比对。
        report.compare(
            &format!("{what}.sky_darken"),
            entry.get("sky_darken").unwrap_or(&Value::Null),
            serde_json::json!(answer.sky_darken),
        );
        // 官方 `BedRule.canSleep(level)` → `Level.isDarkOutside()`。
        report.compare(
            &format!("{what}.can_sleep"),
            entry.get("can_sleep").unwrap_or(&Value::Null),
            serde_json::json!(answer.dark_outside),
        );
        report.compare(
            &format!("{what}.dark_outside"),
            entry.get("dark_outside").unwrap_or(&Value::Null),
            serde_json::json!(answer.dark_outside),
        );
        if break_self_test {
            // 改坏：把天气对变暗的贡献整个去掉（假装永远晴天）。
            // 官方 `Level.updateSkyBrightness` 里雨 / 雷暴层会让这些题的天色不同。
            let clear = replay_sleep_facts(&SleepReplayFacts {
                rain: 0.0,
                thunder: 0.0,
                ..facts
            });
            let official_darken = entry.get("sky_darken").and_then(Value::as_i64).unwrap_or(0);
            if clear.sky_darken as i64 != official_darken {
                report.break_dark += 1;
                report.mismatches += 1;
            }
        }
    }
    for entry in json
        .get("sleep_status")
        .and_then(Value::as_array)
        .ok_or("no sleep_status array")?
    {
        let facts = SleepReplayFacts {
            percentage: int(entry, "percentage")?,
            active: int(entry, "active")?,
            sleeping: int(entry, "sleeping")?,
            ..SleepReplayFacts::default()
        };
        let answer = replay_sleep_facts(&facts);
        let what = format!(
            "sleep[{}%, {}/{}]",
            facts.percentage, facts.sleeping, facts.active
        );
        report.compare(
            &format!("{what}.needed"),
            entry.get("needed").unwrap_or(&Value::Null),
            serde_json::json!(answer.needed),
        );
        report.compare(
            &format!("{what}.enough"),
            entry.get("enough").unwrap_or(&Value::Null),
            serde_json::json!(answer.enough),
        );
        report.compare(
            &format!("{what}.can_sleep_through_night"),
            entry.get("can_sleep_through_night").unwrap_or(&Value::Null),
            serde_json::json!(answer.can_sleep_through_night),
        );
        if break_self_test {
            let broken_needed = answer.needed + 1;
            if broken_needed as i64 != entry.get("needed").and_then(Value::as_i64).unwrap_or(0) {
                report.break_needed += 1;
                report.mismatches += 1;
            }
        }
    }
    for entry in json
        .get("wake_up")
        .and_then(Value::as_array)
        .ok_or("no wake_up array")?
    {
        let facts = SleepReplayFacts {
            total_ticks: entry
                .get("total_ticks")
                .and_then(Value::as_i64)
                .ok_or("wake_up without total_ticks")?,
            ..SleepReplayFacts::default()
        };
        let answer = replay_sleep_facts(&facts);
        let what = format!("wake[{}]", facts.total_ticks);
        report.compare(
            &format!("{what}.wake_up_ticks"),
            entry.get("wake_up_ticks").unwrap_or(&Value::Null),
            serde_json::json!(answer.wake_up_ticks),
        );
        report.compare(
            &format!("{what}.occurs_at"),
            entry.get("occurs_at").unwrap_or(&Value::Null),
            serde_json::json!(answer.occurs_at),
        );
        if break_self_test {
            let broken = answer.wake_up_ticks + 1;
            if broken
                != entry
                    .get("wake_up_ticks")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
            {
                report.break_wake += 1;
                report.mismatches += 1;
            }
        }
    }
    Ok(())
}

fn int(entry: &Value, key: &str) -> Result<i32, String> {
    entry
        .get(key)
        .and_then(Value::as_i64)
        .map(|value| value as i32)
        .ok_or_else(|| format!("sleep case has no {key}"))
}
