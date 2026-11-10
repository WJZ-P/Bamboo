import type { BlockDef } from "./blocks";
import { TILE_SIZE, tileCanvas } from "./tiles";

const ICON_SIZE = TILE_SIZE * 2;
const cache = new Map<string, string>();

/**
 * 给一种方块画一张物品栏图标，返回 data URL。
 * 整格方块画成等轴测的小立方体：顶面原色，左面略暗，右面更暗。其它形状直接用它的贴图。
 *
 * Draws an inventory icon for a block and returns it as a data URL. Full cubes become a small
 * isometric cube: the top at full brightness, the left face dimmer and the right face
 * dimmer still. Other shapes just show their tile.
 */
export function blockIcon(block: BlockDef, lit = false): string {
  const key = `${block.key}:${lit}`;
  const cached = cache.get(key);
  if (cached) {
    return cached;
  }

  const canvas = document.createElement("canvas");
  canvas.width = ICON_SIZE;
  canvas.height = ICON_SIZE;
  const context = canvas.getContext("2d");
  if (!context) {
    return "";
  }
  context.imageSmoothingEnabled = false;

  if (block.shape === "stalk") {
    // 竹子图标：一根斜放的竹竿，上面画两片叶子。
    // The bamboo icon: a pole with two leaves on top.
    const stalk = tileCanvas("bambooStalk");
    const leaves = tileCanvas("bambooLeaf");
    context.drawImage(leaves, 0, 0, TILE_SIZE, TILE_SIZE, 0, 0, ICON_SIZE, ICON_SIZE);
    context.drawImage(stalk, 0, 0, 3, TILE_SIZE, ICON_SIZE / 2 - 3, 0, 6, ICON_SIZE);
  } else if (block.shape !== "cube") {
    context.drawImage(tileCanvas(block.side), 0, 0, ICON_SIZE, ICON_SIZE);
  } else {
    const side = lit && block.litTile ? block.litTile : block.side;
    const top = lit && block.litTile ? block.litTile : block.top;
    const faces = [
      { tile: top, transform: [1, 0.5, -1, 0.5, TILE_SIZE, 0], shade: 0 },
      { tile: side, transform: [1, 0.5, 0, 1, 0, TILE_SIZE / 2], shade: 0.22 },
      { tile: side, transform: [1, -0.5, 0, 1, TILE_SIZE, TILE_SIZE], shade: 0.42 },
    ] as const;
    for (const face of faces) {
      const [a, b, c, d, e, f] = face.transform;
      context.setTransform(a, b, c, d, e, f);
      context.drawImage(tileCanvas(face.tile), 0, 0);
      if (face.shade > 0) {
        context.fillStyle = `rgba(0, 0, 0, ${face.shade})`;
        context.fillRect(0, 0, TILE_SIZE, TILE_SIZE);
      }
    }
    context.setTransform(1, 0, 0, 1, 0, 0);
  }

  const url = canvas.toDataURL();
  cache.set(key, url);
  return url;
}
