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
  `HolderSet`，后者（`--ore-only`，现已泛化成 `--types ID,ID`）按 `Feature.DIRECT_CODEC`
  的类型 ID 只保留白名单里的类型；两者都不动 `featuresPerStep`，所以全局 index 和
  `setFeatureSeed` 参数与官方一致。
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
| `chunkgen java --seeds 3 --chunks 8 --through features --steps 6 --ore-only --salt 1/7` | **0 不一致**（修复后） |
| `chunkgen java --seeds 6 --chunks 36 --through features --steps 6 --ore-only --salt 1/7` | **0 不一致**（216 中心 / 各 191102976 方块） |
| `chunkgen java --seeds 2 --chunks 6 --through carvers --salt 1` | 0 不一致（旧阶段回归） |
| `worldgen java` | 0 不一致 |
| `cargo test -p mc-bamboo-worldgen` | 115 个测试全过（含组合类 / typed provider 新单测） |
| `cargo clippy -p mc-bamboo-worldgen -p mc-bamboo-vanilla --all-targets -- -D warnings` | 干净 |

**曾有的差异（已解决，见下）**：`--ore-only --steps 6` 在第三个种子的
chunk `(100,100)` 时，`minecraft:ore_clay` 的 46 个候选位置在 Java 与
Bamboo 之间**逐个不同**（Java 首个 `(1608,-53,1615)`，Bamboo 首个
`(1606,178,1603)`），`BiomeFilter` 放行数也不同，产生约 200 格差异。

诊断证据：
* `ore_clay` 只属于 `minecraft:lush_caves`（注册表 ID 31），只有该群系出现在
  3×3 群系并集里才会被收集，所以 `--seeds 1/2` 全绿、`--seeds 3` 才复现。
* 输入 terrain 里 clay 数为 0，确认差异来自 features 阶段。
* `decoration_seed(seed=1, x=1600, z=1600) = 5899775146600525569`，
  Java 与 Rust 一致；`setFeatureSeed(deco, 27, 6)` 的参数也一致。
* 603 行 `(step, index, placed feature id)` 序列两边逐行完全一致；
  但 Java 运行时 `featuresPerStep[6]` 只有 29 项、`ore_clay` 是 index 26，
  而导出的表是 34 项、index 27（这正是根因，见下）。
* 跳过 Rust 的 `ore_clay` 后差异方向反转（变成官方有 clay、Bamboo 没有），
  说明 Java 确实跑了 `ore_clay`，不是 `featureSet` 过滤差异。

### 根因与修复（已解决）

问题**不在 Rust 的地物实现**，而在 `--ore-only` / `--steps` 这两个诊断开关本身。

`ChunkGenerator.featuresPerStep` 是一个 **memoized supplier**，第一次
`Supplier.get()` 时才按注册表此刻的 `BiomeGenerationSettings.features()`
跑 `FeatureSorter` 排全局顺序。`applyBiomeDecoration` 是它的第一个调用者，
而两个诊断过滤在 `run()` 里**先于** `applyBiomeDecoration` 执行，于是
supplier 在**被过滤后的**注册表上求值：`--ore-only` 让 step 6 从导出的
34 项缩成 29 项，`ore_clay` 的全局 index 从 **27 变成 26**。
`setFeatureSeed(decorationSeed, index, step)` 因此用了错的 seed，
46 个候选位置全错位。index 0..25 恰好在两份列表里下标相同，所以之前一直全绿。

修法：在 `ChunkgenOracle.run()` 里、两个诊断过滤**之前**调
`generator.validate()`（`ChunkGenerator.validate` 的字节码就是
`featuresPerStep.get()`），把 supplier 固定在未过滤的 34 项上。
同时让 Rust 的 `indices` 在 `--ore-only` 下按 `allowed_kinds` 过滤，
两边跑的 `(index, step)` 集合也对齐。

**修复后 step 6 的 `--ore-only` 多种子全绿**：

| 命令 | 结果 |
|---|---|
| `--seeds 3 --chunks 8 --salt 1` | 0 不一致（24 中心 / 21233664 方块） |
| `--seeds 3 --chunks 8 --salt 7` | 0 不一致 |
| `--seeds 4 --chunks 12 --salt 1` | 0 不一致（48 中心 / 42467328 方块） |
| `--seeds 6 --chunks 36 --salt 1` | **0 不一致**（216 中心 / 191102976 方块） |
| `--seeds 6 --chunks 36 --salt 7` | **0 不一致** |

诊断过程中逐位排除（结论仍成立，探针都在 `target/vanilla/*.java`，未提交）：
`decoration_seed`、`setFeatureSeed` 参数、`nextInt(3)` / `nextInt(16)` /
`nextInt(321)` / `nextFloat` / `nextDouble` 全部序列、`Mth.sin` 表、
`Math.sin/cos`、`OreFeature.place` 的 `fmul`/`dmul` 顺序、`count=46`、
placement 链、栈 DFS 交错、`Context::uncached()`、603 行
`(step, index, feature id)` 序列（两边逐行完全一致）。
`Mth.sin` 的索引在 26.3 是 **double 乘法** `dmul`
（`ldc2_w 10430.378350470453d`），不是旧版的 float 乘法。

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
   720|### 本轮（cb-wg-feat-a）追加

`--ore-only` 泛化成 `--types ID,ID`（Java 侧 `ChunkgenOracle.java` 和 Rust 侧
`chunkgen.rs` 两边对齐，`features_per_step` 的全局 index、`setFeatureSeed` 参数不变）；
`Generator::features` / `FeatureContext` 的过滤参数从 `&'static [&'static str]` 改成
`Option<&[String]>`。新增 `feature/dispatch.rs` 做类型分派，`run.rs` 把写死的
`minecraft:ore` 改成按类型调用已移植模块。新增 `predicates.rs`（`BlockPredicate`）、
`state_provider.rs`（`BlockStateProvider`）、`disk.rs`、`underwater_magma.rs`、
`spring.rs`；`placement.rs` 加 `block_predicate_filter` /
`surface_relative_threshold_filter`，`provider.rs` 加 `very_biased_to_bottom`，
`world.rs` 的 `set_block_state` 照 `Heightmap.update` 增量更新四种 `FINAL_HEIGHTMAPS`。
5.2 的分类表已单独提交（`70b5db1`）。

每个新加的 `--types` 比较都先故意写错方块、确认报出 `mismatches` 再改回：disk 写
`STONE` 报 160 处；spring 写 `STONE` 报 1 处；underwater_magma 写 `STONE` 在 24 / 72
中心下分别报 11 / 36 / 51 处；改回后全部 0 不一致。

| 命令 | 结果 |
|---|---|
| `chunkgen java --through features --steps 6`（step 6 全 34 个 placed feature） | `--seeds 1 --chunks 1` / `2×4 --salt 1` / `3×8 --salt 7` / `6×12 --salt 1` 均 **0 不一致** |
| `--types minecraft:disk,minecraft:underwater_magma,minecraft:spring_feature`（跨全部 11 步） | `--seeds 2 --chunks 4` / `3×8 --salt 7` / `6×12 --salt 1`（72 中心 / 63700992 方块）均 **0 不一致** |
| `--types minecraft:disk` / `minecraft:spring_feature` / `minecraft:underwater_magma`（分别跑） | 多种子 / 区块 **0 不一致**（magma 跑到 72 中心 / 63700992 方块） |
| `--steps 6 --ore-only`（回归） | `--seeds 2 --chunks 4 --salt 1` **0 不一致** |
| `--through carvers`（旧阶段回归） | `--seeds 2 --chunks 6 --salt 1` **0 不一致** |
| `--steps 2 --feature-types minecraft:geode --chunk X Z`（晶洞，6 个已知生成点） | 每个 **0 不一致**（每区块 884736 方块、886272 次比较） |
| `--steps 1 --feature-types minecraft:lake` | `--seeds 2 --chunks 4` / `--seeds 3 --chunks 8 --salt 7`（24 中心 / 21233664 方块）均 **0 不一致** |
| `--steps 2 --feature-types minecraft:large_dripstone --chunk 2 36 --scan 25`（25 个滴水石洞穴区块） | **0 不一致**（22118400 方块）；这批位置官方与 Bamboo 都没长柱 |
| `--steps 2 --feature-types minecraft:large_dripstone --chunk -53 -206` / `-50 -200` / `-52 -204` / `-49 -199`（4 个真的长柱的区块） | 每个 **0 不一致**；故意把 `dripstone_block` 改成 `calcite` 后报 **13 / 33 处不一致**，主路径已逐方块覆盖 |
| `--steps 4 --feature-types minecraft:blue_ice --chunk X Z`，5 个真实生成点 `(255,-109) (260,-106) (242,-104) (243,-103) (241,-102)` | 每个 **0 不一致**（884736 方块 / 886272 次比较）；故意把 `yOff < 2` 改成 `yOff < 3` 后报 **5 处不一致** |
| `--steps 4 --feature-types minecraft:spike --chunk 268 -300 --scan 3`（冰刺群系） | **0 不一致**（9 中心 / 7962624 方块）；故意把 `nextInt(4) + 7` 改成 `+ 8` 后报 **160 处不一致** |
| `--steps 3 --feature-types minecraft:monster_room --chunk 3 -1` / `--chunk 4 -6` / `--chunk 3 1` | 三个真实生成点各 **0 不一致**；`--seeds 2 --chunks 4` **0 不一致**（8 中心 / 7077888 方块）；故意把 `nextInt(2) + 2` 改成 `+ 3` 后报 **160 处不一致** |
| `--steps 7 --feature-types minecraft:speleothem_cluster --chunk -55 -200 --scan 9`（3×3 网格） | **0 不一致**（9 中心 / 7962624 方块）；`--chunk -56 -200 --scan 3` / `--chunk -53 -200 --scan 3` 也各 **0 不一致**；故意把 `next_float() > density` 改成 `> density + 0.9` 后报 **105 处不一致** |
| `--steps 4`（step 4 全量） | `--seeds 2 --chunks 4` **0 不一致** |
| `--steps 2`（step 2 全量：`lake` / `geode` / `iceberg` / `large_dripstone` / `block_blob`） | `--seeds 2 --chunks 4` **0 不一致** |

**本轮的接手工作**（2026-11-14 接续）：

* 先把前任留下的、损坏的 `git merge cb-wg-aquifer` 中止，改为直接合并 `main`
  （含水层修复已在 main 上）。前任的工作区冲突解决把 main 侧的组合类地物、
  `simple_block`、`block_predicate`、`composition`、`heightmap`、`survival` 全丢了；
  这次以 main 为骨架重新合并，把我方类型挂上去，同时删掉了并行的重复实现
  （`predicates.rs` 并入 `block_predicate.rs`，`state_provider.rs` 合并两边形态）。
* `--types` 诊断白名单被 main 的 `--feature-types` 取代（超集：支持按 placed feature
  名字或 Feature 类型 ID 匹配），保留 main 的实现。
