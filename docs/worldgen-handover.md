# 主世界生成交接（2026-11-15）/ Overworld generation handover

本轮目标：主世界地形、地物、结构与原版 26.3 逐方块一致，全部由 Java oracle 对拍验证。生成期刷怪不在本轮范围。

Goal of this round: overworld terrain, features and structures identical to vanilla 26.3 block for block, every piece verified against a Java oracle. Generation-time mob spawning is out of scope.

## 已合进 main / Merged on main

都已通过全套合并检查：fmt、clippy、全量测试、全部对拍 0 不一致、联机探针 719/719。

All passed the full merge check: fmt, clippy, full tests, every oracle at 0 mismatches, network probe 719/719.

| 内容 | 合并提交 |
|---|---|
| 地形（noise、aquifer、surface、carver，含 paths 修正） | 753d877 及之前 |
| 树引擎（全部树形、巨型蘑菇、mangrove、poplar），`FeatureWorld::finish` 按格取最后一次写入 | 7373539 |
| 地物集成（水生、洞穴、地物 A、结构布局） | 1c9853c6 |
| 废弃矿井、要塞放置 | 2ba852ee |
| 雪屋、沉船、海底遗迹、废弃传送门放置 | 780a291f |
| 洞穴与冰雪类地物（9 种） | 32c19dde |
| 地物 A（12 种非植被） | 5c7e9995 |
| 海底神殿、林地府邸放置 | e4ed2f50 |
| 生成期光照直接读生成方块（光照阶段 35.1 → 11.3 ms/区块），`wg-perf` 计时工具（`perf` feature，默认关） | 9fb4ee35 |
| 地物 B（植被、花、竹子、海草、藤蔓等） | cd05c7e5 |
| 水生与洞穴植被，整步对拍 95 中心 0 不一致 | 8fd828bb |
| jigsaw 放置进展（模板 processor、`FeatureWorld` 方块实体与流体） | dcc8784d |

**已接进 Director（本轮）/ Wired into the Director (this round):** 结构起点与引用走
`structure_starts` / `structure_references` 两个阶段的纯函数，features 每个
decoration step 先放结构件再放地物，区块 NBT 写回 `structures` 键并能读盘恢复。
`cb-wg-structures` 合入 main 后，四个代码生成单件结构与废弃传送门的 `postProcess`
也逐方块对拍 0 不一致，允许清单里所有已验证类型全部打开。`structure-pipeline`
对拍把官方 17×17 的 `createStructures` / `createReferences` 与游戏里的同一条路径
逐项比（28 行、84 次比较、2482 个结构件、2752512 格带真实 Beardifier 的 CARVERS 方块
  0 不一致；`--break-self-test` 报 28 处），
覆盖矿井 / jigsaw / 埋藏的宝藏 / 女巫小屋 / 沉船 / 废弃传送门等 8 类实际放置。
真实 `Beardifier` 已接进 Director 的 terrain 阶段与纯 `game_pipeline`，
0/2/8 计算线程逐字节相同（见 `docs/worldgen-structures-plan.md` §12）。

## 带结构的整块端到端对拍（2026-11-16 追加）

`gen-world --through features --structures` / `structure-e2e` 已把**开着结构**的整块生成
纳入 Java 对拍：官方 `createStructures` / `createReferences`（半径 9 网格）+ 真实
`Beardifier` 的 CARVERS + 带结构的 `applyBiomeDecoration`，与游戏侧
`game_features_with_mask`（`generate_structures=true`）逐方块比 3×3 九宫格。
干净跑 50 个结构区块 / 44236800 格 0 不一致；`--break-structure` 六种类型、
`long-set --break-self-test`（3000 组 / 287248 次比较）、`FeaturePoolElement` 多取一次
随机数，都做了改坏确认。默认无结构模式完全不变。

本轮修掉的真 bug：结构分支 `WorldgenRandom` 用 Xoroshiro、官方 `LongOpenHashSet`
引用迭代序、具名 `processor_list` 前缀、`ListPoolElement` / `FeaturePoolElement`
放置、`minecraft:block_pile` 叶子地物、`GravityProcessor` 的 WG 高度图、
`BlockWriter` 的 FINAL 高度图更新、假世界的 `getBlockEntity(pos,type)`。
逐条现象 / 根因 / 盯着的对拍 / 之前漏掉的原因见
[`docs/worldgen-structures-e2e.md`](worldgen-structures-e2e.md)。

**剩余缺口**：沙漠神殿 / 要塞的 `Layout` 起点区块在真实 `WorldGenRegion` 上还有
33 / 412 处 `postProcess` 随机方块变体不一致（`--known-gaps` 单独跑，根因未定位）；
`(42,-32,-16)` 是无结构模式也复现的 `sulfur_pool` 地物差异。默认查询集用这两类的
被引用邻居覆盖跨区块放置。林地府邸 / 海底神殿 / 丛林神殿 / 海底遗迹在本机扫描里
没有有效起点，仍由 `structure-pieces` / `structure-place` / `jigsaw pools` 覆盖。

