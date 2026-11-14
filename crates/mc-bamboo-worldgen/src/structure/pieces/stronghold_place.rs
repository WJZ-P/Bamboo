//! 要塞十二种结构件的 `postProcess`（`StrongholdPieces`）。
//!
//! 逐行照官方 26.3 翻译。公共设施先落在这里：
//! * `SmoothStoneSelector`：`StructurePiece.BlockSelector` 的实现，按「是否边缘」在
//!   `CRACKED_STONE_BRICKS` / `MOSSY_STONE_BRICKS` / `INFESTED_STONE_BRICKS` /
//!   `STONE_BRICKS` 之间取（边缘才取随机），非边缘是 `CAVE_AIR`；
//! * `generateSmallDoor`：四种 `SmallDoorType` 各写若干方块。
//!
//! 十二个子类见下方 `*_post_process`。箱子战利品表与方块实体不比，但随机取数照取。
//!
//! `postProcess` of the twelve stronghold pieces (`StrongholdPieces`), translated line
//! by line from vanilla 26.3. The shared machinery lives here: `SmoothStoneSelector`
//! (edge → a random stone-brick variant, interior → cave air) and `generateSmallDoor`
//! (the four `SmallDoorType`s). Chest loot tables and block entities are not compared,
//! but their random draws still happen.

use mc_bamboo_data::block_id_of_state;

use crate::random::WorldgenRandom;
use crate::structure::piece_place::{
    BlockSelector, PieceLevel, block, box_is_inside, is_air, is_interior, maybe_generate_block,
    with_property,
};
use crate::structure::pieces::HDir;
use crate::structure::state_transform::with_props;

/// 要塞结构件放置的公共状态。
///
/// Shared placement state of a stronghold piece.
pub struct StrongholdPiece<'a> {
    pub ctx: crate::structure::piece_place::PieceCtx,
    pub level: &'a mut dyn PieceLevel,
    pub chunk_bb: [i32; 6],
}

impl StrongholdPiece<'_> {
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

    /// `generateBox(..., skipAir=true, random, SMOOTH_STONE_SELECTOR)`。
    ///
    /// `generateBox(..., skipAir=true, random, SmoothStoneSelector)`.
    #[allow(clippy::too_many_arguments)]
    fn box_selected(
        &mut self,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        skip_air: bool,
        random: &WorldgenRandom,
    ) {
        let mut selector = SmoothStoneSelector::new();
        crate::structure::piece_place::generate_box_selected(
            &self.ctx,
            self.level,
            &self.chunk_bb,
            [x0, y0, z0, x1, y1, z1],
            skip_air,
            random,
            &mut selector,
        );
    }

    fn place(&mut self, state: u32, x: i32, y: i32, z: i32) {
        self.ctx
            .place_block(self.level, state, x, y, z, &self.chunk_bb);
    }
}

/// `StrongholdPieces.SmoothStoneSelector`。
///
/// `StrongholdPieces.SmoothStoneSelector`.
pub struct SmoothStoneSelector {
    next: u32,
}

impl SmoothStoneSelector {
    pub fn new() -> Self {
        Self {
            next: mc_bamboo_data::blocks::AIR,
        }
    }
}

impl Default for SmoothStoneSelector {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockSelector for SmoothStoneSelector {
    /// 边缘：一次 `nextFloat()`，按 0.2 / 0.5 / 0.55 分成四档；非边缘：洞穴空气。
    ///
    /// Edge: one `nextFloat()` split at 0.2 / 0.5 / 0.55; interior: cave air.
    fn next(&mut self, random: &WorldgenRandom, _x: i32, _y: i32, _z: i32, edge: bool) {
        self.next = if edge {
            let selection = random.next_float();
            if selection < 0.2 {
                block("minecraft:cracked_stone_bricks")
            } else if selection < 0.5 {
                block("minecraft:mossy_stone_bricks")
            } else if selection < 0.55 {
                block("minecraft:infested_stone_bricks")
            } else {
                block("minecraft:stone_bricks")
            }
        } else {
            cave_air()
        };
    }

    fn get_next(&self) -> u32 {
        self.next
    }
}

/// `CAVE_AIR`。
///
/// `CAVE_AIR`.
fn cave_air() -> u32 {
    block("minecraft:cave_air")
}

/// `StrongholdPiece.SmallDoorType`。
///
/// `StrongholdPiece.SmallDoorType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmallDoorType {
    Opening,
    WoodDoor,
    Grates,
    IronDoor,
}

