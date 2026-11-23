//! 用官方 codec、地形和单中心 3×3 装饰视图逐字段、逐方块对拍超平坦。
//!
//! Compare flat codec fields, terrain and the 3×3 decoration view for one
//! center at a time against vanilla.

use std::process::Command;

use mc_bamboo_data::{block_by_name, block_of_state, registries};
use mc_bamboo_game::world_options::{FlatSettings, LevelType, WorldGenConfig};
use serde_json::Value;

const USAGE: &str = "usage: flat-world <java> [--show N] [--break-self-test]";
const DEFAULT_SHOW: usize = 8;

#[derive(Debug)]
struct Options {
    show: usize,
    break_self_test: bool,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            show: DEFAULT_SHOW,
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
    cases: u64,
    blocks: u64,
    decorated_chunks: usize,
    mismatches: Vec<String>,
}

/// 启动官方 `FlatWorldOracle.java`，逐行比较解析字段和逐方块快照。
/// Launch the official `FlatWorldOracle.java` and compare decoded fields and
/// block snapshots line by line.
pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let output = command
        .output()
        .map_err(|error| format!("cannot run java: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "FlatWorldOracle.java failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|error| format!("oracle stdout is not UTF-8: {error}"))?;

    let mut stats = Stats::default();
    let mut first = true;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value =
            serde_json::from_str(line).map_err(|error| format!("bad oracle line: {error}"))?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or("oracle line has no type")?;
        if kind != "flat" {
            return Err(format!("unexpected oracle line type {kind}"));
        }
        let input = value
            .get("input")
            .and_then(Value::as_str)
            .ok_or("flat line missing input")?;
        let flat = FlatSettings::parse(input).map_err(|error| format!("{error}: {input}"))?;
        stats.cases += 1;
        compare_fields(&mut stats, input, &value, &flat);
        compare_blocks(
            &mut stats,
            input,
            &value,
            &flat,
            options.break_self_test,
            &mut first,
        )?;
        if let Some(case) = value.get("case").and_then(Value::as_str) {
            compare_decoration(&mut stats, case, &value, flat)?;
        }
    }
    finish(stats, &options)
}

/// 比较 CODEC 解出的五个字段和 Bamboo `FlatSettings::parse` 的结果。
/// Compare the five decoded CODEC fields with Bamboo `FlatSettings::parse`.
fn compare_fields(stats: &mut Stats, input: &str, value: &Value, flat: &FlatSettings) {
    let get_bool = |key: &str| value.get(key).and_then(Value::as_bool).unwrap_or(false);
    if flat.features != get_bool("features") {
        stats
            .mismatches
            .push(format!("features differ for {input}"));
    }
    if flat.lakes != get_bool("lakes") {
        stats.mismatches.push(format!("lakes differ for {input}"));
    }
    let want_biome = value
        .get("biome")
        .and_then(Value::as_str)
        .unwrap_or("minecraft:plains");
    let our_biome = registries::biome::NAMES
        .get(flat.biome as usize)
        .copied()
        .unwrap_or("<unknown>");
    if our_biome != want_biome {
        stats.mismatches.push(format!(
            "biome differ for {input}: official {want_biome}, game {our_biome}"
        ));
    }
    let empty: &[Value] = &[];
    let want_layers: Vec<(String, i64)> = value
        .get("layers")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(empty)
        .iter()
        .map(|entry| {
            (
                entry
                    .get("block")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                entry.get("height").and_then(Value::as_i64).unwrap_or(-1),
            )
        })
        .collect();
    let our_layers: Vec<(String, i64)> = flat
        .layers
        .iter()
        .map(|&(state, height)| {
            (
                block_of_state(state)
                    .map(|b| b.name.to_owned())
                    .unwrap_or_else(|| "minecraft:air".to_owned()),
                i64::from(height),
            )
        })
        .collect();
    if our_layers != want_layers {
        stats.mismatches.push(format!(
            "layers differ for {input}: official {want_layers:?}, game {our_layers:?}"
        ));
    }
    let want_overrides: Option<Vec<String>> = value
        .get("structure_overrides")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_owned))
                .collect()
        });
    if flat.structure_overrides != want_overrides {
        stats.mismatches.push(format!(
            "structure_overrides differ for {input}: official {want_overrides:?}, game {:?}",
            flat.structure_overrides
        ));
    }
}

