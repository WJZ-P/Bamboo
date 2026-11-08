# Bamboo M2 设计

这份文档是 M2 实现的总约定。协议和客户端行为的事实看同目录的规格：

| 文件 | 内容 |
|---|---|
| `coverage.md` | 69 个 serverbound 包的分类、合并后的进服、离开、tick 顺序、超时规则、验收用例。**所有实现者都要读** |
| `chunks-movement.md` | 移动包、`T(vd)`、`PlayerChunkSender`、传送确认 |
| `entities.md` | Tab 列表、可见性、`sendChanges`、元数据、挥手、装备 |
| `blocks-inventory.md` | 破坏、放置、ack、物品栏、中键、数据生成、方块实体列表 |
| `chat-join.md` | 文本组件、聊天、命令、进服序列 |
| `folia.md` 第 6 节 | 线程规则 R1–R12 的出处 |

规格之间冲突时以 `coverage.md` 为准。规格和这份文档冲突时以这份文档的“决定”一节为准。

## 1. 目标和范围

做到：两个真实 26.3 客户端能走动（区块跟随）、互相看见、一起破坏和放置方块、聊天，长时间不断线。

不做：正版验证和加密、压缩、存档、世界生成、生存模式、服务端物理、方块更新和红石、性能优化（光照裁剪、区块缓存、`section_blocks_update`、`light_update`）。

## 2. 决定

| # | 问题 | 决定 | 理由 |
|---|---|---|---|
| D1 | 方块实体列表 | **实现**。`Chunk` 记录有方块实体的位置，区块包写条目，NBT 写 `00` | 这是世界数据本来就该有的东西，几十行。拒绝放置箱子等于创造模式玩不了 |
| D2 | 同名重连 | **按官方踢旧连接**：旧连接收到 `multiplayer.disconnect.duplicate_login`，新连接等它离开后进入 | 断网时旧连接最多留 30 秒，这段时间重连被拒绝很烦人 |
| D3 | 放不好的方块 | 拒绝放置（只回 ack）：两格方块 47 种、默认含水 24 种、`PlaceOnWaterBlockItem` 2 种、`GameMasterBlockItem` 7 种。其余一律放默认状态 | 规格第 10 章的建议。README 写明 |
| D4 | 放置音效 `sound`、`section_blocks_update` | 不做 | P2 |
| D5 | `update_attributes` | 进服时发给自己一次，配对时不发 | `coverage.md` 第 3 节第 3 条 |
| D6 | 入站积压 | 不停止读 socket。每连接 `InFlight` 配额 4096，超了就断开这名玩家 | `coverage.md` G14。这条代替 `folia.md` 6.4 的“停止读 socket” |
| D7 | 断开原因 | 用翻译键：`disconnect.timeout`、`multiplayer.disconnect.server_shutdown`、`multiplayer.disconnect.duplicate_login`、`multiplayer.disconnect.invalid_player_movement`、`multiplayer.disconnect.illegal_characters` | 客户端按自己的语言显示 |
| D8 | `GlobalEffect` | 现在不建这个类型。`Region` 的方法拿不到 `Global`，`Global` 的方法只返回要广播的帧，由 `World` 路由 | 目前没有真正的 Region → Global 影响，不做空抽象 |
| D9 | ticket | 现在不做。所有区块读写都经过 `ChunkStore`，以后 ticket 加在这一个入口上 | 平坦世界没有加载这回事 |
| D10 | Latency | 走 `Inbound::Latency` 消息，不用共享原子变量 | 每 15 秒一条，世界状态保持单一所有者 |

## 3. Crate 结构

```text
mc-bamboo (bin)      apps/desktop
        └──────┬───────────┘
        mc-bamboo-server        网络：accept、握手、登录、配置、连接任务、读写队列
               │
        mc-bamboo-game          世界线程：tick、玩家、区块跟踪、实体同步、方块交互、聊天。不依赖 tokio
          ┌────┴─────┐
 mc-bamboo-world  mc-bamboo-protocol
          │          │  └─ mc-bamboo-nbt
          └────┬─────┘
       mc-bamboo-core  mc-bamboo-data
```

