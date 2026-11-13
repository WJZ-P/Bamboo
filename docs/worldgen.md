# 世界生成：terrain 状态

目标版本 Minecraft Java 26.3（协议 777）。这一篇讲 **从种子到 `terrain` 状态跑完那一刻** 的纯
函数和它的区块级对拍：群系（`BIOMES`）、噪声填充（`NOISE`）、地表规则（`SURFACE`）、雕刻器
（`CARVERS`）。地物、结构、光照和接进游戏还没做。

## 官方 26.3 的阶段

`ChunkStatus` 的列表是 `empty`、`structure_starts`、`structure_references`、`biomes`、`terrain`、
`features`、`initialize_light`、`light`、`spawn`、`full`。**26.3 没有独立的 `NOISE` / `SURFACE` /
`CARVERS` 状态**：`terrain` 一步（`ChunkStatusTasks.buildTerrain`）在**同一个** `NoiseChunk` 上依次
跑三件事：

1. `NoiseBasedChunkGenerator.doFill`：最终密度整块成批求值，含水层把每个方块换成石头、水、
   岩浆或空气；
2. `MaterialSystem.buildSurface`：地表规则（矿脉 `OreVeinRule` 在这里，不在噪声填充里）；
3. `generateCarvers`：雕刻器。

所以对拍里说的 NOISE / SURFACE / CARVERS 阶段，准确说是这三步各自跑完那一刻的
`ProtoChunk`，不是 `ChunkStatus`。Java 侧用反射调私有的 `doFill`、`buildSurface`、
`generateCarvers`，不启动服务端、不读存档、不接受 EULA；一个 `ProtoChunk` 和一个 `NoiseChunk`
贯穿三步（地表和雕刻器要复用 `doFill` 留下的 `cachingSamplers` 和 `Aquifer`）。`NoiseChunk`
走公开构造器，参数和 `buildTerrain` 一致：

* `Beardifier.EMPTY`：新区块没有结构，`forStructuresInChunk` 的 affectedBox 为 null，整块填 0；
* `Blender.empty()`：新世界周围没有老区块，`BlendingData` 为 null；`decorateBiomeResolver` 里
  `blendBiome` 的空表返回 null，`BelowZeroRetrogen` 只对升级中的区块起作用，所以两步都等价于
  原样；
* `globalFluidPicker`：从 generator 的私有字段取，和 `createNoiseChunk` 用的是同一个；
* `DensityVolume(16, height, 16, minBlockX, minY, minBlockZ)`，步长都是 1；
* `BiomeManager(createUncachedResolver(state), obfuscateSeed(seed))`，和 `WorldGenRegion`
  构造里一样；`WorldGenRegion` 传 null，让 `generateCarvers` 走
  `getBiomeGenerationSettingsForCarver` 分支，不需要伪造 `ServerLevel`；
* `possibleBiomes` 传注册表里的全部群系。正式接进游戏时要换成周围 3×3 区块群系调色板的
  并集（`ChunkStatusTasks.collectPossibleBiomes`），逐格判定仍走 `BiomeManager`，但那一层
  静态优化会影响 `BiomeCondition.compile`，到时要再对拍一次。

主世界用官方默认的 `minecraft:overworld` 设置和 `minecraft:overworld` 群系预设。
**下界、末地这一步没做**：它们没有 `aquifers`，`createDisabled` 的含水层是另一条简单的路径，
地表规则也不同，要单独移植和对拍。

## 纯函数与求值路径

`mc-bamboo-worldgen` 暴露 `chunk::Generator`（每个「种子 + 设置」建一次）：

```rust
let worldgen = mc_bamboo_worldgen::Worldgen::load()?;
let generator = mc_bamboo_worldgen::chunk::Generator::new(&worldgen, seed)?;
let biomes: Vec<Vec<u32>> = generator.biomes(chunk_x, chunk_z)?; // 24 段 × 64 个 quart 格
let blocks: Vec<Vec<u32>> = generator.noise(chunk_x, chunk_z)?;  // NOISE 阶段：384 层 × 256 个状态
let surface = generator.terrain(chunk_x, chunk_z, Stage::Surface)?;
let carvers = generator.terrain(chunk_x, chunk_z, Stage::Carvers)?;
```

入口都是纯函数，不碰世界状态。`Generator` 内部用一个 `Compiler` 编译八个路由采样器
（`NoiseRouter`）、含水层的六条密度函数、矿脉的六条密度函数和规则树引用的噪声，和官方一个
`RandomState` 共用一个 `DensityFunctionCompiler` 一样；噪声实例和 `cache` 编号因此也共用。
`terrain` 一次跑完 `doFill → buildSurface → generateCarvers`，只在指定阶段返回方块快照；
三个步骤共用同一个 `Context` 和 `Aquifer`，和官方一个 `NoiseChunk` 一样，不能重算。

