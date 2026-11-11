import type { StatusKey } from "../components/pixel";
import type { Text } from "../i18n";

/**
 * 页面上所有关于项目进度的文字和数字都在这里，出处写在每一组旁边。
 * 代码行数、提交数这类会变的数字不在这里，在 stats.generated.json 里，用 pnpm stats 重新生成。
 *
 * Every statement and figure about the project's progress lives here, with its source noted
 * beside each group. Numbers that drift, such as line and commit counts, are not here: they
 * are in stats.generated.json and are regenerated with `pnpm stats`.
 */

export const REPO_URL = "https://github.com/WJZ-P/Bamboo";

export function docUrl(file: string): string {
  return `${REPO_URL}/blob/main/docs/${file}`;
}

export const TARGET = { version: "26.3", protocol: 777, dataVersion: 5023 } as const;

export const STATUS_LABEL: Record<StatusKey, Text> = {
  done: { zh: "已完成", en: "Done" },
  active: { zh: "进行中", en: "In progress" },
  partial: { zh: "做了一部分", en: "Partly done" },
  planned: { zh: "未开始", en: "Not started" },
};

// ---- 里程碑。出处：README.md、docs/m3-storage.md 第 12 节。 ----
// ---- Milestones. Sources: README.md and section 12 of docs/m3-storage.md. ----

export interface Milestone {
  readonly id: string;
  readonly title: Text;
  readonly status: StatusKey;
  readonly summary: Text;
  readonly progress?: { readonly done: number; readonly total: number };
}

export const MILESTONES: readonly Milestone[] = [
  {
    id: "M0",
    title: { zh: "握手与状态", en: "Handshake & status" },
    status: "done",
    summary: {
      zh: "客户端的服务器列表里能看到 Bamboo，能读到介绍和人数。",
      en: "Bamboo shows up in the client's server list with its description and player count.",
    },
  },
  {
    id: "M1",
    title: { zh: "登录与超平坦", en: "Login & flat world" },
    status: "done",
    summary: {
      zh: "离线登录、Configuration 阶段、超平坦区块、心跳。真实的 26.3 客户端可以进服。",
      en: "Offline login, the Configuration phase, flat chunks and keep-alive. A real 26.3 client can join.",
    },
  },
  {
    id: "M2",
    title: { zh: "移动、方块、聊天", en: "Movement, blocks, chat" },
    status: "active",
    summary: {
      zh: "多人移动、方块、聊天都能用，还提前补上了全部方块行为、红石、容器和 44 条命令。还差世界边界和配方书。",
      en: "Multiplayer movement, blocks and chat work, plus every block behaviour, redstone, containers and 44 commands ahead of plan. The world border and recipe book remain.",
    },
  },
  {
    id: "M3",
    title: { zh: "存档", en: "World saves" },
    status: "active",
    summary: {
      zh: "编解码层已完成：NBT、区块 NBT、Anvil 区域文件，都和官方代码双向对拍。还没接进游戏，关服后世界仍会重置。",
      en: "The codec layer is done: NBT, chunk NBT and Anvil region files, each checked both ways against the official code. It is not wired into the game yet, so the world still resets on shutdown.",
    },
    progress: { done: 4, total: 9 },
  },
  {
    id: "M4",
    title: { zh: "世界生成", en: "World generation" },
    status: "partial",
    summary: {
      zh: "数学部分已完成：随机源、噪声、密度函数、气候和群系源，在几十万个点上和官方结果一致。地形还没接进游戏。",
      en: "The maths is done: random sources, noise, density functions, climate and biome sources match the official results at hundreds of thousands of points. Terrain is not wired in yet.",
    },
  },
  {
    id: "M5",
    title: { zh: "物品栏、实体、生存", en: "Inventory, entities, survival" },
    status: "partial",
    summary: {
      zh: "提前做了一部分：容器和工作站菜单、掉落物、下落方块、TNT、战利品表。生物和 AI 还没开始。",
      en: "Partly done ahead of plan: container and workstation menus, item entities, falling blocks, TNT and loot tables. Mobs and AI have not started.",
    },
  },
];

