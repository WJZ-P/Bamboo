<div align="center">

<img src="assets/icon.png" width="128" alt="Bamboo" />

# Bamboo

用 Rust 从零编写的 Minecraft 服务端

A Minecraft server written from scratch in Rust

</div>

> 项目处于早期阶段。多名玩家已经能用 Minecraft 26.3 进入同一个超平坦世界，走动、互相看见、放置和破坏方块、聊天。现在只有创造模式，也没有存档。

## 运行

命令行版：

```bash
cargo run --bin bamboo
```

默认监听 `0.0.0.0:25565`，也可以传入地址：`cargo run --bin bamboo -- 127.0.0.1:25565`。

桌面控制台（需要 Node.js 和 pnpm）：

```bash
cd apps/desktop
pnpm install
pnpm start
```

打包用 `pnpm tauri build`。

## 现在能玩什么

- 离线模式登录，多名玩家进入同一个超平坦世界。
- 区块跟着玩家走，Tab 列表、玩家互相可见、潜行、冲刺、挥手、手持物同步。
- 创造模式的破坏和放置。全部官方方块类都移植了：朝向、连接、支撑和右键走官方逻辑，用官方代码做 oracle 对比验证。
- 水和岩浆会流动：往下落、往四周流、断了源头会干，两格水源中间会生成新的水源，冲掉花草这些方块，
  岩浆碰到水变成黑曜石、圆石或者石头，灵魂土上碰到蓝冰变成玄武岩。水底的灵魂沙和岩浆块会长出气泡柱。
- 桶：空桶舀起水、岩浆、细雪和含水方块里的水，满桶倒进空地或者含水方块，鱼桶倒出水。创造模式手里的桶不变，
  舀起来的满桶放进物品栏。
- 创造模式物品栏、中键选取、F 换手、Q 丢弃。
- 聊天，加入和离开消息。刷屏的玩家和官方一样会被踢。

已知限制：

- 世界里没有红石信号、方块下落、植物生长和掉落物。`powered` 只有玩家直接切换的方块才会变。
- 流体只有主世界的规则，游戏规则是默认值，改不了。岩浆不会点燃旁边的方块（随机刻不在范围里）。
  岩浆变高时官方随机地慢四倍，Bamboo 没有随机源，用位置和时间的散列代替。鱼桶倒出来的只有水，没有鱼。
  没有音效和粒子：岩浆凝固的嘶嘶声、桶的声音都没有。
- 用桶和炼药锅互动时，官方会给创造模式的物品栏补一个空桶或者满桶（原来没有的话），Bamboo 不补。
- 光照是简化的：天空光把所有非空气方块都当成不透明，不发方块光。要看亮度的判定固定按 15 算，也就是开阔的白天，和官方有偏差。
- 方块实体没有内容：箱子这类容器打不开，只吃掉点击，告示牌不能写字。物品组件不影响放下的方块。
- 不是方块的物品没有自己的用法：斧头削不了原木，打火石点不了火。方块自己认的物品除外，比如剪刀能雕南瓜。
- 命令方块这类管理员方块不能放置，Bamboo 没有权限系统。
- 不存档：关服后世界重置，重连回到出生点，物品栏清空。
- 没有命令，没有物品实体，丢出的物品会消失。鞘翅不能用，攻击其他玩家没有效果。
- 没有正版验证、加密和压缩。

## 项目结构

| 路径 | 职责 |
|---|---|
| `crates/mc-bamboo` | 命令行入口，可执行文件 `bamboo` |
| `crates/mc-bamboo-server` | 网络层：监听、连接状态机、读写队列、登录准入 |
| `crates/mc-bamboo-game` | 世界线程：tick、玩家、区块跟踪、实体同步、方块交互、聊天，不依赖 tokio |
| `crates/mc-bamboo-protocol` | 网络协议：VarInt、帧、数据包，只处理字节 |
| `crates/mc-bamboo-world` | 游戏世界：区块、调色板、光照、世界生成，不知道网络 |
| `crates/mc-bamboo-blocks` | 方块行为：放置、形状更新、破坏、右键、计划刻，纯逻辑，不知道网络和存储 |
| `crates/mc-bamboo-core` | 共用的基础类型：方块坐标、区块坐标、方向 |
| `crates/mc-bamboo-nbt` | NBT 编码 |
| `crates/mc-bamboo-data` | 版本数据：由官方服务端生成的协议号、包 ID、方块、物品、注册表和标签 |
| `apps/desktop` | 桌面控制台：Tauri + React + styled-components |
| `tools/mc-bamboo-vanilla` | 开发工具：生成版本数据、用官方代码验证 Bamboo 的协议和方块行为 |

依赖方向是单向的：

```text
mc-bamboo   apps/desktop
       \     /
  mc-bamboo-server
         |
   mc-bamboo-game
    /    |     \
protocol blocks world
```

`protocol`、`blocks` 和 `world` 互不依赖，它们都依赖 `core` 和 `data`，`protocol` 还依赖 `nbt`。

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

然后启动 Bamboo，用官方网络代码让三个客户端走一遍进服、移动、建造和聊天，检查收到的每个包：

```bash
cargo run -p mc-bamboo-vanilla -- probe 127.0.0.1 25565
```

最后让官方代码给方块行为出题和给答案，用 `mc-bamboo-blocks` 重放，比较两边的结果：

```bash
cargo run --release -p mc-bamboo-vanilla -- oracle <java 路径> [--cases N] [--only <方块或类名>]
```

这些命令需要 Java 25 或更新版本。

## License

代码使用 [AGPL-3.0-only](LICENSE) 许可。以下内容不在 AGPL 许可范围内：

- `assets/icon.png`、`apps/desktop/src/assets/icon.png` 和 `apps/desktop/src-tauri/icons/` 是 Minecraft 的竹子贴图，版权归 Mojang 所有。
- `crates/mc-bamboo-data/src/generated/` 里的数据来自官方服务端，相关权利归 Mojang 所有。
- `apps/desktop/src/assets/fonts/` 是 [Fusion Pixel Font](https://github.com/TakWolf/fusion-pixel-font)，许可证见同目录的 `licenses/`。

Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.
