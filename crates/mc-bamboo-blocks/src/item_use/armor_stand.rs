//! 盔甲架物品的 `useOn`：官方 `ArmorStandItem.useOn`。
//!
//! 面朝下、放置格被方块或实体占住都失败；否则按玩家朝向取整到 45° 摆一个盔甲架，
//! 应用物品上的 `entity_data`，发放置音效与 `ENTITY_PLACE` 事件，生存模式消耗一个。

use mc_bamboo_core::{Aabb, Direction, Vec3, mth};
use mc_bamboo_data::{registries::data_component_type, sounds};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue};

use crate::{
    CollisionContext,
    context::{PlaceContext, UseContext},
    entity::{
        Entity,
        mob::{MobType, armor_stand, nbt, spawn},
    },
    game_event::{GameEvent, dispatch::EventContext},
    interact::InteractionResult,
    item_exchange::ItemExchange,
    world::World,
};

/// `minecraft:entity.armor_stand.place` 的音效注册表 ID。
const ARMOR_STAND_PLACE: u16 = 84;

/// `ArmorStandItem.useOn`。
pub(super) fn use_on(world: &mut World, ctx: &UseContext) -> InteractionResult {
    if ctx.clicked_face() == Direction::Down {
        return InteractionResult::Fail;
    }
    let pos = PlaceContext::new(*ctx, world).clicked_pos();
    let center = Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y),
        f64::from(pos.z) + 0.5,
    );
    // `EntityTypes.ARMOR_STAND.getDimensions().makeBoundingBox(center)`：底心落在 center。
    // `EntityTypes.ARMOR_STAND.getDimensions().makeBoundingBox(center)`: bottom centre.
    let (width, height, _) = armor_stand::NORMAL_DIMENSIONS;
    let half = f64::from(width) / 2.0;
    let bounds = Aabb::new(
        center.x - half,
        center.y,
        center.z - half,
        center.x + half,
        center.y + f64::from(height),
        center.z + half,
    );
    // `level.noCollision(null, aabb)`：放置格不能有方块碰撞形状。
    // `level.noCollision(null, aabb)`: no block collision shape in the placement cell.
    if !world
        .block_collisions(bounds, CollisionContext::Empty)
        .is_empty()
    {
        return InteractionResult::Fail;
    }
    // `level.getEntities(null, aabb).isEmpty()`：也不能有别的实体占着。
    // `level.getEntities(null, aabb).isEmpty()`: no other entity may occupy it.
    let occupied = world.level().entities_table().iter_public().any(|public| {
        if public.removed {
            return false;
        }
        let (w, h) = public.kind.dimensions();
        let hw = f64::from(w) / 2.0;
        let other = Aabb::new(
            public.position.x - hw,
            public.position.y,
            public.position.z - hw,
            public.position.x + hw,
            public.position.y + f64::from(h),
            public.position.z + hw,
        );
        bounds.intersects(other)
    });
    if occupied {
        return InteractionResult::Fail;
    }

    // `EntityType.create(createDefaultStackConfig(...))`：构造、应用物品的 entity_data，
    // 再 `snapTo` 到 45° 朝向（官方 snapTo 的 xRot 恒为 0）。
    // `EntityType.create(createDefaultStackConfig(...))`: build, apply the item's
    // entity_data, then `snapTo` with the 45° yaw (vanilla's snapTo xRot is always 0).
    let mut entity =
        world.prepare_entity(Entity::by_mob_type(MobType::ArmorStand, Vec3::default()));
    let mut entries = entity_data_entries(ctx);
    entries.retain(|(key, _)| key != "id");
    entries.push((
        "id".to_owned(),
        Tag::String(MobType::ArmorStand.name().to_owned()),
    ));
    if nbt::load_into(&mut entity, &Tag::Compound(entries)).is_err() {
        return InteractionResult::Fail;
    }
    let yaw = ((mth::wrap_degrees(ctx.rotation() - 180.0) + 22.5) / 45.0).floor() * 45.0;
    spawn::snap_to(&mut entity, center, yaw, 0.0);
    entity.with_mob(|me, mob| {
        mob.y_head_rot = me.y_rot;
        mob.y_body_rot = me.y_rot;
        mob.needs_sync = true;
    });
    // `addFreshEntityWithPassengers`。
    let uid = world.add_prepared_entity(entity);
    world.finish_jockeys(uid);
    // `playSound(ARMOR_STAND_PLACE, BLOCKS, 0.75, 0.8)`，玩家自己也听得到。
    // `playSound(ARMOR_STAND_PLACE, BLOCKS, 0.75, 0.8)`, audible to the player too.
    world.play_sound(center, ARMOR_STAND_PLACE, sounds::BLOCKS, 0.75, 0.8, false);
    // `armorstand.gameEvent(ENTITY_PLACE, player)`。
    world.game_event(
        GameEvent::EntityPlace,
        center,
        EventContext {
            source_player: Some(ctx.actor.uid),
            ..Default::default()
        },
    );
    // `itemStack.shrink(1)`：生存模式减一个。
    // `itemStack.shrink(1)`: shrink one in survival.
    world.set_item_exchange(ItemExchange::Shrink);
    InteractionResult::Success
}

