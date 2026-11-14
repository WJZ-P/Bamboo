//! `gen-world`：官方生成和「游戏里用的那份生成代码」逐区块对拍。
//!
//! `ChunkgenOracle.java --through carvers` 在 JVM 里把一批（种子，区块）跑到
//! `terrain`（CARVERS），输出 quart 群系和方块。Bamboo 侧不直接调 `mc-bamboo-worldgen`，
//! 而是调 `mc-bamboo-game::worldgen::generate_chunk` —— 也就是 `OverworldGenerator`
//! 在游戏里走 `Staged` 流水线时用的同一份纯函数，外加把结果组装成 `RegionColumn`
//! 的那一步。两边逐 quart 格、逐方块比较，0 不一致才算过。
//!
//! 地物、结构、光照还没移植：Java 侧 `--through carvers` 本来就不跑它们，游戏侧
//! 的 `features` / `initialize_light` / `light` / `spawn` 阶段本轮也是透传，两边
//! 用同样的边界（CARVERS 快照）比较，符合 `--through` / `--steps` 的做法。
//! `--level-type` 把 `normal` / `large_biomes` / `amplified` 三种噪声世界逐个对拍。
//!
//! `gen-world`: compare vanilla generation against the SAME code the game runs.
//! `ChunkgenOracle.java --through carvers` emits quart biomes and blocks for a set
//! of (seed, chunk) to `terrain` (CARVERS). Bamboo calls
//! `mc-bamboo-game::worldgen::generate_chunk` — the exact pure function
//! `OverworldGenerator` uses on the `Staged` pipeline — plus the column assembly.
//! Features / structures / light are not ported: the Java side stops at CARVERS
//! and the game's `features` / light / spawn stages are pass-throughs, so both
//! compare the same CARVERS snapshot (the `--through` convention).
//! `--level-type` compares each of `normal` / `large_biomes` / `amplified`.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_data::{block_by_name, registries, version};
use mc_bamboo_game::{
    world_options::{LevelType, WorldGenConfig},
    worldgen::GeneratedChunk,
};
use serde_json::Value;

use crate::oracle::collect_log;

const USAGE: &str = "usage: gen-world <java> [--seeds N] [--chunks N] [--salt S] [--show N] \
     [--level-type normal|large_biomes|amplified] [--break-self-test]";
const DEFAULT_SHOW: usize = 8;

#[derive(Debug)]
struct Options {
    forwarded: Vec<String>,
    show: usize,
    /// 对拍哪种噪声世界；三种共用 overworld 群系预设，只换 NoiseGeneratorSettings。
    /// Which noise world to compare; all three share the overworld biome preset and
    /// differ only in NoiseGeneratorSettings.
    level_type: LevelType,
    /// 故意把 Bamboo 的第一个方块改坏，确认对拍会报不一致（自检）。
    /// Deliberately corrupt the first Bamboo block to verify the oracle fails (self-test).
    break_self_test: bool,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            // 固定跑到 carvers：游戏侧同一阶段也只有 CARVERS 快照。
            // Always go through carvers: the game side only has that snapshot too.
            forwarded: vec!["--through".to_owned(), "carvers".to_owned()],
            show: DEFAULT_SHOW,
            level_type: LevelType::Normal,
            break_self_test: false,
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--seeds" | "--chunks" | "--salt" => {
                    let value = args.next().ok_or(USAGE)?;
                    options
                        .forwarded
                        .extend([arg.to_owned(), (*value).to_owned()]);
                }
                "--show" => {
                    options.show = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?;
                }
                "--level-type" => {
                    let value = args.next().ok_or(USAGE)?;
                    let level = LevelType::from_level_id(value)
                        .filter(|l| !l.is_flat())
                        .ok_or(USAGE)?;
                    let settings = level
                        .settings_id()
                        .and_then(|id| id.strip_prefix("minecraft:"))
                        .ok_or(USAGE)?;
                    options.level_type = level;
                    options
                        .forwarded
                        .extend(["--settings".to_owned(), settings.to_owned()]);
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
    chunks: u64,
    biome_cells: u64,
    blocks: u64,
    mismatches: Vec<String>,
}

/// 启动官方进程，一边读一边把收到的线和游戏侧生成结果比较。
/// `command` 是已经指向 `ChunkgenOracle.java` 的命令。
///
/// Launch the official process and compare each line against the in-game generator.
/// `command` already points at `ChunkgenOracle.java`.
pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let mut child = command
        .args(&options.forwarded)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;

    let mut report = Report {
        pending_biomes: HashMap::new(),
        stats: Stats::default(),
        show: options.show,
        level_type: options.level_type,
        break_self_test: options.break_self_test,
        header_ok: false,
        official_checks: 0,
    };
    let read = report.read_lines(BufReader::new(stdout));
    if read.is_err() {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    read.map_err(|error| format!("{error}\n{log}"))?;
    if !status.success() {
        return Err(format!("ChunkgenOracle.java failed: {status}\n{log}"));
    }
    report.finish()
}

