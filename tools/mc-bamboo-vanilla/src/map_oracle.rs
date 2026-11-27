//! `mc-bamboo-vanilla map-oracle <java 路径> [--break color|save|packet|fluid]`：
//! 用官方 26.3 的 `NoiseBasedChunkGenerator`、`MapItem`、`MapItemSavedData`、
//! `MapIndex` 与 `ClientboundMapItemDataPacket.STREAM_CODEC` 对拍 Bamboo 地图实现：
//! 1. 固定种子 8×9 区块的 scale=0 纯地形 128×128 颜色、scale=1 2×2 众数颜色，
//!    以及含水半砖 / 玻璃（`MapColor.NONE`）/ 虚空基岩 / 水深专项颜色；
//! 2. `data/minecraft/maps/0.dat` 与 `data/minecraft/maps/last_id.dat` 双向存档往返；
//! 3. 玩家手持地图沿 20 刻轨迹移动（含越界、远距消失、回图）与锁定地图 6 刻发出的
//!    `ClientboundMapItemDataPacket` 逐包逐字节比较；
//! 4. `--break color|save|packet|fluid` 4 项对拍层改坏确认。
//!
//! Vanilla 26.3 map oracle comparing fixed-seed 128x128 terrain colors (scale 0,
//! scale 1, and special blocks), bidirectional map saved-data round trips, and
//! exact tick-by-tick `ClientboundMapItemDataPacket` wire bytes while walking.

use std::{collections::BTreeMap, fs};

use mc_bamboo_blocks::{
    BlockState,
    state::{SLAB_TYPE, SlabType, WATERLOGGED},
};
use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::{RecipeType, blocks, items, registries::data_component_type as kinds, version};
use mc_bamboo_game::{
    PlayerId,
    map::{
        COLORS_LEN, MapBanner, MapFrame, MapIndex, MapItemSavedData, MapStore, banner_from_world,
        make_filled_map, on_crafted_post_process, stack_map_id, update_colors,
        update_colors_with_banner_lookup,
    },
    world_options::{LevelType, WorldGenConfig},
    worldgen::{build_region_column, generate_chunk},
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry, cursor::Cursor},
    item::ItemStack,
    packet::ClientboundPacket,
    text::Component,
};
use mc_bamboo_recipes::{CraftingContext, CraftingInput, RecipeManager, assemble};
use mc_bamboo_storage::world_dir::{WorldDir, read_saved_data_at, write_saved_data_at};
use mc_bamboo_world::store::ChunkStore;
use serde::Deserialize;

const SOURCE: &str = "tools/mc-bamboo-vanilla/java/MapOracle.java";
const USAGE: &str = "usage: map-oracle <java> [--break color|save|packet|fluid|banner-toggle|banner-check|map-extend]";
const SEED: i64 = 42;
const MIN_CX: i32 = -4;
const MAX_CX: i32 = 3;
const MIN_CZ: i32 = -5;
const MAX_CZ: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BreakMode {
    None,
    Color,
    Save,
    Packet,
    Fluid,
    BannerToggle,
    BannerCheck,
    MapExtend,
}

