# 世界生成收尾清单（以代码为准，2026-11-19）

> 目的：定「把世界生成这里收尾」的范围。所有结论都对着本分支代码、`docs/` 里较新的记录、
> `tools/mc-bamboo-vanilla` 的对拍、以及官方 26.3 字节码 / 数据包核对过；早期设计稿里
> 「地物只移植了 `minecraft:ore`」「生成期光照没做」的说法已经落后于代码，不再采信。
>
> 本轮**只摸底、只记录**，没有改功能代码。验证命令都跑在工作树自己的 `target` 下
> （`unset CARGO_TARGET_DIR`），Java 用
> `/Users/wjz/Desktop/Github/Bamboo/.deps/jdk-25.0.5+8/Contents/Home/bin`，
> 官方 jar 是 `target/vanilla/versions/26.3/server-26.3.jar`。没有启动真实服务端 / 客户端，
> 没有接受 EULA，没有抄反编译源码。

## 结论速览

| 条目 | 结论 | 用户是否可见 | 量级（人日） |
|---|---|---|---:|
| 主世界三种噪声预设、多种子、带/不带结构、超平坦的扩大对拍 | **做完**（0 不一致），但工具本身有两处扩大样本才暴露的缺口 | 缺口只挡回归，不影响已生成的世界 | 0.5–1 |
| 主世界地物 / 结构 `unsupported` | **部分**：28 类型全 PASS、13 类结构可放；`pale_moss` 装饰器、纯下界/末地结构、生成期实体未覆盖 | 苍郁花园的苍白橡树、部分结构生成期实体 | 2–5 |
| 超平坦 `structure_overrides`（含关结构） | **没做**：只解析并写进存档，不生成 | 超平坦预设里不会出现村庄 / 湖 / 地物 | 3–6 |
| `BlendingData` / `Blender` / `BelowZeroRetrogen` | **没做**，而且当前**走不到**非空分支 | 读 26.3 之前老存档时不会升级、会当空世界或报错 | 8–15（需先拍板要不要支持老存档） |
| 非 `full` 区块读回后接着生成 | **没做**，也没有对拍 | 官方写出的半成品区块被 Bamboo 读成 `full` 模板 | 5–8 |
| 高度图 | **做完**（逐位一致）；增量 WG 维护未搬，是纯性能差异 | 无（当前结果正确） | 0–2（可选优化） |
| `gen-light` 光引擎对拍 | **做完**（0 不一致） | 无 | 0 |
| 游戏流水线 `light` 阶段的**输入** | **部分 / 有正确性风险**：用 CARVERS 快照，官方用 FEATURES 后视图 | 树冠、萤石、灯笼、结构光源附近可能算错天空光/方块光 | 2–4 |
| 生成期刷怪（`spawn`） | **没做** | 新区块生成时一只动物都没有 | 15–60（取决于实体范围） |
| 下界 / 末地**生成本身** | **部分**：数学与三套 settings、下界多噪声预设、五群系末地源、无含水层地形已导出/对拍；真维度地形、地物、结构没接 | 进不了下界 / 末地，所以暂时不可见 | 20–40 |
| 下界 / 末地**游戏侧多维度** | **没做** | 玩家无法进下界 / 末地、无法换维度、重生不回主世界 | 40–80（大项目，需用户拍板） |
| 性能 | **已有数据**，本轮不优化 | 只影响区块到达延迟 | 0（记录） |

---

## 0. 本轮扩大样本实跑

### 0.1 估算和口径

先按默认口径各跑一次冒烟再估时间：`chunkgen --seeds 6 --chunks 36 --through carvers`
约 38–113 s，`gen-world --through features --all-features --seeds 6 --chunks 36` 约 72–100 s，
`structure-e2e` 全量约 61–166 s，`gen-light` 12 场景约 5–18 s，`flat-world` 约 7–19 s，
`wg-perf --seeds 3 --chunks 8 --repeat 3 --skip-aux` 在十分钟量级。**单项都不到 20 分钟，没有缩样本**。
长时间对拍曾放后台，但发现同一脚本被重复拉起并和另两个子代理的 Java 进程抢 CPU，已清理；
下面列的是清理后串行 / 工作树自己 `target/release` 二进制的实跑结果。早期用主仓库 release 二进制
跑出的结果（源码和 main 只差岛的改动）按主代理指示照常引用，并在工作树自己的二进制上复跑了核心项。

全套验证里和世界生成相关的子命令见 `/tmp/verify-main2.sh`：`worldgen`、`chunkgen`、
`gen-world`、`gen-light`、`flat-world`、`spawn`、`structures*`、`structure-e2e`、
`feature-merge`、`feature-merge-struct`、`beard`、`jigsaw`、`long-set` 等。它们大多用固定查询集，
不是多种子扫描。

### 0.2 结果（全部 `0 mismatches`，除「新不一致」一节两处工具问题）

| 对拍 | 扩大的样本 | 结果 |
|---|---|---|
| `worldgen java --seeds 8 --points 10 --biome-points 64 --salt 7` | 默认 3 种子 → 8 种子、56 维度状态、20480 噪声值、23840 密度函数值、1536 群系 | 0，跳过 2720 个含 `interpolated` 的成批题 |
| `chunkgen java --seeds 6 --chunks 36 --salt 7 --through carvers` | 6 种子 × 36 区块，三阶段共 64032768 checks | 0（工作树二进制复跑） |
| `chunkgen java --seeds 8 --chunks 36 --salt 987654321 --through carvers` | 8 种子 × 36 区块 | 0 |
| 三预设 `chunkgen --through carvers --settings overworld\|large_biomes\|amplified` | 各 6 种子 × 36 区块 × salt 7 | 全 0 |
| 三预设 `gen-world --level-type normal\|large_biomes\|amplified --seeds 6 --chunks 36 --salt 7`（跑到 CARVERS） | quart 群系、方块、6 张高度图列、4 张 FINAL 字 | 全 0 |
| 三预设 `chunkgen --through features --settings overworld\|large_biomes\|amplified` | 各 6 种子 × 36 区块 × salt 7，全部 11 step / 全部地物，216 中心 / 191102976 格 / 0 skipped | 全 0 |
| `gen-world java --through features --all-features --seeds 6 --chunks 36 --salt 7` | 216 中心 / 191102976 格 / 20 方块实体 | 0（工作树二进制复跑） |
| 同上 `--salt 987654321` | 216 中心 / 191102976 格 | 0 |
| `gen-world java --through features --feature-types minecraft:ore --seeds 5 --chunks 36 --salt 1` | 180 中心 / 159252480 格 / 0 skipped | 0 |
| `structure-e2e java`（开结构，全量固定 60 查询） | 60 区块 / 53084160 格 / 133 方块实体 / 4743 官方结构件 | 0（工作树二进制复跑） |
| `structure-e2e java --chunks-file <2 行>`（开结构冒烟） | 2 区块 / 313 结构件 | 0 |
| `feature-merge java --orders isolated` / `--structures --orders isolated` | 3 / 5 个 target×order 行，按来源中心固定先后合并 | 0 differing |
| `gen-light java --scenes 6 --seed 42` / `--scenes 8 --seed 42` / `--scenes 12 --seed 7` | 6 / 8 / 12 合成场景，每场景 196608 格 | 全 0（工作树二进制复跑 8 场景） |
| `flat-world java` | 固定 5 个 `generator-settings` case（含 `features` / `lakes` / `structure_overrides` 组合） | 0 |
| `spawn java --seeds 8 --salt 7` / `--seeds 12 --salt 7` | 8 / 12 种子出生区块 + 出生点 | 全 0 |
| `chunkgen --through carvers --settings nether\|end --seeds 6 --chunks 36 --salt 987654321` | 各 216 区块、三阶段 | 全 0（**注意**：Java header 的 `preset` 仍写 `minecraft:overworld`，见 §4.1） |
| `chunkgen --through features --settings nether` | 所有中心 `unsupported` 跳过 | 见 §4.1 |
| `chunkgen --through features --settings end` | 0 skipped，但只用主世界全局 index | 见 §4.1 |

