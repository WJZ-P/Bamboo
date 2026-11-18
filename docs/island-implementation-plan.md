# 岛（多 region）的实施计划

> **状态：已确认（2026-11-17），按第 6 节实施；第 0 步已完成。** 用户对第 9 节八件事的答复逐条记在第 9 节：
> 第 1–6、8 条照建议定，**第 7 条改成「新模式默认开启」**（分离驱动和多岛各自在引入它的那一步验收通过后缺省就开，
> 精确 / 齐步模式保留为配置项，供对拍和测试显式选用），第 6 节的表和 6.1 / 6.2 已按它调整。
> 原轮只交文档和基线，主循环、`Region`、`Global`、Director 的结构都没动；第 0 步另外修了实体拾取的一处全表扫描
> （性能修复，行为逐位不变），见第 6 节第 0 步和 `docs/performance-optimizations.md` 第 19 节。
> 「岛」就是总设计稿（`docs/world-model-design.md`）里的 region，叫法沿用 `docs/multi-region-tick-design.md`。
> 本计划落实总设计稿第 16 节的第 4、5 步（第 6 节的形成、调度、两种时间、合并拆分、离境和到达、全局状态）。
> 基线数字在 `docs/profiling.md` 第 14 节。
>
> 标注约定同总设计稿：`[事实]` 有来源，`[推导]` 推导写在原地，`[判断]` 可以被推翻。

## 0. 结论

1. **玩家分散时岛的收益接近线性。** `islands-K` 基线里每组每刻 2.5–3.1 ms，以后留在主控上的串行段只有 0.02–0.04 ms；同一轮里「K 组各自单独跑」的上限是 2.00× / 3.44× / 8.18×（K = 2 / 4 / 8，`docs/profiling.md` 14.2）。第 0 步修掉实体阶段的 O(K²) 之后，同轮复测上限变成 1.42× / 3.80× / 6.09×（`docs/performance-optimizations.md` 第 19 节）；基线那一行保留作历史对照，后续步骤请用修后数字比。
2. **前 7 步都只有一个岛**；第 1–5 步行为和现在逐位一致，第 6、7 步在精确（齐步）模式下逐位一致（精确模式的全部对拍、探针、`BAMBOO_TEST_LOADED` 测试都还能当验收），第 7 步验收后缺省改成分离驱动（交付晚一刻）。第 8 步起才出现多个岛，先「只算不用」，第 10 步上线并在验收后缺省打开。要求逐位一致的测试、对拍、探针一律**显式**选精确模式，不依赖缺省值（第 9 节第 7 条）。
3. **玩家的视野永远不跨两个岛** `[推导，见 4.2]`：玩家能看到的区块都是他自己的票加载的、和他所在区块连通，按不变量必然在同一个岛。所以实体追踪、方块广播、区块发送都不需要跨岛；跨岛的只有离境 / 到达、全局状态、聊天、命令。
4. 第 1 步已有的 `split_off` / `absorb` 只覆盖「单元」那四类表，**岛的拆合还缺**：区块柱和光照、实体表、`TickLevels`、玩家及跟着玩家走的表、实体追踪的配对、绝对时刻的平移、整批搬运（不逐柱重算邻居段计数）。见第 3.3 节。
5. 对总设计稿第 6 节（和第 16 节）有 8 处修改建议（第 8.2 节），最重要的两条：岛按**票想要加载的区块**（等级 ≤ 33）划分，而不是按已交付的区块；`time_offset` 不放在每个区块上，**合并时按两岛时钟差平移被吸收方的绝对时刻**。
6. ~~基线里发现一个单岛也该修的问题：一个大 `Region` 里掉落物的实体阶段超线性
   （每个每刻 3.5 µs → 6.3 µs，K = 1 → 8）。~~ **第 0 步已做完**：原因是
   `drops.rs::pickup_for` 每名玩家每刻全表克隆 `EntityStore` 找能捡的物品，
   总量 O(K²)；改用已有的空间索引后修后单价 1.99–2.89 µs、与 K 无关，
   `islands-8` 的 `entities` 阶段 12.891 → 4.364 ms，K 组独立跑的上限
   8.78× → 6.09×。见 `docs/profiling.md` 14.3 第 2 条和
   `docs/performance-optimizations.md` 第 19 节。第 0 步的结论不改变下面 1–11 步
   的顺序，只是把「拆成 K 个岛」的收益里来自「表变小」的那部分先消掉。

**拍板的结果**在第 9 节（2026-11-17 确认）。

---

## 1. 现状 `[事实]`

- 一条世界线程（`crates/mc-bamboo-server/src/server.rs` 起的 `bamboo-world`）跑 `World::run`；`World { global, region, inbox, host, pacer, ticket_centers, player_directions, player_demand, director_tick, save, … }`（`crates/mc-bamboo-game/src/world.rs:146-177`）。
- 入站是**一条**全服共用的 `mpsc::Receiver<Inbound>`（`Join` / `Request` / `Latency` / `Leave` / `WhitelistReload` / `ConsoleCommand` / `Shutdown`），世界线程每刻按到达顺序一次排空（`world.rs` 的 `process_inbox`）。没有序号，顺序就是通道顺序。
- 出站：每名玩家一个 `PlayerSink`，**只有世界线程写**，放在 `Region.links` 里；全服广播是 `region.links.broadcast`。
- Director 和宿主（`director_host.rs`）在世界线程上；`pump` 一直跑到收敛，**中途直接拿 `&mut Region` 交付 / 收回**（`flush_region_mail_into` → `region_message` → `Region::deliver_column` / `reclaim_columns`）。票的变化在收包阶段就同步跑到底，内联执行器下同一刻就能交付。
- `Region` 已经有调度时钟 `RegionTick`（`region.rs:109-138`），但**方块侧的计划刻用的是全局 `GameTime`**：`interaction::run_block_ticks(region, global.time())`（`world.rs:737`）把它塞进 `BlockTicks.now`，`World::game_time()` 读的就是它；红石缓存那条路径却用 `region.tick`（`interaction.rs:122,149`）。两套时间现在恰好同步增长，所以没出过错。
- 实体 ID：玩家来自服务端的 `static NEXT_ENTITY_ID`（`mc-bamboo-server/src/players.rs:35`，从 1 开始）；非玩家实体来自每张 `EntityStore` 自己的计数器，从 `FIRST_ENTITY_ID = 1 << 24` 开始（`mc-bamboo-blocks/src/entity/store.rs:361`）。两个岛各有一张表就会撞号。
- Director 里已经有 `RegionId = u32` 和 `Owner::Region(RegionId)`（`mc-bamboo-world/src/director/state.rs:35-52`），现在恒为 0；没有「区块 → 岛」的索引。

---

## 2. 状态归属清单

「以后归谁」：**主控** = 主控线程上的 `Global` + Director 宿主；**岛** = 某个 `Region`。
「多岛时怎么读写」里的「快照」指主控每刻发布、岛在 tick 开头取一次的只读 `GlobalView`（最多旧一刻）；「效果」指岛在一刻里攒下、交回时由主控按到达顺序应用的 `GlobalEffect`。

| # | 状态 | 现在放在哪 | 以后归谁 | 多岛时怎么读写 |
|---|---|---|---|---|
| 1 | 世界时间 `GameTime` | `Global.time`（`global.rs:34`），`advance_time` 每刻 +1 | 主控 | 主控固定 20 TPS 推进；岛从快照读（存盘、`/time query gametime`） |
| 2 | 昼夜时钟 `Clock` / `end_clock` | `Global.clock` / `end_clock` | 主控 | `/time`、`/tick` 类命令在主控执行；`SetTime` 帧由主控生成，经各岛信箱发给玩家 |
| 3 | 岛的模拟时间 | `Region.tick: RegionTick`；`BlockTicks.now`（现在喂的是 `GameTime`） | 岛 | 岛自己推进；见第 5 节 |
| 4 | 天气 | `Global.state().weather`；`weather::advance(&mut global, &region)` **用 `region.random` 取数**；`Region.block_weather` 是快照 | 主控 | 快照；多岛时主控要有自己的随机源（差异，见 8.2 第 6 条） |
| 5 | 游戏规则 | `Global.rules` 权威；`Region.block_rules`、`send_command_feedback` 等四个布尔是快照 | 主控 | 快照；`/gamerule` 在主控执行 |
| 6 | 世界边界 | `Global.world_border` 权威；`Region.world_border` 快照（`publish_border`） | 主控 | 已经是快照模型，不变 |
| 7 | 难度、出生点、默认模式、种子 | `Global.state()` | 主控 | 快照；命令在主控执行 |
| 8 | 计分板、队伍 | `Global.scoreboard`；`tick_criteria(&mut region)` 每刻读玩家；聊天读发送者的队伍前后缀、`/tellraw` / `/title` 读分数（第 5a 步核对时补） | 主控 | 岛把六个只读准则的变化作为效果交回（第 5b 步起每名玩家的 `lastRecorded*` 放在 `Player` 上，同官方）；聊天和文本命令读快照；计分板帧由主控生成、经信箱下发 |
| 9 | Boss 栏 | `Global.boss_bars`，进服时 `on_join` | 主控 | 同上 |
| 10 | 玩家列表（Tab）、延迟 | `Global.roster`；`Inbound::Latency` 直接写 Global；每 601 刻广播 | 主控 | 进出、换模式是效果；延迟消息走主控队列；广播帧经信箱 |
| 11 | 管理名单（op / ban / 白名单）、闲置踢出 | `Global.admin`；本刻的变化攒在 `Region.admin_changes`；`tick_idle(&mut global, &mut region)` | 主控 | 名单命令在主控执行；玩家的权限等级变化作为消息发给玩家所在的岛；闲置时限从快照读，踢人在岛里做 |
| 12 | 聊天、签名聊天 | 每名接收者的 `player.chat.next_index` / `cache`（`secure_chat/deliver.rs:75-82`）、发送者的链状态，都在 `Player` 上；刷屏计数在 `Player` | 岛（跟着玩家） | 发送者所在岛校验签名后，把消息作为效果交给主控，主控扇出到每个岛，**各岛给自己的玩家逐个打包**（每名接收者的索引和缓存只由他所在的岛改）；跨岛的接收者晚 ≤ 1 刻 |
| 13 | 命令 | 全部在世界线程以 `(&mut Global, &mut Region)` 执行；控制台命令也是 | 分三类 | 见 4.4 |
| 14 | 实体 ID | 玩家：服务端静态原子计数；非玩家：每张 `EntityStore` 从 `1 << 24` 计数 | 进程级 | 改成一个共享的原子分配器（见 8.2 第 4 条）；不存盘，所以不影响存档 |
| 15 | 随机源 | `Region.random`（方块逻辑和天气）、`sound_seed_generator`、`loot_sequences`（`RandomSequences::new(0)`，不存盘）、`/random` 和 `/loot` 用的按世界种子建的序列（原在 `WorldState.sequences`，第 5b 步搬到 `Region.random_sequences`）；实体各自带随机源 | 岛 | 拆分时按岛的起源 section 派生，合并时留大的那个；天气另用主控的（第 9 步核对：取数的地方和待定的几处见 3.3「岛的随机源」） |
| 16 | Director、票表、停放表、IO、计算池、存盘队列 | `World.host`（`DirectorHost`） | 主控 | 不变；交付 / 收回改成发到岛的信箱 |
| 17 | 玩家票中心、移动方向、需求快照 | `World.ticket_centers` / `player_directions` / `player_demand` | 主控 | 岛每刻在交回的报告里带上「哪些玩家跨了区块」 |
| 18 | 每区块的 tick 档位 | `Region.tick_levels: TickLevels` | 岛 | `LevelChanged` 按区块归属路由到岛 |
| 19 | 区块柱和带位置的表 | `Region.chunks` / `block_ticks` / `block_events` / `block_entities` / `redstone` / `containers` / `block_items` / `command_blocks` / 讲台的书 | 岛 | 岛独占 |
| 20 | 运行时光照 | `ChunkStore` 里，`run_light_updates` | 岛 | 岛独占；不同岛的区块不相邻，光传不过去 |
| 21 | 实体、实体追踪、玩家网格 | `Region.entities` / `entity_tracking` / `player_grid` | 岛 | 岛独占（4.2：视野不跨岛） |
| 22 | 待广播的方块 / 光照变化 | `Region.block_changes` / `light_changes` | 岛 | tick 边界一定是空的 |
| 23 | 存盘 | `World.save: SaveSession`；`Region.dirty`；`host.save_loaded_dirty(&mut region)` 借 region 拍快照 | 主控编排，岛编码 | 主控在岛交回时给出「这次要拍哪些区块」，岛在下一刻末尾用 `encode_loaded_column` 编好随报告交回；`/save-all` 等所有岛都交回后过栅栏 |
| 24 | 红石缓存 | `Region.redstone_cache` | 岛 | 拆合时直接丢 |
| 25 | 单元划分统计 | `Region.partitioning` | 岛 | 拆合时重建 |
| 26 | 剖析器 | `Region.profiler`，宿主也往里记子项 | 每岛一份 + 主控一份 | 报告按岛分开打，另出一份合计 |
| 27 | 节拍 | `World.pacer`、`Schedule`（落后 2 秒内连续追赶） | 主控（Global 的 20 TPS）+ 调度器（每岛截止时间） | 见第 5.2 节 |
| 28 | 出站缓冲 | `Region.links` | 岛（跟着玩家） | **只有玩家所在的岛写他的 sink**；主控产生的帧经信箱 |
| 29 | 菜单、讲台视图、告示牌编辑者 | `Region.menus` / `lecterns` / 告示牌表 | 岛（跟着玩家或位置） | 玩家离境时一起搬 |
| 30 | 视距、人数上限、正版验证、会话公钥 | `Region` 字段（只读配置） | 主控 | 只读，`Arc` 共享或进快照 |
| 31 | 对端 IP、最后操作时刻 | `Region.peer_ips` / `last_action_millis` | 岛（跟着玩家） | 随玩家搬 |
| 32 | 存盘请求、`save-on/off` | `Global.save_requests` / `save_auto` | 主控 | 不变 |
| 33 | `director_tick`、待回复的 `/save-all` | `World` | 主控 | 不变 |
| 34 | 压测钩子 | `World.bench_hooks`，直接改 `Region` | 测试专用 | 多岛时按位置找岛 |
| 35 | 聊天时钟 `clock_millis` | `Region`（测试可固定） | 岛 | 测试专用 |

