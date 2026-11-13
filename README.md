<div align="center">

<img src="assets/icon.png" width="128" alt="Bamboo" />

# Bamboo

用 Rust 从零编写的 Minecraft 服务端

A Minecraft server written from scratch in Rust

</div>

> 项目处于早期阶段。多名玩家已经能用 Minecraft 26.3 进入同一个超平坦世界，走动、互相看见、放置和破坏方块、在生存模式里按方块硬度和工具挖方块、在告示牌上写字、用书与笔写字并署名、阅读成书、把书放到讲台上翻页、让活塞推方块、打开箱子、聊天。放置方块仍然不扣物品（往讲台上放书是例外：生存模式扣一本，创造模式不扣）。生存和冒险模式挖掉方块会按战利品表掉在地上，地上的物品能捡起来，按 Q 能丢出来。有 Anvil 存档：关服再开，改过的区块、方块实体、计划刻、三种实体、玩家位置和物品栏都还在，官方服务端能接着读这个世界目录（见「存档」一节和下面的已知限制）。

## 运行

命令行版：

```bash
cargo run --bin bamboo
```

服务器目录缺省是当前目录，里面放 `server.properties`（官方 26.3 的键和默认值）和
`bamboo.toml`（Bamboo 自己的项）；两个文件不存在时会按默认值写出来。
`bamboo --server-dir /srv/bamboo` 可以指定服务器目录，`bamboo 127.0.0.1:25566`
可以临时覆盖这一次的监听地址而不写回文件。完整说明见 `docs/configuration.md`。

桌面控制台（需要 Node.js 和 pnpm）：

```bash
cd apps/desktop
pnpm install
pnpm start
```

打包用 `pnpm tauri build`。

桌面端有总览、控制台、玩家和设置四个页面。设置页分成「官方设置」和「Bamboo 优化设置」
两个分区，读写 APP 数据目录 `server/` 下的 `server.properties` 和 `bamboo.toml`，
和命令行版完全是同一套文件。每一项都写了用途和默认值，越界的值和互斥的组合
（比如正版验证加代理转发）会当场提示。老版本的 `server-settings.json` 第一次启动时
导入一次，之后不再读它。服务端只在启动时读一次，所以改完要重启服务器才生效。

控制台顶部有 CPU 和内存监控，每秒更新一次，曲线显示最近 60 秒，参照任务管理器。
服务端跑在应用进程里，所以显示的是这个进程占整机的 CPU 和它的工作集，旁边附整机的 CPU 和内存；
WebView2 的渲染进程不算在内。

## 存档

命令行版和桌面版默认把世界存在 `服务器目录/<level-name>/`（`level-name` 默认 `world`），
用官方 26.3 的 Anvil 布局：`level.dat` / `level.dat_old` / `session.lock`、
`data/minecraft/*.dat`、`players/data/<uuid>.dat`（含 `.dat_old`）、
`dimensions/minecraft/overworld/{region,entities,poi}/` 和
`dimensions/minecraft/overworld/data/minecraft/chunk_tickets.dat`。
`ServerConfig.world_dir` 为 `None` 时不落盘（单元测试、oracle、压测都走这条路）。

一条 IO 线程负责区域文件的压缩和读写；世界线程只做 NBT 解析/序列化，绝不等待 IO：
启动时扫一遍区域文件头，得到「盘上有哪些区块」的内存集合（写盘时更新）。盘上已有
数据的区块发异步读、结果下一刻开头经信箱收回；盘上没有的区块直接在世界线程走超平坦
模板，不引入一 tick 的读盘延迟，保住进服首批次区块的顺序。改过的区块无论卸载与否都照官方字节码的节奏保存：`MinecraftServer` 每 6000 刻
一次全量（只写改过的），`ChunkMap.saveChunksEagerly` 每刻最多给 20 个改过的区块
拍快照交给 IO 线程，单区块 10 秒（200 刻）冷却；卸载的快照另走一条有上限的匀速队列。
「改过没有」用改动标记：方块和光照用 `ChunkStore` 的版本号，上层内容（容器、物品
组件、命令方块、讲台、方块实体、计划刻、实体、红石记忆）在写路径把区块放进脏集合，
每刻只碰这个集合，开销不随已加载区块数增长。没改过的区块不写盘，下次按超平坦模板
重建。快照是只读的（`BorrowedParts` 从 `&Region` 克隆），复用卸载存盘同一条编码路径，
不改运行态表的顺序、不换版本号，写出的 NBT 与卸载逐字节一致、区块保持加载。关服按官方 `stopServer` 顺序：先存所有在线玩家，
再收回并存所有区块，写 `level.dat` / `data/` / `chunk_tickets.dat`，等 IO 落盘、
`sync_all`，最后释放 `session.lock`。`/save-all [flush]`、`/save-on`、`/save-off`
按官方消息和权限等级 4 实现；`/save-all` 立刻给所有已加载脏区块拍快照交给 IO 线程、
再存玩家 / `level.dat` / `data/`，每次请求排一道自己的不阻塞 IO 栅栏，成功/失败消息
等这道栅栏的回执再发（世界线程从不等待 IO）；`/save-off` 照官方 `noSave` 停掉匀速保存、
每 6000 刻的自动保存和区块卸载，`/save-on` 恢复。写盘失败时区块重新标脏、按连续失败
次数做有上限的退避后由正常保存重试，绝不永久暂停；盒子留到写成功才释放，关服时
把还没写成功的停放盒子全部重新编码提交；到关服磁盘还在失败就 `error!` 列出没存下的
区块，照常退出。每次写盘或刷盘失败都在世界线程 `error!` 记日志（带路径和原因）并计数，
`flush` 失败作为错误报出，不当成功。IO 线程 panic 或通道断开按设计稿 §14 / R12 当成
致命错误：记日志、走关服流程把还能同步写的 `level.dat` / `data/` 存下来，不挂死。

探针和每个测试都从空世界开始：单元测试 / oracle 用 `world_dir = None` 或临时目录，
不碰世界目录；跑官方探针前删掉 `probe-run/world` 再启动 `bamboo`
（`probe-run/ops.json` 不要提交）。仓库里不留世界目录。

### 存档已知限制

- 只做 Anvil。Linear、lz4（压缩 ID 4）、自定义压缩（ID 127）、老布局转换都不做；
  读到老布局（世界根有 `region/`）当空世界并写日志（Q1/Q2/Q8）。
- 不逐次强制刷盘：写盘在 IO 线程异步做，关服才 `sync_all`（Q4）。`kill -9` 崩溃最多丢
  最近还没匀速轮到的脏改动和没落完的快照；`/save-off` 期间不写盘也不卸载。
  方块实体里不存档的运行态（容器开合人数、告示牌编辑中的玩家）改了不标脏；
  以后接结构、生物群系存档时要补上对应的标脏入口。
- 只持久化改动过的（脏）区块。没改过的按超平坦模板重建；`data/minecraft/world_gen_settings.dat`
  用官方 `WorldGenSettings.CODEC` 编出 `minecraft:flat` 生成器，层和 Bamboo 的超平坦
  模板一致，官方服务端打开这个世界后新生成的区块和 Bamboo 相同（Q9）。
- 玩家数据只恢复已建模字段：位置取整到方块（小数部分丢掉、不补发位置包）、朝向、
  游戏模式、物品栏、末影箱、选中槽。血量、经验、能力、饥饿、食物等运行时没建模，
  保存用官方默认值。
- `level.dat` 写回运行时的出生点、默认游戏模式、难度（和 `time` / `last_played`），
  启动时读回；`data/minecraft/game_rules.dat` 用官方 `GameRuleMap.CODEC` 的键
  （带命名空间、布尔 Byte / 整数 Int）写 `/gamerule` 的真实值，启动时读回；
  世界边界用 `WorldBorder::export_settings` / `restore`（`restore` 传存档里的游戏时间）
  写到官方按维度存放的 `dimensions/minecraft/overworld/data/minecraft/world_border.dat`，
  启动时读回。`data/minecraft/world_gen_settings.dat` 写官方 `WorldGenSettings.CODEC`
  编出的超平坦默认值（层与模板一致）。这些文件都用官方 codec 对拍（`saved-data` 子命令）。
  天气、世界时钟、计划事件、流浪商人、自定义 Boss 栏、末影龙战斗这些没建模的
  saved data 不写文件（官方 `computeIfAbsent` 会自己造默认值）；`/weather` 不写存档。
  `chunk_tickets.dat` 写空 `tickets` 列表（无 `/forceload`）。
- 盘上 `Status` 不是 `minecraft:full` 的区块、或读坏的区域文件，都当坏块 warn + 用超平坦
  模板，不走官方的重新生成流水线，也不另存区块备份。
- 玩家文件名照官方 `getStringUUID()` 用带连字符 UUID；坏玩家文件另存
  `<uuid>_corrupted_<yyyy-MM-dd_HH-mm-ss>.dat`（官方 `FileNameDateFormatter`，Bamboo
  按 UTC 算，官方按本地时区）。`/save-all` 的成功消息不带 `flush` 时在快照都交给 IO
  线程后发、带 `flush` 时等 IO 栅栏回执再发，都不在请求入队那一刻发（比官方同步保存晚，
  但世界线程不等待 IO）；拍快照时临时取出盒子再放回，期间会把光照算完、区块包缓存重新编码。
- 火把抖动表、容器开合状态与计数、方块实体的 `dead` / `recheck_at` 等瞬时状态不存；
  世界随机源 / 随机序列、计分板 / 队伍 / Boss 栏也不存档（沿用原有限制）。

## 现在能玩什么

- 多名玩家进入同一个超平坦世界，默认走离线模式登录。
- 玩家位置不再照单全收。一个 tick 里离本 tick 起点太远会拉回（`moved too quickly`），
  位移和方块碰撞对不上、或者新位置穿进实心方块也会拉回。生存和冒险模式悬空超过 80 tick
  会按 `multiplayer.disconnect.flying` 断开。创造和旁观可以飞。`allow-flight` 没有配置项，按官方默认关闭。
- 世界边界按官方 26.3 做全了：中心、大小、渐变（lerp）、伤害量、安全区、警告时间、警告距离和
  绝对最大值都可调，`/worldborder` 的全部子命令、等级 2 权限、成功和失败消息照官方
  `WorldBorderCommand`。进服序列在官方 `PlayerList.sendLevelInfo` 的位置发 `initialize_border`，
  改动按 `BorderChangeListener` 广播五个包（伤害两项官方就不发包）。数学（距离、`isWithinBounds`
  的几种重载、`clampToBounds`、碰撞形状、`isInsideCloseToBorder`）和逐刻渐变、六个包的编码都用官方
  `WorldBorder` 类在 JVM 里对拍。方块交互、放置、破坏、活塞、下落方块、桶、床的
  `getWorldBorder().isWithinBounds` 调用点、三种实体和玩家移动的边界碰撞都读真正的状态。
  状态按维度归 `Global`（设计稿 6.6 的全局状态），region 每刻取一份只读快照；渐变在
  `ServerLevel.tick` 的 "world border" 阶段、天气和时间之前推进。边界外照官方
  `LevelChunk.isTicking` 只停方块实体：按方块自己的坐标判断，跨边界的区块里界内的照跑、
  界外的暂停，边界回来后从原进度接着走；计划刻、流体刻、方块事件、随机刻和实体官方都不看
  边界，Bamboo 也照常跑。缩小、扩大、移动和渐变每刻只换一个整数方块盒，睡着的方块实体
  只查新旧盒子之间那一圈区块，`border-oracle` 逐刻比较这个方块盒和官方 `isWithinBounds(BlockPos)`。
- 区块跟着玩家走，Tab 列表、玩家互相可见、潜行、冲刺、挥手、手持物同步。
  Tab 列表的延迟每 601 tick 广播一次，数值是心跳往返时间的加权平均，公式和官方一样。
  心跳 id 和正在等的那次对不上，或者当前没在等心跳，都按官方 `disconnect.timeout` 断开。
- 创造模式的破坏和放置，生存模式的挖掘：按方块硬度和手里的工具攒进度，带对了工具快 30 倍，水里和空中更慢，
  松开太早只是留下裂纹，旁边的玩家能看到裂纹和挖掘声音。冒险模式按主手物品的 `can_break` 组件判定能不能挖，
  放置时按 `can_place_on` 组件判定点到的方块能不能放。旁观模式右键按官方分支：能打开的容器打开，
  传送门没有目的地时提示 `spectator.cannot_teleport`，其余不处理，对着空气使用也直接放过。
  挖掘速度和进度用官方代码逐状态对照，方块的 `attack`（红石矿点亮、龙蛋传送、音符盒发声）和其中的随机数也按官方逻辑执行。
  不在注册表里的状态 ID 不当成空气：挖掘进度是 0，也不算带对了工具。官方传入的总是注册表里的方块状态，这是本地防御。
  全部官方方块类都移植了：朝向、连接、支撑和右键走官方逻辑，用官方代码做 oracle 对比验证。
