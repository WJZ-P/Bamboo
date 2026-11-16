# 主世界地物剩余项：`unsupported` 实测与交接线索

本文记录 `cb-wg-feat-c` 开工时对 `features_per_step`（11 步共 171 个全局有序条目）
的实测，以及转交给 `cb-wg-e2e` 的逐类型验证线索。数据版本 Minecraft Java 26.3。

## 1. 真实区块上哪些条目报 `unsupported`

做法：临时在 `feature/run.rs` 的 `if !outcome.supported` 分支里按 `(step, placed_id)`
记数（环境变量开关 + 文件输出），再跑 `chunkgen --through features`。诊断代码是临时的，
不进提交；统计结果如下。

| 对拍参数（`--through features`） | 中心数 | skipped | (step, index, 名字, 次数) |
|---|---:|---:|---|
| `--seeds 3 --chunks 12 --salt 7` | 36 | 0 | 无 |
| `--seeds 3 --chunks 40 --salt 7 --steps 9` | 120 | 0 | 无 |
| `--chunk 480 0 --scan 9 --seeds 1`（低段 lush_caves） | 9 | 0 | 无 |
| `--chunk 529509 523106 --scan 5 --seeds 2`（pale_garden） | 18 | 0 | 无 |
| `--chunk -58 -217 --scan 5 --seeds 2`（繁茂洞穴天花板） | 17 | **1** | `(9, 86, minecraft:lush_caves_ceiling_vegetation, 1)` |
| `--chunk 0 52 --scan 9 --seeds 1` | 9 | 0 | 无 |
| `--chunk -8628 0 --scan 9 --seeds 1`（蘑菇岛） | 9 | 0 | 无 |
| `--chunk 177 -177 --scan 3 --seeds 1`（deep_dark） | 9 | 0 | 无 |

唯一会报 `unsupported` 的条目是 step 9 的 `minecraft:lush_caves_ceiling_vegetation`，
原因不是地物类型没移植，而是 `feature/java_set.rs` 的 `HashMap` 桶树化：
临时诊断在 `VegetationPatch::place` 里打印出触发时的地表集合是
`origin=(-914, -27, -3470) size=64`，容量到 64 后有桶超过 8 个节点，官方会转红黑树，
本实现当时只置 `treeified` 标记让整块跳过。`cb-wg-feat-c` 已补上树化（见
`docs/worldgen-features-plan.md`「第二十二轮」与 `feature/java_set.rs`），这个 skipped
随之消失。

不带 `--steps 9` 的 `--seeds 3 --chunks 40 --salt 7` 会遇到官方
`FossilFeature.place` 在 `ChunkgenOracle` 的代理上抛
`ClassCastException`（`getLevel()` 要 `ServerLevel`），JVM 侧在出完部分线后异常退出；
这是已知的对拍工具限制，不是 Rust 侧的 `unsupported`，要跑大样本就加 `--steps 9`
或换不含化石的区块。

### 静态核对（递归遍历 171 个 step 条目 + 内联子项，共 216 个 placed feature）

所有出现过的构件在 main 上都已实现并接入 `dispatch` / `placement` / `composition`：

* 修饰器 15 种：`count` / `in_square` / `biome` / `heightmap` /
  `block_predicate_filter` / `offset` / `height_range` / `rarity_filter` /
  `surface_water_depth_filter` / `environment_scan` / `noise_threshold_count` /
  `noise_based_count` / `surface_relative_threshold_filter` / `randomly_selected` /
  `random_chance`。
* 谓词 10 种：`matching_blocks` / `matching_block_tag` / `matching_fluids` /
  `all_of` / `any_of` / `not` / `solid` / `true` / `has_sturdy_face` /
  `inside_world_bounds`。
* 状态提供者：内联 bare state、`weighted` / `randomized_int` / `rule_based` /
  `rotated` / `random_block` / 三个 noise 形态，外加 `block_state_provider` 注册表
  （字符串 holder）已在 `worldgen.json` 导出并由 `registry.rs::state_provider()` 解析。
* `IntProvider` 8 种：`constant` / `uniform` / `trapezoid` / `biased_to_bottom` /
  `very_biased_to_bottom` / `clamped` / `clamped_normal` / `weighted_list`。
* 地物类型：`features_per_step` 的 12 种顶层类型加递归子项全部有实现；
  `block_pile` / `huge_fungus` / `chorus_plant` / `projected_random_patchy_square`
  不在主世界表里（`steps=[]`），不需要实现。

也就是说，**`features_per_step` 上没有「整类标 unsupported 且未移植」的地物类型**。
旧文档 `docs/worldgen-vegetal-features.md` 进度表和
`docs/worldgen-features-plan.md`「还没做」里列的睡莲 / 孢子花 / 苔藓地毯 / 海泡菜
`canSurvive`、`noise_threshold_count` / `count_on_every_layer` /
`surface_water_depth_filter`、`clamped_normal`、`BLOCK_STATE_PROVIDER` 注册表等，
都已在 main 落地，进度表已同步更新。

### 一处顺带修掉的解析缺口

大样本诊断曾报 `(9, 242, minecraft:trees_cherry, 2)`：追到 `feature/tree/trunk.rs` 的
`CherryTrunkPlacer` 解析，官方 `BRANCH_START_CODEC` 是 `UniformInt.MAP_CODEC` +
`validate(max-min>=1)`，导出的 `branch_start_offset_from_top` 只有
`min_inclusive` / `max_inclusive` 两个字段、**没有 `type`**；旧代码走
`IntProvider::parse` 要求 `type` 而返回 `None`，于是 `cherry` 树整株标 unsupported。
现在按两个边界直接构造 `UniformInt`，`trees_cherry` 在 `--seeds 3 --chunks 40
--salt 7 --steps 9` 下不再报 unsupported。

