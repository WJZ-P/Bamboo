# 生物实体：整体设计

状态：**方向已由用户确认（2026-11-16）**。本稿是整体方案；针对单个行为的定制优化（例如僵尸找海龟蛋直接读调色板）不在这里，以后单独考虑。

依据：对 Paper（`63e41db9`）、Folia（`73378203`）、Leaf、Lithium（`2f3cbe1`）源码的调研，都是 26.3 对应版本。只借鉴思路，不抄代码。地基是总设计稿 `docs/world-model-design.md` 第 8 节（并行算、按顺序落地）和第 10 节（降级阶梯），岛的部分见 `docs/multi-region-tick-design.md`。

标注：`[事实]` 读源码或实测确认；`[判断]` 设计选择；`[待测]` 要等 Bamboo 自己有生物之后量。

## 0. 结论

1. 生物在算阶段读的是本刻不变的世界，所以感知、寻路、碰撞可以直接多线程跑，没有数据竞争。Leaf 的异步寻路读活世界、结果不可复现；Paper 和 Folia 在一个区域内部单线程 `[事实]`。
2. 寻路做成每刻一个独立的批处理阶段，结果下一刻生效。换来去重、排优先级、限预算、吃满所有核，结果和线程数无关。
3. 远处的生物分三档降频，三档的距离都可配置。
4. 不改变行为的提速从第一天就按这个写。
5. 收益现在给不出数字 `[待测]`：第一批生物做出来后先压测，再按实测排后面的顺序。

## 1. 用户已定的四件事

| 事项 | 决定 |
|---|---|
| 远处生物降频 | 三档（全速 / 降频 / 休眠），距离可配置 |
| 寻路结果晚一刻 | 默认开 |
| 刷怪上限按玩家算 | 默认开；精确模式保留原版的全局上限 |
| 开工 | 第 1 步（地基）现在做；第 2 步（第一批生物）等岛的单岛重构合并后开始 |

总原则：默认允许轻微改变原版行为（晚一刻、远处变迟钝这个量级）；每一项改变行为的手段都有开关，全部关掉就是精确模式，继续和官方逐刻对拍；同样的输入，1 线程和 N 线程必须跑出同样的世界。

## 2. 调研对照 `[事实]`

| 手段 | Paper | Folia | Leaf | Lithium | 改变行为 |
|---|---|---|---|---|---|
| 远处生物休眠（激活范围） | 有，默认开，约 32 格 | 沿用 | 改成圆形距离 | 无 | 是 |
| 大脑按距离降频 | 无 | 无 | 有，默认关，12 格外开始 | 无 | 是 |
| 异步寻路 | 无 | 无 | 有，默认关，读活世界 | 无 | 是 |
| 区域内多线程实体 tick | 无 | 无 | 无 | 无 | — |
| 区域之间并行 | 无 | 有 | 无 | 无 | 是 |
| 实体按区块段分桶索引 | 有 | 沿用 | 沿用 | 有 | 否 |
| 碰撞快路径 | 有 | 沿用 | 有 | 有 | 否（1e-7 量级除外） |
| 寻路格子类型预计算 | 无 | 无 | 无 | 有 | 否 |
| 寻路失败后冷却 | 失败 10 次后 40 刻内不再试 | 沿用 | 沿用 | 无 | 是 |
| 挤压次数上限 | 每刻 8 次 | 沿用 | 可选只推玩家 | 无 | 是 |
| 按玩家算刷怪上限 | 有，默认开 | 每区域各算 | 加异步计数 | 无 | 是 |

Paper 和 Folia 对寻路算法本身都没动。Folia 的刷怪上限每个区域各算各的，全服总量随区域数增长。

## 3. 一刻里生物怎么跑

```text
实体算阶段（并行）  对着本刻开始时的快照：感知 → 决策 → 沿已有路径移动、碰撞
                    要改世界的动作记成命令；需要新路径就发一条寻路请求
寻路阶段（并行）    收集本刻所有请求 → 去重 → 排优先级 → 按预算并行算
落地阶段（串行）    按（uid 升序，实体内序号）应用命令
下一刻开头          寻路结果交给生物
```

算和落地两段已存在（`mc-bamboo-blocks/src/entity/parallel.rs`）。新增寻路阶段。精确模式下寻路请求当场同步算完，语句顺序和原版一致。

## 4. 分项

### 4.1 寻路

不改变行为：

- 每种方块状态启动时算好寻路类型（自身的、作为邻居时的），形状依赖方块实体的算不出来就回退逐格计算（Lithium）。
- 每个 16³ 段维护“危险方块”计数，放置方块时增减；计数为 0 时判断格子不扫 26 个邻居（Lithium）。
- 维护“当前有路径的生物”清单，方块碰撞形状变了只通知它们（Lithium）。

改变行为（各有开关）：

- **寻路阶段，结果晚一刻。** 请求发出后这一刻沿旧路走或原地等。依赖结果的后续逻辑（写记忆、开始走）放在结果的处理函数里，不放在发请求的语句后面。
  - 去重：起点格、终点、体型、导航类型都相同的只算一次。
  - 优先级：有攻击目标的、离玩家近的先算；相同按 uid。顺序固定。
  - 预算：每个岛每刻最多展开的 A* 节点数。**按节点数不按时间**，结果才和机器快慢、线程数无关。超出的顺延，等得越久优先级越高。
- 失败冷却：连续失败超过 10 次，40 刻内不再试（Paper 的数值）。
- 未加载的区块当成阻挡，AI 和寻路永远不触发区块加载（Paper）。

先不做：特别长的路径跨多刻后台算（需要区块的只读快照）；群体流场。等实测证明预算不够再说。

### 4.2 三档降频

按到最近玩家的水平距离分档，距离从每个区块的附近玩家表取。

| 档 | 默认距离（可配置） | 做什么 |
|---|---|---|
| 全速 | 32 格内，或命中豁免 | 每刻完整 tick |
| 降频 | 32–64 格 | 移动和物理每刻照跑；感知和决策每 2–4 刻一次，越远越稀；寻路优先级低 |
| 休眠 | 64 格外 | 每刻只推进计时器（年龄、繁殖冷却、消失、拾取延迟）；每 20 刻一次完整 tick 但不主动走动 |

- 豁免（算全速）：有攻击目标、刚受伤、着火、没落地、在水流里、被活塞推、被拴着、幼年或求爱中、村民处于恐慌或袭击状态、点燃的苦力怕（清单来自 Paper）。
- 限量唤醒：休眠太久的轮流醒一会儿，每刻唤醒数有上限（Paper 的令牌桶）。
- 降频的子系统自带“上次运行时刻”，下次运行按经过的刻数补偿。
- 配置：三个距离各一项，降频档和休眠档可以分别关掉（距离设 0 表示不启用这一档）。

### 4.3 感知和决策

不改变行为：目标标志用位运算；大脑维护可启动行为清单、记忆没变不重查前置条件；关掉用不上的传感器；判断先便宜后贵（类型、距离，最后视线）；兴趣点加索引、由近及远，**保持原版并列时的先后顺序**（游戏里能观察到）。

改变行为：感知的是本刻开始时别的实体的样子（并行模型自带）；视线结果可缓存几刻，默认 1 刻即不缓存。

### 4.4 移动和碰撞

不改变行为：每种方块状态预算碰撞形状（空、整格、单盒各走快路径）；全空气段跳过；段内“超大碰撞箱”计数决定扫描盒要不要外扩；站着不动只查脚下薄盒；普通生物只和硬碰撞实体（船、矿车、潜影贝等）做碰撞，这类单独建索引。

改变行为：挤压每只每刻最多 8 次，上限下推到查询里；推挤读快照位置、发推力命令，落地时累加。

### 4.5 存储和索引

- 按区块 × 16 格段分桶（`EntityStore.grid` 已有）。
- 新增子索引：按类型、硬碰撞实体、当前有路径的生物。
- 每个区块的附近玩家表，只在玩家跨区块时更新，带版本号；降频、刷怪、追踪发包共用。

### 4.6 刷怪和消失

- 按玩家算上限，只统计自然生成的；计数增量维护。上限跟着玩家走，不需要跨岛的全局计数。
- 候选位置的检查并行，通过的按固定顺序提交，提交时再查一次上限。
- 消失规则不改。

### 4.7 和岛的关系

| 问题 | 做法 |
|---|---|
| 归属 | 在哪个岛的实体表里就归谁；只在生成、移除、岛合并拆分、传送时变 |
| 走到另一个岛 | 走不到：岛之间隔着未加载区块，到边缘就停 |
| 传送 | 源岛整个拿出 → 主控 → 目标岛下一刻开头放进世界（总设计稿 6.5） |
| 引用别的实体 | 存 uid，用时查表，查不到当作没有 |
| 带时间的字段 | 存岛的模拟时间，不存全局绝对时间 |
| 实体编号 | 主控给每个岛预发号段 |
| 跨岛可见 | 不会发生：玩家离自己岛的边缘至少 12 个区块，加岛间隔 8 个区块，超过最大追踪距离 128 格 |

### 4.8 和原版的差异

| 差异 | 玩家能感觉到什么 | 开关 |
|---|---|---|
| 寻路结果晚一刻 | 转向晚 50 毫秒 | 有 |
| 感知到的是本刻开始时的状态 | 无感 | 并行模型自带 |
| 降频档 | 远处生物反应慢 0.1–0.2 秒 | 有，距离可调 |
| 休眠档 | 远处生物基本不动；靠生物自己走进陷阱的农场变慢 | 有，距离可调 |
| 寻路预算用完后顺延 | 极端拥挤时个别生物多等几刻 | 有 |
| 寻路失败冷却 | 够不着的怪重试变稀 | 有 |
| 挤压上限 | 上百只挤一格时推力不完全 | 有 |
| 刷怪上限按玩家算 | 多人时每人附近的怪更均匀 | 有 |

## 5. 实施顺序

| 步 | 内容 | 状态 |
|---|---|---|
| 1 | 地基：方块状态的寻路类型表和碰撞形状快路径、区块段计数、附近玩家表、实体子索引 | 完成（五项见 5.1） |
| 2 | 第一批生物，精确模式下和官方逐刻对拍：牛、僵尸、骷髅 | 第一部分（生物地基 + 寻路 + 牛的行为）完成；第二段的生成入口（`/summon`、刷怪蛋）和网络同步完成，见 5.3；死亡掉落经验、存档、交互留下一轮 |
| 3 | 压测场景（牧场、追玩家、挤一格的刷怪塔），量移动、感知、寻路各占多少 | — |
| 4 | 按第 3 步的数字排顺序：寻路阶段、三档降频、挤压上限、按玩家刷怪 | — |
| 5 | 其余生物分批；村民单独一批 | — |

### 5.1 第 1 步的分项状态（2026-11-16）

| 分项 | 状态 | 实测 / 对拍 |
| --- | --- | --- |
| 方块状态的寻路类型表 | 完成 | `path-type java`：35723 个状态、71446 项比较、199 个「运行时算」、**0 不一致**；`--break-self-test` 1 处不一致。表在 `mc_bamboo_data::STATE_PATH_TYPE` / `NEIGHBOR_PATH_TYPE`，生成期由官方 `WalkNodeEvaluator.getPathTypeFromState` 和 `checkNeighbourBlocks` 导出。 |
| 区块段危险方块计数 | 完成 | 增量维护 + 读盘 / 生成期光照整段重算；4000 次随机写每步都等于全量重算，另有读盘 / 全段仙人掌 / 漏维护的改坏确认。`section_hash_cost` A-B-A-B 交替 6 个回合：无计数 52.9–53.4 ns/写、有计数 52.5–54.4 ns/写，中位数差 **+0.1 ns/写**，无可测常驻代价。`docs/performance-optimizations.md` §20。 |
| 碰撞快路径 | 完成（两项有收益） | 压测 `sample` 10 秒：`collect_block_collisions` 及形状查询约占算阶段 **36%**（2303 + 965 / 9129 主线程样本）。状态表预算「无碰撞 / 整格 / 单盒 / 其它」：无碰撞（5109 状态）和整格（3348 状态）走快路径，全空气段按段调色板一次跳过；**单盒快路径撤掉**（`joinIsNotEmpty` 的 1e-7 坐标合并不等于裸 Aabb 判交，单测抓到差异）；**站着不动缩小扫描盒撤掉**（会改变零位移时的逐格读数，没有可测收益）。压测 1 线程 compute 同轮 A-B-A-B 三对中位数：基线 59.7 → **48.8 ms/10 刻（1.22×）**。`move-oracle` 55 / 0、`entity-oracle` 31 例 709 步 / 0 不一致；单测含改坏确认。`docs/performance-optimizations.md` §21。 |
| 实体子索引 | 完成 | `EntityStore` 加按注册表类型的 uid 升序索引和硬碰撞实体索引（现在恒为空，三种实体都不是硬碰撞体）；取放仍 O(1)，`ids()` / `iter()` 的 uid 顺序不变。单测覆盖 spawn / take / put / remove / take_chunk / insert_chunk，含故意漏登记的改坏确认。 |
| 每个区块的附近玩家表 | 完成（只补查询） | 现有 `PlayerGrid`（64×64 格）和玩家表已经能直接回答：`players_within(x, z, radius, &players)` 给水平距离不超过半径的玩家及距离（uid 升序），`nearest_distance` 给最近距离。网格只出候选，真实位置来自玩家表，所以不依赖视距 / 已跟踪区块；位置仍只经 `Player::set_position` 改，跨区块没有额外维护表或版本号。单测覆盖格子边界、精确距离筛除、uid 顺序、移动、离开，以及「网格留着旧格子」的改坏确认。现在没有调用方，等第一批生物的降频 / 刷怪阶段接。 |

### 5.2 第 2 步第一部分：生物地基和牛（2026-11-17）

只做精确模式。代码在 `mc-bamboo-blocks/src/entity/mob/`：属性、`travel`（地面 / 水 / 岩浆）、
跳跃、重力、摔落伤害、受伤和死亡、无敌时间、击退；移动 / 视线 / 跳跃 / 身体朝向控制；
目标选择器（隔刻调度、随机数顺序和官方一致）；`WalkNodeEvaluator` + A\* + `PathNavigation`。
寻路留了接缝：`path::PathRequest` 进、`Option<Path>` 出，`find_path` 只读 `BlockView`，
以后搬到单独的寻路阶段不用改调用方。牛的 8 个目标都在（漂浮、惊慌、避水闲逛、看玩家、
随便看看、繁殖、诱惑、跟随父母）。繁殖目标的判定和走近伴侣都在，但喂食（设 `inLove`）和
生小牛没接，所以实际不会触发；没有幼崽，跟随父母也不会触发。这两项都不取随机数。

| 分项 | 状态 | 实测 / 对拍 |
| --- | --- | --- |
| `mob-oracle` | 完成 | 进程内官方 `ServerLevel`，36 例 46600 刻，位置 / 速度 / 朝向 / 血量 / 落地 / 路径节点 / 运行中目标 / 随机数种子逐刻按位比较，**0 不一致**。场景：平地闲逛 4 种子 × 2000 刻、无玩家、台阶、水边、水中、悬崖、围栏、摔伤、摔死、挨打惊慌、看玩家、诱惑（含三面墙和迷宫）、岩浆 / 仙人掌 / 岩浆块 / 甜浆果 / 蜘蛛网 / 细雪、玩家打死、两头牛互推。改坏确认（堆比较 `<`→`<=`、上浮 `>`、松弛 `<=`、去掉看玩家 / 环境音的一次随机数）都被抓到。 |
| 岛时钟平移 | 完成 | 生物的绝对时刻（`Mob.last_damage_stamp`、`Navigation.last_timeout_check` / `time_last_recompute`）和 `World::game_time` 同一时基，`Region::rebase_clock` 经 `ClockParts::entities` → `EntityStore::rebase` 平移；单测 `mob_time_stamps_follow_a_clock_rebase` 含漏平移的改坏确认。 |
| 每头牛每刻耗时 | 测了 | `cow_tick_cost`（测试专用分段计时，正式构建不编译）：平地 100 / 1000 头，约 6.2 / 6.5 µs，其中行走 4.3、方块效果 0.58、推挤 0.2–0.4、目标 0.13–0.18、导航和控制 0.11、寻路 0.15–0.17。测试世界的方块在散列表里，偏保守。 |
| 网络同步、存档、`/summon` | 网络同步和生成入口完成，见 5.3；存档已有读写实现但「读盘后真的跑起来」未接；交互未做 | 生成入口和网络同步由 `mob-oracle` 的 4 个逐字节出站包场景覆盖。 |

### 5.3 第 2 步第二段前半：牛接进游戏（2026-11-18）

- 实体随机源改为「生成时从所在岛的种子源取一个 `nextLong`」，读盘进表时同样取；
  `entity-oracle` / `move-oracle` / `mob-oracle` 的注入点跟着改，见 `docs/island-implementation-plan.md` 3.3 第 4 项。
- 生成入口：`/summon minecraft:cow`（带位置、可选 NBT）和牛的刷怪蛋（`SpawnEggItem.useOn`
  → `EntityType.spawn`）。不做自然刷怪。
- 网络同步：牛的生成包、元数据（含幼年标志、变回默认时也要发）、属性包、移动 / 转头 /
  速度包、受伤 / 死亡 / 环境音事件。游戏路径和逐刻对拍共用
  `entity_tracking::TrackedEntity::pairing_packets` / `step` 和 `event_packets`，
  `ServerEntity.sendChanges` 的量化、周期、强制同步、插值步数都按官方字节码对齐。
  官方一侧在 `MobOracle.java` 里跑真的 `ServerEntity`，用官方 `StreamCodec` 把每个包
  编成「包 ID + 包体」，Rust 侧逐包逐字节比较；4 个 `packets_*` 场景共 1600 刻。
- 状态：43 个场景 50000 刻 **0 不一致**；故意改坏（丢掉头部朝向包、改位置增量阈值、
  改位置量化）都能被对拍抓到。探针新增 9 项，`728/728` 全过。
- 未做：死亡掉落和经验球、读盘后牛真的进实体表跑起来、喂小麦 / 繁殖 / 成长 / 挤奶、
  拴绳和命名牌、并行模式的逐位一致性。


### 5.4 第 2 步第三段：牛的生活周期（2026-11-19）

- 死亡：击退、无敌帧、死亡动画 20 刻都在上一轮的地基里；这一轮把
  `LivingEntity.dropAllDeathLoot` 接进游戏世界。牛死时按真的
  `minecraft:entities/cow` 战利品表掉皮革 / 牛肉（着火或带 `#smelts_loot`
  时熟牛肉、抢夺附魔增加数量），经验按 `Animal.getBaseExperienceReward`
  拆成经验球。掉落物和经验球都进实体表、发配对包、能存盘。
- 经验球是新的实体类别：重力 / 阻力、朝最近玩家的吸引、岩浆里的随机位移、
  每 20 刻同值合并、6000 刻消失、官方 NBT（`Health` / `Age` / `Value` / `Count`）。
  玩家拾取已经接上：`Player.aiStep` 每刻只碰一颗附近的球，按 `takeXpDelay`、
  `count`、经验修补和 `take_item_entity` 包走官方顺序（见 5.6）。
  熔炉 / 砂轮那些「只取随机数不生成球」的地方本轮未动。
- 存档：`minecraft:cow` 和 `minecraft:experience_orb` 都进建模实体清单，读盘后
  放进实体表继续 tick。`archive::tests::a_loaded_cow_keeps_running_from_its_saved_state`
  验证读盘后的成年牛继续减求爱计时、幼年牛继续长大、位置被 AI 推动。
- 交互：`/summon` 之外的右键路径接上了 `serverbound::INTERACT`。喂小麦让成年牛
  进入 600 刻求爱（`InLove` + 爱心事件 18）、喂幼年牛按官方公式加速成长、
  两头求爱的牛配对生小牛（`Age -24000`、父母 6000 刻冷却、一颗经验球、
  长大时碰撞箱变化）。空桶右键成年牛得到奶桶，`Player.playSound` 的音源是
  PLAYERS，创造模式和背包满时按 `ItemUtils.createFilledResult` / `Inventory.add`。
- 出站包：伤害事件、死亡 / 冒烟 / 爱心实体事件、移除实体、掉落物和经验球的
  生成包、挤奶音效都走上一轮那条「事件只发给跟踪者、音效按范围」的路径，
  逐包逐字节对拍。死亡元数据按官方改成 `Pose.DYING`，不是滑翔标志位。
- 对拍：`mob-oracle` 52 个场景 50640 刻 **0 不一致**。新增 4 个死亡场景
  （玩家打死 / 摔死 / 烧死 / 抢夺 III）和 5 个交互场景（喂成年 / 喂幼年 /
  繁殖 / 挤奶 / 创造模式挤奶），逐刻比 `InLove`、掉落物和经验球、玩家手里的
  物品、以及出站包字节。故意改坏（掉落数量 +1、`experience_value` 分段改错、
  不读 `InLove`、求爱时间改成 500、奶桶换成水桶、幼崽年龄改成 -20000）
  都能被对拍或单测抓到。
- `chunk-content` 439 项、`entity-oracle` 31 例 709 步、`move-oracle` 55 例、
  探针 728/728 全过。
- 未做：拴绳、命名牌、并行模式逐位一致性；玩家经验系统与经验球拾取已在 5.6 接上。

#### 岛的拆合（新增的按实体状态）

- `EntityStore.departed_mob_events`（被丢弃生物留下的事件，按 uid，每刻清空，
  不带绝对时刻）在 `split_off` 里按 uid 跟着搬走的实体分配、在 `absorb` 里 append。
  已经离表的实体不在 `moved` 里，它的事件留在原岛；这类事件一刻内就会发出。
- `Mob.pending_breed`（同一刻内消费，不跨刻，不带绝对时刻）和
  `ExperienceOrb` 的字段（年龄、健康、值、数量、跟随的玩家 uid）全在实体行上，
  整行搬走即可，不需要改 `IslandPart` 清单；`region/island_split.rs` 本轮未改。


### 5.5 实体逻辑看得见游戏里的玩家（2026-11-20）

- `mc-bamboo-blocks` 的 `Level::for_each_player` 原来在正式世界是空实现，只有
  `testing.rs` 和几个对拍上下文给了玩家。于是 `LookAtPlayerGoal` / `TemptGoal`
  看不见人、经验球不会朝玩家飞；`mob-oracle` 用自己的假世界上下文，所以没暴露。
  现在 `RegionLevel`（精确驱动）和 `ReadOnlyWorld` / `SourceView`（并行算阶段）
  都把本岛 `Region.players` 按 `PlayerId` 升序映射成 `PlayerView`。
  顺序对应官方 `ServerLevel.players()` 的进服顺序（实体 ID 单调递增）；
  旁观者、死亡状态和隐身等不在这一层过滤，由 `NO_SPECTATORS` /
  `TargetingConditions` / `ExperienceOrb.followNearbyPlayer` 各自按官方条件处理。
- **为什么只看本岛够用**：`island_partition.rs` 把「票想加载的区块」（等级 ≤ 33）
  按 4×4 区块一个 section 计数，切比雪夫距离 ≤ 2 的非空 section 连成一个岛，
  岛间至少隔 3 个 section（≥ 8 个区块 = 128 格）；`docs/island-implementation-plan.md`
  4.2 由此推出「玩家的视野不跨两个岛」。牛的注视 6 格、诱惑 10 格、经验球跟随
  8 格都远小于这个间隔：能和实体发生这些交互的玩家，其加载票覆盖的区块和实体
  所在区块连通，必在同一个岛。`check_despawn` 用的是 32 格，也仍在间隔内；
  更大的跨岛目标（例如将来加入 `FOLLOW_RANGE` 很大的生物）需要跨岛查询，
  当前没有这类实体，这是已知的边界，不改分区器。
- 测试在真实 `World`（`Harness`）上：拿小麦的玩家让牛转头并走过去、空手不动、
  注视目标被选中、经验球朝玩家加速、旁观者不吸引经验球、并行算阶段结果相同。
  把 `RegionLevel::for_each_player` 退回空实现后，前三个正向断言全部失败，
  恢复后 `6/6` 通过。`mob-oracle` 用自己的上下文，不受影响。


### 5.6 经验球拾取（2026-11-20）

- `entity::pick_up` 现在是 `ExperienceOrb.playerTouch` 的实体侧：减 `count`，归零
  时 `discard`。游戏层 `experience_pickup.rs` 负责 `takeXpDelay`、
  `LivingEntity.take` 的 `take_item_entity` 包、`repairPlayerItems` 和
  `giveExperiencePoints`；`set_experience` 由下一刻 `ServerPlayer.doTick` 补发。
- `Player.aiStep` 的形状照字节码：附近实体先逐个碰（掉落物），经验球单独收集，
  最后用 `Util.getRandom(list, this.random)` 抽一颗碰。名单非空就取一次
  `nextInt(size)`，即使 `takeXpDelay` 没过也会取；`playerTouch` 自己判冷却。
  冷却设为 2 刻，每刻 `Player.tick` 先减 1。
- `repairPlayerItems` 按 `EquipmentSlot.VALUES` 扫主手 / 副手 / 四件盔甲，
  挑有 `minecraft:repair_with_xp` 且损坏的装备（每个附魔条目各算一个候选），
  用玩家随机源抽一件。`modifyDurabilityToRepairFromXp` 从数据包的
  `multiply` 因子算修复量（mending 是 2.0），`repair = min(repairAmount, damage)`，
  剩余经验按 `amount - repair * amount / repairAmount` 递归给下一件；
  没有装备可修时全部经验给玩家。改过的物品栏槽发 `container_set_slot`。
- 拾取音效不另发 `sound` 包：官方 `ExperienceOrb.playerTouch` 和
  `LivingEntity.take` 都只发 `take_item_entity`，客户端收到后本地播
  `entity.experience_orb.pickup`（字幕键 `subtitles.entity.experience_orb.pickup`）。


### 5.7 玩家死亡掉经验：未实现（2026-11-2x，已知差异）

- **现状**：Bamboo 的玩家不会死。`Player` 没有血量 / 伤害 / 死亡字段，
  `Player::view` 的 `alive` 恒为 true；`player_move.rs` 只记录 `fallDistance`，不转成伤害；
  没有玩家 `die` / `hurtServer` 路径，也没有重生流程。新合进来的 `/kill` 命令
  （`command/kill.rs`）遇到玩家目标会明确返回 `command.failed`，不会假装成功；
  它只对非玩家实体走官方 `LivingEntity.kill` / `Entity.kill`。
  因此「玩家死亡掉经验」目前没有可以接的调用点，也没有任何经验球生成、
  经验清零或 `keepInventory` 保留行为。
- **官方字节码要点**（等死亡流程做的时候照这个接，并对拍后再删本节）：
  - `Player.getBaseExperienceReward(ServerLevel)`：
    `keepInventory` 为真 **或** `isSpectator()` 时返回 0，否则
    `min(experienceLevel * 7, 100)`（`Math.min`，int 乘法可能溢出）。
  - `Player.die` / `LivingEntity.dropAllDeathLoot` 在 `keepInventory` 为假时把
    `experienceLevel` / `experienceProgress` / `totalExperience` 清零，并用
    `ExperienceOrb.award(level, position, reward)` 把上面的值拆成经验球
    （`tryMergeToExisting` 的 `nextInt(40)` 取数顺序和别处一样）。
  - `PlayerList.respawn` 在 `keepInventory` 为真时调
    `ServerPlayer.transferInventoryXpAndScore`，把物品栏、等级 / 进度 / 总经验和
    `Score` 原样搬到重生后的玩家；`enchantmentSeed` 也在这一段里一起复制。
- 这一轮没有引入半成品死亡系统；等玩家伤害 / 死亡 / 重生流程实现时，再一起接上
  `getBaseExperienceReward`、`keepInventory` 两条分支和官方对拍。

### 5.8 马、驴、骡、羊驼（2026-11-20，分支 `cx-horse`）

**已做**（代码在 `entity/mob/equine.rs` 和 `entity/mob/equine/`，按牛的粒度）：

| 项 | 官方 | Bamboo | 对拍 |
|---|---|---|---|
| 属性、生成 | `createBaseHorseAttributes`、`randomizeAttributes`、`Horse/Llama.finalizeSpawn` | `equine::attributes` / `finalize_spawn` | `equineSpawn` 64 题 |
| 繁殖遗传 | `setOffspringAttributes`、`createOffspringAttribute`、花色 / 力量继承，马驴生骡 | `equine::offspring` | `equineBreed` 32 题 |
| 每刻生活 | `AbstractHorse.aiStep` / `tick` / `hurtServer` / `isImmobile` | `equine/life.rs` | `zombie_equine_*` 20 场景 15258 刻 |
| 目标表 | `registerGoals` + `addBehaviourGoals`、`Llama.registerGoals`、`RandomStandGoal` | `equine/goals.rs` | 同上（running names 逐刻比） |
| 交互 | `Horse/AbstractChestedHorse/AbstractHorse.mobInteract`、`handleEating`、`Player.interactOn` 兜底 | `equine/interact.rs` | `equineInteract` 468 题 |
| 死亡掉落 | `dropAllDeathLoot`、`dropCustomDeathLoot`、`dropEquipment` | `equine/death.rs` | `equineDeath` 40 题 |
| 元数据 | `DATA_ID_FLAGS`、`DATA_ID_TYPE_VARIANT`、`DATA_ID_CHEST`、`DATA_STRENGTH_ID`、`DATA_VARIANT_ID` | `EntityData::Horse*` / `Llama*`，index 由生成器导出 | 配对 `set_entity_data` 逐字节 |
| 存档 | `addAdditionalSaveData` / `readAdditionalSaveData` | `equine::save` / `load` | `equineSave` 12 题往返 |
| 坐骑物品栏 | `openCustomInventoryScreen`、`ServerPlayer.openHorseInventory`、`HorseInventoryMenu`、`AbstractMountInventoryMenu.quickMoveStack` / `stillValid`、`ArmorSlot.setByPlayer` | 生物一半 `equine/inventory.rs`；菜单是工作台菜单的 `Kind::Mount`（`menu/station/mount.rs`） | 单元测试（开菜单的包、Shift 取放鞍、两次装备声、超距关闭）；没有官方逐题对拍 |
| 落地声 | `AbstractHorse.causeFallDamage` 的 `HORSE_LAND` | `equine::land_sound` | 单元测试；音效包不在对拍范围 |
| 羊驼回击、吐口水 | `Llama.LlamaHurtByTargetGoal`、`RangedAttackGoal(1.25, 40, 20)`、`Llama.spit`、`LlamaSpit`、`DamageTypes.SPIT` | `goals.rs` 的 `Goal::RangedAttack` / `LlamaHurtByTarget`，`equine/ranged.rs`，口水实体 `entity/spit.rs`（存档 `archive/entity/spit.rs`） | `llama_spit_far` / `_near` 两场景各 160 刻 0 不一致（羊驼和口水的位置、速度、随机源、目标名、口水同步包、玩家血量）；`chunk-content` 两道口水存档题 |
| `followMommy` | `AbstractHorse.followMommy`（只 `createPath` 不走） | `equine/life.rs` 的 `follow_mommy` | 单元测试；`Bred` 只能从存档来，逐刻场景没覆盖 |
| 自然生成、生成期刷怪 | `Horse/Llama/AbstractHorse.finalizeSpawn` 的组数据（`HorseGroupData`、`LlamaGroupData` 同组共享毛色，驴骡 `AgeableMobGroupData(0.2)`）、`getMaxSpawnClusterSize() == 6`、`Animal.checkAnimalSpawnRules` | `equine/spawn.rs`，`spawn.rs` 的 `SpawnGroup::Horse` / `Llama`，`natural.rs`、`natural_gen.rs` 各加一行 | `equineGroup`（同组 6 只逐只比属性、花色、年龄、左撇子、世界随机源、配对元数据）；`natural_spawn` 的 `horse_day` / `donkey_day` / `llama_day`；`gen_spawn` 里马科的落点和区域随机源上的 `finalizeSpawn` |

坐骑物品栏每次点击前从生物读回鞍、身体和箱子格，点击后把鞍位 / 身体位的每次 `setByPlayer`
按发生顺序在生物上重放（官方 `ArmorSlot.setByPlayer` 先调一次 `onEquipItem`，装备容器的
`setItem` → `setItemSlot` 再调一次，所以穿上会出两次声、取两次 `nextLong`），箱子格整份写回。
菜单的 `stillValid` 读实体表的公开快照（上一个实体阶段结束时的血量、箱子、位置）。

生物装备从六个人形槽扩成官方全部八个槽（加 `BODY`、`SADDLE`）。马科的全部状态都在
`Mob.equine` 里，跟着实体走，所以岛的拆分合并不需要新的按实体迁移。

**骑乘（2026-11-21，第五轮）。** 按上一轮查证过的形状做了，没有改阶段顺序：

- 玩家记 `riding.vehicle` / `last_vehicle` / `first_good` / `last_good`（`crates/mc-bamboo-game/src/riding.rs`，
  对应 `ServerGamePacketListenerImpl` 的同名字段）；载具记 `Mob.passengers`（只有玩家，按上车顺序）。
- 上车：马科 `mobInteract` 的 `doPlayerRide` 返回 `ride` 标记，游戏层在 `on_interact_entity` 里落
  `startRiding`（设朝向、写两边、`first_good` / `last_good` 从载具位置起算）。
- 下车：`riding::tick` 每刻看潜行键（`Player.rideTick` 的 `wantsToStopRiding`），按
  `AbstractHorse.getDismountLocationForPassenger`（`riding/dismount.rs`：主手一侧、另一侧、
  站立 / 潜行 / 游泳三个姿势、`DismountHelper` 的地面高度和碰撞判定）算落点，再清掉两边的关系；
  `Entity.removePassenger` 的 60 刻 `boardingCooldown` 在 `Riding` 里，每刻减一，`canRide` 拦下车后的立刻再上。
- 骑手位置：`riding::tick` 在实体阶段之后、玩家自己的 tick 之前写玩家表，位置是载具位置加
  官方 `getPassengerAttachmentPoint` 的缺省点（`EntityTypes` 的 `passengerAttachments`：
  马 1.44375、驴 1.1125、骡 1.2125、羊驼 `(1.37, -0.3)`，按载具朝向旋转；扬蹄时
  `AbstractHorse` 附加的 `0.15 * standAnimO` / `-0.7 * standAnimO` 已按 `EquinePublic::stand_anim_o` 建模，羊驼覆写不叠加）。
- 载具位置：`move_vehicle` 在世界线程处理输入时直接改载具行（`handleMoveVehicle` 的夹取、
  `wrapDegrees`、100 平方格的「移动太快」拉回、非法值断开），再用 `move(MoverType.PLAYER)`
  的扫掠（`sweep_movement`，步高取 `STEP_HEIGHT` 属性）和包里的位置比：水平差超过 0.25 格
  且旧位置没卡在方块或边界里就 `absSnapTo` 回原位并发回载具位置。实体之间的碰撞
  （`isEntityCollidingWithAnythingNew`）Bamboo 没有建模，这一步跳过。