**和总设计稿 6.6 不一致或者它没覆盖的**：

1. **天气用的是世界随机源**（官方 `ServerLevel.advanceWeatherCycle` 用 `this.random`，Bamboo 照做，`weather.rs`）。6.6 把天气列为「从快照读」，没说谁推进、用谁的随机数。多岛时主控需要自己的随机源，天气的随机序列和精确模式不同。
2. **全局帧怎么到玩家**：6.6 只说「Global 每刻发布快照」。现状是 `Global` 的方法返回帧、由世界线程写进 `region.links`。多岛时必须坚持「一名玩家只有他所在的岛写」（否则同一名玩家的包会乱序，比如 Tab 列表的加入排在实体生成之后），所以主控的帧要经岛的信箱。
3. **签名聊天的接收方状态**（每名接收者的消息索引和签名缓存）6.6 没提。它决定了聊天必须「主控扇出、各岛打包」，不能由发送者的岛直接写别人的缓冲。
4. **实体 ID**：6.6 写「Global 给每个 region 预发号段」。现状玩家已经用一个进程级原子计数器，实体 ID 不存盘，一个共享原子数比号段简单（8.2 第 4 条）。
5. **战利品随机序列**：6.6 写「按（种子、序列名、region 起源 section）派生」；现状是 `RandomSequences::new(0)`、不存盘、平坦世界种子 0。官方存在 `data/random_sequences.dat` 里。这是一个和岛无关的现有缺口，拆岛时照 6.6 派生即可。
6. **存盘快照要借活的 region**：`save_loaded_dirty` / `snapshot_all_dirty` 现在在世界线程上直接读 `Region`。6.6 没写，改法见上表第 23 行。
7. **计分板的六个只读准则**每刻读所有玩家（`tick_criteria(&mut region)`），**闲置踢出**同时读写两边，**延迟广播**在 Global 里攒、在 region 里发。这三处是 `(&mut Global, &mut Region)` 函数，要拆成「岛算、交回效果」。
8. **控制台命令**没有岛，归主控执行。
9. **`Region` 里的配置字段**（视距、人数上限、正版、会话公钥）总设计稿 4.4 说要搬去 Global，还没搬。

---

## 3. 岛的形成、合并、拆分怎么接在 Director 上

### 3.1 谁、在什么时点保证「不同岛的已加载区块不相邻」

- **分区器住在主控里、紧挨着 Director**，是唯一知道「区块 → 岛」的地方（总设计稿第 2 节「这张表只有主控读写」）。
- **划分的依据是票想要加载的区块**，即两套等级里加载等级 ≤ 33（`generation_status` 给出 `Full`）的区块，再并上 `Owner` 是 `Delivering` / `Region` / `Reclaiming` 的区块（还没收回的）。这比 6.1 的「已交付或正在交付」大，好处是：
  1. 合并在生成期间就发生，而不是卡在交付的路上（生成要好几刻，合并只要等一刻）；
  2. 玩家的方形视野不会因为中间几个区块还没生成好而被误判成两块。
- **时点**：Director 每次 `run_updates()` 产出的全部 `LevelChange`（连同 `classify` 的结果；`Promote` / `Demote` 也会跨过 ≤ 33 的边界）就是分区器的输入，再加上 `Deliver` / `Reclaim` / `Delivered` / `Returned` 带来的 owner 变化，在同一次 `pump` 里更新 section 计数。任何一个 section 从空变非空时查它周围切比雪夫距离 ≤ 2 的 section：
  - 一个岛都没有 → 这个 section 归一个新岛（空岛，还没有区块，可以先接玩家，见 4.1）；
  - 恰好一个岛 → 归它；
  - 两个以上 → 登记一次**待合并**，这些 section 暂时记在编号最小的那个岛名下。
- **交付前检查**：`RegionMessage::Deliver` 带上目标岛编号；宿主在把交付放进岛信箱之前查「目标岛有没有待合并」。有的话这条交付留在 Director 的邮件队列里（现有的 `MailBudget` 本来就允许交付跨刻），等合并完成再发。这一条保证**任何时刻已交付的区块之间不相邻**，即使合并还没做完。
- **断言**：调试构建在主控每次处理完交回时断言「任意两个岛的已交付区块切比雪夫距离 > 1」「每个 `Owner::Region(id)` 的区块恰好在岛 `id` 的表里」。

### 3.2 合并和拆分的流程

**合并**（交付会让两个岛靠得太近时先合并再交付）：

1. 分区器登记待合并 `{A, B, …}`，阻止发往这些岛的、会落在合并区域的交付（3.1）。
2. 调度器不再给其中已经停放的岛派新的一刻；正在跑的等它交回（最多等一刻）。
3. 全部停放后，主控在自己线程上把小的倒进大的：`A.absorb_island(B, time_delta = A.now − B.now)`（3.3），然后改 Director 里这些区块的 `Owner::Region(A)`、改玩家 → 岛的路由表、岛的代数加一。
4. 放行被阻止的交付，A 重新排队。

合并的耗时算在两个岛的这一刻上，目标 < 5 ms（总设计稿 1.2「不顿挫」）。要先量（第 7 节第 3 条）。

**拆分**（只是优化，要迟滞）：

1. 分区器每 20 刻对每个岛的 section 图做一次连通性检查（增量：只看最近有 section 变空的岛）；两块之间切比雪夫距离 ≥ 4 并持续 200 刻（10 秒）才拆。
2. 等岛停放，主控按连通块把区块、实体、玩家分给新岛：`A.split_off_island(chunks, players) -> B`；新岛的随机源按起源 section 派生（6.4），红石缓存、单元划分、编码缓存重建。
3. 改 `Owner`、路由表、代数。

### 3.3 第 1 步的 `split_off` / `absorb` 够不够

`[事实]` 第 1 步交付的是「单元」级的拆合：`BlockTicks` / `BlockEvents` / `BlockEntities` / `RedstoneMemory` 的 `split_off` / `absorb`，加上游戏层 `Region::split_off_unit` / `absorb_unit`（容器、`block_items`、命令方块、讲台的书）。另有按区块整盒取放的 `take_columns` / `insert_column`（含 `ChunkStore` 柱、光照、实体）。

拆岛的时候拿它们当零件**还缺这些**（第 9 步全部补上；「单元编号的高位」一行改了做法，见表下「第 9 步做完以后」）：

| 缺什么 | 为什么 | 怎么补 |
|---|---|---|
| 区块柱整批搬 | `ChunkStore::take_column` / `insert_column` 每柱都按目标世界重算这一柱和 8 个邻居柱的 26 个段状态。按不变量，被搬走的一整块和留下的区块不相邻，这些重算全是空操作，但每柱都要付 | `ChunkStore::split_off(chunks)` / `absorb`，前置断言「边界外一圈都不在」，跳过邻居重算 |
| 实体表整批搬 | `EntityStore::take_chunk` 是逐区块的 | `EntityStore::split_off(chunks)` / `absorb`，按 uid 保序；合并后行号重排 |
| `TickLevels` | 每区块的 31/32/33 档位和由此算出的三个 tick 集合在 `Region` 里，不在任何拆合接口里 | 按区块集合拆合；因为不相邻，半径 1 / 2 的邻域不跨岛，不用重算 |
| 玩家和跟着玩家走的表 | `players`、`player_grid`、`links`、`menus`、讲台的视图、`peer_ips`、`last_action_millis`、玩家的实体同步状态 | 按玩家集合拆合 |
| 实体追踪的配对 | `EntityTracking` 是全 region 一张 `BTreeMap<i32, TrackedEntity>`，`seen_by` 是玩家集合。**清空重建会重发生成包，玩家看得见** | 按实体所在区块拆，`seen_by` 原样带走（4.2 保证配对不跨岛） |
| 绝对时刻平移 | `BlockTicks::absorb` 取两边 `now` 的较大值（`ticks.rs:745-787`），两个时钟不同的岛直接合会把一边的计划刻整体提前或推后 | 每类带绝对时刻的表加 `rebase(delta)`，合并前先把被吸收方平移到吸收方的时钟（第 5.1 节列了全部） |
| 脏集合、编码缓存 | `Region.dirty`、`chunk_cache` | 脏集合按区块拆；编码缓存直接丢 |
| 单元编号的高位 | `TickOrder` / ticker ordinal 的高位是单元编号 | 岛编号复用这一位段，合并后全局顺序确定，和第 1 步的设计一致 |
| 跨区块的方块实体关注 | `watchers_cross_chunks` | 按不变量不可能跨岛；拆的时候断言 |

已经够用的：四类单元表的拆合（含按入队序号稳定归并）、`RegionColumn: Send`、光照的「装进不同邻居中间」逻辑（整批搬时用不上）。

#### 第 9 步做完以后（2026-11-17）：岛上每一类数据的拆合

入口是 `Region::split_off_island(chunks, players, random)` / `absorb_island(other, delta)`（`region/island_split.rs`）和岛这一层的 `Island::split_off_island` / `absorb_island`（`island.rs`）。拆的时候新 `Region` 用结构体字面量逐个字段给出、合的时候把被吸收方逐个字段解构，以后给 `Region` 加字段而没决定怎么拆合，编译就不过。清单按代码重新搜过（`Region` / `Island` 的全部字段、各张表自己的全部字段、`thread_local!` / `static`、所有存绝对时刻的字段），不是照抄第 2 节。

前提（断言）：只在 tick 边界、岛停放、交回已经处理完（`effects` / `replies` 为空）时做；岛这一层先把攒着的光照算完；两部分的已加载区块切比雪夫距离 ≥ 3（`ISLAND_GAP`：实体看半径 2 的邻居是否已交付，分区器给的间隔 ≥ 8）；视野不跨岛（4.2，调试构建断言：搬走的玩家只被搬走的玩家看见）。拆分时两边时钟相同，不平移；合并前被吸收方整体 `rebase_clock(delta)`（`delta = 吸收方.now − 被吸收方.now`，第 3 步的入口，现在有了正式调用者，去掉了那些 `expect(dead_code)`）。

