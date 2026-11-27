//! 带战利品表的容器：官方 `RandomizableContainer.unpackLootTable`。
//!
//! 第一次读或写容器内容之前把表求值、填进空槽，再把表清掉。随机源按官方
//! `LootContext.Builder.create` 的优先级选：非零种子走旧版随机源，表上的
//! `random_sequence` 走世界共用的序列缓存，两者都没有才用 `level.random`。
//!
//! Vanilla `RandomizableContainer.unpackLootTable`: fill the container's empty slots
//! from its table on the first read or write and clear the table. The random source
//! follows `LootContext.Builder.create`.

use mc_bamboo_blocks::{LevelRandom, entity::Entities};
use mc_bamboo_core::BlockPos;
use mc_bamboo_loot::{
    ChestParams, EntityView, LootContainer, LootStack, RandomSequences, chest_loot,
};
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
    pub containers: &'a mut crate::pos_map::PosMap<ContainerEntry>,
    pub sequences: &'a RandomSequences,
    pub random: &'a LevelRandom,
    pub global: &'a crate::global::GlobalView,
}

/// 方块逻辑通过 `Level::unpack_entity_container_loot_table` 传进来的字段。
///
/// The fields block logic passes through `Level::unpack_entity_container_loot_table`.
pub(crate) struct EntityContainerUnpack<'a> {
    pub entities: &'a mut Entities,
    pub chunks: &'a ChunkStore,
    pub sequences: &'a RandomSequences,
    pub random: &'a LevelRandom,
    pub global: &'a crate::global::GlobalView,
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
    let view = ChunkLevel::new(state.chunks, state.global);
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

/// `ContainerEntity.unpackChestVehicleLootTable(player)`：第一次读内容（破坏、
/// 菜单打开、漏斗读取）先求值填进 27 格、清表，再返回解包后的物品。菜单打开
/// 带玩家（`THIS_ENTITY` + `getLuck()`），容器读写和破坏传 `None` / 0。
/// 没有表时返回 `None`，调用方用原本的 `itemStacks`。
///
/// `ContainerEntity.unpackChestVehicleLootTable(player)`: on the first read
/// (destruction, opening the menu, a hopper pulling) fill the 27 slots, clear
/// the table and return the filled slots. Opening a menu passes the player
/// (`THIS_ENTITY` + `getLuck()`); container reads and destruction pass
/// `None` / 0. `None` means there was no table.
pub(crate) fn unpack_entity_container(
    region: &mut Region,
    entity: i32,
    player: Option<EntityView>,
    luck: f32,
) -> Option<Vec<ItemStack>> {
    unpack_entity_container_with(
        EntityContainerUnpack {
            entities: &mut region.entities,
            chunks: &region.chunks,
            sequences: &region.loot_sequences,
            random: &region.random,
            global: &region.global,
        },
        entity,
        player,
        luck,
    )
}

/// 方块逻辑（漏斗读实体容器）走这一份：借的是 `Region` 拆开的字段，不是整个
/// `Region`。`player` / `luck` 在这里恒为空和零，对应
/// `unpackChestVehicleLootTable(null)`。
///
/// The split-field entry for block logic (a hopper reading the entity container);
/// `player` / `luck` are none / zero here, matching
/// `unpackChestVehicleLootTable(null)`.
pub(crate) fn unpack_entity_container_with(
    state: EntityContainerUnpack<'_>,
    entity: i32,
    player: Option<EntityView>,
    luck: f32,
) -> Option<Vec<ItemStack>> {
    let position = state
        .entities
        .public_at(entity)
        .map(|public| public.position)?;
    // `ContainerEntity.unpackChestVehicleLootTable(null)`：运输船和容器矿车共用。
    // `ContainerEntity.unpackChestVehicleLootTable(null)`: shared by chest boats
    // and container carts.
    let (table, seed, mut items) = state
        .entities
        .with_mut(entity, |e| match &mut e.kind {
            mc_bamboo_blocks::entity::EntityKind::Boat(boat) => {
                let chest = boat.chest.as_mut()?;
                let table = chest.loot_table.take()?;
                let seed = std::mem::replace(&mut chest.loot_table_seed, 0);
                Some((table, seed, chest.items.clone()))
            }
            mc_bamboo_blocks::entity::EntityKind::Minecart(cart) if cart.is_container() => {
                let table = cart.loot_table.take()?;
                let seed = std::mem::replace(&mut cart.loot_table_seed, 0);
                Some((table, seed, cart.items.clone()))
            }
            _ => None,
        })
        .flatten()?;
    let view = ChunkLevel::new(state.chunks, state.global);
    let params = ChestParams {
        pos: (
            mc_bamboo_core::mth::floor(position.x),
            mc_bamboo_core::mth::floor(position.y),
            mc_bamboo_core::mth::floor(position.z),
        ),
        player,
        luck,
        level: &view,
    };
    let mut slots = LevelSlots { slots: &mut items };
    let _ = chest_loot(
        &table,
        &mut slots,
        &params,
        seed,
        Some(state.sequences),
        state.random.source(),
    );
    // 把解包结果写回实体（实体可能已被 discard，这时写不进去也不影响掉落）。
    state.entities.with_mut(entity, |e| match &mut e.kind {
        mc_bamboo_blocks::entity::EntityKind::Boat(boat) => {
            if let Some(chest) = &mut boat.chest {
                chest.items = items.clone();
            }
        }
        mc_bamboo_blocks::entity::EntityKind::Minecart(cart)
            if cart.is_container() && cart.items.len() == items.len() =>
        {
            cart.items.clone_from_slice(&items);
        }
        _ => {}
    });
    Some(items)
}

/// `Region` 上的入口：菜单打开、破坏和容器表读写共用。
pub(crate) fn unpack_at(region: &mut Region, pos: BlockPos) {
    let state = Unpack {
        chunks: &region.chunks,
        containers: &mut region.containers,
        sequences: &region.loot_sequences,
        random: &region.random,
        global: &region.global,
    };
    unpack(state, pos);
}

/// 把物品上的 `container_loot` 或方块实体的 `LootTable` 放进这一格的容器条目。
pub(crate) fn set(
    containers: &mut crate::pos_map::PosMap<ContainerEntry>,
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
