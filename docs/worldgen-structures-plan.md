# 世界生成：结构（structures）实施方案

目标版本 Minecraft Java 26.3。本文是 `structure_starts` / `structure_references` 两个阶段、
`features` 阶段里结构件的放置、terrain 阶段的 `Beardifier`、以及结构存档的整体方案。
本轮只做主世界（下界、末地没有维度）。所有行为按官方反编译源码
（`mc-src`，`net.minecraft.world.level.levelgen.structure.*`）和官方 jar 里的
`data/minecraft/worldgen/*.json` 核对，不凭印象；类名、方法名、字段名在下面逐处给出，
移植时逐个读、用 Rust 重写逻辑，绝不抄 Java 源码进仓库。

和 `docs/worldgen.md`（terrain）、`docs/worldgen-features-plan.md`（地物）并列。
地物 A/B、流水线、结构四个队员共用 `feature/` 主循环、`chunkgen.rs` / `ChunkgenOracle.java`；
本文尽量把新代码放进新文件、对共用文件只做小而集中的改动。

## 0. 现状

**本分支起点（2026-11-13）**：

* 世界生成的结构部分**完全没做**。terrain 阶段的 `NoiseChunk` 用 `Beardifier.EMPTY`
  （`docs/worldgen.md`「接进游戏时要重新对拍的两处」）：新区块没有结构，`affectedBox` 为
  null，整块填 0；这正好是「没有结构影响密度」的基线，`terrain` 对拍已经 0 不一致。
* `features` 阶段的 `StructureManager.shouldGenerateStructures()` 被当成 false，
  `applyBiomeDecoration` 里「按 step 分组放结构件」那整段被跳过
  （`docs/worldgen-features-plan.md` 1.1 第 2 条、3.2）。
* 流水线队员会把 `structure_starts` / `structure_references` 两个阶段先接成空实现，
  由队长在合并时接上本文第 8 节写明的纯函数接口。
* 主世界 `minecraft:overworld` 的群系源、气候采样、`BiomeResolver`、
  `WorldgenRandom`（含 `LegacyRandomSource` 包装）在 `mc-bamboo-worldgen` 里已有，
  结构放置的随机源能直接复用。

**本轮已落地（2026-11-13，步骤 1–3）**：

* 方案文档（本文，步 1）。
* 数据导出（步 2）：`Extractor.java` 的 `worldgen()` 用 `encodeRegistry` 加四个键
  （`structure_set` 21、`structure` 52、`template_pool` 245、`processor_list` 40 条），
  经 `extract --no-default-features` 写进 `worldgen.json`，重跑无 diff；
  `Worldgen::parse` 把四个键列为必需字段。
* 解析骨架（步 2）：`mc-bamboo-worldgen::structure::registry` 解析 `structure_set` /
  `structure` 里放置阶段要用的字段，用 `mc-bamboo-data` 的 `worldgen/biome` 标签表展开
  `HolderSet<Biome>`，并复刻 `createForNormal` 的 `possibleStructureSets` 过滤。
  `template_pool` / `processor_list` 只校验为对象，完整解析留到结构件里程碑。
* 放置纯函数（步 3）：`structure/placement.rs` 移植 `random_spread` 的
  `getPotentialStructureChunk`、两种 `spread_type`、四种 `FrequencyReductionMethod`、
  `ExclusionZone`，以及 `concentric_rings` 的确定性环几何（群系吸附待办）；
  `structure::StructureState` 组合出 `isStructureChunk` 和 `start_candidates`。
* 对拍（步 3）：`cargo run -p mc-bamboo-vanilla -- structures <java>` 用官方
  `RandomSpreadStructurePlacement` 对 20 个 random_spread 结构集、464 个 (种子, 区块) 查询
  逐格比潜在起点和频率判定，9280 个比较 0 不一致（其中 1296 条频率判定为 false，确认
  频率削减分支真的被比到）。故意把 `floorDiv` 换成截断除法，对拍立刻报 312 处不一致并
  失败，还原后恢复 0；旧 `chunkgen --through carvers` 仍是 0 不一致。

**第二轮（2026-11-13，队长分派的步 1–2）**：

* 结构选择与起点生成（步 1，`structure/start.rs`）：`StructureStart` / `StructurePiece` /
  `BoundingBox` / `StructureStarts` / `References` / `StartPlan` 数据类型；
  `createStructures` 的加权选择（`setLargeFeatureSeed` + `nextInt(total)` + 移除重试）、
  `existingStart` 短路、`createReferences` 的 17×17 邻域 + 包围盒 XZ 相交 +
  `ChunkPos.pack`。`StructureState::start_plans` 产出每个命中结构集的尝试序列。
  `structures` 对拍扩出一节官方 `WorldgenRandom` 加权选择序列：21 个结构集 × 464 查询 =
  9744 个比较，和放置合计 0 不一致（故意把 `setLargeFeatureSeed` 的 x/z 对调，报 1987
  处不一致并失败）。**`Structure.generate` 的 `findValidGenerationPoint`（群系 + 高度检查）
  没做**，所以没有「起点是否有效 / 最终选中哪个结构」，只有尝试序列。
* 要塞 `concentric_rings`（步 2，`structure/rings.rs`）：`BiomeSource.findBiomeHorizontal`
  的单点气候群系解析器、蓄水池抽样、精确搜索顺序，和 `generateRingPositions` 的主
  `LegacyRandomSource`、`fork()` 消费 `nextLong()`、`Math.round`、角度/距离/spread 更新。
  新子命令 `cargo run -p mc-bamboo-vanilla -- structure-rings <java>` 用官方
  `getRingPositionsFor` / `findBiomeHorizontal` / `isStructureChunk` 对拍：6 个种子、768 个
  环位置、96 个 `findBiomeHorizontal`、864 个 `isStructureChunk` 检查全 0 不一致。
  故意丢掉 `fork()` 的 `nextLong()`，环位置立刻大面积不一致。`StructureState` 现在可以
  注入环位置，要塞的 `isStructureChunk` 不再是恒 false 的占位实现。
* 每一步都单独提交，每个新对拍都先故意改坏一处确认会失败再还原。

**第三轮（2026-11-13，接手后步 1：`findValidGenerationPoint` 通用判定）**：

* `Structure.generate` → `findValidGenerationPoint` 里与结构件无关的三件判定移植到新文件
  `structure/validity.rs`；`chunk.rs::Generator` 上只加两个原语：
  `first_occupied_height`（照 `iterateNoiseColumn` 用 1×height×1、步长 1 的体积和全新
  含水层，自顶向下找首个非空气方块）和 `column_biome_names`（照
  `MultiNoiseBiomeSource.createResolverForChunk` 用 1×N×1、步长 4 的体积，成批采六条
  气候函数再查群系参数表）。`validity.rs` 组合出
  `couldValidBiomeExistOnTopOfChunkCenter`（整列 quart 群系）、`corner_heights` /
  `lowest_y` / `mean_first_occupied_height`（四角高度）、`isValidBiome`（起点单点群系），
  以及 `find_valid_generation_point`（`onTopOfChunkCenter` 基线路径的起点 + 有效性）和
  `select_start`（`StartPlan` + 基线判定的纯组合）。
* 新子命令 `cargo run -p mc-bamboo-vanilla -- structure-validity <java>`：Java 侧
  `StructureValidityOracle.java` 造官方 `Structure.GenerationContext` 直接调
  `couldValidBiomeExistOnTopOfChunkCenter()`、`getFirstOccupiedHeight(...)`、
  `isValidBiome(stub)`，并按 `createStructures` 的加权序列组合出「起点 + 选中的结构 +
  是否有效」；Rust 侧用生产代码复算。6 种子 × 12 区块，52 个结构合法群系名 + 360 个
  高度字段（38 个不同值）+ 1584 个整列 / 起点判定字段 + 432 个加权序列 + 288 个组合字段
  = 2716 个比较 0 不一致。两处故意改坏（反转整列判定、反转起点群系判定）各失败，
  还原后回 0；旧 `structures` / `structure-rings` 对拍仍 0 不一致。
* 每一步单独提交；整列判定与起点单点判定在这批数据上从未不一致（主世界群系沿 Y 基本
  恒定），两条路径各自用故意改坏单独验过。

**第四轮（2026-11-14，`cb-wg-jigsaw` 分支：结构模板导出）**：

* 从官方 jar 原样导出 1511 个 `.nbt`（`structures.bin` + 索引，见 2.2），重跑无 diff；
  `mc-bamboo-data::structures` 暴露字节，`mc-bamboo-worldgen::structure::template`
  用 `mc-bamboo-nbt` 解析方块 / 方块实体 / 拼图方块（单元测试解析全部 1511 个模板）。
* 新对拍 `structure-templates`：官方 `NbtIo` 逐节点比整棵 NBT 树、校验导出字节等于 jar
  原文、比字段计数，1511 个模板 0 不一致；故意改坏一处 1511 个全失败，还原回 0。
* **jigsaw 结构件生成（`JigsawPlacement.addPieces`、五种 pool element、村庄 5 种 /
  掠夺者前哨站 / 古城 / 试炼密室 / 古迹废墟的结构件列表对拍）没做**：官方对拍需要构造
  带 `StructureTemplateManager` 的 `Structure.GenerationContext`（前面队员的 oracle 该参数
  传 `null`），Rust 侧要精确复刻 `Shapes` 的 AABB 布尔运算、`getShuffledJigsawBlocks` 的
  洗牌顺序与 `SequencedPriorityIterator`，本次会话未能达到逐项 0 不一致，见第 10 节。

**第五轮（2026-11-14，`cb-wg-structures` 分支：单件布局 / Beardifier / 存档）**：

* 合并了 `main` 及队友分支 `cb-fix-main` / `cb-wg-jigsaw` / `cb-wg-multipiece` /
  `cb-wg-pipeline` / `cb-wg-paths`（模板导出、多件布局、流水线、真实路径、高度图均在列）。
