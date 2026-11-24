//! 玩家运行时状态 ⇄ `PlayerData` 存盘字段的转换（纯函数，不碰 IO）。
//!
//! 存盘走 `PlayerData::from_runtime_parts`（把菜单槽换算成官方 `Inventory.items`
//! 的下标）；读盘走这里的 [`restore_inventory`]，用 [`disk_to_menu`] 做反向换算。
//! 官方参照：`ServerPlayer.addAdditionalSaveData` / `Player.readAdditionalSaveData`。
//!
//! Conversion between a live `Player` and the persisted `PlayerData` fields.
//! Saving uses `PlayerData::from_runtime_parts` (menu slots -> official inventory
//! slots); loading uses [`restore_inventory`] with the inverse [`disk_to_menu`].
//!
//! 经验三字段和 `XpSeed` 也走同一对函数：存档里 `XpSeed == 0` 表示要从玩家随机源
//! 抽一个新的附魔种子，见 [`restore_inventory`]。
//!
//! The three experience fields and `XpSeed` take the same route. A saved
//! `XpSeed == 0` means a new enchantment seed is drawn from the player's
//! random source, see [`restore_inventory`].

use mc_bamboo_protocol::packet::play::GameMode;
use mc_bamboo_storage::player_data::{PlayerData, RuntimeParts};

use crate::{experience::Experience, player::Player};

/// 从一名在线玩家造出要存档的 `PlayerData`。
pub fn runtime_player_data(player: &Player) -> PlayerData {
    let position = player.position();
    let mut data = PlayerData::from_runtime_parts(&RuntimeParts {
        uuid: *player.uuid.as_bytes(),
        position: [position.x, position.y, position.z],
        rotation: [player.yaw, player.pitch],
        on_ground: player.on_ground,
        game_type: player.game_mode as i32,
        selected_slot: i32::from(player.inventory.selected),
        menu_slots: &player.inventory.slots,
        ender_slots: &player.ender_chest.slots,
        xp_p: player.experience.progress,
        xp_level: player.experience.level,
        xp_total: player.experience.total,
        xp_seed: player.enchantment_seed,
        score: player.score,
    });
    player.health.save(&mut data);
    player.environment.save(&mut data);
    // `LivingEntity.addAdditionalSaveData`：空表不写 `active_effects`。
    if !player.effects.is_empty() {
        data.active_effects = Some(mc_bamboo_blocks::entity::mob::nbt::effects_tag(
            &player.effects,
        ));
    }
    // `Player.addAdditionalSaveData`：只有 Optional 非空才写 `LastDeathLocation`。
    if let Some(pos) = player.last_death_location {
        data.last_death_location = Some((
            mc_bamboo_storage::player_data::OVERWORLD.to_owned(),
            [pos.x, pos.y, pos.z],
        ));
    }
    // `ServerPlayer.saveEnderPearls`：空集合不写 `ender_pearls` 键。
    data.ender_pearls = player.saved_ender_pearls.clone();
    // `dead` 是运行时标志（官方 `dead` 不存档），重生后由 health 字段推：
    // 官方读档时 Health<=0 会立刻进入死亡状态，这里按官方存档字段保留。
    data
}

/// 官方 `GameType.LEGACY_ID_CODEC`：0 生存、1 创造、2 冒险、3 旁观，其余折回生存。
pub fn game_mode_from_id(id: i32) -> GameMode {
    match id {
        1 => GameMode::Creative,
        2 => GameMode::Adventure,
        3 => GameMode::Spectator,
        _ => GameMode::Survival,
    }
}

/// 磁盘槽位 → Bamboo 菜单槽位，和 `PlayerData::menu_to_disk` 互逆。
/// 0..8 快捷栏、9..35 背包、36..39 盔甲（逆序）、40 副手。
fn disk_to_menu(disk: i32) -> Option<usize> {
    match disk {
        0..=8 => Some(36 + disk as usize),
        9..=35 => Some(disk as usize),
        36..=39 => Some(44 - disk as usize),
        40 => Some(45),
        _ => None,
    }
}

