# 世界模型和多线程：探讨稿

> **状态：探讨稿，未定稿。** 整理自 2026-11-09 的讨论。只有第 1 节标“已定”的几条是用户拍板的，
> 其余都是提议、推导或待决定的问题。定稿后另写正式设计，并改掉 `folia.md` 第 6.7 节的升级路径。

目标版本：Minecraft Java 26.3（协议 777）。

标注约定和 `folia.md` 相同：

- `[事实]`：有来源。原版行为的来源是官方 26.3 反编译代码（写类名），外部资料的来源见第 12 节。
- `[推导]`：读代码或代入数字算出来的结论，推导写在原地。
- `[判断]`：工程判断，可以被推翻。
- `UNVERIFIED`：没能确认。

---

## 1. 已定和待定

| 项 | 状态 | 内容 |
|---|---|---|
| 多 region | **已定** | 每个维度从一开始就支持多个 region（Folia 式），不经过“每个维度一条线程”这个中间态 |
| 多 region 模式的原则 | **已定** | 性能优先。多 region 时不追求和原版逐位一致，只要玩家感觉不到明显差别 |
| tick 调度 | **已定方向** | region 各走各的时钟，不齐步走（第 6 节） |
| 单 region 精确模式 | 待确认 | 提议保留：每个维度一个 region、串行，oracle 在这个模式下跑 |
| region 内部也放宽 | 待确认 | 用户在评估“region 内部也尝试并行”（第 10 节），还没定 |
| D2–D9 | 待定 | 第 3 节 |
| 同步设计的七个部件 | 待定 | 第 8 节 |

---

## 2. 原版 26.3 的世界模型 `[事实]`

### 2.1 层次

- 每个维度一个 `ServerLevel`，各有自己的区块表、实体、计划刻、光照引擎。
- `MinecraftServer.tickChildren` 在**同一条主线程**上用一个循环逐个调用 `level.tick()`，全部跑完才 `tickConnection()`。
- `ServerLevel` 连续 300 刻（`EMPTY_TIME_NO_TICK`）没有“保持维度活跃”的 ticket 时，跳过实体 tick。

### 2.2 区块数据

- 区块是一摞 16×16×16 的段。每段用调色板容器存方块（4096 格）和生物群系（4×4×4）。
- 区块上还有高度图（`ChunkStatus` 里分生成期和最终两套）、方块实体、本区块的方块和流体计划刻。
- 生成中的区块是 `ProtoChunk`，完工后是 `LevelChunk`。
- 光照由光照引擎管理，随区块存盘（`BlockLight`、`SkyLight`）。

### 2.3 ticket 和区块等级

`ChunkLevel`：等级越小越强，向外每格加 1。

| 等级 | 含义 |
|---|---|
| ≤ 31 | 实体 tick（`ENTITY_TICKING`） |
| 32 | 方块 tick（`BLOCK_TICKING`） |
| 33 | 已加载，不 tick（`FULL`） |
| 34 起 | 只到某个生成阶段，`MAX_LEVEL = 33 + FULL 阶段的依赖半径` |

26.3 把 ticket 分成**加载**和**模拟**两套（`DistanceManager` 里的 `LoadingChunkTracker` 和 `SimulationChunkTracker`）：

- **视距**：视距内每个区块挂一张 `player_loading`，等级 31（`PLAYER_TICKET_LEVEL`）。决定加载和发送哪些区块。
- **模拟距离**：玩家所在区块挂一张 `player_simulation`，等级 `31 − simulationDistance`，默认 `simulationDistance = 10`。决定哪些区块会 tick。
- 刷怪只在玩家 8 个区块以内（`naturalSpawnChunkCounter`）。

`TicketType`：

| 类型 | 超时（刻） | 标志 |
|---|---|---|
| `player_spawn` | 20 | 加载 |
| `spawn_search` | 1 | 加载 |
| `dragon` | 0 | 模拟、保持维度活跃 |
| `player_loading` | 0 | 加载 |
| `player_simulation` | 0 | 模拟 |
| `forced` | 0 | 存盘、加载、模拟、保持维度活跃 |
| `portal` | 300 | 存盘、加载、模拟、保持维度活跃 |
| `ender_pearl` | 40 | 加载、模拟、保持维度活跃 |
| `unknown` | 1 | 加载、卸载时可过期 |

带“存盘”标志的 ticket 写进 saved data `chunk_tickets`（`TicketStorage`）。

