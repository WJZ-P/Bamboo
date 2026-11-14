# 世界生成：terrain 状态

目标版本 Minecraft Java 26.3（协议 777）。这一篇讲 **从种子到 `terrain` 状态跑完那一刻** 的纯
函数和它的区块级对拍：群系（`BIOMES`）、噪声填充（`NOISE`）、地表规则（`SURFACE`）、雕刻器
（`CARVERS`）。terrain 已经接进 Director 的 `Staged` 流水线（见文末「接进游戏」）；地物、
结构、生成时光照和刷怪还没接完，已知限制见 README。

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

## 数据的导出与重跑

`mc-bamboo-data/src/generated/worldgen.json` 是整份从官方 26.3 导出的，不要手工改。顶端有九个
键：`noise`、`density_function`、`noise_settings` 三个注册表用官方 codec 编回 JSON；
`material_rule`、`material_condition`、`carver` 同理；`biome_carvers` 是每个群系的雕刻器列表，
按注册表名字排序；`multi_noise_presets` 不走 codec，直接导出内存里量化好的定点数（codec 里是
浮点，来回换算会差最低位）；`end_biomes` 是末地源固定引用的五个群系。

`Extractor.java` 把 codec 的结果放进 Gson 的 `JsonElement`，用
`new GsonBuilder().create().toJson(root)` 一次写进 `target/vanilla/extracted.json`。
`JsonPrimitive(double)` 保留 IEEE 754 的 double，Gson 写文本时用 `Double.toString`，也就是
保证能往返的最短十进制表示，中间没有经过 float。Rust 侧的 `extract.rs` 把报告解析成
`serde_json::Value`，`generate.rs::write_worldgen` 再缩进排版写进生成文件，
`mc-bamboo-worldgen` 用同一套 codec 的语义读它。

**两次 JSON 解析都必须按 IEEE 754 正确舍入。** 工作区的 `serde_json` 开了
`float_roundtrip`（在根 `Cargo.toml` 里，Cargo 对整个依赖图合并特性，所以导出工具和运行时
用的是同一个解析器）。默认的快路径用 `significand as f64 * 10^exp`，少数 17 位有效数字的
十进制串会差 1 ulp。26.3 的 `worldgen/noise` 里正好有六个这样的 `base_amplitude`：

| 条目 | 官方运行时 | 位模式 | 快路径读出的错误值 |
|---|---|---|---|
| `minecraft:jagged` | `1.0383104856073737` | `0x3ff09ceb74ac70d7` | `1.0383104856073735` |
| `minecraft:nether/temperature` | `0.9494731054427981` | `0x3fee62156c09de3a` | `0.949473105442798` |
| `minecraft:nether/vegetation` | `0.9494731054427981` | `0x3fee62156c09de3a` | `0.949473105442798` |
| `minecraft:offset` | `0.9381732587751005` | `0x3fee0583ed0d7735` | `0.9381732587751004` |
| `minecraft:pillar` | `0.9494731054427981` | `0x3fee62156c09de3a` | `0.949473105442798` |
| `minecraft:ridge` | `0.9147152149950137` | `0x3fed4558d7b1d486` | `0.9147152149950136` |

位模式来自官方注册表里 `NormalNoise.Parameters.baseAmplitude` 的
`Double.doubleToRawLongBits`，也和官方 jar 的 `data/minecraft/worldgen/noise/*.json` 原文一致。
最早那版生成文件是在打开 `float_roundtrip` 之前导出的，所以留下的是右边那列「差 1 ulp」的值；
后来新增地表和雕刻器四块时是手工并进旧文件的。现在整份重导过，六项都是官方运行时的值，
`worldgen` / `chunkgen` 对拍在新值上仍是 0 不一致（见下面）。

`write_worldgen` 在写文件前把排版后的文本再解析一遍，逐个浮点比较位模式；两边各有单元测试
盯着这六条的位模式（`generate.rs` 的测试和 `mc-bamboo-worldgen/src/lib.rs` 的
`noise_amplitudes_match_the_official_bits`）。以后升级 `serde_json` 或换导出方式，解析器一旦
丢精度，导出和测试会直接失败，不会写出一份看着一样、实际差最低位的数据。