/// `generateSmallDoor(level, random, chunkBB, doorType, footX, footY, footZ)`。
///
/// 四种门的方块布局逐条照抄；`WOOD_DOOR` / `IRON_DOOR` 写两格高的门（上半
/// `HALF=upper`），`IRON_DOOR` 还写两块朝北 / 朝南的石按钮。
///
/// `generateSmallDoor(...)`. The four door layouts are transcribed; `WOOD_DOOR` and
/// `IRON_DOOR` write a two-block door (upper half has `HALF=upper`), and `IRON_DOOR`
/// adds two stone buttons facing north and south.
pub fn generate_small_door(
    piece: &mut StrongholdPiece<'_>,
    door_type: SmallDoorType,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
) {
    let bricks = block("minecraft:stone_bricks");
    match door_type {
        SmallDoorType::Opening => {
            piece.box_at(
                foot_x,
                foot_y,
                foot_z,
                foot_x + 3 - 1,
                foot_y + 3 - 1,
                foot_z,
                cave_air(),
                cave_air(),
                false,
            );
        }
        SmallDoorType::WoodDoor => {
            for (dx, dy) in [(0, 0), (0, 1), (0, 2), (1, 2), (2, 2), (2, 1), (2, 0)] {
                piece.place(bricks, foot_x + dx, foot_y + dy, foot_z);
            }
            let lower = with_property(block("minecraft:oak_door"), "half", "lower")
                .unwrap_or_else(|| block("minecraft:oak_door"));
            let upper = with_property(block("minecraft:oak_door"), "half", "upper")
                .unwrap_or_else(|| block("minecraft:oak_door"));
            piece.place(lower, foot_x + 1, foot_y, foot_z);
            piece.place(upper, foot_x + 1, foot_y + 1, foot_z);
        }
        SmallDoorType::Grates => {
            piece.place(cave_air(), foot_x + 1, foot_y, foot_z);
            piece.place(cave_air(), foot_x + 1, foot_y + 1, foot_z);
            let bars = block("minecraft:iron_bars");
            let west = with_property(bars, "west", "true").unwrap_or(bars);
            let west_east = with_property(west, "east", "true").unwrap_or(west);
            let east = with_property(bars, "east", "true").unwrap_or(bars);
            piece.place(west, foot_x, foot_y, foot_z);
            piece.place(west, foot_x, foot_y + 1, foot_z);
            piece.place(west_east, foot_x, foot_y + 2, foot_z);
            piece.place(west_east, foot_x + 1, foot_y + 2, foot_z);
            piece.place(west_east, foot_x + 2, foot_y + 2, foot_z);
            piece.place(east, foot_x + 2, foot_y + 1, foot_z);
            piece.place(east, foot_x + 2, foot_y, foot_z);
        }
        SmallDoorType::IronDoor => {
            for (dx, dy) in [(0, 0), (0, 1), (0, 2), (1, 2), (2, 2), (2, 1), (2, 0)] {
                piece.place(bricks, foot_x + dx, foot_y + dy, foot_z);
            }
            let lower = with_property(block("minecraft:iron_door"), "half", "lower")
                .unwrap_or_else(|| block("minecraft:iron_door"));
            let upper = with_property(block("minecraft:iron_door"), "half", "upper")
                .unwrap_or_else(|| block("minecraft:iron_door"));
            piece.place(lower, foot_x + 1, foot_y, foot_z);
            piece.place(upper, foot_x + 1, foot_y + 1, foot_z);
            let button = block("minecraft:stone_button");
            let north = with_property(button, "facing", "north").unwrap_or(button);
            let south = with_property(button, "facing", "south").unwrap_or(button);
            piece.place(north, foot_x + 2, foot_y + 1, foot_z + 1);
            piece.place(south, foot_x + 2, foot_y + 1, foot_z - 1);
        }
    }
}

/// `randomSmallDoor`：`nextInt(5)`，0/1 → OPENING，2 → WOOD_DOOR，3 → GRATES，
/// 4 → IRON_DOOR。
///
/// `randomSmallDoor`: `nextInt(5)` mapping to the four door types.
pub fn random_small_door(random: &WorldgenRandom) -> SmallDoorType {
    match random.next_int(5) {
        2 => SmallDoorType::WoodDoor,
        3 => SmallDoorType::Grates,
        4 => SmallDoorType::IronDoor,
        _ => SmallDoorType::Opening,
    }
}

/// 从存档字符串解析门类型（`EntryDoor` 的 NBT 值是枚举名）。
///
/// Parse the door type from the saved enum name (the `EntryDoor` value).
pub fn door_type_from_name(name: &str) -> SmallDoorType {
    match name {
        "WOOD_DOOR" => SmallDoorType::WoodDoor,
        "GRATES" => SmallDoorType::Grates,
        "IRON_DOOR" => SmallDoorType::IronDoor,
        _ => SmallDoorType::Opening,
    }
}

/// 要塞用的四种常见方块（`Blocks.STONE_BRICKS` 等的缓存）。
///
/// The common stronghold blocks, cached.
struct Bricks;

impl Bricks {
    fn bricks() -> u32 {
        block("minecraft:stone_bricks")
    }
}

/// `ChestCorridor.postProcess`。
///
/// `ChestCorridor.postProcess`.
pub fn chest_corridor_post_process(
    piece: &mut StrongholdPiece<'_>,
    random: &WorldgenRandom,
    entry_door: SmallDoorType,
    has_placed_chest: &mut bool,
) {
    piece.box_selected(0, 0, 0, 4, 4, 6, true, random);
    generate_small_door(piece, entry_door, 1, 1, 0);
    generate_small_door(piece, SmallDoorType::Opening, 1, 1, 6);
    let bricks = Bricks::bricks();
    piece.box_at(3, 1, 2, 3, 1, 4, bricks, bricks, false);
    let slab = block("minecraft:stone_brick_slab");
    piece.place(slab, 3, 1, 1);
    piece.place(slab, 3, 1, 5);
    piece.place(slab, 3, 2, 2);
    piece.place(slab, 3, 2, 4);
    for z in 2..=4 {
        piece.place(slab, 2, 1, z);
    }
    let (wx, wy, wz) = piece.ctx.world_pos(3, 2, 3);
    if !*has_placed_chest && box_is_inside(&piece.chunk_bb, wx, wy, wz) {
        *has_placed_chest = true;
        create_chest(piece, random, 3, 2, 3);
    }
}

/// `createChest`：写 `CHEST` 方块并消耗一次 `nextLong()`（战利品表种子）。
/// 方块实体本身不建。
///
/// `createChest`: place a `CHEST` and consume one `nextLong()` for the loot table
/// seed. The block entity itself is not created.
fn create_chest(piece: &mut StrongholdPiece<'_>, random: &WorldgenRandom, x: i32, y: i32, z: i32) {
    let (wx, wy, wz) = piece.ctx.world_pos(x, y, z);
    if !box_is_inside(&piece.chunk_bb, wx, wy, wz) {
        return;
    }
    piece.ctx.place_block(
        piece.level,
        block("minecraft:chest"),
        x,
        y,
        z,
        &piece.chunk_bb,
    );
    random.next_long();
}