* `structure::single`：四个代码生成单件结构（沙漠神殿 / 丛林神殿 / 女巫小屋 / 埋藏宝藏）
  的结构件布局，复用 `structure::pieces` 的 `Piece` 模型。新对拍 `structure-single`
  （`java/StructureSingleOracle.java`）：8 种子 × 12 区块 × 4 类 = 384 查询、384 结构件、
  384 行比较 0 不一致，四个朝向与两种宽深交换形状都覆盖；反转 `axis_z`、改用反方向
  两处故意改坏分别报 192 / 288 处不一致，还原回 0。
* 这一轮在 `structure::beardifier` 里也写过一份 `Beardifier`；合并进 main 时去掉了，
  统一用 `cb-wg-beard` 的 `mc-bamboo-worldgen::beardifier`（见上面 Beardifier 一段和第 5 节）。
* `structure::save`：区块 NBT `structures` 键（`starts` 的 `StructureStart.createTag` /
  `PiecesContainer.save` / `StructurePiece.createTag`、`References` 的 LongArray），
  新对拍 `structure-save`（`java/StructureSaveOracle.java`）：8 case 逐项比规范 NBT
  0 不一致，官方 `PiecesContainer.load` / `save` 往返能读 `Children`，`world-dir` 的
  `SerializableChunkData.parse` 能读带该键的 4 个区块；反转 `BB` 的 int 顺序、把
  `References` 键改小写两处故意改坏各报 8 处不一致，还原回 0。
* **仍没做**：需要模板的单件结构（雪屋 / 沉船 / 海底遗迹 / 废弃传送门）、`features`
  阶段的 `placeInChunk` / `postProcess` / processors / 方块实体 / 拼图替换、把结构起点
  真正接进 Director。模板结构件的 Java 对拍要构造带 `StructureTemplateManager` 的
  `GenerationContext`，且沉船 / 废弃传送门的包围盒依赖 `getLowestY` /
  `getMeanFirstOccupiedHeight`（terrain 高度），`features` 放置要逐方块比 `postProcess`，
  两者都是独立的大块工作，本轮未达到 0 不一致，见第 10 节。

**仍是空实现 / 没接进游戏**：`structure_starts` / `structure_references` 两个阶段、
结构件放置（`features` 阶段）仍没做；结构件布局、`Beardifier`、存档 `structures` 键
已作为纯函数 + oracle 落地，但都没接进 Director，游戏里世界仍是超平坦。`findValidGenerationPoint` 的通用群系 / 高度判定已做（见上），但这里的「是否有效」
是 `onTopOfChunkCenter` 基线路径的通用判定，**不等于** `Structure.generate` 的最终有效：
各结构的 `findGenerationPoint`、包围盒、`.nbt` 模板、jigsaw 仍属第 4 步。组合对拍只覆盖
6 个 `random_spread` 结构集、不含 `isStructureChunk` 命中过滤（那是已对拍的 `start_plans`
的职责）。`.nbt` 结构模板按约定留到第一个真正用到它的结构那一步再导出。
（本节是第四轮结束时的快照；第五轮已把四个代码生成多件式结构的结构件布局、
`findGenerationPoint` 前置判定作为纯函数做完，模板也已在 2.2 导出，见下方第五轮。）

**多件式结构的结构件布局（2026-11-14，分支 `cb-wg-multipiece`，基于 4074b37）**：

* 新增 `mc-bamboo-worldgen::structure::pieces`，把四个「用代码生成结构件列表」的多件式
  结构中的**三个**布局移植成纯函数（结构件的类型、包围盒、朝向、genDepth、各子类参数，
  以及世界随机源取数次数和顺序逐步照官方翻译，不含 `postProcess`）：
  * 废弃矿井（`MineshaftStructure` / `MineshaftPieces`，普通 + 恶地两种）：起始房间、
    递归 `addChildren`、`moveBelowSeaLevel` / 恶地 `getBaseHeight` 竖直平移。恶地的
    `surfaceHeight` 由外部喂入（官方取自 terrain 的 `getBaseHeight`）。
  * 要塞（`StrongholdStructure` / `StrongholdPieces`）：重试循环、加权表、
    `generatePieceFromSmallDoor`、`pendingChildren` 随机出队、各子类 `createPiece` /
    `addChildren`、`moveBelowSeaLevel`。
  * 海底神殿（`OceanMonumentStructure` / `OceanMonumentPieces`）：`createTopPiece`、
    `MonumentBuilding.generateRoomGraph`（房间网格 / 连接 / 开口 / 寻源 / `Util.shuffle` /
    扫描循环）、七个 fitter、`getWorldPos` 平移、两翼与阁楼。Java 侧用反射列出
    `childPieces`，连同 `MonumentBuilding` 一起逐项比。
* 新子命令 `cargo run -p mc-bamboo-vanilla -- structure-pieces <java>` +
  `java/StructurePiecesOracle.java`：只 `Bootstrap.bootStrap()`，不启服务端 / 读存档 /
  接受 EULA / 加载 worldgen 注册表；每条查询自己 `setLargeFeatureSeed(seed,cx,cz)`，
  调官方结构件类后用 `createTag` 按统一格式 dump。6 种子 × 8 区块，240 个查询、
  25660 个结构件、25660 个比较 0 不一致。每个结构先故意改坏一处（矿井丢弃首个
  `nextDouble`、要塞丢弃 `randomSmallDoor` 的 `nextInt(5)`、海底神殿扫描循环 5 改 4）
  确认对拍失败再还原。
* 林地府邸的结构件布局**已在第五轮（见下）做完**，本条是 4074b37 当时的交接记录：
  当时 `createMansion` 约 360 行带 `rotation.rotate(Direction)` 和网格位标志
  （`0x800000`）的位置运算、`MansionGrid` 约 300 行、三个 `FloorRoomCollection` 的
  房间名随机，Java 侧要构造 `StructureTemplateManager`
  （`TemplateStructurePiece` 构造器会 `getOrCreate().getBoundingBox`）才能产出
  `WoodlandMansionPiece`，所以当时按「模板只用名字和位置，不需要读模板内容」把
  `BB` 排除在对拍外、`startPos` 作为 terrain 外部输入喂入。第五轮导出了全部模板、
  构造了真实 `StructureTemplateManager`，现在连模板算出的 `BB` 一起比。
* 以上全是纯函数 + oracle，没接进 Director 的 `structure_starts` / `structure_references`
  阶段；游戏里世界仍是超平坦。结构件放置（第 5 步）、Beardifier（第 6 步）、存档
  `structures` 键（第 7 步）仍归结构队员 / 后续里程碑。

**Beardifier（2026-11-14，分支 `cb-wg-beard`，基于 main 4fd5d68）**：第 6 步
（结构对 terrain 密度的影响）单独落地，见第 5 节。`mc-bamboo-worldgen::beardifier`
逐行移植官方 `Beardifier` 的全部计算与 `forStructuresInChunk` 收集，`finalDensity` 的
`beardifier` 标记节点按官方 `ContextBoundSampler` 在求值时从 `SamplerContext` 取本区块
Beardifier；`Generator::terrain_with_beardifier` 注入，`first_occupied_height` 等基线固定
`EMPTY`。新子命令 `beard` 用官方 Beardifier 连进 `NoiseChunk` 出题，12 用例
5,912,076 个比较逐位 0 不一致（含 NOISE / SURFACE / CARVERS 方块），`--break-self-test`
故意改坏一处会失败。**仍是纯函数 + 对拍，没接进游戏**：结构件生成（jigsaw / 结构 /
多件式结构里程碑）没做完，Java 侧用的是覆盖全部 `TerrainAdjustment` 的代表性输入盒而非
`Structure.generate` 的真实产物，`forStructuresInChunk` 的收集分支只有单元测试覆盖。

**第五轮（2026-11-14，`cb-wg-multipiece` 续：林地府邸布局 / 四个结构的 `findGenerationPoint` 前置判定 / 存档口径）**：

* 合入 `main` 与 `cb-wg-structures`（拿到 `cb-wg-jigsaw` 导出的 1511 个 `.nbt`、
  `mc-bamboo-data::structures`、`structure::template` 解析器、`.gitattributes`、
  `cb-fix-main` 的探针修复），再用 `StructureTemplate::load(name).size` 算每个
  `WoodlandMansionPiece` 的 `BB`。新增 `structure/pieces/woodland_mansion/{mod,grid,placer}.rs`
  照官方移植 `MansionGrid`（`recursiveCorridor` / `cleanEdges` / `identifyRooms` /
  `setupThirdFloor` / `SimpleGrid`）与 `MansionPiecePlacer.createMansion`（入口、外墙、
  屋顶、三层地毯 / 墙 / 房间、`FloorRoomCollection` 房间名随机、`addRoom1x1/1x2/2x2`、
  密室）。`StructurePiecesOracle.java` 构造真实 `StructureTemplateManager`（vanilla pack
  + `ServerPacksSource` + `DataFixers`），对拍模板名 / 位置 / rotation / mirror / `BB`。
  `structure-pieces` 扩到 336 个查询、77960 个结构件、0 不一致；两处故意改坏
  （用错的 `1x1` 房间名 278 处、多取一次 `nextInt` 48592 处）确认失败再还原。
