//! `Warden`：`Monster` → `Warden` 的专属状态。
//!
//! 官方把愤怒值放在 [`AngerManagement`]，把振动监听者放在 `VibrationSystem.Data`，
//! 两者都在 `anger` / `listener` 两个存档键里。姿态和客户端愤怒等级是同步元数据，
//! 心跳 / 触须动画是客户端表现，这里一起记下。

use std::cmp::Ordering;

use mc_bamboo_data::{PathType, registries::attribute};
use mc_bamboo_nbt::Tag;

use crate::{
    archive::tag::{self, put},
    entity::EntityUid,
    game_event::vibration::VibrationData,
};

use super::{attributes::Attributes, path::Malus};

/// 监守者姿态。`Warden.getDefaultDimensions` 在钻出 / 钻入时把高度压成 1。
///
/// The warden poses. `Warden.getDefaultDimensions` fixes the height to 1 while
/// emerging or digging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WardenPose {
    #[default]
    Standing,
    Emerging,
    Digging,
    Roaring,
    Sniffing,
}

impl WardenPose {
    /// 钻出 / 钻入地面：此时无敌、不推动、尺寸压平。
    ///
    /// Emerging or digging: invulnerable, unpushable and flattened.
    pub const fn is_digging_or_emerging(self) -> bool {
        matches!(self, Self::Emerging | Self::Digging)
    }
}

/// 官方 `AngerLevel`：按最低愤怒值降序，`byAnger` 取第一个满足的。
///
/// Vanilla `AngerLevel`: sorted by minimum anger descending; `byAnger` takes the first match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AngerLevel {
    #[default]
    Calm,
    Agitated,
    Angry,
}

impl AngerLevel {
    /// `AngerLevel.byAnger`。
    pub const fn by_anger(anger: i32) -> Self {
        if anger >= Self::ANGRY_MINIMUM {
            Self::Angry
        } else if anger >= Self::AGITATED_MINIMUM {
            Self::Agitated
        } else {
            Self::Calm
        }
    }

    /// `getMinimumAnger()`。
    pub const fn minimum_anger(self) -> i32 {
        match self {
            Self::Calm => 0,
            Self::Agitated => Self::AGITATED_MINIMUM,
            Self::Angry => Self::ANGRY_MINIMUM,
        }
    }

    /// `isAngry()`。
    pub const fn is_angry(self) -> bool {
        matches!(self, Self::Angry)
    }

    /// `getAmbientSound()`。
    pub const fn ambient_sound(self) -> &'static str {
        match self {
            Self::Calm => "minecraft:entity.warden.ambient",
            Self::Agitated => "minecraft:entity.warden.agitated",
            Self::Angry => "minecraft:entity.warden.angry",
        }
    }

    /// `getListeningSound()`。
    pub const fn listening_sound(self) -> &'static str {
        match self {
            Self::Calm => "minecraft:entity.warden.listening",
            Self::Agitated | Self::Angry => "minecraft:entity.warden.listening_angry",
        }
    }

    pub const AGITATED_MINIMUM: i32 = 40;
    pub const ANGRY_MINIMUM: i32 = 80;
}

/// 一个已解析到实体的愤怒嫌疑。`is_player` 供 `AngerManagement.Sorter` 排优先级。
///
/// A suspect already resolved to an entity. `is_player` feeds the `AngerManagement.Sorter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Suspect {
    pub uid: EntityUid,
    pub uuid: [u8; 16],
    pub anger: i32,
    pub is_player: bool,
}

/// 官方 `AngerManagement`：按愤怒排序的嫌疑表，外加读档时按 UUID 暂存的一份。
///
/// Vanilla `AngerManagement`: a suspect list sorted by anger, plus a UUID-keyed
/// staging list used when loading from disk.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AngerManagement {
    pub suspects: Vec<Suspect>,
    pub anger_by_uuid: Vec<([u8; 16], i32)>,
    pub highest_anger: i32,
    pub conversion_delay: i32,
}

impl AngerManagement {
    /// `MAX_ANGER`。
    pub const MAX_ANGER: i32 = 150;

