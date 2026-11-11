import { useEffect, useMemo, useRef, useState } from "react";
import styled, { css, keyframes } from "styled-components";

import { PixelButton, PixelLink } from "../components/pixel";
import { REPO_URL, TARGET } from "../data/project";
import { formatNumber, useLocale, type Locale } from "../i18n";
import { raisedBevel, textShadow } from "../theme/bevel";
import { BLOCK_BY_ID, HOTBAR } from "../voxel/blocks";
import { createVoxelEngine, type DebugInfo, type ToolMode, type VoxelEngine } from "../voxel/engine";
import { blockIcon } from "../voxel/icons";

const drift = keyframes`
  from { background-position-x: 0; }
  to { background-position-x: -1280px; }
`;

const Wrap = styled.header`
  position: relative;
  z-index: 1;
  height: clamp(600px, calc(100vh - ${({ theme }) => theme.layout.navHeight}), 840px);
  overflow: hidden;
  /* 底边是一条草皮：上面是天，下面整页都是地底的石头。 */
  /* The bottom edge is a strip of turf: sky above, and the rest of the page is underground stone. */
  border-bottom: 4px solid #000000;

  @media (max-width: 899px) {
    height: auto;
  }
`;

const skyLayer = css`
  position: absolute;
  inset: 0;
  pointer-events: none;
`;

const DaySky = styled.div`
  ${skyLayer}
  background: linear-gradient(180deg, #4f93ea 0%, #7db6f5 48%, #c4e2fb 100%);
`;

const NightSky = styled.div<{ $night: boolean }>`
  ${skyLayer}
  background:
    var(--tex-stars) 0 0 / 384px 384px,
    linear-gradient(180deg, #060a1c 0%, #101a3c 55%, #24305e 100%);
  image-rendering: pixelated;
  opacity: ${({ $night }) => ($night ? 1 : 0)};
  transition: opacity 0.9s linear;
`;

const Orb = styled.div<{ $night: boolean }>`
  position: absolute;
  top: 9%;
  right: 12%;
  width: 56px;
  height: 56px;
  pointer-events: none;
  background: ${({ $night }) => ($night ? "#dfe7fb" : "#fff6c4")};
  box-shadow: ${({ $night }) =>
    $night
      ? "inset -12px -8px 0 0 #b9c4e6, 0 0 0 8px rgba(223, 231, 251, 0.08)"
      : "0 0 0 8px rgba(255, 246, 196, 0.35), 0 0 0 20px rgba(255, 246, 196, 0.16)"};
  transition:
    background 0.9s linear,
    box-shadow 0.9s linear;

  @media (max-width: 899px) {
    top: 20px;
    right: 24px;
    width: 40px;
    height: 40px;
  }
`;

const Clouds = styled.div<{ $night: boolean; $far?: boolean }>`
  ${skyLayer}
  top: ${({ $far }) => ($far ? "22%" : "4%")};
  height: ${({ $far }) => ($far ? "192px" : "384px")};
  background-image: var(--tex-clouds);
  background-size: ${({ $far }) => ($far ? "640px 192px" : "1280px 384px")};
  background-repeat: repeat-x;
  image-rendering: pixelated;
  opacity: ${({ $night, $far }) => ($night ? 0.1 : $far ? 0.45 : 0.85)};
  transition: opacity 0.9s linear;
  animation: ${drift} ${({ $far }) => ($far ? 260 : 150)}s linear infinite;

  @media (prefers-reduced-motion: reduce) {
    animation: none;
  }
`;

const Turf = styled.div`
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 32px;
  pointer-events: none;
  background-image: var(--tex-grass-side);
  background-size: 32px 32px;
  image-rendering: pixelated;
`;

const Stage = styled.div`
  position: absolute;
  inset: 0;
  z-index: 1;

  @media (max-width: 899px) {
    position: relative;
    height: 480px;
  }
`;

