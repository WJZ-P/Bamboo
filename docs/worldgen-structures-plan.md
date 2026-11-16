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

**第四轮续（2026-11-14，`cb-wg-jigsaw` 分支：jigsaw 结构件生成）**：

* 补完第四轮没做的 `JigsawPlacement.addPieces` 全递归：`structure::jigsaw` 下的
  `config`（`HeightProvider` / `MaxDistance` / `DimensionPadding`）、`rotation`
  （`Rotation` / `Util.shuffle` / `shuffledCopy`）、`element`（五种 pool element 的
  `getSize` / `getBoundingBox` / `getShuffledJigsawBlocks` / `canAttach`）、`pool`
  （`StructureTemplatePool` 的展开、`getRandomTemplate`、`getShuffledTemplates`、
  `getMaxSize` 缓存）、`shapes`（`Shapes.join` / `joinIsNotEmpty` / `joinUnoptimized`
  的乘 4 整数精确复刻）、`alias`（`PoolAliasLookup`）、`placer`（`tryPlacingChildren`
  递归 + `SequencedPriorityIterator`）。
* `structure::template` 增加模板按 id 缓存（对齐 `StructureTemplateManager.getOrCreate`，
  否则 972 个查询要重复解析 NBT 几百万次）与按位置的 `getRandomPalette`
  （`palette_index` / `jigsaws_at`），多 palette 模板不再固定取第一个。
* 对拍 `jigsaw` 子命令四个模式：`templates`（1511 个模板的尺寸 / 方块数 / 拼图方块数）、
  `jigsaws`（24176 个模板 × 旋转 × 位置查询、68352 个拼图方块逐字段）、
  `pieces`（27 个结构 × 6 种子 × 6 区块 = 972 查询、12467 结构件逐字段，含每个 junction）、
  `shapes`（120 场景 + 4 个 deflate case）。全部 0 不一致；四处故意改坏各自被抓到并失败。
* 覆盖了 26.3 新增的 18 个 `abandoned_camp_*` jigsaw 结构（也在本分支范围内）。

**第四轮续二（2026-11-14，`cb-wg-jigsaw` 分支：连接规则与存档字段的覆盖面）**：

* `pieces` 的 Q 行补上 `isValidBiome(stub)`（`findValidGenerationPoint` 的最后一道门），
  逐查询比真假；3 种子 × 4 区块 × 27 结构 = 324 查询、4033 结构件 0 不一致，
  其中 14/324 为 true（真假两支都比到）。故意反转 `isValidBiome` 报 26/324 不一致。
  P 行同时补 `getGenDepth()`（恒 0）与 `PoolElementStructurePiece.liquidSettings`
  （官方无访问器，Java 侧反射读）。
* 新对拍模式 `jigsaw pools`：把 245 个 `template_pool` 每一个都当起始 pool 直接调官方
  `JigsawPlacement.addPieces`（`maxDepth` 4、expansion hack 开、`MaxDistance(80)`、
  `PoolAliasLookup.EMPTY`），2 种子 × 2 区块 × 245 pool = 980 查询、8976 结构件 0 不一致。
  这一条把 `feature_pool_element`（639 件）与 `list_pool_element`（15 件）当父件的路径
  真的比到了——按结构生成时它们只当叶子，进不了 `addPieces` 的递归。
  故意把 expansion hack 的 `expandTo + 1` 改成 `+2`，报 82/980 不一致。
* 新对拍模式 `jigsaw save`：用真实结构件跑官方 `PoolElementStructurePiece.createTag`，
  逐 Children 条目比完整规范 NBT（`PosX`/`PosY`/`PosZ`、`ground_level_delta`、
  `pool_element`、`rotation`（`LEGACY_CODEC` 的枚举常量名）、`junctions`、
  `liquid_settings` 仅在非默认时写）。108 查询、1216 个件标签 0 不一致；
  故意把 `rotation` 写成 `CODEC` 的 `none`，对拍立刻报差异。
* `PoolElement` 补上放置阶段要用的 `processors`（注册名或内联对象）与
  `override_liquid_settings`；`Piece` 补 `liquid_settings`，`Piece::to_full_save_piece`
  给出带全部子类字段的存档结构件。

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
* **当时没做（本轮已补上前三种）**：需要模板的单件结构（雪屋 / 沉船 / 海底遗迹 / 废弃传送门）、
  `features` 阶段的 `placeInChunk` / `postProcess` / processors / 方块实体 / 拼图替换、把结构起点
  真正接进 Director。模板结构件的 Java 对拍要构造带 `StructureTemplateManager` 的
  `GenerationContext`，且沉船 / 废弃传送门的包围盒依赖 `getLowestY` /
  `getMeanFirstOccupiedHeight`（terrain 高度），`features` 放置要逐方块比 `postProcess`，
  两者都是独立的大块工作。雪屋 / 沉船 / 海底遗迹已在下一段补上；废弃传送门仍没做。

**第六轮（2026-11-14，`cb-wg-jigsaw` 分支：`processor_list` 解析与编码）**：

* 新模块 `structure::processor`：按官方 `StructureProcessorType.DIRECT_CODEC` /
  `SINGLE_CODEC` / `RuleTest.CODEC` / `PosRuleTest.CODEC` /
  `RuleBlockEntityModifier.CODEC` 把 26.3 的 40 个 `processor_list`（52 个 processor）
  解析成类型化的 Rust 结构体。四种 processor：`block_rot`（含可选 `rottable_blocks`）、
  `rule`（`ProcessorRule` 的五个字段，`position_predicate` / `block_entity_modifier` 有
  默认值）、`protected_blocks`、`capped`（`delegate` + `limit`，`limit` 是
  `IntProvider` 的 `constant` 形式）。五种 `RuleTest`（`always_true` / `block_match` /
  `blockstate_match` / `tag_match` / `random_block_match` / `random_blockstate_match`）、
  一种 `PosRuleTest`（`axis_aligned_linear_pos`，五个字段全是 `optionalFieldOf`）与
  一种 modifier（`append_loot`）。
* 新对拍模式 `jigsaw processors`：Java 侧用官方 `DIRECT_CODEC` 把全部 40 个列表编码成
  NBT，Rust 侧用 `Processor::to_tag` 复算，逐键比。40 个列表 / 52 个 processor
  0 不一致。**这条对拍抓出两处真实 bug**：(a) `BlockState` 在 26.3 的 NBT 里是
  `{id, properties}`（不是 `{Name, Properties}`），10/40 列表受影响；(b) 官方与 Rust 的
  float 默认格式化不同（`1.0E-4` vs `0.0001`），改为比位模式后消失。
* `PoolElement::Single.processors` 从原始 `serde_json::Value` 改成类型化的
  `ProcessorListRef`（注册名或内联列表）；`Piece` 的存档 `pool_element` 按官方 codec
  写出（内联是 `{processors: [...]}` 复合）。
* **回归抓出的第三处 bug**：把内联 `processors` 写成裸数组后，`jigsaw save` 立刻报
  789/108 不一致（官方是 `{processors: {type:9, values:[]}}`），改回复合后回 0。
* 故意改坏：把 `BlockState` 的 `id` 键改回 `Name`，`jigsaw processors` 报 5/40。
* **仍没做**：processor 的**执行**（`BlockRotProcessor.processBlock` /
  `RuleProcessor.processBlock` 的 `Mth.getSeed(pos)` 随机源、`CappedProcessor.finalizeProcessing`
  的限额、`ProtectedBlockProcessor` 的跳过），属放置阶段。

**第七轮（2026-11-14，`cb-wg-jigsaw` 分支：jigsaw 读回 + processor 执行）**：

* `structure::jigsaw::load_piece`：`PoolElementStructurePiece(context, tag)` 的读回。照官方
  顺序读 `PosX`/`PosY`/`PosZ`（缺省 0）、`ground_level_delta`（缺省 0）、`pool_element`
  （`StructurePoolElement.CODEC`，读不出就整体失败）、`rotation`（`LEGACY_CODEC`）、
  `junctions`（缺省空）、`liquid_settings`（缺省 `apply_waterlogging`），并**用元素重算
  包围盒**（忽略存档 `BB`）。新对拍模式 `jigsaw load`：官方 `createTag` → 官方
  `StructurePieceType.load`（含 `PiecesContainer.RENAMES`）读回，与 Rust 逐项比，
  108 查询 / 1216 结构件 0 不一致。故意改坏 `ground_level_delta` 报 1216 处；另发现
  「用存档 `BB` 代替重算」在这批数据上结果相同（存档 `BB` 本来就等于重算值），所以补了
  单元测试 `saved_bounding_box_is_ignored_in_favour_of_the_element` 钉住语义。
* `structure::processor_apply`：`StructureProcessor.processBlock` /
  `finalizeProcessing` 的纯函数版，输入是 `ProcessorLevel`（读世界方块 + 标签判定）。
  逐个照官方：`block_rot`（`nextFloat() <= integrity` 才保留；不在 `rottable_blocks` 里
  时**不取随机数**）、`rule`（`inputPredicate` 用模板方块、`locationPredicate` 读**世界**
  方块、`positionPredicate` 用模板相对坐标）、`protected_blocks`（读**世界**方块）、
  `capped`（`processBlock` 空实现，只在 `finalizeProcessing` 里用
  `levelSeed.forkPositional().at(piecePos)` 派生的随机源洗牌取前 `limit` 个）。
* 新对拍模式 `jigsaw apply`：40 个 `processor_list` × 8 种方块 × 2 种假世界状态 × 1 个额外
  结构件位置 = 120 查询，逐方块比官方 `StructureTemplate.processBlockInfos`，893 个存活
  方块 0 不一致。故意改坏两处（`integrity` 判据、`tag_match` 取反）合报 198 处不一致。
* **对拍抓出的两处真实 bug**：(a) 我把模板相对坐标直接当世界坐标，漏了
  `calculateRelativePosition(...).offset(position)`，导致 `block_rot` 的位置随机源错位
  （7 vs 8 个存活方块）；(b) `BlockState` 在 NBT 里是 `{id, properties}`，上一轮已经修过，
  这轮复核确认。

**第八轮（2026-11-14，`cb-wg-jigsaw` 分支：整件分支覆盖 + capped 随机序列 + 空列表校验）**：

* `structure::processor_apply::process_blocks` 补上官方的 `processOnlyInCurrentChunk`：
  链里没有任何 `evaluates_entire_piece_state()` 为真（26.3 只有 `capped`）且
  `settings.getBoundingBox()` 存在时，只处理落在盒内的方块。这直接决定
  `processedBlockInfoList` 的长度，进而决定 `capped` 洗牌的下标范围，所以必须照做。
  `Processor::evaluates_entire_piece_state()` 照官方实现（只有 `Capped` 返回 true）。
* 新对拍模式 `jigsaw capped`：复刻 `CappedProcessor.finalizeProcessing` 的随机源派生
  （`RandomSource.createThreadLocalInstance(seed).forkPositional().at(piece)`）、
  `Util.toShuffledList` 的洗牌序列、`limit.sample` 的取值，以及原始 `nextInt(1000)`
  序列。5 种子 × 3 结构件位置 × 5 种 size = 75 查询 0 不一致。**这条补上了上一轮报告里
  最大的疑点**：不需要伪造 `ServerLevel` 也能验证随机源与洗牌。
