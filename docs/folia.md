# M2 架构规格：Folia 调研与 Bamboo 多线程设计

目标版本：Minecraft Java 26.3（协议 777）。调研日期：2026-11-06。
本文档只讲线程架构，没有数据包定义。

标注约定：

- `[事实]`：有来源，来源编号见第 1 节。
- `[推导]`：我读源码或代入公式算出来的结论，推导过程写在原地。
- `[判断]`：我的工程判断，没有来源能证明它，可以被推翻。
- `UNVERIFIED`：没能确认的内容，第 7 节有汇总。

几个术语：

- **fork/join**：一个阶段内把工作分给多个线程，全部做完才进入下一阶段。
- **EDF**（earliest deadline first）：截止时间最早的任务先跑。
- **work stealing**：空闲线程从别的线程的队列里拿任务。
- **backpressure**：下游处理不过来时让上游停下，而不是无限堆积。
- **ticket**：区块加载的引用计数。有 ticket 的区块留在内存，没有就卸载。
- **chunk holder**：服务端为一个区块位置保留的状态对象，区块未完全加载时也可以存在。

---

## 0. 结论

1. **参考 Folia 的所有权模型和不变量，不参考、不移植它的实现。** README 自己说，单线程 Paper 可以看成"一个覆盖所有世界所有区块的巨型 region"[S1]。Bamboo 的单世界线程也应该这样写：它就是只有一个 region 的 Folia。
2. **M2 保持单世界线程。** Folia 面向 16 核以上、几百名分散玩家的服务器[S1, S12]，代价是改掉一批 vanilla 行为（第 3 节）。Bamboo 现在没有这个负载，也没有性能数据。
3. **现在就采用第 6.3 节的 12 条规则。** 它们决定 M2 代码的形状，几乎不增加工作量，但以后想做 regional ticking 时不用重写。其中最重要的三条：世界状态由世界线程独占；`Global` 和 `Region` 从第一天分开；跨域只发消息。
4. **不要学 Pumpkin 的共享状态加锁。** Pumpkin 的并行单位是 tick 内的阶段，所有世界数据都要线程安全，`pumpkin-core` 里有 1539 行 `.lock()`，29 个提交提到 deadlock（不区分大小写，第 5 节）。可以学的是它的边缘设计：独立 ticker 线程、tick 内同步逻辑、批量 flush。
5. **Rust 给了 Folia 没有的东西。** Folia 用运行时检查防止跨 region 访问，Bamboo 可以用 `&mut` 和所有权在编译期保证。
6. **许可证：读可以，不要复制。** Folia 和 Pumpkin 都是 GPLv3。GPLv3 和 AGPLv3 允许合并，但复制进来的部分仍是 GPLv3，Bamboo 就不再是单一的 `AGPL-3.0-only`。

---

## 1. 来源

**Folia**（`PaperMC/Folia` 默认分支 `ver/26.3.x`，commit `73865e0e1c11752a03f30320e0521d4611221e08`，2026-10-30）

| 编号 | 内容 | URL |
| --- | --- | --- |
| S1 | README | https://github.com/PaperMC/Folia ，原文 https://raw.githubusercontent.com/PaperMC/Folia/ver/26.3.x/README.md |
| S2 | Overview 文档 | https://docs.papermc.io/folia/reference/overview ，原文 https://raw.githubusercontent.com/PaperMC/docs/main/src/content/docs/folia/admin/reference/overview.md |
| S3 | Region logic 文档 | https://docs.papermc.io/folia/reference/region-logic ，原文 https://raw.githubusercontent.com/PaperMC/docs/main/src/content/docs/folia/admin/reference/region-logic.md |
| S4 | FAQ | https://docs.papermc.io/folia/faq ，原文 https://raw.githubusercontent.com/PaperMC/docs/main/src/content/docs/folia/admin/reference/faq.md |
| S5 | `0001-Region-Threading-Base.patch`（minecraft）。`ThreadedRegionizer`、`TickRegions`、`TickRegionScheduler`、`RegionizedServer`、`RegionizedWorldData`、`RegionizedTaskQueue`、`RegionizedData` 都在这里 | https://raw.githubusercontent.com/PaperMC/Folia/73865e0e1c11752a03f30320e0521d4611221e08/folia-server/minecraft-patches/features/0001-Region-Threading-Base.patch |
| S6 | `0004-Prevent-block-updates-in-non-loaded-or-non-owned-chu.patch` | https://raw.githubusercontent.com/PaperMC/Folia/73865e0e1c11752a03f30320e0521d4611221e08/folia-server/minecraft-patches/features/0004-Prevent-block-updates-in-non-loaded-or-non-owned-chu.patch |
| S7 | `0001-Region-Threading-Base.patch`（paper）。`TickThread` 检查、`GlobalConfiguration`、`CraftPlayer` | https://raw.githubusercontent.com/PaperMC/Folia/73865e0e1c11752a03f30320e0521d4611221e08/folia-server/paper-patches/features/0001-Region-Threading-Base.patch |
| S8 | Paper 上游的 `EntityScheduler`、`TickThread`、`MoonriseConstants`、`ServerLevel.java.patch`、`ServerGamePacketListenerImpl.java.patch`（Folia 的 `paperRef=804dd0c3`） | https://raw.githubusercontent.com/PaperMC/Paper/804dd0c3f08b666dc51a0250257915c002c59755/paper-server/src/main/java/io/papermc/paper/threadedregions/EntityScheduler.java ，https://raw.githubusercontent.com/PaperMC/Paper/804dd0c3f08b666dc51a0250257915c002c59755/paper-server/src/main/java/ca/spottedleaf/moonrise/common/util/TickThread.java ，https://raw.githubusercontent.com/PaperMC/Paper/804dd0c3f08b666dc51a0250257915c002c59755/paper-server/src/main/java/ca/spottedleaf/moonrise/common/util/MoonriseConstants.java ，https://raw.githubusercontent.com/PaperMC/Paper/804dd0c3f08b666dc51a0250257915c002c59755/paper-server/patches/sources/net/minecraft/server/level/ServerLevel.java.patch ，https://raw.githubusercontent.com/PaperMC/Paper/804dd0c3f08b666dc51a0250257915c002c59755/paper-server/patches/sources/net/minecraft/server/network/ServerGamePacketListenerImpl.java.patch |
| S9 | 调度器库 LeafPile（commit `f97586bd`，GitHub 把 `Spottedleaf/ConcurrentUtil` 重定向到这里），完整 URL 见本节末尾 | https://github.com/Tuinity/LeafPile |
| S10 | 仓库元数据、`gradle.properties`、`PATCHES-LICENSE` | https://api.github.com/repos/PaperMC/Folia ，https://raw.githubusercontent.com/PaperMC/Folia/ver/26.3.x/gradle.properties ，https://raw.githubusercontent.com/PaperMC/Folia/ver/26.3.x/PATCHES-LICENSE |
| S11 | Paper 许可证说明 | https://raw.githubusercontent.com/PaperMC/Paper/804dd0c3f08b666dc51a0250257915c002c59755/LICENSE.md |
| S12 | PaperMC 官网 Folia 页 | https://papermc.io/software/folia |
| S13 | Paper 开发文档的 Folia 支持页 | https://docs.papermc.io/paper/dev/folia-support/ |
| S14 | 构建 API | https://fill.papermc.io/v3/projects/folia ，https://fill.papermc.io/v3/projects/folia/versions/26.3/builds/latest |
| S15 | Issues（第三方报告，除非注明，都未经维护者确认），完整 URL 见本节末尾 | https://github.com/PaperMC/Folia/issues |
| S16 | Moonrise 的许可证元数据 | https://api.github.com/repos/Tuinity/Moonrise |

文档仓库 `PaperMC/docs` 取自 commit `68ac55d8`。

S9 用到的文件：

- https://raw.githubusercontent.com/Tuinity/LeafPile/master/LICENSE
- https://raw.githubusercontent.com/Tuinity/LeafPile/master/concurrentutil/src/main/java/ca/spottedleaf/concurrentutil/scheduler/EDFSchedulerThreadPool.java
- https://raw.githubusercontent.com/Tuinity/LeafPile/master/concurrentutil/src/main/java/ca/spottedleaf/concurrentutil/scheduler/SchedulableTick.java
- https://raw.githubusercontent.com/Tuinity/LeafPile/master/concurrentutil/src/main/java/ca/spottedleaf/concurrentutil/scheduler/Scheduler.java
- https://raw.githubusercontent.com/Tuinity/LeafPile/master/concurrentutil/src/main/java/ca/spottedleaf/concurrentutil/scheduler/StealingScheduledThreadPool.java
- https://raw.githubusercontent.com/Tuinity/LeafPile/master/concurrentutil/src/main/java/ca/spottedleaf/concurrentutil/numa/OSNuma.java
- https://raw.githubusercontent.com/Tuinity/LeafPile/master/concurrentutil/src/main/java/ca/spottedleaf/concurrentutil/numa/LinuxNuma.java
- https://raw.githubusercontent.com/Tuinity/LeafPile/master/common/src/main/java/ca/spottedleaf/common/time/Schedule.java