* 新增 `geode.rs`、`lake.rs`；`noise.rs` 补 `NormalNoise.createParity` 与
  `computeParityBaseAmplitude`；`provider.rs` 补 `IntProvider::max_value`；
  `FeatureWorld` 补 `seed` / `sea_level` / `is_inside_build_height` /
  `is_outside_build_height`；`block_predicate.rs` 补 `inside_world_bounds`；
  `placement.rs` 补 `environment_scan`。
* `ChunkgenOracle.java` 的 `WorldGenLevel` 代理补 `isInsideBuildHeight`
  （此前 `environment_scan` 会抛 `UnsupportedOperationException`）；`block_predicate.rs`
  补 `inside_world_bounds`；`placement.rs` 补 `environment_scan` 修饰器；
  `provider.rs` 补 `IntProvider::max_value` 与浮点 `FloatProvider`（只 `uniform`）。
* 新增 `large_dripstone.rs`（大型滴水石柱）：`Column.scan` 空腔扫描、钟乳石/石笋的
  「往回挪到根嵌进石头」、`SpeleothemUtils` 的高度曲线与风偏移，都按 26.3 移植。
* 新增 `blue_ice.rs`（冻洋蓝冰）：200 次尝试里的 `nextInt(5)` / `nextInt(6)` /
  两个 `nextInt(xzDiff)` 顺序，以及 `yOff / 2` 的**截断除法**（Java 负数向零取整，
  不能用 `div_euclid`）都照官方。
* 新增 `spike.rs`（冰刺）：锥体上下对称、`runLength` 柱脚、`Mth.ceil(scale)` 照官方。
  **关键坑**：官方的 `(insideCircle) && (keep)` 会在 `insideCircle` 为假时短路掉 `keep`
  里的 `random.nextFloat()`；一开始把 `keep` 提前求值，导致每格多取一次随机数，对拍
  报 6 处不一致，改正后 0 不一致。
* 新增 `speleothem_cluster.rs`（滴水石簇 / 硫磺尖簇）：`Column.scan`、`withFloor`、
  `canPlacePool`、`replaceBlocksWithBaseBlocks`、`growSpeleothem`、`clampedMap` 都按官方
  写成；`WorldgenRandom` 补了 `next_gaussian`（`MarsagliaPolarGaussian`，缓存挂在
  `WorldgenRandom` 上、跨 `setSeed` 保留），`FloatProvider` 扩成 `uniform` +
  `clamped_normal` 并接受裸浮点常量（`wetness: 0.0`）。
  **之前卡在 Java 代理**：`ChunkgenOracle` 的 `WorldGenLevel` 缺 `isWaterAt`，
  `SpeleothemUtils.growSpeleothem` 一调用就抛 `UnsupportedOperationException`；补上后
  在滴水石洞穴群系（`--chunk -55 -200 --scan 9`）0 不一致。
* 新增 `monster_room.rs`（地牢）：之前差 2 格的根因是 `dy` 循环少了 `dy = 3`
  （写成 `for dy in (0..=3).rev().map(|v| v - 1)`，漏了 3），改成 `(-1..=3).rev()` 后
  三个真实生成点（`(3,-1)` / `(4,-6)` / `(3,1)`）全部 0 不一致。
* `ChunkgenOracle.java` 的 `WorldGenLevel` 代理补 `isInsideBuildHeight` 与
  `isWaterAt`（共用文件，各 4–8 行）。
* 两个选点探针（`#[ignore]` 测试，手动跑）：
  `cargo test --release -p mc-bamboo-worldgen --lib list_geode_chunks -- --ignored`
  （环境变量 `GEODE_SEED` / `GEODE_RADIUS`）和
  `... find_dripstone_caves_chunks -- --ignored`（`DRIP_SEED` / `DRIP_RADIUS`，
  只采群系，比跑地形快一个数量级）。晶洞和滴水石柱都太稀疏，随机区块撞不上，
  必须先扫出真实生成点再 `--chunk X Z` 逐个复核。

晶洞：`rarity_filter` 是 1/24，seed 0、半径 8 里只有 6 个区块生成，逐个对拍全 0 不一致。
滴水石柱：`dripstone_caves` 群系本身很稀疏（seed 0 的 ±64 区块里只有 9 个），
在这 9 个位置及其 5×5 邻域里，官方和 Bamboo 都**没有**长出柱子——所以「实际生成柱」这条
路径本轮没有被逐方块覆盖，只覆盖了「该位置不长柱」的路径。两个新类型都先故意改坏一处
确认对拍会失败（晶洞改 `filling` 层半径报 130 处；湖把椭球个数从 `nextInt(4)+4` 改成
`+3` 报 40 处），还原后回 0。

### 本轮（cb-wg-caves）追加

按官方反编译源码逐段移植洞穴和冰雪类地物，共用新模块 `feature/dripstone_utils.rs`
（26.3 把旧版 `DripstoneUtils` 更名成 `SpeleothemUtils`；这里连同 `Column.scan`、
`FloatProvider`（uniform / clamped_normal / constant）、`Mth.sin/cos` 查表、
`clampedMap`、`getSpeleothemHeight`、`growSpeleothem` / `buildBaseToTipColumn`、
带 `vertical_direction` / `thickness` / `waterlogged` 的 `pointed_dripstone` 状态一起实现）。

新增并接进 `dispatch.rs`（按类型 ID 字母序）的类型：

| 类型 ID | 官方类 | placed feature |
|---|---|---|
| `minecraft:blue_ice` | `BlueIceFeature` | `blue_ice` |
| `minecraft:large_dripstone` | `LargeDripstoneFeature` | `large_dripstone` |
| `minecraft:speleothem` | `SpeleothemFeature` | `pointed_dripstone`、`sulfur_spike`（容器内的叶子） |
| `minecraft:speleothem_cluster` | `SpeleothemClusterFeature` | `dripstone_cluster`、`sulfur_spike_cluster` |
| `minecraft:spike` | `SpikeFeature` | `ice_spike` |
| `minecraft:freeze_top_layer` | `SnowAndFreezeFeature` | `freeze_top_layer` |

`minecraft:disk` 的 `ice_patch`（step 4）沿用已移植的 `DiskFeature`，本轮一起放开对拍。
配套改动：`WorldgenRandom` 补 `nextGaussian`（`MarsagliaPolarGaussian` 用
`nextDouble()` 且缓存值跨 `setFeatureSeed` 不清，照 `WorldgenRandom.setSeed` 的覆盖行为）、
`FeatureWorld` 带 `seaLevel`、Java 对拍代理补 `LevelReader.isWaterAt` 默认实现
（`SpeleothemUtils.growSpeleothem` 会调）。

每个新 `--types` 比较都先把 `FeatureWorld.set_block_state` 改成写 `STONE`，确认报出
`mismatches` 再改回：全类型一起放开报 2000 处；`large_dripstone` 报 30 处、
`speleothem_cluster` 报 60 处、`spike` 报 60 处、`disk` 在合并对拍里报出
sand/clay/gravel 差异；改回后全部 0 不一致。Java 输出里的 palette 还确认了
`dripstone_block` / `pointed_dripstone` / `packed_ice` 确实被放置。

| 命令 | 结果 |
|---|---|
| `--types minecraft:blue_ice,minecraft:disk,minecraft:large_dripstone,minecraft:speleothem_cluster,minecraft:spike`（一起放开，跨全部 11 步） | `--seeds 6 --chunks 8 --salt 13` / `--salt 99` / `--salt 7` / `--salt 42`（各 48 中心 / 42467328 方块）、`--seeds 9 --chunks 10 --salt 13`（90 中心 / 79626240 方块）均 **0 不一致 / 0 skipped** |
| `--types minecraft:blue_ice` 单独 | `--seeds 2 --chunks 7 --salt 1` **0 不一致** |
| `--types minecraft:large_dripstone` 单独 | `--seeds 6 --chunks 4 --salt 13` **0 不一致** |
| `--types minecraft:speleothem_cluster` 单独 | `--seeds 6 --chunks 4 --salt 13` **0 不一致** |
| `--types minecraft:spike` 单独 | `--seeds 6 --chunks 8 --salt 13` **0 不一致** |
| `--types minecraft:disk` 单独 | `--seeds 6 --chunks 4 --salt 13` **0 不一致** |
| `--steps 6 --ore-only`（回归） | `--seeds 2 --chunks 4 --salt 1` **0 不一致** |
| `--through carvers`（旧阶段回归） | `--seeds 2 --chunks 6 --salt 1` **0 不一致** |
| `worldgen java`（旧阶段回归） | `--seeds 2 --salt 1` **0 不一致** |

#### 本轮追加（第二轮接手）：`speleothem` 与 `freeze_top_layer`

`pointed_dripstone` / `sulfur_spike` 这两个 placed feature 的配置类型是
`minecraft:simple_random_selector`（组合类，已由 `cb-wg-feat-b` 合入），容器里的叶子才是
`minecraft:speleothem`（26.3 新的通用滴水石类，旧的 `PointedDripstoneFeature` 已不存在）。
本轮补上叶子本身、它用的 `EnvironmentScanPlacement` 修饰器和 `ClampedNormalInt`
（`pointed_dripstone` 的 `offset` 三个轴都是 `clamped_normal`）：

| 新增 | 官方出处 | 备注 |
|---|---|---|
| `feature/speleothem.rs` | `SpeleothemFeature#place`、`#getTipDirection`、`#createPatchOfBaseBlocks` | 随机数顺序：`getTipDirection` 的 `nextBoolean` → `createPatchOfBaseBlocks` 的四组 `nextFloat` → `place` 的 `nextFloat` |
| `Modifier::EnvironmentScan` | `EnvironmentScanPlacement#modify` | 中途命中即 return；出界即 return；`allowed` 不成立先 `break` 再补一次判定 |
| `IntProvider::ClampedNormal` | `ClampedNormalInt#sample` | `(int)Mth.clamp(Mth.normal(random, mean, deviation), min, max)` |
| `feature/freeze_top_layer.rs` | `SnowAndFreezeFeature#place` + `Biome#shouldFreeze` / `#shouldSnow` + `SnowLayerBlock#canSurvive` | 走逐点群系解析，复用 `biome_temperature` 里的 `getHeightAdjustedTemperature` |

`freeze_top_layer` 接在 `composition.rs` 的 `place_feature` 里（不用改 `Feature::place`
签名）：它是唯一需要逐点群系回调的叶子，主循环的 `PlacementEnv.biome_at` 已经带着
`BiomeManager` 的未缓存解算器。Java 对拍代理补了 `isInsideBuildHeight`、
`getBrightness`（方块光 0 / 天空光 15）和 `canSeeSky`，否则官方 `Biome.shouldFreeze` /
`shouldSnow` 会撞到 `UnsupportedOperationException`。

