import {
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import styled from "styled-components";

import { raisedBevel } from "../../theme/bevel";

/**
 * 上下两栏，中间一根可以拖的分隔条。
 *
 * - 拖动改变上栏高度；上栏拖到比 `minTop` 还矮就整个收起，下栏剩得比 `minBottom` 还少就把下栏收起。
 * - 双击分隔条恢复默认比例；键盘聚焦后用上下方向键微调，Home / End 收起上栏或下栏。
 * - 收起的那一栏用 `display: none` 藏起来（样式里的 `display: flex` 会盖过 `hidden` 属性），不卸载。
 * - 比例（上栏占比，0 表示收起上栏，1 表示收起下栏）存在 localStorage，读写失败就用默认值。
 *
 * Two stacked panes with a draggable divider. Dragging sets the top pane's height; dragging
 * it below `minTop` collapses the top pane, leaving less than `minBottom` collapses the
 * bottom one. Double-click restores the default; with keyboard focus the arrow keys nudge
 * and Home / End collapse a side. The ratio is kept in localStorage when available.
 */
type Props = {
  top: ReactNode;
  bottom: ReactNode;
  /** localStorage 的键。 */
  storageKey: string;
  /** 默认上栏占比。 */
  defaultRatio: number;
  minTop: number;
  minBottom: number;
  label: string;
};

const BAR = 12;

const Root = styled.div`
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
`;

const Pane = styled.div`
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
`;

/** 分隔条：石头色的凸起横条，中间三道握把；按住时变成竹绿。 */
const Bar = styled.div<{ $dragging: boolean }>`
  position: relative;
  flex: none;
  height: ${BAR}px;
  margin: 2px 0;
  cursor: row-resize;
  touch-action: none;
  background: ${({ theme, $dragging }) => ($dragging ? theme.color.bamboo.base : theme.color.stone.base)};
  border: 2px solid ${({ theme }) => theme.color.black};
  ${({ theme }) => raisedBevel(theme.color.stone.light, theme.color.stone.dark, 2)}

  &:hover {
    background: ${({ theme, $dragging }) => ($dragging ? theme.color.bamboo.base : theme.color.stone.light)};
  }

  &:focus-visible {
    outline: 2px solid ${({ theme }) => theme.color.panelLight};
    outline-offset: 2px;
  }

  &::after {
    content: "";
    position: absolute;
    top: 50%;
    left: 50%;
    width: 36px;
    height: 2px;
    transform: translate(-50%, -3px);
    background: ${({ theme }) => theme.color.stone.dark};
    box-shadow:
      0 2px 0 ${({ theme }) => theme.color.stone.light},
      0 4px 0 ${({ theme }) => theme.color.stone.dark};
  }
`;

const readRatio = (key: string, fallback: number) => {
  try {
    const raw = localStorage.getItem(key);
    const value = raw === null ? Number.NaN : Number(raw);
    return Number.isFinite(value) && value >= 0 && value <= 1 ? value : fallback;
  } catch {
    return fallback;
  }
};

const writeRatio = (key: string, value: number) => {
  try {
    localStorage.setItem(key, String(value));
  } catch {
    // 存不了就只在这次会话里生效。Without storage it only lasts this session.
  }
};

export function PixelSplit({
  top,
  bottom,
  storageKey,
  defaultRatio,
  minTop,
  minBottom,
  label,
}: Props) {
  const rootRef = useRef<HTMLDivElement>(null);
  const [ratio, setRatio] = useState(() => readRatio(storageKey, defaultRatio));
  const [height, setHeight] = useState(0);
  const [dragging, setDragging] = useState(false);

  // 容器高度变了（窗口缩放）要重新算像素。Recompute pixels when the container resizes.
  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const observer = new ResizeObserver(([entry]) => setHeight(entry.contentRect.height));
    observer.observe(root);
    return () => observer.disconnect();
  }, []);

  const space = Math.max(0, height - BAR - 4);

  /** 把想要的上栏像素高度换成比例，并按阈值吸附到收起。 */
  const apply = useCallback(
    (topPixels: number) => {
      if (space <= 0) return;
      let next = topPixels / space;
      if (topPixels < minTop) next = 0;
      else if (space - topPixels < minBottom) next = 1;
      setRatio(next);
      writeRatio(storageKey, next);
    },
    [space, minTop, minBottom, storageKey],
  );

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    setDragging(true);
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (!dragging || !rootRef.current) return;
    const rect = rootRef.current.getBoundingClientRect();
    apply(event.clientY - rect.top - BAR / 2);
  };

  const onPointerUp = (event: PointerEvent<HTMLDivElement>) => {
    event.currentTarget.releasePointerCapture(event.pointerId);
    setDragging(false);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = 24;
    const current = ratio * space;
    if (event.key === "ArrowUp") apply(current - step);
    else if (event.key === "ArrowDown") apply(current + step);
    else if (event.key === "Home") apply(0);
    else if (event.key === "End") apply(space);
    else return;
    event.preventDefault();
  };

  const reset = () => {
    setRatio(defaultRatio);
    writeRatio(storageKey, defaultRatio);
  };

  const topPixels = Math.round(ratio * space);
  return (
    <Root ref={rootRef}>
      <Pane
        style={{ height: topPixels, display: ratio === 0 ? "none" : undefined }}
        aria-hidden={ratio === 0}
      >
        {top}
      </Pane>
      <Bar
        role="separator"
        aria-orientation="horizontal"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(ratio * 100)}
        tabIndex={0}
        $dragging={dragging}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onDoubleClick={reset}
        onKeyDown={onKeyDown}
        title="拖动调整上下比例，双击恢复默认"
      />
      <Pane style={{ flex: 1, display: ratio === 1 ? "none" : undefined }} aria-hidden={ratio === 1}>
        {bottom}
      </Pane>
    </Root>
  );
}
