//! 玩家肩膀上的鹦鹉：`ServerPlayer` 的左右肩槽、上肩、离肩和放回世界。
//!
//! 这份状态与判据是生产逻辑：`mc-bamboo-game` 的真实玩家和 `mob-oracle` 的假世界
//! 玩家都走 [`ShoulderState`] 的同一组函数，逐刻对拍才是在比 Bamboo 自己的行为。
//!
//! Parrots perched on a player's shoulders: the left/right slots, perching, removal
//! conditions and respawning. Both the real player in `mc-bamboo-game` and the fake
//! player in `mob-oracle` go through this shared [`ShoulderState`], so the oracle
//! compares Bamboo's own behaviour instead of hard-coded expectations.

use std::cell::Cell;

use mc_bamboo_core::Vec3;
use mc_bamboo_nbt::Tag;

use crate::entity::{Entity, mob};

/// `ServerPlayer.shoulderEntityLeft` / `Right`：整只鹦鹉的 `saveWithoutId` NBT，
/// 空肩是 `None`。`since` 是上肩后的相对刻数（官方用绝对游戏时刻）。
///
/// `ServerPlayer.shoulderEntityLeft` / `Right`: the whole parrot's
/// `saveWithoutId` tag; `None` is an empty shoulder. `since` counts ticks since
/// the last perch (vanilla uses an absolute game time).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShoulderState {
    pub left: Option<Tag>,
    pub right: Option<Tag>,
    /// 没有鹦鹉时用 `i32::MAX`，表示「早就上过肩」。
    pub since: i32,
}

impl ShoulderState {
    /// 空肩的起点：读档或新玩家，官方 `timeEntitySatOnShoulder` 为 0，
    /// 但相对计数下 `MAX` 才能让读档后的第一次离肩条件立刻生效。
    pub fn empty() -> Self {
        Self {
            left: None,
            right: None,
            since: i32::MAX,
        }
    }

    /// `Player.DATA_SHOULDER_PARROT_*` 同步的品种 ID（客户端画肩膀鹦鹉用）。
    ///
    /// The variant id synced as `DATA_SHOULDER_PARROT_*` (clients draw the parrot).
    pub fn left_variant(&self) -> Option<i32> {
        variant_of(&self.left)
    }

    /// 右肩的品种 ID。
    pub fn right_variant(&self) -> Option<i32> {
        variant_of(&self.right)
    }
}

/// `ShoulderEntity*` 的 `Variant`（旧版整数编码）；空肩 / 坏值返回 `None`。
///
/// The `Variant` of a shoulder entity (legacy integer coding); `None` for an
/// empty shoulder or a malformed tag.
pub fn variant_of(tag: &Option<Tag>) -> Option<i32> {
    let Tag::Compound(entries) = tag.as_ref()? else {
        return None;
    };
    entries
        .iter()
        .find_map(|(key, value)| match (key.as_str(), value) {
            ("Variant", Tag::Int(variant)) => Some(*variant),
            _ => None,
        })
}

/// `ServerPlayer.setEntityOnShoulder`：左肩空先左，右肩空再右，都满返回 false。
/// 成功时把相对倒计时归零（官方上肩后 20 刻内不因离肩条件落下）。
///
/// `ServerPlayer.setEntityOnShoulder`: left first, then right, false when both
/// are occupied. A successful perch resets the relative 20-tick removal lock.
pub fn set_entity_on_shoulder(state: &mut ShoulderState, tag: Tag) -> bool {
    let right_first = ORACLE_TAMPER.with(|cell| cell.get()) == Tamper::SwapShoulders;
    let slot = if state.left.is_none() && !right_first {
        &mut state.left
    } else if state.right.is_none() && right_first {
        &mut state.right
    } else if state.left.is_none() {
        &mut state.left
    } else if state.right.is_none() {
        &mut state.right
    } else {
        return false;
    };
    *slot = Some(tag);
    state.since = 0;
    true
}

/// `ServerPlayer.handleShoulderEntities` 的离肩判据：摔落距离、在水里 / 游泳、
/// 飞行、睡觉、细雪、受伤。
///
/// `ServerPlayer.handleShoulderEntities` removal conditions: fall distance,
/// water / swimming, flying, sleeping, powder snow and being hurt.
#[derive(Debug, Clone, Copy, Default)]
pub struct RemovalFacts {
    pub fall_distance: f64,
    pub in_water: bool,
    pub flying: bool,
    pub sleeping: bool,
    pub in_powder_snow: bool,
    /// `Player.hurtServer` 一开头就调 `removeEntitiesOnShoulder()`。
    pub hurt: bool,
}