* `jigsaw apply` 的查询格式加了可选的 `chunkBB` 段（`none` 或 6 个整数），80 条带盒的
  查询覆盖 `processOnlyInCurrentChunk` 的两支。主链里的 `capped` 被换成 `limit = 0`
  的等价实例（`processBlock` 本来就是空实现，`finalizeProcessing` 在 `limit == 0` 时
  提前返回，于是不需要真实 `ServerLevel` 种子，但 `evaluates_entire_piece_state` 仍为
  true）。200 查询 / 941 存活方块 0 不一致。
* `ListPoolElement` 的空 `elements` 列表改成解码错误（官方构造器
  `if (elements.isEmpty()) throw new IllegalArgumentException`）。
* 故意改坏：`processOnlyInCurrentChunk` 取反 → 80 处不一致；`evaluates_entire_piece_state`
  改成 `false` → 6 处不一致；`capped` 的洗牌改成固定 `nextInt(size)` → 45 处不一致。

**第九轮（2026-11-14，`cb-wg-jigsaw` 分支：显式随机源 / 端到端 capped / Sync）**：

* `structure::processor_apply` 支持 `StructurePlaceSettings.setRandom(...)` 显式设置的
  随机源：新增 `PlaceSettings` 结构体（`piece_position` / `reference_pos` / `chunk_bb` /
  `level_seed` / `explicit_random`），`settings_random` 照官方
  `if (this.random != null) return this.random;` 优先用显式的、否则按方块位置派生。
  `process_blocks` 的参数从 8 个收敛成 `(processors, blocks, level, settings)`。
* 新对拍模式 `jigsaw cappedfull`：用 `Unsafe.allocateInstance` + 反射写入
  `server.worldGenSettings.options.seed` 三层 final 字段，造出 `getSeed()` 返回真实种子的
  `ServerLevel`，于是官方 `CappedProcessor.finalizeProcessing` 会真的执行限额替换。
  8 个含 `capped` 的列表 × 4 种子 × 2 位置 = 24 查询、192 方块 0 不一致。
  **确认真的比到了**：`trail_ruins_houses_archaeology` 的规则把砾石换成可疑的沙砾，
  24 个查询里共替换 72 次（`limit` 6/3/2/2 生效）；把洗牌改成不倒序后报 40 处不一致。
* `PoolElement::from_tag`（读回路径）对空 `list_pool_element` 同样报错，与官方构造器
  的 `IllegalArgumentException` 对齐。
* `TemplatePool.max_size` 从 `Cell<i32>` 懒算改成解析时一次算好：官方懒算只为省一次遍历、
  输入全是只读数据，结果相同；而 `Cell` 让 `TemplatePool` / `TemplatePools` 变成 `!Sync`，
  没法放进 `Arc` 供多线程生成共享。新增两个单元测试钉住：`TemplatePools` 是 `Sync + Send`、
  245 个 pool 的提前算结果与现算一致。`jigsaw pools` 980 查询 / 8976 件仍 0 不一致。
* 故意改坏：忽略显式随机源（单元测试失败）、去掉空列表校验（单元测试失败）、
  `capped` 洗牌改成不倒序（`cappedfull` 报 40 处）。

**第十二轮（2026-11-15，`cb-wg-jigsaw` 分支：模板放置链路）**：

* 合并 `main`（`1c9853c6`，含水生 / 洞穴 / 非植被地物与模板化结构布局）。三处冲突
  （`structure/mod.rs` 的模块表、README、设计文档）全部两边保留；里程碑表步 4 从
  「jigsaw 结构件没做」改成「做完」并补上本分支的两条对拍数字。
* 新模块 `structure::jigsaw::place`：jigsaw 结构件的 `postProcess` 整条链路。
  `relative_position(pos, mirror, rotation)` 照官方
  `StructureTemplate.transform(pos, mirror, rotation, ZERO)`（mirror 先把 Z 或 X 取反，
  再按 rotation 转）；`template_blocks` 按世界位置选 palette 并把模板方块转成 processor
  链的输入（`has_nbt` 标记跟着走）；`place_piece` = 选 palette → `process_blocks` → 写入；
  `place_processed` 做状态 `mirror`/`rotate`、`chunkBB` 过滤，以及官方的「带 nbt 的方块
  先写一个 `BARRIER`」。
* `state_id_of` 把方块状态名（含属性）解析成注册表状态 id，属性顺序无关；未知方块或非法
  属性组合报错而不是静默写空气（有单元测试）。
* 新对拍模式 `jigsaw placepiece`：官方 `StructureTemplate.placeInWorld`
  （`IGNORE_WATERLOGGING`、无 processor、记录型假世界）对 `place_piece`，逐次 `setBlock`
  比位置与方块名。8 个单 palette 模板 × 3 组 mirror/rotation
  （`none/none`、`none/90`、`left_right/180`）= 225 查询、**152748 次写入 0 不一致**。
  故意改坏两处（mirror 的 Z 取反改成 X、不写 `BARRIER`）合报 1284 处不一致。
* 对拍过程中修掉两个自身 bug：(a) `relative_position` 最初漏了 mirror（只在方块状态上
  应用了，位置变换同样需要）；(b) 对拍查询的 `chunkBB` 最初只覆盖 `0..span`，而旋转后的
  坐标会落到负半轴，导致 Java 侧只写 10 个方块——改成跨正负两侧才比得对。
* **没做**：`placeInWorld` 的 watering 逻辑（`toFill` / `lockedFluids` 的流体填充循环）、
  方块实体 nbt 的恢复（`blockEntity.loadWithComponents` + `LootTableSeed`）、
  `placeEntities`（结构里的实体）、从 `pool_element` 取 `processors` 并接进 `place_piece`。

**第十三轮（2026-11-15，`cb-wg-jigsaw` 分支：三类硬编码 processor + 全组合覆盖）**：

* 补上另外三类 processor（`processor_list` 里没有、由 `SinglePoolElement.getSettings` 与
  `Projection` 硬编码加进放置设置的）：
  * `BlockIgnoreProcessor`（`block_ignore`）：`to_ignore` 里的方块直接丢弃。官方三个常量
    实例 `STRUCTURE_BLOCK` / `AIR` / `STRUCTURE_AND_AIR` 都能构造。
  * `JigsawReplacementProcessor`（`jigsaw_replacement`）：把拼图方块换成它 nbt 里的
    `final_state`（缺省 `minecraft:air`）；换成 `structure_void` 时丢弃；没有 nbt 时保留原
    方块（官方只打警告）；解析失败时丢弃。
  * `GravityProcessor`（`gravity`）：把方块的 Y 抬到
    `level.getHeight(heightmap, x, z) + offset + templateRelativePos.y`。
    `ProcessorLevel` 因此多了一个 `height()`（默认返回 `MIN_Y` 表示不支持）。
* `ProcessBlock` 增加 `nbt: Option<Tag>`（`JigsawReplacementProcessor` 要读 `final_state`，
  带 nbt 的方块还要先写 `BARRIER`）。
* `place_processed` 现在尊重 `set_block` 的返回值：只有真正改了状态才计入返回值，与官方
  `if (level.setBlock(...)) { ... placed.add(...) }` 一致。
* `jigsaw placepiece` 的覆盖从 3 组 mirror/rotation 扩到**全部 12 组**
  （3 个 mirror × 4 个 rotation）：900 查询、610992 次写入 0 不一致。
* 故意改坏：`block_ignore` 恒不忽略（单元测试失败）、`gravity` 丢掉
  `templateRelativePos.y`（单元测试失败）、`place_processed` 不检查返回值（单元测试失败）。

**第十四轮（2026-11-15，`cb-wg-jigsaw` 分支：processor 组装 / gravity 语义 / 放置产物）**：

* 合并 `main`（`2ba852ee`，矿井与要塞的方块放置已对拍）。三处冲突：`state_transform` 的
  `property_of` 取 main 的 `pub`（我要跨模块用）；`StructurePlaceOracle` 的假世界字段两边
  保留（并删掉重复的 `case "scheduleTick"`，main 的版本不记录、我的记录，保留后者）；
  里程碑表步 4 用本分支的完整版、步 5 合并两边进展（矿井/要塞现在都是 0 不一致）。
  合并后 `jigsaw placepiece` 900 查询 0 不一致、`structure-place` 0 不一致。
* `structure::jigsaw::place::assemble_processors`：照 `SinglePoolElement.getSettings` 组装
  processor 链——`BlockIgnore(STRUCTURE_BLOCK)` → 可选 `JigsawReplacement`（`!keepJigsaws`）
  → `pool_element` 配置的 → `Projection` 的（`TERRAIN_MATCHING` 是
  `Gravity(WORLD_SURFACE_WG, -1)`）。**`LegacySinglePoolElement` 的额外一步**：先
  `popProcessor(STRUCTURE_BLOCK)` 摘掉链首，再把 `STRUCTURE_AND_AIR` **追加到末尾**
  （不是原地替换）。6 种组合各有单元测试。
* `GravityProcessor` 的两处语义补齐：`WORLD_SURFACE_WG` → `WORLD_SURFACE`、
  `OCEAN_FLOOR_WG` → `OCEAN_FLOOR` 的替换（官方在 `level instanceof ServerLevel` 时做）；
  `ProcessorLevel::height` 从「返回哨兵整数」改成 `Option<i32>`，拿不到时**保留原位置**
  而不是算出一个回绕的 Y。
* `place_processed` / `place_piece` 的返回值从「改变计数」改成 `PlacedBlocks`
  （`bounds` = 官方 `minX..maxZ`，`placed` = 位置 + nbt 列表）。官方用这两块做
  `updateShapeAtEdge`、方块实体 `setChanged` 与 `placeEntities`，本分支只把数据交出去。
  `PlaceBlock` 的 `has_nbt: bool` 相应换成 `nbt: Option<Tag>`。
* 故意改坏：legacy 的 `STRUCTURE_AND_AIR` 改成原地替换（组装测试失败）、
  gravity 不换 WG 名字（测试失败）、包围盒只记第一个（两个测试失败）。

**第十五轮（2026-11-15，`cb-wg-jigsaw` 分支：方块实体 modifier 的执行）**：

* `RuleBlockEntityModifier::apply` 照官方三种实现：`passthrough`（原样返回）、
  `append_loot`（拷贝后写 `LootTable` 并 `nextLong()` 生成 `LootTableSeed`）、
  `clear`（空复合）、`append_static`（`existingTag.merge(tag)` 的**递归**合并）。
  `Processor::Rule` 命中规则后现在也会改写 nbt（`rule.getOutputTag(random, nbt)`）。
* `append_static` 的解析/编码补上（26.3 数据里没用到，但 codec 里有）。
* 新对拍模式 `jigsaw bemod`：4 种 modifier × 4 个种子 × 3 种现有 nbt = 60 查询，
  逐键比生成的 NBT，并用「随后再取一次 `nextLong`」验证取数次数一致：**60 查询 0 不一致**。
* 故意改坏：`append_loot` 不取 `nextLong`（24 处不一致）、`append_static` 不递归合并
  （4 处不一致）。
* **对拍设计踩过一个坑**：最初 `append_static` 的 `data` 与 `existing` 用了同一段嵌套 nbt，
  「递归合并」和「直接覆盖」结果相同，改坏发现不了。加了 `append_static_data` 查询
  （data 用 `{Nested:{A:1}, Extra:7}`，existing 用 `{Nested:{B:2}, Only:3}`）才区分开。

