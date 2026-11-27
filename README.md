<div align="center">

<img src="assets/icon.png" width="128" alt="Bamboo" />

# Bamboo

用 Rust 从零编写的 Minecraft 服务端

A Minecraft server written from scratch in Rust

</div>

> 项目处于早期阶段。多名玩家已经能用 Minecraft 26.3 进入同一个世界，走动、互相看见、放置和破坏方块、在生存模式里按方块硬度和工具挖方块、在告示牌上写字、用书与笔写字并署名、阅读成书、把书放到讲台上翻页、让活塞推方块、打开箱子、聊天。新世界默认 `normal`（和官方一样的噪声地形：群系、地表、洞穴峡谷），`level-type=flat` 可切回超平坦；terrain / 地物 / 结构阶段的纯函数已接进 Director 流水线并逐方块对拍 0 不一致；**自然刷怪只覆盖已建模的生物**（`MobType::ALL` 的 78 种，见「自然生成」一节；群系刷怪表里其它条目仍占权重但不生成），它们也能用命令和刷怪蛋生成。放置方块仍然不扣物品；上鞍、刷怪蛋等会在生存模式扣一个（往讲台上放书也是例外：生存模式扣一本，创造模式不扣）。生存和冒险模式挖掉方块会按战利品表掉在地上，地上的物品能捡起来，按 Q 能丢出来。有 Anvil 存档：关服再开，改过的区块、方块实体、计划刻、已实现的实体、玩家位置和物品栏都还在，官方服务端能接着读这个世界目录（见「存档」一节和下面的已知限制）。服务器目录里的 `server.properties` / `bamboo.toml` 可以调视距、线程数、红石缓存、区块计算池，以及 `level-seed` / `level-type` / `generator-settings` / `generate-structures` 四个世界生成键；世界目录按 `level-name` 放在服务器目录下。

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

打包用 `pnpm tauri build`（生成 MSI 和 NSIS 安装包）。只要免安装的单个程序时用 `pnpm portable`
（即 `tauri build --no-bundle`），产物是 `target/release/bamboo-desktop.exe`，不生成安装包。

桌面端有总览、控制台、玩家和设置四个页面。设置页分成「官方设置」和「Bamboo 优化设置」
两个分区，读写 APP 数据目录 `server/` 下的 `server.properties` 和 `bamboo.toml`，
和命令行版完全是同一套文件。每一项都写了用途和默认值，越界的值和互斥的组合
（比如正版验证加代理转发）会当场提示。「世界生成」一组能改新世界的种子、世界类型、
生成结构开关和超平坦预设；种子和世界类型只对新建的世界生效，已有存档不会变。
「Bamboo 优化设置」能调世界生成的地形和地物缓存；两份缓存按依赖释放，并默认在
生成空闲 30 秒后清空。不改生成结果、不卸载区块，也不保证进程工作集降回启动时的大小。
老版本的 `server-settings.json` 第一次启动时
导入一次，之后不再读它。服务端只在启动时读一次，所以改完要重启服务器才生效。

控制台顶部有 CPU 和内存监控，每秒更新一次，曲线显示最近 60 秒，参照任务管理器。
服务端跑在应用进程里，所以显示的是这个进程占整机的 CPU 和它的工作集，旁边附整机的 CPU 和内存；
WebView2 的渲染进程不算在内。

## 存档

命令行版和桌面版默认把世界存在 `服务器目录/<level-name>/`（`level-name` 默认 `world`），
用官方 26.3 的 Anvil 布局：`level.dat` / `level.dat_old` / `session.lock`、
`data/minecraft/*.dat`（游戏规则、世界时钟、流浪商人的 `wandering_trader.dat`、世界生成设置、命令存储等）、`players/data/<uuid>.dat`（含 `.dat_old`）、
`dimensions/minecraft/overworld/{region,entities,poi}/` 和
`dimensions/minecraft/overworld/data/minecraft/chunk_tickets.dat`。
`ServerConfig.world_dir` 为 `None` 时不落盘（单元测试、oracle、压测都走这条路）。

一条 IO 线程负责区域文件的压缩和读写；世界线程绝不等待 IO：启动时扫一遍区域文件头，
得到「盘上有哪些区块」的内存集合（写盘时更新）。**带世界目录时所有区块都走同一条异步读
流水线**：盘上有数据就发异步读、盘上没有时 IO 线程回 `chunk: None`，两种来源的结果都在
下一刻开头经信箱收回、按距离排序交付；没接世界目录（测试、对拍、压测）仍是当刻同步走
超平坦模板，停放表命中的写盘途中盒子也是同步快路径。NBT 解析 / 序列化默认在世界线程
由 `InlineCodec` 内联做，`bamboo.toml` 里 `chunk_worker_threads ≥ 1` 时交给常驻计算池
（`Decode` / 每个 `Generate` 阶段 / `Save`），`Read` 仍在 IO 线程、世界线程只做记账。改过的区块无论卸载与否都照官方字节码的节奏保存：`MinecraftServer` 每 6000 刻
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
`flush` 失败作为错误报出，不当成功。关服的 `flush` 和计算池排空都有墙钟上限，
超时记错误并继续退出，绝不无限等。IO 线程 panic 或通道断开按设计稿 §14 / R12 当成
致命错误：记日志、走关服流程把还能同步写的 `level.dat` / `data/` 存下来，不挂死。

探针和每个测试都从空世界开始：单元测试 / oracle 用 `world_dir = None` 或临时目录，
不碰世界目录；`tools/probe.sh <目录> <端口> [--loaded]` 在临时目录里写好离线的
`server.properties` 再起 `bamboo`，`--loaded` 模式先建档再重启跑两遍完整 probe。
探针收尾让管理员执行 `/stop` 正常关服，脚本等服务端自己退出（最多 60 秒）当作一项
检查，再查 `level.dat` / 玩家数据 / 区域文件都在、并用 `mc-bamboo-vanilla world-dir`
让官方 codec 读一遍世界目录；没自己退出才 `kill -9` 并判失败。缺省配置下新世界和
`--loaded` 两遍都报 779/779。仓库里不留世界目录。

（2026-11-14 按实现更新：`docs/m3-storage.md` 第 14 节、`docs/world-model-design.md` 5.3.1、
`docs/profiling.md` 第 11 节、`crates/mc-bamboo-game/src/chunk_jobs.rs`、`tools/probe.sh`；
合并提交 `05d8fa1`、`082a7c9`、`54f5fd2`）

（2026-11-14 修 main 上两个回归：① 实体 tick 中被丢弃的实体曾在
`EntityStore::taken` 里永久留一行，让 `ready_to_move_chunks()` 永远为假，玩家全离开后
区块收回卡在 "not ready"、关服（等存完盘）因此挂死；现在 `end_tick` 回收这些借用槽位，
`drain_compute` 和 `ChunkIo::flush` 的关服等待都有墙钟上限，绝不无限等。② `05d8fa1`
（经 `1a0dee9` 合并）让所有区块走异步读流水线后，全新世界进服第一刻 `submit_reads`
取 `region.players` 得到空表，读请求按坐标排序、远处区块抢先到达；现在改从 Director
票表取玩家中心，全新世界和带存档世界第一批都按距离交付。详见 `docs/m3-storage.md`
第 14 节「2026-11-14 修 main 上两个回归」。）

### 存档已知限制

- 只做 Anvil。Linear、lz4（压缩 ID 4）、自定义压缩（ID 127）、老布局转换都不做；
  读到老布局（世界根有 `region/`）当空世界并写日志（Q1/Q2/Q8）。
- 不逐次强制刷盘：写盘在 IO 线程异步做，关服才 `sync_all`（Q4）。`kill -9` 崩溃最多丢
  最近还没匀速轮到的脏改动和没落完的快照；`/save-off` 期间不写盘也不卸载。
  方块实体里不存档的运行态（容器开合人数、告示牌编辑中的玩家）改了不标脏；
  以后接结构、生物群系存档时要补上对应的标脏入口。
- 只持久化改动过的（脏）区块。关结构的默认 flat 未改过的区块按模板重建；走多阶段的世界生成的区块
  一律算「改过」而写盘（官方可以丢弃原样生成区块，见世界生成已知限制）。
  `data/minecraft/world_gen_settings.dat` 用官方 `WorldGenSettings.CODEC` 写出这一局的
  种子和维度生成器：flat 写 `minecraft:flat` + 地层（层和超平坦模板一致），噪声世界写
  `minecraft:noise` + `biome_source` / `settings` 注册名；读档时以它为准，空 `level-seed`
  的随机种子重启后固定。`saved-data` 子命令对拍的是导出的**超平坦默认值常量**；运行时
  编码的噪声世界形状只验证了 Bamboo 自己读写一致，没单独用官方 codec 复核。
- 玩家数据按官方 `PlayerData` 的键恢复：位置取整到方块（小数部分丢掉、不补发位置包）、
  朝向、游戏模式、物品栏、末影箱、选中槽，以及血量、受伤 / 死亡计时、经验三项、
  饥饿与食物、属性、状态效果、能力、肩上鹦鹉、`RootVehicle` 和末影珍珠。
  能力仍按官方默认值恢复（`restore_inventory` 不读 `abilities`），其余字段由
  `runtime_player_data` / `restore_inventory` 写读往返，`player-data` 子命令与官方 codec 对拍。
- `level.dat` 写回运行时的出生点、默认游戏模式、难度（和 `time` / `last_played`），
  启动时读回：游戏时间从 `Time` 接着上次存盘的值走，岛的计划刻时钟也从这个值起算，
  所以漏斗、计划刻等和绝对游戏时间比较的逻辑在重启后不用退回 0；
  `data/minecraft/game_rules.dat` 用官方 `GameRuleMap.CODEC` 的键
  （带命名空间、布尔 Byte / 整数 Int）写 `/gamerule` 的真实值，启动时读回；
  世界边界用 `WorldBorder::export_settings` / `restore`（`restore` 传存档里的游戏时间）
  写到官方按维度存放的 `dimensions/minecraft/overworld/data/minecraft/world_border.dat`，
  启动时读回；`data/minecraft/world_clocks.dat` 用官方 `PackedClockStates.CODEC`
  写主世界和末地的两份世界时钟（`total_ticks` / `partial_tick` / `rate` / `paused`），
  启动时按官方 `ServerClockManager.init` 读回，昼夜时间也接着走。新世界主世界和末地时钟都用官方
  初值（0 刻、流速 1、不暂停），每刻按官方 `ServerClockManager.tick` 推进，`advance_time`
  规则为假时冻结（游戏时间仍照常加一）；`/time set`/`add`/`pause`/`resume`/`rate`/时间标记、
  时间线和 `query` 都挂在正确的时钟上。
  `data/minecraft/world_gen_settings.dat` 写官方 `WorldGenSettings.CODEC`
  形状的种子与维度生成器（见上一条），读档时再读回。游戏规则 / 世界边界 /
  世界时钟 / 导出的超平坦默认值常量用官方 codec 对拍（`saved-data` 子命令）。
  天气、计划事件、自定义 Boss 栏、末影龙战斗这些没建模的
  saved data 不写文件（官方 `computeIfAbsent` 会自己造默认值）；`/weather` 不写存档。
  流浪商人的 `data/minecraft/wandering_trader.dat`、袭击的 `data/minecraft/raids.dat`、
  地图的 `data/minecraft/maps/*.dat` 和 `/data` 的 `command_storage.dat` 都已按官方 codec
  写读（见 `save_data.rs` 与「流浪商人」「袭击」「地图」几节）。
  `chunk_tickets.dat` 写空 `tickets` 列表（无 `/forceload`）。
- 盘上 `Status` 不是 `minecraft:full` 的区块当成 ProtoChunk 读回：方块层、quart 群系、
  高度图、两层光照、`isLightOn`、结构起点 / 引用、PostProcessing、方块实体、计划刻和
  实体按盘上的内容恢复；Director 把盘上阶段记进记录，只跑它之后的阶段到 `Full`。
  下次写盘写真实阶段（官方 `getPersistedStatus()`），只有完工才写 `minecraft:full`。
  `.mcc` 外置大区块能扫到也能读回，丢了就按读坏处理。读坏的区域文件仍是 warn + 模板，
  不另存区块备份。官方 codec 的多阶段往返对拍见 `mc-bamboo-vanilla resume-world`：
  每个能存盘的阶段 × 四种邻居状态组合，逐格比方块、群系、高度图、方块实体、光照和结构，
  带结构 / 不带结构各一组，含三种改坏确认，全部 0 不一致。
- 玩家文件名照官方 `getStringUUID()` 用带连字符 UUID；坏玩家文件另存
  `<uuid>_corrupted_<yyyy-MM-dd_HH-mm-ss>.dat`（官方 `FileNameDateFormatter`，Bamboo
  按 UTC 算，官方按本地时区）。`/save-all` 的成功消息不带 `flush` 时在快照都交给 IO
  线程后发、带 `flush` 时等 IO 栅栏回执再发，都不在请求入队那一刻发（比官方同步保存晚，
  但世界线程不等待 IO）；`/save-all` 和匀速保存都用 `BorrowedParts` 从 `&Region` 只读拍快照，
  **不取出/放回盒子、不算完待算光照、不动区块包缓存、不改运行态表的顺序和版本号**。
- 火把抖动表、容器开合状态与计数、方块实体的 `dead` / `recheck_at` 等瞬时状态不存；
  世界随机源 / 随机序列、计分板 / 队伍 / Boss 栏也不存档（沿用原有限制）。

（2026-11-14 按实现更新：`docs/m3-storage.md` 第 14 节 `外置区块和非 full 的 ProtoChunk`、
`crates/mc-bamboo-game/src/region/content_snapshot.rs`、`crates/mc-bamboo-game/src/region/column.rs`
的 `encode_loaded_column`；合并提交 `7e23929`、`be36ac3`、`1828b8c`）

## 现在能玩什么

- **地图（第一、二期）**：照 26.3 字节码实现 `EmptyMapItem.use`、`MapItem`（`create` / `update` / `inventoryTick` / `useOn` / `onCraftedPostProcess`）、`MapItemSavedData`、`MapIndex` 与 `ClientboundMapItemDataPacket`（Play 客户端包 ID 52）。每个方块状态的 6 位 `MapColor.id`（0..=61）从官方 jar 导出并压缩进 `STATE_FACTS`。空地图右键按玩家所在方块坐标和比例网格对齐中心、经 `MapIndex` 分配递增 `map_id`（首个为 0）、非创造扣 1 个、记 `ITEM_USED` 统计、播 `ui.cartography_table.take_result`（音源 `PLAYERS`）并替换空手或放进背包 / 丢出。存档严格按 26.3 路径与 codec 写入全局 `data/minecraft/maps/last_id.dat` 与 `data/minecraft/maps/<id>.dat`（不是旧版 `idcounts.dat` / `map_<id>.dat`），含 `DataVersion`、GZIP 压缩 NBT、16384 字节 `colors`、`banners` 与 `frames`，接入启动读档与全量 / 自动 / 关服存盘。玩家每刻持有未锁定已填充地图时，主 / 副手槽逐列按 `HoldingPlayer.step` 采样地形坡度亮度、水深亮度、含水非完整支撑顶面方块替换（`getCorrectStateForFluidBlock`）、`MapColor.NONE` 向下跳过、`y <= minY` 兜底基岩与 `scale × scale` 众数（平票取首次出现）；`HoldingPlayer` 合并脏矩形并按 `tick % 5 == 0` 节流发送标记（含越界 `PLAYER_OFF_MAP` / `PLAYER_OFF_LIMITS`）。物品展示框放图前检查 `isTrackedCountOverLimit(256)` 超限返 `FAIL`，并在 `entity_tracking::sync` 中按实体自身 `ServerEntity.tickCount % 10 == 0`（从 0 起算）登记展示框绿标并向维度内玩家发包，移出或破坏展示框时核销标记。制图台接入真实 `MapStore` 预览并执行放大（`SCALE`）、锁定（`LOCK`，保留像素与旗帜并清空展示框标记）与复制（共享同一 `map_id`），在左键 / Shift / 快捷键 / 丢弃 / 创造中键克隆时剥离 `MAP_POST_PROCESSING` 并分配新 `map_id`，且按 `lastSoundTime` 同刻只播一次 `ui.cartography_table.take_result`。**对拍**：`mc-bamboo-vanilla map-oracle java` 用固定种子 `42` 的 8×9 真实生成区块（CARVERS）逐字节比对 `scale=0` 纯地形 16384 像素（含 2260 水像素）、`special0`（含水下置石半砖 / 玻璃 / 虚空基岩 / 水深）16384 像素、`scale=1` 16384 像素、`maps/0.dat` 与 `maps/last_id.dat` 双向官方 codec 往返，以及玩家持图行走（`scale=0` 20 刻、`scale=1` 10 刻增量脏矩形、锁定图 6 刻）每刻发出的 `ClientboundMapItemDataPacket` 精确线字节，全部 0 不一致；7 处对拍层改坏确认（`--break color|save|packet|fluid|banner-toggle|banner-check|map-extend`）均稳定报差异。第二期接上右键旗帜标记：手持已填充地图右键 `#minecraft:banners` 时按官方 `MapItem.useOn` → `MapItemSavedData.toggleBanner` 添加 / 取消标记（`MapBanner` 用 `banner-x,y,z` 作 id，颜色取旗帜底色、名字取方块实体 `CustomName`，锁定图也能切换，越界或追踪超 256 返回 `FAIL`），`MapItem.update` 在每根扫到的列上跑 `checkBanners`，旗帜被破坏 / 换方块 / 改色 / 改名时按官方时机移除标记并置脏；标记读写沿用第一期存档里的 `banners`，同步走 `getUpdatePacket` 与第一期多岛修好的 `Global` / `GlobalPort` 路径。工作台 3×3 `map_extending` 配方按官方 `MapExtendingRecipe.matches` 读地图存档查缩放等级（配方 crate 通过调用方传入的 `MapScaleLookup` 拿到 `scale`，不依赖 `Global`，只判 `scale < 4`，不检查 `locked`），`assemble` 用 `TransmuteRecipe.createWithOriginalComponents` 保留 `CustomName` 等原组件并写入 `MAP_POST_PROCESSING = SCALE`，取出时走 `MapItem.onCraftedPostProcess` 剥离该组件、分配新 `map_id` 并生成 `scaled()` 数据。`map-oracle java` 新增 3 组第二期对拍：带名红色旗帜连续两次右键（11 刻逐包字节、添加 / 取消后的官方 codec 存档 `banners`）、打掉蓝色墙旗帜后 `checkBanners` 的消失刻与 20 刻逐包字节、5 个 `map_extending` 配方用例（匹配 / 预览物品 / 后处理物品 / 新地图 `scale` / 中心 / `locked`）逐字节比对，全部 0 不一致；3 处第二期改坏确认（`--break banner-toggle` 漏名字、`--break banner-check` 漏消失、`--break map-extend` 误拒锁定图）均稳定报差异。**已知限制**：下界天花板噪点采样与指针乱转（Bamboo 只有主世界）、探险地图（`renderBiomePreviewMap` / 战利品表 `exploration_map`）暂未接入，留待后续分期。
- **矿车**：按 26.3 缺省的 `OldMinecartBehavior` 实现 10 种铁轨形状、斜坡、过弯、出轨、动力铁轨的加速和制动、实体碰撞推挤，以及普通矿车高速时自动搭载符合条件的非玩家实体。普通矿车可右键上车、潜行下车，方向按键轻推低速矿车并解除未通电动力铁轨的制动。玩家和发射器都能用矿车物品放置实体。运输矿车和漏斗矿车分别有 27 格和 5 格，共用实体容器菜单、战利品表解包和破坏掉落。方块漏斗能向运输船和容器矿车送出物品，也能从中抽取。漏斗矿车可以隔完整方块吸取上方的物品实体。动力矿车投煤加 3600 刻燃料，上限 32000 刻。TNT 矿车可由激活铁轨点燃，也会因高速撞击爆炸。受伤累计超过 40 时破坏，创造模式可直接移除。
  - 激活铁轨会逐出普通矿车乘客、切换漏斗矿车的吸取状态、点燃 TNT 矿车的 80 刻引信，并以 4 刻冷却触发命令方块矿车执行命令。探测铁轨输出 15，每 20 刻重检，比较器优先读取命令方块矿车的 `SuccessCount`，其次读取容器矿车的满度。命令、输出和成功次数可存档，命令和输出会同步。刷怪笼矿车复用方块刷怪笼的刷怪逻辑，按矿车的位置刷怪。
  - 船与矿车从放置物品继承自定义名字，存档并同步名字与可见性。运输船、运输矿车和漏斗矿车的菜单使用实体显示名，破坏后掉落的载具物品保留名字。
  - 实体对拍覆盖 12 个矿车逐刻运动、铁轨、容器、TNT 和燃料场景，并比较推牛、载猪、方块漏斗向运输矿车送货、激活铁轨触发命令、刷怪笼矿车刷怪，以及命名运输船和运输矿车的标题、存档、同步与掉落。实验特性 `minecart_improvements` 的 `NewMinecartBehavior` 尚未移植。
  - **命令方块矿车命令的执行时机（已知限制）**：官方 `OldMinecartBehavior.tick` 在 `moveAlongTrack` 之后、仍是矿车自己这一刻里调 `MinecartCommandBlock.activateMinecart`（`ACTIVATION_DELAY = 4` 的 `tickCount - lastActivated >= 4` 冷却，命中后 `lastActivated = tickCount`），后者当场调 `BaseCommandBlock.performCommand(ServerLevel)` 走 `Commands.performPrefixedCommand`：命令来源位置是矿车移动后的 `position()`，命令改写的方块、新生成的实体、传送当刻就能被 `entityTickList` 里排在它之后 tick 的实体看到，命令自己读到的也是矿车这一刻中途的世界。Bamboo 在 `minecart::tick`（`crates/mc-bamboo-blocks/src/entity/minecart.rs`）里只按官方当场生成 `CommandActivation`（位置、朝向、`game_time`）并 `emit` 成 `Command::ActivateMinecartCommand`；落地阶段（精确驱动 `apply`，并行驱动 `commit`）只把它转成 `Effect::RunMinecartCommand`，等整个实体阶段（`run_entity_ticks`）跑完才由主控经 `GlobalEffect::RunMinecartCommand` → `minecart_command::perform` → `command::run_prefixed` 执行（直接模式在 `riding::tick` 和方块实体阶段之前执行；排队模式不在此处执行）。因此同一刻里 `entityTickList` 排在矿车之后 tick 的实体看不到命令这一刻的改动，命令读到的也是全实体阶段结束之后的世界；多岛排队的 `GlobalPort::Queued` 还会再晚到主控应用那一步。此外 `minecart_command::perform` 到执行时才按 UID 重新取出命令方块：矿车在激活之后、延迟执行之前被同刻稍后的实体移除时，Bamboo 会整条跳过命令（不只是晚执行），官方 `MinecartCommandBlock.activateMinecart` 当场执行的命令不受后续移除影响；激活也没快照命令文本和 `TrackOutput`，执行时读的是当时的值。这不是移动逻辑的 bug：`mc-bamboo-blocks` 的实体 tick 不能反向依赖 `mc-bamboo-game` 的命令框架，`run_entity_tick` / 并行 `compute` 又把 `Region` 整份借进 `World`（`region.rs::entity_level`），拿不到 `perform` 要的 `&mut Region` + `&mut Global`；`docs/world-model-design.md` 第 8 节的「算阶段只读、对外写走缓冲 `Command`、按 uid 顺序落地」也不允许实体在逐刻里跑整条游戏层命令，并行驱动更没有逐实体的插入点。把执行挪回官方时机会改动 tick 顺序、实体写入规则和线程模型，按设计约束保留现状并列为已知差异。现有 `entity-oracle` 的 `command_minecart` 场景只比矿车自己的命令字段、输出、成功次数和比较器信号，没有同刻交错的第二只实体，不受这条差异影响；要补 `setblock` / `summon` / `tp` 与第二只同刻移动实体交错的场景，需要先解决上面的架构问题。本轮对照 26.3 的 `OldMinecartBehavior.tick` / `MinecartCommandBlock.activateMinecart` / `BaseCommandBlock.performCommand` / `EntityTickList.forEach` 逐行复核，确认把执行挪回移动途中必然改动 `docs/world-model-design.md` 第 8 节的写入规则、tick 顺序与并行线程模型，按设计约束维持延迟执行、不改行为；用户 2026-11-28 决定保留延迟执行；官方顺序、Bamboo 顺序、可选方案与代价见 `docs/world-model-design.md` 8.8，交错场景因此不做。
- **投掷末影之眼与框架装眼推实体**：`EnderEyeItem.use`、`EyeOfEnder` 与 `EndPortalFrameBlock` 装眼的 `Block.pushEntitiesUp` 照 26.3 字节码实现。右键末影之眼先做 `OUTLINE` 轮廓射线（`Fluid.NONE`），瞄着末地传送门框架时直接返回 `PASS` 交给 `useOn`；否则按 `ChunkGenerator.findNearestMapStructure` 在同心环候选区块里定位最近的 `#minecraft:eye_of_ender_located`（`minecraft:stronghold`），关闭结构或找不到要塞时返回 `CONSUME`（不生成、不扣物品、不播音、不记统计、不触发进度）；找到后在 `(player.x, player.getY(0.5), player.z)` 生成 `EyeOfEnder`，调用 `setItem`（空堆回退默认末影之眼、非空取数量 1）与 `signalTo(Vec3.atLowerCornerOf(target))`（水平距离超 12 格时截断至 12 格并抬升 +8 格，否则直飞目标点，并由实体随机源掷 `nextInt(5) > 0` 定 `surviveAfterDeath`），投递 `PROJECTILE_SHOOT` 游戏事件、触发 `minecraft:used_ender_eye` 进度判据（按水平距离平方比 `distance`）、播放 `entity.ender_eye.launch` 音效（音高 `Mth.lerp(level.random.nextFloat(), 0.33F, 0.5F)`；26.3 字节码无 1003 世界事件）、非创造扣 1 个、记 `ITEM_USED` 统计并返回 `SUCCESS_SERVER`。`EyeOfEnder` 每刻按 `updateDeltaMovement` 逐指令更新速度并移动，第 81 刻（`++life > 80`）播放 `entity.ender_eye.death`、移除自身，按 `surviveAfterDeath` 生成 `ItemEntity` 或发 2003 碎裂世界事件；同步 `DATA_ITEM_STACK`（index 8），按官方未设 `noSave()` 正常写入区块实体存档（`"Item"` 键）。装眼 `EnderEyeItem.useOn` 在写入 `HAS_EYE=true` 前执行 `Block.pushEntitiesUp`，把站在空框架新增眼柱碰撞盒上的非旁观者实体与玩家沿 Y 轴推到顶面（`+3/16` 格）并在非创造模式扣 1 个末影之眼。**对拍**：`mob-oracle --only ender_eye --show 2000` 4 个场景（`ender_eye_drop` 86 刻远距离飞行与第 81 刻掉落物品轨迹、`ender_eye_shatter` 86 刻近距离直飞与第 81 刻 2003 碎裂、`ender_eye_frame_push` 11 刻装眼只推起站中央的牛而旁侧对照牛不动、`ender_eye_save` 11 个 NBT 键双向往返），除 12 处已知三角函数表末位差外 0 不一致；3 处改坏确认（`ender_eye_life`、`ender_eye_target`、`ender_eye_push`）均稳定触发对拍失败。**已知限制**：为保持既有 `BlockOracle` 兼容，装满 12 眼开传送门时暂未额外发 1038 `globalLevelEvent`（`BlockOracle.java` 的 `globalLevelEvent` 为空）；客户端粒子轨迹由客户端本地渲染，服务端不发粒子包。
- **信标（`minecraft:beacon`）**：照 26.3 字节码实现 `BeaconBlockEntity`、`BeaconBlock`、`BeaconBeamBlock`、`BeaconMenu` 与 `ServerboundSetBeaconPacket`。每刻向上最多扫 10 格更新 `checkingBeamSections`：信标本身起白色段，第一块染色玻璃 / 染色玻璃板按 `DyeColor.getTextureDiffuseColor()` 开启纯色段（不与白色混合），后续不同色按 `ARGB.average` 逐通道求均值，基岩透光、`lightDampening >= 15` 的不透明方块清空光柱段。每 80 刻（`gameTime % 80 == 0`）在光柱非空时用 `updateBase` 检查下方 1–4 层 `#minecraft:beacon_base_blocks`，有层数且光柱非空时向 `AABB(pos).inflate(levels*10+10).expandTowards(0, height, 0)` 内非旁观者玩家施加持续 `(9 + levels*2) * 20` 刻的环境效果（4 层副效果等于主效果时主效果升 II 级；否则 4 层不同副效果另施 I 级）并播 `block.beacon.ambient`；扫达 `WORLD_SURFACE` 切换 `beamSections`，层数由 0 变正播 `block.beacon.activate` 并向 `AABB(x,y,z,x,y-4,z).inflate(10,5,10)` 内非旁观者玩家触发 `minecraft:construct_beacon` 进度，由正变 0 播 `block.beacon.deactivate`（拆除同样播失效音）。右键打开 `BeaconMenu`（37 格含单个 `#minecraft:beacon_payment_items` 付款槽与 3 个数据槽 `levels` / 编码主效果 / 编码副效果），支持 `CustomName` 与 `lock` 锁定检查、记 `INTERACT_WITH_BEACON` 统计；关菜单时未消耗的付款物照官方 `player.drop(item, false)` 扔出；收 `SET_BEACON` 包走 `validateEffects`（生命恢复仅作 4 层副效果、1–3 层副效果须与主效果一致、层数不足拒绝），校验失败或无付款物照官方 `multiplayer.disconnect.generic` 断开连接，合法则扣 1 个付款物、写效果并播 `block.beacon.power_select`。客户端自己跑 `BeaconBlockEntity.tick` 计算光柱，服务端 tick 中不主动发方块实体更新包，仅在 `getUpdateTag`（`saveCustomOnly`）与区块/方块更新时同步。存档读写 `primary_effect` / `secondary_effect`（按 6 种合法效果过滤）/ `Levels`（写入但不读盘）/ `CustomName` / `lock`（空锁不写），接入隐式物品组件、`block_entity_data` 与 `/data`。**对拍**：`machine java` 新增 6 个信标场景（1–4 层建塔、染色玻璃/玻璃板混色与基岩透光与石块挡光恢复、4 层生命恢复副效果与付款校验、4 层 II 级效果与拆塔失效音、2 层范围与时长边界、存档过滤与往返，共 1150 刻逐刻比数据槽/光柱段/音效/玩家效果，0 不一致）；`chunk-content java` 新增 `beacon-full` 双向往返（666 项全过）；3 处生产代码改坏确认（首块玻璃误与白光混合 41 处不一致、效果时长公式偏移 3 处不一致、`validate_effects` 放行生命恢复作主效果 5 处不一致）均稳定触发对拍失败。

- **潮涌核心（`minecraft:conduit`）**：照 26.3 字节码实现 `ConduitBlockEntity.serverTick` 与 `ConduitBlock` 的含水放置 / 建实体：`gameTime % 40 == 0` 先查核心周围 3×3×3（含自身）共 27 格是否全是 `#minecraft:water`（`isWaterAt`），再按官方 42 格坐标表扫 5×5×5 外壳上三个坐标平面的外环，统计 `#minecraft:conduit_effect_block`（海晶石 / 海晶石砖 / 海灯笼 / 暗海晶石）；至少 16 块激活，42 块进入捕猎。形状变化时播 `block.conduit.activate` / `block.conduit.deactivate`，活跃时每 80 刻播 `block.conduit.ambient`，每刻 `gameTime > nextAmbientSoundActivation` 时取世界随机源 `nextInt(40)` 排下一次并播 `block.conduit.ambient.short`；`applyEffects` 半径是整数除法 `blocks / 7 * 16`（16 块 32 格、42 块 96 格），给 `AABB(pos).inflate(radius).expandTowards(0, height, 0)` 内、整格距离平方 `< radius²` 且 `isInWaterOrRain()` 的玩家施加 260 刻 `conduit_power`（I 级、ambient / visible / showIcon 均真）。满框架时 `selectNewTarget` 在 `AABB(pos).inflate(8)` 内用 `Util.getRandom(list, level.random)` 抽一只水里 / 淋雨的 `Enemy`，播 `block.conduit.attack.target` 并用 `damageSources().magic()` 造成 4 点伤害；目标引用按 UUID 检查存活和 8 格距离，变化时 `sendBlockUpdated(pos, state, state, 2)` 让客户端重读 `getUpdateTag`。存档只写 `Target`（`UUIDUtil.CODEC` 的 `int[4]`）；眼睛开合、`activeRotation += 1`、鹦鹉螺粒子和 `getActiveRotation` 的 `-0.0375f` 旋转都是官方 `clientTick` 客户端自算，服务端不逐刻发这些数据。**对拍**：`machine java` 新增 6 个潮涌场景（16 / 42 块框架、水中 / 缺水失效与补回、范围内外玩家效果、满框架每 40 刻攻击一只溺尸、魔法伤害 / 血量 / 音效 / 世界随机源 / `Target` 引用 / 方块更新，`Target` 存档往返，共 455 刻逐刻比对，0 不一致）；4 处改坏确认 `conduit_min_active`（47）、`conduit_duration`（3）、`conduit_attack`（3）、`conduit_target`（1）均稳定报不一致。**已知限制**：客户端的鹦鹉螺粒子不建模；服务端目标引用按本岛实体表解析，跨维度的 `getEntityInAnyDimension` 不查别的岛。
- **钓鱼**：`FishingRodItem.use` 与 `FishingHook` 实体照 26.3 字节码实现。无浮漂时抛出（按眼高偏移、3× `triangle(0.5, 0.0103365)` 初速度与朝向，播放 `entity.fishing_bobber.throw`、记 `ITEM_USED` 统计，读取 `lure` 附魔每级减 100 刻与 `luck_of_the_sea` 每级加 1 幸运）；有浮漂时收竿（`retrieve` 扣耐久：咬钩 1、钩住实体 5、触地覆盖为 2，播放 `entity.fishing_bobber.retrieve`）。玩家保存 `fishing` 引用，换手不再持有钓竿、距离超 32 格、死亡 / 旁观或离服时按 `shouldStopFishing` 自动移除浮漂。`FishingHook` 具备三态状态机（`FLYING` / `HOOKED_IN_ENTITY` / `BOBBING`）、重力（`0.03F`）、浮力与阻尼、实体 / 方块 / 世界边界反弹碰撞、落地 1200 刻消失、钩住实体跟随与拉拽（`pullEntity`：生物与掉落物在服务端叠加速度，玩家广播 31 号实体事件），同步 `DATA_HOOKED_ENTITY`（index 8）与 `DATA_BITING`（index 9，触发 `onSyncedDataUpdated` 下沉初速），`AddEntity` 的 `data` 传主人实体 ID；照 `noSave()` 不写入 Anvil 区块存档。咬钩计时（`catchingFish`）每刻用 `uuid.lsb ^ gameTime` 重设 `synchronizedRandom`，涵盖 `timeUntilLured` / `timeUntilHooked` / `nibble`、下雨（25% 加速）与遮天（50% 减速）、服务端 `BUBBLE` / `FISHING` / `SPLASH` 粒子包与溅水音效，以及 5×4×5（`dy = -1..=2`）`calculateOpenWater` 露天判定。收竿按 `minecraft:gameplay/fishing` 战利品表求值（照 26.3 字节码传 `ORIGIN`、`TOOL`、`THIS_ENTITY` 与幸运值，不传钓者参数），露天钓鱼可出宝藏、非露天不出宝藏；渔获按官方速度公式朝玩家飞出，经验球生成于 `(owner.x, owner.y + 0.5, owner.z + 0.5)` 且经验值为 `hook.random.nextInt(6) + 1`；属 `#minecraft:fishes` 的渔获记 `FISH_CAUGHT` 统计，并触发 `minecraft:fishing_rod_hooked` 进度判据。**对拍**：`mob-oracle --only fishing --show 2000` 5 个逐刻场景（抛竿飞行落水 41 刻、完整咬钩收竿 146 刻、钩牛拉拽 9 刻含实体事件 31 与 5 点耐久、露天出宝藏 125 刻 + 30 次抽样、非露天 0 宝藏 125 刻 + 30 次抽样，共 446 刻逐刻比坐标/速度/朝向/状态/同步数据/计时器/粒子包/随机源与结算，除 12 处已知三角函数表末位差外 0 不一致）；3 处生产代码改坏确认（`fishing_gravity`、`fishing_open_water`、`fishing_pull`）均稳定触发对拍失败。结算的幸运值是 `luck_of_the_sea` 加上玩家的 `luck` 属性（幸运 / 霉运效果会改它）；抛竿 / 收竿照 `ItemStack.causeUseVibration` 发 `ITEM_INTERACT_START` / `ITEM_INTERACT_FINISH`，受 `use_effects.interact_vibrations` 控制，幽匿感测体能收到。**已知限制**：浮漂按官方 `noSave()` 不持久化到存档。
- **盔甲架（`minecraft:armor_stand`）**：官方 `ArmorStand` 是 `LivingEntity` 一支、没有 AI，Bamboo 复用 `Mob` 结构并用 `noAi` 关掉 AI。普通 / 小型 / 标记三种尺寸由 `DATA_CLIENT_FLAGS`（index 15，BYTE）和 6 个姿势（index 16..21，ROTATIONS）同步；存档写 `Invisible` / `Small` / `ShowArms` / `NoBasePlate` / `Marker` / `Pose` / `DisabledSlots` 和装备。`ArmorStandItem.useOn` 按玩家朝向取整到 45°、查方块与实体碰撞、应用物品的 `entity_data` 组件、发放置音效与 `ENTITY_PLACE`；落点 y 由 `EntityType.create(..., SPAWN_ITEM_USE, true, true)` 的 `getYOffset` 决定（碰撞检查 AABB 用 `Vec3.atBottomCenterOf(clickedPos)`，`shouldOffsetYMore` 恒真）：点顶面、放置格下方有支撑时 `yOffset = 0`，盔甲架站在方块顶；点侧面或悬空时 `yOffset = -1`，低一格。`level.getEntities(null, aabb)` 把玩家也算进去，玩家占着放置格时放置失败。右键 `interactAt` 按点击高度或物品的 `equippable` 槽位穿戴 / 取下 / 交换装备，遵守 `DisabledSlots` 的取 / 放 / 总锁定位和标记 / 无手臂限制，创造模式只复制一个。`hurtServer` 走两段式：5 刻内第二下才破坏，创造一下移除，火 / 爆炸 / 箭各有特殊分支，破坏掉带 `CustomName` 组件的盔甲架物品和全部装备，播放破坏音效与带橡木木板状态参数的 `BLOCK` 粒子；标记或 `NoGravity` 的盔甲架没有物理、不掉落，`isPushable` 恒假。命令 `/summon armor_stand` 带 NBT 走通用读档路径。`hurtServer` 还照字节码接了 `Player.mayBuild`、`mobGriefing` 关闭且来源是 `Mob`、旁观者与命名牌的交互门控；存档照父类链只写到 `LivingEntity`（`Brain` 由 `LivingEntity` 无条件写，保留），不写 `Mob` / `AgeableMob` 层的 `CanPickUpLoot` / `NoAI` / `Age` / `InLove` 等键，战利品表用空的 `minecraft:entities/armor_stand`。发射器按 `EquipmentDispenseItemBehavior` 给前方盔甲架穿戴装备（`ArmorStand.canUseSlot` 排除 BODY / SADDLE 与 `DisabledSlots` 锁定的槽）；按 `DispenseItemBehavior$1` 在正前方一格生成盔甲架（`EntityType.create` 取一次 `level.random`，链上依次 `setYRot(Direction.toYRot())`、`applyComponentsFromItemStack` 的 `CUSTOM_NAME`、`TypedEntityData.loadInto` 的 `ENTITY_DATA`——后者是 `saveWithoutId` → `merge` → `load`，保留前两步的状态；扣一个，外层发 1000 / 2000），`oracle java` 有 3 个固定发射器题真比较余量、事件与世界随机源（0 差异），方块对拍里盔甲架的发射器排除已去掉。命名牌按 `NameTagItem.interactLivingEntity` 给盔甲架命名（有 `CUSTOM_NAME` 组件时写 `CustomName`、生存模式消耗一个；盔甲架是 `LivingEntity` 非 `Mob`，官方不设 `PersistenceRequired`）。**对拍**：`mob-oracle --only armor_stand --show 2000` 共 5 个场景（4 个逐刻：放置后掉落到石头地面、带点击高度穿戴 / 取下装备、两下打碎比掉落物与装备、被箭射；1 个存档往返：整棵 NBT 逐键比 `Pose` / `DisabledSlots` / `Invisible` / 标志 / 装备），逐刻比位置 / 速度 / 着地、八个装备槽与玩家手持、`lastHit` / 血量 / 掉落物，0 场景差异；5 处改坏确认 `armor_stand_no_physics` / `all_pose` / `no_break` / `arrow_damage` / `wrong_slot` 分别在对应场景报不一致（基线只有 12 处三角函数表末位差）。对拍还查出并修了两处生产 bug：全缺省的 `Pose` 应写成空复合标签（`ArmorStandPose.CODEC` 省略等于缺省值的字段），以及 `MobType::ArmorStand` 缺属性分支、曾落到马科缺省的 53 点血（官方 `createLivingAttributes()` 是 20 点血、台阶高度 0）。**已知限制**：官方 `ArmorStand.pushEntities()` 会主动把附近的可骑乘矿车推开（矿车实体已建模，但盔甲架的这一支未接），`isPushable()` 恒假、`doPush()` 空已照官方（`push_entities` 对盔甲架直接返回）；红石 / 方块 oracle 里带 `entity_data` 的刷怪蛋仍按「留在槽里、生成失败」排除整题。

- **画、物品展示框和荧光物品展示框**：`Painting`、`ItemFrame` 与 `GlowItemFrame` 照 26.3 字节码接上 `BlockAttachedEntity` / `HangingEntity` 公共逻辑：精确碰撞箱计算（`calculateBoundingBox`、偶数宽高偏移与贴附角点）、`survives()` 存活与共存判定（每 100 刻后即第 101 刻检查背后支撑方块与同格冲突，画不支持任意朝向重叠，展示框允许不同朝向共存且支持 6 向及水平二极管支撑）、受击 / 箭矢 / TNT 与生物爆炸破坏（水中爆炸与 `mobGriefing=false` 免疫，掉落物按 `facing * 0.15F` 偏移并继承 `CustomName`）以及全套同步（`AddEntity` 发贴附方块角点与 3D 朝向 `data`、`HangingDirection`(8) / `PaintingVariant`(9，写 `id + 1`) / `ItemFrameItem`(9) / `ItemFrameRotation`(10) / 隐身标志 `0x20`，展示框每 10 刻先冲脏元数据、全默认元数据跳过配对 `SetEntityData`）。画右键放置走 `HangingEntityItem.useOn` → `Painting.create`，按墙面空间从 `#minecraft:painting_variant/placeable`（47 种）筛出存活的最大面积候选并用实体随机源随机抽取一张，也支持物品组件 `painting_variant` 指定变体。展示框支持空框放入物品（生存扣 1 个）、8 档旋转、隔实心红石导体向比较器输出模拟信号（`0` 或 `rotation % 8 + 1`，多框同向返空，比较器读数接入红石缓存 `ReadMethod::ItemFrameSignal` 守卫）、左键两步破坏（第一下打掉框内物品并刷新比较器、第二下打掉框本体）、`Fixed` / `Invisible` 与 `ItemDropChance`，以及带 `map_id` 组件物品的 1×1 包围盒扩展；荧光物品展示框照字节码覆写 5 种 `entity.glow_item_frame.*` 音效与掉落物品（发光为纯客户端全亮渲染，服务端不发光源与描边标志）。存档 NBT 按官方大小写区分：画写 `block_pos`、`facing`（2D 字节）与 `variant`；展示框写 `block_pos`、`Facing`（大写 F，3D 字节）、`Item`、`ItemRotation`、`ItemDropChance`、`Invisible` 与 `Fixed`。**对拍**：`mob-oracle --only hanging` 4 个逐刻场景（375 刻：4×4 墙与 2×1 窄墙放画并拆墙第 101 刻掉落、展示框放物品/旋转三下驱动真实隔墙比较器 1→2→3→4→0 及两步破坏、顶面荧光展示框五种音效与 `Fixed`/`Invisible` 免疫，逐刻比状态/变体/精确坐标与包围盒/比较器/掉落物/音效/网络包字节，除 12 处已知三角函数表末位差外 0 不一致）；`chunk-content java` 5 例存档双向往返全过（661 项全过）；3 处生产代码改坏确认（`CHECK_INTERVAL` 改为 99、展示框 `analog_output` 去掉 `+ 1`、展示框 NBT `Facing` 改小写 `facing`）均稳定触发对拍失败。**已知限制**：展示框内的地图现已接入 `MapItemSavedData`（支持放图 256 追踪上限检查、展示框绿标登记与移除，以及按 `ServerEntity.tickCount % 10 == 0` 发 `ClientboundMapItemDataPacket`）；`/summon` 暂沿用现有设计仅支持已建模生物与闪电，悬挂实体通过对应物品右键放置或从存档读入。
- **试炼刷怪笼**：试炼密室的 `minecraft:trial_spawner` 方块实体逐刻跑六态状态机（非激活 → 等玩家 → 激活 → 等弹奖励 → 弹奖励 → 冷却），按附近玩家人数决定刷怪总数和同时存在数，找玩家（整块坐标距离 + `getVisualShape` 视线）、带 `trial_spawner` 装备表刷怪、弹出战利品表奖励、不祥（`BAD_OMEN` → `TRIAL_OMEN`）切换、丢弃已记录生物和 `spawner_blocks_work` 门控都已接上；存档键（`normal_config` / `ominous_config` / `registered_players` / `current_mobs` / `cooldown_ends_at` / `next_mob_spawns_at` / `total_mobs_spawned` / `spawn_data` / `ejecting_loot_table`）经 `chunk-content java` 逐键逐值对拍，内联 `normal_config` 也有覆盖。刷怪做了 `noCollision`、视线和 `Mob.checkSpawnObstruction`；`checkSpawnRules` 对 `TRIAL_SPAWNER` 恒真（`isSpawner` 短路）、`custom_spawn_rules` 试炼密室不带，都跳过。普通刷怪笼合进主线后，`SpawnData` / `SpawnReason::Spawner` / 阻挡检查统一成一份，普通刷怪笼 `mob-oracle --only spawner` 的 0 差异保持不变。**对拍**：`mob-oracle --only trial_spawner --show 2000` 1 个逐刻场景（321 刻，逐刻比状态机、`registered_players` / `current_mobs`、冷却计时、刷出的怪、弹出的奖励、3011/3012/3013/3014 世界事件和世界随机源，0 场景差异）；4 处改坏确认 `trial_spawner_far_player` / `wrong_entity` / `long_cooldown` / `no_kill` 都报差异。**不祥物品生成器与风弹**：不祥试炼刷怪笼在 ACTIVE 里按 `ticks_between_item_spawners`（缺省 160 刻）调 `TrialSpawnerStateData.getDispensingItems`（`lowResolutionPosition(level, pos)` 做种子的战利品表求值 + `level.random` 加权抽一件），从 `detectedPlayers` 与 `currentMobs` 里按 `nextBoolean()` 二选一、随机挑一个实体，按 `bbHeight + 2 + nextInt(4)` 抬到上方、VISUAL 视线回退一格且那一格碰撞形状为空，生成 `OminousItemSpawner` 并放 `TRIAL_SPAWNER_SPAWN_ITEM_BEGIN` 音（音高取两次 `level.random.nextFloat()`）。生成器自己用 `level.random.nextIntBetweenInclusive(60, 120)` 定到点刻数，`DATA_ITEM`（ITEM_STACK，index 8）同步，存档写 `item` / `spawn_item_after_ticks`；到点前 36 刻放预警音，到点掉一件 `ItemEntity` 或朝 DOWN 按各自 `DispenseConfig` 射出投射物（风弹 1051、火焰弹 1018 覆盖事件），发 3021 号世界事件和 `ENTITY_PLACE` 游戏事件后消失。`WindCharge` / `BreezeWindCharge` 共用一个投射物结构，飞行照 `AbstractHurtingProjectile.tick`（惯性 1、加速度 0、`rotateTowardsMovement(0.2)`），命中走统一的 `explosion.rs`：`ExplosionInteraction.TRIGGER` 的 `SimpleExplosionDamageCalculator` 不伤实体、不掉落不破坏方块，但按标签 `blocks_wind_charge_explosions` 给基岩 / 屏障 3600000.0 抗性、开门 / 按钮 / 拉杆 / 活板门，并按曝光度 × `1.22f`（旋风人风弹 1.0）击退生物和玩家（发 `ClientboundExplodePacket`）。玩家右键风弹按 `WindChargeItem.use` 在眼睛高度生成、力度 1.5 / 散布 1 射出，放 `WIND_CHARGE_THROW` 音、记统计、非创造扣一个并挂 `use_cooldown`；`minecraft:wind_charge` 伤害类型、存档 `Owner` / `LeftOwner` / `HasBeenShot` / `acceleration_power` 和跟踪同步都接上。**对拍**：`mob-oracle --only ominous_item_spawner --show 2000` 2 个逐刻场景（普通物品 / 空堆，81 刻，逐刻比 `tickCount`、`DATA_ITEM`、3021 事件、掉出物品的位置和世界随机源，0 场景差异；`--break-self-test ominous_item_spawner` 把到点刻数 +1 会报差异）；`mob-oracle --only wind_charge --show 2000` 2 个逐刻场景（打方块 / 打牛，13 刻，逐刻比位置、速度、朝向、消失、牛的血量、世界随机源和最终方块，0 场景差异；`--break-self-test wind_charge` 改位置一位会报差异）；存档往返单测覆盖两种风弹的 `Rotation` / `Owner` / `acceleration_power`。**已知限制**：风弹打牛场景没有逐刻比牛的 `delta.x`（官方假世界 `ServerExplosion.getSeenPercent` 的 clip 与 Bamboo 共用 `seen_percent_of` 的 clip 上下文不同，击退量差 0.4；不伤血、世界随机源和方块都逐位一致，击退由单测覆盖）；`SmallFireball`（火焰弹）和 `ThrownExperienceBottle` 实体还没建模，不祥生成器抽到它们时按普通掉落物处理；`canTriggerBlocks` 对 `BreezeWindCharge` 的 `mob_griefing` 门控未接；装备表的 `equipItemIfPossible` 只做了空槽直接套槽 + `setGuaranteedDrop`（「已有装备时掉出 / 换主手 / 盔甲武器比较」分支未做，试炼怪都是新生空槽走不到）；`transformBadOmenIntoTrialOmen` 延迟到 `Effect` 落地（官方当场改，同刻两个刷怪笼扫同一带不祥之兆的玩家时第二个笼看到的路径不同，最终效果相同，这一「同刻双笼」场景未逐刻对拍）；`getOrCreateDisplayEntity` 的 INACTIVE→WAITING_FOR_PLAYERS 显示实体没建模，逐刻对拍从 `waiting_for_players` 起步。
- **宝库（`minecraft:vault`）**：活的方块实体逐刻跑四态状态机（INACTIVE → ACTIVE → UNLOCKING → EJECTING，由 `VaultBlockEntity.Server.tick` 的 `stateUpdatingResumesAt` 门控），用试炼钥匙右键开启、按 `chests/trial_chambers/reward` 战利品表弹奖励、每个玩家只能领一次（`rewarded_players` 存档）、`VaultSharedData` 显示物品每 20 刻轮换、状态转移发 3015/3016/3017 世界事件和开合 / 插入 / 弹出 / 拒绝声音。世界副作用分层：`Level` 钩子（玩家探测 / VAULT 战利品表 / 展示物品随机 / 消耗钥匙）由 game 层落地，其余走 `World` 转发；存档 `config` / `shared_data` / `server_data` 与官方 codec 逐值一致（`chunk-content java` 覆盖）。**对拍**：`mob-oracle --only vault --show 2000` 1 个逐刻场景（241 刻，两侧各自独立算战利品表，逐刻比方块状态、`ServerData` / `SharedData`、世界事件、声音、弹出的奖励物品和世界随机源，0 场景差异）；4 处改坏确认 `vault_far_player` / `no_unlock` / `wrong_key` / `clock_skew` 都报差异。**已知限制**：玩家探测恰好等于范围平方的边界上，官方 `BlockPos.closerThan` 用严格 `<`、Bamboo 钩子用 `<=`（对拍场景玩家同格，不触发边界）；逐刻比较的声音只取 soundId / 音量 / 音高，不比 source 与坐标；`override_loot_table_to_display` 的非默认表、创造模式不扣钥匙的边界没有单独对拍。
- **攻击目标（本期接入）**：僵尸 / 溺尸 / 掠夺者的村民目标（`AbstractVillager`）、僵尸 / 溺尸 / 蜘蛛 / 史莱姆 / 掠夺者的铁傀儡目标、溺尸的美西螈目标、僵尸 / 溺尸 / 狐狸 / 豹猫 / 猫的陆地幼年海龟目标（`Turtle.BABY_ON_LAND_SELECTOR`）和狐狸的成群鱼目标（`AbstractSchoolingFish`：鳕鱼 / 鲑鱼 / 热带鱼，不含河豚）都接上了 `nearest_mob_target` 搜索；`mustSee` 按每条官方目标单独给值（村民 / 狐狸猎物为假，其余为真），命中后的近战、弩、箭伤害 / 击退 / 死亡复用已有链路。`mob-oracle --only attack_target` 15 个逐刻场景比目标选择、位移、受害者血量和死亡，0 不一致：野狼找陆地幼年海龟（`NonTameRandomTargetGoal`）、野狼对泡水幼年海龟不设目标、蜘蛛遇 `IDLE` 犰狳会躲 / 遇缩成球（`SCARED`）犰狳不躲、北极熊找狐狸（含隔着壕沟够不着、两边照样设目标且寻路卡住）、羊驼找狼。`--break-self-test villager_target_none` / `must_see_inverted` / `avoid_inverted`（躲避条件取反）/ `reject_unreachable`（强制拒绝够不着的目标）/ `wolf_turtle_no_land_gate`（去掉 `!isInWater`）五处改坏确认都会报不一致。已知限制：官方村民目标类是 `AbstractVillager`（还含流浪商人），Bamboo 只建模村民，搜索只匹配 `MobType::Villager`；僵尸杀死村民的转化已经接上（困难必定、普通 50% 先掷攻击者的 `nextBoolean`，简单不转也不抽，见「僵尸村民」一节）；溺尸的三叉戟远程命中同样走这条路径（`DrownedTridentAttackGoal` 已能真的投掷，见「溺尸」一节）；村民大脑 / 职业 / 交易和袭击系统已建模，袭击相关的四个 `Raider` 通用目标仍只保持官方注册顺序；野狼的幼年海龟目标（`NonTameRandomTargetGoal`，复用 `Turtle.BABY_ON_LAND_SELECTOR`）和蜘蛛躲犰狳（`AvoidEntityGoal(Armadillo, 6.0F, 1.0D, 1.2D, !isScared())`，按字节码只躲 `IDLE` 状态的犰狳）也接上了。照 26.3 字节码，北极熊的狐狸目标、羊驼的狼目标构造里带的 `mustReach=true` 是不参与 `NearestAttackableTargetGoal` 选目标的**死字段**：`findTarget` 走 `TargetingConditions.test` → `LivingEntity.canAttack(LivingEntity)`（1 参），`canContinueToUse` 同样用 1 参 `Mob.canAttack(LivingEntity)`，都不经过唯一读 `mustReach`、进而调 `canReach` 的 2 参 `TargetGoal.canAttack(LivingEntity, TargetingConditions)`（后者只被 `HurtByTargetGoal` / `OwnerHurtBy` / `OwnerHurtTarget` / `Fox.DefendTrusted` / `TraderLlama.Defend` / `Vex.CopyOwnerTarget` 调用，且这些目标构造时全是 `mustReach=false`）。所以 `canReach` 从不被调用、不产生寻路调用，够不着的狐狸 / 狼照样被设为目标、寻路卡住；Bamboo 不实现 `canReach`（写成死代码没有意义），`docs/mob-entity-design.md` §4.1.1 的寻路调用点表里也不补对应行。- **进度系统（第一期）**：全服共享只读进度树（1866 个官方 26.3 定义、4045 个判据、126 个显示节点），按官方 `TreeNodePosition` 排显示坐标。玩家的判据完成情况、完成时间、监听表和可见性跟着玩家走，存到官方路径 `<世界>/players/advancements/<uuid>.json`（`DataVersion 5023`、本地时区时间串），登录读、存盘 / 退出写。进服发首包 `update_advancements`，之后发增量；`select_advancements_tab` 和客户端的 `seen_advancements` 已接。`/advancement grant|revoke <目标> only|from|through|until|everything [进度] [判据]`，经验与配方奖励、受 `minecraft:show_advancement_messages` 控制的 `chat.type.advancement.task/goal/challenge` 公告都照官方。`tick` 每刻触发、`location` 每 20 刻触发（以 26.3 字节码为准），`inventory_changed` 等 31 种接上现有事件入口、27 种因系统没做暂不触发（58 种全表见文档）。`mc-bamboo-vanilla advancement-oracle java` 用三个独立 JVM 的默认身份哈希顺序逐项对拍定义、坐标和单条包字节（126 个显示节点全部逐位一致），四处改坏确认；另有 `advancement-command-oracle`（官方假世界跑整条命令）、`advancement-trigger-oracle`（4253 条真实触发器实例判定）。设计和完整已知差异见 `docs/advancements.md`；配方书是第二期。

- `/summon minecraft:skeleton` 和骷髅刷怪蛋生成会射箭的骷髅；带 NBT 召唤时和官方一样不执行默认装备生成，无弓时走近战。骷髅复用敌对生物的难度、日晒、装备和战利品流程。普通箭可命中实体或插地，支撑方块失去碰撞后掉落；可拾取的箭按库存、模式和震颤时间判定，普通骷髅的箭不可拾取。箭的运动和插地状态会同步，已建模实体字段写入 Anvil 实体存档。火矢扫过水面会按 `WaterFluid.entityInside` 熄火并播放一次 `entity.generic.extinguish_fire`；穿透箭命中后会在同一刻沿剩余线段重新取候选和方块效果。
- **流浪者和沼骸**：`/summon minecraft:stray` / `minecraft:bogged` 和各自的刷怪蛋；复用骷髅的目标、装备生成、日晒、战利品（`entities/stray` / `entities/bogged`，含玩家击杀才掉的药水箭）和存档。流浪者的箭带 600 刻缓慢，沼骸生命 16、射击间隔 70 刻（困难 50 刻）、箭带 100 刻中毒；效果写在箭的 `potion_contents` 自定义效果里，箭的颜色随之同步。沼骸可用剪刀剪下蘑菇（`shearing/bogged`，每堆在身高处掉落，`sheared` 存盘并同步）。骷髅在细雪里连续 140 刻后开始转化（同步「发抖」标志），再过 300 刻变成流浪者：装备和掉落几率、效果、名字、标签等按 `ConversionType.SINGLE` 搬过去，播放 1048 号世界事件；`StrayConversionTime` / `FreezingTime` 存盘。骷髅和流浪者不会被冻（`canFreeze`），穿 `#freeze_immune_wearables` 的生物也不会。两者都接进了自然刷怪（流浪者要求看得见天、可在细雪里生成）。敌对生物找目标时照官方跳过无敌（创造模式）玩家。已知限制：转化不搬乘客 / 载具 / 队伍 / 睡觉位置，**拴绳按官方也不转移**（`ConversionType.SINGLE` 本身不搬这些，不是因为 Bamboo 缺模型），着火标志位不单独搬；流浪者 / 沼骸的自然刷怪规则已用 `mob-oracle --only stray_spawn`（找天正例 90 只、无天 / 和平 0 只）和 `--only bogged_spawn`（正例 48 只、和平 0 只）端到端对拍 0 不一致，但夹具的 `canSeeSky` 是注入常量，细雪上方连续跳过的那一支没单独区分；沼骸剪蘑菇不发 `SHEAR` 游戏事件，发射器剪蘑菇未接入；骷髅一支躲狼（`AvoidEntityGoal`）、拾取地上物品、副手药水箭当弹药都未做（沿用骷髅现状）。骷髅一支「看不见目标 60 刻后放弃」的计数以前每刻多加一次（约 30 刻就放弃），已照官方改回只在 `canContinueToUse` 里计数。
- **史莱姆**：`/summon minecraft:slime`（`Size` 是大小减一）和刷怪蛋。大小决定尺寸（0.52 × 大小）、生命上限（大小的平方）、移速（0.2 + 0.1 × 大小）、攻击力和经验（都等于大小）。用自己的移动控制一跳一跳地走：每跳之间等 `10 + nextInt(20)` 刻（追击时除以 3），没有目标时每 40–100 刻随机换个方向，在液体里上浮；不寻路。目标是 16 格内看得见、高度差不超过 4 格的玩家，以及铁傀儡（两者现在都能被打）。大小大于 1 的史莱姆碰到玩家就按近战范围和视线打一下（`AbstractCubeMob.playerTouch`），打中才放攻击音效。死后被移除时分裂成 `2 + nextInt(3)` 只小一号的（照 `ConversionType.SPLIT_ON_DEATH` 搬效果、名字、标签等），最小的一号掉 0–2 个粘液球（`entities/slime` 的 `type_specific/cube_mob` 谓词读大小）。自然生成照 `Slime.checkSlimeSpawnRules`：史莱姆区块（`WorldgenRandom.seedSlimeChunk`，读世界种子）里 y < 40 的 1/10，和 `#allows_surface_slime_spawns` 群系 y 在 (50, 70) 之间、按月相（满月 0.5 到新月 0）和亮度的地表生成；生成盒按 `spawnDimensionsScale` 放大 4 倍，生成时的大小 1 / 2 / 4 按 `setSpawnSize` 抽。存盘 `Size`、`wasOnGround` 和 `AgeableMob` 的三个键。同步大小（实体数据 index 18）。已知限制：落地 / 跳跃的挤压动画只在客户端，服务端只保住随机取数和音效；脚步声用脚下方块的（不看上面一格的雪层 / 地毯）；分裂只跑官方的 `convertCommon`：不搬乘客 / 载具 / 拴绳 / 队伍；水里改大小时的溅水音效和粒子未做；岩浆怪未做。

- **硫磺方块怪**：`/summon minecraft:sulfur_cube` 和硫磺方块怪刷怪蛋；`MONSTER` 类别，`sized(0.49, 0.49)` 按大小 1 / 2 缩放，生命 `4 × 大小`，`sulfur_caves` 群系刷怪表首项（权重 100，一组 2–4 只）。照 26.3 字节码把 `SulfurCubeArchetypes.bootstrap` 的 12 个原型接进来了：吞下 `#sulfur_cube_swallowable` 的方块后按物品标签重算击退抗性 / 弹性 / 摩擦 / 空气阻力修饰、浮力、爆炸数据、接触伤害、击退力度和声音；装了方块就不再跳也不再自由转头，竖直空气阻力改走 0.91。原型表里只有 explosive（TNT）会爆炸（威力 3、引信 120、不引火），只有 hot（岩浆块）会造成 1 点 `sulfur_cube_hot` 接触伤害。点燃走火焰 / 着火箭、爆炸伤害和脚下红石信号三条路径，`ExplosionInteraction.TNT` 复用苦力怕 / TNT 的射线与破坏方块的爆炸实现。右键可喂食幼崽、点燃、剪下方块（100 刻捡物冷却）、吞下物品；发射器前方一格的第一只会吞下可吞物品；`SulfurCubeContent` 组件与 BODY 装备槽互转。`MAX_FUSE`(19) / `FROM_BUCKET`(20) 两项元数据从官方 jar 导出并同步，`pickup_timer` / `from_bucket` / `fuse` / `Size` / `wasOnGround` 存档。对拍：`mob-oracle --only sulfur_cube_` 5 个逐刻场景（跳跃、吞物品、HOT、爆炸原型、120 刻引信，共 370 刻）+ 8 个 `saveWithoutId` 往返，除 12 处已知三角函数表末位差外 0 不一致；`--break-self-test sulfur_cube_fuse` / `sulfur_cube_size` / `sulfur_cube_body` 三处都报不一致；`mob-oracle --only sulfur_cube`（`natural_spawn` 的四个硫磺洞穴夹具：暗处 / 白天 / 强光 / 和平，24 例 96 刻 176 只）0 不一致；`chunk-content java` 三个新存档用例全过。**已知限制**：`SulfurCubeArchetype` 是官方可覆盖的同步注册表，Bamboo 只建模写死的 12 个缺省原型，数据包自定义原型不生效；`playerPush` 的玩家速度冲量、`knockback` 的三个角度/力度变换函数、原型 hit / push 声音的音量阈值与冷却、浮水（`travelInFluid` 的正弦上浮）、`maxUpStep = 0`、`canFreeze`、`requiresCustomPersistence` 的装桶 / 装方块分支、桶装时的 `SulfurCubeContent` / `bucket_entity_data` 复制、`SulfurCubeContent` 的 tooltip 和 `CubeMobPredicate` 都未做；`SearchForItemsGoal` / `TemptGoal` 只按官方注册顺序和标志位占位（会根据附近掉落物 / 玩家手里的物品设方向，但没有逐刻对拍）；`hurtServer` 里 `#sulfur_cube_with_block_immune_to` 的伤害免疫与默认击退没有接；死亡分裂固定 2 只幼年，未对拍；爆炸的 level random 取数在官方假世界里会因方块列表洗牌而多取数，引信场景最后 6 刻不比 level random。
- **焦骸和尸壳骆驼**：`/summon minecraft:parched` / `minecraft:camel_husk`、两种刷怪蛋、沙漠刷怪表（`parched` 权重 50、一组 4 只）都能生成焦骸；尸壳骆驼由尸壳自然生成时按官方 `Husk.finalizeSpawn` 带出。焦骸是第四种 `AbstractSkeleton`：16 血、射击间隔 70 刻（困难 50 刻）、箭带 600 刻虚弱（`potion_contents` 自定义效果，箭的颜色随之同步）、四种 `entity.parched.*` 音效、对虚弱免疫（`canBeAffected`），目标 / 装备 / 日晒 / `entities/parched`（含玩家击杀才掉的虚弱药水箭）/ 存档都复用骷髅一支。自然生成照 `SpawnPlacements` 的 `ON_GROUND` + `Monster.checkSurfaceMonstersSpawnRules`（黑暗且看得见天）。尸壳骆驼共享 `Camel` 的属性、姿势状态机、冲刺、大脑、坐骑交互和存档（`LastPoseTick`）：`MONSTER` 类别、按距离消失、`#camel_husk_food`（兔脚）、不能繁殖 / 求爱 / 当幼崽、环境 / 受伤 / 死亡 / 脚步四种 `entity.camel_husk.*` 音效，`entities/camel_husk` 掉 2–3 块腐肉。玩家能按骆驼那一套骑上去（`CamelHusk.interact` 先 `setPersistenceRequired()` 再走 `Camel.interact`），但第一位乘客是生物时 `isMobControlled()` 为真，控制权归骑手。尸壳的三人组按字节码逐位对齐：僵尸链之后第二次捡物判定 → `HuskGroupData`（每个非空组都重新包一层，所以同组第二只还会再试）→ 生成盒 `noCollision` → `nextFloat() < 0.1` 拿铁矛 → `CamelHusk(NATURAL)` 与 `Parched(NATURAL)` 依次构造并跑 `finalizeSpawn` → 尸壳 `startRiding(force=true)`、焦骸 `startRiding` 同骑一头，随机数抽取顺序与官方一致。对拍：`mob-oracle --only parched_bow_`（2 例 201 刻逐刻比箭、包和玩家虚弱）、`--only parched_burn_`（白天着火掉血 / 夜里安全，带覆盖断言）、`--only camel_husk_ride`（80 刻静止三人骑乘）、`--only husk_jockey`（21 例命中 / 玻璃顶挡住 / 同组复用，逐位比世界随机源和实体表）、`--only natural_spawn` 的四个沙漠夹具（26 只焦骸）、`chunk-content java` 的四题存档重存 + 三人组 `Passengers` 树（565 项全过）；三处改坏确认 `--break-self-test parched_arrow` / `camel_husk_ride` / `husk_jockey` 都会报不一致。**已知限制**：骆驼坐下 / 换姿势时 `Camel.getPassengerAttachmentPoint` 的动画偏移没有建模，只实现站直且不在过渡的分支，静止骑乘夹具用 `noAi` + 固定 `gameTime` 避开；尸壳骆驼构造时的朝向来自官方不带种子的实体随机源，`husk_jockey` 夹具对这一列两边都记 0；骆驼的 `DASH` / `LAST_POSE_CHANGE_TICK` 同步沿用普通骆驼的既有实现；`CamelHusk.chargeSpeedModifier = 4.0`、沙地脚步声 `entity.camel_husk.step_sand`、冲刺 / 吃食 / 鞍具 / 起坐声、拴绳 `canBeLeashed()` 以及「第一名乘客是玩家还是生物」的交互差异都没有单独对拍；焦骸沿用骷髅一支躲狼、拾取地上物品、副手药水箭当弹药等既有已知限制。
- **溺尸**：`/summon minecraft:drowned` 和溺尸刷怪蛋；属性是僵尸那一套加 `STEP_HEIGHT = 1.0`。两栖寻路照 `AmphibiousPathNavigation` / `AmphibiousNodeEvaluator`：寻路恒可重算，水里贴着阻挡方块算水边、水里多出上下两个竖直水邻格，求值器在水里的起点换成碰撞盒最小角；在液体中，导航可用忽略流体的碰撞形状射线提前推进到下一节点；代价覆盖照官方只在一次搜索期间生效（水 0、走地 6、水边 4，搜索开始临时改、结束还原，生物身上常驻的只有水 0，否则 `GoalUtils.hasMalus` 会把随机漫游的落点全拒掉）；`canFloat` 对溺尸不生效，入水跳跃仍是 0.3。身体碰水且想游时，`DrownedMoveControl` 直接朝目标加速；上一刻的眼部流体检测也在水里时，`travelInWater` 才走 0.01 的相对移动并每刻乘 0.9。游泳的共享标志（index 0 第 4 位）随 `updateSwimming` 同步。白天的目标只算暗处或水里的玩家。目标列表按官方顺序（玩家、村民、铁傀儡、美西螈、陆地幼年海龟）。自然生成照 `Drowned.checkDrownedSpawnRules`：脚下是水、海平面 5 格以下 1/40（`#more_frequent_drowned_spawns` 群系 1/15），放置类型是 `IN_WATER`。白天在岸上会找水（`DrownedGoToWaterGoal`），夜里在水下会朝海平面游（`DrownedSwimUpGoal`），夜里泡在近海面水里会找岸边（`DrownedGoToBeachGoal`，倒计时取 `nextInt(200)`）。生成时 3% 副手鹦鹉螺壳，`nextFloat() > 0.9` 时 `nextInt(16)` 小于 10 拿三叉戟、否则钓鱼竿（不拿盔甲）。僵尸眼睛在水里连续 600 刻后开始溺水转化（同步 index 18），再过 300 刻变成溺尸并跑 `handleAttributes`、播 1040 号世界事件；`DrownedConversionTime` / `InWaterTime` 存盘。溺尸在 `#can_breathe_under_water` 里，泡水不掉空气。战利品走 `entities/drowned`；战利品表对拍 64 题、0 排除、0 不一致（不含装备掉落）。`mob-oracle --only drowned_spawn` 覆盖固定两区块的深海、浅海、浅河和强光浅河，34 例 136 刻、15 次实际生成、0 不一致；溺尸的生成盒允许液体，但仍检查实体遮挡。`mob-oracle --only drowned_`（夜间平地追击、水中追击、向海平面上浮）3 例 200 刻 0 不一致，逐刻比较位置、速度、路径、游泳标志和随机源。已知限制：`DrownedTridentAttackGoal` 已接上真的怪物三叉戟投掷（`RangedAttackGoal(this, 1.0, 40, 10.0f)`：视线 / 10 格射程 / 40 刻间隔、按 `spawnProjectileUsingShoot` 的 `dy + 水平距离 * 0.2` 瞄准、怪物拾取 `DISALLOWED`、`mob-oracle --only trident_drowned` 逐刻对拍 0 不一致），拿三叉戟的溺尸在 10 格内走远程、否则走近战；僵尸鹦鹉螺坐骑已经建模并会真的生成（见「鹦鹉螺」一节），逐刻对拍覆盖了静止骑乘；尚未对拍真实海洋群系混合刷怪表、装备掉落和存档 NBT；两种水中场景是静水夹具，没有覆盖跨越水面、流动水或上岸；海平面由 `Level::sea_level()` 给（噪声世界 63、超平坦 -63）。白天找水的 10 次尝试里 Bamboo 和官方的目标搜索顺序也可能不同。
- **女巫**：`/summon minecraft:witch` 和刷怪蛋；生命 26、移速 0.25。`Witch.aiStep` 的四支选药（水下呼吸、抗火、治疗、速度）按官方顺序各抽一次 `nextFloat`，喝药时主手换成药水、`usingTime = 32`、移速 -0.25；喝完后清空主手、按物品组件挂上效果，瞬时治疗在效果刻生效；喝药标志是 index 17 的布尔元数据。每 60 刻朝目标扔一瓶喷溅药水：选药照 `Witch.performRangedAttack` 的短路顺序（袭击者按血量给治疗 / 再生并清目标；8 格外且无缓慢给缓慢；血量 ≥ 8 且无中毒给剧毒；3 格内、无虚弱且 25% 概率给虚弱；否则瞬间伤害，只有虚弱一支取 `nextFloat`），提前量按目标速度算。喷溅药水是新的投掷物实体，飞行物理和鸡蛋同一条 `ThrowableProjectile`（重力 0.05），命中后按 `ThrownSplashPotion.onHitAsPotion` 的 4 格水平 / 2 格竖直范围、`computeMargin` 与距离衰减分效果：瞬时治疗 / 伤害按 `(int)(scale * base + 0.5)` 取整并对亡灵反转，其余按 `scale * duration * durationScale + 0.5` 挂上（20 刻以内不挂），跳过有免疫标签的效果；命中实体不再打额外的 0 点 `thrown` 伤害，改用 `indirectMagic` 伤害源击退；广播 2002 / 2007 和 1053 / 1054 号世界事件。魔法、间接魔法和荆棘伤害只吃 15%，自己打自己为 0。战利品走 `entities/witch`。四种自饮药水有 `mob-oracle --only witch_drink`（3 例 268 刻 0 不一致）；投掷有 `mob-oracle --only witch_throw`（远距离缓慢 / 剧毒 / 瞬间伤害、近距离虚弱与自身溅射免疫、对牛（受伤）和僵尸（亡灵被治疗）的范围效果，3 例 553 刻逐刻比位置、血量、效果时长、等级事件和随机源，0 不一致）；普通自然生成按原版群系表和怪物登场规则（`mob-oracle --only witch_spawn`：18 例 72 刻 0 不一致）。已知限制：袭击框架已有（见「袭击」一节），`Raider` 的四个通用目标已按官方实现（穿过村庄、捡队长旗帜、庆祝、寻路到袭击），治疗袭击者目标仍只占顺序（后者照样每刻抽一次 `nextBoolean`）；未实现灭火、美西螈回水及 `onHitBlock` 的营火 / 蜡烛行为；滞留药水云、喷溅水瓶对末影人的伤害和女巫小屋的刷怪未实现；官方 `#witch_resistant_to`（`DamageTypeTagsProvider`）还含 `sonic_boom`（监守者音爆），Bamboo 的伤害类型表已经有 `sonic_boom`（监守者已建模），但女巫的抗性标签里没接这一项，这条 15% 抗性路径仍不可达（魔法 / 间接魔法 / 荆棘三项已按 0.15 实现）。

- **掠夺者**：可用 `/summon minecraft:pillager` 或掠夺者刷怪蛋生成；属性照 `Pillager.createAttributes()`（0.35 移速、24 血、5 攻击、32 跟随）。目标顺序按 `PatrollingMonster` → `Raider` → `AbstractIllager` → `Pillager` 的注册链：巡逻、四个袭击目标（占位，`canUse` 恒假、不取随机数）、`FloatGoal`、躲嘎枝（占位）、原地对峙、`RangedCrossbowAttackGoal`（1.0 移速、8 格射程）、随机漫游、看玩家 / 看生物。弩复用玩家弩的装填状态机（25 刻装填、20–39 刻后射击、`CHARGED_PROJECTILES` 的箭数、多重射击三发），箭复用箭实体。生成时主手弩，`nextFloat() < 0.25 * special` 走一次随机附魔，再 `nextInt(300) == 0` 给穿透 I；非 PATROL 原因下 6% 概率当队长，队长戴必掉的灾厄旗帜。音效、`Inventory` / `PatrolLeader` / `Patrolling` / `patrol_target` 存档、读档 `setCanPickUpLoot(true)` 都按官方。死亡按 `entities/pillager` 掉落，只有队长（`type_specific/raider` 的 `is_captain`）掉不祥之瓶。对拍：`mob-oracle --only pillager` 8 例 1653 刻 0 不一致（装填 / 射击 / 多重射击、巡逻、原地对峙），`loot-oracle --entity-table minecraft:entities/pillager` 4000 例 0 不一致（一半队长）。袭击框架与 `raids.dat` 已接上（见「袭击」一节），不祥之瓶现在能触发袭击；**已知限制**：`Wave` / `RaidId` / `CanJoinRaid` 存档和袭击里的 `Raider` 四个通用目标（穿过村庄、捡队长旗帜、庆祝、寻路到袭击）都已接上，队长捡起灾厄旗帜时戴上并登记为当波队长、死亡掉旗；队长掉瓶后的战利品路径与袭击中队长旗帜的发放尚未打通（击杀记英雄已接）；村民 / 铁傀儡目标已按官方目标顺序真正可打。
- **巡逻队（`PatrolSpawner`）**：每 `12000 + nextInt(1200)` 刻、白天、五分之一概率、离随机玩家 24–47 格处按区域难度刷一支掠夺者巡逻队：0 号是带巡逻目标的队长，其余是跟随者；落点要过 `isValidEmptySpawnBlock`、方块光 ≤ 8 和脚下可生成。位置门槛是主世界时钟 ≥ 120000 刻且群系不是蘑菇岛，周围 10 格区块都要加载。对拍：`mob-oracle --only patrol` 5 例 598000 刻、15 名成员 0 不一致，连结束时的世界随机源种子和计时器都比。已知限制：Bamboo 已有真实 POI 和 `isCloseToVillage`，巡逻队的村庄跳过判定改由 `Region` 的 POI 表提供；选玩家按 `BTreeMap` 的 ID 顺序而不是官方加入顺序（夹具只有一名玩家）；巡逻队生成时身体朝向固定为 0（官方保留构造器的无种子随机朝向），队长的 `findPatrolTarget` 也用无种子实体随机源，这两项只要求「有目标」不逐位比。
- **袭击（第一、二期）**：喝下不祥之瓶（`ominous_bottle_amplifier` 组件给 120000 刻 `BAD_OMEN`，等级等于组件值）后带着不祥之兆走进村庄（`ServerLevel.isVillage`，按真实 POI 的 `#village` 占用判定），`BadOmenMobEffect` 每刻检查并转成 600 刻的 `RAID_OMEN`、记下村庄坐标、移除 `BAD_OMEN`；`RaidOmenMobEffect` 在倒计时最后一刻按 `Raids.createOrExtendRaid` 求所有已占用村庄 POI 的平均中心，在 `VALID_RAID_RADIUS_SQR = 9216` 内复用或新建一场袭击并吸收等级（夹到 5）。`Raid` / `Raids` 的存档字段、波次表（简单 3 / 普通 5 / 困难 7 波）、奖励波、按难度与波次刷怪（`RaiderType.VALUES` 五种按官方顺序取随机数）、`findRandomSpawnPos` 的螺旋候选与高度图 / 区块 / 村庄门槛、Boss 血条（`boss_event` 的 RED / NOTCHED_10、进度、剩余数 / 胜利 / 失败名、观众按 `getRaidAt` 判定）、首波号角（`event.raid.horn`，volume 64）、48000 刻超时、村庄丢失判负、40 刻后判胜并给英雄 `HERO_OF_THE_VILLAGE`（48000 刻、等级 `raid_omen_level - 1`）都在 `blocks/entity/raid.rs`（纯数据与判定）+ `game/raid.rs`（世界交互）。每岛一份 `Raids`，拆岛时整场跟着袭击中心搬、合岛时重新分配不冲突的 id；存盘把各岛汇总进维度级的 `data/minecraft/raids.dat`（全局 `next_id` / `tick`），启动时家岛读回；`saved-data java` 用官方 `Raids.CODEC` 逐字段复核两场袭击的样本并确认坏状态名被拒绝。对拍：`mob-oracle --only raid_` 五个逐刻场景（普通 / 简单难度、奖励波等级、胜利、`doRaids` 关）用官方 `Raids.tick` 和 Bamboo 的正式 `raid::tick` 比每一刻的存档字段、袭击者名册、全局计数和刷出的实体，除已知 12 处三角函数表平台差异外 0 不一致；`--break-self-test raid_no_cooldown` / `raid_no_spawn` / `raid_no_victory` / `raid_fake_hero` 四处改坏都会报不一致。**已知限制**：官方 `/raid` 是 `DEBUG_DEV_COMMANDS || IS_RUNNING_IN_IDE` 下才注册的调试命令，正式命令树里没有，Bamboo 不实现；唤魔者已按官方波次表刷出（第 5、6、7 波和奖励波），困难第 7 波起劫掠兽的骑手也接上（第一只唤魔者、其余卫道士），恼鬼只由唤魔者召唤、幻术师不在波次表里只能 `/summon`；`waveSpawnPos` 已有但官方「位置不再实体 tick 时重找」的区块档位判定未建模；袭击者死亡时 `Raider.die` 的记英雄 / 离开名册 / 去队长登记已接上，袭击里队长捡起灾厄旗帜戴上、登记为当波队长、死亡掉旗也已打通（`ObtainRaidLeaderBannerGoal` → `Raider.pickUpItem` 整条链，逐刻对拍见 `raid_mob_banner`）；`RAID_OMEN` / `BAD_OMEN` / `TRIAL_OMEN` 的施加音效（`event.mob_effect.*`）已按 `withSoundOnAdded` 发，`raid_omen` / `trial_omen` 粒子不在服务端发；袭击者四个通用目标（穿过村庄、捡队长旗帜、庆祝、寻路到袭击）和村民一侧的敲钟（`RingBell`，1/20 之外再抽 `nextFloat()>0.95` 才敲）、躲藏（`RaidLocateHidingPlace`，袭击进行中才找屋子）、胜利游行（`CelebrateVillagersSurvivedRaid` 600 刻庆祝声 + 烟花）都已按官方字节码接上；村民这一侧的「敲钟躲屋」「胜利游行」两个逐刻场景（`villager_p4_raid_bell` / `villager_p4_raid_victory`）也已逐刻对上，烟花按官方取数构造 BURST 爆炸组件、发 `SpawnFirework` 事件逐刻对拍生成位置与组件，只不生成烟花火箭实体（Bamboo 还没有烟花火箭）；袭击者跨岛时不会把中心附近区块绑成不可分组。
- **卫道士**：可用 `/summon minecraft:vindicator` 或卫道士刷怪蛋生成，也会按官方波次表出现在袭击第 3、5、6、7 波和奖励波。属性照 `Vindicator.createAttributes()`（24 血、0.35 移速、12 跟随、5 攻击），尺寸 0.6×1.95、眼高按官方缺省的 0.85 倍身高，四种 `entity.vindicator.*` 音效、`entities/vindicator` 战利品表（玩家击杀掉 0–1 绿宝石）。生成时主手一把铁斧，`PatrollingMonster` / `Mob` 的初始化取数按官方顺序；`Johnny` 存档键与 `VindicatorJohnnyAttackGoal` 注册位、`Wave` / `CanJoinRaid` / `RaidId` 的 `Raider` 存档都按官方。目标顺序是受伤反击、玩家、村民、铁傀儡、Johnny 彩蛋；行为目标按 `Float`、躲嘎枝、`VindicatorBreakDoorGoal`、`RaiderOpenDoorGoal`、`HoldGroundAttackGoal`、`MeleeAttackGoal`、闲逛、看玩家 / 看生物的官方优先级注册。**已知限制**：破门（`VindicatorBreakDoorGoal` 的 240 刻进度与 `hasActiveRaid` 闸门）、Johnny 彩蛋的实际攻击范围、`customServerAiStep` 的袭击中开门标志和袭击中的斧头附魔（`applyRaidBuffs`）都已按官方字节码接上（`applyRaidBuffs` 按 `enchantOdds` 概率给新铁斧套锋利，波次 > 5 用 II；破门只在 NORMAL / HARD 有活跃袭击时启动）；逐刻对拍见 `raid_mob_vindicator_door`。
- **劫掠兽**：可用 `/summon minecraft:ravager` 或劫掠兽刷怪蛋生成，也会按官方波次表出现在袭击第 3、5、7 波和奖励波。属性照 `Ravager.createAttributes()`（100 血、0.3 移速、0.75 击退抗性、12 攻击、1.5 攻击击退、32 跟随、1 格台阶），尺寸 1.95×2.2，八种 `entity.ravager.*` 音效、`entities/ravager`（掉鞍）战利品表。三个同步计时字段 `AttackTick` / `StunTick` / `RoarTick` 按官方键存档并往返，`Raider` 层存档与目标顺序（受伤反击、玩家、村民、铁傀儡）也按官方；构造函数里的 `setPathfindingMalus(LEAVES, 0.0F)` 和 `getMaxHeadYRot()` 的 45 度也照官方。**已知限制**：咆哮击退（`roarTick` 到 10 时对 4 格内非劫掠兽 / 非灾厄村民的活体造成 6 点近战伤害并强击退）、撞击（`doHurtTarget` 记 `attackTick=10`、4 号事件和攻击声）、踩坏 `#leaves`（水平碰撞且 `mobGriefing` 开着时逐格破坏，一片没破且在地上时跳一下，受 `mobGriefing` 约束）和三个计时器的推进都已接上；第 5 波掠夺者骑手已接上（`Raid.spawnGroup` 给每只劫掠兽 `startRiding` 一只掠夺者，`updateControlFlags` 看 `#raiders` 骑手关掉 MOVE / JUMP / LOOK），困难第 7 波起的卫道士 / 唤魔者骑手也已接上（第一只劫掠兽带唤魔者、其余带卫道士，见 `Raid.spawnGroup` 的计数器）；`getAttackBoundingBox` 的加宽判定、`canBeLeader = false` 旗帜和庆祝音、盾牌格挡引发的 `blockedByItem` 眩晕（Bamboo 未建模盾牌格挡）仍未实现；逐刻对拍见 `raid_mob_ravager_roar`。
- **唤魔者**：可用 `/summon minecraft:evoker` 或唤魔者刷怪蛋生成，也会按官方波次表出现在袭击第 5、6、7 波和奖励波，困难第 7 波起当第一只劫掠兽的骑手。属性照 `Evoker.createAttributes()`（0.5 移速、12 跟随、24 血），尺寸 0.6×1.95、眼高按官方缺省的 0.85 倍身高，五种 `entity.evoker.*` 音效、`entities/evoker` 战利品表（玩家击杀掉不死图腾和 0–1 绿宝石）。施法状态走 `SpellcasterIllager` 的 `DATA_SPELL_CASTING_ID` 同步字节与 `SpellTicks` 存档，`isCastingSpell` 只看 `spellCastingTickCount > 0`；目标顺序按 `PatrollingMonster` → `Raider` → `AbstractIllager` → `SpellcasterIllager` → `Evoker` 的注册链：巡逻、四个袭击目标、`Float`、`EvokerCastingSpellGoal`、躲玩家 / 嘎枝、召唤恼鬼、放尖牙、wololo、随机漫游、看玩家 / 看生物。三个法术按官方字节码：尖牙（FANGS，暖机 20 折半、施法 40 刻、间隔 100，近距离 5 颗半径 1.5 的圆加 8 颗半径 2.5 的环、远距离 16 颗直线，`EvokerFangs` 按 `Warmup` / `Owner` 存档，从目标 Y 往下找 sturdy 面落地，到 `-8` 时对 `inflate(0.2, 0, 0.2)` 盒里非主人 / 非盟友的活体打 6 点魔法伤害，`lifeTicks` 到 0 移除）、召唤恼鬼（SUMMON_VEX，16 格内恼鬼数不超过 `nextInt(8)+1` 才放，施法 100、间隔 340，三只恼鬼位置 / 寿命按官方随机顺序）、wololo（WOLOLO，无目标、`mobGriefing` 开着时找 16×4×16 内的蓝羊，暖机 40、施法 60、间隔 140，施法时把目标羊染红）。`applyRaidBuffs` 为空（不取随机数）。对拍：`mob-oracle --only evoker_` 开 AI 逐刻比位置、速度、目标、施法状态、随机源、出站包和新生成的实体（近距离圆形尖牙、远距离直线尖牙、召唤三只恼鬼、wololo 把蓝羊染红），`spellcaster_save` 比三种生物的存档往返；`--break-self-test evoker_fangs_damage` / `evoker_wololo_color` / `evoker_summon_count` 三处改坏都会报不一致。**已知限制**：对拍场景里尖牙的伤害用一头牛的血量和击退来比，玩家一侧的受伤结算走游戏层，不在对拍里逐刻比。
- **恼鬼**：只由唤魔者 `EvokerSummonSpellGoal` 召唤，也可 `/summon minecraft:vex` 或恼鬼刷怪蛋生成。属性照 `Vex.createAttributes()`（14 血、4 攻击），尺寸 0.4×0.8、眼高 0.51875，四种 `entity.vex.*` 音效、`entities/vex` 战利品表。穿墙飞行（`noPhysics` + `VexMoveControl`：MOVE_TO 时按 `speed * 0.05 / len` 逼近、太近减速，无重力），有限生命 `limitedLife`（`life_ticks` 存档，到时每刻 1 点饥饿伤害）；目标顺序是 `Float`、`VexChargeAttackGoal`（7 刻抽一次、4 格外才冲，撞到就打）、`VexRandomMoveGoal`（锚点 `bound_pos` 附近随机飞）、看玩家 / 看生物；目标选择器是受伤反击、`VexCopyOwnerTargetGoal`（复制主人的目标）、最近玩家。主手铁剑、掉落率 0；`bound_pos` / `life_ticks` / `owner` 按官方键存档。对拍：`mob-oracle --only vex_` 开 AI 逐刻比冲锋（充能状态、`VexMoveControl` 的速度）和有限生命到时每刻扣血直至死亡，`evoker_summon` 场景里三只恼鬼的生成和随后的冲锋也逐刻比；`--break-self-test vex_life_damage` 改坏会报不一致。
- **幻术师**：只能 `/summon minecraft:illusioner`（不在袭击波次表里，没有刷怪蛋）。属性照 `Illusioner.createAttributes()`（0.5 移速、18 跟随、32 血），尺寸 0.6×1.95、眼高按官方缺省的 0.85 倍身高，五种 `entity.illusioner.*` 音效、`entities/illusioner` 战利品表。施法状态与唤魔者共用 `SpellcasterIllager`；目标顺序是巡逻、四个袭击目标、`Float`、`SpellcasterCastingSpellGoal`（基类）、躲嘎枝、分身（DISAPPEAR，隐身 1200 刻，暖机 20 折半、施法 20、间隔 340，只在没隐身时放；分身位置只在客户端渲染，服务端只加隐身）、致盲（BLINDNESS，中等级以上难度、目标换了才放，施法 20、间隔 180，给目标 400 刻致盲）、`RangedBowAttackGoal`（速度 0.5、固定 20 刻间隔、15 格射程，主手弓，`performRangedAttack` 复用骷髅的 `getMobArrow` 并播 `SKELETON_SHOOT`）、随机漫游、看玩家 / 看生物；目标选择器是受伤反击、玩家、村民、铁傀儡。`applyRaidBuffs` 为空。对拍：`mob-oracle --only illusioner_blind_arrow`（困难难度）逐刻比隐身加致盲玩家、弓箭的出手时机与新生成的箭的随机源；`--break-self-test illusioner_blindness` 改坏会报不一致。分身的位置和粒子只在客户端渲染，服务端不建模。
- **结构刷怪覆盖**：`natural.rs` 的自然刷怪先查位置所在结构的刷怪覆盖表，命中就用结构表替换群系表，否则回退群系表。数据在 `mc-bamboo-data::spawning::STRUCTURE_SPAWN_OVERRIDES`（由 `worldgen/structure/*.json` 的 `spawn_overrides` 生成，含空表）：前哨站 `STRUCTURE` 盒的 `MONSTER` 换成掠夺者，女巫小屋 `PIECE` 盒的 `MONSTER` / `CREATURE` 换成女巫 / 猫，海底神殿 `STRUCTURE` 盒的 `MONSTER` 换成 2–4 只守卫者并用空表挡住美西螈和发光鱿鱼；`natural-spawn java` 对官方每张覆盖表逐位比抽选、组大小和随机源指纹，`mob-oracle --only guardian_spawn` 的 18 例 72 刻逐位比神殿 `MONSTER` 表的 2–4 只一组、`checkGuardianSpawnRules`（1/20 或水下看不见天、非和平、上下两层水）、`IN_WATER` 放置和 `finalizeSpawn`。查询照 `ChunkGenerator.getMobsAt`：区块的 `STRUCTURE_REFERENCES` → 引用区块的 `STRUCTURE_STARTS` → 按起点盒（含 `terrainAdaptation` 12 格外扩）或结构件盒判定。对拍：`mob-oracle --only structure_spawn` 6 个查询 0 不一致（前哨站空档命中、盒外回退、类别不覆盖、女巫小屋件内 / 件外）。已知限制：前哨站的 jigsaw 结构件还没生成，正式世界里暂时不会出现前哨站起点（机制和表已就绪，女巫小屋已能真实验证）；结构起点按种子重算而不再读区块里的 `structures` 标签；引用遍历顺序是 `BTreeMap` 而非官方哈希序（覆盖盒不重叠，暂无影响）；`RegionLevel` 的群系表仍是既有的默认平原限制。神殿生成时按 `OceanMonumentPieces.spawnElder` 在两翼与阁楼各放一只远古守卫者，随 `FEATURES` 的写增量收进交付盒子的实体表（`PersistenceRequired`、满血 80）；`structure-place java` 的 `E` 行逐项比类型、位置、持久、满血，`finalizeSpawn(STRUCTURE)` 的随机取数全在 `WorldGenRegion.getRandom()` 上、不动结构件的随机源，所以 `gen-world` / `structure-e2e` / `chunk-content` 的方块仍 0 差异。官方实体自身的随机源在世界生成时是 `RandomSource.create()`（不带种子），UUID / 朝向 / `finalizeSpawn(STRUCTURE)` 的跟随距离加成和左撇子都从这里取，Bamboo 里按世界种子和归属区块派生可重放的实体随机源，因此这些字段**不能逐位比较**；`structure-place java` 的 `E` 行实际逐项比的是**实体类型、`snapTo` 的方块位置、`PersistenceRequired`、满血（80）**，`gen-world` / `structure-e2e` / `chunk-content` 的方块仍逐格 0 差异。
- **苦力怕**：可用 `/summon minecraft:creeper`（含 `powered` / `Fuse` / `ExplosionRadius` / `ignited` NBT）或苦力怕刷怪蛋生成，也会在黑暗处自然生成。属性、目标优先级和注册顺序照官方 `Creeper.registerGoals`：离目标不到 3 格开始膨胀（`SwellGoal`），超过 7 格或看不见就收缩，膨胀满 30 刻爆炸；摔落会提前膨胀（最多到引信前 5 刻），有目标时肯从高处跳下（`getMaxFallDistance`）；打火石和火焰弹右键点燃（打火石扣 1 点耐久，火焰弹消耗一个）。爆炸走 `ExplosionInteraction.MOB`：按官方射线算被炸的方块，按距离和曝光度伤害生物和玩家（伤害类型 `player_explosion`，死亡消息是「被苦力怕炸死」），生物按爆炸击退抗性被推开；`mobGriefing` 关着时不破坏方块，`mobExplosionDropDecay`（默认开）下方块掉落带爆炸半径衰减；带电时威力翻倍。死亡按 `entities/creeper` 战利品表掉火药，被骷髅射死时掉一张 `#creeper_drop_music_discs` 唱片，经验 5。膨胀方向、带电、点燃三项元数据同步给客户端，四个存档键和官方逐键一致。对拍：`mob-oracle --only creeper`（9 个场景 920 刻，逐刻比位置、速度、膨胀计数、目标、随机源、旁边的牛受的伤害和击退、真实玩家的血量、死亡消息和爆炸包字节，0 不一致；`--break-self-test explode_particle` / `explode_block_count` / `explode_knockback` 三处都报不一致）、`loot-oracle --entity-table minecraft:entities/creeper`（4000 例 0 不一致）、`chunk-content` 的新建苦力怕存档。已知限制：雷击会把苦力怕变成带电的，仍可用 `powered` NBT；猫和豹猫已建模，但苦力怕的两个 `AvoidEntityGoal`（躲豹猫、躲猫）仍是占位；带电苦力怕炸死僵尸、骷髅、苦力怕时不掉头颅（`killedEntity` 的 `charged_creeper` 表未接）；身上有药水效果时爆炸不留滞留药水云；爆炸不推开也不伤害经验球、箭、下落方块和点燃的 TNT（只处理掉落物、生物和玩家），多个受害者的处理顺序是掉落物、生物（实体 ID 升序）、玩家，官方按实体分区顺序；爆炸本身不发 `GameEvent`（游戏事件 / 振动系统已接进世界，见「幽匿与监守者」一节）；攻击挥手动画（`swingForAttack`）不发。
- **蜘蛛和洞穴蜘蛛**：可用 `/summon minecraft:spider` / `minecraft:cave_spider` 或对应刷怪蛋生成，也会在黑暗处自然生成。属性、尺寸（含眼高）和 `Spider.registerGoals` 的注册顺序照官方：躲犰狳的目标占注册位（犰狳还没有）；`LeapAtTargetGoal` 在离目标 2–4 格且站在地上时按 1/3 跳扑；`SpiderAttackGoal` 是 `MeleeAttackGoal(mob, 1.0, true)` 加「亮处每刻 1/100 放弃目标」；目标只有 `SpiderTargetGoal`，亮处不启动，白天因此中立。导航是 `WallClimberNavigation`：`Spider.tick` 把这一刻的 `horizontalCollision` 写进 `DATA_FLAGS_ID` 第 0 位，`onClimbable()` 读这个标志，贴墙时每刻上升 0.2；路径走完后朝目标方块的三维坐标推。蛛网不减速蜘蛛，中毒效果对蜘蛛无效。困难模式自然生成时按 `getSpecialMultiplier` 的概率带一个随机效果（速度 / 力量 / 回复 / 隐身，同一组共享），无限时长。洞穴蜘蛛生命 12、碰撞箱 0.7×0.5，命中时普通难度给 7 秒、困难给 15 秒中毒（中毒由目标自己 tick，和官方一样 25 >> 等级 刻一跳）。死亡按 `entities/spider` / `entities/cave_spider` 战利品表掉落，经验 5；攀爬位同步给客户端。对拍：`mob-oracle --only spider_` / `--only cave_spider_`（夜里追玩家、被打后翻过一整面石墙、白天中立、洞穴蜘蛛追击五个场景 840 刻逐刻比位置、攀爬位、状态、目标和随机源，0 不一致；`MeleeAttackGoal.tick` 的三参数 `moveTo` 在爬墙时算不出路径会返回假、重算间隔加 15，和官方一样不走 `WallClimberNavigation` 两参数版本的「没路也成功」；移速和官方一样按 float `0.3F` 存，白天 0.8 倍速散步时才看得出这一位）。骷髅骑士按 1% 生成并骑在蜘蛛上（见「生物骑乘」）；已知限制：洞穴蜘蛛打玩家后的中毒没有进对拍（`mob-oracle` 的假玩家不吃效果），只有单测覆盖；`AvoidEntityGoal` 的犰狳一项占位；不吃蛛网（`makeStuckInBlock` 的蜘蛛分支）没有对拍；踩脚下蛛网/甜浆果的减速是在 `entityInside` 里做的，和官方一致但没有逐条对拍。
- **末影人**：可用 `/summon minecraft:enderman`（含 `carriedBlockState` / `anger_end_time` / `angry_at` NBT）或末影人刷怪蛋生成，也会在黑暗处自然生成。属性（40 生命、7 攻击、0.3 移速、64 跟随、1.0 格阶高）、尺寸（0.6×2.9，眼高 2.55）和 `Enderman.registerGoals` 的注册顺序照官方：`FloatGoal`、`EndermanFreezeWhenLookedAt`、`MeleeAttackGoal(1.0, false)`、`WaterAvoidingRandomStrollGoal(1.0, 0.0)`、`LookAtPlayerGoal(8)`、`RandomLookAroundGoal`、`EndermanLeaveBlockGoal(10)`、`EndermanTakeBlockGoal(11)`；目标选择器是 `EndermanLookForPlayerGoal(1)`、`HurtByTargetGoal(2)`、`NearestAttackableTargetGoal<Endermite>(3)`（末影螨已建模且这个目标能真的找到它，`nextInt(5)` 那一笔照取）、`ResetUniversalAngerTargetGoal(4)`（占位，全局怒气规则默认关）。注视判定照 `LivingEntity.isLookingAtMe(player, 0.025, true, false, getEyeY())`：视线夹角、128 格上限、方块遮挡（`COLLIDER`），戴 `#gaze_disguise_equipment`（雕刻南瓜）不算；被盯 3 刻（`adjustedTickDelay(5)`）后锁定，锁定时每刻抬头看玩家并冻结移动，距离 4 格内随机传送；目标跑出 16 格后每 15 刻朝玩家方向传送一次（8×16 的偏移）并重试。怒气：有目标时每刻重抽 400–780 刻的计时（`PERSISTENT_ANGER_TIME`），目标变创造 / 旁观 / 和平、目标死掉（生物）或计时到期且没目标时消气；`angry_at` / `anger_end_time` 存盘。传送照 `LivingEntity.randomTeleport`：世界边界夹取、从落点向下找 `#entities_can_teleport_to`、`noCollision` + 不含液体 + 盒子内没有 `#enderman_does_not_teleport_to`、下方一格不是水；成功时发实体事件 46 和一声 `entity.enderman.teleport`。白天（`isBrightOutside`、亮度 > 0.5、看得见天、距上一次目标变化 600 刻以上）按 `nextFloat() * 30 < (亮度 - 0.4) * 2` 随机传送并清目标；怕水：每刻在水里或雨里受 1 点溺水伤害，之后 9/10 概率传送躲开；所有 `#is_projectile` 的伤害（箭、羊驼口水）不掉血，改成连续 64 次尝试传送。搬方块：`mobGriefing` 开着时，空手每 10 刻判定一次 `EndermanTakeBlockGoal`（半径 2、高 3，`#enderman_holdable` 且射线能到就捡起默认状态）；搬着方块时每 2000 刻判定一次 `EndermanLeaveBlockGoal`（半径 1、高 2，目标格是空气、下方不是基岩且是完整碰撞方块、`canSurvive`、格子里没有别的实体），放下后清空；搬着方块的末影人不自然消失（`requiresCustomPersistence`）。死亡按 `entities/enderman` 战利品表掉末影珍珠（0–1），再按 `dropCustomDeathLoot` 用一把带精准采集的假钻石斧把搬着的方块按方块表掉出来，经验 5。搬着的方块（可选方块状态序列化器）、`creepy`、`stared_at` 三项元数据同步给客户端，`carriedBlockState` / `anger_end_time` / `angry_at` 存档和官方逐键一致。对拍：`mob-oracle --only enderman_`（注视、注视锥边缘、不注视、白天露天传送、夜里淋雨、箭射、捡方块、带方块放置 9 个场景 61416 刻（白天场景跑 1200 刻，跨过 `targetChangeTime + 600` 的门槛，白天传送的判定和取数真的被比到；改坏 `nextFloat() * 30` 为 20 时第 685 刻起报不一致），逐刻比位置、速度、血量、目标、搬着的方块、`creepy` / `stared_at` 和随机源，0 不一致）、`chunk-content` 的新建末影人存档、`natural-spawn`。已知限制：末影螨、末影龙和末地维度都没有；`readPersistentAngerSaveData` 的「读档时立刻把 `angry_at` 设成目标」没有做（读档时手上没有实体表），读档后要到下一次被激怒才会恢复目标；被投掷的喷溅水瓶（`hurtWithCleanWater`）没接，喷溅药水实体还没有；传送只发一声（官方在旧位置广播一声、新位置再 `playSound` 一声），也不发 `GameEvent.TELEPORT`；`isPassenger()` 相关的判断没有（没有乘客）；死亡时搬着的方块掉落用的是 `BlockPos.containing(死亡位置)` 的中心当 `ORIGIN`（官方用精确的实体坐标），而且这条没有和官方逐题对拍；白天传送的 `hasChunkAt` 用 `is_loaded` 近似；末影人走路声只有脚下方块的 `SoundType`（和苦力怕一样，没有 `playCombinationStepSounds`）；随机传送后的那一两刻头部 / 身体朝向可能和官方不同（长场景里白天传送时能看到：我们的头被 `LookControl.clampHeadRotationToBody` 夹到身体 ±75 度，官方停在原值；位置、目标、随机源一致），没有收敛，方块场景因此把天色固定在傍晚来避开；`EndermanLookForPlayerGoal` 在目标玩家下线 / 跨岛时读作目标消失，官方手里的实体引用会继续跟；刷怪笼已做（见「刷怪笼」一节）。
- **铁傀儡和雪傀儡**：可用 `/summon minecraft:iron_golem` / `minecraft:snow_golem`、各自刷怪蛋和雕刻南瓜图案生成（铁块 T 形或两格雪块加南瓜；方块图案、清方块、2001 破坏事件、邻居更新和 `setPlayerCreated(true)` 都照官方，方块对拍有专门场景）。两种傀儡都不在群系刷怪表里，不自然生成。铁傀儡 100 血 / 0.25 移速 / 击退抗性 1.0 / 攻击 15 / 阶高 1.0、1.4×2.7，目标链照 `IronGolem.registerGoals`（村庄守卫、被打记仇、愤怒玩家、`Enemy` 且非苦力怕、`ResetUniversalAngerTargetGoal` 占位）；近战把目标抛起 `0.4 * (1 - 击退抗性)`，伤害 `floor(15)/2 + nextInt(15)`；血量四档裂纹（`Crackiness.GOLEM` 的 0.75/0.5/0.25，跨档多播 `iron_golem.damage`），铁锭回 25 血，没有自然回血；`PlayerCreated` 和持久愤怒存盘。雪傀儡 4 血 / 0.2 移速、0.7×1.9，每 20 刻朝 `Enemy` 目标扔雪球（力度 1.6、散布 12），`mobGriefing` 开着时脚下四角铺一层雪；热群系（`SNOW_GOLEM_MELTS` 环境属性）、水里和雨里每刻受 1 点溺水伤害；剪刀剪下南瓜（`shearing/snow_golem` 掉一个雕刻南瓜），`Pumpkin` 存盘。雪球走通用 `ThrowableItemProjectile` 实体管道（和鸡蛋共用飞行 / 同步 / 存档，只换命中效果）：命中烈焰人一类打 3 点、其余 0 点，广播事件 3 后消失；玩家右键雪球也按 `SnowballItem.use` 生成。死亡掉落走 `entities/iron_golem` / `entities/snow_golem` 表。对拍：`mob-oracle --only golem` 12 个逐刻场景（闲逛、追击、修复、玩家造的、死亡掉落、铺雪 / 关 `mobGriefing`、剪南瓜、雨 / 水、热群系、扔雪球）、8 题近战伤害抽样和 4 题存档读回，另有 `golem_flags` / `golem_snow` / `golem_repair` / `golem_shear` 四种改坏确认；`oracle java --weather clear` 的雕刻南瓜图案题 0 失败。已知限制：村民召唤不做；村庄 / POI 已接入，但 `MoveTowardsTargetGoal`、`MoveBackToVillageGoal`、`GolemRandomStrollInVillageGoal`、`OfferFlowerGoal` 和 `DefendVillageTargetGoal` 都按夹具里没有村庄 / 村民时的官方路径实现（含 `DefaultRandomPos.getPosTowards` 的 10 次抽点、村庄目标退回 `LandRandomPos.getPos`、白天 `nextInt(8000)` 和 `IronGolem.canSpawnSprintParticle` 的 `nextInt(5)` 覆写），逐刻比实体随机源、坐标、猎物血量全部一致；铜傀儡见下一节；玩家扔雪球的冷却未建模；铺雪的 `snow_layer_can_survive` 只读生成碰撞形状表，不走带方块实体的动态形状（移动活塞 / 潜影盒等场景可能有差）。
- **铜傀儡**：可用 `/summon minecraft:copper_golem`、刷怪蛋，或在铜块（四种氧化等级、打蜡与否）上放雕刻南瓜生成；图案按官方换成铜箱子，傀儡的初始氧化等级取铜块（打蜡铜块按去蜡对应块取），播 `entity.copper_golem.spawn`。属性 12 血 / 0.2 移速 / 1.0 阶高，尺寸 0.49×0.98、眼高 0.735，持久化、能开门、要求路径长度 48。大脑走 `CopperGolemAi` 的核心 / 闲逛活动表（恐慌、看目标、走向目标、`InteractWithDoor` 声明式一次性行为、两种倒计时刻、搬运、偶尔看玩家、`RandomStroll.stroll(1.0, 2, 2)` / 发呆）。`DATA_WEATHER_STATE`(index 16，序列化器 38) 与 `COPPER_GOLEM_STATE`(index 17，序列化器 37) 两项元数据按 VarInt 同步，存档写 `next_weather_age`(Long，缺省 -1) 和 `weather_state`(字符串)。受伤 / 死亡 / 脚步 / 转头音按四档氧化换（`unaffected` 与 `exposed` 同组，`weathered` / `oxidized` 各一组，脚步声音量 1.0）。氧化计时照 `updateWeathering`：`-1` 时排 `504000–552000` 刻，到点升一级，到 `oxidized` 把 `next_weather_age` 置 0，此后每刻在脚下是空气且 `nextFloat() <= 0.0058` 时变成氧化雕像（随机姿态、按朝向、掉携带物、播 `entity.copper_golem_become_statue`、移除实体）。右键：空手扔回携带物、剪刀剪天线、蜜脾打蜡（`next_weather_age=-2`，不再氧化）、斧头去蜡或把非未氧化的刮退一级（3003/3004/3005 粒子事件都发）、雷击也退一级，受伤把交互动画状态设回 `IDLE`。**搬运行为**照 `TransportItemsBetweenContainers` 全量移植：空手找最近的铜箱子（源）、拿着物品找最近的普通 / 陷阱箱子（目标），只接受在 32×8×32 搜索盒内、没被访问 / 标记为不可达、上方没有红石导体也没有坐下姿势的猫（`ChestBlock.isChestBlockedAt`）的 `ChestBlockEntity`；行走 / 3 格内按箱子打开者集合（`ContainerOpenersCounter.openCount`）排队 / 抵达后 60 刻交互（第 1 刻开箱设状态、第 9 刻播对应音效、第 60 刻关箱并取放物品），一次最多取 16 个、依官方 `addItemsToContainer` 逐槽合并或填空（空箱子也接受，不匹配同种物品的箱子走 `PLACE_NO_ITEM` 空动画并记进 visited），成功后清 `VISITED_BLOCK_POSITIONS` / `UNREACHABLE_TRANSPORT_BLOCK_POSITIONS`（`GlobalPos` 集合、6000 刻过期、进存档），找不到目标 / 集合超 10 / 50 时进 140 刻 `transport_items_cooldown_ticks` 冷却。箱子打开者人数由 `Command::CopperGolemChestOpen` 在落地阶段增减（精确驱动读当前计数、并行驱动读本刻开始快照），真正改箱子内容的是 `Command::CopperGolemPickUp` / `CopperGolemPutDown`，在落地阶段串行读当前世界（两只傀儡同刻碰同一个箱子时按 uid 顺序先到先得，单测覆盖）；主手装备由行为本体直接写在 `Cx.mob` 上（精确驱动 tick 期间 `Entity.kind` 被借走换成占位，命令落地阶段才拿回完整实体，所以那条路径只改箱子）；只读查询照蜜蜂找巢的先例：精确驱动读当前状态，并行驱动读本刻开始快照，**同一刻两只铜傀儡碰同一个箱子时两种驱动可能不同**。铜傀儡雕像方块右键循环四种姿势（standing→sitting→running→star），随机刻继续氧化；未氧化雕像被斧头点中变回铜傀儡（在方块中心生成、朝向取 `FACING`、播生成声、移除方块），雕像方块实体的自定义名按 `components:"minecraft:custom_name"` 存档往返。对拍：`mob-oracle --only copper_golem` 6 个逐刻场景（搭建生成、取物送物、找不到目标箱子、氧化一级、打蜡不氧化、变成雕像），逐刻比方块 / 容器 27 格槽位 / 两个箱子的打开者计数、氧化等级与计时、交互动画状态、主手物品、大脑摘要、世界随机源结束状态和新生成实体；`--break-self-test cg_transport` / `cg_brain` / `cg_weather` / `cg_build` 四处改坏都会报不一致。`chunk-content java` 覆盖雕像方块实体的裸 / 带自定义名两题，存档往返单测覆盖 `next_weather_age` / `weather_state` 和两处搬运位置集合（含 `ttl`）。**已知限制**：`InteractWithDoor` 只占 CORE 的 `OneShot` 位，实际开关门由寻路 / 方块行为负责；铜傀儡的转头闲置动画和 5 个 `AnimationState` 只在客户端算，服务端只保留状态元数据；铜傀儡 loot table `minecraft:entities/copper_golem` 沿用路径表，未单独对拍掉落；对拍夹具逐刻把官方 `level.random` 起点注入 Bamboo 的世界随机源，再逐位比本刻结束状态（与 `bee_ai` 夹具同构），`no_target` / `transport` 场景跑到第 62 刻（取物完成、进入放下交互），140 刻冷却由单测覆盖；雕像场景里假世界的 `setBlockAndUpdate` 不会自动建 `CopperGolemStatueBlockEntity`，夹具预置方块实体后用反射直接调官方私有 `turnToStatue`，**跳过了 `nextFloat() <= 0.0058` 的掷签**（比较的仍是官方真实转换产物：pose STAR / facing SOUTH / 实体消失）；Bamboo 侧掷签的位置和取数次数另有两个单测（`statue_roll_uses_one_level_random_float` / `statue_roll_miss_leaves_the_golem`）覆盖。搭建场景跑到生成出铜傀儡的第一刻，这一刻新生成的实体按官方 `EntityTickList` 要到下一刻才 tick，两侧同序。
- **旋风人**：`MobType::ALL` 扩到 70 种（含本分支的旋风人和主线合进来的流浪商人 / 行商羊驼），试炼刷怪笼的 `spawn_potentials` 里本来就有 `minecraft:breeze`，接上后能刷出来，也能用 `/summon`、刷怪蛋和 `SpawnData` 生成。属性照 26.3 字节码：30 血 / 0.63 移速 / 24 跟随距离 / 3 攻击，尺寸 0.6×1.77（眼高 1.385）、10 点经验、`entities/breeze` 掉落；`BREEZE_IDLE_GROUND` / `BREEZE_IDLE_AIR` 环境声按 `onGround()` 分派，受伤 / 死亡声各一种；构造时把 `ON_TOP_OF_TRAPDOOR` / `FIRE` 寻路代价设成 -1。`BreezeAi` 的大脑表照官方：`NEAREST_LIVING_ENTITIES` / `HURT_BY` / `NEAREST_PLAYERS` / `BREEZE_ATTACK_ENTITY_SENSOR` 四个传感器，CORE 的 `Swim(0.8)` + `LookAtTargetSink(45,90)`，IDLE 的两个 `StartAttacking`、`SlideToTargetSink(20,40)` 和 `RunOne(DoNothing(20,100) 权重 1 / RandomStroll(0.6) 权重 2)`，FIGHT 五个行为（`StopAttackingIfTargetInvalid` / `Shoot` / `LongJump` / `ShootWhenStuck` / `Slide`，条件 `ATTACK_TARGET` 有值 / `WALK_TARGET` 空），默认活动 FIGHT、`updateActivity(FIGHT, IDLE)`。`BreezeAttackEntitySensor` 从附近活体里挑非创造 / 非旁观、可攻击的玩家或铁傀儡；`Shoot` 充能 15 刻后朝目标（脚底 + `getBbHeight() * 0.3`，`lookAt(EYES, ...)` 立刻转向）在 `getFiringYPosition()` 射一枚 `BreezeWindCharge`（力度 0.7、散布 `5 - 4*难度`），放 `BREEZE_SHOOT` 音，恢复 4 刻、冷却 10 刻；`ShootWhenStuck` 在水里 / 载具上 / 有漂浮时申请射击；`LongJump` 照 `canRun`（`randomPointBehindTarget` 在目标背后 4–8 格取点 → `snapToSurface` 解落点 → 危险方块 / 视线检查）吸气 10 刻后按 `LongJumpUtil.calculateJumpVectorForAngle` 洗牌 40/55/60/75/80 五个角度、`jumpPower = 0.058333334 * FOLLOW_RANGE`、`allowUnsafe=false` 解初速度起跳，落地回站立、`discardFriction` 收放、冷却 2（被打）/10 刻、补 `BREEZE_SHOOT` 100 刻；`Slide` 着地时按 `nextBoolean` 在 `randomPointBehindTarget` / `randomPointInMiddleCircle` 里二选一设 `WalkTarget`，`SlideToTargetSink`（`MoveToTargetSink(20,40)` 子类）进滑行姿态、放 `BREEZE_SLIDE` 音，收尾回站立并给 `BREEZE_SHOOT` 60 刻；`Slide` 内圈命中时先试 `DefaultRandomPos.getPosAway(5,5)`，内圈外才按 `nextBoolean` 在背后点 / 中圈点二选一。箭 / 三叉戟 / 雪球 / 鸡蛋 / 末影珍珠 / 喷溅药水射中旋风人被弹反（`Breeze.deflection` 按 `REVERSE` + `BREEZE_DEFLECT` 音弹开，音效位置在目标实体上），`mob-oracle --only arrow_deflect_breeze` 的逐刻场景 0 差异。**对拍**：`mob-oracle --only breeze` 三个逐刻场景（`breeze_shoot` / `breeze_jump` / `breeze_slide`，逐刻比姿态、大脑记忆、位置 / 速度 / 朝向、mob 随机源和世界随机源），除这台 ARM Mac 的 `Mth.COS_TAB` / `ASIN_TAB` 平台末位差（全局 12 处）和 `lookAt` 的 `Mth.atan2` 末位差（`breeze_shoot` 第 39 刻起 yRot 差 1–2 ULP，射击方向用目标位置、不级联）外 0 不一致；`--break-self-test breeze_pose` / `breeze_brain` / `breeze_motion` / `breeze_margin` 四处改坏都报不一致。**已知限制**：客户端动画状态和旋风粒子是客户端的、不在服务端。- **蠹虫和末影螨**（分支 `silverfish`）：两者都可用 `/summon minecraft:silverfish` / `minecraft:endermite` 或对应刷怪蛋生成，都**不在群系刷怪表里**。属性 8 血 / 0.25 速，攻击力 1（蠹虫）/ 2（末影螨）；尺寸 0.4×0.3、眼高由生成器导出。`Silverfish.registerGoals` 的注册顺序和 `WakeUpFriendsGoal` / `MergeWithStoneGoal` 照官方字节码：受伤（有来源实体或 `#always_triggers_silverfish` 伤害）后倒数 10 刻，扫 11×21×21 格，`mobGriefing` 开着就破坏虫蚀方块并再唤醒一只蠹虫，关着则换成宿主方块；`MergeWithStoneGoal` 是 `RandomStrollGoal(1.0, 10)` 的子类，偶尔把相邻宿主方块变成虫蚀方块并消失；脚下是宿主方块时寻路权重 10。末影螨的 `Lifetime` 存盘，2400 刻后消失，`PersistenceRequired` 为真时不老化，末影人的 `NearestAttackableTargetGoal<Endermite>` 现在能真的找到末影螨。末影螨的经验是 3（覆盖 `Monster` 的 5），两种生物的战利品表都是空的。虫蚀方块（7 种，含 `infested_deepslate`）被生存挖掘且工具不带精准采集，或被爆炸炸掉时，按 `InfestedBlock.spawnAfterBreak` → `spawnInfestation` 生成一只蠹虫（`TRIGGERED` 原因、实体事件 20）；方块对拍的空手 / 精准采集挖掘题和固定爆炸题都覆盖了这两条路径。`mob-oracle` 的 7 个逐刻场景（唤醒、钻石头、近战、寿命、持久化、末影人目标）加存档往返和死亡掉落场景 0 不一致，`--break-self-test small_mob_life` / `small_mob_hurt` 两处改坏都能抓到。已知限制：细雪攀爬目标只占注册位；末影螨客户端的 `PORTAL` 粒子不在服务端，Bamboo 不发粒子包；要塞的蠹虫刷怪笼已建模（见「刷怪笼」一节）；`mobGriefing` 关掉时蠹虫只把虫蚀方块换回宿主方块，不额外生成同伴。
- **刷怪笼（`SpawnerBlockEntity` / `BaseSpawner`）**：`minecraft:spawner` 现在是活的方块实体，地牢（骷髅 / 僵尸 / 蜘蛛）、废弃矿井的洞穴蜘蛛和要塞传送门房间的蠹虫都会真的刷怪。`serverTick` 照官方字节码：`isNearPlayer`（`RequiredPlayerRange`，旁观者不算）、`Delay` 倒数（`MinSpawnDelay` / `MaxSpawnDelay` 重抽）、`SpawnCount` 次取点（`SpawnRange`、y 取 `nextInt(3) - 1`）、`noCollision`、`SpawnPlacements.checkSpawnRules`（SPAWNER 原因跳过脚下方块检查、怪物仍要黑暗）或 `CustomSpawnRules` 的方块光 / 天空光闭区间、`MaxNearbyEntities` 的范围精确类计数、实体标签只有 `id` 时跑 `finalizeSpawn`、`spawnAnim`（实体事件 20）、2004 号世界事件；随机数来源和抽取顺序与官方一致。`delay()` 从 `SpawnPotentials` 加权抽下一个 `SpawnData`，方块事件 1 照官方发出。刷怪蛋右键刷怪笼改它的实体（`SpawnEggItem.useOn` → `BaseSpawner.setEntityId`），`spawner_blocks_work` 游戏规则关掉时不刷也不改。存档 `Delay` / 六个计时字段、storeNullable 的 `SpawnData` 和 `SpawnPotentials` 与官方逐键一致，`resave` 走同一条 `read → saveAdditional` 路径。对拍：`mob-oracle --only spawner` 9 个逐刻场景（僵尸 / 骷髅 / 蜘蛛 / 洞穴蜘蛛 / 蠹虫、玩家在范围外、`MaxNearbyEntities` 封顶、光照规则挡住、刷怪蛋改实体）逐刻比倒计时、当前 `SpawnData`、刷出的实体类型和位置、世界随机源，0 不一致；`--break-self-test spawner_delay` / `spawner_entity` / `spawner_lit` 三处改坏都能抓到；`chunk-content java` 覆盖刷怪笼存档。刷出实体时照官方发 `ENTITY_PLACE` 游戏事件，附近的幽匿感测体能收到。已知限制：只有 `MobType::ALL` 里建模的实体类型能刷出来，未建模的 `id` 按「读不进实体」放弃这一轮；`spin` / `oSpin` 是客户端动画，服务端不保留。
- **末影珍珠**（分支 `ender-pearl`）：`ThrownEnderpearl` 接在通用投掷物上（`ThrowableKind::EnderPearl`）：飞行每刻先重力后阻力，命中时先按珍珠自己的随机源取 32 组 `PORTAL` 粒子数（服务端 `addParticle` 不发包，取数照发生），再给打中的实体 0 点 `thrown` 伤害；主人缺失、已死或正在睡觉时珍珠直接消失；否则用命中前的 `oldPosition()` 作为落点把主人传过去、清下落距离、造成 5 点 `minecraft:ender_pearl` 伤害（`#is_fall` / `#bypasses_armor` / `#no_knockback`，死亡消息走 `fall` 变体）并播 `entity.player.teleport`。落地时 `nextFloat() < 0.05`、`doMobSpawning` 与刷怪规则都开且难度不是和平时，按主人的位置和朝向刷一只末影螨。`EnderpearlItem.use` 用 NEUTRAL 音源播投掷声、以 1.5 力度生成珍珠、记 `ITEM_USED`、扣一个并挂 1 秒（20 刻）的 `use_cooldown`；冷却中再右键直接 `PASS`，不发声也不扣物品。玩家身上的飞行珍珠存在 `ender_pearls`（每项整份实体 NBT 加 `ender_pearl_dimension`，`player-data` 双向对拍），上线时放回世界，下线时先存后按 `UNLOADED_WITH_PLAYER` 移除，`ender_pearls_vanish_on_death` 为真时主人一死就丢弃。每枚飞行中的珍珠按 `ticketTimer` 的节奏给自己所在区块续半径 2、超时 40 刻的 `ENDER_PEARL` 票，远处的珍珠区块因此保持加载、继续 tick（已加载模式的集成测试覆盖「珍珠飞进玩家视距外仍落地传送」）。对拍：`mob-oracle --only pearl` 7 个场景（落地用 `oldPosition`、命中牛、强制刷螨种子、和平难度、玩家扔出 / 移动中扔 / 创造模式扔）逐刻比珍珠位置和随机源、主人位置 / 血量 / 受伤动画、末影螨、传送声和物品栏，只剩 ARM Mac 的 `Mth` 表差异；改坏落点和刷螨都能抓到。已知限制：没有 `connection.isAcceptingMessages()` 的对应物，正在断线的玩家在 Bamboo 里查不到，等价于主人缺失；珍珠票在珍珠消失时立刻撤掉，官方是等 40 刻超时，飞行期间的效果相同；传送门冷却没有建模（Bamboo 没有传送门）；服务端不发粒子包；自动存档用每刻刷新的快照而不是存档那一刻的实体状态。

- **猪**：可用 `/summon` 或猪刷怪蛋生成；支持食物、寻路、繁殖（幼崽随机继承一方花纹）、掉落、花纹和声音变种、上鞍（鞍走 `SetEquipment` 同步，死亡必掉，发射器可上鞍、剪刀可剪鞍）及存档。上了鞍的成年猪被空手（非食物）右键时官方让玩家骑上去并返回 SUCCESS，Bamboo 一样：潜行下车落点照 `Animal.getDismountLocationForPassenger`（先试朝向的八个相邻格，全不行落在猪的包围盒顶）、猪死乘客下车都走通用骑乘。只有第一位乘客且任一只手拿着胡萝卜钓竿的玩家是控制者（`getControllingPassenger`）；`tickRidden` 服务端照骑手视线转向（`setRot`，俯仰乘 0.5 并把 `yRotO` / `yBodyRot` / `yHeadRot` 对齐）、`getRiddenSpeed` 照官方 `MOVEMENT_SPEED * 0.225 * boostFactor`，位置由客户端 `move_vehicle` 给、服务端只维护朝向和加速状态。右键胡萝卜钓竿走 `ItemBasedSteering.boost`（`DATA_BOOST_TIME` 同步 `nextInt(841) + 140`）、每次扣 7 点耐久、用坏按 `transmuteCopyIgnoreEmpty` 变回普通钓竿（`DAMAGE` 清 0），失败只记 `ITEM_USED`。对拍：`mob-oracle --only pig_ride` 覆盖右键上猪（上鞍 / 未上鞍 / 幼年 × 空手 / 钓竿 / 食物 / 鞍 / 木棍 / 剪刀 × 潜行与否）、加速（耐久、同步时长、逐刻速度与朝向）、下车落点，`player-data` 覆盖骑猪下线再上线的 `RootVehicle` 往返，均 0 不一致；`pig_saddle` / `pig_rod_tempt` 回归 0 差异。已知限制：带自定义组件的上鞍已接入（物品堆整份存进鞍槽），但刷怪蛋的 `entity_data` 组件未接入；雷击转化成僵尸猪灵未建模（乘客处理见「闪电」一节的限制）；喂掉手里最后一件食物时官方那一格留着数量 0、物品还在的堆（`ItemStack.is` 仍为真），猪的 `TemptGoal` 继续被诱惑，Bamboo 把格子清空，诱惑目标停下并进入 50 刻冷却（牛、羊、鸡同理）。
- **哞菇**：可用 `/summon` 或哞菇刷怪蛋生成，蘑菇岛（`mooshrooms_spawnable_on` + 亮度）自然刷出；属性、目标、叫声同牛（叫声固定 `classic`），脚下是菌丝时游荡权重 10。碗接蘑菇煲，棕色哞菇喂花（`SuspiciousEffectHolder` 表由生成器导出）后接迷之炖菜（带 `suspicious_stew_effects` 组件），桶挤奶，小麦繁殖 / 催熟；繁殖时父母同色先抽 1/1024 变异再二选一。剪刀把它换成一头牛（公共模块 `conversion.rs`，官方 `Mob.convertTo` 的 `SINGLE`：抄位置、朝向、速度、年龄、效果、名字、标签等，血量不抄），按 `shearing/mooshroom` 表掉 5 个对应颜色的蘑菇并扣剪刀耐久。`Type` / `stew_effects` 存盘，`DATA_TYPE` 同步。对拍：`mob-oracle --only mooshroom` 7 例 0 不一致，`chunk-content` 含新生和棕色带效果两例。已知限制：喂花的 `SMOKE` / `EFFECT` 粒子和剪毛的 `EXPLOSION` 粒子不发；剪毛声发成按坐标的音效包（官方是跟随实体的 `sound_entity` 包，羊同此）；雷击换色（`thunder_hit`）已接上闪电实体；`SHEAR` 游戏事件不发；发射器剪哞菇未接入；转换出的牛照官方 `convertCommon` 抄着火显示标志（`setSharedFlagOnFire`）。
- **熊猫**：可用 `/summon` 或熊猫刷怪蛋生成，丛林按 `checkAnimalSpawnRules` 自然刷出。两份基因按官方显性 / 隐性规则决定七种外观之一（`Brown` / `Weak` 要两份相同才显现），`MainGene` / `HiddenGene` 存盘，`UNHAPPY_COUNTER` / `SNEEZE_COUNTER` / `EAT_COUNTER` / `MAIN_GENE_ID` / `HIDDEN_GENE_ID` / `DATA_ID_FLAGS` 同步。属性 20 血 / 跟随 16 / 速度 0.15 / 攻击 6，虚弱基因 10 血、懒惰基因速度 0.07（只在 `finalizeSpawn` / 繁殖后重设，和官方一样读档不重设）。右键竹子：成年求爱、幼年加速长大、求爱冷却中或成年时坐下吃（吃掉后手里那份消失，中途换竹子会丢下旧的）；脚下有 `panda_eats_from_ground` 的物品会捡进主手。六种移动 / 闲逛目标：panic（坐下就停）、繁殖（附近 8 格找不到竹子就不乐意 32 刻并在 30 秒内不再试）、攻击（做不了动作时不启动）、诱惑、躲避玩家 / 怪物（只有担心基因）、坐下、仰躺（懒惰）、打喷嚏（幼年；虚弱先抽 1/500）、看玩家、打滚（幼年或顽皮）、跟随父母、避水闲逛。打喷嚏按 `gameplay/panda_sneeze` 掉礼物并让 10 格内成年熊猫跳一下；好斗熊猫被同伴叫来（`HurtByTargetGoal.setAlertOthers`，只叫好斗的）；吃到竹子或咬过一口就停手。繁殖在 `getBreedOffspring` 里按官方顺序抽父母各一次 `nextBoolean` 和幼崽两次 1/32 变异。对拍：`mob-oracle --only panda` 6 个逐刻场景（闲逛、三种基因、吃竹子、打喷嚏、繁殖、死亡掉落）+ 512 题基因遗传，共 3278 刻 0 不一致；`--only natural_spawn_panda` 对拍丛林亮度 15 / 8 的产率、双基因与世界随机源，`--only gen_spawn_panda` 对拍竹林完整动物表的生成位置、双基因与随机源指纹。已知限制：吃东西的 `ITEM` 粒子和喷嚏的 `SNEEZE` 粒子按官方服务端 `addParticle` 不发包（取数照官方），带物品参数的 `ItemParticleOption` 编码能力已具备（狼铠开裂粒子在用）；`PandaLookAtPlayerGoal` 的 `lookAt` 只在目标被清掉时重找，跨刻保持的细节和官方一致但未单独对拍；雷雨坐下依赖 `BlockView::is_thundering`，只有真实世界和对拍注入过；喷嚏让同伴跳的水中判定没有建模（只看 `onGround`）；幼崽拿金蒲公英走 `Animal.mobInteract` 的兜底未接；熊猫的 `canBeLeashed` 为假（不能拴）。
- **北极熊**：可用 `/summon` 或北极熊刷怪蛋生成，冰冻海洋和冰冻深海这两个「替代方块」群系按 `checkPolarBearSpawnRules`（亮度 + 脚下 `polar_bears_spawnable_on_alternate`）刷，其它群系走 `checkAnimalSpawnRules`；属性 30 血 / 跟随 20 / 速度 0.25 / 攻击 6，`getWaterSlowDown` 0.98。`PolarBearMeleeAttackGoal`（不要求看得见目标）在够不着但在「目标宽度 + 3」格内时站起来吼（40 刻冷却）再扑咬；幼崽挨打时叫来成年北极熊后自己停手（`alertOther` 只认成年），成年熊带崽时主动打 10 格内的玩家（搜索距离减半）；持久愤怒（`neutral.rs`，`anger_end_time` / `angry_at` 存盘）让被打的熊记住 20–39 秒。成年熊只在环境伤害下恐慌，幼崽和别的动物一样；幼年用 `BABY_DIMENSIONS`，组生成按 `AgeableMobGroupData(1.0)` 从第二只起都是幼崽；幼崽叫声是 `ambient_baby`；`DATA_STANDING_ID` 同步。对拍：`mob-oracle --only polar_bear` 11 例 1121 刻 0 不一致（闲逛带崽、柱子上的玩家、死亡掉落含着火、环境恐慌、挨打反击、带崽打玩家、打幼崽叫帮手、和平难度、成年 / 幼崽选狐）。已知限制：成年熊可选狐狸为目标，幼崽不会；狐狸反向躲熊的目标还未接；`universalAnger` 游戏规则没建模（恒为假，`ResetUniversalAngerTargetGoal` 只占位）；官方玩家死亡时 `NeutralMob.playerDied` 会按 `forgive_dead_players` 消气，Bamboo 没有接（对拍的假世界没有玩家死亡，`polar_bear_fight_cub` 场景因此止于玩家 2 血）。
- **豹猫**：可用 `/summon minecraft:ocelot` 或豹猫刷怪蛋生成，在丛林与竹林群系按官方 `MONSTER` 刷怪表自然刷出（`checkOcelotSpawnRules` 2/3 概率，`checkSpawnObstruction` 要求海平面以上、脚下为草方块或 `#leaves` 且无液体）。属性 10 血 / 0.3 移速 / 3 攻击，成年 0.6×0.7、幼崽 0.3×0.35（眼高 0.34375）；`DATA_TRUSTING`（index 18）同步、`Trusting` 存盘。每刻按移动速度修饰符切换潜行姿势（0.6 速 `Pose::Crouching`）和疾跑（1.33 速 `setSprinting` 挂瞬态 `+0.3F` 移速加成、`Attribute.sync_version` 逐刻触发同步与 `spawnSprintParticle` 两次 `nextDouble` 取数）。未信任时远离 16 格内玩家；手持 `#ocelot_food` 诱惑（`OcelotTemptGoal` 会受惊），3 格内喂食按 1/3 概率信任（事件 41/40 与 7 组高斯粒子取数）；繁殖与 100% 组幼年（`AgeableMobGroupData(1.0F)`）；捕杀鸡与陆地幼年海龟；未信任且存活超 2400 刻可远离消失。已知限制：苦力怕躲避豹猫的 `AvoidEntityGoal` 仍是占位未接；信任成功 / 失败只发实体事件 41/40（客户端自播粒子），服务端不另发粒子包但保留官方随机取数；拴绳已建模（`canBeLeashed` 按官方覆盖）。
- **猫**：可用 `/summon minecraft:cat` 或猫刷怪蛋生成。复用 `tame.rs` 可驯服框架（驯服、跟随、坐下、传送主人、`TamableAnimalPanicGoal`），支持 11 种花色（`minecraft:cat_variant`）、2 种叫声变体（`minecraft:cat_sound_variant`：成年 `classic` / `royal` 与幼猫 `entity.baby_cat.*` 共 9 组音效）与项圈颜色（`CollarColor`，默认红 14）。`finalizeSpawn` 跑 5% 幼猫、按月相亮度（满月 `>= 0.9` 允许 `all_black`）选花色并随机挑叫声变体。未驯服喂 `#cat_food` 1/3 概率驯服并立刻坐下、标持久化并播吃食声；已驯服主人可染项圈（`#cat_collar_dyes`）、喂鱼回血（1×营养或 1）、求爱繁殖（父母花色二选一、项圈混色，和官方一样不重抽 `sound_variant`）或切换坐站。实现 `CatTemptGoal`（300/250 刻切换目标玩家、未驯服每 100 刻乞食声）、`CatLieOnBlockGoal`（床，同步 `IS_LYING`、每 5 刻呼噜声）、`CatSitOnBlockGoal`（箱子、点燃的熔炉、床尾）以及未驯服远离玩家、捕杀兔子与陆地幼年海龟、存活超 2400 刻可远离消失。5 项元数据（`CAT_VARIANT` / `CAT_IS_LYING` / `CAT_RELAX_STATE_ONE` / `CAT_COLLAR` / `CAT_SOUND_VARIANT`）走官方注册表序列化器同步，`variant` / `sound_variant` / `CollarColor` / `Owner` / `Sitting` 与官方逐键存档一致。已知限制：村庄 `CatSpawner` 与沼泽小屋结构刷猫未实现（村庄猫按任务暂不做）；玩家睡觉未建模，故 `CatRelaxOnOwnerGoal`（睡在主人身上与清晨礼物 `gameplay/cat_morning_gift`）未接；`CatSitOnBlockGoal` 的箱子打开人数（`ChestBlockEntity.getOpenCount`）暂未暴露在 `BlockView` 上，箱子按关闭处理；苦力怕躲避猫的 `AvoidEntityGoal` 仍是占位未接；拴绳已建模（`canBeLeashed` 按官方覆盖）。
- **村民（期 2 骨架）**：可用 `/summon minecraft:villager` 或村民刷怪蛋生成，**不在任何群系刷怪表里**。属性照 `Villager.createAttributes`（20 血 / 0.5 移速 / 16 跟随），成年 0.6×1.95 眼高 1.62、幼年 0.49×0.98 眼高 0.63；能开门、能浮水、寻路要求 48 格并能捡东西。`finalizeSpawn` 在未 finalized 时按群系选品种（`VillagerType.byBiome`，没有随机数），繁殖原因会把职业清成 `none`。三项元数据（`DATA_UNHAPPY_COUNTER` 18 / `DATA_VILLAGER_DATA` 19 / `DATA_VILLAGER_DATA_FINALIZED` 20）按官方序列化器同步，`removeWhenFarAway` 恒假，死亡走空的 `entities/villager` 战利品表且不掉经验。存档写官方 8 个键（`VillagerData` / `VillagerDataFinalized` / `FoodLevel` / `Gossips` / `Xp` / `LastRestock` / `LastGossipDecay` / `RestocksToday`），`Offers` / `Inventory` 原样保留、`Brain` 复用通用存档。`mc-bamboo-data` 由生成器导出 68 个 `trade_set`、391 个 `villager_trade`、73 个交易标签、15 个职业、7 个品种、29 条群系映射、5 个八卦类型和日程关键帧。对拍：`mob-oracle --only villager` 13 例（10 个群系生成、1 题存档往返、2 题死亡），只剩这台 ARM Mac 的 `Mth.COS_TAB` / `ASIN_TAB` 平台差异；`--break-self-test villager_spawn` / `villager_save` 两处改坏都能抓到。已知限制：大脑、日程、职业和交易已经接上（见下两条），**八卦语义还没有**（在期 6）；构造期按官方九个传感器的扫描间隔消费随机数，`finalizeSpawn` 的跟随距离加成 / 左撇子已经和官方一致（期 3 接真传感器时替换这段临时取数）；僵尸村民已经建模（见下一节），流浪商人和行商羊驼见下面各自一节。

- **村民大脑、日程与职业（期 3–4）**：村民按官方 `VillagerGoalPackages` 挂 11 个行为包、9 个传感器和日程（`villager_schedule` 关键帧，幼年村民用自己的日程），`customServerAiStep` 跑 `Brain.tick`。期 4 接上真实 POI：`AcquirePoi` 取最近 5 个候选、一次多目标 A\*（`createPath(Set, int)`），失败的候选建 `JitteredLinearRetry`；`GoToPotentialJobSite` / `AssignProfessionFromJobSite` 认领工作站、换职业清 `Offers` 并重建大脑，`ValidateNearbyPoi` / `ResetProfession` / `YieldJobSite` / `PoiCompetitorScan` 处理工作站被拆或被抢。工作有 `WorkAtPoi`（300 刻冷却、1.73 格判定、工作音效）、`WorkAtComposter`、农民的 `HarvestFarmland` / `UseBonemeal` 和三种 POI 闲逛。睡觉照 `LivingEntity.startSleeping` / `stopSleeping`：床 `OCCUPIED`、`sleeping_pos` 元数据和存档、站起点按 `findStandUpPosition`；敲钟写 `HEARD_BELL_TIME`，`ReactToBell` 切到躲藏、`LocateHidingPlace` 找屋子。8 格 `Inventory` 往返，`GoToWantedItem` 按官方 `EntityTracker` 持有物品实体、捡起想要的物品；死亡、记忆过期、转化成僵尸村民时归还 POI。交易接进大脑后，交易玩家身份存在村民实体上，CORE 的 `LookAndFollowTradingPlayerSink` 让交易中的村民停下看着玩家；`WorkAtPoi` 现在真的调 `shouldRestock` / `restock`（补货逻辑搬到 blocks 的 `Villager`），升级后的 200 刻再生效果也走村民实体的状态转移（见下一条）。对拍：`mob-oracle --only villager` 的大脑场景（日程、幼年日程、恐慌、闲逛、农民行为表）和 7 个期 4 真实 POI 逐刻场景（认领、重试、两只抢一个工作站、拆工作站失业、睡觉起床、敲钟躲藏、农民收割播种捡种子）全部 0 帧差异，只剩已知的三角函数表平台差异；`--break-self-test villager_p4_no_take` / `no_unemploy` / `no_sleep` / `no_bell` / `no_harvest` 五处改坏都会报不一致。被困村民降频、睡觉村民降频两个开关缺省关闭，数字见 `docs/performance-optimizations.md` 第 29 节。**已知限制**：`WorkAtPoi` 的补货、交易时的大脑行为和交易后升级都已接进大脑（期 5 交易见下一条）；`TradeWithVillager` 村民之间互抛食物和小麦依赖八卦和繁殖，留到期 6；袭击框架已接上（见「袭击」一节），`SetRaidStatus` 现在按 1/20 取数与 `getRaidAt` 切 `PRE_RAID` / `RAID` 活动，`ReactToBell` 在有袭击时抑制 `HIDE`；`HIDE` 活动表照挂，敲钟（`RingBell`）/ 躲屋（`RaidLocateHidingPlace`）/ 胜利游行（`CelebrateVillagersSurvivedRaid`）等袭击触发行为已按官方字节码接上；流浪商人走自己的刷新器和交易集，不进村民大脑（见下面两节）。
- **村民交易（期 5）**：把 26.3 数据包里的交易接进真实玩法。`ItemCost` / `MerchantOffer` / `MerchantOffers` 照官方 `CODEC` / `STREAM_CODEC` 逐键逐位实现存档和网络编码，村民的 `Offers` 从原样保存改成真解析、存档往返逐字节一致。交易生成照 `VillagerTrade.getOffer` / `AbstractVillager.addOffersFromTradeSet`：`TradeCost` 按 `ContextIntProvider` 求值、`merchant_predicate`（`villager/variant` 品种子谓词）、`given_item_modifier`（附魔等）、`double_trade_price_enchantments` 附加价格翻倍都接上，`random_sequence` 走 `Region.loot_sequences`，`allow_duplicates=false` 时按条目去重。空手右键有职业的成年村民打开交易界面：发 `OpenScreen(menu::MERCHANT)` + `ClientboundMerchantOffersPacket`（`canRestock` 恒真）、三格（付款两格 + 结果格）、`ServerboundSelectTradePacket` 的 `setSelectionHint`、`slotsChanged` 重算结果、`quickMoveStack` 结果格搬进背包并完成交易；取结果格按 `notifyTrade`（`increaseUses` + 经验 + 升级追加交易 + 给 200 刻再生 + 经验球）走完一次真实交易，`shouldIncreaseLevel` 按 `NEXT_LEVEL_XP_THRESHOLDS`（10/70/150/250）升级，`demand` 按 `2*uses - maxUses` 折算、`getModifiedCostCount` 按 `count + floor(count*demand*priceMultiplier) + specialPriceDiff` 夹到堆叠上限算价格，`stillValid` 要求村民活着且 4 格交互距离内。进度 `minecraft:villager_trade`（`TradeTrigger` 的物品 / 村民谓词）接上，`TALKED_TO_VILLAGER` / `TRADED_WITH_VILLAGER` 两项自定义统计也接进 `item_use_stats`。交易状态（`tradingPlayer`）现在存在村民实体上，菜单只负责调用：`startTrading` 记玩家、关菜单 / 走远 / 死亡 / 交易表空时按 `stopTrading`，`stillValid` 还校验 `getTradingPlayer() == player`；CORE 的 `LookAndFollowTradingPlayerSink` 让交易中的村民停下看着玩家，关闭后擦掉 `WALK_TARGET` / `LOOK_TARGET` 恢复。补货 `shouldRestock` / `restock` / `catchUpDemand` / `updateDemand` 搬到 blocks 的 `Villager`，`WorkAtPoi.useWorkstation` 按官方调用、game 层不再留第二份；升级的状态转移（`rewardTradeXp` / `increaseMerchantCareer` 追加新等级交易、200 刻 0 级再生、经验球 `+5`、`ambientSoundTime` 压制）也收在 blocks，撒经验球和生成新等级交易表仍由 game 层负责。对拍：`trade-oracle java` 1824 例（13 职业 × 5 级 × 7 品种 × 4 种子，加流浪商人 4 个世界种子）逐项比 `buy/buyB/sell/uses/maxUses/rewardExp/specialPrice/demand/priceMultiplier/xp`，0 不一致；`menu java` 覆盖商人菜单的选交易（`tryMoveItems` 自动填付款格）、放物品、取结果和结果格 Shift 点击（`doClick` 的 `QUICK_MOVE` 循环连做多笔，付款够 1 次 / 3 次 / 背包快满三种题），`Replay`/`do_click` 跑的是和真实游戏同一套结果格逻辑；单测覆盖 `cost_count` / `update_demand` / 补货重置 / 自动填付款 / 结果格完成交易 / Shift 批量交易 / 幼年村民摇头 / 睡觉村民不交易，e2e 单测从空手右键到拿到交易结果走完一整条链路；`mob-oracle --only villager_trade` 3 个逐刻场景（打开交易村民停下看着玩家 / 关闭恢复、工作站补货与需求变化、交易后升级的倒数 / 再生 / 新交易）逐刻比大脑、位移、随机源、经验 / 等级、再生时长、血量和交易表摘要，0 帧差异，`--break-self-test vt_no_follow` / `vt_no_close` / `vt_no_restock` / `vt_no_levelup` / `vt_no_orb_rng` 五处改坏都会报不一致。**已知限制**：`updateSpecialPrices` 的声望折扣（期 6 八卦）仍按 0 算；英雄效果折扣在打开交易界面时按玩家 `HERO_OF_THE_VILLAGE` 等级接线，补货 / 升级时 blocks 侧只做 `resetSpecialPrices`（声望为 0 时等价），带英雄效果时补货后不重算折扣，留到期 6 一起接；补货与升级已接进大脑（见上），交易经验球用村民自己的随机源（官方 `getRandom()`），`NONE` 职业村民的 `CONSUME`、幼年摇头、睡觉不交易都已做。
- **流浪商人**：`/summon minecraft:wandering_trader`、刷怪蛋和 `WanderingTraderSpawner` 三种来源都接上了，`MobType::ALL` 扩到 65 种。实体沿用 `AbstractVillager` 的属性（20 血 / 0.5 移速 / 16 跟随）、成年 0.6×1.95、`DATA_UNHAPPY_COUNTER` 元数据、`Offers` / `Inventory` 存档、四项环境 / 受伤 / 死亡音效（交易中的叫声换成 `entity.wandering_trader.trade`）；`removeWhenFarAway` 恒假、`canBeLeashed` 恒假、`mobInteract` 在手持村民刷怪蛋 / 幼年 / 交易中落到通用交互，成年空手右键主手记 `TALKED_TO_VILLAGER`、按 buying → uncommon → common 三张交易集懒生成 9 笔交易后打开**和村民共用的** `MerchantMenu`（不是第二份实现）：标题 `entity.minecraft.wandering_trader`、等级 1 / 经验 0、`showProgressBar` / `canRestock` 均为假、不跑声望折扣；取结果格 `increaseUses`、压环境音、按 `rewardTradeXp` 撒 `3 + nextInt(4)` 经验球但不升级、不加村民经验，进度 `villager_trade` 的 `type` 是 `minecraft:wandering_trader`。夜里按 `isDarkOutside() && !isInvisible()` 开始喝隐身药水、白天按 `isBrightOutside() && isInvisible()` 喝牛奶；`updatingUsingItem` 的 32 刻里每 4 刻按 `Consumable.shouldEmitParticlesAndSounds` 用实体随机源取 4 个数发 `OverrideConsumeSound` 音效，喝完走 `PotionContents.forEachEffect` 挂真正的 `INVISIBILITY`（3600 刻）/ 牛奶 `ClearAllStatusEffectsConsumeEffect` 清全部效果，`isInvisible()` 共享标志位由 `updateInvisibilityStatus` 按 `effectsDirty` 刷新；`despawnDelay` 每刻 -1、交易中暂停，到 0 `discard()`；`DespawnDelay` / `wander_target`（可空 `BlockPos.CODEC`）按官方存档，读档时 `Age = max(0, Age)`。刷新器照 `WanderingTraderSpawner.tick`：每 1200 刻把 `spawnDelay` 减 1200，减到 0 重置 24000、把 `spawnChance` 从 25 按 +25 夹到 75，再按**加之前**的概率判 `nextInt(100)`；命中后 `nextInt(10)` 门、玩家附近 48 格内优先 `meeting` POI（村庄钟），10 次 `nextInt(2R)` 取落点、`ON_GROUND` + `MOTION_BLOCKING_NO_LEAVES`、2×3×2 空碰撞、`#without_wandering_trader_spawns` 群系、`SPAWN_WANDERING_TRADERS` 规则；生成时设 `despawnDelay=48000` / `wanderTarget` / `home=(target,16)`，两只行商羊驼在 4 格内落点并拴到商人。存档 `data/minecraft/wandering_trader.dat` 只有 `spawn_delay` / `spawn_chance` 两个 `optionalFieldOf` 字段，全服一份经 `GlobalPort` 提交、`SaveSession` 写盘；`tickDelay` 和 `LegacyRandomSource`（官方 `RandomSource.create()`）的内部种子是岛上的内存状态。对拍：`trade-oracle java` 加 4 题流浪商人交易生成（四个世界种子，三张集逐项比 `MerchantOffers.CODEC`），共 **1824 例 0 不一致**；`mob-oracle --only wandering_trader` 的 8 题刷新器逐刻场景（四个种子 × 规则开 / 关、24000 刻每 1200 刻比一次存档两字段）加商人与行商羊驼各一题官方 `saveWithoutId` 往返，除 12 处已知三角函数表末位差外 0 不一致，`--break-self-test wt_spawner_delay` / `wt_spawner_chance` / `wt_save` / `wt_save_target` 四处改坏都报不一致；`saved-data java` 用官方 `WanderingTraderData.CODEC` 复核 `wandering_trader.dat`；`cargo test -p mc-bamboo-game` 有从空手右键到取走结果的端到端单测。**逐刻对拍（第二轮）**：流浪商人现在带齐官方 `registerGoals` 的整张目标表（`FloatGoal`、两条 `UseItemGoal`、`TradeWithPlayerGoal`、对僵尸 / 唤魔者 / 卫道士 / 恼鬼 / 掠夺者 / 幻术师 / 僵尸疣猪兽的七条 `AvoidEntityGoal`、`PanicGoal`、`LookAtTradingPlayerGoal`、`WanderToPositionGoal`、`MoveTowardsRestrictionGoal`、`WaterAvoidingRandomStrollGoal`、`InteractGoal`、`LookAtPlayerGoal`）和 `Mob.createMobAttributes()` 的基础属性，目标选择器走和其它生物相同的 `serverAiStep` 隔刻相位（`(tickCount + id) % 2 == 0` 才整张表 tick，其余刻只跑 `tickRunningGoals`），不再每刻自己判喝水；`updateInvisibilityStatus` 挪到 `Entity.updateDataBeforeSync` 的同步阶段。`mob-oracle --only wandering_trader` 加了六个开着 AI 的逐刻场景（夜里喝隐身药水、白天喝牛奶、`WanderToPositionGoal` 走向目标点且两只行商羊驼被拴着跟走、僵尸靠近时躲开、`despawnDelay` 到时商人和羊驼一起消失、玩家打开交易界面并完成一笔交易），逐刻比位置 / 目标 / 状态效果 / 实体随机源 / 实体是否存在，除 12 处已知三角函数表末位差外 0 不一致；`--break-self-test wt_despawn`（倒计时多扣一次）/ `wt_invisibility`（不刷新隐身标志）/ `wt_trade`（交易不累加 `uses`）三处在生产代码路径里改坏，都报不一致。**已知限制**：七条 `AvoidEntityGoal` 里只有躲僵尸疣猪兽那一条没有对应实体可触发（下界生物不在范围内），其余目标物种都已建模；逐刻场景只覆盖躲僵尸；交易界面的标题、进度条、补货开关与 `menu java` 的村民菜单共用同一套包体但对流浪商人没有专属对拍题；`getRandomPlayer()` 已照官方用 `level.random.nextInt(players.size())` 抽一名玩家（单人 `nextInt(1)` 也照取），但并列的 `PoiManager.find` 遍历序依赖 POI 段内哈希顺序，多钟场景可能与官方不同；`hasEnoughSpace` 只用静态碰撞形状表，移动活塞 / 潜影盒等动态形状按空处理；刷新出的实体在刷新器场景里只比存档计时，刷出位置 / 拴绳 / `despawnDelay` 由 Rust 单测覆盖，没有与官方假世界逐题比生成序列。
- **行商羊驼**：`/summon minecraft:trader_llama`、刷怪蛋和刷新器都能生成，`MobType::TraderLlama` 进 `is_equine()` 分组，继承羊驼的属性 / 尺寸 / 花色 / 力量 / 箱子 / 地毯 / 存档键、脚步声、`followLeashSpeed=2` 的拴绳跟随和 `entities/trader_llama` 战利品表；刷新器生成的两只强制成年（`Age=0`）并拴在流浪商人身上，`TraderLlama.doPlayerRide` 的「牵引者是流浪商人就不让骑」按通用交互里的 `can_be_leashed` / 乘客路径覆盖。`despawnDelay` 缺省 47999、存 `DespawnDelay`；每刻 `maybeDespawn` 在没被驯服、没拴到商人之外的东西、没被玩家骑、没锁龄、没要求持久时递减，到 0 解绳并 `discard()`；交易中的商人暂停自己的倒计时，羊驼照官方仍按自己的时钟走。目标表照 `TraderLlama.registerGoals`：先走完整羊驼表再追加 `PanicGoal(2.0)`（优先 1）；目标选择器追加 `TraderLlamaDefendWanderingTraderGoal`（优先 1）和针对僵尸 / 灾厄村民的两条 `NearestAttackableTargetGoal`（优先 2）。`MobType::is_llama()` 把行商羊驼和羊驼同样处理，所以和官方一样不进马的吃草取数分支（此前行商羊驼每刻的实体随机数取数次数和官方对不上，就是这个判定漏了它）。对拍：`mob-oracle --only wandering_trader` 的 `wanderingTraderSave` 一题用官方 `TRADER_LLAMA.saveWithoutId` → `load` → 再存一次，逐键比 `DespawnDelay` 和马科 / 通用存档键，0 不一致；刷新器的 Rust 单测确认两只羊驼成年、拴绳指向商人、商人 `despawnDelay=48000` / `home` 正确；`--break-self-test wt_save` 同时抓到商人 / 羊驼的 `DespawnDelay` 偏差。行商羊驼被拴着跟商人走、随商人 `despawnDelay` 一起消失现在由流浪商人的逐刻场景对拍。**已知限制**：`TraderLlamaDefendWanderingTraderGoal`、针对僵尸 / 灾厄村民的两条目标和 `doPlayerRide` 的不可骑分支没有专属逐刻场景；`makeNewLlama` 的繁殖（`BREEDING` + 标持久）和 `canDespawn` 里「拴到别的东西 / 已驯服」两条在 Bamboo 里没有触发路径（行商羊驼只由刷新器拴在商人身上、玩家驯服未建模），按官方的其余条件落地；羊驼的 `despawnDelay` 已照官方 `holder.getDespawnDelay()-1`：通过 `MobPublic.trader_despawn_delay` 按拴绳 uid 读商人的倒计时；商人不在本岛时退回自己的 47999 递减。

- **僵尸村民**：`/summon minecraft:zombie_villager`、刷怪蛋和群系刷怪表（5 权重，夜里 / 暗处）都能生成。它继承 `Zombie` 的属性、目标、装备生成、幼年 / 鸡骑士、日晒和 `entities/zombie_villager` 掉落，另加 `VillagerData`（构造时随机职业、生成时按群系选品种）、`DATA_CONVERTING_ID` 19 / `DATA_VILLAGER_DATA` 20 / `DATA_VILLAGER_DATA_FINALIZED` 21 三项元数据，以及 `VillagerData` / `VillagerDataFinalized` / `Offers` / `Gossips` / `ConversionTime` / `ConversionPlayer` / `Xp` 存档键（可选键按官方 `storeNullable` 只在非空时写）。村民被僵尸一系（含溺尸的三叉戟）打死时按官方 `Zombie.killedEntity` 转化：困难必转、普通先在攻击者自己的随机源抽一次 `nextBoolean()`、简单不抽也不转；转化按 `ConversionParams.single(true, true)` 搬数据、跑 `Zombie.finalizeSpawn`、发 1026 号世界事件。主手金苹果且身上有虚弱时右键开始治疗：时长 `3600 + nextInt(2401)`，移除虚弱、按难度加力量、发实体事件 16；每刻先取一次 `nextFloat()`，`< 0.01` 时扫周围 9×9×9 格里最多 14 个 `#speeds_up_zombie_villager_curing`（铁栏杆 / 床）方块，每个再取一次 `nextFloat() < 0.3` 决定多减一刻；到点变回村民、搬回全部数据、挂 200 刻反胃、发 1027 号世界事件和 `minecraft:cured_zombie_villager` 进度。转化中或有交易经验时不按距离消失。对拍：`mob-oracle --only zombie_villager` 10 个逐刻场景（普通 / 困难 / 简单感染、铁栏杆 / 床 / 无加速 / 无虚弱的治疗、日晒）和 1 题存档往返，0 新增不一致；`--break-self-test zv_no_roll` / `zv_no_weakness` / `zv_cure_radius` / `zombie_villager_save` 四处改坏都会报不一致；`mob-oracle --only zombie_villager_night` / `_day` 覆盖自然生成；`chunk-content java` 两题存档、`natural-spawn java` 的刷怪表、`advancement-trigger-oracle java` 的触发器判据都已接上。**已知限制**：八卦声望系统还没做，`ConversionPlayer` 只存 UUID 并留接口，不广播 `ZOMBIE_VILLAGER_CURED` 声望；`Offers` / `Gossips` 只原样存档不解析；治疗完成调离线版 `villager_ai::refresh_brain_offline` 重建大脑（第 3 期合入后接上），但没有搬僵尸的装备到村民 8 格 `Inventory`；`GameEvent.ZOMBIE_INFECT` 振动和治愈音效的客户端音高取数不单独发；日晒 oracle 的 Java 假世界没有天空亮度，只比实体类型和随机源，不真的点燃。致命转化抽签在攻击者的 `doHurtTarget` 里按 `public.health <= amount` 预判致死：若实际伤害被无敌帧 / 伤害冷却 / 抗性 / 吸收 / 抗火挡下（或并发驱动下血量在命中前被别人改动），Bamboo 会比官方多抽或少抽一次 `nextBoolean()`，攻击者后续随机序列可能错位；正常一路打死的情形（含逐刻对拍）逐位一致。

- **鹦鹉**（2026-11-22，分支 `op-misc`）：可用 `/summon minecraft:parrot` 或鹦鹉刷怪蛋生成，丛林按 `checkParrotSpawnRules`（脚下 `#parrots_spawnable_on` + 亮度 > 8）自然刷出。属性 6 血 / 0.2 移速 / 0.4 飞行速度 / 3 攻击，尺寸 0.5×0.9（眼高 0.765）；五种颜色（`DATA_VARIANT_ID` index 20，`Variant` 按官方旧版整数编码存盘），`canBeABaby` 为假、不繁殖。新增**飞行导航**：`FlyNodeEvaluator` 逐位移植（六向 + 十二个斜向邻居、`WALKABLE` 加一、飞行的路径类型规则），`FlyingPathNavigation` 的起点、临时位置、`getGroundY` 跳过和地表检查跳过，`FlyingMoveControl(10, false)`（空中用 `FLYING_SPEED`、每刻设 `noGravity`、`yya` 推竖直速度），`LivingEntity.travelInAir` 的 `omnidirectionalAirMover` 竖直空气阻力 0.91，`Parrot.aiStep` 的 `calculateFlapping`（含下落时 `delta.y *= 0.6`）和翅膀声。复用 `tame.rs`：`#parrot_food` 种子按 1/10 概率驯服（事件 7 / 6，吃食声音高取两次 `nextFloat`），主人按右键在站在地上时切换坐下；曲奇（`#parrot_poisonous_food`）中毒 900 刻并按玩家攻击结算 `Float.MAX_VALUE` 伤害（交互后由游戏层用 `MobInteraction::self_hurt` 落地）。每 400 刻按世界随机源抽一次模仿附近生物（20 格内、类型在 `MOB_SOUND_MAP` 里、非静音，取一只后按它的类型放模仿音），叫声每 1000 次抽一次从 42 条模仿音里挑；`ParrotWanderGoal` 的树桩落点（±3/±6/±3、树叶或原木上方两格空）和 `HoverRandomPos` / `AirAndWaterRandomPos` 的悬停落点都已接。对拍：`mob-oracle --only parrot` 4 例 860 刻 0 不一致（树桩闲逛、驯服与坐站、曲奇死亡含掉落和包、模仿僵尸叫声）；`--only natural_spawn` 734 例（`parrot_jungle=31`、`parrot_jungle_dim=0`）、`--only gen_spawn` 7000 例 0 不一致。已知限制：**上肩已接上**：`LandOnOwnersShoulderGoal` 在冷却 100 刻、主人不是旁观者、没坐下且左右肩有空位、两只碰撞箱相交时，走官方 `setEntityOnShoulder` 把整只鹦鹉（`saveWithoutId` NBT）存进玩家的 `ShoulderEntityLeft` / `Right` 并 `discard()`；`DATA_SHOULDER_PARROT_LEFT/RIGHT`（OPTIONAL_UNSIGNED_INT，index 19/20）逐刻同步给客户端，玩家档 `ShoulderEntityLeft` / `Right` 存读往返（`player-data java` 2 用例）。`handleShoulderEntities` 每刻按 fallDistance>0.5 / 水 / 飞行 / 睡觉 / 细雪，上肩 20 刻后把鹦鹉从 NBT 造回世界；`mob-oracle --only parrot_shoulder` 的 6 个逐刻场景（跳起 / 落水 / 受伤 / 睡觉 / 细雪 / 飞行）在假世界里真跑 Bamboo 自己的 `LandOnOwnersShoulderGoal` → `Command::SetEntityOnShoulder` → 共享肩膀状态 → `respawnEntityOnShoulder` 生产链路，逐刻比左右肩品种、鹦鹉还在不在世界里和造回来的位置（跳起场景 0 场景差异，只有 12 处全局三角函数表差）；`--break-self-test shoulder_keep_entity` / `shoulder_no_fall` / `shoulder_swap` 三处改的是生产代码（上肩不移除 / 少一种离肩判据 / 左右肩反序）都能报不一致；`FollowMobGoal` 只占优先级位（不跑）；`MOB_SOUND_MAP` 在官方是 `HashMap`，迭代顺序和按名字排序的表不同，每 1000 次叫声里选到的具体模仿音可能不一致；悬停落点一开始从选中落点那一刻起就和官方差两次实体随机取数，根因是 `LandRandomPos.generateRandomPosTowardDirection` 少了官方的第三个判定 `GoalUtils.isNotStable`（`FlyingPathNavigation.isStableDestination`，这里用「目标格是实心方块」近似），已补上：补上后一个无树桩的 400 刻临时悬停场景（种子 906，未进对拍表）从 308 处不一致降到 3 处，随机源逐刻一致；残留的 3 处（第 245–247 刻）只有 `xr` / `hr`：那几刻官方 `LookAtPlayerGoal` 已经在看玩家、`Bamboo` 还没开始，而两边随机源逐刻相同，所以不是取数问题，原因没查到（怀疑两个选择器整刻跑的相位和官方差一刻）；飞行寻路的 `FlyNodeEvaluator.getStart` 少了小体型生物那十个随机候选：鹦鹉的包围盒 `getSize()` 是 0.633（`< 1.0`），官方会从实体随机源抽最多十个起点候选，`Bamboo` 的寻路请求里没有实体随机源，只试包围盒四角；只在起点格代价为负时才走这条，对拍场景没触发；`PlayerView` 没有玩家的飞行 / 水里 / 细雪状态，`LandOnOwnersShoulderGoal.canUse` 只检查了旁观者；`Parrot.getAmbientSound` 的世界随机源取数顺序和模仿判定的随机源都在，但没有和真实客户端核对过模仿音的听感；拴绳已建模（`canBeLeashed` 按官方覆盖）。
- **悦灵**（分支 `allay`）：`/summon minecraft:allay` 和悦灵刷怪蛋生成，`PathfinderMob` + `InventoryCarrier` + `VibrationSystem`。属性 20 血 / 0.1 飞行速度 / 0.1 移速 / 2 攻击，尺寸 0.35×0.6（眼高 0.35），`FlyingMoveControl(20, true)`（原地悬停）+ 复用鹦鹉的 `FlyingPathNavigation`。元数据 `DATA_DANCING` / `DATA_CAN_DUPLICATE` 两个连续布尔；每 10 刻回 1 血，跳舞时每 20 刻检查 `shouldStopDancing`，复制冷却每刻减一。大脑 `AllayAi`：三个常驻记忆（`LIKED_PLAYER` / `LIKED_NOTEBLOCK_POSITION` / `LIKED_NOTEBLOCK_COOLDOWN_TICKS`）、四个传感器（`NEAREST_LIVING_ENTITIES` / `NEAREST_PLAYERS` / `HURT_BY` / `NEAREST_ITEMS`）、`core` / `idle` 两个活动；`RandomStroll.fly(1)` 走 `AirAndWaterRandomPos.getPos(mob, 10, 7, -2, dir.x, dir.z, PI/2)` 的飞行随机落点（视角向量用 `LivingEntity.getViewYRot` 覆写后的头部 `y_head_rot_o`），`GoToWantedItem`（要能捡东西、`WALK_TARGET` 缺席、世界边界内）、`GoAndGiveItemsToTarget`（送给喜欢的玩家 / 音符盒、扔完设 60 刻捡物冷却）、`StayCloseToTarget`（`closerThan` 用 `tooFar=16`、`WalkTarget` 用 `close_enough=4`）、`SetEntityLookTargetSometimes(6, UniformInt(30,60))` 按官方优先级顺排。交互：手持物品右键成为喜欢的玩家并放 `ALLAY_ITEM_GIVEN`，空手右键收回物品（`ALLAY_ITEM_TAKEN`、把背包全扔出、擦掉喜欢的玩家）；跳舞 + `#duplicates_allays` + 能复制时用紫水晶碎片复制一只悦灵（两者 6000 刻冷却、实体事件 18、扣一片、播紫水晶钟声）。振动：`Allay$VibrationUser` 半径 16、眼睛位置，只认 `#allay_can_listen`（`NOTE_BLOCK_PLAY`），收到音符盒振动 → `hearNoteblock` 记 `GlobalPos` + 600 刻冷却；`Allay$JukeboxListener` 半径 10，`JUKEBOX_PLAY` / `STOP_PLAY` 控制跳舞（`setJukeboxPlaying(false)` 只在位置相同时停）。喜欢的音符盒在 1024 格内、同维度、仍是音符盒且冷却没过期时优先送物品（送到音符盒上方一格），否则擦记忆改送 64 格内生存 / 创造的喜欢玩家。存档：`Inventory` 列表、`listener`（`VibrationSystem.Data` CODEC）、`DuplicationCooldown`（写 `putLong`、读 `getIntOr 0`）。`mob-oracle --only allay` 四个逐刻场景（给玩家送物、音符盒改送点、唱片机跳舞、紫水晶复制）逐刻比大脑 / 位置 / 速度 / 朝向 / 跳舞与能复制标志 / 复制冷却 / 手持与背包物品数 / 玩家手持 / 生物与掉落物计数和两侧随机源，另比 `allay` 存档往返。已知限制：飞行寻路 `FlyNodeEvaluator.getStart` 少了小体型生物那十个随机候选（和鹦鹉共用的限制，见「鹦鹉」一节）；悦灵自然生成不在任何群系刷怪表里，只由 `/summon` 和刷怪蛋生成；`allay` 逐刻对拍仍在收尾，当前还差 `Mth.COS_TAB` / `ASIN_TAB` 的 ARM 平台末位差和逐刻随机源对齐的最后一小段（见 `.agent-notes.md`）。
- **蜜蜂与蜂巢 / 蜂箱**（2026-11-23，分支 `bees`）：`MobType::ALL` 扩到 51 种（含 main 合入的蠹虫 / 末影螨 / 铁傀儡 / 雪傀儡），蜜蜂可用 `/summon minecraft:bee` 或刷怪蛋生成，**不在任何群系刷怪表里**，主要由树上的蜂巢放出。属性 10 血 / 0.6 飞行速度 / 0.3 移速 / 2 攻击、尺寸 0.55×0.5，复用鹦鹉的 `FlyingPathNavigation` 和 `FlyingMoveControl`（蜜蜂用 `FlyingMoveControl(20, true)`，寻路代价 FIRE / WATER / COCOA / FENCE = -1、WATER_BORDER = 16）；`DATA_FLAGS_ID`(18) 的翻滚 / 已蜇 / 有花蜜三位与 `DATA_ANGER_END_TIME`(19) 同步，`HasNectar` / `HasStung` / `TicksSincePollination` / `CannotEnterHiveTicks` / `CropsGrownSincePollination` / `hive_pos` / `flower_pos` 按官方存档。蜂巢 / 蜂箱有活的 `BeehiveBlockEntity`：`bees` 列表按 `Occupant.LIST_CODEC` 存读、逐刻加 `ticks_in_hive`，到点按「有花蜜 / 时间线 / 天气」放出蜜蜂、按 `nextInt(100)` 给 `honey_level` 加一（或加二）、出口碰撞 / 营火安抚 / 剪刀取蜜脾 / 玻璃瓶取蜂蜜 / 无精准采集挖掉放蜂都已接。`BEES_STAY_IN_HIVE` 按官方 `timeline/day.json` 的两条关键帧（由生成器导出 12542 / 23460 / 24000）加 RAIN / THUNDER 天气层求值，跨关键帧和下雨都进了逐刻对拍。世界生成的 `BeehiveDecorator` 放巢时把 2–3 只 `Occupant.create(nextInt(599))` 写进方块实体 NBT，走 main 已有的生成期方块实体交付路径；首次读盘由 `archive::block_entity` 造回活实例。对拍：`mob-oracle --only bee` 6 例（白天出巢、夜晚 / 下雨留巢、带蜜回巢加蜜、满蜜、营火安抚取蜜）逐刻比巢里每只蜂的计数、蜜等级、放出数、世界随机源，0 不一致；`--break-self-test bee_clock` / `bee_honey` 两处改坏都能抓到；`block_entity_resave` / `chunk-content` 覆盖蜂巢 NBT，`oracle java --weather clear` 0 失败。蜜蜂的目标集已按官方字节码实现（`bee_goals.rs`）：`BeeAttackGoal`、`BeeEnterHiveGoal`、`ValidateHive/FlowerGoal`、`BeePollinateGoal`（找花、悬停、400 刻采粉）、`BeeLocateHiveGoal`、`BeeGoToHiveGoal`、`BeeGoToKnownFlowerGoal`、`BeeGrowCropGoal`（作物 / 甜浆果 / 洞穴藤蔓）、`BeeWanderGoal`（复用鹦鹉的飞行随机落点）、`BeeHurtByOtherGoal`、`BeeBecomeAngryTargetGoal`，加繁殖 / 诱惑 / 跟随父母 / 漂浮。`Bee.doHurtTarget` 用 `sting` 伤害类型、按难度给 POISON（普通 10 秒 / 困难 18 秒）、置 `HasStung`、停怒并按 `nextInt(clamp(1200 - timeSinceSting, 1, 1200))` 抽签死亡；放出巢里的蜂时给 4 格内无目标的蜂设成玩家目标。精准采集改读官方 `PREVENTS_BEE_SPAWNS_WHEN_MINING` 附魔标签。`mob-oracle --only bee_ai` 8 例（采蜜、回巢、授粉、蜇人、跟随拿花的玩家、愤怒、带蜜、诱惑）逐刻比花蜜 / 已蜇 / 计时 / 愤怒 / 血量 / 随机源，0 不一致；`--break-self-test bee_ai` / `bee_ai_anger` / `bee_clock` / `bee_honey` 都能抓到。放出蜜蜂的两段独立逻辑照官方拆开：`emptyAllLivingFromHive` 用本次取蜜 / 挖巢的玩家本身、按玩家到每只蜂的 4 格距离设目标或（营火安抚时）400 刻不准进巢；`BeehiveBlock.angerNearbyBees` 另取 8/6/8 盒内无目标的蜂，每只按世界随机源 `nextInt(玩家数)` 抽一个玩家设目标（只有一个玩家也照取）。`mob-oracle --only bee_release` 4 例（一 / 两名玩家取蜜、营火取蜜、挖掉蜂巢）逐刻比每只蜂的目标、倒计时和世界随机源，0 不一致；`--break-self-test bee_release` 少取一次随机数能抓到。**已知限制**：`BeeLocateHiveGoal` 现在走 `PoiManager.getInRange(#bee_home, 20, ANY)`（精确 / 并行驱动分别读本岛 `PoiStore` 当前状态和本刻开始快照，按 `blockPosition().distSqr(pos)` 稳定排序、用 `BeehiveBlockEntity.isFull()` 过滤满巢），不再是按区块段调色板逐格扫。`mob-oracle --only bee_locate` 4 个逐刻场景（复用 NoAI 夹具，不跑真目标链 / 寻路）逐刻比整张有序候选表：POI 遍历序、满巢过滤换巢、等距并列、整数距离排序，0 新增不一致；`--break-self-test bee_locate_float_sort` / `bee_locate_no_filter` / `bee_locate_radius19` 三处改坏（排序改回浮点、漏满巢过滤、半径 20 改 19）都能抓到。官方 `PoiSection.byType` 的键是 `Holder.Reference`（identity hash），多类型段内键序每次 JVM 都不同、不可复现，所以 `bee_locate` 的并列场景只用单一 POI 类型（见 `docs/villager-plan.md` 偏差 1）。**整条目标链逐刻对拍（真进 / 出巢、采蜜 / 催熟、整群激怒、夜里 / 雨天回巢时的位置 / 目标 / 随机源 / 巢内蜂数 / 蜜等级 / 出站包）还没有建**（bee_ai 场景用 `NoAI` 比状态机字段，bee / bee_release 场景只比巢方块实体和放蜂 / 激怒；这套「真目标链 + 真 POI + 寻路 + 出站包」的完整夹具单独排期，村民第 4 期共用）；`ValidateHive/FlowerGoal`、`BeePollinateGoal` 的方块扫描、飞行寻路落点、`BeeGrowCropGoal` 的催熟结果、`BeehiveDecorator` 的 NBT 也都没有与官方逐题对拍。
- **羊**：可用 `/summon` 或羊刷怪蛋生成；自然生成颜色按群系权重（含 1/500 粉色），剪刀剪毛按 `shearing/sheep` 战利品表掉 1–3 团羊毛并扣剪刀耐久，染料染色，`EatBlockGoal` 吃草让草方块变泥土并长回羊毛（幼年加速长大），繁殖按合成配方混色、混不出时按世界随机源选一方；死亡按颜色和剪毛状态选子战利品表；`Sheared` / `Color` 存盘，羊毛字节同步。脚下有矮草（`edible_for_sheep`）时先吃矮草（`mob-oracle` 的 `sheep_eat_plant`）；`mobGriefing` 关掉时方块不变，但羊照样长回羊毛、幼年加速（`sheep_eat_no_grief`）。已知限制：`EatBlockGoal` 的游戏事件（`EAT` / `SHEAR`）不发；发射器剪羊毛未接入。
- **鸡**：可用 `/summon` 或鸡刷怪蛋生成；花纹和叫声变种、构造时的下蛋计时（`6000 + nextInt(6000)`）、按 `gameplay/chicken_lay` 礼物表按花纹下（蓝 / 棕）蛋、缓慢下落、种子繁殖和 `EggLayTime` / `IsChickenJockey` 存盘。玩家右键和发射器都能扔出鸡蛋（含蓝 / 棕蛋）；玩家每次扔蛋按物品种类计一次 `ITEM_USED`，创造模式照计，当前只在会话内累计。命中时对生物打 0 点 `thrown` 伤害（有击退），按蛋自己的随机源 1/8 孵小鸡、其中 1/32 孵四只，小鸡按蛋的花纹组件定花纹、从零尺寸找空位放下；发射器扔蛋那一刻的位置、速度、散布和剩余物品与官方逐位一致（`mob-oracle` 的 `egg_dispense` / `egg_dispense_up`）；玩家扔的蛋打中动物时记下 `lastHurtByPlayer`、动物恐慌逃跑，和官方逐刻一致（`egg_hit_cow_owned`）。鸡骑士按 5% 生成并骑在鸡上（见「生物骑乘」），`IsChickenJockey` 按实际状态存盘；已知限制：扇翅膀只影响客户端动画，服务端不记 `flyDist` 的翅膀游戏事件。下蛋声在战利品落地前取音高，礼物表求值失败时官方不放声音，Bamboo 仍会放。扔出的鸡蛋：首刻的气泡柱推力不建模；撞到方块不触发 `onProjectileHit`（标靶、钟、紫颂花等），也不发 `PROJECTILE_SHOOT` / `PROJECTILE_LAND` 游戏事件；打中下落方块不 `markHurt`；水体熄火复用箭的扫掠流体判定，在进出水都发生在同一刻时也会触发；流动水仍按整格水平范围近似，降雨熄火未建模；孵化找空位按盒子切格子求最近点，没有复刻 `VoxelShape` 合并坐标时 1e-7 的容差和同距并列时的遍历顺序。雪球已走通用投掷物实体管道（玩家右键也能扔，见「铁傀儡和雪傀儡」一节）；末影珍珠、喷溅药水另有各自实体。


- **生物骑乘**：生物骑生物按官方 `Entity.startRiding` / `stopRiding` / `rideTick` / `positionRider`：乘客每一刻由载具驱动（`commonTick + rideTick`，位置取载具位置加载具的 `PASSENGER` 附着点、减去乘客自己的 `VEHICLE` 附着点，附着点数值按 `EntityTypes` 注册表里的位模式），载具的乘客名单和乘客的 `vehicle` 双向维护，下车给 60 刻冷却；载具死亡、被移除或关系断开时乘客当场摘下，生物乘客停在原地，玩家按 `getDismountLocationForPassenger` 落位（都走同一套 `stopRiding` / `removePassenger`）。`ServerLevel.tickPassenger` 的 uid 顺序也照做：乘客的 `checkDespawn` 排在它的 `rideTick` 之后；载具由生物乘客控制时（不在 `#non_controlling_rider` 里）`Mob.updateControlFlags` 关掉 MOVE/JUMP/LOOK；同载具的实体不互相推。乘客不单独成行：存档按 `Passengers` 嵌在载具 NBT 里、乘客 `Pos` 用载具的 x/z 加自己的 y，读档递归 `startRiding(force)`；网络的 `set_passengers` 在配对时按「自己有乘客发自己、自己在骑发载具」两个方向发。拆岛时整组载具和乘客一起搬，加调试不变量检查。蜘蛛骑士（1%）、鸡骑士（5%）和尸壳的骆驼尸壳骑士（10%，载两名乘客）按 `finalizeSpawn` 的取数顺序生成并骑上，骷髅陷阱马的 1+3 名骑士也接上了 `startRiding`；骆驼覆写 `canAddPassenger` 后能带两名乘客。控制载具的骑手（生物乘客且不在 `#non_controlling_rider` 里）按官方 `Mob.getNavigation()` / `getMoveControl()` 借**载具**的导航和移动控制：两个 getter 经 `getControlledVehicle()` 一路转到载具链顶端，骑手的目标和寻路写进载具的导航从而驱动载具，移速、步高和寻路代价都用载具自己的；`getJumpControl()` / `getLookControl()` 不转交、仍用骑手自己的字段。`Mob.serverAiStep` 里 `navigation.tick()` / `moveControl.tick()` 用的是字段而不是 getter，借导航时临时换回骑手自己闲置的那两份各 tick 一次，大脑与控制段仍看载具的导航。载具被生物乘客控制时 `Mob.updateControlFlags` 关掉自己的 MOVE/JUMP/LOOK 目标，靠借来的路径经 `travel` 走动；弓骷髅的 `RangedBowAttackGoal` strafe 分支额外让 `getControlledVehicle()` 的载具 `lookAt` 目标，`AbstractSkeleton.rideTick` 把载具的 `yBodyRot` 抄给骑手。骷髅马 / 僵尸马没上鞍被生物骑时 `AbstractHorse.getControllingPassenger()` 回落到 `Mob` 版（骑手就是控制乘客），载具朝向不跟骑手、`tickRidden` 那支不走，乘客的 PASSENGER 附着点是注册的 `1.31875f` 而不是马的身高 1.6。精确驱动和并行驱动都在乘客 tick 前后借入 / 写回共享导航、移动控制与载具朝向，读写顺序照现有骑乘，逐位一致（有单测）。对拍：`mob-oracle` 的 `spider_ride_chase`（弓骷髅骑蜘蛛追玩家并射箭）、`zombie_ride_melee`（幼年僵尸骑鸡追玩家近战）、`skeleton_horse_ride_chase`（持弓骷髅骑没上鞍的骷髅马追玩家并射箭，覆盖马科和蜘蛛 / 鸡不同的 `travel` / `tickRidden` 路径）、`spider_ride_dismount`（第 20 刻下马后各自用回自己的导航）逐刻比骑手和载具的位置、速度、朝向、寻路目标和随机源，0 不一致（Java 侧还断言马真的位移、借到路径、锁定玩家并射出箭）；`ride_bow` / `ride_baby` / `camel_husk_ride` / `ride_nav`（丢掉马借来的路径）四处改坏都报不一致。已知限制：玩家控制的载具（马 / 猪 / 骆驼）走 `travelRidden`，位置由客户端 `move_vehicle` 给，不在这条借导航链里；`#non_controlling_rider` 里建模的只有史莱姆和硫磺方块怪（岩浆怪未建模）。玩家骑手和生物乘客已共用这一份 `passengers` 名单：玩家侧的 `Riding.vehicle` 与名单双向一致，`getFirstPassenger` / `getControllingPassenger` 都从名单算；载具死亡、被移除、`/ride <目标> dismount` 或下车时走同一套 `stopRiding`（玩家按 `getDismountLocationForPassenger` 落位、生物停原地，`set_passengers` 按名单变化发一次）；`/ride <目标> mount <载具>` 按官方 `RideCommand` 的 `startRiding(force)` 建立同一份名单；骑着东西下线时根载具连整棵乘客树写进玩家档的 `RootVehicle`（含 `Attach`），上线读回重新骑上，只有一名玩家乘客时载具从世界移除；拆岛把「载具 + 玩家骑手」当成不可分的一组；`/data get entity` 按 `Entity.saveWithoutId` 带嵌套 `Passengers`。
- **鱿鱼 / 发光鱿鱼**：可用 `/summon` 或刷怪蛋生成，也会自然生成（鱿鱼 `WATER_CREATURE`：`IN_WATER` 落点 + 海平面下 13 格以内、下面是水、上面是水方块；发光鱿鱼 `UNDERGROUND_WATER_CREATURE`：海平面下 33 格以下、原始亮度 0、这一格是水方块；组里第二只起 5% 幼年）。10 点血；按触手节拍游动（两个不占标志位的目标只写移动向量，`travel` 只是 `move`），不吃水流；离水每刻掉 1 点氧气，到 -20 受 2 点溺水伤害；被生物（含玩家）打到时喷墨（喷墨声 + 30 个 `squid_ink` / `glow_squid_ink` 粒子包，发给 32 格内的玩家）并逃离攻击者，发光鱿鱼再变暗 100 刻（`DarkTicksRemaining` 存盘并同步）；死亡按 `entities/squid` / `entities/glow_squid` 掉墨囊 / 荧光墨囊，被玩家打死给 1–3 点经验。逐刻对拍见 `mob-oracle --only squid` / `--only glow`。已知限制：拴绳的 `canBeLeashed` 已按官方覆盖（鱿鱼可拴）；刷怪蛋对着水面用（`SpawnEggItem.use` 的流体射线）和对着鱿鱼用（生幼崽）未实现，只能点方块；`SWIM` 等游戏事件不发；守卫者已建模（见「结构刷怪覆盖」与守卫者条目），但这里鱿鱼 / 发光鱿鱼被守卫者当猎物的逐刻场景仍未对拍；美西螈的 `AxolotlAttackablesSensor` 会把水里的鱿鱼 / 发光鱿鱼当捕猎目标（`#axolotl_hunt_targets`），但大脑运行没有逐 tick 对拍；非生物来源的伤害照官方 `Squid.hurtServer` 返回假（不喷墨）。
- **蝙蝠**（分支 `cl-ambient`，2026-11-22）：`AmbientCreature` 一支，可用 `/summon` 或刷怪蛋生成，也会自然生成（`AMBIENT`：`ON_GROUND` 落点，在 `WORLD_SURFACE` 高度图之下、世界随机源 `nextBoolean` 为假、亮度不超过 `nextInt(4)`、脚下在 `#bats_spawnable_on` 里；26.3 已经没有万圣节窗口和海平面条件）。6 点血；没有目标也不寻路：倒挂时头上不是导体或 4 格内有看得见的玩家就起飞（世界事件 1025），飞行时朝随机方块加速、竖直速度每刻乘 0.6，1/100 且头上是导体时重新倒挂并钉在方块下面；倒挂时四次里三次不叫，音量 0.1、音高 ×0.95；不推人也不被推、没有摔落伤害、没有脚步声；受伤先醒过来；`BatFlags` 存盘、标志字节同步。`mob-oracle --only bat`（4 例 2424 刻）和 `--only bat_spawn`（24 例 96 刻）0 不一致。已知限制：开始倒挂那一刻官方的碰撞箱比位置晚一刻更新（最多 0.1 格），Bamboo 不模拟；击退落在自己这一刻之后时速度包晚一刻。
- **幻翼**（分支 `cl-ambient`，2026-11-22）：官方 `Phantom`（`Mob implements Enemy`，不是 `Monster` 也不寻路），可用 `/summon minecraft:phantom` 或幻翼刷怪蛋生成；白天在露天会像亡灵一样着火（`#burn_in_daylight`），被玩家打死给 5 点经验、按 `entities/phantom` 掉幻翼膜。20 点血、跟随 16；`size` 0–64 存档并同步，尺寸按 `1 + 0.15 * size` 缩放（眼高一起缩放），攻击力基础值 `6 + size`（`setPhantomSize(0)` 不触发更新，所以大小 0 的幻翼仍是 2 点攻击）。AI 照官方四个目标：目标选择器每 60 刻扫描一次 64 格内、碰撞箱外扩 (16, 64, 16) 的玩家（按 Y 从高到低，`forCombat`），`PhantomAttackStrategyGoal` 把锚点放到目标头上 20–39 格（低于海平面时改到海平面上一格）、倒数 5 刻后转入俯冲并按 `(8 + nextInt(4)) * 20` 折半排下一次，放 `entity.phantom.swoop` 音效；`PhantomSweepAttackGoal` 朝目标身体中间飞，0.2 格接触就咬一口（攻击力、世界事件 1039），撞墙或刚受伤也回到盘旋；`PhantomCircleAroundAnchorGoal` 每刻以 15° 绕锚点转，距离 5–15、高度 -4..5，350 / 250 / 450 折半的随机重抽，到点或上下被方块挡住时重选。`PhantomMoveControl` 每刻最多转 4°、速度在 0.1 / 1.8 / 0.2 之间逼近，速度往新方向混合 20%，`travel` 是 `travelFlying(0.2)`：水 0.8、岩浆 0.5、空气 0.91，没有重力。**幻翼刷怪器**（`PhantomSpawner`，主世界每刻）：`spawnMobs` / `spawnMonsters` / `spawnPhantoms` 都开、`nextTick` 倒数到 0（`(60 + nextInt(60)) * 20`）后，夜里 / 雷暴或没有天空光、非旁观者、海平面以上且看得见天、区域难度大于 `nextFloat() * 3`、`clamp(TIME_SINCE_REST, 1, MAX)` 的 `nextInt` 至少 72000，才在玩家头顶 20–34 格、水平 ±10 格找一个空位，按 `1 + nextInt(difficulty + 1)` 只数逐只生成并 `finalizeSpawn`（锚点头上 5 格、大小 0）。`TIME_SINCE_REST` 补了最小实现：不睡觉每刻 +1（int 环绕）、入睡清零、死亡清零；没有写原版 `stats/<uuid>.json`，重启后从 0 重新累计。`mob-oracle --only phantom`（4 个刷怪器场景 + `phantom_hunt` 盘旋 / 俯冲 / 咬中 + `phantom_burn` 白天着火，6 例 534 刻）0 不一致；两个改坏确认（刷怪器整段不跑、出生点高一格）都能抓到。全量 `mob-oracle` 1841 例 190745 刻、`--only natural_spawn` 746 例 2984 刻，0 不一致。对拍还暴露并修了一个公共同步问题：`Entity.push` 推开自己那一半时官方置 `needsSync`，Bamboo 之前只改速度；现在生物互推的两半都会置位（不改变推开的位移）。已知限制：幻翼怕猫的 `PhantomSweepAttackGoal` 分支没有建模（16 格内的猫搜索和 `cat.hiss()` 都不做，不会被吓跑）；刷怪器 / `nextTick` 按岛保存，拆岛时两边从同一个倒数继续、并岛时保留吸收方的，而官方整个主世界只有一个刷怪器；客户端专用的扇翅音效、`MYCELIUM` 粒子和 `isFlapping` 不在服务端；`moveTargetPoint` / `attackPhase` / `PhantomMoveControl.speed` 不存档（官方也不存）。
- **鳕鱼 / 鲑鱼 / 热带鱼 / 河豚**：`WaterAnimal` → `AbstractFish` 一支，可用 `/summon` 和刷怪蛋生成，并会自然生成（`WATER_AMBIENT`：`IN_WATER` 落点 + 海平面下 13 格内、下面是水、上面是水方块；热带鱼在繁茂洞穴仍要求上下水，但不限海平面高度）。3 点血；水下用 `WaterBoundPathNavigation`（`SwimNodeEvaluator`，不允许跃出水面），`FishMoveControl` 每刻上浮 0.005、速度按 0.125 插值；`travelInWater` 推进 0.01、乘 0.9、没有目标时每刻下沉 0.005；离水落在方块上会扑腾（`cod.flop` 等，取两次 `nextFloat` 定音高）并重设 `needsSync`；氧气按 `WaterAnimal.handleAirSupply` 掉，到 -20 受 2 点溺水伤害。目标：0 惊慌、4 `RandomSwimmingGoal`（`BehaviorUtils.getRandomSwimmablePos`，最多 11 次抽点）、5 `FollowFlockLeaderGoal`（成群）；河豚多一个 1 `PufferfishPuffGoal`。鲑鱼的花纹（small / medium / large，权重 30/50/15）按尺寸缩放并同步、存 `type`；热带鱼的花纹打包值（低 16 位花纹、16–23 底色、24–31 花纹色，22 个常见组合由官方导出）按组共享、存 `Variant`；河豚 `PuffState` 0/1/2 决定尺寸、膨胀时蜇附近生物（1+状态点伤害 + 60×状态刻中毒）并存 `PuffState`；河豚没有叫声（`makeSound(null)` 不取音高随机数）。鱼群按 `FollowFlockLeaderGoal` 跟随同种的领头鱼（8 格内、最多到各自的 `getMaxSchoolSize`）。自然生成按官方 `getMaxSpawnClusterSize`（鳕鱼 / 热带鱼 8、鲑鱼 5、河豚 8）和 `TropicalFish.isMaxGroupSizeReached`（单条不成群时一组只出这一条）收尾；`WATER_AMBIENT` 的 `canSpawnFarFromPlayer` 为假（离玩家超过 64 格不刷），鱼不是 `Animal` 所以 `getWalkTargetValue` 取 0。桶装（`Bucketable`）：拿水桶右键活鱼会放 `item.bucket.fill_fish`、把鱼从世界拿掉并换给玩家对应的鱼桶，`bucket_entity_data` 存 `NoAI` / `Silent` / `NoGravity` / `Glowing` / `Invulnerable` / `PersistenceRequired` / `Health`，鲑鱼的 `salmon/size`、热带鱼的三个花纹组件另存；倒鱼桶先放水源再按 `MobBucketItem.spawn` 放出 `FromBucket = true` 的鱼（朝向取 `level.random`，`finalizeSpawn` 用 `BUCKET` 原因，再套组件和 `loadFromBucketTag`），发射器倒鱼桶同样会生成鱼。逐刻对拍见 `mob-oracle --only cod` / `salmon` / `tropical` / `puffer` / `natural_fish`。已知限制：玩家附近 8 格内的 `AvoidEntityGoal`（优先级 2）未建模；鱼群的初始跟群倒计时在官方是构造函数里从实体随机源抽的（那时还没有种子），这里固定为折半后的 100；`Pufferfish.playerTouch`（玩家碰到被蜇）未接；水花声（`entity.fish.swim`）现在和其他生物一样按体积发 `ClientboundSoundPacket`。
- **海豚 / 美西螈**（分支 `op-water`，2026-11-21）：海豚是 `AgeableWaterCreature`（`AgeableMob` 但不是 `Animal`），美西螈是 `Animal` + `Bucketable`，都可用 `/summon` 和刷怪蛋生成，也会自然生成（海豚 `WATER_CREATURE`：和鱿鱼同源的 `checkSurfaceAgeableWaterCreatureSpawnRules`，`IN_WATER` 落点、海平面下 13 格以内、下面是水、上面是水方块；美西螈 `AXOLOTLS` 类别：`IN_WATER` 落点、脚下是 `axolotls_spawnable_on` 黏土，`checkSpawnObstruction` 只剩 `isUnobstructed`、不查液体）。海豚 10 血 / 1.2 移速 / 3 攻击，满氧气 4800、`handleAirSupply` 覆写成空（氧气走 `LivingEntity.baseTick`）、`increaseAirSupply` 直接回满；`SmoothSwimmingMoveControl` + `SmoothSwimmingLookControl` + `WaterBoundPathNavigation`（`allowBreaching = true`，唯一能跃出水面的水生生物）；`travelInWater` 是 `moveRelative` + `move(SELF)` + ×0.9，没有攻击目标时每刻下沉 0.005；`Dolphin.tick` 在 `super.tick()` 后管湿度 `Moistness`（2400，在水里 / 雨里回满，否则每刻减一、到 0 受 1 点 `dry_out`）、`isNoAi()` 把氧气回满、离水着地扑腾（两个 `nextFloat` 定水平速度、一个定 `yRot`、置 `needsSync`）。美西螈 14 血 / 1.0 移速 / 2 攻击 / 台阶高 1，满氧气 6000，`handleAirSupply` 在水里 / 雨里回满、否则 `preTick - 1`、到 -20 归零并受 2 点 `dry_out`，两栖寻路（`AmphibiousPathNavigation`）、`travelInWater` 没有重力项；五花色（0 lucy / 1 wild / 2 gold / 3 cyan / 4 blue 稀有），`finalizeSpawn` 从世界随机源抽两个常见花色作组数据、再抽一个给这一只，一组第三只起是幼年、`BUCKET` 原因直接返回；`hurtServer` 按官方短路判定置 `PLAY_DEAD_TICKS = 200`，`customServerAiStep` 映射成 `playingDead` 元数据，叫声水里 `idle_water` / 水外 `idle_air`、装死时不叫；装桶走 `Bucketable`（`item.bucket.fill_axolotl`，`bucket_entity_data` 存 `Variant` / `Age` / `AgeLocked`，有捕猎冷却时再存剩余刻 `HuntingCooldown`），倒桶 / 发射器放出 `FromBucket = true` 的美西螈；喂热带鱼桶按 `Axolotl.usePlayerItem` 交回一个水桶（生存模式扣掉桶里的鱼，创造模式手里不变）。海豚 `GotFish` / `Moistness`、美西螈 `Variant` / `FromBucket` 存盘并同步（`GOT_FISH`(18) / `MOISTNESS_LEVEL`(19)、`DATA_VARIANT`(18) / `DATA_PLAYING_DEAD`(19) / `FROM_BUCKET`(20)）；拿鱼右键海豚：幼年加速长大、成年置 `gotFish`，消费一条鱼。死亡按 `entities/dolphin` / `entities/axolotl` 掉战利品表，经验按 `Animal` / `AgeableWaterCreature.getBaseExperienceReward`（`1 + nextInt(3)`）。逐刻对拍：`mob-oracle --only dolphin` / `--only axolotl`（海豚 4 场景 804 刻、美西螈 4 场景 804 刻，两边设 `NoAI`、同一实体随机源种子，逐位比位置 / 速度 / 朝向 / 随机源 / 氧气 / 年龄 / 着地 / 在水里 / 各自元数据 / 尺寸 / 世界随机源，0 不一致；海豚场景还逐字比官方 `goalSelector` 注册表：每个目标的优先级、类名、标志位掩码，14 个目标与 `targetSelector` 的次序一致），`--break-self-test water_mob` 故意改坏 `gotFish` / `playingDead`、`--break-self-test dolphin_goals` 清空海豚目标注册表都能被抓到；自然生成夹具 `dolphin_ocean` / `axolotl_lush` 在 `mob-oracle --only natural_spawn` 里比类型、坐标、朝向、幼年、花色和随机源，0 不一致。已知限制：官方 `Dolphin.registerGoals` 的十四个目标（`BreathAirGoal`、`TryFindLiquidGoal`、引路去宝藏的 `DolphinSwimToTreasureGoal`、给玩家海豚恩惠 `DOLPHINS_GRACE` 的 `DolphinSwimWithPlayerGoal`、`RandomSwimmingGoal`、`RandomLookAroundGoal`、`LookAtPlayerGoal`、`DolphinJumpGoal`、`MeleeAttackGoal`、`MoveToItemGoal`、`PlayWithItemsGoal`、跟船 / 鹦鹉螺的 `FollowPlayerRiddenEntityGoal`、躲守卫者的 `AvoidEntityGoal`）和 `targetSelector` 的 `HurtByTargetGoal` 已建成真目标（`dolphin.rs` / `dolphin_goals.rs`，优先级、类名、标志位、注册次序照官方；`DolphinJumpGoal` 继承 `JumpGoal` 锁 MOVE|JUMP），不再是 `Unavailable` 占位。`canUse` / `canContinueToUse` / `start` / `tick` 按官方翻译，但 `findNearestMapStructure`、船 / 鹦鹉螺、掉落物实体、游泳玩家、`DOLPHINS_GRACE` 药水效果这些设施本仓库还没接，依赖它们的分支按 `fox_goals` 的既有做法保住取数次数与顺序后返回假，所以引路去宝藏 / 沉船、给玩家海豚恩惠、捡物仍不会发生；躲守卫者已换成真 `AvoidEntityGoal(Guardian, 8.0, 1.0, 1.0)`（`mob-oracle --only attack_target_dolphin_guardian` 逐刻比目标表中的 `avoiding` 状态）。**goalSelector 其余目标没有逐刻对拍**：`mob-oracle --only attack_target_dolphin_guardian` 用两刻真跑目标链，逐刻比 `AvoidEntityGoal` 是否在跑（两侧读各自的 `running` 标志）、位置 / 速度 / 血量 / 目标，并断言守卫者不把海豚设为目标（`GuardianAttackSelector` 只认玩家 / 鱿鱼 / 美西螈）；`water_mob` 的 NoAI 夹具只核对注册表与 NoAI 分支。真跑其它目标所需的 `WaterBoundPathNavigation` / `SmoothSwimmingMoveControl` / `DefaultRandomPos` 落点 / 标志位锁定保真没有逐刻验证；跃水（`DolphinJumpGoal`）与呼吸换气（`BreathAirGoal` / `TryFindLiquidGoal`）虽是真实逻辑，也未和官方逐刻比较。海豚在水里自由游动超过两刻后的竖直速度有 Java / Rust `Math` 末位差（浮力项），所以场景只取前两刻并记入已知限制。美西螈官方用 Brain（`AxolotlAi`）驱动、没有 `goalSelector`，Bamboo 现在按官方挂了大脑（`axolotl/ai.rs`）：五个传感器（`NearestLivingEntitySensor`、`AdultSensor`、`HurtBySensor`、`AxolotlAttackablesSensor`、`TemptingSensor`，顺序照 `BRAIN_PROVIDER`）和四个活动（`core` / `idle` / `fight` / `play_dead`，优先级、行为表、进入条件、退出时清的记忆照 `AxolotlAi`）；捕猎走 `AxolotlAttackablesSensor`（8 格内、在水里、`#axolotl_always_hostiles` 或没有 `HAS_HUNTING_COOLDOWN` 时的 `#axolotl_hunt_targets`）→ `StartAttacking` → `FIGHT`（`StopAttackingIfTargetInvalid`、`SetWalkTargetFromAttackTargetIfTargetOutOfReach`、`MeleeAttack(20)`、繁殖时清目标）→ 打中走 `Mob.doHurtTarget`；退出战斗时 `updateActivity` 记 2400 刻捕猎冷却；游泳走 `TryFindLiquid(6, 0.15, #axolotl_tries_to_find)` 和 `RandomStroll.swim(0.5)`，水陆速度按 `getSpeedModifier` 0.5 / 0.15、追击 0.6 / 0.15；装死按 `PlayDead`（200 刻，开始时清目标、给 200 刻等级 0 的 `REGENERATION`）和 `ValidatePlayDead.create`（每刻递减 `PLAY_DEAD_TICKS`，到 0 清记忆）在 Brain 里跑，`customServerAiStep` 尾部只把记忆映射成 `playingDead` 元数据。`mob-oracle --only axolotl` 的四个场景除 NoAI 逐刻外还逐字比大脑的结构表（传感器类名 / 扫描间隔 / 必需记忆、行为表的优先级 / 活动 / 类名与官方遍历顺序、活动进入条件、退出时清的记忆），`--break-self-test axolotl_brain` 加一个假传感器能抓到；**大脑的行为没有逐 tick 对拍**（夹具开 `NoAI`，Brain 不跑），捕猎 / 游泳 / 装死的运行时表现只有单元测试和结构表覆盖；繁殖（含 1/1200 蓝色）的产品花色抽取 `getBreedOffspring` 已按官方实现（稀有蓝时 `getRareSpawnVariant` 从单元素名单再抽一次，两条分支都恰好两次取数；`mob-oracle --only axolotl_breed` 43 例逐题比花色与取数后父亲随机源，0 不一致），`Axolotl.onStopAttacking` 给击杀者附近 20 格玩家加再生、`swingForAttack` 挥手动画、`TemptingSensor` 之外的捡物细节还没建模；刷怪蛋对着水面用、游泳粒子 / 动画、`SWIM` 游戏事件、`DOLPHINS_GRACE` 效果落地、死亡掉落的逐题 loot-oracle 都没做。
- **海龟**（分支 `op-water`，2026-11-21）：`Animal` 一支，可用 `/summon` 生成，也会自然生成（`CREATURE`：`ON_GROUND` 落点，脚下是沙子、亮度 > 8、海平面 +4 以下）。30 点血、0.25 速度、台阶高度 1；两栖寻路（`AmphibiousPathNavigation` + `AmphibiousNodeEvaluator`：水里也能走，接受上下两个水邻居，贴着 `BLOCKED` 的水算 `WATER_BORDER`）；`travelInWater` 推进 0.1、乘 0.9，没目标且不在回家附近时每刻下沉 0.005；`nextStep` 是 `moveDist + 0.15`。能在水下呼吸（`#can_breathe_under_water`），游泳声是 `entity.turtle.swim`（音量 ×1.5）。`TurtleMoveControl`：水里每刻上浮 0.005、离家 16 格外速度减半（不低于 0.08）、幼年再除以 3（不低于 0.06），地上速度减半（不低于 0.06），走向目标时速度按 0.125 插值并按高度差补竖直速度，不跳。目标按官方优先级：0 惊慌（先找 7 格内的水）、1 繁殖、1 产卵、2 诱惑、3 找水（`TurtleGoToWaterGoal`，24 格内、脚下两格那一层的水，陆地上的幼年总会找）、4 回家、7 远行（`TurtleTravelGoal`：在水里朝 ±512 格的随机远点一段段游，有远行目标时 `TurtlePathNavigation` 只把水格当落点）、8 看玩家、9 随机漫步（水里、回家途中、身上有蛋时不逛）。两栖寻路在水里按 `AmphibiousNodeEvaluator.getStart` 从包围盒最小角起步。家方块在 `finalizeSpawn` 和读档时记下（`home_pos`）；求爱成功不生成幼崽，母亲挂上 `has_egg`（同步 `HAS_EGG`，存 `has_egg`），6000 刻后不冷却；回家 9 格内、站在沙子上时按 `TurtleLayEggGoal` 放下 1–4 只海龟蛋（`HATCH` 从 0 起、音效音高取世界随机源）；蛋的随机刻按 `TURTLE_EGG_HATCH_CHANCE` 推进 `HATCH`，第三阶段孵出 `EGGS` 只带家方块的幼年海龟；长大成人时按 `entities/turtle_grow` 掉鳞甲。受伤 / 死亡音分幼年，叫声只在成年、水外、站在地上时发（其余不取音高随机数）。已知限制：远行目标检查落点周围 34 格的区块（`hasChunksAt`），Bamboo 的实体视图把所有区块都当作已加载；蛋在三更半夜的孵化概率按 `Timelines` 的 21062–21905 刻用 1.0，其余时段用 0.002，没有建模其它时间线属性；`TurtleEggBlock` 的 `stepOn` / `fallOn` 踩碎已实现（`stepOn` 100 分之一、`fallOn` 3 分之一，只有僵尸一系落地豁免、海龟不踩，`mob_griefing` 关掉时不踩，`--only zombie_turtle_egg` 8 例 3220 刻 0 不一致）；玩家破坏掉蛋（`playerDestroy` 的 `decreaseEggs`：一次只减一枚，没有精准采集时战利品表不掉东西）和 `getShape` 的蛋数形状没做。逐刻对拍：`mob-oracle --only zombie_turtle_ai`（沙滩找水两个种子、幼年、水下远行，共 1500 刻 0 不一致）；`--only zombie_turtle_egg`（带蛋产卵三个种子、离家 12 格回家，外加既有的僵尸踩蛋场景，共 8 例 3220 刻 0 不一致，逐刻比带蛋 / 正在产卵 / 产卵计数 / 回家标志 / 家方块和整张方块表散列，Java 侧有「确实产下蛋」的覆盖断言，`--break-self-test turtle_egg` 故意改坏抓到 12 处不一致）。孵化仍只有方块行为的单元测试：会生成实体的随机刻被 `oracle` 明确排除（`EntityRandomTick`），没有逐刻场景。
- 玩家手持弓右键拉弓，松手按 `getPowerForTime` 射出，满弓暴击；创造模式或无限附魔按官方 `useAmmo` 决定是否消耗箭，无限附魔只对普通箭生效，省下的箭标记为只能创造模式拾取。力量 / 冲击 / 火矢 / 穿透附魔和耐久损耗已接入。弩要先装填（`charged_projectiles` 组件，快速装填 II 缩短到 15 刻），装好后再次右键射出；多重射击一次三发、只消耗一支箭；副手 / 主手拿烟花火箭时也能装填并斜射出去（`shotAtAngle`，力度 1.6，每发扣 3 点耐久）。发射器对准箭、药水箭和光灵箭时生成真的箭实体并播放 1002。药水箭命中后按 `PotionContents.forEachEffect` 施加效果（`potion_duration_scale` 生效），光灵箭给 200 刻发光。弓和弩成功开火各计一次 `Stats.ITEM_USED`，客户端请求时回同步；一组多重射击仍只计一次。
- **箭和骷髅的已知限制**：没有自然刷怪、骷髅变体，或铁傀儡和小海龟的具体行为（骷髅会躲狼，狼也会追骷髅）。弩射烟花火箭已接入（见「烟花火箭」一节），`SHOT_CROSSBOW` 进度仍未触发。开火统计不写原版 `stats/<UUID>.json`，也不更新统计计分板目标；玩家用物标志已同步，但没有官方开始使用时立即同步装备的专门路径。弓继承已接受的客户端位移（在地面时去掉竖直分量）；弩不继承。跨维度和载具上的射手位移未对拍。快速装填音效标志按玩家使用状态存放，不复现官方 `CrossbowItem` 共享实例的多玩家交错。药水箭连续插地 600 刻后按 `Arrow.tick` 褪色：广播实体事件 0，拾取物变回普通箭，颜色回到 -1；`inGroundTime` 不进存档，读档后重新计数，和官方一样。药水箭的瞬间效果（治疗 / 伤害）对生物当场结算，对玩家走延后的 `Effect::PlayerEffect`；`Effect` 落地比官方 `addEffect` 晚一个实体阶段（同一刻内）。箭不触发标靶、木按钮、振动或命中特效；未建模的载具、盾牌弹反和组队/PVP 规则也不参与命中。箭支持只关闭重力、仍保留碰撞的 `NoGravity`，但未实现独立的 no-physics 飞行分支。水体熄火按整格水平范围近似流体形状，流动水可能和官方不同；目前只对箭和扔出的蛋做这条判定。未同步受击者身上插着的箭数量。骷髅生成按现实本地日期判断万圣节；同类武器重装备后仍不强制重建正在运行的目标。骷髅装备掉率的抢夺取致因玩家命中时的主手（包括玩家拥有的箭）；尚不支持无法在本岛解析的致因玩家及自定义掉落附魔效果。精确模式当场落地命中，并行模式用本刻快照取首批碰撞候选，穿透后的重查发生在命令落地时、看到的是落地时刻的实体表，目标在同一刻移动或失效时可能不同。
- **烟花火箭**：`FireworkRocketEntity` 实体已建模。对着方块右键在点击位置生成并扣一个物品；竖直发射的烟花每刻 x/z 乘 1.15、y 加 0.04 上升，斜射（弩 / 发射器）的烟花匀速直线飞，寿命 `lifetime = 10 * flight + nextInt(6) + nextInt(7)`（`flight` 是 `fireworks` 组件的飞行时长加一，无星为 1），第一刻放 `entity.firework_rocket.launch`，飞满寿命、命中实体或带星的撞到方块时 `explode`：广播实体事件 17（客户端粒子），带星（有 `firework_explosion`）时按 `5 + 星数 * 2` 点 `fireworks` 伤害类型伤 5 格内看得见的活体（`f * sqrt((5 - 距离) / 5)`，距离按 `Entity.distanceTo` 的 `f32` 计算，视线按 `COLLIDER` 射线检查，击退方向照 `calculateHorizontalHurtKnockbackDirection` 沿烟花指向目标的水平径向），附着的滑翔玩家先吃满额伤害。同步 `DATA_ID_FIREWORKS_ITEM` / `DATA_ATTACHED_TO_TARGET` / `DATA_SHOT_AT_ANGLE`，存档写 `Owner` / `LeftOwner` / `HasBeenShot` / `Life` / `LifeTime` / `FireworksItem` / `ShotAtAngle`。发射器照 `createDispenseConfig` 生成（`blockSource.center()` 外移 `0.5000099999997474`、力度 0.5、散布 1、事件 1004）。**滑翔加速**：对着空气右键（`FireworkRocketItem.use`）时若 `isFallFlying()` 就生成一枚附着在玩家身上的烟花（主人与 `DATA_ATTACHED_TO_TARGET` 都指向玩家、位置取玩家位置），扣一个物品（创造模式不扣）、记一次 `Stats.ITEM_USED`、返回 `SUCCESS`，不滑翔返回 `PASS`；滑翔时对着方块右键的 `useOn` 也按官方返回 `PASS`。附着烟花每刻按 `getHandHoldingItemAngle`（`calculateViewVector(0, yRot + (arm == RIGHT ? 80 : -80)) * 0.5`，索引 9 的 `OPTIONAL_UNSIGNED_INT` 同步给其它客户端）跟随玩家：`getMainArm()` 取 `client.main_hand`（左右手），`offhand = 副手是烟花 && 主手不是` 时取 `getOpposite()`，即右手主手 `+80°`、左手主手或只有副手拿烟花的右手玩家 `-80°`。按官方逐轴公式 `delta + look*0.1 + (look*1.5 - delta)*0.5` 给玩家加速度并把自己的速度同步成玩家的新速度，寿命到了照常爆炸。加速前先照官方顺序 `dropAllLeashConnections`（复用 `leash_knot::drop_all_leashed_to`，解开拴在玩家身上的生物），有解开的就播 `LEAD_BREAK`。服务端把这份速度写进玩家 `getDeltaMovement()`；玩家位移由客户端决定，但移动校验把它当 `expected` 读（官方判据 `moved - expected > 300 * packets`），客户端则靠同步的附着 id 在本地对同一个玩家施加同样的加速，两边才对得上。`mob-oracle --only firework` 覆盖竖直无星、带星炸牛、弩斜射撞墙、`firework_elytra` / `firework_elytra_left`（右手 / 左手玩家，逐刻比烟花位置 / 速度 / 朝向 / `life` / `lifetime` / 事件 17 / 玩家速度 / 随机源）五个场景与默认 / 带星两题存档往返（0 帧不一致；寿命、竖直加速、爆炸伤害、滑翔加速、附着偏移、主手判断六处 `--break-self-test` 都能抓到）。探针加一项：滑翔中右键烟花，其它客户端收到带附着 id 的实体元数据；单测覆盖滑翔生成、不滑翔不生成、创造不扣、使用统计、主手 / 副手 ±80° 四组合、解拴绳 + `LEAD_BREAK`，以及剪结 / 剪被拴生物 / 滑翔烟花三种路径各掉几根 `LEAD`。**官方 `dropLeash()` 已对齐**：每头被解开的生物按 `spawnAtLocation(ServerLevel, Items.LEAD)` 在自身位置、零偏移、默认拾取延迟掉一根 `LEAD`，清牵引数据、广播 `set_entity_link` 断链包并 `notifyLeasheeRemoved`；`removeLeash()` 与创造模式单绳解开不掉物品。`dropAllLeashConnections` 的三条路径（剪刀剪结、剪刀剪生物、滑翔烟花 `use`）都接上了 `GameEvent.SHEAR`：剪刀带玩家上下文，烟花按字节码发一条空源事件（任务书说 `null` 就不发，字节码没有 `player != null` 判断，按字节码实现）。单测分别数出剪结 2 根、剪被拴生物 2 根、滑翔烟花 1 根；`mob-oracle --only leash` 仍 0。**其余已知限制**：`CelebrateVillagersSurvivedRaid` 的庆祝烟花仍是只取随机数的占位，`Command::SpawnFireworkRocket` 是给 raid-1c 接真实生成的接口；`explode` 不发 `EXPLODE` 游戏事件（和 TNT / 苦力怕爆炸一致），发射音和爆炸声之外的音效、客户端粒子由客户端按事件 17 自己放；撞方块时 `state.entityInside` 的方块效果（营地火、炼药锅）没建模；`ProjectileDeflection`（盾牌弹反等）未接入。
- `/summon minecraft:rabbit` 和兔子刷怪蛋生成兔子：按群系选品种（雪地白兔/白斑兔、沙漠金兔，其余棕/盐/黑），用自己的移动和跳跃控制一跳一跳地走，躲玩家（8 格）、狼（10 格）和敌对生物（4 格），会爬出细雪、啃成熟胡萝卜（`mobGriefing`），吃胡萝卜/金胡萝卜/蒲公英繁殖，幼崽按 1/20 取群系品种、否则随机继承父母一方。存档写 `RabbitType`、`MoreCarrotTicks`，网络同步品种和跳跃事件。`mob-oracle --only rabbit_` 11 个场景逐刻 0 不一致，`loot-oracle --entity-table minecraft:entities/rabbit` 0 不一致。
- **兔子的已知限制**：杀手兔（品种 99）读写品种号，但不加护甲、攻击目标和名字，行为和普通兔子一样；「躲」的目标只认玩家、狼、僵尸/尸壳/骷髅（其余敌对生物还没有）；躲避取点用平台的 `sin`/`cos`，和 Java `Math.sin` 理论上可能差一位。
- `/summon minecraft:fox` 和狐狸刷怪蛋生成狐狸：`finalizeSpawn` 按群系定红狐/雪狐，20% 叼一件东西，并按品种加捕猎目标（和官方一样，直接构造或繁殖出来的狐狸要读盘后才会捕猎）。白天有遮挡、周围没人时睡觉，夜里会尖叫；躲玩家（16 格，潜行和信任的玩家除外）；盯梢兔子、蹲下、扑跳，扑进雪层会脸朝下栽进去；吃浆果丛和发光浆果、捡地上的东西（多的放回、旧的吐出去）、叼满 600 刻后吃掉吃的；坐着四处张望；喂甜浆果/发光浆果繁殖，幼崽信任两边的喂食者。存档写 `Trusted`、`Sleeping`、`Type`、`Sitting`、`Crouching`，网络同步品种、标志和两个信任格。`mob-oracle --only fox_` 11 个场景逐刻 0 不一致，`loot-oracle --entity-table minecraft:entities/fox` 0 不一致。
- **狐狸的已知限制**：没有村庄 POI，`StrollThroughVillageGoal` 只取随机数、`SeekShelterGoal` 的「在村里」恒假；保护信任对象（`DefendTrustedTargetGoal`）拿不到玩家的 `getLastHurtByMob`，只取随机数不触发；捕猎鱼、小海龟和躲避北极熊的目标只注册不命中（躲狼已接上，驯服的狼不躲；鸡已接上：狐狸会盯梢、扑跳和捕猎鸡，鸡也会让狐狸警觉，`fox_hunt_chicken` 场景逐刻对拍过）；吃东西只算 `Consumable` 的随机数、声音和减一个，不应用吃后效果（`on_consume_effects`）也不换 `use_remainder`；捡东西不发 `take_item_entity` 包、不记 `thrower`；刷怪蛋对着成年狐狸生幼崽时的信任没接；`Fox.FoxAlertableEntitiesSelector` 对生物只排除别的狐狸和已驯服的动物（生物的睡觉/潜行状态还没有）；对拍里兔子死后的掉落物 Java 一侧不进实体表，捕猎场景在狐狸去捡之前结束。生物捡东西（`Mob.aiStep` 那一段）只给狐狸实现，僵尸/骷髅的 `CanPickUpLoot` 仍不生效。
- 兔子和狐狸新增的公共机制：`entity/mob/avoid.rs`（`AvoidEntityGoal`，带狐狸的过滤条件）、`LeapAtTargetGoal`、`MeleeAttackGoal` 的 `followingTargetEvenIfNotSeen`、目标的 `isInterruptable`、能在细雪上走的生物（`powder_snow_walkable_mobs` 或皮革靴）的碰撞和攀爬、所有生物 `jumpFromGround` 的 `needsSync`。近战范围按官方 `getAttackBoundingBox` 只水平膨胀（以前竖直也膨胀了）。
- `/summon minecraft:wolf` 和狼刷怪蛋生成狼：`finalizeSpawn` 按 `wolf_variant` 的群系条件选品种（森林、草原、丛林、恶地等八种，其余苍白狼），七种叫声随机一种。野狼闲逛、讨食（盯着拿骨头或肉的玩家歪头）、追兔子和狐狸、打骷髅；被打后记仇 20–39 秒并呼叫附近同主人的狼一起上，仇恨按 `anger_end_time` / `angry_at` 存盘和同步。骨头驯服（狼自己的随机源 1/3，事件 7/6），驯服后最大生命 40、回满并坐下；主人空手右键切换坐下/起身；喂肉按营养值的两倍回血；染料换项圈色；离主人 10 格以上跟随、12 格以上瞬移到主人身边（恐慌时也会瞬移）；主人被打或主人打了谁，没坐着的狼去咬（玩家的 `lastHurtByMob` / `lastHurtMob` 由游戏层记录）；驯服的狼咬死的东西算主人的。两只驯服、发情、没坐着的狼繁殖，幼崽继承主人，项圈色按合成配方混色。淋雨或下水变湿，上岸后抖水（事件 8/56）。存档写 `Owner`、`Sitting`、`CollarColor`、`variant`、`sound_variant`、`anger_end_time`、`angry_at`，网络同步驯服标志、主人、讨食、项圈、仇恨结束时刻、品种和叫声（尾巴高度由客户端按同步的血量算）。`mob-oracle --only wolf_` 12 个场景逐刻 0 不一致，`loot-oracle --entity-table minecraft:entities/wolf` 0 不一致。
- **狼的已知限制**：狼铠已接上：主人右键给成年驯服的狼穿上（BODY 槽 + 必掉 + 装备声）、剪刀脱下、犰狳鳞甲修（`maxDamage * 0.125` = 8 点，创造模式也扣），染色走物品的 `dyed_color` 组件；`Wolf.actuallyHurt` 吸收伤害（扣 `ceil(amount)` 点耐久、不掉血、`Crackiness.WOLF_ARMOR` 四阶段开裂声 + 20 个带物品参数的 ITEM 粒子、破损事件 65），`hurtArmor` 只处理 BODY 槽，发射器也能给狼穿铠（Body 槽 6）。狼铠随通用 `equipment` / `drop_chances` 存盘、走 `set_equipment` 同步；`mob-oracle --only wolf_armor_` 3 个逐刻场景（穿铠 / 挨打开裂 / 鳞甲修）+ 2 处改坏 0 不一致。已知限制：破损的 `break_sound`、非吸收路径的 `hurtArmor(amount/4)` 未建模；`universalAnger` / `forgiveDeadPlayers` 规则 Bamboo 没有，按官方默认（关 / 开）的效果处理，`ResetUniversalAngerTargetGoal` 恒不启动，玩家死后不主动原谅；读盘时 `angry_at` 只记 UUID、不当场把它设成目标，旧版 `AngerTime` 不换算；按名字存的旧主人（`OldUsersConverter`）不转换；驯服的狼死亡时不给主人发死亡消息，`TAME_ANIMAL` 进度不触发；野狼会追羊（`wolf_hunt_sheep` 场景逐刻对拍过）；海龟、苦力怕、恶魂仍未实现；狼躲羊驼（`WolfAvoidEntityGoal` 按羊驼力量）、羊驼打野狼（`LlamaAttackWolfGoal` 四分之一跟随距离）和「保护主人时排除驯服马科」（`Wolf.wantsToAttack`）都已接入，但这三条没有专属逐刻对拍场景，只过回归场景；骑乘不存在，拴绳已接入（`unableToMoveToOwner` 现在也看 `mayBeLeashed`）；主人只按玩家找；玩家用弓箭命中时不记 `lastHurtMob`（只记近战），所以「主人射谁狼就咬谁」不触发；生物被玩家推开时只算生物那一半，推挤顺序按「先玩家后生物」，和官方实体分区的顺序可能不同；`CollarColor` 只认字节标签；两只狼繁殖没有逐刻对拍（夹具只比场景原有的生物，幼崽不在比对范围里），试做过的繁殖场景从第 162 刻起位置发散（官方停在同伴旁边、Bamboo 继续走了约 0.15 格），原因没查。
- 狼带来的公共机制：`entity/mob/tame.rs`（可驯服动物：驯服 / 坐下 / 跟随主人 / 保护主人 / 瞬移，猫和鹦鹉可以直接用）、游戏层 `player_combat_memory.rs`（玩家的 `lastHurtByMob` / `lastHurtMob`）、生物被重叠的玩家推开、兔子 / 狐狸 / 骷髅躲狼、动物同步攻击性位。
- **兔子、狐狸、狼的自然生成**：三种都接进了运行时自然刷怪和生成期刷怪。刷怪规则是各自的方块标签（`#rabbits_spawnable_on` / `#foxes_spawnable_on` / `#wolves_spawnable_on`）加原始亮度 > 8；狐狸的放置方式是 `NO_RESTRICTIONS`，狼一簇最多 8 只。`finalizeSpawn` 的组数据在 `entity/mob/wild_spawn.rs`：兔子一组共用第一只的品种、第二只起都是幼崽（`RabbitGroupData`，幼年概率 1.0，那一次 `nextFloat` 照取）；狐狸一组共用品种、第三只起是幼崽（`FoxGroupData`）；狼群共用品种、叫声各抽各的、不生幼崽（`WolfPackData`）。`MobType::by_entity_type` 现在认所有建模了的动物，所以猪、羊、鸡也真的会自然刷出来（之前只有牛）。`mob-oracle --only gen_spawn` 加了针叶林和雪地针叶林场景（7000 例：类型、坐标、朝向、幼年、左撇子、品种、狼的叫声、狐狸叼的物品，0 不一致；另要求牛、猪、羊、鸡都实际比到生成品种），`--only natural_spawn` 加了猪、羊、鸡和三种野生动物的场景（整组 252 例 1008 刻 0 不一致）。**已知限制**：品种的群系条件（兔子的两个群系标签、狐狸的雪地标签、狼的八条）是照官方手写的，数据包改了不会跟；对拍用的是公式地形和固定群系，没有拿官方真实世界逐区块比过。
- 马、驴、骡、羊驼（`entity/mob/equine*`）：`/summon` 和刷怪蛋按官方 `finalizeSpawn` 随机血量、速度、跳跃、花色和羊驼力量；繁殖按 `createOffspringAttribute` 遗传三项属性，马和驴生骡；吃草、甩尾、自然回血、扬蹄（`RandomStandGoal`、受伤 1/3 概率）、不爬梯子和藤蔓（`AbstractHorse.onClimbable` 恒假）、官方目标表；喂食加脾气、驯服后的发情，没驯服时拿东西右键发脾气，驴骡羊驼装箱子，鞍、马铠和羊驼地毯，幼马走 `interactOn` 的兜底也能套鞍；剪刀右键卸下穿戴的装备（`Mob.attemptToShearEquipment`，按 `EquipmentSlot.VALUES` 顺序先身体后鞍、在 `PASSENGER` 附着点均值处掉落、播组件里的 `shearing_sound`、生存扣 1 耐久并受绑定诅咒限制，潜行或被骑时跳过）；潜行右键驯服的个体打开坐骑物品栏（`mount_screen_open` + `HorseInventoryMenu` 的鞍、身体、箱子格和 Shift 点击路由，穿装备时照官方两次 `onEquipItem`）；马、驴、骡摔落超过 1 格播落地声；脚步按官方 `playStepSound` 取脚下方块的声音类型（上面盖着雪层算雪，木头是 `horse.step_wood`），被骑着的马按 `gallopSoundCounter` 在前五步后每三步播奔跑步声（每次用马自己的随机源判定 1/10 的喷气声），羊驼恒为 `llama.step`；羊驼被打后回击（`LlamaHurtByTargetGoal` + `RangedAttackGoal`），吐出 `llama_spit` 实体（飞行、命中扣 1 点、存档 NBT 和同步包按官方），吐一口就停；繁殖出生标记 `Bred` 的幼崽按 `followMommy` 给最近的同类算路径；死亡掉落战利品表、鞍、马铠、地毯、箱子和箱子里的东西；实体元数据、存档 NBT 都按官方。对拍：`mob-oracle --only equine`（生成、繁殖、含剪刀的 504 题交互、40 题死亡、12 题存档往返，共 684 题，含 `set_entity_data` 字节）、`--only zombie_equine`（22 个逐刻场景，含 4 个甩人、4 个当场驯服、以及羊驼近距锁定野狼 / 远距不锁定而野狼躲羊驼 2 个场景）和 `--only llama_spit`（2 个 160 刻的吐口水场景，逐刻比羊驼、口水和玩家血量，以及口水的同步包）；口水的存档在 `chunk-content` 里和官方逐键比。
- **马科的已知限制**：**骑乘只做了一部分**——右键成年个体上马、未驯服的马科会乱跑并按脾气判定驯服（失败则甩下骑手并加 5 点脾气）、潜行下马、下马时按官方 `getDismountLocationForPassenger`（含 `DismountHelper.canDismountTo` 世界边界检查）在载具两侧找落脚点、下车后 60 刻内不能再上（`boardingCooldown`）、`set_passengers` 同步、骑手位置跟随载具（含扬蹄 `standAnimO` 偏移）、载具位置由客户端的 `move_vehicle` 决定、`start_riding_jump` 的扬蹄和跳跃声、骑乘时按物品栏键打开坐骑物品栏（`OPEN_INVENTORY`）都在。载具位置按官方 `handleMoveVehicle` 校验：夹取、`wrapDegrees`、100 平方格的「移动太快」、`move(MoverType.PLAYER)` 的方块扫掠、和包里位置 0.25 格的 `moved wrongly` 容差，以及 `noCollision(oldAABB)` 的放行分支。没上鞍的载具能骑，但控制权不在客户端（`tickPlayer` 只在 `getControllingPassenger()` 是自己时才记 `lastVehicle`），位置由它自己的 AI 决定，`move_vehicle` 报的位置按「超过一格或离上次重置 20 刻」发回去。**没做**：`isEntityCollidingWithAnythingNew`（Bamboo 的实体之间没有碰撞，撞别的生物不拉回）；载具的摔落伤害和 `clientVehicleIsFloating` 检测；受客户端控制的载具走 `move_vehicle` 时未执行 `Entity.move` 的脚步声与移动振动；发射器给驯服马科装备鞍 / 马铠 / 地毯（`AbstractHorse.canDispenserEquipIntoSlot`，发射器目前只看玩家）；跳跃本身由客户端模拟，服务端只播声音和扬蹄（和官方服务端一致：官方 `START_RIDING_JUMP` 只调 `AbstractHorse.handleStartJump`，真正的冲量 `tickRidden`→`executeRidersJump` 因 `Player.isClientAuthoritative()` 恒真、`isLocalInstanceAuthoritative()` 在服务端为假而只在客户端算；`playerJumpPendingScale` / `allowStandSliding` / `getRiddenInput` 是客户端权威的内部量，Bamboo 服务端不建模，位置由 `move_vehicle` 提供，无玩家可感知差异）。坐骑物品栏的点击逻辑只有单元测试，没有和官方菜单逐题对拍；鞍位和身体位不看绑定诅咒（和玩家盔甲槽一样；剪刀卸装已按官方看绑定诅咒），交互距离按属性默认值加创造模式修饰算，不读玩家被命令改过的属性。马科不播方块的通用落地声（`SoundType.getFallSound`，Bamboo 的生物都还没有），只有自己的 `horse.land`。羊驼不组商队（`LlamaFollowCaravanGoal` 是不取数的占位）；行商羊驼继承了马科的这些限制，另外它的保护商人目标、不可骑乘和「被拴到别的东西就不消失」还没有逐刻对拍（见下行商羊驼一节）；`LlamaAttackWolfGoal` 和 `WolfAvoidEntityGoal` 已有专属逐刻对拍场景（`zombie_equine_llama_wolf_near` / `_far`），驯服狼保护主人时排除驯服马科（`Wolf.wantsToAttack`）只有单元测试；口水打到方块时不调方块的 `onProjectileHit`（和箭一样，标靶、钟、紫颂花不响应），着火口水每 20 刻的 `onFire` 受伤没建模（口水一碰非空气方块就消失，实际烧不起来）。幼崽拿金蒲公英只看交互那只手。装备音效的种子取自生物随机源的 `nextLong`，但发出的包用的是区域的种子源。存档里 `attributes` 列表的顺序和官方的哈希表顺序不同（读档不看顺序）。
- **鹦鹉螺 / 僵尸鹦鹉螺**：`/summon minecraft:nautilus` / `minecraft:zombie_nautilus`、两种刷怪蛋、海洋群系的 `nautilus` 刷怪条目（`IN_WATER` 放置、海平面下 5–25 格、下方是水、上方是水方块）、`entities/nautilus` / `entities/zombie_nautilus` 战利品表、`/summon` 和存档往返都接上了。属性照 `AbstractNautilus.createAttributes`（15 血、1.0 移速、3 攻击、0.3 击退抗性；僵尸版 1.1 移速），0.875×0.95、眼高 0.2751、乘客附着点 1.1375；水中用 `WaterBoundPathNavigation` + `SmoothSwimmingMoveControl(85, 10, 0.011F, 0, true)`，`travelInWater` 是 `moveRelative(getSpeed())` + `move(SELF)` + ×0.9，`handleAirSupply` 水里回满、离水每刻掉一、到 -20 受 2 点干死伤害。大脑 `NautilusAi` / `ZombieNautilusAi` 有五个传感器、`core` / `idle` / `fight` 三个活动；鹦鹉螺有 `AnimalPanic(1.6)` 和 `AnimalMakeLove`，僵尸版没有；两者共用 `FollowTemptation`（1.3 / 0.9）、`RandomStroll.swim(1)`、`SetWalkTargetFromLookTarget(1,3)` 和四条进入条件组成的 `FIGHT`。驯服用 `#nautilus_taming_items`（河豚 / 河豚桶，1/3 概率，事件 7/6），繁殖 / 幼年用 `#nautilus_food`（含桶装鱼，喂完交回水桶），没驯服或幼年才能吃；驯服成年、活着时鞍位和 `BODY` 位才可用，鞍 / 五级鹦鹉螺铠甲可由交互、菜单或发射器穿上，潜行右键开 `NautilusInventoryMenu`（两格、没有箱子格），上鞍后玩家能骑，冲刺有 40 刻冷却和 `DASH` 元数据。僵尸鹦鹉螺按 `#spawns_coral_variant_zombie_nautilus`（暖水海洋）选 warm / temperate 变种，存 `variant` 键、同步 `ZOMBIE_NAUTILUS_VARIANT`；不能生幼崽。`Drowned.finalizeSpawn` 的自然 / 结构生成分支现在真的排出一只 `ZombieNautilus(JOCKEY)`，照官方把父原因传进 `finalizeSpawn`、按群系标签定变种、结构生成标持久，再由通用乘客名单让溺尸骑上去。对拍：`mob-oracle --only nautilus_` 5 个逐刻场景共 525 刻（游动、大脑结构表、驯服、上鞍、静止骑乘），六处改坏确认（`nautilus_brain` / `tame` / `saddle` / `variant` / `species` / `jockey`）；`chunk-content java` 两题存档往返含主人 / 装备 / `Brain` / 变种；`menu java` 十二题 `NautilusInventoryMenu` 点击；`natural-spawn java` 覆盖 329 条海洋刷怪表里的 `nautilus` 条目。**已知限制**：`ChargeAttack` 只按官方类名占行为表的位置，服务端没有冲撞选目标 / 伤害逻辑，`StartAttacking` 只粗略挑水里的 `#nautilus_hostiles`；`Nautilus.getBreedOffspring` 继承主人只在单元测试里覆盖，没有逐刻繁殖对拍；冲刺（`onPlayerJump` / `executeRidersJump`）、气泡粒子、`BREATH_OF_THE_NAUTILUS` 给骑手补氧气、`getRiddenSpeed` 与 `getRiddenInput` 的客户端权威移动、发射器装鞍 / 铠甲、僵尸鹦鹉螺的日晒保护槽、以及真实海洋群系的混合刷怪产率都没有逐刻对拍（骑乘场景显式对齐生成之后的随机源，也不比生成期那一长串世界随机源取数）。
- **嘎枝 / 嘎枝之心**：`/summon minecraft:creaking` 和嘎枝刷怪蛋；属性照 `Creaking.createAttributes`（1 血、0.4 移速、3 攻击、32 跟随、1.0625 阶高），0.9×2.7、眼高 2.3、经验 0。嘎枝之心是新的方块实体：夜里（`CREAKING_ACTIVE` 环境属性，12600–23401 刻）变 `awake`、白天变 `dormant`，两头缺苍白橡木且没绑嘎枝时 `uprooted`；夜里 32 格内有玩家、非和平、`spawnMobs` / `spawnMonsters` 规则开着时按 `SpawnUtil.trySpawnMob` 的 5 次 16×8×8 取点在附近落点刷一只嘎枝并记下它的 UUID，白天或离心情超过 34 格时拆掉。嘎枝照官方 `CreakingAi` 跑完整大脑：`NEAREST_LIVING_ENTITIES` / `NEAREST_PLAYERS` 两个传感器（照官方给扫描间隔与起始延迟）、`CORE`（`Swim` / `LookAtTargetSink` / `MoveToTargetSink`）、`IDLE`（`StartAttacking` / `SetEntityLookTargetSometimes` / `RunOne[RandomStroll, SetWalkTargetFromLookTarget, DoNothing]`）、`FIGHT`（`SetWalkTargetFromAttackTargetIfTargetOutOfReach` / `MeleeAttack(canMove, 40)` / `StopAttackingIfTargetInvalid`），`customServerAiStep` 里 `updateActivity` 按 `canMove` 切活动。被玩家看着（`isLookingAtMe` 三个取样点、12 格）时 `checkCanMove` 读大脑 `NEAREST_PLAYERS` 记忆冻结不能动、发出 `entity.creaking.freeze` / `activate` 声，没人看时切 `FIGHT` 追着玩家走过去打 3 点近战（`attackAnimationRemainingTicks = 15`、事件 4、40 刻攻击冷却），转头看它就停住。受伤不掉血：`invulnerabilityAnimationRemainingTicks = 8`、事件 66、`entity.creaking.sway`，改由心 `creakingHurt` 沿苍白橡木 BFS 两层放 2–3 团树脂（空气 / 水源 / 缺面的树脂团补 `opposite` 面）并起 100 刻的连线 / 心跳效果。`removeProtector` 两条路径分开：心被非玩家移除（活塞 / 爆炸 / `setblock`，`preRemoveSideEffects → removeProtector(null) → tearDown()`）当场移除嘎枝，玩家破坏（`playerWillDestroy → removeProtector(damageSource)`）走 45 刻拆解（`setTearingDown` + `setHealth(0)` → 死亡声 + 碎裂粒子）；心被破坏掉 `NATURAL` 的 20–24 经验。比较器信号 `15 - floor(d/32*15)`。`CAN_MOVE`(16) / `IS_ACTIVE`(17) / `IS_TEARING_DOWN`(18) / `HOME_POS`(19) 元数据同步，`home_pos`（方块坐标）和心的 `creaking`（int-array UUID）存档。对拍：`mob-oracle --only creaking` 7 个逐刻场景（夜里刷出、被看着冻结、不看解冻、受伤放树脂、心被玩家破坏拆解、没人看时追击攻击 / 转头看时停住、心被活塞 / 爆炸非玩家移除当场消失），逐刻比状态 / 绑定 / 冻结 / 激活 / 拆解 / 树脂 / 玩家生命（20→17 证明走进近战范围并攻击）/ 挥击动画（证明 40 刻冷却），逐刻按原始位严格比位置（x/z）、生命、动画和所有离散字段，除 12 处已知三角函数表末位差外 0 不一致，没有任何放宽窗口：`IDLE` / `FIGHT` 的优先级照官方 `ActivityData.create` 从 10 起（早先错写成 0 会让 `MoveToTargetSink` 比官方早一刻起步），修好后两条寻路链同一刻起步。`--break-self-test creaking_freeze` / `creaking_bound` / `creaking_resin` / `creaking_attack` / `creaking_cooldown` / `creaking_active` / `creaking_position` 七处改坏确认都报不一致；临时把攻击冷却改 1、把 `checkCanMove` 改成逐刻读玩家两处引擎改坏也都会报不一致。`chunk-content java` 的心裸 / 带绑定 UUID 两题存档往返全过。**已知限制**：Java 与 Bamboo 两边的大脑传感器起始延迟都定成 1、嘎枝随机源定成 `0x5EED_5EEDL`、闸门洗牌按同一 SplitMix64 种子重设（夹具重放官方 `Brain.Provider.makeBrain` 的两个 `nextInt(20)` 构造取数）以消除对拍里的随机漂移，真实世界里传感器扫描间隔 / 起始延迟 / 随机源各不相同；刷怪蛋 / 命令生成的嘎枝不绑心（`home_pos` 空），和官方一致但没有逐刻对拍；`creakingHeartHurt` 的连线 / 心跳粒子和 `emitParticles` 的拖尾粒子不发（服务端只保随机取数）。

- **大脑 AI**（`entity/mob/brain/`）：官方 `Brain` 的 Rust 版，给不用 `GoalSelector` 的新生物用。记忆带可选过期时间（`ExpirableValue`），没登记的记忆读作没有；传感器按 `scanRate` 扫描、起始延迟随机；活动带进入条件和退出时要清的记忆，`setActiveActivityToFirstValid` 换活动；行为有 `Behavior`（时长取世界随机源）、`BehaviorBuilder` 拼的一次性触发、`DoNothing`、`GateBehavior` / `RunOne` 四种。一刻顺序照 `Brain.tick`（记忆过期 → 传感器 → 按优先级启动 → 推进正在跑的），行为排列复刻官方 `TreeMap<优先级, HashMap<活动, LinkedHashSet>>` 的遍历顺序；大脑在 `customServerAiStep` 里跑，走目标选择器的生物不受影响。记忆按 `Brain.Packed` 的格式存盘（只存有编解码器的）。`mob-oracle` 逐刻比活动、正在跑的行为和全部记忆。
- **犰狳**：`/summon`、刷怪蛋、自然刷怪和生成期刷怪（`#armadillo_spawnable_on`）；恐慌、被疾跑的玩家、亡灵或上次打它的生物吓到缩成球（`DANGER_DETECTED_RECENTLY`，偶尔窥视；官方骑乘中的玩家也算，Bamboo 没有骑乘）、缩着挨打伤害按 `(伤害 − 1) / 2` 算、跟随蜘蛛眼诱惑、繁殖、幼崽跟大人、闲逛和环顾；定时掉鳞甲，刷子刷下鳞甲（扣刷子耐久）；状态同步、`state` / `scute_time` 存盘。`mob-oracle --only armadillo_` 6 个场景逐刻 0 不一致。
- **山羊**：`/summon`、刷怪蛋、自然刷怪和生成期刷怪（`#goats_spawnable_on`）；`finalizeSpawn` 用世界随机源（生成期用区域随机源）定长跳和冲撞冷却、2% 尖叫山羊、成年 10% 少一只角；核心、闲逛、长跳、冲撞四个活动：长跳在周围 11×11×11 里按距离平方加权抽落点（走得到的不跳）、四个角度洗牌、逐段检查碰撞、起跳 40 刻后跳出、落地减速；冲撞找最近的可冲撞目标（不是山羊、在世界边界内、能当敌人、看得见），在目标东南西北 4–7 格找走得到的起跑点、低头 20 刻后以 3 倍速冲过去，撞到活体打 `mob_attack_no_aggro` 伤害再按速度和药水击退（玩家也会被撞开），撞到 `#snaps_goat_horn` 方块掉一只角（角的乐器按 UUID 的 `hashCode` 选）；空桶挤奶，小麦繁殖（幼崽 2% 或随父母一方尖叫），摔落伤害少 10 点，长跳姿势同步并缩小碰撞箱；三个同步布尔值和 `IsScreamingGoat` / `HasLeftHorn` / `HasRightHorn` 存盘（`chunk-content` 重存对拍过，含大脑记忆）。`mob-oracle --only goat_` 7 个场景逐刻 0 不一致（闲逛、石台长跳、撞牛、撞玩家、诱惑和挤奶、撞墙掉角）。
- **青蛙**：`/summon`、刷怪蛋、自然刷怪和生成期放置（`#frogs_spawnable_on`，亮度够）；品种按群系标签选（`#spawns_warm_variant_frogs` 暖、`#spawns_cold_variant_frogs` 冷，其余温带），`finalizeSpawn` 用世界随机源抽长跳冷却。`FrogAi` 的六个活动：闲逛（漫步、蹲叫 60 刻）、游泳（`SmoothSwimmingMoveControl`，`TryFindLand` 按曼哈顿距离找岸）、长跳（`LongJumpToPreferredBlock`：5×9×9 里加权抽落点，一半的概率优先睡莲和大型垂滴叶，活板门和流体按 `FrogAi.isAcceptableLandingSpot` 判）、伸舌头（只吃大小 1 的史莱姆类；走近到 1.75 格内把猎物拉过来，6 刻后 `doHurtTarget`，打死当场移除；走不到的记进 `unreachable_tongue_targets` 100 刻）、产卵（两只喂粘液球后其中一只怀孕，走到水边在水面放蛙卵）和惊慌。寻路是 `Frog.FrogPathNavigation`（两栖、睡莲上面算 `OPEN`、深水加代价、水边不抄近路）；水里用 `Frog.travelInWater`，摔落伤害少 5 点，不吃水流，能在水下呼吸。青蛙 `canBeABaby` 为假：组里后来的成员照样可能被 `setAge(-24000)`，但不算幼年、年龄不涨（存档也不写 `Age`），这一只在重新载入前喂不出求爱——官方就是这样。品种、姿势（蹲叫 / 长跳 / 伸舌头）和舌头目标同步给客户端，`variant` 存盘（`chunk-content` 重存对拍过，含 `is_pregnant` / `is_in_water` 记忆）。`mob-oracle --only frog_` 16 个场景逐刻 0 不一致（平地、睡莲池塘、石台长跳、水池上岸、开阔水域、吃史莱姆、玻璃罩里够不到的史莱姆、繁殖产卵、挨打和摔落、负年龄）；自然刷怪 `frog_day` / `frog_warm` / `frog_dim` 连品种和冷却一起比。
- **蝌蚪**：`/summon`、刷怪蛋、蛙卵孵化（计划刻到点方块碎掉，孵出 2–5 只不会消失的蝌蚪）和蝌蚪桶。它是 `AbstractFish`：鱼的惊慌、躲玩家（`AvoidEntityGoal`）、随机游泳三个目标和 `TadpoleAi` 的大脑（跟随粘液球、游泳漫步）一起跑，离水扑腾、掉氧气；移动和看的控制是 `SmoothSwimmingMoveControl` / `SmoothSwimmingLookControl`。喂粘液球把剩余时间缩掉一成，金蒲公英锁住 / 解开年龄；年龄到 24000 刻变成青蛙（`convertTo`，按所在群系定品种，新青蛙不会消失）。`Age` / `AgeLocked` 存盘，`FromBucket` 恒写真，桶里带年龄。`mob-oracle --only tadpole_` 8 个场景逐刻 0 不一致（游泳、引诱加喂食和锁年龄、挨打、岸上扑腾、水里和岸上长成青蛙、装桶）。
- **青蛙和蝌蚪的已知限制**：岩浆怪还没有，所以青蛙吃岩浆怪掉蛙明灯这条走不到（史莱姆掉一个粘液球的条件靠伤害来源带的攻击者类型，没有逐刻对拍掉落物）。长跳向量里的 `Math.sin` / `Math.cos` 用正确舍入的值：JVM 的英特尔桩代码大约每 800 次有一次差一个末位（`mc-bamboo-core` 的 `jmath`，`Math.atan2` 是 fdlibm 的逐位移植）。蛙卵被下落的方块砸碎（`entityInside`）没做；孵化没有接方块行为对拍（`Rule::TimerTick`），只有单元测试。长成青蛙之后的那只青蛙不在对拍夹具里（只比变的那一刻的移除、世界随机源和大脑）。青蛙 `age` 是负数时客户端同步的 `DATA_BABY_ID` 官方为真、Bamboo 为假（客户端对青蛙不看这一位）。行为放的声音（`level.playSound(null, body, ...)`）音源类别统一按中立发，官方产卵声是 `BLOCKS`。骆驼还没做。
- **大脑生物的已知限制**：`RunOne` 和山羊长跳角度的洗牌在官方用不带种子的随机源（`ShufflingList` 的 `generateUniqueSeed`、`Collections` 的静态 `Random`），Bamboo 按出生时生物随机源的状态和序号算种子，对拍两边按同一公式重设；传感器起始延迟官方取 `Sensor` 的静态随机源，Bamboo 取不到同一串数，对拍按官方报的值设。官方几个静态 `TargetingConditions` 的距离跟着「最后一个扫描的传感器」的跟随距离变，Bamboo 每个生物用自己的跟随距离。并行（性能）模式下世界随机源读作 0：行为时长取最短、长跳按列表顺序抽落点。山羊冲撞的目标检测把玩家排在生物后面（官方按实体分区的顺序）；盾牌格挡减半（`applyItemBlocking`）和被撞者的 `doPostAttackEffects`（荆棘等）没做；长跳的 `noCollision` 不看船、潜影贝这类硬碰撞实体（Bamboo 还没有）；盔甲架已建模，山羊长跳目标按官方排除它。`Goat.setYHeadRot` 的 ±15 度夹角只在读档时做（传送没做）。山羊繁殖没有逐刻对拍（夹具只比场景原有的生物）。
- **拴绳**：`Leashable` 已接入——手持拴绳右键可拴的生物拴到玩家（创造模式不扣拴绳）、右键已拴在自己身上的生物解开并掉一根拴绳、剪刀剪断牵着的和牵着这个生物的所有绳子（扣剪刀 1 耐久）、拴绳结实体（`leash_knot`）挂到栅栏上（手持拴绳或空手右键栅栏走 `LeadItem.bindPlayerMobs`）、空手右键结把结上的生物转回玩家（`LeashFenceKnotEntity.interact` 的 knot→player 方向，已接到游戏层 `on_interact_entity`）、每刻 `whenLeashedTo` 把家设到牵引者附近（半径 5）、近距离按 `PathfinderMob.closeRangeLeashBehaviour` 打开 `MOVE` 控制位并导航到离牵引者 2 格处（玩家牵动物走时动物自己跟着走，弹性拉扯只是兜底；`Llama.followLeashSpeed=2`、`shouldStayCloseToLeashHolder` 除悦灵 / 快乐恶魂恒真，马科 / 骆驼 `onElasticLeashPull` 停下吃草 / 从坐姿站起）、以及按距离弹性拉扯 / 断裂（断裂距离 12、弹性距离 6，角动量和按轴弹性照官方字节码逐位对齐）、牵引者下线 / 死亡时解绳、`notifyLeasheeRemoved`（绳子全断结消失）、栅栏被拆时结消失并放 `item.lead.untied`、`leash` 键存档（实体 UUID 或结的位置）并读档重连、`set_entity_link` 拴上 / 解开 / 进视野时照官方发。`canBeLeashed` 照各生物的官方覆盖（敌对生物大多不能拴，狼看愤怒、鹦鹉螺看 `isAggravated`、硫磺方块怪看有没有身体物品、海龟 / 鱼 / 蝙蝠 / 熊猫 / 村民不能拴）。对拍：`mob-oracle --only leash` 八个逐刻场景共 648 刻，逐刻比牛的位置 / 速度 / 四个朝向 / 随机源 / 绳状态（牵引者种类、角动量）/ 掉落 / 结的存活。五个 `NoAI` 场景（拴玩家走远的弹性段 + 超距断绳、拴到栅栏、从栅栏解回、剪刀剪绳、拆栅栏结被打掉）只比拴绳物理；三个开着 AI 的场景 `leash_ai_follow` / `leash_ai_stop` / `leash_ai_snap`（玩家牵牛走、牛跟着走、玩家停下牛停在附近、超距断绳）另外逐刻比导航路径和正在跑的目标类名——`leash_ai_follow` 里官方牛自己从 x=0.5 走到 x=6.4，78 刻有路径、35 刻有运行目标，Bamboo 全对上，0 不一致（只剩这台 ARM Mac 的 `Mth.COS_TAB` / `ASIN_TAB` 平台末位差，与所有场景共有）。逐刻对拍发现并修正了弹簧力方向、服务端玩家 `getKnownMovement` 恒零、dampening 用被拴生物自己的 movement、`setLeashedTo` 保留角动量、断绳帧仍应用 `yRot -= angularMomentum` 五处物理偏差，并据此补齐了 `closeRangeLeashBehaviour` 让牛自己跟着走。`--break-self-test leash_attach` / `leash_motion` / `leash_holder` / `leash_nav` 四处改坏都能抓到不一致（第四处让 `closeRangeLeashBehaviour` 不调导航，三个 AI 场景各报 78 刻差异）；`chunk-content` 加了实体 / 结两种 `leash` 存档形态和 Bamboo 生成写出的往返题目（官方 `load -> save` 与 Bamboo `read -> write` 逐键比），`player-data` 回归无破坏。**已知限制**：快乐恶魂现在已建模（见「快乐恶魂和干恶魂方块」一节）：`shouldStayCloseToLeashHolder=false`、弹性 / 断裂距离覆盖、holder 四点附着点和四点弹簧合力已接入，但 `Leashable.supportQuadLeash()` 官方只有船覆盖，船虽已建模、快乐恶魂作为被拴方仍返回 `SUPPORTS_QUAD_AS_LEASHED = false`，所以「快乐恶魂牵引别的生物」在 26.3 里仍走单点弹簧；船作为被拴方的四点弹簧合力已接入，两点之间的船四点拴绳没有单独逐刻对拍；`CamelHusk` / `ZombieHorse` 的 `canBeLeashed` 覆盖按官方看 `isMobControlled()`（第一名乘客是生物），生物乘客已建模但没有为这两个覆盖单独对拍；鹦鹉螺 `isAggravated()` 官方看 `ANGRY_AT` 或 `ATTACK_TARGET` 大脑记忆，Bamboo 没建模 `ANGRY_AT`，只认 `ATTACK_TARGET`；拴绳结被打掉的 `dropItem` 在 26.3 只放音效、不掉拴绳，与官方一致。
- **嗅探兽**：`/summon minecraft:sniffer` 和刷怪蛋；14 血、0.1 速，成年 1.9×1.75（眼高 1.05）、幼年减半，挖掘时换成 1.35 高的矮尺寸。照 26.3 `SnifferAi` 接四个活动（core / idle / sniff / dig）和六个专属行为（`Sniffing` / `Searching` / `Digging` / `FinishedDigging` / `Scenting` / `FeelingHappy`）：闲逛、被诱惑、恐慌，成年且没诱惑 / 恐慌 / 求爱 / 在水里时嗅闻，用 `LandRandomPos` 五档落点找 `#sniffer_diggable_block` 且走得到的位置，走过去（SEARCHING）→ 挖（DIGGING，160–180 刻）→ 起身（RISING）→ 按 `gameplay/sniffer_digging` 礼物表掉火把花种子或瓶子草荚，把位置写进 `SNIFFER_EXPLORED_POSITIONS`（最多 20 个）并进入 9600 刻 `SNIFF_COOLDOWN`，挖过的位置不再挖。喂 `#sniffer_food`（火把花种子）让两只求爱，繁殖不生幼崽而是下一颗嗅探兽蛋（掉落物）加经验球。`DATA_STATE`（index 18、35 号 `SNIFFER_STATE` 序列化器）和 `DATA_DROP_SEED_AT_TICK`（index 19、INT）同步给客户端，动画靠它们驱动；`SNIFFER_EXPLORED_POSITIONS` / `SNIFF_COOLDOWN` 存档。`SnifferEggBlock` 计划刻分三阶段孵化，普通方块 24000 刻、底下是苔藓块（`#sniffer_egg_hatch_boost`）12000 刻，第三阶段拿掉方块孵出一只幼年嗅探兽。对拍：`mob-oracle --only sniffer_` 的 `sniffer_dig_23/37/53/101`（完整挖掘闭环加掉种子，逐刻比位置 / state / 记忆 / 世界战利品随机序列 / 掉落物）、`sniffer_wander_11`（闲逛对照）、`sniffer_breed`（产蛋加经验球），除 12 处已知三角函数表外 0 不一致；`chunk-content java` 的带 `Brain` 记忆重存和 fresh sniffer 用例（583 checks 全过）；三处 `--break-self-test sniffer_state` / `sniffer_brain` / `sniffer_drop` 都报不一致；另有蛋孵化三阶段 / 苔藓加速、记忆往返、挖过不再挖的 Rust 单测。**已知限制**：拴绳没建模，`isLeashed` 恒假（另一分支在做），`canSniff` 里那条判断恒真；嗅探兽蛋从温暖海洋遗迹的可疑沙子靠刷子考古获得这条来源不在本任务；蛋孵化是方块计划刻，用 Rust 单测覆盖而非 `mob-oracle` 逐刻对拍；挖掘粒子和搜寻音是客户端效果，服务端只对齐状态、取数和声音事件。

- **快乐恶魂和干恶魂方块**：`/summon minecraft:happy_ghast`、刷怪蛋，以及泡水的 `dried_ghast` 方块孵化。干恶魂方块照 `DriedGhastBlock` 走含水（`WATERLOGGED`）、四级 `hydration`、`FACING` 和 5000 刻计划刻：含水时每次计划刻放 `block.dried_ghast.transition` 并升一级、发 `BLOCK_CHANGE` 游戏事件，吸满（3）时拿掉方块、在底心孵出一只朝 `FACING` 的幼年快乐恶魂（年龄 -24000）并放 `entity.ghastling.spawn`；脱水的方块退一级（无声音）。`randomTick` 只在干燥或者已经吸水时排计划刻，含水且水合 0 的方块由 `updateShape` 排的流体刻推进。快乐恶魂 20 血、`FLYING_SPEED=0.05`、跟随 / 诱惑距离 16、相机距离 8，成年 4×4（眼高 2.6）、幼年 `scale(0.2375)` → 0.95×0.95（眼高 0.46875），幼年两套 `entity.ghastling.*` 声音 / 成年 `entity.happy_ghast.*`，音高恒 1、音量幼年 1 / 成年 4，没有脚步声也没有摔落伤害，不能求爱 / 繁殖。幼年大脑照 `HappyGhastAi` 接五传感器（`AdultSensorAnyType` 只认 `#followable_friendly_mobs` 的成年个体）和三张活动表（CORE 的 Swim / AnimalPanic / LookAtTargetSink / MoveToTargetSink / CountDownCooldownTicks，IDLE 的 FollowTemptation（`lookInTheEyes=true`，走路目标取玩家眼睛那一格）/ 两种 BabyFollowAdult / RunOne{RandomStroll.fly, SetWalkTargetFromLookTarget}，PANIC 空表且以 `IS_PANICKING` 为入口条件）；喂 `#happy_ghast_food`（雪球）按 `AgeableMob.ageUp` 加速长大。幼年用 `FlyingMoveControl(this, 180, true)` 和 `BabyFlyingPathNavigation`（不开门、可浮水、路径长 48），成年 `adultGhastSetup` 换成 `GhastMoveControl(this, true, isOnStillTimeout)`、`HappyGhastLookControl`（折角 / 直接写朝向 / 对着水平速度）、普通寻路器和三个目标（优先级 3 悬停、4 `TemptGoal.ForNonPathfinders`、5 `Ghast.RandomFloatAroundGoal`，其 `isGoodTarget` 要求目标格空气且六方向 16 格内有方块），并停大脑清记忆；`travel` 走 `travelFlying(FLYING_SPEED × 5/3)`（无重力、速度整体 ×0.91），长大跨尺寸时按 `fudgePositionAfterSizeChange` 把脚底对齐。成年 BODY 槽可穿十六色挽具（`equippable`：穿上 / 剪刀脱下各带装备音、脱下掉回原物品并扣剪刀耐久），十六种挽具染色配方（任意非目标色挽具 + 对应染料）和发射器给成年快乐恶魂穿挽具都已接好（幼年 `canUseSlot(BODY)` 为假）；空手右键穿了挽具的成年恶魂上车；`HappyGhast.canAddPassenger` 放宽到四名，四个 `PASSENGER` 附着点 `(0,4,1.7)/(-1.7,4,0)/(0,4,-1.7)/(1.7,4,0)` 按乘客下标 clamp，第一名玩家乘客是控制者（穿了挽具且不在悬停超时），`tickRidden` 把朝向按 0.08 插值并抄到头 / 身，`getRiddenInput` 用 `FLYING_SPEED × 3.9` 和玩家俯仰 / 跳跃算飞行向量，下车点固定 `(x, bb.maxY, z)`。服务端对玩家骑着的载具只维护朝向 / 悬停和乘客座位，**位置由客户端权威**：`Entity.getMoveSimulationType()` 对玩家是 `AUTHORITATIVE_SIDE_AND_SERVER`，载具 `isClientAuthoritative()` 因控制者是玩家而为真，`canSimulateMovement()` 在服务端为假，`travelRidden` 只跑 `tickRidden` 再把速度清零；玩家报上来的位置走 `ServerboundMoveVehiclePacket` / `on_move_vehicle`。头顶有玩家时 `serverStillTimeout` 设 10 刻、同步 `STAYS_STILL`（index 19 BOOLEAN），`IS_LEASH_HOLDER`（index 18 BOOLEAN）随 `leashHolderTime`；`still_timeout` 无条件存档；不满血时在云层（`isInClouds`：云色 alpha 非零且实体跨过 `[192,196]`）或降水群系每 20 刻、否则每 600 刻回 1 血，`checkRestriction` 按「成年且 BODY 槽空着」把家半径设 64，其余 32。拴绳覆盖 `leashElasticDistance=10` / `leashSnapDistance=16`、`shouldStayCloseToLeashHolder=false`、`supportQuadLeashAsHolder=true`（`getQuadLeashHolderOffsets` 的四角按宽 / 高公式）、`getLeashOffset=ZERO`、`onElasticLeashPull` 停移动控制；`Leashable.createQuadLeashOffsets` 和共享四角附着点、四点弹簧合力 ×0.25 也接进了拴绳物理。对拍：`mob-oracle --only happy_ghast` 六个 `NoAI` 逐刻场景（孵化 7 刻、喂雪球 61 刻、穿 / 脱挽具 41 刻、四座骑乘与朝向 61 刻、四点拴绳拉扯 41 刻、存档往返）逐刻比位置 / 朝向 / 血量 / 年龄 / 尺寸 / BODY 槽 / 乘客数 / 悬停超时 / 两项布尔元数据 / 大脑快照 / 随机源 / 干恶魂方块是否移除 / 被拴牛的位置与绳状态，并逐字比大脑结构表；另有五个**开着 AI** 的逐刻场景 `happy_ghast_ai_adult`（成年自由飘浮 121 刻）、`happy_ghast_ai_baby`（拿雪球的玩家 + 大脑 + 飞行寻路 121 刻）、`happy_ghast_ai_grow`（长大那一刻换控制 / 目标 81 刻）、`happy_ghast_ai_still`（静止超时 31 刻）、`happy_ghast_ai_ride`（玩家骑乘的服务端维护部分 31 刻），逐刻比位置 / 速度 / 朝向 / 年龄 / 尺寸 / 移动控制目标 / 正在跑的目标 / 导航路径 / 大脑快照 / 家 / 悬停状态 / 随机源。除 12 处已知三角函数表末位差外 0 不一致；`--break-self-test happy_ghast_hatch` / `happy_ghast_harness` / `happy_ghast_quad` / `happy_ghast_age` / `happy_ghast_brain` / `happy_ghast_still` 六处旧改坏，加 `happy_ghast_ai_speed`（飞行速度乘错，319 处不一致）/ `happy_ghast_ai_grow`（长大不换目标表，91 处不一致）两处新改坏都能抓到；`chunk-content java` 加了三题 `still_timeout` / BODY 挽具 / 幼年年龄的重存和一题 fresh happy ghast（629 checks 全过）；另有方块计划刻、飞行控制 / 目标表 / 长大切换 / 云层回血周期 / 发射器装备 / 挽具染色的 Rust 单测。**已知限制**：`canBeCollidedWith`、`getWalkTargetValue` 和 `#happy_ghast_tempt_items` 的逐刻运行分支没有单独场景；`RunOne` 的飞行闲逛在幼年 AI 场景里靠两边都从同一个已存在的 `WALK_TARGET` 起步避开官方无种子洗牌的分叉（洗牌种子公式两边一致，仍按官方夹具重设）；`Leashable.supportQuadLeash()` 官方只有船覆盖，Bamboo 已建模船并接了船作为被拴方的四点弹簧，但快乐恶魂自己作为被拴方仍是单点，所以「快乐恶魂作为牵引者」目前仍走单点弹簧（四点偏移 / 合力公式已备好，逐刻场景比的是弹性距离 / 断裂距离和牛的位置 / 绳状态）；`notifyLeashHolder` 的 5 刻 `IS_LEASH_HOLDER` 标志因被拴一方恒不支持四点而不可达；干恶魂方块的 `animateTick` 粒子（`WHITE_SMOKE` / `HAPPY_VILLAGER`）和环境音走 `playLocalSound` / `addParticle`，是纯客户端效果，服务端不发。
- **玩家左键近战攻击**按官方 26.3 `ServerGamePacketListenerImpl.handleAttack` → `Player.attack` →
  `ServerPlayer.crit` / `magicCrit` / `getEnchantedDamage` 完整实现：攻击冷却（基础伤害
  `×(0.2 + s² × 0.8)`、附魔加成 `× s`）、`ATTACK_SPEED` 属性与 `getCurrentItemAttackStrengthDelay`、
  挥空 / 攻击 / 换主手物品重置冷却（换物品按官方 `isSameItem` 只比物品 ID，耐久 / 附魔 /
  数量这类组件变化只刷新缓存、不清冷却）；`s > 0.9` 且下落中的暴击 `× 1.5`（发 `CRITICAL_HIT` /
  `MAGIC_CRITICAL_HIT` 动画）；拿剑、在地上、满充能、非暴击非疾跑时的横扫（打主目标
  `inflate(1, 0.25, 1)` 里 3 格内的生物，`sweeping_edge` 按 `L/(L+1)` 加成，发 `SWEEP_ATTACK`
  粒子，横扫命中的生物也照 `doPostAttackEffects` 吃火焰附加 / 节肢杀手效果）；疾跑 /
  `knockback` 附魔 / `attack_knockback` 的额外击退并取消疾跑；锋利 / 亡灵杀手 /
  节肢杀手（及节肢杀手的缓慢）、火焰附加、武器耐久、伤害指示粒子和
  `player.attack.strong / weak / crit / sweep / knockback / nodamage` 音效都真的发到客户端。
  PVP 按 `server.properties` 的 `pvp` 开关和官方 `canHarmPlayer` 处理（Bamboo 没有队伍）；
  玩家目标的额外击退、火焰附加点燃、横扫波及都按官方对 `LivingEntity` 一视同仁，
  额外击退还照 `Player.causeExtraKnockback` 末尾的 `ServerPlayer.syncVelocity` 分支，
  速度包发出去之后恢复受击前的服务端速度。横扫伤害源沿用主攻击的
  `damageSource`（含 `isCreativePlayer` 标记），创造模式攻击者也能绕过被横扫目标的
  受伤无敌帧。`Player.damageStatsAndHearts` 的
  `awardStat(Stats.DAMAGE_DEALT, Math.round(实际伤害 × 10))` 在会话内累计，
  `RequestStats` 时作为 `minecraft:custom` 统计发到客户端（`custom_stat` 的注册表 ID
  由生成器从 `registries.json` 导出）。
  对拍：`mob-oracle --only player_attack`（13 场景 91 刻，逐刻比每头牛的血量 / 击退速度 /
  着火 / 受伤时间 / 是否移除、**每头牛自己的随机源状态**，以及出站效果包
  `damage_event` / `sound` / `level_particles` / `animate` / `entity_event`，0 不一致；
  音效包末尾 8 字节的随机种子不比，见下面已知限制；`--break-self-test attack_charge` 能抓到
  故意改坏的充能值，`--break-self-test sweep_effects` 能抓到漏掉的横扫后附魔效果）；
  `a_critical_attack_emits_its_packets`、`a_sweep_ignites_swept_targets_with_fire_aspect`、
  `a_sweep_attack_emits_its_sound_and_particle`、
  `an_enchanted_attack_emits_the_magic_critical_animation`、`a_weak_attack_emits_its_sound`、
  `a_full_strength_attack_emits_the_strong_sound` 和
  `damage_indicator_particles_follow_the_damage_threshold`
  单测按音效 / 粒子注册表 ID 和动画动作字节，确认暴击 / 附魔暴击动画、横扫 / 伤害
  指示粒子（含 `damageStatsAndHearts` 的 `actualDamage > 2` 门槛）、strong / weak /
  sweep 音效真的发到客户端、横扫目标也着火且掉血；`melee_attack_hits_another_player`
  覆盖玩家互打的伤害、火焰附加、速度恢复和横扫波及第三名玩家。
  `melee_hit_awards_damage_dealt_on_request_stats` 走完整 `on_attack` → `RequestStats`，
  配合协议层 `award_stats_encodes_the_damage_dealt_custom_stat` 逐字节比 `award_stats` 包，
  确认近战命中真的把 `DAMAGE_DEALT` 发到客户端（故意把累计值改成 0，前者立刻失败）。
  `can_critical_requires_every_modelled_precondition` /
  `is_sweep_attack_requires_full_charge_ground_and_a_sword` 逐条锁住暴击 / 横扫的
  官方前置判定（去掉 `onGround` 门控两条都失败，横扫阈值按官方的 double 精度比较）。
  `a_zero_damage_attack_skips_the_hit_and_its_sound` /
  `a_rejected_attack_emits_only_the_nodamage_sound` 分别锁住「基础伤害与附魔加成
  都是 0 时整段跳过」和「`hurtOrSimulate` 被无敌帧拒绝时只发 `nodamage` 音」，
  `a_weapon_attack_range_component_overrides_the_default` 逐字段比武器
  `attack_range` 组件，`a_crouching_player_target_keeps_its_real_collision_box`
  锁住蹲下玩家目标的 1.5 高碰撞盒。每一条都做过改坏自检。
  `nodamage` 音效只有单测、仍没有对拍场景覆盖。
- **重锤下砸**按官方 `MaceItem`：`canSmashAttack`（摔落 > 1.5 格且没有滑翔）、分段加伤 `fall≤3 → 4·fall`、`3<fall≤8 → 12 + 2·(fall−3)`、`fall>8 → 22 + fall − 8`
  在暴击前加（不乘充能），致密 `smash_damage_per_fallen_block`（每级 0.5）乘摔落距离叠加；
  下砸时伤害源换成 `minecraft:mace_smash`，范围击退取主目标碰撞盒 `inflate(3.5)` 里的其它
  `LivingEntity`（玩家也算，攻击者 / 主目标 / 旁观者 / 同盟除外），沿「候选 − 主目标」方向
  按 `(3.5 − dist) · 0.7 · (fall>5 ? 2 : 1) · (1 − 击退抗性)` 推开（竖直恒 0.7）；命中后
  攻击者竖直速度设成下砸冲量 `0.009999999776482582`、摔落距离归零、发 `set_entity_motion`
  给自己，落地音分三档（`mace.smash_air` / `smash_ground` / `smash_ground_heavy`），`levelEvent
  2013` 地面撞击事件；破甲 `armor_effectiveness`（每级 -0.15）加进护甲减伤比例再夹 [0,1]，
  对生物和玩家受击路径都生效。**对拍**：`mob-oracle --only player_attack_mace` 覆盖 2 / 5 / 10
  格下砸（比伤害、目标血量、目标速度和攻击者速度 / 摔落距离）、范围击退（旁边两头牛）、
  致密 III（低充能压低基础伤害，让致密决定牛死不死），`--break-self-test mace_smash` /
  `mace_density` / `mace_aoe` 三处改坏都报不一致；破甲对拍 `player_attack_mace_breach0/4`
  给目标牛穿铁胸甲、带 / 不带破甲各砸一次比伤害（`--break-self-test mace_breach` 能抓到
  剥掉破甲的改坏）。风爆 `wind_burst` 按官方 `post_attack` 的 `explode`：下砸且
  `fall_distance ≥ 1.5` 时以攻击者为圆心半径 3.5 的爆炸，只按 `knockback_multiplier`
  （lookup `[1.2, 1.75, 2.2]`、fallback `1.5 + 0.35×(L−1)`）推开范围内的实体、不伤不破坏
  方块，攻击者自己那份 `pushFromExplosion` 写进服务端速度、爆炸包（gust 粒子和
  `wind_burst` 音效）`connection.send` 给自己；对拍 `player_attack_mace_wind_burst`
  比攻击者被弹起的速度（`--break-self-test mace_wind_burst` 能抓到剥掉风爆的改坏）。
- **长矛**按官方 `PiercingWeapon` / `KineticWeapon`：左键穿刺走 `PlayerActionKind::Stab` →
  `PiercingWeapon.attack`，射线（`getHitEntitiesAlong` + 擦边 margin + 方块遮挡）逐个
  `stabAttack`（伤害、两下击退、可下马），命中后 `postPiercingAttack` 接 `lunge` 的
  `post_piercing_attack`（前冲、扣耐久、饥饿、音效），`MINIMUM_ATTACK_CHARGE` 门槛、
  破甲伤害源都接；挥手的动画读 `attack_animation` 组件（长矛是刺击 `stab` 时长 19）。
  右键蓄力走 `KineticWeapon.damageEntities`：`Item.use` 开始使用并播蓄力音，逐刻
  射线接触伤害（`contactCooldownTicks` 跳过刚刺中的实体、三组 `Condition` 按时长 /
  绝对速度 / 相对速度判定），`baseDamage` 用属性**基础值**加 `floor(rel × damage_multiplier)`、
  正在使用时伤害不乘充能，命中后实体事件 2（客户端播命中音）、`SPEAR_MOBS_TRIGGER`、
  耐久 / 统计 / 饥饿。对拍：`player_attack_spear_stab`（穿刺）和
  `player_attack_spear_charge`（蓄力逐刻接触伤害，`--break-self-test spear_charge` 能抓到
  取消蓄力的改坏）。
- **宠物继承主人的队伍**：`TamableAnimal.getTeam()` 覆写——驯服动物自己没队时返回
  `getRootOwner().getTeam()`，Bamboo 的 `goals.rs` 同盟判定和重锤范围击退的候选过滤都补上
  这一层回退（`mob_team_index` 单测钉住，全量 mob-oracle 仍是 46）。
- **玩家近战攻击的已知限制**：暴击前置判定的
  `onClimbable`（脚下的可攀爬方块）、`isInWater`（碰到水）、失明（`isMobilityRestricted()`）、
  骑乘（`isPassenger()`）都照官方判断；
  节肢杀手的缓慢时长按 `ApplyMobEffect` 从**受害者自己的随机源**取三次
  （`nextInt(1)`、时长、等级），落在 `[30, 30 + 10×(L−1)]` 刻、等级恒为 3；`Stats.DAMAGE_DEALT`
  按官方 `awardStat` 在会话内累计、在 `RequestStats` 时作为 `minecraft:custom` 统计发给客户端，
  但不写原版 `stats/<UUID>.json`（也不更新统计计分板目标）；`attackStrengthTicker` 的每刻递增放在玩家 tick 开头
  （官方在实体 `aiStep`），只在同一刻边界的取数顺序可能差一刻；攻击音效 / 粒子 / 动画
  包已经在 `mob-oracle` 里逐字节比，但**音效包末尾 8 字节的随机种子不比**：官方每发一个
  音效就当场从世界随机源取种子，Bamboo 把一拍的效果攒到末尾统一发，取数顺序因此不同，
  客户端只用这个种子给音高加一点扰动；横扫候选里的玩家排在生物之后（官方按实体分区
  顺序），不影响伤害和取数，只影响多目标时的发包顺序；攻击场景的对拍夹具跳过
  `cow.tick()`，否则官方一侧的重力 / AI 会污染击退速度，逐位比较不可行；PVP 的对拍
  没有专门场景，只有 `melee_attack_hits_another_player` 单测；玩家目标的额外击退按
  官方 `ServerPlayer.syncVelocity` 分支在发完速度包后恢复受击前速度，但 Bamboo 没有
  `syncVelocity` 这个状态位，`ServerEntity` 的逐刻速度同步语义也没有对齐。够距按官方
  `handleAttack` 的
  `isWithinAttackRange`：实体交互距离（生存 3.0 / 创造 5.0，武器带 `attack_range`
  组件时读组件）配 3.0 缓冲，量眼睛到目标整个碰撞盒的闭区间距离，并查世界边界
  （`mainHandItem.isItemEnabled(enabledFeatures())` 的 feature flag 没查）；这一层完全
  没进 `mob-oracle`（夹具直接调 `Player.attack`），由
  `attack_reach_uses_the_entity_interaction_range` 等单测守住；
  横扫候选按主目标的整个碰撞盒 `inflate(1, 0.25, 1)` 与候选碰撞盒相交判定（不是位置
  点），玩家也算候选（额外击退 / 火焰附加同样对玩家生效），但仍没有队伍判定；
  `ATTACK_SPEED` / `ATTACK_KNOCKBACK` 只累加 `ADD_VALUE` 修饰符、横扫阈值把 `getSpeed()`
  写死成 f32 的 0.1（乘 2.5、求平方都照官方用 double），带乘法修饰符或速度药水时
  可能不同；受害者装备附魔（荆棘）和
  `autoSpinAttack` 的武器来源都未建模。

- 多名玩家进入同一个世界；新世界默认 `normal`（噪声地形，见上面世界生成一节），
  `level-type=flat` 用超平坦；登录模式由服务器目录里的 `online-mode` 决定（官方默认正版验证）。
- 默认按官方 `server.properties` 的 `online-mode=true` 走正版验证；官方未实现的键原样保留、
  写回并在启动日志里列出「现在不起作用」。要离线直连就要在 `server.properties` 里写
  `online-mode=false`（`probe` / 单元测试 / 压测工具的内置 `serve` 用 `ServerConfig::default()`，本来就是离线）。
  `bamboo.toml` 的 `redstone_cache` 默认关、`chunk_worker_threads` 默认 0（内联）、
  `entity_threads` 默认 0（精确模式），完整清单见 `docs/configuration.md`。
- 玩家位置不再照单全收。一个 tick 里离本 tick 起点太远会拉回（`moved too quickly`），
  位移和方块碰撞对不上、或者新位置穿进实心方块也会拉回。生存和冒险模式悬空超过 80 tick
  会按 `multiplayer.disconnect.flying` 断开。创造和旁观可以飞。`allow-flight` 已在
  `server.properties` 里原样保留，但 Bamboo 还没实现这个键，按官方默认关闭。
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
  鱼桶倒出水之后按 `MobBucketItem.checkExtraContent` 在落点生成 `FromBucket = true` 的鱼并叫一声；发射器倒鱼桶同样生成。
  美西螈现已建模，倒美西螈桶（含发射器）按 `MobBucketItem.spawn` 放出 `FromBucket = true` 的美西螈，`bucket_entity_data` 存花色 / 年龄 / 锁定标志；`BUCKET` 原因下 `finalizeSpawn` 直接返回，世界随机源只取朝向那一次 `nextFloat`。
  蝌蚪 / 硫磺立方桶还没建模，只按官方 `EntityType.create` 的次数取随机数。硫磺立方桶不放方块，只放自己的倒出声。
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
  打雷时先对所有区块跑官方 `tickThunder` 的 `nextInt(100000)`，命中后按高度图 / 避雷针 POI /
  附近活物选落点，生成闪电实体（按 `SPAWN_MOBS` 和区域难度还可能生成骷髅陷阱马），
  闪电第一刻引火、激活避雷针、除铜锈，之后逐刻对附近实体 `thunderHit`（苦力怕带电），再跑全部区块的 `tickChunk`。
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
  TNT 生成点燃的实体并播放 `entity.tnt.primed`。箭、药水箭和光灵箭按 `ProjectileDispenseBehavior` 生成真的箭实体：
  位置是 `getDispensePosition` 再加 0.1 的 y，速度 1.1、散布 6，拾取方式是允许，声音换成 1002（仍发朝向上方的 2000）。
  发射器也能放船 / 竹筏 / 运输船（按 `BoatDispenseItemBehavior` 算落点、朝向并继承名字与 `entity_data`）、
  用刷怪蛋生成对应生物（按 `SpawnEggItemBehavior` 应用 `entity_data`、投递 `ENTITY_PLACE` 并扣一个）；
  放不下或没有实体数据时分别走官方的内层默认抛出与默认抛出分支。
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
- 正版验证和加密（`server.properties` 的 `online-mode`，默认 true；`ServerConfig::default()` / oracle / probe 是 false）：启动时生成 RSA 1024 密钥对，收到 `hello` 后把公钥和 4 字节挑战码发给客户端，
  客户端回来的共享密钥和挑战码按官方解密校验，密钥包之后两个方向都走 AES/CFB8，加密在最外层，连长度前缀一起加密。
  serverId 哈希用官方那套带符号的十六进制 SHA-1，再按官方格式问 session server 的 `hasJoined`，
  打开 `prevent-proxy-connections` 时带上客户端 IP。失败的分类照 authlib 来：连不上、超时和 5xx 是
  「验证服务器不可用」，其余（4xx、304、读不出档案、回复点名了几种已知错误）都算没验过，
  两种情况各用官方的翻译键。连接和读取各 5 秒，也和 authlib 的两个常量一样。
  问 session server 在连接任务里做，世界线程不等这段 IO。session server 给的 UUID、皮肤和签名原样进 Tab 列表。
  和代理转发互斥，两个都开时转发赢。
- **收纳袋（`minecraft:bundle_contents`）**：照 26.3 字节码把 `BundleItem`、`BundleContents` / `Mutable`、
  `ServerboundSelectBundleItemPacket` 和 `AbstractContainerMenu.tryItemClickBehaviourOverride` 接进菜单点击、
  选中格、右键使用和物品被毁四条路径。容量按官方的化简分数算：普通物品每件占 `1 / max_stack_size`，
  嵌套收纳袋占「内层重量 + 1/16」，有 `bees` 的蜂箱占满一袋，潜影盒装不进去；`tryInsert` 把同种物品
  合并并移到列表最前，`tryTransfer` 只从槽里拿装得下的数量，满袋再装保持两边不变并播
  `item.bundle.insert_fail`。左键 / 右键的光标方向、空格取出、`Slot.safeInsert` 的剩余物回装，
  以及 `insert` / `remove_one` / `drop_contents` 的音效音量与随机音高都按字节码实现。
  客户端报上来的 `SERVERBOUND_BUNDLE_ITEM_SELECTED` 切换服务端本地的选中下标：`Mutable.removeOne`
  会取选中的那一整堆并清掉选中，物品堆从格子挪到光标再放回别的格时下标跟着走；
  网络 / 存档的 `bundle_contents` 和 `ItemStack.equals` 都不含这个下标，切选中格不会补发槽位包。
  右键使用照 `BundleItem.use` 只进入 200 刻的「正在使用」状态，`onUseTick` 在第一刻和剩余小于
  190 的偶数刻逐件倒出：每次走官方 `Player.drop(stack, false, PREDICTED)` 的位置与速度，
  播 `item.bundle.drop_contents` 并记一次 `ITEM_USED`。火烧 / 岩浆等把收纳袋物品实体打到血量
  归零时，`ItemEntity.hurtServer` → `ItemStack.onDestroyed` → `ItemUtils.onContainerDestroyed`
  把内容物在原地逐件掉出；摔进虚空只 `discard()`，不掉内容物。
  **对拍**：`menu java` 六道固定题（左 / 右键装取、62/64 装到满、选中第 2 件后拿到光标再放到别的格取出、
  嵌套收纳袋、满袋再装）在 245 题 2629 次点击里逐步比全部 63 个槽位、光标、组件补丁十六进制、
  `state_id` 与补发槽位，0 不一致；`entity-oracle java` 的 `bundle_destroyed` 用官方 `onDestroyed`
  生成的两件掉落物逐刻比位置 / 速度 / 数量 / 年龄 / 拾取延迟，67 题 1212 刻 0 不一致。
  四处改坏确认（`--break-self-test bundle_first|bundle_slots|bundle_selected` 与
  `entity-oracle java --break-self-test bundle-drops`）分别报 5 / 15 / 12 / 2 处差异。
  **已知限制**：选中下标是服务端本地状态，复制物品堆时两份会带上同一个下标；
  存档和网络字节不持久化它；收纳袋的 /tooltip 显示格数、`Consumable` 之外的物品使用动画、
  以及 `BundlePredicate` 的物品判据暂未单独接入。
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

世界生成的数学已经移植好了；**主世界 `terrain`（`doFill` + 地表规则 + 雕刻器）的纯函数
已接进 Director 的多阶段生成流水线**，新世界默认 `normal`（噪声世界），不再是超平坦：

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
- 区块级的 `chunkgen` 对拍：官方在 JVM 里用 `ChunkGenerator` / `RandomState` / `ProtoChunk`
  生成同一个主世界区块到 `BIOMES` / `NOISE` / `SURFACE` / `CARVERS`，逐格比较群系和方块，
  0 不一致（命令和耗时见 `docs/worldgen.md`）。含水层、成批密度求值和官方 sampler 缓存也已移植。
- **接进游戏**（`docs/worldgen.md`「接进游戏」）：`server.properties` 的 `level-seed` /
  `level-type` / `generator-settings` / `generate-structures` 生效，认 `normal` / `flat` /
  `large_biomes` / `amplified`；`World::build` 对非 flat 世界用 `PipelineKind::Staged` 按
  `ChunkPyramid` 派十个 `ChunkStatus` 阶段，`terrain` 一步算出 quart 群系和跑完
  CARVERS 的方块并重算高度图（`biomes` 阶段透传，避免两次组装 column；官方
  `terrain` 里地表/雕刻器本就 uncached 重算群系），默认起一个小计算池把生成移出世界线程；
  `world_gen_settings.dat` 按官方 `WorldGenSettings.CODEC` 持久化种子与维度生成器，读档时以它为准。
  `gen-world <java>` 用官方出题、调**游戏里同一份** `generate_chunk` 逐格对拍，多种子 0 不一致。
  `gen-world --level-type large_biomes|amplified` 还把这两种世界换上对应的官方
  `NoiseGeneratorSettings` 逐方块对拍 0 不一致（三种共用 `minecraft:overworld` 群系预设）。
- **出生点**照官方 `MinecraftServer.setInitialSpawn` 整条链移植：`NoiseBasedChunkGenerator.getOrigin`
  → `NoiseSpawnFinder` 的气候螺旋 + `SpawnTargetPoint.sampleFitness`（`mc-bamboo-worldgen::spawn`），
  暂定点 `(spawnChunk*16+8, 64, +8)`，`PlayerSpawnFinder` 的 11×11 区块螺旋 + 三种高度图 +
  流体/碰撞判定和 `fixupSpawnHeight` 兜底；奖励箱按专用服务端默认关闭。`spawn <java>`
  对拍在 `normal` / `large_biomes` / `amplified` 上多个种子把出生区块和最终出生点与官方
  逐个比，0 不一致。`saved-data` 让官方 `WorldGenSettings.CODEC` 复核了噪声生成器的形状
  （三种 settings id 都过）。
- **flat `generator-settings`**：五个字段保存到 `world_gen_settings.dat`。
  `structure_overrides` 缺省选所有符合固定群系的结构集，显式 `[]` 选空集；
  `generate-structures=false` 关结构而不关地物和湖。`features`、`lakes` 和非阻挡地层
  的末步填充均经过 Director 流水线。`flat-world <java>` 比 11 格 terrain 和 54 格
  FEATURES（含村庄、关结构、空覆盖、虚空平台/雪层、平原草、熔岩湖），共 6,389,760 方块 0 不一致。
- release 实测 `generate_chunk` 各阶段耗时、在 `normal` 世界上跑了 6 个飞行假玩家的压测
  （生成不占世界线程、默认 4 线程会积压区块），数字和计算池默认线程数建议见 `docs/worldgen.md`。

玩家穿戴可用的滑翔装备后，可以在空中开始滑翔。服务端同步滑翔标志、姿势和速度，计算撞墙与落地伤害。
滑翔每 10 刻发一次 `ELYTRA_GLIDE` 游戏事件，每 20 刻随机挑一件可用的滑翔装备扣耐久，创造模式免耐久消耗。
移动校验的平方位移阈值在滑翔时为 300，通常为 100，按同一服务端刻处理的包数缩放。
`player_movement_check` 控制全体玩家的速度校验，`elytra_movement_check` 控制滑翔玩家的速度校验。
滑翔中右键烟花会生成附着在玩家身上的加速烟花（见「烟花火箭」一节）：服务端把官方公式算出的速度写进玩家
`getDeltaMovement()`，移动校验的 `expected`（官方判据 `moved - expected > 300 * packets` 里减掉的那一顶）因此变大，
客户端那份加速则靠同步过来的 `DATA_ATTACHED_TO_TARGET` 在本地对同一玩家施加；`elytra_movement_check` 关掉时这段校验整体跳过。

已知限制：

- 烟花加速按官方 `isFallFlying()` 门控：玩家中途落地后烟花不再加速、但仍跟随到寿命。左右手（`getMainArm()`）
  与副手判断、解拴绳（`LEAD_BREAK`）、`dropLeash()` 掉 `LEAD` 物品和 `dropAllLeashConnections` 的 SHEAR 事件
  都已接入；烟花 `use` 的 SHEAR 源实体为空（字节码与任务书「null 不发」不同，按字节码），细节见「烟花火箭」一节。
- 移动校验比官方严，也有几处更松。激流、睡眠、击退宽限、切维度和单人房主仍有差异。
  非滑翔时的预期位移主要来自跳跃和卡在方块里的减速。碰撞算方块和世界边界，
  边界按官方 `Entity.collectCollidersIgnoringWorldBorder` 的条件（离边界够近）才把碰撞形状加进来；
  玩家互相穿身，±3.0E7 的坐标夹取仍在（`Level.isInWorldBoundsHorizontal`，和世界边界是两件事）。
  没加载的区块仍用超平坦模板，官方那里没有碰撞。玩家的移动包落地时按支撑方块分发 `fallOn`，
  结算伤害、音效、游戏事件和踩踏。床按方块属性折减下落距离，耕地变成泥土时会发相对传送以抬高玩家。
  世界边界的伤害也不结算，见下一条。
  蜘蛛网和细雪会在 tick 结束时套上卡住的速度并清掉摔落距离；压力板、绊线和甜浆果丛被踩到的效果没接。
  终末之诗、每 200 tick 补发脚下方块、载具移动都没做。
- 世界边界还有几处没接：玩家生命值和伤害系统已建模，但官方 `LivingEntity.baseTick` 里的
  边界伤害（`damageSources().outOfBorder()`）不结算，`damage amount` / `damage buffer` 只存起来、
  跟着 `initialize_border` 发给客户端；`max-world-size` 没读，绝对最大值固定 29999984。
  重生点和传送不按官方 `getWorldBorderAdjustedRespawnData`、`PortalForcer`、
  `LivingEntity.randomTeleport` 夹进边界（死亡重生走自己的 `PlayerList.respawn` 等价路径，
  但没有下界传送门和随机传送）；
  `canSpawnEntitiesInChunk`、刷怪和生物 AI 里的边界判断用不上。
  边界状态写进官方按维度存放的 `dimensions/minecraft/overworld/data/minecraft/world_border.dat`
  （`export_settings` / `restore`，`restore` 传存档里的游戏时间），启动时读回，编码和官方
  `WorldBorder.Settings.CODEC` 对拍。边界伤害的警告遮罩、`BorderStatus.getColor` 由客户端自己算。
- **世界生成（本轮接入）的已知限制**：`biomes` / `terrain` 阶段接了真生成，`possibleBiomes`、雕刻器的
  `carverBiome` 分支和 `WorldGenRegion` 的 `uncachedBiomeResolver` 已按真实服务端路径逐方块对拍 0 不一致，
  六张高度图逐列、存档里四张 FINAL 高度图位流逐字对拍 0 不一致（见 `docs/worldgen.md`）。
  `terrain` 的地表阶段现在读**相邻区块里 `BIOMES` 阶段成批填好的 quart 群系格**，和官方
  `WorldGenRegion.getBiomeManager()` 一样，而不是重算单点气候：成批 `sampleVolume` 和单点 `sampleValue`
  的浮点顺序不同，气候值落在 `ParameterList` R 树最近邻的临界点上时会选到不同群系。**这条以前漏过**：
  种子 0 的区块 (644792,109695)（恶地和沙漠交界）地表 33 格被放成各色陶瓦，官方是砂岩/沙子；修复后
  该区块 0 不一致，并加了回归测试 `worldgen::tests::badlands_desert_boundary_uses_stored_quart_biomes`。
  代价是每个区块的 `terrain` 多算 8 个邻居区块的群系（气候成批采样，本机约 +4 ms/区块）。
  `features` 阶段已接进游戏：`Task::Generate` 按 `ChunkStep` 依赖半径带邻居引用，世界线程把每个区块的
  CARVERS 半成品存成不可变 `Arc` 快照，计算池上跑 `Generator::features()`，写邻居的增量按 `TaskId` 顺序合并、
  完工时按来源中心的固定先后（先写的赢）落到交付盒子；1 线程与 4 线程计算池生成同一片区块逐字节相同。已合进 main 的地物类型都会在
  游戏里放置，`gen-world --through features` 目前逐方块对拍的是矿石（8 个中心 / 7077888 方块 0 不一致），
  其余已移植类型由 `chunkgen` 单独对拍；还没移植的类型照对拍的做法标 `unsupported` 跳过。
  `initialize_light` / `light` 两个阶段在流水线里用邻居快照算出生成期光照，交付的区块带 `isLightOn=true`，
  不再等运行时补算（`gen-light` 对拍）。`structure_starts` / `structure_references` 已接成纯函数重算
  （半径 8 邻域 + 线程本地缓存），terrain 注入真实 `Beardifier`，features 每个 decoration step 先放结构件
  再放地物，区块 NBT 写 `structures` 键并能读盘恢复；`structure-pipeline` 对拍把官方 17×17 的
  `createStructures` / `createReferences` 与游戏里的同一条路径逐项比（28 行 / 2482 结构件 0 不一致，
  改坏确认会失败）。放置已对拍的 jigsaw / 矿井 / 要塞 / 海底神殿 / 林地府邸 / 雪屋 / 沉船 / 海底遗迹
  会在游戏里写方块（含四个单件结构与废弃传送门）；`spawn` 阶段的生成期刷怪见下面「生成期刷怪」一条。
  `features` 与官方的**语义差异**：官方邻居溢出的写来自各邻居区块跑到 `FEATURES` 时的 `WorldGenRegion`，
  结果取决于实际的生成先后；Bamboo 在中心这次 `features` 里用纯函数重算 8 个邻居中心写进来的增量，按
  来源中心的固定先后（C 自己最先、邻居按 `(dz,dx)`）合、同格先写的赢，邻居快照是干净的 CARVERS 地形。结果与线程
  调度无关、可重现，但边界上重叠的写入可能和某一次官方生成不同；地物越来越多后要重新核对。
  `PostProcessing` 位置表已按官方 `markPosForPostProcessing` 逐段对拍并写进存档
  （`gen-world --through features` 比 packed short，两种 salt 的 6×36 扩大样本各 0 不一致；
  完工表按「地形标记 + 9 个中心的标记」组装，`feature-merge` 比交付盒子和存档往返）。
  区块加载等级跨入 `BLOCK_TICKING` 时，拥有它的岛按这张表做 `postProcessGeneration`
  （形状更新、流体 tick），清表后才发包（`mc-bamboo-vanilla post-process` 对拍）；
  地物和结构写下的方块实体随九中心组装进交付盒子；
  `ProtoChunk` 其余字段、非 `full` 区块从盘上读回后按状态继续生成仍由 `worldgen-resume`
  分支负责。`ChunkColumn::from_parts`
  产出的盒子 `untouched=false`，生成的区块一律算「改过」而写盘，官方可以丢弃原样生成区块。
  出生点的 `setInitialSpawn` 链已按官方逐段移植并对拍（`getOrigin` 气候螺旋、
  `PlayerSpawnFinder` 的 11×11 螺旋 + 高度图/流体/碰撞、`fixupSpawnHeight` 兜底），
  但**只比较到 CARVERS 地形**：出生点仍在 CARVERS 快照上算，官方真实世界里树、雪层、
  结构会改变 `MOTION_BLOCKING` 高度图和落点，Bamboo 目前算的是没有这些内容的地表，
  因此个别种子的实际出生点仍可能和满配官方世界不同；等 `full` 阶段补齐后要重跑
  `spawn` 对拍。`getCollisionShape` 在 Java 对拍里用 `EmptyBlockGetter`、在 Rust 里用
  `COLLISION_FULL_BLOCK` 事实近似，对 CARVERS 的完整方块/流体/空气等价。`setInitialSpawn`
  本身不调 `fixupSpawnHeight`（那是 `PlayerSpawnFinder.findSpawn` 进服兜底），Bamboo 只在
  11×11 整片找不到落点这一罕见情形用它，且在实测种子上是恒等变换。flat 世界仍保留旧的
  超平坦出生点（`(8, 0, 8)`），`initial_spawn` 只对三种噪声世界生效。超平坦结构只用内置注册表：
  `structure_overrides` 里的 `#tag` 或任何查不到的名字都按 `lenientOptionalFieldOf` 当作
  缺省（生成该固定群系适用的全部结构集），不展开标签；缺 `layers` 的 JSON 官方会整份解析失败、
  回退默认预设，Bamboo 只对 `structure_overrides` 做同样的回退，同一 JSON 里的 `biome` /
  `features` / `lakes` 仍会生效。
  未对拍所有结构、群系组合或超平坦光照与续生成。
  世界生成缓存（[配置](docs/configuration.md#世界生成缓存)）只影响速度和内存，不影响结果：官方在
  `ChunkMap.releaseGeneration` 等真实持有引用处释放，Bamboo 用「邻域使用者完成位 + 空闲定时」近似，
  已生成区域最外几圈可能提前释放、下次再要时现算（约 10–20 ms/区块）；空闲清理是进程级策略，
  官方没有对应行为。默认地形 128 MiB、地物 64 MiB、空闲 30 s。
  奖励箱按官方专用服务端默认关闭。
  随机种子（`level-seed` 为空）用 `nanoTime ^ uniquifier` 近似官方 `RandomSupport`，进程内固定，
  只和官方「空种子 = 随机」的语义一致、不逐位比较。`normal` 世界的飞行压测已实跑：生成
  不占世界线程（`tick_total` p50 0.12–1.18 ms），但默认 4 线程在 6 个飞行玩家下会积压
  区块（backlog 峰值 436、区块延迟数秒）；计算池默认仍取核数一半夹在 1..=4（不盲目调高，
  内存吃紧），部署可显式加大，换上真实地物/结构后要重量，见 `docs/worldgen.md`。
  下界和末地、非空 `BlendingData` 也没做；`Beardifier` 的非空密度路径已移植并逐位对拍（见下面 Beardifier 一条），
  `structure_starts` / `structure_references` 由 `terrain` 和 `feature` 阶段按纯函数重算，
  `generate_terrain_stage` 把真实 Beardifier 注进 `finalDensity`（见上面「结构生成」一条）。
  按区块分格插值（官方 `interpolated` 的成批求值）
  在区块网格上已由 `chunkgen` 覆盖，单点 `worldgen` 对拍里跳过这批函数。

  （2026-11-14 按实现更新：`docs/worldgen.md`「还没做」、`docs/performance-optimizations.md` 第 9 节、
  README「区块级对拍」；合并提交 `ae76868`、`9a96b5e`、`9d6e94c`。第二轮 features 接入见
  `docs/worldgen.md`、`docs/worldgen-features-plan.md`、`docs/world-model-design.md` 5.3.1 / 第 16 节）
- **世界生成数据全进程共用一份**（`op-wgmem`，2026-11-21）：只读的解析结果（噪声、密度函数、
  群系表、地物 / 结构定义）由 `Worldgen::shared()` 的 `OnceLock` 只解析一次，所有生成线程引用
  同一份 `&'static`，每条线程只留自己的 `Generator` 和随机 / 临时缓冲。群系 R 树 `lastResult`
  （官方 `ThreadLocal`）从 `Cell` 改成「线程 + 表」的线程本地状态，共享数据是 `Sync` 的只读数据；
  `gen-world` / `--through features` / `structure-e2e` 0 不一致、`staged_tests` 1/2/4/8 线程
  逐字节相同。4 / 8 线程闲置 RSS 实测 889 → 700 / 1476 → 1094 MiB（各 3 轮中位数，见
  `docs/performance-optimizations.md` 第 24 节）。**已知限制**：数据随编译嵌入、进程内只有一份，
  将来支持运行时换数据包或同进程多份生成数据时 `shared()` 要按实例索引；解析失败的错误会被
  `OnceLock` 缓存到进程结束（当前内嵌数据恒能解析）。
- **内存构成实测与结构模板压缩**（`op-memprof`，2026-11-21）：新增堆分析器
  `tools/mc-bamboo-memprof` —— 记账全局分配器按调用栈统计还活着的字节，再按归属汇总，
  另读 Windows 进程工作集、私有提交、堆与地址空间、线程栈；`run.sh` 固定种子 20261121、
  视距 8，在「起服后空载 / 一名假玩家在线 / 玩家离开后」三个时刻各取一份读数。实测最大头是
  **解析后的结构模板 NBT**：在线约 318 MB、玩家离开后仍约 318 MB 不释放（官方
  `StructureTemplateManager` 的 `structureRepository` 也是全量常驻，Bamboo 原先还按线程各留一份）。
  两项原型已落地：① 拼装阶段只留「尺寸 + 拼图方块」的全进程共用外形表，整份模板按需解析进
  **有上限**的线程本地缓存（`crates/mc-bamboo-worldgen/src/structure/template.rs` 的
  `TemplateShape`）；② 缓存的压缩区块帧 `shrink_to_fit` 掉按未压缩长度预留的多余容量
  （`crates/mc-bamboo-game/src/outgoing.rs`）。同一固定种子、不带分析器的 release 服务端实测
  在线工作集 588 → 260 MB、玩家离开后 505 → 201 MB；`jigsaw templates` / `jigsaws` / `pieces`
  与 `gen-world` 结构位置 0 不一致。**已知限制**：有界模板缓存是**故意和官方不同**的——官方
  `getOrCreate` 的 `structureRepository` 从不清空，Bamboo 攒到 `CACHE_BLOCK_BUDGET` 个方块项
  就整表清空、下次重新解析（模板不可变、可重解析，结果逐字节不变，只多花一点 CPU）；全进程
  外形表用一把 `Mutex`，多生成线程同时查外形会短暂争用，没测吞吐影响。堆分析器自身每块多一个
  ≥8 字节的头、另占 76 MB 站点表，进程级读数偏大，真实读数一律以不带分析器的 `bamboo` 为准；
  归属规则是按函数名匹配的启发式，改函数命名或内联会串类；只在 Windows 上有 PDB / `HeapCompact`
  实现。`HeapCompact` 压实实验显示空闲堆块不会退还给系统，故没有改分配器。
- **地物注册表全进程共享**（`op-memprof` 续，2026-11-22）：堆实测里优化后最大的常驻项是
  每条生成线程各 `parse` 一份的只读 `feature::registry::Registry`（在线 / 离开后约 25 MB 常驻）。
  现把它解析进 `Worldgen`（已由 `Worldgen::shared()` 全进程共用一份），`chunk::Generator` 改为
  借用 `&'a Registry`，去掉每线程副本。结果逐字节不变：`gen-world --through features`
  （3538944 方块）、`chunkgen`、`structures`、`structure-rings`、`jigsaw templates` 全 0 不一致。
  同口径重测该归属 25.14 → 0 MB、共享 `Worldgen` 22.02 → 24.93 MB（吸收唯一一份），净常驻约
  −22 MB；不带分析器的 release 服务端玩家离开后工作集 181 → 157 MB，见
  `W:/data/bamboo-agents/reports/op-memprof.md` 与 `docs/profiling.md` §20.1。
- **结构状态与群系表全进程共享**（`op-memprof` 续，2026-11-22）：每条计算线程原先各自解析
  一份 `WorldStructureState`（结构集、结构 / jigsaw 注册表、模板池、要塞环位置）和一份群系
  `ParameterList`。两者构造后只读，现按 `(seed, settings, flat)` 全进程缓存一份 `Arc`，R 树
  `lastResult` 仍是线程本地。结果逐字节不变：`structures`、`structure-rings`、
  `chunkgen --through features` 全 0 不一致；同口径重测气候树 6.85 → 1.71 MB、结构状态类
  15.74 → 10.56 MB，约 −10.9 MB 常驻，`plain` 离开后工作集 157 → 145 MB，见
  `docs/profiling.md` §20.2。**已知限制**：共享缓存按 `(seed, settings, flat)` 常驻不回收，
  进程内换种子 / 平坦设置会累积多份（改动前每线程缓存同样不回收，只是每份副本更多）。
- **`Worldgen` 只留启服后还会读的注册表**（`op-memprof` 续，2026-11-22）：`Worldgen::raw` 原先
  克隆整份 `worldgen.json`。启服后只有 10 张顶层表会被读（噪声设置、密度函数、地表规则 / 条件、
  雕刻器、结构集 / 结构、模板池、处理阀），多噪声预设、地物三张表、`features_per_step`、
  `block_state_provider`、`end_biomes`、`noise` 解析完就没人再读；现在只复制那 10 张
  （`crates/mc-bamboo-worldgen/src/lib.rs` 的 `RUNTIME_TABLES`）。结果逐字节不变——只是不再
  保留没人读的键：`chunkgen --through features`、`structures` 全 0 不一致。同一快照
  `shared Worldgen` 24.93 → 11.50 MB（−13.43 MB 常驻）；不带分析器的 release 服务端
  54.6 / 212.4 / 145.0 → 38.9 / 192.5 / 128.3 MB（空载 / 在线 / 离开后，忙机器 ±10–20 MB），
  见 `docs/profiling.md` §20.3。模板池后来进一步改成进程级常驻，`RUNTIME_TABLES` 收到 9 张，
  见下一条。
- **不再重复全量解析世界生成数据 + 模板池进程级常驻**（`op-memprof` 续，2026-11-22）：
  `chunk::Generator::with_settings` 原先通过 `parse_settings` **重新解析整份 `worldgen.json`**
  （4.4 MB）只为取一个几 KB 的 `noise_settings` 条目，每条生成线程各付一次；
  `feature::fossil::chain` 每次放化石也要解析两遍。现在都直接取 `Worldgen::raw()` 里已有的
  `noise_settings` / `processor_list`，无调用者的 `feature::noise_state::holder_registry()`
  一并删掉。拼图模板池 `template_pool` 只随数据版本变，解析一次存进 `Worldgen::template_pools()`，
  `WorldStructureState` 克隆 `Arc`，`raw` 从 10 张收到 9 张。结果逐字节不变：`chunkgen
  --through features`（886272 项）、`jigsaw pools`（2205 池 / 19994 件）全 0 不一致。同一快照
  `startup` 的 tracked live 13.5 → 11.9 MB、`shared Worldgen` 11.50 → 8.41 MB（−3.09 MB）。
  **已知限制**：`TemplatePools::parse` 在启服时就预计算 `max_size`，因而会提前加载模板外形；
  空服比改动前多约 1.5 MB（有玩家时本来就会加载），见 `docs/profiling.md` §20.4。另实测
  「玩家离开后」：离开 20 s 时工作集 296.7 MB，3 s 后 104.2 MB（默认 30 s 空闲释放清掉
  128 MB 地形 + 64 MB 地物两份缓存），再强制 `EmptyWorkingSet` 只降到 2.2 MB 而私有提交仍是
  90.1 MB（纯换出页面，不是真正归还）。所以用户看到的「离开后掉不下来」有一部分是读数早于
  空闲释放；两份缓存的 MB 上限就是在线峰值的硬上限，可在 `bamboo.toml` 调小。
- **sin 表与结构群系 R 树去重**（`op-memprof` 续，2026-11-22）：`Mth.sin` 的 65536 项表
  原在 `mc-bamboo-core`、`carver`、`feature::ores`、`feature::dripstone_utils` 和
  `mc-bamboo-loot` 各建一份（各 256 KB），现在只留 `mc-bamboo-core::mth` 那一张。
  `ParameterList` 的 R 树 / 参数点 / 值改为 `Arc` 共享，新增 `fork()`：结构线程不再从条目
  重建一棵群系 R 树，而是 fork `Worldgen` 里的预置，只换 `lastResult` 槽的编号（官方每份
  `ParameterList` 的 `ThreadLocal lastResult` 本来就独立）。同一在线快照「template pools and
  structure state」9.43 → 7.90–8.52 MB，结果逐字节不变（`chunkgen --through features` 3545088 项、
  `structure-rings` / `structures` / `structure-validity` 全 0 不一致）。见
  `docs/profiling.md` §20.5。剩下这 7.9 MB 里约 6 MB 是**每条计算线程各编译一份**的密度函数
  采样器（`SeededRouter`），见下一条。
- **一份编译好的生成器 + 丢掉路由的重写记忆**（`op-memprof` 续，2026-11-22）：用计数分配器
  实测，一个 `SeededRouter` 建成后留下 ≈1.43 MB，其中 ≈1.28 MB 是 `Compiler` 的
  `SLICE_UNIFORM_AXES` 重写记忆（用地址钉住整份密度函数图），编译完就没用了；
  一个 `chunk::Generator` ≈237 KB，而每条计算线程原本一个、结构线程再一个。现在
  `SeededRouter::new` 编译完立刻 `Compiler::release_rewrites()`，`Worldgen::shared_generator`
  按 `(设置, 种子)` 全进程缓存一份 `Arc<chunk::Generator>`，地形线程和结构线程共用（平坦世界
  要先覆盖常数列，仍按线程各建一份）。为了让生成器能跨线程共享，`material::System` 的
  陶土色带随机源改成 `prepare` 的局部变量（只在那里用一次，取数顺序不变）。实测同一
  `left` 快照「template pools and structure state」7.84 → 1.54 MB、「generators」
  1.16 → 0 MB，合计约 **−7 MB 常驻**；进程级读数在忙机器噪声里看不出来。结果逐字节不变：
  `chunkgen --through features` 3545088 项、`structures`、`structure-rings` 全 0 不一致，
  见 `docs/profiling.md` §20.6。**已知限制**：共享生成器按 `(设置, 种子)` 常驻不回收；
  `SeededRouter` 自身仍有每线程一份采样器（约 146 KB），可以再共享但收益小。
- **自然刷怪（本轮接入）**：`NaturalSpawner` 已照官方移植并接进岛的一刻，位置在随机刻之前
  （`ServerChunkCache.tickChunks` 先刷怪再跑随机刻）。群系刷怪表、`MobCategory` 上限和
  `spawn_cost` 由生成器从官方 `Biome` 的 `NATURAL_MOB_SPAWNS` 环境属性导出
  （`crates/mc-bamboo-data/src/generated/mob_spawns.rs`），不手写。已做：每刻重算计数
  （`createState`）、全局上限 `maxInstancesPerChunk * spawnableChunkCount / 289`、
  `spawn_mobs` / `spawn_monsters` 规则、`gameTime % 400 == 0` 才刷的持久类别、按玩家的上限
  （`bamboo.toml` 的 `spawn_per_player_cap`，默认开）、`getRandomPosWithin` 与
  `WORLD_SURFACE` 高度图、离玩家和出生点 24 格、`ON_GROUND` 的方块/光照/碰撞条件、
  `Monster.isDarkEnoughToSpawn`、`PathfinderMob.checkSpawnRules`（`getWalkTargetValue >= 0`）、
  `finalizeSpawn` 与按组生成（牛按 `AgeableMobGroupData` 抽幼年；马按 `HorseGroupData`、羊驼按 `LlamaGroupData` 同组共享毛色，驴和骡的幼年概率是 0.2；马科的 `getMaxSpawnClusterSize` 是 6）。刷怪区块列表用岛自己的
  世界随机源打乱（即使这一刻没有可刷的类别也照官方打乱）。**已知差异与未做**：
  - **只有已建模的生物会真的生成**（`MobType::ALL` 的 78 种；下表只列会自然生成的种类，盔甲架等只由 `/summon` 或物品放置，不出现在群系刷怪表里：牛、铜傀儡、哞菇、猪、羊、鸡、北极熊、熊猫、豹猫、猫、鹦鹉、马、驴、骡、羊驼、骆驼、尸壳骆驼、骷髅马、僵尸马、兔子、狐狸、狼、犰狳、山羊、青蛙、蝌蚪、僵尸、尸壳、骷髅、流浪者、沼骸、焦骸、史莱姆、溺尸、女巫、掠夺者、苦力怕、蜘蛛、洞穴蜘蛛、末影人、蠹虫、末影螨、铁傀儡、雪傀儡、鱿鱼、发光鱿鱼、鳕鱼、鲑鱼、热带鱼、河豚、海龟、海豚、美西螈、蝙蝠、幻翼、蜜蜂、村民、僵尸村民、守卫者、远古守卫者、硫磺方块怪、鹦鹉螺、僵尸鹦鹉螺、嗅探兽、嘎枝、监守者、卫道士、劫掠兽、旋风人、唤魔者、恼鬼、幻术师、流浪商人、行商羊驼；其中豹猫挂在丛林 / 竹林 `MONSTER` 表，猫不在群系刷怪表内，蝙蝠和幻翼各有自己的刷怪路径，掠夺者由巡逻队刷怪器和前哨站结构刷出）：群系刷怪表里其它条目仍占原版权重，抽完组大小后结束这一组。官方会继续尝试生成该物种、消耗后续随机数，并把实际生成的实体计入上限；所以**只有刷怪表里全是已建模物种的情形**逐刻产率和官方一致。`ON_GROUND` 按实体类型检查落脚块，冰和霜冰只允许北极熊通过此层，北极熊自己的群系与亮度规则仍须通过。已对拍的刷怪路径（`mob-oracle --only natural_spawn`，当前 830 例 3320 刻 0 不一致）覆盖单条目刷怪表、固定区块顺序、四档难度、日 / 夜 / 雷暴 / 火把、24/32/128 格、全局与局部上限、史莱姆区块和沼泽月相、流浪者找天和沼骸的雪地 / 沼泽群系表、焦骸的沙漠夜里 / 白天 / 遮天 / 和平四个场景、硫磺方块怪的 sulfur_caves 暗处 / 白天 / 强光 / 和平四个场景、犰狳与山羊、鱿鱼 / 发光鱿鱼 / 鱼类的水体夹具，鹦鹉的丛林白天与暗处，以及海豚 / 美西螈、青蛙 / 蝌蚪和野生动物的夹具；猪和鸡各有温带 / 暖 / 冷三种花色的白天场景（`pig_temperate`、`pig_day`、`pig_cold`、`chicken_day`、`chicken_warm`、`chicken_cold`），另比每只的花色 / 羊毛颜色和叫声，鸡的 `eggTime` 取自实体自己没播种的随机源不比，另有马、驴、羊驼各一个白天场景。水生生物的海平面由游戏层按世界类型给（超平坦 -63，噪声世界读设置，进程内只解析一次）。真实群系里混着未建模物种时，后续随机序列和产率仍会偏离，等这些生物合进来后自动对齐。幻翼不在群系刷怪表里，只由主世界的 `PhantomSpawner` 刷出；蠹虫、末影螨、铁傀儡和雪傀儡也不在群系刷怪表里，只由 `/summon`、刷怪蛋、虫蚀方块或雕刻南瓜图案生成。
  - 生成位置的实体阻挡照 `Mob.checkSpawnObstruction` 看所有 `blocksBuilding` 实体（生物、
    TNT、下落方块和玩家）；`noCollision(AABB)` 只看方块和硬碰撞实体，船与矿车都已建模，
    当前 `is_hard_colliding` 只把船算进来（矿车不算这里的硬碰撞实体）。
  - 僵尸和尸壳的 `finalizeSpawn` 已按 `Zombie` / `Husk.finalizeSpawn` 完整移植（跟随距离、左撇子、
    捡物、组内共享的幼年、鸡骑士判定、破门、盔甲与武器、附魔、万圣节头饰、`handleAttributes`、
    尸壳的二次捡物判定）。鸡骑士已生成并骑乘（见「生物骑乘」）；尸壳自然生成时照官方
    `Husk.finalizeSpawn` 在骆驼尸壳生成盒无碰撞后取 10% 概率，命中时给铁矛并用通用骑乘
    接上「尸壳 + 焦骸骑尸壳骆驼」三人组（见「焦骸和尸壳骆驼」）。
  - 未做：下界要塞的 `FORTRESS_ENEMIES`、`customSpawners` 里的猫和流浪商人、刷怪增援。
    （刷怪笼已做成活的方块实体，见上面「刷怪笼」一条。）
    结构刷怪覆盖（前哨站 / 女巫小屋 / 海底神殿）和 `PatrolSpawner` 已接入，见上面的
    「掠夺者」「巡逻队」「结构刷怪覆盖」三条。
  - `spawnableChunkCount` 取本岛玩家周围 17×17 方形的并集，不依赖区块加载状态；旁观者由
    `spectators_generate_chunks` 决定是否计入。官方使用维度级票据追踪器，跨岛与票据尚在
    交付时可能不同。
  - 刷怪列表只收已达方块 tick 等级、水平离非旁观玩家不到 128 格的区块，打乱后再检查
    实体 tick 等级。Bamboo 以坐标顺序收集，官方为哈希顺序；无限超平坦用模拟距离近似票据等级。
    官方在遍历刷怪区块时穿插雷击；Bamboo 保留既有的雷击/随机刻阶段，雷暴下取数顺序不同。
  - `doMobSpawning` 为假时整段不跑、一个随机数都不取；官方这时仍会打乱刷怪区块列表。
- **生成期刷怪（`spawnMobsForChunkGeneration`，本轮接入）**：噪声世界新生成的区块（以及盘上
  还没到 `SPAWN` 就续跑的区块）在 Director 完工组装时按查询点群系的 `CREATURE` 刷怪表和
  `CREATURE_WORLD_GEN_SPAWN_PROBABILITY`（生成器导出，0.1 / 0.07 / 0.04 / 0.03）刷动物，生物放进
  区块的实体盒，随区块交付和存盘；超平坦和 `doMobSpawning` 为假时不刷，和官方一样。装饰随机源
  （循环、选表、组大小、起点、漂移、朝向）与区域随机源（`finalizeSpawn`）分开建，查询点
  `(minX, 319, minZ)` 的全分辨率群系和 quart 群系分别决定刷怪表和概率。`mob-oracle --only gen_spawn`
  用官方平原表和公式地形（台阶、水、沙、树叶、草丛，四种光照）比对装饰随机源指纹、
  已建模动物的类型、坐标、朝向，以及区域随机源同步时的幼年、左撇子、农场动物的花色 / 羊毛颜色和叫声、
  马科的属性、变种、花色与力量；`mob-oracle --only gen_spawn_misc` 另对拍蘑菇岛与冰冻海洋的动物表，
  `--only gen_spawn_panda` 对拍竹林，都比随机源指纹与每只动物的类型、坐标、朝向、幼年和左撇子。
  **已知差异**：
  - 竹林的鹦鹉只模拟生成期放置，推进装饰随机源，不生成实体、不推进鹦鹉 `finalizeSpawn` 的区域随机源。因此同区块若先生成过鹦鹉，后续区域随机源上的属性仍可能偏离原版。
  - 真正生成的有牛、猪、羊、鸡、兔子、狐狸、狼、马、驴、骡、羊驼、犰狳、山羊、青蛙、哞菇、北极熊、熊猫，
    它们的 `finalizeSpawn` 都取区域随机源。
    海龟、骆驼、炽足兽的放置规则还没翻译，抽到时停止这个
    区块的生成期刷怪（之后的取数没法对齐）。这些生物合进来后在 `natural_gen.rs` 的放置表里接上。
    海龟虽然已经能在运行时自然生成，生成期这一支还没接。
  - 刷怪在世界线程的完工组装里跑，不在计算池的 `spawn` 阶段：那一刻计算线程还拿不到最终方块
    和光照。每个区块平均只取一两个随机数。
  - 中心区块外的格子按空气、无光照数据读（官方 `spawn` 只要求邻居到 `biomes`）；群系按纯函数
    重算，没有读区块里存的群系，R 树 `lastResult` 的平局可能和官方批量结果不同。对拍夹具用固定
    群系，查询点位置（八角插值、quart y 夹进区块高度）只有单元测试，没有和官方真实世界对拍。
  - 生成的实体 UUID 由世界种子和区块坐标派生（官方实体随机源不带种子）。
- 地物（`ChunkStatus.FEATURES`）阶段：纯函数已按 `docs/worldgen-features-plan.md` 的分类
  逐类移植，接入游戏流水线由流水线分支负责。按 5.2 的分类，非植被里已按官方 26.3
  移植并对拍 0 不一致的有 `ore`（流水线里程碑）、`disk`、`underwater_magma`、
  `spring_feature`，以及洞穴和冰雪类的 `large_dripstone`、`speleothem_cluster`
  （`dripstone_cluster` / `sulfur_spike_cluster`）、`speleothem`
  （`pointed_dripstone` / `sulfur_spike`）、`spike`（`ice_spike`）、
  `blue_ice`、`disk` 的 `ice_patch` 和 `freeze_top_layer`；它们共用 `dripstone_utils`
  （`SpeleothemUtils`、`Column.scan`、`FloatProvider`、`Mth` 助手、带属性的
  `pointed_dripstone` 状态），`WorldgenRandom` 补了 `nextGaussian`，`FeatureWorld` 带
  `seaLevel`，`placement.rs` 补了 `environment_scan`，`provider.rs` 补了
  `clamped_normal`，Java 对拍代理补了 `isWaterAt` / `isInsideBuildHeight` /
  `getBrightness`。`chunkgen` 的 `--ore-only` 已泛化成 `--types ID,ID`（也认
  `--feature-types`）诊断白名单，过滤不动 `features_per_step` 的全局 index。
  已知限制：`blue_ice` 按 `BlueIceFeature.java` 移植。它要先有相邻的 `packed_ice`
  （原版来自 `iceberg`，现在已随 main 合入），所以要把它和 `iceberg` 一起放开才比得到：
  `--types minecraft:blue_ice,minecraft:iceberg --chunk -200 117 --scan 3 --seeds 1 --salt 13`
  （种子 0 的 `frozen_ocean`）是 **0 不一致 / 0 skipped**，把 `place` 开头改成写一格 `STONE`
  会报 **54 处**，确认不是空跑。种子 -1 的 `(53,78)` 附近也是 0 不一致。
  注意默认坐标（`FIXED_CHUNKS` + 随机）里没有冻洋，单跑 `blue_ice` 会空跑。
  step 7 地下装饰里的 `sculk_vein` 由 `multiface_growth` 覆盖（代码来自 `cb-wg-aquatic`），
  在 `deep_dark` 定点对拍 0 不一致；**注意默认坐标（`FIXED_CHUNKS` + 随机）下它一次都不放方块，
  必须用 `--chunk -42 111 --scan N`（种子 0 的 `deep_dark`）才比得到**。
  `sculk_patch`（`SculkPatchFeature` + `SculkSpreader`）也已移植到
  `feature/sculk_patch.rs` 并**对拍 0 不一致**：`--steps 7 --types
  minecraft:sculk_patch_deep_dark --chunk -42 111 --scan 3` 在三种子 / 三种扫描尺寸下
  均 0 不一致 / 0 skipped（最多 27 中心 / 23887872 方块），把 `attemptPlaceSculk`
  写死的方块换成石头会报 160 处，确认不是空跑。step 7 全 7 个 placed feature
  一起放开也是 0 不一致。踩过的坑：`BlockPos.betweenClosed` 的遍历顺序是
  **x 内层、y 中层、z 外层**，按 x 外层写会让 `ChargeCursor` 的邻格洗牌从不同的
  格子起步，导致所有游标走错路径（写入总量反而更多，但位置全偏）。
  `sculk_patch_ancient_city`（古城结构专用，`placement` 为空）没做。
  其余非植被类型（`lake`、`geode`、`iceberg`、`block_blob`、`monster_room`、`fossil`、
  `overlay`）以及全部植被类型，在完整 11 步对拍里仍标 `unsupported` / `skipped`：
  `fossil` 要生成器导出结构模板、`monster_room` 要刷怪笼 / 战利品表 / 方块实体；
  植被由 `cb-wg-feat-b` 负责。
  `freeze_top_layer` 已移植并逐方块 0 不一致：它复用 `biome_temperature` 里的
  `getHeightAdjustedTemperature`，逐点群系由主循环的 `BiomeManager` 未缓存解算器给出，
  没有改 `Feature::place` 签名（在 `composition.rs` 的 `place_feature` 里单开一个分支）。
  已移植的类型都不产生方块实体（本段是早期记录）；`FeatureResult` 现在带
  方块实体和 `PostProcessing` 位置表，后者已逐段对拍并随 `build_region_column` 写进存档。
- 结构生成（`docs/worldgen-structures-plan.md`）已接进 Director（本轮）：**所有**结构的起点、引用、
  Beardifier 和 `structures` 存档键都照官方生成并存盘；`cb-wg-structures` 合入 main 后
  四个代码生成单件结构与废弃传送门的 `postProcess` 也已逐方块对拍 0 不一致，允许清单里
  所有已验证类型全部打开写方块。以下为历史对拍明细：
  `worldgen.json` 从官方导出了 `structure_set` / `structure` / `template_pool` / `processor_list`
  四个注册表。`mc-bamboo-worldgen::structure` 移植并逐格对拍了 `random_spread` 的潜在起点、
  四种频率削减（9280 个比较 0 不一致）、`createStructures` 的加权选择 / `existingStart` 短路
  （9744 个序列 0 不一致）、`createReferences` 的数据结构，以及要塞 `concentric_rings` 的
  `findBiomeHorizontal` 群系吸附和环位置（6 种子 768 个环位置、96 个 `findBiomeHorizontal`、
  864 个 `isStructureChunk` 检查全 0 不一致）。每个新对拍都先故意改坏一处确认会失败。
  `structure-validity` 又把 `Structure.generate` 的 `findValidGenerationPoint` 三件通用判定
  （整列 quart 群系成批扫描 `couldValidBiomeExistOnTopOfChunkCenter`、
  `iterateNoiseColumn` 的四角 `getFirstOccupiedHeight` / `getLowestY` /
  `getMeanFirstOccupiedHeight`、单点 `createResolver` 的 `isValidBiome`）用官方
  `Structure.GenerationContext` 逐字段对拍：6 种子 × 12 区块，比 52 个结构的合法群系名、
  360 个高度字段（38 个不同值）、1584 个整列 / 起点群系判定字段、432 个加权序列、
  288 个「起点 + 选中的结构 + 是否有效」组合字段，共 2716 个比较 0 不一致；反转整列判定、
  反转起点群系判定两处故意改坏各被抓到并失败，还原后回 0。
  已知限制：这里的「是否有效」是 `onTopOfChunkCenter` 基线路径的通用群系 / 高度判定，不是
  `Structure.generate` 的最终有效，组合对拍只覆盖 6 个 `random_spread` 结构集、不含
  `isStructureChunk` 命中过滤（那是已对拍的 `start_plans` 的职责）。
  这批对拍当时是纯函数；后来 `structure_starts` / `structure_references` 已按纯函数重算接进
  Director 流水线（`structure-pipeline` 用官方 `createStructures` / `createReferences` 对拍
  游戏里同一条路径），起点也经 `pipeline::WorldStructureStart` 带上完整结构件列表供放置阶段使用。
  本轮新增并完成对拍的里程碑：四个代码生成单件结构（沙漠神殿、丛林神殿、女巫小屋、
  埋藏的宝藏）的结构件布局（`structure-single`，8 种子 × 12 区块 × 4 类 = 384 查询、
  384 结构件、384 行比较 0 不一致）；雪屋 / 沉船 / 海底遗迹三种需要 `.nbt` 模板的单件结构
  布局（`structure-templated`，8 种子 × 12 区块 × 5 类 = 480 查询、1685 结构件、1685 行
  比较 0 不一致——雪屋覆盖 3 个模板 / 4 种 rotation / 1–12 件的深度分支，沉船覆盖 23 个
  模板（沙滩表 11 + 海洋表 20，两表重叠 8 个）、海底遗迹覆盖暖 / 冷两种群系、48 个模板、
  大遗迹 cluster 分支 3–27 件；三处故意改坏——雪屋梯子 pivot 262 处、沉船 pivot 142 处、
  海底遗迹 cluster 位置下界 68 处与冷水完好度 263 处——确认失败后还原回 0）；废弃传送门的
  结构件布局（`structure-ruined-portal`，6 个结构变体 × 6 种子 × 18 区块 = 648 查询、
  110 个结构件、110 行比较 0 不一致——覆盖五种 `VerticalPlacement`（`in_nether` 不做）、
  `Mirror.FRONT_BACK` 与 `NONE`、`can_be_cold` 真假，`findSuitableY` 走新增的
  `Generator::base_column` 整列原语；两处故意改坏——`findSuitableY` 的三格门槛改两格 23 处、
  `MIRROR_FRONT_BACK` 常量 2 改 1 68 处——确认失败后还原回 0）；存档
  `structures` 键
  （`structure-save`，8 case 的规范 NBT 0 不一致，官方 `PiecesContainer.load` / `save` 往返、
  `world-dir` 的 `SerializableChunkData.parse` 都能读带该键的区块）。`structure::post_process` 里
  四个代码生成单件结构（埋藏的宝藏、女巫小屋、沙漠神殿、丛林神殿）的 `postProcess` 方块部分
  已由 `structure-post-process` 按**写入日志**逐格对拍（比「写前写后的状态差异」严格：结构写在
  地形上的方块不算状态变化，但官方确实写了），4 种子 × 4 区块 × 4 类共 64 查询 0 不一致；
  这四类和废弃传送门的 `postProcess` 都在 `feature` 阶段的结构分支里真的写方块，
  `structure-e2e` 用官方 `ChunkgenOracle --structures` 和游戏里同一条 FEATURES 路径逐方块比
  3×3 九宫格（含沙漠神殿起点、前哨站、村庄、要塞、矿井、沉船、海底遗迹、古城、古迹废墟等）。
  另有把结构起点真正接进 Director（`Beardifier` 见下面单独一条）；
  `.nbt` 结构模板已由 `cb-wg-jigsaw` 分支从官方 jar 原样导出成
  `mc-bamboo-data::structures`（1511 个、4236847 gzip 字节，重跑无 diff），并用官方
  `NbtIo` 逐节点对拍 0 不一致；Rust 侧 `structure::template` 能读出方块、方块实体和拼图方块。
  jigsaw 结构件生成（`JigsawPlacement.addPieces`、五种 pool element、村庄 5 种 / 前哨站 /
  古城 / 试炼密室 / 古迹废墟的结构件列表与官方对拍）和 `pillager_outposts` 的排除区
  都已接进游戏里的结构路径（`structure-e2e` 有前哨站、古城和古迹废墟的定点区块）。
  整列判定与起点单点判定在这批数据上从未不一致（主世界群系沿 Y
  基本恒定），两条路径各自用故意改坏单独验过。结构刷怪覆盖已照 `ChunkGenerator.getMobsAt`
  接好（前哨站 / 女巫小屋 / 海底神殿，见生物一节的「结构刷怪覆盖」），前哨站的结构件已在正式
  世界里放置并刷出掠夺者。
- 多件式结构的结构件布局与前置判定（分支 `cb-wg-multipiece`）：
  `mc-bamboo-worldgen::structure::pieces` 把四个「用代码生成结构件列表」的结构布局**全部**
  移植成纯函数——废弃矿井（普通 / 恶地）、要塞、海底神殿、林地府邸
  （`MansionGrid` + `MansionPiecePlacer.createMansion`，模板包围盒由 `cb-wg-jigsaw` 导出的
  1511 个 `.nbt` 经 `mc-bamboo-data::structures` / `structure::template` 算出）：结构件的类型、
  包围盒、朝向、genDepth、各子类参数，以及世界随机源的取数次数和顺序逐步照官方 26.3 翻译；
  不含把方块写进世界的 `postProcess`（属放置那一步）。`structure-pieces` 对拍
  （`StructurePiecesOracle.java` 直接调官方结构件类、不启服务端 / 读存档 / 接受 EULA，
  海底神殿的 `childPieces` 走反射、林地府邸的 Java oracle 构造真实
  `StructureTemplateManager`）6 种子 × 8 区块、336 个查询、77960 个结构件逐项比
  0 不一致；四个结构各自先故意改坏一处确认失败（矿井丢弃首个 `nextDouble`、要塞丢弃
  `randomSmallDoor` 的 `nextInt(5)`、海底神殿扫描循环 5 改 4、林地府邸用错房间名 /
  多取一次随机）再还原。
  `mc-bamboo-worldgen::structure::start_point` 又移植四个结构各自的 `findGenerationPoint`
  前置判定（`findValidGenerationPoint` = stub 存在且 `isValidBiome(stub)`）：恶地矿井
  `getBaseHeight(WORLD_SURFACE_WG)`、海底神殿 `getBiomesWithin` 周边海洋 +
  `OCEAN_FLOOR_WG`、要塞 `getWorldPosition()` 起点、林地府邸四角最低高度 +
  `startPos.getY() < 60`；`chunk.rs` 加 `first_occupied_height_ocean_floor` 原语。新对拍
  `cargo run -p mc-bamboo-vanilla -- structure-generation <java>` 用官方
  `Structure.findValidGenerationPoint` 造 96 个查询、480 行、5286 个字段逐项比，
  0 不一致；故意丢掉 `isValidBiome` 过滤立刻 192 处不一致，还原回 0。
  海底神殿「只存一个 `MonumentBuilding`、读回 `regeneratePiecesAfterLoad` 重建房间图」的
  存档口径写在 `docs/worldgen-structures-plan.md` 4.1。
  存档这一块（第 7 步里属于这四个结构的部分）也已落地：`structure::pieces::Piece` 的
  子类字段带 NBT 类型（`Extra::Byte/Int/Float/Str`），`structure::save::SavePiece::from_layout`
  逐键写出官方类型（布尔 Byte、`Num`/`Type`/`Steps` Int、矿井 `D` 走
  `Direction.LEGACY_ID_CODEC_2D` 的 Byte、模板名 String，`Entrances` 是
  `BoundingBox.CODEC.listOf()` 的 IntArray 列表）；读回侧移植了
  `PiecesContainer.load`（jigsaw 的 `RENAMES`、未知 id 与坏标签跳过）、
  `StructureStart.loadStaticStart` 的字段集（`id = "INVALID"` 返回空）以及
  `OceanMonumentStructure.regeneratePiecesAfterLoad`（先取一次 `getRandomDirection` 对齐
  取数位置，再用存档 `BB` 的 `minX/minZ` 与存档朝向重建 `MonumentBuilding`，与生成共用
  同一个 `monument_building` 构造器）。新对拍
  `cargo run -p mc-bamboo-vanilla -- structure-load <java>`：80 个查询、8218 个读回结构件、
  656 行海底神殿重建逐项比 0 不一致；把重建里的方向取数换成常量会报 610 处不一致。
  **改坏验证发现的真 bug**：`structure-pieces` 原来把子类存档值压平成十进制，字节与整型
  写错也发现不了；现在类型进 token（`B:` / `I:`），矿井 `D` 按官方写成 Byte 之前会报
  3312 处不一致。
  矿井和要塞的结构件放置（`postProcess`）已移植，`structure-place` 对拍 0 不一致，并已随
  `PLACEABLE_TYPES` 允许清单接进 `feature` 阶段的结构分支：
  * `structure::piece_place`：官方 `StructurePiece` 的放置辅助（`getWorldPos` /
    `placeBlock` / `generateBox` 含 `BlockSelector` 重载 / `generateMaybeBox` /
    `maybeGenerateBlock` / `generateUpperHalfSphere` / `fillColumnDown` / `isInterior` /
    `isReplaceableByStructures`），用 `PieceLevel` trait 抽象世界读写。
  * `structure::state_transform`：`placeBlock` 里的 `BlockState.mirror` / `rotate`，
    覆盖栅栏（`CrossCollisionBlock` 四面）、墙上火把与按钮（`HorizontalDirectionalBlock` 的
    `facing`）、门（`DoorBlock`，镜像时还要 `cycle(HINGE)`）、铁轨（十种 `RailShape`）。
  * `structure::pieces::mineshaft_place`：矿井四种结构件（走廊 / 交叉口 / 楼梯 / 房间）的
    `postProcess`。`structure-place` 对拍（`StructurePlaceOracle.java` 在假
    `WorldGenLevel` 上跑官方 `postProcess`）**0 不一致**（普通 / 恶地各 32 条查询）。
  * `structure::pieces::stronghold_place`：要塞十二种结构件（`ChestCorridor` /
    `FillerCorridor` / `Straight` / `PrisonHall` / `LeftTurn` / `RightTurn` /
    `RoomCrossing` 三种 layout / `StairsDown` / `StraightStairsDown` / `FiveCrossing` /
    `Library` / `PortalRoom`）的 `postProcess`，含 `SmoothStoneSelector` 与四种
    `SmallDoorType`。`structure-place` 对拍 **0 不一致**（32 条查询，十二种结构件都放到）。
  * 四个代码生成单件结构的 `postProcess`（埋藏的宝藏 / 女巫小屋 / 沙漠神殿 / 丛林神殿）
    走另一个对拍子命令 `structure-post-process`（`StructurePlaceOracle.java`，按写入日志逐格比）：
    4 种子 × 4 区块 × 4 类共 64 查询 0 不一致（女巫小屋生成实体那一支按官方 abort，方块部分两边都写完）。
  * 对拍口径说明：`structure-place` 对整个结构碰到的每个区块调一次 `postProcess`，比
    结构件清单、每次调用的写入次数（`STRUCTURE_PLACE_TRACE=1` 时比按顺序的每一次写入）
    和每个写过的位置的最终状态，挖空也比。最终状态里空气家族合并成 `AIR`：
    `ProtoChunk.getBlockState` 对全空气的 section 直接返回 `air`，读不出 `cave_air`。
  已知限制：布局函数里恶地的 `surfaceHeight` / 府邸的 `startY` 在对拍中仍作为外部输入喂入
  （前置判定模块已用 `Generator` 的高度原语算出并喂给布局）；jigsaw 与其余旧式结构的结构件
  布局和放置后来都已接进 Director：`structure_starts` 从盘上 `structures` 标签恢复或按
  `createStructures` 纯函数重算，`feature` 阶段按引用反查起点、逐个 decoration step 先放
  结构件再放地物，读盘续生成也把 `structures` 标签装回交付盒子（`resume_replay`）。
  Beardifier 见下面单独一条。
- jigsaw 结构件生成（分支 `cb-wg-jigsaw`）：`mc-bamboo-worldgen::structure::jigsaw` 把
  `JigsawStructure.findGenerationPoint` + `JigsawPlacement.addPieces` 整条递归移植成纯函数，
  照官方 26.3 的取数顺序翻译：`HeightProvider.sample`（`constant` / `uniform`）、
  `Rotation.getRandom` / `getShuffled`、`StructureTemplateManager.getOrCreate`（模板按 id
  缓存）、`StructurePlaceSettings.getRandomPalette`（多 palette 模板按**世界位置**的
  `Mth.getSeed` 选，不消耗世界随机源）、`Palette.jigsaws()` 的 `(y, x, z)` 顺序 +
  `Util.shuffle` + 稳定按 `selectionPriority` 降序、`PoolAliasLookup.create`（`direct` /
  `random` / `random_group` 共用一条 `RandomSource.create(seed).forkPositional().at(pos)`）、
  `StructureTemplatePool.getRandomTemplate` / `getShuffledTemplates` / `getMaxSize`、
  `Rotation.getShuffled`、`canAttach`、`Shapes` 的 `join` / `joinIsNotEmpty` /
  `joinUnoptimized`（`AABB.deflate(0.25)`，用乘 4 整数精确表示）、
  `SequencedPriorityIterator`、`project_start_to_heightmap` / `getFirstFreeHeight`、
  `isStartTooCloseToWorldHeightLimits` 与 `doExpansionHack`。五种 pool element
  （`single` / `legacy_single` / `list` / `feature` / `empty`）都支持。
  对拍 `cargo run -p mc-bamboo-vanilla -- jigsaw <java> [templates|jigsaws|pieces|pools|save|shapes]`
  用官方 `StructureTemplateManager`（真实数据包）与 `Structure.GenerationContext`：
  `pieces` 覆盖村庄 5 种、掠夺者前哨站、古城、试炼密室、古迹废墟和 26.3 新增的全部
  18 种 `abandoned_camp`，3 种子 × 4 区块 × 27 结构 = 324 个查询、4033 个结构件的
  类型 / 模板名 / 位置 / 旋转 / 包围盒 / `groundLevelDelta` / `getGenDepth` /
  `liquidSettings` / **每个 junction 的全部字段**，以及 Q 行的 `isValidBiome(stub)`
  （`findValidGenerationPoint` 的最后一道门，14/324 为 true，真假两支都比到）逐项
  0 不一致；`pools` 把 245 个 `template_pool` 每一个都当起始 pool 直接调官方
  `addPieces`（2 种子 × 2 区块 = 980 查询、8976 结构件 0 不一致），把
  `feature_pool_element`（639 件）和 `list_pool_element`（15 件）当父件的递归路径真的比到；
  `save` 用真实结构件跑官方 `PoolElementStructurePiece.createTag`，逐 Children 条目比完整
  规范 NBT（`PosX`/`PosY`/`PosZ`、`ground_level_delta`、`pool_element`、`rotation`、
  `junctions`、非默认才写的 `liquid_settings`），108 查询、1216 个件标签 0 不一致；
  `load` 把每个结构件 `createTag` 后用官方 `StructurePieceType.load` 读回，与 Rust 的
  `Piece::from_save_tag` 逐项比位置 / 旋转 / **重算的包围盒** / `groundLevelDelta` /
  junction 数 / 液体设置，108 查询、1216 个结构件往返 0 不一致；
  `processors` 用官方 `StructureProcessorType.DIRECT_CODEC` 整表编码 40 个 `processor_list`
  （52 个 processor），逐键 0 不一致——这一条覆盖 26.3 的四种 processor（`block_rot` /
  `rule` / `protected_blocks` / `capped`）、五种 `RuleTest`、`axis_aligned_linear_pos` 和
  `append_loot`；
  `apply` 在假世界上用官方 `StructureTemplate.processBlockInfos` 跑 40 个列表
  （200 查询 / 941 个存活方块，其中 80 条带 `chunkBB`）与 `structure::processor_apply`
  逐块 0 不一致，覆盖了 `processOnlyInCurrentChunk` 的两支；
  `capped` 复刻 `CappedProcessor.finalizeProcessing` 的随机源派生
  （`createThreadLocalInstance(seed).forkPositional().at(piece)`）、`Util.toShuffledList`
  的洗牌序列与 `limit.sample`，75 查询 0 不一致；
  `cappedfull` 用 `Unsafe.allocateInstance` 造出 `getSeed()` 返回真实种子的
  `ServerLevel`（链路 `server.worldGenSettings.options.seed`，三层 final 字段反射写入），
  于是**完整的 `finalizeProcessing`** 会真的执行限额替换：24 查询 / 192 方块 0 不一致
  （砾石被 `capped` 规则换成可疑的沙砾，实际替换 72 次，说明限额真的生效）；
  `placepiece` 把**整条模板放置链路**（按世界位置选 palette → 位置变换含 mirror 与
  rotation → processor 链 → 带 nbt 的方块先写 `BARRIER` → `chunkBB` 过滤 → 状态
  mirror/rotate）与官方 `StructureTemplate.placeInWorld` 逐写入比：900 查询
  （8 个单 palette 模板 × 全部 12 种 mirror/rotation 组合）、610992 次写入 0 不一致；
  `jigsaws` 对 1511 个模板 × 4 种旋转 × 4 个位置 = 24176 个查询、
  68352 个拼图方块的位置 / front / top / name / target / pool / joint / 两个优先级逐项
  0 不一致；`templates` 1511 个模板的尺寸 / 方块数 / 拼图方块数 0 不一致；`shapes`
  120 个场景 + 4 个 deflate case 0 不一致。故意改坏各自被抓到并失败，还原回 0：
  别名随机源位置 +1、拼图方块 orientation 写死 `north_up`、junction 的 `sourceGroundY` +1、
  palette 选择、`isValidBiome` 取反（26/324）、expansion hack 的 `expandTo + 1` 改 `+2`
  （82/980）、`rotation` 写成 `CODEC` 的 `none`（对拍立刻报差异）、`BlockState` 的
  `id` 写成 `Name`（5/40）、把内联 `processors` 的 `{processors: [...]}` 外壳去掉
  （`save` 模式报 789/108）。
  这批对拍当时是纯函数 + oracle；现在 `structure_starts` 已由 Director 流水线里的纯函数
  重算驱动（`structure-pipeline` 逐项比官方 `createStructures` / `createReferences`），
  jigsaw 结构件随 `feature` 阶段的结构分支写进游戏。已知限制：`Piece` 带结构件列表
  （元素、位置、旋转、包围盒、`groundLevelDelta`、junctions、
  `liquid_settings`），方块写入（`placeInChunk` / `postProcess` / processors / 拼图替换 /
  方块实体）属放置那一步，`Piece::to_full_save_piece` 给出 `pool_element` / `rotation` /
  `junctions` / `liquid_settings` 全部子类字段的存档标签，`processor_list` 的 40 个条目也已
  按官方 codec 解析成类型化的 `structure::processor::Processor` 并逐键对拍过编码；
  除了 `processor_list` 里的四种（`block_rot` / `rule` / `protected_blocks` / `capped`），
  放置阶段硬编码的三种（`block_ignore` / `jigsaw_replacement` / `gravity`）也已移植。
  `TemplatePools` 在解析时就把每个 pool 的 `maxSize` 一次算好（官方懒算，结果相同），
  因此它是 `Sync + Send`，可以放进 `Arc` 供多线程区块生成共享。
  `start_height` 只支持 `constant`（absolute）与 `uniform`——26.3 主世界全部 28 个 jigsaw
  结构只用这两种，其余 provider 标 unsupported；`bastion_remnant` 是下界结构，按分工不做。
  `liquid_settings` 已随 `Piece` 带到放置阶段（`SinglePoolElement` 用
  `overrideLiquidSettings.orElse(liquidSettings)`），结构件生成本身不读它。
  `Shapes` 按 0.25 的倍数用整数精确复刻，若将来有非 0.25 倍的 AABB 进来（目前没有）
  会失去精度。
  对拍 `cargo run -p mc-bamboo-vanilla -- jigsaw <java> [templates|jigsaws|pieces|shapes]`
  用官方 `StructureTemplateManager`（真实数据包）与 `Structure.GenerationContext`：
  `pieces` 覆盖村庄 5 种、掠夺者前哨站、古城、试炼密室、古迹废墟和 26.3 新增的全部
  18 种 `abandoned_camp`，6 种子 × 6 区块 × 27 结构 = 972 个查询、12467 个结构件的
  类型 / 模板名 / 位置 / 旋转 / 包围盒 / `groundLevelDelta` / **每个 junction 的全部字段**
  逐项 0 不一致；`jigsaws` 对 1511 个模板 × 4 种旋转 × 4 个位置 = 24176 个查询、
  68352 个拼图方块的位置 / front / top / name / target / pool / joint / 两个优先级逐项
  0 不一致；`templates` 1511 个模板的尺寸 / 方块数 / 拼图方块数 0 不一致；`shapes`
  120 个场景 + 4 个 deflate case 0 不一致。四处故意改坏（别名随机源位置 +1、拼图方块
  orientation 写死 `north_up`、junction 的 `sourceGroundY` +1、`pieces` 的 palette 选择）
  各自被抓到并失败，还原回 0。
  这一条是早期记录：上面那批 324 查询的对拍已经补上 `save` / `load` / `processors` /
  `apply` / `placepiece` 等子命令，`Piece::to_full_save_piece` 也给出 `pool_element` /
  `rotation` / `junctions` / `liquid_settings` 全部子类字段；结构件生成和放置都已接进
  Director 流水线（见上面「结构生成」一条）。`Piece` 只带结构件列表（元素、位置、旋转、
  包围盒、`groundLevelDelta`、junctions），方块写入（`placeInChunk` / `postProcess` /
  processors / 拼图替换 / 方块实体）属放置那一步。
  `start_height` 只支持 `constant`（absolute）与 `uniform`——26.3 主世界全部 28 个 jigsaw
  结构只用这两种，其余 provider 标 unsupported；`bastion_remnant` 是下界结构，按分工不做。
  `liquid_settings` 只解析不参与结构件生成（官方也要到放置阶段才用）。`Shapes` 按 0.25
  的倍数用整数精确复刻，若将来有非 0.25 倍的 AABB 进来（目前没有）会失去精度。
- 地物（features）阶段的纯函数正按官方 `ChunkGenerator.applyBiomeDecoration` 逐类移植，
  游戏里的接入由流水线分支（邻居快照 + `features` 阶段）负责。已逐方块对拍
  0 不一致的有 step 6 的矿石、step 9 的植被子集，以及水下植被与 `vegetation_patch`
  （详见 `docs/worldgen-features-plan.md` 5.1）；其余类型仍明确标 `unsupported`，对拍时
  整块跳过，不假装支持。已移植并接入：`simple_block` 的内联 bare-state 与 `weighted` /
  `randomized_int` / `random_block` typed provider、holder 字符串引用（本轮补导出
  `block_state_provider` 注册表）、一批继承 `VegetationBlock` 的存活判定（含
  `LeafLitterBlock` 的 `isFaceSturdy(UP)`）、双高植物 `DoublePlantBlock.placeAt`、
  `would_survive` / `has_sturdy_face` / `matching_fluids` 谓词、组合类
  （`random_selector` / `weighted_random_selector` / `simple_random_selector` /
  `random_boolean_selector` / `sequence`，递归 `PlacedFeature.place`）、`block_column`
  （仙人掌 / 甘蔗 / 海带 / 洞穴藤蔓）、
  `vines`、`bamboo`、`multiface_growth`（发光地衣 / 幽匿脉络）、`vegetation_patch`
  （苔藓 / 苍白苔藓），以及 `environment_scan` / `surface_relative_threshold_filter` /
  `noise_based_count` / `random_chance` 修饰器和 `clamped` / `clamped_normal` /
  `weighted_list` IntProvider。蘑菇的亮度判据（`FEATURES` 阶段 `getRawBrightness` 恒为 15，
  只剩 `overrides_mushroom_light_requirement` 标签生效）、睡莲、孢子花、海泡菜、海草、
  地毯的 `canSurvive` 也都已移植。
  非植被那一批（`block_blob` / `geode` / `iceberg` / `lake` / `monster_room` /
  `spring_feature` / `disk` / `underwater_magma` / `large_dripstone` / `speleothem` /
  `speleothem_cluster` / `spike` / `blue_ice` / `freeze_top_layer`）由
  `cb-wg-feat-a` 与 `cb-wg-caves` 移植，逐方块对拍 0 不一致。
  诚实的已知差异与未做：藓类 patch 的
  地表集合用 Java `HashSet` 的迭代顺序决定植被随机数分配；本实现只复刻链表与容量翻倍，
  **桶内 8 个以上元素且容量 ≥ 64 时官方会转红黑树、顺序不同** —— 这种情况直接置
  `treeified` 标记，把整次放置标 `unsupported`（对拍跳过该区块）。这是有意的取舍，
  红黑树的迭代序不做复刻；
  `waterlogged_vegetation_patch`（`clay_pool_with_dripleaves`）已接到
  `lush_caves_clay` 上并逐方块对拍，改坏 `is_exposed` 会报 160 处；
  `noise_threshold_count` / `count_on_every_layer` / `cuboid` 修饰器。
  树（`tree` 全部变种、`fallen_tree`、巨型蘑菇）已由 `cb-wg-trees` 合入 main；
  `projected_random_patchy_square` 在 26.3 导出数据里没有条目。
  组合类里只有 `simple_random_selector`（forest_flowers / flower_forest_flowers）
  拿到 0 不一致对拍，其余组合类的 place 逻辑按官方源码移植但未逐方块验证。此前 terrain 含水层在个别邻居区块（seed -1、chunk (52,-26)、
  y=-54）与官方 3 格不一致，现已修复：补上官方 `Aquifer.NoiseBasedAquifer.computeSubstance`
  在「最近格子给的是水、正下方全局流体是岩浆」时跳过压力计算直接返回水的短路，并
  对齐 `shouldScheduleFluidUpdate` 的 similarity 门槛。该短路只影响岩浆层上边界 y=-54，
  只在连续的洞穴含水层区域里出现，稀疏的随机区块扫描碰不到，所以加了以指定区块为中心的
  连续网格扫描模式（`chunkgen --chunk X Z --scan N`）和单区块回归测试验证；
  多种子、多 salt、含负坐标与远坐标的地形阶段扫描已逐方块对拍 0 不一致。
- 树（`mc-bamboo-worldgen::feature::tree`）：`tree`、`fallen_tree`、`huge_brown_mushroom`、
  `huge_red_mushroom` 已照官方移植并在游戏里放置。十种树干放置器、十二种树叶放置器、
  红树根放置器、十种装饰器（树干藤蔓、树叶藤蔓、可可、蜂巢、灰化土、树叶附着物、原木
  附着物、地面落叶、层架蘑菇、嘎枝之心）都有。官方用 `HashSet<BlockPos>` 记树干和树叶，
  `updateLeaves` 和装饰器的结果取决于它的迭代顺序，所以 `tree/java_set.rs` 照
  `java.util.HashMap` 的规则重放这个顺序。放完之后的 `updateShapeAtEdge` 通过
  `feature/shape_update.rs` 直接调用 `mc-bamboo-blocks` 里方块自己的 `updateShape`。
  对拍：`chunkgen --through features` 放开全部 26 组含树的 placed feature，随机 156 个
  中心区块 0 不一致；红树林沼泽、斑驳森林、黑森林、蘑菇岛各取 3×3 的定点区块
  （共 72 个中心）0 不一致；把树叶 `distance`、红树根、白杨横向原木各故意改坏一处都会
  报不一致。苍白橡树的 `pale_moss`（含递归 `pale_moss_patch`）也已移植；种子 0
  的苍郁花园定点 9 个中心、7,962,624 方块 0 不一致。蜂巢里的蜜蜂、战利品这类方块
  实体内容没有对拍，只照取了随机数；`HashSet` 的一个桶超过 8 个元素时 Java 会把它变成
  红黑树，这种情况没有复现，只置了标记（树的坐标聚在一起，哈希几乎不冲突）。
- Beardifier / 结构对地形密度的影响（分支 `cb-wg-beard`）：`mc-bamboo-worldgen::beardifier`
  逐行移植官方 `net.minecraft.world.level.levelgen.Beardifier`：五种 `TerrainAdjustment`
  （`none` / `bury` / `beard_thin` / `beard_box` / `encapsulate`）、`BEARD_KERNEL`（24³
  个 `e^(-dist²/16)`，构造期算好）、`getBeardContribution` / `getBuryContribution`
  （含 `Mth.fastInvSqrt`）、`sampleValueUnchecked`（`compute`）和 `sampleVolume`（`fillArray`
  的 z→x→y 成批求值），以及 `forStructuresInChunk` 的结构件 / jigsaw 连接点收集。
  `finalDensity` 里的 `beardifier` 标记节点现在像官方 `ContextBoundSampler` 一样在求值时
  从 `SamplerContext` 取本区块 Beardifier；`Generator::terrain_with_beardifier` 把它注进去，
  `first_occupied_height` / `iterateNoiseColumn` 始终固定走 `EMPTY` 那支，结构为空时
  `finalDensity` 逐位不变（`chunkgen --through carvers` 仍 0 不一致）。输入类型
  `StructureStartInput` / `PieceInput` / `Rigid` / `Junction` 设计成能直接接收
  `structure_starts` 产出的结构件，留给合并时接上。对拍
  `cargo run -p mc-bamboo-vanilla -- beard <java>`：Java 侧用官方 Beardifier 的
  `@VisibleForTesting` 构造器从导出的盒 + 调整类型 + 连接点造真实 Beardifier，连进
  `NoiseChunk` 跑 `doFill` / `buildSurface` / `generateCarvers`；Rust 侧从同一份输入重建
  Beardifier，逐位比较 `BEARD_KERNEL` 原始位模式、整块 `sampleValue` / `sampleVolume`、
  `affectedBox`，以及 NOISE / SURFACE / CARVERS 三个阶段的方块。12 用例（含空、五种
  adjustment、junction、跨区块 / 贴顶贴底边界）共 5,912,076 个比较 0 不一致；
  `--break-self-test` 故意改坏一个 `groundLevelDelta` 报 32 处不一致并失败。
  **已知限制**：Java 侧的盒是按真实结构尺度构造的代表性输入，覆盖全部 `TerrainAdjustment`，
  但**不是** `Structure.generate` 跑出来的真实结构起点（本轮不重复实现）。Beardifier 的输入
  本就只是包围盒 + 调整类型 + 连接点，因此对密度计算是完整逐位对拍；`structure_starts`
  接进流水线后，`generate_terrain_stage` 从半径 8 的定稿起点 / 引用构造真实 Beardifier
  注进 `finalDensity`，`structure-e2e` 在村庄 / 前哨 / 要塞等定点区块逐方块比过这条路径。
  `forStructuresInChunk` 的收集（`isCloseToChunk`、RIGID projection、junction XZ 过滤）
  只有 Rust 单元测试覆盖，没有单独和官方 `StructureManager` 端到端对拍。
- 地物（非植被，分支 `cb-wg-feat-a`）：`block_blob`（森林岩石）、`disk`、
  `spring_feature`、`underwater_magma`、`iceberg`（含 `blue_ice` 变体）、`geode`
  （紫水晶晶洞）、`lake`（岩浆湖）、`large_dripstone`（大型滴水石柱）、`blue_ice`
  （冻洋蓝冰）、`spike`（冰刺）、`monster_room`（地牢）、`speleothem_cluster`
  （滴水石簇 / 硫磺尖簇）、`freeze_top_layer`（顶层结冰与积雪）已按官方 26.3 移植并
  逐方块对拍 0 不一致；`chunkgen` 的 `--ore-only` 已泛化成 `--feature-types ID,ID`
  诊断白名单（条目可以是 Feature 类型 ID 或 placed feature 注册名），过滤不动
  `features_per_step` 的全局 index。其余非植被类型（化石、沙漠井）在完整 11 步对拍里
  仍标 `unsupported` / `skipped`，按「不发明行为」先留白。`geode` 的
  `rarity_filter` 是 1/24、`large_dripstone` 只在稀疏的 `dripstone_caves` 群系里出现、
  `blue_ice` 只在冻洋且要求相邻浮冰、`spike` 只在 `ice_spikes` 类群系、
  `speleothem_cluster` 只在滴水石洞穴里且要原点位于空腔、`freeze_top_layer` 只在
  冷群系（温度 < 0.15）真的结冰下雪，这些都要先扫出真实生成点再 `--chunk X Z --scan N`
  复核（仓库里有三个 `#[ignore]` 探针）；`lake` 的主世界两个变体都是岩浆湖，官方
  「水做的湖表面结冰」分支（`Biome.shouldFreeze`）因此不会进入，Rust 侧对非水流体
  同样跳过。
  这些类型的实际生成路径都已逐方块对拍到（`large_dripstone` 在滴水石洞穴里找到
  4 个真的长柱的区块，`--chunk -53 -206` / `-50 -200` / `-52 -204` / `-49 -199`；
  `freeze_top_layer` 在 `--chunk 255 -109` / `260 -106` / `268 -300` / `242 -104`
  四个冷群系区块上各 0 不一致）。
  还没验证过、因此**没有**接进 dispatch 的：`overlay`（沙漠井）与 `fossil`
  —— 两者都依赖 `minecraft:template` 地物，而这个 Feature **无法用当前的 chunkgen
  oracle 对拍**：`TemplateFeature.place` 要
  `level.getLevel().getServer().getStructureTemplateManager()`，oracle 的
  `WorldGenLevel` 是动态代理，cast 到 `ServerLevel` 会抛 `ClassCastException`。
  `template_feature.rs`（加权选模板、`Util.getRandom(rotations)`、`getRotatedOffset`
  居中、`state_transform` 的镜像/旋转、`rule` processor 的 `block_match` /
  `always_true` / `append_loot`）、`overlay` 组合类、`volume_match` 谓词、
  `randomly_selected` 修饰器都已按官方写好，但**没有一次真正的逐方块比较**。
  **`freeze_top_layer` 的已知前提**：features 阶段没有生成期光照，官方
  `Biome.shouldFreeze` / `shouldSnow` 里的 `getBrightness(BLOCK) < 10` 恒成立；
  Rust 侧不查亮度，`ChunkgenOracle` 的代理把方块光返回 0、天空光返回 15。
- 红石的已知差异：发射器射箭、药水箭和光灵箭时生成真的箭实体，事件是 1002；烟花火箭也生成真的烟花实体，事件是 1004；雪球、蛋（含蓝蛋、棕蛋）、喷溅药水、滞留药水、经验瓶、火焰弹和风弹仍走默认抛出，官方那些是弹射物，这些弹射物出现事件 1002、1004、1018 或 1051 时，方块 oracle 仍排除整题；箭触发的 1002 会比较。默认抛出和余料都生成真的物品实体。发射器会按铁轨位置生成矿车。船可放置时只扣物品，无法放置时走默认抛出。带 `entity_data` 的刷怪蛋留在槽里，当作生成失败，数量变少的整题不比较（盔甲架已真的生成实体并逐项比较）。TNT 物品在 `tnt_explodes` 为真时生成点燃的 TNT 并播放 `entity.tnt.primed`，数量减一；规则为假时物品留着、发 1001，不取朝向。打火石点着 TNT 方块时走 `TntBlock.prime` 再 `removeBlock` 再扣耐久。这些题比较方块、声音、容器和 `level.random`。点燃的 TNT 实体本身不进比较，方块 oracle 也不 tick 它。同一题里如果还有弹射物事件或刷怪蛋离开发射器，仍按那些原因整题排除。鱼桶倒水后会生成鱼，但方块 oracle 的假世界不 tick 实体，生物自己的环境音不进比较。倒成功后仍取官方 `EntityType.create` 的一次 `nextFloat`，以及 `Mob.finalizeSpawn` 的 `triangle` 和一次 `nextFloat`。美西螈在 `BUCKET` 原因下 `finalizeSpawn` 直接返回，只取朝向那一次。热带鱼再按 `nextFloat() < 0.9` 取常见变种或随机花纹。硫磺立方桶不放方块，生成用的随机数同样照取。剪刀收满蜜的蜂巢按 `minecraft:harvest/beehive` 掉 3 个蜜脾，战利品表走自己的 `random_sequence`，不碰 `level.random`；`Block.popResource` 为这一组取 3 次位置偏移，生成物品实体。桶倒进可替换方块时，官方 `destroyBlock` 会先在方块还在时按 `getDrops` 一组一组 `popResource`，再换成流体方块；方块 oracle 把那些表换成空表，两边都不取数，也不生成物品。骨粉催树苗、杜鹃、蘑菇、菌、草方块、菌丝和要地物的方块时整题不比较。默认抛出和塞不回发射器的余料都生成物品实体。速度按官方 `spawnItem`：先取一个 `nextDouble` 算 `pow`，X、Z 的均值是朝向步长乘 `pow`，Y 是 0.2，再取三个 `triangle` 的六个 double；这套随机数只取一次。打火石和剪刀的耐久走通用 `hurtAndBreak`，耐久附魔用 `level.random`。玩家没有身体和鞍这两格。骷髅头、僵尸头、苦力怕头、龙首和猪灵头不放方块，只试着穿到前方的人身上。合成器按官方逻辑合成并吐出产物，朝空气吐出的东西按 `DefaultDispenseItemBehavior.spawnItem` 生成真的物品实体（精度 6），成就和地图的后处理也没做。方块 oracle 给合成器加了固定场景的题，合成成功的题逐项比较方块状态、容器内容、等级事件和世界随机源最后的种子；合成器到期时如果官方没有合成成功，只发失败事件 1050，这一声照常比较。漏斗把同一件物品搬进搬出不算合成。投掷器朝空气抛出的物品生成物品实体；压力板、探测铁轨、绊线和绊线钩、标靶、幽匿感测体、避雷针要实体、弹射物、振动或雷击才触发，地上的物品还没有接上这些触发，生物和箭没有接上这些方块触发，下落方块和点燃的 TNT 也不会踩这些；阳光探测器不随时间变化，只在右键时按当前天空光算；方块 oracle 默认正午，另有 `--clock-ticks 18000` 的夜晚批。唱片机放进唱片后立刻算在放（`ownSignal` 为 15），但不跑官方的歌曲计时：假世界也不登记唱片机的 ticker，所以不抽每 20 刻的 `nextInt(4)`，歌也不会放到结束。宝库只在官方结果里出现宝库方块实体的声音或状态事件 3015–3017 时整题不比较，没触发的宝库照常比较。
- 活塞不推实体，也不推动玩家。下落方块和点燃的 TNT 已经在世界里，但官方 `PistonMovingBlockEntity` 的
  `moveCollidedEntities`、`moveStuckEntities`、`fixEntityWithinPistonBase` 三段没实现：掉落物、下落方块
  和点燃的 TNT 不会被活塞推动，也不会被粘液块、蜂蜜块带着走。移动中的活塞按方块实体算出的形状挡住实体；
  实体在方块内的形状默认整格，潜影盒盖、细雪（皮革靴 / 潜行）和脚手架按实体状态取形状，
  精确和并行两条驱动都读同一份（官方 `entity-oracle` / `move-oracle` 有对应场景）。
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
  也按官方 `hurtEntities` 伤害生物和玩家（伤害类型 `explosion`，生物按爆炸击退抗性被推开，玩家那份击退
  向量放进 `ClientboundExplodePacket`），并照 `ServerLevel.explode` 把爆炸包发给 64 格内的玩家；
  包里的爆炸声和粒子走这个包，不在 `playSound`。爆炸中心是 `PrimedTnt` 的 `getY(0.0625)`
  （实体高 0.98，抬升 0.06125），强度 4。伤害来源没有 owner 时是 `explosion`（`direct` 指 TNT、
  `causing` 为空），玩家用打火石 / 火焰弹点着时是 `player_explosion`（`direct` 指 TNT、`causing`
  指点它的玩家）。对拍：`mob-oracle --only tnt_blast` 三个难度逐刻比 TNT 从 y=70 落到基岩地面的
  位置、引信（80→0）、爆炸那刻牛的血量和击退速度、玩家包里的击退向量和方块数，0 不一致；
  `--break-self-test explode_tnt_hurt`（不调 `hurtEntities`）和 `explode_tnt_center`（丢 0.0625 抬升）
  两处都报不一致。被 TNT 直接炸死的玩家，死亡消息照官方带上第二个来源实体
  （`entity.minecraft.tnt` + `show_entity`）；点着它的玩家按 `player_explosion` 出现在消息里。
  TNT 实体本身没有 UUID，Bamboo 的 `show_entity` 用零 UUID，官方是随机 UUID，所以这一项没有对拍；
  场景里玩家站得够远不会死。
  TNT 爆炸在换成空气之前按 `getDrops` 收集掉落，整圈方块处理完再 `popResource`；`tntExplosionDropDecay` 默认是假，
  所以不传爆炸半径，`explosion_decay` 不取数。TNT 自己的 `dropFromExplosion` 是假，不掉 TNT 物品。
  容器被炸开时，`dropContents` 仍在这一圈里、在后面 TNT 的引信之前取 `level.random`。不调用 `spawnAfterBreak`：
  矿石和刷怪笼的经验随机数不取，虫蚀方块也不生成蠹虫。掉落物上的战利品组件按统一的
  `JSON → NBT → 组件补丁` 转换写回，解不开的那一项记一条警告后丢掉。方块掉落求值时把方块实体
  的 `collectComponents` 结果作为 `BLOCK_ENTITY` 参数传给 `copy_components`，所以装了东西、改过名的潜影盒
  （以及带图案的旗帜、带纹样的陶罐）炸开后掉出来的物品仍带着那些组件；创造模式挖非空潜影盒按官方
  `playerWillDestroy` 直接掉出带内容的盒子，空盒子不掉。
  游戏规则 `tnt_explodes` 为假时不点燃、不爆炸，也不取朝向和短引信的随机数。默认是真。非玩家实体的 ID 从 16777216 起，不和玩家共用计数器。
  未氧化的铜傀儡雕像被斧头点中时方块消失、播放 `entity.copper_golem.spawn` 并在方块中心生成一只铜傀儡（朝向取雕像 `FACING`）。
- 流体只有主世界的流动速度。源头转换读 `water_source_conversion`（默认真）和 `lava_source_conversion`（默认假）。岩浆会点燃旁边的方块，岩浆变高时官方随机地慢四倍，
  Bamboo 用同一个随机源。鱼桶现在会生成鱼，但方块 oracle 的假世界不 tick 实体，鱼的 `playAmbientSound` 不进比较；倒出声仍按这种桶自己的 `emptySound` 播放。
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
  骨粉不能让树苗长成树、不能催大型蘑菇和草方块上的植物，刷怪蛋、拴绳放不出来，水瓶不能把泥土变成泥巴，
  刷子只吃掉点击。世界生成决定的方块（紫颂花生长）和要生成实体（嗅探兽蛋、青蛙卵）的随机刻也不做；海龟蛋的随机刻会推进 `HATCH` 并在第三阶段孵出小海龟。
- 生存模式的挖掘有进度、裂纹和工具耐久。硬度不是 0 时按 `tool` 的 `damage_per_block` 扣，剪刀不看硬度、剪火不扣。
  创造模式不扣。冒险模式挖得动的时候照样扣，旁观模式挖不动。耐久用完物品减一，并发实体事件。
  生存和冒险模式挖掉方块后按战利品表生成物品实体，创造和旁观不掉。不给经验，也不调用 `spawnAfterBreak`，
  所以矿石和刷怪笼该从 `level.random` 抽的经验数没有抽。不扣饥饿值、不记统计，急迫、挖掘疲劳、精准采集、时运这些附魔和状态效果都没有：
  工具只按物品 ID 交给战利品表；方块实体按官方 `BlockEntity.collectComponents` 传给 `BLOCK_ENTITY` 参数，
  `copy_components` 的 `block_entity` 来源把容器内容、自定义名、锁、战利品表、旗帜图案和陶罐纹样带回掉落物。
  掉落上由战利品函数写出的组件
  经统一的 `JSON → NBT → 组件补丁` 转换写进物品堆，解不开的单项丢掉并记警告；`/loot` 和开箱填充也用这一份转换。
  盔甲受伤、打实体、钓竿和盾牌也不扣耐久；弓和弩射出一发扣一点，滑翔装备每 20 刻扣一点，创造模式免耐久消耗。实体死亡已按战利品表掉落，开箱也已经接上（见下一段）。
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
  对拍，也会随改过的区块一起写进世界目录。熔炉 / 合成器 / 营火 / 酿造台 / 漏斗会睡到下一个
  事件（闲置漏斗按关注格子和吸取盒休眠），区块不能 tick 或方块在世界边界外时按官方暂停；
  世界边界按方块坐标判断，跨边界的区块里界内的照跑、界外的暂停，边界回来后从原进度接着走。
  红石缓存默认关，打开后按确定性工作量门槛只在划算的触发点生效。
- 命令已经接上：`/gamemode`、`/gamerule`、`/tp`（`/teleport`）、`/time`、`/give`、`/clear`、`/setblock`、`/fill`、`/say`、`/msg`（`/tell`、`/w`）、`/me`、`/list`、`/kick`、`/help`、`/scoreboard`、`/team`、`/bossbar`、`/tellraw`、`/title`、`/playsound`、`/particle`（这七个要等级 2）；世界设置类还接上了 `/difficulty`、`/defaultgamemode`、`/seed`、`/setworldspawn`、`/spawnpoint`、`/clone`、`/stopsound`、`/random`、`/enchant`、`/loot`、`/weather`；服务器管理类还接上了 `/op`、`/deop`、`/ban`、`/ban-ip`、`/banlist`、`/pardon`、`/pardon-ip`、`/whitelist`、`/stop`、`/setidletimeout`、`/transfer`、`/version`（权限见下一段）。
  语法、权限和提示按官方 Brigadier。启动时从服务器目录读 `ops.json`（命令行版的工作目录或 `--server-dir` 指定的目录，桌面版是 APP 数据目录的 `server/`），名单里的人用上面的等级，其他人是 0；等级 0 但在名单里仍算管理员（免刷屏、能看到管理员广播）。`/op`、`/deop` 可以在运行中改这份名单。
  26.3 的游戏规则由生成器导出，权威副本在世界线程的全局状态里，全世界共用。`/gamerule` 要等级 2。改完立刻给方块逻辑用的那份快照，并按官方 `GameRuleMap.CODEC` 写进 `data/minecraft/game_rules.dat`，启动时读回。默认特性集下 `max_minecart_speed` 不启用，命令和客户端界面里都没有它。`send_command_feedback` 和 `log_admin_commands` 管命令成功消息和管理员日志。`/fill` 的上限读 `max_block_modifications`（默认 32768）。`block_drops` 会挡住 `Block.popResource` / `popResourceFromFace` 生成物品实体（位置和速度的随机数照取），`entity_drops` 会挡住下落方块 `spawnAtLocation`，`tnt_explodes` 会挡住 TNT 点燃，掉落按战利品表生成物品实体。`respawn_radius`（缺省 10）按官方 `PlayerSpawnFinder.findSpawn` 用：新玩家进服和无床 / 锚重生都在世界出生点周围散布、找能站的列；冒险模式直接走 `fixupSpawnHeight`，超平坦世界也散布。`/spawnpoint` 设的强制个人点在两格都能重生时直接落在 `pos + (0.5, 0.1, 0.5)`，否则回退到世界出生点并发 `NO_RESPAWN_BLOCK_AVAILABLE`。世界出生点由 `/setworldspawn` 改；老玩家进服照官方 `SavedPosition` 用存档里的位置。主世界里对充能的重生锚空手右键会照 `RespawnAnchorBlock.explode` 真的爆炸：强度 5、带火、伤害类型 `bad_respawn_point`、`BLOCK` 交互（`block_explosion_drop_decay`），近水时按 `RespawnAnchorBlock$1` 把被移除那格的爆炸抗性换成水的抗性，放火逐格按 1/3 概率取 `level.random`。单测覆盖了爆炸、方块破坏和近水判定；**已知限制**：这条爆炸没有像 TNT 那样进 `mob-oracle` / 方块 oracle 的逐刻字节对拍（放火位置、方块破坏和世界随机源没有逐位比），重生锚在下界设重生点的 `findStandUpPosition` / 充能扣减也还没做（Bamboo 只有主世界，充能锚空手右键必爆）。`/weather` 只写天气状态和回成功提示。世界每 tick 按官方 `ServerLevel.advanceWeatherCycle` 倒计时、自然开始/结束下雨和打雷（`advance_weather` 规则为假时冻结）、每刻把雨和雷暴强度渐变 0.01，并在强度变化、`isRaining()` 翻转时广播 `RAIN_LEVEL_CHANGE` / `THUNDER_LEVEL_CHANGE` / `START_RAINING` / `STOP_RAINING`；进服时按 `PlayerList.sendLevelInfo` 补发当前的渐变强度。战利品表的 `weather_check` 条件也读这份天气。夜里或雷暴天右键完整的床会按官方 `ServerPlayer.startSleepInBed` 检查天色、距离、遮挡和
  附近怪物，成功后置 `occupied`、设重生点、换成睡姿并发位置 / 元数据包；失败发对应的快捷栏提示。
  `ServerLevel` 的 `SleepStatus` 按 `players_sleeping_percentage` 统计熟睡人数；主控在每刻开头、
  天气之后、岛开跑之前用上一刻全部岛交回的计数决定跳到 `minecraft:wake_up_from_sleep` 时间标记、
  清掉雨和雷、叫醒所有人，和官方 `ServerLevel.tick` 同一刻。躺下和醒来都走和 `/tp` / 进服
  同一条传送路径（递增的 teleport id 和等待客户端确认），`StopSleeping` 也会起床。
  天色按官方 `Level.updateSkyBrightness` 算：主世界时钟的 `minecraft:day` 时间线、
  `SKY_LIGHT_LEVEL` 关键帧和雨 / 雷暴的 `WeatherAttributes` 层合成 `getSkyDarken()`，
  在 `ServerLevel.tick` 的天气阶段之后、方块刻之前的同一刻发布给每个岛；精确模式的
  `World`、并行的实体只读视图、随机刻里读亮度的作物 / 草和菌丝蔓延 / 冰雪融化 /
  阳光探测器都读这一份值。雷暴时 `tick_thunder` 已经按官方顺序生成闪电实体和陷阱马，
  `LightningBolt.tick` 会引火、激活避雷针、除铜锈并逐刻对附近实体 `thunderHit`（含苦力怕带电）；
  还没接的差异是天空、雾和云的颜色变化（客户端自己算颜色）。`spawn_mobs` / `spawn_monsters` 等
  刷怪规则、难度对伤害的缩放、`pvp` / `natural_health_regeneration` / `keep_inventory` 等规则都已接到
  对应系统，`limited_crafting` 目前只广播给客户端、还没限制合成。
  生存和冒险会改模式和能力。挖掘和工具右键会扣耐久；弓和弩射出一发扣一点；滑翔装备每 20 刻扣一点；盔甲、近战武器、钓竿和盾牌不扣。饥饿值已经建模但挖掘不扣饥饿值。`/give` 多出来的物品会消失（没有捡起音效）。`/clear` 不清创造模式光标。`/time` 能改主世界和末地的钟；进服 / 重生按官方 `sendLevelInfo` 发两份时钟的整表同步，`/time` 和 `/gamerule advance_time` 各发单独 / 整表更新，每 20 刻再发一次只带 `gameTime` 的空表同步；这个周期按官方 `MinecraftServer.tickCount`
  （每次开服从 0 数起、不存档）计，不用存档里接着走的 `gameTime`，读档开服时相位才对。
  包里带的 `gameTime` 仍是这一刻 `tickTime` 加一之前的值。发给客户端的流速在时钟暂停或
  `advance_time` 为假时是 0，客户端不会自己把时间往前推。
  按 Q、在菜单里丢出、以及背包放不下时，会生成物品实体。创造模式把物品拖出界面有官方的丢弃限流。旁观模式不丢也不捡。玩家有血量，血量不大于 0（正在死亡或等重生）时不捡（`drops.rs` 按 `is_dead_or_dying` 判定），其余非旁观玩家都捡。捡起会发 `take_item_entity`。攻击其他玩家按 `pvp` 设置结算伤害（见「玩家左键近战攻击」）。物品在地上 5 分钟（6000 刻）后消失，能合并、能被火、岩浆、仙人掌和爆炸毁掉，能被漏斗吸走。玩家不在实体名单里，捡起发生在这一拍所有非玩家实体 tick 之前，看到的是物品移动前的位置。
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
  两个命令的世界坐标都取执行来源的位置（玩家脚底、控制台出生点），没有命令方块或 `/execute` 的来源偏移。
  难度默认 `easy`（和官方专用服务器一致；`server.properties` 的 `difficulty` 键还没实现，按官方未实现的键原样保留、写回并列进启动日志），`/difficulty` 改完广播给所有人，进服时同步，运行值写进 `level.dat` 的 `difficulty_settings` 并在启动时读回。默认游戏模式是创造模式（官方专用服务器默认是生存；`gamemode` 已能在 `server.properties` 里读到，但 Bamboo 还没实现这个键）；`force-gamemode` 按官方默认关闭，`/defaultgamemode` 不改在线玩家的模式。`level-seed` 已实现：按官方 `WorldOptions.parseSeed` 解析，空串取进程内固定的随机种子，写进 `world_gen_settings.dat` 供重启沿用，`/seed` 报的是这一局的世界种子，`login` 里的哈希种子按官方 `BiomeManager.obfuscateSeed` 算。
  `/clone` 复制方块和方块实体数据，上限读 `max_block_modifications`；`/random` 按官方的种子规则共用 `mc-bamboo-loot` 的随机序列；`/enchant` 给主手物品写附魔组件；`/stopsound` 只给选中的玩家发包。`/loot` 的 `give`、`insert`、`replace`、`spawn` 和来源 `loot`、`mine` 可用；`spawn` 按官方 `dropInWorld` 生成带默认拾取延迟的物品实体，`kill`、`fish` 已在命令树里注册但执行时返回失败（这两个来源要实体掉落表和钓鱼参数，`command/loot.rs::need_entity` 明确拒绝；钓鱼系统本身和实体死亡掉落都已在游戏里接上）。
  `/execute` 按官方 `ExecuteCommand` 注册：`as`、`at`、`positioned`、`rotated`、`facing`、
  `align`、`anchored`、`in`、`if/unless block|blocks|entity|score|predicate`、`store result|success`、
  `run`；链上每层用 `CommandGraph` 的重定向上下文保存参数，执行来源按 `source::with_override`
  临时替换，`as` 分叉出的每个分支当场顺序执行，`store score` 在所有分支结束后写回。
  设计见 `docs/command-execution-context.md`；`execute-oracle` 用补全了实体 getter 的官方假世界
  跑 32 条整命令（含 `as @e[type=cow]` 分叉），逐条比较方块、计分板、是否跑到 `run` 和
  翻译键/参数递归展开后的反馈；`--break-self-test` 和按类别的 `--break-group` 确认能抓到改坏。
  已知差异：只有主世界，`in` 其它维度明确失败；`on`、`summon`、`positioned over`、
  `if biome|items|slots|stopwatch|function|dimension|loaded` 和 `if predicate` 还没接上；
  `facing` / `anchored` 用近似眼高。`if/unless data` 和 `store ... block|entity|storage` 已接上。
  `/data` 的 `storage` 是世界数据：按官方 `SavedDataStorage.getDataFile` 写成
  `data/<命名空间>/command_storage.dat`，NBT 信封是 `{DataVersion, data:{contents:{<路径>:<复合标签>}}}`，
  开服读回、存盘写出，官方写的文件也能读（往返测试 + `storage` 对拍）。权威副本在主控的 `Global`，
  多岛下不放在岛里。方块实体能改/会报错的类型清单见 `docs/data-block-entities.md`。
  `/data` 的完整命令树（官方 751 个子节点）逐节点一致；`get` / `merge` / `remove` 和
  `modify set|merge|append|prepend|insert` 配 `value` / `from` 可执行，三种数据源是
  `block`（游戏层方块实体表）、`entity`（`archive::write_entity` / `read_entity`）和 `storage`
  （`Global` 里全服一份的 `CommandStorage`）。`/data merge entity` 对玩家按官方报
  `commands.data.entity.invalid`。`modify ... string|compute` 和 int/float provider 变体解析照常，
  执行时明确报 `command.exception`；`/execute store ... block|entity|storage`、
  `if/unless data` 和 `if score` 都有整条命令对拍（共 42 条）。见 `docs/command-execution-context.md`。
  `/kill` 按官方 `KillCommand`：权限 2、无参杀 `@s`、带选择器杀选中的玩家和非玩家实体；牛走已有的
  `LivingEntity.kill` → 掉落、经验球、死亡动画，其它非玩家实体直接 `remove(KILLED)`。玩家目标走
  `hurt_player` 的 `genericKill` 伤害 → `player_death` 的完整死亡流程（死亡界面包、掉落物、经验球、
  死亡消息、`last_death_location`），再等客户端 `PERFORM_RESPAWN` 由 `player_respawn` 重生。
  实体选择器 `@e` / `@n` / `@s` 现在同时
  遍历玩家和非玩家实体，按官方 `EntitySelector` 的 `type`（含 `!` 与 `#` 标签）、`distance`、
  `x/y/z/dx/dy/dz`、`sort`、`limit`、`name`、`tag`、`gamemode`、`team`、`scores` 过滤和排序；
  `nbt`、`advancements`、`predicate` 在解析时明确报错。`tools/mc-bamboo-vanilla command-oracle` 把
  24 个选择器交给官方 26.3 的 `EntitySelectorParser` 逐项对拍，`--break-self-test` 故意改坏一处。
  已知差异：`name=` 取组件的纯文本或英文实体类名（官方按客户端语言解析 `typeName()` 翻译键）；
  `/tp` 到非玩家实体只改实体表并在下一刻同步（官方 `Entity.teleportTo` 还处理跨维度、载具和即时位置包）；
  `sort=random` 按官方字节码用 `Collections.shuffle` 那份进程级、不带种子的 `java.util.Random`，
  Bamboo 用独立的进程级 `JavaRandom`，不会推进 `level.random` 或任何实体随机源。
- 战利品表（`mc-bamboo-loot`）按官方 `LootTable` 算出方块掉落、实体死亡和箱子填充，随机源的取数次数和顺序跟官方对齐。生存和冒险挖方块、TNT 爆炸、`/loot` 和实体死亡（生物 `die` 走 `Command::DropMobLoot` → `Level::mob_loot_drops`）都已经调用。容器上的 `LootTable` / `LootTableSeed`（或物品上的 `container_loot` 组件放下后的方块实体）在官方 `unpackLootTable` 的调用点解包：玩家打开菜单、漏斗和投掷器读写、发射器取槽、饰纹陶罐读写、比较器读信号、破坏掉内容之前。填完就清表，旁观者打开不解包。随机源按 `LootContext.Builder.create` 选：种子非 0 用 `LegacyRandomSource(seed)`，种子为 0 且表有 `random_sequence` 用世界共用的序列缓存，都没有才用 `level.random`；结构生成时 `setBlockEntityLootTable` 的 `nextLong` 还没接。未解包的表会原样写回中键物品的 `container_loot`。`/setblock` 的方块 NBT 后缀和 `/give` 的物品组件后缀目前会明确报不支持，所以暂时不能用命令造出带战利品表的箱子；方块实体数据里的 `LootTable` / `LootTableSeed` 和 `container_loot` 组件能直接由物品或 `/clone` 带进游戏。解包不走 `THIS_ENTITY` 参数，玩家幸运值按默认的 0；依赖 `this_entity` 的自定义表会报不支持并清表。`loot-oracle` 的箱子题逐项比较 `fill`，另加了一道带 `BLOCK_ENTITY` 的潜影盒掉落题逐项比组件；`menu` oracle 加了一道「打开带战利品表的箱子」。序列缓存在世界里，种子是平坦世界的 0，不存档。
- 计分板、队伍和 Boss 栏按官方 26.3 的命令树和权限等级 2 实现，全服一份，放在世界线程的全局状态里，**不写存档**：重启后目标、分数、队伍和 Boss 栏全部回到空。
  `/scoreboard objectives` 支持 `list|add|remove|setdisplay|modify`（`displayname`、`rendertype`、`displayautoupdate`、`numberformat` 的 `blank`/`styled`/`fixed`）；
  `players` 支持 `list|get|set|add|remove|reset|enable|operation|display`（`name` 和 `numberformat`）。
  准则只实现 `dummy`、`trigger` 以及只读准则的只读语义；`health`、`food`、`air`、`armor` 仍按官方初值（20、20、300、0）在进服第一刻写一次后不再更新，`xp`、`level` 跟着玩家经验的真实值走，统计类准则能解析和创建但不会自动加分（没有统计系统）。玩家离线时分数保留，别人侧边栏上仍能看到。
  目标只有被 `setdisplay` 显示出来时才给客户端发 `set_objective` / `set_display_objective` / `set_score`，进服时会补发正在显示的目标、全部分数和全部队伍。
  `/team` 支持 `list|add|remove|empty|join|leave|modify`（`displayName`、`color`、`friendlyFire`、`seeFriendlyInvisibles`、`nametagVisibility`、`deathMessageVisibility`、`collisionRule`、`prefix`、`suffix`），
  队伍颜色、前后缀会用在聊天显示名和命令反馈里；Tab 列表的 `display_name` 仍是空的，客户端按队伍自己画名字。
  `/bossbar` 支持 `add|remove|list|set (name|color|style|value|max|visible|players)|get (value|max|visible|players)`，
  玩家加入、离开和重连按官方 `ServerBossEvent` 发 add/remove 包。命令反馈用官方翻译键，`send_command_feedback` 照旧管自己的系统消息。
  选择器的 `scores=` 和 `team=` 现在能查计分板；`trigger` 命令、统计类准则的自动加分、队伍的 `deathMessageVisibility` / `friendlyFire` / `collisionRule` 还没接到对应的游戏逻辑（全局 `show_death_messages` 规则已按官方决定要不要广播死亡消息）。
- 服务器管理命令：`/op`、`/deop`、`/ban`、`/ban-ip`、`/banlist`、`/pardon`、`/pardon-ip`、`/whitelist`、`/stop`、`/setidletimeout`、`/transfer`、`/version`。
  命令树（节点名、参数解析器和属性、`executable`、权限）和官方数据生成器的 `commands.json` 逐节点一致，`/version` 要等级 2、`/stop` 要等级 4，其余要等级 3。
  四份名单文件在服务器目录（命令行版是当前目录或 `--server-dir` 指定的目录，桌面版是 APP 数据目录的 `server/`）：`ops.json`、`banned-players.json`、`banned-ips.json`、`whitelist.json`。启动时读一次，命令改完按官方 `StoredUserList.save` 的字段顺序写回（pretty-printed JSON 数组）；Bamboo 先写临时文件再改名，比官方直接覆盖安全。世界线程只持有内存里的权威副本，写盘和 `/whitelist reload` 的读盘在连接层的专门线程上，世界线程不做 IO。
  `/op`、`/deop` 改完立刻给在线的当事人重发权限等级（`entity_event` 24–28）和他用得了的整棵命令树；`/op` 新加的等级按官方 `op-permission-level` 默认 4。`/ban` 和 `/ban-ip` 当场踢人，登录时按官方 `PlayerList.canPlayerLogin` 的顺序（玩家封禁 → 白名单 → IP 封禁）拒绝并给出带原因和到期时间的翻译文本；连接层检查完、世界线程处理 `Join` 时再按权威副本复查一次。封禁项里的 `created` 按官方 `yyyy-MM-dd HH:mm:ss Z` 写入，Bamboo 用 UTC；过期时间只按读入时解析出的毫秒值判断。
  `/whitelist` 有 `on`、`off`、`add`、`remove`、`list`、`reload`。`reload` 的读盘在连接层线程上，结果下一 tick 才被世界线程采用（官方同步读盘后立刻生效，这是世界线程不做 IO 的代价）；成功消息在发出读盘请求时立刻回给命令来源。两个开关都按官方默认值：`white-list` 运行时由 `on|off` 改、重启恢复 false（官方会写回 `server.properties`；Bamboo 会原样保留这个文件里的 `white-list`，但目前不读它也不存档）；`enforce-whitelist` 官方默认 false，Bamboo 没有这个配置项，所以 `on` / `remove` / `reload` 默认不会踢在线玩家，踢人路径只在单元测试里注入开关覆盖。OP 不受白名单限制。
  `/stop` 发 `commands.stop.stopping` 之后走现有关服流程，世界线程给所有玩家发 `multiplayer.disconnect.server_shutdown` 再退出，服务端随世界线程结束。`/setidletimeout` 的单位是分钟，0 关闭；空闲判断照官方 `ServerGamePacketListenerImpl.tick`，Bamboo 已解码且能对应的动作（聊天、命令、按键输入、玩家指令、挥手、换快捷栏、破坏、右键、容器点击和按钮、告示牌、客户端指令、位移）会重置 `lastActionTime`，官方另外那些 handler（攻击实体、交互、旁观者动作、放配方、书与笔等）还没接。
  `/transfer` 发 `ClientboundTransferPacket`（主机名、端口，默认 25565），`/version` 按官方 `VersionCommand.dumpVersion` 的行序打印；版本信息用 `mc-bamboo-data` 的常量，`series`、`build_time`、两个 `pack_version` 按 26.3 官方 `version.json` 固定，测试会读官方 jar 逐个对照。
  名字参数用官方 `GameProfileArgument`：`@` 开头按玩家选择器解析，在线玩家按名字解析；离线模式不在线的名字用官方 `UUIDUtil.createOfflinePlayerUUID`（`OfflinePlayer:<name>` 的 MD5）。正版模式按官方 `CachedUserNameToIdResolver` 维护一份「本次启动以来进过服的玩家 名字→UUID」缓存（`Join` 时记下，不写盘；官方会持久化到 `usercache.json`），在线玩家查不到时先查这份缓存，再查不到就按官方「查不到」报 `argument.player.unknown`（Bamboo 没有 Mojang 档案查询，也不从白名单猜 UUID）。`ban-ip` / `pardon-ip` 的 target 是 IP 或在线玩家名（用连接层报来的对端 IP）。
  `/save-all [flush]`、`/save-on`、`/save-off` 按官方消息和权限等级 4 实现，见「存档」一节
  （成功消息在快照交给 IO 线程后、或 `flush` 的 IO 栅栏回执后才发，差异记在已知限制里）。`/ban-ip` 的 `commands.banip.info` 用逗号连接玩家名，和官方 `EntitySelector.joinNames` 的格式不同；`/version` 的 `build_time` 是纯文本而不是官方的 `Date` 翻译参数。
  控制台命令（照官方 `DedicatedServer` 的 "Server console handler" 线程）：命令行版起一条
  单独的线程读 stdin，每行当作服务器控制台的命令，经 `Inbound::ConsoleCommand` 交给世界线程
  执行，世界线程从不读 stdin。来源是官方 `CommandSourceStack` 的服务器来源：名字 `Server`、
  权限 4、不是世界里的实体；`broadcast-console-to-ops` 按 `server.properties`（默认 true）
  决定反馈要不要广播给在线 OP，反馈打进日志（官方 `MinecraftServer.sendSystemMessage`）。
  `/stop` 走同一条关服流程，所以控制台 `stop` 和客户端 `/stop` 都能正常关服并存盘。
  已知限制：Bamboo 没有语言文件，日志里保留翻译键（如 `commands.stop.stopping`）加参数，
  不解析成英文；控制台来源不是世界里的实体，无参 `/kill`、只收玩家的 `@s` 等按官方报
  `permissions.requires.player`；坐标类命令以世界出生点方块的左下角为执行位置（官方
  `MinecraftServer.createCommandSourceStack`），绝对坐标照用、`~` / `^` 也从这个位置算；
  桌面端是 GUI，不起读 stdin 的线程；
  Windows 后台起的原生进程没有可用 stdin，控制台线程读到 EOF 就退出，`tools/probe.sh`
  因此用客户端 `/stop`，有真实控制台/管道时往 stdin 写 `stop` 一样能关服。
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
  发 1029 或 1030 事件，并保留朝向。经验等级按官方判定：生存模式要 `level >= cost` 才取得出结果、
  取出后扣 `cost` 级，创造模式不扣等级也不受 40 级上限限制。没有聊天过滤器，改名不走文字过滤；没有统计和进度。
  制图台按 `CartographyTableMenu` 收带 `map_id` 的地图和纸 / 空地图 / 玻璃板，真实世界直接查 `MapStore`（找不到对应地图数据时照官方 `setupResultSlot` 直接返回不碰结果格）：
  复制（`map` → 两份，共享 `map_id`）、放大（`paper` → `MAP_POST_PROCESSING=SCALE`，取出时剥离组件并分配新 `map_id` 与 `scaled()` 数据）
  和锁定（`glass_pane` → `LOCK`，取出时分配新 `map_id` 与 `locked()` 数据），同时保留菜单 oracle 假世界的逐槽对拍。取结果按 `lastSoundTime` 同刻只发一次 `ui.cartography_table.take_result`。
  附魔台按 `EnchantingTableBlock.BOOKSHELF_OFFSETS` 数书架，用 `enchantmentSeed` 和生成的附魔表算三个
  `costs`、`enchantClue`、`levelClue`（共 10 个数据槽）。每名玩家进服时有一个附魔种子，附魔成功后按
  `onEnchantmentPerformed` 换新种子；选第 0/1/2 项会扣青金石、扣等级、写附魔、播
  `block.enchantment_table.use`（音调从 `level.random` 取一次）。生存模式按真实经验等级判定并扣级，
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
- 幽匿与监守者（本轮接入）：游戏事件注册表、按岛登记的监听者表和振动系统（选择器、飞行延迟、
  频率表、羊毛遮挡）已接进世界；幽匿感测体和校频幽匿感测体是真的振动监听者，能激活、给红石强度和比较器
  频率、冷却、共鸣，逐刻对拍覆盖激活 / 羊毛遮挡 / 冷却 / 校频过滤，加了两只感测体到发声点距离不同的
  投递顺序场景。幽匿尖啸体听感测体激活发出的 `sculk_sensor_tendrils_clicking`（源是玩家才响应），能警告
  玩家、置 `shrieking` 状态并给附近玩家黑暗；`WardenSpawnTracker` 的警告等级累积、200 刻冷却和 12000 刻
  衰减已实现，逐刻对拍覆盖警告加一 / 冷却期不再加 / 衰减 / 黑暗，并写进玩家档的 `warden_spawn_tracker` 键
  （`player-data` 双向往返）。警告到 4 级时按 `SpawnUtil.trySpawnMob` 的 20 次取点（±5 水平、+6 抬升、
  `ON_TOP_OF_COLLIDER` 找脚下完整顶面）召唤监守者；`checkSpawnRules` / `checkSpawnObstruction`
  （含 `noCollision` 生成盒）和生成后的环境音也照官方；没召唤成才播按等级的回复音。
  幽匿催发体是活的方块实体：`ENTITY_DIE` 游戏事件把死者的基础经验转成电荷，`bloom`
  动画（`PULSE` 8 刻复位、两枚 `sculk_soul` 粒子、一声 bloom），`SculkSpreader` 逐刻
  推进电荷游标、把幽匿块 / 幽匿脉络铺开，`cursors` 用同一份 codec 存档往返。对拍
  `mob-oracle --only sculk_catalyst`：`sculk_catalyst_save` 存档往返，`sculk_catalyst_spread`
  一只僵尸死在旁边、逐刻比游标 / 方块变化 / 世界随机源，只差已知的三角函数表末位差。
- **监守者**：`MobType::Warden`（生命 500、移速 0.3、击退抗性 1、攻击 30、跟随 24、防火、
  不因距离消失）；`WardenAi` 的 8 个活动（CORE / EMERGE / DIG / IDLE / ROAR / FIGHT / INVESTIGATE / SNIFF）、
  两个传感器、15 个记忆和全部行为，含咆哮 / 嗅探 / 近战 / 音波 / 钻出钻入、愤怒管理
  （`AngerManagement` 按愤怒排序选目标、衰减、存档 `anger` 键）、黑暗光环、`CLIENT_ANGER_LEVEL`
  元数据；监守者自己是半径 16 的振动监听者（眼睛位置、`#warden_can_listen`），收到振动加愤怒并去调查；
  `/summon`、刷怪蛋、掉落 `sculk_catalyst` 都接上了；尖啸体 4 级警告按 `SpawnUtil.trySpawnMob`
  取点召唤，`finalizeSpawn(TRIGGERED)` 的记忆 / 姿态 / 随机取数照字节码。近战伤害、
  `sonic_boom` 伤害来源与 10 点穿墙伤害、音波沿射线从胸口附着点打到目标眼睛的一串
  `sonic_boom` 粒子、音波命中后按目标击退抗性缩放的额外 `push`、击退
  （`causeExtraKnockback` 把攻击者水平速度压到 0.6，再给受害者竖直 0.5 / 水平按攻击
  击退属性的第二下击退）、黑暗光环、心跳与按姿态变化的环境音、射弹拥有者分支
  （10 / 35 点愤怒与 `RECENT_PROJECTILE`）都已落地。`canTargetEntity` 的过滤
  （创造 / 旁观、无敌、世界边界、不是另一个监守者）也已接上。`mob-oracle --only warden` 覆盖召唤后钻出、听振动去调查、嗅探、发怒咆哮、近战、
  音波、无目标钻回地下、射弹与重复射弹、音波把一只僵尸击飞、以及 `anger` / `listener`
  的存档往返，共 11 个逐刻场景和 2 个存档场景，逐刻比姿态、愤怒、大脑、坐标 / 速度 / 朝向、
  两种随机源、玩家血量与效果、近战打中那一刻发给玩家的速度包、被音波击飞的生物速度；
  8 个 `--break-self-test`（姿态 / 愤怒 / 大脑 / 位移 / 血量 / 黑暗 / 存档 / 玩家击退包）都能报出不一致。
  **已知限制**：`AngerManagement.conversionDelay` 初值用官方线程本地随机源、自身不可复现，读档后
  UUID→嫌疑转换那几刻不对拍（只比存档字节）；召唤时环境音的音高随机数、
  `canTargetEntity` 里盔甲架 / 同队这几项过滤（Bamboo 已有盔甲架生物、生物不加入队伍）、
  生物碰撞推挤别的实体（`Entity.pushEntities` 那一套物理推挤）还没建模——推挤只改速度、
  不归到击退对拍里；声音事件由同步层发出，音效参数不逐位对拍。
- **凋灵（本轮接入）**：`MobType::Wither` 注册进类型表（`ALL` 74 种），属性照 `WitherBoss.createAttributes`
  （300 血 / 0.6 移速与飞行速度 / 40 跟随 / 4 护甲）、尺寸 0.9×3.5、50 经验、`Invul` 存档键往返。
  **召唤**照 `WitherSkullBlock.checkSpawn`：玩家放下凋灵骷髅头时用 `CarvedPumpkinBlock.clearPatternBlocks`
  那套 `BlockPattern` 检测灵魂沙 / 灵魂土 T 形加三个头的图案（`^^^` / `###` / `~#~`），清 9 格发 2001 事件、
  更新邻居，按 `forwards` 轴摆位置和身体朝向、调 `makeInvulnerable`（220 刻无敌、血量 100、血条清零），
  给 50 格内玩家触发 `SUMMONED_ENTITY`；和平难度或高度不够不召唤。**实体行为**：`customServerAiStep`
  里无敌期每刻递减 `Invul`、每 10 刻回 10 血、进度按 `1 - invul/220` 推进，归零时以眼高为心发 7 格 `MOB` 爆炸
  并以 `globalLevelEvent` 全服广播 1023 号事件（协议包 `global=true`，不看距离）；之后按 `destroyBlocksTick`
  扫包围盒内非 `#wither_immune` 方块逐格 `destroyBlock` 发 1022 事件、
  每 20 刻回 1 血、血条进度按血量比更新。`aiStep` 接上追主目标的飞行速度和两个侧头朝 `DATA_TARGET_B/C`
  的折角插值（`rotlerp`）。`hurtServer` 挡下无敌期、溺水（`#wither_immune_to`）、另一个凋灵、
  `#wither_friends`（`#undead`）的伤害，半血（`isPowered`）时箭 / 风弹被弹开，放行前设 `destroyBlocksTick=20`、
  `idleHeadUpdates` 各加 3。`goalSelector` / `targetSelector` 照 `WitherBoss.registerGoals` 注册：无敌期
  `WitherDoNothingGoal` 占住 MOVE/JUMP/LOOK，之后是 40 刻一次的 `RangedAttackGoal`（追目标、盯住、每 40 刻射主头）、
  `WaterAvoidingRandomFlyingGoal`（`HoverRandomPos` / `AirAndWaterRandomPos` 挑空中落点）、看玩家、环顾；
  `HurtByTargetGoal` 和 `NearestAttackableTargetGoal<LivingEntity>` 用 `LIVING_ENTITY_SELECTOR` 排除 `#wither_friends`
  （亡灵），`randomInterval=0` 每刻索敌，凋灵会飞起来追目标。`aiStep` 的烟雾 / 实体效果粒子照字节码消耗实体随机源（每刻 9 次高斯、半血时每头一次
  `level.random.nextInt(4)`、无敌期再加 6 次高斯 + 3 次浮点）。死亡掉下界之星，`LivingEntity.createWitherRose`
  照字节码排在所有掉落命令之后：`mobGriefing` 且脚下方块是空气、且 `WitherRoseBlock.canSurvive`（走行为表
  `mayPlaceOn` 的 `#supports_wither_rose`）时种下玫瑰，否则按 `getX/getY/getZ` 的实体坐标掉玫瑰物品。
  **凋灵之首**：`WitherSkull` 是新的投射物实体，飞行照 `AbstractHurtingProjectile.tick`：
  `applyInertia` 每刻沿速度方向加 `accelerationPower=0.1` 再按惯性缩放（普通 0.95、蓝色危险 0.73、
  水中 `getLiquidInertia()=0.8` 并发 4 个气泡粒子），初速是归一化方向乘 0.1；构造用 owner 的朝向，
  `rotateTowardsMovement` 先把上一刻角度平移到目标角度的 `[-180,180)` 邻域再按 0.2 插值。命中路径复用风弹的
  `ProjectileUtil.getHitResultOnMoveVector` 加 `computeMargin`。命中实体造成 8 点 `wither_skull` 伤害
  （无 owner 时 5 点 `magic()`），目标被打死时 `owner.heal(5)`、存活时跑 `EnchantmentHelper.doPostAttackEffects`
  （遍历目标装备的 `POST_ATTACK` 附魔，凋灵之首 / 生物目标恒为空，Bamboo 暂无通用框架）；命中即发 1 格 `MOB` 爆炸；
  蓝色头把非 `#wither_immune` 方块的爆炸抗性压到 0.8，能真的炸掉方块；普通难度挂 200 刻、困难挂 800 刻的
  2 级凋零（简单 / 和平不挂）。`DATA_DANGEROUS` 是同步元数据 index 8 的 BOOLEAN，蓝色头客户端可见。
  `RangedAttackGoal` 每 40 刻射主头（head 0，0.1% 概率蓝色），`customServerAiStep`
  里两个侧头按 `nextHeadUpdate` 索敌、空闲累计到 16 次朝随机点射蓝色头；每次发射先发 1024 号世界事件。
  存档写 `dangerous` 键。
  **验证**：`cargo test -p mc-bamboo-blocks -p mc-bamboo-game`、`cargo clippy --workspace --all-targets`、
  `cargo fmt --all --check` 全过；`mob-oracle java --show 2000` 267 处已知末位差（`frog_lily_41` 偶尔多约 206 行、
  复跑 `--only frog` 回到 46；无凋灵相关差异），
  `entity-oracle` / `player-data` / `chunk-content` 三个 Java 对拍全过。
  **Boss 血条**：`ServerBossEvent` 每只凋灵一根栏，栏 UUID 用 `Mth.createInsecureUUID(random)`（构造时消耗
  实体随机源两次 `nextLong`，不存档、每只凋灵独立），game 层每刻跟着 `EntityTracking.seen_by` 给新进入
  跟踪范围的玩家发 `BossEvent` add（PURPLE / PROGRESS / `setDarkenScreen(true)`，名字跟 `getDisplayName()`，
  有自定义名就用它、否则 `entity.minecraft.wither`）、离开的发 remove、进度或名字变化的发 update，
  凋灵消失时给所有观众发 remove；拆岛 / 合岛时血条跟着实体搬。
  **逐刻对拍**：`mob-oracle --only wither` 的存档往返场景把官方 `WitherBoss` 的 NBT
  （`Invul` 取 0 / 137 / 220）逐键对拍 0 差异；`--only wither_invul` 的无敌期逐刻场景（2 例 46 刻）逐刻比
  `Invul` / 血量 / 位置 / 速度 / 朝向 / 两侧头旋转和两种随机源；`--only wither_explode` 的无敌期结束爆炸场景
  逐刻比 16 个状态字段 + 爆炸 / 1023 全服事件（2 例）；`--only wither_skull` 的射凋灵之首命中牛场景（21 刻）
  逐刻比射弹轨迹 / 朝向、牛血量 / 击退、2 级凋零效果、世界随机源；`--only wither_arrow` 的半血弹箭场景（9 刻）
  逐刻比箭的轨迹、凋灵血量（不减）、两种随机源；`--only wither_destroy` 的破坏方块场景（23 刻）逐刻比
  包围盒扫描区 36 格方块状态与两种随机源（第 20 刻 32 格石头全被破坏）。以上场景都 0 差异。
  改坏确认 `wither_save_invul` / `wither_save_health` / `wither_save_pos` / `wither_save_rotation` /
  `wither_invul` / `wither_explode` / `wither_skull` / `wither_arrow` / `wither_destroy` 都报不一致。
  **已知限制**：召唤图案（T 形方块检测）的官方逐刻对拍场景还没做（召唤逻辑本身已接，只是没有逐刻夹具）；
  `EnchantmentHelper.doPostAttackEffects` 的通用 `POST_ATTACK` 附魔效果框架 Bamboo 暂无，凋灵之首命中
  生物 / 无装备目标时恒为空、按字节码的空结果处理；水中分支的气泡粒子官方用的是 `Level.addParticle`，服务端不发包，Bamboo 也不发（水中分支的对拍场景未覆盖）；侧头 `getNearbyEntities` 已按字节码把玩家一并纳入候选，
  但玩家目标的逐刻对拍场景未覆盖。
- 正版验证的限制：`server.properties` 的 `online-mode` 官方默认 true，命令行版和桌面端都读同一份文件、
  设置页里可以改；`ServerConfig::default()`（不读配置文件的嵌入用法）是 false。
  和代理转发互斥：打开转发时正版验证会被关掉，因为代理已经替服务端验过了。
  正版验证打开且公钥取到了之后，聊天按上面一节签名。取钥失败会按 authlib 的间隔在后台重试，钥匙到达后安全聊天立刻生效。
  RSA 解密用的 `rsa` crate 有 RUSTSEC-2023-0071（Marvin 计时侧信道），上游还没有修好的版本，
  Bamboo 用 `decrypt_blinded` 做盲化缓解，详见 `crates/mc-bamboo-auth/src/keypair.rs` 的注释。
- 系统消息的可见性按官方做，但官方 `ServerPlayer.sendSystemMessage` 还用
  `PacketSendListener.exceptionallySend` 在发包失败时补一条 `multiplayer.message_not_delivered`；
  Bamboo 的链路写失败时直接关连接，没有这个回调，所以不发这条补发消息。
- 方块漏斗的 `getEntityContainer` 在没有方块容器时搜索中心 1×1×1 盒里活着的运输船、运输矿车和漏斗矿车，
  同一范围有多个实体容器时用 `level.random` 选择。抽取与输出都走这条查找路径。
- 桌面端设置页保存这四个世界生成键时和官方的写法有两处差别（语义不变）：`level-type` 的旧名
  `default` / `largebiomes` 会归一成 `minecraft:normal` / `minecraft:large_biomes`，种子和
  `generator-settings` 的前后空白会去掉；官方保留文件里的原字符串。另外，世界类型是超平坦时，
  界面里解析不了的 `generator-settings`（坏 JSON、未知方块或群系）会**拒绝保存并提示**，
  而官方只是记一条 error、静默用默认超平坦预设。命令行版和直接改文件都不受影响。

（2026-11-14 按实现更新：`docs/performance-optimizations.md` 7g.1–7g.6 / 7i / 第 9 节、
`docs/configuration.md`、`README.md`「现在能玩什么」的世界边界条目、`crates/mc-bamboo-server/src/config_files.rs`；
合并提交 `54f5fd2`、`69cd662`、`4864a49`、`b02ad69`、`e6f8d6e`、`edce3ab`、`fa91b84`）

## 项目结构

| 路径 | 职责 |
|---|---|
| `crates/mc-bamboo` | 命令行入口，可执行文件 `bamboo` |
| `crates/mc-bamboo-server` | 网络层：监听、连接状态机、读写队列、登录准入 |
| `crates/mc-bamboo-game` | 世界线程：tick、玩家、区块跟踪、实体同步、方块交互、方块实体的物品数据、聊天，不依赖 tokio |
| `crates/mc-bamboo-protocol` | 网络协议：VarInt、帧、数据包、物品数据组件，只处理字节 |
| `crates/mc-bamboo-world` | 游戏世界：区块、调色板、光照、超平坦，不知道网络 |
| `crates/mc-bamboo-worldgen` | 世界生成的数学和区块级生成：随机源、噪声、密度函数、气候、群系源、官方群系温度、含水层、主世界 `BIOMES` / `terrain`（`doFill`+地表+雕刻器）、features 和结构件的纯函数；`terrain`、features 各 decoration step、结构件放置和生成期光照都已接进 Director 流水线（在 `mc-bamboo-game::worldgen` 里组装成 `RegionColumn`） |
| `crates/mc-bamboo-blocks` | 方块行为：放置、形状更新、破坏、右键（含工具对方块的右键）、计划刻、方块事件、会 tick 的方块实体（按事件休眠）和触发级红石缓存，纯逻辑，不知道网络和存储 |
| `crates/mc-bamboo-command` | 命令分发和参数解析：Brigadier 风格的树、坐标、方块状态、选择器，纯逻辑，不知道网络 |
| `crates/mc-bamboo-recipes` | 配方查找和匹配：有序、无序、烧炼、切石机、锻造、酿造和特殊配方，纯逻辑，不知道网络和世界线程 |
| `crates/mc-bamboo-advancements` | 进度系统的纯数据层：共享只读树、官方显示排版、玩家进度和进度档案 JSON 编解码，不认识世界线程 |
| `crates/mc-bamboo-loot` | 战利品表：方块掉落已经接进生存和冒险挖掘、爆炸、可替换方块的破坏和 `/loot`，箱子填充接进开箱和容器读写，实体死亡掉落也已接进生物死亡流程 |
| `crates/mc-bamboo-core` | 共用的基础类型：方块坐标、区块坐标、方向、玩家档案 |
| `crates/mc-bamboo-auth` | 正版验证和协议加密：RSA 密钥对、AES/CFB8、serverId 哈希、问 session server |
| `crates/mc-bamboo-nbt` | NBT 编解码，解码带官方的配额和深度上限 |
| `crates/mc-bamboo-data` | 版本数据：由官方服务端生成的协议号、包 ID、方块、物品、配方、战利品表、注册表和标签 |
| `apps/desktop` | 桌面控制台：Tauri + React + styled-components |
| `tools/mc-bamboo-vanilla` | 开发工具：生成版本数据、用官方代码验证 Bamboo 的协议、方块行为、光照、菜单、熔炉、酿造台和营火的逐刻状态、物品组件、方块谓词、配方、进度树与包字节、战利品表、世界边界和世界生成 |
| `tools/mc-bamboo-memprof` | 堆内存分析器：用记账全局分配器按调用栈统计还活着的字节、按归属汇总，实测空载 / 一人在线 / 玩家离开后的进程内存构成（`run.sh` 起服取样，Windows + PDB 符号解析） |

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
`worldgen` 是纯函数；`game` 现在既用它算 `Biome.getTemperature`（天气的结冰、积雪和降水
类型），也在 `game::worldgen` 里调它的 `BIOMES` / `terrain` 生成区块接进 Director 流水线
（`worldgen` 本身不认识世界线程，`RegionColumn` 的组装在 `game`）。工具里的
`chunkgen` / `worldgen` / `gen-world` oracle 用其余的数学并逐格对拍。它依赖 `data`
（官方导出的世界生成数据和群系气候表）和 `blocks`（里面有官方的 `LegacyRandomSource`）。
`loot` 算方块掉落、实体死亡和箱子填充，由 `game` 在挖掘、爆炸、`destroyBlock`、`/loot` 和容器第一次读写时调用；
它依赖 `data`（嵌入的战利品表 JSON）、`blocks`（随机源、方块状态）和 `core`（`Mth.sin` 的查找表）。

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

`generated/` 下的注册表由生成器按官方注册顺序（报告里的 `protocol_id`）导出，不要手写或按名字排序：
`game_event` 等表在 26.3 的官方顺序里 `entity_action` 在 `entity_place` 之后、`resonate_1..15` 在末尾，
和字典序不同。存档里的振动事件只按名字查注册过的条目，不依赖数字 ID，顺序只影响常量取值。

`generated/` 下的文件（以及 `crates/mc-bamboo-worldgen/src/structure/post_process/jungle_temple_ops.rs`）全部由
`tools/mc-bamboo-vanilla` 的生成器产出，不要直接编辑。要改生成数据——新增常量、修正数值、调整顺序——
一律改生成器（`java/Extractor.java`、`src/extract.rs`、`src/generate.rs`）再重跑导出；只改生成文件不改生成器，
下次全量重跑就会被冲掉。官方有些集合（例如 POI 的 `matchingStates()`）是 `HashSet`，迭代顺序每次 JVM 运行
都不同，生成器必须显式排序，保证全量重跑逐字节可重复。`tools/check-generated.sh` 用现有的报告全量重跑两遍
导出，并断言生成文件没变；改完生成器就跑它，确保提交里的生成文件和生成器一致（前提是先跑过一次 `generate`
让 `target/vanilla` 里有报告）。

然后启动 Bamboo，用官方网络代码让三个客户端走一遍进服、移动、建造、聊天和开关箱子，检查收到的每个包。
区块包里的方块和流体计数、高度图都由官方代码从解出来的方块重算，再和收到的值逐段比较：

**探针是按超平坦校准的**：新世界默认已经是 `normal`，所以运行探针的服务器目录里的
`server.properties` 必须设 `level-type=minecraft:flat`（建议同时设 `level-seed=0` 固定种子），
否则客户端收到的是噪声地形、探针里按超平坦写死的方块断言会失败。这个文件不进仓库。

```bash
cargo run -p mc-bamboo-vanilla -- probe 127.0.0.1 25565
```

`probe` 连的是你启动的那个 Bamboo，它跑的是离线模式。正版验证那一段要换一个服务端（`online_mode`
在启动时就定下来了），所以 `probe` 会在自己的进程里再起一个开了正版验证的 Bamboo，外加一个假的
session server 顶替 Mojang，让官方客户端代码走完加密登录，全程只走 `127.0.0.1`，不访问外网。
`tools/probe.sh` 会设 `BAMBOO_PROBE_STOP=1`，让探针收尾用 `/stop` 正常关服再验证存档；
直接跑上面的命令不会停服务端，要自己关。

最后让官方代码给方块行为出题和给答案，用 `mc-bamboo-blocks` 重放，比较两边的结果：

```bash
cargo run --release -p mc-bamboo-vanilla -- oracle <java 路径> [--cases N] [--only <方块或类名>] [--weather clear|rain|thunder] [--clock-ticks N]
```

时钟和天色单独用官方 26.3 的类逐刻对拍：`clock-oracle` 覆盖一整个昼夜循环、
雨、雷暴、渐变中的雨、`/time set` 跳变、`advance_time` 关闭、非整数流速和暂停，
逐刻比两份世界时钟的状态与 `getSkyDarken()`；`--break-self-test` 确认天气贡献、
更新时机和时钟初值改坏都能被抓到。`--clock-ticks 18000` 让方块 / 随机刻 oracle
在夜晚场景里检查依赖亮度的方块行为。

```bash
cargo run -p mc-bamboo-vanilla -- clock-oracle <java 路径> [--break-self-test] [--show N]
```

睡眠判定和跳早数学另用官方 `BedRule.canSleep(Level)` / `Level.isDarkOutside` /
`SleepStatus` / `ClockTimeMarker` / `GameRules` 逐项对拍（`sleep-oracle`）：构造官方
`Level`，用官方时钟、时间线和天气层算出 `updateSkyBrightness()`，再真的调
`BedRule.canSleep`，覆盖多个时刻 × 晴 / 雨 / 雷、`players_sleeping_percentage` 多档和
正负时间刻；三种改坏确认覆盖天气对变暗的贡献、熟睡人数公式和跳早目标。
方块 oracle 的 `--clock-ticks 18000` 夜晚批让假世界的方块光为 0、天空光为 15，
于是 `getRawBrightness(pos, skyDarken) = max(block, sky - darken)` 真的随天色变暗，
作物 / 树苗 / 草和菌丝蔓延 / 霜冰 / 阳光探测器 / 床都在夜里重新对拍。

```bash
cargo run -p mc-bamboo-vanilla -- sleep-oracle <java 路径> [--break-self-test] [--show N]
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
主世界区块到 `biomes`、`doFill`、地表规则和雕刻器（`--through carvers`），逐方块、逐群系比较；
不启动服务端、不接受 EULA。细节和耗时见 `docs/worldgen.md`：

```bash
cargo run --release -p mc-bamboo-vanilla -- chunkgen <java 路径> \
    [--seeds N] [--chunks N] [--salt S] [--through carvers] [--show N]
```

`gen-world` 是接进游戏后的对拍：同一套官方出题，Bamboo 侧调的是**游戏里用的那份**
`game::worldgen::generate_chunk`（`OverworldGenerator` 走 `Staged` 流水线时的纯函数）
而不是直接调 `mc-bamboo-worldgen`。默认逐 quart 格、逐方块比较 CARVERS 快照；
`--through features --all-features` 跑完 11 个 decoration step，逐格比 3×3 方块、
方块实体和 `PostProcessing` 位置表。`--break-self-test` 故意改坏一个方块，确认
对拍会报不一致：

```bash
cargo run --release -p mc-bamboo-vanilla -- gen-world <java 路径> \
    [--seeds N] [--chunks N] [--salt S] [--level-type normal|large_biomes|amplified] [--show N]
```

`spawn` 让官方 `MinecraftServer.setInitialSpawn` 出题（出生区块 + 最终出生点），Bamboo 侧
调游戏里那份 `initial_spawn`，三种噪声世界、多个种子逐个比；`flat-world` 让官方
`FlatLevelGeneratorSettings.CODEC` + `FlatLevelSource.buildTerrain` 出题，Bamboo 侧调
`FlatSettings::parse` + `generate_chunk`，逐字段、逐方块比较。两者都支持
`--break-self-test`：

```bash
cargo run --release -p mc-bamboo-vanilla -- spawn <java 路径> \
    [--seeds N] [--salt S] [--level-type normal|large_biomes|amplified] [--show N]
cargo run --release -p mc-bamboo-vanilla -- flat-world <java 路径> [--show N]
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

### 闪电、避雷针、骷髅陷阱马、骷髅马和僵尸马（分支 `lightning`）

- **闪电（`LightningBolt`）**：雷暴里 `ServerLevel.tickThunder` 的 1/100000 命中按官方顺序取
  `getBlockRandomPos` → 高度图 → 避雷针 → 附近能看见天的活物 → `nextDouble()` 的陷阱判定，
  落点生成闪电实体；`/summon minecraft:lightning_bolt` 也能生成。第一刻按难度引火
  （`spawnFire(4)`，先看 `FIRE_SPREAD_RADIUS_AROUND_PLAYER`）、激活避雷针（8 刻后收回）、
  给脚下和附近的铜除锈（用 `level.random` 随机游走），之后逐刻对 3 格盒内的实体调
  `thunderHit`：苦力怕带电、哞菇换色、海龟掉碗、其它生物着火并受 5 点雷击伤害、玩家着火受伤。
  存活刻数、闪光次数和雷击种子都由实体自己的随机源决定，和官方逐位一致。
  `mob-oracle --only lightning` / `--only trap` 6 个场景逐刻对拍，
  `--break-self-test lightning` / `trap` 均能抓到不一致。后续补了
  `thunder_target`（多活物按官方实体分区顺序选落点）、`thunder_hit`
  （苦力怕带电、海龟掉碗、牛着火受伤）、`trap_gear`（四名骑士的
  `finalizeSpawn` 装备和 `MOB_SPAWN_EQUIPMENT` 附魔逐位比）、
  `undead_horse`（水下 / 驯服 / 交互拒绝逐刻、SNBT 往返、死亡掉落）三类场景，
  每类都有改坏确认；精确与并行两种驱动最后一刻的命令落地另有一道单测。
- **避雷针 POI**：官方 `findLightningRod` 查 `PoiManager` 里 `PoiTypes.LIGHTNING_ROD`
  的记录；Bamboo 走 `Level::lightning_rod_near` 薄封装 → `poi_find_closest`
  （`LIGHTNING_ROD`、`WORLD_SURFACE - 1` 高度过滤、128 格、`ANY`，返回上方一格），
  精确驱动读本岛 `PoiStore` 当前状态、并行驱动读本刻开始快照，两种驱动不分叉。
  `lightning_rod.rs` 单测用真 POI 表覆盖多根、等距、被挡、超半径；`mob-oracle --only
  lightning` 对拍官方假世界空 POI 时的兜底路径。
- **骷髅陷阱马**：雷暴生成带 `SkeletonTrap` 的骷髅马，玩家进 10 格时再劈一道视觉闪电，
  变成 4 名骷髅骑士；`SkeletonTrap` / `SkeletonTrapTime` 存盘，18000 刻后没触发就消失。
- **骷髅马 / 僵尸马**：属性、幼年尺寸与眼高由生成器导出，马科行为复用 `equine`，
  骷髅马能在水下走（`getWaterSlowDown = 0.96`、`#can_breathe_under_water`）、不溺水；
  驯服与交互、战利品表、刷怪蛋和存档都接好。

**已知限制**：

- 官方 POI 记录在区块未加载时也存在，`getHeight` 还会强制加载区块；Bamboo 只看已加载的区块。
  同一个 POI 段里多根等距避雷针的并列顺序官方由 `HashMap` 决定，Bamboo 取确定顺序。
- 村民被雷劈变女巫、猪被雷劈变僵尸猪灵这两种雷击转化未建模（Bamboo 没有僵尸猪灵，
  女巫虽已建模但 `thunder.rs` 只接了哞菇 / 苦力怕 / 海龟 / 铜傀儡的覆盖层），这些生物
  只按基类着火受伤。猪的雷击转化照官方 `Pig.thunderHit`：非和平难度下
  `convertTo(ZOMBIFIED_PIGLIN, ConversionParams.single(this, false, true))`，转换时
  `convertCommon` 不转移乘客——新僵尸猪灵不带乘客生成，原来的猪被 `discard` 掉、
  骑在上面的玩家被弹下来；Bamboo 没有僵尸猪灵，因此骑着的玩家会留在猪上，与官方不一致。
- **骑士已经真的骑在马上**：「生物骑生物」的载具模型已接入（见「生物骑乘」一节；
  蜘蛛骑士、鸡骑士、骆驼尸壳骑士也都在）。`SkeletonTrapGoal.tick` 照官方生成四名骷髅骑士，
  `spawn_trap` 给原陷阱马 `add_passenger`、给三匹新马各 `start_riding`，
  乘客名单、`set_passengers` 同步包和逐刻骑手位置都走通用骑乘链；
  `trap_gear` 逐位对拍四名骑士的 `finalizeSpawn` 装备与 `MOB_SPAWN_EQUIPMENT` 附魔，
  `trap_near` / `trap_far` 比 `isTrap` / `isTame`、生成实体数和随机源。
  三匹新马的跳跃力随机化走 `randomize_skeleton`，同样写进属性表。
- 服务端不发 `LIGHTNING_STRIKE` 游戏事件，也不播官方天气层的雷声 / 撞击声
  （那两声是客户端 `playLocalSound`；天气层另有 `globalLevelEvent`），
  雷击的进度触发器（`LIGHTNING_STRIKE` / `CHANNELED_LIGHTNING`）未接。
- 骷髅马 / 僵尸马已有逐刻、存档往返和死亡掉落对拍（`undead_horse` 系列），
  但没有单独的 `natural-spawn` 新场景（官方假世界的群系刷怪表没有这两种）；
  `natural-spawn java` 的 1727 例仍 0 不一致。方块 oracle 的雷暴夜晚批
  （`--clock-ticks 18000 --weather thunder`）0 失败，雷击引火 / 铜除锈由
  `mob-oracle --only lightning` 的 `finalBlocks` 逐格比对。

### 船、竹筏和运输船（分支 `boats`）

- **实体**：11 种木船 / 竹筏及各自的运输船版本（`AbstractBoat` / `Boat` / `Raft` /
  `AbstractChestBoat` / `ChestBoat` / `ChestRaft`）按注册表实体类型建模，尺寸
  1.375×0.5625、眼高 0.5625（官方 `eyeHeight(0.5625f)`，不是 `height*0.85`）。
  船不是生物，伤害走 `VehicleEntity` 的同步数据（受伤时间、方向、伤害值），另有左右桨
  和气泡时间；`isEyeInFluid(WATER)` 与 `in_water`（`isInFluid`）分开记，前者给
  `canAddPassenger` 用。运输船带 27 格物品栏和战利品表。存档按 `id` 名认船，运输船的
  `Items` / `LootTable` / `LootTableSeed` 和 `Passengers` 往返都有单测。
- **能玩起来**：`BoatItem.use` 用 `ClipContext.Fluid.ANY` 射线放置（含 `noCollision`
  校验、消耗物品、5 格内 `CAN_BE_PICKED` 检查按官方 `Entity.isPickable()` 覆写分类）；
  右键上船（普通船两名 / 运输船一名）、潜行下船、`clampRotation` 乘客朝向限制；
  `AbstractBoat.tick` 第 9 步既推实体，也把附近没骑东西、比船窄、且船眼睛不在水里的
  动物 `startRiding(boat)`。`Boat.passengers` 接到骑乘层，玩家控制时 `move_vehicle`
  照马校验；`ServerboundPaddleBoatPacket` 写进船的左右桨并同步。船的 6 项 EntityData
  （受伤时间 / 方向 / 伤害 / 左右桨 / 气泡时间）按 index 8–13 逐刻 diff 同步。
- **运输船容器**：加 `Target::Boat`（参考 `Target::Villager` 实体菜单先例），满员或潜行
  右键打开 27 格 `GENERIC_9x3` 菜单，点击写回船的 `chest.items`，玩家走远或船被移除就
  关菜单；`ChestParams` 带打开菜单的玩家和幸运值。漏斗上方 1×1×1 格里没有方块容器时，
  走 `getEntityContainer` / `CONTAINER_ENTITY_SELECTOR` 的实体容器路径，从运输船抽走
  一件并写回船的 27 格。战利品表在破坏、菜单打开、漏斗读取三处都按
  `unpackChestVehicleLootTable` 解包（后两者分别带玩家 / `null`）。
- **服务端物理**：`AbstractBoat.tick` 的服务端路径，`getStatus` / `checkInWater` /
  `isUnderwater` / `getWaterLevelAbove` / `getGroundFriction`（逐格 ±1 扫描、跳外圈
  角落、碰撞形状相交）/ `floatBoat` / `tickBubbleColumn` 照官方字节码实现，含气泡柱
  冲量、水流推动（`Entity.updateFluidInteraction`，在 `floatBoat` 之前）、冰面摩擦和
  桨动画。控制者是玩家时服务端清速度、位置由 `move_vehicle` 决定。`floatBoat` 四个
  状态分支有单测；`Entity.push` 两边都推（船没乘客时也反推，并按 `AbstractBoat.push`
  的 AABB 高度门控）；气泡柱 71/72 事件只发给跟踪者，甩下来的 mob 乘客走完整
  `stopRiding` 落位。
- **受伤破坏**：玩家打船累计伤害（`damage > 40` 或创造模式破坏）、掉船物品与运输船
  箱内物品；岩浆每刻两次 4.0 伤害（`applyEffectsFromBlocks()` 调两次）、火每 20 刻
  1.0；官方船 `checkFallDamage` 只记 `lastYd` / `fallDistance`（不受摔落伤害）。
- **逐刻 Java 对拍**：`EntityOracle.java` 在假世界里跑官方船，`entity_oracle.rs` 用
  Bamboo 方块层逐刻重放，6 个场景（静水漂浮下沉、冰面滑、水位梯度水流推动、岩浆
  破坏掉物、运输船存档往返、动物上船）共 46 题 811 刻，逐位比位置 / 速度 / 状态 /
  伤害 / 朝向 / 流体字段 / 乘客 / 27 格物品 / 掉出的物品 / 方块，`entity-oracle java`
  报 `46 cases, 811 steps, 0 mismatches`。Java 假世界补了记分板、
  `isPositionEntityTicking`、实体名单在实体 tick 前可见、破坏掉物留快照，流体直接由
  方块状态给出；船的随机源按 `entitySeed`（`entitySeeds`）取。对拍过程中修正了五处
  真实物理偏差：构造时 `status` 为 null（第一刻不进「空中→水」分支）、
  `checkFallDamage` 记 `move` 前速度、水流推动早于 `floatBoat`、岩浆伤害两次、
  `canAddPassenger` 的眼睛在水里检查。**对拍层面的改坏确认** 5 处（重力 0.04、
  水下阻力 0.45、水流推力 0.014、岩浆伤害次数、首刻状态标志）故意改坏生产代码，
  `entity-oracle java` 每次都报 12 处不一致，恢复后回到 0 不一致。

**已知限制**：

- 玩家控制船时位置由客户端 `move_vehicle` 决定，只比了校验（照现有马的做法），
  没有 Java 逐刻对拍。
- 漏斗实体容器路径没有接缓存录制 / 重放。
