# M3 存档设计

这份文档是 M3（存档）的设计约定。**第 12 节第 1–4 步（NBT、生成器数据、
区块 NBT 写读、Anvil）已经实现并和官方代码对拍过，都是编解码，不接进游戏**；
再往后的存档线程、加载线程、ticket 和卸载按 `world-model-design.md` 的区块平面做。
线程规则沿用 `folia.md` 的 R1–R12，crate 和 tick 顺序沿用 `design.md`。
官方类名和方法名都写出来，方便逐行核对。

所有“官方”指 26.3（protocol 777，`DataVersion` 5023）的服务端代码。Linear 格式没有正式
规范，第 7 节分开标了**确定**（读过参考实现源码）和**推测**（没有权威来源）。

## 1. 目标和范围

做到：

- 关服和周期保存，重启后世界、玩家位置和物品栏都在。
- 存档格式可插拔。先做和原版二进制兼容的 **Anvil**（`.mca`），再做 **Linear**（`.linear`）。
- 原版客户端不参与，但**原版服务端和官方代码要能读 Bamboo 写出的存档**，Bamboo 也要能读原版存档。
- **保存绝不在世界线程上做**。世界线程只拿快照，NBT 编码、压缩、写盘都在后台线程。
- 一个 Anvil ⇄ Linear 的转换工具。

不做（写明理由）：

| 不做 | 理由 |
|---|---|
| 世界生成 | 还是平坦模板。存档只负责持久化改动过的区块，生成是 M4 的事 |
| 实体存档（`entities/`） | 只编解码下落方块、点燃的 TNT 和地上的物品三种；没有 mob，也还没接进加载/卸载 |
| POI（`poi/`） | 没有村民 |
| 结构、`chunk_tickets.dat`、游戏规则和世界生成设置的 saved data | 没有对应的游戏功能，写空壳没有意义 |
| DataFixerUpper | 只支持一个版本。读到 `DataVersion` 不等于 5023 的区块就报错拒绝，不猜 |
| 多维度 | 只有主世界。目录结构按多维度留好（见第 2 节） |
| 真正的 ticket 和区块卸载 | 第 12 节第 5 步（区块平面）才做，前四步只是编解码 |

## 2. 磁盘布局

26.3 的布局和老版本不一样，这些是从 `LevelResource`、`DimensionType.getStorageFolder` 和
`DimensionStorageFileFix`、`PlayerStorageFileFix` 两个 data fixer 里读出来的：

```text
<world>/
  level.dat          level.dat_old          session.lock
  data/                                     全局 saved data
  players/
    data/<uuid>.dat  <uuid>.dat_old         玩家数据（老版本在 players/ 下）
    advancements/  stats/
  dimensions/minecraft/overworld/
    region/r.<x>.<z>.mca                    区块（老版本在 <world>/region/）
    entities/  poi/
    data/minecraft/chunk_tickets.dat
```

要点：

- **主世界也在 `dimensions/minecraft/overworld/` 下面**，没有根目录的 `region/`，也没有
  `DIM-1`、`DIM1`。路径来自 `DimensionType.getStorageFolder`：命名空间是 `minecraft` 时也不省略。
- 老存档由 `DimensionStorageFileFix` 和 `PlayerStorageFileFix` 搬过来。Bamboo 不做迁移，
  只按新布局读写；读到老布局就当成空世界，日志里说清楚。
- `session.lock` 是一个内容为雪人字符 `☃` 的文件，官方 `DirectoryLock` 对它做
  `FileChannel.tryLock()` 独占锁，防止两个进程开同一个世界。Bamboo 照做：拿不到锁就拒绝启动。

## 3. 决定

| # | 问题 | 决定 | 理由 |
|---|---|---|---|
| S1 | 存档格式抽象 | `trait ChunkStorage`：`read(ChunkPos) -> Option<Vec<u8>>`、`write(ChunkPos, Option<&[u8]>)`、`flush()`。进出都是**未压缩的 NBT 字节**，压缩是实现内部的事 | Anvil 每区块一段压缩流，Linear 整个区域一段，只有这个边界两边都成立 |
| S2 | 先做哪个 | Anvil 先做并且作为默认。Linear 是可选项 | 只有 Anvil 能和官方代码对照验证，Linear 没有权威规范 |
| S3 | 快照在哪一层 | **不用 `ChunkSnapshot`**：`ChunkColumn`（区块束）本身就是搬运单位，存盘时复制它，读盘时从 NBT 组装它 | 见 `world-model-design.md` 4.2；这一节原来写的 `ChunkStore::snapshot` 已废弃 |
| S4 | 后台线程数 | 一条“存档线程”（NBT 编码 + 压缩 + 写盘）加一条“加载线程”，都是 `std::thread` + `std::sync::mpsc` | 对应官方 `IOWorker` 的单 IO 线程。两条分开，读不被写堵住 |
| S5 | 压缩方式 | Anvil 默认 zlib（压缩 ID 2），对应官方 `region-file-compression=deflate`。读的时候认 gzip、zlib、none | 和原版默认一致，用已有的 `flate2`，不加依赖 |
| S6 | lz4 区块 | **不支持**，读到压缩 ID 4 就报错 | 原版默认不写 lz4，要加新依赖，见第 13 节 Q2 |
| S7 | 调色板上盘形式 | 磁盘上永远是“调色板 + 下标”，位数按调色板大小重算，和网络格式分开写 | 官方 `PalettedContainer.pack` 就是这样，见第 5.2 节 |
| S8 | 坏区块 | 当成“这个区块不存在”，日志 `warn`，继续跑。不 panic，不删文件 | 对应 `ChunkMap` 读失败后重新生成的行为 |
| S9 | 红石记忆 | `RedstoneMemory` 按位置切片存进区块 NBT 的 `block_entities`，火把抖动表不存 | 官方把它们放在方块实体里；火把表是 `RedstoneTorchBlock.RECENT_TOGGLES` 这个 static `WeakHashMap`，官方也不存 |
| S10 | 玩家数据 | `players/data/<uuid>.dat`，字段名照官方，只写 Bamboo 真的有的那几个 | 官方读的时候缺字段都有默认值 |
| S11 | 保存节奏 | 关服一次全量，运行中每 300 秒（6000 tick）扫一遍脏区块；单区块 10 秒内不重复写 | 10 秒冷却是官方 `ChunkMap` 的 `nextChunkSaveTime = now + 10000` |
| S12 | `sync-chunk-writes` | 不实现，不开 `DSYNC` | 官方默认 `true`，Bamboo 先按 `false` 的行为做，见第 13 节 Q4 |

