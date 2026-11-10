import { AIR, BLOCK_BY_ID, BLOCKS, isOpaque } from "./blocks";
import { createRandom, valueNoise } from "./random";

export const WORLD_SIZE = { x: 36, y: 30, z: 36 } as const;

/** 主岛草地所在的高度。 / The height of the main island's lawn. */
export const GROUND_LEVEL = 15;

/** 相机绕着转的点。 / The point the camera orbits. */
export const WORLD_FOCUS = [WORLD_SIZE.x / 2, GROUND_LEVEL - 1.5, WORLD_SIZE.z / 2] as const;

const SEED = 2603;

/**
 * 一小块体素世界：一个定长数组存方块 id，另一个存方块光。
 *
 * A small voxel world: one fixed array of block ids and another of block light.
 */
export class World {
  readonly blocks = new Uint8Array(WORLD_SIZE.x * WORLD_SIZE.y * WORLD_SIZE.z);
  readonly light = new Uint8Array(this.blocks.length);
  /** 灯是不是亮着。夜里才亮。 / Whether lamps are lit. They are lit only at night. */
  lampsLit = false;

  inBounds(x: number, y: number, z: number): boolean {
    return x >= 0 && y >= 0 && z >= 0 && x < WORLD_SIZE.x && y < WORLD_SIZE.y && z < WORLD_SIZE.z;
  }

  private index(x: number, y: number, z: number): number {
    return x + WORLD_SIZE.x * (z + WORLD_SIZE.z * y);
  }

  get(x: number, y: number, z: number): number {
    return this.inBounds(x, y, z) ? this.blocks[this.index(x, y, z)] : AIR;
  }

  set(x: number, y: number, z: number, id: number) {
    if (this.inBounds(x, y, z)) {
      this.blocks[this.index(x, y, z)] = id;
    }
  }

  lightAt(x: number, y: number, z: number): number {
    return this.inBounds(x, y, z) ? this.light[this.index(x, y, z)] : 0;
  }

  /** 这一列最高的整格方块的高度，没有就是 -1。 / The height of the topmost full cube in a column, or -1. */
  surface(x: number, z: number): number {
    for (let y = WORLD_SIZE.y - 1; y >= 0; y -= 1) {
      if (isOpaque(this.get(x, y, z))) {
        return y;
      }
    }
    return -1;
  }

  countBlocks(): number {
    let count = 0;
    for (const id of this.blocks) {
      if (id !== AIR) {
        count += 1;
      }
    }
    return count;
  }

  /**
   * 重算方块光：从每个亮着的光源出发做广度优先搜索，每走一格减一，整格方块挡光。
   *
   * Recomputes block light: a breadth-first flood from every lit source, losing one level
   * per step, stopped by full cubes.
   */
  relight() {
    this.light.fill(0);
    if (!this.lampsLit) {
      return;
    }
    const queue: number[] = [];
    for (let y = 0; y < WORLD_SIZE.y; y += 1) {
      for (let z = 0; z < WORLD_SIZE.z; z += 1) {
        for (let x = 0; x < WORLD_SIZE.x; x += 1) {
          const emission = BLOCK_BY_ID[this.get(x, y, z)]?.emission ?? 0;
          if (emission > 0) {
            this.light[this.index(x, y, z)] = emission;
            queue.push(x, y, z);
          }
        }
      }
    }
    for (let head = 0; head < queue.length; head += 3) {
      const x = queue[head];
      const y = queue[head + 1];
      const z = queue[head + 2];
      const next = this.light[this.index(x, y, z)] - 1;
      if (next <= 0) {
        continue;
      }
      for (const [dx, dy, dz] of NEIGHBOURS) {
        const nx = x + dx;
        const ny = y + dy;
        const nz = z + dz;
        if (!this.inBounds(nx, ny, nz) || isOpaque(this.get(nx, ny, nz))) {
          continue;
        }
        const index = this.index(nx, ny, nz);
        if (this.light[index] < next) {
          this.light[index] = next;
          queue.push(nx, ny, nz);
        }
      }
    }
  }
}

const NEIGHBOURS = [
  [1, 0, 0],
  [-1, 0, 0],
  [0, 1, 0],
  [0, -1, 0],
  [0, 0, 1],
  [0, 0, -1],
] as const;

const ORES = [BLOCKS.oreDiamond, BLOCKS.oreGold, BLOCKS.oreRedstone, BLOCKS.oreEmerald];

/** 在一列草地上种一根竹子。 / Plants one bamboo stalk on a grass column. */
function plantBamboo(world: World, x: number, z: number, height: number): boolean {
  const top = world.surface(x, z);
  if (top < 0 || world.get(x, top, z) !== BLOCKS.grass.id || world.get(x, top + 1, z) !== AIR) {
    return false;
  }
  for (let step = 1; step <= height; step += 1) {
    world.set(x, top + step, z, BLOCKS.bamboo.id);
  }
  return true;
}

/** 堆一个上宽下尖的浮空小岛。 / Builds a small floating islet, wide on top and pointed below. */
function buildIslet(world: World, centerX: number, centerZ: number, top: number, radius: number) {
  const reach = Math.ceil(radius);
  for (let z = Math.floor(centerZ) - reach; z <= Math.floor(centerZ) + reach; z += 1) {
    for (let x = Math.floor(centerX) - reach; x <= Math.floor(centerX) + reach; x += 1) {
      const distance = Math.hypot(x + 0.5 - centerX, z + 0.5 - centerZ) / radius;
      if (distance >= 1) {
        continue;
      }
      const depth = 1 + Math.round((1 - distance) * (radius + 1.5));
      for (let below = 0; below <= depth; below += 1) {
        const id = below === 0 ? BLOCKS.grass.id : below <= 1 ? BLOCKS.dirt.id : BLOCKS.stone.id;
        world.set(x, top - below, z, id);
      }
    }
  }
}

