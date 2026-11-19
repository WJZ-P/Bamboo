//! 玩家死亡：掉落、经验球、死亡消息和死亡界面。
//!
//! 官方 `ServerPlayer.die` → `LivingEntity.dropAllDeathLoot` → `Player.dropEquipment`
//! → `Inventory.dropAll` → `ExperienceOrb.award`。Bamboo 不重建玩家对象，
//! 状态重置集中在 [`crate::player_respawn`]，这里只做「死的那一刻」。
//!
//! Player death: drops, experience orbs, the death message and the death screen.
//! Vanilla `ServerPlayer.die` → `LivingEntity.dropAllDeathLoot` →
//! `Player.dropEquipment` → `Inventory.dropAll` → `ExperienceOrb.award`.
//! Bamboo does not rebuild the player; the state reset lives in
//! [`crate::player_respawn`].

use mc_bamboo_blocks::{World, entity::mob::DamageSource};
use mc_bamboo_core::{BlockPos, Hand, Vec3};
use mc_bamboo_data::gamerules;
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, defaults},
    item::ItemStack,
    packet::play::{EntityEvent, GameMode, PlayerCombatKill, SystemChat},
    text::{Component, TranslateArg},
};

use crate::{inbound::PlayerId, links, region::Region};

/// `minecraft:vanishing_curse` 在附魔注册表里的 ID。
const VANISHING_CURSE: i32 = 41;

/// `CombatTracker` 一条战斗记录里死亡消息用得到的部分。
/// 官方存的是 `DamageSource` 和 `tickCount`（绝对时刻）；这里只存
/// 「最后一次致死伤害」的种类和攻击者显示组件，按官方的
/// `getDeathMessage` 展开翻译键。不存岛时钟的绝对时刻，拆岛 / 并岛不用平移。
///
/// The part of one official `CombatEntry` needed by the death message. Vanilla
/// stores the source and an absolute tick; this holds only the killing blow's
/// kind and attacker component, with no absolute instant.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CombatEntry {
    pub kind: mc_bamboo_blocks::entity::mob::DamageKind,
    pub attacker: Option<Component>,
    pub weapon: Option<Component>,
}

impl CombatEntry {
    /// 官方 `CombatTracker.getDeathMessage` 的最后一段：
    /// `DamageSource.getLocalizedDeathMessage`。
    ///
    /// 有攻击者实体时是 `death.attack.<msgId>` 加参数（武器有 custom_name
    /// 时改成 `.item` 再加一个参数）；没有攻击者时只有死者一个参数。
    /// `CombatTracker` 没有记录时是 `death.attack.generic`。
    ///
    /// Official `CombatTracker.getDeathMessage`: the final call to
    /// `DamageSource.getLocalizedDeathMessage`. With an attacker the key is
    /// `death.attack.<msgId>` (or `.item` for a named weapon). An empty
    /// tracker uses `death.attack.generic`.
    pub fn message(&self, victim: &str, uuid: [u8; 16]) -> Component {
        // 官方 `Player.getDisplayName()` 带 hover/show_entity、click/suggest_command
        // 和 insertion，死亡消息和 PlayerCombatKill 都用同一个组件。
        let name = || victim_component(victim, uuid);
        let msg = self.kind.message_id();
        let (key, args): (String, Vec<TranslateArg>) = match (&self.attacker, &self.weapon) {
            (Some(attacker), Some(weapon)) => (
                format!("death.attack.{msg}.item"),
                vec![
                    name().into(),
                    TranslateArg::from(attacker.clone()),
                    TranslateArg::from(weapon.clone()),
                ],
            ),
            (Some(attacker), None) => (
                format!("death.attack.{msg}"),
                vec![name().into(), TranslateArg::from(attacker.clone())],
            ),
            // 官方在没有攻击者实体且 `getKillCredit()` 为空时写
            // `death.attack.<msgId>` 一个参数。
            (None, _) => (format!("death.attack.{msg}"), vec![name().into()]),
        };
        // 官方 `MutableComponent` 的键序：`with` 在 `translate` 前面。
        Component::trusted(mc_bamboo_nbt::Tag::Compound(vec![
            (
                "with".to_owned(),
                mc_bamboo_nbt::Tag::List(args.into_iter().map(TranslateArg::into_tag).collect()),
            ),
            ("translate".to_owned(), mc_bamboo_nbt::Tag::String(key)),
        ]))
    }
}

