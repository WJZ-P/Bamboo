# 命令执行上下文：`/execute` 的设计

本文定义 `/execute` 如何改变一条命令的执行来源（官方 `CommandSourceStack`），
以及改过以后怎么执行链尾 `run` 的命令。实现放在 `crates/mc-bamboo-game/src/command`。

## 1. 为什么现有框架不够

`command::run` 现在把解析结果 `ParsedCommand` 按 `parsed.action` 查一张
`Action::Island(IslandFn)` / `Action::Global(CommandFn)` 表，直接调用一次动作。
动作拿到的「来源」只有 `PlayerId`：玩家位置/朝向/权限都从 `region.players[id]` 查，
控制台则从 `source::of` 取全局出生点。这个模型没法表达 `execute as @e at @s run ...`
里「位置和执行者是不同实体」「每一步都产生一个新来源」以及「`as` 会分叉出多个来源」。

26.3 官方 `ExecuteCommand.register` 用 Brigadier 的 `fork` / `redirect`：
`as`、`at` 是 `fork`（一个来源展开成多个），`positioned`、`rotated`、`facing`、
`align`、`anchored`、`in` 是 `SingleRedirectModifier`（一对一改来源），
`if` / `unless` 在执行时按条件过滤，`store` 在链尾命令结束以后把结果写回，
`run` 后面重定向到整棵命令树的根节点。Bamboo 的 `CommandGraph` 已经支持
`redirect` 和重定向后的多层 `Context`（`dispatcher.rs`），但没有 `fork`，
也没有把「链上每一层的参数」保留给执行期。

## 2. 解析结果：`ExecutionChain`

在 `mc-bamboo-command` 里给 `ParsedCommand` 增加一条可选的 `chain`
（默认 `None`，其它命令不受影响）：

`ParsedCommand.chain: Vec<ChainStep>`。`ChainStep` 带 `level`（重定向产生的
上下文层号）、`kind`（这一层第一个字面量：`as` / `at` / `if` / `store` / `run` ...）
和这一层自己的 `arguments: Vec<(String, Value)>`。解析仍由 `CommandGraph` 做：
`/execute` 的语法树按官方 `commands.json` 注册，链上每个修饰符节点的 redirect
指回 `execute`，`run` 的 redirect 指到根。`dispatcher::collect_execute_chain` 从
`Context::layers()` 把每层参数原样抄进 `chain`（不另写一套 Brigadier）。
链尾 `run` 后面的原文由执行器按 `parsed.cursor` 和层号从原命令行切出来。

链尾命令原文在执行期重新用同一棵 `CommandGraph::parse` 解析，权限级别沿用
**原始来源**的权限（官方 `CommandSourceStack.permissions()` 不随 `as` 变）；
这一步不做签名校验、不重复刷屏计数，和官方在 `execute` 内部执行子命令一致。

## 3. 执行来源：`Context`

```text
Context {
    source: PlayerId,        // 反馈、日志、广播用的原始来源，永不变
    position: Vec3,          // getPosition()
    yaw, pitch: f32,         // getRotation()
    anchor: EntityAnchor,    // feet | eyes
    entity: Option<Target>,  // getEntity()：as 改变它，at 不改变
    dimension: &str          // 只有 minecraft:overworld；in 其它维度直接失败
    permission: u8,          // 原始来源的权限，链上所有解析都用它
}
```

- 玩家来源从 `source::of` 初始化；控制台来源是出生点左下角、ZERO 朝向、无实体。
- `as` **只**改 `entity`；`at` 把 `position`、`yaw`、`pitch` 改为目标实体的值和朝向，
  **不**改 `entity`。`positioned as` / `rotated as` 同理分别只改位置/朝向。
- `anchored eyes` 影响 `facing entity` 的起点（`CommandSourceStack.getPosition`
  在官方里随 anchor 变）；实现把 `anchor` 单独保存，`facing` 计算时按 anchor 加眼高。
- `in` 只允许 `minecraft:overworld`；其它维度报 `argument.dimension.invalid`。
- 分叉：`as` / `at` / `facing entity` / `rotated as` / `positioned as` /
  `if entity`（多目标时按官方分叉）会把一个 `Context` 展开成多个；其余步骤一对一。
  分叉的顺序就是 `EntitySelector` 解析出来的顺序，条件为假时该分支被丢弃。
