//! `light-pipeline`：整块生成期光照对拍。
//!
//! 官方在 JVM 里把中心 3×3 的每个区块按 `ChunkStatus` 顺序跑到 FEATURES，再用
//! 官方 `LevelLightEngine` 跑 `initializeLight` + `light`（`ChunkgenOracle.java
//! --light-pipeline`），输出 FEATURES 后 3×3 的方块状态 ID 和中心柱逐格天空光 /
//! 方块光。Bamboo 侧拿同一批方块 ID 走和 `gen-light` 一样的光引擎，同时调游戏
//! 流水线 `LIGHT` 阶段的入口（`generate_light_stage_with_input`）算一遍，两组都要
//! 逐格等于官方。前者证明「官方这批方块 → 官方光照」在 Bamboo 引擎里一致，后者
//! 证明「游戏流水线自己挑的方块 → 光照」也一致。
//!
//! `--break-carvers` 把游戏流水线的光照输入故意退回 CARVERS：树林、萤石、灯笼、
//! 结构光源附近必须报不一致，否则这批坐标没覆盖到会改光的地物。
//!
//! `light-pipeline`: whole-chunk generation-time light oracle. Vanilla runs the
//! center 3x3 through FEATURES and then through its official `LevelLightEngine`
//! (`initializeLight` + `light`), emitting the post-FEATURES block state ids and
//! the center column's per-cell sky/block light. Bamboo runs its engine on those
//! exact ids and also its own pipeline's `LIGHT` entry on the same world; both
//! must match vanilla cell by cell. `--break-carvers` deliberately reverts the
//! pipeline input to CARVERS and must report mismatches near canopies, glowstone,
//! lanterns or structure light sources.

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Command, Stdio},
};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};
use mc_bamboo_data::{registries, version};
use mc_bamboo_game::{
    region::RegionColumn,
    world_options::{LevelType, WorldGenConfig},
    worldgen::{GeneratedChunk, build_region_column},
    worldgen_light::{LightInput, generate_light_stage_with_input, post_features_blocks},
};
use mc_bamboo_world::{
    MIN_Y, SECTION_COUNT, WORLD_HEIGHT,
    column::{ChunkColumn, ChunkColumnParts},
    light::generate::generate_light_from_layers,
};
use serde_json::Value;

use crate::oracle::collect_log;

const USAGE: &str = "usage: light-pipeline <java> [--seeds N] [--chunks N] [--salt S] \
     [--structures] [--target SEED X Z]... [--show N] [--break-carvers]";
const DEFAULT_SHOW: usize = 8;
/// 一柱逐格数 `y × x × z`。 Cells per column.
const CELLS: usize = WORLD_HEIGHT as usize * 256;
/// 一个 quart 群系段的格子数。 Cells per biome section.
const BIOME_CELLS: usize = 64;