固定样本、不能靠 CLI 扩大的还有：`gen-light` 默认 4 个确定性合成场景（已扩到 12）、
`flat-world` 固定 5 个 case 且只生成 `(0,0)` 一个区块、`structure-e2e` 固定 60 个结构查询、
`feature-e2e.sh` 固定 28 个类型的触发坐标、`structure-*` 各 oracle 自带查询文件。
`tools/mc-bamboo-vanilla/feature-e2e.sh` 全量回归本轮被重复后台副本打断，没有声明重跑通过；
逐类型 PASS 证据以 `docs/worldgen-e2e-coverage.md` 为准，并用上面的 `--all-features` 整步对拍补了回归。
没有对 `chunkgen --structures` 单独扩样本：`chunkgen.rs` 不转发该开关，带结构整块对拍走
`gen-world --structures` / `structure-e2e`，以及 `feature-merge --structures`。

### 0.3 扩大样本才暴露的两处新不一致（只记录，不修）

两处都在**对拍工具 / JVM 假世界**，不是游戏生成代码；本轮按任务要求没有改。

1. `gen-world --through features` 在 `large_biomes` / `amplified` 下报整块差异。
   `tools/mc-bamboo-vanilla/src/gen_world.rs:692` 的 `on_features`、
   `:854` 的 `on_coverage`、`:1069` 的 `collect_structure_coverage` 把
   `WorldGenConfig.level_type` 写死 `LevelType::Normal`；而 `:609` 的 `on_carvers` 用的是
   `self.level_type`，Java 侧 `--settings` 已经换成了 `large_biomes` / `amplified`。
   所以游戏侧算的是 `normal` 地形，官方出的是另一种 settings，必然差几百万格。
   `chunkgen --through features --settings large_biomes|amplified` 用 header 反查 settings，
   同坐标同种子 0 不一致，交叉证明生成代码没问题。

   复现（工作树自己的 `target/release`）：

   ```bash
   export PATH=/Users/wjz/Desktop/Github/Bamboo/.deps/jdk-25.0.5+8/Contents/Home/bin:$PATH
   target/release/mc-bamboo-vanilla gen-world java --through features --all-features \
       --level-type large_biomes --seeds 1 --chunks 1 --salt 7
   # 60933 mismatches；换成 amplified 是 37473 mismatches
   target/release/mc-bamboo-vanilla chunkgen java --through features \
       --settings large_biomes --seeds 6 --chunks 36 --salt 7
   # 0 mismatches / 0 skipped
   ```

   修法预计是把上述三处改成 `self.level_type`（一两行级别），但本轮不修；文档里记为
   「对拍工具缺口」，避免误读成主世界生成不一致。

2. 扩大到 `salt 1`、多种子、全部地物时，官方假世界崩在 `PaleMossDecorator`：

   ```bash
   target/release/mc-bamboo-vanilla chunkgen java \
       --seeds 5 --chunks 36 --salt 1 --through features
   # 或 gen-world java --through features --all-features --seeds 6 --chunks 36 --salt 1
   # ChunkgenOracle.java failed: net.minecraft.ReportedException: Feature placement
   # Caused by: NullPointerException: ServerLevel.getChunkSource() is null
   #   at PaleMossDecorator.lambda$place$1
   ```

   `javap -c -p net.minecraft.world.level.levelgen.feature.treedecorators.PaleMossDecorator`
   显示 `ground` 分支通过 `level.getLevel().getChunkSource().getGenerator()` 拿
   `minecraft:pale_moss_patch` 再调 `Feature.place`；`ChunkgenOracle.java:2329`
   的 `installEntityStub` 只补了 `server` / `worldData` / `structureTemplateManager` / 若干惰性实体，
   没有 `chunkSource`。`salt 7` / `987654321` 的样本不触发该分支，所以上面的扩大结果仍是干净的。
   这是 oracle 假世界缺口；修它需要给不跑构造器的 `ServerLevel` 补 `chunkSource` 字段（或换掉该调用），
   预计 0.5 人日，本轮不修。

---

## 1. 主世界还有没有没对上的

### 1.1 地形、地物、结构的扩大对拍 — **做完 / 部分**

**证据**：§0.2 全部 0 不一致。`mc-bamboo-worldgen::chunk::Generator` 的 `doFill` /
`buildSurface` / `generateCarvers` 路径由 `chunkgen`、`gen-world --real-path` 逐格比；
`mc-bamboo-game::worldgen_features::game_features_with_mask` 是 Director 用的同一份
`run_features`，由 `gen-world --through features` / `structure-e2e` 逐格比 3×3 九宫格；
`feature-merge` 用官方共享 `ProtoChunk` 的 `center-first` / `center-last` 顺序和
`isolated` 固定先后合并比过写增量（本轮 3 / 5 行全 0 differing）。

**还没被任何对拍覆盖的**：
- `gen-light` 只比「给定同一批方块 → 光引擎输出」，不比「游戏流水线给光引擎的方块对不对」（§1.7）。
- `spawn` 阶段完全没有官方对拍（§2）。
- 非 `full` 读盘续跑没有对拍（§1.5）。
- `flat-world` 只比 `(0,0)` 一格的 `buildTerrain` 和五字段 codec，不比超平坦的地物/湖/结构生成（§1.3）。
- `structure-*` 的查询集是固定坐标，不是多种子扫描；扩大样本靠 `structure-e2e` 的 60 个定点 +
  `gen-world --through features --structures`。

### 1.2 地物、结构里标 `unsupported` 或被跳过的 — **部分**