| 命令 | 结果 |
|---|---|
| `--types minecraft:pointed_dripstone,minecraft:sulfur_spike`（跨全部 11 步） | `--seeds 6 --chunks 8 --salt 13` / `--salt 99`（各 48 中心 / 42467328 方块）**0 不一致 / 0 skipped**；`--seeds 4 --chunks 10 --salt 42`（40 中心 / 35389440 方块）**0 不一致** |
| `--through features --steps 7`（step 7 全 7 个 placed feature） | `--seeds 2 --chunks 4 --salt 1`（8 中心 / 7077888 方块）**0 不一致** |
| `--through features --steps 10`（`freeze_top_layer`） | `--seeds 3 --chunks 6 --salt 1`（18 中心 / 15925248 方块）、`--seeds 6 --chunks 8 --salt 13`（48 中心 / 42467328 方块）、`--seeds 5 --chunks 10 --salt 7`（50 中心 / 44236800 方块）均 **0 不一致** |
| 全 7 个类型一起放开 | `--seeds 6 --chunks 8 --salt 13` / `--salt 99`（各 48 中心）、`--seeds 9 --chunks 10 --salt 3`（90 中心 / 79626240 方块）均 **0 不一致 / 0 skipped** |
| 故意改坏 | `speleothem` 每次先写一格 `STONE`：`--seeds 6 --chunks 8 --salt 13` 报 **9 mismatches**；`freeze_top_layer` 把雪换成 `STONE`：`--seeds 3 --chunks 6 --salt 1` 报 **160 mismatches**；改回后都是 0 |

`SnowLayerBlock.canSurvive` 的「下方是 8 层满雪」这一条最初写成「下方有 `layers` 属性」，
自查时改成真的比较当前层数（`layers_of`），并加了单元测试
`layers_of_reads_the_current_layer_count` 覆盖 1..8 层。

**注意**：`--seeds 3 --chunks 3 --salt 13` 这类小配置里 `pointed_dripstone`/
`sulfur_spike` 一格都没放（那两个群系不在窗口内），即使把 `place` 整个改成
`return false` 也报 0 —— 这是空跑，不能算验证。上面的 0 不一致都选了能真正放方块的
配置，并用故意改坏确认过。


**`blue_ice` 的对拍前提**：`BlueIceFeature.place` 要先在候选中找到相邻的
`packed_ice`（原版由 `iceberg_packed` / `iceberg_blue` 这两个 `minecraft:iceberg`
在冻洋水下生成）。`iceberg` 已随 main 合入，所以现在可以把它和 `blue_ice` 一起放开：
`--types minecraft:blue_ice,minecraft:iceberg --chunk -200 117 --scan 3 --seeds 1 --salt 13`
（种子 0 的 `frozen_ocean`）是 **0 不一致 / 0 skipped**；把 `place` 开头改成写一格
`STONE` 会报 **54 处**，确认不是空跑。种子 -1 的 `(53,78)` 附近同样 0 不一致。
默认坐标（`FIXED_CHUNKS` + 随机）里没有冻洋，单跑 `blue_ice` 仍会空跑。
实现本身按 `BlueIceFeature.java` 逐句移植（海平面、水、packed_ice 邻格、200 次偏移扩散）。

### 本轮（cb-wg-caves 收尾）追加

合并 `main` 与 `cb-wg-aquatic` 之后，本分支新增/保留的内容：

| 内容 | 状态 |
|---|---|
| `feature/speleothem.rs`（`pointed_dripstone` / `sulfur_spike` 的叶子） | **已对拍 0 不一致** |
| `feature/freeze_top_layer.rs`（含 `SnowLayerBlock.canSurvive` 与 `snowy` 属性） | **已对拍 0 不一致** |
| `modifier::EnvironmentScan` / `int_provider::ClampedNormal` | 由上面两项使用，随它们验证 |
| `modifier::RandomChance` | `sculk_patch_deep_dark` 的 `sequence` 用；已随 `--steps 7` 验证 |
| 代理补 `isInsideBuildHeight` / `getBrightness` / `canSeeSky` / `playSound` / `levelEvent` / `findBlocksIn` | 支撑上面两项与 sculk 家族 |
| `survival::classify` 补 `SculkCatalystBlock` / `SculkSensorBlock` / `SculkShriekerBlock` / `CalibratedSculkSensorBlock` → `Kind::Default` | 官方四类都没覆写 `canSurvive`；只影响 `sculk_patch` 的 `sequence` |
| `feature/sculk_patch.rs`（`SculkPatchFeature` + `SculkSpreader`） | **已对拍 0 不一致，已接入 dispatch** |

对拍配置（全部在 `--through features` 下）：

| 命令 | 结果 |
|---|---|
| `--types minecraft:blue_ice,minecraft:disk,minecraft:large_dripstone,minecraft:speleothem,minecraft:speleothem_cluster,minecraft:spike,minecraft:freeze_top_layer --seeds 4 --chunks 6 --salt 13` | 24 中心 / 21233664 方块，**0 不一致 / 0 skipped** |
| `--steps 7 --types minecraft:sculk_vein --chunk -42 111 --scan 4 --seeds 1 --salt 13` | 9 中心 / 7962624 方块，**0 不一致**；故意改坏 `is_own_block` 报 20 处 |
| `--steps 7 --types minecraft:sculk_patch_deep_dark --chunk -42 111 --scan 3` | **0 不一致 / 0 skipped**（9 中心 / 7962624 方块）；`--seeds 2 --salt 7` 与 `--seeds 3 --salt 42` 各 18 / 27 中心（15925248 / 23887872 方块）也是 0 |
| 同上，把 `attemptPlaceSculk` 写死的方块换成石头 | **160 处不一致**，确认非空跑 |
| `--steps 7`（全 7 个 placed feature 一起放开）`--chunk -42 111 --scan 3` | **0 不一致 / 0 skipped** |

### 还没做

**已完成**（见上两轮「本轮追加」）：step 6 里非 ore 的 5 个 placed feature
（`disk` 4 + `underwater_magma` 1）和 step 8 的 3 个 `spring_feature` 已按官方 26.3
移植；`block_predicate_filter` / `surface_relative_threshold_filter` 两个修饰器、
`BlockPredicate` / `BlockStateProvider`（simple / rule_based）、`very_biased_to_bottom`
高度提供者、`Heightmap.update` 增量更新均已补齐。这些类型在 `--steps 6` 全量、
以及跨全部 11 步的 `--types` 白名单下都逐方块 0 不一致。

**还没做**（按 5.2 的分类，仍标 `unsupported` / `skipped`）：

* step 1/2/3/4/7/10 的其余非植被类型：`fossil`、`overlay`（desert_well）。
  已完成的是 `disk`、`underwater_magma`、`spring_feature`、`iceberg`、`block_blob`、
  `geode`、`lake`、`large_dripstone`、`blue_ice`、`spike`、`monster_room`、
  `speleothem_cluster`（`cb-wg-feat-a`），以及 `speleothem`（`pointed_dripstone` /
  `sulfur_spike`）、`freeze_top_layer`（`cb-wg-caves`）。
* step 7（`UNDERGROUND_DECORATION`）的非植被部分：`sulfur_spike_cluster` /
  `dripstone_cluster`（`speleothem_cluster`）已移植并对拍 0 不一致。
  `sculk_vein` 的 `multiface_growth` 由 `cb-wg-aquatic` 移植（`feature/multiface.rs`），
  在 `deep_dark` 定点对拍 0 不一致（9 中心 / 7962624 方块）；
  **默认坐标下它一次都不放方块，必须 `--chunk -42 111 --scan N` 才比得到**。
  `sculk_patch_deep_dark`（`sequence` 包着 `sculk_patch`）也已完成：
  `feature/sculk_patch.rs` 实现了 `SculkPatchFeature` + `SculkSpreader` + `SculkBlock` /
  `SculkVeinBlock` 两个 `SculkBehaviour` + `SculkVeinSpreaderConfig`，
  **已接入 dispatch 并对拍 0 不一致**（三种子 × 三种扫描尺寸，最多 27 中心 /
  23887872 方块；把 `attemptPlaceSculk` 写成石头会报 160 处）。
  **踩过的坑**：`BlockPos.betweenClosed` 的遍历顺序是 x 内层、y 中层、z 外层，
  按 x 外层写会让 `ChargeCursor` 的 18 个邻格洗牌从不同格子起步，所有游标走错路径
  （表现为写入总量更多但位置全偏，最终 8 处差异）。`sculk_patch_ancient_city`
  （古城结构专用，`placement` 为空）没做。
* 具体原因：`fossil` 的结构模板要由生成器导出（不能手写）；`overlay`（沙漠井）依赖
  `minecraft:template` 地物（要落结构模板 + processor），那条线还没做。
  植被类型（树、花、草、patch、selector / sequence 容器，见 5.2）由 `cb-wg-feat-b` 负责。
* `skipped` 白名单目前只在 Rust 侧用 `unsupported` 标记；Java 侧还没按「已移植集合」
  直接输出 `skipped`，`--types` 是手工白名单。
* 接进 Director 的**多阶段流水线形状**已经就位（流水线分支：`PipelineKind::Staged`
  按 `ChunkPyramid` 派 `structure_starts … full` 十个阶段、terrain 已接真生成）。
  但 **5.3.1 的邻居只读快照 `Arc<ProtoChunk>` + `FeatureResult.deltas` 按 `TaskId`
  合并还没做**：`Task::Generate.neighbors` 仍恒为空，所以游戏里的 `features` 阶段目前
  是透传、不放置任何地物（纯函数只在对拍里跑）。`initialize_light` / `light` / `spawn`
  同样透传。把 features 真正接进游戏要等这块邻居快照落地。
* 方块实体 / POI / 后期处理位置表：Java 侧 `ProtoChunk` 会记，但 `FeatureResult` 只带
  方块；本轮已移植的 disk / magma / spring / 洞穴与冰雪类都不产生方块实体，对拍不需要，
  接树 / 怪物房 / 湖时再补输出和比较。
* 高度图的增量更新：走 `setBlockState` 的路径（`simple_block`、disk、滴水石等）已按
  `Heightmap.update` 在 `set_block_state` 里补齐；矿石走 `BulkSectionAccess` 直接写
  `LevelChunkSection`，**绕过** `ChunkAccess.setBlockState`，所以 step 6 期间高度图是
  primed 后固定的（照官方）。接树 / 湖时复用同一个 `set_block_with_heightmaps` 即可。

## 5.2 `features_per_step` 全部 placed feature 的分类（非植被 / 植被）

分工按「placed feature 的配置类型」（`Feature.DIRECT_CODEC` 的类型 ID）划界：`cb-wg-feat-a`
做非植被，`cb-wg-feat-b` 做植被。下表把导出的 `features_per_step`（11 步、共 167 个
placed feature，主世界 67 个群系的全局有序并集）按类型逐类列出。`ore`（step 6 的 29 个 +
step 7 的 1 个）是流水线里程碑已完成的，本轮不再改，列出来只是为了让表完整。

