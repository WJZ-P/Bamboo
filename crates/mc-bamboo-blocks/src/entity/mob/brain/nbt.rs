//! 大脑的存档：官方 `Brain.Packed.CODEC`，也就是 `{memories: {<记忆名>: {value, ttl}}}`。
//! 只写带编解码器的记忆（`MemoryModuleType.canSerialize`），`ttl` 只在会过期时写。
//!
//! Brain persistence: vanilla `Brain.Packed.CODEC`, `{memories: {<name>: {value, ttl}}}`.
//! Only memories with a codec are written (`MemoryModuleType.canSerialize`); `ttl` only
//! when the memory expires.

use mc_bamboo_core::BlockPos;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::components::value::GlobalPos;

use super::{
    Brain,
    memory::{MemoryType, MemoryValue},
};
use crate::archive::tag::{self, put};

/// `Brain.pack()` 加 `Brain.Packed.CODEC`。
///
/// `Brain.pack()` plus `Brain.Packed.CODEC`.
pub fn write(brain: Option<&Brain>) -> Tag {
    let mut memories = Vec::new();
    for (kind, value, ttl) in brain.into_iter().flat_map(|brain| brain.memories.filled()) {
        if !kind.serializable() {
            continue;
        }
        let Some(value) = value_tag(value) else {
            continue;
        };
        let mut entry = vec![("value".to_owned(), value)];
        if let Some(ttl) = ttl {
            put(&mut entry, "ttl", Tag::Long(ttl));
        }
        put(
            &mut memories,
            &format!("minecraft:{}", kind.name()),
            Tag::Compound(entry),
        );
    }
    Tag::Compound(vec![("memories".to_owned(), Tag::Compound(memories))])
}

fn value_tag(value: &MemoryValue) -> Option<Tag> {
    match value {
        MemoryValue::Bool(value) => Some(Tag::Byte(i8::from(*value))),
        MemoryValue::Int(value) => Some(Tag::Int(*value)),
        MemoryValue::Long(value) => Some(Tag::Long(*value)),
        MemoryValue::Unit => Some(Tag::Compound(Vec::new())),
        MemoryValue::GlobalPos(pos) => Some(Tag::Compound(vec![
            ("dimension".to_owned(), Tag::String(pos.dimension.clone())),
            (
                "pos".to_owned(),
                Tag::IntArray(vec![pos.pos.x, pos.pos.y, pos.pos.z]),
            ),
        ])),
        // `GlobalPos.CODEC.listOf()`：`List<Compound>`，每个是 `{dimension, pos}`。
        // `GlobalPos.CODEC.listOf()`: a list of `{dimension, pos}` compounds.
        MemoryValue::GlobalPosList(list) => Some(Tag::List(
            list.iter()
                .map(|pos| {
                    Tag::Compound(vec![
                        ("dimension".to_owned(), Tag::String(pos.dimension.clone())),
                        (
                            "pos".to_owned(),
                            Tag::IntArray(vec![pos.pos.x, pos.pos.y, pos.pos.z]),
                        ),
                    ])
                })
                .collect(),
        )),
        _ => None,
    }
}

/// `new Brain(..., packed.memories, ...)` 里的 `setMemoryInternal`：认识的、登记过的
/// 记忆按存档的值和 `ttl` 放回去。
///
/// The `setMemoryInternal` loop of `new Brain(..., packed.memories, ...)`: known,
/// registered memories come back with their saved value and `ttl`.
pub fn read(brain: &mut Brain, saved: &Tag) {
    let Some(Tag::Compound(entries)) = tag::field(saved, "memories") else {
        return;
    };
    for (name, entry) in entries {
        let Some(kind) = MemoryType::by_name(name) else {
            continue;
        };
        if !kind.serializable() {
            continue;
        }
        let Some(raw) = tag::field(entry, "value") else {
            continue;
        };
        let Some(value) = value_of(kind, raw) else {
            continue;
        };
        match tag::field(entry, "ttl").and_then(|ttl| tag::java_i64(ttl).ok()) {
            Some(ttl) => brain.memories.set_with_ttl(kind, value, ttl),
            None => brain.memories.set(kind, value),
        }
    }
}