**地物**：`crates/mc-bamboo-worldgen/src/feature/dispatch.rs:113` 的 `parse_leaf` 只认
移植过的叶子；`composition.rs:162` 的 `place_placed` 遇到不支持的修饰器 / 组合类返回
`PlaceOutcome::unsupported()`；`run.rs:185` / `:197` / `:230` 把整区块标 `unsupported`。
对拍里 `gen_world.rs:786` 遇到 `result.unsupported` 直接报错，`chunkgen.rs:735` 计入
`skipped_chunks`，所以「0 不一致」不会被 unsupported 伪装成通过。

`docs/worldgen-e2e-coverage.md` 记录 28 个 Feature 类型「干净跑 0 + 改坏确认」全部 PASS，
包括 `fossil` 和 `overlay`（沙漠水井）；`docs/worldgen-structures-e2e.md` 记录了 JVM 侧
原来挡住水井 / 化石的 `getLevel()` / `StructureTemplateManager` 限制已经被惰性
`ServerLevel` 补上，`structure-e2e` 默认集含 `(42,-267,-276)` 水井、`(42,-267,-277)` 化石。
`minecraft:root_system` 是官方 26.3 自己的死代码（成功路径不可达），不算缺口。

**真正剩下的主世界地物缺口**：
- `pale_moss` 树装饰器没移植：`feature/tree/decorator.rs:8` 明写「还没移植」，
  `Decorator::parse` 的 `_ => return None`（`:190` 附近）让 `minecraft:pale_oak` /
  `minecraft:pale_oak_creaking` 解析失败；`feature/tree/config.rs:191` 的单测甚至固定了
  `TreeConfig::parse(&features["minecraft:pale_oak"]).is_none()`。苍郁花园的树会标
  `unsupported` 跳过。`feature/world.rs:392` 等位置还有若干嵌套 provider / 谓词的
  `unsupported` 兜底，但常见主世界坐标的整步对拍已 0 skipped。
- 生成期实体只保证官方的随机取数和方块写入序列一致，不比较实体本身
  （`docs/worldgen-structures-e2e.md` §已知限制）；玩家在游戏里看不见村庄村民、
  女巫、猫、守卫者等。

**结构**：`crates/mc-bamboo-worldgen/src/structure/place_step.rs:47` 的
`PLACEABLE_TYPES` 有 13 类；`:317` / `:343` 把不在清单里的起点标 unsupported 跳过。
`worldgen.json` 的 52 个 `structure` 里，清单外的只有 `minecraft:end_city`、
`minecraft:fortress`、`minecraft:nether_fossil`（`bastion_remnant` 的 `type` 是
`minecraft:jigsaw`，但 `structure/pipeline.rs:437` 的注释把纯下界 / 末地结构集
列为「不会有起点」）。`structure-e2e.rs:112` 的 `KNOWN_GAPS` 当前为空；默认 60 查询全 0 不一致，结构件统计见
`docs/worldgen-structures-e2e.md`。也就是说，**主世界结构本身没有已知的逐方块残差**。

**用户可见影响**：苍郁花园的苍白橡树及其苔藓、垂苔、嘎枝之心不会生成；
村庄等结构有建筑但没有生成期实体。工作量 2–5 人日（`pale_moss` 装饰器 +
`pale_moss_patch` 递归放置 + JVM 桩 + 逐类型对拍 2–3 人日；生成期实体若要真放进世界，
依赖 §2 的实体系统，另算）。依赖：实体系统、`feature-e2e.sh` 新增 case、
`ChunkgenOracle` 的 `chunkSource` 桩（§0.3）。

### 1.3 超平坦关闭结构时的 `structure_overrides` — **没做**

`crates/mc-bamboo-game/src/world_options.rs:92` 的 `FlatSettings` 按官方
`FlatLevelGeneratorSettings.CODEC` 解析 `layers` / `biome` / `features` / `lakes` /
`structure_overrides` 五字段；`world_gen_settings.rs:140` 把 `structure_overrides`
写进 `world_gen_settings.dat`；`flat_world.rs:111` 的 `compare_fields` 和
`FlatWorldOracle.java` 的 5 个 case 逐字段对拍（本轮 0 不一致）。但注释和
`worldgen.rs:248` 的 `flat_chunk` 都明说：这些字段**只影响地物 / 湖 / 结构**，
Bamboo 的超平坦地形只填地层和群系；`world.rs:467` 对 flat 用 `PipelineKind::Flat`，
`chunk_generator.rs:91` 的 `FlatGenerator` 任何阶段都 `passthrough_stage`。

官方 `javap -c -p net.minecraft.world.level.levelgen.FlatLevelSource` 里
`createState` 会把 `structureOverrides()` 传给 `ChunkGeneratorStructureState.createForFlat`；
`spawnOriginalMobs` 直接 `return`（超平坦不生成期刷怪）；`applyBiomeDecoration`
仍会按 `features` / `lakes` 放地物和湖。`generate-structures=false` 在噪声世界里由
`worldgen_structures.rs:410` / `:419` 和 `worldgen_features.rs:685` 跳过结构阶段；
flat 世界里这个开关和 `structure_overrides` 都不会产生任何结构。

**用户可见影响**：超平坦预设写 `structure_overrides=["minecraft:villages", ...]`、
`features=true` / `lakes=true` 时，官方有村庄和湖，Bamboo 只有平坦地层。
工作量 3–6 人日：复用 `FlatLevelSource` 的 `createForFlat` 结构集、超平坦地物白名单、
湖、结构起点/放置，加 `flat-world` 之外的逐区块对拍。依赖 §1.2 的结构 / 地物纯函数，
以及决定「flat 是否也走 `Staged` 流水线」。

### 1.4 `BlendingData` / `Blender` / `BelowZeroRetrogen`（老存档升级）— **没做，且走不到**

`crates/mc-bamboo-worldgen/src/sampler.rs:398` / `:1239` / `:1832` 都只实现
「上下文里没有 `Blender`，原样返回」的空分支；`ChunkgenOracle.java` 用
`Blender.empty()`、`Beardifier.EMPTY`、`UpgradeData.EMPTY` 出新 `ProtoChunk`，
所以对拍只覆盖新世界。`chunk_nbt.rs:176` 的 `KNOWN_KEYS` 认识
`blending_data` / `below_zero_retrogen` / `UpgradeData`，但 `docs/m3-storage.md` §5.1
明说不写（官方只在非空时写），读盘时按「不认识的键原样保留」带回；
`chunk_nbt.rs:882` 对 `DataVersion != 5023` 直接 `NewerDataVersion` / `OlderDataVersion` 报错，
`world_dir.rs:207` 把老布局（世界根 `region/`）当空世界。**因此读老存档时不会进入
非空 `Blender` / `BelowZeroRetrogen` 分支**：要么报错 / 当空世界，要么按当前版本的键原样保留但不参与生成。

