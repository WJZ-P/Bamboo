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
use mc_bamboo_protocol::{item::ItemStack, packet::play::GameMode};

use crate::{
    entity_tracking,
    inbound::PlayerId,
    inventory::{self, Inventory},
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
    if let Some(entity) = world.entity_mut(entity_id) {
        mc_bamboo_blocks::entity::set_throw_velocity(entity, delta);
    }
}

/// 实体 tick 之前，玩家碰得到的物品先捡起来。
///
/// Items a player is touching are picked up before entity ticks.
pub(crate) fn pickup_before_entities(region: &mut Region) {
    let players: Vec<PlayerId> = region.players.keys().copied().collect();
    for id in players {
        pickup_for(region, id);
    }
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
    let ids: Vec<i32> = region
        .entities
        .iter()
        .filter(|entity| entity.bounding_box().intersects(reach))
        .map(|entity| entity.id)
        .collect();
    for entity_id in ids {
        let Some(entity) = region.entities.get(entity_id) else {
            continue;
        };
        if !mc_bamboo_blocks::entity::can_pick_up(entity, &owner) {
            continue;
        }
        let stack = entity
            .ground()
            .map(|item| item.stack.clone())
            .unwrap_or_else(ItemStack::empty);
        let original = stack.count;
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
        if let Some(player) = region.players.get_mut(&id) {
            for slot in changed {
                inventory::send_slot(player, &mut region.links, slot);
            }
        }
        let shown = {
            let Some(entity) = region.entities.get_mut(entity_id) else {
                continue;
            };
            mc_bamboo_blocks::entity::take_stack(entity, left)
        };
        entity_tracking::broadcast_take(region, entity_id, id, shown);
        // `discard` 只打上标记。实体还留在这一拍的名单里，轮到它时不再 tick，然后才删掉。
        // `discard` only marks the entity. It stays on this tick's list, skips its tick, then goes.
    }
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