S15 引用的 issue：

- https://github.com/PaperMC/Folia/issues/531
- https://github.com/PaperMC/Folia/issues/469
- https://github.com/PaperMC/Folia/issues/291
- https://github.com/PaperMC/Folia/issues/196
- https://github.com/PaperMC/Folia/issues/199
- https://github.com/PaperMC/Folia/issues/203
- https://github.com/PaperMC/Folia/issues/334
- https://github.com/PaperMC/Folia/issues/473
- https://github.com/PaperMC/Folia/issues/39

另外用 GitHub API 列出了 open issue 和 redstone 相关 issue（https://api.github.com/repos/PaperMC/Folia/issues ，https://api.github.com/search/issues?q=repo:PaperMC/Folia+redstone ）。一次 WebSearch 的结果只有 https://github.com/PaperMC/Folia 和它的镜像，没有新信息。

**其他**

| 编号 | 内容 |
| --- | --- |
| V | 26.3 官方反编译源码：`net/minecraft/server/MinecraftServer.java`、`network/Connection.java`、`network/PacketProcessor.java`、`network/protocol/PacketUtils.java`、`server/level/ServerLevel.java`、`server/level/ServerChunkCache.java`、`server/level/ChunkMap.java`、`server/network/ServerCommonPacketListenerImpl.java`、`server/network/ServerConnectionListener.java`、`server/network/ServerGamePacketListenerImpl.java`、`util/FutureChain.java`、`util/thread/BlockableEventLoop.java`、`world/clock/ServerClockManager.java`、`world/ticks/LevelTicks.java`、`world/ticks/ScheduledTick.java`、`world/level/Level.java`、`world/level/LevelReader.java`。 |
| P | 本地 Pumpkin：`W:\data\GithubProjects\Pumpkin`，commit `494c29a7`（2026-11-04）。下文的路径都相对于这个目录。 |
| T | tokio 1.54.1 源码（本机 cargo registry，和 Bamboo 的 `Cargo.lock` 版本一致），`src/sync/mpsc/mod.rs`、`bounded.rs`。 |

---

## 2. Folia 实际做了什么 `[事实]`

### 2.1 定位

- Paper 的 fork，给 dedicated server 加 regionised multithreading。每个 region 有自己的 tick 循环，以 20 TPS 在线程池上并行执行。没有 main thread [S1]。
- 独立项目，可预见的将来不会合并回 Paper [S1]。
- 默认分支已经是 26.3：`mcVersion=26.3`，`channel=ALPHA` [S10]。26.3 最新构建是 #6，channel 为 ALPHA，2026-10-29 [S14]。
- 规模 `[推导]`：两个 `Region Threading Base` patch 共改 407 个文件（minecraft 203 个，其中新建 13 个；paper 204 个）[S5, S7]。这是把单线程假设从一份现成代码里挖出来的代价。

### 2.2 Region 和不变量

region 是一组 owned 区块位置加一个数据对象，活着的 region 之间互不重叠。每个 world 有自己的 `ThreadedRegionizer` [S3]。

| 不变量 | 内容 [S3] |
| --- | --- |
| 1 | 每个存在的 chunk holder 属于且只属于一个 region。 |
| 2 | 每个 holder 周围 merge radius 内的位置都归同一 region，所以 region 不会贴着别的 region。 |
| 3 | ticking 中的 region 不能扩张，所以两个 ticking region 不会争抢中间的区块。 |
| 4 | region 只有四种状态：`transient`（不能 tick）、`ready`（可以转 ticking）、`ticking`、`dead`。 |

不变量 2 和 3 合起来，ticking region 可以创建并访问周围的 holder，不会踩到别的 region [S3]。

实际粒度是 section（N×N 区块，N = 2^`gridExponent`）。region 拥有 section，不直接拥有区块，这是一个性能取舍 [S3]。默认值 [S5 `ServerLevel` 初始化块、`ThreadedRegionizer` 构造函数；S7 `GlobalConfiguration.ThreadedRegions`]：

| 参数 | 公式 | 默认结果 |
| --- | --- | --- |
| `gridExponent` | 配置项 | 4，即 N = 16 区块 = 256 格 |
| `minSectionRecalcCount` | `max(2, max(1, 2048 / (1 << 2*shift)))` | 8 |
| `maxDeadRegionPercent` | 常量 | 1/6 |
| `emptySectionCreateRadius` | `max(1, 8 / (1 << shift))` | 1 个 section |
| `regionSectionMergeRadius` | 常量 | 1 个 section |

### 2.3 形成、合并、拆分

入口是 chunk holder 的创建和删除：`BaseChunkSystemHooks.onChunkHolderCreate/Delete` 调用 `regioniser.addChunk/removeChunk` [S5]。决定 region 的是 holder，不是玩家。

**`addChunk`** [S3, S5 `ThreadedRegionizer.addChunk`]

1. section 已存在且非空：只置一个 bit，不拿锁。
2. 否则拿写锁，创建 section，以及周围 `emptySectionCreateRadius` 内的空 section。
3. 收集 `create + merge` 半径内已有 section 所属的 region。这一步没有新建任何 section 时直接返回，不做 merge。
4. 没有邻居：新建 region，进入 `ready`。
5. 只有一个非 ticking 邻居：把新 section 加给它，region 状态不变。
6. 其他情况：选一个非 ticking 的 region 接收新 section，没有就新建 region，这样不违反不变量 3。其余邻居里，非 ticking 的立刻 `killAndMergeInto`，ticking 的记进 `mergeIntoLater`，等它本次 tick 结束。还有待合并关系时，接收 section 的 region 保持或降为 `transient`，不能被调度，这是相邻 region 不会同时 tick 的原因。

**merge** [S5 `ThreadedRegion.mergeInto`、`TickRegionData.mergeInto`、`RegionizedWorldData.REGION_CALLBACK`]

- section 和待合并关系转给目标。`RegionizedData` 逐项合并。
- 绝对 deadline 加上 `fromTickOffset = toTick - fromTick`，这样相对 deadline 不变。redstone time 单独计算 offset [S2]。
- 任务队列插到目标队列的前面，保持原顺序。
- 下次 tick 的时间取两者较晚的一个，防止 TPS 超过 20。

**tick 结束**（`markNotTicking` → `onRegionRelease`）[S3, S5]

1. 处理 `expectingMergeFrom`，把等着并入的 region 收进来。
2. 自己还要并入别的 ticking region：转 `transient`，返回。
3. 满足下面任一条就删除 dead section：刚发生过 merge；所有 section 都 dead；section 数 ≥ 8 且 dead 比例 ≥ 1/6。
4. 确实删过 dead section 才尝试拆分：以 `max(merge, create)` 为半径对所有 section 做 BFS，得到多个连通块就拆。

**split** [S5 `TickRegionData.split`、`RegionizedWorldData.REGION_CALLBACK.split`]

- 新 region 继承 `currentTick`、redstone time 和调度时间，所以 deadline 不用调整 [S2]。
- 数据按位置分配：`regionToData.get(sectionKey)`。连接跟随玩家的位置。
- block event、block entity ticker 这类没有 holder 保证的项，找不到目标就丢弃。

**多近才算一个 region** `[推导]`：`addChunk` 的搜索半径是 2 个 section，别的 region 的空 section 又向外延伸 1 个。所以两个非空 section 的 Chebyshev 距离（x、z 差值的较大者）≤ 3 时一定合并，≥ 4 才可能独立，拆分时的 BFS 给出同样的结果。默认配置下，两个独立 region 的已加载区块之间至少隔 48 个区块（768 格）。设每名玩家周围的 holder 半径为 r 区块，两人的区块坐标差要达到 2r+49 才可能分属不同 region，达到 2r+64 才保证邻接规则不再把它们连在一起。r = 10 时是 69 到 84 个区块，约 1100 到 1350 格。两人原本在同一 region 时，走远了也不会马上分开：split 要等上文的 dead section 条件触发。ticket 会向外传播，holder 的实际半径应该比视距大，所以这是下界，准确值 UNVERIFIED。

### 2.4 什么在并行

一个 region 的 tick 就是一次完整的服务端 tick [S5 `MinecraftServer.tickServer`、`tickChildren`、`ServerLevel.tick`]。顺序是：

1. `drainTasks()`，执行本 region 的任务队列；
2. 处理本 region 所有玩家的包；
3. region scheduler；
4. entity scheduler；
5. `level.tick()`；
6. `tickConnections()`。