## 4. Crate 结构

编解码层已经落地（第 12 节第 1–4 步）。它不接进游戏，磁盘上不会因为它多出任何东西：

```text
 mc-bamboo-storage        ← 新增：chunk_nbt、anvil、player、level_dat
   ├── mc-bamboo-world    读盘后组装 ChunkColumn，卸载时读它
   ├── mc-bamboo-protocol 物品类型和物品的存档 codec
   ├── mc-bamboo-nbt      网络形式 + 文件形式
   └── mc-bamboo-core  mc-bamboo-data
```

- **`mc-bamboo-storage`**：依赖 `mc-bamboo-core`、`mc-bamboo-world`、`mc-bamboo-nbt`、
  `mc-bamboo-data`、`mc-bamboo-protocol`、`flate2`、`thiserror`。依赖 `mc-bamboo-protocol`
  只为了物品类型和 `components::archive::slots` 的带槽位物品编解码（玩家物品栏、
  末影箱和箱子等容器的 `Items` 都要用）；公开接口只露物品相关的类型，不露网络包和
  编解码缓冲。**以后物品模型搬出 protocol 时这条依赖跟着换。**不依赖 `mc-bamboo-game`、
  `tokio`。已有模块：`chunk_nbt`（写和读）、`anvil`（区域文件、扇区分配、`.mcc`）、
  `player`、`level_dat`（编解码，不接进游戏）、`chunk_content`（跨类型的 NBT 合并与实体
  文件根，纯函数）。`linear`、`paths` 还没做。
- **`mc-bamboo-nbt`**：网络形式（`write_network` 写无名根）之外，加了文件形式：
  带名字的根（`write_named` / `read_named`）、gzip / zlib / 不压缩外壳、解压后的
  字节数上限，标签本身沿用原来的配额（`Accounter`）和 512 层深度上限。
- **`mc-bamboo-world`**：**不加** `ChunkStore::snapshot` / `insert_loaded` / `take_dirty`，
  也不加 `ChunkSnapshot` 这个类型。存档接在区块束上：`ChunkColumn` 是编码的输入、
  解码的输出。新增的公开项只有读取和构造：
  - `ChunkColumn::light_on()`；`ChunkColumn::section(layer, SectionPos)`、`chunk()`、
    `heightmap(kind)` 这些只读接口原本就有。
  - `ChunkColumnParts` / `SectionParts` / `ColumnPartsError` 和
    `ChunkColumn::from_parts(parts)`：输入是磁盘调色板形式（调色板 + long 数组）、
    四张高度图、26 段两层光照、告示牌和 update tag。构造时不逐格重放 `set_block`，
    直接建 `PalettedContainer`，按调色板下标统计段计数，高度图缺了才从方块重算。
    所有输入都当成不可信数据，越界和长度不对返回 `Err`。
  - `palette` 模块公开 `disk_bits` / `disk_data_len` / `packed_capacity` /
    `read_disk_index` / `write_disk_index`，`PalettedContainer::packed_for_disk`，
    磁盘位数和网络位数分开算。
  - `mc-bamboo-blocks` 的 `BlockTicks::take_chunks` / `insert_chunk` main 上已经有了，
    不用补；这次也没碰 `mc-bamboo-blocks` 和 `mc-bamboo-game`。
- 方块实体的内容（箱子里的物品等）、计划刻、红石记忆在更上层，不在 `ChunkColumn` 里。
  `chunk_nbt` 的编码接口留了位置（`ChunkEncodeExtra`）：调用方可以给每个方块实体位置
  提供一段额外的 NBT 复合标签，以及 `block_ticks` / `fluid_ticks` 列表；解码时把
  不认识的顶层键、整条方块实体条目、计划刻和 `Heightmaps` 原样交还（`DecodedChunk`）。
  每条新写的方块实体条目都按官方 `LevelChunk.getBlockEntityNbtForSaving` 补
  `components: {}` 和 `keepPacked: 0b`。谁拥有状态，谁负责写它的键：
  `mc-bamboo-blocks::archive` 写方块实体自己的键、计划刻、红石记忆和三种实体；
  游戏层写容器、`block_items`、命令方块和讲台；`chunk_content` 只做按位置的合并。
  `BlockEntity` trait 的 `write_additional` 没有默认实现，新加一种方块实体漏了存盘会编译失败。
- 第 5 步以后（存档线程、加载线程、ticket 和卸载）按 `world-model-design.md` 的
  区块平面来做：`(区块位置 → 未压缩 NBT 字节)` 的 IO 线程接在流水线的两头，
  解析和序列化在流水线里做；不照这一节原来写的 `Saver` / `Loader` 线程方案。

## 5. 区块 NBT

### 5.1 字段

官方 `SerializableChunkData.write()` 的写入顺序（NBT 复合标签无序，但按同样顺序写方便对比字节）：

| 键 | Bamboo 的来源 |
|---|---|
| `DataVersion` | `mc_bamboo_data::version::DATA_VERSION` = 5023，生成器导出，不手写 |
| `xPos` / `zPos` | `ChunkPos` |
| `yPos` | `MIN_Y / 16` = −4 |
| `LastUpdate` | 调用方给的 `last_update`（接进游戏后来自 `GameTime`） |
| `InhabitedTime` | 调用方给的 `inhabited_time`（现在写 0） |
| `Status` | `"minecraft:full"`；读的时候不是 `full` 就明确拒绝，交给生成流水线 |
| `UpgradeData` | 不写（官方只在非空时写） |
| `blending_data` / `below_zero_retrogen` | 不写（官方 `storeNullable` 只在非空时写） |
| `sections` | 见 5.2 |
| `isLightOn` | 光照算完了才写 `true`；官方为 `false` 时**不写这个键**，不是写 `false` |
| `block_entities` | `ChunkColumn` 的方块实体类型 + 告示牌，加上调用方给的每个位置的额外 NBT，见 5.4 |
| `entities` | 不写（官方只在 PROTOCHUNK 时写这个键） |
| `block_ticks` / `fluid_ticks` | 调用方原样给的两条列表；**即使为空也必写**（`saveTicks` 无条件 `store`） |
| `PostProcessing` | 24 个空列表（一段一个） |
| `Heightmaps` | 4 个，见 5.3；读进来时原样保留官方只给了三张的情况 |
| `structures` | `{ starts: {}, References: {} }`，**必写** |

