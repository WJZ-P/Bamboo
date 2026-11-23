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
  可能范围之内。`advancement-oracle` 用默认 JVM（不加 `-XX:hashCode`），把当次官方树的
  兄弟顺序导出喂进 Bamboo 的排版算法，逐项比较全部显示节点的坐标和单条
  `UpdateAdvancements` 包字节；至少跑 3 个独立 Java 进程，每次用 salt 参数改变身份哈希
  发号序列以覆盖不同排列。报告打印三次的兄弟顺序摘要，确实不同。
  上一位的实验（`-XX:hashCode=2` 时官方两次自己比自己 0 不一致）是这条结论的旁证，
  对拍不依赖它。
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
  不接受 EULA。`--break-layout` 打乱排版里第一棵根的兄弟顺序，坐标和包字节必须报出来。
  `command-oracle` 另加 12 条 `/advancement` 语法题：Java 侧用官方 Brigadier
  `AdvancementCommands.register` 的 dispatcher 解析，Rust 侧用 Bamboo 命令树解析并比较
  `ok` / `trailing:<光标>` / `error`；反馈键按 26.3 `AdvancementCommands$Action` 拼出来的
  24 个键选。`--break-self-test` 同时改坏一个选择器和一条命令。

## 已接上的触发器

`tick`、`location`、`inventory_changed`、`recipe_unlocked`、`recipe_crafted`、
`placed_block`、`item_used_on_block`、`consume_item`、`slept_in_bed`、
`enter_block`、`filled_bucket`、`effects_changed`、`player_killed_entity`、
`entity_killed_player`、`player_hurt_entity`、`entity_hurt_player`。

条件判定复用 `mc-bamboo-loot` 的物品 / 实体 / 位置谓词和 `mc-bamboo-protocol`
的方块谓词；本层只做「条件字段能不能被现有谓词完整表达」的检查，不能表达的
一律不触发，避免把没建模的判据白送给玩家。

## 已知差异

- **排版边界**：官方 `apportion` 里 `vol` / `vor` 两个轮廓游标为 null 时的分支
  （官方数据会靠 `thread` 接上，Bamboo 在特定兄弟排列下遇到 null）尚未完全复现。
  三个 salt 排列里共 4 个显示节点的 y 差 0.5；对拍把这 4 个节点列为已知差异，
  其余 122 个逐位相同。曾用反射导出官方 `firstWalk` 之前的内部状态逐节点核对，
  126 个里 125 个的 `y` / `mod` / `shift` / `change` / `thread` 完全一致。
- **复杂图标组件**：3 个显示节点的图标带 `pot_decorations` / `banner_patterns`
  等组件，Bamboo 目前把 JSON 转成通用 NBT，没按官方 `DataComponentPatch` 的
  线上 codec 重写，单条包字节不同。其余 123 个显示节点逐字节相同；这 3 条对拍
  跳过包字节比较，坐标仍逐位比。
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
- **`function` 奖励**：Bamboo 没有函数系统，跳过。内置 26.3 定义里没有 function 奖励。
- **`loot` 奖励**：内置定义里没有 loot 奖励，接口保留但没有对拍。
- **`location` 的群系**：目前只上报主世界 `plains`，群系 / 维度谓词要靠世界生成
  接入后才能完整判定。
- **命令反馈**：键和参数已按官方字节码选，但「目标数量 × 进度数量」的 24 个键里
  只有语法和键表进了 `command-oracle` 对拍，**没有**在进程内构造官方假玩家执行
  `/advancement` 比返回值和反馈文本（任务书原本要求的那一步）。这是本期剩余的主要
  对拍缺口。
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