| 并行单元 | 说明 |
| --- | --- |
| region tick | 线程池 `Folia Region Scheduler Thread #n`。默认调度器 EDF，可选 `WORK_STEALING` [S5 `TickRegionScheduler`；S7]。 |
| global tick | 同一个线程池里的一个特殊 handle，不是独立线程 [S5 `RegionizedServer.init`]。 |
| Netty IO、chunk 系统 IO、chunk worker | 独立线程，FAQ 分别给了配额 [S4]。 |
| 插件 async 任务 | `AsyncScheduler`，和 tick 无关 [S13]。 |

- 不同 world 的 region 互不相干，都在同一个池子里调度 [S5]。
- 每个 region 自己维护 deadline，A 落后不影响 B [S2]。
- tick 结束时 `scheduledStart = max(tickEnd, 理想时间)`，防止超时的 region 永远排在最前 [S5 `RegionScheduleHandle.runTick`]。
- 默认线程数 [S5 `TickRegions.getTickThreads`；S7 默认 `threads = -1`]：`threads <= 0` 时取 `total = OSNuma.getNativeInstance().getTotalCores()`。`total/2 <= 4` 时 1 个线程，否则 `total/8`（整数除法）。`total` 是逻辑 CPU 数，不是物理核：没有 NUMA 库时是 `availableProcessors()` [S9 `OSNuma.NoOp`]；Linux 路径是 `numa_num_possible_cpus()` 大小的 `coreToNuma` 数组 [S9 `LinuxNuma`]，`MoonriseConstants` 的日志把这个数叫 "virtual cores" [S8]。所以 16 个逻辑 CPU 得 2 个线程，32 个得 4 个，一台 16 物理核、32 线程的机器默认是 4 个。
- 每个 world 一把 `StampedLock`。`tryMarkTicking`、`markNotTicking` 和慢路径的 `addChunk` 都拿写锁，读 region 走乐观读 [S5]。

### 2.5 Global region

- 只有一个，总是以 20 TPS 调度，不参与 merge 和 split，不拥有 region 数据 [S2]。
- 文档列出的职责：game rules、global game time、daylight time、console commands、world border、weather [S2]。
- 代码里还有：clock、服务器状态采样、登录和配置阶段的连接、睡眠检查、raid 全局部分、sky brightness、`PlayerList.tick()`、地图保存 [S5 `RegionizedServer.globalTick`]。
- region 在 tick 开始时拿到 game time 的拷贝（`WorldLevelData.nonRedstoneGameTime`，`updateTickData`），保证一次 tick 内两次 `getGameTime()` 结果相同 [S2, S5]。S2 说 daylight time 也这样拷贝，这个 commit 的代码没有，见第 2.9 节的对照表。

### 2.6 跨 region 访问

| 机制 | 寻址方式 | 何时执行 | 要点 |
| --- | --- | --- | --- |
| `RegionizedTaskQueue` | 区块位置 | 目标 region 下次 tick | 入队时加 `TASK_QUEUE_TICKET`，目标 region 不存在也会被创建。队列已死就重试 [S5]。 |
| `EntityScheduler` | 实体 | 拥有实体的 region，最快下一 tick | 任务跟着实体走。`run` 和 `retired` 回调只执行一个 [S8]。 |
| global 队列 | 无 | global tick | 改 game rule、天气、时间这类数据 [S2]。 |
| 玩家的 `PacketProcessor` | 玩家 | 拥有玩家的 region | 每名玩家一个队列，region 变化时队列不用搬 [S5 `PacketUtils`；S7 `CraftPlayer.schedulePacket`]。 |
| `Connection.send` | 玩家 | 任何线程 | 进 `MultiThreadedQueue`，CAS 保证同一时刻只有一个线程 flush [S5]。 |
| 异步 teleport | 目标位置 | 分步 | 先把实体从世界移除（transform），再向目标位置投递任务（async place）。传送途中实体不在世界里，shutdown 要专门处理 [S2]。 |

region 内部数据放在 `RegionizedData`，merge 和 split 时由 callback 搬运。插件 API 没有开放它，因为 callback 持有关键锁 [S2]。

### 2.7 禁止事项

- 一个 region 的代码访问或修改另一个 region 的数据。结果是数据损坏："sharing of data will cause data corruption" [S1]。
- 违反所有权时运行时失败：`TickThread.ensureTickThread` 记录错误并抛 `IllegalStateException` [S8]。`setBlock`、`getEntities`、`LevelTicks.schedule` 都有这个检查 [S5]。
- 跨 region 同步加载区块：`syncLoad` 有 `ensureTickThread` 检查 [S5 `ServerChunkCache`]。
- `Entity#teleport`，"will NEVER UNDER ANY CIRCUMSTANCE come back"，只能用 `teleportAsync` [S1]。
- 在 regionizer callback 里阻塞或读写世界状态；callback 抛异常会让服务端进入不可恢复状态 [S5 `ThreadedRegionizer`、`RegionizedData`]。
- 插件没有声明 `folia-supported: true` 就不加载 [S1]。

### 2.8 保证

- ticking region 的线程对它拥有的 entity/chunk/POI 数据有独占写权限。这是唯一的线程安全保证，和插件数据无关 [S1, S2]。
- 同一个任务的 `runTick` 和 `runTasks` 不会并行 [S9 `SchedulableTick`]。
- 线程池没占满时，tick 不超过 50ms 的 region 能保持 20 TPS [S2]。
- merge 和 split 后相对 deadline 不变 [S2]。
- 一次 tick 内 game time 的读取结果稳定 [S2, S5]。S2 对 day time 的同样说法，代码不支持，见第 2.9 节的对照表。
- 事件在所属 region 上调用。事件源周围约 8 个区块一般可以假定归同一 region，但这不是硬保证 [S1]。

### 2.9 已知限制和不兼容

- 插件兼容性预期是 0。broken API：大部分 portal、respawn、部分登录 API，全部 scoreboard API，world load/unload [S1]。
- 玩家都挤在一起时只有一个 region，也就只有一个线程在 tick `[推导]`，这正是 Folia 只推荐给分散玩家的原因 [S1, S12]。
- 任何 region tick 抛异常，整个服务端停机（`regionFailed` → `halt` → `stopServer`）[S5]。
- 每 world 一把锁的竞争：Spottedleaf 在 #291 确认有改进空间，但用 `ReentrantAreaLock` "would greatly complicate the implementation"，建议用较大的 grid exponent，他举的例子是 4 或 5 [S15]。现在的默认值已经是 4（第 2.2 节）。
- 第三方报告：实体移动到 region 外抛异常（#199）、AI 访问 region 外方块（#203）、红石机器触发跨线程实体访问（#334）[S15]。它们都处于 open 状态，但都针对旧版本（1.20.4、1.21.4），26.3 是否复现 UNVERIFIED。

**文档和代码不一致的地方**（以代码为准）

| 文档 | 代码 |
| --- | --- |
| S2 说调度器是 `SchedulerThreadPool` | 现在是 `EDFSchedulerThreadPool` 和 `StealingScheduledThreadPool` [S5, S9]。 |
| S2 说调度不感知 NUMA | 默认 EDF 确实不感知 [S9]，`WORK_STEALING` 加 `MoonriseConstants.NUMA_ENABLE` 时按 NUMA 节点分配线程 [S5]。 |
| S2 说 region 在 tick 开始时拷贝 game time 和 daylight time | `WorldLevelData` 只有 `nonRedstoneGameTime` [S5 `RegionizedServer`、`ServerLevel.updateTickData`]。26.3 的日间时间是 clock：`Level.getOverworldClockTime` 直接读 `ServerClockManager` 里的 `totalTicks`，这个值由 global tick 的 `tickClocks` 更新 [V `Level`、`ServerClockManager`；S5 `RegionizedServer.tickClocks`]。`[推导]` 所以 region 一次 tick 内读两次 clock，中间可能被 global tick 改过。 |
| S3 说 set X 只有 1 个 region 时直接把 section 加给它 | 只有这个 region 不在 ticking 时才这样。它在 ticking 就新建 region，再 merge later [S5 `addChunk`]。 |
| S4 列出 Ride、Rotate 被禁用 | 这个 commit 仍然注册它们，`/ride` 跨 region 时报错 [S5 `Commands`、`RideCommand`]。 |
| S4 没列 Compute、Dialog、Test、Waypoint | 这个 commit 注释掉了它们的注册 [S5 `Commands`]。 |
| S1 说 patch 在 `./patches` | 实际在 `folia-server/` 和 `folia-api/` 下 [S10]。 |

---

## 3. Folia 改变了哪些 vanilla 行为 `[事实]`

### 3.1 基线：vanilla 26.3 的 tick [V]

