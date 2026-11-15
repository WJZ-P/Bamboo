//! Jigsaw 官方对拍（Rust 侧）。
//!
//! `jigsaw` 子命令跑 `JigsawOracle.java`：Java 端造真实的
//! `StructureTemplateManager` 和 `Structure.GenerationContext`，这里用生产代码
//! `mc-bamboo-worldgen::structure` 复算并逐字段比较。
//!
//! - `templates`：验证每个模板被 `StructureTemplateManager` 正确加载（size、
//!   save() 的 blocks 数、getJigsaws 的拼图方块数）。
//! - `jigsaws`：`getJigsaws(position, rotation)` 的拼图方块，含 `getRandomPalette`
//!   的索引选择。
//! - `pieces`：验证 `findGenerationPoint` + `addPieces` 的**全部结构件**
//!   （RandomTemplate / Rotation / 包围盒 / project_start_to_heightmap /
//!   VoxelShape 空间检查 / junction 数）。默认跑满 6 种子 × 12 区块 × 27 结构。
//! - `piecesbiome`：同 `pieces`，但**先按群系挑起点**（`column_has_valid_biome`
//!   扫半径 64 的区块方阵）。`pieces` 的固定区块只有 5.8% 能通过 `isValidBiome`，
//!   这个模式能到 55%，真正跑完 `addPieces` 的查询多一个数量级。
//! - `piecesexact`：在 `piecesbiome` 之上再加 `random_spread` 的**放置门**
//!   （`StructureState::is_structure_chunk`）与 `find_valid_generation_point`
//!   （列群系 + 起点群系）。命中率 **93%（112/120）**，是覆盖 `addPieces` 的主力。
//! - `pools`：每个 template pool 的结构件生成。245 个池 × 3 种子 × 3 区块
//!   = 2205 查询 / 19994 件。
//! - `save` / `load`：结构件 NBT 的编码与读回（含 `regeneratePiecesAfterLoad`）。
//!   规模与 `pieces` 相同（1944 查询 / 25650 件）。
//! - `processors` / `apply`：`processor_list` 的解析与 `processBlock` 执行。
//! - `capped`：`CappedProcessor.finalizeProcessing` 的**端到端**比（真实
//!   `CappedProcessor(RuleProcessor)`，比存活方块），`cappedraw` 是它的细粒度
//!   诊断版（只比随机源 / 洗牌 / 取样三段）。
//! - `cappedfull`：带真实种子跑完整 `process_blocks`，比最终方块的**状态**。
//! - `placepiece`：`StructureTemplate.placeInWorld` 的方块写入（含多 palette 模板）。
//! - `waterpiece`：同上但带 `APPLY_WATERLOGGING` 与流体模型，逐写入比完整状态。
//! - `entities`：`StructureTemplate.placeEntities` 的几何与 nbt 改写。
//! - `bemod`：`RuleBlockEntityModifier.apply` 的 NBT 生成。
//! - `rc`：`RandomizableContainer` 的判定表（全部方块）。
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
use mc_bamboo_worldgen::{
    Worldgen,
    chunk::Generator,
    settings::SeededRouter,
    structure::{
        BoundingBox, StructureSets, StructureState, Structures, allowed_biome_names,
        column_has_valid_biome, find_valid_generation_point,
        jigsaw::{
            JigsawConfig, JigsawStructures, Piece, Rotation, StructurePieces, TemplatePools,
            add_pieces,
        },
        processor::{
            BlockSetRef, BlockStateSpec, PosRuleTest, Processor, ProcessorLists, ProcessorRule,
            RuleBlockEntityModifier, RuleTest,
        },
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
const CHUNKS: [(i32, i32); 12] = [
    (0, 0),
    (1, 1),
    (-5, 7),
    (34, -34),
    (12, -3),
    (-40, 40),
    // 补几个不同群系带：靠近热带、寒带与内陆，提高 `isValidBiome` 的通过率。
    // Extra chunks from different biome bands (tropical, cold, inland) so more stubs pass
    // `isValidBiome` and reach `addPieces`.
    (-120, 260),
    (300, 410),
    (-780, -160),
    (1024, -1024),
    (37, 512),
    (-600, 900),
];

pub fn run(command: Command, work_dir: &Path, options: &[&str]) -> Result<(), String> {
    match options.first().copied().unwrap_or("templates") {
        "templates" => run_templates(command, work_dir),
        "jigsaws" => run_jigsaws(command, work_dir),
        "pieces" => run_pieces(command, work_dir, options.get(1), options.get(2)),
        "piecesbiome" => run_pieces_biome(command, work_dir),
        "piecesexact" => run_pieces_exact(command, work_dir),
        "pools" => run_pools(command, work_dir),
        "save" => run_save(command, work_dir),
        "processors" => run_processors(command, work_dir),
        "load" => run_load(command, work_dir),
        "apply" => run_apply(command, work_dir),
        "capped" => run_capped(command, work_dir),
        "cappedraw" => run_capped_raw(command, work_dir),
        "cappedfull" => run_capped_full(command, work_dir),
        "placepiece" => run_place_piece(command, work_dir),
        "entities" => run_entities(command, work_dir),
        "waterpiece" => run_water_piece(command, work_dir),
        "bemod" => run_block_entity_modifier(command, work_dir),
        "rc" => run_randomizable_container(command, work_dir),
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
    // 每个池都跑一遍所有 (seed, chunk) 组合：池的洗牌与选中顺序依赖随机源，
    // 多跑几组才能覆盖不同路径。
    // Every pool runs under all (seed, chunk) pairs; the pool's shuffle and selection
    // order depend on the random source, so more pairs cover more paths.
    for &seed in &SEEDS[..3] {
        for &(cx, cz) in &CHUNKS[..3] {
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
    // 与 `pieces` 一致：只要求「数据里真的有」且「会产件」的类型被比到。
    // `empty_pool_element` 的 `getPieces` 恒返回空列表，数据里虽有 31 处，但不可能
    // 出现在上面这份统计里。
    // Same rule as `pieces`: only kinds that exist in the data and yield pieces are
    // required. `empty_pool_element.getPieces` always returns an empty list, so although
    // the data has 31 of them they can never appear in the summary above.
    let available = pool_element_kinds_in_data()?;
    let missing: Vec<&str> = ["single", "legacy", "list", "feature"]
        .into_iter()
        .filter(|name| available.contains(*name) && !type_counts.contains_key(*name))
        .collect();
    if !missing.is_empty() {
        println!(
            "note: pool element types never compared: {}",
            missing.join(", ")
        );
    }
    if available.contains("empty") {
        println!(
            "note: `empty` elements never appear above because they yield no pieces, but \
             their `break` in the target loop is covered by the Q/P comparison (turning it \
             into `continue` reports 109/980 pools differing)"
        );
    }
    Ok(())
}

/// `save` 模式的查询与 `pieces` 相同（结构 × 种子 × 区块），比的是官方 `createTag`
/// 编出的完整 Children 标签。
///
/// `save` mode queries match `pieces` (structure × seed × chunk) but compare the full
/// Children tag produced by vanilla `createTag`.
fn run_save(mut command: Command, work_dir: &Path) -> Result<(), String> {
    let queries = build_queries(SEEDS.len(), CHUNKS.len());
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
    let queries = build_queries(SEEDS.len(), CHUNKS.len());
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
    // 覆盖数据里**全部** 48 种规则输入方块（从 `processor_list` 的 `input_predicate`
    // 汇总），加上草方块与空气作对照。这样每条 `rule` 至少命中一次，
    // `random_block_match` 的概率分支也能被不同方块反复触发。
    // Every one of the 48 rule input blocks the data actually tests (collected from each
    // `processor_list`'s `input_predicate`), plus grass and air as controls. That way every
    // `rule` fires at least once and `random_block_match`'s probability branches get
    // exercised repeatedly on different blocks.
    let states = [
        "minecraft:acacia_log",
        "minecraft:acacia_planks",
        "minecraft:acacia_stairs",
        "minecraft:acacia_wood",
        "minecraft:blackstone",
        "minecraft:campfire",
        "minecraft:chiseled_polished_blackstone",
        "minecraft:coal_ore",
        "minecraft:cobblestone",
        "minecraft:cracked_polished_blackstone_bricks",
        "minecraft:cut_sandstone",
        "minecraft:deepslate_bricks",
        "minecraft:deepslate_tile_slab",
        "minecraft:deepslate_tiles",
        "minecraft:dirt",
        "minecraft:dirt_path",
        "minecraft:gilded_blackstone",
        "minecraft:glass_pane",
        "minecraft:gold_block",
        "minecraft:gravel",
        "minecraft:lantern",
        "minecraft:magma_block",
        "minecraft:mossy_cobblestone",
        "minecraft:mud_bricks",
        "minecraft:oak_log",
        "minecraft:oak_planks",
        "minecraft:oak_stairs",
        "minecraft:orange_terracotta",
        "minecraft:polished_blackstone_bricks",
        "minecraft:red_terracotta",
        "minecraft:smooth_sandstone",
        "minecraft:smooth_sandstone_slab",
        "minecraft:smooth_sandstone_stairs",
        "minecraft:soul_lantern",
        "minecraft:spruce_log",
        "minecraft:spruce_planks",
        "minecraft:spruce_slab",
        "minecraft:stripped_oak_log",
        "minecraft:stripped_spruce_log",
        "minecraft:stripped_spruce_wood",
        "minecraft:terracotta",
        "minecraft:torch",
        "minecraft:wall_torch",
        "minecraft:waxed_copper_bulb",
        "minecraft:wheat",
        "minecraft:white_terracotta",
        "minecraft:yellow_terracotta",
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
                nbt: None,
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
        // 与 Java 的 `stateName(state, inputProps)` 对齐：只输出**输入方块里出现过的**
        // 属性名。规则可能给方块加上输入里没有的属性（例如把
        // `waxed_exposed_copper_bulb` 换成带 `lit` 的变体），那些属性 Java 侧不打印，
        // 这里也必须省掉，否则会报假差异。
        // Match Java's `stateName(state, inputProps)`: only print property names that
        // occurred in the **inputs**. A rule can add properties the inputs never had (for
        // example turning `waxed_exposed_copper_bulb` into a `lit` variant); Java does not
        // print those, so neither may this side or the comparison reports a false diff.
        let mut input_props: Vec<String> = Vec::new();
        for block in &blocks {
            for (name, _) in &block.state.properties {
                if !input_props.iter().any(|seen| seen == name) {
                    input_props.push(name.clone());
                }
            }
        }
        input_props.sort();
        for (index, block) in processed.iter().enumerate() {
            let shown: Vec<String> = input_props
                .iter()
                .filter_map(|name| {
                    block
                        .state
                        .properties
                        .iter()
                        .find(|(key, _)| key == name)
                        .map(|(key, value)| format!("{key}={value}"))
                })
                .collect();
            let state_text = if shown.is_empty() {
                block.state.name.clone()
            } else {
                format!("{}[{}]", block.state.name, shown.join(","))
            };
            let want = format!(
                "{}|{}|{}|{state_text}",
                block.world_pos.0, block.world_pos.1, block.world_pos.2
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
    for seed in [0_i64, 1, -1, 42, 12_345_678_901, -98_765_432_123] {
        for piece in [
            (0, 64, 0),
            (-13, 40, 27),
            (100, -50, -200),
            (-1000, 200, 1000),
        ] {
            // `limit` 覆盖 0（不替换）、小于列表长度、恰好等于、大于；列表长度取三种；
            // `integrity` 取 0（全保留）、0.5、1（全替换成空气），
            // 这样 delegate 是否真的生效也能看出来。
            // `limit` covers 0, below the list size, exactly it and above; three list
            // sizes; `integrity` covers 0 (nothing rots), 0.5 and 1 (everything does),
            // so the delegate's effect is visible too.
            for list_size in [1_usize, 7, 33] {
                for limit in [0_i32, 1, 5, 33, 1000] {
                    for integrity in ["0", "0.5", "1"] {
                        queries.push(format!(
                            "{seed}|{}|{}|{}|{list_size}|{limit}|{integrity}",
                            piece.0, piece.1, piece.2
                        ));
                    }
                }
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

/// `capped` 对拍里第 `index` 个输入方块用的羊毛颜色。
///
/// The wool colour used for the `index`th input block in the `capped` oracle.
fn wool_colour(index: usize) -> &'static str {
    const COLOURS: [&str; 16] = [
        "minecraft:white_wool",
        "minecraft:orange_wool",
        "minecraft:magenta_wool",
        "minecraft:light_blue_wool",
        "minecraft:yellow_wool",
        "minecraft:lime_wool",
        "minecraft:pink_wool",
        "minecraft:gray_wool",
        "minecraft:light_gray_wool",
        "minecraft:cyan_wool",
        "minecraft:purple_wool",
        "minecraft:blue_wool",
        "minecraft:brown_wool",
        "minecraft:green_wool",
        "minecraft:red_wool",
        "minecraft:black_wool",
    ];
    COLOURS[index % COLOURS.len()]
}

/// `cappedraw` 模式：只比 `finalizeProcessing` 的三段随机路径，
/// 用产品里的 `capped_random` / `capped_indices` 复算，作为 `capped` 端到端比的
/// 细粒度定位手段。
///
/// `cappedraw` mode: compares only the three random stages of `finalizeProcessing`,
/// recomputed through the production `capped_random` / `capped_indices`, so a failure in
/// the end-to-end `capped` mode can be localised.
fn run_capped_raw(mut command: Command, work_dir: &Path) -> Result<(), String> {
    let mut queries = Vec::new();
    for seed in [0_i64, 1, -1, 42, 12_345_678_901, -98_765_432_123] {
        for piece in [
            (0, 64, 0),
            (-13, 40, 27),
            (100, -50, -200),
            (-1000, 200, 1000),
        ] {
            for list_size in [1_usize, 7, 33] {
                for limit in [0_i32, 1, 5, 33, 1000] {
                    queries.push(format!(
                        "{seed}|{}|{}|{}|{list_size}|{limit}",
                        piece.0, piece.1, piece.2
                    ));
                }
            }
        }
    }
    let queries_path = work_dir.join("jigsaw_capped_raw_queries.txt");
    fs::write(&queries_path, queries.join("\n"))
        .map_err(|error| format!("cannot write queries: {error}"))?;
    let output = command
        .arg("cappedraw")
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
    compare_capped_raw(&String::from_utf8_lossy(&output.stdout), &queries)
}

/// 解析 `cappedraw` 的输出并逐项比。
///
/// Parse the `cappedraw` output and compare it item by item.
fn compare_capped_raw(stdout: &str, queries: &[String]) -> Result<(), String> {
    let mut java_c: HashMap<String, (i32, i32, String)> = HashMap::new();
    let mut java_r: HashMap<String, String> = HashMap::new();
    let mut java_t: HashMap<String, String> = HashMap::new();
    for line in stdout.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() < 7 {
            continue;
        }
        // 查询是 6 段，紧跟 `C|` / `R|` / `T|`。
        // A query has six fields, right after `C|` / `R|` / `T|`.
        let key = fields[1..7].join("|");
        match fields[0] {
            "C" => {
                java_c.insert(
                    key,
                    (
                        fields[7].parse().unwrap_or(0),
                        fields[8].parse().unwrap_or(0),
                        fields[9..].join("|"),
                    ),
                );
            }
            "R" => {
                java_r.insert(key, fields[7..].join("|"));
            }
            "T" => {
                java_t.insert(key, fields[7..].join("|"));
            }
            _ => {}
        }
    }

    let mut checked = 0usize;
    let mut indices_checked = 0usize;
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
        let list_size: usize = parse_i32(fields[4])? as usize;
        let limit: i32 = parse_i32(fields[5])?;
        checked += 1;

        // `ConstantInt.of(limit).sample(random)` 不取随机数，直接返回常量。
        // `ConstantInt.of(limit).sample(random)` returns the constant without drawing.
        let sampled = limit;
        let max_to_replace = sampled.min(list_size as i32).max(0) as usize;
        // 产品代码里的随机源派生。完整序列与截断序列各用**独立的**随机源，
        // 因为 `capped_indices` 每调用一次都会消耗随机数。
        // The random derivation from product code. The full and truncated sequences use
        // **independent** sources, because each `capped_indices` call consumes randomness.
        let full = processor_apply::capped_indices(
            list_size,
            list_size,
            &processor_apply::capped_random(seed, piece),
        );
        let truncated = processor_apply::capped_indices(
            list_size,
            max_to_replace,
            &processor_apply::capped_random(seed, piece),
        );
        indices_checked += truncated.len();
        let want_full = full
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(",");

        let Some((want_sampled, want_max, want_indices)) = java_c.get(query) else {
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
        if *want_max != max_to_replace as i32 {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!(
                    "{query}: maxToReplace rust {max_to_replace} java {want_max}"
                ));
            }
        }
        if *want_indices != want_full {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!(
                    "{query}: indices rust {want_full} java {want_indices}"
                ));
            }
        }
        // `maxToReplace` 已在上面的 `C` 行比过；官方**不**截断洗牌序列（靠循环提前退出），
        // 所以这里只确认「`maxToReplace` 不超过完整序列长度」。
        // `maxToReplace` is already compared in the `C` line; vanilla does **not** truncate
        // the shuffled list (the loop exits early instead), so this only checks that the
        // quota fits inside the list.
        if max_to_replace > full.len() {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!(
                    "{query}: maxToReplace {max_to_replace} exceeds list size {}",
                    full.len()
                ));
            }
        }

        // 原始 `nextInt(1000)` 序列，单独验随机源（用独立随机源，避免被洗牌消耗）。
        // The raw `nextInt(1000)` sequence, checking the source itself with an independent
        // source so the shuffle does not consume it.
        let source = processor_apply::capped_random(seed, piece);
        let raw = (0..8)
            .map(|_| source.next_int(1000).to_string())
            .collect::<Vec<_>>()
            .join(",");
        if java_r.get(query).is_some_and(|want| *want != raw) {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!(
                    "{query}: raw rust {raw} java {:?}",
                    java_r.get(query)
                ));
            }
        }
    }

    if checked != queries.len() {
        return Err(format!(
            "checked {checked} capped-raw queries, expected {}",
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
        "jigsaw cappedraw: {checked} queries, {indices_checked} indices, \
         source + shuffle + sample all identical"
    );
    Ok(())
}

/// `capped` 的 Java 输出是 `C|key|count` 加一串 `S|key|x|y|z|block`。
///
/// The `capped` Java output is `C|key|count` followed by `S|key|x|y|z|block` lines.
fn compare_capped(stdout: &str, queries: &[String]) -> Result<(), String> {
    let mut java_c: HashMap<String, usize> = HashMap::new();
    let mut java_s: HashMap<String, Vec<String>> = HashMap::new();
    for line in stdout.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() < 9 {
            continue;
        }
        // 查询是 7 段，所以 key 是 `fields[1..8]`。
        // A query has seven fields, so the key is `fields[1..8]`.
        let key = fields[1..8].join("|");
        match fields[0] {
            "C" => {
                java_c.insert(key, fields[8].parse().unwrap_or(0));
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
        let seed = fields[0]
            .parse::<i64>()
            .map_err(|error| format!("bad seed {}: {error}", fields[0]))?;
        let piece = (
            parse_i32(fields[1])?,
            parse_i32(fields[2])?,
            parse_i32(fields[3])?,
        );
        let list_size: usize = parse_i32(fields[4])? as usize;
        let limit: i32 = parse_i32(fields[5])?;
        let integrity: f32 = fields[6]
            .parse()
            .map_err(|error| format!("bad integrity {}: {error}", fields[6]))?;
        checked += 1;

        // 输入方块与 Java 侧一致：每个位置一块**不同**的羊毛。
        // 必须各不相同，否则 `BlockRotProcessor` 把方块换成空气后，
        // 「换掉了哪一个」在输出里看不出来，洗牌顺序就无法被验证。
        // Inputs match the Java side: a **distinct** wool colour per position. They must
        // differ, otherwise `BlockRotProcessor` turning a block into air makes "which one
        // was replaced" invisible and the shuffle order becomes unverifiable.
        let mut blocks = Vec::with_capacity(list_size);
        for index in 0..list_size {
            let pos = ((index % 4) as i32, (index / 4) as i32, 0);
            blocks.push(processor_apply::ProcessBlock {
                pos,
                world_pos: (pos.0 + piece.0, pos.1 + piece.1, pos.2 + piece.2),
                state: BlockStateSpec::parse_str(wool_colour(index))?,
                nbt: None,
            });
        }
        // `new CappedProcessor(<delegate>, ConstantInt.of(limit))`。
        // delegate 必须是 `RuleProcessor`（把方块换成别的方块），不能用
        // `BlockRotProcessor`：后者返回 `null`，而官方 `finalizeProcessing` 只在
        // `maybeAltered != null && !processedBlockInfo.equals(maybeAltered)` 时
        // 才 `replaced++`，`null` 被跳过——那样 delegate 的效果完全不可观测，
        // 洗牌顺序也就无法被验证。
        //
        // `integrity` 在这里表示「启用前 N 条规则的比例」：`0` 时规则表为空（delegate
        // 永不改动任何方块），`0.5` 时只启用一半，`1` 时全部启用。三种情况分别覆盖
        // 「一个都改不了」（`replaced` 永远 0）、「有些能改」、「全都能改」。
        //
        // `new CappedProcessor(<delegate>, ConstantInt.of(limit))`. The delegate must be a
        // `RuleProcessor` that turns a block into a different block, not a
        // `BlockRotProcessor`: the latter returns `null`, and vanilla's
        // `finalizeProcessing` only counts `replaced` when
        // `maybeAltered != null && !processedBlockInfo.equals(maybeAltered)`. A `null`
        // result is skipped, making the delegate's effect invisible and the shuffle order
        // unverifiable.
        //
        // `integrity` here is the fraction of rules enabled: `0` means an empty rule list
        // (the delegate never changes anything), `0.5` enables half, `1` enables all three,
        // covering "nothing replaceable", "some replaceable" and "all replaceable".
        let all_rules: [(&str, &str); 16] = [
            ("minecraft:white_wool", "minecraft:blue_wool"),
            ("minecraft:orange_wool", "minecraft:light_blue_wool"),
            ("minecraft:magenta_wool", "minecraft:lime_wool"),
            ("minecraft:light_blue_wool", "minecraft:yellow_wool"),
            ("minecraft:yellow_wool", "minecraft:purple_wool"),
            ("minecraft:lime_wool", "minecraft:pink_wool"),
            ("minecraft:pink_wool", "minecraft:gray_wool"),
            ("minecraft:gray_wool", "minecraft:light_gray_wool"),
            ("minecraft:light_gray_wool", "minecraft:cyan_wool"),
            ("minecraft:cyan_wool", "minecraft:orange_wool"),
            ("minecraft:purple_wool", "minecraft:magenta_wool"),
            ("minecraft:blue_wool", "minecraft:white_wool"),
            ("minecraft:brown_wool", "minecraft:red_wool"),
            ("minecraft:green_wool", "minecraft:brown_wool"),
            ("minecraft:red_wool", "minecraft:green_wool"),
            ("minecraft:black_wool", "minecraft:cyan_wool"),
        ];
        let enabled = (all_rules.len() as f32 * integrity).round() as usize;
        let mut rules = Vec::new();
        for (source, output) in all_rules.iter().take(enabled.min(all_rules.len())) {
            rules.push(ProcessorRule {
                input_predicate: RuleTest::BlockMatch {
                    block: (*source).to_owned(),
                },
                location_predicate: RuleTest::AlwaysTrue,
                position_predicate: PosRuleTest::AlwaysTrue,
                output_state: BlockStateSpec::parse_str(output)?,
                block_entity_modifier: RuleBlockEntityModifier::Passthrough,
            });
        }
        let processors = vec![Processor::Capped {
            delegate: Box::new(Processor::Rule { rules }),
            limit,
        }];
        let level = FakeLevel::new("minecraft:air");
        let processed = processor_apply::process_blocks(
            &processors,
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
        blocks_checked += processed.len();

        let Some(want_count) = java_c.get(query) else {
            return Err(format!("Java missing C for {query}"));
        };
        if *want_count != processed.len() {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!(
                    "{query}: count rust {} java {want_count}",
                    processed.len()
                ));
            }
            continue;
        }
        let want = java_s.get(query).cloned().unwrap_or_default();
        for (index, block) in processed.iter().enumerate() {
            // 只比方块名（状态里的属性 Java 侧也没输出）。
            // Compare only the block name; the Java side prints just the name too.
            let got = format!(
                "{0}|{1}|{2}|{3}",
                block.world_pos.0, block.world_pos.1, block.world_pos.2, block.state.name
            );
            if want.get(index).is_none_or(|line| *line != got) {
                mismatches += 1;
                if first.is_none() {
                    first = Some(format!(
                        "{query}[{index}]: rust {got} java {:?}",
                        want.get(index)
                    ));
                }
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
    println!("jigsaw capped: {checked} queries, {blocks_checked} survivors, all identical");
    Ok(())
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
        for seed in [0_i64, 1, 42, -7, 12_345_678_901, -98_765_432_123] {
            for piece in [
                (0, 64, 0),
                (-13, 40, 27),
                (128, -32, -96),
                (-1000, 200, 1000),
                (37, 7, -53),
            ] {
                // `gravel` 取 1（少于所有 limit）、3（等于最小的 limit）、8（多于最大 limit），
                // 这样 `min(limit.sample(), listSize)` 两侧都走到。
                // `gravel` is 1 (below every limit), 3 (equal to the smallest) and 8 (above
                // the largest), so both sides of `min(limit.sample(), listSize)` are hit.
                for gravel in [1_usize, 3, 8] {
                    queries.push(build_capped_full_query(&name, seed, piece, gravel));
                }
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
fn build_capped_full_query(name: &str, seed: i64, piece: (i32, i32, i32), gravel: usize) -> String {
    // `minecraft:trail_ruins_replaceable` 只含 `minecraft:gravel`，所以至少要放几个砾石，
    // `capped` 才会真的替换；其余方块作为不命中的对照。`gravel` 个数可变，用来覆盖
    // `min(limit.sample(), listSize)` 的两种取值：砾石多时取 `limit`，砾石少时取列表长度。
    // `minecraft:trail_ruins_replaceable` only contains `minecraft:gravel`, so the batch
    // needs several gravel blocks for `capped` to do anything; the rest are controls.
    // `gravel` varies so both sides of `min(limit.sample(), listSize)` are covered: many
    // gravel hits the limit, few gravel hits the list size.
    let mut states: Vec<&str> = vec!["minecraft:gravel"; gravel];
    states.push("minecraft:stone");
    states.push("minecraft:deepslate_bricks");
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
                nbt: None,
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
        // 同 `apply`：只输出输入里出现过的属性名，与 Java 的 `stateName(.., inputProps)` 一致。
        // Same as `apply`: only print property names the inputs had, matching Java's
        // `stateName(.., inputProps)`.
        let mut input_props: Vec<String> = Vec::new();
        for block in &blocks {
            for (name, _) in &block.state.properties {
                if !input_props.iter().any(|seen| seen == name) {
                    input_props.push(name.clone());
                }
            }
        }
        input_props.sort();
        for (index, block) in processed.iter().enumerate() {
            let shown: Vec<String> = input_props
                .iter()
                .filter_map(|name| {
                    block
                        .state
                        .properties
                        .iter()
                        .find(|(key, _)| key == name)
                        .map(|(key, value)| format!("{key}={value}"))
                })
                .collect();
            let state_text = if shown.is_empty() {
                block.state.name.clone()
            } else {
                format!("{}[{}]", block.state.name, shown.join(","))
            };
            let want = format!(
                "{}|{}|{}|{state_text}",
                block.world_pos.0, block.world_pos.1, block.world_pos.2
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
    // 单 palette 的模板每 20 个取一个控制规模；多 palette 的 20 个模板**全取**，
    // 因为它们的 `getRandomPalette` 会按位置选不同的 palette，是 `jigsaw jigsaws`
    // 只验索引、验不到方块写入的那一块。两边都用同一个 piece 位置，
    // `getRandomPalette` 的结果因此可复现。
    // Single-palette templates are sampled every 20th to keep the run small; all 20
    // multi-palette templates are included, because `getRandomPalette` picks a palette by
    // position and `jigsaw jigsaws` only verifies the index, not the block writes. Both
    // sides use the same piece position, so the roll is reproducible.
    let mut single: Vec<String> = Vec::new();
    let mut multi: Vec<String> = Vec::new();
    for &(name, _, _) in blob::ENTRIES {
        let Ok(template) = StructureTemplate::load(name) else {
            continue;
        };
        if template.blocks.is_empty() {
            continue;
        }
        if template.palettes.len() == 1 {
            single.push(name.to_owned());
        } else {
            multi.push(name.to_owned());
        }
    }
    single.sort();
    multi.sort();
    let mut picked: Vec<String> = single.iter().step_by(20).cloned().collect();
    picked.extend(multi);

    let mut queries = Vec::new();
    for name in &picked {
        // 全部 12 种 mirror × rotation 组合（`Mirror` 3 个 × `Rotation` 4 个）。
        // All 12 mirror × rotation combinations (3 mirrors × 4 rotations).
        for mirror in 0_u32..3 {
            for rotation in 0_u32..4 {
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

/// `waterpiece` 模式：带 `APPLY_WATERLOGGING` 与流体模型的 `placeInWorld` 对
/// `jigsaw::place::place_piece`，逐写入比**完整状态**（含属性）。
///
/// `waterpiece` mode: `placeInWorld` with `APPLY_WATERLOGGING` and a fluid model against
/// `jigsaw::place::place_piece`, comparing every write's full state (properties included).
fn run_water_piece(mut command: Command, work_dir: &Path) -> Result<(), String> {
    use mc_bamboo_worldgen::structure::jigsaw::place::state_id_of;
    use mc_bamboo_worldgen::structure::processor::BlockStateSpec;
    let names = water_piece_templates();
    let mut queries = Vec::new();
    for (name, template) in &names {
        let (sx, sy, sz) = template.size;
        let span = sx.max(sz) + 2;
        // 每层都铺水：模板里可含水的方块可能在任何高度（`ancient_city` 的蜡烛在 y=4~5），
        // 只铺脚下一层够不到。层数受模板高度限制，体积 = (2span+1)^2 * (sy+1)，所以只对
        // 小模板这么干，免得水位表大到把 Java 的 StringBuilder 撑爆。
        // Fill every layer: waterloggable blocks can sit at any height (ancient city candles
        // are at y=4..5), so a single layer under the template misses them. Volume is
        // (2*span+1)^2 * (sy+1), so this only applies to small templates to keep the fluid
        // table from blowing up the Java side.
        // 精确铺水：只给「模板里含可含水方块的那个格子」及其下方一层放水。这样
        // 无论模板多大，条目数都不超过方块数 × 2，且一定覆盖蜡烛/营火/台阶。
        // Precise fill: drop water exactly where the template has a waterloggable block
        // (and one block below). The entry count stays proportional to the block count no
        // matter how big the template is, and always covers candles / campfires / slabs.
        let mut targets: Vec<(i32, i32, i32)> = Vec::new();
        for block in &template.blocks {
            let entry = &template.palettes[0][block.state];
            let Ok(state) = state_id_of(&BlockStateSpec {
                name: entry.name.clone(),
                properties: entry.properties.clone(),
            }) else {
                continue;
            };
            if mc_bamboo_worldgen::structure::state_transform::property_of(state, "waterlogged")
                .is_none()
            {
                continue;
            }
            // 模板本地坐标；piece 在 (0,-1,0)，所以世界坐标 = 本地 + (0,-1,0)。
            // Template-local coordinates; the piece sits at (0,-1,0), so world = local - 1
            // in y.
            targets.push(block.pos);
        }
        if targets.is_empty() {
            continue;
        }
        // 全部 12 种 mirror × rotation 组合。注水是按模板里可含水方块的**精确位置**
        // 做的，条目数与模板体积无关，所以 12 组也不会把水位表撑大。
        // All twelve mirror × rotation combinations. The fluid goes at each waterloggable
        // block's **exact** template position, so the entry count is independent of the
        // template's volume and twelve combinations stay cheap.
        for mirror in 0_u32..3 {
            for rotation in 0_u32..4 {
                for variant in 0_u32..3 {
                    let bb = (-span, -2, -span, span, sy + 3, span);
                    // 三种水位：源水、流动水、岩浆。
                    // Three fluid variants: source water, flowing water, lava.
                    let fluid = match variant {
                        0 => "water",
                        1 => "flowing_water",
                        _ => "lava",
                    };
                    let mut entries = Vec::new();
                    for &(x, y, z) in &targets {
                        // `place_piece` 的 piece 位置是 (0,-1,0)，所以模板本地 y 落在世界 y-1。
                        // The piece sits at (0,-1,0), so template-local y lands on world y-1.
                        entries.push(format!("{fluid}:{x},{},{}", y - 1, z));
                    }
                    queries.push(format!(
                        "{name}|0|-1|0|{mirror}|{rotation}|{}|{}|{}|{}|{}|{}|{}",
                        bb.0,
                        bb.1,
                        bb.2,
                        bb.3,
                        bb.4,
                        bb.5,
                        entries.join(";")
                    ));
                }
            }
        }
    }
    let queries_path = work_dir.join("jigsaw_water_piece_queries.txt");
    fs::write(&queries_path, queries.join("\n"))
        .map_err(|error| format!("cannot write queries: {error}"))?;
    let output = command
        .arg("waterpiece")
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
    compare_water_piece(&String::from_utf8_lossy(&output.stdout), &queries)
}

/// `waterpiece` 用的模板：按「可含水方块种类」定向挑，确保 `SlabBlock` /
/// `CandleBlock` / `CampfireBlock` 三类覆写都被真实模板覆盖到。
///
/// Templates for `waterpiece`, picked by waterloggable block kind so that the
/// `SlabBlock` / `CandleBlock` / `CampfireBlock` overrides are all exercised by real
/// templates.
fn water_piece_templates() -> Vec<(String, StructureTemplate)> {
    use mc_bamboo_worldgen::structure::jigsaw::place::state_id_of;
    use mc_bamboo_worldgen::structure::processor::BlockStateSpec;

    let mut candidates: Vec<(String, StructureTemplate)> = Vec::new();
    let mut names: Vec<String> = blob::ENTRIES
        .iter()
        .map(|(name, _, _)| (*name).to_owned())
        .collect();
    names.sort();
    for name in names {
        let Ok(template) = StructureTemplate::load(&name) else {
            continue;
        };
        if template.palettes.len() != 1 || template.blocks.is_empty() {
            continue;
        }
        candidates.push((name, template));
    }

    // 一个模板里的可含水方块属于哪几类：台阶 / 蜡烛 / 营火 / 其他 / **含 lit 的火方块**。
    // Which kinds of waterloggable block a template contains: slab, candle, campfire,
    // other, and **a fire block that is lit**.
    let kinds_of = |template: &StructureTemplate| -> (bool, bool, bool, bool, bool) {
        if template.blocks.is_empty() {
            return (false, false, false, false, false);
        }
        let palette = &template.palettes[0];
        let mut slab = false;
        let mut candle = false;
        let mut campfire = false;
        let mut other = false;
        let mut lit_fire = false;
        for entry in palette {
            let Ok(state) = state_id_of(&BlockStateSpec {
                name: entry.name.clone(),
                properties: entry.properties.clone(),
            }) else {
                continue;
            };
            if mc_bamboo_worldgen::structure::state_transform::property_of(state, "waterlogged")
                .is_none()
            {
                continue;
            }
            let is_fire = entry.name.ends_with("_candle")
                || entry.name == "minecraft:campfire"
                || entry.name == "minecraft:soul_campfire";
            if is_fire
                && mc_bamboo_worldgen::structure::state_transform::property_of(state, "lit")
                    == Some("true")
            {
                lit_fire = true;
            }
            match entry.name.as_str() {
                // 台阶的判据就是 `type` 属性（26.3 里只有台阶用它）。
                // A slab is anything with the `type` property (only slabs use it in 26.3).
                _ if mc_bamboo_worldgen::structure::state_transform::property_of(state, "type")
                    .is_some() =>
                {
                    slab = true
                }
                name if name.ends_with("_candle") => candle = true,
                "minecraft:campfire" | "minecraft:soul_campfire" => campfire = true,
                _ => other = true,
            }
        }
        (slab, candle, campfire, other, lit_fire)
    };

    // 第一类取台阶、第二类取营火、第三类取**含 lit=true 火方块**的（这是 `lit` 清位唯一
    // 能被真实模板触发的地方：`ancient_city/barracks` 与 `trial_chambers/ominous_vault`
    // 里的蜡烛就是 lit），第四类补其他可含水方块。
    // Kinds: slabs, campfires, templates with a **lit** fire block (the only real templates
    // where the `lit` clearing branch is reachable — the candles in `ancient_city/barracks`
    // and `trial_chambers/ominous_vault` are lit), then other waterloggable blocks.
    let mut picked: Vec<(String, StructureTemplate)> = Vec::new();
    let mut picked_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for wanted in 0..4 {
        let limit = if wanted == 3 { 12 } else { 6 };
        let mut taken = 0;
        for (name, template) in &candidates {
            if taken == limit {
                break;
            }
            if picked_names.contains(name) {
                continue;
            }
            let kinds = kinds_of(template);
            let hit = match wanted {
                0 => kinds.0,
                1 => kinds.1,
                2 => kinds.4,
                // 其他可含水方块，且不属于前三类。
                // Waterloggable but none of the first three kinds.
                _ => kinds.3 && !kinds.0 && !kinds.1 && !kinds.4,
            };
            if hit {
                picked.push((name.clone(), template.clone()));
                picked_names.insert(name.clone());
                taken += 1;
            }
        }
    }
    // 打印选中的模板，报告里要能看出覆盖了哪几类。
    // Print the picked templates so the report can show which kinds are covered.
    if std::env::var_os("JIGSAW_DUMP").is_some() {
        for (name, _) in &picked {
            eprintln!("waterpiece template {name}");
        }
    }
    picked
}

/// 解析 Java 的 `waterpiece` 输出并与 Rust 侧逐写入比较。
///
/// Parse the Java `waterpiece` output and compare every write with the Rust side.
fn compare_water_piece(stdout: &str, queries: &[String]) -> Result<(), String> {
    let mut java_q: HashMap<String, (usize, usize)> = HashMap::new();
    let mut java_w: HashMap<String, Vec<String>> = HashMap::new();
    let mut java_keys: HashMap<String, String> = HashMap::new();
    for line in stdout.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() < 15 {
            continue;
        }
        // 查询本身是 13 段（0..=12），`Q` 行是 `Q|13 段 key|placed|writes|fluidQueries`。
        // A query has 13 fields (0..=12); a `Q` line is
        // `Q|the 13 key fields|placed|writes|fluidQueries`.
        let key = fields[1..14].join("|");
        match fields[0] {
            "Q" => {
                java_q.insert(
                    key.clone(),
                    (
                        fields[15].parse().unwrap_or(0),
                        fields[16].parse().unwrap_or(0),
                    ),
                );
                java_keys.insert(key, fields[1..14].join("|"));
            }
            "W" => {
                java_w.entry(key).or_default().push(fields[14..].join("|"));
            }
            _ => {}
        }
    }

    let mut checked = 0usize;
    let mut writes_checked = 0usize;
    let mut fluid_queries = 0usize;
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
        // 水位表。
        // The fluid table.
        let mut fluids: HashMap<(i32, i32, i32), (bool, bool)> = HashMap::new();
        for entry in fields[12].split(';') {
            let Some((kind, coords)) = entry.split_once(':') else {
                continue;
            };
            let xyz: Vec<i32> = coords
                .split(',')
                .map(|value| value.parse::<i32>().unwrap_or(0))
                .collect();
            let (is_water, source) = match kind {
                "water" => (true, true),
                "flowing_water" => (true, false),
                "lava" => (false, true),
                _ => continue,
            };
            fluids.insert((xyz[0], xyz[1], xyz[2]), (is_water, source));
        }
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
        /// 记录写入，并按水位表回答 `fluid_at`；`block_state` 返回最后一次写的状态
        /// （watering 第二段要读刚写的方块）。
        ///
        /// Records writes, answers `fluid_at` from the fluid table, and returns the last
        /// written state from `block_state` (the second watering pass reads it).
        #[derive(Default)]
        struct Recorder {
            writes: Vec<(i32, i32, i32, u32)>,
            fluids: HashMap<(i32, i32, i32), (bool, bool)>,
            last: HashMap<(i32, i32, i32), u32>,
        }
        impl mc_bamboo_worldgen::structure::jigsaw::place::BlockWriter for Recorder {
            fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
                self.writes.push((x, y, z, state));
                self.last.insert((x, y, z), state);
                true
            }
            fn fluid_at(&self, x: i32, y: i32, z: i32) -> Option<(bool, bool)> {
                Some(
                    self.fluids
                        .get(&(x, y, z))
                        .copied()
                        .unwrap_or((false, false)),
                )
            }
            fn block_state(&self, x: i32, y: i32, z: i32) -> u32 {
                self.last
                    .get(&(x, y, z))
                    .copied()
                    .unwrap_or(mc_bamboo_data::blocks::AIR)
            }
        }
        let level = Level;
        let mut recorder = Recorder {
            fluids,
            ..Recorder::default()
        };
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
            &mc_bamboo_worldgen::random::Random::Legacy(LegacyRandom::new(0)),
            &mut recorder,
        )?;
        checked += 1;
        writes_checked += recorder.writes.len();
        let key = fields[..13].join("|");
        let Some((placed, count, fluid_queries_java)) = java_q
            .get(&key)
            .map(|(count, queries)| (true, *count, *queries))
        else {
            return Err(format!("Java missing Q for {key}"));
        };
        let _ = placed;
        fluid_queries += fluid_queries_java;
        let _ = java_keys.get(&key);
        if count != recorder.writes.len() {
            mismatches += 1;
            if first.is_none() {
                // 查询串（尤其水位表）很长，只报前 12 段。
                // The query string (especially the fluid table) is long; report only the
                // first twelve fields.
                first = Some(format!(
                    "{}: writes rust {} java {count}",
                    fields[..12].join("|"),
                    recorder.writes.len()
                ));
            }
            continue;
        }
        let want = java_w.get(&key).cloned().unwrap_or_default();
        for (index, (x, y, z, state)) in recorder.writes.iter().enumerate() {
            let got = format!("{x}|{y}|{z}|{}", state_with_properties(*state));
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
            "checked {checked} water queries, expected {}",
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
        "jigsaw waterpiece: {checked} queries, {writes_checked} writes, \
         {fluid_queries} fluid queries, all identical"
    );
    Ok(())
}

/// 状态的比较文本：`minecraft:oak_slab[type=bottom,waterlogged=false]`，属性按名字排序。
///
/// Comparable state text with every property, sorted by name.
fn state_with_properties(state: u32) -> String {
    let name = mc_bamboo_worldgen::structure::jigsaw::place::state_text(state);
    let properties = mc_bamboo_worldgen::structure::jigsaw::place::state_properties(state);
    if properties.is_empty() {
        return name;
    }
    format!("{name}[{}]", properties.join(","))
}

/// `entities` 模式：官方 `StructureTemplate.placeEntities` 的几何与 nbt 改写对
/// `jigsaw::place::place_entities`，逐实体比。
///
/// `entities` mode: vanilla's `StructureTemplate.placeEntities` geometry and nbt rewriting
/// against `jigsaw::place::place_entities`, compared entity by entity.
fn run_entities(mut command: Command, work_dir: &Path) -> Result<(), String> {
    // 只挑带实体的模板。
    // Only templates that carry entities.
    let mut names: Vec<String> = blob::ENTRIES
        .iter()
        .filter(|(name, _, _)| {
            StructureTemplate::load(name).is_ok_and(|template| !template.entities.is_empty())
        })
        .map(|(name, _, _)| (*name).to_owned())
        .collect();
    names.sort();

    let mut queries = Vec::new();
    for name in &names {
        let (sx, sy, sz) = StructureTemplate::load(name)
            .map_err(|error| format!("cannot load {name}: {error}"))?
            .size;
        let span = sx.max(sz) + 2;
        for mirror in 0_u32..3 {
            for rotation in 0_u32..4 {
                // 一半查询用放得下的盒，一半用只覆盖一角的盒，两条分支都走。
                // Half the queries use a box that fits, half one that clips a corner.
                let bb = if (mirror + rotation) % 2 == 0 {
                    (-span, 0, -span, span, sy + 2, span)
                } else {
                    (0, 0, 0, span / 2, sy / 2, span / 2)
                };
                queries.push(format!(
                    "{name}|0|0|0|{mirror}|{rotation}|{}|{}|{}|{}|{}|{}",
                    bb.0, bb.1, bb.2, bb.3, bb.4, bb.5
                ));
            }
        }
    }
    let queries_path = work_dir.join("jigsaw_entities_queries.txt");
    fs::write(&queries_path, queries.join("\n"))
        .map_err(|error| format!("cannot write queries: {error}"))?;
    let output = command
        .arg("entities")
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
    compare_entities(&String::from_utf8_lossy(&output.stdout), &queries)
}

/// 解析 Java 的 `entities` 输出并与 Rust 侧逐实体比较。
///
/// Parse the Java `entities` output and compare it entity by entity with the Rust side.
fn compare_entities(stdout: &str, queries: &[String]) -> Result<(), String> {
    let mut java: HashMap<String, Vec<String>> = HashMap::new();
    let mut java_counts: HashMap<String, usize> = HashMap::new();
    for line in stdout.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() < 14 {
            continue;
        }
        let key = fields[1..13].join("|");
        match fields[0] {
            "Q" => {
                java_counts.insert(key, fields[13].parse().unwrap_or(0));
            }
            "E" => {
                java.entry(key).or_default().push(fields[13..].join("|"));
            }
            _ => {}
        }
    }

    let mut checked = 0usize;
    let mut entities_checked = 0usize;
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
        let placed = mc_bamboo_worldgen::structure::jigsaw::place::place_entities(
            &template,
            piece,
            mirror,
            rotation,
            Some(bb),
        );
        checked += 1;
        entities_checked += placed.len();
        let key = fields.join("|");
        let Some(total) = java_counts.get(&key) else {
            return Err(format!("Java missing Q for {key}"));
        };
        if *total != template.entities.len() {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!(
                    "{key}: template entities rust {} java {total}",
                    template.entities.len()
                ));
            }
            continue;
        }
        let want = java.get(&key).cloned().unwrap_or_default();
        if want.len() != placed.len() {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!(
                    "{key}: placed entities rust {} java {}",
                    placed.len(),
                    want.len()
                ));
            }
            continue;
        }
        for (index, entity) in placed.iter().enumerate() {
            let got = entity_line(entity);
            if want[index] != got {
                mismatches += 1;
                if first.is_none() {
                    first = Some(format!("{key}[{index}]: rust {got} java {}", want[index]));
                }
            }
        }
    }
    if checked != queries.len() {
        return Err(format!(
            "checked {checked} entity queries, expected {}",
            queries.len()
        ));
    }
    if mismatches > 0 {
        return Err(format!(
            "{mismatches} differences over {checked} queries; first: {}",
            first.unwrap_or_default()
        ));
    }
    println!("jigsaw entities: {checked} queries, {entities_checked} entities, all identical");
    Ok(())
}

/// 一个实体的比较行，顺序与 Java 侧 `E|key|...` 之后那段一致：
/// `blockPos(3) | pos 三个 raw double | yRot raw float | xRot raw float | UUID 是否存在 |
/// 改写后的 block_pos | id`。
///
/// One entity's comparison line, in the same order as the Java `E|key|...` payload:
/// `blockPos (three ints) | three raw doubles | raw yRot | raw xRot | UUID present? |
/// rewritten block_pos | id`.
fn entity_line(entity: &mc_bamboo_worldgen::structure::jigsaw::place::PlacedEntity) -> String {
    let (block_pos, id) = match &entity.tag {
        mc_bamboo_nbt::Tag::Compound(entries) => {
            let block_pos = entries
                .iter()
                .find(|(key, _)| key == "block_pos")
                .map(|(_, value)| match value {
                    mc_bamboo_nbt::Tag::IntArray(values) if values.len() == 3 => {
                        format!("{},{},{}", values[0], values[1], values[2])
                    }
                    _ => "-".to_owned(),
                })
                .unwrap_or_else(|| "-".to_owned());
            let id = entries
                .iter()
                .find(|(key, _)| key == "id")
                .map(|(_, value)| match value {
                    mc_bamboo_nbt::Tag::String(text) => text.clone(),
                    _ => String::new(),
                })
                .unwrap_or_default();
            (block_pos, id)
        }
        _ => ("-".to_owned(), String::new()),
    };
    let has_uuid = match &entity.tag {
        mc_bamboo_nbt::Tag::Compound(entries) => entries.iter().any(|(key, _)| key == "UUID"),
        _ => false,
    };
    format!(
        "{},{},{}|{}|{}|{}|{}|{}|{}|{block_pos}|{id}",
        entity.block_pos.0,
        entity.block_pos.1,
        entity.block_pos.2,
        // `to_bits()` 是 `u64`/`u32`，Java 的 `doubleToRawLongBits` /
        // `floatToRawIntBits` 是带符号的，按位转过去才可比。
        // `to_bits()` yields `u64`/`u32`; Java's `doubleToRawLongBits` /
        // `floatToRawIntBits` are signed, so reinterpret the same bits as signed.
        entity.x.to_bits() as i64,
        entity.y.to_bits() as i64,
        entity.z.to_bits() as i64,
        entity.y_rot.to_bits() as i32,
        entity.x_rot.to_bits() as i32,
        i32::from(has_uuid),
    )
}
fn compare_place_piece(stdout: &str, queries: &[String]) -> Result<(), String> {
    let mut java_q: HashMap<String, (bool, usize)> = HashMap::new();
    let mut java_w: HashMap<String, Vec<String>> = HashMap::new();
    let mut java_seeds: HashMap<String, Vec<String>> = HashMap::new();
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
            "S" => {
                java_seeds
                    .entry(key)
                    .or_default()
                    .push(fields[13..].join("|"));
            }
            _ => {}
        }
    }

    let mut checked = 0usize;
    let mut writes_checked = 0usize;
    let mut seeds_checked = 0usize;
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
            seeds: Vec<(i32, i32, i32, i64)>,
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
            // 只记 `LootTableSeed`：Java 侧也只输出这个。
            // Only the `LootTableSeed`; the Java side prints just that too.
            fn set_block_entity(&mut self, x: i32, y: i32, z: i32, nbt: &mc_bamboo_nbt::Tag) {
                if let mc_bamboo_nbt::Tag::Compound(entries) = nbt
                    && let Some((_, mc_bamboo_nbt::Tag::Long(seed))) =
                        entries.iter().find(|(key, _)| key == "LootTableSeed")
                {
                    self.seeds.push((x, y, z, *seed));
                }
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
            &mc_bamboo_worldgen::random::Random::Legacy(LegacyRandom::new(0)),
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
        // 随机容器的 `LootTableSeed`。官方只在「该位置带 nbt 且方块实体是
        // `RandomizableContainer`」时覆盖，所以这一项同时验证判定与取数次数。
        // The `LootTableSeed` of randomizable containers; vanilla only overwrites it when
        // the position carries nbt and the block entity is a `RandomizableContainer`.
        let mut my_seeds = recorder.seeds.clone();
        my_seeds.sort_unstable();
        let want_seeds = java_seeds.get(&key).cloned().unwrap_or_default();
        if want_seeds.len() != my_seeds.len() {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!(
                    "{key}: container seeds rust {} java {}",
                    my_seeds.len(),
                    want_seeds.len()
                ));
            }
        } else {
            for (index, (x, y, z, seed)) in my_seeds.iter().enumerate() {
                let got = format!("{x}|{y}|{z}|{seed}");
                if want_seeds.get(index).is_none_or(|line| *line != got) {
                    mismatches += 1;
                    if first.is_none() {
                        first = Some(format!(
                            "{key} seed[{index}]: rust {got} java {:?}",
                            want_seeds.get(index)
                        ));
                    }
                }
            }
        }
        seeds_checked += my_seeds.len();
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
    println!(
        "jigsaw placepiece: {checked} queries, {writes_checked} writes, {seeds_checked} seeds, \
         all identical"
    );
    Ok(())
}

/// `bemod` 模式：`RuleBlockEntityModifier.apply` 对官方同名方法，逐键比 NBT，
/// 并用「随后再取一次 `nextLong`」验证取数次数一致。
///
/// `bemod` mode: `RuleBlockEntityModifier.apply` against vanilla's, comparing the NBT key by
/// key plus one extra `nextLong` draw to prove the same number of draws.
fn run_block_entity_modifier(mut command: Command, work_dir: &Path) -> Result<(), String> {
    // 26.3 的 `processor_list` 里 `append_loot` 有两张表；另外三种也要覆盖（不是数据驱动，
    // 因为 `clear` / `passthrough` / `append_static` 在数据里没出现）。
    // The 26.3 `processor_list` uses two loot tables for `append_loot`; the other three
    // kinds are covered too even though the data never uses them.
    //
    // `existing` 覆盖六种形态，重点在**递归合并**与**直接覆盖**会给出不同结果的那些：
    // 同名顶层键、同名嵌套键、只在一侧存在的嵌套键、值为列表、值为纯量。
    // 数据里的 `data` 与每种 `existing` 的组合只要有一处不同，改坏「递归合并」就能被发现。
    // `existing` covers six shapes chosen so recursive merge and plain overwrite differ:
    // same top-level key, same nested key, nested key on one side only, list value, scalar.
    let mut queries = Vec::new();
    for seed in [0_i64, 1, 42, -7, 12_345_678_901, -98_765_432_123] {
        for existing in [
            "-",
            "{Keep:5,LootTable:\"minecraft:old\"}",
            "{Nested:{B:2},Only:3}",
            // 与数据里的 `LootTable` 同名：覆盖 vs 保留能区分。
            // Same key as the data's `LootTable`: overwrite and keep differ.
            "{LootTable:\"minecraft:keep_me\",Extra:1}",
            // 同名嵌套键与只在 existing 里存在的键。
            // A nested key present in both, plus one only in `existing`.
            "{Nested:{A:9,B:9},Deep:{X:{Y:1}}}",
            // 标量与嵌套混合。
            // Scalars mixed with nesting.
            "{Scalar:\"s\",Deeper:{X:{Y:{Z:1}}}}",
            // 列表值：与数据里的列表同键冲突时，「整体替换」与「逐元素合并」结果不同。
            // List values: when the key collides with a list in the data, replacing the
            // whole list and merging element-wise differ.
            "{List:[1,2,3],Other:{L:[4b,5b]}}",
        ] {
            for loot in [
                "minecraft:archaeology/trail_ruins_common",
                "minecraft:archaeology/trail_ruins_rare",
            ] {
                queries.push(format!("append_loot|{seed}|{loot}|{existing}"));
            }
            queries.push(format!("clear|{seed}|minecraft:empty|{existing}"));
            queries.push(format!("passthrough|{seed}|minecraft:empty|{existing}"));
            // 数据的 data 与 existing 的嵌套键**不同**，否则「递归合并」与
            // 「直接覆盖」结果相同，改坏也发现不了（早前踩过）。
            // The data's keys differ from `existing`'s nested keys; otherwise recursive
            // merge and plain overwrite agree and a break goes unnoticed.
            queries.push(format!("append_static|{seed}|minecraft:empty|{existing}"));
            queries.push(format!(
                "append_static_data|{seed}|minecraft:empty|{existing}"
            ));
        }
    }
    let queries_path = work_dir.join("jigsaw_bemod_queries.txt");
    fs::write(&queries_path, queries.join("\n"))
        .map_err(|error| format!("cannot write queries: {error}"))?;
    let output = command
        .arg("bemod")
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
    compare_block_entity_modifier(&String::from_utf8_lossy(&output.stdout), &queries)
}

fn compare_block_entity_modifier(stdout: &str, queries: &[String]) -> Result<(), String> {
    let mut java_m: HashMap<String, Option<Value>> = HashMap::new();
    let mut java_n: HashMap<String, i64> = HashMap::new();
    for line in stdout.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() < 6 {
            continue;
        }
        let key = fields[1..5].join("|");
        match fields[0] {
            "M" => {
                let rest = fields[5..].join("|");
                java_m.insert(
                    key,
                    if rest == "-" {
                        None
                    } else {
                        Some(
                            serde_json::from_str(&rest)
                                .map_err(|error| format!("bad M json: {error}"))?,
                        )
                    },
                );
            }
            "N" => {
                java_n.insert(key, fields[5].parse().unwrap_or(0));
            }
            _ => {}
        }
    }

    let mut checked = 0usize;
    let mut mismatches = 0usize;
    let mut first: Option<String> = None;
    for query in queries {
        let fields: Vec<&str> = query.split('|').collect();
        let kind = fields[0];
        let seed = fields[1]
            .parse::<i64>()
            .map_err(|error| format!("bad seed: {error}"))?;
        let loot_table = fields[2];
        let existing_text = fields[3];
        let existing = if existing_text == "-" {
            None
        } else {
            Some(parse_snbt(existing_text)?)
        };

        let modifier = match kind {
            "append_loot" => {
                mc_bamboo_worldgen::structure::processor::RuleBlockEntityModifier::AppendLoot {
                    loot_table: loot_table.to_owned(),
                }
            }
            "clear" => mc_bamboo_worldgen::structure::processor::RuleBlockEntityModifier::Clear,
            "passthrough" => {
                mc_bamboo_worldgen::structure::processor::RuleBlockEntityModifier::Passthrough
            }
            "append_static" => {
                mc_bamboo_worldgen::structure::processor::RuleBlockEntityModifier::AppendStatic {
                    data: existing
                        .clone()
                        .unwrap_or(mc_bamboo_nbt::Tag::Compound(Vec::new())),
                }
            }
            // 用一段与 existing 嵌套键**不同**的固定 data，才能区分「递归合并」与「覆盖」。
            // A fixed data whose nested keys differ from `existing`, so recursive merge and
            // plain overwrite give different results.
            "append_static_data" => {
                mc_bamboo_worldgen::structure::processor::RuleBlockEntityModifier::AppendStatic {
                    data: mc_bamboo_nbt::Tag::Compound(vec![
                        (
                            "Nested".to_owned(),
                            mc_bamboo_nbt::Tag::Compound(vec![(
                                "A".to_owned(),
                                mc_bamboo_nbt::Tag::Int(1),
                            )]),
                        ),
                        ("Extra".to_owned(), mc_bamboo_nbt::Tag::Int(7)),
                    ]),
                }
            }
            other => return Err(format!("unknown modifier {other}")),
        };
        let random = mc_bamboo_worldgen::random::Random::Legacy(LegacyRandom::new(seed));
        checked += 1;
        let applied = modifier
            .apply(&random, existing.as_ref())
            .map_err(|error| format!("apply {kind}: {error}"))?;
        let got = applied.as_ref().map(canonical_tag);
        let key = fields[..4].join("|");
        let Some(want) = java_m.get(&key) else {
            return Err(format!("Java missing M for {key}"));
        };
        if &got != want {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!(
                    "{key}: rust {} java {}",
                    got.map_or("-".to_owned(), |v| v.to_string()),
                    want.as_ref().map_or("-".to_owned(), |v| v.to_string())
                ));
            }
        }
        // 随后再取一次 `nextLong`，验证取数次数一致。
        // One more `nextLong` to prove the same number of draws.
        let after = random.next_long();
        if java_n.get(&key).is_some_and(|want| *want != after) {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!(
                    "{key}: draw-after rust {after} java {:?}",
                    java_n.get(&key)
                ));
            }
        }
    }
    if checked != queries.len() {
        return Err(format!(
            "checked {checked} modifier queries, expected {}",
            queries.len()
        ));
    }
    if mismatches > 0 {
        return Err(format!(
            "{mismatches} differences over {checked} queries; first: {}",
            first.unwrap_or_default()
        ));
    }
    println!("jigsaw bemod: {checked} queries, all identical");
    Ok(())
}