`DataVersion` 不等于 5023 的区块读的时候明确报错（高于和低于分开报），不做升级。
`CompoundTag` 用 `HashMap`，官方写出的键顺序是哈希序，不是写入顺序；和官方对拍要
按键比较规范化的 NBT，不能逐字节比，见第 11 节。

### 5.2 sections 和调色板

`sections` 是一个列表，每项：`Y`（byte）、`block_states`、`biomes`、`BlockLight`、`SkyLight`。

- `Y` 遍历的是**光照段范围**，不是方块段范围。官方 `SerializableChunkData.copyOf` 从
  `minLightSection` 到 `maxLightSection` 循环，只要“有方块段 **或** 有光照”就产出一项。
  主世界方块段是 −4..=19（24 段），光照段是 −5..=20（26 段），所以 Y = −5 和 Y = 20 这两项
  只有光照、没有 `block_states`。Bamboo 的 `LIGHT_SECTION_COUNT` 已经是 26，范围对得上。
  注意「遍历 26 段」不等于「一定写 26 项」：某一光照段既没有方块段、又没有非空的
  `DataLayer` 时，官方 `write()` 会跳过这一项。超平坦模板的 Y = 20 就没存光照，
  `sections` 只有 25 项；手工在 −5 和 20 都放上光照后才是 26 项。
- `BlockLight` / `SkyLight` 是 2048 字节的 byte array。官方 `copyOf` 跳过
  `DataLayer.isEmpty()`（没分配数组且默认值 0）；Bamboo 的 `ChunkColumn` 里
  `None`（没存光照）和 `Some(Filled(0))`（存着、全是 0）不同，为了原样往返，
  存着的层一律写成 2048 字节，解码时把均匀的数组还原成 `Filled`。
  官方 `parse` 读成非空的 `DataLayer`，`write()` 也会写回来，对拍按键一致。
- `block_states` / `biomes` 的磁盘形式是 `{ palette: [...], data: [long...] }`，
  `data` 在位数为 0（单值）时**整个键都不写**。

**调色板的位数和网络上不一样**，这是最容易写错的地方。官方 `PalettedContainer.pack()`：

1. 用一个新的 `HashMapPalette` 重新编码全部内容 —— **没用到的调色板项会被丢掉**。
   Bamboo 的 `PalettedContainer::repack` 从不收缩调色板，上盘前必须做这一步。
2. `bitsOnDisc = Strategy.getConfigurationForPaletteSize(palette.len()).bitsInStorage()`。
   `getConfigurationForPaletteSize` 先算 `ceillog2(n)`，再交给 `getConfigurationForBitCount` 查表：
   方块 0 位 → `ZERO_BITS`；**1..4 位都落到 `FOUR_BITS_LINEAR`**（所以 2..=16 项都是 4 位）；
   5..8 位各用各的；>8 走 `Configuration.Global`，而 `Global` 的 `bitsInStorage` 就是
   请求的位数 `ceillog2(n)`，**不是 16 位全局 ID**。生物群系 1..3 位直接用，>3 也是
   `Global(ceillog2(n))`。
3. 所以磁盘上**永远存调色板下标**，位数 = 方块的查表结果、生物群系 `ceillog2(n)`，
   `n == 1` 时位数为 0、不写 `data`。实测边界：方块 1 项 → 0 位，2 / 4 / 5 / 16 项 → 4 位，
   17 项 → 5 位，256 项 → 8 位，257 项 → 9 位；生物群系 2 项 → 1 位，3 / 4 项 → 2 位，
   5 / 8 项 → 3 位，9 项 → 4 位。
   Bamboo 的 `Rules::bits_for_palette` 是网络形式（超过 `max_indirect_bits` 返回 `global_bits`），
   上盘用 `palette::disk_bits` 另算。
4. 读回来时官方按 `palette.size()` 自己算位数，和 `PackedData.bitsPerEntry` 不符就报
   `"Invalid bit count, calculated X, but container declared Y"`；位数非 0 却没有 `data` 报
   `"Missing values for non-zero storage"`。磁盘形式里没有显式的 `bitsPerEntry` 字段
   （`PackedData` 的 `data` 是 `lenientOptionalFieldOf`，缺了就是 −1，跳过这个校验），
   所以解码器按调色板长度自己算位数并检查 long 数组长度。
   `Configuration.Global.alwaysRepack()` 是 `true`，读进来之后会重编码成内存里的全局 ID 形式。
5. long 数组是按字（word）对齐的 —— 一个 long 里塞 `64 / bits` 个条目，跨不过 long 边界。
   Bamboo 的 `read_packed` / `write_packed` 已经是这个规则。
   条目下标 `(y << b | z) << b | x`，方块 `b = 4`（4096 项），生物群系 `b = 2`（64 项）。

官方 `BlockState.CODEC` 对方块的**默认状态**写成裸注册名字符串，非默认状态才写
`{ Name: "minecraft:stone", Properties: { ... } }`；`Properties` 的值都是字符串。
Bamboo 存的是状态 ID，靠 `mc-bamboo-data` 的 `blocks::BLOCKS`（`name`、`first_state`、
`default_state`、`properties`，最后一个属性变化最快）把 ID 还原成名字加属性，不手写表。
生物群系的名字来自同步注册表 `minecraft:worldgen/biome` 的条目列表，生成器给每个
注册表模块都导出了 `NAMES` / `by_name` / `name_of`（第 5.4 节说「只有数字 ID」已过时）。

### 5.3 Heightmaps