### 两条求值路径

官方每个 `DensitySampler` 都有 `sampleValue`（单点）和 `sampleVolume`（成批），两条的浮点运算
顺序不同，最后一位会差。`sampler.rs` 两条都实现：

* `sample_value`：`sampleBlockValueUncached` 用的那条，对拍单点题和 `worldgen` 子命令走它；
* `sample_volume`：真正的一块 `Volume`（`densityfunction/DensityVolume` 的移植），按官方的
  `z → x → y` 顺序填缓冲。

**`BIOMES`**：`doCreateBiomes` 用开了缓存的 `SamplerContext` 建 `Climate.Sampler`；
`MultiNoiseBiomeSource.createResolverForChunk` 对六个气候函数在 quart 体积
`(4, height/4, 4)`、步长 `(4,4,4)` 上各做一次 `sampleVolume`，再 `Climate.target` 量化成定点数查
R 树。填进区块时 `section` 从下到上、段内 `x → y → z`，和
`LevelChunkSection.fillBiomesFromNoise` 一样。R 树的 `lastResult` 是每线程一格 `Cell`：平局时选
哪条依赖查询顺序，所以顺序也照搬。

**`NOISE`**：`doFill` 先对整块 `16 × height × 16` 体积做**一次** `finalDensity.sampleVolume`，
然后 `z → x → y`（y 从顶到底）逐点问 `Aquifer.computeSubstance`。含水层的六条密度函数走
`sampleValue`（单点），唯一一次成批是构造器里 `maxSurfaceLevel` 对 `surfaceLevel` 的 quart 网格
扫描；那次扫描会留在 `cache` 记忆里，顺序不能省。

`SamplerContext` 的缓存语义是逐位一致的关键：`CacheCell` 记一个单点键/值和**最后一块成批体积**
的缓冲；单点求值没命中键时，若坐标落在那块体积的格点上就直接读缓冲。所以「上一次成批算过哪
块体积」会影响之后的单点求值。`Context::uncached()` 对应官方 `EMPTY_UNCACHED`（`worldgen`
对拍），`Context::new(n)` 对应 `enableCaches()`（区块生成，每步一个新上下文，不跨步）。

编译前官方跑 `DfRewriteRule.SLICE_UNIFORM_AXES`：把用不到某个轴的子树包进 `slice`。这一步不改
单点的值，但会改变成批求值时体积的形状，进而改变 `cache` 记住的体积，所以区块生成必须照做。
`interpolated` 的 `sampleVolume` 在格角体积上成批求输入，`fillCell` 的 xz 方向用 `Mth.lerp`、
y 方向是起点加逐格累加的 `delta`（`fadd` 顺序），和单点 `lerp3` 不是同一串运算。

### 地表规则与雕刻器

数据导出扩了四个顶层键：`material_rule`、`material_condition`、`carver`、`biome_carvers`
（每个群系的 carver ID 列表）。`noise_settings` 里的 `material_rule` 只是名字引用；
`Noises.*` 九条内置噪声的参数本来就在 `worldgen/noise` 里，用 `ResourceKey.identifier()`
（`minecraft:surface` 等）从 `RandomState.random` 实例化。

`material.rs` 按官方 `compile` 顺序展开规则树：两条矿脉的 density/richness 在
`expectedVolume` 上成批求值，fillerGap 逐点求值；`preliminarySurfaceFunction` 是
`NoiseRouter.chunkSurfaceLevel`，在 `(16,1,16)` y=0 的体积上成批求值，按
`floor(f) + surfaceDepth - 8` 给 `above_preliminary_surface` 条件用。`BiomeManager` 的
`getBiome` 是 8 角 fiddled distance（SHA-256 混淆种子 + 线性同余），`coldEnoughToSnow`
复用 `biome_temperature.rs`。`getSurfaceDepth` 用 `surface` 噪声、`clay_bands` 色带和两个
特殊扩展（恶地石柱、冻洋冰山）都在 `MaterialSystem` 里，逐列从 `WORLD_SURFACE_WG + 1`
向 minY 扫。

