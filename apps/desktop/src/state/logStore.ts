import { type LogLine, onLogBatch, recentLogs } from "../api/bamboo";

const MAX_LINES = 1000;

let lines: LogLine[] = [];
const listeners = new Set<() => void>();

function append(batch: LogLine[]) {
  // 历史日志和实时日志可能重叠，靠递增的 id 去重。
  const lastId = lines.at(-1)?.id ?? -1;
  const fresh = batch.filter((line) => line.id > lastId);
  if (fresh.length === 0) {
    return;
  }
  lines = [...lines, ...fresh].slice(-MAX_LINES);
  listeners.forEach((listener) => listener());
}

/**
 * 日志放在 React 之外，只有控制台页面会订阅。
 * 日志是高频数据，放进顶层 state 会让整棵组件树跟着重渲染。
 */
export const logStore = {
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },
  getSnapshot: () => lines,
  clear() {
    lines = [];
    listeners.forEach((listener) => listener());
  },
};

/** 应用启动时调用一次：先接上实时日志，再补齐历史日志。 */
export async function startLogStream() {
  await onLogBatch(append);
  append(await recentLogs());
}