/**
 * 生成展示用的浮空岛：草地、土层、带矿的石头底、一个池塘、一片竹林、两盏灯和两个小岛。
 * 同一个种子每次生成的结果一样。
 *
 * Generates the showcase island: lawn, dirt, an ore-speckled stone underside, a pond, a
 * bamboo grove, two lamps and two islets. One seed always yields the same result.
 */
export function generateIsland(world: World) {
  world.blocks.fill(AIR);
  const rand = createRandom(SEED);
  const centerX = WORLD_SIZE.x / 2;
  const centerZ = WORLD_SIZE.z / 2;
  const pond = { x: centerX + 3.5, z: centerZ - 2.5, radius: 3.1 };
  const grove = { x: centerX - 4.5, z: centerZ + 3.5, radius: 4.2 };

  for (let z = 0; z < WORLD_SIZE.z; z += 1) {
    for (let x = 0; x < WORLD_SIZE.x; x += 1) {
      const outline = 10.5 + (valueNoise(x * 0.18, z * 0.18, SEED) - 0.5) * 3;
      const distance = Math.hypot(x + 0.5 - centerX, z + 0.5 - centerZ) / outline;
      const pondDistance = Math.hypot(x + 0.5 - pond.x, z + 0.5 - pond.z);
      const onShore = pondDistance < pond.radius + 1.4;
      if (distance >= 1 && !onShore) {
        continue;
      }
      const inward = Math.max(0, 1 - distance);
      const hill = Math.round(valueNoise(x * 0.22 + 40, z * 0.22 + 40, SEED) * 2.6 * (1 - distance * distance));
      const top = onShore ? GROUND_LEVEL : GROUND_LEVEL + Math.max(0, hill);
      const depth = 2 + inward ** 0.75 * 10 + valueNoise(x * 0.35 + 90, z * 0.35 + 90, SEED) * 3;
      const bottom = Math.max(1, Math.round(GROUND_LEVEL - depth));
      const dirtLayers = 2 + Math.floor(rand() * 2);

      for (let y = bottom; y <= top; y += 1) {
        const below = top - y;
        let id: number = BLOCKS.stone.id;
        if (below === 0) {
          id = onShore ? BLOCKS.sand.id : BLOCKS.grass.id;
        } else if (below <= dirtLayers) {
          id = onShore && below === 1 ? BLOCKS.sand.id : BLOCKS.dirt.id;
        } else if (rand() < 0.07) {
          id = ORES[Math.floor(rand() * ORES.length)].id;
        }
        world.set(x, y, z, id);
      }

      if (pondDistance < pond.radius) {
        world.set(x, GROUND_LEVEL, z, BLOCKS.water.id);
        if (pondDistance < pond.radius - 1.2) {
          world.set(x, GROUND_LEVEL - 1, z, BLOCKS.water.id);
          world.set(x, GROUND_LEVEL - 2, z, BLOCKS.sand.id);
        }
      }
    }
  }

  buildIslet(world, 7.5, 28.5, GROUND_LEVEL + 3, 2.6);
  buildIslet(world, 29.5, 6.5, GROUND_LEVEL - 2, 2.2);
  plantBamboo(world, 7, 28, 4);

  // 竹林：在一个圆里随机挑位置，长短不一。
  // The grove: random spots inside a circle, with varied heights.
  for (let attempt = 0; attempt < 44; attempt += 1) {
    const angle = rand() * Math.PI * 2;
    const reach = Math.sqrt(rand()) * grove.radius;
    const x = Math.floor(grove.x + Math.cos(angle) * reach);
    const z = Math.floor(grove.z + Math.sin(angle) * reach);
    plantBamboo(world, x, z, 3 + Math.floor(rand() * 6));
  }

  // 两盏灯：两格竹板上面放一个灯。
  // Two lamps: a lamp on a two-block post of bamboo planks.
  for (const [x, z] of [
    [centerX - 5, centerZ - 6],
    [centerX + 4, centerZ + 5],
  ]) {
    const top = world.surface(x, z);
    if (top >= 0 && world.get(x, top + 1, z) === AIR) {
      world.set(x, top + 1, z, BLOCKS.planks.id);
      world.set(x, top + 2, z, BLOCKS.planks.id);
      world.set(x, top + 3, z, BLOCKS.lamp.id);
    }
  }

  // 草丛和花只长在露天的草方块上。
  // Tufts and flowers grow only on open grass blocks.
  for (let z = 0; z < WORLD_SIZE.z; z += 1) {
    for (let x = 0; x < WORLD_SIZE.x; x += 1) {
      const top = world.surface(x, z);
      if (top < 0 || world.get(x, top, z) !== BLOCKS.grass.id || world.get(x, top + 1, z) !== AIR) {
        continue;
      }
      const roll = rand();
      if (roll < 0.17) {
        world.set(x, top + 1, z, BLOCKS.tuft.id);
      } else if (roll < 0.2) {
        world.set(x, top + 1, z, BLOCKS.flowerRed.id);
      } else if (roll < 0.23) {
        world.set(x, top + 1, z, BLOCKS.flowerYellow.id);
      }
    }
  }

  world.relight();
}
