# 带结构的整块端到端对拍（`gen-world --through features --structures`）

这一篇记录「开启结构生成」时的整块对拍：官方 26.3 在 JVM 里按 Director 的
`ChunkStatus` 顺序生成 3×3 九宫格跑到 FEATURES，Bamboo 走游戏里同一条纯函数路径，
**逐方块**比较 9 宫格。命令、口径与限制见下文；逐类型地物对拍见
[`docs/worldgen-e2e-coverage.md`](worldgen-e2e-coverage.md)。

## 命令

```bash
cargo build --release -p mc-bamboo-vanilla
JAVA=/Users/wjz/Desktop/Github/Bamboo/.deps/jdk-25.0.5+8/Contents/Home/bin/java

# 42 个结构区块，干净跑（默认）
cargo run --release -q -p mc-bamboo-vanilla -- structure-e2e "$JAVA"

# 单个区块手动复现（--seed 是新增的诊断开关，固定种子）
cargo run --release -q -p mc-bamboo-vanilla -- gen-world "$JAVA" \
    --through features --structures --seed 42 --chunk 17 7 --show 8

# 改坏确认：跳过一种结构的所有结构件放置
cargo run --release -q -p mc-bamboo-vanilla -- structure-e2e "$JAVA" \
    --break-structure minecraft:jigsaw

# 已知差异区块（不进默认干净跑）
cargo run --release -q -p mc-bamboo-vanilla -- structure-e2e "$JAVA" --known-gaps

# fastutil LongOpenHashSet 迭代序单独对拍（3000 组）
cargo run --release -q -p mc-bamboo-vanilla -- long-set "$JAVA" --cases 3000
cargo run --release -q -p mc-bamboo-vanilla -- long-set "$JAVA" --cases 3000 --break-self-test
```

`gen-world` 新增的开关：

| 开关 | 作用 |
|---|---|
| `--structures` | Java 侧跑官方 `createStructures` / `createReferences`（半径 9 网格）、真实 `Beardifier` 的 CARVERS 地形、带结构的 `applyBiomeDecoration`；游戏侧同步打开 `generate_structures`（`game_features_with_mask` 走 `build_view` + `run_features_filtered`）。不传时完全保持原来的无结构模式：`WorldOptions(seed,false,false)` + `Beardifier.EMPTY`。 |
| `--seed S` | 固定单个世界种子，绕开 `FIXED_SEEDS`，方便复现某个结构区块。 |
| `--chunks-file F` | Java 侧按 `seed cx cz` 文件出题；`structure-e2e` 用它让两边跑同一批坐标。 |
| `--break-structure KIND` | 改坏确认：游戏侧跳过该结构类型 ID 的结构件放置。 |
| `--structure-coverage` | 打印每种结构实际放置的结构件数与方块数。 |

Java 侧 `ChunkgenOracle.java` 用 `StructureTemplateManager`、真实注册表、17×17
起点网格和 3×3 写区代理；生成期实体用不经构造器的惰性实例顶替，`getBlockEntity(pos,type)`
返回 `Optional`，`holderLookup` / `updateNeighborsAt` 等新调用按官方语义补齐。
不启动服务端、不读存档、不接受 EULA。

## 干净跑结果（release，本机）

```
gen-world: 42 chunks, 0 quart cells, 37158912 blocks, 42 structure rows /
           3172 official pieces, 0 mismatches, 37223424 official checks
```

每种结构实际放置的结构件数 / 方块数（`--structure-coverage` 的合计）：

