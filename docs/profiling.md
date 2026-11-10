# 剖析器与压测工具

`mc-bamboo-game/src/profiler.rs` 给世界线程每刻记一份耗时直方图。本文说明怎么打开它、
报告里每一项是什么、怎么用假玩家给服务端加负载，以及第一份基线数据。

## 1. 打开剖析器

设环境变量 `BAMBOO_PROFILE=<秒>` 后启动服务端。世界创建时读一次，按这个间隔用
`tracing::info!` 打一份报告，打完把样本清零，下一份报告只覆盖之后的那些 tick。
没设、设成 0、负数或解析不了都当成关闭（仍累计但不打印）。

```bash
BAMBOO_PROFILE=10 cargo run --release --bin bamboo -- 127.0.0.1:25565
```

`RUST_LOG` 可以调日志级别，默认是 `info`；剖析报告是 `info` 级别的，日志级别调到
`warn` 以上就看不到。报告打在服务端进程的标准输出里。

## 2. 报告里每一项

报告分两段。第一段 `phases (by mean)` 按平均耗时降序列出 `World::tick` 的 15 个阶段：
`inbox`（处理入站消息）、`light_updates`、`weather_and_time`、`scheduled_ticks`、
`random_ticks`、`block_events`、`entities`、`block_entities`、`player_misc`、`views`、
`block_change_broadcast`、`entity_sync_and_tracking`、`chunk_sending`、`flush`、`tick_tail`。
每行是 `count`（样本数）、`mean`、`p50`、`p95`、`p99`、`max`，单位 ms。
分位数落在对数直方图的桶下界上，是偏保守的估计；桶在 0..15 ns 是逐纳秒，
之上每个 2 的幂再分 8 份，相对误差不超过 12.5%。

第二段 `metrics` 里，`tick_total`、`tick_start_lateness` 和
`tick_interval_deviation` 是时间（ms）：

| 名称 | 含义 |
|---|---|
| `tick_total` | `end_tick − begin_tick`，一刻的实际总耗时 |
| `tick_start_lateness` | 实际开始时刻减去调度器给出的应开始时刻；提前醒了记 0 |
| `tick_interval_deviation` | \|相邻两刻**实际起点**的间隔 − 50 ms\|，设计稿 1.2「抖动 < 2 ms」看这一项 |
| `tick_spin_us` | 这一刻为等下一刻自旋了多久（微秒）；`plain` 和追赶中的刻记 0 |
| `inbound_messages` | 每刻从信箱取出并处理的入站消息数 |
| `online_players` | 每刻开始时的在线玩家数 |
| `due_scheduled_ticks` | 每刻开始时到期的方块/流体计划刻数 |
| `entities` | 每刻开始时的实体数 |
| `chunks_sent` | 每刻写进出站缓冲的区块数；这一刻一个都没发也要记 0 |
| `chunk_latency_us` | 一个区块从进入待发送集合到写进出站缓冲，过了多少微秒（墙上时间） |
| `flush_bytes` | 这一刻 `Links::flush` 交给连接层的全部 `Outgoing` 的未压缩字节数；一个都没有也记 0 |
| `skipped_catch_ups` | 这段时间内放弃追赶的次数（只有累计值，没有直方图） |

`tick_start_lateness` 量的是「每刻都比计划晚多少」，`tick_interval_deviation` 量的是
「刻与刻之间均不均匀」：睡眠精度带来的固定 4 ms 偏差只会出现在前一项里。
刻间隔偏差的口径是**只统计相邻两刻都按 50 ms 网格正常调度**的那一对
（上一刻睡过、这一刻没在追赶、也没放弃过追赶）：落后时连续跑的刻之间没有睡眠，
间隔由上一刻的耗时决定，量的是完全不同的东西，会把偏差撑成几十毫秒，
所以追赶中的刻既不记样本、也不作为下一对的基准；放弃追赶后重新起步的
第一对同理不记。每份报告打印清零样本时也会丢掉「上一刻」的基准，
所以跨报告的第一对不算样本。