#[derive(Debug)]
struct Options {
    forwarded: Vec<String>,
    structures: bool,
    targets: Vec<(i64, i32, i32)>,
    show: usize,
    break_carvers: bool,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            forwarded: vec!["--light-pipeline".to_owned()],
            structures: false,
            targets: Vec::new(),
            show: DEFAULT_SHOW,
            break_carvers: false,
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
                "--structures" => {
                    options.structures = true;
                    options.forwarded.push(arg.to_owned());
                }
                "--show" => {
                    options.show = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?;
                }
                "--target" => {
                    let number = |text: &str| -> Result<i64, String> {
                        text.parse().map_err(|_| USAGE.to_owned())
                    };
                    let seed = number(args.next().ok_or(USAGE)?)?;
                    let x = number(args.next().ok_or(USAGE)?)? as i32;
                    let z = number(args.next().ok_or(USAGE)?)? as i32;
                    options.targets.push((seed, x, z));
                }
                "--break-carvers" => options.break_carvers = true,
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

#[derive(Debug, Default)]
struct Stats {
    scenes: u64,
    engine_cells: u64,
    pipeline_cells: u64,
    block_cells: u64,
    mismatches: Vec<String>,
}

/// 启动官方进程，一边读一边比较。`command` 已指向 `ChunkgenOracle.java`。
///
/// Launch the official process and compare each line. `command` already points
/// at `ChunkgenOracle.java`.
pub fn run(mut command: Command, queries_path: &Path, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    // 默认查询：树林 / 发光地衣 / 矿石区；带结构组换成光源结构。
    // 有些结构（如沙漠神殿 (42,-375,-22)）的结构件写会撞上别的结构件，
    // 官方调度与 README 记录的「固定先后、先写的赢」本来就可能不同；
    // 那类坐标留给 `--target` 排查，默认表只放整块逐格一致的结构光源坐标。
    // Default queries cover canopies / glow lichen / ores; the structure group
    // uses light-bearing structure chunks. A few structure pieces overlap in a
    // way that depends on vanilla's scheduling (README documents Bamboo's fixed
    // first-writer order); those coordinates are left to `--target` diagnostics.
    let default_queries: &[(i64, i32, i32)] = if options.structures {
        &[(42, 17, 7), (42, -267, -276), (42, 4, 0)]
    } else {
        &[
            (2026, -2, -2),
            (2026, 0, 0),
            (2026, 2, 2),
            (42, 13, -400),
            (42, 255, -109),
            (0, 0, 0),
        ]
    };
    let targets = if options.targets.is_empty() {
        default_queries.to_vec()
    } else {
        options.targets.clone()
    };
    let text = targets
        .iter()
        .map(|(seed, x, z)| format!("{seed} {x} {z}\n"))
        .collect::<String>();
    std::fs::write(queries_path, text)
        .map_err(|error| format!("cannot write {}: {error}", queries_path.display()))?;
    command.arg("--chunks-file").arg(queries_path);

    let mut child = command
        .args(&options.forwarded)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut report = Report {
        stats: Stats::default(),
        options: &options,
        header_ok: false,
        official_checks: 0,
        scene_keys: BTreeMap::new(),
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

struct Report<'a> {
    stats: Stats,
    options: &'a Options,
    header_ok: bool,
    official_checks: u64,
    /// 每个场景里发光方块数，确认这批坐标真的覆盖到光源。
    scene_keys: BTreeMap<(i64, i32, i32), u64>,
}

impl Report<'_> {
    fn read_lines(&mut self, source: BufReader<impl Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            let value: Value =
                serde_json::from_str(&line).map_err(|error| format!("bad oracle line: {error}"))?;
            match value.get("type").and_then(Value::as_str) {
                Some("header") => self.check_header(&value)?,
                Some("lightscene") => self.on_scene(&value)?,
                Some("end") => {
                    self.official_checks = value.get("checks").and_then(Value::as_u64).unwrap_or(0);
                }
                // `--light-pipeline` 仍会写 biomes 线，本对拍不用它。
                Some("biomes") => {}
                other => return Err(format!("unexpected oracle line {other:?}")),
            }
        }
        if !self.header_ok {
            return Err("oracle did not emit a header".to_owned());
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
        if get_i64("minY")? != i64::from(MIN_Y)
            || get_i64("height")? != i64::from(WORLD_HEIGHT)
            || value.get("lightPipeline").and_then(Value::as_bool) != Some(true)
        {
            return Err("light-pipeline dimension or mode mismatch".to_owned());
        }
        self.header_ok = true;
        Ok(())
    }

    /// 一条题：官方 3×3 方块 → 官方光；Bamboo 光引擎和游戏 LIGHT 阶段各算一遍。
    /// A scene: official 3x3 blocks → official light; compare Bamboo's engine and
    /// the game's LIGHT stage against it.
    fn on_scene(&mut self, value: &Value) -> Result<(), String> {
        let get = |key: &str| -> Result<i64, String> {
            value
                .get(key)
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("lightscene line missing {key}"))
        };
        let (seed, x, z) = (get("seed")?, get("x")? as i32, get("z")? as i32);
        if value.get("structures").and_then(Value::as_bool) != Some(self.options.structures) {
            return Err(format!("seed {seed} ({x},{z}): structures flag mismatch"));
        }
        if get("minY")? != i64::from(MIN_Y)
            || get("height")? != i64::from(WORLD_HEIGHT)
            || get("cells")? != CELLS as i64
        {
            return Err(format!("seed {seed} ({x},{z}): dimension mismatch"));
        }
        let palette = int_array(value, "palette")?;
        let chunks = value
            .get("chunks")
            .and_then(Value::as_array)
            .ok_or("lightscene line missing chunks")?;
        if chunks.len() != 9 {
            return Err(format!(
                "seed {seed} ({x},{z}): {} chunks vs 9",
                chunks.len()
            ));
        }
        let official_sky = byte_array(value, "sky")?;
        let official_block = byte_array(value, "block")?;
        if official_sky.len() != CELLS || official_block.len() != CELLS {
            return Err(format!("seed {seed} ({x},{z}): sky/block length mismatch"));
        }
        let mut built: Vec<(ChunkPos, GeneratedChunk)> = Vec::with_capacity(9);
        let mut center_region: Option<Box<RegionColumn>> = None;
        let mut emitters = 0_u64;
        for (grid, chunk) in chunks.iter().enumerate() {
            let dz = grid as i32 / 3 - 1;
            let dx = grid as i32 % 3 - 1;
            if chunk.get("dx").and_then(Value::as_i64) != Some(i64::from(dx))
                || chunk.get("dz").and_then(Value::as_i64) != Some(i64::from(dz))
            {
                return Err(format!("seed {seed} ({x},{z}) grid {grid}: dx/dz mismatch"));
            }
            let indices = int_array(chunk, "blocks")?;
            if indices.len() != CELLS {
                return Err(format!(
                    "seed {seed} ({x},{z}) grid {grid}: bad block count"
                ));
            }
            let generated = generated_chunk(&palette, &indices, &mut emitters)?;
            if dx == 0 && dz == 0 {
                center_region = Some(build_region_column(&generated).map_err(|error| {
                    format!("seed {seed} ({x},{z}): cannot assemble column: {error}")
                })?);
            }
            built.push((ChunkPos::new(x + dx, z + dz), generated));
        }
        let center_region =
            center_region.ok_or_else(|| format!("seed {seed} ({x},{z}): no center chunk"))?;
        self.scene_keys.insert((seed, x, z), emitters);

        // 0. 游戏 LIGHT 阶段挑的 3×3 方块必须逐格等于官方 FEATURES 后的 3×3。
        //    这能区分「输入方块不对」和「光引擎算错」。
        let config = WorldGenConfig {
            seed,
            level_type: LevelType::Normal,
            generate_structures: self.options.structures,
            ..WorldGenConfig::default()
        };
        let game_blocks = post_features_blocks(&config, x, z, None, &[])
            .map_err(|error| format!("seed {seed} ({x},{z}): post-features blocks: {error}"))?;
        for (grid, (official, game)) in built
            .iter()
            .map(|(_, generated)| generated.blocks.as_slice())
            .zip(game_blocks.iter())
            .enumerate()
        {
            let dz = grid as i32 / 3 - 1;
            let dx = grid as i32 % 3 - 1;
            for (y, (want, have)) in official.iter().zip(game.iter()).enumerate() {
                for (cell, (&want, &have)) in want.iter().zip(have.iter()).enumerate() {
                    self.stats.block_cells += 1;
                    if want == have || self.stats.mismatches.len() >= 20_000 {
                        continue;
                    }
                    let lx = cell / 16;
                    let lz = cell % 16;
                    self.stats.mismatches.push(format!(
                        "seed {seed} ({x},{z}) blocks ({dx},{dz}) at ({lx},{},{lz}): official {want}, game {have}",
                        y as i32 + MIN_Y
                    ));
                }
            }
        }

        // 1. 同一批方块喂给光引擎（和 gen-light 同样的路径）。
        let columns: Vec<(ChunkPos, &[Vec<u32>])> = built
            .iter()
            .map(|(pos, generated)| (*pos, generated.blocks.as_slice()))
            .collect();
        let light = generate_light_from_layers(ChunkPos::new(x, z), &columns)
            .map_err(|error| format!("seed {seed} ({x},{z}): engine: {error}"))?;
        let (engine_sky, engine_block) = light_cells(&center_region, &light)?;
        let base = Compare {
            pipeline: false,
            seed,
            x,
            z,
            layer: "",
        };
        compare_layer(
            &mut self.stats,
            Compare {
                layer: "engine sky",
                ..base
            },
            &official_sky,
            &engine_sky,
        );
        compare_layer(
            &mut self.stats,
            Compare {
                layer: "engine block",
                ..base
            },
            &official_block,
            &engine_block,
        );

        // 2. 游戏流水线 LIGHT 阶段自己挑的 3×3 方块算出来的光；改坏确认时
        //    故意把输入退回 CARVERS，比较器必须报不一致。
        let input = if self.options.break_carvers {
            LightInput::Carvers
        } else {
            LightInput::PostFeatures
        };
        let game = generate_light_stage_with_input(&config, x, z, None, &[], input)
            .map_err(|error| format!("seed {seed} ({x},{z}): pipeline: {error}"))?;
        let (game_sky, game_block) = light_cells(&center_region, &game)?;
        let pipeline = Compare {
            pipeline: true,
            ..base
        };
        compare_layer(
            &mut self.stats,
            Compare {
                layer: "pipeline sky",
                ..pipeline
            },
            &official_sky,
            &game_sky,
        );
        compare_layer(
            &mut self.stats,
            Compare {
                layer: "pipeline block",
                ..pipeline
            },
            &official_block,
            &game_block,
        );
        self.stats.scenes += 1;
        Ok(())
    }

