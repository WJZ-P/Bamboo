# 端到端逐类型对拍覆盖报告（`gen-world --through features`）

这一篇记录「官方在 JVM 里生成、Bamboo 经 Director 的同一条路径比较」的逐类型对拍结果，
命令和用法见 `docs/worldgen.md` 的「端到端逐类型对拍」一节。数据来自本分支在本机
release 构建上的实跑；每次都先跑干净对拍（要求 0 不一致），再跑改坏确认（要求对拍报错），
两次都通过才算 `PASS`。只做过干净跑、没做改坏确认的 0 不一致不算数。

## 统计口径

* 「触发次数」是这一批中心区块里，该类型的 placed feature 交给 `Feature.place` 的
  **候选位置总数**（`perf::FeatureRecord::candidates`，只在顶层 placed feature 上记一次，
  由 `--coverage` 打印）。候选位置通过了整条修饰器链，但不保证最后真的写了方块；
  「改坏格数」才是真正被该类型改过的格数（每个中心最多取一格）。
* 「种子」列里的 `--seeds N` 用的是 `ChunkgenOracle.java` 的 `FIXED_SEEDS`：
  `0, -1, 1, 1234567890123456789, -8765432109876543210`，取前 N 个。
  `--chunk X Z --scan N` 是以该区块为中心的连续网格。
* 改坏确认由 `--break-type KIND` 完成：单独跑这个类型，和「一个地物都不跑」的
  CARVERS 基线逐格比较，找到它真正写过的格子，把那一格换成岩浆，再要求逐方块比较器
  报不一致。`0 不一致 + 改坏不报错` 会被判 `FAIL`。
* 所有行都带着该类型实际出现的 step 掩码（`--steps`）；白名单两边用同一组字符串，
  Java 侧把不在白名单里的 `HolderSet` 换空，Bamboo 侧用同一规则收集 `(step, index)`，
  保持 `setFeatureSeed` 的全局 index 不变。

## 已确认的类型（28 个，全部 PASS）

| Feature 类型 | step | 种子 / 区块 | 触发次数 | 干净跑 | 改坏格数 | 改坏不一致 | 结果 |
|---|---:|---|---:|---|---:|---:|---|
| `minecraft:lake` | 1,2 | `--seeds 3 --chunks 8 --salt 7` | 2 | 0 | 2 | 2 | PASS |
| `minecraft:geode` | 2 | 同上 | 1 | 0 | 1 | 1 | PASS |
| `minecraft:iceberg` | 2 | `--seeds 5 --chunk 146 -50 --scan 9` | 1 | 0 | 1 | 1 | PASS |
| `minecraft:large_dripstone` | 2 | `--seeds 1 --chunk -53 -206 --scan 5` | 50 | 0 | 3 | 3 | PASS |
| `minecraft:block_blob` | 2 | `--seeds 2 --chunk 83 -274 --scan 9` | 10 | 0 | 6 | 6 | PASS |
| `minecraft:monster_room` | 3 | `--seeds 1 --chunk 3 -1 --scan 5` | 126 | 0 | 1 | 1 | PASS |
| `minecraft:fossil` | 3 | `--seed 42 --chunk -267 -276 --scan 25` | 1 | 0 | 1 | 1 | PASS |
| `minecraft:overlay`（`desert_well`） | 4 | `--seed 42 --chunk -267 -276 --scan 1` | 1 | 0 | 1 | 1 | PASS |
| `minecraft:blue_ice` | 4 | `--seeds 1 --chunk 255 -109 --scan 5` | 29 | 0 | 1 | 1 | PASS |
| `minecraft:spike` | 4 | `--seeds 1 --chunk 268 -300 --scan 3` | 10 | 0 | 1 | 1 | PASS |
| `minecraft:disk` | 4,6 | `--seeds 3 --chunks 8 --salt 7` | 80 | 0 | 4 | 4 | PASS |
| `minecraft:ore` | 6,7 | 同上 | 5792 | 0 | 24 | 24 | PASS |
| `minecraft:underwater_magma` | 6 | 同上 | 414 | 0 | 1 | 1 | PASS |
| `minecraft:multiface_growth` | 7,9 | `--seeds 1 --chunk -42 111 --scan 4` | 1206 | 0 | 9 | 9 | PASS |
| `minecraft:speleothem_cluster` | 7 | `--seeds 1 --chunk -55 -200 --scan 9` | 54 | 0 | 2 | 2 | PASS |
| `minecraft:sequence` | 1,7 | `--seeds 1 --chunk -42 111 --scan 3` | 544 | 0 | 9 | 9 | PASS |
| `minecraft:simple_random_selector` | 7,9 | `--seeds 3 --chunks 12 --salt 13` | 269 | 0 | 1 | 1 | PASS |
| `minecraft:spring_feature` | 8 | `--seeds 3 --chunks 8 --salt 7` | 1080 | 0 | 2 | 2 | PASS |
| `minecraft:bamboo` | 9 | `--seeds 1 --chunk -358 -372 --scan 9` | 637 | 0 | 6 | 6 | PASS |
| `minecraft:block_column` | 9 | `--seeds 3 --chunks 8 --salt 7` | 324 | 0 | 7 | 7 | PASS |
| `minecraft:random_boolean_selector` | 9 | `--seeds 3 --chunks 12 --salt 7` | 2 | 0 | 1 | 1 | PASS |
| `minecraft:random_selector` | 9 | `--seeds 3 --chunks 8 --salt 7` | 60 | 0 | 9 | 9 | PASS |
| `minecraft:simple_block` | 9 | `--seeds 1 --chunk 0 52 --scan 5` | 4056 | 0 | 9 | 9 | PASS |
| `minecraft:tree` | 9 | `--seeds 1 --chunk 13 -400 --scan 9` | 17 | 0 | 9 | 9 | PASS |
| `minecraft:vegetation_patch` | 9 | `--seeds 3 --chunks 8 --salt 7` | 1 | 0 | 1 | 1 | PASS |
| `minecraft:vines` | 9 | 同上 | 15 | 0 | 1 | 1 | PASS |
| `minecraft:weighted_random_selector` | 9 | 同上 | 534 | 0 | 17 | 17 | PASS |
| `minecraft:freeze_top_layer` | 10 | `--seeds 1 --chunk 255 -109 --scan 9` | 9 | 0 | 8 | 8 | PASS |

