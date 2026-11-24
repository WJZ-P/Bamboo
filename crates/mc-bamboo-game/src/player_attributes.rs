//! 玩家属性表：每刻按当前状态重算修饰符，并给 `Player.getDestroySpeed` 提供输入。
//!
//! 官方 `LivingEntity` 把属性存在 `AttributeMap` 里，装备变化（`collectEquipmentChanges`）、
//! 药水效果（`MobEffect.addAttributeModifiers`）、创造模式触及距离、疾跑 / 细雪都以
//! `addTransientModifier` / `addPermanentModifier` 挂上去。这里不逐事件增删，而是每刻按
//! 当前状态把「临时修饰符」整体重建一遍：基础值和存档里的永久修饰符保留，其余清掉后
//! 从装备（六槽，含附魔 `minecraft:attributes`）、效果、游戏模式重新算。结果永远一致，
//! 也方便每刻的脏属性同步比对。
//!
//! 重建点放在挖掘 / 攻击 / 效果 / 同步之前（见 `world::run_island_simulation`）。入站包
//! 在重建之前处理，所以当场读到的属性是上一刻末的状态，和官方 `detectEquipmentUpdates`
//! 在实体 tick 里跑的一样有一刻延迟。
//!
//! Player attribute map. Rather than mirroring each add/remove event, Bamboo rebuilds
//! all transient modifiers each tick from the current equipment, effects and game mode,
//! keeping base values and saved permanent modifiers. This runs before mining, attack,
//! effects and sync; inbound packets are handled before it, matching the one-tick lag of
//! vanilla `detectEquipmentUpdates`.

use mc_bamboo_blocks::entity::mob::{
    attributes::{Attributes, Modifier, Operation},
    effects::attribute_modifiers,
    equipment::stack_modifiers,
};
use mc_bamboo_data::registries::{attribute, mob_effect};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{EquipmentSlot, GameMode},
};

use crate::{player::Player, region::Region};

/// 六个玩家装备槽，顺序和 [`Player::equipment_stacks`] 一致（按 `EquipmentSlot` ordinal）。
///
/// The six player equipment slots, in `Player::equipment_stacks` order.
const EQUIPMENT_SLOTS: [EquipmentSlot; 6] = [
    EquipmentSlot::MainHand,
    EquipmentSlot::OffHand,
    EquipmentSlot::Feet,
    EquipmentSlot::Legs,
    EquipmentSlot::Chest,
    EquipmentSlot::Head,
];

/// `Player.getDestroySpeed` 要读的属性值（已 `d2f`）和状态效果。
///
/// The resolved attribute values (already `d2f`) and status effects that
/// `Player.getDestroySpeed` reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MiningInputs {
    /// `(float) MINING_EFFICIENCY`。
    pub mining_efficiency: f32,
    /// `(float) BLOCK_BREAK_SPEED`。
    pub block_break_speed: f32,
    /// `(float) SUBMERGED_MINING_SPEED`。
    pub submerged_mining_speed: f32,
    /// `MobEffectUtil.hasDigSpeed`：有急迫或潮涌能量。
    pub has_dig_speed: bool,
    /// `MobEffectUtil.getDigSpeedAmplification`。
    pub dig_speed_amplification: i32,
    /// 有 `MINING_FATIGUE` 时的放大器；`None` 表示没有。
    pub mining_fatigue: Option<i32>,
}

/// 从属性表和效果表读出 `getDestroySpeed` 的输入。属性的 `double` 在这里 `d2f`，
/// 和官方 `(float) getAttributeValue(...)` 一致。
///
/// Reads `getDestroySpeed` inputs from the attribute/effect maps; each attribute
/// `double` is narrowed with `d2f`, matching `(float) getAttributeValue(...)`.
pub(crate) fn mining_inputs(player: &Player) -> MiningInputs {
    let haste = player.effects.amplifier(mob_effect::HASTE);
    let conduit = player.effects.amplifier(mob_effect::CONDUIT_POWER);
    MiningInputs {
        mining_efficiency: player.attributes.value(attribute::MINING_EFFICIENCY) as f32,
        block_break_speed: player.attributes.value(attribute::BLOCK_BREAK_SPEED) as f32,
        submerged_mining_speed: player.attributes.value(attribute::SUBMERGED_MINING_SPEED) as f32,
        // MobEffectUtil：hasDigSpeed = 有 haste 或 conduit_power；
        // getDigSpeedAmplification = max(haste_amp, conduit_amp)，缺的按 0。
        has_dig_speed: haste.is_some() || conduit.is_some(),
        dig_speed_amplification: haste.unwrap_or(0).max(conduit.unwrap_or(0)),
        mining_fatigue: player.effects.amplifier(mob_effect::MINING_FATIGUE),
    }
}

/// 每刻重算所有玩家的属性修饰符，早于挖掘 / 攻击 / 效果 / 同步调用。
///
/// Rebuilds every player's attribute modifiers once per tick, before mining,
/// attack, effects and sync.
pub(crate) fn rebuild(region: &mut Region) {
    for player in region.players.values_mut() {
        rebuild_one(player);
    }
}

/// 官方 `ServerPlayer.updatePlayerAttributes` 的创造模式触及距离修饰符。
/// 数值和 `region::CREATIVE_ATTRIBUTES` 一致（有逐字节对拍）。
fn creative_modifiers() -> [(i32, Modifier); 2] {
    [
        (
            attribute::BLOCK_INTERACTION_RANGE,
            Modifier {
                id: "minecraft:creative_mode_block_range".to_owned(),
                amount: 0.5,
                operation: Operation::AddValue,
                permanent: false,
            },
        ),
        (
            attribute::ENTITY_INTERACTION_RANGE,
            Modifier {
                id: "minecraft:creative_mode_entity_range".to_owned(),
                amount: 2.0,
                operation: Operation::AddValue,
                permanent: false,
            },
        ),
    ]
}