/// 逐方块比较 `FlatLevelSource.buildTerrain` 与 `generate_chunk` 的 CARVERS 快照。
/// Compare the `FlatLevelSource.buildTerrain` and `generate_chunk` blocks per cell.
fn compare_blocks(
    stats: &mut Stats,
    input: &str,
    value: &Value,
    flat: &FlatSettings,
    break_self_test: bool,
    first: &mut bool,
) -> Result<(), String> {
    let palette = value
        .get("palette")
        .and_then(Value::as_array)
        .ok_or("flat line missing palette")?;
    let indices = value
        .get("blocks")
        .and_then(Value::as_array)
        .ok_or("flat line missing blocks")?;
    let mut states = Vec::with_capacity(palette.len());
    for entry in palette {
        let text = entry.as_str().ok_or("flat palette entry is not a string")?;
        states.push(state_of(text).ok_or_else(|| format!("unknown official block state {text}"))?);
    }
    let config = WorldGenConfig {
        level_type: LevelType::Flat,
        flat: flat.clone(),
        ..WorldGenConfig::default()
    };
    let generated = mc_bamboo_game::worldgen::generate_chunk(&config, 0, 0)
        .map_err(|error| format!("game flat generation failed for {input}: {error}"))?;
    if generated
        .biomes
        .iter()
        .flatten()
        .any(|&id| id != flat.biome)
    {
        stats
            .mismatches
            .push(format!("flat biome grid differs for {input}"));
    }
    let height = generated.blocks.len();
    if indices.len() != height * 256 {
        stats.mismatches.push(format!(
            "flat {input}: {} blocks vs official {}",
            height * 256,
            indices.len()
        ));
        return Ok(());
    }
    let mut corrupt = break_self_test && *first;
    *first = false;
    for (index, entry) in indices.iter().enumerate() {
        let id = entry.as_u64().ok_or("flat block index is not a number")? as usize;
        let official = states
            .get(id)
            .copied()
            .ok_or_else(|| format!("flat {input}: palette index {id} out of range"))?;
        let mut bamboo = generated.blocks[index / 256][index % 256];
        if corrupt {
            // 自检：把第一个方块改坏，确认下面会报不一致。
            // Self-test: corrupt the first block so the comparison must fail.
            bamboo = mc_bamboo_data::blocks::LAVA;
            corrupt = false;
        }
        stats.blocks += 1;
        if official != bamboo {
            stats.mismatches.push(format!(
                "flat {input} y {} x {} z {}: official {official}, game {bamboo}",
                index as i32 / 256 - 64,
                (index % 256) / 16,
                index % 16
            ));
        }
    }
    Ok(())
}

