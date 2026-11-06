import { useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import styled from "styled-components";

import type { LogLevel } from "../api/bamboo";
import { LogRow } from "../components/LogRow";
import { PixelButton, PixelPanel, PixelSwitch, SunkenBox } from "../components/pixel";
import { logStore } from "../state/logStore";

type LevelFilter = "ALL" | "WARN" | "ERROR";

const FILTERS: { id: LevelFilter; label: string; levels: LogLevel[] | null }[] = [
  { id: "ALL", label: "全部", levels: null },
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
    <Panel>
      <Toolbar>
        {FILTERS.map(({ id, label }) => (
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
  );
}