`chunk_latency_us` 是墙上时间：区块进入 `pending` 时记 `Instant`，写进出站
缓冲时取经过的微秒数，不再是「过了几 tick」。稳态下同一刻入队、同一刻
发出的区块读数很小但不是恒为 0；它只覆盖排队到写出这一段，不含读盘，
不能直接和设计稿的「读盘 p95 < 100 ms」比较。`flush_bytes` 是压缩前的字节数
（共享区块帧按引用展开计数），用来看每刻出站量，不含连接层自己的心跳等包。

数量指标的直方图记的是「每刻多少个」（区块延迟是「每个区块多少微秒」），
所以这些行的 `mean`、`p50`、`p95`、`p99`、`max` 单位不是 ms，而是每行末尾的 `unit`。
`count` / `samples` 是这段报告的样本数：`phases`、`tick_total`、
`tick_start_lateness`、`tick_interval_deviation`、`tick_spin_us` 和 `flush_bytes` 每刻一次
（追赶和放弃追赶的刻少掉一条间隔偏差样本），`chunk_latency_us` 每发出一个区块一次。
报告头部 `N ticks` 是这段时间内跑了多少刻。

## 3. 假玩家压测工具

`tools/mc-bamboo-loadtest`（二进制 `bamboo-loadtest`）是自己写的最小协议客户端，
不用真实的 Minecraft 客户端或服务端。它只依赖 `mc-bamboo-protocol`、tokio 和仓库里
已有的 crate。所有假玩家共用一个 tokio 运行时，每名玩家一条异步任务，
不会一名玩家一个线程。

```bash
cargo build --release -p mc-bamboo-loadtest

./target/release/bamboo-loadtest \
    --addr 127.0.0.1:25565 --bots 50 --pattern walk --layout spread \
    --duration 60 --ramp 5
```

参数：

- `--addr`：服务端地址，默认 `127.0.0.1:25565`。
- `--bots`：假玩家数，默认 1。`--ramp` 秒内均匀放人。
- `--pattern`：`stand`（原地）、`walk`（固定方向匀速，默认 4.3 格/秒）、
  `fly`（固定方向 33 格/秒，创造模式飞行）、`wander`（出生点附近半径 8 格内随机游走，
  默认 4.3 格/秒）。`--speed` 可以覆盖速度，超过 200 格/秒会被拒绝。
- `--layout`：`cluster`（都在出生点附近）、`spread`（每人一个不同方向，越走越远）。
- `--duration`：跑多少秒；`--ramp`：多少秒内把假玩家陆续连上。
- `--json <文件>`：额外写一份机器可读的报告，基线脚本用。
- `--view-distance` 只属于 `serve`；假玩家请求的视距固定为 8，和 Bamboo 默认值一致。
- `--name-prefix`：玩家名前缀，默认 `Bot`，加四位编号后不超过 16 个字符。

假玩家走完握手 → 登录 → 配置 → Play，离线模式，支持服务端默认的压缩（阈值 256），
不支持加密。配置阶段回应 `select_known_packs`（声明认识官方内容包）；进服之后发
`player_loaded`，回应心跳、传送确认和 `chunk_batch_finished`（期望速率 64 区块/tick，
服务端允许的上限），按 20 Hz 发 `move_player_pos` 和 `client_tick_end`。
`fly` 还发一次 `player_abilities` 的飞行位，服务端默认游戏模式是创造模式，允许飞行。
收到的其它包只统计，不解析。位置每 50 ms 前进不到 2 格（33 格/秒），
在服务端的速度检查范围内。

结束时打印：连上的、登录失败的、中途掉线的（带原因）、每秒收到的字节/包数/区块数、
每秒发出的字节数、心跳往返时间的 p50/p95/p99/max。往返时间由每秒一次的
`ping_request` / `pong_response` 量出，和心跳回应走同一条连接任务路径。