官方路径（`javap -c -p ChunkStatusTasks` 的 `generateSpawn` 也有 `ChunkAccess.isUpgrading()` 判断）：
非空 `BlendingData` 会改 `doFill` 的密度、`decorateBiomeResolver` 的群系混合、
`generateBorderTicks`，`BelowZeroRetrogen` 还会重跑旧区块下半部分。
`docs/worldgen.md` §「真实服务端路径与简化路径」已列为空路径不覆盖。

**用户可见影响**：把 1.18 之前（或任意非 5023 数据版本）的官方存档放进 Bamboo 不会按官方升级；
当前 Bamboo 自己写出的存档都是 `full`，不受影响。工作量 8–15 人日（导出 / 解码
`blending_data`、`Blender` 的三条密度函数与群系混合、`BelowZeroRetrogen`、border ticks、
老存档升级 oracle），依赖**用户先拍板是否要兼容 26.3 之前存档**；若明确不做，
这项只需在 README 固定「不支持老存档」即可，0.5 人日。

### 1.5 非 `full` 区块存盘读回、接着生成 — **没做，无对拍**

`crates/mc-bamboo-storage/src/chunk_nbt.rs:889` 读任意 `Status`，`:1067` 起对非 `full`
的 ProtoChunk 用 `ChunkColumn::from_template()` 补齐方块层和光照、重算高度图，
保留盘上的方块实体 / 计划刻 / 顶层元数据；`:680` 下次写盘写死 `minecraft:full`。
`crates/mc-bamboo-game/src/region/chunk_content.rs:253` 的 `decode` 组装成
`DecodedChunk { column, problems }`，**没有把 `status` 交给 Director**；
`director/pyramid.rs:88` 起的 `LOADING_PYRAMID` 十步全无依赖、直接到 `full`。
`docs/m3-storage.md` §「外置区块和非 full 的 ProtoChunk」和 README 已知限制都写了
「官方会从原阶段续跑生成流水线，Bamboo 不会」。

流水线阶段函数本身是纯的（`chunk_generator.rs`），但读盘路径不按 `Status` 选阶段，
也没有「写几个非 full 状态 → 重启 → 继续生成 → 和官方逐格比」的 oracle。

**用户可见影响**：读官方中途保存的半成品区块时，Bamboo 会把缺失阶段用超平坦模板填掉，
再当成已经 `full` 的区块交付；地形错误且状态被抬到 `full`。工作量 5–8 人日：
把 `status` 传进 Director / LOADING_PYRAMID、按阶段恢复 `Arc<GeneratedChunk>` 和
`feature_deltas` / `generated_light` / `structures`、写多阶段往返 oracle。依赖 §1.6 的高度图、
§1.2 的地物增量合并、存档层 `chunk_content::decode` 改接口。

### 1.6 高度图：从方块重算还是增量维护，是否逐位一致 — **做完，仅性能差异**

- `crates/mc-bamboo-game/src/worldgen.rs:211` 的 `compute_heightmaps` 按官方
  `Heightmap.primeHeightmaps` 从最终 CARVERS 方块一次算出六张图；
  `:896` 的 `build_region_column` 优先直接打包 `GeneratedChunk.heightmaps` 里的四张 FINAL，
  缺图时才 `compute_heightmaps` 重算；`:949` / `:969` 的打包字和
  `gen_world.rs:1616` / `:1666` 的 `compare_heightmaps` / `compare_heightmap_words`
  逐列、逐 long 对拍官方（本轮三预设 6×36 共 331776 列 / 31968 字，全 0）。
- FEATURES 阶段不是「从方块重算」：`crates/mc-bamboo-worldgen/src/feature/world.rs:218`
  的 `set_block_with_heightmaps` 调 `:232` 的 `heightmap::update` 增量维护四张 FINAL，
  `:359` 的 `prime_heightmaps` 给 3×3 视图建六张；`docs/worldgen-e2e-coverage.md`
  的 `heightmap-e2e.sh` 用 12 组改坏钩子证明六张图都被地物读到。
- Director 完工时 `worldgen_features.rs:1120` 的 `assemble_feature_column` 把
  `terrain + 地物写增量` 交给 `build_region_column`；`build_region_column` 里没有
  「让 `Chunk::from_sections` 每次重算」的强制路径（`:628` 的 `from_sections` 只在
  `heightmaps` 有 `None` 时逐格重算）。早期文档里「仍让 `Chunk::from_sections` 重算」
  已过时。
- 唯一没搬的是官方 `doFill` 里 `Heightmap.update` 对两张 WG 图的**增量维护过程**
  （`docs/worldgen.md` §高度图）；最终值等于从最终 CARVERS 方块 prime，逐列对拍已证明。

**用户可见影响**：无，当前逐位一致。工作量 0–2 人日（可选的性能优化：
把 WG 增量维护搬进 `doFill`，省一次 prime；不影响结果，需重跑 `chunkgen` /
`gen-world` 全部高度图比较）。依赖：无。

### 1.7 额外发现：生成期光照的**输入**还是 CARVERS — **部分 / 正确性风险**

`crates/mc-bamboo-game/src/worldgen_light.rs:98` 的 `generate_light_stage` 从
`resolve_shared_view` 拿 3×3 `Arc<GeneratedChunk>`，`:107` 直接把
`generated.blocks` 交给 `generate_light_from_layers`；`chunk_generator.rs:148`
在 `ChunkStatus::Light` 调它，传入的 `center_snapshot` / `neighbors` 是
`director_host.rs:2318` 在 `terrain` 阶段存下的 CARVERS 快照。
而官方 `ChunkStatusTasks.light`（`javap -c -p`）把 `WorldGenRegion` 里
**已经跑完 FEATURES 的**区块交给 `LevelLightEngine`；
`crates/mc-bamboo-world/src/light/generate.rs:1` 的模块注释也这么写。

`gen-light` 对拍（`tools/mc-bamboo-vanilla/java/GenLightOracle.java`）是确定性合成场景：
Java 和 Bamboo 拿**同一批 3×3 方块 ID** 分别跑官方光引擎和 Bamboo 光引擎，逐格比天空光 /
方块光；它证明光引擎和 NBT 回退一致，**不证明流水线挑的方块一致**。
`worldgen_light.rs:1-28` 的注释还停留在「当前已移植的地物只有矿石（不透明、不发光）」，
但现在树、树叶、萤石、灯笼、发光地衣、结构光源都已移植，CARVERS 与 FEATURES 后的
遮挡 / 发光不再等价。

**用户可见影响**：树冠下天空光偏亮、萤石 / 灯笼 / 海晶灯 / 结构光源周围方块光缺失；
客户端收到 `isLightOn=true` 的区块，运行时不会自动补算，差异会一直留着。
工作量 2–4 人日：把 §1.2 的 `feature_deltas` 合并规则复用到 `light` 阶段输入
（或让 `partials` 在 FEATURES 后更新快照），加一条「官方 FEATURES 后 3×3 → 官方 LIGHT
vs 游戏 LIGHT」的整块 oracle，重跑 `gen-light` 和 `structure-e2e`。
依赖：`feature-merge` 的固定先后合并、`GenerateOutput.light` 装配顺序。

