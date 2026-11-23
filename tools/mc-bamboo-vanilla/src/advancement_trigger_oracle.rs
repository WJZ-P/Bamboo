//! 进度触发器条件判定对拍：官方 `Criterion.CODEC` 解析出的真实触发器实例
//! 在固定事件输入下的判定，和 Bamboo 的判定逐项比较。

use std::process::Command;

use serde_json::Value as Json;

const USAGE: &str = "usage: advancement-trigger-oracle <java> [--show N] [--break-self-test]";

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
            "AdvancementTriggerOracle.java failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|error| format!("stdout is not UTF-8: {error}"))?;
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
            Some("trigger") => {
                let id = text_field(&value, "id")?;
                let criterion = text_field(&value, "criterion")?;
                let trigger = text_field(&value, "trigger")?;
                let event = text_field(&value, "event")?;
                let conditions = text_field(&value, "conditions")?;
                let official = value
                    .get("official")
                    .and_then(Json::as_bool)
                    .ok_or("trigger line missing official")?;
                let mut ours = mc_bamboo_game::advancement_triggers::matches_trigger(
                    &trigger, &event, &conditions,
                );
                if options.break_self_test && cases == 0 {
                    ours = !ours;
                }
                cases += 1;
                if ours != official {
                    mismatches.push(format!(
                        "{id}/{criterion} {trigger} {event}: java {official}, rust {ours}"
                    ));
                }
            }
            Some("end") => {}
            other => return Err(format!("unexpected oracle line type {other:?}")),
        }
    }
    if cases < 1000 {
        return Err(format!("oracle emitted only {cases} trigger cases"));
    }
    for mismatch in mismatches.iter().take(options.show) {
        println!("{mismatch}");
    }
    if mismatches.is_empty() {
        println!("advancement-trigger-oracle: {cases} decisions matched");
        Ok(())
    } else {
        Err(format!(
            "advancement-trigger-oracle: {} mismatch(es) in {cases} decisions",
            mismatches.len()
        ))
    }
}

fn text_field(value: &Json, name: &str) -> Result<String, String> {
    value
        .get(name)
        .and_then(Json::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("line missing {name}"))
}
