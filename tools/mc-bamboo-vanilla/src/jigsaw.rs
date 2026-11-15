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

use mc_bamboo_blocks::LegacyRandom;
use mc_bamboo_data::{structures as blob, worldgen::WORLDGEN};
use mc_bamboo_nbt::Tag;
use mc_bamboo_worldgen::random::Random;
use mc_bamboo_worldgen::{
    Worldgen,
    chunk::Generator,
    settings::SeededRouter,
    structure::{
        BoundingBox, Structures, allowed_biome_names,
        jigsaw::{
            JigsawConfig, JigsawStructures, Piece, Rotation, StructurePieces, TemplatePools,
            add_pieces,
        },
        processor::{BlockSetRef, BlockStateSpec, Processor, ProcessorLists},
        processor_apply::{self, ProcessBlock, ProcessorLevel},
        rings::BiomeResolver,
        template::StructureTemplate,
        validity,
    },
};
use serde_json::Value;

/// 本轮 `pieces` 对拍的结构：村庄五种 + 前哨站 + 古城 + 试炼密室 + 古迹废墟 +
/// 26.3 新增的全部 18 种 `abandoned_camp`。`bastion_remnant` 是下界结构，本轮不做。
///
/// Structures in the `pieces` oracle: five villages, pillager outpost, ancient city,
/// trial chambers, trail ruins and all 18 `abandoned_camp` structures added in 26.3.
/// `bastion_remnant` is a nether structure and out of scope.
const STRUCTURES: [&str; 27] = [
    "minecraft:village_plains",
    "minecraft:village_desert",
    "minecraft:village_savanna",
    "minecraft:village_snowy",
    "minecraft:village_taiga",
    "minecraft:pillager_outpost",
    "minecraft:ancient_city",
    "minecraft:trial_chambers",
    "minecraft:trail_ruins",
    "minecraft:abandoned_camp_bamboo_jungle",
    "minecraft:abandoned_camp_birch_forest",
    "minecraft:abandoned_camp_cherry_grove",
    "minecraft:abandoned_camp_dappled_forest",
    "minecraft:abandoned_camp_flower_forest",
    "minecraft:abandoned_camp_forest",
    "minecraft:abandoned_camp_meadow",
    "minecraft:abandoned_camp_old_growth_birch_forest",
    "minecraft:abandoned_camp_old_growth_pine_taiga",
    "minecraft:abandoned_camp_old_growth_spruce_taiga",
    "minecraft:abandoned_camp_pale_garden",
    "minecraft:abandoned_camp_savanna",
    "minecraft:abandoned_camp_snowy_taiga",
    "minecraft:abandoned_camp_sparse_jungle",
    "minecraft:abandoned_camp_swamp",
    "minecraft:abandoned_camp_taiga",
    "minecraft:abandoned_camp_windswept_forest",
    "minecraft:abandoned_camp_wooded_badlands",
];
const SEEDS: [i64; 6] = [0, 1, -1, 42, 12_345_678_901, -98_765_432_123];
const CHUNKS: [(i32, i32); 6] = [(0, 0), (1, 1), (-5, 7), (34, -34), (12, -3), (-40, 40)];