`--json` 报告里还有每名掉线玩家的编号、掉线时刻和原因，以及每秒的逐秒计数。

### 内置服务端 `serve`

命令行版 `bamboo` 的 `ServerConfig::default()` 把 `max_players` 固定为 20，
`crates/mc-bamboo/src/main.rs` 又没有任何配置入口（环境变量、配置文件都没有），
50 人和 200 人场景会被「服务器已满」拒绝。桌面端有 `server-settings.json`，
命令行版没有。**这是服务端以后要补的一条**：命令行版没有配置项可改，
人数上限、视距、压缩阈值等都改不了。

基线因此用压测工具自己的子命令起服，两个进程分开，不共用 tokio 运行时：

```bash
BAMBOO_PROFILE=10 ./target/release/bamboo-loadtest serve \
    --addr 127.0.0.1:25977 --max-players 500 --view-distance 8
```

`serve` 照 `crates/mc-bamboo/src/main.rs` 的结构用现有的公开接口
（`Server::new`、`Server::run`）起服，日志初始化方式相同；配置是
`ServerConfig::default()`，只覆盖 `max_players`（默认 500）和 `view_distance`
（默认沿用 `ServerConfig::default()` 的 8）。和命令行版 `bamboo` 的差别只有：
人数上限、可选的视距、日志里第一行写的是 "loadtest server is listening"、
玩家名前缀是 `Bot`。游戏逻辑、tick 顺序和网络参数都没改；没有改 `crates/` 下任何代码。

`ops.json` 的 `bypassesPlayerLimit` 现在参与登录准入：满员时只有这个位为真的 OP
能进来（官方 `DedicatedPlayerList.canBypassPlayerLimit`）。基线里的假玩家都不是 OP，
所以这一项不影响上面任何一组数据。

## 4. 一键跑基线

```bash
tools/mc-bamboo-loadtest/run-baseline.sh [结果目录] [场景名...]
```

不带参数时依次跑五个场景，每个场景结束后重启服务端，收集服务端的剖析报告
（`server.log`）和压测工具的统计（`client.log`、`client.json`）到临时目录，
结束时杀掉服务端、删掉临时目录。传了结果目录就保留目录；`KEEP=1` 也保留。
`PORT`（默认 25977）、`RAMP`（默认 5 秒）、`PROFILE_SECS`（默认 10 秒）可以改。
只想跑其中几个场景就把名字列在结果目录后面。

| 场景 | 假玩家 | 移动 | 布局 | 时长 |
|---|---|---|---|---|
| idle | 1 | stand | cluster | 30 s |
| crowd-50 | 50 | wander | cluster | 60 s |
| crowd-200 | 200 | wander | cluster | 60 s |
| walk-50 | 50 | walk | spread | 60 s |
| fly-20 | 20 | fly | spread | 60 s |

脚本最后用 `summarize.py` 打一张汇总表并写 `summary.tsv` / `summary.json`。
剖析器每 10 秒打完报告就清零样本，没法从多份直方图精确合并分位数，
所以表里的时间分位数、阶段平均耗时和数量指标是**多份 10 秒报告按样本数加权的平均**；
`tick max` 是这些报告各自最大值的最大者；只统计在线人数峰值达到目标人数 90%
以上的报告。客户端的「每秒收到字节数」是整个 `--duration` 的逐秒平均。

## 5. 第一份基线数据

> 这一节和下一节是**历史记录**，里面的指标名（`jitter` / `chunk_latency_ticks`）
> 是当时的旧名，对应现在的 `tick_start_lateness` / `chunk_latency_us`（口径已改，
> 数值不能直接比较）。第 5a 节是改名之后重新跑的基线。

测量环境：

