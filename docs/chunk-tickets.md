# 区块平面之一：ticket 表

`crates/mc-bamboo-world/src/ticket/`。这是 `docs/world-model-design.md` 第 16 节第 3 步里最底下的一块：
给定 ticket 的增减，算出每个区块的**加载等级**和**模拟等级**。它是纯数据结构，不认识线程、
区块内容、IO，也不做加载/生成/卸载；以后的 Director 拿它的输出决定做什么。

所有"官方事实"都对着 26.3 的字节码（`javap -c -p`）读过；有疑问的常量另外在 JVM 里直接调官方类打印过。
讨论稿 `world-model-discussion.md` 2.3 的表原先有三处错，已按字节码更正。

## 1. 官方机制

### 1.1 `TicketType`

来源：`net.minecraft.server.level.TicketType` 的 `<clinit>`（九次 `register(name, timeout, flags)`）
和 `FLAG_*` 常量、`persist/doesLoad/doesSimulate/shouldKeepDimensionActive/canExpireIfUnloaded/hasTimeout`。
`NO_TIMEOUT = 0`，`hasTimeout()` 就是 `timeout != 0`。

| 类型 | 超时（刻） | flags | 存盘 | 加载 | 模拟 | 保持维度活跃 | 卸载时可过期 |
|---|---:|---:|:-:|:-:|:-:|:-:|:-:|
| `player_spawn` | 20 | 2 | | ✓ | | | |
| `spawn_search` | 1 | 2 | | ✓ | | | |
| `dragon` | 0 | 6 | | ✓ | ✓ | | |
| `player_loading` | 0 | 2 | | ✓ | | | |
| `player_simulation` | 0 | 12 | | | ✓ | ✓ | |
| `forced` | 0 | 15 | ✓ | ✓ | ✓ | ✓ | |
| `portal` | 300 | 15 | ✓ | ✓ | ✓ | ✓ | |
| `ender_pearl` | 40 | 14 | | ✓ | ✓ | ✓ | |
| `unknown` | 1 | 18 | | ✓ | | | ✓ |

讨论稿 2.3 那张表错的三处：`dragon` 不带"保持维度活跃"、`player_simulation` 带、
`ender_pearl` 和 `unknown` 都带"加载"。`flag` 位：`PERSIST=1 / LOADING=2 / SIMULATION=4 /
KEEP_DIMENSION_ACTIVE=8 / CAN_EXPIRE_IF_UNLOADED=16`。

### 1.2 等级

来源：`ChunkLevel`（`FULL_CHUNK_LEVEL=33 / BLOCK_TICKING_LEVEL=32 / ENTITY_TICKING_LEVEL=31`）、
`FullChunkStatus`、`ChunkPyramid.GENERATION_PYRAMID` 的 `FULL` 步累计依赖半径、`ChunkStatus`。
在 JVM 里直接调 `ChunkLevel.MAX_LEVEL`、`ChunkLevel.generationStatus(0..46)`、
`ChunkLevel.byStatus(ChunkStatus)`、`ChunkMap.FORCED_TICKET_LEVEL` 核过：

- `RADIUS_AROUND_FULL_CHUNK = 11`，`MAX_LEVEL = 33 + 11 = 44`，无票 = `45`。
- `fullStatus`：`<=31` 实体 tick，`32` 方块 tick，`33` 已加载不 tick，`>=34` 不可访问。
- `generationStatus`：34 `initialize_light`、35 `terrain`、36 `biomes`、37..44 `structure_starts`、
  `>=45` 为 `null`（不加载）；`<=33` 是 `full`。
- `byStatus`：`empty`/`structure_starts`=44，`structure_references`/`biomes`=36，`terrain`=35，
  `features`/`initialize_light`=34，`light`/`spawn`/`full`=33。
- `isLoaded(level) = level <= 44`。
- `ChunkMap.FORCED_TICKET_LEVEL = 31`，`MIN_VIEW_DISTANCE = 2`，`MAX_VIEW_DISTANCE = 32`。

