# 进度系统（advancements）第一期

对应官方 26.3（`protocol 777`，`DataVersion 5023`）。本文写设计和已知差异；
配方书是第二期，本期只留接口。

## 模块边界

照配方系统的先例拆成三层：

| 层 | crate | 内容 |
| --- | --- | --- |
| 纯数据 | `mc-bamboo-advancements` | 全服只读共享树（1866 个进度、4045 个判据、126 个显示节点、5 个有显示的根 + 1 个无显示的 `recipes/root`、58 种触发器注册表）、`TreeNodePosition` 排版、单个玩家的 `AdvancementProgress` 与进度档案 JSON 编解码 |
| 协议 / 文件 | `mc-bamboo-protocol`、`mc-bamboo-storage` | 三个网络包；`players/advancements/<uuid>.json` 的读写 |
| 运行时 | `mc-bamboo-game` | 每名玩家的 `PlayerAdvancements`（监听表、可见性、增量包）、触发器接入、奖励、公告、`/advancement` 命令、进服 / 存盘 / 退出 |

树用 `OnceLock`，三种岛模式（齐步、精确、并行）共用同一份只读数据。玩家状态挂在
`Player` 上，随玩家整行在岛之间搬动，触发发生在玩家所在的岛线程上。公告走
`chat::broadcast_system_message` 的跨岛广播。`/advancement` 只读写玩家状态和出站链路，
借岛规划器把它归到 `Players` 类，不会一律退回全拼。

## 和官方的对应

- **数据加载**：和配方、战利品表一样由 `mc-bamboo-vanilla` 的生成器导出
  `data/minecraft/advancement/**`，编进 `mc-bamboo-data::ADVANCEMENTS_JSON`；
  内部保留数据包原有的对象键序和 requirements 顺序。运行时解析父子、判据、
  requirements、显示、奖励。
- **显示排版**：官方 `TreeNodePosition` 是 Buchheim/Walker 树布局
  （`firstWalk` / `apportion` / `secondWalk` / `thirdWalk`），已按 26.3 字节码
  逐条移植到 `layout.rs`。无显示节点不下沉，把它的显示后代挂到当前层。
- **兄弟顺序**：官方 `AdvancementNode.children` 是 `ReferenceOpenHashSet`（身份哈希），
  每次启动都可能不同，所以「同样的兄弟顺序进去算出同样的坐标」才有意义。
  Bamboo 正式运行选**按进度 id 字典序**这一种确定的顺序；任何一种顺序都在官方的
  可能范围之内。`advancement-oracle` 用默认 JVM（不加 `-XX:hashCode`），至少跑 3 个
  独立 Java 进程；默认 JVM 在这台机器上的身份哈希发号序列是稳定的，连续三次进程
  会给出完全相同的顺序，所以给 Java 侧加了一个 `salt` 参数：先往一个
  `ReferenceOpenHashSet` 插 `salt` 个短命对象，再构造 `ServerAdvancementManager`。
  这仍然是不带任何 `-XX:hashCode` 参数的默认 JVM，只是扰动了发号序列，三次
  （`layout 0 / 7 / 14`）因此拿到三组不同的兄弟排列。报告打印三次的顺序摘要。
  上一位的实验（`-XX:hashCode=2` 时官方两次自己比自己 0 不一致）是这条结论的旁证，
  对拍不依赖它。

  另一个坑：官方 `AdvancementNode.children` 的身份哈希集合在两个不同时刻遍历，
  顺序未必相同（`run()` 会重新构造一棵 `TreeNodePosition` 树）。所以 Java 侧
  **从跑完三步的那棵官方排版树本身**反射导出每个显示节点的直接孩子顺序，和
  `x` / `y` 一起输出；Rust 侧拿同一份顺序当输入。之前分别导出 order 和坐标时，
  少数节点的 y 差 0.5，实际是比较了两棵不同顺序的树，不是排版算法有差异。
- **存档**：26.3 字节码 `LevelResource.PLAYER_ADVANCEMENTS_DIR` 是
  `players/advancements`，所以路径是 `<世界>/players/advancements/<uuid>.json`
  （任务书里写的 `<世界>/advancements/` 是旧版本路径）。格式照官方
  `PlayerAdvancements$Data.CODEC`：`criteria`（判据名 → `yyyy-MM-dd HH:mm:ss Z`，
  本地时区）、`done`、末尾的 `DataVersion`。登录读、存盘写、退出写。
  读到未知进度照官方打日志后跳过；未知判据按当前进度定义的 requirements 清掉。