| 类别（测试里的 `IslandPart`） | 拆 | 合 | 平移 |
|---|---|---|---|
| 区块表 `Chunks`：方块、两层光照（引擎的存储和给别人读的一份）、版本号、原样起点、已加载登记、存盘的两个改动集合、还没发出去的光照变化 | 按区块整批搬，**不逐柱重算邻居段状态**（断言搬走的每个已加载区块的 8 个邻居要么也搬、要么没加载）；按要搬的个数预留容量；版本号原样带走，两边的版本号计数器照抄、接着往上发 | 并；断言两边不相邻、不重叠；版本号计数器取较大值、`lowest_section` 取较小值 | 无 |
| `TickLevels`：档位、方块 / 实体两个集合和它们的有序版、随机刻集合、待交出的成员变化 | 按区块搬，不重算（隔 ≥ 3 时没有邻域跨两部分） | 并 | 无 |
| 计划刻 `BlockTicks`：主队列和搁置表（方块、流体）、`scheduled` 去重集、移动活塞的 `lastTicked`、`ChunkActivity` 的标记和本刻清单 | 按区块；**新岛不占新的单元编号**，`unit` 和顺序号计数照抄，两边从同一个值接着往上数 | 按（时刻、优先级、顺序号、位置）归并；断言排序键不撞 | `trigger_tick`、`lastTicked`、最近排刻时刻、`now` |
| 方块事件 `BlockEvents` | 按区块，保留入队序号 | 按序号归并 | 无 |
| 方块实体 `BlockEntities`：内容、槽位、ordinal 运行顺序、睡眠分桶和 `wake_at`、位置表（拆完指向新表压紧后的下标）、按区块索引、关注表 `Watchers` | 按区块；`next_ordinal` / `next_id` 照抄；关注表真的分开，两边的段集合都记成「变过」 | 两边拆开以后新登记的 ticker 可能撞 ordinal：撞了就按（ordinal，吸收方在前）从 0 重新编号，每个岛自己的运行顺序不变；没撞时原样 | 表的 `now`、睡眠分桶、`wake_at`、漏斗和三种机器的 `Sleep`（第 3 步） |
| 红石记忆 `Redstone`：比较器、书架、唱片机、合成器两张、讲台页码、火把记录 | 按位置；火把记录保持先后 | 并 | 火把时刻 |
| 容器 `Containers`、方块组件 `BlockItems`、命令方块 `CommandBlocks` | 按位置 | 并 | 容器的 `recheck_at` |
| 讲台 `Books` / `LecternViews`：书、打开的讲台视图、待发格子 | 书按位置；视图和待发格子按玩家（断言视图的讲台在同一边） | 并 | 待发格子的 `deliver_at` |
| 实体表 `Entities`：主表、行号、uid 索引、按类型 / 硬碰撞两个子索引、空间网格、不认识的保留条目、关注段集合、还没交出去的位置事件 | 只扫一遍主表，搬走的行在新表重新分配行号（外面看不见）、子索引和网格在新表里建；保留条目按 `Pos`；关注段集合两边各抄一份（多出来的段只会多报一次位置事件，方块实体表再按真实关注盒筛）；位置事件按盒子所在区块；**ID 分配器共用，不重新取号** | 按 uid 升序放进来；断言共用一个分配器、uid 不重复 | 无（`age` 等都是相对量） |
| 实体追踪 `EntityTracking` | 按实体（搬走的网络 ID）跟着走，`seen_by` 原样带走，不清空重建；实体已经没了、等着下一刻发移除包的条目跟它的观察者走 | 并；断言不重复 | 无 |
| 待存标记 `Dirty`、待广播的光照 / 方块变化 `LightChanges` / `BlockChanges` | 按区块（后两类在 tick 边界通常是空的） | 并 | 无 |
| 区块包缓存 `ChunkCache` | 原岛丢掉被拆走的那部分，新岛从空的开始 | 被吸收方的丢掉 | 无 |
| 玩家表 `Players`（`Player` 整个搬：传送确认、实体同步、已发区块、聊天链和签名缓存、入站队列、玩家自己的随机源等） | 按玩家 | 并；断言一名玩家不在两个岛上 | 传送确认的 `sent_at` |
| 玩家网格 `PlayerGrid`、出站链路 `Links`（含没刷出去的缓冲）、菜单 `Menus`、对端 IP `PeerIps`、最后操作时刻 `LastAction` | 按玩家；`Links` 断言出站信箱关着、空着 | 并（网格按玩家编号） | 无（最后操作时刻是墙上时钟毫秒） |
| 管理名单的本刻改动 `admin_changes` | 留在原岛（全局的，由主控交回时取走） | `merge` | 无 |
| 四个随机源 | 调用方给（`IslandRandom`），见下「岛的随机源」 | 留吸收方的，被吸收方的丢掉 | — |
| 红石缓存、单元划分统计、剖析器 | 新岛从空的开始（照抄配置和开关） | 红石缓存在平移时已经换成空的；单元划分重建；被吸收方的剖析器丢掉 | 缓存直接换空（第 3 步） |
| 岛的时钟、视距、人数上限、正版验证、会话公钥、聊天时钟、全局快照、区块包模板 | 照抄 | 留吸收方的 | 时钟 |
| 岛这一层：区块信箱（待交付的盒子、待收回的位置）、存盘名单 `SavePlan`、存盘报告 `SaveReport` | 按区块；信箱预算照抄、本刻计数从零；名单的剖析指标留原岛、`drain` 照抄 | 接在后面（名单里「放回改动集合」那份绝不能丢）；信箱断言同一个位置不在两边 | 无 |
| 岛这一层：排队的全局改动、回执、状态 | 断言为空 / 停放 | 同左 | — |

**按代码重搜时补的**（第 2 节和上一张表没写）：岛这一层的区块信箱、存盘名单和存盘报告；方块实体的关注表；实体表的保留条目、关注段集合和位置事件；`TickLevels` 待交出的成员变化；光照引擎里还没发出去的变化；`ChunkActivity`；讲台的待发格子；管理名单的本刻改动；每个实体的随机源由网络 ID 派生（见下第 4 条）。核过、不用管的：线程局部的 `SLEEP_CLOCK`（每次读访问前从表的 `now` 重设）；`taken` / `taken_snapshots` / `pending` 这几张实体表的 tick 内暂存（tick 边界为空，`ready_to_move_chunks` 守着）。

**改了做法的一行**：「单元编号的高位，岛编号复用这一位段」**没有照做**。计划刻和方块实体的新岛都不占新的单元编号，计数器照抄、两边接着往上数：同一个位置的刻只由当时拥有它的岛排，而它的计数总不小于这个位置用过的任何顺序号，所以合并后排序键仍然唯一、每个岛自己的先后不变；方块实体的 ordinal 两边可能撞，合并时重编号。这样不会有 24 位用完、岛编号复用后撞号的问题。两个岛之间同一（时刻、优先级）上的先后按顺序号交错，和从没拆过时不一定相同——这正是 6.2「分开跑」允许的。

**还不在这一步、第 10 步接运行时要做的**：宿主里按岛分开的**邮筒**（主控已经投出、岛还没收的交付 / 收回 / 换档）拆合时要按区块改投到新岛，Director 的 `Owner::Region`、玩家 → 岛的路由表、岛的代数跟着改；拆合本身只碰岛。

#### 岛的随机源（第 9 步核对，**有几处要审阅人拍板**）

岛上现在的随机源和取数的地方：

| 随机源 | 谁取 | 拆 / 合现在怎么做 |
|---|---|---|
| `Region.random`（官方 `Level.random`） | 随机刻和方块逻辑（经 `interaction/level.rs` 的 `World::random`）、方块掉落（`block_drops.rs:58`）、容器战利品的兜底（`container_loot.rs:165`）、不带序列的 `/random`（`command/random.rs:49`）、`/loot` 的兜底（`command/loot.rs:308`）、`/playsound` 的种子（`command/playsound.rs:76`）、`/weather` 的时长（`command/weather.rs:60`）、**天气推进**（`weather.rs:28`） | 新岛由调用方给；原岛接着用自己的；合并留吸收方的 |
| `sound_seed_generator` | 音效种子（`interaction.rs:532`） | 同上 |
| `loot_sequences`（`RandomSequences::new(0)`） | 方块掉落、容器战利品、`interaction/level.rs` 的战利品 | 同上 |
| `random_sequences`（按世界种子建的按名序列） | 带序列的 `/random`、`/loot`（`command/random.rs`、`command/loot.rs:309`） | 同上 |
| 岛的实体种子源（`EntityStore.seeds`）和每个实体的 `JavaRandom` | 实体生成或读盘放进岛时取一个 `nextLong` 当种子；实体自己的逻辑用自己的随机源 | 种子源在实体表上，拆分时新岛的种子源由原岛种子源的状态和搬走的最小区块搅出来（原岛不动），合并留吸收方的；取过的种子留在实体身上、跟着实体走 |
| 每名玩家的随机源 | 掉落散布、砂轮等 | 跟着玩家走 |

`IslandRandom::derive(base, origin)` 给了「只由（`base`，起源 section）派生、和原岛的随机状态无关」的一种具体做法（四个源各取一条 SplitMix 流，两个按名序列沿用世界种子、`salt` 按起源 section 派生），但**拆合本身不替调用方选**：`split_off_island` 只收一份 `IslandRandom`。离线测试按 6.2 第 2 条的前提避开了随机数（`randomTickSpeed = 0`、机器不取随机数、指纹里实体的随机源和网络 ID 清零）。要定的：

1. **6.2 和总设计稿 6.4 矛盾**。6.2 第 1 条要「岛的随机源只由岛自己的起源 section 派生，不依赖拆分的先后」；总设计稿 6.4 写的是拆分时「随机源按 section 坐标顺序**从原来的**派生」，结果取决于原岛的随机状态、也就是拆分的先后。只能留一个。
2. 如果选「只由起源 section 派生」：**起源 section 是什么**没有定义（建议：拆分那一刻岛的 section 集合里按（x，z）最小的那个）；`base` 取什么（世界种子？）；**原岛要不要也重新派生**（它的起源可能变了；不重派，原岛的状态仍然依赖历史，严格说 6.2 第 1 条就不成立）；合并后的岛要不要按新起源重派。每次拆合都重派，等于每次拆合都把随机流重置一次。
3. **按名序列**（`/random ... <序列>`）。官方是全世界一份、存在 `random_sequences.dat`。现在拆分时新岛拿到新派生的一份，玩家被拆到新岛后同名序列从别处重新开始；合并时被吸收方的序列进度丢掉。另两种做法：拆分时两边各抄一份（两个岛会出同样的数），或者把按名序列挪到主控、带序列的 `/random` / `/loot` 改成全局类命令（改 4.4 的分类）。
4. **实体的随机源：已定，已实现（2026-11-17，生物第 2 步第二段）。** 原来按网络 ID 派生；网络 ID 用世界共享的分配器（第 2 步），多个岛交替取号以后同一个实体在「拆开跑」和「一起跑」里拿到的 ID 不同，行为就不可复现。现在**实体生成时从它所在岛的实体种子源取一个 `nextLong` 当种子**，读盘的实体在放进岛的实体表时同样取（`EntityStore::insert_chunk`，刚解码的盒子按行的先后重新取号、取种子；在岛之间整盒搬的盒子原样放回）。官方每个实体是不带种子的 `RandomSource.create()`，所以这不偏离官方。
   - 种子源是实体表自己的一个 `JavaRandom`（`EntityStore.seeds`，缺省种子 `ENTITY_RANDOM_SEED`），**不是** `Region.random`：从 `Level.random` 取数会让方块逻辑、掉落散布这些和官方逐位对拍的世界随机序列错开。它在实体表上，所以岛拆合（`region/island_split.rs`）不用动：拆分时新岛的种子源由原岛种子源的状态和搬走的最小区块搅出来、原岛的不动（拆了马上合回来逐位相同），合并留吸收方的；取过的种子就在实体身上，跟着实体走。新岛种子源依赖拆分时原岛种子源的状态，这一点和第 1、2 条一起等定（要「只由起源 section 派生」时把这一处换成起源 section 即可）。
   - 对拍：`entity-oracle` 的官方一侧照同一个种子源给新生成的物品取种子（第 k 个生成的实体取第 k 个 `nextLong`）；`mob-oracle` 的牛仍由场景显式给种子，新生成的小牛、掉落物、经验球按种子源取。三套对拍 0 不一致。顺带修了一处被新种子暴露出来的旧差异：物品卡在方块里时官方 `Entity.move` 走 `noPhysics` 分支直接穿过，Bamboo 原来照常做碰撞（`item_hopper`）。
   - 测试：`entity_seeds_do_not_depend_on_network_ids`（另一张表交替取号，种子序列不变；改回按 ID 派生就失败）、`disk_boxes_take_fresh_ids_and_seeds_and_live_boxes_keep_theirs`（读盘的两个盒子占位 uid 相同也不撞号——原来会撞）。
5. 天气用岛的随机源：第 10 步按 8.2 第 6 条给主控自己的随机源，已经定了，这里只是再列一次。

---

## 4. 跨岛

### 4.1 离境和到达

总设计稿 6.5 的推导成立：带票的东西不会走出岛（玩家走近别的岛之前两边已经合并：两岛已加载区块之间至少隔 8 个区块，鞘翅 33 格/秒也要 4 秒才能跨过，合并只要等一刻）；不带票的东西走不出已加载范围。所以要处理的只有：