---

## 2. 生成时的刷怪（`NaturalSpawner.spawnMobsForChunkGeneration`）— **没做**

### 官方在哪一步、用到什么

`javap -c -p net.minecraft.world.level.chunk.status.ChunkStatusTasks` 的 `generateSpawn`
（字节码 274–292）：`ChunkAccess.isUpgrading()` 为假时，用中心 + `ChunkStep` 依赖表造
`WorldGenRegion`，调 `ChunkGenerator.spawnOriginalMobs(region)`；
`javap -c -p net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator` 的
`spawnOriginalMobs`（1039–1079）：先看 `NoiseGeneratorSettings.disableMobGeneration()`，
再用 `WorldgenRandom(LegacyRandomSource(RandomSupport.generateUniqueSeed()))`
+ `setDecorationSeed(worldSeed, chunkMinX, chunkMinZ)`，以 `center.getWorldPosition().atY(maxY)`
为查询点调 `NaturalSpawner.spawnMobsForChunkGeneration(region, pos, chunkPos, random)`。
`FlatLevelSource.spawnOriginalMobs` 是空实现（超平坦不刷）。

`javap -c -p net.minecraft.world.level.NaturalSpawner`：
- 先查 gamerule `SPAWN_MOBS`（`gamerules.rs:72` 已有 `SPAWN_MOBS` 常量，但没有读取点）；
- 从 `environmentAttributes().getValue(NATURAL_MOB_SPAWNS, pos)` 取该位置的
  `MobSpawnSettings`，只取 `MobCategory.CREATURE` 的加权表；
- 从 `CREATURE_WORLD_GEN_SPAWN_PROBABILITY` 取概率（默认 **0.1**，
  少数群系覆盖成 0.03 / 0.04 / 0.07，见官方 `data/minecraft/worldgen/biome/*.json` 的
  `attributes.minecraft:gameplay/creature_world_gen_spawn_probability`）；
- 循环 `random.nextFloat() < probability` → 加权表选 `SpawnerData` →
  `IntProvider.sample(random)` 取 count → 区块内随机 (x,z) →
  `getTopNonCollidingPos` 按 `SpawnPlacements.getHeightmapType` 查高度图 →
  `EntityType.canSummon` / `SpawnPlacements.isSpawnPositionOk` / `noCollision` /
  `SpawnPlacements.checkSpawnRules` → `EntityType.create(..., NATURAL)` →
  `snapTo(x, y, z, random.nextFloat()*360, 0)` → `Mob.finalizeSpawn(..., CHUNK_GENERATION, ...)`
  → `addFreshEntityWithPassengers`，失败的尝试最多 4 次 / 只，位置越界时在初始列附近重取。

官方 26.3 数据包 67 个群系的 `CREATURE` 表共 **20 种**实体
（`data/minecraft/worldgen/biome/*.json` 统计）：
`chicken`(28 群系)、`sheep`(27)、`pig`(26)、`cow`(25)、`rabbit`(13)、`wolf`(9)、
`armadillo`(6)、`fox`(6)、`donkey`(6)、`strider`(5)、`horse`(5)、`polar_bear`(4)、
`llama`(4)、`goat`(3)、`parrot`(2)、`panda`(2)、`frog`(2)、`turtle`(1)、
`camel`(1)、`mooshroom`(1)。常见主世界群系（平原）是羊 / 猪 / 鸡 / 牛 / 马 / 驴；
下界群系里还有 `strider`。

### Bamboo 现状

`crates/mc-bamboo-game/src/chunk_generator.rs:186` 的 `_ => passthrough_stage(center)`
覆盖 `ChunkStatus::Spawn` / `Full`；`ChunkStatus::ALL` 十个阶段都在跑，但 spawn 不写任何东西。
`crates/mc-bamboo-blocks/src/entity/mob/mod.rs:39` 的 `MobType` **只有 `Cow`**，
`entity/mod.rs:199` 的 `EntityKind` 也只有下落方块 / TNT / 物品 / 经验球 / `Mob`。
`worldgen.json` 没有导出群系的 `NATURAL_MOB_SPAWNS` 环境属性或
`CREATURE_WORLD_GEN_SPAWN_PROBABILITY`；`worldgen_spawn.rs` 那个 `spawn` 是
`setInitialSpawn`（玩家出生点），不是生成期刷怪。没有 `spawnMobsForChunkGeneration` 的 oracle。

**用户可见影响**：新区块生成时没有任何动物；官方开新世界平原上能看到的牛羊猪鸡都没有，
后续也没有每刻自然刷怪（`NaturalSpawner.spawnForChunk`）来补，因为实体系统还没做。
工作量分两档：
- 只做「生成期刷牛」最小闭环：导出群系 CREATURE 表 + 环境属性、随机数链、
  `SpawnPlacements` / `finalizeSpawn` 的取数、`addFreshEntityWithPassengers` 落进实体表与存档、
  JVM oracle（含 `isUpgrading` 分支）→ **5–10 人日**，依赖实体存档和 `docs/mob-entity-design.md`；
- 20 种实体全做（含各自的 `SpawnPlacements` / `finalizeSpawn` 变体、AI、同步包）→ **30–60 人日**，
  和 `docs/zombie-skeleton-plan.md` 的生物系统是一整个项目。

**建议**：此项需要用户拍板实体范围，不属于「马上能收尾」的小项。

---

## 3. 生成期光照：确认已做完（`gen-light`）— **做完**

`crates/mc-bamboo-game/src/worldgen_light.rs:98` 在 `ChunkStatus::Light` 调
`mc_bamboo_world::light::generate::generate_light_from_layers`，产出 `GeneratedLight`；
`director_host.rs:2116` 在 `Task::Assemble` 里 `set_generated_light` 并标 `isLightOn=true`，
交付时不再等运行时光照引擎补算。`tools/mc-bamboo-vanilla/src/gen_light.rs` 用
`GenLightOracle.java` 的 3×3 合成场景（实心地面 + 随机光源 / 透光 / 遮光方块 / 挖空，
外圈 5×5 全空气）逐格比天空光和方块光：本轮 6 / 8 / 12 场景、每场景 196608 格，
全 0 不一致；`/tmp/verify-main2.sh` 常规跑 `gen-light java` 也 0。

**过时说法**：`docs/worldgen.md` 原来写「生成期光照 `initialize_light` / `light`
的真实计算是下一轮」「光照由运行时引擎在交付后补算」，已经不对，本轮改掉。
**但**「计算做完」不等于「流水线输入正确」，见 §1.7：这一条要分开写，不能把
`gen-light` 的 0 不一致当成整块生成期光照已经和官方一致。

---

## 4. 下界和末地

### 4.1 生成本身 — **部分（数学层做完，真维度生成没做）**