`carver.rs` 按 `generateCarvers` 的硬编码 17×17 邻域循环（`(-8..=8)^2`，不是 `getRange()`
的 4），每 (邻域区块, carver 序号) 调 `setLargeFeatureSeed(worldSeed + index, nx, nz)`，
`isStartChunk` 命中后生成洞穴/峡谷隧道；隧道内部用 `SingleThreadedRandomSource`，和
`LegacyRandomSource` 取数序列一致。`CarvingMask` 的位序是
`(y - minY) + (z + x*16) * height`，`carveEllipsoid` 的 y 循环下界是**开的**
（`y > y0`）。`applyCarvingMask` 从高到低用同一个 `Aquifer.computeSubstance(x,y,z,0.0)`
换方块，草/菌丝下方的泥土用 `MaterialSystem.topMaterial` 在 1×1×1 体积上重跑规则树补回。

### 含水层与随机数

`Aquifer.NoiseBasedAquifer` 的移植在 `aquifer.rs`。随机数用
`RandomState.getOrCreateRandomFactory("minecraft:aquifer")`：世界种子的 `forkPositional()` 再
`fromHashOf(Identifier)`（即 `minecraft:aquifer` 的 MD5 种子）再 `forkPositional()`，
`at(gridX, gridY, gridZ)` 之后按 `nextInt(10)`、`nextInt(9)`、`nextInt(10)` 决定 12 个候选格子的
随机位置。这个顺序、以及等距时的插入顺序，都会改变选出的四个最近格子。

几个容易搞错、已按 26.3 字节码核对的点：

* 候选格子是 `x: 0..1`、`y: -1..1`、`z: 0..1` 共 **12** 个，不是 3×3×3；
* `calculatePressure` 里 `scaled` 超出 `[-2, 2]` 时官方只是把 barrier 噪声当 0，压力仍是
  `2 * scaled`，**不是**把压力归零；
* `Mth.map` / `Mth.clampedMap` 的参数方向、`similarity(a, b) = 1 - (b-a)/25`、
  `Mth.quantize(…, 3)`、`ChunkPos.pack(x, z) = x | z << 32`；
* `FluidStatus.at(y)` 在 `y >= level` 时给空气，`FluidStatus.equals` 比较液面和流体类型。

`shouldScheduleFluidUpdate` 只影响官方的后期处理位置表，对拍不比它，但计算照搬，以后接游戏时
要用。

## 区块级对拍

`tools/mc-bamboo-vanilla` 的 `chunkgen` 子命令：

```bash
cargo run --release -p mc-bamboo-vanilla -- chunkgen java \
    [--seeds N] [--chunks N] [--salt S] [--through biomes|noise|surface|carvers] \
    [--biomes-only] [--show N]
```

`ChunkgenOracle.java` 按官方 `WorldLoader` 的前半段读数据包注册表，对每个（种子，区块）造一个
`ProtoChunk`（`LevelHeightAccessor.create(minY, height)`、`PalettedContainerFactory.create(...)`、
`BlendingData` 为 null），调 `ChunkGenerator.createBiomes(...).join()`，再按 `--through` 依次
反射调 `doFill`、`buildSurface`、`generateCarvers`。输出一行一个 JSON：`header`、每个区块一条
`biomes`（24 段 × 64 个群系 ID）和最多三条方块快照 `noise` / `surface` / `carvers`
（调色板 + 16×height×16 个下标，顺序 y、x、z）、最后 `end`。三步都只放默认状态，Rust 侧按
注册名查回默认状态 ID 逐个比较；群系按注册表 ID 逐个比较。

坐标覆盖原点、八个固定区块（含远处海洋/山地）和按 salt 随机的区块（三档范围，最远 ±100 万），
种子含 0、±1、很大的正负数。两边的版本、协议、设置 ID、`minY`/`height`/`seaLevel` 也会核对。

`worldgen` 子命令（数学层对拍）仍然独立存在，逐值比较噪声、密度函数、路由、群系和温度：

```bash
cargo run --release -p mc-bamboo-vanilla -- worldgen java
```

当前结果（release，本机）：

| 对拍 | 规模 | 不一致 |
|---|---|---|
| `chunkgen --through carvers` | 6 种子 × 36 区块 × 2 个 salt = 432 区块，663552 个 quart 格、每阶段 42,467,328 个方块（三阶段共 127,401,984 次比较） | 0 |
| `chunkgen --through surface` | 同上规模，只比到 SURFACE | 0 |
| `chunkgen --through noise` | 同上规模，只比到 NOISE | 0 |
| `chunkgen --biomes-only` | 8 种子 × 36 区块 = 442368 个 quart 格 | 0 |
| `worldgen` | 3 种子、21 个维度状态、4608 个噪声值、5364 个密度函数值、288 个群系、536 个温度 | 0 |

