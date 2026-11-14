//! `findValidGenerationPoint` 通用判定的对拍：官方 `Structure.GenerationContext` 对
//! `mc-bamboo-worldgen::structure::validity` 的纯函数实现。
//!
//! 两边都从磁盘上的同一份 `worldgen.json` 读结构注册表；Java 侧从官方注册表取
//! `structure.biomes()`，Rust 侧用 `Structures::parse` 加标签表展开。比 BIOME（合法群系名）、
//! H（中心 + 四角高度）、Q（每结构的整列 + 起点群系判定）、S（加权尝试序列）、
//! D（组合出的选中结构与有效性）五组，逐字段相等才算 0 不一致。
//!
//! Oracle for the generic `findValidGenerationPoint` gates: vanilla
//! `Structure.GenerationContext` vs `mc-bamboo-worldgen::structure::validity`. Both read the
//! same `worldgen.json`; Java takes `structure.biomes()` from the official registry, Rust uses
//! `Structures::parse` + tag expansion. Five groups are compared field by field.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::Path,
    process::Command,
};

use mc_bamboo_worldgen::{
    Worldgen,
    chunk::Generator,
    settings::SeededRouter,
    structure::{
        StartPlan, StructureSets, Structures, allowed_biome_names, column_has_valid_biome,
        is_valid_biome_at, rings::BiomeResolver, select_start, selection_sequence,
    },
};
use serde_json::Value;

/// 覆盖原点、固定点和远处坐标的种子。
///
/// Seeds covering the origin, fixed points and far coordinates.
const SEEDS: [i64; 6] = [0, 1, -1, 42, 12_345_678_901, -98_765_432_123];
/// 覆盖多种群系带的区块坐标。
///
/// Chunk coordinates spanning several biome bands.
const CHUNKS: [(i32, i32); 12] = [
    (0, 0),
    (1, 1),
    (-1, -1),
    (34, 34),
    (-34, 50),
    (100, -100),
    (1000, 1000),
    (-1000, 1000),
    (12, -7),
    (250, 250),
    (-500, 300),
    (7, -13),
];

struct QueryData {
    heights: [i32; 5],
    gates: HashMap<String, (i32, i32)>,
    orders: HashMap<String, Vec<String>>,
    decision: Option<(String, String, i32, i32)>,
}

struct OracleOutput {
    sets: Vec<String>,
    structures: Vec<String>,
    biomes: HashMap<String, Vec<String>>,
    queries: BTreeMap<(i64, i32, i32), QueryData>,
}

