//! 方块实体自己的存档键和按 `id` 造回具体类型的工厂。
//!
//! 官方 `BlockEntity.saveAdditional` 由 [`BlockEntity::write_additional`] 实现；容器、战利品表、
//! 名字、锁、合成器、讲台、命令方块这些在 `mc-bamboo-game` 的表里，由那一层补进同一个条目。
//! 这里只写六种会动的方块实体真正持有的键，以及 `Items`（`ItemStackWithSlot.CODEC`）。

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{BlockAncestor, blocks, registries, registries::block_entity_type as kinds};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::archive::slots::{self, DecodedItems, SlottedItem},
    item::ItemStack,
};

use super::tag::{self, Error, Result};
use crate::{
    BlockEntity, BlockState,
    behaviour::machines::{
        BrewingStandEntity, CampfireEntity, FurnaceEntity, PistonMoving, ShulkerBoxEntity,
    },
    hopper::HopperEntity,
};

/// Bamboo 会实例化的方块实体类型。读盘只认这些；其它 `id` 由调用方整条原样保留。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockEntityKind {
    Furnace,
    BlastFurnace,
    Smoker,
    BrewingStand,
    Hopper,
    ShulkerBox,
    Campfire,
    Piston,
}

impl BlockEntityKind {
    /// 按 NBT 里的 `id` 找类型。`minecraft:block_entity_type` 的注册名。
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "minecraft:furnace" => Self::Furnace,
            "minecraft:blast_furnace" => Self::BlastFurnace,
            "minecraft:smoker" => Self::Smoker,
            "minecraft:brewing_stand" => Self::BrewingStand,
            "minecraft:hopper" => Self::Hopper,
            "minecraft:shulker_box" => Self::ShulkerBox,
            "minecraft:campfire" => Self::Campfire,
            "minecraft:soul_campfire" => Self::Campfire,
            "minecraft:piston" => Self::Piston,
            _ => return None,
        })
    }

    pub fn from_id(id: i32) -> Option<Self> {
        registries::block_entity_type::name_of(id).and_then(Self::from_name)
    }

    /// 官方写进 `id` 的注册名。
    pub fn name(self) -> &'static str {
        match self {
            Self::Furnace => "minecraft:furnace",
            Self::BlastFurnace => "minecraft:blast_furnace",
            Self::Smoker => "minecraft:smoker",
            Self::BrewingStand => "minecraft:brewing_stand",
            Self::Hopper => "minecraft:hopper",
            Self::ShulkerBox => "minecraft:shulker_box",
            Self::Campfire => "minecraft:campfire",
            Self::Piston => "minecraft:piston",
        }
    }

    /// 每种类型各造一次，测试用它盯住工厂分支不要漏。
    pub const ALL: [Self; 8] = [
        Self::Furnace,
        Self::BlastFurnace,
        Self::Smoker,
        Self::BrewingStand,
        Self::Hopper,
        Self::ShulkerBox,
        Self::Campfire,
        Self::Piston,
    ];
}

/// 官方 `BlockEntity.loadStatic` 的 Bamboo 版：按 `id` 造具体方块实体，再读自己的键。
///
/// `state` 用来选熔炉的配方种类；坏数据一律返回 [`Error`]，不 panic。
pub fn create_block_entity(
    id: &str,
    pos: BlockPos,
    _state: BlockState,
    tag: &Tag,
) -> Result<Option<Box<dyn BlockEntity>>> {
    let Some(kind) = BlockEntityKind::from_name(id) else {
        return Ok(None);
    };
    Ok(Some(match kind {
        BlockEntityKind::Furnace | BlockEntityKind::BlastFurnace | BlockEntityKind::Smoker => {
            let cooking = match kind {
                BlockEntityKind::Furnace => mc_bamboo_data::CookingKind::Smelting,
                BlockEntityKind::BlastFurnace => mc_bamboo_data::CookingKind::Blasting,
                _ => mc_bamboo_data::CookingKind::Smoking,
            };
            let mut entity = FurnaceEntity::new(pos, cooking);
            entity.set_data(0, tag::int_or(tag, "lit_time_remaining", 0));
            entity.set_data(1, tag::int_or(tag, "lit_total_time", 0));
            entity.set_data(2, tag::int_or(tag, "cooking_time_spent", 0));
            entity.set_data(3, tag::int_or(tag, "cooking_total_time", 0));
            let speed = tag::float_or(tag, "speed_multiplier", 1.0);
            let recipes = read_recipes_used(tag)?;
            entity.set_archive_values(speed, recipes);
            read_items_into(tag, entity.items().len(), |slot, stack| {
                entity.write_item(slot, stack);
            })?;
            Box::new(entity)
        }
        BlockEntityKind::BrewingStand => {
            let mut entity = BrewingStandEntity::new(pos);
            entity.set_data(0, tag::int_or(tag, "BrewTime", 0));
            entity.set_data(2, tag::int_or(tag, "total_brew_time", 400));
            entity.set_data(1, tag::int_or(tag, "Fuel", 0));
            entity.set_data(3, tag::int_or(tag, "total_fuel", 20));
            entity.set_speed_multiplier(tag::float_or(tag, "speed_multiplier", 1.0));
            read_items_into(tag, entity.items().len(), |slot, stack| {
                entity.set_item(slot, stack);
            })?;
            Box::new(entity)
        }
        BlockEntityKind::Hopper => {
            let mut entity = HopperEntity::new(pos);
            entity.set_cooldown_from_archive(tag::int_or(tag, "TransferCooldown", -1));
            Box::new(entity)
        }
        BlockEntityKind::ShulkerBox | BlockEntityKind::Campfire => {
            if kind == BlockEntityKind::ShulkerBox {
                Box::new(ShulkerBoxEntity::new(pos)) as Box<dyn BlockEntity>
            } else {
                let mut entity = CampfireEntity::new(pos);
                read_items_into(tag, 4, |slot, stack| entity.set_item(slot, stack))?;
                let times = tag::int_array(tag, "CookingTimes");
                let totals = tag::int_array(tag, "CookingTotalTimes");
                for slot in 0..8 {
                    let value = if slot < 4 {
                        times.get(slot).copied().unwrap_or(0)
                    } else {
                        totals.get(slot - 4).copied().unwrap_or(0)
                    };
                    entity.set_data(slot, value);
                }
                Box::new(entity)
            }
        }
        BlockEntityKind::Piston => {
            let moved = match tag::field(tag, "blockState") {
                Some(value) => tag::decode_block_state(value)?,
                None => BlockState::AIR,
            };
            let direction = Direction::from_id(i32::from(tag::byte_or(tag, "facing", 0)));
            let extending = tag::bool_or(tag, "extending", false);
            let source = tag::bool_or(tag, "source", false);
            let mut entity = PistonMoving::new(pos, moved, direction, extending, source);
            entity.set_archive_progress(tag::float_or(tag, "progress", 0.0));
            Box::new(entity)
        }
    }))
}

