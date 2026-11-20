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

use mc_bamboo_blocks::entity::mob::{DamageKind, DamageSource, DeathMessageType};
use mc_bamboo_core::{BlockPos, Hand};
use mc_bamboo_data::gamerules;
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, defaults},
    item::ItemStack,
    packet::play::{EntityEvent, GameMode, PlayerCombatEnd, PlayerCombatKill, SystemChat},
    text::{Component, TranslateArg},
};

use std::collections::BTreeMap;

use mc_bamboo_blocks::{PlayerDeathDrops, entity::Entities};

use crate::{inbound::PlayerId, links, player::Player, player_hurt::HurtParts, region::Region};

/// `minecraft:vanishing_curse` 在附魔注册表里的 ID。
const VANISHING_CURSE: i32 = 41;
/// 官方 `setLastHurtByPlayer(..., 100)` 与 `lastHurtByMobTimestamp` 的 100 刻过期。
pub(crate) const KILL_CREDIT_TICKS: i32 = 100;
/// 对拍改坏用：只影响 `replay_player_life` 这一次重放。
#[cfg(feature = "oracle-samples")]
static KILL_CREDIT_OVERRIDE: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(KILL_CREDIT_TICKS);

#[cfg(feature = "oracle-samples")]
pub(crate) fn set_kill_credit_ticks_for_oracle(value: i32) {
    KILL_CREDIT_OVERRIDE.store(value, std::sync::atomic::Ordering::Relaxed);
}

/// 本次重放用的归属过期时间。
pub(crate) fn kill_credit_ticks() -> i32 {
    #[cfg(feature = "oracle-samples")]
    {
        let value = KILL_CREDIT_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed);
        if value != KILL_CREDIT_TICKS {
            return value;
        }
    }
    KILL_CREDIT_TICKS
}
/// 官方 `CombatTracker.recheckStatus` 的空闲阈值（战斗中是 300 刻）。
pub(crate) const COMBAT_IDLE_IN: i32 = 300;
pub(crate) const COMBAT_IDLE_OUT: i32 = 100;

/// 官方 `CombatEntry` 里死亡消息用得到的部分。
/// 官方存 `DamageSource` 和绝对 `tickCount`；这里只存种类、显示组件、
/// 摔落距离与摔落位置 ID，全部不是绝对时刻，岛拆合时整行跟着 `Players` 搬。
///
/// The part of an official `CombatEntry` needed by the death message. Vanilla
/// stores the source and an absolute tick; this holds the kind, display
/// components, fall distance and fall-location id, with no absolute instants.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CombatEntry {
    pub kind: DamageKind,
    pub attacker: Option<Component>,
    pub weapon: Option<Component>,
    /// `CombatEntry.damage()`，用于 `getMostSignificantFall` 的第二判据。
    pub damage: f32,
    /// `LivingEntity.fallDistance` 在 `recordDamage` 那一刻的值。
    pub fall_distance: f32,
    /// `FallLocation.id()`，没有攀爬物 / 不在水里时是 `None`。
    pub fall_location: Option<&'static str>,
}

/// 官方 `LivingEntity.getKillCredit()` 返回的那名实体的显示组件。
/// `memory` / `age` 是相对倒计时，不存绝对时刻。
///
/// The display component of the entity `LivingEntity.getKillCredit()` returns.
/// `memory` / `age` are relative countdowns, not absolute instants.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum KillCredit {
    /// `lastHurtByPlayer`，`memory` 每刻减 1，到 0 清空。
    Player { display: Component, memory: i32 },
    /// `lastHurtByMob`，`age` 每刻加 1，超过 100 清空。
    Mob { display: Component, age: i32 },
}

impl KillCredit {
    pub(crate) fn display(&self) -> &Component {
        match self {
            Self::Player { display, .. } | Self::Mob { display, .. } => display,
        }
    }

    /// 官方 `LivingEntity.baseTick`：
    /// `lastHurtByPlayerMemoryTime > 0` 时减一，否则清空引用；
    /// `lastHurtByMob` 在 `tickCount - timestamp > 100` 时清空。
    pub(crate) fn tick(&mut self) -> bool {
        match self {
            Self::Player { memory, .. } => {
                if *memory > 0 {
                    *memory -= 1;
                    true
                } else {
                    false
                }
            }
            // `age` 是 `tickCount - lastHurtByMobTimestamp` 的相对计数；
            // 边界（101/102 刻）由对拍场景逐字节核对。
            Self::Mob { age, .. } => {
                *age += 1;
                *age <= kill_credit_ticks()
            }
        }
    }
}