- 控制权：`tickPlayer` 只在载具的 `getControllingPassenger()` 是这名玩家时记 `lastVehicle`，
  而马科要上了鞍才算有人控制。没上鞍的载具能骑、AI 照跑，`move_vehicle` 的位置不收，
  按「离载具超过一格或离上次重置 20 刻」发回载具位置。
- 朝向：马科 `tick_ridden` 把载具朝向设成骑手朝向、俯仰取一半，旧朝向、身体和头都对齐；
  玩家控制时 `isEffectiveAi()` 为假，AI 和 `travel` 都不跑，速度清零（和官方服务端一致）。
- 跳跃：`START_RIDING_JUMP` → `handleStartJump`（扬蹄加 `entity.horse.jump` 声），`STOP_RIDING_JUMP` 是空的。
- 开坐骑物品栏：`OPEN_INVENTORY` 按官方 `AbstractHorse.openCustomInventoryScreen` 的条件
  （驯服，且骑手就是这名玩家）打开，走和潜行右键同一个 `menu::station::open_mount`。
- 同步：`Entity.passengers` 进公开快照，像 `ServerEntity.sendChanges` 那样在名单变化时发 `set_passengers`。

**还没做**：载具移动校验里实体之间的碰撞（`isEntityCollidingWithAnythingNew`），
载具摔落伤害和 `clientVehicleIsFloating` 踢人；玩家下线时 `RootVehicle` 存进玩家档；岛拆分把
「载具 + 骑手」当成不可分的一组。
这几项都不改阶段顺序，但要么要改移动校验、要么要改存档和分区器，留给队长定。

**骑乘驯服（第六轮）。** `equine/tame.rs` 在官方优先级注册 `RunAroundLikeCrazyGoal`：
未驯服且有乘客时查找随机落点、以 1.2 倍速导航，按目标的半频 tick 抽签（每次抽
`nextInt(25)`，命中才抽 `nextInt(maxTemper)`）。成功写入现有的 `Owner` 和 `Tame` 并发
实体事件 7，保留乘客；失败把脾气加 5（限制在该物种范围内），清乘客名单、发怒和事件 6。
`riding::tick` 在已有阶段中清理玩家侧的关系，已有同步路径发送乘客名单变更。
骑乘期间，`AbstractHorse.aiStep` 跳过起始吃草的随机抽取，`LookAtPlayerGoal` 不选自己的骑手。
对拍用 `mob-oracle` 新增的 `zombie_equine_buck_*`（甩人）和 `zombie_equine_tame_*`（当场驯服）
场景，逐刻比位置、目标、随机源、脾气、驯服标志、骑手数量和主人 UUID，场景在甩人/驯服那一刻截断。
`riding::passenger_offset` 改用上面查正后的成年附着点（原先误用了幼崽的值）。

羊驼的口水是新的实体种类 `EntityKind::LlamaSpit`：生成走 `Command::ShootLlamaSpit`，
命中实体走 `Command::SpitHit`（落地时按主人当时的公开状态造 `spit` 伤害来源），
打到方块在算阶段直接 `discard`。同步层里口水和箭一样用 `InterpolationHandler.NO_OP`，
位置路径永远是线性的。`didSpit` 在 `EquineState` 里，跟着实体走，岛拆合不需要新迁移。

**脚步声。** 实体移动时，`motion::movement_emission` 调用 `equine::play_step_sound`，按
`AbstractHorse.playStepSound` 选择方块声型、木质脚步和骑乘奔跑节拍（羊驼走自己的覆写）。仅马的
`Horse.playGallopSound` 在奔跑声后从实体随机源取一次 `nextInt(10)`；结果为 0 时按年龄播喷气声，
静音只抑制声音、不跳过抽取。游戏层 `riding::on_move_vehicle` 的方块扫掠仍不产生移动声音或振动。

**与 main 合并（2026-11-21，第八轮）。** 把 main 的鱿鱼、鱼、桶装鱼、海龟合进分支：冲突按 main 的结构解
（`finalize_cow(kind, …)`、`check_spawn_rules` 按 kind 分派，`generated/` 直接取 main 的重新生成结果），
两边取并集保留 `MobInteraction` 的 `ride` / `discard` / `limit_creative`、`travelRidden` 与鱿鱼 `travel`、
马科与水生的目标 tick、马科 `playStepSound` 与 `MovementEmission.EVENTS`（游泳声只在 `emitsSounds()` 为真时
取两个 `nextFloat`，`play_sound` 不取数，所以马科的取数顺序不变）。`cargo check --workspace --all-targets`
通过；`mob-oracle --only equine / llama_spit / zombie_equine / wolf_ / natural_spawn / gen_spawn` 复跑全部
0 不一致。本轮另补上狼与羊驼的互动，见 11.7。

**复核 main 合并（2026-11-21，第九轮）。** 本轮开始时 `main` 已是 HEAD 的祖先
（`git merge main` 报 `Already up to date`），按 main 的结构核对了 `spawn.rs` 的
`finalize_spawn_in_group` / `finalize_generated` 按 `kind.is_equine()` 分派到
`equine::finalize_spawn`、`natural.rs` 的 `check_spawn_rules` 把马 / 驴 / 骡 / 羊驼归到
`animal_spawn_rules`，没有重复的马科实现。`cargo check --workspace --all-targets` 通过；
`mob-oracle --only equine`（648 题）、`--only zombie_equine`（20 场景 15258 刻）、
`--only llama_spit`（2 场景 322 刻）复跑全部 0 不一致，确认合并没有改变马科的取数顺序。
另补了 `START_RIDING_JUMP` 服务端门槛的单元测试（上鞍 + 正刻度才扬蹄、播跳跃声）。

**与 main 合并（2026-11-21，第十轮）。** 本轮 `main` 又前进了 10 个 worldgen 缓存提交
（`c093090f` 一带：`worldgen_cache.rs` / `worldgen_compact.rs`、缓存设置与按依赖 / 空闲回收）。
`git merge main` 用 ort 自动合并了双方都改过的 `README.md`、`mc-bamboo-game/src/lib.rs`、
`mc-bamboo-game/src/world.rs`，没有冲突；`cargo check --workspace --all-targets` 通过。
`mob-oracle --only equine`（648 题）、`--only zombie_equine`（20 场景 15258 刻）、
`--only llama_spit`（2 场景 322 刻）复跑全部 0 不一致，确认缓存改动没有碰马科的取数顺序。
本轮把上一轮未落盘的 `START_RIDING_JUMP` 单元测试提交，并逐行核对了官方 26.3 的跳跃链：
`ServerGamePacketListenerImpl.handlePlayerCommand` 的 `START_RIDING_JUMP` 分支只调
`PlayerRideableJumping.handleStartJump`（即 `AbstractHorse.handleStartJump`：扬蹄 + 跳跃声，不用刻度值）；
真正的冲量 `AbstractHorse.tickRidden` → `executeRidersJump` 受 `isLocalInstanceAuthoritative()` 约束，
而 `Player.isClientAuthoritative()` 恒真、载具的 `isClientAuthoritative()` 随控制乘客为真，
故服务端该值为假、冲量只在客户端算。Bamboo 服务端只做扬蹄和跳跃声、位置由 `move_vehicle` 提供，
与官方一致；`playerJumpPendingScale` / `allowStandSliding` / `getRiddenInput` 是客户端权威的
AI 内部量，服务端不建模，无玩家可感知差异。没有改动任何马科行为代码。

**与 main 合并（2026-11-21，第十一轮）。** 本轮 `main` 又前进了 19 个提交
（`ceb0562f` 一带：苦力怕、蜘蛛、洞穴蜘蛛、末影人）。接手时分支上有一个已解决冲突、
但未 `git add` / 未提交的在途合并；先逐文件核对 7 处冲突（`README.md`、`goals.rs`、
`interact.rs`、`mod.rs`、`motion.rs`、`MobOracle.java`、`mob_oracle.rs`）都是按 main 结构的
真并集：马科的 `playStepSound`（保留 `view` / `pos` 形参）、`RunAroundLikeCrazyGoal`、
`LlamaAttackWolfGoal`、`notRiding` 乘客判断、载具 `passengers` / `start_riding_jump`、
`MobInteraction` 的 `ride` 与 `damage_held`、骑乘 / 吐口水对拍场景，和 main 的 `Swell` /
`Enderman*` / `Spider*` 目标、苦力怕 `after_fall`、蜘蛛网不减速、`uses_block_step_sound`
方块脚步声、敌对生物的场景与存档字段全部共存，没有丢掉任一边。`spawn.rs` 的
`finalize_spawn_in_group` / `finalize_generated` 按 kind 分派、`natural.rs` 的
`check_spawn_rules` 按 kind 分支本就自动合并好。`generated/` 用合并后的生成器重新跑
`extract`（不手合），34 个文件逐字节可复现；`game_event` 注册表因此从旧的字典序换成官方
`protocol_id` 顺序（`entity_action` 从 15 变 21、`resonate_*` 移到末尾），但全仓只有
`archive/passive.rs` 的 `by_name` 按名字判存在、丢弃数字 ID，无任何数字 ID 消费方，行为不变。
补上 `PlayerView` 新增的 `look_direction` / `gaze_disguise` / `yaw` / `pitch` 到马科和蜘蛛的
测试夹具。`cargo check --workspace --all-targets` 通过；`mob-oracle --only equine`（648 题）、
`--only zombie_equine`（20 场景 15258 刻）、`--only llama_spit`（2 场景 322 刻）复跑全部
0 不一致，确认合并没动马科的取数次数和顺序。本轮没有改动任何马科行为代码；剩下的差距仍
是要动线程模型 / tick 顺序或真实客户端才能收口的部分（见汇报），按规矩停下来不自己定。

**与 main 合并与马科收口（2026-11-21，第十二轮）。** 本轮先合入 `main` 新增的 19 个提交
（`e1d69b66` 一带：哞菇、北极熊、熊猫），按 main 结构解 5 处冲突（`goals.rs` 统一
`TargetType::follow_scale` 同时支持北极熊 `0.5` 与羊驼 `0.25`、`interact.rs` 并集保留马科与熊猫食物、
`MobOracle.java` / `mob_oracle.rs` 并集保留马科与熊猫夹具、`README.md` 并集保留两方对拍说明），
补修 `main` 合入的 `Extractor.java` `stewFlowers()` 缺失右花括号后重跑 `extract` 确认 `generated/`
逐字节一致，更新马科测试夹具适配 `running_names(mob_type)` 与 `MobPublic::of(&mob)`。合并后补齐
三处对照官方 26.3 查出的马科行为缺口：① `Entity.interact` → `Mob.attemptToShearEquipment`
（剪刀右键按 `EquipmentSlot.VALUES` 顺序卸马铠 / 羊驼地毯 / 鞍，按 `PASSENGER` 附着点均值掉落、
播 `shearing_sound`、扣耐久并检查绑定诅咒），`equineInteract` 加 `Items.SHEARS` 扩到 504 题；
② `AbstractHorse.onClimbable()` 恒假（马科不再爬梯子 / 藤蔓）；③ `DismountHelper.canDismountTo`
补世界边界检查（`border.is_within_bounds_aabb`）。另给 `LlamaAttackWolfGoal` 与 `WolfAvoidEntityGoal`
补上两场非空跑逐刻对拍（`zombie_equine_llama_wolf_near` / `_far`）。核实 `LivingEntity.rideTick`
确实调 `resetFallDistance()`，原疑点 15 消解。

其余已知差异见 README「马科的已知限制」。

**与 main 合并（2026-11-21，14:35 追加，op-water）。** `main`（`c9773f61`：马科骑乘 / 农场动物
收尾、哞菇 / 北极熊 / 熊猫、苦力怕组、缓存与计时测试）合进分支，唯一冲突是 `README.md`：按 main
结构保留新版羊 / 鸡条目，保留 op-water 的鱿鱼限制句与「海豚 / 美西螈」条目，删掉块内重复的弓条目，
自然生成夹具说明取并集、数字改成 330 例 1320 刻。`generated/` 重跑 `extract`：`entity_types.rs`
由生成器重排并去掉分支手写的 6 行注释（常量值不变），`registries.rs` 逐字节一致；`mob-oracle`
场景编号两边无新增重复。复核：`cargo check --workspace --all-targets` 通过、全量 `mob-oracle`
1740 例 163321 刻 0 不一致、`--only natural_spawn` 330 例 1320 刻、`natural-spawn` 1727 例、
`island_command` 4 passed、`--break-self-test water_mob` 抓到 12 处不一致。海豚 / 美西螈逐刻场景、
故意改坏确认与自然生成接入全部复核通过；Brain 相关行为与海龟产卵 / 孵化 / 回家场景仍未做，
见 README 已知限制。本轮未改动任何生物行为代码。

**海龟产卵 / 回家逐刻场景（2026-11-21，第七轮）。** 见文末「海龟的实施现状」：新增四个
逐刻场景、开局带蛋 / 家方块钩子、逐刻状态比较和覆盖断言，故意改坏确认有效；顺带修掉
`Command::LayTurtleEgg` 的越界崩溃。合并状态：`main = c9773f61` 仍是 HEAD 的祖先，
`git merge main` 报 Already up to date。全量 `mob-oracle` 1744 例 165621 刻 0 不一致。

**与 main 合并（2026-11-21，14:35 追加，第八轮，op-water）。** 本轮 `main` 已前进到
`650625b5`（Brain AI 框架、犰狳、山羊），比分支多 20 个提交，按「每轮先 merge main」执行。
三处冲突都按 main 结构解并集：① `entity/mob/mod.rs` 保留两边的 `axolotl` / `brain` 模块声明，
`MobType::ALL` 收下 Dolphin / Axolotl / Armadillo / Goat 四项、长度由 30 改成 32（合并后枚举
共 32 个变体）；② 本文件按 main 把 5.9 定为「大脑 AI、犰狳、山羊」、5.10 定为「中立生物地基和
北极熊」，去掉分支重复的 5.9 标题，保留两段 op-water 记录；③ `MobOracle.java` 的
`cowSetupJson` 同时 append 海龟的 `turtleEgg` / `turtleHome` 和大脑的 `sensors` / `scute`，
`zombieStepJson` 同时保留海龟与山羊两个 `instanceof` 状态块（补回各自闭合括号）。
`generated/` 用合并后的生成器重跑 `extract`，`entity_types.rs` 与自动合并结果逐字节一致。
`mob-oracle` 场景编号两边不重复：水生生物走 `squid` / `fish` / `water_mob` 独立驱动，不与
cow-kind 表里犰狳 13 / 山羊 14 / 海龟 60 撞号；Rust `CowSetup` / `CowStep` 两侧字段自动合并齐全。
合并把工作区检出成 CRLF、而仓库是 LF，逐文件转回 LF 后 diff 只剩真实内容改动。
复核：`cargo check --workspace --all-targets`、`cargo fmt --check`、
`cargo clippy --workspace --all-targets -- -D warnings` 全通过；全量 `mob-oracle`
1757 例 170741 刻 0 不一致（brain 新增 13 场景 5120 刻正好对上），`--only dolphin`
3 场景 603 刻、`--only axolotl` 4 场景 804 刻、`--break-self-test water_mob` 抓到 12 处不一致，
`natural-spawn` 1727 例 0 不一致；`natural.rs` / `spawn.rs` 的水生生成规则接入经核对未被覆盖。
`island_command` 4 passed、`mc-bamboo-blocks` 779 passed、`mc-bamboo-vanilla` 101 + 8 passed、
`cargo check -p mc-bamboo-desktop` 通过、联机探针 769/769。05:00 两处回归（sheep_eat 位置、
岛 1 随机源）的修复在 `9f210652` 已在 main，本轮全量对拍与 `island_command` 复核未复发。
本轮未改动任何生物行为代码，只做合并、行尾规范化和文档。

**与 main 合并（2026-11-21，第九轮，op-water）。** 起始 `HEAD = 6f85fff8`（第八轮），
`main = 54ee195a`：比合并基点 `650625b5` 多 8 个提交（`fceb0a89` 一带，op-wgmem 的
「一份共享 Worldgen 数据集」，改 `worldgen` / `climate` / 生成线程与文档），与
`mc-bamboo-blocks` 的水生生物代码不重叠。按「每轮先 merge main」执行，`git merge main`
自动合并、无冲突（只 `README.md` 自动并入 main 的 9 行条目，水生条目原样保留）。`generated/`
用合并后的生成器重跑 `extract`（JDK 26），`git status` 干净、逐字节幂等；`mob-oracle`
场景编号两边不重复（cow-kind 表 1..=17 / 40..=43 / 60，水生走 squid / fish / water_mob
独立驱动）。复核全通过：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、
`cargo check --workspace --all-targets`、全量 `mob-oracle` 1757 例 170741 刻 0 不一致、
`--only dolphin` 3 场景 603 刻、`--only axolotl` 4 场景 804 刻、`--only sheep_eat` 3 场景 900 刻、
`--only natural_spawn` 342 例、`natural-spawn` 1727 例、`--break-self-test water_mob` 抓到 12 处、
`island_command` 4 passed、`mc-bamboo-blocks` 779 passed、`mc-bamboo-vanilla` 101 + 8 passed、
`cargo check -p mc-bamboo-desktop`、联机探针 769/769。05:00 两处回归（sheep_eat 位置、岛 1
随机源，修复在 `9f210652`）复核未复发；09:30 追加的海豚 / 美西螈逐刻对拍、故意改坏确认与
自然生成接入全部复核仍 0 不一致。本轮未改动任何生物行为代码，只做合并与文档；Brain 驱动的
剩余已知限制（引路 / 恩惠 / 装死回血 / 攻击水生怪物 / 繁殖）需动 `living.rs` 的 tick 顺序与
目标分派，按「超出已定设计、需改线程模型或 tick 顺序的部分停下来写汇报，不要自己定」留给
队长定夺，见 README 已知限制与本轮汇报。

### 5.9 大脑 AI、犰狳、山羊（2026-11-21，分支 `brain`）

**已做**（代码在 `entity/mob/brain/`、`entity/mob/armadillo.rs`、`entity/mob/goat.rs`）：

| 项 | 官方 | Bamboo | 对拍 |
|---|---|---|---|
| 记忆 | `Brain.memories`、`ExpirableValue`、`MemorySlot` | `brain/memory.rs`：每种一个槽，过期倒数，没登记读作没有 | 逐刻比全部有值的记忆 |
| 传感器 | `Sensor.tick`（`scanRate`、`randomlyDelayStart`）、`NearestLivingEntitySensor`、`PlayerSensor`、`NearestItemSensor`、`AdultSensor`、`HurtBySensor`、`TemptingSensor`、`MobSensor` | `brain/sensor.rs` | 起始延迟取官方报的值 |
| 活动 | `ActivityData`、`setActiveActivityToFirstValid`、`activityRequirements` / `activityMemoriesToEraseWhenStopped` | `brain/mod.rs` | 逐刻比活动集合 |
| 行为 | `Behavior`、`OneShot`（`BehaviorBuilder`）、`DoNothing`、`GateBehavior` / `RunOne` | `brain/behavior.rs` + `behaviors.rs` 的 `Kind`，生物专属的挂在 `Kind::Armadillo` / `Kind::Goat` | 逐刻比正在跑的行为（闸门带子行为） |
| 存档 | `Brain.Packed` 的 `memories`、`Goat` / `Armadillo` 的 `add/readAdditionalSaveData` | `brain/nbt.rs`（只存有编解码器的记忆）、`mob/nbt.rs` | 单元测试往返；`chunk-content` 山羊 / 犰狳重存三题、新造山羊一题 |
| 犰狳 | `Armadillo`、`ArmadilloAi` | `armadillo.rs` | `armadillo_*` 6 场景 |
| 山羊 | `Goat`、`GoatAi`、`LongJumpMidJump`、`LongJumpToRandomPos`、`LongJumpUtil`、`PrepareRamNearestTarget`、`RamTarget` | `goat.rs` | `goat_*` 7 场景 |

大脑放在 `Mob.brain: Option<Box<Brain>>`，跑的时候整个取出来（`Cx` 里同时拿着实体、生物、
上下文和记忆），跑完放回。一刻里的位置照官方 `customServerAiStep`：先 `brain.tick`，再
换活动，然后 `Animal` 那层。行为的遍历顺序见 `brain/mod.rs` 的 `order_entries`。

**随机源。** 行为时长、`SetEntityLookTargetSometimes` 的间隔、长跳的候选抽签和冷却、冲撞冷却、
山羊吃东西的音高用世界随机源；漫步、环顾、窥视、掉角用生物自己的随机源；官方不带种子的
（`ShufflingList`、`Collections.shuffle`、`Sensor` 的静态随机源）在 Bamboo 里由出生时的随机源
状态派生，对拍时两边按 `shuffle_seed` 重设。山羊的 `LongJumpToRandomPos` 在官方一侧换成只改
了洗牌随机源的子类。

**对拍夹具。** `goat_horn` 把所有传感器的起始延迟定成 3，场景每次跑都一样，才能在冲锋途中
打死小牛、让山羊撞墙掉角；这个场景只跑 100 刻，因为官方一侧不 tick 掉落物（角停在原地），
Bamboo 的会飞出去，之后山羊看不看得见它就不一样了。繁殖出的幼崽不在比对范围里，山羊繁殖
没有逐刻对拍。

**并行模式。** `level_random_int` 在并行视图里恒为 0（行为时长取最短、长跳按列表顺序抽落点），
冲撞命中走 `Command::RamHit`、掉角走 `Command::DropItemWithDelta`，都在落地阶段按官方顺序
执行；大脑跟着实体走，岛拆合不需要新的按实体迁移。

其余已知差异见 README「大脑生物的已知限制」。

### 5.10 青蛙和蝌蚪（2026-11-21，分支 `brain`）

**已做**（代码在 `entity/mob/frog.rs`、`entity/mob/tadpole.rs`、`behaviour/misc/frogspawn.rs`、
`mc-bamboo-core/src/jmath.rs`）：

| 部分 | 官方 | Bamboo | 验证 |
|---|---|---|---|
| 青蛙大脑 | `Frog`、`FrogAi`、`Croak`、`TryFindLand`、`TryFindLandNearLiquid`、`TryLaySpawnOnFluidNearLand`、`StartAttacking`、`StopAttackingIfTargetInvalid`、`ShootTongue`、`RandomStroll.swim` | `frog::Behavior`（挂在 `Kind::Frog`） | `frog_*` 16 场景 |
| 长跳 | `LongJumpToRandomPos`、`LongJumpToPreferredBlock`、`LongJumpMidJump`、`LongJumpUtil` | `goat::JumpSpec` + `goat::LongJump`（山羊和青蛙共用） | `frog_lily_*`、`frog_long_jump`、`goat_long_jump` |
| 传感器 | `FrogAttackablesSensor`、`IsInWaterSensor`、`TemptingSensor(FROG_TEMPTATIONS)` | `SensorKind::FrogAttackables` / `IsInWater` / `FrogTemptations` | 同上 |
| 移动和寻路 | `SmoothSwimmingMoveControl`、`SmoothSwimmingLookControl`、`Frog.FrogPathNavigation` / `FrogNodeEvaluator`、`Frog.travelInWater` | `frog::move_control_tick`、`tadpole::look_control_tick`、`EvaluatorFlags.frog` | `frog_pool_*`、`frog_deep_*`、`tadpole_swim_*` |
| 蝌蚪 | `Tadpole`、`TadpoleAi`、`AbstractFish` 的三个目标 | `tadpole.rs` + `fish.rs`（`is_fish` 含蝌蚪） | `tadpole_*` 8 场景 |
| 蛙卵 | `FrogspawnBlock.tick` / `spawnTadpoles` | `behaviour/misc/frogspawn.rs` | 单元测试 |
| 存档和同步 | `VariantUtils.writeVariant`、`Tadpole.add/readAdditionalSaveData`、`DATA_VARIANT_ID` / `DATA_TONGUE_TARGET_ID` | `mob/nbt.rs`、`entity_tracking.rs`（下标和序列化器由生成器导出） | `chunk-content` 4 个重存用例 |

**几处照官方的怪行为。**
- `AmphibiousNodeEvaluator.prepare` 把生物的水代价设成 0，`done` 只还原 `WALKABLE` 和
  `WATER_BORDER`：青蛙构造时设的水代价 4 在第一次寻路之后就一直是 0。Bamboo 在
  `Navigation.cleared_water_malus` 记这件事，`Mob::pathfinding_malus` 读它
  （随机落点的 `GoalUtils.hasMalus`、长跳落点判定都走这里）。
- 蝌蚪没有覆盖 `AbstractFish.registerGoals`，目标选择器和大脑同时在跑；拿着粘液球的玩家
  一边被大脑跟随、一边被 `AvoidEntityGoal` 躲开。
- 青蛙 `canBeABaby` 为假，但 `AgeableMob.finalizeSpawn` 照样可能 `setAge(-24000)`：
  `Mob::is_baby` 对青蛙恒假，年龄不涨也不存。
- `ShootTongue.eatEntity` 打死猎物后当场 `remove(KILLED)`：猎物只标记丢弃，轮到它时
  tick 循环把它拿掉、这一刻攒下的事件（死亡动画）照样发出去；同一刻青蛙的
  `getTongueTarget()` 已经是空的，俯仰归零。

**随机源。** 长跳抽落点、行为时长、产卵后蛙卵的孵化延迟、品种和长跳冷却用世界随机源；
`LongJumpToPreferredBlock.start` 的「这次挑不挑偏好方块」、漫步落点、喂蝌蚪的粒子位置
用生物自己的随机源；角度洗牌和闸门洗牌按 `shuffle_seed` 派生（和山羊同一套序号）。

**对拍夹具的三处改动。** 假世界的 `getEntity(int)` 不再返回已移除的实体（真实的
`ServerLevel` 在 `setRemoved` 的回调里就把它从查找表里拿掉）；假世界的 `setBlock` 不跑
`onPlace`，蛙卵的孵化延迟在那里补抽一次；大脑快照里跟踪已移除实体的记忆记成 `?`。

**没做**：骆驼（`Camel` / `CamelAi`，见汇报里的排期）；岩浆怪和蛙明灯掉落；蛙卵被下落的
方块砸碎。
### 5.10 中立生物地基和北极熊（2026-11-21）

- **`NeutralMob`**（`entity/mob/neutral.rs`，新文件）：`persistentAngerEndTime` /
  `persistentAngerTarget` 放在 `Mob::anger`，`update` 是官方的
  `updatePersistentAnger(level, stayAngryIfTargetPresent)`，`write_nbt` / `read_nbt` 是
  `add/readPersistentAngerSaveData`（`anger_end_time` 恒写，`angry_at` 有才写）。
  狼、蜜蜂、末影人、僵尸猪灵可以直接复用；`universalAnger` 规则没有建模（恒为假）。
- **`HurtByTargetGoal.alertOthers`**：新增 `Command::AlertMobs`，落地时逐个看
  `other.getTarget() == null` 再 `setTarget`。`alertOther` 的过滤（只有成年北极熊）
  在发起方做。铁傀儡、僵尸猪灵、熊猫也能用。
- **`TargetGoal` 的 `mustSee` 记忆**：`canContinueToUse` 里按
  `unseenTicks <= reducedTickDelay(memory)` 判定，记忆 300 刻；`PolarBearAttackPlayersGoal`
  的跟随距离折半（`getFollowDistance() * 0.5`）。
- **共享修正（影响所有生物，对拍全部重跑）**：
  1. 动物（非怪物）的元数据原来不写 `Mob.DATA_MOB_FLAGS_ID` 的攻击性位，
     所以 `MeleeAttackGoal.start` 的 `setAggressive(true)` 不会同步。现在两种分支都写。
  2. 着火显示标志原来由 `remainingFireTicks > 0` 直接推出，点燃当刻就同步；
     官方在 `baseTick` 末尾才 `setSharedFlagOnFire`，晚一刻。现在 `Mob.visible_fire`
     在 `baseTick` 末尾重算，`convertCommon` 照官方抄这个标志。
- **状态**：北极熊已对拍 0 不一致（`mob-oracle --only polar_bear` 11 例 1121 刻，含成年 / 幼崽选狐两场景）。
  `SpawnPlacementTypes.ON_GROUND` 的落脚块检查传入实体类型：冰和霜冰按官方 `Blocks`
  的专用谓词只允许北极熊，之后仍检查 `PolarBear.checkPolarBearSpawnRules`。
  自然生成与生成期刷怪的对拍分别覆盖蘑菇岛菌丝上的哞菇、冰冻海洋冰面上的北极熊。
  山羊 / 青蛙 / 犰狳 / 骆驼用官方 `Brain`（`GoatAi` / `FrogAi` / `ArmadilloAi` / `CamelAi`）；
  犰狳、山羊的 `Brain` 已在 5.9 落地，青蛙、骆驼仍等这套地基。

### 5.11 熊猫（2026-11-21，分支 `op-misc`）

- 代码在 `entity/mob/panda.rs`。六个基因按官方显性 / 隐性规则合成七种外观，显性基因直接
  是外观，`Brown` / `Weak` 要 `main == hidden` 才显现；`MainGene` / `HiddenGene` 存盘，
  六个元数据（三个计数器、两个基因、一个旗帜字节）同步，index 由生成器导出。
- 目标表照 `Panda.registerGoals` 一比一注册，包括 `PandaPanicGoal`、`PandaBreedGoal`、
  `PandaAttackGoal`、`PandaAvoidGoal`、`PandaSitGoal`、`PandaLieOnBackGoal`、
  `PandaSneezeGoal`、`PandaLookAtPlayerGoal`、`PandaRollGoal`、`PandaHurtByTargetGoal`。
  对拍的 running names 按种类取官方子类名（`Goal::name_for`）。
- 每刻在 `super.tick()` 之后处理：担心基因在雷雨坐下、不乐意计数器（29 / 14 刻叫）、
  喷嚏计数和 `afterSneeze`、打滚、坐着清零俯仰、`handleEating` 的 80 / 100 刻和粒子取数。
- 交互直接复刻 `Panda.mobInteract`：害怕时 PASS、仰躺时起来、竹子让成年求爱 / 幼年长大 /
  坐下吃（换竹子丢旧的）、吃的东西进主手。捡物在 `Mob.aiStep` 的 looting 位置，只捡
  `panda_eats_from_ground` 且主手为空时，走 `Command::PickUpItem`。
- 繁殖的基因遗传在 `getBreedOffspring`：父母各一次 `nextBoolean`（`getOneOfGenesRandomly`
  在父母自己的随机源上）、幼崽一次 `nextBoolean` 和两次 1/32 变异。父亲在命令落地时不在
  实体表里，所以它的基因随 `Command::BreedAnimals` 带过来。
- 状态：`mob-oracle --only panda` 6 个场景 3278 刻 + 512 题基因遗传 0 不一致；
  生成期刷怪把熊猫接进 `natural_gen` 的放置表，运行时接进 `check_spawn_rules` 和
  `finalizeSpawn`（组数据 `AgeableMobGroupData(0.2)`）。
- 已知差异：`ITEM` / `SNEEZE` 粒子不发（取数照官方）；喷嚏让同伴跳的水中判定没有建模；
  幼崽金蒲公英的 `Animal.mobInteract` 兜底未接。青蛙、骆驼仍等 `Brain`（犰狳、山羊见 5.9）。

### 5.11 08:40 合并回归的根因与端到端复现（op-misc，2026-11-21）

- 队长 08:40 看到的两处快速检查失败，在 `op-misc` 的最终提交上均已修复并验证：
  `MobOracle.java` 的 `caseVariant(Mob) throws Exception`（被调的 `farmVariant(Animal)` 声明了
  `throws Exception`，单文件源码编译需要这一行）；`goals.rs` 的 `NearestAttackableTargetGoal`
  对 `TargetType` 显式穷举，`Fox` 只搜狐狸实体，不再经 `_ if kind.available()` 兜底。
- 根因（第 5 轮静态分析，本轮端到端复现确认）：旧代码 `available()` 把 `Fox` 也算作已实现，
  北极熊的 Fox 目标因此落进 `nearest_attackable` 的玩家搜索。`polar_bear_idle` 出生点附近有玩家，
  熊从第 1 刻起就有了目标、愤怒结束时间 `e1778`，并多取一次随机数，世界随机源随之分叉。
  故意把 `Fox` 分支改回 `nearest_attackable` 后，`mob-oracle --only polar_bear` 精确复现
  `tick 1 ... official "s0|w0|e0|t-" bamboo "s0|w0|e1778|t...0002"`，恢复后 0 不一致。
- 现状：`git merge main` 为 Already up to date（`main` 已是 `op-misc` 的祖先，合进 main 是快进）；
  全套 `mob-oracle` 1677 例 160485 刻 0 不一致、`natural-spawn` 1727 例 0 不一致。
  下次合并冲突解决请保留上面两处（显式 `Fox` 分支、`throws Exception`）。
- 其余生物：山羊 / 青蛙 / 犰狳 / 骆驼用 `Brain`（`GoatAi` / `FrogAi` / `ArmadilloAi` /
  `CamelAi`），骆驼 / 鹦鹉上肩还要载具与乘客（世界模型 8.2 未定），按「建议顺序」由队长先整合
  `brain` 分支、定案载具后再接；猫 / 鹦鹉 / 豹猫用 `tame.rs`，各一个新文件、共用文件最小追加、
  各配逐刻对拍场景。本轮不新增 `MobType`，避免刚被撤回的合并再次扩大共用文件冲突面。

### 5.12 剩余生物的状态与接入规格（op-misc，2026-11-21，第 8 轮）

**本轮复核**：按规矩先 `git merge main`，当时返回 `Already up to date`（`main=2448b76c`）。
收尾时 `main` 又前进到 `826bf524`（cx-farm 的猪羊鸡 / thrown-eggs 工作），按 04:30 追加再次
`git merge main`：无冲突，`goals.rs` 未被 main 改动，08:40 的显式 Fox 修复完好；`MobOracle.java`
与本文档为自动合并。08:40 两处修复逐字核对仍在：`MobOracle.java` 的
`caseVariant(Mob mob) throws Exception`、`goals.rs` 的显式 `TargetType::Fox` 分支
（`available()` 已从本分支彻底移除）；5.11 节在。全套检查在最终合并提交上重跑、全部通过
（数字见本轮汇报）。**本轮不新增生物、不新增 `MobType`**，理由见下。下面行号均以当前提交
为准，合并后可能漂移，定位以 `fn` 名 / 符号名为准。