#[derive(Debug, Deserialize)]
struct OracleReport {
    data_version: i32,
    scale0_colors: String,
    special0_colors: String,
    scale1_colors: String,
    walk_packets: Vec<Option<String>>,
    scale1_walk_packets: Vec<Option<String>>,
    locked_walk_packets: Vec<Option<String>>,
    banner_toggle: BannerToggleReport,
    banner_break: BannerBreakReport,
    map_extending: Vec<MapExtendingCaseReport>,
    saved: SavedSummary,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct BannerJson {
    x: i32,
    y: i32,
    z: i32,
    color: String,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BannerToggleReport {
    add_success: bool,
    remove_success: bool,
    packets: Vec<Option<String>>,
    vanilla_after_add: Vec<BannerJson>,
    vanilla_after_remove: Vec<BannerJson>,
    bamboo_saved_after_add: Vec<BannerJson>,
    bamboo_saved_after_remove: Vec<BannerJson>,
}

#[derive(Debug, Deserialize)]
struct BannerBreakReport {
    banner_counts: Vec<usize>,
    packets: Vec<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct MapExtendingCaseReport {
    matched: bool,
    preview_hex: Option<String>,
    crafted_hex: Option<String>,
    new_map_id: Option<i32>,
    new_scale: Option<i8>,
    new_center_x: Option<i32>,
    new_center_z: Option<i32>,
    new_locked: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SavedSummary {
    dimension: String,
    center_x: i32,
    center_z: i32,
    scale: i8,
    locked: bool,
    colors: String,
    last_map_id_next: i32,
}

pub fn run(java: &str, args: &[&str]) -> Result<(), String> {
    let break_mode = parse_args(args)?;

    // 1. Bamboo 侧用真实游戏世界生成函数生成同一片固定种子 8×9 区块（CARVERS）。
    let config = WorldGenConfig {
        seed: SEED,
        level_type: LevelType::Normal,
        ..WorldGenConfig::default()
    };
    let mut pure_chunks = ChunkStore::loaded();
    let mut special_chunks = ChunkStore::loaded();
    let mut banner_toggle_chunks = ChunkStore::loaded();
    let mut banner_break_chunks = ChunkStore::loaded();
    for cx in MIN_CX..=MAX_CX {
        for cz in MIN_CZ..=MAX_CZ {
            let generated = generate_chunk(&config, cx, cz)
                .map_err(|e| format!("Bamboo generate_chunk ({cx},{cz}) failed: {e}"))?;
            let col = build_region_column(&generated)
                .map_err(|e| format!("build_region_column ({cx},{cz}) failed: {e}"))?;
            let chunk_col = col.chunk_column().clone();
            pure_chunks.load(ChunkPos::new(cx, cz), Box::new(chunk_col.clone()));
            special_chunks.load(ChunkPos::new(cx, cz), Box::new(chunk_col.clone()));
            banner_toggle_chunks.load(ChunkPos::new(cx, cz), Box::new(chunk_col.clone()));
            banner_break_chunks.load(ChunkPos::new(cx, cz), Box::new(chunk_col));
        }
    }

    // 1a. scale=0 纯生成地形 128×128 颜色。
    let player = PlayerId(1);
    let mut scale0 =
        MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, "minecraft:overworld");
    for _ in 0..16 {
        update_colors(
            &mut scale0,
            player,
            0.5,
            0.5,
            "minecraft:overworld",
            &pure_chunks,
        );
    }
    if break_mode == BreakMode::Color {
        // 改坏确认 1：故意翻转 (64, 64) 像素的亮度低两位。
        scale0.colors[64 + 64 * 128] ^= 0x03;
    }

    // 1b. 专项方块 + scale=1 颜色。
    place_special_columns(&mut special_chunks, break_mode == BreakMode::Fluid);
    let mut special0 =
        MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, "minecraft:overworld");
    for _ in 0..16 {
        update_colors(
            &mut special0,
            player,
            0.5,
            0.5,
            "minecraft:overworld",
            &special_chunks,
        );
    }
    let mut scale1 =
        MapItemSavedData::create_fresh(0.0, 0.0, 1, true, false, "minecraft:overworld");
    for _ in 0..16 {
        update_colors(
            &mut scale1,
            player,
            0.5,
            0.5,
            "minecraft:overworld",
            &special_chunks,
        );
    }

    // 2. 构造带真实采样颜色 + 旗帜 + 展示框的存档文件，写入 data/minecraft/maps/{0,last_id}.dat。
    let dir = crate::workspace_root().join("target/vanilla/map-oracle-check");
    let _ = fs::remove_dir_all(&dir);
    let world_dir = WorldDir::new(&dir);
    world_dir
        .create_layout()
        .map_err(|e| format!("cannot create map-oracle dir: {e}"))?;
    let in_map_path = world_dir.saved_data("minecraft:maps/0");
    let in_idx_path = world_dir.saved_data("minecraft:maps/last_id");
    let out_map_path = dir.join("official_map0.dat");
    let out_idx_path = dir.join("official_last_id.dat");

    let mut save_sample = scale0.clone();
    let banner = MapBanner {
        pos: BlockPos::new(10, 70, -12),
        color: 14, // red
        name: Some(Component::text("Base")),
    };
    let mut map_tag = {
        save_sample.set_frame_marker(BlockPos::new(4, 68, 8), 90, 42, None);
        // 通过解码-再编码或公开字段挂旗帜：在 tag 的 banners 列表里加入 banner 并重解。
        let mut tag = save_sample.to_tag();
        if let Tag::Compound(ref mut entries) = tag {
            for (k, v) in entries.iter_mut() {
                if k == "banners" {
                    *v = Tag::List(vec![banner.to_tag()]);
                }
            }
        }
        save_sample = MapItemSavedData::from_tag(&tag).ok_or("self-decode save_sample failed")?;
        save_sample.to_tag()
    };
    if break_mode == BreakMode::Save {
        // 改坏确认 2：把必填字段 xCenter 改成错名，官方 MapItemSavedData.CODEC 必须拒绝。
        map_tag = corrupt_key(&map_tag, "xCenter", "xCenter_BROKEN");
    }
    let idx_sample = MapIndex::new(7);
    write_saved_data_at(&in_map_path, &map_tag)
        .map_err(|e| format!("write maps/0.dat failed: {e}"))?;
    write_saved_data_at(&in_idx_path, &idx_sample.to_tag())
        .map_err(|e| format!("write maps/last_id.dat failed: {e}"))?;

    // 3. Bamboo 侧跑 20 刻行走发包（scale=0）、10 刻增量探索发包（scale=1）与 6 刻锁定地图发包。
    let bamboo_walk = run_walk_scenario(&pure_chunks, 0, false, break_mode == BreakMode::Packet);
    let bamboo_scale1_walk = run_walk_scenario(&pure_chunks, 1, false, false);
    let bamboo_locked_walk = run_walk_scenario(&pure_chunks, 0, true, false);

    // 3b. 第二期场景：右键带名旗帜添加 / 取消（包 + 存档）、打掉旗帜后 checkBanners 消失时机与包、工作台 map_extending。
    //     先铺好与官方 `protos` 相同的专项列（Java 侧在 scale1 后复用同一批 ProtoChunk）。
    place_special_columns(&mut banner_toggle_chunks, false);
    place_special_columns(&mut banner_break_chunks, false);
    let in_banner_added_path = dir.join("bamboo_banner_added.dat");
    let in_banner_removed_path = dir.join("bamboo_banner_removed.dat");
    let bamboo_banner_toggle = run_banner_toggle_scenario(
        &mut banner_toggle_chunks,
        &in_banner_added_path,
        &in_banner_removed_path,
        break_mode == BreakMode::BannerToggle,
    )?;
    let bamboo_banner_break =
        run_banner_break_scenario(&mut banner_break_chunks, break_mode == BreakMode::BannerCheck);
    let bamboo_map_extending = run_map_extending_scenario(break_mode == BreakMode::MapExtend);

    // 4. 启动官方 MapOracle.java 对拍。
    let mut command = crate::official_program(java, SOURCE)?;
    command
        .arg(SEED.to_string())
        .arg(&in_map_path)
        .arg(&in_idx_path)
        .arg(&out_map_path)
        .arg(&out_idx_path)
        .arg(&in_banner_added_path)
        .arg(&in_banner_removed_path);
    let output = command
        .output()
        .map_err(|e| format!("cannot run java MapOracle: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "MapOracle.java failed ({}):\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text =
        String::from_utf8(output.stdout).map_err(|e| format!("MapOracle stdout not UTF-8: {e}"))?;
    let report: OracleReport =
        serde_json::from_str(&text).map_err(|e| format!("bad MapOracle JSON: {e}\n{text}"))?;

    // 核对版本。
    if report.data_version != version::DATA_VERSION {
        return Err(format!(
            "DataVersion differs: vanilla {}, Bamboo {}",
            report.data_version,
            version::DATA_VERSION
        ));
    }

    // 核对 1a：scale=0 纯生成地形 128×128 颜色。
    let official_scale0 = from_hex(&report.scale0_colors)?;
    compare_colors("scale=0 terrain", &official_scale0, &scale0.colors)?;

    // 核对 1b：专项方块 scale=0 颜色与 scale=1 颜色。
    let official_special0 = from_hex(&report.special0_colors)?;
    compare_colors(
        "scale=0 special blocks",
        &official_special0,
        &special0.colors,
    )?;
    let official_scale1 = from_hex(&report.scale1_colors)?;
    compare_colors("scale=1 terrain+special", &official_scale1, &scale1.colors)?;

    // 核对 2：存档双向往返（Bamboo → 官方解码核对 + 官方编码 → Bamboo 解码核对）。
    if report.saved.dimension != save_sample.dimension
        || report.saved.center_x != save_sample.center_x
        || report.saved.center_z != save_sample.center_z
        || report.saved.scale != save_sample.scale
        || report.saved.locked != save_sample.locked
        || report.saved.last_map_id_next != 8
    {
        return Err(format!(
            "official decoded saved map fields differ: {:?}",
            report.saved
        ));
    }
    let official_saved_colors = from_hex(&report.saved.colors)?;
    compare_colors(
        "saved map colors",
        &official_saved_colors,
        &save_sample.colors,
    )?;

    let restored_map_tag =
        read_saved_data_at(&out_map_path).ok_or("official_map0.dat missing or unreadable")?;
    let restored_map = MapItemSavedData::from_tag(&restored_map_tag)
        .ok_or("Bamboo failed to decode official re-encoded map 0.dat")?;
    if restored_map.dimension != save_sample.dimension
        || restored_map.center_x != save_sample.center_x
        || restored_map.center_z != save_sample.center_z
        || restored_map.scale != save_sample.scale
        || restored_map.tracking_position != save_sample.tracking_position
        || restored_map.unlimited_tracking != save_sample.unlimited_tracking
        || restored_map.locked != save_sample.locked
        || restored_map.colors != save_sample.colors
        || restored_map.frame_markers().len() != 1
        || restored_map.decorations().count() != 2
    {
        return Err("official -> Bamboo map 0.dat round-trip fields differ".to_owned());
    }
    let expected_frame = MapFrame {
        pos: BlockPos::new(4, 68, 8),
        rotation: 90,
        entity_id: 42,
    };
    if restored_map.frame_markers().get(&expected_frame.id()) != Some(&expected_frame) {
        return Err("official -> Bamboo map 0.dat frame marker differs".to_owned());
    }
    let restored_idx_tag =
        read_saved_data_at(&out_idx_path).ok_or("official_last_id.dat missing or unreadable")?;
    let restored_idx = MapIndex::from_tag(&restored_idx_tag);
    if restored_idx.last_map_id != 7 {
        return Err(format!(
            "official -> Bamboo last_id.dat differs: expected 7, got {}",
            restored_idx.last_map_id
        ));
    }

    // 核对 3：20 刻 scale=0 行走发包、10 刻 scale=1 增量探索发包、6 刻锁定行走发包逐字节一致。
    compare_packets("unlocked walk", &report.walk_packets, &bamboo_walk)?;
    compare_packets(
        "scale1 incremental walk",
        &report.scale1_walk_packets,
        &bamboo_scale1_walk,
    )?;
    compare_packets(
        "locked walk",
        &report.locked_walk_packets,
        &bamboo_locked_walk,
    )?;

    // 核对 4（第二期）：右键带名旗帜添加 / 取消标记包与存档。
    if !report.banner_toggle.add_success || !bamboo_banner_toggle.add_success {
        return Err("banner toggle add did not succeed on both sides".to_owned());
    }
    if !report.banner_toggle.remove_success || !bamboo_banner_toggle.remove_success {
        return Err("banner toggle remove did not succeed on both sides".to_owned());
    }
    if report.banner_toggle.vanilla_after_add != report.banner_toggle.bamboo_saved_after_add {
        return Err(format!(
            "banner toggle saved after add differs: vanilla={:?}, bamboo={:?}",
            report.banner_toggle.vanilla_after_add, report.banner_toggle.bamboo_saved_after_add
        ));
    }
    if report.banner_toggle.vanilla_after_remove != report.banner_toggle.bamboo_saved_after_remove {
        return Err(format!(
            "banner toggle saved after remove differs: vanilla={:?}, bamboo={:?}",
            report.banner_toggle.vanilla_after_remove,
            report.banner_toggle.bamboo_saved_after_remove
        ));
    }
    compare_packets(
        "banner toggle",
        &report.banner_toggle.packets,
        &bamboo_banner_toggle.packets,
    )?;

    // 核对 5（第二期）：打掉旗帜后 checkBanners 精确消失刻与地图包。
    if report.banner_break.banner_counts != bamboo_banner_break.banner_counts {
        return Err(format!(
            "banner break checkBanners disappearance tick differs: vanilla={:?}, bamboo={:?}",
            report.banner_break.banner_counts, bamboo_banner_break.banner_counts
        ));
    }
    compare_packets(
        "banner break",
        &report.banner_break.packets,
        &bamboo_banner_break.packets,
    )?;

    // 核对 6（第二期）：工作台 map_extending 配方匹配、预览物品与后处理物品。
    compare_map_extending(&report.map_extending, &bamboo_map_extending)?;

    let _ = fs::remove_dir_all(&dir);
    let non_zero_scale0 = scale0.colors.iter().filter(|&&b| b != 0).count();
    let water_scale0 = scale0.colors.iter().filter(|&&b| (b >> 2) == 12).count();
    let sent_walk = bamboo_walk.iter().filter(|p| p.is_some()).count();
    let sent_scale1_walk = bamboo_scale1_walk.iter().filter(|p| p.is_some()).count();
    let sent_locked = bamboo_locked_walk.iter().filter(|p| p.is_some()).count();
    let sent_banner_toggle = bamboo_banner_toggle
        .packets
        .iter()
        .filter(|p| p.is_some())
        .count();
    let sent_banner_break = bamboo_banner_break
        .packets
        .iter()
        .filter(|p| p.is_some())
        .count();
    let vanish_tick = bamboo_banner_break
        .banner_counts
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(usize::MAX);
    println!(
        "MAP-ORACLE RESULT: 0 mismatches across scale0 (16384 px, {non_zero_scale0} painted, {water_scale0} water), special0 (16384 px), scale1 (16384 px), bidirectional maps/0.dat + maps/last_id.dat round-trip, unlocked walk ({sent_walk}/20 packets byte-exact), scale1 incremental walk ({sent_scale1_walk}/10 packets byte-exact), locked walk ({sent_locked}/6 packets byte-exact), banner toggle ({sent_banner_toggle}/11 packets + saved banners exact), banner break (vanished at tick {vanish_tick}, {sent_banner_break}/20 packets byte-exact), and map_extending ({} crafting cases exact)",
        bamboo_map_extending.len()
    );
    Ok(())
}

fn parse_args(args: &[&str]) -> Result<BreakMode, String> {
    match args {
        [] => Ok(BreakMode::None),
        ["--break", "color"] => Ok(BreakMode::Color),
        ["--break", "save"] => Ok(BreakMode::Save),
        ["--break", "packet"] => Ok(BreakMode::Packet),
        ["--break", "fluid"] => Ok(BreakMode::Fluid),
        ["--break", "banner-toggle"] => Ok(BreakMode::BannerToggle),
        ["--break", "banner-check"] => Ok(BreakMode::BannerCheck),
        ["--break", "map-extend"] => Ok(BreakMode::MapExtend),
        _ => Err(USAGE.to_owned()),
    }
}

fn place_special_columns(chunks: &mut ChunkStore, break_fluid: bool) {
    // (0, 120, 0): 含水下置石半砖（break_fluid 时改成普通干石半砖模拟漏掉流体替换）
    chunks.set_block(BlockPos::new(0, 119, 0), blocks::STONE);
    let slab = if break_fluid {
        BlockState(blocks::STONE_SLAB)
            .with(SLAB_TYPE, SlabType::Bottom)
            .with(WATERLOGGED, false)
            .0
    } else {
        BlockState(blocks::STONE_SLAB)
            .with(SLAB_TYPE, SlabType::Bottom)
            .with(WATERLOGGED, true)
            .0
    };
    chunks.set_block(BlockPos::new(0, 120, 0), slab);
    for y in 121..320 {
        chunks.set_block(BlockPos::new(0, y, 0), blocks::AIR);
    }

    // (1, 120, 0): 玻璃（MapColor.NONE）盖金块
    chunks.set_block(BlockPos::new(1, 119, 0), blocks::GOLD_BLOCK);
    chunks.set_block(BlockPos::new(1, 120, 0), blocks::GLASS);
    for y in 121..320 {
        chunks.set_block(BlockPos::new(1, y, 0), blocks::AIR);
    }

    // (2, *, 0): 整列空气
    for y in -64..320 {
        chunks.set_block(BlockPos::new(2, y, 0), blocks::AIR);
    }

    // (3, 120..=122, 0): 3 格水压石头
    chunks.set_block(BlockPos::new(3, 119, 0), blocks::STONE);
    for y in 120..=122 {
        chunks.set_block(BlockPos::new(3, y, 0), blocks::WATER);
    }
    for y in 123..320 {
        chunks.set_block(BlockPos::new(3, y, 0), blocks::AIR);
    }
}

fn run_walk_scenario(
    chunks: &ChunkStore,
    scale: i8,
    locked: bool,
    break_packet: bool,
) -> Vec<Option<Vec<u8>>> {
    let mut data =
        MapItemSavedData::create_fresh(0.0, 0.0, scale, true, false, "minecraft:overworld");
    if locked {
        data = data.locked();
    }
    let player = PlayerId(1);
    let steps: &[(f64, f64, f64)] = if locked {
        &[
            (0.5, 0.5, 0.0),
            (4.5, 2.5, 45.0),
            (8.5, 4.5, 90.0),
            (12.5, 6.5, 135.0),
            (16.5, 8.5, 180.0),
            (20.5, 10.5, -90.0),
        ]
    } else if scale == 1 {
        &[
            (-60.5, -60.5, -45.0),
            (-48.5, -48.5, -35.0),
            (-36.5, -36.5, -25.0),
            (-24.5, -24.5, -15.0),
            (-12.5, -12.5, -5.0),
            (-0.5, -0.5, 5.0),
            (11.5, 11.5, 15.0),
            (23.5, 23.5, 25.0),
            (35.5, 35.5, 35.0),
            (47.5, 47.5, 45.0),
        ]
    } else {
        &[
            (-20.5, -15.5, -180.0),
            (-17.3, -13.4, -152.5),
            (-14.1, -11.3, -125.0),
            (-10.9, -9.2, -97.5),
            (-7.7, -7.1, -70.0),
            (-4.5, -5.0, -42.5),
            (-1.3, -2.9, -15.0),
            (1.9, -0.8, 12.5),
            (5.1, 1.3, 40.0),
            (8.3, 3.4, 67.5),
            (11.5, 5.5, 95.0),
            (14.7, 7.6, 122.5),
            (17.9, 9.7, 150.0),
            (21.1, 11.8, 177.5),
            (24.3, 13.9, 205.0),
            (90.5, 10.5, 45.0),
            (95.5, 12.5, 135.0),
            (420.5, 10.5, 90.0),
            (12.5, -10.5, 180.0),
            (14.0, -8.0, 0.0),
        ]
    };

    let mut out = Vec::with_capacity(steps.len());
    for (t, &(x, z, mut yaw)) in steps.iter().enumerate() {
        if break_packet && t == 5 {
            // 改坏确认 3：在第 5 刻故意偏转朝向 +30°，触发标记字节不一致。
            yaw += 30.0;
        }
        let game_time = (t as i64) + 1;
        data.tick_carried_by_player(
            player,
            "Oracle",
            x,
            z,
            yaw,
            game_time,
            "minecraft:overworld",
        );
        if !data.locked {
            update_colors(&mut data, player, x, z, "minecraft:overworld", chunks);
        }
        let encoded = data.get_update_packet(0, player).map(|update| {
            let mut body = Vec::new();
            update.as_packet().write_body(&mut body);
            body
        });
        out.push(encoded);
    }
    out
}

struct BambooBannerToggle {
    add_success: bool,
    remove_success: bool,
    packets: Vec<Option<Vec<u8>>>,
}

struct BambooBannerBreak {
    banner_counts: Vec<usize>,
    packets: Vec<Option<Vec<u8>>>,
}

struct BambooMapExtendingCase {
    matched: bool,
    preview: Option<ItemStack>,
    crafted: Option<ItemStack>,
    new_map_id: Option<i32>,
    new_scale: Option<i8>,
    new_center_x: Option<i32>,
    new_center_z: Option<i32>,
    new_locked: Option<bool>,
}

fn place_banner_column(
    chunks: &mut ChunkStore,
    block_items: &mut BTreeMap<BlockPos, ComponentPatch>,
    pos: BlockPos,
    banner_state: u32,
    custom_name: Option<&str>,
) {
    chunks.set_block(pos.below(), blocks::STONE);
    chunks.set_block(pos, banner_state);
    for y in (pos.y + 1)..320 {
        chunks.set_block(BlockPos::new(pos.x, y, pos.z), blocks::AIR);
    }
    if let Some(name) = custom_name {
        let mut patch = ComponentPatch::new();
        patch.put(
            kinds::CUSTOM_NAME,
            PatchEntry::Set(ComponentValue::CustomName(Component::text(name))),
        );
        block_items.insert(pos, patch);
    }
}

fn encode_update_packet(data: &mut MapItemSavedData, player: PlayerId) -> Option<Vec<u8>> {
    data.get_update_packet(0, player).map(|update| {
        let mut body = Vec::new();
        update.as_packet().write_body(&mut body);
        body
    })
}

fn run_banner_toggle_scenario(
    chunks: &mut ChunkStore,
    added_path: &std::path::Path,
    removed_path: &std::path::Path,
    break_banner_toggle: bool,
) -> Result<BambooBannerToggle, String> {
    let banner_pos = BlockPos::new(5, 120, -3);
    let mut block_items = BTreeMap::new();
    place_banner_column(
        chunks,
        &mut block_items,
        banner_pos,
        blocks::RED_BANNER,
        if break_banner_toggle {
            // 改坏确认 5：故意漏掉旗帜自定义名，验证对拍包与存档立即报差异。
            None
        } else {
            Some("Camp Alpha")
        },
    );

    let mut data = MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, "minecraft:overworld");
    let player = PlayerId(1);
    let mut packets = Vec::with_capacity(11);

    // Tick 0: 初始同步。
    data.tick_carried_by_player(player, "Oracle", 0.5, 0.5, 0.0, 1, "minecraft:overworld");
    update_colors_with_banner_lookup(
        &mut data,
        player,
        0.5,
        0.5,
        "minecraft:overworld",
        chunks,
        |p| banner_from_world(chunks, &block_items, p),
    );
    packets.push(encode_update_packet(&mut data, player));

    // 第一次右键旗帜：添加标记，写出存档并跑 5 刻。
    let banner1 = banner_from_world(chunks, &block_items, banner_pos);
    let add_success = data.toggle_banner(banner_pos, banner1, Some(1));
    write_saved_data_at(added_path, &data.to_tag())
        .map_err(|e| format!("write bamboo_banner_added.dat failed: {e}"))?;
    for i in 0..5 {
        let gt = 2 + i;
        data.tick_carried_by_player(player, "Oracle", 0.5, 0.5, 0.0, gt, "minecraft:overworld");
        update_colors_with_banner_lookup(
            &mut data,
            player,
            0.5,
            0.5,
            "minecraft:overworld",
            chunks,
            |p| banner_from_world(chunks, &block_items, p),
        );
        packets.push(encode_update_packet(&mut data, player));
    }

    // 第二次右键同一面旗帜：取消标记，写出存档并再跑 5 刻。
    let banner2 = banner_from_world(chunks, &block_items, banner_pos);
    let remove_success = data.toggle_banner(banner_pos, banner2, Some(6));
    write_saved_data_at(removed_path, &data.to_tag())
        .map_err(|e| format!("write bamboo_banner_removed.dat failed: {e}"))?;
    for i in 0..5 {
        let gt = 7 + i;
        data.tick_carried_by_player(player, "Oracle", 0.5, 0.5, 0.0, gt, "minecraft:overworld");
        update_colors_with_banner_lookup(
            &mut data,
            player,
            0.5,
            0.5,
            "minecraft:overworld",
            chunks,
            |p| banner_from_world(chunks, &block_items, p),
        );
        packets.push(encode_update_packet(&mut data, player));
    }

    Ok(BambooBannerToggle {
        add_success,
        remove_success,
        packets,
    })
}

fn run_banner_break_scenario(
    chunks: &mut ChunkStore,
    break_banner_check: bool,
) -> BambooBannerBreak {
    let banner_pos = BlockPos::new(5, 120, -3);
    let mut block_items = BTreeMap::new();
    place_banner_column(
        chunks,
        &mut block_items,
        banner_pos,
        blocks::BLUE_WALL_BANNER,
        Some("Watchtower"),
    );

    let mut data = MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, "minecraft:overworld");
    let player = PlayerId(1);
    let mut tick: i64 = 0;

    // 16 刻探索完 128×128。
    for _ in 0..16 {
        tick += 1;
        data.tick_carried_by_player(player, "Oracle", 0.5, 0.5, 0.0, tick, "minecraft:overworld");
        update_colors_with_banner_lookup(
            &mut data,
            player,
            0.5,
            0.5,
            "minecraft:overworld",
            chunks,
            |p| banner_from_world(chunks, &block_items, p),
        );
        let _ = data.get_update_packet(0, player);
    }

    // 右键添加蓝色墙旗帜标记，跑 5 刻送出标记包。
    let banner = banner_from_world(chunks, &block_items, banner_pos);
    let _ = data.toggle_banner(banner_pos, banner, Some(tick));
    for _ in 0..5 {
        tick += 1;
        data.tick_carried_by_player(player, "Oracle", 0.5, 0.5, 0.0, tick, "minecraft:overworld");
        update_colors_with_banner_lookup(
            &mut data,
            player,
            0.5,
            0.5,
            "minecraft:overworld",
            chunks,
            |p| banner_from_world(chunks, &block_items, p),
        );
        let _ = data.get_update_packet(0, player);
    }

    // 打掉旗帜。
    chunks.set_block(banner_pos, blocks::AIR);
    block_items.remove(&banner_pos);

    let mut banner_counts = Vec::with_capacity(20);
    let mut packets = Vec::with_capacity(20);
    for _ in 0..20 {
        tick += 1;
        data.tick_carried_by_player(player, "Oracle", 0.5, 0.5, 0.0, tick, "minecraft:overworld");
        if break_banner_check {
            // 改坏确认 6：故意用假 lookup 返回仍在原位的旗帜，模拟漏掉 checkBanners。
            update_colors_with_banner_lookup(
                &mut data,
                player,
                0.5,
                0.5,
                "minecraft:overworld",
                chunks,
                |p| {
                    (p == banner_pos).then(|| MapBanner {
                        pos: banner_pos,
                        color: 11,
                        name: Some(Component::text("Watchtower")),
                    })
                },
            );
        } else {
            update_colors_with_banner_lookup(
                &mut data,
                player,
                0.5,
                0.5,
                "minecraft:overworld",
                chunks,
                |p| banner_from_world(chunks, &block_items, p),
            );
        }
        banner_counts.push(data.banner_markers().len());
        packets.push(encode_update_packet(&mut data, player));
    }

    BambooBannerBreak {
        banner_counts,
        packets,
    }
}

fn run_map_extending_scenario(break_map_extend: bool) -> Vec<BambooMapExtendingCase> {
    let mut store = MapStore::new();
    store.insert(
        0,
        MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, "minecraft:overworld"),
    );
    store.insert(
        1,
        MapItemSavedData::create_fresh(0.0, 0.0, 3, true, false, "minecraft:overworld"),
    );
    store.insert(
        2,
        MapItemSavedData::create_fresh(0.0, 0.0, 4, true, false, "minecraft:overworld"),
    );
    store.insert(
        3,
        MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, "minecraft:overworld").locked(),
    );
    store.index.last_map_id = 3;