含 `interpolated` 的函数在 `worldgen` 的单点成批比较里跳过 612 项（成批路径在 1×1×1 上不是纯
函数），但它们的输入成批路径已被单点求值和 `chunkgen` 覆盖。

## 耗时

`cargo run --release -p mc-bamboo-worldgen --example chunk_timing`：每个种子先预热一次，再对
7 个区块、三个阶段各计时取中位数；5 个种子共 35 个区块 × 3 阶段。三个时间都是从头跑到该
阶段的累计耗时，不含群系、不含 JVM、不含对拍 IO、不含建 `Generator`（每个种子只建一次）。

优化前后各连续跑三轮，取中间一轮（同一台 Apple M4 Pro，release）：

| 阶段 | 优化前中位数 | 优化后中位数 | 倍数 |
|---|---|---|---|
| NOISE | 3.39 ms | 3.53 ms | 1.0×（没改这一段） |
| SURFACE | 121.36 ms | 15.99 ms | 7.6× |
| CARVERS | 121.85 ms | 17.47 ms | 7.0× |

```
Noise:   all 35 chunks median   3.53 ms, min  3.25 ms, max   3.87 ms
Surface: all 35 chunks median  15.99 ms, min 11.76 ms, max  20.37 ms
Carvers: all 35 chunks median  17.47 ms, min 12.18 ms, max  21.78 ms
```

CARVERS 只比 SURFACE 多约 1.5 ms：17×17 邻域的大部分 carver 在 `isStartChunk` 就被概率
过滤掉了。三项优化里的两项有可测收益，逐项前后都量过 `chunk_timing` 并跑
`chunkgen --through carvers` 多种子对拍；没有收益的一项（条件结果的惰性缓存，见下）撤掉了。
详见 `performance-optimizations.md` 第 10 节。

### 拆分表：慢在哪

基准是临时插桩（嵌在 `Generator::terrain` 和地表/雕刻器里，量完已还原，没进仓库）：
21 区块 × 3 种子，嵌套计时器报**自身耗时**（扣掉子段），所以带插桩的墙钟 58.7 ms/区块
高于无插桩的 40 ms/区块，各行的绝对数只能横向比，不能和上面的 `chunk_timing` 直接相减。
另用 `sample`（12 秒、1 ms 间隔）采样交叉验证：8 714 个主线程样本里 6 576 个在
`MaterialRules::build_surface`，其中约 5 800 个在 Perlin 噪声、468 个在 R 树查找、
雕刻器隧道只有 6 个。下表是**第一项（气候缓存）之后、第二项之前**的状态。

| 段 | 细项 | 自身 ms/区块 | 调用次数/区块 |
|---|---|---|---|
| `doFill` | `finalDensity.sampleVolume` | 2.88 | 1 |
| `doFill` | 含水层构造 + `computeSubstance` | 0.73 | 1 + 98 304 |
| 地表 | 两条矿脉成批求值 | 0.62 | 3 |
| 地表 | `WORLD_SURFACE_WG` 高度图 | 0.34 | 1 547 |
| 地表 | `updateXZ`（surfaceDepth + 2d 条件噪声） | 0.24 | 258 |
| 地表 | **`updateY`（15 条 3d 条件噪声，全部 eager 求值）** | **21.63** | 29 093 |
| 地表 | 规则树 `apply` 自身 | 15.68 | 292 450 |
| 地表 | 群系查询（气候缓存未命中 + R 树） | 8.89 | 28 572 |
| 地表 | 条件 `test` 自身 | 3.37 | 152 828 |
| 地表 | `preliminarySurface` 成批 + 列循环自身 + 扩展 | 1.63 | — |
| 雕刻器 | 17×17 邻域群系 | 0.82 | 289 |
| 雕刻器 | 隧道 | 0.11 | 67 |
| 雕刻器 | `applyCarvingMask` 自身 + 含水层 + `topMaterial` | 0.17 | 1 + 1 280 + 2 |

优化前的额外热点由官方侧交叉验证：给 `ChunkgenOracle.java` 的
`BiomeResolver` 包一层计时器（临时副本放在 `target/vanilla/`，不进仓库），
1 种子 × 120 区块下官方是 `doFill` 4.84 ms、`buildSurface` 101.1 ms、
`generateCarvers` 1.70 ms、整个 `terrain` 108 ms 每区块；共 31 046 次群系查询/区块、
109 ms/区块（计时器本身有开销），把 resolver 换成固定群系后 `buildSurface` 降到 8.99 ms。
也就是说**官方在这条对拍路径上同样慢在 uncached 群系查询**，不是几毫秒量级；
`-Xcomp` 更慢，GC 日志排除了 GC。Bamboo 优化后的 17 ms 已明显快于它的 108 ms。

