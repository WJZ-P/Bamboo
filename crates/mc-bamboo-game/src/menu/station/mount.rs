//! 坐骑物品栏：官方 `ServerPlayer.openHorseInventory` 和 `HorseInventoryMenu` /
//! `AbstractMountInventoryMenu`。第 0 格鞍、第 1 格身体（都是 `ArmorSlot`），然后是
//! 三行箱子格，最后玩家的 36 格。点击走工作台菜单那一套 `doClick`；每次点击前从生物
//! 读回这几格，点击后把 `ArmorSlot.setByPlayer` 按发生顺序写回生物（会出装备声、取
//! 生物随机数），箱子格整份写回。
//!
//! Mount inventories: vanilla `ServerPlayer.openHorseInventory` with
//! `HorseInventoryMenu` / `AbstractMountInventoryMenu`. Slot 0 is the saddle and
//! slot 1 the body (both `ArmorSlot`s), then three rows of chest, then the player's
//! 36 slots. Clicks run through the workstation `doClick`; before each click the
//! slots are read back from the mob, and afterwards every `ArmorSlot.setByPlayer`
//! is replayed on the mob in order (equip sounds and mob random draws included),
//! while the chest slots are written back whole.

use mc_bamboo_blocks::entity::mob::{MobType, equine::inventory};
use mc_bamboo_core::Vec3;
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{EquipmentSlot, GameMode, MountScreenOpen},
};

use super::{Kind, Menu, MoveResult, Role, Station, send_outgoing};
use crate::{container::ContainerKind, inbound::PlayerId, menu::Target, region::Region};

/// `isWithinEntityInteractionRange(mount, 4.0)` 的额外距离。
///
/// The extra reach in `isWithinEntityInteractionRange(mount, 4.0)`.
const MOUNT_REACH_BUFFER: f64 = 4.0;

/// 菜单记住的那头坐骑。
///
/// The mount a menu points at.
pub(super) struct Mount {
    entity: i32,
    mob_type: MobType,
    /// 鞍位的 `canUseSlot`，每次读回时刷新。
    ///
    /// `canUseSlot` for the saddle, refreshed on every read.
    saddle_usable: bool,
    /// 打开时有没有箱子。`hasInventoryChanged`：装上箱子会换掉整个物品栏。
    ///
    /// Whether a chest was fitted at open time. `hasInventoryChanged`: fitting a
    /// chest replaces the whole inventory.
    chested: bool,
    /// 这次点击里 `ArmorSlot.setByPlayer` 的（格、之前、之后），按发生顺序。
    ///
    /// `ArmorSlot.setByPlayer` calls of this click as (slot, previous, new), in order.
    equips: Vec<(EquipmentSlot, ItemStack, ItemStack)>,
}

impl Station {
    /// `ArmorSlot.mayPlace`。
    pub(super) fn mount_accepts(&self, index: usize, stack: &ItemStack) -> bool {
        let Some(mount) = &self.mount else {
            return false;
        };
        let slot = if index == 0 {
            EquipmentSlot::Saddle
        } else {
            EquipmentSlot::Body
        };
        if mc_bamboo_blocks::entity::mob::nautilus::is_nautilus(mount.mob_type) {
            // `NautilusInventoryMenu` 的两格也要按驯服 / 成年判 `canUseSlot`：
            // 菜单里的 `saddle_usable` 每次 `pull` 都从生物刷新。
            // The two `NautilusInventoryMenu` slots also check `canUseSlot`; the
            // menu's `saddle_usable` is refreshed from the mob on every pull.
            return mc_bamboo_blocks::entity::mob::nautilus::inventory_accepts_with_flag(
                mount.mob_type,
                mount.saddle_usable,
                slot,
                stack,
            );
        }
        inventory::accepts(mount.mob_type, mount.saddle_usable, slot, stack)
    }

    /// 鞍位或身体位被 `setByPlayer` 了：记下来，点击结束后写回生物。
    ///
    /// The saddle or body slot went through `setByPlayer`: remember it for the
    /// write-back after the click.
    pub(super) fn note_mount_equip(&mut self, index: usize, previous: ItemStack) {
        let slot = match self.role(index) {
            Role::MountSaddle => EquipmentSlot::Saddle,
            Role::MountBody => EquipmentSlot::Body,
            _ => return,
        };
        let current = self.slots[index].clone();
        if let Some(mount) = &mut self.mount {
            mount.equips.push((slot, previous, current));
        }
    }