```text
runServer 循环
├─ processPacketsAndTick
│  ├─ packetProcessor.processQueuedPackets()      全局 FIFO，每 tick 一次，只在开头（聊天等例外见下文）
│  └─ tickServer → tickChildren
│     ├─ 所有玩家 suspendFlushing
│     ├─ functions.tick，clockManager.tick
│     ├─ tickCount % 20 == 0 时 forceGameTimeSynchronization
│     ├─ 每个 ServerLevel.tick
│     │  ├─ world border，weather，睡眠，sky brightness，tickTime（gameTime + 1）
│     │  ├─ blockTicks，fluidTicks                按 (priority, subTickOrder) 串行
│     │  ├─ raids
│     │  ├─ chunkSource.tick
│     │  │  ├─ 清理 ticket，distance manager 更新
│     │  │  ├─ tickChunks（随机 tick、刷怪）
│     │  │  ├─ broadcastChangedChunks             方块变化在这里发出
│     │  │  ├─ chunkMap.tick()                    每名玩家 updateChunkTracking，再跑 entity tracker
│     │  │  └─ chunkMap.tick(haveTime)            POI，卸载区块
│     │  ├─ runBlockEvents
│     │  └─ entities，blockEntities，entityManager
│     ├─ tickConnection                           每连接 Connection.tick → ServerGamePacketListenerImpl.tick
│     │                                           先发 BlockChangedAck，再 player.doTick，再 keepalive
│     │                                           Connection.tick 末尾 channel.flush()
│     ├─ playerList.tick
│     └─ 每名玩家 chunkSender.sendNextChunks，resumeFlushing（再 flush 一次）
└─ waitUntilNextTick                              runAllTasks，事件循环任务队列在这里消费
```

- `processQueuedPackets` 在服务端只有这一个调用点 [V `MinecraftServer.processPacketsAndTick`]。vanilla 的 `pollTask` 不碰它，所以进了 `PacketProcessor` 的包只在 tick 开头处理。Paper 的 `pollTask` 会在 tick 之间处理这些包，这是 Paper 的改动，不是 vanilla 行为 [V `MinecraftServer.pollTask`；S5 `MinecraftServer` 里被删除的那行]。
- 不是所有入站包都进 `PacketProcessor`。`Connection.channelRead0` 在 Netty 线程上直接调 `packet.handle`，只有 handler 开头调用 `ensureRunningOnSameThread` 的包才转进队列 [V `Connection`、`PacketUtils`]。M2 里碰世界的包都走这条路（移动、挖掘、放置、切换手持物品、创造模式物品栏、`ChunkBatchReceived`、`ClientTickEnd`）。例外有两类 [V `ServerGamePacketListenerImpl`、`ServerCommonPacketListenerImpl`]：
  - `handleKeepAlive`、`handlePingRequest`、`handleChatAck` 就在 Netty 线程上跑完。
  - `handleChat`、`handleChatCommand`、`handleSignedChatCommand` 先在 Netty 线程上校验（`tryHandleChat`，`handleChat` 还有 `unpackAndApplyLastSeen`），再用 `server.execute` 进事件循环任务队列。这个队列由 `waitUntilNextTick` 的 `runAllTasks` 和 `managedBlock` 消费，也就是两次 tick 之间。聊天广播再经 `chatMessageChain`（`FutureChain(server)`）投递一次，走的是同一个队列 [V `BlockableEventLoop`、`FutureChain`]。
- 所以聊天和移动包之间没有 FIFO 关系：先发移动、后发聊天，聊天可能先生效 `[推导]`。这段时间不在 `suspendFlushing` 里，聊天广播发出后立刻 flush [V `ServerCommonPacketListenerImpl.send`]。
- `suspendFlushing` 不是"每 tick 只写一次"。tick 开头处理包时还没暂停，发出的包立刻 flush。暂停之后，`Connection.tick` 和 `resumeFlushing` 各 flush 一次 [V]。
- 落后时 `nextTickTimeNanos += 50ms`，不等待，连续补 tick。落后超过 1s + 20 tick（约 2s），且距上次警告满 10s + 100 tick 时，才警告并跳过 [V `runServer`]。

### 3.2 时间和 tick

- **Current Tick**：每个 region 一个。
- **Game Time** 拆成两个：**Redstone Time** 每个 region 一个；**Global Game Time** 和 Daylight Time 由 global region 维护 [S2]。
- `createTick` 和 `LevelTicks.tick` 用 Redstone Time，`getGameTime()` 返回 tick 开始时的拷贝 [S5 `LevelAccessor`、`Level`、`ServerLevel.tick`]。
- 拆开的原因：世界总天数要有个计数；大量实体 AI 用 game time 存绝对 deadline，全部调整工作量太大 [S2]。
- **落后时不补 tick** `[推导]`：region 的 `tickRegion(tickCount, ...)` 忽略 `tickCount`，只跑一次 `tickServer`，然后 `tickSchedule.advanceBy(tickCount)` 把错过的周期全部跳过 [S5 `RegionScheduleHandle.runTick`、`ConcreteRegionTickHandle.tickRegion`；S9 `Schedule`]。global tick 却把 `tickCount` 加到 game time 上 [S5 `tickTime`]。所以 region 的 TPS 不超过 20，丢掉的 tick 不补，Redstone Time 会落后 Global Game Time。vanilla 是追赶（第 3.1 节）。
- `/tick`、`pause-when-empty` 不工作：`tickRateManager.tick()` 被注释，empty 暂停被 `if (false && ...)` 关掉 [S5 `tickServer`；`Commands`]。
- 时间同步包改成每名玩家按 `(tick + id) % 20` 错开发送 [S5 `forceGameTimeSynchronization`]。

### 3.3 红石和顺序敏感的逻辑

| 场景 | 行为 |
| --- | --- |
| 同一 region 内 | 仍是单线程，`LevelTicks`、`CollectingNeighborUpdater`、wire handler、block event 列表都移到 `RegionizedWorldData`，逐 region 一份 [S5]。没有文档声称它和 vanilla 逐 tick 完全一致，UNVERIFIED。 |
| 不同 region 之间 | 没有公共时钟，也没有先后顺序保证。各自的 deadline 和 Redstone Time 独立，lag 时相位漂移 [S2]。#469 的报告者观察到两个维度的红石时钟快慢不同 [S15]。 |
| merge | 相对 deadline 不变（hopper cooldown、burnout 列表都按 offset 调整）[S2, S5]。但 entity、block entity 列表是 append：`from` 的元素接在 `into` 后面 [S5 `merge`]，迭代顺序不再是全局插入顺序 `[推导]`。 |
| split | block event 和 ticker 按位置分配，没有目标就丢弃 [S5]。 |
| 区块边界 | patch 0004：block update 涉及未加载或不归自己的区块时直接跳过。判断用 25 格余量，tripwire 用 4 格。commit 说明承认这引入了 "explicit block update suppression techniques"，但比服务端崩溃好 [S6]。vanilla 的 `getBlockState` 会同步加载区块 [V `Level.getBlockState`、`LevelReader.getChunk`、`ServerChunkCache.getChunk`]。 |
| 命令方块 | `BaseCommandBlock.performCommand` 直接返回 `false` [S5]。社区成员在 #473 说这是有意的，#39 里有 PaperMC member 确认没有开启办法 [S15]。 |
| 数据包函数 | `getFunctions().tick()` 被注释，注释写 `TODO Purge functions` [S5]；`/function`、`/schedule` 未注册 [S5]。 |

### 3.4 网络包时序

- 每名玩家一个队列，tick 开始时逐个玩家清空 [S5 `tickServer`]。
- 默认的 EDF 调度器不支持 tick 之间的任务：`notifyTasks` 是空实现，类注释写明 [S9]。所以进 `PacketProcessor` 的包只在 tick 开头处理，这一点和 vanilla 26.3 相同。`WORK_STEALING` 调度器才会在 tick 之间调用 `drainOnePacket` [S5 `TickRegions`；S9 `StealingScheduledThreadPool`]。
- 聊天和命令不走这条路，而且两者不同。Paper 给 `tryHandleChat` 加了 `sync` 参数：普通聊天 `sync=false`，在 Netty 线程上直接运行，`chatMessageChain` 用 `server.chatExecutor`，完全不进 tick；命令 `sync=true` [S8 `ServerGamePacketListenerImpl.java.patch`]。Folia 的 `MinecraftServer.execute` 直接抛异常，所以把命令这个分支换成玩家的 `EntityScheduler`，延迟 1 tick，在 region tick 的第 4 步执行（第 2.4 节）[S5 `MinecraftServer`、`ServerGamePacketListenerImpl`]。vanilla 的聊天和命令都在两次 tick 之间执行（第 3.1 节）。
- 差别在顺序：vanilla 是全局 FIFO，Folia 是"玩家列表顺序 × 每人 FIFO"。#531 报告这在 PvP 中决定谁先生效 [S15]。上述代码路径我核对过，属实；PvP 的实际影响是报告者的说法，issue 尚未 triage。
- #196（旧版本）讨论过包延迟一个 tick 的问题。PaperMC member 解释：region 之间不忙等，无法把任务"立刻"投递到实体的上下文，而且实体不是严格绑定某个 region 的，所以基于 region 的做法也不稳 [S15]。

### 3.5 其他变化

