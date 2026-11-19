# 配置文件

Bamboo 从**服务器目录**里读两份文件：

| 文件 | 谁定义的 | 内容 |
|---|---|---|
| `server.properties` | 官方 26.3 专用服务端 | 官方已有的键。键名、写法（`Java Properties` 格式、UTF-8）和默认值都照官方，从官方服务端迁过来的人可以直接沿用原来的文件 |
| `bamboo.toml` | Bamboo | 官方没有的项：代理转发、连接护栏、实体并行线程数和区块信箱预算 |

命令行版和桌面版读写的是**同一套文件**（代码在 `mc-bamboo-server/src/config_files.rs`）。
文件不存在时按默认值启动并写出带默认值和注释的文件；官方第一次启动也会写
`server.properties`，Bamboo 每次启动都把两份文件写回一遍，这样坏掉的值会被换成
默认值、后来新增的键也会补齐。

## 命令行版

```bash
bamboo                                  # 用当前目录作为服务器目录
bamboo --server-dir /srv/bamboo         # 指定服务器目录
bamboo --server-dir=/srv/bamboo         # 等价写法
bamboo 127.0.0.1:25566                  # 临时覆盖这一次的监听地址，不写回文件
```

`--server-dir` 省略时是当前工作目录。位置参数 `host:port` 只覆盖这一次的监听地址，
不写回 `server-ip` / `server-port`，日志会说明覆盖了；不带它时监听地址来自
`server-ip`（空 = `0.0.0.0`）和 `server-port`。别的参数会报错退出。

启动日志里打出两个文件的绝对路径、哪些项不是默认值（密钥类只写「已设置」、
不打值）、以及官方有而 Bamboo 还没实现的键清单（见下面）。

桌面版的服务器目录固定是 APP 数据目录下的 `server/`，设置页顶部显示这个路径。
把命令行版的 `--server-dir` 指到同一个目录，两边读到的就是完全相同的内容。

## `server.properties`：官方已有的项

### Bamboo 已经实现的键

| 键 | 类型 | 官方 26.3 默认值 | 作用 |
|---|---|---|---|
| `online-mode` | bool | `true` | 正版验证。打开后登录加密、每名玩家都要经过 Mojang 的 session server 确认；**关掉才能用离线账号直连** |
| `prevent-proxy-connections` | bool | `false` | 问 session server 时带上客户端 IP，Mojang 会比对玩家报备的那个 |
| `server-ip` | 字符串 | ``（空） | 监听地址，空表示监听所有网卡 |
| `motd` | 字符串 | `A Minecraft Server` | 服务器列表里显示的介绍文字 |
| `level-name` | 字符串 | `world` | 世界名字；**服务器目录 + `level-name` = 世界目录** |
| `server-port` | 整数 | `25565` | 监听端口 |
| `view-distance` | 整数 | `10` | 客户端渲染半径；Bamboo 把它夹在 2..32 |
| `max-players` | 整数 | `20` | 同时在线的玩家上限 |
| `network-compression-threshold` | 整数 | `256` | 包负载超过这么多字节才压缩；负数关闭，0 压缩每个包 |
| `enforce-secure-profile` | bool | `true` | 正版玩家的聊天必须带 Mojang 签过的公钥；没开正版验证或没取到公钥时不生效 |

### 世界生成相关的键

世界生成接进游戏后，这四个键生效（解析在 `mc-bamboo-game/src/world_options.rs`，
接进流水线见 `docs/worldgen.md`）：

| 键 | 类型 | 官方 26.3 默认值 | 作用 |
|---|---|---|---|
| `level-seed` | 字符串 | ``（空） | 空 = 随机种子（首次存档时写进 `world_gen_settings.dat` 后固定）；数字按 `Long.parseLong`；其它字符串按 `String.hashCode()` |
| `level-type` | 字符串 | `minecraft:normal` | 世界预设。认 `normal` / `flat` / `large_biomes` / `amplified`（含旧名 `default` / `largebiomes`）；查不到回退 `normal` 并记日志；**新世界默认 normal，不再是超平坦** |
| `generator-settings` | JSON | `{}` | 只在 `flat` 预设生效，`FlatLevelGeneratorSettings.CODEC` 的 `layers` / `biome`（`features` / `lakes` 等本轮不接）；坏 JSON 回退默认超平坦预设 |
| `generate-structures` | bool | `true` | 先读进来存进 `world_gen_settings.dat`；结构阶段接上后生效，目前结构阶段是空实现 |

