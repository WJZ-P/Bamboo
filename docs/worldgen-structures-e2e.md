# 带结构的整块端到端对拍（`gen-world --through features --structures`）

这一篇记录「开启结构生成」时的整块对拍：官方 26.3 在 JVM 里按 Director 的
`ChunkStatus` 顺序生成 3×3 九宫格跑到 FEATURES，Bamboo 走游戏里同一条纯函数路径，
**逐方块**比较 9 宫格。命令、口径与限制见下文；逐类型地物对拍见
[`docs/worldgen-e2e-coverage.md`](worldgen-e2e-coverage.md)。

## 命令

```bash
cargo build --release -p mc-bamboo-vanilla
JAVA=/Users/wjz/Desktop/Github/Bamboo/.deps/jdk-25.0.5+8/Contents/Home/bin/java

# 60 个结构区块，干净跑（默认）
cargo run --release -q -p mc-bamboo-vanilla -- structure-e2e "$JAVA"

# 单个区块手动复现（--seed 是新增的诊断开关，固定种子）
cargo run --release -q -p mc-bamboo-vanilla -- gen-world "$JAVA" \
    --through features --structures --seed 42 --chunk 17 7 --show 8

# 改坏确认：跳过一种结构的所有结构件放置
cargo run --release -q -p mc-bamboo-vanilla -- structure-e2e "$JAVA" \
    --break-structure minecraft:jigsaw

# 已知差异区块（不进默认干净跑；目前为空）
cargo run --release -q -p mc-bamboo-vanilla -- structure-e2e "$JAVA" --known-gaps

# 沙漠水井候选区块（只算放置链前两步 + 沙漠群系，输出给 --chunks-file）
cargo run --release -q -p mc-bamboo-vanilla -- structure-e2e "$JAVA" --scan-desert-well 42 300

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
gen-world: 60 chunks, 0 quart cells, 53084160 blocks, 133 block entities,
           60 structure rows / 4743 official pieces, 0 mismatches, 53176461 official checks
```

每种结构实际放置的结构件数 / 方块数（`--structure-coverage` 的合计）：

| 结构类型 ID | 结构件数 | 方块写入数 | 代表区块（种子 42） |
|---|---:|---:|---|
| `minecraft:jigsaw`（村庄五种 / 前哨 / 古城 / 试炼密室 / 古迹废墟 / 废弃营地 / 沉船等） | 138 | 41783 | `(-383,-289)` 平原村庄、`(-366,-319)` 沙漠村庄、`(-393,-227)` 热带草原村庄、`(364,-29)` 雪原村庄、`(-351,378)` 针叶林村庄、`(-379,-303)` 前哨、`(-165,80)` 古城、`(17,7)` 试炼密室、`(-405,273)` 古迹废墟、`(-14,21)` 废弃营地 |
| `minecraft:mineshaft` | 115 | 10606 | `(4,0)`、`(0,-17)`、`(1,34,34)` 等 |
| `minecraft:ruined_portal` | 5 | 3036 | `(-19,11)`、`(16,22)`、`(19,-24)` |
| `minecraft:shipwreck` | 2 | 1172 | `(12,28)`、`(40,55)`（搁浅） |
| `minecraft:igloo` | 12 | 1014 | `(208,-305)` |
| `minecraft:desert_pyramid` | 2 | 5273 | 起点 `(-375,-22)` 与被引用邻居 `(-374,-22)` |
| `minecraft:buried_treasure` | 2 | 5 | `(-39,-14)`、`(-13,40)` |
| `minecraft:swamp_hut` | 1 | 153 | `(-17,22)` |
| `minecraft:woodland_mansion` | 65 | 15361 | `(-6380,-6052)`、`(-6380,-2374)` |
| `minecraft:ocean_monument` | 2 | 5846 | `(-12,-148)`、`(49,-11)` |
| `minecraft:jungle_temple` | 1 | 2302 | `(-85,180)` |
| `minecraft:ocean_ruin` | 6 | 166 | `(3,10)` 冷海、`(-32,-79)` 暖海、`(-50,-197)` 带 5 个刷扫方块实体 |

