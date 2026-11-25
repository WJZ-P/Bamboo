//! 非方块物品自己的 `Item.useOn`：方块没有接住右键时，手上的物品对点中的方块做什么。
//!
//! | 官方类 | 做什么 |
//! |---|---|
//! | `Item` 带 `block_transformer` 组件 | 斧子去皮、刮铜、去蜡，锄头耕地，锹压土径，见 [`transformer`] |
//! | `FlintAndSteelItem`、`FireChargeItem` | 点燃营火、蜡烛和蜡烛蛋糕，或者在点中的面前生火，见 [`ignite`] |
//! | `HoneycombItem` | 给铜涂蜡 |
//! | `ShearsItem` | 让还在长的藤蔓头、海带头停止生长 |
//! | `EnderEyeItem` | 装进末地传送门框架，一圈装满时生成传送门 |
//! | `DebugStickItem` | 管理员右键改属性，见 [`debug_stick`] |
//! | `BrushItem` | 开始刷，刷出东西在后面的使用刻里，不在范围里 |
//! | `BoneMealItem` | 催熟方块，或者在水里长海草，见 [`bone_meal`] |
//!
//! 别的物品类的 `useOn` 要生成实体、看物品组件，或者只改手上的物品（刷怪蛋、矿车、指南针这些），
//! 这里返回 `Pass`。`mc-bamboo-vanilla` 的 oracle 排除表列着它们和原因。
//!
//! 创造模式的数量由游戏层在动作结束后改回去。手上有玩家时，`hurtAndBreak` 和 `shrink` 在官方的位置执行。
//! 音效、掉落物和游戏事件仍不做，骨粉的粒子发给玩家。
//!
//! Creative mode restores the count in the game layer after the action. When a player is installed, `hurtAndBreak` and `shrink` run where vanilla runs them.
//! Sounds, drops and game events are still omitted. Bone meal particles are sent to the player.

mod bone_meal;
pub(crate) use bone_meal::{grow_crop, grow_water_plant};
mod debug_stick;
mod ender_eye;
mod honeycomb;
pub(crate) use honeycomb::waxed;
mod ignite;
mod lead;
pub(crate) use ignite::can_light;
mod shears;
mod spawn_egg;
mod transformer;

pub use debug_stick::{StickSelect, select as debug_stick_select};

use mc_bamboo_data::{Item, ItemKind, items};

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
        ItemKind::DebugStick => debug_stick::use_on(world, ctx),
        // `BrushItem.useOn` 只开始使用物品，结果总是 `CONSUME`。
        ItemKind::Brush => InteractionResult::Consume,
        ItemKind::BoneMeal => bone_meal::use_on(world, ctx),
        _ if ctx.item == items::LEAD => lead::use_on(world, ctx),
        // `SpawnEggItem.useOn`：被点格的碰撞形状为空时生成在那一格，否则沿点击面
        // 挪一格；流体方块直接放过。落点和 `shouldOffsetYMore` 见 `spawn_egg::use_egg`。
        // `SpawnEggItem.useOn`: spawn in the clicked cell when its collision shape is
        // empty, otherwise one cell along the clicked face; fluid cells are refused.
        _ if ctx.item == items::COW_SPAWN_EGG => spawn_egg::cow(world, ctx),
        _ if ctx.item == items::POLAR_BEAR_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::PolarBear)
        }
        _ if ctx.item == items::MOOSHROOM_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Mooshroom)
        }
        _ if ctx.item == items::OCELOT_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Ocelot)
        }
        _ if ctx.item == items::CAT_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Cat)
        }
        _ if ctx.item == items::RABBIT_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Rabbit)
        }
        _ if ctx.item == items::FOX_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Fox)
        }
        _ if ctx.item == items::WOLF_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Wolf)
        }
        _ if ctx.item == items::HORSE_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Horse)
        }
        _ if ctx.item == items::DONKEY_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Donkey)
        }
        _ if ctx.item == items::MULE_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Mule)
        }
        _ if ctx.item == items::LLAMA_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Llama)
        }
        _ if ctx.item == items::ARMADILLO_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Armadillo)
        }
        _ if ctx.item == items::GOAT_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Goat)
        }
        _ if ctx.item == items::FROG_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Frog)
        }
        _ if ctx.item == items::TADPOLE_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Tadpole)
        }
        _ if ctx.item == items::BAT_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Bat)
        }
        _ if ctx.item == items::PHANTOM_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Phantom)
        }
        _ if ctx.item == items::PIG_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Pig)
        }
        _ if ctx.item == items::SHEEP_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Sheep)
        }
        _ if ctx.item == items::CHICKEN_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Chicken)
        }
        _ if ctx.item == items::SQUID_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Squid)
        }
        _ if ctx.item == items::GLOW_SQUID_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::GlowSquid)
        }
        _ if ctx.item == items::COD_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Cod)
        }
        _ if ctx.item == items::SALMON_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Salmon)
        }
        _ if ctx.item == items::TROPICAL_FISH_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::TropicalFish)
        }
        _ if ctx.item == items::PUFFERFISH_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Pufferfish)
        }
        _ if ctx.item == items::VILLAGER_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::Villager)
        }
        _ if ctx.item == items::WANDERING_TRADER_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::WanderingTrader)
        }
        _ if ctx.item == items::TRADER_LLAMA_SPAWN_EGG => {
            spawn_egg::use_egg(world, ctx, crate::entity::mob::MobType::TraderLlama)
        }
        _ if ctx.item == items::ZOMBIE_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Zombie)
        }
        _ if ctx.item == items::SKELETON_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Skeleton)
        }
        _ if ctx.item == items::CREEPER_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Creeper)
        }
        _ if ctx.item == items::HUSK_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Husk)
        }
        _ if ctx.item == items::PARCHED_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Parched)
        }
        _ if ctx.item == items::CAMEL_HUSK_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::CamelHusk)
        }
        _ if ctx.item == items::STRAY_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Stray)
        }
        _ if ctx.item == items::BOGGED_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Bogged)
        }
        _ if ctx.item == items::SLIME_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Slime)
        }
        _ if ctx.item == items::DROWNED_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Drowned)
        }
        _ if ctx.item == items::GUARDIAN_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Guardian)
        }
        _ if ctx.item == items::ELDER_GUARDIAN_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::ElderGuardian)
        }
        _ if ctx.item == items::SULFUR_CUBE_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::SulfurCube)
        }
        _ if ctx.item == items::NAUTILUS_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Nautilus)
        }
        _ if ctx.item == items::ZOMBIE_NAUTILUS_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::ZombieNautilus)
        }
        _ if ctx.item == items::WITCH_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Witch)
        }
        _ if ctx.item == items::PILLAGER_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Pillager)
        }
        _ if ctx.item == items::SPIDER_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Spider)
        }
        _ if ctx.item == items::ENDERMAN_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::Enderman)
        }
        _ if ctx.item == items::CAVE_SPIDER_SPAWN_EGG => {
            spawn_egg::monster(world, ctx, crate::entity::mob::MobType::CaveSpider)
        }
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
