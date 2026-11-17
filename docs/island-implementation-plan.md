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
| 8 | 计分板、队伍 | `Global.scoreboard`；`tick_criteria(&mut region)` 每刻读玩家 | 主控 | 岛把六个只读准则的变化作为效果交回；计分板帧由主控生成、经信箱下发 |
| 9 | Boss 栏 | `Global.boss_bars`，进服时 `on_join` | 主控 | 同上 |
| 10 | 玩家列表（Tab）、延迟 | `Global.roster`；`Inbound::Latency` 直接写 Global；每 601 刻广播 | 主控 | 进出、换模式是效果；延迟消息走主控队列；广播帧经信箱 |
| 11 | 管理名单（op / ban / 白名单）、闲置踢出 | `Global.admin`；本刻的变化攒在 `Region.admin_changes`；`tick_idle(&mut global, &mut region)` | 主控 | 名单命令在主控执行；玩家的权限等级变化作为消息发给玩家所在的岛；闲置时限从快照读，踢人在岛里做 |
| 12 | 聊天、签名聊天 | 每名接收者的 `player.chat.next_index` / `cache`（`secure_chat/deliver.rs:75-82`）、发送者的链状态，都在 `Player` 上；刷屏计数在 `Player` | 岛（跟着玩家） | 发送者所在岛校验签名后，把消息作为效果交给主控，主控扇出到每个岛，**各岛给自己的玩家逐个打包**（每名接收者的索引和缓存只由他所在的岛改）；跨岛的接收者晚 ≤ 1 刻 |
| 13 | 命令 | 全部在世界线程以 `(&mut Global, &mut Region)` 执行；控制台命令也是 | 分三类 | 见 4.4 |
| 14 | 实体 ID | 玩家：服务端静态原子计数；非玩家：每张 `EntityStore` 从 `1 << 24` 计数 | 进程级 | 改成一个共享的原子分配器（见 8.2 第 4 条）；不存盘，所以不影响存档 |
| 15 | 随机源 | `Region.random`（方块逻辑和天气）、`sound_seed_generator`、`loot_sequences`（`RandomSequences::new(0)`，不存盘）；实体各自带随机源 | 岛 | 拆分时按岛的起源 section 派生，合并时留大的那个；天气另用主控的 |
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
- **时点**：Director 每次 `run_updates()` 产出的 `LevelChange`（`classify` 为 `Load` / `Unload` 的那些）就是分区器的输入，在同一次 `pump` 里更新 section 计数。任何一个 section 从空变非空时查它周围切比雪夫距离 ≤ 2 的 section：
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

拆岛的时候拿它们当零件**还缺这些**：

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

### 4.4 命令分三类

`command/catalog.rs` 现在注册了 51 个顶层字面量（含 `tp`、`tell`、`w` 这类别名）。

| 类 | 命令 | 多岛时 |
|---|---|---|
| 只动本岛 | `setblock`、`fill`、`clone`、`particle`、`playsound`、`loot`、`random`、`enchant`、`clear` / `give` / `gamemode` / `stopsound` / `spawnpoint` / `title` / `tellraw` / `kick` / `transfer`（目标都在本岛时） | 当场执行，同现在 |
| 只动全局 | `time`、`weather`、`gamerule`、`difficulty`、`worldborder`、`seed`、`setworldspawn`、`defaultgamemode`、`op` / `deop` / `ban` / `ban-ip` / `banlist` / `pardon` / `pardon-ip` / `whitelist`、`save-all` / `save-on` / `save-off`、`stop`、`setidletimeout`、`version`、`help`、`list`、`scoreboard`、`team`、`bossbar` | 岛把（来源、命令行）作为效果交回，主控执行，反馈和帧经信箱回来 |
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
  | 补：计划刻表自己的 `now`、每区块最近排刻时刻（`ChunkActivity` 的 `ChunkMark`，给存盘修剪和单元划分用） | `ticks.rs` | `now` 不搬；最近排刻时刻随区块拆、`absorb` 取较大值 | 同上。原来用「时刻 + 1，0 表示没有」编码，平移到 −1 会和哨兵撞上，改成 `i64::MIN` 哨兵 |
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
  一处要记住的语义：漏斗 `None` 的「绝对 0」不平移，所以「平移后再跑 = 时钟本来就差 d」照样成立；但只有当别的漏斗的 `tickedGameTime`（= 时钟 − 年龄）都 > 0 时，它才和官方「读盘后的漏斗是很久以前」一致。时钟接近 0 时（Bamboo 现在每次启动 `GameTime` 都从 0 起，见最终报告的疑点）两者可能差一刻冷却。第 3 步的随机测试因此让两边时钟都不小于 0。