验收（本机 release）：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets`
0 警告、`cargo test --workspace`、`BAMBOO_TEST_LOADED=1 cargo test -p mc-bamboo-game`、
`cargo check -p mc-bamboo-vanilla --no-default-features` 0 警告；
`worldgen` / `chunkgen` / `gen-world` / `gen-light` / `structures*` / `beard` /
`jigsaw` / `structure-pipeline` / `structure-e2e` / `long-set` 全 0 不一致；
`oracle java --weather clear`；联机探针 719/719 与 `--loaded` 两遍。

## 未合并的分支 / Unmerged branches

每个分支的详细报告在 `W:\data\bamboo-agents\reports\cb-wg-<名>.md`。

| 分支 | main 之外的提交 | 状态 | 下一步 |
|---|---|---|---|
| `cb-wg-structwire` | 42 | 结构接入与 jigsaw 存档；报告称本轮对拍全部 0 不一致（含 1944 查询） | 审阅后合并；再接进 Director |
| `cb-wg-e2e` | 31 | 端到端逐类型对拍，修了 `BiomeFilter`（影响任何 step 9 地物，例如 `patch_grass_jungle`） | 先核实 `BiomeFilter` 修复再合；`monster_room` 的 0 不一致是假绿（改坏抓不到）；另有 20 个类型只做了干净跑，没做改坏确认 |
| `cb-wg-structures` | 14 | 沙漠神殿、丛林神殿、女巫小屋等 4 个代码生成单件结构的 `postProcess`，64 查询 0 不一致 | 和 main 的 `piece_place.rs` 统一放置辅助后合并（报告第 4 节有对应关系） |
| `cb-wg-aquatic` | 2 | 主体已合；剩 `waterlogged_vegetation_patch` 野外触发太少、`pale_moss_patch` 2 处疑点 | 见报告第 5 节交接建议 |
| `cb-wg-feat-b` | 2 | 主体已合；尾部文档 | 可直接并入 |
| `cb-wg-bands` | 1 | 按段生成原型，已暂停（收益约 7%） | 暂不处理 |

其余 `cb-wg-*` 分支已全部并入 main。

The rest of the `cb-wg-*` branches are fully merged.

## 怎么验证 / How to verify

- **Java**：必须用 `C:\Users\Administrator\.jdks\openjdk-26.0.1\bin\java.exe`。PATH 里的 java 是 17，探针会失败。
- **全套合并检查**：在仓库根目录运行
  `bash /w/data/bamboo-agents/tools/merge-checks.sh <名> <端口> [额外对拍子命令...]`。
  结果写到 `W:\data\bamboo-agents\reports\merge-<名>-checks.txt`。结构类合并要额外加 `structure-place structure-templated-place`，光照类要加 `gen-light`。
- **单项对拍**：`cargo run -q -p mc-bamboo-vanilla -- <子命令> <java>`，常用子命令：
  - `gen-world`（`--through features`）
  - `chunkgen --through features`
  - `structure-*`
  - `gen-light`
- **联机探针怕满载**：机器很忙时会随机超时失败，每次挂的项都不一样。全速再跑一遍才算数；719/719 才算通过。
- **重活锁**：`W:\data\bamboo-agents\tools\heavy.js`。全量测试、探针、计时同一时间只跑一个。队员环境里的 `shim\cargo.cmd` 会让全量 `cargo test` 和 `wg-perf` 自动排队。

## 队员设施 / Agent infrastructure

全部在 `W:\data\bamboo-agents`：
- 工作目录：`wt\<名>`；
- 任务书：`prompts\`；
- 报告：`reports\`；
- 日志：`logs\`；
- 启动脚本：
  - `cur-run.sh` / `cur-loop.sh`（Cursor CLI）；
  - `cb-run.sh` / `cb-resume.sh`（CodeBuddy hy4）；
- 停队员：`kill-agent.ps1 -Name <名>`，必须单独一条命令执行；
- 限速：`lowprio.sh` 的 `THROTTLE`，配合 `throttle-now.ps1` / `unthrottle-now.ps1`。

模型顺序：
1. 优先 hy4（CodeBuddy `hy4std`，免费）；
2. 已在跑的 DS Flash 队员继续用；
3. 只有长时间无进展的队员，才换 Cursor 的 `claude-opus-5-5-medium`。

DS Flash 会话太长时会空转：每轮只说"开始执行"就退出。遇到这种情况就新开会话。

Model order:
1. hy4 first (free);
2. DS Flash for agents already running on it;
3. Opus 5.5 medium only for agents stalled a long time.

Restart a DS Flash session once it starts looping without acting.