/// `CombatTracker.getDeathMessage` 的完整分支：
/// `FALL_VARIANTS` 走 `getMostSignificantFall` / `getFallMessage`，
/// `INTENTIONAL_GAME_DESIGN` 走带链接的那条，其余 `getLocalizedDeathMessage`。
pub(crate) fn death_message(
    entries: &[CombatEntry],
    victim: &str,
    uuid: [u8; 16],
    kill_credit: Option<&Component>,
) -> Component {
    let name = || victim_component(victim, uuid);
    // 官方 `entries.isEmpty()` → `death.attack.generic` 单参数。
    let Some(last) = entries.last() else {
        return translation("death.attack.generic", vec![name().into()]);
    };
    let msg = last.kind.message_id();
    match last.kind.death_message_type() {
        DeathMessageType::FallVariants => {
            if let Some(fall) = most_significant_fall(entries) {
                return fall_message(fall, last, victim, uuid);
            }
        }
        DeathMessageType::IntentionalGameDesign => {
            return intentional_game_design(msg, victim, uuid);
        }
        DeathMessageType::Default => {}
    }
    localized_death_message(last, msg, victim, uuid, kill_credit)
}

/// 官方 `DamageSource.getLocalizedDeathMessage`。
/// 有攻击者实体时用 `death.attack.<msgId>`（命名武器是 `.item`）；
/// 没有攻击者实体时看 `getKillCredit()`：有归属用
/// `death.attack.<msgId>.player`，没有就用单参数。
fn localized_death_message(
    last: &CombatEntry,
    msg: &str,
    victim: &str,
    uuid: [u8; 16],
    kill_credit: Option<&Component>,
) -> Component {
    let name = || victim_component(victim, uuid);
    let (key, args): (String, Vec<TranslateArg>) = match (&last.attacker, &last.weapon) {
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
        (None, _) => match kill_credit {
            Some(credit) => (
                format!("death.attack.{msg}.player"),
                vec![name().into(), TranslateArg::from(credit.clone())],
            ),
            None => (format!("death.attack.{msg}"), vec![name().into()]),
        },
    };
    translation(&key, args)
}

/// 官方 `CombatTracker.getMostSignificantFall`：按摔落距离（>5）选最显著的一次，
/// 否则按 `fallLocation` 非空且 damage 最大的那次（damage 也要 >5）。
/// `ALWAYS_MOST_SIGNIFICANT_FALL` 是 `Float.MAX`，26.3 的建模种类里没有。
fn most_significant_fall(entries: &[CombatEntry]) -> Option<&CombatEntry> {
    let mut by_fall: Option<(&CombatEntry, f32)> = None;
    let mut by_damage: Option<(&CombatEntry, f32)> = None;
    for (index, entry) in entries.iter().enumerate() {
        if entry.kind.is_fall() {
            let distance = entry.fall_distance;
            if distance > 0.0 && by_fall.is_none_or(|(_, best)| distance > best) {
                // 官方用「上一条」；第一条用自己。
                by_fall = Some((
                    if index > 0 {
                        &entries[index - 1]
                    } else {
                        entry
                    },
                    distance,
                ));
            }
        }
        if entry.fall_location.is_some() && by_damage.is_none_or(|(_, best)| entry.damage > best) {
            by_damage = Some((entry, entry.damage));
        }
    }
    if let Some((entry, distance)) = by_fall
        && distance > 5.0
    {
        return Some(entry);
    }
    if let Some((entry, damage)) = by_damage
        && damage > 5.0
    {
        return Some(entry);
    }
    None
}

/// 官方 `CombatTracker.getFallMessage`。
/// `IS_FALL` 的条目走 `FallLocation.languageKey()`（`death.fell.accident.<id>`）；
/// 否则按 `getDeathMessage` 传入的致死来源实体和条目的攻击者实体拼
/// `death.fell.assist` / `finish` / `killer`。
fn fall_message(
    entry: &CombatEntry,
    last: &CombatEntry,
    victim: &str,
    uuid: [u8; 16],
) -> Component {
    let name = || victim_component(victim, uuid);
    if entry.kind.is_fall() {
        let id = entry.fall_location.unwrap_or("generic");
        return translation(&format!("death.fell.accident.{id}"), vec![name().into()]);
    }
    // `getDeathMessage` 传的是最后一条 `DamageSource.getEntity()`。
    let death_display = last.attacker.clone();
    let entry_display = entry.attacker.clone();
    let differs = entry_display
        .as_ref()
        .is_some_and(|entry| death_display.as_ref() != Some(entry));
    if differs {
        // `getMessageForAssistedFall(entry 的攻击者, ...)`。
        return assisted_fall(
            entry_display.as_ref().expect("上面判过非空"),
            entry.weapon.as_ref(),
            "death.fell.assist.item",
            "death.fell.assist",
            victim,
            uuid,
        );
    }
    if let Some(display) = death_display {
        return assisted_fall(
            &display,
            last.weapon.as_ref(),
            "death.fell.finish.item",
            "death.fell.finish",
            victim,
            uuid,
        );
    }
    translation("death.fell.killer", vec![name().into()])
}