和手工合并的旧文件逐值比较，改动只有上面表格里的六个 double：旧值全都是快路径舍错、
比官方运行时低 1 ulp 的那个版本，新值等于官方内存里的位模式，也等于官方数据包 JSON 的
原文，所以新的才对。另外有 18 处纯排版差异：样条里 `2.9802322e-08` / `5.9604645e-08` 变成
`2.9802322e-8` / `5.9604645e-8`，指数少一个前导零，数值和位模式都没变。其余键、数组顺序、
结构一律不变。

### 怎么重跑

在同一个 jar 上，`extract` 用上次 `generate` 留下的报告重跑 `Extractor.java`（重写
`extracted.json`），再导出全部生成文件；`generate` 会连官方的数据生成器报告一起重跑：

```bash
# 完整重跑；第一次或换了官方 jar 时用这条。
# 报告装在 target/vanilla，换工作树时可以从旧工作树 cp -R 过来。
cargo run -p mc-bamboo-vanilla -- generate /path/to/server-26.3.jar

# 只改了 Rust 侧排版、jar 没换：重跑 Extractor.java 就够，快得多。
cargo run -p mc-bamboo-vanilla -- extract

# 生成文件坏了、工具带默认特性编译不过时，用这个跑 generate / extract。
cargo run -p mc-bamboo-vanilla --no-default-features -- generate /path/to/server-26.3.jar
```

重跑后 `git diff -- crates/mc-bamboo-data/src/generated/` 应当为空：`worldgen.json` 之外，
`saved_world.rs` 也是生成流程写出的（官方 `WorldGenSettings.CODEC` 编出的超平坦
`world_gen_settings.dat` 内容），`mod.rs` 的模块列表也由生成流程重写。`worldgen.json`
写出前会递归按键名排序：默认特性的依赖图统一开了 serde_json 的 `preserve_order`，
不排序就会让两种特性配置写出键序不同的文件。`worldgen.json` 不为空时先查是不是有
新版本的官方数据，再看 `docs/worldgen.md` 里这六个位模式有没有变。
确认数据没问题后跑对拍：

```bash
cargo run --release -p mc-bamboo-vanilla -- worldgen java
cargo run --release -p mc-bamboo-vanilla -- chunkgen java --seeds 6 --chunks 36 --through carvers
cargo run --release -p mc-bamboo-vanilla -- chunkgen java --seeds 6 --chunks 36 --through carvers --salt 1
```

除了 `worldgen.json`，`generate` 还会写 `generated/` 下的其它文件（含 `saved_world.rs`
和 `mod.rs`）；只关心世界生成数据时，别把那些无关的改动混进同一个提交。`extract` 需要
`target/vanilla/generated/reports`，`generate` 会用官方 jar 重建它。

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

### 真实服务端路径与简化路径

`chunkgen`（数学层纯函数对拍）走简化路径：单个 `ProtoChunk`，`possibleBiomes` 传注册表
全部群系，`generateCarvers` 传 `null` region（走 `getBiomeGenerationSettingsForCarver`），
`biomeManager` 用 `createUncachedResolver` 逐点重算气候。`gen-world`（游戏路径对拍）的
Java 端已改成真实服务端路径，用 `ChunkgenOracle.java --real-path`：

* 造 17×17 个 `ProtoChunk`（`generateCarvers` 的邻域是 `(-8..=8)²`），其中 3×3 跑
  `createBiomes` 填好 quart 格；用 `Unsafe.allocateInstance` 造一个不跑构造器的
  `WorldGenRegion`，只填 `getChunk` / `getBiomeManager` 用到的 `cache` / `center` /
  `generatingStep` / `biomeManager` / `uncachedBiomeResolver`（`directDependencies` 照
  `ChunkPyramid` 的 terrain 步 `[BIOMES, BIOMES, STRUCTURE_STARTS×7]`）。不启动服务端、
  不接受 EULA。
* `possibleBiomes` 照 `ChunkStatusTasks.collectPossibleBiomes(region, 1)`：周围 3×3
  区块 `collectBiomesInPalette` 的并集。
* `buildSurface` 用这个集合和 `region.getBiomeManager()`；`generateCarvers` 传非空的
  `carverBiomeRegion`，走 `region.getChunk(x,z).carverBiome(supplier)` 分支。
  `WorldGenRegion` 的 `biomeManager` 是 `new BiomeManager(this, obfuscateSeed(seed))`，
  其 `getBiome` 走 `LevelReader.getNoiseBiome` 默认实现：先读已加载区块的 quart 格，
  读不到才回退 `getUncachedNoiseBiome()`（`ServerLevel.uncachedBiomeResolver()`）。