判据：树、花草、蘑菇、patch、海草/海带/珊瑚、竹、甘蔗、仙人掌、藤蔓、地衣、发光
地衣、孢子花等，以及只包着这些的 selector / sequence 容器归植被；湖、晶洞、冰山、森林
岩石、滴水石、矿石、disk、地下水岩浆、怪物房、化石、泉水、积雪结冰、地下装饰里的
sculk/硫磺/滴水石簇归非植被。**拿不准的容器类型（`sequence`、`simple_random_selector`、
`multiface_growth`、`root_system`、`random_boolean_selector`）一律归植被**，在汇报里列出。

### 非植被（`cb-wg-feat-a`）

| step | 类型 ID | placed feature |
|---:|---|---|
| 1 | `minecraft:lake` | `lake_lava_underground`, `lake_lava_surface` |
| 2 | `minecraft:geode` | `amethyst_geode` |
| 2 | `minecraft:iceberg` | `iceberg_packed`, `iceberg_blue` |
| 2 | `minecraft:large_dripstone` | `large_dripstone` |
| 2 | `minecraft:block_blob` | `forest_rock` |
| 3 | `minecraft:monster_room` | `monster_room`, `monster_room_deep` |
| 3 | `minecraft:fossil` | `fossil_upper`, `fossil_lower` |
| 4 | `minecraft:spike` | `ice_spike` |
| 4 | `minecraft:disk` | `ice_patch` |
| 4 | `minecraft:overlay` | `desert_well` |
| 4 | `minecraft:blue_ice` | `blue_ice` |
| 6 | `minecraft:ore` | 29 个 `ore_*`（已完成） |
| 6 | `minecraft:disk` | `disk_grass`, `disk_sand`, `disk_clay`, `disk_gravel` |
| 6 | `minecraft:underwater_magma` | `underwater_magma` |
| 7 | `minecraft:ore` | `ore_infested`（已完成） |
| 7 | `minecraft:speleothem_cluster` | `sulfur_spike_cluster`, `dripstone_cluster` |
| 8 | `minecraft:spring_feature` | `spring_water`, `spring_lava`, `spring_lava_frozen` |
| 10 | `minecraft:freeze_top_layer` | `freeze_top_layer` |

step 6 的修饰器 `surface_relative_threshold_filter`（3 处）、`block_predicate_filter`（155 处
中的 disk 几处）、以及 disk 用到的 `rule_based` / `simple` 状态提供者、`matching_blocks` /
`matching_fluids` / `solid` / `any_of` / `not` 谓词也归 `cb-wg-feat-a`（它们是放置/配置的
公共构件，不是某一类地物）。

### 植被（`cb-wg-feat-b`）

| step | 类型 ID | 个数 | 代表 placed feature |
|---:|---|---:|---|
| 1 | `minecraft:root_system` | 1 | `rooted_sulfur_spring`（拿不准，归植被） |
| 1 | `minecraft:sequence` | 1 | `sulfur_pool`（拿不准，归植被） |
| 7 | `minecraft:multiface_growth` | 1 | `sculk_vein`（拿不准，归植被） |
| 7 | `minecraft:sequence` | 1 | `sculk_patch_deep_dark`（拿不准，归植被） |
| 7 | `minecraft:simple_random_selector` | 2 | `sulfur_spike`, `pointed_dripstone`（拿不准，归植被） |
| 9 | `minecraft:tree` | 2 | `trees_swamp`, `trees_cherry` |
| 9 | `minecraft:random_selector` | 23 | `trees_plains`, `trees_savanna`, … |
| 9 | `minecraft:weighted_random_selector` | 9 | `seagrass_river`, `trees_dappled_forest`, … |
| 9 | `minecraft:simple_random_selector` | 3 | `flower_forest_flowers`, `warm_ocean_vegetation`, … |
| 9 | `minecraft:random_boolean_selector` | 2 | `lush_caves_clay`, `mushroom_island_vegetation` |
| 9 | `minecraft:simple_block` | 52 | `flower_plains`, `patch_grass_normal`, 蘑菇、孢子和花等 |
| 9 | `minecraft:bamboo` | 2 | `bamboo`, `bamboo_light` |
| 9 | `minecraft:block_column` | 9 | `cave_vines`, `patch_sugar_cane`, `patch_cactus`, `kelp_*` |
| 9 | `minecraft:vegetation_patch` | 3 | `pale_moss_patch`, `lush_caves_vegetation`, … |
| 9 | `minecraft:vines` | 2 | `classic_vines_cave_feature`, `vines` |
| 9 | `minecraft:multiface_growth` | 1 | `glow_lichen` |
| 9 | `minecraft:root_system` | 1 | `rooted_azalea_tree` |

`placed_feature` 注册表里还有一批只在下界/末地群系出现的类型（`end_island`、`end_spike`、
`chorus_plant`、`huge_fungus`、`delta_feature`、`netherrack_replace_blobs`、`void_start_platform`
等），它们**不在**主世界 `features_per_step` 里，本轮不做。
### 植被地物进度（分支 `cb-wg-feat-b`）
按 `features_per_step` 分类见 `docs/worldgen-vegetal-features.md`：step 9 共 109 项。
**范围调整（2026-11-14）**：树（`tree` 全部变种、`fallen_tree`、巨型蘑菇）移交
`cb-wg-trees`，水下植被与其余植被移交 `cb-wg-aquatic`；本分支只做组合类、
`simple_block` 剩余种类和谓词。逐项提交：

前任已落地（`804f1f3` .. `606ae1b`）：`--feature-types` 过滤、`Heightmap.update`
增量更新、`IntProvider` / `BlockPredicate` / `block_predicate_filter` 支撑件、
`SimpleBlockFeature` 内联 bare-state + 方块类白名单，以及 11 个内联 simple_block
植被 placed feature 的 0 不一致对拍。

本轮新增（`95629d7`、`54af08b`、`ad01c41`）：

* `survival.rs`：共享 `canSurvive` 分派，白名单扩到 SweetBerryBush / Eyeblossom /
  FireflyBush / Sapling / FlowerBed（`supports_vegetation`）、Short/TallDryGrass
  （`supports_dry_vegetation`）、LeafLitter（`isFaceSturdy(FULL, UP)`）、
  DoublePlant / TallFlower（标签判定 + `placeAt`）。蘑菇（`getRawBrightness`）、
  睡莲、孢子花、苔藓地毯、海泡菜（碰撞形状面）等特殊 `canSurvive` 仍 `unsupported`。
* `SimpleBlockFeature`：`DoublePlantBlock.placeAt`（上下两格 + `half` 属性）、
  上方格的 `!isAir → fluidEquals && canBeReplaced` 检查；`schedule_tick` 忽略。
* 三个谓词：`would_survive`（复用 `survival::can_survive`）、`has_sturdy_face`
  （`facts::STURDY_FULL` 位）、`matching_fluids`（流体 HolderSet）。
* `state_provider.rs`：typed `BlockStateProvider` —— `weighted`
  （`WeightedList.nextInt(totalWeight)` 累计权重轮盘）、`randomized_int`
  （source + `IntProvider.sample` 改整数属性）、内联 bare-state。holder 字符串引用
  （`BLOCK_STATE_PROVIDER` 注册表未导出）仍 `unsupported`。
* `composition.rs`：组合类框架 + 递归 `PlacedFeature.place` —— `random_selector` /
  `weighted_random_selector` / `simple_random_selector` / `random_boolean_selector` /
  `sequence`；子项为注册表引用或内联配置，子项用 `placeWithBiomeCheck=false`
  （topFeature 为空）。`place_candidates` 改为递归回调并汇总 `supported` / `placed`。
* `clamped` IntProvider（递归 source + `clamp`）。

#### 本分支对拍结果（`--through features --steps 9`）

| 命令（省略 `--through features`） | 结果 |
|---|---|
| `--steps 9 --feature-types "$FEATS_OLD" --seeds 2 --chunks 4 --salt 1`（11 个前任名字回归） | 0 不一致（8 中心 / 7077888 方块，0 skipped） |
| `--steps 9 --feature-types "$FEATS_INLINE" --seeds 3 --chunks 30 --salt 3`（12 个新内联 simple_block） | 0 不一致（90 中心 / 79626240 方块，0 skipped） |
| `--steps 9 --feature-types "$FEATS_INLINE" --seeds 2 --chunks 40 --salt 7` | 3 格差异，全是 terrain 含水层既有差异（见下），features 阶段无新增 |
| 故意把 simple_block 放的状态换成石头（`FEATS_INLINE`，salt 7） | 160 处不一致（确认非假绿） |
| `--steps 9 --feature-types "forest_flowers,flower_forest_flowers" --seeds 3 --chunks 30 --salt 3` | 0 不一致（90 中心 / 79626240 方块，0 skipped） |
| 故意把 `simple_random_selector` 固定选第 0 个子项（同参数） | 52 处不一致（确认非假绿） |
| `--steps 9 --feature-types "$FEATS_WEIGHTED" --seeds 3 --chunks 30 --salt 3`（9 个 weighted provider） | 0 不一致（90 中心 / 79626240 方块，0 skipped） |
| 故意把 weighted provider 固定选第 0 条（同参数） | 120 处不一致（确认非假绿） |
| `--steps 6 --ore-only`、`--through carvers`（`--seeds 2 --chunks 4/6 --salt 1`）、`worldgen java` | 各 0 不一致（旧阶段回归） |
| `cargo test -p mc-bamboo-worldgen` | 115 个测试全过 |

**terrain 含水层既有差异**：`--salt 7` 下 seed -1、中心 chunk (52,-27) 的 +z 邻居
(52,-26)、y=-54、x=14、z=1..3 三格，官方是水、Bamboo 是深板岩；`--steps 0`
（不跑任何 feature）同样复现，确认与地物改动无关，是邻居区块 terrain 的既有差异
（旧阶段 carvers 回归只比中心单区块，故未暴露）。上面的 0 不一致结果都用 `--salt 3`
等避开该区块的参数取得。**该差异已在 `cb-wg-aquifer` 分支修复**：补齐官方
`Aquifer.NoiseBasedAquifer.computeSubstance` 的「水 + 下方岩浆」短路后，(52,-26) 以
`chunkgen --chunk 52 -26` 精确复现为 0 不一致，详见 `docs/worldgen.md`「岩浆层上边界的短路」。

#### 本分支明确还没做（诚实标 `unsupported`）

* 树（全部变种、`fallen_tree`、巨型蘑菇）和水下/其余植被已按分工移交
  `cb-wg-trees` / `cb-wg-aquatic`，不在本分支范围。
