//! 四个代码生成结构的 `findGenerationPoint` 前置判定与「这个起点是否真的生成」的对拍：
//! 官方 `StructureGenerationOracle` 对 `mc-bamboo-worldgen::structure::start_point` 的纯函数。
//!
//! 每条查询由 Rust 生成 queries.txt，Java 侧调官方 `Structure.findValidGenerationPoint`
//! 并 dump KINDS / BIOME / R 三类行；Rust 侧用生产纯函数复算，逐字段比较 R 行：
//! valid、stub 三点、col、col7、sur、oceanFloor、base、五个高度、lowest、below60、pv。
//! 任一字段不同即报不一致。查询集含普通地形、深海、恶地、黑森林等区块，保证各条判定的
//! 真 / 假分支都被比到。
//!
//! Oracle for the four code-built structures' `findGenerationPoint` gates and
//! "whether this start really generates": vanilla `StructureGenerationOracle` vs
//! `mc-bamboo-worldgen::structure::start_point`. Rust writes queries.txt; Java calls the
//! official `Structure.findValidGenerationPoint` and dumps KINDS / BIOME / R lines; Rust
//! recomputes each field with production pure functions and compares every R-line field.
//! The query set spans plains, deep ocean, badlands and dark forest so both branches of
//! every gate are exercised.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    process::Command,
};

use mc_bamboo_worldgen::{
    Worldgen,
    chunk::Generator,
    settings::SeededRouter,
    structure::{
        Structures, allowed_biome_names,
        rings::BiomeResolver,
        start_point::{self, PointKind, Site},
    },
};
use serde_json::Value;

/// 五个结构 id，顺序与 Java oracle 的 KINDS 一致。
const KINDS: [&str; 5] = [
    "minecraft:mineshaft",
    "minecraft:mineshaft_mesa",
    "minecraft:stronghold",
    "minecraft:monument",
    "minecraft:mansion",
];
/// 覆盖原点、负坐标、远处坐标和随机源大数的种子。
const SEEDS: [i64; 6] = [0, 1, -1, 42, 12_345_678_901, -98_765_432_123];
/// 覆盖各判定真 / 假分支的区块坐标：
/// 普通地形、深海（sur=1、海底神殿 valid=1）、低地（below60=1）、
/// 恶地（恶地矿井 valid=1）、黑森林（林地府邸 valid=1）。
const CHUNKS: [(i32, i32); 16] = [
    (0, 0),
    (1, 1),
    (-1, -1),
    (34, 34),
    (100, -100),
    (1000, 1000),
    (-450, 0),
    (-300, -450),
    (-150, -300),
    (0, 150),
    (-600, 150),
    (300, -300),
    (-300, -300),
    (-150, -150),
    (-600, 600),
    (150, 0),
];

/// 一条 Java `R` 行的全部字段。
struct RRow {
    valid: bool,
    stub: (i32, i32, i32),
    col: bool,
    col7: bool,
    sur: bool,
    ocean_floor: i32,
    base: i32,
    heights: [i32; 5],
    lowest: i32,
    below_60: bool,
    pv: bool,
}

struct OracleOutput {
    kinds: Vec<String>,
    biomes: HashMap<String, Vec<String>>,
    rows: HashMap<(i64, i32, i32, String), RRow>,
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
    let structures = Structures::parse(raw.get("structure").ok_or("missing structure")?)?;

    let queries: Vec<(i64, i32, i32)> = SEEDS
        .iter()
        .flat_map(|&seed| CHUNKS.iter().map(move |&(cx, cz)| (seed, cx, cz)))
        .collect();
    write_queries(queries_path, &queries)?;

