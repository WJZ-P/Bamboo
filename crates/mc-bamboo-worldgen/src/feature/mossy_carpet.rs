//! `MossyCarpetBlock.placeAt`：苍白苔藓地毯（`pale_moss_carpet`）的放置。
//!
//! `SimpleBlockFeature` 对 `MossyCarpetBlock` 走 `MossyCarpetBlock.placeAt(level, pos,
//! level.getRandom(), 2)`，它会**额外消费 `nextBoolean`** 决定上层的侧面是否保留。
//! 顺序照官方：先按 `getUpdatedState(..., createSides=true)` 放下层，再用
//! `createTopperWithSideChance` 判上层；上层不是空气时放上去，然后**再更新一次下层**。
//!
//! 随机数：`createTopperWithSideChance` 对每个非 `NONE` 的侧面调一次
//! `sideSurvivalTest.getAsBoolean()`（即 `random::nextBoolean`），顺序是
//! `Direction.Plane.HORIZONTAL`（NORTH / EAST / SOUTH / WEST）。
//!
//! `MossyCarpetBlock.placeAt` for pale moss carpet. `SimpleBlockFeature` routes
//! `MossyCarpetBlock` through `MossyCarpetBlock.placeAt`, which consumes extra
//! `nextBoolean` draws: one per non-`NONE` side in `Direction.Plane.HORIZONTAL` order.

use mc_bamboo_core::Direction;
use mc_bamboo_data::{block_of_state, facts, state_facts};
use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{
    multiface::can_attach_to,
    state::{parse_state, with_property},
    world::FeatureWorld,
};

type Pos = (i32, i32, i32);

/// `MossyCarpetBlock.PROPERTY_BY_DIRECTION`，顺序是 `Direction.Plane.HORIZONTAL`。
/// `MossyCarpetBlock.PROPERTY_BY_DIRECTION` in `Direction.Plane.HORIZONTAL` order.
const HORIZONTAL: [(Direction, &str); 4] = [
    (Direction::North, "north"),
    (Direction::East, "east"),
    (Direction::South, "south"),
    (Direction::West, "west"),
];

/// `WallSide` 的三个取值，`ordinal` 和导出数据的属性值顺序一致。
/// The three `WallSide` values; the ordinal matches the exported property order.
const NONE: &str = "none";
const LOW: &str = "low";
const TALL: &str = "tall";

fn offset(direction: Direction) -> (i32, i32, i32) {
    match direction {
        Direction::Down => (0, -1, 0),
        Direction::Up => (0, 1, 0),
        Direction::North => (0, 0, -1),
        Direction::South => (0, 0, 1),
        Direction::West => (-1, 0, 0),
        Direction::East => (1, 0, 0),
    }
}

/// 一个已解析的苍白苔藓地毯状态集合。
/// The parsed pale-moss-carpet state constants.
pub struct MossyCarpet {
    /// `Blocks.PALE_MOSS_CARPET.defaultBlockState()`。
    /// `Blocks.PALE_MOSS_CARPET.defaultBlockState()`.
    default: u32,
    air: u32,
}

impl MossyCarpet {
    /// 解析。配置里只给出一个 `to_place` 状态（`{"id": "minecraft:pale_moss_carpet"}`），
    /// 它的方块类必须是 `MossyCarpetBlock`。
    ///
    /// Parse. The config names one `to_place` state (`{"id": "minecraft:pale_moss_carpet"}`);
    /// its block class must be `MossyCarpetBlock`.
    pub fn parse(state: u32) -> Option<Self> {
        let block = block_of_state(state)?;
        if block.name != "minecraft:pale_moss_carpet" {
            return None;
        }
        let air: Value = serde_json::from_str("\"minecraft:air\"").ok()?;
        Some(Self {
            default: block.default_state,
            air: parse_state(&air).ok()?,
        })
    }

