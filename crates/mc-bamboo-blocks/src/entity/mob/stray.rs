//! 流浪者：`AbstractSkeleton` 的目标、属性和装备原样复用，只有三处不同——射出的箭带
//! 600 刻缓慢、自然生成要看得见天（细雪不算遮挡）、不会被冻（`#freeze_immune_entity_types`）。
//! 骷髅在细雪里变成流浪者的那一段在 [`super::conversion`]。
//!
//! Strays reuse the `AbstractSkeleton` goals, attributes and equipment. Three things
//! differ: their arrows carry 600 ticks of slowness, natural spawns need open sky (powder
//! snow does not count as cover) and they never freeze (`#freeze_immune_entity_types`).
//! The powder-snow conversion of skeletons lives in [`super::conversion`].

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{blocks::POWDER_SNOW, registries::mob_effect};

use crate::world::World;

/// `Stray.getArrow`：`new MobEffectInstance(MobEffects.SLOWNESS, 600)`。
pub(super) const ARROW_EFFECT: (i32, i32) = (mob_effect::SLOWNESS, 600);

/// `Stray.checkStraySpawnRules` 在 `Monster.checkMonsterSpawnRules` 之后的那一半：从上一格
/// 起跳过连续的细雪，最后一格细雪（没有细雪时就是生成格本身）要看得见天。自然生成不是
/// 刷怪笼，`EntitySpawnReason.isSpawner` 那一支不成立。
///
/// The half of `Stray.checkStraySpawnRules` after `Monster.checkMonsterSpawnRules`: skip
/// the powder snow stacked from the block above, then the last powder-snow block (the
/// spawn block itself when there is none) must see the sky. Natural spawns are not
/// spawners, so the `EntitySpawnReason.isSpawner` branch never applies.
pub(super) fn sees_sky_through_powder_snow(world: &World, pos: BlockPos) -> bool {
    let mut check = BlockPos::new(pos.x, pos.y + 1, pos.z);
    while world.block_state(check).0 == POWDER_SNOW {
        check = BlockPos::new(check.x, check.y + 1, check.z);
    }
    world
        .level()
        .can_see_sky(BlockPos::new(check.x, check.y - 1, check.z))
}