    /// `AbstractMountInventoryMenu.quickMoveStack`。
    ///
    /// 玩家格之间互挪那一支官方最后总是 `return EMPTY`，不清理来源格，来源格的数量
    /// 直接在原物品上改。
    ///
    /// The player-to-player branch always ends in `return EMPTY` without the slot
    /// cleanup; the source count changes on the stack itself.
    pub(super) fn route_mount(&mut self, index: usize, stack: &mut ItemStack) -> MoveResult {
        let player_start = self.kind.player_start();
        let end = self.slots.len();
        if index < player_start {
            return Self::moved_or_stop(self.move_items(stack, player_start, end, true), false);
        }
        if self.may_place(1, stack) && self.slots[1].is_empty() {
            return Self::moved_or_stop(self.move_items(stack, 1, 2, false), false);
        }
        if self.may_place(0, stack) && self.slots[0].is_empty() {
            return Self::moved_or_stop(self.move_items(stack, 0, 1, false), false);
        }
        if player_start > 2 && self.move_items(stack, 2, player_start, false) {
            return MoveResult::Moved { crafted: false };
        }
        let inventory_end = player_start + 27;
        if (inventory_end..end).contains(&index) {
            self.move_items(stack, player_start, inventory_end, false);
        } else if (player_start..inventory_end).contains(&index) {
            self.move_items(stack, inventory_end, end, false);
        }
        MoveResult::Stop { write_back: true }
    }
}

/// `AbstractHorse.openCustomInventoryScreen` → `ServerPlayer.openHorseInventory`：
/// 关掉开着的菜单、容器编号加一、发 `mount_screen_open`，再 `initMenu` 发全部内容。
///
/// `openCustomInventoryScreen` → `ServerPlayer.openHorseInventory`: close any open
/// menu, bump the container counter, send `mount_screen_open`, then `initMenu`
/// sends the full contents.
pub(crate) fn open(region: &mut Region, id: PlayerId, entity: i32) -> bool {
    let Some(public) = region.entities.public_at(entity) else {
        return false;
    };
    let pos = public.block_position();
    let Some((columns, mob_type, chested)) = region
        .entities
        .with_mut(entity, |row| {
            row.with_mob(|_, mob| {
                let chested = mob.equine.as_ref().is_some_and(|state| state.chested);
                (inventory::columns(mob), mob.mob_type, chested)
            })
        })
        .flatten()
    else {
        return false;
    };
    let Ok(columns) = u8::try_from(columns) else {
        return false;
    };
    crate::lectern::close_for_other_menu(region, id);
    if region.menus.contains_key(&id) {
        crate::menu::close_menu(region, id);
    }
    let Some((container_id, creative, selected, inv)) = region.players.get_mut(&id).map(|player| {
        player.container_counter = player.container_counter % 100 + 1;
        (
            player.container_counter,
            player.game_mode == GameMode::Creative,
            player.inventory.selected,
            player.inventory.slots.clone(),
        )
    }) else {
        return false;
    };
    region.links.send(
        id,
        &MountScreenOpen {
            container_id,
            inventory_columns: i32::from(columns),
            entity_id: entity,
        },
    );
    let mut station = Station::create(Kind::Mount(columns), container_id, pos);
    station.creative = creative;
    station.in_world = true;
    station.mount = Some(Mount {
        entity,
        mob_type,
        saddle_usable: false,
        chested,
        equips: Vec::new(),
    });
    station.load_player(&inv, selected);
    pull(region, &mut station);
    station.send_all();
    let packets = std::mem::take(&mut station.packets);
    send_outgoing(region, id, container_id, packets);
    let mut menu = Menu::new(container_id, ContainerKind::Chest, Target::Single(pos), 0);
    menu.station = Some(station);
    region.menus.insert(id, menu);
    true
}