/// `FillerCorridor.postProcess`：`steps` 段石头砖通道，中间三格挖空。
///
/// `FillerCorridor.postProcess`: `steps` stone-brick slices with a three-wide hollow.
pub fn filler_corridor_post_process(piece: &mut StrongholdPiece<'_>, steps: i32) {
    let bricks = Bricks::bricks();
    for i in 0..steps {
        for x in 0..=4 {
            piece.place(bricks, x, 0, i);
        }
        for y in 1..=3 {
            piece.place(bricks, 0, y, i);
            for x in 1..=3 {
                piece.place(cave_air(), x, y, i);
            }
            piece.place(bricks, 4, y, i);
        }
        for x in 0..=4 {
            piece.place(bricks, x, 4, i);
        }
    }
}

/// `Straight.postProcess`。
///
/// 选择器盒 → 门 → 四处 0.1F 火把 → 左右两侧挖空。
///
/// `Straight.postProcess`: selector box, door, four 0.1F torches, then the two side
/// openings.
pub fn straight_post_process(
    piece: &mut StrongholdPiece<'_>,
    random: &WorldgenRandom,
    entry_door: SmallDoorType,
    left_child: bool,
    right_child: bool,
) {
    piece.box_selected(0, 0, 0, 4, 4, 6, true, random);
    generate_small_door(piece, entry_door, 1, 1, 0);
    generate_small_door(piece, SmallDoorType::Opening, 1, 1, 6);
    let east_torch = with_property(block("minecraft:wall_torch"), "facing", "east")
        .unwrap_or_else(|| block("minecraft:wall_torch"));
    let west_torch = with_property(block("minecraft:wall_torch"), "facing", "west")
        .unwrap_or_else(|| block("minecraft:wall_torch"));
    maybe_place(piece, random, 0.1, 1, 2, 1, east_torch);
    maybe_place(piece, random, 0.1, 3, 2, 1, west_torch);
    maybe_place(piece, random, 0.1, 1, 2, 5, east_torch);
    maybe_place(piece, random, 0.1, 3, 2, 5, west_torch);
    if left_child {
        piece.box_at(0, 1, 2, 0, 3, 4, cave_air(), cave_air(), false);
    }
    if right_child {
        piece.box_at(4, 1, 2, 4, 3, 4, cave_air(), cave_air(), false);
    }
}

/// `PrisonHall.postProcess`。
///
/// `PrisonHall.postProcess`.
pub fn prison_hall_post_process(
    piece: &mut StrongholdPiece<'_>,
    random: &WorldgenRandom,
    entry_door: SmallDoorType,
) {
    piece.box_selected(0, 0, 0, 8, 4, 10, true, random);
    generate_small_door(piece, entry_door, 1, 1, 0);
    piece.box_at(1, 1, 10, 3, 3, 10, cave_air(), cave_air(), false);
    // 四道竖直的选择器盒，把牢房隔开。
    // Four vertical selector boxes separating the cells.
    for z in [1, 3, 7, 9] {
        piece.box_selected(4, 1, z, 4, 3, z, false, random);
    }
    let bars = block("minecraft:iron_bars");
    let ns = with_props(bars, &[("north", "true"), ("south", "true")]);
    let ns_east = with_props(
        bars,
        &[("north", "true"), ("south", "true"), ("east", "true")],
    );
    let we = with_props(bars, &[("west", "true"), ("east", "true")]);
    for y in 1..=3 {
        piece.place(ns, 4, y, 4);
        piece.place(ns_east, 4, y, 5);
        piece.place(ns, 4, y, 6);
        piece.place(we, 5, y, 5);
        piece.place(we, 6, y, 5);
        piece.place(we, 7, y, 5);
    }
    piece.place(ns, 4, 3, 2);
    piece.place(ns, 4, 3, 8);
    // 两扇朝西的铁门（各两格高）。
    // Two west-facing iron doors, two blocks tall each.
    let door = block("minecraft:iron_door");
    let bottom = with_props(door, &[("facing", "west"), ("half", "lower")]);
    let top = with_props(door, &[("facing", "west"), ("half", "upper")]);
    piece.place(bottom, 4, 1, 2);
    piece.place(top, 4, 2, 2);
    piece.place(bottom, 4, 1, 8);
    piece.place(top, 4, 2, 8);
}

/// `RightTurn.postProcess`。
///
/// `RightTurn.postProcess`.
pub fn right_turn_post_process(
    piece: &mut StrongholdPiece<'_>,
    random: &WorldgenRandom,
    entry_door: SmallDoorType,
    orientation: Option<HDir>,
) {
    piece.box_selected(0, 0, 0, 4, 4, 4, true, random);
    generate_small_door(piece, entry_door, 1, 1, 0);
    match orientation {
        Some(HDir::North | HDir::East) => {
            piece.box_at(0, 1, 1, 0, 3, 3, cave_air(), cave_air(), false);
        }
        _ => {
            piece.box_at(4, 1, 1, 4, 3, 3, cave_air(), cave_air(), false);
        }
    }
}