**已经在代码里、有对拍的**：
- `crates/mc-bamboo-data/src/generated/worldgen.json` 导出 7 套 `noise_settings`
  （含 `minecraft:nether` / `minecraft:end`）、`minecraft:nether` 多噪声群系预设、
  `end_biomes` 五个群系；`crates/mc-bamboo-worldgen/src/lib.rs:337` 的单测确认三套 settings
  都能编译 `SeededRouter`，`:402` 测过 `TheEndBiomeSource` 的中心/外圈分带。
- `worldgen java` 逐值比过主世界 / 下界 / 末地的噪声、密度函数、8 条路由输出和群系
  （本轮扩到 8 种子 / 56 维度状态，0 不一致）。
- `crates/mc-bamboo-worldgen/src/aquifer.rs:166` 的 `new_disabled` 实现了官方
  `Aquifer.createDisabled`（无 `aquifers` 配置时只用全局流体）；`chunk.rs:379` 的
  `make_aquifer` 自动切换；`chunkgen --settings nether|end --through carvers`
  6 种子 × 36 区块 × salt 987654321 三阶段 0 不一致。

**但这些对拍不等于「下界/末地地形做完」**：`ChunkgenOracle.java:920` 只对
`minecraft:nether` 换 `minecraft:nether` 多噪声预设，`minecraft:end` 的 header 仍写
`preset = minecraft:overworld`，即末地用的是主世界群系源 + 末地 `NoiseGeneratorSettings`，
**不是**官方 `TheEndBiomeSource`。`chunkgen.rs:382` 也只允许两个 preset。
真正的末地岛密度 / 五群系分带、下界基岩顶底与老 fbm、`legacy_random_source` 的完整路径
没有区块级对拍。

**地物 / 结构缺口**：
- `features_per_step` 只按主世界群系并集导出 171 个全局 index
  （`crates/mc-bamboo-worldgen/src/feature/run.rs:176` 的注释明说下界 / 末地 placed
  feature 不在表里会标 `unsupported`）。`chunkgen --through features --settings nether`
  实测所有中心 skipped；`--settings end` 虽 0 skipped，也只是因为该批坐标的群系没触发
  末地专属 placed feature，不能算通过。
- 官方数据包里末地 / 下界专属 Feature 类型没有移植：`end_island`、`end_spike`、
  `end_platform` / `void_start_platform`、`end_gateway_delayed` / `end_gateway_return`、
  `chorus_plant`、`huge_fungus`、`delta_feature`、`netherrack_replace_blobs`、
  `spring_*_nether`、`basalt_pillar` 的 `cap_feature`、`crimson/warped_fungi` 等
  （`docs/worldgen-features-plan.md` §5 和 `worldgen.json` 的 `feature` / `placed_feature` 表）。
- 结构：`structure/pipeline.rs:437` 明说 `fortress` / `end_city` / `bastion_remnant` /
  `nether_fossil` 不会生成起点；`place_step.rs:47` 的允许清单没有 `end_city` /
  `fortress` / `nether_fossil`。末地城、下界要塞、堡垒遗迹的布局 / 放置都没移植。
- 末影龙战斗、黑曜石柱、返回传送门、末地折跃门、末地出生平台属于
  `EndDragonFight` / 维度初始化，不是 `ChunkStatus` 流水线；仓库里没有对应模型。
- `docs/worldgen-structures-plan.md:5` 明确「本轮只做主世界（下界、末地没有维度）」，
  `README.md` 也写「下界和末地没做」。

**用户可见影响**：当前还不能进下界 / 末地，所以不可见；一旦做 §4.2，没有这些生成内容
就是空岛 / 石头世界。工作量 **20–40 人日**：真末地群系源和 `chunkgen --settings end`
oracle 3–5；下界 / 末地专属地物 10–20；四类结构 8–15；末地维度初始化 / 龙战斗
另算 10–20。依赖：扩展 `Extractor.java` 的 `features_per_step`（要按三个维度的群系并集
固化全局 index）、`ChunkgenOracle` 支持 `the_end` preset、§4.2 的维度框架（纯函数可以先做）。

### 4.2 游戏侧多维度 — **没做（大项目，需用户拍板）**

**代码事实**：
- `crates/mc-bamboo-game/src/region.rs:66` 的 `DIMENSION` 常量写死
  `"minecraft:overworld"`；`:699` 的 `Login.dimensions = &[DIMENSION]`、
  `:703` 的 `dimension_type = OVERWORLD`、`:802` 的 `SetDefaultSpawnPosition` 都用它；
  `player_respawn.rs:163` / `:199` 的 `Respawn` 也写死主世界。
- `crates/mc-bamboo-storage/src/world_dir.rs:114` 的 `OVERWORLD` 路径常量
  `dimensions/minecraft/overworld` 决定 `region/` / `entities/` / `poi/` / `data/`；
  没有 `DIM-1` / `DIM1` 或按维度选目录的参数。
- `WorldConfig`（`world.rs:142`）只有 `islands: bool`、`world_dir: Option<PathBuf>`、
  一份 `WorldGenConfig`；`World` / `DirectorHost` / `Global` / `TicketTable` 都只有一份。
- 岛（`island.rs:293` 的 `Island`、`:545` 的 `Islands`）是**同一维度内**按玩家位置切分的
  `BTreeMap<RegionId, IslandSlot>`，没有维度字段；`docs/world-model-design.md` §16 第 4 步
  的状态栏也写「现在只有主世界一个维度，『维度之间并行』没有对象」。
- 协议层 `Login.dimensions` / `CommonPlayerSpawnInfo`（`packet/play/mod.rs:60` / `:244`）
  已经能编多维度列表和 `dimension_type` ID，`registries.rs:146` 也有
  `THE_END` / `THE_NETHER` 的 `dimension_type` 常量和两份 `world_clock`，但游戏层没用上。

**要让玩家真的进下界 / 末地**，至少要做：
1. 每个维度一份 `World` / `Global` / `Director` / `TicketTable` / `ChunkStore` / `Islands`，
   或把现有结构改成按 `DimensionId` 索引；`WorldDir` 按维度派生目录；
   `level.dat` / `world_gen_settings.dat` 的 `dimensions` 注册表写出三个维度。
2. 岛的分区器 / 离境到达 / 命令路由 / 票表全部带上维度键，跨维度不能拆合；
   `IslandIdentity` / `RegionId` 现在只在单维度内唯一。
3. `Login` 发三个维度；换维度发 `Respawn`（`data_to_keep` / `last_death` / `portal_cooldown`）、
   重新发边界 / 时间 / 天气 / 玩家能力 / 区块流；重生回主世界按 `RespawnData` 的维度锚点。
4. 传送门：下界传送门方块已能点亮（`README.md`），但 `PortalForcer` / 坐标换算 /
   传送门冷却 / 末地传送门 / 末地折跃门 / 返回传送门都没有；旁观者右键传送门目前只发
   `spectator.cannot_teleport`（`interaction.rs:905`）。