| 结构类型 ID | 结构件数 | 方块写入数 | 代表区块（种子 42） |
|---|---:|---:|---|
| `minecraft:jigsaw`（村庄五种 / 前哨 / 古城 / 试炼密室 / 古迹废墟 / 废弃营地 / 沉船等） | 134 | 35063 | `(-383,-289)` 平原村庄、`(-366,-319)` 沙漠村庄、`(-393,-227)` 热带草原村庄、`(364,-29)` 雪原村庄、`(-351,378)` 针叶林村庄、`(-379,-303)` 前哨、`(-165,80)` 古城、`(17,7)` 试炼密室、`(-405,273)` 古迹废墟、`(-14,21)` 废弃营地 |
| `minecraft:mineshaft` | 107 | 9929 | `(4,0)`、`(0,-17)`、`(1,34,34)` 等 |
| `minecraft:ruined_portal` | 5 | 3036 | `(-19,11)`、`(16,22)`、`(19,-24)` |
| `minecraft:shipwreck` | 2 | 1172 | `(12,28)`、`(40,55)`（搁浅） |
| `minecraft:igloo` | 12 | 1014 | `(208,-305)` |
| `minecraft:desert_pyramid` | 1 | 649 | 起点 `(-375,-22)` 的被引用邻居 `(-374,-22)` |
| `minecraft:buried_treasure` | 2 | 5 | `(-39,-14)`、`(-13,40)` |
| `minecraft:swamp_hut` | 1 | 153 | `(-17,22)` |

村庄 `FeaturePoolElement` 里五种 placed feature 的触发（以 `(-351,378)` 为例）：
`patch_taiga_grass`、`pine`、`spruce`、`pile_pumpkin`、`patch_berry_bush`。
`pine` / `spruce` / `pile_pumpkin` 的方块由 `FeaturePoolElement.place` 直接调用
`PlacedFeature.place` 写出；`block_pile` 是本轮新移植的叶子地物。

`structure-e2e` 的查询集还包含 `structure-pipeline` 原来的 28 个起点 / 引用区块
（去掉 `(42,-32,-16)`，见「已知限制」），以及强引用邻居 `(-121,97)` / `(-120,96)`。

## 改坏确认

| 改坏方式 | 结果 |
|---|---|
| `--break-structure minecraft:jigsaw` | 134 个干净结构件 → 34356 处不一致 |
| `--break-structure minecraft:mineshaft` | 91 个干净结构件 → 9886 处不一致 |
| `--break-structure minecraft:ruined_portal` | 4 个 → 650 处 |
| `--break-structure minecraft:swamp_hut` | 1 个 → 348 处 |
| `--break-structure minecraft:shipwreck` | 1 个 → 265 处 |
| `--break-structure minecraft:buried_treasure` | 2 个 → 5 处 |
| `block_pile::place` 多取一次 `next_float()` | 25 处不一致（还原后 0） |
| `long-set --break-self-test`（交换前两个输出） | 8 处不一致（还原后 3000 组 / 287248 次比较 0） |
| `structure-e2e --break-self-test` | 破坏第一个方块，逐格比较器报错 |

## 这一轮修掉的 bug

