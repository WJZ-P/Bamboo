# 世界生成：地物（`features`）实施方案

目标版本 Minecraft Java 26.3。本文是 `terrain`（`docs/worldgen.md`）之后 `ChunkStatus.FEATURES`
那一步的实施方案：数据怎么导、纯函数长什么样、怎么和 Director 衔接、怎么对拍、按什么顺序移植。
官方行为全部按 `javap -c -p` 的字节码和官方 jar 里的 `data/minecraft/worldgen/*.json` 核对，
不凭印象。

## 0. 先纠正一个前提：26.3 没有 `configured_feature`

任务描述沿用旧版本的说法，26.3 已经改了数据结构：

* `RegistryDataLoader.WORLD_REGISTRIES` 的静态初始化里，index 4 是
  `Registries.FEATURE` + `Feature.DIRECT_CODEC`，index 5 是
  `Registries.PLACED_FEATURE` + `PlacedFeature.DIRECT_CODEC`，**没有 CONFIGURED_FEATURE**。
  官方 jar 里 `data/minecraft/worldgen/feature/` 有 240 条、`placed_feature/` 273 条、
  `configured_feature/` **0 条**。
* `Feature` 从类变成了 **interface**，`Feature.DIRECT_CODEC` 是
  `BuiltInRegistries.FEATURE_TYPE.byNameCodec().dispatch(codec())`：类型 ID（`FeatureTypes` 里
  58 个，例如 `minecraft:ore`、`minecraft:tree`、`minecraft:simple_block`）就是配置的类型，
  配置字段直接展开在对象上。旧的 `ConfiguredFeature<FC, F>` 包装没了。
* `PlacedFeature` 是 record `feature: Holder<Feature>` + `placement: List<PlacementModifier>`；
  `feature` 字段用 `Feature.CODEC`，可以是注册表引用（`"minecraft:ore_coal"`）也可以内联。
* `BiomeGenerationSettings` 里 `features()` 是
  `List<HolderSet<PlacedFeature>>`，下标就是 `GenerationStep.Decoration` 的 ordinal
  （11 个，从 `raw_generation` 到 `top_layer_modification`）。`hasFeature(pf)` 用预建的
  `Set<PlacedFeature>`（按对象身份，`Collectors.toSet()`）。
* `RandomPatchFeature` 也没了：26.3 的同类是 `ProjectedRandomPatchySquare`
  （类型 ID `minecraft:projected_random_patchy_square`），花草多数是 `simple_block`、
  `random_selector` / `weighted_random_selector` 之一，配 `block_predicate_filter` 等修饰器。

所以本文后面统一说 **feature 注册表**（不是 configured_feature）和 **placed_feature 注册表**。
方案里的键名也按这个来。

## 1. 官方这一步到底做什么

### 1.1 `ChunkGenerator.applyBiomeDecoration`

字节码（`.agent-tmp/ChunkGenerator.txt` 604–833 行）逐段核对：

1. `sectionPos = SectionPos.of(chunkPos, worldGenLevel.getMinSectionY())`，
   `origin = sectionPos.origin()`，即 `(chunkX*16, minSectionY*16, chunkZ*16)`。
2. `STRUCTURE` 注册表按 `Structure.step().ordinal()` 分组。本轮对拍把
   `StructureManager.shouldGenerateStructures()` 设为 false，**整段跳过**；结构本身还没移植。
3. `featuresPerStep = Suppliers.memoize(lambda$new$1)`，即
   `FeatureSorter.buildFeaturesPerStep(
        List.copyOf(biomeSource.possibleBiomes()),
        biome -> biome.value().getGenerationSettings().features(),
        true)`。
   输入是**群系源的全部可能群系**（主世界 67 个），不是中心区块真正出现的群系。
4. 随机源：`new WorldgenRandom(new XoroshiroRandomSource(RandomSupport.generateUniqueSeed()))`。
   内层种子是 `nanoTime ^ uniquifier`，但紧接着被 `setDecorationSeed` 覆盖，所以不进结果；
   关键是**算法是 Xoroshiro**，`WorldgenRandom.next(bits)` 走 `nextLong() >>> (64-bits)`
   （`mc-bamboo-worldgen::random::WorldgenRandom` 已按这个移植过）。
5. `decorationSeed = setDecorationSeed(level.getSeed(), origin.x, origin.z)`：
   `setSeed(seed)`；`a = nextLong()|1`；`b = nextLong()|1`；
   `result = (x*a + z*b) ^ seed`；`setSeed(result)`。y 不进种子。
6. `ObjectArraySet<Holder<Biome>> biomes`：对
   `ChunkPos.rangeClosed(sectionPos.chunk(), 1)` 的 9 个区块，把**每个 section 的 quart
   群系调色板**用 `PalettedContainerRO.getAll(biomes::add)` 收进集合（不是逐格采样），
   再 `retainAll(biomeSource.possibleBiomes())`。
7. 主循环 `step = 0 .. max(featuresPerStep.size(), Decoration.values().length)`（11）：
   * 先跑本 step 的结构放置（本轮跳过），每个结构 `setFeatureSeed(decorationSeed, index, step)`。
   * `IntArraySet indices`：按 `biomes` 的迭代顺序（ObjectArraySet 的插入序），对每个群系取
     `generationSettings.features().get(step)`（越界跳过），把该 HolderSet 里每个
     `PlacedFeature` 通过 `featuresPerStep[step].indexMapping()` 换成全局 index。
   * `int[] ordered = indices.toIntArray(); Arrays.sort(ordered)`。
   * 按 `ordered` 升序：`PlacedFeature pf = featuresPerStep[step].features().get(index)`，
     `setFeatureSeed(decorationSeed, index, step)`，
     `featurePlacer.placeWithBiomeCheck(pf, random, origin)`。
8. `setFeatureSeed(seed, index, step) = setSeed(seed + index + 10000*step)`。

**关键**：第 7 步用的是**全局 index**（每个 step 内从 0 开始的 `StepFeatureData.features()`
下标），不是该群系自己的局部下标。随机种子因此依赖 `FeatureSorter` 给出的全局排序。

### 1.2 `FeatureSorter` 的全局顺序

`buildFeaturesPerStep`（`FeatureSorter` 字节码 222–490 行）：

* 按输入群系列表的顺序、每个群系 `features` 的 step 顺序、每个 HolderSet 的迭代顺序，
  给每个 `PlacedFeature` **对象**（`Reference2IntOpenHashMap`，身份相等）分配递增编号。
* 每个群系内部的相邻 step 之间建「依赖边」：同一群系里前一个放置位置指向后一个
  （`FeatureData(featureIndex, step, feature)` 作为 `TreeMap` 的 key，比较器
  `comparingInt(featureIndex).thenComparingInt(step)`）。
* 对 keySet 做 `Graph.depthFirstSearch`，把访问序收集后 `Collections.reverse`，
  得到全局有序的 `FeatureData` 列表；每个 step 过滤出该 step 的 `PlacedFeature` 列表。
* `StepFeatureData` 还带一个 `Util.createIndexIdentityLookup(features)`，把同一个对象映射回
  `features` 里的下标。

这套顺序依赖 Java `ImmutableSet` 的迭代序、`TreeMap`/`TreeSet` 的比较器和 DFS 的邻接表顺序，
**不适合在 Rust 里重推**。方案是：**由 Java 侧把排好的顺序导出**，Rust 直接用。
（`ChunkGenerator.featuresPerStep` 是私有 `Supplier`，导出时用反射 `get()` 取
 `List<StepFeatureData>`，再逐条 `features()` + `PlacedFeature` 的注册表名。）

### 1.3 `FeaturePlacer` 的修饰器链

