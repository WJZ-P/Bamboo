//! `FallenTreeFeature`：一个树桩，加上旁边倒在地上的一段原木。照官方 26.3 逐行翻译。
//!
//! 装饰器复用树的那一套。树桩的装饰器看到的原木集合是 `Set.of(stump)`，只有一个元素；
//! 倒木的集合是 `HashSet`，迭代顺序照 [`super::java_set`] 重放。
//!
//! `FallenTreeFeature`: a stump plus a log lying on the ground next to it, translated
//! line by line from official 26.3.
//!
//! Decorators are the trees' own. The stump decorators see `Set.of(stump)`, a single
//! element; the fallen log's set is a `HashSet` whose iteration order is replayed by
//! [`super::java_set`].

use serde_json::Value;

use mc_bamboo_data::{facts, state_facts};

use crate::{
    feature::{
        provider::IntProvider, state::with_property, state_provider::StateProvider,
        world::FeatureWorld,
    },
    random::WorldgenRandom,
};

use super::{
    HORIZONTAL, Pos, TreePlace, axis_of, decorator::Decorator, random_horizontal, valid_tree_pos,
};

/// 已解析的 `fallen_tree`。
///
/// A parsed `fallen_tree`.
pub struct FallenTreeFeature {
    trunk_provider: StateProvider,
    log_length: IntProvider,
    stump_decorators: Vec<Decorator>,
    log_decorators: Vec<Decorator>,
}

impl FallenTreeFeature {
    pub fn parse(config: &Value) -> Option<Self> {
        let object = config.as_object()?;
        let decorators = |name: &str| {
            object
                .get(name)?
                .as_array()?
                .iter()
                .map(Decorator::parse)
                .collect::<Option<Vec<_>>>()
        };
        Some(Self {
            trunk_provider: StateProvider::parse(object.get("trunk_provider")?)?,
            log_length: IntProvider::parse(object.get("log_length")?).ok()?,
            stump_decorators: decorators("stump_decorators")?,
            log_decorators: decorators("log_decorators")?,
        })
    }

    /// 官方 `FallenTreeFeature.place`：总是返回 `true`。
    ///
    /// Official `FallenTreeFeature.place`: always returns `true`.
    pub fn place(
        &self,
        world: &mut FeatureWorld<'_>,
        random: &WorldgenRandom,
        origin: Pos,
    ) -> bool {
        // 倒木的装饰器不用递归地物；给一个不会用到的群系回调占位。
        // Fallen-tree decorators never recurse; pass a placeholder biome callback.
        let mut no_biome = |_x: i32, _y: i32, _z: i32| 0_u32;
        // `placeStump`。
        let mut stump = TreePlace::new(world);
        self.place_log_block(&mut stump, random, origin, None);
        for decorator in &self.stump_decorators {
            decorator.place(&mut stump, random, &mut no_biome, None);
        }
        let world = stump.world;

        let direction = random_horizontal(random);
        let log_length = self.log_length.sample(random) - 2;
        let distance = 2 + random.next_int(2);
        let step = HORIZONTAL[direction];
        let mut start = (
            origin.0 + step.0 * distance,
            origin.1,
            origin.2 + step.1 * distance,
        );
        // `setGroundHeightForFallenLogStartPos`：先抬一格，再最多往下找 6 次落脚点。
        // `setGroundHeightForFallenLogStartPos`: up one, then at most six tries downwards.
        start.1 += 1;
        for _ in 0..6 {
            if may_place_on(world, start) {
                break;
            }
            start.1 -= 1;
        }
        if !can_place_entire_log(world, log_length, start, step) {
            return true;
        }
        // `placeFallenLog`。
        let mut log = TreePlace::new(world);
        let mut pos = start;
        for _ in 0..log_length {
            self.place_log_block(&mut log, random, pos, Some(axis_of(direction)));
            pos.0 += step.0;
            pos.2 += step.1;
        }
        for decorator in &self.log_decorators {
            decorator.place(&mut log, random, &mut no_biome, None);
        }
        true
    }

    /// 官方 `placeLogBlock`：直接写方块，不检查原来是什么。
    ///
    /// Official `placeLogBlock`: writes the block without checking what was there.
    fn place_log_block(
        &self,
        place: &mut TreePlace<'_, '_>,
        random: &WorldgenRandom,
        pos: Pos,
        axis: Option<&str>,
    ) {
        let Some(mut state) = self.trunk_provider.get_state(random) else {
            return;
        };
        if let Some(axis) = axis {
            state = with_property(state, "axis", axis).unwrap_or(state);
        }
        place.set_trunk(pos, state);
    }
}

/// 官方 `isOverSolidGround`：下面那格 `isFaceSturdy(UP)`。
///
/// Official `isOverSolidGround`: the block below is `isFaceSturdy(UP)`.
fn is_over_solid_ground(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let below = world.get_block(pos.0, pos.1 - 1, pos.2);
    // `Direction.UP` 的 ordinal 是 1。
    // `Direction.UP` has ordinal 1.
    state_facts(below) & (1u64 << (facts::STURDY_FULL + 1)) != 0
}

/// 官方 `mayPlaceOn`。
fn may_place_on(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    valid_tree_pos(world, pos) && is_over_solid_ground(world, pos)
}

/// 官方 `canPlaceEntireFallenLog`：整段都能放，而且悬空的连续格子不超过 2 个。
///
/// Official `canPlaceEntireFallenLog`: every position is placeable and no more than
/// two consecutive positions hang over a gap.
fn can_place_entire_log(
    world: &FeatureWorld<'_>,
    log_length: i32,
    start: Pos,
    step: (i32, i32),
) -> bool {
    let mut gap = 0;
    let mut pos = start;
    for _ in 0..log_length {
        if !valid_tree_pos(world, pos) {
            return false;
        }
        if is_over_solid_ground(world, pos) {
            gap = 0;
        } else {
            gap += 1;
            if gap > 2 {
                return false;
            }
        }
        pos.0 += step.0;
        pos.2 += step.1;
    }
    true
}