fn value_of(kind: MemoryType, raw: &Tag) -> Option<MemoryValue> {
    match kind {
        MemoryType::IsPanicking
        | MemoryType::DangerDetectedRecently
        | MemoryType::HuntingCooldown
        | MemoryType::GolemDetectedRecently => match raw {
            Tag::Byte(value) => Some(MemoryValue::Bool(*value != 0)),
            _ => None,
        },
        // `Unit.CODEC` 存成空 compound，读回来是 `Unit`。
        // `Unit.CODEC` writes an empty compound; reading it back yields `Unit`.
        MemoryType::IsInWater | MemoryType::IsPregnant => match raw {
            Tag::Compound(_) => Some(MemoryValue::Unit),
            _ => None,
        },
        // `GlobalPos.CODEC`：`{dimension: String, pos: IntArray[3]}`。
        // `GlobalPos.CODEC`: `{dimension: String, pos: IntArray[3]}`.
        MemoryType::Home
        | MemoryType::JobSite
        | MemoryType::PotentialJobSite
        | MemoryType::MeetingPoint => {
            let dimension = tag::string_or(raw, "dimension")?.to_owned();
            let Some(Tag::IntArray(coords)) = tag::field(raw, "pos") else {
                return None;
            };
            let [x, y, z] = coords.as_slice() else {
                return None;
            };
            Some(MemoryValue::GlobalPos(GlobalPos {
                dimension,
                pos: BlockPos::new(*x, *y, *z),
            }))
        }
        MemoryType::LastWorkedAtPoi | MemoryType::LastSlept | MemoryType::LastWoken => {
            tag::java_i64(raw).ok().map(MemoryValue::Long)
        }
        MemoryType::SnifferExploredPositions => {
            let Tag::List(entries) = raw else {
                return None;
            };
            let mut list = Vec::with_capacity(entries.len());
            for entry in entries {
                let dimension = tag::string_or(entry, "dimension")?.to_owned();
                let Some(Tag::IntArray(coords)) = tag::field(entry, "pos") else {
                    return None;
                };
                let [x, y, z] = coords.as_slice() else {
                    return None;
                };
                list.push(GlobalPos {
                    dimension,
                    pos: BlockPos::new(*x, *y, *z),
                });
            }
            Some(MemoryValue::GlobalPosList(list))
        }
        _ => tag::java_i32(raw).ok().map(MemoryValue::Int),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 会过期的危险记忆连同 `ttl` 往返；不能存的记忆不写。
    ///
    /// An expiring danger memory round-trips with its `ttl`; unserialisable ones are skipped.
    #[test]
    fn memories_round_trip_with_ttl() {
        let mut brain = super::super::super::armadillo::brain();
        brain.memories.set_with_ttl(
            MemoryType::DangerDetectedRecently,
            MemoryValue::Bool(true),
            42,
        );
        brain
            .memories
            .set(MemoryType::WalkTarget, MemoryValue::Unit);
        let saved = write(Some(&brain));
        let mut loaded = super::super::super::armadillo::brain();
        read(&mut loaded, &saved);
        assert_eq!(
            loaded
                .memories
                .time_until_expiry(MemoryType::DangerDetectedRecently),
            42
        );
        assert!(!loaded.memories.has(MemoryType::WalkTarget));
    }

    /// 村民带编解码器的记忆（`GlobalPos` / `Long` / `Bool` / `Int`）按官方 `Brain.Packed` 格式往返，
    /// 不带编解码器的记忆（如 `nearest_hostile`）不写盘。
    #[test]
    fn villager_serializable_memories_round_trip() {
        use super::super::{Activity, ActivityData, Brain, behavior::Kind};

        let build = || {
            let mut brain = Brain::new(
                &[],
                vec![ActivityData::sequential(
                    Activity::Core,
                    0,
                    vec![Kind::DoNothing { min: 10, max: 10 }],
                )],
            );
            for kind in [
                MemoryType::Home,
                MemoryType::JobSite,
                MemoryType::LastSlept,
                MemoryType::LastWoken,
                MemoryType::LastWorkedAtPoi,
                MemoryType::GolemDetectedRecently,
                MemoryType::ItemPickupCooldownTicks,
                MemoryType::NearestHostile,
            ] {
                brain.memories.register(kind);
            }
            brain
        };

        let mut brain = build();
        let home = GlobalPos {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(10, 64, -20),
        };
        let job = GlobalPos {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(14, 64, -18),
        };
        brain
            .memories
            .set(MemoryType::Home, MemoryValue::GlobalPos(home.clone()));
        brain
            .memories
            .set(MemoryType::JobSite, MemoryValue::GlobalPos(job.clone()));
        brain
            .memories
            .set(MemoryType::LastSlept, MemoryValue::Long(12_345));
        brain
            .memories
            .set(MemoryType::LastWoken, MemoryValue::Long(24_001));
        brain
            .memories
            .set(MemoryType::LastWorkedAtPoi, MemoryValue::Long(3_000));
        brain.memories.set_with_ttl(
            MemoryType::GolemDetectedRecently,
            MemoryValue::Bool(true),
            599,
        );
        brain
            .memories
            .set(MemoryType::ItemPickupCooldownTicks, MemoryValue::Int(40));
        brain
            .memories
            .set(MemoryType::NearestHostile, MemoryValue::Entity(99));

        let saved = write(Some(&brain));
        let mut loaded = build();
        read(&mut loaded, &saved);

        assert_eq!(loaded.memories.global_pos(MemoryType::Home), Some(&home));
        assert_eq!(loaded.memories.global_pos(MemoryType::JobSite), Some(&job));
        assert_eq!(loaded.memories.long(MemoryType::LastSlept), Some(12_345));
        assert_eq!(loaded.memories.long(MemoryType::LastWoken), Some(24_001));
        assert_eq!(
            loaded.memories.long(MemoryType::LastWorkedAtPoi),
            Some(3_000)
        );
        assert_eq!(
            loaded.memories.bool(MemoryType::GolemDetectedRecently),
            Some(true)
        );
        assert_eq!(
            loaded
                .memories
                .time_until_expiry(MemoryType::GolemDetectedRecently),
            599
        );
        assert_eq!(
            loaded.memories.int(MemoryType::ItemPickupCooldownTicks),
            Some(40)
        );
        assert!(!loaded.memories.has(MemoryType::NearestHostile));
    }
}