* 新增 `structure/start_point.rs`（`Site` / `SiteGates` / `PointKind` / `site_gates` /
  `start_point`），逐行翻译四个结构各自的 `findGenerationPoint` 前置判定并用
  `findValidGenerationPoint`（`findGenerationPoint(context).filter(context::isValidBiome)`）
  判「这个起点是否真的生成」：矿井 / 恶地矿井恒有 stub（Y = 50+yOffset，恶地按结构件
  包围盒中心取 `getBaseHeight(WORLD_SURFACE_WG)` 作 surfaceHeight）、要塞
  `chunkPos.getWorldPosition()` = (minX,0,minZ)、海底神殿 `getBiomesWithin` 周边海洋 +
  `couldValidBiomeExistOnTopOfChunkCenter` + `OCEAN_FLOOR_WG` 高度、林地府邸
  `couldValidBiomeExistInTerrainColumn(blockX7,blockZ7)` + 四角最低高度 +
  `startPos.getY() < 60`。`chunk.rs` 加 `first_occupied_height_ocean_floor`
  （`OCEAN_FLOOR_WG` 用 `BLOCKS_MOTION_IN_HEIGHTMAP` 标签），与已有的
  `first_occupied_height` / `column_biome_names` / `validity.rs` 原语组合。
  新对拍 `structure-generation`（`StructureGenerationOracle.java` 造官方
  `Structure.GenerationContext` 直接调 `Structure.findValidGenerationPoint`，
  `structureTemplateManager` 传 null——所有结构件生成 consumer 在
  `findValidGenerationPoint` 里都不会被触发）：96 个查询、480 行 R、5286 个字段
  逐项比（valid / stub / col / col7 / sur / oceanFloor / base / 五个高度 / lowest /
  below60 / pv + BIOME / KINDS），0 不一致；故意把 `isValidBiome` 过滤丢掉
  （`stub_biome_valid = true`）立刻 192 处不一致，还原回 0。查询集含深海
  （sur=1 32 个区 / 海底神殿 valid=1 16 个）、恶地（恶地矿井 valid=1 6 个）、
  黑森林（林地府邸 valid=1 3 个）、低地（below60=1 2 个），真 / 假分支都被比到。
* `docs/worldgen-structures-plan.md` 4.1 写清海底神殿存档口径：存档时 `Children`
  只有一个 `MonumentBuilding`（`OMB`，基类四键，`OceanMonumentPiece.addAdditionalSaveData`
  为空，房间图是私有 `childPieces`），读回时 `StructureStart.loadStaticStart` 对
  `OceanMonumentStructure` 调 `regeneratePiecesAfterLoad`，用世界种子 + 区块坐标 +
  存档 `BB` 角 + 存档朝向确定性重建整个 `childPieces`。留给结构队员第 7 步。
* 每一步单独提交，每个新对拍都先故意改坏一处确认会失败再还原。以上仍是纯函数 +
  oracle，没接进游戏；四个 `findGenerationPoint` 的「是否生成」是纯判定，
  不写方块，`StructureStart` 仍只是数据类型，第 4–7 步的主体依旧归结构队员。

## 1. 官方 26.3 的结构生成全景

`ChunkStatus` 顺序（`docs/worldgen.md`）：`empty` → `structure_starts` →
`structure_references` → `biomes` → `terrain` → `features` → `initialize_light` →
`light` → `spawn` → `full`。结构和其中五个地方有关：

1. **`structure_starts`**（`ChunkStatusTasks.generateStructureStarts`）：算出「这个区块是哪个
   结构的起点」，生成 `StructureStart`（含全部 `StructurePiece` 及其包围盒）。
2. **`structure_references`**（`ChunkStatusTasks.generateStructureReferences`）：把 8 格半径内
   起点区块的包围盒引用写到本区块，供 `features` 阶段和 `Beardifier` 查。
3. **`terrain`**：`Beardifier.forStructuresInChunk` 进 `NoiseChunk` 的密度函数，影响地形
   （村庄地基、古城埋藏等）。现在用 `EMPTY`。
4. **`features`**（`ChunkGenerator.applyBiomeDecoration`）：每个 decoration step 里**先放结构件
   `StructureStart.placeInChunk`，再放该 step 的地物**（`docs/worldgen-features-plan.md`
   1.1 第 7 条）。结构件靠 `postProcess` 写方块、放方块实体、跑 processor、替换拼图方块。
5. **存档**：区块 NBT 的 `structures` 键存 `starts` 和 `References`。

数据流：`structure_set`（放置规则 + 结构加权表）→ `structure`（类型配置）→
`jigsaw` 结构再引 `template_pool` → `template_pool` 的元素引 `.nbt` 模板和
`processor_list`。`StructureStart` / `StructurePiece` 是运行期产物，不进注册表。

## 2. 数据导出

### 2.1 四个注册表

官方 jar 的 `data/minecraft/worldgen/` 下有（`target/vanilla/versions/26.3/server-26.3.jar`
里的计数）：

| 目录 | `.json` 条目数 | 注册表键 | DIRECT_CODEC |
|---|---:|---|---|
| `worldgen/structure_set/` | 21 | `Registries.STRUCTURE_SET` | `StructureSet.DIRECT_CODEC` |
| `worldgen/structure/` | 52 | `Registries.STRUCTURE` | `Structure.DIRECT_CODEC` |
| `worldgen/template_pool/` | 245 | `Registries.TEMPLATE_POOL` | `StructureTemplatePool.DIRECT_CODEC` |
| `worldgen/processor_list/` | 40 | `Registries.PROCESSOR_LIST` | `StructureProcessorType.DIRECT_CODEC` |

（条目数是 jar 里 `*.json` 的文件数，和 `encodeRegistry` 导出的键数一致；不要把
`unzip -l` 里的目录项算进去。）

`Structure.DIRECT_CODEC` 是 `BuiltInRegistries.STRUCTURE_TYPE.byNameCodec().dispatch(...)`，
`StructureTemplatePool.DIRECT_CODEC` 引 `StructurePoolElement.CODEC`（按 `element_type` 分发），
`StructureProcessorType.DIRECT_CODEC` 是 `SINGLE_CODEC.listOf()`（按 `processor_type` 分发）。
它们都用 `RegistryOps` 把 `Holder` 编成注册名或 `#tag`；`worldgen()` 里已有的
`ops = registries.createSerializationContext(JsonOps.INSTANCE)` 足够，沿用现有的
`encodeRegistry(registries, ops, key, codec)`（`Extractor.java` 1504 行）即可，按名字排序。

所以 `Extractor.java` 的 `worldgen(RegistryAccess)`（1397 行）只加四行：

```java
worldgen.add("structure_set", encodeRegistry(registries, ops, Registries.STRUCTURE_SET, StructureSet.DIRECT_CODEC));
worldgen.add("structure", encodeRegistry(registries, ops, Registries.STRUCTURE, Structure.DIRECT_CODEC));
worldgen.add("template_pool", encodeRegistry(registries, ops, Registries.TEMPLATE_POOL, StructureTemplatePool.DIRECT_CODEC));
worldgen.add("processor_list", encodeRegistry(registries, ops, Registries.PROCESSOR_LIST, StructureProcessorType.DIRECT_CODEC));
```

需要补的 import：`Registries`、`StructureSet`、`Structure`、`StructureTemplatePool`、
`StructureProcessorType`（都在 `net.minecraft.world.level.levelgen.structure.*` 或其
`templatesystem` / `pools` 子包）。`Extracted.worldgen` 是 `serde_json::Value`
（`extract.rs` 86 行），`generate.rs::write_worldgen`（145 行）会把整棵树按键名递归排序后
写进 `worldgen.json`，`check_worldgen_floats` 自动保护新增的浮点（如 `frequency`）。
**重跑方式**：`cargo run -p mc-bamboo-vanilla --no-default-features -- extract <java>`
（`preserve_order` 会改键序，必须关掉默认特性），`git diff -- crates/mc-bamboo-data/src/generated/`
除 `worldgen.json` 的四个新键外应为空；再跑旧 `chunkgen --through carvers` 确认没破坏。

`Structure` 的 `spawn_overrides`、`step`、`terrain_adaptation`、`biomes`（`HolderSet<Biome>`）、
`jigsaw` 的 `start_pool` / `start_height` / `project_start_to_heightmap` / `size` /
`max_distance_from_center` 等都会由官方 codec 展开成普通 JSON。`biomes` 是
`#minecraft:has_structure/...` 这类群系标签，Rust 侧要用 `mc-bamboo-data` 已有的
群系/标签表解析（见 7.2）。

### 2.2 结构模板 `.nbt`

`data/minecraft/structure/**.nbt` 共 1511 个、约 4.04 MiB，是 `StructureTemplate` 的
NBT（`size` / `palette` / `blocks` / `entities`），被 `single_pool_element` 的 `location`
按注册名引用。这些是二进制文件，不能手写。

**已落地（`cb-wg-jigsaw` 分支，2026-11-14）**：生成器新步骤
（`tools/mc-bamboo-vanilla/src/structure_templates.rs`）从 jar 的 zip 条目解掉外层
deflate，把每个 gzip 字节按注册名排序拼成 `structures.bin`，另写 `structures.rs` 索引
（`BLOB` / `ENTRIES` / `get(name)`），由 `mc-bamboo-data` 用 `include_bytes!` 暴露。
1511 个模板、4236847 字节，重跑 `extract` 逐字节无 diff（`.gitattributes` 给二进制加
了例外）。Rust 侧 `mc-bamboo-worldgen::structure::template` 用
`mc-bamboo-nbt::read_compressed` 解析出 `size` / `palette`（26.3 的 `id` /
`properties`）/ `blocks`（含方块实体 `nbt`）/ `entities` / 拼图方块（`orientation` 的
front / top、`name` / `target` / `pool` / 优先级 / `joint`），单元测试解析全部 1511 个
模板。`structure-templates` 对拍用官方 `NbtIo` 逐节点规范化整棵 NBT 树并校验导出字节
等于 jar 原文，1511 个模板 0 不一致，故意改坏 Rust 的 int 标记会让 1511 个全部失败。
多 palette 模板有 20 个，`getRandomPalette` 的按位置随机选择留到结构件生成那一步处理。

### 2.3 为什么用 codec 重编码而不是直接拷 jar 里的 JSON

和 `feature` / `placed_feature` / `noise_settings` 一致：codec 会把内联引用、默认值、
`Holder` 统一编成同一套自洽形式，Rust 侧用一套解析语义即可，且 `worldgen.json` 可重跑无
diff。jar 里的原始 JSON 作为「官方原文」保留在 `target/vanilla`，需要逐字段核对时和导出
结果 diff；两者若不符，以 codec 结果为准（它是官方运行时实际加载的形式）。

