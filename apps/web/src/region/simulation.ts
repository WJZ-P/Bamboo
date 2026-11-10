import { createRandom } from "../voxel/random";

/**
 * Region 分区规则的示意模拟，规则照 docs/world-model-design.md 6.1：
 * 网格单位是 section（4×4 个区块）；两片已加载区域相隔不超过 2 个 section 就合并，
 * 拉开到 4 个并保持一段时间才拆分。距离和时间都压缩过，只用来演示，不是服务端的代码。
 *
 * An illustrative simulation of the region partition rules in section 6.1 of
 * docs/world-model-design.md. The grid unit is a section (4×4 chunks); two loaded areas merge
 * when they come within 2 sections and split only after staying 4 apart for a while. Distance
 * and time are compressed. This is a demonstration, not server code.
 */
export const GRID = { columns: 64, rows: 36, cell: 10 } as const;

/** 视距 10 时 full 级区块的半径是 12 个区块，也就是 3 个 section。 / At view distance 10 the full-level radius is 12 chunks, which is 3 sections. */
const LOAD_RADIUS = 3;
const MERGE_WITHIN = 2;
const SPLIT_BEYOND = 4;
/** 设计里是 10 秒，演示里压缩成 1.6 秒。 / Ten seconds in the design, compressed to 1.6 here. */
const SPLIT_HOLD_SECONDS = 1.6;
const PLAYER_COUNT = 6;
const FULL_TPS = 20;

export interface SimPlayer {
  x: number;
  z: number;
  targetX: number;
  targetZ: number;
  speed: number;
}

export interface SimRegion {
  /** 用成员里编号最小的玩家当 region 的编号，颜色跟着它走。 / The lowest member index names the region, and its colour follows it. */
  readonly id: number;
  readonly players: readonly number[];
  /** 这个 region 自己的时钟走多快。 / How fast this region's own clock runs. */
  readonly tps: number;
  /** 包围盒，单位是 section。 / The bounding box, in sections. */
  readonly minX: number;
  readonly minZ: number;
  readonly maxX: number;
  readonly maxZ: number;
}

export class RegionSimulation {
  readonly players: SimPlayer[] = [];
  /** 每个 section 属于哪个 region，-1 是没加载。 / The region owning each section; -1 means not loaded. */
  readonly owner = new Int8Array(GRID.columns * GRID.rows);
  regions: SimRegion[] = [];

  private readonly rand = createRandom(6103);
  private readonly linked: boolean[][] = [];
  private readonly apartFor: number[][] = [];

  constructor() {
    for (let index = 0; index < PLAYER_COUNT; index += 1) {
      const player: SimPlayer = { x: 0, z: 0, targetX: 0, targetZ: 0, speed: 0.9 + this.rand() * 0.8 };
      [player.x, player.z] = this.randomSpot();
      [player.targetX, player.targetZ] = this.randomSpot();
      this.players.push(player);
      this.linked.push(new Array<boolean>(PLAYER_COUNT).fill(false));
      this.apartFor.push(new Array<number>(PLAYER_COUNT).fill(0));
    }
    this.step(0);
  }

  private randomSpot(): [number, number] {
    const margin = LOAD_RADIUS + 1;
    return [
      margin + this.rand() * (GRID.columns - margin * 2),
      margin + this.rand() * (GRID.rows - margin * 2),
    ];
  }

  /** 两名玩家的已加载区域之间隔着几个 section。 / How many sections lie between two players' loaded areas. */
  private gap(a: SimPlayer, b: SimPlayer): number {
    const dx = Math.abs(Math.floor(a.x) - Math.floor(b.x));
    const dz = Math.abs(Math.floor(a.z) - Math.floor(b.z));
    return Math.max(dx, dz) - LOAD_RADIUS * 2;
  }

  step(seconds: number) {
    for (const player of this.players) {
      const dx = player.targetX - player.x;
      const dz = player.targetZ - player.z;
      const distance = Math.hypot(dx, dz);
      const travel = player.speed * seconds;
      if (distance <= travel || distance < 0.01) {
        [player.targetX, player.targetZ] = this.randomSpot();
      } else {
        player.x += (dx / distance) * travel;
        player.z += (dz / distance) * travel;
      }
    }

    // 合并是立刻的；拆分要在足够远的地方保持一段时间，中间的距离维持原状。
    // Merging is immediate. Splitting needs the pair to stay far enough apart for a while, and
    // distances in between leave things as they are.
    for (let a = 0; a < PLAYER_COUNT; a += 1) {
      for (let b = a + 1; b < PLAYER_COUNT; b += 1) {
        const gap = this.gap(this.players[a], this.players[b]);
        if (gap <= MERGE_WITHIN) {
          this.linked[a][b] = true;
          this.apartFor[a][b] = 0;
        } else if (gap >= SPLIT_BEYOND && this.linked[a][b]) {
          this.apartFor[a][b] += seconds;
          if (this.apartFor[a][b] >= SPLIT_HOLD_SECONDS) {
            this.linked[a][b] = false;
          }
        } else {
          this.apartFor[a][b] = 0;
        }
      }
    }

    this.rebuildRegions();
  }

  /** 连在一起的玩家归进同一个 region（并查集）。 / Players linked together fall into one region (union–find). */
  private rebuildRegions() {
    const parent = this.players.map((_, index) => index);
    const find = (index: number): number => {
      while (parent[index] !== index) {
        parent[index] = parent[parent[index]];
        index = parent[index];
      }
      return index;
    };
    for (let a = 0; a < PLAYER_COUNT; a += 1) {
      for (let b = a + 1; b < PLAYER_COUNT; b += 1) {
        if (this.linked[a][b]) {
          // 让编号小的当根，这样根就是 region 的编号。
          // The lower index becomes the root, so the root is the region's id.
          const rootA = find(a);
          const rootB = find(b);
          parent[Math.max(rootA, rootB)] = Math.min(rootA, rootB);
        }
      }
    }

    const members = new Map<number, number[]>();
    this.players.forEach((_, index) => {
      const root = find(index);
      members.set(root, [...(members.get(root) ?? []), index]);
    });

    this.owner.fill(-1);
    this.regions = [...members.entries()].map(([id, players]) => {
      let minX: number = GRID.columns;
      let minZ: number = GRID.rows;
      let maxX = 0;
      let maxZ = 0;
      for (const index of players) {
        const centerX = Math.floor(this.players[index].x);
        const centerZ = Math.floor(this.players[index].z);
        for (let z = centerZ - LOAD_RADIUS; z <= centerZ + LOAD_RADIUS; z += 1) {
          for (let x = centerX - LOAD_RADIUS; x <= centerX + LOAD_RADIUS; x += 1) {
            if (x < 0 || z < 0 || x >= GRID.columns || z >= GRID.rows) {
              continue;
            }
            this.owner[z * GRID.columns + x] = id;
            minX = Math.min(minX, x);
            minZ = Math.min(minZ, z);
            maxX = Math.max(maxX, x);
            maxZ = Math.max(maxZ, z);
          }
        }
      }
      // 示意：假设三人以上的 region 过载，它自己的时钟变慢，别的 region 不受影响。
      // Illustration: assume a region with three or more players is overloaded. Its own clock
      // slows down and no other region is affected.
      const tps = players.length >= 4 ? 11 : players.length === 3 ? 15.5 : FULL_TPS;
      return { id, players, tps, minX, minZ, maxX, maxZ };
    });
  }
}
