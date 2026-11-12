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

**和 `ServerConfig::default()` 的关系**：写出来的文件和这份表格一律用官方默认值；
`ServerConfig::default()` 是「不读配置文件」的嵌入用法（单元测试、oracle、probe、
压测工具的内置 `serve`）的默认值，保持 Bamboo 原来的取值（`motd=A Bamboo Server`、
`view-distance=8`、`online-mode=false`）。对拍和 probe 不读配置文件，所以不受官方默认值
影响；命令行版和桌面版读文件，默认就是官方值。

### 官方有、Bamboo 还没实现的键

读进来原样保留、写回时原样写出去，不报错。启动时用一条日志列出这些键
「现在不起作用」。共 59 个：

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
`accepts-transfers`、`level-seed`、`generate-structures`、`generator-settings`、
`level-type`。

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
| `deliveries_per_tick` | 整数 | `64` | Director 信箱每刻最多交付多少个区块；0 表示不限 |
| `reclaims_per_tick` | 整数 | `64` | Director 信箱每刻最多收回多少个区块；0 表示不限 |

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

### 不在文件里的项

- `session_server_url`（Mojang 的 `hasJoined` 地址）只给测试用，留在 `ServerConfig`
  里由代码指定，不开放给用户改。
- 四份管理名单（`ops.json`、`banned-players.json`、`banned-ips.json`、
  `whitelist.json`）固定在服务器目录里，不单独配置。
- `BAMBOO_PROFILE=<秒>`（剖析器）和 `BAMBOO_TEST_LOADED=1`（测试走已加载模式）
  是调试用的环境变量，保持环境变量，不进配置文件。`RUST_LOG` 调日志级别。
- 精确节拍（`BAMBOO_TICK_SLEEP` / `BAMBOO_TICK_SPIN_MAX_US` / `BAMBOO_TICK_QOS`）
  已于 2026-11-12 废弃并删除，见 `performance-optimizations.md` 第 6 节。

### 值非法时怎么办

- 整份 TOML 语法错：记一条警告，全部用默认值。
- 语法对但某个键类型不对、或者超范围：只把**这一项**换成默认值，警告里写文件名、
  键、原值和用的默认值；其它项照读。和官方 `server.properties` 逐键回落一致。
- `forwarding` 只认 `off` / `legacy` / `modern`；`modern` 没有密钥时服务端启动会
  退回 `off`（界面上会当场报错）。
- 未知键忽略，另记一条日志，不报错。
- 所有来自文件的数值都不会触发 panic：线程数夹在 0..256、信箱预算夹在 0..65536、
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

## 存档接进来之后

`level-name` 默认 `world`，`config_files::world_dir(服务器目录, 配置)` 给出
`服务器目录.join(level_name)`。存档接进游戏时把这个值放进 `ServerConfig.world_dir`
（`None` = 不落盘）即可，不需要再加新的配置项。
