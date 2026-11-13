# 世界生成：噪声阶段

目标版本 Minecraft Java 26.3（协议 777）。这一篇只讲 **从种子到 NOISE 阶段方块** 的纯函数和它的
区块级对拍：群系（`BIOMES`）和噪声填充（本文叫 `NOISE`）两步。地表规则、雕刻器、地物、结构、
光照和接进游戏都还没做。

## 官方 26.3 的阶段

`ChunkStatus` 的列表是 `empty`、`structure_starts`、`structure_references`、`biomes`、`terrain`、
`features`、`initialize_light`、`light`、`spawn`、`full`。**26.3 没有独立的 `NOISE` / `SURFACE` /
`CARVERS` 状态**：`terrain` 一步（`ChunkStatusTasks.buildTerrain`）在**同一个** `NoiseChunk` 上依次
跑三件事：

1. `NoiseBasedChunkGenerator.doFill`：最终密度整块成批求值，含水层把每个方块换成石头、水、
   岩浆或空气；
2. `MaterialSystem.buildSurface`：地表规则（矿脉 `OreVeinRule` 在这里，不在噪声填充里）；
3. `generateCarvers`：雕刻器。

所以对拍里说的 NOISE 阶段，准确说是 **`doFill` 跑完那一刻** 的 `ProtoChunk`，不是某个
`ChunkStatus`。Java 侧用反射调私有的 `doFill(NoiseChunk, ChunkAccess)`，不启动服务端、不读存档、
不接受 EULA。`NoiseChunk` 走公开构造器，参数和 `buildTerrain` 一致：

* `Beardifier.EMPTY`：新区块没有结构，`forStructuresInChunk` 的 affectedBox 为 null，整块填 0；
* `Blender.empty()`：新世界周围没有老区块，`BlendingData` 为 null；`decorateBiomeResolver` 里
  `blendBiome` 的空表返回 null，`BelowZeroRetrogen` 只对升级中的区块起作用，所以两步都等价于
  原样；
* `globalFluidPicker`：从 generator 的私有字段取，和 `createNoiseChunk` 用的是同一个；
* `DensityVolume(16, height, 16, minBlockX, minY, minBlockZ)`，步长都是 1。

主世界用官方默认的 `minecraft:overworld` 设置和 `minecraft:overworld` 群系预设。
**下界、末地这一步没做**：它们没有 `aquifers`，`createDisabled` 的含水层是另一条简单的路径，
地表规则也不同，要单独移植和对拍。

## 纯函数与求值路径

`mc-bamboo-worldgen` 暴露 `chunk::Generator`（每个「种子 + 设置」建一次）：

```rust
let worldgen = mc_bamboo_worldgen::Worldgen::load()?;
let generator = mc_bamboo_worldgen::chunk::Generator::new(&worldgen, seed)?;
let biomes: Vec<Vec<u32>> = generator.biomes(chunk_x, chunk_z)?; // 24 段 × 64 个 quart 格
let blocks: Vec<Vec<u32>> = generator.noise(chunk_x, chunk_z)?;  // 384 层 × 256 个默认状态 ID
```

两个入口都是纯函数，不碰世界状态。`Generator` 内部用一个 `Compiler` 编译八个路由采样器
（`NoiseRouter`）和含水层的六条密度函数，和官方一个 `RandomState` 共用一个
`DensityFunctionCompiler` 一样；噪声实例和 `cache` 编号因此也共用。

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
    [--seeds N] [--chunks N] [--salt S] [--biomes-only] [--show N]
```

`ChunkgenOracle.java` 按官方 `WorldLoader` 的前半段读数据包注册表，对每个（种子，区块）造一个
`ProtoChunk`（`LevelHeightAccessor.create(minY, height)`、`PalettedContainerFactory.create(...)`、
`BlendingData` 为 null），调 `ChunkGenerator.createBiomes(...).join()`，再反射调 `doFill`。
输出一行一个 JSON：`header`、每个区块一条 `biomes`（24 段 × 64 个群系 ID）和一条 `noise`
（调色板 + 16×height×16 个下标，顺序 y、x、z）、最后 `end`。方块只会出现石头、空气、水、
岩浆的默认状态，Rust 侧按注册名查回默认状态 ID 逐个比较；群系按注册表 ID 逐个比较。

坐标覆盖原点、八个固定区块（含远处海洋/山地）和按 salt 随机的区块（三档范围，最远 ±100 万），
种子含 0、±1、很大的正负数。两边的版本、协议、设置 ID、`minY`/`height`/`seaLevel` 也会核对。

`worldgen` 子命令（数学层对拍）仍然独立存在，逐值比较噪声、密度函数、路由、群系和温度：

```bash
cargo run --release -p mc-bamboo-vanilla -- worldgen java
```

当前结果（release，本机）：

| 对拍 | 规模 | 不一致 |
|---|---|---|
| `chunkgen` | 6 种子 × 36 区块 × 2 个 salt = 432 区块，663552 个 quart 格、42467328 个方块 | 0 |
| `chunkgen --biomes-only` | 8 种子 × 36 区块 × 2 个 salt = 884736 个 quart 格（另一次单 salt 442368 格） | 0 |
| `worldgen` | 3 种子、21 个维度状态、4608 个噪声值、5364 个密度函数值、288 个群系、536 个温度 | 0 |

含 `interpolated` 的函数在 `worldgen` 的单点成批比较里跳过 612 项（成批路径在 1×1×1 上不是纯
函数），但它们的输入成批路径已被单点求值和 `chunkgen` 覆盖。

## 耗时

`cargo run --release -p mc-bamboo-worldgen --example chunk_timing`：每个种子先预热一次，再对
7 个区块计时取中位数；5 个种子共 35 个区块：

```
seed                    0: median 3.6 ms/chunk
seed                    1: median 3.5 ms/chunk
seed                   -1: median 3.4 ms/chunk
seed  1234567890123456789: median 3.6 ms/chunk
seed -8765432109876543210: median 3.6 ms/chunk
all 35 chunks: median 3.6 ms, min 3.3 ms, max 4.0 ms
```

**单区块生成到 NOISE（不含群系）的中位数约 3.6 ms**（不同次运行在 3.5–3.7 ms 之间）。测量包含 `finalDensity` 整块成批求值和
含水层逐点求值，不含 JVM、不含对拍 IO、不含建 `Generator`（每个种子只建一次）。当前实现优先
逐位一致：临时缓冲从 `Context` 的池里复用，但没有并行，噪声和插值都按官方顺序逐点算。

## 还没做

* 地表规则（`MaterialSystem`，含矿脉）、雕刻器、地物、结构、光照；`terrain` 状态的后两件事。
* 下界和末地（无 `aquifers` 的路径、五群系末地源、下界老 fbm）。
* 接进游戏：Director、世界类型、区块流水线、计算池；这一步只有纯函数和对拍。
* `BlendingData` / `Blender` 的非空路径（老存档升级、旧区块混合）。
* `Beardifier` 的非空路径（结构影响密度）。
* 高度图、后期处理位置表和 `ProtoChunk` 的其余字段；对拍目前只比方块和群系。
* 并行化和 SIMD：先有逐位一致的基线再谈优化。
