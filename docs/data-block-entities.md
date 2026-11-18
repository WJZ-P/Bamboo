# `/data` 方块实体支持范围

`/data merge block ...` / `/data modify block ...` 回写时按 NBT 键的拥有者分派。
下表按 26.3 的方块实体注册名列出常见类型；「能改」表示**该类型运行时表拥有的键**
可以读、合并、修改、删除并写回运行时状态，下一次存档/发包看到的就是新值。
「会报错」表示整条标签里有当前实现不回写的键，命令报 `command.failed`（或没有方块实体时报 `commands.data.block.invalid`），
不会静默丢数据。

> 「拥有者」是关键：同一种方块（例如箱子）的 `Items` 在游戏层的容器表里，
> `CustomName` 在方块实体从物品带来的组件表里，`id`/`x`/`y`/`z` 是位置元数据。
> `/data get block` 把这些表按官方 `saveWithoutMetadata` 的形状拼起来读出来；
> 活方块实体的 `saveAdditional` 键直接走 `archive::write_block_entity_additional`。

## 能改（常见容器 / 机器 / 展示类）

| 方块实体类型 | 可回写的键 | 拥有者 / 回写路径 |
|---|---|---|
| `minecraft:chest`、`trapped_chest`、`barrel` | `Items`、`LootTable`、`LootTableSeed`、`CustomName` | 容器表 / `block_items` 组件补丁 |
| `minecraft:hopper`、`dispenser`、`dropper` | 同上，另加 `TransferCooldown` | 容器表 / `archive::create_block_entity` |
| `minecraft:chiseled_bookshelf`、`crafter`、`shelf` | `Items`、`CustomName` | 容器表 / 组件补丁 |
| `minecraft:jukebox`、`decorated_pot`、`shulker_box`、`ender_chest` | `Items`（`Slot` 列表）、`CustomName`；`jukebox` 另有 `RecordItem` 形式的存档键 | 容器表 / `block_items` 组件补丁 |
| `minecraft:furnace`、`blast_furnace`、`smoker` | `Items`、`RecipesUsed`、`cooking_time_spent`、`cooking_total_time`、`lit_time_remaining`、`lit_total_time`、`speed_multiplier`、`CustomName` | 活方块实体：`archive::create_block_entity` → `BlockEntities::insert` |
| `minecraft:brewing_stand` | `Items`、`BrewTime`、`total_brew_time`、`Fuel`、`total_fuel`、`speed_multiplier`、`CustomName` | 同上 |
| `minecraft:campfire`、`soul_campfire` | `Items`、`CookingTimes`、`CookingTotalTimes` | 同上 |
| `minecraft:piston`（移动中的） | `blockState`、`facing`、`extending`、`source`、`progress` | 同上 |
| `minecraft:sign`、`hanging_sign` | `front_text`、`back_text`、`is_waxed`、`CustomName` | `ChunkStore::set_sign` / 组件补丁 |
| `minecraft:lectern` | `Book`、`Page` | `lectern::Tables::books` |
| `minecraft:command_block` | `Command`、`SuccessCount`、`CustomName` | `command_blocks` 表 / 组件补丁 |
| `minecraft:vault`、`sculk_sensor`、`calibrated_sculk_sensor`、`sculk_shrieker` | 存档态字段 | `archive::create_block_entity`（`PassiveBlockEntity`） |

## 会报错

| 情形 | 表现 |
|---|---|
| 目标格没有方块实体（普通石头、空气等） | `commands.data.block.invalid` |
| 标签里带 `lock`、`components`、`RecordItem` / `item` 等未建模的组件 / 内容键 | `command.failed`；Bamboo 用 `ComponentPatch` 和 `Items` 列表管单项，不接受整块替换，也不静默丢键 |
| 标签里带未知键 | `command.failed`，不静默忽略 |
| `/data remove block` 删掉 `id` / `x` / `y` / `z` | 这些键按官方是元数据，跳过；删其它键按上表分派 |

## 测试覆盖

`crates/mc-bamboo-game/src/command/data.rs` 的 `block_entity_tests` 覆盖箱子 / 木桶 /
漏斗 / 发射器 / 投掷器的 `Items`、告示牌文字、讲台的书与页、熔炉 / 高炉 / 烟熏炉的
存档工厂回写；`/data` 的 `tests` 覆盖实体存档 codec、storage 读写、玩家报错和不支持分支。
`tools/mc-bamboo-vanilla storage java` 的 `command-storage` 题用官方 `CommandStorage` 读回
Bamboo 写出的 `contents`，逐项比较 NBT。