/// `RoomCrossing.postProcess`，三种 `type` 各一份完整布局。
///
/// `RoomCrossing.postProcess` with all three `type` layouts.
pub fn room_crossing_post_process(
    piece: &mut StrongholdPiece<'_>,
    random: &WorldgenRandom,
    entry_door: SmallDoorType,
    extra_type: i32,
) {
    piece.box_selected(0, 0, 0, 10, 6, 10, true, random);
    generate_small_door(piece, entry_door, 4, 1, 0);
    piece.box_at(4, 1, 10, 6, 3, 10, cave_air(), cave_air(), false);
    piece.box_at(0, 1, 4, 0, 3, 6, cave_air(), cave_air(), false);
    piece.box_at(10, 1, 4, 10, 3, 6, cave_air(), cave_air(), false);
    let bricks = Bricks::bricks();
    let slab = block("minecraft:smooth_stone_slab");
    match extra_type {
        0 => {
            for y in 1..=3 {
                piece.place(bricks, 5, y, 5);
            }
            let west_torch = with_property(block("minecraft:wall_torch"), "facing", "west")
                .unwrap_or_else(|| block("minecraft:wall_torch"));
            let east_torch = with_property(block("minecraft:wall_torch"), "facing", "east")
                .unwrap_or_else(|| block("minecraft:wall_torch"));
            let south_torch = with_property(block("minecraft:wall_torch"), "facing", "south")
                .unwrap_or_else(|| block("minecraft:wall_torch"));
            let north_torch = with_property(block("minecraft:wall_torch"), "facing", "north")
                .unwrap_or_else(|| block("minecraft:wall_torch"));
            piece.place(west_torch, 4, 3, 5);
            piece.place(east_torch, 6, 3, 5);
            piece.place(south_torch, 5, 3, 4);
            piece.place(north_torch, 5, 3, 6);
            for (x, z) in [
                (4, 4),
                (4, 5),
                (4, 6),
                (6, 4),
                (6, 5),
                (6, 6),
                (5, 4),
                (5, 6),
            ] {
                piece.place(slab, x, 1, z);
            }
        }
        1 => {
            for i in 0..5 {
                piece.place(bricks, 3, 1, 3 + i);
                piece.place(bricks, 7, 1, 3 + i);
                piece.place(bricks, 3 + i, 1, 3);
                piece.place(bricks, 3 + i, 1, 7);
            }
            for y in 1..=3 {
                piece.place(bricks, 5, y, 5);
            }
            piece.place(block("minecraft:water"), 5, 4, 5);
        }
        2 => {
            let cobble = block("minecraft:cobblestone");
            for z in 1..=9 {
                piece.place(cobble, 1, 3, z);
                piece.place(cobble, 9, 3, z);
            }
            for x in 1..=9 {
                piece.place(cobble, x, 3, 1);
                piece.place(cobble, x, 3, 9);
            }
            for (x, y, z) in [
                (5, 1, 4),
                (5, 1, 6),
                (5, 3, 4),
                (5, 3, 6),
                (4, 1, 5),
                (6, 1, 5),
                (4, 3, 5),
                (6, 3, 5),
            ] {
                piece.place(cobble, x, y, z);
            }
            for y in 1..=3 {
                piece.place(cobble, 4, y, 4);
                piece.place(cobble, 6, y, 4);
                piece.place(cobble, 4, y, 6);
                piece.place(cobble, 6, y, 6);
            }
            piece.place(block("minecraft:wall_torch"), 5, 3, 5);
            let planks = block("minecraft:oak_planks");
            for z in 2..=8 {
                piece.place(planks, 2, 3, z);
                piece.place(planks, 3, 3, z);
                if z <= 3 || z >= 7 {
                    piece.place(planks, 4, 3, z);
                    piece.place(planks, 5, 3, z);
                    piece.place(planks, 6, 3, z);
                }
                piece.place(planks, 7, 3, z);
                piece.place(planks, 8, 3, z);
            }
            let ladder = with_property(block("minecraft:ladder"), "facing", "west")
                .unwrap_or_else(|| block("minecraft:ladder"));
            for y in 1..=3 {
                piece.place(ladder, 9, y, 3);
            }
            create_chest(piece, random, 3, 4, 8);
        }
        _ => {}
    }
}

/// `StairsDown.postProcess`（`StartPiece` 只是 `isSource` 不同，方块完全相同）。
///
/// 螺旋下降的阶梯：石砖 / 台阶交替，不是六层楼梯。
///
/// `StairsDown.postProcess` (`StartPiece` differs only by `isSource`; the blocks are
/// identical). A spiralling stone-brick and slab staircase, not six stacked stairs.
pub fn stairs_down_post_process(
    piece: &mut StrongholdPiece<'_>,
    random: &WorldgenRandom,
    entry_door: SmallDoorType,
) {
    piece.box_selected(0, 0, 0, 4, 10, 4, true, random);
    generate_small_door(piece, entry_door, 1, 7, 0);
    generate_small_door(piece, SmallDoorType::Opening, 1, 1, 4);
    let bricks = Bricks::bricks();
    let slab = block("minecraft:smooth_stone_slab");
    for (state, x, y, z) in [
        (bricks, 2, 6, 1),
        (bricks, 1, 5, 1),
        (slab, 1, 6, 1),
        (bricks, 1, 5, 2),
        (bricks, 1, 4, 3),
        (slab, 1, 5, 3),
        (bricks, 2, 4, 3),
        (bricks, 3, 3, 3),
        (slab, 3, 4, 3),
        (bricks, 3, 3, 2),
        (bricks, 3, 2, 1),
        (slab, 3, 3, 1),
        (bricks, 2, 2, 1),
        (bricks, 1, 1, 1),
        (slab, 1, 2, 1),
        (bricks, 1, 1, 2),
        (slab, 1, 1, 3),
    ] {
        piece.place(state, x, y, z);
    }
}

