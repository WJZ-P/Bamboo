//! 玩家运行时状态 ⇄ `PlayerData` 存盘字段的转换（纯函数，不碰 IO）。
//!
//! 存盘走 `PlayerData::from_runtime_parts`（把菜单槽换算成官方 `Inventory.items`
//! 的下标）；读盘走这里的 [`restore_inventory`]，用 [`disk_to_menu`] 做反向换算。
//! 官方参照：`ServerPlayer.addAdditionalSaveData` / `Player.readAdditionalSaveData`。
//!
//! Conversion between a live `Player` and the persisted `PlayerData` fields.
//! Saving uses `PlayerData::from_runtime_parts` (menu slots -> official inventory
//! slots); loading uses [`restore_inventory`] with the inverse [`disk_to_menu`].

use mc_bamboo_protocol::packet::play::GameMode;
use mc_bamboo_storage::player_data::{PlayerData, RuntimeParts};

use crate::player::Player;

/// 从一名在线玩家造出要存档的 `PlayerData`。
pub fn runtime_player_data(player: &Player) -> PlayerData {
    let position = player.position();
    PlayerData::from_runtime_parts(&RuntimeParts {
        uuid: *player.uuid.as_bytes(),
        position: [position.x, position.y, position.z],
        rotation: [player.yaw, player.pitch],
        on_ground: player.on_ground,
        game_type: player.game_mode as i32,
        selected_slot: i32::from(player.inventory.selected),
        menu_slots: &player.inventory.slots,
        ender_slots: &player.ender_chest.slots,
    })
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

/// 把 `PlayerData` 里的物品栏和末影箱写回玩家。其它字段（血量、经验、能力等）
/// 目前没有对应的运行时状态，保存时用官方默认值；这里只恢复建模过的部分。
pub fn restore_inventory(player: &mut Player, data: &PlayerData) {
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