pub fn run(command: Command, work_dir: &Path, options: &[&str]) -> Result<(), String> {
    match options.first().copied().unwrap_or("templates") {
        "templates" => run_templates(command, work_dir),
        "jigsaws" => run_jigsaws(command, work_dir),
        "pieces" => run_pieces(command, work_dir, options.get(1), options.get(2)),
        "pools" => run_pools(command, work_dir),
        "save" => run_save(command, work_dir),
        "processors" => run_processors(command, work_dir),
        "load" => run_load(command, work_dir),
        "apply" => run_apply(command, work_dir),
        "capped" => run_capped(command, work_dir),
        "cappedfull" => run_capped_full(command, work_dir),
        "placepiece" => run_place_piece(command, work_dir),
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

/// `pools` 模式的查询：每个 template pool × 每组 (seed, chunk)。
///
/// `pools` mode queries: every template pool × each (seed, chunk).
fn run_pools(mut command: Command, work_dir: &Path) -> Result<(), String> {
    let raw: Value =
        serde_json::from_str(WORLDGEN).map_err(|error| format!("bad worldgen.json: {error}"))?;
    let pools = TemplatePools::parse(raw.get("template_pool").ok_or("missing template_pool")?)?;
    let mut names = pools.names().cloned().collect::<Vec<_>>();
    names.sort();

    let mut queries = Vec::new();
    for &seed in &SEEDS[..2] {
        for &(cx, cz) in &CHUNKS[..2] {
            for name in &names {
                queries.push(format!("{name}|{seed}|{cx}|{cz}"));
            }
        }
    }
    let queries_path = work_dir.join("jigsaw_pool_queries.txt");
    fs::write(&queries_path, queries.join("\n"))
        .map_err(|error| format!("cannot write queries: {error}"))?;
    let output = command
        .arg("pools")
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
    compare_pools(&String::from_utf8_lossy(&output.stdout), &queries)
}

/// Java 的 Q 行 `Q|key|status|sx|sy|sz|n`，P 行 `P|key|idx|type|loc|pos(3)|rot|bb(6)|gld|...`。
fn compare_pools(stdout: &str, queries: &[String]) -> Result<(), String> {
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
    let pools = TemplatePools::parse(raw.get("template_pool").ok_or("missing template_pool")?)?;
    let allowed = HashSet::new();

    let mut by_seed: BTreeMap<i64, Vec<&String>> = BTreeMap::new();
    for query in queries {
        let fields: Vec<&str> = query.split('|').collect();
        let seed = fields[1]
            .parse::<i64>()
            .map_err(|error| format!("bad pool query seed: {error}"))?;
        by_seed.entry(seed).or_default().push(query);
    }

    let mut checked = 0usize;
    let mut pieces_checked = 0usize;
    let mut mismatches = 0usize;
    let mut first: Option<String> = None;
    let mut type_counts: BTreeMap<String, usize> = BTreeMap::new();
    for (seed, group) in &by_seed {
        let generator = Generator::new(&worldgen, *seed)?;
        for query in group {
            let mut fields = query.split('|');
            let pool = fields.next().ok_or("empty pool query")?;
            let _ = fields.next();
            let cx: i32 = fields
                .next()
                .and_then(|value| value.parse().ok())
                .ok_or("bad pool query chunk x")?;
            let cz: i32 = fields
                .next()
                .and_then(|value| value.parse().ok())
                .ok_or("bad pool query chunk z")?;
            let config = JigsawConfig::for_pool(pool, 64);
            let result = add_pieces(&config, &pools, &generator, &allowed, dims, *seed, (cx, cz))?;
            checked += 1;
            let q = java_q
                .get(*query)
                .ok_or_else(|| format!("Java missing Q for {query}"))?;
            if let Some(diff) = check_query(&result, true, q, java_p.get(*query))? {
                mismatches += 1;
                if first.is_none() {
                    first = Some(format!("{query}: {diff}"));
                }
            }
            if let Some(structure) = &result {
                pieces_checked += structure.pieces.len();
                for piece in &structure.pieces {
                    *type_counts
                        .entry(piece.element.type_name().to_owned())
                        .or_default() += 1;
                }
            }
        }
    }

    if checked != queries.len() {
        return Err(format!(
            "checked {checked} pools, expected {}",
            queries.len()
        ));
    }
    if mismatches > 0 {
        return Err(format!(
            "{mismatches}/{checked} pools differ; first: {}",
            first.unwrap_or_default()
        ));
    }
    let summary = type_counts
        .iter()
        .map(|(name, count)| format!("{name}={count}"))
        .collect::<Vec<_>>()
        .join(" ");
    println!("jigsaw pools: {checked} pools, {pieces_checked} pieces, all Q/P fields identical");
    println!("pool element types: {summary}");
    Ok(())
}

/// `save` 模式的查询与 `pieces` 相同（结构 × 种子 × 区块），比的是官方 `createTag`
/// 编出的完整 Children 标签。
///
/// `save` mode queries match `pieces` (structure × seed × chunk) but compare the full
/// Children tag produced by vanilla `createTag`.
fn run_save(mut command: Command, work_dir: &Path) -> Result<(), String> {
    let queries = build_queries(2, 2);
    let queries_path = work_dir.join("jigsaw_save_queries.txt");
    let text = queries
        .iter()
        .map(|query| format!("{}|{}|{}|{}", query.id, query.seed, query.cx, query.cz))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&queries_path, text).map_err(|error| format!("cannot write queries: {error}"))?;
    let output = command
        .arg("save")
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
    compare_save(&String::from_utf8_lossy(&output.stdout), &queries)
}

/// Java 的 `Q|key|status|n` 给状态和件数，`T|key|idx|<canonical JSON>` 给每个 Children 条目。
///
/// Java emits `Q|key|status|n` for status and count, `T|key|idx|<canonical JSON>` per
/// Children entry.
fn compare_save(stdout: &str, queries: &[Query]) -> Result<(), String> {
    let mut java_q: HashMap<String, Vec<String>> = HashMap::new();
    let mut java_t: HashMap<String, BTreeMap<usize, Value>> = HashMap::new();
    for line in stdout.lines() {
        // key 是 `id|seed|cx|cz` 四段，后面再跟 idx 和 JSON，所以最多切 7 段。
        // The key is four fields (`id|seed|cx|cz`) followed by the index and the JSON,
        // so split into at most seven parts.
        let fields: Vec<&str> = line.splitn(7, '|').collect();
        if fields.len() < 6 {
            continue;
        }
        let key = format!("{}|{}|{}|{}", fields[1], fields[2], fields[3], fields[4]);
        match fields[0] {
            "Q" => {
                java_q.insert(key, fields[5..].iter().map(|s| (*s).to_owned()).collect());
            }
            "T" => {
                let index = fields[5]
                    .parse::<usize>()
                    .map_err(|error| format!("bad T index: {error}"))?;
                let json: Value = serde_json::from_str(fields[6])
                    .map_err(|error| format!("bad T json: {error}"))?;
                java_t.entry(key).or_default().insert(index, json);
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

    let mut by_seed: BTreeMap<i64, Vec<&Query>> = BTreeMap::new();
    for query in queries {
        by_seed.entry(query.seed).or_default().push(query);
    }

    let mut checked = 0usize;
    let mut tags_checked = 0usize;
    let mut mismatches = 0usize;
    let mut first: Option<String> = None;
    for (seed, group) in &by_seed {
        let generator = Generator::new(&worldgen, *seed)?;
        for query in group {
            let key = query.key();
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
            let got = java_q
                .get(&key)
                .ok_or_else(|| format!("Java missing Q for {key}"))?;
            let want_status = if result.is_some() { "1" } else { "0" };
            if got.first().map(String::as_str) != Some(want_status) {
                mismatches += 1;
                if first.is_none() {
                    first = Some(format!(
                        "{key}: stub status rust {want_status} java {got:?}"
                    ));
                }
                continue;
            }
            let Some(structure) = &result else {
                continue;
            };
            if got.get(1).map(String::as_str) != Some(&structure.pieces.len().to_string()) {
                mismatches += 1;
                if first.is_none() {
                    first = Some(format!(
                        "{key}: piece count rust {} java {:?}",
                        structure.pieces.len(),
                        got.get(1)
                    ));
                }
                continue;
            }
            let lines = java_t.get(&key);
            for (index, piece) in structure.pieces.iter().enumerate() {
                let want = canonical_tag(&piece.to_full_save_piece()?.to_tag());
                let have = lines
                    .and_then(|lines| lines.get(&index))
                    .ok_or_else(|| format!("Java missing T[{index}] for {key}"))?;
                tags_checked += 1;
                if &want != have {
                    mismatches += 1;
                    if first.is_none() {
                        first = Some(format!("{key}: T[{index}] rust {want} java {have}"));
                    }
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
            "{mismatches}/{checked} queries differ; first: {}",
            first.unwrap_or_default()
        ));
    }
    println!("jigsaw save: {checked} queries, {tags_checked} piece tags, all fields identical");
    Ok(())
}

/// 和 `structure_save::canonical_tag` 同一套规范化（type + 按键排序的 values）。
/// 浮点用位模式（`#xxxxxxxx`），不用文本：`0.0001` 与 `1.0E-4` 是同一个值但 Java 与
/// Rust 的默认格式化不同，比文本会假失败。
///
/// Same normalisation as `structure_save::canonical_tag` (type plus key-sorted values).
/// Floats use their bit pattern (`#xxxxxxxx`) rather than text: `0.0001` and `1.0E-4` are
/// the same value but Java and Rust format them differently, so text would differ falsely.
fn canonical_tag(tag: &mc_bamboo_nbt::Tag) -> Value {
    use mc_bamboo_nbt::Tag;
    match tag {
        Tag::Byte(v) => serde_json::json!({"type": 1, "value": v.to_string()}),
        Tag::Short(v) => serde_json::json!({"type": 2, "value": v.to_string()}),
        Tag::Int(v) => serde_json::json!({"type": 3, "value": v.to_string()}),
        Tag::Long(v) => serde_json::json!({"type": 4, "value": v.to_string()}),
        Tag::Float(v) => serde_json::json!({"type": 5, "value": format!("#{:08x}", v.to_bits())}),
        Tag::Double(v) => {
            serde_json::json!({"type": 6, "value": format!("#{:016x}", v.to_bits())})
        }
        Tag::String(v) => serde_json::json!({"type": 8, "value": v}),
        Tag::ByteArray(v) => serde_json::json!({"type": 7, "values": v}),
        Tag::IntArray(v) => serde_json::json!({"type": 11, "values": v}),
        Tag::LongArray(v) => serde_json::json!({"type": 12, "values": v}),
        Tag::List(v) => serde_json::json!({
            "type": 9,
            "values": v.iter().map(canonical_tag).collect::<Vec<_>>()
        }),
        Tag::Compound(entries) => {
            let mut sorted: Vec<_> = entries.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            let mut map = serde_json::Map::new();
            for (key, value) in sorted {
                map.insert(key.clone(), canonical_tag(value));
            }
            serde_json::json!({"type": 10, "values": Value::Object(map)})
        }
    }
}

/// `processors` 模式：不带 queries，把 40 个 `processor_list` 用官方 `DIRECT_CODEC`
/// 编码后逐键比 Rust 侧的 `Processor::to_tag`。
///
/// `processors` mode: no queries; compares the official `DIRECT_CODEC` encoding of all 40
/// `processor_list` entries against the Rust `Processor::to_tag`, key by key.
fn run_processors(mut command: Command, _work_dir: &Path) -> Result<(), String> {
    let output = command
        .arg("processors")
        .output()
        .map_err(|error| format!("cannot run JigsawOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "JigsawOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare_processors(&String::from_utf8_lossy(&output.stdout))
}

/// Java 的 `P|id|<reference>|<inline>`；只比 `inline`（引用就是注册名，无信息量）。
///
/// Java emits `P|id|<reference>|<inline>`; only `inline` is compared (the reference is
/// just the registry name).
fn compare_processors(stdout: &str) -> Result<(), String> {
    let raw: Value =
        serde_json::from_str(WORLDGEN).map_err(|error| format!("bad worldgen.json: {error}"))?;
    let lists = ProcessorLists::parse(raw.get("processor_list").ok_or("missing processor_list")?)?;

    let mut checked = 0usize;
    let mut processors_checked = 0usize;
    let mut mismatches = 0usize;
    let mut first: Option<String> = None;
    for line in stdout.lines() {
        let fields: Vec<&str> = line.splitn(4, '|').collect();
        if fields.len() != 4 || fields[0] != "P" {
            continue;
        }
        let id = fields[1];
        let want: Value = serde_json::from_str(fields[3])
            .map_err(|error| format!("bad inline json for {id}: {error}"))?;
        let list = lists
            .get(id)
            .ok_or_else(|| format!("Rust missing processor list {id}"))?;
        let got = canonical_tag(&Tag::Compound(vec![(
            "processors".to_owned(),
            Tag::List(
                list.iter()
                    .map(|processor| processor.to_tag())
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        )]));
        checked += 1;
        processors_checked += list.len();
        if got != want {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!("{id}: rust {got} java {want}"));
            }
        }
    }
    if checked != lists.len() {
        return Err(format!(
            "checked {checked} processor lists, expected {}",
            lists.len()
        ));
    }
    if mismatches > 0 {
        return Err(format!(
            "{mismatches}/{checked} processor lists differ; first: {}",
            first.unwrap_or_default()
        ));
    }
    println!(
        "jigsaw processors: {checked} processor lists, {processors_checked} processors, \
         all fields identical"
    );
    Ok(())
}

/// `load` 模式：生成结构件 → `to_full_save_piece` → `Piece::from_save_tag` 读回，
/// 逐项比官方同一套「写→读」的结果（含重算的包围盒与元素编码）。
///
/// `load` mode: generate pieces, `to_full_save_piece`, then `Piece::from_save_tag`, and
/// compare each field with vanilla's own write-then-read result (including the recomputed
/// bounding box and the re-encoded element).
fn run_load(mut command: Command, work_dir: &Path) -> Result<(), String> {
    let queries = build_queries(2, 2);
    let queries_path = work_dir.join("jigsaw_load_queries.txt");
    let text = queries
        .iter()
        .map(|query| format!("{}|{}|{}|{}", query.id, query.seed, query.cx, query.cz))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&queries_path, text).map_err(|error| format!("cannot write queries: {error}"))?;
    let output = command
        .arg("load")
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
    compare_load(&String::from_utf8_lossy(&output.stdout), &queries)
}

/// Java 的 `Q|key|status|n`、`R|key|idx|...`、`A|key|reloaded`。
/// `R` 行是官方读回的字段：`pos(3)|rot|bb(6)|gld|njunc|liquid|element`。
///
/// Java emits `Q|key|status|n`, `R|key|idx|...` and `A|key|reloaded`. The `R` line holds
/// what vanilla read back: `pos(3)|rot|bb(6)|gld|njunc|liquid|element`.
fn compare_load(stdout: &str, queries: &[Query]) -> Result<(), String> {
    let mut java_q: HashMap<String, Vec<String>> = HashMap::new();
    let mut java_r: HashMap<String, BTreeMap<usize, Vec<String>>> = HashMap::new();
    for line in stdout.lines() {
        // `Q|key(4)|status|n` 与 `R|key(4)|idx|field...`：key 固定 4 段，之后才是
        // 状态或下标。
        // `Q|key(4)|status|n` and `R|key(4)|idx|field...`: the key is always four fields,
        // followed by the status or the index.
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() < 6 {
            continue;
        }
        let key = format!("{}|{}|{}|{}", fields[1], fields[2], fields[3], fields[4]);
        match fields[0] {
            "Q" => {
                java_q.insert(key, fields[5..].iter().map(|s| (*s).to_owned()).collect());
            }
            "R" => {
                let index = fields[5]
                    .parse::<usize>()
                    .map_err(|error| format!("bad R index: {error}"))?;
                java_r
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

    let mut by_seed: BTreeMap<i64, Vec<&Query>> = BTreeMap::new();
    for query in queries {
        by_seed.entry(query.seed).or_default().push(query);
    }

    let mut checked = 0usize;
    let mut reloaded = 0usize;
    let mut mismatches = 0usize;
    let mut first: Option<String> = None;
    for (seed, group) in &by_seed {
        let generator = Generator::new(&worldgen, *seed)?;
        for query in group {
            let key = query.key();
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
            let q = java_q
                .get(&key)
                .ok_or_else(|| format!("Java missing Q for {key}"))?;
            let status = if result.is_some() { "1" } else { "0" };
            if q.first().map(String::as_str) != Some(status) {
                mismatches += 1;
                if first.is_none() {
                    first = Some(format!("{key}: stub status rust {status} java {q:?}"));
                }
                continue;
            }
            let Some(structure) = &result else {
                continue;
            };
            let lines = java_r.get(&key);
            for (index, piece) in structure.pieces.iter().enumerate() {
                let save = piece.to_full_save_piece()?;
                let tag = save.to_tag();
                let Some(loaded) = Piece::from_save_tag(&tag)? else {
                    mismatches += 1;
                    if first.is_none() {
                        first = Some(format!("{key}: Rust cannot read piece {index}"));
                    }
                    continue;
                };
                let Some(line) = lines.and_then(|lines| lines.get(&index)) else {
                    mismatches += 1;
                    if first.is_none() {
                        first = Some(format!("{key}: Java missing R[{index}]"));
                    }
                    continue;
                };
                reloaded += 1;
                let want = [
                    loaded.position.0.to_string(),
                    loaded.position.1.to_string(),
                    loaded.position.2.to_string(),
                    loaded.rotation.index().to_string(),
                    loaded.bounding_box.min_x.to_string(),
                    loaded.bounding_box.min_y.to_string(),
                    loaded.bounding_box.min_z.to_string(),
                    loaded.bounding_box.max_x.to_string(),
                    loaded.bounding_box.max_y.to_string(),
                    loaded.bounding_box.max_z.to_string(),
                    loaded.ground_level_delta.to_string(),
                    loaded.junctions.len().to_string(),
                    loaded.liquid_settings.name().to_owned(),
                ];
                for (field, want) in want.iter().enumerate() {
                    if line.get(field).is_none_or(|got| got != want) {
                        mismatches += 1;
                        if first.is_none() {
                            first = Some(format!(
                                "{key}: R[{index}][{field}] rust {want} java {:?}",
                                line.get(field)
                            ));
                        }
                    }
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
            "{mismatches} differences over {checked} queries; first: {}",
            first.unwrap_or_default()
        ));
    }
    println!(
        "jigsaw load: {checked} queries, {reloaded} pieces round-tripped, all fields identical"
    );
    Ok(())
}

/// `apply` 模式：对每个 `processor_list`（以及几条合成的组合）在假世界上跑
/// `process_blocks`，与官方 `StructureTemplate.processBlockInfos` 逐方块比。
///
/// `apply` mode: runs `process_blocks` over every `processor_list` (plus a few synthetic
/// combinations) on a fake level and compares it with vanilla
/// `StructureTemplate.processBlockInfos` block by block.
fn run_apply(mut command: Command, work_dir: &Path) -> Result<(), String> {
    let raw: Value =
        serde_json::from_str(WORLDGEN).map_err(|error| format!("bad worldgen.json: {error}"))?;
    let lists = ProcessorLists::parse(raw.get("processor_list").ok_or("missing processor_list")?)?;
    let queries = build_apply_queries(&lists);
    let queries_path = work_dir.join("jigsaw_apply_queries.txt");
    fs::write(&queries_path, queries.join("\n"))
        .map_err(|error| format!("cannot write queries: {error}"))?;
    let output = command
        .arg("apply")
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
    compare_apply(&String::from_utf8_lossy(&output.stdout), &queries, &lists)
}

/// 一条 apply 查询里要处理的方块：`(pos, state)`。
///
/// The blocks of one apply query: `(pos, state)`.
type ApplyBlock = ((i32, i32, i32), String);

/// 一条 apply 查询。
///
/// One apply query.
struct ApplyQuery {
    list: String,
    piece: (i32, i32, i32),
    reference: (i32, i32, i32),
    world_state: String,
    blocks: Vec<ApplyBlock>,
    /// `settings.getBoundingBox()`：`Some` 时只有盒内的方块被处理（除非链里有
    /// `capped`）。这是覆盖 `processOnlyInCurrentChunk` 的唯一途径。
    ///
    /// `settings.getBoundingBox()`: when set, only blocks inside it are processed (unless
    /// the chain contains a `capped`). This is the only way to cover
    /// `processOnlyInCurrentChunk`.
    chunk_bb: Option<(i32, i32, i32, i32, i32, i32)>,
}

impl ApplyQuery {
    fn key(&self) -> String {
        let bb = self
            .chunk_bb
            .map(|bb| format!("{}|{}|{}|{}|{}|{}", bb.0, bb.1, bb.2, bb.3, bb.4, bb.5))
            .unwrap_or_else(|| "none".to_owned());
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{bb}",
            self.list,
            self.piece.0,
            self.piece.1,
            self.piece.2,
            self.reference.0,
            self.reference.1,
            self.reference.2,
            self.world_state,
            self.blocks.len()
        )
    }

    fn to_line(&self) -> String {
        let mut line = self.key();
        for (pos, state) in &self.blocks {
            line.push_str(&format!("|{}|{}|{}|{state}", pos.0, pos.1, pos.2));
        }
        line
    }
}

/// 每个 `processor_list` 配一组覆盖各分支的方块与两种假世界状态。
///
/// Every `processor_list` gets one block set covering each branch, with two fake world
/// states.
fn build_apply_queries(lists: &ProcessorLists) -> Vec<String> {
    // 覆盖 `rule` / `block_rot` / `protected_blocks` / `capped` 会用到的方块：
    // 深板岩砖（ancient city 规则）、可疑的沙砾（trail ruins）、黑石（bastion）、
    // 草方块与空气（对照）。
    // Blocks covering the rules of `rule` / `block_rot` / `protected_blocks` / `capped`:
    // deepslate bricks, suspicious gravel, blackstone, plus grass and air as controls.
    let states = [
        "minecraft:deepslate_bricks",
        "minecraft:cracked_deepslate_bricks",
        "minecraft:suspicious_gravel",
        "minecraft:blackstone",
        "minecraft:gilded_blackstone",
        "minecraft:polished_blackstone_bricks",
        "minecraft:grass_block[snowy=false]",
        "minecraft:air",
    ];
    let mut queries = Vec::new();
    for name in lists.names().cloned().collect::<Vec<_>>() {
        // 每组方块放在相邻位置，覆盖 `axis_aligned_linear_pos` 的距离分支。
        // Blocks sit at consecutive positions so the distance branches of
        // `axis_aligned_linear_pos` are covered.
        let blocks: Vec<ApplyBlock> = states
            .iter()
            .enumerate()
            .map(|(index, state)| {
                (
                    ((index as i32) % 4, 64 + index as i32 / 4, 0),
                    (*state).to_owned(),
                )
            })
            .collect();
        for world_state in ["minecraft:air", "minecraft:deepslate_bricks"] {
            queries.push(
                ApplyQuery {
                    list: name.clone(),
                    piece: (0, 64, 0),
                    reference: (0, 64, 0),
                    world_state: world_state.to_owned(),
                    blocks: blocks.clone(),
                    chunk_bb: None,
                }
                .to_line(),
            );
            // 带 `chunkBB`：只覆盖前半批方块，用来比 `processOnlyInCurrentChunk`
            // （链里没有 `capped` 时生效）。
            // With a `chunkBB` covering only the first half, exercising
            // `processOnlyInCurrentChunk` (effective when the chain has no `capped`).
            queries.push(
                ApplyQuery {
                    list: name.clone(),
                    piece: (0, 64, 0),
                    reference: (0, 64, 0),
                    world_state: world_state.to_owned(),
                    blocks: blocks.clone(),
                    chunk_bb: Some((0, 64, 0, 1, 65, 0)),
                }
                .to_line(),
            );
        }
        // 再换一个结构件位置，覆盖 `CappedProcessor` 按位置派生的随机源。
        // Another piece position to cover the `CappedProcessor` position-derived source.
        queries.push(
            ApplyQuery {
                list: name,
                piece: (-13, 40, 27),
                reference: (-13, 40, 27),
                world_state: "minecraft:air".to_owned(),
                blocks,
                chunk_bb: None,
            }
            .to_line(),
        );
    }
    queries
}

/// Java 的 `A|key|count` 与 `S|key|x|y|z|state`（只列保留下来的方块）。
///
/// Java emits `A|key|count` and `S|key|x|y|z|state` for the blocks that survive.
fn compare_apply(stdout: &str, queries: &[String], lists: &ProcessorLists) -> Result<(), String> {
    // Java 输出的 key 与查询行的前缀一一对应，所以先为每条查询算出它自己的 key，
    // 再按 `java` 行去掉 key 之后的剩余字段归类。这样不用猜 key 的长度。
    // Each query's key is a prefix of its own line, so compute the keys up front and key
    // the Java rows by their remainder; no need to guess the key length.
    let mut query_by_key: HashMap<String, &String> = HashMap::new();
    for line in queries {
        let fields: Vec<&str> = line.split('|').collect();
        let key_len = if fields[9] == "none" { 10 } else { 15 };
        query_by_key.insert(fields[..key_len].join("|"), line);
    }

    let mut java_a: HashMap<String, usize> = HashMap::new();
    let mut java_s: HashMap<String, Vec<String>> = HashMap::new();
    let mut java_c: HashMap<String, usize> = HashMap::new();
    let mut java_d: HashMap<String, Vec<String>> = HashMap::new();
    for line in stdout.lines() {
        let Some(row) = split_java_row(line, &query_by_key) else {
            continue;
        };
        let (key, rest) = row;
        match line.split('|').next().unwrap_or("") {
            "A" => {
                java_a.insert(key, rest.parse().unwrap_or(0));
            }
            "S" => {
                java_s.entry(key).or_default().push(rest);
            }
            "C" => {
                java_c.insert(key, rest.parse().unwrap_or(0));
            }
            "D" => {
                java_d.entry(key).or_default().push(rest);
            }
            _ => {}
        }
    }

    let mut checked = 0usize;
    let mut blocks_checked = 0usize;
    let mut mismatches = 0usize;
    let mut first: Option<String> = None;
    for line in queries {
        let fields: Vec<&str> = line.split('|').collect();
        let list_id = fields[0];
        let piece = (
            parse_i32(fields[1])?,
            parse_i32(fields[2])?,
            parse_i32(fields[3])?,
        );
        let reference = (
            parse_i32(fields[4])?,
            parse_i32(fields[5])?,
            parse_i32(fields[6])?,
        );
        let world_state = fields[7].to_owned();
        let count: usize = parse_i32(fields[8])? as usize;
        // 第 10 段（下标 9）是 `chunkBB`：`none` 或 6 个整数。
        // Field 10 (index 9) is the `chunkBB`: `none` or six ints.
        let chunk_bb = if fields[9] == "none" {
            None
        } else {
            let mut bb = [0_i32; 6];
            for (index, slot) in bb.iter_mut().enumerate() {
                *slot = parse_i32(fields[9 + index])?;
            }
            Some(BoundingBox {
                min_x: bb[0],
                min_y: bb[1],
                min_z: bb[2],
                max_x: bb[3],
                max_y: bb[4],
                max_z: bb[5],
            })
        };
        let bb_fields = if chunk_bb.is_some() { 6 } else { 1 };
        let mut blocks = Vec::with_capacity(count);
        let mut at = 9 + bb_fields;
        for _ in 0..count {
            let pos = (
                parse_i32(fields[at])?,
                parse_i32(fields[at + 1])?,
                parse_i32(fields[at + 2])?,
            );
            let state = fields[at + 3].to_owned();
            at += 4;
            blocks.push(ProcessBlock {
                pos,
                // `calculateRelativePosition(settings, pos).offset(position)`：默认
                // settings（mirror / rotation 都是 NONE、pivot 是 ZERO）下就是
                // `pos + position`。
                // With default settings (NONE mirror/rotation, ZERO pivot) this is
                // `pos + position`.
                world_pos: (pos.0 + piece.0, pos.1 + piece.1, pos.2 + piece.2),
                state: BlockStateSpec::parse_str(&state)?,
            });
        }
        let processors = lists
            .get(list_id)
            .ok_or_else(|| format!("Rust missing processor list {list_id}"))?;
        // key 直接取查询行的前 9 段，和 Java 侧 `copyOfRange(p, 0, 9)` 一致。
        // The key is the first nine fields of the query line, matching the Java side's
        // `copyOfRange(p, 0, 9)`.
        let key = if chunk_bb.is_some() {
            fields[..15].join("|")
        } else {
            fields[..10].join("|")
        };
        let level = FakeLevel::new(&world_state);
        // `CappedProcessor` 的 `processBlock` 是空实现，所以把它留在链里不会改变方块；
        // 但它的 `evaluatesEntirePieceState()` 为 true，会关掉 `processOnlyInCurrentChunk`，
        // 这正是要覆盖的分支。
        // `CappedProcessor.processBlock` is a no-op, so keeping it in the chain changes no
        // block, but its `evaluatesEntirePieceState()` disables
        // `processOnlyInCurrentChunk` — exactly the branch to cover.
        // 官方那边主链里的 `capped` 被换成了 `limit = 0`（避免需要真实 `ServerLevel`），
        // 所以 Rust 这边也要跳过 `finalize_capped`，只保留它的 `evaluates_entire_piece_state`
        // 效果（关掉 `processOnlyInCurrentChunk`）。
        // The oracle swaps the main-chain `capped` for `limit = 0`, so Rust must skip
        // `finalize_capped` too and keep only its `evaluates_entire_piece_state` effect.
        let zero_capped: Vec<Processor> = processors
            .iter()
            .map(|processor| match processor {
                Processor::Capped { delegate, .. } => Processor::Capped {
                    delegate: delegate.clone(),
                    limit: 0,
                },
                other => other.clone(),
            })
            .collect();
        let processed = processor_apply::process_blocks(
            &zero_capped,
            &blocks,
            &level,
            processor_apply::PlaceSettings {
                piece_position: piece,
                reference_pos: reference,
                chunk_bb,
                level_seed: 0,
                explicit_random: None,
            },
        );

        checked += 1;
        blocks_checked += processed.len();
        let want_count = java_a
            .get(&key)
            .ok_or_else(|| format!("Java missing A for {key}"))?;
        if *want_count != processed.len() {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!(
                    "{key}: kept rust {} java {want_count}",
                    processed.len()
                ));
            }
            continue;
        }
        let want_states = java_s.get(&key).cloned().unwrap_or_default();
        for (index, block) in processed.iter().enumerate() {
            let want = format!(
                "{}|{}|{}|{}",
                block.world_pos.0,
                block.world_pos.1,
                block.world_pos.2,
                block.state.text()
            );
            if want_states.get(index).is_none_or(|got| *got != want) {
                mismatches += 1;
                if first.is_none() {
                    first = Some(format!(
                        "{key}[{index}]: rust {want} java {:?}",
                        want_states.get(index)
                    ));
                }
            }
        }
        // `CappedProcessor` 的 delegate 单独跑一遍，与官方 `C`/`D` 行比。
        // Run the `CappedProcessor` delegate separately and compare with the `C`/`D` lines.
        for processor in processors {
            let Processor::Capped { delegate, .. } = processor else {
                continue;
            };
            let via_delegate = processor_apply::process_blocks(
                std::slice::from_ref(delegate.as_ref()),
                &blocks,
                &level,
                processor_apply::PlaceSettings {
                    piece_position: piece,
                    reference_pos: reference,
                    chunk_bb,
                    level_seed: 0,
                    explicit_random: None,
                },
            );
            let Some(want_count) = java_c.get(&key) else {
                continue;
            };
            if *want_count != via_delegate.len() {
                mismatches += 1;
                if first.is_none() {
                    first = Some(format!(
                        "{key}: capped delegate kept rust {} java {want_count}",
                        via_delegate.len()
                    ));
                }
                continue;
            }
            let want_states = java_d.get(&key).cloned().unwrap_or_default();
            for (index, block) in via_delegate.iter().enumerate() {
                let want = format!(
                    "{}|{}|{}|{}",
                    block.world_pos.0,
                    block.world_pos.1,
                    block.world_pos.2,
                    block.state.text()
                );
                if want_states.get(index).is_none_or(|got| *got != want) {
                    mismatches += 1;
                    if first.is_none() {
                        first = Some(format!(
                            "{key} capped[{index}]: rust {want} java {:?}",
                            want_states.get(index)
                        ));
                    }
                }
            }
        }
    }

    if checked != queries.len() {
        return Err(format!(
            "checked {checked} apply queries, expected {}",
            queries.len()
        ));
    }
    if mismatches > 0 {
        return Err(format!(
            "{mismatches} differences over {checked} queries; first: {}",
            first.unwrap_or_default()
        ));
    }
    println!("jigsaw apply: {checked} queries, {blocks_checked} surviving blocks, all identical");
    Ok(())
}

fn parse_i32(text: &str) -> Result<i32, String> {
    text.parse::<i32>()
        .map_err(|error| format!("bad number {text}: {error}"))
}

/// 假世界：所有位置的方块都是同一个状态；标签按 `mc-bamboo-data` 的方块表判定。
///
/// Fake level: every position holds the same state; tags are resolved through the
/// `mc-bamboo-data` block tables.
struct FakeLevel {
    state: BlockStateSpec,
}

impl FakeLevel {
    fn new(text: &str) -> Self {
        Self {
            state: BlockStateSpec::parse_str(text).unwrap_or(BlockStateSpec {
                name: "minecraft:air".to_owned(),
                properties: Vec::new(),
            }),
        }
    }
}

/// `state.is(HolderSet<Block>)`：标签走 `mc-bamboo-data` 的方块标签表，显式列表比对名字。
///
/// `state.is(HolderSet<Block>)`: tags go through the `mc-bamboo-data` block tag table,
/// explicit lists compare names.
fn in_block_set(state: &BlockStateSpec, set: &BlockSetRef) -> bool {
    match set {
        BlockSetRef::Tag(text) => {
            let name = text.strip_prefix('#').unwrap_or(text);
            let Some(ids) = mc_bamboo_data::block_tag(name) else {
                return false;
            };
            let Some(block) = mc_bamboo_data::block_by_name(&state.name) else {
                return false;
            };
            // `Block` 没有自己的 id 字段，用它的默认状态反查方块注册表 id。
            // `Block` has no id field, so derive it from the default state.
            let Some(id) = mc_bamboo_data::block_id_of_state(block.default_state) else {
                return false;
            };
            ids.contains(&(id as i32))
        }
        BlockSetRef::Blocks(names) => names.contains(&state.name),
    }
}

impl ProcessorLevel for FakeLevel {
    fn block_state(&self, _pos: (i32, i32, i32)) -> BlockStateSpec {
        self.state.clone()
    }

    fn in_block_set(&self, state: &BlockStateSpec, set: &BlockSetRef) -> bool {
        in_block_set(state, set)
    }
}

/// `capped` 模式：验证 `CappedProcessor.finalizeProcessing` 的随机源派生与洗牌顺序。
///
/// `capped` mode: verifies the random-source derivation and shuffle order of
/// `CappedProcessor.finalizeProcessing`.
fn run_capped(mut command: Command, work_dir: &Path) -> Result<(), String> {
    let mut queries = Vec::new();
    for seed in [0_i64, 1, -1, 42, 12_345_678_901] {
        for piece in [(0, 64, 0), (-13, 40, 27), (100, -50, -200)] {
            for size in [1_usize, 2, 5, 8, 33] {
                queries.push(format!("{seed}|{}|{}|{}|{size}", piece.0, piece.1, piece.2));
            }
        }
    }
    let queries_path = work_dir.join("jigsaw_capped_queries.txt");
    fs::write(&queries_path, queries.join("\n"))
        .map_err(|error| format!("cannot write queries: {error}"))?;
    let output = command
        .arg("capped")
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
    compare_capped(&String::from_utf8_lossy(&output.stdout), &queries)
}

/// Java 的 `C|key|sampled|indices` 与 `R|key|nextInt,nextInt,...`。
///
/// Java emits `C|key|sampled|indices` and `R|key|nextInt,...`.
fn compare_capped(stdout: &str, queries: &[String]) -> Result<(), String> {
    let mut java_c: HashMap<String, (i32, String)> = HashMap::new();
    let mut java_r: HashMap<String, String> = HashMap::new();
    for line in stdout.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() < 7 {
            continue;
        }
        let key = fields[1..6].join("|");
        match fields[0] {
            "C" => {
                java_c.insert(key, (fields[6].parse().unwrap_or(0), fields[7..].join("|")));
            }
            "R" => {
                java_r.insert(key, fields[6..].join("|"));
            }
            _ => {}
        }
    }

    let mut checked = 0usize;
    let mut mismatches = 0usize;
    let mut first: Option<String> = None;
    for query in queries {
        let fields: Vec<&str> = query.split('|').collect();
        let seed = fields[0]
            .parse::<i64>()
            .map_err(|error| format!("bad seed {}: {error}", fields[0]))?;
        let piece = (
            parse_i32(fields[1])?,
            parse_i32(fields[2])?,
            parse_i32(fields[3])?,
        );
        let size: usize = parse_i32(fields[4])? as usize;
        checked += 1;

        // `RandomSource.createThreadLocalInstance(seed).forkPositional().at(piece)`。
        // `RandomSource.createThreadLocalInstance(seed).forkPositional().at(piece)`.
        let random = Random::Legacy(LegacyRandom::new(seed))
            .fork_positional()
            .at(piece.0, piece.1, piece.2);
        // `limit.sample(random)` 用一个常数 6 的 provider（与 Java 侧一致）。
        // `limit.sample(random)` with a constant-6 provider, matching the Java side.
        let sampled = constant_int_sample(&random, 6);
        let mut indices: Vec<usize> = (0..size).collect();
        shuffle_indices(&mut indices, &random);
        let got = indices
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(",");

        let Some((want_sampled, want_indices)) = java_c.get(query) else {
            return Err(format!("Java missing C for {query}"));
        };
        if *want_sampled != sampled {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!(
                    "{query}: sampled rust {sampled} java {want_sampled}"
                ));
            }
        }
        if *want_indices != got {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!("{query}: indices rust {got} java {want_indices}"));
            }
        }

        // 再比原始 `nextInt(1000)` 序列，单独验随机源。
        // Then compare the raw `nextInt(1000)` sequence to check the source itself.
        let src = Random::Legacy(LegacyRandom::new(seed))
            .fork_positional()
            .at(piece.0, piece.1, piece.2);
        let raw = (0..8)
            .map(|_| src.next_int(1000).to_string())
            .collect::<Vec<_>>()
            .join(",");
        if let Some(want_raw) = java_r.get(query)
            && *want_raw != raw
        {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!("{query}: raw rust {raw} java {want_raw}"));
            }
        }
    }

    if checked != queries.len() {
        return Err(format!(
            "checked {checked} capped queries, expected {}",
            queries.len()
        ));
    }
    if mismatches > 0 {
        return Err(format!(
            "{mismatches} differences over {checked} queries; first: {}",
            first.unwrap_or_default()
        ));
    }
    println!("jigsaw capped: {checked} queries, source + shuffle + sample all identical");
    Ok(())
}