/// 从生物读回鞍、身体和箱子格。
///
/// Read the saddle, body and chest slots back from the mob.
pub(super) fn pull(region: &mut Region, station: &mut Station) {
    let Some(entity) = station.mount.as_ref().map(|mount| mount.entity) else {
        return;
    };
    let read = region.entities.with_mut(entity, |row| {
        row.with_mob(|_, mob| {
            let chest = mob
                .equine
                .as_ref()
                .map(|state| state.chest_items.clone())
                .unwrap_or_default();
            (
                mob.equipment.get(EquipmentSlot::Saddle).clone(),
                mob.equipment.get(EquipmentSlot::Body).clone(),
                chest,
                inventory::saddle_usable(mob),
            )
        })
    });
    let Some((saddle, body, chest, saddle_usable)) = read.flatten() else {
        return;
    };
    station.slots[0] = saddle;
    station.slots[1] = body;
    let chest_slots = station.kind.player_start() - 2;
    if chest.len() == chest_slots {
        station.slots[2..2 + chest_slots].clone_from_slice(&chest);
    }
    if let Some(mount) = &mut station.mount {
        mount.saddle_usable = saddle_usable;
        mount.equips.clear();
    }
}

/// 点击之后写回生物：鞍位和身体位按 `setByPlayer` 的顺序重放，箱子格整份覆盖。
///
/// Write back after a click: replay saddle and body `setByPlayer` calls in order,
/// then overwrite the chest slots.
pub(super) fn flush(region: &mut Region, station: &mut Station) {
    let Some(mount) = &mut station.mount else {
        return;
    };
    let equips = std::mem::take(&mut mount.equips);
    let entity = mount.entity;
    let chest_slots = station.kind.player_start() - 2;
    let chest = station.slots[2..2 + chest_slots].to_vec();
    region.entities.with_mut(entity, |row| {
        row.with_mob(|me, mob| {
            for (slot, previous, stack) in &equips {
                inventory::set_by_player(me, mob, *slot, previous, stack);
            }
            if chest_slots > 0 {
                inventory::set_chest_items(mob, &chest);
            }
        })
    });
}