- **合并时把被吸收方整体平移 `delta = 吸收方.now − 被吸收方.now`**：上表除最后两行外各有 `rebase(delta)`，`Region::rebase_clock` 一次做完（第 3 步）。不在每个区块束上存 `time_offset`（对 6.3 的修改，理由见 8.2 第 2 条）。拆分时两边时钟相同，不用平移。

### 5.2 追赶规则

- 现在：落后 2 秒以内连续追赶（`MAX_CATCH_UP`）。总设计稿 6.2 原来写的是「最多连续追赶 2 刻，之后 `下次开始 = max(现在, 理想时间)`」。两者不一致，**已定（第 9 节第 5 条，2026-11-17）：每个岛沿用现在的 2 秒**，理由是用户已经决定不追求精确节拍，而且 2 秒内追平能让卡了一下的岛的模拟时间和世界时间重新对齐。总设计稿 6.2 已同步改。
- 主控的 `GameTime` 不追赶，固定 20 TPS：主控自己很轻（基线里 < 0.05 ms / 刻）。

### 5.3 现在直接读全局游戏时间、以后要读岛的时间的地方

源头只有 4 处，其余都经过它们：

| 源头 | 位置 | 下游 |
|---|---|---|
| `run_block_ticks(region, global.time())` | `world.rs:737` → `interaction.rs:104` | `BlockTicks.now` → `World::game_time()` → 方块逻辑（约 15 处计划刻、约 40 处方块实体和休眠、2 处红石火把），以及线程局部的 `SLEEP_CLOCK`（`block_entity.rs:116-134`，`World::new` 里设） |
| `record_tick_counts` 的 `global.time() + 1` | `world.rs:568` | 剖析器的「到期计划刻数」 |
| `run_partition_stage(global.time())` | `world.rs:725` | 单元划分的 `last_active` |
| `BenchWorld::game_time` | `bench.rs:67` | 压测输入的相位 |