* 组合类里只有 `simple_random_selector`（forest_flowers / flower_forest_flowers）
  拿到 0 不一致对拍；`random_selector` / `weighted_random_selector` /
  `random_boolean_selector` / `sequence` 的 place 逻辑按官方源码逐行移植，但子项多为
  树 / 水生（已移交），本轮未逐方块验证，待对方分支 merge 本分支后再放开对拍。
  `projected_random_patchy_square` 在 26.3 导出数据里没有条目。
* `to_place` 的 holder 字符串引用（flower_plain / flower_meadow / flower_flower_forest）：
  `BLOCK_STATE_PROVIDER` 注册表未导出，需生成器补数据；特殊 `canSurvive`
  （蘑菇、睡莲、孢子花、苔藓地毯、海泡菜）同上。
* `noise_threshold_count` / `noise_based_count` / `environment_scan` /
  `surface_water_depth_filter` / `surface_relative_threshold_filter` 修饰器、
  `clamped_normal` / `weighted_list` IntProvider、方块实体数据（蜂巢、可疑沙等）。
* 接进 Director 的多阶段生成与邻居增量合并（流水线分支负责）。

### 水生和其他植被进度（分支 `cb-wg-aquatic`，基于 `cb-wg-feat-b` 的 `606ae1b`）

范围：分类表里「植被」类中除组合类、`simple_block` 和树以外的全部类型。本分支已合并
`main`（`fdd9d5c`，含邻居快照、`features` 阶段接入、组合类、Beardifier）与
`cb-wg-feat-b` 的最新提交（`d40fd7c`）。

本轮落地（逐项提交）：

* `block_column`（`8e4040a`）：`BlockColumnFeature` 逐行照官方 `place` —— 先按层
  `height.sample(random)` 累加总高、总高为 0 直接返回 false、从 `origin + direction`
  起按 `allowed_placement` 逐格检查、放不下就 `truncate`（`prioritize_tip` 决定砍头
  还是砍尾）、放置阶段每格调一次 `BlockStateProvider.getState` 再走
  `setBlock(pos, state, 2)`（即 `set_block_with_heightmaps`）。覆盖 `cactus`（两层：
  `biased_to_bottom(1..3)` 的仙人掌 + `weighted_list` 的花）、`sugar_cane`（
  `biased_to_bottom(2..4)`）、`kelp`（`uniform(0..9)` 的 `kelp_plant` + 单格
  `randomized_int(age 20..23)` 的头部，`prioritize_tip=true`）、`cave_vine`
  （`weighted_list` 高度 + `randomized_int` 头部，`direction=down`）与
  `twisting_vines`。
* `weighted_list` `IntProvider`（同提交）：`WeightedList.getRandomOrThrow` ——
  先 `nextInt(totalWeight)`，再 sample 选中的那一项。
* `cactus` / `sugar_cane` 的 `canSurvive`（`survival.rs`）：仙人掌的
  「水平相邻非 `isSolid`、非岩浆」「下面在 `supports_cactus` 或同类」「上面非
  `liquid()`」，甘蔗的「下面是甘蔗直接成立；否则下面在 `supports_sugar_cane` 且水平
  相邻的流体或方块在 `supports_sugar_cane_adjacently`」。
* `environment_scan` / `surface_relative_threshold_filter` 修饰器（`5b6d57d`）：
  竖直逐格扫描（先 `allowed_search_condition` 再 `target_condition`，
  `isOutsideBuildHeight` 立即返回，扫描条件失败后补试一次 target）；
  `surfaceY + min <= y <= surfaceY + max`（用 `i64` 算，避免溢出）。
* `multiface_growth`（`2a936d8`）：`MultifaceGrowthFeature` 全体 ——
  `validDirections()`（天花板 / 地板 / 四个墙面按官方加入顺序）、
  `Util.shuffledCopy` 的 Fisher-Yates（每步一次 `nextInt`，size 为 0/1 时不取数）、
  `placeGrowthIfPossible`（`getStateForPlacement` 返回 null 时**直接 false**，不再试
  下一个方向；成功后 `nextFloat` 判 `chance_of_spreading`）、
  `MultifaceSpreader.DefaultSpreaderConfig` 的三种 `SpreadType`（同一格 / 同一平面 /
  绕过去）与 `stateCanBeReplaced`。覆盖 `glow_lichen`（step 9）与 `sculk_vein`
  （step 7）。

对拍（`--through features`，逐方块，0 不一致）：

| 集合 | 参数 | 结果 |
|---|---|---|
| `minecraft:block_column`（9 个 placed feature） | `--steps 9 --seeds 3 --chunks 8 --salt 3` | 0 不一致（23 中心 / 20348928 方块 / 1 skipped） |
| 同上，故意把柱状方块换成石头 | 同上 | 160 处不一致（确认非假绿） |
| `seagrass_normal,warm,deep` + `sea_pickle` | `--steps 9 --seeds 3 --chunks 12 --salt 5` | 0 不一致（20 中心 / 17694720 方块 / 16 skipped） |
| 9 个 seagrass + `sea_pickle` + `patch_waterlily` + `spore_blossom`（`6a3e4aa`） | `--steps 9 --seeds 3 --chunks 8 --salt 3` | 0 不一致（24 中心 / 21233664 方块，0 skipped） |
| 同上，故意把 15 个 placed feature 一起放开 | `--steps 9 --seeds 4 --chunks 20 --salt 11` | 15 处不一致，全部是 terrain 既有差异（见下） |
| 同上 | `--steps 9 --seeds 4 --chunks 20 --salt 3` | 36 处不一致，`--steps 0` 复现同一批，属 terrain |
| `minecraft:cave_vines` | `--steps 9 --seeds 3 --chunks 8 --salt 3` | 0 不一致（23 中心 / 20348928 方块 / 1 skipped） |
| `minecraft:kelp_cold` | `--steps 9 --seeds 3 --chunks 12 --salt 5` | 0 不一致（36 中心 / 31850496 方块） |
| `minecraft:glow_lichen` | `--steps 9 --seeds 3 --chunks 12 --salt 7` | 0 不一致（36 中心 / 31850496 方块） |
| `glow_lichen,sculk_vein` | `--steps 9 --seeds 3 --chunks 8 --salt 3` | 0 不一致（24 中心 / 21233664 方块） |
| 同上，故意只保留洗牌后的第一个候选面 | `--seeds 2 --chunks 4 --salt 3` | 12 处不一致（确认非假绿） |
| `minecraft:vines` + `bamboo` 回归 | `--steps 9 --seeds 3 --chunks 8 --salt 3` | 0 不一致（24 中心 / 21233664 方块） |
| `--steps 0`（不跑地物，纯 terrain 回归） | `--seeds 3 --chunks 12 --salt 5` | 0 不一致 |

`--steps 9` 下 `kelp_cold` 单独跑 `--seeds 3 --chunks 12 --salt 5` 曾报 400 处不一致，
换成同一集合去掉 `kelp_cold` 则 0 不一致；**根因是旧构建缓存**：`touch` 源码重编后
同一命令 0 不一致。如果以后复现「某个 placed feature 单独跑就错位、整批跑反而对」，
先确认 `target/debug/mc-bamboo-vanilla.exe` 和 `mc-bamboo-worldgen` 是同一份构建。

### 第二轮：`vegetation_patch`、`random_block` 与蘑菇（分支 `cb-wg-aquatic`）

提交：`ec80aeb`（`JavaPosSet`）、`3b8d9d6`（`vegetation_patch` + `random_block` +
蘑菇/地毯存活判定）、`3479dd3`（修 `HashMap` 的 resize 顺序 + 记录树化限制）。
开工前合入 main 的 `753d877`（地形真实路径修复）。

合并进 main 前（`a486b4e`）又做了一次收尾：合入 main 的树 / 倒木 / 巨型蘑菇 /
`shape_update.rs`；`weighted_list` 取 main 已有一份（`Box<IntProvider>` 版，更通用），
删掉本分支的那份；`environment_scan` / `surface_relative_threshold_filter` /
`multiface` / `JavaPosSet` / `vegetation_patch` / `block_state_provider` 的
`parse_with` main 上没有，保留本分支的。
`waterlogged_vegetation_patch` 因为**完全没对拍**，`e71a389` 里改成不接入
（`place_feature` 只认 `minecraft:vegetation_patch`），代码留着，标 `unsupported`。

本轮落地：

* `feature/java_set.rs`：复刻 `java.util.HashSet<BlockPos>` 的迭代顺序。
  `VegetationPatchFeature` 把地表收进 `HashSet` 后按迭代顺序每点消费一次 `nextFloat`，
  顺序错就整块随机流错位。实现按 OpenJDK 的 `HashMap`：容量 16 起、装载因子 0.75、
  `h = (y + z*31)*31 + x` 再 `h ^ (h >>> 16)`、桶内尾插、`putVal` 先 `treeifyBin`
  （桶内第 8 个且容量 < 64 就翻倍）再判 `++size > threshold`、翻倍按 `hash & oldCap`
  做 lo/hi 拆分并保持相对顺序。单测用官方 JVM 实际打印的顺序（197 点圆盘、64 点椭圆、
  20 点翻倍）逐一比对。
* `feature/vegetation_patch.rs`：`VegetationPatchFeature` 全体 —— `xz_radius` 采样两次
  各 +1、`placeGroundPatch` 的角列/边列短路（边列要 `extra_edge_column_chance != 0`
  且 `nextFloat() <= chance`）、竖直两段扫描、`belowState.isFaceSturdy(opposite)`、
  `depth.sample` + `extra_bottom_block_chance`、`placeGround`（同方块不移动位置）、
  `distributeVegetation` 按集合迭代序、`WaterloggedVegetationPatchFeature` 的
  `isExposed` + 换水 + `waterlogged=true` 覆写。
* `state_provider.rs` 的 `random_block`（`HolderSet<Block>.getRandomElement`，等概率
  `nextInt(size)`，用默认状态）；`StateProvider::parse_with` 支持 `Holder` 字符串查
  `block_state_provider` 注册表。
* `Extractor.java` 新增 `block_state_provider` 键（`worldgen.json` +174 行），解锁
  `cave_vines_body` / `cave_vines_head`，`minecraft:cave_vines` 从 skipped 变成真跑通。
* `survival.rs`：`CarpetBlock.canSurvive`（下面非空气）、`MushroomBlock.canSurvive`。

对拍（`--through features`，逐方块）：