/// `ConstantInt.of(value).sample(random)`：不取随机数，直接返回常量。
///
/// `ConstantInt.of(value).sample(random)`: returns the constant without drawing.
fn constant_int_sample(_random: &Random, value: i32) -> i32 {
    value
}

/// `Util.toShuffledList(IntStream.range(0, size), random)` 的洗牌循环。
///
/// The shuffle loop of `Util.toShuffledList(IntStream.range(0, size), random)`.
fn shuffle_indices(items: &mut [usize], random: &Random) {
    for i in (2..=items.len()).rev() {
        let swap_to = random.next_int(i as i32) as usize;
        items.swap(i - 1, swap_to);
    }
}

/// 把一行 Java 输出拆成 `(key, 剩余部分)`：key 是 `query_by_key` 里某个查询前缀。
///
/// Split one Java output line into `(key, rest)` where the key is the prefix of one of the
/// queries.
fn split_java_row(line: &str, query_by_key: &HashMap<String, &String>) -> Option<(String, String)> {
    for key in query_by_key.keys() {
        let prefix = format!("|{key}|");
        if let Some(index) = line.find(&prefix) {
            let rest = &line[index + prefix.len()..];
            return Some((key.clone(), rest.to_owned()));
        }
    }
    None
}

/// `cappedfull` 模式：带真实种子跑完整 `process_blocks`（含 `finalize_capped`），
/// 与官方 `processBlockInfos` 逐方块比。这是 `capped` 三段式验证之外的端到端检查。
///
/// `cappedfull` mode: runs the whole `process_blocks` with a real seed (so
/// `finalize_capped` really executes) and compares block by block with vanilla
/// `processBlockInfos`.
fn run_capped_full(mut command: Command, work_dir: &Path) -> Result<(), String> {
    let raw: Value =
        serde_json::from_str(WORLDGEN).map_err(|error| format!("bad worldgen.json: {error}"))?;
    let lists = ProcessorLists::parse(raw.get("processor_list").ok_or("missing processor_list")?)?;

    // 只有含 `capped` 的列表才有意义（其余走 `apply` 已覆盖）。
    let mut queries = Vec::new();
    for name in lists.names().cloned().collect::<Vec<_>>() {
        let processors = lists.get(&name).ok_or("missing list")?;
        if !processors
            .iter()
            .any(|processor| matches!(processor, Processor::Capped { .. }))
        {
            continue;
        }
        for seed in [0_i64, 1, 42, -7] {
            for piece in [(0, 64, 0), (-13, 40, 27)] {
                queries.push(build_capped_full_query(&name, seed, piece));
            }
        }
    }
    let queries_path = work_dir.join("jigsaw_capped_full_queries.txt");
    fs::write(&queries_path, queries.join("\n"))
        .map_err(|error| format!("cannot write queries: {error}"))?;
    let output = command
        .arg("cappedfull")
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
    compare_capped_full(&String::from_utf8_lossy(&output.stdout), &queries, &lists)
}

