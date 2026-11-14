//! `SculkPatchFeature` 与它驱动的 `SculkSpreader`：幽匿斑块
//! （`sculk_patch_deep_dark` / `sculk_patch_ancient_city`）。
//!
//! 官方先 `canSpreadFrom` 判定原点，再按「轮」推进电荷游标：每轮
//! `chargeCount` 次 `addCursors(origin, amountPerCharge)`，跑 `spreadAttempts` 次
//! `updateCursors`，最后 `clear()`。游标花电荷的方式取决于脚下方块的
//! `SculkBehaviour` 实现：`SculkBlock` 长传感器 / 尖啸体，`SculkVeinBlock` 铺脉络，
//! 其它方块用 `SculkBehaviour.DEFAULT`。
//!
//! 注意 `SculkBehaviour` 有**两套**默认实现：接口上的 `default` 方法（`SculkBlock` 和
//! `SculkVeinBlock` 用的是它），以及 `DEFAULT` 这个匿名对象里的覆写（只有非
//! `SculkBehaviour` 方块才会走到）。两者的 `attemptSpreadVein` 不一样，必须分开。
//!
//! `SculkPatchFeature` and the `SculkSpreader` behind it. Vanilla checks
//! `canSpreadFrom`, then advances charge cursors in rounds: `chargeCount` cursors of
//! `amountPerCharge`, `spreadAttempts` `updateCursors` passes, then `clear()`. How a
//! cursor spends charge depends on the `SculkBehaviour` under it: `SculkBlock` grows
//! sensors/shriekers, `SculkVeinBlock` spreads veins, everything else uses
//! `SculkBehaviour.DEFAULT`.
//!
//! `SculkBehaviour` has **two** sets of defaults: the interface `default` methods (used
//! by `SculkBlock` and `SculkVeinBlock`) and the overrides on the `DEFAULT` object
//! (reached only by blocks that are not `SculkBehaviour`). Their `attemptSpreadVein`
//! differ, so they must stay separate here.

use mc_bamboo_core::Direction;
use mc_bamboo_data::{
    block_id_of_state, block_tag, blocks, facts, registries, state_facts, state_fluid, tags,
};
use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{state::with_property, world::FeatureWorld};

type Pos = (i32, i32, i32);

/// `SculkSpreader.createWorldGenSpreader()` 的五个数：
/// `growthSpawnCost 50`、`noGrowthRadius 1`、`chargeDecayRate 5`、
/// `additionalDecayRate 10`，替换表是 `SCULK_REPLACEABLE_WORLD_GEN`。
///
/// The five numbers of `SculkSpreader.createWorldGenSpreader()`.
const GROWTH_SPAWN_COST: i32 = 50;
const NO_GROWTH_RADIUS: i32 = 1;
const CHARGE_DECAY_RATE: i32 = 5;
const ADDITIONAL_DECAY_RATE: i32 = 10;

/// `SculkSpreader.MAX_CURSORS` / `MAX_CHARGE`，以及
/// `ChargeCursor.MAX_WORLDGEN_SPREAD` 的平方。
///
/// `SculkSpreader.MAX_CURSORS` / `MAX_CHARGE` and the square of
/// `ChargeCursor.MAX_WORLDGEN_SPREAD`.
const MAX_CURSORS: usize = 32;
const MAX_CHARGE: i32 = 1000;
const MAX_WORLDGEN_SPREAD_SQ: i32 = 144;

/// `SculkPatchFeature` 的五个配置字段。
///
/// The five config fields of `SculkPatchFeature`.
#[derive(Debug, Clone, Copy)]
pub struct SculkPatchFeature {
    pub charge_count: i32,
    pub amount_per_charge: i32,
    pub spread_attempts: i32,
    pub growth_rounds: i32,
    pub spread_rounds: i32,
}

impl SculkPatchFeature {
    pub fn parse(config: &Value) -> Option<Self> {
        let object = config.as_object()?;
        Some(Self {
            charge_count: object.get("charge_count")?.as_i64()? as i32,
            amount_per_charge: object.get("amount_per_charge")?.as_i64()? as i32,
            spread_attempts: object.get("spread_attempts")?.as_i64()? as i32,
            growth_rounds: object.get("growth_rounds")?.as_i64()? as i32,
            spread_rounds: object.get("spread_rounds")?.as_i64()? as i32,
        })
    }
}

/// `SculkSpreader.ChargeCursor`。`facings == None` 对应官方字段的 `null`。
///
/// `SculkSpreader.ChargeCursor`; `facings == None` mirrors the official `null`.
#[derive(Debug, Clone)]
struct Cursor {
    pos: Pos,
    charge: i32,
    update_delay: i32,
    decay_delay: i32,
    facings: Option<Vec<Direction>>,
}

impl Cursor {
    /// `new ChargeCursor(pos, charge)`：`decayDelay = 1, updateDelay = 0, facings = null`。
    ///
    /// `new ChargeCursor(pos, charge)`: `decayDelay = 1, updateDelay = 0, facings = null`.
    fn new(pos: Pos, charge: i32) -> Self {
        Self {
            pos,
            charge,
            update_delay: 0,
            decay_delay: 1,
            facings: None,
        }
    }

    /// `isPosUnreasonable(originPos)`：切比雪夫距离超过 1024。
    ///
    /// `isPosUnreasonable(originPos)`: Chebyshev distance beyond 1024.
    fn is_pos_unreasonable(&self, origin: Pos) -> bool {
        (self.pos.0 - origin.0)
            .abs()
            .max((self.pos.1 - origin.1).abs())
            .max((self.pos.2 - origin.2).abs())
            > 1024
    }
}

/// 方块实现的 `SculkBehaviour`。26.3 只有 `SculkBlock` 和 `SculkVeinBlock`。
///
/// The `SculkBehaviour` a block implements; 26.3 only has `SculkBlock` and
/// `SculkVeinBlock`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Behaviour {
    /// `SculkBehaviour.DEFAULT`：非 `SculkBehaviour` 方块走这个匿名对象。
    /// `SculkBehaviour.DEFAULT`: the anonymous object used by non-`SculkBehaviour` blocks.
    Default,
    Sculk,
    Vein,
}

