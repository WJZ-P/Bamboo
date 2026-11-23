//! `/advancement` 整条命令的官方执行对拍：官方假世界 / 假玩家跑
//! `ExecuteOracle.java advancements`，Bamboo 用同一组输入在纯数据层复放，
//! 比较改动数、反馈键和执行后的完成进度集合。不启动真实服务端、不接受 EULA。

use std::process::Command;

use serde_json::Value as Json;

const USAGE: &str = "usage: advancement-command-oracle <java> [--show N] [--break-self-test]";

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
    command.arg("advancements");
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
            Some("advancement") => {
                let input = text_field(&value, "input")?;
                let feedback = text_field(&value, "feedback")?;
                let done = text_field(&value, "done")?;
                let (count, key, completed) = mc_bamboo_game::replay_advancement_command(&input);
                // 官方反馈是「翻译键 + 参数」展开；只要键一致就对上，
                // 具体参数由 `command::feedback` 的翻译组件负责。
                let official_key = first_translation_key(&feedback);
                let mut ours_key = key.clone();
                let mut ours_done = completed.clone();
                if options.break_self_test && cases == 0 {
                    ours_key.push_str(".broken");
                    ours_done.push_str("broken");
                }
                cases += 1;
                if !official_key.is_empty() && !ours_key.contains(official_key) {
                    mismatches.push(format!(
                        "{input}: feedback java {official_key}, rust {ours_key}"
                    ));
                }
                if done != ours_done {
                    mismatches.push(format!("{input}: done java {done}, rust {ours_done}"));
                }
                if count == 0 && !ours_key.contains("failure") && !ours_key.contains("NotFound") {
                    mismatches.push(format!("{input}: zero changes without a failure key"));
                }
            }
            Some("end") => {}
            other => return Err(format!("unexpected oracle line type {other:?}")),
        }
    }
    if cases < 12 {
        return Err(format!("oracle emitted only {cases} advancement cases"));
    }
    for mismatch in mismatches.iter().take(options.show) {
        println!("{mismatch}");
    }
    if mismatches.is_empty() {
        println!("advancement-command-oracle: {cases} commands, feedback and progress matched");
        Ok(())
    } else {
        Err(format!(
            "advancement-command-oracle: {} mismatch(es) in {cases} commands",
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

/// `T[key]` 里的第一个翻译键，忽略后面的参数展开。
fn first_translation_key(feedback: &str) -> &str {
    feedback
        .split("T[")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .unwrap_or("")
}