**只对全新世界生效**：世界存在 `data/minecraft/world_gen_settings.dat` 时，种子、
世界类型和 flat 预设一律以存档为准，`level-type` / `level-seed` 改了也不影响老存档
（官方行为）。四种 `level-type` 的差别：三种噪声世界共用 `minecraft:overworld`
群系预设，只是 `NoiseGeneratorSettings` 不同（`normal`=`overworld`、
`large_biomes`=`large_biomes`、`amplified`=`amplified`）；`flat` 走 `FlatLevelSource`。
`generate-structures` 目前只存储、不影响生成。

**和 `ServerConfig::default()` 的关系**：写出来的文件和这份表格一律用官方默认值；
`ServerConfig::default()` 是「不读配置文件」的嵌入用法（单元测试、oracle、probe、
压测工具的内置 `serve`）的默认值，保持 Bamboo 原来的取值（`motd=A Bamboo Server`、
`view-distance=8`、`online-mode=false`）。对拍和 probe 不读配置文件，所以不受官方默认值
影响；命令行版和桌面版读文件，默认就是官方值。

### 官方有、Bamboo 还没实现的键

读进来原样保留、写回时原样写出去，不报错。启动时用一条日志列出这些键
「现在不起作用」（个别逻辑按官方默认值硬编码，例如聊天刷屏阈值按 10 秒算）。官方 26.3
默认文件里有、Bamboo 还没实现的共 **55 个**（`IMPLEMENTED` 从 10 个增到 14 个：新增
`level-seed` / `level-type` / `generator-settings` / `generate-structures`）。下面清单还多列了
2 个旧版遗留键（`announce-player-achievements`、`resource-pack-hash`），官方 26.3 默认文件里
已经没有、读到时按未知键原样保留，不算进这 55 个：

`allow-flight`、`enable-code-of-conduct`、`bug-report-link`、`force-gamemode`、
`enforce-whitelist`、`difficulty`、`gamemode`、`management-server-enabled`、
`management-server-host`、`management-server-port`、`management-server-secret`、
`management-server-tls-enabled`、`management-server-tls-keystore`、
`management-server-tls-keystore-password`、`management-server-allowed-origins`、
`announce-player-achievements`、`enable-query`、`query.port`、`enable-rcon`、
`rcon.port`、`rcon.password`、`hardcore`、`use-native-transport`、`spawn-protection`、
`op-permission-level`、`function-permission-level`、`max-tick-time`、
`max-chained-neighbor-updates`、`rate-limit`、`command-spam-threshold-seconds`、
`chat-spam-threshold-seconds`、`simulation-distance`、`broadcast-rcon-to-ops`、
`broadcast-console-to-ops`、`max-world-size`、`sync-chunk-writes`、
`region-file-compression`、`enable-jmx-monitoring`、`enable-status`、
`hide-online-players`、`entity-broadcast-range-percentage`、`text-filtering-config`、
`text-filtering-version`、`resource-pack-id`、`resource-pack`、`resource-pack-sha1`、
`resource-pack-hash`、`require-resource-pack`、`resource-pack-prompt`、
`initial-enabled-packs`、`initial-disabled-packs`、`player-idle-timeout`、
`status-heartbeat-interval`、`white-list`、`log-ips`、`pause-when-empty-seconds`、
`accepts-transfers`。

`management-server-secret` 官方是启动时随机生成的，Bamboo 不实现管理服务器，
生成的文件里这一项是空的。键和值的转义照 Java `Properties.store`（`=`、`:`、
`#`、`!`、反斜杠、开头的空格），读回来时按 `Properties.load` 还原，反复写回不会
把反斜杠越堆越多。