- 告示牌能写字：放下后给放置者打开编辑界面，右键重新编辑，按玩家面向哪一面决定编辑正面还是背面。
  染料、发光墨囊、墨囊和蜜脾按官方的条件处理。两面文字、颜色、发光和上蜡跟方块实体数据一起发给
  附近的玩家，晚到的玩家从区块包里读到同样的内容。
- 书与笔：保存时把页面写进 `writable_book_content`，署名时换成成书，写下标题、玩家名、代数 0
  和已解析的页面。同步走当前打开的菜单：没开容器时是背包（容器 ID 0，槽位是背包菜单的下标）；
  开着箱子时是箱子菜单的容器 ID 和箱子里的那一格。副手不在箱子菜单里，这时不会另发一格。
  没有聊天过滤，所以不写 `filtered`。右键成书会把未解析的 `written_book_content` 标成 `resolved`
  并同步这一格，然后发 `open_book`；已经解析过的只发 `open_book`。选择器、计分板和 NBT 留着原文。
  书与笔右键也是成功，但不发 `open_book`。讲台只接受 `lectern_books` 里的书，放上去的是带组件的那一本，
  生存模式扣 1，创造模式不扣。有书时右键打开讲台菜单：上一页、下一页、跳页和取书都走
  `container_button_click`，页码用 `container_set_data` 同步，取书放回物品栏。翻页把 `powered` 拉高两刻，
  比较器按当前页和总页数读。客户端的讲台书模型只看 `has_book` 和朝向，不看方块实体。
- 工具对方块的右键：斧子去皮、刮铜、去蜡，锄头耕地，锹压土径，打火石和火焰弹点火、点燃 TNT、点亮营火和蜡烛、点亮下界传送门，
  蜜脾给铜涂蜡，剪刀让藤蔓停止生长，末影之眼装进末地传送门框架并打开传送门。
- 水和岩浆会流动：往下落、往四周流、断了源头会干，两格水源中间会生成新的水源，冲掉花草这些方块，
  岩浆碰到水变成黑曜石、圆石或者石头，灵魂土上碰到蓝冰变成玄武岩。水底的灵魂沙和岩浆块会长出气泡柱。
- 桶：空桶舀起水、岩浆、细雪和含水方块里的水，满桶倒进空地或者含水方块，鱼桶倒出水。交还物品走
  `ItemUtils.createFilledResult`：创造模式倒完手里还是原来的满桶，舀起来的满桶在物品栏里还没有时才放进去；
  生存模式消耗一个，手里空了就换成结果，放不下的按 `Player.drop` 掉在地上。只有生物、没有流体的桶（硫磺立方桶）不在这里消耗。
  舀起的声音用方块上导出的 `BucketPickup.getPickupSound`（`PLAYERS`，在玩家脚下，跳过这名玩家）。倒出的声音用生成器从官方导出的
  `playEmptySound`：水桶和岩浆桶是 `BLOCKS`，生物桶是各自的 `emptySound` 和 `NEUTRAL`，都在方块中心、跳过玩家。
  鱼桶倒出的只有水，没有鱼；硫磺立方桶不放方块，只放自己的倒出声。生物的 `playAmbientSound` 不发。
- 方块音效：放下方块、开门和活板门、按钮和拉杆、栅栏门、音符盒（含头颅的自定义音效）、钟、唱片机（放入 1010、取出和拆掉 1011）、
  活塞伸出和收回、箱子、铜箱子、木桶、末影箱和潜影盒的开合、投掷器的 1000/1001/2000、空发射器的 1001、
  TNT 点燃（红石、打火石，以及火烧掉 TNT 时的 `prime`）、未氧化的铜傀儡雕像被斧头唤醒时的生成音效、炼药锅、桶、流体、营火和蜡烛、
  中继器和比较器、讲台、重生锚、堆肥桶、蜂巢、錾制书架、告示牌染色和上蜡、铜去锈和涂蜡，
  以及斧、锄、锹、剪刀、打火石和蜜脾这些右键改方块的音效，都按官方 `playSound` / `levelEvent` 发
  `ClientboundSoundPacket` 或等级事件包：音量和音高来自方块的 `SoundType`，随机音高的抽取顺序和官方一样；
  广播范围按 `SoundEvent.getRange`，等级事件和方块事件是 64 格。官方把玩家传进 `except` 的（放置、开门），动手的玩家不在广播里；
  官方传 `null` 的（活塞、容器开合、音符盒）附近的人都能听到。
- 随机刻：模拟距离内的区块每 tick 按官方 `ServerLevel.tickChunk` 的顺序挑方块。每个区块段挑几格由游戏规则 `random_tick_speed` 决定，默认 3。单个区块内的取数顺序和官方逐位相同；
  多个区块之间官方按哈希顺序遍历，Bamboo 按坐标顺序，每个区块拿到的数不同，机会相同。
  作物和瓜茎长大、甘蔗和仙人掌长高、竹子长节、海带和下界藤蔓生长、洞穴藤蔓结浆果、甜浆果和可可成熟、下界疣、
  草方块和菌丝长草、菌岩蔓延、树叶枯萎、耕地变干变湿、铜氧化、紫水晶芽长大、钟乳石生长、冰和雪融化、
  红石矿熄灭、珊瑚死亡、火蔓延和熄灭、岩浆点燃附近能烧的方块都按官方逻辑。火能不能蔓延读 `fire_spread_radius_around_player`（默认 128，`-1` 表示不看玩家距离）。藤蔓蔓延读 `spread_vines`，关掉时不取那次 `nextInt(4)`。
  天气也接进了随机刻：下雨时 `tickChunk` 掷中的那一格按官方 `tickPrecipitation` 结冰、积雪（读 `max_snow_accumulation_height`）、让炼药锅接雨接雪；
  打雷时先对所有区块跑官方 `tickThunder` 的 `nextInt(100000)`，命中后照取落点那一格，不生成闪电实体、骷髅陷阱马，也不给苦力怕充能，再跑全部区块的 `tickChunk`。
  晴天路径仍与官方逐位相同；方块 oracle 默认跑晴天、下雨、打雷三批题。`isRainingAt` 按真实天空光、MOTION_BLOCKING 高度图和区块里的群系调色板算，群系温度和降水类型由 `mc-bamboo-worldgen` 用官方 `Biome.getTemperature` 的算法算出（含固定种子的噪声），不写死平原。
- 下落方块和点燃的 TNT：沙子、砂砾、铁砧、混凝土粉末、滴水石这类方块到了计划刻会变成实体往下掉，
  落地再放回方块，铁砧按摔落距离可能裂开。打火石、火焰弹、火和红石信号会把 TNT 点成引信 80 刻的实体。
  爆炸按官方射线和抗爆性破坏方块，会掉东西的方块按战利品表生成物品，并连锁点燃别的 TNT。位置、速度和落地结果用官方
  `FallingBlockEntity.tick` 和 `PrimedTnt.tick` 逐刻逐位对比。
- 骨粉：作物、瓜茎、甜浆果、可可和海带按官方成功率和生长量催熟，创造模式不消耗。
- 光照和官方一样：屋顶下面的天空光从四边漏进来，玻璃透光，树叶和水减弱光，半砖、楼梯这些方块按形状挡光，
  火把、萤石、岩浆这些方块发光。区块包带着真实的天空光和方块光，光照变化按官方的规则发给玩家。
  作物、蘑菇和阳光传感器看的是真实亮度，阳光传感器只看天空光。光照引擎和官方逐格对比验证。
  一次改很多方块时，光照和官方一样比方块晚出来：每 tick 只做固定的工作量（release 构建约 10 ms），
  剩下的留到后面几个 tick，tick 不会被拖住。
- 区块包带着官方要发给客户端的高度图：世界表面、挡运动、挡运动不计树叶三种。放上方块、挖掉方块时会跟着改，
  树叶、水和玻璃这些方块三种高度图的算法不同，都和官方一样。区块段头里的方块数和流体数也按官方的方式算，
  客户端靠它们跳过空段。
- 创造模式物品栏、中键选取、F 换手、Q 丢弃。中键按官方 `getCloneItemStack`：没有物品的方块
  （传送门、末地传送门、末地折跃门、霜冰、移动中的活塞）选不中；作物给种子，洞穴藤蔓给发光浆果，
  花盆给里面的植物，高海草给海草，活塞头给对应的活塞，瓜茎给种子，藤蔓的茎给头部的物品，
  竹笋给竹子，蜡烛蛋糕给蛋糕。`light` 带上亮度，测试方块带上模式，蜂箱和蜂巢在 Ctrl+中键时带上
  蜂蜜等级，铜傀儡雕像带上姿势。旗帜和饰纹陶罐照旧带上自己的组件。Ctrl+中键另外带上方块实体的数据。
- 容器：箱子、陷阱箱、铜箱子、木桶、潜影盒和末影箱都能打开、存取，陷阱箱和铜箱子算箱子的一种。
  漏斗、发射器、投掷器和合成器也有内容，右键能打开菜单。箱子拼成大箱子时是 6 行菜单，末影箱的内容每个玩家一份。
  漏斗每刻先往朝向那一格的容器送一件，再从上面一格的容器吸一件，送出去或吸进来之后冷却 8 刻，
  被红石充能（`enabled` 为假）时不搬东西；双箱按合并后的 54 格算，潜影盒按 `WorldlyContainer` 的面收发货。
  堆肥桶也是容器：等级不到 7 时只从上面收一个能堆肥的物品，放进去就按堆肥概率决定升不升级
  （升不升级都消耗这一个），等级 7 不收也不给，等级 8 时从下面给出一个骨粉，取走后清空。
  比较器读堆肥桶仍按方块状态里的等级，不看这张临时容器。
  投掷器被红石触发时从 9 格里按官方 `getRandomSlot` 挑一格，往朝向的容器里塞一件；
  朝空气时按官方 `DefaultDispenseItemBehavior.spawnItem` 取速度随机数、生成物品实体，再发 1000 和 2000；九格都空时发 1001。
  发射器被红石触发时按官方 `DispenseItemBehavior` 发射
  （桶、骨粉、打火石、潜影盒、雕刻南瓜、凋灵骷髅头、玻璃瓶、水瓶、蜜脾、荧石、剪刀、装备，
  以及 `tnt_explodes` 为真时的 TNT），
  成功发 1000，失败发 1001，两种都再发朝向的 2000。空的发射器只发 1001。
  默认抛出和塞不回发射器的余料都会在 `getDispensePosition` 生成真的物品实体：位置是方块中心沿朝向外 0.7 格，
  速度取官方 `spawnItem` 的一个 `nextDouble` 和三个 `triangle`，这套随机数只取一次。
  TNT 生成点燃的实体并播放 `entity.tnt.primed`。箭、船、矿车、盔甲架和刷怪蛋仍见已知限制。
  目标正好是空漏斗时按官方规则给它排 8 刻冷却。
  合成器被信号触发后隔 4 刻走官方 `CrafterBlock.dispenseFrom`：查 3×3 配方，没有配方或产物为空只发 1050；
  成功时把 `crafting` 亮起六刻（方块实体滴答器到时关掉），产物和剩余物按 `HopperBlockEntity.addItem` 放进
  朝向的容器，放不下的走 `DefaultDispenseItemBehavior.spawnItem` 并发 1049、2010，九格各减一个。
  朝空气吐出的东西在取完官方要用的 7 个随机数后消失（世界里没有掉落物），成就
  `CRAFTER_RECIPE_CRAFTED` 和地图的 `onCraftedBySystem` 后处理没做。
  合成器右键打开 `CrafterMenu`：九格加玩家物品栏，结果格只显示不能取出，`ContainerData` 的
  九个禁用位和 `triggered` 同步给客户端；`container_slot_state_changed` 按官方规则只切换空格。
  比较器读容器时按官方 `getRedstoneSignalFromContainer` 算：箱子（含双箱）、陷阱箱、铜箱子、木桶、潜影盒、
  漏斗、发射器、投掷器和饰纹陶罐都有信号，内容变化时会重新读。
  雕纹书架、唱片机和合成器也接进了漏斗和投掷器，比较器不走满度：书架读最近动过的那一格加一，
  唱片机读唱片的 `comparator_output`，合成器读非空或禁用的格子数。
  熔炉、高炉和烟熏炉是 `WorldlyContainer`：上面进原料，侧面进燃料（燃料格里已经是空桶时不再收空桶），
  下面取产物，燃料格只能取出空桶和水桶。酿造台上面进原料，侧面进烈焰粉和空瓶，下面取瓶子，
  材料格只能取出玻璃瓶。比较器按 3 格或 5 格的满度读。架子是普通 `Container`，三格都能进出；
  比较器只从背面读，三格占用分别是 1、2、4，不是满度。
  唱片机的那一张唱片和陶罐的那一格存在方块实体的物品栏里，玩家右键和漏斗读写同一份。
  多人同时打开同一个容器时看到的是同一份内容，箱子的开合人数通过方块事件发给附近的玩家（盖子的动画）。
  菜单支持拾取放下、Shift 快速移动、数字键交换、F 和副手交换、中键克隆、Q 丢弃、拖动分配和双击收集。
  菜单的槽位、`stateId` 和官方菜单代码逐槽、逐个状态对比验证。容器和活塞共用一套方块事件队列，
  先后顺序和官方 `ServerLevel.blockEvents` 一样；开合人数在随机刻之后的方块事件阶段执行。
  木桶开合走完整的写方块路径（改 `open` 状态并更新邻居和形状），潜影盒的盖子由方块实体 ticker
  每刻推进，碰撞形状和支撑面跟着开盖进度走。