/// 极简 SNBT：只支持 `{Key:value,...}`、字符串（双引号）、整数。
/// 对拍里只用固定几组手写输入，不需要完整解析器。
///
/// Minimal SNBT for the fixed hand-written inputs: compounds, quoted strings and ints.
/// 解析 SNBT 子集，返回 `Tag`。
///
/// 支持：复合 `{k:v,...}`、列表 `[v,...]`（元素必须同型）、带引号的字符串
/// （含 \`\\"\` 与 \`\\\\\` 转义）、以及带类型后缀的数字
/// `1b` / `2s` / `3` / `4L` / `5.0f` / `6.0d`。裸 `true`/`false` 按 Java 的
/// SNBT 规范当作 byte。
///
/// 不支持数组（`[B;...]` / `[I;...]` / `[L;...]`）与下标语法；对拍用不到。
///
/// Parses an SNBT subset into a `Tag`: compounds, homogeneous lists, quoted strings with
/// \`\\"\` / \`\\\\\` escapes, and numbers with the `b` / `s` / `L` / `f` / `d`
/// suffixes. Bare `true`/`false` become bytes, as in Java's SNBT. Arrays and index syntax
/// are unsupported; the oracle does not need them.
fn parse_snbt(text: &str) -> Result<mc_bamboo_nbt::Tag, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0usize;
    let tag = parse_snbt_value(&chars, &mut at)?;
    skip_snbt_ws(&chars, &mut at);
    if at != chars.len() {
        return Err(format!("trailing snbt input in {text}"));
    }
    Ok(tag)
}