触发次数为 0 或太少的类型，都换了能触发它的种子和区块：`monster_room` 用三个已知
生成点 `(3,-1)` 附近的连续网格，`iceberg` / `block_blob` / `bamboo` / `tree` 先按群系
预设找到对应群系主导的区块再扫网格，`blue_ice` / `spike` / `freeze_top_layer` 用设计稿
里记录的冰雪群系区块，`large_dripstone` / `speleothem_cluster` 用滴水石洞穴定点。

### `monster_room` 的假绿已修好

交接里说 `monster_room` 的 0 不一致是假绿（改坏抓不到）。根因不在地物实现，而是
`gen-world --through features` 当时**硬编码 `--ore-only`**，Bamboo 侧只跑矿石；拿
`monster_room` 的对拍结果去解释它当然抓不到任何改动。现在命令支持 `--feature-types`
和 `--break-type`，上表这一行是在「Java 和 Bamboo 都只跑 `minecraft:monster_room`」的
条件下实跑的：改坏一格报 1 处不一致。

## 已知限制（1 个类型）

| Feature 类型 | step | 字节码核对的触发条件 |
|---|---:|---|
| `minecraft:root_system` | 1,9 | `RootSystemFeature.place` 先要求 `origin` 是空气；`placeDirtAndTree` 第一轮把 `workingPos` 上移一格，然后要求 `workingPos.below()`（**正好就是 origin**）`isSolid()`，不满足直接 `return false`。这是官方 26.3 自身的死代码，见 `docs/worldgen-features-plan.md` 第十六轮核对。 |

`fossil` 和 `overlay` 原来记在这里：`fossil` 被当成「对拍框架不可比」（JVM 侧抛
`ClassCastException` / NPE），`overlay` 是 `rarity 1/1000` 扫不到。两者现在都已覆盖，
见文末「沙漠水井 / 化石」一节。

## 复跑

```bash
cargo build --release -p mc-bamboo-vanilla
tools/mc-bamboo-vanilla/feature-e2e.sh               # 全部 26 个类型
tools/mc-bamboo-vanilla/feature-e2e.sh minecraft:ore # 只跑一个类型
```