### 1.3 两套传播

来源：`ChunkTracker`、`LoadingChunkTracker`、`SimulationChunkTracker`、
`net.minecraft.world.level.lighting.DynamicGraphMinFixedPoint`、`LeveledPriorityQueue`。

- 加载平面 `levelCount = MAX_LEVEL+2 = 46`（内部上限 45）；模拟平面 `levelCount = 34`（上限 33，
  `SimulationChunkTracker.MAX_LEVEL = 33`，`>=33` 视为无票）。
- 每个位置的**源等级**是 `TicketStorage.getTicketLevelAt(pos, simulation?)`：该位置全部票里带相应
  标志的最小 `ticketLevel`，无票时 `MAX_LEVEL+1`。
- `computeLevelFromNeighbor` 是 8 邻 `+1`（切比雪夫距离），`getComputedLevel` 取 8 邻最小值；
  `checkNeighborsAfterUpdate` 在等级 `>= levelCount-2` 时不再外传。
- `setLevel` 的结果等价于 `level(p) = min over sources (source_level + chebyshev(p, source))`，
  钳到 `[0, limit]`。多张票取最小，撤票后重算。官方用分级优先队列增量维护这个不动点，
  结果与处理顺序无关（不动点唯一）。

### 1.4 票的存储、增删、超时

来源：`Ticket`、`TicketStorage`。

- 票按位置存成列表；`addTicket(long, Ticket)` 若已存在同 `(type, level)` 的票，只
  `resetTicksLeft()`，**不插入第二张**；`removeTicket` 只删第一张同 `(type, level)` 的票。
- `Ticket.decreaseTicksLeft()` 只递减带超时的票；`isTimedOut()` 是 `ticksLeft < 0`。
  所以超时 `N` 的票在第 `N+1` 次 `purge` 才被移除（`N=1` 的 `spawn_search`/`unknown` 在第 2 刻过期）。
- `purgeStaleTickets` 每刻对每个票先问 `canTicketExpire`：
  `hasTimeout() && (canExpireIfUnloaded() || holder == null || holder.isReadyForSaving())`；
  只有为真才递减并判过期。**存在但还没准备好存盘的区块**（正在生成/加载）上，普通超时票
  这一刻不倒计时。`ServerChunkCache.tick` 每刻先 `purgeStaleTickets` 再 `runDistanceManagerUpdates()`。
- `addTicketWithRadius(type, pos, radius)` 的等级是 `33 - radius`；`removeTicketWithRadius` 同理。
- `updateChunkForced(pos, bool)` 加/删 `FORCED, 31` 票；`getForceLoadedChunks()` 是有 FORCED 票的区块。
- `packTickets` 只收 `type.persist()` 的票（`forced`、`portal`）；`Ticket.CODEC` 存
  `type / level / ticks_left`，saved data 名 `chunk_tickets`。
- `replaceTicketLevelOfType(level, type)` 把**该类型的全部票**（不论来源）删掉再按新等级加回。
- `shouldKeepDimensionActive()` 看有没有带 `KEEP_DIMENSION_ACTIVE` 的票。

### 1.5 玩家票

来源：`DistanceManager`、`DistanceManager$PlayerTicketTracker`、
`DistanceManager$FixedPlayerDistanceChunkTracker`、`ChunkMap.setServerViewDistance`、
`ChunkMap.updatePlayerPos`、`ServerChunkCache.tick`。

- `PLAYER_TICKET_LEVEL = 31`，默认 `simulationDistance = 10`，
  `getPlayerTicketLevel() = max(0, 31 - simulationDistance)`。
- `addPlayer(section, player)`：把玩家记进 `playersPerChunk`；更新 `naturalSpawnChunkCounter`
  （半径 8）和 `playerTicketManager`（半径 32+2）；在玩家所在区块加一张
  `player_simulation, 31 - simulationDistance`。`removePlayer` 反着来，且只有该区块最后一个玩家
  离开时才撤源和票。