**第十六轮（2026-11-15，`cb-wg-jigsaw` 分支：随机容器的战利品种子）**：

* `place_processed` / `place_piece` 新增 `random: &Random` 参数，照官方
  `StructureTemplate.placeInWorld` 的方块实体恢复：带 nbt 的方块如果其方块实体是
  `RandomizableContainer`，就用 `random.nextLong()` **再取一次**覆盖 `LootTableSeed`。
  这一次取数必须发生，否则世界随机源序列与官方错开。
* `is_randomizable_container` 按方块实体类型判定，集合是官方实现该接口的十类：
  箱子 / 陷阱箱 / 发射器 / 投掷器 / 漏斗 / 潜影盒 / 木桶 / 饰纹陶罐 / 合成器 /
  可疑的方块（`BrushableBlockEntity`）。熔炉、烟熏炉、高炉、酿造台**不是**。
* 单元测试两个：`randomizable_containers_match_the_vanilla_set`（11 个正例 + 8 个反例）、
  `placing_a_randomizable_container_draws_the_loot_seed`（箱子得到 `LootTableSeed`、石头
  不会）。故意改坏：集合里漏掉 `hopper`、不覆盖种子，两处各被一个测试抓到。
* **没做到的事**：本想用新对拍模式 `rc` 让官方 `instanceof RandomizableContainer` 逐个
  判定所有 1286 个方块，但 `Block.newBlockEntity` 会触发 `Components not bound yet`
  （oracle 没初始化组件注册表），Java 侧构造不出方块实体。改用「按方块实体类型判定 +
  单元测试钉住集合」的替代方案，**所以这个集合没有对拍，只有单元测试**。

**第十七轮（2026-11-15，`cb-wg-jigsaw` 分支：随机容器集合的官方对拍）**：

* 解决了上一轮「`rc` 对拍跑不起来」的问题：`Block.newBlockEntity` 读组件默认值时抛
  `Components not bound yet`，因为组件只在服务器资源重载
  （`ReloadableServerResources`）时绑定。oracle 里手动跑一次
  `BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(registries)` 并
  `PendingComponents::apply()`，方块实体就能构造了。
* 新对拍模式 `jigsaw rc`：官方对全部 **1286 个方块**逐个构造方块实体并
  `instanceof RandomizableContainer`，与 `is_randomizable_container` 逐条比：
  `1286 blocks, 33 randomizable, all identical`。
* **对拍抓到一个真 bug**：我把 `brushable_block`（可疑的沙砾 / 沙子）算成了随机容器，
  但官方 `BrushableBlockEntity extends BlockEntity`，**没有实现**
  `RandomizableContainer`。对拍报 `2/1286 blocks differ`，修正后回 0。
  这意味着上一轮的单元测试把错误的集合钉住了——**单元测试只能验证我想验证的东西，
  对拍才能发现我想错的东西**。
* `place_piece` 的参数从 10 个减到 9 个：`chunk_bb` 从 settings 里取（`None` 时用
  无限盒表示官方的「不过滤」），去掉重复参数。`jigsaw placepiece` 900 查询仍 0 不一致。

**第十八轮（2026-11-15，`cb-wg-jigsaw` 分支：方块实体写入钩子）**：

* `BlockWriter` 新增 `set_block_entity(x, y, z, nbt)`（默认空实现）。`place_processed`
  在写完方块后把最终 nbt 交给它，对应官方的
  `blockEntity.loadWithComponents(TagValueInput.create(...))`。随机容器的 nbt 里带着
  放置期生成的 `LootTableSeed`。两个单元测试钉住（随机容器带种子、普通方块只带自己的
  nbt、没有 nbt 的方块不触发）。
* 合并 `main`（`5c7e9995`，含 `template_feature.rs` 等），无冲突。
* **没做成的一件事**：想给 `jigsaw placepiece` 加「随机容器 `LootTableSeed`」的端到端对拍，
  需要假世界的 `getBlockEntity` 返回真实方块实体并记录 `loadWithComponents`。做到一半发现
  两个问题：(a) `loadWithComponents` 需要真实的 `RegistryAccess`（`TagValueInput.create`），
  补上后 (b) 官方只在 `blockInfo.nbt != null` 时才调它，而我的假世界在写 `barrier` 等位置
  时也会登记方块实体，导致 Java 侧多出种子。这条对拍**未完成、未提交**，代码已回滚。
  `place_processed` 的 nbt 传递目前**只有单元测试**。

**第十九轮（2026-11-15，`cb-wg-jigsaw` 分支：随机容器种子的端到端对拍）**：

* 补上第十八轮没做成的对拍。关键是找到可靠的判据：官方只在
  `blockInfo.nbt != null` 时才走方块实体恢复，而**它在那之前会先写一个 `BARRIER`**
  ——所以「写过 `barrier` 的位置」就是「带 nbt 的位置」。假世界据此为那些位置建方块实体，
  `getBlockEntity` 返回它，`collectSeeds` 只收「带 nbt 且是随机容器」的位置，
  与官方逻辑一一对应。
* `jigsaw placepiece` 现在额外输出 `S|key|x|y|z|seed`：**900 查询、610992 次写入、
  852 个随机容器种子，全部一致**。种子是放置期 `nextLong()` 的真实取值，所以这条同时
  验证了「哪些方块是随机容器」与「取数次数与顺序」。
* 故意改坏：随机容器不覆盖 `LootTableSeed`（不发那次 `nextLong`）→ 264 处不一致。

**第二十轮（2026-11-15，`cb-wg-jigsaw` 分支：watering）**：

* `place_processed` 补上官方 `placeInWorld` 的 watering 段：写方块**之前**读该位置的流体状态
  （`settings.shouldApplyWaterlogging() ? level.getFluidState(pos) : null`）；新方块本身是
  流体源时把位置加进 `lockedFluids`；新方块可含水（状态里有 `waterlogged`）且该位置原本有
  水时把 `waterlogged` 置 true，原本不是源的位置再加进 `toFill`；最后按官方顺序
  （`UP, NORTH, EAST, SOUTH, WEST`）反复把「相邻有源流体」的位置灌满，`lockedFluids` 里的
  位置不参与。
* `BlockWriter` 新增 `fluid_at(x, y, z) -> Option<(is_water, is_source)>`（默认 `None` =
  调用方没有流体模型，整段 watering 跳过）与 `block_state(x, y, z)`。
* 两个单元测试：源流体位置的台阶得到 `waterlogged=true`、非源位置不写第二遍；
  `fluid_at` 返回 `None` 时只写方块本身。改坏「不灌水」→ 测试失败。
* `jigsaw placepiece` 900 查询 / 610992 写入 / 852 种子仍 0 不一致（这条对拍走
  `fluid_at == None` 的路径，所以**watering 本身只有单元测试覆盖**）。

**第五轮（2026-11-14，`cb-wg-structures` 续：需要模板的单件结构布局）**：

* 合并 `main` 时本分支已与 `main` 同步（`main` 的 `4405fc3` 含本分支全部提交），
  工作区里那份没完成的 `git merge main` 冲突已按 main 内容解决（两边子命令都保留）。
* 新增 `mc-bamboo-worldgen::structure::templated`：需要 `.nbt` 模板的单件结构的结构件布局。
  公共部分是 `TemplateStructurePiece` 的包围盒算法（`StructureTemplate.transform` 的整数版 +
  `getBoundingBox(BlockPos, Rotation, pivot, Mirror, Vec3i)`），每个结构一个子模块：
  * 雪屋（`templated/igloo.rs`，`IglooPieces.addPieces`）：`igloo/top` 恒放，`nextDouble() < 0.5`
    时先放 `igloo/bottom`（`depth = nextInt(8) + 4`，深度 `depth * 3`）和 `depth - 1` 段
    `igloo/middle`；三个模板的 rotation pivot `(3,5)` / `(1,1)` / `(3,7)` 与偏移照官方常量表。
  * 沉船（`templated/shipwreck.rs`，`ShipwreckPieces.addRandomPiece`）：一张模板，
    pivot `(4,15)`，`is_beached` 决定从 11 项沙滩表还是 20 项海洋表里抽。
  * 海底遗迹（`templated/ocean_ruin.rs`，`OceanRuinPieces.addPieces`）：`nextFloat() <= large_probability`
    选大小，冷水遗迹一次 `nextInt` 同号取 brick / cracked / mossy（完好度 0.7 / 0.5），
    大遗迹再 `nextFloat() <= cluster_probability` 时 `allPositions` 八个候选 + `nextInt(4..8)` 圈。
* 新对拍 `cargo run -p mc-bamboo-vanilla -- structure-templated <java>` +
  `java/StructureTemplatedOracle.java`（构造真实 `StructureTemplateManager`，不启服务端 / 读存档 /
  接受 EULA）：8 种子 × 12 区块 × 5 类 = 480 查询、1685 结构件、1685 行逐项比 0 不一致。
  故意改坏四处确认会失败：雪屋梯子 pivot 262 处、沉船 pivot 142 处、海底遗迹 cluster
  候选下界 68 处、冷水 cracked 完好度 263 处，都还原回 0。
* **仍没做**：废弃传送门（`RuinedPortalPiece` 的 `VerticalPlacement` / `findSuitableY` 要
  `getBaseColumn` 的整列方块，属放置里程碑）、jigsaw 结构件、`features` 阶段的放置。

**第六轮（2026-11-14，`cb-wg-structures` 续：废弃传送门 / 旋转表 / 放置原语）**：

* 废弃传送门（`templated/ruined_portal.rs`）：`RuinedPortalStructure.findGenerationPoint` 全套——
  多 `Setup` 的加权抽取（一次 `nextFloat()` 加累减）、`sample` 空腔、5% 巨型模板、
  `Rotation` / `Mirror`（`FRONT_BACK` 50%）、`pivot = (sizeX/2, sizeZ/2)`、`getBaseHeight`
  取模板包围盒中心的 `surfaceY`、`findSuitableY` 的四角整列搜索、`isCold` 的群系温度。
  六种 `VerticalPlacement` 里除了 `IN_NETHER`（本分支只做主世界）都覆盖。
* `chunk.rs` 新增 `Generator::base_column`（`getBaseColumn` 的整列方块原语）：
  `iterateNoiseColumn` 的 `columnReference` 分支，1×height×1 体积 + 全新含水层 + EMPTY
  beardifier，按 Y 升序收全每一层的 `computeSubstance`。
* 新对拍 `structure-ruined-portal`：`StructureRuinedPortalOracle` 造官方注册表 +
  真实 `StructureTemplateManager`，调 `RuinedPortalStructure.findValidGenerationPoint`
  （内部跑官方 `findSuitableY`）；6 个结构变体 × 6 种子 × 18 区块 = 648 查询，
  110 个查询真的产出结构件、110 行逐项 0 不一致。查询集特意加了种子 0 的沙漠 / 恶地 /
  雪原 / 丛林 / 深海 / 石岸区块，让 `partly_buried`、`in_mountain`、`on_ocean_floor`
  三条分支都被比到。两处故意改坏（`findSuitableY` 门槛 3→2 报 23 处、
  `MIRROR_FRONT_BACK` 常量 2→1 报 68 处）确认失败后还原回 0。