官方键之外的键（未来版本新增的、或者用户自己加的）也原样保留，另用一条日志说明
「未知键已保留」，不算进上面那份清单。注释行不保留——官方 `Properties.store`
也不保留。

### 值非法时怎么办

照官方 `Settings.get` 的行为：

- 整数用 `Integer.parseInt`；解析不了就用默认值，并把这个默认值写回文件
  （例如 `view-distance=abc` 会变成 `view-distance=10`）。Bamboo 另外把
  `view-distance` 夹在 2..32，`server-port` 由 `TcpListener::bind` 检查。
- 布尔用 `Boolean.valueOf`：只有忽略大小写等于 `true` 才是真，**其它任何值都是 false**，
  不报错（例如 `online-mode=YES` 等于关闭）。
- 字符串不解析，原样使用。
- 整个文件读不出来（UTF-8 解码失败、权限问题等）记一条警告并用全默认值。
  官方 `loadFromFile` 在 UTF-8 失败时还会回退 ISO-8859-1，这里没有实现。

## `bamboo.toml`：Bamboo 自己的项

每一项上面有一行中文注释，说明它是什么、默认值是多少。全部是启动时读一次，
改完要重启服务端才生效。

| 键 | 类型 | 默认值 | 作用 |
|---|---|---|---|
| `pre_play_timeout_seconds` | 整数（秒） | `30` | 一个连接从握手到进入游戏的总时限，超时断开 |
| `max_pre_play_connections` | 整数 | `1024` | 同时处于握手、登录、配置阶段的连接上限，超出的连接直接丢弃 |
| `keep_alive_seconds` | 整数（秒） | `15` | 心跳间隔；必须小于客户端的 30 秒读取时限 |
| `forwarding` | `off` / `legacy` / `modern` | `off` | 代理转发模式：BungeeCord 旧式或 Velocity 现代。和 `online-mode` 互斥 |
| `forwarding_secret` | 字符串 | `""` | Velocity 现代转发的签名密钥，和代理的 `forwarding.secret` 内容相同 |
| `auth_connect_timeout_seconds` | 整数（秒） | `5` | 连接 Mojang session server 的时限 |
| `auth_read_timeout_seconds` | 整数（秒） | `5` | 等 session server 回下一段字节的时限 |
| `entity_threads` | 整数 | `0` | 实体并行线程数（实验）。0 = 精确模式，≥ 1 = 性能模式 |
| `chunk_worker_threads` | 整数 | `0` | 区块计算池的线程数：区块的解析、生成和存盘序列化放到多少条常驻后台线程上做。0 = 在世界线程上内联做 |
| `deliveries_per_tick` | 整数 | `64` | Director 信箱每刻最多交付多少个区块；0 表示不限 |
| `reclaims_per_tick` | 整数 | `64` | Director 信箱每刻最多收回多少个区块；0 表示不限 |
| `island_threads` | 整数 | 逻辑核数的一半，夹在 2 到 8 之间 | 岛的 tick 线程数。0 = 齐步（精确模式，岛在世界线程上内联跑，和官方逐位一致）；≥ 1 = 分离（岛的一刻在 tick 线程上跑，和主控并行，区块交付晚一刻）。缺省值在第一次启动时按这台机器算好写进文件。见下面「岛的 tick 线程」 |
| `islands` | 布尔 | `true` | 多岛：相距很远的玩家群各自一个岛，各岛并行跑。`false` = 整个世界一个岛。见下面「多岛」 |
| `island_dead_seconds` | 整数 | `30` | 多岛时一个岛的同一刻在 tick 线程上连续跑超过这么多秒就判为死岛（5 到 3600）。见下面「多岛」 |

### 精确模式 / 性能模式

这一段照 `world-model-design.md` 第 13 节和 `performance-optimizations.md` 第 5 节。