### 2.4 生成流水线

`ChunkStatus`：`empty → structure_starts → structure_references → biomes → terrain → features → initialize_light → light → spawn → full`。
26.3 把旧版的 `noise`、`surface`、`carvers` 合成了一个 `terrain`。

`ChunkPyramid.GENERATION_PYRAMID` 的依赖：

| 阶段 | 需要邻居 | 写邻居方块 |
|---|---|---|
| `structure_references`、`biomes` | `structure_starts` 半径 8 | — |
| `terrain` | `structure_starts` 半径 8，`biomes` 半径 1 | 半径 0 |
| `features` | `structure_starts` 半径 8，`terrain` 半径 1 | **半径 1** |
| `light` | `initialize_light` 半径 1 | — |
| `spawn` | `biomes` 半径 1 | — |

从磁盘读的区块走 `LOADING_PYRAMID`，大部分阶段是空操作。

线程（`ChunkMap`）：

- 生成调度走串行队列 `ConsecutiveExecutor("worldgen")`，光照走 `ConsecutiveExecutor("light")`，共用后台线程池。
- 重活派到 `Util.backgroundExecutor()`：`buildTerrain`、读盘后的 `parseChunk`、DFU 升级 `upgradeChunk`。
- `ChunkStatusTasks.full`（升级成 `LevelChunk`、放进世界）在主线程。

### 2.5 存储

磁盘布局见 `m3-storage.md` 第 2 节：主世界也在 `dimensions/minecraft/overworld/` 下面，每个维度有 `region/`、`entities/`、`poi/`、`data/`；玩家数据在 `players/data/<uuid>.dat`。

今天补充核实的：

- 26.3 把很多东西从 `level.dat` 拆成了 `data/` 下独立的 saved data：`game_rules`、`weather`、`world_border`、`world_clocks`、`world_gen_settings`、`scoreboard`、`random_sequences`、`chunk_tickets`、`custom_boss_events`、`scheduled_events`、`stopwatches`、`wandering_trader`、`maps/last_id`、`ender_dragon_fight`。`level.dat` 只剩元数据（世界名、版本历史、上次游玩时间、`Time` 等）。
- `RegionFile`：扇区 4096 字节；每区块 5 字节头；超过 256 个扇区的区块写到 `c.<x>.<z>.mcc`（压缩字节加 128 标记）。压缩 ID：1 gzip、2 deflate（默认）、3 不压缩、4 LZ4、127 自定义。
- 区块 NBT 顶层：`DataVersion`、`xPos/yPos/zPos`、`Status`、`LastUpdate`、`InhabitedTime`、`sections`、`Heightmaps`、`block_entities`、`block_ticks`、`fluid_ticks`、`PostProcessing`、`structures`、`isLightOn`、`UpgradeData`。
- IO：每种存储一个 `IOWorker`（`IOWorker-<类型>`），在 `Util.ioPool()` 上用带优先级的串行队列。
- 保存节奏：
  - `MinecraftServer.autoSave` 每 300 秒（按 tick 速率换算，20 TPS 时 6000 刻）全量一次；
  - `ChunkMap.saveChunksEagerly` 每刻最多顺手存 20 个，单区块冷却 10 秒；
  - 卸载时每刻最多存 200 个（`CHUNK_SAVED_PER_TICK`）；
  - 同时最多 128 个写入在途（`MAX_ACTIVE_CHUNK_WRITES`）。

  `m3-storage.md` 的 S11 只做了“关服全量 + 每 300 秒扫脏区块 + 10 秒冷却”，没有每刻 20 个的顺手保存。

### 2.6 实体

- 实体单独存在 `entities/`，按区块组织，由 `EntityStorage` 带反序列化队列异步读入。
- `PersistentEntitySectionManager` 记录每个区块的实体加载状态：`FRESH`、`PENDING`、`LOADED`。
- 实体可见性 `Visibility`：`HIDDEN`、`TRACKED`、`TICKING`，跟随区块等级。

### 2.7 光照

`ThreadedLevelLightEngine` 跑在 `light` 串行队列上，不在主线程。生成流程里有 `initialize_light` 和 `light` 两步。

### 2.8 每刻 tick 的范围

`ServerChunkCache.tickChunks`：

