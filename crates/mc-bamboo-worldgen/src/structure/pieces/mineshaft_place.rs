//! 废弃矿井四种结构件的 `postProcess`（`MineshaftPieces`，普通 / 恶地两种材质）。
//!
//! 逐行照官方 26.3 翻译：
//! * `MineShaftCorridor.postProcess`：两条空气通道、蜘蛛走廊的概率空气、每段
//!   `placeSupport`（支撑木 + 随机顶板 + 火把）、八处 `maybePlaceCobWeb`、两次
//!   `createChest`（1%）、蜘蛛刷怪笼、地板 `setPlanksBlock`、两端支撑柱、矿车轨道；
//! * `MineShaftCrossing.postProcess`：两层 / 单层两种挖空、四根支撑柱、地板；
//! * `MineShaftStairs.postProcess`：三段空气；
//! * `MineShaftRoom.postProcess`：顶部空气、每个入口的空气、上半球。
//!
//! 都先过 `isInInvalidLocation`（`MINESHAFT_BLOCKING` 群系或盒边界有液体就整件跳过）。
//! 箱子 / 矿车实体 / 刷怪笼实体的战利品表与实体数据不比，但它们的随机取数照取。
//!
//! `postProcess` of the four mineshaft pieces (`MineshaftPieces`, normal and mesa),
//! translated line by line from vanilla 26.3: the corridor's tunnels, cobwebs,
//! supports, rail and `isSupportingBox` gate; the crossing's two excavation shapes,
//! four pillars and floor; the stairs' three air boxes; the room's ceiling,
//! entrances and upper half sphere. All start with `isInInvalidLocation`. Chest loot
//! tables, minecart entities and spawner entity data are not compared, but their
//! random draws still happen.

use mc_bamboo_data::{block_by_name, block_id_of_state};

use crate::random::WorldgenRandom;
use crate::structure::piece_place::{
    Dir, PieceCtx, PieceLevel, block, box_is_inside, is_air, is_face_sturdy_full, is_interior,
    is_lava, is_replaceable_by_structures, is_solid_render, maybe_generate_block, with_property,
};

/// `MineshaftStructure.Type`：材质三件套 + `ordinal`。
///
/// `MineshaftStructure.Type`: the three block states plus `ordinal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaftType {
    Normal,
    Mesa,
}

impl ShaftType {
    /// `type.ordinal()`。
    pub fn ordinal(self) -> i32 {
        match self {
            Self::Normal => 0,
            Self::Mesa => 1,
        }
    }

    /// `getWoodState()`。
    pub fn wood(self) -> u32 {
        block(match self {
            Self::Normal => "minecraft:oak_log",
            Self::Mesa => "minecraft:dark_oak_log",
        })
    }

    /// `getPlanksState()`。
    pub fn planks(self) -> u32 {
        block(match self {
            Self::Normal => "minecraft:oak_planks",
            Self::Mesa => "minecraft:dark_oak_planks",
        })
    }

    /// `getFenceState()`。
    pub fn fence(self) -> u32 {
        block(match self {
            Self::Normal => "minecraft:oak_fence",
            Self::Mesa => "minecraft:dark_oak_fence",
        })
    }
}

fn cave_air() -> u32 {
    block("minecraft:cave_air")
}

/// `MineshaftPieces.MineShaftPiece.canBeReplaced`：不是本类型的三件套、也不是铁链时可换。
///
/// `MineshaftPieces.MineShaftPiece.canBeReplaced`: replaceable unless the block is one
/// of this type's wood blocks or an iron chain.
pub fn can_replace(shaft_type: ShaftType, state: u32) -> bool {
    let id = block_id_of_state(state);
    let is = |state: u32| block_id_of_state(state) == id;
    if is(shaft_type.planks()) || is(shaft_type.wood()) || is(shaft_type.fence()) {
        return false;
    }
    match block_by_name("minecraft:iron_chain") {
        Some(chain) => block_id_of_state(chain.default_state) != id,
        None => true,
    }
}