### 改了什么、为什么不改结果

1. **按 quart 坐标缓存气候参数**（`biome_manager::ClimateCache`）。地表逐列扫描和
   雕刻器 17×17 邻域会反复问同一批 quart 格，每次都要在一个空的 `SamplerContext`
   上重算六条气候密度函数。这六条是纯函数：不记忆上下文、不取随机数，所以按
   `(quartX, quartY, quartZ)` 记下 `TargetPoint` 即可。**只缓存气候参数，不缓存群系 ID**：
   R 树的 `lastResult` 是平局时的初始候选，跳过一次查表会改变后续查询的结果；
   每次仍照原顺序查表。范围外的坐标走原路径不缓存。
2. **条件噪声 supplier 惰性求值**（`material::Scan::noise_get`）。`javap -c -p` 核对
   `MaterialRuleContext$1`（2d）和 `$2`（3d）的 `getAsDouble`：两者各按
   `lastUpdateXZ` / `lastUpdateY` 代数记住结果，只在代数变了时调 `Noise.get`。
   原实现却在每格把 15 条 3d 噪声全部 eager 求一遍（29 093 次/区块）。改成
   「每条噪声这一代第一次被问到才算」；`updateXZ` 照 bytecode 把两个代数都加一，
   `updateY` 只加 Y。各噪声互不依赖、`Noise.get` 不取随机数，所以求值顺序、浮点
   结果和随机序列都不变。
3. **试过并撤销**：`LazyYCondition` / `LazyXZCondition` 的结果缓存。字节码确认了
   `BiomeCondition` 等编译成 `LazyYCondition`、`HoleCondition`/`SteepCondition` 编译成
   `LazyXZCondition`，按编号 + 代数记忆实现后对拍仍是 0 不一致，但 21 区块基准
   改前 36.9 / 37.5 / 45.8 ms、改后 37.6 / 37.6 / 37.5 ms，`chunk_timing` 也都在
   40 ms 附近，**没有可测收益**，按「没有可测收益的撤掉」撤销。

剩余的大头是规则树 `apply` 自身（15.68 ms/区块）和气候缓存未命中时的 R 树查找，
以及 `finalDensity` 的 2.9 ms；并行化和 SIMD 留到有逐位一致的基线之后再谈。

### 接进游戏时要重新对拍的两处

这一轮用 `javap -c -p` 核对 `ChunkStatusTasks`、`WorldGenRegion`、`ServerLevel`、
`NoiseBasedChunkGenerator` 时确认了两点，对拍目前走的是简化路径：

* `possibleBiomes` 是 `ChunkStatusTasks.collectPossibleBiomes(region, 1)`，即周围
  3×3 区块 `collectBiomesInPalette` 的并集；对拍传的是注册表里全部群系。这只影响
  `BiomeCondition.compile` 的静态优化，逐格判定仍走 `BiomeManager`，但接进游戏时要再
  对拍一次。注意 `WorldGenRegion` 的 `biomeManager` 用的是
  `ServerLevel.uncachedBiomeResolver()`（`createUncachedResolver(randomState)`），
  也就是**真实服务端这一步同样逐点重算气候**，不是从已存 quart 格读群系。
* 雕刻器在真实服务端里 `WorldGenRegion` 不为空，走
  `region.getChunk(x, z).carverBiome(supplier)`，读的是 `BIOMES` 阶段已存的 quart 格；
  对拍传 null，走 `getBiomeGenerationSettingsForCarver` 的 uncached 分支。两条路径的
  群系来源不同，接进游戏时要再对拍一次。

## 还没做

* 地物、结构、光照。
* 下界和末地（无 `aquifers` 的路径、五群系末地源、下界老 fbm）。
* 接进游戏：Director、世界类型、区块流水线、计算池；这一步只有纯函数和对拍。
* `BlendingData` / `Blender` 的非空路径（老存档升级、旧区块混合）。
* `Beardifier` 的非空路径（结构影响密度）。
* 正式流程里的 `possibleBiomes`（周围 3×3 区块调色板的并集）；对拍用「全部群系」。
* 高度图、后期处理位置表和 `ProtoChunk` 的其余字段；对拍目前只比方块和群系。
* 真实流程里的 `possibleBiomes` 和雕刻器的 `carverBiome` 分支（见「耗时」最后一节）。
* 并行化和 SIMD：先有逐位一致的基线再谈优化。
