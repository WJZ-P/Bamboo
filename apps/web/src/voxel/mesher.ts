import { AIR, BLOCK_BY_ID, BLOCKS, isOpaque, type BlockDef } from "./blocks";
import { hash2 } from "./random";
import { tileLayer } from "./tiles";
import { WORLD_SIZE, type World } from "./world";

/**
 * 每个顶点 10 个浮点数：位置 3、贴图坐标 2、贴图层 1、天空光 1、方块光 1、摇摆幅度 1、摇摆相位 1。
 *
 * Ten floats per vertex: position 3, texture coordinate 2, texture layer 1, sky light 1,
 * block light 1, sway amplitude 1, sway phase 1.
 */
export const VERTEX_FLOATS = 10;

export interface MeshData {
  readonly vertices: Float32Array;
  readonly indices: Uint32Array;
}

export interface WorldMesh {
  /** 不透明和镂空的面。 / Opaque and cut-out faces. */
  readonly solid: MeshData;
  /** 半透明的水，要在最后画。 / Translucent water, drawn last. */
  readonly water: MeshData;
  readonly faces: number;
}

type Vec3 = readonly [number, number, number];

interface Face {
  readonly normal: Vec3;
  /** 面内的两个轴，u × v = normal，这样四个角从外面看是逆时针。 / In-plane axes with u × v = normal, so corners run counter-clockwise seen from outside. */
  readonly u: Vec3;
  readonly v: Vec3;
  /** 朝向带来的明暗：顶面最亮，底面最暗。 / Shading by direction: brightest on top, darkest below. */
  readonly shade: number;
}

const FACES: readonly Face[] = [
  { normal: [1, 0, 0], u: [0, 0, -1], v: [0, 1, 0], shade: 0.62 },
  { normal: [-1, 0, 0], u: [0, 0, 1], v: [0, 1, 0], shade: 0.62 },
  { normal: [0, 1, 0], u: [1, 0, 0], v: [0, 0, -1], shade: 1 },
  { normal: [0, -1, 0], u: [1, 0, 0], v: [0, 0, 1], shade: 0.5 },
  { normal: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0], shade: 0.8 },
  { normal: [0, 0, -1], u: [-1, 0, 0], v: [0, 1, 0], shade: 0.8 },
];

const TOP = 2;
const BOTTOM = 3;

/** 四个角在面内的位置，按逆时针排。 / Corner signs in the face plane, counter-clockwise. */
const CORNERS: readonly (readonly [number, number])[] = [
  [-1, -1],
  [1, -1],
  [1, 1],
  [-1, 1],
];

/** 环境光遮蔽的四档亮度：角落被挡得越多越暗。 / Four ambient-occlusion levels: the more a corner is enclosed, the darker. */
const OCCLUSION = [0.48, 0.66, 0.83, 1];

/** 方块光等级到亮度的曲线，离光源越远掉得越快。 / Block light level to brightness; it falls off faster away from the source. */
function brightness(level: number): number {
  const ratio = level / 15;
  return ratio / (4 - 3 * ratio);
}

class MeshBuilder {
  private readonly vertices: number[] = [];
  private readonly indices: number[] = [];
  private count = 0;
  faces = 0;

  vertex(
    position: Vec3,
    u: number,
    v: number,
    layer: number,
    sky: number,
    block: number,
    sway = 0,
    phase = 0,
  ): number {
    this.vertices.push(position[0], position[1], position[2], u, v, layer, sky, block, sway, phase);
    this.count += 1;
    return this.count - 1;
  }

  /** 四个顶点按逆时针给出；split13 为真时沿 1–3 对角线切开。 / Four counter-clockwise vertices; `split13` cuts along the 1–3 diagonal. */
  quad(a: number, b: number, c: number, d: number, split13 = false) {
    if (split13) {
      this.indices.push(b, c, d, b, d, a);
    } else {
      this.indices.push(a, b, c, a, c, d);
    }
    this.faces += 1;
  }

  finish(): MeshData {
    return { vertices: new Float32Array(this.vertices), indices: new Uint32Array(this.indices) };
  }
}

function tileFor(block: BlockDef, face: number, lit: boolean): number {
  if (lit && block.litTile) {
    return tileLayer(block.litTile);
  }
  return tileLayer(face === TOP ? block.top : face === BOTTOM ? block.bottom : block.side);
}

/**
 * 整格方块的一个面。每个角看它旁边的三格算环境光遮蔽，方块光取周围没被挡住的格子的平均。
 *
 * One face of a full cube. Each corner derives ambient occlusion from its three neighbours,
 * and block light is averaged over the surrounding cells that are not blocked.
 */