    fn finish(self) -> Result<(), String> {
        let Stats {
            scenes,
            engine_cells,
            pipeline_cells,
            block_cells,
            mismatches,
        } = self.stats;
        println!(
            "light-pipeline: {scenes} scenes, {block_cells} blocks, {engine_cells} engine cells, \
             {pipeline_cells} pipeline cells, {} mismatches, {} official checks, emitters {:?}",
            mismatches.len(),
            self.official_checks,
            self.scene_keys
        );
        if scenes == 0 {
            return Err("light-pipeline compared no scenes".to_owned());
        }
        if self.options.break_carvers {
            if mismatches.is_empty() {
                return Err(
                    "break-carvers reported 0 mismatches; the light input detector is broken"
                        .to_owned(),
                );
            }
            println!(
                "light-pipeline break confirmation: detector works ({} mismatches)",
                mismatches.len()
            );
            return Ok(());
        }
        // 这批坐标里至少要有一个发光方块，否则「0 不一致」可能只是没覆盖到光源。
        // At least one emitter must appear, or 0 mismatches could be a false green.
        if self.scene_keys.values().sum::<u64>() == 0 {
            return Err("light-pipeline queries contained no light-emitting blocks".to_owned());
        }
        if !mismatches.is_empty() {
            for message in mismatches.iter().take(self.options.show) {
                eprintln!("  MISMATCH {message}");
            }
            return Err(format!(
                "light-pipeline found {} mismatches",
                mismatches.len()
            ));
        }
        println!("light-pipeline: 0 mismatches");
        Ok(())
    }
}