官方 `ChunkStatus.FINAL_HEIGHTMAPS` 是 `OCEAN_FLOOR`、`WORLD_SURFACE`、`MOTION_BLOCKING`、
`MOTION_BLOCKING_NO_LEAVES` 四个，`full` 区块只保存这四个 —— 正好是 Bamboo 的四个
`HeightmapKind`。NBT 键是 `Heightmap.Types` 的大写名字（`WORLD_SURFACE` 等）。
9 位一项、256 项、按字对齐 → 37 个 long，和 `Chunk::heightmap` 的输出一致。
`WORLD_SURFACE_WG` / `OCEAN_FLOOR_WG` 是世界生成阶段用的，`full` 区块不写。

### 5.4 方块实体

`block_entities` 是一个列表，每项至少有 `id`（注册名字符串）、`x`、`y`、`z`（绝对坐标）。

`mc-bamboo-data` 的 `registries::block_entity_type` 本来就导出了 `NAMES`（下标就是 ID）；
生成器现在还给每个注册表模块补了 `by_name` / `name_of` 两个查表函数，流体的注册名也一样，
不用手写表。

Bamboo 现在真的有内容的方块实体，和官方字段名的对应（都核对过官方源码）：

| Bamboo | 官方类 | 磁盘字段 |
|---|---|---|
| `SignContent` | `SignBlockEntity` | `front_text`、`back_text`（各有 `messages`（4 条）、`color`、`has_glowing_text`）、`is_waxed` |
| `RedstoneMemory::comparator_outputs` | `ComparatorBlockEntity` | `OutputSignal`（int） |
| `RedstoneMemory::bookshelf_slots` | `ChiseledBookShelfBlockEntity` | `last_interacted_slot`（int） |
| `RedstoneMemory::jukebox_playing` | `JukeboxBlockEntity` | `RecordItem`、`ticks_since_song_started` |
| `RedstoneMemory::torch_toggles` | `RedstoneTorchBlock.RECENT_TOGGLES` | **官方不存**，static `WeakHashMap`，重启后清空 |

`messages` 官方是 4 个 `Component`，`ComponentSerialization.CODEC` 在 NBT 上把纯文本写成裸字符串。
Bamboo 只有纯文本（见 `mc-bamboo-core/src/sign.rs` 的模块注释），直接写字符串就和官方一致。
`color` 和 `has_glowing_text` 用 `optionalAlwaysPresentFieldOf` 包着：读的时候可省
（默认 `black` 和 `false`），**写的时候一定写**。`filtered_messages` 是真正的可选字段，
离线模式没有聊天过滤，不写。
`SignContent::editor`（官方 `playerWhoMayEdit`）官方也不存盘，Bamboo 不存。

其余有方块实体类型的方块，`ChunkColumn` 只记了「这里有一个方块实体」。上盘时写
`{ id, x, y, z }`，没有别的字段；调用方可以按位置给一段额外的 NBT 复合标签，
原样合并进条目（编解码器自己写的键优先，附加内容只补别的键）。解码时整条原始复合标签
原样交还，不认识的键不会丢。官方读到缺字段的箱子会得到一个空箱子，这是可接受的
数据丢失（Bamboo 本来也没有箱子内容），但要写进 README 的已知限制。

### 5.5 待执行刻

官方 `SavedTick` 的键：`i`（类型注册名）、`x`、`y`、`z`、`t`（延迟，相对当前刻）、`p`（优先级）。
`LevelChunkTicks.pack(gameTime)` 先写 pending 再写队列，保持 sub-tick 顺序；
`unpack` 把 sub-tick 序号从 `-pendingTicks.size()` 开始重排。

Bamboo 的 `BlockTicks` 是**一条全局队列**，不是官方的每区块 `LevelChunkTicks`。上盘时按位置
过滤出属于这个区块的刻，按执行顺序写；`t = trigger_tick - now`。延迟是相对值，所以用
`RegionTick` 还是 `GameTime` 做基准都不影响磁盘内容，只要装回来时用同一个时钟。

装回来时 sub-tick 顺序按列表顺序重新发号，和官方 `unpack` 等价。**但跨区块的相对顺序丢了**：
原本在同一 tick 同一优先级、分属两个区块的两个刻，保存再读回之后谁先谁后由区块加载顺序决定。
官方有同样的问题（它也是每区块独立 pack），但顺序来源不同。这是第 13 节 Q6。

`i` 是方块或流体的注册名（生成器两张表都有）。`SavedTick.codec` 的 `t` 是 int，
Bamboo 的 `ChunkTicks.delay` 是 i64：写之前饱和到 i32 范围并报告，不照官方的强转回绕。
`p` 是 int 优先级，`TickPriority.byValue` 把越界值折到两端。

## 6. Anvil

### 6.1 格式

文件 `region/r.<regionX>.<regionZ>.mca`，`regionX = chunkX >> 5`。扇区 4096 字节。

```text
扇区 0–1：8192 字节头
  offset[1024]     大端 i32：高 24 位是扇区号，低 8 位是扇区数
  timestamp[1024]  大端 i32，秒
下标 = (chunkX & 31) + (chunkZ & 31) * 32

每个区块：
  i32 大端  长度（含下面那 1 字节）
  u8        压缩 ID
  ...       压缩后的 NBT
  补 0 到整扇区
```

压缩 ID 来自 `RegionFileVersion`：1 gzip、2 zlib（官方叫 `deflate`，带 zlib 头，不是裸 deflate）、
3 不压缩、4 lz4、127 自定义。默认由 server.properties 的 `region-file-compression` 决定，
默认值 `"deflate"`（`DedicatedServerProperties` 里字段名拼错成 `regionFileComression`）。

超大区块：扇区数 ≥ 256（1 MiB）时写外部文件 `region/c.<x>.<z>.mcc`，区域文件里只留 5 字节桩：
长度写 1、压缩 ID 或上 `128`（`RegionFile.EXTERNAL_STREAM_FLAG`）。
外部文件先写临时文件，再 `Files.move(..., REPLACE_EXISTING)` 作为提交动作。

打开文件时官方会校验并修掉坏表项（`RegionFile` 构造函数）：扇区号 < 2（压到头上）、
扇区数 == 0、`扇区号 * 4096 > 文件大小` → 该项清零并 `warn`；头不足 8192 字节也 `warn`。
Bamboo 照做。

