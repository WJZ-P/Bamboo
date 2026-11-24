//! 玩家右键扔鸡蛋、雪球或末影珍珠：官方 `EggItem.use` / `SnowballItem.use` /
//! `EnderpearlItem.use`。先用世界随机源取投掷声的音高，再在眼睛下方 0.1 处生成
//! `ThrowableItemProjectile`，按视线以力度 1.5、散布 1 射出，记使用统计，最后
//! 非创造模式扣一个。珍珠另外有 20 刻 `use_cooldown`，并注册到玩家的珍珠集合。
//!
//! A player throwing an egg, snowball or ender pearl, vanilla `EggItem.use` /
//! `SnowballItem.use` / `EnderpearlItem.use`: the throw sound's pitch comes from
//! the level random first, then a `ThrowableItemProjectile` spawns 0.1 below the
//! eyes and is shot along the view with power 1.5 and uncertainty 1. Award the
//! use stat, then consume one unless the player has infinite materials. Ender
//! pearls also get a 20-tick `use_cooldown` and are registered on the player.

use mc_bamboo_blocks::{
    World,
    entity::thrown_egg::{EggShot, ThrowableKind},
};
use mc_bamboo_core::Hand;
use mc_bamboo_data::{items, sounds};
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
    let (projectile, sound, neutral_source) = match stack.item {
        items::EGG | items::BLUE_EGG | items::BROWN_EGG => {
            (ThrowableKind::Egg, "minecraft:entity.egg.throw", false)
        }
        items::SNOWBALL => (
            ThrowableKind::Snowball,
            "minecraft:entity.snowball.throw",
            false,
        ),
        items::ENDER_PEARL => (
            ThrowableKind::EnderPearl,
            "minecraft:entity.ender_pearl.throw",
            true,
        ),
        _ => return false,
    };
    // `ServerPlayerGameMode.useItem` 开头先问 `isOnCooldown(stack)`；在冷却中直接
    // `PASS`，连投掷声都不发。珍珠的 `use_cooldown` 组件是 1 秒（20 刻）。
    if projectile == ThrowableKind::EnderPearl && crate::ender_pearl::on_cooldown(player, &stack) {
        return false;
    }
    let item = stack.item;
    let (owner, owner_uuid) = (player.id.0, *player.uuid.as_bytes());
    let pitch = 0.4 / (level_random_float(region) * 0.4 + 0.8);
    if neutral_source {
        // `EnderpearlItem.use` 用 NEUTRAL 音源，鸡蛋 / 雪球用 PLAYERS。
        let position = player_position(region, id);
        let effects = {
            let (mut level, ticks) = region.block_level();
            let mut world = World::new(&mut level, ticks);
            if let Some(position) = position {
                world.play_sound(
                    position,
                    sounds::sound_event(sound),
                    sounds::NEUTRAL,
                    0.5,
                    pitch,
                    false,
                );
            }
            world.drain_effects()
        };
        crate::interaction::send_effects(region, None, &effects);
    } else {
        play_sound(region, player_position(region, id), sound, 0.5, pitch);
    }
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
        item: stack.clone(),
        projectile,
    };
    let pearl_id = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        world.spawn_egg_shot(&shot)
    };
    crate::item_use_stats::award(region, id, item);
    consume_one(region, id, hand);
    // `ItemStack.use` 成功后 `UseCooldown.apply` 挂冷却；`setOwner` 时注册到玩家。
    if let Some(player) = region.players.get_mut(&id) {
        crate::ender_pearl::apply_cooldown(player, &stack);
    }
    if projectile == ThrowableKind::EnderPearl {
        crate::ender_pearl::register(region, id, pearl_id);
    }
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

/// 一次扔蛋重放的结果：蛋的位置 / 速度 / 朝向原始位、随机源、蛋里的物品，以及手里剩下的
/// 数量和世界随机源，再加本次发出的使用统计。
///
/// One throw replay: the egg's raw position / velocity / angle bits, its random, the
/// item inside, the count left in hand, the level random and the use stat.
#[cfg(feature = "oracle-samples")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayEggThrow {
    pub motion: [i64; 8],
    pub seed: i64,
    pub item: [i32; 2],
    pub left: i32,
    pub level_seed: i64,
    pub item_used: Vec<(i32, i32)>,
    /// 扔完之后物品是不是进了冷却。鸡蛋没有 `use_cooldown`，恒假。
    ///
    /// Whether the stack is on cooldown after the throw. Eggs have no
    /// `use_cooldown`, so this is always false for them.
    pub cooldown: bool,
}