- 代理转发：Velocity 的现代转发在登录时用 `velocity:player_info` 频道询问，用配置里的密钥验证
  HMAC-SHA256 签名；BungeeCord 的旧式转发从握手地址里读 NUL 分隔的 IP、UUID 和属性。
  UUID、名字和皮肤都由代理决定，皮肤跟着登录成功包和 Tab 列表发给其他玩家。默认关闭，
  关闭时和离线模式完全一样，握手地址里的 NUL 只是普通文本。
  配置是 `ServerConfig` 里的 `forwarding`（`Off`、`Legacy`、`Modern`）和 `forwarding_secret`；
  `Modern` 没有密钥时启动会退回 `Off`，桌面端的设置页里也能改。
- 聊天，加入和离开消息。刷屏的玩家和官方一样会被踢。代码里按官方 `server.properties` 的 `chat-spam-threshold-seconds` 默认值（10）计算，不是游戏规则；这个键本身还没实现，改它暂时不起作用。
  发送者不是全零 UUID 的聊天发 `player_chat`。没签名时签名位是空的，正文按 `PlayerChatMessage.unsigned`
  （时间是现在、盐是 0、已读列表是空的）。全零 UUID 的系统消息（不带签名的 `chat_command`）仍是 `disguised_chat`。
  不可信的广播日志带 `[Not Secure]`，格式和官方 `PlayerList.logChatMessage` 一样。
  `enforce-secure-profile` 默认 true，但只在 `online_mode` 打开、并且服务公钥已经能验玩家钥匙时才生效
  （官方是发现文档里的 `getPublicKeys`，正式环境是 `https://api.minecraftservices.com/publickeys`；
  session host 不是 `sessionserver.mojang.com` 时改问同一个源，这样探针不访问外网）。
  生效时 `login` 和状态 JSON 的 `enforcesSecureChat` 为 true，钥匙后来才到也会立刻按这个判断。
  `chat_session_update` 用 `SHA1withRSA` 验玩家公钥（过期时间、公钥 DER、Mojang 的签名必须属于这名玩家），
  失败按官方翻译键断开。验过之后广播只有 `INITIALIZE_CHAT` 的 `player_info_update`。
  签名聊天和带 `message` 参数的签名命令（`/say`、`/me`、`/msg`）走消息链和 `LastSeenMessages`：
  链断了、签名错了、时间倒退只给发送者发红色系统消息；已读窗口不合法则断开。
  合法的签名消息按接收者打包成 `player_chat`，对方客户端能验过。
  取公钥在连接任务里按 authlib `RetryableFetch` 重试：失败后 5、10、20、40、80、160 分钟，之后一直是 320 分钟，没有次数上限；
  成功后 24 小时再刷新。第一次不论成败都放行登录。失败时保留上一份钥匙，安全聊天保持关闭、服务器照常开。
  没有发现文档。离线模式不去取公钥。
  没有聊天过滤和举报。`delete_chat` 的编码在，原版专用服务器不会发，这里也不发。
  所有非 overlay 的系统消息（命令成功/失败与错误位置、给其他 OP 的 `chat.type.admin`、加入/离开提示、
  `/gamerule`、`/random`、签名命令被拒等）都照官方 `ServerPlayer.sendSystemMessage` 看可见性：
  聊天设成 `HIDDEN` 时不发，`overlay=true` 的动作栏消息照常发；「聊天已关闭」的提示照官方
  `tryHandleChat` 直接发、不看可见性。被踢但还没 `Leave` 的玩家不再进聊天广播名单。
- 网络压缩：登录末尾告诉客户端阈值（默认 256，负值关闭），之后两个方向都按官方格式收发压缩帧，
  小于阈值的包原样发。压缩只在连接任务里做，世界线程交出的帧本身不变。
- 正版验证和加密（`online_mode`，默认关闭）：启动时生成 RSA 1024 密钥对，收到 `hello` 后把公钥和 4 字节挑战码发给客户端，
  客户端回来的共享密钥和挑战码按官方解密校验，密钥包之后两个方向都走 AES/CFB8，加密在最外层，连长度前缀一起加密。
  serverId 哈希用官方那套带符号的十六进制 SHA-1，再按官方格式问 session server 的 `hasJoined`，
  打开 `prevent-proxy-connections` 时带上客户端 IP。失败的分类照 authlib 来：连不上、超时和 5xx 是
  「验证服务器不可用」，其余（4xx、304、读不出档案、回复点名了几种已知错误）都算没验过，
  两种情况各用官方的翻译键。连接和读取各 5 秒，也和 authlib 的两个常量一样。
  问 session server 在连接任务里做，世界线程不等这段 IO。session server 给的 UUID、皮肤和签名原样进 Tab 列表。
  和代理转发互斥，两个都开时转发赢。
- 物品的数据组件能完整解码和编码：122 种组件每种一个类型，格式照官方每一种的 `StreamCodec`，
  包括文本组件（线上是 NBT）、`Holder` 和 `HolderSet`、方块谓词、嵌套的物品堆（容器、收纳袋、
  弩的弹药）和带 NBT 的组件。NBT 也补齐了解码，带官方的配额和深度上限。
  每个物品的默认组件由生成器从官方导出，按组件类型 ID 排好序存成一串字节，第一次用到时解开缓存；
  物品堆的合法性校验和官方 `ItemStack.validateStrict` 一样，看的是默认组件被补丁覆盖之后的结果。
  用官方代码出题比对：全部物品的默认组件、创造模式物品栏的全部物品，加一批手写的组件，
  解码再编码逐字节相同，官方也能把 Bamboo 写出来的字节解回同样的内容；
  按 ID 查表的那 19 个枚举还逐个试越界的 ID，折回的结果和官方一致。
  存档用的 `Codec`（DFU，写成 NBT）也按官方补齐了：`DataComponents` 里注册的每一种，
  包括没有网络编解码的 `lock`、`container_loot`、`map_decorations`、`debug_stick_state`、`recipes`。
  `ItemStack.CODEC`、`DataComponentPatch.CODEC`（键前面加 `!` 表示移除）和 `ItemStackTemplate` 都有。
  `container_click` 的 `HashedStack` 用 `HashOps`（CRC32C）对每项组件跑这份存档 `Codec`，
  得到的整数和官方 `HashedPatchMap` 一样比较。SNBT 的解析和打印照 `TagParser`、`StringTagVisitor`
  和 `SnbtPrinterTagVisitor`，命令还没有接上。同一批物品堆的存档 NBT 和哈希，以及一批 SNBT，
  也交给官方比对过。
- 组件接进了玩法：客户端放进创造模式槽位的组件照官方 `ItemStack.validatedStreamCodec` 逐项解析，
  解不开就断开连接（官方抛 `DecoderException`）；数量超过上限的那一格和官方一样静默忽略。
  校验过的组件发给所有人：`set_equipment`、容器槽位和物品栏都带着组件，所以别人看得到附魔的光
  和改过的名字。那两类没法完全校验的组件只发回给物品的主人：它自己的物品栏、光标、末影箱，
  以及它自己点开、自己放进去的容器格。堆叠上限、能不能叠（`isStackable`）和「是不是同一种东西」
  （`isSameItemSameComponents`）都按解析好的组件算，改过 `max_stack_size` 的物品、
  改过名的物品不会错叠在一起。
- 方块实体带上物品里的数据，照官方 `BlockEntity.applyComponents` 和 `collectComponents`：
  旗帜的图案和名字、头颅的档案和音符音效、饰纹陶罐的纹样、容器的自定义名（就是菜单标题）和内容
  （放下潜影盒时 `container` 组件里的东西进方块实体）。旗帜、头颅和陶罐要渲染，所以把官方
  `getUpdateTag` 编成 NBT，区块包和 `block_entity_data` 都带上它，和告示牌一样。
  创造模式 Ctrl+中键（官方的 `includeData`）把这些组件还给物品，告示牌还会带上 `block_entity_data`。
  方块实体是共享的，所以中键时那几种没法完全校验的组件不还（见下面的已知限制）。
  探针里官方客户端拿着这些组件和 NBT，用官方的 `Codec` 解回来逐项比过。
- 红石信号、邻居更新和计划刻按官方逻辑计算（计划刻带优先级）：导线、火把（含烧坏）、中继器、比较器、侦测器、按钮、拉杆、红石灯、铜灯、门、活板门、栅栏门、音符盒、唱片机、动力铁轨、发射器、投掷器和合成器的计划刻都能用。
  漏斗由方块实体滴答器每刻驱动（冷却 8 刻），投掷器被信号触发后隔 4 刻执行一次 `dispenseFrom`。
  音符盒上面是头颅时按官方 `NoteBlock.getCustomSoundId` 读头颅的 `note_block_sound`：有音效才发声，
  并消耗一个随机数，声音按 `SoundEvent.createVariableRangeEvent` 内联进音效包（`Effect` 仍是 `Copy`，名字记在下标表里）；
  没有音效就什么都不做，也不碰随机源。音符粒子仍靠方块事件，官方客户端在自己的 `triggerEvent` 里放。
- 活塞和粘性活塞：信号变化时排方块事件，下一刻开始推；最多推 12 格，粘液块和蜂蜜块会带上一串方块，
  黑曜石、基岩这些推不动的挡住整排，花、火把这类会被弹掉。要移动的方块换成 `moving_piston` 和方块实体，
  每刻推进半格，到一格时把方块放回去并更新邻居，动画期间被破坏也有官方的处理。官方客户端会收到方块事件，
  播放伸出和收回的动画。`PushReaction` 和推不动的方块来自官方导出的数据。

世界生成的数学已经移植好了；天气只用到了其中的群系温度，地形生成**还没有接进游戏**，世界还是超平坦：

- `mc-bamboo-worldgen` 是「种子 + 坐标 → 数值」的纯函数：官方的随机源（`XoroshiroRandomSource`、
  `RandomSupport` 的种子混合、两种 `PositionalRandomFactory`、`WorldgenRandom` 的种子设置）、
  `synth` 下的全部噪声、全部密度函数类型和三次样条、气候参数的 R 树查找，
  还有主世界、下界、末地的群系源。给一个种子和一个坐标，能算出噪声路由的八个输出和那一格的群系。
- 官方的密度函数有两条求值路径（单点的 `sampleValue` 和成批的 `sampleVolume`），
  浮点运算的顺序不同，最后一位会差，两条都移植了。
- 数据（`worldgen/noise`、`worldgen/density_function`、`worldgen/noise_settings` 和两个
  多噪声群系预设）由生成器从官方导出，存在 `mc-bamboo-data/src/generated/worldgen.json` 里。
- 用官方代码出题比对：几十万个点上每条噪声的两条路径、每条注册表密度函数和七套维度设置的
  噪声路由的两条路径、三个维度的群系，以及 67 个群系在几处坐标上的 `Biome.getTemperature`
  （含固定种子的三份噪声和 `frozen` 修饰），浮点按 `Float.floatToRawIntBits` 逐位相同。

已知限制：

- 移动校验比官方严，也有几处更松。没有鞘翅的 300 格上限、漂浮、激流、睡觉、骑乘、击退宽限、
  切维度、单人房主豁免，游戏规则 `player_movement_check` 也关不掉，速度上限始终是 100。
  不跑 `LivingEntity.travel`，所以预期位移只有跳跃和卡在方块里的减速。碰撞算方块和世界边界，
  边界按官方 `Entity.collectCollidersIgnoringWorldBorder` 的条件（离边界够近）才把碰撞形状加进来；
  玩家互相穿身，±3.0E7 的坐标夹取仍在（`Level.isInWorldBoundsHorizontal`，和世界边界是两件事）。
  没加载的区块仍用超平坦模板，官方那里没有碰撞。摔落距离只记录，不结算伤害，也不调用方块的 `fallOn`；
  世界边界的伤害也不结算，见下一条。
  蜘蛛网和细雪会在 tick 结束时套上卡住的速度并清掉摔落距离；压力板、绊线和甜浆果丛被踩到的效果没接。
  终末之诗、每 200 tick 补发脚下方块、载具移动都没做。