- **精确模式**（`entity_threads=0`，默认）：实体逐个算完立刻落地，和官方逐位一致；
  所有 oracle、probe、单元测试都在这个模式下跑。
- **性能模式**（`entity_threads≥1`）：实体阶段改成「并行算，再按（uid 升序、
  实体内序号）串行落地」。3000 个掉落物加 300 个下落方块的场景里，8 线程整刻
  快 4.0 倍；1 线程走同一份并行代码、只用来对比。剖析器把这段记成
  `entity_compute` 和 `entity_commit` 两段，精确模式仍是 `entities` 一段。

性能模式相对官方的已知差异（目前只实现了实体阶段这一层，第 13 节里跨 region、
随机数序列、区块压缩等差异还不存在）：

1. 实体看到的别的实体是这一刻实体阶段开始时的样子，不是排在它前面的实体刚动完的样子。
2. 物品合并少看一拍：精确模式下 uid 小的物品可能当场合并掉 uid 大的物品；
   性能模式里两者都先算完，最多晚一刻合并。
3. TNT 爆炸的击退晚一刻体现在物理上。
4. 漏斗吸走物品后，物品自己这一刻的 `age` 和位置仍按未被吸走算。

不属于差异的：方块状态、`Effect` 顺序、实体 ID 分配、新实体的随机源播种、
`level.random` 的消费序列。同一个 region 内部结果仍然是确定的：同样输入、不同线程数，
结果相同。

**目前性能模式每刻新起线程，开销大，只用于实验，不建议在正式服开。**

### 区块计算池

`chunk_worker_threads`（`world-model-design.md` 5.3、第 12 节）：

- **0（默认）= 内联执行器**：区块的解析、生成、存盘序列化在世界线程上当场做完，
  和以前完全一样；精确模式、oracle、probe、单元测试都用它。
- **≥ 1**：起这么多条常驻线程（线程名 `bamboo-chunk-worker-N`），Director 派出的
  解析、每个生成阶段、卸载存盘的序列化交给它们，结果在下一刻开头收回。
  读写文件仍在单独的 IO 线程上。
- **区块内容和线程数无关**：同一场景用 0 和 4 跑，盘上的字节逐个相同（有测试）。
  不同的只是区块**到达的时刻**：池子算完的结果最早下一刻才交付，所以开着池子时
  区块可能比内联晚一两刻出现。
- 默认先保持 0。等真实地形生成接进来、压测确认收益之后再改默认值。
- 调试用的环境变量 `BAMBOO_FAKE_GEN_DELAY_US=<微秒>` 让超平坦生成器每个区块多睡
  这么久，模拟慢的生成器，只用于测量，不进配置文件。

### 岛的 tick 线程

`island_threads`（`island-implementation-plan.md` 第 6 节第 7 步、`world-model-design.md`
第 6.2 节和第 12 节）。世界线程就是主控线程，持有 `Global`（时间、天气、规则、名单、
计分板）和 Director（票、区块流水线、存盘编排）；「岛」是一块 `Region`（区块、方块、
实体、玩家）。多岛关着时整个世界一个岛；开着时（默认）每群相距很远的玩家一个岛，
见下面「多岛」。下面说的三段是一个岛时的样子，多岛时由混合驱动按岛各自推进。

- **0 = 齐步**：岛一直在世界线程上，主控和岛按原来的顺序一行不差地交替执行，
  区块在 Director 发出的那一刻当场交付。这是精确模式：所有对拍、探针和要求逐位一致的
  测试都显式选它，不靠缺省值。
- **≥ 1 = 分离（默认 1）**：每刻分三段。① 岛停放在主控手里：岛先收上一刻投来的区块
  信件；主控收入站消息（玩家请求、进服、控制台命令）、推进边界 / 天气 / 时间。
  ② 岛整个（一个指针）交给常驻的 tick 线程（线程名 `bamboo-island-N`）跑它的一刻：
  光照、方块、实体、视图、方块广播、实体同步、发区块，最后照上次的名单编存盘快照；
  主控同时推进 Director（超时票、到点任务、匀速存盘队列），只往岛的邮筒投信、碰不到岛。
  ③ 岛交回、停放：主控收下回执和排队的全局改动、发延迟广播、刷出站缓冲、给下一份
  存盘名单、处理 `/save-all` 和关服。