/// 官方 `Player.readAdditionalSaveData` 里 `XpSeed` 的那一段：
/// `enchantmentSeed = input.getIntOr("XpSeed", 0)`；只有 0（NO_ENCHANTMENT_SEED）
/// 才 `this.random.nextInt()` 重抽。没有存档的新玩家同样走「0 → 抽一次」。
/// 分开成一个函数，是因为 `Player` 构造函数自己不取随机数。
///
/// The `XpSeed` part of official `Player.readAdditionalSaveData`:
/// `enchantmentSeed = input.getIntOr("XpSeed", 0)`, and only a zero value
/// (NO_ENCHANTMENT_SEED) draws `this.random.nextInt()`. A new player with no
/// save takes the same "zero → draw once" path. This is separate because the
/// `Player` constructor itself draws nothing.
pub fn restore_experience_seed(player: &mut Player, data: Option<&PlayerData>) {
    let saved = data.map_or(0, |data| data.xp_seed);
    player.enchantment_seed = if saved == 0 {
        player.random.next_int()
    } else {
        saved
    };
}

/// 把 `PlayerData` 里的物品栏、末影箱、经验、生命和饥饿写回玩家。
/// 能力还没有对应的运行时状态，保存时用官方默认值。
/// `XpSeed` 由 [`restore_experience_seed`] 单独处理，因为要不要取随机数取决于它。
///
/// Restores inventory, ender chest, experience, health and hunger from
/// `PlayerData`. Abilities still keep the official defaults.
/// [`restore_experience_seed`] handles `XpSeed` because it decides whether a
/// random number is drawn.
pub fn restore_inventory(player: &mut Player, data: &PlayerData) {
    player.health.restore(data);
    player.environment.restore(data);
    // `LivingEntity.readAdditionalSaveData`：读盘不触发 `onEffectAdded` 的包，
    // 属性修饰和最大血量由玩家效果层在下一刻重建。
    player.effects = data
        .active_effects
        .as_ref()
        .map(mc_bamboo_blocks::entity::mob::nbt::read_effects_tag)
        .unwrap_or_default();
    player.experience = Experience {
        level: data.xp_level,
        total: data.xp_total,
        progress: data.xp_p,
    };
    player.score = data.score;
    player.last_death_location = data
        .last_death_location
        .as_ref()
        .filter(|(dimension, _)| dimension == mc_bamboo_storage::player_data::OVERWORLD)
        .map(|(_, pos)| mc_bamboo_core::BlockPos::new(pos[0], pos[1], pos[2]));
    player.dead = player.health.health <= 0.0;
    player.awaiting_respawn = player.dead;
    player.yaw = data.rotation[0];
    player.pitch = data.rotation[1];
    player.head_yaw = data.rotation[0];
    player.on_ground = data.on_ground;
    player.inventory.selected = data.selected_item_slot.clamp(0, 8) as u8;
    for item in &data.inventory {
        if let Some(menu) = disk_to_menu(item.slot)
            && let Some(slot) = player.inventory.slots.get_mut(menu)
        {
            *slot = item.stack.clone();
        }
    }
    for item in &data.ender_items {
        if item.slot >= 0
            && let Some(slot) = player.ender_chest.slots.get_mut(item.slot as usize)
        {
            *slot = item.stack.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_and_disk_slot_mappings_are_inverses() {
        // `PlayerData::menu_to_disk` 的合法菜单槽（跳过合成格 0..4）。
        for menu in [5, 6, 7, 8, 9, 20, 35, 36, 40, 44, 45] {
            // 复刻 from_runtime_parts 里的换算，验证往返一致。
            let disk = match menu {
                5..=8 => 36 + (8 - menu),
                9..=35 => menu,
                36..=44 => menu - 36,
                45 => 40,
                _ => unreachable!(),
            } as i32;
            if let Some(back) = disk_to_menu(disk) {
                assert_eq!(back, menu, "slot {disk}");
            }
        }
    }
}