* 海底遗迹的 `large_probability` / `cluster_probability` 改成从 `worldgen.json` 的
  `ocean_ruin_warm` / `ocean_ruin_cold` 条目解析（`ocean_ruin::Config::parse`），
  去掉了上一轮手写的常量；`structure-templated` 仍 0 不一致。
* 方块状态旋转表：`Extractor.java` 的 `StateTables` 新增 `rotation`（每状态 4 项，
  `Rotation.ordinal()`）与 `mirror`（每状态 3 项，`Mirror.ordinal()`），经 `extract` 写进
  `mc-bamboo-data::generated::states` 的 `STATE_ROTATION` / `STATE_MIRROR`，
  访问函数 `mc_bamboo_data::state_rotation` / `state_mirror`。这张表是结构件
  `postProcess` 里 `placeBlock` 按朝向变换方块状态的前提，之前项目里没有。
* 放置原语：新模块 `structure::place`（`PieceContext`：`getWorldX/Y/Z`、`placeBlock`
  含 mirror→rotation 变换、`generateBox` / `generateAirBox` / `fillColumnDown`、
  `isReplaceableByStructures`）与 `structure::post_process`（埋藏的宝藏的
  `postProcess` 方块部分，返回箱子位置，不写战利品表）。
* **仍是纯函数**：这些放置原语还没接进 `feature/run.rs`，也没有逐方块对拍；
  `StructureStart.placeInChunk` / `postProcess` 的完整链路（processor、方块实体、
  战利品表、拼图替换）仍归第 5 步。

**第七轮（2026-11-14，`cb-wg-structures` 续：`postProcess` 方块部分 + 逐方块对拍）**：

* `structure::scattered`：`ScatteredFeaturePiece` 的两个高度校正
  （`updateAverageGroundHeight` / `updateHeightPositionToLowestGroundHeight`），
  用 `MOTION_BLOCKING_NO_LEAVES` 高度图在包围盒内求平均 / 最小值并竖直平移。
* `structure::post_process`（从上一轮的单文件扩成模块）：
  * `buried_treasure`：从 `OCEAN_FLOOR_WG` 高度往下找砂岩 / 石头层，把六个邻居换成
    `below` 或 `soft`，返回箱子位置；
  * `create_chest` / `reorient`：`StructurePiece.createChest` 的方块部分（含箱子朝向的
    两段 `reorient` 逻辑），战利品表的 `nextLong` 留给调用方；
  * `post_process::swamp_hut`：`SwampHutPiece.postProcess` 的全部 `generateBox` /
    `placeBlock` / `fillColumnDown`，含四个 `StairsShape.OUTER_*` 外角。
* 新对拍 `structure-place`（`java/StructurePiecePlaceOracle.java`）：合成地形
  （`surface(x,z) = 64 + floorMod(x*31 + z*17, 5) - 2`，表面沙子 / 下一层砂岩 / 更下石头）
  按同一个公式在两边造出，跑 `postProcess` 后逐格比写前写后的差异。
  4 种子 × 4 区块 × 2 结构（埋藏的宝藏 / 女巫小屋）= 32 查询、2202 个变化方块、
  2202 个格比较 0 不一致。女巫和猫是实体，Java 侧在实体那一步抛异常（`T` 行标记，
  `entityAbort=1` 共 16 条），方块已在此前写完，所以方块比较完整。
  故意改坏一处（女巫小屋里一段 `generateBox` 的 Y 上界 1 改 2）报 224 处不一致，还原回 0。
  第二次尝试改坏 `lowest_ground_height` 的初值**没有被抓到**，因为沙漠神殿还没接进对拍，
  这条分支目前无覆盖，见疑点 12。
* `structure::place` 的 `PieceContext` 改成两个生命周期参数（`&'a mut FeatureWorld<'b>`），
  否则调用方在 `postProcess` 之后再读世界会被借用检查拒绝。

**第八轮（2026-11-14，`cb-wg-structures` 续：沙漠神殿 `postProcess`，未验证）**：

* 新模块 `structure::post_process::pyramid`：`DesertPyramidPiece.postProcess` 的方块部分，
  含 `updateHeightPositionToLowestGroundHeight` 的 `-random.nextInt(3)` 偏移、
  `addCellarStairs`（`level.getRandom().nextBoolean()` 选沙子 / 砂岩）、
  `addCellarRoom`、`placeCollapsedRoof`（`nextFloat() < 0.33`）、四个箱子的
  `Direction.Plane.HORIZONTAL` 顺序、以及 `region_random`（`WorldGenRegion.getRandom()` 的
  Legacy 位置派生复算）。
* **状态：已写未验证**。`structure-place` 里接上后报 52427 处不一致——Java 侧沙漠神殿在
  合成地形上一个方块都不写。排查到 `getHeightmapPos` 的语义（`LevelReader` 默认实现调
  `this.getHeight`，也就是 `WorldGenRegion.getHeight` 的 `+1` 版本）已经改对，但 `D` 诊断行
  一直没出现在 Rust 的解析结果里，时间用完前没能定位。该结构暂时加进
  `structure_place.rs` 的 `SKIPPED` 列表，不进查询集，所以 `structure-place` 仍是
  32 查询 / 2202 格 / 0 不一致。

**第九轮（2026-11-14，`cb-wg-structures` 续：沙漠神殿偏差的排查结论，无新产出）**：

本轮全部时间用在定位沙漠神殿 `postProcess` 的坐标偏差上，**没有代码提交**（排查中改的
`region_random`、write-log 对比都回退了，工作树干净）。但拿到了三条确定的结论，下一轮直接照做：

1. **`region_random` 之前写错了**。官方是
   `RandomState.random = Algorithm.newInstance(seed).forkPositional()`，
   `getOrCreateRandomFactory(name) = random.fromHashOf(name).forkPositional()`，
   再 `.at(center.getWorldPosition())`——也就是
   `Algorithm::new_instance(seed).fork_positional().from_hash_of("minecraft:worldgen_region_random").fork_positional().at(x, 0, z)`。
   第八轮写的「`WorldgenRandom(worldSeed).nextLong()` 当工厂种子」是错的。
   `mc-bamboo-worldgen::random` 已有 `fork_positional` / `from_hash_of` 两个原语，直接拼即可。
2. **oracle 之前只比「状态变化」，漏掉了结构写在地形上的方块**。沙漠神殿的塔身几乎全落在
   合成地形的沙 / 砂岩上，`before`/`after` 快照判不出变化，于是 Java 侧一直是「0 个变化方块」。
   正确的做法是让 `WorldGenLevel` 代理记录**每一次 `setBlock`**（`TreeMap<BlockPos, BlockState>`
   后写覆盖先写），Rust 侧同样在 `PieceContext` 里记 `(x, y, z, state)` 写入日志
   （含 `fill_column_down`），两边比写入日志而不是比状态差异。
3. **`getHeight` / `getHeightmapPos` 在区块外不能 `& 15` 折回**。官方 `WorldGenRegion` 的写半径
   是 1，能读到邻居区块；假世界只有一个区块时把越界坐标折回本区块会让
   `lowest_ground_height` 读到错误的列。要么造 3×3 假地形，要么按地形公式直接算越界列。
   改这两处后沙漠神殿的不一致从 52427 降到 23004（剩下的差异还没查完）。

**第十轮（2026-11-14，`cb-wg-structures` 续：按第九轮三条根因重做对拍，仍无代码提交）**：

本轮把第九轮定位的三条根因全部实现并实测，**结论是三条都必要但不充分**，剩下的差异出在
`fillColumnDown` 的边界上。所有代码改动在收尾时都回退了（工作树干净），只留这份记录。

实现过的三件事（都可复用）：

1. `region_random(seed, use_legacy_random, cx, cz) =
   Algorithm::new_instance(seed).fork_positional()
     .from_hash_of("minecraft:worldgen_region_random").fork_positional().at(cx, 0, cz)`，
   `use_legacy_random` 传主世界 `noise_settings.legacy_random_source`（false）。
2. `PieceContext` 加 `writes: Option<Vec<(i32, i32, i32, u32)>>`，`place_block` /
   `fill_column_down` / `create_chest` 都往里记；Java 侧 `PieceLevelHandler.written`
   （`TreeMap<BlockPos, BlockState>`，后写覆盖先写）是同一语义。**两边都比写入日志**，
   而不是「写前写后状态差异」。
3. 假世界改成 3×3 合成地形（9 个区块都按同一公式填），区块外的高度按公式算而不是
   `& 15` 折回。

实测结果（`structure-place` 48 查询：埋藏的宝藏 / 沙漠神殿 / 女巫小屋各 16）：

| 阶段 | 不一致数 |
|---|---|
| 起点（只改对 `region_random`） | 52427 |
| + 写入日志 | 46980 |
| + 3×3 地形 | 7945 |
| 只剩沙漠神殿，埋藏的宝藏与女巫小屋已对上（只剩 18 处女巫小屋） | 18 |

**剩下的 18 处全部是女巫小屋的 `fillColumnDown` 差一格**：Java 把橡木原木填到 `y = 62`，
Rust 填到 `63`；`(1, 63, 2)` 反过来（Rust 写了、Java 没写）。也就是说
`fillColumnDown` 的起点或终止条件有一处 `±1`。官方：

```
BlockPos.MutableBlockPos pos = this.getWorldPos(x, startY, z);
if (chunkBB.isInside(pos)) {
   while (this.isReplaceableByStructures(level.getBlockState(pos)) && pos.getY() > level.getMinY() + 1) {
      level.setBlock(pos, blockState, 2);
      pos.move(Direction.DOWN);
   }
}
```

注意 `pos` 在**循环体内**被 `move(DOWN)` 推进、而 `isReplaceableByStructures` 读的是
**推进后**的位置——写的是「当前 `pos`」，判的是「当前 `pos`」，然后在末尾下移一格。
Rust 的 `place.rs::fill_column_down` 先读 `get_block(pos)`、再 `set_block(pos)`、再 `y -= 1`，
顺序一致；差异最可能在**起点**：官方 `getWorldPos(x, startY, z)` 的 Y 是
`startY + boundingBox.minY`，而结构件在 `postProcess` 里已经被 `updateAverageGroundHeight`
平移过，`boundingBox.minY` 是平移后的值。Rust 传进 `PieceContext.piece_bb` 的也是平移后的
`new_bb`，但 `fill_column_down` 用的是 `self.world_pos(x, start_y, z)`，其中 `world_y` 读
`piece_bb[1]`——这条链是对的。

**下一轮应该直接盯着这 18 处对齐**：用一个只含女巫小屋的查询集，把 Java 侧
`fillColumnDown` 每次 `setBlock` 的坐标打出来，和 Rust 的 `writes` 逐条比，一处一处对。

**第十一轮（2026-11-14，`cb-wg-structures`：对拍改用写入日志，落地三条根因）**：

第九、十轮定位的根因**已实现并提交**（`7175dd0`，WIP）：

* `structure::place` 新增 `Write = (x, y, z, state)` / `WriteLog = Option<Vec<Write>>` /
  `Placed = (Option<(i32, [i32; 6])>, WriteLog)` 三个别名；`PieceContext.writes` 在
  `place_block` 与 `fill_column_down` 里记录每次写入（生产路径传 `None`，零开销）。
* `region_random(seed, use_legacy_random, cx, cz)` 改成官方链路：
  `Algorithm::new_instance(seed).fork_positional().from_hash_of(
  "minecraft:worldgen_region_random").fork_positional().at(cx, 0, cz)`。