/// 一条 `cappedfull` 查询：`list|seed|piece(3)|worldState|count|blocks`。
///
/// One `cappedfull` query line.
fn build_capped_full_query(name: &str, seed: i64, piece: (i32, i32, i32)) -> String {
    // `minecraft:trail_ruins_replaceable` 只含 `minecraft:gravel`，所以至少要放几个砾石，
    // `capped` 才会真的替换；其余方块作为不命中的对照。
    // `minecraft:trail_ruins_replaceable` only contains `minecraft:gravel`, so the batch
    // needs several gravel blocks for `capped` to do anything; the rest are controls.
    let states = [
        "minecraft:gravel",
        "minecraft:gravel",
        "minecraft:gravel",
        "minecraft:gravel",
        "minecraft:gravel",
        "minecraft:gravel",
        "minecraft:stone",
        "minecraft:deepslate_bricks",
    ];
    let mut line = format!(
        "{name}|{seed}|{}|{}|{}|minecraft:air|{}",
        piece.0,
        piece.1,
        piece.2,
        states.len()
    );
    for (index, state) in states.iter().enumerate() {
        // 查询里放**模板相对坐标**：官方 `processBlockInfos` 自己会
        // `calculateRelativePosition(...).offset(position)`，所以这里不能预先加 `piece`。
        // The query holds template-relative positions: vanilla `processBlockInfos` adds
        // `position` itself, so `piece` must not be pre-added.
        let pos = ((index as i32) % 4, index as i32 / 4, 0);
        line.push_str(&format!("|{}|{}|{}|{state}", pos.0, pos.1, pos.2));
    }
    line
}