    // 内存紧，给这个 java 对拍进程设上限（JVM 选项经 JAVA_TOOL_OPTIONS 生效）。
    // Memory is tight; cap this java oracle process (JVM option via JAVA_TOOL_OPTIONS).
    command.env("JAVA_TOOL_OPTIONS", "-Xmx3g");
    let output = command
        .arg(queries_path)
        .output()
        .map_err(|error| format!("cannot run StructureGenerationOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "StructureGenerationOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare(
        &structures,
        &queries,
        &parse_output(&String::from_utf8_lossy(&output.stdout))?,
    )
}

fn write_queries(path: &Path, queries: &[(i64, i32, i32)]) -> Result<(), String> {
    let mut text = String::new();
    for (seed, cx, cz) in queries {
        text.push_str(&format!("{seed} {cx} {cz}\n"));
    }
    fs::write(path, text).map_err(|error| format!("cannot write queries file: {error}"))
}

fn p_i32(f: &[&str], i: usize, line: &str) -> Result<i32, String> {
    f.get(i)
        .ok_or_else(|| format!("short line: {line}"))?
        .parse()
        .map_err(|_| format!("bad i32 in {line}"))
}

fn p_bool(f: &[&str], i: usize, line: &str) -> Result<bool, String> {
    match *f.get(i).ok_or_else(|| format!("short line: {line}"))? {
        "1" => Ok(true),
        "0" => Ok(false),
        _ => Err(format!("bad bool in {line}")),
    }
}

fn parse_output(stdout: &str) -> Result<OracleOutput, String> {
    let mut kinds = Vec::new();
    let mut biomes = HashMap::new();
    let mut rows = HashMap::new();
    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        match f[0] {
            "KINDS" => {
                let n: usize = f
                    .get(1)
                    .ok_or_else(|| format!("bad KINDS line: {line}"))?
                    .parse()
                    .map_err(|_| format!("bad KINDS count: {line}"))?;
                kinds = f[2..2 + n].iter().map(|s| (*s).to_owned()).collect();
            }
            "BIOME" => {
                let id = f
                    .get(1)
                    .ok_or_else(|| format!("bad BIOME line: {line}"))?
                    .to_owned();
                let names: Vec<String> = f
                    .get(2)
                    .unwrap_or(&"")
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
                biomes.insert(id.to_owned(), names);
            }
            "R" => {
                if f.len() != 22 {
                    return Err(format!(
                        "R line has {} fields, expected 22: {line}",
                        f.len()
                    ));
                }
                let seed: i64 = f[1].parse().map_err(|_| format!("bad seed in {line}"))?;
                let cx = p_i32(&f, 2, line)?;
                let cz = p_i32(&f, 3, line)?;
                let row = RRow {
                    valid: p_bool(&f, 5, line)?,
                    stub: (
                        p_i32(&f, 6, line)?,
                        p_i32(&f, 7, line)?,
                        p_i32(&f, 8, line)?,
                    ),
                    col: p_bool(&f, 9, line)?,
                    col7: p_bool(&f, 10, line)?,
                    sur: p_bool(&f, 11, line)?,
                    ocean_floor: p_i32(&f, 12, line)?,
                    base: p_i32(&f, 13, line)?,
                    heights: [
                        p_i32(&f, 14, line)?,
                        p_i32(&f, 15, line)?,
                        p_i32(&f, 16, line)?,
                        p_i32(&f, 17, line)?,
                        p_i32(&f, 18, line)?,
                    ],
                    lowest: p_i32(&f, 19, line)?,
                    below_60: p_bool(&f, 20, line)?,
                    pv: p_bool(&f, 21, line)?,
                };
                rows.insert((seed, cx, cz, f[4].to_owned()), row);
            }
            other => return Err(format!("unknown oracle line kind {other}: {line}")),
        }
    }
    Ok(OracleOutput {
        kinds,
        biomes,
        rows,
    })
}

/// 累加一次字段比较；不一致时记入列表（最多显示 24 条，计数精确）。
fn check(
    mismatches: &mut Vec<String>,
    mismatch_count: &mut usize,
    total: &mut usize,
    label: &str,
    ours: String,
    theirs: String,
) {
    *total += 1;
    if ours != theirs {
        *mismatch_count += 1;
        if mismatches.len() < 24 {
            mismatches.push(format!("{label}: rust {ours} vs java {theirs}"));
        }
    }
}

fn compare(
    structures: &Structures,
    queries: &[(i64, i32, i32)],
    output: &OracleOutput,
) -> Result<(), String> {
    let mut mismatches: Vec<String> = Vec::new();
    let mut mismatch_count = 0usize;
    let mut total = 0usize;
    let (mut valid_yes, mut valid_no) = (0usize, 0usize);
    let (mut sur_yes, mut sur_no) = (0usize, 0usize);
    let (mut below_yes, mut below_no) = (0usize, 0usize);
    let mut kind_valid: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();

    // KINDS：两边结构 id 列表必须完全一致。
    if output.kinds != KINDS {
        check(
            &mut mismatches,
            &mut mismatch_count,
            &mut total,
            "KINDS",
            format!("{:?}", KINDS),
            format!("{:?}", output.kinds),
        );
    } else {
        total += 1;
    }

    // BIOME：Rust 标签展开展开出的名字集合要和官方 HolderSet 完全一致。
    let worldgen = Worldgen::load()?;
    let settings = worldgen
        .settings("minecraft:overworld")
        .ok_or("missing overworld settings")?;
    let preset = worldgen
        .preset("minecraft:overworld")
        .ok_or("missing overworld preset")?;
    let mut allowed_by_kind: HashMap<String, HashSet<String>> = HashMap::new();
    for id in KINDS {
        let def = structures
            .get(id)
            .ok_or_else(|| format!("Rust is missing structure {id}"))?;
        let allowed = allowed_biome_names(def)?;
        allowed_by_kind.insert(id.to_owned(), allowed.clone());
        let mut ours: Vec<String> = allowed.into_iter().collect();
        ours.sort();
        let theirs = output
            .biomes
            .get(id)
            .ok_or_else(|| format!("oracle missing BIOME for {id}"))?;
        check(
            &mut mismatches,
            &mut mismatch_count,
            &mut total,
            &format!("BIOME {id}"),
            format!("{ours:?}"),
            format!("{theirs:?}"),
        );
    }

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
            let site = Site { seed, cx, cz };
            let gates = start_point::site_gates(&generator, &mut resolver, site)?;
            for id in KINDS {
                let kind =
                    PointKind::from_id(id).ok_or_else(|| format!("no PointKind for {id}"))?;
                let allowed = allowed_by_kind
                    .get(id)
                    .ok_or_else(|| format!("missing allowed set for {id}"))?;
                let result = start_point::start_point(
                    kind,
                    site,
                    &generator,
                    &mut resolver,
                    allowed,
                    &gates,
                )?;
                let key = (seed, cx, cz, id.to_owned());
                let row = output
                    .rows
                    .get(&key)
                    .ok_or_else(|| format!("oracle missing R for {key:?}"))?;
                let label = format!("R ({seed},{cx},{cz}) {id}");
                check(
                    &mut mismatches,
                    &mut mismatch_count,
                    &mut total,
                    &format!("{label} valid"),
                    result.valid.to_string(),
                    row.valid.to_string(),
                );
                check(
                    &mut mismatches,
                    &mut mismatch_count,
                    &mut total,
                    &format!("{label} stub"),
                    format!("{:?}", result.stub),
                    format!("{:?}", row.stub),
                );
                check(
                    &mut mismatches,
                    &mut mismatch_count,
                    &mut total,
                    &format!("{label} col"),
                    result.col.to_string(),
                    row.col.to_string(),
                );
                check(
                    &mut mismatches,
                    &mut mismatch_count,
                    &mut total,
                    &format!("{label} col7"),
                    result.col7.to_string(),
                    row.col7.to_string(),
                );
                check(
                    &mut mismatches,
                    &mut mismatch_count,
                    &mut total,
                    &format!("{label} sur"),
                    gates.surrounding.to_string(),
                    row.sur.to_string(),
                );
                check(
                    &mut mismatches,
                    &mut mismatch_count,
                    &mut total,
                    &format!("{label} oceanFloor"),
                    gates.ocean_floor.to_string(),
                    row.ocean_floor.to_string(),
                );
                check(
                    &mut mismatches,
                    &mut mismatch_count,
                    &mut total,
                    &format!("{label} base"),
                    gates.base.to_string(),
                    row.base.to_string(),
                );
                check(
                    &mut mismatches,
                    &mut mismatch_count,
                    &mut total,
                    &format!("{label} heights"),
                    format!("{:?}", gates.heights),
                    format!("{:?}", row.heights),
                );
                check(
                    &mut mismatches,
                    &mut mismatch_count,
                    &mut total,
                    &format!("{label} lowest"),
                    gates.lowest.to_string(),
                    row.lowest.to_string(),
                );
                check(
                    &mut mismatches,
                    &mut mismatch_count,
                    &mut total,
                    &format!("{label} below60"),
                    gates.below_60.to_string(),
                    row.below_60.to_string(),
                );
                check(
                    &mut mismatches,
                    &mut mismatch_count,
                    &mut total,
                    &format!("{label} pv"),
                    result.pv.to_string(),
                    row.pv.to_string(),
                );
                if row.valid {
                    valid_yes += 1;
                    *kind_valid.entry(id).or_default() += 1;
                } else {
                    valid_no += 1;
                }
                // 结构无关的三个分支只按 (seed,cx,cz) 记一次。
                if id == KINDS[0] {
                    if row.sur {
                        sur_yes += 1;
                    } else {
                        sur_no += 1;
                    }
                    if row.below_60 {
                        below_yes += 1;
                    } else {
                        below_no += 1;
                    }
                }
            }
        }
    }

    let kind_summary = kind_valid
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "structure-generation: {} queries, {} R rows (valid {} yes / {} no; sur {}/{} ones/zeros; below60 {}/{}; per-kind valid {{{}}}), {} field comparisons",
        queries.len(),
        output.rows.len(),
        valid_yes,
        valid_no,
        sur_yes,
        sur_no,
        below_yes,
        below_no,
        kind_summary,
        total
    );
    if mismatches.is_empty() {
        println!("STRUCTURE-GENERATION RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in &mismatches {
            println!("STRUCTURE-GENERATION FAIL: {line}");
        }
        Err(format!(
            "STRUCTURE-GENERATION RESULT: {} mismatches",
            mismatch_count
        ))
    }
}