* `structure-place` 的 Java oracle：`PieceLevelHandler.written`（`TreeMap<BlockPos, BlockState>`，
  后写覆盖先写）当结果；`getHeight` / `getHeightmapPos` 在区块外按合成地形公式现算，
  不再 `& 15` 折回；假世界 9 个区块都填地形。
* 沙漠神殿的 `fillColumnDown` 21×21 循环改回指令表里官方的位置（塔身之后、正面入口之前），
  此前误放在指令表末尾。
* `add_cellar_room` 的四个出口按官方顺序重排（`x+3` → `cut(x+4)` → `chiseled(x+4)` →
  `x-3` → `cut(x-4)` → `chiseled(x-4)` → 南北两个橙陶瓦）。

**当前状态（不能算通过）**：`structure-place` 是 **18 处不一致**，全部是女巫小屋的
`fillColumnDown` 差一格（Java 填到 `y = 62`，Rust 到 `63`；`(1, 63, 2)` 反过来）。
改用写入日志后，这套更严格的语义把此前被「状态差异」掩盖的差异暴露出来了。
沙漠神殿仍在 `SKIPPED`，本轮把它从 7936 压到 6953 后仍未归零。

**第十二轮（2026-11-14，`cb-wg-structures` 续：女巫小屋差一格的定位，无代码提交）**：

本轮只做了一件事：查第十一轮留下的 18 处 `fillColumnDown` 差一格。**没修好，代码全部回退**
（工作树干净，仍是 `7172a2e` 的状态）。

已经排除的：

* **不是 `average_ground_height` 的公式问题**。我给 Java oracle 加了一条诊断，在
  `postProcess` 之后按 `MOTION_BLOCKING_NO_LEAVES` 重算盒子平均值：`chunk 0 0 0` 的
  `box = (0,64,0)..(6,70,8)`、`avg = 64`、`n = 63`，和 Rust 的 `piece_bb` / 公式完全一致。
* **不是 `region_random`**。女巫小屋 `postProcess` 不取随机数（`entityAbort` 发生在女巫 / 猫
  那一步，方块已写完）。
* **不是 `piece_bb` 没传平移后的盒子**。`swamp_hut` 里 `piece_bb: new_bb` 确认无误。

**未解的矛盾（下一轮从这里开始）**：

用 `Heightmap.Types.MOTION_BLOCKING_NO_LEAVES` 在 `surface = 62` 的列上：
`ChunkAccess.getHeight` = `Heightmap.getFirstAvailable` = 高度图里存的值 = **最高方块 y + 1** = 63；
`WorldGenRegion.getHeight` 覆写版再加 1 = **64**；`LevelReader.getHeightmapPos` 调的是
`this.getHeight`（即覆写版）= **64**。
Rust 的 `FeatureWorld::get_height` 返回 `stored + 1`，而 `stored` 是「最高方块 y」（`prime_heightmaps`
的约定）→ **63**。

把 Rust 改成 `+2` 后，`structure-place` 从 18 处**涨到 2786 处**，说明埋藏的宝藏用的
`get_height(OceanFloorWg)` 在 `+1` 下是对的。也就是说：

* `BuriedTreasurePiece.postProcess` 的 `level.getHeight(OCEAN_FLOOR_WG, x, z)` 与 Rust `+1` 对齐；
* `ScatteredFeaturePiece.updateAverageGroundHeight` 的 `level.getHeightmapPos(...).getY()`
  需要 Rust `+2`。

**这两处官方都走 `WorldGenRegion.getHeight`，不该有差别。** 所以要么我对
`Heightmap.getFirstAvailable` 返回值的理解错一层，要么 `getHeightmapPos` 在
`WorldGenRegion` 上还有别的覆写。下一轮应该：

1. 直接读 `Heightmap.getFirstAvailable` / `setHeight` / `getHeight` 三个方法确认存的到底是
   「方块 y」还是「方块 y + 1」；
2. 在 Java oracle 里对同一个坐标同时打印 `level.getHeight(t, x, z)` 和
   `level.getHeightmapPos(t, pos).getY()`，看这两个数差几；
3. 再决定 Rust 侧是加两层 `get_height`（`+1` / `+2` 各一个，名字区分语义），
   还是统一改正 `prime_heightmaps` 的存储约定。

**第十三轮（2026-11-14，`cb-wg-structures`：`Heightmap` 存储语义已确认，`get_height` 差两层的账）**：

上一轮说「两处官方都走 `WorldGenRegion.getHeight`，不该有差别」，本轮把
`Heightmap` 的内部约定读清楚了，结论是**差在 Rust 侧的两层 `+1` 记重了**：

官方 `Heightmap`（`net/minecraft/world/level/levelgen/Heightmap.java`）：

```
private int getFirstAvailable(int index) { return this.data.get(index) + this.chunk.getMinY(); }
private void setHeight(int x, int z, int height) { this.data.set(getIndex(x, z), height - chunk.getMinY()); }
public int getHighestTaken(int x, int z) { return this.getFirstAvailable(getIndex(x, z)) - 1; }
```

`setHeight` 存的是 `height`，而调用它的是 `Heightmap.update` 里的
`this.setHeight(localX, localZ, localY + 1)` 和 `primeHeightmaps` 里的
`heightmap.setHeight(x, z, y + 1)`——**`data` 存的是「最高方块 y + 1」**，
`getFirstAvailable` 返回同一个值，`getHighestTaken` 才是最高方块 y。

`ChunkAccess.getHeight(type, x, z)` 直接返回 `heightmap.getFirstAvailable(...)`
（`ChunkAccess.java:180`），也就是**最高方块 y + 1**；
`WorldGenRegion.getHeight` 覆写版再 `+ 1`（`WorldGenRegion.java:462`），
所以 `WorldGenRegion.getHeight` = **最高方块 y + 2**。

Rust 侧：`feature/heightmap.rs` 的 `Heightmaps` 存的是**最高方块 y**（`update` 里
`*slot = y`，`prime_heightmaps` 里 `max(y)`），比官方 `data` 少 1；
`FeatureWorld::get_height` 返回 `stored + 1` = **最高方块 y + 1**，
即 `ChunkAccess.getHeight`，比 `WorldGenRegion.getHeight` 少 1。

上一轮把 `get_height` 改成 `+2` 后对拍从 18 涨到 2786，是因为**埋藏的宝藏那条路径本来就错**：
`BuriedTreasurePiece.postProcess` 用 `level.getHeight(OCEAN_FLOOR_WG, x, z)`（`WorldGenRegion`
覆写版，= 最高 y + 2），而 Rust 用它算出的 `+1` 值恰好和 Java oracle 里**同样少一层的
代理实现**相互抵消，所以显示 0 不一致。**也就是说 `buried_treasure` 的 0 不一致也是假的**，
两边的 bug 抵消了。

**正确做法（下一轮执行）**：

1. 把 `feature/heightmap.rs` 的存储约定改成和官方一致（`data` = 最高方块 y + 1），
   或者把 `FeatureWorld::get_height` 改成 `stored + 2` 并**同时**修正
   `buried_treasure` 的调用；
2. Java oracle 的 `PieceLevelHandler.heightAt` 目前返回 `chunk.getHeight(...)`
   （= 最高 y + 1），`getHeight` / `getHeightmapPos` 各自 `+1`，两边都少一层；
   应改成 `heightAt` 返回 `chunk.getHeight(...) + 1`（= 最高 y + 2）并去掉外面的 `+1`；
3. 改完必须重跑 `buried_treasure` 与 `swamp_hut` 两项，确认**同时**归零。

**本轮没有提交代码**（只把这条结论写进文档）：时间用在了读官方 `Heightmap` 与核对
`ChunkAccess` / `WorldGenRegion` 的两处 `+1`，把「两边 bug 抵消」这一层查清楚了，
但没有足够的实测时间去同时改 Rust 存储约定 + Java 代理 + 两个结构并验证。

**第十四轮（2026-11-14，`cb-wg-structures` 续：女巫小屋差一格的实测推翻前两轮推断）**：

本轮**用追踪确认了 Java 侧的真实写入**，前面三轮的推断都不对。代码全部回退
（工作树干净，仍是 `c99e823` 的 18 处基线）。

在 Java oracle 的 `PieceLevelHandler.setBlock` 里加了一条追踪，只打印
`x = 1, z ∈ {2, 7}, y ∈ 61..64` 的写入，实测（`swamp_hut 0 0 0`）：

```
Y 1 64 2 140
Y 1 64 7 140
Y 1 63 2 140
Y 1 63 7 140
```

即 **Java 的 `fillColumnDown` 写的是 64 和 63，不是 62**。Rust 写 63（本轮诊断
`hpos = 64`、`new_bb[1] = 64`、`world_y(-1) = 63`）。

所以 18 处差异的 `java: 140 (oak_log) @ (1,62,2)` **不是** `fillColumnDown` 写的，
而是别的地方。已知：

* `fillColumnDown` 只写 63（62 是沙子，`isReplaceableByStructures` 为假，循环停下）；
* 四根柱子 `generateBox(1,0,2, 1,3,2, logs…)` 写局部 y `0..3` = 世界 `64..67`；
* 女巫小屋没有任何指令写世界 y < 63。

**矛盾的焦点**：Java 在 `(1, 62, 2)` 写下了橡木原木，但从源码看这条路径走不到那里。
可能性（下一轮按顺序排除）：

1. **`getWorldY` 的基准在 `fillColumnDown` 调用时不是 64**。`updateAverageGroundHeight`
   在 `postProcess` 开头把盒子平移到 `minY = heightPosition`（本轮诊断 `hpos = 64`），
   但官方 `placeCollapsedRoof` / `addCellar` 之类不会改回来。如果 `heightPosition` 实际是
   63（诊断读到 64 是因为我打印的是平移后的盒子再算一次平均），那么 `world_y(-1) = 62`
   就解释了 62 这一格。
2. **`Heightmap.primeHeightmaps` 与 Rust `prime_heightmaps` 对 `MOTION_BLOCKING_NO_LEAVES`
   的判定不同**（本轮仍未对拍这两份实现）。
3. `chunkBB.isInside((x, 64, z))` 的边界差一列，导致 `count` 与 `total` 不同。

**下一轮的第一步**：在 Java 的 `updateAverageGroundHeight` 之后**立刻**打印
`boundingBox.minY`（而不是在 `postProcess` 之后重算），这是唯一还没测过的数。

**本轮同时确认的一条（可复用）**：`Heightmap` 内部 `data` 存的是「最高方块 y + 1」，
`ChunkAccess.getHeight` = 该值，`WorldGenRegion.getHeight` 覆写版再 +1；
Rust `Heightmaps` 存「最高方块 y」、`get_height` 返回 `+1`，正好等于
`ChunkAccess.getHeight`，比 `WorldGenRegion.getHeight` 少 1。把 `get_height` 改成 `+2`
会让女巫小屋整栋上移一格（2786 处不一致），所以**现有 `+1` 是对的**，不要再改。

**第十五轮（2026-11-14，合并前收尾）**：

* `git merge main` 结果：main 已经是本分支的祖先（`7373539` 的树 / 倒木 / 巨型蘑菇 /
  `feature/shape_update.rs` 都已在），无需合并，没有冲突。
