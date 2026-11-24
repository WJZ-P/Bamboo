import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";

import {
  defaultSettings,
  getSnapshot,
  loadSettings,
  mergePerformancePreset,
  onPlayersChanged,
  onStateChanged,
  performancePresets,
  saveSettings,
  type PerformancePreset,
  type PerformancePresets,
  type ServerSettings,
  type ServerSnapshot,
  startServer,
  stopServer,
} from "../api/bamboo";

interface ServerContextValue {
  snapshot: ServerSnapshot | null;
  settings: ServerSettings | null;
  /** 出厂设置，设置页用它显示每一项的默认值。 */
  defaults: ServerSettings | null;
  /** 一键性能模式的推荐值和精确模式的值。 */
  presets: PerformancePresets | null;
  /** 正在启动或停止，这期间按钮不能再点。 */
  busy: boolean;
  error: string | null;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  updateSettings: (settings: ServerSettings) => Promise<void>;
  /** 以已保存的设置为基准套用一键预设并保存，返回保存后的完整设置。 */
  applyPerformancePreset: (preset: PerformancePreset) => Promise<ServerSettings>;
  dismissError: () => void;
}

const ServerContext = createContext<ServerContextValue | null>(null);

/** 把 Tauri 抛出的错误统一成可以显示的文字。 */
const errorText = (error: unknown) => (typeof error === "string" ? error : String(error));

export function ServerProvider({ children }: { children: ReactNode }) {
  const [snapshot, setSnapshot] = useState<ServerSnapshot | null>(null);
  const [settings, setSettings] = useState<ServerSettings | null>(null);
  const [defaults, setDefaults] = useState<ServerSettings | null>(null);
  const [presets, setPresets] = useState<PerformancePresets | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    const unlisteners = [
      onStateChanged(setSnapshot),
      onPlayersChanged((players) =>
        setSnapshot((current) => (current ? { ...current, players } : current)),
      ),
    ];

    getSnapshot()
      .then((value) => !disposed && setSnapshot(value))
      .catch((reason) => setError(errorText(reason)));
    loadSettings()
      .then((value) => !disposed && setSettings(value))
      .catch((reason) => setError(errorText(reason)));
    defaultSettings()
      .then((value) => !disposed && setDefaults(value))
      .catch((reason) => setError(errorText(reason)));
    performancePresets()
      .then((value) => !disposed && setPresets(value))
      .catch((reason) => setError(errorText(reason)));

    return () => {
      disposed = true;
      unlisteners.forEach((unlisten) => void unlisten.then((stop) => stop()));
    };
  }, []);

  const start = useCallback(async () => {
    if (!settings) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      setSnapshot(await startServer(settings));
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy(false);
    }
  }, [settings]);

  const stop = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setSnapshot(await stopServer());
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy(false);
    }
  }, []);

  const updateSettings = useCallback(async (next: ServerSettings) => {
    await saveSettings(next);
    setSettings(next);
  }, []);

  /**
   * 以上次保存的设置为基准合并预设，界面草稿里其它未保存的修改不参与。
   * Merge a preset on top of the last saved settings; other unsaved draft
   * edits are deliberately left out.
   */
  const applyPerformancePreset = useCallback(
    async (preset: PerformancePreset) => {
      if (!settings) {
        throw new Error("设置还没有加载完");
      }
      const merged = await mergePerformancePreset(settings, preset);
      await updateSettings(merged);
      return merged;
    },
    [settings, updateSettings],
  );

  const dismissError = useCallback(() => setError(null), []);

  const value = useMemo(
    () => ({
      snapshot,
      settings,
      defaults,
      presets,
      busy,
      error,
      start,
      stop,
      updateSettings,
      applyPerformancePreset,
      dismissError,
    }),
    [
      snapshot,
      settings,
      defaults,
      presets,
      busy,
      error,
      start,
      stop,
      updateSettings,
      applyPerformancePreset,
      dismissError,
    ],
  );

  return <ServerContext.Provider value={value}>{children}</ServerContext.Provider>;
}

export function useServer() {
  const value = useContext(ServerContext);
  if (!value) {
    throw new Error("useServer 必须在 ServerProvider 内部使用");
  }
  return value;
}
