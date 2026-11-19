//! `mc-bamboo-vanilla saved-data <java 路径>`：把 Bamboo 编码器写出的
//! `game_rules.dat` / `world_border.dat` / `world_clocks.dat` /
//! `world_gen_settings.dat` 交给官方 `GameRuleMap.CODEC` /
//! `WorldBorder.Settings.CODEC` / `PackedClockStates.CODEC` /
//! `WorldGenSettings.CODEC` 解码，逐项核对；超平坦和三种噪声世界的
//! `world_gen_settings.dat` 都复核，再故意写坏一个键、一个噪声生成器类型，
//! 确认官方 codec 会报错。
//!
//! Compare Bamboo's `game_rules.dat` / `world_border.dat` / `world_clocks.dat` /
//! `world_gen_settings.dat` against the official codecs: decode them and check
//! the values. Both the flat and the three noise `world_gen_settings.dat` shapes
//! are validated; then deliberately break one key and one noise generator type
//! to confirm the oracle rejects them.

use std::{
    fs,
    path::{Path, PathBuf},
};

use mc_bamboo_game::world_options::{LevelType, WorldGenConfig};
use mc_bamboo_nbt::Tag;
use mc_bamboo_storage::world_dir::{WorldDir, write_saved_data_at};
use serde::Deserialize;
use serde_json::{Value, json};

const SOURCE: &str = "tools/mc-bamboo-vanilla/java/SavedDataOracle.java";

#[derive(Debug, Deserialize)]
struct Report {
    game_rules: Vec<RuleEntry>,
    world_border: BorderReport,
    world_clocks: Vec<ClockEntry>,
    world_gen_settings: GenReport,
    noise_generators: Vec<NoiseGenReport>,
}

#[derive(Debug, Deserialize)]
struct GenReport {
    layers: Vec<GenLayer>,
    biome: String,
}

/// 官方 `WorldGenSettings.CODEC` 解出的 `minecraft:noise` 生成器形状。
/// Shape of a `minecraft:noise` generator decoded by the official codec.
#[derive(Debug, Deserialize)]
struct NoiseGenReport {
    generator: String,
    settings: String,
    biome_source: String,
    preset: String,
    seed: i64,
    generate_structures: bool,
    bonus_chest: bool,
}

