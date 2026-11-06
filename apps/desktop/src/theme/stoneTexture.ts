const TILE_SIZE = 16;
const SHADES = ["#1b1b1b", "#1f1f1f", "#232323", "#272727", "#2b2b2b"];

/** 固定种子的伪随机数，保证每次启动生成的纹理都一样。 */
function createRandom(seed: number) {
  let state = seed;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 2 ** 32;
  };
}

/**
 * 用代码生成一块可平铺的石头纹理，写进 CSS 变量 `--stone-texture`。
 *
 * 纹理是程序生成的原创内容，不依赖游戏素材。
 */
export function installStoneTexture() {
  const canvas = document.createElement("canvas");
  canvas.width = TILE_SIZE;
  canvas.height = TILE_SIZE;
  const context = canvas.getContext("2d");
  if (!context) {
    return;
  }

  const random = createRandom(26);
  for (let y = 0; y < TILE_SIZE; y += 1) {
    for (let x = 0; x < TILE_SIZE; x += 1) {
      context.fillStyle = SHADES[Math.floor(random() * SHADES.length)];
      context.fillRect(x, y, 1, 1);
    }
  }

  // 几道短横线模拟石头的裂纹。
  context.fillStyle = "#171717";
  for (let crack = 0; crack < 9; crack += 1) {
    const length = 2 + Math.floor(random() * 3);
    context.fillRect(Math.floor(random() * TILE_SIZE), Math.floor(random() * TILE_SIZE), length, 1);
  }

  document.documentElement.style.setProperty("--stone-texture", `url(${canvas.toDataURL()})`);
}