## 3. `StructurePlacement`（放置）

接口 `StructurePlacement`（`placement/StructurePlacement.java`）：核心是
`isStructureChunk(ChunkGeneratorStructureState state, int sourceX, int sourceZ)`。
`ChunkGeneratorStructureState.createForNormal`（`chunk/ChunkGeneratorStructureState.java`）
先按 `hasBiomesForStructureSet` 过滤 `possibleStructureSets`：结构集里任一结构的 `biomes()`
与 `biomeSource.possibleBiomes()` 有交集才保留；主世界因此会排除纯下界/末地结构集。
这一步是「哪些结构集参与放置」的前提，Rust 侧照 `possibleBiomes()`（主世界预设 67 个群系）
过滤。

`AbstractSpreadingStructurePlacement.isStructureChunk` =
`isPlacementChunk(state,x,z)` && `applyAdditionalChunkRestrictions(x,z,levelSeed)` &&
`applyInteractionsWithOtherStructures(state,x,z)`，三者都是与运算，顺序照搬（虽然都是纯函数，
但决定了语义）。

### 3.1 `random_spread`（20 个结构集）

`RandomSpreadStructurePlacement.getPotentialStructureChunk(seed, sourceX, sourceZ)`：

```
spacedGridX = floorDiv(sourceX, spacing);  spacedGridZ = floorDiv(sourceZ, spacing)
random = new WorldgenRandom(new LegacyRandomSource(0L))
random.setLargeFeatureWithSalt(seed, spacedGridX, spacedGridZ, salt)
limit = spacing - separation
spreadX = spreadType.evaluate(random, limit)   // 先消耗随机数
spreadZ = spreadType.evaluate(random, limit)
return ChunkPos(spacedGridX*spacing + spreadX, spacedGridZ*spacing + spreadZ)
```

`isPlacementChunk` = `potential.x == sourceX && potential.z == sourceZ`。
`setLargeFeatureWithSalt(seed,x,z,blend) = setSeed(x*341873128712L + z*132897987541L + seed + blend)`
（`WorldgenRandom.java`）。`WorldgenRandom` 内层是 `LegacyRandomSource`，`nextInt(bound)`
走 `BitRandomSource` 默认实现（`next(31)` + 无偏取模），**不是** Xoroshiro 的 Lemire；
`mc-bamboo-worldgen::random::WorldgenRandom` 已有 `set_large_feature_with_salt` / `next_int`，
按 `Random::Legacy(LegacyRandom::new(0))` 组合即可。

`RandomSpreadType`（`placement/RandomSpreadType.java`）：
`LINEAR = nextInt(limit)`；`TRIANGULAR = (nextInt(limit)+nextInt(limit))/2`（整数除）。
`end_cities` / `ocean_monuments` / `woodland_mansions` 是 triangular。

频率削减 `applyAdditionalChunkRestrictions` = `!(frequency < 1.0F) ||
method.shouldGenerate(seed, salt, sourceX, sourceZ, frequency)`（`frequency` 默认 1.0，
`method` 默认 `default`）。四种 `FrequencyReductionMethod` 都要照字节码：

| 方法 | 用到的结构集 | 逻辑 |
|---|---|---|
| `default` | 无（当前数据里 frequency<1 的都指定了别的方法） | `probabilityReducer`：`setLargeFeatureWithSalt(seed, salt, sourceX, sourceZ)`，`nextFloat() < frequency` |
| `legacy_type_1` | `pillager_outposts`（0.2） | `legacyPillagerOutpostReducer`：`cx=sourceX>>4, cz=sourceZ>>4`；`setSeed(cx ^ cz<<4 ^ seed)`；`nextInt()`；`nextInt((int)(1/frequency))==0` |
| `legacy_type_2` | `buried_treasures`（0.01） | `legacyArbitrarySaltProbabilityReducer`：`setLargeFeatureWithSalt(seed, sourceX, sourceZ, 10387320)`，`nextFloat() < frequency` |
| `legacy_type_3` | `mineshafts`（0.004） | `legacyProbabilityReducerWithDouble`：`setLargeFeatureSeed(seed, sourceX, sourceZ)`，`nextDouble() < frequency` |

注意 `probabilityReducer` 把 `salt` 当 `setLargeFeatureWithSalt` 的 `x` 参数、
`sourceX` 当 `z`、`sourceZ` 当 `blend`（与另三个方法的参数直觉不同，照抄 26.3 源码）；
`legacy_type_2` 忽略配置的 `salt`，用硬编码 `10387320`（`HIGHLY_ARBITRARY_RANDOM_SALT`）。

`ExclusionZone`（`AbstractSpreadingStructurePlacement.ExclusionZone`）：只有
`pillager_outposts` 有（`other_set: minecraft:villages`，`chunk_count: 10`）。
`applyInteractionsWithOtherStructures` = `!exclusionZone.isPresent() ||
!isPlacementForbidden(...)`；`isPlacementForbidden` 调
`state.hasStructureChunkInRange(otherSet, sourceX, sourceZ, chunkCount)`，它对
`[x-range, x+range] × [z-range, z+range]` 每格调 `otherSet.placement().isStructureChunk(...)`。
`villages` 是 `random_spread`、frequency=1、无 exclusion_zone，所以这层递归只需
`getPotentialStructureChunk`，纯函数可算。

### 3.2 `concentric_rings`（`strongholds` / 要塞）

`ConcentricRingsStructurePlacement.isPlacementChunk` =
`generatorState.getRingPositionsFor(this).contains(ChunkPos(sourceX,sourceZ))`。
环位置在 `ChunkGeneratorStructureState.generateRingPositions` 里算：

```
random = RandomSource.create(); random.setSeed(concentricRingsSeed)  // 普通世界 = levelSeed
angle = random.nextDouble() * 2π
for i in 0..count:
    dist = 4*distance + distance*circle*6 + (random.nextDouble()-0.5)*(distance*2.5)
    initialX = round(cos(angle)*dist); initialZ = round(sin(angle)*dist)
    biomeSearchGenerator = random.fork()
    // 异步：biomeSource.findBiomeHorizontal(initialX*16+8, 0, initialZ*16+8, 112,
    //            preferredBiomes::contains, biomeSearchGenerator, randomState)
    //   命中则把位置移到该群系所在的区块，否则用 (initialX, initialZ)
    angle += 2π/spread
    if ++positionInCircle == spread:
        circle++; positionInCircle=0
        spread += 2*spread/(circle+1); spread = min(spread, count-i)
        angle += random.nextDouble()*2π
```

`RandomSource.create()` 在 26.3 是 `XoroshiroRandomSource`（不是 Legacy），`fork()` 走
`PositionalRandomFactory`；`preferredBiomes` = `#minecraft:stronghold_biased_to`。
关键外部依赖是 `BiomeSource.findBiomeHorizontal`：以气候采样器在半径 112 内按
`preferredBiomes` 找最近群系，逐格用 `BiomeResolver`，取数顺序和 `Climate.Sampler`
的缓存行为都必须和官方一致。`mc-bamboo-worldgen` 已有 `climate` / `sampler` /
`biome` / `biome_manager`，但 `findBiomeHorizontal` 这条具体搜索还没移植
（也是 `docs/worldgen-features-plan.md` 7.3 提到的 uncached 群系查询疑点同源）。
因此 **ring 位置的确定性几何 + 环参数解析先做，`findBiomeHorizontal` 的群系吸附
标为待办/已知限制**，要塞的「起点区块」在吸附完成前不能声称 0 不一致。

### 3.3 纯函数接口（对接流水线队员）

`mc-bamboo-worldgen::structure` 本轮已落地的接口（`structure/mod.rs` / `placement.rs` /
`registry.rs`），供流水线队员和队长合并时对接：

```rust
// 解析（structure/registry.rs）
pub struct StructureSets { /* ... */ }
impl StructureSets {
    /// 从 worldgen.json 的 structure_set 对象解析，按名字排序。
    pub fn parse(raw: &serde_json::Value) -> Result<Self, String>;
    pub fn get(&self, name: &str) -> Option<&StructureSetEntry>;
    pub fn random_spread_names(&self) -> Vec<String>;
}
pub fn possible_structure_sets(sets: &StructureSets, structures: &Structures,
                               possible_biomes: &HashSet<i32>) -> Vec<String>;

// 放置判定（structure/placement.rs + structure/mod.rs）
pub enum Placement { RandomSpread(RandomSpread), ConcentricRings(ConcentricRings) }
impl RandomSpread {
    /// 官方 getPotentialStructureChunk(seed, sourceX, sourceZ) -> (chunkX, chunkZ)。
    pub fn get_potential_structure_chunk(&self, seed: i64, x: i32, z: i32) -> (i32, i32);
    pub fn is_placement_chunk(&self, seed: i64, x: i32, z: i32) -> bool;
}
impl Placement {
    /// 官方 applyAdditionalChunkRestrictions（频率削减）。
    pub fn apply_additional_chunk_restrictions(&self, seed: i64, x: i32, z: i32) -> bool;
}

/// ChunkGeneratorStructureState 的纯数据等价物，只依赖解析好的注册表。
pub struct StructureState { /* ... */ }
impl StructureState {
    pub fn new(sets: StructureSets) -> Self;
    /// 注入 rings::ring_positions 的结果；concentric_rings 的 isPlacementChunk 用它。
    pub fn set_ring_positions(&mut self, set_name: &str, positions: impl IntoIterator<Item = (i32, i32)>);
    /// 官方 StructurePlacement.isStructureChunk(state, x, z)：
    /// isPlacementChunk && applyAdditionalChunkRestrictions && applyInteractionsWithOtherStructures。
    pub fn is_structure_chunk(&self, set_name: &str, seed: i64, x: i32, z: i32) -> bool;
    /// createStructures 的放置门：返回命中结构集（按名字排序），只到「结构集」这一层。
    pub fn start_candidates(&self, seed: i64, x: i32, z: i32) -> Vec<StartCandidate>;
    /// 放置门 + existingStart 短路 + 加权选择序列（structure/start.rs）。
    pub fn start_plans(&self, seed: i64, x: i32, z: i32, existing: &StructureStarts) -> Vec<StartPlan>;
}
pub struct StartCandidate { pub set: String, pub structures: Vec<SelectionEntry> }
pub struct StartPlan { pub set: String, pub attempts: Vec<String> }
// 数据与辅助函数（structure/start.rs）
pub struct StructureStart { /* structure, chunk_x/z, references, pieces, terrain_adaptation */ }
pub struct StructurePiece { /* piece_type, bounding_box, orientation, ground_level_delta */ }
pub struct BoundingBox { /* min/max x,y,z */ }
pub struct StructureStarts { pub by_structure: BTreeMap<String, StructureStart> }
pub type References = BTreeMap<String, Vec<i64>>;
pub fn selection_sequence(entries: &[SelectionEntry], seed: i64, x: i32, z: i32) -> Vec<String>;
pub fn chunk_pos_pack(x: i32, z: i32) -> i64;
pub fn structure_references(center_x: i32, center_z: i32,
    neighbors: &[(i32, i32, &StructureStarts)]) -> References;
// 要塞环位置（structure/rings.rs）
pub fn ring_positions(seed: i64, resolver: &mut BiomeResolver, allowed: &HashSet<String>,
    distance: i32, spread: i32, count: i32) -> Vec<(i32, i32)>;
pub fn find_biome_horizontal(/* ... */) -> Option<(i32, i32, String)>;
```