5. 每维度一套生成配置（`LevelType` 现在是主世界预设枚举，没有 `nether` / `end`），
   接上 §4.1 的生成纯函数；末地还要龙战斗 / 末地平台初始化。

**用户可见影响**：这是「能不能玩下界 / 末地」的分界线；不做就只有主世界。
工作量 **40–80 人日**（架构改造 15–25、协议 / 传送门 / 重生 10–20、存档与岛 10–20、
联机探针 5–15），依赖**用户先拍板**：是做完整官方维度，还是只做「能传送过去看地形」的
最小版本；以及和 `docs/island-implementation-plan.md` 的岛设计怎么配合
（建议：维度是 `World` 级隔离，岛仍是维度内的分区，跨维度绝不合并）。

---

## 5. 性能 — **只记录，不优化**

### 本轮在工作树自己 `target` 的 `wg-perf`

```bash
unset CARGO_TARGET_DIR
target/release/mc-bamboo-vanilla wg-perf --seeds 1 --chunks 4 --repeat 1 --skip-aux
target/release/mc-bamboo-vanilla wg-perf --seeds 3 --chunks 8 --repeat 3 --skip-aux
```

单线程、release、不含 `build_view`（工具由调用方给 3×3 视图，真实 Director 路径另算）：

| 阶段（中位数 ms/区块） | 1 种子 × 10 群系 × 4 | 3 种子 × 10 群系 × 8 × 3 遍 |
|---|---:|---:|
| `biomes` | 2.633 | 3.111 |
| `noise_density` + `substance` | 2.396 | 2.727 |
| `surface` | 2.245 | 2.752 |
| `carver_mask` + `carver_apply` | 1.138 | 1.361 |
| `features` | 3.817 | 5.376 |
| `light` | 5.301 | 5.294 |
| `assembly` | 0.796 | 0.886 |
| **墙钟合计** | **19.536** | **27.542** |

第二轮机器上有另外两个子代理在编译，绝对值只作趋势参考。`build_region_column`
之外还有 `build_view` 3×3 地形（工具实测每区块 78.7–125.5 ms，真实游戏里由
`partials` 的 CARVERS 快照缓存摊掉）。

### 仓库里已有的同机 / 官方对比

- `docs/worldgen.md` §「整块生成再降一轮」：同一轮交替量 `9495ddbb` 与优化后二进制，
  墙钟中位数 **20.303 / 18.561**、**21.659 / 18.259** ms/区块；
  `biomes 2.1`、`noise_density 1.5`、`features 3.7`、`light 4.3`、`assembly 0.8`。
- `docs/worldgen.md` §「真实 Director 流水线」：单线程分阶段约 **44.6 ms/区块**
  （terrain 8.0 / `game_features` 30.6 / light 5.5 / assembly 0.7）；
  `director_pipeline_bench` 开结构时 1/2/4/8 线程计算池 CPU 每交付区块
  **43.5 / 46.6 / 52.0 / 62.4 ms**，吞吐 **21.0 / 40.2 / 62.5 / 70.6 区块/秒**。
- `docs/performance-optimizations.md` §10 / `docs/worldgen.md` §「拆分表」：给官方
  `ChunkgenOracle` 的 `BiomeResolver` 临时包计时器，1 种子 × 120 区块量到官方
  `doFill 4.84 ms`、`buildSurface 101.1 ms`、`generateCarvers 1.70 ms`，
  整 `terrain` **约 108 ms/区块**（插桩本身有开销，且这条 oracle 路径用 uncached
  resolver；Bamboo 优化后 17 ms，快约 6 倍）。**这不是同口径的整块对比**：
  官方数字只到 CARVERS、带插桩；Bamboo 的 18–27 ms 是整块到组装。
- 飞行压测（`docs/worldgen.md`）：6 名飞行玩家 / 视距 8 / 4 线程时 tick 不被拖慢
  （p50 0.12–1.18 ms），但 `compute_backlog` 稳态 155–328、区块延迟 p50 涨到 5.8–12.6 s，
  吞吐跟不上。那是更早一版按线程各存 CARVERS 缓存的数字，缓存改成全进程共享后没重跑。

**结论**：当前每区块 20 ms 量级（纯函数）/ 45–70 ms（真实 Director 含邻居与池），
官方同机只有插桩到 CARVERS 的 108 ms 可引用；没有「官方整块生成端到端」的同机数字。
这一轮按任务要求**只记录，不优化**。若以后优化，优先项由数据看是 `build_view` /
邻居地形重算、`features` 的 3×3 视图、`light` 传播；每项都要重跑 `chunkgen` /
`gen-world` / `gen-light` / `structure-e2e` 证明逐位不变。

---

## 6. 其他在代码里发现的缺口

| 缺口 | 代码位置 | 用户可见影响 | 量级（人日） | 依赖 |
|---|---|---|---:|---|
| 出生点 `setInitialSpawn` 仍在 CARVERS 快照上算 | `worldgen_spawn.rs:11`、`:190`；`SpawnOracle.java` 只比到 CARVERS | 树 / 雪层 / 结构改变 `MOTION_BLOCKING` 时，个别种子的出生点和满配官方世界不同 | 1–2 | §1.2 完工方块、`full` 阶段 |
| `PostProcessing` 位置表恒写 24 个空列表，官方 `ProtoChunk` 其余字段未逐项对拍 | `chunk_nbt.rs:790`、`docs/m3-storage.md` §5.1 | 官方读档后再做一次后处理时行为不同；当前 Bamboo 自己不读这个表 | 2–4 | 地物 / 结构的 `markPosForPostprocessing` |
| 生成期实体不进假世界，只保证随机序列和方块写入一致 | `docs/worldgen-structures-e2e.md` §已知限制、`ChunkgenOracle.java:2329` | 村庄 / 府邸 / 海底神殿没有村民、悦灵、守卫者、女巫、猫 | 见 §2 | 实体系统 |
| `features` 邻居溢出合并是「固定来源先后、先写的赢」，官方由调度决定 | `worldgen_features.rs:99`、README 已知限制、`feature-merge` oracle | 边界上重叠写入可能和某一次官方生成不同；本轮 `feature-merge` 3 / 5 行 0 differing | 2–5（若要改成官方调度语义） | Director 任务结构 |
| 超平坦 / 噪声世界的 `generate-structures=false` 语义不同 | `worldgen_structures.rs:410`、`:419`、`worldgen_features.rs:685`、`world.rs:467` | flat 里关不关结构都一样（本来就没结构） | 见 §1.3 | §1.3 |
| 并行化 / SIMD | `docs/worldgen.md` 末节 | 影响吞吐，不影响逐位一致 | 3–10（可选） | 全部对拍基线 |
| `docs/worldgen.md` 开头、§「接进游戏」、§「第二轮」、§「结构阶段」还有早期记录 | `worldgen.md:1`、`:580`、`:611`、`:945` | 只影响读者判断 | 0.5 | 本清单 |

