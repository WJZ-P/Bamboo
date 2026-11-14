//! Jigsaw 官方对拍（Rust 侧）。
//!
//! `jigsaw` 子命令跑 `JigsawOracle.java`：Java 端造真实的
//! `StructureTemplateManager` 和 `Structure.GenerationContext`，这里用生产代码
//! `mc-bamboo-worldgen::structure::jigsaw` 复算并逐字段比较。
//!
//! - `templates`：验证每个模板被 `StructureTemplateManager` 正确加载（size、
//!   save() 的 blocks 数、getJigsaws 的拼图方块数）。
//! - `pieces`：验证 `findGenerationPoint` + `addPieces` 的**中心结构件**
//!   （RandomTemplate / Rotation / 包围盒 / project_start_to_heightmap / stub 位置）。
//!   递归出的其余结构件由后续里程碑验证。
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
        jigsaw::{GenerationPoint, JigsawStructures, TemplatePools, center_piece},
        template::StructureTemplate,
    },
};
use serde_json::Value;

/// 本轮要对的五类 jigsaw 结构（村庄五种 + 前哨站 + 古城 + 试炼密室 + 古迹废墟）。
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
        "pieces" => run_pieces(command, work_dir, options.get(1), options.get(2)),
        other => Err(format!("unknown jigsaw mode {other}")),
    }
}

/// 全部单 palette 模板 id（blob 索引已按名字排序）。
fn template_queries() -> Vec<String> {
    blob::ENTRIES
        .iter()
        .filter(|(name, _, _)| {
            StructureTemplate::load(name).is_ok_and(|template| template.palettes.len() == 1)
        })
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
            template.jigsaws.len()
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
        .unwrap_or(3)
        .clamp(1, SEEDS.len());
    let chunks = chunks_arg
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(3)
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
/// 本轮比 Q 的 status/stub 位置和 P[idx=0] 的中心结构件字段（不含 n、njunc）。
fn compare_pieces(stdout: &str, queries: &[Query]) -> Result<(), String> {
    let mut java_q: HashMap<String, Vec<String>> = HashMap::new();
    let mut java_p0: HashMap<String, Vec<String>> = HashMap::new();
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
            "P" if fields[5] == "0" => {
                java_p0.insert(key, fields[6..].iter().map(|s| (*s).to_owned()).collect());
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
            let result = center_piece(
                config,
                &pools,
                &generator,
                allowed,
                dims,
                query.seed,
                (query.cx, query.cz),
            )?;
            checked += 1;
            let got_p0 = java_p0.get(&key);
            if let Some(diff) = expected_fields(&result, got_p0, q)? {
                mismatches += 1;
                if first.is_none() {
                    first = Some(format!("{key}: {diff}"));
                }
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
            "{mismatches}/{checked} center pieces differ; first: {}",
            first.unwrap_or_default()
        ));
    }
    println!("jigsaw center pieces: {checked} queries, all Q/stub/center-piece fields identical");
    Ok(())
}

/// 用生产 `center_piece` 的结果比 Q 行与 P[0] 行；返回第一个差异描述。
fn expected_fields(
    result: &Option<GenerationPoint>,
    java_p0: Option<&Vec<String>>,
    java_q: &[String],
) -> Result<Option<String>, String> {
    let status_ok = matches!(
        (result, java_q.first().map(String::as_str)),
        (None, Some("0")) | (Some(_), Some("1"))
    );
    if !status_ok {
        return Ok(Some(format!(
            "status mismatch: rust_some={} java_status={:?}",
            result.is_some(),
            java_q.first()
        )));
    }
    let Some(point) = result else {
        if java_p0.is_some() {
            return Ok(Some(
                "java has a P0 line but rust returned empty".to_owned(),
            ));
        }
        return Ok(None);
    };
    let q_want = [
        point.position.0.to_string(),
        point.position.1.to_string(),
        point.position.2.to_string(),
    ];
    for (index, want) in q_want.iter().enumerate() {
        if java_q.get(index + 1).is_none_or(|got| got != want) {
            return Ok(Some(format!(
                "Q stub[{}]: rust {want} java {:?}",
                index,
                java_q.get(index + 1)
            )));
        }
    }
    let p0 = java_p0.ok_or("java missing P[0] for non-empty stub")?;
    let center = &point.center;
    let want = vec![
        center.element.type_name().to_owned(),
        center.element.location().unwrap_or("-").to_owned(),
        center.position.0.to_string(),
        center.position.1.to_string(),
        center.position.2.to_string(),
        center.rotation.index().to_string(),
        center.bounding_box.min_x.to_string(),
        center.bounding_box.min_y.to_string(),
        center.bounding_box.min_z.to_string(),
        center.bounding_box.max_x.to_string(),
        center.bounding_box.max_y.to_string(),
        center.bounding_box.max_z.to_string(),
        center.ground_level_delta.to_string(),
    ];
    for (index, want) in want.iter().enumerate() {
        if p0.get(index).is_none_or(|got| got != want) {
            return Ok(Some(format!(
                "P0[{}]: rust {want} java {:?}",
                index,
                p0.get(index)
            )));
        }
    }
    Ok(None)
}