| 情况 | 流程 |
|---|---|
| 进服 | 主控收到 `Join` → 挂出生点的玩家票 → 分区器当场给出目标岛（可能是新建的空岛）→ 玩家作为到达对象进那个岛的信箱 → 岛下一刻开头放进世界、发进服序列。和现在一样不等区块交付 |
| 离开 | `Leave` 进玩家自己的入站队列 → 岛移除玩家、编码玩家数据，作为效果交回 → 主控更新 Tab 列表、交给 IO 写盘、撤票 |
| `/tp` 到别的岛（含 `/tp A B`） | 源岛把玩家整个拿出来（`Player`、`links` 里他的 sink、菜单、讲台视图、追踪状态）作为 `Depart { 玩家, 目的地 }` 交回；主控按目的地挂票、问分区器要目标岛（可能新建），放进它的信箱。和进服一样不等区块 |
| `/tp` 到同一岛内 | 照现在的 `move_player`，不经过主控 |
| 掉出已加载范围的普通实体 | 现在的三种实体速度都很小，实体只在 5×5 都已交付的区块里 tick，正常走不出去。**现状没有核对过实体移进未加载区块时 Bamboo 怎么处理**（第 7 节第 5 条）。第一版断言不发生，发生了就按 6.5 交给主控随区块存盘 |
| 跨岛的命令 | 4.4 |
| 全局效果 | 主控生成帧，经信箱发给各岛的玩家（第 2 节第 2 条） |

### 4.2 玩家视野跨两个岛时的发包

`[推导]` **不会发生。** 一名玩家能收到的区块都满足「在他的视距内、已经交付」。视距内的方形区域全部被他自己的加载票覆盖（视距 ≤ 加载半径），方形是连通的，含他所在的区块；这些区块被分区器按 3.1 归进同一个岛（中间没生成好的区块也算，因为划分用的是票想要的集合）。所以：

- **实体追踪**（`entity_tracking.rs` 的可见条件是「在实体追踪距离和视距里、且区块已发给这名玩家」）、**玩家互相可见**（`entity_sync.rs`）、**方块变化广播**（`chunk_broadcast.rs` 按 `player.chunks.is_chunk_tracked` 过滤）、**区块发送**（`chunk_tracking.rs` 只从本岛的 `ChunkStore` 取）都只涉及本岛，代码不用改。
- 唯一的例外是拆合和离境的那一刻，此时岛是停放的，没有发包。
- 调试构建断言「玩家追踪的每个区块都在他所在的岛里」。

如果以后加了比视距更远的追踪（例如末影龙、全局音效），按「全局效果」处理。

### 4.3 「跨岛对玩家无感」具体指什么

| 可观察的东西 | 怎么保证 | 残留差别 |
|---|---|---|
| 看到的方块、实体、别的玩家 | 4.2：视野不跨岛 | 无 |
| 走近另一群人时的卡顿 | 3.1：按票的集合提前合并；合并 < 5 ms | 合并那一刻两岛都多花几毫秒（要量） |
| 区块到达的速度 | Director 不变；交付经信箱 | 性能模式下交付晚一刻（≤ 50 ms），不再在同一刻的收包阶段就插进来 |
| 昼夜、天气 | 主控 20 TPS 推进，同一份帧发给所有岛 | 各岛的玩家收到 `SetTime` 的刻可以差 1 刻 |
| Tab 列表、进出服消息、延迟 | 主控扇出 | 跨岛的人晚 ≤ 1 刻 |
| 聊天 | 主控扇出，各岛打包 | 跨岛晚 ≤ 1 刻；不同岛的两条几乎同时的消息，在不同岛的接收者那里先后可以不同 |
| 跨岛传送 | 离境 / 到达 | 晚 1 刻，和现在传送到没加载的地方要等区块一样 |
| 全局命令（`/time`、`/weather`、`/gamerule` …）的效果和反馈 | 主控执行 | 发令的人在岛里，反馈晚 ≤ 2 刻 |
| 一个岛卡顿 | 各岛各自的截止时间 | 卡的岛里机器整体变慢，别的岛不受影响（这是目的） |
| 机器的相对时序 | 岛内齐步走，拆合时平移时钟 | 无 |
| 玩家的出站缓冲（第 7 步定） | 只有玩家所在的岛写他的缓冲。只有一个岛时主控只在岛停放的时候写（分离驱动一刻的第 ① ③ 段），岛在 tick 线程上跑时不碰，刷出在岛交回之后，主控这一刻的帧和岛的帧同一次交给连接层；多岛时主控的帧投进各岛的出站信箱（`links.rs` 的 `FrameMail`），岛下一刻写进去 | 单岛：岛里算出的计分板准则和延迟广播排到这一刻岛写的包后面（同一刻）；多岛：晚 ≤ 1 刻 |

### 4.4 命令分三类

`command/catalog.rs` 原表是 51 个顶层字面量（含 `tp`、`tell`、`w` 这类别名）；生物第二段接入 `/summon` 后是 52 个，`/summon` 只动本岛的实体表，归本岛类。

| 类 | 命令 | 多岛时 |
|---|---|---|
| 只动本岛 | `setblock`、`fill`、`clone`、`particle`、`playsound`、`enchant`、`clear` / `give` / `gamemode` / `stopsound` / `spawnpoint` / `title` / `tellraw` / `kick` / `transfer`（目标都在本岛时） | 当场执行，同现在 |
| 只动全局 | `time`、`weather`、`gamerule`、`difficulty`、`worldborder`、`seed`、`setworldspawn`、`defaultgamemode`、`op` / `deop` / `ban` / `ban-ip` / `banlist` / `pardon` / `pardon-ip` / `whitelist`、`save-all` / `save-on` / `save-off`、`stop`、`setidletimeout`、`version`、`help`、`list`、`scoreboard`、`team`、`bossbar`、`loot`、`random`（第 10 步改：按名随机序列全世界一份、挪到主控，见 3.3「岛的随机源」第 2 条） | 岛把（来源、命令行）作为效果交回，主控执行，反馈和帧经信箱回来 |
| 可能跨岛 | 带 `@a` / `@p` / 玩家名的 `tp`、`give`、`clear`、`gamemode`、`kick`、`title`、`tellraw`、`msg` / `tell` / `w`、`me`、`say` | 选择器先在本岛解析；有本岛以外的目标时交给主控按岛拆成子命令下发，各岛下一刻执行并回报，主控汇总反馈（6.6「最多晚两刻」） |

只有一个岛时（第 1–7 步、精确模式）三类都当场执行，和现在逐位相同。

---

## 5. 各自的时钟

### 5.1 `time_offset` 和平移

- 一个岛一个时钟。**把 `BlockTicks.now` 改成由岛的时钟驱动**（现在喂的是 `GameTime`），`RegionTick` 和它合成一个；岛建立时的初值取当时的 `GameTime`，所以精确模式下两者永远相等、逐位一致。
- 带绝对时刻、拆合时要平移的数据 `[事实，逐个查过；2026-11-17 第 3 步动手前按「搜所有存绝对时刻的字段」重核一遍，补了标「补」的三行]`：

  | 数据 | 位置 | 现在搬区块时 | 第 3 步的 `rebase` |
  |---|---|---|---|
  | 计划刻的 `trigger_tick`（主队列和搁置表） | `mc-bamboo-blocks/src/ticks.rs` | 装盒时转相对延迟；单元 `absorb` 取 `max(now)` | `BlockTicks::rebase` |
  | 移动活塞的 `lastTicked`（`ticked_entities`） | `ticks.rs` | 绝对值，随区块拆 | 同上 |
  | 补：计划刻表自己的 `now`、每区块最近排刻时刻（`ChunkActivity` 的 `ChunkMark`，给存盘修剪和单元划分用） | `ticks.rs` | `now` 不搬；最近排刻时刻随区块拆、`absorb` 取较大值 | 同上。原来用「时刻 + 1，0 表示没有」编码，平移到 −1 会和哨兵撞上，改成存 `时刻 ^ i64::MIN`（0 仍表示没有、零初始化不变，按无符号比较保持先后） |
  | 红石火把开关记录 | `level.rs`（`RedstoneMemory::torch_toggles`） | 装盒转相对年龄 | `RedstoneMemory::rebase` |
  | 漏斗的 `last_tick_now`、`HopperSleep { since, wake_at }` | `hopper.rs` | 绝对值 | `BlockEntity::rebase`。`last_tick_age` 是相对量不动；`None`（官方读盘后的 `tickedGameTime = 0`）是绝对的 0，不平移 |
  | 熔炉 / 酿造台 / 营火的睡眠日程 `Sleep { since, wake_at }` | `behaviour/machines/cooking.rs`、`brewing.rs`、`campfire_entity.rs`、`sleep_clock.rs` | 绝对值 | `BlockEntity::rebase`；`i64::MAX`（等外部叫醒）不动 |
  | 补：方块实体表自己的 `now`、睡眠分桶 `sleeping` 的键、每个 ticker 的 `wake_at` | `block_entity.rs`（`BlockEntities`） | 装盒转相对剩余刻数 | `BlockEntities::rebase`（含上两行）；`i64::MAX` 不动 |
  | 容器的 `recheck_at: RegionTick` | `mc-bamboo-game/src/container.rs` | 装盒转相对 | `Region::rebase_clock` |
  | 讲台的 `deliver_at`、玩家传送的 `sent_at` | `lectern.rs`、`movement.rs` | 跟着玩家，绝对值 | 同上 |
  | 补：岛的时钟本身 `Region.tick` | `region.rs` | — | 同上。内部从 `u64` 改成 `i64`，平移可以到 0 以下 |
  | 红石缓存的 `now` / `started_at` / `used` / 每条记录的 `last_used` / 触发点的 `stopped_until` | `mc-bamboo-blocks/src/cache/mod.rs` | 拆合时丢 | `Region::rebase_clock` 直接换一份空的（重放和解释执行逐位相同，丢掉只损失命中） |
  | 单元划分的 `last_active` | `region/partition.rs` | 拆合时重建 | 不平移，只是统计 |

  核过、不用平移的：线程局部的 `SLEEP_CLOCK`（每次读访问前从表的 `now` 重设）；`BlockEvents` 和火把记录的入队序号（不是时刻）；`EntityStore` 的实体（`age`、`tick_count` 等都是相对量）；玩家的挖掘进度（玩家自己的 `gameTicks` 计数，跟着玩家走）；实体同步的周期计数；合成器的 `craftingTicksRemaining`（剩余刻数）；存盘冷却和 `Dirty`（`director_tick`，主控）；封禁过期和聊天（墙上时钟毫秒）；世界边界渐变（主控的 `GameTime`）。
  一处要记住的语义：漏斗 `None` 的「绝对 0」不平移，所以「平移后再跑 = 时钟本来就差 d」照样成立；但只有当别的漏斗的 `tickedGameTime`（= 时钟 − 年龄）都 > 0 时，它才和官方「读盘后的漏斗是很久以前」一致。时钟接近 0 时（全新世界的 `GameTime` 从 0 起；老存档从 `level.dat` 的 `Time` 恢复）两者可能差一刻冷却。第 3 步的随机测试因此让两边时钟都不小于 0。
- **合并时把被吸收方整体平移 `delta = 吸收方.now − 被吸收方.now`**：上表除最后两行外各有 `rebase(delta)`，`Region::rebase_clock` 一次做完（第 3 步）。不在每个区块束上存 `time_offset`（对 6.3 的修改，理由见 8.2 第 2 条）。拆分时两边时钟相同，不用平移。

### 5.2 追赶规则

- 现在：落后 2 秒以内连续追赶（`MAX_CATCH_UP`）。总设计稿 6.2 原来写的是「最多连续追赶 2 刻，之后 `下次开始 = max(现在, 理想时间)`」。两者不一致，**已定（第 9 节第 5 条，2026-11-17）：每个岛沿用现在的 2 秒**，理由是用户已经决定不追求精确节拍，而且 2 秒内追平能让卡了一下的岛的模拟时间和世界时间重新对齐。总设计稿 6.2 已同步改。
- 主控的 `GameTime` 不追赶，固定 20 TPS：主控自己很轻（基线里 < 0.05 ms / 刻）。
- （2026-11-17 第 7 步补）**只有一个岛时主控和岛共用一个节拍**：收包（进服直接进岛、玩家请求可能直接写 `Global`）和天气（用岛的随机源）都要岛停放着，所以主控每刻等岛交回才进下一段串行的活，岛落后时主控跟着一起在 2 秒内追赶，`GameTime` 和岛的时钟仍然一一对应。上一条「主控固定 20 TPS、不随岛追赶」要等第 10 步进服改成到达、天气改用主控自己的随机源之后才真正分开。

### 5.3 现在直接读全局游戏时间、以后要读岛的时间的地方

源头只有 4 处，其余都经过它们：