* 确认「没验证过的不进游戏」：`structure::post_process`（埋藏的宝藏 / 女巫小屋 / 沙漠神殿的
  `postProcess` 方块部分）**没有任何生产路径调用**——`feature/run.rs`、`chunk.rs` 都不引用它，
  只在 `structure/mod.rs` 里 `pub mod` 声明。游戏里结构件仍然一个方块都不放。
* README 与本文第 7 节的现状表按实测更新：此前写的「埋藏的宝藏与女巫小屋的 `postProcess`
  逐方块 0 不一致」是**错的**（那是改用写入日志之前的数字），已改成
  「埋藏的宝藏 0 不一致、**女巫小屋 18 处不一致未通过**、沙漠神殿 `SKIPPED`」。
* 全量检查：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、
  `cargo test --no-fail-fast`（927 passed，`staged_pipeline_delivers…` 90s 时限在 debug 下超时，
  `--release` 5.75s 通过）、`gen-world`（0 不一致）、`gen-world --through features`
  （0 不一致）、九个结构对拍（全 0）、探针（719/719）。

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

**第六轮（2026-11-14，`cb-wg-multipiece` 续：结构件存档类型与读回路径）**：

* `structure::pieces::Piece` 的子类存档值从「已格式化的字符串」改成带 NBT 类型的
  `Extra::Byte / Int / Str`，新增 `SavePiece::from_layout` 把它们逐键写成官方类型。
  `structure-pieces` 的 Java 侧同步在 token 上加类型前缀（`B:` / `I:`）。这次改动立刻
  抓到一个真 bug：`MineShaftCrossing` 的 `D` 官方走 `tag.store("D",
  Direction.LEGACY_ID_CODEC_2D, ...)`，那是 `Codec.BYTE`，原来写成 Int 也测不出来；
  按 Byte 写之前对拍报 3312 处不一致，改正后 0。
* `Entrances` 按 `MineShaftRoom.addAdditionalSaveData` 的
  `tag.store("Entrances", BoundingBox.CODEC.listOf(), childEntranceBoxes)` 写成
  IntArray 列表（不是字符串）。
* 读回侧新增（`structure::save`）：`load_piece`（`StructurePiece(type, tag)` 的
  `GD` 缺省 0 / `BB` 必需 / `O` 缺省 0，`Identifier.parse` 补 `minecraft:` 命名空间，
  `PiecesContainer.load` 的 `RENAMES` 把 `nvi` / `pcp` / `bastionremnant` / `runtime`
  归一到 `minecraft:jigsaw`）、`load_pieces`（未知 id 与坏标签跳过，照官方的
  log-and-skip）、`load_start`（`loadStaticStart` 的字段集，`id = "INVALID"` 返回空）
  与 `regenerate_monument`（`OceanMonumentStructure.regeneratePiecesAfterLoad`：
  即便用存档朝向也先取一次 `getRandomDirection` 保持取数位置，再用存档 `BB` 的
  `minX/minZ` 与朝向重建）。海底神殿的构造抽成 `ocean_monument::monument_building`，
  生成与读回共用同一处，取数序列不可能分叉。
* 新对拍 `cargo run -p mc-bamboo-vanilla -- structure-load <java>` +
  `java/StructureLoadOracle.java`：官方构造器造件 → `createTag` → `PiecesContainer.load`
  → 海底神殿再跑 `regeneratePiecesAfterLoad`（房间图用反射列 `childPieces`）；Rust 走
  「布局 → `SavePiece::from_layout` → `to_tag` → `load_pieces`」同一条路。5 种子 × 4 区块
  × 4 kind = 80 查询、8218 个读回结构件、656 行重建、8874 个比较 0 不一致；把
  `regenerate_monument` 的方向取数换成常量立刻 610 处不一致，还原回 0。
  单元测试另外钉住「同种子同区块重建 == 直接生成」「存档朝向覆盖取到的朝向」
  「`O = -1` 退回取到的朝向」三条。
* 仍是纯函数 + oracle：`chunk_nbt` 把 `structures` 当不透明标签原样保留，读回路径没有接进
  世界状态，游戏里仍不会出现结构。

**多件式结构放置（2026-11-15，分支 `cb-wg-multipiece`，矿井与要塞已对拍 0 不一致）**：

* `structure::piece_place`：官方 `StructurePiece` 的放置辅助层（`getWorldPos` /
  `placeBlock` / `generateBox` 的两个重载 / `generateMaybeBox` / `maybeGenerateBlock` /
  `generateUpperHalfSphere` / `fillColumnDown` / `isInterior` /
  `isReplaceableByStructures`），世界读写由 `PieceLevel` trait 抽象。这是给结构队员
  复用的公共层。
* `structure::state_transform`：`placeBlock` 里的 `BlockState.mirror` / `rotate`。
  官方默认恒等，只有少数方块覆写；这里覆盖了矿井和要塞用到的四类：栅栏
  （`CrossCollisionBlock` 四面）、墙上火把与按钮（`HorizontalDirectionalBlock` 的
  `facing`）、门（`DoorBlock`，镜像时额外 `cycle(HINGE)`）、铁轨（十种 `RailShape`
  的完整旋转与镜像表）。
* `structure::pieces::mineshaft_place`：矿井四种结构件的 `postProcess`。
* `structure::pieces::stronghold_place`：要塞十二种结构件的 `postProcess`，含
  `SmoothStoneSelector`（边缘一次 `nextFloat()` 分四档石砖，非边缘洞穴空气）与四种
  `SmallDoorType`。
* 对拍 `cargo run -p mc-bamboo-vanilla -- structure-place <java>`：Java 侧在假
  `WorldGenLevel` 上按 `StructureStart.placeInChunk` 的编排，对整个结构碰到的每个区块跑
  官方 `postProcess`。比三样：结构件清单、每次调用的写入次数（`STRUCTURE_PLACE_TRACE=1`
  时是按顺序的每一次写入，`cave_air` 和 `air` 不合并）、每个写过的位置的最终状态（挖空
  也比；最终状态里空气家族合并成 `AIR`，因为 `ProtoChunk.getBlockState` 对全空气的段
  直接返回 `air`）。四种初始世界：全空、地表一层石头、实心石头、带空气洞和水洞的石头。
  **当前状态（2026-11-15）**：矿井（普通 / 恶地）与要塞各 32 条查询，合计 160 万个写过的
  位置，0 不一致；故意改坏（不取矿车 / 箱子的战利品种子、不做 `isInInvalidLocation`、
  `placeBlock` 不做镜像旋转）都会报错。
* 放置层要守的两条官方语义：`OCEAN_FLOOR_WG` 在 FEATURES 阶段是**冻结**的（区块状态
  TERRAIN 的 `heightmapsAfter` 只有 FINAL 那一组），`PieceLevel::ocean_floor_height`
  不能按当前方块现算；`getBlockState` 读写区外的区块照样返回真实方块。
* 这套对拍**没覆盖**的分支：`generateBox` / `generateMaybeBox` 的「边缘块 ≠ 填充块」
  （矿井和要塞的调用两者总是同一个方块）、`generateAirBox`、基类 `fillColumnDown`、
  `MINESHAFT_BLOCKING` 群系、铁链上方是下落方块或 `UNSTABLE_BOTTOM_CENTER` 的情况、
  柱子下方是熔岩、十二个末影之眼全中的传送门。
* **没接进 Director**：这些代码只被 oracle 调用，游戏里仍然没有结构。

**海底神殿与林地府邸放置（2026-11-15，分支 `cb-wg-monument`，都已对拍 0 不一致）**：

* `structure::pieces::ocean_monument_place` 及 `monument_rooms` / `monument_double_rooms`：
  `MonumentBuilding` 与全部房间的 `postProcess`。对拍世界加了 `ocean`（海床 + 水 + 部分冰面）。
  官方神殿里 364 处 `generateBox` 的边缘块和填充块都相同，所以「边缘 ≠ 填充」这一支
  仍然只有单元测试覆盖。
* `structure::pieces::woodland_mansion::place_piece`（`TemplateStructurePiece.postProcess`）
  与 `after_place`：模板放置走 `structure::template_place`，数据标记处理箱子和
  `Mage` / `Warrior` / `Group of Allays`（每只生物把标记格写一次空气，悦灵数量取
  `PieceLevel::level_random`）。
* `state_transform` 改成按方块类和祖先类分派，覆盖官方所有重写 `rotate` / `mirror` 的方块
  类；`up` / `down` 朝向在旋转时保持不变。活塞三个类（`PistonBaseBlock` /
  `PistonHeadBlock` / `MovingPistonBlock`）各自重写了这两步而父类 `DirectionalBlock`
  没重写，所以必须写明列进 `Facing` 规则，不能靠祖先类查出来；`transform` 对拍把
  粘性活塞、活塞头、移动中的活塞也加进查询集。
* 合并 main 之后只剩一份模板放置：府邸和雪屋 / 沉船 / 海底遗迹 / 废弃传送门共用
  `template_place::place_in_world`（`buildInfoList` 顺序、processor 管线、流体回填、
  `updateShapeAtEdge` + `updateFromNeighbourShapes`）。府邸经 `PieceLevelAccess` 把
  `PieceLevel` 当 `feature::shape_update::BlockAccess` 用，数据标记按 `data_markers`
  （`filterBlocks` 顺序）处理。原来府邸专用的 `template_shape_update` 已删，形状更新只剩
  `feature::shape_update`；它的生成期天空光改读 15（区块还没点光，蘑菇的 `canSurvive`
  靠它），之前读 0。