/// 官方 `Entity.getDisplayName()` 对玩家的结果：`Component.translatable`
/// 之外的富文本部分（点击建议 `/tell <name> `、悬停 show_entity、insertion）。
///
/// Vanilla `Entity.getDisplayName()` for a player: the rich-text fields the
/// death message and `player_combat_kill` carry.
fn victim_component(name: &str, uuid: [u8; 16]) -> Component {
    use mc_bamboo_nbt::Tag;
    let ints: Vec<i32> = (0..4)
        .map(|index| {
            i32::from_be_bytes(uuid[index * 4..index * 4 + 4].try_into().unwrap_or([0; 4]))
        })
        .collect();
    let hover = Tag::Compound(vec![
        ("name".to_owned(), Tag::String(name.to_owned())),
        ("action".to_owned(), Tag::String("show_entity".to_owned())),
        ("id".to_owned(), Tag::String("minecraft:player".to_owned())),
        ("uuid".to_owned(), Tag::IntArray(ints)),
    ]);
    let click = Tag::Compound(vec![
        (
            "action".to_owned(),
            Tag::String("suggest_command".to_owned()),
        ),
        ("command".to_owned(), Tag::String(format!("/tell {name} "))),
    ]);
    // 官方 `Component` 的键序：click_event、insertion、text、hover_event。
    Component::trusted(Tag::Compound(vec![
        ("click_event".to_owned(), click),
        ("insertion".to_owned(), Tag::String(name.to_owned())),
        ("text".to_owned(), Tag::String(name.to_owned())),
        ("hover_event".to_owned(), hover),
    ]))
}

