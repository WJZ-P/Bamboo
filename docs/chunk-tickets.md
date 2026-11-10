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
5. **不遍历哈希图产生任何输出**：变化列表经 `BTreeMap` 收集，其余列表都显式排序。
   哈希表只用于查找，键是 `ChunkPos`，用模块内自定义的便宜哈希（`ChunkHasher`）。
6. 所有公开类型 `Send`（静态断言测试）。视距等越界输入夹到合法范围，不 panic。
7. 玩家跨一个区块边界只处理两个方形视野的差集（一排进、一排出，视距 10 时 21+21 个区块），
   不重算整个 21×21 视野；视距变化只加/撤两个半径之间的方环，四个角只算一次。
   多玩家时按引用计数决定要不要真的增删票。

### 内部表示（为什么不是官方的 `DynamicGraphMinFixedPoint`）

官方的分级优先队列维护的是同一个不动点。这里换成"两阶段增量"：

- 票（源）按位置存在哈希表里，源变化攒到 `run_updates` 一起处理。
- 等级存成按 64×64 区块分块的稠密数组（`LevelGrid`），邻居绝大多数落在同一块里，
  块内直接下标；没分配过的块就是无票等级。
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
```

200 名玩家、视距 10、每刻每人跨一次区块边界（每刻 200 次移动，连续 200 刻）：

| 指标 | 数值 |
|---|---|
| 每次移动 | **327 µs** |
| 每次移动变等级的区块数 | 平均 1053 |
| 等级表项数（加载 + 模拟） | 547600 |
| `set_player` 部分 | ≈ 5 µs / 次 |
| `run_updates` 部分 | ≈ 322 µs / 次 |

单名玩家对照：`303 µs` / 次，平均 1146 个区块变等级，表 2738 项。

耗时构成（用 `TICKET_PROFILE=1` 临时打点测得，一次移动在两张平面上的量级）：
加载平面约 42 个源变化、840 个清空区块 → 升高锥 BFS ≈ 80 µs、建种子 ≈ 45 µs、
分桶松弛 ≈ 60 µs、收变化列表 ≈ 10 µs；模拟平面约 529 个清空区块，合计约 110 µs；
`run_updates` 里合并两张平面的变化列表另约 55 µs。

**没有达到预期**：主代理给的目标是每次跨区块 50 µs 以内，实测 327 µs，差 6.5 倍。
`set_player` 的差集本身只有 5 µs（设计稿 5.1 的要求达到了），全部开销在等级传播和变化列表合并。
即使玩家数从 1 增到 200，单次移动耗时只从 303 µs 涨到 327 µs，说明它没有退化成重算整个表，
但每次移动要重算约一千个区块的等级，每张平面要过三遍。已把这一项交给主代理复审：
是否接受当前数字，还是换成"外圈等级不落表、按需从票算"或别的表示。
