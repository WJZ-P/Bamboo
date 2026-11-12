//! `mc-bamboo-vanilla saved-data <java 路径>`：把 Bamboo 编码器写出的
//! `game_rules.dat` / `world_border.dat` 交给官方 `GameRuleMap.CODEC` /
//! `WorldBorder.Settings.CODEC` 解码，逐项核对；再故意写坏一个键，确认它会报错。
//!
//! Compare Bamboo's `game_rules.dat` / `world_border.dat` against the official
//! codecs: decode them with the official classes and check the values, then
//! deliberately break one key and confirm the oracle rejects it.

use std::{fs, path::Path};

use mc_bamboo_nbt::Tag;
use mc_bamboo_storage::world_dir::{WorldDir, write_saved_data_at};
use serde::Deserialize;
use serde_json::{Value, json};

const SOURCE: &str = "tools/mc-bamboo-vanilla/java/SavedDataOracle.java";

#[derive(Debug, Deserialize)]
struct Report {
    game_rules: Vec<RuleEntry>,
    world_border: BorderReport,
}

#[derive(Debug, Deserialize)]
struct RuleEntry {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    value: Value,
}

#[derive(Debug, Deserialize)]
struct BorderReport {
    center_x: f64,
    center_z: f64,
    damage_per_block: f64,
    safe_zone: f64,
    warning_blocks: i32,
    warning_time: i32,
    size: f64,
    lerp_time: i64,
    lerp_target: f64,
}

fn decode(java: &str, game_rules: &Path, world_border: &Path) -> Result<Report, String> {
    let output = crate::official_program(java, SOURCE)?
        .arg(game_rules)
        .arg(world_border)
        .output()
        .map_err(|error| format!("cannot run java: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "official codec rejected the files: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|error| format!("official oracle stdout is not UTF-8: {error}"))?;
    serde_json::from_str(&text).map_err(|error| format!("bad oracle JSON {text:?}: {error}"))
}

fn check_rule(report: &Report, name: &str, kind: &str, value: &Value) -> Result<(), String> {
    let entry = report
        .game_rules
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| format!("official codec did not decode {name}"))?;
    if entry.kind != kind || entry.value != *value {
        return Err(format!(
            "{name}: expected {kind} {value}, got {} {}",
            entry.kind, entry.value
        ));
    }
    Ok(())
}

pub fn run(java: &str) -> Result<(), String> {
    let (game_rules_tag, world_border_tag) = mc_bamboo_game::saved_data_samples();
    let dir = crate::workspace_root().join("target/vanilla/saved-data-check");
    let _ = fs::remove_dir_all(&dir);
    let world = WorldDir::new(&dir);
    world
        .create_layout()
        .map_err(|error| format!("cannot create check dir: {error}"))?;
    let game_rules_path = world.saved_data("minecraft:game_rules");
    let world_border_path = world.world_border_dat();
    write_saved_data_at(&game_rules_path, &game_rules_tag)
        .map_err(|error| format!("cannot write game_rules.dat: {error}"))?;
    write_saved_data_at(&world_border_path, &world_border_tag)
        .map_err(|error| format!("cannot write world_border.dat: {error}"))?;

    // 1. 官方 codec 能解码 Bamboo 写出的文件，且值一致。
    // The official codecs decode Bamboo's files and the values match.
    let report = decode(java, &game_rules_path, &world_border_path)?;
    check_rule(&report, "minecraft:random_tick_speed", "int", &json!(7))?;
    check_rule(&report, "minecraft:fire_damage", "bool", &json!(false))?;
    let b = &report.world_border;
    for (name, got, want) in [
        ("center_x", b.center_x, 12.5),
        ("center_z", b.center_z, -8.0),
        ("damage_per_block", b.damage_per_block, 0.3),
        ("safe_zone", b.safe_zone, 4.0),
        ("size", b.size, 123.0),
        ("lerp_target", b.lerp_target, 200.0),
    ] {
        if got != want {
            return Err(format!("world_border {name}: expected {want}, got {got}"));
        }
    }
    if b.warning_blocks != 7 || b.warning_time != 42 || b.lerp_time != 99 {
        return Err(format!(
            "world_border int fields differ: blocks={} time={} lerp={}",
            b.warning_blocks, b.warning_time, b.lerp_time
        ));
    }

    // 2. 故意写坏一个键名，官方 codec 必须报错（非 0 退出）。
    // Deliberately break a key; the official codec must reject it.
    let Tag::Compound(entries) = &world_border_tag else {
        return Err("world_border sample is not a compound".to_owned());
    };
    let renamed: Vec<(String, Tag)> = entries
        .iter()
        .map(|(key, value)| {
            let key = if key == "center_x" {
                "center_x_BROKEN".to_owned()
            } else {
                key.clone()
            };
            (key, value.clone())
        })
        .collect();
    write_saved_data_at(&world_border_path, &Tag::Compound(renamed))
        .map_err(|error| format!("cannot write broken world_border.dat: {error}"))?;
    if decode(java, &game_rules_path, &world_border_path).is_ok() {
        return Err("official codec accepted a world_border.dat with a broken key".to_owned());
    }

    let _ = fs::remove_dir_all(&dir);
    println!(
        "SAVED-DATA RESULT: game_rules.dat and world_border.dat match the official codecs; broken key rejected"
    );
    Ok(())
}