/// 把官方的 palette + 下标还原成一份 `GeneratedChunk`，顺便数发光方块。
/// Rebuild a `GeneratedChunk` from the official palette + indices and count emitters.
fn generated_chunk(
    palette: &[u32],
    indices: &[u32],
    emitters: &mut u64,
) -> Result<GeneratedChunk, String> {
    let mut blocks = Vec::with_capacity(WORLD_HEIGHT as usize);
    for y in 0..WORLD_HEIGHT as usize {
        let mut layer = Vec::with_capacity(256);
        for column in 0..256 {
            let id = indices[y * 256 + column] as usize;
            let state = *palette
                .get(id)
                .ok_or_else(|| format!("palette index {id} out of range"))?;
            if mc_bamboo_data::light_emission(state) > 0 {
                *emitters += 1;
            }
            layer.push(state);
        }
        blocks.push(layer);
    }
    let biomes = vec![vec![registries::biome::PLAINS as u32; BIOME_CELLS]; SECTION_COUNT];
    Ok(GeneratedChunk {
        blocks,
        biomes,
        heightmaps: Default::default(),
        post_processing: Vec::new(),
        block_entities: Vec::new(),
    })
}

/// 把 `GeneratedLight` 装进真正的 `ChunkColumn`（走官方 `None` 段天空光回退）。
/// Install `GeneratedLight` into a real `ChunkColumn` (vanilla's `None`-section
/// sky fallback) and read per-cell sky/block light.
fn light_cells(
    center: &RegionColumn,
    light: &mc_bamboo_world::light::generate::GeneratedLight,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut parts = ChunkColumnParts::read_from(center.parts().column);
    parts.light = [light.sky.clone(), light.block.clone()];
    parts.light_on = true;
    let column = ChunkColumn::from_parts(parts)
        .map_err(|error| format!("cannot install generated light: {error}"))?;
    let mut sky = Vec::with_capacity(CELLS);
    let mut block = Vec::with_capacity(CELLS);
    for y in MIN_Y..MIN_Y + WORLD_HEIGHT {
        for x in 0..16 {
            for z in 0..16 {
                let at = BlockPos::new(x, y, z);
                sky.push(column.light_value(LightLayer::Sky, at));
                block.push(column.light_value(LightLayer::Block, at));
            }
        }
    }
    Ok((sky, block))
}