/// `getMessageForAssistedFall`：主手物品有 `custom_name` 时用 `.item` 键并多带一个参数。
fn assisted_fall(
    display: &Component,
    weapon: Option<&Component>,
    item_key: &str,
    plain_key: &str,
    victim: &str,
    uuid: [u8; 16],
) -> Component {
    let mut args = vec![
        victim_component(victim, uuid).into(),
        TranslateArg::from(display.clone()),
    ];
    if let Some(weapon) = weapon {
        args.push(TranslateArg::from(weapon.clone()));
        translation(item_key, args)
    } else {
        translation(plain_key, args)
    }
}

/// 官方 `INTENTIONAL_GAME_DESIGN` 分支：`[<death.attack.<msg>.link>]`
/// 带 MCPE-28723 链接样式，外层是 `death.attack.<msg>.message`。
/// 26.3 建模的伤害种类里触发不到，分支按字节码留着。
fn intentional_game_design(msg: &str, victim: &str, uuid: [u8; 16]) -> Component {
    use mc_bamboo_nbt::Tag;
    let link = format!("death.attack.{msg}.link");
    let bracketed = Component::translate(
        "chat.square_brackets",
        [TranslateArg::from(
            Component::translate(&link, [])
                .with_entry(
                    "click_event",
                    Tag::Compound(vec![
                        ("action".to_owned(), Tag::String("open_url".to_owned())),
                        (
                            "value".to_owned(),
                            Tag::String("https://bugs.mojang.com/browse/MCPE-28723".to_owned()),
                        ),
                    ]),
                )
                .with_entry("hover_event", hover_show_text("MCPE-28723"))
                .with_entry("italic", Tag::Byte(1)),
        )],
    );
    translation(
        &format!("death.attack.{msg}.message"),
        vec![
            victim_component(victim, uuid).into(),
            TranslateArg::from(bracketed),
        ],
    )
}

fn hover_show_text(text: &str) -> mc_bamboo_nbt::Tag {
    use mc_bamboo_nbt::Tag;
    Tag::Compound(vec![
        ("action".to_owned(), Tag::String("show_text".to_owned())),
        ("value".to_owned(), Tag::String(text.to_owned())),
    ])
}

/// `Component.translatable(key, args)` 的线上键序：`with` 在 `translate` 前。
fn translation(key: &str, args: Vec<TranslateArg>) -> Component {
    use mc_bamboo_nbt::Tag;
    Component::trusted(Tag::Compound(vec![
        (
            "with".to_owned(),
            Tag::List(args.into_iter().map(TranslateArg::into_tag).collect()),
        ),
        ("translate".to_owned(), Tag::String(key.to_owned())),
    ]))
}