const Canvas = styled.canvas`
  display: block;
  width: 100%;
  height: 100%;
  cursor: grab;
  /* 竖着滑照常滚动页面，横着滑用来转小岛。 */
  /* Vertical swipes still scroll the page; horizontal ones turn the island. */
  touch-action: pan-y;

  &:active {
    cursor: grabbing;
  }
`;

const Fallback = styled.p`
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  text-align: center;
  color: #ffffff;
  ${textShadow()}
`;

const Copy = styled.div`
  position: absolute;
  z-index: 2;
  top: 44%;
  left: max(28px, calc((100vw - ${({ theme }) => theme.layout.maxWidth}) / 2 + 20px));
  width: min(440px, 36vw);
  transform: translateY(-50%);
  /* 文字不挡鼠标，底下的小岛照样能拖。 */
  /* The text lets the mouse through, so the island underneath can still be dragged. */
  pointer-events: none;

  a,
  button {
    pointer-events: auto;
  }

  @media (max-width: 899px) {
    position: relative;
    top: auto;
    left: auto;
    width: auto;
    transform: none;
    padding: 36px 20px 0;
  }
`;

const Title = styled.h1`
  font-size: ${({ theme }) => theme.font.display};
  line-height: 1.1;
  letter-spacing: 4px;
  color: #ffffff;
  text-shadow:
    6px 6px 0 #000000,
    0 12px 24px rgba(0, 0, 0, 0.45);

  @media (max-width: 899px) {
    font-size: 48px;
    text-shadow: 4px 4px 0 #000000;
  }
`;

const Tagline = styled.p`
  margin-top: 14px;
  font-size: ${({ theme }) => theme.font.lead};
  line-height: 1.4;
  color: #ffffff;
  ${textShadow("#000000", 2)}
`;

const Lead = styled.p`
  margin-top: 12px;
  color: #f2f6fb;
  ${textShadow("#0b1a33", 2)}
`;

const Chips = styled.ul`
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 18px;
`;

const Chip = styled.li`
  padding: 1px 10px;
  border: 2px solid #000000;
  background: rgba(16, 16, 16, 0.78);
  color: ${({ theme }) => theme.color.text};
  white-space: nowrap;
  ${raisedBevel("rgba(255, 255, 255, 0.16)", "rgba(0, 0, 0, 0.5)", 2)}

  b {
    font-weight: normal;
    color: ${({ theme }) => theme.color.bamboo.light};
  }
`;

const Actions = styled.div`
  display: flex;
  flex-wrap: wrap;
  gap: 14px;
  margin-top: 24px;
`;

const Debug = styled.pre`
  position: absolute;
  top: 16px;
  left: 16px;
  z-index: 2;
  margin: 0;
  padding: 4px 8px;
  background: rgba(16, 16, 16, 0.5);
  color: #ffffff;
  font-family: inherit;
  font-size: ${({ theme }) => theme.font.caption};
  line-height: 1.5;
  pointer-events: none;

  /* 窗口不够宽时标题会顶到左上角，调试文字让位。 */
  /* On narrower windows the title reaches the top-left corner, so the debug text gives way. */
  @media (max-width: 1199px) {
    display: none;
  }
`;

const Hud = styled.div`
  position: absolute;
  z-index: 2;
  /* 宽屏时小岛偏右，快捷栏跟着放在小岛正下方，不和左边的标题抢位置。 */
  /* On wide screens the island sits right of centre, so the hotbar goes directly under it and leaves the title alone. */
  left: 36%;
  right: 0;
  bottom: 48px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  pointer-events: none;

  @media (max-width: 899px) {
    left: 0;
  }

  > * {
    pointer-events: auto;
  }
`;

const Hint = styled.p`
  padding: 0 10px;
  color: #ffffff;
  text-align: center;
  ${textShadow()}
  pointer-events: none !important;
`;