脚本逐类型跑「干净对拍 + 改坏确认」，打印和本表同口径的统计，任一失败就以非 0 退出。
新加地物时把类型、step 掩码和能触发它的种子/区块加进脚本的 `CASES` 表，再按
`docs/worldgen.md` 的步骤更新本文。

## primed 高度图覆盖（2026-11-16）

`mc-bamboo-worldgen::feature::world::prime_heightmaps` 从 CARVERS 方块算出
`FeatureChunks.heights` 的六张图，地物阶段的下列路径会读它：

| 读哪张图 | 官方读它的位置（`javap -c -p` 核对） | Bamboo 侧 |
|---|---|---|
| `WORLD_SURFACE_WG` | `HeightmapPlacement`（`PlacementContext.getHeight`）、`LargeDripstoneFeature` 的向上生长上限、`SurfaceRelativeThresholdFilter`（glow_lichen / underwater_magma / lake_lava_underground） | `placement.rs::Modifier::Heightmap`、`large_dripstone.rs`、`placement.rs::SurfaceRelativeThreshold` |
| `WORLD_SURFACE` | `BambooFeature`（podzol 铺面）、`SurfaceWaterDepthFilter`（与 OCEAN_FLOOR 之差）、`SurfaceRelativeThresholdFilter` | `bamboo.rs`、`placement.rs` |
| `OCEAN_FLOOR_WG` | `OreFeature` 的逐列剪枝、`HeightmapPlacement`（disk_* / warm_ocean_vegetation） | `ores.rs`、`placement.rs` |
| `OCEAN_FLOOR` | `SurfaceWaterDepthFilter`、`HeightmapPlacement`（kelp / seagrass / 树） | `placement.rs` |
| `MOTION_BLOCKING` | `SnowAndFreezeFeature`（freeze_top_layer）、`HeightmapPlacement`、find_on_ground 的起点 | `freeze_top_layer.rs`、`placement.rs` |
| `MOTION_BLOCKING_NO_LEAVES` | `TreeDecorator.Context` 的叶藤蔓装饰、`HeightmapPlacement`（pale_garden_flowers / pale_moss_patch） | `tree/decorator.rs`、`placement.rs` |

之前 `gen-world --through features` 不带白名单时**默认只放矿石**（Java 侧
`--ore-only`），所以只有 `OCEAN_FLOOR_WG` 会被读到；把六张图任意写错，其余五张都
报 0 不一致——这是端到端对拍的盲区，等价性只剩单测。

现在 `tools/mc-bamboo-vanilla/heightmap-e2e.sh` 用
`--through features --all-features --seeds 3 --chunks 8 --salt 7`（24 个中心、九宫格
共 21 233 664 格）跑一批**全部地物**的坐标，配合新增的 `--break-heightmap FIELD:COLUMN`
改坏钩子（`perf` 特性下把 primed 图的某一列 +1，`COLUMN=all` 时整张图每列 +1）逐张确认：

| 高度图 | 整体 +1 报不一致 | 单列 +1 报不一致 | 结果 |
|---|---:|---:|---|
| `WORLD_SURFACE_WG` | 176 | 列 32：6 | PASS |
| `WORLD_SURFACE` | 3189 | 列 127：184 | PASS |
| `OCEAN_FLOOR_WG` | 1044 | 列 0：8 | PASS |
| `OCEAN_FLOOR` | 5616 | 列 0：2 | PASS |
| `MOTION_BLOCKING` | 1006 | 列 0：2 | PASS |
| `MOTION_BLOCKING_NO_LEAVES` | 3611 | 列 0：1261 | PASS |

单列列号是**实测会被某个地物候选位置读到**的列：地物只在自己候选位置的
`(x,z)` 上问 `getHeight`，任意一列不一定被问到，所以不能要求每一列都报错。
干净跑 0 不一致，12 组改坏全部报不一致；任一报不出来脚本以非 0 退出。
`feature-e2e.sh` 也在逐类型循环之前加了同一批坐标的 `--all-features` 整步干净跑，
保证这批坐标常驻在常规回归里。