| 源头 | 位置 | 下游 |
|---|---|---|
| `run_block_ticks(region, global.time())` | `world.rs:737` → `interaction.rs:104` | `BlockTicks.now` → `World::game_time()` → 方块逻辑（约 15 处计划刻、约 40 处方块实体和休眠、2 处红石火把），以及线程局部的 `SLEEP_CLOCK`（`block_entity.rs:116-134`，`World::new` 里设） |
| `record_tick_counts` 的 `global.time() + 1` | `world.rs:568` | 剖析器的「到期计划刻数」 |
| `run_partition_stage(global.time())` | `world.rs:725` | 单元划分的 `last_active` |
| `BenchWorld::game_time` | `bench.rs:67` | 压测输入的相位 |

已经读 `region.tick` 的（约 12 处）：容器重查、讲台、传送确认、红石缓存。保持 `GameTime` 的（约 15 处）：`/time`、`SetTime`、`level.dat` 的 `Time`、世界边界渐变、战利品条件的 `game_time` / `day_time`。Director 用自己的 `director_tick`（约 12 处），留在主控不动。

（2026-11-17 第 5a 步核对时补：「战利品条件的 `game_time` / `day_time`」现在其实不读任何时间——`block_drops.rs` 的 `LootLevel` 实现对两者都返回 `None`，`/loot` 的 `LootParams` 也不带时间；以后接上时读岛的快照里的 `GameTime`。岛内真正读 `GameTime` 的只有进服序列里的 `SetTime`（`Region::start_loading_chunks`），第 5a 步起读快照；`/worldborder` 渐变读 `global.time()` 是全局类命令，在主控。）

### 5.4 岛内读 `Global` 的全部位置（2026-11-17 第 5a 步动手前核对）

按「搜所有拿 `&Global` / `&mut Global` 的函数、所有 `global.` 的读」逐个看过。「岛内」= 收包处理、岛类命令（4.4 前两类里的本岛类和可能跨岛类）、岛的各个 tick 阶段；全局类命令以后在主控执行，读权威副本，不算。

| 读的地方 | 读什么 | 第 5a 步 |
|---|---|---|
| 方块逻辑、实体碰撞、移动校验、挖掘 | 规则的方块快照、天气快照、世界边界 | 原来 `Region` 上的 `block_rules` / `block_weather` / `world_border` 三份快照并进 `GlobalView`（`region.global.*`），发布时机不变 |
| 命令反馈、命令方块 | `send_command_feedback` / `log_admin_commands` / `command_blocks_work` | 同上，原来是 `Region` 上的三个布尔 |
| `chat::on_chat` | 发送者的队伍前后缀（`scoreboard().format_player_name`） | 读快照的计分板。**第 2 节第 8 行没写**：聊天也读计分板 |
| `/tellraw`、`/title`（`command/text.rs::resolve` 的 `score` 组件） | 分数 | 读快照的计分板；`/scoreboard`、`/team`、`/bossbar` 解析组件参数时仍读权威副本（全局类） |
| `gamerules::on_request_values` | 全部规则的值 | 读快照 |
| `command::admin::tick_idle` | 闲置分钟数 | 读快照（第 2 节第 11 行已写） |
| 进服序列的岛内部分（`World::on_join` → `Region::add_player` / `start_loading_chunks`） | 默认游戏模式、世界出生点、种子哈希、难度包、时间和主世界时钟、天气、边界 | 读快照。Tab 名单、OP 名单复查、`remember_player`、计分板和 Boss 栏的进服包仍在主控 |
| `/loot`、`/random` | `WorldState.sequences`（按世界种子建的随机序列；`/loot` 经 `get_random_items_with_seed` 的内部可变性推进它，`/random` 直接取） | **不是只读，进不了快照。第 2 节第 15 行漏了这一份**（只写了 `Region.loot_sequences`）。按第 15 行「随机源归岛」，第 5b 步搬到岛上 |
| `/gamemode`、`change_game_mode` 包、`chat_session_update`、客户端设置的帽子 | 写 Tab 名单 | 写，第 5b 步变效果（`GlobalEffect::GameMode` / `ChatSession` / `Hat`） |
| `set_game_rule` 包 | 写规则 | 写，第 5b 步权限在岛上查、改规则作为效果（`GlobalEffect::GameRules`） |
| `ServerPlayer.doTick` 的六个只读准则（`tick_criteria`） | 写计分板 | 写，第 5b 步「岛算、主控应用」：`lastRecorded*` 搬到 `Player` 上（官方也在 `ServerPlayer` 上），岛比对后交回 `GlobalEffect::Criteria`，主控只在有目标跟踪这些准则时才拿可写的计分板。第 5a 步的只读预检 `criteria_pending` 随之去掉 |
| 玩家离开（`on_leave`） | 写 Tab 名单、Boss 栏在线集合，读「离开了游戏」 | 第 5b 步补：原来直接改 `Global`，改成 `GlobalEffect::LeaveMessage` / `Left` |

`GlobalView` 的发布时机：每刻开头、边界推进后、天气和时间推进后，以及只有一个岛时每处理完一条入站消息、每次准则写入、每次压测回调之后（这些地方以前会直接改 `Global`，重发保证岛读到的永远是最新的，逐位一致）。第 5b 步起玩家请求不再直接改 `Global`，改成「端口应用一条会动到快照的效果后重发」（全局类命令、`set_game_rule` 包、准则写入）；主控队列的消息（进服、延迟、名单重载、控制台命令）仍然每条处理完重发。规则和计分板在 `Global` 里改成 `Arc` 共享，发布只加引用计数，写的时候 `Arc::make_mut` 才复制。直接改快照字段的几处测试和测量脚手架（`force_chunks_unpaced` 关随机刻、两处多阶段生成测试、一处计分板测试）改成改权威规则再发布，否则会被下一次发布覆盖。

---

## 6. 实施步骤

每步单独合并；前 7 步只有一个岛。第 1–5 步行为逐位一致；第 6、7 步在精确（齐步）模式下逐位一致。
「规模」是改动行数的粗估，含测试。

**缺省值（第 9 节第 7 条，2026-11-17）**：新模式默认开启。分离驱动（第 7 步的 `island_threads`）和多岛（第 10 步的 `islands`）**各自在那一步的验收通过后缺省就是开**，不等第 11 步；精确 / 齐步模式保留为配置项，供对拍和测试用。因此从第 6 步起：

- 所有要求逐位一致的测试、对拍（`mc-bamboo-vanilla` 的各子命令）、探针（`tools/probe.sh`、hy4 探针）**必须显式选精确模式**，不能依赖缺省值；第 6 步就把这个显式选择加上，第 7、10 步改缺省时这些入口不用再动。
- 第 6、7、10 步的验收都包含「**缺省配置下**全套测试和探针都过」，另外「显式精确模式下」的逐位一致照旧要过。

