//! 玩家丢掉和捡起地上的物品。官方 `Player.drop`、`LivingEntity.createItemStackToDrop` 和 `ItemEntity.playerTouch`。
//!
//! 丢出时的朝向和散开用玩家自己的随机源。物品实体构造时还会用它自己的随机源取朝向和初速度，
//! 随后被这里算出来的速度盖掉。
//!
//! Throwing uses the player's own random source. The item entity also draws its facing and
//! initial velocity from its own random source, and this module then overwrites that velocity.

use mc_bamboo_blocks::{
    World,
    entity::{ENTITY_RANDOM_SEED, Entity, JavaRandom},
};
use mc_bamboo_core::{Vec3, mth};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{GameEvent, GameMode},
};

use crate::{
    entity_tracking,
    inbound::PlayerId,
    inventory::{self, Inventory},
    links,
    player::Player,
    region::Region,
};

/// `TickThrottler(20, 1480)` 的阈值。创造模式把物品拖出界面时用。
const CREATIVE_DROP_THRESHOLD: i32 = 1480;
const CREATIVE_DROP_STEP: i32 = 20;

/// 每 tick 给创造模式丢弃限流减 1。
///
/// Decrements the creative-drop throttle once per tick.
pub(crate) fn tick_drop_throttle(region: &mut Region) {
    for player in region.players.values_mut() {
        if player.drop_spam > 0 {
            player.drop_spam -= 1;
        }
    }
}

/// Q 和 Ctrl+Q。旁观模式什么也不丢。空着手再按一次也不取随机数。
///
/// Q and Ctrl+Q. Spectators drop nothing. An empty hand draws no random numbers.
pub(crate) fn drop_held(region: &mut Region, id: PlayerId, all: bool) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if player.game_mode == GameMode::Spectator {
        return;
    }
    let slot = player.inventory.hand_slot(mc_bamboo_core::Hand::Main);
    let selected = player.inventory.slots[slot].clone();
    if selected.is_empty() {
        return;
    }
    let amount = if all { selected.count } else { 1 };
    let mut removed = selected.clone();
    removed.count = amount;
    if let Some(player) = region.players.get_mut(&id) {
        let stack = &mut player.inventory.slots[slot];
        stack.count -= amount;
        if stack.count <= 0 {
            *stack = ItemStack::empty();
        }
    }
    spawn_throw(region, id, removed);
}

/// 创造模式把物品拖出界面。限流没过才生成实体。
///
/// Creative mode dragging a stack outside the screen. An entity is spawned only under the throttle.
pub(crate) fn creative_drop_outside(region: &mut Region, id: PlayerId, stack: ItemStack) {
    if stack.is_empty() {
        return;
    }
    {
        let Some(player) = region.players.get_mut(&id) else {
            return;
        };
        if player.game_mode != GameMode::Creative || player.drop_spam >= CREATIVE_DROP_THRESHOLD {
            return;
        }
        player.drop_spam += CREATIVE_DROP_STEP;
    }
    spawn_throw(region, id, stack);
}

/// 菜单里丢出，以及背包放不下时丢出。速度公式和 Q 一样。
///
/// A menu throw, or a stack that no longer fits. The velocity formula matches Q.
pub(crate) fn spawn_throw(region: &mut Region, id: PlayerId, stack: ItemStack) {
    if stack.is_empty() {
        return;
    }
    let (position, delta) = {
        let Some(player) = region.players.get_mut(&id) else {
            return;
        };
        (throw_position(player), throw_delta(player))
    };
    let (mut level, ticks) = region.block_level();
    let mut world = World::new(&mut level, ticks);
    let thrown = Entity::ground_item(position, stack);
    let entity_id = world.spawn_entity(thrown);
    world.with_entity_mut(entity_id, &mut |entity| {
        mc_bamboo_blocks::entity::set_throw_velocity(entity, delta);
    });
}