pub fn run(
    mut command: Command,
    worldgen_path: &Path,
    queries_path: &Path,
    _options: &[&str],
) -> Result<(), String> {
    let text = fs::read_to_string(worldgen_path)
        .map_err(|error| format!("cannot read worldgen.json: {error}"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|error| format!("bad worldgen.json: {error}"))?;
    let raw = value.as_object().ok_or("worldgen.json must be an object")?;
    let sets = StructureSets::parse(raw.get("structure_set").ok_or("missing structure_set")?)?;
    let structures = Structures::parse(raw.get("structure").ok_or("missing structure")?)?;

    let queries: Vec<(i64, i32, i32)> = SEEDS
        .iter()
        .flat_map(|&seed| CHUNKS.iter().map(move |&(x, z)| (seed, x, z)))
        .collect();
    write_queries(queries_path, &queries)?;

    let output = command
        .arg(worldgen_path)
        .arg(queries_path)
        .output()
        .map_err(|error| format!("cannot run StructureValidityOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "StructureValidityOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare(&sets, &structures, &String::from_utf8_lossy(&output.stdout))
}

fn write_queries(path: &Path, queries: &[(i64, i32, i32)]) -> Result<(), String> {
    let mut text = String::new();
    for (seed, x, z) in queries {
        text.push_str(&format!("{seed} {x} {z}\n"));
    }
    fs::write(path, text).map_err(|error| format!("cannot write queries file: {error}"))
}

fn parse_i32(fields: &[&str], index: usize, line: &str) -> Result<i32, String> {
    fields
        .get(index)
        .ok_or_else(|| format!("bad line: {line}"))?
        .parse()
        .map_err(|_| format!("bad i32 in {line}"))
}

fn parse_output(stdout: &str) -> Result<OracleOutput, String> {
    let mut set_list: Vec<String> = Vec::new();
    let mut structure_list: Vec<String> = Vec::new();
    let mut biomes: HashMap<String, Vec<String>> = HashMap::new();
    let mut queries: BTreeMap<(i64, i32, i32), QueryData> = BTreeMap::new();

    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields[0] {
            "SETS" => {
                set_list = fields
                    .get(2)
                    .unwrap_or(&"")
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            "STRUCTURES" => {
                structure_list = fields
                    .get(2)
                    .unwrap_or(&"")
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            "BIOME" => {
                let id = fields[1].to_owned();
                let names: Vec<String> = fields
                    .get(2)
                    .unwrap_or(&"")
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
                biomes.insert(id, names);
            }
            "H" | "Q" | "S" | "D" => {
                let seed: i64 = fields[1]
                    .parse()
                    .map_err(|_| format!("bad seed: {line}"))?;
                let cx = parse_i32(&fields, 2, line)?;
                let cz = parse_i32(&fields, 3, line)?;
                let key = (seed, cx, cz);
                let entry = queries.entry(key).or_insert_with(|| QueryData {
                    heights: [0; 5],
                    gates: HashMap::new(),
                    orders: HashMap::new(),
                    decision: None,
                });
                match fields[0] {
                    "H" => {
                        for i in 0..5 {
                            entry.heights[i] = parse_i32(&fields, 4 + i, line)?;
                        }
                    }
                    "Q" => {
                        let id = fields[4].to_owned();
                        let col = parse_i32(&fields, 5, line)?;
                        let pv = parse_i32(&fields, 6, line)?;
                        entry.gates.insert(id, (col, pv));
                    }
                    "S" => {
                        let set = fields[4].to_owned();
                        let order: Vec<String> = fields
                            .get(5)
                            .unwrap_or(&"")
                            .split(',')
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .collect();
                        entry.orders.insert(set, order);
                    }
                    "D" => {
                        entry.decision = Some((
                            fields[4].to_owned(),
                            fields[5].to_owned(),
                            parse_i32(&fields, 6, line)?,
                            parse_i32(&fields, 7, line)?,
                        ));
                    }
                    _ => unreachable!(),
                }
            }
            other => return Err(format!("unknown oracle line kind {other}: {line}")),
        }
    }
    Ok(OracleOutput {
        sets: set_list,
        structures: structure_list,
        biomes,
        queries,
    })
}

fn compare(
    sets: &StructureSets,
    structures: &Structures,
    stdout: &str,
) -> Result<(), String> {
    let output = parse_output(stdout)?;
    let mut mismatches: Vec<String> = Vec::new();
    let mut total = 0usize;
    let (mut biome_cmp, mut height_cmp, mut gate_cmp, mut order_cmp, mut decision_cmp) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    // 记一下两条判定分支和组合结果各命中多少，确认对拍真的比到了 true/false 两边。
    // Count how often each gate branch and each composition result is hit, to confirm the
    // oracle actually exercises both true/false sides.
    let (mut col_one, mut col_zero, mut pv_one, mut pv_zero) = (0usize, 0usize, 0usize, 0usize);
    let (mut d_valid, mut d_invalid) = (0usize, 0usize);
    let mut gates_disagree = 0usize;
    let mut heights_seen: HashSet<i32> = HashSet::new();

    // BIOME：Rust 标签解析展开出的名字集合要和官方 HolderSet 完全一致。
    // BIOME: Rust tag expansion must equal the official HolderSet.
    for (id, names) in &output.biomes {
        let def = structures
            .get(id)
            .ok_or_else(|| format!("Rust is missing structure {id}"))?;
        let mut ours: Vec<String> = allowed_biome_names(def)?.into_iter().collect();
        ours.sort();
        biome_cmp += 1;
        total += 1;
        if ours != *names {
            push(&mut mismatches, format!("BIOME {id}: ours {ours:?} vs {names:?}"));
        }
    }

    let worldgen = Worldgen::load()?;
    let settings = worldgen
        .settings("minecraft:overworld")
        .ok_or("missing overworld settings")?;
    let min_y = settings.min_y;
    let height = settings.height;
    let preset = worldgen
        .preset("minecraft:overworld")
        .ok_or("missing overworld preset")?;

    // 每个种子建一次 Generator（噪声/含水层）和 SeededRouter（单点群系）。
    // One Generator (noise/aquifer) and one SeededRouter (single-point biome) per seed.
    for &seed in &SEEDS {
        let generator = Generator::new(&worldgen, seed)?;
        let mut router = SeededRouter::new(settings, seed);
        let mut resolver = BiomeResolver {
            router: &mut router,
            source: preset,
        };
        for &(cx, cz) in &CHUNKS {
            let Some(data) = output.queries.get(&(seed, cx, cz)) else {
                return Err(format!("oracle missing query ({seed},{cx},{cz})"));
            };
            // H：中心 + 四角高度。
            // H: center and four corner heights.
            let center_x = cx * 16 + 8;
            let center_z = cz * 16 + 8;
            let min_x = cx * 16;
            let min_z = cz * 16;
            let ours_heights = [
                generator.first_occupied_height(center_x, center_z)?,
                generator.first_occupied_height(min_x, min_z)?,
                generator.first_occupied_height(min_x, min_z + 16)?,
                generator.first_occupied_height(min_x + 16, min_z)?,
                generator.first_occupied_height(min_x + 16, min_z + 16)?,
            ];
            for i in 0..5 {
                height_cmp += 1;
                total += 1;
                heights_seen.insert(data.heights[i]);
                if ours_heights[i] != data.heights[i] {
                    push(
                        &mut mismatches,
                        format!(
                            "H ({seed},{cx},{cz})[{i}]: ours {} vs {}",
                            ours_heights[i], data.heights[i]
                        ),
                    );
                }
            }

            // Q：每个结构的整列 + 起点群系判定。
            // Q: column + start biome gate for every structure.
            let stub_y = ours_heights[0];
            for id in &output.structures {
                let def = structures
                    .get(id)
                    .ok_or_else(|| format!("Rust is missing structure {id}"))?;
                let allowed: HashSet<String> = allowed_biome_names(def)?;
                let col =
                    column_has_valid_biome(&generator, (cx, cz), &allowed, (min_y, height))?;
                let pv = is_valid_biome_at(&mut resolver, center_x, stub_y, center_z, &allowed);
                let (jcol, jpv) = *data
                    .gates
                    .get(id)
                    .ok_or_else(|| format!("oracle missing Q for {id}"))?;
                gate_cmp += 2;
                total += 2;
                if jcol == 1 {
                    col_one += 1;
                } else {
                    col_zero += 1;
                }
                if jpv == 1 {
                    pv_one += 1;
                } else {
                    pv_zero += 1;
                }
                if jcol != jpv {
                    gates_disagree += 1;
                }
                if i32::from(col) != jcol || i32::from(pv) != jpv {
                    push(
                        &mut mismatches,
                        format!(
                            "Q ({seed},{cx},{cz}) {id}: ours ({},{}) vs ({},{})",
                            i32::from(col),
                            i32::from(pv),
                            jcol,
                            jpv
                        ),
                    );
                }
            }

            // S：加权尝试序列。
            // S: weighted attempt order.
            let mut plans = Vec::with_capacity(output.sets.len());
            for set in &output.sets {
                let entry = sets
                    .get(set)
                    .ok_or_else(|| format!("Rust is missing set {set}"))?;
                let order = selection_sequence(&entry.structures, seed, cx, cz);
                let jorder = data
                    .orders
                    .get(set)
                    .ok_or_else(|| format!("oracle missing S for {set}"))?;
                order_cmp += 1;
                total += 1;
                if order != *jorder {
                    push(
                        &mut mismatches,
                        format!("S ({seed},{cx},{cz}) {set}: ours {order:?} vs {jorder:?}"),
                    );
                }
                plans.push(StartPlan {
                    set: set.clone(),
                    attempts: order,
                });
            }

            // D：组合出的「选中的结构 + 是否有效 + 起点」。
            // D: composed "selected structure + valid + start".
            let selected = select_start(
                &plans,
                &structures,
                &generator,
                &mut resolver,
                (cx, cz),
                (min_y, height),
            )?;
            let (jset, jstructure, jvalid, jstub) = data
                .decision
                .as_ref()
                .ok_or_else(|| format!("oracle missing D for ({seed},{cx},{cz})"))?;
            let (oset, ostructure, ovalid, ostub) = match &selected {
                Some(s) => (s.set.clone(), s.structure.clone(), 1, s.stub.1),
                None => ("-".to_owned(), "-".to_owned(), 0, stub_y),
            };
            decision_cmp += 4;
            total += 4;
            if *jvalid == 1 {
                d_valid += 1;
            } else {
                d_invalid += 1;
            }
            if oset != *jset
                || ostructure != *jstructure
                || ovalid != *jvalid
                || (ovalid == 1 && ostub != *jstub)
            {
                push(
                    &mut mismatches,
                    format!(
                        "D ({seed},{cx},{cz}): ours ({oset},{ostructure},{ovalid},{ostub}) vs ({jset},{jstructure},{jvalid},{jstub})"
                    ),
                );
            }
        }
    }

    println!(
        "structure-validity: {} biome sets, {} height fields ({} distinct), {} gate fields (col {}/{} ones/zeros, pv {}/{}, col!=pv {}), {} selection orders, {} decision fields ({} valid / {} invalid), {} comparisons total",
        biome_cmp,
        height_cmp,
        heights_seen.len(),
        gate_cmp,
        col_one,
        col_zero,
        pv_one,
        pv_zero,
        gates_disagree,
        order_cmp,
        decision_cmp,
        d_valid,
        d_invalid,
        total
    );
    if mismatches.is_empty() {
        println!("STRUCTURE-VALIDITY RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in &mismatches {
            println!("STRUCTURE-VALIDITY FAIL: {line}");
        }
        Err(format!(
            "STRUCTURE-VALIDITY RESULT: {} mismatches",
            mismatches.len()
        ))
    }
}

fn push(list: &mut Vec<String>, line: String) {
    if list.len() < 24 {
        list.push(line);
    }
}