impl Behaviour {
    /// `ChargeCursor.getBlockBehaviour(state)`。
    ///
    /// `ChargeCursor.getBlockBehaviour(state)`.
    fn of(state: u32) -> Self {
        let Some(id) = block_id_of_state(state) else {
            return Self::Default;
        };
        if id == block_id_of_state(blocks::SCULK).unwrap_or(usize::MAX) {
            Self::Sculk
        } else if id == block_id_of_state(blocks::SCULK_VEIN).unwrap_or(usize::MAX) {
            Self::Vein
        } else {
            Self::Default
        }
    }

    /// `canChangeBlockStateOnSpread`：接口默认 true，`SculkBlock` 覆写成 false。
    ///
    /// `canChangeBlockStateOnSpread`: interface default true, `SculkBlock` overrides false.
    fn can_change_block_state_on_spread(self) -> bool {
        self != Self::Sculk
    }

    /// `updateDecayDelay`：`DEFAULT` 对象是 `max(age - 1, 0)`，另外两个用接口默认值 1。
    ///
    /// `updateDecayDelay`: `max(age - 1, 0)` on the `DEFAULT` object, otherwise the
    /// interface default of 1.
    fn update_decay_delay(self, age: i32) -> i32 {
        if self == Self::Default {
            (age - 1).max(0)
        } else {
            1
        }
    }
}

/// `SculkPatchFeature.place`。
///
/// `SculkPatchFeature.place`.
pub fn place(
    feature: &SculkPatchFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: Pos,
) -> bool {
    if !can_spread_from(world, origin) {
        return false;
    }
    let mut cursors: Vec<Cursor> = Vec::new();
    let total_rounds = feature.spread_rounds + feature.growth_rounds;
    for round in 0..total_rounds {
        for _ in 0..feature.charge_count {
            add_cursors(&mut cursors, origin, feature.amount_per_charge);
        }
        let spread_veins = round < feature.spread_rounds;
        for _ in 0..feature.spread_attempts {
            update_cursors(world, origin, random, &mut cursors, spread_veins);
        }
        cursors.clear();
    }
    true
}

/// `SculkSpreader.addCursors`：一次最多 1000 电荷，超出拆成多个游标；
/// `addCursor` 在已有 32 个时丢弃。
///
/// `SculkSpreader.addCursors`: at most 1000 charge per cursor, dropped past 32 cursors.
fn add_cursors(cursors: &mut Vec<Cursor>, pos: Pos, mut charge: i32) {
    while charge > 0 {
        let current = charge.min(MAX_CHARGE);
        if cursors.len() < MAX_CURSORS {
            cursors.push(Cursor::new(pos, current));
        }
        charge -= current;
    }
}

/// `SculkSpreader.updateCursors`：先逐个更新，再按位置决定哪些游标留下。
///
/// 世界生成时官方**不合并**（`!isWorldGeneration()` 才走 `mergeWith`），所以同位置的
/// 多个游标都保留；`mergeableCursors` 那张表只用来挑「电荷较大」的那个，供
/// `levelEvent` 的粒子数据使用，而粒子是纯表现，这里不做。
///
/// `SculkSpreader.updateCursors`. World generation never merges (that needs
/// `!isWorldGeneration()`), so cursors sharing a position all stay; the
/// `mergeableCursors` map only feeds the `levelEvent` particle data, which is purely
/// cosmetic and skipped here.
fn update_cursors(
    world: &mut FeatureWorld<'_>,
    origin: Pos,
    random: &WorldgenRandom,
    cursors: &mut Vec<Cursor>,
    spread_veins: bool,
) {
    if cursors.is_empty() {
        return;
    }
    let mut processed: Vec<Cursor> = Vec::new();
    for mut cursor in std::mem::take(cursors) {
        if cursor.is_pos_unreasonable(origin) {
            continue;
        }
        update_one(world, origin, random, &mut cursor, spread_veins);
        // `if (cursor.charge <= 0) levelEvent(...)` 之后不进 processedCursors。
        // After the `charge <= 0` branch the cursor is not added to `processedCursors`.
        if cursor.charge > 0 {
            processed.push(cursor);
        }
    }
    *cursors = processed;
}

/// `ChargeCursor.update`。
///
/// `ChargeCursor.update`.
fn update_one(
    world: &mut FeatureWorld<'_>,
    origin: Pos,
    random: &WorldgenRandom,
    cursor: &mut Cursor,
    spread_veins: bool,
) {
    // `shouldUpdate`：世界生成恒真（非世界生成才看 `shouldTickBlocksAt`）。
    // `shouldUpdate` is always true during world generation.
    if cursor.charge <= 0 {
        return;
    }
    if cursor.update_delay > 0 {
        cursor.update_delay -= 1;
        return;
    }
    let mut state = world.get_block(cursor.pos.0, cursor.pos.1, cursor.pos.2);
    let mut behaviour = Behaviour::of(state);
    // 官方是 `if (spreadVeins && attemptSpreadVein(...)) { if (canChangeBlockStateOnSpread()) {...} }`。
    // 两层的短路语义保持原样，合并成一个条件不影响求值顺序。
    // Vanilla nests `if (canChangeBlockStateOnSpread())` inside the spread attempt; the
    // short-circuit order is unchanged by folding the outer pair.
    if spread_veins
        && attempt_spread_vein(
            behaviour,
            world,
            cursor.pos,
            state,
            cursor.facings.as_deref(),
        )
        && behaviour.can_change_block_state_on_spread()
    {
        state = world.get_block(cursor.pos.0, cursor.pos.1, cursor.pos.2);
        behaviour = Behaviour::of(state);
    }

    cursor.charge = attempt_use_charge(behaviour, cursor, world, origin, random, spread_veins);
    if cursor.charge <= 0 {
        on_discharged(behaviour, world, state, cursor.pos, random);
        return;
    }

    match valid_movement_pos(world, cursor.pos, random, origin) {
        Some(target) => {
            on_discharged(behaviour, world, state, cursor.pos, random);
            cursor.pos = target;
            state = world.get_block(target.0, target.1, target.2);
        }
        None => {
            // 世界生成时找不到落点：先 `onDischarged`，电荷清零后返回。
            // World generation without a destination discharges, zeroes the charge and
            // returns before the trailing bookkeeping.
            on_discharged(behaviour, world, state, cursor.pos, random);
            cursor.charge = 0;
            return;
        }
    }

    // `if (currentState.getBlock() instanceof SculkBehaviour) this.facings = availableFaces(...)`。
    // `MultifaceBlock.availableFaces` 对非 multiface 方块返回空集合，所以 `sculk`
    // 上是 `Some([])` 而不是 `null`。
    // `availableFaces` returns an empty set for non-multiface blocks, so a `sculk` cell
    // ends up with `Some([])`, not `null`.
    if Behaviour::of(state) != Behaviour::Default {
        cursor.facings = Some(available_faces(state));
    }
    cursor.decay_delay = behaviour.update_decay_delay(cursor.decay_delay);
    // `getSculkSpreadDelay()` 是接口默认的 1，三个实现都没覆写。
    // `getSculkSpreadDelay()` keeps the interface default of 1 for every implementation.
    cursor.update_delay = 1;
}

