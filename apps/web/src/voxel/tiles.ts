import { createRandom } from "./random";

/**
 * 方块贴图。每一张都是 16×16 的像素画，由下面的代码按固定种子画出来，
 * 不使用任何游戏素材。
 *
 * Block textures. Each one is a 16×16 pixel drawing produced by the code below from a fixed
 * seed. No game assets are used.
 */
export const TILE_SIZE = 16;

export const TILE_NAMES = [
  "grassTop",
  "grassSide",
  "dirt",
  "stone",
  "sand",
  "water",
  "planks",
  "lampOn",
  "lampOff",
  "oreDiamond",
  "oreGold",
  "oreRedstone",
  "oreEmerald",
  "bambooStalk",
  "bambooLeaf",
  "bambooLeafSmall",
  "tuft",
  "flowerRed",
  "flowerYellow",
] as const;

export type TileName = (typeof TILE_NAMES)[number];

/** 贴图在纹理数组里的层号。 / The layer of a tile in the texture array. */
export function tileLayer(name: TileName): number {
  return TILE_NAMES.indexOf(name);
}

class Tile {
  readonly pixels = new Uint8ClampedArray(TILE_SIZE * TILE_SIZE * 4);

  set(x: number, y: number, color: string, alpha = 255) {
    if (x < 0 || y < 0 || x >= TILE_SIZE || y >= TILE_SIZE) {
      return;
    }
    const offset = (y * TILE_SIZE + x) * 4;
    this.pixels[offset] = Number.parseInt(color.slice(1, 3), 16);
    this.pixels[offset + 1] = Number.parseInt(color.slice(3, 5), 16);
    this.pixels[offset + 2] = Number.parseInt(color.slice(5, 7), 16);
    this.pixels[offset + 3] = alpha;
  }

  /** 每个像素从调色板里随机取一个颜色。 / Fills every pixel with a random palette colour. */
  noise(rand: () => number, palette: readonly string[], alpha = 255) {
    for (let y = 0; y < TILE_SIZE; y += 1) {
      for (let x = 0; x < TILE_SIZE; x += 1) {
        this.set(x, y, pick(rand, palette), alpha);
      }
    }
  }
}

function pick<T>(rand: () => number, items: readonly T[]): T {
  return items[Math.floor(rand() * items.length)];
}

const GRASS = ["#5d9c3a", "#6aab44", "#539033", "#74b84e", "#4f8a30"];
const DIRT = ["#866043", "#7a563b", "#946c4c", "#6d4c34", "#8c6447"];
const STONE = ["#7d7d7d", "#747474", "#6b6b6b", "#828282", "#8a8a8a"];
const SAND = ["#dbd3a0", "#d4cb96", "#e3dbaa", "#cfc58f"];
const WATER = ["#3f76e4", "#4682ea", "#3a6ed6", "#4c8af0"];
const LEAF = { light: "#8fd456", mid: "#62aa36", dark: "#3f7a22" };

function paintStone(tile: Tile, rand: () => number, palette: readonly string[], crack: string, glint: string) {
  tile.noise(rand, palette);
  for (let stroke = 0; stroke < 6; stroke += 1) {
    const x = Math.floor(rand() * TILE_SIZE);
    const y = Math.floor(rand() * TILE_SIZE);
    const length = 2 + Math.floor(rand() * 3);
    for (let step = 0; step < length; step += 1) {
      tile.set((x + step) % TILE_SIZE, y, stroke < 4 ? crack : glint);
    }
  }
}

function paintOre(tile: Tile, rand: () => number, main: string, shine: string) {
  paintStone(tile, rand, STONE, "#5f5f5f", "#939393");
  for (let cluster = 0; cluster < 6; cluster += 1) {
    const x = 2 + Math.floor(rand() * 11);
    const y = 2 + Math.floor(rand() * 11);
    tile.set(x + 1, y + 1, "#4c4c4c");
    tile.set(x + 2, y + 1, "#4c4c4c");
    tile.set(x, y, main);
    tile.set(x + 1, y, main);
    tile.set(x, y + 1, main);
    if (rand() < 0.6) {
      tile.set(x + 1, y - 1, main);
    }
    tile.set(x, y, shine);
  }
}