村庄 `FeaturePoolElement` 里五种 placed feature 的触发（以 `(-351,378)` 为例）：
`patch_taiga_grass`、`pine`、`spruce`、`pile_pumpkin`、`patch_berry_bush`。
`pine` / `spruce` / `pile_pumpkin` 的方块由 `FeaturePoolElement.place` 直接调用
`PlacedFeature.place` 写出；`block_pile` 是本轮新移植的叶子地物。

`structure-e2e` 的查询集还包含 `structure-pipeline` 原来的 28 个起点 / 引用区块
（含已修好的 `(42,-32,-16)`），补充的村庄五种 / 前哨 / 古城 / 古迹废墟 /
沙漠神殿 / 雪屋 / 要塞邻居 / 一个带刷扫方块的海底遗迹，以及按 `random_spread` 电位起点定位到的四种稀有结构
起点（林地府邸、海底神殿、丛林神殿、海底遗迹）。

## 改坏确认

| 改坏方式 | 结果 |
|---|---|
| `--break-structure minecraft:jigsaw` | 138 个干净结构件 → 41155 处不一致 |
| `--break-structure minecraft:mineshaft` | 115 个干净结构件 → 11412 处不一致 |
| `--break-structure minecraft:ruined_portal` | 4 个 → 650 处 |
| `--break-structure minecraft:swamp_hut` | 1 个 → 348 处 |
| `--break-structure minecraft:shipwreck` | 1 个 → 265 处 |
| `--break-structure minecraft:buried_treasure` | 2 个 → 5 处 |
| `block_pile::place` 多取一次 `next_float()` | 25 处不一致（还原后 0） |
| `long-set --break-self-test`（交换前两个输出） | 8 处不一致（还原后 3000 组 / 287248 次比较 0） |
| `structure-e2e --break-self-test` | 破坏第一个方块 → 52 处不一致（逐格比较器报错） |
| 方块实体比较器白名单改坏：把一个白名单键在游戏侧写成非默认值 | 117 处不一致（还原后 52 区块 0 不一致） |
| `place_suspicious_sand` 去掉 `set_block_entity` | 沙漠神殿起点 4 个刷扫方块实体报「game missing」 |
| 海底遗迹 `archyRuleProcessor` 的 `AppendLoot` 去掉 | `(-50,-197)` 的 5 个刷扫方块实体报「game missing」 |
| 把 `WorldgenRandom.next_long()` 换回内层 `Random::next_long()` | 20+ 处 `LootTableSeed` 不一致 |
| `PieceContext::place_block` 换回不带高度图的 `set_block` | 沙漠神殿起点 3 处 step-9 干草不一致 |

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
| 沙漠神殿起点整片沙 / 砂岩变体不同 | `DesertPyramidStructure.afterPlace`（可疑的沙子 + 塌陷屋顶）未实现 | `structure-e2e` 沙漠神殿区块 | 逐件对拍只跑 `postProcess`，`afterPlace` 只在起点区块的写区里生效 |
| 要塞 412 处石砖变体整片错位 | `dispatch_stronghold` 在放置阶段重新取 `randomSmallDoor`，官方只读布局阶段存好的 `EntryDoor` | `structure-e2e` 要塞区块 | `structure-place` 自建随机源，多取一次不影响它自己 |
| 林地府邸 / 海底神殿 / 丛林神殿 / 海底遗迹从不出起点 | `generate_start` 按 `type_id` 分派，四个分支写的是注册 ID（`mansion` / `monument` / `jungle_pyramid` / `ocean_ruin_*`） | `structure-e2e` 四种稀有结构起点 | 之前的扫描坐标里没有有效起点，误判为「群系门槛太严」 |
| 海底神殿起点 1770 处（完全不写方块） | `place_code_piece` 对 `monument` 直接返回，房间图 / 子件的 `place_in_chunk` 没接进 `place_start` | `structure-e2e` 的 `(-12,-148)` / `(49,-11)` | `structure-place` 单独覆盖过 `place_in_chunk`，但游戏路径没调它 |
| `sulfur_pool` 少放湿润的强效硫磺 | `lake::place` 对水湖返回 false，`SequenceFeature` 短路第二个子项；`PotentSulfurBlock` 未进 `survival::classify` | `gen-world --chunk -32 -16`、`structure-e2e` | 无结构模式也复现，之前的查询集把该坐标移除了 |