    /// `increaseAnger(entity, amount)`：封顶 150，新嫌疑要把同名 UUID 的暂存值并进来。
    ///
    /// `increaseAnger(entity, amount)`: clamp to 150; a new suspect absorbs any
    /// staged anger keyed by the same UUID.
    pub fn increase_anger(
        &mut self,
        uid: EntityUid,
        uuid: [u8; 16],
        amount: i32,
        is_player: bool,
    ) -> i32 {
        let known = self.suspects.iter().position(|s| s.uid == uid);
        let base = known.map(|index| self.suspects[index].anger).unwrap_or(0);
        let mut anger = (base + amount).min(Self::MAX_ANGER);
        if let Some(index) = known {
            self.suspects[index].anger = anger;
        } else {
            if let Some(staged) = self.anger_by_uuid.iter().position(|(id, _)| *id == uuid) {
                anger = (anger + self.anger_by_uuid.remove(staged).1).min(Self::MAX_ANGER);
            }
            self.suspects.push(Suspect {
                uid,
                uuid,
                anger,
                is_player,
            });
        }
        self.sort_and_update_highest();
        anger
    }

    /// `AngerManagement.tick`：每 20 刻一次，嫌疑与按 UUID 暂存的愤怒各衰减 1、
    /// 值到 1 就清掉，再重排。读档的 UUID→实体解析依赖世界查询，不在这里做
    /// （对拍避开那几刻，见 README 已知限制）。
    ///
    /// `AngerManagement.tick`: once every 20 ticks, decay every suspect and staged
    /// anger by 1, drop values reaching 1, then re-sort. UUID→entity resolution
    /// needs a world lookup and is not done here (the oracle avoids those ticks).
    pub fn tick(&mut self) {
        self.suspects.retain_mut(|s| {
            s.anger -= 1;
            s.anger > 1
        });
        self.anger_by_uuid.retain_mut(|(_, anger)| {
            *anger -= 1;
            *anger > 1
        });
        self.sort_and_update_highest();
    }

    /// `clearAnger(entity)`。
    pub fn clear_anger(&mut self, uid: EntityUid) {
        self.suspects.retain(|s| s.uid != uid);
        self.sort_and_update_highest();
    }

    /// `getAngerLevel()`：按 `getActiveAnger(null)` 换算（活动切换用）。
    ///
    /// `getAngerLevel()`: from `getActiveAnger(null)` (used for activity switches).
    pub fn anger_level(&self) -> AngerLevel {
        AngerLevel::by_anger(self.highest_anger)
    }

    /// `getActiveAnger(entity)`：没有目标时返回 `highestAnger`。
    ///
    /// `getActiveAnger(entity)`: without a target, return `highestAnger`.
    pub fn get_active_anger(&self, target: Option<EntityUid>) -> i32 {
        match target {
            None => self.highest_anger,
            Some(uid) => self
                .suspects
                .iter()
                .find(|s| s.uid == uid)
                .map_or(0, |s| s.anger),
        }
    }

    /// `getActiveEntity()`：排序后的第一个嫌疑。
    pub fn get_active_entity(&self) -> Option<EntityUid> {
        self.suspects.first().map(|s| s.uid)
    }

