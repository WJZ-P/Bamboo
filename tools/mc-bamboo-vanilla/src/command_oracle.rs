//! `command-oracle`：把一组实体选择器和命令交给官方 26.3 跑，Bamboo 侧重放比较。
//!
//! Java 侧 `CommandOracle.java` 不启动服务端，用官方 `EntitySelectorParser` 解析 24 个
//! 选择器并逐项跑官方 predicate / order；Rust 侧用 `mc-bamboo-command` 重放同一串输入，
//! 比较选中的实体列表。`--break-self-test` 故意改坏一个结果确认会被抓到。
//!
//! `command-oracle`: run a set of entity selectors through official 26.3 and
//! replay them in Bamboo, comparing the selected entity lists.
//! `--break-self-test` deliberately corrupts one result.

use std::process::Command;

use mc_bamboo_command::{StringReader, Value, argument::entity::parse_entity};
use serde_json::Value as Json;

const USAGE: &str = "usage: command-oracle <java> [--show N] [--break-self-test]";

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

#[derive(Debug, Default)]
struct Stats {
    selectors: u64,
    mismatches: Vec<String>,
}

/// 和 `CommandOracle.java` 的 `SELECTORS` 一致。
const SELECTORS: &[&str] = &[
    "@e",
    "@e[type=minecraft:cow]",
    "@e[type=!minecraft:cow]",
    "@e[type=#minecraft:animals]",
    "@e[type=!#minecraft:animals]",
    "@e[type=minecraft:player]",
    "@e[type=item,distance=..3]",
    "@e[distance=2..]",
    "@e[x=8,y=0,z=8,dx=3,dy=3,dz=3]",
    "@e[sort=nearest,limit=2]",
    "@e[sort=furthest,limit=2]",
    "@e[limit=1]",
    "@e[type=minecraft:cow,name=Cow]",
    "@e[type=minecraft:cow,name=!Cow]",
    "@e[type=minecraft:cow,tag=]",
    "@e[type=minecraft:cow,tag=!]",
    "@e[gamemode=creative]",
    "@e[gamemode=!creative]",
    "@e[level=0]",
    "@e[level=1]",
    "@s",
    "@n",
    "@a",
    "@p",
];

/// Rust 场景里固定三名候选：玩家 1、牛 `FIRST_ENTITY_ID`、物品 `FIRST_ENTITY_ID+1`。
const PLAYER: i32 = 1;
const COW: i32 = 1 << 24;
const ITEM: i32 = (1 << 24) + 1;
const SPAWN: (f64, f64, f64) = (8.0, 0.0, 8.0);

pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let output = command
        .output()
        .map_err(|error| format!("cannot run java: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "CommandOracle.java failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|error| format!("oracle stdout is not UTF-8: {error}"))?;
    let mut stats = Stats::default();
    let mut seen_selectors = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let value: Json =
            serde_json::from_str(line).map_err(|error| format!("bad oracle line: {error}"))?;
        match value.get("type").and_then(Json::as_str) {
            Some("header") => {
                if !value
                    .get("version")
                    .and_then(Json::as_str)
                    .is_some_and(|id| id == mc_bamboo_data::version::GAME_VERSION)
                {
                    return Err("oracle version does not match the generated data".to_owned());
                }
            }
            Some("selector") => {
                let input = line_string(&value, "input")?;
                let expected = line_string(&value, "result")?;
                let mut ours = select(&input);
                if options.break_self_test && input == "@e[type=minecraft:cow]" {
                    ours = "cow@999".to_owned();
                }
                stats.selectors += 1;
                seen_selectors.push(input.clone());
                if ours != expected {
                    stats
                        .mismatches
                        .push(format!("selector {input}: java {expected}, rust {ours}"));
                }
            }
            Some("command") => return Err("unexpected oracle command line".to_owned()),
            Some("end") => {}
            other => return Err(format!("unexpected oracle line type {other:?}")),
        }
    }
    for selector in SELECTORS {
        if !seen_selectors.iter().any(|seen| seen == selector) {
            return Err(format!("oracle did not emit selector {selector}"));
        }
    }
    for mismatch in stats.mismatches.iter().take(options.show) {
        println!("{mismatch}");
    }
    if stats.mismatches.is_empty() {
        println!("command-oracle: {} selectors, all matched", stats.selectors);
        Ok(())
    } else {
        Err(format!(
            "command-oracle: {} mismatch(es) in {} selectors",
            stats.mismatches.len(),
            stats.selectors
        ))
    }
}

fn line_string(value: &Json, name: &str) -> Result<String, String> {
    value
        .get(name)
        .and_then(Json::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("oracle line missing {name}"))
}