/// 物品 `entity_data` 组件里的 NBT 条目（`TypedEntityData` 已去掉 `id`）。
///
/// The NBT entries from the item's `entity_data` component (`TypedEntityData` drops `id`).
fn entity_data_entries(ctx: &UseContext) -> Vec<(String, Tag)> {
    ctx.stack
        .and_then(|stack| ComponentPatch::from_opaque(stack).ok())
        .and_then(|patch| {
            patch
                .get(data_component_type::ENTITY_DATA)
                .and_then(|value| match value {
                    ComponentValue::EntityData(data) => Some(data.tag.clone()),
                    _ => None,
                })
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{BlockPos, Direction, Hand, Vec3};
    use mc_bamboo_data::{blocks, items};
    use mc_bamboo_protocol::{
        components::{ComponentPatch, ComponentValue, PatchEntry, TypedEntityData},
        item::ItemStack,
    };

    use super::*;
    use crate::{Actor, BlockState, BlockTicks, context::BlockHit, testing::TestLevel};

    fn actor(yaw: f32) -> Actor {
        Actor {
            uid: 1,
            uuid: [0; 16],
            position: Vec3::new(3.5, 64.0, 5.5),
            yaw,
            head_yaw: yaw,
            pitch: 0.0,
            secondary_use: false,
            main_hand: items::ARMOR_STAND,
            off_hand: items::AIR,
            eye_height: 1.62,
            block_interaction_range: 5.0,
            may_build: true,
            prevents_block_drops: false,
            can_use_game_master_blocks: false,
        }
    }

    fn hit(pos: BlockPos, face: Direction) -> BlockHit {
        BlockHit {
            pos,
            face,
            location: Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y) + 0.5,
                f64::from(pos.z) + 0.5,
            ),
            inside: false,
        }
    }

    #[test]
    fn a_down_face_fails_without_spawning() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        let pos = BlockPos::new(3, 64, 5);
        let a = actor(0.0);
        let mut world = World::new(&mut level, &mut ticks);
        let ctx = UseContext::new(&a, Hand::Main, hit(pos, Direction::Down));
        assert_eq!(use_on(&mut world, &ctx), InteractionResult::Fail);
        let _ = world.into_effects();
        assert!(level.entities().ids().is_empty());
    }

    #[test]
    fn placing_snaps_the_facing_to_45_degrees() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        let pos = BlockPos::new(3, 64, 5);
        let a = actor(37.0);
        let mut world = World::new(&mut level, &mut ticks);
        let ctx = UseContext::new(&a, Hand::Main, hit(pos, Direction::Up));
        assert_eq!(use_on(&mut world, &ctx), InteractionResult::Success);
        let _ = world.into_effects();
        let ids = level.entities().ids();
        assert_eq!(ids.len(), 1, "one armour stand is spawned");
        let public = level
            .entities()
            .public_at(ids[0])
            .expect("the stand is placed");
        assert_eq!(public.position, Vec3::new(3.5, 64.0, 5.5));
        // floor((wrapDegrees(37 - 180) + 22.5) / 45) * 45 = -135.
        assert_eq!(public.yaw, -135.0);
    }

    #[test]
    fn a_solid_cell_blocks_the_placement() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        let pos = BlockPos::new(3, 64, 5);
        level.set(pos, BlockState(blocks::STONE).default_state());
        // 点实心方块顶面时 `BlockPlaceContext` 把放置挪到上一格；让那一格
        // 也有碰撞形状才应该失败。
        // Clicking a solid top shifts placement up a cell; that cell must be
        // blocked too for the placement to fail.
        level.set(
            pos.relative(Direction::Up),
            BlockState(blocks::STONE).default_state(),
        );
        let a = actor(0.0);
        let mut world = World::new(&mut level, &mut ticks);
        let ctx = UseContext::new(&a, Hand::Main, hit(pos, Direction::Up));
        assert_eq!(use_on(&mut world, &ctx), InteractionResult::Fail);
        let _ = world.into_effects();
        assert!(level.entities().ids().is_empty());
    }

    #[test]
    fn the_item_entity_data_is_applied_to_the_stand() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        let pos = BlockPos::new(3, 64, 5);
        let a = actor(0.0);
        let mut stack = ItemStack::new(items::ARMOR_STAND, 1);
        let mut patch = ComponentPatch::from_opaque(&stack).unwrap_or_default();
        patch.put(
            data_component_type::ENTITY_DATA,
            PatchEntry::Set(ComponentValue::EntityData(TypedEntityData {
                kind: mc_bamboo_data::registries::entity_type::ARMOR_STAND,
                tag: vec![
                    ("Small".to_owned(), Tag::Byte(1)),
                    ("Marker".to_owned(), Tag::Byte(1)),
                ],
            })),
        );
        let (added, removed) = patch.to_opaque();
        stack.added = added;
        stack.removed = removed;
        let mut world = World::new(&mut level, &mut ticks);
        let ctx = UseContext {
            actor: &a,
            hand: Hand::Main,
            hit: hit(pos, Direction::Up),
            item: items::ARMOR_STAND,
            stack: Some(&stack),
        };
        assert_eq!(use_on(&mut world, &ctx), InteractionResult::Success);
        let _ = world.into_effects();
        let ids = level.entities().ids();
        let public = level
            .entities()
            .public_at(ids[0])
            .expect("the stand is placed");
        // 标记盔甲架的碰撞盒是 (0, 0)。
        assert_eq!(public.kind.dimensions(), (0.0, 0.0));
    }
}