/// 实体 tick 之前，玩家碰得到的物品先捡起来。
///
/// Items a player is touching are picked up before entity ticks.
pub(crate) fn pickup_before_entities(region: &mut Region) {
    // 官方 `Player.tick` 先减拾取冷却，实体再碰玩家。
    // Vanilla `Player.tick` decrements the pickup cooldown before entities touch players.
    for player in region.players.values_mut() {
        if player.take_xp_delay > 0 {
            player.take_xp_delay -= 1;
        }
    }
    let players: Vec<PlayerId> = region.players.keys().copied().collect();
    for id in players {
        pickup_for(region, id);
    }
}

/// `AbstractCubeMob.playerTouch`：玩家碰到史莱姆，史莱姆能打就打一下，打中了放攻击音效。
/// 官方在 `Player.aiStep` 的 `touch(entity)` 里调，要求玩家活着。
///
/// `AbstractCubeMob.playerTouch`: a slime the player touches hits back when it can and plays
/// its attack sound on a hit. Vanilla calls it from `touch(entity)` in `Player.aiStep`, for
/// living players only.
fn slime_touch(region: &mut Region, id: PlayerId, slime: mc_bamboo_blocks::entity::EntityUid) {
    let alive = region
        .players
        .get(&id)
        .is_some_and(|player| !player.health.is_dead_or_dying());
    if !alive {
        return;
    }
    // 带玩家那一半的世界：受伤当场走正式入口，和生物在自己 tick 里打人是同一条路。
    // A world with the player half: the hit runs the real hurt path at once, the same way
    // a mob hits from its own tick.
    let (mut level, ticks) = region.entity_level();
    let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
    mc_bamboo_blocks::entity::mob::slime::player_touch(&mut world, slime, id.0);
}

/// `Pufferfish.playerTouch`：玩家碰到膨胀的河豚时受伤、中毒，没被静音时再给这名玩家
/// 发一次 `PUFFER_FISH_STING` 游戏事件（客户端播蜇刺声）。官方在 `Player.aiStep` 的
/// `touch(entity)` 里调，没有近战范围判定。
///
/// `Pufferfish.playerTouch`: hurt and poison a player touching an inflated pufferfish,
/// then send the `PUFFER_FISH_STING` game event to that player unless silenced. Vanilla
/// calls it from `touch(entity)` in `Player.aiStep` with no melee-range check.
/// `SulfurCube.playerTouch` 里 HOT 原型的接触伤害。
fn sulfur_cube_touch(region: &mut Region, id: PlayerId, cube: mc_bamboo_blocks::entity::EntityUid) {
    let alive = region
        .players
        .get(&id)
        .is_some_and(|player| !player.health.is_dead_or_dying());
    if !alive {
        return;
    }
    let (mut level, ticks) = region.entity_level();
    let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
    mc_bamboo_blocks::entity::mob::sulfur_cube::contact_player_touch(&mut world, cube, id.0);
}

fn puffer_touch(
    region: &mut Region,
    id: PlayerId,
    fish: mc_bamboo_blocks::entity::EntityUid,
    silent: bool,
) {
    let alive = region
        .players
        .get(&id)
        .is_some_and(|player| !player.health.is_dead_or_dying());
    if !alive {
        return;
    }
    let seconds = {
        let (mut level, ticks) = region.entity_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        mc_bamboo_blocks::entity::mob::fish::puffer_player_hit(&mut world, fish, id.0)
    };
    let Some(seconds) = seconds else {
        return;
    };
    // 官方顺序：先发游戏事件，再 `addEffect`。
    // Vanilla order: the game event first, then `addEffect`.
    if !silent {
        let frames = links::encode_frames(&GameEvent {
            event: GameEvent::PUFFER_FISH_STING,
            param: 0.0,
        });
        region.links.send_frames(id, &frames);
    }
    let (mut level, ticks) = region.entity_level();
    let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
    mc_bamboo_blocks::entity::mob::fish::puffer_player_poison(&mut world, id.0, seconds);
}

