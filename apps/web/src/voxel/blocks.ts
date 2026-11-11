import type { TileName } from "./tiles";

/**
 * 方块的形状。cube 是整格方块；liquid 是半透明的水；stalk 是细竹竿；
 * cross 是两片交叉的贴图，草丛和花用它。
 *
 * Block shapes. `cube` fills its cell, `liquid` is translucent water, `stalk` is a thin
 * bamboo pole, and `cross` is two crossed quads used by grass tufts and flowers.
 */
export type BlockShape = "cube" | "liquid" | "stalk" | "cross";

export interface BlockDef {
  readonly id: number;
  readonly key: string;
  readonly name: { readonly zh: string; readonly en: string };
  readonly shape: BlockShape;
  readonly top: TileName;
  readonly side: TileName;
  readonly bottom: TileName;
  /** 发光等级，0 到 15。只在夜里点亮。 / Light emission, 0 to 15. Lit only at night. */
  readonly emission: number;
  /** 发光时换用的贴图。 / The tile used while emitting light. */
  readonly litTile?: TileName;
}

function cube(
  id: number,
  key: string,
  zh: string,
  en: string,
  top: TileName,
  side: TileName = top,
  bottom: TileName = side,
): BlockDef {
  return { id, key, name: { zh, en }, shape: "cube", top, side, bottom, emission: 0 };
}

export const AIR = 0;

export const BLOCKS = {
  grass: cube(1, "grass", "草方块", "Grass Block", "grassTop", "grassSide", "dirt"),
  dirt: cube(2, "dirt", "泥土", "Dirt", "dirt"),
  stone: cube(3, "stone", "石头", "Stone", "stone"),
  sand: cube(4, "sand", "沙子", "Sand", "sand"),
  water: {
    ...cube(5, "water", "水", "Water", "water"),
    shape: "liquid",
  } as BlockDef,
  planks: cube(6, "planks", "竹板", "Bamboo Planks", "planks"),
  lamp: {
    ...cube(7, "lamp", "灯", "Lamp", "lampOff"),
    emission: 15,
    litTile: "lampOn",
  } as BlockDef,
  oreDiamond: cube(8, "oreDiamond", "钻石矿", "Diamond Ore", "oreDiamond"),
  oreGold: cube(9, "oreGold", "金矿", "Gold Ore", "oreGold"),
  oreRedstone: cube(10, "oreRedstone", "红石矿", "Redstone Ore", "oreRedstone"),
  oreEmerald: cube(11, "oreEmerald", "绿宝石矿", "Emerald Ore", "oreEmerald"),
  bamboo: {
    ...cube(12, "bamboo", "竹子", "Bamboo", "bambooStalk"),
    shape: "stalk",
  } as BlockDef,
  tuft: {
    ...cube(13, "tuft", "草丛", "Grass", "tuft"),
    shape: "cross",
  } as BlockDef,
  flowerRed: {
    ...cube(14, "flowerRed", "红花", "Red Flower", "flowerRed"),
    shape: "cross",
  } as BlockDef,
  flowerYellow: {
    ...cube(15, "flowerYellow", "黄花", "Yellow Flower", "flowerYellow"),
    shape: "cross",
  } as BlockDef,
} as const;

/** 按 id 查方块；0 号是空气，没有定义。 / Blocks by id. Id 0 is air and has no definition. */
export const BLOCK_BY_ID: readonly (BlockDef | undefined)[] = (() => {
  const table: (BlockDef | undefined)[] = [undefined];
  for (const block of Object.values(BLOCKS)) {
    table[block.id] = block;
  }
  return table;
})();

/** 这个 id 是不是挡光、挡视线的整格方块。 / Whether the id is a full cube that blocks light and sight. */
export function isOpaque(id: number): boolean {
  return BLOCK_BY_ID[id]?.shape === "cube";
}

/** 快捷栏里能放下去的九种方块。 / The nine blocks offered in the hotbar. */
export const HOTBAR: readonly BlockDef[] = [
  BLOCKS.grass,
  BLOCKS.dirt,
  BLOCKS.stone,
  BLOCKS.sand,
  BLOCKS.planks,
  BLOCKS.lamp,
  BLOCKS.oreDiamond,
  BLOCKS.oreGold,
  BLOCKS.bamboo,
];