    let mut map0 = make_filled_map(0);
    {
        let mut patch = ComponentPatch::from_opaque(&map0).unwrap();
        patch.put(
            kinds::CUSTOM_NAME,
            PatchEntry::Set(ComponentValue::CustomName(Component::text("Expedition"))),
        );
        let (added, removed) = patch.to_opaque();
        map0.added = added;
        map0.removed = removed;
    }

    let centers = [
        map0,
        make_filled_map(1),
        make_filled_map(2),
        make_filled_map(3),
        ItemStack::new(items::FILLED_MAP, 1), // 无 MAP_ID
    ];
    let manager = RecipeManager::global();
    let paper = ItemStack::new(items::PAPER, 1);

    let mut out = Vec::with_capacity(centers.len());
    for center in centers {
        let slots = [
            paper.clone(),
            paper.clone(),
            paper.clone(),
            paper.clone(),
            center,
            paper.clone(),
            paper.clone(),
            paper.clone(),
            paper.clone(),
        ];
        let input = CraftingInput::of(3, 3, &slots);
        let recipe = {
            let ctx = CraftingContext::new(&input, |id| {
                let d = store.get(id)?;
                if break_map_extend && d.locked {
                    // 改坏确认 7：故意拒绝已锁定地图（违反 26.3 MapExtendingRecipe 字节码）。
                    return None;
                }
                Some(d.scale)
            });
            manager.get_recipe_for(RecipeType::Crafting, &ctx, None)
        };
        let Some(recipe) = recipe else {
            out.push(BambooMapExtendingCase {
                matched: false,
                preview: None,
                crafted: None,
                new_map_id: None,
                new_scale: None,
                new_center_x: None,
                new_center_z: None,
                new_locked: None,
            });
            continue;
        };
        let preview = assemble(recipe, input.slots()).unwrap();
        let mut crafted = preview.clone();
        on_crafted_post_process(&mut crafted, &mut store);
        let new_map_id = stack_map_id(&crafted);
        let new_data = new_map_id.and_then(|id| store.get(id));
        out.push(BambooMapExtendingCase {
            matched: true,
            preview: Some(preview),
            crafted: Some(crafted),
            new_map_id,
            new_scale: new_data.map(|d| d.scale),
            new_center_x: new_data.map(|d| d.center_x),
            new_center_z: new_data.map(|d| d.center_z),
            new_locked: new_data.map(|d| d.locked),
        });
    }
    out
}