/// 当前装备 / 效果 / 游戏模式给出的全部临时修饰符，按官方 `addXModifier` 的顺序。
/// 疾跑、细雪的 transient / permanent 修饰符待接入（任务 #14）。
///
/// All transient modifiers from equipment, effects and game mode, in vanilla
/// `addXModifier` order. Sprinting / powder-snow modifiers are not wired yet.
fn transient_modifiers(player: &Player) -> Vec<(i32, Modifier)> {
    let mut mods = Vec::new();
    // 装备：六槽的 attribute_modifiers 组件 + 附魔 minecraft:attributes。
    let empty = ItemStack::empty();
    let stacks = player.equipment_stacks(&empty);
    for (slot, stack) in EQUIPMENT_SLOTS.into_iter().zip(stacks) {
        mods.extend(stack_modifiers(stack, slot));
    }
    // 药水效果：MobEffect.addAttributeModifiers 的 minecraft:effect.* 修饰符。
    for instance in player.effects.iter() {
        mods.extend(attribute_modifiers(instance.effect, instance.amplifier));
    }
    // 创造模式触及距离。
    if player.game_mode == GameMode::Creative {
        mods.extend(creative_modifiers());
    }
    mods
}

fn rebuild_one(player: &mut Player) {
    // 先算成拥有所有权的 Vec，避免和下面 &mut player.attributes 撞借用。
    // Compute into an owned Vec first, so the immutable borrows end before
    // the mutable borrow of `player.attributes` below.
    let mods = transient_modifiers(player);
    let attributes: &mut Attributes = &mut player.attributes;
    // 存档里的永久修饰符保留，上一刻的临时修饰符整体清掉。
    // Keep saved permanent modifiers, drop all transient ones from last tick.
    for entry in &mut attributes.entries {
        entry.modifiers.retain(|modifier| modifier.permanent);
    }
    // `addTransientModifier`：同 id 的替换（官方用按 id 的集合）。
    // `addTransientModifier` replaces by id.
    for (id, modifier) in mods {
        if let Some(instance) = attributes.instance(id) {
            instance.remove_modifier(&modifier.id);
            instance.modifiers.push(modifier);
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::entity::mob::effects::EffectInstance;
    use mc_bamboo_protocol::packet::play::ClientInformation;
    use uuid::Uuid;

    use super::*;
    use crate::inbound::PlayerId;

    fn player(game_mode: GameMode) -> Player {
        Player::new(
            PlayerId(1),
            Uuid::nil(),
            "T".to_owned(),
            game_mode,
            ClientInformation::default(),
            mc_bamboo_core::Vec3::default(),
        )
    }

    #[test]
    fn plain_player_uses_registry_default_mining_attributes() {
        let p = player(GameMode::Survival);
        let inputs = mining_inputs(&p);
        assert_eq!(inputs.mining_efficiency, 0.0);
        assert_eq!(inputs.block_break_speed, 1.0);
        assert_eq!(inputs.submerged_mining_speed, 0.2_f32);
        assert!(!inputs.has_dig_speed);
        assert_eq!(inputs.dig_speed_amplification, 0);
        assert_eq!(inputs.mining_fatigue, None);
    }

    #[test]
    fn creative_mode_adds_the_two_reach_modifiers() {
        let mut p = player(GameMode::Creative);
        rebuild_one(&mut p);
        let block_reach = p
            .attributes
            .get(attribute::BLOCK_INTERACTION_RANGE)
            .unwrap();
        assert_eq!(block_reach.value(), 5.0);
        assert!(
            block_reach
                .modifier("minecraft:creative_mode_block_range")
                .is_some()
        );
        let entity_reach = p
            .attributes
            .get(attribute::ENTITY_INTERACTION_RANGE)
            .unwrap();
        assert_eq!(entity_reach.value(), 5.0);
        assert!(
            entity_reach
                .modifier("minecraft:creative_mode_entity_range")
                .is_some()
        );
    }

    #[test]
    fn mining_inputs_follow_haste_conduit_and_fatigue() {
        let mut p = player(GameMode::Survival);
        p.effects
            .add(EffectInstance::with_amplifier(mob_effect::HASTE, 100, 2));
        p.effects.add(EffectInstance::with_amplifier(
            mob_effect::MINING_FATIGUE,
            100,
            1,
        ));
        let inputs = mining_inputs(&p);
        assert!(inputs.has_dig_speed);
        assert_eq!(inputs.dig_speed_amplification, 2);
        assert_eq!(inputs.mining_fatigue, Some(1));

        // 潮涌能量等级更高时取较大值。
        p.effects.add(EffectInstance::with_amplifier(
            mob_effect::CONDUIT_POWER,
            100,
            4,
        ));
        assert_eq!(mining_inputs(&p).dig_speed_amplification, 4);
    }

    #[test]
    fn rebuild_is_idempotent_and_drops_stale_modifiers() {
        let mut p = player(GameMode::Creative);
        rebuild_one(&mut p);
        let once = p.attributes.clone();
        rebuild_one(&mut p);
        assert_eq!(p.attributes, once);
        // 切回生存，创造修饰符被清掉，触及距离回到基础值。
        p.game_mode = GameMode::Survival;
        rebuild_one(&mut p);
        assert_eq!(p.attributes.value(attribute::BLOCK_INTERACTION_RANGE), 4.5);
        assert!(
            p.attributes
                .get(attribute::BLOCK_INTERACTION_RANGE)
                .unwrap()
                .modifier("minecraft:creative_mode_block_range")
                .is_none()
        );
    }
}
