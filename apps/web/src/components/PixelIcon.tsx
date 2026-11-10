import { memo } from "react";

import type { IconName } from "../data/project";

interface Sprite {
  readonly rows: readonly string[];
  readonly palette: Readonly<Record<string, string>>;
}

/**
 * 12×12 的像素图标。每个字符是一个像素，点是透明，其余字符在调色板里查颜色。
 *
 * 12×12 pixel icons. Each character is one pixel: a dot is transparent, and any other
 * character is looked up in the palette.
 */
const SPRITES: Record<IconName, Sprite> = {
  players: {
    palette: { s: "#e0a878", t: "#c98f62", d: "#2b2b2b", g: "#3c8527", b: "#3f76e4" },
    rows: [
      "............",
      ".ssss.......",
      ".sdds.......",
      ".ssss..tttt.",
      ".ssss..tddt.",
      ".......tttt.",
      "gggggg.tttt.",
      "gggggg......",
      "ggggggbbbbbb",
      "ggggggbbbbbb",
      "......bbbbbb",
      "......bbbbbb",
    ],
  },
  pickaxe: {
    palette: { c: "#4fd8e6", h: "#8a5a2b" },
    rows: [
      "...ccccc....",
      "..cc...ccc..",
      "........hcc.",
      ".......h.cc.",
      "......h...c.",
      ".....h....c.",
      "....h.....c.",
      "...h........",
      "..h.........",
      ".h..........",
      "h...........",
      "............",
    ],
  },
  redstone: {
    palette: { r: "#e8322c", R: "#ff9a94", w: "#8a5a2b", d: "#a01c18" },
    rows: [
      "............",
      ".....rr.....",
      "....rRRr....",
      "....rRRr....",
      ".....rr.....",
      ".....ww.....",
      ".....ww.....",
      ".....ww.....",
      ".....ww.....",
      "..d..ww..d..",
      ".ddd.ww.ddd.",
      "dddddddddddd",
    ],
  },
  water: {
    palette: { b: "#4682ea", w: "#a9cbff" },
    rows: [
      ".....b......",
      ".....b......",
      "....bbb.....",
      "....bbb.....",
      "...bbbbb....",
      "..bbbbbbb...",
      "..bwbbbbb...",
      ".bbwbbbbbb..",
      ".bbwbbbbbb..",
      ".bbbbbbbbb..",
      "..bbbbbbb...",
      "...bbbbb....",
    ],
  },
  chest: {
    palette: { k: "#5a3a16", w: "#b07a2e", y: "#e6e6e6" },
    rows: [
      "............",
      ".kkkkkkkkkk.",
      ".kwwwwwwwwk.",
      ".kwwwwwwwwk.",
      ".kkkkkkkkkk.",
      ".kwwwyywwwk.",
      ".kwwwyywwwk.",
      ".kwwwwwwwwk.",
      ".kwwwwwwwwk.",
      ".kwwwwwwwwk.",
      ".kkkkkkkkkk.",
      "............",
    ],
  },
  terminal: {
    palette: { k: "#8b8b8b", n: "#101010", g: "#6be04a" },
    rows: [
      "............",
      "kkkkkkkkkkkk",
      "knnnnnnnnnnk",
      "kngnnnnnnnnk",
      "knngnnnnnnnk",
      "knnngnnnnnnk",
      "knngnnnnnnnk",
      "kngnnnggggnk",
      "knnnnnnnnnnk",
      "kkkkkkkkkkkk",
      "............",
      "............",
    ],
  },
  sun: {
    palette: { y: "#ffcc33", Y: "#fff3a8" },
    rows: [
      ".....yy.....",
      ".y...yy...y.",
      "..y......y..",
      "....yyyy....",
      "...yYYYYy...",
      "yy.yYYYYy.yy",
      "yy.yYYYYy.yy",
      "...yYYYYy...",
      "....yyyy....",
      "..y......y..",
      ".y...yy...y.",
      ".....yy.....",
    ],
  },
  gem: {
    palette: { c: "#4fd8e6", W: "#d8fbff", C: "#2a9fb0" },
    rows: [
      "............",
      "...cccccc...",
      "..cWWcccCc..",
      ".cWWccccCCc.",
      "cccccccccccc",
      ".cCccccccCc.",
      "..cCccccCc..",
      "...cCccCc...",
      "....cCCc....",
      ".....cc.....",
      "............",
      "............",
    ],
  },
  lock: {
    palette: { k: "#8b8b8b", y: "#ffcc33", h: "#3a2a00" },
    rows: [
      "............",
      "....kkkk....",
      "...kk..kk...",
      "...k....k...",
      "...k....k...",
      "..yyyyyyyy..",
      "..yyyyyyyy..",
      "..yyyhhyyy..",
      "..yyyhhyyy..",
      "..yyyyyyyy..",
      "..yyyyyyyy..",
      "............",
    ],
  },
  monitor: {
    palette: { k: "#8b8b8b", n: "#101010", g: "#6be04a" },
    rows: [
      "............",
      "kkkkkkkkkkkk",
      "knnnnnnnnnnk",
      "knnnnnnnngnk",
      "knnnnnngngnk",
      "knnnngngngnk",
      "knngngngngnk",
      "knnnnnnnnnnk",
      "kkkkkkkkkkkk",
      ".....kk.....",
      "...kkkkkk...",
      "............",
    ],
  },
};

/** 画一个像素图标。纯装饰，读屏软件会跳过它。 / Draws a pixel icon. It is decorative, so screen readers skip it. */
export const PixelIcon = memo(function PixelIcon({ name, size = 36 }: { name: IconName; size?: number }) {
  const sprite = SPRITES[name];
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 12 12"
      shapeRendering="crispEdges"
      aria-hidden="true"
      style={{ flex: "none", filter: "drop-shadow(2px 2px 0 rgba(0, 0, 0, 0.45))" }}
    >
      {sprite.rows.flatMap((row, y) =>
        [...row].map((cell, x) =>
          cell === "." ? null : <rect key={`${x}:${y}`} x={x} y={y} width={1} height={1} fill={sprite.palette[cell]} />,
        ),
      )}
    </svg>
  );
});