| 集合 | 参数 | 结果 |
|---|---|---|
| `lush_caves_vegetation`（floor `moss_patch`） | `--steps 9 --seeds 3 --chunks 20 --salt 5` | 10 处，`--steps 0` 复现同一批（terrain 既有差异） |
| `pale_moss_patch`（floor，含 `MossyCarpetBlock` 存活判定） | 同上 | 10 处，同 terrain |
| `minecraft:cave_vines`（`block_column` + holder provider） | `--steps 9 --seeds 3 --chunks 8 --salt 3` | **0 不一致，0 skipped**（之前是 skipped） |
| `lush_caves_vegetation,pale_moss_patch,sea_pickle,kelp_cold,glow_lichen,cave_vines` | `--steps 9 --seeds 3 --chunks 12 --salt 7` | **0 不一致，0 skipped**（36 中心 / 31850496 方块） |
| `lush_caves_ceiling_vegetation`（ceiling `moss_patch_ceiling`） | `--steps 9 --seeds 3 --chunks 20 --salt 5` | **20 处未解决**：10 处 terrain + 10 处 `cave_vines` 位置错位，见疑点 |
| 故意跳过 `place_ground_patch` | `--seeds 2 --chunks 4 --salt 3` | 0 处差异（该批次无繁茂洞穴，不能作为非假绿证据） |

#### 已知限制：`HashMap` 桶内 8+ 元素时的树化

官方 `HashMap` 在容量 ≥ 64 且某个桶到第 8 个节点时把桶转成红黑树，`treeify` 之后
`moveRootToFront` 把树根挪到桶首，迭代顺序因此和纯链表不同。本实现只复刻链表
（`ec80aeb` + `3479dd3`），**没有实现 `TreeNode` 树化**。`java_set.rs` 里
`heavy_collisions_expose_the_untreeified_limit` 锁住了这个差异（官方从 192 开始、
本实现从 0 开始）。

这很可能是 `lush_caves_ceiling_vegetation` 那 10 处差异的根因：ceiling 的
`vegetation_chance` 只有 0.08，surface 集合里只有约 5 个点被选中，顺序一错就选中
完全不同的列；floor 的 `vegetation_chance` 是 0.8，几乎所有列都长植被，顺序错也看不
出来。

#### 第三轮：`noise_threshold_count`、`clamped_normal`、`count_on_every_layer`、
`MossyCarpetBlock.placeAt`（分支 `cb-wg-aquatic`）

提交：`8d12409d`、`cad07370`（均为 `WIP:` 开头，按本轮工作模式保留）。

本轮落地：

* `IntProvider::ClampedNormal`：`clamp(mean + (float) nextGaussian * deviation, min, max)`。
  为此给 `WorldgenRandom` 补了 `next_gaussian`（`MarsagliaPolarGaussian`，缓存另一半，
  `set_seed` 时清缓存）。导出数据里只有 `pointed_dripstone` / `sulfur_spike` 用它，
  **不属本分支范围，未对拍**。
* `Modifier::NoiseThresholdCount`：`BIOME_INFO_NOISE.get(x/200, z/200)` 与
  `noise_level` 比较，取 `below_noise` / `above_noise`。对拍见下表。
* `Modifier::CountOnEveryLayer`：逐层找「空气在实心之上」的第 n 个交界，
  床岩不算支撑。导出数据里只被下界类型用。
* `feature/mossy_carpet.rs`：`MossyCarpetBlock.placeAt` —— `getUpdatedState`
  （`BASE` / `LOW` / `TALL` 与上方同类地毯的联动）、`createTopperWithSideChance`
  （每个非 `NONE` 侧面一次 `nextBoolean`）、放 topper 后再更新一次下层。
* `survival.rs` 的 `MossyCarpet` 分支（`canSurvive`：`BASE` 时下面非空气，
  否则下面同类且 `BASE`），`simple_block.rs` 分派到 `placeAt`。

**关键发现：`level.getRandom()` 不是地物的 `random`。** `SimpleBlockFeature.place`
对 `MossyCarpetBlock` 调的是 `MossyCarpetBlock.placeAt(level, origin,
level.getRandom(), 2)`（字节码 129 是 `WorldGenLevel.getRandom()`），而
`WorldGenRegion.random` 来自 `RandomState.getOrCreateRandomFactory("worldgen_region_random")`
在中心区块**世界原点**上取的随机源，整个 `features` 调用共用一份，和按
`(index, step)` 重新播种的地物 `random` 完全无关。

为此在 `Generator` 里加了 `region_random: PositionalFactory`，
`FeatureContext` / `PlacementEnv` 各加一个 `region_random` 字段，
`place_leaf` / `SimpleBlockFeature::place` 把它传到 `MossyCarpet::place_at`。
改之前 `pale_moss_patch` 对拍 15 处不一致，改之后降到 2 处。

对拍（`--through features --steps 9`，逐方块）：

| 集合 | 参数 | 结果 |
|---|---|---|
| `flower_cherry,flower_plains,patch_grass_meadow,patch_grass_plain,patch_tall_grass_2,wildflowers_meadow`（`noise_threshold_count` 的 6 个 placed feature） | `--seeds 4 --chunks 20 --salt 7` | **0 不一致**（79 中心 / 69894144 方块 / 1 skipped） |
| 同上，故意把阈值偏 1.0 | `--seeds 2 --chunks 8 --salt 7` | 24 处不一致（确认比到了） |
| `pale_moss_patch`（`MossyCarpetBlock.placeAt`） | `--seeds 6 --chunks 40 --salt 7` | **2 处未解决**（240 中心 / 212336640 方块 / 0 skipped）。改区域随机源前是 15 处 |
| 14 个已验证 placed feature 一起放开（`block_column,vines,bamboo,multiface_growth,cave_vines,sea_pickle,seagrass_*×4,patch_waterlily,spore_blossom,kelp_cold,lush_caves_vegetation`） | `--seeds 4 --chunks 12 --salt 7` | **0 不一致**（48 中心 / 42467328 方块 / 0 skipped） |

#### 第五轮：合并 main 后的收尾（分支 `cb-wg-aquatic`）

main 已把本分支（`1c9853c6`）合进去了，所以再 `git merge main` 时两边是同一批改动的两个副本，
产生了大量「同一项出现两次」的冲突（`Opaque` 式重复枚举项 / match 分支 / 解析分支）。

处理原则：**同一功能两份一模一样时保留 main 那份**，删掉本分支的副本。

* `provider.rs`：`IntProvider::ClampedNormal` 的枚举项、`parse` 分支、`sample` 分支
  各去重一次（保留 main 的 `number()` 版本）。
* `placement.rs`：`SurfaceWaterDepthFilter` / `Unsupported` 的 match 分支去重。
* `dispatch.rs`：`Ore` / `SimpleBlock` 的 `place_leaf` 分支去重。
* `mod.rs`：`lake` / `large_dripstone` / `monster_room`（main）与
  `mossy_carpet`（本分支）都保留。
* `root_system.rs`（别人在本分支上的提交）有一个 `clippy::identity_op`：
  `1u64 << (facts::STURDY_FULL + 0)` 改成 `1u64 << facts::STURDY_FULL`。

另外修正了 `WorldGenRegion.random` 的采样点：`ChunkPos.getWorldPosition()` 是
`new BlockPos(minBlockX, **0**, minBlockZ)`，y 恒为 **0**，不是维度的 `minY`。
之前用 `min_y`（-64）是错的。

#### 第四轮：`HashMap` 桶树化（分支 `cb-wg-aquatic`）

提交：`e27882c6`（`WIP:` 开头，按工作模式保留）。

`VegetationPatchFeature` 把地表位置收进 `HashSet<BlockPos>` 后按迭代顺序分配植被随机数。
上一轮只复刻了链表形状，桶内到 8 个元素时官方 `HashMap` 会转红黑树，`treeify` 之后
`moveRootToFront` 把**树根**挪到桶首，迭代顺序因此不同。

本轮实现：

* `java_set.rs` 的 `Tree`：按桶内链表顺序逐个插入红黑树（`putTreeVal` + `balanceInsertion`
  + `rotateLeft` / `rotateRight`），比较规则是 `HashMap.treeNode` —— hash 优先，再
  `Vec3i.compareTo`（`y` → `z` → `x`）。`tieBreakOrder` 的 `identityHashCode` 分支用不到：
  `compareTo` 全等的两个位置必然 `equals`，不可能同时存在于同一个 `HashSet`。
* `JavaPosSet::insert` 的 `treeifyBin` 分支：容量 < 64 时翻倍，容量 ≥ 64 时调
  `treeify`（把桶内链表重排成「根在最前、其余保持插入序」）。

单测 `heavy_collisions_move_the_tree_root_to_the_bin_head` 用官方 JVM 实际打印的 40 个
`(i*16, 62, 0)` 的顺序逐项比对，**40 个全部一致**。

对拍：`moss_patch_ceiling` 从 **20 处降到 14 处**（其中 10 处是 terrain 既有差异，
地物阶段剩 4 处）。剩下的 4 处见「疑点」。

#### 第六轮：`vegetation_patch` 家族与垂滴叶（分支 `cb-wg-aquatic`）

按队长指示，**停止「让 HashMap 桶树化后迭代顺序和 Java 一致」**（投入产出比低）。
`java_set.rs` 改成：遇到本该树化的桶（容量 ≥ 64 且桶内 8 个）只置 `treeified` 标记，
`VegetationPatch::place` 看到标记就把整次放置标 `unsupported`，对拍时跳过该区块。
红黑树实现已删除。提交 `f749ee66`。

* `survival.rs` 新增三个 `canSurvive`：
  * `SmallDripleafBlock`：上半格看下半格；下半格看 `supports_small_dripleaf`，
    或本格上方是水源且下面在 `supports_vegetation` 里。
  * `BigDripleafBlock`：下面同类、`big_dripleaf_stem`，或 `supports_big_dripleaf`。
  * `BigDripleafStemBlock`：下面同类或 `supports_big_dripleaf`，**并且**上面同类或
    `big_dripleaf`。
* `waterlogged_vegetation_patch`（`clay_pool_with_dripleaves`）重新接进
  `composition::place_feature`。

对拍（`--through features --steps 9`，繁茂洞穴 `--chunk 0 52`）：

| 集合 | 参数 | 结果 |
|---|---|---|
| `lush_caves_clay`（`waterlogged_vegetation_patch` + `dripleaf`） | `--chunk 0 52 --scan 5 --seeds 1` | 0 不一致 / 0 skipped（9 中心 / 7962624 方块） |
| `lush_caves_vegetation` | `--chunk -1 52 --seeds 1` | 0 不一致 |
| `lush_caves_ceiling_vegetation` | `--chunk 0 52 --scan 25 --seeds 1` | 0 不一致（25 中心 / 22118400 方块） |
| 三者一起 | `--chunk 0 52 --scan 25 --seeds 1` | 0 不一致，**2 skipped**（`treeified` 标记生效） |

#### `lush_caves_clay` 为什么在野外几乎不触发（第十三/十四轮诊断）

`lush_caves_clay` 是 `waterlogged_vegetation_patch`（`clay_pool_with_dripleaves`）唯一的入口，
只属于 `minecraft:lush_caves` 群系（`features_per_step[9]` 的全局 index 32）。它长期对拍
0 不一致但改坏也不报，第十三轮加了逐修饰器的计数诊断，结论如下：