| 项目 | 变化 |
| --- | --- |
| 命令 | 未注册：bossbar、clone、compute、data、datapack、debug、dialog、function、item、loot、reload、return、schedule、scoreboard、spectate、spreadplayers、tag、team、teammsg、test、tick、trigger、waypoint、perf、save-all [S5 `Commands`]。 |
| 团队 | `Entity.getTeam()` 恒返回 `null` [S5]。 |
| 交互范围 | 方块破坏和使用要求目标周围 8 区块都归当前 region，否则忽略 [S5 `ServerGamePacketListenerImpl`]。 |
| 移动 | `areChunksLoadedForMove` 拒绝移动到不归自己的区块 [S5 `ServerLevel`]。这个方法是 Paper 加的 [S8 `ServerLevel.java.patch`]，S5 的 hunk 在方法体内，所以 S5 里搜不到方法名。 |
| 传送门 | 重写：实体和玩家都会创建出口传送门；vanilla 里非玩家实体可能被拒绝，这个逻辑改掉了 [S2]。 |
| 刷怪 | `spawnChunkTracker` 和 cat、patrol、phantom spawner 的下次 tick 按 region 维护。merge 时这三个 next tick 取较大值，split 时拷贝给每个新 region [S5 `RegionizedWorldData`、`DistanceManager`]。 |
| 空世界 | 总是 tick：`ServerLevel.tick` 里的 `isActive` 被改成恒真 [S5]。 |
| 登录 | 连接在进入世界之前归 global region，spawn 位置异步计算 [S5 `PrepareSpawnTask`、`RegionizedServer.isNotOwnedByGlobalRegion`]。 |

### 3.6 目标硬件和用户

- 硬件："Ideally, at least 16 cores (not threads)"。世界建议预生成 [S1, S4]。
- 线程分配是基于约 330 名玩家峰值测试服的粗略估计：每 200 到 300 名玩家配 Netty IO 约 4、chunk IO 约 3、chunk worker 约 2（已预生成时）。减掉这些之后，到总分配达到 CPU 的 80% 之前的余量给 tick 线程，最后 20% 不分配，留给插件和服务端自己的线程 [S1, S4]。
- 用户：玩家天然分散的服务器（skyblock、SMP），玩家数要可观 [S1, S4]。官网的说法："won't be useful for the majority of servers out there and will break most public plugins"，"ideal for servers with very high players counts"，"by no means a drop-in replacement for Paper" [S12]。
- 我没有找到一手的性能基准数字（比如 N 核下多少玩家），UNVERIFIED。

---

## 4. 许可证

**事实**

- Folia：`PATCHES-LICENSE` 是 GPLv3 全文，README 说它覆盖 api 和 server patch。GitHub 识别为 `GPL-3.0` [S1, S10]。项目没有声明 "or later"，上游 Paper 写明是 GPL version 3 [S11]。
- 上游 Paper：GPLv3。作者可以选择把自己的代码放到 MIT，名单在 `LICENSE.md`，不在名单上的按 GPL 算 [S11]。Spottedleaf 不在名单上（`grep` 无结果）。
- Folia 依赖的 LeafPile 根目录 `LICENSE` 是 GPLv3 [S9]。Moonrise 的 GitHub 元数据是 GPL-3.0 [S16]。
- Pumpkin：`Cargo.toml:121` 写 `license = "GPL-3.0"`，`LICENSE` 是 GPLv3 [P]。
- Bamboo：`Cargo.toml` 写 `license = "AGPL-3.0-only"`，`LICENSE` 是 AGPLv3。
- GPLv3 §13 和 AGPLv3 §13 互相允许把两种许可证的作品合并成一个 combined work 并分发。各部分继续受各自许可证约束，AGPL 的网络交互条款作用于组合体 [S10 `PATCHES-LICENSE`；Bamboo `LICENSE`]。
- GPLv3 §0：`modify` 指以需要版权许可的方式 "copy from or adapt" 作品的全部或部分。§2：不分发的情况下，可以无条件地制作和运行 [S10]。

**含义**

| 行为 | 结论 |
| --- | --- |
| 阅读 README、文档、patch | GPLv3 不限制阅读。 |
| 理解算法和不变量，用自己的设计重新实现 | `[判断]` 这是本文推荐的做法。思想和算法一般不受版权保护，这是通用法律常识，不是来自上面的来源，具体边界 UNVERIFIED，本文不是法律意见。 |
| 把 Java 逐行翻译成 Rust | `[判断]` 按 §0 的 "adapt"，这大概率算 modified version，和复制一样处理。不要这样做。 |
| 把 GPLv3 代码复制进 Bamboo | 按 §13 可以合并，但复制的部分仍是 GPLv3：要保留版权和许可声明，`license` 字段不再是单一的 `AGPL-3.0-only`，以后也不能单方面改这部分的许可。`[判断]` 不值得，Java 的并发代码也翻译不到 Rust 的所有权模型上。 |
| 复制 patch 的上下文行 | 那是 Mojang 的反编译代码，项目规则已经禁止。 |
| Pumpkin 的代码 | 和 Folia 一样处理。 |

**`[判断]` 工作方式**：读完之后写自己的设计文档，实现时对照设计文档，不要开着 Folia 或 Pumpkin 的源码写。

---

## 5. Pumpkin 怎么处理并发

本节的简写：`world/mod.rs`、`entity_islands.rs` 在 `crates/pumpkin-core/src/world/` 下；`player.rs` 在 `crates/pumpkin-core/src/entity/` 下；`ticker.rs` 在 `crates/pumpkin-core/src/server/` 下；`schedule.rs` 在 `crates/pumpkin-world/src/chunk_system/` 下；`domain.rs` 在 `crates/pumpkin-scheduler/src/` 下。

### 5.1 线程 `[事实]`

| 线程 | 用途 | 位置 |
| --- | --- | --- |
| `Server-Ticker` | tick 主循环，独立 OS 线程 | `crates/pumpkin-core/src/lib.rs:397`，`crates/pumpkin-core/src/server/ticker.rs:49` |
| tokio runtime | 网络、磁盘、插件事件 | `AGENTS.md:12` |
| rayon `Simulation-*` | tick 内并行，线程数等于 CPU 数 | `crates/pumpkin-core/src/server/mod.rs:292` |
| rayon `Compute-*` | 包和区块的 CPU 工作，`clamp(cpus/2, 2, 8)` | `crates/pumpkin-core/src/server/mod.rs:300` |
| 每个 Level 一个 `Schedule` 线程，加生成线程池 | 区块生成调度，自己持有 DAG 和队列 | `crates/pumpkin-world/src/chunk_system/schedule.rs:157` |

### 5.2 一次 tick `[事实]`

路径：`Ticker::run` → `Server::tick`，整个 tick 在 `simulation_pool.install` 里，先暂停 flush，结束时恢复（`crates/pumpkin-core/src/server/mod.rs:1215`）。世界之间 `worlds.par_iter()`（`:1271`）。

`World::tick`（`crates/pumpkin-core/src/world/mod.rs:1553`）是 fork/join 的流水线：

1. flush 上一 tick 的方块更新，环境，raid。
2. `tick_chunks`（`:2100`）：scheduled block tick、fluid tick、random tick 各用 `par_chunks(32)` 并行（`:2116`、`:2136`、`:2152`）。
3. `players.par_iter()` 调 `Player::tick`（`:1601`）。**玩家的入站包在这里处理**：`Player::tick` 第一行是 `process_inbound_packets`，每人每 tick 最多 64 个包（`crates/pumpkin-core/src/entity/player.rs:3029`、`:3112`）。
4. 实体：规划 island，能并行就并行，否则串行（`world/mod.rs:1636` 到 `:1747`）。
5. entity tracker，block entity `par_chunks(16)`（`:1770`）。

世界数据全部共享：`World` 里有多个 `Mutex`（`:232` 起），`loaded_chunks` 是 `DashMap`（`crates/pumpkin-world/src/level.rs:81`），`ChunkSections` 里是 `RwLock`（`crates/pumpkin-world/src/chunk/mod.rs:106`），实体位置是 `AtomicCell<Vector3<f64>>`（`crates/pumpkin-core/src/entity/mod.rs:1393`）。

### 5.3 做得好的地方