fn decode_trusted_item_stack(bytes: &[u8]) -> Result<(i32, i32, ComponentPatch), String> {
    let mut cursor = Cursor::new(bytes);
    let count = cursor.varint().map_err(|e| format!("count: {e:?}"))?;
    if count <= 0 {
        cursor.finish().map_err(|e| format!("trailing: {e:?}"))?;
        return Ok((0, 0, ComponentPatch::new()));
    }
    let item = cursor.varint().map_err(|e| format!("item: {e:?}"))?;
    let patch = ComponentPatch::read(&mut cursor).map_err(|e| format!("patch: {e:?}"))?;
    cursor.finish().map_err(|e| format!("trailing: {e:?}"))?;
    Ok((item, count, patch))
}

fn compare_item_stack(label: &str, official_hex: &str, bamboo: &ItemStack) -> Result<(), String> {
    let van_bytes = from_hex(official_hex)?;
    let (van_item, van_count, van_patch) = decode_trusted_item_stack(&van_bytes)
        .map_err(|e| format!("{label}: failed to decode vanilla ItemStack: {e}"))?;
    // 确认 Bamboo 自己的 ItemStack 编码也能无损往返。
    let mut bam_bytes = Vec::new();
    bamboo.write(&mut bam_bytes);
    let (bam_item, bam_count, bam_patch) = decode_trusted_item_stack(&bam_bytes)
        .map_err(|e| format!("{label}: failed to round-trip bamboo ItemStack: {e}"))?;
    if van_item != bam_item || van_count != bam_count || van_patch != bam_patch {
        return Err(format!(
            "{label}: ItemStack differs: vanilla=(item={}, count={}, patch={:?}) vs bamboo=(item={}, count={}, patch={:?})",
            van_item, van_count, van_patch, bam_item, bam_count, bam_patch
        ));
    }
    Ok(())
}

