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

use mc_bamboo_blocks::{
    BlockState,
    entity::mob::{
        attributes::{Attributes, Modifier, Operation},
        effects::attribute_modifiers,
        equipment::stack_modifiers,
    },
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{
    is_client_syncable_attribute,
    registries::{attribute, mob_effect},
};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{
        AttributeModifier, AttributeOperation, AttributeSnapshot, EquipmentSlot, GameMode,
        UpdateAttributes,
    },
};

use crate::{inbound::PlayerId, links::encode_frames, player::Player, region::Region};

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
    let Region {
        players, chunks, ..
    } = region;
    for player in players.values_mut() {
        let on_solid = frost_block_solid(player, chunks);
        rebuild_player(player, on_solid);
    }
}

/// 官方 `LivingEntity.tryAddFrost`：`!getBlockStateOnLegacy().isAir()`，
/// 也就是脚底下方 0.2 格（`Entity.getOnPos(0.2F)`）不是空气。
/// 没冻结时直接返回假，避免每刻无谓查区块。
///
/// Vanilla `LivingEntity.tryAddFrost`: `!getBlockStateOnLegacy().isAir()`,
/// i.e. the block 0.2 below feet is not air. Skips the chunk lookup when not frozen.
fn frost_block_solid(player: &Player, chunks: &mc_bamboo_world::store::ChunkStore) -> bool {
    if player.environment.ticks_frozen <= 0 {
        return false;
    }
    let pos = player.position();
    let below = BlockPos::containing(Vec3::new(pos.x, pos.y - 0.2, pos.z));
    chunks
        .block(below)
        .is_some_and(|id| !BlockState(id).is_air())
}

/// 官方 `ServerPlayer.updatePlayerAttributes` 的创造模式触及距离修饰符：
/// `minecraft:creative_mode_block_range` +0.5、`minecraft:creative_mode_entity_range` +2.0，
/// 都是 `ADD_VALUE` 临时修饰符。
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

/// 官方 `ServerPlayer.updatePlayerAttributes`：蹲下时给
/// `WAYPOINT_TRANSMIT_RANGE` 挂 `minecraft:waypoint_transmit_range_crouch`
/// （-1.0，`ADD_MULTIPLIED_TOTAL`，临时修饰符）。
fn crouch_waypoint_modifier() -> (i32, Modifier) {
    (
        attribute::WAYPOINT_TRANSMIT_RANGE,
        Modifier {
            id: "minecraft:waypoint_transmit_range_crouch".to_owned(),
            amount: -1.0,
            operation: Operation::AddMultipliedTotal,
            permanent: false,
        },
    )
}

/// 官方 `LivingEntity.SPEED_MODIFIER_SPRINTING`：`minecraft:sprinting`
/// （`(double) 0.3F`，`ADD_MULTIPLIED_TOTAL`，临时修饰符）。
fn sprinting_modifier() -> (i32, Modifier) {
    (
        attribute::MOVEMENT_SPEED,
        Modifier {
            id: "minecraft:sprinting".to_owned(),
            amount: f64::from(0.3_f32),
            operation: Operation::AddMultipliedTotal,
            permanent: false,
        },
    )
}

/// 官方 `LivingEntity.tryAddFrost`：`minecraft:powder_snow`
/// （`(double) (-0.05F * getPercentFrozen())`，`ADD_VALUE`，临时修饰符）。
fn powder_snow_modifier(ticks_frozen: i32) -> (i32, Modifier) {
    let percent = ticks_frozen.min(140) as f32 / 140.0_f32;
    (
        attribute::MOVEMENT_SPEED,
        Modifier {
            id: "minecraft:powder_snow".to_owned(),
            amount: f64::from(-0.05_f32 * percent),
            operation: Operation::AddValue,
            permanent: false,
        },
    )
}

