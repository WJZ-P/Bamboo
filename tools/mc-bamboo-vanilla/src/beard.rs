//! Beardifier 对拍：官方 `Beardifier`（@VisibleForTesting 构造器 + 真实 NoiseChunk）出题，
//! Bamboo 用导出的 Rigid / junction 重建 `Beardifier`，逐位比较：
//!
//! * `BEARD_KERNEL` 的原始位模式；
//! * 每个区块全 16×height×16 体积上的 `sampleValue`（density）和 `sampleVolume`（volume）；
//! * `affectedBox`（并集 + inflatedBy(24)）；
//! * NOISE / SURFACE / CARVERS 三个阶段的方块。
//!
//! 这样不依赖 Bamboo 自己的结构件生成：`from_parts` 的输入就是结构 / jigsaw 队员以后
//! 从真实 `StructureStart` 产出的那批 Rigid + junction。Java 侧用的是覆盖全部
//! TerrainAdjustment 的代表性结构件盒（不是 {@code Structure.generate} 的产物，模板
//! 导出归其它里程碑），但 Beardifier 的输入本就只是盒 + 调整类型 + 连接点。
//! `--break-self-test` 故意改坏一个 `groundLevelDelta`，确认对拍会失败。
//!
//! Beardifier oracle: vanilla `Beardifier` (the @VisibleForTesting constructor inside a real
//! `NoiseChunk`) poses questions, Bamboo rebuilds a `Beardifier` from the exported Rigid /
//! junction inputs and compares bit-for-bit: the `BEARD_KERNEL` raw bits, the full-volume
//! `sampleValue` / `sampleVolume`, the `affectedBox`, and NOISE/SURFACE/CARVERS blocks. The
//! Java boxes are representative structure footprints covering all TerrainAdjustment kinds
//! (not `Structure.generate` output; template export belongs to another milestone), but the
//! Beardifier input is only boxes + adjustment + junctions. This does not depend on Bamboo's
//! own structure-piece generation. `--break-self-test` perturbs a `groundLevelDelta` to prove
//! the oracle actually fails.

use std::{
    collections::{BTreeMap, HashMap},
    process::Command,
};

use mc_bamboo_data::block_by_name;
use mc_bamboo_worldgen::{
    Worldgen,
    beardifier::{Beardifier, Box3, Junction, Rigid, TerrainAdjustment, kernel_values},
    chunk::{Generator, Stage},
    volume::Volume,
};
use serde_json::Value;

struct Header {
    min_y: i32,
    height: i32,
    sea_level: i32,
    count: usize,
}

struct RigidSpec {
    box_: [i32; 6],
    adj: String,
    gld: i32,
}

struct BlocksSnapshot {
    palette: Vec<String>,
    blocks: Vec<u32>,
}

struct Case {
    seed: i64,
    x: i32,
    z: i32,
    rigids: Vec<RigidSpec>,
    junctions: Vec<[i32; 3]>,
    affected: Option<[i32; 6]>,
    density: Vec<u32>,
    volume: Vec<u32>,
    blocks: BTreeMap<String, BlocksSnapshot>,
}

struct Parsed {
    header: Header,
    kernel: Vec<u32>,
    cases: BTreeMap<u32, Case>,
    checks: u64,
}

pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let mut break_self_test = false;
    let mut forward: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--break-self-test" => break_self_test = true,
            "--through" => {
                let stage = args
                    .get(i + 1)
                    .ok_or("--through needs a stage: noise|surface|carvers")?;
                forward.push("--through".to_owned());
                forward.push((*stage).to_owned());
                i += 1;
            }
            other => return Err(format!("unknown beard option {other}")),
        }
        i += 1;
    }
    let worldgen = Worldgen::load()?;
    let output = command
        .args(&forward)
        .output()
        .map_err(|error| format!("cannot run BeardifierOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "BeardifierOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed = parse_output(&stdout)?;
    compare(&worldgen, &parsed, break_self_test)
}

fn int_field(value: &Value, key: &str) -> Result<i64, String> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("missing integer field {key}"))
}

fn str_field<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string field {key}"))
}