fn pickup_for(region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if player.game_mode == GameMode::Spectator {
        return;
    }
    let reach = player.bounding_box().inflate(1.0, 0.5, 1.0);
    let owner = *player.uuid.as_bytes();
    let creative = player.game_mode == GameMode::Creative;
    // 官方 `Player.aiStep` 用一次 `Level.getEntities(this, aabb)` 按实体分区
    // 顺序（`SectionPos.asLong` 升序）取候选：经验球和非经验球都在这个顺序里，
    // 最后 `Util.getRandom(orbs, random)` 只挑一颗。`for_each_in_section_order`
    // 复刻这个顺序；`intersects(reach)` 在遍历内部精确过滤。
    //
    // Vanilla `Player.aiStep` collects candidates with one
    // `Level.getEntities(this, aabb)` in entity-section order
    // (`SectionPos.asLong` ascending); both orbs and other entities come from
    // that order, and `Util.getRandom(orbs, random)` picks one orb at the end.
    let mut ids = Vec::new();
    let mut orbs = Vec::new();
    region
        .entities
        .for_each_in_section_order(reach, &mut |public| {
            if matches!(
                public.kind,
                mc_bamboo_blocks::entity::KindPublic::ExperienceOrb { .. }
            ) {
                orbs.push(public.uid);
            } else {
                ids.push(public.uid);
            }
        });
    for entity_id in ids {
        let Some(entity) = region.entities.get(entity_id) else {
            continue;
        };
        if let mc_bamboo_blocks::entity::EntityKind::Mob(mob) = &entity.kind {
            let mob_type = mob.mob_type;
            let silent = mob.silent;
            // 官方 `Player.aiStep` 只碰 `!isRemoved()` 的实体；已经打上移除标记的还留在
            // 这一拍的名单里。
            // Vanilla `Player.aiStep` only touches entities with `!isRemoved()`; a
            // discarded entity still sits in this tick's list.
            if entity.removed {
                continue;
            }
            if mob_type == mc_bamboo_blocks::entity::mob::MobType::Slime {
                slime_touch(region, id, entity_id);
            } else if mob_type == mc_bamboo_blocks::entity::mob::MobType::SulfurCube {
                sulfur_cube_touch(region, id, entity_id);
            } else if mob_type == mc_bamboo_blocks::entity::mob::MobType::Pufferfish {
                puffer_touch(region, id, entity_id, silent);
            }
            continue;
        }
        if let mc_bamboo_blocks::entity::EntityKind::Arrow(arrow) = &entity.kind {
            if !entity.removed
                && arrow.in_ground
                && arrow.shake_time == 0
                && region
                    .players
                    .get(&id)
                    .is_some_and(|player| !player.health.is_dead_or_dying())
            {
                pickup_arrow(region, id, entity_id, arrow, creative);
            }
            continue;
        }
        if !mc_bamboo_blocks::entity::can_pick_up(&entity, &owner) {
            continue;
        }
        let stack = entity
            .ground()
            .map(|item| item.stack.clone())
            .unwrap_or_else(ItemStack::empty);
        let original = stack.count;
        let picked_item = stack.item;
        let (left, changed, picked_any) = {
            let Some(player) = region.players.get_mut(&id) else {
                return;
            };
            let before = player.inventory.slots.clone();
            let left = player.inventory.receive(stack, creative);
            let changed = changed_slots(&before, &player.inventory);
            let picked_any = left.count < original || (creative && left.is_empty() && original > 0);
            (left, changed, picked_any)
        };
        if !picked_any {
            continue;
        }
        // `ServerPlayer.take` 的 THROWN_ITEM_PICKED_UP_BY_PLAYER：官方把
        // 掉落物的 owner（悦灵 / 玩家）作为第三个实体上下文。
        crate::advancement_triggers::record(
            region,
            id,
            "minecraft:thrown_item_picked_up_by_player",
            serde_json::json!({
                "item": crate::interaction::item_name_of(picked_item),
                "entity": {"type": "minecraft:player"},
            }),
        );
        if let Some(player) = region.players.get_mut(&id) {
            for slot in changed {
                inventory::send_slot(player, &mut region.links, slot);
            }
        }
        let shown = region
            .entities
            .with_mut(entity_id, |entity| {
                mc_bamboo_blocks::entity::take_stack(entity, left)
            })
            .unwrap_or(0);
        entity_tracking::broadcast_take(region, entity_id, id, shown);
        // `discard` 只打上标记。实体还留在这一拍的名单里，轮到它时不再 tick，然后才删掉。
        // `discard` only marks the entity. It stays on this tick's list, skips its tick, then goes.
    }
    // 官方 `Player.aiStep` 把经验球单独收集起来，最后只碰一颗：
    // `Util.getRandom(list, this.random)` 在名单非空时取一次 `nextInt(size)`。
    // 其它实体（掉落物）是逐个碰的，上面已经按实体分区顺序做完。
    //
    // Official `Player.aiStep` collects orbs separately and touches exactly one
    // at the end: `Util.getRandom(list, this.random)` draws one `nextInt(size)`
    // when the list is non-empty. Other entities were touched above in section
    // order.
    if !orbs.is_empty() {
        let Some(player) = region.players.get_mut(&id) else {
            return;
        };
        let chosen = orbs[player.random.next_int_bounded(orbs.len() as i32) as usize];
        crate::experience_pickup::pickup_orb(region, id, chosen);
    }
}