const Hotbar = styled.div`
  display: flex;
  padding: 3px;
  border: 3px solid #000000;
  background: rgba(22, 22, 22, 0.82);
  ${raisedBevel("rgba(255, 255, 255, 0.22)", "rgba(0, 0, 0, 0.55)", 3)}
`;

const HotbarSlot = styled.button<{ $selected: boolean }>`
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 48px;
  height: 48px;
  padding: 0;
  border: 2px solid;
  border-color: #373737 #8b8b8b #8b8b8b #373737;
  background: rgba(139, 139, 139, 0.55);
  cursor: pointer;

  img {
    width: 32px;
    height: 32px;
    filter: drop-shadow(2px 2px 0 rgba(0, 0, 0, 0.4));
  }

  /* 选中的格子外面套一圈白框，和游戏里的快捷栏一样。 */
  /* The selected slot gets a white frame, like the in-game hotbar. */
  ${({ $selected }) =>
    $selected &&
    css`
      z-index: 1;
      outline: 4px solid #ffffff;
      outline-offset: -1px;
      box-shadow: 0 0 0 6px #000000;
    `}

  &:hover {
    background: rgba(198, 198, 198, 0.7);
  }

  @media (max-width: 520px) {
    width: 34px;
    height: 34px;

    img {
      width: 24px;
      height: 24px;
    }
  }
`;

const Controls = styled.div`
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 8px;
`;

function describe(info: DebugInfo, locale: Locale): string {
  const target = info.target;
  const name = target ? BLOCK_BY_ID[target.id]?.name[locale] : undefined;
  const lines = [
    `Bamboo web · ${TARGET.version} / ${TARGET.protocol}`,
    `${info.fps} fps · ${formatNumber(info.faces, locale)} ${locale === "zh" ? "个面" : "faces"} · ` +
      `${formatNumber(info.blocks, locale)} ${locale === "zh" ? "个方块" : "blocks"}`,
    target && name ? `XYZ: ${target.position.join(" ")} · ${name}` : "XYZ: —",
  ];
  return lines.join("\n");
}

