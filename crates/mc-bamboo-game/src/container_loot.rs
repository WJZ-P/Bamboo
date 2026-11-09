//! 带战利品表的容器：官方 `RandomizableContainer.unpackLootTable`。
//!
//! 第一次读或写容器内容之前把表求值、填进空槽，再把表清掉。随机源按官方
//! `LootContext.Builder.create` 的优先级选：非零种子走旧版随机源，表上的
//! `random_sequence` 走世界共用的序列缓存，两者都没有才用 `level.random`。
//!
//! Vanilla `RandomizableContainer.unpackLootTable`: fill the container's empty slots
//! from its table on the first read or write and clear the table. The random source
//! follows `LootContext.Builder.create`.

use std::collections::BTreeMap;

use mc_bamboo_blocks::LevelRandom;
use mc_bamboo_core::BlockPos;
use mc_bamboo_loot::{ChestParams, LootContainer, LootStack, RandomSequences, chest_loot};
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_world::store::ChunkStore;
use tracing::debug;

use crate::{
    block_drops::ChunkLevel,
    container::{ContainerEntry, ContainerKind},
    loot_stacks,
    region::Region,
};

/// `container_loot` 组件（也就是方块实体的 `LootTable` / `LootTableSeed`）解析后的内容。
///
/// The parsed `container_loot` component, i.e. a block entity's `LootTable` and seed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContainerLoot {
    pub table: String,
    pub seed: i64,
}

impl ContainerLoot {
    /// 存档形式：`{loot_table:"minecraft:chests/simple_dungeon",seed:42L}`。
    ///
    /// Archive shape: `{loot_table:"minecraft:chests/simple_dungeon",seed:42L}`.
    pub(crate) fn from_tag(tag: &mc_bamboo_nbt::Tag) -> Option<Self> {
        let mc_bamboo_nbt::Tag::Compound(entries) = tag else {
            return None;
        };
        let table = entries
            .iter()
            .find(|(name, _)| name == "loot_table")
            .and_then(|(_, value)| match value {
                mc_bamboo_nbt::Tag::String(name) => Some(name.clone()),
                _ => None,
            })?;
        let seed = entries
            .iter()
            .find(|(name, _)| name == "seed")
            .and_then(|(_, value)| match value {
                mc_bamboo_nbt::Tag::Long(seed) => Some(*seed),
                mc_bamboo_nbt::Tag::Int(seed) => Some(i64::from(*seed)),
                _ => None,
            })
            .unwrap_or(0);
        Some(Self { table, seed })
    }

    /// 写回存档形式。种子为 0 时官方省略 `seed`。
    ///
    /// Back to the archive shape. Vanilla omits a zero seed.
    pub(crate) fn to_tag(&self) -> mc_bamboo_nbt::Tag {
        let mut entries = vec![(
            "loot_table".to_owned(),
            mc_bamboo_nbt::Tag::String(self.table.clone()),
        )];
        if self.seed != 0 {
            entries.push(("seed".to_owned(), mc_bamboo_nbt::Tag::Long(self.seed)));
        }
        mc_bamboo_nbt::Tag::Compound(entries)
    }
}

/// 方块逻辑通过 `Level::unpack_loot_table` 传进来的那一组字段。
///
/// The fields block logic passes through `Level::unpack_loot_table`.
pub(crate) struct Unpack<'a> {
    pub chunks: &'a ChunkStore,
    pub containers: &'a mut BTreeMap<BlockPos, ContainerEntry>,
    pub sequences: &'a RandomSequences,
    pub random: &'a LevelRandom,
}

/// 把一张表的产出写进容器槽位。
pub(crate) struct LevelSlots<'a> {
    pub(crate) slots: &'a mut [ItemStack],
}

impl LootContainer for LevelSlots<'_> {
    fn slot_count(&self) -> usize {
        self.slots.len()
    }

    fn is_slot_empty(&self, slot: usize) -> bool {
        self.slots[slot].is_empty()
    }

    fn set_slot(&mut self, slot: usize, stack: LootStack) {
        self.slots[slot] = loot_stacks::loot_stack(&stack);
    }
}

/// 解包这一格的表。返回是否真的填过内容。
///
/// 官方 `unpackLootTable` 先清表再 `LootTable.fill`，所以只要求值开始（哪怕表里
/// 没有池或者缺世界信息）表也会消失。
///
/// Unpacks one cell. Vanilla clears the table before `LootTable.fill`, so it stays
/// cleared even when the table is empty or a condition cannot be answered.
pub(crate) fn unpack(state: Unpack<'_>, pos: BlockPos) -> bool {
    let Some(entry) = state.containers.get_mut(&pos) else {
        return false;
    };
    let Some(table) = entry.loot_table.take() else {
        return false;
    };
    let seed = std::mem::replace(&mut entry.loot_table_seed, 0);
    let view = ChunkLevel {
        chunks: state.chunks,
    };
    let params = ChestParams {
        pos: (pos.x, pos.y, pos.z),
        player: None,
        luck: 0.0,
        level: &view,
    };
    let mut slots = LevelSlots {
        slots: &mut entry.slots,
    };
    if let Err(error) = chest_loot(
        &table,
        &mut slots,
        &params,
        seed,
        Some(state.sequences),
        state.random.source(),
    ) {
        debug!(
            %error,
            table,
            x = pos.x,
            y = pos.y,
            z = pos.z,
            "skipping container loot: the table needs data this world does not have"
        );
    }
    true
}

/// `Region` 上的入口：菜单打开、破坏和容器表读写共用。
pub(crate) fn unpack_at(region: &mut Region, pos: BlockPos) {
    let state = Unpack {
        chunks: &region.chunks,
        containers: &mut region.containers,
        sequences: &region.loot_sequences,
        random: &region.random,
    };
    unpack(state, pos);
}

/// 把物品上的 `container_loot` 或方块实体的 `LootTable` 放进这一格的容器条目。
pub(crate) fn set(
    containers: &mut BTreeMap<BlockPos, ContainerEntry>,
    pos: BlockPos,
    kind: ContainerKind,
    loot: Option<ContainerLoot>,
) {
    if loot.is_none() && !containers.contains_key(&pos) {
        return;
    }
    let entry = crate::container::entry_in(containers, pos, kind);
    match loot {
        Some(loot) => {
            entry.loot_table = Some(loot.table);
            entry.loot_table_seed = loot.seed;
        }
        None => {
            entry.loot_table = None;
            entry.loot_table_seed = 0;
        }
    }
}

/// 还没解包的表。中键选取方块时按官方 `collectImplicitComponents` 写回物品。
pub(crate) fn pending(entry: &ContainerEntry) -> Option<ContainerLoot> {
    entry.loot_table.as_ref().map(|table| ContainerLoot {
        table: table.clone(),
        seed: entry.loot_table_seed,
    })
}