- `FixedPlayerDistanceChunkTracker` 以玩家所在区块为源做 8 邻传播，得到"到最近玩家的切比雪夫距离"；
  `PlayerTicketTracker.haveTicketFor(level) = level <= viewDistance`；只在跨越 `viewDistance`
  边界时增删 `player_loading, 31` 票。
- **视距是全服一个值**。`ChunkMap.setServerViewDistance(d)` 把 `d` 夹到 `[2,32]`，调
  `DistanceManager.updatePlayerTickets(d)` → `PlayerTicketTracker.updateViewDistance(d)`。
  玩家自己的 `requestedViewDistance()` 只在 `getPlayerViewDistance` 里被夹到 `<= serverViewDistance`，
  用于 `updateChunkTracking`（决定发哪些区块），**不影响票**。
- `updateSimulationDistance(d)` 变了就调 `replaceTicketLevelOfType(31-d, PLAYER_SIMULATION)`。
- **节流**：`player_loading` 票的所有增删都走 `ThrottlingChunkTaskDispatcher`
  （`maxChunksInExecution = 4`，名字 "player ticket throttler"）。玩家刚进区块或刚改视距时，
  官方不是立刻给满整个方形视野：视距 4 的场景实测第 1 刻只有 4 张票，跑完节流才 81 张。
  这是加载调度的节奏，不是票的语义；设计稿 5.4 把优先级和限速归 Director。

## 2. `TicketTable` 的接口和不变量

```text
TicketTable::new()                         视距 10、模拟距离 10
add(ty, pos, level) / remove(ty, pos, level)
add_with_radius(ty, pos, radius) / remove_with_radius(...)   level = 33 - radius
set_forced(pos, bool) / forced_chunks() -> Vec<ChunkPos>
set_player(id: u64, section: Option<ChunkPos>)    None = 离开
set_view_distance(distance)                全服一个值，夹到 2..=32
set_simulation_distance(distance)          夹到 0..=31
tick(&mut dyn FnMut(ChunkPos) -> bool)     is_busy 为真 = 该区块存在且还没准备好存盘
tick_idle()                                全都不忙
run_updates() -> &[LevelChange]             传播完积压的源变化，返回这次变了的区块
loading_level(pos) / simulation_level(pos)
tickets_at(pos) / ticket_positions()
persistent_tickets() -> Vec<SavedTicket> / restore_tickets(...)
keep_dimension_active() / view_distance() / simulation_distance() / level_count()
```

`LevelChange { pos, loading: (旧, 新), simulation: (旧, 新) }`。没变的那个分量也写成 `(新, 新)`，
调用方可以直接拿两个分量用。

不变量：

1. 同一位置不会有两张 `(ty, level)` 相同的票；`add` 只重置超时。
2. 平面的源等级 = 该位置同标志票的最小等级；平面等级 = 8 邻 +1 取最小后的不动点。
3. 每个区块的 `player_loading` 引用计数 = 视距方形覆盖它的玩家数；
   `player_simulation` 引用计数 = 站在该区块的玩家数。多玩家重叠不会误撤票。
4. `run_updates()` 的输出按 `(x, z)` 排序，**恰好**包含等级变了的区块，不多不少。
   同一操作序列跑两遍输出逐字节相同（有单元测试）。
5. **不遍历哈希图产生任何输出**：两平面的变化列表各自按 `(x, z)` 排序，再线性归并；
   清空区按 BFS 发现顺序遍历，其余列表都显式排序。哈希表只用于查找，键是 `ChunkPos`，
   用模块内自定义的便宜哈希（`ChunkHasher`）。
6. 所有公开类型 `Send`（静态断言测试）。视距等越界输入夹到合法范围，不 panic。
7. 玩家跨一个区块边界只处理两个方形视野的差集（一排进、一排出，视距 10 时 21+21 个区块），
   不重算整个 21×21 视野；视距变化只加/撤两个半径之间的方环，四个角只算一次。
   多玩家时按引用计数决定要不要真的增删票。

### 内部表示（为什么不是官方的 `DynamicGraphMinFixedPoint`）

