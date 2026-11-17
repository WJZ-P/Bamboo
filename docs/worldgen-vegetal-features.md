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

见 `docs/worldgen-features-plan.md` 5.1 与下面的进度表。所有未移植类型照现有做法在
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

## 进度表

以 `--feature-types` 按 placed feature 名隔离对拍，逐块 0 不一致才算做完一项。
范围调整（2026-11-14）：树 / `fallen_tree` / 巨型蘑菇移交 `cb-wg-trees`，水下植被与
其余植被移交 `cb-wg-aquatic`；本分支只做组合类、`simple_block` 剩余种类、谓词。

| 类型 / 支撑件 | 状态 | 对拍 |
|---|---|---|
| `minecraft:simple_block`（内联 bare-state provider） | 已做，按方块类白名单判 `canSurvive` | 前 11 个 placed feature 多种子 0 不一致 |
| 内联新类（SweetBerryBush / Eyeblossom / FireflyBush / Sapling / FlowerBed / Short·TallDryGrass / LeafLitter / melon / DoublePlant） | 已做 | 12 个 placed feature 90 中心 0 不一致，改坏报 160 处 |
| `DoublePlantBlock.placeAt`（上下两格 + `half`） | 已做 | 随 DoublePlant 花 / 草对拍 |
| `IntProvider` trapezoid / biased / very_biased / clamped | 已做 | 单测 + `forest_flowers` 的 count 对拍 |
| `BlockPredicate`：tag / blocks / all_of / any_of / not / replaceable / solid / true | 已做 | 单测 + `block_predicate_filter` 对拍 |
| `would_survive` / `has_sturdy_face` / `matching_fluids` 谓词 | 已做 | firefly_bush / melon 等对拍；has_sturdy_face 仅单测（kelp 已移交） |
| `block_predicate_filter` / `Heightmap.update` / `--feature-types` | 已做 | 见上，故意改坏确认非假绿 |
| `BlockStateProvider` weighted / randomized_int / 内联 bare-state | 已做 | 9 个 weighted 90 中心 0 不一致，改坏报 120 处 |
| 组合类：random_selector / weighted_random_selector / simple_random_selector / random_boolean_selector / sequence | 已做（框架 + 递归 `PlacedFeature.place`） | simple_random_selector（forest_flowers / flower_forest_flowers）90 中心 0 不一致，改坏报 52 处；其余子项多为树 / 水生（已移交），未逐块验证 |
| 树 / `fallen_tree` / 巨型蘑菇 | 移交 `cb-wg-trees`（**已合入 main**） | — |
| 水下植被 / 其余植被（见 `docs/worldgen-features-plan.md`「第二轮」） | 已由 `cb-wg-aquatic` 完成大部分 | 见该文档的对拍表 |
| `noise_threshold_count` / `count_on_every_layer` / `surface_water_depth_filter` 修饰器 | 已做（`placement.rs`） | `noise_threshold_count` 在 step 9 的 6 个花/草 placed feature 上 0 不一致；`surface_water_depth_filter` 随 26 个 `trees_*` 对拍；`count_on_every_layer` 主世界 `steps=[]`（只被下界使用），只有单测 |
| `environment_scan` / `surface_relative_threshold_filter` / `noise_based_count` 修饰器 | 已由 `cb-wg-aquatic` 完成 | `block_column` / `glow_lichen` / `kelp_cold` 多种子 0 不一致 |
| 蘑菇（亮度）/ 睡莲 / 孢子花 / 海泡菜 / 海草 / 地毯的 `canSurvive` | 已由 `cb-wg-aquatic` 完成 | 见 `docs/worldgen-features-plan.md`「第二轮」 |
| 苔藓地毯（`MossyCarpetBlock.placeAt`，含侧面 `nextBoolean`） | 已做（`mossy_carpet.rs`，`region_random` 用裸 `RandomSource`） | `pale_moss_patch` 在 `--chunk 529509 523106 --seeds 6` 0 处，改坏 `nextBoolean` 报 6 处 |
| holder 引用的 `to_place`（`BLOCK_STATE_PROVIDER` 注册表） | 已由 `cb-wg-aquatic` 完成（注册表已导出） | `cave_vines` 24 中心 0 不一致 |
| `clamped_normal` IntProvider | 已做（`provider.rs` 的 `ClampedNormal` + `next_gaussian`） | 单测；主世界只被 step 7 的 dripstone 族用到 |
| `weighted_list` IntProvider | 已做 | 见 `docs/worldgen-features-plan.md`「第二轮」（合并 main 后取 main 的 `Box<IntProvider>` 版） |
| Director 路径的地物合并（中心自己的写 + 8 个邻居中心的溢出，规范序） | 已做；2026-11-16 修掉结构附近无快照补格不带 Beardifier 造成的 1/N 线程差异，地物结果改为全进程共享缓存 | 游戏内 `director_matches_pure_features_near_structures`（1/4 线程与纯函数逐格相同，改回旧补格报差异）、`one_and_four_threads_generate_identical_feature_blocks`；无 Java 对拍覆盖 |
| `java_set`（`HashSet<BlockPos>` 桶树化后的迭代序） | 已做（`cb-wg-feat-c`，按 JDK 25 字节码复刻 `treeifyBin` / `treeify` / `putTreeVal` / `TreeNode.split`） | `JavaSetOracle.java` 80 组插入序列逐个比对；`--chunk -58 -217 --scan 5 --seeds 2` 从 1 skipped 变成 0 skipped / 0 不一致；把 `putTreeVal` 改成追加桶尾报 10 处 |
| `projected_random_patchy_square` | 26.3 导出数据无条目 | 不需要实现 |
| 方块实体数据（蜂巢 / 可疑沙等） | 已做（结构线 `structure-e2e` 现在比 NBT；可疑的沙子的表与种子已按字节码写入） | `gen-world --through features --structures` / `structure-e2e` 52 区块 117 个方块实体 0 不一致 |

实测说明见 `docs/worldgen-feat-c-notes.md`：`features_per_step` 的 171 个条目在真实区块上
已不再报 `unsupported`（`fossil` / `desert_well` 的 JVM 侧限制属结构线，不是 Rust 侧跳过）。