**剩余 7 种的状态**（任务清单：山羊、青蛙、犰狳、骆驼、鹦鹉、豹猫、猫）：

| 生物 | 状态 | 前置 |
|---|---|---|
| 哞菇 | 已对拍（`e1d69b66`，剪成牛 / 炖蘑菇 / 闪电变色） | — |
| 北极熊 | 已对拍（`08e297cf` 修 Fox 目标后 11 例 1121 刻 0 不一致） | — |
| 熊猫 | 已对拍（6 场景 3278 刻 + 512 题基因遗传 0 不一致） | — |
| 山羊 / 青蛙 / 犰狳 / 骆驼 | 没做 | 官方 `Brain`（`GoatAi`/`FrogAi`/`ArmadilloAi`/`CamelAi`），本仓库只有 `GoalSelector`（5.9）；骆驼另需载具 |
| 鹦鹉 | 没做（仅生成期放置模拟 `natural_gen.rs`） | 上肩需载具 / 乘客（世界模型 8.2 未定） |
| 豹猫 / 猫 | 没做 | 无硬前置（`registerGoals()`，非 `Brain`），但接入面见下 |

**5 种被前置挡住**：5.9 / 5.10 / 5.11 已确认 `Brain` 是「下一步的公共地基」、`brain` 分支未合入
main；世界模型 8.2「载具和它的乘客」未定案，牵涉岛拆分与一刻顺序。按「超出设计文档、又必须
改动线程模型或 tick 顺序的部分停下来写汇报，不要自己定」，不自行引入 `Brain` / 载具。

**豹猫 / 猫本轮也不做**：二者技术上可行（官方 `Ocelot` / `Cat` 都用 `registerGoals()`，非
`Brain`），但新增 `MobType` 会一次性改到一批和 `cx-wild` / `cx-horse` / `op-water` 等并行队员
高度共用的文件（下表），且 `by_entity_type`（`mod.rs:719`，遍历 `ALL`）把官方刷怪表的
`minecraft:ocelot` 条目（`registries.rs:585` 已有 ID 92）映射成新种类后，会直接碰到「必须
0 不一致」的关键路径 `natural-spawn` 与 `check_spawn_rules`。本分支刚被撤回过一次合并，
单轮无人值守（一旦开工无法收敛必留半成品、违背「检查要干净」）不宜扩大共用文件冲突面；
5.11 的「建议顺序」也把它们排在 `Brain` / 载具整合之后。这是判断取舍、非硬阻塞。

**更正 5.11 一处**：豹猫 `Ocelot extends Animal`，**不是** `TamableAnimal`，用
`DATA_TRUSTING` 信任位（无主人 / 跟随 / 坐下）；只有猫 / 鹦鹉是 `TamableAnimal`、才复用
`tame.rs`。豹猫新文件应自带 `trusting` 状态，不动 `tame.rs:77` 的 `is_tamable`。

**豹猫（`Ocelot`）接入规格**（官方 `net.minecraft.world.entity.animal.feline.Ocelot.java`）：

- 属性 `createAttributes()`：`MAX_HEALTH` 10.0、`MOVEMENT_SPEED` 0.3F、`ATTACK_DAMAGE` 3.0。
  成年尺寸取 `entity_types.rs:114`（0.6×0.7）；幼崽 `BABY_DIMENSIONS` 0.3×0.35、眼高 0.34375。
  ambient 间隔 900。**注意**：`entity_types.rs` 目前只有 `RABBIT_MAX_HEALTH`（`:204`）/
  `FOX_MAX_HEALTH`（`:215`）等，没有 `OCELOT_MAX_HEALTH` / `OCELOT_MOVEMENT_SPEED` /
  `OCELOT_ATTACK_DAMAGE`；按「版本数字由生成器导出，不要手写」，做豹猫要先动生成器 /
  `Extractor.java` 的实体属性导出（共用文件禁区），须先和队长确认。
- `registerGoals()` 优先级（照官方）：goal 1 `FloatGoal`、3 `OcelotTemptGoal(0.6,
  OCELOT_FOOD, canScare)`、7 `LeapAtTargetGoal(0.3F)`、8 `OcelotAttackGoal`、9
  `BreedGoal(0.8)`、10 `WaterAvoidingRandomStrollGoal(0.8, 1.0000001E-5F)`、11
  `LookAtPlayerGoal(Player, 10)`；target 1 `NearestAttackableTargetGoal<Chicken>`、
  1 `NearestAttackableTargetGoal<Turtle, BABY_ON_LAND_SELECTOR>`；`reassessTrustingGoals`
  在 `!trusting` 时加 goal 4 `OcelotAvoidEntityGoal<Player>(16, 0.8, 1.33)`。
- `goals.rs` 现状：`Goal::Float/Tempt/LeapAtTarget/Breed/Stroll/LookAtPlayer/Avoid/
  MeleeAttack/NearestAttackableTarget`（`:33` 起）齐备；`TargetType`（`:293`）已有 `Turtle`、
  **缺 `Chicken`**，`Turtle` 的 `BABY_ON_LAND_SELECTOR`（只选陆地幼年海龟）要特判；
  `OcelotTemptGoal.canScare` / `OcelotAvoidEntityGoal` / `OcelotAttackGoal` 的 `trusting`
  闸门与官方子类名（`Goal::name`）要按种类补。
- 每刻 `customServerAiStep`：按 moveControl speed `0.6`→`CROUCHING`、`1.33`→
  `STANDING`+疾跑、其余 `STANDING`；接 `living.rs` 每刻分派。
  `removeWhenFarAway`：`!trusting && tickCount > 2400`。
- 交互 `mobInteract`：tempt 未跑或在跑、`!trusting`、`OCELOT_FOOD`（`tags.rs:461` 已有）、
  `distanceToSqr<9` → `usePlayerItem` 后 `nextInt(3)==0` 才 `setTrusting(true)` + 心形粒子 +
  event 41，否则烟粒子 + event 40；`spawnTrustingParticles` 取 7 组
  `nextGaussian()*0.02`（世界随机源取数须对齐）。接 `interact.rs`。
- 存盘：`addAdditionalSaveData` / `readAdditionalSaveData` 的 `Trusting` 布尔，接 `nbt.rs`。
- **关键路径取数**：`checkOcelotSpawnRules` 是 `random.nextInt(3) != 0`；`natural.rs:625`
  的 `check_spawn_rules` 是穷举 `match`，必须补 Ocelot 分支，否则编译不过且 `natural-spawn`
  分叉。`checkSpawnObstruction`：海平面以上、脚下 `GRASS_BLOCK` 或 `LEAVES`、无液体。
- `spawn.rs`：`:335` `finalize_spawn_in_group` 按 kind 分派，豹猫 `finalizeSpawn` 的
  `groupData == null → AgeableMobGroupData(1.0F)`；`:149` `/summon`、`:196` 刷怪蛋。
  `getBreedOffspring` 为 `OCELOT(BREEDING)`。
- `mod.rs` 需最小追加的点（下列为 `fn` 起始行，分支在其后）：`:77` 枚举、`:113`
  `ALL: [Self; 28]`→29、`:145` `entity_type()`、`:179` 官方名字、`:220` `is_food` 的 tag、
  `:246` `dimensions`、`:454` `loot_table`、`:494` `ambient_sound_interval`、
  `:526`/`:565`/`:592`/`:622` 音效、`:642` `spawn_egg`（`items.rs:1303` 的
  `OCELOT_SPAWN_EGG`）、`:688` 翻译键；`:719` `by_entity_type` 自动覆盖。
- 元数据 `DATA_TRUSTING` 的 `SynchedEntityData` index 来源（生成器是否导出、还是像
  `living.rs:1145` 熊猫那样按种类写）**待确认**；若需动生成器同样撞共用文件禁区。
- 对拍：`MobOracle.java` 与 `mob_oracle.rs` 目前都**没有** ocelot 场景，新建逐刻场景须覆盖
  信任 `nextInt(3)`、avoid-player、扑击、Chicken / Turtle 目标、pose / sprint。

**猫（`Cat`）接入规格**（官方 `net.minecraft.world.entity.animal.feline.Cat.java extends
TamableAnimal`，复用 `tame.rs` 的 `is_tamable` / `Tamable` / 四个公共目标）：比豹猫多
`DATA_VARIANT_ID`（`CatVariant` registry，26.3 为 registry holder）、`DATA_COLLAR_COLOR`、
`DATA_SOUND_VARIANT_ID`（`Cat.java:77-84`）；`finalizeSpawn` 用
`VariantUtils.selectVariantToSpawn` + `CatSoundVariants.pickRandomSoundVariant` 抽品种
（`Cat.java:398-406`）；驯服 `nextInt(3)`、`mobInteract` 喂食回血（`:408` 起）、怕苦力怕、
晨间礼物、蹭人。村庄猫按任务「可先不做」。品种接 `mc-bamboo-data` 的 registry holder 面更大
（`component_registries.rs:1521` 有 `CAT_VARIANT` 表、`registries.rs:776` 有 ID），故排在豹猫之后。

**下一轮建议**：按 5.11 顺序，先由队长整合 `brain` 分支、定案载具；解锁前若仍要产出，优先选
豹猫（共用面最小、非 `TamableAnimal`），严格照上表「一种一个新文件、共用文件最小追加、先小样本
守 `natural-spawn` 关键路径、各配逐刻场景」；动生成器 / 元数据 index 前先和队长确认。

### 5.13 08:40 回归修复后合并 main（op-misc，2026-11-21，第 9 轮）

- 接手时 `HEAD=cf37b3af`，`main` 已前进到 `c9773f61`（35 个 `cx-horse` 提交：马科骑乘、
  `equine/tame.rs` 的 `RunAroundLikeCrazyGoal`、`riding.rs`、狼驼目标、羊驼吐口水等），
  `main` 不是 `HEAD` 的祖先。按 08:40「最优先」先 `git merge main`。
- 三个 `changed in both` 文件里 `MobOracle.java` 与本文档自动合并，`goals.rs` 是唯一内容冲突：
  `main` 保留 `TargetType::available()`（`Fox`/`FoxPrey`/`Wolf` 算已实现）加 `_ if kind.available()`
  兜底；`op-misc` 是 `08e297cf` 删掉 `available()`、对 `TargetType` 显式穷举的版本。冲突解决保留
  `op-misc` 的显式穷举，`impl TargetType` 开头加一条中英双语警示注释防止下次合并把兜底加回。
- 自动合并另把 `main` 新增的 `TargetType::Wolf => nearest_wild_wolf_target(...)`
  （羊驼 `LlamaAttackWolfGoal`）与 `op-misc` 的 `Villager|...|Wolf|Endermite => None` 拼进同一个
  `match`，`Wolf` 出现两次（文本合并成功、语义重复）。已从 `=> None` 那支删掉 `Wolf`，保留 `main`
  的专门分支，`match` 仍穷举 11 个变体。
- 08:40 两处修复逐字核对仍在：`MobOracle.java:954` 的 `caseVariant(Mob mob) throws Exception`、
  `goals.rs` 的显式 `TargetType::Fox` 分支。全套 `mob-oracle` 在合并态 1733 例 161914 刻 0 不一致
  （含合入的 `llama_spit` / `arrow` 场景），`polar_bear_idle` 的 tick 1 回归未复现。
- 前置重新核查（基于含 `cx-horse` 的新 `main`）：`Brain`（`GoatAi`/`FrogAi`/`ArmadilloAi`/
  `CamelAi`）在 `mc-bamboo-blocks` 仍无任何实现；新合入的 `riding.rs` 是「玩家骑实体、客户端
  权威」，README 马科已知限制写明「载具 + 骑手不可分」「岛拆分」未做，世界模型 8.2 乘客归属仍未
  定案；通用 `tame.rs`（`cx-wild`）在，`equine/tame.rs` 是马科专用甩人驯服、不是通用驯服。故 5.12
  的「剩余 7 种」结论不变：4 种等 `Brain`，骆驼 / 鹦鹉上肩等 8.2，猫 / 豹猫技术上可行但共用文件
  冲突面大。**本轮不新增 `MobType`、不新增生物**，理由见本轮汇报第 4 节。
- 全套检查（fmt / clippy / `cargo check --workspace --all-targets` / `cargo test -p mc-bamboo-blocks`
  / `cargo test -p mc-bamboo-vanilla` / `cargo check -p mc-bamboo-desktop` / 全套 `mob-oracle` /
  `natural-spawn` / `tools/probe.sh`）全部通过，数字见本轮汇报第 2 节。

### 5.14 第 10 轮：合入复核与 chunk-content 北极熊存档缺陷修复（op-misc，2026-11-21）

- 按队长追加先 `git merge main`，返回 `Already up to date`：`main`（`c9773f61`）是
  `HEAD`（`3c6a9dfd`）的祖先，本轮无新冲突、无冲突解决。08:40 两处修复逐字复核仍在：
  `MobOracle.java:954` 的 `caseVariant(Mob mob) throws Exception`、`goals.rs:1534` 的
  显式 `TargetType::Fox` 分支（`available()` 仍已移除，只剩防回退的中英双语警示注释）。
- **修好 `chunk-content` 长期存在的 1 处失败（关键路径「存档格式」）**：夹具
  「polar bear with persistent anger」过去报 `official could not load this entity`
  （`1 of 495 checks failed`）。根因不是存档格式：官方
  `NeutralMob.readPersistentAngerSaveData` 读 `angry_at` 后会走
  `EntityReference.getLivingEntity` → `ServerLevel.getEntityInAnyDimension` →
  `ServerLevel.getEntities()`，而 `ChunkContentOracle.java` 的 `FakeLevel` 用 Unsafe
  分配、没初始化 `entityManager`，`NullPointerException` 被
  `EntityType.loadStaticEntity` 吞掉，`loadEntityRecursive` 返回 null。Bamboo 写的
  `angry_at`（`IntArrayTag`，4 个 int）与官方 `UUIDUtil.CODEC`（`Codec.INT_STREAM`）
  本来逐键一致；`brain` 分支疑点 20 记的「与官方 26.3 `EntityReference` 复合格式
  不符、官方侧加载不了」是误判（用不吞异常的 `EntityType.create` 打印 cause 链定位到
  `ServerLevel.getEntities` 的 `entityManager == null`）。
- 修法（只动 Java 夹具 `ChunkContentOracle.java`，21 行，提交 `3a4bcc62`）：给
  `FakeLevel` 补 `getEntityInAnyDimension` / `getPlayerInAnyDimension` 两个 override
  返回 null，表示「查不到这个实体」；`persistentAngerTarget` 保留 UUID 并原样写回
  `angry_at`，只有 `Mob.target` 为空，和官方找不到目标一致。修复后 `chunk-content`
  为 **496 checks passed**（原 1/495 失败）。Bamboo 的 `neutral.rs` / `nbt.rs` 未改，
  北极熊 `angry_at` 存档与官方逐键一致得到对拍确认。
- **前置按最新情报重新核查**：`brain` 分支（`9b846b6b`，报告 15:22）已做完山羊 / 犰狳
  和 Brain 框架、是当前 `main` 的直接后代、可无冲突 fast-forward 合进 main；青蛙仍
  整块未做，brain 报告明确「单独开一整轮按 `FrogAi` 整块搬完再对拍，不要先落半截
  实体骨架」；骆驼等 `docs/mob-entity-design.md` 5.8 骑乘定稿。故任务清单里
  山羊 / 犰狳 / 青蛙 / 骆驼都已有专门分支或硬前置，**本轮不重做、不抢做**。
- 剩 3 种：鹦鹉上肩等世界模型 8.2「载具和它的乘客」定案；豹猫 / 猫技术上可行
  （`registerGoals()`，非 Brain），但按 5.12 须先动生成器 / `Extractor.java` 导出
  `OCELOT_*` 属性与元数据 index、新建 `MobOracle.java` / `mob_oracle.rs` 逐刻场景，
  属「须先和队长确认」的共用文件禁区。队长 09:00 追加与 brain 报告的当前节奏是
  「把各分支整理干净、可 fast-forward、不要开半截 WIP 挡住合进 main」，单轮无人
  值守不宜扩大这批共用文件冲突面，故**本轮不新增 `MobType`、不新增生物**。
- 本轮全套检查（fmt / clippy / `cargo test -p mc-bamboo-blocks` /
  `cargo test -p mc-bamboo-vanilla` / `cargo check -p mc-bamboo-desktop` / 全套
  `mob-oracle` / `natural-spawn` / `chunk-content` / `tools/probe.sh`）在最终提交上
  实跑，结果见本轮汇报第 2 节。请队长定夺下一步：合进 main，或单独开一整轮做
  某一种生物。

### 5.15 第 11 轮：合并含 Brain / 山羊 / 犰狳的 main 并全套复核（op-misc，2026-11-21）

- 接手时 `HEAD=2d5ac486`、`main=c9773f61`。本轮按 04:30 / 08:40「先 `git merge main`」执行时，
  `main` 已被别的队员推进到 `650625b5`（"Merge the Brain AI framework with armadillos and
  goats"），合并**不是 no-op**：产生双亲合并 `56060028`，带入 47 文件 / 6797 行
  （`entity/mob/brain/`、`goat.rs`、`armadillo.rs`、`MobType::ALL` 28→30、`natural.rs` /
  `spawn.rs` 按 kind 分发、`MobOracle.java` +450 行、`chunk_content_oracle.rs` 等）。
  自此 `main` 是 `HEAD` 的祖先，合回 main 是无冲突 fast-forward。
- 08:40 两处修复 + 第 10 轮 chunk-content 修复在自动合并后逐字复核仍在：
  `MobOracle.java:958` 的 `caseVariant(Mob mob) throws Exception`（main 给该文件 +450 行、
  行号漂移，签名与双语警示注释都保留）、`goals.rs:1534` 的显式 `TargetType::Fox` 分支与
  `impl TargetType` 开头的防回退注释（`available()` 仍已移除）、`ChunkContentOracle.java:361,366`
  的 `FakeLevel` 两个 override。`goals.rs` 本次只把 `float_can_use` / `damage_type_tagged` /
  `look_for_water` / `nearest_tempting_player` 改成 `pub(super)` 供 Brain 复用，目标匹配未动。
- 全套检查在最终提交上实跑、全部通过（数字见本轮汇报第 2 节）：fmt / clippy `-D warnings` /
  `cargo check --workspace --all-targets` / `cargo check -p mc-bamboo-desktop` /
  `cargo test -p mc-bamboo-blocks` 775 通过 / `cargo test -p mc-bamboo-vanilla` 109 通过 /
  全套 `mob-oracle` 1746 例 167034 刻 0 不一致（含 `polar_bear_idle`，回归未复现）/
  `--only polar_bear` 11 例 1121 刻 0 / `natural-spawn` 1727 例 0 / `chunk-content` 503 项 /
  `tools/probe.sh` 769/769。
- **前置状态更新**：Brain 框架（5.9）、山羊、犰狳已随 main 落地，任务清单里这三种不再是
  「没做」；青蛙的硬前置「缺 Brain」已解除，只剩「整块一轮按 `FrogAi` 搬完再对拍」的时间约束
  （brain 报告明确不要先落半截实体骨架）；骆驼仍等 5.8 骑乘 / 世界模型 8.2 载具；
  鹦鹉上肩仍等 8.2；豹猫 / 猫仍须新增 `MobType`。
- **更正 5.12 的前置判断**：5.12 说「做豹猫须先动生成器 / `Extractor.java` 导出 `OCELOT_*`
  属性」——属性一项不准确。`panda.rs` 的 `attributes()` 把 20.0 / 0.15 / 6.0 手写在新文件里
  （照官方 `Panda.createAttributes`），`entity_types.rs` 的 `FACTS[ocelot]`（`:114`，0.6×0.7）
  已由生成器导出、可直接读；元数据 `DATA_TRUSTING` 的 index 才需要 `Extractor.java` 的
  `entityAccessor(Ocelot.class, "DATA_TRUSTING").id()` 一行（brain 已把这条导出链路用在
  `ARMADILLO_STATE` / `GOAT_*`）。故剩余硬约束不是「属性导出」，而是新增 `MobType` 碰
  `natural-spawn` / `check_spawn_rules` 关键路径、新建 Java/Rust 逐刻场景、动 `Extractor.java` /
  generated 共用文件前的确认。
- **合并带来的文档编号遗留（报告队长，不自行重编号）**：main 把 5.9 改成「大脑 AI」、
  原 5.9→5.10、原 5.10（熊猫）→5.11，但 op-misc 特有的 `5.11 08:40 根因`、`5.12`、`5.13`、
  `5.14` 未随之重排，当前文件有两个 `### 5.11`（熊猫、08:40 根因），且 5.12~5.14 里
  「5.9 / 5.10 / 5.11 已确认」「按 5.11 顺序」「更正 5.11 一处」等交叉引用在新编号下语义
  已错乱。重编号会大改这个共用文件、并与并行队员再冲突，留给队长统一处理。
- 本轮不新增 `MobType`、不新增生物：单轮无人值守的核心任务是合并与全套复核，树已可
  fast-forward，青蛙按 brain 报告须整块一轮，其余有硬前置。请队长定夺下一步：
  (A) 把合并提交 fast-forward 合进 main；(B) 单独开一整轮按 5.12 规格做豹猫或青蛙。

### 5.16 第 12 轮：再次合并 main、全套复核、查实 panda_breed 偶发不一致的根因（op-misc，2026-11-21）

- 接手时 `HEAD=f694ca4f`、`main=54ee195a`（第 11 轮结束时 main 又前进了 8 个 `op-wgmem`
  世界生成内存优化提交：`worldgen.rs` / `climate.rs` / `worldgen_spawn.rs` 等，不碰三处修复文件、
  设计文档、`natural.rs` / `spawn.rs` / `mod.rs`）。按 04:30 / 08:40「最优先」先 `git merge main`，
  产生双亲合并 `e74eefab`；`git merge-base --is-ancestor main HEAD` 为 0，自此 main 又是 HEAD
  的祖先，合回 main 是无冲突 fast-forward。合并只带进 10 个 worldgen / README / 性能文档文件，
  无内容冲突、无需按 kind 手工解决。
- 三处 08:40 / 第 10 轮修复逐字复核仍在：`MobOracle.java:958` 的
  `caseVariant(Mob mob) throws Exception`、`goals.rs:1534` 的显式 `TargetType::Fox` 分支
  （`available()` 已移除）、`ChunkContentOracle.java:361,366` 的 `FakeLevel` 两个 override。
- 全套检查在最终提交上实跑、全部通过（数字见本轮汇报第 2 节）：`cargo fmt --check` 0 /
  `cargo clippy --workspace --all-targets -- -D warnings` 0 警告 /
  `cargo check --workspace --all-targets` 0 / `cargo check -p mc-bamboo-desktop` 0 /
  `cargo test -p mc-bamboo-blocks` 775 通过 0 失败 6 忽略 /
  `cargo test -p mc-bamboo-vanilla` 101 单元 + 8 集成 = 109 通过 0 失败 /
  `cargo build -p mc-bamboo-vanilla`（worldgen 合并后重链）0 /
  全套 `mob-oracle` 1746 例 167034 刻 0 不一致 / `--only polar_bear` 11 例 1121 刻 0 /
  `natural-spawn` 1727 例 0 / `chunk-content` 503 项通过 /
  `tools/probe.sh` 769/769（`JAVA` 设成 JDK 26）。
- **查实第 11 轮疑点 1 的根因，并推翻其「并行干扰」结论**：`panda_breed` 约 6% 的偶发
  12 处不一致不是并行 java / 高负载干扰，而是官方夹具自身的随机不确定性。官方
  `Panda.getBreedOffspring`（`net.minecraft.world.entity.animal.panda.Panda`）先
  `EntityTypes.PANDA.create` 造幼崽，`Entity` 的 `random` 字段是无种子的
  `RandomSource.create()`（`RandomSource.create()` → `RandomSupport.generateUniqueSeed()`，
  即 `SEED_UNIQUIFIER.updateAndGet(...) ^ System.nanoTime()`，含墙钟），随后
  `baby.setGeneFromParents(this, partner)` 从这个幼崽自己的 `Entity.random` 抽两次
  `nextInt(32) == 0` 的变异；夹具的 `reseedBorn`（`MobOracle.java:9394`）要等到这一刻
  `collectBorn` 才把幼崽随机源换成确定性的岛种子，晚于基因抽取；`spawnJson`（`:8223`）
  对新生幼崽也不序列化基因。于是官方一侧的幼崽主 / 隐基因带约 2×(1/32)≈6% 的墙钟
  变异，而 Bamboo 侧 `panda::offspring_genes` 用确定性种子重放、几乎不触发变异，
  两边在 `var: g5/0` 与 `g0/0` 上分叉，连带元数据同步包字节数 18 vs 15。
  验证：在持有全机 heavy 锁、无并行 java 的干净负载下连跑
  `mob-oracle --only panda` 12 次，第 11 次复现出逐字相同的 `tick 60 ... g5/0`↔`g0/0`
  与 12 处不一致，其余 11 次全 0——与两次 1/32 变异、约 6% 的概率吻合；第 11 轮把它判成
  「并行重活干扰」是误判。Bamboo 的基因遗传本身是对的：独立的 `playPandaGenes`
  512 题（显式 `setRandom(baby, babySeed)` 后再 `setGeneFromParents`）逐题 0 不一致。
- **故意不改夹具、写进本轮汇报疑点 1 请队长核实**：干净修法要么在官方幼崽构造时
  （`getBreedOffspring` 之前）就把 `Entity.random` 换成确定性种子（要钩官方实体构造、
  动 `MobOracle.java` 共用夹具），要么把新生幼崽的基因像 `seed` / `uuid` / 朝向一样当成
  「场景起点」交给 Bamboo（要给 `Spawn` 加字段并改动 `panda_breed` 比对了什么，可能
  「改掉测试的本意」）。两者都是动共用文件 / 改对拍语义的取舍，无人值守单轮不宜自己定；
  且这约 6% 的抖动是官方夹具、不是玩家可感知的行为差异，写进 README 已知限制会误导，
  故只记在本节与汇报疑点 1，附最小修法建议供队长定夺。
- 第 11 轮疑点 2（probe 767/769 的 `durability: the break event is 47`，实体事件到达顺序的
  时序断言）本轮 `tools/probe.sh` 一次即 769/769，未复现；仍是高负载下的偶发时序，不改代码。
- 两个 `### 5.11` 与 5.12~5.14 的错乱交叉引用（第 11 轮疑点 3）本轮仍未自行重编号，
  留给队长统一处理（理由同 5.15）。
- 本轮不新增 `MobType`、不新增生物：剩余五种（青蛙 / 骆驼 / 鹦鹉 / 豹猫 / 猫）各有硬前置
  （Brain 整块一轮、载具 / 世界模型 8.2 未定案、新增 `MobType` 撞 `natural-spawn` 关键路径
  与 `Extractor.java` / generated 共用文件禁区「须先和队长确认」），无人值守单轮无法确认、
  也不宜在可 fast-forward 的干净树上开半截 WIP。请队长二选一：(A) 把 `e74eefab` 直接
  fast-forward 合进 main；(B) 单独开一整轮按 5.12 / brain 规格做一种（豹猫共用面最小）。

### 5.17 第 13 轮：合并 33980ce0、修复合并引入的编译中断、全套复核（op-misc，2026-11-21）

- 接手时 `HEAD=3cf0a9fa`（第 12 轮报告）、`main=33980ce0`。第 12 轮明确「会话中途不再追
  合并 33980ce0」：main 相对 `merge-base=54ee195a` 又前进了 41 个提交（op-hostile2 的女巫 /
  史莱姆 / 溺尸 / 流浪者 / 沼骸、便携 desktop 构建脚本、四次 main 合并与「full mob-oracle
  fix」）。本轮按 04:30 / 08:40「最优先」把这该 main 合进来：`2042107b` 是 ort 自动合并、
  无内容冲突、双亲；`goals.rs` / `MobOracle.java` / 本文档三个共用文件自动合并，无需按 kind
  手工解决。合并后 `git merge-base --is-ancestor main HEAD` = 0，合回 main 是 fast-forward。
- **本轮实质的代码修复：合并引入的编译中断**。main 的 op-water 工作给 `TargetType` 加了
  `Axolotl` 变体（`drowned.rs:168` 的溺尸目标占位，官方 `NearestAttackableTargetGoal<Axolotl>`，
  Bamboo 没有该实体），而 op-misc 的 08:40 修复把 `Goals::canUse` 的 `match kind` 改成显式穷举，
  两者合到一起后 `cargo build -p mc-bamboo-vanilla` 报 `E0004: &mut TargetType::Axolotl not
  covered`。修法（`a44a6c3d`，`goals.rs` 6 行、中英双语注释）：把 `Axolotl` 并进 `=> None`
  那一支，语义和 main 的 `available()` 不含 Axolotl → `_ => None` 一致——仍按间隔取
  `nextInt(interval)`、不查世界。`nearest_attackable` 只有显式
  Player / GuardedPlayer / AngerPlayer 一个调用方，op-misc 去掉 `!kind.available()` 守卫安全。
- 三处 08:40 / 第 10 轮修复在合并态逐字复核仍在：`MobOracle.java:977` 的
  `caseVariant(Mob mob) throws Exception`（被调 `farmVariant(Animal) throws Exception`，
  单文件源码编译需要这一行）、`goals.rs:1690` 的显式 `TargetType::Fox` 分支（`available()`
  仍已彻底移除、`impl TargetType` 开头的防回退双语警示注释在）、`ChunkContentOracle.java`
  的 `FakeLevel` 两个 override（`getSeaLevel` / `getPlayerInAnyDimension`）。
- **08:40 撤回合并的根因（沿用 5.11，本轮在合并态复核）**：一次冲突解决把两处修复静默回退——
  `caseVariant` 丢了 `throws Exception`（整个 `mob-oracle` 编译不过）、`Fox` 显式分支被 main
  旧的 `_ if kind.available() => nearest_attackable` 兜底取代（旧 `available()` 把 Fox 也算
  已实现，北极熊的 Fox 目标落进玩家搜索，出生点玩家被设成目标、愤怒结束时间 `e1778`、从第 1
  刻起多取一次随机数，世界随机源随之分叉，即 `polar_bear_idle` 的 12 处不一致）。当前 main
  （`e83ce946` / `6037f3d2` 一带）已自带这两处修复：`goals.rs` 的显式 Fox 分支在 `available()`
  兜底之前、`MobOracle.java` 的 `caseVariant` 带 `throws Exception`。故本轮干净的 ort 合并
  两者都在，`polar_bear_idle`（`mob-oracle --only polar_bear` 11 例 1121 刻）未复现回归。
- 全套检查在最终提交上实跑、全部通过（数字见本轮汇报第 2 节）：`cargo fmt --check` /
  `cargo clippy --workspace --all-targets -- -D warnings` / `cargo check --workspace --all-targets`
  / `cargo check -p mc-bamboo-desktop` / `cargo test -p mc-bamboo-blocks` 790 通过 0 失败
  6 忽略 / `cargo test -p mc-bamboo-vanilla` 101 单元 + 8 集成 = 109 通过 / 全套 `mob-oracle`
  1773 例 170990 刻 0 不一致 / `--only polar_bear` 11 例 1121 刻 0 / `natural-spawn`
  1727 例 0 / `chunk-content` 503 项通过 / `tools/probe.sh` 769/769（`JAVA` 设成 JDK 26）。
- 本轮不新增 `MobType`、不新增生物：核心任务是「合并当前 main、修复合并引入的编译中断、
  全套复核、保持可 fast-forward 的干净树」，剩余五种的前置沿用 5.16；两个 `### 5.11` 与
  5.12~5.14 的错乱交叉引用仍未自行重编号，留给队长统一处理（理由同 5.15 / 5.16）。

### 5.18 第 14 轮：独立复核 08:40 撤回合并的修复、新增 MobType 注册表守卫（op-misc，2026-11-21）

- 接手时 `HEAD=c188de3a`（第 13 轮报告）、`main=33980ce0`。本轮开头复核：`main` 仍是
  `HEAD` 的祖先（`git merge-base --is-ancestor` = 0、`HEAD..main` 为空），`git merge main`
  返回 `Already up to date`（本轮 main 未再前进）。按 04:30 / 08:40「先 `git merge main`」
  执行、无需合并提交；`git status --short` 干净，合回 main 仍是 fast-forward。
- **08:40 两处修复本轮逐字复核、仍在本分支**（未重做，均为历史提交）：
  `MobOracle.java:977` 的 `caseVariant(Mob mob) throws Exception`（被调
  `farmVariant(Animal) throws Exception`，单文件源码编译要求这一行）、`goals.rs:1690` 的显式
  `TargetType::Fox` 分支（北极熊 Fox 目标只搜狐狸、不落回 `nearest_attackable` 玩家搜索；
  `available()` 仍已彻底移除、`impl TargetType` 开头的防回退双语警示注释在）。因 op-misc 已是
  main 的 fast-forward 后代，队长把 op-misc 合进 main 不再需要冲突解决，这两处修复不会被
  静默回退——这正是 08:40「撤回合并」的根因（5.11 / 5.17）已被消除的机制。
- **本轮实质代码改动：`mod.rs` tests 模块新增 1 个回归测试**
  `mob_type_registry_ids_round_trip`（26 行、中英双语注释）。断言 `MobType::ALL` 每种的
  `entity_type()` 两两不同、且 `by_entity_type(id)` 能反查回自己。这是给「剩余五种生物各要
  新增 `MobType`」做的前置守卫：漏进 `ALL` 或撞了已有注册表 ID 会静默把官方刷怪表条目
  映射错（或漏映射）、直接分叉 `natural-spawn` 关键路径。不改任何游戏行为、不碰生成器。
- **本轮不新增 `MobType`、不新增生物**（沿用 5.15 / 5.16 / 5.17 判断）：剩余五种
  （青蛙 / 骆驼 / 鹦鹉 / 豹猫 / 猫）都需要 `Extractor.java` 导出的实体数据访问器 index
  （`generated/entity_types.rs` 已有这五种的尺寸 facts，但 `pub const` 访问器常量里没有
  它们的 `DATA_*`；`Extractor.java` 只为已做生物导出 `entityAccessor(...).id()`）——按 5.12 /
  5.15「动生成器 / 元数据 index 前先和队长确认」，无人值守单轮无法确认。青蛙另须按 brain
  报告整块一轮搬完 `FrogAi`；骆驼 / 鹦鹉上肩须等世界模型 8.2「载具和它的乘客」定案（涉
  tick 顺序 / 岛拆分，按架构规矩停下写汇报、不自己定）；豹猫 / 猫新增 `MobType` 会一次改到
  ~35 处穷举 match、14 个共用文件并碰 `natural-spawn` / `check_spawn_rules` 关键路径，还要
  新建 Java / Rust 逐刻场景，与并行队员冲突面大。故本轮只做复核 + 一个安全守卫。
