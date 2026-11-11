//! `ChunkRedstone` 里对应官方方块实体的那几项 ⇄ NBT 键。
//!
//! 火把抖动表官方不存（`RedstoneTorchBlock.RECENT_TOGGLES` 是 static WeakHashMap，
//! 见 `docs/m3-storage.md` S9），所以这里不写 `torch_toggles`。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::registries::block_entity_type as kinds;
use mc_bamboo_nbt::Tag;

use super::{
    block_entity::crafter_disabled_slots,
    tag::{self, Result, put},
};
use crate::{ArchiveRedstone, LecternFacts, RedstoneEntry};

/// 写合成器/唱片机时需要游戏层和方块状态补的两项。
#[derive(Debug, Clone, Copy, Default)]
pub struct RedstoneFacts {
    /// 唱片机第一格有没有唱片；没有唱片时官方不写 `ticks_since_song_started`。
    pub jukebox_has_record: bool,
    /// 方块状态 `TRIGGERED`；官方 `CrafterBlockEntity.addTriggered` 写 0 或 1。
    pub crafter_triggered: bool,
    /// 原样保留的 `ticks_since_song_started`，`None` 时新写的方块实体写 0。
    pub song_ticks: Option<i64>,
}

/// 按位置把红石记忆写进方块实体条目。一个位置只写属于它那种方块实体的键。
pub fn write_redstone(
    pos: BlockPos,
    id: &str,
    memory: &ArchiveRedstone<'_>,
    facts: RedstoneFacts,
) -> Result<Vec<(String, Tag)>> {
    let mut out = Vec::new();
    let kind = kinds::by_name(id);
    if kind == Some(kinds::COMPARATOR) {
        let signal = memory
            .comparator_outputs
            .iter()
            .find(|(at, _)| *at == pos)
            .map_or(0, |(_, signal)| *signal);
        put(&mut out, "OutputSignal", Tag::Int(signal));
    }
    if id == "minecraft:chiseled_bookshelf" {
        let slot = memory
            .bookshelf_slots
            .iter()
            .find(|(at, _)| *at == pos)
            .map_or(-1, |(_, slot)| i32::try_from(*slot).unwrap_or(-1));
        put(&mut out, "last_interacted_slot", Tag::Int(slot));
    }
    if id == "minecraft:jukebox" && facts.jukebox_has_record {
        // 官方只在有歌曲时写；保留值由 song_ticks 给出，新写的方块实体写 0。
        put(
            &mut out,
            "ticks_since_song_started",
            Tag::Long(facts.song_ticks.unwrap_or(0)),
        );
    }
    if id == "minecraft:crafter" {
        let mask = memory
            .crafter_disabled
            .iter()
            .find(|(at, _)| *at == pos)
            .map_or(0, |(_, mask)| *mask);
        let remaining = memory
            .crafter_crafting
            .iter()
            .find(|(at, _)| *at == pos)
            .map_or(0, |(_, remaining)| *remaining);
        put(&mut out, "crafting_ticks_remaining", Tag::Int(remaining));
        put(&mut out, "disabled_slots", crafter_disabled_slots(mask));
        put(
            &mut out,
            "triggered",
            Tag::Int(i32::from(facts.crafter_triggered)),
        );
    }
    Ok(out)
}

/// 从一条官方方块实体 NBT 还原红石记忆。没有可建模的状态时返回空列表。
pub fn read_redstone(pos: BlockPos, id: &str, tag: &Tag) -> Vec<RedstoneEntry> {
    let mut out = Vec::new();
    match id {
        "minecraft:comparator" => out.push(RedstoneEntry::Comparator {
            pos,
            signal: tag::int_or(tag, "OutputSignal", 0),
        }),
        "minecraft:chiseled_bookshelf" => {
            let slot = tag::int_or(tag, "last_interacted_slot", -1);
            if let Ok(slot) = usize::try_from(slot) {
                out.push(RedstoneEntry::Bookshelf { pos, slot });
            }
        }
        "minecraft:jukebox" => {
            // 有唱片才记「在放」。
            if tag::field(tag, "RecordItem").is_some() {
                out.push(RedstoneEntry::Jukebox { pos });
            }
        }
        "minecraft:crafter" => {
            let mut mask = 0u16;
            for slot in tag::int_array(tag, "disabled_slots") {
                if (0..9).contains(&slot) {
                    mask |= 1 << slot;
                }
            }
            let remaining = tag::int_or(tag, "crafting_ticks_remaining", 0);
            out.push(RedstoneEntry::CrafterDisabled { pos, mask });
            out.push(RedstoneEntry::CrafterCrafting { pos, remaining });
        }
        "minecraft:lectern" => {
            let page = tag::int_or(tag, "Page", 0);
            if let Some(_book) = tag::field(tag, "Book") {
                out.push(RedstoneEntry::Lectern {
                    pos,
                    facts: LecternFacts {
                        page,
                        // 页数和是否有内容由书重新算，游戏层读入时补。
                        page_count: 0,
                        has_content: true,
                    },
                });
            }
        }
        _ => {}
    }
    out
}

/// `id` 是这五种有红石记忆的官方方块实体之一。
pub fn has_redstone(id: &str) -> bool {
    matches!(
        id,
        "minecraft:comparator"
            | "minecraft:chiseled_bookshelf"
            | "minecraft:jukebox"
            | "minecraft:crafter"
            | "minecraft:lectern"
    )
}