26.3 把旧的 `Stream<BlockPos>` 链改成了 `FeaturePlacer` 里的**显式栈 DFS**
（`FeaturePlacer.place`，字节码 598–780 行）：

```
positions = [origin]; modifierIndices = [0]; result = false
while positions 非空:
    pos = positions.removeLast(); i = modifierIndices.removeLast()
    placement[i].modify(ctx, random, pos, modifiedPositions::add)
    if i+1 < placement.size():
        for k in (modifiedPositions.len()-1) downto 0:   // 倒序压栈
            positions.push(modifiedPositions[k]); modifierIndices.push(i+1)
    else:
        for p in modifiedPositions (正序):               // 最终放置
            result |= Feature.place(level, generator, random, p)
    modifiedPositions.clear()
```

* `placement` 为空时直接调一次 `Feature.place(level, generator, random, origin)`。
* `PlacementContext(level, generator, Optional<PlacedFeature> topFeature)`；
  `placeWithBiomeCheck` 传 `Optional.of(pf)`，普通的 `place` 传 `empty()`。
* `PlacementFilter.modify`：`shouldPlace(...)` 为真才把**原 pos** 交给 consumer。
* `RepeatingPlacement.modify`：先 `count(random, pos)`，再发 `count` 次原 pos。

必须照抄这个栈顺序：它决定多个候选位置消费随机数的先后。

### 1.4 修饰器清单与频次

从 273 个 `placed_feature` 统计（全部 67 个群系用到的共 211 个）：

| 修饰器 | 次数 | 备注 |
|---|---:|---|
| `biome` | 211 | `BiomeFilter`：`level.getBiome(pos)` → `hasFeature(topFeature)` |
| `count` | 201 | `IntProvider.sample(random)` |
| `in_square` | 198 | 两次 `nextInt(16)` |
| `block_predicate_filter` | 155 | `BlockPredicate.test(level, pos)` |
| `heightmap` | 111 | `PlacementContext.getHeight(type,x,z) = ChunkAccess.getHeight()+1` |
| `offset` | 93 | 三个 `IntProvider` |
| `height_range` | 86 | `HeightProvider.sample(random, WorldGenerationContext)` |
| `rarity_filter` | 55 | `nextFloat() < 1/chance` |
| `surface_water_depth_filter` | 26 | `WORLD_SURFACE - OCEAN_FLOOR <= maxWaterDepth` |
| `environment_scan` | 12 | 沿方向逐格扫，两个 `BlockPredicate` |
| `count_on_every_layer` | 8 | 按高度层展开 |
| `noise_threshold_count` | 6 | 需要 `NormalNoise` |
| `noise_based_count` | 4 | 同上 |
| `surface_relative_threshold_filter` | 3 | 需要 `NoiseGeneratorSettings` 的 surface |
| `fixed_placement` | 1 | 无随机 |

`BiomeFilter` 用的 `level.getBiome(BlockPos)` 是
`LevelReader.getBiome` 默认实现 → `BiomeManager.getBiome(pos)`，即 **8 角 fiddled distance
逐点平滑**（`mc-bamboo-worldgen::biome_manager` 已有，含 `ClimateCache`）。**不能**只读
`BIOMES` 阶段存下的 quart 格。

### 1.5 `WorldGenRegion` 的读写范围

* `ChunkPyramid.GENERATION_PYRAMID` 的 FEATURES 步（`lambda$static$5`）：
  `addRequirement(STRUCTURE_STARTS, 8)` + `addRequirement(TERRAIN, 1)` +
  `blockStateWriteRadius(1)`。`WorldGenRegion.writeRadius = 1`。
* `isWithinWriteZone(cx, cz)` = `|cx-centerX| <= 1 && |cz-centerZ| <= 1`，即 **3×3**。
  读超出这个范围只打 `Util.logAndPauseIfInIde` 日志，仍尝试 `getChunk`；写超出则
  `ensureCanWrite` 返回 false，直接丢弃。
* `getHeight(type, x, z)` = 对应 chunk 的 `ChunkAccess.getHeight(type, x&15, z&15) + 1`。
  `ChunkStatus.FINAL_HEIGHTMAPS` 含 `OCEAN_FLOOR`、`WORLD_SURFACE`、
  `MOTION_BLOCKING`、`MOTION_BLOCKING_NO_LEAVES`，在 `terrain` 阶段就该 primed。
* `getNoiseBiome(quart)` 走 `getChunk(...).getNoiseBiome(...)`（读已存的 quart 格）；
  `getBiome(BlockPos)` 走 `BiomeManager` 的逐点平滑（uncached resolver）。
* `WorldGenRegion` 构造还要 `ServerLevel.uncachedBiomeResolver()`、
  `getChunkSource().randomState()`、`getSeed()`、`dimensionType()` 等。

### 1.6 各 step 的工作量（按 67 个群系统计 `features[step]` 的条目数）

| step | 名称 | 条目数 | distinct placed | 主要 feature 类型 |
|---:|---|---:|---:|---|
| 0 | raw_generation | 1 | 1 | — |
| 1 | lakes | 112 | 4 | `lake`、`root_system`、`sequence` |
| 2 | local_modifications | 64 | 6 | `geode`、`iceberg`、`block_blob` |
| 3 | underground_structures | 118 | 4 | `monster_room`、`fossil` |
| 4 | surface_structures | 10 | 9 | 末地/下界类 |
| 5 | strongholds | 0 | 0 | — |
| **6** | **underground_ores** | **1635** | **34** | **`ore`(1414)、`disk`(165)、`underwater_magma`(56)** |
| 7 | underground_decoration | 86 | 30 | `spring_feature`、`scattered_ore`、`simple_block` |
| 8 | fluid_springs | 114 | 3 | `spring_feature` |
| 9 | vegetal_decoration | 528 | 118 | `simple_block`、`tree`、`random_selector`、patch 类 |
| 10 | top_layer_modification | 58 | 3 | `freeze_top_layer` |

step 6 的 34 个 placed feature 只用到 9 种修饰器：
`in_square`(34)、`biome`(34)、`height_range`(30)、`count`(28)、`heightmap`(4)、
`block_predicate_filter`(4)、`rarity_filter`(4)、`offset`(1)、
`surface_relative_threshold_filter`(1)。配置里的 `BlockPredicate` 只有 5 种：
`tag_match`(124)、`all_of`(76)、`any_of`(38)、`height_match`(38)、`not`(38)。
`RuleTest` 在矿石 `targets` 里是同样的几种（外加 `always_true` / `block_match` / `random_block_match`）。

## 2. 数据导出

### 2.1 加三个顶层键

`Extractor.java` 的 `worldgen(RegistryAccess)`（现有 1386 行）加：

```java
worldgen.add("feature",
        encodeRegistry(registries, ops, Registries.FEATURE, Feature.DIRECT_CODEC));
worldgen.add("placed_feature",
        encodeRegistry(registries, ops, Registries.PLACED_FEATURE, PlacedFeature.DIRECT_CODEC));
worldgen.add("biome_features", biomeFeatures(registries));
worldgen.add("features_per_step", featuresPerStep(registries));
```

* 前两个沿用现有的 `encodeRegistry`（按注册名排序，用官方 `DIRECT_CODEC` 编回 JSON）。
  `Feature.DIRECT_CODEC` 需要 `RegistryOps`，已有 `ops`；`BlockState` / `BlockPredicate` /
  `RuleTest` / `IntProvider` / `HeightProvider` / `BlockStateProvider` 都会由官方 codec
  展开成普通 JSON，`mc-bamboo-data` 里已有的方块状态/标签表足够解析。
* `biome_features`：按群系注册名排序，值是 11 个 step 的数组，每个 step 是
  PlacedFeature 注册名（或 tag）的字符串数组。等价于 `biome_carvers` 的做法，
  直接从 `Biome.getGenerationSettings().features()` 取 `HolderSet`：
  由 `unwrapKey()` 取注册名，标签用 `#minecraft:...`。