fn skip_snbt_ws(chars: &[char], at: &mut usize) {
    while *at < chars.len() && chars[*at].is_whitespace() {
        *at += 1;
    }
}

/// 解析一个值，推进 `at`。
///
/// Parse one value and advance `at`.
fn parse_snbt_value(chars: &[char], at: &mut usize) -> Result<mc_bamboo_nbt::Tag, String> {
    skip_snbt_ws(chars, at);
    let Some(&first) = chars.get(*at) else {
        return Err("unexpected end of snbt".to_owned());
    };
    match first {
        '{' => parse_snbt_compound(chars, at),
        '[' => parse_snbt_list(chars, at),
        '"' => parse_snbt_string(chars, at),
        _ => parse_snbt_scalar(chars, at),
    }
}

fn parse_snbt_compound(chars: &[char], at: &mut usize) -> Result<mc_bamboo_nbt::Tag, String> {
    use mc_bamboo_nbt::Tag;
    *at += 1; // '{'
    let mut entries = Vec::new();
    loop {
        skip_snbt_ws(chars, at);
        match chars.get(*at) {
            Some('}') => {
                *at += 1;
                break;
            }
            Some(',') => {
                *at += 1;
                continue;
            }
            None => return Err("unterminated snbt compound".to_owned()),
            _ => {}
        }
        // key：带引号或裸标识符。
        // The key, quoted or bare.
        let key = if chars.get(*at) == Some(&'"') {
            match parse_snbt_string(chars, at)? {
                Tag::String(text) => text,
                _ => return Err("snbt key is not a string".to_owned()),
            }
        } else {
            let start = *at;
            while *at < chars.len() && !matches!(chars[*at], ':' | ',' | '}') {
                *at += 1;
            }
            chars[start..*at]
                .iter()
                .collect::<String>()
                .trim()
                .to_owned()
        };
        skip_snbt_ws(chars, at);
        if chars.get(*at) != Some(&':') {
            return Err("snbt entry has no colon".to_owned());
        }
        if key.is_empty() {
            return Err("snbt entry has an empty key".to_owned());
        }
        *at += 1;
        let value = parse_snbt_value(chars, at)?;
        entries.push((key, value));
        skip_snbt_ws(chars, at);
        match chars.get(*at) {
            Some(',') => *at += 1,
            Some('}') => {
                *at += 1;
                break;
            }
            _ => return Err("snbt compound entry is not terminated".to_owned()),
        }
    }
    Ok(Tag::Compound(entries))
}