- 一个 `execute` 链里所有分支**当场顺序执行**（同一 tick、同一岛）；Bamboo 没有
  命令队列的跨刻调度。官方 26.3 的分叉在 `ExecutionContext` 里排队，但结果等价。

## 4. `if` / `unless` 与 `store`

- `if/unless block`：按当前 `Context` 解 `BlockPos`，读 `region.chunks`；
  `blocks` 比较两个长方体（`all` / `masked`），超过 `MAX_TEST_AREA` 报错。
- `if/unless entity`：用 `select::targets` 按当前上下文（执行位置、执行者）解析，
  `if` 时每个命中实体分叉一个 `Context`（官方 `expect(..., true, ...)`），
  `unless` 只在零命中时继续。
- `if/unless score`：查权威计分板（`GlobalPort::Direct` 里的 `Global` 或岛上的
  全局快照），`matches` 和两种目标的比较都按官方字节码。
- `if/unless predicate`：调用 `mc-bamboo-loot` 的谓词求值；依赖缺失的数据源明确报错，
  不静默判假。
- 条件失败时官方发 `command.execute.conditions.not_met*` 反馈；Bamboo 同样按
  分支数发，且不执行链尾命令。
- `store result|success`：`result` 是链尾命令的返回值，`success` 是
  「成功执行的分支数」（官方 `CommandResultCallback`）。Bamboo 的动作返回
  `Result<(), CommandFail>`，所以 `success` = 成功的分支数，`result` 先固定为
  该次数（官方很多命令的返回值就是成功次数）。`store score` 在链尾命令**全部
  分支跑完**以后一次性写回目标分数；`store bossbar` 写 value/max；
  `store block|entity|storage` 需要 NBT 路径，第三期接 `/data` 时复用同一套，
  本期的 NBT 目标明确报不支持。
- `run` 的结果回传：链尾每个分支的动作照常发自己的成功/失败反馈，
  执行器把它折算成 `(result, success)` 交给最近一层 `store`；没有 `store` 时丢弃。
  链上没有 `run` 时整条命令不执行（`command.unknown.command`）。

## 5. 多岛

`SCOPES` 的 `execute` 必须是 `World`：链上任一步都可能按坐标读别的岛的方块、
按 `@e` 选中别的岛的非玩家实体，或者 `run` 出一条方块命令。主控按 4.5 把全部岛
临时拼成一个再执行；`CLASSES` 里 `execute` 归 `CrossIsland`：入口是岛上动作，但 `run` 重定向到
整棵树，`class.rs` 的动作核对因此允许岛上/全局两类动作同时可达。
`store score` 经 `GlobalPort` 写权威计分板，其它分支直接改拼起来的 `Region`。

## 6. 对拍

`tools/mc-bamboo-vanilla execute-oracle` 用官方 `Commands.performCommand`
在假世界里跑 34 条 `/execute` 命令；假世界补全了 `ServerLevel` 的实体 getter、
`ChunkMap` 和真正的牛实体，所以 `as @e[type=cow]`、`at @e`、`if entity @e[...]`
真的会选出牛并分叉。记录：

- 每条反馈的 `TranslatableContents` 翻译键和参数，递归展开子组件；
- 方块表、玩家位置、计分板分数这些**副作用**；
- `as @e`/`at @e` 的分叉次数和顺序。

Rust 侧在同一串命令上重放并逐项比较；`--break-self-test` 改坏一处副作用，
`--break-group run|conditions|as|at|positioned|rotated|facing|align|anchored|blocks|store`
按类别改坏一个字段，确认每类都能被抓到。

## 7. 已知差异

- 只有主世界，`in` 其它维度明确失败；没有 `on`、`summon`、`positioned over`、
  `if biome|data|items|slots|stopwatch|function|dimension|loaded|predicate` 的
  完整实现：命令树注册但执行时明确报错。
- `facing` / `anchored` 用脚底或眼高的近似眼高，属性表没有逐实体眼高。
- `store ... block|entity|storage` 的 NBT 路径等第三期 `/data` 接上。