1. 收集刷怪区块，用 `level.getRandom()` 打乱，逐个尝试刷怪；`getGameTime() % 400 == 0` 时才刷持久生物。
2. 对所有方块 tick 区块做随机刻，次数按游戏规则 `random_tick_speed`。
3. 广播方块变化。

`ServerLevel.tick` 的阶段顺序：天气 → 方块计划刻 → 流体计划刻 → 袭击 → `chunkSource`（刷怪、随机刻）→ 方块事件 → 实体 → 方块实体。

### 2.9 线程模型一句话

主线程独占一切；生成、光照、IO、实体反序列化在旁边的串行队列和线程池里做，结果交回主线程。

---

## 3. 九项决策

| # | 决策 | 选项 | 提议 `[判断]` | 状态 |
|---|---|---|---|---|
| D1 | 多维度和多线程 | — | 每个维度从一开始就支持多个 region | **已定** |
| D2 | 区块加载 | A：照原版 ticket，加载和模拟分开<br>B：只按视距加载、模拟距离 tick | A。region 的形成也挂在 ticket 上 | 待定 |
| D3 | 世界生成在哪跑 | A：独立的生成线程独占 `ProtoChunk` 和依赖图，重活交给线程池，完工区块整块交给世界线程<br>B：世界线程同步生成 | A | 待定 |
| D4 | 存档格式 | Anvil 兼容原版，之后加 Linear | 见 `m3-storage.md` S1、S2 | 已有设计 |
| D5 | 旧版本存档 | 只认 `DataVersion` 5023，其他拒绝 | 见 `m3-storage.md` 第 1 节 | 已有设计 |
| D6 | 保存不卡 tick | 世界线程拍快照，后台编码、压缩、写盘 | 见 `m3-storage.md` S3、S4、S11；可以考虑补上原版每刻 20 个的顺手保存 | 已有设计 |
| D7 | 光照 | A：留在世界线程（现状）<br>B：像原版一样放到独立队列 | 先 A，光照数据存盘 | 待定 |
| D8 | 实体存储 | 照原版单独存 `entities/`，异步加载 | 兼容原版就只能这样；`m3-storage.md` 把它排在 M3 之外 | 待定 |
| D9 | 方块实体存盘 | A：每种都照官方 `saveAdditional` / `loadAdditional` 完整读写，不认识的键原样保留<br>B：只存认识的字段 | A | 待定 |

---

## 4. D1 的讨论过程

### 4.1 Folia 和 Pumpkin（详见 `folia.md` 第 2、5 节）

| | Folia | Pumpkin |
|---|---|---|
| 并行单位 | 空间：region，长期存在，会合并拆分 | 工序：tick 里的各阶段，每刻现分 |
| 数据访问 | 所有权，运行时检查 | 共享，靠锁和原子类型 |
| tick 内顺序 | region 内串行 | 并行阶段不保证 |
| 进程 | 单进程多线程 | 单进程多线程 |
| 主要代价 | 改了 407 个文件；插件不兼容；玩家扎堆时只有一个线程 | 锁、死锁（历史上 29 个修死锁的提交）、顺序漂移、`try_lock` 失败静默丢逻辑 |

Pumpkin 在 2026-11-04 到 2026-11-08 之间又有 36 个提交，没有改线程模型，其中有一个修死锁的提交。

### 4.2 多进程：不放进核心

原版维度之间的耦合 `[事实]`：

1. 同一刻里按顺序 tick（`tickChildren`）。
2. 穿越传送门在源维度 tick 中途把实体直接加进目标维度（`Entity.teleportCrossDimension` → `addDuringTeleport`）；目标维度排在后面时，同一刻可能再 tick 它一次。
3. 找传送门出口时同步加载目标维度的 POI（`PortalForcer`，搜索半径主世界 128 格、下界 16 格）。
4. 战利品随机序列 `RandomSequences` 全服一份，挂在 `MinecraftServer` 上。
5. 选择器（`@e` 等）遍历所有维度（`EntitySelector`）。
6. 计分板、游戏规则、时钟、玩家列表、聊天都是全局的。

| | 一条线程 | 每个维度一线程 | 每个维度一进程 |
|---|---|---|---|
| 和原版一致 | 完全一致 | 跨维度顺序会变 | 变化很大 |
| 跨维度操作 | 直接调用 | 消息 + 同步点 | 跨进程调用 |
| 玩家换维度 | 同一连接 | 同一连接 | 要代理层或让客户端重连 |
| 加速 | 无 | 上限取决于主世界 | 同左，还要扣通信开销 |

