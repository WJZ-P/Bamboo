import { type ReactNode, useSyncExternalStore } from "react";
import styled, { useTheme } from "styled-components";

import type { MetricsSample } from "../api/bamboo";
import { HISTORY_LENGTH, metricsStore } from "../state/metricsStore";
import { SunkenBox } from "./pixel";

const Strip = styled.div`
  display: grid;
  flex: none;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
`;

const Card = styled(SunkenBox)`
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
  padding: 10px 12px;
`;

const Header = styled.div`
  display: flex;
  align-items: baseline;
  gap: 8px;
`;

const Name = styled.span`
  font-size: ${({ theme }) => theme.font.nav};
`;

const Value = styled.span<{ $color: string }>`
  margin-left: auto;
  color: ${({ $color }) => $color};
  font-size: ${({ theme }) => theme.font.title};
  line-height: 1.25;
  text-shadow: 2px 2px 0 ${({ theme }) => theme.color.black};
  white-space: nowrap;
`;

const AxisRow = styled.div`
  display: flex;
  justify-content: space-between;
  gap: 8px;
  color: ${({ theme }) => theme.color.textMuted};
  font-size: ${({ theme }) => theme.font.small};
  line-height: 1.25;
  white-space: nowrap;
`;

const Legend = styled.span`
  overflow: hidden;
  text-overflow: ellipsis;
`;

const Swatch = styled.span<{ $color: string }>`
  display: inline-block;
  width: 8px;
  height: 8px;
  margin-right: 4px;
  background: ${({ $color }) => $color};
`;

const LegendItem = styled.span`
  & + & {
    margin-left: 12px;
  }
`;

const ChartFrame = styled.div`
  height: 64px;
  border: 2px solid ${({ theme }) => theme.chart.grid};
`;

const Svg = styled.svg`
  display: block;
  width: 100%;
  height: 100%;
  shape-rendering: crispEdges;
`;

const MIB = 1024 ** 2;
const GIB = 1024 ** 3;

function formatPercent(value: number) {
  return `${value < 10 ? value.toFixed(1) : Math.round(value)}%`;
}

function formatBytes(bytes: number) {
  return bytes >= GIB ? `${(bytes / GIB).toFixed(1)} GB` : `${Math.round(bytes / MIB)} MB`;
}

/** 和任务管理器一样写成 `20.4/30.9 GB（66%）`。 */
function formatSystemMemory(used: number, total: number) {
  const percent = Math.round((used / total) * 100);
  return `${(used / GIB).toFixed(1)}/${(total / GIB).toFixed(1)} GB（${percent}%）`;
}

/** 内存曲线的上限：从 64 MB 起翻倍，直到比最近 60 秒的最大值高出一截，曲线不会顶到边。 */
function memoryScale(values: number[]) {
  const peak = Math.max(0, ...values) * 1.1;
  let scale = 64 * MIB;
  while (scale < peak) {
    scale *= 2;
  }
  return scale;
}

interface Series {
  values: number[];
  stroke: string;
  /** 有填充色时画成面积图，没有时只画线。 */
  fill?: string;
}

/**
 * 每个采样占一格宽的阶梯曲线，最新的在最右边，和任务管理器一样从右往左流。
 * 坐标系是 60 × 100，拉伸到图框大小；线宽不跟着拉伸，保持 2 像素。
 */
function stepPath(values: number[], max: number) {
  const offset = HISTORY_LENGTH - values.length;
  const points = values.flatMap((value, index) => {
    const y = 100 - Math.min(Math.max(value / max, 0), 1) * 100;
    return [`${offset + index} ${y}`, `${offset + index + 1} ${y}`];
  });
  const line = `M ${points.join(" L ")}`;
  return { line, area: `${line} L ${HISTORY_LENGTH} 100 L ${offset} 100 Z` };
}

const GRID_COLUMNS = [6, 12, 18, 24, 30, 36, 42, 48, 54];
const GRID_ROWS = [25, 50, 75];