/// `StraightStairsDown.postProcess`。
///
/// `StraightStairsDown.postProcess`.
pub fn straight_stairs_down_post_process(
    piece: &mut StrongholdPiece<'_>,
    random: &WorldgenRandom,
    entry_door: SmallDoorType,
) {
    piece.box_selected(0, 0, 0, 4, 10, 7, true, random);
    generate_small_door(piece, entry_door, 1, 7, 0);
    generate_small_door(piece, SmallDoorType::Opening, 1, 1, 7);
    let stairs = with_property(block("minecraft:cobblestone_stairs"), "facing", "south")
        .unwrap_or_else(|| block("minecraft:cobblestone_stairs"));
    let bricks = Bricks::bricks();
    for i in 0..6 {
        piece.place(stairs, 1, 6 - i, 1 + i);
        piece.place(stairs, 2, 6 - i, 1 + i);
        piece.place(stairs, 3, 6 - i, 1 + i);
        if i < 5 {
            piece.place(bricks, 1, 5 - i, 1 + i);
            piece.place(bricks, 2, 5 - i, 1 + i);
            piece.place(bricks, 3, 5 - i, 1 + i);
        }
    }
}

/// `FiveCrossing.postProcess`。
///
/// `FiveCrossing.postProcess`.
pub fn five_crossing_post_process(
    piece: &mut StrongholdPiece<'_>,
    random: &WorldgenRandom,
    entry_door: SmallDoorType,
    flags: (bool, bool, bool, bool),
) {
    let (left_low, left_high, right_low, right_high) = flags;
    piece.box_selected(0, 0, 0, 9, 8, 10, true, random);
    generate_small_door(piece, entry_door, 4, 3, 0);
    if left_low {
        piece.box_at(0, 3, 1, 0, 5, 3, cave_air(), cave_air(), false);
    }
    if right_low {
        piece.box_at(9, 3, 1, 9, 5, 3, cave_air(), cave_air(), false);
    }
    if left_high {
        piece.box_at(0, 5, 7, 0, 7, 9, cave_air(), cave_air(), false);
    }
    if right_high {
        piece.box_at(9, 5, 7, 9, 7, 9, cave_air(), cave_air(), false);
    }
    piece.box_at(5, 1, 10, 7, 3, 10, cave_air(), cave_air(), false);
    piece.box_selected(1, 2, 1, 8, 2, 6, false, random);
    piece.box_selected(4, 1, 5, 4, 4, 9, false, random);
    piece.box_selected(8, 1, 5, 8, 4, 9, false, random);
    piece.box_selected(1, 4, 7, 3, 4, 9, false, random);
    piece.box_selected(1, 3, 5, 3, 3, 6, false, random);
    let slab = block("minecraft:smooth_stone_slab");
    piece.box_at(1, 3, 4, 3, 3, 4, slab, slab, false);
    piece.box_at(1, 4, 6, 3, 4, 6, slab, slab, false);
    piece.box_selected(5, 1, 7, 7, 1, 8, false, random);
    piece.box_at(5, 1, 9, 7, 1, 9, slab, slab, false);
    piece.box_at(5, 2, 7, 7, 2, 7, slab, slab, false);
    piece.box_at(4, 5, 7, 4, 5, 9, slab, slab, false);
    piece.box_at(8, 5, 7, 8, 5, 9, slab, slab, false);
    let double_slab = with_property(slab, "type", "double").unwrap_or(slab);
    piece.box_at(5, 5, 7, 7, 5, 9, double_slab, double_slab, false);
    let torch = with_property(block("minecraft:wall_torch"), "facing", "south")
        .unwrap_or_else(|| block("minecraft:wall_torch"));
    piece.place(torch, 6, 5, 6);
}

