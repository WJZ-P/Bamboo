//! 玩家开始滑翔，对应 `Player.tryToStartFallFlying`。
//! Starting a player glide, matching `Player.tryToStartFallFlying`.

use mc_bamboo_blocks::{durability::next_damage_will_break, state};
use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{
    BlockAncestor, blocks,
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
    !player.on_ground
        && !player.riding.is_passenger()
        && !player.sync.flying
        && !player.effects.has(mob_effect::LEVITATION)
        && !fluid.is_tagged(fluid_tags::WATER)
        && !fluid.is_tagged(fluid_tags::LAVA)
        && !on_climbable(player, blocks)
        && EQUIPMENT_SLOTS.into_iter().any(|slot| {
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
