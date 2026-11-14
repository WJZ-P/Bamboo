//! Jigsaw 官方对拍（Rust 侧）。
//!
//! `jigsaw` 子命令跑 `JigsawOracle.java`：Java 端造真实的
//! `StructureTemplateManager` 和 `Structure.GenerationContext`，这里用生产代码
//! `mc-bamboo-worldgen::structure::jigsaw` 复算并逐字段比较。
//!
//! - `templates`：验证每个模板被 `StructureTemplateManager` 正确加载（size、
//!   save() 的 blocks 数、getJigsaws 的拼图方块数）。
//! - `pieces`：验证 `findGenerationPoint` + `addPieces` 的**全部结构件**
//!   （RandomTemplate / Rotation / 包围盒 / project_start_to_heightmap /
//!   VoxelShape 空间检查 / junction 数）。
//! - `shapes`：独立的 `Shapes` AABB 布尔运算小场景（见 `jigsaw_shapes`）。
//!
//! Rust side of the vanilla jigsaw oracle. `JigsawOracle.java` builds a real
//! `StructureTemplateManager` and `GenerationContext`; the production
//! `structure::jigsaw` recomputes and compares field by field.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::Path,
    process::Command,
};

use mc_bamboo_data::{structures as blob, worldgen::WORLDGEN};
use mc_bamboo_worldgen::{
    Worldgen,
    chunk::Generator,
    structure::{
        Structures, allowed_biome_names,
        jigsaw::{JigsawStructures, Rotation, StructurePieces, TemplatePools, add_pieces},
        template::StructureTemplate,
    },
};
use serde_json::Value;

/// 本轮 `pieces` 对拍的结构（村庄五种 + 前哨站 + 古城 + 试炼密室 + 古迹废墟）。
const STRUCTURES: [&str; 9] = [
    "minecraft:village_plains",
    "minecraft:village_desert",
    "minecraft:village_savanna",
    "minecraft:village_snowy",
    "minecraft:village_taiga",
    "minecraft:pillager_outpost",
    "minecraft:ancient_city",
    "minecraft:trial_chambers",
    "minecraft:trail_ruins",
];
const SEEDS: [i64; 6] = [0, 1, -1, 42, 12_345_678_901, -98_765_432_123];
const CHUNKS: [(i32, i32); 6] = [(0, 0), (1, 1), (-5, 7), (34, -34), (12, -3), (-40, 40)];

pub fn run(command: Command, work_dir: &Path, options: &[&str]) -> Result<(), String> {
    match options.first().copied().unwrap_or("templates") {
        "templates" => run_templates(command, work_dir),
        "jigsaws" => run_jigsaws(command, work_dir),
        "pieces" => run_pieces(command, work_dir, options.get(1), options.get(2)),
        "shapes" => crate::jigsaw_shapes::run(command, work_dir, options.get(1)),
        other => Err(format!("unknown jigsaw mode {other}")),
    }
}

/// 全部模板 id（多 palette 的也要，才能验证 `getRandomPalette`）。
fn template_queries() -> Vec<String> {
    blob::ENTRIES
        .iter()
        .map(|(name, _, _)| (*name).to_owned())
        .collect()
}