fn bits(value: &Value) -> Result<u32, String> {
    value
        .as_u64()
        .map(|bits| bits as u32)
        .ok_or_else(|| format!("bad float bits {value}"))
}

fn bits_array(value: &Value, key: &str) -> Result<Vec<u32>, String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing array field {key}"))?
        .iter()
        .map(bits)
        .collect()
}

fn int_array<const N: usize>(value: &Value) -> Result<[i32; N], String> {
    let array = value
        .as_array()
        .ok_or_else(|| format!("expected array of {N} ints"))?;
    if array.len() != N {
        return Err(format!("expected {N} ints, got {}", array.len()));
    }
    let mut result = [0_i32; N];
    for (index, element) in array.iter().enumerate() {
        result[index] = element
            .as_i64()
            .ok_or_else(|| format!("bad int in {value}"))? as i32;
    }
    Ok(result)
}

fn parse_case(value: &Value) -> Result<Case, String> {
    let rigids = value
        .get("rigids")
        .and_then(Value::as_array)
        .ok_or("missing rigids")?
        .iter()
        .map(|r| {
            Ok(RigidSpec {
                box_: int_array::<6>(r.get("box").ok_or("missing box")?)?,
                adj: str_field(r, "adj")?.to_owned(),
                gld: int_field(r, "gld")? as i32,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let junctions = value
        .get("junctions")
        .and_then(Value::as_array)
        .ok_or("missing junctions")?
        .iter()
        .map(int_array::<3>)
        .collect::<Result<Vec<_>, String>>()?;
    let affected = match value.get("affected") {
        None | Some(Value::Null) => None,
        Some(array) => Some(int_array::<6>(array)?),
    };
    Ok(Case {
        seed: int_field(value, "seed")?,
        x: int_field(value, "x")? as i32,
        z: int_field(value, "z")? as i32,
        rigids,
        junctions,
        affected,
        density: bits_array(value, "density")?,
        volume: bits_array(value, "volume")?,
        blocks: BTreeMap::new(),
    })
}

fn parse_output(stdout: &str) -> Result<Parsed, String> {
    let mut header: Option<Header> = None;
    let mut kernel: Option<Vec<u32>> = None;
    let mut cases: BTreeMap<u32, Case> = BTreeMap::new();
    let mut pending_blocks: Vec<(u32, String, BlocksSnapshot)> = Vec::new();
    let mut checks = 0_u64;
    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let value: Value =
            serde_json::from_str(line).map_err(|error| format!("bad oracle line: {error}"))?;
        match str_field(&value, "type")? {
            "header" => {
                header = Some(Header {
                    min_y: int_field(&value, "minY")? as i32,
                    height: int_field(&value, "height")? as i32,
                    sea_level: int_field(&value, "seaLevel")? as i32,
                    count: int_field(&value, "count")? as usize,
                });
            }
            "kernel" => kernel = Some(bits_array(&value, "bits")?),
            "case" => {
                let id = int_field(&value, "id")? as u32;
                cases.insert(id, parse_case(&value)?);
            }
            "blocks" => {
                let id = int_field(&value, "id")? as u32;
                let stage = str_field(&value, "stage")?.to_owned();
                let palette = value
                    .get("palette")
                    .and_then(Value::as_array)
                    .ok_or("missing palette")?
                    .iter()
                    .map(|name| {
                        name.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| format!("bad palette name {name}"))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let blocks = value
                    .get("blocks")
                    .and_then(Value::as_array)
                    .ok_or("missing blocks")?
                    .iter()
                    .map(|index| {
                        index
                            .as_u64()
                            .map(|index| index as u32)
                            .ok_or_else(|| format!("bad block index {index}"))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                pending_blocks.push((id, stage, BlocksSnapshot { palette, blocks }));
            }
            "end" => checks = int_field(&value, "checks")? as u64,
            other => return Err(format!("unknown oracle line type {other}")),
        }
    }
    for (id, stage, snapshot) in pending_blocks {
        cases
            .get_mut(&id)
            .ok_or_else(|| format!("blocks for unknown case {id}"))?
            .blocks
            .insert(stage, snapshot);
    }
    Ok(Parsed {
        header: header.ok_or("missing header")?,
        kernel: kernel.ok_or("missing kernel")?,
        cases,
        checks,
    })
}

fn state_of(name: &str) -> Option<u32> {
    let body = name.strip_prefix("Block{")?;
    let (id, _) = body.split_once('}')?;
    Some(block_by_name(id)?.default_state)
}

fn stage_of(name: &str) -> Option<Stage> {
    Some(match name {
        "noise" => Stage::Noise,
        "surface" => Stage::Surface,
        "carvers" => Stage::Carvers,
        _ => return None,
    })
}

fn compare(worldgen: &Worldgen, parsed: &Parsed, break_self_test: bool) -> Result<(), String> {
    let mut mismatches: Vec<String> = Vec::new();
    let overworld = worldgen
        .settings("minecraft:overworld")
        .ok_or("missing overworld settings")?;
    if overworld.sea_level != parsed.header.sea_level {
        push(
            &mut mismatches,
            format!(
                "header seaLevel {} vs Bamboo overworld {}",
                parsed.header.sea_level, overworld.sea_level
            ),
        );
    }
    let mut kernel_cmp = 0_usize;
    let mut affected_cmp = 0_usize;
    let mut density_cmp = 0_usize;
    let mut volume_cmp = 0_usize;
    let mut block_cmp = 0_usize;
    let mut generators: HashMap<i64, Generator<'_>> = HashMap::new();

    let ours_kernel = kernel_values();
    if ours_kernel.len() != parsed.kernel.len() {
        mismatches.push(format!(
            "kernel length {} vs official {}",
            ours_kernel.len(),
            parsed.kernel.len()
        ));
    }
    for (index, &official) in parsed.kernel.iter().enumerate() {
        kernel_cmp += 1;
        let Some(&ours) = ours_kernel.get(index) else {
            break;
        };
        if ours.to_bits() != official {
            push(
                &mut mismatches,
                format!(
                    "kernel[{index}] bits {:08x} vs official {official:08x}",
                    ours.to_bits()
                ),
            );
        }
    }

    if parsed.cases.len() != parsed.header.count {
        push(
            &mut mismatches,
            format!(
                "case count {} vs header {}",
                parsed.cases.len(),
                parsed.header.count
            ),
        );
    }

    for (&id, case) in &parsed.cases {
        let mut rigids: Vec<Rigid> = case
            .rigids
            .iter()
            .map(|r| {
                let b = r.box_;
                Ok(Rigid {
                    box_: Box3 {
                        min_x: b[0],
                        min_y: b[1],
                        min_z: b[2],
                        max_x: b[3],
                        max_y: b[4],
                        max_z: b[5],
                    },
                    terrain_adjustment: TerrainAdjustment::parse(&r.adj)
                        .ok_or_else(|| format!("case {id}: unknown adjustment {}", r.adj))?,
                    ground_level_delta: r.gld,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if break_self_test
            && let Some(rigid) = rigids
                .iter_mut()
                .find(|r| r.terrain_adjustment != TerrainAdjustment::None)
        {
            rigid.ground_level_delta += 1;
        }
        let junctions: Vec<Junction> = case
            .junctions
            .iter()
            .map(|j| Junction {
                source_x: j[0],
                source_ground_y: j[1],
                source_z: j[2],
            })
            .collect();
        let beardifier = Beardifier::from_parts(rigids, junctions);

        // affectedBox：并集 + inflatedBy(24) 的结果，逐字段核对。
        affected_cmp += 1;
        match (beardifier.affected_box(), case.affected) {
            (None, None) => {}
            (Some(ours), Some(expected)) => {
                let got = [
                    ours.min_x, ours.min_y, ours.min_z, ours.max_x, ours.max_y, ours.max_z,
                ];
                if got != expected {
                    push(
                        &mut mismatches,
                        format!("case {id} affectedBox: ours {got:?} vs {expected:?}"),
                    );
                }
            }
            (ours, expected) => push(
                &mut mismatches,
                format!("case {id} affectedBox present: ours {ours:?} vs {expected:?}"),
            ),
        }

        let volume = Volume::blocks(
            (16, parsed.header.height, 16),
            (case.x * 16, parsed.header.min_y, case.z * 16),
        );
        let expected_len = volume.len();
        if case.density.len() != expected_len || case.volume.len() != expected_len {
            push(
                &mut mismatches,
                format!(
                    "case {id} density/volume length {} / {} vs {expected_len}",
                    case.density.len(),
                    case.volume.len()
                ),
            );
            continue;
        }

        // sampleValue 逐点，原始位模式。
        let mut index = 0_usize;
        for z in 0..16 {
            for x in 0..16 {
                for y in 0..parsed.header.height {
                    let block_x = case.x * 16 + x;
                    let block_y = parsed.header.min_y + y;
                    let block_z = case.z * 16 + z;
                    let ours = beardifier.sample_value(block_x, block_y, block_z).to_bits();
                    density_cmp += 1;
                    if ours != case.density[index] {
                        push(
                            &mut mismatches,
                            format!(
                                "case {id} density ({block_x},{block_y},{block_z}) bits {ours:08x} vs {:08x}",
                                case.density[index]
                            ),
                        );
                    }
                    index += 1;
                }
            }
        }

        // sampleVolume 成批，原始位模式。
        let mut out = vec![0.0_f32; expected_len];
        beardifier.sample_volume(&mut out, &volume);
        for (index, &ours) in out.iter().enumerate() {
            volume_cmp += 1;
            if ours.to_bits() != case.volume[index] {
                push(
                    &mut mismatches,
                    format!(
                        "case {id} volume[{index}] bits {:08x} vs {:08x}",
                        ours.to_bits(),
                        case.volume[index]
                    ),
                );
            }
        }

        // 三个阶段的方块。
        for (stage_name, snapshot) in &case.blocks {
            let Some(stage) = stage_of(stage_name) else {
                push(
                    &mut mismatches,
                    format!("case {id}: unknown stage {stage_name}"),
                );
                continue;
            };
            let mut states = Vec::with_capacity(snapshot.palette.len());
            for name in &snapshot.palette {
                match state_of(name) {
                    Some(state) => states.push(state),
                    None => {
                        push(
                            &mut mismatches,
                            format!("case {id} {stage_name}: unknown state {name}"),
                        );
                        continue;
                    }
                }
            }
            if let std::collections::hash_map::Entry::Vacant(entry) = generators.entry(case.seed) {
                entry.insert(Generator::new(worldgen, case.seed)?);
            }
            let generator = &generators[&case.seed];
            let layers =
                generator.terrain_with_beardifier(case.x, case.z, stage, Some(&beardifier))?;
            let flat: Vec<u32> = layers.iter().flatten().copied().collect();
            if flat.len() != snapshot.blocks.len() {
                push(
                    &mut mismatches,
                    format!(
                        "case {id} {stage_name}: {} blocks vs official {}",
                        flat.len(),
                        snapshot.blocks.len()
                    ),
                );
                continue;
            }
            for (index, (&official_index, &bamboo)) in
                snapshot.blocks.iter().zip(flat.iter()).enumerate()
            {
                block_cmp += 1;
                let Some(&official) = states.get(official_index as usize) else {
                    push(
                        &mut mismatches,
                        format!(
                            "case {id} {stage_name}[{index}]: palette index {official_index} out of range"
                        ),
                    );
                    continue;
                };
                if official != bamboo {
                    push(
                        &mut mismatches,
                        format!(
                            "case {id} {stage_name}[{index}]: official {} ({}), bamboo {}",
                            snapshot.palette[official_index as usize], official, bamboo
                        ),
                    );
                }
            }
        }
    }

    let total = kernel_cmp + affected_cmp + density_cmp + volume_cmp + block_cmp;
    println!(
        "beard: {} cases (official {} checks), {} kernel, {} affectedBox, {} density, {} volume, {} block comparisons, {} total",
        parsed.cases.len(),
        parsed.checks,
        kernel_cmp,
        affected_cmp,
        density_cmp,
        volume_cmp,
        block_cmp,
        total
    );
    if mismatches.is_empty() {
        println!("BEARD RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in &mismatches {
            println!("BEARD FAIL: {line}");
        }
        Err(format!("BEARD RESULT: {} mismatches", mismatches.len()))
    }
}

fn push(list: &mut Vec<String>, line: String) {
    if list.len() < 32 {
        list.push(line);
    }
}