注意与官方的差别，合并时别当成已完成：

* `start_candidates` / `start_plans` 只给「哪个结构集把 (x,z) 判为起点」和
  「按什么顺序尝试结构」，**不含** `Structure.generate`（`findValidGenerationPoint`
  的群系 + 高度检查 + 结构件生成）。所以没有「起点是否有效 / 最终选中哪个结构」，
  `StructureStarts` 目前只是数据类型，没有生产它的实现。
* `concentric_rings` 的 `is_structure_chunk` 必须先调 `set_ring_positions` 注入
  `ring_positions` 的结果；没注入时仍返回 false（占位）。`Placement::is_placement_chunk`
  对 `ConcentricRings` 单独调用时恒 false，只有 `StructureState` 这条路径是准的。

后续阶段的接口（本轮**未实现**，供队长接上）：

* `structure_starts` 阶段：`Generator::structure_starts(seed, chunk_x, chunk_z) ->
  StructureStarts`。在 `start_plans` 之上补 `Structure.generate`，产出 `StructureStart`
  （含 `StructurePiece` 及包围盒）。
* `structure_references` 阶段：`Generator::structure_references(center, neighbor_starts) ->
  References`，照 `createReferences` 的 17×17 邻域、包围盒相交、`ChunkPos.pack`
  （`pack(x,z) = (x as u32 as i64) | ((z as u32 as i64) << 32)`）写引用。
  纯函数 `structure_references` 已就位，但没接进流水线。
* 两个函数都只依赖 `&self` 和入参，不碰世界状态、不取系统随机数，可进计算池
  （`docs/world-model-design.md` 5.3.1）。流水线队员先接空实现，队长合并时按此签名接上。

## 4. `StructureStart` / `StructurePiece` 的存档格式

区块 NBT 的 `structures` 键（`chunk/storage/SerializableChunkData.java` 457、505 行）：

```
structures (Compound)
├── starts (Compound)  : { "<structure id>": <StructureStart tag>, ... }
└── References (Compound): { "<structure id>": long[] (ChunkPos.pack 的区块键), ... }
```

`StructureStart.createTag`（`StructureStart.java`）：有效时写
`id`（结构注册名）、`ChunkX`、`ChunkZ`、`references`(int)、`Children`(ListTag)；
无效时只写 `id = "INVALID"`。`loadStaticStart` 读回，`OceanMonumentStructure` 还要
`regeneratePiecesAfterLoad`。`StructurePiece` 的基类 tag（`StructurePiece.java` 86 行）：
`id`（`BuiltInRegistries.STRUCTURE_PIECE` 注册名）、`BB`（包围盒，6 个 int）、
`O`（朝向的 2D 序号，-1 为 null）、`GD`（`groundLevelDelta`），外加各子类的
`addAdditionalSaveData`（模板名、rotation、jigsaw junction、箱子标记等）。
`References` 的 long 用 `putLongArray`，读回时按 `chessboardDistance(pos) > 8` 过滤
（`SerializableChunkData.java` 549 行）。`world-dir` 对拍工具
（`tools/mc-bamboo-vanilla/src/world_dir.rs`）用来确认官方能读回。

### 4.1 海底神殿的存档口径（留给第 7 步存档）

海底神殿的「结构件列表」和其它结构不一样：**存档时只存一个结构件，读回时把房间图整体
重建**。第 7 步写 / 读 `structures` 键必须照下面的口径做，不能把房间图当成普通结构件列表
存进去、也不能只读回 `Children` 就直接 `placeInChunk`。

**生成 / 存档时存什么**：

* `OceanMonumentStructure.generatePieces(builder, context)` 只
  `builder.addPiece(createTopPiece(...))`：`PiecesContainer` 里**只有一个**
  `OceanMonumentPieces.MonumentBuilding`（注册名 `OMB`）。整个房间图——
  `MonumentBuilding.generateRoomGraph(random)` 造的 `RoomDefinition` 网格、入口 / 核心房间、
  七个 `MonumentRoomFitter` 造的房间、两翼 `OceanMonumentWingRoom`、阁楼
  `OceanMonumentPenthouse`——是 `MonumentBuilding` 的**私有字段 `childPieces`**，不是
  `PiecesContainer` 里的独立结构件；`MonumentBuilding.postProcess`
  （`OceanMonumentPieces.java` 394 行）才遍历 `childPieces` 逐间 `postProcess`。
* `StructureStart.createTag(context, chunkPos)`（`StructureStart.java` 103 行）写
  `Children = pieceContainer.save(context)`，因此海底神殿的 `Children` 是长度 1 的
  `ListTag`，只有那个 `MonumentBuilding`。
* `StructurePiece.createTag`（84 行）只写基类四个键 `id` / `BB` / `O` / `GD`；
  `OceanMonumentPiece.addAdditionalSaveData`（`OceanMonumentPieces.java` 1501 行）是
  **空实现**，所以 `MonumentBuilding` 不存任何房间图 / `childPieces` / 随机源状态，
  只有这四项。`BB` 来自 `makeBoundingBox(west, 39, north, direction, 58, 23, 58)`
  （固定 58×23×58、Y 从 39 起，`minX = west`、`minZ = north`），`O` 是朝向
  （`setOrientation(direction)`），`GD = 0`。

**读回时怎么重建**：

* `SerializableChunkData.read`（292 行）把 `level.getSeed()` 传给
  `StructureStart.loadStaticStart(context, tag, seed)`；它先
  `PiecesContainer.load(children, context)` 用 `MonumentBuilding(CompoundTag)`
  构造器（`OceanMonumentPieces.java` 223 行）造出那个结构件——这个构造器**只调
  `super(..., tag)`，`childPieces` 是空的**。若就此拿去 `placeInChunk`，`postProcess`
  遍历空列表，神殿一个房间都放不出来。
* 因此 `loadStaticStart` 在 `structure instanceof OceanMonumentStructure` 时
  （`StructureStart.java` 58–60 行）额外调
  `OceanMonumentStructure.regeneratePiecesAfterLoad(chunkPos, seed, pieces)`
  （`OceanMonumentStructure.java` 54 行）重建房间图。这是**只对海底神殿**的特例。
* `regeneratePiecesAfterLoad` 的过程：

  ```
  random = new WorldgenRandom(new LegacyRandomSource(RandomSupport.generateUniqueSeed()))
  random.setLargeFeatureSeed(seed, chunkPos.x, chunkPos.z)   // 唯一随机源；初始 seed 被覆盖
  old = savedPieces.pieces().get(0)                          // 那个 MonumentBuilding
  west = old.getBoundingBox().minX(); north = old.getBoundingBox().minZ()
  defaultOrientation = Direction.Plane.HORIZONTAL.getRandomDirection(random)  // 消费一次 nextInt(4)
  orientation = old.getOrientation() != null ? old.getOrientation() : defaultOrientation
  topPiece = new MonumentBuilding(random, west, north, orientation)           // 重跑 generateRoomGraph
  return 只含 topPiece 的 PiecesContainer
  ```

  * `generateUniqueSeed()` 只是初始值，`setLargeFeatureSeed(seed, x, z)` 立刻把它重置成
    「世界种子 + 区块坐标」的确定性值，所以读回是**确定性重建**而非重新随机：
    `generateRoomGraph` 里 `Util.shuffle`、`nextInt()` 等取数序列与当初生成逐次一致。
  * `defaultOrientation` 这次 `nextInt(4)` **即便 `orientation` 用存档里的朝向也必须取**，
    目的是让 `random` 的取数位置和「当初 `createTopPiece` 先 `getRandomDirection`、
    再 `new MonumentBuilding(random, ...)`」对齐，重建出的房间图才和原版逐块一致。
  * 返回的 `PiecesContainer` 仍只有一个 `MonumentBuilding`，但它的 `childPieces` 已被
    `generateRoomGraph` 完整重建，`postProcess` 能照常放全部房间。读回后的 `BB` / 朝向
    来自存档的 `MonumentBuilding`，`west/north` 就是存档 `BB.minX()/minZ()`。
* 其它结构（矿井 / 要塞 / 林地府邸 / jigsaw）没有这一步：每个结构件各自用
  `(CompoundTag)` 构造器 + `addAdditionalSaveData` 的字段恢复状态，`Children` 存的就是
  完整结构件列表，读回直接用，不需要重建。