`--break-heightmap` 只在 `mc-bamboo-worldgen` 的 `perf` 特性下生效（`mc-bamboo-vanilla`
的 `oracle` 特性会打开它）；默认构建里 `heightmap_break()` 恒为 `None`，
`prime_heightmaps` 不改任何值，生成结果不变。改坏钩子刻意放在
`prime_heightmaps` 的返回值上，而不是对拍框架里，这样 Java 侧和 Bamboo 侧用的仍是
同一套方块，只有 Bamboo 的 primed 图被改坏——比较器必须报出来。

```bash
cargo build --release -p mc-bamboo-vanilla
tools/mc-bamboo-vanilla/heightmap-e2e.sh
# 单独验一张图
./target/release/mc-bamboo-vanilla gen-world java --through features \
    --all-features --seeds 3 --chunks 8 --salt 7 \
    --break-heightmap MOTION_BLOCKING_NO_LEAVES:all
```

## 带结构的整块对拍修掉的 bug（2026-11-16）

`gen-world --through features --structures` / `structure-e2e` 把开结构的整块生成
逐方块对拍后，抓出并修掉了下面这些真 bug。完整覆盖表、命令与剩余缺口见
[`docs/worldgen-structures-e2e.md`](worldgen-structures-e2e.md)。

| 现象 | 根因 | 现在由哪个对拍盯着 | 之前为什么没发现 |
|---|---|---|---|
| 矿井 / 试炼密室随机方块整片错位 | `place_step` 的 `WorldgenRandom` 包 `LegacyRandomSource`；官方 `applyBiomeDecoration` 包 `XoroshiroRandomSource` | `structure-e2e`、`gen-world --structures --steps 3` | 旧结构放置对拍用合成地形 + 自建随机源，不是真实 decoration 随机流 |
| 多起点反查顺序不同导致后续随机全错 | `References` 用扫描顺序 `Vec`；官方是 `LongOpenHashSet` 的桶序 | `long-set`（3000 组 / 287248 次比较）、`structure-e2e` 的 `(0,-17)` | `structure-pipeline` 比起点前先排序，顺序不影响它 |
| 试炼密室铜灯不氧化 | 具名 `processor_list` 查表前剥掉了 `minecraft:` 前缀 | `structure-e2e` 的 `(17,7)` | `jigsaw apply` 直接传列表对象，不走注册表 |
| 前哨瞭望塔 / 部分村庄结构件不生成 | `ListPoolElement` 被当成无模板跳过 | `structure-e2e` 的 `(-379,-303)` 与村庄五种 | `jigsaw placepiece` 只测 `SinglePoolElement` |
| 村庄植被 / 树 / 南瓜堆不生成 | `FeaturePoolElement` 被跳过；官方调 `PlacedFeature.place` 并复用 `StructureStart` 的随机源 | `structure-e2e` 村庄区块 | 旧查询集没有村庄起点 |
| 村庄街道逐层抬高 | `GravityProcessor` 无条件把 `WORLD_SURFACE_WG` 换成 `WORLD_SURFACE`；官方只在 `ServerLevel` 上换 | `structure-e2e` 村庄区块 | 只在单件上对拍过 gravity |
| step 10 地物在村庄读到旧高度 | `BlockWriter for FeatureWorld` 用不更新高度图的 `set_block` | `structure-e2e` 全步、`feature-e2e.sh minecraft:freeze_top_layer` | 无结构模式没有结构件写方块 |
| 沙漠神殿 `afterPlace` 抛异常 | 假世界没实现 `getBlockEntity(pos,type)` 的 `Optional` 重载 | `structure-e2e` 的 `(-374,-22)` | 旧对拍没跑到 `placeSuspiciousSand` |
| `minecraft:block_pile` 标 unsupported | 叶子地物未移植；阈值分支方向按字节码是 `dist2 > threshold` 才走 `nextFloat()<0.031` | `structure-e2e` 村庄区块 + 改坏确认 | 主世界 `biome_features` 不含它，只作结构池元素 |

## 第 2 部分补查（`wg-e2e-struct` 续）

