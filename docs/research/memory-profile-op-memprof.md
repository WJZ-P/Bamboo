# op-memprof：服务器内存构成实测与压缩

## 0. 摘要、分支与提交列表

分支 `op-memprof`，工作树干净、**未推送**。未动其它分支、未碰 `W:\data\GithubProjects\Bamboo`
主目录、未碰 `crates/mc-bamboo/src/main.rs`、未启动真实 Minecraft 服务端 / 客户端、未接受 EULA、
未改任何进程优先级。

相对基点 `e83ce946` 共 **23 个提交**（`git rev-list --count e83ce946..HEAD` = 23）。前 21 个是
更早五次会话的产物（见 §0.1），原型 9 那次会话新增 2 个：

| 提交 | 标题 | 内容 |
|---|---|---|
| **`25bdf76e`** | **Share one compiled generator per seed and drop the router rewrite memo** | 原型 9（代码） |
| **`51a476a9`** | **Document the shared generator and the dropped rewrite memo** | 文档（README / `docs/profiling.md` §20.6 / `docs/worldgen.md` / `docs/worldgen-structures-plan.md`） |

提交信息均为纯英文祈使句、无 `Co-authored-by` 之类署名行。

**最后一次接手（2026-11-22 07:15 之后）没有再改产品代码**：补了一组同一台机器上背靠背的
缓存上限 A/B 实测（§2.7，把推荐 #1 从「旁证」变成同口径数据），并在 `51a476a9` 上把验收
检查重跑了一遍（§9）。分支仍为 23 个提交，工作树干净。

**原型 9 的结论**（两个改动合起来）：

1. **`SeededRouter` 把整份密度函数图钉在内存里**。用计数分配器实测：一个
   `SeededRouter` 建成后留下 **1,464,588 B（≈1.43 MB）**，其中 ≈1.28 MB 是
   `Compiler::rewritten` / `keep_alive` —— `DfRewriteRule.SLICE_UNIFORM_AXES` 的重写记忆，
   用节点地址当键、同时持有原始节点防地址复用，编译完就没用了。`SeededRouter::new`
   现在编译完立刻 `Compiler::release_rewrites()`，同一实例只剩 **149,886 B（≈146 KB）**
   （采样器 + 它们引用的噪声）。
2. **每条计算线程一个 `Generator`，结构线程再多一个**。一个 `chunk::Generator` 实测
   **242,766 B（≈237 KB）**。`Worldgen::shared_generator(settings_id, seed)` 现在按
   `(设置, 种子)` 全进程缓存一份 `Arc<chunk::Generator<'static>>`，地形线程
   （`with_generator`）和结构线程（`StructureThread.generator`）共用；平坦世界要先覆盖
   常数列，仍按线程各建一份。为了让 `Generator` 能跨线程共享，把 `material::System` 里
   唯一的 `Cell` 载体（陶土色带随机源）挪到 `prepare` 的局部变量。
3. **实测**（`s11-profile`，`left` 快照同口径对照 s10）：「worldgen per thread: template
   pools and structure state」**7.84 → 1.54 MB**、「generators」**1.16 → 0.00 MB（1 块）**。
   默认 4 条计算线程时约 **−7 MB 常驻**。`plain` 进程级读数（38.9 / 349.2 / 105.4 MB）
   和 s10（38.8 / 338.6 / 106.9）差在忙机器 ±10–20 MB 噪声里，**7 MB 在进程级读数上
   看不出来**；以分配点实测和归属表为准。
4. **结果逐字节不变**：`chunkgen --through features --seeds 1 --chunks 4`（3545088 项）、
   `structures`（9280 放置 / 9744 选择）、`structure-rings`（864 项）全 0 不一致。

### 0.1 接手说明

分支上已有前序 21 个提交和本报告，七次会话均按「先读汇报 / `git log` / `git status` 接着做，
不要重做」执行，没有重跑前序的八项原型。原型 9 那次会话把「每线程各编译一份密度函数」
（§3 的 #10，归属表里约 6 MB）列为剩下最大的常驻项但判为高风险没做；本会话先用计数分配器
把单个实例的保留量量准（1.43 MB，其中 1.28 MB 是重写记忆），发现**不用拆共享程序**、
只要在编译后丢掉记忆就能拿回大半，再把 `Generator` 本身进程级共享，两项合计约 −7 MB，
且都不改求值路径。

前 21 个提交（沿用，未改）：

| 提交 | 标题 | 归属 |
|---|---|---|
| `33980ce0` | Add a portable desktop build script that skips the installers | 合并带入，与内存无关 |
| `ee813725` | Add a heap profiler that attributes live server memory to owners | 工具 |
| `b4f05aea` | Keep only shapes of structure templates and bound the parsed cache | 原型 1 |
| `e31d860f` | Shrink cached compressed chunk frames to their real length | 原型 2 |
| `4947a869` | Separate per-thread worldgen copies in memprof and add a heap compaction probe | 工具 |
| `20bd51c9` | Pin the world seed in the memprof run script | 工具 |
| `f1d80793` | Document the memory profiler and the structure template savings | 文档 |
| `aa3c4994` | Share the feature registry across all generation threads | 原型 3（代码） |
| `cd5c822a` | Document the shared feature registry memory savings | 原型 3（文档） |
| `53b3adca` | Share the structure state and biome table across generation threads | 原型 4（代码） |
| `b77c2ad7` | Document the shared structure state memory savings | 原型 4（文档） |
| `7f9e5ebe` | Record the shared structure state in the design status | 设计文档现状 |
| `c361c4ce` | Keep only the runtime worldgen registries in memory | 原型 5（代码） |
| `beb8f2b0` | Document trimming the worldgen raw tables | 原型 5（文档） |
| `72888506` | Reuse the shared worldgen tables instead of re-parsing the document | 原型 6（代码） |
| `ba0f24b8` | Parse the jigsaw template pools once into Worldgen | 原型 7（代码） |
| `7f6591de` | Measure the working set that can be trimmed and guard the settings shortcut | 回归单测 + `run.sh` 的 `TRIM=1` 实验 |
| `c1c33acb` | Document the removed re-parses and the shared template pools | 文档（§20.4） |
| `fe65bf37` | Share one sine table and the structure preset R-tree | 原型 8（代码） |
| `94e49748` | Document the shared sine table and forked biome R-tree | 文档（§20.5） |
| `50da35d9` | Tighten the measured attribution range in the profiling notes | 文档 |

## 1. 方法与口径

- 工具 `tools/mc-bamboo-memprof`：`serve` 把全局分配器换成记账分配器（`tracker.rs`），每次
  分配抓最多 32 层调用栈、按分配点累计还活着的字节，`owners.rs` 用函数名规则归到大类；
  `os.rs` 另读 Windows 进程工作集 / 私有提交、`HeapWalk` 忙块与空闲块、`VirtualQuery`
  地址空间分段和线程栈。`report` 把快照按归属打印成表。符号解析靠 PDB。
- 脚本 `run.sh <结果目录> [端口] [profile|plain]`：固定种子 **20261121**、视距 **8**、
  离线模式，一名 `bamboo-loadtest` 假玩家在线；三时刻读数：起服后空载（`SETTLE`）、
  在线（离开前 10 s）、玩家离开 `AFTER` 秒后。`TRIM=1`（仅 `plain`）在离开后再读
  `settled`（+3 s）、调 `EmptyWorkingSet`、再读 `trimmed`。
- 本轮新增一个**计数分配器**小实验（临时 example，跑完即删）：包一层
  `System` 统计活着的字节，在同一进程里量 `Worldgen::shared()` 之后建一个
  `SeededRouter` / `Generator` 的净增量。数值见 §2.6，这是本轮判断依据里最硬的一份。