- 两个 `### 5.11` 与 5.12~5.14 的错乱交叉引用仍未自行重编号，留给队长统一处理（理由同
  5.15 / 5.16 / 5.17）。
- 全套完成标准检查在最终提交上重跑，逐项数字与结果见本轮汇报 `reports/op-misc.md` 第 2 节。
- 交接：请队长二选一 —— (A) 把本轮提交 fast-forward 合进 main；(B) 先确认 `Extractor.java`
  的 `DATA_*` 导出、再单独开一整轮按 5.12 规格做豹猫（共用面最小）或按 brain 规格整块做青蛙。

### 5.19 第 15 轮：合并青蛙与骆驼 main、实现豹猫与猫并通过逐刻与存档对拍（op-misc，2026-11-22）

- **合并 main**：先后合并 `4ad4370c`（青蛙 / 蝌蚪 / Brain）与 `c72e9b0d`（骆驼 / Brain 姿势机），
  合并后 `main` 为 `HEAD` 祖先，合回 `main` 为无冲突 fast-forward。合并骆驼时因 `main` 占用了
  `MobOracle` 场景编号 `20`（`MobType::Camel`），将豹猫与猫的场景编号顺延为 `21` / `22`，避免碰撞；
  `MobType::ALL` 扩至 40 种并由 `mob_type_registry_ids_round_trip` 守卫。
- **实现豹猫（`ocelot.rs`，提交 `3b9c3c27`）**：
  - 生成器导出 `OCELOT_TRUSTING = 18`；属性 10 / 0.3F / 3.0，成年 0.6×0.7、幼崽 0.3×0.35（眼高 0.34375）；
    `Trusting` 存盘与同步。
  - 新增 `LivingEntity.setSprinting` 瞬态速度修饰符（`+0.3F` `AddMultipliedTotal`）、`Attribute.sync_version`
    脏计数（每次 `setSprinting(true)` 按官方 `AttributeInstance.setDirty()` 触发下一次同步包）和
    `Entity.spawnSprintParticle` 服务端 `2 × nextDouble()` 随机取数。
  - 完整目标链（`Float` / `OcelotAvoidEntityGoal` / `OcelotTemptGoal` / `LeapAtTarget` / `OcelotAttackGoal` /
    `Breed` / `WaterAvoidingRandomStroll` / `LookAtPlayer`，捕杀 `Chicken` 与陆地幼年 `Turtle`）、信任交互
    （1/3 + 7 组高斯取数 + 事件 40/41）、丛林/竹林 `MONSTER` 自然刷怪（`checkOcelotSpawnRules` 2/3 与
    `checkSpawnObstruction` 要求海平面以上、草方块或树叶且无液体）。
  - 顺手修了两处既有的海龟缺陷（`ocelot_hunt_turtle` 暴露）：`turtle.rs` 眼高从 `height` 改为
    `height * 0.85`（成年 0.34、幼年 0.102，对齐 `EntityTypes.TURTLE`），`TurtleRandomStrollGoal` 改用
    `PLAIN_STROLL` 并补全 `TurtleRandomStrollGoal` / `TurtlePanicGoal` / `TurtleBreedGoal` 的 `subclass_name`。
- **实现猫（`cat.rs`，提交 `ab1a863e`）**：
  - 生成器导出 5 个索引（`CAT_VARIANT` / `CAT_IS_LYING` / `CAT_RELAX_STATE_ONE` / `CAT_COLLAR` /
    `CAT_SOUND_VARIANT`），协议层补 `SERIALIZER_CAT_VARIANT = 21` 与 `SERIALIZER_CAT_SOUND_VARIANT = 22`。
  - 复用 `tame.rs` 可驯服框架与 `wolf::mixed_dye` / `nutrition_of`；实现 11 种花色、2 种叫声变体 × 9 槽位
    （含幼猫 `entity.baby_cat.*`）、月相选花色（满月 `>= 0.9` 允许 `all_black`）、驯服 / 染项圈 / 喂鱼回血 /
    求爱繁殖（不重抽 `sound_variant`）/ 切换坐站、`CatTemptGoal` / `CatLieOnBlockGoal` / `CatSitOnBlockGoal` /
    捕杀兔子与陆地幼年海龟、以及 `interact.rs::play_eating_sound` 对猫吃食声的分派。
- **未实现与交接**：本分支 10 种动物至此已完成 9 种（哞菇、山羊、北极熊、熊猫、青蛙/蝌蚪、犰狳、骆驼、
  豹猫、猫），仅剩 **鹦鹉（Parrot）**（需飞行导航 `FlyingPathNavigation` + 玩家肩膀乘骑 `ShoulderRidingEntity`
  与世界模型 8.2 乘客方案）。猫的村庄 `CatSpawner`、沼泽小屋猫、睡觉主人的 `CatRelaxOnOwnerGoal`/晨间礼物、
  `CatSitOnBlockGoal` 箱子开合人数判定以及苦力怕躲避豹猫/猫的 `AvoidEntityGoal` 均已记入 README 已知限制与汇报。

---

### 5.20 第 16 轮：实现鹦鹉、飞行导航与鹦鹉的自然刷怪（op-misc，2026-11-22）

- **新基础设施**：
  - `path.rs` 新增 `EvaluatorFlags.fly` 与 `FlyNodeEvaluator` 的逐位移植（`fly_start` / `fly_probe` /
    `fly_neighbors` / `fly_path_type`）：六轴 + 单轴斜向 + 水平对角 + 八个斜向角，探测顺序照官方；
    `WALKABLE` 节点代价加一；`OPEN` 格的下方规则（`FIRE` / `DAMAGING` / `COCOA` / `FENCE` / 其余转
    `WALKABLE`）和 `checkNeighbourBlocks` 保留传入类型。小体型生物的十个随机起点候选没有接（鹦鹉的
    `getSize()` 是 0.633、本该走那条，见 §5.21）。
  - `navigation.rs`：飞行导航的 `canUpdatePath`（不是乘客恒真）、`createPath` 跳过加载与地表检查、
    `tick` 的 `getGroundY` 跳过、`getTempMobPos` 用实体坐标、`stable_destination` 用实心近似；
    `canMoveDirectly` 的「流体也算阻挡」目前只挡源方块水（已知差异）。
  - `control` / `living.rs`：`FlyingMoveControl(10, false)`（空中用 `FLYING_SPEED`、`setNoGravity`、
    `yya` 竖直速度）、`omnidirectionalAirMover` 的竖直 0.91 空气阻力、`Parrot.checkFallDamage` 空实现、
    `hurtServer` 清坐姿。
  - `goals.rs` 新增 `Goal::Parrot` 与 `hover_random_pos` / `air_and_water_random_pos`；`RandomPos`
    家族的扇形偏移改成官方的 f32 乘法 + `Mth.SQRT_OF_TWO`（float）常量。
  - 实体跟踪补 `DATA_EFFECT_PARTICLES` / `DATA_EFFECT_AMBIENCE_ID`（可见效果的颜色列表，ARGB 补不透明
    alpha），曲奇中毒的死亡刻因此和官方包一致。
- **鹦鹉（`parrot.rs`）**：属性 6 / 0.2 / 0.4 / 3、尺寸 0.5×0.9（眼高 0.765）、五种颜色
  （`PARROT_VARIANT = 20`，`Variant` 用官方旧版整数编码存盘）、不繁殖、不能长大；`Parrot.registerGoals`
  的 8 个目标（恐慌 / 浮水 / 看玩家 / 坐下 / 跟主人 / `ParrotWanderGoal` / `LandOnOwnersShoulderGoal` /
  `FollowMobGoal` 占位）；种子 1/10 驯服（事件 7/6、吃食声两次 `nextFloat`）、曲奇中毒 900 刻 + 玩家攻击
  `Float.MAX_VALUE`（`MobInteraction::self_hurt` 交给游戏层结算）、主人切换坐站；每 400 刻的世界随机源模仿
  附近生物、每 1000 次的叫声里挑 42 条模仿音之一；`calculateFlapping` 与翅膀声；自然刷怪
  `#parrots_spawnable_on` + 亮度 > 8；`finalizeSpawn` 抽品种 + `AgeableMobGroupData(false)`（不抽幼崽）。
- **对拍**：`mob-oracle --only parrot` 4 例 860 刻 0 不一致（`parrot_idle` 树桩闲逛、`parrot_tame`
  驯服与坐站、`parrot_cookie` 中毒死亡含掉落与包、`parrot_imitate` 模仿僵尸）；`natural_spawn` 734 例
  2936 刻 0 不一致（`parrot_jungle=31`、`parrot_jungle_dim=0`）；`gen_spawn` 7000 例 0 不一致
  （新增 `gen_parrot`）。
- **已知限制与交接**：上肩只做到 `LandOnOwnersShoulderGoal` 运行（冷却、主人旁观者、没命令坐下），
  玩家 `ShoulderEntityLeft` / `Right` 存档与同步未建模，所以两箱相交时不 `discard()`；`FollowMobGoal`
  占位；`MOB_SOUND_MAP` 的 `HashMap` 迭代顺序按名字排序近似；悬停落点的随机源错位已在 §5.21 定位并修掉；
  玩家的飞行 / 水里 / 细雪状态没进 `PlayerView`；拴绳未建模。


### 5.21 第 17 轮：补上 `LandRandomPos.generateRandomPosTowardDirection` 的 `isNotStable`（op-misc，2026-11-22）

- **根因**：`goals.rs` 的 `land_toward_direction` 只做了官方的「越界」和「离家」两个判定，少了第三个
  `!GoalUtils.isNotStable(mob.getNavigation(), pos)`。Bamboo 里这个函数只有 `hover_random_pos` 调用
  （`land_random_pos` 是另一份内联实现、本来就带 `stable_destination`），所以只有悬停落点缺这条判定：
  官方会丢掉一批「目标格站不住」的候选，Bamboo 却接受，10 次抽点的接受序列因此不同，从选中落点那一刻起
  实体随机源就错位两次。
- **修复**：补上 `!stable_destination(mob, view, pos)`（飞行导航里是 `FlyingPathNavigation.isStableDestination`
  的实心方块近似，见 README 已知差异）。
- **验证**：`mob-oracle --only parrot` 4 例 860 刻仍 0 不一致；一个临时的无树桩 400 刻悬停场景（平坦地面、
  种子 906、未进对拍表）从 308 处不一致降到 3 处，随机源逐刻一致；残留 3 处（第 245–247 刻）只有 `xr` / `hr`，
  官方那几刻 `LookAtPlayerGoal` 已在看玩家、Bamboo 还没开始，随机源相同，原因未查到。
- **仍没做**：`FlyNodeEvaluator.getStart` 小体型生物（`getSize() < 1.0`）的十个随机起点候选——寻路请求里没有
  实体随机源，只试包围盒四角；鹦鹉的 `getSize()` 是 0.633，理论上会走那条，只在起点格代价为负时触发。

---

## 6. 玩家生命期 0 + 1a（2026-11-18）

期 0 的公共接缝和期 1a 的「受伤与减伤」已合入。以下列出**本期范围**和**已知差异**，
供 1b（死亡、掉落、重生）和期 2/3 的敌对生物接着做。

### 6.1 已做

- `DamageKind` 扩到 22 种（含 `MobAttackNoAggro`、`Arrow`、`Starve`、`Thorns`、`Magic`、
  `Explosion`），`DamageSource` 补 `direct` / `raw_position` / `scale_when_non_player` /
  `weapon`；护甲 / 附魔 / 吸收 / 无敌帧 / 冷却 / 饥饿消耗的纯函数在
  `mc-bamboo-blocks/src/entity/mob/damage.rs`。
- `BlockView::difficulty` / `sky_darken` / `can_see_sky` / `max_local_raw_brightness`
  / `game_time` 在精确 `World` 和并行 `SourceView` 上给同一份答案；权威副本仍是
  `GlobalView`，`World::difficulty_id()` 不再写死为普通。
- `Player` 增加 `health`（血量、吸收、三个倒计时、饥饿四字段、上次伤害种类 / 方向），
  与 `PlayerData` 的 `Health` / `HurtTime` / `DeathTime` / `AbsorptionAmount` / `food*`
  双向接上；`Player::view` 填真实 `alive` 和 `armor_cover`。
- 出站包：`set_health`、`damage_event`、`hurt_animation`、`player_combat_enter`、
  受伤音效，以及 `set_entity_data` 里的真实血量。`/damage <target> <amount> [type]
  [at|by [from]]` 接在 `SCOPES` 的 `World` 范围里；`serverbound::ATTACK` 能打非玩家
  实体，伤害按手持物品的 `attribute_modifiers`。近战攻击的完整链路见 6.4。
- 对拍：`mob-oracle` 原 52 场景加 36 个 `player_hurt_*` 场景，逐刻比血量 / 吸收 /
  无敌帧 / 冷却 / 饥饿 / 速度 / 随机源 / 包字节；`player-data` 增加生命和饥饿的
  非默认往返用例；五处故意改坏（难度缩放、护甲、保护附魔、无敌帧、击退）都能抓到。
  op-combat 再加 12 个 `player_attack_*` 近战场景（见 6.4）。

### 6.2 新增的按玩家字段（岛拆合逐个说明）

`Player` 整体随 `IslandPart::Players` 搬，**没有新增 `IslandPart`**，也没有改
`crates/mc-bamboo-game/src/region/island_split.rs`。逐字段：

| 字段 | 类型 | 绝对时刻？ | 拆岛 / 并岛 |
|---|---|---|---|
| `health.health` | `f32` | 否 | 整行搬 |
| `health.absorption` | `f32` | 否 | 整行搬 |
| `health.hurt_time` | `i32` 倒计时 | **否** | 整行搬，不需平移 |
| `health.hurt_duration` | `i32` 倒计时 | **否** | 整行搬，不需平移 |
| `health.damage_cooldown` | `i32` 倒计时 | **否** | 整行搬，不需平移 |
| `health.last_hurt` | `f32` | 否 | 整行搬 |
| `health.invulnerable_time` | `i32` 倒计时 | **否** | 整行搬，不需平移 |
| `health.death_time` | `i32` 倒计时（本期恒 0） | **否** | 整行搬，不需平移 |
| `health.food.{food_level, saturation, exhaustion, tick_timer}` | 数值 | 否 | 整行搬 |
| `health.last_damage_source` | `Option<DamageKind>` | 否 | 整行搬 |
| `health.hurt_dir` | `f32` | 否 | 整行搬 |
| `health_sync`（`lastSentHealth` 等三个判据） | 数值快照 | 否 | 整行搬 |
| `in_combat` | `bool` | 否 | 整行搬 |
| `attack_strength_ticker`（6.4） | `i32` 相对计数 | 否 | 整行搬，不需平移 |
| `last_main_hand_item`（6.4） | `ItemStack` | 否 | 整行搬 |

这一期**刻意不建** `last_damage_stamp`（官方存的是 `Level.getGameTime()` 的绝对值），
它只在 1b 拼死亡消息时需要。届时要么存相对倒计时，要么登记进 `ClockParts` 的
`players` 一类（当前 `ClockParts` 没有这一类并需要在报告里明说）。

### 6.3 已知差异（1a 的范围）

- **减伤精度选项 A**：只做护甲值 / 韧性 + `minecraft:protection` 等级的静态求和。
  武器 `minecraft:breach`（`modifyArmorEffectiveness`）、抗性效果、格挡
  (`BLOCKS_ATTACKS`)、不死图腾、四种专性保护附魔都**未做**。对拍场景里用无附魔
  武器、无药水效果。
- **饥饿**：1a 时四个字段只按官方缺省值存读档、不 tick；1b 起按下节实现完整的
  `FoodData.tick` / 和平回血 / 饥饿掉血（见第 7 节）。
- **攻击（op-combat，2026-11-21 起已合入，见 6.4）**：`handleAttack` → `Player.attack`
  的完整链路已经做了：冷却 / 暴击 / 横扫 / 额外击退 / 伤害附魔 / 火焰附加 / 耐久 /
  粒子 / 音效 / 动画 / PVP 开关。仍缺的部分见 6.4 和 README「玩家近战攻击的已知限制」：
  `getAttackDamageBonus` / `PIERCING_WEAPON` 的武器行为、`onClimbable` / `isInWater`
  的暴击前置（`isPassenger` / `isMobilityRestricted` 已按 `riding.vehicle` / `EffectMap`
  判）、节肢杀手缓慢时长的随机源、`Stats.DAMAGE_DEALT` 的落盘、
  以及 `mob-oracle` 对攻击音效 / 粒子 / 动画包的逐字节比对（官方假世界不捕获）。
  旁观者和非法目标仍按官方 `handleAttack` 忽略（官方会断开连接，Bamboo 只忽略）。
- **死亡**：1a 时血量可以降到 0 但不触发死亡；1b 起按下节实现完整死亡 / 掉落 /
  重生流程，`/kill` 也不再报错（见第 7 节）。
- **玩家受伤音效**：`DamageType.effects` 的六种变体和 `thorns` 的第二声已按官方发；
  但 `LivingEntity.handleDamageEvent` 的 `getVoicePitch()` 随机变体（玩家恒 1.0）
  未做，`playSecondaryHurtSound` 只覆盖 `thorns`。
- **实体事件**：1a 不发 `entity_event`（官方玩家受伤时 `markHurt` 只置
  `syncVelocity`，不广播 2 号事件）；`set_entity_motion` 的速度同步包未单独对拍。
- **天色 / 时钟**：新世界时钟按官方从 0 刻走起来，`sky_darken` 由 `GlobalView`
  每刻按官方 `Level.updateSkyBrightness` 算好；`ViewFacts::sky_darken` 由调用方填这份值，
  `SourceView` 和 `World` 的读数一致。
- **服务器连接层**：`serverbound::ATTACK` 的畸形包按官方会断开，Bamboo 目前
  只在解码层报 `TrailingBytes`，连接层是否断开由通用解码路径决定。

### 6.4 玩家近战攻击（op-combat，2026-11-21）

`crates/mc-bamboo-game/src/player_attack.rs` 照官方 26.3
`ServerGamePacketListenerImpl.handleAttack` → `Player.attack` →
`ServerPlayer.crit` / `magicCrit` / `getEnchantedDamage` 翻译，入口仍是
`player_hurt::on_attack`（`world.rs` 的 `serverbound::ATTACK` 与对拍重放共用）。
`on_attack` 拆成 `resolve_target` / `plan_attack` / `land_attack` / `finish_hit`
四步，中间产物放在 `AttackPlan`。

- **冷却**：`Player.attackStrengthTicker`（新增按玩家字段，见 6.2）每刻在玩家 tick
  开头 `+1`；`handlePunch`（挥空）、`onAttack`（每次攻击）、主手物品换成不同物品 /
  组件时清零。`getCurrentItemAttackStrengthDelay = 1 / ATTACK_SPEED × 20`，
  `s = clamp((ticker + 0.5) / delay, 0, 1)`。基础伤害 `× (0.2 + s² × 0.8)`，
  附魔加成 `× s`。玩家 `ATTACK_DAMAGE` 基值是 `Player.createAttributes()` 的 1.0，
  **不是**属性注册表缺省的 2.0（对拍抓到过这个差）。
- **暴击 / 横扫 / 击退**：`s > 0.9` 且下落、不着地、没失明、没骑乘、非疾跑时
  `× 1.5`，发 `ClientboundAnimatePacket`(1) 和 `player.attack.crit`；附魔加成 > 0
  再发 (2)。拿剑、着地、移动够慢、满充能且非暴击非疾跑时横扫，附魔
  `sweeping_edge` 给 `SWEEPING_DAMAGE_RATIO = L/(L+1)`。疾跑 / `knockback` /
  `attack_knockback` 的额外击退走 `World::knockback_mob`，同时把攻击者水平速度
  `× 0.6` 并 `setSprinting(false)`。
- **附魔与效果**：锋利 `1 + 0.5×(L−1)`、亡灵 / 节肢 `2.5×L`（按生成的实体类型
  标签判定），火焰附加 `4×L` 秒点燃、节肢杀手挂缓慢 IV（`doSweepAttack` 对每个横扫
  命中的 nearby 也调一次 `doPostAttackEffects`）；`hurtEnemy` 扣 1 点耐久；
  打中加 0.1 饥饿消耗、记 `lastHurtMob`。26.3 的数据驱动附魔表由生成器导出，
  这里只按 `Enchantments.java` 的 `AddValue` / `Fraction` 公式算。
- **发包**：`world.play_sound` 把攻击音效记进同一个 `World` 的效果列表，落地后
  一次 `send_effects` 发出（弓箭曾踩过「生成了效果却没发出去」的坑）；
  `ClientboundAnimatePacket`、`LevelParticles`（`DAMAGE_INDICATOR` / `SWEEP_ATTACK`）、
  `swing_animation` 都走 `links`。`a_critical_attack_emits_its_packets`、
  `a_sweep_attack_emits_its_sound_and_particle`、
  `an_enchanted_attack_emits_the_magic_critical_animation`、`a_weak_attack_emits_its_sound`、
  `a_full_strength_attack_emits_the_strong_sound` 和
  `damage_indicator_particles_follow_the_damage_threshold` 单测按注册表 ID / 动作字节
  确认暴击 / 附魔暴击动画、横扫 / 伤害指示粒子（含 `actualDamage > 2` 门槛）和
  strong / weak / sweep 音效真的发到客户端；`crit` / `knockback` / `nodamage` 音效仍
  只按包类型松散覆盖或未精确断言（见 README 已知限制）。
- **PVP**：`pvp` 为真时按 `ServerPlayer.canHarmPlayer`（`isPvpAllowed` + 队伍，
  Bamboo 无队伍）打另一名玩家，走 `hurt_player_from_mob`；否则忽略。额外击退 /
  火焰附加 / 横扫波及的玩家这几项目标在实体表里的操作（`knockback_mob` /
  `ignite_mob` / 候选收集）对玩家不生效，玩家不在实体表里；见 README 已知限制。
- **对拍**：`mob-oracle --only player_attack` 13 个场景（充能 0/2/4/5/20、下落暴击、
  疾跑击退、横扫、`sweeping_edge`、横扫 + 火焰附加、锋利 / 火焰附加 / 击退）共 91 刻，
  逐刻比每头牛的血量 / 击退速度 / 着火刻数 / 受伤时间 / 是否移除和 `damage_event` 包，
  0 不一致；`--break-self-test attack_charge` 故意把充能少算一刻，牛的血量立刻不一致；
  `--break-self-test sweep_effects` 去掉武器上的火焰附加，横扫目标的着火刻数立刻不一致。
  单测 `a_sweep_ignites_swept_targets_with_fire_aspect` 覆盖横扫目标也着火。
  攻击场景的官方夹具跳过 `cow.tick()`，否则重力 / AI 会污染击退速度；假世界也不
  捕获 crit 动画 / 粒子 / 挥手 / 耐久槽包，这些由单测覆盖（见 6.3 与 README）。
  够距判定（`within_block_interaction_range` + 方块点 vs 官方 `isWithinAttackRange` +
  AABB + 武器距离 + 世界边界）、横扫候选盒近似、`baseDamage > 0 || magicBoost > 0`
  门槛，以及属性修饰符 / `getSpeed()` / 受害者装备附魔等其余已知差异见 README
  「玩家近战攻击的已知限制」；够距这一层没有 `mob-oracle` 场景（夹具直接调
  `Player.attack`）。

---

## 7. 玩家生命期 1b（死亡、掉落、重生与自然回血）

期 1b 已合入。官方字节码核对范围：`ServerPlayer.die` / `LivingEntity.die` /
`dropAllDeathLoot` / `Player.dropEquipment` / `Inventory.dropAll` /
`ExperienceOrb.award` / `PlayerList.respawn` / `ServerPlayer.restoreFrom` /
`FoodData.tick` / `ServerPlayer.tickRegeneration` /
`CombatTracker.getDeathMessage` / `DamageSource.getLocalizedDeathMessage` /
`ServerGamePacketListenerImpl.handleClientCommand` /
`Player.addAdditionalSaveData` / `Player.readAdditionalSaveData`。

### 7.1 重置 / 保留字段清单

官方重建 `ServerPlayer` 再 `restoreFrom(old, keepEverything=false)`；Bamboo
**原地重置 `Player`、不换对象**，唯一入口是
`crates/mc-bamboo-game/src/player_respawn.rs::reset_after_respawn`。以后改成重建对象
只需改这一处和 `respawn()` 里的包序列。

| 字段 | 重置为 | 保留 |
|---|---|---|
| `health`（血量 / 吸收 / 三个受伤倒计时 / 饥饿四字段） | 官方构造默认值 | — |
| `dead` / `awaiting_respawn` / `in_combat` / `deathTime` | false / 0 | — |
| `combat_entries` / `combat_idle_ticks` / `hurt_by_player` / `hurt_by_mob` | 清空 / 0 / None | — |
| `health_sync` 三个哨兵 / `last_sent_exp` | `INITIAL` / -1 | — |
| `inventory` / `experience` / `score` | 非 `keepInventory` 时清空 | `keepInventory` 或旁观者时原样保留 |
| `enchantment_seed` / `ender_chest` / `respawn` / `last_death_location` | — | 原样保留（官方 `restoreFrom` 也搬） |
| `id` / `uuid` / `name` / `game_mode` / 权限 / `client` | — | 原样保留 |
| `sync`（含 `seen_by`）/ `menus` / `player_grid` 登记 / `links` / `inbox` | — | 原样保留，不重发配对包 |
| `random` / `chat` / `spam` / `last_action_millis` / `peer_ips` | — | 原样保留 |
| 光标 / 采矿 / 预测 / 速度 / 传送确认 / `take_xp_delay` / `recorded_criteria` | 构造默认值 | — |
| `entity_ticks` | 0（相对计数，不是绝对时刻） | — |

新增按玩家字段全部不是绝对时刻，整行随 `IslandPart::Players` 搬；
**没有改 `region/island_split.rs`，也没有新增 `IslandPart` 类别**。
官方 `LivingEntity` 用实体自己的 `tickCount` 存 `lastHurtByPlayerMemoryTime`
（每刻减 1，到 0 清空）、`lastHurtByMobTimestamp`（`tickCount - timestamp > 100`
清空）和 `CombatTracker.lastDamageTime`（战斗中 300 刻 / 否则 100 刻过期）；
Bamboo 全改成相对倒计时 `memory` / `age` / `combat_idle_ticks`，
`CombatEntry` 只存种类、显示组件、`damage`、`fall_distance` 和 `fall_location` id，
不存绝对 `tickCount`。条目在死亡（`recheckStatus`）与重生时清空。

### 7.2 重生包序列与官方逐包对照结论

`PlayerList.respawn` 的 `connection.send` 顺序（`mob-oracle` 的 29 个
`player_death_*` / `player_regen_*` 场景逐字节比对，
全量 117 cases / 52844 ticks / 0 不一致）：

1. `player_abilities`（`restoreFrom` 里 `onUpdateAbilities`）
2. `respawn`（`CommonPlayerSpawnInfo` + `dataToKeep=0`）
3. `player_position`（`connection.teleport`）
4. `set_default_spawn_position`
5. `change_difficulty`
6. `set_experience`
7. `sendActivePlayerEffects`（Bamboo 没有药水效果，空）
8. `sendLevelInfo`：`initialize_border` / `set_time` /
   `set_default_spawn_position` / 雨的三个 `game_event` / `game_event` 13
9. `sendPlayerPermissionLevel`：`entity_event` 权限 + 命令树
10. `addRespawnedPlayer` 的配对包与 `initInventoryMenu`：
    `set_entity_data` / `update_attributes`（仅创造）/ `set_equipment` /
    `container_set_content`（46 格）/ `set_held_slot` / `player_abilities`
11. 下一刻 `doTick` 补 `set_health` / `set_experience`

假世界用一个空根命令节点给 `commands`（16）包占位，**它的 ID 位置进入
包序列比较，字节内容不比**（命令树内容由 `command-oracle` 负责）；
其余关键包（`player_combat_kill` / `system_chat` / `respawn` / `player_position` /
`set_default_spawn_position` / `change_difficulty` / `set_experience` /
`player_abilities` / `set_health` / `entity_event` / `damage_event` / `sound`）
按官方 `StreamCodec` 逐字节相等；配对包的字节由假世界的桩构造，按 ID 序列比。
`CommonPlayerSpawnInfo` 的 `lastDeathLocation` 与 `seaLevel` 用 Bamboo 现有的
超平坦常量，官方编解码器一致。`previousGameType` 恒 `None`。

死亡时的包顺序：`damage_event` → `hurt_animation`（`indicateDamage`）→ 死亡音效
（`entity.player.death`，非受伤音效）→ `player_combat_kill` → `system_chat`
（全服广播）→ `entity_event`(3) → 可选 `player_combat_end` → `set_health`。
`show_death_messages=false` 时 `PlayerCombatKill` 的消息体是空组件且不广播。

### 7.3 存档

`PlayerData` 新增 `LastDeathLocation`（`GlobalPos.CODEC`：维度字符串 + pos 三个 int），
`Optional` 为空时不写键；`runtime_player_data` / `restore_inventory` 双向接上，
`player-data` oracle 用官方 codec 加了用例（3016 checks / 496 cases）。
`DeathTime` 与 `Health` 的存读沿用 1a；`dead` 是运行时标志，官方也不存档，
读档时按 `Health<=0` 推。

### 7.4 新增已知差异

- **饥饿消耗来源**：本期只做受伤（`causeFoodExhaustion`）和回血本身
  （`FoodData.tick` 里的 `addExhaustion`）；移动、跳跃、疾跑、攻击、挖掘的消耗
  和吃东西不做。创造 / 旁观模式下 `FoodData.tick` 仍按官方跑（官方无条件调用），
  但创造模式的能力无敌照旧；原有创造模式测试未变。
- **摔落伤害**：仍**不做**，只记录 `fall_distance`。下一项单独做。
- **死亡消息**：`getLocalizedDeathMessage` 的 `.player` 分支按
  `getKillCredit()`（`lastHurtByPlayer` 优先，否则 `lastHurtByMob`）实现；
  `FALL_VARIANTS` 的 `getMostSignificantFall` / `getFallMessage`
  （`death.fell.accident.*` / `assist` / `finish` / `killer`）与
  `INTENTIONAL_GAME_DESIGN` 的带链接分支都按字节码留着。新增 7 个
  `player_death_credit_*` / `player_death_fall_*` 场景（含 100 刻过期和
  101/102 刻边界），以及 `--break-self-test credit` 改坏确认。队伍可见性
  （`broadcastSystemToTeam` / `...ExceptTeam`）按全服广播处理。
  `INTENTIONAL_GAME_DESIGN` 在 26.3 建模的伤害种类里触发不到（只有
  `minecraft:bad_respawn_point` 用它），其字节未单独对拍。
  `FallLocation.getCurrentFallLocation` 的攀爬物（梯子 / 藤蔓 / 脚手架）与
  水中判定未做，`CombatEntry.fall_location` 恒 `None`，所以摔落消息只覆盖
  `death.fell.accident.generic` 这一档。
- **重生点**（1c 起按官方）：进服与无床 / 锚重生共用
  `spawn_landing::landing` → `PlayerSpawnFinder.findSpawn`。两格都能重生的
  `/spawnpoint` 强制点按 `findRespawnAndUseSpawnBlock` 的 forced 分支直接落地；
  否则回退到世界出生点并补发 `NO_RESPAWN_BLOCK_AVAILABLE`。床 / 重生锚的
  `findStandUpPosition`、充能扣减和末地维度传送仍未做。
- **进服位置**：新玩家由 `PrepareSpawnTask.start` 的 `findSpawn` 决定；老玩家
  照官方 `SavedPosition.position()` 用存档里的精确位置，不再重跑搜索。
- **`dataToKeep`**：官方末地传送重生传 `KEEP_ALL_DATA`，Bamboo 恒 0；
  末地 / 维度切换未做。
- **命令树包**：重生序列里的 `commands` 包内容由 `command-oracle` 管，
  这轮对拍只比它的 ID 位置（假世界发一个空根占位）。
- **死亡后的 `FoodData.tick`**：官方 `ServerPlayer.doTick` 在死亡界面等待期间
  仍跑 `FoodData.tick`（`FoodData.tick` 自己按 `isHurt()` 分支），Bamboo 的
  `player_hurt::tick` 在 `dead` 为真时跳过食物 / 回血，只递减受伤倒计时；
  等待重生通常只有几刻，对拍场景（最多 2 刻）逐刻状态一致。
- **死亡动画**：`deathTime` 只存相对倒计时，不推进 `tickDeath` 的
  `remove(KILLED)`；玩家对象不重建所以不需要移除。
- **计分板 / 统计**：`DEATH_COUNT`、`ENTITY_KILLED_BY`、`TIME_SINCE_DEATH`
  等官方死亡时的计分板 / 统计奖励未接。

## 8. 玩家出生落脚点 1c（进服与普通世界重生）

### 8.1 官方算法（`javap -c -p` 核对的方法名）

- 进服 `PrepareSpawnTask.start`：`SavedPosition.position()` 为空（新玩家）时调
  `PlayerSpawnFinder.findSpawn(spawnLevel, respawnData.pos)`，返回的 `Vec3` 由
  `PrepareSpawnTask$Ready.spawn` 的 `ServerPlayer.snapTo` 使用；`Preparing.tick`
  先等 future 完成，再给最终位置所在区块挂 `PLAYER_SPAWN` 半径 3 的加载票，
  票就绪后才让玩家进场。老玩家的 `SavedPosition.position()` 直接用存档坐标。
- 重生 `PlayerList.respawn` → `ServerPlayer.findRespawnPositionAndUseSpawnBlock`
  → `findRespawnAndUseSpawnBlock`。`RespawnConfig` 的 `forced` 位（`/spawnpoint`
  通过 `SetSpawnCommand.setSpawn` 写入）且 `spawn.pos` 与 `spawn.pos.above()`
  都满足 `Block.isPossibleToRespawnInThis` 时，直接返回
  `new Vec3(x + 0.5, y + 0.1, z + 0.5)`，不散布。个人点失效、或没有个人点，
  走 `TeleportTransition.missingRespawnBlock` / `createDefault` →
  `findAdjustedSharedSpawnPos` → `Entity.adjustSpawnLocation` →
  `ServerPlayer.adjustSpawnLocation` → `PlayerSpawnFinder.findSpawn`，前者
  在 `PlayerList.respawn` 里补一包 `NO_RESPAWN_BLOCK_AVAILABLE` 并清个人点。