fn parse_snbt_list(chars: &[char], at: &mut usize) -> Result<mc_bamboo_nbt::Tag, String> {
    use mc_bamboo_nbt::Tag;
    *at += 1; // '['
    let mut values = Vec::new();
    loop {
        skip_snbt_ws(chars, at);
        match chars.get(*at) {
            Some(']') => {
                *at += 1;
                break;
            }
            Some(',') => {
                *at += 1;
                continue;
            }
            None => return Err("unterminated snbt list".to_owned()),
            _ => {}
        }
        values.push(parse_snbt_value(chars, at)?);
        skip_snbt_ws(chars, at);
        match chars.get(*at) {
            Some(',') => *at += 1,
            Some(']') => {
                *at += 1;
                break;
            }
            _ => return Err("snbt list entry is not terminated".to_owned()),
        }
    }
    // 官方 SNBT 要求列表元素同型；这里也校验，否则编码时会写出非法 NBT。
    // Vanilla SNBT requires homogeneous lists; checking here keeps the encoder honest.
    if let Some(first) = values.first() {
        let kind = std::mem::discriminant(first);
        if values
            .iter()
            .any(|value| std::mem::discriminant(value) != kind)
        {
            return Err("snbt list mixes element types".to_owned());
        }
    }
    Ok(Tag::List(values))
}

