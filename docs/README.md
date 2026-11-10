# 文档索引

目标版本只有一个：Minecraft Java 26.3，协议 777，`DataVersion` 5023。

## 总约定

| 文件 | 内容 |
|---|---|
| `design.md` | M2 实现的总约定：决定表、crate 结构、线程和队列、tick 顺序、代码规范、验证阶梯。规格和它冲突时以它的“决定”一节为准 |
| `folia.md` | Folia 调研和 Bamboo 的多线程设计。线程规则 R1–R12 的出处 |
| `coverage.md` | M2 完整性审查：69 个 serverbound 包的分类、合并后的进服和离开序列、tick 顺序、超时规则、验收用例。规格之间冲突时以它为准 |

## 协议和行为规格

| 文件 | 内容 |
|---|---|
| `chunks-movement.md` | 移动包、`T(vd)`、`PlayerChunkSender`、传送确认 |
| `entities.md` | Tab 列表、实体可见性、`sendChanges`、元数据、挥手、装备 |
| `blocks-inventory.md` | 破坏、放置、ack、创造模式物品栏、中键、数据生成、方块实体列表 |
| `chat-join.md` | 文本组件、聊天、命令入口、进服和离开序列 |
| `placement.md` | 方块行为：放置状态、形状更新、开关类方块 |

## 设计

| 文件 | 内容 |
|---|---|
| `m3-storage.md` | M3 存档设计：Anvil 和 Linear 格式、区块 NBT、后台存档线程、崩溃安全、玩家数据和 `level.dat`、转换工具、分步实现计划 |
| `world-model-discussion.md` | **探讨稿，未定稿**：原版 26.3 世界模型的关键细节、九项决策、多 region 的切分合并和同步设计、调研出处、region 内部并行的选项 |
| `world-model-design.md` | **已确认，实施中**：世界模型的整套架构。三个平面加主控、区块束、区块加载流水线、region 的形成和调度、实体和生物 AI 的并行、红石热点优化、过载降级、实施顺序 |
| `chunk-tickets.md` | 区块平面的第一块：原版 26.3 ticket 机制的事实、`TicketTable` 的接口和不变量、与官方的差异、oracle 的用法和结果、玩家移动基准 |

## 测量

| 文件 | 内容 |
|---|---|
| `profiling.md` | tick 剖析器各项的含义、假玩家压测工具与 `run-baseline.sh` 的用法、第一份基线数据 |
