//! 玩家右键扔鸡蛋：官方 `EggItem.use`。先用世界随机源取扔蛋声的音高，再在眼睛下方
//! 0.1 处生成 `ThrownEgg`，按视线以力度 1.5、散布 1 射出，最后非创造模式扣一个。
//!
//! A player throwing an egg, vanilla `EggItem.use`: the throw sound's pitch comes
//! from the level random first, then a `ThrownEgg` spawns 0.1 below the eyes and is
//! shot along the view with power 1.5 and uncertainty 1; survival consumes one.

use mc_bamboo_blocks::{World, entity::thrown_egg::EggShot};
use mc_bamboo_core::Hand;
use mc_bamboo_data::items;
use mc_bamboo_protocol::packet::play::GameMode;

use crate::player_weapon::{
    level_random_float, play_sound, player_eye, player_movement, player_position, player_rotation,
    shoot_from_rotation, sync_slots,
};
use crate::{inbound::PlayerId, region::Region};

/// 手里是三种蛋之一就扔出去，返回真；否则什么都不做。
///
/// Throws when the hand holds one of the three eggs and returns true; otherwise
/// does nothing.
pub(crate) fn throw(region: &mut Region, id: PlayerId, hand: Hand) -> bool {
    let Some(player) = region.players.get(&id) else {
        return false;
    };
    let stack = player.inventory.held(hand).clone();
    if !matches!(stack.item, items::EGG | items::BLUE_EGG | items::BROWN_EGG) {
        return false;
    }
    let (owner, owner_uuid) = (player.id.0, *player.uuid.as_bytes());
    let pitch = 0.4 / (level_random_float(region) * 0.4 + 0.8);
    play_sound(
        region,
        player_position(region, id),
        "minecraft:entity.egg.throw",
        0.5,
        pitch,
    );
    let Some(position) = player_eye(region, id) else {
        return false;
    };
    let (yaw, x_rot) = player_rotation(region, id);
    let shot = EggShot {
        position,
        direction: shoot_from_rotation(yaw, x_rot, 0.0),
        power: 1.5,
        uncertainty: 1.0,
        source_movement: player_movement(region, id),
        owner: Some(owner),
        owner_uuid: Some(owner_uuid),
        item: stack,
    };
    {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        world.spawn_egg_shot(&shot);
    }
    consume_one(region, id, hand);
    true
}

/// `itemStack.consume(1, player)`：有无限材料（创造模式）时不扣。
///
/// `itemStack.consume(1, player)`: nothing is taken with infinite materials.
fn consume_one(region: &mut Region, id: PlayerId, hand: Hand) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    if player.game_mode == GameMode::Creative {
        return;
    }
    let slot = player.inventory.hand_slot(hand);
    let stack = &mut player.inventory.slots[slot];
    stack.count -= 1;
    if stack.count <= 0 {
        *stack = mc_bamboo_protocol::item::ItemStack::empty();
    }
    sync_slots(region, id, [slot]);
}