/// 当前装备 / 效果 / 游戏模式 / 疾跑 / 细雪 / 潜行给出的全部临时修饰符。
///
/// All transient modifiers from equipment, effects, game mode, sprinting,
/// powder snow and crouching.
fn transient_modifiers(player: &Player, on_solid_for_frost: bool) -> Vec<(i32, Modifier)> {
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
    // 潜行隐藏路标发送距离。
    if player.sync.is_crouching() {
        mods.push(crouch_waypoint_modifier());
    }
    // 疾跑加速。
    if player.sync.is_sprinting() {
        mods.push(sprinting_modifier());
    }
    // 细雪减速。
    if on_solid_for_frost && player.environment.ticks_frozen > 0 {
        mods.push(powder_snow_modifier(player.environment.ticks_frozen));
    }
    mods
}

/// 重建单名玩家的全部临时修饰符，保留基础值和存档永久修饰符。
///
/// Rebuilds one player's transient modifiers, keeping base values and saved
/// permanent modifiers.
pub(crate) fn rebuild_player(player: &mut Player, on_solid_for_frost: bool) {
    // 先算成拥有所有权的 Vec，避免和下面 &mut player.attributes 撞借用。
    // Compute into an owned Vec first, so the immutable borrows end before
    // the mutable borrow of `player.attributes` below.
    let mods = transient_modifiers(player, on_solid_for_frost);
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

/// 一次属性同步的快照状态：(`id`, `base`, `modifiers`, `sync_version`)。
/// `EntitySync` 存"上次发出的状态"做逐刻脏比对。
///
/// One attribute's sync state. `EntitySync` keeps the last-sent states for
/// the per-tick dirty comparison.
pub(crate) type AttributeState = (i32, f64, Vec<(String, f64, i32)>, u64);

fn operation_code(operation: Operation) -> i32 {
    match operation {
        Operation::AddValue => 0,
        Operation::AddMultipliedBase => 1,
        Operation::AddMultipliedTotal => 2,
    }
}

/// 当前全部 `is_client_syncable_attribute` 属性的状态（按 entries 顺序）。
///
/// The state of every client-syncable attribute, in table order.
pub(crate) fn syncable_states(player: &Player) -> Vec<AttributeState> {
    player
        .attributes
        .entries
        .iter()
        .filter(|entry| is_client_syncable_attribute(entry.id))
        .map(|entry| {
            let modifiers = entry
                .modifiers
                .iter()
                .map(|m| (m.id.clone(), m.amount, operation_code(m.operation)))
                .collect();
            (entry.id, entry.base, modifiers, entry.sync_version)
        })
        .collect()
}

/// 把可同步属性编成 `UpdateAttributes` 帧。`only` 给定时只发这些 id（脏同步），
/// 为 `None` 时发全部（全量同步）。返回的是可直接 `send_frames` 的完整帧。
///
/// Encode `UpdateAttributes` frames for the syncable attributes. `only` restricts
/// to dirty ids; `None` sends all. Returns complete frames ready for `send_frames`.
fn attribute_frames(player: &Player, only: Option<&[i32]>) -> Option<Vec<u8>> {
    let wanted = |id: i32| only.is_none_or(|ids| ids.contains(&id));
    let mut snapshots = Vec::new();
    let mut modifiers: Vec<Vec<AttributeModifier<'_>>> = Vec::new();
    for entry in &player.attributes.entries {
        if !is_client_syncable_attribute(entry.id) || !wanted(entry.id) {
            continue;
        }
        let list = entry
            .modifiers
            .iter()
            .map(|m| AttributeModifier {
                id: &m.id,
                amount: m.amount,
                operation: match m.operation {
                    Operation::AddValue => AttributeOperation::AddValue,
                    Operation::AddMultipliedBase => AttributeOperation::AddMultipliedBase,
                    Operation::AddMultipliedTotal => AttributeOperation::AddMultipliedTotal,
                },
            })
            .collect();
        modifiers.push(list);
        snapshots.push((entry.id, entry.base));
    }
    let attributes: Vec<AttributeSnapshot<'_>> = snapshots
        .iter()
        .zip(&modifiers)
        .map(|(&(id, base), list)| AttributeSnapshot {
            attribute: id,
            base,
            modifiers: list,
        })
        .collect();
    if attributes.is_empty() {
        return None;
    }
    Some(encode_frames(&UpdateAttributes {
        entity_id: player.id.0,
        attributes: &attributes,
    }))
}

/// 全量可同步属性帧（进服 / 重生 / 配对）。
///
/// Full syncable-attribute frames (join / respawn / pairing).
pub(crate) fn full_frames(player: &Player) -> Option<Vec<u8>> {
    attribute_frames(player, None)
}

/// 进服 / 重生 / 切模式：给玩家自己发全量属性，并记下已同步状态。
/// 先按当前状态重建一次，保证发出的属性和随后每刻重建的一致（不会有首刻假脏）。
///
/// Send full attributes to the player's own client and record the synced state.
/// Rebuilds first so the sent state matches the next tick's rebuild (no spurious
/// first-tick dirty packet).
pub(crate) fn send_full_to_self(region: &mut Region, id: PlayerId) {
    let (frames, states) = {
        let Region {
            players, chunks, ..
        } = region;
        let Some(player) = players.get_mut(&id) else {
            return;
        };
        let on_solid = frost_block_solid(player, chunks);
        rebuild_player(player, on_solid);
        (full_frames(player), syncable_states(player))
    };
    if let Some(frames) = frames {
        region.links.send_frames(id, &frames);
    }
    if let Some(player) = region.players.get_mut(&id) {
        player.sync.sent_attributes = states;
    }
}

/// 每刻脏属性同步：和自己 + 观察者上次发出的状态比，变了就发 `UpdateAttributes`。
/// `sent_attributes` 为空时（进服第一刻 / 重生后清空）发全量。
///
/// Per-tick dirty attribute sync. Compares with the last-sent state and sends
/// changed attributes to the player and every observer. An empty
/// `sent_attributes` (first tick / after a respawn reset) sends everything.
pub(crate) fn sync_dirty(region: &mut Region) {
    let ids: Vec<PlayerId> = region.players.keys().copied().collect();
    for id in ids {
        let (frames, viewers, current) = {
            let Some(player) = region.players.get(&id) else {
                continue;
            };
            let current = syncable_states(player);
            let sent = &player.sync.sent_attributes;
            let changed: Vec<i32> = if sent.is_empty() {
                current.iter().map(|state| state.0).collect()
            } else {
                current
                    .iter()
                    .filter(|state| {
                        !sent.iter().any(|prev| {
                            prev.0 == state.0
                                && prev.1 == state.1
                                && prev.2 == state.2
                                && prev.3 == state.3
                        })
                    })
                    .map(|state| state.0)
                    .collect()
            };
            let frames = if changed.is_empty() {
                None
            } else {
                attribute_frames(player, Some(&changed))
            };
            let viewers: Vec<PlayerId> = player.sync.seen_by().iter().copied().collect();
            (frames, viewers, current)
        };
        if let Some(frames) = frames {
            // 官方 `ServerEntity.sendDirtyEntityData` 调 `sendToTrackingPlayersAndSelf`：
            // 脏属性发给所有正在追踪的观察者以及玩家自己，这样客户端的最大生命、移速、
            // 挖掘速度等状态和服务端保持一致。观察者与自己共享同一份字节。
            //
            // Vanilla `ServerEntity.sendDirtyEntityData` calls
            // `sendToTrackingPlayersAndSelf`, sending dirty attributes to every
            // tracking observer and the player themselves. All recipients share
            // the same encoded bytes.
            for viewer in &viewers {
                region.links.send_frames(*viewer, &frames);
            }
            region.links.send_frames(id, &frames);
        }
        if let Some(player) = region.players.get_mut(&id) {
            player.sync.sent_attributes = current;
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
        rebuild_player(&mut p, false);
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
        rebuild_player(&mut p, false);
        let once = p.attributes.clone();
        rebuild_player(&mut p, false);
        assert_eq!(p.attributes, once);
        // 切回生存，创造修饰符被清掉，触及距离回到基础值。
        p.game_mode = GameMode::Survival;
        rebuild_player(&mut p, false);
        assert_eq!(p.attributes.value(attribute::BLOCK_INTERACTION_RANGE), 4.5);
        assert!(
            p.attributes
                .get(attribute::BLOCK_INTERACTION_RANGE)
                .unwrap()
                .modifier("minecraft:creative_mode_block_range")
                .is_none()
        );
    }

    #[test]
    fn sprinting_powder_snow_and_crouch_modifiers_match_vanilla() {
        use mc_bamboo_protocol::packet::play::ENTITY_FLAG_SPRINTING;

        let mut p = player(GameMode::Survival);
        // 疾跑：MOVEMENT_SPEED 乘 1 + (double)0.3F。
        p.sync.shared_flags |= ENTITY_FLAG_SPRINTING;
        rebuild_player(&mut p, false);
        let expected_sprint = f64::from(0.1_f32) * (1.0 + f64::from(0.3_f32));
        assert_eq!(
            p.attributes.value(attribute::MOVEMENT_SPEED),
            expected_sprint
        );

        // 细雪半冻（70/140）且脚下非空气：ADD_VALUE -0.05F * 0.5，在 ADD_MULTIPLIED_TOTAL 之前。
        p.environment.ticks_frozen = 70;
        rebuild_player(&mut p, true);
        let frost = f64::from(-0.05_f32 * (70.0_f32 / 140.0_f32));
        let expected_both = (f64::from(0.1_f32) + frost) * (1.0 + f64::from(0.3_f32));
        assert_eq!(p.attributes.value(attribute::MOVEMENT_SPEED), expected_both);
        // 悬空在空气上时不挂细雪减速。
        rebuild_player(&mut p, false);
        assert_eq!(
            p.attributes.value(attribute::MOVEMENT_SPEED),
            expected_sprint
        );

        // 潜行把 WAYPOINT_TRANSMIT_RANGE 乘 0。
        p.sync.shift_key_down = true;
        rebuild_player(&mut p, false);
        assert_eq!(p.attributes.value(attribute::WAYPOINT_TRANSMIT_RANGE), 0.0);
    }

    /// 速度药水后给观察者和自己各发一包脏 `update_attributes`：MOVEMENT_SPEED 带上
    /// `minecraft:effect.speed`（速度 II = 0.2*(1+1)，ADD_MULTIPLIED_TOTAL）。
    /// 稳定一刻（属性没变）不该再发包。
    ///
    /// A speed potion sends one dirty update_attributes packet to observers and
    /// to self for MOVEMENT_SPEED with the effect modifier; a stable tick sends none.
    #[test]
    fn a_speed_potion_sends_a_dirty_attribute_packet_to_observers_and_self() {
        use crate::world::tests::{Harness, frames_with_id};
        use mc_bamboo_data::{
            packets::play::clientbound,
            registries::{attribute as attr, mob_effect},
        };

        fn varint(buf: &[u8], at: &mut usize) -> i32 {
            let mut value = 0i32;
            let mut shift = 0;
            loop {
                let byte = buf[*at];
                *at += 1;
                value |= i32::from(byte & 0x7f) << shift;
                if byte & 0x80 == 0 {
                    return value;
                }
                shift += 7;
            }
        }

        let mut server = Harness::new(8);
        let alice = server.join_and_settle("Alice");
        let bob = server.join_and_settle("Bob");
        server.tick();
        server.tick();
        assert!(
            server.region().players[&alice.id]
                .sync
                .seen_by()
                .contains(&bob.id),
            "Bob tracks Alice"
        );
        alice.take_frames();
        bob.take_frames();

        // 稳定一刻：属性没变，观察者和自己都不该再收到属性包。
        server.tick();
        assert!(
            frames_with_id(&bob.take_frames(), clientbound::UPDATE_ATTRIBUTES).is_empty(),
            "no attribute packet on a stable tick"
        );
        assert!(
            frames_with_id(&alice.take_frames(), clientbound::UPDATE_ATTRIBUTES).is_empty(),
            "no attribute packet to self on a stable tick"
        );

        // 给 Alice 速度 II（amplifier=1）。
        {
            let p = server.region().players.get_mut(&alice.id).unwrap();
            p.effects
                .add(EffectInstance::with_amplifier(mob_effect::SPEED, 200, 1));
        }
        server.tick();

        for (who, frames) in [
            ("observer", bob.take_frames()),
            ("self", alice.take_frames()),
        ] {
            let updates = frames_with_id(&frames, clientbound::UPDATE_ATTRIBUTES);
            assert_eq!(
                updates.len(),
                1,
                "{who}: exactly one dirty attribute packet"
            );
            let body = &updates[0].body;
            let mut at = 0;
            assert_eq!(varint(body, &mut at), alice.id.0, "{who}: entity_id");
            let count = varint(body, &mut at);
            let mut found = false;
            for _ in 0..count {
                let id = varint(body, &mut at);
                at += 8; // base f64
                let modifiers = varint(body, &mut at);
                for _ in 0..modifiers {
                    let len = varint(body, &mut at) as usize;
                    let modifier_id = std::str::from_utf8(&body[at..at + len]).unwrap();
                    at += len;
                    let amount = f64::from_be_bytes(body[at..at + 8].try_into().unwrap());
                    at += 8;
                    let operation = varint(body, &mut at);
                    if id == attr::MOVEMENT_SPEED && modifier_id == "minecraft:effect.speed" {
                        found = true;
                        assert_eq!(amount, f64::from(0.2_f32) * 2.0, "{who}: speed II amount");
                        assert_eq!(operation, 2, "{who}: ADD_MULTIPLIED_TOTAL");
                    }
                }
            }
            assert_eq!(at, body.len(), "{who}: no trailing bytes");
            assert!(found, "{who}: MOVEMENT_SPEED got the speed effect modifier");
        }
    }

    /// 所有已接入属性表的子系统都真的读 `player.attributes`（包括自定义基础值和
    /// 永久修饰符），不是写死的常量或只查物品表。含改坏确认。
    ///
    /// Every wired player subsystem reads `player.attributes` (including custom
    /// base values and permanent modifiers) instead of hardcoded constants.
    /// Includes break-it checks.
    #[test]
    fn wired_subsystems_read_player_attributes() {
        use mc_bamboo_blocks::entity::{
            Entity, JavaRandom,
            mob::{DamageKind, DamageSource},
        };
        use mc_bamboo_core::{BlockPos, Vec3};
        use mc_bamboo_data::blocks;

        // 1. 摔落伤害：SAFE_FALL_DISTANCE（含跳跃提升）与 FALL_DAMAGE_MULTIPLIER。
        let mut p = player(GameMode::Survival);
        assert_eq!(crate::player_environment::fall_damage(&p, 6.0), 3);
        p.effects.add(EffectInstance::with_amplifier(
            mob_effect::JUMP_BOOST,
            200,
            1,
        ));
        rebuild_player(&mut p, false);
        assert_eq!(
            p.attributes.value(attribute::SAFE_FALL_DISTANCE),
            5.0,
            "Jump Boost II raises SAFE_FALL_DISTANCE by 2"
        );
        assert_eq!(crate::player_environment::fall_damage(&p, 6.0), 1);
        // 改坏确认：把 FALL_DAMAGE_MULTIPLIER 改成 2.0，摔落伤害翻倍。
        p.attributes
            .instance(attribute::FALL_DAMAGE_MULTIPLIER)
            .unwrap()
            .base = 2.0;
        assert_eq!(crate::player_environment::fall_damage(&p, 6.0), 2);

        // 2. 重力 / 跳跃高度 / 跨步高度读 GRAVITY / JUMP_STRENGTH / STEP_HEIGHT。
        let mut p = player(GameMode::Survival);
        assert_eq!(crate::player_move::maximum_flying_ticks(&p), 80);
        assert_eq!(crate::player_move::step_height(&p), 0.6_f32);
        p.attributes.instance(attribute::GRAVITY).unwrap().base = 0.04;
        assert_eq!(crate::player_move::maximum_flying_ticks(&p), 160);
        p.attributes
            .instance(attribute::JUMP_STRENGTH)
            .unwrap()
            .base = 0.84;
        crate::player_move::jump_from_ground(&mut p);
        assert_eq!(p.motion.delta.y, f64::from(0.84_f32));
        p.attributes.instance(attribute::STEP_HEIGHT).unwrap().base = 1.25;
        assert_eq!(crate::player_move::step_height(&p), 1.25_f32);

        // 3. 触及距离：自定义 BLOCK_INTERACTION_RANGE 修饰符放宽够得着判定。
        let mut p = player(GameMode::Survival);
        let far = BlockPos::new(7, 1, 0);
        assert!(!crate::interaction::within_reach(&p, far));
        p.attributes
            .instance(attribute::BLOCK_INTERACTION_RANGE)
            .unwrap()
            .modifiers
            .push(Modifier {
                id: "minecraft:test_reach".to_owned(),
                amount: 3.0,
                operation: Operation::AddValue,
                permanent: true,
            });
        assert!(crate::interaction::within_reach(&p, far));

        // 4. 最大生命、回血夹取与刷新上限。
        let mut region = Region::new(2, 20, false);
        let pid = PlayerId(6);
        let mut p = Player::new(
            pid,
            Uuid::from_u128(6),
            "H".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::default(),
        );
        p.attributes.instance(attribute::MAX_HEALTH).unwrap().base = 26.0;
        p.attributes
            .instance(attribute::MAX_HEALTH)
            .unwrap()
            .modifiers
            .push(Modifier {
                id: "minecraft:test_hp".to_owned(),
                amount: 4.0,
                operation: Operation::AddValue,
                permanent: true,
            });
        p.health.health = 22.0;
        let max_hp = crate::player_effects::max_health(&p);
        assert_eq!(max_hp, 30.0);
        p.health.heal(10.0, max_hp);
        assert_eq!(p.health.health, 30.0);
        // 改坏确认：移除永久修饰符并刷新上限，血量夹回 26.0。
        p.attributes
            .instance(attribute::MAX_HEALTH)
            .unwrap()
            .remove_modifier("minecraft:test_hp");
        region.insert_player(p);
        crate::player_effects::refresh_limits(&mut region, pid);
        assert_eq!(region.players[&pid].health.health, 26.0);

        // 5. 护甲 / 韧性 / 击退抗性直接读属性表（即使裸体）。
        let mut region = Region::new(2, 20, false);
        let pid = PlayerId(7);
        let mut victim = Player::new(
            pid,
            Uuid::from_u128(7),
            "V".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        victim.attributes.instance(attribute::ARMOR).unwrap().base = 20.0;
        victim
            .attributes
            .instance(attribute::ARMOR_TOUGHNESS)
            .unwrap()
            .base = 12.0;
        victim
            .attributes
            .instance(attribute::KNOCKBACK_RESISTANCE)
            .unwrap()
            .base = 1.0;
        let vel_before = victim.motion.delta;
        region.insert_player(victim);
        let mut source = DamageSource::environment(DamageKind::MobAttack);
        source.position = Some(Vec3::new(2.5, 64.0, 0.5));
        crate::player_hurt::hurt_player(&mut region, pid, source, 10.0, &mut None);
        let hp_armored = region.players[&pid].health.health;
        assert!(
            hp_armored > 15.0,
            "20 armor + 12 toughness reduces 10 damage well below 5, got hp={hp_armored}"
        );
        assert_eq!(
            region.players[&pid].motion.delta, vel_before,
            "1.0 knockback_resistance cancels knockback completely"
        );

        // 6. 氧气加成（OXYGEN_BONUS）按官方 1/(bonus+1) 跳过扣氧；鹦鹉螺之息不扣氧。
        let mut region = Region::new(2, 20, false);
        let pid = PlayerId(8);
        let mut diver = Player::new(
            pid,
            Uuid::from_u128(8),
            "Diver".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, 10.0, 0.5),
        );
        diver.random = JavaRandom::new(42);
        diver
            .attributes
            .instance(attribute::OXYGEN_BONUS)
            .unwrap()
            .base = 9.0;
        region
            .chunks
            .set_block(BlockPos::new(0, 11, 0), blocks::WATER);
        region.insert_player(diver);
        for _ in 0..40 {
            crate::player_environment::tick_environment(&mut region);
        }
        let air_with_bonus = region.players[&pid].environment.air_supply;
        assert!(
            air_with_bonus > 260,
            "OXYGEN_BONUS=9 should preserve most air over 40 ticks, got {air_with_bonus}"
        );
        // 鹦鹉螺之息期间完全不扣氧。
        let air_before_nautilus = region.players[&pid].environment.air_supply;
        region
            .players
            .get_mut(&pid)
            .unwrap()
            .effects
            .add(EffectInstance::with_amplifier(
                mob_effect::BREATH_OF_THE_NAUTILUS,
                200,
                0,
            ));
        for _ in 0..10 {
            crate::player_environment::tick_environment(&mut region);
        }
        assert_eq!(
            region.players[&pid].environment.air_supply, air_before_nautilus,
            "BREATH_OF_THE_NAUTILUS prevents air loss"
        );

        // 7. 攻击伤害与实体触及距离读 ATTACK_DAMAGE / ENTITY_INTERACTION_RANGE。
        let mut region = Region::new(2, 20, false);
        let pid = PlayerId(9);
        let mut attacker = Player::new(
            pid,
            Uuid::from_u128(9),
            "Fighter".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        attacker.attack_strength_ticker = 100;
        attacker
            .attributes
            .instance(attribute::ATTACK_DAMAGE)
            .unwrap()
            .base = 5.0;
        attacker
            .attributes
            .instance(attribute::ENTITY_INTERACTION_RANGE)
            .unwrap()
            .base = 6.0;
        region.insert_player(attacker);
        // 牛站在 5.5 格远：默认生存实体触及 3.0 够不着，改成 6.0 能打到并造成 5 点伤害。
        let cow = region
            .entities
            .spawn(Entity::cow(Vec3::new(6.0, 64.0, 0.5)));
        crate::player_attack::on_attack(&mut region, pid, cow);
        let cow_hp = region
            .entities
            .get(cow)
            .and_then(|e| e.kind.mob().map(|m| m.health))
            .expect("cow alive");
        assert_eq!(cow_hp, 5.0, "custom ATTACK_DAMAGE=5 hits cow at 5.5 blocks");
        // 改坏确认：把 ENTITY_INTERACTION_RANGE 改回 3.0，同一位置的牛够不着。
        region
            .players
            .get_mut(&pid)
            .unwrap()
            .attributes
            .instance(attribute::ENTITY_INTERACTION_RANGE)
            .unwrap()
            .base = 3.0;
        region.players.get_mut(&pid).unwrap().attack_strength_ticker = 100;
        region.entities.with_mut(cow, |e| {
            if let Some(m) = e.kind.mob_mut() {
                m.invulnerable_time = 0;
            }
        });
        crate::player_attack::on_attack(&mut region, pid, cow);
        let cow_hp_after = region
            .entities
            .get(cow)
            .and_then(|e| e.kind.mob().map(|m| m.health))
            .expect("cow alive");
        assert_eq!(cow_hp_after, 5.0, "out-of-reach attack must not land");
    }
}