- **相对齐步的差别只有时序**：区块晚一刻交付（Director 的信件下一刻开头才收）；
  `/save-all` 等岛这一刻交回之后才过栅栏，成功消息晚一刻；岛里算出来的计分板准则
  变化和每 30 秒一次的延迟广播在岛交回后写进玩家的缓冲，和这一刻岛写的包同一次刷出，
  只是排在这一刻的包后面。包的内容、同一刻里岛自己写的包的顺序、盘上字节都不变
  （有测试：线程数 0 / 1 / 3 在没有区块信件的场景里玩家收到的字节逐位相同，
  齐步 / 分离 × 内联 / 计算池四份存档逐字节相同）。
- **玩家的出站缓冲归岛**：只有在岛停放的时候主控才往玩家的缓冲里写（第 ① ③ 段），
  岛在线程上跑的时候主控不碰它，所以主控的包不会和岛的包抢、也不会插到岛这一刻的包
  中间；刷出在第 ③ 段，主控这一刻的包和岛的包同一次交给连接层。
- **栅栏**：`/save-all`、`/stop` 和关服都在岛停放之后做，存下的是这一刻结束时的世界
  （有测试：最后一刻里掉下去的沙子在盘上已经是空气）。关服请求到达时岛还在跑，主控
  再等它 10 秒；还不回来就当它卡死了，不再等：`level.dat` 和 `data/` 照常写、主控手里
  已有的区块快照写完、世界目录的锁放掉，日志里列出岛上存不下来的区块，然后退出。
  没有关服请求时一刻跑得再慢主控也一直等，每 10 秒记一条「岛的一刻还在跑」。
- **崩溃**：岛在 tick 线程上 panic 时，线程把 panic 连同岛一起交回，主控在世界线程上
  原样重新抛出，走原来的「世界线程 panic → 关服、告诉玩家原因」那条路，不会静默挂起；
  tick 线程整个没了（交回的通道断开）同样当成崩溃。
- **线程数**：一个岛一刻只用得上一条 tick 线程；多岛时按岛派发，尽量回到上次那条线程，
  岛比线程多时多出来的进公共队列，谁先空出来谁接。**默认是逻辑核数的一半，夹在 2 到 8
  之间**（`mc_bamboo_game::recommended_island_threads`，第一次启动时算好写进
  `bamboo.toml`；拿不到核数时按 2）。理由（第 10 步 d 段的测量，`docs/profiling.md`
  第 16 节）：① 至少 2 条：只有 1 条时一个过载的岛占着它，别的岛都排在后面，
  `islands-heavy-4` 里四个岛一起掉到 7–15 TPS；2 条起只有重岛掉、其余三个稳在 20 TPS。
  ② 一半：另一半留给主控线程、连接层、区块计算池、IO 线程和机器上的别的进程；
  `islands-16` 在 14 核的机器上 4 条和 8 条的每刻均值都降到多岛关着时的约 0.2 倍（0.23
  对 0.21），8 条的交接 p95 在机器忙时到几毫秒（线程被唤醒得慢）。③ 最多 8：岛再多也只按 8 条并行，空着的线程不占 CPU。
  多岛关着时只有一个岛，多给的线程闲着，没有坏处。
- **剖析器**（`BAMBOO_PROFILE`）分离时多记三项：`master_busy`（主控线程这一刻自己干活
  的时间）、`island_busy`（岛在 tick 线程上的时间）、`island_handoff`（派出到线程开跑，
  加上岛跑完到主控接着干活）；`delivery` 阶段记的是并行那段里主控推进 Director 的耗时。