fn run_templates(mut command: Command, work_dir: &Path) -> Result<(), String> {
    let queries_path = work_dir.join("jigsaw_queries.txt");
    let ids = template_queries();
    fs::write(&queries_path, ids.join("\n"))
        .map_err(|error| format!("cannot write queries: {error}"))?;
    let output = command
        .arg("templates")
        .arg(&queries_path)
        .output()
        .map_err(|error| format!("cannot run JigsawOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "JigsawOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare_templates(&String::from_utf8_lossy(&output.stdout), ids.len())
}

fn compare_templates(stdout: &str, expected_count: usize) -> Result<(), String> {
    let mut checked = 0usize;
    let mut mismatches = 0usize;
    let mut first: Option<(String, String)> = None;
    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let id = line.split('|').next().unwrap_or("");
        let template =
            StructureTemplate::load(id).map_err(|error| format!("cannot load {id}: {error}"))?;
        let want = format!(
            "{id}|{}|{}|{}|{}|{}",
            template.size.0,
            template.size.1,
            template.size.2,
            template.blocks.len(),
            template.palette_jigsaws.first().map_or(0, Vec::len)
        );
        checked += 1;
        if line != want {
            mismatches += 1;
            if first.is_none() {
                first = Some((line.to_owned(), want));
            }
        }
    }
    if checked != expected_count {
        return Err(format!(
            "Java reported {checked} templates, expected {expected_count}"
        ));
    }
    if mismatches > 0 {
        let (got, want) = first.unwrap_or_default();
        return Err(format!(
            "{mismatches}/{checked} templates differ\n  java: {got}\n  rust: {want}"
        ));
    }
    println!("jigsaw templates: {checked} templates, size/blocks/jigsaws all identical");
    Ok(())
}

/// 每个模板 × 四种旋转 × 几个位置，验证 `getJigsaws` 的 palette 选择与旋转。
///
/// 位置既包含 `(0,0,0)`（`getRandom` 的种子是 0）也包含非零坐标，覆盖多 palette 的
/// 全部选择分支。
///
/// Every template times four rotations times a few positions, verifying the palette
/// choice and rotation of `getJigsaws`. Positions include `(0,0,0)` (seed 0) and
/// non-zero ones so that every multi-palette branch is exercised.
const JIGSAW_POSITIONS: [(i32, i32, i32); 4] =
    [(0, 0, 0), (1, 2, 3), (-7, 64, 11), (100, -20, -33)];

fn run_jigsaws(mut command: Command, work_dir: &Path) -> Result<(), String> {
    let queries_path = work_dir.join("jigsaw_jigsaw_queries.txt");
    let mut queries = Vec::new();
    for &(name, _, _) in blob::ENTRIES {
        for rotation in 0..4 {
            for (x, y, z) in JIGSAW_POSITIONS {
                queries.push(format!("{name}|{rotation}|{x}|{y}|{z}"));
            }
        }
    }
    fs::write(&queries_path, queries.join("\n"))
        .map_err(|error| format!("cannot write queries: {error}"))?;
    let output = command
        .arg("jigsaws")
        .arg(&queries_path)
        .output()
        .map_err(|error| format!("cannot run JigsawOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "JigsawOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare_jigsaws(&String::from_utf8_lossy(&output.stdout), &queries)
}

/// Java 的 `J|key|n` 行给数量，`K|key|...` 行给每个拼图方块；逐条用 Rust 重算比较。
///
/// Java emits `J|key|n` for the count and `K|key|...` for each jigsaw block; every
/// line is recomputed on the Rust side and compared.
fn compare_jigsaws(stdout: &str, queries: &[String]) -> Result<(), String> {
    // `J|id|rot|x|y|z|n` 的 key 是前 5 段；`K|id|rot|x|y|z|...` 同样。
    // The key of both `J|id|rot|x|y|z|n` and `K|id|rot|x|y|z|...` is its first five fields.
    fn split_key<'a>(fields: &[&'a str]) -> Option<(String, Vec<&'a str>)> {
        if fields.len() < 6 {
            return None;
        }
        Some((fields[1..6].join("|"), fields[6..].to_vec()))
    }
    let mut counts: HashMap<String, String> = HashMap::new();
    let mut blocks: HashMap<String, Vec<String>> = HashMap::new();
    for line in stdout.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        match fields.first().copied() {
            Some("J") => {
                if let Some((key, rest)) = split_key(&fields) {
                    counts.insert(key, rest.first().copied().unwrap_or("").to_owned());
                }
            }
            Some("K") => {
                if let Some((key, rest)) = split_key(&fields) {
                    blocks.entry(key).or_default().push(rest.join("|"));
                }
            }
            _ => {}
        }
    }

    let mut checked = 0usize;
    let mut jigsaws_checked = 0usize;
    for query in queries {
        let (id, rest) = query.split_once('|').ok_or("bad jigsaw query")?;
        let parts: Vec<i32> = rest
            .split('|')
            .map(|value| value.parse::<i32>())
            .collect::<Result<_, _>>()
            .map_err(|error| format!("bad jigsaw query {query}: {error}"))?;
        let [rotation, x, y, z] = parts[..] else {
            return Err(format!("bad jigsaw query {query}"));
        };
        let template =
            StructureTemplate::load(id).map_err(|error| format!("cannot load {id}: {error}"))?;
        let rotation = Rotation::from_index(rotation);
        let want = template.jigsaws_at((x, y, z));
        let got_count = counts
            .get(query.as_str())
            .map(String::as_str)
            .unwrap_or("-");
        if got_count != want.len().to_string() {
            return Err(format!(
                "{query}: jigsaw count rust {} java {got_count}",
                want.len()
            ));
        }
        let empty: Vec<String> = Vec::new();
        let got_lines = blocks.get(query.as_str()).unwrap_or(&empty);
        if got_lines.len() != want.len() {
            return Err(format!(
                "{query}: jigsaw lines rust {} java {}",
                want.len(),
                got_lines.len()
            ));
        }
        for (jigsaw, line) in want.iter().zip(got_lines) {
            let local = rotation.transform(jigsaw.pos);
            let want_fields = [
                (local.0 + x).to_string(),
                (local.1 + y).to_string(),
                (local.2 + z).to_string(),
                rotation
                    .rotate_direction(jigsaw.front().ok_or("bad orientation")?)
                    .name()
                    .to_owned(),
                rotation
                    .rotate_direction(jigsaw.top().ok_or("bad orientation")?)
                    .name()
                    .to_owned(),
                jigsaw.name.clone(),
                jigsaw.target.clone(),
                jigsaw.pool.clone(),
                jigsaw.joint_name().to_owned(),
                jigsaw.placement_priority.to_string(),
                jigsaw.selection_priority.to_string(),
            ];
            // `line` 此时不含 key 前缀，逐字段比。
            // `line` has no key prefix here, so compare field by field.
            let got_fields: Vec<&str> = line.split('|').collect();
            for (index, want) in want_fields.iter().enumerate() {
                if got_fields.get(index).is_none_or(|got| *got != want) {
                    return Err(format!(
                        "{query}: jigsaw field {index} rust {want} java {:?}",
                        got_fields.get(index)
                    ));
                }
            }
            jigsaws_checked += 1;
        }
        checked += 1;
    }
    println!(
        "jigsaw blocks: {checked} template/rotation/position queries, \
         {jigsaws_checked} jigsaw blocks, all fields identical"
    );
    Ok(())
}