| 点 | 证据 | 对 Bamboo |
| --- | --- | --- |
| tick 在独立 OS 线程，超载逻辑照搬 vanilla 语义 | `ticker.rs:49`、`:151` | `[判断]` 同样这样做（自己实现）。 |
| 游戏逻辑同步在 tick 内，async 只在边缘 | `AGENTS.md:12`，`BlockBehaviour` 等方法是同步的（`AGENTS.md:78`） | `[判断]` 同样这样做。 |
| 不阻塞 tokio worker，CPU 工作走 rayon | `AGENTS.md:124`，`CONTRIBUTING.md:92` | 同样这样做。 |
| 入站用无锁队列，出站有字节上限，tick 内暂停 flush | `player.rs:772`、`crates/pumpkin-core/src/net/java/mod.rs:184`、`:418` | 思路可取。 |
| 区块调度是单一所有者线程加 channel | `schedule.rs:157` | 这是好的 actor 模式。 |
| entity island 以"结果等同于串行"为目标，有保守回退：planning 超预算、出现 global reach、插件监听实体事件时退回串行 | `crates/pumpkin-core/src/world/entity_islands.rs:1`、`:132`、`:697` | 思路值得记住，但现在不需要。 |
| 用 `order.rs` 把并行阶段的结果排回确定顺序 | `crates/pumpkin-world/src/tick/order.rs`、`world/mod.rs:5105` | 同上。 |
| `ExecutionDomain` 的注释把契约写得很清楚：跨域用 owned 输入，引用和锁不能跨挂起点 | `crates/pumpkin-scheduler/src/domain.rs:30` | 这条契约和本文规则一致。 |

### 5.4 问题

| 问题 | 证据 `[事实]` | 评价 `[判断]` |
| --- | --- | --- |
| 共享一切，靠锁保护 | `crates/pumpkin-core/src` 里 `.lock()` 出现在 1539 行，`try_lock()` 138 处；`crates/pumpkin-core` 和 `crates/pumpkin-world` 有 10 个文件用 `DashMap`/`DashSet` | 每加一个功能都要想清楚锁顺序。没有测过竞争开销，代价是否显著 UNVERIFIED。 |
| scheduled tick 的顺序丢失 | `crates/pumpkin-world/src/level.rs:629` 先 `sort_unstable`，然后 `world/mod.rs:2116` 用 `par_chunks(32)` 并行。vanilla 严格串行 [V `LevelTicks`] | 同一 tick 超过 32 个 scheduled tick 时，批与批之间没有顺序，红石类逻辑有风险。只读了代码，没有实测到可见 bug，UNVERIFIED。 |
| 玩家包并行处理 | `world/mod.rs:1601`，`player.rs:3112` | 两名玩家的操作可以同时改世界。这和 vanilla 的全局 FIFO 差得最远。 |
| 包在 tick 中段处理 | `tick_chunks` 在玩家之前（`world/mod.rs:1581`、`:1601`） | 和 vanilla 的 tick 开头不同。 |
| `try_lock` 失败就跳过逻辑 | `player.rs:3147`、`:3164`、`:3180`、`:3189` | 锁竞争变成静默丢逻辑，比如统计少加一次。很难发现。 |
| 死锁历史 | `git log -i --grep=deadlock` 有 29 个提交（不加 `-i` 是 27 个），比如 `381ca7c97`、`17b37c3ef`、`f36baa6f4`、`4254dbcbc` | 这是共享状态加锁的典型成本。 |
| 曾经把逻辑做成 async，又改回来 | 2026-08-26 的 `refactor: from async to sync` 系列（`8ee11b06c` 等） | 提交说明是空的，动机是我的推断：把游戏逻辑放进 async 不划算。Bamboo 不要走这条路。 |
| island 的维护成本 | 精确的 reach 要逐个 mob 和 goal 人工审计，没审计的用更宽的保守估计，代价是 island 变大（`CONTRIBUTING.md:110`）。审计漏报会改变交互顺序（`AGENTS.md:84`，`CONTRIBUTING.md:108`）；越界检查只在 debug 构建，而且只是警告（`entity_islands.rs:812`） | 这是用人工审计换并行，长期负担很重。 |
| region 概念只是占位 | `ExecutionDomain` 有 `Region`，但 "Only Global admission is currently implemented"（`domain.rs:33`）；`pumpkin-scheduler` 只是 workspace 成员，没有 crate 依赖它（`Cargo.toml:12`） | 说明 Pumpkin 自己也在往 Folia 式所有权走，但现有的共享状态很难迁过去。 |
| 线程超配 | `Simulation` 等于 CPU 数，还有 `Compute`、tokio、生成线程池 | 没有测过，UNVERIFIED。 |

另外，`AGENTS.md:14` 说明 Pumpkin 的玩家移动是 client authoritative，这是有意和 vanilla 不同的地方。

### 5.5 对比

| | Folia | Pumpkin | Bamboo 目标 |
| --- | --- | --- | --- |
| 并行单位 | 空间上的 region，长期存在 | tick 内的阶段，每 tick 现算 | M2 无。以后按 region |
| 数据访问 | 所有权，运行时检查 | 共享，靠锁和原子 | 所有权，编译期检查 |
| 一个 tick 内的顺序 | region 内串行 | 实体尽量保持，其余不保证 | 串行，和 vanilla 一致 |
| 收益条件 | 玩家分散 | 实体多、island 小 | M2 不追求 |
| 主要代价 | 改 vanilla 行为，复杂的 regionizer | 锁、死锁、顺序风险、人工审计 | 现在只是遵守规则 |

---

## 6. 给 Bamboo 的建议 `[判断]`

本节全部是工程判断，事实依据在前面几节。

### 6.1 要不要参考 Folia

要，但只参考这些：

1. **所有权模型**：region 独占数据，跨 region 只投递任务。
2. **不变量**：特别是 ticking region 不能扩张、region 之间留缓冲。
3. **时间拆分**：调度用的时钟和世界时间是两回事。
4. **全局和局部分离**：global region 和 tick 开始时的快照。
5. **失败策略**：所有权被破坏时要硬失败，不要带病继续。
6. **反面教材**：第 3 节的每一项，都说明有些 vanilla 行为在 regional ticking 下要付出代价。

不参考实现。Folia 的复杂度有一大半来自两件事：改造一份假设单线程的现成代码，以及兼容 Bukkit API。Bamboo 这两样都没有。

### 6.2 为什么 M2 不做 regional ticking

- 用户的目标是先能玩，性能以后再说。M2 的范围里没有 mob、存盘和世界生成，红石也不在范围内。
- Folia 自己说大多数服务器用不上[S12]，而且玩家聚集时收益为零。
- regional ticking 的正确性依赖完整的 chunk ticket 系统，Bamboo 还没有。
- Bamboo 没有任何性能数据。在没有测量的时候设计并行，方向很可能是错的。

### 6.3 现在就采用的规则

每条规则写了 M2 的做法和对应的 Folia 代价，能机械检查的也写了怎么检查。

**R1 世界状态由世界线程独占。**
`Global` 和 `Region` 按值 move 进世界线程，通过 `&mut` 传递。不放进 `Arc`，游戏状态不用 `Mutex`、`RwLock`、`DashMap`、原子类型。
对应：Folia 的 `TickThread` 运行时检查（第 2.7 节）；Pumpkin 的反例（第 5.4 节）。
检查：`mc-bamboo-world` 不依赖 tokio（现状满足，它只依赖 `mc-bamboo-data`）；代码审查时 `Arc<Mutex<...>>` 不能出现在世界状态里。

**R2 `Region` 必须是 `Send`，不用 `thread_local!` 存游戏状态。**
RNG 也归 `Region` 所有。这样以后它才能在线程池里移动。
对应：Folia 把 `Level.random` 换成 `ThreadLocalRandomSource`，把大量逐世界的缓存搬进 `RegionizedWorldData` [S5]。
检查：编译期断言，见第 6.5 节。

**R3 `Global` 和 `Region` 从第一天分开。**

- `Global`：时间、天气、玩家名单、tab list、聊天、控制台命令、每名玩家的 `PlayerLink`。
- `Region`：区块、实体、玩家在世界里的状态、计划任务、待广播的方块变化、RNG。

`Region` 拿不到 `&mut Global`。它只读 tick 开始时的 `TickSnapshot`，把对全局的影响写进 `Vec<GlobalEffect>`，tick 之后由 `Global` 应用。
发给单个玩家的包（区块、方块变化、entity tracker）也要有去处。第 6.5 节的骨架只写了 `Broadcast`，实现时再加一个 `GlobalEffect::Send { player, frames }`，或者让 `Region` 自己持有 `PlayerLink` 的克隆。这里还没有定。
对应：Folia 里 global region 和 `WorldLevelData`。这条成本最低，价值最高。
检查：`Region::tick` 的函数签名。

**R4 跨域只发 owned 消息，互相引用只用 ID。**
实体之间、玩家之间用 `PlayerId`、`EntityId`、`ChunkPos`、`BlockPos`，每次重新查，查不到是正常情况（`Option`）。不在字段里存 `Arc<Player>` 或借用。
对应：`EntityScheduler` 存在的原因就是实体不知道什么时候会换 region；Pumpkin `domain.rs:30` 的契约。

**R5 世界线程永不阻塞。**
没有 `.await`、`block_on`、阻塞 IO。出站用 `try_send`，满了就断开那名玩家，不等待。
对应：Folia 的 callback 约束；Pumpkin `AGENTS.md:124`。
检查：世界线程的代码不依赖 async，`blocking_send` 不出现。