/// 血量归零：走一次完整的官方死亡流程。
/// 只有 `!dead` 且血量不大于 0 时才有作用；由 `player_hurt::hurt_player`
/// 和饥饿伤害在造成伤害后调用。
///
/// Runs the official death flow once after health reached zero. Called by the
/// hurt path and by starvation damage.
pub(crate) fn die(region: &mut Region, id: PlayerId, source: DamageSource) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if player.dead || player.health.health > 0.0 {
        return;
    }
    let position = player.position();
    let name = player.name.clone();
    let uuid = *player.uuid.as_bytes();
    let in_combat = player.in_combat;
    let keep_inventory = region.global.rules.bool(gamerules::KEEP_INVENTORY);
    let show_messages = region.global.rules.bool(gamerules::SHOW_DEATH_MESSAGES);
    let spectator = player.game_mode == GameMode::Spectator;
    let level = player.experience.level;

    // `CombatTracker.recordDamage` 只在 `amount != 0` 的完整受击里写。
    // 死亡消息取最后一条记录；攻击者显示名按实体表 / 玩家表查。
    let entry = CombatEntry {
        kind: source.kind,
        attacker: attacker_name(region, &source),
        weapon: named_weapon(region, &source),
    };
    // `ServerPlayer.die`：show_death_messages 决定是否构造 / 广播消息。
    let death_message = if show_messages {
        entry.message(&name, uuid)
    } else {
        Component::text(String::new())
    };

    // 官方在广播死亡消息之前先给死者发 player_combat_kill。
    region.links.send(
        id,
        &PlayerCombatKill {
            player_id: id.0,
            message: &death_message,
        },
    );
    if show_messages {
        let frames = links::encode_frames(&SystemChat {
            content: &death_message,
            overlay: false,
        });
        crate::chat::broadcast_system_message(region, &frames, None);
    }

    // `Player.dropEquipment`：!keepInventory 时先清消失诅咒，再 dropAll。
    // `LivingEntity.dropAllDeathLoot` 里 `dropExperience` 紧跟在 dropEquipment 后面。
    let mut drops = Vec::new();
    if !keep_inventory {
        let player = region.players.get_mut(&id).expect("玩家刚才还在");
        for slot in &mut player.inventory.slots {
            if slot.is_empty() {
                continue;
            }
            if has_vanishing_curse(slot) {
                *slot = ItemStack::empty();
                continue;
            }
            drops.push(slot.clone());
            *slot = ItemStack::empty();
        }
    }
    let reward = if keep_inventory || spectator {
        0
    } else {
        (level.saturating_mul(7)).min(100)
    };
    {
        let (mut block_level, ticks) = region.block_level();
        let mut world = World::new(&mut block_level, ticks);
        // `LivingEntity.createItemStackToDrop`：眼睛高度 -0.3，拾取延迟 40。
        let at = Vec3::new(
            position.x,
            position.y + 1.62 - 0.300_000_011_920_928_96,
            position.z,
        );
        for stack in drops {
            let Some(entity_id) = world.spawn_at_location(at, stack) else {
                continue;
            };
            world.with_entity_mut(entity_id, &mut |entity| {
                if let Some(item) = entity.ground_mut() {
                    item.set_pickup_delay(40);
                }
            });
        }
        if reward > 0 {
            mc_bamboo_blocks::entity::award(&mut world, position, reward);
        }
    }
    // 官方 `ServerPlayer.die` 不清经验；等级经验在下一刻 `PlayerList.respawn`
    // 的 `restoreFrom` 里丢掉（keepInventory 时原样留下）。Bamboo 原地重置，
    // 所以到重生那一刻才按 `reset_after_respawn` 清。
    let player = region.players.get_mut(&id).expect("玩家刚才还在");
    player.dead = true;
    player.health.death_time = 0;
    player.last_combat = Some(entry);
    player.last_death_location = Some(BlockPos::containing(position));
    player.in_combat = false;
    // `Player.dropEquipment` / 重生后的 `transferInventoryXpAndScore` 只在
    // `keepInventory` 为假时把经验清成 0，官方读盘状态也这样。
    player.sync.metadata_dirty = true;

    // `LivingEntity.die` 的 `broadcastEntityEvent(entity, 3)`：死亡动画给跟踪者。
    broadcast_death_event(region, id);

    // `ServerPlayer.die` 末尾 `combatTracker.recheckStatus` 在死亡时若还在
    // 战斗里就 `onLeaveCombat`，给死者发 player_combat_end。
    if in_combat {
        region.links.send(
            id,
            &mc_bamboo_protocol::packet::play::PlayerCombatEnd { duration: 0 },
        );
    }

    // `immediate_respawn` 为真时在死亡这一刻直接重生；否则等客户端
    // `PERFORM_RESPAWN`（死亡界面）。
    if region.global.rules.bool(gamerules::IMMEDIATE_RESPAWN) {
        crate::player_respawn::respawn(region, id);
    } else {
        let player = region.players.get_mut(&id).expect("玩家刚才还在");
        player.awaiting_respawn = true;
    }
}

/// `EnchantmentHelper.has(stack, PREVENT_EQUIPMENT_DROP)`：
/// 26.3 里只有 `minecraft:vanishing_curse` 带这个效果。
fn has_vanishing_curse(stack: &ItemStack) -> bool {
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return false;
    };
    let Some(ComponentValue::Enchantments(levels)) = defaults::resolve(
        stack.item,
        &patch,
        mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
    ) else {
        return false;
    };
    levels
        .iter()
        .any(|(id, level)| *id == VANISHING_CURSE && *level > 0)
}