* `features_per_step`：**`FeatureSorter` 排好的全局顺序**。最稳的做法是在 Extractor 里
  建一个 `NoiseBasedChunkGenerator`（已有 `Registries.NOISE_SETTINGS` 和
  `MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST`，和 `ChunkgenOracle.java` 一样），
  反射读私有 `featuresPerStep` 这个 `Supplier` 并 `get()`，再对每步的
  `StepFeatureData.features()` 用 `Registry<PlacedFeature>.getKey(pf)` 取名字。
  这样 Rust 侧不需要重推拓扑序，索引与 `setFeatureSeed` 用的 index 天然一致。
  **如果这一步在 Extractor 里不好造（要 `ChunkGenerator` 实例），退路是在
  `ChunkgenOracle.java` 里导出，或由 Rust 侧按字节码重现 `FeatureSorter`；优先前者。**

  为了不依赖「用一个特定 generator 实例」，还要在导出时断言：
  `getBiomeSource().possibleBiomes()` 就是主世界预设里的 67 个群系，且
  `featuresPerStep.size() == 11`。

### 2.2 重跑无 diff

`docs/worldgen.md`「数据的导出与重跑」要求 `worldgen.json` 能重跑无 diff：

* 新增的四个键都走现有的 `encodeRegistry` / 手写 JsonArray，Gson 的
  `JsonPrimitive(double)` 保留 IEEE 754 double；`generate.rs::write_worldgen` 的
  `check_worldgen_floats` 会对整棵树逐个浮点比位模式，新增键自动受保护。
* `serde_json` 在工作区开了 `float_roundtrip`，新键里的 `discard_chance_on_air_exposure`
  等浮点也会按 IEEE 754 舍入。
* 步骤：`cargo run -p mc-bamboo-vanilla -- extract`（用现有报告），再跑一次 `generate`
  确认 `git diff -- crates/mc-bamboo-data/src/generated/worldgen.json` 为空；
  然后 `chunkgen --through carvers` 与 `worldgen java` 复跑，确认旧阶段的 0 不一致没被破坏。
* **不要**把 `generate` 顺带重写的其它 `generated/` 文件混进同一个提交
  （`saved_world.rs` 由另一个子代理在改，本任务只提交 `worldgen.json` 和必要的
  `worldgen.rs` 注释）。

### 2.3 Rust 侧解析

`mc-bamboo-worldgen` 新增 `feature/` 模块：

* `feature::Registry`：把 `feature` 键解析成 `Vec<FeatureDef>`（类型 ID + 原始 `Value`），
  按名字索引；`placed_feature` 解析成 `PlacedFeature { feature: String, placement: Vec<Value> }`。
* `feature::Steps`：读 `features_per_step`，得到 `Vec<Vec<usize>>`（每个 step 的
  `PlacedFeature` 索引；索引即 `FeatureSorter` 的全局 index）。
* `feature::Biomes`：读 `biome_features`，建 `Vec<Vec<Vec<String>>>`（群系 ID → step →
  placed feature 名），以及每个群系的 `HashSet<PlacedFeatureId>` 供 `BiomeFilter.hasFeature`。
* `Worldgen::parse` 的必需字段检查从现有的 4 个（`material_rule` / `material_condition` /
  `carver` / `biome_carvers`）加上这 4 个新键；缺了明确报错。
* 方块状态解析：26.3 的 codec 用 `{"Name": "minecraft:stone"}` 或
  `{"id": "...", "properties": {...}}` 两种形式（见 `spring_water.json`）。
  `mc-bamboo-data` 的 `block_by_name` + `PropertyDef.values` 可以按属性名/值算出状态 ID
  （`first_state` + 笛卡尔积下标，最后一个属性变化最快）；现有的
  `chunkgen.rs::state_of` 只处理 `BlockState.toString()`，这里要新写一个
  `feature::state::parse`，并加单元测试覆盖带 `properties` 的状态。
* `BlockPredicate` / `RuleTest` 的求值需要按状态 ID 判断方块标签：
  `block_id_of_state(state)` + `block_tag(name)` 已够用。

## 3. 结构与纯函数入口

### 3.1 一个 3×3 的区块视图

现有 `Generator::terrain` 返回 `Vec<Vec<u32>>`（384 层 × 256 个状态 ID）。地物阶段把
9 个这样的数组拼成一个可读写视图：

```rust
/// 中心区块加周围 8 个区块，各自是 `terrain` 阶段跑完的方块。
/// 下标顺序固定为 (dz, dx) = (-1..=1, -1..=1)，中心在 [(0,0)]。
pub struct FeatureChunks {
    pub center_x: i32,
    pub center_z: i32,
    pub min_y: i32,
    pub height: i32,
    /// 9 个区块，每块 `height` 层 × 256 个状态 ID。
    pub chunks: [Vec<Vec<u32>>; 9],
    /// 9 个区块各自的 quart 群系格（BIOMES 阶段的结果），供 `getNoiseBiome`。
    pub biomes: [Vec<Vec<u32>>; 9],
    /// 4 张 primed heightmap（每块 256 个 i32），供 `PlacementContext.getHeight`。
    /// 由 terrain 之后的方块自顶向下扫一遍算出，等价于 `Heightmap.primeHeightmaps`。
    pub heights: [Heightmaps; 9],
}

/// 一次 `features` 阶段的结果。
pub struct FeatureResult {
    /// 9 个区块跑完 `features` 之后的方块（中心 + 8 个邻居，含邻居被写的部分）。
    pub chunks: [Vec<Vec<u32>>; 9],
    /// 每个邻居相对中心的「写增量」：只记真正被 `setBlock` 改过的格子。
    /// 中心不在这里，中心的最终方块直接交回 Director。
    pub deltas: [Vec<(BlockPos, u32)>; 9],
    /// 该中心区块参与对拍时是否遇到未移植的 feature / modifier / predicate。
    /// 见第 5 节：为 true 时 Java 侧把这块标成 `skipped`，不参与逐方块比较。
    pub unsupported: bool,
    /// 本区块用到的 feature 类型 / modifier 类型，给对拍报告和覆盖率统计。
    pub used: FeatureUsage,
}
```

入口（`mc-bamboo-worldgen::feature`）：

```rust
impl Generator<'_> {
    /// `ChunkGenerator.applyBiomeDecoration`，纯函数。
    ///
    /// 输入：中心坐标 + 9 个区块到 CARVERS 阶段的结果、9 个区块的 BIOMES 群系格。
    /// 输出：9 个区块改成 FEATURES 阶段之后的样子 + 给 8 个邻居的写增量 +
    ///       是否含未移植的地物。
    pub fn features(&self, input: FeatureChunks) -> Result<FeatureResult, String>;
}
```

内部流程照第 1 节：

1. `biomes = 9 个区块 quart 调色板的并集`（按 `ObjectArraySet` 的插入序：
   `ChunkPos.rangeClosed(center, 1)` 的顺序、每块 section 从下到上、每格按
   `PalettedContainerRO.getAll` 的顺序）。Rust 侧按
   `for dz in -1..=1 { for dx in -1..=1 { ... } }` 遍历，用一个 `Vec` + `HashSet`
   保持首次出现顺序（`ObjectArraySet` 就是插入序）。
   **注意**：`applyBiomeDecoration` 里这个集合只是用来决定「哪些 step 的哪些全局 index
   要跑」，它的迭代顺序会影响 `indices` 的构造顺序，但随后 `Arrays.sort` 会排序，
   所以只要并集内容正确，顺序不影响结果。