- `plain` 用不带分析器的 `bamboo.exe`，**是真实服务端进程级读数**；`profile` 带分析器，
  每块多 ≥8 字节头 + 76 MB 站点表，进程读数偏大，只看比例和归属。
- 本轮所有实测、对拍与官方探针都经 `node W:/data/bamboo-agents/tools/heavy.js` 排队独占
  机器级锁，串行执行，端口 26221–26223。
- 机器是忙机器（31 GB，另有队员在跑 cargo / 测试 / 探针）。**忙机器读数 ±10–20 MB 属正常**，
  绝对值以归属表的常量项为准；同一次运行内的前后差比跨运行横比可靠。

## 2. 实测明细表

### 2.1 进程级真实读数（不带分析器的 `plain`，MB）

| 时刻 | 优化前（文档） | 三项原型后（s6） | 五项原型后（s8） | 七项原型后（s9） | 八项原型后（s10） | **九项原型后（s11）** |
|---|---:|---:|---:|---:|---:|---:|
| 起服后空载 | — | 50.4（private 39.6） | 38.9（private 23.4） | 37.7（private 21.9） | 38.8（private 22.9） | **38.9（private 22.9）** |
| 一人在线（视距 8） | 588 | 216.6 | 192.5（缓存几乎为空，见下） | 332.2（private 315.9） | 338.6（private 324.0） | **349.2（private 332.4）** |
| 玩家离开后 | 505 | 157.3 | 128.3 | 296.7 / 283.7，3 s 后 104.2 / 90.1 | 离开 20 s 已 106.9 / 95.2 | **离开 20 s 105.4 / 90.7** |

（最后一次接手的同机 A/B 见 §2.7：默认预算在线 371.6，减半 313.7。那次与 s11 的
349.2 不是同一次运行，不能直接横比。）

- s8 的在线数不能横比：那次假玩家站着不动、机器被压满，服务端只生成了很少区块
  （地形缓存 92.10 MB → ≈0，`Can't keep up` 刷屏），192.5 是「缓存几乎为空」的数。
- s11 在线 349.2 比 s10 的 338.6 高 10.6 MB，但归属表里两个目标项都降了（§2.3）。
  这是同口径（`ONLINE=60 AFTER=20`）下的一次读数，落在忙机器 ±10–20 MB 噪声里；
  **不能**当成回归，也不能把 −7 MB 当成进程级已兑现。s11 `plain` 线程数 32。
- 用户最初实测的 770 → 500 MB 是 **release 桌面 App 进程**（内嵌服务端，还含 WebView2 等
  进程份额），与裸服务端不同口径，不能直接相减；压缩比例以裸服务端为准。

### 2.2 离开后到底能回收多少（s9 `plain`，同一次运行）

| 时刻 | working_set | private_commit | 说明 |
|---|---:|---:|---|
| 在线（离开前 10 s） | 332.2 | 315.9 | 两份缓存已填满 |
| 离开后 20 s | 296.7 | 283.7 | 空闲释放还没触发 |
| 离开后 23 s（`settled`） | **104.2** | **90.1** | 默认 30 s 空闲释放清掉 192 MB 缓存 |
| `EmptyWorkingSet` 后 | **2.2** | **90.1** | 只是换出页面；提交没有归还 |

- 空闲释放由 `crates/mc-bamboo-game/src/worldgen_cache.rs:183` 的 `release_if_idle`
  每刻检查，`DEFAULT_IDLE_SECONDS = 30`。用户观察到「一离开就掉不下来」很可能就是读数早于
  这一次释放（也可能桌面 App 里 WebView 的份额压着）。
- `EmptyWorkingSet` 只改 RSS 曲线，**私有提交 90.1 MB 一分没少**，所以它不是「归还内存」，
  只是把页面换到页面文件、下次访问再缺页读回。
- 90.1 MB 里约 68 MB 是 image 段（`mc-bamboo-data` 的编译期静态表，§2.4），真正可归还的堆
  只剩约 20 MB。这推翻了更早「分配器还能省 ≈78 MB」的估计。
- s11 `plain` 的 `left` / `settled` / `trimmed` 是 105.4 / 105.4 / 2.1（private 90.7），
  和 s9 的 104.2 / 90.1 一致：**用户在「离开后」看到的地板仍然是约 90 MB 私有提交**，
  本轮没有改善它（也没打算改善，见 §3 #11）。

### 2.3 按归属的活跃堆（`left` 快照，MB）

两侧都是玩家离开后的快照；s10 那次两份缓存已释放，s11 的还没有（`AFTER=25`），
所以**只比逐项、不比合计**。

| 归属 | s10 `left` | **s11 `left`** | 变化 |
|---|---:|---:|---|
| worldgen per thread: template pools and structure state | 7.84 | **1.54** | **−6.30**（重写记忆） |
| worldgen per thread: generators (seeded noises, compiled samplers) | ≈1.16（s9 读法） | **0.00（1 块）** | **−1.16**（全进程一份） |
| worldgen data: shared Worldgen | 8.41 | **8.64** | +0.23（噪声） |
| worldgen: structure templates (parsed NBT) | 1.52 / 7.82 | **3.14** | 随结构缓存水位波动 |
| `climate::bucketize`（innermost，共享 R 树） | 1.66 | 1.66 | — |
| `mc_bamboo_core::mth::sin` | 0.25 | 0.25 | — |
| tracked live 合计 | 29.5（缓存已释放） | 129.6（缓存仍占 80.2 + 28.0） | 不可比 |

s11 `left` 的 innermost 里已经没有 `density::*` / `noise::*` / `spline` 那一组每线程编译
分配（s10 里分别是 3.06 / 1.53 / 0.41 MB）；剩下的「template pools and structure state」
1.54 MB 主体是 `Placer::try_placing_children`（0.39 MB）、结构起点 / 引用缓存等，
不再是编译好的密度函数。

`startup` 快照：s11 tracked live **11.9 MB**（`shared Worldgen` 8.64、结构模板外形 1.52、
其余 <0.5）。s10 的 `startup` 抓在出生点搜索结束前（含临时 JSON 根），不可比。

### 2.4 任务 1 点名的大头逐项对照

| 任务 1 项 | 当前实测 | 说明 |
|---|---|---|
| 解析后的 Worldgen（噪声、密度函数树、群系表、地物、结构模板） | 共享 `Worldgen` **8.64 MB 恒定**（含唯一一份地物注册表 + 模板池）+ 结构模板 1.52（空载）/ 3.1（离开）；群系桶 1.66 | 整份 `raw` 深拷曾是 24.93 MB |
| `mc-bamboo-data` 方块状态表与注册表 | 不占堆：image 段在线约 68 MB 提交 / 27 MB 常驻 | 全是 `&'static` 编译期静态表 |
| 战利品表 | 0.01 MB 以下 | 非大头 |
| 配方 | 未出现（≈0） | 非大头 |
| 命令树 | 0.27 MB | 小头 |
| 线程栈和缓冲 | 3.3 MB 提交 / 2.5 MB 常驻（s11，32–33 条） | 小头 |
| 常驻出生点区块 | `loaded chunks: blocks/biomes/heightmaps` 0.26（空载）/ 3.70（在线）/ 0.97（离开） | 随常驻票保留 |
| 每个已加载区块的光照数组 | 空载 0.02 / 在线 3.65 / 离开 0.07（s11） | 列级 COW 已实现 |
| 两份世界生成缓存 | 上限 128 + 64 = 192 MB 载荷，空闲 30 s 清空；s11 在线 77.6（地形）+ 29.9（地物） | 在线峰值的硬上限 |
| 分配器不还内存 | 离开后私有提交 90.7 MB，其中 image ≈68 MB | 可归还的堆只剩约 20 MB |
| 每线程编译的密度函数 / 噪声 | 归属表 **0.00 MB**（原 6 MB 量级） | 原型 9 |
| 每线程 `Generator` | 1 块（原 1.16 MB 量级） | 原型 9 |