## 这一轮（第 2 部分）补查的三处不一致

| 现象 | 根因（字节码核对） | 修法 |
|---|---|---|
| 沙漠神殿起点 33 处（沙 / 砂岩变体） | `DesertPyramidStructure.afterPlace` 完全没实现：官方在所有件放完后用 `SortedArraySet` 收集 83 个 `placeSand` 位置，`Util.shuffle` + `nextIntBetweenInclusive(5,8)` 选可疑的沙子，其余写沙；只在起点区块生效 | 移植 `pyramid_after.rs`（含 `placeSuspiciousSand` 的 typed `getBlockEntity`、`getRandomCollapsedRoofPos` 的位置随机源）→ 33 → 3 |
| 要塞起点 412 处（苔石 / 裂石 / 虫蚀石砖整片错位） | `dispatch_stronghold` 在放置阶段重新调 `randomSmallDoor(random)`；官方在**结构件构造器**（布局阶段）取一次 `nextInt(5)` 存成 `EntryDoor`，`postProcess` 只读这个字段。多取一次让同一 `WorldgenRandom` 后续所有 `nextFloat` 错位 | 从 `piece.extras["EntryDoor"]` 读 → 412 → 4 |
| `(42,-32,-16)` 的 `potent_sulfur[wet]` 1 处（无结构也复现） | `sulfur_pool` 是 `sequence`：第一步水做的 `lake` 放硫磺屏障，第二步 `simple_block` 放湿润的强效硫磺。`LakeFeature.place` 对水湖返回 **true**（`SequenceFeature` 在第一个子项返回 false 时短路），我们的 `lake::place` 提前 `return false`；另外 `PotentSulfurBlock` 没进 `survival::classify` 的 `Kind::Default` | 两处都修 → 0 不一致，该坐标已加回默认查询集 |

33 → 3 的残差后来定位为 `StructurePiece.placeBlock` 漏了 FINAL 高度图更新
（step 9 的 `heightmap MOTION_BLOCKING` 读到旧高度，已修）；412 → 4 的残差当时怀疑是
栅栏的 `isFaceSturdy` 边界，实际是游戏路径漏了 `MineShaftPiece.canBeReplaced`
覆写（见「三处残差的根因」，已修）。

## 稀有结构起点定位

四种之前扫不出有效起点的结构，根因是 `create_structures` 的 `generate_start`
按 `def.type_id` 分派，但部分分支写的是**结构注册 ID**：
`mansion` / `monument` / `jungle_pyramid` / `ocean_ruin_*` 的 `type_id` 分别是
`woodland_mansion` / `ocean_monument` / `jungle_temple` / `ocean_ruin`，
于是整条落到 `_ => 无起点`。补上类型 ID 别名后，用官方 `random_spread` 的
`getPotentialStructureChunk` 逐格定位：12 个种子 × 4 个结构集 × 241×241 网格里
找到 193175 个有效起点，取离原点最近的加入查询集；海底神殿另外补上
`ocean_monument_place::place_in_chunk`（富模型按 seed + 起点区块重建房间图）。
覆盖行的「结构件数 / 清单」按官方 `PiecesContainer` 口径把房间图子件折回
`MonumentBuilding` 一件，子件由逐方块比较验证。

## 方块实体比较