fn pickup_arrow(
    region: &mut Region,
    id: PlayerId,
    uid: i32,
    arrow: &mc_bamboo_blocks::entity::arrow::Arrow,
    creative: bool,
) {
    use mc_bamboo_blocks::entity::arrow::{ArrowVariant, Pickup};
    // `ThrownTrident.playerTouch`：有主人的三叉戟只有主人能碰；没有主人时谁都能碰。
    // `ThrownTrident.playerTouch`: only the owner can touch an owned trident;
    // a trident with no owner can be touched by anyone.
    if arrow.variant == ArrowVariant::Trident {
        let owned = region.players.get(&id).is_some_and(|player| {
            arrow
                .owner_uuid
                .is_none_or(|uuid| uuid == *player.uuid.as_bytes())
        });
        if !owned {
            return;
        }
    }
    match arrow.pickup {
        Pickup::Disallowed => return,
        Pickup::CreativeOnly if !creative => return,
        Pickup::CreativeOnly => {}
        Pickup::Allowed => {
            let Some(player) = region.players.get_mut(&id) else {
                return;
            };
            let before = player.inventory.slots.clone();
            let (_, added) = player
                .inventory
                .receive_with_add_result(arrow.pickup_item.clone(), creative);
            for slot in changed_slots(&before, &player.inventory) {
                inventory::send_slot(player, &mut region.links, slot);
            }
            if !added {
                return;
            }
        }
    }
    entity_tracking::broadcast_take(region, uid, id, 1);
    region
        .entities
        .with_mut(uid, |entity| entity.removed = true);
}

fn changed_slots(before: &[ItemStack], inventory: &Inventory) -> Vec<usize> {
    before
        .iter()
        .zip(inventory.slots.iter())
        .enumerate()
        .filter(|(_, (previous, current))| previous != current)
        .map(|(index, _)| index)
        .collect()
}

fn throw_position(player: &Player) -> Vec3 {
    let feet = player.position();
    // `getEyeY() - 0.3F`。0.3 是 float。
    // `getEyeY() - 0.3F`. The 0.3 is a float.
    Vec3::new(
        feet.x,
        feet.y + player.eye_height() - f64::from(0.3_f32),
        feet.z,
    )
}

