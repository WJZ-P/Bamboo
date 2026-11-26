//! 玩家滑翔的开始条件与每刻更新。
//! Player glide eligibility and tick updates.

use mc_bamboo_blocks::{
    World,
    durability::{damage_of, hurt_and_break, next_damage_will_break},
    game_event::{GameEvent, dispatch::EventContext},
    state,
};
use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{
    BlockAncestor, blocks, items,
    registries::{data_component_type as components, mob_effect},
    tags::{block_tags, fluid_tags},
};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, defaults},
    item::ItemStack,
    packet::play::{EquipmentSlot, GameMode},
};

use crate::{
    entity_sync::EQUIPMENT_SLOTS, inbound::PlayerId, player::Player, player_move::PlayerBlocks,
    region::Region,
};

/// 检查装备组件和剩余耐久，对应 `LivingEntity.canGlideUsing`。
/// Check the equipment components and remaining durability.
fn can_glide_using(stack: &ItemStack, slot: EquipmentSlot) -> bool {
    if stack.is_empty() {
        return false;
    }
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return false;
    };
    // 装备组件用 EquipmentSlot.id，装备包用 ordinal。
    // The component uses EquipmentSlot.id, while the equipment packet uses its ordinal.
    let slot_id = match slot {
        EquipmentSlot::MainHand => 0,
        EquipmentSlot::Feet => 1,
        EquipmentSlot::Legs => 2,
        EquipmentSlot::Chest => 3,
        EquipmentSlot::Head => 4,
        EquipmentSlot::OffHand => 5,
        EquipmentSlot::Body => 6,
        EquipmentSlot::Saddle => 7,
    };
    defaults::resolve(stack.item, &patch, components::GLIDER).is_some()
        && matches!(
            defaults::resolve(stack.item, &patch, components::EQUIPPABLE),
            Some(ComponentValue::Equippable(equippable)) if equippable.slot == slot_id
        )
        && !next_damage_will_break(stack)
}

fn can_glide(player: &Player, blocks: &PlayerBlocks<'_>) -> bool {
    let feet = BlockPos::containing(player.position());
    let fluid = blocks.state(feet).fluid();
    !player.dead
        && player.spawn_pending.is_none()
        && !player.on_ground
        && !player.riding.is_passenger()
        && !player.sync.flying
        && !player.effects.has(mob_effect::LEVITATION)
        && !fluid.is_tagged(fluid_tags::WATER)
        && !fluid.is_tagged(fluid_tags::LAVA)
        && !on_climbable(player, blocks)
        && gliders(player).next().is_some()
}

fn gliders(player: &Player) -> impl Iterator<Item = EquipmentSlot> + '_ {
    EQUIPMENT_SLOTS.into_iter().filter(|&slot| {
        player
            .inventory
            .equipment(slot)
            .is_some_and(|stack| can_glide_using(stack, slot))
    })
}

/// 玩家脚下有可攀爬方块，或方向相同的梯子延伸到打开的活板门。
/// The player's feet are in a climbable block or an open trapdoor above a matching ladder.
pub(crate) fn on_climbable(player: &Player, blocks: &PlayerBlocks<'_>) -> bool {
    if player.sync.flying || player.game_mode == GameMode::Spectator {
        return false;
    }
    let pos = BlockPos::containing(player.position());
    let state = blocks.state(pos);
    if player.sync.is_fall_flying() && state.is_tagged(block_tags::CAN_GLIDE_THROUGH) {
        return false;
    }
    if state.is_tagged(block_tags::CLIMBABLE) {
        return true;
    }
    if state.is_a(BlockAncestor::TrapDoorBlock) && state.try_get(state::OPEN) == Some(true) {
        let below = blocks.state(pos.below());
        return below.is(blocks::LADDER)
            && below.try_get(state::HORIZONTAL_FACING) == state.try_get(state::HORIZONTAL_FACING);
    }
    false
}

/// 处理开始滑翔请求，成功时设置共享标志。
/// Handle a glide request and set the shared flag on success.
pub(crate) fn try_start(region: &mut Region, id: PlayerId) -> bool {
    let Some(player) = region.players.get_mut(&id) else {
        return false;
    };
    let blocks = PlayerBlocks::new(&region.chunks, &region.block_entities);
    if player.sync.is_fall_flying() || !can_glide(player, &blocks) {
        return false;
    }
    player.sync.set_fall_flying(true);
    true
}

/// 更新所有玩家的滑翔状态，对应 `LivingEntity.updateFallFlying`。
/// Update player gliding, matching `LivingEntity.updateFallFlying`.
pub(crate) fn tick(region: &mut Region) {
    let ids: Vec<_> = region
        .players
        .values()
        .filter(|player| player.sync.is_fall_flying() || player.fall_fly_ticks != 0)
        .map(|player| player.id)
        .collect();
    for id in ids {
        if update_fall_flying(region, id) {
            crate::player_move::travel_fall_flying(region, id);
        }
    }
}

fn update_fall_flying(region: &mut Region, id: PlayerId) -> bool {
    let Some(player) = region.players.get_mut(&id) else {
        return false;
    };
    if player.sync.is_fall_flying() && player.motion.delta.y > -0.5 {
        player.motion.fall_distance = player.motion.fall_distance.min(1.0);
    }
    let blocks = PlayerBlocks::new(&region.chunks, &region.block_entities);
    if !player.sync.is_fall_flying() || !can_glide(player, &blocks) {
        player.sync.set_fall_flying(false);
        player.fall_fly_ticks = 0;
        return false;
    }
    player.fall_fly_ticks = player.fall_fly_ticks.wrapping_add(1);
    if player.fall_fly_ticks % 10 != 0 {
        return true;
    }
    let position = player.position();
    let context = EventContext {
        sneaking: player.sync.shift_key_down,
        spectator: player.game_mode == GameMode::Spectator,
        source_player: Some(id.0),
        source_entity: Some(id.0),
        ..Default::default()
    };
    if player.fall_fly_ticks % 20 == 0 {
        damage_glider(region, id);
    }
    let (mut level, ticks) = region.block_level();
    let mut world = World::new(&mut level, ticks);
    world.game_event(GameEvent::ElytraGlide, position, context);
    let effects = world.into_effects();
    crate::interaction::send_effects(region, Some(id), &effects);
    true
}

fn damage_glider(region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let slots: Vec<_> = gliders(player).collect();
    if slots.is_empty() {
        return;
    }
    let equipment = slots[player.random.next_int_bounded(slots.len() as i32) as usize];
    if player.game_mode == GameMode::Creative {
        return;
    }
    let Some(slot) = player.inventory.equipment_index(equipment) else {
        return;
    };
    let stack = &mut player.inventory.slots[slot];
    let before = damage_of(stack);
    let damaged = hurt_and_break(stack.clone(), 1, region.random.source());
    if damaged.matches(stack) {
        return;
    }
    let after = damage_of(&damaged);
    let item = items::ITEMS.get(stack.item as usize).map(|item| item.name);
    let broke = damaged.count < stack.count;
    *stack = if damaged.is_empty() {
        ItemStack::empty()
    } else {
        damaged
    };
    crate::player_weapon::sync_slots(region, id, [slot]);
    if let Some((before, after)) = before.zip(after).filter(|(before, after)| before != after) {
        crate::advancement_triggers::record(
            region,
            id,
            "minecraft:item_durability_changed",
            serde_json::json!({
                "item": item,
                "durability": after,
                "delta": after - before,
            }),
        );
    }
    if broke {
        crate::durability::broadcast_equipment_break(region, id, equipment);
    }
}