impl RemovalFacts {
    /// 官方 `fallDistance > 0.5 || isInWater() || abilities.flying ||
    /// isSleeping() || isInPowderSnow`（外加受伤那条独立入口）。
    fn should_remove(self) -> bool {
        let skip_fall = ORACLE_TAMPER.with(|cell| cell.get()) == Tamper::NoFallRemoval;
        (!skip_fall && self.fall_distance > 0.5)
            || self.in_water
            || self.flying
            || self.sleeping
            || self.in_powder_snow
            || self.hurt
    }
}

/// 一次离肩的结果：按官方顺序先左后右，交给调用方从 NBT 造回实体。
///
/// One removal result: left then right, as vanilla does; the caller turns each
/// tag back into an entity.
pub struct Removed {
    pub position: Vec3,
    pub tags: Vec<Tag>,
}

/// 每刻的 `handleShoulderEntities`：推进相对倒计时；上肩 20 刻后遇到离肩判据
/// 就清空两肩（官方 `timeEntitySatOnShoulder + 20 < gameTime`）。
///
/// `handleShoulderEntities` per tick: advance the relative counter and, after
/// 20 ticks, clear both shoulders when a removal condition holds.
pub fn step(
    state: &mut ShoulderState,
    facts: RemovalFacts,
    player_position: Vec3,
) -> Option<Removed> {
    state.since = state.since.saturating_add(1);
    if state.left.is_none() && state.right.is_none() {
        return None;
    }
    if state.since > 20 && facts.should_remove() {
        // `respawnEntityOnShoulder` 把鹦鹉放在玩家 y + 0.7 处。
        let position = Vec3::new(
            player_position.x,
            player_position.y + 0.7,
            player_position.z,
        );
        let mut tags = Vec::new();
        if let Some(tag) = state.left.take() {
            tags.push(tag);
        }
        if let Some(tag) = state.right.take() {
            tags.push(tag);
        }
        state.since = i32::MAX;
        return Some(Removed { position, tags });
    }
    None
}

/// `ServerPlayer.respawnEntityOnShoulder`：`EntityType.create(LOAD)` 从 NBT 造回
/// 鹦鹉、认回主人、放到玩家 y + 0.7 处。坏 NBT 返回 `None`，不 panic。
///
/// `ServerPlayer.respawnEntityOnShoulder`: rebuild the parrot from NBT, restore
/// its owner and place it 0.7 above the player. Malformed NBT yields `None`.
pub fn respawn(tag: &Tag, position: Vec3, owner_uuid: Option<[u8; 16]>) -> Option<Entity> {
    let row = mob::nbt::read_mob(tag, "minecraft:parrot", 0).ok()??;
    let mut entity = row.into_entity();
    entity.position = position;
    if let Some(mob) = entity.kind.mob_mut() {
        mob.tame.owner = owner_uuid;
        mob.tame.flags |= mob::tame::FLAG_TAME;
    }
    Some(entity)
}

/// 是否在上肩时跳过 `discard()`。仅 `mob-oracle --break-self-test` 使用。
///
/// Whether to skip `discard()` on perch. Only used by the oracle break checks.
pub fn keep_entity_on_perch() -> bool {
    ORACLE_TAMPER.with(|cell| cell.get()) == Tamper::KeepEntity
}

/// 改坏确认：这几种故意改坏的是生产路径本身（上肩移除、离肩判据、左右肩顺序）。
/// 缺省 `None`，正式游戏零影响。
///
/// Break checks deliberately corrupt the production path itself (perch removal,
/// removal conditions, shoulder order). `None` by default, no effect in game.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tamper {
    None,
    KeepEntity,
    NoFallRemoval,
    SwapShoulders,
}

thread_local! {
    static ORACLE_TAMPER: Cell<Tamper> = const { Cell::new(Tamper::None) };
}