`[判断]` 多进程的收益小（瓶颈在主世界）、代价大（上面每条耦合都变成跨进程通信）。Java 世界“每进程一个 GC 堆”的理由对 Rust 不成立；崩溃隔离我们本来就不靠它（tick 出错即停服）。多进程适合的是“多台机器、多个独立世界”，那是代理层（Velocity 一类）的事，Bamboo 用 `/transfer` 和代理转发支持就够了。

### 4.3 维度只是数据边界

Folia 每个维度有自己的分区器（`ThreadedRegionizer`），region 不跨维度；所有维度的 region 在**同一个线程池**里调度，没有“维度线程”（`folia.md` 第 2.2、2.4 节）。

所以一旦按 region 并行，维度就不再是并行的单位，只剩三个身份：数据边界、跨维度等于跨 region、“按维度一份但不属于任何 region”的状态（`level.random`、末影龙战斗等）。

用户据此决定：**直接做每个维度多个 region**。

---

## 5. region 的切分、合并、拆分（初稿）

### 5.1 切分

- 网格单位是 section：**16×16 个区块**（Folia 默认 `gridExponent = 4`）。
- region 拥有 section。section 里有已加载的区块（chunk holder）就是“非空”。region 的形状由区块决定，不由玩家决定，所以 **D2 的 ticket 系统就是分区器的输入**。
- 每组非空 section 外圈一层缓冲 section，归同一个 region。
- 不变量：每个已加载的区块恰好属于一个 region；两个 region 之间隔着各自的缓冲带。
- `[推导]`（`folia.md` 第 2.3 节）：两组非空 section 相隔不到 4 个 section 就必须合并；两个独立 region 的已加载区块之间至少隔 48 个区块（768 格）。

缓冲带保证 region 在 tick 里碰不到别的 region 的方块：活塞、爆炸、红石都走不出 16 个区块；读方块不隐式加载区块（R6），越界只会读到“未加载”。

### 5.2 合并

- 新加载的区块让某个 section 变成非空，附近出现两个以上 region 时合并。
- 选一个保留下来（规则要确定，比如区块多的留下，一样多取坐标最小的），其余 region 的数据整块 move 过去。所有工作项都按位置存（R8），合并就是并表。
- 随机源保留一个。

### 5.3 拆分

- 沿用 Folia 的阈值：空 section ≥ 8 个且占比 ≥ 1/6 时清理，再按缓冲距离做 BFS，分出几块就拆成几个。
- 数据按所在 section 分配；玩家带着网络连接跟着自己的位置走。
- 随机源：用原 region 的随机源，按 section 坐标排序，依次为每个新 region 派生种子，结果确定。

---

## 6. 一个 tick 怎么拼：两个方案

### 6.1 齐步走（已否决）

每个服务器 tick 分三段：

1. **独占段 1**（一条线程，拿到所有 region 的 `&mut`）：接收加载完成的区块 → 分区器增减 section → 合并拆分 → 把包交给玩家所在的 region → 给 Global 拍快照。
2. **并行段**：每个 region 按原版顺序跑一次完整的 `ServerLevel.tick`，跨 region 的事只写进发件箱。
3. **独占段 2**：按确定顺序处理所有发件箱、跨 region 传送和命令、全局效果，统一 flush 网络。

优点：region 集合只在独占段里变，Folia 的过渡态和“等它 tick 完再合并”都不需要；游戏时间天然一致；结果确定。

否决理由：一刻的时长取决于最慢的 region，一个 region 卡住全服都慢；和“性能优先”冲突。

### 6.2 各走各的（采用方向）

和 Folia 一样，每个 region 有自己的 tick 计数，在线程池里按自己的截止时间调度。在这个基础上做第 8 节的改进。

---

## 7. 调研：可借鉴的成熟体系

三路调研，出处见第 12 节。只取分片、消息、所有权和同步的思路，**不取共识**：单进程里各方互相信任，共享内存本身就是证明。

### 7.1 会动态拆合的分片区块链