/// `DamageSource.getEntity().getDisplayName()`：玩家用名字，实体用自定义名或
/// `entity.minecraft.<type>` 翻译组件。
fn attacker_name(region: &Region, source: &DamageSource) -> Option<Component> {
    use mc_bamboo_nbt::Tag;
    let uid = source.attacker.or(source.direct)?;
    if let Some(player) = region.players.get(&PlayerId(uid)) {
        return Some(victim_component(&player.name, *player.uuid.as_bytes()));
    }
    let public = region.entities.public_at(uid)?;
    if let mc_bamboo_blocks::entity::KindPublic::Mob(mob) = &public.kind
        && let Some(tag) = &mob.custom_name
    {
        return Some(Component::trusted(tag.clone()));
    }
    // 官方 `Entity.getDisplayName()` 对生物给的富文本组件：
    // insertion 是 UUID 文本，hover 里 name 又是同一段翻译组件。
    let key = mc_bamboo_data::registries::entity_type::NAMES
        .get(public.kind.entity_type() as usize)
        .map(|name| name.replace("minecraft:", "entity.minecraft."))
        .unwrap_or_else(|| "entity.minecraft.cow".to_owned());
    let entity_id = mc_bamboo_data::registries::entity_type::NAMES
        .get(public.kind.entity_type() as usize)
        .copied()
        .unwrap_or("minecraft:cow");
    let uuid = match &public.kind {
        mc_bamboo_blocks::entity::KindPublic::Mob(mob) => mob.uuid,
        _ => [0; 16],
    };
    let ints: Vec<i32> = (0..4)
        .map(|index| {
            i32::from_be_bytes(uuid[index * 4..index * 4 + 4].try_into().unwrap_or([0; 4]))
        })
        .collect();
    let text = uuid_text(&uuid);
    let inner = Tag::Compound(vec![("translate".to_owned(), Tag::String(key.clone()))]);
    let hover = Tag::Compound(vec![
        ("name".to_owned(), inner.clone()),
        ("action".to_owned(), Tag::String("show_entity".to_owned())),
        ("id".to_owned(), Tag::String(entity_id.to_owned())),
        ("uuid".to_owned(), Tag::IntArray(ints)),
    ]);
    Some(Component::trusted(Tag::Compound(vec![
        ("insertion".to_owned(), Tag::String(text)),
        ("hover_event".to_owned(), hover),
        ("translate".to_owned(), Tag::String(key)),
    ])))
}

/// `UUID.toString()`，官方死亡消息里生物的 `insertion`。
fn uuid_text(bytes: &[u8; 16]) -> String {
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// `getLocalizedDeathMessage` 里攻击者主手物品的 `custom_name`。
fn named_weapon(region: &Region, source: &DamageSource) -> Option<Component> {
    let uid = source.attacker.or(source.direct)?;
    let player = region.players.get(&PlayerId(uid))?;
    let stack = player.inventory.held(Hand::Main);
    if stack.is_empty() {
        return None;
    }
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return None;
    };
    let Some(ComponentValue::CustomName(name)) = defaults::resolve(
        stack.item,
        &patch,
        mc_bamboo_data::registries::data_component_type::CUSTOM_NAME,
    ) else {
        return None;
    };
    Some(name.clone())
}

/// `ServerLevel.broadcastEntityEvent` 的死亡动画：发给所有观察者（含自己）。
/// 官方经 `ChunkMap.sendToTrackingPlayersAndSelf` 广播，Bamboo 的 `Links::broadcast`
/// 走同一条全服广播路径。
///
/// Official `broadcastEntityEvent`: the death animation goes to every tracker
/// including the player.
pub(crate) fn broadcast_death_event(region: &mut Region, id: PlayerId) {
    let frames = links::encode_frames(&EntityEvent {
        entity_id: id.0,
        event: 3,
    });
    region.links.broadcast(&frames);
}

/// 装备槽里带消失诅咒的一件物品（对拍和单测注入用）。
#[allow(dead_code)]
#[cfg(any(test, feature = "oracle-samples"))]
pub(crate) fn vanishing_stack(item: i32) -> ItemStack {
    let mut stack = ItemStack::new(item, 1);
    let mut bytes = Vec::new();
    mc_bamboo_protocol::codec::write_varint(&mut bytes, 1);
    mc_bamboo_protocol::codec::write_varint(&mut bytes, VANISHING_CURSE);
    mc_bamboo_protocol::codec::write_varint(&mut bytes, 1);
    stack.added.push((
        mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
        bytes,
    ));
    stack
}