- **网络包**：`UpdateAdvancements`（首包 `reset=true`，之后增量，`added` 带坐标、
  `removed`、`progress`、`showAdvancements=true`）、`SelectAdvancementsTab`
  （可选 id）、`SeenAdvancements`（客户端打开 / 关闭标签页）。可见性照
  `AdvancementVisibilityEvaluator`：从根压栈，完成 → SHOW、无显示或 `hidden` → HIDE，
  其余 NO_CHANGE；未完成节点向上最多看两级。
- **玩家进度**：`progress` / `visible` / `progressChanged` / `rootsToUpdate` /
  `activeTriggers` 对应官方 `PlayerAdvancements` 的同名可变状态；注册与注销时机照
  `registerListeners` / `unregisterListeners`，已完成的进度不再监听。
  `flushDirty` 首包即使三类变化都空也要走一遍，但三者全空时不发包；
  退出方法前清 `isFirstPacket`。
- **触发器**：`tick` 每刻触发（26.3 字节码的 `ServerPlayer.tick` 没有模 20，
  任务书写「每 20 刻」是旧版本行为），`location` 每 20 刻触发一次。
  第一个玩家 tick 的 `recipes/decorations/crafting_table` 的无条件
  `unlock_right_away` 判据因此会完成，首包跟着带上它。
- **奖励**：经验照官方 `giveExperiencePoints`；`recipes` 记进玩家的已知配方集合并随
  进度档案存档（配方书网络包是第二期）；`loot` / `function` 见已知差异。
- **公告**：`chat.type.advancement.task/goal/challenge`，受规则
  `minecraft:show_advancement_messages` 控制，走逐玩家的跨岛广播。
- **命令**：`/advancement grant|revoke <目标> only|from|through|until|everything
  [进度] [判据]`，反馈与失败消息照官方键。
- **对拍**：`mc-bamboo-vanilla advancement-oracle java [--runs N] [--show N]
  [--break-layout]`，进程内加载官方 `ServerAdvancementManager`，不启动真实服务端、
  不接受 EULA。`--break-layout` 打乱排版里第一棵根的兄弟顺序，坐标和包字节必须报出来；
  `--break-definitions`、`--break-packet`、`--break-save` 是另外三处独立改坏确认。
  `command-oracle` 另加 12 条 `/advancement` 语法题：Java 侧用官方 Brigadier
  `AdvancementCommands.register` 的 dispatcher 解析，Rust 侧用 Bamboo 命令树解析并比较
  `ok` / `trailing:<光标>` / `error`。
  `advancement-command-oracle` 在官方假世界 / 假玩家（`ExecuteOracle.java advancements`，
  不启动真实服务端、不接受 EULA）里执行整条 `/advancement`：12 组
  grant/revoke × only/from/through/until/everything × 单判据 / 连带命令，比较反馈翻译键
  和执行后的完成进度集合；`--break-self-test` 改坏一处。反馈键按 26.3
  `AdvancementCommands$Action` 拼出来的 24 个键选，形如
  `commands.advancement.<grant|revoke>.<criterion|one|many>.to.<one|many>.<success|failure>`：
  第一段是动作，第二段是**进度数**，第三段是**玩家数**。

## 58 种触发器的接入表

「已接」= 有事件入口且会判定；「不做」= Bamboo 没有对应系统（下界 / 末地 /
村庄 / 钓鱼 / 袭击 / 不死图腾 / 标靶等），只解析不触发。
所有 58 种的判据 JSON 都能解析；条件字段没建模时一律判为不匹配，不白送判据。