| 步 | 内容 | 改哪些文件 | 行为 | 怎么验证 | 规模 | 碰 Director |
|---|---|---|---|---|---|---|
| 0 | ✅ **已完成（2026-11-16）**：查清并修掉实体阶段的超线性。原因是 `drops.rs::pickup_for` 每名玩家每刻全表克隆 `EntityStore`（O(玩家数 × 实体数)），换成已有的空间索引，候选名单和 uid 升序逐项不变 | 实际只改 `mc-bamboo-game/src/drops.rs`（`pickup_for`）、`world.rs` 的一行测试模块声明、新增 `world/island_scale_tests.rs`；没碰 `mc-bamboo-blocks/src/entity/` / `hopper.rs` / `watch.rs` | 逐位不变 | 微基准 N=1–16 单价 2.20–2.25 µs 与 N 无关；`islands-K` 同轮：`entities` 每物品 3.18/2.98/4.59/6.29 → 2.89/1.99/2.55/2.13 µs，上限 8.78× → 6.09×；四项 oracle 0 不一致 / 0 fail、探针 719/719、3 个回归测试 + 改坏确认；见 `docs/performance-optimizations.md` 第 19 节 | 小 | 否 |
| 1 | ✅ **已完成（2026-11-17）**。**主控和岛分家（只搬字段）**：`World { master: Master, island: Island }`；`Master` 收 `global`、`host`、节拍、票中心、`director_tick`、存盘会话、压测钩子；`Island` 收 `region`、`entity_threads`。调用顺序一行不变 | `world.rs`、`world/*.rs`、`bench.rs`、`command/console.rs` | 逐位一致 | `cargo test --workspace`、`BAMBOO_TEST_LOADED=1`、全部 oracle、`tools/probe.sh --loaded`、`islands-1` / `spread-16` 同轮 A-B-A 在噪声内 | 中（400–700，机械） | 否 |
| 2 | ✅ **已完成（2026-11-17）**：`EntityIds`（`Arc<AtomicI32>`）在 `mc-bamboo-blocks/src/entity/store.rs`，`World::build` 建一个交给岛的实体表，`with_next_id` 仍给对拍用（各自一个新计数器）。**实体 ID 共享分配器**：每个 `World` 一个 `Arc<AtomicI32>`，从 `1 << 24` 起，`EntityStore` 不再自带计数器（不用全局静态量，免得并行跑的测试互相影响） | `mc-bamboo-blocks/src/entity/store.rs`、`entity/mod.rs`、`mc-bamboo-game/src/region.rs` | 逐位一致（一个岛时号的序列相同） | 同上；加「两张表交替分配不撞号」的单元测试 | 小（~150） | 否 |
| 3 | ✅ **已完成（2026-11-17）**：`RegionTick` 改成 `i64`、从 `GameTime` 起算，`run_block_ticks` / 划分阶段 / 剖析器计数 / `BenchWorld::game_time` 都读它；平移入口是 `Region::rebase_clock`，覆盖的字段见 5.1（核对时补了三行）。**岛的时钟**：`BlockTicks.now` 改由岛的时钟驱动，初值取 `GameTime`；去掉岛内对 `global.time()` 的 4 处读；给 5.1 表里的七类数据加 `rebase(delta)` | `world.rs`、`interaction.rs`、`region.rs`、`bench.rs`、`mc-bamboo-blocks` 的 `ticks.rs` / `level.rs` / `hopper.rs` / `cooking.rs` / `brewing.rs` / `campfire_entity.rs` / `sleep_clock.rs` / `block_entity.rs`、`lectern.rs`、`movement.rs` | 逐位一致 | 同上；每类一个「平移 d 后再跑 = 时钟本来就差 d 时跑」的随机测试，加改坏确认（故意漏平移一类要被抓到） | 中（~700） | 否 |
| 4 | ✅ **已完成（2026-11-17，`7cb7237f`）**。**入站按序号归并**：服务端给每条 `Inbound` 盖一个进程级递增序号；按玩家分队列（队列挂在 `Player` 上，跟着玩家走），非玩家消息进主控队列；世界每刻把主控队列和玩家队列按序号归并 | `mc-bamboo-server` 的 `connection/play.rs`、`players.rs`、`server.rs`、`console.rs`；游戏层 `inbound.rs`、`world.rs`、`player.rs` | 逐位一致（序号就是现在的通道顺序） | 同上；探针；加「两名玩家交错发包、序号归并后顺序和单通道相同」的测试 | 中（~500） | 否 |
| 5a | ✅ **已完成（2026-11-17，`632c9276`）**：核对结果和发布时机见 5.4。**`Global` 快照**：`GlobalView`（规则、天气、时间和时钟、边界、难度、出生点、名单的只读部分）每刻发布一次，岛内所有读 `Global` 的地方改读它 | `global.rs`、`weather.rs`、`gamerules.rs`、`world.rs`、`command/*` 的读路径、`region.rs` 的快照字段 | 逐位一致 | 同上 | 中（~800） | 否 |
| 5b | ✅ **已完成（2026-11-17，`482e8c34`）**：`global_port.rs`（`GlobalEffect` 八种、`GlobalPort::{Direct, Queued}`、交回 `hand_back` 在出站刷出之前）；命令动作分 `IslandFn` / `CommandFn` 两种签名，`command/class.rs` 是 4.4 的表并由测试核对 51 个顶层字面量；聊天、`secure_chat/deliver.rs`、延迟广播单岛时不写 `Global`，留给第 10 步。**效果消息和命令分类**：岛对 `Global` 的写变成 `GlobalEffect`；`GlobalPort` 有两种实现——「直接」（立刻应用，单岛和精确模式用）和「排队」（交回时应用）；计分板准则、闲置踢出、延迟广播、Tab 列表拆成「岛算、主控应用」；命令按 4.4 分类，全局类走 `GlobalPort` | `global.rs`、`command/*`（51 个顶层字面量里约 25 个）、`chat.rs`、`secure_chat/deliver.rs`、`scoreboard/`、`admin.rs`、`world.rs` | 逐位一致（「直接」实现） | 同上；单元测试里用「排队」实现跑一遍全部命令测试，只允许反馈晚到、内容不变 | 大（~1500，可再拆两次合并） | 否 |
| 6 | ✅ **已完成（2026-11-17）**：`island.rs`（`Island`、`ChunkMailbox`、`IslandDrive::{Lockstep, Detached}`、`SavePlan` / `SaveReport`），`RegionMessage` 的交付 / 收回 / 换档和 `Event::Delivered` 带岛编号（`HOME_REGION = 0`），宿主只往按岛分开的邮筒投信、不再拿 `Region`；出站缓冲加「主控执行期间先进岛的出站信箱」（第 5b 步留下的全局类 / 控制台命令）；`WorldConfig` / `ServerConfig` 加 `island_drive`，缺省齐步，`BAMBOO_ISLAND_DRIVE` 环境变量可改。实测见表下「第 6 步的实测」。**岛的停放 / 运行所有权和区块信箱**：`Island: Send`；Director 的 `ToRegion` 改为进岛的信箱（带目标岛编号），宿主不再在 `pump` 里直接拿 `&mut Region`；存盘快照改成「主控出名单、岛编码交回」；宿主记的子项改记主控的剖析器。两种驱动：**齐步**（岛借给主控，信箱在现在的同一个时点当场应用，精确模式；这一步仍是缺省）和**分离**（信箱在岛下一刻开头应用）。所有要求逐位一致的测试、对拍、探针在这一步改成显式选齐步 | `director_host.rs` 的 `pump` / `flush_region_mail_into` / `region_message` / `poll_io` / `save_loaded_dirty` / `snapshot_all_dirty` / `shutdown`；`mc-bamboo-world/src/director/state.rs` 的 `RegionMessage`（加岛编号）；`world.rs`、`region/column.rs`、`profiler.rs`；测试、`mc-bamboo-vanilla`、`tools/probe.sh` 的模式选择 | 齐步：逐位一致；分离：交付晚一刻 | 显式齐步跑全部测试和对拍；缺省配置下全套测试和探针都过；分离模式跑 `BAMBOO_TEST_LOADED` 测试，按「交付晚一刻」调整的少数断言单独列出；`inline_and_pool_produce_identical_worlds` 两种驱动都过 | 大（~1000） | **是**（宿主的邮件路径、`RegionMessage`） |
| 7 | ✅ **已完成（2026-11-17）**：新 `island_pool.rs`（`IslandPool`：常驻线程 `bamboo-island-N`、每条 64 MiB 栈、岛尽量回到上次的线程、`catch_unwind` 把 panic 连同岛交回）；`World.island` 改成 `IslandSlot`（`Box<Island>`，派出时整个拿走、主控这时只拿得到自己的状态）；分离驱动的一刻分三段：① 岛停放：收上一刻的信、主控收包、边界 / 天气 / 时间；② 岛在 tick 线程上跑（光照到发区块，最后照名单编存盘快照），主控同时推进 Director（只投信）；③ 岛交回：回执和排队的全局改动、延迟广播、刷出站缓冲、下一份存盘名单、`/save-all`、关服。齐步（`island_threads = 0`）照原顺序内联、一行不差。岛的 panic 在主控线程上 `resume_unwind`，走世界线程原来的崩溃路径；tick 线程全没了也当崩溃；关服请求到达时岛还在跑，再等 10 秒不回来就不等了（全局状态照常写盘、放锁、列出存不下来的区块）。剖析器加 `master_busy` / `island_busy` / `island_handoff`。`bamboo.toml` 的 `island_threads` 取代第 6 步的 `island_drive`，`BAMBOO_ISLAND_THREADS` 取代 `BAMBOO_ISLAND_DRIVE`（盖过文件和缺省、不写回文件），桌面端设置页同步加一项；**验收后缺省改成 1**。实测见表下「第 7 步的实测」。**tick 线程池和调度（仍一个岛）**：主控线程（20 TPS，`Global` + Director）+ tick 线程池；岛按截止时间派发、跑完交回；分离驱动下主控和岛并行；`bamboo.toml` 加 `island_threads`（0 = 齐步、内联）。**验收通过后缺省改成 ≥ 1（分离驱动）**，具体值按测量定。总设计稿第 12 节第 3 条（等子任务的线程只能帮子任务）这一步不涉及，岛内还没有子任务 | 新 `island_pool.rs`、`world.rs`、`island.rs`、`director_host.rs`（关服拆出不带岛的一条）、`profiler.rs`、`mc-bamboo-server/src/server.rs`、`config.rs`、`config_files.rs`、桌面端设置、`tools/probe.sh`、`mc-bamboo-vanilla` 两处显式齐步、`docs/configuration.md` | `island_threads = 0`：逐位一致；缺省（分离）：同第 6 步分离模式，另有两处同一刻内的先后变化（准则写计分板、延迟广播排到岛这一刻的包后面，同一次刷出） | 显式 `island_threads = 0` 跑全部测试和对拍；**缺省配置下全套测试和探针都过**；量「主控 ↔ 岛交接」每刻的额外耗时（目标 < 50 µs）；`islands-1` / `normal-*` 打开 / 关闭同轮对比 | 中（实际 ~1800，含测试） | 否 |
| 8 | ✅ **已完成（2026-11-17）**。**分区器，只算不用**：主控按 3.1 维护 section 计数和岛的划分、待合并 / 待拆分，统计岛数、大小、每秒合并 / 拆分次数，断言不变量；不真的拆（运行时行为不变） | 新 `island_partition.rs` / `island_partition_stats.rs` / `world/island_partition_tests.rs`；`director_host.rs` 的 `pump`（把 `LevelChange` 和 owner 事件喂给分区器）；`mc-bamboo-world/src/director/state.rs`（`classify` 的结果对外暴露）；`profiler.rs`、`lib.rs`、`world.rs` 一行模块声明、`run-heavy.sh` 的 `ISLANDS` | 不变 | `cargo fmt` / `clippy --workspace --all-targets` 0 警告；`cargo test -p mc-bamboo-game -p mc-bamboo-world` 与 `BAMBOO_TEST_LOADED=1` 全过；`ticket-oracle` 400 场景 0 不一致、`chunk-content` 139/139、`oracle java` 10114 例 0 fail、hy4 探针 719/719；随机玩家 / 随机票 5 种子 × 400 刻逐刻断言不变量、增量 == 全量、漏合并改坏确认、拆分迟滞 200 刻与取消；`=2` 的正式 `World::tick` 全量对照不报错；7 场景统计见 `docs/profiling.md` 第 15 节：`islands-K` 稳态正好 K 个岛（25 / 36 个 section）、合并 / 拆分都是 0 / 秒，`island_partition_ns` p50 0.2–1.1 µs、合计 mean 0.29–21.2 µs/刻，`=2` 全量对照 0.19–0.24 ms/刻；3 轮 A-B-A 中位数 `islands-1` 0.851、`normal-20-walk` 1.005，在噪声内 | 中（~800） | **是** |
| 9 | ✅ **已完成（2026-11-17）**：新 `region/island_split.rs`（`Region::split_off_island(chunks, players, random)` / `absorb_island(other, delta)`、`IslandPart`、`IslandRandom`），`Island::split_off_island` / `absorb_island` 再分区块信箱、存盘名单和报告；每一类怎么拆、怎么并、要不要平移见 3.3「第 9 步做完以后」，随机源的待定事项见 3.3「岛的随机源」；`rebase_clock` 有了正式调用者，去掉 `expect(dead_code)`。实测见表下「第 9 步的实测」。**岛的拆合（离线）**：`Region::split_off_island(chunks, players)` / `absorb_island(other, delta)`，补齐 3.3 表里缺的零件；还不在运行时调用 | 实际：`region/island_split.rs`（新）、`region.rs`、`island.rs`、`mc-bamboo-world` 的 `store.rs` / `light/{mod,level,storage}.rs`、`mc-bamboo-blocks` 的 `entity/store.rs` / `ticks.rs` / `block_entity.rs` / `block_entity/watch.rs` / `level.rs`、`tick_levels.rs`、`entity_tracking.rs`、`entity_sync.rs`、`player_grid.rs`、`links.rs`、`lectern.rs`、`chunk_encoding/cache.rs`、`region/dirty.rs`、`region/partition.rs`、`profiler.rs`、新测试 `world/island_split_tests.rs`；没动 `region/unit.rs`（单元拆合原样保留，岛的拆合另起一套入口） | 不变（运行时不调） | 拆了再合的内容指纹（方块、方块实体、计划刻含顺序、实体去掉网络 ID、容器、追踪配对）逐位相同；平移和不平移的对照；微基准：1000 个区块的拆 / 合耗时（目标 < 5 ms） | 大（实际 ~3400，含测试） | 否 |
| 10 | **多岛上线（验收后缺省开）**：分区器驱动真实的拆合；交付按岛路由、合并后再交付；进服 / 离开 / 跨岛 `/tp` 走离境和到达；聊天、全局帧、跨岛命令扇出；`bamboo.toml` 加 `islands`，**这一步验收通过后缺省改成 `true`**，`islands = false` 保留给对拍和测试 | `world.rs`、`director_host.rs`、`island_partition.rs`、`island_pool.rs`、`command/teleport.rs`、`secure_chat/deliver.rs`、`chat.rs`、`command/*`、`docs/configuration.md` | 打开时：第 13 节差异清单新增的几条 | 第 6.2 节的多岛测试；**缺省配置（多岛开）下全套测试和探针都过**，显式精确模式下逐位一致照旧；`islands-K` 和第 14 节基线对比 | 大（~1500） | **是**（`Deliver` 的目标岛） |
| 11 | **长时间压力验收和调参**：长时间压力、合并拆分频率和等待时间（总设计稿 17.2）、分区器迟滞参数和 `island_threads` 的缺省值按测量调整、把差异写进第 13 节。缺省值已在第 7、10 步改过，这一步不再改开关 | 文档、`tools/mc-bamboo-loadtest`、必要时 `config_files.rs` 的缺省值 | — | 见 6.2 | 小 | 否 |

**第 1–3 步的实测（2026-11-17）**：6.1 的验收全过（`cargo test --workspace` + `BAMBOO_TEST_LOADED=1` 共 4313 个测试 0 失败，11 个对拍 0 不一致，`oracle java` 两种配置 10114 / 0 fail，hy4 探针和 `--loaded` 两遍共 5 次 719/719）。性能同轮 A-B-A（第 0 步合并点对第 3 步末），每场景 5 轮、每次 20 秒，逐轮 `tick_total` 均值比的中位数：`islands-1` 0.904、`spread-16` 1.018、`normal-20-walk` 1.022；同机 A/A 对照（`islands-1`）0.956，单轮范围 0.84–1.07，三个场景都在噪声内。微基准 7 轮 A-B 交替：`cache_bench`（关缓存）0.994；`ticks_bench` 总中位数 1.010（rolling 1.006 / 1.018 / 1.034），只改了计划刻热路径上「最近排刻时刻」的编码，单独对照这一处是中性的，剩下的差别只出现在测试二进制里（第 3 步往同一个测试二进制加了不少测试代码），正式二进制的 `cache_bench` 没有。

**第 4–5b 步的实测（2026-11-17）**：按压缩验收每步跑 `fmt --check`、`clippy --workspace --all-targets` 0 警告、`cargo test -p mc-bamboo-game`（第 4 步另加 `-p mc-bamboo-server`）和 `BAMBOO_TEST_LOADED=1` 各 978–979 个 0 失败、`entity-oracle` / `move-oracle` / `menu` 0 不一致、`oracle java --weather clear` 10114 / 0 fail、hy4 探针 719/719；第 4 步的交错发包测试做了改坏确认（去掉按序号排序就失败），第 5b 步的分类测试也做了（把 `setworldspawn` 改注册成本岛类就失败）。**排队实现**：`BAMBOO_TEST_GLOBAL_PORT=queued cargo test -p mc-bamboo-game --lib` 979 个里 2 个不同，都是同一件事：离开时「离开了游戏」从实体移除包之前挪到之后（`leave_tells_the_others_in_order`、`entity_sync::tests::leaving_while_watched_removes_the_entity_before_the_tab_entry`，包的内容不变）。另做了一次只用于对照的试验：把交回挪到出站刷出之后（模拟多岛时主控的帧经信箱晚一刻），24 个测试不同，全是「反馈、Tab 名单、计分板 / 队伍 / Boss 栏的包在发令的那一刻还没到」，没有内容差别。性能同轮 A-B-A（第 3 步合并点 `45ec5a2b` 对第 5b 步末），3 轮、每次 20 秒，逐轮 `tick_total` 均值比的中位数：`islands-1` 0.982、`normal-20-walk` 1.099；同机同时段 A/A 对照（`normal-20-walk`）3 轮 0.755 / 1.523 / 0.773，机器负载 22–58，两个场景都在噪声内，但这一轮的噪声比第 1–3 步那次大得多。

