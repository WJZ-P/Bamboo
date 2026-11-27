//! `Block.fallOn` 的伤害参数和落地时的方块变化。
//! Damage parameters and block changes from `Block.fallOn`.

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{BlockAncestor, blocks};

use crate::{
    BlockState, World,
    game_event::{GameEvent, dispatch::EventContext},
    state::{SPELEOTHEM_THICKNESS, SpeleothemThickness, VERTICAL_DIRECTION},
    world::block_center_public,
};

use super::{Entity, mob::DamageKind};

/// 方块传给 `Entity.causeFallDamage` 的参数。返回 `None` 时方块不调用伤害入口。
/// Parameters passed by the block to `Entity.causeFallDamage`. `None` skips the call.
pub fn fall_damage(
    state: BlockState,
    distance: f64,
    suppresses_bounce: bool,
) -> Option<(f64, f32, DamageKind)> {
    if state.is(blocks::SLIME_BLOCK) {
        return (!suppresses_bounce).then_some((distance, 0.0, DamageKind::Fall));
    }
    if state.is_a(BlockAncestor::PowderSnowBlock) {
        return None;
    }
    if state.is(blocks::HAY_BLOCK) || state.is(blocks::HONEY_BLOCK) {
        return Some((distance, 0.2, DamageKind::Fall));
    }
    if state.is(blocks::POINTED_DRIPSTONE)
        && state.get(VERTICAL_DIRECTION) == Direction::Up
        && state.get(SPELEOTHEM_THICKNESS) == SpeleothemThickness::Tip
    {
        return Some((distance + 2.5, 2.0, DamageKind::Stalagmite));
    }
    let reduction = if state.is_a(BlockAncestor::BedBlock) || state.is(blocks::SHELF_MUSHROOM) {
        0.5_f32
    } else {
        0.0
    };
    Some((distance * f64::from(1.0 - reduction), 1.0, DamageKind::Fall))
}

/// `FarmlandBlock.fallOn` 的体积门限，用 float 运算。
/// The volume threshold in `FarmlandBlock.fallOn`, with float arithmetic.
pub fn can_trample_farmland(width: f32, height: f32) -> bool {
    width * width * height > 0.512_f32
}

/// `FarmlandBlock.turnToBaseBlock`，移动中的生物可能暂时脱离实体表。
/// `FarmlandBlock.turnToBaseBlock`; the moving mob may be detached from the entity table.
pub fn turn_farmland_to_dirt(
    world: &mut World<'_>,
    pos: BlockPos,
    mut context: EventContext,
    detached: Option<&mut Entity>,
) {
    let state = world.block_state(pos);
    if !state.is(blocks::FARMLAND) {
        return;
    }
    let dirt = BlockState(blocks::DIRT);
    world.push_entities_up_with_detached(pos, state, dirt, detached);
    world.set_block_and_update(pos, dirt);
    context.affected_state = Some(dirt);
    world.game_event(GameEvent::BlockChange, block_center_public(pos), context);
}

/// `TurtleEggBlock.destroyEgg` 的随机数和方块更新。调用方检查实体是否有权限踩蛋。
/// Random draw and block update in `TurtleEggBlock.destroyEgg`. The caller checks eligibility.
pub fn trample_turtle_egg(world: &mut World<'_>, pos: BlockPos, chance: i32) {
    let state = world.block_state(pos);
    if state.is(blocks::TURTLE_EGG) && chance > 0 && world.random().next_int(chance) == 0 {
        crate::behaviour::misc::turtle_egg::decrease_eggs(world, pos, state);
    }
}

/// 落地粒子取脚下方块。只有实体中心越过方块边界时才把粒子位置收回到边缘。
/// Landing particles use the supporting block and move to its edge when the entity overhangs.
pub fn fall_particles(
    position: Vec3,
    pos: BlockPos,
    state: BlockState,
    power: f64,
) -> Option<(Vec3, i32)> {
    let power = mc_bamboo_core::mth::floor(power);
    if power <= 0 || state.is_air() {
        return None;
    }
    let mut at = position;
    let feet = BlockPos::containing(position);
    if feet.x != pos.x || feet.z != pos.z {
        let x = position.x - f64::from(pos.x) - 0.5;
        let z = position.z - f64::from(pos.z) - 0.5;
        let max = x.abs().max(z.abs());
        at.x = f64::from(pos.x) + 0.5 + x / max * 0.5;
        at.z = f64::from(pos.z) + 0.5 + z / max * 0.5;
    }
    let spread = (f64::from(0.2_f32) + f64::from(power) / 15.0).min(2.5);
    Some((at, (150.0 * spread) as i32))
}

/// `Entity.getOnPosLegacy` 的零点二格偏移，优先取支撑方块的 X/Z。
/// The 0.2-block offset in `Entity.getOnPosLegacy`, using the support block's X/Z.
pub fn on_pos_legacy(
    position: Vec3,
    supporting: Option<BlockPos>,
    block_at: impl FnOnce(BlockPos) -> BlockState,
) -> BlockPos {
    use mc_bamboo_data::tags::block_tags;

    let below = BlockPos::containing(Vec3::new(
        position.x,
        position.y - f64::from(0.2_f32),
        position.z,
    ));
    let Some(main) = supporting else {
        return below;
    };
    let state = block_at(main);
    if state.is_tagged(block_tags::FENCES)
        || state.is_tagged(block_tags::WALLS)
        || state.is_a(BlockAncestor::FenceGateBlock)
    {
        main
    } else {
        BlockPos::new(main.x, below.y, main.z)
    }
}
