import styled from "styled-components";

export interface BarRow {
  readonly key: string;
  readonly label: string;
  /** 标签下面的一行小字说明。 / A line of small print under the label. */
  readonly caption?: string;
  readonly value: number;
  /** 写在条形末端的数值文字。 / The value text written at the end of the bar. */
  readonly display: string;
  /** 同一根轴上的第二个量，画成一道竖线。 / A second quantity on the same axis, drawn as a tick. */
  readonly marker?: number;
  /** 鼠标悬停时显示的说明。 / The explanation shown on hover. */
  readonly detail?: string;
}

interface BarChartProps {
  readonly rows: readonly BarRow[];
  /** 轴的最大值。所有条形共用这一根轴。 / The axis maximum. Every bar shares this one axis. */
  readonly max: number;
  readonly labelHeader: string;
  readonly valueHeader: string;
  /** 轴两端的文字，比如“0”和“50 ms”。 / The text at each end of the axis, such as "0" and "50 ms". */
  readonly axis?: readonly [string, string];
  readonly labelWidth?: number;
}

const Table = styled.table`
  width: 100%;
  border-collapse: collapse;
  table-layout: fixed;
`;

/**
 * 表头压成零高度：读屏软件照常读到列名，眼睛看不到。
 * table-layout: fixed 按第一行定列宽，所以列宽也写在这一行上。
 *
 * The header row is squashed to zero height: screen readers still announce the column names
 * while nothing shows. With `table-layout: fixed` the first row sets the column widths, so
 * the widths live on this row too.
 */
const HeadCell = styled.th<{ $width?: number; $narrowWidth?: number }>`
  ${({ $width }) => ($width === undefined ? "" : `width: ${$width}px;`)}
  height: 0;
  padding: 0;
  border: 0;
  font-size: 0;
  line-height: 0;
  overflow: hidden;

  @media (max-width: 640px) {
    ${({ $narrowWidth }) => ($narrowWidth === undefined ? "" : `width: ${$narrowWidth}px;`)}
  }
`;

const VALUE_WIDTH = 104;
const NARROW_VALUE_WIDTH = 84;
const narrow = (width: number) => Math.round(width * 0.62);

const Row = styled.tr`
  position: relative;

  &:hover td {
    background: rgba(255, 255, 255, 0.06);
  }
`;

const LabelCell = styled.th`
  padding: 6px 12px 6px 6px;
  font-weight: normal;
  text-align: left;
  vertical-align: middle;
  line-height: 1.3;
  color: ${({ theme }) => theme.color.text};
  overflow-wrap: anywhere;

  small {
    display: block;
    font-size: ${({ theme }) => theme.font.caption};
    line-height: 1.4;
    color: ${({ theme }) => theme.color.textMuted};
  }

  @media (max-width: 640px) {
    padding-right: 8px;

    small {
      display: none;
    }
  }
`;

const BarCell = styled.td`
  position: relative;
  padding: 6px 0;
  vertical-align: middle;
`;

const Track = styled.div`
  position: relative;
  height: 14px;
  background: ${({ theme }) => theme.chart.track};
`;

const Fill = styled.div`
  position: absolute;
  inset: 0 auto 0 0;
  min-width: 2px;
  background: ${({ theme }) => theme.chart.bar};
  box-shadow:
    inset 0 3px 0 0 rgba(255, 255, 255, 0.22),
    inset 0 -3px 0 0 rgba(0, 0, 0, 0.25);
`;

const Marker = styled.div`
  position: absolute;
  top: -4px;
  bottom: -4px;
  width: 3px;
  margin-left: -1px;
  background: ${({ theme }) => theme.color.text};
  box-shadow: 0 0 0 1px ${({ theme }) => theme.chart.surface};
`;

const ValueCell = styled.td`
  padding: 6px 6px 6px 12px;
  text-align: right;
  vertical-align: middle;
  white-space: nowrap;
  color: ${({ theme }) => theme.color.text};

  @media (max-width: 640px) {
    padding-left: 8px;
  }
`;

/** 悬停提示，样子照着游戏里的物品提示框：深紫底、紫色描边。 / The hover tip, styled after the in-game item tooltip: dark purple with a violet edge. */
const Tip = styled.div`
  position: absolute;
  z-index: 5;
  left: 8px;
  bottom: calc(100% - 2px);
  display: none;
  max-width: 320px;
  padding: 4px 10px;
  border: 2px solid #2d0a63;
  background: rgba(16, 0, 16, 0.96);
  box-shadow: 0 0 0 2px rgba(16, 0, 16, 0.96);
  color: #ffffff;
  line-height: 1.4;
  pointer-events: none;

  ${Row}:hover & {
    display: block;
  }
`;

const AxisRow = styled.div<{ $labelWidth: number }>`
  display: flex;
  justify-content: space-between;
  margin: 2px ${VALUE_WIDTH}px 0 ${({ $labelWidth }) => $labelWidth}px;
  padding-top: 4px;
  border-top: 2px solid ${({ theme }) => theme.chart.grid};
  font-size: ${({ theme }) => theme.font.caption};
  line-height: 1.4;
  color: ${({ theme }) => theme.color.textMuted};

  @media (max-width: 640px) {
    margin: 2px ${NARROW_VALUE_WIDTH}px 0 ${({ $labelWidth }) => narrow($labelWidth)}px;
  }
`;

/**
 * 横向条形图，本身是一张表：每行是“名称、条形、数值”。所有条形共用一根从零开始的轴，
 * 只用一种颜色，数值直接写在条形旁边。
 *
 * A horizontal bar chart that is itself a table: each row is label, bar and value. Every bar
 * shares one zero-based axis and one colour, and the value is written beside the bar.
 */
export function BarChart({ rows, max, labelHeader, valueHeader, axis, labelWidth = 200 }: BarChartProps) {
  const percent = (value: number) => `${Math.min(100, (value / max) * 100).toFixed(2)}%`;
  return (
    <div>
      <Table>
        <thead>
          <tr>
            <HeadCell scope="col" $width={labelWidth} $narrowWidth={narrow(labelWidth)}>
              {labelHeader}
            </HeadCell>
            <HeadCell scope="col" />
            <HeadCell scope="col" $width={VALUE_WIDTH} $narrowWidth={NARROW_VALUE_WIDTH}>
              {valueHeader}
            </HeadCell>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <Row key={row.key}>
              <LabelCell scope="row">
                {row.label}
                {row.caption && <small>{row.caption}</small>}
              </LabelCell>
              <BarCell>
                <Track>
                  <Fill style={{ width: percent(row.value) }} />
                  {row.marker !== undefined && <Marker style={{ left: percent(row.marker) }} />}
                </Track>
                {row.detail && <Tip role="tooltip">{row.detail}</Tip>}
              </BarCell>
              <ValueCell>{row.display}</ValueCell>
            </Row>
          ))}
        </tbody>
      </Table>
      {axis && (
        <AxisRow $labelWidth={labelWidth} aria-hidden="true">
          <span>{axis[0]}</span>
          <span>{axis[1]}</span>
        </AxisRow>
      )}
    </div>
  );
}