### 2.5 优化前 → 九项原型后（总览）

| 大头 | 优化前（文档） | 九项原型后 | 变化 |
|---|---:|---:|---|
| 结构模板 NBT | 在线 ≈318 MB、离开后仍 ≈318 MB | 3.14 / 1.33 MB（有界缓存） | **≈ −300 MB 常驻**（原型 1） |
| 每线程地物注册表副本 | ≈25 MB 常驻 | 0 MB（并入 `Worldgen`） | **≈ −22 MB 常驻**（原型 3） |
| 每线程群系表副本 | 6.85 MB 常驻 | 1.71 MB | −5.14 MB（原型 4） |
| 每线程结构状态 / 模板池 | 14.91 MB 常驻 | 1.54–2.3 MB | −12 MB 级（原型 4 + 7 + 9） |
| 结构线程重建的群系 R 树 | 8.51 MB 归属 / 1.71 MB 树副本 | 1.66 / 0 | −2.34 MB（原型 8） |
| `Mth.sin` 65536 项表 | 5 份 × 256 KB | 1 份 0.25 MB | −1.0 MB（原型 8） |
| `Worldgen.raw` 整份 JSON | 24.93 MB 恒定 | 8.64 MB（含模板池与外形） | **−16.3 MB**（原型 5+7） |
| 重复全量解析 `worldgen.json` | 每条生成线程 4.4 MB × ~30 + 每次化石 ×2 | 0 | 原型 6 |
| 每线程编译的密度函数 | ≈6 MB 常驻 | 0 MB | **−6.3 MB**（原型 9） |
| 每线程 `Generator` | ≈1.16 MB 常驻 | 0.24 MB（全进程一份） | **−0.9 MB**（原型 9） |
| 地形 / 地物缓存 | 在线 ≈105 / ≈39 MB | 上限 128 + 64 MB，空闲清空 | 峰值相近，可配置 |
| plain working_set（代表值） | 在线 588 / 离开 505 MB | 在线 349 / 离开 20 s 105 | 见 §2.1–2.2 |

### 2.6 单个实例的分配实测（本轮新增，计数分配器）

`Worldgen::shared()` 之后取活字节差值，种子 20261121、`minecraft:overworld`：

| 对象 | 建成后 |
|---|---:|
| 一个 `SeededRouter`（改动前） | **1,464,588 B（≈1.43 MB）** |
| 同上，`release_rewrites()` 之后 | **149,886 B（≈146 KB）** |
| 一个 `chunk::Generator`（`with_settings`） | **242,766 B（≈237 KB）** |
| `Worldgen::shared_generator` 第一次 / 第二次 | 247,501 / **0 B** |
| 4 个 sealed `SeededRouter` | 604,216 B（≈151 KB/个） |

推算：默认 4 条计算线程时，改动前是 8 个 `Generator`（地形 4 + 结构 4）×237 KB ≈ 1.9 MB
加 4 个 `SeededRouter` ×1.43 MB ≈ 5.7 MB，合计 ≈7.6 MB；改动后是 1 个生成器 0.24 MB
加 4 × 0.15 MB，合计 ≈0.85 MB。**约 −6.8 MB**，和归属表 7.84 → 1.54 + 1.16 → 0 对得上。

### 2.7 缓存上限 A/B（最后一次接手新增，同机背靠背）

两次都是 `plain`、同一提交 `51a476a9`、种子 20261121、视距 8、`SETTLE=30 ONLINE=60
AFTER=20 TRIM=1`、端口 26341 / 26342，经 heavy 锁串行跑；两次之间只改 `bamboo.toml`
的两项缓存预算，没有改代码。

| 时刻 | 默认 128 + 64（`s12-cache-default`） | 减半 64 + 32（`s12-cache-halved`） | 差 |
|---|---:|---:|---:|
| 起服后空载 working_set / private | 38.1 / 22.3 | 38.0 / 22.3 | 0 |
| 一人在线 working_set / private | **371.6 / 355.1** | **313.7 / 296.7** | **−57.9 / −58.4** |
| 离开 20 s working_set / private | 111.0 / 109.5 | 111.1 / 104.7 | −0 / −4.8 |
| `EmptyWorkingSet` 后 working_set | 2.1 | 2.1 | 0 |

- **在线峰值差 −57.9 MB 是这次 A/B 里最可信的一项**：同一台机器、同一提交、同样的假玩家
  与时长、连续两次运行，只差两项预算。它比 §3.1 早前「303.0 对 332.2」的跨运行旁证可靠。
- 差值不到上限差（96 MB），说明这次在线时两份缓存**没有填到预算上限**：默认那次在线峰值
  比空载高 333.5 MB，减半那次高 275.7 MB，缓存只填了预算的一部分。真实收益取决于跑图强度，
  上限是 96 MB、这次实测是 57.9 MB。
- 两次「离开 20 s」都还没触发空闲清理（仍是 111 MB），所以这一栏不能拿来比「离开后能掉
  多少」；`settled` 与 `left` 相同，说明 60 秒在线结束时缓存刚被查询过，30 秒空闲窗口还没
  走完。`TRIM` 仍只是换页（private 不变）。
- 代价（重复生成、吞吐下降）这一轮**没有测**：60 秒里假玩家站着不动，没有跑图压力。
  这也是推荐 #1 仍留给产品决策的原因。

## 3. 排序后的压缩推荐

省多少 MB 是**估算**（基于归属表与分配实测），分「在线峰值」「常驻」「离开后」三种口径，
不能相加。工作量 / 风险是相对评级。**类别**按任务 3 区分「逐字节一致」与「结果略有不同」。