| 触发器 | 状态 | 入口 / 缺失 |
| --- | --- | --- |
| `tick` | 已接 | `advancements::tick`，26.3 每刻 |
| `location` | 已接 | `advancements::tick`，每 20 刻 |
| `inventory_changed` | 已接 | `inventory::give_or_drop` 等物品栏变化 |
| `recipe_unlocked` | 已接 | 配方解锁入口（框架已挂） |
| `recipe_crafted` | 已接 | 合成结果入口（框架已挂） |
| `placed_block` | 已接 | `interaction::on_use_item_on` 放置成功 |
| `item_used_on_block` | 已接 | `interaction::on_use_item_on` |
| `any_block_use` | 已接 | `interaction::on_use_item_on` |
| `default_block_use` | 已接 | `interaction::use_held_item`：`used_without_item` 且主手、结果 consumesAction |
| `consume_item` | 已接 | `player_consume::complete` |
| `slept_in_bed` | 已接 | `sleep::on_sleep_in_bed` |
| `effects_changed` | 已接 | `player_effects::give` |
| `player_hurt_entity` | 已接 | `player_attack::on_attack` |
| `player_killed_entity` | 已接 | `player_attack::on_attack`（击杀分支） |
| `player_interacted_with_entity` | 已接 | `player_attack::on_attack` / `interaction::on_interact_entity` |
| `started_riding` | 已接 | `riding::mount` |
| `shot_crossbow` | 已接 | `player_weapon::shoot_crossbow` |
| `using_item` | 已接 | `player_weapon::spawn_arrow` |
| `enchanted_item` | 已接 | `command::enchant` 成功改完主手物品，传物品与等级 |
| `tame_animal` | 已接 | `MobInteraction::tamed` / `MobEvent::Tamed`（狼、猫、马科驯服） |
| `bred_animals` | 已接 | `Effect::BredAnimals`：幼崽落地时按求爱者 UUID 触发 |
| `entity_killed_player` | 已接 | `player_death::die_in`：击杀者实体类型经 `HurtParts` 回传 |
| `entity_hurt_player` | 已接 | `player_hurt::hurt_player_damage`：实际扣血时带伤害来源与数值 |
| `used_totem` | 不做 | 不死图腾 / `checkTotemDeathProtection` 系统 Bamboo 没有建模 |
| `killed_by_arrow` | 已接 | `Effect::KilledByArrow`：箭击杀实体时回传射手、受击者类型与武器 |
| `item_durability_changed` | 已接 | `durability::run` 比较两只手前后的 `damage` 值 |
| `fall_from_height` | 已接 | `player_environment::on_land`：落地时按下落距离与起点 y 触发 |
| `thrown_item_picked_up_by_player` | 已接 | `drops::pickup_before_entities`：玩家收下掉落物时触发 |
| `summoned_entity` | 已接 | `Effect::SummonedEntity`：南瓜图案生成铁 / 雪 / 铜傀儡（5 格）、凋灵头图案（50 格） |
| `target_hit` | 不做 | 标靶方块 / 红石信号强度系统 Bamboo 没有建模 |
| `enter_block` | 已接 | `player_environment::tick_player`：卡在方块里时传方块状态 |
| `filled_bucket` | 已接 | `Effect::FilledBucket` / `MobInteraction::filled_bucket`：装流体与装生物桶 |
| `crafter_recipe_crafted` | 已接 | `Effect::CrafterCrafted`：合成器吐出产物时给 17³ 内玩家 |
| `impossible` | 不做 | 官方定义里没用到；永不触发 |
| `brewed_potion` | 不做 | 酿造台出药水的玩家归因没做 |
| `construct_beacon` | 已接 | `Effect::ConstructBeacon`：信标激活时给范围内玩家带当前塔层数 |
| `used_ender_eye` | 已接 | `player_ender_eye::use_eye`：抛出末影之眼后传水平距离平方，按 `MinMaxBounds.Doubles.matchesSqr` 判定 `distance` |
| `cured_zombie_villager` | 不做 | 村民没做 |
| `villager_trade` | 不做 | 村民交易没做 |
| `hero_of_the_village` | 不做 | 袭击没做 |
| `voluntary_exile` | 不做 | 袭击没做 |
| `levitation` | 不做 | 潜影贝 / 漂浮效果时长没做 |
| `changed_dimension` | 不做 | 下界 / 末地维度没做 |
| `nether_travel` | 不做 | 下界传送门没做 |
| `fishing_rod_hooked` | 不做 | 钓鱼没做 |
| `channeled_lightning` | 不做 | 引雷 / 闪电实体没做 |
| `spear_mobs` | 不做 | 长矛没做 |
| `slide_down_block` | 不做 | 蜂蜜块滑落没做 |
| `bee_nest_destroyed` | 不做 | 蜂箱破坏的蜜蜂释放没做 |
| `player_generates_container_loot` | 不做 | 容器第一次开箱的玩家归因没做 |
| `thrown_item_picked_up_by_entity` | 不做 | 生物捡物品没做 |
| `player_sheared_equipment` | 不做 | 狼铠 / 剪装备没做 |
| `lightning_strike` | 不做 | 闪电实体没做 |
| `ride_entity_in_lava` | 不做 | 岩浆里骑乘没做 |
| `kill_mob_near_sculk_catalyst` | 不做 | 幽匿催发体没做 |
| `allay_drop_item_on_block` | 不做 | 悦灵没做 |
| `avoid_vibration` | 不做 | 幽匿感测体没做 |
| `fall_after_explosion` | 不做 | 爆炸后摔落归因没做 |