| 系统 | 拆合 | 跨分片消息 | 对我们有用的点 |
|---|---|---|---|
| TON | 按账户前缀拆成两半，兄弟分片可以合并；按最近 48 块的加权负载决定，决定后默认再等 100 秒执行 | 消息留在发送方出队列，接收方按 (lt, hash) 导入；lt 是 Lamport 逻辑时钟；不丢、不重、同一对发送方和接收方之间 FIFO | 逻辑时钟；拆合时按目标地址切分在途消息；单一投递路径（加过即时路由的快速通道，丢了 FIFO，现已关闭） |
| NEAR | 按状态大小自动拆，两个 epoch 后生效，暂时只拆不合 | 跨分片的 receipt 在下一块执行；没有自动回滚，失败时退款给调用方 | 拆分时在途 receipt 的分配规则；拥塞控制的教训（负载不均时延迟可能无限增长） |
| MultiversX | 按节点数和交易量拆合；兄弟分片互存状态，所以合并很快 | 源分片扣款，目标分片入账，整批要么全成要么全败 | 兄弟互存状态让合并变快 |
| CockroachDB（range 合并） | 被吞并方先静止（没有在途命令），合并后旧请求弹回重新路由；超过阈值的不合并，防抖动；带每次拆合递增的 generation 防 ABA | — | “合并带来的 bug 是性能、稳定性、正确性三类里最糟的” |

### 7.2 所有权和并行执行

| 系统 | 机制 | 对我们有用的点 |
|---|---|---|
| Sui | 独占对象不需要全局排序；共享对象先排序再执行，热点共享对象要拆开 | 三类所有权；全局状态当共享对象 |
| Solana Sealevel | 交易预先声明读写的账户，不冲突的并行 | region 内实体按声明范围并行；热点（同一账户）退化成串行 |
| Aptos Block-STM | 预设顺序 + 乐观并行 + 多版本内存 + 校验重跑，结果等于串行；32 线程最高约 20 倍，高竞争时退化 | 一个共享计数器就能让整块变串行；aggregator 让可交换的加减不形成依赖 |
| Cosmos IBC / Polkadot XCMP | 有序和无序通道、确认、超时（必须在接收方证明）、ICS-20 的托管和退款 | 托管式交接；只由目标端判定超时 |

### 7.3 仿真和游戏

| 来源 | 机制 | 对我们有用的点 |
|---|---|---|
| PDES 保守同步（Chandy–Misra–Bryant） | 只在确定不会再收到更早消息时推进；lookahead：p 能断定不会给 q 早于 t 的事件 | 用距离推出时钟差上限（第 8.2 节） |
| PDES 乐观同步（Time Warp） | 先跑，遇到迟到消息就回滚；GVT 以下回收 | **不借**：发给玩家的包不能撤回 |
| EVE Online | 一个 node 承载多个星系；过载时时间膨胀（最低 10%），玩家“看表”的计时不膨胀；早期把星系打散映射，结果远处星区互相拖慢 | 按 region 的时间膨胀；调度要有地理亲和 |
| SpatialOS | 每个组件同时只有一个 worker 可写；交接时旧 worker 交出状态，过渡期命令先缓冲，超时强制切换；重叠的权限带防抖动 | 两阶段交接、命令缓冲；成本比传统专用服务器高 8–35%，2019 年至少三款游戏下架 |
| HPC 分子动力学（LAMMPS、GROMACS） | 幽灵格：每步交换邻居边界的副本；负载不均时移动边界而不是合并；开了动态负载均衡就不能逐位复现 | 幽灵格读取；**不借**移动边界（MC 红石对顺序敏感） |
| Star Citizen、New World | 每个实体只有一个权威服务器；中央复制层一崩整个分片掉线 | 前车之鉴：别做中央复制层 |
| MultiPaper | 多台服务器分区块所有权，订阅别人的方块变化 | MC 上做多服的先例 |

---

## 8. 提议的同步设计（待定）

### 8.1 三类所有权（Sui）

| 类别 | 例子 | 规则 |
|---|---|---|
| region 独占 | 方块、方块实体、实体、玩家、计划刻、region 随机源 | 只有拥有者能读写，编译器保证 |
| 全局共享 | 计分板、游戏规则、玩家列表、天气时间、战利品随机序列 | region 提交操作，全局按确定顺序应用；读每刻发布的快照，叠加自己刚写的 |
| 不可变 | 注册表、战利品表和配方定义 | 共享不加锁；`/reload` 时整体换新 |

计数器（击杀统计、实体 ID）按 aggregator 处理：实体 ID 由全局给每个 region 预发号段。Folia 至今把计分板 API 标为不可用，原因就是全局状态。