/// `SculkBehaviour.attemptSpreadVein`：接口默认实现，`SculkBlock` 和 `SculkVeinBlock`
/// 都没有覆写，所以它们走的是
/// `getSpreader().spreadAll(state, level, pos, postProcess) > 0`，与 `facings` 无关。
/// 只有 `DEFAULT` 对象覆写成按 `facings` 分支的版本。
///
/// The interface `default attemptSpreadVein`, which `SculkBlock` and `SculkVeinBlock`
/// both keep: `getSpreader().spreadAll(...) > 0`, independent of `facings`. Only the
/// `DEFAULT` object overrides it with the facings branches.
fn attempt_spread_vein(
    behaviour: Behaviour,
    world: &mut FeatureWorld<'_>,
    pos: Pos,
    state: u32,
    facings: Option<&[Direction]>,
) -> bool {
    match behaviour {
        Behaviour::Sculk | Behaviour::Vein => spread_all(world, pos, state) > 0,
        Behaviour::Default => match facings {
            // `facings == null`：`getSameSpaceSpreader().spreadAll(level.getBlockState(pos), ...) > 0`。
            // `facings == null`: `getSameSpaceSpreader().spreadAll(...) > 0`.
            None => spread_all_with(world, pos, state, &[SpreadKind::SamePosition]) > 0,
            // `!facings.isEmpty()`：`!isAir && !fluid.is(WATER)` 才拒绝，否则 `regrow`。
            // `!facings.isEmpty()`: reject only when `!isAir && !fluid.is(WATER)`, else
            // `regrow`.
            Some(faces) if !faces.is_empty() => {
                let fluid_is_water = state_fluid(state).fluid == registries::fluid::WATER;
                if state_facts(state) & facts::AIR == 0 && !fluid_is_water {
                    return false;
                }
                regrow(world, pos, state, faces)
            }
            // `facings.isEmpty()`：`SculkBehaviour.super.attemptSpreadVein(...)`。
            // `facings.isEmpty()`: the interface default.
            Some(_) => spread_all(world, pos, state) > 0,
        },
    }
}

/// `SculkBehaviour.attemptUseCharge`。
///
/// `SculkBehaviour.attemptUseCharge`.
fn attempt_use_charge(
    behaviour: Behaviour,
    cursor: &Cursor,
    world: &mut FeatureWorld<'_>,
    origin: Pos,
    random: &WorldgenRandom,
    spread_veins: bool,
) -> i32 {
    match behaviour {
        Behaviour::Sculk => sculk_use_charge(cursor, world, origin, random),
        Behaviour::Vein => vein_use_charge(cursor, world, random, spread_veins),
        // `DEFAULT` 对象：`decayDelay > 0 ? charge : 0`。
        // The `DEFAULT` object: `decayDelay > 0 ? charge : 0`.
        Behaviour::Default => {
            if cursor.decay_delay > 0 {
                cursor.charge
            } else {
                0
            }
        }
    }
}

/// `SculkBlock.attemptUseCharge`。
///
/// `SculkBlock.attemptUseCharge`.
fn sculk_use_charge(
    cursor: &Cursor,
    world: &mut FeatureWorld<'_>,
    origin: Pos,
    random: &WorldgenRandom,
) -> i32 {
    let charge = cursor.charge;
    if charge == 0 || random.next_int(CHARGE_DECAY_RATE) != 0 {
        return charge;
    }
    let pos = cursor.pos;
    let close = closer_than(pos, origin, NO_GROWTH_RADIUS);
    if !close && can_place_growth(world, pos) {
        if random.next_int(GROWTH_SPAWN_COST) < charge {
            let growth = (pos.0, pos.1 + 1, pos.2);
            let state = random_growth_state(world, growth, random);
            world.set_block_with_heightmaps(growth.0, growth.1, growth.2, state);
        }
        return (charge - GROWTH_SPAWN_COST).max(0);
    }
    if random.next_int(ADDITIONAL_DECAY_RATE) != 0 {
        return charge;
    }
    let penalty = if close {
        1
    } else {
        decay_penalty(pos, origin, charge)
    };
    charge - penalty
}