- `mc-bamboo-core`：`BlockPos`、`ChunkPos`、`Vec3`、`Direction`、`Hand`。已写好。
- `mc-bamboo-game`：`src/inbound.rs` 的公开约定已写好，不要改它的公开形状，确实要改时写进返回结果说明原因。
- `chunk_encoding.rs` 从 `mc-bamboo-server` 搬到 `mc-bamboo-game`。

## 4. 线程和队列

```text
连接任务（tokio，每玩家一个）                    世界线程（std::thread，20 TPS）
  读帧、解码
  keep_alive / ping_request / Other 就地处理
  其余 ──────────────── Inbound（std mpsc，全局一条）──▶  按 FIFO 处理
  写任务 ◀──── 出站队列（每连接一条，按字节限额）──────  每 tick 每玩家 send 一次
```

| 通道 | 类型 | 满或断开时 |
|---|---|---|
| inbox | `std::sync::mpsc::Sender<Inbound>`，无界 | `InFlight` 配额 4096，超了断开。`Join`、`Leave`、`Latency`、`Shutdown` 不占配额 |
| 出站 | `tokio::sync::mpsc::UnboundedSender` 加一个字节计数 | 超过 64 MiB 时 `send` 返回 `false` 并立刻中止连接 |

规则：

1. 世界状态只有世界线程能写（R1）。没有 `Arc<Mutex<...>>`。`Shared.players` 不是游戏状态，它是登录准入表，留在 `mc-bamboo-server`。
2. 世界线程永不阻塞（R5）。
3. tick 里迭代的集合有确定顺序（R10）：`BTreeMap`、`BTreeSet`、`Vec`。查找用的 `HashMap` 可以用，但不迭代。
4. 全服遍历只在 `Global` 和 `World` 里（R11）。`Region` 里写“看得见这个区块的玩家”。
5. 世界线程 panic 是致命错误（R12）：`mc-bamboo-server` 用 `catch_unwind` 记日志，然后走关闭流程，取消所有连接。
6. 心跳只在连接任务里，计时不依赖 socket 写完成（`coverage.md` 5.2）。
7. 所有写 socket 的路径都有时限。

### 4.1 `mc-bamboo-game` 内部形状

```rust
// world.rs
pub struct WorldConfig { pub view_distance: i32 /* 已 clamp 到 2..=32 */, pub max_players: i32 }
pub struct World { /* global, region, inbox */ }
impl World {
    pub fn new(config: WorldConfig, inbox: Receiver<Inbound>) -> Self;
    /// 阻塞运行到收到 Shutdown 或 inbox 断开。按绝对时间排下一 tick。
    pub fn run(self);
    fn tick(&mut self) -> ControlFlow<()>;   // 测试直接调用
}

// global.rs：名单、时间、聊天。不碰玩家的链路，方法只返回帧
struct Global { time: GameTime, roster: BTreeMap<PlayerId, RosterEntry> }

// region.rs：区块、玩家在世界里的状态、方块变化、出站链路
struct Region { players: BTreeMap<PlayerId, Player>, links: Links, chunks: ChunkStore, ... }

// links.rs：每玩家一个缓冲区，整 tick 的包攒在一起 flush 一次
struct Links { ... }
impl Links {
    fn add(&mut self, id: PlayerId, sink: Box<dyn PlayerSink>);
    fn remove(&mut self, id: PlayerId);
    fn send<P: ClientboundPacket>(&mut self, to: PlayerId, packet: &P);
    fn send_frames(&mut self, to: PlayerId, frames: &[u8]);
    fn broadcast(&mut self, frames: &[u8]);
    fn broadcast_except(&mut self, except: PlayerId, frames: &[u8]);
    fn kick(&mut self, to: PlayerId, reason: &Component);  // 发 disconnect，flush，close，之后的写入丢弃
    fn flush(&mut self);
}
```