- Apple M4 Pro，10 个性能核 + 4 个能效核（共 14 核），48 GiB 内存，macOS，arm64。
- Rust 1.99.1，`cargo build --release -p mc-bamboo-loadtest`（release，默认优化档）。
- 提交 `ac746c9`（2026-11-09，PST）。基线跑的是这一版的 release 二进制；
  之后的提交只动文档和压测工具的 tick 节奏整理，服务端的剖析器和游戏逻辑没变。
- 服务端 `BAMBOO_PROFILE=10`，`--max-players 500 --view-distance 8`，
  压缩阈值 256，离线模式，超平坦世界；压测工具 `--ramp 5`。
- 五个场景都在同一台机器上跑，服务端进程和假玩家进程会互相抢 CPU；
  这组数据不是「客户端在另一台机器」的结果。

| 场景 | tick p50 / p95 / p99 / max (ms) | jitter p50 / p99 (ms) | 耗时最多的三阶段（平均 ms） | 每刻区块数 | 区块延迟 p95 (tick) | 客户端 B/s | 掉线 |
|---|---|---|---|---|---|---|---|
| idle | 0.257 / 0.502 / 0.721 / 4.190 | 4.020 / 4.719 | random_ticks 0.217, inbox 0.034, chunk_sending 0.016 | 0.5 | 6.000 | 1 259 | 0 |
| crowd-50 | 1.485 / 3.452 / 5.287 / 19.528 | 4.107 / 4.981 | inbox 0.623, random_ticks 0.618, chunk_sending 0.271 | 20.0 | 4.365 | 333 732 | 0 |
| crowd-200 | 4.239 / 6.643 / 7.211 / 10.631 | 3.452 / 4.719 | inbox 1.343, entity_sync_and_tracking 1.302, random_ticks 1.149 | 79.1 | 4.364 | 4 975 601 | 0 |
| walk-50 | 1.966 / 3.713 / 4.412 / 6.874 | 3.626 / 4.894 | random_ticks 1.122, inbox 0.537, chunk_sending 0.315 | 29.2 | 3.201 | 212 835 | 0 |
| fly-20 | 2.556 / 4.631 / 5.549 / 11.784 | 3.495 / 4.719 | random_ticks 2.133, chunk_sending 0.433, inbox 0.234 | 52.1 | 1.295 | 133 776 | 0 |

五个场景的 `connected` 都等于目标人数，`login_failures` 和 `drops` 都是 0，
服务端日志里没有 `moved too quickly`、`moved wrongly`、飞行踢或断开警告。
心跳往返的 p99 在 0..1 ms，最大值 1..7 ms。客户端每秒收到的包/区块数：
idle 12.9 / 11.0，crowd-50 23 938.5 / 400.8，crowd-200 381 497.3 / 1 586.2，
walk-50 12 426.0 / 585.6，fly-20 2 378.9 / 1 047.2。

汇总用的报告数：idle 3 份（602 tick），其余各 6 份（约 1 204 tick）。

## 5a. 第二份基线数据（新指标）

测量环境和第一份相同（Apple M4 Pro、release 构建、五个场景、`BAMBOO_PROFILE=10`、
`--max-players 500 --view-distance 8`、压缩阈值 256、离线模式、`--ramp 5`），
日期 2026-11-10（PST），基线跑的是提交 `557b7e9` 的工作树（父提交 `88f6de3`），
即第一份基线 `ac746c9` 之后的改动只涉及剖析器指标和报告口径，游戏逻辑没变。
汇总表的列换成新指标：
`lateness` 是 `tick_start_lateness`，`interval dev` 是 `tick_interval_deviation`，
`chunk p95` 现在是微秒，`flush bytes/tick` 是压缩前的每刻出站字节数。