已经读 `region.tick` 的（约 12 处）：容器重查、讲台、传送确认、红石缓存。保持 `GameTime` 的（约 15 处）：`/time`、`SetTime`、`level.dat` 的 `Time`、世界边界渐变、战利品条件的 `game_time` / `day_time`。Director 用自己的 `director_tick`（约 12 处），留在主控不动。

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
| 1 | **主控和岛分家（只搬字段）**：`World { master: Master, island: Island }`；`Master` 收 `global`、`host`、节拍、票中心、`director_tick`、存盘会话、压测钩子；`Island` 收 `region`、`entity_threads`。调用顺序一行不变 | `world.rs`、`world/*.rs`、`bench.rs`、`command/console.rs` | 逐位一致 | `cargo test --workspace`、`BAMBOO_TEST_LOADED=1`、全部 oracle、`tools/probe.sh --loaded`、`islands-1` / `spread-16` 同轮 A-B-A 在噪声内 | 中（400–700，机械） | 否 |
| 2 | **实体 ID 共享分配器**：每个 `World` 一个 `Arc<AtomicI32>`，从 `1 << 24` 起，`EntityStore` 不再自带计数器（不用全局静态量，免得并行跑的测试互相影响） | `mc-bamboo-blocks/src/entity/store.rs`、`entity/mod.rs`、`mc-bamboo-game/src/region.rs` | 逐位一致（一个岛时号的序列相同） | 同上；加「两张表交替分配不撞号」的单元测试 | 小（~150） | 否 |
| 3 | **岛的时钟**：`BlockTicks.now` 改由岛的时钟驱动，初值取 `GameTime`；去掉岛内对 `global.time()` 的 4 处读；给 5.1 表里的七类数据加 `rebase(delta)` | `world.rs`、`interaction.rs`、`region.rs`、`bench.rs`、`mc-bamboo-blocks` 的 `ticks.rs` / `level.rs` / `hopper.rs` / `cooking.rs` / `brewing.rs` / `campfire_entity.rs` / `sleep_clock.rs` / `block_entity.rs`、`lectern.rs`、`movement.rs` | 逐位一致 | 同上；每类一个「平移 d 后再跑 = 时钟本来就差 d 时跑」的随机测试，加改坏确认（故意漏平移一类要被抓到） | 中（~700） | 否 |
| 4 | **入站按序号归并**：服务端给每条 `Inbound` 盖一个进程级递增序号；按玩家分队列（队列挂在 `Player` 上，跟着玩家走），非玩家消息进主控队列；世界每刻把主控队列和玩家队列按序号归并 | `mc-bamboo-server` 的 `connection/play.rs`、`players.rs`、`server.rs`、`console.rs`；游戏层 `inbound.rs`、`world.rs`、`player.rs` | 逐位一致（序号就是现在的通道顺序） | 同上；探针；加「两名玩家交错发包、序号归并后顺序和单通道相同」的测试 | 中（~500） | 否 |
| 5a | **`Global` 快照**：`GlobalView`（规则、天气、时间和时钟、边界、难度、出生点、名单的只读部分）每刻发布一次，岛内所有读 `Global` 的地方改读它 | `global.rs`、`weather.rs`、`gamerules.rs`、`world.rs`、`command/*` 的读路径、`region.rs` 的快照字段 | 逐位一致 | 同上 | 中（~800） | 否 |
| 5b | **效果消息和命令分类**：岛对 `Global` 的写变成 `GlobalEffect`；`GlobalPort` 有两种实现——「直接」（立刻应用，单岛和精确模式用）和「排队」（交回时应用）；计分板准则、闲置踢出、延迟广播、Tab 列表拆成「岛算、主控应用」；命令按 4.4 分类，全局类走 `GlobalPort` | `global.rs`、`command/*`（51 个顶层字面量里约 25 个）、`chat.rs`、`secure_chat/deliver.rs`、`scoreboard/`、`admin.rs`、`world.rs` | 逐位一致（「直接」实现） | 同上；单元测试里用「排队」实现跑一遍全部命令测试，只允许反馈晚到、内容不变 | 大（~1500，可再拆两次合并） | 否 |
| 6 | **岛的停放 / 运行所有权和区块信箱**：`Island: Send`；Director 的 `ToRegion` 改为进岛的信箱（带目标岛编号），宿主不再在 `pump` 里直接拿 `&mut Region`；存盘快照改成「主控出名单、岛编码交回」；宿主记的子项改记主控的剖析器。两种驱动：**齐步**（岛借给主控，信箱在现在的同一个时点当场应用，精确模式；这一步仍是缺省）和**分离**（信箱在岛下一刻开头应用）。所有要求逐位一致的测试、对拍、探针在这一步改成显式选齐步 | `director_host.rs` 的 `pump` / `flush_region_mail_into` / `region_message` / `poll_io` / `save_loaded_dirty` / `snapshot_all_dirty` / `shutdown`；`mc-bamboo-world/src/director/state.rs` 的 `RegionMessage`（加岛编号）；`world.rs`、`region/column.rs`、`profiler.rs`；测试、`mc-bamboo-vanilla`、`tools/probe.sh` 的模式选择 | 齐步：逐位一致；分离：交付晚一刻 | 显式齐步跑全部测试和对拍；缺省配置下全套测试和探针都过；分离模式跑 `BAMBOO_TEST_LOADED` 测试，按「交付晚一刻」调整的少数断言单独列出；`inline_and_pool_produce_identical_worlds` 两种驱动都过 | 大（~1000） | **是**（宿主的邮件路径、`RegionMessage`） |
| 7 | **tick 线程池和调度（仍一个岛）**：主控线程（20 TPS，`Global` + Director）+ tick 线程池；岛按截止时间派发、跑完交回；分离驱动下主控和岛并行；`bamboo.toml` 加 `island_threads`（0 = 齐步、内联）。**验收通过后缺省改成 ≥ 1（分离驱动）**，具体值按测量定。总设计稿第 12 节第 3 条（等子任务的线程只能帮子任务）这一步不涉及，岛内还没有子任务 | 新 `island_pool.rs`、`world.rs`、`mc-bamboo-server/src/server.rs`、`config_files.rs`、`docs/configuration.md` | `island_threads = 0`：逐位一致；缺省（分离）：同第 6 步分离模式 | 显式 `island_threads = 0` 跑全部测试和对拍；**缺省配置下全套测试和探针都过**；量「主控 ↔ 岛交接」每刻的额外耗时（目标 < 50 µs）；`islands-1` / `normal-*` 打开 / 关闭同轮对比 | 中（~700） | 否 |
| 8 | **分区器，只算不用**：主控按 3.1 维护 section 计数和岛的划分、待合并 / 待拆分，统计岛数、大小、每秒合并 / 拆分次数，断言不变量；不真的拆 | 新 `island_partition.rs`；`director_host.rs` 的 `pump`（把 `LevelChange` 喂给分区器）；`mc-bamboo-world/src/director/state.rs`（`classify` 的结果对外暴露） | 不变 | 随机移动玩家 / 随机挂票的模糊测试逐刻断言不变量；增量和全量划分逐刻相同（照单元划分第 2 步的做法）；`islands-K` / `normal-*-fly` 下的统计 | 中（~800） | **是** |
| 9 | **岛的拆合（离线）**：`Region::split_off_island(chunks, players)` / `absorb_island(other, delta)`，补齐 3.3 表里缺的零件；还不在运行时调用 | `region.rs`、`region/unit.rs`（扩成岛）、`mc-bamboo-world/src/store.rs`、`mc-bamboo-blocks/src/entity/store.rs`、`tick_levels.rs`、`entity_tracking.rs`、`player_grid.rs` | 不变（运行时不调） | 拆了再合的内容指纹（方块、方块实体、计划刻含顺序、实体去掉网络 ID、容器、追踪配对）逐位相同；平移和不平移的对照；微基准：1000 个区块的拆 / 合耗时（目标 < 5 ms） | 大（~1200） | 否 |
| 10 | **多岛上线（验收后缺省开）**：分区器驱动真实的拆合；交付按岛路由、合并后再交付；进服 / 离开 / 跨岛 `/tp` 走离境和到达；聊天、全局帧、跨岛命令扇出；`bamboo.toml` 加 `islands`，**这一步验收通过后缺省改成 `true`**，`islands = false` 保留给对拍和测试 | `world.rs`、`director_host.rs`、`island_partition.rs`、`island_pool.rs`、`command/teleport.rs`、`secure_chat/deliver.rs`、`chat.rs`、`command/*`、`docs/configuration.md` | 打开时：第 13 节差异清单新增的几条 | 第 6.2 节的多岛测试；**缺省配置（多岛开）下全套测试和探针都过**，显式精确模式下逐位一致照旧；`islands-K` 和第 14 节基线对比 | 大（~1500） | **是**（`Deliver` 的目标岛） |
| 11 | **长时间压力验收和调参**：长时间压力、合并拆分频率和等待时间（总设计稿 17.2）、分区器迟滞参数和 `island_threads` 的缺省值按测量调整、把差异写进第 13 节。缺省值已在第 7、10 步改过，这一步不再改开关 | 文档、`tools/mc-bamboo-loadtest`、必要时 `config_files.rs` 的缺省值 | — | 见 6.2 | 小 | 否 |

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