/// 一个会动的方块实体写入自己的键。容器、名字、锁等由游戏层补。
pub fn write_block_entity_additional(entity: &dyn BlockEntity) -> Result<Vec<(String, Tag)>> {
    entity.write_additional()
}

/// 官方 `ContainerHelper.saveAllItems`：`Items` 是带 `Slot` 的列表。
/// `keep_empty` 为真时空列表也写 `[]`（潜影盒之外都是），否则不写这个键。
pub fn items_tag(items: &[ItemStack]) -> Result<Tag> {
    let slotted: Vec<SlottedItem> = items
        .iter()
        .enumerate()
        .filter(|(_, stack)| !stack.is_empty())
        .map(|(slot, stack)| SlottedItem {
            slot: slot as i32,
            stack: stack.clone(),
        })
        .collect();
    slots::encode_list(&slotted).map_err(|error| Error::new(error.to_string()))
}

/// 官方 `ContainerHelper.loadAllItems`：越界槽位和坏条目跳过，重复的槽位后一条覆盖前一条。
/// 空容器不写 `Items` 时 `tag::field` 返回 `None`，按空列表处理。
pub fn read_items(tag: &Tag, size: usize) -> Result<Vec<ItemStack>> {
    let mut out = vec![ItemStack::empty(); size];
    read_items_into(tag, size, |slot, stack| out[slot] = stack)?;
    Ok(out)
}

fn read_items_into(tag: &Tag, size: usize, mut emit: impl FnMut(usize, ItemStack)) -> Result<()> {
    let empty = Tag::List(Vec::new());
    let list = tag::field(tag, "Items").unwrap_or(&empty);
    let DecodedItems { items, problems: _ } = slots::decode_list(list, size);
    for item in items {
        emit(item.slot as usize, item.stack);
    }
    Ok(())
}

/// `RecipesUsed` 是 `unboundedMap(Recipe.KEY_CODEC, Codec.INT)`，值只收配方表里的 ID。
fn read_recipes_used(tag: &Tag) -> Result<Vec<(&'static str, i32)>> {
    let Some(Tag::Compound(values)) = tag::field(tag, "RecipesUsed") else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (name, value) in values {
        let Some(recipe) = mc_bamboo_recipes::RecipeManager::global().recipe(name) else {
            continue;
        };
        out.push((recipe.id, tag::java_i32(value)?));
    }
    Ok(out)
}

/// 官方 `CrafterBlockEntity` 的禁用槽掩码、倒计时和 `triggered`（第二段由游戏层调用）。
pub fn crafter_disabled_slots(mask: u16) -> Tag {
    Tag::IntArray((0..9).filter(|slot| mask & (1 << slot) != 0).collect())
}

/// 官方 `Direction.LEGACY_ID_CODEC`，0..5 与 `Direction` 的判别值一致。
pub fn legacy_direction(tag: &Tag) -> Direction {
    Direction::from_id(i32::from(tag::byte_or(tag, "facing", 0)))
}

/// `id` 必须是 `minecraft:block_entity_type` 里的合法名字。
pub fn valid_block_entity_id(name: &str) -> bool {
    registries::block_entity_type::by_name(name).is_some()
}

/// `state` 是否属于 `kind` 允许的方块，读盘时用来决定要不要丢掉原样保留的条目。
pub fn state_matches(state: BlockState, id: &str) -> bool {
    let Some(kind) = registries::block_entity_type::by_name(id) else {
        return false;
    };
    match kind {
        kinds::FURNACE | kinds::BLAST_FURNACE | kinds::SMOKER => {
            state.is(blocks::FURNACE) || state.is(blocks::BLAST_FURNACE) || state.is(blocks::SMOKER)
        }
        kinds::BREWING_STAND => state.is(blocks::BREWING_STAND),
        kinds::HOPPER => state.is(blocks::HOPPER),
        kinds::SHULKER_BOX => state
            .class()
            .ancestors()
            .contains(&BlockAncestor::ShulkerBoxBlock),
        kinds::CAMPFIRE => state.is(blocks::CAMPFIRE) || state.is(blocks::SOUL_CAMPFIRE),
        kinds::PISTON => state.is(blocks::MOVING_PISTON),
        _ => true,
    }
}