2. `decorationSeed = WorldgenRandom(Xoroshiro).set_decoration_seed(seed, centerX*16, centerZ*16)`。
3. 对 `step in 0..11`：收集 `indices`（并集里每个群系 `features[step]` 中每个
   placed feature 的全局 index），排序、去重，逐个
   `set_feature_seed(decorationSeed, index, step)` 后跑 `FeaturePlacer`。
4. `FeaturePlacer` 的栈 DFS 里，`PlacementContext` 通过 `FeatureChunks` 提供：
   `get_block(x,y,z)` / `set_block(x,y,z,state)` / `get_height(type,x,z)` /
   `get_biome(x,y,z)`（逐点 `BiomeManager`） / `get_noise_biome(quart)`（读 quart 格）。
   写超出 3×3 直接丢弃（等价于 `ensureCanWrite`），写进邻居时同时记进 `deltas`。
5. 未移植的类型：不静默跳过。命中 `unsupported` 时置位并（可选）提前返回，
   让对拍明确知道这块不能比。

### 3.2 和 Director 多阶段生成的衔接

`docs/world-model-design.md` 5.3.1 已经写好的方案：

> `features` 写半径 1 的部分随结果带回「要写进邻居的改动」，世界线程按 `TaskId` 顺序合进去，
> 写范围排他调度已经保证不会有两个任务同时写同一个邻居。

对应到这里的接口：

* **邻居只读视图**：Director 把 8 个邻居的半成品存成
  `Arc<ProtoChunk>`（5.3.1 的「不可变快照」）。本函数的 `FeatureChunks`
  就是把这个视图具体化成 9 份方块数组 + 群系格 + heightmap；接进游戏时由
  `ChunkGenerator::generate` 的 `neighbors` 参数填进来，不需要复制（可以先按
  `Arc` 借用，写时复制）。
* **写邻居的增量**：`FeatureResult.deltas` 就是 5.3.1 说的「要写进邻居的改动」。
  中心区块的改动在 `chunks[4]` 里，世界线程直接换进中心 `ProtoChunk`；8 个邻居的
  `deltas` 按 `TaskId` 顺序合进各自的 `ProtoChunk`。
* **写范围排他调度**：`FeatureChunks` 只读邻居，`deltas` 是纯数据，函数本身不改任何
  共享状态，所以可以进计算池；同一时刻不会有第二个 `features` 任务写同一组邻居
  （依赖图保证），合并顺序由 `TaskId` 决定，结果和线程调度无关。
* **纯函数性质**：函数只依赖 `&self`（编译好的密度函数/群系源）和 `input`，
  不碰世界状态、不取系统随机数。`WorldgenRandom` 是从世界种子和区块坐标派生的，
  所以同一输入必然同一输出。
* **只读输入不含结构**：本轮 `StructureManager.shouldGenerateStructures() == false`，
  结构放置留到结构里程碑；`FeatureChunks` 里没有 `structure_starts` 字段，
  以后要加时扩展输入结构即可，不用改函数签名之外的东西。

### 3.3 目录组织

```
crates/mc-bamboo-worldgen/src/feature/
    mod.rs          // FeatureChunks / FeatureResult / Generator::features / 主循环
    registry.rs     // feature / placed_feature / biome_features / features_per_step 解析
    state.rs        // BlockState JSON → 状态 ID；BlockPredicate / RuleTest 求值
    placement.rs    // PlacementModifier 的枚举与栈 DFS（FeaturePlacer）
    provider.rs     // IntProvider / HeightProvider / VerticalAnchor / BlockStateProvider
    random.rs       // 复用 crate::random::WorldgenRandom（不新增）
    ores.rs         // OreFeature / ScatteredOreFeature
    simple.rs       // SimpleBlockFeature / DiskFeature / ...
    trees.rs        // TreeFeature / trunk & foliage placer（后续）
    predicates.rs   // BlockPredicate（tag_match / all_of / height_match / ...）
```

`lib.rs` 加 `pub mod feature;`，`Generator` 加 `features: feature::Registry`
（按 `Worldgen` 建一次，和 `MaterialRules` / `Carvers` 一样）。

## 4. 对拍：`chunkgen --through features`

### 4.1 Java 侧：够用的 `WorldGenLevel`

`applyBiomeDecoration` 的签名是
`(WorldGenLevel, ChunkAccess, StructureManager)`，它只通过 `WorldGenLevel` 接口访问世界。
`ChunkStatusTasks.generateFeatures` 传的是官方 `WorldGenRegion`，但**对拍不需要真的
`WorldGenRegion`**。推荐方案（按风险从低到高）：

**方案 A（推荐）：`WorldGenLevel` 的动态代理 + 3×3 `ProtoChunk` 数组。**

在 `ChunkgenOracle.java` 里写一个 `InvocationHandler`：

* 造中心 + 8 个邻居的 `ProtoChunk`，每个都跑现有的 `createBiomes` /
  `doFill` / `buildSurface` / `generateCarvers` 三条反射调用（把现有
  `run()` 里的单块生成抽成 `generateTerrain(seed, x, z)`），
  `setPersistedStatus(ChunkStatus.TERRAIN)`。
* 对每个 chunk 调 `Heightmap.primeHeightmaps(chunk, ChunkStatus.FINAL_HEIGHTMAPS)`
  （公开静态方法），让 `ChunkAccess.getHeight` 直接可用。
* `InvocationHandler` **只实现地物真正用到的方法**；每个方法的语义都照
  `WorldGenRegion` 的字节码核对（下表给出对应行号，指 `javap -c -p
  net.minecraft.server.level.WorldGenRegion` 的输出）。**没实现的方法一律抛
  `UnsupportedOperationException`，不返回默认值**：没覆盖到的路径要立刻暴露，
  不能悄悄算错。
* 按 `BlockPos` 算出 `(chunk index, local x/y/z)` 转发给对应的 `ProtoChunk`：

