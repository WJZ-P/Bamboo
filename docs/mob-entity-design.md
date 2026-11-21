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
| 每刻生活 | `AbstractHorse.aiStep` / `tick` / `hurtServer` / `isImmobile` | `equine/life.rs` | `zombie_equine_*` 12 场景 14800 刻 |
| 目标表 | `registerGoals` + `addBehaviourGoals`、`Llama.registerGoals`、`RandomStandGoal` | `equine/goals.rs` | 同上（running names 逐刻比） |
| 交互 | `Horse/AbstractChestedHorse/AbstractHorse.mobInteract`、`handleEating`、`Player.interactOn` 兜底 | `equine/interact.rs` | `equineInteract` 468 题 |
| 死亡掉落 | `dropAllDeathLoot`、`dropCustomDeathLoot`、`dropEquipment` | `equine/death.rs` | `equineDeath` 40 题 |
| 元数据 | `DATA_ID_FLAGS`、`DATA_ID_TYPE_VARIANT`、`DATA_ID_CHEST`、`DATA_STRENGTH_ID`、`DATA_VARIANT_ID` | `EntityData::Horse*` / `Llama*`，index 由生成器导出 | 配对 `set_entity_data` 逐字节 |
| 存档 | `addAdditionalSaveData` / `readAdditionalSaveData` | `equine::save` / `load` | `equineSave` 12 题往返 |
| 坐骑物品栏 | `openCustomInventoryScreen`、`ServerPlayer.openHorseInventory`、`HorseInventoryMenu`、`AbstractMountInventoryMenu.quickMoveStack` / `stillValid`、`ArmorSlot.setByPlayer` | 生物一半 `equine/inventory.rs`；菜单是工作台菜单的 `Kind::Mount`（`menu/station/mount.rs`） | 单元测试（开菜单的包、Shift 取放鞍、两次装备声、超距关闭）；没有官方逐题对拍 |
| 落地声 | `AbstractHorse.causeFallDamage` 的 `HORSE_LAND` | `equine::land_sound` | 单元测试；音效包不在对拍范围 |
| 羊驼回击、吐口水 | `Llama.LlamaHurtByTargetGoal`、`RangedAttackGoal(1.25, 40, 20)`、`Llama.spit`、`LlamaSpit`、`DamageTypes.SPIT` | `goals.rs` 的 `Goal::RangedAttack` / `LlamaHurtByTarget`，`equine/ranged.rs`，口水实体 `entity/spit.rs`（存档 `archive/entity/spit.rs`） | `llama_spit_far` / `_near` 两场景各 160 刻 0 不一致（羊驼和口水的位置、速度、随机源、目标名、口水同步包、玩家血量）；`chunk-content` 两道口水存档题 |
| `followMommy` | `AbstractHorse.followMommy`（只 `createPath` 不走） | `equine/life.rs` 的 `follow_mommy` | 单元测试；`Bred` 只能从存档来，逐刻场景没覆盖 |

坐骑物品栏每次点击前从生物读回鞍、身体和箱子格，点击后把鞍位 / 身体位的每次 `setByPlayer`
按发生顺序在生物上重放（官方 `ArmorSlot.setByPlayer` 先调一次 `onEquipItem`，装备容器的
`setItem` → `setItemSlot` 再调一次，所以穿上会出两次声、取两次 `nextLong`），箱子格整份写回。
菜单的 `stillValid` 读实体表的公开快照（上一个实体阶段结束时的血量、箱子、位置）。

生物装备从六个人形槽扩成官方全部八个槽（加 `BODY`、`SADDLE`）。马科的全部状态都在
`Mob.equine` 里，跟着实体走，所以岛的拆分合并不需要新的按实体迁移。

**没做：骑乘。** 骑乘是牛、马、猪、骆驼共用的机制，但它要先定两件世界模型里没定的事
（第 8.2 节明确写了「载具和它的乘客」还没实现）：

1. **归属**：乘客是玩家，玩家在区域的玩家表里，载具在实体表里。官方 `ServerLevel.tickPassenger`
   在载具 tick 之后紧接着 tick 乘客（`rideTick` → `positionRider`），玩家控制时载具的位置来自
   `ServerboundMoveVehiclePacket`（客户端权威，`handleMoveVehicle` 做移动校验）。载具和乘客要算一个
   单位：同一个岛、同一个任务里按官方顺序跑，岛拆分时不能把它们拆开。
2. **一刻顺序**：玩家的位置变成由载具决定（`positionRider` 在实体阶段），而现在玩家的位置只在
   连接的移动包里更新。

建议的接法（待队长定）：玩家记 `vehicle: Option<EntityUid>`，载具记 `passengers`；
`MoveVehicle` 在世界线程处理输入时直接改载具行并做官方的移动校验；实体阶段载具 tick 完以后，
在落地阶段按 uid 顺序给乘客玩家 `positionRider`；岛拆分把「载具 + 乘客玩家」当成不可分的一组；
`SetPassengers` 包跟着实体同步发。马的部分（`tickRidden`、`getRiddenInput`、`executeRidersJump`、
`RunAroundLikeCrazyGoal` 的驯服）在这之后补，猪和骆驼复用同一套。

补充（2026-11-20 第三轮查证）：服务端上玩家控制的载具 `canSimulateMovement()` 为假，
`travelRidden` 只跑 `tickRidden`（朝向、跳跃蓄力清零）再把速度清零，位置完全来自
`ServerboundMoveVehiclePacket`；骑手玩家的服务端位置只在 `rideTick → positionRider` 里更新。
所以「`positionRider`」可以做成载具在算阶段发出的一条意图命令（按 uid 顺序在落地阶段写玩家表），
不必改阶段顺序；真正要定的只剩岛的拆分不能把载具和骑手拆到两边，以及玩家下线时载具跟着
`RootVehicle` 存进玩家档。