struct Query {
    id: String,
    seed: i64,
    cx: i32,
    cz: i32,
}
impl Query {
    fn key(&self) -> String {
        format!("{}|{}|{}|{}", self.id, self.seed, self.cx, self.cz)
    }
}

fn build_queries(seeds: usize, chunks: usize) -> Vec<Query> {
    let mut queries = Vec::new();
    for &seed in &SEEDS[..seeds] {
        for &(cx, cz) in &CHUNKS[..chunks] {
            for &id in &STRUCTURES {
                queries.push(Query {
                    id: id.to_owned(),
                    seed,
                    cx,
                    cz,
                });
            }
        }
    }
    queries
}

fn run_pieces(
    mut command: Command,
    work_dir: &Path,
    seeds_arg: Option<&&str>,
    chunks_arg: Option<&&str>,
) -> Result<(), String> {
    let seeds = seeds_arg
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(2)
        .clamp(1, SEEDS.len());
    let chunks = chunks_arg
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(2)
        .clamp(1, CHUNKS.len());
    let queries = build_queries(seeds, chunks);
    let queries_path = work_dir.join("jigsaw_piece_queries.txt");
    let text = queries
        .iter()
        .map(|query| format!("{}|{}|{}|{}", query.id, query.seed, query.cx, query.cz))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&queries_path, text).map_err(|error| format!("cannot write queries: {error}"))?;

    let output = command
        .arg("pieces")
        .arg(&queries_path)
        .output()
        .map_err(|error| format!("cannot run JigsawOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "JigsawOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare_pieces(&String::from_utf8_lossy(&output.stdout), &queries)
}

/// Java 的 Q 行 `Q|key|status|sx|sy|sz|n`，P 行 `P|key|idx|type|loc|pos(3)|rot|bb(6)|gld|njunc`。
/// 全部结构件逐项比（含 npieces、njunc）；junction 详情由后续里程碑补 J 行。
fn compare_pieces(stdout: &str, queries: &[Query]) -> Result<(), String> {
    let mut java_q: HashMap<String, Vec<String>> = HashMap::new();
    let mut java_p: HashMap<String, BTreeMap<usize, Vec<String>>> = HashMap::new();
    for line in stdout.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() < 6 {
            continue;
        }
        let key = format!("{}|{}|{}|{}", fields[1], fields[2], fields[3], fields[4]);
        match fields[0] {
            "Q" => {
                java_q.insert(key, fields[5..].iter().map(|s| (*s).to_owned()).collect());
            }
            "P" => {
                let index = fields[5]
                    .parse::<usize>()
                    .map_err(|error| format!("bad P index: {error}"))?;
                java_p
                    .entry(key)
                    .or_default()
                    .insert(index, fields[6..].iter().map(|s| (*s).to_owned()).collect());
            }
            _ => {}
        }
    }

    let worldgen = Worldgen::load()?;
    let settings = worldgen
        .settings("minecraft:overworld")
        .ok_or("missing overworld settings")?;
    let dims = (settings.min_y, settings.height, settings.sea_level);
    let raw: Value =
        serde_json::from_str(WORLDGEN).map_err(|error| format!("bad worldgen.json: {error}"))?;
    let structures = Structures::parse(raw.get("structure").ok_or("missing structure")?)?;
    let configs = JigsawStructures::parse(raw.get("structure").ok_or("missing structure")?)?;
    let pools = TemplatePools::parse(raw.get("template_pool").ok_or("missing template_pool")?)?;

    let mut allowed_cache: HashMap<String, HashSet<String>> = HashMap::new();
    for id in &STRUCTURES {
        let def = structures
            .get(id)
            .ok_or_else(|| format!("missing structure def {id}"))?;
        allowed_cache.insert((*id).to_owned(), allowed_biome_names(def)?);
    }

    // 按种子分组，每个种子只建一次 Generator（噪声/含水层）。
    let mut by_seed: BTreeMap<i64, Vec<&Query>> = BTreeMap::new();
    for query in queries {
        by_seed.entry(query.seed).or_default().push(query);
    }

    let mut checked = 0usize;
    let mut pieces_checked = 0usize;
    let mut mismatches = 0usize;
    let mut first: Option<String> = None;
    for (seed, group) in &by_seed {
        let generator = Generator::new(&worldgen, *seed)?;
        for query in group {
            let key = query.key();
            let q = java_q
                .get(&key)
                .ok_or_else(|| format!("Java missing Q for {key}"))?;
            let config = configs
                .get(&query.id)
                .ok_or_else(|| format!("missing jigsaw config {}", query.id))?;
            let allowed = &allowed_cache[&query.id];
            let result = add_pieces(
                config,
                &pools,
                &generator,
                allowed,
                dims,
                query.seed,
                (query.cx, query.cz),
            )?;
            checked += 1;
            let got_p = java_p.get(&key);
            if let Some(diff) = check_query(&result, q, got_p)? {
                mismatches += 1;
                if first.is_none() {
                    first = Some(format!("{key}: {diff}"));
                }
            }
            if let Some(structure) = &result {
                pieces_checked += structure.pieces.len();
            }
        }
    }

    if checked != queries.len() {
        return Err(format!(
            "checked {checked} queries, expected {}",
            queries.len()
        ));
    }
    if mismatches > 0 {
        return Err(format!(
            "{mismatches}/{checked} queries differ; first: {}",
            first.unwrap_or_default()
        ));
    }
    println!("jigsaw pieces: {checked} queries, {pieces_checked} pieces, all Q/P fields identical");
    Ok(())
}