| 代理方法 | 照搬的 `WorldGenRegion` 逻辑 |
|---|---|
| `getBlockState(BlockPos)` | 331：先 `warnIfReadOutsideWriteZone`（超 3×3 只记日志），再 `getChunk(sectionX, sectionZ)` → `ChunkAccess.getBlockState` |
| `getFluidState(BlockPos)` | 353：同上，转发 `getFluidState` |
| `getChunk(int,int)` / `getChunk(int,int,status,boolean)` | 169 / 178：按 chessboard 距离查 9 宫格，超出返回 null；`ChunkStatus.isOrBefore` 判定照 71–80 |
| `getChunk(BlockPos)` | 用 `SectionPos.blockToSectionCoord` 换算后转发 |
| `hasChunk(int,int)` | 311：`distance < directDependencies.size()`（本轮 9 宫格内恒 true） |
| `ensureCanWrite(BlockPos)` | 635：`isWithinWriteZone` = `|cx-centerX| <= 1 && |cz-centerZ| <= 1`（612 行），且不在升级中；false 就不写 |
| `setBlock(BlockPos, BlockState, int, int)` | 691：先 `ensureCanWrite`；`ChunkAccess.setBlockState(pos, state, flags)`；`state.hasBlockEntity()` 时按 44–101 登记/移除方块实体（PROTOCHUNK 走 104–167 的 DUMMY NBT 分支）；`flags & 16 == 0` 时按 189–215 调 `getPostProcessPos` + `markPosForPostProcessing`。**`flags` 不能丢**：`Feature.safeSetBlock` 传 2，`setBlockAndUpdate` 传 3 |
| `markPosForPostProcessing(BlockPos)` | 794：转发给对应 `ChunkAccess` |
| `getHeight(Heightmap.Types,int,int)` | 946：`getChunk(sectionX, sectionZ).getHeight(type, x&15, z&15) + 1`；高度图随 `ChunkAccess.setBlockState` 里 `Heightmap.update` 自动增量更新 |
| `getBiome(BlockPos)` | `LevelReader.getBiome`（24–30）：`getBiomeManager().getBiome(pos)`，即 8 角 fiddled distance；**不是** quart 格直读 |
| `getNoiseBiome(int,int,int)` | `LevelReader.getNoiseBiome`（74–98）：先 `getChunk(...).getNoiseBiome(quart)`，取不到才 `getUncachedNoiseBiome` |
| `getUncachedNoiseBiome(int,int,int)` | 391：转发给 `BiomeResolver`（`createUncachedResolver(state)`） |
| `getBiomeManager()` | 385：返回 `new BiomeManager(resolver, BiomeManager.obfuscateSeed(seed))`（构造 150–165 行） |
| `getSeed()` / `getRandom()` | 915 / 940：`getSeed()` 返回世界种子；`getRandom()` 用 `RandomState.getOrCreateRandomFactory("minecraft:worldgen_region").at(中心区块 origin)`（构造 106–130 行），地物放置本身不用它但要给对 |
| `getMinY()` / `getHeight()` / `getMinSectionY()` / `getSeaLevel()` | 1029 / 1036 / 对应 `LevelHeightAccessor` 默认 / 933：从 `generatorSettings` 和 `LevelHeightAccessor` 取 |
| `isOutsideBuildHeight(int)` / `isStateAtPosition` / `isFluidAtPosition` | 996 / 1005：用 `getBlockState` / `getFluidState` 转发 |
| `getBlockEntity(BlockPos)` | 428：先 `getChunk(...).getBlockEntity`，为 null 时按 18–113 处理 DUMMY NBT |
| `getBlockTicks()` / `getFluidTicks()` | 921 / 927：`WorldGenTickAccess`，本轮只比方块，转发给 ProtoChunk 的 tick 容器 |
| `setCurrentlyGenerating(Supplier<String>)` | 162：存起来，仅 `warnIfReadOutsideWriteZone` 打日志用 |
| `registryAccess()` / `enabledFeatures()` | 849 / 856：返回加载好的 `RegistryAccess` / `FeatureFlagSet` |
| `getLevel()` / `isClientSide()` | 843 / 838：`getLevel()` 返回代理自身（对拍里没有真 `ServerLevel`），`isClientSide()` 返回 false |
| `findBlocksIn(BlockPos,BlockPos)` | 554：对覆盖的每个 section 调 `warnIfReadOutsideWriteZone`，再走 `WorldGenLevel.findBlocksIn` 默认实现 |
| `destroyBlock` / `removeBlock` | 408 / 822：转发到 `setBlock(AIR, 3)` / `setBlockAndUpdate` |
| `getHeightmapPos(type,pos)` | `LevelReader.getHeightmapPos`（122–134）：x/z 不变，y 换成 `getHeight(type,x,z)`；`PlaceOnGroundDecorator`（树）会调 |
| `getSectionIndex(y)` / `getSectionIndexFromSectionY(y)` / `getSectionYFromSectionIndex(i)` | `LevelHeightAccessor` 默认实现（89–108）：`blockToSectionCoord(y) - minSectionY`；`BulkSectionAccess`（矿石）会调 |
| `getBlockTicks()` / `getFluidTicks()` | 921 / 927 + 构造 5–35：`WorldGenTickAccess`，`containerGetter = pos -> chunkAt(pos).getBlockTicks()/getFluidTicks()` |
| `scheduleTick(pos, target, delay[, priority])` / `nextSubTickCount()` | `ScheduledTickAccess` 默认实现（9–32）+ `LevelAccessor.createTick`（5–36）：转发到上一条的 `WorldGenTickAccess.schedule`。树叶 `updateShape` 会调；本轮只比方块，时间从 0 递增即可 |
| `isOutsideBuildHeight(y)` / `isEmptyBlock(pos)` / `getMaxY()` / `getMinSectionY()` / `getMaxSectionY()` / `getSectionsCount()` | `LevelHeightAccessor` / `LevelReader` 默认实现换成代理后要自己返回，否则接口默认方法会走 `getHeight()` 之外的路径 |
| `getNearestPlayer` / `addFreshEntity` / `playSound` / `addParticle` / `levelEvent` / `gameEvent` / `getEntities` / `players` / `getWorldBorder` / `getLightEngine` / `getSkyDarken` / `getCurrentDifficultyAt` / `getServer` / `getChunkSource` / `getLevelData` / `dimensionType` / `environmentAttributes` | **本轮不实现**：一旦被调用就抛 `UnsupportedOperationException`，立刻暴露新路径；确实需要时再逐个按 `WorldGenRegion` 对应字节码补 |

上表多出来的几行是第一次跑完整 11 步时实测出来的：矿石走 `BulkSectionAccess`，
树走 `PlaceOnGroundDecorator` 和 `LeavesBlock.updateShape`，分别需要 section index、
heightmapPos 和计划刻。**代理没有返回默认值的分支**，未覆盖方法都会抛异常。

* `getHeight` 要照 `WorldGenRegion` 加 1；`setBlock` 要照 `ensureCanWrite` 丢掉 3×3 之外的写。
* 「空实现」只限于本来就无副作用的 `setCurrentlyGenerating` 和 `isClientSide`；
  其余未覆盖方法一律抛异常。
* `StructureManager` 用 `new StructureManager(proxy, new WorldOptions(..., generateStructures=false),
  ...)` 或 `Unsafe` 造的子类覆写 `shouldGenerateStructures() -> false`，让结构分支整段跳过。
* 调 `generator.applyBiomeDecoration(proxy, centerChunk, structureManager)`。
* **不启动服务端、不读存档、不接受 EULA**，和现有 `ChunkgenOracle` 一致。

优点：完全绕开 `ServerLevel` / `StaticCache2D` / `GenerationChunkHolder` / `ChunkStep`
这些重对象的伪造，也不用 `Unsafe`；`applyBiomeDecoration` 看到的就是一个正常的
`WorldGenLevel`。缺点：代理要覆盖的方法列表要靠实测补齐，第一次跑会有几轮
`UnsupportedOperationException` 迭代。

**方案 B（备选）：真的 `WorldGenRegion` + 假 `ServerLevel`。**

照 `BlockOracle.java` / `EntityOracle.java` 的 `FakeLevel`（`Unsafe.allocateInstance`
跳过构造器）造 `ServerLevel`，反射填 `registryAccess`、`dimensionTypeRegistration`、
`palettedContainerFactory`、`chunkSource`（假的 `ServerChunkCache` 提供 `RandomState`）、
`server`、`players`、`environmentAttributes`、`thread` 和 seed；再用
`StaticCache2D.create(centerX-1, centerZ-1, 3, ...)` 和 9 个
`GenerationChunkHolder`（Unsafe + 反射塞 `futures`）造 `WorldGenRegion`。
更贴近官方，但伪造面大得多、也更脆。**只在方案 A 遇到无法绕过的
`instanceof ServerLevel` 分支时退回方案 B。**

### 4.2 输出格式

现有 `ChunkgenOracle` 每个区块输出一行 `noise` / `surface` / `carvers`。`features` 阶段
要输出 **9 个区块**的结果，因为地物会写邻居：

```
{"type":"features","seed":...,"x":...,"z":...,
 "chunks":[{"dx":-1,"dz":-1,"palette":[...],"blocks":[...]}, ... 9 条 ...],
 "skipped":false,
 "unsupported":["minecraft:tree","minecraft:lake"],   // Java 侧可选，便于诊断
 "checks":...}
```