function emitCubeFace(
  builder: MeshBuilder,
  world: World,
  block: BlockDef,
  x: number,
  y: number,
  z: number,
  faceIndex: number,
  lit: boolean,
) {
  const face = FACES[faceIndex];
  const [nx, ny, nz] = face.normal;
  const layer = tileFor(block, faceIndex, lit);
  const outX = x + nx;
  const outY = y + ny;
  const outZ = z + nz;
  const corners: number[] = [];
  const occlusion: number[] = [];

  for (const [su, sv] of CORNERS) {
    const ux = face.u[0] * su;
    const uy = face.u[1] * su;
    const uz = face.u[2] * su;
    const vx = face.v[0] * sv;
    const vy = face.v[1] * sv;
    const vz = face.v[2] * sv;

    const side1 = isOpaque(world.get(outX + ux, outY + uy, outZ + uz));
    const side2 = isOpaque(world.get(outX + vx, outY + vy, outZ + vz));
    const corner = isOpaque(world.get(outX + ux + vx, outY + uy + vy, outZ + uz + vz));
    const level = side1 && side2 ? 0 : 3 - Number(side1) - Number(side2) - Number(corner);
    occlusion.push(level);

    let lightSum = world.lightAt(outX, outY, outZ);
    let lightCells = 1;
    if (!side1) {
      lightSum += world.lightAt(outX + ux, outY + uy, outZ + uz);
      lightCells += 1;
    }
    if (!side2) {
      lightSum += world.lightAt(outX + vx, outY + vy, outZ + vz);
      lightCells += 1;
    }
    if (!corner && !(side1 && side2)) {
      lightSum += world.lightAt(outX + ux + vx, outY + uy + vy, outZ + uz + vz);
      lightCells += 1;
    }
    const blockLight = lit ? 1 : brightness(lightSum / lightCells) * (0.55 + 0.45 * OCCLUSION[level]);

    const position: Vec3 = [
      x + 0.5 + 0.5 * (nx + ux + vx),
      y + 0.5 + 0.5 * (ny + uy + vy),
      z + 0.5 + 0.5 * (nz + uz + vz),
    ];
    corners.push(
      builder.vertex(position, (su + 1) / 2, (1 - sv) / 2, layer, face.shade * OCCLUSION[level], blockLight),
    );
  }

  // 沿更亮的那条对角线切，四个方向看起来才一致。
  // Cut along the brighter diagonal so the shading looks the same in every orientation.
  const split13 = occlusion[1] + occlusion[3] > occlusion[0] + occlusion[2];
  builder.quad(corners[0], corners[1], corners[2], corners[3], split13);
}

/** 任意长方体的一个面，没有环境光遮蔽。 / One face of an arbitrary box, without ambient occlusion. */
function emitBoxFace(
  builder: MeshBuilder,
  min: Vec3,
  max: Vec3,
  faceIndex: number,
  layer: number,
  uvMax: readonly [number, number],
  blockLight: number,
  sway: readonly [number, number],
  phase: number,
) {
  const face = FACES[faceIndex];
  const center: Vec3 = [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2, (min[2] + max[2]) / 2];
  const half: Vec3 = [(max[0] - min[0]) / 2, (max[1] - min[1]) / 2, (max[2] - min[2]) / 2];
  const corners = CORNERS.map(([su, sv]) => {
    const position: Vec3 = [
      center[0] + half[0] * (face.normal[0] + face.u[0] * su + face.v[0] * sv),
      center[1] + half[1] * (face.normal[1] + face.u[1] * su + face.v[1] * sv),
      center[2] + half[2] * (face.normal[2] + face.u[2] * su + face.v[2] * sv),
    ];
    const upper = position[1] > center[1];
    return builder.vertex(
      position,
      ((su + 1) / 2) * uvMax[0],
      ((1 - sv) / 2) * uvMax[1],
      layer,
      face.shade,
      blockLight,
      upper ? sway[1] : sway[0],
      phase,
    );
  });
  builder.quad(corners[0], corners[1], corners[2], corners[3]);
}

/**
 * 两片交叉的竖直贴图，正反两面都画。草丛、花和竹叶用它。
 *
 * Two crossed upright quads, drawn on both sides. Tufts, flowers and bamboo leaves use it.
 */
function emitCross(
  builder: MeshBuilder,
  centerX: number,
  centerZ: number,
  bottom: number,
  top: number,
  halfWidth: number,
  layer: number,
  blockLight: number,
  sway: readonly [number, number],
  phase: number,
) {
  const reach = halfWidth * Math.SQRT1_2;
  for (const direction of [1, -1]) {
    const fromX = centerX - reach;
    const toX = centerX + reach;
    const fromZ = centerZ - reach * direction;
    const toZ = centerZ + reach * direction;
    const a = builder.vertex([fromX, bottom, fromZ], 0, 1, layer, 0.94, blockLight, sway[0], phase);
    const b = builder.vertex([toX, bottom, toZ], 1, 1, layer, 0.94, blockLight, sway[0], phase);
    const c = builder.vertex([toX, top, toZ], 1, 0, layer, 0.94, blockLight, sway[1], phase);
    const d = builder.vertex([fromX, top, fromZ], 0, 0, layer, 0.94, blockLight, sway[1], phase);
    builder.quad(a, b, c, d);
    builder.quad(d, c, b, a);
  }
}

const STALK_HALF_WIDTH = 1.5 / 16;
const STALK_UV = 3 / 16;