fn parse_snbt_string(chars: &[char], at: &mut usize) -> Result<mc_bamboo_nbt::Tag, String> {
    use mc_bamboo_nbt::Tag;
    *at += 1; // '"'
    let mut out = String::new();
    loop {
        let Some(&ch) = chars.get(*at) else {
            return Err("unterminated snbt string".to_owned());
        };
        *at += 1;
        match ch {
            '"' => break,
            '\\' => {
                let Some(&escaped) = chars.get(*at) else {
                    return Err("unterminated snbt escape".to_owned());
                };
                *at += 1;
                out.push(match escaped {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    other => other,
                });
            }
            other => out.push(other),
        }
    }
    Ok(Tag::String(out))
}

fn parse_snbt_scalar(chars: &[char], at: &mut usize) -> Result<mc_bamboo_nbt::Tag, String> {
    use mc_bamboo_nbt::Tag;
    let start = *at;
    while *at < chars.len() && !matches!(chars[*at], ',' | ']' | '}' | ':') {
        *at += 1;
    }
    let raw = chars[start..*at].iter().collect::<String>();
    let text = raw.trim();
    if text.is_empty() {
        return Err("empty snbt scalar".to_owned());
    }
    if text == "true" {
        return Ok(Tag::Byte(1));
    }
    if text == "false" {
        return Ok(Tag::Byte(0));
    }
    // 带后缀的数字：b / s / L / f / d。后缀区分大小写（Java 用 `L` 表示 long）。
    // Numbers with a `b` / `s` / `L` / `f` / `d` suffix; the suffix is case sensitive
    // (`L` means long in Java's SNBT).
    let (body, kind) = match text.chars().last() {
        Some(suffix @ ('b' | 's' | 'L' | 'f' | 'd')) => (&text[..text.len() - 1], Some(suffix)),
        _ => (text, None),
    };
    match kind {
        Some('b') => body
            .parse::<i8>()
            .map(Tag::Byte)
            .map_err(|error| format!("bad snbt byte {text}: {error}")),
        Some('s') => body
            .parse::<i16>()
            .map(Tag::Short)
            .map_err(|error| format!("bad snbt short {text}: {error}")),
        Some('L') => body
            .parse::<i64>()
            .map(Tag::Long)
            .map_err(|error| format!("bad snbt long {text}: {error}")),
        Some('f') => body
            .parse::<f32>()
            .map(Tag::Float)
            .map_err(|error| format!("bad snbt float {text}: {error}")),
        Some('d') => body
            .parse::<f64>()
            .map(Tag::Double)
            .map_err(|error| format!("bad snbt double {text}: {error}")),
        _ => {
            // 无后缀：带小数点或指数按 double，否则按 int（Java SNBT 也是这个规则）。
            // No suffix: a decimal point or exponent means double, otherwise int, matching
            // Java's SNBT rules.
            if body.contains('.') || body.contains('e') || body.contains('E') {
                body.parse::<f64>()
                    .map(Tag::Double)
                    .map_err(|error| format!("bad snbt double {text}: {error}"))
            } else {
                body.parse::<i32>()
                    .map(Tag::Int)
                    .map_err(|error| format!("bad snbt int {text}: {error}"))
            }
        }
    }
}