```
modifier Count(Constant(62))     -> 62 out
modifier InSquare                ->  1 out   (×62，每次 in_square 只产 1 个)
modifier HeightRange(uniform)    ->  1 out   (×62)
modifier EnvironmentScan{down}   ->  0 out   (×60) / 1 out (×2)
modifier Offset{y:+1}            ->  1 out   (×2)
modifier Biome                   ->  0 out   (×2)   ← 全部被 BiomeFilter 杀掉
```

再给 `Biome` 加诊断，两个候选的拒绝原因都是：

```
biome filter rejected at (3, 80, 844): biome=29 placed=87
biome filter rejected at (4, 88, 842): biome=29 placed=87
```

`biome=29` 是 **`minecraft:jungle`**，而 `lush_caves` 的注册表 ID 是 **31**。

原因是 `BiomeManager.getBiome`（官方 `BiomeFilter` 走的就是它）会按八角距离做平滑，
而这两个候选的 y 是 **80 / 88** —— `environment_scan` 从 `height_range` 的随机 y 向下
找到的第一块 solid 在地表附近，那里的平滑群系是 `jungle` 而不是地下的 `lush_caves`。
**所以 BiomeFilter 杀掉它们是正确的官方行为**，`lush_caves_clay` 只在「扫描落点恰好
在 `lush_caves` 主导的深度」时才真正跑，野外极少。

结论：`waterlogged_vegetation_patch` / `SmallDripleafBlock` 的 `placeAt` 路径**不是实现有问题**，
而是这个 placed feature 本身在地表附近几乎不触发。要拿到逐方块对拍证据，需要专门找
「`environment_scan` 停在深处」的区块，或在集成测试里直接构造（`waterlogged_patch_lays_ground_and_turns_deep_cells_into_water`
已经这么做了）。

#### 第五轮：`root_system`（分支 `cb-wg-aquatic`）

提交：`02887dfa`（`WIP:` 开头）。

* `feature/root_system.rs`：`RootSystemFeature` 全体 —— `placeDirtAndTree`（向上逐格找能
  放树的位置、`spaceForTree` 的竖直空间与四角检查、下面不能是岩浆且必须 `isSolid`）、
  `placeDirt` + `placeRootedDirt`（每层 `root_placement_attempts` 次随机水平偏移，
  只在 `root_replaceable` 里替换）、`placeRoots`（悬挂根，要求空气 + `canSurvive` +
  上面 `isFaceSturdy(DOWN)`）。
* 接进 `composition::place_feature`（需要注册表和 `PlacementEnv` 递归调 `treeFeature.place`）。
* `hanging_root_state_provider` / `root_state_provider` 走 `StateProvider::parse_with`。

对拍：`--steps 1,9 --feature-types minecraft:rooted_azalea_tree,minecraft:rooted_sulfur_spring
--seeds 3 --chunks 20 --salt 7` → 60 中心 / 53084160 方块 / 0 skipped / 0 不一致；
`--chunk -300 -275 --scan 5 --seeds 1`（繁茂洞穴）→ 9 中心 / 7962624 方块 / 0 不一致。
**但两次「故意改坏」都仍是 0 不一致**，说明这两组参数下 `rooted_azalea_tree` 实际上
一次都没放成（`allowed_tree_position` 或 `root_replaceable` 没匹配上），
所以这一项应视为**已写未验证**，不是「已验证」。

#### 已知限制（第四轮更新）

* **`moss_patch_ceiling` 还剩 4 处 `cave_vines` 错位**：`chunkgen --through features
  --steps 9 --feature-types minecraft:lush_caves_ceiling_vegetation --chunk -58 -217
  --seeds 2` 报 14 处 = 10 处 terrain + 4 处地物。官方在 `(13,-27..-24,6)` 竖着放 4 格
  `cave_vines`，Bamboo 放到邻块 `(1,0)` 的 `(0,-29..-26,1)`。surface 迭代序（`treeify`）
  已修正，但仍选到不同列，说明 **surface 集合内容不同或 `nextFloat` 次数不同**。未定位。
* **`pale_moss_patch` 还剩 2 处**：`chunk (529509,523106)` 的 `(12,135,10)` /
  `(12,136,10)`。诊断结果：Bamboo 的 `place_at` 放下了 base `35584`（与官方一致）和
  topper `35662`（`{east:low,bottom:false}`），随后「再更新一次下层」把 base 的 east
  从 `low` 升成 `tall`（因为看到上层同类地毯 east 非 none）。官方没放 topper，所以 base
  保持 `low`。**根因在 `topper_with_side_chance` 的返回**，未定位。
  已确认方向正确：把 `region_random` 换成地物 `random` 会退化成 15 处。
* `clamped_normal` 只有单测，没对拍（用它的两个 placed feature 属洞穴/冰雪分支）。
* `count_on_every_layer` 只有单测，没对拍（只被下界类型用，主世界不出现）。

#### 蘑菇的亮度判据（已查清并落地）

`WorldGenRegion` **没有覆写** `getRawBrightness`，它转发到 `level.getLightEngine()`
（`BlockAndLightGetter` 的默认实现）。`LevelLightEngine.getRawBrightness` 取
`max(blockLight, skyLight - skyDampen)`。`ChunkStatus.FEATURES` 在 `INITIALIZE_LIGHT`
**之前**，区块还没有任何光照 DataLayer：

* `SkyLightSectionStorage.getLightValue`：`topSections` 的默认值是
  `currentLowestY`（构造时 `Integer.MAX_VALUE`，只有 `onNodeAdded` 才下调），
  该列没有数据时走 `topSection == currentLowestY` 分支返回 **15**。
* `BlockLightSectionStorage.getLightValue`：没有 DataLayer 时返回 **0**。

所以 FEATURES 阶段 `getRawBrightness(pos, 0)` **恒为 15**，`< 13` 永不成立。蘑菇的
`canSurvive` 因此只剩「下面在 `overrides_mushroom_light_requirement` 里」（
`cherry_leaves` / `coarse_dirt` / `note_block`）。这一条已按此实现并写进
`survival.rs` 的注释。

**未验证的部分**：`mushroom_island_vegetation`（蘑菇岛群系，`huge_red/brown_mushroom`
巨型蘑菇）和 `brown_mushroom_normal` 等 placed feature 没有拿到 0 不一致对拍 ——
前者依赖 `huge_*_mushroom`（属 `cb-wg-trees`），后者需要专门的群系覆盖。所以「亮度
恒为 15」这个结论**目前只有源码推导，没有逐方块对拍证据**。

#### 本分支还没做（诚实标 `unsupported`）

* 珊瑚族：`random_block` 状态提供者已在本轮（`3b8d9d6`）移植并通过单测，
  但 `warm_ocean_vegetation` 仍依赖 `overlay`（地物 A 的类型）和嵌套
  `simple_random_selector`，所以 step 9 仍标 skipped。
* `root_system`（`rooted_azalea_tree`、`rooted_sulfur_spring`）：需要 `TreeFeature`，
  属 `cb-wg-trees`。
* `block_pile`、`huge_fungus`、`chorus_plant`：只被组合类内联或只在下界/末地。
* `noise_threshold_count` / `count_on_every_layer` / `surface_water_depth_filter`
  修饰器：第三轮（`8d12409d`）已全部移植。
  `noise_threshold_count` 用在 step 9 的 6 个植被 placed feature 上并对拍 0 不一致；
  `count_on_every_layer` 只被下界类型（`crimson_fungi` 等）用，主世界不出现；
  `surface_water_depth_filter` 由 main 移植。
* `seagrass` / `sea_pickle` / `lily_pad` / `spore_blossom` / `mushroom` 的
  `canSurvive`：全部已移植。`mushroom` 的亮度判据见上文「蘑菇的亮度判据」。
  （`supports_lily_pad` 流体/方块标签 + 上方空流体）、`SporeBlossomBlock`
  （上方 `canSupportCenter(DOWN)` 且本格非水））；只有蘑菇仍不支持，因为它要
  `level.getRawBrightness(pos, 0) < 13`，而 `FeatureWorld` 没有光照数据。
* `TallSeagrassBlock` 的 `DoublePlantBlock.placeAt`：`is_double_plant` 现在也认
  `TallSeagrassBlock`（官方是 `instanceof DoublePlantBlock`），否则上下两格只会放
  下半格、上半格留成水。
* holder 字符串引用的 `to_place`（`cave_vines_body` / `cave_vines_head` 等）：
  `block_state_provider` 注册表已在第二轮（`3b8d9d6`）导出，`cave_vines` 现在真跑通
  （0 不一致、0 skipped）。

### 还没做

* `DiskFeature` / `UnderwaterMagmaFeature` / `surface_relative_threshold_filter`：
  step 6 里非 ore 的几类 placed feature 目前标 `unsupported`，完整 11 步对拍会跳过
  这些区块。（`block_predicate_filter` 修饰器本身已由 `cb-wg-feat-b` 移植。）
* 其它 10 个 step（湖、晶洞、怪物房、积雪等）里仍未移植的类型全部标 `unsupported`；
  植被类的 step 9 进度见上。
* `skipped` 白名单目前只在 Rust 侧用 `unsupported` 标记；Java 侧还没按
  「已移植集合」输出 `skipped`，`--feature-types` 是手工过滤。
* 接进 Director 的**多阶段流水线形状**已经就位（流水线分支：`PipelineKind::Staged`
  按 `ChunkPyramid` 派 `structure_starts … full` 十个阶段、terrain 已接真生成）。
  **第二轮（本轮）已把 features 接进游戏**：`Task::Generate.neighbors` 按 `ChunkStep`
  半径带邻居引用，世界线程存不可变 `Arc` CARVERS 快照（`director_host.rs::partials`），
  计算池跑 `Generator::features()`，写邻居增量按 `TaskId` 追加、完工
  `assemble_feature_column` 排规范序落盘；中心还会纯函数重算 8 个邻居中心溢进来的写，
  保证集合与邻居任务落地时刻无关。见 `docs/worldgen.md`「第二轮」、`mc-bamboo-game/src/worldgen_features.rs`。
  `gen-world --through features`（Java `--ore-only`）0 不一致、1/4 线程逐字节相同。
  **还没做**：其余地物类型（A/B 在做）、`initialize_light` / `light` 的真实计算
  （`light` 已带邻居引用）、`spawn` 刷怪。
* 高度图的增量更新：矿石走 `BulkSectionAccess` 直接写 `LevelChunkSection`，
  **绕过** `ChunkAccess.setBlockState`，所以 step 6 期间高度图是 primed 后固定的。
  `cb-wg-feat-b` 已为走 `setBlockState` 的 `simple_block` 补上 `Heightmap.update`；
  接树 / 湖时复用同一个 `set_block_with_heightmaps` 即可。

## 5.2 `features_per_step` 全部 placed feature 的分类（非植被 / 植被）