/// `AbstractMountInventoryMenu.stillValid`：物品栏没被换掉、坐骑活着、玩家眼睛到
/// 坐骑碰撞箱的距离小于实体交互距离加 4。
///
/// `AbstractMountInventoryMenu.stillValid`: the inventory was not replaced, the
/// mount is alive, and the player's eyes are within entity interaction range plus
/// four of its bounding box.
pub(super) fn still_valid(station: &Station, region: &Region, id: PlayerId) -> bool {
    let Some(mount) = &station.mount else {
        return false;
    };
    let Some(player) = region.players.get(&id) else {
        return false;
    };
    let Some(public) = region.entities.public_at(mount.entity) else {
        return false;
    };
    let mc_bamboo_blocks::entity::KindPublic::Mob(mob) = &public.kind else {
        return false;
    };
    let chested = mob.equine.as_ref().is_some_and(|state| state.chested);
    if public.removed || mob.health <= 0.0 || chested != mount.chested {
        return false;
    }
    let eye = Vec3::new(
        player.position().x,
        player.position().y + player.eye_height(),
        player.position().z,
    );
    let range = player
        .attributes
        .value(mc_bamboo_data::registries::attribute::ENTITY_INTERACTION_RANGE)
        + MOUNT_REACH_BUFFER;
    public.bounding_box().distance_to_sqr(eye) < range * range
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use mc_bamboo_blocks::entity::{Entity, mob::MobEvent};
    use mc_bamboo_core::{Hand, Vec3};
    use mc_bamboo_data::items;
    use mc_bamboo_protocol::packet::play::{ContainerInput, HashedStack};

    use super::*;
    use crate::{
        menu::{ClickPacket, on_click},
        world::tests::{Client, Harness, SPAWN_Y, frames_with_id},
    };

    /// 一头带箱子、带鞍的驴，玩家站在旁边。
    ///
    /// A chested, saddled donkey next to the player.
    fn stage(tame: bool) -> (Harness, Client, i32) {
        let mut server = Harness::new(2);
        let client = server.join("Rider");
        server.tick();
        let _ = client.take_frames();
        let mut donkey = Entity::by_mob_type(MobType::Donkey, Vec3::new(9.5, SPAWN_Y, 8.5));
        donkey.with_mob(|_, mob| {
            let state = mob.equine.as_mut().expect("donkey state");
            state.tame = tame;
            state.chested = true;
            state.chest_items = vec![ItemStack::empty(); 15];
            mob.equipment
                .set(EquipmentSlot::Saddle, ItemStack::new(items::SADDLE, 1));
        });
        let uid = server.spawn_entity(donkey);
        let (mut level, ticks) = server.region().block_level();
        let _ = mc_bamboo_blocks::run_entity_tick(&mut level, ticks);
        (server, client, uid)
    }

    fn click(server: &mut Harness, client: &Client, slot: i16, input: ContainerInput) {
        let container_id = server.region().menus[&client.id].container_id;
        let packet = ClickPacket {
            container_id,
            state_id: 0,
            slot,
            button: 0,
            input,
            changed_slots: BTreeMap::new(),
            carried: HashedStack::Empty,
        };
        on_click(server.region(), client.id, packet);
    }

    fn saddle_of(server: &mut Harness, uid: i32) -> ItemStack {
        let entity = server.region().entities.get(uid).expect("donkey row");
        let mob = entity.kind.mob().expect("donkey is a mob");
        mob.equipment.get(EquipmentSlot::Saddle).clone()
    }

    fn saddle_sounds(server: &mut Harness, uid: i32) -> usize {
        let entity = server.region().entities.get(uid).expect("donkey row");
        let mob = entity.kind.mob().expect("donkey is a mob");
        mob.events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    MobEvent::Sound {
                        sound: "minecraft:entity.horse.saddle",
                        ..
                    }
                )
            })
            .count()
    }

    /// 潜行右键驯服的驴：发 `mount_screen_open`（5 列、实体 ID 是 int），
    /// Shift 点鞍位把鞍放进快捷栏最后一格，再 Shift 点回去两次出鞍声。
    ///
    /// Sneak-clicking a tamed donkey sends `mount_screen_open` (5 columns, an int
    /// entity id); shift-clicking the saddle moves it to the last hotbar slot, and
    /// shift-clicking it back plays the saddle sound twice.
    #[test]
    fn sneaking_on_a_tamed_donkey_opens_its_inventory_and_moves_the_saddle() {
        let (mut server, client, uid) = stage(true);
        crate::interaction::on_interact_entity(server.region(), client.id, uid, Hand::Main, true);
        server.region().links.flush();
        let frames = client.take_frames();
        let opened = frames_with_id(&frames, 42);
        assert_eq!(opened.len(), 1, "one mount_screen_open");
        let container_id = server.region().menus[&client.id].container_id;
        let mut expected = vec![container_id as u8, 5];
        expected.extend_from_slice(&uid.to_be_bytes());
        assert_eq!(opened[0].body, expected);

        let last_hotbar = (2 + 15 + 36 - 1) as i16;
        click(&mut server, &client, 0, ContainerInput::QuickMove);
        assert!(saddle_of(&mut server, uid).is_empty());
        let held = server.region().players[&client.id].inventory.slots[44].clone();
        assert_eq!(held.item, items::SADDLE);

        click(&mut server, &client, last_hotbar, ContainerInput::QuickMove);
        assert_eq!(saddle_of(&mut server, uid).item, items::SADDLE);
        assert!(server.region().players[&client.id].inventory.slots[44].is_empty());
        assert_eq!(saddle_sounds(&mut server, uid), 2);
    }

    /// 没驯服的驴潜行右键不开菜单。
    ///
    /// An untamed donkey does not open a menu on sneak-click.
    #[test]
    fn an_untamed_donkey_keeps_its_inventory_closed() {
        let (mut server, client, uid) = stage(false);
        crate::interaction::on_interact_entity(server.region(), client.id, uid, Hand::Main, true);
        assert!(!server.region().menus.contains_key(&client.id));
        server.region().links.flush();
        assert!(frames_with_id(&client.take_frames(), 42).is_empty());
    }

    /// 坐骑死了或者走远了，菜单下一刻失效。
    ///
    /// The menu stops being valid once the mount dies or is out of reach.
    #[test]
    fn the_menu_closes_when_the_mount_leaves_reach() {
        let (mut server, client, uid) = stage(true);
        crate::interaction::on_interact_entity(server.region(), client.id, uid, Hand::Main, true);
        assert!(server.region().menus.contains_key(&client.id));
        server
            .region()
            .entities
            .with_mut(uid, |entity| {
                entity.position = Vec3::new(30.5, SPAWN_Y, 8.5)
            })
            .expect("donkey row");
        let (mut level, ticks) = server.region().block_level();
        let _ = mc_bamboo_blocks::run_entity_tick(&mut level, ticks);
        crate::menu::tick(server.region());
        assert!(!server.region().menus.contains_key(&client.id));
    }
}
