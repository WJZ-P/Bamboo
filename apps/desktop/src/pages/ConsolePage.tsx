import { useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import styled from "styled-components";

import type { LogLevel } from "../api/bamboo";
import { LogRow } from "../components/LogRow";
import { PerformanceStrip } from "../components/PerformanceStrip";
import { PixelButton, PixelPanel, PixelSplit, PixelSwitch, SunkenBox } from "../components/pixel";
import { logStore } from "../state/logStore";

type LevelFilter = "ALL" | "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR";

interface FilterTier {
  id: LevelFilter;
  label: string;
  /** 这一档留下哪些级别；null 是全部。 / The levels this tier keeps; null keeps all. */
  levels: LogLevel[] | null;
  /**
   * 只有服务端实际输出过这些级别之一时才显示这一档；不写就一直显示。
   * Shown only once the server has actually emitted one of these levels; always shown when absent.
   */
  onlyWith?: LogLevel[];
}

// 「仅 TRACE」「仅 DEBUG」和「信息及以上」三档只在服务端真的输出过 debug / trace 时才出现
// （日志级别在「设置 → 日志」里调）；缺省 info 下筛选栏和原来一样。
// The "TRACE only", "DEBUG only" and "INFO and above" tiers appear only once the
// server has really emitted debug / trace lines (the level is under Settings →
// Logging); at the default info level the toolbar is what it used to be.
const FILTERS: FilterTier[] = [
  { id: "ALL", label: "全部", levels: null },
  { id: "TRACE", label: "仅 TRACE", levels: ["TRACE"], onlyWith: ["TRACE"] },
  { id: "DEBUG", label: "仅 DEBUG", levels: ["DEBUG"], onlyWith: ["DEBUG"] },
  {
    id: "INFO",
    label: "信息及以上",
    levels: ["INFO", "WARN", "ERROR"],
    onlyWith: ["DEBUG", "TRACE"],
  },
  { id: "WARN", label: "警告及以上", levels: ["WARN", "ERROR"] },
  { id: "ERROR", label: "仅错误", levels: ["ERROR"] },
];

const Panel = styled(PixelPanel)`
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
`;

const Toolbar = styled.div`
  display: flex;
  align-items: center;
  gap: 8px;
`;

const Spacer = styled.div`
  flex: 1;
`;

const Log = styled(SunkenBox)`
  flex: 1;
  min-height: 0;
  padding: 8px 10px;
  overflow: auto;
  line-height: 1.6;
  user-select: text;
`;

const Empty = styled.p`
  margin: 0;
  color: ${({ theme }) => theme.color.textMuted};
`;

export function ConsolePage() {
  const lines = useSyncExternalStore(logStore.subscribe, logStore.getSnapshot);
  const [filter, setFilter] = useState<LevelFilter>("ALL");
  const [followTail, setFollowTail] = useState(true);
  const logRef = useRef<HTMLDivElement>(null);

  const visibleLines = useMemo(() => {
    const levels = FILTERS.find(({ id }) => id === filter)?.levels;
    return levels ? lines.filter((line) => levels.includes(line.level)) : lines;
  }, [lines, filter]);

  // 正选着的那一档一直留着，免得日志滚走之后按钮自己消失。
  // The selected tier always stays, so its button does not vanish when the
  // lines scroll away.
  const shownFilters = useMemo(() => {
    const present = new Set(lines.map((line) => line.level));
    return FILTERS.filter(
      ({ id, onlyWith }) =>
        id === filter || !onlyWith || onlyWith.some((level) => present.has(level)),
    );
  }, [lines, filter]);

  useLayoutEffect(() => {
    const log = logRef.current;
    if (followTail && log) {
      log.scrollTop = log.scrollHeight;
    }
  }, [visibleLines, followTail]);

  // 切换页面回来时也要落到底部。
  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, []);

  return (
    <PixelSplit
      storageKey="bamboo.console.split"
      defaultRatio={0.28}
      minTop={110}
      minBottom={140}
      label="调整资源监控和控制台的高度"
      top={<PerformanceStrip />}
      bottom={
          <Panel>
            <Toolbar>
              {shownFilters.map(({ id, label }) => (
                <PixelButton
                  key={id}
                  type="button"
                  $size="small"
                  $variant={id === filter ? "bamboo" : "stone"}
                  aria-pressed={id === filter}
                  onClick={() => setFilter(id)}
                >
                  {label}
                </PixelButton>
              ))}
              <Spacer />
              <PixelSwitch checked={followTail} label="跟随最新" onChange={setFollowTail} />
              <PixelButton type="button" $size="small" onClick={logStore.clear}>
                清空
              </PixelButton>
            </Toolbar>
            <Log ref={logRef} role="log">
              {visibleLines.length === 0 ? (
                <Empty>还没有日志。启动服务器后，这里会显示运行记录。</Empty>
              ) : (
                visibleLines.map((line) => <LogRow key={line.id} line={line} />)
              )}
            </Log>
          </Panel>
      }
    />
  );
}