fn compare_capped_full(
    stdout: &str,
    queries: &[String],
    lists: &ProcessorLists,
) -> Result<(), String> {
    let mut java_a: HashMap<String, usize> = HashMap::new();
    let mut java_s: HashMap<String, Vec<String>> = HashMap::new();
    for line in stdout.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() < 9 {
            continue;
        }
        // key 是前 7 段（list|seed|piece(3)|worldState|count）。
        // The key is the first seven fields.
        let key = fields[1..8].join("|");
        match fields[0] {
            "A" => {
                java_a.insert(key, fields[8].parse().unwrap_or(0));
            }
            "S" => {
                java_s.entry(key).or_default().push(fields[8..].join("|"));
            }
            _ => {}
        }
    }

    let mut checked = 0usize;
    let mut blocks_checked = 0usize;
    let mut mismatches = 0usize;
    let mut first: Option<String> = None;
    for query in queries {
        let fields: Vec<&str> = query.split('|').collect();
        let list_id = fields[0];
        let seed = fields[1]
            .parse::<i64>()
            .map_err(|error| format!("bad seed: {error}"))?;
        let piece = (
            parse_i32(fields[2])?,
            parse_i32(fields[3])?,
            parse_i32(fields[4])?,
        );
        let world_state = fields[5].to_owned();
        let count: usize = parse_i32(fields[6])? as usize;
        let mut blocks = Vec::with_capacity(count);
        let mut at = 7;
        for _ in 0..count {
            let pos = (
                parse_i32(fields[at])?,
                parse_i32(fields[at + 1])?,
                parse_i32(fields[at + 2])?,
            );
            let state = fields[at + 3].to_owned();
            at += 4;
            blocks.push(ProcessBlock {
                // 查询里是模板相对坐标，世界坐标由 `piece + pos` 得到（与官方一致的默认
                // settings 语义）。
                // The query holds template-relative positions; the world position is
                // `piece + pos`, matching vanilla with default settings.
                pos,
                world_pos: (pos.0 + piece.0, pos.1 + piece.1, pos.2 + piece.2),
                state: BlockStateSpec::parse_str(&state)?,
            });
        }
        let processors = lists
            .get(list_id)
            .ok_or_else(|| format!("Rust missing processor list {list_id}"))?;
        let key = fields[..7].join("|");
        let level = FakeLevel::new(&world_state);
        let processed = processor_apply::process_blocks(
            processors,
            &blocks,
            &level,
            processor_apply::PlaceSettings {
                piece_position: piece,
                reference_pos: piece,
                chunk_bb: None,
                level_seed: seed,
                explicit_random: None,
            },
        );
        checked += 1;
        blocks_checked += processed.len();
        let Some(want_count) = java_a.get(&key) else {
            return Err(format!("Java missing A for {key}"));
        };
        if *want_count != processed.len() {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!(
                    "{key}: kept rust {} java {want_count}",
                    processed.len()
                ));
            }
            continue;
        }
        let want_states = java_s.get(&key).cloned().unwrap_or_default();
        for (index, block) in processed.iter().enumerate() {
            let want = format!(
                "{}|{}|{}|{}",
                block.world_pos.0,
                block.world_pos.1,
                block.world_pos.2,
                block.state.text()
            );
            if want_states.get(index).is_none_or(|got| *got != want) {
                mismatches += 1;
                if first.is_none() {
                    first = Some(format!(
                        "{key}[{index}]: rust {want} java {:?}",
                        want_states.get(index)
                    ));
                }
            }
        }
    }
    if checked != queries.len() {
        return Err(format!(
            "checked {checked} capped-full queries, expected {}",
            queries.len()
        ));
    }
    if mismatches > 0 {
        return Err(format!(
            "{mismatches} differences over {checked} queries; first: {}",
            first.unwrap_or_default()
        ));
    }
    println!("jigsaw cappedfull: {checked} queries, {blocks_checked} blocks, all identical");
    Ok(())
}