**给第 7 步的验收口径**：写 `structures.starts.<structure>.Children` 时海底神殿只写
一个 `OMB` 结构件（基类四键）；读回必须先 `PiecesContainer.load`、再对
`OceanMonumentStructure` 跑 `regeneratePiecesAfterLoad`，然后才进 `placeInChunk`。
`world-dir` 对拍应验证：存档前后 `Children` 长度恒为 1、重建后的 `BB` / 朝向 /
`postProcess` 覆盖的房间集合与直接生成一致。结构队员的存档实现以本节为准，不要凭
「每个结构件一个 tag」的通用直觉把房间图拆开存。

## 5. `Beardifier`

`Beardifier.forStructuresInChunk(structureManager, chunkPos)`（`levelgen/Beardifier.java`）：

1. 取 `terrainAdaptation() != NONE` 的 `StructureStart`，遍历其 `StructurePiece`，
   收 `piece.isCloseToChunk(chunkPos, 12)` 的：`PoolElementStructurePiece` 且
   `Projection.RIGID` 收成 `Rigid(box, terrainAdjustment, groundLevelDelta)`，
   其 `JigsawJunction` 在 `chunk±12` 范围内也收；非 pool piece 收成 `Rigid(..., 0)`。
2. `affectedBox = 所有 piece/junction 包围盒的并集再 inflatedBy(24)`；没有则 `EMPTY`。
3. `sampleValue` / `sampleVolume`：`affectedBox` 内逐点 `sampleValueUnchecked`，
   对每个 `Rigid` 按 `TerrainAdjustment`（`NONE` / `BURY` / `BEARD_THIN` / `BEARD_BOX` /
   `ENCAPSULATE`）用 `getBuryContribution` / `getBeardContribution` 累加，
   对每个 junction 用 `getBeardContribution(dx,dy,dz,dy)*0.4` 累加。
   `BEARD_KERNEL` 是构造期算好的 24³ 个 `e^(-dist²/16)`（`computeBeardContribution`），
   `getBeardContribution` 里还用 `Mth.fastInvSqrt`。

接入方式：它实现 `DensityFunction`（`Beardifier.CONTEXT_KEY`），terrain 的
`finalDensity` 通过 `SamplerContext` 里的 `Beardifier` 取值。现在
`mc-bamboo-worldgen::chunk` 的 `createNoiseChunk` 传 `Beardifier.EMPTY`（恒 0）。
第 6 步换成按 `structure_starts`/`structure_references` 结果构造的真实 `Beardifier`，
并照官方顺序把它并进密度函数编译（不能改变噪声实例的取数次数和顺序）。
对拍：terrain 阶段在没有结构的区块必须仍是 0 不一致（EMPTY 路径回归），在有结构地基的
区块逐方块比 `finalDensity` / `doFill` 结果。

**第 6 步已落地（2026-11-14，分支 `cb-wg-beard`，基于 main 4fd5d68）**：

* 逐行移植到 `mc-bamboo-worldgen::beardifier`（新模块）：五种 `TerrainAdjustment`、
  `BEARD_KERNEL`（24³ 个 `e^(-dist²/16)`，构造期算好，逐位核对官方原始位模式）、
  `getBuryContribution` / `getBeardContribution`（含 `Mth.fastInvSqrt` 的 Quake 魔数反平方根）、
  `sampleValueUnchecked`（官方 `compute`）和 `sampleVolume`（`fillArray` 的 z→x→y 成批、
  `affectedBox` 相交裁剪），以及 `forStructuresInChunk` 的收集（`isCloseToChunk(12)`、
  RIGID projection、junction 的 `chunk±12` 严格 XZ 过滤、并集 `inflatedBy(24)`）。
* 接入按官方 `ContextBoundSampler` 的语义：`finalDensity` 里的 `beardifier` 标记节点编译成
  `Sampler::Beardifier`，在 `sampleValue` / `sampleVolume` 时从 `sampler::Context` 的
  user field 取本区块 Beardifier，没注入就是 `Beardifier.EMPTY`（恒 0）。**编译仍是一份**，
  不按区块重编译，所以噪声实例、`cache` 编号和取数顺序都不变；`Generator::terrain_with_beardifier`
  把 Beardifier 注进 `Context`，`terrain` / `noise` / `first_occupied_height` 等基线路径永远
  不注入（固定 `EMPTY`，满足疑点 2）。结构为空时 `finalDensity` 逐位不变。
* 输入类型 `StructureStartInput` / `PieceInput` / `Rigid` / `Junction` 设计成能直接接收
  `structure_starts` 产出的结构件；`Beardifier::from_parts(rigids, junctions)` 是对拍和合并
  共用的入口，`for_structures_in_chunk(chunk_x, chunk_z, starts)` 是留给流水线接上的接口。
* 对拍 `cargo run -p mc-bamboo-vanilla -- beard <java>`（新子命令 + `java/BeardifierOracle.java`）：
  Java 侧用官方 `Beardifier` 的 `@VisibleForTesting` 构造器，从导出的 Rigid（盒 + 调整类型 +
  `groundLevelDelta`）和 junction 造真实 Beardifier，连进 `NoiseChunk` 跑
  `doFill` / `buildSurface` / `generateCarvers`；Rust 侧从同一份输入重建 Beardifier，逐位比较
  `BEARD_KERNEL` 原始位模式、整块 16×height×16 的 `sampleValue` / `sampleVolume`、
  `affectedBox`，以及 NOISE / SURFACE / CARVERS 三个阶段的方块。12 个用例（空、五种
  `TerrainAdjustment`、junction、跨区块 / 贴顶贴底边界）共 5,912,076 个比较 0 不一致。
  `--break-self-test` 故意把一个 `groundLevelDelta` 加 1，报 32 处不一致并失败，还原后回 0；
  旧 `chunkgen --through carvers` 仍 0 不一致（EMPTY 回归）。
* **没做 / 已知限制**：Java 侧的盒是按真实结构尺度构造的代表性输入，覆盖全部
  `TerrainAdjustment`，但**不是** `Structure.generate` 跑出的真实结构起点——jigsaw 模板
  导出与结构件生成归 jigsaw / 结构 / 多件式结构里程碑。Beardifier 的输入本就只是包围盒 +
  调整类型 + 连接点，所以对密度计算是完整逐位对拍；等结构件接上后把
  `for_structures_in_chunk` 的产物填进同一套输入即可。`forStructuresInChunk` 的收集分支
  （`isCloseToChunk`、RIGID projection、junction 过滤）只有 Rust 单元测试覆盖，没有和官方
  `StructureManager` 端到端对拍（那需要能产出真实 `PoolElementStructurePiece` 的模板环境）。
  本轮全是纯函数 + 对拍，**没接进 Director**：`structure_starts` / `structure_references`
  仍是空实现，游戏里 `terrain` 仍走 `EMPTY`，世界仍是超平坦。

## 6. `features` 阶段里结构件的放置

`ChunkGenerator.applyBiomeDecoration`（351 行起，逐段）：

1. `structuresByStep = structuresRegistry` 按 `structure.step().ordinal()` 分组
   （`GenerationStep.Decoration` 11 个，`surface_structures` 是大多数结构的 step）。
2. 每个 step `stepIndex`：若 `structureManager.shouldGenerateStructures()`，
   对该 step 的每个结构用**局部** `index`（从 0 起，和地物的全局 index 分开）
   `random.setFeatureSeed(decorationSeed, index, stepIndex)`，再
   `structureManager.startsForStructure(sectionX, sectionZ, structure).forEach(start ->
   start.placeInChunk(level, structureManager, generator, random, getWritableArea(chunk), centerPos))`。
3. 之后才跑该 step 的地物（`docs/worldgen-features-plan.md` 1.1）。
   `StructureStart.placeInChunk`（`StructureStart.java`）对每个与 `chunkBB` 相交的
   `StructurePiece` 调 `postProcess`（写方块、方块实体、processor、拼图替换），
   最后 `structure.afterPlace`。

第 5 步按这个顺序把结构件放置插到 `feature/` 主循环每个 step 的开头：地物 A/B 在改
主循环，本分支**只对主循环做一处小而集中的调用**（`feature/run.rs` 里插一个
`structure::place_step(...)`，具体逻辑全放新文件）。`postProcess` 需要
`StructureTemplate`（`.nbt`）、`StructureProcessor`（`processor_list`）、拼图方块替换、
箱子战利品表（`setBlockEntityLootTable` 的 `nextLong`，README 已记「结构生成时还没接」），
逐方块对拍（`chunkgen --through features`，见 `docs/worldgen-features-plan.md` 第 4 节）。

## 7. 移植顺序（每步单独提交 + 对拍）