export function Hero() {
  const { locale, t } = useLocale();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const debugRef = useRef<HTMLPreElement>(null);
  const engineRef = useRef<VoxelEngine | null>(null);
  const localeRef = useRef(locale);
  const [supported, setSupported] = useState(true);
  const [selected, setSelected] = useState(0);
  const [mode, setMode] = useState<ToolMode>("break");
  const [night, setNight] = useState(false);

  localeRef.current = locale;

  // 引擎只创建一次。调试文字每秒刷新四次，直接写进 DOM，不经过 React 状态。
  // The engine is created once. The debug text refreshes four times a second and is written
  // straight to the DOM, bypassing React state.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) {
      return;
    }
    const engine = createVoxelEngine(canvas, {
      reducedMotion: window.matchMedia("(prefers-reduced-motion: reduce)").matches,
      onDebug: (info) => {
        if (debugRef.current) {
          debugRef.current.textContent = describe(info, localeRef.current);
        }
      },
    });
    engineRef.current = engine;
    setSupported(engine !== null);
    return () => {
      engine?.dispose();
      engineRef.current = null;
    };
  }, []);

  useEffect(() => engineRef.current?.setSelected(HOTBAR[selected].id), [selected]);
  useEffect(() => engineRef.current?.setMode(mode), [mode]);
  useEffect(() => engineRef.current?.setNight(night), [night]);

  // 数字键 1–9 选快捷栏，和游戏里一样。正在输入框里打字时不抢按键。
  // Number keys 1–9 pick a hotbar slot, as in the game. Typing in a field is left alone.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const focused = document.activeElement;
      if (event.ctrlKey || event.metaKey || event.altKey || focused instanceof HTMLInputElement) {
        return;
      }
      const index = Number.parseInt(event.key, 10) - 1;
      if (index >= 0 && index < HOTBAR.length) {
        setSelected(index);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const icons = useMemo(() => HOTBAR.map((block) => blockIcon(block, block.emission > 0)), []);

  return (
    <Wrap id="top">
      <DaySky />
      <NightSky $night={night} />
      <Orb $night={night} />
      <Clouds $night={night} $far />
      <Clouds $night={night} />

      <Copy>
        <Title>Bamboo</Title>
        <Tagline>{t({ zh: "用 Rust 从零编写的 Minecraft 服务端", en: "A Minecraft server written from scratch in Rust" })}</Tagline>
        <Lead>
          {t({
            zh: "只追一个版本，把每条行为都和官方代码逐项对上；架构朝着把机器的每个核都用上去设计。",
            en: "One target version, with every behaviour checked item by item against the official code, and an architecture designed to use every core of the machine.",
          })}
        </Lead>
        <Chips>
          <Chip>
            Minecraft Java <b>{TARGET.version}</b>
          </Chip>
          <Chip>
            {t({ zh: "协议", en: "Protocol" })} <b>{TARGET.protocol}</b>
          </Chip>
          <Chip>
            <b>AGPL-3.0</b>
          </Chip>
        </Chips>
        <Actions>
          <PixelLink $tone="bamboo" $large href={REPO_URL} target="_blank" rel="noopener noreferrer">
            {t({ zh: "查看源码", en: "View source" })}
          </PixelLink>
          <PixelLink $large href="#progress">
            {t({ zh: "看看进度", en: "See progress" })}
          </PixelLink>
        </Actions>
      </Copy>

      <Stage>
        <Canvas
          ref={canvasRef}
          role="img"
          aria-label={t({
            zh: "一座浮空的竹林小岛，可以拖动旋转，点击挖掉或放下方块",
            en: "A floating bamboo island. Drag to turn it, click to break or place blocks",
          })}
        />
        {supported ? (
          <>
            <Debug ref={debugRef} aria-hidden="true" />
            <Hud>
              <Hint>
                {mode === "break"
                  ? t({ zh: "拖动旋转 · 点击挖掘 · 右键放置", en: "Drag to turn · Click to break · Right-click to place" })
                  : t({ zh: "拖动旋转 · 点击放置 · 右键挖掘", en: "Drag to turn · Click to place · Right-click to break" })}
              </Hint>
              <Hotbar role="radiogroup" aria-label={t({ zh: "快捷栏", en: "Hotbar" })}>
                {HOTBAR.map((block, index) => (
                  <HotbarSlot
                    key={block.key}
                    type="button"
                    role="radio"
                    aria-checked={index === selected}
                    aria-label={block.name[locale]}
                    title={`${index + 1} · ${block.name[locale]}`}
                    $selected={index === selected}
                    onClick={() => setSelected(index)}
                  >
                    <img src={icons[index]} alt="" />
                  </HotbarSlot>
                ))}
              </Hotbar>
              <Controls>
                <PixelButton type="button" onClick={() => setMode((current) => (current === "break" ? "place" : "break"))}>
                  {t({ zh: "模式：", en: "Mode: " })}
                  {mode === "break" ? t({ zh: "挖掘", en: "Break" }) : t({ zh: "放置", en: "Place" })}
                </PixelButton>
                <PixelButton type="button" onClick={() => setNight((current) => !current)}>
                  {t({ zh: "时间：", en: "Time: " })}
                  {night ? t({ zh: "夜晚", en: "Night" }) : t({ zh: "白天", en: "Day" })}
                </PixelButton>
                <PixelButton type="button" onClick={() => engineRef.current?.reset()}>
                  {t({ zh: "重置小岛", en: "Reset island" })}
                </PixelButton>
              </Controls>
            </Hud>
          </>
        ) : (
          <Fallback>
            {t({
              zh: "这个浏览器不支持 WebGL2，三维小岛没法显示。下面的内容不受影响。",
              en: "This browser lacks WebGL2, so the 3D island cannot be shown. Everything below still works.",
            })}
          </Fallback>
        )}
      </Stage>
      <Turf />
    </Wrap>
  );
}
