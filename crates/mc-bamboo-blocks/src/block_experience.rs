//! `DropExperienceBlock` 的 `xpRange`，按官方 `Blocks` 静态初始化里的
//! `UniformInt.of` / `ConstantInt.of` 抄来的区间。挖掉矿石时
//! `Block.tryDropExperience` 用它从 `level.random` 取一次数，再交给
//! `ExperienceOrb.award` 拆成经验球。
//!
//! The `xpRange` of `DropExperienceBlock`, from the `UniformInt.of` /
//! `ConstantInt.of` calls in the official `Blocks` static initializer.
//! `Block.tryDropExperience` samples it once from `level.random` and passes
//! the result to `ExperienceOrb.award`.

use mc_bamboo_data::blocks;

/// 返回 `(min, max)`；常数区间写成 `(v, v)`。不在表里的方块不掉经验。
///
/// Returns `(min, max)`; a constant provider is `(v, v)`. Blocks outside the
/// table drop no experience.
pub fn experience_range(block: u32) -> Option<(i32, i32)> {
    Some(match block {
        blocks::COAL_ORE | blocks::DEEPSLATE_COAL_ORE => (0, 2),
        blocks::NETHER_GOLD_ORE => (0, 1),
        blocks::LAPIS_ORE | blocks::DEEPSLATE_LAPIS_ORE => (2, 5),
        blocks::DIAMOND_ORE | blocks::DEEPSLATE_DIAMOND_ORE => (3, 7),
        blocks::EMERALD_ORE | blocks::DEEPSLATE_EMERALD_ORE => (3, 7),
        blocks::NETHER_QUARTZ_ORE => (2, 5),
        // 官方 `SculkBlock` 用 `ConstantInt.of(1)`。
        blocks::SCULK => (1, 1),
        // 金、铁、铜矿在 26.3 里是 `ConstantInt.of(0)`：仍然调 `sample`
        // （不取随机数），也不会生成球。
        blocks::GOLD_ORE
        | blocks::DEEPSLATE_GOLD_ORE
        | blocks::IRON_ORE
        | blocks::DEEPSLATE_IRON_ORE
        | blocks::COPPER_ORE
        | blocks::DEEPSLATE_COPPER_ORE => (0, 0),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_official_ore_ranges_are_present() {
        assert_eq!(experience_range(blocks::COAL_ORE), Some((0, 2)));
        assert_eq!(experience_range(blocks::DIAMOND_ORE), Some((3, 7)));
        assert_eq!(experience_range(blocks::NETHER_QUARTZ_ORE), Some((2, 5)));
        assert_eq!(experience_range(blocks::IRON_ORE), Some((0, 0)));
        assert_eq!(experience_range(blocks::SCULK), Some((1, 1)));
        assert_eq!(experience_range(blocks::STONE), None);
    }
}