`Links` 放在 `Region` 里，和 `players` 是两个字段，这样遍历 `players` 时可以写 `links`，借用不冲突。以后有多个 region 时，玩家和它的链路一起移动。

被踢的玩家留在世界里，直到 `Leave` 到达。这段时间它的请求被忽略，写入被丢弃。`Leave` 处理要幂等：未知 ID 直接忽略。

`Inbound` 严格按 FIFO 处理，`Join`、`Request`、`Leave` 不分批。这和 `coverage.md` 4.3 的阶段 0、1 等价，因为同一玩家的消息本来就有序。

### 4.2 tick 顺序

```text
1. 取出 inbox 里已有的全部消息，FIFO 处理
   然后算完排着的光照，记下变了的段（官方在两个 tick 之间做这一步）
2. game_time + 1
3. 派生状态：head_yaw、姿势、元数据脏标记
4. 区块跟踪：比较 (center, vd)，center → forget → pending
5. light_update 发给区块在跟踪范围边上的玩家，然后广播方块变化给 isChunkTracked 的玩家
6. visible 配对和解除，然后每名玩家 send_changes
7. 比较装备，发 set_equipment
8. 每名玩家 block_changed_ack
9. 每 601 tick 广播 UPDATE_LATENCY
10. 每名玩家发一批区块
11. 全部 flush
```

### 4.3 进服

按 `coverage.md` 4.1。`World::on_join` 调用 `Region` 和 `Global` 的方法把 10 步串起来，全部写进同一条出站缓冲。

### 4.4 离开

按 `coverage.md` 4.2。连接层保证：`Leave` 发出之后 `PlayerSession` 才销毁。

### 4.5 关闭

`Server::run` 收到关闭信号后：

1. 放开端口。
2. 发 `Inbound::Shutdown`。
3. 世界线程给所有玩家发 `multiplayer.disconnect.server_shutdown`，flush，`close`，退出。
4. 等世界线程结束，取消 `cancel`，等连接任务结束。

世界线程提前结束（panic）时也走第 4 步。连接任务发现 `cancel` 时补发一次断开包，队列已经关闭时这一步什么也不做。

## 5. 代码规范

- 注释写中文，只在代码表达不了意图时写。公开项写 `///`。
- 风格跟现有代码：小函数、有意义的命名、`thiserror`、`expect("中文原因")` 只用于确实不可能的情况。客户端发来的任何值都不能触发 panic。
- 不要用 Rust 1.96 之后才稳定的 API。工作区 `rust-version` 是 1.96，本机工具链是 1.99.1，编译器不会替你检查这一点。
- 全部通过 `cargo fmt`、`cargo clippy --all-targets`（0 警告）、`cargo test`。桌面端 crate 也要能编译：`cargo check -p mc-bamboo-desktop`。
- 不要提交。不要改 `crates/mc-bamboo/src/main.rs`，那里有用户未提交的修改。
- 不要把反编译的源码、Folia、Pumpkin 的代码抄进仓库。可以读，只能按规格自己写。Folia 和 Pumpkin 是 GPL，Bamboo 是 AGPL-3.0-only。
- 不要启动真实的 Minecraft 服务端或客户端，不要接受 EULA。
- 字节例子都写成单元测试。规格里标了官方实测字节的，测试就用那些字节。

## 6. 验证

1. 单元测试：协议字节、`T(vd)`、`PlayerChunkSender`、`send_changes`、放置、物品栏。
2. `mc-bamboo-game` 的测试用假的 `PlayerSink` 直接驱动 `World::tick`，不开 socket。
3. `mc-bamboo-server/tests`：真实 TCP，两个客户端。
4. `tools/mc-bamboo-vanilla` 的 `probe`：官方网络代码解码 Bamboo 发出的每个包。
5. 用户的真实客户端：`coverage.md` 第 8 节、`blocks-inventory.md` 第 13 章。这一步只有用户能做。