| # | 方案 | 省多少 | 口径 | 工作量 | 风险 | 类别 / 状态 |
|---|---|---|---|---|---|---|
| 1 | 调小两份世界生成缓存上限（`bamboo.toml`，已有键） | **同机 A/B 实测 57.9 MB**（128+64 → 64+32，上限 96 MB） | 在线峰值 | 小 | 低（代价是重复生成、吞吐下降） | 逐字节一致 |
| 2 | 进一步收紧结构模板有界缓存预算 | ≈4–6 MB | 常驻 / 在线 | 小 | 低 | **结果略有不同（只多花 CPU）** |
| 3 | 区块压缩帧不缓存 / 按订阅者共享 | ≈22 MB | 在线峰值 | 中 | 中 | 逐字节一致 |
| 4 | `Worldgen.raw` 只留运行时表 | 实测 13.43 MB | 常驻 | 小 | 低 | 逐字节一致 · **已落地（原型 5）** |
| 5 | 去掉重复全量解析（生成器 / 化石 / 死代码） | 启动峰值与时延下降 | 峰值 / CPU | 小 | 低 | 逐字节一致 · **已落地（原型 6）** |
| 6 | 模板池进程级常驻 + `Arc` 共享 | 实测 3.09 MB | 常驻 | 小 | 低 | 逐字节一致 · **已落地（原型 7）** |
| 7 | 共享结构状态 + 群系表 | 实测 ≈10.9 MB | 常驻 | 中 | 中 | 逐字节一致 · **已落地（原型 4）** |
| 8 | 全进程共享 feature registry | 实测 ≈22 MB | 常驻 | 中 | 中 | 逐字节一致 · **已落地（原型 3）** |
| 9 | 结构模板只留外形 + 有界缓存 | ≈300 MB | 常驻 / 在线 | 中 | 中 | **结果略有不同（只多花 CPU）· 已落地（原型 1）** |
| 10 | 每线程 `SeededRouter`：编译后丢掉重写记忆、`Generator` 进程级共享 | 分配实测 **≈6.8 MB**（1.43 → 0.15 MB/个 + 8 → 1 个生成器） | 常驻 | 小 | 低 | 逐字节一致 · **已落地（原型 9）** |
| 11 | 换分配器 / 主动归还空闲堆 | **≈10–15 MB**（私有提交 90.7 里 image 占 68） | 离开后 | 中 | 中高 | **结果略有不同（仅 RSS 曲线）** |
| 12 | 光照逐段 COW | 3–8 MB（在线 light 3.65 MB） | 在线峰值 | 中 | 高 | 逐字节一致 |
| 13 | 线程栈 / 缓冲区调小 | 1–3 MB | 常驻 | 小 | 低 | 逐字节一致 |
| 14 | 战利品表 / 配方 / 命令树 | <1 MB | 常驻 | — | — | 不值得动 |
| 15 | `Mth.sin` 表全进程一份 | 实测 1.0 MB | 常驻 | 小 | 低 | 逐字节一致 · **已落地（原型 8）** |
| 16 | 结构线程 fork `Worldgen` 的群系预置 | 实测 2.34 MB | 常驻 | 小 | 低 | 逐字节一致 · **已落地（原型 8）** |
| 17 | `SeededRouter` 采样器也算一份共享（只读程序 + 线程本地上下文） | ≈0.45 MB（4 × 146 KB → 1 份） | 常驻 | 中 | 中 | 逐字节一致 · 未做 |

### 3.1 「逐字节一致」类

- **#1 缓存上限（零代码，现在最划算）**：默认地形 128 MB + 地物 64 MB，空闲 30 s 清空。
  §2.7 的同机背靠背 A/B（同一提交、同参数，只改这两项）在线峰值
  **371.6 → 313.7 MB working_set（−57.9），private 355.1 → 296.7（−58.4）**；
  上限差是 96 MB，实际差 57.9 MB 说明那次缓存没填满。这是**运行时调优**，
  结果逐字节一致，代价是重复生成（本轮未压吞吐）。推荐把桌面 App 的默认值下沉，
  或按可用内存自动缩放（要改 `bamboo.toml` 默认值，属产品决策，本轮没动）。
- **#4–#8、#15、#16、#10 已落地**，见 §4。
- **#3 区块帧**：原型 2 已 `shrink_to_fit`；进一步按订阅共享、或不缓存整帧，会碰区块发送
  路径，改动需跑官方探针。
- **#12 光照**：`light/storage.rs:337-345` 已做列级 COW；在线 light 只有 3.65 MB，
  逐段 COW 的上限比早前估计小，收益几 MB、要动光照正确性。
- **#13 线程栈**：3.3 MB 提交 / 2.5 MB 常驻，收益小，且要按平台改线程创建参数。
- **#17 采样器共享**：`SeededRouter` 现在只剩约 146 KB/线程，4 条线程共享后省约 0.45 MB。
  要做就得在 `Worldgen` 里再缓存一份 `Arc<Samplers>`，`SeededRouter` 改成
  「`Arc<Samplers>` + 自己的 `Context`」；`compile` / `noise` 仍要 `Compiler`，
  接口要拆，收益是 0.45 MB，本轮判定不值得。

### 3.2 「结果略有不同」类

- **#11 分配器归还**：差异只在**进程 RSS / 工作集曲线**，地形、结构位置、存档、刷怪掉落、
  网络全部不变，玩家看不出。上限 ≈10–15 MB：离开后私有提交 90.7 MB，其中 image 68 MB，
  堆只剩约 20 MB。Windows 上 `HeapCompact` 前轮实测只回收约 5 MB；外部 `EmptyWorkingSet`
  只换出页面（90.7 MB 提交不变）。要再拿这 10–15 MB 得换 jemalloc / mimalloc（本机 cargo
  缓存里没有这些 crate，需要联网拉依赖；全局分配器要落在 `crates/mc-bamboo/src/main.rs`
  或一个被所有二进制链接的库上，后者会和 memprof 自己的记账分配器冲突），属全局性改动，
  按架构规矩停下留队长决策。
- **#2 结构模板有界缓存（已落地）**：攒满后整表清空、下次重解析；官方
  `StructureTemplateManager.getOrCreate` 的 `structureRepository` 从不清空。**多花 CPU，
  结果逐字节不变，玩家看不出**。
- **#9 原型 1** 同上，只是缓存的是整份模板。
- **#1 缓存上限**：结果逐字节一致，但吞吐会掉；用户能感觉到的是区块迟到，不是世界不同。

**九项原型都没有改动世界生成结果**：用户允许的「结果略有不同」有硬约束（同种子地形
与结构位置不能变、存档协议兼容、刷怪掉落概率不变），而收益足够大的几项差异都不落在
游戏内容上。

## 4. 已落地的九项原型

1. **结构模板只留外形 + 有界按需解析**（`b4f05aea`，
   `crates/mc-bamboo-worldgen/src/structure/template.rs`）：全进程一张 `SHAPES` 表，只含
   「尺寸 + 拼图方块」；真正摆方块时才解析整份模板进线程本地缓存，攒到
   `CACHE_BLOCK_BUDGET = 400_000` 就整表清空。
2. **缓存压缩区块帧 `shrink_to_fit`**（`e31d860f`，
   `crates/mc-bamboo-game/src/outgoing.rs:89`）。
3. **全进程共享 feature registry**（`aa3c4994`，`worldgen/src/lib.rs` + `chunk.rs`）：
   实测该归属 25.14 → 0 MB，净常驻 ≈ −22 MB。
4. **结构状态与群系表全进程共享**（`53b3adca`，
   `crates/mc-bamboo-game/src/worldgen_structures.rs`）：按 `(seed, settings, flat)` 缓存
   一份 `Arc<WorldStructureState>` + `Arc<MultiNoiseBiomeSource>`，实测 −10.9 MB。
5. **`Worldgen.raw` 只留启服后还会读的注册表**（`c361c4ce`，现在 9 张）：
   实测 `shared Worldgen` 24.93 → 11.50 MB（−13.43）。
6. **去掉重复全量解析**（`72888506`）：
   - `crates/mc-bamboo-worldgen/src/chunk.rs:1088` `parse_settings`：原先
     `serde_json::from_str(WORLDGEN)` 整份重解析，现在取
     `worldgen.raw()["noise_settings"][settings_id]`。每条生成线程建 `Generator` 时省一次
     4.4 MB 解析（~30 条线程 → 启动时省 ~30 次）。
   - `crates/mc-bamboo-worldgen/src/feature/fossil.rs:151` `chain`：原先每次放化石解析两遍
     整份文档，现在取 `Worldgen::shared().raw()["processor_list"]`。
   - 删除 `crates/mc-bamboo-worldgen/src/feature/noise_state.rs` 的 `holder_registry()`
     （全仓无调用者；`Registry::parse` 已持有同一张 `block_state_provider` 表）。
   - 新增 `chunk::tests::settings_match_a_fresh_full_parse`（`chunk.rs:1181`）。
   - 数据同源，**结果逐字节不变**。