function paintLamp(tile: Tile, rand: () => number, lit: boolean) {
  const frame = lit ? "#b8721f" : "#4a2e10";
  const edge = lit ? "#7a4a14" : "#33200b";
  const cell = lit ? ["#ffd76a", "#ffe38f", "#ffc847"] : ["#6e4a22", "#7a5328", "#674520"];
  const core = lit ? "#fff3bf" : "#8a6030";
  for (let y = 0; y < TILE_SIZE; y += 1) {
    for (let x = 0; x < TILE_SIZE; x += 1) {
      const onEdge = x === 0 || y === 0 || x === 15 || y === 15;
      const onFrame = x % 5 === 0 || y % 5 === 0;
      const inCore = (x % 5 === 2 || x % 5 === 3) && (y % 5 === 2 || y % 5 === 3);
      tile.set(x, y, onEdge ? edge : onFrame ? frame : inCore ? core : pick(rand, cell));
    }
  }
}

interface Blade {
  x: number;
  y: number;
  dx: number;
  dy: number;
  length: number;
}

/** 竹叶：从中间的竹竿向两侧伸出去，叶尖略微下垂。 / Bamboo leaves reach out from the stalk and droop at the tip. */
function paintLeaves(tile: Tile, blades: readonly Blade[]) {
  for (const blade of blades) {
    for (let step = 0; step <= blade.length; step += 1) {
      const x = Math.round(blade.x + blade.dx * step);
      const y = Math.round(blade.y + blade.dy * step + 0.07 * step * step);
      const nearTip = step >= blade.length - 1;
      tile.set(x, y, nearTip ? LEAF.light : LEAF.mid);
      if (!nearTip && step > 0) {
        tile.set(x, y + 1, LEAF.dark);
      }
    }
  }
}

function paintFlower(tile: Tile, petal: string, petalShade: string, heart: string) {
  for (let y = 7; y < TILE_SIZE; y += 1) {
    tile.set(7, y, y % 3 === 0 ? LEAF.dark : LEAF.mid);
  }
  tile.set(6, 11, LEAF.light);
  tile.set(5, 10, LEAF.mid);
  tile.set(8, 13, LEAF.light);
  tile.set(9, 12, LEAF.mid);
  const rows = [".PP.", "PHHP", "SHHS", ".SS."];
  rows.forEach((row, rowIndex) => {
    [...row].forEach((cell, column) => {
      if (cell !== ".") {
        tile.set(6 + column, 3 + rowIndex, cell === "P" ? petal : cell === "S" ? petalShade : heart);
      }
    });
  });
}