fn compare_map_extending(
    official: &[MapExtendingCaseReport],
    bamboo: &[BambooMapExtendingCase],
) -> Result<(), String> {
    if official.len() != bamboo.len() {
        return Err(format!(
            "map_extending case count differs: vanilla {}, bamboo {}",
            official.len(),
            bamboo.len()
        ));
    }
    for (idx, (van, bam)) in official.iter().zip(bamboo.iter()).enumerate() {
        if van.matched != bam.matched {
            return Err(format!(
                "map_extending case {idx}: matched differs: vanilla={}, bamboo={}",
                van.matched, bam.matched
            ));
        }
        if !van.matched {
            continue;
        }
        compare_item_stack(
            &format!("map_extending case {idx} preview"),
            van.preview_hex
                .as_deref()
                .ok_or_else(|| format!("map_extending case {idx}: missing vanilla preview"))?,
            bam.preview
                .as_ref()
                .ok_or_else(|| format!("map_extending case {idx}: missing bamboo preview"))?,
        )?;
        compare_item_stack(
            &format!("map_extending case {idx} crafted"),
            van.crafted_hex
                .as_deref()
                .ok_or_else(|| format!("map_extending case {idx}: missing vanilla crafted"))?,
            bam.crafted
                .as_ref()
                .ok_or_else(|| format!("map_extending case {idx}: missing bamboo crafted"))?,
        )?;
        if van.new_map_id != bam.new_map_id
            || van.new_scale != bam.new_scale
            || van.new_center_x != bam.new_center_x
            || van.new_center_z != bam.new_center_z
            || van.new_locked != bam.new_locked
        {
            return Err(format!(
                "map_extending case {idx}: new map data differs: vanilla=({:?},{:?},{:?},{:?},{:?}) vs bamboo=({:?},{:?},{:?},{:?},{:?})",
                van.new_map_id,
                van.new_scale,
                van.new_center_x,
                van.new_center_z,
                van.new_locked,
                bam.new_map_id,
                bam.new_scale,
                bam.new_center_x,
                bam.new_center_z,
                bam.new_locked,
            ));
        }
    }
    Ok(())
}