- 世界边界还有几处没接：Bamboo 没有玩家生命值和伤害系统，官方 `LivingEntity.baseTick` 里的
  边界伤害（`damageSources().outOfBorder()`）不结算，`damage amount` / `damage buffer` 只存起来、
  跟着 `initialize_border` 发给客户端；`max-world-size` 没读，绝对最大值固定 29999984。
  重生点和传送不按官方 `getWorldBorderAdjustedRespawnData`、`PortalForcer`、
  `LivingEntity.randomTeleport` 夹进边界（没有死亡重生、下界传送门和随机传送）；
  `canSpawnEntitiesInChunk`、刷怪和生物 AI 里的边界判断用不上（没有生物）。
  边界状态写进官方按维度存放的 `dimensions/minecraft/overworld/data/minecraft/world_border.dat`
  （`export_settings` / `restore`，`restore` 传存档里的游戏时间），启动时读回，编码和官方
  `WorldBorder.Settings.CODEC` 对拍。边界伤害的警告遮罩、`BorderStatus.getColor` 由客户端自己算。
- 世界生成只有数学，没有地形：区块生成的流程、线程和区块状态都没动，世界仍然是超平坦。
  含水层、表面规则、雕刻器、地物、结构、高度图和出生点查找都没做。
  按区块分格插值（官方 `interpolated` 的成批求值）要区块网格才有意义，只做了单个坐标上的结果。
- 红石的已知差异：发射器会发射不生成实体的弹射物，默认抛出和余料都生成真的物品实体。箭、雪球、蛋（含蓝蛋、棕蛋）、喷溅药水、滞留药水、经验瓶、烟花、火焰弹和风弹走默认抛出，官方那些是弹射物，出现事件 1002、1004、1018 或 1051 的整题不比较。船和矿车放得下时只扣物品、不生成实体，放不下时再套一层默认抛出。盔甲架和带 `entity_data` 的刷怪蛋留在槽里，当作生成失败，数量变少的整题不比较。TNT 物品在 `tnt_explodes` 为真时生成点燃的 TNT 并播放 `entity.tnt.primed`，数量减一；规则为假时物品留着、发 1001，不取朝向。打火石点着 TNT 方块时走 `TntBlock.prime` 再 `removeBlock` 再扣耐久。这些题比较方块、声音、容器和 `level.random`。点燃的 TNT 实体本身不进比较，方块 oracle 也不 tick 它。同一题里如果还有弹射物事件、盔甲架或刷怪蛋离开发射器，仍按那些原因整题排除。鱼桶只倒水，不生成鱼，环境音不比较。倒成功后仍取官方 `EntityType.create` 的一次 `nextFloat`，以及 `Mob.finalizeSpawn` 的 `triangle` 和一次 `nextFloat`。美西螈在 `BUCKET` 原因下 `finalizeSpawn` 直接返回，只取朝向那一次。热带鱼再按 `nextFloat() < 0.9` 取常见变种或随机花纹。硫磺立方桶不放方块，生成用的随机数同样照取。剪刀收满蜜的蜂巢按 `minecraft:harvest/beehive` 掉 3 个蜜脾，战利品表走自己的 `random_sequence`，不碰 `level.random`；`Block.popResource` 为这一组取 3 次位置偏移，生成物品实体。桶倒进可替换方块时，官方 `destroyBlock` 会先在方块还在时按 `getDrops` 一组一组 `popResource`，再换成流体方块；方块 oracle 把那些表换成空表，两边都不取数，也不生成物品。骨粉催树苗、杜鹃、蘑菇、菌、草方块、菌丝和要地物的方块时整题不比较。默认抛出和塞不回发射器的余料都生成物品实体。速度按官方 `spawnItem`：先取一个 `nextDouble` 算 `pow`，X、Z 的均值是朝向步长乘 `pow`，Y 是 0.2，再取三个 `triangle` 的六个 double；这套随机数只取一次。打火石和剪刀的耐久走通用 `hurtAndBreak`，耐久附魔用 `level.random`。玩家没有身体和鞍这两格。骷髅头、僵尸头、苦力怕头、龙首和猪灵头不放方块，只试着穿到前方的人身上。合成器按官方逻辑合成并吐出产物，朝空气吐出的东西按 `DefaultDispenseItemBehavior.spawnItem` 生成真的物品实体（精度 6），成就和地图的后处理也没做。方块 oracle 给合成器加了固定场景的题，合成成功的题逐项比较方块状态、容器内容、等级事件和世界随机源最后的种子；合成器到期时如果官方没有合成成功，只发失败事件 1050，这一声照常比较。漏斗把同一件物品搬进搬出不算合成。投掷器朝空气抛出的物品生成物品实体；压力板、探测铁轨、绊线和绊线钩、标靶、幽匿感测体、避雷针要实体、弹射物、振动或雷击才触发，地上的物品还没有接上这些触发，世界里没有生物和弹射物，下落方块和点燃的 TNT 也不会踩这些；阳光探测器不随时间变化，只在右键时按当前天空光算，世界时钟停在正午。唱片机放进唱片后立刻算在放（`ownSignal` 为 15），但不跑官方的歌曲计时：假世界也不登记唱片机的 ticker，所以不抽每 20 刻的 `nextInt(4)`，歌也不会放到结束。宝库只在官方结果里出现宝库方块实体的声音或状态事件 3015–3017 时整题不比较，没触发的宝库照常比较。
- 活塞不推实体，也不推动玩家。下落方块和点燃的 TNT 已经在世界里，但官方 `PistonMovingBlockEntity` 的
  `moveCollidedEntities`、`moveStuckEntities`、`fixEntityWithinPistonBase` 三段没实现：掉落物、下落方块
  和点燃的 TNT 不会被活塞推动，也不会被粘液块、蜂蜜块带着走。移动中的活塞按方块实体算出的形状挡住实体；
  伸出和收回的音效按官方 `PistonBaseBlock.triggerEvent` 发出。
  `moveBlocks` 里的破坏事件 2001 官方包在 `level.isClientSide()` 里，服务端不发，客户端处理方块事件时自己放。
  移动中的方块不发方块实体数据，客户端靠方块事件播放动画。
- 下落方块放不回去、掉出世界、或者下落超过 600 刻时，`dropItem` 且游戏规则 `entity_drops` 为真才按官方
  `spawnAtLocation(ServerLevel, ItemLike)` 生成方块对应的物品；规则关掉时方块直接消失，不放回也不掉落。
  落地失败的两条路径先 `discard` 再 `callOnBrokenAfterFall`，超时和掉出世界那一条不调用它。
  物品实体按自己的随机源取朝向和初速度，拾取延迟 10；`entity-oracle` 逐刻比较它的位置、速度、物品和随机数。
  `time > 100 && 出界 || time > 600` 是官方的超时条件，`/gamerule entity_drops false` 能挡住这三个分支。
  砸到实体的伤害没做（铁砧和滴水石的伤害标记照官方记着，只是不结算）。下落方块不走传送门，落地不恢复方块实体数据。
  铁砧落地发 1031，摔碎发 1029；滴水石摔碎发 1045，硫磺尖刺发 1052；刷子方块摔碎在碰撞盒中心发 2001。
  点燃的 TNT 播放 `entity.tnt.primed` 并生成实体。它不走传送门，也不被水推动。爆炸会打到地上的物品，
  但不发 `ClientboundExplodePacket`（官方的爆炸声和粒子在这个包里，不在 `playSound`）。
  TNT 爆炸在换成空气之前按 `getDrops` 收集掉落，整圈方块处理完再 `popResource`；`tntExplosionDropDecay` 默认是假，
  所以不传爆炸半径，`explosion_decay` 不取数。TNT 自己的 `dropFromExplosion` 是假，不掉 TNT 物品。
  容器被炸开时，`dropContents` 仍在这一圈里、在后面 TNT 的引信之前取 `level.random`。不调用 `spawnAfterBreak`：
  矿石和刷怪笼的经验随机数不取，虫蚀方块也不生成蠹虫。掉落物上的战利品组件按统一的
  `JSON → NBT → 组件补丁` 转换写回，解不开的那一项记一条警告后丢掉。方块掉落求值时把方块实体
  的 `collectComponents` 结果作为 `BLOCK_ENTITY` 参数传给 `copy_components`，所以装了东西、改过名的潜影盒
  （以及带图案的旗帜、带纹样的陶罐）炸开后掉出来的物品仍带着那些组件；创造模式挖非空潜影盒按官方
  `playerWillDestroy` 直接掉出带内容的盒子，空盒子不掉。
  游戏规则 `tnt_explodes` 为假时不点燃、不爆炸，也不取朝向和短引信的随机数。默认是真。非玩家实体的 ID 从 16777216 起，不和玩家共用计数器。
  未氧化的铜傀儡雕像被斧头点中时方块消失并播放 `entity.copper_golem.spawn`，不生成铜傀儡。
- 流体只有主世界的流动速度。源头转换读 `water_source_conversion`（默认真）和 `lava_source_conversion`（默认假）。岩浆会点燃旁边的方块，岩浆变高时官方随机地慢四倍，
  Bamboo 用同一个随机源。鱼桶倒出来的只有水，没有鱼；倒出声仍按这种桶自己的 `emptySound` 播放。
  比较时从官方声音列表里去掉这只桶生成的生物自己的声音：`MobBucketItem.spawn` 只调用一次
  `playAmbientSound`，经 `Entity.playSound` 发出，不跳过玩家，注册表名以 `minecraft:entity.<这种生物>.` 开头
  （生物是生成器导出的 `MobBucketItem.type`）。桶自己的那一声会跳过玩家，留下来照比。
  方块发出的 `entity.generic.extinguish_fire`、`entity.tnt.primed` 和 `entity.copper_golem.spawn` 路径对不上，也留下来。
  方块变化、计划刻和这些声音照常比较。
- 音效还缺实体、生物和环境这些。生物桶生成的生物那一声因此不模拟，只在比较时从官方列表滤掉，不把整题排除。
  紫水晶的共鸣要在投射物击中时触发，世界里没有投射物所以不响。漏斗官方就没有音效。
  音效包里没显式给出的种子，官方来自 `Level.soundSeedGenerator`（由世界种子初始化）。Bamboo 用单独的 `LevelRandom`，
  种子是启动时的纳秒数，和官方的 `threadSafeRandom` 不是同一个序列。这是有记录的随机差异，只影响客户端挑同一音效的哪个变体。
- 用桶和炼药锅互动时，创造模式按 `ItemUtils.createFilledResult` 补一个空桶或满桶（物品栏里还没有同样的那一个时），
  并播放对应的桶声音。水下用岩浆桶或细雪桶只消耗、不倒出，也不补桶。
  炼药锅其余要看物品组件的条目已经接上：没有自定义效果的水瓶给空锅或没满的水锅加一层水，玩家拿回玻璃瓶；
  玻璃瓶从水锅里舀走一层，拿到 `potion_contents` 是水的药水；`cauldron_can_remove_dye` 里带 `dyed_color`
  的物品整堆洗掉颜色；带 `banner_patterns` 的旗帜去掉最后一层，一摞只洗一个；染色潜影盒换成不染色的，
  其余组件按 `ItemStack.transmuteCopy` 留在新物品上。创造模式按 `ItemUtils.createFilledResult`：
  不消耗手里那一个，水瓶和玻璃瓶在物品栏里已经有同样的结果时不再加一份，旗帜和潜影盒总是再给一份。
  洗染料不走这条，创造和生存都是整堆去掉颜色。倒水和舀水会放 `bottle_empty`、`bottle_fill` 和桶的声音。
  没有统计（`use_cauldron`、`item_used`、`clean_armor`、`clean_banner`、`clean_shulker_box`、
  `fill_cauldron`），也没有 `gameEvent`（`fluid_place`、`fluid_pickup`、`block_change`）。
  生存模式放不下时按 `Player.drop` 把剩下的掉在地上。创造模式放不进去的那一份按官方清掉，不掉在地上。
  `PREDICTED` 和 `SERVER_ONLY` 这里都只在服务端生成一个实体，客户端预测的那一份对不上。