* `blocks` 仍是 `16×height×16` 的下标数组，顺序 y、x、z，和现有 `writeBlocks` 一样。
* **`skipped`**：Java 侧按 `--features-allow` 白名单（或 Rust 侧报告的 `unsupported`）
  决定这个中心区块是否含未移植的地物。含的区块仍输出（便于以后逐步开放），
  但 `skipped: true`，Rust 侧跳过逐方块比较，只在报告里计数。
  判定依据：中心 3×3 里 9 个区块的群系并集涉及的所有 step 的 placed feature，
  其 feature 类型 / modifier 类型是否都在「已移植」集合里。
* 白名单放在 Rust 侧一份常量（`chunkgen.rs` 的 `SUPPORTED_*`），Java 侧通过命令行
  参数 `--supported-features <逗号分隔>` / `--supported-modifiers` 传进去，
  避免两边各维护一份会漂移的清单。

**已落地的过渡开关**（`--steps 6`）：Java 侧不能改 `featuresPerStep`（它是
`ImmutableList`，且 `applyBiomeDecoration` 会按群系列表里留下的 index 直接
`features.get(index)`，改短会越界）。实际做法是用反射把每个群系
`BiomeGenerationSettings.features()` 里**没选中的 step 换成空 `HolderSet`**，并重建
`featureSet` supplier，这样 `indices` 集合自然为空，不会调用 `placeWithBiomeCheck`；
`featuresPerStep`、全局 index、`setFeatureSeed(decorationSeed, index, step)` 全部保持
原样，留下的 step 与官方逐位一致。Rust 侧 `chunkgen.rs` 转发 `--steps`，并核对
header / features 行里的 `steps` 掩码。
* 现有 `header` 加 `through: 4` 表示 features；`Line` 枚举加 `Features` 变体；
  `Options::through` 加 `"features" => Stage::Features`（或直接扩展
  `chunk::Stage`，见下）。

### 4.3 Rust 侧

`chunk::Stage` 加 `Features`（`terrain` 三步之外的新枚举值，或者单独一个
`FeatureStage` 枚举——**倾向后者**：`terrain()` 只到 Carvers，`features()` 是另一个入口，
不要硬塞进 `Stage` 让 `terrain()` 的语义变模糊）。

`chunkgen.rs::Report`：

* 收到 `Features` 行时，按 `(seed,x,z)` 建 9 个区块的 `FeatureChunks`
  （调 `generator.terrain(x+dx, z+dz, Stage::Carvers)` 九次 + `generator.biomes` 九次 +
  算 heightmap），再调 `generator.features(input)`；
* `skipped == true` 或 `FeatureResult.unsupported == true` → 计数跳过，不比；
* 否则按 9 个 chunk 逐个和 `chunks[4]` 之后的方块比较，和现有
  `compare_blocks` 一样按 palette 名字查回状态 ID。
  **调色板解析要升级**：地物会放带属性的状态（`[level=1]`、`[facing=north]` 等），
  不能再用 `chunkgen.rs::state_of` 那个「忽略属性」的简易版；用
  `feature::state::parse` 的逆映射（状态 ID → `BlockState.toString()` 的等价形式），
  或者在 Java 侧直接输出 `state.getBlock().getDescriptionId()` + 属性 map，
  Rust 侧按 `PropertyDef` 精确还原 ID。
* `finish()` 打印新增的「features 比较了多少块、跳过多少块、跳过原因分布」。

### 4.4 种子/区块规模和耗时

`chunkgen` 现有 `--seeds 6 --chunks 36 --salt 1` 是 216 个中心区块；features 阶段每个
中心要生成 9 个 terrain 区块（~17 ms/块，约 150 ms/中心），加上地物本身，
一轮几分钟量级。先按 `--seeds 1 --chunks 4` 调通，再逐步放回全量。

## 5. 移植顺序

原则：**先拿到「某些区块逐方块 0 不一致」**，再扩展覆盖面。不追求一次全做完。

### 第 0 阶段：数据 + 框架（本轮先做）

1. `Extractor.java` 加四个键；重跑 `extract` / `generate` 确认 `worldgen.json` 无 diff
   （新增键稳定）、旧 `chunkgen --through carvers` 仍 0 不一致。
2. `ChunkgenOracle.java` 加 3×3 假世界 + `--through features` + `skipped` 标记；
   `chunkgen.rs` 加对应的 `Line::Features` / `Options` / `Report` 处理。
   **此时全部区块 `skipped: true`**（一个地物都没移植），验证框架的
   `terrain` 九宫格、`features_per_step` 索引、随机种子、输出格式都对得上——
   可以先加一个「只跑 `FeaturePlacer` 的修饰器链、不调 `Feature.place`」的诊断模式，
   对拍「每个 step 每个 index 的候选 `BlockPos` 序列」，这样能独立于具体 feature
   验证 `setFeatureSeed` / `biome` filter / 高度图 / 全局排序。
3. Rust 侧 `feature/` 骨架：`Registry` / `Steps` / `FeatureChunks` / `FeatureResult` /
   主循环 / `Worldgen` 新字段检查 / 单元测试。

### 第 1 阶段：只跑 step 6 的矿石（最快拿到 0 不一致）

按频率，`UNDERGROUND_ORES` 占全部地物条目的 60%，而且只需：

* 修饰器：`count`、`in_square`、`height_range`、`biome`、`rarity_filter`、`heightmap`、
  `surface_relative_threshold_filter`、`offset`（8 种；`block_predicate_filter` 由 disk 用到，
  可以先把 disk 也标为不支持）。
* 解析器：`IntProvider`（constant / uniform / biased_to_bottom / clamped / weighted_list）、
  `HeightProvider`（uniform / trapezoid / very_biased_to_bottom / biased_to_bottom / constant）、
  `VerticalAnchor`（absolute / above_bottom / below_top）。
* 谓词：`tag_match`、`all_of`、`any_of`、`not`、`height_match`、`always_true`、
  `block_match`、`random_block_match`（后两种数量少，可先不支持）。
* Feature：`OreFeature`（+ 复用同一 `doPlace` 的 `ScatteredOreFeature`）。
* 方块状态：矿石全是默认状态，草/花那种带属性的状态这一阶段用不到，
  `parse` 可以先只支持 `{"Name": "..."}` / `"minecraft:coal_ore"` 字符串形式。

**跳过策略**：第 1 阶段把「已移植集合」设成
`features = {minecraft:ore, minecraft:scattered_ore}`、
`modifiers = {count, in_square, height_range, biome, rarity_filter, heightmap, offset}`。
Java 侧按白名单判定：中心 3×3 涉及的**所有** step 的 placed feature 里，
只要有任一未移植类型就把整块标 `skipped: true`。这样：
* 多数群系都会因为 step 1/2/3/9/10 的湖、晶洞、怪物房、花草、积雪被标 skipped；
* 可以先加一个**对拍专用的「只跑 step 6」开关**（Java 侧 `--steps 6`，Rust 侧同样只跑
  step 6），让所有 67 个群系的区块都能参与比较，最快拿到大面积 0 不一致；
* 确认 step 6 全绿之后，再逐个放开其它 step。

**注意**：即使只跑 step 6，也要照官方顺序跑完整的
`setFeatureSeed(decorationSeed, index, step)`（index 是全局 index，step 固定 6），
不能自己重编号；跳过其它 step 只是不调 `placeWithBiomeCheck`，随机数源是每个
`(index, step)` 重新 `setSeed` 的，所以跳过不影响 step 6 的随机序列。

### 第 2 阶段：低垂果实

1. `DiskFeature`（step 6 的 `disk_sand/clay/gravel`，165 处）+ `UnderwaterMagmaFeature`：
   需要 `heightmap` / `block_predicate_filter` / `surface_relative_threshold_filter`。
   做完 step 6 可以整体放开。
2. `SimpleBlockFeature` + `ProjectedRandomPatchySquare` + `random_selector` /
   `simple_random_selector` / `weighted_random_selector`（step 7/9 的一部分）：
   需要 `BlockStateProvider`（simple / weighted / randomized_int / noise / dual_noise …）
   和 `BlockPredicate` 的 matcher 类。