- **环境变量 `BAMBOO_ISLAND_THREADS=<n>`**：设了就盖过 `bamboo.toml` 和代码里的缺省，
  但不写回文件（启动日志会说明值来自环境变量）。对拍、探针（`tools/probe.sh` 缺省设成 0，
  `PROBE_ISLAND_THREADS=default` 时不设）和测试整进程切精确模式用它。代码里显式给的
  `WorldConfig.island_threads` / `ServerConfig.island_threads` 不受它影响。
  第 6 步的 `BAMBOO_ISLAND_DRIVE` 已经删掉，由它取代：`lockstep` 对应 `0`，`detached`
  对应 `≥ 1`。

### 多岛

`islands`（`island-implementation-plan.md` 第 5 节、第 6 节第 10 步）。**默认开**。分区器
把相距很远（已加载的区块之间隔着至少几个空区块）的玩家群拆成各自的岛，靠近了再合并；
各岛在 tick 线程上并行推进，各有自己的时钟，主控（时间、天气、规则、名单、Director）
每刻 20 次照常走，不等慢的岛（「混合驱动」，计划 5.5）。

- **只有一个岛时**（玩家都在一处）和多岛关着几乎一样：每刻多花约 10 µs 的记账
  （分区器、截止时间、落地检查）。
- **一个岛过载只拖慢它自己**：落后的岛自己追赶，落后超过 2 秒就丢掉欠的刻（日志
  「an island can't keep up」），别的岛和主控照常 20 TPS。前提是 tick 线程不止一条。
- **判死**（计划 5.6）：一个岛的同一刻在 tick 线程上连续跑超过 `island_dead_seconds`
  （默认 30 秒）就判为死岛：它的玩家被断开（提示这一片世界停止响应、上次存盘之后的改动
  丢了、请重连）、它的区块从上次存盘
  重新加载、日志记一条 `ERROR`；服务端和别的岛不受影响。正常的慢（重的命令、区块洪峰）
  到不了 30 秒。环境变量 `BAMBOO_ISLAND_DEAD_SECONDS` 在运行时盖过它；桌面端界面上没有
  这一项。
- **命令**：玩家命令和控制台命令都等全部岛停放再执行（计划 5.5.2）。碰方块、方块实体或
  非玩家实体的命令（`/setblock`、`/fill`、`/clone`、`/summon`、`/loot`、`/gamemode`、
  `/defaultgamemode`、`/enchant`、`/execute`、`/kill`、`/teleport`、`/tp`，计划 4.5）在有
  不止一个岛时把全部岛临时拼成一个执行、再按原样拆回去；拼和拆的耗时和已加载的区块数成
  正比，**每 1000 个已加载区块合计约 6.5 ms**，那一刻的整刻耗时相应变长。其余命令只借
  别的岛上的玩家，不拼；权限不够用这条命令的人发它也不拼。
- **玩家来回跑的服务器**：很多人在相距很远的几处之间频繁往返时，路上的人和身后还没卸载
  的区块会把几处一直连成一个岛（`profiling.md` 第 17 节），这时和多岛关着差不多；玩家
  各自长时间待在远处时并行的收益最大。
- **和精确模式的差异**（全部列在 `world-model-design.md` 第 13 节）：跨岛的传送、命令
  反馈、聊天投递晚一到几刻；各岛的时钟可以短暂不同步（落后的岛在追）；天气的随机序列
  和官方不同（主控用自己的随机源）；岛内的一切和一个岛时相同。
- **精确模式**：`islands = false` 并且 `island_threads = 0`。对拍、探针和要求逐位一致的
  测试都显式选它，不靠缺省值。
- **环境变量 `BAMBOO_ISLANDS=0|1`**：在运行时盖过文件里的值，不写回文件。

### 不在文件里的项

- `session_server_url`（Mojang 的 `hasJoined` 地址）只给测试用，留在 `ServerConfig`
  里由代码指定，不开放给用户改。
- 四份管理名单（`ops.json`、`banned-players.json`、`banned-ips.json`、
  `whitelist.json`）固定在服务器目录里，不单独配置。