    /// `Sorter.compare`：先 ANGRY、再玩家、最后愤怒值降序；排序的副作用是刷新 `highestAnger`。
    ///
    /// `Sorter.compare`: ANGRY first, then players, then descending anger; sorting
    /// refreshes `highestAnger` as a side effect.
    pub fn sort_and_update_highest(&mut self) {
        self.suspects.sort_by(|a, b| {
            if a.uid == b.uid {
                return Ordering::Equal;
            }
            let a_angry = AngerLevel::by_anger(a.anger).is_angry();
            let b_angry = AngerLevel::by_anger(b.anger).is_angry();
            if a_angry != b_angry {
                return if a_angry {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
            if a.is_player != b.is_player {
                return if a.is_player {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
            b.anger.cmp(&a.anger)
        });
        self.highest_anger = match self.suspects.len() {
            0 => 0,
            1 => self.suspects[0].anger,
            _ => self.suspects.iter().map(|s| s.anger).max().unwrap_or(0),
        };
    }

    /// `AngerManagement.codec` 的 `suspects`：`List<{uuid: int[4], anger: int}>`。
    ///
    /// The `suspects` field of `AngerManagement.codec`.
    pub fn write_codec(&self) -> Tag {
        let mut list = Vec::with_capacity(self.suspects.len() + self.anger_by_uuid.len());
        for s in &self.suspects {
            list.push(suspect_tag(s.uuid, s.anger));
        }
        for &(uuid, anger) in &self.anger_by_uuid {
            list.push(suspect_tag(uuid, anger));
        }
        let mut root = Vec::new();
        put(&mut root, "suspects", Tag::List(list));
        Tag::Compound(root)
    }

    /// 读回 `anger`；坏数据回到空表。读到的都是按 UUID 暂存、等下一刻解析成实体。
    ///
    /// Reads `anger`; bad data falls back to an empty table. Loaded entries stay
    /// UUID-staged until they resolve to entities.
    pub fn read_codec(tag: &Tag) -> Self {
        let mut out = Self::default();
        let Some(Tag::List(items)) = tag::field(tag, "suspects") else {
            return out;
        };
        for item in items {
            let Some(Tag::IntArray(words)) = tag::field(item, "uuid") else {
                continue;
            };
            if words.len() != 4 {
                continue;
            }
            let anger = tag::int_or(item, "anger", 0);
            if anger > 0 {
                out.anger_by_uuid.push((uuid_bytes(words), anger));
            }
        }
        out
    }
}

/// `UUIDUtil.CODEC` 的 `int[4]`。
fn suspect_tag(uuid: [u8; 16], anger: i32) -> Tag {
    let mut one = Vec::new();
    put(&mut one, "uuid", uuid_tag(&uuid));
    put(&mut one, "anger", Tag::Int(anger));
    Tag::Compound(one)
}

fn uuid_tag(bytes: &[u8; 16]) -> Tag {
    let mut words = [0_i32; 4];
    for (index, word) in words.iter_mut().enumerate() {
        let mut value = 0_u32;
        for &byte in &bytes[index * 4..index * 4 + 4] {
            value = value << 8 | u32::from(byte);
        }
        *word = value as i32;
    }
    Tag::IntArray(words.to_vec())
}

fn uuid_bytes(words: &[i32]) -> [u8; 16] {
    let mut out = [0_u8; 16];
    for (index, word) in words.iter().take(4).enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&(*word as u32).to_be_bytes());
    }
    out
}

/// `Warden`：每个 `Mob` 都带一份，字段只在 `Warden` 分支读写。
///
/// `Warden`: carried by every `Mob`; only the `Warden` branch reads and writes it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Warden {
    pub anger: AngerManagement,
    pub listener: VibrationData,
    pub pose: WardenPose,
    /// `CLIENT_ANGER_LEVEL` 元数据，等于 `getActiveAnger()`。
    ///
    /// The `CLIENT_ANGER_LEVEL` metadata, equal to `getActiveAnger()`.
    pub client_anger_level: i32,
    pub tendril_animation: i32,
    pub heart_animation: i32,
}

impl Warden {
    /// `getAngerLevel()`。
    pub fn anger_level(&self) -> AngerLevel {
        AngerLevel::by_anger(self.anger.get_active_anger(None))
    }
}

/// `Warden.createAttributes()`：`Monster.createMonsterAttributes()` 上覆盖六个值。
///
/// `Warden.createAttributes()`: six overrides on `Monster.createMonsterAttributes()`.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::monster();
    table.set_base(attribute::MAX_HEALTH, 500.0);
    table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.3_f32));
    table.set_base(attribute::KNOCKBACK_RESISTANCE, 1.0);
    table.set_base(attribute::ATTACK_KNOCKBACK, 1.5);
    table.set_base(attribute::ATTACK_DAMAGE, 30.0);
    table.set_base(attribute::FOLLOW_RANGE, 24.0);
    table
}