// ---- 世界模型改造的九步。出处：docs/world-model-design.md 第 16 节和合并记录。 ----
// ---- The nine steps of the world-model rework. Sources: section 16 of
// ---- docs/world-model-design.md and the merge history.

export interface Step {
  readonly index: number;
  readonly title: Text;
  readonly status: StatusKey;
  readonly note: Text;
}

export const WORLD_MODEL_STEPS: readonly Step[] = [
  {
    index: 0,
    title: { zh: "剖析器", en: "Profiler" },
    status: "done",
    note: {
      zh: "每个 tick 阶段的耗时直方图、假玩家压测工具、三份基线。",
      en: "Per-phase tick histograms, a fake-player load tool and three baselines.",
    },
  },
  {
    index: 1,
    title: { zh: "区块束", en: "Chunk columns" },
    status: "active",
    note: {
      zh: "方块、光照、方块实体、计划刻已经能整柱搬走和放回。",
      en: "Blocks, light, block entities and scheduled ticks can already be taken and put back a column at a time.",
    },
  },
  {
    index: 2,
    title: { zh: "网络", en: "Network" },
    status: "active",
    note: {
      zh: "出站缓冲已分段，区块帧只做一次、所有玩家共用。",
      en: "Outbound buffers are segmented, and a chunk frame is built once and shared by every player.",
    },
  },
  {
    index: 3,
    title: { zh: "区块平面", en: "Chunk plane" },
    status: "active",
    note: {
      zh: "ticket 表已完成并和官方逐位对拍。Director 和加载流水线还没做。",
      en: "The ticket table is done and matches the official one bit for bit. The Director and loading pipeline are still to come.",
    },
  },
  {
    index: 4,
    title: { zh: "主控和 region 调度", en: "Coordinator & scheduling" },
    status: "planned",
    note: { zh: "每个维度先一个 region，维度之间并行。", en: "One region per dimension first, with dimensions in parallel." },
  },
  {
    index: 5,
    title: { zh: "分区、合并、拆分", en: "Partition, merge, split" },
    status: "planned",
    note: { zh: "一个维度里出现多个 region。", en: "Several regions inside one dimension." },
  },
  {
    index: 6,
    title: { zh: "子任务和降级", en: "Subtasks & degradation" },
    status: "planned",
    note: { zh: "扎堆场景下 region 内部并行，过载时先降玩家看不见的东西。", en: "Parallelism inside a crowded region, and shedding what players cannot see under overload." },
  },
  {
    index: 7,
    title: { zh: "实体系统", en: "Entity system" },
    status: "active",
    note: {
      zh: "实体存储和“并行算、按顺序落地”的两段式驱动已合并，目前在开关后面。",
      en: "The entity store and the two-phase driver (compute in parallel, commit in order) are merged, behind a switch for now.",
    },
  },
  {
    index: 8,
    title: { zh: "生物和 AI", en: "Mobs & AI" },
    status: "planned",
    note: { zh: "感知、决策、寻路都落在并行的那一段里。", en: "Sensing, decisions and pathfinding all land in the parallel phase." },
  },
];

// ---- 现在能玩什么。出处：README.md“现在能玩什么”一节。 ----
// ---- What works today. Source: the "现在能玩什么" section of README.md. ----

export type IconName =
  | "players"
  | "pickaxe"
  | "redstone"
  | "water"
  | "chest"
  | "terminal"
  | "sun"
  | "gem"
  | "lock"
  | "monitor";

export interface Feature {
  readonly icon: IconName;
  readonly title: Text;
  readonly body: Text;
  readonly tags: readonly Text[];
}

const same = (text: string): Text => ({ zh: text, en: text });