- `BAMBOO_PROFILE=<秒>`（剖析器）和 `BAMBOO_TEST_LOADED=1`（测试走已加载模式）
  是调试用的环境变量，保持环境变量，不进配置文件。`RUST_LOG` 调日志级别。
- `BAMBOO_ISLAND_THREADS=<n>`：盖过 `island_threads`，见上面「岛的 tick 线程」。
- `BAMBOO_ISLANDS=0|1`、`BAMBOO_ISLAND_DEAD_SECONDS=<秒>`：盖过 `islands` /
  `island_dead_seconds`，见上面「多岛」。
- `BAMBOO_PROFILE_ISLANDS=1|2`：分区器的统计进剖析报告；`2` 另外每刻全量重算对照，并在每次
  照做拆合之后断言多岛的全部不变量（release 构建也查，长时间压测用，主控每刻多花几到十几
  毫秒）。
- `BAMBOO_ISLAND_MERGE_DISTANCE` / `BAMBOO_ISLAND_SPLIT_DISTANCE` / `BAMBOO_ISLAND_SPLIT_DELAY`：
  只给调参对比用，盖过分区器的合并距离（缺省 2 个 section）、拆分距离（4）和拆分迟滞
  （200 刻）；组合不合法（合并距离 < 1、拆分距离不大于合并距离）就整组用缺省并警告，设了
  就在启动日志里警告一次。正式运行不要设。
- 精确节拍（`BAMBOO_TICK_SLEEP` / `BAMBOO_TICK_SPIN_MAX_US` / `BAMBOO_TICK_QOS`）
  已于 2026-11-12 废弃并删除，见 `performance-optimizations.md` 第 6 节。

### 值非法时怎么办

- 整份 TOML 语法错：记一条警告，全部用默认值。
- 语法对但某个键类型不对、或者超范围：只把**这一项**换成默认值，警告里写文件名、
  键、原值和用的默认值；其它项照读。和官方 `server.properties` 逐键回落一致。
- `forwarding` 只认 `off` / `legacy` / `modern`；`modern` 没有密钥时服务端启动会
  退回 `off`（界面上会当场报错）。
- 未知键忽略，另记一条日志，不报错。
- 所有来自文件的数值都不会触发 panic：实体线程数夹在 0..256、区块计算线程数和岛的 tick 线程数夹在 0..64、信箱预算夹在 0..65536、
  心跳夹在 1..29 秒。

## APP 和命令行怎么共用

- APP 的服务器目录：APP 自己的数据目录 `app_data_dir()/server`。设置页顶部显示这个
  路径；命令行版用 `bamboo --server-dir <这个路径>` 就读写同一份。
- APP 的设置页分成「官方设置」和「Bamboo 优化设置」两个分区：前者写
  `server.properties`，后者写 `bamboo.toml`。每一项下面标了对应的文件里的键名和默认值。
- 老版本 APP 把设置存在应用配置目录的 `server-settings.json` 里。第一次启动时如果
  服务器目录里两个新文件都不存在且老文件存在，就导入一次、写成新文件，之后不再读它。
  老文件**保留不删**（用户数据），只是不再读取。旧文件里「正版验证 + 代理转发」这种
  互斥组合按服务端的行为修正（关掉正版验证）。
- 两个入口都只在启动时读一次，改完文件要重启服务端才生效。

## 存档目录

`level-name` 默认 `world`，`config_files::world_dir(服务器目录, 配置)` 给出
`服务器目录.join(level_name)`。`ConfigFiles::to_server_config` 已经把这个值放进
`ServerConfig.world_dir`，命令行版和桌面版都会据此存档；`None` = 不落盘
（`ServerConfig::default()` 就是这个值，单元测试 / oracle / 压测用它，不会碰世界目录）。
世界线程启动时建 26.3 目录布局、拿 `session.lock` 独占锁、读 `level.dat`，
关服按官方顺序写回并释放锁。格式和已知限制见 `docs/m3-storage.md` 与 README「存档」一节。