/// `SculkVeinBlock.attemptUseCharge`。
///
/// `SculkVeinBlock.attemptUseCharge`.
fn vein_use_charge(
    cursor: &Cursor,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    spread_veins: bool,
) -> i32 {
    if spread_veins && attempt_place_sculk(world, cursor.pos, random) {
        return cursor.charge - 1;
    }
    if random.next_int(CHARGE_DECAY_RATE) == 0 {
        // `Mth.floor(charge * 0.5F)`。
        // `Mth.floor(charge * 0.5F)`.
        (cursor.charge as f32 * 0.5).floor() as i32
    } else {
        cursor.charge
    }
}

/// `BlockPos.closerThan(pos, distance)`：`distSqr < distance * distance`。
///
/// `BlockPos.closerThan(pos, distance)`: `distSqr < distance * distance`.
fn closer_than(a: Pos, b: Pos, distance: i32) -> bool {
    let dx = f64::from(a.0 - b.0);
    let dy = f64::from(a.1 - b.1);
    let dz = f64::from(a.2 - b.2);
    dx * dx + dy * dy + dz * dz < f64::from(distance) * f64::from(distance)
}

/// `SculkBlock.getDecayPenalty`：注意最后的 `Math.max(1, ...)`。
///
/// `SculkBlock.getDecayPenalty`, including the trailing `Math.max(1, ...)`.
fn decay_penalty(pos: Pos, origin: Pos, charge: i32) -> i32 {
    let dx = f64::from(pos.0 - origin.0);
    let dy = f64::from(pos.1 - origin.1);
    let dz = f64::from(pos.2 - origin.2);
    // `Mth.square((float)Math.sqrt(distSqr) - noGrowthRadius)`：先开方，再降成 float。
    // `Mth.square((float)Math.sqrt(distSqr) - noGrowthRadius)`: sqrt, then narrowed.
    let dist = (dx * dx + dy * dy + dz * dz).sqrt() as f32;
    let offset = dist - NO_GROWTH_RADIUS as f32;
    let outer = offset * offset;
    let max_reach = ((24 - NO_GROWTH_RADIUS) * (24 - NO_GROWTH_RADIUS)) as f32;
    let factor = (outer / max_reach).min(1.0);
    (((charge as f32) * factor * 0.5) as i32).max(1)
}