- `PlayerSpawnFinder.findSpawn`：`WorldData.getGameType() == ADVENTURE` 直接
  `fixupSpawnHeight`；否则 `radius = min(max(0, respawn_radius),
  floor(WorldBorder.getDistanceToBorder(x, z)))`，边界距离 ≤ 1 时取 1。
  `candidateCount = min(1024, (2r+1)^2)`，`coprime = count <= 16 ? count-1 : 17`，
  `offset = RandomSource.createThreadLocalInstance().nextInt(count)`。
  `scheduleNext` 按 `slot = (offset + coprime*i) % count` 走
  `x = origin.x + slot % (2r+1) - r`、`z = origin.z + slot / (2r+1) - r`；
  `scheduleCandidate` 先 `ServerChunkCache.addTicketAndLoadWithRadius(SPAWN_SEARCH, ..., 0)`
  再调 `lambda$scheduleNext$0`：`getLevelRespawnPos`（MOTION_BLOCKING /
  WORLD_SURFACE / OCEAN_FLOOR 三张高度图、流体、`Block.isFaceFull(..., UP)`）+
  `noCollisionNoLiquid`（`EntityDimensions.PLAYER.makeBoundingBox` 的
  `noCollision(null, AABB, true)`），命中则 `Vec3.atBottomCenterOf`。
  全部候选失败时 `scheduleNext` 最后一次调 `fixupSpawnHeight`。
- `ServerPlayer.createCommonSpawnInfo` 的 `isFlat()` 读 `WorldData.isFlatWorld()`，
  `getSeaLevel()` 读 `ServerChunkCache.getGenerator().getSeaLevel()`：
  `FlatLevelSource` 恒 -63，`NoiseBasedChunkGenerator` 读
  `NoiseGeneratorSettings.seaLevel()`（三个主世界预设都是 63）。

### 8.2 我们的实现

`crates/mc-bamboo-game/src/spawn_landing.rs` 是进服与重生**共用的唯一入口**：

- `landing` 判 `/spawnpoint` forced 点，再走 `find_spawn`；`find_spawn_impl`
  复刻半径、互素步长、候选序列、三张高度图、流体 / 碰撞形状与 fixup 兜底。
  随机源是 `mc-bamboo-blocks::entity::JavaRandom`（官方
  `SingleThreadedRandomSource` 的同一条 LCG）；生产按官方语义用不确定的线程
  本地种子，对拍两边注入固定种子。
- `region.rs::add_player`：`login` / `respawn` 包的 `is_flat` 与 `sea_level`
  由发布到 `GlobalView` 的 `world_gen`（存档优先）给出。新玩家走搜索；老玩家
  按官方 `SavedPosition` 用存档位置。
- `player_respawn.rs::respawn`：共用 `landing`，失效个人点发
  `NO_RESPAWN_BLOCK_AVAILABLE` 并清空；跨岛仍走 1b 的
  `place_player_at_respawn` / `relocate_players`。

### 8.3 区块未就绪时的做法

官方在配置阶段同步等 `SPAWN_SEARCH` / `PLAYER_SPAWN` 票把区块生成完再让玩家
进场；Bamboo 的岛上不能同步生成，也不在主控线程等待：

- 搜索碰到未交付的**噪声世界**候选列就返回 `NotReady`；进服先用
  `fixupSpawnHeight` 那段高度的临时位置登记玩家、挂现有的玩家票，
  **不发** `player_position`；重生同样先不落位。等待期间不跑物理、不受伤、
  不处理移动包（`player_move` / `player_hurt` / `movement::finish_tick`）。
- `spawn_landing::finish_pending` 在收信 / 推进一刻时检查覆盖的区块，
  全部就绪就重跑同一搜索并补发位置包；超平坦模板在所有位置内容一致，
  未交付位置直接读 store 的模板，不需要等待。40 刻（2 秒）超时后照已加载
  的部分算一次并记日志，不让玩家永远卡在加载界面。
- 单元测试 `unready_noise_chunk_defers_until_the_same_landing_is_available`
  断言：区块没就绪返回 `NotReady`，交付后同一个搜索给出与全就绪时相同的
  精确落点。

### 8.4 对拍与改变过的期望

`MobOracle.java` 新增 7 个 `player_spawn_*` 场景（地表 / 水面 / 树上 /
半径 0 / 缺省 10 / 个人点可用 / 个人点被堵 / 冒险模式），反射构造官方
`PlayerSpawnFinder` 并把候选 `offset` 钉成固定 LCG 重放值；Rust 侧
`spawn_landing_sample` 用生产代码重放，逐位比坐标、半径、offset、随机源
末态与 `missing`。三处改坏（`spawn_radius` / `spawn_safety` /
`spawn_fallback`）全部抓到。旧 `player_life` 场景两边一起把官方
`respawn_radius` 设成 0，重生包字节不变。全量 `mob-oracle` 124 cases /
52851 ticks / 0 mismatches。

官方在超平坦世界**也会散布**（Bamboo 缺省 `GameType.CREATIVE`，不走
ADVENTURE 的 fixup 分支；草方块每列都能站）。因此：

- `tools/mc-bamboo-vanilla/java/Probe.java`：从 `player_position` 读实际落点，
  动态算 `spawnChunk` / `walkTarget` / `expectedSpawnChunks` / 第一批三乘三 /
  区块中心与最近区块，保留全部原有检查；新增 4 项：x、z 在出生点
  ±`respawn_radius` 内，且都是「整数 + 0.5」。
  探针世界用官方格式的 `data/minecraft/game_rules.dat`（由 `seed-world`
  写出）把 `respawn_radius` 显式设成 0，因为声音 / 活塞 / 物品栏 / 工作台
  等十几段在固定坐标摆方块；官方半径 0 的结果就是固定落点，仍走新搜索。
  检查数由 760 增到 764。
- `crates/mc-bamboo-game/src/world/tests.rs` 的所有 `Harness` 与
  `crates/mc-bamboo-server/tests/common/mod.rs` 的 `TestServer` 都显式把
  官方 `respawn_radius` 设成 0（后者通过 `ServerConfig::spawn_radius`），
  断言固定出生位置的测试因此仍逐位一致；缺省规则未改。

### 8.5 已知差异

- 床 / 重生锚的 `findStandUpPosition`、充能扣减、`bad_respawn_point`
  伤害与 `PlayerList.respawn` 末端的重生锚音效未做。
- `dataToKeep` 恒 0（官方末地传送重生传 `KEEP_ALL_DATA`），维度切换未做。
- `PlayerSpawnFinder.noCollisionNoLiquid` 用生成的碰撞形状数据判两格，
  依赖方块实体 / 实体的动态碰撞形状按 `CollisionShapeKind::Other` 处理。
- 官方 `createThreadLocalInstance()` 的 `ThreadLocalRandom` 种子不确定，
  生产同样不确定；对拍才注入固定种子。

---

## 9. 敌对生物地基 + 僵尸（期 2，已合并）

### 9.1 公共接缝（不碰天色计算）

- `BlockView` / `SourceView` 补 `is_bright_outside()`（`skyDarken < 4`）、
  `monsters_burn()`（`EnvironmentAttributes.MONSTERS_BURN`）和
  `is_raining_at(pos)`：并行算阶段从 [`ViewFacts`] 的天气快照、区块表的真实
  高度图和群系调色板算出与精确 `World::is_raining_at` 相同的答案。
  权威副本仍是主控每刻发布的 `GlobalView`；`sky_darken` 怎么算由另一期负责，
  这里只读接口，不改数值来源。
- `BlockGameRules` 增 `spawn_monsters`，`GameRuleSet::block_snapshot` 一起下发；
  `ServerLevel.isSpawningMonsters()` 是 `spawn_mobs && spawn_monsters`。
- 单测 `both_drives_read_the_same_weather_facts` 覆盖雨天/晴天、被地面挡住
  的格子、夜晚与关掉 `MONSTERS_BURN` 的快照；故意改回晴天会让断言失败。

### 9.2 已知差异（本节随实现补齐）

- 尸壳的附加饥饿效果：不接（Bamboo 没有药水效果系统），本体行为、音效和
  战利品表照官方。
- HARD 难度增援、溺尸转化、鸡骑士、僵尸村民转化：后放。
- 村民 / 铁傀儡 / 海龟 / 狼 / 僵尸猪灵目标：Bamboo 没有这些实体，
  `NearestAttackableTargetGoal` 按「类型可配置、缺的跳过」实现；对拍场景明确
  「无此类实体」。
- 自然生成（期 4）不做；生成入口只有 `/summon` 和刷怪蛋。
- 正式游戏里「白天会烧」的端到端测试等时钟/天色那一期合入后补；
  本期白天燃烧在 Java 假世界里注入 `sky_darken` 对拍。

### 9.3 期 2 当前状态与已知差异

- 牛的 52 个场景、玩家 1a/1b/1c 的 65 个场景在泛化后仍逐刻 0 不一致。
- 僵尸场景的第一批（追逐、绕墙、白天/夜晚/雨天燃烧、和平清除、幼年、尸壳、
  远处消失、海龟蛋、两只僵尸同刻打玩家）已接入 `mob-oracle`，前 21 刻逐位一致；
  注册顺序与 `adjustedTickDelay` 修正后追逐场景前 62 刻已逐位一致，
  第 63 刻起的攻击性位/目标停止差异尚未收敛，见 `.agent-notes.md`。
  在收敛之前不能声称这一组场景通过。原有 124 个场景按前缀复跑仍 0 不一致，
  `player-data` 3016 checks / 496 cases、探针两种配置各 764/764 通过。
- 尸壳不接饥饿效果；HARD 增援、溺尸转化、鸡骑士、村民/铁傀儡/海龟/狼目标、
  长矛与穿村行为按字节码保留注册项但本期不建模，写进已知差异。
- 正式游戏的白天燃烧端到端测试等 `day-night` 合入后补；本期不在正式世界计算天色。

### 9.4 期 4：自然生成（2026-11-20）

- **数据**：生成器从官方 `Biome` 的 `NATURAL_MOB_SPAWNS` 环境属性（overlay 到空表上）导出
  每个群系的加权刷怪表与 `spawn_cost`，以及 `MobCategory.values()` 的上限表
  （`Extractor.java` 的 `mobSpawns`；`mc-bamboo-data/src/generated/mob_spawns.rs`）。
  条目顺序就是 `WeightedList.unwrap()` 的顺序，`SpawnerData.count()` 展开成 `min` / `max`
  （官方刷怪表里只有常量和均匀分布，别的类型在提取时直接报错）。
- **`NaturalSpawner`**（`mc-bamboo-blocks/src/entity/mob/natural.rs`）：
  `SpawnState::create` 每刻从本岛实体表重算（跳过持久化生物、按类别计数、刷怪代价、
  每名玩家附近的计数），`filtered_categories` 按全局上限
  （`maxInstancesPerChunk * spawnableChunkCount / 289`）、`spawn_mobs` / `spawn_monsters`
  和 `gameTime % 400 == 0` 的持久类别过滤；`spawn_for_chunk` → `spawn_category_for_chunk`
  → `spawn_category_for_position`：3 组、初始步数 `ceil(nextFloat * 4)`、每步四次
  `nextInt(6)`、加权表 `nextInt(总权重)`、`count.sample`、离玩家/出生点 24 格、
  `ON_GROUND` 的方块/光照/碰撞条件、`noCollision`、`checkSpawnObstruction`、
  `finalizeSpawn` 与组生成。
- **刷怪规则**：牛、猪、羊、鸡和马科走 `Animal.checkAnimalSpawnRules`（`#animals_spawnable_on` + 原始亮度 > 8），
  僵尸走 `Monster.checkMonsterSpawnRules`（`isDarkEnoughToSpawn`：天空光 `> nextInt(32)`、
  方块光上限 0、`maxLocalRawBrightness <= nextInt(8)`，再查可生成方块），
  尸壳额外要求 `canSeeSky`。落脚块用生成器导出的状态位 `facts::VALID_SPAWN`（官方
  `BlockState.isValidSpawn`，用牛问：默认是顶面 `isFaceSturdy(FULL)` 且发光 < 14，树叶、玻璃、
  冰、雕刻南瓜等整个换掉），生成实体还要过 `isValidPositionForMob`：
  `PathfinderMob.checkSpawnRules`（`getWalkTargetValue >= 0`，牛看草方块/亮度，怪物取负的
  光照代价）、`checkSpawnObstruction`（不泡液体、没有实体挡路）。
- **接入**：`crates/mc-bamboo-game/src/natural_spawn.rs` 在 `run_island_simulation` 里
  随机刻之前跑。先收集可 tick 方块且水平距非旁观玩家不到 128 格的候选区块，用
  `Region.random` 打乱，再检查实体 tick 等级。全局上限使用本岛玩家周围 17×17 方形的并集，
  不限制为已加载区块；旁观者仅在 `spectators_generate_chunks` 开启时贡献区块。
- **配置**：`bamboo.toml` 的 `spawn_per_player_cap`，默认开；关掉只剩官方全局上限。
- **对拍**：`mc-bamboo-vanilla natural-spawn` 见下节。
- **已知差异**：（2026-11-20 第二轮起，猪、羊、鸡、兔子、狐狸、狼、马科、骷髅也会真的生成，见 11.5。）
  现在能走实体生成路径的有牛、猪、羊、鸡、马、驴、骡、羊驼、兔子、狐狸、狼、僵尸、尸壳、骷髅
  （猪、羊、鸡 2026-11-20 接入，`mob-oracle --only natural_spawn` 的 `pig_temperate` / `pig_day` /
  `pig_cold` / `sheep_day` / `chicken_day` / `chicken_warm` / `chicken_cold` 和 `gen_spawn` 另比
  花色 / 羊毛颜色和叫声，暖、冷、温带三种花色都覆盖到）；表里其它生物被挑中后照常抽组大小，
  然后结束这一组，不复现官方后续随机数和上限占用，不能保证逐刻产率一致。
  僵尸/尸壳的 `finalizeSpawn` 沿用 9.3 的实现，随机序列和
  官方不同。`doMobSpawning` 为假时整段不跑（官方仍会打乱刷怪区块列表）。区块列表按坐标顺序
  收集而不是官方 `Long2ByteOpenHashMap` 的哈希顺序，官方还在刷怪列表内穿插雷击。
  结构刷怪、刷怪笼、`customSpawners`、下界要塞表未做。生成期刷怪已接入牛、猪、羊、鸡、马科、
  兔子、狐狸和狼；
  装饰随机源决定放置，区域随机源用于 `finalizeSpawn`，用 `mob-oracle --only gen_spawn` 分别比对。
- **对拍**（`mc-bamboo-vanilla natural-spawn`）：`NaturalSpawnOracle.java` 对每个群系的每个
  `MobCategory`，用 7 个固定种子各跑 8 次官方 `WeightedList.getRandom` + `IntProvider.sample`，
  再取一个 `nextInt(1000)` 当随机源指纹；另外把 0..15 交给官方 `Util.shuffle`。
  也直接调用官方 `getRandomPosWithin`（含空列和非平面高度表）与
  `ChunkMap.euclideanDistanceSquared`，比较起点、后续随机指纹与 128 格区块距离条件。
  **没有对拍完整刷怪链路**：`spawnCategoryForPosition` 的方块/光照/距离判定与
  `finalizeSpawn` 仍未做端到端比较。
## 10. 箭实体与骷髅（期 3）

骷髅随敌对生物走同一对选择器。弓目标按视线记忆决定寻路或横移，并把生成箭作为实体命令落地；无弓切换到近战。箭不需要自己的岛表：所有者 UID、装备、插地状态和计时跟随 `Entities`，配对和生成快照仍随 `EntityTracking` / `SpawnSnapshots`。岛重定基只平移近战选择器的游戏时间戳，不改箭的寿命或拉弓计数。精确驱动用当前状态解析命中结果；并行驱动用快照选碰撞候选，落地时才确认伤害。支持范围与已知差异见 README 和 `zombie-skeleton-plan.md`。

## 11. 兔子、狐狸和狼（cx-wild，2026-11-20）

### 11.1 现状

| 生物 | 状态 | 代码 | 对拍 |
| --- | --- | --- | --- |
| 兔子 | 已对拍 0 不一致 | `entity/mob/rabbit.rs`、`avoid.rs` | `mob-oracle --only rabbit_`：闲逛 4 个种子、躲玩家、和平难度不躲、躲僵尸、水里、幼年、细雪、啃胡萝卜，11 例 3240 刻；`loot-oracle --entity-table minecraft:entities/rabbit` 2000 例 |
| 狐狸 | 已对拍 0 不一致 | `entity/mob/fox.rs`、`fox_goals.rs` | `mob-oracle --only fox_`：白天 2 个种子、夜里、屋顶下睡觉、躲玩家、吃浆果、幼年、捕猎兔子 2 个种子、雪层里扑跳，10 例 3493 刻；`loot-oracle --entity-table minecraft:entities/fox` 500 例 |
| 狼 | 已对拍 0 不一致 | `entity/mob/wolf.rs`、`wolf_goals.rs`、`tame.rs`；游戏层 `player_combat_memory.rs` | `mob-oracle --only wolf_`：闲逛 2 个种子、追兔子、骨头驯服 + 染项圈 + 喂肉 + 起身 3 个种子、主人在 14 格外时跟随和瞬移 2 个种子、被打后反击并呼叫同伴（真实玩家受伤链路）、湿身抖水、幼崽讨食，11 例 3790 刻；`loot-oracle --entity-table minecraft:entities/wolf` 500 例。改坏确认：驯服概率改成 1/2 时 `wolf_tame_62` 第 2 刻报出驯服状态和实体事件包 |

### 11.2 新的公共接缝

- `Goal::Avoid(AvoidEntity)`：`AvoidEntityGoal`，`AvoidClass`（玩家 / 狼 / 敌对生物 / 北极熊）加 `AvoidFilter`（缺省排除创造旁观；狐狸的三种）。取点是 `DefaultRandomPos.getPosAway`（`goals::default_random_pos_away`）。
- `Goal::Fox(FoxGoal)`：狐狸专属的目标都在这个变体里，`goals.rs` 只做分派；`Goal::LeapAtTarget`；`goals::melee_attack(speed, followingTargetEvenIfNotSeen, 类名)`。
- `Goal::interruptable`：`WrappedGoal.canBeReplacedBy` 现在看占位目标能不能打断（狐狸扑跳不能）。
- `GoalSelector::running_class_names(kind)`：兔子和狐狸的几个目标是内部子类，对拍按子类名比较。
- `TargetType::FoxPrey` / `Fish`、`goals::nearest_mob_target`：`NearestAttackableTargetGoal` 的生物分支；狐狸和骷髅的 `combat_target` 可以是生物，和平难度只排除玩家目标。
- `Mob::walks_on_powder_snow` 和 `Entity::mob_collision_context`：生物 tick 时被拆出实体，碰撞上下文单独带上能不能在细雪上走；攀爬分支加上细雪那一支。
- `Mob.needs_sync`：所有生物 `jumpFromGround` 都设，实体同步按它发速度。
- `Command::BreedAnimals` 带上父亲的种类、兔子/狐狸品种和求爱者：落地时父亲被拆出来读不到（以前读 `me.kind` 恒为空，繁殖整个失效，`interact_breed` 场景抓到）。
- `BlockView::is_thundering`、`World::spawn_item_with_delay`。
- **可驯服动物（`entity/mob/tame.rs`，给猫、鹦鹉复用）**：`Mob.tame: Tamable`（`DATA_FLAGS_ID` 的坐姿位 1 / 驯服位 4、主人 UUID、`orderedToSit`），`tame::set_tame` / `tame` / `owner` / `unable_to_move_to_owner` / `should_try_teleport_to_owner` / `try_to_teleport_to_owner`（10 次 `nextIntBetweenInclusive` 取落点，`canTeleportTo` 看路径类型、树叶和碰撞），以及 `Goal::Tame(TameGoal)` 里的 `SitWhenOrderedToGoal`、`FollowOwnerGoal`、`OwnerHurtByTargetGoal`、`OwnerHurtTargetGoal`。`TamableAnimalPanicGoal` 的瞬移挂在 `Goal::Panic` 的 tick 上（`tame::is_tamable`）。新种类只要在 `tame::is_tamable` 和 `apply_taming_side_effects` 里加一行，NBT（`Owner`、`Sitting`）、`canAttack` 排除主人、两项元数据（`TameFlags` / `TameOwner`）都是现成的。
- **玩家战斗记忆（游戏层 `player_combat_memory.rs`）**：`Player.combat_memory` 记 `lastHurtByMob`（`#no_anger` 除外）、`lastHurtMob`（`Player.attack` 打中后）和 `getLastDamageSource(100)` 是否不在 `#no_wolf_retaliation` 里，时间戳用玩家自己的 `entity_ticks`；实体逻辑通过 `PlayerView.combat` 读。狐狸的 `DefendTrustedTargetGoal` 以后也可以接它。
- `goals::attack_candidate`：`canAttack` 的公共部分（玩家：非和平、活着、非创造旁观、不是主人；生物：活着）。`goals::target_goal_can_continue`、`goals::max_head_x_rot`（坐着的狼 20）。
- `DamageSource.owner_credit`：驯服的狼咬死的东西记到主人头上（`resolvePlayerResponsibleForDamage`）。
- `Command::WolfAlert`（`HurtByTargetGoal.alertOthers`），`Command::BreedAnimals` 带上父亲的狼状态。
- `LivingEntity.pushEntities` 现在也被重叠的玩家推开（只算生物那一半；服务端对玩家的推力不生效）。`AvoidClass::Wolf` 匹配狼：兔子、狐狸（不躲驯服的）、骷髅（`AvoidEntityGoal(Wolf, 6, 1.0, 1.2)`）都会躲狼。
- 动物的 `MobFlags` 同步带上攻击性位（狼和狐狸的近战目标会设它）。

### 11.3 拆岛 / 并岛

兔子和狐狸的新状态都在 `Mob` 里随实体走（`Mob.rabbit`、`Mob.fox`、目标里的倒计时），不含绝对时钟，拆合不需要平移。狐狸的信任名单和繁殖求爱者都是 UUID。狼的 `anger_end_time` 是岛时钟上的绝对时刻，`Mob::rebase` 里随 `Wolf::rebase` 平移；主人和 `angry_at` 都是 UUID，`angry_at_uid` 找不到时按 UUID 重新匹配。玩家的战斗记忆用玩家自己的 tick 数，整行随玩家走。

### 11.4 已知差异

见 README「兔子的已知限制」「狐狸的已知限制」「狼的已知限制」。对拍环境另有几处：Java 一侧玩家现在在牛、僵尸和野生动物场景里都进实体表（`getEntitiesOfClass(Player)` 和 `getPushableEntities` 才找得到），排在生物前面；僵尸式场景的右键交互和玩家攻击现在两边都重放，交互之后玩家手里换回场景给的那件；兔子死后的掉落物不进 Java 实体表；狐狸的 `SleepGoal` 倒计时在官方构造时用不可复现的种子取，对拍把官方的值报给 Bamboo 照填。

### 11.5 自然生成和生成期刷怪（2026-11-20 第二轮）

- **刷怪规则**（`natural.rs` 的 `check_spawn_rules`）：`Rabbit.checkRabbitSpawnRules`、`Fox.checkFoxSpawnRules`、`Wolf.checkWolfSpawnRules` 都是「脚下方块在各自的标签里 + `isBrightEnoughToSpawn`（原始亮度 > 8）」，共用 `natural::bright_spawn_rules`。狐狸在 `SpawnPlacements` 里登记的是 `NO_RESTRICTIONS`（`spawn_placement_ok` 恒真），兔子和狼是 `ON_GROUND`。`Wolf.getMaxSpawnClusterSize` 是 8。
- **组数据**（`entity/mob/wild_spawn.rs`）：`SpawnGroup::Wild(WildGroup { group_size, pack })`，`pack` 是 `Pack::Rabbit / Fox / Wolf` 带共享的品种。`RabbitGroupData`：每只都先按群系抽一次 `nextInt(100)`，有组数据时用组里的品种；幼年概率 1.0，第二只起 `nextFloat() <= 1.0` 照取、都是幼崽。`FoxGroupData`：品种跟第一只，组里已有两只时是幼崽，`AgeableMob` 那层不取数；然后 `setTargetGoals` 和嘴里的东西。`WolfPackData`：品种只在没有狼群数据时选（`nextInt(1)`），叫声每只 `nextInt(7)`，不生幼崽。最后都走 `Mob.finalizeSpawn` 那层（跟随距离、左撇子）。`/summon` 和刷怪蛋传空的组数据，行为和以前一样。
- **生成期刷怪**（`natural_gen.rs`）：放置表加了兔子、狼（`ON_GROUND`）和狐狸（不查落脚点），规则换成各自的标签；`finalize_generated` 用区域随机源跑 `wild_spawn::finalize`。
- **`MobType::by_entity_type`** 以前只列了牛和三种怪物，所以猪、羊、鸡、兔子、狐狸、狼即使接好了规则也不会被自然刷出来；现在列全了。
- **捕食关系**：`Wolf.PREY_SELECTOR` 加上羊；狐狸的猎物 / `STALKABLE_PREY` / `FoxAlertableEntitiesSelector` 加上鸡（`fox_goals::is_small_prey`）。
- **对拍**：`mob-oracle --only gen_spawn` 的场景带上群系（平原 4 个 + 针叶林 3 个 + 雪地针叶林 1 个，共 7000 例），所有建模了的生物比类型、坐标、朝向，区域随机源同步时再比幼年、左撇子和 `genSpawnDetail`（兔子品种、狐狸品种和嘴里的物品、狼的品种和叫声）：1088 只里 34 只兔子、109 只狐狸、126 只狼，0 不一致。对拍还要求在区域随机源同步时，每种农场动物都至少有一对生成品种匹配（并比较适用物种的叫声）；本轮牛、猪、羊、鸡分别匹配了 156、194、232、192 只。`--only natural_spawn` 加了猪 / 羊 / 鸡（平原）、兔子 / 狐狸 / 狼（针叶林、雪地针叶林）、森林的狼和夜里的狐狸，整组 252 例 1008 刻 0 不一致（比类型、位置、朝向、幼年、左撇子、主手物品和每刻之后的世界随机源种子）。改坏确认：把狐狸幼崽的门槛从「组里已有两只」改成一只，`gen_taiga_flat` 报出幼年不一致。
- **对拍夹具的一处修正**：僵尸式场景里玩家的实体 ID 和第一只生物都是 `FIRST_ID`。Bamboo 用同一套号认目标，`fox_sleep_wild_wolf` 里狼恨的是狐狸、却把同号的玩家认成了仇恨对象（第 13 刻随机源差一次取数）。Java 一侧玩家改成 `FIRST_ID + 1024 + 下标`，全量 `mob-oracle` 439 例 72339 刻 0 不一致。真实游戏里非玩家实体从 `FIRST_ENTITY_ID`（`1 << 24`）起发号、玩家用它下面的号，不会撞号。

### 11.6 捕食关系的逐刻对拍（第三轮）

- **场景**：`fox_hunt_chicken`（狐狸盯梢鸡、扑跳咬中，鸡受伤后惊慌逃跑）和 `wolf_hunt_sheep`（野狼把羊当猎物，羊挨打后逃跑）。夹具里羊是 kind 4、鸡是 kind 5，和 Rust 一侧的编号一致；`MobOracle.java` 的 `case` 分支加了这两种生物。
- **夹具补的两列**：僵尸式场景的 `var` / `snd` 原来写死成 `minecraft:temperate` / `minecraft:classic`（牛、兔、狐、狼都吃这两个默认值）。加了羊和鸡以后改成 `caseVariant` / `caseSound`：羊报「颜色|是否剪毛」、鸡报「品种|egg计时」和叫声变种，其余仍是默认值。鸡的 `eggTime` 在构造时抽取，Java 一侧在场景起点 JSON 里报 `egg`，Bamboo 照填（缺省时保留自己的值，免得旧场景被填成 0 立刻下蛋）。
- **改坏确认**：把 `wolf_goals::is_prey` 里的羊去掉、`fox_goals::is_small_prey` 里的鸡去掉，两个场景分别在第 60 刻和第 14 刻起报不一致；改回后 0 不一致。
- **没做**：两只狼的繁殖场景。夹具只比场景里原有的生物，幼崽不在比对范围里；试做的一个版本从第 162 刻起位置发散（官方停在同伴旁边、Bamboo 继续走了约 0.15 格），原因没查，没提交。

### 11.7 与马科合并后的现状

- 运行时与生成期均保留牛、猪、羊、鸡、兔、狐、狼与马科规则。`finalize_cow(kind, …)` 继续分派农场动物；野生动物和马科分别维护各自的组数据，生成期统一使用区域随机源。
- 生成期对拍同时校验野生动物的品种 / 叼物 / 叫声和马科属性；逐刻夹具给兔 / 狐 / 狼保留 6–8 号，马 / 驴 / 骡 / 羊驼使用 9–12 号。场景名称不变。
- 此次合并没有实现狼与羊驼之间的躲避 / 主动攻击，也未扩展马科骑乘。
- 2026-11-21 再次合并 `main`（苦力怕 / 蜘蛛 / 洞穴蜘蛛 / 末影人、水生生物、世界生成缓存）后复核农场动物仍 0 不一致：`mob-oracle --only pig` 5 例 348 刻、`--only sheep` 10 例 1300 刻、`--only chicken` 3 例 255 刻、`--only egg` 11 例 149 刻、`--only natural_spawn` 306 例 1224 刻；三种 `loot-oracle` 各 200 题、`chunk-content` 488 项均通过。此次合并未改动农场动物的代码与上述现状。
- 2026-11-21 再合并 `main`（op-misc：哞菇 / 熊猫 / 北极熊）后复核农场动物仍 0 不一致：`mob-oracle --only pig` 5 例 348 刻、`--only sheep` 10 例 1300 刻、`--only chicken` 3 例 255 刻、`--only egg` 11 例 149 刻、`--only natural_spawn` 318 例 1272 刻、`--only gen_spawn` 7000 例；三种 `loot-oracle` 各 200 题，生成期 `--only gen_spawn_misc` 128 例 / `--only gen_spawn_panda` 128 例均 0 不一致。`chunk-content` 495 项里农场动物与牛的存档用例全通过，仅 `polar bear with persistent anger` 一项报「官方无法载入该实体」：该用例及官方加载器与 `main` 逐字相同，是随本次合并带入的 op-misc 北极熊夹具历史问题，与农场动物无关（详见汇报疑点清单）。此次合并只解决公共文件的冲突，未改动农场动物代码。
- 2026-11-21 合并 `main`（`2448b76c`：compute pool / IO 线程 / 岛 / staged pipeline 的计时测试确定性，仅动 `mc-bamboo-game` 的测试与 `chunk_io.rs`，与农场动物代码零重叠）后复核农场动物仍 0 不一致：`mob-oracle --only pig` 5 例 348 刻、`--only sheep` 10 例 1300 刻、`--only chicken` 3 例 255 刻、`--only egg` 11 例 149 刻、`--only natural_spawn` 318 例 1272 刻、`--only gen_spawn` 7000 例；三种 `loot-oracle` 各 200 题 0 不一致。`cargo check --workspace --all-targets` 通过。`chunk-content` 495 项里农场动物与牛的存档用例全通过，仅 `polar bear with persistent anger` 一项仍报「官方无法载入该实体」，是第十五轮合并带入的 op-misc 北极熊夹具历史问题，与农场动物无关。此次为无文本冲突合并，未改动农场动物代码。
- 狼躲羊驼（`Wolf.WolfAvoidEntityGoal`：未驯服的狼按羊驼 `getStrength() >= nextInt(5)` 判定躲不躲，
  仍先跑 `super.canUse()` 取逃跑点）、羊驼打野狼（`LlamaAttackWolfGoal`：`0.25 × FOLLOW_RANGE` 内
  看得见的最近野狼，`canContinueToUse` 同样按 0.25 倍跟随距离）和「保护主人时排除驯服的马科」
  （`Wolf.wantsToAttack`）于 2026-11-21 接入。三条只过 `wolf_` / `zombie_equine` 等回归对拍、
  没有专属逐刻场景。马科骑乘此后未再扩展。

## 猪的实施现状

猪沿用 Animal 的实体表与岛迁移路径，专属目标、属性和音效位于
`entity/mob/pig.rs`。钓竿诱惑和食物诱惑是同一优先级的两个独立目标，
按官方注册次序争用移动和观察标志。鞍存为 LivingEntity 的装备槽，
不引入猪专属装备 NBT；玩家装上的鞍使用保证掉落概率。
载具和乘客的同组 tick 尚未实现（见世界模型设计第 7 段），所以
上鞍不会启用骑乘或加速，未新增玩家输入或跨岛载具规则。

## 羊和鸡的实施现状

羊的专属逻辑在 `entity/mob/sheep.rs`：羊毛字节（`DATA_WOOL_ID`）存在 `Mob.wool`，`EatBlockGoal` 进同一个目标选择器（非每刻目标，`adjustedTickDelay` 折半），改方块走 `Command::EatBlock`，羊自己的 `ate` 当场执行。剪毛在 `World::interact_mob` 里落地：玩家音源的剪毛声、`shearing/sheep` 表（游戏层 `interaction/level/farm_loot.rs` 给实体谓词补 `sheep/color` 组件和 `type_specific/sheep`）、每团羊毛一个物品实体加五个羊随机数的速度，最后用 `durability::hurt_hand` 扣剪刀耐久（游戏层在交互期间把手借给方块逻辑）。繁殖混色按生成的合成配方表查两格染料（`findColorMixInRecipes`），混不出时取世界随机源。

鸡在 `entity/mob/chicken.rs`：构造函数在 UUID 和朝向之后抽 `eggTime`；`aiStep` 尾部做缓慢下落和下蛋，下蛋表经 `Command::DropEntityLoot` 在落地时求值（礼物参数集：原点 + 实体本身，鸡花纹进 `chicken/variant` 组件）。剪毛和下蛋共用 `Level::entity_loot_drops` / `EntityLootRequest`，以后的雪傀儡、哞菇也能复用。

生物装备扩成官方全部八个槽（含身体和鞍），动物死亡统一走「战利品表 → 按槽掉装备 → `Animal` 经验」。逐刻对拍场景：`pig_*`、`sheep_*`、`chicken_*`（`mob-oracle --only`），存档格式见 `chunk-content` 的 fresh 实体对拍。