/// `Library.postProcess`（`isTall` 决定高度与书架 / 栏杆层）。
///
/// `Library.postProcess` (`isTall` selects the height and the shelf/fence layers).
pub fn library_post_process(
    piece: &mut StrongholdPiece<'_>,
    random: &WorldgenRandom,
    entry_door: SmallDoorType,
    is_tall: bool,
) {
    let current_height = if is_tall { 11 } else { 6 };
    piece.box_selected(0, 0, 0, 13, current_height - 1, 14, true, random);
    generate_small_door(piece, entry_door, 4, 1, 0);
    let cobweb = block("minecraft:cobweb");
    crate::structure::piece_place::generate_maybe_box(
        &piece.ctx,
        piece.level,
        &piece.chunk_bb,
        random,
        0.07,
        [2, 1, 1, 11, 4, 13],
        cobweb,
        cobweb,
        false,
        false,
    );
    let planks = block("minecraft:oak_planks");
    let shelf = block("minecraft:bookshelf");
    let east_torch = with_property(block("minecraft:wall_torch"), "facing", "east")
        .unwrap_or_else(|| block("minecraft:wall_torch"));
    let west_torch = with_property(block("minecraft:wall_torch"), "facing", "west")
        .unwrap_or_else(|| block("minecraft:wall_torch"));

    for d in 1..=13 {
        if (d - 1) % 4 == 0 {
            piece.box_at(1, 1, d, 1, 4, d, planks, planks, false);
            piece.box_at(12, 1, d, 12, 4, d, planks, planks, false);
            piece.place(east_torch, 2, 3, d);
            piece.place(west_torch, 11, 3, d);
            if is_tall {
                piece.box_at(1, 6, d, 1, 9, d, planks, planks, false);
                piece.box_at(12, 6, d, 12, 9, d, planks, planks, false);
            }
        } else {
            piece.box_at(1, 1, d, 1, 4, d, shelf, shelf, false);
            piece.box_at(12, 1, d, 12, 4, d, shelf, shelf, false);
            if is_tall {
                piece.box_at(1, 6, d, 1, 9, d, shelf, shelf, false);
                piece.box_at(12, 6, d, 12, 9, d, shelf, shelf, false);
            }
        }
    }

    let mut d = 3;
    while d < 12 {
        piece.box_at(3, 1, d, 4, 3, d, shelf, shelf, false);
        piece.box_at(6, 1, d, 7, 3, d, shelf, shelf, false);
        piece.box_at(9, 1, d, 10, 3, d, shelf, shelf, false);
        d += 2;
    }

    if is_tall {
        piece.box_at(1, 5, 1, 3, 5, 13, planks, planks, false);
        piece.box_at(10, 5, 1, 12, 5, 13, planks, planks, false);
        piece.box_at(4, 5, 1, 9, 5, 2, planks, planks, false);
        piece.box_at(4, 5, 12, 9, 5, 13, planks, planks, false);
        piece.place(planks, 9, 5, 11);
        piece.place(planks, 8, 5, 11);
        piece.place(planks, 9, 5, 10);

        let fence = block("minecraft:oak_fence");
        let we_fence = with_props(fence, &[("west", "true"), ("east", "true")]);
        let ns_fence = with_props(fence, &[("north", "true"), ("south", "true")]);
        let ns_we = with_props(
            fence,
            &[
                ("north", "true"),
                ("south", "true"),
                ("west", "true"),
                ("east", "true"),
            ],
        );

        piece.box_at(3, 6, 3, 3, 6, 11, ns_fence, ns_fence, false);
        piece.box_at(10, 6, 3, 10, 6, 9, ns_fence, ns_fence, false);
        piece.box_at(4, 6, 2, 9, 6, 2, we_fence, we_fence, false);
        piece.box_at(4, 6, 12, 7, 6, 12, we_fence, we_fence, false);

        let ne = with_props(fence, &[("north", "true"), ("east", "true")]);
        piece.place(ne, 3, 6, 2);
        let se = with_props(fence, &[("south", "true"), ("east", "true")]);
        piece.place(se, 3, 6, 12);
        let nw = with_props(fence, &[("north", "true"), ("west", "true")]);
        piece.place(nw, 10, 6, 2);
        let sw = with_props(fence, &[("south", "true"), ("west", "true")]);

        for i in 0..=2 {
            piece.place(sw, 8 + i, 6, 12 - i);
            if i != 2 {
                piece.place(ne, 8 + i, 6, 11 - i);
            }
        }

        let ladder = with_property(block("minecraft:ladder"), "facing", "south")
            .unwrap_or_else(|| block("minecraft:ladder"));
        for y in 1..=7 {
            piece.place(ladder, 10, y, 13);
        }

        let e_fence = with_property(fence, "east", "true").unwrap_or(fence);
        let w_fence = with_property(fence, "west", "true").unwrap_or(fence);
        piece.place(e_fence, 6, 9, 7);
        piece.place(w_fence, 7, 9, 7);
        piece.place(e_fence, 6, 8, 7);
        piece.place(w_fence, 7, 8, 7);
        piece.place(ns_we, 6, 7, 7);
        piece.place(ns_we, 7, 7, 7);
        piece.place(e_fence, 5, 7, 7);
        piece.place(w_fence, 8, 7, 7);
        piece.place(ne, 6, 7, 6);
        piece.place(se, 6, 7, 8);
        piece.place(nw, 7, 7, 6);
        piece.place(sw, 7, 7, 8);

        let torch = block("minecraft:torch");
        piece.place(torch, 5, 8, 7);
        piece.place(torch, 8, 8, 7);
        piece.place(torch, 6, 8, 6);
        piece.place(torch, 6, 8, 8);
        piece.place(torch, 7, 8, 6);
        piece.place(torch, 7, 8, 8);
    }

    create_chest(piece, random, 3, 3, 5);
    if is_tall {
        piece.place(cave_air(), 12, 9, 1);
        create_chest(piece, random, 12, 8, 1);
    }
}