| 步 | 内容 | 对拍 / 完成标准 | 本轮状态 |
|---|---|---|---|
| 1 | 本方案文档 | — | **做完** |
| 2 | 数据导出：`worldgen.json` 加四键，Rust 解析骨架 | `extract` 重跑无 diff（除四键）、旧 `chunkgen --through carvers` 仍 0 | **做完**：重跑无 diff、`--through carvers` 小样本 0 不一致 |
| 3 | 放置：`random_spread`（含频率削减、exclusion zone、spread type）、`concentric_rings` 几何 | 大范围区块「哪个区块有哪个结构起点」0 不一致；新对拍先故意改坏一处确认会失败 | **做完**：20 集 × 464 查询 = 9280 放置比较 + 9744 加权选择比较 0 不一致；`concentric_rings` 6 种子 768 环位置 + 864 `isStructureChunk` 0 不一致 |
| 4 | 结构件生成：旧式（沉船、海底遗迹、沙漠/丛林神殿、女巫小屋、雪屋、埋藏宝藏、废弃矿井、要塞、林地府邸、海底神殿、废弃传送门）先做，再 jigsaw（村庄、前哨站、古城、试炼密室、古迹废墟） | 每个结构起点 + 全部结构件的包围盒、朝向、类型 0 不一致 | **部分做完**：四个代码生成单件结构（沙漠神殿 / 丛林神殿 / 女巫小屋 / 埋藏宝藏，`structure-single` 384 查询 0 不一致）、废弃矿井 / 要塞 / 海底神殿（`cb-wg-multipiece` 240 查询 25660 结构件 0 不一致）已做；需要模板的单件结构（雪屋 / 沉船 / 海底遗迹 / 废弃传送门）、林地府邸、jigsaw 结构件没做 |
| 5 | `features` 阶段结构件放置：processor、方块实体、拼图替换、战利品表 | `chunkgen --through features` 逐方块 0 不一致 | 没做 |
| 6 | `Beardifier` 接进 terrain 密度 | terrain 阶段（EMPTY 回归 + 有结构地基）0 不一致 | **做完（纯函数 + 对拍，`cb-wg-beard`）**：`beard` 12 用例 5,912,076 比较 0 不一致（kernel / sampleValue / sampleVolume / affectedBox / 三阶段方块），`--break-self-test` 会失败；`chunkgen --through carvers` EMPTY 回归仍 0。**没接进 Director**：游戏里仍走 `EMPTY` |
| 7 | 存档：`structures` 键写/读；海底神殿「只存一个 `MonumentBuilding`、读回重建房间图」的口径见 4.1 | 读回一致，`world-dir` 确认官方能读 | **通用编码做完**：`structure-save` 8 case 规范 NBT 0 不一致，官方 `PiecesContainer.load` / `save` 往返、`world-dir` 的 `SerializableChunkData.parse` 都能读；各结构件 `addAdditionalSaveData` 字段与读档路径由 `cb-wg-multipiece` 在其上补齐 |

第三轮还落地了步 1 与步 4 之间的一层通用原语：`Structure.generate` 的
`findValidGenerationPoint` 群系 / 高度判定（整列群系、四角高度、起点单点群系），
见第 0 节与 7.1。第五轮（本轮）在步 4 / 6 / 7 上分别落地了代码生成单件结构布局、
`Beardifier` 纯函数、存档 `structures` 键，各自新增独立 oracle 并先故意改坏一处确认会失败；
详见第 0 节「第五轮」和 7.1 末尾。所有里程碑仍是纯函数 + oracle，没接进 Director。

### 7.1 对拍方法

* **第 3 步（本轮重点，已落地）**：新增 `cargo run -p mc-bamboo-vanilla -- structures <java>`
  子命令和 `java/StructurePlacementOracle.java`（新文件，不改 `ChunkgenOracle.java`）。
  Java 侧用 Gson 从同一份 `worldgen.json` 独立读 `structure_set` 参数、用官方
  `RandomSpreadStructurePlacement` 的公开构造器造对象，对「8 个种子 × 58 个区块」（坐标
  含 ±100 万）逐格调官方 `getPotentialStructureChunk` 和 `applyAdditionalChunkRestrictions`；
  Rust 侧用生产代码 `StructureSets` 解析同一份文件、用 `get_potential_structure_chunk` +
  `apply_additional_chunk_restrictions` 复算，逐行比潜在起点和频率判定。两边解析、随机源
  都独立，任一侧错都出不一致。结果：20 个 random_spread 集、9280 个比较 0 不一致
  （1296 条频率判定为 false，确认四种 reducer 的 false 分支真的被比到）。
  故意把 `floorDiv` 换成截断除法，对拍立刻报 312 处不一致并失败，还原后恢复 0。
  `exclusion zone`（`pillager_outposts`）和 `concentric_rings` 不在此对拍内，见第 9 节疑点。
* **第二轮的步 1（加权选择，已落地）**：`structures` 子命令加了一节 `SELECT`，Java 用官方
  `WorldgenRandom.setLargeFeatureSeed` + `nextInt(total)` 对 21 个结构集输出尝试序列，
  Rust 用生产代码 `selection_sequence` 复算，9744 个比较 0 不一致。故意把
  `setLargeFeatureSeed` 的 x/z 对调会报 1987 处不一致。
* **第二轮的步 2（要塞环位置，已落地）**：`cargo run -p mc-bamboo-vanilla --
  structure-rings <java>`，`java/StructureRingsOracle.java` 按 `WorldLoader` 前半段读官方
  注册表、造 `NoiseBasedChunkGenerator` / `RandomState` / `ChunkGeneratorStructureState`，
  调官方 `getRingPositionsFor`、`findBiomeHorizontal`、`isStructureChunk` 输出；Rust 用
  `rings::ring_positions` / `find_biome_horizontal` / `StructureState::is_structure_chunk`
  复算。6 个种子、768 个环位置、96 个 `findBiomeHorizontal`、864 个 `isStructureChunk`
  检查全 0 不一致。故意丢掉 `fork()` 的 `nextLong()` 会让环位置大面积不一致。
* **第三轮的步 1（`findValidGenerationPoint` 通用判定，已落地）**：
  `cargo run -p mc-bamboo-vanilla -- structure-validity <java>`，
  `java/StructureValidityOracle.java` 造官方 `Structure.GenerationContext` 直接调三个
  通用判定方法，Rust 用 `structure::validity` 复算。6 种子 × 12 区块、2716 个比较
  0 不一致；反转整列判定、反转起点群系判定两处故意改坏各失败。注意「是否有效」是
  `onTopOfChunkCenter` 基线的通用判定，不是 `Structure.generate` 的最终有效。
* 第 4 步及以后用 `chunkgen` 的 `--through features` 九宫格假世界
  （`docs/worldgen-features-plan.md` 4.1）逐方块比，和 `world-dir` 比存档。
* `.nbt` 模板导出、各结构的 `findGenerationPoint`（结构件生成）、第 4–7 步的主体
  都超出本轮，做不完按 WIP 规则提交并在汇报里交接。

### 7.2 Rust 侧解析的注意点

* `biomes` 字段是 `#minecraft:has_structure/...` 群系标签或 `[id,...]` 列表，要用
  `mc-bamboo-data` 的群系注册表 + 标签表展开成 `BiomeSet`；`possibleBiomes()` 用主世界
  预设（`multi_noise_presets.minecraft:overworld`）的 67 个群系。
* `GenerationStep.Decoration` 的 ordinal 顺序（`surface_structures` = 4、
  `underground_ores` = 6 …）从 26.3 源码固定，版本相关数字/表由生成器导出，不手写。
* 方块状态 / 标签 / `RuleTest` / `IntProvider` 的 JSON 解析沿用 `feature/state.rs`、
  `feature/rule_test.rs`、`feature/provider.rs` 的做法，不重复造轮子。

## 8. 和流水线队员 / 地物队员的接口约定

* `structure_starts` / `structure_references` 在两个分支里先接成**空实现**，函数签名按
  第 3.3 节：`Generator::structure_starts(seed, x, z) -> StructureStarts`、
  `Generator::structure_references(center, neighbor_starts) -> References`，
  输入输出都是纯数据（`StructureStart` 的包围盒 + 类型 + pieces 描述、引用是
  `Vec<i64>` packed chunk pos）。队长合并时把本分支的纯实现接进这两个阶段。
* `feature/run.rs` 主循环：本分支只在每个 step 开头插一处
  `structure::place_step(&mut world, step, ...)` 调用，具体放置逻辑放新文件；
  「先结构后地物」的顺序照 `applyBiomeDecoration` 377–434 行。
* `feature/` 的 `run.rs` / `registry.rs` / `mod.rs`、`chunkgen.rs`、
  `ChunkgenOracle.java`、`Extractor.java`、`Probe.java`、`mc-bamboo-data` 的
  `lib.rs` / `world.rs`、README 是共用文件，改动要小、集中，新增注册表分发分支按
  字母序或类型 ID 插入。

## 9. 风险与疑点

1. **`Structure.DIRECT_CODEC` 能否整表 `encodeStart`**：`spawn_overrides` 引
   `EntityType`、`jigsaw` 引 `StructureTemplatePool`、`template_pool` 的
   `feature_pool_element` 引 `PlacedFeature`，都靠 `RegistryOps`。**本 jar 上已实测通过**
   （四键导出成功、重跑无 diff）；换官方 jar 版本后仍要重跑 `extract` 看 diff，
   失败则退回「从 jar 原始 JSON 导出」并记差异。
2. **`concentric_rings` 的气候采样器上下文**：官方 `findBiomeHorizontal` 走
   `createCachingResolver(randomState)`（`SamplerContext.enableCaches()`），Bamboo 的
   `SeededRouter::climate` 用 `Context::uncached()`。该搜索的 3249 个 quart 坐标互不相同、
   没有 `sampleVolume` 成批调用，所以缓存不会命中、单点值应相同；6 种子 0 不一致
   佐证了这点，但换成别的搜索半径/分辨率时仍要重验。R 树 `lastResult` 官方是
   `ThreadLocal`（ForkJoinPool 多线程）、Bamboo 是 `Cell`，环位置对拍 0 不一致说明
   这批数据没有因平局选不同群系；自定义预设里平局变多时可能露出来。
3. **`probabilityReducer` 的参数方向**：26.3 把 `salt` 当 `x`、`sourceX` 当 `z`、
   `sourceZ` 当 `blend`，和另三个方法不同；当前数据没有用 `default` + frequency<1 的
   结构集，但对拍要覆盖到这条路径（可用合成参数单独验）。
4. **加权选择（已对拍 0 不一致）**：`createStructures` 在 `structures.size() > 1` 时用
   `setLargeFeatureSeed(seed, x, z)` 后 `nextInt(total)` 逐个试；`mineshafts`、
   `villages`、`ruined_portals`、`abandoned_camp` 等是多结构集，9744 个序列比较 0 不一致。
   但官方在第一个「生成成功」的结构就提前返回，Bamboo 目前只能给完整尝试序列；
   最终选中的结构仍要等 `Structure.generate` 有效性判定（未做）。
