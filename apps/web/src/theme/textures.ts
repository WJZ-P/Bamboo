import { createRandom } from "../voxel/random";
import { TILE_SIZE, tileCanvas, type TileName } from "../voxel/tiles";

function createCanvas(width: number, height: number) {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d");
  if (context) {
    context.imageSmoothingEnabled = false;
  }
  return { canvas, context };
}

function publish(name: string, canvas: HTMLCanvasElement) {
  document.documentElement.style.setProperty(name, `url(${canvas.toDataURL()})`);
}

/** 页面背景：一片石头，里面零星嵌着矿石。 / The page backdrop: a wall of stone with a few ores set in it. */
function paintBackdrop() {
  const tiles = 10;
  const { canvas, context } = createCanvas(TILE_SIZE * tiles, TILE_SIZE * tiles);
  if (!context) {
    return;
  }
  const rand = createRandom(777);
  const ores: TileName[] = ["oreDiamond", "oreGold", "oreRedstone", "oreEmerald"];
  const stone = tileCanvas("stone");
  for (let row = 0; row < tiles; row += 1) {
    for (let column = 0; column < tiles; column += 1) {
      const tile = rand() < 0.06 ? tileCanvas(ores[Math.floor(rand() * ores.length)]) : stone;
      context.drawImage(tile, column * TILE_SIZE, row * TILE_SIZE);
    }
  }
  publish("--tex-backdrop", canvas);
}

/** 一条能首尾相接的像素云带。 / A strip of pixel clouds that tiles horizontally. */
function paintClouds() {
  const width = 160;
  const height = 48;
  const { canvas, context } = createCanvas(width, height);
  if (!context) {
    return;
  }
  const rand = createRandom(31);
  for (let cloud = 0; cloud < 7; cloud += 1) {
    const x = Math.floor(rand() * width);
    const y = 4 + Math.floor(rand() * (height - 16));
    const length = 12 + Math.floor(rand() * 22);
    const puffs = 3 + Math.floor(rand() * 3);
    for (let puff = 0; puff < puffs; puff += 1) {
      const offset = Math.floor(rand() * length * 0.6);
      const span = Math.floor(length * (0.4 + rand() * 0.5));
      const rise = puff === 0 ? 0 : 1 + Math.floor(rand() * 3);
      context.fillStyle = puff === 0 ? "rgba(255, 255, 255, 0.9)" : "rgba(255, 255, 255, 0.78)";
      // 画两次：超出右边的部分从左边接上。
      // Drawn twice so the part past the right edge wraps round to the left.
      for (const shift of [0, -width]) {
        context.fillRect(x + offset + shift, y - rise, span, 3 + rise);
      }
    }
  }
  publish("--tex-clouds", canvas);
}

/** 夜空里的星星。 / Stars for the night sky. */
function paintStars() {
  const size = 128;
  const { canvas, context } = createCanvas(size, size);
  if (!context) {
    return;
  }
  const rand = createRandom(4096);
  for (let star = 0; star < 46; star += 1) {
    context.fillStyle = `rgba(255, 255, 255, ${(0.35 + rand() * 0.65).toFixed(2)})`;
    context.fillRect(Math.floor(rand() * size), Math.floor(rand() * size), 1, 1);
  }
  publish("--tex-stars", canvas);
}

/**
 * 把程序生成的贴图写进 CSS 变量，样式里用 var(--tex-…) 引用。
 *
 * Writes the generated textures into CSS variables; styles refer to them as var(--tex-…).
 */
export function installTextures() {
  paintBackdrop();
  paintClouds();
  paintStars();
  publish("--tex-dirt", tileCanvas("dirt"));
  publish("--tex-sand", tileCanvas("sand"));
  publish("--tex-stone", tileCanvas("stone"));
  publish("--tex-grass-side", tileCanvas("grassSide"));
}
