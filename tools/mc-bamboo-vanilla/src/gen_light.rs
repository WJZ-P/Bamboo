//! `gen-light`：把 Bamboo 的生成期光照和官方 `LevelLightEngine` 逐格对拍。
//!
//! `GenLightOracle.java` 在 JVM 里给一片 3×3 的确定性合成方块场景跑官方的
//! `initializeLight` + `light`（生成期那两步），输出中心柱逐格的天空光、方块光，
//! 外加这片 3×3 各方格的官方方块状态 ID。这里用**同一份**方块状态 ID 拼成逐层方块，
//! 调 `mc_bamboo_world::light::generate::generate_light_from_layers`（也就是游戏里
//! `light` 阶段用的那份纯函数），把结果装进真正的 `ChunkColumn` 走官方的
//! `None` 段天空光回退后逐格比较。两边用同一份方块，不依赖地形对拍。
//!
//! `--break-self-test` 故意改坏 Bamboo 的一格光，确认比较器会报不一致。
//!
//! `gen-light`: compare Bamboo's generation-time light against the official
//! `LevelLightEngine` cell by cell. `GenLightOracle.java` drives the official
//! `initializeLight` + `light` steps over a deterministic 3×3 synthetic scene and
//! emits the center column's per-cell sky/block light plus the 3×3 block state
//! ids. This side builds block layers from those exact ids, calls
//! `generate_light_from_layers` (the pure function the `light` stage uses), installs the
//! result into a real `ChunkColumn` (to apply vanilla's `None`-section sky
//! fallback), and compares. Both use the same blocks, so no terrain oracle is
//! involved. `--break-self-test` corrupts one Bamboo cell to prove the detector
//! fires.

use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};
use mc_bamboo_data::{registries, version};
use mc_bamboo_game::{
    region::RegionColumn,
    worldgen::{GeneratedChunk, build_region_column},
};
use mc_bamboo_world::{
    MIN_Y, SECTION_COUNT, WORLD_HEIGHT,
    column::{ChunkColumn, ChunkColumnParts},
    light::generate::{GeneratedLight, generate_light_from_layers},
};
use serde_json::Value;

use crate::oracle::collect_log;

const USAGE: &str =
    "usage: gen-light <java> [--scenes N] [--seed S] [--show N] [--break-self-test]";
const DEFAULT_SHOW: usize = 8;
/// 一柱逐格数 `y × x × z`。 Cells per column.
const CELLS: usize = WORLD_HEIGHT as usize * 256;
/// 一个 quart 群系段的格子数。 Cells per biome section.
const BIOME_CELLS: usize = 64;

#[derive(Debug)]
struct Options {
    forwarded: Vec<String>,
    show: usize,
    /// 故意改坏 Bamboo 的一格光，确认对拍会报不一致（自检）。
    /// Corrupt one Bamboo light cell to verify the oracle fails (self-test).
    break_self_test: bool,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            forwarded: Vec::new(),
            show: DEFAULT_SHOW,
            break_self_test: false,
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--scenes" | "--seed" => {
                    let value = args.next().ok_or(USAGE)?;
                    options
                        .forwarded
                        .extend([arg.to_owned(), (*value).to_owned()]);
                }
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
    scenes: u64,
    cells: u64,
    mismatches: Vec<String>,
}

/// 启动官方进程，一边读一边比较。`command` 已经指向 `GenLightOracle.java`。
///
/// Launch the official process and compare each line. `command` already points
/// at `GenLightOracle.java`.
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
        stats: Stats::default(),
        show: options.show,
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
        return Err(format!("GenLightOracle.java failed: {status}\n{log}"));
    }
    report.finish()
}