7. **模板池进程级常驻**（`ba0f24b8`）：
   - `crates/mc-bamboo-worldgen/src/lib.rs:235`：`Worldgen::parse` 里
     `Arc::new(TemplatePools::parse(...))`；`lib.rs:308` `template_pools()`。
   - `crates/mc-bamboo-worldgen/src/structure/pipeline.rs:217` `pools: Arc<TemplatePools>`。
   - `RUNTIME_TABLES`（`lib.rs:84`）从 10 张收到 9 张。
   - 实测 `shared Worldgen` 11.50 → 8.41 MB（−3.09）。
8. **`Mth.sin` 表去重 + 结构线程 fork 群系预置**（`fe65bf37`）：
   - `mc_bamboo_core::mth` 的 65536 项表成为全进程唯一一份。
   - `climate.rs` 的 `ParameterList` 把 `points` / `values` / `root` 放进 `Arc`，新增
     `ParameterList::fork()`；`biome.rs` 加 `MultiNoiseBiomeSource::fork()`。
   - `worldgen_structures.rs:207` 普通世界不再重建 `MultiNoiseBiomeSource`，改成 `.fork()`。
   - 实测群系 R 树副本 1.71 → 0、`climate::bucketize` 3.32 → 1.66、sin 表 5 份 → 1 份。
9. **一份编译好的生成器 + 丢掉路由的重写记忆**（`25bdf76e`，本会话）：
   - `crates/mc-bamboo-worldgen/src/sampler.rs:1798` `Compiler::release_rewrites()`：
     清 `rewritten` / `keep_alive` / `caches` 并 `shrink_to_fit`。
   - `crates/mc-bamboo-worldgen/src/settings.rs:230`：`SeededRouter::new` 编译完八个路由
     采样器后立刻调用。实测 1,464,588 → 149,886 B。
   - `crates/mc-bamboo-worldgen/src/lib.rs:136` 新增
     `generators: Mutex<HashMap<(String, i64), Arc<chunk::Generator<'static>>>>`；
     `lib.rs:335` `Worldgen::shared_generator(settings_id, seed)`，第一次调用时编译并缓存。
   - `crates/mc-bamboo-game/src/worldgen.rs:100`：线程本地缓存改为 `Arc<Generator>`，
     `:128` 走 `shared_generator`。
   - `crates/mc-bamboo-game/src/worldgen_structures.rs:84`：`StructureThread.generator`
     改为 `Arc`；`:290` 平坦世界仍建线程本地一份并 `set_flat_columns`，否则用共享那份。
   - `crates/mc-bamboo-worldgen/src/material.rs:289`：陶土色带随机源从 `System` 字段挪到
     `prepare` 的局部变量（`System` 里的 `Random` 带 `Cell`，不 `Sync`，会挡住共享）。
   - 实测 `left`：「template pools and structure state」7.84 → 1.54 MB、「generators」
     1.16 → 0.00 MB；新增单测见 §9。
   - **结果逐字节不变**：三个对拍全 0 不一致。

## 5. 疑点清单

每条按「文件行号 / 实现做了什么 / 官方怎么做 / 为什么不确定」列，供队长逐条核实。

### 5.1 本会话（原型 9）新增 / 改动处

32. `crates/mc-bamboo-worldgen/src/settings.rs:230` 与
    `crates/mc-bamboo-worldgen/src/sampler.rs:1798`：`SeededRouter::new` 编译完八个路由
    采样器就清掉 `Compiler::rewritten`、`keep_alive`、`caches`。官方 `RandomState` 没有
    这套重写记忆（`DfRewriteRule` 在编译期间用，编译完就不需要），所以方向一致。
    不确定：清 `caches` 之后，如果调用方再 `SeededRouter::compile` 同一个 `cache` 节点，
    会拿到**新的** cache 编号（`next_cache_id` 继续递增）。采样器本身仍是同一个函数，
    编号只要在同一个 `Context` 里自洽就不影响值——`SeededRouter` 的上下文是
    `EMPTY_UNCACHED`，不落格子；工具 `mc-bamboo-vanilla` 的 `compare_function` 用
    `sample_with` 也走这个上下文，单测 `compile_after_releasing_rewrites_matches_a_fresh_compiler`
    逐位比对过。没有验证的情形：先 `compile` 一个含 `cache` 的函数、seal、再 `compile`
    另一个共用同一 `cache` 节点的函数，然后用**开启缓存**的上下文求值（目前没有这种调用方）。
33. `crates/mc-bamboo-worldgen/src/lib.rs:335-355`：`shared_generator` 在持有
    `generators` 互斥锁的期间完成编译。官方 `ChunkMap` 的 `RandomState` 由世界创建时建好，
    所有工作线程直接读。不确定：①锁在编译期间被持有（毫秒级、只在第一次），如果
    `Generator::with_settings` 里将来有代码反过来锁 `Worldgen::generators` 会死锁；目前
    没有这种路径。②缓存键是 `(settings_id 字符串, seed)`，不是 `Settings` 的地址或内容；
    如果将来同一进程里出现两份数据不同但 ID 相同的 `Worldgen`，会共用错生成器——目前
    只有 `Worldgen::shared()` 一个实例，且 `shared_generator` 要求 `&'static self`。
    ③缓存永不回收，按种子 / 设置累积（和结构状态缓存同一取舍）。
34. `crates/mc-bamboo-game/src/worldgen.rs:100`、
    `crates/mc-bamboo-game/src/worldgen_structures.rs:84`：`Generator` 从「每线程一份」
    变成「全进程一份 `Arc`」，被世界线程、计算池线程和结构线程同时以 `&self` 使用。
    `Generator` 的所有方法都是 `&self`（只有 `set_flat_columns` 是 `&mut self`，现在只在
    建平坦世界那份时用），编译期已由 `Worldgen: Sync` 保证没有 `Cell` / `RefCell`。
    不确定：**可变的线程本地状态只剩群系 R 树 `lastResult`**，它按「线程 + 表」存在
    thread-local（`climate::ParameterList` 的编号索引），所以共用同一棵树不共享槽；但这依赖
    **每个入口都先 `reset_biome_state`**（`worldgen.rs:141`、`worldgen_structures.rs:539`），
    没有编译期约束。改动前每线程一份对象时，漏掉 reset 只影响本线程上一轮的历史；
    现在同一线程仍只影响本线程（槽还是线程本地的），风险没有变大，但「对象是共享的」
    容易让人误以为可以省掉 reset。
35. `crates/mc-bamboo-worldgen/src/material.rs:273-291`：`System` 不再持有
    `bands_random`，`prepare` 里用 `positional.from_hash_of("minecraft:clay_bands")` 现造
    一个局部 `Random` 交给 `generate_bands`。官方 `MaterialSystem` 在构造时用
    `randomState.random.fromHashOf("minecraft:clay_bands")` 造色带，之后不再用这个随机源。
    不确定：各 `Random` 实例由 positional factory 独立派生，造的先后不影响取数（矿脉、
    条件噪声、九条内置噪声各用各的 `from_hash_of`）；但这是「取数顺序」敏感的代码区，
    已用 `chunkgen --through features` 覆盖地表规则（含恶地色带），没有单独断言色带数组。
36. 测量口径：s11 `plain` 在线 349.2 MB 比 s10 的 338.6 高 10.6 MB，而归属表里两个目标项
    合计降了约 7.5 MB。两次是同参数（`SETTLE` 不同：s10 的 `plain` 没记录，s11 是 30）、
    不同运行，机器负载不同。不确定：这 10 MB 是噪声还是共享对象让 `Context` / 缓存水位
    变了（共享后 `Generator` 只有一份，`MaterialRules` 的 `clay_bands` 等不再重复，理论上
    只会更小）。结论以分配实测（§2.6）和归属表（§2.3）为准，进程级读数只当旁证。