/// 用生产 `add_pieces` 的结果比 Q 行与全部 P 行；返回第一个差异描述。
fn check_query(
    result: &Option<StructurePieces>,
    q: &[String],
    java_p: Option<&BTreeMap<usize, Vec<String>>>,
) -> Result<Option<String>, String> {
    let status_ok = matches!(
        (result, q.first().map(String::as_str)),
        (None, Some("0")) | (Some(_), Some("1"))
    );
    if !status_ok {
        return Ok(Some(format!(
            "status mismatch: rust_some={} java_status={:?}",
            result.is_some(),
            q.first()
        )));
    }
    let Some(structure) = result else {
        if q.get(4).is_none_or(|n| n == "0") && java_p.is_none_or(BTreeMap::is_empty) {
            return Ok(None);
        }
        return Ok(Some("empty stub but Java has pieces".to_owned()));
    };
    let q_want = [
        structure.position.0.to_string(),
        structure.position.1.to_string(),
        structure.position.2.to_string(),
        structure.pieces.len().to_string(),
    ];
    for (index, want) in q_want.iter().enumerate() {
        if q.get(index + 1).is_none_or(|got| got != want) {
            return Ok(Some(format!(
                "Q[{}]: rust {want} java {:?}",
                index + 1,
                q.get(index + 1)
            )));
        }
    }
    let lines = java_p.ok_or("Java missing P lines for non-empty stub")?;
    if lines.len() != structure.pieces.len() {
        return Ok(Some(format!(
            "piece count: rust {} java {}",
            structure.pieces.len(),
            lines.len()
        )));
    }
    for (index, piece) in structure.pieces.iter().enumerate() {
        let line = lines
            .get(&index)
            .ok_or_else(|| format!("Java missing P[{index}]"))?;
        let want = vec![
            piece.element.type_name().to_owned(),
            piece.element.location().unwrap_or("-").to_owned(),
            piece.position.0.to_string(),
            piece.position.1.to_string(),
            piece.position.2.to_string(),
            piece.rotation.index().to_string(),
            piece.bounding_box.min_x.to_string(),
            piece.bounding_box.min_y.to_string(),
            piece.bounding_box.min_z.to_string(),
            piece.bounding_box.max_x.to_string(),
            piece.bounding_box.max_y.to_string(),
            piece.bounding_box.max_z.to_string(),
            piece.ground_level_delta.to_string(),
            piece.junctions.len().to_string(),
        ];
        for (field, want) in want.iter().enumerate() {
            if line.get(field).is_none_or(|got| got != want) {
                return Ok(Some(format!(
                    "P[{index}][{field}]: rust {want} java {:?}",
                    line.get(field)
                )));
            }
        }
    }
    Ok(None)
}