扇区分配用一个 bitset，前两个扇区预留（`RegionFileStorage` 的 LRU 缓存 256 个打开的区域文件，
Bamboo 也用 LRU，上限做成配置）。

### 6.2 写入顺序和崩溃安全

官方 `RegionFile.write` 的顺序，Bamboo 一模一样照抄这个顺序：

1. 在**空闲扇区**里分配新位置（绝不覆盖还在用的数据）。
2. 写载荷（超大则写 `.mcc` 临时文件）。
3. 改内存里的 offset 和 timestamp。
4. 一次 `write(header, 0)` 把 8192 字节头整个写回。
5. 跑提交动作（`.mcc` 的 rename）。
6. 释放旧扇区。

诚实地说：**这个顺序不是原子的**。8192 字节的头写到一半断电，可能一半新一半旧。官方就是
这样，Bamboo 不发明新格式（否则官方代码读不了）。好处是第 1 步保证旧数据还在原地，
所以“半新半旧的头”最差的结果是部分区块回到上一个版本，不会读到垃圾。

`sync-chunk-writes=true`（官方默认）时通道用 `DSYNC` 打开，每次写都落盘。
`close()` 会补齐到整扇区再 `force(true)`。Bamboo 第一版不开 `DSYNC`（S12），
但关服时做一次 `sync_all`。

## 7. Linear

### 7.1 确定的部分

来源：`LinearRegionFileFormatTools` 仓库里的 `linear.py`（参考实现，读过源码）。

```text
32 字节超级块（大端，Python struct ">QBQbhIQ"）
  u64  magic = 0xc3ff13183cca9d9a
  u8   version（写 1；读接受 1 和 2）
  u64  newest_timestamp
  i8   compression_level
  i16  chunk_count（非空区块数）
  u32  complete_region_length（下面那段压缩数据的长度）
  u64  保留 / hash，恒为 0，读的时候不校验

<complete_region_length> 字节：一整段 zstd 流，解压后是
  8192 字节表：1024 × (u32 size, u32 timestamp)，没有偏移量
  然后是所有非空区块的**未压缩 NBT**，按下标顺序紧挨着

8 字节尾：magic 再写一遍
```

- 文件名 `r.<x>.<z>.linear`，区域坐标**只在文件名里**，数据里没有。参考实现的 TODO 里
  自己写了“没有校验文件名”。
- 每个区块的位置靠累加前面所有 `size` 得到。`size == 0` 就是空区块，没有数据体。
- 区块体是**裸 NBT，没有 5 字节头、没有每区块压缩**。压缩只有外层那一次 zstd
  （写入时 `checksumFlag: 1`）。
- 下标 → 坐标：`x = 32 * regionX + i % 32`，`z = 32 * regionZ + i / 32`，X 是快变轴，和 Anvil 一样。
- 读取时的校验和报错：`Superblock invalid`、`Version invalid`、`Footer signature invalid`、
  `Decompressed size invalid`（所有 size 之和 + 8192 要等于解压长度）、`Chunk count invalid`。
- 写入是 `.wip` 临时文件 → flush → fsync → 改时间戳 → rename，**整个文件原子替换**。
- 没有 `.mcc` 这种东西；单个区域文件上限 4 GiB。
- 压缩等级：参考实现的 README 建议 6，Kaiiju 这类服务端默认用 1。

### 7.2 推测的部分

- version 2 和 3 的布局**没有任何权威描述**。参考实现只写 version 1，读的时候接受 2 但不区分
  处理。`blinear_v2`（`.b_linear`）是另一个后继格式，不是 Linear v2。
  **Bamboo 只写 version 1，读到别的版本直接报错。**
- 哪些服务端用哪个版本、v2 加了什么字段 —— 没查到，不猜。

### 7.3 取舍

| | Anvil | Linear |
|---|---|---|
| 和原版兼容 | 是 | 不是，官方代码读不了 |
| 体积 | 基准 | 主世界和下界省约 50%，末地约 95%（来自参考实现 README） |
| 读一个区块 | 解压那一段 | **解压整个区域** |
| 写一个区块 | 改几个扇区加头 | **重写整个文件** |
| 原子性 | 头不是原子的 | rename，整文件原子 |
| 碎片 | 有，要管扇区 | 没有 |
| 超大区块 | `.mcc` | 没有，塞在流里 |

“写一个区块要重写整个文件”是 Linear 的硬伤。社区实现（LinearPaper、Kaiiju）用一个
flush 频率把改动攒在内存里批量写。Bamboo 的 Linear 实现同样要在内存里持有整个区域的
区块字节，按区域批量落盘 —— 这是推导，不是从规范里读到的。

## 8. 崩溃安全和坏数据

| 场景 | 做法 | 官方对应 |
|---|---|---|
| 区块写一半断电 | Anvil：新扇区 + 一次写头（见 6.2）。Linear：`.wip` + rename | `RegionFile.write` |
| `level.dat`、玩家数据 | 临时文件 → 备份成 `*_old` → 删 → rename，失败回滚 | `Util.safeReplaceFile`（10 次重试） |
| 读 `level.dat` 失败 | 退回 `level.dat_old` | `LevelStorageSource` |
| 读玩家数据失败 | 退回 `<uuid>.dat_old`；原文件另存成 `<uuid>_corrupted_<日期>.dat`，玩家按新玩家进服 | `PlayerDataStorage.backup` |
| 区块解压失败、NBT 坏、`DataVersion` 不对 | 当成“没有这个区块”，`warn` 一条带坐标的英文日志，用模板。**不删文件、不覆盖**，等用户处理 | `ChunkMap` 读失败后重新生成 |
| 区域文件头坏 | 坏表项清零（见 6.1），其余区块照常读 | `RegionFile` 构造函数 |
| 同一个世界被开两次 | `session.lock` 独占锁，拿不到就拒绝启动 | `DirectoryLock` |
| 存档线程 panic | 和世界线程 panic 同级（R12）：记日志，走关闭流程 | 官方把 IO 异常传回主线程 |

磁盘满、权限不足这类错误不能让世界线程崩。存档线程把错误计数发回世界线程，连续失败就
在日志里喊，并且停止接受新的保存请求（内存里的数据还在，关服时再试一次）。