37. 测量口径：s11 `plain` 的 `left` 与 `settled` 完全相同（105.4 / 90.7），说明两份缓存在
    离开 20 s 时已经释放；而 s11 `profile` 的 `left`（`AFTER=25`）里缓存还在（地形 80.2 +
    地物 28.0）。同一个 `release_if_idle` 在两个模式下触发时刻不同，原因和 §5.3 第 31 条
    相同（profile 的记账开销让区块任务一直在途），所以 **profile 的 `left` 只能比归属常量，
    不能判断「离开后能掉多少」**。
38. 测量口径（§2.7 的 A/B）：两次都只跑 60 秒、假玩家站着不动，**没有跑图压力**，因此
    「减半后吞吐掉多少」没有测；在线峰值差 −57.9 MB 只说明这两种预算下的驻留量差，不能
    当成「用户一定少 58 MB」——用户实际跑图越狠、缓存填得越满，省得越多（上限 96 MB）。
    另外两次运行相隔几分钟，机器负载不完全相同（32 线程不变，空载读数相同 38.0/38.1），
    但 −57.9 MB 远大于忙机器常见的 ±10–20 MB 噪声，方向可信。

### 5.2 前几轮新增 / 改动处（仍有效）

1. `crates/mc-bamboo-worldgen/src/chunk.rs:1088`：`parse_settings` 改读
   `Worldgen::raw()`。官方 `NoiseGeneratorSettings` 在 `RandomState.create` 时只编一次；
   这里方向一致，但依赖「`raw` 一定含 `noise_settings`」——`Worldgen::parse` 保证（不在名单
   就报 `missing worldgen table noise_settings`），且新增单测逐项比对。不确定：将来若有人再
   从 `RUNTIME_TABLES` 删 `noise_settings`，生成器会直接报错（不是静默错世界），但编译期
   没有绑定。
2. `crates/mc-bamboo-worldgen/src/feature/fossil.rs:151`：`chain` 改用
   `Worldgen::shared()`。官方 `FossilFeature` 通过 `StructureTemplateManager` / 注册表引用取
   processor 链，不存在「每次放置重新解析文档」。不确定：`chain` 现在总是读**进程内嵌的**
   那份 `Worldgen`；如果将来支持运行时换数据包、而某个调用方用非共享的 `Worldgen` 建化石，
   这里会和调用方的数据不一致（改动前同样是解析内嵌数据，行为未变差）。
3. `crates/mc-bamboo-worldgen/src/feature/noise_state.rs`：删除 `holder_registry()`。
   全仓 `grep holder_registry` 只有定义处；它是 `pub`，属 API 删除。不确定：外部 crate /
   工具将来是否会想要这张表；`Registry::provider(name)` 仍可用。
4. `crates/mc-bamboo-worldgen/src/lib.rs:235`：`Worldgen::parse` 现在**立即**解析模板池。
   官方 `StructureTemplatePool` 按注册表在数据加载时构造，`maxSize` 懒算。Bamboo 的
   `TemplatePool::parse` 提前算 `max_size`（`pool.rs` 字段注释解释：为了保持 `Sync`），因此
   会提前加载模板外形：空服 tracked live +1.52 MB。不确定：这算不算「启动变慢」不可接受；
   有玩家时外形本来就要加载，常驻净减 3.09 MB。
5. `crates/mc-bamboo-worldgen/src/structure/pipeline.rs:217`：`WorldStructureState.pools`
   从 `TemplatePools` 变成 `Arc<TemplatePools>`，跨种子 / 跨线程共享。官方每世界一份
   `ChunkGeneratorStructureState`。不确定：模板池构造后只读，目前没有 `&mut self` 方法；将来
   若给 `TemplatePools` 加可变状态，会同时影响所有世界，且没有编译期约束。
6. `crates/mc-bamboo-worldgen/src/lib.rs:84`：`RUNTIME_TABLES` 现在 9 张。官方没有「运行时表」
   概念（`RegistryAccess` 一直持有全部注册表）。不确定：这份名单靠人工核对读点 + 单测
   `raw_keeps_only_the_runtime_tables` 盯键集合，没有编译期绑定。
7. `crates/mc-bamboo-worldgen/src/lib.rs:469`：单测断言 `template_pools().len() == 245`。
   不确定：官方数据升级后池数变化会红；这是有意的「数据变了要人看一眼」，不是行为断言。
8. `crates/mc-bamboo-worldgen/src/lib.rs:235`：`TemplatePools::parse` 内部会
   `compute_max_size` → `element.bounding_box` → `StructureTemplate::load_cached`，即启服时
   会读结构模板 NBT。不确定：若模板数据缺失，现在启服就失败（原先首个区块生成时失败）；
   错误更早更明确，但改变了失败时机。
9. `crates/mc-bamboo-worldgen/src/climate.rs:176-184`：`ParameterList` 的 `points` / `values` /
   `root` 从 `Vec` / `Node` 改成 `Arc<[T]>` / `Arc<Node>`，新增 `fork()`。官方
   `Climate.ParameterList` 每份实例都自己持有数据和 `ThreadLocal lastResult`。不确定：`Arc`
   让「一份表被两个世界共享」变成合法状态；目前没有 `&mut self` 方法、也没有
   `Arc::get_mut`，所以没有可见风险，但这是新的共享面。
10. `crates/mc-bamboo-worldgen/src/climate.rs:239`：`fork()` 共享 `points` / `values` / `root`，
    只换 `id`。`id` 只用于索引线程本地的 `LAST_RESULTS`，进程内单调递增、永不复用。
    不确定：`LAST_RESULTS` 向量按最大 `id` 增长，每次 fork 多一格 `Option<usize>`（8 字节），
    可忽略；但如果将来有大量一次性 `ParameterList`，这张 thread-local 向量会跟着长。
11. `crates/mc-bamboo-game/src/worldgen_structures.rs:207-222`：普通世界改成
    `worldgen.preset("minecraft:overworld")?.fork()`；平坦世界仍是单点表。官方
    `MultiNoiseBiomeSource` 每世界一份。不确定：结构线程和地形生成器各持**同树不同槽**的
    两份表（现在 `Generator` 都共享了，这一条指 `SeededRouter` 与 `Generator` 之间）；
    树内容完全一致，只要所有入口都先 `reset` 就没有差别，**这靠调用点保证，没有编译期约束**。
12. `crates/mc-bamboo-loot/Cargo.toml`：新增 `mc-bamboo-core` 依赖，只为 `number.rs` 的
    `mth_sin` / `mth_cos`。方向仍是 loot → core。不确定：这是本 crate 第一次依赖 `core`；
    如果团队更希望 loot 自带表，可以撤回（代价 +256 KB）。
13. `crates/mc-bamboo-worldgen/src/carver.rs:31`、`feature/ores.rs:18`、
    `feature/dripstone_utils.rs:99`、`crates/mc-bamboo-loot/src/number.rs:596`：删除本地
    `Mth.sin` 表，改为调用 `mc_bamboo_core::mth`。原建表公式与 core 同源，
    `mc-bamboo-core` 的单测 `sin_table_matches_the_official_one` 把 65536 个位模式和官方
    FNV-1a 比对过。不确定：没有逐 crate 做「改动前后每个下标的值」的比对，靠 `chunkgen`
    与 loot 单测覆盖。
14. 测量归属：`tools/mc-bamboo-memprof/src/owners.rs` 的规则是「调用栈里出现即命中，
    先到先得」。`SeededRouter::new` 的分配栈里带 `worldgen_structures::with_thread`，所以
    每线程编译的密度函数曾被算进「template pools and structure state」，innermost 里显示成
    `density::impl$52::fmt` / `noise::impl$42::fmt`（**符号名是 `fmt`，实际是
    `Compiler::compile_node` 里的分配**，PDB 最近符号）。
