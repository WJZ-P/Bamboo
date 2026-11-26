//! 玩家右键扔风弹：官方 `WindChargeItem.use`。
//!
//! `spawnProjectileFromRotation(..., 0, 1.5, 1)` 生成的 `WindCharge` 位于玩家的
//! x / z 和眼睛 y（不减 0.1），按视线射出；随后放投掷音（音高取世界随机源）、
//! 记使用统计、非创造模式扣一个，并按 `use_cooldown` 组件挂冷却。

use mc_bamboo_blocks::{
    World,
    entity::wind_charge::{WindChargeShot, WindChargeVariant},
};
use mc_bamboo_core::{Hand, Vec3};
use mc_bamboo_data::{items, sounds};
use mc_bamboo_protocol::packet::play::GameMode;

use crate::{
    inbound::PlayerId,
    player_weapon::{
        level_random_float, player_movement, player_position, player_rotation, shoot_from_rotation,
        sync_slots,
    },
    region::Region,
};

/// 手里是风弹就扔出去，返回真；否则什么都不做。
pub(crate) fn throw(region: &mut Region, id: PlayerId, hand: Hand) -> bool {
    let Some(player) = region.players.get(&id) else {
        return false;
    };
    let stack = player.inventory.held(hand).clone();
    if stack.item != items::WIND_CHARGE {
        return false;
    }
    // `ServerPlayerGameMode.useItem`：冷却中连投掷声都不发。
    if crate::ender_pearl::on_cooldown(player, &stack) {
        return false;
    }
    let (owner, owner_uuid) = (player.id.0, *player.uuid.as_bytes());
    let pitch = 0.4 / (level_random_float(region) * 0.4 + 0.8);
    let position = player_position(region, id);
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        if let Some(position) = position {
            world.play_sound(
                position,
                sounds::sound_event("minecraft:entity.wind_charge.throw"),
                sounds::NEUTRAL,
                0.5,
                pitch,
                false,
            );
        }
        world.drain_effects()
    };
    crate::interaction::send_effects(region, None, &effects);
    let Some(base) = player_position(region, id) else {
        return false;
    };
    let eye_y = region
        .players
        .get(&id)
        .map(|player| base.y + f64::from(player.eye_height() as f32))
        .unwrap_or(base.y);
    let (yaw, x_rot) = player_rotation(region, id);
    let mut direction = shoot_from_rotation(yaw, x_rot, 0.0);
    // `shootFromRotation` 最后把射手已知位移加进速度；`WindChargeItem.shoot`
    // 是空实现，但 `Projectile.shootFromRotation` 仍会叠加。
    let movement = player_movement(region, id).unwrap_or_default();
    direction = direction.add(Vec3::new(movement.x, 0.0, movement.z));
    let shot = WindChargeShot {
        position: Vec3::new(base.x, eye_y, base.z),
        direction,
        power: 1.5,
        uncertainty: 1.0,
        variant: WindChargeVariant::Player,
        owner: Some(owner),
        owner_uuid: Some(owner_uuid),
    };
    {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        world.spawn_wind_charge_shot(&shot);
    }
    crate::item_use_stats::award(region, id, stack.item);
    consume_one(region, id, hand);
    if let Some(player) = region.players.get_mut(&id) {
        crate::ender_pearl::apply_cooldown(player, &stack);
    }
    true
}

/// `ItemStack.consume(1, player)`：有无限材料（创造模式）时不扣。
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