/// 由 `mob-oracle` 按 `--break-self-test` 设置，其他驱动不调用。
///
/// Set by `mob-oracle` from `--break-self-test`; no other driver calls this.
pub fn set_oracle_tamper(name: &str) {
    let tamper = match name {
        "shoulder_keep_entity" => Tamper::KeepEntity,
        "shoulder_no_fall" => Tamper::NoFallRemoval,
        "shoulder_swap" => Tamper::SwapShoulders,
        _ => Tamper::None,
    };
    ORACLE_TAMPER.with(|cell| cell.set(tamper));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parrot(variant: i32) -> Tag {
        Tag::Compound(vec![("Variant".to_owned(), Tag::Int(variant))])
    }

    #[test]
    fn perches_left_then_right() {
        let mut state = ShoulderState::empty();
        assert!(set_entity_on_shoulder(&mut state, parrot(2)));
        assert_eq!(state.left_variant(), Some(2));
        assert!(set_entity_on_shoulder(&mut state, parrot(4)));
        assert_eq!(state.right_variant(), Some(4));
        assert!(!set_entity_on_shoulder(&mut state, parrot(1)));
    }

    #[test]
    fn removal_waits_twenty_ticks() {
        let mut state = ShoulderState::empty();
        set_entity_on_shoulder(&mut state, parrot(2));
        let facts = RemovalFacts {
            fall_distance: 1.0,
            ..RemovalFacts::default()
        };
        for _ in 0..20 {
            assert!(step(&mut state, facts, Vec3::default()).is_none());
        }
        let removed = step(&mut state, facts, Vec3::new(0.5, 64.0, 0.5)).unwrap();
        assert_eq!(removed.tags, vec![parrot(2)]);
        assert_eq!(removed.position.y, 64.7);
        assert_eq!(state.left_variant(), None);
    }

    #[test]
    fn each_removal_condition_triggers() {
        for facts in [
            RemovalFacts {
                fall_distance: 1.0,
                ..RemovalFacts::default()
            },
            RemovalFacts {
                in_water: true,
                ..RemovalFacts::default()
            },
            RemovalFacts {
                flying: true,
                ..RemovalFacts::default()
            },
            RemovalFacts {
                sleeping: true,
                ..RemovalFacts::default()
            },
            RemovalFacts {
                in_powder_snow: true,
                ..RemovalFacts::default()
            },
            RemovalFacts {
                hurt: true,
                ..RemovalFacts::default()
            },
        ] {
            let mut state = ShoulderState::empty();
            set_entity_on_shoulder(&mut state, parrot(2));
            state.since = 21;
            assert!(step(&mut state, facts, Vec3::default()).is_some());
        }
    }

    #[test]
    fn break_checks_corrupt_the_production_path() {
        set_oracle_tamper("shoulder_swap");
        let mut state = ShoulderState::empty();
        set_entity_on_shoulder(&mut state, parrot(2));
        assert_eq!(state.left_variant(), None);
        assert_eq!(state.right_variant(), Some(2));

        set_oracle_tamper("shoulder_no_fall");
        let mut state = ShoulderState::empty();
        set_entity_on_shoulder(&mut state, parrot(2));
        state.since = 21;
        assert!(
            step(
                &mut state,
                RemovalFacts {
                    fall_distance: 1.0,
                    ..RemovalFacts::default()
                },
                Vec3::default()
            )
            .is_none()
        );

        set_oracle_tamper("shoulder_keep_entity");
        assert!(keep_entity_on_perch());
        set_oracle_tamper("");
        assert!(!keep_entity_on_perch());
        let mut state = ShoulderState::empty();
        set_entity_on_shoulder(&mut state, parrot(2));
        state.since = 21;
        assert!(
            step(
                &mut state,
                RemovalFacts {
                    fall_distance: 1.0,
                    ..RemovalFacts::default()
                },
                Vec3::default()
            )
            .is_some()
        );
    }

    #[test]
    fn respawn_restores_the_parrot() {
        let tag = Tag::Compound(vec![
            ("Variant".to_owned(), Tag::Int(3)),
            (
                "Owner".to_owned(),
                Tag::IntArray(vec![1, 2, 3, 4]),
            ),
        ]);
        let entity = respawn(&tag, Vec3::new(0.5, 64.7, 0.5), Some([7; 16])).unwrap();
        assert_eq!(entity.position.y, 64.7);
        let mob = entity.kind.mob().unwrap();
        assert_eq!(mob.parrot.variant, 3);
        assert_eq!(mob.tame.owner, Some([7; 16]));
    }
}