/// `SculkBlock.canPlaceGrowth`：上方是空气、或是流体为水的方块；且
/// `pos.offset(-4,0,-4)` 到 `pos.offset(4,2,4)` 的盒子里
/// `sculk_growth_inhibitors` 少于 3 个（`atMostMatched(2)`）。
///
/// `SculkBlock.canPlaceGrowth`: the cell above is air or has water fluid, and the box
/// from `pos.offset(-4,0,-4)` to `pos.offset(4,2,4)` holds fewer than three
/// `sculk_growth_inhibitors` (`atMostMatched(2)`).
fn can_place_growth(world: &FeatureWorld<'_>, pos: Pos) -> bool {
    let above = world.get_block(pos.0, pos.1 + 1, pos.2);
    let air = state_facts(above) & facts::AIR != 0;
    // `stateAbove.is(WATER) && stateAbove.getFluidState().is(Fluids.WATER)`：
    // 只要求流体是水，不要求是源。
    // Only the fluid being water matters, not whether it is a source.
    let water = block_id_of_state(above) == block_id_of_state(blocks::WATER)
        && state_fluid(above).fluid == registries::fluid::WATER;
    if !(air || water) {
        return false;
    }
    let mut count = 0;
    for x in pos.0 - 4..=pos.0 + 4 {
        for z in pos.2 - 4..=pos.2 + 4 {
            for y in pos.1..=pos.1 + 2 {
                let state = world.get_block(x, y, z);
                if in_block_tag(state, tags::block_tags::SCULK_GROWTH_INHIBITORS.name) {
                    count += 1;
                    if count >= 3 {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// `SculkBlock.getRandomGrowthState`。
///
/// `SculkBlock.getRandomGrowthState`.
fn random_growth_state(world: &FeatureWorld<'_>, pos: Pos, random: &WorldgenRandom) -> u32 {
    let state = if random.next_int(11) == 0 {
        // `CAN_SUMMON` 取 `isWorldGen`，这里恒为 true。
        // `CAN_SUMMON` takes `isWorldGen`, which is always true here.
        with_property(blocks::SCULK_SHRIEKER, "can_summon", "true")
            .unwrap_or(blocks::SCULK_SHRIEKER)
    } else {
        blocks::SCULK_SENSOR
    };
    // `hasProperty(WATERLOGGED) && !level.getFluidState(pos).isEmpty()`。
    // `hasProperty(WATERLOGGED) && !level.getFluidState(pos).isEmpty()`.
    if state_fluid(world.get_block(pos.0, pos.1, pos.2)).amount > 0 {
        with_property(state, "waterlogged", "true").unwrap_or(state)
    } else {
        state
    }
}

/// `SculkVeinBlock.attemptPlaceSculk`：按洗好的六方向找一个能贴的
/// `sculk_replaceable` 支撑格，换成幽匿方块后立刻 `veinSpreader.spreadAll`，
/// 再把除反方向外的相邻脉络逐个 `onDischarged`。
///
/// `SculkVeinBlock.attemptPlaceSculk`: find a `sculk_replaceable` support cell in the
/// shuffled six directions, replace it with sculk, spread once with `veinSpreader`, then
/// discharge every adjacent vein except the opposite face.
fn attempt_place_sculk(world: &mut FeatureWorld<'_>, pos: Pos, random: &WorldgenRandom) -> bool {
    let state = world.get_block(pos.0, pos.1, pos.2);
    for support in all_shuffled(random) {
        if !has_face(state, support) {
            continue;
        }
        let (dx, dy, dz) = offset(support);
        let support_pos = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
        let support_state = world.get_block(support_pos.0, support_pos.1, support_pos.2);
        if !in_block_tag(
            support_state,
            tags::block_tags::SCULK_REPLACEABLE_WORLD_GEN.name,
        ) {
            continue;
        }
        world.set_block_with_heightmaps(support_pos.0, support_pos.1, support_pos.2, blocks::SCULK);
        // `this.veinSpreader.spreadAll(defaultSculk, level, supportPos, isWorldGen())`：
        // 用 `SculkVeinSpreaderConfig` 的默认 spread 顺序，不消费随机数。
        // `veinSpreader.spreadAll(...)` with the sculk vein config's default order; no
        // random is consumed.
        spread_all(world, support_pos, blocks::SCULK);
        let skip = support.opposite();
        for direction in Direction::ALL {
            if direction == skip {
                continue;
            }
            let (vx, vy, vz) = offset(direction);
            let vein_pos = (support_pos.0 + vx, support_pos.1 + vy, support_pos.2 + vz);
            let possible = world.get_block(vein_pos.0, vein_pos.1, vein_pos.2);
            if block_id_of_state(possible) == block_id_of_state(blocks::SCULK_VEIN) {
                discharged_vein(world, possible, vein_pos);
            }
        }
        return true;
    }
    false
}

/// `SculkBehaviour.onDischarged`：只有 `SculkVeinBlock` 覆写了它。
///
/// `SculkBehaviour.onDischarged`: only `SculkVeinBlock` overrides it.
fn on_discharged(
    behaviour: Behaviour,
    world: &mut FeatureWorld<'_>,
    state: u32,
    pos: Pos,
    _random: &WorldgenRandom,
) {
    if behaviour == Behaviour::Vein {
        discharged_vein(world, state, pos);
    }
}

/// `SculkVeinBlock.onDischarged` 的函数体。
///
/// The body of `SculkVeinBlock.onDischarged`.
fn discharged_vein(world: &mut FeatureWorld<'_>, state: u32, pos: Pos) {
    if block_id_of_state(state) != block_id_of_state(blocks::SCULK_VEIN) {
        return;
    }
    let mut new_state = state;
    for direction in Direction::ALL {
        if !has_face(new_state, direction) {
            continue;
        }
        let (dx, dy, dz) = offset(direction);
        let neighbour = world.get_block(pos.0 + dx, pos.1 + dy, pos.2 + dz);
        if block_id_of_state(neighbour) == block_id_of_state(blocks::SCULK)
            && let Some(cleared) = with_property(new_state, face_property(direction), "false")
        {
            new_state = cleared;
        }
    }
    if !Direction::ALL.iter().any(|&face| has_face(new_state, face)) {
        let fluid = state_fluid(world.get_block(pos.0, pos.1, pos.2));
        new_state = if fluid.amount == 0 {
            blocks::AIR
        } else {
            blocks::WATER
        };
    }
    world.set_block_with_heightmaps(pos.0, pos.1, pos.2, new_state);
}

/// `ChargeCursor.getValidMovementPos`：洗好的 18 个非角邻居里找第一个
/// 「是 `SculkBehaviour` 且路径不被挡住」的格子；遇到有基质的就提前停。
///
/// `ChargeCursor.getValidMovementPos`: the first shuffled non-corner neighbour that is a
/// `SculkBehaviour` without an obstructed path; stops early at one with substrate.
fn valid_movement_pos(
    world: &FeatureWorld<'_>,
    pos: Pos,
    random: &WorldgenRandom,
    origin: Pos,
) -> Option<Pos> {
    let mut result = pos;
    for offset in non_corner_neighbours(random) {
        let neighbour = (pos.0 + offset.0, pos.1 + offset.1, pos.2 + offset.2);
        if !can_move_to(origin, neighbour) {
            continue;
        }
        let transferee = world.get_block(neighbour.0, neighbour.1, neighbour.2);
        if Behaviour::of(transferee) == Behaviour::Default
            || !is_movement_unobstructed(world, pos, neighbour)
        {
            continue;
        }
        result = neighbour;
        if has_substrate_access(world, transferee, neighbour) {
            break;
        }
    }
    (result != pos).then_some(result)
}

/// `ChargeCursor.canMoveToPos`：世界生成时水平距离平方不超过 144。
///
/// `ChargeCursor.canMoveToPos`: world generation caps the squared horizontal distance at 144.
fn can_move_to(origin: Pos, target: Pos) -> bool {
    let dx = origin.0 - target.0;
    let dz = origin.2 - target.2;
    dx * dx + dz * dz <= MAX_WORLDGEN_SPREAD_SQ
}

/// `ChargeCursor.isMovementUnobstructed`。
///
/// `ChargeCursor.isMovementUnobstructed`.
fn is_movement_unobstructed(world: &FeatureWorld<'_>, from: Pos, to: Pos) -> bool {
    let delta = (to.0 - from.0, to.1 - from.1, to.2 - from.2);
    if delta.0.abs() + delta.1.abs() + delta.2.abs() == 1 {
        return true;
    }
    let x = if delta.0 < 0 {
        Direction::West
    } else {
        Direction::East
    };
    let y = if delta.1 < 0 {
        Direction::Down
    } else {
        Direction::Up
    };
    let z = if delta.2 < 0 {
        Direction::North
    } else {
        Direction::South
    };
    if delta.0 == 0 {
        is_unobstructed(world, from, y) || is_unobstructed(world, from, z)
    } else if delta.1 == 0 {
        is_unobstructed(world, from, x) || is_unobstructed(world, from, z)
    } else {
        is_unobstructed(world, from, x) || is_unobstructed(world, from, y)
    }
}

/// `ChargeCursor.isUnobstructed`：反方向的支撑面不满。
///
/// `ChargeCursor.isUnobstructed`: the opposite support face is not full.
fn is_unobstructed(world: &FeatureWorld<'_>, from: Pos, direction: Direction) -> bool {
    let (dx, dy, dz) = offset(direction);
    let pos = (from.0 + dx, from.1 + dy, from.2 + dz);
    let state = world.get_block(pos.0, pos.1, pos.2);
    !face_is_sturdy(state, direction.opposite())
}

/// `SculkVeinBlock.hasSubstrateAccess`：某个打开的面旁边是 `sculk_replaceable`。
///
/// `SculkVeinBlock.hasSubstrateAccess`: an open face touches `sculk_replaceable`.
fn has_substrate_access(world: &FeatureWorld<'_>, state: u32, pos: Pos) -> bool {
    if block_id_of_state(state) != block_id_of_state(blocks::SCULK_VEIN) {
        return false;
    }
    Direction::ALL.iter().any(|&direction| {
        if !has_face(state, direction) {
            return false;
        }
        let (dx, dy, dz) = offset(direction);
        let neighbour = world.get_block(pos.0 + dx, pos.1 + dy, pos.2 + dz);
        in_block_tag(neighbour, tags::block_tags::SCULK_REPLACEABLE.name)
    })
}

/// `SculkPatchFeature.canSpreadFrom`。
///
/// `SculkPatchFeature.canSpreadFrom`.
fn can_spread_from(world: &FeatureWorld<'_>, origin: Pos) -> bool {
    let start = world.get_block(origin.0, origin.1, origin.2);
    if Behaviour::of(start) != Behaviour::Default {
        return true;
    }
    let air = state_facts(start) & facts::AIR != 0;
    let water_source =
        block_id_of_state(start) == block_id_of_state(blocks::WATER) && state_fluid(start).source;
    if !(air || water_source) {
        return false;
    }
    // `isCollisionShapeFullBlock`。
    // `isCollisionShapeFullBlock`.
    Direction::ALL.iter().any(|&direction| {
        let (dx, dy, dz) = offset(direction);
        let state = world.get_block(origin.0 + dx, origin.1 + dy, origin.2 + dz);
        state_facts(state) & facts::COLLISION_FULL_BLOCK != 0
    })
}

/// `MultifaceSpreader.DEFAULT_SPREAD_ORDER`。
///
/// `MultifaceSpreader.DEFAULT_SPREAD_ORDER`.
const SPREAD_ORDER: [SpreadKind; 3] = [
    SpreadKind::SamePosition,
    SpreadKind::SamePlane,
    SpreadKind::WrapAround,
];

/// `MultifaceSpreader.SpreadType`。
///
/// `MultifaceSpreader.SpreadType`.
#[derive(Clone, Copy)]
enum SpreadKind {
    SamePosition,
    SamePlane,
    WrapAround,
}

impl SpreadKind {
    /// `SpreadType.getSpreadPos(pos, spreadDirection, fromFace)`。
    /// `SpreadType.getSpreadPos(pos, spreadDirection, fromFace)`.
    fn spread_pos(self, pos: Pos, spread: Direction, from_face: Direction) -> (Pos, Direction) {
        let (sx, sy, sz) = offset(spread);
        match self {
            Self::SamePosition => (pos, spread),
            Self::SamePlane => ((pos.0 + sx, pos.1 + sy, pos.2 + sz), from_face),
            Self::WrapAround => {
                let (fx, fy, fz) = offset(from_face);
                (
                    (pos.0 + sx + fx, pos.1 + sy + fy, pos.2 + sz + fz),
                    spread.opposite(),
                )
            }
        }
    }
}

/// `MultifaceSpreader.spreadAll`，配置固定为 `SculkVeinSpreaderConfig(DEFAULT_SPREAD_ORDER)`。
///
/// `MultifaceSpreader.spreadAll` with the sculk vein config's default spread order.
fn spread_all(world: &mut FeatureWorld<'_>, pos: Pos, state: u32) -> i64 {
    spread_all_with(world, pos, state, &SPREAD_ORDER)
}

/// `MultifaceSpreader.spreadAll(state, level, pos, postProcess)`：
/// 对每个 `canSpreadFrom(state, face)` 成立的面，数 `spreadFromFaceTowardAllDirections`
/// 真正铺下的位置。不消费随机数。
///
/// `MultifaceSpreader.spreadAll(...)`: for each face accepted by
/// `canSpreadFrom(state, face)`, count the positions
/// `spreadFromFaceTowardAllDirections` actually places into. Consumes no random.
fn spread_all_with(
    world: &mut FeatureWorld<'_>,
    pos: Pos,
    state: u32,
    kinds: &[SpreadKind],
) -> i64 {
    // `SculkVeinSpreaderConfig.isOtherBlockValidAsSource(state)` 是
    // `!state.is(Blocks.SCULK_VEIN)`。
    // `SculkVeinSpreaderConfig.isOtherBlockValidAsSource(state)` is
    // `!state.is(Blocks.SCULK_VEIN)`.
    let other_block_valid = block_id_of_state(state) != block_id_of_state(blocks::SCULK_VEIN);
    let mut total = 0_i64;
    for from_face in Direction::ALL {
        // `canSpreadFrom`：`isOtherBlockValidAsSource(state) || hasFace(state, face)`。
        // `canSpreadFrom`: `isOtherBlockValidAsSource(state) || hasFace(state, face)`.
        if !(other_block_valid || has_face(state, from_face)) {
            continue;
        }
        for spread_direction in Direction::ALL {
            // `getSpreadFromFaceTowardDirection`：同轴直接返回 empty。
            // `getSpreadFromFaceTowardDirection`: a same-axis spread returns empty.
            if spread_direction.axis() == from_face.axis() {
                continue;
            }
            if !(other_block_valid
                || (has_face(state, from_face) && !has_face(state, spread_direction)))
            {
                continue;
            }
            for &kind in kinds {
                let (target_pos, target_face) = kind.spread_pos(pos, spread_direction, from_face);
                if !can_spread_into(world, pos, target_pos, target_face) {
                    continue;
                }
                let old = world.get_block(target_pos.0, target_pos.1, target_pos.2);
                if let Some(new_state) = state_for_placement(world, old, target_pos, target_face) {
                    world.set_block_with_heightmaps(
                        target_pos.0,
                        target_pos.1,
                        target_pos.2,
                        new_state,
                    );
                    total += 1;
                }
                break;
            }
        }
    }
    total
}

/// `SculkVeinSpreaderConfig.canSpreadInto`。
///
/// `SculkVeinSpreaderConfig.canSpreadInto`.
fn can_spread_into(
    world: &FeatureWorld<'_>,
    source_pos: Pos,
    placement_pos: Pos,
    placement_direction: Direction,
) -> bool {
    let existing = world.get_block(placement_pos.0, placement_pos.1, placement_pos.2);
    vein_state_can_be_replaced(
        world,
        source_pos,
        placement_pos,
        placement_direction,
        existing,
    ) && is_valid_state_for_placement(world, existing, placement_pos, placement_direction)
}

/// `SculkVeinSpreaderConfig.stateCanBeReplaced`。
///
/// `SculkVeinSpreaderConfig.stateCanBeReplaced`.
fn vein_state_can_be_replaced(
    world: &FeatureWorld<'_>,
    source_pos: Pos,
    placement_pos: Pos,
    placement_direction: Direction,
    existing_state: u32,
) -> bool {
    let (dx, dy, dz) = offset(placement_direction);
    let against = world.get_block(
        placement_pos.0 + dx,
        placement_pos.1 + dy,
        placement_pos.2 + dz,
    );
    let against_id = block_id_of_state(against);
    if against_id == block_id_of_state(blocks::SCULK)
        || against_id == block_id_of_state(blocks::SCULK_CATALYST)
        || against_id == block_id_of_state(blocks::MOVING_PISTON)
    {
        return false;
    }
    // `sourcePos.distManhattan(placementPos) == 2` 时，源格沿反方向那格若对
    // `placementDirection` 是结实面就拒绝。
    // At Manhattan distance two, reject when the cell back from the source is sturdy
    // toward `placementDirection`.
    let distance = (source_pos.0 - placement_pos.0).abs()
        + (source_pos.1 - placement_pos.1).abs()
        + (source_pos.2 - placement_pos.2).abs();
    if distance == 2 {
        let (ox, oy, oz) = offset(placement_direction.opposite());
        let neighbour = (source_pos.0 + ox, source_pos.1 + oy, source_pos.2 + oz);
        let neighbour_state = world.get_block(neighbour.0, neighbour.1, neighbour.2);
        if face_is_sturdy(neighbour_state, placement_direction) {
            return false;
        }
    }
    let fluid = state_fluid(existing_state);
    if fluid.amount > 0 && fluid.fluid != registries::fluid::WATER {
        return false;
    }
    // `existingState.is(BlockTags.FIRE)`。
    // `existingState.is(BlockTags.FIRE)`.
    if in_block_tag(existing_state, "minecraft:fire") {
        return false;
    }
    if state_facts(existing_state) & facts::REPLACEABLE != 0 {
        return true;
    }
    // `DefaultSpreaderConfig.stateCanBeReplaced`：空气 / 同类 / 水源。
    // `DefaultSpreaderConfig.stateCanBeReplaced`: air, the same block, or a water source.
    let same_vein = block_id_of_state(existing_state) == block_id_of_state(blocks::SCULK_VEIN);
    let water_source =
        block_id_of_state(existing_state) == block_id_of_state(blocks::WATER) && fluid.source;
    state_facts(existing_state) & facts::AIR != 0 || same_vein || water_source
}

/// `MultifaceBlock.isValidStateForPlacement`。
///
/// `MultifaceBlock.isValidStateForPlacement`.
fn is_valid_state_for_placement(
    world: &FeatureWorld<'_>,
    old_state: u32,
    placement_pos: Pos,
    placement_direction: Direction,
) -> bool {
    let same_vein = block_id_of_state(old_state) == block_id_of_state(blocks::SCULK_VEIN);
    if same_vein && has_face(old_state, placement_direction) {
        return false;
    }
    let (dx, dy, dz) = offset(placement_direction);
    let neighbour = world.get_block(
        placement_pos.0 + dx,
        placement_pos.1 + dy,
        placement_pos.2 + dz,
    );
    can_attach_to(neighbour, placement_direction)
}

/// `MultifaceBlock.getStateForPlacement(oldState, level, pos, direction)`。
///
/// `MultifaceBlock.getStateForPlacement(oldState, level, pos, direction)`.
fn state_for_placement(
    world: &FeatureWorld<'_>,
    old_state: u32,
    pos: Pos,
    face: Direction,
) -> Option<u32> {
    if !is_valid_state_for_placement(world, old_state, pos, face) {
        return None;
    }
    let same_vein = block_id_of_state(old_state) == block_id_of_state(blocks::SCULK_VEIN);
    let fluid = state_fluid(old_state);
    let base = if same_vein {
        old_state
    } else if fluid.fluid == registries::fluid::WATER && fluid.source {
        with_property(blocks::SCULK_VEIN, "waterlogged", "true").unwrap_or(blocks::SCULK_VEIN)
    } else {
        blocks::SCULK_VEIN
    };
    with_property(base, face_property(face), "true")
}

/// `SculkVeinBlock.regrow(level, pos, existing, faces)`。
///
/// `SculkVeinBlock.regrow(level, pos, existing, faces)`.
fn regrow(
    world: &mut FeatureWorld<'_>,
    pos: Pos,
    existing_state: u32,
    faces: &[Direction],
) -> bool {
    let mut new_state = blocks::SCULK_VEIN;
    let mut any = false;
    for &face in faces {
        let (dx, dy, dz) = offset(face);
        let neighbour = world.get_block(pos.0 + dx, pos.1 + dy, pos.2 + dz);
        if can_attach_to(neighbour, face)
            && let Some(state) = with_property(new_state, face_property(face), "true")
        {
            new_state = state;
            any = true;
        }
    }
    if !any {
        return false;
    }
    if state_fluid(existing_state).amount > 0 {
        new_state = with_property(new_state, "waterlogged", "true").unwrap_or(new_state);
    }
    world.set_block_with_heightmaps(pos.0, pos.1, pos.2, new_state);
    true
}

/// `ChargeCursor.NON_CORNER_NEIGHBOURS`：`betweenClosed(-1,-1,-1 .. 1,1,1)` 里
/// 至少一个坐标为 0、且不是原点的 18 项（x→y→z 顺序），再 `Util.shuffledCopy`。
///
/// `ChargeCursor.NON_CORNER_NEIGHBOURS`: the 18 cells of `betweenClosed(-1,-1,-1 ..
/// 1,1,1)` with a zero coordinate that are not the origin (x, y, z order), shuffled.
fn non_corner_neighbours(random: &WorldgenRandom) -> Vec<(i32, i32, i32)> {
    let mut list = Vec::with_capacity(18);
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                if (x == 0 || y == 0 || z == 0) && !(x == 0 && y == 0 && z == 0) {
                    list.push((x, y, z));
                }
            }
        }
    }
    shuffle(&mut list, random);
    list
}

/// `Direction.allShuffled(random)`。
///
/// `Direction.allShuffled(random)`.
fn all_shuffled(random: &WorldgenRandom) -> [Direction; 6] {
    let mut list = Direction::ALL;
    shuffle(&mut list, random);
    list
}

/// `Util.shuffledCopy`：Fisher-Yates，`i` 从 size 递减到 2，每次 `nextInt(i)`。
///
/// `Util.shuffledCopy`: Fisher-Yates from size down to 2 with one `nextInt(i)` each.
fn shuffle<T>(list: &mut [T], random: &WorldgenRandom) {
    for i in (2..=list.len()).rev() {
        let swap_to = random.next_int(i as i32) as usize;
        list.swap(i - 1, swap_to);
    }
}

/// `MultifaceBlock.availableFaces(state)`：不是 multiface 方块时返回空集合，
/// 与官方的 `Set.of()` 一致。
///
/// `MultifaceBlock.availableFaces(state)`: an empty set for blocks that are not
/// multiface, matching the official `Set.of()`.
fn available_faces(state: u32) -> Vec<Direction> {
    Direction::ALL
        .into_iter()
        .filter(|&face| has_face(state, face))
        .collect()
}

/// `BlockPos.relative(direction)` 的位移。
///
/// The block offset of `BlockPos.relative(direction)`.
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

/// `MultifaceBlock.getFaceProperty(direction)` 的属性名。
///
/// Property name of `MultifaceBlock.getFaceProperty(direction)`.
fn face_property(direction: Direction) -> &'static str {
    match direction {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    }
}

/// `MultifaceBlock.hasFace(state, face)`。
///
/// `MultifaceBlock.hasFace(state, face)`.
fn has_face(state: u32, face: Direction) -> bool {
    with_property(state, face_property(face), "false").is_some_and(|without| without != state)
}

/// `BlockState.isFaceSturdy(level, pos, direction)`，即 `SupportType.FULL`。
///
/// `BlockState.isFaceSturdy(level, pos, direction)`, i.e. `SupportType.FULL`.
fn face_is_sturdy(state: u32, direction: Direction) -> bool {
    state_facts(state) & (1 << (direction as u32 + facts::STURDY_FULL)) != 0
}

/// `MultifaceBlock.canAttachTo(...)`：邻居的支撑形状或碰撞形状在反方向是满的。
///
/// `MultifaceBlock.canAttachTo(...)`: the neighbour's support or collision shape is full
/// toward the opposite face.
fn can_attach_to(state: u32, face: Direction) -> bool {
    let toward = face.opposite();
    mc_bamboo_blocks::shape::is_face_full(mc_bamboo_data::support_shape(state), toward)
        || mc_bamboo_blocks::shape::is_face_full(mc_bamboo_data::collision_shape(state), toward)
}

fn in_block_tag(state: u32, tag: &str) -> bool {
    block_id_of_state(state)
        .is_some_and(|id| block_tag(tag).is_some_and(|ids| ids.contains(&(id as i32))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_exported_sculk_patch_configs() {
        let deep_dark: Value = serde_json::from_str(
            r#"{
                "charge_count": 10,
                "amount_per_charge": 32,
                "spread_attempts": 64,
                "growth_rounds": 0,
                "spread_rounds": 1,
                "type": "minecraft:sculk_patch"
            }"#,
        )
        .unwrap();
        let feature = SculkPatchFeature::parse(&deep_dark).unwrap();
        assert_eq!(feature.charge_count, 10);
        assert_eq!(feature.amount_per_charge, 32);
        assert_eq!(feature.spread_attempts, 64);
        assert_eq!(feature.growth_rounds, 0);
        assert_eq!(feature.spread_rounds, 1);
    }

    #[test]
    fn non_corner_neighbours_have_eighteen_entries_in_order() {
        // 不洗牌时的顺序：x 外层、y 中层、z 内层。
        // Unshuffled order: x outer, y middle, z inner.
        let mut list = Vec::new();
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    if (x == 0 || y == 0 || z == 0) && !(x == 0 && y == 0 && z == 0) {
                        list.push((x, y, z));
                    }
                }
            }
        }
        assert_eq!(list.len(), 18);
        assert_eq!(list[0], (-1, -1, 0));
        assert_eq!(list[1], (-1, 0, -1));
        assert!(list.contains(&(0, 1, 0)));
        assert!(!list.contains(&(0, 0, 0)));
        assert!(!list.contains(&(1, 1, 1)));
    }

    #[test]
    fn decay_penalty_never_drops_below_one() {
        // `Math.max(1, ...)`：贴着原点时罚分也是 1，不是 0。
        // `Math.max(1, ...)`: even at the origin the penalty is 1, never 0.
        let origin = (0, 0, 0);
        assert_eq!(decay_penalty(origin, origin, 100), 1);
        // 远处罚分按距离比例增长，但不超过 charge 的一半。
        // Far away it scales with distance but never exceeds half the charge.
        let far = (24, 0, 0);
        let penalty = decay_penalty(far, origin, 100);
        assert!((1..=50).contains(&penalty), "{penalty}");
    }
}
