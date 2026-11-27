//! `ExplorationMapFunction` 的世界侧：`ServerLevel.findNearestMapStructure`、
//! `MapItem.applyNewSavedData`、`MapItem.renderBiomePreviewMap` 和
//! `MapItemSavedData.addTargetDecoration` 在 Bamboo 里的落地。
//!
//! `mc-bamboo-loot` 是纯逻辑，靠 `LevelView::exploration_map` 把这几步交给世界。
//! 这里提供 [`build_exploration_map`]：调用方（世界线程 / 对拍夹具）带着可变的
//! [`MapStore`]、世界生成配置和群系查询进来，拿到要写到物品堆上的 `map_id` 与
//! 结构坐标，世界状态由它自己改。
//!
//! World side of `ExplorationMapFunction`: `ServerLevel.findNearestMapStructure`,
//! `MapItem.applyNewSavedData`, `MapItem.renderBiomePreviewMap` and
//! `MapItemSavedData.addTargetDecoration`. `mc-bamboo-loot` is pure and delegates
//! these steps through `LevelView::exploration_map`; [`build_exploration_map`]
//! takes the mutable [`MapStore`], worldgen config and a biome lookup and returns
//! the component values while applying all world-side state changes itself.

use std::time::Instant;

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{biome_is_water_on_map_outlines, expand_structure_map_destination};
use mc_bamboo_loot::{ExplorationMapRequest, ExplorationMapResult};

use crate::{
    map::{MapItemSavedData, MapStore, decoration_id_from_name, render_biome_preview_map},
    world_options::WorldGenConfig,
    worldgen_structures::find_nearest_map_structure,
};

/// 官方 `ExplorationMapFunction.run` 的全部世界副作用。
///
/// `biome(x, y, z) -> Option<biome registry id>` 对应 `ServerLevel.getBiome`；
/// `dimension` 是当前维度名，`sea_level` 是 `ServerLevel.getSeaLevel()`。
/// 返回 `None` 表示找不到结构，官方此时原样退回输入物品堆。
///
/// All world-side effects of `ExplorationMapFunction.run`. `biome(x, y, z)` is
/// `ServerLevel.getBiome`; `dimension` and `sea_level` are the current dimension
/// and `ServerLevel.getSeaLevel()`. `None` means no structure was found, in
/// which case vanilla returns the input stack unchanged.
pub fn build_exploration_map(
    config: &WorldGenConfig,
    maps: &mut MapStore,
    dimension: &str,
    sea_level: i32,
    biome: impl Fn(i32, i32, i32) -> Option<i32>,
    request: &ExplorationMapRequest<'_>,
) -> Option<ExplorationMapResult> {
    let destination = expand_structure_map_destination(request.destination)?;
    let origin = BlockPos::new(request.origin.0, request.origin.1, request.origin.2);

    // 官方 `run` 在 skipKnownStructures 时先 `claimStructureAt`：给落在箱子上的结构
    // 起点加引用。Bamboo 没有结构引用表，这一步不建模（见 README 已知限制）。
    // With skipKnownStructures vanilla first calls `claimStructureAt` to add a
    // reference to a start at the loot position. Bamboo has no reference table.
    let started = Instant::now();
    let target = find_nearest_map_structure(
        config,
        &destination,
        origin,
        request.search_radius,
        request.skip_existing_chunks,
    )?;
    let elapsed = started.elapsed();
    // 官方是同步搜索，这里照做；把耗时打到日志里，方便评估世界线程的阻塞时间。
    // Vanilla searches synchronously; log the elapsed time so the world-thread
    // stall can be measured.
    tracing::debug!(
        destination = request.destination,
        radius = request.search_radius,
        elapsed_ms = elapsed.as_secs_f64() * 1000.0,
        found = true,
        "exploration map structure search"
    );

    // `MapItem.applyNewSavedData(..., zoom, true, true)` → `MapItemSavedData.createFresh`。
    // `MapItem.applyNewSavedData(..., zoom, true, true)` → `MapItemSavedData.createFresh`.
    let mut data = MapItemSavedData::create_fresh(
        f64::from(target.x),
        f64::from(target.z),
        request.zoom,
        true,
        true,
        dimension,
    );
    // `renderBiomePreviewMap` 只在维度一致时上色；createFresh 用的就是当前维度。
    // `renderBiomePreviewMap` only paints when dimensions match; createFresh uses
    // the current dimension, so the check always passes here.
    render_biome_preview_map(&mut data, sea_level, |x, y, z| {
        biome(x, y, z).is_some_and(biome_is_water_on_map_outlines)
    });
    // `MapItemSavedData.addTargetDecoration` 写的是物品堆的 `MAP_DECORATIONS` 组件，
    // 玩家持图时 `tickCarriedBy` 才把它补进 saved data。Bamboo 没有把物品组件接进
    // 地图 tick 的链路，改为在建图时就写进 saved data：玩家持图发出的
    // `ClientboundMapItemDataPacket` 字节一致，但没被持有过的地图存档会提前带上这个
    // 标记（见 README 已知限制）。
    // `MapItemSavedData.addTargetDecoration` writes the item-stack MAP_DECORATIONS
    // component; `tickCarriedBy` copies it into saved data only when a player
    // carries the map. Bamboo does not wire item components into the map tick, so
    // it writes the marker into saved data at creation: held-map packet bytes
    // match, but an un-held map's save carries the marker earlier than vanilla.
    if let Some(kind) = decoration_id_from_name(request.decoration) {
        data.add_decoration(
            kind,
            None,
            "+".to_owned(),
            f64::from(target.x),
            f64::from(target.z),
            180.0,
            None,
        );
    }

    // `MapItem.createNewSavedData`：`getFreeMapId()` + `setMapData(id, newData)`。
    // `MapItem.createNewSavedData`: `getFreeMapId()` + `setMapData(id, newData)`.
    let map_id = maps.next_map_id();
    maps.insert(map_id, data);
    Some(ExplorationMapResult {
        map_id,
        target_x: target.x,
        target_z: target.z,
    })
}