/// `placepiece` 模式：官方 `StructureTemplate.placeInWorld` 对
/// `mc_bamboo_worldgen::structure::jigsaw::place::place_piece`，逐写入比。
///
/// `placepiece` mode: official `StructureTemplate.placeInWorld` against
/// `mc_bamboo_worldgen::structure::jigsaw::place::place_piece`, compared write by write.
fn run_place_piece(mut command: Command, work_dir: &Path) -> Result<(), String> {
    // 只挑单 palette 的模板：`getRandomPalette` 在这轮不是重点（已由 `jigsaw jigsaws` 覆盖），
    // 而多 palette 会让每个查询的期望值依赖位置随机。
    // Only single-palette templates: `getRandomPalette` is covered by `jigsaw jigsaws`, and
    // multi-palette templates would make each expectation depend on the position roll.
    let mut names: Vec<String> = blob::ENTRIES
        .iter()
        .filter(|(name, _, _)| {
            StructureTemplate::load(name)
                .is_ok_and(|t| t.palettes.len() == 1 && !t.blocks.is_empty())
        })
        .map(|(name, _, _)| (*name).to_owned())
        .collect();
    names.sort();
    // 每 20 个取一个，控制规模。
    // Take every 20th to keep the run small.
    let picked: Vec<String> = names.iter().step_by(20).cloned().collect();

    let mut queries = Vec::new();
    for name in &picked {
        for (mirror, rotation) in [(0_u32, 0_u32), (0, 1), (1, 2)] {
            let (sx, sy, sz) = StructureTemplate::load(name)
                .map_err(|error| format!("cannot load {name}: {error}"))?
                .size;
            // 旋转会把模板相对坐标变成负值（90 度时映射成 (-z, y, x)），所以盒要跨正负两侧。
            // Rotations turn template positions negative (90 degrees maps to (-z, y, x)), so
            // the box must straddle zero.
            let span = sx.max(sz) + 1;
            let bb = (-span, 0, -span, span, sy + 1, span);
            queries.push(format!(
                "{name}|0|0|0|{mirror}|{rotation}|{}|{}|{}|{}|{}|{}|minecraft:air",
                bb.0, bb.1, bb.2, bb.3, bb.4, bb.5
            ));
        }
    }
    let queries_path = work_dir.join("jigsaw_place_piece_queries.txt");
    fs::write(&queries_path, queries.join("\n"))
        .map_err(|error| format!("cannot write queries: {error}"))?;
    let output = command
        .arg("placepiece")
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
    compare_place_piece(&String::from_utf8_lossy(&output.stdout), &queries)
}