/// `PortalRoom.postProcess`。
///
/// `PortalRoom.postProcess`.
/// `PortalRoom.postProcess`。
///
/// 12 块末地传送门框架各取一次 `nextFloat() > 0.9F` 决定有没有眼睛；全部有眼睛时铺九格
/// `END_PORTAL`。蠹虫刷怪笼的方块要写，实体 id 不比。
///
/// `PortalRoom.postProcess`. Twelve end-portal frames each draw `nextFloat() > 0.9F` for
/// the eye; when every frame has one, nine `END_PORTAL` blocks go in. The silverfish
/// spawner block is placed, but its entity id is not compared.
pub fn portal_room_post_process(
    piece: &mut StrongholdPiece<'_>,
    random: &WorldgenRandom,
    has_placed_spawner: &mut bool,
) {
    // 官方固定用 `GRATES` 门，不看 `entryDoor`。
    // Vanilla always uses a `GRATES` door here, ignoring `entryDoor`.
    piece.box_selected(0, 0, 0, 10, 7, 15, false, random);
    generate_small_door(piece, SmallDoorType::Grates, 4, 1, 0);
    // 天花板四条边。
    // The four ceiling edges.
    piece.box_selected(1, 6, 1, 1, 6, 14, false, random);
    piece.box_selected(9, 6, 1, 9, 6, 14, false, random);
    piece.box_selected(2, 6, 1, 8, 6, 2, false, random);
    piece.box_selected(2, 6, 14, 8, 6, 14, false, random);
    // 入口两侧的平台与熔岩。
    // The platforms and lava beside the entrance.
    piece.box_selected(1, 1, 1, 2, 1, 4, false, random);
    piece.box_selected(8, 1, 1, 9, 1, 4, false, random);
    let lava = block("minecraft:lava");
    piece.box_at(1, 1, 1, 1, 1, 3, lava, lava, false);
    piece.box_at(9, 1, 1, 9, 1, 3, lava, lava, false);
    // 传送门厅地板与中央熔岩池。
    // The portal-hall floor and its central lava pool.
    piece.box_selected(3, 1, 8, 7, 1, 12, false, random);
    piece.box_at(4, 1, 9, 6, 1, 11, lava, lava, false);

    let bars = block("minecraft:iron_bars");
    let ns_bars = with_props(bars, &[("north", "true"), ("south", "true")]);
    let we_bars = with_props(bars, &[("west", "true"), ("east", "true")]);
    let mut z = 3;
    while z < 14 {
        piece.box_at(0, 3, z, 0, 4, z, ns_bars, ns_bars, false);
        piece.box_at(10, 3, z, 10, 4, z, ns_bars, ns_bars, false);
        z += 2;
    }
    let mut x = 2;
    while x < 9 {
        piece.box_at(x, 3, 15, x, 4, 15, we_bars, we_bars, false);
        x += 2;
    }

    // 通往传送门的三级台阶。
    // The three steps up to the portal.
    piece.box_selected(4, 1, 5, 6, 1, 7, false, random);
    piece.box_selected(4, 2, 6, 6, 2, 7, false, random);
    piece.box_selected(4, 3, 7, 6, 3, 7, false, random);
    let stairs = with_property(block("minecraft:stone_brick_stairs"), "facing", "north")
        .unwrap_or_else(|| block("minecraft:stone_brick_stairs"));
    for x in 4..=6 {
        piece.place(stairs, x, 1, 4);
        piece.place(stairs, x, 2, 5);
        piece.place(stairs, x, 3, 6);
    }

    // 12 块框架的眼睛：每块一次 `nextFloat()`，全部命中就生成传送门。
    // Eyes on the twelve frames: one `nextFloat()` each; all hit opens the portal.
    let frame = block("minecraft:end_portal_frame");
    let mut eyes = [false; 12];
    let mut all_eyes = true;
    for eye in &mut eyes {
        *eye = random.next_float() > 0.9;
        all_eyes &= *eye;
    }
    let frame_with = |facing: &str, has_eye: bool| {
        with_props(
            frame,
            &[
                ("facing", facing),
                ("eye", if has_eye { "true" } else { "false" }),
            ],
        )
    };
    piece.place(frame_with("north", eyes[0]), 4, 3, 8);
    piece.place(frame_with("north", eyes[1]), 5, 3, 8);
    piece.place(frame_with("north", eyes[2]), 6, 3, 8);
    piece.place(frame_with("south", eyes[3]), 4, 3, 12);
    piece.place(frame_with("south", eyes[4]), 5, 3, 12);
    piece.place(frame_with("south", eyes[5]), 6, 3, 12);
    piece.place(frame_with("east", eyes[6]), 3, 3, 9);
    piece.place(frame_with("east", eyes[7]), 3, 3, 10);
    piece.place(frame_with("east", eyes[8]), 3, 3, 11);
    piece.place(frame_with("west", eyes[9]), 7, 3, 9);
    piece.place(frame_with("west", eyes[10]), 7, 3, 10);
    piece.place(frame_with("west", eyes[11]), 7, 3, 11);

    if all_eyes {
        let portal = block("minecraft:end_portal");
        for z in [9, 10, 11] {
            for x in [4, 5, 6] {
                piece.place(portal, x, 3, z);
            }
        }
    }

    let (wx, wy, wz) = piece.ctx.world_pos(5, 3, 6);
    if !*has_placed_spawner && box_is_inside(&piece.chunk_bb, wx, wy, wz) {
        *has_placed_spawner = true;
        piece
            .level
            .set_block(wx, wy, wz, block("minecraft:spawner"));
    }
}
/// 供对拍比较用的方块名（调试与断言）。
///
/// Block name for oracle comparisons (debug and assertions).
pub fn same_block(state: u32, name: &str) -> bool {
    mc_bamboo_data::block_by_name(name)
        .is_some_and(|b| block_id_of_state(b.default_state) == block_id_of_state(state))
}

/// 未使用的占位，保持接口对称（`is_air` / `is_interior` 由子类实现按需调用）。
///
/// Kept for symmetry; subclasses call `is_air` / `is_interior` as needed.
pub fn _unused(state: u32) -> bool {
    is_air(state)
}

/// 未使用：`maybe_generate_block` 的转发，保持与官方 `maybeGenerateBlock` 同名。
///
/// Forwarder for `maybeGenerateBlock`.
#[allow(clippy::too_many_arguments)]
pub fn maybe_place(
    piece: &mut StrongholdPiece<'_>,
    random: &WorldgenRandom,
    probability: f32,
    x: i32,
    y: i32,
    z: i32,
    state: u32,
) {
    maybe_generate_block(
        &piece.ctx,
        piece.level,
        &piece.chunk_bb,
        random,
        probability,
        x,
        y,
        z,
        state,
    );
}