15. `s10-profile` / `s10-profile20` 的 `startup` 快照抓在 `World::build` 还没结束时
    （`spawn landing timed out`，`shared Worldgen` 27.2 MB 含临时 JSON 根），**不能**和
    s9 的 `startup` 8.41 MB 比。`run.sh` 的 `SETTLE` 默认值对 profile 模式偏短；
    s11 用 `SETTLE=60` 拿到了完整快照（11.9 MB）。
16. s10-plain 用 `ONLINE=60 AFTER=20`（s9 是 90 / 20），且 `left` 时缓存已释放，所以只能和
    s9 逐项比常数，不能比总量。
17. s9-profile 的 `left`（假玩家离开 120 s 后）里两份世界生成缓存**仍然占着**，而同一天
    `plain` 模式离开约 23 s 就释放了。不确定：profile 模式的记账开销让区块任务一直在途，
    `release_if_idle` 的「空闲」条件达不到；**profile 模式的 `left` 不能用来判断「离开后
    能掉多少」**，只能比对归属常量。
18. 有界模板缓存**故意和官方不同**：官方 `StructureTemplateManager.getOrCreate` 的
    `structureRepository` 从不清空，Bamboo 攒到 `CACHE_BLOCK_BUDGET` 就整表清空、下次重新
    解析；结果逐字节不变，只多花 CPU。`SHAPES` 用一把全进程 `Mutex`，争用对吞吐的影响未测。
19. `crates/mc-bamboo-game/src/outgoing.rs:89`：压完 `bytes.shrink_to_fit()`。
    `shrink_to_fit` 可能触发 realloc，在系统分配器不归还的前提下是否增加碎片未测；帧内容
    逐字节不变。
20. `tools/mc-bamboo-memprof/src/os.rs`：`HeapCompact` / `VirtualQuery` / PDB 符号解析只在
    Windows 实现；非 Windows 走空实现。
21. `crates/mc-bamboo-world/src/light/storage.rs:337-345`：未加载列返回 `EMPTY`、写入时
    `Box::new(template.clone())`，已做列级 COW；进一步逐段 COW 的收益未实测。
22. `crates/mc-bamboo-data/src/lib.rs`：方块状态表 / 注册表是 `&'static` 编译期表，落在
    image 段（68 MB 提交 / 27 MB 常驻），不占堆。官方 `Block.BLOCK_STATE_REGISTRY`
    是运行时注册。
23. 官方探针 `ClientboundSoundPacket` / `SoundEvent` 的到达判定历史上偶发抖动
    （`768/769` 与 `769/769` 交替）；本轮一次干净 `769/769`。
24. `W:/data/bamboo-agents/reports/op-memprof-data/s9-plain2/`：`left`（离开 20 s，296.7 MB）
    与 `settled`（+3 s，104.2 MB）差 192.5 MB，正好等于两份缓存预算；但**空闲释放的触发
    时刻**取决于最后一次查询时间，`AFTER` 的 20 s 并不保证读到「释放前」或「释放后」。
25. `s9-plain-halved/`：把缓存上限减半（64 / 32）只跑了一次，在线 303.0 MB，默认上限那次
    332.2 MB。两次是不同运行、机器负载不同，**不能当成 −29 MB 的净收益**；硬上限才是可信的。
26. `TRIM` 实验（`tools/mc-bamboo-memprof/run.sh`）用外部 PowerShell 调 `EmptyWorkingSet`，
    只改工作集、不改提交；`trimmed` 的 2.1–2.2 MB 是「页面全被换出」的瞬时值，进程一有
    活动就会缺页读回。不确定：不同 Windows 版本 / 内存压力下回收比例不同。
27. `tools/mc-bamboo-memprof/src/tracker.rs:38,112-157`：站点表 `SLOTS = 1<<18`、用到 3/4 后
    新调用栈全记进 0 号「溢出」分配点。本轮 20600–21300 sites，未触发。
28. 优化前 318 MB 模板 / 588 → 260 MB 等数字来自更早一轮实测（docs §20 / README），代码
    已在优化后状态，**无法复现优化前**，只能引用文档。
29. `crates/mc-bamboo-game/src/worldgen_structures.rs:167-262`：共享
    `WorldStructureState` / `MultiNoiseBiomeSource`；全局缓存按 `(seed, settings, flat)` 累积
    不回收。官方每世界一份、世界卸载后释放；Bamboo 没有世界卸载路径，这一点和官方不同。
30. `crates/mc-bamboo-worldgen/src/lib.rs:84`：`RUNTIME_TABLES` 名单靠人工维护（同 §5.2 第 6 条）。
31. `tools/mc-bamboo-vanilla/src/worldgen.rs:324`：工具里 `router.compile` + `sample_with`
    走的是 `SeededRouter` 自带的 `EMPTY_UNCACHED` 上下文，和改动前一致；seal 之后 `compile`
    仍然可用（单测覆盖），但工具的 router 缓存里那些实例每次都是新建的，没受影响。

## 6. 已知限制（与 README / docs 现状一致）

- 有界模板缓存**故意和官方不同**：官方 `structureRepository` 从不清空，Bamboo 攒到
  `CACHE_BLOCK_BUDGET` 就整表清空、下次重新解析；结果逐字节不变，只多花 CPU。
- 全进程 `SHAPES` 外形表用一把 `Mutex`，多生成线程同时查外形会短暂争用，未测吞吐影响。
- 结构状态 / 群系表共享缓存按 `(seed, settings, flat)` 常驻不回收；进程内换种子 / 平坦
  设置会累积多份。
- 编译好的 `Generator` 按 `(settings_id, seed)` 常驻不回收，同样按种子 / 设置累积；
  只有 `Worldgen::shared()` 那一份能给出 `'static` 生命周期。
- 平坦世界的 `Generator` 仍按线程各建一份（要先 `set_flat_columns`），不进程级共享。
- `SeededRouter` 仍是每线程一份，但编译完只剩约 146 KB 采样器（重写记忆已丢）。
- 共享 `Generator` 的群系 R 树 `lastResult` 按「线程 + 表」隔离，结果一致靠每个入口先
  `reset`，**没有编译期约束**。
- `Worldgen::raw()` 只返回启服后还会读的 **9 张**顶层表，**不再是完整文档**；模板池改由
  `Worldgen::template_pools()` 提供。新增读点必须同步 `RUNTIME_TABLES`。
- `TemplatePools::parse` 在启服时就预计算 `max_size`、提前加载模板外形：空服比更早版本多
  约 1.5 MB，有玩家时本来就会加载。
- 世界生成缓存默认 128 MB 地形 + 64 MB 地物，空闲 30 s 清空；这是在线峰值的硬上限，也是
  用户看到的「离开后掉不下/掉得慢」的主要来源。
- 离开后私有提交地板约 90 MB，其中 image ≈68 MB；`HeapCompact` 与外部 `EmptyWorkingSet`
  都拿不回来。**没有换分配器**。
- 堆分析器自身每块多一个 ≥8 字节头、另占 76 MB 站点表，进程级读数偏大；**真实读数一律以
  不带分析器的 `bamboo`（plain）为准**，profile 只看比例和归属。
- 归属规则是按函数名匹配的启发式；只在 Windows 上有 PDB / `HeapCompact` 实现。

## 7. 没做的和故意排除的内容，及原因