/// `rc` 模式：`is_randomizable_container` 对官方 `instanceof RandomizableContainer`，
/// 全量 1286 个方块逐个比。
///
/// `rc` mode: `is_randomizable_container` against vanilla's
/// `instanceof RandomizableContainer` for every one of the 1286 blocks.
fn run_randomizable_container(mut command: Command, work_dir: &Path) -> Result<(), String> {
    let mut names: Vec<&str> = mc_bamboo_data::blocks::BLOCKS
        .iter()
        .map(|block| block.name)
        .collect();
    names.sort();
    let queries_path = work_dir.join("jigsaw_rc_queries.txt");
    fs::write(&queries_path, names.join("\n"))
        .map_err(|error| format!("cannot write queries: {error}"))?;
    let output = command
        .arg("rc")
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
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut checked = 0usize;
    let mut randomizable = 0usize;
    let mut mismatches = 0usize;
    let mut first: Option<String> = None;
    for line in stdout.lines() {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() != 3 || fields[0] != "R" {
            continue;
        }
        let name = fields[1];
        let want = fields[2] == "1";
        let got = mc_bamboo_worldgen::structure::jigsaw::place::is_randomizable_container(name);
        checked += 1;
        if want {
            randomizable += 1;
        }
        if got != want {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!("{name}: rust {got} java {want}"));
            }
        }
    }
    if checked != names.len() {
        return Err(format!(
            "checked {checked} blocks, expected {}",
            names.len()
        ));
    }
    if mismatches > 0 {
        return Err(format!(
            "{mismatches}/{checked} blocks differ; first: {}",
            first.unwrap_or_default()
        ));
    }
    println!("jigsaw rc: {checked} blocks, {randomizable} randomizable, all identical");
    Ok(())
}

