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

/** 服务端的全部可调选项。字段和 `src-tauri` 的 `ServerSettings` 一一对应。 */
export interface ServerSettings {
  host: string;
  port: number;
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
