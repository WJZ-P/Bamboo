import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** 这些类型和 Rust 侧 `src-tauri` 里序列化的结构一一对应。 */
export interface PlayerView {
  name: string;
  uuid: string;
}

export interface ServerSnapshot {
  running: boolean;
  address: string | null;
  startedAtMs: number | null;
  maxPlayers: number | null;
  players: PlayerView[];
  gameVersion: string;
  protocolVersion: number;
}

/** 从代理接收玩家信息的方式。 */
export type Forwarding = "off" | "legacy" | "modern";

/** 新建世界用的世界预设，和官方 `level-type` 的四个注册名一一对应。 */
export type WorldType = "normal" | "flat" | "largeBiomes" | "amplified";

/** 日志级别，和 `bamboo.toml` 的 `log_level` 一一对应。 */
export type LogLevelName = "error" | "warn" | "info" | "debug" | "trace";

/** 服务端的全部可调选项。字段和 `src-tauri` 的 `ServerSettings` 一一对应。 */
export interface ServerSettings {
  /** 服务器目录，只读展示；APP 和命令行版共用这里的两个配置文件。 */
  serverDirectory: string;
  host: string;
  port: number;
  levelName: string;
  motd: string;
  maxPlayers: number;
  viewDistance: number;
  prePlayTimeoutSeconds: number;
  maxPrePlayConnections: number;
  keepAliveSeconds: number;
  compressionThreshold: number;
  onlineMode: boolean;
  preventProxyConnections: boolean;
  enforceSecureProfile: boolean;
  forwarding: Forwarding;
  forwardingSecret: string;
  /** 世界生成种子，空串表示新建世界时随机取一个。只对新建的世界生效。 */
  levelSeed: string;
  /** 新建世界的世界类型。已有存档按存档里的类型继续生成。 */
  levelType: WorldType;
  /** 新建世界要不要生成结构。 */
  generateStructures: boolean;
  /** 超平坦预设的 JSON 原文，只在 levelType 是 flat 时生效。 */
  generatorSettings: string;
  entityThreads: number;
  /** 每只生物每刻最多处理几次推挤，0 = 不限（精确模式）。 */
  maxEntityCollisions: number;
  /** 三档降频的降频档起点（格），0 = 不启用。 */
  mobThrottleDistance: number;
  /** 三档降频的休眠档起点（格），0 = 不启用。 */
  mobSleepDistance: number;
  /** 休眠生物每刻唤醒名额的倍率，1 = Paper 缺省。 */
  mobWakeupsScale: number;
  /** 独立寻路阶段（实验）：结果晚一刻交给生物，转向最多慢 0.05 秒。 */
  mobPathfindingPhase: boolean;
  /** 每个岛每刻最多展开的 A* 节点数，0 = 65536。只在寻路阶段打开时生效。 */
  mobPathfindingBudget: number;
  /** 寻路失败冷却（实验）：连续失败的生物歇 2 秒再算。只在寻路阶段打开时生效。 */
  mobPathfindingFailCooldown: boolean;
  /** 被困村民降频（Purpur lobotomize）：四面受困的村民按间隔运行大脑。 */
  villagerLobotomize: boolean;
  /** 被困村民大脑运行间隔（刻），推荐 100。 */
  villagerLobotomizeInterval: number;
  /** 为真时只对已锁定交易（经验 > 0）的被困村民降频。 */
  villagerLobotomizeWaitTradeLock: boolean;
  /** 睡觉村民大脑运行间隔（刻），0 = 关闭，推荐 5。 */
  villagerSleepBrainInterval: number;
  /** 红石缓存（实验），设置页里可以开关。 */
  redstoneCache: boolean;
  chunkWorkerThreads: number;
  /** 世界生成缓存：地形 / 地物上限（MiB）和空闲释放秒数。 */
  worldgenTerrainCacheMb: number;
  worldgenFeatureCacheMb: number;
  worldgenCacheIdleSeconds: number;
  deliveriesPerTick: number;
  reclaimsPerTick: number;
  islandThreads: number;
  islands: boolean;
  /** 全局日志级别，写进 bamboo.toml 的 log_level；RUST_LOG 环境变量优先。 */
  logLevel: LogLevelName;
  /** 追加的 EnvFilter 指令，按模块细调级别，写进 bamboo.toml 的 log_filter。 */
  logFilter: string;
}

/** 一键性能模式的一组值，字段和 Rust 侧 `PerformancePreset` 一一对应。 */
export interface PerformancePreset {
  entityThreads: number;
  maxEntityCollisions: number;
  mobThrottleDistance: number;
  mobSleepDistance: number;
  mobWakeupsScale: number;
  mobPathfindingPhase: boolean;
  mobPathfindingBudget: number;
  mobPathfindingFailCooldown: boolean;
  villagerLobotomize: boolean;
  villagerLobotomizeInterval: number;
  villagerLobotomizeWaitTradeLock: boolean;
  villagerSleepBrainInterval: number;
}

/** 推荐值和恢复原版精确模式的两套值，都由 Rust 侧集中定义。 */
export interface PerformancePresets {
  recommended: PerformancePreset;
  exact: PerformancePreset;
}

export type LogLevel = "ERROR" | "WARN" | "INFO" | "DEBUG" | "TRACE";

export interface LogLine {
  id: number;
  timestampMs: number;
  level: LogLevel;
  target: string;
  message: string;
}

/** 一次性能采样，每秒一次。CPU 都按整机算，0 到 100；内存是字节。 */
export interface MetricsSample {
  timestampMs: number;
  processCpu: number;
  systemCpu: number;
  /** Bamboo 进程的工作集。 */
  processMemory: number;
  systemMemoryUsed: number;
  systemMemoryTotal: number;
  /** 逻辑处理器个数。 */
  cpuCount: number;
}

export const getSnapshot = () => invoke<ServerSnapshot>("get_snapshot");
export const startServer = (settings: ServerSettings) =>
  invoke<ServerSnapshot>("start_server", { settings });
export const stopServer = () => invoke<ServerSnapshot>("stop_server");
export const loadSettings = () => invoke<ServerSettings>("load_settings");
export const saveSettings = (settings: ServerSettings) =>
  invoke<void>("save_settings", { settings });
export const defaultSettings = () => invoke<ServerSettings>("default_settings");
/** 一键性能模式的推荐值和恢复原版精确模式的值。 */
export const performancePresets = () => invoke<PerformancePresets>("performance_presets");
/** 以已保存的设置为基准套用预设，结果只改那八项；其它字段原样保留。 */
export const mergePerformancePreset = (saved: ServerSettings, preset: PerformancePreset) =>
  invoke<ServerSettings>("merge_performance_preset", { saved, preset });
export const recentLogs = () => invoke<LogLine[]>("recent_logs");
export const recentMetrics = () => invoke<MetricsSample[]>("recent_metrics");

export const onStateChanged = (handler: (snapshot: ServerSnapshot) => void): Promise<UnlistenFn> =>
  listen<ServerSnapshot>("bamboo://state", (event) => handler(event.payload));

export const onPlayersChanged = (handler: (players: PlayerView[]) => void): Promise<UnlistenFn> =>
  listen<PlayerView[]>("bamboo://players", (event) => handler(event.payload));

export const onLogBatch = (handler: (lines: LogLine[]) => void): Promise<UnlistenFn> =>
  listen<LogLine[]>("bamboo://log", (event) => handler(event.payload));

export const onMetrics = (handler: (sample: MetricsSample) => void): Promise<UnlistenFn> =>
  listen<MetricsSample>("bamboo://metrics", (event) => handler(event.payload));