function Chart({ series, max }: { series: Series[]; max: number }) {
  const theme = useTheme();
  return (
    <ChartFrame>
      <Svg viewBox={`0 0 ${HISTORY_LENGTH} 100`} preserveAspectRatio="none" aria-hidden>
        {GRID_COLUMNS.map((x) => (
          <line
            key={`x${x}`}
            x1={x}
            x2={x}
            y1={0}
            y2={100}
            stroke={theme.chart.grid}
            vectorEffect="non-scaling-stroke"
          />
        ))}
        {GRID_ROWS.map((y) => (
          <line
            key={`y${y}`}
            x1={0}
            x2={HISTORY_LENGTH}
            y1={y}
            y2={y}
            stroke={theme.chart.grid}
            vectorEffect="non-scaling-stroke"
          />
        ))}
        {series
          .filter(({ values }) => values.length > 0)
          .map(({ values, stroke, fill }) => {
            const { line, area } = stepPath(values, max);
            return (
              <g key={stroke}>
                {fill && <path d={area} fill={fill} />}
                <path
                  d={line}
                  fill="none"
                  stroke={stroke}
                  strokeWidth={2}
                  vectorEffect="non-scaling-stroke"
                />
              </g>
            );
          })}
      </Svg>
    </ChartFrame>
  );
}

function MetricCard(props: {
  name: string;
  value: string;
  color: string;
  legend: ReactNode;
  scale: string;
  series: Series[];
  max: number;
}) {
  return (
    <Card role="group" aria-label={`${props.name}：${props.value}`}>
      <Header>
        <Name>{props.name}</Name>
        <Value $color={props.color}>{props.value}</Value>
      </Header>
      <AxisRow>
        <Legend>{props.legend}</Legend>
        <span>{props.scale}</span>
      </AxisRow>
      <Chart series={props.series} max={props.max} />
      <AxisRow>
        <span>60 秒</span>
        <span>0</span>
      </AxisRow>
    </Card>
  );
}

/** 控制台顶部的 CPU 和内存监控。数据来自 Rust 每秒一次的采样，只算 Bamboo 所在的这个进程。 */
export function PerformanceStrip() {
  const samples = useSyncExternalStore(metricsStore.subscribe, metricsStore.getSnapshot);
  const theme = useTheme();
  const latest: MetricsSample | undefined = samples.at(-1);
  const processMemory = samples.map((sample) => sample.processMemory);
  const memoryMax = memoryScale(processMemory);

  return (
    <Strip>
      <MetricCard
        name="CPU"
        value={latest ? formatPercent(latest.processCpu) : "--"}
        color={theme.chart.cpu}
        legend={
          <>
            <LegendItem>
              <Swatch $color={theme.chart.cpu} />
              Bamboo 进程
            </LegendItem>
            <LegendItem>
              <Swatch $color={theme.chart.system} />
              整机 {latest ? formatPercent(latest.systemCpu) : "--"}
            </LegendItem>
          </>
        }
        scale="100%"
        max={100}
        series={[
          {
            values: samples.map((sample) => sample.processCpu),
            stroke: theme.chart.cpu,
            fill: theme.chart.cpuFill,
          },
          { values: samples.map((sample) => sample.systemCpu), stroke: theme.chart.system },
        ]}
      />
      <MetricCard
        name="内存"
        value={latest ? formatBytes(latest.processMemory) : "--"}
        color={theme.chart.memory}
        legend={
          <>
            <LegendItem>
              <Swatch $color={theme.chart.memory} />
              Bamboo 工作集
            </LegendItem>
            <LegendItem>
              整机{" "}
              {latest ? formatSystemMemory(latest.systemMemoryUsed, latest.systemMemoryTotal) : "--"}
            </LegendItem>
          </>
        }
        scale={formatBytes(memoryMax)}
        max={memoryMax}
        series={[
          { values: processMemory, stroke: theme.chart.memory, fill: theme.chart.memoryFill },
        ]}
      />
    </Strip>
  );
}