扔出的鸡蛋在 `entity/thrown_egg.rs`，是新的实体种类 `EntityKind::Egg`（不是生物）：`ThrowableProjectile.tick` 先重力后阻力，沿速度找方块（含世界边界）和实体命中；移动段穿过水时用共用的流体判定熄火，灭火声的音高取蛋自己的随机源；命中经 `Command::EggImpact` 落地，打目标 0 点 `thrown` 伤害（新增 `DamageKind::Thrown`），再用蛋自己的随机源 `nextInt(8)` / `nextInt(32)` 孵一或四只小鸡。小鸡构造（种子、UUID、朝向、`eggTime`）在 `prepare_entity` 里取，`snapTo` 后照 `fudgePositionAfterSizeChange` 从零尺寸找空位（`findFreePosition` 用盒子切格子求最近点）。实体事件 3 走生物事件通道，蛋当刻消失时由 `keep_departed_mob_events` 留给同步层（并行驱动在命令落地后也会留）。玩家入口是游戏层 `player_egg.rs`（`EggItem.use`），生成后按手中蛋的种类累加会话内使用统计，再按模式消耗；发射器在 `dispense/entity.rs`。对拍：`mob-oracle --only egg`（飞行、穿水熄火、孵一只、孵四只、打牛、玩家扔蛋、发射器，逐刻比位置、随机源、着火状态、出站包、小鸡和牛，玩家扔蛋另比 `ITEM_USED`），存档见 `chunk-content` 的两枚蛋。

## 流浪者和沼骸的实施现状（2026-11-20）

两者都是 `AbstractSkeleton`：`MobType::is_skeleton()` 把骷髅原有的分支（弓 / 近战目标的重选、
死亡掉落、`finalizeSpawn`、日晒）一起放开，专属部分各一个文件。

- `entity/mob/stray.rs`：箭的效果（缓慢 600 刻）和 `Stray.checkStraySpawnRules` 的找天。
  `MobType::immune_to()` 是 `EntityType.Builder.immuneTo` 的方块标签，自然刷怪的
  `isBlockDangerous` 先查它，所以流浪者能刷在细雪里。
- `entity/mob/bogged.rs`：生命 16、射击间隔 70 / 50、箭的效果（中毒 100 刻）、剪蘑菇。
  剪蘑菇和羊共用 `MobInteraction::shear` 与 `World::interact_mob`，按生物种类分派到
  `bogged::apply_shear`；`Bogged.DATA_SHEARED` 存在 `Mob.bogged_sheared`。
- 箭的效果随 `Command::ShootMobArrow { effect }` 带到落地阶段，`skeleton::add_arrow_effect`
  照 `Arrow.addEffect` 写进拾取物品的 `potion_contents` 自定义效果；命中后的效果、箭的颜色
  都从这个组件算，不需要箭上另加字段。
- **公共机制：生物转化**（`entity/mob/conversion.rs`，别的生物可以直接复用）。
  `ConversionTracker` 是官方同名类的计数（受影响计时、转化倒计时、同步布尔）和存读盘；
  `Command::ConvertMob { from, to, level_event }` 落地 `Mob.convertTo(type,
  ConversionParams.single(..))`：新实体走和 `/summon` 一样的构造取数，再按
  `ConversionType.SINGLE.convert` + `convertCommon` 搬状态。发起者在自己的 tick 里 `discard`，
  这一刻剩下的 `baseTick` 照跑、`aiStep` 不跑（官方 `LivingEntity.tick` 的 `isRemoved` 判断）。
  僵尸溺水变溺尸在官方是同一个 `ConversionTracker`，接的时候只需要新的受影响条件和参数。
- `LivingEntity.canFreeze()`：`living::can_freeze` 看盔甲槽的 `#freeze_immune_wearables` 和
  类型（骷髅覆写成假，流浪者在 `#freeze_immune_entity_types`）。细雪的 `FREEZE` 效果在
  不能冻时只置「在细雪里」，`aiStep` 照常每刻减 2、不受冻伤。
- 元数据：骷髅的 `DATA_STRAY_CONVERSION_ID` 和沼骸的 `DATA_SHEARED` 都是 index 16 的
  BOOLEAN，同步层复用 `EntityData::Baby`（字节相同），由 `MobPublic.skeleton_flag` 给值。
- **岛的拆合**：新增的 `Mob.freeze_conversion`（两个相对计数 + 一个布尔）和
  `Mob.bogged_sheared` 都在实体行上，不含绝对时刻，整行搬走即可；`ConvertMob` 在同一刻内
  落地，不跨刻。没有新增 `IslandPart`，没有改 `region/island_split.rs`。
- 顺手修的一处：`NearestAttackableTargetGoal` 找玩家时没有排除能力里 `invulnerable` 的玩家
  （官方 `TargetingConditions.test` → `canAttack` → `Player.canBeSeenAsEnemy`）。以前目标
  选择器会为创造模式玩家启动并锁住 `TARGET` 标志（攻击目标另有过滤，所以不会真的打），
  随机源的取数次数因此和官方错开；现在在测距离和视线之前就跳过。
- 对拍（`mob-oracle --only projectile`，37 个场景 1346 刻 0 不一致）新增 5 个场景：
  `stray_bow_normal`、`bogged_bow_normal`、`bogged_bow_hard`（逐刻比射手和箭的位置 / 速度 /
  随机源、箭的配对包字节（含颜色）、玩家血量和玩家身上的效果）、`skeleton_freeze`（细雪井里
  450 刻：转化标志、冻结刻数、转化后的实体类型 / 编号 / 血量 / 主手、1048 号世界事件）、
  `bogged_shear`（两次剪刀右键：结果、掉落物的物品 / 位置 / 速度、已剪标志）。官方一侧让
  真的 `Stray.getArrow` / `Bogged.getArrow` 造箭，再把它加的效果搬到带种子的箭上。
  改坏确认：中毒时长改成 101、受冻刻数改成 141，两个场景各自报不一致。
- 自然刷怪对拍（2026-11-21）：`mob-oracle --only stray_spawn`（28 例 112 刻 0 不一致；
  `stray_night` 雪地针叶林找天正例刷 90 只，`stray_no_sky` 无天、`stray_peaceful` 和平各 0 只）
  和 `--only bogged_spawn`（12 例 48 刻 0 不一致；`bogged_night` 沼泽正例 48 只、
  `bogged_peaceful` 0 只）。夹具把 `sky=15` 注入成 `canSeeSky`，逐刻比类型、位置、朝向、
  幼年、左撇子、主手和每刻世界随机源种子；`Stray.checkStraySpawnRules` 的「从上一格起跳过
  连续细雪再找天」已和官方字节码核对（26.3 的 `Stray.java` 用 `do..while` 上抬一格，等价）。
  改坏确认：`--only stray_spawn --break-self-test natural_spawn` 把 Bamboo 正例的找天判据拿掉，
  官方仍刷而 Bamboo 不刷，报 4 处不一致。并入 `--only natural_spawn` 后 566 例 2264 刻
  0 不一致，原有牛 / 马科 / 史莱姆 / 溺尸 / 女巫场景无回归。夹具的 `canSeeSky` 是注入常量，
  细雪上方连续跳过的那一支无法单独区分，仍记在已知限制。
- 未对拍：死亡掉落（走骷髅同一条路径，战利品表由 `mc-bamboo-loot` 求值）、存档 NBT 和
  官方的逐键比较、转化时带效果 / 名字 / 标签的搬运。

## 史莱姆的实施现状（2026-11-20）

史莱姆是 `AbstractCubeMob` → `Slime`：继承 `AgeableMob`（所以有 `Age` 等三个存档键和幼年 /
年龄锁两项元数据），只实现 `Enemy`，不是 `Monster`。`MobType::is_enemy()` 把「`Monster` 一支
加史莱姆」单独分出来，管和平难度清除、128 格消失、`MobCategory.MONSTER` 和敌对音源；
`is_monster()` 仍只表示 `Monster` 类（亮处涨 `noActionTime`、僵尸的幼年元数据等）。

- `entity/mob/slime.rs`：全部专属逻辑。`CubeState`（`Mob.cube`）放大小、`wasOnGround` 和
  `CubeMobMoveControl` 的三个字段，都是相对量，随实体行搬岛，没有新增 `IslandPart`。
- 尺寸：`Mob::dimensions()` / `MobPublic::dimensions()` 对史莱姆按大小算，`set_size` 同时改
  `Entity.dimensions`、眼高、三项属性的基础值；大小真的变了才把朝向改成头的朝向
  （`onSyncedDataUpdated`）。眼高和元数据下标（18）由生成器导出
  （`entity_types::SLIME_EYE_HEIGHT` / `CUBE_MOB_SIZE`）。
- 移动：`living::server_ai_step` 对史莱姆改调 `slime::move_control_tick`，`jumpFromGround` 改调
  `slime::jump_from_ground`（竖直速度直接设成跳跃力，并要求同步一次速度）。`tick` 末尾的
  `slime::after_tick` 做落地判断：按宽度取粒子的随机数、放落地音效。
- 目标：`Goal::CubeFloat` / `CubeAttack` / `CubeRandomDirection` / `CubeKeepJumping`，注册顺序和
  优先级照官方（1、4、5、2）。目标选择器是玩家（间隔 5、要看得见、高度差不超过 4）和铁傀儡
  （缺实体，照常取数）。`TargetGoal.canContinueToUse` 的 `mustSee` 计数和骷髅共用一支。
- 碰撞伤害：`slime::player_touch` 是 `AbstractCubeMob.playerTouch` 的全部。游戏层在
  `drops::pickup_before_entities` 里（官方 `Player.aiStep` 的 `touch(entity)`，和捡物品同一处）
  对玩家包围盒外扩 (1, 0.5, 1) 碰到的史莱姆各调一次，用带玩家那一半的世界当场走玩家受伤的
  正式入口。
- 分裂：`living::tick_death` 在第 20 刻移除之前调 `slime::split_on_remove`，数量和每只的朝向
  取母体的随机源，随 `Command::SplitCubeMob` 落地；每只走 `/summon` 一样的构造取数，再
  `conversion::copy_common`（`ConversionType.SPLIT_ON_DEATH` 只跑 `convertCommon`）、
  `setSize(half, true)`、`snapTo`。和平难度不分裂（`EntityType.create` 返回空）。
- 生成：`slime::finalize_spawn`（`AgeableMobGroupData(false)`、跟随距离加成、左撇子、
  `setSpawnSize`）；`slime::check_spawn_rules` 是 `Slime.checkSlimeSpawnRules` 的两支，
  `Level::world_seed()`（新增，游戏层给 `global.seed`）给史莱姆区块，
  `sky::surface_slime_spawn_chance` 是月相时间线的那条轨道。生成盒乘
  `spawnDimensionsScale`（4）。
- 掉落：`MobDeathLoot.cube_size` 进战利品谓词的 `cube_mob_size`；经验等于大小。
- **公共机制**（别的生物可以复用）：`conversion::copy_common`（`convertCommon`，带
  `preserveCanPickUpLoot`）、`Level::world_seed()`、`MobType::is_enemy()`、
  `CombatTarget::hitbox()`，以及按方块音效类型放脚步声的写法（`motion::step_effects`）。
- 对拍：
  - `mob-oracle --only slime_`：13 个场景 2063 刻 0 不一致。`slime_hop_{1,2,4}`（自己乱跳）、
    `slime_chase_{1,2,4}`（真实玩家：找目标、追、碰撞伤害、玩家被打死时的死亡消息包）、
    `slime_two_touch`、`slime_target_height`（高度差和视线记忆）、`slime_water`、
    `slime_split_{1,2,4}`（分裂出来的只数、位置、朝向、大小、血量）、`slime_peaceful_clear`。
    逐刻比位置、速度、朝向、血量、随机源、正在跑的目标、目标实体，以及大小 / 跳跃延迟 /
    移动控制的目标角度 / `wasOnGround`。官方一侧的 `touch(entity)` 由对拍自己重放（玩家在
    场景里不 tick）；分裂出来的小史莱姆记完就丢，不继续 tick。
  - `mob-oracle --only natural_spawn`：新增 8 个史莱姆场景（史莱姆区块的普通 / 困难 / 和平、
    非史莱姆区块、沼泽的满月 / 亏凸月 / 新月 / 白天），共 390 例 1560 刻 0 不一致，其中 100 只
    史莱姆。世界种子用官方 `WorldgenRandom.seedSlimeChunk` 挑。大小不是 1 的史莱姆朝向来自
    官方不带种子的构造随机源，两边都不比。
  - 改坏确认：跳跃延迟 `+10` 改成 `+11`（`slime_hop_1` 第 4 刻起报不一致）、分裂的 z 偏移
    `0.5` 改成 `0.25`（`slime_split_2` / `_4` 第 24 刻报不一致）。
- 顺手修的一处（影响骷髅一支）：`tick_goal` 里给 `NearestAttackableTarget` 每刻多加了一次
  `unseen_ticks`，官方 `TargetGoal` 没有 `tick()`。以前看不见目标约 30 刻就放弃，现在是 60 刻。
- 未对拍：死亡掉落（粘液球）和经验球的生成、存档 NBT 和官方逐键比较、元数据包字节、
  `/summon` 和刷怪蛋、并行驱动下的逐位一致。
- 没做：岩浆怪；水里改大小时的溅水（只保住那一次取数）；脚步声的
  `getPrimaryStepSoundBlockPos`；`push(Entity)` 打铁傀儡；附魔的 `doPostAttackEffects`。
### 5.8 溺尸、女巫和喷溅药水（2026-11-21）

- **溺尸**（`entity/mob/drowned.rs`）：`MobType::Drowned` 接进属性、目标、音效、刷怪和存档表；
  `is_zombie()` 把僵尸、尸壳、溺尸归成一族，近战、破门、增援、日晒和 `handleAttributes`
  继续共用。`Zombie.tick` 结尾的 `drowningTracker`（`Mob.drown_conversion`）眼睛在水里
  600 刻后开始转化，再过 300 刻发 `Command::ConvertMob`；落地时照 `ConversionType.SINGLE`
  搬状态（共享的是 `conversion::copy_state`），并额外跑一次 `Zombie.handleAttributes`
  （`Command::ConvertMob.zombie_attributes`）。
- **两栖寻路**（2026-11-21 重做）：`Navigation` 加了 `EvaluatorFlags::amphibious`，溺尸走
  `AmphibiousPathNavigation` 的规则：`canUpdatePath` 恒真、`getTempMobPos` 取身体中间、
  `getGroundY` 原样返回节点高度、`isStableDestination` 只要求脚下不是空气、
  `setCanFloat` 不生效（所以 `jumpInLiquid` 是 0.3 而不是浮水生物的 0.04）。
  `Search` 里照 `AmphibiousNodeEvaluator`：水里贴着 `BLOCKED` 的格子算 `WATER_BORDER`
  （代价 4），水里多出上下两个竖直水邻格（`isVerticalNeighborValid` 只收 `WATER`），
  求值器起点在水中换成碰撞盒最小角、`isAmphibious` 让 `findAcceptedNode` 不再把水节点
  换成水下地面。代价覆盖照官方只在一次搜索期间生效：`Drowned` 身上只有水 0；
  `AmphibiousNodeEvaluator.start()` 搜索期间把走地改 6、水边改 4，`done()` 还原
  （Bamboo 在求值器的 `malus` 里按 `EvaluatorFlags::amphibious` 覆盖这两项）。
  2026-11-21 的 `main` 合并后这条被发现曾把三项都常驻在 `Mob::malus` 上，导致
  `GoalUtils.hasMalus` 拒绝所有草地落点、溺尸打完架后一直无法随机漫游；改成搜索期覆盖后
  `drowned_flat_chase_3` 从 9 处不一致回到 0。
  上次报告里「平地上 `createPath` 返回空」是误判：白天 `Drowned.okTarget` 本来就不选玩家，
  夜间场景下原来的僵尸近战逻辑能正常寻路；真正缺的是 `AmphibiousPathNavigation.canUpdatePath`
  恒真和 `Drowned.closeToNextPos` 量的是终点格子的角而不是格心，两处都已修。
  `DrownedGoToBeachGoal`（`MoveToBlockGoal`）也补上了：倒计时取 `nextInt(200)`、
  方环搜索岸边、`start` 两次 `nextInt(1200)`、`tick` 每 40 刻重算。
  `PathNavigation.shouldTargetNextNodeInDirection` 在几何折角检查前，照官方对液体中的两栖
  生物作一次忽略流体的碰撞射线：从临时位置（身体中点）到当前路径节点的身体中点，
  无阻挡即可推进节点。
- **溺尸对拍**：`mob-oracle --only drowned_`（平地追击 120 刻、水中追击 40 刻、上浮 40 刻）
  3 例 200 刻 0 不一致，逐刻比较位置、速度、路径节点及索引、游泳标志和随机源。
  尚未覆盖跨水面、流动水和上岸。
- **海平面**：`Level::sea_level()` 和 `BlockView::sea_level()` 取代溺尸文件里手写的 63；
  游戏层按 `WorldGenConfig.level_type` 给噪声世界 63、超平坦 -63，`mob-oracle` 的假世界
  跟官方的 `-63` 一致，场景可以用 `Scene.sea` 覆盖。`DrownedGoToWaterGoal` 的白天判断
  （`!isBrightOutside`）、`DrownedSwimUpGoal` 不锁标志位、铁傀儡 / 美西螈目标的
  `nextInt(reducedTickDelay(10))` 已照官方改正。
- **游泳**：`updateSwimming`（共享标志第 4 位）、`DrownedMoveControl`（`mob.swimming` /
  `searching_for_land` / 目标高度）、`travelInWater` 的 0.01 分支、`isPushedByFluid` 为假时
  不推水流。`updateSwimming` 和 `travelInWater` 读 `Entity.isUnderWater` 的缓存眼部状态
  和当前身体入水状态，而不是本刻扫描到的眼部状态；`DrownedMoveControl` 保持用 `f32`
  插值速度，再按 Java 数值提升规则用 `f64` 计算位移增量。
- **生成**：`SpawnPlacementTypes.IN_WATER` 放置、`checkDrownedSpawnRules` 的 1/40（
  `#more_frequent_drowned_spawns` 1/15）和海平面 5 格、3% 副手鹦鹉螺壳、三叉戟 / 钓鱼竿
  的 `populateDefaultEquipmentSlots`（不拿盔甲）、`handleAttributes`、`entities/drowned`
  战利品。照 `Drowned.checkSpawnObstruction`，实例生成阶段检查实体遮挡但不因碰撞盒内有液体而拒绝。
  `mob-oracle --only drowned_spawn`：固定区块 (0,0) 与 (1,0)，4 种水域／光照条件，
  34 例 136 刻 0 不一致。深海生成 2 只、浅河 13 只；浅海和强光浅河为 0。比较
  类型、位置、朝向、幼年、左撇子、双手装备和每刻世界随机源。未覆盖真实混合刷怪表；
  鹦鹉螺僵尸骑手只取触发随机数、不生成坐骑。
- **溺尸战利品表对拍**：`loot-oracle --entity-table minecraft:entities/drowned --cases 64 --seed 3`，
  64 题实际比较，0 排除、0 不一致；只验证表求值与随机源后继值，不涵盖装备掉落和死亡流程。
  离线夹具提供 `ServerLevel.getSeaLevel`，供构造函数注册上浮目标时调用。
- **女巫**（`entity/mob/witch.rs`）：`MobType::Witch` 接进各表；`Witch.aiStep` 在
  `Monster.aiStep` 之前跑喝药逻辑和 7.5e-4 的冒烟事件，`WitchHealRaiders` 目标在没有袭击时
  仍每刻抽一次 `nextBoolean`，`RangedAttackGoal` 用 `Goal::WitchRangedAttack` 实现。
  喝完用 `PotionContents` 加效果（读取物品的持续时间倍率）；瞬时治疗进入效果表，在效果刻回血。
  `LivingEntity.hurtServer` 会先拒绝抗火生物收到的火焰伤害，不改受伤冷却或攻击者归因。
  喝药移速修饰符是瞬时的 `minecraft:drinking`；入水的 `FloatGoal` 会打开导航的浮水标志。
  同步 index 17 的 `Witch.DATA_USING_ITEM`（`EntityData::WitchUsingItem`）。自然生成是
  `ON_GROUND` + `Monster.checkMonsterSpawnRules`；`PatrollingMonster.finalizeSpawn` 会先掷一次
  巡逻队长概率，即使女巫不能当队长，也必须消费该随机数。
  `mob-oracle --only witch_spawn`：平原黑暗、方块光 1、和平，18 例 72 刻 0 不一致，生成 30 只。
  `Witch.performRangedAttack` 的选药五支（袭击者按血量给治疗 / 再生、缓慢、剧毒、虚弱、瞬间伤害，
  短路顺序和官方一致，只有虚弱一支取 `nextFloat`）和按目标速度的提前量照官方实现；
  `Witch.getDamageAfterMagicAbsorb` 现在也把 `indirectMagic` 算进 15% 抗性；
  `mob-oracle --only witch_throw` 逐刻对拍（3 例 553 刻 0 不一致）。
- **喷溅药水**（`entity/splash_potion.rs`）：新的投掷物实体，物理照 `ThrowableProjectile`
  （重力 0.05）。命中后按 `ThrownSplashPotion.onHitAsPotion` 的 4 格水平 / 2 格竖直范围、
  `computeMargin`（`clamp((tickCount - 2) / 20, 0, 0.3)`）和距离缩放分效果：瞬时治疗 / 伤害按
  `(int)(scale * base + 0.5)` 取整并对 `#inverted_healing_and_harm` 的亡灵反转，其余按
  `scale * duration * durationScale + 0.5` 挂上（20 刻以内不挂），跳过带免疫标签的效果。命中实体
  不打额外的 0 点 `thrown` 伤害，改用 `DamageSources.indirectMagic` 做击退；世界事件
  2002 / 2007（有无瞬间效果）和 1053 / 1054。选药、提前量、亡灵反转、时长取整和等级事件都和官方
  逐刻对拍（`mob-oracle --only witch_throw`：3 例 553 刻 0 不一致）；存档、同步和
  `entity-oracle` 的快照都接上了。
- **仓库外改动**：`mc-bamboo-protocol` 加了一个实体数据项；`mc-bamboo-storage` 的建模实体
  清单加了溺尸和女巫。
- **没做的**：三叉戟投掷（没有 `ThrownTrident`）、鹦鹉僵尸（僵尸骑鹦鹉螺）、袭击、
  女巫小屋刷怪、喷溅药水的灭火 / 美西螈 / `onHitBlock`（营火 / 蜡烛）、滞留药水云、
  喷溅水瓶对末影人的伤害。溺尸的对拍及未覆盖的水面、上岸边界见上文和 README。女巫四种自饮药水由
  `mob-oracle --only witch_drink` 逐刻比较位置、血量、效果时长、喝药状态和随机源
  （3 例、268 刻、0 不一致）；投掷由 `mob-oracle --only witch_throw` 逐刻对拍
  （3 例 553 刻 0 不一致）。
## 哞菇的实施现状（op-misc，2026-11-20）

哞菇是 `MobType::Mooshroom`，属性、目标选择器、叫声全部复用牛（`AbstractCow`），自己的部分在
`entity/mob/mooshroom.rs`：品种 / 炖菜效果 / 上一道闪电存在 `Mob.mooshroom`（跟实体行走，岛拆合不需要
平移时钟），交互三个分支（碗、剪刀、棕色吃花）排在 `AbstractCow.mobInteract` 之前，`getWalkTargetValue`
对菌丝给 10，生成规则是 `checkMushroomSpawnRules`。花对应的炖菜效果表 `STEW_FLOWERS` 和 `DATA_TYPE` 的
index 由生成器从 `SuspiciousEffectHolder` / `MushroomCow` 导出。

剪毛走新的公共模块 `entity/mob/conversion.rs`（官方 `Mob.convertTo` + `ConversionType.SINGLE`）：先
`prepare_entity` 构造牛（拿 ID 和种子，和官方 `EntityType.create` 同一时刻），抄状态，回调里掉蘑菇
（`World::spawn_fresh_item`：直接 `new ItemEntity`，拾取延迟 0），牛进世界，哞菇丢弃。以后僵尸转化、
猪被雷劈可以复用；乘客 / 载具 / 拴绳 / 队伍 / `Brain` 记忆没有建模。

`MobInteraction` 多了 `replacement_limit_creative`（`createFilledResult` 的 `limitCreativeStackSize`）。游戏层
有交回物品时不再另扣手里的物品：`createFilledResult` 自己消耗一个，原来挤奶时手里一叠桶会多扣一个。

`MobType::ALL` 统一了按名字 / 注册表 ID 的查找；猪、羊、鸡、哞菇因此进入运行时自然生成和生成期刷怪
（之前 `by_entity_type` 只列了牛和三种怪物）。`can_spawn_far_from_player` 改成按类别（`CREATURE` 为真）。

对拍：`mob-oracle --only mooshroom`（接炖菜、挤奶、喂花、迷之炖菜、剪成牛红 / 棕、同色繁殖红 / 棕、幼年），
逐刻比位置、随机源、出站包、交互结果和新生实体，7 例 498 刻 0 不一致；`chunk-content` 比新生哞菇和
带 `Type` / `stew_effects` 的读写往返。对拍工具里「交互生成的实体」改成在实体 tick 之前收集（真服务端里
它们这一刻就会 tick），新生生物多带一个身体朝向 `yr`。

## 北极熊与熊猫的补充对拍（op-misc，2026-11-21）

- 北极熊的 `NearestAttackableTargetGoal<Fox>` 在抽取间隔随机数后才过滤幼崽；成年熊扫描附近活着且可见的狐狸。`mob-oracle --only polar_bear_fight_fox` 分别比较成年和幼崽的目标与逐刻状态。狐狸反向躲熊尚未接入。
- `mob-oracle --only natural_spawn_panda` 从原版丛林表固定熊猫条目，比较亮度 15 / 8 下逐刻的刷出数量、位置、双基因和世界随机源。
- `mob-oracle --only gen_spawn_panda` 用竹林完整动物表，比较位置、朝向、双基因和随机源指纹。鹦鹉按官方 `ON_GROUND`、`MOTION_BLOCKING` 和 `parrots_spawnable_on` 模拟放置，但不创建实体，也不运行鹦鹉的 `finalizeSpawn`。同一区块先出现原版鹦鹉时，仍比较装饰随机源和后续已实现生物的位置；后续区域随机源和依赖它的属性不在夹具比较范围内。
## 苦力怕的实施现状（2026-11-20）

苦力怕在 `entity/mob/creeper.rs`：`CreeperState`（`swell` / `maxSwell` / `explosionRadius` 和三项同步元数据）挂在 `Mob.creeper` 上，全是相对计数，随实体行搬岛，不需要时钟平移，也没有新增 `IslandPart`。`Creeper.tick` 里 `super.tick()` 之前的那段在 `living::tick_mob` 的 `base_tick` 之前调用；`SwellGoal` 是 `Goal::Swell`（每刻目标，MOVE 标志），记住的目标按原始引用查死活，不走 `getTarget()` 的可攻击过滤。近战用通用的 `Goal::MeleeAttack`，和 main 合并后由 `class` 字段（官方类名）区分 `ZombieAttackGoal` 和普通 `MeleeAttackGoal`（苦力怕、骷髅）；`Creeper.doHurtTarget` 直接返回真，不造成伤害。目标选择走骷髅那套完整的 `TargetGoal` 规则（`MobType::full_target_rules`）。

爆炸是新命令 `Command::ExplodeMob`，落地在 `World::explode_mob`（`entity/explosion.rs`）：和 TNT 共用射线和 `interact_with_blocks`，另加 `hurt_living_by_explosion`（生物和玩家的伤害、生物的击退）。命令里带来源生物的 `EntityPublic`：发出者正在 tick，`me.kind` 是占位，玩家的死亡消息要靠它拿名字和 UUID。方块掉落的衰减经 `Level::explosion_block_drops_decayed` 把爆炸半径交给战利品表，由新的方块层游戏规则 `mob_explosion_drop_decay` 控制。伤害类型新增 `DamageKind::PlayerExplosion`。并行驱动里这条命令和 TNT 的 `Explode` 一样在落地阶段执行，世界随机数只在落地时取。

别的生物可以复用的东西：`GoalSelector::from_goals` / `Goal::melee` / `Goal::nearest_target`（按注册顺序搭目标表）、`MobType::has_no_ambient_sound`（`getAmbientSound()` 为 null 时不取音高随机数）、`MobType::uses_block_step_sound`（走路声用方块的 `SoundType`）、`MobInteraction::damage_held`（交互后给手里的物品扣 1 点耐久）、`World::explode_mob`（任何生物引发的 `ExplosionInteraction.MOB` 爆炸）、`creeper::finalize_spawn`（只有 `Mob.finalizeSpawn` 那一层的怪物）。

顺带修正：`MeleeAttackGoal.tick` 重算路径前要先 `hasLineOfSight`（`followingTargetEvenIfNotSeen` 为假时），原先漏了；隔墙被打的场景才会暴露，僵尸和骷髅的既有场景逐刻结果不变。

逐刻对拍场景：`creeper_chase_*`、`creeper_ignited`、`creeper_powered`、`creeper_fall`、`creeper_wall_hurt`、`creeper_blast_player_{1,2,3}`（`mob-oracle --only creeper`）。已知限制见 README 的苦力怕一条。

## 蜘蛛和洞穴蜘蛛的实施现状（2026-11-21）

蜘蛛在 `entity/mob/spider.rs`：`SpiderState`（`DATA_FLAGS_ID` 的攀爬位）挂在 `Mob.spider` 上，是相对状态，随实体行搬岛。`Spider.tick` 在 `living::tick_mob` 的 `ai_step` 之后执行，把 `Entity.horizontal_collision` 写进标志；`living::on_climbable` 对 `MobType::climbs_walls()` 的种类直接读这个标志（官方 `Spider.onClimbable`），爬墙的 0.2 上升、-0.15 下滑和水平 0.15 的夹取都走原有的 `travel` 代码。导航新增 `Navigation::wall_climber` / `path_to_position`：`create_path` 记录目标方块，`tick` 在路径走完后返回目标方块的三维坐标（`WallClimberNavigation`）。`move_to_entity` 是三参数的 `moveTo(entity, reach, speed)`，走基类：没有路径就返回假、不改速度（官方只改写了两参数版本，蜘蛛的目标不调它）。

目标：`Goal::LeapAtTarget` 是新的目标（JUMP + MOVE 标志，起跳改速度）；`Goal::MeleeAttack` 新增 `light_flee`（`SpiderAttackGoal` 的亮处放弃和类名）和 `follow_unseen`（`followingTargetEvenIfNotSeen=true`，`canContinueToUse` 不看路径走完，`tick` 不要求视线）。`Goal::NearestAttackableTarget` 新增 `light_gate`（`SpiderTargetGoal` 的亮处闸门，连 `nextInt` 都不取）。`living::light_dependent_magic` 是 `Entity.getLightLevelDependentMagicValue` 的共用实现。

洞穴蜘蛛复用同一套目标和尺寸表，只有属性（12 点生命）和命中中毒不同：`living::do_hurt_target` 命中后按难度给目标中毒，玩家走新命令 `Command::AddPlayerEffect`，生物走 `Command::HurtMob` 的新字段 `on_hit`（伤害真的落地才挂）。`living::add_effect` 会先问 `spider::can_be_affected`，中毒对蜘蛛无效。自然生成：`check_spawn_rules` 走怪物那条黑暗分支，`spawn::SpawnGroup::Spider` 保存同一组共享的药水效果。

别的生物可以复用的东西：`Command::AddPlayerEffect` / `World::give_mob_effect`（给另一个实体挂效果）、`Goal::LeapAtTarget`、`MeleeAttack` 的 `follow_unseen`、`spider::can_be_affected` 这种「按种类挡效果」的钩子。

逐刻对拍场景：`spider_chase_*`（午夜追玩家）、`spider_wall`（被打后翻过一整面石墙）、`spider_day`（白天中立）、`cave_spider_chase`（`mob-oracle --only spider_` / `--only cave_spider_`），5 个场景 840 刻 0 不一致。`spider_wall` 原先从翻墙那一刻起随机源比官方多走：`move_to_entity` 曾按两参数版本在没有路径时返回真，`MeleeAttackGoal.tick` 因此少加 15 刻重算间隔、多抽 `nextInt(7)`；改回基类语义后一致（2026-11-21，改坏确认：攀爬位恒为假时第 43 刻起报不一致）。`spider_day` 原先从第 34 刻（散步启动）起位置 / 速度差约 1e-9：根源是官方 `Spider.createAttributes` 写的移速是 float `0.3F`（加宽后 0.30000001192092896），我们存的是 double 0.3，0.8 倍速散步时 `setSpeed` 的 float 差一个 ulp；改成 `f64::from(0.3_f32)` 后四个场景 0 不一致（2026-11-21）。已知限制见 README 的蜘蛛一条。

## 末影人的实施现状（2026-11-21）

末影人在 `entity/mob/enderman.rs`：`EndermanState`（`carried` / `creepy` / `stared_at`、`target_change_time`、怒气目标与到期刻、待落地的搬放方块）挂在 `Mob.enderman` 上，都是相对状态，随实体行搬岛。`MobType::Enderman` 在 `set_target` 上套了一层 `enderman::set_target`：写目标的同刻设 `DATA_CREEPY`、`targetChangeTime` 并加「attacking」移速修饰（+0.15 加法），清目标时全部回退；所有设目标的地方（目标选择器、`HurtByTargetGoal`、`getNearestPlayer` 路径）都走这个包装，和官方 `Enderman.setTarget` 一致。

目标选择器：`Goal::EndermanFreezeWhenLookedAt`（JUMP + MOVE，`canUse` 只看 `getTarget()` 是玩家、16 格内、`isBeingStaredBy`）和 `Goal::EndermanGaze`（`EndermanLookForPlayerGoal`：`getNearestPlayer(forCombat().range(64).selector(isAngerInducing))`，agro 计时 `adjustedTickDelay(5)`，锁定时 4 格内传送、16 格外每 15 刻朝玩家传送）；`Goal::EndermanBlock { place }` 复用 `canUse` 的随机闸门和 `tick` 的三次 double，改方块走新命令 `Command::EndermanBlock`（游戏层在世界线程落地，`World::set_block_and_update` / `remove_block`）。`NearestAttackableTargetGoal<Endermite>` 用 `TargetType::Endermite`：`available()` 为假，但 `nextInt(reducedTickDelay(10))` 照取，随机源不会错开。

`LivingEntity.isSensitiveToWater`：`living::animal_ai_step` 在 `pushEntities` 之后、`Mob.aiStep` 的日晒之前调 `enderman::hurt_if_wet`（1 点溺水伤害，位置用包围盒顶上的那一格，官方 `isInRain`）。`LivingEntity.hurtServer` 的投掷物分支在 `living::hurt_server` 顶部：`DamageKind::Arrow | Spit` 先连续 64 次 `teleport()` 并返回 false；其余伤害走原路径后再按「来源不是生物」1/10 概率传送。`Entity.getLightLevelDependentMagicValue` 用 `living::light_dependent_magic` 的共用实现（眼位置那一格）。