| 场景 | tick p50 / p95 / p99 / max (ms) | lateness p50 / p99 (ms) | interval dev p50 / p99 (ms) | 耗时最多的三阶段（平均 ms） | 每刻区块数 | chunk p95 (us) | flush bytes/tick | 客户端 B/s | 掉线 |
|---|---|---|---|---|---|---|---|---|---|
| idle | 0.311 / 0.612 / 1.027 / 28.118 | 3.670 / 5.942 | 1.354 / 6.281 | random_ticks 0.285, inbox 0.034, chunk_sending 0.018 | 0.5 | 229 376 | 3 963.2 | 1 259.3 | 0 |
| crowd-50 | 1.604 / 3.670 / 4.498 / 19.777 | 3.583 / 5.503 | 1.398 / 5.066 | inbox 0.660, random_ticks 0.656, chunk_sending 0.272 | 20.0 | 179 035 | 158 135.4 | 333 724.9 | 0 |
| crowd-200 | 3.277 / 7.602 / 8.738 / 12.961 | 5.680 / 9.437 | 2.840 / 9.088 | inbox 1.309, entity_sync_and_tracking 1.227, random_ticks 1.066 | 79.2 | 178 451 | 794 918.7 | 4 975 744.7 | 0 |
| walk-50 | 1.747 / 3.931 / 5.328 / 9.679 | 4.632 / 9.437 | 2.753 / 11.265 | random_ticks 1.039, inbox 0.586, chunk_sending 0.283 | 29.2 | 131 256 | 218 656.5 | 212 803.8 | 0 |
| fly-20 | 1.790 / 4.432 / 5.414 / 10.648 | 4.195 / 6.469 | 1.310 / 6.383 | random_ticks 1.687, chunk_sending 0.353, inbox 0.237 | 52.2 | 53 240 | 379 534.6 | 133 827.0 | 0 |

五个场景的 `connected` 都等于目标人数，`login_failures` 和 `drops` 都是 0。
汇总用的报告数：idle 3 份（603 tick），其余各 6 份（约 1 205 tick）。

新指标的读数：`tick_start_lateness` 仍稳定在 3.6..5.7 ms（p50），和第一份基线
4 ms 左右的固定晚起一致；`tick_interval_deviation` 的 p50 只有 1.3..2.8 ms，
说明刻与刻之间确实比「每一刻都晚约 4 ms」均匀得多，但 p99 在 5.1..11.3 ms，
设计稿 1.2「抖动 < 2 ms」在尾部分位数上还没达到。`chunk_latency_us` 稳态不再是
恒为 0：p95 在 53..229 ms（首批区块排队和被背压压住的那几个批次占主导，
对数直方图的分位数落在桶下界），`flush_bytes` 随人数和移动量从 4 KB/tick 涨到
795 KB/tick，和客户端每秒收到的字节数同向。

## 5b. 第三份基线数据（暂停后台编译后）

前两份基线测量时，这台机器上同时有四五个编译任务在跑（别的开发任务），尾部分位数不可信。
这一份在测量前把那些任务全部暂停、等了 20 秒：开始时 1 分钟平均负载还有 9.1（在回落），
结束时 3.7；测量期间仍有图形界面等日常进程在用 CPU（窗口服务约半个核）。所以它比前两份干净，
但不是一台空机器。其余条件和第二份相同，提交 `89d4b27`，2026-11-10（PST）。

| 场景 | tick p50 / p95 / p99 / max (ms) | lateness p50 / p99 (ms) | interval dev p50 / p99 (ms) | 耗时最多的三阶段（平均 ms） | 每刻区块数 | chunk p95 (us) | flush bytes/tick | 掉线 |
|---|---|---|---|---|---|---|---|---|
| idle | 0.382 / 0.611 / 1.071 / 4.424 | 3.408 / 4.719 | 1.486 / 4.544 | random_ticks 0.302, inbox 0.049, chunk_sending 0.021 | 0.5 | 229 376 | 3 969.8 | 0 |
| crowd-50 | 2.403 / 3.757 / 4.893 / 15.991 | 3.495 / 4.807 | 1.289 / 4.456 | random_ticks 0.876, inbox 0.866, chunk_sending 0.377 | 19.8 | 180 606 | 156 993.7 | 0 |
| crowd-200 | 6.466 / 8.738 / 9.612 / 11.749 | 4.719 / 5.592 | 1.268 / 7.335 | inbox 2.098, entity_sync_and_tracking 1.796, random_ticks 1.623 | 79.4 | 178 943 | 796 870.2 | 0 |
| walk-50 | 3.189 / 4.587 / 5.591 / 10.726 | 3.102 / 4.981 | 1.529 / 4.457 | random_ticks 1.625, inbox 0.856, chunk_sending 0.422 | 29.2 | 131 276 | 218 478.2 | 0 |
| fly-20 | 4.281 / 5.898 / 6.815 / 14.400 | 3.102 / 4.807 | 1.464 / 4.632 | random_ticks 3.294, chunk_sending 0.588, inbox 0.462 | 52.2 | 53 369 | 379 534.3 | 0 |

