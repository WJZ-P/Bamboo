//! 中键选取用的物品，官方 `BlockBehaviour.getCloneItemStack` 和它的覆盖。
//!
//! 默认是方块自己的物品（`asItem`）。覆盖要么换成别的物品，要么在物品上写
//! `block_state` 组件。`include_data` 已经按创造模式收过口，和官方
//! `hasInfiniteMaterials && packet.includeData()` 是同一个值。

use mc_bamboo_blocks::BlockState;
use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{
    Block, BlockAncestor, BlockClass, FieldValue, block_of_state, blocks, items,
    registries::data_component_type,
};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry},
    item::ItemStack,
};

use crate::{block_items, region::Region};

/// 官方 `BlockState.getCloneItemStack`。结果是空物品堆时中键没有效果。
pub(super) fn clone_item_stack(
    region: &Region,
    pos: BlockPos,
    state: BlockState,
    include_data: bool,
) -> ItemStack {
    let item = clone_item(state);
    if item == items::AIR {
        return ItemStack::empty();
    }
    let mut stack = ItemStack::new(item, 1);
    apply_components(region, pos, state, include_data, &mut stack);
    stack
}

/// 覆盖换成的物品。没有对应覆盖时是 `asItem()`，没有物品时是空气。
fn clone_item(state: BlockState) -> i32 {
    let block = state.block();
    // `CaveVinesPlantBlock` 也是 `GrowingPlantBodyBlock`，但它自己的覆盖优先。
    if state.is_a(BlockAncestor::CaveVines) {
        return items::GLOW_BERRIES;
    }
    if state.is_a(BlockAncestor::GrowingPlantBodyBlock) {
        return head_item(block);
    }
    if state.is_a(BlockAncestor::CropBlock) {
        return field_item(block, "getBaseSeedId()").unwrap_or(block.item);
    }
    match block.class {
        // 空花盆是花盆自己，种了东西的盆给里面那株植物。
        BlockClass::FlowerPotBlock => flower_pot_item(block),
        // 注册表里没有这种子时用茎方块自己，官方 `DataFixUtils.orElse(..., this)`。
        BlockClass::StemBlock | BlockClass::AttachedStemBlock => stem_seed(block),
        BlockClass::TallSeagrassBlock => item_of_state(blocks::SEAGRASS),
        BlockClass::SweetBerryBushBlock => items::SWEET_BERRIES,
        BlockClass::NetherWartBlock => items::NETHER_WART,
        BlockClass::BambooSaplingBlock => items::BAMBOO,
        BlockClass::BigDripleafStemBlock => item_of_state(blocks::BIG_DRIPLEAF),
        BlockClass::CandleCakeBlock => item_of_state(blocks::CAKE),
        BlockClass::PistonHeadBlock => piston_item(state),
        BlockClass::MovingPistonBlock
        | BlockClass::NetherPortalBlock
        | BlockClass::EndPortalBlock
        | BlockClass::EndGatewayBlock
        | BlockClass::FrostedIceBlock => items::AIR,
        _ => block.item,
    }
}

/// 覆盖写到物品上的组件。旗帜、陶罐和铜像不看 `includeData`。
fn apply_components(
    region: &Region,
    pos: BlockPos,
    state: BlockState,
    include_data: bool,
    stack: &mut ItemStack,
) {
    if state.is_a(BlockAncestor::AbstractBannerBlock) {
        // `BannerBlockEntity.getItem`：整份 `collectComponents`。
        copy_collected(region, pos, stack);
        return;
    }
    if state.class() == BlockClass::DecoratedPotBlock {
        // 只带纹样。罐子里的东西是 `addBlockDataToItem` 的事，不在这次覆盖里。
        copy_decorations(region, pos, stack);
        return;
    }
    if state.is_a(BlockAncestor::CopperGolemStatueBlock) {
        // `CopperGolemStatueBlockEntity.getItem`：先盖上组件，再把姿势写成 `block_state`。
        // 方块实体不在时官方走默认实现；放进世界的雕像都有方块实体，这里一律带上姿势。
        copy_collected(region, pos, stack);
        set_state_property(stack, state, "copper_golem_pose", false);
        return;
    }
    match state.class() {
        // `LightBlock.setLightOnStack`：只写亮度，不写含水。
        BlockClass::LightBlock => set_state_property(stack, state, "level", false),
        // `TestBlock.setModeOnStack`：合进已有的 `block_state`，没有就新建。
        BlockClass::TestBlock => set_state_property(stack, state, "mode", true),
        // 蜂蜜等级只在 `includeData` 时写。蜜蜂在后面的 `addBlockDataToItem` 里。
        BlockClass::BeehiveBlock if include_data => {
            set_state_property(stack, state, "honey_level", false);
        }
        _ => {}
    }
}