export const FEATURES: readonly Feature[] = [
  {
    icon: "players",
    title: { zh: "多人联机", en: "Multiplayer" },
    body: {
      zh: "多名玩家进同一个世界，互相看得见。区块跟着玩家走，潜行、冲刺、挥手、手持物都同步。",
      en: "Several players share one world and see each other. Chunks follow the player; sneaking, sprinting, arm swings and held items are synchronised.",
    },
    tags: [{ zh: "Tab 列表", en: "Tab list" }, { zh: "实体跟踪", en: "Entity tracking" }],
  },
  {
    icon: "pickaxe",
    title: { zh: "方块行为", en: "Block behaviour" },
    body: {
      zh: "全部官方方块类都已移植：朝向、连接、支撑、右键，逐个状态和官方代码比对。生存挖掘有进度、裂纹和工具耐久。",
      en: "Every official block class is ported: facing, connections, support and use, compared with the official code state by state. Survival mining has progress, cracks and tool durability.",
    },
    tags: [{ zh: "告示牌", en: "Signs" }, { zh: "书与笔", en: "Book & quill" }, { zh: "讲台", en: "Lecterns" }],
  },
  {
    icon: "redstone",
    title: { zh: "红石", en: "Redstone" },
    body: {
      zh: "信号、邻居更新和带优先级的计划刻按官方逻辑计算。活塞最多推 12 格，粘液块和蜂蜜块会带上一串方块。",
      en: "Signals, neighbour updates and prioritised scheduled ticks follow the official logic. Pistons push up to 12 blocks, and slime and honey blocks drag their neighbours along.",
    },
    tags: [
      { zh: "中继器", en: "Repeaters" },
      { zh: "比较器", en: "Comparators" },
      { zh: "侦测器", en: "Observers" },
      { zh: "漏斗", en: "Hoppers" },
      { zh: "合成器", en: "Crafters" },
    ],
  },
  {
    icon: "water",
    title: { zh: "流体和生长", en: "Fluids & growth" },
    body: {
      zh: "水和岩浆会流动，断了源头会干，两格水源之间生出新水源。随机刻让作物生长，骨粉催熟，天气会结冰积雪。",
      en: "Water and lava flow, dry up without a source and form new sources between two others. Random ticks grow crops, bone meal speeds them up, and weather freezes water and lays snow.",
    },
    tags: [{ zh: "桶", en: "Buckets" }, { zh: "骨粉", en: "Bone meal" }, { zh: "天气", en: "Weather" }],
  },
  {
    icon: "chest",
    title: { zh: "容器和工作站", en: "Containers & workstations" },
    body: {
      zh: "箱子、木桶、潜影盒、末影箱能存取。工作台、熔炉、酿造台、铁砧、附魔台、织布机、切石机、锻造台都有菜单。",
      en: "Chests, barrels, shulker boxes and ender chests store items. Crafting tables, furnaces, brewing stands, anvils, enchanting tables, looms, stonecutters and smithing tables all have menus.",
    },
    tags: [{ zh: "配方匹配", en: "Recipe matching" }, { zh: "容器锁", en: "Container locks" }],
  },
  {
    icon: "gem",
    title: { zh: "掉落和战利品", en: "Drops & loot" },
    body: {
      zh: "挖掉的方块按战利品表掉在地上，能捡起来，按 Q 丢出去。随机数的取用次数和顺序跟官方一致。",
      en: "Mined blocks drop according to loot tables, can be picked up and thrown with Q. Random numbers are drawn the same number of times, in the same order, as the official code.",
    },
    tags: [{ zh: "箱子战利品", en: "Chest loot" }, { zh: "下落方块", en: "Falling blocks" }, same("TNT")],
  },
  {
    icon: "terminal",
    title: { zh: "44 条命令", en: "44 commands" },
    body: {
      zh: "Brigadier 风格的命令树，带坐标、方块状态和选择器的解析。计分板、队伍和 Boss 栏照官方的命令树实现。",
      en: "A Brigadier-style command tree with parsers for coordinates, block states and selectors. Scoreboards, teams and boss bars follow the official command tree.",
    },
    tags: [same("/tp"), same("/give"), same("/fill"), same("/scoreboard"), same("/whitelist")],
  },
  {
    icon: "sun",
    title: { zh: "光照", en: "Lighting" },
    body: {
      zh: "天空光和方块光和官方一样：屋顶下的光从四边漏进来，树叶和水减弱光，半砖和楼梯按形状挡光。",
      en: "Sky light and block light match the official results: light leaks in under roofs, leaves and water dim it, and slabs and stairs block it by shape.",
    },
    tags: [{ zh: "高度图", en: "Heightmaps" }],
  },
  {
    icon: "lock",
    title: { zh: "网络和安全", en: "Network & security" },
    body: {
      zh: "压缩、正版验证和加密、安全聊天、Velocity 现代转发。玩家的移动会被校验，走得太快或穿进方块会被拉回。",
      en: "Compression, online-mode authentication and encryption, secure chat and Velocity modern forwarding. Player movement is validated: moving too fast or into a block snaps you back.",
    },
    tags: [same("RSA + AES/CFB8"), { zh: "刷屏踢出", en: "Spam kicks" }],
  },
  {
    icon: "monitor",
    title: { zh: "桌面控制台", en: "Desktop console" },
    body: {
      zh: "Tauri 写的桌面应用：总览、控制台、玩家、设置四个页面，带 CPU 和内存曲线。服务端就跑在应用进程里。",
      en: "A Tauri desktop app with overview, console, players and settings pages, plus CPU and memory graphs. The server runs inside the app's own process.",
    },
    tags: [same("Tauri"), same("React")],
  },
];