* terrain 跑完后照 `buildTerrain` 的 `thenApply` 升到 `TERRAIN` 并
  `Heightmap.primeHeightmaps(proto, FINAL_HEIGHTMAPS)`。

`gen-world` 用这份真实路径出题，Bamboo 侧仍调游戏里同一份 `generate_chunk`（简化路径）。
多种子、多区块、多 salt 下逐方块、逐 quart 格 0 不一致——也就是这两条路径在当前数据上
结果相同，所以 Rust 侧没有为它们改 `Generator::terrain`（改了反而会动到 `chunkgen`
纯函数对拍的 `lastResult` 顺序）。

#### 两条路径什么时候会不同

* **`possibleBiomes` 的静态折叠**：`BiomeCondition.compile` 在 `possibleBiomes != null`
  时会把「集合里没有任何该群系」的条件折叠成常量 `false`、「集合里全是该群系」折叠成
  常量 `true`，跳过逐格 `getBiome`。全部群系是最大集，几乎不折叠；3×3 并集较小，可能
  折叠。只要 3×3 并集覆盖当前区块 `getBiome` 能返回的所有群系（8 角 fiddled distance
  只会选到相邻区块的 quart 格），折叠就安全、两条路径相同。**当邻居的 `BIOMES` 阶段
  没先生成、调色板不全，或人为缩小 `collectPossibleBiomes` 半径，使某个实际会出现的
  群系不在集合里**，真实路径会把本该逐点判定的条件错误短路，地表方块可能不同。
  `MaterialSystem.topMaterial` 传 `possibleBiomes=null`，永远不折叠。
* **`getBiome` 的群系来源**：真实路径的 `region.getBiomeManager()` 优先读 `BIOMES` 阶段
  成批 `sampleVolume` 填进 quart 格的群系，简化路径用 uncached `sampleValue` 单点重算。
  两条浮点路径在气候参数落在 R 树最近邻/量化的临界点时可能选到不同群系；若那个群系
  恰好决定地表材质或 `coldEnoughToSnow`，两种路径的地表方块可能不同。本轮对拍没有
  命中这种临界点。`applyCarvingMask` 的 `topMaterial` 用
  `withDifferentSource(createUncachedResolver)`，两条路径在这一步都回到单点 uncached。
* **雕刻器群系**：26.3 的 `carverBiome(supplier)` 的 supplier 仍是
  `getBiomeGenerationSettingsForCarver(createUncachedResolver, sourcePos)`（单点、
  `quartY=0`），只是把结果缓存在 `ChunkAccess.carverBiomeSettings` 上跨中心区块复用，
  所以当前两条路径 carver 列表相同。若 supplier 改成从已存 quart 格（成批量化）读，
  单点与成批的边界差会改变 carver 列表。
* **非空附加状态**：两条路径都是新世界（`Blender.empty`、无 `BlendingData`、
  `Beardifier.EMPTY`、无结构）。老存档升级、结构 Beardifier、`BelowZeroRetrogen`
  这些非空路径会改 `doFill` 密度和地表，简化路径不覆盖。

### 高度图

官方 26.3 的六张 `Heightmap.Types`（`Heightmap.java`）：

| 类型 | `Usage` | 谓词 `isOpaque` |
|---|---|---|
| `WORLD_SURFACE_WG` | `WORLDGEN` | `NOT_AIR` |
| `WORLD_SURFACE` | `CLIENT` | `NOT_AIR` |
| `OCEAN_FLOOR_WG` | `WORLDGEN` | `BLOCKS_MOTION_IN_HEIGHTMAP` |
| `OCEAN_FLOOR` | `LIVE_WORLD` | `BLOCKS_MOTION_IN_HEIGHTMAP` |
| `MOTION_BLOCKING` | `CLIENT` | `BLOCKS_MOTION_IN_HEIGHTMAP` 或非空流体 |
| `MOTION_BLOCKING_NO_LEAVES` | `CLIENT` | `BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES` 或非空流体 |

* `ChunkStatus.WORLDGEN_HEIGHTMAPS = {OCEAN_FLOOR_WG, WORLD_SURFACE_WG}` 是
  `empty..biomes` 阶段后的两张；`FINAL_HEIGHTMAPS = {OCEAN_FLOOR, WORLD_SURFACE,
  MOTION_BLOCKING, MOTION_BLOCKING_NO_LEAVES}` 是 `terrain` 及之后阶段的四张。
