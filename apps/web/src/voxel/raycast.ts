import { AIR } from "./blocks";
import type { Vec3 } from "./math";
import { WORLD_SIZE, type World } from "./world";

export interface RayHit {
  /** 射线打中的方块。 / The block the ray hit. */
  readonly block: Vec3;
  /** 打中的那个面朝外的方向，放置方块时往这边放。 / The outward direction of the face hit; blocks are placed on that side. */
  readonly normal: Vec3;
}

const SIZE: Readonly<Vec3> = [WORLD_SIZE.x, WORLD_SIZE.y, WORLD_SIZE.z];

/**
 * 沿射线一格一格地走，找到第一个不是空气的方块（Amanatides–Woo 算法）。
 * 射线从世界外面出发时先走到世界的边界。
 *
 * Walks the ray cell by cell to the first block that is not air (Amanatides–Woo). A ray that
 * starts outside the world is first advanced to the world's boundary.
 */
export function castRay(world: World, origin: Readonly<Vec3>, direction: Readonly<Vec3>): RayHit | null {
  let enter = 0;
  let exit = Number.POSITIVE_INFINITY;
  let enterAxis = -1;
  for (let axis = 0; axis < 3; axis += 1) {
    if (Math.abs(direction[axis]) < 1e-9) {
      if (origin[axis] < 0 || origin[axis] > SIZE[axis]) {
        return null;
      }
      continue;
    }
    const first = (0 - origin[axis]) / direction[axis];
    const second = (SIZE[axis] - origin[axis]) / direction[axis];
    const near = Math.min(first, second);
    const far = Math.max(first, second);
    if (near > enter) {
      enter = near;
      enterAxis = axis;
    }
    exit = Math.min(exit, far);
  }
  if (enter > exit) {
    return null;
  }

  const start: Vec3 = [
    origin[0] + direction[0] * (enter + 1e-4),
    origin[1] + direction[1] * (enter + 1e-4),
    origin[2] + direction[2] * (enter + 1e-4),
  ];
  const cell: Vec3 = [Math.floor(start[0]), Math.floor(start[1]), Math.floor(start[2])];
  const step: Vec3 = [Math.sign(direction[0]), Math.sign(direction[1]), Math.sign(direction[2])];
  const delta: Vec3 = [0, 0, 0];
  const next: Vec3 = [0, 0, 0];
  for (let axis = 0; axis < 3; axis += 1) {
    if (step[axis] === 0) {
      delta[axis] = Number.POSITIVE_INFINITY;
      next[axis] = Number.POSITIVE_INFINITY;
    } else {
      const edge = step[axis] > 0 ? cell[axis] + 1 : cell[axis];
      delta[axis] = Math.abs(1 / direction[axis]);
      next[axis] = (edge - start[axis]) / direction[axis];
    }
  }

  let lastAxis = enterAxis;
  const limit = SIZE[0] + SIZE[1] + SIZE[2];
  for (let walked = 0; walked < limit; walked += 1) {
    if (!world.inBounds(cell[0], cell[1], cell[2])) {
      return null;
    }
    if (world.get(cell[0], cell[1], cell[2]) !== AIR) {
      const normal: Vec3 = [0, 0, 0];
      if (lastAxis >= 0) {
        normal[lastAxis] = -step[lastAxis];
      } else {
        normal[1] = 1;
      }
      return { block: [cell[0], cell[1], cell[2]], normal };
    }
    const axis = next[0] < next[1] ? (next[0] < next[2] ? 0 : 2) : next[1] < next[2] ? 1 : 2;
    cell[axis] += step[axis];
    next[axis] += delta[axis];
    lastAxis = axis;
  }
  return null;
}