## 9. 玩家数据和 level.dat

### 9.1 `players/data/<uuid>.dat`

gzip 的 NBT，根复合标签有名字（空字符串）。字段名全部核对过官方
（`Entity.saveWithoutId`、`Player.addAdditionalSaveData`、`ServerPlayer.addAdditionalSaveData`）：

| 键 | 类型 | Bamboo 来源 | 官方出处 |
|---|---|---|---|
| `DataVersion` | int | 生成器导出的 5023 | `NbtUtils.addCurrentDataVersion` |
| `UUID` | int array（4 项） | `Player::uuid` | `Entity.TAG_UUID` |
| `Pos` | 3 个 double | `Player::position` | `Entity.TAG_POS` |
| `Motion` | 3 个 double | 全 0 | `Entity.TAG_MOTION` |
| `Rotation` | 2 个 float（yaw、pitch） | `Player::yaw`、`pitch` | `Entity.TAG_ROTATION` |
| `OnGround` | bool | `Player::on_ground` | `Entity.TAG_ON_GROUND` |
| `Dimension` | string | `"minecraft:overworld"` | `ServerPlayer.java:428` 的 `putString("Dimension", ...)` |
| `playerGameType` | int（legacy id） | `Player::game_mode` | `ServerPlayer.storeGameTypes` |
| `Inventory` | list，每项带 `Slot` | `Inventory::slots` | `Player.addAdditionalSaveData` |
| `SelectedItemSlot` | int | `Inventory::selected` | 同上 |

不写的字段（官方读的时候都有默认值）：`fall_distance`、`Fire`、`Air`、`Invulnerable`、
`XpP`/`XpLevel`/`XpTotal`/`XpSeed`、`Score`、`SleepTimer`、`abilities`、`EnderItems`、
`foodData` 的几项、`warden_spawn_tracker`、`recipeBook`、`respawn`、`seenCredits`、
`ShoulderEntity*`。创造模式里这些都不影响玩法。

`Inventory` 和 `EnderItems` 的每一项是 `ItemStackWithSlot.CODEC`，它把
`ItemStack.MAP_CODEC` 平铺在同一层；箱子等容器的 `Items` 也是这个格式：

```text
{ Slot: 0b, id: "minecraft:stone", count: 1 }
```

- `Slot`：**byte 标签**（`ExtraCodecs.UNSIGNED_BYTE` 是 `Codec.BYTE` 加无符号映射，
  `UnsignedBytes.toInt`），用 `optionalAlwaysPresentFieldOf` 包着 —— 读的时候可省
  （默认 0），**写的时候一定写**。Bamboo 的 [`SlottedItem::slot`](`mc-bamboo-protocol`
  的 `components::archive::slots`) 是 0..=255 的 `i32`。
- `id`：物品注册名字符串（`Item.CODEC_WITH_BOUND_COMPONENTS`）。
- `count`：**int 标签**，`ExtraCodecs.intRange(1, 99)`，同样是“读可省、写必写”。
  超出 1..=99 官方直接报错，Bamboo 的编码器返回 `Err`。
- `components`：`optionalFieldOf(..., DataComponentPatch.EMPTY)`，**补丁为空时不写**；
  只含瞬态组件的补丁会写成空复合标签。Bamboo 的 `ItemStack` 现在有组件，
  `mc-bamboo-protocol` 的 `components::archive` 已经把 122 种组件类型的存档 `Codec`
  全部覆盖（`components java` 对拍 2602 个物品堆）。
- 空槽位不写条目（整个列表只放非空物品）。
- 这一层的编解码在 `mc-bamboo-protocol::components::archive::slots`：一个物品 ⇄ 一个
  复合标签（`encode_item` / `decode_item`）、一个带槽位列表 ⇄ 一个 NBT 列表
  （`encode_list` / `decode_list`）。`ItemStack.CODEC` 是 `MapCodec`，物品条目里
  `id` / `count` / `components` 之外的键会被忽略，写回时不再出现。
- 列表里解不开的条目按官方 `TagValueInput$TypedListWrapper` 处理：报告问题并跳过这一条，
  其余条目照常解出来；列表本身不是 `ListTag` 时按 `listOrEmpty` 当成空列表并报告。
  容器装载时槽位越界的条目也跳过（`ItemStackWithSlot.isValidInContainer`），槽位重复时
  后一条覆盖前一条（`Inventory.load`、`ContainerHelper.loadAllItems`）。官方在
  `DataResult` 带 `partialValue` 时会用部分结果，Bamboo 一律跳过整条并报告，
  这是已知的差别：组件补丁里某一项解不开时官方 DFU 有时能给出「不带这个组件的物品」。
  物品这一层不做 D9 的原样保留；D9 只用在玩家文件和 `level.dat` 的复合标签上。

### 9.2 `level.dat`

gzip 的 NBT，根是 `{ Data: { ... } }`。来自 `PrimaryLevelData.setTagData`：

| 键 | 值 |
|---|---|
| `version` | 19133（NBT 存档版本，不是游戏版本） |
| `Version` | `{ Name, Id, Snapshot, Series }` |
| `DataVersion` | 5023 |
| `version_history` | 版本历史列表 |
| `LevelName` | 世界名 |
| `Time` | `GameTime` |
| `LastPlayed` | 毫秒时间戳 |
| `GameType` | 默认游戏模式 |
| `spawn` | `RespawnData`：`GlobalPos` 加 `yaw`、`pitch` |
| `difficulty_settings` | `{ difficulty, hardcore, locked }` |
| `allowCommands`、`initialized` | bool |

26.3 把世界生成设置和游戏规则搬出了 `level.dat`，变成 `data/` 下独立的 saved data 文件。
Bamboo 不写它们；官方读不到就用默认值，但**官方服务端打开这个世界时会按默认设置生成**，
和 Bamboo 的平坦模板不一样。验证的时候只用官方的类读文件，不开真实服务端（见第 11 节）。

## 10. 转换工具

放在已有的 `tools/` 下面，作为 `mc-bamboo-vanilla` 的新子命令还是独立 bin，见第 13 节 Q8。
行为：