const PAINTERS: Record<TileName, (tile: Tile, rand: () => number) => void> = {
  grassTop: (tile, rand) => tile.noise(rand, GRASS),
  grassSide: (tile, rand) => {
    tile.noise(rand, DIRT);
    for (let x = 0; x < TILE_SIZE; x += 1) {
      const fringe = 2 + Math.floor(rand() * 3);
      for (let y = 0; y < fringe; y += 1) {
        tile.set(x, y, y === fringe - 1 ? "#4a822c" : pick(rand, GRASS));
      }
    }
  },
  dirt: (tile, rand) => {
    tile.noise(rand, DIRT);
    for (let speck = 0; speck < 7; speck += 1) {
      tile.set(Math.floor(rand() * TILE_SIZE), Math.floor(rand() * TILE_SIZE), "#5a3f2b");
    }
  },
  stone: (tile, rand) => paintStone(tile, rand, STONE, "#5f5f5f", "#939393"),
  sand: (tile, rand) => {
    tile.noise(rand, SAND);
    for (let speck = 0; speck < 5; speck += 1) {
      tile.set(Math.floor(rand() * TILE_SIZE), Math.floor(rand() * TILE_SIZE), "#bfb47f");
    }
  },
  water: (tile, rand) => {
    tile.noise(rand, WATER, 196);
    for (let streak = 0; streak < 5; streak += 1) {
      const x = Math.floor(rand() * TILE_SIZE);
      const y = Math.floor(rand() * TILE_SIZE);
      for (let step = 0; step < 3; step += 1) {
        tile.set((x + step) % TILE_SIZE, y, "#7fb0f7", 196);
      }
    }
  },
  planks: (tile, rand) => {
    for (let y = 0; y < TILE_SIZE; y += 1) {
      const row = Math.floor(y / 4);
      for (let x = 0; x < TILE_SIZE; x += 1) {
        const onSeam = y % 4 === 3 || x === (row * 5 + 3) % TILE_SIZE;
        tile.set(x, y, onSeam ? "#94823a" : pick(rand, ["#c9b45b", "#c2ad55", "#d0bb63", "#c6b058"]));
      }
    }
  },
  lampOn: (tile, rand) => paintLamp(tile, rand, true),
  lampOff: (tile, rand) => paintLamp(tile, rand, false),
  oreDiamond: (tile, rand) => paintOre(tile, rand, "#4fd8e6", "#c4f8fb"),
  oreGold: (tile, rand) => paintOre(tile, rand, "#f5d63d", "#fff8b8"),
  oreRedstone: (tile, rand) => paintOre(tile, rand, "#e8322c", "#ff9a94"),
  oreEmerald: (tile, rand) => paintOre(tile, rand, "#2fcf68", "#a9f5c4"),
  bambooStalk: (tile) => {
    const columns = ["#5a9a2e", "#86c94f", "#447d22"];
    for (let y = 0; y < TILE_SIZE; y += 1) {
      for (let x = 0; x < TILE_SIZE; x += 1) {
        const column = x % 3;
        const color = y % 8 === 7 ? "#2f5c17" : y % 8 === 0 && column === 1 ? "#a8e072" : columns[column];
        tile.set(x, y, color);
      }
    }
  },
  bambooLeaf: (tile) =>
    paintLeaves(tile, [
      { x: 6, y: 13, dx: -1, dy: -0.75, length: 6 },
      { x: 9, y: 12, dx: 1, dy: -0.7, length: 6 },
      { x: 6, y: 9, dx: -1, dy: -0.7, length: 6 },
      { x: 9, y: 8, dx: 1, dy: -0.75, length: 6 },
      { x: 6, y: 5, dx: -1, dy: -0.6, length: 5 },
      { x: 9, y: 4, dx: 1, dy: -0.65, length: 5 },
      { x: 7, y: 2, dx: -0.6, dy: -0.5, length: 3 },
      { x: 8, y: 2, dx: 0.6, dy: -0.5, length: 3 },
    ]),
  bambooLeafSmall: (tile) =>
    paintLeaves(tile, [
      { x: 6, y: 11, dx: -1, dy: -0.7, length: 4 },
      { x: 9, y: 7, dx: 1, dy: -0.7, length: 4 },
      { x: 6, y: 3, dx: -1, dy: -0.6, length: 3 },
    ]),
  tuft: (tile, rand) => {
    for (const base of [1, 3, 5, 6, 8, 10, 12, 14]) {
      const height = 4 + Math.floor(rand() * 8);
      const lean = (rand() - 0.5) * 0.5;
      for (let step = 0; step < height; step += 1) {
        const tone = step > height * 0.66 ? LEAF.light : step > height * 0.33 ? LEAF.mid : LEAF.dark;
        tile.set(Math.round(base + lean * step), 15 - step, tone);
      }
    }
  },
  flowerRed: (tile) => paintFlower(tile, "#f0584d", "#bb342c", "#ffd84a"),
  flowerYellow: (tile) => paintFlower(tile, "#ffe066", "#d9a92b", "#fff7d1"),
};

const cache = new Map<TileName, Uint8ClampedArray>();

/** 一张贴图的 RGBA 像素，行从上到下。 / One tile's RGBA pixels, rows from top to bottom. */
export function tilePixels(name: TileName): Uint8ClampedArray {
  let pixels = cache.get(name);
  if (!pixels) {
    const tile = new Tile();
    PAINTERS[name](tile, createRandom(2603 + tileLayer(name) * 97));
    pixels = tile.pixels;
    cache.set(name, pixels);
  }
  return pixels;
}

/** 把一张贴图画到一块新的画布上。 / Draws one tile onto a fresh canvas. */
export function tileCanvas(name: TileName): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = TILE_SIZE;
  canvas.height = TILE_SIZE;
  const context = canvas.getContext("2d");
  if (context) {
    const image = context.createImageData(TILE_SIZE, TILE_SIZE);
    image.data.set(tilePixels(name));
    context.putImageData(image, 0, 0);
  }
  return canvas;
}