- 容器在方块之间存取。漏斗会吸上方和身体碰到的物品实体。投掷器朝空气生成物品实体，
  发射器按红石一节的范围发射，方块实体没有持久化，关服后内容清空。破坏容器时槽里的物品会按 `dropContents` 掉在地上。
  漏斗和投掷器认箱子、木桶、潜影盒、漏斗、发射器、投掷器、堆肥桶、雕纹书架、唱片机、饰纹陶罐、合成器、
  熔炉、高炉、烟熏炉、酿造台和架子（`shelf`）。
  玩家从雕纹书架取出的书按 `Inventory.add` 进背包并同步槽位，同时播放取出的声音；生存模式放不下的按 `Player.drop` 丢掉，
  创造模式放不下的按官方清掉。官方掉落用物品实体自己的随机源，不碰 `level.random`。从唱片机弹出和破坏时都会生成唱片实体。
  工作台、合成器、切石机、锻造台、熔炉、高炉、烟熏炉、酿造台、织布机、砂轮、
  附魔台、铁砧和制图台会打开菜单。合成器的结果格只显示产物，菜单 oracle 会随机出
  禁用位和配方材料，把九格、结果格和 10 个数据槽一起比过。营火右键能烧的食物会放上去，不打开菜单。
  箱子、陷阱箱、铜箱子、木桶、末影箱和潜影盒的开合音效按官方 `ContainerOpenersCounter` 发出：
  左半个箱子不响也不取随机数，右半个偏到两箱中间，木桶先响再改 `open`，潜影盒在方块事件之后、人数跨过 0 或 1 时才响。
- 物品组件接进玩法的地方是挖掘和放置：`tool` 组件决定挖掘速度和有没有掉落物；冒险模式靠主手的
  `can_break` 谓词决定能不能挖，靠 `can_place_on` 谓词决定点到的方块能不能放。谓词里的方块列表、
  方块标签和方块状态属性都按官方匹配，属性类型（整数、布尔、枚举）和枚举的声明顺序来自生成的数据。
  整数按 `Integer.parseInt` 解析，包括全角和阿拉伯-印度数字这些 `Character.digit` 认的十进制数字。
  带方块实体 NBT 的谓词读不出来，一律按不匹配算。
  书与笔保存和署名会解析并写回 `writable_book_content` 和 `written_book_content`。
  炼药锅读 `potion_contents`（没有自定义效果的水）、`dyed_color`（物品在 `cauldron_can_remove_dye` 里）
  和 `banner_patterns`（去掉最后一层）。染色潜影盒换成不染色的，补丁按 `transmuteCopy` 套到新物品上。
- 组件按官方原样发给所有人（`set_equipment`、容器槽位、区块包、`block_entity_data`、中键选取）。
  `lock`、`container_loot`、`map_decorations`、`debug_stick_state`、`recipes` 和文本组件都先按
  官方 `Codec` 解码再按它写回去：解不开的创造模式槽位断开连接，收下的重新编码和官方写出的字节一致。
  没有权限系统，只有管理员才能写入自定义数据的方块实体（命令方块、讲台、告示牌、悬挂告示牌、刷怪笼、试炼刷怪笼）
  按非管理员处理，物品上的 `block_entity_data` 不写进去。锁的谓词里，附魔、药水、容器、烟花、书页、
  属性修饰、纹饰、唱片和村民种类这些还没实现，匹配时算不通过，所以用它们当锁的容器打不开。
  NBT 谓词的嵌套深度上限是 32，比官方的 512 严。`custom_data` 和 `intangible_projectile` 的官方 `Codec`
  接受任何复合标签，和我们的编解码一致。
- 只含瞬态组件（`creative_slot_lock`、`map_post_processing`、`additional_trade_cost`）的物品堆，
  官方 `ItemStack.CODEC` 会写出空的 `components`，再解一次瞬态就没了、这个字段也会被省掉，
  官方自己也往返不了。算哈希时瞬态组件没有存档 `Codec`，`HashedStack` 对不上，多发一次槽位同步。
  存档编解码不做 DataFixer 的版本升级，读到别的 `DataVersion` 直接失败，也不写 JSON 形式。
  SNBT 能解析和打印，`/tellraw`、`/title` 的文本组件和 `/particle` 的选项已经用上了它。
- 物品上的 `block_entity_data` 按 `BlockItem.updateCustomBlockEntityTag` 写进方块实体，但只保留我们认识的字段：
  唱片机的 `RecordItem`、自定义名和锁。不认识的键丢掉。命令方块、讲台、告示牌、悬挂告示牌、刷怪笼和试炼刷怪笼
  按非管理员跳过。容器、雕纹书架和饰纹陶罐的物品格跟官方一样被 `container` 组件覆盖（没有这个组件就是空的）。
  创造模式 Ctrl+中键把这些数据还给物品。唱片机不写 `ticks_since_song_started`。营火上的食物不在这份数据里。
  放下的方块实体只留官方 `applyImplicitComponents` 读的那几项，物品上多出来的组件
  （比如旗帜上的 `lore`）不进方块实体，中键也还不回来。
- 容器的锁按 `LockCode.canUnlock` 拦人：旁观者直接开，其他人要主手物品通过 `ItemPredicate`。
  打不开时发 `container.isLocked` 的覆盖层消息，没有开锁失败的音效包（官方这段也不发声音包）。
  双箱两边都要能开，提示用先有自定义名的那一侧，否则是 `container.chestDouble`。
  潜影盒先看头顶空间，被挡住时不提示上锁。末影箱没有这把锁。
  谓词支持物品集合、数量区间、`components` 精确匹配，以及 `damage`、`custom_data` 和「组件在不在」。
  其余谓词类型算不通过。
- 饰纹陶罐的 `getUpdateTag` 在没有战利品表时把那一格写成 `item`（物品堆的存档 NBT）。纹样照旧。
  放好之后漏斗和玩家右键读写的是同一格，比较器按这一格算。
- 方块实体的 `getUpdateTag` 只写我们认识的字段：旗帜那张 `components` 表永远是空的，
  NBT 的键序也和官方的 `CompoundTag`（哈希表）不同，客户端照样解得开。
- 冒险模式 `can_break` 谓词里带方块实体 NBT 的那一类读不出来，一律按不匹配算。
- 附魔效果里只有 `item_damage` 会改工具损耗。官方数据包里带这个效果的只有耐久（`unbreaking`）。
  别的附魔效果、属性数值和食物药水的使用效果还没有。
- 方块实体的内容有告示牌的文字、容器的物品、熔炉和高炉烟熏炉的燃料与进度、酿造台的药水与燃料、营火上的食物、
  从物品带来的那几项组件和潜影盒的盖子动画，
  活塞的移动方块实体只有动画进度。蜂箱里的蜜蜂和铜傀儡雕像的自定义名不在这份数据里，
  中键带不回来；蜂蜜等级和雕像姿势写在方块状态上，会带上。
  告示牌的文字只有纯文本，没有样式和点击事件（`§` 格式码和官方一样会被去掉）。编辑走离线模式：
  没有聊天过滤，也没有给管理员随时编辑的开关（`allow_op_features`）。
- 工具右键会扣耐久：斧子去皮、刮铜、去蜡，锄头耕地，锹压土径和熄灭营火，打火石点火，剪刀剪藤、刻南瓜、收蜂巢，
  火焰弹点 TNT 会消耗一个（创造模式数量改回去）。损坏时按槽位发实体事件 47 或 48，并同步那一格。
  这些操作里官方有的方块音效已经接上，`BlockTransformer` 的粒子也在。锄头翻生根泥土会按面掉一根垂根，剪刀刻南瓜掉 4 个种子，剪刀剪满蜜的蜂巢掉 3 个蜜脾。
  玻璃瓶对着满蜜的蜂巢即使创造模式也消耗一个，手里空了就换成蜜瓶，否则放进背包，放不下就按 `Player.drop` 丢掉；不放蜜蜂、不检查营火的烟，也不记统计。
  要生成实体、改世界生成或者看物品组件的物品还不能用：
  骨粉不能让树苗长成树、不能催大型蘑菇和草方块上的植物，刷怪蛋、矿车、盔甲架、拴绳放不出来，水瓶不能把泥土变成泥巴，
  刷子只吃掉点击。世界生成决定的方块（紫颂花生长）和要生成实体（海龟蛋、嗅探兽蛋、青蛙卵）的随机刻也不做。
- 生存模式的挖掘有进度、裂纹和工具耐久。硬度不是 0 时按 `tool` 的 `damage_per_block` 扣，剪刀不看硬度、剪火不扣。
  创造模式不扣。冒险模式挖得动的时候照样扣，旁观模式挖不动。耐久用完物品减一，并发实体事件。
  生存和冒险模式挖掉方块后按战利品表生成物品实体，创造和旁观不掉。不给经验，也不调用 `spawnAfterBreak`，
  所以矿石和刷怪笼该从 `level.random` 抽的经验数没有抽。不扣饥饿值、不记统计，急迫、挖掘疲劳、精准采集、时运这些附魔和状态效果都没有：
  工具只按物品 ID 交给战利品表；方块实体按官方 `BlockEntity.collectComponents` 传给 `BLOCK_ENTITY` 参数，
  `copy_components` 的 `block_entity` 来源把容器内容、自定义名、锁、战利品表、旗帜图案和陶罐纹样带回掉落物。
  掉落上由战利品函数写出的组件
  经统一的 `JSON → NBT → 组件补丁` 转换写进物品堆，解不开的单项丢掉并记警告；`/loot` 和开箱填充也用这一份转换。
  盔甲受伤、打实体、弓、钓竿、鞘翅和盾牌也不扣耐久。实体死亡仍不调用战利品表，开箱已经接上（见下一段）。
  矿石、幽匿这类方块破坏时官方 `spawnAfterBreak` 还会按 `xpRange` 生成经验球并取一份随机数，
  这里没有经验球，也不取这份随机数（`Level::spawn_after_break_random` 只留了钩子）。
  冒险模式按官方 `Player.blockActionRestricted` 判定：主手的 `can_break` 能匹配这一格才挖得动；放置按官方
  `ItemStack.useOn` 的检查，`can_place_on` 能匹配点到的方块才放得下。旁观模式挖不动。传送门没有别的维度，
  折跃门也不存出口，所以旁观者右键只会看到 `spectator.cannot_teleport`，不会真的传送。
  工作台、合成器、切石机、锻造台、熔炉、高炉、烟熏炉、酿造台、织布机、砂轮、
  附魔台、铁砧和制图台旁观者也能打开，有书的讲台也能打开，没有书的讲台和架子放过。
  旁观者的菜单按钮全部忽略，包括织布机选图案和附魔台选选项。旁观者不计入箱子的开合人数。探针里管理员用 `/gamemode` 把 `Spectator` 切成旁观模式，再让它右键工作台打开菜单。
  没有 `container.spectatorCantOpen`。
  矿石点亮、龙蛋传送和音符盒发声这些方块 `attack` 已经按官方逻辑执行。音符盒的声音在方块事件里算好后发成音效包，
  音符粒子由方块事件交给客户端。头颅的自定义音效按 `createVariableRangeEvent` 内联发送。
- 命令方块、结构方块、拼图方块和测试方块只有 `canUseGameMasterBlocks`（创造模式的瞬间破坏，并且权限等级至少 2）才能放。命令方块能打开编辑界面并保存命令、模式和条件，不执行，重启后记录消失。调试棒同样要这个权限：左键切换属性，右键改值。权限来自 `ops.json`，`/op` 和 `/deop` 能在运行中改名单。
- 书与笔可以写字和署名，成书写回物品栏，并经当前打开的菜单同步。右键成书会发 `open_book`。
  没有聊天过滤，成书不带 `filtered` 文本。选择器、计分板和 NBT 解析不了，页面留原文，仍然标成 `resolved`。
  页面解析后 JSON 超过 32767 时官方会解析失败、不广播格子；这里一律当成解析成功并广播。
  不记物品使用统计。放书会放 `item.book.put`，没有 `gameEvent`。物品栏放不下的书按 `Player.drop` 丢出，拆掉讲台时按朝向生成物品实体
  （官方用物品实体自己的随机源，不碰 `level.random`）。创造模式物品栏满了时取回的书按官方清掉，不掉在地上。
  活塞推动不带走台上的书。讲台菜单不能用光标把书从槽里拿出来，
  取书只走按钮。旁观者可以打开讲台，按钮全部忽略。冒险模式能翻页，不能取书。
  没有权限系统，所以放置讲台时不读物品里的 `block_entity_data`。
- 存档已经接进区块的加载和卸载，但只做 Anvil、只支持 26.3 新布局，且有一批内容不存，
  见上面「存档已知限制」和 `docs/m3-storage.md`。方块实体内容、计划刻、红石记忆、机器状态、
  方块实体带来的物品组件、命令方块、讲台上的书和三种实体的官方 NBT 都能整块组装并与官方
  对拍，也会随改过的区块一起写进世界目录。
