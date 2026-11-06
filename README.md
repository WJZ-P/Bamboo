<div align="center">

<img src="assets/icon.png" width="128" alt="Bamboo" />

# Bamboo

用 Rust 从零编写的 Minecraft 服务端

A Minecraft server written from scratch in Rust

</div>

> 项目处于早期阶段。玩家已经能用 Minecraft 26.3 进入一个超平坦世界，但走动时不会加载新区块，也不能和方块交互。

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

## 项目结构

| 路径 | 职责 |
|---|---|
| `crates/mc-bamboo` | 命令行入口，可执行文件 `bamboo` |
| `crates/mc-bamboo-server` | 服务端运行时：监听、连接状态机、玩家表 |
| `crates/mc-bamboo-protocol` | 网络协议：VarInt、帧、数据包，只处理字节 |
| `crates/mc-bamboo-world` | 游戏世界：区块、调色板、光照、世界生成，不知道网络 |
| `crates/mc-bamboo-nbt` | NBT 编码 |
| `crates/mc-bamboo-data` | 版本数据：由官方服务端生成的 ID、注册表和标签 |
| `apps/desktop` | 桌面控制台：Tauri + React + styled-components |
| `tools/mc-bamboo-vanilla` | 开发工具：生成版本数据、用官方网络代码验证 Bamboo |

依赖方向是单向的：

```text
mc-bamboo   apps/desktop
       \     /
  mc-bamboo-server
     /         \
 protocol     world
   /   \        |
 nbt    data <--+
```

## 升级游戏版本

版本相关的数字都在 `mc-bamboo-data` 里。升级时从官方下载 `server.jar`，再运行：

```bash
cargo run -p mc-bamboo-vanilla -- generate <server.jar>
```

然后启动 Bamboo，用官方网络代码走一遍进服流程，检查发出的每个包：

```bash
cargo run -p mc-bamboo-vanilla -- probe 127.0.0.1 25565
```

这两个命令需要 Java 25 或更新版本。

## License

代码使用 [AGPL-3.0-only](LICENSE) 许可。以下内容不在 AGPL 许可范围内：

- `assets/icon.png`、`apps/desktop/src/assets/icon.png` 和 `apps/desktop/src-tauri/icons/` 是 Minecraft 的竹子贴图，版权归 Mojang 所有。
- `crates/mc-bamboo-data/src/generated/` 里的数据来自官方服务端，相关权利归 Mojang 所有。
- `apps/desktop/src/assets/fonts/` 是 [Fusion Pixel Font](https://github.com/TakWolf/fusion-pixel-font)，许可证见同目录的 `licenses/`。

Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.
