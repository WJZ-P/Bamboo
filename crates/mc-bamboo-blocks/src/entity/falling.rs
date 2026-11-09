//! 下落的方块，官方 `FallingBlockEntity`。
//!
//! 这类实体不调 `baseTick`：重力、移动、方块效果都在 `tick` 里自己做，落地之后
//! 把方块放回去。放不回去、或者掉出世界时，`dropItem` 且 `entity_drops` 为真
//! 才 `spawnAtLocation`。
//!
//! These entities do not call `baseTick`. When the block cannot be placed back, or it
//! falls out of the world, `dropItem` spawns the block's item via `spawnAtLocation`,
//! but only while the `entity_drops` game rule is on.

use mc_bamboo_core::{BlockPos, Direction, Vec3, mth};
use mc_bamboo_data::{BlockAncestor, blocks, tags::block_tags};
use mc_bamboo_protocol::item::ItemStack;

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
        let entity_drops = world.block_game_rules().entity_drops;
        data.time += 1;
        self.apply_gravity();
        let movement = self.delta;
        self.move_entity(world, movement);
        // `move` 里的摔落伤害可能把铁砧摔裂或设上 `cancelDrop`。只取这两个字段：
        // `data.time` 是这一拍刚加过的，整份拷回来会让 100/600 刻超时永远不触发。
        // `move` may crack an anvil or set `cancelDrop`. Copy only those fields: `data.time`
        // was just incremented, and a whole-struct copy would stop the timeout from ever firing.
        if let EntityKind::FallingBlock(updated) = &self.kind {
            data.state = updated.state;
            data.cancel_drop = updated.cancel_drop;
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
                && world.fluid_state(hit.pos).is_water()
            {
                pos = hit.pos;
                stuck_in_water = true;
            }
        }
        if !self.on_ground && !stuck_in_water {
            // 官方比的是方块坐标：`pos.getY() <= getMinY() || pos.getY() > getMaxY()`。
            // `LevelHeightAccessor.getMaxY()` 是 `minY + height - 1`，和这里的 `max_y` 一样含顶层。
            //
            // Official compares the block position: `pos.getY() <= getMinY() || pos.getY() > getMaxY()`.
            // `LevelHeightAccessor.getMaxY()` is `minY + height - 1`, the same inclusive top as `max_y`.
            let out_of_world = pos.y <= world.min_y() || pos.y > world.max_y();
            if data.time > 100 && out_of_world || data.time > 600 {
                // 超时先 `spawnAtLocation` 再 `discard`。`cancelDrop` 不参与这一支，
                // 这一支也不调用 `callOnBrokenAfterFall`。
                // A timeout calls `spawnAtLocation` before `discard`. `cancelDrop` does not
                // apply, and `callOnBrokenAfterFall` is not invoked on this path.
                if data.drop_item && entity_drops {
                    self.spawn_falling_drop(world, data.state);
                }
                self.discard();
            }
        } else {
            let current_state = world.block_state(pos);
            self.delta = self.delta.multiply(0.7, -0.5, 0.7);
            if !current_state.is(blocks::MOVING_PISTON) {
                if data.cancel_drop {
                    // `cancelDrop` 不放回方块，但 `callOnBrokenAfterFall` 仍然要走。
                    // `cancelDrop` does not place the block, but `callOnBrokenAfterFall` still runs.
                    let center = bounding_box_center(self);
                    let state = data.state;
                    self.discard();
                    crate::behaviour::misc::falling::on_broken_after_fall(
                        world, pos, state, center,
                    );
                } else {
                    let land_here =
                        self.can_land(data, current_state, is_concrete, stuck_in_water, world, pos);
                    if land_here {
                        let mut placed = data.state;
                        if placed.has(WATERLOGGED) {
                            // `FluidState.is(Fluids.WATER)` 只认水源，流动的水不算。
                            // `FluidState.is(Fluids.WATER)` is the source fluid, not flowing water.
                            if world
                                .fluid_state(pos)
                                .is(mc_bamboo_data::registries::fluid::WATER)
                            {
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
                        } else if data.drop_item && entity_drops {
                            // 放不回去：先丢掉实体，再 `callOnBrokenAfterFall`，再生成掉落。
                            // 音效和 `onBrokenAfterFall` 不取 `level.random`。
                            //
                            // Placement failed: discard, then `callOnBrokenAfterFall`, then the drop.
                            // The sound and `onBrokenAfterFall` do not draw from `level.random`.
                            let center = bounding_box_center(self);
                            let state = data.state;
                            self.discard();
                            crate::behaviour::misc::falling::on_broken_after_fall(
                                world, pos, state, center,
                            );
                            self.spawn_falling_drop(world, state);
                        }
                    } else {
                        let center = bounding_box_center(self);
                        let state = data.state;
                        let drop_item = data.drop_item && entity_drops;
                        self.discard();
                        if drop_item {
                            crate::behaviour::misc::falling::on_broken_after_fall(
                                world, pos, state, center,
                            );
                            self.spawn_falling_drop(world, state);
                        }
                    }
                }
            }
        }
        self.delta = self.delta.scale(physics::air_drag());
        !self.removed
    }

    /// `Entity.spawnAtLocation(level, block)`：用方块对应的物品，空气不生成也不取实体随机数。
    ///
    /// `Entity.spawnAtLocation(level, block)`: the block's item. Air spawns nothing and draws no entity random.
    fn spawn_falling_drop(&self, world: &mut World, state: BlockState) {
        world.spawn_at_location(self.position, ItemStack::new(state.block().item, 1));
    }

    /// `FallingBlockEntity.causeFallDamage`。砸到实体的伤害不在范围里，但铁砧会摔裂，
    /// 所以伤害数值和那一颗骰子照官方算。
    ///
    /// 返回的永远是假：官方这个方法最后也是 `return false`，下落方块自己不受伤。
    ///
    /// Entity damage is out of scope, but an anvil can crack, so the damage value and that one roll follow vanilla.
    ///
    /// This always returns false: vanilla's method also ends with `return false`. A falling block does not hurt itself.
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
        // The roll happens only for an anvil whose fall damage is above zero.
        if data.state.is_tagged(block_tags::ANVIL)
            && damage > 0
            && self.random.next_float() < 0.05_f32 + distance as f32 * 0.05_f32
        {
            match anvil_damage(data.state) {
                Some(damaged) => data.state = damaged,
                // 已经是最破的铁砧：落地时方块直接消失。`cancelDrop` 仍会走 `onBrokenAfterFall`（1029）。
                // The anvil is already fully damaged: the block vanishes on landing.
                // `cancelDrop` still runs `onBrokenAfterFall` (1029).
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
        let would_continue_falling =
            is_free(world.block_state(pos.below())) && (!is_concrete || !stuck_in_water);
        may_replace && data.state.can_survive(world, pos) && !would_continue_falling
    }
}