/// `piecesbiome` 模式：像 `pieces` 一样比结构件，但**先按群系挑起点**。
///
/// `pieces` 用固定的 12 个区块，实测只有 5.8% 的查询能通过 `isValidBiome`，
/// 其余 94% 只验证了「返回 `None`」。这里对每个 (结构, 种子) 在半径 `RADIUS` 的
/// 区块范围内扫 `column_has_valid_biome`，挑出前 `PER_STRUCTURE` 个真正属于该结构
/// 群系的区块，于是几乎每个查询都会跑完 `addPieces`。
///
/// `piecesbiome` mode: compares pieces like `pieces` but **picks start points by biome**
/// first. `pieces` uses twelve fixed chunks and only 5.8% of its queries pass
/// `isValidBiome`, so the rest only exercise the "returns None" path. Here each
/// (structure, seed) scans `column_has_valid_biome` over a chunk square of radius
/// `RADIUS` and keeps the first `PER_STRUCTURE` chunks whose column really holds the
/// structure's biome, so nearly every query runs `add_pieces` to completion.
fn run_pieces_biome(mut command: Command, work_dir: &Path) -> Result<(), String> {
    /// 每个方向扫多少个区块（`2*RADIUS+1` 见方）。
    /// How many chunks to scan per axis (`2*RADIUS+1` square).
    const RADIUS: i32 = 64;
    /// 每个 (结构, 种子) 最多保留几个命中区块。
    /// At most this many matching chunks per (structure, seed).
    const PER_STRUCTURE: usize = 3;

    let worldgen = Worldgen::load()?;
    let settings = worldgen
        .settings("minecraft:overworld")
        .ok_or("missing overworld settings")?;
    let dims = (settings.min_y, settings.height);
    let raw: Value =
        serde_json::from_str(WORLDGEN).map_err(|error| format!("bad worldgen.json: {error}"))?;
    let structures = Structures::parse(raw.get("structure").ok_or("missing structure")?)?;
    let mut allowed_cache: HashMap<String, HashSet<String>> = HashMap::new();
    for id in &STRUCTURES {
        let def = structures
            .get(id)
            .ok_or_else(|| format!("missing structure def {id}"))?;
        allowed_cache.insert((*id).to_owned(), allowed_biome_names(def)?);
    }

    let mut queries = Vec::new();
    let mut scanned = 0usize;
    let mut per_structure: HashMap<String, usize> = HashMap::new();
    for &seed in SEEDS.iter().take(3) {
        let generator = Generator::new(&worldgen, seed)?;
        for id in &STRUCTURES {
            let allowed = &allowed_cache[*id];
            let mut found = 0usize;
            'scan: for dz in -RADIUS..=RADIUS {
                for dx in -RADIUS..=RADIUS {
                    let chunk = (dx, dz);
                    scanned += 1;
                    if !column_has_valid_biome(&generator, chunk, allowed, dims)? {
                        continue;
                    }
                    queries.push(Query {
                        id: (*id).to_owned(),
                        seed,
                        cx: chunk.0,
                        cz: chunk.1,
                    });
                    found += 1;
                    if found == PER_STRUCTURE {
                        break 'scan;
                    }
                }
            }
            *per_structure.entry((*id).to_owned()).or_default() += found;
            if found == 0 {
                println!("note: {id} seed {seed} has no valid-biome chunk within radius {RADIUS}");
            }
        }
    }
    println!(
        "scanned {scanned} (structure, chunk) columns by biome, kept {} queries",
        queries.len()
    );

    let queries_path = work_dir.join("jigsaw_piece_biome_queries.txt");
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