fn compare_colors(label: &str, official: &[u8], bamboo: &[u8; COLORS_LEN]) -> Result<(), String> {
    if official.len() != COLORS_LEN {
        return Err(format!(
            "{label}: official colors len {}, expected {COLORS_LEN}",
            official.len()
        ));
    }
    let mut diffs = 0usize;
    let mut first_msg = String::new();
    for y in 0..128 {
        for x in 0..128 {
            let idx = x + y * 128;
            if official[idx] != bamboo[idx] {
                if diffs == 0 {
                    first_msg = format!(
                        "first diff at ({x},{y}): vanilla=0x{:02x} (color={}, bright={}) vs bamboo=0x{:02x} (color={}, bright={})",
                        official[idx],
                        official[idx] >> 2,
                        official[idx] & 3,
                        bamboo[idx],
                        bamboo[idx] >> 2,
                        bamboo[idx] & 3
                    );
                }
                diffs += 1;
            }
        }
    }
    if diffs > 0 {
        return Err(format!("{label}: {diffs} pixel mismatches; {first_msg}"));
    }
    Ok(())
}

fn compare_packets(
    label: &str,
    official: &[Option<String>],
    bamboo: &[Option<Vec<u8>>],
) -> Result<(), String> {
    if official.len() != bamboo.len() {
        return Err(format!(
            "{label}: tick count mismatch: vanilla {}, bamboo {}",
            official.len(),
            bamboo.len()
        ));
    }
    for (tick, (van, bam)) in official.iter().zip(bamboo.iter()).enumerate() {
        match (van, bam) {
            (None, None) => {}
            (Some(hex), None) => {
                return Err(format!(
                    "{label} tick {tick}: vanilla sent {} bytes, bamboo sent nothing",
                    hex.len() / 2
                ));
            }
            (None, Some(bytes)) => {
                return Err(format!(
                    "{label} tick {tick}: vanilla sent nothing, bamboo sent {} bytes",
                    bytes.len()
                ));
            }
            (Some(hex), Some(bytes)) => {
                let van_bytes = from_hex(hex)?;
                if van_bytes != *bytes {
                    let first = van_bytes
                        .iter()
                        .zip(bytes.iter())
                        .position(|(a, b)| a != b)
                        .unwrap_or_else(|| van_bytes.len().min(bytes.len()));
                    return Err(format!(
                        "{label} tick {tick}: packet bytes differ (vanilla {} B, bamboo {} B, first diff at byte {first}: vanilla={:?}, bamboo={:?})",
                        van_bytes.len(),
                        bytes.len(),
                        van_bytes.get(first),
                        bytes.get(first)
                    ));
                }
            }
        }
    }
    Ok(())
}

fn from_hex(hex: &str) -> Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) {
        return Err(format!("odd hex length {}", hex.len()));
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    let bytes = hex.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = hex_nibble(bytes[i])?;
        let lo = hex_nibble(bytes[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(format!("invalid hex char {}", b as char)),
    }
}

fn corrupt_key(tag: &Tag, from: &str, to: &str) -> Tag {
    match tag {
        Tag::Compound(entries) => Tag::Compound(
            entries
                .iter()
                .map(|(k, v)| {
                    let key = if k == from { to.to_owned() } else { k.clone() };
                    (key, v.clone())
                })
                .collect(),
        ),
        other => other.clone(),
    }
}