/** 还做不到的事，照实写。 / What is not there yet, stated plainly. */
export const NOT_YET: readonly Text[] = [
  { zh: "不存档：关服后世界重置，物品栏清空。", en: "No saving: the world resets and inventories empty on shutdown." },
  { zh: "世界只有超平坦，地形生成还没接进游戏。", en: "The world is flat only; terrain generation is not wired in." },
  { zh: "没有生物，也没有 AI。", en: "No mobs and no AI." },
  { zh: "活塞不推实体，也不推玩家。", en: "Pistons do not push entities or players." },
  { zh: "放置方块还不扣物品。", en: "Placing a block does not consume the item yet." },
];

// ---- 架构。出处：docs/world-model-design.md 第 0、2、6 节。 ----
// ---- Architecture. Source: sections 0, 2 and 6 of docs/world-model-design.md. ----

export interface Plane {
  readonly key: "network" | "simulation" | "coordinator" | "chunk";
  readonly title: Text;
  readonly runsOn: Text;
  readonly body: Text;
}

export const PLANES: readonly Plane[] = [
  {
    key: "network",
    title: { zh: "网络平面", en: "Network plane" },
    runsOn: { zh: "tokio，每名玩家一条任务", en: "tokio, one task per player" },
    body: {
      zh: "读帧、解码、加解密、写 socket。只做 IO，不碰世界。",
      en: "Reads frames, decodes, encrypts and writes sockets. IO only; it never touches the world.",
    },
  },
  {
    key: "simulation",
    title: { zh: "模拟平面", en: "Simulation plane" },
    runsOn: { zh: "tick 线程池", en: "Tick thread pool" },
    body: {
      zh: "Region × N。每个 region 独占自己的区块、实体和玩家，不用锁，按截止时间调度。",
      en: "Region × N. Each region owns its chunks, entities and players outright, with no locks, scheduled by deadline.",
    },
  },
  {
    key: "coordinator",
    title: { zh: "主控", en: "Coordinator" },
    runsOn: { zh: "一条线程上的几个 actor", en: "A few actors on one thread" },
    body: {
      zh: "Global 管全局状态，每个维度一个 Director 管 ticket 表和分区。只记账和路由，不在热路径上。",
      en: "Global keeps server-wide state; one Director per dimension keeps the ticket table and the partition. It only keeps books and routes, off the hot path.",
    },
  },
  {
    key: "chunk",
    title: { zh: "区块平面", en: "Chunk plane" },
    runsOn: { zh: "后台计算池加 IO 线程", en: "Background pool plus IO threads" },
    body: {
      zh: "解析、生成、光照、编码、压缩。只跑纯函数，做好的区块束整盒交给 region。",
      en: "Parsing, generation, lighting, encoding and compression. Pure functions only; finished chunk columns are handed to regions as whole boxes.",
    },
  },
];