- 命令已经接上：`/gamemode`、`/gamerule`、`/tp`（`/teleport`）、`/time`、`/give`、`/clear`、`/setblock`、`/fill`、`/say`、`/msg`（`/tell`、`/w`）、`/me`、`/list`、`/kick`、`/help`、`/scoreboard`、`/team`、`/bossbar`、`/tellraw`、`/title`、`/playsound`、`/particle`（这七个要等级 2）；世界设置类还接上了 `/difficulty`、`/defaultgamemode`、`/seed`、`/setworldspawn`、`/spawnpoint`、`/clone`、`/stopsound`、`/random`、`/enchant`、`/loot`、`/weather`；服务器管理类还接上了 `/op`、`/deop`、`/ban`、`/ban-ip`、`/banlist`、`/pardon`、`/pardon-ip`、`/whitelist`、`/stop`、`/setidletimeout`、`/transfer`、`/version`（权限见下一段）。
  语法、权限和提示按官方 Brigadier。启动时从工作目录读 `ops.json`，名单里的人用上面的等级，其他人是 0；等级 0 但在名单里仍算管理员（免刷屏、能看到管理员广播）。`/op`、`/deop` 可以在运行中改这份名单。
  26.3 的游戏规则由生成器导出，权威副本在世界线程的全局状态里，全世界共用。`/gamerule` 要等级 2。改完立刻给方块逻辑用的那份快照，并按官方 `GameRuleMap.CODEC` 写进 `data/minecraft/game_rules.dat`，启动时读回。默认特性集下 `max_minecart_speed` 不启用，命令和客户端界面里都没有它。`send_command_feedback` 和 `log_admin_commands` 管命令成功消息和管理员日志。`/fill` 的上限读 `max_block_modifications`（默认 32768）。`block_drops` 会挡住 `Block.popResource` / `popResourceFromFace` 生成物品实体（位置和速度的随机数照取），`entity_drops` 会挡住下落方块 `spawnAtLocation`，`tnt_explodes` 会挡住 TNT 点燃，掉落按战利品表生成物品实体。`respawn_radius` 存着，死亡重生还没做。世界出生点由 `/setworldspawn` 改，新玩家直接落在它的水平中心（不做 `respawn_radius` 随机找落脚点），`/spawnpoint` 只把玩家自己的重生点存起来，不影响进服位置。`/weather` 只写天气状态和回成功提示。世界每 tick 按官方 `ServerLevel.advanceWeatherCycle` 倒计时、自然开始/结束下雨和打雷（`advance_weather` 规则为假时冻结）、每刻把雨和雷暴强度渐变 0.01，并在强度变化、`isRaining()` 翻转时广播 `RAIN_LEVEL_CHANGE` / `THUNDER_LEVEL_CHANGE` / `START_RAINING` / `STOP_RAINING`；进服时按 `PlayerList.sendLevelInfo` 补发当前的渐变强度。战利品表的 `weather_check` 条件也读这份天气。还没接的差异：天气变暗的天空光（下雨/打雷时官方会改 `getSkyDarken`，进而影响阳光探测器、雪和霜冰的亮度判定）、雷击生成的实体、闪电对避雷针/实体/方块的作用和苦力怕充能，以及天空、雾和云的颜色变化。刷怪、伤害、睡眠、合成限制这些规则还没接到对应系统。
  生存和冒险会改模式和能力。挖掘和工具右键会扣耐久；盔甲、武器、弓、钓竿、鞘翅和盾牌不扣。没有饥饿。`/give` 多出来的物品会消失（没有捡起音效）。`/clear` 不清创造模式光标。`/time` 能改主世界和末地的钟，登录包仍只带主世界。
  按 Q、在菜单里丢出、以及背包放不下时，会生成物品实体。创造模式把物品拖出界面有官方的丢弃限流。旁观模式不丢也不捡。玩家没有生命值，官方血量不大于 0 时不捡，这里只要不是旁观就捡。捡起会发 `take_item_entity`。鞘翅不能用，攻击其他玩家没有效果。物品在地上 5 分钟（6000 刻）后消失，能合并、能被火、岩浆、仙人掌和爆炸毁掉，能被漏斗吸走。玩家不在实体名单里，捡起发生在这一拍所有非玩家实体 tick 之前，看到的是物品移动前的位置。
  `/tellraw` 和 `/title` 的文本组件按官方 26.3 的 SNBT 语法读（`{text:"hi",color:"red"}`、`["a","b"]`），
  不是 JSON 文本组件。组件过一遍官方 `Codec`；执行时按 `ComponentUtils.resolve` 展开：
  `selector` 变成玩家名列表，`score` 查 `/scoreboard` 的分数并套用数字格式（`*` 指接收者，名字当选择器编译），
  `keybind` 和合法的 `object`（图集精灵、玩家头像）原样保留并解析 `fallback`。
  `nbt` 组件和 `block` 数据源还没做：Bamboo 的方块实体是类型化结构，没有统一的 NBT 树和 NbtPath 求值器，
  所以按「没有数据源」返回空文本。`ComponentArgument` 校验失败时的详细 DFU 文案固定写成 `invalid component`。
  `/title` 六种动作都有，成功消息按目标数用 `.single` / `.multiple`；`times` 的 `t`/`s`/`d` 照官方换算成刻。
  `/playsound` 的 `sound` 带 `minecraft:available_sounds` 补全（官方命令树的快照里没有写 suggestion_id，
  这里让客户端来问服务端），11 种来源、位置、音量、音高和 `minVolume` 都在；
  声音按官方走内联定义（网络 ID 0 + 资源名），种子取一次 `Level.random`，距离超过 16 或 16×音量时，
  `minVolume` > 0 会把音源挪到玩家附近两格。
  `/particle` 能解析注册表里全部 128 种粒子名；带选项的 19 种按官方 `ParticleTypes.STREAM_CODEC` 编码
  （`dust` 的颜色和缩放、`block` 的方块状态、`item` 的物品堆、颜色类、geyser、trail、vibration 等）。
  `item` 选项只支持 `id` 和 `count`，不支持数据组件；`vibration` 只支持 `block` 位置来源（官方也拒绝 entity 来源）。
  两个命令的世界坐标都取执行者脚底，没有命令方块或 `/execute` 的来源偏移。
  难度默认 `easy`（和官方专用服务器一致；`difficulty` 已能在 `server.properties` 里读到，但 Bamboo 还没实现这个键），`/difficulty` 改完广播给所有人，进服时同步。默认游戏模式是创造模式（官方专用服务器默认是生存；`gamemode` 已能在 `server.properties` 里读到，但 Bamboo 还没实现这个键）；`force-gamemode` 按官方默认关闭，`/defaultgamemode` 不改在线玩家的模式。`level-seed` 还没有实现，世界种子固定为 0，`/seed` 报的就是它，`login` 里的哈希种子按官方 `BiomeManager.obfuscateSeed` 算。
  `/clone` 复制方块和方块实体数据，上限读 `max_block_modifications`；`/random` 按官方的种子规则共用 `mc-bamboo-loot` 的随机序列；`/enchant` 给主手物品写附魔组件；`/stopsound` 只给选中的玩家发包。`/loot` 的 `give`、`insert`、`replace`、`spawn` 和来源 `loot`、`mine` 可用；`spawn` 按官方 `dropInWorld` 生成带默认拾取延迟的物品实体，`kill`、`fish` 已在命令树里注册但执行时返回失败（还没有实体死亡和钓鱼系统）。
- 战利品表（`mc-bamboo-loot`）按官方 `LootTable` 算出方块掉落、实体死亡和箱子填充，随机源的取数次数和顺序跟官方对齐。生存和冒险挖方块、TNT 爆炸和 `/loot` 已经调用；实体死亡还没接。容器上的 `LootTable` / `LootTableSeed`（或物品上的 `container_loot` 组件放下后的方块实体）在官方 `unpackLootTable` 的调用点解包：玩家打开菜单、漏斗和投掷器读写、发射器取槽、饰纹陶罐读写、比较器读信号、破坏掉内容之前。填完就清表，旁观者打开不解包。随机源按 `LootContext.Builder.create` 选：种子非 0 用 `LegacyRandomSource(seed)`，种子为 0 且表有 `random_sequence` 用世界共用的序列缓存，都没有才用 `level.random`；结构生成时 `setBlockEntityLootTable` 的 `nextLong` 还没接。未解包的表会原样写回中键物品的 `container_loot`。`/setblock` 的方块 NBT 后缀和 `/give` 的物品组件后缀目前会明确报不支持，所以暂时不能用命令造出带战利品表的箱子；方块实体数据里的 `LootTable` / `LootTableSeed` 和 `container_loot` 组件能直接由物品或 `/clone` 带进游戏。解包不走 `THIS_ENTITY` 参数，玩家幸运值按默认的 0；依赖 `this_entity` 的自定义表会报不支持并清表。`loot-oracle` 的箱子题逐项比较 `fill`，另加了一道带 `BLOCK_ENTITY` 的潜影盒掉落题逐项比组件；`menu` oracle 加了一道「打开带战利品表的箱子」。序列缓存在世界里，种子是平坦世界的 0，不存档。
- 计分板、队伍和 Boss 栏按官方 26.3 的命令树和权限等级 2 实现，全服一份，放在世界线程的全局状态里，**不写存档**：重启后目标、分数、队伍和 Boss 栏全部回到空。
  `/scoreboard objectives` 支持 `list|add|remove|setdisplay|modify`（`displayname`、`rendertype`、`displayautoupdate`、`numberformat` 的 `blank`/`styled`/`fixed`）；
  `players` 支持 `list|get|set|add|remove|reset|enable|operation|display`（`name` 和 `numberformat`）。
  准则只实现 `dummy`、`trigger` 以及只读准则的只读语义；`health`、`food`、`air`、`armor`、`xp`、`level` 因为 Bamboo 没有对应的玩家状态，进服后第一刻各按默认值（20、20、300、0、0、0）写一次，之后不再更新，统计类准则能解析和创建但不会自动加分（没有统计系统）。玩家离线时分数保留，别人侧边栏上仍能看到。
  目标只有被 `setdisplay` 显示出来时才给客户端发 `set_objective` / `set_display_objective` / `set_score`，进服时会补发正在显示的目标、全部分数和全部队伍。
  `/team` 支持 `list|add|remove|empty|join|leave|modify`（`displayName`、`color`、`friendlyFire`、`seeFriendlyInvisibles`、`nametagVisibility`、`deathMessageVisibility`、`collisionRule`、`prefix`、`suffix`），
  队伍颜色、前后缀会用在聊天显示名和命令反馈里；Tab 列表的 `display_name` 仍是空的，客户端按队伍自己画名字。
  `/bossbar` 支持 `add|remove|list|set (name|color|style|value|max|visible|players)|get (value|max|visible|players)`，
  玩家加入、离开和重连按官方 `ServerBossEvent` 发 add/remove 包。命令反馈用官方翻译键，`send_command_feedback` 照旧管自己的系统消息。
  选择器的 `scores=` 和 `team=` 现在能查计分板；`trigger` 命令、统计加分、死亡消息可见性、友军伤害和碰撞规则还没接到对应的游戏逻辑。