/// 官方 `Entity.getDisplayName()` 对玩家的结果：`Component.translatable`
/// 之外的富文本部分（点击建议 `/tell <name> `、悬停 show_entity、insertion）。
///
/// Vanilla `Entity.getDisplayName()` for a player: the rich-text fields the
/// death message and `player_combat_kill` carry.
pub(crate) fn victim_component(name: &str, uuid: [u8; 16]) -> Component {
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
/// 只有 `!dead` 且血量不大于 0 时才有作用；由 `player_hurt::hurt_player_in` 在造成伤害后调用。
/// 返回要在世界里生成的掉落（物品和经验）：生成不发包，交给调用方在这之后做
/// （`World::spawn_player_death_drops`）。
///
/// Runs the official death flow once after health reached zero and returns the
/// drops for the caller to spawn.
pub(crate) fn die_in(
    parts: &mut HurtParts<'_>,
    id: PlayerId,
    _source: DamageSource,
) -> Option<PlayerDeathDrops> {
    let player = parts.players.get(&id)?;
    if player.dead || player.health.health > 0.0 {
        return None;
    }
    let position = player.position();
    let name = player.name.clone();
    let uuid = *player.uuid.as_bytes();
    let in_combat = player.in_combat;
    let keep_inventory = parts.rules.bool(gamerules::KEEP_INVENTORY);
    let show_messages = parts.rules.bool(gamerules::SHOW_DEATH_MESSAGES);
    let spectator = player.game_mode == GameMode::Spectator;
    let level = player.experience.level;
    // `getKillCredit()`：优先 `lastHurtByPlayer`，否则 `lastHurtByMob`。
    let kill_credit = player
        .hurt_by_player
        .as_ref()
        .or(player.hurt_by_mob.as_ref())
        .map(|credit| credit.display().clone());

    // 死亡消息先用完整的 `CombatTracker.entries` 拼（官方在 `recheckStatus`
    // 清空之前调 `getDeathMessage`）。
    let death_message = if show_messages {
        death_message(&player.combat_entries, &name, uuid, kill_credit.as_ref())
    } else {
        Component::text(String::new())
    };

    // 官方在广播死亡消息之前先给死者发 player_combat_kill。
    parts.links.send(
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
        crate::chat::broadcast_system_frames(parts.players, parts.links, &frames, None);
    }

    // `Player.dropEquipment`：!keepInventory 时先清消失诅咒，再 dropAll。
    // `LivingEntity.dropAllDeathLoot` 里 `dropExperience` 紧跟在 dropEquipment 后面。
    let mut drops = Vec::new();
    if !keep_inventory {
        let player = parts.players.get_mut(&id).expect("玩家刚才还在");
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
    // 生成掉落交给调用方：`LivingEntity.createItemStackToDrop` 的眼高 -0.3、
    // 拾取延迟 40 和经验球拆分都在 `World::spawn_player_death_drops` 里。
    let drops = PlayerDeathDrops {
        position,
        items: drops,
        experience: reward,
    };
    // 官方 `ServerPlayer.die` 不清经验；等级经验在下一刻 `PlayerList.respawn`
    // 的 `restoreFrom` 里丢掉（keepInventory 时原样留下）。Bamboo 原地重置，
    // 所以到重生那一刻才按 `reset_after_respawn` 清。
    let player = parts.players.get_mut(&id).expect("玩家刚才还在");
    player.dead = true;
    player.health.death_time = 0;
    player.last_death_location = Some(BlockPos::containing(position));
    player.in_combat = false;
    // `ServerPlayer.die` 末尾 `combatTracker.recheckStatus` 清空 entries。
    player.combat_entries.clear();
    player.combat_idle_ticks = 0;
    player.sync.metadata_dirty = true;

    // `LivingEntity.die` 的 `broadcastEntityEvent(entity, 3)`：死亡动画给跟踪者。
    broadcast_death_event(parts.links, id);

    // `combatTracker.recheckStatus` 在死亡时若还在战斗里就 `onLeaveCombat`。
    if in_combat {
        parts.links.send(id, &PlayerCombatEnd { duration: 0 });
    }

    // 官方 `ServerPlayer` 不读 `IMMEDIATE_RESPAWN`：规则只由 `PlayerList`（进服）
    // 和 `MinecraftServer`（规则变化）发给客户端，客户端收到死亡界面包后决定
    // 要不要立刻发 `PERFORM_RESPAWN`。服务端一律等这个包，血量 > 0 时忽略它。
    // 客户端不发，玩家就一直是死的。
    //
    // Vanilla `ServerPlayer` never reads `IMMEDIATE_RESPAWN`: the rule reaches
    // the client only through `PlayerList` (on join) and `MinecraftServer`
    // (on rule changes). The client decides whether to send
    // `PERFORM_RESPAWN` right after the death-screen packet; the server always
    // waits for it and ignores it while health > 0. Without a client packet
    // the player stays dead.
    let player = parts.players.get_mut(&id).expect("玩家刚才还在");
    player.awaiting_respawn = true;
    Some(drops)
}

/// `CombatTracker.recordDamage`：一次 `amount != 0` 的完整受击写一条记录。
/// `fallDistance` 取玩家当时的 `Motion.fall_distance`（官方 `LivingEntity.fallDistance`），
/// `fallLocation` 由 `FallLocation.getCurrentFallLocation` 给，Bamboo 这一期
/// 还没有攀爬物 / 水中判定，恒 `None`。
pub(crate) fn record_combat(
    parts: &mut HurtParts<'_>,
    id: PlayerId,
    source: &DamageSource,
    damage: f32,
) {
    // 先只读地把显示组件查出来，再借 `players` 写状态。
    let attacker = attacker_name(parts.players, parts.entities, parts.attacker, source);
    let weapon = named_weapon(parts.players, source);
    let fall_distance = parts
        .players
        .get(&id)
        .map(|player| player.motion.fall_distance as f32)
        .unwrap_or(0.0);
    let uid = source.attacker.or(source.direct);
    let player_credit = uid
        .and_then(|uid| parts.players.get(&PlayerId(uid)))
        .map(|other| victim_component(&other.name, *other.uuid.as_bytes()));
    let mob_credit = uid
        .and_then(|uid| {
            parts
                .attacker
                .filter(|public| public.uid == uid)
                .or_else(|| parts.entities.public_at(uid))
        })
        .and_then(|public| {
            matches!(public.kind, mc_bamboo_blocks::entity::KindPublic::Mob(_))
                .then(|| attacker.clone())
                .flatten()
        });
    let Some(player) = parts.players.get_mut(&id) else {
        return;
    };
    player.combat_entries.push(CombatEntry {
        kind: source.kind,
        attacker,
        weapon,
        damage,
        fall_distance,
        fall_location: None,
    });
    // 相对倒计时，官方 `CombatTracker.lastDamageTime` 的对应量；
    // 边界（101/102 刻）由 `player_death_credit_boundary_*` 场景逐字节核对。
    player.combat_idle_ticks = 0;
    // `LivingEntity.hurtServer` 的 `resolveMobResponsibleForDamage` /
    // `resolvePlayerResponsibleForDamage`：只在 `getEntity()` 非空时设置归属。
    if let Some(display) = player_credit {
        player.hurt_by_player = Some(KillCredit::Player {
            display,
            memory: KILL_CREDIT_TICKS,
        });
    } else if let Some(display) = mob_credit {
        player.hurt_by_mob = Some(KillCredit::Mob { display, age: 0 });
    }
}

/// `LivingEntity.baseTick` 的归属过期与 `CombatTracker.recheckStatus`。
/// 全部用相对倒计时，不存绝对时刻。
pub(crate) fn tick_combat(region: &mut Region) {
    for player in region.players.values_mut() {
        if let Some(credit) = player.hurt_by_player.as_mut()
            && !credit.tick()
        {
            player.hurt_by_player = None;
        }
        if let Some(credit) = player.hurt_by_mob.as_mut()
            && !credit.tick()
        {
            player.hurt_by_mob = None;
        }
        if !player.combat_entries.is_empty() {
            player.combat_idle_ticks += 1;
            let threshold = if player.in_combat {
                COMBAT_IDLE_IN
            } else {
                COMBAT_IDLE_OUT
            };
            if player.combat_idle_ticks > threshold {
                player.combat_entries.clear();
                player.combat_idle_ticks = 0;
            }
        }
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
fn attacker_name(
    players: &BTreeMap<PlayerId, Player>,
    entities: &Entities,
    ticking: Option<&mc_bamboo_blocks::entity::EntityPublic>,
    source: &DamageSource,
) -> Option<Component> {
    use mc_bamboo_nbt::Tag;
    let uid = source.attacker.or(source.direct)?;
    if let Some(player) = players.get(&PlayerId(uid)) {
        return Some(victim_component(&player.name, *player.uuid.as_bytes()));
    }
    let public = ticking
        .filter(|public| public.uid == uid)
        .or_else(|| entities.public_at(uid))?;
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
fn named_weapon(players: &BTreeMap<PlayerId, Player>, source: &DamageSource) -> Option<Component> {
    let uid = source.attacker.or(source.direct)?;
    let player = players.get(&PlayerId(uid))?;
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
pub(crate) fn broadcast_death_event(links: &mut links::Links, id: PlayerId) {
    let frames = links::encode_frames(&EntityEvent {
        entity_id: id.0,
        event: 3,
    });
    links.broadcast(&frames);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_credit_is_kept_for_one_hundred_ticks() {
        let display = Component::text("Cow");
        let mut player = KillCredit::Player {
            display: display.clone(),
            memory: 100,
        };
        for _ in 0..100 {
            assert!(player.tick());
        }
        assert!(!player.tick(), "第 101 刻清空");
        let mut mob = KillCredit::Mob { display, age: 0 };
        for _ in 0..100 {
            assert!(mob.tick());
        }
        assert!(!mob.tick(), "tickCount - timestamp > 100 时清空");
    }

    #[test]
    fn intentional_game_design_branch_stays_encoded() {
        let component = intentional_game_design(
            "badRespawnPoint",
            "Kitten",
            [0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        let text = format!("{:?}", component.tag());
        assert!(text.contains("death.attack.badRespawnPoint.message"));
        assert!(text.contains("death.attack.badRespawnPoint.link"));
        assert!(text.contains("MCPE-28723"));
    }
}