#[derive(Debug, Deserialize)]
struct GenLayer {
    block: String,
    height: i32,
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

/// 官方 `ClockState.CODEC` 解出的一项世界时钟。
/// One world clock decoded by the official `ClockState.CODEC`.
#[derive(Debug, Deserialize)]
struct ClockEntry {
    name: String,
    total_ticks: i64,
    partial_tick: f32,
    rate: f32,
    paused: bool,
}

fn decode(
    java: &str,
    game_rules: &Path,
    world_border: &Path,
    world_clocks: &Path,
    world_gen_settings: &Path,
    noise_files: &[&Path],
) -> Result<Report, String> {
    let mut command = crate::official_program(java, SOURCE)?;
    command
        .arg(game_rules)
        .arg(world_border)
        .arg(world_clocks)
        .arg(world_gen_settings);
    for path in noise_files {
        command.arg(path);
    }
    let output = command
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
    let (game_rules_tag, world_border_tag, world_clocks_tag) = mc_bamboo_game::saved_data_samples();
    let dir = crate::workspace_root().join("target/vanilla/saved-data-check");
    let _ = fs::remove_dir_all(&dir);
    let world = WorldDir::new(&dir);
    world
        .create_layout()
        .map_err(|error| format!("cannot create check dir: {error}"))?;
    let game_rules_path = world.saved_data("minecraft:game_rules");
    let world_border_path = world.world_border_dat();
    let world_clocks_path = world.saved_data("minecraft:world_clocks");
    let world_gen_path = world.saved_data("minecraft:world_gen_settings");
    write_saved_data_at(&game_rules_path, &game_rules_tag)
        .map_err(|error| format!("cannot write game_rules.dat: {error}"))?;
    write_saved_data_at(&world_border_path, &world_border_tag)
        .map_err(|error| format!("cannot write world_border.dat: {error}"))?;
    write_saved_data_at(&world_clocks_path, &world_clocks_tag)
        .map_err(|error| format!("cannot write world_clocks.dat: {error}"))?;
    let world_gen_tag = mc_bamboo_nbt::parse_snbt(mc_bamboo_data::saved_world::WORLD_GEN_SETTINGS)
        .map_err(|error| format!("cannot parse generated world_gen_settings SNBT: {error}"))?;
    write_saved_data_at(&world_gen_path, &world_gen_tag)
        .map_err(|error| format!("cannot write world_gen_settings.dat: {error}"))?;

    // 三种噪声世界各写一份 world_gen_settings.dat，交给官方 WorldGenSettings.CODEC 复核形状。
    // Write one world_gen_settings.dat per noise world for the official codec to validate.
    let noise_cases: Vec<(WorldGenConfig, &str)> = vec![
        (
            WorldGenConfig {
                seed: 12345,
                level_type: LevelType::Normal,
                generate_structures: true,
                ..WorldGenConfig::default()
            },
            "minecraft:overworld",
        ),
        (
            WorldGenConfig {
                seed: -7,
                level_type: LevelType::LargeBiomes,
                generate_structures: true,
                ..WorldGenConfig::default()
            },
            "minecraft:large_biomes",
        ),
        (
            WorldGenConfig {
                seed: 99,
                level_type: LevelType::Amplified,
                generate_structures: false,
                ..WorldGenConfig::default()
            },
            "minecraft:amplified",
        ),
    ];
    let mut noise_paths: Vec<PathBuf> = Vec::with_capacity(noise_cases.len());
    for (index, (config, _)) in noise_cases.iter().enumerate() {
        let path = dir.join(format!("noise_world_gen_settings_{index}.dat"));
        let tag = mc_bamboo_game::world_gen_settings_sample(config);
        write_saved_data_at(&path, &tag)
            .map_err(|error| format!("cannot write noise world_gen_settings.dat: {error}"))?;
        noise_paths.push(path);
    }
    let noise_refs: Vec<&Path> = noise_paths.iter().map(PathBuf::as_path).collect();

    // 1. 官方 codec 能解码 Bamboo 写出的文件，且值一致。
    // The official codecs decode Bamboo's files and the values match.
    let report = decode(
        java,
        &game_rules_path,
        &world_border_path,
        &world_clocks_path,
        &world_gen_path,
        &noise_refs,
    )?;
    check_rule(&report, "minecraft:random_tick_speed", "int", &json!(7))?;
    check_rule(&report, "minecraft:fire_damage", "bool", &json!(false))?;
    // 两份世界时钟的四个字段都要和 Bamboo 编码器写出的值一致。
    // All four fields of both world clocks must match Bamboo's encoder.
    let expected_clocks = [
        // 官方 `ServerClockInstance` 的缺省是 running；旧的正午暂停特例已去掉。
        // Official `ServerClockInstance` defaults to running; the old noon-paused
        // special case is gone.
        ("minecraft:overworld", 12_345_i64, 0.0_f32, 2.5_f32, false),
        ("minecraft:the_end", 678, 0.0, 1.0, false),
    ];
    if report.world_clocks.len() != expected_clocks.len() {
        return Err(format!(
            "expected {} world clocks, got {}",
            expected_clocks.len(),
            report.world_clocks.len()
        ));
    }
    for (name, total, partial, rate, paused) in expected_clocks {
        let clock = report
            .world_clocks
            .iter()
            .find(|clock| clock.name == name)
            .ok_or_else(|| format!("official codec did not decode {name}"))?;
        if clock.total_ticks != total
            || clock.partial_tick != partial
            || clock.rate != rate
            || clock.paused != paused
        {
            return Err(format!("world_clocks {name} differs: {clock:?}"));
        }
    }
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
    // 官方 codec 解出的超平坦层要和 Bamboo 的模板逐项一致，
    // 这样官方接着生成的新区块和 Bamboo 的模板相同。
    let expected: Vec<(String, i32)> = mc_bamboo_world::flat::archive_layers()
        .into_iter()
        .map(|(name, height)| (name.to_owned(), height))
        .collect();
    let actual: Vec<(String, i32)> = report
        .world_gen_settings
        .layers
        .iter()
        .map(|layer| (layer.block.clone(), layer.height))
        .collect();
    if actual != expected {
        return Err(format!(
            "flat layers differ: official {actual:?}, Bamboo {expected:?}"
        ));
    }
    if report.world_gen_settings.biome != "minecraft:plains" {
        return Err(format!(
            "flat biome is {}, expected minecraft:plains",
            report.world_gen_settings.biome
        ));
    }
    if b.warning_blocks != 7 || b.warning_time != 42 || b.lerp_time != 99 {
        return Err(format!(
            "world_border int fields differ: blocks={} time={} lerp={}",
            b.warning_blocks, b.warning_time, b.lerp_time
        ));
    }
    // 官方 codec 解出的三种噪声生成器形状要和 Bamboo 的编码逐项一致。
    // The decoded noise generator shapes must match Bamboo's encoding field by field.
    if report.noise_generators.len() != noise_cases.len() {
        return Err(format!(
            "expected {} noise generators, got {}",
            noise_cases.len(),
            report.noise_generators.len()
        ));
    }
    for ((config, settings), got) in noise_cases.iter().zip(&report.noise_generators) {
        if got.generator != "minecraft:noise"
            || got.settings != *settings
            || got.biome_source != "minecraft:multi_noise"
            || got.preset != "minecraft:overworld"
        {
            return Err(format!("noise generator shape differs: {got:?}"));
        }
        if got.seed != config.seed
            || got.generate_structures != config.generate_structures
            || got.bonus_chest
        {
            return Err(format!(
                "noise world options differ for {settings}: {got:?}"
            ));
        }
    }

    // 2. 故意把一份噪声 world_gen_settings.dat 的 generator type 改坏，官方 codec 必须报错。
    // Deliberately corrupt a noise generator type; the official codec must reject it.
    let broken_noise_path = dir.join("noise_world_gen_settings_broken.dat");
    let broken = corrupt_generator_type(&mc_bamboo_game::world_gen_settings_sample(
        &noise_cases[0].0,
    ));
    write_saved_data_at(&broken_noise_path, &broken)
        .map_err(|error| format!("cannot write broken noise world_gen_settings.dat: {error}"))?;
    if decode(
        java,
        &game_rules_path,
        &world_border_path,
        &world_clocks_path,
        &world_gen_path,
        &[broken_noise_path.as_path()],
    )
    .is_ok()
    {
        return Err(
            "official codec accepted a noise world_gen_settings.dat with a broken generator type"
                .to_owned(),
        );
    }

    // 3. 故意写坏一个键名，官方 codec 必须报错（非 0 退出）。
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
    if decode(
        java,
        &game_rules_path,
        &world_border_path,
        &world_clocks_path,
        &world_gen_path,
        &[],
    )
    .is_ok()
    {
        return Err("official codec accepted a world_border.dat with a broken key".to_owned());
    }

    // 4. 故意写坏一份 world_clocks.dat：必填的 total_ticks 挪成错的键名，
    // 官方 `ClockState.CODEC` 必须拒绝整个文件。
    // 4. Deliberately break world_clocks.dat: rename the required total_ticks
    // key; the official ClockState.CODEC must reject the whole file.
    let Tag::Compound(entries) = &world_clocks_tag else {
        return Err("world_clocks sample is not a compound".to_owned());
    };
    let broken_clocks = Tag::Compound(
        entries
            .iter()
            .map(|(name, value)| {
                let value = match value {
                    Tag::Compound(fields) => Tag::Compound(
                        fields
                            .iter()
                            .map(|(key, field)| {
                                let key = if key == "total_ticks" {
                                    "total_ticks_BROKEN".to_owned()
                                } else {
                                    key.clone()
                                };
                                (key, field.clone())
                            })
                            .collect(),
                    ),
                    other => other.clone(),
                };
                (name.clone(), value)
            })
            .collect(),
    );
    write_saved_data_at(&world_clocks_path, &broken_clocks)
        .map_err(|error| format!("cannot write broken world_clocks.dat: {error}"))?;
    if decode(
        java,
        &game_rules_path,
        &world_border_path,
        &world_clocks_path,
        &world_gen_path,
        &[],
    )
    .is_ok()
    {
        return Err("official codec accepted a world_clocks.dat with a broken key".to_owned());
    }

    let _ = fs::remove_dir_all(&dir);
    println!(
        "SAVED-DATA RESULT: game_rules.dat, world_border.dat, world_clocks.dat, the flat and three noise world_gen_settings.dat match the official codecs; broken keys and broken noise generator rejected"
    );
    Ok(())
}

/// 把 `data` 复合标签里 `minecraft:noise` 的 generator `type` 换成一个不存在的注册名，
/// 用来做官方 codec 的自检。
/// Replace the `minecraft:noise` generator `type` in a `data` compound with a
/// nonexistent id for the official-codec self-test.
fn corrupt_generator_type(tag: &Tag) -> Tag {
    match tag {
        Tag::Compound(entries) => Tag::Compound(
            entries
                .iter()
                .map(|(key, value)| {
                    let value = if key == "type"
                        && matches!(value, Tag::String(s) if s == "minecraft:noise")
                    {
                        Tag::String("minecraft:bogus_generator".to_owned())
                    } else {
                        corrupt_generator_type(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Tag::List(items) => Tag::List(items.iter().map(corrupt_generator_type).collect()),
        other => other.clone(),
    }
}