/// 用游戏里同一个 [`throw`] 入口重放一次扔末影珍珠，连冷却一起回报。
///
/// Replays throwing an ender pearl through the same [`throw`] entry and reports the
/// cooldown it leaves behind.
#[cfg(feature = "oracle-samples")]
pub fn replay_pearl_throw(
    yaw: f32,
    pitch: f32,
    movement: Option<mc_bamboo_core::Vec3>,
    creative: bool,
) -> Option<ReplayEggThrow> {
    use mc_bamboo_blocks::entity::EntityKind;
    use mc_bamboo_protocol::{item::ItemStack, packet::play::ClientInformation};

    let mut region = Region::new(2, 20, false);
    region.random = mc_bamboo_blocks::LevelRandom::new(1);
    let id = PlayerId(1 << 24);
    let mode = if creative {
        GameMode::Creative
    } else {
        GameMode::Survival
    };
    let mut player = crate::player::Player::new(
        id,
        uuid::Uuid::from_u128(1),
        "PearlOracle0".to_owned(),
        mode,
        ClientInformation::default(),
        mc_bamboo_core::Vec3::new(0.5, 64.0, 0.5),
    );
    player.yaw = yaw;
    player.head_yaw = yaw;
    player.pitch = pitch;
    player.motion.known_movement = movement.unwrap_or_default();
    let main = player.inventory.hand_slot(Hand::Main);
    player.inventory.slots[main] = ItemStack::new(items::ENDER_PEARL, 3);
    region.insert_player(player);
    if !throw(&mut region, id, Hand::Main) {
        return None;
    }
    let uid = region.entities.ids().into_iter().next()?;
    let entity = region.entities.get(uid)?;
    let EntityKind::Egg(egg) = &entity.kind else {
        return None;
    };
    let (p, v) = (entity.position, entity.delta);
    let player = region.players.get_mut(&id)?;
    let on_cooldown = player.cooldowns.is_on_cooldown(items::ENDER_PEARL, None);
    Some(ReplayEggThrow {
        motion: [
            p.x.to_bits() as i64,
            p.y.to_bits() as i64,
            p.z.to_bits() as i64,
            v.x.to_bits() as i64,
            v.y.to_bits() as i64,
            v.z.to_bits() as i64,
            i64::from(entity.y_rot.to_bits() as i32),
            i64::from(egg.x_rot.to_bits() as i32),
        ],
        seed: entity.random.state(),
        item: [egg.item.item, egg.item.count],
        left: player.inventory.slots[main].count,
        level_seed: region.random.source().seed(),
        item_used: player.item_use_stats.take_dirty(),
        cooldown: on_cooldown,
    })
}

/// 用游戏里同一个 [`throw`] 入口重放一次扔棕色鸡蛋。
///
/// Replays throwing a brown egg through the same [`throw`] entry the game uses.
#[cfg(feature = "oracle-samples")]
pub fn replay_throw(
    yaw: f32,
    pitch: f32,
    movement: Option<mc_bamboo_core::Vec3>,
    creative: bool,
) -> Option<ReplayEggThrow> {
    use mc_bamboo_blocks::entity::EntityKind;
    use mc_bamboo_protocol::{item::ItemStack, packet::play::ClientInformation};

    let mut region = Region::new(2, 20, false);
    region.random = mc_bamboo_blocks::LevelRandom::new(1);
    let id = PlayerId(1 << 24);
    let mode = if creative {
        GameMode::Creative
    } else {
        GameMode::Survival
    };
    let mut player = crate::player::Player::new(
        id,
        uuid::Uuid::from_u128(1),
        "Oracle0".to_owned(),
        mode,
        ClientInformation::default(),
        mc_bamboo_core::Vec3::new(0.5, 64.0, 0.5),
    );
    player.yaw = yaw;
    player.head_yaw = yaw;
    player.pitch = pitch;
    player.motion.known_movement = movement.unwrap_or_default();
    let main = player.inventory.hand_slot(Hand::Main);
    player.inventory.slots[main] = ItemStack::new(items::BROWN_EGG, 3);
    region.insert_player(player);
    if !throw(&mut region, id, Hand::Main) {
        return None;
    }
    let uid = region.entities.ids().into_iter().next()?;
    let entity = region.entities.get(uid)?;
    let EntityKind::Egg(egg) = &entity.kind else {
        return None;
    };
    let (p, v) = (entity.position, entity.delta);
    let player = region.players.get_mut(&id)?;
    Some(ReplayEggThrow {
        motion: [
            p.x.to_bits() as i64,
            p.y.to_bits() as i64,
            p.z.to_bits() as i64,
            v.x.to_bits() as i64,
            v.y.to_bits() as i64,
            v.z.to_bits() as i64,
            i64::from(entity.y_rot.to_bits() as i32),
            i64::from(egg.x_rot.to_bits() as i32),
        ],
        seed: entity.random.state(),
        item: [egg.item.item, egg.item.count],
        left: player.inventory.slots[main].count,
        level_seed: region.random.source().seed(),
        item_used: player.item_use_stats.take_dirty(),
        cooldown: false,
    })
}