/// 未使用：`isInterior` 的转发。
///
/// Forwarder for `isInterior`.
pub fn interior(piece: &StrongholdPiece<'_>, x: i32, y: i32, z: i32) -> bool {
    is_interior(&piece.ctx, piece.level, x, y, z, &piece.chunk_bb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::piece_place::PieceLevel;
    use std::collections::BTreeMap;

    /// 一个测试用的稀疏世界。
    ///
    /// A sparse test world.
    struct TestLevel {
        blocks: BTreeMap<(i32, i32, i32), u32>,
    }

    impl TestLevel {
        fn new() -> Self {
            Self {
                blocks: BTreeMap::new(),
            }
        }
    }

    impl PieceLevel for TestLevel {
        fn get_block(&self, x: i32, y: i32, z: i32) -> u32 {
            self.blocks
                .get(&(x, y, z))
                .copied()
                .unwrap_or(mc_bamboo_data::blocks::AIR)
        }

        fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> bool {
            self.blocks.insert((x, y, z), state);
            true
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn max_y(&self) -> i32 {
            319
        }

        fn ocean_floor_height(&self, _x: i32, _z: i32) -> i32 {
            0
        }
    }

    fn piece<'a>(level: &'a mut TestLevel, bb: [i32; 6]) -> StrongholdPiece<'a> {
        StrongholdPiece {
            ctx: crate::structure::piece_place::PieceCtx::new(bb, None),
            level,
            chunk_bb: [-64, -64, -64, 64, 320, 64],
        }
    }

    #[test]
    fn random_small_door_matches_vanilla_weights() {
        use crate::structure::pieces::make_random;
        let random = make_random(42, 0, 0);
        let mut counts = [0usize; 4];
        for _ in 0..400 {
            let door = random_small_door(&random);
            let index = match door {
                SmallDoorType::Opening => 0,
                SmallDoorType::WoodDoor => 1,
                SmallDoorType::Grates => 2,
                SmallDoorType::IronDoor => 3,
            };
            counts[index] += 1;
        }
        // 0/1 都映射到 OPENING，所以它大约占 40%。
        // Both 0 and 1 map to OPENING, so it takes about 40% of the draws.
        assert!(counts[0] > counts[1], "{counts:?}");
        assert!(
            counts[1] > 0 && counts[2] > 0 && counts[3] > 0,
            "{counts:?}"
        );
    }

    #[test]
    fn openings_are_three_wide_cave_air() {
        let mut level = TestLevel::new();
        let mut p = piece(&mut level, [0, 0, 0, 8, 8, 8]);
        generate_small_door(&mut p, SmallDoorType::Opening, 1, 1, 0);
        for x in 1..=3 {
            for y in 1..=3 {
                assert_eq!(
                    level.get_block(x, y, 0),
                    block("minecraft:cave_air"),
                    "{x},{y}"
                );
            }
        }
    }

    #[test]
    fn wood_door_writes_bricks_and_a_two_high_door() {
        let mut level = TestLevel::new();
        let mut p = piece(&mut level, [0, 0, 0, 8, 8, 8]);
        generate_small_door(&mut p, SmallDoorType::WoodDoor, 1, 1, 0);
        let bricks = block("minecraft:stone_bricks");
        assert_eq!(level.get_block(1, 1, 0), bricks);
        assert_eq!(level.get_block(1, 2, 0), bricks);
        assert_eq!(level.get_block(1, 3, 0), bricks);
        // 门在 (2,1,0) / (2,2,0)。
        // The door sits at (2,1,0) / (2,2,0).
        let door = block("minecraft:oak_door");
        assert_eq!(
            mc_bamboo_data::block_id_of_state(level.get_block(2, 1, 0)),
            mc_bamboo_data::block_id_of_state(door)
        );
        assert_eq!(
            mc_bamboo_data::block_id_of_state(level.get_block(2, 2, 0)),
            mc_bamboo_data::block_id_of_state(door)
        );
    }

    #[test]
    fn grates_use_iron_bars_with_side_connections() {
        let mut level = TestLevel::new();
        let mut p = piece(&mut level, [0, 0, 0, 8, 8, 8]);
        generate_small_door(&mut p, SmallDoorType::Grates, 1, 1, 0);
        let bars = block("minecraft:iron_bars");
        assert_eq!(
            mc_bamboo_data::block_id_of_state(level.get_block(1, 1, 0)),
            mc_bamboo_data::block_id_of_state(bars)
        );
        assert_eq!(
            mc_bamboo_data::block_id_of_state(level.get_block(3, 1, 0)),
            mc_bamboo_data::block_id_of_state(bars)
        );
        // 中间两格是洞穴空气。
        // The two middle cells are cave air.
        assert_eq!(level.get_block(2, 1, 0), block("minecraft:cave_air"));
        assert_eq!(level.get_block(2, 2, 0), block("minecraft:cave_air"));
    }

    #[test]
    fn iron_door_adds_two_stone_buttons() {
        let mut level = TestLevel::new();
        let mut p = piece(&mut level, [0, 0, 0, 8, 8, 8]);
        generate_small_door(&mut p, SmallDoorType::IronDoor, 1, 1, 0);
        let button = block("minecraft:stone_button");
        assert_eq!(
            mc_bamboo_data::block_id_of_state(level.get_block(3, 2, 1)),
            mc_bamboo_data::block_id_of_state(button)
        );
        assert_eq!(
            mc_bamboo_data::block_id_of_state(level.get_block(3, 2, -1)),
            mc_bamboo_data::block_id_of_state(button)
        );
    }

    #[test]
    fn smooth_stone_selector_picks_interior_air_and_edge_bricks() {
        use crate::structure::pieces::make_random;
        let random = make_random(7, 0, 0);
        let mut selector = SmoothStoneSelector::new();
        selector.next(&random, 0, 0, 0, false);
        assert_eq!(selector.get_next(), block("minecraft:cave_air"));
        selector.next(&random, 0, 0, 0, true);
        let edge = selector.get_next();
        let candidates = [
            block("minecraft:cracked_stone_bricks"),
            block("minecraft:mossy_stone_bricks"),
            block("minecraft:infested_stone_bricks"),
            block("minecraft:stone_bricks"),
        ];
        assert!(candidates.contains(&edge), "unexpected edge block {edge}");
    }

    #[test]
    fn filler_corridor_writes_steps_slices() {
        let mut level = TestLevel::new();
        let mut p = piece(&mut level, [0, 0, 0, 8, 8, 8]);
        filler_corridor_post_process(&mut p, 2);
        let bricks = block("minecraft:stone_bricks");
        assert_eq!(level.get_block(0, 0, 0), bricks);
        assert_eq!(level.get_block(4, 4, 1), bricks);
        assert_eq!(level.get_block(2, 2, 0), block("minecraft:cave_air"));
        // `steps` 之外不写。
        // Nothing beyond `steps`.
        assert_eq!(level.get_block(0, 0, 2), mc_bamboo_data::blocks::AIR);
    }
}
