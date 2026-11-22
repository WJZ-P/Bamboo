//! 大脑的存档：官方 `Brain.Packed.CODEC`，也就是 `{memories: {<记忆名>: {value, ttl}}}`。
//! 只写带编解码器的记忆（`MemoryModuleType.canSerialize`），`ttl` 只在会过期时写。
//!
//! Brain persistence: vanilla `Brain.Packed.CODEC`, `{memories: {<name>: {value, ttl}}}`.
//! Only memories with a codec are written (`MemoryModuleType.canSerialize`); `ttl` only
//! when the memory expires.

use mc_bamboo_nbt::Tag;

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
        MemoryType::IsPanicking | MemoryType::DangerDetectedRecently => match raw {
            Tag::Byte(value) => Some(MemoryValue::Bool(*value != 0)),
            _ => None,
        },
        // `Unit.CODEC` 存成空 compound，读回来是 `Unit`。
        // `Unit.CODEC` writes an empty compound; reading it back yields `Unit`.
        MemoryType::IsInWater | MemoryType::IsPregnant => match raw {
            Tag::Compound(_) => Some(MemoryValue::Unit),
            _ => None,
        },
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
}