官方的分级优先队列维护的是同一个不动点。这里换成"两阶段增量"：

- 票（源）按位置存在哈希表里，源变化攒到 `run_updates` 一起处理。
- 等级存成按 64×64 区块分块的稠密数组（`LevelGrid`），邻居绝大多数落在同一块里，
  块内直接下标；没分配过的块就是无票等级。一轮更新用的旧等级、BFS 访问标记和清空标记
  也按同样的分块存，用 epoch 判失效，块放进有上界的池子跨轮复用。
- 同一批源变化先按 `(坐标, 到达顺序)` 排序后折叠；各类临时 `Vec` 和分桶队列跨轮复用容量。
- 阶段一：所有等级变大的源一起做逐层 BFS，候选值 `旧源等级 + 切比雪夫距离`。当前等级
  恰好等于候选值的区块原来靠这些源取得等级，把它们清空；等级更低的地方由没变的源撑着，剪枝。
- 阶段二：用按等级分桶的 FIFO 队列（等价于官方 `LeveledPriorityQueue`）从新源、清空区的
  边界值出发做 Dijkstra 式松弛，只有候选值真比当前等级低才继续往邻居走。

结果和官方逐位一致，由 oracle 验证；实现比复刻优先队列简单，且输出顺序可控。

## 3. 和官方有意不同的地方

| # | 官方 | 这里 | 理由 |
|---|---|---|---|
| 1 | `player_loading` 票的增删经 `ThrottlingChunkTaskDispatcher`（每刻 4 个在途），玩家进服/改视距后要几刻才给满视野 | 同步给满，稳定状态逐位一致 | 4 个在途是加载调度参数，不是票的语义；设计稿 5.4 把优先级和限速归 Director。纯 ticket 表不该带一个"推进节流"的概念 |
| 2 | `purgeStaleTickets` 读 `ChunkHolder.isReadyForSaving()` | `tick(is_busy)` 由调用方传入 | 这张表不认识区块内容；Director 以后正好知道哪些区块有在途工作。两个分支 oracle 都覆盖 |
| 3 | `TicketStorage.deactivateTicketsOnClosing` 关服时把非 `unknown` 票移到 `deactivatedTickets` | 没有 | 属于存档/关服流程，不在这一层的范围 |
| 4 | 官方 `ChunkHolder` 在 `isChunkToRemove` 时把等级当成 45 | 没有这个概念 | 属于卸载流程，归 Director |
| 5 | 官方视距是服务端配置，玩家请求值另算 | 一样（全服一个值） | 字节码确认。`set_player` 不带视距参数 |
| 6 | `setSimulationDistance` 不夹输入 | 夹到 `0..=31` | 客户端能控制的值不能让等级变负或溢出，且结果对合法输入完全一致 |

**以后可以做**：按每名玩家各自的视距给票（现在照官方用全服一个值）；外圈"只到某个生成阶段"
的等级不落表、按需从票算（见第 5 节的耗时构成）。

## 4. Oracle

### 挂在哪一层

`tools/mc-bamboo-vanilla/java/TicketOracle.java` 在 JVM 里**直接继承官方 `DistanceManager`**
（三个抽象方法自己实现），配 `new TicketStorage()`；`ChunkMap` 用
`Unsafe.allocateInstance` 跳过构造函数，只填 `updatingChunkMap`（一张铺满 `ChunkHolder` 的表），
并用一个子类覆盖 `getUpdatingChunkIfPresent` 保证任何坐标都有 holder。玩家用
`Unsafe.allocateInstance(ServerPlayer.class)` 造空壳并 `setId()`，只当集合里的身份用。
**不需要 `ServerLevel`，不启动服务端，不接受 EULA。**

两个和真机调度有关的细节：

- 官方把 `player_loading` 票的增删 post 到主线程执行器；同步 `Runnable::run` 会让
  `updateViewDistance` 在写完 `viewDistance` 字段**之前**执行那批任务，`haveTicketFor` 把它们
  全判掉。oracle 用一个攒着任务的 `Deferred` 执行器，在 `settle` 里按 FIFO 放行。
