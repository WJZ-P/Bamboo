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
    // `level.getEntities(null, aabb).isEmpty()`：方块、玩家和其它实体都不能占着。
    // 官方的 `getEntities` 把场景里所有玩家算进去；交互者由 `UseContext.actor` 给出，
    // 玩家碰撞盒宽 0.6、高 1.8（潜行只改眼高，不改盒子），旁观玩家的盒由
    // `Level::is_unobstructed` 查（真实服务端里它查所有在线玩家）。
    //
    // `level.getEntities(null, aabb).isEmpty()`: no block, player or other entity may
    // occupy it. Vanilla counts every player in the scene; the actor comes from
    // `UseContext.actor`, with a 0.6 x 1.8 box (crouching changes the eye height, not
    // the box), and bystanders' boxes are checked through `Level::is_unobstructed`
    // (all online players on a real server).
    let actor = ctx.actor;
    let player_blocks = {
        let hw = 0.3_f64;
        let other = Aabb::new(
            actor.position.x - hw,
            actor.position.y,
            actor.position.z - hw,
            actor.position.x + hw,
            actor.position.y + 1.8,
            actor.position.z + hw,
        );
        bounds.intersects(other)
    };
    let entity_blocks = world.level().entities_table().iter_public().any(|public| {
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
    // `getEntities` 里的旁观玩家：`OracleLevel` 把它们放在场景实体盒里，服务端是
    // 在线玩家表，两种实现都由 `is_unobstructed` 统一回答。
    //
    // Bystander players in `getEntities`: the oracle keeps them as scene entity boxes,
    // a real server as online players; both are answered by `is_unobstructed`.
    let bystander_blocks = !world.level().is_unobstructed(std::slice::from_ref(&bounds));
    if player_blocks || entity_blocks || bystander_blocks {
        return InteractionResult::Fail;
    }

    // `EntityType.create(createDefaultStackConfig(...))`：构造、应用物品的 entity_data，
    // 再 `snapTo` 到 45° 朝向（官方 snapTo 的 xRot 恒为 0）。
    // `EntityType.create(createDefaultStackConfig(...))`: build, apply the item's
    // entity_data, then `snapTo` with the 45° yaw (vanilla's snapTo xRot is always 0).
    let mut entity =
        world.prepare_entity(Entity::by_mob_type(MobType::ArmorStand, Vec3::default()));
    // `EntityType.create(..., SPAWN_ITEM_USE, true, true)`：`shouldOffsetY` 与
    // `shouldOffsetYMore` 都是 true（刷怪蛋的 useOn 后者看点面，盔甲架恒为真）。
    // 先摆到 `pos.y + 1`，再按 `getYOffset` 的碰撞扫描算下落量。
    //
    // `EntityType.create(..., SPAWN_ITEM_USE, true, true)`: both `shouldOffsetY` and
    // `shouldOffsetYMore` are true (a spawn egg conditions the latter on the face; the
    // armour stand always uses true). Place at pos.y + 1, then drop by `getYOffset`.
    entity.position = Vec3::new(center.x, f64::from(pos.y + 1), center.z);
    let offset = spawn::spawn_y_offset(&*world, pos, true, entity.bounding_box());
    let at = Vec3::new(center.x, f64::from(pos.y) + offset, center.z);
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
    // 官方随后 `stand.snapTo(stand.getX/Y/Z(), yaw, 0)` 只改朝向；这里用算出的落点
    // 覆盖 `loadInto` 在物品没有 `Pos` 组件时清零的坐标。
    //
    // Vanilla's following `stand.snapTo(getX/Y/Z(), yaw, 0)` only changes the yaw; put
    // the computed landing point back since a stack without `Pos` loads a zero here.
    spawn::snap_to(&mut entity, at, yaw, 0.0);
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
    world.play_sound(at, ARMOR_STAND_PLACE, sounds::BLOCKS, 0.75, 0.8, false);
    // `armorstand.gameEvent(ENTITY_PLACE, player)`。
    world.game_event(
        GameEvent::EntityPlace,
        at,
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
            // 玩家不能站在放置格里：官方 `getEntities(null, aabb)` 会把玩家算进去。
            // The player must not stand in the placement cell: vanilla's
            // `getEntities(null, aabb)` counts the player.
            position: Vec3::new(1.5, 64.0, 5.5),
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

    /// `EntityType.create(..., SPAWN_ITEM_USE, true, true)` 的 `getYOffset`：点顶面时
    /// 放置格下面有支撑，`yOffset = 0`，盔甲架站在方块顶；点侧面时放置格悬空，
    /// `yOffset = -1`，盔甲架比放置格低一格。两种 x/z 都是放置格中心。
    #[test]
    fn top_face_lands_on_the_block_and_side_face_drops_one() {
        // 顶面：点击 (3, 63, 5) 石头的上面，放置格 (3, 64, 5) 下方有石头。
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        level.set(
            BlockPos::new(3, 63, 5),
            BlockState(blocks::STONE).default_state(),
        );
        let a = actor(0.0);
        let mut world = World::new(&mut level, &mut ticks);
        let ctx = UseContext::new(&a, Hand::Main, hit(BlockPos::new(3, 63, 5), Direction::Up));
        assert_eq!(use_on(&mut world, &ctx), InteractionResult::Success);
        let _ = world.into_effects();
        let ids = level.entities().ids();
        assert_eq!(ids.len(), 1, "one stand on the top face");
        assert_eq!(
            level.entities().public_at(ids[0]).unwrap().position,
            Vec3::new(3.5, 64.0, 5.5)
        );

        // 侧面：点击 (3, 64, 5) 石头的东面，放置格 (4, 64, 5) 悬空。
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        level.set(
            BlockPos::new(3, 64, 5),
            BlockState(blocks::STONE).default_state(),
        );
        let a = actor(0.0);
        let mut world = World::new(&mut level, &mut ticks);
        let ctx = UseContext::new(
            &a,
            Hand::Main,
            hit(BlockPos::new(3, 64, 5), Direction::East),
        );
        assert_eq!(use_on(&mut world, &ctx), InteractionResult::Success);
        let _ = world.into_effects();
        let ids = level.entities().ids();
        assert_eq!(ids.len(), 1, "one stand on the side face");
        assert_eq!(
            level.entities().public_at(ids[0]).unwrap().position,
            Vec3::new(4.5, 63.0, 5.5)
        );
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
        // 点在空气格上，下方没有支撑，`getYOffset = -1`：落点比放置格低一格。
        // The clicked cell is air with nothing below, so `getYOffset = -1`.
        assert_eq!(public.position, Vec3::new(3.5, 63.0, 5.5));
        // floor((wrapDegrees(37 - 180) + 22.5) / 45) * 45 = -135.
        assert_eq!(public.yaw, -135.0);
    }

    /// 官方 `getEntities(null, aabb)` 把场景里所有玩家算进去；旁观玩家占着放置格时
    /// `Level::is_unobstructed` 为假，放置必须失败（`use tripwire_hook` 那道题）。
    #[test]
    fn a_bystander_in_the_placement_cell_blocks_it() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        let pos = BlockPos::new(3, 64, 5);
        // `is_unobstructed` 由旁观玩家盒决定；这里直接让场景里有实体挡着。
        level.obstructed = true;
        let a = actor(0.0);
        let mut world = World::new(&mut level, &mut ticks);
        let ctx = UseContext::new(&a, Hand::Main, hit(pos, Direction::Up));
        assert_eq!(use_on(&mut world, &ctx), InteractionResult::Fail);
        let _ = world.into_effects();
        assert!(level.entities().ids().is_empty());
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