存档：`carriedBlockState` / `anger_end_time` / `angry_at`；`requiresCustomPersistence` 让搬着方块的末影人不被自然清除。同步：`EntityData::EndermanCarried`（`OPTIONAL_BLOCK_STATE`，序列化器号 15）、`EndermanCreepy` / `EndermanStaredAt`（BOOLEAN），index 来自生成器。死亡掉落：实体表之外，游戏层 `mob_loot_drops` 在表求值后用一把带 `minecraft:silk_touch` 的假钻石斧再查一次方块表（`MobDeathLoot.carried`）。

其他生物可以复用的：`TargetType::Endermite` 这种「类型不存在但取数照旧」的目标写法、`MobType::uses_block_step_sound`（末影人和苦力怕一样用脚下方块的 `SoundType`）、`enderman::set_target` 这种「设目标时同步专属状态」的包装。

逐刻对拍场景（`mob-oracle --only enderman_`）：`enderman_stare` / `enderman_neutral`（注视与不注视）、`enderman_day_teleport`（白天露天）、`enderman_rain`（夜里淋雨）、`enderman_arrow`（箭）、`enderman_stare_edge_3` / `enderman_stare_edge_6`（注视锥的边缘，偏 3 度和 6 度）、`enderman_blocks`（脚边的泥土捡起再放下）、`enderman_place`（开局搬着泥土再放下）。9 个场景 61416 刻 0 不一致。`enderman_day_teleport` 原来只跑 200 刻，到不了 `tickCount >= targetChangeTime + 600` 的门槛，白天传送一次都没判定过；2026-11-21 改成 1200 刻，改坏确认（`nextFloat() * 30` 改成 20）第 685 刻起报不一致。这个场景里的传送尝试都落空（两边一致），传送成功后的朝向仍只在注视 / 淋雨 / 箭的场景里覆盖。方块场景把天色固定在傍晚并关掉「看得见天」，避免长场景跑到白天传送；传送后的头部 / 身体朝向差异见 README。已知限制见 README 的末影人一条。

## 鱿鱼和发光鱿鱼的实施现状

鱿鱼一支是官方的 `AgeableWaterCreature`（`AgeableMob` 但不是 `Animal`）。专属逻辑在 `entity/mob/squid.rs`，海豚和鱼类共用的公共部分在 `entity/mob/water.rs`（见文末海豚一节）：

- `water::handle_air_supply`（`AgeableWaterCreature.handleAirSupply`，在 `super.baseTick()` 之后用这一刻开始时的氧气算）、`water::surface_spawn_rules`（`checkSurfaceAgeableWaterCreatureSpawnRules`）、`water::in_water_position_ok`（`SpawnPlacementTypes.IN_WATER`）和 `water::finalize_ageable`（不带花色那一层的 `AgeableMob.finalizeSpawn` + `Mob.finalizeSpawn`）。
- `MobType` 新增几个按类型的开关，别的水生生物加进来时只改这些函数：`is_ageable_water_creature`、`is_animal`（存档的 `InLove` / `LoveCause` 只有 `Animal` 写）、`can_breathe_under_water`、`is_pushed_by_fluid`、`emits_movement_sounds`（`MovementEmission.EVENTS` 不放脚步声、也不取游泳声的两个随机数）、`spawns_in_water`。
- `living::hurt_server` 现在是 `hurtServer` 的虚分派入口：鱿鱼走 `squid::hurt_server`（父类成功且 `getLastHurtByMob() != null` 才返回真并喷墨，发光鱿鱼再设 100 刻变暗），其余走 `living_hurt_server`。
- 目标：`Goal::SquidRandomMovement`（优先级 0）和 `Goal::SquidFlee`（优先级 1，每刻目标）都不占标志位，只写 `Mob.squid.movement`；`Squid.aiStep` 的尾部（`squid::ai_step`）按触手节拍把它写成速度，`travel` 只是 `move(SELF, deltaMovement)`。官方 `SquidFleeGoal.tick` 里 `fleeTo.normalize()` 的返回值被丢弃，这里同样不归一化。
- 新的生物事件 `MobEvent::Particles`（`ServerLevel.sendParticles`）：游戏层按「玩家所在方块的中心离粒子不到 32 格」发 `level_particles`。以后别的生物的服务端粒子可以复用。
- 发光鱿鱼的 `DATA_DARK_TICKS_REMAINING` 下标由生成器导出（`entity_types::GLOW_SQUID_DARK_TICKS`）。服务端的 `addParticle(GLOW, getRandomX, getRandomY, getRandomZ)` 不发包，但三个 `nextDouble` 照取。
- 自然生成：`SpawnParams` 多一个 `sea_level`（`natural_spawn.rs` 按世界类型给）；`check_spawn_rules`、`spawn_placement_ok`、`is_valid_position_for_mob`（`AgeableWaterCreature.checkSpawnObstruction` 不查液体）按类型分派。`walk_target_value` 对非 `Animal` 返回 `PathfinderMob` 的缺省值 0。

岛的拆合：新增的按实体状态只有 `Mob.squid`（`SquidState`：身体倾角、触手节拍、移动向量、变暗倒计时）和目标里的 `flee_ticks`，全是相对量、都在实体行上，整行搬走即可，没有改 `IslandPart` 清单和 `region/island_split.rs`。

对拍：`mob-oracle --only squid_` / `--only glow`（`squid_swim`、`squid_hurt`、`squid_beached`、`squid_swim_baby`、`glow_squid_swim`、`glow_squid_hurt`，共 1666 刻）逐刻比位置、速度、朝向、随机源、氧气、身体与触手状态、移动向量、掉落、世界随机源和出站包字节（配对、同步、实体事件 19、音效、墨汁粒子、变暗元数据）；`--break-self-test squid` 把那一下伤害改成 2 点能被抓到。自然生成见 `mob-oracle --only natural_spawn` 的 `squid_ocean*` / `glow_squid_*` 夹具，存档见 `chunk-content` 的 fresh 实体对拍。

已知差异：生物受伤后 `ServerEntity.sendChanges` 因 `hurtMarked` 多发的那个速度包，正式游戏路径一直没有发（`Mob.hurt_marked` 只写不读，牛和僵尸也一样）；`mob-oracle` 的鱿鱼场景在对拍里自己读并清这个标志。

## 鱼的实施现状（2026-11-21）

鳕鱼、鲑鱼、热带鱼、河豚是官方的 `WaterAnimal` → `AbstractFish` 一支，专属逻辑在
`entity/mob/fish.rs`。它们不是 `AgeableMob`：没有年龄、求爱和繁殖，存档不写 `Age`。

- 公共部分：`water::handle_air_supply` 对 `is_water_animal()` 也生效；`MobType` 加了
  `is_water_animal` / `is_ageable`，`is_animal`、`is_pushed_by_fluid`、`spawns_in_water`、
  `can_breathe_under_water` 都跟着这两种鱼类分支改。
- 导航：`EvaluatorFlags.swim = Some(allowBreaching)` 选中 `SwimNodeEvaluator`
  （六个方向 + 两边都可走的四个水平对角），`WaterBoundPathNavigation` 的
  `canUpdatePath` / `getTempMobPos` / `getGroundY` / `isStableDestination` /
  `getMaxVerticalDistanceToWaypoint` 都按 `flags.swim` 分派；鱼的
  `allowBreaching` 是假，海豚是真（见文末海豚一节）。`BlockState::is_pathfindable_water` 来自新导出的
  `PATHFINDABLE_WATER` 状态事实。
- 移动：`fish::move_control_tick`（`FishMoveControl`）、`fish::travel_in_water`
  （`AbstractFish.travelInWater`）、`fish::ai_step`（离水扑腾）；`motion::movement_emission`
  现在真的发游泳声（音量按速度、音高两个 `nextFloat`），鱼的脚步声为空但「踩过」仍成立。
- 目标：`Goal::Swim`（`RandomSwimmingGoal` + `getRandomSwimmablePos` 的 11 次抽点）、
  `Goal::PufferfishPuff`、`Goal::FollowFlock`（`FollowFlockLeaderGoal`，跟随时用
  `Command::SchoolFollow` / `SchoolLeave` 改领头和群计数）。`AvoidEntityGoal`（优先级 2）未建模。
- 河豚蜇刺走新的 `Command::PufferSting`：先 `hurtMob`，打中了才下毒并放蜇刺声。
- 数据：`FROM_BUCKET`(16)、`SALMON_VARIANT`/`TROPICAL_FISH_VARIANT`/`PUFFERFISH_PUFF_STATE`(17)
  由生成器导出，元数据和存档都用它们；`MobPublic` 多带鱼的花纹、膨胀和群的三个字段。
- 对拍：`mob-oracle --only cod|salmon|tropical|puffer` 和 `--only natural_fish`。
  `cod_swim` / `salmon_swim` / `tropical_fish_swim` 各 241 刻 **0 不一致**（位置、速度、
  朝向、随机源、氧气、尺寸、掉落、世界随机源、出站包逐字节）；`cod_beached` 461 刻、
  `pufferfish_puff` 241 刻也都 **0 不一致**（2026-11-21 修好，原因见下）。
- 自然生成：`natural_fish` 五个夹具（鳕鱼 / 鲑鱼 / 热带鱼 / 繁茂洞穴热带鱼 / 河豚）
  各 6 个种子 × 4 个时刻，**30 例 0 不一致**。为此修了两处公共规则：
  `MobType::can_spawn_far_from_player` 原来对所有非怪物返回真，官方只对
  `CREATURE | MISC` 为真（`WATER_AMBIENT` 的消失距离是 64，超过就不该进
  `isValidSpawnPostitionForType`）；`goals::walk_target_value` 对 `is_water_animal()`
  也返回 0，因为鱼不是 `Animal`，`PathfinderMob.getWalkTargetValue` 的缺省就是 0
  （原来落到 `Animal` 的亮度权重，繁茂洞穴里是负数，滤掉全部候选）。
- 桶装（`Bucketable`）：`entity/mob/bucketable.rs` 负责
  `saveDefaultDataToBucketTag` / `loadDefaultDataFromBucketTag`（`bucket_entity_data`
  组件）、鱼桶物品、`bucketMobPickup` 和 `MobBucketItem.spawn`。水桶右键活鱼按
  `AbstractFish.mobInteract` → `Bucketable.bucketMobPickup` 换桶并 `discard()`；
  倒鱼桶先走 `BucketItem.emptyContents` 放水，再 `checkExtraContent` 在落点生成
  `FromBucket = true` 的鱼（朝向取 `level.random`，`finalizeSpawn` 用 `BUCKET` 原因，
  之后套组件并 `loadFromBucketTag`），发射器倒鱼桶也生成。鲑鱼存 `salmon/size`，
  热带鱼存 `tropical_fish/pattern` / `base_color` / `pattern_color`，其余只有默认数据。
  美西螈现已建模（见文末），倒美西螈桶会放出 `FromBucket = true` 的美西螈；
  蝌蚪 / 硫磺立方桶还没建模，只按官方 `EntityType.create` 的次数取随机数。
  验证：块级单元测试（组件拆分、只改有的键、存档往返）、游戏层两条（生存换桶、
  创造加桶）、发射器一条（生成的鱼 + 随机源种子）。没有 Java 逐刻对拍。
- 2026-11-21 修掉的三处：`AbstractFish.aiStep`（离水扑腾）原来对所有动物都跑，陆地动物
  站在地上每刻都会蹦、取实体随机数、放扑腾声并从岛的音效种子源取数（`sheep_eat` 漂移和
  `island_command` 岛 1 随机源不一致都是它），现在只对 `is_fish` 跑；扑腾时补上
  `needsSync`，死后倒计时那 20 刻里速度和移动包才会当刻发出（`cod_beached`）；河豚没有
  叫声，原来回落到牛的叫声并多取两个音高随机数（`pufferfish_puff` 第 62 刻），另外对拍
  Rust 一侧的牛位置从 1.5 改成和官方一样的 0.9。

## 海龟的实施现状（2026-11-21，分支 `op-water`）

海龟是官方 `Animal` 一支，专属逻辑在 `entity/mob/turtle.rs`。它的移动是两栖的，
所以这一轮给寻路加了一层公共设施：

- `EvaluatorFlags.amphibious`：`AmphibiousNodeEvaluator`（水当可走、上下两个水邻居、
  贴着 `BLOCKED` 的水是 `WATER_BORDER`、搜索期间 `WATER` / `WALKABLE` / `WATER_BORDER`
  的代价临时改成 0 / 6 / 4）和 `AmphibiousPathNavigation`（`canUpdatePath` 恒真、
  `getTempMobPos` 取 `getY(0.5)`、`getGroundY` 原样、`isStableDestination` 要求下面
  不是空气、只在液体里允许直走）。美西螈现在直接用这个开关（见文末）。
- 实体：`Mob.turtle`（家方块、`travelPos`、`goingHome`、`layEggCounter`、`has_egg`、
  `laying_egg`、回家目标的两个计数、产卵 `MoveToBlockGoal` 的四个进度）。属性、
  尺寸（幼年 0.3 倍）、水 / 门三种 `PathfindingMalus`、音效（幼年受伤和死亡、
  陆地上的叫声、`nextStep = moveDist + 0.15`）、`travelInWater` 和 `canFallInLove`
  都按官方。
- 目标：0 `TurtlePanicGoal`（先找 7 格内的水）、1 `TurtleBreedGoal`、1
  `TurtleLayEggGoal`、2 `TemptGoal`、3 `TurtleGoToWaterGoal`、4 `TurtleGoHomeGoal`、
  7 `TurtleTravelGoal`、8 `LookAtPlayerGoal`、9 `TurtleRandomStrollGoal`。
  `MoveToBlockGoal.findNearestBlock` 按官方的 y / 圈 / x / z 交替顺序写成一个通用函数，
  产卵（半径 16，y 依次 0、1、-1）和找水（半径 24，只看 y = -1 那一层，即脚下两格）共用；
  `MoveToBlockGoal` 没有 `stop`，停止时不停导航。远行目标读 `BlockView::sea_level`
  （游戏层按世界类型给 63 / -63），有远行目标时 `TurtlePathNavigation.isStableDestination`
  只认水格。
- 移动：`TurtleMoveControl`（水里上浮 0.005，离家 16 格外速度减半、幼年再除以 3，地上
  速度减半，0.125 插值，不跳也不回到 `WAIT`）；`AmphibiousNodeEvaluator.getStart` 在水里
  从包围盒最小角起步（原来按走路求值器往下找地面，会穿过水下的空气一路找到世界底部）。
  能在水下呼吸，游泳声 `entity.turtle.swim` 音量 ×1.5。
- 繁殖：`TurtleBreedGoal.breed` 不走 `Animal.spawnChildFromBreeding`，母亲设
  `has_egg`、双方年龄 6000、掉一颗 `1 + nextInt(7)` 的经验球；`apply_breed` 里有一条
  海龟分支。喂食时 `canFallInLove` 为假（有蛋）就不进求爱。
- 产卵和孵化：`TurtleLayEggGoal` 在离家 9 格内找上面是空气、下面是沙子的格子，
  `layEggCounter > 200` 时用 `Command::LayTurtleEgg` 放 1–4 只蛋（`EGGS`），
  蛋的 `randomTick` 按 `TURTLE_EGG_HATCH_CHANCE`（白天 0.002、夜里 21062–21905 刻
  为 1.0）推进 `HATCH`，第三阶段清掉方块并生成 `EGGS` 只幼年海龟（家方块是蛋的位置）。
- 存档和同步：`home_pos` / `has_egg` 存盘；`HAS_EGG`(18) / `LAYING_EGG`(19) 同步，
  非默认才发。长大的那一刻按 `entities/turtle_grow` 掉鳞甲（走 `EntityLootRequest`）。
- 验证：`mob-oracle --only zombie_turtle_ai`（类型 60，借僵尸场景的多生物路径）四个场景：
  沙滩找水两个种子、幼年、水下远行，共 1500 刻 **0 不一致**（位置、速度、朝向、随机源、
  氧气、运行中的目标、路径节点）。产卵和回家见下一段，孵化仍只有单元测试，没有自然生成夹具。
- 验证（2026-11-21 追加）：`mob-oracle --only zombie_turtle_egg` 四个新场景（带蛋产卵三个
  种子、离家 12 格回家）加既有的僵尸踩蛋场景共 8 例 3220 刻 **0 不一致**。`Scene.turtleEgg`
  开局把 `has_egg` 和家方块写进两侧；`zombieStepJson` 逐刻比带蛋 / 正在产卵 / 产卵计数 /
  回家标志 / 家方块，方块表散列 `bh` 覆盖蛋放下那一刻；Java 侧断言每个产卵场景确实产下蛋；
  `--break-self-test turtle_egg` 把带蛋标志取反，抓到 12 处不一致。孵化的随机刻会生成实体，
  `oracle` 明确按 `EntityRandomTick` 排除，只有 `behaviour/misc/turtle_egg.rs` 的单元测试。
  同时修掉 `ctx.rs` 里 `Command::LayTurtleEgg` 把状态 ID 当方块注册表下标用的越界崩溃
  （之前从没被跑到，游戏里海龟一产卵就会 panic）。全量 `mob-oracle` 复核 1744 例 165621 刻
  0 不一致。

## 海豚和美西螈的实施现状（2026-11-21/22，分支 `op-water`）

海豚是官方 `Dolphin`（`AgeableWaterCreature` = `AgeableMob` 但不是 `Animal`），美西螈是
`Animal` + `Bucketable`、官方用 Brain（`AxolotlAi`）驱动。两者专属逻辑各自在
`entity/mob/dolphin.rs` / `entity/mob/axolotl.rs`，美西螈的大脑在
`entity/mob/axolotl/ai.rs`，共用的水面刷怪规则、`AgeableMob` 组数据那一层在
`entity/mob/water.rs`。

- 实体：`Mob.dolphin`（`got_fish` / `moistness` / `treasure_pos` / `stuck`）、
  `Mob.axolotl`（`variant` / `playing_dead` / `from_bucket`；装死倒计时是大脑记忆
  `PLAY_DEAD_TICKS`，不再放实体状态里）。
  属性、尺寸（幼年 `BABY_DIMENSIONS`）、`WATER` 的 `PathfindingMalus`、音效、战利品表
  `entities/dolphin` / `entities/axolotl`、刷怪蛋、`/summon` 的 `by_entity_type` 都按官方。
- 氧气：海豚覆写 `handleAirSupply` 为空（氧气走 `LivingEntity.baseTick`）、`increaseAirSupply`
  直接回满，满值 4800；美西螈满值 6000，`baseTick` 之后按 `preTick - 1` 掉、到 -20 归零并
  受 2 点 `dry_out`。`living::base_tick` 按 `squid::is_squid` / `fish::is_fish` /
  `MobType::Axolotl` 分派，海豚不进 `handleAirSupply`，`isNoAi()` 时只剩 `LivingEntity.baseTick`。
- 海豚：`Dolphin.tick` 在 `super.tick()` 后管湿度 `Moistness`（2400，水 / 雨里回满、否则每刻减一，
  到 0 受 1 点 `dry_out`）、离水着地扑腾（两个 `nextFloat` 定水平速度、一个定 `yRot`、`needsSync`）。
  `SmoothSwimmingMoveControl` + 允许跃出水面的 `WaterBoundPathNavigation`（`swim = Some(true)`）；
  `travelInWater` 是 `moveRelative` + `move(SELF)` + ×0.9、无攻击目标每刻下沉 0.005。
  拿 `#fishes` 右键走自己的 `mobInteract`：幼年 `ageUp`、成年 `gotFish`，消费一条鱼。
- 美西螈：五花色，`finalizeSpawn` 抽两个常见花色编号组成 `AxolotlGroupData`、再抽一个给这一只，
  一组第三只起是幼年、`BUCKET` 原因直接返回；`hurtServer` 前按官方短路判定写
  `PLAY_DEAD_TICKS = 200`；两栖寻路（`AmphibiousPathNavigation`）、`travelInWater` 没有
  重力项、不吃水流。
- 美西螈大脑（`axolotl/ai.rs`）：`Axolotl.BRAIN_PROVIDER` 的五个传感器（顺序照官方）和
  `AxolotlAi.getActivities()` 的四个活动 `core` / `idle` / `fight` / `play_dead`，
  优先级、行为表、进入条件、退出时清的记忆照官方（`mob-oracle` 逐字比这张表）。
  捕猎：`AxolotlAttackablesSensor`（8 格、在水里、`#axolotl_always_hostiles` 或没有
  `HAS_HUNTING_COOLDOWN` 时的 `#axolotl_hunt_targets`）→ `StartAttacking` → `FIGHT`
  （`StopAttackingIfTargetInvalid`、`SetWalkTargetFromAttackTargetIfTargetOutOfReach`、
  `MeleeAttack(20)`、繁殖时清 `ATTACK_TARGET`）→ `Mob.doHurtTarget`；`updateActivity`
  从 `FIGHT` 退出时记 2400 刻捕猎冷却。游泳：`TryFindLiquid(6, 0.15,
  #axolotl_tries_to_find)`、`RandomStroll.swim(0.5)`、`SetWalkTargetFromLookTarget`
  （水 / 陆要和自己在不在水里一致），速度按 `getSpeedModifier` 0.5 / 0.15、追击与跟随成年
  0.6 / 0.15。装死：`PlayDead`（200 刻，开始时清 `WALK_TARGET` / `LOOK_TARGET`、加 200 刻
  等级 0 的 `REGENERATION`）和 `ValidatePlayDead.create`（每刻递减，到 0 清
  `PLAY_DEAD_TICKS` 与 `HURT_BY_ENTITY`）；`customServerAiStep` 尾部只把记忆映射成
  `playingDead` 元数据。
- 装桶 / 存档 / 同步：`Bucketable`（鲑鱼 / 热带鱼的花纹组件之外）为美西螈写
  `bucket_entity_data` 的 `Variant` / `Age` / `AgeLocked`，倒桶 / 发射器按 `MobBucketItem.spawn`
  放出 `FromBucket = true` 的美西螈；`GotFish` / `Moistness`、`Variant` / `FromBucket` 进存档
  （`nbt.rs`）；`GOT_FISH`(18) / `MOISTNESS_LEVEL`(19)、`DATA_VARIANT`(18) /
  `DATA_PLAYING_DEAD`(19) / `FROM_BUCKET`(20) 在 `entity_tracking.rs` / `packet/play/entity.rs`
  同步，多岛拆合时这些状态随 `Mob` 整块搬。
- 自然生成：`natural.rs` 的 `check_spawn_rules` 给海豚接
  `checkSurfaceAgeableWaterCreatureSpawnRules`（和鱿鱼同源）、给美西螈接
  `checkAxolotlSpawnRules`（脚下 `axolotls_spawnable_on` 黏土）；`spawn.rs` 的 `finalizeSpawn`
  走 `dolphin::finalize_spawn` / `axolotl::finalize_spawn`，组数据 `SpawnGroup::Axolotl`；
  `MobType::mob_category` 给海豚 `WaterCreature`、美西螈 `Axolotls`。
- 验证：`mob-oracle --only dolphin`（4 场景 804 刻）/ `--only axolotl`（4 场景 804 刻），
  两边都设 `NoAI`、给同一个显式实体随机源种子、不重放构造取数，逐刻比位置、速度、朝向、
  随机源、氧气、年龄、着地 / 在水里、各自元数据、尺寸和世界随机源，**0 不一致**；
  海豚场景还逐字比官方 `goalSelector` 注册表（14 个目标的优先级、类名、标志位掩码，
  `DolphinJumpGoal` 继承 `JumpGoal` 锁 MOVE|JUMP）；美西螈场景逐字比大脑结构表（传感器
  类名 / 扫描间隔 / `requires()`、行为表的优先级 / 活动 / 类名与官方遍历顺序、活动进入条件、
  退出时清的记忆），4 个场景全等；
  `--break-self-test water_mob` 把 `got_fish` / `playing_dead` 改坏能在第 0 刻被抓到，
  `--break-self-test dolphin_goals` 清空海豚目标注册表、`--break-self-test axolotl_brain`
  加一个假传感器也会被抓到。
  `mob-oracle --only natural_spawn` 的 `dolphin_ocean` / `axolotl_lush` 夹具（后者把底层水换成黏土）
  各刷出 37 / 40 只，0 不一致；`natural-spawn`（全部）1727 例 0 不一致。
- 已知限制：海豚 `registerGoals` 的十四个目标和 `HurtByTargetGoal` 已建成真目标
  （`dolphin.rs` / `dolphin_goals.rs`，优先级 / 类名 / 标志位 / 次序照官方；
  `canUse` / `canContinueToUse` / `start` / `tick` 按官方翻译），不再是 `Unavailable` 占位。
  但 `findNearestMapStructure`、船 / 鹦鹉螺、掉落物实体、游泳玩家、`DOLPHINS_GRACE`
  这些设施还没接，依赖它们的分支按 `fox_goals` 做法保住取数次数与顺序后返回假——引路去宝藏、
  给玩家 `DOLPHINS_GRACE`、捡物、躲守卫者仍不发生。**goalSelector 没有逐刻对拍**：夹具开
  `NoAI`，只核对注册表与 NoAI 分支，真跑目标所需的导航 / 移动控制 / `DefaultRandomPos`
  落点 / 标志位锁定保真没逐刻验证，跃水与呼吸换气也未和官方逐刻比较；美西螈大脑的
  传感器 / 记忆 / 四个活动与捕猎、游泳、装死已按官方接线，但**行为没有逐 tick 对拍**
  （夹具开 `NoAI`，Brain 不跑），只有结构表与单元测试覆盖；繁殖已按官方实现（见下一小节），
  `Axolotl.onStopAttacking` 的再生回调、
  `swingForAttack` 挥手动画没建模（`AxolotlLookControl` / `AxolotlMoveControl` 已按
  `SmoothSwimming*` 参数接上，见下一小节）；
  拴绳、刷怪蛋对着水面用、粒子 / 动画、`SWIM` 游戏事件、
  死亡掉落的逐题 loot-oracle 都没做。逐刻对拍开 `NoAI`，没覆盖 AI / 受伤 / 繁殖 / 死亡路径。

### 第 14 轮补完：繁殖、桶、控制与踩蛋（2026-11-22）

- 繁殖：`Axolotl.getBreedOffspring` 的花色在 `axolotl::offspring_variant`。先
  `nextInt(1200)`；命中时 `Variant.getRareSpawnVariant` 从单元素名单再抽一次
  `nextInt(1)`，否则一次 `nextBoolean` 取父亲或母亲。两条分支都恰好两次取数。父亲花色
  随 `Command::BreedAnimals` 新增的 `father_axolotl` 落到 `apply_breed`（命令落地时父亲
  不在实体表里），幼崽的年龄和尺寸走通用 `set_age`。
- 桶：`Axolotl.saveToBucketTag` 的 `HuntingCooldown = brain.getTimeUntilExpiry(...)`
  （剩余刻）写进 `bucket_entity_data`，`loadFromBucketTag` 用同一个数作时长恢复。Bamboo
  的记忆 TTL 本来就是相对刻，语义一致。
- 喂食：`Axolotl.usePlayerItem` 把热带鱼桶换成水桶（创造模式手里不变）。
- 移动 / 视角：新模块 `entity/mob/smooth_swim.rs` 收 `SmoothSwimmingMoveControl` /
  `SmoothSwimmingLookControl`，青蛙、蝌蚪、鱼、海豚、美西螈共用。美西螈
  `(85, 10, 0.1, 0.5, false)`，装死时移动和视角都停；海豚 `(85, 10, 0.02, 0.1, true)`
  与 `SmoothSwimmingLookControl(10)`（之前海豚落到通用 `MoveControl`）。
- 海龟蛋：`fallOn` 只豁免僵尸一系（原来错误地豁免所有怪物），`destroyEgg` 排除海龟。
- 验证：`mob-oracle --only axolotl` 47 例 847 刻 0 不一致（含 43 例繁殖花色，
  `--break-self-test axolotl_breed` 故意改坏能被抓到）；`--only zombie_turtle_egg`
  8 例 3220 刻 0 不一致；`--only frog` 16 例 9000 刻、`--only tadpole` 8 例 2360 刻、
  `--only salmon` 1 例 241 刻 0 不一致，确认共用控制重构没改行为。
- 仍未做：美西螈 AI 的逐 tick 对拍、`onStopAttacking` 的玩家再生、`swingForAttack`、
  拴绳、海龟蛋的玩家破坏与 `getShape`。

## 与 main 合并后的现状（op-hostile2，2026-11-21）

按队长 04:30 的要求先 `git merge main`（`ceb0562f`，op-hostile 的苦力怕 / 蜘蛛 /
洞穴蜘蛛 / 末影人和世界生成缓存）。公共文件按两边并集解：`generated/entity_types.rs`
用 `extract` 重新生成，和并集逐字节一致；`drowned.rs` / `slime.rs` / `witch.rs` 的
`NearestAttackableTarget` 补上 main 新增的 `light_gate: false`，`spider.rs` 测试补齐
`PlayerView` 新字段，`mod.rs` 合并重复的 `ambient_sound` 分支。合并后
`cargo check --workspace --all-targets`、clippy、单测、五种生物各前缀的针对性
`mob-oracle`（含 `--only natural_spawn` 620 例 2480 刻）、`loot-oracle` 和官方联机探针
（769/769）都通过。全量 `mob-oracle` 仍有 6 处 `player_death_*` 不一致，是超平坦世界
重生包 `sea_level` 给 -63、官方离线夹具给 0 的既有差异（base 已带该字段），与生物无关，
见 README 已知限制；按 `verify-scope.md` 属非关键路径，不追。

### 与 main 二次合并后的现状（op-hostile2，2026-11-21 本轮）

main 又合进了马驴骡羊驼、兔狐狼、水生（鱿鱼、鱼、海龟）和杂项（哞菇、北极熊、熊猫），
本轮 `git merge main`（`e1d69b66`）。公共文件按 main 的结构解冲突：`MobType` 改用
`Self::ALL`（扩到 33 种，含本分支的流浪者 / 沼骸 / 史莱姆 / 溺尸 / 女巫），
`spawn.rs` 的 `finalize_spawn_in_group` 按 `is_skeleton()` / `is_zombie()` 加 Panda 分支，
`natural.rs` 的 `ON_GROUND` 落脚检查合成一个 `entity_type` + `immune_to` 的入口，
`generated/entity_types.rs` 用 `extract` 重新生成（和并集逐字节一致，
`registries.rs` 的 `game_event` 注册序漂移照旧还原）。两处真分叉按官方字节码定：
① `MobPublic::of` 保留 `in_water` 入参（女巫 / 溺尸 AI 读公开视图），`on_fire` 改用
`Mob.visible_fire`（`Entity.baseTick` 才写共享标志，tick 0 仍为假，官方 `Entity.baseTick`），
`unseenTicks` 只在 `canContinueToUse` 里按视线刷新（`TargetGoal` 没有 `tick`），去掉了
main 在 `tick_goal` 里多加的一次自增——`slime_target_height` 有石柱挡视线时会累积发散；
② `mob-oracle` 的 `cow.kind` 是全局命名空间，两分支都占了 13/14/15，本分支的史莱姆 /
女巫 / 溺尸改到 16/17/18，main 的北极熊 / 哞菇 / 熊猫保留 13/14/15，Java 侧
`MobOracle.java` 的 `.mob()` 场景号和 switch 一起改。合并后 `cargo check --workspace
--all-targets`、clippy `-- -D warnings`、`cargo fmt --check` 都通过；`mob-oracle` 按前缀
针对性对拍：本分支五种生物（natural_spawn 632 例 2528 刻、stray_spawn、bogged_spawn、
witch_spawn、drowned_spawn、witch_drink、witch_throw、slime_、drowned_、projectile）
和 main 的杂项（polar_bear_、panda_、mooshroom_）全部 0 不一致。全量 `mob-oracle` 剩
12 处 `player_death_*` 不一致，全是上述重生包 `sea_level` 既有差异（main 新增死亡消息
场景后由 6 处增至 12 处），按 `verify-scope.md` 属非关键路径，不追。

### 与 main 三次合并后的现状（op-hostile2，2026-11-21 本轮）

按队长 14:35 的要求再 `git merge main`，这次 main 到 `c9773f61`（马科骑乘、羊驼吐口水、
狼躲羊驼、剩余马科工作、扔鸡蛋孵小鸡、农场动物收尾、计时测试修复）。5 个文本冲突都按
main 的结构取并集：`README.md` 的已建模生物清单合到牛/猪/羊/鸡/马/驴/骡/羊驼/僵尸/
尸壳/骷髅/流浪者/沼骸/史莱姆/溺尸/女巫；`goals.rs` 的 `stop()` 把 main 的
`RunAroundLikeCrazy` 并进本分支的 Cube/Drowned/Witch 一组（去掉重复的 `RandomStand`）；
`mod.rs` 的 `create_navigation` 并列保留本分支溺尸两栖导航和 main 羊驼
`required_path_length`；`MobOracle.java` 的 `play()` 前缀并集和 `spawnVariants` 的
`zombie_equine_llama_wolf` 标志块、`natural_spawn.rs` 的 `Step` 取 12 列 `spawned` +
`variants` 字段（打印串补 `farm`/`slimes`/`witches`）。`generated/` 用 `extract` 重新生成：
`registries.rs` 的 `game_event` 已是 main 修好的官方注册序（无需再还原），`entity_types.rs`
保持本分支的 `CUBE_MOB_SIZE` / `SLIME_EYE_HEIGHT`。语义编译冲突补齐 main 新增字段：
`egg.rs` / `equine/tame.rs` 的 `PlayerView` 补 `delta`/`health`/`effects`，
`riding/tests.rs` 的 `MobPublic::of` 补 `in_water` 实参。

另修一个本分支既有（非本轮合并引入）的失败：史莱姆按官方
`Slime.checkSlimeSpawnRules`（`ServerLevel implements WorldGenLevel`，y<40 的史莱姆区块
不看亮度就能刷）接进运行时自然刷怪后，合成超平坦测试世界里每个区块都隐式存在、没有
真实的区块交付和实体 tick 票，环境刷出的史莱姆会污染实体跟踪、跨岛逐字节比较、弩箭等
9 个无关单测（纯 `main` 全绿、本分支合并前的 `e2243b4c` 已失败）。生产逻辑不动，按该
文件既有两处 `SPAWN_MOBS=false` 的约定，在 `Harness::build_with` 给合成测试世界默认关掉
`spawn_mobs`；`mob-oracle --only natural_spawn` 用的是独立的 `MobLevel` 夹具，不受影响。
`cargo test -p mc-bamboo-game --lib` 修后 1256 过 / 0 失败 / 13 忽略。