- **没做换分配器 / 主动归还原型**：本机 cargo 缓存里没有 mimalloc / jemalloc；全局分配器必须
  落在 `crates/mc-bamboo/src/main.rs`（禁区）或一个被所有二进制链接的库（会和 memprof 的
  记账分配器冲突）。且实测上限 ≈10–15 MB（提交 90.7 MB 里 image 占 68），属全局性改动，
  按架构规矩留给队长决策。
- **没改两份缓存的默认上限**：已有 `bamboo.toml` 键，属产品 / 部署决策（牺牲吞吐换内存），
  本轮只给实测数据与推荐。
- **没做 `SeededRouter` 采样器共享（#17）**：只剩约 146 KB/线程、合计 0.45 MB，要拆
  `compile` / `noise` 的接口，收益不值。
- **没做光照逐段 COW（#12）**：在线 light 3.65 MB，收益上限比早前估计小，且要动光照正确性。
- **没调线程栈（#13）**：3.3 MB 提交 / 2.5 MB 常驻，要按平台改线程创建参数。
- **没再收紧 `raw`**：剩下的 `density_function`（紧凑 0.11 MB）等由 `Generator` 编译含水层 /
  地表规则时按维度读，改成编译产物要动 `MaterialRules::compile` / aquifer，收益不到 1 MB。
- **没做密度函数节点合并**：会改变求值顺序与随机源取数，用户硬约束「同种子地形不能变」。
- **没跑全量方块 oracle**：本会话 2 个提交没有改方块行为（只换编译结果的存放方式与重用），
  任务规定「改到方块行为的话」才需要。改了世界生成的取数路径，所以按关键路径跑了
  `chunkgen --through features` 与两个结构对拍（见 §9）。
- **没跑 workspace 全量 `cargo test --no-fail-fast`**：工作区 cargo shim 直接禁止（退出码 2），
  按 2026-11-20 验证范围不绕过，改跑改动 crate 的 `-p` 测试，全量留队长合并时跑。
- **没跑完整 `cargo test -p mc-bamboo-game`**（1270 tests、前轮 949 s）：超出前台时限；改动面
  只把 `generator` 换成 `Arc`，用 `cargo check -p mc-bamboo-game --all-targets` 与 clippy
  全目标确认调用点仍编译，并跑了 `worldgen_structures::` 的 10 个测试。
- 没推送、没动别的分支、没碰 `W:\data\GithubProjects\Bamboo` 主目录、没碰
  `crates/mc-bamboo/src/main.rs`、没启动真实 Minecraft 服务端 / 客户端、没接受 EULA、没 kill
  别的进程、没改任何进程优先级。所有程序后台静默启动；端口 26221–26223 无 LISTENING；
  `locks/heavy` 已释放。

## 8. 产物与复现

- 产物 `W:/data/bamboo-agents/reports/op-memprof-data/`：
  - `s12-cache-default/`、`s12-cache-halved/`：最后一次接手的同机缓存上限 A/B
    （`plain`，`SETTLE=30 ONLINE=60 AFTER=20 TRIM=1`，端口 26341 / 26342）。
  - `s11-profile/`：原型 9 后的 `startup` / `online` / `left` 归属表与 `control/`
    （`SETTLE=60 ONLINE=45 AFTER=25`）。
  - `s11-plain/`：原型 9 后不带分析器的读数（`SETTLE=30 ONLINE=60 AFTER=20 TRIM=1`）。
  - `s10-profile/`、`s10-profile20/`、`s10-plain/`：原型 8 后的基线。
  - `s9-*`、`s8-*`、`s7-*`、`s6-*`、`s5-*`、`s4-*`、`s3-*` 与 `checks-*` / `recheck-*` 均保留。
- 复现：`CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --release -p mc-bamboo-memprof
  -p mc-bamboo-loadtest -p mc-bamboo`，再
  `node W:/data/bamboo-agents/tools/heavy.js bash tools/mc-bamboo-memprof/run.sh <结果目录>
  <端口> profile`（或 `plain`，加 `TRIM=1` 跑换出实验）。种子 20261121、视距 8。
  A/B 的减半那次加 `BAMBOO_TOML=$'worldgen_terrain_cache_mb = 64\nworldgen_feature_cache_mb = 32'`。
  对拍显式 `JAVA=/c/Users/Administrator/.jdks/openjdk-26.0.1/bin/java`；探针要设
  `JAVA_HOME` 到同一 JDK 26（PATH 里的 JDK 17 编不了官方 oracle）。
- 工具说明见 `docs/profiling.md` §20 / §20.1 / §20.2 / §20.3 / §20.4 / §20.5 / **§20.6**。

## 9. 验收检查实际结果（在最终提交 `51a476a9` 上实跑）

机器是忙机器，所有重活串行：内存实测与探针经 `node W:/data/bamboo-agents/tools/heavy.js`
排队独占机器级锁；cargo 构建 / 测试由工作区 cargo shim 与目标目录锁串行；同一时刻最多一个
java 对拍进程。`CARGO_BUILD_JOBS` 未改、未改优先级、未弹窗口。下表是**最后一次接手**
在同一提交上重新跑出来的结果（不是上一轮的数字）。

| 检查 | 结果 | 实际数字 |
|---|---|---|
| `cargo fmt --check` | 通过 | `FMT_EXIT=0` |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 | `CLIPPY_EXIT=0`，`Finished dev profile ... 8.35s`，0 warnings |
| `cargo test --no-fail-fast` | **被 shim 禁止** | `WORKSPACE_TEST_EXIT=2`，原文 `[bamboo] workspace-wide cargo test is disabled for agents: ... The lead runs the merge checks.` |
| `cargo test -p mc-bamboo-worldgen --lib` | 通过 | **358 passed; 0 failed; 4 ignored**（79.11s） |
| `cargo test -p mc-bamboo-game --lib -- worldgen_structures::` | 通过 | **10 passed; 0 failed**（141.24s，1261 filtered out），含 `threads_share_one_generator` |
| `cargo test -p mc-bamboo-vanilla` | 通过 | main `101 passed`（6.15s）+ `block_entity_resave` `8 passed`（0.69s）= **109 passed / 0 failed** |
| `cargo check -p mc-bamboo-desktop` | 通过 | `DESKTOP_EXIT=0`（1.62s） |
| 官方探针 `tools/probe.sh`（经 heavy 锁，JDK 26） | 通过 | 端口 26343：`PROBE RESULT 769/769 checks passed`；`shutdown: fresh server exited normally`；`level.dat` / 玩家数据 6 个 / 区域文件 4 个都在；`WORLD-DIR RESULT: official codecs read every item; broken-file self-check passed` |
| `chunkgen --through features --seeds 1 --chunks 4` | 通过 | 4 chunks / 6144 群系格 / 3545088 项，**0 mismatches** |
| `structure-rings` | 上一轮同提交通过，本轮未重跑 | 6 seeds / 768 环位 / 96 群系吸附 / 864 项，0 mismatches |
| `structures` | 上一轮同提交通过，本轮未重跑 | 20 套 / 464 查询 / 9280 放置 + 9744 选择，0 mismatches |
| 全量方块 oracle 两个新种子各 30 万题 | **未跑** | 本轮无代码提交、未改方块行为 |

收尾核实：`git status` 干净、`HEAD=51a476a9`、`git rev-list --count e83ce946..HEAD` = 23；
端口 26341–26343 无 LISTENING；本轮起的 `bamboo` / `bamboo-memprof` / `bamboo-loadtest`
进程都已退出，`locks/heavy` 已释放。探针与对拍期间出现过 `[heavy-lock] waiting`，按规则
等待，没有 kill 任何持有锁的任务。
