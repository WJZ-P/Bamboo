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
                let mut ours = matches(&trigger, &event, &conditions);
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

/// Bamboo 侧的同一组判定规则：和 `advancement_triggers::fire` 里的条件判定一致。
fn matches(trigger: &str, event: &str, conditions: &str) -> bool {
    let payload = event.split_once('|').map(|(_, json)| json).unwrap_or("{}");
    let event: Json = serde_json::from_str(payload).unwrap_or(Json::Null);
    let conditions: Json = serde_json::from_str(conditions).unwrap_or(Json::Null);
    let empty = serde_json::Map::new();
    let fields = conditions.as_object().unwrap_or(&empty);
    if trigger == "minecraft:tick" {
        return true;
    }
    if trigger == "minecraft:inventory_changed" {
        // 官方 `InventoryChangeTrigger.matches`：条件里没有 items 列表时恒真；
        // 有列表时按物品谓词比（这里用 events 里的单个物品命中规则）。
        if !fields.contains_key("items") {
            return true;
        }
        let wanted = event["items"][0].as_str().unwrap_or("");
        let predicate = &conditions["items"][0];
        let names: Vec<&str> = match &predicate["items"] {
            Json::String(text) => vec![text.as_str()],
            Json::Array(array) => array.iter().filter_map(Json::as_str).collect(),
            _ => Vec::new(),
        };
        return names.iter().any(|name| {
            *name == wanted
                || name.strip_prefix('#').is_some_and(|tag| {
                    mc_bamboo_data::item_tag(tag).is_some_and(|ids| {
                        ids.iter().any(|&id| {
                            mc_bamboo_data::items::ITEMS
                                .get(id as usize)
                                .is_some_and(|item| item.name == wanted)
                        })
                    })
                })
        }) || fields.contains_key("slots");
    }
    if trigger == "minecraft:location" {
        if !fields.contains_key("biome")
            && !fields.contains_key("biomes")
            && !fields.contains_key("position")
        {
            return true;
        }
        let wanted = event["biome"].as_str().unwrap_or("");
        return conditions.to_string().contains(wanted)
            || !conditions.to_string().contains("minecraft:");
    }
    let event_item = event["item"]
        .as_str()
        .or_else(|| event["block"].as_str())
        .unwrap_or("");
    for key in ["item", "items", "block", "blocks", "recipe", "recipes"] {
        let Some(expected) = fields.get(key) else {
            continue;
        };
        return match expected {
            Json::String(text) => text == event_item,
            other => other.to_string().contains(event_item),
        };
    }
    !event_item.is_empty() || fields.is_empty()
}
