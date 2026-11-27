# Starlight 的并行光照结构，以及 Bamboo 能不能照搬

日期：2026-11-21。只读调研，没有改 Bamboo 代码，没有编译或运行任何东西。

读过的源码：

- Starlight：`Tuinity/Starlight` 的 `TECHNICAL_DETAILS.md`（文档自己声明只对 1.16–1.19 的对比有效）。
- Moonrise：`Tuinity/Moonrise` 提交 `dc27e877`（2026-11-18），下文路径都相对
  `src/main/java/ca/spottedleaf/moonrise/`。浅克隆在 `W:\data\bamboo-agents\tmp-starlight\`。
- Bamboo：`main`（`4ad4370c`）与 `op-genperf` 工作树的 `docs/performance-optimizations.md` 第 23 节。

Starlight / Moonrise 是 LGPL / GPL 系代码。本文只描述思路，不抄代码；落地时也只能按思路重写。

---

## 0. 先说结论

1. **Bamboo 的生成期光照已经在计算池里并行跑了，而且比 Starlight 更「无状态」。**
   `generate_light_stage` 是纯函数（3×3 的 FEATURES 后方块 → 中心柱两层光），没有共享可变状态，
   任意两个区块的光照任务可以同时跑。Starlight 的 `LIGHT` 阶段反而**不是**并行安全的
   （Moonrise 源码原话：`LIGHT is not parallel safe`），它靠「按区域互斥的调度器」把
   相邻区块的光照任务串行化，只让互不重叠的区域并行。
2. **所以「把光照搬出世界线程」这件事在 Bamboo 里没有可搬的东西。** 世界线程上和光有关的
   只剩：把算好的光装进盒子（`set_generated_light`）、交付时重算 3×3 段状态（`rebind_area`）、
   以及运行时方块变化的光照更新（每 tick 8 万步预算）。
3. **「世界线程约 15 ms/交付区块」这个数字不能当作世界线程的真实工作量。**
   `director_pipeline_bench` 的 `settle` 循环不睡觉、不停调 `tick_unpaced`，世界线程在等池子时空转，
   空转的 CPU 全记进 `world_cpu`（第 18.1 节自己写明了这一点）。插桩真正量到的世界线程工作只有
   `assemble` 1.8–2.5 ms/区块，加上没有单独计时的交付和刷怪。23.3 也写了「不能把聚合 CPU 当作可
   直接相加的串行路径」。**先量准，再决定改不改 Director。**
4. Starlight 真正值得借的是三个结构性思路，都不是「把光搬到别的线程」：
   - **光照读的是已经物化好的邻居方块，不为每次点灯重拼一遍。** Bamboo 现在每个光照任务把
     3×3 九格各折写一遍地物（`light_fold` 54.9 ms，占 `light` 的 82%），同一格被 9 个光照任务
     各拼一次。对应到 Bamboo 就是「FEATURES 后方块每格只物化一次、`Arc` 共享」（23.4 的
     `reuse-final` 原型，逐字节不变）。
   - **光数据挂在区块上，附带预先算好的「段空不空」表（emptiness map）。** Bamboo 交付时在
     世界线程按方块重算 3×3 段状态，可以改成计算池预先算好、交付时只合并。
   - **完工组装是纯函数，可以整个进池。** Starlight 的光照任务自己推进区块状态，主线程只做
     `FULL`。Bamboo 的 `Task::Assemble`（克隆快照、合并地物写、打包调色板、装光、生成期刷怪）
     全部输入都是不可变或独占的，可以变成一个池任务。
5. 光值与刷怪：上面三项都不改变喂给光引擎的方块，也不改变光引擎，**光值逐格不变、生成期刷怪
   读到的光不变**。不需要记录任何新的官方差异。

---

## 1. Starlight 怎么做到「光照和生成同时跑」

### 1.1 光数据挂在区块上，引擎本身无状态

官方把光存在光引擎自己的全局表里（`section → DataLayer` 的哈希表，写时复制），所有光照任务
都要经过这一张表，所以官方只有一条光照线程。`TECHNICAL_DETAILS.md` 的「Notice about invalid
gen test results」专门提到这张表的写时复制会随已生成区块数增长而变慢。

Starlight 把这张表拆掉了：

- 每个区块自己带两组 nibble 数组（天空光、方块光）和两张 emptiness map。
  接口在 `patches/starlight/chunk/StarlightChunk.java`，由
  `mixin/starlight/chunk/ChunkAccessMixin.java` 混进 `ChunkAccess`。
- 引擎对象（`SkyStarLightEngine` / `BlockStarLightEngine`）不持有任何世界数据，只有工作缓冲
  （BFS 队列、5×5 区块 × 全高度的段指针缓存）。`StarLightInterface.getSkyLightEngine()`
  从 `ThreadLocal` 里借一个，用完 `releaseSkyLightEngine()` 还回去
  （`patches/starlight/light/StarLightInterface.java:359-410`）。哪条线程跑光照任务，就用哪条
  线程自己的引擎实例。
- 每次任务开始 `setupCaches()` 把中心周围的段指针和 nibble 指针抄进缓存，结束
  `destroyCaches()` 清掉（`StarLightEngine.java:183-218`、`285`）。半径 1 的邻居拿方块和
  nibble（可读可写），半径 2 的邻居只拿 emptiness map。

这就是「stateless light sections」的含义：状态全在区块上，引擎只是算法加临时缓冲。

### 1.2 单写多读的 nibble：光照线程写，主线程读，不加锁

`SWMRNibbleArray`（Single Writer Multi Reader，`patches/starlight/light/SWMRNibbleArray.java`）
每段光有两份存储：`storageUpdating`（只有持有该区域的光照任务能写）和 `volatile storageVisible`
（别的线程读）。四种状态：`NULL`（不存在）、`UNINIT`（全 0 但没分配）、`INIT`、`HIDDEN`
（有数据但转成官方格式时当作不存在）。

光照任务跑完才调 `updateVisible()`（第 333 行起）把写入的那份发布成可见的；主线程、网络线程、
存盘线程读的永远是上一次发布的完整结果，不会读到算了一半的光。发布完两份指向同一数组，
多出来的缓冲放回线程本地池。

Bamboo 运行时光照已经有同样的结构：`LevelLight` 里 `engines`（在算的）和 `visible`
（给别人读的）两份，`LightStorage::publish` 一轮算完才换上去
（`crates/mc-bamboo-world/src/light/storage.rs:453`、`light/mod.rs` 的 `Visible`）。

### 1.3 调度：光照任务不是并行安全的，靠「区域互斥」保证不打架

这是最容易误解的一点。`patches/chunk_system/scheduling/ChunkTaskScheduler.java:120-185`
给每个阶段标了写半径和是否并行安全：

| 阶段 | 写半径 | 并行安全 |
|---|---:|---|
| `STRUCTURE_STARTS` / `STRUCTURE_REFERENCES` / `BIOMES` / `TERRAIN`（noise+surface+carvers） | 0 | 是 |
| `FEATURES` | 1 | **否**（写邻居） |
| `INITIALIZE_LIGHT` | 0 | 是（空操作） |
| `LIGHT` | **2** | **否** |
| `SPAWN` | 0 | 否（注释解释了为什么保守） |
| `FULL` | 0 | 在主线程跑 |

并行安全的阶段直接丢进 `parallelGenExecutor`。不安全的阶段走
`radiusAwareScheduler = new AreaDependentQueue(parallelGenExecutor, 4)`（第 279 行）：
世界按 16×16 区块切格子，任务声明「中心 + 写半径」，调度器保证**覆盖格子有重叠的两个任务不会
同时执行**，不重叠的照常并行，底下用的是同一个线程池。

所以 Starlight 的并行度是这样来的：

- 地形（最贵的 noise/surface/carvers）完全并行，和光照互不相干，因为它只写自己。
- `FEATURES` 和 `LIGHT` 在同一片区域内互斥，但两片离得够远的区域可以各跑各的。
  一个玩家身边的区块基本落在同一两个格子里，这部分实际上是串行的；多玩家分散时才并行。
- 光照任务不需要显式的邻居锁。互斥由调度器保证，任务体里直接读写邻居的 nibble。

`ChunkLightTask`（`patches/chunk_system/scheduling/task/ChunkLightTask.java`）本身很薄：
`scheduleTask` 把一个 `LightTask` 塞进 `ServerLightQueue`（第 100-106 行），真正执行时
（第 148-180 行）：盘上读来且已点亮的区块走 `forceLoadInChunk` + `checkChunkEdges`
（只做边缘校验）；否则 `lightChunk`，然后把区块状态推进到 `LIGHT`。**状态推进在光照任务所在
线程完成，不回主线程。**

### 1.4 点一个区块需要什么邻居

- 进入 `LIGHT` 前，官方的阶段金字塔要求半径 1 的邻居都到 `FEATURES` 之后（`INITIALIZE_LIGHT`）。
  Moonrise 没改这条依赖。
- `StarLightEngine.light()`（第 901-924 行）给中心新建一组 nibble，强制把中心放进缓存，
  先处理 emptiness 变化，再 `lightChunk`，最后 `setNibbles` + `updateVisible`。
- 邻居只有「已点亮」才参与：`canUseChunk()` 要求邻居状态 ≥ `LIGHT` 且 `isLightCorrect()`
  （`SkyStarLightEngine.java:240`、`BlockStarLightEngine.java:48`）。**没点亮的邻居不放进缓存，
  光不会传进去，也不从它那里读光。** 文档原话：`Starlight will not propagate into unlit chunks`。
- 邻居后来点亮时，它自己的 `lightChunk(needsEdgeChecks=true)` 做边缘检查，把两边对不上的格子
  补传播。也就是说 Starlight 的生成期光照是**增量收敛**的：每个区块点亮时只和已点亮的邻居对齐，
  最终结果等于「全部方块一次算完」的不动点，但中间状态取决于点亮顺序。
- 点亮之后再有邻居的地物溢出写进来（`FEATURES` 写半径 1），走的是普通方块变化路径（见 1.5），
  `queueTaskForSection` 只接受状态 ≥ `LIGHT` 的区块的更新
  （`mixin/starlight/lightengine/ThreadedLevelLightEngineMixin.java` 的 `queueTaskForSection`）。

### 1.5 运行时方块变化怎么批处理

`ServerLightQueue`（`StarLightInterface.java:746-936`）是一张 `区块坐标 → ServerChunkTasks`
的并发表。同一区块的所有待办并进同一个对象：

- `changedPositions`：一个 `Set<BlockPos>`，同一格改多次只算一次；
- `changedSectionSet`：哪些段从空变非空或反过来；
- `queuedEdgeChecksSky` / `queuedEdgeChecksBlock`：要做边缘检查的段；
- `lightTasks`：这个区块的整块点亮任务（如果有，点完就不再处理上面的增量）；
- `onComplete`：完成回调。

`ServerChunkTasks` 构造时就在 `radiusAwareScheduler` 上建一个写半径 2 的任务（第 891-897 行），
`schedule()` 入队。执行时 `runTasks()`（第 616-662 行）借引擎、一次处理这个区块攒下的全部变化、
`updateVisible`、跑回调。主线程调 `checkBlock` 只是往表里加一条，从不等光照。

为了保证邻居在场，入队时给区块加一张 `CHUNK_WORK_TICKET`（等级对应 `LIGHT` 状态），
任务完成的回调里撤票（`queueTaskForSection`）。批量重新点亮（`/relight`）则用整批区块的包围盒
向 `radiusAwareScheduler` 申请一个大区域任务。

### 1.6 和官方光引擎的区别（影响 Bamboo 取舍的部分）

| | 官方（1.20+，26.3 相同） | Starlight |
|---|---|---|
| 光数据放哪 | 光引擎的全局表，写时复制 | 挂在区块上 |
| 线程 | 一条光照线程，任务按批（1000 个）排队 | 生成线程池里按区域互斥 |
| 传播算法 | 1.20 起官方已经照 Starlight 改成「向邻居推」的 BFS | 同左，实现更省读 |
| 天空光初始化 | 靠传播器从最高段往下推 | 手工从上往下扫出光源，只把必要的格子入队 |
| 往没点亮的区块传光 | 会传，之后被覆盖 | 不传 |
| 存盘格式 | 官方 | 多一个自己的「已点亮」标记，对官方显示为未点亮（互相会重新点灯） |
| 最终光值 | — | 声明与官方一致，差异视为 bug |

`TECHNICAL_DETAILS.md` 开头的 Obsoletion notice 说得很直接：1.20 的官方光引擎已经抄走了文档里
的传播算法，生成测试里两者差距小到不值得比。**Starlight 现在的价值在调度和数据归属，不在算法。**

---

## 2. Bamboo 现在怎么做

### 2.1 数据模型：不可变快照 + 纯函数阶段

- 十个 `ChunkStatus` 阶段是纯函数，Director（`crates/mc-bamboo-world/src/director/state.rs`）
  只管依赖和票，`director_host.rs` 是世界线程上的宿主。
- `terrain` 阶段交回不可变的 `Arc<GeneratedChunk>`（CARVERS 后的逐层方块），世界线程放进
  `partials`。之后没人改它。
- `features` 阶段不写邻居，而是产出「世界坐标增量」（`FeatureWrites`），世界线程按 `TaskId`
  顺序追加到 `feature_deltas`。邻居中心溢进本区块的写由本区块纯函数重算
  （`cached_feature_run`，全进程分片缓存）。
- `light` 阶段：`crates/mc-bamboo-game/src/worldgen_light.rs` 的 `generate_light_stage`。
  拼 3×3 的 CARVERS 快照，对九格各做一次 `feature_writes_into` + `merge_feature_writes`
  还原 FEATURES 后方块，交给 `mc_bamboo_world::light::generate::generate_light_from_layers`。
  后者临时建一套 `Engines`，跑 `initialize_chunk` → `propagate_light_sources` →
  `run_light_updates`，取中心柱两层光返回 `GeneratedLight`。视图外读成空气。
- 运行时光照：每个 `Region`（island）一份 `LevelLight`，`light_updates::run_light_updates`
  在 tick 第 1 阶段之后跑，每 tick 最多 `LIGHT_BUDGET = 80_000` 步（`light_updates.rs:23`），
  `world.rs:1294` / `3439` 调用。island 拆分合并时光照跟着 `split_off` / `absorb`。

### 2.2 哪些在池里，哪些在世界线程

| 工作 | 在哪跑 | 依据 |
|---|---|---|
| terrain / features / light 三个阶段的全部计算 | 计算池 | `ChunkWork::Generate` 经 `queue_compute` 派发（`director_host.rs` 约 2163 行） |
| 3×3 折写（`light_fold`）、光引擎（`light_engine`） | 计算池 | `worldgen_light.rs` 的计时器 |
| 回执合并：`partials` 插 `Arc`、增量追加、存 `GeneratedLight` | 世界线程 | `merge_generated`（`director_host.rs:2522`），都是指针或小 `Vec` 操作 |
| **完工组装** `Task::Assemble`：克隆快照（约 393 KiB）、`merge_feature_writes`、`build_region_column`（调色板打包、高度图）、`set_generated_light`、`spawn_original_mobs`、装 `structures` 标签 | **世界线程** | `director_host.rs` 约 2190-2290 行；`ASSEMBLE_NS` 量到 1.8–2.5 ms/区块（不含刷怪） |
| **交付** `deliver_column`：插区块、插光柱、`rebind_area` 按方块重算 3×3 段状态、插各张上层表 | **世界线程** | `region/column.rs:636-700`、`store.rs:701-735`、`light/mod.rs:362` |
| 运行时方块变化的光照 | 世界线程（island tick） | `light_updates.rs` |

为什么组装在世界线程：`feature_deltas` 是世界线程的表，要等「这个区块需要的增量都到齐」才能合，
而到没到齐只有 Director 知道；最早的实现就顺手在出任务的地方合了。18.6.4 的 β / γ 讨论过把它
挪走，一直等线程模型确认。

为什么交付必须在世界线程：它改 `Region` 的权威状态（区块表、光照表、实体表），这是线程模型的
硬规则（状态只有一个所有者）。能挪走的只有交付里的**计算**部分，不是交付本身。

另一个串行耦合：`deliver_column` 要求 `ready_to_move_columns()`，即这个 region 的运行时光照
已经全部算完，否则这一 tick 不交付。玩家在大量改方块时，交付会被光照预算拖后。

### 2.3 时间实际花在哪（引用 23.3，单线程每交付区块）

- 计算池 CPU 197.5 ms，其中 `light` 67.4 ms：`light_fold` 54.9 ms + 光引擎 12.4 ms。
- `terrain` 4.4 ms、`features` 14.2 ms（阶段墙钟，不含被 `light` 阶段代付的重算）。
- `column` 3.5 ms（`build_region_column`，池里各阶段和世界线程组装都会调）。
- `assemble` 2.5 ms（世界线程）。
- 81 个交付区块要算 13×13 个地物中心、15×15 块地形；官方同一条链是 11×11 / 13×13。

**光引擎本身只占 12.4 ms（6%）。贵的是为光引擎准备输入。**

---

## 3. 关于「世界线程 15 ms/区块」这个数字

第 23.3 / 23.5 / 23.7 把 8 线程以上的瓶颈归到世界线程（`world_cpu` 15.0 ms/区块）。
读了测量代码之后，这个归因证据不足：

1. `examples/director_pipeline_bench.rs` 的 `settle()`（第 193-214 行）是一个不睡觉的循环：
   `world.tick_unpaced()` → 检查是否全部加载 → 再来。池子没回执时世界线程在空转。
2. `world_cpu_ms = thread_cpu_ms()` 的差值（第 422 / 439 行）量的是这条线程的 CPU 时间，
   空转全部算在内。第 18.1 节写着：「测量循环不睡觉，世界线程等池子时空转，整段墙钟几乎都记成
   世界线程 CPU（1 线程 4.1 s ≈ 墙钟 4.2 s）」。
3. 8 线程时 `world_cpu` 15.0 ms 小于墙钟 32.2 ms，最可能的解释是机器超订（8 条池线程 + 世界线程
   + 当时 26%–100% 的整机负载），世界线程被调度器拿走了一半时间片，而不是它只「忙」了 15 ms。
4. 插桩量到的世界线程真实工作：`assemble` 1.80 ms/区块（8 线程）。交付和刷怪没有单独的每区块
   数字。即使按 5 ms 估，也只占 32.2 ms 墙钟的 15%。

有一个值得验证的巧合：8 线程时 `池 CPU / 8 = 17.4 ms`，`17.4 + 15.0 = 32.4 ≈ 墙钟 32.2`。
如果这不是巧合，它指向的不是「世界线程工作太多」，而是「**池子在等世界线程派活**」：
任务只在世界线程 tick 里派发，每线程队列深度 4，一个区块要经过 terrain → features → light →
assemble 四次往返。这是调度粒度问题，和光照在哪条线程无关。目前没有数据区分这两种解释。

**因此第一步必须是量准**（见第 7 节 S0），而不是直接动 Director。

> 现状（2026-11-27，S0 已完成）：实测见 [§31](../performance-optimizations.md#31-s0-世界线程实测区分做事空队列与-50-ms-派活节奏2026-11-27)。上面「目前没有数据区分这两种解释」是 S0 之前的原始判断。实测结论：世界线程真实做事约 9.8–10.1 ms/块（OS CPU 7.3–8.3 ms/块），组装约 5 ms/块、装光约 1.8–1.9 ms/块、交付存储含 `rebind_area` 仅 0.02–0.04 ms/块；8/12 线程 50 ms 节拍下池空队列等活中位 58.50% / 68.61%，改为回执即派后降到 26.43% / 37.60%。因此先做 S4（调度粒度），S1 省 CPU，S2 待复测。

---

## 4. Starlight 的哪些思路 Bamboo 用得上

| Starlight 的做法 | Bamboo 现状 | 判断 |
|---|---|---|
| 光数据挂在区块上，引擎无状态 | 生成期：`GeneratedLight` 随任务回执走，引擎每任务新建，已经无状态。运行时：光在 `LevelLight`（每 region 一份） | 生成期已具备；运行时不必改 |
| 单写多读、算完再发布 | `engines` / `visible` 两份 + `publish` | 已具备 |
| 光照任务读**已物化**的邻居方块 | 每个光照任务把 3×3 九格重新折写一遍 | **差距最大，值得做（方案 A）** |
| 光照任务自己推进区块状态，主线程只做 `FULL` | `Task::Assemble` 在世界线程做组装 + 装光 + 刷怪 | **值得做（方案 B）** |
| 每区块带 emptiness map，光照任务里更新 | 交付时世界线程按方块重算 3×3 段状态 | 值得做，但先量（方案 C） |
| 区域互斥调度（`AreaDependentQueue`） | 纯函数，不需要互斥 | **不要引入**：它是给共享可变区块用的，Bamboo 没有这个问题，引入只会降低并行度 |
| 不往未点亮邻居传光，邻居点亮时做边缘检查（增量收敛） | 每个区块一次算出最终值 | **不要引入**：增量收敛让中间状态依赖调度顺序，和「线程数不影响结果」的现有保证冲突 |
| 运行时方块变化按区块合批、在池里算 | 世界线程按步数预算算 | 和生成吞吐无关，另开题目（第 6.4 节） |

一句话：**Starlight 用「调度互斥 + 增量收敛」换来了在共享可变区块上的并行；Bamboo 用「不可变
快照 + 纯函数」已经拿到了更强的并行，代价是重复物化。要补的是重复物化，不是并行。**

---

## 5. 给 Bamboo 的迁移设计

### 5.1 方案 A：FEATURES 后方块每格只物化一次

**现在**：光照任务 `light(C)` 对 3×3 的每一格 N 做 `clone(CARVERS(N))` +
`feature_writes_into(N)`（查 9 个来源中心）+ `merge`。一格 N 会出现在 9 个光照任务的视图里，
于是被物化 9 次；完工组装时 C 自己还要再物化第 10 次。

**改成**：新增一种纯函数产物 `PostFeatures(N) = Arc<GeneratedChunk>`（CARVERS + 9 个来源中心的
写，按现有固定先后合并），全进程缓存，每格只算一次。

- `light(C)` 取 9 个 `Arc<PostFeatures>`，直接喂 `generate_light_from_layers`，不再克隆和折写。
- `assemble(C)` 直接用 `PostFeatures(C)`，不再克隆快照和合并增量。
- 这就是 23.4 的 `reuse-final` 原型做的事（已实测：计算池 CPU −12%，方块和光逐字节不变，
  缓存约 96 MiB）。它当时因为「单独不到 20%」没进主流程。和方案 B 合起来做，它同时是 B 的前提。

数据归属规则：

- `PostFeatures(N)` 是不可变值，键是 `(配置, N)`，创建后任何线程只读。不需要邻居锁。
- 谁先需要谁算，同键并发用现有分片缓存的 `OnceLock` 语义（一个算、其余等）。
- 释放沿用 `worldgen_cache::chunk_finished` 的消费者位图：N 的 9 个光照消费者加 1 个组装消费者
  全部完工即释放。未被请求的外圈由预算淘汰。
- 结果与调度无关：合并顺序是来源中心的固定先后（18.10），和现在完全相同。

注意它**不**减少「13×13 个地物中心」这笔结构性开销（那是 `light-local` 放宽想省的，会改光值）。
A 只去掉重复物化。

### 5.2 方案 B：完工组装进计算池

把 `Task::Assemble` 的计算体变成一个池任务：

```
assemble_job(
    post_features: Arc<GeneratedChunk>,   // 方案 A 的产物；没有 A 时传 CARVERS 快照 + 增量 Vec
    light:         GeneratedLight,        // 从 generated_light 表里取出，移动进任务
    structures:    Option<StructuresTag>, // 同上
    disk_upper:    Option<Vec<u8>>,       // 读盘带来的上层内容
    config, pos, spawn_mobs,
) -> Box<RegionColumn>
```

任务体依次做：`build_region_column` → `adopt_upper_content` → `set_generated_light` →
`spawn_original_mobs` → `set_structures_tag` → 抬 `ChunkStatus::Full`。

世界线程保留的：

1. Director 判定「可以组装」（依赖到齐）——不变；
2. 从 `partials` / `feature_deltas` / `generated_light` / `generated_structures` /
   `disk_upper_bytes` 里**取出**这个位置的条目，移动进任务（所有权转移，世界线程不再持有）；
3. 收回执 → `TaskResult::Assembled { column }` → `deliver_column`。

规则：

- **世界线程发出任务时一次性交出全部输入，任务期间不再碰这个位置的任何表项。** 现有的
  `cancelled_compute` 机制照用：任务在跑时被取消，回执丢弃，纯函数重跑结果相同。
- `spawn_original_mobs` 已经是 `(种子, 位置, 盒子)` 的纯函数（`worldgen_mobs.rs:42`，
  经 `with_generator` 用线程本地 `Generator`，随机数从种子和坐标派生），不读世界状态，可以原样
  进池。顺带好处：世界线程不再需要自己那份 `Generator`。
- `spawn_mobs` 游戏规则在**派发时**读取并随任务带走。这和现在「组装那一刻读」相差最多一次池往返
  （亚 tick 到几 tick），只在规则切换的瞬间可见；需要在提交说明里写明。
- `worldgen_cache::chunk_finished` 仍在世界线程收到回执后调用，保持释放顺序确定。
- 无池（内联执行器）路径原样保留，同一个函数当场调用。

这一步之后世界线程上每交付区块只剩：回执合并（指针操作）+ 交付插表 + `rebind_area`。

### 5.3 方案 C：段状态预计算（对应 Starlight 的 emptiness map）

`ChunkStore::insert_column` 每次交付调 `rebind_area`，后者 `SectionStatesAround::compute`
按 3×3 区块此刻的方块重算段的空 / 非空与邻居计数（`light/mod.rs:362`）。

改法：每个 `ChunkColumn` 带一张 26 位的「本柱各段是否非空」位图，在池里的组装任务中算好
（段的非空计数 `Chunk` 本来就有）。交付时世界线程只读 9 张位图做合并，不再扫方块。

这一步**先量再做**：`rebind_area` 目前没有单独计时，如果它本来就只读段计数，收益可能接近 0。

### 5.4 不做的事

- 不引入区域互斥调度、不引入共享可变的生成期区块。
- 不改光引擎算法。官方 1.20+ 的传播已是 Starlight 式，Bamboo 的引擎是照官方对拍过的，
  光引擎只占 12.4 ms/区块。
- 不把生成期光照改成「先交付、运行时补算」（23.5 第 4 条）。那会改变生成期刷怪读到的光。
- 不做 Starlight 式的「只和已点亮邻居对齐」的增量点灯。

---

## 6. 预期收益、对拍影响、风险

### 6.1 预期收益（带推理）

记 W = 世界线程每交付区块的**真实**串行工作，P = 池 CPU / 交付区块，N = 线程数。
理想墙钟 ≈ max(W, P/N)，实际介于它和 W + P/N 之间（取决于池子等不等世界线程派活）。

- **方案 A**：P 降约 12%（`reuse-final` 实测）。1 线程墙钟按比例降；8 线程 P/N 从 17.4 降到
  约 15.3 ms。若和 B 合并，组装里的克隆 + 合并（约 1 ms）也一并消失。
- **方案 B**：W 减去 `assemble` 1.8–2.5 ms 和刷怪。
  - 如果 15 ms 大部分是空转（我认为更可能）：W 真实值约 3–5 ms，B 把它降到 1–2 ms。
    8 线程墙钟改善上限约 2–3 ms / 32.2 ms，**不到 10%**。
  - 如果 15 ms 是真实工作（最乐观的假设）：已知可搬的只有约 2.5 ms + 刷怪，剩下 12 ms
    不在组装里，B 仍然只拿得到约 10%；剩余部分要靠 S0 找出来。
  - 两种情况下 B 都不会带来「8 线程吞吐翻倍」。它的确定收益是：世界线程的 tick 抖动变小
    （组装是 tick 里单笔最大的生成相关开销，一 tick 交付多个区块时会叠加）。
- **方案 C**：未知，等 S0。
- **如果 S0 证实是「池子等派活」**：真正的杠杆是派发粒度（回执到达即派后继、加深每线程队列、
  把 features → light → assemble 串成池内续跑不回世界线程）。上限是墙钟从 32.2 逼近 P/N = 17.4，
  即 **吞吐约 +85%**。这比 A + B + C 加起来都大，但它是 Director 调度改动，属于线程模型范围，
  要走评审。B 是它的前提之一（组装不进池，链条就必须回世界线程）。

### 6.2 光值与对拍

- A、B、C 都不改变喂给光引擎的方块集合、不改变合并先后、不改变光引擎。**光值逐格不变。**
  验收用现有的 `light-pipeline`（6 个无结构 + 3 个带结构场景，应仍为 0 不一致）、
  `--break-carvers` 改坏确认、`staged_tests::worker_counts_generate_identical_feature_blocks`
  （1/2/4/8 线程方块与两层光相同）、`blocks_hash` 仍为 `3c0a7f8a3b11b219`。
- 不新增官方差异，README 的已知差异不用动。

### 6.3 生成期刷怪

- `spawn_original_mobs` 的输入是装好方块和光的盒子 + 种子 + 位置。B 只换执行线程，输入逐字节
  相同，输出应逐字节相同。验收用 23.12 / 23.13 的生成期刷怪不变量定点对拍和 bench 的
  `entities_hash`（类型 + 出生坐标的摘要）。
- 唯一语义变化是 `spawn_mobs` 规则的读取时刻提前到派发时（5.2）。刷怪概率不变。

### 6.4 风险

| 风险 | 说明 | 缓解 |
|---|---|---|
| 15 ms 的归因错了，做完吞吐不动 | 第 3 节 | S0 先量；A 有独立的 CPU 收益，不依赖这个归因 |
| 所有权转移遗漏 | 组装输入散在 5 张世界线程表里，漏移一张会出现任务期间表项被 `Forgot` 清掉或泄漏 | 把 5 张表的取出收进一个函数；复用现有取消路径的测试（`compute_pool_tests`） |
| 读盘续跑分支 | `disk_saved_after_features`、`resumed_after_spawn`、`disk_upper_bytes` 三个分支要原样带进任务 | 这部分属于生命周期代码，按评审规则走代理评审；`storage_diff_tests` 覆盖 |
| `PostFeatures` 缓存内存 | 每条约 393 KiB，`reuse-final` 实测约 96 MiB | 并进现有 `worldgen_terrain_cache_mb` 预算和消费者位图释放 |
| 每线程 `Generator` 内存 | 刷怪进池后每条池线程的 `Generator` 多一条使用路径，但对象已存在 | 无新增；世界线程那份可以省掉 |
| 交付仍被运行时光照卡住 | `ready_to_move_columns` 的耦合不在本设计范围 | 记录为后续题目 |

关于运行时光照（Starlight 的 1.5）：把方块变化的光照按区块合批搬进池，是另一件事，
收益在 tick 而不在生成吞吐，而且会改变「光在哪个 tick 可见」。官方的光照线程本身就是异步的，
时刻不属于必须精确的层级，但这要单独设计和评审，本报告不展开。

---

## 7. 分阶段计划

> S0 后续实测：[世界线程与计算池的三轮矩阵](../performance-optimizations.md#31-s0-世界线程实测区分做事空队列与-50-ms-派活节奏2026-11-27)。
> 下表是原调研的方案与验收建议，优化顺序以 S0 实测为准。


工作量按一个熟悉代码的代理的净工作日估，不含排队等机器的时间。

| 阶段 | 内容 | 改动范围 | 验收 | 估时 |
|---|---|---|---|---|
| **S0 量准** | (1) bench 的 `settle` 在池子无回执时阻塞等待而不是空转（或另记「世界线程忙碌时间」）；(2) 给 `deliver_column` / `rebind_area` / 刷怪加每区块计时；(3) 记录池线程空闲比例（在飞任务数直方图） | 只动 bench 和默认关的插桩 | 得到 W 的真实值；回答「池子在等派活吗」 | 0.5–1 天 |
| **S1 方案 A** | `PostFeatures` 物化一次并缓存；`light` 和组装都改用它（把 `reuse-final` 原型转正） | `worldgen_light.rs`、`worldgen_features.rs`、`worldgen_cache.rs` | `light-pipeline` 0 不一致、`blocks_hash` 不变、`entities_hash` 不变；池 CPU −10% 以上 | 2–3 天 |
| **S2 方案 B** | `ChunkWork::Assemble` 池任务；世界线程只做取出、派发、交付 | `director_host.rs`、`chunk_jobs.rs`、`worldgen_mobs.rs` 的调用点 | 同 S1，加 `staged_tests`、`storage_diff_tests`、取消与 `Forgot` 路径测试；需要代理评审（线程 + 生命周期） | 3–4 天 |
| **S3 方案 C** | 段非空位图随盒子走，`rebind_area` 改合并位图 | `mc-bamboo-world` 的 `column.rs`、`light/storage.rs`、`store.rs` | 运行时光照对拍（`gen-light` 及交付后改方块的光照测试）不变 | 1–2 天；**仅当 S0 显示 `rebind_area` > 0.5 ms/区块** |
| **S4 调度粒度** | 池内续跑 / 回执即派 / 队列深度；只在 S0 证实池子有明显空闲时做 | Director 与宿主的派发路径 | 线程数不影响结果的全部现有测试；吞吐按 S0 的口径复测 | 设计 1–2 天 + 实现 4–6 天，先出设计给用户定 |

建议顺序：S0 → S1 → 复测 → 再决定 S2 / S4 的先后。S0 的结果很可能改变优先级：
如果池子确实在等派活，S4 才是主菜，S2 是它的前菜；如果池子是满的，那 8 线程以上的瓶颈就是
P/N 本身（以及整机负载），应回到减少 P（S1、以及 `light-local` 这类需要用户拍板的放宽）。

---

## 8. 源码索引

Moonrise（`dc27e877`，路径相对 `src/main/java/ca/spottedleaf/moonrise/`）：

- `patches/chunk_system/scheduling/ChunkTaskScheduler.java:120-185` 各阶段写半径与并行安全表；
  `:279` `radiusAwareScheduler`；`:655-668` 按目标状态建任务。
- `patches/chunk_system/scheduling/task/ChunkLightTask.java:100-106` 入队；`:148-180` 任务体。
- `patches/starlight/light/StarLightInterface.java:359-410` 线程本地引擎借还；
  `:556-662` `ChunkTasks` 合批与 `runTasks`；`:746-936` `ServerLightQueue` / `ServerChunkTasks`。
- `patches/starlight/light/StarLightEngine.java:183-218` `setupCaches`；`:268-283` `updateVisible`；
  `:901-924` `light()`。
- `patches/starlight/light/SkyStarLightEngine.java:240`、`BlockStarLightEngine.java:48` `canUseChunk`。
- `patches/starlight/light/SWMRNibbleArray.java` 四种状态与 `updateVisible()`（`:333`）。
- `mixin/starlight/lightengine/ThreadedLevelLightEngineMixin.java` `queueTaskForSection`、
  `checkBlock`、`updateSectionStatus`、`starlight$serverRelightChunks`。

Starlight：`TECHNICAL_DETAILS.md` 的 Obsoletion notice、Light propagation algorithm、
Light data management、Chunk lighting algorithms、Chunk Save Format 各节。

Bamboo（`main`）：

- `crates/mc-bamboo-game/src/worldgen_light.rs` 生成期光照阶段。
- `crates/mc-bamboo-world/src/light/generate.rs:73-125` 纯函数光照入口。
- `crates/mc-bamboo-world/src/light/mod.rs:187-450` `LevelLight`；`:362` `rebind_area`。
- `crates/mc-bamboo-game/src/director_host.rs:2150-2290` 任务派发与 `Task::Assemble`；
  `:2522` `merge_generated`。
- `crates/mc-bamboo-game/src/worldgen_features.rs:1363` `assemble_feature_column`。
- `crates/mc-bamboo-game/src/worldgen_mobs.rs:42` `spawn_original_mobs`。
- `crates/mc-bamboo-game/src/region/column.rs:636-700` 交付；
  `crates/mc-bamboo-world/src/store.rs:701-735` `insert_column`。
- `crates/mc-bamboo-game/src/light_updates.rs` 运行时光照预算。
- `docs/worldgen.md`「接进游戏」「生成期光照的输入」；`docs/performance-optimizations.md` 第 18 节。
- `W:\data\bamboo-agents\wt\op-genperf\docs\performance-optimizations.md` 第 23.3 / 23.5 / 23.7 节；
  `W:\data\bamboo-agents\wt\op-genperf\crates\mc-bamboo-game\examples\director_pipeline_bench.rs:193-214, 419-439`。
