//! `DropExperienceBlock` 的 `xpRange`，按官方 `Blocks` 静态初始化里的
//! `UniformInt.of` / `ConstantInt.of` 抄来的区间。挖掉矿石时
//! `Block.tryDropExperience` 用它从 `level.random` 取一次数，再交给
//! `ExperienceOrb.award` 拆成经验球。
//!
//! The `xpRange` of `DropExperienceBlock`, from the `UniformInt.of` /
//! `ConstantInt.of` calls in the official `Blocks` static initializer.
//! `Block.tryDropExperience` samples it once from `level.random` and passes
//! the result to `ExperienceOrb.award`.

use std::sync::OnceLock;

use mc_bamboo_data::{block_id_of_state, blocks};

/// 方块注册表 ID → `(min, max)`；常数区间写成 `(v, v)`。
/// 不在表里的方块不掉经验。
///
/// Block registry id → `(min, max)`; a constant provider is `(v, v)`.
/// Blocks outside the table drop no experience.
pub fn experience_range(block: u32) -> Option<(i32, i32)> {
    static RANGES: OnceLock<Vec<Option<(i32, i32)>>> = OnceLock::new();
    let ranges = RANGES.get_or_init(|| {
        let mut out = vec![None; blocks::BLOCKS.len()];
        let mut put = |state: u32, range: (i32, i32)| {
            if let Some(id) = block_id_of_state(state)
                && let Some(slot) = out.get_mut(id)
            {
                *slot = Some(range);
            }
        };
        // 官方 `Blocks` 静态初始化：煤矿 0..2，下界金矿 0..1，
        // 青金石 / 下界石英 2..5，钻石 / 绿宝石 3..7，
        // 金 / 铁 / 铜矿 ConstantInt.of(0)，幽匿块 ConstantInt.of(1)。
        for state in [blocks::COAL_ORE, blocks::DEEPSLATE_COAL_ORE] {
            put(state, (0, 2));
        }
        put(blocks::NETHER_GOLD_ORE, (0, 1));
        for state in [blocks::LAPIS_ORE, blocks::DEEPSLATE_LAPIS_ORE] {
            put(state, (2, 5));
        }
        for state in [blocks::DIAMOND_ORE, blocks::DEEPSLATE_DIAMOND_ORE] {
            put(state, (3, 7));
        }
        for state in [blocks::EMERALD_ORE, blocks::DEEPSLATE_EMERALD_ORE] {
            put(state, (3, 7));
        }
        put(blocks::NETHER_QUARTZ_ORE, (2, 5));
        put(blocks::SCULK, (1, 1));
        // 26.3 里金、铁、铜矿是 `ConstantInt.of(0)`：仍然调 `sample`
        // （不取随机数），也不会生成球。
        for state in [
            blocks::GOLD_ORE,
            blocks::DEEPSLATE_GOLD_ORE,
            blocks::IRON_ORE,
            blocks::DEEPSLATE_IRON_ORE,
            blocks::COPPER_ORE,
            blocks::DEEPSLATE_COPPER_ORE,
        ] {
            put(state, (0, 0));
        }
        out
    });
    usize::try_from(block)
        .ok()
        .and_then(|index| ranges.get(index))
        .copied()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(state: u32) -> Option<(i32, i32)> {
        experience_range(block_id_of_state(state).unwrap() as u32)
    }

    #[test]
    fn the_official_ore_ranges_are_present() {
        assert_eq!(of(blocks::COAL_ORE), Some((0, 2)));
        assert_eq!(of(blocks::DIAMOND_ORE), Some((3, 7)));
        assert_eq!(of(blocks::NETHER_QUARTZ_ORE), Some((2, 5)));
        assert_eq!(of(blocks::IRON_ORE), Some((0, 0)));
        assert_eq!(of(blocks::SCULK), Some((1, 1)));
        assert_eq!(of(blocks::STONE), None);
    }
}