fn compare_place_piece(stdout: &str, queries: &[String]) -> Result<(), String> {
    let mut java_q: HashMap<String, (bool, usize)> = HashMap::new();
    let mut java_w: HashMap<String, Vec<String>> = HashMap::new();
    for line in stdout.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() < 14 {
            continue;
        }
        let key = fields[1..13].join("|");
        match fields[0] {
            "Q" => {
                java_q.insert(key, (fields[13] == "1", fields[14].parse().unwrap_or(0)));
            }
            "W" => {
                java_w.entry(key).or_default().push(fields[13..].join("|"));
            }
            _ => {}
        }
    }

    let mut checked = 0usize;
    let mut writes_checked = 0usize;
    let mut mismatches = 0usize;
    let mut first: Option<String> = None;
    for query in queries {
        let fields: Vec<&str> = query.split('|').collect();
        let name = fields[0];
        let piece = (
            parse_i32(fields[1])?,
            parse_i32(fields[2])?,
            parse_i32(fields[3])?,
        );
        let mirror = parse_i32(fields[4])? as u32;
        let rotation = parse_i32(fields[5])? as u32;
        let bb = BoundingBox {
            min_x: parse_i32(fields[6])?,
            min_y: parse_i32(fields[7])?,
            min_z: parse_i32(fields[8])?,
            max_x: parse_i32(fields[9])?,
            max_y: parse_i32(fields[10])?,
            max_z: parse_i32(fields[11])?,
        };
        let template = StructureTemplate::load(name)
            .map_err(|error| format!("cannot load {name}: {error}"))?;
        struct Level;
        impl ProcessorLevel for Level {
            fn block_state(&self, _pos: (i32, i32, i32)) -> BlockStateSpec {
                BlockStateSpec::parse_str("minecraft:air").unwrap()
            }
            fn in_block_set(&self, _state: &BlockStateSpec, _set: &BlockSetRef) -> bool {
                false
            }
        }
        #[derive(Default)]
        struct Recorder {
            writes: Vec<(i32, i32, i32, String)>,
        }
        impl mc_bamboo_worldgen::structure::jigsaw::place::BlockWriter for Recorder {
            fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
                self.writes.push((
                    x,
                    y,
                    z,
                    mc_bamboo_worldgen::structure::jigsaw::place::state_text(state),
                ));
                true
            }
        }
        let level = Level;
        let mut recorder = Recorder::default();
        mc_bamboo_worldgen::structure::jigsaw::place::place_piece(
            &template,
            piece,
            &[],
            &level,
            processor_apply::PlaceSettings {
                piece_position: piece,
                reference_pos: piece,
                chunk_bb: Some(bb),
                level_seed: 0,
                explicit_random: None,
            },
            mirror,
            rotation,
            bb,
            &mut recorder,
        )?;
        checked += 1;
        writes_checked += recorder.writes.len();
        let key = fields[..12].join("|");
        let Some((_placed, count)) = java_q.get(&key) else {
            return Err(format!("Java missing Q for {key}"));
        };
        if *count != recorder.writes.len() {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!(
                    "{key}: writes rust {} java {count}",
                    recorder.writes.len()
                ));
            }
            continue;
        }
        let want = java_w.get(&key).cloned().unwrap_or_default();
        for (index, (x, y, z, state)) in recorder.writes.iter().enumerate() {
            // 只比方块名（Java 侧也只输出名字）：属性差异在这一轮先不追究。
            // Compare block names only (the Java side prints names too); property
            // differences are a separate concern.
            let got = format!("{x}|{y}|{z}|{state}");
            if want.get(index).is_none_or(|line| *line != got) {
                mismatches += 1;
                if first.is_none() {
                    first = Some(format!(
                        "{key}[{index}]: rust {got} java {:?}",
                        want.get(index)
                    ));
                }
            }
        }
    }
    if checked != queries.len() {
        return Err(format!(
            "checked {checked} place queries, expected {}",
            queries.len()
        ));
    }
    if mismatches > 0 {
        return Err(format!(
            "{mismatches} differences over {checked} queries; first: {}",
            first.unwrap_or_default()
        ));
    }
    println!("jigsaw placepiece: {checked} queries, {writes_checked} writes, all identical");
    Ok(())
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
    let mut valid_count = 0usize;
    let mut first: Option<String> = None;
    // 每种 pool element 类型比到多少件，用来确认五种类型都真的被比过。
    // How many pieces of each pool element type were compared, proving all five ran.
    let mut type_counts: BTreeMap<String, usize> = BTreeMap::new();
    let preset = worldgen
        .preset("minecraft:overworld")
        .ok_or("missing overworld preset")?;
    for (seed, group) in &by_seed {
        let generator = Generator::new(&worldgen, *seed)?;
        // `findValidGenerationPoint` 最后一道门 `isValidBiome(stub)` 走单点群系解析。
        // The final gate of `findValidGenerationPoint`, `isValidBiome(stub)`, uses the
        // single-point biome resolver.
        let mut router = SeededRouter::new(settings, *seed);
        let mut resolver = BiomeResolver {
            router: &mut router,
            source: preset,
        };
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
            // `isValidBiome` 用 stub 位置的三点 quart 群系，与结构件列表无关。
            // `isValidBiome` uses the stub's quart biome and is independent of the pieces.
            let valid = match &result {
                Some(structure) => validity::is_valid_biome_at(
                    &mut resolver,
                    structure.position.0,
                    structure.position.1,
                    structure.position.2,
                    allowed,
                ),
                None => false,
            };
            if valid {
                valid_count += 1;
            }
            let got_p = java_p.get(&key);
            if let Some(diff) = check_query(&result, valid, q, got_p)? {
                mismatches += 1;
                if first.is_none() {
                    first = Some(format!("{key}: {diff}"));
                }
            }
            if let Some(structure) = &result {
                pieces_checked += structure.pieces.len();
                for piece in &structure.pieces {
                    *type_counts
                        .entry(piece.element.type_name().to_owned())
                        .or_default() += 1;
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
            "{mismatches}/{checked} queries differ; first: {}",
            first.unwrap_or_default()
        ));
    }
    println!("jigsaw pieces: {checked} queries, {pieces_checked} pieces, all Q/P fields identical");
    println!("isValidBiome true for {valid_count}/{checked} queries");
    let summary = type_counts
        .iter()
        .map(|(name, count)| format!("{name}={count}"))
        .collect::<Vec<_>>()
        .join(" ");
    println!("piece element types: {summary}");
    let missing: Vec<&str> = ["empty", "single", "legacy", "list", "feature"]
        .into_iter()
        .filter(|name| !type_counts.contains_key(*name))
        .collect();
    if !missing.is_empty() {
        println!(
            "note: pool element types never compared: {}",
            missing.join(", ")
        );
    }
    Ok(())
}