**第 6 步的实测（2026-11-17）**：合并点是 `main`（含第 8 步分区器，`4721ce9b`），合并时分区器挂在 `pump` 上的两处增量记账原样保留，每刻收尾的 `finish_island_tick` 从宿主的 `tick` 挪到 `World` 里调（宿主不再拿 `Region`，统计写进岛的剖析器）。`fmt --check`、`clippy --workspace --all-targets` 0 警告、`check -p mc-bamboo-vanilla --no-default-features` 0 警告；`cargo test -p mc-bamboo-game -p mc-bamboo-world -p mc-bamboo-server` 全过（game 1002 个）；`BAMBOO_TEST_LOADED=1` 显式齐步 1002 / 0 失败；**分离驱动**下 `BAMBOO_TEST_LOADED=1` 1002 / 0 失败（有一次跑里 `worldgen::tests::in_flight_terrain_is_computed_once_and_shared` 失败，是进程级地形缓存和并行测试互相驱逐，和驱动无关，单独重跑和整套重跑都过）；`BAMBOO_TEST_GLOBAL_PORT=queued` 仍是第 5b 步那 2 个不同、没有多；`inline_and_pool_produce_identical_worlds` 改成齐步 / 分离 × 内联 / 4 线程，四份盘上字节两两相同。对拍（全部显式齐步）`ticket-oracle` 400 场景 0 不一致、`chunk-content` 139、`storage` 3887、`saved-data` 通过、`entity-oracle` 31 例 709 步 0 不一致、`machine` 0 不一致、`oracle java --weather clear` 10114 / 0 fail；hy4 探针 719/719，`tools/probe.sh --loaded` 两遍 719/719、三项区块顺序判据都过、两次正常关服、`world-dir` 通过。**分离模式按「交付晚一刻」调整的断言**（齐步下全是空操作）：测试脚手架 `receive_all_chunks` 等到邮筒空、`director_mail` 把邮筒里的信算进积压、脚下 3×3 的检查把「已投进邮筒 / 已收进信箱、下一刻豁免预算交付」算作到位（`loaded_tests::assert_footprints_loaded`、`fuzz::check_footprints`）、`assert_tick_sets` 在邮筒里还有换档时不和票表比；逐条加一刻的 11 个：`two_players_watching_one_chunk_get_the_same_shared_frame`、`an_oversized_chunk_kicks_its_requester_without_caching_an_empty_frame`、`clicks_before_the_teleport_is_confirmed_change_nothing`、`a_chunk_waiting_for_reclaim_stops_ticking_as_soon_as_its_ticket_goes`、`teleport_lands_on_a_loaded_footprint_and_the_view_follows`、`a_saved_world_delivers_the_nearest_chunk_first`、`join_writes_the_ten_steps_in_order_in_one_tick`、`first_batch_is_exactly_the_three_by_three_around_the_spawn`、`no_more_chunks_without_an_acknowledgement`、`walking_before_the_chunks_arrived_drops_them_without_forgetting`、`save_all_sends_success_after_handoff`（`/save-all` 等岛交回再过栅栏，成功消息晚一刻）；`save_and_reopen_resumes_game_time_day_time_and_scheduled_ticks` 的比较基准挪一刻（区块不在岛上时计划刻不走，相对到点时刻从交付那刻算）。全部只是时序，包的内容、顺序和盘上字节都不变。另加 3 个驱动测试（分离的信件等下一刻、分离存盘照名单晚一刻编码、分离 `/save-all` 并进未编码的名单）和出站信箱的逐字节对照测试。性能同轮 A-B-A，3 轮、每次 20 秒，逐轮 `tick_total` 均值比：合并 `main` 之前对第 4–5b 步合并点 `26587d82`，`islands-1` 1.008 / 0.991 / 0.980、`normal-20-walk` 1.013 / 1.042 / 0.938（负载 2–6）；合并之后对 `main`，`islands-1` 1.118 / 0.947 / 1.013、`normal-20-walk` 1.142 / 0.960 / 0.985，同时段 A/A（`islands-1`，`main` 对 `main`）0.969 / 0.874 / 0.656（负载 4–11），都在噪声内；**分离对齐步**（同一二进制、只换环境变量）`islands-1` 0.993 / 1.029 / 1.020、`normal-20-walk` 0.950 / 1.049 / 0.957，也在噪声内（这一步分离驱动仍在同一条线程上，只改了收信的时点）。

**第 7 步的实测（2026-11-17）**：`fmt --check`、`clippy --workspace --all-targets` 0 警告、`check -p mc-bamboo-vanilla --no-default-features` 0 警告；`cargo test -p mc-bamboo-game -p mc-bamboo-world -p mc-bamboo-server` 缺省配置（分离、1 条 tick 线程）全过（game 1009 个）；`BAMBOO_TEST_LOADED=1` 缺省配置和显式 `BAMBOO_ISLAND_THREADS=0` 各 1009 / 0 失败；`BAMBOO_TEST_GLOBAL_PORT=queued` 仍是第 5b 步那 2 个不同。**第 6 步按「交付晚一刻」调整过的断言这一步一条都不用再改**：分离驱动的时点（收信在收包之前、`/save-all` 等交回）原样保留，只是岛的一刻挪到了 tick 线程上。新测试 7 个（`world/tests/island_pool_tests.rs`，各带 120 秒超时，连跑 20 遍 20 过）：岛真的在 `bamboo-island-0` 上跑、三项耗时每刻都记；没有区块信件的场景里线程数 0 / 1 / 3 玩家收到的字节逐位相同（齐步 = 分离）；岛 panic 在世界线程上原样抛出、岛交回；岛卡住时没有关服请求就一直等、收到后等够超时（测试里调成 300 ms）就放弃，`level.dat` 写出且锁放掉（同目录立刻重开、游戏时间接得上）；`/stop` 两种驱动、分离的 `/save-all` 存下的都是最后一刻沙子掉下去之后的区块（直接读区域文件，不起世界），改坏确认：把 `/stop` 挪到岛的一刻之前、去掉 `/save-all` 的推迟都会被抓到。`inline_and_pool_produce_identical_worlds` 的分离那两份现在真在 tick 线程上跑，四份盘上字节仍两两相同。对拍（全部显式齐步；跑的时候不设环境变量，确认入口自己选了齐步）`ticket-oracle` 400 场景 4364 步 0 不一致、`chunk-content` 139、`storage` 3887、`saved-data` 通过、`entity-oracle` 31 例 709 步 0 不一致、`machine` 10 轮 1070 刻 0 不一致、`oracle java --weather clear` 10114 例 9823 过 / 0 fail / 291 排除；hy4 探针 719/719；`tools/probe.sh --loaded` 齐步一遍、**缺省配置（分离）一遍**（`PROBE_ISLAND_THREADS=default`），各两次 719/719、三项区块顺序判据都过、两次正常关服、`world-dir` 通过，分离那遍没有只差时序的项。顺手修了 `worldgen::tests::in_flight_terrain_is_computed_once_and_shared` 的偶发失败：原来读分片里的 `computed_by`，会被别的测试清缓存、或者填满缓存时淘汰掉；改成只数测试种子的计算次数（测试种子只有拿着 `TEST_CACHE_LOCK` 的并发测试用），两个会清缓存的测试也拿这把锁，断言仍是「恰好算一次」。

性能（release，`tools/mc-bamboo-loadtest/compare-island-threads.sh`，同一二进制只换 `BAMBOO_ISLAND_THREADS`，A = 0 齐步、B = 1 分离，3 轮 A-B-A、每次 20 秒，机器负载 3–14）：**交接**（`island_handoff`，派出到线程开跑 + 岛跑完到主控接着干活）分离那遍每刻 mean 31–48 µs、p50 29–37 µs、p95 49–74 µs，负载低的一轮 mean 14–21 µs，平均达到 < 50 µs 的目标（p95 在负载高时略超）；背靠背不睡的空世界里只多 6.4 µs / 刻（两边线程都还没睡下，不用唤醒），所以实际开销几乎全是线程从睡眠里被唤醒两次。逐轮 `tick_total` 均值比 B / A：`islands-1` 0.975 / 0.916 / 0.948、`normal-20-walk` 0.920 / 0.364 / 0.997（第 2 轮两次 A 差了 40%，是噪声）、`spread-16` 1.039 / 0.988 / 0.912；同时段 A/A（`islands-1`，两边都是 1）1.173 / 0.927 / 0.489，都在噪声内。**两条线程各自每刻的耗时**（分离那遍的 mean）：`islands-1` 主控 0.10–0.58 ms、岛 1.65–5.56 ms；`spread-16` 主控 0.39–0.44 ms、岛 6.28–6.85 ms；`normal-20-walk` 主控 1.03–1.38 ms、岛 0.38–1.14 ms。**整刻余量**：只有一个岛时整刻 ≈ 主控的串行段（收信、收包、天气时间、交回后的刷出和存盘编排）+ max(岛, 主控推进 Director) + 交接；和岛并行的只有 Director 的每刻推进（`delivery`，几到几十 µs），所以 `islands-1` / `spread-16` 这种岛占整刻 85–92% 的场景余量基本不变（多 30–40 µs 交接，省下一段很短的 Director 推进）。`normal-20-walk` 里主控最重的是收 IO / 计算池结果后的 Director pump（`director_events` 约 4 ms / 刻，记在 `inbox` 里），它在派发之前串行，这一步没有挪进并行段：挪进去要把「收回执」和「pump」拆开，并且会让 Director 先处理这一刻的收包、后处理上一刻的计算结果，事件先后变了，留到第 10 步和多岛一起做。缺省线程数定为 1：一个岛一刻只用得上一条线程，多给的线程闲着。

**第 8 步的实测（2026-11-17）**：验收结果和 7 个场景的岛数 / 大小 / 合并拆分
频率 / 分区器耗时写在第 6 节表内，完整数字与口径见 `docs/profiling.md` 第 15 节。
稳态布局里合并 / 拆分都是 0 次，所以「迟滞参数在真实玩家轨迹上合不合适」这个问题
还没答，留给第 11 步的长时间压力；模糊测试（随机移动玩家 / 随机挂票）和 `=2`
的正式 `World::tick` 全量对照覆盖的是分区器自身的不变量和增量正确性。

**第 9 步的实测（2026-11-17）**：`fmt --check`、`clippy --workspace --all-targets` 0 警告；`cargo test -p mc-bamboo-game -p mc-bamboo-world -p mc-bamboo-blocks` 全过（game 1013 个 + 集成 11 个、blocks 576 个、world 169 个）；`BAMBOO_TEST_LOADED=1` 显式 `BAMBOO_ISLAND_THREADS=0` 和缺省配置各 1013 / 0 失败；`entity-oracle` 31 例 709 步 0 不一致、`machine` 10 轮 1070 刻 0 不一致、`oracle java --weather clear` 10114 例 9823 过 / 0 fail / 291 排除；hy4 探针 719/719、正常关服、`world-dir` 通过。**离线测试**（`world/island_split_tests.rs`。场景：三组相隔 24–27 个区块的内容，每组一到两名玩家、漏斗链、在烧的熔炉、互相盯着的侦测器、流动的水、掉落的沙子、地上的物品、讲台、命令方块、带组件的箱子、红石记忆、等着重查的容器、打开的菜单、待发的讲台格子、没确认的传送；`randomTickSpeed = 0`，机器不取随机数；一刻用分离驱动在 tick 线程上跑的同一段 `run_detached_island`）：① 拆了马上合回来：4 个种子 × 30 刻里随机 3 个时刻、随机的组，和从没拆过的世界逐位相同（含网络 ID、版本号、跨组的先后），之后每刻的指纹和每名玩家收到的字节也逐位相同；② 拆开、两半各自跑 5–34 刻再合回来（中途两边各新放一个漏斗、在拆分前三条远期刻的同一（时刻、优先级）上各排三条，逼出 ordinal 撞号和顺序号交错）：4 个种子，合并那一刻和之后 20 刻每组的内容逐刻等于从没拆过（实体不比网络 ID 和由它派生的随机源，不比跨组的先后）；③ 合并时两边时钟差 3 / 17 刻，结果等于时钟相同；④ **改坏确认**：漏平移 7 类里任何一类（岛的时钟、计划刻、红石火把记录、方块实体、容器重查、讲台待发、传送确认）都被抓到；拆分时漏搬 23 类 `IslandPart` 里任何一类，按位置 / 按玩家的核对都报出那一类；岛这一层的区块信箱、存盘名单、存盘报告拆完手动塞回原岛，也都报出来。各张表自己的单元测试另测了按 uid / 入队序号保序、子索引一致、位置表指向压紧后的下标（原来的单元拆分漏了这一步，顺手修了）。**微基准**（`island_split_and_merge_bench`，release：2000 个已加载区块拆走 1000 个再合回，每个区块有改过的方块、4 条计划刻、一个漏斗方块实体、两个容器、一个地上的物品；每遍 7 轮取中位数，跑 7 遍）：拆 3.7–4.2 ms（单轮最大 6.4 ms，是每遍的第一轮）、合 1.7–1.9 ms，中位数达到 < 5 ms 的目标。拆的耗时大头：区块表 ≈ 1.1 ms、计划刻 ≈ 0.8 ms、方块实体和红石 ≈ 0.65 ms、实体 ≈ 0.4 ms、容器 ≈ 0.3 ms、`TickLevels` ≈ 0.25 ms；区块表和光照按要搬的个数预留容量省了约 15%（4.7–4.9 → 3.9–4.2 ms）。按位置的几张 `BTreeMap`（容器、组件、命令方块等）拆的时候要扫整张表，耗时和岛的总大小成正比、不只和拆走的部分成正比；拆分本来就有 200 刻迟滞，先不改。性能同轮 A-B-A（`main` 合并点 `a8d5cbfa` 对第 9 步末，缺省配置，3 轮、每次 20 秒，逐轮 `tick_total` 均值比 B / A）：`islands-1` 0.847 / 0.997 / 1.036、`normal-20-walk` 1.025 / 1.019 / 0.991，负载 3–14，在噪声内（运行时没有调用拆合）。

