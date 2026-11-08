//! 把玩家的两只手交给方块逻辑，动作结束后写回物品栏。
//!
//! 创造模式只把用掉的那只手的数量改回去，耐久不改。物品损坏时发实体事件，
//! 没有打开菜单时再把那一格发给客户端。
//!
//! Installs the player's hands for block logic and writes them back afterwards.
//!
//! Creative mode restores only the used hand's count; damage stays. A break sends an entity
//! event, and the slot is sent when no menu is open.

use mc_bamboo_blocks::durability::{ActiveHands, with_player_hands};
use mc_bamboo_core::Hand;
use mc_bamboo_protocol::packet::play::{EntityEvent, GameMode};

use crate::{
    inbound::PlayerId,
    inventory::{self, Inventory},
    links::encode_frames,
    region::Region,
};

/// `LivingEntity.entityEventForEquipmentBreak`：主手 47，副手 48。
const BREAK_MAIN_HAND: u8 = 47;
const BREAK_OFF_HAND: u8 = 48;

struct Prepared {
    hands: ActiveHands,
    used_count: i32,
}

/// 跑 `body` 时方块逻辑看得到这名玩家的手。玩家不在了就只跑 `body`。
pub(crate) fn run<R>(
    region: &mut Region,
    id: PlayerId,
    hand: Hand,
    body: impl FnOnce(&mut Region) -> R,
) -> R {
    let Some(prepared) = prepare(region, id, hand) else {
        return body(region);
    };
    let (result, hands) = with_player_hands(prepared.hands, || body(region));
    finish(region, id, hand, prepared.used_count, hands);
    result
}

fn prepare(region: &Region, id: PlayerId, hand: Hand) -> Option<Prepared> {
    let player = region.players.get(&id)?;
    let inventory = &player.inventory;
    Some(Prepared {
        used_count: inventory.held(hand).count,
        hands: ActiveHands {
            main: inventory.held(Hand::Main).clone(),
            off: inventory.held(Hand::Off).clone(),
            infinite_materials: player.game_mode == GameMode::Creative,
            broken: Vec::new(),
        },
    })
}

fn finish(region: &mut Region, id: PlayerId, hand: Hand, used_count: i32, mut hands: ActiveHands) {
    if hands.infinite_materials {
        // `ServerPlayerGameMode.useItemOn` 把用掉的那只手的数量改回去。耐久留着。
        //
        // `ServerPlayerGameMode.useItemOn` restores the used hand's count. Damage stays.
        match hand {
            Hand::Main => hands.main.count = used_count,
            Hand::Off => hands.off.count = used_count,
        }
    }
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let broken = hands.broken.clone();
    let entity_id = player.id.0;
    let changed = write_hands(&mut player.inventory, hands);
    let menu_open = region.menus.contains_key(&id);
    for hand in broken {
        broadcast_break(region, id, entity_id, break_event(hand));
    }
    if menu_open {
        return;
    }
    for slot in changed {
        let Some(player) = region.players.get_mut(&id) else {
            return;
        };
        inventory::send_slot(player, &mut region.links, slot);
    }
}

fn write_hands(inventory: &mut Inventory, hands: ActiveHands) -> Vec<usize> {
    let mut changed = Vec::new();
    for (hand, stack) in [(Hand::Main, hands.main), (Hand::Off, hands.off)] {
        let slot = inventory.hand_slot(hand);
        if !inventory.slots[slot].matches(&stack) {
            inventory.slots[slot] = stack;
            changed.push(slot);
        }
    }
    changed
}

fn break_event(hand: Hand) -> u8 {
    match hand {
        Hand::Main => BREAK_MAIN_HAND,
        Hand::Off => BREAK_OFF_HAND,
    }
}

/// `broadcastEntityEvent`：自己和跟踪这个玩家的人都收到。
///
/// `broadcastEntityEvent`: the player and everyone tracking them receive it.
fn broadcast_break(region: &mut Region, id: PlayerId, entity_id: i32, event: u8) {
    let frames = encode_frames(&EntityEvent { entity_id, event });
    region.links.send_frames(id, &frames);
    if let Some(player) = region.players.get(&id) {
        player.sync.send_to_viewers(&mut region.links, &frames);
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{BlockPos, Direction, Hand};
    use mc_bamboo_data::{
        blocks, items, packets::play::clientbound, registries::data_component_type as kinds,
    };
    use mc_bamboo_protocol::{
        components::{ComponentPatch, ComponentValue, PatchEntry, defaults},
        item::ItemStack,
        packet::play::{PlayRequest, PlayerActionKind},
    };

    use super::*;
    use crate::world::tests::Harness;

    fn almost_broken(item: i32) -> ItemStack {
        let max = match defaults::of_item(item).get(kinds::MAX_DAMAGE) {
            Some(ComponentValue::MaxDamage(value)) => *value,
            _ => panic!("{item} should have max_damage"),
        };
        let mut patch = ComponentPatch::new();
        patch.put(
            kinds::DAMAGE,
            PatchEntry::Set(ComponentValue::Damage(max - 1)),
        );
        let (added, removed) = patch.to_opaque();
        ItemStack {
            item,
            count: 1,
            added,
            removed,
        }
    }

    #[test]
    fn a_tool_on_its_last_point_breaks_in_survival() {
        let mut server = Harness::new(2);
        let client = server.join_and_settle("kitten");
        client.take_frames();
        let slot = {
            let player = server.region().players.get_mut(&client.id).unwrap();
            let slot = player.inventory.hand_slot(Hand::Main);
            player.inventory.slots[slot] = almost_broken(items::SHEARS);
            slot
        };
        let pos = BlockPos::new(8, -60, 9);
        server.region().chunks.set_block(pos, blocks::SHORT_GRASS);
        server.request(
            &client,
            PlayRequest::ChatCommand {
                command: "gamemode survival".to_owned(),
            },
        );
        server.tick();
        client.take_frames();

        server.request(
            &client,
            PlayRequest::PlayerAction {
                action: PlayerActionKind::StartDestroyBlock,
                pos,
                face: Direction::Up,
                sequence: 1,
            },
        );
        server.tick();
        let frames = client.take_frames();
        let event = frames
            .iter()
            .find(|frame| frame.id == clientbound::ENTITY_EVENT)
            .expect("break event");
        assert_eq!(&event.body[4..], &[BREAK_MAIN_HAND]);
        assert!(
            server
                .region()
                .players
                .get(&client.id)
                .unwrap()
                .inventory
                .slots[slot]
                .is_empty()
        );
        assert!(
            frames
                .iter()
                .any(|frame| frame.id == clientbound::CONTAINER_SET_SLOT)
        );
    }
}