---

## 7. 工作量与依赖汇总

| 工作包 | 人日 | 前置依赖 | 是否需要用户拍板 |
|---|---:|---|---|
| 修 `gen-world` 三处 `LevelType::Normal` + `PaleMossDecorator` 的 JVM 桩，重跑扩大样本 | 0.5–1 | 无 | 否 |
| 生成期光照输入改成 FEATURES 后视图 + 整块 oracle | 2–4 | `feature-merge` 合并规则 | 否 |
| `pale_moss` 装饰器 / 苍白橡树 + 逐类型对拍 | 2–3 | 上面的 JVM 桩 | 否 |
| 超平坦 `features` / `lakes` / `structure_overrides` | 3–6 | 结构 / 地物纯函数；flat 是否走 `Staged` | 是（范围） |
| 非 `full` 读盘续跑 + 多阶段往返 oracle | 5–8 | `chunk_content::decode` 改接口、存档层 | 否 |
| 出生点改用 `full` 地形 | 1–2 | 完工方块、§1.7 | 否 |
| `PostProcessing` / `ProtoChunk` 其余字段 | 2–4 | 地物 / 结构写路径 | 否 |
| `Heightmap.update` 增量维护（纯性能） | 0–2 | 无 | 否 |
| 生成期刷怪（最小：只放牛） | 5–10 | 环境属性导出、实体表 / 存档、新 oracle | **是（实体范围）** |
| 生物系统 + 20 种生成期实体全量 | 30–60 | `docs/mob-entity-design.md`、`zombie-skeleton-plan.md` | **是** |
| 下界 / 末地真地形 + 专属地物 + 四类结构 | 20–40 | `features_per_step` 按三维度导出、真 `TheEndBiomeSource` oracle | **是（范围）** |
| 多维度框架 + 岛 + 存档 + 协议 + 传送门 + 重生 | 40–80 | 上面生成层、岛设计、联机探针 | **是（大项目）** |
| `BlendingData` / `BelowZeroRetrogen` | 8–15 | 老存档升级策略 | **是（要不要支持老存档）** |
| 性能优化 / SIMD | 3–10 | 全部对拍基线 | 否（本轮不做） |

---

## 8. 建议的分期和顺序

### 第 0 期：主世界收尾，可以马上做（约 1–2 周，合计 8–16 人日）

1. **先修对拍工具两处缺口**（0.5–1 人日）：`gen-world` 的 `level_type`、
   `ChunkgenOracle` 的 `ServerLevel.chunkSource`。修完把 §0.2 的扩大样本固化为常规回归，
   尤其是 `salt 1` 的多种子全地物和三种预设的 `gen-world --through features`。
2. **生成期光照输入**（2–4 人日）：把 FEATURES 后的写折进 `light` 阶段，加整块 oracle；
   这是当前唯一有明确正确性风险、且玩家容易看出来的主世界缺口。
3. **`pale_moss` / 苍白橡树**（2–3 人日）：补 `Decorator::parse` 和
   `pale_moss_patch` 递归放置，撤掉 `TreeConfig` 里 `pale_oak is_none()` 的单测，
   在 `feature-e2e.sh` 加 case。
4. **小尾巴**（1–4 人日）：出生点改读 `full` 地形；`PostProcessing` /
   `ProtoChunk` 字段；把 `docs/worldgen.md` / README 里其余早期描述同步。
5. **非 `full` 续跑**（5–8 人日）：如果第 0 期有余量就做，否则放第 1 期开头；
   它动存档接口，适合单独一个可回滚的提交序列。

**验收**：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets` 0 警告、
`cargo test --workspace`、`BAMBOO_TEST_LOADED=1 cargo test -p mc-bamboo-game`，
以及 `/tmp/verify-main2.sh` 里全部生成类 oracle + 扩大样本 + 联机探针。

### 第 1 期：需要用户拍板的中型项目（约 2–6 周）

- **A. 超平坦完整生成**（3–6 人日）：是否要把 flat 也接进 `Staged` 流水线。
- **B. 老存档升级**（8–15 人日）：是否承诺兼容 26.3 之前存档；若否，只写文档。
- **C. 生成期刷怪**（5–10 / 30–60 人日）：先定实体范围（只放牛 vs 20 种全量）。
  C 依赖生物系统，建议和 `zombie-skeleton-plan.md` 合并排期，不要塞进「主世界收尾」。

### 第 2 期：大项目，必须用户先定范围（数月）

1. **下界 / 末地生成本身**（20–40 人日）：可以先只做纯函数 + oracle，不接维度，
   这样能独立验证；顺序是真末地群系源 → 三维度 `features_per_step` → 专属地物 →
   下界要塞 / 堡垒遗迹 / 末地城 → 末地平台 / 黑曜石柱 / 折跃门 / 龙战斗。
2. **多维度游戏侧**（40–80 人日）：维度是 `World` 级隔离，岛仍是维度内的分区；
   维度键要贯穿 `WorldDir` / `TicketTable` / `Islands` / `Login` / `Respawn` /
   传送门 / 重生。建议先做「单玩家能进下界并回来」的探针，再铺多人 / 岛 / 存档。
3. 两项都做完才重跑飞行压测和 `wg-perf`，评估 `chunk_worker_threads` 默认值。

### 不建议现在做的

- 并行化 / SIMD：逐位一致的基线还没把 §1.7 修完，先别动求值顺序。
- 把 `features` 邻居合并改成官方调度语义：当前 `feature-merge` 0 differing，
  改它要动 Director 任务结构，收益不确定，应等有实际不一致案例再做。

---

## 9. 交给主代理审的疑点

1. `gen-world --level-type large_biomes|amplified` 的几百万不一致已判定为对拍工具把
   `level_type` 写死 `Normal`；文档按「工具缺口」记录而非「生成不一致」。请复核这个定性。
2. `worldgen_light.rs` 用 CARVERS 快照算光、官方用 FEATURES 后视图，已列为主世界正确性缺口；
   `gen-light` 的 0 不一致只覆盖光引擎。请复核是否要把它排在第 0 期第 2 项。
3. `chunkgen --settings end` 的 header 仍写 `preset=minecraft:overworld`，因此现有
   下界 / 末地地形 0 不一致不能算「生成本身做完」。请复核 §4.1 的工作量估计。
4. 生成期刷怪的实体范围（只牛 / 20 种全量）会显著改变人日数，
   需要用户拍板；本清单给了两档但没替用户选。
5. 本轮没有重跑 `tools/mc-bamboo-vanilla/feature-e2e.sh` 全量（重复后台副本被清理），
   逐类型 PASS 引用的是 `docs/worldgen-e2e-coverage.md`；第 0 期修完工具后应补一次全量。