* `doFill` 用 `getOrCreateHeightmapUnprimed` 拿两张 WG 图，从高到低放非空气方块时
  `Heightmap.update(x, y, z, state)`（只记录最高的 opaque 方块 `y+1`）；
  `buildSurface` 的 `getSurfaceDepth` 读 `WORLD_SURFACE_WG + 1` 从顶往下扫。
  `applyCarvingMask` 的 `chunk.setBlockState` 在 `getPersistedStatus()=BIOMES` 时继续
  update 这两张 WG 图。
* `ChunkStatusTasks.buildTerrain` 的 `thenApply` 在 terrain 之后
  `Heightmap.primeHeightmaps(generatedChunk, FINAL_HEIGHTMAPS)`：每列从顶往下找第一个
  非空气方块，对每张 remaining 图测谓词，命中记 `y+1`。
* `LevelChunk` 构造只保留 `FINAL_HEIGHTMAPS` 四张（丢掉 WG），
  `SerializableChunkData.write` 把 `chunk.getHeightmaps()` 写进 `Heightmaps` 的
  `LongArrayTag`。所以存档里是四张 FINAL。

Bamboo 侧 `GeneratedChunk.heightmaps` 对六张图都从最终 CARVERS 方块按官方谓词
`primeHeightmaps` 算出（doFill 的两张 WG 经 `setBlockState` 增量维护到 CARVERS，
最终值等于从最终方块 prime，本轮逐列对拍证实）。`build_region_column` 仍让
`Chunk::from_sections` 重算四张 FINAL，`gen-world` 逐列比六张、再比
`build_region_column` 组装出的四张 FINAL 打包位流和官方 `getRawData()`，全 0 不一致。

## 接进游戏（流水线分支）

terrain 的纯函数已经接进游戏（`mc-bamboo-game/src/worldgen.rs` +
`chunk_generator.rs` 的 `OverworldGenerator`）：

* `server.properties` 的 `level-seed` / `level-type` / `generator-settings` /
  `generate-structures` 解析成 `WorldGenConfig`（`world_options.rs`，种子按官方
  `WorldOptions.parseSeed`），新世界默认 `normal`；`World::build` 用
  `PipelineKind::Staged`、把 `WorldState.seed` 设成配置种子、并默认起一个小计算池
  （`chunk_worker_threads` 未配时取核数一半、夹在 1..=4），让几十毫秒的地形生成
  不占住 20 TPS 的世界线程。flat 配置仍走原来的超平坦模板 + 一步流水线，行为不变。
* 十个 `ChunkStatus` 阶段是纯函数 `(阶段, 位置, 配置) → RegionColumn`：
  `structure_starts` / `structure_references` 空实现（结构队员填）；`biomes` 透传；
  `terrain` 一步同时算 quart 群系和 `doFill → buildSurface → generateCarvers` 的方块、
  `Chunk::from_sections` 从方块重算 4 张高度图（等价 `primeHeightmaps`）。把 BIOMES
  折进 terrain 是为了避免两次组装 `RegionColumn`（纯 all-air 的 biomes 盒子在
  `from_sections` 里也要重算 4 张高度图，很慢且马上被 terrain 覆盖）；官方 `terrain`
  里地表/雕刻器本就是 `BiomeManager` uncached 重算群系，这里连 quart 格也一起算。
  `features` / `initialize_light` / `light` / `spawn` / `full` 本轮原样透传。
* 数据交接：`Worldgen` 解析一次后按线程 `Box::leak`，`chunk::Generator` 每条计算线程
  各缓存一份（它的 `ParameterList` 用 `Cell` 记 R 树 `lastResult`，官方是
  `ThreadLocal`，不能跨线程共享）。生成的方块/群系按官方 `PalettedContainer` 下标写进
  24 个 `SectionParts`，组装成 `ChunkColumn` / `RegionColumn`。
* 存档：`data/minecraft/world_gen_settings.dat` 按官方 `WorldGenSettings.CODEC` 写出
  种子与维度生成器（`flat` 写 `minecraft:flat` + 地层，噪声世界写 `minecraft:noise` +
  settings 注册名），读档时以它为准，空 `level-seed` 的随机种子重启后固定。