/// `isInInvalidLocation`：`MINESHAFT_BLOCKING` 群系，或盒边界任一格是液体。
///
/// `isInInvalidLocation`: a `MINESHAFT_BLOCKING` biome, or any liquid on the box
/// border.
pub fn is_in_invalid_location(
    piece_bb: &[i32; 6],
    chunk_bb: &[i32; 6],
    level: &dyn PieceLevel,
    biome_blocking: bool,
) -> bool {
    if biome_blocking {
        return true;
    }
    let x0 = (piece_bb[0] - 1).max(chunk_bb[0]);
    let y0 = (piece_bb[1] - 1).max(chunk_bb[1]);
    let z0 = (piece_bb[2] - 1).max(chunk_bb[2]);
    let x1 = (piece_bb[3] + 1).min(chunk_bb[3]);
    let y1 = (piece_bb[4] + 1).min(chunk_bb[4]);
    let z1 = (piece_bb[5] + 1).min(chunk_bb[5]);

    let liquid =
        |x: i32, y: i32, z: i32| crate::structure::piece_place::is_liquid(level.get_block(x, y, z));
    for x in x0..=x1 {
        for z in z0..=z1 {
            if liquid(x, y0, z) || liquid(x, y1, z) {
                return true;
            }
        }
    }
    for x in x0..=x1 {
        for y in y0..=y1 {
            if liquid(x, y, z0) || liquid(x, y, z1) {
                return true;
            }
        }
    }
    for z in z0..=z1 {
        for y in y0..=y1 {
            if liquid(x0, y, z) || liquid(x1, y, z) {
                return true;
            }
        }
    }
    false
}

/// 矿井结构件放置的公共状态。
///
/// Shared placement state of a mineshaft piece.
pub struct ShaftPiece<'a> {
    pub ctx: PieceCtx,
    pub shaft: ShaftType,
    pub level: &'a mut dyn PieceLevel,
    pub chunk_bb: [i32; 6],
    /// [`invalid_location_probe`] 那一格的群系是否带 `MINESHAFT_BLOCKING` 标签，
    /// 由调用方查好传进来。
    ///
    /// Whether the biome at [`invalid_location_probe`] carries the `MINESHAFT_BLOCKING`
    /// tag; looked up by the caller.
    pub biome_blocking: bool,
}