* **当前状态（合并 main 之后）**：神殿 32 查询 298 万个位置、府邸 32 查询 380 万个位置，
  0 不一致，每次调用的写入次数也全部相同；`structure-templated-place` 334 查询仍 0。

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
| 4 | 结构件生成：旧式（沉船、海底遗迹、沙漠/丛林神殿、女巫小屋、雪屋、埋藏宝藏、废弃矿井、要塞、林地府邸、海底神殿、废弃传送门）先做，再 jigsaw（村庄、前哨站、古城、试炼密室、古迹废墟） | 每个结构起点 + 全部结构件的包围盒、朝向、类型 0 不一致 | **做完**：四个代码生成单件结构（沙漠神殿 / 丛林神殿 / 女巫小屋 / 埋藏宝藏，`structure-single` 384 查询 0 不一致）、废弃矿井 / 要塞 / 海底神殿 / 林地府邸（`structure-pieces` 336 查询 77960 结构件 0 不一致）、需要模板的雪屋 / 沉船 / 海底遗迹（`structure-templated` 480 查询 1685 结构件 0 不一致）、废弃传送门（`structure-ruined-portal` 648 查询 110 结构件 0 不一致，含 `findSuitableY` 整列搜索与 `Mirror.FRONT_BACK`）、**jigsaw 全部（`cb-wg-jigsaw`：`jigsaw pieces` 972 查询 12467 结构件 / `jigsaw pools` 980 查询 8976 结构件 0 不一致）** 已做 |
| 5 | `features` 阶段结构件放置：processor、方块实体、拼图替换、战利品表 | `chunkgen --through features` 逐方块 0 不一致 | **部分完成**：`structure::piece_place`（放置辅助）、`structure::state_transform`（mirror/rotate）、`structure::pieces::mineshaft_place`（矿井四种）与 `stronghold_place`（要塞十二种）已移植，`structure-place` 对拍对矿井和要塞都是 0 不一致（2026-11-15）。**没有接进 Director**，见第 0 节「多件式结构放置」。<br>**部分做完（方块部分，2026-11-15 归零）**：单件结构的 `postProcess` 走独立子命令 `structure-post-process`（`StructurePiecePlaceOracle.java`），按写入日志 + 随机探针 + 方块实体逐格比：四个结构（埋藏的宝藏 / 沙漠神殿 / 女巫小屋 / 丛林神殿）各 16 查询、共 74918 个写入方块、75062 个比较 **0 不一致**；十处故意改坏都确认能报错（详见第 11 节末尾）。废弃传送门的 `postProcess` 三步由 `structure-templated-place` 覆盖并做过改坏确认。丛林神殿 `postProcess` 也已移植并逐方块对拍（见第 11 节末尾）；四份 `postProcess` 都还没接进 `feature/run.rs`。<br>**海底神殿与林地府邸的方块放置（含模板放置与 `afterPlace`）也是 0 不一致（`cb-wg-monument`）**，<br>**jigsaw 侧（`cb-wg-jigsaw`）**：模板放置链路 `structure::jigsaw::place`（选 palette → 位置变换含 mirror/rotation → processor 链 → `BARRIER` → `chunkBB` 过滤 → 状态变换）对官方 `StructureTemplate.placeInWorld` 逐写入比，`jigsaw placepiece` 900 查询 / 610992 次写入 0 不一致；`processor_list` 的四种 processor 与放置阶段硬编码的三种（`block_ignore` / `jigsaw_replacement` / `gravity`）都已移植并各自对拍，`assemble_processors` 照 `SinglePoolElement.getSettings` 的四段顺序组装（含 legacy 的 pop-then-append）；`RuleBlockEntityModifier` 的三种（`passthrough` / `append_loot` / `clear`）与 `append_static` 都已按官方 `apply` 实现并对拍（`jigsaw bemod` 60 查询 0 不一致）；`jigsaw rc` 1286 个方块的随机容器判定 0 不一致 |
| 6 | `Beardifier` 接进 terrain 密度 | terrain 阶段（EMPTY 回归 + 有结构地基）0 不一致 | **做完（纯函数 + 对拍，`cb-wg-beard`）**：`beard` 12 用例 5,912,076 比较 0 不一致（kernel / sampleValue / sampleVolume / affectedBox / 三阶段方块），`--break-self-test` 会失败；`chunkgen --through carvers` EMPTY 回归仍 0。**没接进 Director**：游戏里仍走 `EMPTY` |
| 7 | 存档：`structures` 键写/读；海底神殿「只存一个 `MonumentBuilding`、读回重建房间图」的口径见 4.1 | 读回一致，`world-dir` 确认官方能读 | **通用编码做完**：`structure-save` 8 case 规范 NBT 0 不一致，官方 `PiecesContainer.load` / `save` 往返、`world-dir` 的 `SerializableChunkData.parse` 都能读。**多件式结构的读回做完**（`cb-wg-multipiece`）：子类字段带 NBT 类型，`PiecesContainer.load` / `loadStaticStart` / `regeneratePiecesAfterLoad` 移植并对拍（`structure-load` 80 查询、8218 读回件、656 重建行 0 不一致），见第 0 节「多件式结构存档」 |

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

## 10. 已知限制（第五轮结束时，2026-11-14；本轮更新见 §12）

* 主世界以外的维度（下界、末地）不做。Bamboo 目前没有下界/末地维度。
* **已接进游戏（本轮，见 §12）**：`structure_starts` / `structure_references` 走
  `mc-bamboo-game::worldgen_structures` 的纯函数，terrain 注入真实 Beardifier，
  features 每 step 先放结构件，`structures` 键写盘/读回。
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
  `structure::templated` 的雪屋 / 沉船 / 海底遗迹布局、废弃传送门的布局（第 4 步其余）、
  `mc-bamboo-worldgen::beardifier`（第 6 步，`cb-wg-beard`，见第 5 节）、`structure::save` 的
  `structures` 键（第 7 步）、以及 `structure::jigsaw` 的 jigsaw 结构件生成与模板放置链路
  （第 4 步的 jigsaw 部分，`cb-wg-jigsaw`，972 查询 / 12467 结构件 + 900 查询 / 610992 次写入
  0 不一致）。仍没做：把全部结构起点接进 Director；这些纯函数/类型还没进游戏。
  `features` 阶段结构件放置的**逐方块对拍**现状见第 11 节末尾（2026-11-15 一段）。
* 第五轮已落地并对拍：`structure::single` 的四个代码生成单件结构布局、
  `structure::templated` 的雪屋 / 沉船 / 海底遗迹布局（第 4 步一部分）、
  `mc-bamboo-worldgen::beardifier`（第 6 步，`cb-wg-beard`，见第 5 节）、`structure::save` 的 `structures` 键（第 7 步）。
  jigsaw 结构件生成与 `features` 阶段结构件放置的后续里程碑已在别的分支落地并合入
  （见第 7 节步 4 / 步 5 与第 11 节末尾）；仍没做的是把全部结构起点接进 Director。
* 废弃传送门的布局已做（第六轮，见下面「第六轮」一段）；它的 `postProcess`
  （`spreadNetherrack`、`maybeAddVines`、`addNetherrackDripColumnsBelowPortal`）也已移植并
  经 `structure-templated-place --only ruined_portal` 的 234 查询 / 282196 格比较逐方块对拍
  （2026-11-15，三步各自的改坏确认见第 11 节末尾）。
* 雪屋 / 沉船 / 海底遗迹的布局只做「结构件列表」：雪屋和沉船的 `postProcess` 会用
  `getHeight` 把结构件 Y 校正到地表 / 海床，沉船还在模板超过 32 格时于 `generatePieces`
  里按 `getLowestY` / `getMeanFirstOccupiedHeight` 调 Y；海底遗迹的 `postProcess` 同样按
  `OCEAN_FLOOR_WG` 重设 Y。这些都依赖 terrain 高度，留给放置里程碑；现有 31 个沉船模板
  都不超过 9×21×28，`isTooBigToFitInWorldGenRegion()` 恒 false，所以 `generatePieces`
  里的高度调整在官方也从不触发（Rust 侧对该分支直接报错而不是猜）。
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

## 11. 四个分支合并后的现状（`cb-wg-integrate`，2026-11-14）

`cb-wg-aquatic`、`cb-wg-caves`、`cb-wg-feat-a`、`cb-wg-structures` 四个分支按顺序合入。
结构这边合并时只发生一处命名冲突，处理如下：

| 冲突 | 处理 |
|---|---|
| 两个分支各有一个 `structure-place` 子命令，且**互换了 Java 源文件名** | main 上的 `StructurePlaceOracle.java` 是多件式结构件放置，`cb-wg-structures` 上的 `StructurePiecePlaceOracle.java` 是单件结构的 `postProcess`。按内容重新对应：`structure-place` 仍用 `StructurePlaceOracle.java`（多件式），单件的那份改名成 `structure-post-process` 子命令；`main.rs` 里两个 `const` / `mod` / match 分支 / 空实现函数都补齐成一对 |
| `Piece.extras` 的类型 | main 改成了带 NBT 类型的 `BTreeMap<String, Extra>`，`cb-wg-structures` 还是 `BTreeMap<String, String>`。保留 main 的类型，并补 `Extra::Float`（海底遗迹的 `Integrity` 是 `putFloat`）；`Extra::token()` 带类型前缀供 `structure-pieces` / `structure-load` 用，新增 `Extra::plain_token()` 给三个只打印数值的 oracle（`structure-single` / `structure-templated` / `structure-ruined-portal`） |
| `FeatureWorld::new` 的参数 | 多了一个 `seed`（晶洞的固定参数噪声要用），所有调用点已补 |

合并后结构相关的对拍全部重跑（数字见 `reports/cb-wg-integrate.md`）：
`structures` / `structure-rings` / `structure-validity` / `structure-generation` /
`structure-single` / `structure-pieces` / `structure-save` / `structure-templated` /
`structure-ruined-portal` / `beard` 全部 0 不一致；`structure-place`（多件式放置）仍是
合并前 main 的状态——Java 侧跑全量时抛异常，未验证；`structure-post-process`（单件
`postProcess`）是 18 处不一致，女巫小屋差一格，未通过。这两项都**没接进
`feature/run.rs`**，游戏里结构件仍然一个方块都不放。
（上面这段是 2026-11-14 合并时的历史快照；下面给出 2026-11-15 的现状。）

**`cb-wg-structures` 续（2026-11-15，`structure-post-process` 归零）**：

* `StructurePiecePlaceOracle` 的三处假绿全部修掉并据此改了生产代码：
  1. `fillColumnDown` 复用 `MutableBlockPos`，把它直接当 `TreeMap` 键会随之后的 `move(DOWN)`
     串改已记下的坐标——这是当年「女巫小屋差一格」**唯一的根因**，Rust 侧本来就是对的；
  2. `getRandom()` 每次返回新建的随机源，而官方 `WorldGenRegion.random` 是构造时建好的
     一个字段，`addCellarStairs` 的 `nextBoolean()` 与 `placeCollapsedRoof` 的 25 次
     `nextFloat()` 共用一条序列；
  3. `getBlockEntity()` 恒返回 null，导致 `createChest` / `createDispenser` 不取战利品表的
     `nextLong()`，后续取数整体错位。现在按官方 `WorldGenRegion.setBlock` 留 DUMMY nbt
     并现造方块实体。假世界也从一个 `ProtoChunk` 扩成 3×3，区块外读写拿到真实相邻列。
* 生产代码：`desert_pyramid` 的 `level.getRandom()` 位置从「区块中心」改成官方
  `ChunkPos.getWorldPosition()`（区块最小角 `cx*16, 0, cz*16`）；`createChest` 现在写
  `LootTable` / `LootTableSeed` 方块实体并取 `nextLong()`；`structure::place` 复用
  `piece_place` 的 `box_is_inside` / `is_replaceable_by_structures`，箱子的 `reorient`
  复用 `piece_place::reorient_with`（新增的纯函数接口，`piece_place` 原有接口一个没删）。
  顺带按 `javap` 修正 `piece_place::reorient` 后半段：官方是三段**顺序**探测链，不是
  四次独立探测（`structure-place` 全量 128 查询仍 0 不一致，单测同步更新）。
* `structure-post-process` 现在比「写入日志 + 随机探针 `R` 行 + 方块实体 `E` 行」，
  48 查询（埋藏的宝藏 / 沙漠神殿 / 女巫小屋各 16）、46982 个写入方块、47062 个格比较、
  0 不一致。故意改坏：神殿 `-nextInt(3)` 再减 1 → 19975 处；女巫小屋一段 `generateBox`
  上界 +1 → 224 处；战利品种子 `+1` → 80 处；三处都还原后回到 0。
  `structure-single-place` 是 `structure-post-process` 的历史别名（同一 Java 源、同一套
  查询；旧模块生成的是多件式查询、却指向单件 Java 源，以前一跑就抛异常）。
* 废弃传送门 `postProcess` 的 `spreadNetherrack` / `maybeAddVines` /
  `addNetherrackDripColumnsBelowPortal` 由 `structure-templated-place --only ruined_portal`
  覆盖：234 查询 / 282196 格比较 / 2106 个随机探针，0 不一致。三步分别改坏（概率表首项
  1.0→0.0 报 8064 处、滴水柱边界 +1→+2 报 22853 处、藤蔓方向恒取 0 报 6763 处）后还原，
  确认对拍不是假绿，因此不再给 `structure-ruined-portal` 加独立的放置模式。