3. `SpringFeature`（step 8，114 处），`SnowAndFreezeFeature`（step 10，56 处，
   复用 `biome_temperature.rs` 的 `cold_enough_to_snow`）。
4. `GeodeFeature`（step 2，56 处）：配置大但逻辑自洽，读写都在 3×3 内。

### 第 3 阶段：树和其余

`TreeFeature` 是 45 个 feature 定义、涉及 trunk/foliage/root placer 和 decorator，
**单独一个里程碑**；`LakeFeature`（step 1，110 处）、`MonsterRoomFeature`（step 3，
112 处）、`IcebergFeature`、`End*` / 下界类出现频率低，可以一直标 skipped，
等主世界常见群系全绿之后再补。

### 明确暂不做的

* 结构放置（`StructureManager` / `structure_starts`）：假世界关掉，和本轮地物正交。
* 下界 / 末地：terrain 阶段就还没做，地物也顺延。
* `Blender.generateBorderTicks`：本轮只比方块，不比计划刻。
* 方块实体 / POI / 后期处理位置表：Java 侧 `ProtoChunk` 会记，但 Rust 侧
  `FeatureResult` 暂时只带方块；接进游戏时补。
* 高度图随方块更新：本轮 `getHeight` 用 terrain 阶段的 primed heightmap，
  并照官方 `Heightmap.update` 在每次 `setBlock` 后增量更新（否则后续 feature 的
  `heightmap` 修饰器会读到过期值）。这一条**不能省**，要在第 0 阶段的
  `FeatureChunks::set_block` 里就实现。

## 5.1 当前进度（本轮落地）

已提交的实现：

* 数据导出：`feature` / `placed_feature` / `biome_features` / `features_per_step`
  四个顶层键，`worldgen.json` 重跑无 diff（必须用 `--no-default-features extract`，
  否则 `preserve_order` 会改键顺序）。
* Java 对拍：`--through features` 用 `WorldGenLevel` 动态代理 + 9 个跑到 CARVERS 的
  `ProtoChunk`，输出 9 宫格。`--steps 6` 和 `--ore-only` 是过渡期诊断开关：
  前者把没选中的 step 在每群系的 `BiomeGenerationSettings.features()` 里换成空
  `HolderSet`，后者按 `Feature.DIRECT_CODEC` 的类型 ID 只保留 `minecraft:ore`；
  两者都不动 `featuresPerStep`，所以全局 index 和 `setFeatureSeed` 参数与官方一致。
* Rust：`feature/` 模块（注册表、provider、state、rule_test、world、placement、
  ores、run），`Generator::features()` 入口，`chunkgen.rs` 逐方块比 9 宫格。
  `WorldgenRandom` 补齐了 `BitRandomSource` 默认实现的 `nextInt` / `nextFloat` /
  `nextDouble`：它继承 `LegacyRandomSource`，这些方法走 `next(31)` / `next(24)` /
  `next(26)+next(27)`，不是内层 Xoroshiro 自己的 Lemire / 53 位路径。

**一个必须照抄的顺序细节**：`FeaturePlacer` 的栈 DFS 里
`PlacementModifier.modify` 和 `Feature.place` 是**交错执行**的。若先把全部候选
位置收集完再逐个放置，`Feature.place` 消费的随机数会打乱后续候选位置的
`in_square` / `height_range` 求值，得到完全不同的结果（实测会差几千个方块）。
`placement::place_candidates` 用回调在 DFS 内部立即放置。

### 对拍结果

| 命令 | 结果 |
|---|---|
| `chunkgen java --seeds 1 --chunks 1 --through features --steps 6 --ore-only` | **0 不一致**（884736 个方块） |
| `chunkgen java --seeds 2 --chunks 4 --through features --steps 6 --ore-only --salt 1` | **0 不一致**（7077888 个方块，8 个中心） |
| `chunkgen java --seeds 3 --chunks 8 --through features --steps 6 --ore-only --salt 1/7` | 24 个中心里 23 个 0 不一致；固定区块 `(100,100)` 有约 200 个格子的差异 |
| `chunkgen java --seeds 2 --chunks 6 --through carvers --salt 1` | 0 不一致（旧阶段回归） |
| `worldgen java` | 0 不一致 |
| `cargo test -p mc-bamboo-worldgen` | 93 个测试全过 |
| `cargo clippy -p mc-bamboo-worldgen -p mc-bamboo-vanilla --all-targets -- -D warnings` | 干净 |

**剩余差异（已定位到 `ore_clay` 的候选位置）**：`--ore-only --steps 6` 在第三个
种子的 chunk `(100,100)` 时，`minecraft:ore_clay` 的 46 个候选位置在 Java 与
Bamboo 之间**逐个不同**（Java 首个 `(1608,-53,1615)`，Bamboo 首个
`(1606,178,1603)`），因此 `BiomeFilter` 放行的数量也不同（Java 2 个、
Bamboo 1 个），各自写下的 clay 团位置不同，产生约 200 格差异。

诊断证据：
* `ore_clay` 只属于 `minecraft:lush_caves`（注册表 ID 31），只有该群系出现在
  3×3 群系并集里才会被收集，所以 `--seeds 1/2` 全绿、`--seeds 3` 才复现。
* 输入 terrain 里 clay 数为 0，确认差异来自 features 阶段。
* `decoration_seed(seed=1, x=1600, z=1600) = 5899775146600525569`，
  Java 与 Rust 一致；`setFeatureSeed(deco, 27, 6)` 的参数也一致。
* 两边的 `indices` 里 `ore_clay` 都是 step 6 的全局 index 27，
  而 index 0..25 的所有 ore 都逐方块一致。
* 跳过 Rust 的 `ore_clay` 后差异方向反转（变成官方有 clay、Bamboo 没有），
  说明 Java 确实跑了 `ore_clay`，不是 `featureSet` 过滤差异。

**已确认**：Java 与 Rust 的 `indices` 集合不同。`--ore-only` 把非 ore 的
HolderSet 换空，Java 的 step 6 `indices` 只含 26 个 ore；Rust 仍从**未过滤**的
`biome_features` 收集，多出 `underwater_magma`(26)、`disk_sand`(30)、
`disk_clay`(31)、`disk_gravel`(32)。这些在 Rust 里因 `kind != ore` 或
`Modifier::Unsupported` 被 `continue`，不调 `Feature.place`；而且
`setFeatureSeed` 按 index 重新播种，理论上不该污染 index 27。但
`FeaturePlacer` 的栈 DFS 里 `in_square` / `height_range` 会在
`Feature.place` **之前**消费随机数，若 Rust 多跑的 index 在某些路径上
没有完全跳过，就会打乱后续序列。

已让 Rust 的 `indices` 在 `--ore-only` 下按 `allowed_kinds` 过滤，与 Java 的
`keep` 集合对齐（两边现在都只跑 26 个 ore 的 index）；**差异仍存在**，符合
`setFeatureSeed` 按 index 重置的预期，但这一步让两侧语义一致，保留。
`OreProbe.java` 确认 `ore_clay` 的链是
`CountPlacement[count=46] → InSquare → HeightRange[0 above bottom-256 absolute]
→ BiomeFilter`，与导出的 JSON 一致。

补充排除：`OreProbe2.java` 逐位验证了 `WorldgenRandom.nextInt(16)` 与
`nextInt(321)` 序列（已写进 `random.rs` 的 `bit_source_tests`）。
`RandomState.createClimateSampler(EMPTY_UNCACHED)` 用
`samplersWithContext` + `NoiseRouter.createClimateSampler(DensitySamplerSet)`，
与 Rust 的 `settings::Samplers` 数学上等价，但 `BiomeManager::biome` +
`Context::uncached()` 这条组合从未单独对拍过（terrain 走 `biome_cached` +
`Context::new` + `ClimateCache`）。