```text
convert <输入目录> <输出目录> --from anvil|linear --to anvil|linear
```

- 逐个区域文件转，不解析 NBT，只搬“未压缩 NBT 字节”（S1 的边界正好够用）。
- 时间戳原样保留。
- 空区块保持空。
- 转完做一遍自检：两边的区块坐标集合相同，每个区块的 NBT 字节相同。
- 不原地转换，不删输入。输出目录非空就拒绝。

## 11. 验证

从便宜到贵，每一级都能单独跑：

1. **单元测试（`mc-bamboo-storage`）**
   - NBT 读写往返，包含空列表、空字符串、嵌套、非法输入。
   - Anvil 头的字节例子（手算的 offset 和 timestamp）。
   - 扇区分配器：写、改大、改小、删，不重叠不漏。
   - Linear 超级块的 32 字节、尾 8 字节、空区块表。
   - 调色板上盘位数的边界：1 项、4 项、5 项、256 项、257 项（方块）；1、2、3、4 项（生物群系）。
2. **官方代码读 Bamboo 写的存档**（`cargo run -p mc-bamboo-vanilla -- storage <java 路径>`，
   和 `probe` 一个路子，`StorageOracle.java` 在 JVM 里调官方类，不启动服务端）
   - 用官方 `RegionFile` 打开 Bamboo 写的 `.mca`，三种压缩 ID 都读一遍；Bamboo 也能读
     官方 `RegionFile` 写出的 `.mca`。`.mcc`、扇区重用、坏表项各有单元测试。
   - 把字节交给官方 `SerializableChunkData.parse`，再 `write()` 回来，和 Bamboo 的原始
     NBT **按键比较规范化的 JSON**：`CompoundTag` 用 `HashMap`，官方写出的键顺序是
     哈希序，逐字节比会假报警。这一步同时抓住调色板位数、光照段范围、heightmap 长度。
   - 官方 `NbtIo.writeCompressed` 写出的 gzip 文件，Bamboo 能读。
3. **Bamboo 读原版存档**
   - 用官方的类在临时目录里造一个存档（只调 `RegionFile` 和 `SerializableChunkData`，
     **不启动服务端、不接受 EULA**），Bamboo 读进来，比较方块、光照、heightmap、方块实体。
4. **往返**：Bamboo 写 → Bamboo 读 → 再写，字节必须完全相同；
   随机区块（固定种子，多种调色板大小、单值段、空段、有/没有光照、告示牌和方块实体）
   编码再解码逐项相等；`from_parts`、区块 NBT 解码器和 Anvil 读取器都有截断、翻位、
   随机输入的模糊测试，不 panic、不超时、内存有上界。
   **基准**：`cargo test --release -p mc-bamboo-storage chunk_nbt_cost -- --ignored --nocapture`，
   分别报告超平坦和内容丰富区块的编码/解码耗时、压缩前后字节数，解码拆成解压、
   NBT 解析成 `Tag`、组装 `ChunkColumn` 三段。
5. **Anvil ⇄ Linear ⇄ Anvil 往返**：字节完全相同。
6. **`probe`**：存档不改网络包，`probe` 只是回归检查。
7. **用户的真实客户端**：放方块、关服、重启、重连，东西还在。这一步只有用户能做。

## 12. 实现计划

每一步都能单独验证，每一步结束时 `cargo fmt --check`、
`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test` 都干净。
**第 1–4 步已完成**（都是编解码，不接进游戏，磁盘上不会因为它们多出东西）。

1. ✅ **NBT 解码**（`mc-bamboo-nbt`）。读取本来就有（网络形式），补了带名字的根、
   gzip / zlib / 不压缩外壳、解压字节数上限；深度和长度上限沿用 `Accounter`。
   验证：往返单元测试、压缩炸弹、截断/翻位、官方 `NbtIo.writeCompressed` 写出的文件能读。
2. ✅ **生成器补数据**。`block_entity_type` 和流体的 `NAMES` 表本来就有；
   生成器现在给每个注册表模块补 `by_name` / `name_of`，重新生成了生成文件。
   验证：`cargo test -p mc-bamboo-data`，双向查表覆盖两个注册表的全部条目。
3. ✅ **区块 NBT 写 + 读**（新建 `mc-bamboo-storage`，模块 `chunk_nbt`）。
   编码输入、解码输出都是 `mc_bamboo_world::ChunkColumn`，不用手搓的 `ChunkSnapshot`；
   `mc-bamboo-world` 只加了只读接口和 `ChunkColumn::from_parts`（调色板形式，直接建容器）。
   上层附加数据走 `ChunkEncodeExtra`，解码出不认识的键走 `DecodedChunk`。
   验证：官方 `SerializableChunkData.parse` 读 Bamboo 写的 NBT 再 `write()` 回来按键一致；
   官方写出的 NBT，Bamboo 读成 `ChunkColumn` 再写出来按键一致，放进 `ChunkStore` 后
   方块 / 高度图 / 光照读数一致；往返字节相同；调色板边界、光照段范围、模糊测试。
4. ✅ **Anvil 读写**。区域文件、扇区分配器、`.mcc`、坏表项修复，压缩 ID 1 / 2 / 3，
   读到 ID 4（lz4）或 127（自定义）在错误里写清楚不支持。Linear 不做。
   验证：第 11 节第 1、2、3、4 条；官方 `RegionFile` 双向、扇区重用、坏表项、模糊。
5. **区块平面的加载和保存**（按 `world-model-design.md` 第 5 节，不按下面原来写的
   `Saver` / `Loader` 线程方案）。IO 线程读写「未压缩 NBT 字节」，解析和序列化在
   流水线里，Director 管 ticket 和区块状态机，region 只收完工的区块束。
   验证：`world-model-design.md` 第 5.3、5.5 的交付和卸载路径；放方块、关服、重启、重连。
6. **玩家数据和 level.dat**。
   验证：第 11 节第 2 条用官方 `NbtIo` 读；重启后位置和物品栏在。
7. **Linear**（可选，看 Q1 的答复）。
   验证：第 11 节第 1、5 条。
