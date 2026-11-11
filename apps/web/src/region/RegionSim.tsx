import { useEffect, useRef } from "react";
import styled, { useTheme } from "styled-components";

import { useLocale, type Locale } from "../i18n";
import { textShadow } from "../theme/bevel";
import { GRID, RegionSimulation, type SimRegion } from "./simulation";

const WIDTH = GRID.columns * GRID.cell;
const HEIGHT = GRID.rows * GRID.cell;
const MAX_LABELS = 6;

const Frame = styled.div`
  position: relative;
  width: 100%;
  aspect-ratio: ${WIDTH} / ${HEIGHT};
  overflow: hidden;
`;

const Canvas = styled.canvas`
  display: block;
  width: 100%;
  height: 100%;
`;

const Label = styled.span`
  position: absolute;
  top: 0;
  left: 0;
  display: none;
  padding: 0 6px;
  background: rgba(16, 16, 16, 0.85);
  color: #ffffff;
  font-size: ${({ theme }) => theme.font.caption};
  line-height: 1.5;
  white-space: nowrap;
  pointer-events: none;
  ${textShadow("#000000", 1)}

  &::before {
    content: "";
    display: inline-block;
    width: 8px;
    height: 8px;
    margin-right: 6px;
    background: var(--region-color);
  }
`;

const Clock = styled.span`
  position: absolute;
  right: 8px;
  bottom: 6px;
  padding: 0 6px;
  background: rgba(16, 16, 16, 0.85);
  color: ${({ theme }) => theme.color.textMuted};
  font-size: ${({ theme }) => theme.font.caption};
  line-height: 1.5;
  pointer-events: none;
`;

function describe(region: SimRegion, locale: Locale, compact: boolean): string {
  if (compact) {
    return `R${region.id + 1} · ${region.tps.toFixed(1)} TPS`;
  }
  const count = region.players.length;
  const people = locale === "zh" ? `${count} 人` : `${count} ${count === 1 ? "player" : "players"}`;
  return `R${region.id + 1} · ${people} · ${region.tps.toFixed(1)} TPS`;
}

/**
 * Region 分区的示意动画。模拟和绘制都在 requestAnimationFrame 里直接操作画布和标签，
 * 不经过 React 状态；滚出屏幕时暂停。
 *
 * The region partition animation. Simulation and drawing act on the canvas and the labels
 * directly inside requestAnimationFrame, bypassing React state, and pause when off screen.
 */
export function RegionSim() {
  const { locale, t } = useLocale();
  const theme = useTheme();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const labelRefs = useRef<(HTMLSpanElement | null)[]>([]);
  const localeRef = useRef(locale);
  localeRef.current = locale;

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    if (!canvas || !context) {
      return;
    }
    const simulation = new RegionSimulation();
    const colors = theme.chart.series;
    const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let frame = 0;
    let last = 0;
    let visible = true;

    const draw = () => {
      const { cell, columns, rows } = GRID;
      context.fillStyle = theme.chart.surface;
      context.fillRect(0, 0, WIDTH, HEIGHT);

      // 每个 section 的角上点一个暗点，当网格。
      // A dim dot at every section corner serves as the grid.
      context.fillStyle = "#2a2a2a";
      for (let z = 0; z <= rows; z += 1) {
        for (let x = 0; x <= columns; x += 1) {
          context.fillRect(x * cell, z * cell, 1, 1);
        }
      }

      const owner = simulation.owner;
      const at = (x: number, z: number) => (x < 0 || z < 0 || x >= columns || z >= rows ? -1 : owner[z * columns + x]);
      for (let z = 0; z < rows; z += 1) {
        for (let x = 0; x < columns; x += 1) {
          const id = at(x, z);
          if (id < 0) {
            continue;
          }
          const color = colors[id % colors.length];
          context.globalAlpha = 0.32;
          context.fillStyle = color;
          context.fillRect(x * cell, z * cell, cell, cell);
          // 只在 region 的外沿画实线，里面不画，看起来是一整块。
          // Solid lines only on the region's outer edge, so it reads as one piece.
          context.globalAlpha = 1;
          if (at(x - 1, z) !== id) context.fillRect(x * cell, z * cell, 2, cell);
          if (at(x + 1, z) !== id) context.fillRect((x + 1) * cell - 2, z * cell, 2, cell);
          if (at(x, z - 1) !== id) context.fillRect(x * cell, z * cell, cell, 2);
          if (at(x, z + 1) !== id) context.fillRect(x * cell, (z + 1) * cell - 2, cell, 2);
        }
      }

      for (const player of simulation.players) {
        const x = Math.round(player.x * cell);
        const z = Math.round(player.z * cell);
        context.fillStyle = "#000000";
        context.fillRect(x - 5, z - 5, 10, 10);
        context.fillStyle = "#ffffff";
        context.fillRect(x - 3, z - 3, 6, 6);
      }

      for (let index = 0; index < MAX_LABELS; index += 1) {
        const label = labelRefs.current[index];
        const region = simulation.regions[index];
        if (!label) {
          continue;
        }
        if (!region) {
          label.style.display = "none";
          continue;
        }
        // 标签贴在包围盒的左上角外侧，位置用百分比，画布缩放时跟着走。
        // The label sits just above the box's top-left corner, placed in percentages so it
        // follows the canvas when it scales.
        const left = Math.min(region.minX * cell, WIDTH - 150);
        const top = Math.max(0, region.minZ * cell - 20);
        label.style.display = "block";
        label.style.left = `${((left / WIDTH) * 100).toFixed(2)}%`;
        label.style.top = `${((top / HEIGHT) * 100).toFixed(2)}%`;
        label.style.setProperty("--region-color", colors[region.id % colors.length]);
        // 画布很窄时标签只留编号和速度，免得挤在一起。
        // On a narrow canvas the label keeps only the id and the rate, so labels do not pile up.
        label.textContent = describe(region, localeRef.current, canvas.clientWidth < 480);
      }
    };

    const tick = (now: number) => {
      const seconds = last === 0 ? 0 : Math.min(0.1, (now - last) / 1000);
      last = now;
      simulation.step(seconds);
      draw();
      if (visible && !still) {
        frame = requestAnimationFrame(tick);
      }
    };

    const observer = new IntersectionObserver(([entry]) => {
      visible = entry.isIntersecting;
      cancelAnimationFrame(frame);
      if (visible) {
        last = 0;
        frame = requestAnimationFrame(tick);
      }
    });
    observer.observe(canvas);

    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, [theme]);

  return (
    <Frame>
      <Canvas
        ref={canvasRef}
        width={WIDTH}
        height={HEIGHT}
        role="img"
        aria-label={t({
          zh: "示意动画：六名玩家在区块网格上走动，靠近的合并成一个 region，走远的拆开",
          en: "Animation: six players move over the chunk grid; those close together merge into one region and those far apart split",
        })}
      />
      {Array.from({ length: MAX_LABELS }, (_, index) => (
        <Label
          key={index}
          ref={(element) => {
            labelRefs.current[index] = element;
          }}
          aria-hidden="true"
        />
      ))}
      <Clock aria-hidden="true">{t({ zh: "世界时间 20.0 TPS", en: "World time 20.0 TPS" })}</Clock>
    </Frame>
  );
}
