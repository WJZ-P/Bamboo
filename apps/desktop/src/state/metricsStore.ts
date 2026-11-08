import { type MetricsSample, onMetrics, recentMetrics } from "../api/bamboo";

/** 曲线显示最近 60 秒，每秒一个采样。 */
export const HISTORY_LENGTH = 60;

let samples: MetricsSample[] = [];
const listeners = new Set<() => void>();

function append(batch: MetricsSample[]) {
  // 历史采样可能比第一条实时采样晚到，按时间戳合并去重，不能只接在末尾。
  const known = new Set(samples.map((sample) => sample.timestampMs));
  const fresh = batch.filter((sample) => !known.has(sample.timestampMs));
  if (fresh.length === 0) {
    return;
  }
  samples = [...samples, ...fresh]
    .sort((a, b) => a.timestampMs - b.timestampMs)
    .slice(-HISTORY_LENGTH);
  listeners.forEach((listener) => listener());
}

/** 性能采样和日志一样放在 React 之外，只有控制台页面会订阅。 */
export const metricsStore = {
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },
  getSnapshot: () => samples,
};

/** 应用启动时调用一次：先接上实时采样，再补齐历史采样。 */
export async function startMetricsStream() {
  await onMetrics((sample) => append([sample]));
  append(await recentMetrics());
}