**R6 读方块不隐式加载区块。**
`get_block(pos)` 返回 `Option<BlockState>`，未加载是 `None`。所有调用方都按"可能没加载"写。
对应：vanilla 的 `getBlockState` 会同步加载区块。Folia 的 patch 0004 要把红石和 neighbor update 里的这类读取改成 `getBlockStateIfLoaded`，否则 physics 越过 region 边界会触发线程检查（第 3.3 节）。M2 的超平坦世界可以在 tick 内同步生成，但 API 不能假设这一点。

**R7 区块生命周期只有一个入口。**
只能通过 ticket 增减（玩家视距、强制加载等）。区块的创建和删除各只有一处代码。
对应：Folia 把 regionizer 挂在 `onChunkHolderCreate/Delete` 上（第 2.3 节）。以后 region 的形成只需要在这里加一个 hook。

**R8 每个工作项都带位置。**
计划任务、block event、block entity ticker、待广播的方块变化、延迟任务，都有 `BlockPos` 或 `ChunkPos`。没有"无位置"的 tick 列表。
对应：Folia 的 `split` 对这些工作项全靠 `regionToData.get(sectionKey)` 按位置分配（第 2.3 节）。没有位置的值（redstone time、spawner 的下次 tick）只能拷贝给每个新 region。

**R9 世界时间和调度时钟用不同的类型。**
`GameTime`（全局，存盘）和 `RegionTick`（调度用，归 `Region`）是两个 newtype，不能互相比较。M2 里两者同步增长。不用墙钟时间驱动游戏逻辑。
对应：Folia 事后才拆成 Redstone Time 和 Global Game Time，要逐处修改（第 3.2 节）。

**R10 tick 里迭代的集合有确定顺序。**
用 `Vec`、`BTreeMap` 或 `IndexMap`，不迭代 `HashMap`。不在 tick 内用 rayon 处理顺序敏感的列表。纯函数可以放到别的线程（区块编码、光照、生成）。
对应：Pumpkin 的 scheduled tick 问题（第 5.4 节）；`CONTRIBUTING.md` 也指出无序迭代会改变 tie-break。

**R11 全服遍历只在 `Global` 里。**
`Region` 里的逻辑不写"遍历所有玩家"，要写成"看得见这个区块的玩家"。全服广播（聊天、tab list）走 `GlobalEffect`。
对应：Folia 把 `level.players()` 大量替换成 `getLocalPlayers()`（第 2.1 节的 407 个文件有一部分就是这个）。

**R12 tick 内 panic 是全局致命错误。**
世界线程 `catch_unwind` 之后走关闭流程，不继续 tick，因为状态可能已经不一致。单个连接任务的 panic 只影响这个连接。
对应：`regionFailed`（第 2.9 节）。

### 6.4 队列设计

| 通道 | 方向 | 类型 | 满或断开时 |
| --- | --- | --- | --- |
| inbox | 连接 → 世界 | 一个 `std::sync::mpsc` 无界队列，加每连接一个 `Semaphore` 作为在途配额 | 配额用完时连接任务停止读 socket，TCP 自然 backpressure。`Leave` 不占配额，永不丢失。 |
| 玩家出站 | 世界 → 连接 | 每连接一个有界 `tokio::sync::mpsc::Sender<Bytes>`，世界用 `try_send` | 满了说明客户端读得太慢，断开它。容量按能容忍落后多少 tick 算，建议 5 秒（100 条）。 |
| effects | `Region` → `Global` | 线程内 `Vec<GlobalEffect>` | 不涉及线程。 |
| 界面事件 | 世界 → 桌面端 | 现有的 `broadcast` | 丢了没关系。 |

理由：

- 一个共享 inbox 对应 vanilla 的全局 FIFO `PacketProcessor` [V]，所以 M2 碰世界的包（第 3.1 节列的那些）在玩家之间按到达顺序处理，不会有 Folia 的 #531 问题。
- 这只是这部分包的顺序一致，不是所有包。`[判断]` M2 把聊天也放进 inbox，这是有意的简化，和 vanilla 不同：vanilla 的聊天走事件循环任务队列，在两次 tick 之间执行，和移动包之间没有 FIFO 关系（第 3.1 节）。Bamboo 里聊天在 tick 开头处理，tick 结束时才发出，最多多等约一个 tick。这个差别协议上看不出来，所以不值得单独做一条队列。
- tokio 文档建议 async 到 sync 的无界通道用标准库或 crossbeam，并说明非 async 方法可以在 runtime 外调用 [T `mpsc/mod.rs`]。
- 无界队列的风险由配额兜住：在途的 `Request` 总数不超过 `玩家数 × 配额`。`Join` 和 `Leave` 每条连接各只有一条。
- `Leave` 走无界队列，`std::sync::mpsc::Sender::send` 不阻塞，所以 `PlayerSession::drop` 可以直接发。这保住了 `players.rs` 现在的保证：连接无论怎么结束都不留下幽灵玩家。
- 世界线程先 `try_iter().collect()` 再处理：配额要到处理时才释放，所以取消息这一步一定会结束。

**keep-alive 留在连接层。** 现有的 `connection/play.rs` 已经这样做，vanilla 的 `handleKeepAlive` 和 `handlePingRequest` 也不进 `PacketProcessor`，在 Netty 线程上处理 [V]。它不读写世界状态，所以世界线程短暂卡顿不会造成误踢。

这有一个边界，实现时必须处理。配额用完后连接任务停止读 socket，keep-alive 回复就排在未读的数据后面，读不到。现有代码在下一次 `keep_alive.tick()` 时发现 `pending_keep_alive` 还在，就会以超时断开（周期默认 15 秒）。所以世界线程卡得足够久时，所有在移动的玩家都会被误踢。`[判断]` 等配额期间要暂停这个超时判定，或者把 keep-alive 的截止时间往后推。

**decode 留在连接层。** 对应 vanilla 在 Netty 线程解码。世界线程收到的是类型化的请求。非法包在连接层就断开。

**以后的变化**：有了多个 region，包要路由到玩家所属的 region，region 变化时队列要跟着走。到那时改成每名玩家一个队列，并给消息加全局递增的 `arrival` 序号，region 在 tick 开始时按序号合并。这样既有 Folia"队列跟着玩家走"的好处，又避开 #531。M2 不需要做。

### 6.5 世界线程骨架

下面的代码节选自一个 scratch crate，它在 rustc 1.96、tokio 1.54.1 下编译通过，两个测试通过（聊天送达且配额归还；不读数据的客户端不会卡住 tick）。复审时用 rustc 1.99.1 重跑，两个测试仍然通过。这是说明类型关系的骨架，不是最终实现，省略了 vanilla 超载跳过、发给单个玩家的包等细节。

```rust
/// 连接层的写入口。可以克隆到任何线程，但只能用来发字节。
#[derive(Clone)]
pub struct PlayerLink {
    frames: mpsc::Sender<Bytes>,
}

pub enum Inbound {
    Join { player: PlayerId, link: PlayerLink },
    /// `quota` 在世界线程处理完这条消息后释放，连接任务才能继续读 socket。
    Request { player: PlayerId, request: Request, quota: OwnedSemaphorePermit },
    /// 不占配额，所以连接无论怎么结束都能送达。
    Leave { player: PlayerId },
}

// Region 必须能在线程之间移动，日后才能交给线程池。
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<Region>();
};

/// 世界线程的主循环。不是 async，也不持有任何锁。
pub fn run(inbox: std_mpsc::Receiver<Inbound>, stop: CancellationToken) {
    let mut global = Global::default();
    let mut region = Region::default();
    let mut next_tick = Instant::now();

    while !stop.is_cancelled() {
        next_tick += TICK;

        // 对应 processQueuedPackets。先全部取出再处理：配额要到处理时才释放，
        // 连接任务补不进新消息，所以这一步一定会结束。
        let batch: Vec<Inbound> = inbox.try_iter().collect();
        let mut effects = Vec::new();
        for message in batch {
            match message {
                Inbound::Join { player, link } => {
                    global.links.insert(player, link);
                    region.players.insert(player, (0.0, 0.0));
                }
                Inbound::Request { player, request, quota } => {
                    region.handle(player, request, &mut effects);
                    drop(quota);
                }
                Inbound::Leave { player } => {
                    global.links.remove(&player);
                    region.players.remove(&player);
                }
            }
        }

        global.game_time.0 += 1;
        let snapshot = TickSnapshot { game_time: global.game_time };
        region.tick(snapshot, &mut effects);
        global.apply(effects);

        // 每名玩家每 tick 最多写一次。思路来自 vanilla 的 suspendFlushing/resumeFlushing，
        // 但 vanilla 每 tick 不止写一次，见第 3.1 节。
        for player in global.flush() {
            global.links.remove(&player);
            region.players.remove(&player);
        }

        // 绝对截止时间，sleep 的误差不会累积。
        let now = Instant::now();
        if next_tick > now {
            thread::sleep(next_tick - now);
        }
    }
}
```