struct Report {
    /// `(seed, x, z)` → 该区块的 quart 群系，等对应的 carvers 线到了再一起比。
    /// `(seed, x, z)` → quart biomes, compared when the matching carvers line arrives.
    pending_biomes: HashMap<(i64, i32, i32), Vec<Vec<String>>>,
    stats: Stats,
    show: usize,
    level_type: LevelType,
    break_self_test: bool,
    header_ok: bool,
    official_checks: u64,
}

impl Report {
    fn read_lines(&mut self, source: BufReader<impl Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            let value: Value =
                serde_json::from_str(&line).map_err(|error| format!("bad oracle line: {error}"))?;
            let kind = value
                .get("type")
                .and_then(Value::as_str)
                .ok_or("oracle line has no type")?;
            match kind {
                "header" => self.check_header(&value)?,
                "biomes" => self.on_biomes(&value)?,
                // noise / surface 是 CARVERS 之前的中间快照，游戏侧没有，跳过。
                // noise / surface are pre-CARVERS snapshots the game side lacks; skip.
                "noise" | "surface" => {}
                "carvers" => self.on_carvers(&value)?,
                "end" => {
                    self.official_checks = value.get("checks").and_then(Value::as_u64).unwrap_or(0);
                }
                other => return Err(format!("unexpected oracle line type {other}")),
            }
        }
        if !self.header_ok {
            return Err("oracle did not emit a header".to_owned());
        }
        if !self.pending_biomes.is_empty() {
            return Err(format!(
                "{} biomes lines had no matching carvers line",
                self.pending_biomes.len()
            ));
        }
        Ok(())
    }

    fn check_header(&mut self, value: &Value) -> Result<(), String> {
        let get_str = |key: &str| -> Result<&str, String> {
            value
                .get(key)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("header missing {key}"))
        };
        let get_i64 = |key: &str| -> Result<i64, String> {
            value
                .get(key)
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("header missing {key}"))
        };
        let reported = get_str("version")?;
        let protocol = get_i64("protocol")?;
        if reported != version::GAME_VERSION || protocol != i64::from(version::PROTOCOL_VERSION) {
            return Err(format!(
                "oracle is {reported}/{protocol}, bamboo is {}/{}",
                version::GAME_VERSION,
                version::PROTOCOL_VERSION
            ));
        }
        // 三种噪声世界共用 minecraft:overworld 群系预设，只换 NoiseGeneratorSettings；
        // header 里的 settings 必须等于这次对拍选的世界类型。
        // All three noise worlds share the minecraft:overworld biome preset and differ
        // only in NoiseGeneratorSettings; the header settings must match the level type.
        let settings_id = self
            .level_type
            .settings_id()
            .ok_or("flat has no noise settings")?;
        if get_str("settings")? != settings_id || get_str("preset")? != "minecraft:overworld" {
            return Err(format!(
                "gen-world expects {settings_id} / minecraft:overworld"
            ));
        }
        let min_y = get_i64("minY")?;
        let height = get_i64("height")?;
        let sea_level = get_i64("seaLevel")?;
        let settings = mc_bamboo_worldgen::Worldgen::load()?
            .settings(settings_id)
            .map(|s| (s.min_y, s.height, s.sea_level))
            .ok_or("missing noise settings")?;
        if (min_y, height, sea_level)
            != (
                i64::from(settings.0),
                i64::from(settings.1),
                i64::from(settings.2),
            )
        {
            return Err(format!(
                "settings differ: official {min_y}/{height}/{sea_level}, bamboo {}/{}/{}",
                settings.0, settings.1, settings.2
            ));
        }
        let through = get_i64("through")?;
        if through != 3 {
            return Err(format!(
                "oracle ran through {through}, gen-world needs carvers (3)"
            ));
        }
        self.header_ok = true;
        Ok(())
    }

    fn on_biomes(&mut self, value: &Value) -> Result<(), String> {
        let key = line_key(value)?;
        let sections = value
            .get("sections")
            .and_then(Value::as_array)
            .ok_or("biomes line missing sections")?;
        let mut parsed = Vec::with_capacity(sections.len());
        for section in sections {
            let cells = section.as_array().ok_or("biomes section is not an array")?;
            parsed.push(
                cells
                    .iter()
                    .map(|cell| {
                        cell.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| "biome cell is not a string".to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        self.pending_biomes.insert(key, parsed);
        Ok(())
    }

    fn on_carvers(&mut self, value: &Value) -> Result<(), String> {
        let key = line_key(value)?;
        let (seed, x, z) = key;
        let biomes = self
            .pending_biomes
            .remove(&key)
            .ok_or_else(|| format!("carvers line for ({seed},{x},{z}) has no biomes line"))?;
        let palette = value
            .get("palette")
            .and_then(Value::as_array)
            .ok_or("carvers line missing palette")?;
        let block_indices = value
            .get("blocks")
            .and_then(Value::as_array)
            .ok_or("carvers line missing blocks")?;

        let config = WorldGenConfig {
            seed,
            level_type: self.level_type,
            ..WorldGenConfig::default()
        };
        let mut generated =
            mc_bamboo_game::worldgen::generate_chunk(&config, x, z).map_err(|error| {
                format!("game generation failed for seed {seed} ({x},{z}): {error}")
            })?;
        if self.break_self_test {
            // 自检：把第一层第一个方块改成岩浆，确认下面会报不一致。
            // Self-test: corrupt one block so the comparison must report a mismatch.
            generated.blocks[0][0] = mc_bamboo_data::blocks::LAVA;
        }
        self.stats.chunks += 1;
        compare_biomes(&mut self.stats, seed, x, z, &biomes, &generated);
        compare_blocks(
            &mut self.stats,
            seed,
            x,
            z,
            palette,
            block_indices,
            &generated,
        )?;
        Ok(())
    }

    fn finish(self) -> Result<(), String> {
        let Stats {
            chunks,
            biome_cells,
            blocks,
            mismatches,
        } = self.stats;
        println!(
            "gen-world: {chunks} chunks, {biome_cells} quart cells, {blocks} blocks, \
             {} mismatches, {} official checks",
            mismatches.len(),
            self.official_checks
        );
        if self.break_self_test {
            if mismatches.is_empty() {
                return Err(
                    "self-test broke a block but gen-world reported 0 mismatches; \
                            the detector is broken"
                        .to_owned(),
                );
            }
            println!(
                "gen-world self-test: detector works ({} mismatches from the deliberate break)",
                mismatches.len()
            );
            return Ok(());
        }
        if chunks == 0 {
            return Err("gen-world compared no chunks".to_owned());
        }
        if !mismatches.is_empty() {
            for message in mismatches.iter().take(self.show) {
                eprintln!("  MISMATCH {message}");
            }
            return Err(format!("gen-world found {} mismatches", mismatches.len()));
        }
        println!("gen-world: 0 mismatches");
        Ok(())
    }
}

/// JSON 一行里的 `(seed, x, z)`。
/// `(seed, x, z)` from one JSON line.
fn line_key(value: &Value) -> Result<(i64, i32, i32), String> {
    let get = |key: &str| -> Result<i64, String> {
        value
            .get(key)
            .and_then(Value::as_i64)
            .ok_or_else(|| format!("line missing {key}"))
    };
    Ok((get("seed")?, get("x")? as i32, get("z")? as i32))
}

/// 官方调色板是 `BlockState.toString()`；terrain 只放默认状态，忽略属性即可。
/// Official palette entries are `BlockState.toString()`; terrain uses default
/// states only, so properties can be ignored.
fn state_of(name: &str) -> Option<u32> {
    let body = name.strip_prefix("Block{")?;
    let (id, _) = body.split_once('}')?;
    Some(block_by_name(id)?.default_state)
}

fn compare_biomes(
    stats: &mut Stats,
    seed: i64,
    x: i32,
    z: i32,
    sections: &[Vec<String>],
    generated: &GeneratedChunk,
) {
    if sections.len() != generated.biomes.len() {
        stats.mismatches.push(format!(
            "biomes seed {seed} ({x},{z}): {} sections vs {}",
            sections.len(),
            generated.biomes.len()
        ));
        return;
    }
    for (section, (ours, expected)) in generated.biomes.iter().zip(sections).enumerate() {
        if ours.len() != expected.len() {
            stats.mismatches.push(format!(
                "biomes seed {seed} ({x},{z}) section {section}: {} cells vs {}",
                ours.len(),
                expected.len()
            ));
            continue;
        }
        for (cell, (&id, name)) in ours.iter().zip(expected).enumerate() {
            let ours_name = registries::biome::NAMES
                .get(id as usize)
                .copied()
                .unwrap_or("<unknown>");
            stats.biome_cells += 1;
            if ours_name != name {
                stats.mismatches.push(format!(
                    "biomes seed {seed} ({x},{z}) section {section} cell {cell}: \
                     official {name}, game {ours_name} ({id})"
                ));
            }
        }
    }
}

fn compare_blocks(
    stats: &mut Stats,
    seed: i64,
    x: i32,
    z: i32,
    palette: &[Value],
    indices: &[Value],
    generated: &GeneratedChunk,
) -> Result<(), String> {
    let height = generated.blocks.len();
    if indices.len() != height * 256 {
        stats.mismatches.push(format!(
            "carvers seed {seed} ({x},{z}): {} blocks vs official {}",
            height * 256,
            indices.len()
        ));
        return Ok(());
    }
    let mut states = Vec::with_capacity(palette.len());
    for name in palette {
        let text = name
            .as_str()
            .ok_or("carvers palette entry is not a string")?;
        states.push(state_of(text).ok_or_else(|| format!("unknown official block state {text}"))?);
    }
    for (index, entry) in indices.iter().enumerate() {
        let id = entry
            .as_u64()
            .ok_or("carvers block index is not a number")? as usize;
        if id >= states.len() {
            stats.mismatches.push(format!(
                "carvers seed {seed} ({x},{z}) at {index}: palette index {id} out of range"
            ));
            continue;
        }
        let official = states[id];
        let bamboo = generated.blocks[index / 256][index % 256];
        stats.blocks += 1;
        if official != bamboo {
            stats.mismatches.push(format!(
                "carvers seed {seed} ({x},{z}) y {} x {} z {}: official {official}, game {bamboo}",
                index as i32 / 256 - 64,
                (index % 256) / 16,
                index % 16
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 故意改坏一处：确认逐方块比较会报不一致，而不是假绿。
    /// Deliberately break one block: the comparison must report a mismatch, not a false green.
    #[test]
    fn block_comparator_detects_a_single_wrong_block() {
        let generated = GeneratedChunk {
            blocks: vec![vec![mc_bamboo_data::blocks::STONE; 256]; 1],
            biomes: vec![],
        };
        let palette = vec![Value::String("Block{minecraft:stone}".to_owned())];
        let indices: Vec<Value> = (0..256).map(|_| Value::from(0u64)).collect();
        let mut clean = Stats::default();
        compare_blocks(&mut clean, 1, 0, 0, &palette, &indices, &generated).unwrap();
        assert!(clean.mismatches.is_empty());
        assert_eq!(clean.blocks, 256);

        // 把官方期望改成另一个方块，比较器必须报 256 处不一致。
        // Change the official expectation; the comparator must report 256 mismatches.
        let wrong_palette = vec![Value::String("Block{minecraft:dirt}".to_owned())];
        let mut broken = Stats::default();
        compare_blocks(&mut broken, 1, 0, 0, &wrong_palette, &indices, &generated).unwrap();
        assert_eq!(broken.mismatches.len(), 256);
    }

    #[test]
    fn biome_comparator_detects_a_wrong_id() {
        let generated = GeneratedChunk {
            blocks: vec![],
            biomes: vec![vec![registries::biome::PLAINS as u32; 64]],
        };
        let sections = vec![vec!["minecraft:desert".to_owned(); 64]];
        let mut stats = Stats::default();
        compare_biomes(&mut stats, 7, 2, 3, &sections, &generated);
        assert_eq!(stats.mismatches.len(), 64);
    }

    #[test]
    fn options_force_carvers_and_forward_seed_args() {
        let options = Options::parse(&["--seeds", "2", "--chunks", "4", "--salt", "9"]).unwrap();
        assert!(options.forwarded.contains(&"--through".to_owned()));
        assert!(options.forwarded.contains(&"carvers".to_owned()));
        assert!(options.forwarded.contains(&"2".to_owned()));
        assert!(Options::parse(&["--bogus"]).is_err());
    }

    #[test]
    fn level_type_forwards_the_settings_id() {
        let normal = Options::parse(&[]).unwrap();
        assert_eq!(normal.level_type, LevelType::Normal);
        assert!(!normal.forwarded.contains(&"--settings".to_owned()));
        let large = Options::parse(&["--level-type", "large_biomes"]).unwrap();
        assert_eq!(large.level_type, LevelType::LargeBiomes);
        let forwarded = large
            .forwarded
            .windows(2)
            .map(|w| (w[0].as_str(), w[1].as_str()));
        assert!(
            forwarded
                .into_iter()
                .any(|(k, v)| k == "--settings" && v == "large_biomes")
        );
        let amplified = Options::parse(&["--level-type", "minecraft:amplified"]).unwrap();
        assert_eq!(amplified.level_type, LevelType::Amplified);
        // flat 不是噪声世界，不接受。/ flat is not a noise world, so it is rejected.
        assert!(Options::parse(&["--level-type", "flat"]).is_err());
    }
}