| 现象 | 根因 | 现在由哪个对拍盯着 |
|---|---|---|
| 沙漠神殿起点 33 处 | `DesertPyramidStructure.afterPlace` 未实现（`SortedArraySet` + `Util.shuffle` 放可疑的沙子与塌陷屋顶） | `structure-e2e` 沙漠神殿区块 |
| 要塞起点 412 处 | `randomSmallDoor` 在放置阶段被重取，官方只读布局阶段存好的 `EntryDoor` | `structure-e2e` 要塞区块 |
| `(42,-32,-16)` 的 `potent_sulfur[wet]` | `LakeFeature.place` 对水湖返回 true，`SequenceFeature` 才不短路；`PotentSulfurBlock` 需进 `survival::classify` | `gen-world --chunk -32 -16`（已加回 `structure-e2e`） |
| 四种稀有结构从不出起点 | `generate_start` 按 `type_id` 分派却写了注册 ID；海底神殿的 `place_in_chunk` 没接进 `place_start` | `structure-e2e` 的林地府邸 / 海底神殿 / 丛林神殿 / 海底遗迹起点 |

## 带结构的整块对拍：方块实体（后续一轮）

`gen-world --through features` / `structure-e2e` 现在也导出并比较 3×3 每格
实例化过的方块实体，详见 [`docs/worldgen-structures-e2e.md`](worldgen-structures-e2e.md)
的「方块实体比较」。默认 52 区块 `46006272 blocks / 117 block entities /
0 mismatches`。新比较器翻出并修掉了 `LootTableSeed` 随机源类型、模板实体 nbt
未记录、古城箱子多写种子、宝库 `config` 默认表、数值字段类型、告示牌文本编码、
刷怪笼 / 雪屋箱子缺实体，以及可疑的沙子（沙漠神殿 / 海底遗迹 / 沙漠水井）
没写战利品表与种子等问题。沙漠神殿起点 3 处残差也在这轮修掉并加回默认集；
要塞起点还剩 4 处矿井支撑柱差异，留在 `--known-gaps`。

## 沙漠水井 / 化石（`wg-struct-residual`）

* **假世界缺口**：`FossilFeature.place` / `TemplateFeature.place` 第一步都是
  `level.getLevel().getServer().getStructureTemplateManager()`。无结构模式下
  `ChunkgenOracle` 的 `getLevel()` 返回 null（`ClassCastException` 的来源），
  带结构模式下不跑构造器的服务端 `structureTemplateManager` 字段是 null（NPE）。
  现在两种模式都给 `getLevel()` 接上惰性 `ServerLevel`，服务端字段接上对拍自己的
  `StructureTemplateManager`。另外补了 `WorldGenRegion.getEntities`（两个重载都返回
  空表；`SculkVeinBlock` → `Block.pushEntitiesUp` 会调）。
* **一放出来就翻出的真问题**（字节码核对）：
  * `TemplateFeature` / `FossilFeature` 都调了 `settings.setRandom(random)`，于是
    `StructurePlaceSettings.getRandomPalette` 的 `getRandom(pos)` 直接返回地物随机源，
    单调色板也要取一次 `nextInt(1)`（化石两次 `placeInWorld` 各一次）。少取这一次，
    水井后两个 `randomly_selected` 选错可疑的沙子的位置。
  * `TemplateFeature` 写方块要按 `Palette.blocks()`（`buildInfoList` 顺序），带 nbt 的格先
    写 `BARRIER`；`AppendLoot` 的种子来自 `RuleProcessor.processBlock` 的
    `Mth.getSeed(blockInfo.pos())`——这里的 `blockInfo` 已经被 `processBlockInfos` 换算成
    旋转、平移后的**世界坐标**；原实现用模板相对坐标（改回相对坐标即 44 处
    `LootTableSeed` 不一致，作为改坏确认）。
  * `FossilFeature` 写方块走 `WorldGenRegion.setBlock`，要更新 FINAL 高度图。
  * `MonsterRoomFeature` 的箱子（`RandomizableContainer.setBlockEntityLootTable`，
    `chests/simple_dungeon` + `nextLong` 种子）和刷怪笼（`setEntityId` 写出的默认刷怪
    数据）方块实体之前只取数不记录。
* `structure-e2e --scan-desert-well SEED RADIUS` 只算 `desert_well` 放置链的前两步
  （`rarity_filter` + `in_square`）和候选列的沙漠群系，打印 `seed cx cz` 候选行给
  `--chunks-file`。种子 42、半径 300 出 29 个候选，整块对拍（方块 + 方块实体）
  0 不一致；`(42,-267,-276)`（水井）、`(42,-267,-277)`（化石）、`(42,-267,-186)`（地牢）
  已进 `structure-e2e` 默认集。