    /// `MossyCarpetBlock.getPropertyForFace(direction)` 的属性名。
    /// The property name of `MossyCarpetBlock.getPropertyForFace(direction)`.
    fn property(direction: Direction) -> Option<&'static str> {
        HORIZONTAL
            .iter()
            .find(|(dir, _)| *dir == direction)
            .map(|(_, name)| *name)
    }

    fn side(state: u32, direction: Direction) -> Option<&'static str> {
        let name = Self::property(direction)?;
        [NONE, LOW, TALL]
            .into_iter()
            .find(|value| with_property(state, name, value) == Some(state))
    }

    fn set_side(&self, state: u32, direction: Direction, value: &str) -> Option<u32> {
        with_property(state, Self::property(direction)?, value)
    }

    fn is_base(&self, state: u32) -> bool {
        with_property(state, "bottom", "true") == Some(state)
    }

    /// `MossyCarpetBlock.hasFaces`。
    /// `MossyCarpetBlock.hasFaces`.
    fn has_faces(&self, state: u32) -> bool {
        if self.is_base(state) {
            return true;
        }
        HORIZONTAL
            .iter()
            .any(|(dir, _)| Self::side(state, *dir) != Some(NONE))
    }

    /// `MossyCarpetBlock.canSupportAtFace`：`UP` 恒 false，其余走
    /// `MultifaceBlock.canAttachTo`。
    /// `MossyCarpetBlock.canSupportAtFace`: false for `UP`, otherwise
    /// `MultifaceBlock.canAttachTo`.
    fn can_support_at_face(
        &self,
        world: &FeatureWorld<'_>,
        pos: Pos,
        direction: Direction,
    ) -> bool {
        if direction == Direction::Up {
            return false;
        }
        let (dx, dy, dz) = offset(direction);
        let neighbour = world.get_block(pos.0 + dx, pos.1 + dy, pos.2 + dz);
        can_attach_to(neighbour, direction)
    }

    /// `MossyCarpetBlock.getUpdatedState(state, level, pos, createSides)`。
    ///
    /// 只读方块，不消费随机数。
    ///
    /// `MossyCarpetBlock.getUpdatedState(state, level, pos, createSides)`. Reads blocks
    /// only; consumes no random.
    fn updated_state(
        &self,
        world: &FeatureWorld<'_>,
        mut state: u32,
        pos: Pos,
        create_sides: bool,
    ) -> u32 {
        let mut above_state: Option<u32> = None;
        let mut below_state: Option<u32> = None;
        let create_sides = create_sides || self.is_base(state);

        for (direction, _) in HORIZONTAL {
            let supported = self.can_support_at_face(world, pos, direction);
            let current = Self::side(state, direction).unwrap_or(NONE);
            let mut side = if supported {
                if create_sides { LOW } else { current }
            } else {
                NONE
            };
            if side == LOW {
                let above =
                    *above_state.get_or_insert_with(|| world.get_block(pos.0, pos.1 + 1, pos.2));
                // 上层是「非 BASE 的苍白苔藓地毯」且该侧面不是 NONE → 变 TALL。
                // A non-base pale moss carpet above with a non-NONE side makes this TALL.
                if self.is_own(above)
                    && Self::side(above, direction).is_some_and(|s| s != NONE)
                    && !self.is_base(above)
                {
                    side = TALL;
                }
                if !self.is_base(state) {
                    let below = *below_state
                        .get_or_insert_with(|| world.get_block(pos.0, pos.1 - 1, pos.2));
                    if self.is_own(below) && Self::side(below, direction) == Some(NONE) {
                        side = NONE;
                    }
                }
            }
            state = self.set_side(state, direction, side).unwrap_or(state);
        }
        state
    }

    fn is_own(&self, state: u32) -> bool {
        block_of_state(state).is_some_and(|block| block.name == "minecraft:pale_moss_carpet")
    }

    /// `MossyCarpetBlock.createTopperWithSideChance`：上层候选状态，`AIR` 表示不放。
    ///
    /// `side_survival` 每遇到一个非 `NONE` 侧面被调用一次（`nextBoolean`）。
    ///
    /// `MossyCarpetBlock.createTopperWithSideChance`: the topper state, `AIR` meaning
    /// "place nothing". `side_survival` runs once per non-`NONE` side (`nextBoolean`).
    fn topper_with_side_chance(
        &self,
        world: &FeatureWorld<'_>,
        pos: Pos,
        side_survival: &mut dyn FnMut() -> bool,
    ) -> u32 {
        let above_pos = (pos.0, pos.1 + 1, pos.2);
        let above_previous = world.get_block(above_pos.0, above_pos.1, above_pos.2);
        let above_is_carpet = self.is_own(above_previous);
        let replaceable = state_facts(above_previous) & facts::REPLACEABLE != 0;
        // `(!isCarpetAbove || !abovePreviousState.getValue(BASE)) &&
        //  (isCarpetAbove || abovePreviousState.canBeReplaced())`
        let above_is_base = above_is_carpet && self.is_base(above_previous);
        if above_is_base || (!above_is_carpet && !replaceable) {
            return self.air;
        }

        let no_base = with_property(self.default, "bottom", "false").unwrap_or(self.default);
        let mut above_state = self.updated_state(world, no_base, above_pos, true);
        for (direction, _) in HORIZONTAL {
            let side = Self::side(above_state, direction).unwrap_or(NONE);
            if side != NONE && !side_survival() {
                above_state = self
                    .set_side(above_state, direction, NONE)
                    .unwrap_or(above_state);
            }
        }
        if self.has_faces(above_state) && above_state != above_previous {
            above_state
        } else {
            self.air
        }
    }

    /// `MossyCarpetBlock.placeAt(level, pos, random, 2)`。
    ///
    /// 返回是否放了方块（下层一定放）。
    ///
    /// `MossyCarpetBlock.placeAt(level, pos, random, 2)`. Returns whether anything was
    /// placed (the base layer always is).
    pub fn place_at(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        pos: Pos,
    ) -> bool {
        // 先把下层状态算出来并放下（`setBlock(pos, adjustedCarpetLayer, updateType)`）。
        // 用 world 的只读视图算，算完再写。
        // Compute the base layer, write it, then compute the topper.
        let adjusted = self.updated_state(world, self.default, pos, true);
        world.set_block_with_heightmaps(pos.0, pos.1, pos.2, adjusted);

        let mut side_survival = || random.next_boolean();
        let topper = self.topper_with_side_chance(world, pos, &mut side_survival);
        if topper == self.air {
            return true;
        }
        world.set_block_with_heightmaps(pos.0, pos.1 + 1, pos.2, topper);
        // `getUpdatedState(adjustedCarpetLayer, level, pos, true)` 再放一次下层。
        // Recompute and write the base layer once more.
        let updated = self.updated_state(world, adjusted, pos, true);
        world.set_block_with_heightmaps(pos.0, pos.1, pos.2, updated);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pale() -> u32 {
        mc_bamboo_data::block_by_name("minecraft:pale_moss_carpet")
            .unwrap()
            .default_state
    }

    #[test]
    fn default_state_is_base_with_all_sides_none() {
        let carpet = MossyCarpet::parse(pale()).unwrap();
        assert!(carpet.is_base(carpet.default));
        for (direction, _) in HORIZONTAL {
            assert_eq!(MossyCarpet::side(carpet.default, direction), Some(NONE));
        }
        assert!(carpet.has_faces(carpet.default));
    }

    #[test]
    fn non_base_without_sides_has_no_faces() {
        let carpet = MossyCarpet::parse(pale()).unwrap();
        let no_base = with_property(carpet.default, "bottom", "false").unwrap();
        assert!(!carpet.is_base(no_base));
        assert!(!carpet.has_faces(no_base));
        let low = carpet.set_side(no_base, Direction::North, LOW).unwrap();
        assert!(carpet.has_faces(low));
    }

    #[test]
    fn decode_the_three_states_from_the_failing_chunk() {
        // 官方 `35584` = `{east:low, south:low, bottom:true}`；
        // 官方空气对应 `35662` = `{bottom:false, 所有 side:none}`。
        // Official `35584` is `{east:low, south:low, bottom:true}`; `35662` is
        // `{bottom:false, every side:none}`.
        let carpet = MossyCarpet::parse(pale()).unwrap();
        let east_low_south_low = 35584;
        assert!(carpet.is_base(east_low_south_low));
        assert_eq!(
            MossyCarpet::side(east_low_south_low, Direction::East),
            Some(LOW)
        );
        assert_eq!(
            MossyCarpet::side(east_low_south_low, Direction::South),
            Some(LOW)
        );
        assert_eq!(
            MossyCarpet::side(east_low_south_low, Direction::North),
            Some(NONE)
        );
        assert_eq!(
            MossyCarpet::side(east_low_south_low, Direction::West),
            Some(NONE)
        );

        let no_base = 35662;
        assert!(!carpet.is_base(no_base));
        // `35662` 是 `{east:low, bottom:false}` —— 有 east 面，所以 `hasFaces` 为真。
        // 「全 none 且无 base」的情形由 `non_base_without_sides_has_no_faces` 覆盖。
        // `35662` is `{east:low, bottom:false}`: it has an east face, so `hasFaces` holds.
        // The "every side none and no base" case is covered by
        // `non_base_without_sides_has_no_faces`.
        assert_eq!(MossyCarpet::side(no_base, Direction::East), Some(LOW));
        assert_eq!(MossyCarpet::side(no_base, Direction::South), Some(NONE));
        assert_eq!(MossyCarpet::side(no_base, Direction::North), Some(NONE));
        assert_eq!(MossyCarpet::side(no_base, Direction::West), Some(NONE));
        assert!(carpet.has_faces(no_base));
    }

    #[test]
    fn only_pale_moss_carpet_parses() {
        let moss = mc_bamboo_data::block_by_name("minecraft:moss_carpet")
            .unwrap()
            .default_state;
        assert!(MossyCarpet::parse(moss).is_none());
    }
}