- `settle()` 反复 `runAllUpdates` + 释放在途任务，直到票发完、等级不动，才出这一步的题。
  所以比的是**稳定状态**（见第 3 节第 1 条）。

### 用法

```text
# 先跑过 generate（target/vanilla 里有解压好的官方 jar 和依赖）
cargo run -p mc-bamboo-vanilla -- generate <官方 server.jar>

cargo run -p mc-bamboo-vanilla -- ticket-oracle java \
    --scenes 500 --seed 12345 --show 5
# 单独看一个场景、并打印具体不一致的区块
cargo run -p mc-bamboo-vanilla -- ticket-oracle java \
    --scenes 1 --seed 12345 --scene 44 --detail --show 3
```

Java 侧生成固定种子的随机操作序列：加/撤任意 `TicketType`、带半径的加/撤、玩家进服/离开/
跨区块、视距变化、模拟距离变化、强制加载、推进刻（一半的题让 3 个区块"忙"，覆盖
`canTicketExpire` 的两个分支）。每一步输出 49×49 窗口里全部区块两套等级的 64 位 FNV-1a 哈希
和非默认区块数；`--detail` 再输出全部非默认区块的 `[x, z, loading, simulation]`。
Rust 侧 `ticket_oracle.rs` 重放到 `TicketTable`，逐步比哈希和数量，出错时定位第一个不一致的位置。

### 结果

| 命令 | 步数 | 不一致 |
|---|---:|---:|
| `--scenes 500 --seed 12345` | 5487 | 0 |
| `--scenes 400 --seed 1` | 4364 | 0 |
| `--scenes 400 --seed 999` | 4348 | 0 |
| `--scenes 400 --seed 20260331` | 4345 | 0 |

合计四个种子、1700 个场景、18544 步，两套等级逐位一致。对拍过程中查出的两个真实差异
（视距方环的四个角被重复计数、`replaceTicketLevelOfType` 要换掉全部同类票）都已按字节码修正。

## 5. 基准

```text
cargo run --release -p mc-bamboo-world --example ticket_move_bench -- [玩家数] [刻数]
cargo run --release -p mc-bamboo-world --example ticket_alloc_count -- [玩家数] [刻数]
TICKET_PROFILE=1 cargo run --release -p mc-bamboo-world --example ticket_move_bench -- 200 100
```

200 名玩家、视距 10、每刻**每人**跨一次区块边界，每刻只调一次 `run_updates`，连续 200 刻。
所以"每次移动"是每刻总耗时除以 200：一次 `update` 要处理约 8400 个源变化，
一次合并要合两张平面各约 10 万条变化。单名玩家时每刻一次移动、一次 `run_updates`。

2026-11-11，`aedaa96` .. `b6596f2`（见 `performance-optimizations.md` 第 7e 节）改完，
`ec68ced` 留的旧二进制和当前二进制在同一轮里交替各跑 5 次（200×200），取中位数：

| 指标 | 改之前 | 改之后 |
|---|---:|---:|
| 每次移动，200 名玩家 | **301 µs** | **141 µs** |
| 每次移动，1 名玩家（2 万刻） | **289 µs** | **126 µs** |
| 每次移动变等级的区块数 | 平均 1053 | 平均 1053（不变） |
| 等级表项数（加载 + 模拟） | 547600 | 547600 |
| `set_player` 部分 | ≈ 5 µs / 次 | ≈ 5 µs / 次 |
| `run_updates` 部分 | ≈ 296 µs / 次 | ≈ 136 µs / 次 |

单名玩家时平均 1146 个区块变等级，表 2738 项。玩家数从 1 增到 200 只从 126 涨到
141 µs，仍然没有退化成重算整个表。

### 耗时构成

`TICKET_PROFILE=1` 留在代码里（只在这个环境变量下计时和计数，关闭时只是一次原子布尔判断）。
单位都是 µs/移动，按上面的口径把每刻总时间除以移动次数；打点打开后绝对值会偏慢 5%–10%，
看比例用。改前是 `ec68ced` 上的同一套打点，改后是 `b6596f2`：