fn throw_delta(player: &mut Player) -> Vec3 {
    let degrees = (std::f64::consts::PI / 180.0) as f32;
    let sin_x = mth::sin(f64::from(player.pitch * degrees));
    let cos_x = mth::cos(f64::from(player.pitch * degrees));
    let sin_y = mth::sin(f64::from(player.yaw * degrees));
    let cos_y = mth::cos(f64::from(player.yaw * degrees));
    let full_turn = (std::f64::consts::PI * 2.0) as f32;
    let direction = player.random.next_float() * full_turn;
    let spread = 0.02 * player.random.next_float();
    let y_jitter = (player.random.next_float() - player.random.next_float()) * 0.1;
    Vec3::new(
        f64::from(-sin_y * cos_x * 0.3) + f64::cos(f64::from(direction)) * f64::from(spread),
        f64::from(-sin_x * 0.3 + 0.1 + y_jitter),
        f64::from(cos_y * cos_x * 0.3) + f64::sin(f64::from(direction)) * f64::from(spread),
    )
}

/// 玩家随机源的种子，和实体一样由 ID 决定，测试才能重放。
///
/// The player's random seed is derived from the id, same as entities, so tests can replay it.
pub(crate) fn player_random(id: PlayerId) -> JavaRandom {
    JavaRandom::new(i64::from(id.0) ^ ENTITY_RANDOM_SEED)
}

