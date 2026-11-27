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
    let found = find_nearest_map_structure(
        config,
        &destination,
        origin,
        request.search_radius,
        request.skip_existing_chunks,
    );
    let elapsed = started.elapsed();
    // 官方是同步搜索，这里照做；把耗时打到日志里，方便评估世界线程的阻塞时间。
    // 未命中要扫完整个半径，往往比命中更慢，所以两种结果都要记录。
    // Vanilla searches synchronously; log the elapsed time so the world-thread
    // stall can be measured. A miss scans the whole radius and is often slower,
    // so both outcomes are recorded.
    tracing::debug!(
        destination = request.destination,
        radius = request.search_radius,
        elapsed_ms = elapsed.as_secs_f64() * 1000.0,
        found = found.is_some(),
        "exploration map structure search"
    );
    let target = found?;

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
    // 地图 tick 的链路，改为在建图时就加入内存标记。地图存档只保存旗帜和展示框，
    // 因此重载地图后不会从这里恢复目标标记（见 README 已知限制）。
    // `MapItemSavedData.addTargetDecoration` writes the item-stack MAP_DECORATIONS
    // component; `tickCarriedBy` copies it into saved data only when a player
    // carries the map. Bamboo does not wire item components into the map tick, so
    // it adds an in-memory decoration at creation. The save codec persists only
    // banners and frames, so this marker is not recovered on load.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_options::{LevelType, WorldGenConfig};
    use mc_bamboo_data::registries::biome;

    fn normal_config(seed: i64) -> WorldGenConfig {
        WorldGenConfig {
            seed,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        }
    }

    /// `generate-structures=false`：官方 `findNearestMapStructure` 立即返回空，
    /// `run` 原样退回，不分配 `map_id`、不写地图数据。
    ///
    /// With `generate-structures=false`, vanilla `findNearestMapStructure` returns
    /// empty at once; `run` returns the input unchanged, allocating no `map_id`
    /// and writing no saved data.
    #[test]
    fn no_structure_returns_none_and_allocates_no_map() {
        let mut config = normal_config(42);
        config.generate_structures = false;
        let mut maps = MapStore::new();
        let request = ExplorationMapRequest {
            origin: (0, 64, 0),
            destination: "#minecraft:on_treasure_maps",
            decoration: "minecraft:red_x",
            zoom: 1,
            search_radius: 50,
            skip_existing_chunks: false,
        };
        let result = build_exploration_map(
            &config,
            &mut maps,
            "minecraft:overworld",
            63,
            |_, _, _| None,
            &request,
        );
        assert!(result.is_none());
        assert!(maps.is_empty());
    }

    /// 固定种子 42 的真实结构定位加建图：命中埋藏的宝藏，分配首个
    /// `map_id=0`，按 `shipwreck_map` 的 `zoom=1` 建图，写 `red_x` 目标标记，
    /// 群系预览上色。世界侧四个官方步骤第一次端到端走真实种子。
    ///
    /// A real structure search plus map build on seed 42: locates a buried
    /// treasure, allocates the first `map_id=0`, builds the map at the
    /// `shipwreck_map` `zoom=1`, writes the `red_x` target decoration and paints
    /// the biome preview. The four vanilla world-side steps run end to end on a
    /// real seed for the first time.
    #[test]
    fn real_seed_builds_explorer_map() {
        let config = normal_config(42);
        let mut maps = MapStore::new();
        // 条带水 / 陆群系，让预览一定出现非默认颜色。
        // Striped water / land biomes so the preview paints non-default colors.
        let biome = |x: i32, _y: i32, z: i32| {
            Some(if ((x >> 4) + z) % 2 == 0 {
                biome::OCEAN
            } else {
                biome::PLAINS
            })
        };
        let request = ExplorationMapRequest {
            origin: (0, 64, 0),
            destination: "#minecraft:on_treasure_maps",
            decoration: "minecraft:red_x",
            zoom: 1,
            search_radius: 50,
            skip_existing_chunks: false,
        };
        let result = build_exploration_map(
            &config,
            &mut maps,
            "minecraft:overworld",
            63,
            biome,
            &request,
        );
        let Some(result) = result else {
            panic!("seed 42 must locate a buried treasure near the origin");
        };
        assert_eq!(result.map_id, 0);
        assert_eq!(maps.index.last_map_id, 0);
        let data = maps.get(0).expect("map 0 created");
        assert_eq!(data.scale, 1);
        assert_eq!(data.dimension, "minecraft:overworld");
        let red_x = decoration_id_from_name("minecraft:red_x").expect("red_x is registered");
        assert!(
            data.decorations
                .iter()
                .any(|(key, decoration)| key == "+" && decoration.kind == red_x)
        );
        assert!(
            data.colors.iter().any(|&color| color != 0),
            "the biome preview must paint pixels"
        );
    }

    /// 官方 `ChunkGenerator.findNearestMapStructure` 是同步搜索，这里量一下它阻塞
    /// 世界线程多久；用 `--ignored --nocapture` 单独跑，把数字写进汇报。
    ///
    /// Vanilla `ChunkGenerator.findNearestMapStructure` is synchronous; measure
    /// how long it stalls the world thread. Run with `--ignored --nocapture` and
    /// record the numbers in the report.
    #[test]
    #[ignore = "timing probe; run with --ignored --nocapture"]
    fn measure_map_structure_search_time() {
        use crate::worldgen_structures::find_nearest_map_structure;
        let config = normal_config(42);
        let treasure =
            mc_bamboo_data::expand_structure_map_destination("#minecraft:on_treasure_maps")
                .expect("tag exported");
        let mansion =
            mc_bamboo_data::expand_structure_map_destination("#minecraft:on_woodland_mansion_maps")
                .expect("tag exported");
        // 预热共享的 StructureThread，把首次加载注册表的时间排除掉。
        // Warm up the shared StructureThread to exclude one-time registry loading.
        let _ = find_nearest_map_structure(
            &config,
            &treasure,
            mc_bamboo_core::BlockPos::new(0, 64, 0),
            50,
            false,
        );
        for origin in [0, 512, -1024, 4096] {
            let started = Instant::now();
            let found = find_nearest_map_structure(
                &config,
                &treasure,
                mc_bamboo_core::BlockPos::new(origin, 64, origin),
                50,
                false,
            );
            eprintln!(
                "treasure origin {origin}: found={} elapsed={:?}",
                found.is_some(),
                started.elapsed()
            );
        }
        // 未命中要扫完整个半径，往往比命中慢，单独量一次。
        // A miss scans the whole radius and is often slower; measure it once.
        let started = Instant::now();
        let found = find_nearest_map_structure(
            &config,
            &mansion,
            mc_bamboo_core::BlockPos::new(0, 64, 0),
            50,
            false,
        );
        eprintln!(
            "mansion: found={} elapsed={:?}",
            found.is_some(),
            started.elapsed()
        );
    }
}