### 结构线（不动）

`fossil`（desert / swamp / mangrove_swamp，step 3）的 Rust 实现完整，但官方
`FossilFeature.place` 需要 `ServerLevel.getStructureTemplateManager()`，
`ChunkgenOracle` 的 `WorldGenLevel` 代理抛 `ClassCastException`，JVM 侧一格都写不出，
只能靠集成测试验证。`desert_well` 的 `minecraft:template` / processor 子集同理属结构
线（`cb-wg-structures`），本分支不改。

## 2. 转交 `cb-wg-e2e` 的逐类型验证线索

以下「逐类型触发次数 + 改坏确认」按分工由 `cb-wg-e2e` 做，本分支不碰
`gen-world` 命令行与端到端框架。线索来自 `docs/worldgen-features-plan.md` 与导出数据：

* `waterlogged_vegetation_patch` 唯一入口是 `minecraft:lush_caves_clay`
  （`random_boolean_selector` 子项 `clay_pool_with_dripleaves`），只在 `lush_caves`。
  野外几乎不触发：`environment_scan` 从 `height_range` 落点向下找 solid，落点常在地
  表附近，`BiomeManager` 的八角平滑把它判成 `jungle`，被 `BiomeFilter` 杀掉。
  能真正跑到 `waterlogged=true` 的种子/区块：seed 0 的低 8 段 lush_caves 占 75%+
  的 `(476,0)` / `(480,0)` 0.803 / `(484,0)` / `(488,0)` / `(-528,528)` /
  `(-532,532)`；用
  `--steps 9 --feature-types minecraft:lush_caves_clay --chunk 480 0 --scan 9 --seeds 1`
  曾得到 0 不一致 / 0 skipped（9 中心 / 7962624 方块），把 `is_exposed` 改成恒 `true`
  报 160 处。
  **注意**：`--feature-types` 必须写 placed feature 注册名 `minecraft:lush_caves_clay`，
  写类型 ID `minecraft:waterlogged_vegetation_patch` 会让 Java 侧
  `applyFeatureAllowList` 抛错（白名单按 placed feature 过滤）。
* `pale_moss_patch`（`MossyCarpetBlock.placeAt`，含侧面 `nextBoolean`）：文档第 20 轮
  的 2 处差异在 `chunk (529509,523106)` 的 `(12,135,10)` / `(12,136,10)`，根因是
  `region_random` 类型（官方是裸 `XoroshiroRandomSource`，`nextBoolean` 取 long 低位，
  不是 `WorldgenRandom.next(1)` 的高位）。修好後 `--chunk 529509 523106 --seeds 6`
  为 0 处，把取数改成 `!random.next_boolean()` 报 6 处。
* `root_system`（`rooted_azalea_tree` / `rooted_sulfur_spring`）：官方 26.3 的逻辑本身
  几乎不可达（`place` 要求 origin 是空气，`placeDirtAndTree` 第一次迭代又要求 origin
  `isSolid()`），「改坏抓不到」是正确结果而非假绿；需要合成场景或说明，不要当缺陷。
* 需要确认在真实区块上确实被取用的构件：
  * `noise_threshold_count`：6 个 placed feature，全在 step 9 ——
    `flower_cherry` / `flower_plains` / `patch_grass_meadow` / `patch_grass_plain` /
    `patch_tall_grass_2` / `wildflowers_meadow`。
  * `surface_water_depth_filter`：26 个，全在 step 9 —— `bamboo_vegetation`、
    `birch_tall`、`dark_forest_vegetation`、`pale_garden_vegetation`、`trees_badlands`、
    `trees_birch`、`trees_birch_and_oak_leaf_litter`、`trees_cherry`、
    `trees_dappled_forest`、`trees_flower_forest`、`trees_grove`、`trees_jungle`、
    `trees_mangrove`、`trees_meadow`、`trees_old_growth_pine_taiga`、
    `trees_old_growth_spruce_taiga`、`trees_plains`、`trees_savanna`、`trees_snowy`、
    `trees_sparse_jungle`、`trees_swamp`、`trees_taiga`、`trees_water`、
    `trees_windswept_forest`、`trees_windswept_hills`、`trees_windswept_savanna`。
  * `count_on_every_layer`：主世界 `features_per_step` 0 处，只被 8 个下界 placed
    feature 用（`crimson_forest_vegetation` / `crimson_fungi` / `delta` /
    `large_basalt_columns` / `nether_sprouts` / `small_basalt_columns` /
    `warped_forest_vegetation` / `warped_fungi`），无法在主世界对拍。
  * `clamped_normal`：主世界只出现在 `dripstone_cluster` / `pointed_dripstone` /
    `sulfur_spike`（step 7 的 `speleothem_cluster` / `simple_random_selector`）。
  * 睡莲 / 孢子花 / 苔藓地毯 / 海泡菜 / 海草 / 蘑菇的 `canSurvive` 与
    `DoublePlantBlock.placeAt`：分别在 `lily_pad`、`spore_blossom`、
    `pale_moss_patch`、`sea_pickle`、`seagrass` / `tall_seagrass`、
    `mushroom_island_vegetation` 等 placed feature 上验证；蘑菇岛可用
    `--chunk -8628 0 --scan 9 --seeds 1`（seed 0 下 1536/1536 个 quart 格都是
    `mushroom_fields`）。