/// `isInInvalidLocation` 查群系的那一格：结构件盒外扩一格、裁到 `chunkBB` 之后的中心
/// （三轴都是 Java 的整数除法）。
///
/// The position `isInInvalidLocation` reads the biome at: the centre of the piece box
/// grown by one and clipped to `chunkBB` (Java integer division on every axis).
pub fn invalid_location_probe(piece_bb: &[i32; 6], chunk_bb: &[i32; 6]) -> (i32, i32, i32) {
    let x0 = (piece_bb[0] - 1).max(chunk_bb[0]);
    let y0 = (piece_bb[1] - 1).max(chunk_bb[1]);
    let z0 = (piece_bb[2] - 1).max(chunk_bb[2]);
    let x1 = (piece_bb[3] + 1).min(chunk_bb[3]);
    let y1 = (piece_bb[4] + 1).min(chunk_bb[4]);
    let z1 = (piece_bb[5] + 1).min(chunk_bb[5]);
    ((x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2)
}

impl ShaftPiece<'_> {
    /// `MineShaftPiece.isInInvalidLocation(level, chunkBB)`。
    ///
    /// `MineShaftPiece.isInInvalidLocation(level, chunkBB)`.
    fn is_in_invalid_location(&self) -> bool {
        is_in_invalid_location(
            &self.ctx.bb,
            &self.chunk_bb,
            &*self.level,
            self.biome_blocking,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn box_at(
        &mut self,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge: u32,
        fill: u32,
        skip_air: bool,
    ) {
        crate::structure::piece_place::generate_box(
            &self.ctx,
            self.level,
            &self.chunk_bb,
            [x0, y0, z0, x1, y1, z1],
            edge,
            fill,
            skip_air,
        );
    }
}

/// `MineShaftCorridor` 的可变部分（`postProcess` 会改 `hasPlacedSpider`）。
///
/// Mutable part of a `MineShaftCorridor` (`postProcess` may set `hasPlacedSpider`).
pub struct CorridorPlace {
    pub num_sections: i32,
    pub has_rails: bool,
    pub spider_corridor: bool,
    pub has_placed_spider: bool,
}

/// `MineShaftCorridor.postProcess`。
///
/// `MineShaftCorridor.postProcess`.
pub fn corridor_post_process(
    piece: &mut ShaftPiece<'_>,
    random: &WorldgenRandom,
    state: &mut CorridorPlace,
) {
    if piece.is_in_invalid_location() {
        return;
    }
    let length = state.num_sections * 5 - 1;
    piece.box_at(0, 0, 0, 2, 1, length, cave_air(), cave_air(), false);
    crate::structure::piece_place::generate_maybe_box(
        &piece.ctx,
        piece.level,
        &piece.chunk_bb,
        random,
        0.8,
        [0, 2, 0, 2, 2, length],
        cave_air(),
        cave_air(),
        false,
        false,
    );
    if state.spider_corridor {
        crate::structure::piece_place::generate_maybe_box(
            &piece.ctx,
            piece.level,
            &piece.chunk_bb,
            random,
            0.6,
            [0, 0, 0, 2, 1, length],
            block("minecraft:cobweb"),
            cave_air(),
            false,
            true,
        );
    }

    for section in 0..state.num_sections {
        let z = 2 + section * 5;
        place_support(piece, random, 0, 0, z, 2, 2);
        maybe_place_cobweb(piece, random, 0.1, 0, 2, z - 1);
        maybe_place_cobweb(piece, random, 0.1, 2, 2, z - 1);
        maybe_place_cobweb(piece, random, 0.1, 0, 2, z + 1);
        maybe_place_cobweb(piece, random, 0.1, 2, 2, z + 1);
        maybe_place_cobweb(piece, random, 0.05, 0, 2, z - 2);
        maybe_place_cobweb(piece, random, 0.05, 2, 2, z - 2);
        maybe_place_cobweb(piece, random, 0.05, 0, 2, z + 2);
        maybe_place_cobweb(piece, random, 0.05, 2, 2, z + 2);

        if random.next_int(100) == 0 {
            corridor_chest(piece, random, 2, 0, z - 1);
        }
        if random.next_int(100) == 0 {
            corridor_chest(piece, random, 0, 0, z + 1);
        }

        if state.spider_corridor && !state.has_placed_spider {
            let new_z = z - 1 + random.next_int(3);
            let (wx, wy, wz) = piece.ctx.world_pos(1, 0, new_z);
            if box_is_inside(&piece.chunk_bb, wx, wy, wz)
                && is_interior(&piece.ctx, piece.level, 1, 0, new_z, &piece.chunk_bb)
            {
                state.has_placed_spider = true;
                piece
                    .level
                    .set_block(wx, wy, wz, block("minecraft:spawner"));
            }
        }
    }

    for x in 0..=2 {
        for z in 0..=length {
            set_planks_block(piece, x, -1, z);
        }
    }

    place_double_lower_or_upper_support(piece, 0, -1, 2);
    if state.num_sections > 1 {
        place_double_lower_or_upper_support(piece, 0, -1, length - 2);
    }

    if state.has_rails {
        let rail = rail_state(true);
        for z in 0..=length {
            let floor = piece.ctx.get_block(piece.level, &piece.chunk_bb, 1, -1, z);
            if !is_air(floor) && is_solid_render(floor) {
                let probability = if is_interior(&piece.ctx, piece.level, 1, 0, z, &piece.chunk_bb)
                {
                    0.7
                } else {
                    0.9
                };
                maybe_generate_block(
                    &piece.ctx,
                    piece.level,
                    &piece.chunk_bb,
                    random,
                    probability,
                    1,
                    0,
                    z,
                    rail,
                );
            }
        }
    }
}

/// `MineShaftCorridor.createChest`：空格且下方非空时放一段铁轨和一辆运输矿车。
/// 矿车实体这里不生成，但它的战利品种子 `random.nextLong()` 照取。返回值同官方。
///
/// `MineShaftCorridor.createChest`: a rail plus a chest minecart on an empty block whose
/// neighbour below is not empty. The minecart entity is not spawned here, but its loot
/// seed `random.nextLong()` is still drawn. Returns what vanilla returns.
fn corridor_chest(
    piece: &mut ShaftPiece<'_>,
    random: &WorldgenRandom,
    x: i32,
    y: i32,
    z: i32,
) -> bool {
    let (wx, wy, wz) = piece.ctx.world_pos(x, y, z);
    if !box_is_inside(&piece.chunk_bb, wx, wy, wz) {
        return false;
    }
    if !is_air(piece.level.get_block(wx, wy, wz)) || is_air(piece.level.get_block(wx, wy - 1, wz)) {
        return false;
    }
    // `random.nextBoolean()` 选走向，随后 `random.nextLong()` 给战利品表。
    // `nextBoolean` picks the shape; the `nextLong` seeds the loot table.
    let north_south = random.next_boolean();
    let rail = rail_state(north_south);
    piece
        .ctx
        .place_block(piece.level, rail, x, y, z, &piece.chunk_bb);
    random.next_long();
    true
}

/// `Blocks.RAIL` + `RailBlock.SHAPE`。
///
/// `Blocks.RAIL` with `RailBlock.SHAPE`.
fn rail_state(north_south: bool) -> u32 {
    let rail = block("minecraft:rail");
    with_property(
        rail,
        "shape",
        if north_south {
            "north_south"
        } else {
            "east_west"
        },
    )
    .unwrap_or(rail)
}

fn place_double_lower_or_upper_support(piece: &mut ShaftPiece<'_>, x: i32, y: i32, z: i32) {
    let planks = block_id_of_state(piece.shaft.planks());
    for dx in [0, 2] {
        let here = piece
            .ctx
            .get_block(piece.level, &piece.chunk_bb, x + dx, y, z);
        if block_id_of_state(here) == planks {
            fill_pillar_down_or_chain_up(piece, piece.shaft.wood(), x + dx, y, z);
        }
    }
}

/// `fillPillarDownOrChainUp`：往下找可放柱子的面，或往上挂铁链。
///
/// `fillPillarDownOrChainUp`: find a face below to stand a pillar on, or hang a chain.
fn fill_pillar_down_or_chain_up(piece: &mut ShaftPiece<'_>, pillar: u32, x: i32, y: i32, z: i32) {
    let (wx, wy, wz) = piece.ctx.world_pos(x, y, z);
    if !box_is_inside(&piece.chunk_bb, wx, wy, wz) {
        return;
    }
    let mut check_below = true;
    let mut check_above = true;
    let mut distance = 1;
    while check_below || check_above {
        if check_below {
            let by = wy - distance;
            let below = piece.level.get_block(wx, by, wz);
            let empty = is_replaceable_by_structures(below) && !is_lava(below);
            if !empty && is_face_sturdy_full(below, Dir::Up) {
                fill_column_between(piece.level, pillar, wx, wz, by + 1, wy);
                return;
            }
            check_below = distance <= 20 && empty && by > piece.level.min_y() + 1;
        }
        if check_above {
            let ay = wy + distance;
            let above = piece.level.get_block(wx, ay, wz);
            let empty = is_replaceable_by_structures(above);
            if !empty && crate::structure::piece_place::can_hang_chain_below(above) {
                piece.level.set_block(wx, wy + 1, wz, piece.shaft.fence());
                fill_column_between(
                    piece.level,
                    block("minecraft:iron_chain"),
                    wx,
                    wz,
                    wy + 2,
                    wy + distance,
                );
                return;
            }
            check_above = distance <= 50 && empty && ay < piece.level.max_y();
        }
        distance += 1;
    }
}

/// `fillColumnBetween(level, state, pos, bottomInclusive, topExclusive)`。
fn fill_column_between(
    level: &mut dyn PieceLevel,
    state: u32,
    x: i32,
    z: i32,
    bottom_inclusive: i32,
    top_exclusive: i32,
) {
    for y in bottom_inclusive..top_exclusive {
        level.set_block(x, y, z, state);
    }
}

/// `placeSupport`：先 `isSupportingBox`，再两侧栅栏 + 顶板 / 火把。
///
/// `placeSupport`: the `isSupportingBox` gate, then the two fence sides plus a plank
/// ceiling or wall torches.
fn place_support(
    piece: &mut ShaftPiece<'_>,
    random: &WorldgenRandom,
    x0: i32,
    y0: i32,
    z: i32,
    y1: i32,
    x1: i32,
) {
    if !is_supporting_box(piece, x0, x1, y1, z) {
        return;
    }
    let planks = piece.shaft.planks();
    let fence = piece.shaft.fence();
    let west_fence = with_property(fence, "west", "true").unwrap_or(fence);
    let east_fence = with_property(fence, "east", "true").unwrap_or(fence);
    piece.box_at(x0, y0, z, x0, y1 - 1, z, west_fence, cave_air(), false);
    piece.box_at(x1, y0, z, x1, y1 - 1, z, east_fence, cave_air(), false);
    if random.next_int(4) == 0 {
        piece.box_at(x0, y1, z, x0, y1, z, planks, cave_air(), false);
        piece.box_at(x1, y1, z, x1, y1, z, planks, cave_air(), false);
    } else {
        piece.box_at(x0, y1, z, x1, y1, z, planks, cave_air(), false);
        let south_torch = with_property(block("minecraft:wall_torch"), "facing", "south")
            .unwrap_or_else(|| block("minecraft:wall_torch"));
        let north_torch = with_property(block("minecraft:wall_torch"), "facing", "north")
            .unwrap_or_else(|| block("minecraft:wall_torch"));
        maybe_generate_block(
            &piece.ctx,
            piece.level,
            &piece.chunk_bb,
            random,
            0.05,
            x0 + 1,
            y1,
            z - 1,
            south_torch,
        );
        maybe_generate_block(
            &piece.ctx,
            piece.level,
            &piece.chunk_bb,
            random,
            0.05,
            x0 + 1,
            y1,
            z + 1,
            north_torch,
        );
    }
}

/// `isSupportingBox`：`y1 + 1` 那一排只要有一格空气就不算「顶得住」。
///
/// `isSupportingBox`: any air at `y1 + 1` means the ceiling cannot hold.
fn is_supporting_box(piece: &ShaftPiece<'_>, x0: i32, x1: i32, y1: i32, z: i32) -> bool {
    for x in x0..=x1 {
        let state = piece
            .ctx
            .get_block(piece.level, &piece.chunk_bb, x, y1 + 1, z);
        if is_air(state) {
            return false;
        }
    }
    true
}

/// `maybePlaceCobWeb`：`isInterior` → `nextFloat()` → 相邻坚固面。
///
/// `maybePlaceCobWeb`: `isInterior` → `nextFloat()` → sturdy neighbours.
fn maybe_place_cobweb(
    piece: &mut ShaftPiece<'_>,
    random: &WorldgenRandom,
    probability: f32,
    x: i32,
    y: i32,
    z: i32,
) {
    if !is_interior(&piece.ctx, piece.level, x, y, z, &piece.chunk_bb) {
        return;
    }
    if random.next_float() >= probability {
        return;
    }
    if !has_sturdy_neighbours(piece, x, y, z, 2) {
        return;
    }
    piece.ctx.place_block(
        piece.level,
        block("minecraft:cobweb"),
        x,
        y,
        z,
        &piece.chunk_bb,
    );
}

/// `hasSturdyNeighbours`：按 `Direction.values()` 顺序（DOWN/UP/NORTH/SOUTH/WEST/EAST）。
/// 方向是**世界**方向：官方先 `getWorldPos` 再 `worldPos.move(direction)`，不经过结构件
/// 朝向变换。
///
/// `hasSturdyNeighbours` iterating `Direction.values()`
/// (DOWN/UP/NORTH/SOUTH/WEST/EAST). The directions are **world** directions: vanilla
/// calls `getWorldPos` first and then `worldPos.move(direction)`, bypassing the piece
/// orientation.
fn has_sturdy_neighbours(piece: &ShaftPiece<'_>, x: i32, y: i32, z: i32, count: i32) -> bool {
    let mut sturdy = 0;
    let (px, py, pz) = piece.ctx.world_pos(x, y, z);
    for (dx, dy, dz, opposite) in [
        (0, -1, 0, Dir::Up),
        (0, 1, 0, Dir::Down),
        (0, 0, -1, Dir::South),
        (0, 0, 1, Dir::North),
        (-1, 0, 0, Dir::East),
        (1, 0, 0, Dir::West),
    ] {
        let (wx, wy, wz) = (px + dx, py + dy, pz + dz);
        if box_is_inside(&piece.chunk_bb, wx, wy, wz)
            && is_face_sturdy_full(piece.level.get_block(wx, wy, wz), opposite)
        {
            sturdy += 1;
            if sturdy >= count {
                return true;
            }
        }
    }
    false
}

/// `setPlanksBlock`：只有 `isInterior` 且上面不是满固体时才铺木板。
///
/// `setPlanksBlock`: lay planks only when `isInterior` and the block above is not a
/// full solid face.
fn set_planks_block(piece: &mut ShaftPiece<'_>, x: i32, y: i32, z: i32) {
    if !is_interior(&piece.ctx, piece.level, x, y, z, &piece.chunk_bb) {
        return;
    }
    let (wx, wy, wz) = piece.ctx.world_pos(x, y, z);
    let existing = piece.level.get_block(wx, wy, wz);
    if !is_face_sturdy_full(existing, Dir::Up) {
        piece.level.set_block(wx, wy, wz, piece.shaft.planks());
    }
}

/// `MineShaftCrossing.postProcess`。
///
/// 两层时挖上下两层，单层时挖一个贯通的空间；然后四角支撑柱，最后整块地板铺木板。
///
/// `MineShaftCrossing.postProcess`. Two-floored crossings excavate both floors,
/// single ones a full-height space; then four corner pillars and the plank floor.
pub fn crossing_post_process(piece: &mut ShaftPiece<'_>, two_floored: bool) {
    if piece.is_in_invalid_location() {
        return;
    }
    let bb = piece.ctx.bb;
    if two_floored {
        piece.box_at(
            bb[0] + 1,
            bb[1],
            bb[2],
            bb[3] - 1,
            bb[1] + 3 - 1,
            bb[5],
            cave_air(),
            cave_air(),
            false,
        );
        piece.box_at(
            bb[0],
            bb[1],
            bb[2] + 1,
            bb[3],
            bb[1] + 3 - 1,
            bb[5] - 1,
            cave_air(),
            cave_air(),
            false,
        );
        piece.box_at(
            bb[0] + 1,
            bb[4] - 2,
            bb[2],
            bb[3] - 1,
            bb[4],
            bb[5],
            cave_air(),
            cave_air(),
            false,
        );
        piece.box_at(
            bb[0],
            bb[4] - 2,
            bb[2] + 1,
            bb[3],
            bb[4],
            bb[5] - 1,
            cave_air(),
            cave_air(),
            false,
        );
        piece.box_at(
            bb[0] + 1,
            bb[1] + 3,
            bb[2] + 1,
            bb[3] - 1,
            bb[1] + 3,
            bb[5] - 1,
            cave_air(),
            cave_air(),
            false,
        );
    } else {
        piece.box_at(
            bb[0] + 1,
            bb[1],
            bb[2],
            bb[3] - 1,
            bb[4],
            bb[5],
            cave_air(),
            cave_air(),
            false,
        );
        piece.box_at(
            bb[0],
            bb[1],
            bb[2] + 1,
            bb[3],
            bb[4],
            bb[5] - 1,
            cave_air(),
            cave_air(),
            false,
        );
    }

    place_support_pillar(piece, bb[0] + 1, bb[1], bb[2] + 1, bb[4]);
    place_support_pillar(piece, bb[0] + 1, bb[1], bb[5] - 1, bb[4]);
    place_support_pillar(piece, bb[3] - 1, bb[1], bb[2] + 1, bb[4]);
    place_support_pillar(piece, bb[3] - 1, bb[1], bb[5] - 1, bb[4]);
    let y = bb[1] - 1;

    for x in bb[0]..=bb[3] {
        for z in bb[2]..=bb[5] {
            set_planks_block(piece, x, y, z);
        }
    }
}

/// `MineShaftCrossing.placeSupportPillar`：上方有东西顶着才整根铺木板。
///
/// `MineShaftCrossing.placeSupportPillar`: a plank pillar only when something holds
/// the top.
fn place_support_pillar(piece: &mut ShaftPiece<'_>, x: i32, y0: i32, z: i32, y1: i32) {
    let above = piece
        .ctx
        .get_block(piece.level, &piece.chunk_bb, x, y1 + 1, z);
    if !is_air(above) {
        let planks = piece.shaft.planks();
        piece.box_at(x, y0, z, x, y1, z, planks, cave_air(), false);
    }
}

/// `MineShaftStairs.postProcess`。
///
/// `MineShaftStairs.postProcess`.
pub fn stairs_post_process(piece: &mut ShaftPiece<'_>) {
    if piece.is_in_invalid_location() {
        return;
    }
    piece.box_at(0, 5, 0, 2, 7, 1, cave_air(), cave_air(), false);
    piece.box_at(0, 0, 7, 2, 2, 8, cave_air(), cave_air(), false);
    for i in 0..5 {
        let bottom = 5 - i - i32::from(i < 4);
        piece.box_at(
            0,
            bottom,
            2 + i,
            2,
            7 - i,
            2 + i,
            cave_air(),
            cave_air(),
            false,
        );
    }
}

/// `MineShaftRoom.postProcess`。
///
/// 顶部空气（`minY+1` 到 `min(minY+3, maxY)`）、每个入口的最后三层空气、再挖一个上半球。
///
/// `MineShaftRoom.postProcess`: ceiling air from `minY+1` to `min(minY+3, maxY)`, the
/// last three layers of every entrance, then an upper half sphere.
pub fn room_post_process(piece: &mut ShaftPiece<'_>, entrances: &[[i32; 6]]) {
    if piece.is_in_invalid_location() {
        return;
    }
    let bb = piece.ctx.bb;
    piece.box_at(
        bb[0],
        bb[1] + 1,
        bb[2],
        bb[3],
        (bb[1] + 3).min(bb[4]),
        bb[5],
        cave_air(),
        cave_air(),
        false,
    );
    for entrance in entrances {
        piece.box_at(
            entrance[0],
            entrance[4] - 2,
            entrance[2],
            entrance[3],
            entrance[4],
            entrance[5],
            cave_air(),
            cave_air(),
            false,
        );
    }
    crate::structure::piece_place::generate_upper_half_sphere(
        &piece.ctx,
        piece.level,
        &piece.chunk_bb,
        [bb[0], bb[1] + 4, bb[2], bb[3], bb[4], bb[5]],
        cave_air(),
        false,
    );
}