5. **`Beardifier` 并入 `finalDensity` 的方式（已对拍解决）**：照官方
   `ContextBoundSampler`，`finalDensity` 的 `beardifier` 标记编译成一份固定的
   `Sampler::Beardifier`，求值时从 `Context` 的 user field 取本区块 Beardifier，**不按
   区块重编译**，所以噪声实例、`cache` 编号和取数序列都没变；EMPTY 注入 `None` 时返回
   0.0，和原来 `Sampler::Constant(0.0)` 逐位相同（`chunkgen --through carvers` 仍 0
   不一致）。`beard` 对拍里单点和成批两条路径都逐位比过。**仍存疑**：`first_occupied_height`
   / `iterateNoiseColumn` 固定走 `EMPTY` 是照结构队员疑点 2 的要求，但那批基线路径没有在
   「有结构」的区块上单独和官方对拍过（官方 `findValidGenerationPoint` 用的也是
   `Beardifier.EMPTY`）。
6. **局部 index vs 全局 index**：`features` 阶段结构件用 per-step 局部 `index`，
   地物用 `FeatureSorter` 全局 index，两者 `setFeatureSeed` 的第二参数含义不同。
7. **排除区组合路径未对拍**：`pillager_outposts` 的 `exclusion_zone` 在
   `StructureState::is_structure_chunk` 里实现，靠 `hasStructureChunkInRange` 调
   `villages` 的 `isStructureChunk`（递归深度上限 `MAX_EXCLUSION_DEPTH` 防自定义数据包
   互相排除）。`structures` 对拍只比了无状态的 `getPotentialStructureChunk` 和
   `applyAdditionalChunkRestrictions`，没把这套交互与官方逐格对拍（官方要
   `ChunkGeneratorStructureState` 实例才能调 `applyInteractionsWithOtherStructures`）。
   底层原语单独验过，但组合语义置信度低一档。
8. **环几何与吸附已对拍 0 不一致**：`structure/rings.rs` 照字节码复刻了
   `RandomSource.create()`（LegacyRandomSource）、`setSeed(concentricRingsSeed)`、
   `nextDouble()` 的角度/距离推进、`fork()` 消费 `nextLong()`、`Math.round`、
   `findBiomeHorizontal` 的搜索顺序与蓄水池抽样；6 种子 768 个环位置与官方
   `getRingPositionsFor` 全一致。故意丢掉 `fork()` 的 `nextLong()` 会立刻全错。
   旧的 `placement.rs::ring_positions_geometry`（未吸附的几何草稿）保留但不再用于
   `isStructureChunk`，实际判定走 `rings::ring_positions` 注入的结果。
9. **`findValidGenerationPoint` 通用判定已做，各结构 `findGenerationPoint` 没做**：
   `couldValidBiomeExistOnTopOfChunkCenter`（整列 quart 群系扫描，走
   `createResolverForChunk` 的成批 `sampleVolume`）、`getLowestY` /
   `getMeanFirstOccupiedHeight`（4 角 `getFirstOccupiedHeight` → `getBaseHeight` →
   `iterateNoiseColumn` 的 terrain 密度）和 `isValidBiome`（起点 quart 群系）已在
   `structure/validity.rs` 移植并对拍 0 不一致。但每个结构真正的 `findGenerationPoint`
   （结构件生成、模板、jigsaw）还没移植，所以「是否有效」只是 `onTopOfChunkCenter`
   基线路径的通用判定，`StructureStart` 仍只是数据类型，没有生产它的入口。
10. **整列判定与起点单点判定在这批数据上从未不一致**：`structure-validity` 的
    1584 个判定字段里 `col != pv` 为 0（主世界群系沿 Y 基本恒定，整列有一个合法群系
    ⇔ 起点那格群系合法）。两条路径是独立移植的（成批 `sampleVolume` vs 单点
    `Path::Point`），各自的故意改坏都能被对拍抓到，但一个同时影响两条路径且方向一致的
    错误不会被这组数据发现。换自定义群系预设（Y 方向有分层）时应重验。
11. **组合 `D` 不是完整 `createStructures`**：Java / Rust 的「起点 + 选中的结构 +
    是否有效」组合只按 6 个 `random_spread` 结构集名字顺序 + 加权序列走，没做
    `isStructureChunk` 命中过滤，也不含 `concentric_rings`（要塞）。命中过滤是
    `StructureState::start_plans` 的职责，已在 `structures` 对拍里逐格验过（9280 个
    比较），但这里的组合与它没有拼在一起逐格对拍。

## 10. 已知限制（第五轮结束时，2026-11-14）

* 主世界以外的维度（下界、末地）不做。Bamboo 目前没有下界/末地维度。
* **全是纯函数 + 对拍，没接进游戏**：`structure_starts` / `structure_references` 两个阶段
  仍是空实现，本模块的结果没有进 Director 的流水线，游戏里世界仍是超平坦。
* `.nbt` 结构模板**已导出**（第 2.2 节）：`cb-wg-jigsaw` 分支从 jar 原样导出 1511 个、
  4236847 字节，重跑无 diff，Rust 侧可解析并经官方 `NbtIo` 逐节点对拍 0 不一致。
  20 个多 palette 模板的 `StructurePlaceSettings.getRandomPalette`（按位置
  `RandomSource.create(Mth.getSeed(pos))` 选 palette）尚未移植，结构件生成要用时补。
* **jigsaw 结构件生成（`JigsawPlacement.addPieces` 及 5 类 jigsaw 结构）没做**：`addPieces`
  依赖 `StructureTemplateManager` 加载模板、`Shapes` 的 VoxelShape 布尔空间检查（`ONLY_FIRST`
  / `ONLY_SECOND` / `deflate(0.25)` / `MutableObject` 共享 / `sourceFree` 与 `contextFree`）、
  `Rotation.getShuffled` / `getShuffledTemplates` / `getRandomTemplate` 的随机取数顺序、
  `SequencedPriorityIterator` 的 placementPriority 队列，以及各结构 `findGenerationPoint`
  的 `HeightProvider` / `PoolAliasLookup` / `DimensionPadding` / `LiquidSettings`。官方对拍
  还要把 `Structure.GenerationContext` 的 `structureTemplateManager` 从 `null` 换成真实实例
  （构造器需要 `LevelStorageAccess` 与 `DataFixer`）。这些都没移植，不假装支持。
* `concentric_rings`（要塞）的环几何 + `findBiomeHorizontal` 吸附已移植并对拍
  0 不一致；但要塞的 `isStructureChunk` 必须先由调用方 `set_ring_positions` 注入
  `ring_positions` 的结果，没注入时仍返回 false。`Placement::is_placement_chunk` 单独
  调用 `ConcentricRings` 时恒 false，只有 `StructureState` 这条路径是准的。
* `pillager_outposts` 的 `exclusion_zone` 在 `StructureState::is_structure_chunk` 里实现并
  有单元测试（村庄/前哨不同点），但 `structures` 对拍调的是官方无状态的
  `getPotentialStructureChunk` + `applyAdditionalChunkRestrictions`，**没有把排除区交互
  （`hasStructureChunkInRange`）拿来和官方逐格对拍**；它依赖的 `getPotentialStructureChunk`
  已单独验过，但组合路径置信度低一档，见第 9 节疑点。
* **`Structure.generate` 只做了通用判定**：`findValidGenerationPoint` 的整列群系扫描、
  四角 `getFirstOccupiedHeight` 高度检查、起点群系检查已移植并对拍 0 不一致
  （`structure/validity.rs`、`chunk.rs` 的两个 `Generator` 原语），但「是否有效」是
  `onTopOfChunkCenter` 基线路径的通用判定，不是 `Structure.generate` 的最终有效：
  各结构的 `findGenerationPoint`（结构件生成、模板、jigsaw）没做，`StructureStart`
  仍是数据类型，`start_plans` 只给加权尝试序列。组合对拍只覆盖 6 个 `random_spread`
  结构集、不含 `isStructureChunk` 命中过滤。
* 第五轮已落地并对拍：`structure::single` 的四个代码生成单件结构布局（第 4 步一部分）、
  `mc-bamboo-worldgen::beardifier`（第 6 步，`cb-wg-beard`，见第 5 节）、`structure::save` 的 `structures` 键（第 7 步）。
  仍没做：需要模板的单件结构与 jigsaw 结构件生成（第 4 步其余）、`features` 阶段结构件
  放置（第 5 步，`placeInChunk` / `postProcess` / processors / 方块实体 / 拼图替换），
  以及把全部结构起点接进 Director。这些仍是纯函数/类型，没有接进游戏。
* `structure-validity` 的整列判定与起点单点判定在这批数据上从未不一致（第 9 节疑点
  10）；两条路径各自用故意改坏单独验过。
* `structure_references` 的纯函数 `structure_references` 已就位（含 `ChunkPos.pack`
  与包围盒 XZ 相交），但没接进流水线，也没和官方 `createReferences` 对拍。
* `Structure.DIRECT_CODEC` 整表 `encodeStart` 已在本 jar 上实测通过（四键导出成功、
  重跑无 diff），但换官方 jar 版本后仍要重跑 `extract` 看 diff。
* **Beardifier 对拍的输入不是真实结构起点**：`beard` 的 Java 侧用官方 `Beardifier` 的
  `@VisibleForTesting` 构造器，从一组覆盖五种 `TerrainAdjustment` 的代表性盒 + junction
  造真实 Beardifier，**不是** `Structure.generate` 跑出来的真实结构件——jigsaw 模板导出
  与结构件生成归 jigsaw / 结构 / 多件式结构里程碑。Beardifier 的输入本就只是包围盒 +
  调整类型 + 连接点，所以对密度计算仍是完整逐位对拍；但
  `forStructuresInChunk` 的收集分支（`isCloseToChunk`、RIGID projection、junction XZ
  过滤）只有 Rust 单元测试覆盖，没有和官方 `StructureManager` 端到端对拍。等结构件
  接上后，把 `for_structures_in_chunk` 的产物填进同一套输入即可，并可把 `beard` 的
  输入换成真实生成的结构件再对拍一次。
* 未完成的结构/阶段不假装支持：地物分支用 `unsupported` 标记，结构里程碑沿用同一做法。