### 6.1 单岛阶段（第 0–7 步）怎么验

- 每步都跑：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets` 0 警告、`cargo test --workspace`、`BAMBOO_TEST_LOADED=1 cargo test -p mc-bamboo-game`、`cargo check -p mc-bamboo-vanilla --no-default-features` 0 警告、`tools/probe.sh --loaded`、hy4 探针、方块 / 实体 / 世界生成各 oracle。
- 第 1–5 步只有一种模式，上面这些就是逐位一致的验收。
- 第 6 步起有两种模式，上面这些分两遍：
  1. **显式精确（齐步）模式**：全部对拍、探针、要求逐位一致的测试，逐位一致。这些入口在第 6 步就改成显式选模式，不靠缺省值（第 9 节第 7 条）。
  2. **缺省配置**：全套测试和探针都过。第 6 步缺省仍是齐步，这一遍和上一遍相同；第 7 步验收通过后缺省是分离驱动，这一遍就是分离模式。分离模式下「交付晚一刻」引起的断言变化逐条列出来，确认只有时序、没有内容差别。
- 性能：`islands-1`、`spread-16`、`normal-20-walk` 同轮 A-B-A 交替（`compare-partition-overhead.sh` 的做法），中位数在噪声内；第 7 步另比「分离（新缺省）」对「齐步」。

### 6.2 多岛阶段（第 8–11 步）怎么验

多岛之间的先后本来就不固定，不能和精确模式逐刻比。替代办法：

0. **两遍照旧**：显式精确模式下 6.1 的全部逐位一致验收照旧要过；第 10 步验收通过后缺省是多岛，缺省配置下全套测试和探针也都要过。
1. **分开跑 = 一起跑**。同一个世界里放两组相距很远的内容（`islands-2` 的布局，去掉随机），一份强制切成两个岛、用确定性的单线程驱动（两个岛按编号轮流跑、同步推进），另一份每组单独起一个世界跑同样的刻数。每个岛的内容指纹和「单独跑」的那份逐刻相同。前提：岛的随机源只由岛自己的起源 section 派生，不依赖拆分的先后；天气不碰岛的随机源；实体网络 ID 不进指纹。
2. **拆合不改内容**。齐步驱动（所有岛每刻都跑一次）下，按随机时刻强制拆分和合并，和「从不拆」的单岛逐刻比内容指纹。为了排除随机数，场景用 `randomTickSpeed = 0`、不用随机数的机器（第 12 节九台机器里除了发射器、投掷器的都行）。改坏确认：故意漏平移一类时间、漏搬一类表，要能报出来。
3. **不变量的压力测试**。随机走动、随机传送、随机进出服的假玩家，tick 线程 1 / 4 / 8 条，每次主控处理交回时断言：不同岛的已交付区块不相邻；每个已交付区块恰好在一个岛；每名玩家在他所在区块的岛里；玩家追踪的区块都在他的岛里；实体网络 ID 全进程唯一；停放中的岛没有被调度。
4. **线程数无关**。没有跨岛交互（没人跨岛传送、没有跨岛命令）的场景里，每个岛的内容和线程数无关。
5. **性能**：`islands-K` 与 `docs/profiling.md` 第 14 节同轮对比（目标接近 14.2 的上限）；`normal-*` 和 `spread-16-players`（一个岛）不变慢；合并 / 拆分的次数、等待时间、单次耗时进剖析器。第 11 步的长时间压力在缺省配置（分离驱动 + 多岛）下跑。

---

## 7. 和别的分支的冲突

| 分支 | 在改什么 | 和本计划哪些步重叠 | 建议 |
|---|---|---|---|
| `wg-feature-view` | `director_host.rs`（`run_task` 的 Generate / Assemble、`partials`、`feature_deltas`）、`mc-bamboo-world/src/director/`（`Task::Generate.neighbors`、写范围、`pyramid.rs`）、地物阶段的任务结构 | 第 6 步（`director_host.rs` 的邮件路径、`state.rs` 的 `RegionMessage`）、第 8 步（`pump` 和 `classify`）、第 10 步（`Deliver` 的目标岛） | 第 0–5 步不碰 Director，可以马上开始并行做；第 6 步改的是 `pump` / `flush_region_mail_into` / `region_message` / 存盘那几个函数，和 `run_task` 不是同一段，但在同一个文件里，**等 `wg-feature-view` 合进 main 之后再开工**，避免两边都在大文件里改 |
| `wg-struct-gaps` | `mc-bamboo-worldgen/src/structure/` | 无 | 无 |

---

## 8. 风险、要先量的东西、对总设计稿第 6 节的修改建议

### 8.1 风险和要先量的

1. ~~**实体阶段在大 `Region` 里超线性**（`docs/profiling.md` 14.3）。不查清楚的话，岛的收益里有一部分其实来自「表变小」，单岛修好之后上限会回到 K 倍以下。列为第 0 步。~~
   **第 0 步已结案**：不是「表变小」或空间索引的缓存问题，而是 `pickup_for` 的
   全表扫描，是本该在单岛修掉的 O(K²)。修后同轮复测 K=8 的上限 6.09×（修前 8.78×），
   已经低于 K；`islands-K` 的基线数字见 `docs/profiling.md` 14.3 第 2 条。
   剩下要量的：`entity_tracking::sync` 每刻全表一次、 `update_pairings` 对每个实体扫
   全部玩家（在 `entity_sync_and_tracking` 阶段，K=8 时每组 0.16 ms），
   以及第 7 步的主控 ↔ 岛交接开销。
2. **主控和岛的交接开销**。只有一个岛时分离驱动比齐步多两次线程切换；第 7 步要量，目标每刻 < 50 µs。
3. **合并 / 拆分的单次耗时**。要搬几百到几千个区块；现有的逐柱取放每柱都重算邻居段计数，必须做整批搬（3.3）。第 9 步先做微基准，超过 5 ms 就要调大网格或分摊。
4. **主控是单点**（总设计稿 17.7）。票的等级传播一次玩家跨区块是 126–141 µs（`docs/chunk-tickets.md` 第 5 节），多岛之后所有岛的玩家移动都排在主控上。基线里 80 人游走时 `ticket_update` 只有 0.04 ms / 刻；飞行场景要另量。
5. **实体移进未加载区块的现状没核对**（4.1）。第 10 步之前读代码确认。
6. **随机数的分配**：天气、战利品序列、岛的随机源都会变。精确模式不受影响，性能模式写进第 13 节差异清单。
7. **分离驱动下交付晚一刻**会让少数已加载模式测试的时序断言变化，要逐条确认只是时序。
8. **分区器的迟滞参数**（4×4 网格、合并距离 2、拆分距离 4 + 10 秒）没有在真实玩家轨迹上验证过；第 8 步「只算不用」就是为了先量。

### 8.2 改总设计稿第 6 节的地方（已同意，2026-11-17 已同步进总设计稿）

第 1–7 条已改进 `docs/world-model-design.md` 第 6 节原文（6.1、6.2、6.3、6.4、6.6），并顺带改了 4.2 里两处引用 `time_offset` 的地方；第 8 条是对第 16 节的说明，不改第 6 节。

1. **6.1：岛按「票想要加载的区块」划分**（加载等级 ≤ 33，加上还没收回的），而不是「已交付或正在交付」。理由：合并在生成期间完成，不卡交付；玩家的视野不会因为中间几个没生成好的区块被拆开（4.2 的推导依赖这一点）。代价是岛比实际加载的稍大，按 4×4 网格本来就粗，影响可以忽略。
2. **6.3：去掉「每个区块束带 `time_offset`」，改为合并时把被吸收方的绝对时刻整体平移。** 理由：存盘格式里的时间全是相对的，绝对时刻只出现在 5.1 表里的七类内存数据中；合并本来就要搬这些表，顺手平移是一次线性扫描；每区块一个 offset 却要在每次读计划刻 / 休眠日程时多做一次加法，而且第 1 步已经证明计划刻热路径对这种常数很敏感。
3. **6.2：追赶规则写成「每个岛落后 2 秒以内连续追赶」**，和现状一致（总设计稿第 1.1 节和第 6.2 节原来互相矛盾）。已定（第 9 节第 5 条）。
4. **6.6：实体 ID 用一个共享的原子分配器，不预发号段。** 玩家已经这么做；实体 ID 不存盘；一次原子加的成本可以忽略。测试里比较内容时不比较网络 ID。
5. **6.6：补上「全局帧和跨岛聊天经岛的信箱，一名玩家只有他所在的岛写」**，以及签名聊天的接收方状态由接收者的岛维护（第 2 节第 2、3 条）。
6. **6.6：天气由主控用自己的随机源推进**（性能模式），精确模式仍用唯一的岛的随机源。加进第 13 节差异清单。
7. **6.6：命令按 4.4 分三类**，全局类和控制台命令在主控执行。原文「涉及别的 region 的由主控分发」只覆盖了第三类。
8. **第 16 节第 4 步「维度之间并行」**：Bamboo 现在只有主世界，这一条现在没有收益。本计划的第 1–7 步对应它的「精确模式成形」部分。

---

## 9. 已经定的事

2026-11-17 用户答复。每条先写原来的问题，再写决定。

1. **步骤和顺序**：照第 6 节的 0–11 做，第 0–5 步不碰 Director、可以马上开始，第 6 步等 `wg-feature-view` 合入。
   **决定：照建议。**
2. ~~**第 0 步**：先查实体阶段的超线性，再开始拆分。~~ 已做完（第 6 节第 0 步）；第 1 步随第 1 条一起开工。
3. **岛的划分依据**改成「票想要加载的区块」（8.2 第 1 条）。
   **决定：照建议。** 总设计稿 6.1 已改。
4. **`time_offset`** 改成「合并时整体平移」（8.2 第 2 条）。
   **决定：照建议。** 不在每个区块上存偏移；总设计稿 6.3、6.4 已改；第 3 步给 5.1 表里的数据加 `rebase(delta)`。
5. **追赶规则**：每个岛沿用现在的「落后 2 秒内连续追赶」，还是照总设计稿 6.2 的「最多 2 刻」？
   **决定：每个岛沿用「落后 2 秒内连续追赶」。** 总设计稿 6.2 已改。
6. **实体 ID** 用共享原子分配器、不预发号段（8.2 第 4 条）。
   **决定：照建议。** 总设计稿 6.6 已改；第 2 步实施。
7. **默认驱动**：原建议是第 6、7 步做完、多岛上线之前缺省仍是齐步，分离驱动和多岛都由 `bamboo.toml` 打开，第 11 步验收后再改缺省。
   **决定：改了，新模式默认开启**（用户原话：「这里我建议默认开启吧，因为是很大的优化点」）。落实方式：
   - 分离驱动（第 7 步的 `island_threads`）和多岛（第 10 步的 `islands`）**各自在那一步的验收通过后缺省就是开**，不再等第 11 步；
   - 精确 / 齐步模式保留为配置项，供对拍和测试用；
   - 所有要求逐位一致的测试、对拍、探针必须**显式**选精确模式，不能依赖缺省值（第 6 步加上这个显式选择）；
   - 第 6、7、10 步的验收包含「缺省配置下全套测试和探针都过」（6.1、6.2）；
   - 第 11 步改成「长时间压力验收和调参」。
8. **天气的随机源**：性能模式下由主控单独持有，天气序列和原版不同（8.2 第 6 条）。
   **决定：照建议。** 多岛时主控单独持有天气的随机源；精确模式仍用唯一的岛的随机源。总设计稿 6.6 已改；差异在第 10 步写进总设计稿第 13 节。