- 服务器管理命令：`/op`、`/deop`、`/ban`、`/ban-ip`、`/banlist`、`/pardon`、`/pardon-ip`、`/whitelist`、`/stop`、`/setidletimeout`、`/transfer`、`/version`。
  命令树（节点名、参数解析器和属性、`executable`、权限）和官方数据生成器的 `commands.json` 逐节点一致，`/version` 要等级 2、`/stop` 要等级 4，其余要等级 3。
  四份名单文件在服务器目录（命令行版的工作目录或 `--server-dir` 指定的目录，桌面版是 APP 数据目录的 `server/`）：`ops.json`、`banned-players.json`、`banned-ips.json`、`whitelist.json`。启动时读一次，命令改完按官方 `StoredUserList.save` 的字段顺序写回（pretty-printed JSON 数组）；Bamboo 先写临时文件再改名，比官方直接覆盖安全。世界线程只持有内存里的权威副本，写盘和 `/whitelist reload` 的读盘在连接层的专门线程上，世界线程不做 IO。
  `/op`、`/deop` 改完立刻给在线的当事人重发权限等级（`entity_event` 24–28）和他用得了的整棵命令树；`/op` 新加的等级按官方 `op-permission-level` 默认 4。`/ban` 和 `/ban-ip` 当场踢人，登录时按官方 `PlayerList.canPlayerLogin` 的顺序（玩家封禁 → 白名单 → IP 封禁）拒绝并给出带原因和到期时间的翻译文本；连接层检查完、世界线程处理 `Join` 时再按权威副本复查一次。封禁项里的 `created` 按官方 `yyyy-MM-dd HH:mm:ss Z` 写入，Bamboo 用 UTC；过期时间只按读入时解析出的毫秒值判断。
  `/whitelist` 有 `on`、`off`、`add`、`remove`、`list`、`reload`。`reload` 的读盘在连接层线程上，结果下一 tick 才被世界线程采用（官方同步读盘后立刻生效，这是世界线程不做 IO 的代价）；成功消息在发出读盘请求时立刻回给命令来源。两个开关都按官方默认值：`white-list` 运行时由 `on|off` 改、重启恢复 false（官方会写回 `server.properties`；Bamboo 会原样保留这个文件里的 `white-list`，但目前不读它也不存档）；`enforce-whitelist` 官方默认 false，Bamboo 没有这个配置项，所以 `on` / `remove` / `reload` 默认不会踢在线玩家，踢人路径只在单元测试里注入开关覆盖。OP 不受白名单限制。
  `/stop` 发 `commands.stop.stopping` 之后走现有关服流程，世界线程给所有玩家发 `multiplayer.disconnect.server_shutdown` 再退出，服务端随世界线程结束。`/setidletimeout` 的单位是分钟，0 关闭；空闲判断照官方 `ServerGamePacketListenerImpl.tick`，Bamboo 已解码且能对应的动作（聊天、命令、按键输入、玩家指令、挥手、换快捷栏、破坏、右键、容器点击和按钮、告示牌、客户端指令、位移）会重置 `lastActionTime`，官方另外那些 handler（攻击实体、交互、旁观者动作、放配方、书与笔等）还没接。
  `/transfer` 发 `ClientboundTransferPacket`（主机名、端口，默认 25565），`/version` 按官方 `VersionCommand.dumpVersion` 的行序打印；版本信息用 `mc-bamboo-data` 的常量，`series`、`build_time`、两个 `pack_version` 按 26.3 官方 `version.json` 固定，测试会读官方 jar 逐个对照。
  名字参数用官方 `GameProfileArgument`：`@` 开头按玩家选择器解析，在线玩家按名字解析；离线模式不在线的名字用官方 `UUIDUtil.createOfflinePlayerUUID`（`OfflinePlayer:<name>` 的 MD5）。正版模式按官方 `CachedUserNameToIdResolver` 维护一份「本次启动以来进过服的玩家 名字→UUID」缓存（`Join` 时记下，不写盘；官方会持久化到 `usercache.json`），在线玩家查不到时先查这份缓存，再查不到就按官方「查不到」报 `argument.player.unknown`（Bamboo 没有 Mojang 档案查询，也不从白名单猜 UUID）。`ban-ip` / `pardon-ip` 的 target 是 IP 或在线玩家名（用连接层报来的对端 IP）。
  `/save-all [flush]`、`/save-on`、`/save-off` 按官方消息和权限等级 4 实现，见「存档」一节
  （成功消息在快照交给 IO 线程后、或 `flush` 的 IO 栅栏回执后才发，差异记在已知限制里）。`/ban-ip` 的 `commands.banip.info` 用逗号连接玩家名，和官方 `EntitySelector.joinNames` 的格式不同；`/version` 的 `build_time` 是纯文本而不是官方的 `Date` 翻译参数。
  会改组件的函数只记下「要设置什么」的 JSON，不写真正的组件字节。
  做完的函数：`set_count`、`set_item`、`set_components`、`apply_bonus`（三种公式）、`copy_state`、`discard`、`explosion_decay`、`filtered`、`limit_count`、`set_damage`、`set_enchantments`、`set_instrument`、`set_ominous_bottle_amplifier`、`set_potion`（只写药水 id）、`set_stew_effect`、`enchant_randomly`、`enchant_with_levels`、`enchanted_count_increase`、`furnace_smelt`、`copy_components`（只做 `block_entity` 来源）、不带实体的 `set_name`。
  没做、求值时直接报不支持的：`exploration_map`、`set_custom_data`、`set_attributes`、`copy_name`、`set_contents`、`modify_contents`、`set_loot_table`、`set_lore`、`fill_player_head`、`copy_custom_data`、`set_banner_pattern`、`set_random_dyes`、`set_random_potion`、`set_fireworks`、`set_firework_explosion`、`set_book_cover`、`set_written_book_pages`、`set_writable_book_pages`、`toggle_tooltips`、`set_custom_model_data`，以及 `copy_components` 的实体或物品来源、带实体的 `set_name`、按名字引用的物品修饰器。
  数值里的 `score`、`storage`、`environment_attribute`，条件里的 `entity_scores` 和 `environment_attribute_check`，条目里的 `slots` 和战利品表标签（`#`）同样报不支持。
  位置谓词不查流体和结构。实体谓词里还没做的子谓词（效果、NBT、玩家、装备槽、闪电等）会让整次求值失败，而不是当成不匹配。
  随机序列不存档，也没有 `salt`、`includeWorldSeed`、`includeSequenceId` 开关（默认 salt 0，后两项都开）。掉落不查物品的功能开关（`isItemEnabled`）。乘客谓词只看第一个乘客。
- 工作台（含玩家物品栏里的 2×2）、合成器、切石机、锻造台、熔炉、高炉、烟熏炉、酿造台、织布机、
  砂轮、附魔台、铁砧和制图台已经接上菜单。
  合成、烧炼、切石、锻造、酿造、织布和砂轮会查配方或组件。纹饰、染色、药水、烟花、书复制、修复、盾牌装饰、
  旗帜复制、地图放大和装饰罐会写上组件。没有配方书，也没有 `place_recipe`。
  铁砧按 `AnvilMenu.createResult` 算修复、合并附魔、改名和 `cost`，按 `rename_item` 改名字（过滤非法字符、
  最多 50 个 UTF-16 码元、空名字去掉自定义名）；取出结果会按 12% 概率损坏铁砧（anvil → chipped → damaged → 消失）、
  发 1029 或 1030 事件，并保留朝向。没有经验等级：生存模式固定按 0 级处理，只能取 `cost` 为 0 的结果；
  创造模式不扣等级也不受 40 级上限限制。没有聊天过滤器，改名不走文字过滤；没有统计和进度。
  制图台按 `CartographyTableMenu` 收带 `map_id` 的地图和纸 / 空地图 / 玻璃板。Bamboo 没有 `MapItemSavedData`，
  真实世界里按官方「找不到地图数据」的分支处理：`setupResultSlot` 直接返回，不预览结果、不放大、不锁定；
  菜单 oracle 的假世界补上比例和锁定状态，复制（`map` → 两份）、放大（`paper` → `MAP_POST_PROCESSING=SCALE`）
  和锁定（`glass_pane` → `LOCK`）三支都和官方逐槽比过。取结果发 `ui.cartography_table.take_result`。
  附魔台按 `EnchantingTableBlock.BOOKSHELF_OFFSETS` 数书架，用 `enchantmentSeed` 和生成的附魔表算三个
  `costs`、`enchantClue`、`levelClue`（共 10 个数据槽）。每名玩家进服时有一个附魔种子，附魔成功后按
  `onEnchantmentPerformed` 换新种子；选第 0/1/2 项会扣青金石、扣等级、写附魔、播
  `block.enchantment_table.use`（音调从 `level.random` 取一次）。生存模式同样按 0 级处理，
  创造模式不扣等级；没有经验球、统计和进度。
  织布机按 `LoomMenu` 列图案：空的图案槽用 `minecraft:no_item_required`，图案物品用 `provides_banner_patterns`。
  结果在 `banner_patterns` 上加一层。`slotsChanged` 把层数卡在 6；按钮包本身不检查这个上限，
  已经有 6 层时再发一次按钮可以写出第 7 层，下一回格子变化才会把结果清掉。取出结果只消耗旗帜和染料，不消耗图案物品。
  没有织布音效。
  砂轮去掉非诅咒附魔，两件同种物品按官方公式合并耐久，并写上 `repair_cost`。
  附魔组件按网络和输入顺序保存，不是官方 `Object2IntOpenHashMap` 的哈希顺序。
  在世界上取出结果时，经验先从 `level.random` 取 `nextInt`，再按 `ExperienceOrb.award` 每次拆分取一次 `nextInt(40)`，
  然后发 `levelEvent(1042)`。这一下不取随机数，也不生成经验球。经验球自己的随机源不碰 `level.random`，所以没取。
  菜单重放用的是 `ContainerLevelAccess.NULL`，不取这些数，也不发这个事件。
  熔炉和酿造台自己 tick：燃料、进度、`lit` 或 `has_bottle_*`、比较器输出都在。
  tick 期间方块实体不在表里。比较器要读熔炉或酿造台时先排进队列，实体放回去之后按请求顺序再读，所以熔炉点着的那一刻比较器读到的是点燃之后的物品。
  熔炉 tick 里的 `setChanged` 在消耗燃料、烧完一格、`lit` 变化时发出；酿造台在补燃料、每个酿造刻、开始酿造时发出。
  熔炉格子的 `setItem` 只有原料格换成另一种物品才通知比较器，`removeItem` 拿走东西才通知，漏斗把吸不走的燃料或产物放回去不再次通知。
  方块 oracle 不跑熔炉和酿造台的 ticker，和官方假世界一样。
  燃料时间来自物品的 `cooking_fuel` 组件（26.3 没有 `FuelValues` 这张表）。
  取出烧炼产物时按官方 `createExperience` 从世界随机源取数，但不生成经验球，溢出的物品也不掉到地上。
  菜单把格子写回熔炉或酿造台之后通知一次比较器（官方是每个格子的 `setChanged`，这里合并到写完）。
  架子的玩家交换把物品写进三格，但不改玩家物品栏：手上的物品被复制进架子，换下来的物品不回到背包。
  通红石时快捷栏只建模第 0 格，其余当成空的。没有架子的 `gameEvent`、`sendBlockUpdated` 和音效。
  营火按 `campfire_cooking` 烧四格食物。点着时进度每刻加一，到时间按 `Containers.dropItemStack` 的取数顺序生成物品实体。熄灭时进度每刻减 2，不低于 0。被水或能扑灭营火的物品熄灭时食物留在格子里，26.3 的 `douse` 不往外掉。
  营火没有比较器输出；格子里有东西的烧炼刻仍会 `setChanged`，通知旁边的比较器。
  发给客户端的 `getUpdateTag` 只有 `Items`，食物靠它画在营火上。进度不在这个标签里，也不存档。
  同一刻烧完多格时只发一次最终的标签，官方是每格一包。不查 `isItemEnabled`。
  物品上的 `container` 组件不会在放下时灌进营火。漏斗不能存取营火。
  铲子扑灭先发 `levelEvent(1009)`，再灭掉，再按 `hurtAndBreak(1)` 扣耐久（创造模式不取耐久附魔的随机数）。
  被水浇灭时先放 `entity.generic.extinguish_fire`。没有粒子、统计和 `gameEvent`。客户端的 `particleTick` 不在服务端跑。
  不记统计、不发配方音效、不触发进度。有限合成（`limited_crafting`）没做，默认和官方一样是关的。
- 正版验证的限制：`online_mode` 默认关闭，命令行版用的是默认配置，桌面端的设置页里可以打开。
  和代理转发互斥：打开转发时正版验证会被关掉，因为代理已经替服务端验过了。
  正版验证打开且公钥取到了之后，聊天按上面一节签名。取钥失败会按 authlib 的间隔在后台重试，钥匙到达后安全聊天立刻生效。
  RSA 解密用的 `rsa` crate 有 RUSTSEC-2023-0071（Marvin 计时侧信道），上游还没有修好的版本，
  Bamboo 用 `decrypt_blinded` 做盲化缓解，详见 `crates/mc-bamboo-auth/src/keypair.rs` 的注释。
- 系统消息的可见性按官方做，但官方 `ServerPlayer.sendSystemMessage` 还用
  `PacketSendListener.exceptionallySend` 在发包失败时补一条 `multiplayer.message_not_delivered`；
  Bamboo 的链路写失败时直接关连接，没有这个回调，所以不发这条补发消息。

## 项目结构

| 路径 | 职责 |
|---|---|
| `crates/mc-bamboo` | 命令行入口，可执行文件 `bamboo` |
| `crates/mc-bamboo-server` | 网络层：监听、连接状态机、读写队列、登录准入 |
| `crates/mc-bamboo-game` | 世界线程：tick、玩家、区块跟踪、实体同步、方块交互、方块实体的物品数据、聊天，不依赖 tokio |
| `crates/mc-bamboo-protocol` | 网络协议：VarInt、帧、数据包、物品数据组件，只处理字节 |
| `crates/mc-bamboo-world` | 游戏世界：区块、调色板、光照、超平坦，不知道网络 |
| `crates/mc-bamboo-worldgen` | 世界生成的数学：随机源、噪声、密度函数、气候、群系源和官方群系温度，纯函数；天气的群系温度已接进游戏，生成地形还没接 |
| `crates/mc-bamboo-blocks` | 方块行为：放置、形状更新、破坏、右键（含工具对方块的右键）、计划刻、方块事件和会 tick 的方块实体，纯逻辑，不知道网络和存储 |
| `crates/mc-bamboo-command` | 命令分发和参数解析：Brigadier 风格的树、坐标、方块状态、选择器，纯逻辑，不知道网络 |
| `crates/mc-bamboo-recipes` | 配方查找和匹配：有序、无序、烧炼、切石机、锻造、酿造和特殊配方，纯逻辑，不知道网络和世界线程 |
| `crates/mc-bamboo-loot` | 战利品表：方块掉落已经接进生存和冒险挖掘、爆炸、可替换方块的破坏和 `/loot`，箱子填充接进开箱和容器读写，实体死亡还没接进游戏 |
| `crates/mc-bamboo-core` | 共用的基础类型：方块坐标、区块坐标、方向、玩家档案 |
| `crates/mc-bamboo-auth` | 正版验证和协议加密：RSA 密钥对、AES/CFB8、serverId 哈希、问 session server |
| `crates/mc-bamboo-nbt` | NBT 编解码，解码带官方的配额和深度上限 |
| `crates/mc-bamboo-data` | 版本数据：由官方服务端生成的协议号、包 ID、方块、物品、配方、战利品表、注册表和标签 |
| `apps/desktop` | 桌面控制台：Tauri + React + styled-components |
| `tools/mc-bamboo-vanilla` | 开发工具：生成版本数据、用官方代码验证 Bamboo 的协议、方块行为、光照、菜单、熔炉、酿造台和营火的逐刻状态、物品组件、方块谓词、配方、战利品表、世界边界和世界生成 |

