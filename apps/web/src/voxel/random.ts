/**
 * 固定种子的伪随机数（mulberry32）。同一个种子每次生成同样的贴图和同样的小岛。
 *
 * A seeded pseudo-random generator (mulberry32). One seed always yields the same textures
 * and the same island.
 */
export function createRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let mixed = state;
    mixed = Math.imul(mixed ^ (mixed >>> 15), mixed | 1);
    mixed ^= mixed + Math.imul(mixed ^ (mixed >>> 7), mixed | 61);
    return ((mixed ^ (mixed >>> 14)) >>> 0) / 4294967296;
  };
}

/** 把整数坐标散列成 0 到 1 之间的数。 / Hashes integer coordinates into a number in [0, 1). */
export function hash2(x: number, z: number, seed: number): number {
  let mixed = Math.imul(x, 374761393) ^ Math.imul(z, 668265263) ^ Math.imul(seed, 2246822519);
  mixed = Math.imul(mixed ^ (mixed >>> 13), 1274126177);
  return ((mixed ^ (mixed >>> 16)) >>> 0) / 4294967296;
}

/**
 * 二维值噪声：格点上取散列值，中间用平滑曲线插值。输出在 0 到 1 之间。
 *
 * Two-dimensional value noise: hashed values on the lattice, smoothly interpolated in
 * between. The output is in [0, 1].
 */
export function valueNoise(x: number, z: number, seed: number): number {
  const cellX = Math.floor(x);
  const cellZ = Math.floor(z);
  const fx = x - cellX;
  const fz = z - cellZ;
  const smoothX = fx * fx * (3 - 2 * fx);
  const smoothZ = fz * fz * (3 - 2 * fz);
  const near = lerp(hash2(cellX, cellZ, seed), hash2(cellX + 1, cellZ, seed), smoothX);
  const far = lerp(hash2(cellX, cellZ + 1, seed), hash2(cellX + 1, cellZ + 1, seed), smoothX);
  return lerp(near, far, smoothZ);
}

export function lerp(from: number, to: number, amount: number): number {
  return from + (to - from) * amount;
}
