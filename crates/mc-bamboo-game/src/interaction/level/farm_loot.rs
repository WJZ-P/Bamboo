//! 农场动物的战利品参数：羊的颜色 / 剪毛状态、鸡的花纹，以及剪毛和下蛋的实体战利品表。
//!
//! Loot parameters for farm animals: sheep colour and shear state, chicken variant, and the
//! shearing and egg-laying entity loot tables.

use mc_bamboo_blocks::entity::mob::{CowVariant, EntityLootRequest, MobType, sheep};
use mc_bamboo_protocol::item::ItemStack;
use serde_json::Value;

use super::{RegionLevel, game_stack_to_loot, loot_position};

/// 给实体谓词补上这种动物会被查的组件和 `type_specific` 字段。
///
/// Adds the components and `type_specific` fields loot predicates read for this animal.
pub(super) fn describe(
    view: &mut mc_bamboo_loot::EntityView,
    kind: MobType,
    wool: u8,
    variant: Option<CowVariant>,
) {
    match kind {
        MobType::Sheep => {
            let color = mc_bamboo_core::sign::DyeColor::from_id(i32::from(wool & 15));
            view.is_sheep = true;
            view.sheep_sheared = wool & sheep::SHEARED_BIT != 0;
            view.components.insert(
                "minecraft:sheep/color".to_owned(),
                Value::String(color.name().to_owned()),
            );
        }
        MobType::Chicken => {
            if let Some(variant) = variant {
                view.components.insert(
                    "minecraft:chicken/variant".to_owned(),
                    Value::String(variant.name().to_owned()),
                );
            }
        }
        _ => {}
    }
}

/// `dropFromShearingLootTable` / `dropFromGiftLootTable` 的求值，随机源和死亡掉落一样
/// 先找表的随机序列。
///
/// Rolls a shearing or gift table; like death loot it uses the table's random sequence.
pub(super) fn entity_loot_drops(
    level: &mut RegionLevel<'_>,
    request: EntityLootRequest,
) -> Vec<ItemStack> {
    let mut entity = mc_bamboo_loot::EntityView::new(request.kind.entity_type());
    entity.is_living = true;
    entity.position = Some(loot_position(request.origin));
    describe(
        &mut entity,
        request.kind,
        request.wool,
        Some(request.variant),
    );
    let view = crate::block_drops::ChunkLevel {
        chunks: level.chunks,
        weather: level.block_weather,
    };
    let params = mc_bamboo_loot::EntityLootParams {
        entity,
        origin: loot_position(request.origin),
        tool: request.tool.as_ref().map(game_stack_to_loot),
        level: Some(&view),
    };
    match mc_bamboo_loot::entity_loot_drops(
        request.table,
        &params,
        Some(level.loot_sequences),
        level.random.source(),
    ) {
        Ok(stacks) => stacks.iter().map(crate::loot_stacks::loot_stack).collect(),
        Err(error) => {
            tracing::debug!(%error, table = request.table, "skipping entity loot");
            Vec::new()
        }
    }
}