export const REGION_RULES: readonly Text[] = [
  { zh: "网格单位是 4×4 个区块，叫一个 section。", en: "The grid unit is 4×4 chunks, called a section." },
  { zh: "两个 region 的 section 相隔不超过 2 格就合并。", en: "Two regions merge when their sections come within 2 cells." },
  { zh: "拉开到 4 格并保持 10 秒才拆分，避免来回抖动。", en: "They split only after staying 4 cells apart for 10 seconds, which avoids flapping." },
  { zh: "每个 region 有自己的时钟：卡了只慢自己，太阳照常走。", en: "Each region has its own clock: lag slows only that region, and the sun keeps moving." },
  {
    zh: "视距 10 时，两名玩家拉开约 590–640 格就会分到两个 region；Folia 默认要 69–84 个区块。",
    en: "At view distance 10, two players about 590–640 blocks apart end up in separate regions; Folia's default needs 69–84 chunks.",
  },
];

export const CRATE_ROLES: Record<string, Text> = {
  "mc-bamboo": { zh: "命令行入口", en: "Command-line entry point" },
  "mc-bamboo-server": { zh: "网络层：监听、连接状态机、登录", en: "Network layer: listener, connection state machine, login" },
  "mc-bamboo-game": { zh: "世界线程：tick、玩家、同步、交互", en: "World thread: ticks, players, sync, interaction" },
  "mc-bamboo-protocol": { zh: "协议：帧、数据包、物品组件", en: "Protocol: frames, packets, item components" },
  "mc-bamboo-world": { zh: "区块、调色板、光照、ticket 表", en: "Chunks, palettes, lighting, ticket table" },
  "mc-bamboo-worldgen": { zh: "世界生成的数学", en: "World generation maths" },
  "mc-bamboo-blocks": { zh: "方块行为、红石、方块实体", en: "Block behaviour, redstone, block entities" },
  "mc-bamboo-command": { zh: "命令树和参数解析", en: "Command tree and argument parsing" },
  "mc-bamboo-recipes": { zh: "配方查找和匹配", en: "Recipe lookup and matching" },
  "mc-bamboo-loot": { zh: "战利品表", en: "Loot tables" },
  "mc-bamboo-core": { zh: "共用的基础类型", en: "Shared basic types" },
  "mc-bamboo-auth": { zh: "正版验证和协议加密", en: "Authentication and protocol encryption" },
  "mc-bamboo-nbt": { zh: "NBT 编解码", en: "NBT codec" },
  "mc-bamboo-data": { zh: "版本数据（由官方服务端生成）", en: "Version data (generated from the official server)" },
  "mc-bamboo-storage": { zh: "存档编解码：区块 NBT、Anvil", en: "Save codecs: chunk NBT, Anvil" },
};

// ---- 测量。出处写在每一组里。 ----
// ---- Measurements. The source is noted in each group. ----

