# 植被类地物：分类与移植进度（分支 `cb-wg-feat-b`）

本文按官方 `worldgen.json` 的 `features_per_step`（`FeatureSorter` 排好的全局顺序）给
`minecraft:feature` / `minecraft:placed_feature` 的**配置类型 ID**分类。`features_per_step`
每个 step 里的条目是 placed feature，下面把它解析成 `Feature.DIRECT_CODEC` 的类型 ID 后统计。
「植被」归本分支（`cb-wg-feat-b`），其余归地物 A（`cb-wg-feat-a`）；拿不准的按分工归本分支。

数据版本 Minecraft Java 26.3，协议 777。`feature` 注册表 240 条、`placed_feature` 273 条、
`features_per_step` 11 步共 **171** 个全局有序条目；各步条数 `[0,4,5,4,4,0,34,7,3,109,1]`，
step 9（`vegetal_decoration`）占 109 个。

## 分类表（以 `features_per_step` 为准）

`defs` 是 `feature` 注册表里该类型的条数，`placed` 是 `placed_feature` 注册表里顶层为该类型
的条数，`steps` 是该类型的 placed feature 出现在哪些 `GenerationStep.Decoration` 步里。
`[]` 表示只被别的组合地物内联引用、或只在下界/末地，本身不直接进主世界
`features_per_step`（例如绝大多数 `tree` 都被 `random_selector` 内联）。

### 植被类（本分支 `cb-wg-feat-b`）

| 类型 ID | defs | placed | steps | 说明 |
|---|---:|---:|---|---|
| `minecraft:tree` | 45 | 46 | 9 | 含 trunk/foliage/root placer、decorator、蜂巢；樱花/红树/苍白橡树等变种；只有 `trees_cherry`/`trees_swamp` 直接进 step 9，其余被 selector 内联 |
| `minecraft:fallen_tree` | 6 | 6 | [] | 倒下的树，被 selector 内联 |
| `minecraft:simple_block` | 37 | 64 | 9 | 花草大多是它；也含蘑菇、南瓜、西瓜、海泡菜等 |
| `minecraft:vegetation_patch` | 6 | 3 | 9 | 26.3 的 patch 类（`RandomPatchFeature` 已不存在） |
| `minecraft:waterlogged_vegetation_patch` | 1 | 0 | [] | 含水 patch，被内联引用 |
| `minecraft:random_selector` | 21 | 23 | 9 | 组合类（按 chance 依次尝试） |
| `minecraft:weighted_random_selector` | 8 | 11 | 9 | 组合类（加权） |
| `minecraft:simple_random_selector` | 5 | 5 | 7, 9 | 组合类（等概率） |
| `minecraft:random_boolean_selector` | 2 | 2 | 9 | 组合类（`nextBoolean`） |
| `minecraft:sequence` | 3 | 3 | 1, 7 | 组合类（依次全部执行）；也见 step 1/7 |
| `minecraft:bamboo` | 2 | 3 | 9 | 竹子 |
| `minecraft:block_column` | 6 | 11 | 9 | 垂滴叶/大垂滴叶等竖直柱状 |
| `minecraft:block_pile` | 5 | 5 | [] | 树叶堆等，被 selector 内联 |
| `minecraft:vines` | 1 | 2 | 9 | 藤蔓 |
| `minecraft:multiface_growth` | 2 | 2 | 7, 9 | 发光地衣、苔藓等多面生长 |
| `minecraft:root_system` | 2 | 2 | 1, 9 | 根系（垂根）；`lush_caves` 的黏土+垂滴叶 |
| `minecraft:huge_fungus` | 4 | 2 | [] | 巨型菌（下界为主，按植被归本分支） |
| `minecraft:huge_brown_mushroom` | 1 | 0 | [] | 巨型棕蘑菇，被 selector 内联 |
| `minecraft:huge_red_mushroom` | 1 | 0 | [] | 巨型红蘑菇，被 selector 内联 |
| `minecraft:chorus_plant` | 1 | 1 | [] | 紫颂花（末地，按植被归类） |

### 非植被类（地物 A `cb-wg-feat-a`）