* 对拍：`mc-bamboo-vanilla gen-world <java>` 用 `ChunkgenOracle.java --through carvers
  --real-path` 按真实服务端路径出题（见「真实服务端路径与简化路径」），Bamboo 侧调
  **游戏里用的同一份** `generate_chunk` 逐 quart 格、逐方块、逐高度图列比较，还比存档
  写出的四张 FINAL 高度图位流。`--break-self-test` 故意改坏方块、高度图列和高层方块，
  确认三个比较器都会失败。两边都停在 CARVERS，地物/结构/光照按 `--steps` 的思路一起
  排除。`gen-world --seeds 3 --chunks 12`（两个 salt 各 36 区块）每轮比 55296 个 quart
  格、3538944 个方块、55296 个高度图列、5328 个高度图 long，全部 `0 mismatches`。

### 各阶段耗时与生成能否跟上飞行

`cargo run --release -p mc-bamboo-worldgen --example chunk_timing` 量的是纯函数
（见「耗时」表）：NOISE 3.53 ms、SURFACE 15.99 ms、CARVERS 17.47 ms，一个区块到
CARVERS 约 37 ms（release，不含建 `Generator`、不含 `ChunkColumn` 组装）。
`gen-world` 的 release 实跑：每区块额外要做 `PalettedContainer` 重打包和
`Chunk::from_sections` 重算 4 张高度图，量到约 40 ms/区块量级。

飞行的压测场景（`fly-20` 之类）本轮**没有实跑**：`PipelineKind::Staged` 的完整依赖图
里 `structure_references` / `biomes` / `terrain` / `features` 要半径 8 的
`structure_starts`，`features` 要半径 1 的 `terrain`，`light` 要半径 1 的
`initialize_light`；一个可见区块会牵出一大圈依赖区块。结构/地物/光照阶段目前是透传
空实现，这些依赖都是廉价的空盒子，但真实地形仍要按 `ChunkStep` 的半径铺开。
计算池默认 4 线程、每线程一个 `Generator`，生成不占世界线程；世界线程只收
`TaskId` 排好序的回执。**结论待量**：换上真实地物/结构后要重跑 `fly-20`，量
玩家飞行时每刻新生成区块数、在途生成数、信箱积压和 tick 起点抖动，再决定线程数。

## 还没做

* 地物（`features` 阶段透传，不放置任何地物；第 6 步矿石的纯函数已有、没接进游戏）、
  结构（两个阶段是空实现）、生成时的 `initialize_light` / `light`（透传，光照由
  **运行时**光照引擎在交付后补算）、`spawn` 阶段刷怪（空实现）。
* 邻居只读快照 `Arc<ProtoChunk>` 与 `FeatureResult.deltas` 按 `TaskId` 顺序合并
  （设计稿 5.3.1 的 M4 剩余部分）：`Task::Generate.neighbors` 目前恒为空，features /
  light 拿不到半径 1 的邻居视图，所以这两块先透传。
* 下界和末地（无 `aquifers` 的路径、五群系末地源、下界老 fbm）。
* `BlendingData` / `Blender` 的非空路径（老存档升级、旧区块混合）。
* `Beardifier` 的非空路径（结构影响密度）。
* `possibleBiomes` / `carverBiome` / `region.getBiomeManager()` 已在 `gen-world` 按真实
  服务端路径重新对拍（见「真实服务端路径与简化路径」），但 Rust 侧 `generate_chunk`
  仍走等价的简化路径：没有把 3×3 调色板并集的 `BiomeCondition.compile` 折叠、读
  quart 格的 `BiomeManager` 真的搬进 `mc-bamboo-worldgen`。两条路径在本轮对拍里
  0 不一致，理论差异见上一节。
* 高度图：六张已从最终 CARVERS 方块按官方谓词算出并逐列对拍 0 不一致，存档写出的
  四张 FINAL 打包位流也和官方逐字对拍 0 不一致（见「高度图」）。但游戏侧
  `build_region_column` 仍让 `Chunk::from_sections` 从方块重算，没有把 doFill 阶段的
  `Heightmap.update` 增量维护真的搬进 Rust；后期处理位置表和 `ProtoChunk` 其余字段
  也还没逐项对拍。
* 非 `full` 的 ProtoChunk 从盘上读回后从它的状态继续生成：流水线按阶段纯函数写好了，
  但读盘路径仍按现有模板/ProtoChunk 逻辑，未针对多阶段重新对拍。
* 并行化和 SIMD：先有逐位一致的基线再谈优化。
