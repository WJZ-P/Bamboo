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

### 5.9 中立生物地基和北极熊（2026-11-21）

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
  山羊 / 青蛙 / 犰狳 / 骆驼用官方 `Brain`（`GoatAi` / `FrogAi` / `ArmadilloAi` / `CamelAi`），
  Bamboo 只有 `GoalSelector`，没有 `Brain` / 传感器 / 记忆，这是下一步的公共地基。

### 5.10 熊猫（2026-11-21，分支 `op-misc`）

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
  幼崽金蒲公英的 `Animal.mobInteract` 兜底未接。山羊 / 青蛙 / 犰狳 / 骆驼仍等 `Brain`。

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
  实体，伤害按手持物品的 `attribute_modifiers`。
- 对拍：`mob-oracle` 原 52 场景加 36 个 `player_hurt_*` 场景，逐刻比血量 / 吸收 /
  无敌帧 / 冷却 / 饥饿 / 速度 / 随机源 / 包字节；`player-data` 增加生命和饥饿的
  非默认往返用例；五处故意改坏（难度缩放、护甲、保护附魔、无敌帧、击退）都能抓到。

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
- **攻击**：左键只对非玩家实体造成一次 `player_attack`；无攻击冷却、无暴击、
  无横扫、无附魔伤害加成、无 `minimum_attack_charge` 校验、无物品耐久 / 音效。
  PVP（打玩家）未做：官方 `Player.canHarmPlayer` 的队伍判定缺失，目前明确忽略。
  旁观者和非法目标按官方 `handleAttack` 忽略（非法目标官方会断开连接，Bamboo 目前
  只忽略，见下一条）。
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
  氧气、运行中的目标、路径节点）。产卵、孵化、回家还没有场景，也没有自然生成夹具。

## 海豚和美西螈的实施现状（2026-11-21，分支 `op-water`）

海豚是官方 `Dolphin`（`AgeableWaterCreature` = `AgeableMob` 但不是 `Animal`），美西螈是
`Animal` + `Bucketable`、官方用 Brain（`AxolotlAi`）驱动。两者专属逻辑各自在
`entity/mob/dolphin.rs` / `entity/mob/axolotl.rs`，共用的水面刷怪规则、`AgeableMob`
组数据那一层在 `entity/mob/water.rs`。

- 实体：`Mob.dolphin`（`got_fish` / `moistness` / `treasure_pos` / `stuck`）、
  `Mob.axolotl`（`variant` / `playing_dead` / `from_bucket` / `play_dead_ticks`）。
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
  一组第三只起是幼年、`BUCKET` 原因直接返回；`hurtServer` 前按官方短路判定把
  `PLAY_DEAD_TICKS` 设 200，`customServerAiStep` 映射成 `playingDead` 元数据，装死时不叫；
  两栖寻路（`AmphibiousPathNavigation`）、`travelInWater` 没有重力项、不吃水流。
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
- 验证：`mob-oracle --only dolphin`（3 场景 603 刻）/ `--only axolotl`（4 场景 804 刻），
  两边都设 `NoAI`、给同一个显式实体随机源种子、不重放构造取数，逐刻比位置、速度、朝向、
  随机源、氧气、年龄、着地 / 在水里、各自元数据、尺寸和世界随机源，**0 不一致**；
  `--break-self-test water_mob` 把 `got_fish` / `playing_dead` 改坏能在第 0 刻被抓到。
  `mob-oracle --only natural_spawn` 的 `dolphin_ocean` / `axolotl_lush` 夹具（后者把底层水换成黏土）
  各刷出 37 / 40 只，0 不一致；`natural-spawn`（全部）1727 例 0 不一致。
- 已知限制：海豚 `registerGoals` 的十四个目标和 `HurtByTargetGoal` 都按官方优先级登记成
  `Unavailable`（`canUse` 恒假、不取随机数），因为 Brain、`findNearestMapStructure`、物品实体、
  船 / 鹦鹉螺、守卫者这些设施还没接——引路去宝藏、给玩家 `DOLPHINS_GRACE`、跃水、捡物、近战
  都不发生，`gotFish` 目前只靠喂鱼设置；美西螈 Brain 的传感器 / 记忆 / 活动、装死回血、
  攻击水生怪物、繁殖（1/1200 蓝色）没建模，`PLAY_DEAD_TICKS` 设 200 后没有 Brain 逐刻递减，
  `playingDead` 不会自己复位；拴绳、刷怪蛋对着水面用、粒子 / 动画、`SWIM` 游戏事件、
  死亡掉落的逐题 loot-oracle 都没做。逐刻对拍开 `NoAI`，没覆盖 AI / 受伤 / 繁殖 / 死亡路径。