分工按「placed feature 的配置类型」（`Feature.DIRECT_CODEC` 的类型 ID）划界：`cb-wg-feat-a`
做非植被，`cb-wg-feat-b` 做植被。下表把导出的 `features_per_step`（11 步、共 167 个
placed feature，主世界 67 个群系的全局有序并集）按类型逐类列出。`ore`（step 6 的 29 个 +
step 7 的 1 个）是流水线里程碑已完成的，本轮不再改，列出来只是为了让表完整。

判据：树、花草、蘑菇、patch、海草/海带/珊瑚、竹、甘蔗、仙人掌、藤蔓、地衣、发光
地衣、孢子花等，以及只包着这些的 selector / sequence 容器归植被；湖、晶洞、冰山、森林
岩石、滴水石、矿石、disk、地下水岩浆、怪物房、化石、泉水、积雪结冰、地下装饰里的
sculk/硫磺/滴水石簇归非植被。**拿不准的容器类型（`sequence`、`simple_random_selector`、
`multiface_growth`、`root_system`、`random_boolean_selector`）一律归植被**，在汇报里列出。

### 非植被（`cb-wg-feat-a`）

| step | 类型 ID | placed feature |
|---:|---|---|
| 1 | `minecraft:lake` | `lake_lava_underground`, `lake_lava_surface` |
| 2 | `minecraft:geode` | `amethyst_geode` |
| 2 | `minecraft:iceberg` | `iceberg_packed`, `iceberg_blue` |
| 2 | `minecraft:large_dripstone` | `large_dripstone` |
| 2 | `minecraft:block_blob` | `forest_rock` |
| 3 | `minecraft:monster_room` | `monster_room`, `monster_room_deep` |
| 3 | `minecraft:fossil` | `fossil_upper`, `fossil_lower` |
| 4 | `minecraft:spike` | `ice_spike` |
| 4 | `minecraft:disk` | `ice_patch` |
| 4 | `minecraft:overlay` | `desert_well` |
| 4 | `minecraft:blue_ice` | `blue_ice` |
| 6 | `minecraft:ore` | 29 个 `ore_*`（已完成） |
| 6 | `minecraft:disk` | `disk_grass`, `disk_sand`, `disk_clay`, `disk_gravel` |
| 6 | `minecraft:underwater_magma` | `underwater_magma` |
| 7 | `minecraft:ore` | `ore_infested`（已完成） |
| 7 | `minecraft:speleothem_cluster` | `sulfur_spike_cluster`, `dripstone_cluster` |
| 8 | `minecraft:spring_feature` | `spring_water`, `spring_lava`, `spring_lava_frozen` |
| 10 | `minecraft:freeze_top_layer` | `freeze_top_layer` |

step 6 的修饰器 `surface_relative_threshold_filter`（3 处）、`block_predicate_filter`（155 处
中的 disk 几处）、以及 disk 用到的 `rule_based` / `simple` 状态提供者、`matching_blocks` /
`matching_fluids` / `solid` / `any_of` / `not` 谓词也归 `cb-wg-feat-a`（它们是放置/配置的
公共构件，不是某一类地物）。

### 植被（`cb-wg-feat-b`）

| step | 类型 ID | 个数 | 代表 placed feature |
|---:|---|---:|---|
| 1 | `minecraft:root_system` | 1 | `rooted_sulfur_spring`（拿不准，归植被） |
| 1 | `minecraft:sequence` | 1 | `sulfur_pool`（拿不准，归植被） |
| 7 | `minecraft:multiface_growth` | 1 | `sculk_vein`（拿不准，归植被） |
| 7 | `minecraft:sequence` | 1 | `sculk_patch_deep_dark`（拿不准，归植被） |
| 7 | `minecraft:simple_random_selector` | 2 | `sulfur_spike`, `pointed_dripstone`（拿不准，归植被） |
| 9 | `minecraft:tree` | 2 | `trees_swamp`, `trees_cherry` |
| 9 | `minecraft:random_selector` | 23 | `trees_plains`, `trees_savanna`, … |
| 9 | `minecraft:weighted_random_selector` | 9 | `seagrass_river`, `trees_dappled_forest`, … |
| 9 | `minecraft:simple_random_selector` | 3 | `flower_forest_flowers`, `warm_ocean_vegetation`, … |
| 9 | `minecraft:random_boolean_selector` | 2 | `lush_caves_clay`, `mushroom_island_vegetation` |
| 9 | `minecraft:simple_block` | 52 | `flower_plains`, `patch_grass_normal`, 蘑菇、孢子和花等 |
| 9 | `minecraft:bamboo` | 2 | `bamboo`, `bamboo_light` |
| 9 | `minecraft:block_column` | 9 | `cave_vines`, `patch_sugar_cane`, `patch_cactus`, `kelp_*` |
| 9 | `minecraft:vegetation_patch` | 3 | `pale_moss_patch`, `lush_caves_vegetation`, … |
| 9 | `minecraft:vines` | 2 | `classic_vines_cave_feature`, `vines` |
| 9 | `minecraft:multiface_growth` | 1 | `glow_lichen` |
| 9 | `minecraft:root_system` | 1 | `rooted_azalea_tree` |

`placed_feature` 注册表里还有一批只在下界/末地群系出现的类型（`end_island`、`end_spike`、
`chorus_plant`、`huge_fungus`、`delta_feature`、`netherrack_replace_blobs`、`void_start_platform`
等），它们**不在**主世界 `features_per_step` 里，本轮不做。

## 5.3 四个分支合并后的现状（`cb-wg-integrate`）

`cb-wg-aquatic`、`cb-wg-caves`、`cb-wg-feat-a`、`cb-wg-structures` 四个分支按顺序合入
`cb-wg-integrate`。合并后 `feature/dispatch.rs` 的叶子、`placement.rs` 的修饰器、
`provider.rs` 的 `IntProvider`、`state_provider.rs` 的提供者都是两边的并集，按类型 ID
字母序排列。

**同名实现只留一份**：

| 名字 | 保留 | 原因 |
|---|---|---|
| `weighted_list` IntProvider | `provider.rs` 的 `Box<IntProvider>` 版 | main 上已有，且能递归任意子 provider |
| `environment_scan` 修饰器 | `placement.rs` 的 `direction: i32` 版（`up=+1` / `down=-1`） | `cb-wg-aquatic` 引入，`cave_vines` / `lush_caves_*` 已对拍 0 不一致 |
| `surface_relative_threshold_filter` | `placement.rs` 的 `SurfaceRelativeThreshold`（`i32` 边界 + `i64` 运算） | 两个分支各写了一份（一份 `i32`、一份 `i64` 字段）；官方的 `min_inclusive` / `max_inclusive` 是 `Codec.INT`，`i32` 更贴近，加法仍按官方用 long |
| `multiface_growth` | `feature/multiface.rs` | 只有 `cb-wg-aquatic` 有实现 |
| `clamped_normal` | `provider.rs` 的 `IntProvider::ClampedNormal` + `dripstone_utils::FloatProvider::ClampedNormal` | 两者分别服务 int 与 float 的官方类型（`ClampedNormalInt` / `ClampedNormalFloat`），不是重复 |
| `FloatProvider`（float 版） | `dripstone_utils.rs` 那份，`provider.rs` 里的删掉 | 两处同名；`dripstone_utils` 版有 `Constant` 变体，裸浮点**不消费**随机数（官方 `ConstantFloat.sample` 不取数），更贴近官方 |
| `random_chance` 修饰器 | `placement.rs`（只有 `cb-wg-caves` 有） | — |
| `java_set`（`HashSet<BlockPos>` 迭代序） | `feature/java_set.rs` | 只有 `cb-wg-aquatic` 有 |

**状态**（详细数字见 `reports/cb-wg-integrate.md`）：

* 已验证 0 不一致并且仍接入 `dispatch`：`bamboo`、`block_column`、`block_blob`、
  `blue_ice`、`disk`、`fallen_tree`、`geode`、`huge_*_mushroom`、`iceberg`、`lake`、
  `large_dripstone`、`monster_room`、`multiface_growth`、`ore`、`simple_block`、
  `speleothem`、`speleothem_cluster`、`spike`、`spring_feature`、`tree`、
  `underwater_magma`、`vegetation_patch`（floor / ceiling）、`vines`。
* 已写但**不接入**（`parse_leaf` 返回 `None`，整块标 `unsupported`）：`sculk_patch`
  （`deep_dark` 单区块 8 处不一致，未收敛）、`waterlogged_vegetation_patch`
  （`clay_pool_with_dripleaves` 完全没对拍）。
* 已知未解决的差异：`moss_patch_ceiling` 的 10 处 `cave_vines` 错位（根因是
  `JavaPosSet` 没实现 `java.util.HashMap` 的红黑树化）。
* 没做：`fossil`、`overlay`（沙漠井，要 `template` 地物）、`root_system`、
  `huge_fungus`、`chorus_plant`、`block_pile`、`noise_threshold_count` /
  `count_on_every_layer` 修饰器。

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

## 树（2026-11-14，分支 `cb-wg-trees`）

`feature/tree/` 照官方 `TreeFeature.place` 的流程移植：取树高、树叶高度和半径，检查高度
范围和可用空间，放树根，放树干得到树叶挂点，每个挂点放树叶，跑装饰器，`updateLeaves`
写树叶的 `distance`，最后 `updateShapeAtEdge`。

| 文件 | 内容 |
|---|---|
| `mod.rs` | `TreeFeature.place` / `doPlace` / `getMaxFreeTreeHeight`，四个位置集合 |
| `java_set.rs` | 复现 Java `HashSet<BlockPos>` 的迭代顺序 |
| `config.rs` | 配置解析、`FeatureSize`、`below_trunk_provider` |
| `trunk.rs` | 十种 `TrunkPlacer` |
| `foliage.rs` | 十二种 `FoliagePlacer` |
| `root.rs` | `MangroveRootPlacer` |
| `decorator.rs` | 十种 `TreeDecorator`（没有 `pale_moss`） |
| `leaves.rs` | `updateLeaves` |
| `fallen.rs` | `FallenTreeFeature` |
| `../huge_mushroom.rs` | 两种巨型蘑菇 |
| `../shape_update.rs` | `Level` 适配层和 `StructureTemplate.updateShapeAtEdge` |

两处容易漏的官方行为：

* `updateLeaves` 里同一片树叶可能同时待在两个距离桶里，从更远的桶再被取出时 `distance`
  会被改写成更大的值。结果取决于 `HashSet` 的迭代顺序，这里照原样重放，不做修正。
* `trunkSetter` 把树干正下方那块泥土也记进了原木集合，装饰器的 `logs.getFirst()` 取到
  的是它。

对拍数字和已知限制见 README 的"树"一条。验证时随机抽样碰不到的群系要定点比：用
`W:\data\bamboo-agents\tools\tmp_find.rs` 这类临时测试按群系找区块，再用
`chunkgen --chunk X Z --scan 3`。