/// `Warden` 构造函数里的六项 `setPathfindingMalus`。
///
/// The six `setPathfindingMalus` calls in the `Warden` constructor.
pub(super) fn malus() -> Malus {
    let mut malus = Malus::monster();
    malus.set(PathType::UnpassableRail, 0.0);
    malus.set(PathType::Damaging, 8.0);
    malus.set(PathType::PowderSnow, 8.0);
    malus.set(PathType::Lava, 8.0);
    malus.set(PathType::Fire, 0.0);
    malus.set(PathType::FireInNeighbor, 0.0);
    malus
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anger_level_thresholds() {
        assert_eq!(AngerLevel::by_anger(0), AngerLevel::Calm);
        assert_eq!(AngerLevel::by_anger(39), AngerLevel::Calm);
        assert_eq!(AngerLevel::by_anger(40), AngerLevel::Agitated);
        assert_eq!(AngerLevel::by_anger(79), AngerLevel::Agitated);
        assert_eq!(AngerLevel::by_anger(80), AngerLevel::Angry);
        assert!(AngerLevel::Angry.is_angry());
    }

    #[test]
    fn anger_clamps_and_sorts() {
        let mut anger = AngerManagement::default();
        assert_eq!(anger.increase_anger(1, [1; 16], 35, false), 35);
        assert_eq!(anger.increase_anger(2, [2; 16], 80, true), 80);
        // 封顶 150。
        assert_eq!(anger.increase_anger(1, [1; 16], 200, false), 150);
        // 玩家优先，其次愤怒值降序。
        assert_eq!(anger.get_active_entity(), Some(2));
        anger.clear_anger(2);
        assert_eq!(anger.get_active_entity(), Some(1));
        assert_eq!(anger.get_active_anger(Some(1)), 150);
    }

    #[test]
    fn anger_codec_round_trip() {
        let mut anger = AngerManagement::default();
        anger.increase_anger(1, [7; 16], 35, false);
        anger.anger_by_uuid.push(([9; 16], 12));
        let tag = anger.write_codec();
        let read = AngerManagement::read_codec(&tag);
        assert_eq!(read.anger_by_uuid.len(), 2);
        assert_eq!(read.anger_by_uuid[0], ([7; 16], 35));
        assert_eq!(read.anger_by_uuid[1], ([9; 16], 12));
        assert_eq!(
            AngerManagement::read_codec(&Tag::Int(1)),
            AngerManagement::default()
        );
    }

    #[test]
    fn attributes_match_the_official_values() {
        let table = attributes();
        assert_eq!(table.value(attribute::MAX_HEALTH), 500.0);
        assert_eq!(table.value(attribute::MOVEMENT_SPEED), f64::from(0.3_f32));
        assert_eq!(table.value(attribute::KNOCKBACK_RESISTANCE), 1.0);
        assert_eq!(table.value(attribute::ATTACK_KNOCKBACK), 1.5);
        assert_eq!(table.value(attribute::ATTACK_DAMAGE), 30.0);
        assert_eq!(table.value(attribute::FOLLOW_RANGE), 24.0);
    }

    /// 监守者生成时是半径 16 的振动监听者；钻出 / 钻入时高度压成 1。
    ///
    /// A fresh warden is a radius-16 vibration listener; its height is 1 while
    /// emerging or digging.
    #[test]
    fn warden_construction_and_pose_facts() {
        use crate::entity::mob::{Mob, MobType};
        use crate::game_event::dispatch::DeliveryMode;

        let mut mob = Mob::new(MobType::Warden);
        let listener = mob.entity_listener.as_ref().expect("warden is a listener");
        assert_eq!(listener.radius, 16);
        assert_eq!(listener.delivery_mode, DeliveryMode::ByDistance);
        assert_eq!(mob.dimensions(), (0.9, 2.9));
        mob.warden.pose = WardenPose::Emerging;
        assert_eq!(mob.dimensions(), (0.9, 1.0));
        mob.warden.pose = WardenPose::Standing;
        assert_eq!(mob.dimensions(), (0.9, 2.9));
        assert_eq!(MobType::Warden.sound_volume(), 4.0);
        assert!(MobType::Warden.fire_immune());
        assert!(!MobType::Warden.removes_when_far_away());
    }
}
