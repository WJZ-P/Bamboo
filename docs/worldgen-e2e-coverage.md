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

## 已确认的类型（26 个，全部 PASS）

| Feature 类型 | step | 种子 / 区块 | 触发次数 | 干净跑 | 改坏格数 | 改坏不一致 | 结果 |
|---|---:|---|---:|---|---:|---:|---|
| `minecraft:lake` | 1,2 | `--seeds 3 --chunks 8 --salt 7` | 2 | 0 | 2 | 2 | PASS |
| `minecraft:geode` | 2 | 同上 | 1 | 0 | 1 | 1 | PASS |
| `minecraft:iceberg` | 2 | `--seeds 5 --chunk 146 -50 --scan 9` | 1 | 0 | 1 | 1 | PASS |
| `minecraft:large_dripstone` | 2 | `--seeds 1 --chunk -53 -206 --scan 5` | 50 | 0 | 3 | 3 | PASS |
| `minecraft:block_blob` | 2 | `--seeds 2 --chunk 83 -274 --scan 9` | 10 | 0 | 6 | 6 | PASS |
| `minecraft:monster_room` | 3 | `--seeds 1 --chunk 3 -1 --scan 5` | 126 | 0 | 1 | 1 | PASS |
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

## 已知限制（3 个类型，单次扫描不超过 10 分钟）

这三个类型在下面的扫描里触发次数都是 0，所以**没有改坏确认证据**，不能记作通过。
触发条件都按 `javap -c -p` 字节码核对过。

| Feature 类型 | step | 试过的种子 / 区块 | 触发次数 | 字节码核对的触发条件 |
|---|---:|---|---:|---|
| `minecraft:overlay` | 4 | `--seeds 5 --chunks 20 --salt 13`（100 中心）；`--seeds 5 --chunk -15 -274 --scan 100`（605 中心，沙漠） | 0 | `minecraft:desert_well` 是 `rarity_filter chance=1000`（`nextFloat() < 1/1000`），后面还要过 `heightmap MOTION_BLOCKING`、`offset y=-1`、`all_of{matching_blocks(sand), not(volume_match(±2,-2..-1,±2, air))}` 和 `biome`。`OverlayFeature.place` 只是在子 `PlacedFeature` 上再跑一遍 `FeaturePlacer.place`。 rarity 与地形谓词连乘，命中率极低。 |
| `minecraft:fossil` | 3 | `--seeds 5 --chunks 20 --salt 13`（100 中心）；`--seeds 1 --chunk -15 -274 --scan 9` | 0 | `FossilFeature.place` 第一句是 `level.getLevel().getServer().getStructureTemplateManager()`；`ChunkgenOracle` 的 `WorldGenLevel` 动态代理的 `getLevel()` 返回自己，强转 `ServerLevel` 会抛 `ClassCastException`。即便找到命中点，JVM 侧也放不出模板方块，属于**对拍框架不可比**，见 `docs/worldgen-features-plan.md` 5.3。 |
| `minecraft:root_system` | 1,9 | `--seeds 5 --chunks 20 --salt 7`（100 中心）；`--seeds 1 --chunk -300 -275 --scan 25` | 0 | `RootSystemFeature.place` 先要求 `origin` 是空气；`placeDirtAndTree` 第一轮把 `workingPos` 上移一格，然后要求 `workingPos.below()`（**正好就是 origin**）`isSolid()`，不满足直接 `return false`。后面的循环轮次永远到不了。这是官方 26.3 自身的死代码，见 `docs/worldgen-features-plan.md` 第十六轮核对。 |

`overlay` 的 605 中心扫描耗时约 2 分 20 秒，按主代理要求单次不超过 10 分钟就停；
不再扩大扫描，留作已知限制。`fossil` 和 `root_system` 是机制性不可触发，不是扫描量不够。

## 复跑

```bash
cargo build --release -p mc-bamboo-vanilla
tools/mc-bamboo-vanilla/feature-e2e.sh               # 全部 26 个类型
tools/mc-bamboo-vanilla/feature-e2e.sh minecraft:ore # 只跑一个类型
```

脚本逐类型跑「干净对拍 + 改坏确认」，打印和本表同口径的统计，任一失败就以非 0 退出。
新加地物时把类型、step 掩码和能触发它的种子/区块加进脚本的 `CASES` 表，再按
`docs/worldgen.md` 的步骤更新本文。

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