世界线程用 `std::thread`，不用 tokio task。tick 是 CPU 工作，放在 tokio worker 上会和网络任务互相拖累。Pumpkin 也是独立线程 [P `lib.rs:397`]。

**tick 内的顺序以第 3.1 节为基线。** M2 用到的子集：

1. 处理 inbox（入站包、Join、Leave）。
2. 每 20 tick 同步时间。
3. game time + 1。
4. ticket 更新和区块加载。
5. 广播方块变化。
6. 更新每名玩家的 chunk tracking，然后 entity tracker（vanilla 的 `chunkMap.tick()` 就是这个顺序）。
7. 卸载区块。
8. 发 `BlockChangedAck`，然后玩家 tick。
9. 发送区块批次。
10. flush。

其中"方块变化在 ack 之前发出"是 vanilla 的顺序 [V `ServerChunkCache.tick` 在 `ServerGamePacketListenerImpl.tick` 之前]。

**M2 功能的归属**

| 功能 | 归属 | 说明 |
| --- | --- | --- |
| 聊天 | `Global` | 不碰区块和实体。`Region` 只把它转成 `GlobalEffect`，或由世界线程在 `Global` 阶段直接处理。 |
| tab list，时间同步 | `Global` | |
| 破坏和放置方块 | `Region` | 只影响附近区块的观察者。 |
| 区块跟随玩家 | `Region` | 走 ticket（R7）。 |
| 玩家互相可见 | `Region` | Folia 的 tracker 也只在 region 内（`trackerEntities`、`getLocalPlayers`），不归本 region 的玩家在 `TrackedEntity.updatePlayer` 里直接算不可见 [S5 `ChunkMap`]。`[推导]` 两名玩家在彼此视距内时 holder 连通，按第 2.3 节会归入同一 region，所以正常情况下看不到这个分支。没有来源直接声明这一点。 |
| Join、Leave | `Global` 加 `Region` | 消息形式，tick 开头生效，不在 tick 中途改列表。 |

**和现有代码的关系**

- `Shared.players`、`Shared.status` 可以留着，它们不是游戏状态。`Shared.status` 是服务器列表的响应缓存。`Shared.players` 除了给桌面端看，还是登录准入表：`add_player` 在这里判断重复登录和满员。这发生在连接任务里，要当场给出结果，所以 `[判断]` 不适合挪进 `Global`。Folia 的 `connectionsStateLock` 保护的也是这种表：`connectionByName`、`connectionById` 查重复登录，`usersCountedAgainstLimit` 算人数上限 [S5 `PlayerList`]。
- `PlayerSession` 留在连接层，`Drop` 时多发一条 `Leave`。`PlayerId` 要用每次登录新分配的 ID（比如现有的 `entity_id`），不能用 UUID。否则同一账号快速重连时，旧连接的 `Leave` 可能排在新连接的 `Join` 后面，把新玩家删掉。
- `connection/play.rs` 的循环要改成同时处理"读 socket"和"世界发来的字节"。每条连接只有一个写入方，所以不需要 Folia 的 `flushingQueue`。

**Folia 概念到 M2 的对应**

| Folia | Bamboo M2 |
| --- | --- |
| region | `Region`（只有一个） |
| global region | `Global`，世界线程里的一个阶段 |
| `RegionizedWorldData` | `Region` 的字段 |
| `WorldLevelData` | `TickSnapshot` |
| `Connection.send` 线程安全 | `PlayerLink` |
| `TickThread` 检查 | `&mut` 和 `Send` |
| `regionFailed` | R12 |
| `onChunkHolderCreate/Delete` | R7 的入口 |
| `PlayerList.players` 和 `localPlayers` | `Global.links` 和 `Region.players` |

### 6.6 Folia 里现在不要做的部分

| 机制 | 为什么不做 | 什么时候再看 |
| --- | --- | --- |
| `ThreadedRegionizer`（section、merge later、dead section） | 只有一个 region 时全部是空转 | 阶段 C |
| `TickRegionScheduler`、EDF、work stealing、NUMA | 只有一个线程 | 阶段 C |
| `RegionizedData` 的 merge/split callback | R8 已经保证以后能写 | 阶段 C |
| `RegionizedTaskQueue` | 没有跨 region 的目标。需要延迟任务时用 `Region` 里一个带位置的 `Vec` | 阶段 C |
| `EntityScheduler` | M2 没有需要跨 tick 跟踪的实体任务 | M5 之后 |
| 两步 teleport 和 shutdown 处理 | M2 没有跨 region 传送 | 有多维度时 |
| 每 region 的 Redstone Time 和 offset | R9 的两个 newtype 已经够了 | 阶段 C |
| 独立调度的 global tick | 一个阶段就够 | 阶段 C |
| 运行时线程检查 | 借用检查器做这件事。阶段 C 可以加 `debug_assert!` | 阶段 C |
| 多写者的 flush 锁 | 每条连接只有一个写入方 | 不需要 |
| 插件调度 API | 没有插件系统 | 有插件时 |
| Bukkit 兼容带来的所有东西 | 不适用 | 不需要 |

### 6.7 升级路径

1. **阶段 A（M2 到 M5）**：单世界线程。把纯函数搬到线程池，结果走队列回世界线程：区块生成、存盘 IO、光照、区块编码。Folia 的 chunk 系统也有独立的 IO 和 worker 线程 [S4]。这一步不改变 tick 内的顺序。
2. **阶段 B（有下界和末地之后）**：一个维度一个 `Region`，各跑一个线程。不需要 regionizer。vanilla 里维度是串行的，所以跨维度的顺序会变，但只有传送门这类交互能观察到。这是第一个真正的并行，也是性价比最高的一步。
3. **阶段 C（有明确证据时）**：真正的 regional ticking。

进入阶段 C 的条件（三条都满足）：

- 普通优化之后，目标玩家数下单维度 tick 的 p95 仍接近 50ms；
- profile 显示瓶颈是 tick 逻辑本身，而不是 IO、编码或某个算法；
- 玩家确实分散（多组玩家相距 1000 格以上），而且机器有足够的核。

这些阈值是我定的，到时候应该按实际目标调整。

Rust 对阶段 C 有帮助。`Region` 按值持有，`ready` 就是它在调度队列里，`ticking` 就是它被 move 到某个 worker 上。别的线程根本拿不到正在 tick 的 region，所以"ticking region 不能被 merge"由类型系统保证。合并是 `fn merge(into: &mut Region, from: Region, tick_offset: i64)`。需要共享的只剩一张 section 到 region id 的映射表。

---

## 7. UNVERIFIED 汇总

| 项目 | 原因 |
| --- | --- |
| Folia 的性能基准 | 一手来源只有"约 330 名玩家峰值的测试服"和"至少 16 核"，没有找到量化的扩展性数据。一次网络搜索没有发现官方公告或基准。 |
| 单个 region 内是否逐 tick 等同 vanilla | 没有文档这样声称，我也没有运行测试。Folia 还继承了 Paper 自己的行为差异。 |
| 玩家周围 chunk holder 的实际半径 | 第 2.3 节的距离是下界，没有读 Moonrise 的 ticket 传播代码。 |
| 彼此视距内的玩家是否一定在同一 region | 第 6.5 节是推导，没有来源直接声明，也没有读 holder 半径。代码只确认了跨 region 的玩家会被 tracker 当作不可见。 |
| region 一次 tick 内读 clock 时值是否真的会变 | 代码里没有 tick 内拷贝是读到的事实，会不会读到不同的值是推导，没有运行验证。 |
| #531 的 PvP 实际影响 | 代码路径我核对过，影响是报告者的说法，issue 尚未 triage。 |
| #199、#203、#334 在 26.3 上是否复现 | 这些报告针对旧版本，或者没有维护者确认。 |
| #473 的"有意为之" | 回复者不是 member。`performCommand` 直接返回 `false` 是我读到的代码事实；是否有意，#39 的 member 回复只说明无法开启。 |
| Pumpkin 的锁竞争、线程超配、并行 scheduled tick 的可见后果 | 按要求没有运行任何服务端，只读了代码。 |
| Pumpkin 从 async 改回 sync 的动机 | 提交说明是空的，这是我的推断。 |
| "思想和算法不受版权保护"的具体边界，以及逐行翻译是否构成衍生作品 | 这是法律问题，不在给定来源内，本文不是法律意见。 |
| 第 6 节的所有阈值和容量（配额、5 秒出站队列、阶段 C 条件） | 工程判断，Bamboo 没有性能数据可以校准。 |
| 配额用完时 keep-alive 被误判超时 | 第 6.4 节的风险是读 `play.rs` 和本文设计推出来的，没有实测，处理办法也是判断。 |
| 发给单个玩家的包走 `GlobalEffect` 还是 `Region` 持有 `PlayerLink` | 第 6.3 节 R3 没有定，骨架没有覆盖这条路径。 |