struct Report {
    stats: Stats,
    show: usize,
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
                "genlight" => self.on_genlight(&value)?,
                "end" => {
                    self.official_checks = value.get("checks").and_then(Value::as_u64).unwrap_or(0);
                }
                other => return Err(format!("unexpected oracle line type {other}")),
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
            || get_i64("cells")? != CELLS as i64
        {
            return Err("gen-light dimension mismatch between oracle and bamboo".to_owned());
        }
        self.header_ok = true;
        Ok(())
    }

    /// 一道题：用官方给的 9 宫格方块状态 ID 拼 `Chunk`，算生成期光照，
    /// 再逐格和官方的中心柱天空光 / 方块光比较。
    ///
    /// One scene: build `Chunk`s from the official 9-chunk state ids, run
    /// generation-time light, and compare the center column cell by cell.
    fn on_genlight(&mut self, value: &Value) -> Result<(), String> {
        let index = value.get("index").and_then(Value::as_u64).unwrap_or(0);
        let palette = int_array(value, "palette")?;
        let chunks = value
            .get("chunks")
            .and_then(Value::as_array)
            .ok_or("genlight line missing chunks")?;
        if chunks.len() != 9 {
            return Err(format!("genlight {index}: {} chunks vs 9", chunks.len()));
        }
        let official_sky = byte_array(value, "sky")?;
        let official_block = byte_array(value, "block")?;
        if official_sky.len() != CELLS || official_block.len() != CELLS {
            return Err(format!(
                "genlight {index}: sky/block length {} / {} vs {CELLS}",
                official_sky.len(),
                official_block.len()
            ));
        }

        // 拼 3×3 的逐层方块，同时组装中心柱的盒子供后面 `light_cells` 用。
        // Build the 3×3 block layers and assemble the center box for `light_cells`.
        let mut built: Vec<(ChunkPos, GeneratedChunk)> = Vec::with_capacity(9);
        let mut center_region: Option<Box<RegionColumn>> = None;
        for (grid, chunk) in chunks.iter().enumerate() {
            let dz = grid as i32 / 3 - 1;
            let dx = grid as i32 % 3 - 1;
            let reported_dx = chunk.get("dx").and_then(Value::as_i64).unwrap_or(99);
            let reported_dz = chunk.get("dz").and_then(Value::as_i64).unwrap_or(99);
            if reported_dx != i64::from(dx) || reported_dz != i64::from(dz) {
                return Err(format!(
                    "genlight {index} grid {grid}: ({reported_dx},{reported_dz}) vs ({dx},{dz})"
                ));
            }
            let indices = int_array(chunk, "blocks")?;
            if indices.len() != CELLS {
                return Err(format!(
                    "genlight {index} grid {grid}: {} blocks vs {CELLS}",
                    indices.len()
                ));
            }
            let generated = generated_chunk(&palette, &indices)?;
            if dx == 0 && dz == 0 {
                center_region = Some(build_region_column(&generated).map_err(|error| {
                    format!("genlight {index} grid {grid}: cannot assemble column: {error}")
                })?);
            }
            built.push((ChunkPos::new(dx, dz), generated));
        }
        let center_region =
            center_region.ok_or_else(|| format!("genlight {index}: no center chunk"))?;

        // 游戏 `light` 阶段用的那份纯函数：直接读逐层方块。
        // The pure function the `light` stage uses, reading block layers directly.
        let columns: Vec<(ChunkPos, &[Vec<u32>])> = built
            .iter()
            .map(|(pos, generated)| (*pos, generated.blocks.as_slice()))
            .collect();
        let light = generate_light_from_layers(ChunkPos::new(0, 0), &columns)
            .map_err(|error| format!("genlight {index}: {error}"))?;
        let (mut sky, block) = light_cells(&center_region, &light)?;
        if self.break_self_test {
            // 自检：改坏第一格光，确认下面会报不一致。
            // Self-test: corrupt one cell so the comparison must report a mismatch.
            sky[0] ^= 1;
        }
        self.stats.scenes += 1;

        compare_layer(
            &mut self.stats,
            index,
            "sky",
            &official_sky,
            &sky,
            self.show,
        );
        compare_layer(
            &mut self.stats,
            index,
            "block",
            &official_block,
            &block,
            self.show,
        );
        Ok(())
    }

    fn finish(self) -> Result<(), String> {
        let Stats {
            scenes,
            cells,
            mismatches,
        } = self.stats;
        println!(
            "gen-light: {scenes} scenes, {cells} cells, {} mismatches, {} official checks",
            mismatches.len(),
            self.official_checks
        );
        if self.break_self_test {
            if mismatches.is_empty() {
                return Err(
                    "self-test broke a light cell but gen-light reported 0 mismatches; \
                            the detector is broken"
                        .to_owned(),
                );
            }
            println!(
                "gen-light self-test: detector works ({} mismatches from the deliberate break)",
                mismatches.len()
            );
            return Ok(());
        }
        if scenes == 0 {
            return Err("gen-light compared no scenes".to_owned());
        }
        if !mismatches.is_empty() {
            for message in mismatches.iter().take(self.show) {
                eprintln!("  MISMATCH {message}");
            }
            return Err(format!("gen-light found {} mismatches", mismatches.len()));
        }
        println!("gen-light: 0 mismatches");
        Ok(())
    }
}