| 类型 ID | defs | placed | steps |
|---|---:|---:|---|
| `minecraft:ore` | 30 | 38 | 6, 7 |
| `minecraft:scattered_ore` | 2 | 2 | [] |
| `minecraft:disk` | 5 | 5 | 4, 6 |
| `minecraft:underwater_magma` | 1 | 1 | 6 |
| `minecraft:lake` | 1 | 2 | 1 |
| `minecraft:geode` | 1 | 1 | 2 |
| `minecraft:iceberg` | 2 | 2 | 2 |
| `minecraft:block_blob` | 1 | 1 | 2 |
| `minecraft:large_dripstone` | 1 | 1 | 2 |
| `minecraft:fossil` | 2 | 2 | 3 |
| `minecraft:monster_room` | 1 | 2 | 3 |
| `minecraft:blue_ice` | 1 | 1 | 4 |
| `minecraft:overlay` | 10 | 3 | 4 |
| `minecraft:spike` | 1 | 1 | 4 |
| `minecraft:spring_feature` | 6 | 7 | 8 |
| `minecraft:speleothem_cluster` | 2 | 2 | 7 |
| `minecraft:freeze_top_layer` | 1 | 1 | 10 |
| `minecraft:delta_feature` | 1 | 1 | [] |
| `minecraft:netherrack_replace_blobs` | 2 | 2 | [] |
| `minecraft:random_neighbor_spread` | 1 | 2 | [] |
| `minecraft:bonus_chest` | 1 | 0 | [] |
| `minecraft:end_gateway` | 2 | 1 | [] |
| `minecraft:end_island` | 1 | 1 | [] |
| `minecraft:end_platform` | 1 | 1 | [] |
| `minecraft:end_podium` | 2 | 0 | [] |
| `minecraft:end_spike` | 1 | 1 | [] |
| `minecraft:void_start_platform` | 1 | 1 | [] |

### 有歧义、按「拿不准的归你」划入本分支

`minecraft:simple_block`、`minecraft:sequence`、`minecraft:root_system`、
`minecraft:multiface_growth`、`minecraft:simple_random_selector` 也出现在 step 1 / 7 等
非植被步（例如 `lush_caves` 的黏土+垂滴叶）。按分工说明「也包括其他步里属于植被的」与
「拿不准的归你」，这些类型的实现由本分支负责；地物 A 在它的 step 里遇到这些类型时可复用。

## 本轮落地

见 `docs/worldgen-features-plan.md` 5.1 与本文件末尾的进度表。所有未移植类型照现有做法在
`run.rs` 标 `unsupported`，对拍时整块跳过，不假装支持。

## 依赖（`BlockStateProvider` / `BlockPredicate` / 值提供者 / 修饰器）

植被类 step 9 的 placed feature 用到的配置类型（按 `worldgen.json` 递归统计）：

- 修饰器：`block_predicate_filter`、`environment_scan`、`noise_based_count`、
  `noise_threshold_count`、`surface_water_depth_filter`、`surface_relative_threshold_filter`、
  `count_on_every_layer`、`fixed_placement`，加上已有的 `count`/`in_square`/`height_range`/
  `heightmap`/`biome`/`rarity_filter`/`offset`。
- `IntProvider`：`trapezoid`、`clamped`、`weighted_list`、`biased_to_bottom`、
  `very_biased_to_bottom`、`clamped_normal`（step 9 主要用 `trapezoid` 的 `offset`）。
- `BlockStateProvider`：`simple`（`{"id":..}` 内联块状态）、`weighted`、`randomized_int`、
  `rotated`、`random_block`、`noise`/`dual_noise`/`noise_threshold`（后三者需要 `NormalNoise`）。
- `BlockPredicate`：`matching_block_tag`、`matching_blocks`、`matching_fluids`、`all_of`、
  `any_of`、`not`、`replaceable`、`solid`、`has_sturdy_face`、`would_survive`、`always_true`。
- 高度图：`WORLD_SURFACE_WG` / `MOTION_BLOCKING` / `MOTION_BLOCKING_NO_LEAVES` /
  `OCEAN_FLOOR_WG` 等，须按官方 `Heightmap.update` 在 `setBlockState` 后增量更新。

`noise_threshold_count` / `noise_based_count` 用的是 `Biome.BIOME_INFO_NOISE`
（`SimplexNoise(WorldgenRandom(LegacyRandomSource(2345L)))`，与世界种子无关），
`mc-bamboo-worldgen::biome_temperature` 已构造同一份，见那里的 `static_noises`。