和第二份（有后台编译）对比，量到的是：

- `tick_interval_deviation` 的 p99 从 5.1..11.3 ms 降到 4.5..7.3 ms，p50 基本没变（1.3..1.5 ms）。
  也就是说后台编译只解释了尾部的一部分；机器安静时 p99 仍在 4.5 ms 以上，
  设计稿 1.2 的“抖动 < 2 ms”在 p50 上达到、在 p99 上没达到。
- `tick_start_lateness` 没有变化：p50 仍是 3.1..4.7 ms。每一刻比网格晚三到五毫秒，和负载无关。
- **tick 耗时反而变长了**：`crowd-200` 的 p50 从 3.3 ms 到 6.5 ms，`fly-20` 从 1.8 ms 到 4.3 ms，
  各阶段大致同比例变长。原因没有查证。一个待验证的解释是：机器空闲时，这条每 50 ms 醒一次、
  只干几毫秒的线程被系统放到了能效核或者较低的频率上；机器繁忙时所有核都在高频。
- 单刻最大值（10..16 ms）在安静的机器上仍然出现，不全是后台编译造成的。

## 6. 从数据里看到的（第一份基线，历史记录）

- **哪一阶段最贵**：1 人空服时 `random_ticks` 占主导（0.217 ms，tick 总耗时 p50
  0.257 ms）。200 人聚集时 `inbox`（1.343 ms）和 `entity_sync_and_tracking`
  （1.302 ms）变成了前两名，`random_ticks` 只到 1.149 ms。20 人以 33 格/秒飞时
  `random_ticks` 又回到第一（2.133 ms），因为跟踪范围随位置不断变化、每 tick 要
  跑随机刻的区块集合变大；`chunk_sending` 也升到 0.433 ms，每刻发出 52.1 个区块。
- **随人数怎么涨**：同是 cluster + wander，50 人 tick p50 1.485 ms，200 人 4.239 ms，
  约 2.9 倍（人数 4 倍）；p99 从 5.287 到 7.211 ms，约 1.4 倍。每刻发出的区块数
  从 20.0 到 79.1，客户端每秒收到的字节从 0.33 MB 到 4.98 MB，约 15 倍。
  `crowd-50` 的 tick max 19.528 ms 是五个场景里最高的单刻尖峰，比 200 人场景的
  10.631 ms 还高，这一份报告里的 p99 也因此偏高。
- **对上设计稿第 1.2 节的目标**：`tick p99 < 45 ms` 在所有场景都达到（最高 7.211 ms）。
  `抖动 < 2 ms` 没达到：jitter p50 在 3.452..4.107 ms、p99 在 4.719..4.981 ms，
  空服也有 4.020 / 4.719 ms，看起来是调度/睡眠精度而不是负载造成的。
  `单次 < 5 ms` 的「不顿挫」目标要看单次尖峰：idle 的 tick max 4.190 ms 之内，
  其余四个场景的 tick max 在 6.874..19.528 ms，都出现了超过 5 ms 的单刻。
  设计稿的「读盘 p95 < 100 ms」目前没有读盘，报告里的 `chunk_latency_ticks`
  量的是从进入待发送集合到写进出站缓冲的 tick 数，不是毫秒；不能和这个目标直接比。