/** 竹子离地 n 格处的摇摆幅度，越高摆得越大。 / Sway amplitude n blocks above the ground; higher sways more. */
function stalkSway(height: number): number {
  return 0.007 * Math.max(0, height) ** 1.5;
}

function emitStalk(builder: MeshBuilder, world: World, x: number, y: number, z: number) {
  let below = 0;
  while (world.get(x, y - below - 1, z) === BLOCKS.bamboo.id) {
    below += 1;
  }
  const centerX = x + 0.5 + (hash2(x, z, 11) - 0.5) * 0.5;
  const centerZ = z + 0.5 + (hash2(x, z, 23) - 0.5) * 0.5;
  const phase = hash2(x, z, 37) * Math.PI * 2;
  const sway: [number, number] = [stalkSway(below), stalkSway(below + 1)];
  const blockLight = brightness(world.lightAt(x, y, z));
  const min: Vec3 = [centerX - STALK_HALF_WIDTH, y, centerZ - STALK_HALF_WIDTH];
  const max: Vec3 = [centerX + STALK_HALF_WIDTH, y + 1, centerZ + STALK_HALF_WIDTH];
  const layer = tileLayer("bambooStalk");
  const above = world.get(x, y + 1, z);

  for (let faceIndex = 0; faceIndex < FACES.length; faceIndex += 1) {
    if (faceIndex === TOP && above === BLOCKS.bamboo.id) {
      continue;
    }
    if (faceIndex === BOTTOM && below > 0) {
      continue;
    }
    const vertical = faceIndex === TOP || faceIndex === BOTTOM;
    emitBoxFace(builder, min, max, faceIndex, layer, [STALK_UV, vertical ? STALK_UV : 1], blockLight, sway, phase);
  }

  // 顶上一格长大叶子，往下一格长小叶子。
  // The top block carries large leaves and the one below carries small ones.
  if (above !== BLOCKS.bamboo.id) {
    const leafSway: [number, number] = [stalkSway(below - 0.2), stalkSway(below + 1.3)];
    emitCross(builder, centerX, centerZ, y - 0.2, y + 1.3, 1.05, tileLayer("bambooLeaf"), blockLight, leafSway, phase);
  } else if (world.get(x, y + 2, z) !== BLOCKS.bamboo.id && below > 0) {
    emitCross(builder, centerX, centerZ, y, y + 1, 0.8, tileLayer("bambooLeafSmall"), blockLight, sway, phase);
  }
}

function emitWater(builder: MeshBuilder, world: World, x: number, y: number, z: number) {
  const above = world.get(x, y + 1, z);
  const surface = above === BLOCKS.water.id ? 1 : 14 / 16;
  const layer = tileLayer("water");
  const blockLight = brightness(world.lightAt(x, y, z));
  for (let faceIndex = 0; faceIndex < FACES.length; faceIndex += 1) {
    const [nx, ny, nz] = FACES[faceIndex].normal;
    const neighbour = world.get(x + nx, y + ny, z + nz);
    if (neighbour === BLOCKS.water.id || isOpaque(neighbour)) {
      continue;
    }
    emitBoxFace(builder, [x, y, z], [x + 1, y + surface, z + 1], faceIndex, layer, [1, 1], blockLight, [0, 0], 0);
  }
}

/** 把整个世界变成两份网格。世界很小，每次改动都整个重建。 / Turns the whole world into two meshes. The world is small, so every edit rebuilds it. */
export function buildMesh(world: World): WorldMesh {
  const solid = new MeshBuilder();
  const water = new MeshBuilder();

  for (let y = 0; y < WORLD_SIZE.y; y += 1) {
    for (let z = 0; z < WORLD_SIZE.z; z += 1) {
      for (let x = 0; x < WORLD_SIZE.x; x += 1) {
        const id = world.get(x, y, z);
        const block = id === AIR ? undefined : BLOCK_BY_ID[id];
        if (!block) {
          continue;
        }
        switch (block.shape) {
          case "cube": {
            const lit = world.lampsLit && block.emission > 0;
            for (let faceIndex = 0; faceIndex < FACES.length; faceIndex += 1) {
              const [nx, ny, nz] = FACES[faceIndex].normal;
              if (!isOpaque(world.get(x + nx, y + ny, z + nz))) {
                emitCubeFace(solid, world, block, x, y, z, faceIndex, lit);
              }
            }
            break;
          }
          case "liquid":
            emitWater(water, world, x, y, z);
            break;
          case "stalk":
            emitStalk(solid, world, x, y, z);
            break;
          case "cross": {
            const phase = hash2(x, z, 53) * Math.PI * 2;
            const jitterX = (hash2(x, z, 61) - 0.5) * 0.3;
            const jitterZ = (hash2(x, z, 67) - 0.5) * 0.3;
            emitCross(
              solid,
              x + 0.5 + jitterX,
              z + 0.5 + jitterZ,
              y,
              y + 1,
              0.62,
              tileLayer(block.side),
              brightness(world.lightAt(x, y, z)),
              [0, 0.035],
              phase,
            );
            break;
          }
        }
      }
    }
  }

  return { solid: solid.finish(), water: water.finish(), faces: solid.faces + water.faces };
}