/// 用真实玩家拾取流程重放一支箭；满背包时可预留一叠箭的空位。
///
/// Replay one arrow through the real pickup phase, optionally with a full inventory
/// and an existing arrow stack of the specified size.
#[cfg(any(test, feature = "oracle-samples"))]
pub fn replay_arrow_pickup(
    count: i32,
    pickup: mc_bamboo_blocks::entity::arrow::Pickup,
    grounded: bool,
    shake: u8,
    creative: bool,
    full_inventory: Option<i32>,
) -> (bool, i32, i32) {
    use mc_bamboo_blocks::entity::EntityKind;
    use mc_bamboo_data::items;
    let id = PlayerId(100);
    let at = Vec3::new(0.5, 64.0, 0.5);
    let mut region = Region::new(2, 20, false);
    let mut player = Player::new(
        id,
        uuid::Uuid::nil(),
        "ArrowTest".to_owned(),
        if creative {
            GameMode::Creative
        } else {
            GameMode::Survival
        },
        mc_bamboo_protocol::packet::play::ClientInformation::default(),
        at,
    );
    if let Some(arrows) = full_inventory {
        player.inventory.slots[9..45].fill(ItemStack::new(items::STONE, 64));
        if arrows > 0 {
            player.inventory.slots[inventory::HOTBAR_START] = ItemStack::new(items::ARROW, arrows);
        }
    }
    region.insert_player(player);
    let mut entity = Entity::arrow(at);
    let EntityKind::Arrow(arrow) = &mut entity.kind else {
        unreachable!()
    };
    arrow.pickup = pickup;
    arrow.in_ground = grounded;
    arrow.shake_time = shake;
    arrow.pickup_item = ItemStack::new(items::ARROW, count);
    let uid = region.entities.spawn(entity);
    pickup_before_entities(&mut region);
    let entity = region.entities.get(uid).unwrap();
    let EntityKind::Arrow(arrow) = &entity.kind else {
        unreachable!()
    };
    let arrows = region.players[&id]
        .inventory
        .slots
        .iter()
        .filter(|stack| stack.item == items::ARROW)
        .map(|stack| stack.count)
        .sum();
    (entity.removed, arrows, arrow.pickup_item.count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_blocks::entity::{Entity, EntityKind, arrow::Pickup};
    use mc_bamboo_protocol::packet::play::ClientInformation;
    use uuid::Uuid;

    #[test]
    fn partial_arrow_pickup_leaves_the_original_arrow_in_survival() {
        assert_eq!(
            replay_arrow_pickup(2, Pickup::Allowed, true, 0, false, Some(63)),
            (false, 64, 2),
        );
        assert_eq!(
            replay_arrow_pickup(2, Pickup::Allowed, true, 0, true, Some(63)),
            (true, 64, 2),
        );
    }

    /// 膨胀到最高一档的河豚：玩家碰到它时掉血并中毒 120 刻。测试区域的难度是简单，
    /// `MobAttack` 按难度缩放：3 点变成 `3 / 2 + 1 = 2.5`。
    ///
    /// A pufferfish at the top puff state: a touching player loses health and gets 120
    /// ticks of poison. The test region is on easy difficulty, so `MobAttack` scales:
    /// 3 becomes `3 / 2 + 1 = 2.5`.
    #[test]
    fn a_puffed_pufferfish_hurts_and_poisons_a_touching_player() {
        let id = PlayerId(101);
        let mut region = Region::new(2, 20, false);
        let at = Vec3::new(0.5, 64.0, 0.5);
        region.insert_player(Player::new(
            id,
            Uuid::nil(),
            "PufferTest".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            at,
        ));
        let mut fish = Entity::by_mob_type(
            mc_bamboo_blocks::entity::mob::MobType::Pufferfish,
            Vec3::new(0.9, 64.0, 0.5),
        );
        fish.with_mob(|_, mob| {
            mob.fish.puff_state = 2;
            mob.uuid = [7; 16];
        })
        .expect("a pufferfish is a mob");
        region.entities.spawn(fish);
        pickup_before_entities(&mut region);
        let player = &region.players[&id];
        assert_eq!(player.health.health, 17.5);
        assert!(
            player
                .effects
                .has(mc_bamboo_data::registries::mob_effect::POISON)
        );
    }

    #[test]
    fn a_deflated_pufferfish_does_not_hurt_a_touching_player() {
        let id = PlayerId(102);
        let mut region = Region::new(2, 20, false);
        let at = Vec3::new(0.5, 64.0, 0.5);
        region.insert_player(Player::new(
            id,
            Uuid::nil(),
            "PufferTest".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            at,
        ));
        let fish = Entity::by_mob_type(
            mc_bamboo_blocks::entity::mob::MobType::Pufferfish,
            Vec3::new(0.9, 64.0, 0.5),
        );
        region.entities.spawn(fish);
        pickup_before_entities(&mut region);
        let player = &region.players[&id];
        assert_eq!(player.health.health, 20.0);
        assert!(
            !player
                .effects
                .has(mc_bamboo_data::registries::mob_effect::POISON)
        );
    }

    #[test]
    fn embedded_arrow_pickup_checks_mode_and_shake() {
        let id = PlayerId(100);
        let mut region = Region::new(2, 20, false);
        let at = Vec3::new(0.5, 64.0, 0.5);
        region.insert_player(Player::new(
            id,
            Uuid::nil(),
            "ArrowTest".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            at,
        ));
        let embed = |pickup, shake_time, region: &mut Region| {
            let mut entity = Entity::arrow(at);
            let EntityKind::Arrow(arrow) = &mut entity.kind else {
                unreachable!()
            };
            arrow.pickup = pickup;
            arrow.in_ground = true;
            arrow.shake_time = shake_time;
            region.entities.spawn(entity)
        };
        let refused = embed(Pickup::Disallowed, 0, &mut region);
        let shaky = embed(Pickup::Allowed, 1, &mut region);
        let creative = embed(Pickup::CreativeOnly, 0, &mut region);
        let ready = embed(Pickup::Allowed, 0, &mut region);

        pickup_before_entities(&mut region);
        for stays in [refused, shaky, creative] {
            assert!(!region.entities.get(stays).unwrap().removed);
        }
        assert!(region.entities.get(ready).unwrap().removed);
        assert_eq!(
            region.players[&id]
                .inventory
                .slots
                .iter()
                .filter(|stack| stack.item == mc_bamboo_data::items::ARROW)
                .map(|stack| stack.count)
                .sum::<i32>(),
            1
        );

        region.players.get_mut(&id).unwrap().game_mode = GameMode::Creative;
        pickup_before_entities(&mut region);
        assert!(region.entities.get(creative).unwrap().removed);
        assert_eq!(
            region.players[&id]
                .inventory
                .slots
                .iter()
                .filter(|stack| stack.item == mc_bamboo_data::items::ARROW)
                .map(|stack| stack.count)
                .sum::<i32>(),
            1
        );
    }
}