18 个原「待接」触发器现在都接在现有入口上：方块层新加的
`Effect::BredAnimals` / `SummonedEntity` / `KilledByArrow` / `CrafterCrafted`
把借用被拆开的事件带回 `send_effects`，其余用 `HurtParts::pending_triggers`
或直接 `record`，统一由 `Region::pending_triggers` 的 `drain` 在玩家 tick 末尾派发。
`used_totem`（不死图腾）与 `target_hit`（标靶方块）改列「不做」：Bamboo 没有
对应的判定系统，只保留判据解析。

`advancement-trigger-oracle java` 用官方 `ServerAdvancementManager` 里
`Criterion.CODEC` 解析出的**真实触发器实例**，在 JVM 内的假世界、假物品栏和
`LootContext` 上反射调用实例自己的 `matches`，对 43 组固定事件输入判定，
导出 19 种触发器共 4253 条判定；Rust 侧 `advancement_triggers::matches_trigger`
复用正式判定规则逐条复放，0 不一致；`--break-self-test` 改坏一处能报出来。
`loot` 奖励接了现有的 `LootTable::get_random_items_with_seed` +
`inventory::give_or_drop`；`location` 的群系接了 `ChunkStore::biome_at` +
`BIOME_CLIMATES`。

游戏内集成测试（`interaction/advancement_tests.rs`）走真实流程验证判据完成：
驯服狼拿到 `husbandry/tame_an_animal`、两头牛繁殖拿到
`husbandry/breed_an_animal`、水桶装鳕鱼完成 `tactical_fishing` 的
`cod_bucket` 判据，另有空桶舀水与空手右键合成器的事件记录检查。

## 已知差异

- **触发器覆盖**：58 种全部能解析并建监听；31 种在 `CONNECTED_TRIGGERS` 里
  （见表），都有明确的现有事件调用点。27 种「不做」的触发器列进表里，原因都是
  Bamboo 没有对应系统（下界 / 末地、村庄与袭击、钓鱼、悦灵、不死图腾、标靶等）。
  条件字段没建模时一律判为不匹配，不白送判据：实体谓词只实现了
  `minecraft:entity_type`，物品谓词按 ID / 标签，伤害谓词按 `blocked` 与
  `#minecraft:is_projectile`，距离谓词按 `y` 的 min / max。
- **触发器 oracle 覆盖**：`advancement-trigger-oracle` 的 4253 条判定覆盖
  19 种有固定事件输入的触发器；其余 39 种（含 27 种「不做」）没有官方实例
  判定题，判据 JSON 仍全部能解析。
- **`function` 奖励**：Bamboo 没有函数系统，留到第二期。内置 26.3 定义里没有
  function 奖励。
- **配方书**：奖励里的配方集合目前写在进度档案里；官方的 `recipeBook` 字段在
  玩家 NBT，第二期配方书会一起对齐。
- **统计、遥测事件**：`sends_telemetry_event` 照官方编码进包，但服务端不上报遥测。
- **命令执行对拍的范围**：假玩家的物品栏 / 实体数据只初始化到能跑
  `/advancement` 的程度，`grant @s everything` 会走到配方奖励里更深的字段，
  所以执行对拍用 `revoke everything` 预置状态 + `only/from/through/until`
  组题；`everything` 的语法题仍在 `command-oracle` 里覆盖。
- **群系**：`location` 现在从 `ChunkStore::biome_at` 查真实群系 ID，但超平坦
  世界里区块的群系表固定是 plains，其它群系只由已生成的噪声世界区块给出。

## 第二期（配方书）

- `ClientboundRecipeBookAddPacket` / `RemovePacket` / `SettingsPacket`、
  `ServerboundRecipeBook*` 包；
- 把奖励配方写进玩家 NBT 的 `recipeBook`（`recipes`、`toBeDisplayed`、
  `isFilteringCraftable`、`isGuiOpen`、`isSmeltingFilteringCraftable` 等字段）；
- `/recipe` 命令、`place_recipe`、合成界面里已知 / 可合成 / 新配方的显示与红点；
- 补齐 `recipe_crafted` 的配方输入条件和 `recipe_unlocked` 的解锁时机。