/// 用生产 `add_pieces` 的结果比 Q 行与全部 P 行；返回第一个差异描述。
fn check_query(
    result: &Option<StructurePieces>,
    valid: bool,
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
        if valid { "1" } else { "0" }.to_owned(),
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
        let mut want = vec![
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
            // `PoolElementStructurePiece` 的构造器是 `super(JIGSAW, 0, bb)`，没有别处调
            // `setGenDepth`，所以 `getGenDepth()` 恒为 0。
            // The constructor is `super(JIGSAW, 0, bb)` and nothing calls `setGenDepth`,
            // so `getGenDepth()` is always 0.
            "0".to_owned(),
            piece.liquid_settings.name().to_owned(),
            piece.junctions.len().to_string(),
        ];
        // junction 详情（`sourceX|sourceGroundY|sourceZ|deltaY|destProjection` × N）。
        // Junction details (`sourceX|sourceGroundY|sourceZ|deltaY|destProjection` × N).
        for junction in &piece.junctions {
            want.extend([
                junction.source_x.to_string(),
                junction.source_ground_y.to_string(),
                junction.source_z.to_string(),
                junction.delta_y.to_string(),
                junction.dest_projection.name().to_owned(),
            ]);
        }
        for (field, want) in want.iter().enumerate() {
            if line.get(field).is_none_or(|got| got != want) {
                return Ok(Some(format!(
                    "P[{index}][{field}]: rust {want} java {:?}",
                    line.get(field)
                )));
            }
        }
        if line.len() != want.len() {
            return Ok(Some(format!(
                "P[{index}]: rust {} fields java {}",
                want.len(),
                line.len()
            )));
        }
    }
    Ok(None)
}