依赖方向是单向的：

```text
mc-bamboo   apps/desktop
       \     /
  mc-bamboo-server ─── mc-bamboo-auth
         |
   mc-bamboo-game
    /    |     \
protocol blocks world
```

`world` 和 `recipes` 只依赖 `core` 和 `data`；`protocol` 也依赖它们，另外依赖 `nbt`；
`blocks` 也依赖 `protocol`，容器里的物品直接用 `protocol` 的 `ItemStack`。
`command` 依赖 `core`、`data`、`blocks` 和 `uuid`；只有 `game` 依赖它。它不认识网络，也不认识世界线程。
`recipes` 依赖 `data` 和 `protocol`：组装产物要写组件。它不认识数据包的帧，也不认识世界线程。
`auth` 只有 `server` 依赖它，它自己只依赖 `core`：它不认识数据包，也不认识世界线程。
`worldgen` 的地形生成还没接进游戏；`game` 只用它算 `Biome.getTemperature`（天气的结冰、积雪和降水类型），
工具里的 oracle 用其余的数学。它依赖 `data`（官方导出的世界生成数据和群系气候表）和
`blocks`（里面有官方的 `LegacyRandomSource`）。
`loot` 算方块掉落、实体死亡和箱子填充，由 `game` 在挖掘、爆炸、`destroyBlock`、`/loot` 和容器第一次读写时调用；
它依赖 `data`（嵌入的战利品表 JSON）和 `blocks`（随机源、方块状态）。

## 线程模型

```text
连接任务（tokio，每玩家一个）               世界线程（20 TPS）
  读帧、解码、回心跳 ─── 请求队列 ───▶  按到达顺序处理，推进世界
  写 socket ◀──────── 出站队列 ────────  每 tick 给每名玩家写一批
```

世界状态只归世界线程所有，不用锁。连接任务只做 IO，世界线程不等待任何 IO。
这个结构相当于只有一个 region 的 [Folia](https://github.com/PaperMC/Folia)：
`Global` 和 `Region` 从一开始就分开，以后可以一个维度一个线程，再往后才考虑按区域并行。

## 升级游戏版本

版本相关的数字都在 `mc-bamboo-data` 里。升级时从官方下载 `server.jar`，再运行：

```bash
cargo run -p mc-bamboo-vanilla -- generate <server.jar>
```

然后启动 Bamboo，用官方网络代码让三个客户端走一遍进服、移动、建造、聊天和开关箱子，检查收到的每个包。
区块包里的方块和流体计数、高度图都由官方代码从解出来的方块重算，再和收到的值逐段比较：

```bash
cargo run -p mc-bamboo-vanilla -- probe 127.0.0.1 25565
```

`probe` 连的是你启动的那个 Bamboo，它跑的是离线模式。正版验证那一段要换一个服务端（`online_mode`
在启动时就定下来了），所以 `probe` 会在自己的进程里再起一个开了正版验证的 Bamboo，外加一个假的
session server 顶替 Mojang，让官方客户端代码走完加密登录，全程只走 `127.0.0.1`，不访问外网。

最后让官方代码给方块行为出题和给答案，用 `mc-bamboo-blocks` 重放，比较两边的结果：

```bash
cargo run --release -p mc-bamboo-vanilla -- oracle <java 路径> [--cases N] [--only <方块或类名>] [--weather clear|rain|thunder]
```

下落方块和点燃的 TNT 的物理也一样：官方 `FallingBlockEntity.tick` 和 `PrimedTnt.tick` 在假世界里跑，
`mc-bamboo-blocks` 重放，每刻的位置、速度、落地后的方块和引信按 `Double.doubleToRawLongBits` 逐位比较：

```bash
cargo run -p mc-bamboo-vanilla -- entity-oracle <java 路径>
```

玩家移动也一样：官方 `handleMovePlayer` 在假世界里处理给定的方块和一串移动包，
Bamboo 重放，比较接受、拉回还是断开，以及最终位置和摔落距离的原始位：

```bash
cargo run -p mc-bamboo-vanilla -- move-oracle <java 路径> [--cases N] [--seed S]
```

光照也一样：官方光照引擎在随机场景里改方块、算光照，`mc-bamboo-world` 重放，逐段比较每个光照值：

```bash
cargo run --release -p mc-bamboo-vanilla -- light <java 路径> [--scenes N] [--seed S]
```

菜单也一样：官方菜单代码跑随机点击序列，`mc-bamboo-game` 重放，逐槽、逐个 `stateId` 比较：

```bash
cargo run --release -p mc-bamboo-vanilla -- menu <java 路径> [--cases N] [--seed S]
```

物品组件也一样：官方代码把全部物品的默认组件（还要和生成的 `item_components.rs` 对一遍）、
创造模式物品栏的全部物品、一批手写的组件，再加上按 ID 查表的枚举的越界输入
按官方的 `StreamCodec` 编码，`mc-bamboo-protocol` 解码再编码，逐字节比较，
最后把 Bamboo 写出来的字节交回官方解码，确认解出来的还是同一份组件。
同一批物品堆再用 `ItemStack.CODEC` 和 `DataComponentPatch.CODEC` 编成 NBT，
Bamboo 解码再编码后逐项相等，官方也能解回同一份物品堆；每份的 `HashedStack` 哈希也要一致。
另外比对一批 SNBT 的打印和解析：

```bash
cargo run -p mc-bamboo-vanilla -- components <java 路径> [--show N]
```

冒险模式的方块谓词用官方的 `StatePropertiesPredicate` 和 `BlockPredicate` 出题：随机挑方块状态和谓词，
覆盖方块列表、方块标签、精确值、区间、缺一端、错值、不存在的属性名、Unicode 十进制数字和 NBT，按官方 `StreamCodec`
编码后交给 `mc-bamboo-blocks` 解码重放，逐条比较匹配结果：

```bash
cargo run -p mc-bamboo-vanilla -- predicate-oracle <java 路径> [--cases N] [--seed S]
```

配方用官方的 `RecipeManager` 出题：每条有序和无序配方按自己的原料摆出来，加上平移、镜像、
换物品、缺一格、多一格和数量不同，再把每种烧炼、切石机、锻造和酿造的输入过一遍，加上随机网格；
`mc-bamboo-recipes` 重放后比较全部匹配的配方、第一条和产物的物品数量：

```bash
cargo run --release -p mc-bamboo-vanilla -- recipe-oracle <java 路径> [--cases N] [--seed S]
```

战利品表也一样：官方对随机方块状态、工具、爆炸半径和世界种子跑 `LootTable.getRandomItems`，
箱子题再按 `fill` 放进容器，实体题按 `LivingEntity.dropFromLootTable` 的参数（不放 `LAST_DAMAGE_PLAYER`）跑死亡表。
`mc-bamboo-loot` 重放，比较物品、数量、两边都能表示的组件，以及跑完后随机源的下一个 `nextLong`。
造不出的实体、袭击查询和探险地图的结构搜索会记成排除：

```bash
cargo run -p mc-bamboo-vanilla -- loot-oracle <java 路径> [--cases N] [--seed S] [--show N]
```

世界生成的数学也一样：官方代码按一批种子和一批坐标算出每条噪声的两条求值路径、
每条注册表密度函数和七套维度设置的噪声路由的两条路径，还有三个维度的群系，
`mc-bamboo-worldgen` 重算，浮点按 `Float.floatToRawIntBits` 逐位比较：

```bash
cargo run --release -p mc-bamboo-vanilla -- worldgen <java 路径> \
    [--seeds N] [--salt S] [--points N] [--biome-points N] [--show N]
```

区块级对拍另有一道：官方在 JVM 里用 `ChunkGenerator`、`RandomState` 和 `ProtoChunk` 生成同一个
主世界区块到 `biomes` 和噪声填充（26.3 的 `terrain` 步只跑 `doFill`），逐方块、逐群系比较；
不启动服务端、不接受 EULA。细节和耗时见 `docs/worldgen.md`：

```bash
cargo run --release -p mc-bamboo-vanilla -- chunkgen <java 路径> \
    [--seeds N] [--chunks N] [--salt S] [--biomes-only] [--show N]
```

存档的编解码层也一样：官方 `SerializableChunkData.parse` 读 Bamboo 写出的区块 NBT 再
`write()` 回来，官方写出的 NBT Bamboo 读成 `ChunkColumn` 再写出来，两边都按 NBT 的键
比较（`CompoundTag` 用 `HashMap`，键序是哈希序）；官方 `RegionFile` 和 Bamboo 的区域文件
三个压缩 ID 双向对读，外加官方 `NbtIo.writeCompressed` 的 gzip 文件。
`StorageOracle.java` 只在 JVM 里调官方类，不启动服务端、不接受 EULA：

```bash
cargo run -p mc-bamboo-vanilla -- storage <java 路径> [--show N]
```

带槽位物品、玩家数据和 `level.dat` 的编解码也一样：官方 `ItemStack.CODEC` /
`ItemStackWithSlot.CODEC`、`TagValueInput` 和 `PrimaryLevelData.parse` / `createTag`
双向按键比较，不认识的键原样保留；只在 JVM 里调官方类：

```bash
cargo run -p mc-bamboo-vanilla -- player-data <java 路径> [--show N]
```

区块上层内容的存档 NBT（方块实体、计划刻、三种实体）另有一道对拍：
`ChunkContentOracle.java` 在 JVM 里逐类型调官方 `BlockEntity.loadStatic` /
`saveWithFullMetadata`、`SavedTick.codec` / `LevelChunkTicks.pack` 和
`EntityType.loadEntityRecursive` / `Entity.save`，Bamboo 用 `mc-bamboo-blocks::archive`
与 `mc-bamboo-storage::chunk_content` 重放，按键比较规范 JSON；另外用官方区块解析器
生成一份完整区块，Bamboo 读成 `RegionColumn` 再写出，按键比较。实体读取用进程内的假
世界，不启动服务端、不接受 EULA：

```bash
cargo run -p mc-bamboo-vanilla -- chunk-content <java 路径> [--show N]
```

编解码的耗时和压缩前后字节数（超平坦和内容丰富的合成区块各一份，解码拆成解压、
NBT 解析、组装三段）：

```bash
cargo test --release -p mc-bamboo-storage chunk_nbt_cost -- --ignored --nocapture
```

区块的加载/模拟等级也一样：官方 `DistanceManager` 在 JVM 里直接跑（不用 `ServerLevel`），
随机加票、撤票、玩家移动、改视距和模拟距离、推进若干刻，`TicketTable` 重放，逐步比较
49×49 窗口里全部区块的两套等级。事实、用法和结果见 `docs/chunk-tickets.md`：

```bash
cargo run -p mc-bamboo-vanilla -- ticket-oracle <java 路径> \
    [--scenes N] [--seed S] [--scene I] [--detail] [--show N]
```

世界边界也一样：官方 `WorldBorder` 在 JVM 里对随机的中心、大小、绝对最大值和渐变出题，
比对距离、`isWithinBounds` 的几种重载、`clampToBounds`、`isInsideCloseToBorder` 与
`Shapes.collide` 的碰撞结果、逐刻渐变后的尺寸和状态，以及六个 clientbound 包的官方编码；
不启动服务端，用官方类直接算：

```bash
cargo run -p mc-bamboo-vanilla -- border-oracle <java 路径> [--cases N] [--seed S] [--show N]
```

这些命令需要 Java 25 或更新版本。

## License

代码使用 [AGPL-3.0-only](LICENSE) 许可。以下内容不在 AGPL 许可范围内：

- `assets/icon.png`、`apps/desktop/src/assets/icon.png` 和 `apps/desktop/src-tauri/icons/` 是 Minecraft 的竹子贴图，版权归 Mojang 所有。
- `crates/mc-bamboo-data/src/generated/` 里的数据来自官方服务端，相关权利归 Mojang 所有。
- `apps/desktop/src/assets/fonts/` 是 [Fusion Pixel Font](https://github.com/TakWolf/fusion-pixel-font)，许可证见同目录的 `licenses/`。

Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.