/// `GrowingPlantBodyBlock.getCloneItemStack`：头部方块的物品。
fn head_item(block: &Block) -> i32 {
    match block.field("getHeadBlock()") {
        Some(FieldValue::Block(id)) => item_of_block(id),
        _ => items::AIR,
    }
}

fn flower_pot_item(block: &Block) -> i32 {
    if matches!(block.field("isEmpty()"), Some(FieldValue::Bool(true))) {
        return block.item;
    }
    match block.field("potted") {
        Some(FieldValue::Block(id)) => item_of_block(id),
        _ => items::AIR,
    }
}

fn stem_seed(block: &Block) -> i32 {
    match block.field("seed") {
        Some(FieldValue::Item(item)) if item != items::AIR => item,
        _ => block.item,
    }
}

fn field_item(block: &Block, name: &str) -> Option<i32> {
    match block.field(name) {
        Some(FieldValue::Item(item)) => Some(item),
        _ => None,
    }
}

fn piston_item(state: BlockState) -> i32 {
    let sticky = state.property_text("type") == Some("sticky");
    item_of_state(if sticky {
        blocks::STICKY_PISTON
    } else {
        blocks::PISTON
    })
}

fn item_of_block(id: i32) -> i32 {
    usize::try_from(id)
        .ok()
        .and_then(|id| blocks::BLOCKS.get(id))
        .map(|block| block.item)
        .unwrap_or(items::AIR)
}

fn item_of_state(state: u32) -> i32 {
    block_of_state(state)
        .map(|block| block.item)
        .unwrap_or(items::AIR)
}

fn copy_collected(region: &Region, pos: BlockPos, stack: &mut ItemStack) {
    let (added, removed) = block_items::collect(region, pos).to_opaque();
    stack.added = added;
    stack.removed = removed;
}

fn copy_decorations(region: &Region, pos: BlockPos, stack: &mut ItemStack) {
    let Some(decorations) = block_items::collect(region, pos)
        .get(data_component_type::POT_DECORATIONS)
        .cloned()
    else {
        return;
    };
    let mut patch = ComponentPatch::new();
    patch.put(
        data_component_type::POT_DECORATIONS,
        PatchEntry::Set(decorations),
    );
    write_patch(stack, &patch);
}

/// `BlockItemStateProperties.with`：`merge` 为假时整份换成这一项，为真时改已有的同名属性。
fn set_state_property(stack: &mut ItemStack, state: BlockState, name: &str, merge: bool) {
    let value = state.property_text(name).unwrap_or_else(|| {
        panic!("{} has no property {name}", state.block().name);
    });
    let mut patch = stack.patch();
    let mut properties = if merge {
        match patch.get(data_component_type::BLOCK_STATE) {
            Some(ComponentValue::BlockState(existing)) => existing.clone(),
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };
    match properties.iter_mut().find(|(key, _)| key == name) {
        Some(entry) => entry.1 = value.to_owned(),
        None => properties.push((name.to_owned(), value.to_owned())),
    }
    patch.put(
        data_component_type::BLOCK_STATE,
        PatchEntry::Set(ComponentValue::BlockState(properties)),
    );
    write_patch(stack, &patch);
}

fn write_patch(stack: &mut ItemStack, patch: &ComponentPatch) {
    let (added, removed) = patch.to_opaque();
    stack.added = added;
    stack.removed = removed;
}