### 8.2 各走各的时钟 + 按距离约束（PDES lookahead）

```text
L = d ÷ (v_A + v_B)
```

- d：两个 region 的加载区到合并阈值还剩的距离；v：双方当前最快实体的接近速度。
- 相邻 region 的时钟差必须始终 ≤ L。离得远就各走各的，靠近时自动收紧，到该合并时时钟几乎已经对齐。
- 每个 region 发布“我安全到第几刻”，邻居读它只是一次原子读。远处的 region 之间不约束。
- 特别快的东西（实验规则下矿车最高 1000 格/秒、TNT 推出去的实体）不计入 v，走交接或提前合并。
- `[推导]` 例：d = 500 格，双方疾跑（约 5.6 格/秒）约 890 刻；双方鞘翅加烟花（约 33.5 格/秒）约 150 刻。

Folia 的相邻 region 在合并完成前不能 tick，快的一方要停下来等。

### 8.3 跨 region 消息（TON、NEAR、CockroachDB）

- 按稳定的键寻址（目标坐标或实体 UUID），送达时再查 region。region 拆合后消息照样落到对的地方。
- 带逻辑时钟 `lt`：发送时附上；接收时 `lt = max(lt, msg.lt) + 1`；合并取较大值。收件箱按 `(lt, 来源, 序号)` 排序。
- 带目标刻：珍珠落点、`/tp` 写明在第几刻执行。时钟差在 L 以内就不会出现“来自未来”的事件。
- 只走一条路，不给同线程的 region 开直接调用的后门。
- 有界队列 + 最低配额；退回和回复不限流，防死锁。

### 8.4 托管式交接（IBC、SpatialOS）

场景：末影珍珠、穿越传送门、`/tp`、骑矿车冲过边界。

1. 源 region 把实体从 tick 列表拿掉，整个 move 进交接消息，留一条托管记录。
2. 交接期间对它的伤害和交互先缓冲，完成后在目标端重放。
3. 目标端能接收就插入并回确认；接收不了（区块没加载、超期限）就原样退回。退回的不能再退回。
4. **只有目标端判定超时**，实体不会被复制。
5. 在途对象用 `InTransit<T>` 包住，被意外丢弃时在已知坐标生成掉落物兜底。
6. 托管记录就是停服和存档时的在途清单。

### 8.5 合并和拆分（CockroachDB、TON、GROMACS）

- 合并是正确性要求，不能拖。先静止、再吸收：两边都在 tick 间隙时拼收发件箱、按 `lt` 重排、保留一个随机源。利用 8.2 的 L，在它快用完之前就提前开始合并。
- 拆分是优化，要迟滞：分开的条件连续保持 N 刻，region 至少存在 M 刻，拆完不会马上又合回去。
- 每个 region 带 generation，每次拆合加一，过期引用一眼认出。

### 8.6 按 region 的时间膨胀（EVE）

- 过载的 region 主动降低自己的 tick 速率，用原版 `ClientboundTickingStatePacket`（`/tick rate` 用的包）通知这个 region 里的客户端。
- 只有这个 region 变慢。“看表”的计时（拍卖到期、临时封禁、重启倒计时）不膨胀。
- **UNVERIFIED**：原版客户端在 tick 速率低于 20 时是否平滑跟随，要用真客户端测。

### 8.7 以后再做

- 幽灵格读取（HPC）：只读的感知逻辑（索敌、寻路读方块、幽匿振动）读邻居边界区块的每刻快照；写操作作为命令送回拥有者。region 可以靠得更近而不必合并。红石、活塞靠近边界时仍要合并。
- 亲和调度（EVE）：region 多于线程时把相邻的放同一线程；热点给专用核。

### 8.8 和 Folia 对比

| | Folia | 本提议 |
|---|---|---|
| 时钟 | 各走各的，无约束 | 各走各的，相邻 region 按距离约束时钟差 |
| 合并时 | 相邻 region 停下等 | 按剩余 L 提前开始 |
| 跨 region 事件 | 拥有者下一刻执行，无时间戳 | 逻辑时钟 + 目标刻 |
| 交接失败 | 停服时专门处理在途实体 | 托管、确认、退回；丢弃兜底 |
| 过载 | 默默掉 TPS | 按 region 时间膨胀并通知客户端 |
| 拆分抖动 | 只看空 section 比例 | 时间和距离双重迟滞 + generation |
| 全局状态 | 计分板 API 不可用 | 共享对象 + 快照 + 聚合计数 |
| 越权访问 | 运行时检查抛异常 | 编译期挡住 |

