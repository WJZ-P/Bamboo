//! 下落的方块，官方 `FallingBlockEntity`。
//!
//! 这类实体不调 `baseTick`：重力、移动、方块效果都在 `tick` 里自己做，落地之后
//! 把方块放回去。放不回去的时候官方会生成一个掉落物实体，这个世界没有掉落物，
//! 所以方块直接消失。

use mc_bamboo_core::{BlockPos, Direction, mth};
use mc_bamboo_data::{BlockAncestor, blocks, tags::block_tags};

use crate::{
    behaviour::common::is_free,
    context::PlaceContext,
    entity::{Entity, EntityKind, FallingBlock, physics},
    state::{BlockState, HORIZONTAL_FACING, WATERLOGGED},
    world::World,
};

impl Entity {
    /// `FallingBlockEntity.tick()`。
    pub(crate) fn tick_falling_block(&mut self, mut data: FallingBlock, world: &mut World) {
        if data.state.is_air() {
            self.discard();
        } else if self.tick_falling_entity(&mut data, world) {
            self.kind = EntityKind::FallingBlock(data);
        }
        // 这个 tick 撞到过东西，同步时不能省掉精确位置。
        self.set_requires_precise_position(self.horizontal_collision || self.vertical_collision);
    }

    /// 掉一格的具体流程。返回假表示实体已经不在了，不用再写回 `kind`。
    fn tick_falling_entity(&mut self, data: &mut FallingBlock, world: &mut World) -> bool {
        data.time += 1;
        self.apply_gravity();
        let movement = self.delta;
        self.move_entity(world, movement);
        // `move` 里的摔落伤害可能把铁砧摔裂，`blockState` 换成了裂开的那种。
        if let EntityKind::FallingBlock(updated) = self.kind {
            *data = updated;
        }
        self.apply_effects_from_blocks(world);
        // `handlePortal` 不在范围里。
        let mut pos = self.block_position();
        let is_concrete = data.state.is_a(BlockAncestor::ConcretePowderBlock);
        let mut stuck_in_water = is_concrete && world.fluid_state(pos).is_water();
        if is_concrete && self.delta.length_sqr() > 1.0 {
            let context = self.collision_context();
            if let Some(hit) =
                world.clip_collider_source_water(self.old_position, self.position, context)
            {
                if world.fluid_state(hit.pos).is_water() {
                    pos = hit.pos;
                    stuck_in_water = true;
                }
            }
        }
        if !self.on_ground && !stuck_in_water {
            let out_of_world = pos.y <= world.min_y() || pos.y > world.max_y();
            if data.time > 100 && out_of_world || data.time > 600 {
                // `dropItem` 决定要不要生成掉落物，这个世界没有掉落物，只剩消失。
                self.discard();
            }
        } else {
            let current_state = world.block_state(pos);
            self.delta = self.delta.multiply(0.7, -0.5, 0.7);
            if !current_state.is(blocks::MOVING_PISTON) {
                if data.cancel_drop {
                    self.discard();
                } else {
                    let land_here = self.can_land(data, current_state, is_concrete, stuck_in_water, world, pos);
                    if land_here {
                        let mut placed = data.state;
                        if placed.has(WATERLOGGED) {
                            let fluid = world.fluid_state(pos);
                            if fluid.is_water() && fluid.is_source() {
                                placed = placed.with(WATERLOGGED, true);
                            }
                        }
                        if world.set_block_and_update(pos, placed) {
                            data.state = placed;
                            self.discard();
                            crate::behaviour::misc::falling::on_land(
                                world,
                                pos,
                                data.state,
                                current_state,
                            );
                        } else if data.drop_item {
                            self.discard();
                        }
                    } else {
                        self.discard();
                    }
                }
            }
        }
        self.delta = self.delta.scale(physics::air_drag());
        !self.removed
    }

    /// `FallingBlockEntity.causeFallDamage`。砸到实体的伤害不在范围里，但铁砧会摔裂，
    /// 所以伤害数值和那一颗骰子照官方算。
    ///
    /// 返回的永远是假：官方最后返回的是 `super.causeFallDamage`，而下落方块自己不受伤。
    pub(crate) fn falling_block_cause_fall_damage(
        &mut self,
        mut data: FallingBlock,
        fall_distance: f64,
    ) -> bool {
        if !data.hurt_entities {
            return false;
        }
        let distance = mth::ceil(fall_distance - 1.0);
        if distance < 0 {
            return false;
        }
        // 官方这里是 `fallDistanceInt * fallDamagePerDistance`，在 `float` 上乘。
        let damage = mth::floor(f64::from(distance as f32 * data.fall_damage_per_distance))
            .min(data.fall_damage_max);
        // 骰子只在既是铁砧、伤害又大于零的时候掷。
        if data.state.is_tagged(block_tags::ANVIL)
            && damage > 0
            && self.random.next_float() < 0.05_f32 + distance as f32 * 0.05_f32
        {
            match anvil_damage(data.state) {
                Some(damaged) => data.state = damaged,
                // 已经是最破的铁砧：落地时方块直接消失，也不走 `onBrokenAfterFall`。
                None => data.cancel_drop = true,
            }
        }
        self.kind = EntityKind::FallingBlock(data);
        false
    }

    /// 落地那一格能不能换成这个方块。
    ///
    /// 官方还要两个条件：这一格能被替换，而且放下去之后不会接着往下掉。
    fn can_land(
        &self,
        data: &FallingBlock,
        current_state: BlockState,
        is_concrete: bool,
        stuck_in_water: bool,
        world: &World,
        pos: BlockPos,
    ) -> bool {
        // `DirectionalPlaceContext`：空手，点的是这一格的上面。
        let ctx = PlaceContext::directional(world, pos, Direction::Up);
        let may_replace = current_state.can_be_replaced(&ctx, world);
        let would_continue_falling = is_free(world.block_state(pos.below()))
            && (!is_concrete || !stuck_in_water);
        may_replace && data.state.can_survive(world, pos) && !would_continue_falling
    }
}

/// `AnvilBlock.damage`：铁砧裂一级，朝向不变。已经是最破的那种时返回 `None`。
fn anvil_damage(state: BlockState) -> Option<BlockState> {
    let facing = state.get(HORIZONTAL_FACING);
    if state.is(blocks::ANVIL) {
        Some(BlockState(blocks::CHIPPED_ANVIL).with(HORIZONTAL_FACING, facing))
    } else if state.is(blocks::CHIPPED_ANVIL) {
        Some(BlockState(blocks::DAMAGED_ANVIL).with(HORIZONTAL_FACING, facing))
    } else {
        None
    }
}