/** docs/profiling.md 5b：第三份基线，提交 89d4b27，超平坦世界，假玩家。 / docs/profiling.md 5b: the third baseline, commit 89d4b27, flat world, fake players. */
export const TICK_BASELINE = {
  budgetMs: 50,
  source: "profiling.md",
  scenarios: [
    { key: "idle", label: { zh: "1 人站着", en: "1 player standing" }, p50: 0.382, p99: 1.071 },
    { key: "crowd-50", label: { zh: "50 人扎堆游走", en: "50 wandering together" }, p50: 2.403, p99: 4.893 },
    { key: "walk-50", label: { zh: "50 人四散行走", en: "50 walking apart" }, p50: 3.189, p99: 5.591 },
    { key: "fly-20", label: { zh: "20 人四散飞行", en: "20 flying apart" }, p50: 4.281, p99: 6.815 },
    { key: "crowd-200", label: { zh: "200 人扎堆游走", en: "200 wandering together" }, p50: 6.466, p99: 9.612 },
  ],
} as const;

/** docs/world-model-design.md 8.6：3000 个掉落物加 300 个下落方块，release。 / docs/world-model-design.md 8.6: 3000 item entities plus 300 falling blocks, release build. */
export const ENTITY_SCALING = {
  source: "world-model-design.md",
  runs: [
    { threads: 1, ms: 4.22 },
    { threads: 2, ms: 2.67 },
    { threads: 4, ms: 1.39 },
    { threads: 8, ms: 1.05 },
  ],
} as const;

export interface Finding {
  readonly value: string;
  readonly unit: Text;
  readonly title: Text;
  readonly body: Text;
  readonly source: string;
}

export const FINDINGS: readonly Finding[] = [
  {
    value: "98.1–99.8",
    unit: same("%"),
    title: { zh: "红石缓存命中率", en: "Redstone cache hit rate" },
    body: {
      zh: "纯信号电路跑 4000 刻的实验结果。九台机器约 16.4 万次触发里，“键相同但效果不同”出现 0 次。",
      en: "Measured on pure-signal circuits over 4,000 ticks. Across nine machines and about 164,000 triggers, “same key, different effect” happened 0 times.",
    },
    source: "redstone-cache-experiment.md",
  },
  {
    value: "327",
    unit: { zh: "微秒 / 次", en: "µs per move" },
    title: { zh: "玩家跨一次区块边界", en: "A player crossing a chunk border" },
    body: {
      zh: "200 名玩家、视距 10、每刻每人跨一次。每次移动平均有 1053 个区块改变等级。",
      en: "With 200 players at view distance 10, each crossing once per tick. A move changes the level of 1,053 chunks on average.",
    },
    source: "chunk-tickets.md",
  },
  {
    value: "0",
    unit: { zh: "处不一致", en: "mismatches" },
    title: { zh: "ticket 表对拍", en: "Ticket table vs. official" },
    body: {
      zh: "四个种子、1700 个场景、18544 步，加载和模拟两套等级和官方 DistanceManager 逐位一致。",
      en: "Four seeds, 1,700 scenes and 18,544 steps: both the loading and simulation levels match the official DistanceManager bit for bit.",
    },
    source: "chunk-tickets.md",
  },
];

// ---- 验证。出处：README.md“项目结构”里 tools/mc-bamboo-vanilla 一行，以及合并记录。 ----
// ---- Verification. Sources: the tools/mc-bamboo-vanilla row of README.md and the merge history.

export const ORACLES: readonly Text[] = [
  { zh: "协议", en: "Protocol" },
  { zh: "方块行为", en: "Block behaviour" },
  { zh: "光照", en: "Lighting" },
  { zh: "菜单", en: "Menus" },
  { zh: "熔炉、酿造台、营火的逐刻状态", en: "Furnaces, brewing stands, campfires tick by tick" },
  { zh: "物品组件", en: "Item components" },
  { zh: "方块谓词", en: "Block predicates" },
  { zh: "配方", en: "Recipes" },
  { zh: "战利品表", en: "Loot tables" },
  { zh: "实体", en: "Entities" },
  { zh: "移动校验", en: "Movement checks" },
  { zh: "世界生成数学", en: "World generation maths" },
  { zh: "存档格式", en: "Save format" },
  { zh: "区块 ticket", en: "Chunk tickets" },
];