/// 一条逐格比较的上下文：场景坐标、层名、是不是流水线那一组。
/// Context for one cell-by-cell comparison: scene coordinates, layer, group.
#[derive(Debug, Clone, Copy)]
struct Compare<'a> {
    pipeline: bool,
    seed: i64,
    x: i32,
    z: i32,
    layer: &'a str,
}

/// 逐格比较一层光。 Compare one light layer cell by cell.
fn compare_layer(stats: &mut Stats, at: Compare<'_>, official: &[u8], bamboo: &[u8]) {
    for (cell, (&expected, &ours)) in official.iter().zip(bamboo).enumerate() {
        if at.pipeline {
            stats.pipeline_cells += 1;
        } else {
            stats.engine_cells += 1;
        }
        if expected == ours || stats.mismatches.len() >= 20_000 {
            continue;
        }
        let y = cell as i32 / 256 + MIN_Y;
        let lx = (cell % 256) / 16;
        let lz = cell % 16;
        stats.mismatches.push(format!(
            "seed {} ({},{}) {} at ({lx},{y},{lz}): official {expected}, bamboo {ours}",
            at.seed, at.x, at.z, at.layer
        ));
    }
}

fn int_array(value: &Value, key: &str) -> Result<Vec<u32>, String> {
    let array = value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("line missing {key} array"))?;
    array
        .iter()
        .map(|entry| {
            entry
                .as_u64()
                .map(|v| v as u32)
                .ok_or_else(|| format!("{key} entry is not a number"))
        })
        .collect()
}

fn byte_array(value: &Value, key: &str) -> Result<Vec<u8>, String> {
    let array = value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("line missing {key} array"))?;
    array
        .iter()
        .map(|entry| {
            entry
                .as_u64()
                .map(|v| v as u8)
                .ok_or_else(|| format!("{key} entry is not a number"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_forward_scene_args_and_break_confirmation() {
        let options = Options::parse(&[
            "--seeds",
            "2",
            "--chunks",
            "3",
            "--structures",
            "--break-carvers",
        ])
        .unwrap();
        assert!(options.forwarded.contains(&"--light-pipeline".to_owned()));
        assert!(options.forwarded.contains(&"--structures".to_owned()));
        assert!(options.structures);
        assert!(options.break_carvers);
        assert!(Options::parse(&["--bogus"]).is_err());
    }

    /// 故意改坏一格：逐格比较必须报不一致。
    /// Deliberately break one cell: the comparator must report a mismatch.
    #[test]
    fn layer_comparator_detects_a_single_wrong_cell() {
        let official = vec![0u8; 4];
        let mut stats = Stats::default();
        compare_layer(
            &mut stats,
            Compare {
                pipeline: false,
                seed: 1,
                x: 0,
                z: 0,
                layer: "sky",
            },
            &official,
            &official,
        );
        assert!(stats.mismatches.is_empty());
        assert_eq!(stats.engine_cells, 4);

        let broken = vec![0u8, 1, 0, 0];
        let mut stats = Stats::default();
        compare_layer(
            &mut stats,
            Compare {
                pipeline: true,
                seed: 1,
                x: 0,
                z: 0,
                layer: "sky",
            },
            &official,
            &broken,
        );
        assert_eq!(stats.mismatches.len(), 1);
        assert_eq!(stats.pipeline_cells, 4);
    }
}