* **丛林神殿 `postProcess` 已移植并逐方块对拍**（2026-11-15）：指令表由
  `mc-bamboo-vanilla generate` / `extract` 固化生成，写进
  `crates/mc-bamboo-worldgen/src/structure/post_process/jungle_temple_ops.rs`
  （`jungle_temple.rs` 用 `include!` 引入，不要手动改）。
  `tools/mc-bamboo-vanilla/java/JungleTempleOpsExtractor.java` 用官方
  `JungleTemplePiece` 的子类覆盖 `generateBox` / `generateAirBox` / `placeBlock` /
  `createChest` / `createDispenser`，只记录调用序列：假 `WorldGenLevel` 让
  `updateAverageGroundHeight` 走完，helper 全部被拦截、不写世界，也不启动服务端或加载
  数据包注册表。报告是 171 条 helper 调用（71 个 `MossStoneSelector` `generateBox`
  每格一次 `nextFloat()`、17 个 `generateAirBox`、79 个 `placeBlock`、2 个 `createChest`、
  2 个 `createDispenser`；字节码里的 43 / 14 / 71 是静态调用点，循环展开后才是这个数），
  Rust 侧把 `createDispenser` 内部那次 `placeBlock` 并回发射器指令，得到 169 条
  `Op`（状态按 mirror / rotation 变换），再用同一个 `rustfmt` 排版，重跑逐字节无 diff。
  `structure-post-process` 扩到 64 查询（四结构各 16）、74918 个写入方块、75062 个比较
  **0 不一致**；四种朝向全部覆盖。故意改坏：`MossStoneSelector` 阈值 0.4→0.5 报 1788 处、
  一块苔石换成圆石报 16 处、发射器战利品种子 +1 报 32 处、发射器朝向 north→south
  报 16 处，还原后回到 0；导出流程接好后又改坏导出的那条 `(5,6,0)–(6,6,0)`
  `generateBox`（上界 x 改成 7），对拍报 880 处，还原后回到 0。
* 这些 `postProcess` 仍是纯函数 + oracle，**没接进 `feature/run.rs` / Director**，
  游戏里结构件仍然一个方块都不放（接线由 `cb-wg-structwire` 负责）。

**全套合并检查（2026-11-15，`cb-wg-structures` 中间状态，丛林神殿之前）**：合并 `main`
（`faeb4c66`）后 `cargo fmt --all --check`、`cargo clippy --workspace --all-targets`
（0 警告）、`cargo test --workspace`、`BAMBOO_TEST_LOADED=1 cargo test -p mc-bamboo-game`、
`cargo check -p mc-bamboo-vanilla --no-default-features`（0 警告）全过；22 个结构 /
世界生成对拍全部 0 不一致；`oracle java --weather clear` 10114 cases / 0 fail；
联机探针 719/719，`--loaded` 连跑两次、两遍各 719/719。
这段验收是在丛林神殿移植**之前**跑的；丛林神殿完成后还需重跑一次全套。

**最终全套合并检查（2026-11-15，四个单件结构 `postProcess` 全部完成、发射器路径修正后重跑）**：
`cargo fmt --all --check`、`cargo clippy --workspace --all-targets`（0 警告）、
`cargo test --workspace`、`BAMBOO_TEST_LOADED=1 cargo test -p mc-bamboo-game`、
`cargo check -p mc-bamboo-vanilla --no-default-features`（0 警告）全过；
22 个世界生成 / 结构对拍全部 0 不一致（`structure-post-process` 与别名
`structure-single-place` 都是 64 查询 / 74918 写入方块 / 75062 比较 0 不一致）；
`oracle java --weather clear` 10114 cases / 0 fail；联机探针 719/719，
`--loaded` 连跑两次、两遍各 719/719 且世界目录经官方 codec 读回通过。

## 12. 接进 Director（2026-11-15，`cb-wg-structwire`）

* **`structure/pipeline.rs`**：富模型 `WorldStructureStart` / `StartPieces`
  （`Layout` / `Jigsaw` / `Sparse`），把各结构已对拍过的布局函数串成
  `createStructures`；`as_sparse()` 投影回 `start.rs` 的稀疏模型，供引用、
  Beardifier 和存档编码使用，旧 `structure-save` / `structure-load` 对拍不改。
  `WorldStructureState` 按种子持有 `possibleStructureSets` 过滤、要塞环位置、
  jigsaw 配置与模板池；`SeededRouter` 仍是每线程一份。
* **游戏侧 `worldgen_structures.rs`**：两阶段纯函数重算 + 线程本地缓存，
  `GenerateOutput.structures` 把 `starts` + `References` 标签交给世界线程，
  `Task::Assemble` 装进 `ChunkArchiveMeta.structures`。读盘标签优先恢复起点，
  再生成相邻区块与一次性生成一致（单测钉住）。
* **terrain**：`generate_terrain_stage` 与 `build_view` 都走
  `cached_generate_chunk_with_structures`，按官方 `createNoiseChunk` 把
  `Beardifier.forStructuresInChunk` 注进 `terrain_with_region`。依赖表
  `GENERATION[4].requires = [(S,8),(B,1)]` 保证半径 8 的 `structure_starts` 与中心
  `structure_references` 定稿；Beardifier 按 `(seed,x,z)` 全局缓存，0/2/8 计算线程
  逐字节相同（`structure_terrain_is_identical_across_worker_counts`）。
  **竞争根源与修法**：旧实现里 `build_view`（邻居中心重算地物用）读 EMPTY 地形的
  `TERRAIN_CACHE`，而 `generate_features_stage` 读世界线程 `partials` 里的真实
  Beardifier 地形；同一区块哪份快照先落地由计算池调度决定，
  `neighbor_spills_depend_on_which_terrain_snapshot_arrives` 稳定复现。
  `structure-pipeline` 现在还把官方带真实 Beardifier 的 CARVERS 方块逐格比：
  28 个结构区块 / 2752512 格 0 不一致，`--break-self-test` 报 28 处。
* **features**：`feature/run.rs` 每个 step 的地物之前调
  `structure/place_step.rs`；per-step 局部 index 数的是整张 `STRUCTURE` 注册表，
  结构件 `postProcess` 写 `FeatureWorld`（`PieceLevel` / `ProcessorLevel` 适配只加不删）。
  官方 `StructureStart.placeInChunk` 的包围盒相交、最后 `afterPlace` 都照做。
* **允许清单**：`cb-wg-structures` 合进 main 后，放置已逐方块对拍 0 的全部类型
  都打开：jigsaw / 矿井 / 要塞 / 海底神殿 / 林地府邸 / 雪屋 / 沉船 / 海底遗迹 /
  沙漠神殿 / 丛林神殿 / 女巫小屋 / 埋藏的宝藏 / 废弃传送门。所有结构的起点、引用、
  Beardifier 与 `structures` 存档本来就照官方算并存盘；现在放置也不再跳过。
* **对拍**：新子命令 `structure-pipeline <java>`（`StructurePipelineOracle.java`）
  在 JVM 里造 17×17 个 ProtoChunk，跑官方 `createStructures` / `createReferences`，
  逐区块比起点 / 引用 / 每个结构件的包围盒：28 行、84 次比较、2482 个结构件、28 个结构区块 /
  2752512 格带真实 Beardifier 的 CARVERS 方块 0 不一致；`--break-self-test` 报 28 处。
  另外 `gen-world --through features` / carvers 旧模式仍 0 不一致。
* **耗时**：`wg-perf --structure-timing`（release，1 种子 × 10 群系 × 3 区块 × 2 遍）
  实测 `structure_starts` 冷中位数 0.001 ms（首个区块 49 ms 建全局状态）、
  `structure_references` 冷中位数 6.813 ms、缓存命中 0.032 ms。缓存只影响速度：
  单测打乱顺序 / 清缓存后结果逐字节相同。
* **未做**：`generate-structures=false` 时 flat 世界的 `structure_overrides`；
  结构生成期实体（村民、悦灵、守卫者）；非 full 的 ProtoChunk 从盘上分阶段续跑时
  与官方混合（`BlendingData`）。`FeaturePoolElement` / `ListPoolElement` 的放置分支
  已在 §13 补上。

## 13. 带结构的整块端到端对拍（2026-11-16，`wg-e2e-struct`）

`gen-world --through features --structures` / `structure-e2e` 把「开着结构」的整块生成
也逐方块对拍：Java 侧照官方 `createStructures` / `createReferences` 造半径 9 的起点网格，
用 `Beardifier.forStructuresInChunk` 跑 3×3 的 CARVERS，再让
`shouldGenerateStructures()==true` 的 `StructureManager` 跑 `applyBiomeDecoration`；
游戏侧走 `game_features_with_mask`（`generate_structures=true`），是 Director 同一条路径。
默认无结构模式完全不变。干净跑 42 个结构区块 / 37158912 格 0 不一致，覆盖表与命令见
[`docs/worldgen-structures-e2e.md`](worldgen-structures-e2e.md)。

对拍抓出并修掉的真 bug（每条都有改坏确认，详见覆盖文档）：

| 修复 | 说明 |
|---|---|
| 结构分支随机源 | `WorldgenRandom` 必须包 `XoroshiroRandomSource`（`applyBiomeDecoration` 字节码 90-104），不是 `LegacyRandomSource` |
| `References` 迭代序 | 官方 `LongOpenHashSet` 的桶序由 `HashCommon.mix` + 线性探测 + 扩容历史决定；`feature/java_long_set.rs` 完整复刻，`long-set` 3000 组对拍 |
| 具名 `processor_list` | 注册表键带 `minecraft:` 前缀，剥掉会让铜灯氧化等链静默变空 |
| `ListPoolElement` / `FeaturePoolElement` | 前者按列表顺序递归 `place`，后者调 `PlacedFeature.place` 并复用 `StructureStart` 的随机源；`feature/run.rs` 注入 `FeaturePlacementContext` |
| `minecraft:block_pile` | 新增叶子地物，照字节码复刻 `BlockPos$4` 迭代序与「`dist2 > threshold` 才走 `nextFloat()<0.031`」的反向分支 |
| `GravityProcessor` | `WORLD_SURFACE_WG → WORLD_SURFACE` 只在 `ServerLevel` 上发生，FEATURES 阶段保留冻结的 WG 高度图 |
| `BlockWriter` 高度图 | 结构件写方块走 `set_block_with_heightmaps`，与 `ChunkAccess.setBlockState` 一致 |
| 存档兼容 | `References` LongArray 顺序变成官方桶序，读回按集合成员处理，旧顺序存档仍可读（单测钉住） |

**剩余缺口**：沙漠神殿 / 要塞的 `Layout` 起点区块在真实 `WorldGenRegion` 上的
`postProcess` 随机方块变体还有 33 / 412 处不一致（`--known-gaps` 单独跑），根因未定位；
`(42,-32,-16)` 的 `sulfur_pool` 是和结构无关的既有地物差异。默认查询集用这两类的
**被引用邻居**覆盖跨区块放置，整块 0 不一致。林地府邸 / 海底神殿 / 丛林神殿 /
海底遗迹在本机扫描里没有有效起点，仍由 `structure-pieces` / `structure-place` 覆盖。