### 8.9 不借的

共识和签名证明；Time Warp 回滚；在活跃区域中间移动边界；全局齐步走；进程外 worker 加中央复制层。

### 8.10 风险

- 这是成熟系统的组合，但组合本身没人在 MC 上验证过。L 的公式和七个部件的落地方式是推导，不是文献结论。
- 8.2 要求跟踪每个 region 最快实体的速度，先测开销，不划算就退回 Folia 式的完全独立。

建议顺序：先做 8.1、8.3、8.4、8.5（骨架），再做 8.2、8.6，最后 8.7。

---

## 9. 原版共享状态在多 region 下的处理

| 原版状态 `[事实]` | 原版 | 多 region 时（提议） |
|---|---|---|
| `Level.random` | 每个维度一条 | 每个 region 一条，合并保留一条，拆分按确定规则派生 |
| `Level.subTickCount` | 每个维度一个，决定同刻同优先级计划刻的先后 | 每个 region 各自计数；合并时同刻的按来源 region 顺序排 |
| `MinecraftServer.randomSequences` | 全服一份 | 每个 region 派生一份（偏差写进文档），或者取数走全局队列，待定 |
| `ServerLevel.ENTITY_COUNTER` | 全服一个静态 `AtomicInteger` | 全局给每个 region 预发号段 |
| 跨维度、跨 region 传送 | 当场搬过去 | 托管式交接（8.4），最早下一刻在目标 region tick |

---

## 10. region 内部并行（如果放宽，待定）

解决 Folia 的硬伤：玩家扎堆时只有一个 region、一个线程。region 内部也能并行，大 region 就能借用空闲线程（work stealing）。

总思路：**读的并行，写的分组，冲突时退回串行。**

| 档 | 做什么 | 行为影响 |
|---|---|---|
| 1. 只读并行 | 实体追踪和打包（按玩家，放在 tick 结束后的只读段）；区块包和光照的编码压缩；刷怪候选位置（按区块并行算，再串行挑选） | 不变 |
| 2. 用上一刻的快照 | 寻路（AI 里最贵的部分）；传感器和目标打分。实体 tick 拆成“思考”（只读，全并行）和“行动”（写世界，分组） | 延迟一刻，确定 |
| 3. 按空间着色并行写 | region 内切成小格按棋盘着色（如 3×3 九色），同色同时跑：随机刻（每格自己的随机源）、方块实体、实体的“行动”（实体孤岛或预先声明范围）、光照。红石计划刻默认保持串行 | 确定，分布不变，不逐位一致 |
| 4. 玩家感觉不到的放宽（做成开关） | 远处生物降低 AI 频率（Pufferfish 的 DAB：间隔 = 距离² ÷ 2^8，最长 20 刻，可能干扰刷怪塔）；更快的红石算法（Paper 的 Alternate Current，红石线快很多倍，更新顺序改变） | 改变玩法，默认关 |

外部实践 `[事实]`：Leaf（Paper 系分支）做了异步寻路（怪物不会立刻拿到路径）、异步刷怪、异步实体追踪（文档承认有时让某些 NPC 显示异常，因为它在 tick 中途读数据）。

注意：刷怪塔、村民交易所这类热点里实体全部冲突，第 3 档帮不上；能帮的是第 2 档的“思考并行”和第 4 档。

建议顺序：**先加按阶段、按 region 计时的剖析器**，再按测量结果从第 1 档往后做。

---

## 11. 未决问题

1. 单 region 精确模式是否保留（提议保留，oracle 在这个模式下跑）。
2. region 内部是否也放宽，放到第几档。
3. D2、D3、D7、D8、D9（第 3 节）。D4–D6 已有 `m3-storage.md`，是否补上原版每刻 20 个的顺手保存。
4. 8.2 的时钟差约束是否值得做，还是先完全独立。
5. 战利品随机序列：按 region 派生，还是取数走全局队列。
6. 跨 region 的命令（`@e`、`/kill @e` 等）：结果反馈晚一刻是否可以接受。
7. 按 region 的时间膨胀：先用真客户端验证 tick 速率低于 20 时的表现。
8. section 大小（16×16 区块）和缓冲宽度是否沿用 Folia 默认值，还是借助幽灵格缩小。
9. 剖析器先行：要不要在动手前先加按阶段、按 region 的计时。

