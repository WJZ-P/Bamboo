# Bamboo 展示页

项目的网页介绍：进度、已有功能、架构和测量数据，顶部是一座用 WebGL2 画的浮空竹岛。

The project's showcase page: progress, features, architecture and measurements, under a
floating bamboo island drawn with WebGL2.

技术栈和桌面控制台（`apps/desktop`）一致：React + Vite + TypeScript + styled-components。
它是一个独立的包，不依赖仓库里的其它代码，也不改动它们。

## 运行

```bash
cd apps/web
pnpm install
pnpm dev
```

打包用 `pnpm build`，产物在 `dist/`，是纯静态文件，资源路径是相对的，放在任何子路径下都能打开。

## 数据从哪来

| 文件 | 内容 | 怎么更新 |
|---|---|---|
| `src/data/stats.generated.json` | 提交数、Rust 行数、测试数、每个 crate 的行数 | `pnpm stats`，脚本只读仓库 |
| `src/data/project.ts` | 里程碑、世界模型九步、功能列表、基线数据、oracle 列表 | 手写，每组旁边注明出自哪份文档 |

页面上的数字都来自仓库里的 README 和 `docs/`。进度变了，改 `project.ts` 里对应的那一条。

## 结构

| 路径 | 职责 |
|---|---|
| `src/voxel` | 体素场景：程序生成的贴图、小岛、网格构建（带环境光遮蔽和方块光）、射线拾取、WebGL2 渲染 |
| `src/region` | Region 合并与拆分的示意动画，规则照 `docs/world-model-design.md` 6.1 |
| `src/sections` | 页面的各个板块 |
| `src/components` | 像素风的按钮、面板、图表、图标和导航栏 |
| `src/theme` | 设计变量、全局样式、背景贴图 |
| `src/i18n.tsx` | 中英文切换 |

## 素材

方块贴图全部由 `src/voxel/tiles.ts` 按固定种子画出来，不使用任何游戏素材。
字体是 Fusion Pixel（OFL-1.1），许可证在 `src/assets/fonts/licenses/`。