fn compare_decoration(
    stats: &mut Stats,
    case: &str,
    row: &Value,
    flat: FlatSettings,
) -> Result<(), String> {
    use mc_bamboo_game::{worldgen_features::game_features, worldgen_structures::structure_starts};
    use mc_bamboo_worldgen::feature::state::state_from_official_name;

    let coordinate = |key| {
        row.get(key)
            .and_then(Value::as_i64)
            .and_then(|v| i32::try_from(v).ok())
            .ok_or_else(|| format!("missing {key} coordinate"))
    };
    let (x, z) = (coordinate("x")?, coordinate("z")?);
    let config = WorldGenConfig {
        flat,
        generate_structures: row["generateStructures"]
            .as_bool()
            .ok_or("missing generateStructures")?,
        ..WorldGenConfig::default()
    };
    let starts = structure_starts(&config, x, z)?.by_structure.len();
    let expected_starts = row["starts"].as_u64().ok_or("missing starts")? as usize;
    let required_starts = usize::from(case == "village");
    if starts != expected_starts || expected_starts != required_starts {
        stats.mismatches.push(format!("flat {case} ({x},{z}) starts: official {expected_starts}, bamboo {starts}, fixture requires {required_starts}"));
    }
    let generated = game_features(&config, x, z, None)?;
    let chunks = row["decorated"]
        .as_array()
        .ok_or("missing decorated chunks")?;
    if chunks.len() != 9 || generated.unsupported {
        return Err(format!(
            "flat {case} ({x},{z}): expected 9 chunks and supported decoration, got {} chunks, unsupported={}",
            chunks.len(),
            generated.unsupported
        ));
    }
    use mc_bamboo_data::blocks;
    let mut witnesses: Vec<(&str, u32, usize)> = match case {
        "village" => vec![("oak_planks", blocks::OAK_PLANKS, 0)],
        "void-platform-and-deferred-layer" => vec![
            ("cobblestone", blocks::COBBLESTONE, 0),
            ("snow", blocks::SNOW, 0),
        ],
        "plains-decoration" => vec![("short_grass", blocks::SHORT_GRASS, 0)],
        "lava-lake" => vec![("lava", blocks::LAVA, 0)],
        _ => Vec::new(),
    };
    for (index, (chunk, bamboo)) in chunks.iter().zip(&generated.blocks).enumerate() {
        let palette = chunk["palette"]
            .as_array()
            .ok_or("missing decoration palette")?;
        let states: Vec<u32> = palette
            .iter()
            .map(|v| state_from_official_name(v.as_str().ok_or("non-string decoration state")?))
            .collect::<Result<_, _>>()?;
        let blocks = chunk["blocks"]
            .as_array()
            .ok_or("missing decoration blocks")?;
        if blocks.len() != bamboo.len() * 256 {
            return Err(format!("flat {case} ({x},{z}): unexpected block count"));
        }
        for (cell, (want, &got)) in blocks.iter().zip(bamboo.iter().flatten()).enumerate() {
            let id = want
                .as_u64()
                .and_then(|id| states.get(id as usize))
                .copied()
                .ok_or("invalid decoration palette index")?;
            for (_, state, count) in &mut witnesses {
                *count += usize::from(*state == id);
            }
            if got != id {
                let bx = (x + index as i32 % 3 - 1) * 16 + (cell as i32 % 256) / 16;
                let bz = (z + index as i32 / 3 - 1) * 16 + cell as i32 % 16;
                let by = cell as i32 / 256 - 64;
                stats.mismatches.push(format!(
                    "flat {case} ({bx},{by},{bz}): official {id}, bamboo {got}"
                ));
            }
        }
        stats.blocks += blocks.len() as u64;
        stats.decorated_chunks += 1;
    }
    let mut coverage = String::new();
    for (name, _, count) in witnesses {
        if count == 0 {
            return Err(format!(
                "flat {case} ({x},{z}): official fixture did not place {name}"
            ));
        }
        coverage.push_str(&format!(", {count} {name}"));
    }
    println!("flat-world {case} ({x},{z}): 9 decorated chunks, {starts} starts{coverage}");
    Ok(())
}

/// 官方调色板是 `BlockState.toString()`；terrain 只放默认状态，忽略属性即可。
/// Official palette entries are `BlockState.toString()`; terrain uses default
/// states only, so properties can be ignored.
fn state_of(name: &str) -> Option<u32> {
    let body = name.strip_prefix("Block{")?;
    let (id, _) = body.split_once('}')?;
    Some(block_by_name(id)?.default_state)
}

fn finish(stats: Stats, options: &Options) -> Result<(), String> {
    let count = stats.mismatches.len();
    println!(
        "flat-world: {} cases, {} decorated chunks, {} blocks, {} mismatches",
        stats.cases, stats.decorated_chunks, stats.blocks, count
    );
    if options.break_self_test {
        if count != 1 {
            for message in stats.mismatches.iter().take(options.show) {
                eprintln!("  MISMATCH {message}");
            }
            return Err(
                "self-test expected exactly one mismatch from the deliberate break".to_owned(),
            );
        }
        println!(
            "flat-world self-test: detector works ({count} mismatches from the deliberate break)"
        );
        return Ok(());
    }
    if stats.cases == 0 {
        return Err("flat-world compared no cases".to_owned());
    }
    if count != 0 {
        for message in stats.mismatches.iter().take(options.show) {
            eprintln!("  MISMATCH {message}");
        }
        return Err(format!("flat-world found {count} mismatches"));
    }
    println!("flat-world: 0 mismatches");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_reject_unknown_flags() {
        assert!(Options::parse(&[]).is_ok());
        assert!(
            Options::parse(&["--break-self-test"])
                .unwrap()
                .break_self_test
        );
        assert!(Options::parse(&["--bogus"]).is_err());
    }

    #[test]
    fn state_of_parses_default_block_state() {
        assert_eq!(
            state_of("Block{minecraft:stone}"),
            Some(mc_bamboo_data::blocks::STONE)
        );
        assert!(state_of("not a block").is_none());
    }
}