本轮检查：`cargo fmt --check`、clippy `--workspace --all-targets -- -D warnings`、
`cargo check --workspace --all-targets`、`cargo check -p mc-bamboo-desktop` 全过；
`cargo test -p mc-bamboo-game --lib` 1256 过 / 0 失败、`cargo test -p mc-bamboo-blocks
--lib` 780 过 / 0 失败 / 6 忽略、`cargo test -p mc-bamboo-vanilla` 101 + 8 过 / 0 失败；
官方联机探针 769/769。五种生物各前缀针对性 `mob-oracle`（`natural_spawn` 674 例 2696 刻、
`stray_spawn`、`bogged_spawn`、`witch_drink`、`witch_throw`、`slime_`、`drowned_`、
`projectile`）全部 0 不一致，`loot-oracle --entity-table minecraft:entities/drowned`
64 题 0 不一致；故意改坏 `natural_spawn` 一次能抓到不一致。全量 `mob-oracle` 的
`player_death_*` 重生包 `sea_level` 既有差异仍在，按 `verify-scope.md` 属非关键路径，不追。

### 与 main 四次合并及全量 `mob-oracle` 0 不一致（op-hostile2，2026-11-21）

按队长 16:40 要求 `git merge main`（`54ee195a`，含 Brain AI 框架、犰狳、山羊与全进程共享世界生成数据），
解决 9 个文件冲突：`MobType::ALL` 扩至 35 种，`DamageKind::panic_causes` 按官方 `DamageTypeTagsProvider`
去掉 `Thorns`、保留 `IndirectMagic` 与 `Wither`，`MobOracle` 的场景生物编号把史莱姆 / 女巫 / 溺尸移至
50 / 51 / 52（避开 main 的 13 犰狳 / 14 山羊 / 16 北极熊 / 17 哞菇），`generated/entity_types.rs`
由 `extract` 重新生成。

随后查清并修掉全量 `mob-oracle` 的三处根因：

1. **`player_death_*` 全部重生包 `sea_level`（官方 0 vs Bamboo -63）**：根因是本分支提交 `503956d7`
   把 `FakeLevel.getSeaLevel()` 从 `return -63;` 改成 `return injectedSeaLevel;`，但 `FakeLevel.create`
   通过 `Unsafe.allocateInstance` 分配对象，不执行字段初始化 `int injectedSeaLevel = -63;`，导致未走
   `playZombie` 的 `playLife` 场景里 `injectedSeaLevel` 保持 0。在 `FakeLevel.create` 显式赋
   `level.injectedSeaLevel = -63;` 后，全部 `player_death_*` 场景 0 不一致（移除 README 对应旧限制条目）。
2. **`rabbit_flee_zombie` 第 1 刻随机源分叉**：本分支早先合并把 `mob_oracle.rs` 里 `bright_outside` /
   `monsters_burn` 的判断误留成 `is_monster_scene`（仅匹配 `zombie_` / `drowned_`）而非 main 的
   `zombie_style`，导致 `rabbit_flee_zombie` 里的 `monstersBurn=false` 未生效、僵尸白天着火。把
   `slime_` / `witch_drink_` / `drowned_` 并入 `zombie_style` 并统一使用它后消除。
3. **`panda_breed` 约 6.15% 间歇性不一致（main 提交 `901b97a8` 既有）**：官方 `Panda.getBreedOffspring`
   在 `addFreshEntity` / `reseedBorn` 之前就用新生幼崽没播种的随机源掷两次 1/32 基因变异。照幼鸡
   `eggTime` 的做法，在 `spawnJson` 传出新生熊猫的 `genes`，由 `apply_born_setup` 与配对快照同步
   （基因遗传本身仍由 `playPandaGenes` 用确定性种子单独对拍）。

全量 `mob-oracle`（不带 `--only`）实测 **1773 例、170990 刻、0 不一致**；`--only natural_spawn`
**686 例、2744 刻、0 不一致**；`--only gen_spawn` **7000 例、0 不一致**。

## 蝙蝠的实施现状（cl-ambient，2026-11-22）

官方 `Bat`（`AmbientCreature` → `Mob`，不是 `PathfinderMob`）。代码在 `entity/mob/bat.rs`，状态是 `Mob.bat`（`BatState`：`DATA_ID_FLAGS` 整个字节和不存档的 `targetPosition`）。

- **属性和事实**：6 点血，尺寸 0.5×0.9、眼高 0.45；`MobCategory.AMBIENT`（消失距离 128，`removeWhenFarAway` 为真，和平难度照样存在）；叫声间隔用 `Mob` 的缺省值 80，音量 0.1，`getVoicePitch()` 乘 0.95（叫声、受伤、死亡三种声音都乘）；倒挂时 `getAmbientSound` 先取 `nextInt(4)`，不是 0 就不叫、也不取音高。
- **AI**：两个目标选择器都是空的。`customServerAiStep`（在 `Mob.serverAiStep` 里导航之后、移动控制之前）：倒挂时头上不是导体就起飞；否则 1/200 把头转到 `nextInt(360)`，4 格内有看得见的玩家（`forNonCombat().range(4.0)`，带潜行 / 隐身的可见度和视线）就起飞；起飞时发世界事件 1025（静音的不发）。飞行时目标方块不是空气或落到世界底就丢掉，没有目标、1/30、或离目标中心不到 2 格时重抽（x 两次 `nextInt(7)`、y 一次 `nextInt(6)`、z 两次），朝目标加速并转身，1/100 且头上是导体时倒挂。`MoveControl` 处于 `WAIT`，所以官方写的 `zza = 0.5F` 当刻就被清零。
- **`tick` 尾部**：倒挂时速度清零、`setPosRaw` 到这一格顶部下面一个身高；飞行时竖直速度乘 0.6（`double` 常量）。物理还是普通的 `LivingEntity.travel`（有重力）。
- **覆写成空的几个方法**：`pushEntities` / `doPush` / `isPushable`（`MobType::is_pushable`：蝙蝠不推别人，别人的推挤名单也不收它）、`checkFallDamage`（`MobType::ignores_fall_damage_check`：摔落距离不累计也不清零）、`getMovementEmission` 是 `EVENTS`（没有脚步声）。
- **受伤**：`Bat.hurtServer` 没有免疫就先醒过来再走父类。蝙蝠的实体类型是 `dontTrackDeltas`，速度包只在 `needsSync` 时发；`LivingEntity.knockback` 置的 `needsSync` 现在记在 `Mob.knockback_sync` 里，下一刻开头并进 `needs_sync`（受伤常常发生在生物自己的一刻之外）。
- **生成**：`SpawnPlacements` 登记 `ON_GROUND` + `Bat.checkBatSpawnRules`：在 `WORLD_SURFACE` 高度图之下、世界随机源 `nextBoolean` 为假、`getMaxLocalRawBrightness` 不超过 `nextInt(4)`、脚下在 `#bats_spawnable_on` 里，再加 `Mob.checkMobSpawnRules`。`finalizeSpawn` 只有 `Mob` 那一层（跟随距离加成和左撇子）。蝙蝠不是 `PathfinderMob`，`checkSpawnRules` 恒真。**26.3 的 `checkBatSpawnRules` 里已经没有万圣节的亮度窗口，也不再和海平面比较**（任务说明里的这两条是旧版本的规则），所以这里没有实现它们。
- **存档和同步**：`BatFlags`（BYTE）；元数据 `BAT_FLAGS`（下标由生成器导出，现在是 16）。可用 `/summon` 和刷怪蛋生成。

岛的拆合：新增的按实体状态是 `Mob.bat`（标志字节和一个绝对方块坐标的飞行目标，目标不存档、随实体整行搬）和 `Mob.knockback_sync`（一个布尔），都不带时钟，拆合时不用平移。

对拍：`mob-oracle --only bat`（`bat_open` 没有天花板第一刻起飞、`bat_wake` 倒挂着等玩家走近、`bat_hurt` 被牛打醒再打死、`bat_low` 在两格高的洞里飞到重新倒挂再被玩家吵醒，共 4 例 2424 刻）逐刻比位置、速度、朝向、随机源、血量、标志位、飞行目标、摔落距离、`zza`、世界事件、世界随机源和出站包，0 不一致；故意改坏（玩家晚一刻走近）能抓到。`mob-oracle --only bat_spawn`（`bat_dark`、`bat_dim`、`bat_open`、`bat_peaceful`，24 例 96 刻）0 不一致，`--break-self-test natural_spawn` 把 `bat_dim` 的方块光改成 15 能抓到。

已知差异：

- 官方 `setPosRaw` 不刷新碰撞箱，开始倒挂的那一刻到下一刻 `travel` 里的 `move` 之间，碰撞箱还停在原来的高度（最多低 0.1 格）；Bamboo 的碰撞箱始终跟着位置。只影响这一小段里的流体检测和被打飞时的碰撞。
- 击退发生在生物自己这一刻的 `aiStep` 之后时（别的生物的攻击命令落地），`needsSync` 晚一刻生效；只有蝙蝠（不跟踪速度）看得出来，表现是击退的速度包晚一刻。真实联机里生物的 `hurtMarked` 速度包本来就还没有接到同步层（既有差异）。
- 拴绳（`canBeLeashed` 为假）和压力板 / 绊线（`isIgnoringBlockTriggers` 为真）：Bamboo 的生物本来就没有拴绳，也不触发这两种方块，所以没有要做的。

## 幻翼的实施现状（cl-ambient，2026-11-22）

官方 `Phantom`（`Mob implements Enemy`，既不是 `Monster` 也不是 `PathfinderMob`）。代码在 `entity/mob/phantom.rs`、`entity/mob/phantom_spawner.rs`（刷怪器），游戏层入口在 `phantom_spawn.rs`、`sleep.rs`（`TIME_SINCE_REST`）。状态是 `Mob.phantom`（`PhantomState`：大小、`moveTargetPoint`、`anchorPoint`、是否俯冲、`PhantomMoveControl.speed`）。

- **属性和尺寸**：`DefaultAttributes.PHANTOM` 用 `Monster.createMonsterAttributes()`（`Mob.createMobAttributes()` 加攻击力 2）：20 点血、跟随 16、攻击力基础 2；经验 5，战利品表 `entities/phantom`，`#undead`（所以 `#can_breathe_under_water` 成立）。`getDefaultDimensions` 把基础尺寸 0.9×0.5、眼高 0.175 乘 `1 + 0.15 * size`；`size` 同步（`PHANTOM_SIZE`，下标 16），攻击力基础值在 `onSyncedDataUpdated` 里改成 `6 + size`。`setPhantomSize(0)` 在新建实体上不改变同步值，因此**不会**触发更新，大小 0 的幻翼保持 2 点攻击力；读档时 `setPhantomSize` 才会按读到的值改尺寸和攻击力。
- **AI**（`registerGoals` 的优先级照官方）：目标选择器只有 1 `PhantomAttackPlayerTargetGoal`，`nextScanTick` 初值 `reducedTickDelay(20)`，每 60 折半后再扫。扫描范围 64 格、碰撞箱外扩 (16, 64, 16)，按 Y 从高到低排（稳定排序），用 `forCombat`（不限制距离、要活着、不旁观、非创造、非和平、有视线）。目标选择器先于行为选择器跑，两个选择器共用隔刻判断——和其他生物一样。
  - 1 `PhantomAttackStrategyGoal`（不占标志位）：`start` 把 `nextSweepTick` 设成 `adjustedTickDelay(10)`、回到盘旋、把锚点放到目标头上 `20 + nextInt(20)` 格（低于海平面时改到 `sea_level + 1`）。每刻倒数，到 0 时转俯冲、重设锚点、按 `adjustedTickDelay((8 + nextInt(4)) * 20)` 排下一次，放 `entity.phantom.swoop`（音量 10，音高 `0.95 + nextFloat * 0.1`）。`stop` 把锚点按 `getHeightmapPos(MOTION_BLOCKING, anchor).above(10 + nextInt(20))` 抬高。
  - 2 `PhantomSweepAttackGoal`（占 `MOVE`）：`canUse` 要求有目标和俯冲；每刻把 `moveTargetPoint` 设成目标 `getY(0.5)`，碰撞箱外扩 0.2 与目标碰撞箱相交就 `doHurtTarget`、回到盘旋、发世界事件 1039（静音的不发）；撞墙或 `wasHurtRecently` 也回到盘旋。`stop` 清目标、回盘旋。猫的 16 格搜索和 `cat.hiss()` 分支没有建模：猫本身已在 main 上，但俯冲目标的 `canContinueToUse` 里要替别的实体发声，`EntityCtx` 还没有按实体发声的接口，`isScaredOfCat` 恒假（见已知差异）。
  - 3 `PhantomCircleAroundAnchorGoal`（占 `MOVE`）：没有目标或还在盘旋时运行；`start` 抽 `distance = 5 + nextFloat * 10`、`height = -4 + nextFloat * 9`、`clockwise = nextBoolean ? 1 : -1`；每刻按 350 / 250 / 450 折半的间隔重抽高度、距离（超过 15 回到 5 并反向）、角度（`nextFloat * 2π`），到点、目标点下方是实心方块、上方是实心方块时 `selectNext`。`selectNext` 没有锚点时用当前方块，角度加 `clockwise * 15°`，目标点 = 锚点 + (`distance * cos`, `-4 + height`, `distance * sin`)。
- **移动和朝向**：`PhantomMoveControl.tick` 撞墙时 `yRot += 180`、速度回 0.1；否则按目标点算水平/三维距离，最多转 4°，`yBodyRot` 跟着 `yRot`，转身小于 3° 时速度往 1.8 逼近（步长 `0.005 * (1.8 / speed)`），否则往 0.2 逼近（步长 0.025）；俯仰取 `atan2(-dy, 水平距离)`，速度按朝向和俯仰分解后与当前速度混合 20%。`PhantomLookControl.tick` 是空的；`PhantomBodyRotationControl.clientTick` 让头跟上一刻的身体、身体跟 `yRot`。`travel` 是 `travelFlying(input, 0.2F)`（水 0.02/0.8、岩浆 0.02/0.5、空气 0.2/0.91），没有重力；`onClimbable` 恒假、`checkFallDamage` 空。
- **刷怪器**：`PhantomSpawner` 在主世界每刻跑（`ServerChunkCache.tickChunks` 里自然刷怪和随机刻之后、方块事件和实体之前）。`spawnMobs && spawnMonsters && spawnPhantoms` 都开时 `nextTick--`，到 0 后先 `nextTick += (60 + nextInt(60)) * 20`，再要求 `skyDarken >= 5 || !hasSkyLight`。对每个非旁观玩家：`!hasSkyLight || y >= seaLevel && canSeeSky(feet)`，`effectiveDifficulty > nextFloat() * 3`（先取数），`Mth.clamp(TIME_SINCE_REST, 1, MAX)` 的 `nextInt(value) >= 72000`，落点 `feet.above(20 + nextInt(15)).east(-10 + nextInt(21)).south(-10 + nextInt(21))`（取数顺序 y、x、z），`NaturalSpawner.isValidEmptySpawnBlock` 通过后按 `1 + nextInt(difficulty.getId() + 1)` 生成一组；每只 `snapTo(落点, 0, 0)`，`finalizeSpawn` 把锚点设到头上 5 格、大小 0，再走 `Mob.finalizeSpawn`（跟随距离加成和左撇子各取一次世界随机数）。
- **`TIME_SINCE_REST`**（最小实现）：`Player.time_since_rest`，不睡觉的每一刻 `wrapping_add(1)`（官方是 `StatsCounter` 的 int，`awardStat` 一样环绕）；`ServerPlayer.startSleeping` 对应 `on_sleep_in_bed` 清零；`ServerPlayer.die` 对应玩家死亡清零。刷怪器只读它，并按 `i32::MAX` 夹。**没有**写原版 `stats/<uuid>.json`，重启后从 0 重新累计（见已知差异）。
- **存档和同步**：`anchor_pos`（`BlockPos` 数组，空时不写）和 `size`（int）；`PHANTOM_SIZE` 同步。`moveTargetPoint` / `attackPhase` / `PhantomMoveControl.speed` 不存档（官方也不存）。可用 `/summon minecraft:phantom` 和幻翼刷怪蛋生成。
- **岛拆合**：新增按实体状态 `Mob.phantom`（绝对方块坐标的锚点、大小、不存档的目标点和阶段、速度），拆岛随实体整行搬。`PhantomSpawner.nextTick` 是岛级状态：拆岛时两边从同一个倒数继续（官方整个主世界只有一个刷怪器），并岛时吸收方保留自己的。

公共接缝的修正：`Entity.push` 在官方会给自己和对方都置 `needsSync`；Bamboo 的 `push_entities` / `apply_push` 原来只改 `delta`，幻翼贴着玩家俯冲咬人时官方每刻都发移动包、Bamboo 不发，导致之后的相对位置包错位。现在两半被推开时都会置 `Mob.needs_sync`（位移本身不变），全量 `mob-oracle` 复跑 0 不一致。

对拍：`mob-oracle --only phantom` 6 例 534 刻 0 不一致——`phantom_spawn_night`（11 只）、`phantom_spawn_rested` / `phantom_spawn_day` / `phantom_spawn_off`（都不生成，分别验证 `TIME_SINCE_REST`、天色、规则关）；`phantom_hunt`（有玩家，走到盘旋 → 俯冲 → 咬中 1039，240 刻）和 `phantom_burn`（无玩家、露天白天，真的着火）。逐刻比位置、速度、四个朝向、血量、`hurtTime` / `deathTime` / `noActionTime`、大小、俯冲标志、目标、锚点、`moveTargetPoint`、着火刻数、实体随机源、世界随机源、世界事件和出站包。改坏确认：`--break-self-test phantom_spawn`（刷怪器整段不跑）和 `--break-self-test phantom_ai`（出生点高一格）都能抓到。全量 `mob-oracle`（不带 `--only`）1841 例 190745 刻、`--only natural_spawn` 746 例 2984 刻，0 不一致；蝙蝠仍是 0。

已知差异：

- 幻翼怕猫：`PhantomSweepAttackGoal.canContinueToUse` 的 16 格猫搜索和 `cat.hiss()` 没有建模。猫现在已在 main 上，但 `EntityCtx` 只能替自己发声，没有「让另一只实体发声」的接口；把嘶声放在幻翼位置会错位，所以整段先不做，`isScaredOfCat` 恒假，俯冲不会被猫打断。
- `TIME_SINCE_REST` 不写原版统计文件：重启后从 0 重新累计；官方从 `stats/<uuid>.json` 读回来。只影响幻翼刷怪的时间门槛（要累计 72000 刻）。
- 刷怪器按岛保存：官方整个主世界只有一个 `PhantomSpawner`（一个 `nextTick`），Bamboo 每个岛各有一份；拆岛时两份从同一个倒数继续，并岛时保留吸收方的。多岛同时有玩家时尝试次数可能比官方多。
- 客户端专用的表现不在服务端：`Phantom.tick` 的扇翅音效和 `MYCELIUM` 粒子、`isFlapping()` 的相位、`shouldRenderAtSqrDistance` 恒真都只影响客户端；Bamboo 只发官方的服务端音效 / 世界事件。
- 俯冲时玩家那一侧的推挤只做生物自己的一半（和其他生物一样）：玩家的速度变化和 `needsSync` 不结算，伤害走游戏层的 `hurt_player`。对拍里玩家是不受伤的替身，所以没有逐刻比玩家血量。
- 刷怪器按玩家顺序取数：官方按 `ServerLevel.players` 的加入顺序遍历，Bamboo 的 `region.players` 是按 `PlayerId` 排序的 `BTreeMap`；正常情况下两者一致，玩家退出重进后 `PlayerId` 可能和加入顺序不同，多个玩家同时满足条件时刷怪落点 / 数量可能和官方不同。

## 官方联机探针与每刻耗时热点修复（cl-ambient，2026-11-22）

把蝙蝠 / 幻翼接进自然刷怪后，`tools/probe.sh` 在 debug 构建下偶发掉刻（`Can't keep up!`），
导致爬柱子与移动检查被 `moved too quickly` 拉回：

- **实体阶段（`entities`）热点**：`EntityStore::replace_row` 原本在每次 `put_entity` / `with_entity_mut`
  都运行 `debug_assert!(self.sub_index_matches())`，而 `sub_index_matches` 每次从头分配并重建三套
  `BTreeMap` / `HashMap` / `BTreeSet`，对 $N$ 个实体形成每刻 $O(N^2)$ 的堆分配开销；改成零分配原地校验并移到每刻结束
  （`end_tick` / `publish_rows`）后，单生物每刻开销从 ~121–152 µs 降到 ~26–46 µs。
- **方块反查与碰撞热点**：`mc_bamboo_data::block_id_of_state` 从 1286 项二分查找改为编译期静态表
  `STATE_TO_BLOCK` 的 $O(1)$ 下标访问；`collect_block_collisions_impl` 对空气和完整方块直接短路，不再走多余形状盒查询。
- **自然刷怪阶段（`natural_spawn`）热点**：超平坦平原上 `Ambient`（蝙蝠）在地面层被 `check_spawn_rules`
  拒绝后，每刻会在全部 ~240 个可刷怪区块跑满 3 组 × 8 步尝试。将 `spawning::for_biome_id` 改为按生物群系 ID
  直接索引 `LazyLock` 数组、`nearest_player_distance_sqr` 直接遍历已收集的 `state.players`、
  `BlockState::is_valid_spawn_surface` 先测 `VALID_SPAWN` 位、`ChunkStore` 高度图与群系查询合并成单次哈希查找、
  外层循环复用 `RegionLevel` 与 `World` 后，`natural_spawn` 耗时回到与 `main` 一致的水平。
- **`Probe.java` 保持原有严格度**：撤回对位置拉回的容忍（`toleratingPullbacks` 等），只保留两项合理校准：
  门音效检查只数 `SoundSource.BLOCKS`（排除自然刷怪生物的环境音），以及把官方本来就会广播的 `damage_event`
  加入包白名单。

## 与 main 五次合并及装死回血（op-water，2026-11-21，第十轮）

按队长 14:35 追加先 `git merge main`（`33980ce0`，含女巫 / 史莱姆 / 溺尸 / 流浪者 / 沼骸、
桌面构建脚本、投掷物与药水）。8 个文本冲突按 main 的结构取并集：`MobType` 枚举自动合并成
37 种（本分支的海豚 / 美西螈 + main 的五个敌对），`ALL` 长度改 37，`attributes` / `goals` /
`target_goals` 三张表并入海豚 / 美西螈、去掉重复的 `Skeleton` arm，`removes_when_far_away`
改用 main 的 `is_enemy()` 并保留水生分支，`living.rs` 的水下呼吸合并 main 的溺尸 /
`WATER_BREATHING` 与本分支的 `max_air_supply` / `increase_air_supply` 分派，`natural.rs` 的
`checkSpawnObstruction` 保留美西螈分支。`generated/entity_types.rs` 用 `extract` 重跑后逐字节
一致；`mob-oracle` 场景编号表（1–17 / 40–43 / 50–52 / 60）两边不重复，水生走独立驱动。
Java 侧 `playNaturalSpawnScenes` 并入本分支的海豚 / 美西螈夹具（`dolphin_ocean` /
`axolotl_lush`、`clayFloor`），Rust 侧 `natural_spawn.rs` 保留 main 的 `floors` / `water_depth`
并注入 `clay_floor`。

合并后 `mob-oracle --only natural_spawn` 一度报 9 处 `axolotl_lush` 不一致：main 把 spawned
数组扩成 12 列、美西螈花色放在最后一列（index 11），本分支的 `spawn_row` 仍按 11 列布局放在
index 8。按官方夹具把 `spawn_row` 的 index 8 去掉美西螈分支、index 11 补上 `Axolotl` 后，
698 例 2792 刻 0 不一致。

本轮新写一处生物行为：美西螈装死按官方 `ValidatePlayDead.create` 每刻递减
`PLAY_DEAD_TICKS`、到 0 后下一刻复位 `playingDead`，装死一开始按 `PlayDead.start` 加
200 刻等级 0 的 `REGENERATION`（`axolotl.rs` 的 `update_playing_dead`，复用已有的
`living::add_effect` 与效果系统）。这段不取随机数、不动 tick 顺序、`NoAI` 对拍夹具不触发，
新增一个单元测试覆盖 200 刻倒计时 / 效果只加一次 / NoAI 不动，没有和官方逐刻对拍。
全量 `mob-oracle` 复核 **1784 例 174697 刻 0 不一致**，`--only natural_spawn` 698 例 2792 刻、
`natural-spawn` 1727 例、`--only dolphin` 3 例 603 刻、`--only axolotl` 4 例 804 刻、
`--only sheep_eat` 3 例 900 刻全部 0 不一致，`--break-self-test water_mob` 故意改坏抓到
12 处（RC=1）。

剩余已知限制：海豚十四个目标和 `HurtByTargetGoal` 仍是 `Unavailable`（引路、恩惠、跃水、
捡物、近战都不发生），美西螈 Brain 的传感器 / 记忆 / 活动、攻击水生怪物、繁殖（1/1200 蓝色）
没建模，装死只有单元测试没有逐刻对拍。正确接 Brain 需要改 `living.rs` 的
`custom_server_ai_step` / tick 顺序与对拍夹具（开 `NoAI=false`），按任务书「超出已定设计又必须
改动 tick 顺序的部分停下来写进汇报」留给队长决定。

## 第十一轮复核（op-water，2026-11-21）

第十轮（合并 `33980ce0` + 装死回血）提交后没有写汇报，本轮把记录补齐并在最终提交
`7975682d` 上跑完完成标准。当前 `main = 33980ce0`，`git merge main` 返回
`Already up to date`（领先 16、落后 0，合并基点就是 main HEAD），14:35 追加要求的合并已由
`219d8cb6` 提交，本轮无需再合。本轮不改动任何生物行为代码：任务书原文第 1 段剩下的海豚恩惠 /
引路、美西螈攻击 / 繁殖、桶装鱼与海龟蛋孵化逐刻场景都依赖 Brain 框架、结构定位、物品实体或
「会生成实体的随机刻」对拍设施，按「超出已定设计又必须改动 tick 顺序的部分停下来写进汇报」
留给队长决定，避免再次触发 05:00 那类回归。

最终提交上实测（java 对拍与探针按新规走 `node W:/data/bamboo-agents/tools/heavy.js`，同一时间
只跑一条重活）：`cargo fmt --check` 干净；`cargo clippy --workspace --all-targets -- -D warnings`
干净（0 warning、未加 allow）；`cargo check --workspace --all-targets` 通过；
`cargo check -p mc-bamboo-desktop` 通过；`cargo test -p mc-bamboo-blocks` 795 passed / 0 failed /
6 ignored；`cargo test -p mc-bamboo-vanilla` 101 + 8 passed；
`cargo test -p mc-bamboo-game --lib island_command` 4 passed（05:00 回归 2 未复发）。
工作区全量 `cargo test --no-fail-fast` 被 cargo shim 按 `verify-scope.md` 拦下（RC=2），没有绕过，
按提示改跑各 `-p` 测试，全量由队长合并时统一跑。java 侧：全量 `mob-oracle` **1784 例 174697 刻
0 不一致**；`--only dolphin` 3 例 603 刻、`--only axolotl` 4 例 804 刻、`--only squid` 4 例 1184 刻、
`--only sheep_eat` 3 例 900 刻（05:00 回归 1 未复发）、`--only natural_spawn` 698 例 2792 刻全部
0 不一致；`--break-self-test water_mob` 故意改坏抓到 12 处（RC=1）；`natural-spawn` 1727 例
0 不一致；官方 `tools/probe.sh`（JDK 26）一次 766/770（重锁排队期间时序抖动）、换端口重跑
**769/769**，`WORLD-DIR` 全过、正常关服。`natural.rs` 的 check_spawn_rules / `spawn.rs` 的
finalizeSpawn 水生分派在合并后完整保留。

疑点（详见本轮汇报）：装死倒计时 / `REGENERATION` 一段仍挂在 `customServerAiStep` 尾部而不是
Brain 行为里，只有单元测试、`NoAI` 夹具不触发，没有和官方逐刻对拍；海豚恩惠 / 引路、美西螈
攻击 / 繁殖仍是已知限制。按生物列状态：鱿鱼 / 发光鱿鱼、鳕鱼 / 鲑鱼 / 热带鱼 / 河豚、桶装鱼、
海龟、海豚、美西螈十类主体全部「已逐刻对拍 0 不一致」；Brain 驱动的特有行为为「已知限制 / 未做」。

## 第十二轮：合并 main（青蛙 / 蝌蚪 / Brain）后复核（op-water，2026-11-21）

本轮按「每轮开始先 `git merge main`」与 14:35 追加，先 `git merge main` 把 main 从
`33980ce0` 追到 `4ad4370c`（10 个提交：青蛙 / 蝌蚪 / 蛙卵、完整 Brain 框架层、便携桌面构建
脚本、缓存改造、山羊长跳共用等）。合并基点仍是 `33980ce0`。本轮**不新写生物行为代码**，
只解冲突、重生成 generated、按 main 结构对齐现状、复核；理由见本轮汇报第 4 节（第十一轮
已把可独立实现的水生生物主体做完并逐刻对拍，剩下的都依赖 Brain / tick 顺序，按任务书停下
汇报）。合并带来的青蛙 / 蝌蚪行为是 main 已完成且自带对拍的内容，不属本分支新写。

**冲突（5 个文件，按 main 结构 + 两边内容并入）**：
- `README.md`、`docs/mob-entity-design.md`：两边各自累计的「现状」段落。`MobType::ALL`
  由 37 种合成 39 种（本分支海豚 / 美西螈 + main 青蛙 / 蝌蚪），`--only natural_spawn`
  数字按实测更新；设计文档并入 main 新增的「5.10 青蛙和蝌蚪」节（HEAD 该处为空）。main
  自身有两个 5.10 编号（青蛙蝌蚪、中立生物地基和北极熊），按「按 main 结构」原样保留，
  未自行重编号。
- `entity/mob/bucketable.rs`（7 处）：HEAD 建模美西螈桶、main 建模蝌蚪桶，两边都保留 ——
  `bucket_item` / `kind_of_bucket` 同时收 `AXOLOTL_BUCKET` / `TADPOLE_BUCKET`，
  `save_default_data` / `load_default_data` 各写各的物种块（花色 / 年龄 / 锁定标志），
  `pickup` 音效合成三路 match（美西螈 `fill_axolotl`、蝌蚪 `fill_tadpole`、其余 `fill_fish`）。
  `is_mob_bucket` 合并后只剩 `SULFUR_CUBE_BUCKET`：美西螈和蝌蚪都已建模，都走
  `kind_of_bucket` → `spawn_from_bucket`，不再进「未建模仍取随机数」的分支。
- `entity/mob/mod.rs`（3 处）：`can_breathe_under_water` 同时保留 Axolotl 与 Frog；保留
  main 新增的 `can_be_a_baby`（青蛙恒假）并保留本分支海豚感知的 `is_ageable_water_creature`
  注释；malus 分派同时保留 Dolphin / Axolotl / Tadpole 三条。
- `entity/ctx.rs`：海龟蛋注释两边二选一，行为无关。
- **自动合并的一处真 bug**：两边都把 `MobType::ALL` 从 35 扩到 37（各加 2 种、加在不同位置），
  git 把 4 个元素都留下、长度声明却停在 `[Self; 37]`。改成 `[Self; 39]` 后
  `cargo check --workspace --all-targets` 才通过。`entity_type()` / `generated/entity_types.rs`
  的按 kind match 自动合并后已覆盖全部 39 种。

**场景编号不冲突**：水生生物走名字路由（`squid` / `fish` / `water_mob` 独立驱动，不占
`cow.kind` 数字表），main 的青蛙 18 / 蝌蚪 19 只进那张数字表；两边编号无交集。全量
`mob-oracle` 1808 例 0 不一致间接验证。`extract`（JDK 26）重新生成 generated/ 成功
（1511 结构模板 / 169 丛林神殿指令 / 协议 777），逐字节幂等、工作区无新增改动。

**最终提交上实测**（java 对拍与探针走 `node W:/data/bamboo-agents/tools/heavy.js` 共享锁，
前台跑）：`cargo fmt --check` 干净；`cargo clippy --workspace --all-targets -- -D warnings`
干净（0 warning、未加 allow）；`cargo check --workspace --all-targets` 通过；
`cargo check -p mc-bamboo-desktop` 通过；`cargo test -p mc-bamboo-blocks`
**808 passed / 0 failed / 6 ignored**（223.71s，比第十一轮 795 / 412s 更快，main 缓存改造生效）；
`cargo test -p mc-bamboo-vanilla` 101 + 8 passed；
`cargo test -p mc-bamboo-game --lib island_command` 4 passed（05:00 回归 2 未复发）。
工作区全量 `cargo test --no-fail-fast` 被 cargo shim 按 `verify-scope.md` 拦下（RC=2），
没有绕过。java 侧：全量 `mob-oracle` **1808 例 186057 刻 0 不一致**（含全部青蛙 / 蝌蚪场景
与本分支全部水生场景）；`natural-spawn` 1727 例 0 不一致；`--only natural_spawn`
**716 例 2864 刻 0 不一致**（含 frog_day / frog_warm / dolphin_ocean / axolotl_lush）；
`--only dolphin` 3 例 603 刻、`--only axolotl` 4 例 804 刻、`--only squid` 4 例 1184 刻、
`--only sheep_eat` 3 例 900 刻（05:00 回归 1 未复发）全部 0 不一致；
`--break-self-test water_mob` 故意改坏抓到 gotFish 位扰动（RC=1，每轮最多一次）；
官方 `tools/probe.sh`（JDK 26，端口 26337）**769/769 checks passed**，`WORLD-DIR` 全过、
正常关服、level.dat / 6 玩家数据 / 4 区域文件都在。`natural.rs` 的 check_spawn_rules 与
`spawn.rs` 的 finalizeSpawn 水生分派（squid / fish / turtle / dolphin / axolotl）合并后完整保留。

**05:00 两处回归根因（复核，未复发）**：回归 1 `sheep_eat` 是鱼类「离水着地扑腾」被无条件
套到陆地生物，修复在 `9f210652`（`living.rs` 用 `super::fish::is_fish` 门控）；回归 2
`island_command` seed 40 是拆 / 并岛随机源没从世界种子确定性派生，修复在 `d74cf013`。
本轮没有改流体 / 移动 / 碰撞 / 世界随机源代码，全量对拍与两个专项检查都在最终提交上通过。

疑点与按生物列状态沿用第十一轮，本轮未改动生物行为代码：装死倒计时 / `REGENERATION` 仍挂
`customServerAiStep` 尾部、只有单元测试、无官方逐刻对拍；海豚恩惠 / 引路、美西螈攻击 /
繁殖、桶装鱼 / 海龟蛋孵化逐刻场景仍是已知限制；桶装鱼、海龟蛋孵化的主体仍是
「已写未验证（无逐刻场景）」。详见本轮汇报。