`gen-world --through features` / `structure-e2e` 现在除了逐方块，还比较 3×3 每格
**实例化过**的方块实体：官方侧导出 `BlockEntity.saveWithFullMetadata` 的紧凑 SNBT
（跳过只是 `DUMMY` 占位的 pending nbt），Rust 侧用
`FeatureResult::block_entities`（`FeatureWorld::set_block_entity` 累计）逐位置比。
`id` / `x` / `y` / `z` / 空 `components` 是存盘元数据，比较时丢掉。官方构造器
无条件写出的默认值（`VaultBlockEntity` 的空 `server_data` / `shared_data`、
`AbstractFurnaceBlockEntity` 的 `speed_multiplier`、`SignBlockEntity` 的
`is_waxed`、`SculkSensorBlockEntity` 的监听器、酿造台的计时总量）按
`(方块实体类型, 键, 官方默认值)` 白名单忽略；只有官方值等于默认值且游戏侧
缺这个键才忽略，游戏侧写了不同的值照样报不一致。

60 个默认查询区块现在 `53084160 blocks / 133 block entities / 0 mismatches`。

这一轮加比较器后翻出并修掉的真问题：

| 现象 | 根因（字节码核对） |
|---|---|
| 20+ 处 `LootTableSeed` 整组移位 / 低位小偏移 | `StructureTemplate.placeInWorld` 用的是 `StructureStart.placeInChunk` 传下来的 `WorldgenRandom`（`BitRandomSource.nextLong` = 两次 `next(32)`），不是内层 Xoroshiro 的原生 `nextLong` |
| 模板里的方块实体完全没进 `FeatureResult` | `place_in_world` / `place_processed` 只写方块，`BlockAccess` 和 `PieceLevel` 没有 nbt 钩子 |
| 古城箱子多一个 `LootTableSeed` | `RandomizableContainer.tryLoadLootTable` 在没有 `LootTable` 时清掉整份状态，`trySaveLootTable` 不会单独存种子 |
| 宝库 `config.loot_table` 官方没有 | `VaultConfig.CODEC.encode` 对等于默认 `TRIAL_CHAMBERS_REWARD` 的表省略该键 |
| 熔炉 / 酿造台计时字段类型、告示牌文本 | 模板 nbt 用 short / byte / 组件对象，`saveAdditional` 用 `putInt`、`SignText.CODEC` 存纯字符串 |
| 矿井 / 府邸刷怪笼、雪屋箱子缺实体 | 放置时没有 `setBlockEntity` 对应的记录 |

可疑的沙子相关修复：`DesertPyramidStructure.afterPlace` 现在给刷扫方块实体设
`minecraft:archaeology/desert_pyramid` 与 `pos.asLong()` 作种子（字节码里 lambda
的参数，不是随机数）；`OceanRuinPieces.archyRuleProcessor` 的 `AppendLoot`
（`ocean_ruin_warm` / `ocean_ruin_cold`）、海底遗迹箱子的 `UNDERWATER_RUIN_*` 表
和 `desert_well` 模板链的 `AppendLoot`（`desert_well`）也照
`RuleProcessor.processBlock` 的按位置随机源写表 + 种子。`structure-e2e` 默认集
新增 `(42,-50,-197)`：海底遗迹真的放下 5 个可疑的沙砾 + 1 个箱子，方块实体
逐项 0 不一致。为了让官方假世界能跑完海底遗迹的 `drowned` 数据标记，
`ChunkgenOracle` 的惰性实体桩补了 `InertDrowned`。

`--chunks-file FILE` 可以用外部 `seed cx cz` 文件代替内置查询集，方便定位新结构
区块；默认不用。

## 三处残差的根因（`wg-struct-residual`）

每处都先在官方侧定位「哪个结构件、哪个方法、哪一次调用」写的：`ChunkgenOracle.java`
新增诊断开关 `-Dbamboo.debug.trace-pos=x,y,z;...`（`-Dbamboo.debug.trace-pos.file=`），
`FeatureLevelHandler.setBlock` 对这些位置记下新状态与调用栈。用法：
`JAVA_TOOL_OPTIONS="-Dbamboo.debug.trace-pos=-1922,8,1548" mc-bamboo-vanilla structure-e2e ...`。