羊驼的口水是新的实体种类 `EntityKind::LlamaSpit`：生成走 `Command::ShootLlamaSpit`，
命中实体走 `Command::SpitHit`（落地时按主人当时的公开状态造 `spit` 伤害来源），
打到方块在算阶段直接 `discard`。同步层里口水和箭一样用 `InterpolationHandler.NO_OP`，
位置路径永远是线性的。`didSpit` 在 `EquineState` 里，跟着实体走，岛拆合不需要新迁移。

其余已知差异见 README「马科的已知限制」。

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
- **刷怪规则**：牛走 `Animal.checkAnimalSpawnRules`（`#animals_spawnable_on` + 原始亮度 > 8），
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
- **已知差异**：只有牛、僵尸、尸壳会真的生成；表里其它生物被挑中后照常抽组大小，
  然后结束这一组，不复现官方后续随机数和上限占用，不能保证逐刻产率一致。
  僵尸/尸壳的 `finalizeSpawn` 沿用 9.3 的实现，随机序列和
  官方不同。`doMobSpawning` 为假时整段不跑（官方仍会打乱刷怪区块列表）。区块列表按坐标顺序
  收集而不是官方 `Long2ByteOpenHashMap` 的哈希顺序，官方还在刷怪列表内穿插雷击。
  生成期刷怪、结构刷怪、刷怪笼、`customSpawners`、下界要塞表未做。
- **对拍**（`mc-bamboo-vanilla natural-spawn`）：`NaturalSpawnOracle.java` 对每个群系的每个
  `MobCategory`，用 7 个固定种子各跑 8 次官方 `WeightedList.getRandom` + `IntProvider.sample`，
  再取一个 `nextInt(1000)` 当随机源指纹；另外把 0..15 交给官方 `Util.shuffle`。
  也直接调用官方 `getRandomPosWithin`（含空列和非平面高度表）与
  `ChunkMap.euclideanDistanceSquared`，比较起点、后续随机指纹与 128 格区块距离条件。
  **没有对拍完整刷怪链路**：`spawnCategoryForPosition` 的方块/光照/距离判定与
  `finalizeSpawn` 仍未做端到端比较。
## 10. 箭实体与骷髅（期 3）

骷髅随敌对生物走同一对选择器。弓目标按视线记忆决定寻路或横移，并把生成箭作为实体命令落地；无弓切换到近战。箭不需要自己的岛表：所有者 UID、装备、插地状态和计时跟随 `Entities`，配对和生成快照仍随 `EntityTracking` / `SpawnSnapshots`。岛重定基只平移近战选择器的游戏时间戳，不改箭的寿命或拉弓计数。精确驱动用当前状态解析命中结果；并行驱动用快照选碰撞候选，落地时才确认伤害。支持范围与已知差异见 README 和 `zombie-skeleton-plan.md`。

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

扔出的鸡蛋在 `entity/thrown_egg.rs`，是新的实体种类 `EntityKind::Egg`（不是生物）：`ThrowableProjectile.tick` 先重力后阻力，沿速度找方块（含世界边界）和实体命中；命中经 `Command::EggImpact` 落地，打目标 0 点 `thrown` 伤害（新增 `DamageKind::Thrown`），再用蛋自己的随机源 `nextInt(8)` / `nextInt(32)` 孵一或四只小鸡。小鸡构造（种子、UUID、朝向、`eggTime`）在 `prepare_entity` 里取，`snapTo` 后照 `fudgePositionAfterSizeChange` 从零尺寸找空位（`findFreePosition` 用盒子切格子求最近点）。实体事件 3 走生物事件通道，蛋当刻消失时由 `keep_departed_mob_events` 留给同步层（并行驱动在命令落地后也会留）。玩家入口是游戏层 `player_egg.rs`（`EggItem.use`），发射器在 `dispense/entity.rs`。对拍：`mob-oracle --only egg`（飞行、孵一只、孵四只、打牛、玩家扔蛋，逐刻比位置、随机源、出站包、小鸡和牛），存档见 `chunk-content` 的两枚蛋。

## 鱿鱼和发光鱿鱼的实施现状

鱿鱼一支是官方的 `AgeableWaterCreature`（`AgeableMob` 但不是 `Animal`）。专属逻辑在 `entity/mob/squid.rs`，以后海豚、鱼类也要用的公共部分在 `entity/mob/water.rs`：

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
  `allowBreaching` 是假（以后海豚为真）。`BlockState::is_pathfindable_water` 来自新导出的
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
  朝向、随机源、氧气、尺寸、掉落、世界随机源、出站包逐字节）；`cod_beached` 461 刻只剩
  溺水那一刻的两组包差一拍（4 处）；`pufferfish_puff` 从第 64 刻起实体随机源相差一个
  `blow_up` 音效的取值（见下）。
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
  美西螈 / 蝌蚪 / 硫磺立方桶还没建模，只按官方 `EntityType.create` 的次数取随机数。
  验证：块级单元测试（组件拆分、只改有的键、存档往返）、游戏层两条（生存换桶、
  创造加桶）、发射器一条（生成的鱼 + 随机源种子）。没有 Java 逐刻对拍。
- 已知差异：`pufferfish_puff` 里河豚从 1 鼓到 2 时 Bamboo 的音效早官方一刻（早取两个
  `nextFloat`），第 64 刻起实体随机源就不一致；`cod_beached` 溺水瞬间两组包差一拍。