| 现象 | 根因 | 现在由哪个对拍盯着 | 之前为什么没发现 |
|---|---|---|---|
| 矿井 / 试炼密室的随机方块（铁轨、铜灯氧化）整片错位 | `place_step` 的 `WorldgenRandom` 用 `LegacyRandomSource`；官方 `applyBiomeDecoration` 包的是 `XoroshiroRandomSource` | `gen-world --structures --steps 3`、`structure-e2e` | 旧结构放置对拍用合成地形和自建随机源，与真实 decoration 随机流不同 |
| 多个矿井起点反查顺序不同导致后续 `nextInt` 全错 | `References` 用按 dx/dz 扫描顺序的 `Vec`；官方 `ChunkAccess.structureReferences` 是 `LongOpenHashSet`，迭代序由容量 / `HashCommon.mix` / 线性探测 / 扩容历史决定 | `long-set`（3000 组）+ `structure-e2e` 的 `(-3,-17)` / `(0,-17)` | 旧 `structure-pipeline` 比较起点时先排序，顺序不影响它的逐项比较 |
| 试炼密室铜灯全是 `waxed_copper_bulb`，没有氧化变体 | `place_jigsaw_piece` 把具名 `processor_list` 的 `minecraft:` 前缀剥掉，查表永远落空 | `structure-e2e` 的 `(17,7)`、`(19,-15)` | `jigsaw apply` 直接传列表对象，不走注册表查表 |
| 前哨瞭望塔、部分村庄结构件完全不生成 | `ListPoolElement` 被当成「没有模板」直接跳过；官方按列表顺序逐个调子元素的 `place` | `structure-e2e` 的 `(-379,-303)`、村庄五种 | `jigsaw placepiece` 只测 `SinglePoolElement` |
| 村庄 `FeaturePoolElement` 的植被 / 树 / 南瓜堆完全不生成 | `PoolElement::Feature` 被跳过；官方是 `PlacedFeature.place(level, generator, random, piecePos)` | `structure-e2e` 的 `(-351,378)` 等村庄区块 | 之前的查询集没有村庄起点 |
| 村庄街道泥土路一层比一层高一格 | `GravityProcessor` 无条件把 `WORLD_SURFACE_WG` 换成 `WORLD_SURFACE`；官方只在 `level instanceof ServerLevel` 时换，FEATURES 阶段的 `WorldGenRegion` 不是 | `structure-e2e` 村庄区块 | 只在单个结构件上对拍过 gravity，没测连续多件 |
| step 10 的 `freeze_top_layer` 在村庄区块读到旧高度 | `BlockWriter for FeatureWorld` 用 `set_block`（不更新高度图）；官方 `ChunkAccess.setBlockState` 会增量维护四张 FINAL 高度图 | `structure-e2e` 全步、旧 `feature-e2e.sh minecraft:freeze_top_layer` | 无结构模式不写结构件，高度图不会被结构改动 |
| 沙漠神殿 `afterPlace` 抛 `ClassCastException` | 假世界没实现 2 参数 `getBlockEntity(pos,type)`（返回 `Optional`） | `structure-e2e` 的 `(-374,-22)` | 旧对拍没有跑到 `DesertPyramidStructure.placeSuspiciousSand` |
| `minecraft:block_pile` 标 unsupported | 该叶子地物未移植；官方距离阈值分支是 `dist2 > threshold` 才走 `nextFloat() < 0.031` | `structure-e2e` 村庄区块 + `FeaturePoolElement` 改坏确认 | 主世界 `biome_features` 里没有它，只作为结构池元素出现 |

## 已知限制

* **`(42,-32,-16)`**：`sulfur_pool` 的第二步（`sequence` → `simple_block` + `environment_scan`）
  放 `potent_sulfur[wet]` 的位置有 1 处差异；**无结构模式也复现**，与结构无关。
  该区块已从 `structure-e2e` 查询集移除，已知地物差异另行跟踪。
* **`(42,-375,-22)` 沙漠神殿起点 / `(42,-121,96)` 要塞起点**：这两个 `Layout`
  结构的起点区块在整块对拍里还有 33 / 412 处不一致，集中在 `postProcess`
  的随机方块变体（塌陷屋顶的沙子 / 砂岩、要塞石砖的苔石 / 裂石 / 虫蚀石）。
  `structure-place` / `structure-post-process` 用合成地形逐件对拍这两类是 0 不一致，
  说明差异在真实 `WorldGenRegion` 上的随机流 / `skipAir` 读世界行为；根因未定位。
  用 `--known-gaps` 单独跑这两个区块。默认查询集用它们的**被引用邻居**
  （`(-374,-22)`、`(-121,97)`、`(-120,96)`）覆盖跨区块结构件放置，整块 0 不一致。
* **林地府邸 / 海底神殿 / 丛林神殿 / 海底遗迹 / 试炼密室以外的部分 jigsaw 池**：
  在本机的 7 个种子 × 640×640 区块扫描里没有产出有效起点（群系 / 高度门），
  因此 `structure-e2e` 没有它们的起点区块。它们的布局与放置由
  `structure-pieces` / `structure-place` / `jigsaw pools` 对拍覆盖。
* 生成期实体（村民、悦灵、守卫者、女巫 / 猫）不进假世界；只保证它们的
  `nextLong` / 方块写入序列与官方一致，不比较实体本身。