fn run_pieces(
    mut command: Command,
    work_dir: &Path,
    seeds_arg: Option<&&str>,
    chunks_arg: Option<&&str>,
) -> Result<(), String> {
    let seeds = seeds_arg
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(SEEDS.len())
        .clamp(1, SEEDS.len());
    let chunks = chunks_arg
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(CHUNKS.len())
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
/// `piecesexact` 模式：在 `piecesbiome` 的群系过滤之上再加一道**放置门**。
///
/// `piecesbiome` 只保证「这块的列群系属于该结构」，但 `random_spread` 的频率判定
///（`isStructureChunk`）仍然会否掉大部分区块，所以它的 `no_stub` 还有 53/131。
/// 这里先算「包含该结构的每个 `structure_set`」，再要求至少一个 set
/// `is_structure_chunk` 命中，最后才用群系过滤。三者同时成立时
/// `findGenerationPoint` 几乎必然给出起点。
///
/// `piecesexact` mode: adds the **placement gate** on top of `piecesbiome`'s biome
/// filter. `piecesbiome` only guarantees the column biome matches, but `random_spread`'s
/// frequency test (`isStructureChunk`) still rejects most chunks, which is why it keeps
/// 53/131 `no_stub` results. Here every `structure_set` containing the structure is
/// collected first, at least one set must pass `is_structure_chunk`, and only then does
/// the biome filter run. With all three true `findGenerationPoint` almost always yields
/// a stub.
fn run_pieces_exact(mut command: Command, work_dir: &Path) -> Result<(), String> {
    /// 每个方向扫多少个区块（`2*RADIUS+1` 见方）。
    /// How many chunks to scan per axis (`2*RADIUS+1` square).
    const RADIUS: i32 = 96;
    /// 每个 (结构, 种子) 最多保留几个命中区块。
    /// At most this many matching chunks per (structure, seed).
    const PER_STRUCTURE: usize = 4;

    let worldgen = Worldgen::load()?;
    let settings = worldgen
        .settings("minecraft:overworld")
        .ok_or("missing overworld settings")?;
    let dims = (settings.min_y, settings.height);
    let raw: Value =
        serde_json::from_str(WORLDGEN).map_err(|error| format!("bad worldgen.json: {error}"))?;
    let structures = Structures::parse(raw.get("structure").ok_or("missing structure")?)?;
    let sets = StructureSets::parse(raw.get("structure_set").ok_or("missing structure_set")?)?;
    let state = StructureState::new(sets);
    let mut allowed_cache: HashMap<String, HashSet<String>> = HashMap::new();
    for id in &STRUCTURES {
        let def = structures
            .get(id)
            .ok_or_else(|| format!("missing structure def {id}"))?;
        allowed_cache.insert((*id).to_owned(), allowed_biome_names(def)?);
    }
    // 结构 → 包含它的所有 set 名。
    // Structure -> every set containing it.
    let mut sets_by_structure: HashMap<String, Vec<String>> = HashMap::new();
    for entry in state.sets().iter() {
        for selection in &entry.structures {
            sets_by_structure
                .entry(selection.structure.clone())
                .or_default()
                .push(entry.name.clone());
        }
    }

    let mut queries = Vec::new();
    let mut scanned = 0usize;
    let mut placement_hits = 0usize;
    let preset = worldgen
        .preset("minecraft:overworld")
        .ok_or("missing overworld preset")?;
    for &seed in SEEDS.iter() {
        let generator = Generator::new(&worldgen, seed)?;
        let mut router = SeededRouter::new(settings, seed);
        let mut resolver = BiomeResolver {
            router: &mut router,
            source: preset,
        };
        for id in &STRUCTURES {
            let allowed = &allowed_cache[*id];
            let names = sets_by_structure.get(*id).cloned().unwrap_or_default();
            let mut found = 0usize;
            'scan: for dz in -RADIUS..=RADIUS {
                for dx in -RADIUS..=RADIUS {
                    let chunk = (dx, dz);
                    scanned += 1;
                    let placed = names
                        .iter()
                        .any(|set| state.is_structure_chunk(set, seed, chunk.0, chunk.1));
                    if !placed {
                        continue;
                    }
                    placement_hits += 1;
                    // `find_valid_generation_point` 同时判「列群系」与「起点单点群系」，
                    // 后者正是 `isValidBiome` 用的那一个。只用 `column_has_valid_biome`
                    // 会漏掉起点群系不匹配的区块（实测有 9 个）。
                    // `find_valid_generation_point` checks both the column biome and the
                    // single-point start biome, and the latter is exactly what
                    // `isValidBiome` uses. `column_has_valid_biome` alone misses chunks
                    // whose start point sits in another biome (9 of them in practice).
                    let point = find_valid_generation_point(
                        &generator,
                        &mut resolver,
                        chunk,
                        allowed,
                        dims,
                    )?;
                    if !point.valid {
                        continue;
                    }
                    queries.push(Query {
                        id: (*id).to_owned(),
                        seed,
                        cx: chunk.0,
                        cz: chunk.1,
                    });
                    found += 1;
                    if found == PER_STRUCTURE {
                        break 'scan;
                    }
                }
            }
            if found == 0 {
                println!("note: {id} seed {seed} has no placed+biome chunk within radius {RADIUS}");
            }
        }
    }
    println!(
        "scanned {scanned} (structure, chunk) cells, {placement_hits} passed the placement gate, \
         kept {} queries",
        queries.len()
    );

    let queries_path = work_dir.join("jigsaw_piece_exact_queries.txt");
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
    // 四种结果分开计数，免得「大部分查询只是返回 None」被一个总数掩盖：
    // `stub_and_valid` = 有起点且群系合法（真正跑完了 `addPieces`）；
    // `stub_invalid_biome` = 有起点但群系不合法（只验证了 `isValidBiome`）；
    // `no_stub` = `findGenerationPoint` 返回空（没走到 `addPieces`）。
    // Four outcomes are counted separately so a bare total cannot hide "most queries just
    // return None": `stub_and_valid` really ran `addPieces`; `stub_invalid_biome` only
    // exercised `isValidBiome`; `no_stub` never reached `addPieces`.
    let mut stub_and_valid = 0usize;
    let mut stub_invalid_biome = 0usize;
    let mut no_stub = 0usize;
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
            match (&result, valid) {
                (Some(_), true) => stub_and_valid += 1,
                (Some(_), false) => stub_invalid_biome += 1,
                // `None` 与 Java 的 `status=0` 已经逐查询比过，所以这里的计数只是
                // 报告「有多少查询最终没有结构」，不代表覆盖缺口。
                // The `None` result is already compared against Java's `status=0` per
                // query, so this counter only reports how many queries end with no
                // structure; it is not a coverage gap.
                (None, _) => no_stub += 1,
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
    println!(
        "outcomes: stub_and_valid={stub_and_valid} stub_invalid_biome={stub_invalid_biome} \
         no_stub={no_stub}"
    );
    let summary = type_counts
        .iter()
        .map(|(name, count)| format!("{name}={count}"))
        .collect::<Vec<_>>()
        .join(" ");
    println!("piece element types: {summary}");
    // `empty_pool_element` 的 `getPieces` 恒返回空列表，**永远不会产出结构件**，
    // 所以它不该进「必须有件」的检查。数据里它出现在 31 个 pool 里（例如
    // `village/plains/houses` 的 weight=10），但它对结构件列表的贡献本来就是零。
    // `empty_pool_element.getPieces` always returns an empty list, so it never yields a
    // piece and must not be required here. The data does contain it in 31 pools (for
    // example `village/plains/houses` at weight 10), but its contribution to the piece
    // list is zero by definition.
    let available = crate::jigsaw::pool_element_kinds_in_data()?;
    let missing: Vec<&str> = ["single", "legacy", "list", "feature"]
        .into_iter()
        .filter(|name| available.contains(*name) && !type_counts.contains_key(*name))
        .collect();
    if !missing.is_empty() {
        println!(
            "note: pool element types never compared: {}",
            missing.join(", ")
        );
    }
    let absent: Vec<&str> = ["single", "legacy", "list", "feature"]
        .into_iter()
        .filter(|name| !available.contains(*name))
        .collect();
    if !absent.is_empty() {
        println!(
            "note: pool element types absent from the data: {}",
            absent.join(", ")
        );
    }
    if available.contains("empty") {
        println!(
            "note: `empty` elements never appear above because they yield no pieces, but \
             their `break` in the target loop is covered by the Q/P comparison (turning it \
             into `continue` reports 109/980 pools differing)"
        );
    }
    Ok(())
}

/// 26.3 数据里实际出现过的 pool element 类型（`empty` / `single` / `legacy` /
/// `list` / `feature` 五种的子集）。
///
/// The pool element kinds that actually occur in the 26.3 data (a subset of `empty` /
/// `single` / `legacy` / `list` / `feature`).
pub fn pool_element_kinds_in_data() -> Result<std::collections::BTreeSet<&'static str>, String> {
    let raw: Value =
        serde_json::from_str(WORLDGEN).map_err(|error| format!("bad worldgen.json: {error}"))?;
    let pools = raw
        .get("template_pool")
        .and_then(Value::as_object)
        .ok_or("missing template_pool")?;
    let mut kinds = std::collections::BTreeSet::new();
    for pool in pools.values() {
        let Some(elements) = pool.get("elements").and_then(Value::as_array) else {
            continue;
        };
        for element in elements {
            // `element_type` 嵌在 `element` 对象里：`{element: {element_type, ...}, weight}`。
            // `element_type` sits inside `element`: `{element: {element_type, ...}, weight}`.
            let Some(kind) = element
                .get("element")
                .and_then(|inner| inner.get("element_type"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let mapped = if kind.ends_with("empty_pool_element") {
                "empty"
            } else if kind.ends_with("legacy_single_pool_element") {
                "legacy"
            } else if kind.ends_with("single_pool_element") {
                "single"
            } else if kind.ends_with("list_pool_element") {
                "list"
            } else if kind.ends_with("feature_pool_element") {
                "feature"
            } else {
                continue;
            };
            kinds.insert(mapped);
        }
    }
    Ok(kinds)
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

#[cfg(test)]
mod snbt_tests {
    use super::parse_snbt;
    use mc_bamboo_nbt::Tag;

    /// 标量与后缀：无后缀整数是 int，带点按 double，后缀区分类型。
    ///
    /// Scalars and suffixes: bare integers are ints, a decimal point means double, and
    /// suffixes select the type.
    #[test]
    fn scalars_and_suffixes() {
        let Tag::Compound(entries) =
            parse_snbt("{A:1,B:2b,C:3s,D:4L,E:5.5f,F:6.5d,G:true,H:false,I:-7}").unwrap()
        else {
            panic!("expected a compound");
        };
        let get = |key: &str| {
            entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, tag)| tag.clone())
                .unwrap_or_else(|| panic!("missing {key}"))
        };
        assert_eq!(get("A"), Tag::Int(1));
        assert_eq!(get("B"), Tag::Byte(2));
        assert_eq!(get("C"), Tag::Short(3));
        assert_eq!(get("D"), Tag::Long(4));
        assert_eq!(get("E"), Tag::Float(5.5));
        assert_eq!(get("F"), Tag::Double(6.5));
        assert_eq!(get("G"), Tag::Byte(1));
        assert_eq!(get("H"), Tag::Byte(0));
        assert_eq!(get("I"), Tag::Int(-7));
    }

    /// 列表：同型可解析，混型必须报错（官方 SNBT 也不允许）。
    ///
    /// Lists: homogeneous ones parse, mixed ones must fail (vanilla SNBT rejects them too).
    #[test]
    fn lists_require_homogeneous_elements() {
        let Tag::Compound(entries) = parse_snbt("{L:[1,2,3],Nested:{L:[4b,5b]}}").unwrap() else {
            panic!("expected a compound");
        };
        let some = |key: &str| {
            entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, tag)| tag.clone())
                .unwrap()
        };
        assert_eq!(
            some("L"),
            Tag::List(vec![Tag::Int(1), Tag::Int(2), Tag::Int(3)])
        );
        let Tag::Compound(nested) = some("Nested") else {
            panic!("expected a nested compound");
        };
        assert_eq!(nested[0].1, Tag::List(vec![Tag::Byte(4), Tag::Byte(5)]));
        assert!(
            parse_snbt("{Bad:[1,2b]}").is_err(),
            "a mixed list must be rejected"
        );
        assert!(parse_snbt("{Empty:[]}").is_ok(), "an empty list is fine");
    }

    /// 字符串转义与键的两种写法。
    ///
    /// String escapes and both key spellings.
    #[test]
    fn strings_and_quoted_keys() {
        let Tag::Compound(entries) = parse_snbt(r#"{"Quoted Key":"a\"b\\c","Plain":1}"#).unwrap()
        else {
            panic!("expected a compound");
        };
        assert_eq!(entries[0].0, "Quoted Key");
        assert_eq!(entries[0].1, Tag::String("a\"b\\c".to_owned()));
        assert_eq!(entries[1].0, "Plain");
    }

    /// 结构错误必须报错，不能静默吞掉。
    ///
    /// Structural errors must be reported, never swallowed.
    #[test]
    fn malformed_input_is_rejected() {
        for text in [
            "{A:1",
            "[1,2",
            "{A}",
            "{:1}",
            "{A:1}}",
            "{A:\"unterminated}",
        ] {
            assert!(parse_snbt(text).is_err(), "{text} should not parse");
        }
    }
}