8. **转换工具**。验证：第 11 节第 5 条。
9. **README**：删掉“不存档”这条已知限制，补上存档格式、保存节奏、还有哪些东西不存
    （火把抖动表、容器的开合状态等）。箱子内容和三种实体已经有编解码器，见第 5.4 节之后。

第 5 步之前磁盘上什么都不会变，所以前四步可以安全地合进 main。

上层内容的编解码按 `world-model-design.md` 5.5 的 D9 处理：

- 方块实体条目里不认识的键、不认识的 `id` 整条原样保留；同一位置换了类型、或者方块
  被拆掉时，旧条目的键跟着丢掉，不会挂到新方块上。`RecordItem` 变了之后
  `ticks_since_song_started` 回到 0（`chunk_content::BoundKeys`）。
- 计划刻里不认识的 `i` 原样保留在列表最前面（`LevelChunkTicks.pack` 先放 pending）；
  实体文件里不认识的 `id` 和根键原样保留。空实体区块不写文件（`IOWorker.STORE_EMPTY`）。
- 物品条目按 `player-data` 的约定不做 D9（`ItemStack.CODEC` 是 MapCodec，多余的键忽略）；
  已建模的三种实体里官方有而 Bamboo 不建模的键，这次也不保留，读回按官方默认值补齐。
  **这是一条已知限制**：实体还没有持久 UUID 和随身携带的未建模 NBT，做生物之前要补；
  现在 `UUID` 由实体 ID 确定性派生（和网络层同一个推导），读回时 uid 由生成流程重分配。
- 官方不存的 Bamboo 状态一律不写、也不加私有键：火把抖动表（S9）、容器开合人数和
  `recheck_at` / `dead`、潜影盒开盖动画、`HopperEntity.tickedGameTime`、
  `PistonMoving.lastTicked`、各种配方/方块状态缓存、实体的随机源 / `tickCount` /
  上一刻位置 / 碰撞标志 / `start_pos` / `bob_offs`。读回后分别回到构造器默认值。
- 第一段的 `chunk-content` 对拍（`ChunkContentOracle.java`）逐类型调官方
  `BlockEntity.loadStatic` + `saveWithFullMetadata`、`SavedTick.codec` +
  `LevelChunkTicks.pack/unpack`、`EntityType.loadEntityRecursive` + `Entity.save`。
  实体读取用 JVM 里的假世界（`EntityOracle.java` 的 `FakeLevel` 形状），不启动服务端、
  不接受 EULA；`SerializableChunkData.parse -> write` 另外检查 `components` / `keepPacked`
  能从区块 NBT 原样带回。
- 第二段（等 `chunk-director` 落地）再把原样保留的复合标签挂进 `ChunkColumn` 的
  方块实体类型表旁边、给 `ChunkEntities` 加访问器、把游戏层四张表接进条目，并做
  `RegionColumn` ⇄ 「区块 NBT + 实体 NBT」的整块组装。

## 13. 风险和待用户决定的问题

### 风险

| 风险 | 应对 |
|---|---|
| 调色板上盘位数算错，官方读到 `Invalid bit count` | 第 11 节第 2 条的按键对比直接能抓到；`palette_bit_boundaries` 用例钉住了 1 / 4 / 5 / 16 / 17 / 256 / 257 项 |
| 光照段范围写错（漏了 Y = −5 和 20） | 同上；`light_sections_cover_the_two_extra_sections` 用例在两端各放一层光照后检查 26 项 |
| 并发改动 `mc-bamboo-blocks`（有别的代理在改红石） | `BlockTicks::take_chunks` / `insert_chunk` main 上已有，这次没碰这个 crate |
| 内存：31 GB 机器上多个代理同时跑 | 一次只跑一个构建或测试；Linear 要整区域在内存里，region 上限做成配置 |
| 区块束跨线程搬运 | `ChunkColumn` 必须 `Send` 且不借用世界状态；`mc-bamboo-world` 已有 `assert_is_send::<ChunkColumn>()` 的测试 |

### 待用户决定

| # | 问题 | 我的建议 |
|---|---|---|
| Q1 | Linear 要加 `zstd` 这个新依赖（Anvil 不用，`flate2` 已经在工作区里）。做不做？ | 做，但排在最后（第 12 节第 7 步）。Anvil 先上线 |
| Q2 | 要不要支持读 lz4 压缩的区域（压缩 ID 4）？要再加一个 `lz4` 依赖 | 不支持。原版默认不写 lz4，读到就报错，日志里写清楚 |
| Q3 | `docs/README.md` 原本不存在（任务里说“在索引里加一行”）。我新建了它，列出 `docs/` 下所有文档 | 保留。不想要就删掉这个文件，索引换个地方 |
| Q4 | `sync-chunk-writes` 要不要实现（官方默认 `true`，每次写都 `DSYNC`）？开了慢很多 | 第一版不开，关服做一次 `sync_all`。要开就加成配置项 |
| Q5 | `RedstoneMemory` 存哪里？按 S9 切成区块级的方块实体条目和官方一致，但要在 `ChunkStore` 和 `RedstoneMemory` 之间多一层按位置查询 | 按 S9 做。另一条路是单独存一个 `data/bamboo_redstone.dat`，简单但官方读不了 |
| Q6 | 待执行刻跨区块的相对顺序会在存盘后丢（见 5.5）。官方也有这个问题，但顺序来源不同 | 接受。真要一致就得把 `BlockTicks` 改成每区块一条队列，那是另一个任务 |
| Q7 | 转换工具放哪：`tools/mc-bamboo-vanilla` 的新子命令，还是一个独立的小 bin？ | 独立 bin（比如 `tools/mc-bamboo-convert`）。`mc-bamboo-vanilla` 要调 Java，转换工具不该依赖 JDK |
| Q8 | 主世界写在 `dimensions/minecraft/overworld/` 下（26.3 的新布局，见第 2 节），老存档不迁移 | 按新布局。读到老布局就当空世界，不自动迁移（迁移要 DataFixerUpper） |
| Q9 | `level.dat` 不写世界生成设置和游戏规则（26.3 已经搬到 `data/`）。官方服务端打开这个世界会按默认设置生成，和 Bamboo 的平坦模板不一样 | 接受。目标是“官方代码能读”，不是“原版服务端能接着玩” |
