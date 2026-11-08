//! 非方块物品自己的 `Item.useOn`：方块没有接住右键时，手上的物品对点中的方块做什么。
//!
//! | 官方类 | 做什么 |
//! |---|---|
//! | `Item` 带 `block_transformer` 组件 | 斧子去皮、刮铜、去蜡，锄头耕地，锹压土径，见 [`transformer`] |
//! | `FlintAndSteelItem`、`FireChargeItem` | 点燃营火、蜡烛和蜡烛蛋糕，或者在点中的面前生火，见 [`ignite`] |
//! | `HoneycombItem` | 给铜涂蜡 |
//! | `ShearsItem` | 让还在长的藤蔓头、海带头停止生长 |
//! | `EnderEyeItem` | 装进末地传送门框架，一圈装满时生成传送门 |
//! | `DebugStickItem` | 要 gamemaster 权限，这个世界里总是失败 |
//! | `BrushItem` | 开始刷，刷出东西在后面的使用刻里，不在范围里 |
//! | `BoneMealItem` | 催熟方块，或者在水里长海草，见 [`bone_meal`] |
//!
//! 别的物品类的 `useOn` 要生成实体、看物品组件，或者只改手上的物品（刷怪蛋、矿车、指南针这些），
//! 这里返回 `Pass`。`mc-bamboo-vanilla` 的 oracle 排除表列着它们和原因。
//!
//! 全部按创造模式写。官方 `ServerPlayerGameMode.useItemOn` 在 `hasInfiniteMaterials` 时把数量改回去，
//! 也不按结果换手上的物品；这里的物品都不在 `useOn` 里自己换物品，所以只剩世界的变化。
//! 耐久、音效、掉落物和游戏事件都不做，骨粉的粒子发给玩家。

mod bone_meal;
mod ender_eye;
mod honeycomb;
mod ignite;
mod shears;
mod transformer;

use mc_bamboo_data::{Item, ItemKind};

use crate::{context::UseContext, interact::InteractionResult, world::World};

/// `Item.useOn` 按物品类分派。方块物品不走这里，见 `placement::use_on`。
pub(crate) fn use_on(world: &mut World, item: &Item, ctx: &UseContext) -> InteractionResult {
    match item.kind {
        ItemKind::FlintAndSteel => ignite::flint_and_steel(world, ctx),
        ItemKind::FireCharge => ignite::fire_charge(world, ctx),
        ItemKind::Honeycomb => honeycomb::use_on(world, ctx),
        ItemKind::Shears => {
            shears::use_on(world, ctx).unwrap_or_else(|| default_use_on(world, item, ctx))
        }
        ItemKind::EnderEye => ender_eye::use_on(world, ctx),
        // `DebugStickItem.handleInteraction` 先看 `canUseGameMasterBlocks`，Bamboo 没有权限系统。
        ItemKind::DebugStick => InteractionResult::Fail,
        // `BrushItem.useOn` 只开始使用物品，结果总是 `CONSUME`。
        ItemKind::Brush => InteractionResult::Consume,
        ItemKind::BoneMeal => bone_meal::use_on(world, ctx),
        _ => default_use_on(world, item, ctx),
    }
}

/// `Item.useOn` 的默认实现：只看 `block_transformer` 组件。
fn default_use_on(world: &mut World, item: &Item, ctx: &UseContext) -> InteractionResult {
    item.block_transformer
        .map_or(InteractionResult::Pass, |rules| {
            transformer::transform_block(world, rules, ctx)
        })
}