| 阶段 | 改之前 | 改之后 |
|---|---:|---:|
| 加载 升高锥 BFS | 61–78 | 18.7 |
| 加载 建种子 | 29–36 | 14.3 |
| 加载 分桶松弛 | 59–77 | 33.6 |
| 加载 收变化列表 | 4–5 | 12.7 |
| 模拟 升高锥 BFS | 34–41 | 13.0 |
| 模拟 建种子 | 22–27 | 11.0 |
| 模拟 分桶松弛 | 19–24 | 13.0 |
| 模拟 收变化列表 | 3–4 | 4.7 |
| 合并两平面 | 69–81 | 3.1 |
| 分流 pending | 0.1 | 0.05 |
| 折叠源变化（两平面共用） | 2.1 | 0.11 |

`sample` 10 秒也印证了改前的热点：`run_updates` 的 `BTreeMap::or_insert` /
`IntoValues`、`Plane::update` 的 `HashMap::insert`（BFS 的 `seen`/`reset`）、
`LevelGrid::set` 和 `VecDeque` 扩容。

改后每次 `run_updates` 还剩多少格被碰（`TICKET_PROFILE=1` 的计数，单位 次/移动）：
BFS 访问 3668 格、清空 1355 格、建种子查邻居 10843 次、松弛入队 5959 次 / 出队 6054 次
（真正写等级 1780 次）、收变化扫描 11973 格、输出 1135 条变化。两张平面每格至少被
BFS、建种子、松弛、收变化各碰一遍，这是这一批改法剩下的主要单价。

### 每次 `run_updates` 的堆分配

`ticket_alloc_count` 用一个计数全局分配器，只数 `run_updates` 那一段（`set_player` 不算）：

| | 改之前 | 改之后 |
|---|---:|---:|
| 200 名玩家：alloc / realloc / dealloc / 拷贝字节 | 37.7 / 406.2 / 37.6 / 28 MiB | 4.3 / 52.2 / 4.0 / 5.5 MiB |
| 1 名玩家：alloc / realloc / dealloc / 拷贝字节 | 38.1 / 182.0 / 38.0 / 146 KiB | 4.1 / 19.0 / 4.0 / 26 KiB |

改之前每轮新建 `folded`/`effective`/`frontier`/`original`/两张结果表和 46 个 `VecDeque`；
现在它们都跨轮复用容量。剩下 4 次主要是两张平面各返回一次 `Vec<(ChunkPos, i32, i32)>` 和
`TicketTable::changes` 的容量增长（只在表变大时）。`realloc` 是这些向量扩容时的搬移。

### 没有达到 80 µs 的目标

主代理给的目标是每次跨区块 80 µs（更好 50 µs），实测 **141 µs / 126 µs**，还差约 1.8 倍。
大批容易摘的果实已经摘完：合并从 70–80 µs 降到 3 µs，BFS 从 60–78 µs 降到 19 µs，
分配从每次 `run_updates` 38 次降到 4 次。剩下的是每个变等级区块的固有单价：约 1053 个
区块 × 两张平面 × 每格数次哈希/数组访问，以及 6000 次松弛出队里只有 1780 次真更新。
试过但**没有可测收益、已撤掉**的做法：把建种子和清空合成一遍、按 (位置, 候选值) 给松弛
队列去重、8 邻等级一次性读、用裸指针缓存块（还破坏了公开类型的 `Send`）。

按主代理的决定，外圈等级继续落表（`ChunkPyramid` 的生成依赖图要用），所以约一千个
区块变等级这个数量是固有的。要再降到 80 µs 以下，得换传播算法的表示而不是继续换容器，
例如让升高锥和降低松弛共用一次遍历、或者把 64×64 块换成更大的连续区域；
这些都会改动两阶段算法的结构，语义是否仍能逐位一致需要重新对拍。见
`performance-optimizations.md` 第 9 节。