| 现象 | 官方写入来源 | 根因（字节码核对） | 改坏确认 |
|---|---|---|---|
| 要塞起点 `(42,-121,96)` 4 处（官方 `oak_log`，游戏 `oak_fence` / `oak_planks`） | 四格都由 `MineShaftCorridor.postProcess` → `placeDoubleLowerOrUpperSupport` → `fillPillarDownOrChainUp` → `fillColumnBetween` 写成 `oak_log`，**之后**再也没有被改写；游戏侧同一走廊先写 `oak_log`，后面另一条走廊的 `generateBox`（空气 / 栅栏）又把它覆盖 | `StructurePiece` 子类里只有 `MineShaftPiece` 覆写 `canBeReplaced`：本类型的木板 / 原木 / 栅栏 / 铁链不可替换，`placeBlock`（`generateBox` 等都经过它）跳过这些格。`structure-place` 的旧对拍注入了覆写，游戏路径 `place_code_piece` 没注入。和栅栏 `isFaceSturdy` 无关 | 去掉覆写即原 4 处不一致 |
| 海底遗迹 `(42,67,-236)`：官方 6 个刷扫方块实体，游戏在另外 7 个位置写了表 | 三个遗迹件互相覆盖：前一件 `placeInWorld` 写了可疑的沙砾 + 战利品表，后一件把同一格写成普通沙砾 / 深色橡木板 | 不是 `CappedProcessor`：`WorldGenRegion.setBlock` 在新状态没有方块实体、旧状态有时调 `ChunkAccess.removeBlockEntity`，我们的 `FeatureWorld` 不删记录。修掉后又暴露 `place_layout_piece` 对已经直接写进世界的 `outcome.writes` 再重放一遍，重放里的 `BARRIER` 把刚写的表删掉（第二次的 6 处「game missing」） | 去掉删除逻辑即原 7 处；保留重放即 11 处 |
| 沙漠水井找不到、`FossilFeature` 抛异常 | — | 见 `docs/worldgen-e2e-coverage.md`「沙漠水井 / 化石」：假世界 `getLevel()` / 模板管理器缺失；放出来以后修了模板地物的调色板取数、写入顺序、`AppendLoot` 种子坐标、化石高度图、地牢方块实体 | 调色板不取数：水井 70 处 / 化石 16 处；种子用相对坐标：44 处 |

扩大扫描又翻出并修掉的两个问题：

| 现象 | 根因 | 改坏确认 |
|---|---|---|
| 深暗之域里的矿井件官方不放、游戏照放 | `MineShaftPiece.isInInvalidLocation` 第 136-152 行先查 `level.getBiome(probe).is(MINESHAFT_BLOCKING)`；游戏路径把 `biome_blocking` 写死成 false。现在按 `BiomeManager.getBiome`（fiddled 角 + 3×3 quart 格）查 | 写死 false：`(42,-145,76)` 等 4 区块 1678 处 |
| 针叶林村庄 `(42,-159,84)` 1 处台阶（官方 `half=bottom,waterlogged=false`） | 拼图 `final_state` 是 `cobblestone_stairs[facing=south]`；`BlockStateParser.parseForBlock` 从 `defaultBlockState()` 起逐个 `setValue`，`state_id_of` 却在状态表里取第一个匹配的状态 | 单测 `partial_properties_start_from_the_default_state` + 该区块进默认集 |

`structure-e2e --known-gaps` 现在为空；上面的区块都已进默认集。

## 已知限制

* 生成期实体（村民、悦灵、守卫者、女巫 / 猫）不进假世界；只保证它们的
  `nextLong` / 方块写入序列与官方一致，不比较实体本身。
* 官方构造器默认值按白名单忽略。Bamboo 运行时读盘再存盘的情况由
  `tools/mc-bamboo-vanilla/tests/block_entity_resave.rs` 记录：熔炉写出的键与
  `AbstractFurnaceBlockEntity.saveAdditional` 一致（七个键，含 `speed_multiplier`）；
  宝库、幽匿传感器 / 校准幽匿传感器在 Bamboo 里没有实例，条目原样保留——只有 `id`
  和坐标的条目存盘后仍缺 `config` / `shared_data` / `server_data` 或
  `last_vibration_frequency` / `listener`，官方会补出这些键；熔炉条目里官方不认识的
  旧键（如 `BurnTime`）Bamboo 合并时保留，官方会丢掉。本轮只记录不修。