/// 把官方的 palette + 下标还原成一份 `GeneratedChunk`（群系用固定值，不影响光照）。
///
/// Rebuild a `GeneratedChunk` from the official palette + indices (biomes are a
/// fixed placeholder and do not affect light).
fn generated_chunk(palette: &[u32], indices: &[u32]) -> Result<GeneratedChunk, String> {
    let mut blocks = Vec::with_capacity(WORLD_HEIGHT as usize);
    for y in 0..WORLD_HEIGHT as usize {
        let mut layer = Vec::with_capacity(256);
        for column in 0..256 {
            let id = indices[y * 256 + column] as usize;
            let state = *palette
                .get(id)
                .ok_or_else(|| format!("palette index {id} out of range"))?;
            layer.push(state);
        }
        blocks.push(layer);
    }
    let biomes = vec![vec![registries::biome::PLAINS as u32; BIOME_CELLS]; SECTION_COUNT];
    // 生成期光照只读方块，不读高度图，这里给空表。
    // Generation-time light reads blocks only, not heightmaps, so they stay empty here.
    Ok(GeneratedChunk {
        blocks,
        biomes,
        heightmaps: Default::default(),
        post_processing: Vec::new(),
    })
}

/// 把 `GeneratedLight` 装进真正的 `ChunkColumn`（走官方 `None` 段天空光回退），
/// 读出中心柱逐格的天空光 / 方块光。
///
/// Install `GeneratedLight` into a real `ChunkColumn` (to apply vanilla's
/// `None`-section sky fallback) and read per-cell sky/block light.
fn light_cells(
    center: &RegionColumn,
    light: &GeneratedLight,
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

/// 逐格比较一层光，把不一致记进统计。 Compare one light layer cell by cell.
fn compare_layer(
    stats: &mut Stats,
    scene: u64,
    layer: &str,
    official: &[u8],
    bamboo: &[u8],
    _show: usize,
) {
    for (cell, (&expected, &ours)) in official.iter().zip(bamboo).enumerate() {
        stats.cells += 1;
        if expected != ours {
            let y = cell as i32 / 256 + MIN_Y;
            let x = (cell % 256) / 16;
            let z = cell % 16;
            stats.mismatches.push(format!(
                "scene {scene} {layer} at ({x},{y},{z}): official {expected}, bamboo {ours}"
            ));
        }
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

    /// 故意改坏一处：逐格比较必须报不一致，而不是假绿。
    /// Deliberately break one cell: the comparator must report a mismatch.
    #[test]
    fn layer_comparator_detects_a_single_wrong_cell() {
        let official = vec![0u8; 4];
        let clean = vec![0u8; 4];
        let mut stats = Stats::default();
        compare_layer(&mut stats, 0, "sky", &official, &clean, 8);
        assert!(stats.mismatches.is_empty());
        assert_eq!(stats.cells, 4);

        let broken = vec![0u8, 1, 0, 0];
        let mut stats = Stats::default();
        compare_layer(&mut stats, 0, "sky", &official, &broken, 8);
        assert_eq!(stats.mismatches.len(), 1);
    }

    #[test]
    fn options_forward_scene_args_and_self_test() {
        let options =
            Options::parse(&["--scenes", "3", "--seed", "9", "--break-self-test"]).unwrap();
        assert_eq!(options.forwarded, ["--scenes", "3", "--seed", "9"]);
        assert!(options.break_self_test);
        assert!(Options::parse(&["--bogus"]).is_err());
    }
}