---

## 12. 来源

原版行为：官方 26.3 反编译代码，类名见正文。Folia 和 Pumpkin：见 `folia.md` 第 1 节；Pumpkin 本地克隆在 2026-11-09 看过 `494c29a7` 之后的 36 个提交。

分片区块链：

- TON 白皮书 https://docs.ton.org/foundations/whitepapers/ton
- TON Blockchain 规范 https://docs.ton.org/foundations/whitepapers/tblkch
- TON 内部消息 https://docs.ton.org/foundations/messages/internal
- TON 拆合实现 https://github.com/ton-blockchain/ton/blob/master/validator/impl/collator.cpp
- NEAR receipt https://near.github.io/nearcore/architecture/how/tx_receipts.html
- NEAR 重分片 NEP-568 https://github.com/near/NEPs/blob/master/neps/nep-0568.md
- NEAR 拥塞控制 NEP-539 https://github.com/near/NEPs/blob/master/neps/nep-0539.md
- MultiversX 白皮书 https://cdn.multiversx.com/webflow/multiversx-whitepaper.pdf
- CockroachDB range 合并 https://github.com/cockroachdb/cockroach/blob/master/docs/tech-notes/range-merges.md
- CockroachDB 合并 RFC https://github.com/cockroachdb/cockroach/blob/master/docs/RFCS/20180330_range_merges.md

所有权和并行执行：

- Sui 对象所有权 https://docs.sui.io/concepts/object-ownership
- Sui Lutris 论文 https://arxiv.org/html/2310.18042v5
- Solana 交易 https://github.com/solana-foundation/developer-content/blob/main/docs/core/transactions.md
- Solana 调度器 https://apfitzge.github.io/posts/solana-scheduler/
- Block-STM 论文 https://arxiv.org/abs/2203.06871
- Aptos aggregator https://aptos-labs.github.io/framework-book/aptos-framework/aggregator_v2.html
- IBC 通道和包 https://github.com/cosmos/ibc/blob/main/spec/core/ics-004-channel-and-packet-semantics/README.md
- ICS-20 https://github.com/cosmos/ibc/blob/main/spec/app/ics-020-fungible-token-transfer/README.md
- Polkadot HRMP https://paritytech.github.io/polkadot/book/runtime/hrmp.html

仿真和游戏：

- Nicol 1996（PDES 综述）https://www.informs-sim.org/wsc96papers/017.pdf
- Jefferson 1985（Time Warp）https://doi.org/10.1145/3916.3988
- EVE 时间膨胀 https://www.eveonline.com/news/view/introducing-time-dilation-tidi
- EVE 时间膨胀回顾 https://www.eveonline.com/news/view/time-dilation-hows-that-going
- EVE 星系映射 https://www.eveonline.com/news/view/building-a-balanced-universe
- SpatialOS 交接专利 https://patents.google.com/patent/US10878146
- SpatialOS 负载均衡专利 https://patents.google.com/patent/US10579434
- LAMMPS 通信 https://docs.lammps.org/Developer_par_comm.html
- GROMACS 域分解 https://manual.gromacs.org/current/reference-manual/algorithms/parallelization-domain-decomp.html
- Star Citizen server meshing https://robertsspaceindustries.com/comm-link/transmission/18397-Server-Meshing-And-Persistent-Streaming-Q-A
- New World 架构 https://aws.amazon.com/blogs/gametech/the-unique-architecture-behind-amazon-games-seamless-mmo-new-world
- MultiPaper https://github.com/MultiPaper/MultiPaper
- Folia 概览 https://docs.papermc.io/folia/reference/overview
- `/tick` 命令 https://minecraft.wiki/w/Commands/tick

Minecraft 服务端优化：

- Leaf https://github.com/Winds-Studio/Leaf
- Leaf 优化文档 https://leaf-website.pages.dev/docs/how-to/optimize-leaf-server
- Pufferfish 配置 https://docs.pufferfish.host/setup/pufferfish-fork-configuration/
- Pufferfish 优化指南 https://docs.pufferfish.host/optimization/pufferfish-server-optimization-guide/
- Paper Alternate Current 补丁 https://ayakael.net/mirrors/papermc/raw/commit/1ef1281f21e86cb723251d537aa32b97c0dda413/patches/server/Add-Alternate-Current-redstone-implementation.patch