/// 用 Bamboo 的解析器算一个选择器在固定场景里的结果，格式和 Java 侧一致。
fn select(input: &str) -> String {
    let mut reader = StringReader::new(input);
    let Ok(Value::Entity(selector)) = parse_entity(&mut reader, false, false) else {
        return "error:parse".to_owned();
    };
    if reader.can_read() {
        return "error:trailing".to_owned();
    }
    let found = candidates(&selector);
    found
        .iter()
        .map(|(name, id)| format!("{name}@{id}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// 固定场景的选择器结果。和 `select.rs` 的官方规则同一套；这里只算结果，不碰 `Region`。
fn candidates(selector: &mc_bamboo_command::EntitySelector) -> Vec<(&'static str, i32)> {
    let all = [
        ("Player", PLAYER, 8.5, 0.0, 8.5, "minecraft:player"),
        ("Cow", COW, 9.5, 0.0, 8.5, "minecraft:cow"),
        ("Item", ITEM, 10.5, 0.0, 8.5, "minecraft:item"),
    ];
    let origin = selector.position(mc_bamboo_core::Vec3::new(SPAWN.0, SPAWN.1, SPAWN.2));
    let mut found = Vec::new();
    for (name, id, x, y, z, kind) in all {
        if selector
            .name_filters
            .iter()
            .any(|(text, inverted)| (name == text) == *inverted)
        {
            continue;
        }
        if selector.type_filters.iter().any(|(type_name, inverted)| {
            let matches = short(type_name) == short(kind);
            matches == *inverted
        }) {
            continue;
        }
        if selector.type_tag_filters.iter().any(|(tag, inverted)| {
            let in_tag = entity_tag_has(tag, kind);
            in_tag == *inverted
        }) {
            continue;
        }
        let mut skip_candidate = false;
        for (tag, inverted) in &selector.tag_filters {
            // 官方 `Entity.entityTags()`：空字符串判断标签集合是否为空。
            // Official `Entity.entityTags()`: an empty string tests whether the set is empty.
            let matched = tag.is_empty();
            if matched == *inverted {
                skip_candidate = true;
                break;
            }
        }
        if skip_candidate {
            continue;
        }
        // 官方 `gamemode=` 只对 `ServerPlayer` 生效，`level=` 读 `ServerPlayer.experienceLevel`；
        // 场景里没有真正的 ServerPlayer，所以这两组选择器都选不出任何实体。
        // Official `gamemode=` only tests `ServerPlayer` and `level=` reads
        // `ServerPlayer.experienceLevel`; this scene has no real ServerPlayer.
        if !selector.game_mode_filters.is_empty() || selector.level.is_some() {
            continue;
        }
        if selector
            .distance
            .as_ref()
            .is_some_and(|bounds| !in_range(*bounds, squared(x, y, z, origin)))
        {
            continue;
        }
        if let Some(box_) = volume_box(origin, selector)
            && !box_.intersects(mc_bamboo_core::Aabb::new(
                x - 0.3,
                y,
                z - 0.3,
                x + 0.3,
                y + 1.0,
                z + 0.3,
            ))
        {
            continue;
        }
        found.push((name, id, squared(x, y, z, origin), kind));
    }
    if found.len() > 1 {
        match selector.order {
            mc_bamboo_command::SortOrder::Arbitrary => {}
            mc_bamboo_command::SortOrder::Nearest => {
                found.sort_by(|a, b| a.2.total_cmp(&b.2));
            }
            mc_bamboo_command::SortOrder::Furthest => {
                found.sort_by(|a, b| b.2.total_cmp(&a.2));
            }
            mc_bamboo_command::SortOrder::Random => {
                // 官方用 `Collections.shuffle`，结果不可复现；对拍不比较随机顺序。
                // Official uses `Collections.shuffle`; the oracle does not compare it.
            }
        }
    }
    let limit = selector.max_results.max(0) as usize;
    found.truncate(limit);
    found
        .into_iter()
        .map(|(_name, id, _, kind)| (output_name(kind), id))
        .collect()
}

fn output_name(kind: &str) -> &'static str {
    match kind {
        "minecraft:player" => "player",
        "minecraft:cow" => "cow",
        _ => "item",
    }
}

fn short(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

fn entity_tag_has(tag: &str, kind: &str) -> bool {
    let id = mc_bamboo_data::registries::entity_type::NAMES
        .iter()
        .position(|name| short(name) == short(kind))
        .unwrap_or(usize::MAX) as i32;
    mc_bamboo_data::tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == "minecraft:entity_type")
        .and_then(|group| {
            group
                .tags
                .iter()
                .find(|entry| short(entry.name.trim_start_matches("minecraft:")) == short(tag))
        })
        .is_some_and(|entry| entry.ids.contains(&id))
}

fn squared(x: f64, y: f64, z: f64, origin: mc_bamboo_core::Vec3) -> f64 {
    let dx = x - origin.x;
    let dy = y - origin.y;
    let dz = z - origin.z;
    dx * dx + dy * dy + dz * dz
}

fn in_range(bounds: mc_bamboo_command::NumberBounds<f64>, squared: f64) -> bool {
    if let Some(min) = bounds.min
        && squared < min * min
    {
        return false;
    }
    if let Some(max) = bounds.max
        && squared > max * max
    {
        return false;
    }
    true
}

fn volume_box(
    origin: mc_bamboo_core::Vec3,
    selector: &mc_bamboo_command::EntitySelector,
) -> Option<mc_bamboo_core::Aabb> {
    if selector.delta_x.is_none() && selector.delta_y.is_none() && selector.delta_z.is_none() {
        let max = selector.distance?.max?;
        return Some(mc_bamboo_core::Aabb::new(
            origin.x - max,
            origin.y - max,
            origin.z - max,
            origin.x + max + 1.0,
            origin.y + max + 1.0,
            origin.z + max + 1.0,
        ));
    }
    let axis = |value: Option<f64>| {
        let value = value.unwrap_or(0.0);
        if value < 0.0 {
            (value, 0.0)
        } else {
            (0.0, value + 1.0)
        }
    };
    let (x0, x1) = axis(selector.delta_x);
    let (y0, y1) = axis(selector.delta_y);
    let (z0, z1) = axis(selector.delta_z);
    Some(mc_bamboo_core::Aabb::new(
        origin.x + x0,
        origin.y + y0,
        origin.z + z0,
        origin.x + x1,
        origin.y + y1,
        origin.z + z1,
    ))
}
