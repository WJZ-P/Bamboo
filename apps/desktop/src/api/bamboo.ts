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

export interface ServerSettings {
  host: string;
  port: number;
  motd: string;
  maxPlayers: number;
  viewDistance: number;
}

export type LogLevel = "ERROR" | "WARN" | "INFO" | "DEBUG" | "TRACE";

export interface LogLine {
  id: number;
  timestampMs: number;
  level: LogLevel;
  target: string;
  message: string;
}

export const getSnapshot = () => invoke<ServerSnapshot>("get_snapshot");
export const startServer = (settings: ServerSettings) =>
  invoke<ServerSnapshot>("start_server", { settings });
export const stopServer = () => invoke<ServerSnapshot>("stop_server");
export const loadSettings = () => invoke<ServerSettings>("load_settings");
export const saveSettings = (settings: ServerSettings) =>
  invoke<void>("save_settings", { settings });
export const recentLogs = () => invoke<LogLine[]>("recent_logs");

export const onStateChanged = (handler: (snapshot: ServerSnapshot) => void): Promise<UnlistenFn> =>
  listen<ServerSnapshot>("bamboo://state", (event) => handler(event.payload));

export const onPlayersChanged = (handler: (players: PlayerView[]) => void): Promise<UnlistenFn> =>
  listen<PlayerView[]>("bamboo://players", (event) => handler(event.payload));

export const onLogBatch = (handler: (lines: LogLine[]) => void): Promise<UnlistenFn> =>
  listen<LogLine[]>("bamboo://log", (event) => handler(event.payload));