/// `AABB.getCenter`。刷子方块的 2001 发在碰撞盒中心。
///
/// `AABB.getCenter`. A brushable block's 2001 is sent at the bounding-box centre.
fn bounding_box_center(entity: &Entity) -> Vec3 {
    let bounds = entity.bounding_box();
    Vec3::new(
        (bounds.min_x + bounds.max_x) * 0.5,
        (bounds.min_y + bounds.max_y) * 0.5,
        (bounds.min_z + bounds.max_z) * 0.5,
    )
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

#[cfg(test)]
mod tests {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::{blocks, items};
    use mc_bamboo_protocol::item::ItemStack;

    use crate::{BlockTicks, World, entity::EntityKind, state::BlockState, testing::TestLevel};

    const GROUND: BlockPos = BlockPos::new(0, 64, 0);

    /// 从 `start` 开始掉，直到世界里出现第一个物品实体或下落方块没了。
    ///
    /// Drops from `start` until the first ground item appears or the falling block is gone.
    fn fall_until_spawn(
        level: &mut TestLevel,
        start: BlockPos,
        state: u32,
        limit: usize,
    ) -> Vec<crate::entity::Entity> {
        let mut ticks = BlockTicks::new();
        let mut world = World::new(level, &mut ticks);
        let id = world.spawn_falling_block(start, BlockState(state));
        // 官方的 `time > 100 && out_of_world || time > 600`：这里要测的是 600 刻那一支，
        // 所以把 `time` 直接设到边界前，让方块在没有地面可落的世界里继续掉。
        if start.y < 0
            && let Some(entity) = world.entity_mut(id)
            && let EntityKind::FallingBlock(data) = &mut entity.kind
        {
            data.time = 590;
        }
        for _ in 0..limit {
            world.tick_entities();
            let live = world.live_entities();
            if live.iter().any(|entity| entity.ground().is_some()) || world.entity_ids().is_empty()
            {
                return live;
            }
        }
        world.live_entities()
    }

    /// 落地放不回去时，只有 `entity_drops` 开着才 `spawnAtLocation`。
    ///
    /// Only an enabled `entity_drops` rule spawns the item when landing fails.
    #[test]
    fn a_failed_landing_drops_only_when_entity_drops_is_on() {
        for enabled in [true, false] {
            let mut level = TestLevel::new();
            level.rules.entity_drops = enabled;
            level.set(GROUND, BlockState(blocks::STONE));
            // 火把占着落点，沙子不能替换它。
            level.set(BlockPos::new(0, 65, 0), BlockState(blocks::TORCH));
            let live = fall_until_spawn(&mut level, BlockPos::new(0, 66, 0), blocks::SAND, 8);
            let items: Vec<_> = live.iter().filter_map(|entity| entity.ground()).collect();
            assert_eq!(items.len(), usize::from(enabled), "entity_drops={enabled}");
            if enabled {
                assert_eq!(items[0].stack, ItemStack::new(items::SAND, 1));
                assert_eq!(items[0].pickup_delay, 10);
            }
        }
    }

    /// 超过 600 刻时按同一条规则生成掉落物，不调用 `onBrokenAfterFall`。
    ///
    /// The 600-tick timeout spawns the item under the same rule, without `onBrokenAfterFall`.
    #[test]
    fn a_timeout_drops_only_when_entity_drops_is_on() {
        for enabled in [true, false] {
            let mut level = TestLevel::new();
            level.rules.entity_drops = enabled;
            // 世界底部又没有地面时，方块一路掉到 `min_y` 以下；`time > 100` 之后
            // 的那一支和 `time > 600` 共用同一段 `spawnAtLocation` + `discard`。
            let live = fall_until_spawn(&mut level, BlockPos::new(0, -50, 0), blocks::SAND, 620);
            eprintln!(
                "enabled={enabled} live={:?}",
                live.iter()
                    .map(|e| (e.id, e.position.y, e.ground().map(|g| g.stack.clone())))
                    .collect::<Vec<_>>()
            );
            let items: Vec<_> = live.iter().filter_map(|entity| entity.ground()).collect();
            assert_eq!(items.len(), usize::from(enabled), "entity_drops={enabled}");
            if enabled {
                assert_eq!(items[0].stack, ItemStack::new(items::SAND, 1));
            }
        }
    }
}
