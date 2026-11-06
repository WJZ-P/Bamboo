import { memo } from "react";
import styled from "styled-components";

import type { LogLevel, LogLine } from "../api/bamboo";

const Row = styled.div`
  white-space: pre-wrap;
  word-break: break-all;
`;

const Time = styled.span`
  color: ${({ theme }) => theme.color.textMuted};
`;

const Level = styled.span<{ $level: LogLevel }>`
  display: inline-block;
  width: 44px;
  margin: 0 8px;
  color: ${({ theme, $level }) => theme.log[$level]};
`;

const timeFormat = new Intl.DateTimeFormat("zh-CN", {
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hour12: false,
});

/** 单行日志不可变，用 memo 避免新日志到来时旧行重新渲染。 */
export const LogRow = memo(function LogRow({ line }: { line: LogLine }) {
  return (
    <Row>
      <Time>{timeFormat.format(line.timestampMs)}</Time>
      <Level $level={line.level}>{line.level}</Level>
      {line.message}
    </Row>
  );
});