- `chunk_latency_ticks` 有一处**计数对不上**：每个场景第一份 10 秒报告的 p95 是
  6 tick（首批区块入队后攒了几刻才发），之后各份报告的 p95 全是 0，但同期的
  `chunks_sent` 每刻仍有几十个区块（例如 fly-20 稳态约 49 个/tick）；`idle` 后两份
  报告没有发出区块，样本数为 0。汇总表里的 `chunk p95` 是按样本数加权的平均，
  只由有样本的报告决定：`idle` 是 6.000，`crowd-50` / `crowd-200` 约 4.36，
  `walk-50` / `fly-20` 更低是因为稳态报告里 0 的样本数更多。
  不能把它当成稳态的区块延迟读。这个疑点已记给主代理审查，本任务没有改服务端。

## 7. tick 节拍的可切换策略

世界创建时读一次下面三个环境变量，启动时用一条 `tick pacing` 日志打出实际生效的值
（`sleep`、`spin_max_us`、`qos`）。读到不认识的值记一条警告并回落到缺省值。
和 `BAMBOO_PROFILE` 一样，`run-baseline.sh` 起服务端时会继承这些变量；
脚本在汇总表标题行前打印它们，`summarize.py` 也加了一列 `spin us/tick`。
本节只说明开关做了什么，不含测量结论；默认行为是原来的 `thread::sleep`。

| 变量 | 取值 | 缺省 | 含义 |
|---|---|---|---|
| `BAMBOO_TICK_SLEEP` | `plain` / `hybrid` | `plain` | 怎么睡到下一刻 |
| `BAMBOO_TICK_SPIN_MAX_US` | 微秒数 | `6000` | `hybrid` 时每刻最多愿意自旋多久；0 等价于 `plain` |
| `BAMBOO_TICK_QOS` | `default` / `interactive` | `default` | 世界线程的优先级设置 |

- **plain**：`thread::sleep` 一直睡到下一刻的截止时刻；系统提前唤醒时补睡，
  返回时一定不早于截止时刻。追赶中的刻（已经落后）不睡也不自旋。
- **hybrid**：先睡到「截止时刻 − 余量」，剩下的时间用 `spin_loop` 自旋到截止时刻。
  余量取最近 64 次睡眠「睡过头了多久」的最大值再加一点，夹在 250 us 和
  `BAMBOO_TICK_SPIN_MAX_US` 之间（余量还不会超过这一刻本来要等的时间）；
  自旋期间这条线程占满一个核。上限是每刻愿意付的占核代价，设为 0 就完全退回
  `plain`。睡眠超时太多、导致醒来时已经过了截止时刻，这一刻直接返回，不多自旋。

`tick_spin_us` 每刻记一条：`hybrid` 记这一刻实际自旋的墙上时长（从睡眠醒来
到截止时刻，最多不超过自旋上限；睡眠超时直接过了截止时刻就记 0），`plain`、
上限为 0 以及追赶中的刻记 0。它量的是为了准时付出的 CPU 占用，不是 tick 本身的
耗时，也不算在 `tick_total` 里。

线程优先级的平台现状：

- macOS：`BAMBOO_TICK_QOS=interactive` 时，世界线程开始跑主循环前调
  `pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0)` 把自己设成用户
  交互级 QoS。这是仓库里少数用 `unsafe` 的地方，只在这一小段函数里；调用失败
  只记一条警告，不影响运行。`default` 不改动线程优先级。
- Linux：这次没有实现优先级设置，开关 `interactive` 是空操作并记一条警告。
  睡眠精度在这些平台上的具体表现没有验证。
- Windows：这次没有实现优先级设置，开关 `interactive` 是空操作并记一条警告，
  留给以后；睡眠精度也没有验证。

设计稿第 12 节还要求 tick 线程绑到性能核，这次没有做。