**下一步**：在 Java 代理里给 `FeaturePlacer` 的 `in_square` / `height_range`
加探针（或直接给 `WorldgenRandom` 包一层记数），逐项对比 `ore_clay` 的
46 个候选位置在两边各自消费的 `nextInt(16)` / `nextInt(321)` 序列；
同时确认 Java 的 `setFeatureSeed` 实际用的 index 值。既然种子、
所有取数方法、`count` 与配置都一致，分歧只能在这条链的调用顺序上。

**已排除**：`Math.sin/cos` 的 Java/Rust 位模式（逐值比过）、`Mth.sin` 65536
项查表（用 `MthProbe.java` 逐值比过）、`WorldgenRandom` 的
`nextInt(384)` / `nextFloat` / `nextDouble` / `nextInt(3)` 序列和
`setDecorationSeed` 值（用 `RandProbe2.java` / `OreProbe.java` 逐位比过）、
`OreFeature.place` 的 float/double 运算顺序（字节码 `fmul`/`dmul` 逐条核过）、
`Context` 缓存（已换成 `Context::uncached()`）、栈 DFS 的交错顺序、
`getHeight` 序列、`OreFeature` 全部配置的解析。

**下一步**：在 Java 代理的 `getBiome` 里打印 `ore_clay` 46 个候选位置的群系，
和 Rust 的 `biome_at` 逐点对比。字节码确认
`ServerLevel.uncachedBiomeResolver()` =
`BiomeSource.createUncachedResolver(RandomState)`，它用
`RandomState.createClimateSampler(SamplerContext.EMPTY_UNCACHED)` 重新构造
`Climate.Sampler`；Rust 侧复用的却是 `settings::Samplers`（直接编译
`NoiseRouter` 六个字段）。terrain 阶段的对拍走的是 `biome_cached` +
`Context::new`（带缓存）+ `ClimateCache`，**`BiomeManager::biome` 的
`Context::uncached()` 组合从未单独对拍过**。优先核对这两条 climate sampler
构造路径是否逐位一致，以及 `interpolated` 在 uncached 单点路径下的行为
（`docs/worldgen.md` 提到它在 1×1×1 上不是纯函数）。
**这一处差异不影响其它 23/24 区块，也不影响已确认的 0 不一致结论，
但不能算「step 6 全绿」。**

### 还没做

* `DiskFeature` / `UnderwaterMagmaFeature` / `surface_relative_threshold_filter` /
  `block_predicate_filter`：step 6 里非 ore 的 5 个 placed feature 目前标
  `unsupported`，完整 11 步对拍会跳过这些区块。
* 其它 10 个 step（湖、晶洞、怪物房、花草、积雪等）全部标 `unsupported`。
* `skipped` 白名单目前只在 Rust 侧用 `unsupported` 标记；Java 侧还没按
  「已移植集合」输出 `skipped`，`--ore-only` 是手工过滤。
* 接进 Director（5.3.1 的邻居快照 + 增量合并）还没做。
* 高度图的增量更新：矿石走 `BulkSectionAccess` 直接写 `LevelChunkSection`，
  **绕过** `ChunkAccess.setBlockState`，所以 step 6 期间高度图是 primed 后固定的；
  以后接树/湖时要照 `Heightmap.update` 补上。

## 6. 工作量估计

按「每做完一小步就提交」的节奏，单位是人时（单人、含对拍调试，不含等答复）：

| 阶段 | 内容 | 估计 | 产出 |
|---|---|---:|---|
| 0a | `Extractor.java` 四个键 + 重跑无 diff + `Worldgen` 解析骨架 | 2–3 h | 数据落地，`worldgen.json` 稳定 |
| 0b | Java 3×3 假 `WorldGenLevel` + `chunkgen --through features` 框架 + `skipped` | 3–5 h | 能跑通空地物对拍，输出 9 宫格 |
| 0c | Rust 主循环（`FeatureSorter` 顺序、随机种子、`FeaturePlacer` 栈 DFS）+ 候选位置诊断对拍 | 3–4 h | 修饰器链逐位一致 |
| 1 | `IntProvider` / `HeightProvider` / `VerticalAnchor` / `BlockPredicate` / `RuleTest` + `OreFeature` | 4–6 h | **step 6 逐方块 0 不一致** |
| 2 | `DiskFeature`、`SimpleBlockFeature`、selector 类、`BlockStateProvider`、`SpringFeature`、`freeze_top_layer` | 6–10 h | step 6/7/8/10 大部分放开 |
| 3 | `TreeFeature` 全家、`LakeFeature`、`GeodeFeature`、`MonsterRoomFeature`、其余低频 | 15–25 h | 常见群系全绿 |
| — | 接进 Director（5.3.1 的邻居快照 + 增量合并），不在本轮 | 另行估 | — |

**本轮（约 5 小时）可交付**：阶段 0a + 0b + 0c 的大部分，以及阶段 1 的
`OreFeature` 骨架；如果时间允许，拿到「`--steps 6` 下若干区块 0 不一致」。
完整做完 features（到常见群系全绿）按上表是 30–50 人时，明显超出一轮。

## 7. 风险与疑点

1. **`features_per_step` 的导出方式**：在 `Extractor.java` 里造一个
   `NoiseBasedChunkGenerator` 需要 `RegistryAccess` 和群系预设，理论上可行
   （`ChunkgenOracle.java` 已经这么造过），但 `featuresPerStep` 是私有 `Supplier`，
   要反射。若反射造 generator 有困难，退路是在 `ChunkgenOracle.java` 里导出同一份表
   （它本来就有 generator），但它属于对拍工具而不是数据生成器，两边数据来源会分叉。
   **优先在 Extractor 里做**，做不通再交回主代理。
2. **`FeatureSorter` 的群系列表顺序**：`List.copyOf(possibleBiomes())` 来自
   `ImmutableSet`，迭代序由注册表的插入序决定。导出的 `features_per_step`
   会把它固化下来，Rust 侧不需要关心；但换 jar 版本重跑时要确认这份表变了没有
   （`worldgen.json` 的 diff 会直接暴露）。
3. **`BiomeManager.getBiome` 的平局 / R 树 `lastResult`**：`BiomeFilter` 每次过滤都会
   走一次逐点群系查询，`ParameterList` 的 R 树带 `lastResult` 当平局候选，
   查询顺序会影响结果。本轮主循环严格照官方 step/index 顺序，应该一致，
   但对拍第一次跑要专门盯这一条（现有 `ClimateCache` 已经处理了「只缓存气候、
   不缓存群系 ID」，地物阶段照样沿用）。
4. **带属性的方块状态**：`chunkgen.rs` 现在把 `BlockState.toString()` 的属性全忽略
   （terrain 三步只有默认状态）。地物会放 `water[level=1]`、`log[axis=y]` 等，
   第 0b 阶段就得把 palette 的解析升级，否则第 1 阶段之后所有对拍都会假绿/假红。
5. **高度图的增量更新**：`Heightmap.update` 在 `setBlockState` 里调，
   只更新 `ChunkAccess.heightmaps` 里已有的类型；`FINAL_HEIGHTMAPS` 四种都要维护。
   伪造的 `WorldGenLevel` 代理如果直接转发给 `ProtoChunk.setBlockState`（官方实现），
   这一条自动成立；Rust 侧要照 `Heightmap.update` 的字节码补一份。
6. **`--steps 6` 对拍专用开关**：这不是官方行为，只是过渡期的诊断手段；
   要在文档和测试里写清楚它不能当最终结果，最终必须跑完整 11 个 step。
