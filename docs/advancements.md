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

「已接」= 有事件入口且会判定；「待接」= Bamboo 已有对应系统、这一期暂没挂事件入口；
「不做」= Bamboo 没有对应系统（下界 / 末地 / 村庄 / 钓鱼 / 袭击等），只解析不触发。
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
| `default_block_use` | 待接 | `interaction::on_use_item_on` 里有 `useWithoutItem`，尚未区分 default |
| `consume_item` | 已接 | `player_consume::complete` |
| `slept_in_bed` | 已接 | `sleep::on_sleep_in_bed` |
| `effects_changed` | 已接 | `player_effects::give` |
| `player_hurt_entity` | 已接 | `player_attack::on_attack` |
| `player_killed_entity` | 已接 | `player_attack::on_attack`（击杀分支） |
| `player_interacted_with_entity` | 已接 | `player_attack::on_attack` / `interaction::on_interact_entity` |
| `started_riding` | 已接 | `riding::mount` |
| `shot_crossbow` | 已接 | `player_weapon::shoot_crossbow` |
| `using_item` | 已接 | `player_weapon::spawn_arrow` |
| `enchanted_item` | 待接 | `command::enchant` 有附魔成功入口，尚未挂事件 |
| `tame_animal` | 待接 | `mc-bamboo-blocks::entity::mob::tame::tame` 有，尚未挂事件 |
| `bred_animals` | 待接 | `mc-bamboo-blocks::entity::mob::living::apply_breed` 有，尚未挂事件 |
| `entity_killed_player` | 待接 | `player_death::die_in` 有，借用被拆成 `HurtParts`，尚未挂事件 |
| `entity_hurt_player` | 待接 | `player_hurt::hurt_player_in` 有，同上 |
| `used_totem` | 待接 | 不死图腾判定尚未建模 |
| `killed_by_arrow` | 待接 | 箭命中实体在 `mc-bamboo-blocks::entity::arrow` 里，尚未回传玩家 |
| `item_durability_changed` | 待接 | `mc-bamboo-blocks::durability` 有，尚未挂事件 |
| `fall_from_height` | 待接 | `player_hurt::hurt_player_in` 有摔落伤害，尚未分类 |
| `thrown_item_picked_up_by_player` | 待接 | `interaction` 的掉落物拾取有，尚未挂事件 |
| `summoned_entity` | 待接 | `command::summon` / 刷怪蛋有，尚未挂事件 |
| `target_hit` | 待接 | 标靶方块尚未建模 |
| `enter_block` | 待接 | `player_environment::tick_environment` 有卡墙判定，尚未挂事件 |
| `filled_bucket` | 待接 | `inventory` 的桶交换有，尚未挂事件 |
| `crafter_recipe_crafted` | 待接 | `mc-bamboo-blocks::crafter` 有，尚未挂事件 |
| `impossible` | 不做 | 官方定义里没用到；永不触发 |
| `brewed_potion` | 不做 | 酿造台出药水的玩家归因没做 |
| `construct_beacon` | 不做 | 信标层级变化没做 |
| `used_ender_eye` | 不做 | 末影之眼 / 要塞没做 |
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

「待接」的 20 种里，绝大部分事件已经在 `Region::pending_triggers` 的框架覆盖范围内
（`record` + `drain`），补的是一个调用点，不需要另起事件系统。

## 已知差异

- **没接上的触发器**：58 种里已接 16 种（框架能解析全部 58 种，条件字段里
  没建模的谓词一律不匹配，不白送判据）。下界 / 末地维度、村民交易、钓鱼、
  驯服、繁殖、酿造、附魔、信标、劫掠、避雷针、标靶、潜影盒、刷怪蛋相关的
  触发器，以及 `default_block_use`、`any_block_use`、`using_item`、
  `item_durability_changed`、`levitation`、`changed_dimension`、`used_totem`、
  `nether_travel`、`fishing_rod_hooked`、`channeled_lightning`、`shot_crossbow`、
  `spear_mobs`、`killed_by_arrow`、`hero_of_the_village`、`voluntary_exile`、
  `slide_down_block`、`bee_nest_destroyed`、`target_hit`、`avoid_vibration`、
  `player_generates_container_loot`、`thrown_item_picked_up_by_entity`、
  `thrown_item_picked_up_by_player`、`player_interacted_with_entity`、
  `player_sheared_equipment`、`started_riding`、`lightning_strike`、
  `fall_from_height`、`ride_entity_in_lava`、`kill_mob_near_sculk_catalyst`、
  `allay_drop_item_on_block`、`crafter_recipe_crafted`、`fall_after_explosion`、
  `impossible`、`enchanted_item`、`brewed_potion`、`construct_beacon`、
  `used_ender_eye`、`summoned_entity`、`bred_animals`、`cured_zombie_villager`、
  `tame_animal`、`villager_trade`、`ender_eye` 类都只解析不触发：Bamboo 没有
  对应系统，或者现有事件入口还没把事件原文传进来。
- **`function` 奖励**：Bamboo 没有函数系统，留到第二期。内置 26.3 定义里没有 function 奖励。
- **`loot` 奖励**：内置定义里没有 loot 奖励，接口保留但没有对拍。
- **`location` 的群系**：目前只上报主世界 `plains`，群系 / 维度谓词要靠世界生成
  接入后才能完整判定。
- **触发器 oracle**：任务书要的「每个接上的触发器用官方真实触发器实例判定」没有
  单独的对拍；目前只有 `predicate-oracle` 覆盖已有的谓词实现和命令 / 存档 / 进度树
  对拍。
- **图标组件**：见上；奖励里的配方集合目前写在进度档案里，官方的
  `recipeBook` 字段在玩家 NBT，第二期配方书会一起对齐。
- **统计、遥测事件**：`sends_telemetry_event` 照官方编码进包，但服务端不上报遥测。

## 第二期（配方书）

- `ClientboundRecipeBookAddPacket` / `RemovePacket` / `SettingsPacket`、
  `ServerboundRecipeBook*` 包；
- 把奖励配方写进玩家 NBT 的 `recipeBook`（`recipes`、`toBeDisplayed`、
  `isFilteringCraftable`、`isGuiOpen`、`isSmeltingFilteringCraftable` 等字段）；
- `/recipe` 命令、`place_recipe`、合成界面里已知 / 可合成 / 新配方的显示与红点；
- 补齐 `recipe_crafted` 的配方输入条件和 `recipe_unlocked` 的解锁时机。
