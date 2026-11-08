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
  onPlayersChanged,
  onStateChanged,
  saveSettings,
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
  /** 正在启动或停止，这期间按钮不能再点。 */
  busy: boolean;
  error: string | null;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  updateSettings: (settings: ServerSettings) => Promise<void>;
  dismissError: () => void;
}

const ServerContext = createContext<ServerContextValue | null>(null);

/** 把 Tauri 抛出的错误统一成可以显示的文字。 */
const errorText = (error: unknown) => (typeof error === "string" ? error : String(error));

export function ServerProvider({ children }: { children: ReactNode }) {
  const [snapshot, setSnapshot] = useState<ServerSnapshot | null>(null);
  const [settings, setSettings] = useState<ServerSettings | null>(null);
  const [defaults, setDefaults] = useState<ServerSettings | null>(null);
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

  const dismissError = useCallback(() => setError(null), []);

  const value = useMemo(
    () => ({
      snapshot,
      settings,
      defaults,
      busy,
      error,
      start,
      stop,
      updateSettings,
      dismissError,
    }),
    [snapshot, settings, defaults, busy, error, start, stop, updateSettings, dismissError],
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
