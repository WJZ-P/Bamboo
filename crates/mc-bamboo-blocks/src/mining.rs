//! 生存模式的挖掘进度，照官方的 `BlockBehaviour.getDestroyProgress` 和 `Player.getDestroySpeed`。
//!
//! 官方的进度是 `getDestroyProgress(state) * (挖了几刻 + 1)`，见 `ServerPlayerGameMode`。
//! 这里只算每一刻的基础进度，攒进度和判断阈值在 `mc-bamboo-game`。
//! 没有状态效果，所以急迫和挖掘疲劳都按没有算。

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{block_id_of_state, block_of_state, items, registries::attribute_defaults};

use crate::{
    fluid::Fluid,
    level::Level,
    state::{FluidState, SupportType},
};

/// `Player.getDestroySpeed` 读到的几样东西。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Miner {
    /// 主手物品的注册表 ID，空手是 [`items::AIR`]。
    pub main_hand: i32,
    /// `onGround()`。客户端移动包里的值。
    pub on_ground: bool,
    /// `isEyeInFluid(FluidTags.WATER)`。
    pub eye_in_water: bool,
}

/// 玩家的挖掘速度，官方的 `Player.getDestroySpeed`。
///
/// 附魔和状态效果都还没有：`MINING_EFFICIENCY` 只是属性的默认值，
/// `MobEffectUtil.hasDigSpeed` 和 `MINING_FATIGUE` 在这里是恒假。
pub fn destroy_speed(state: u32, miner: Miner) -> f32 {
    let block = block_id_of_state(state).unwrap_or_default() as i32;
    let item = &items::ITEMS[miner.main_hand.max(0) as usize];
    let mut speed = item.tool.map_or(1.0, |tool| tool.mining_speed(block));
    if speed > 1.0 {
        speed += attribute_defaults::MINING_EFFICIENCY as f32;
    }
    speed *= attribute_defaults::BLOCK_BREAK_SPEED as f32;
    if miner.eye_in_water {
        speed *= attribute_defaults::SUBMERGED_MINING_SPEED as f32;
    }
    if !miner.on_ground {
        speed /= 5.0;
    }
    speed
}

/// 挖一下增加多少进度，官方的 `BlockBehaviour.getDestroyProgress`。
///
/// 硬度是 `-1`（挖不动）时是 0。带对了工具时除以 30，否则除以 100。
pub fn destroy_progress(state: u32, miner: Miner) -> f32 {
    let hardness = block_of_state(state).map_or(-1.0, |block| block.destroy_time);
    if hardness == -1.0 {
        return 0.0;
    }
    let modifier = if is_correct_for_drops(state, miner.main_hand) {
        30.0
    } else {
        100.0
    };
    destroy_speed(state, miner) / hardness / modifier
}

/// 手里这件工具挖这块方块会不会有掉落物，官方的 `Player.hasCorrectToolForDrops`。
///
/// 掉落物本身还没有，但进度快慢取决于它。
pub fn is_correct_for_drops(state: u32, main_hand: i32) -> bool {
    let Some(block) = block_of_state(state) else {
        return false;
    };
    if !block.requires_correct_tool_for_drops {
        return true;
    }
    let block_id = block_id_of_state(state).unwrap_or_default() as i32;
    let tool = &items::ITEMS[main_hand.max(0) as usize];
    tool.tool
        .is_some_and(|tool| tool.is_correct_for_drops(block_id))
}

/// 眼睛是不是泡在水里，官方的 `Entity.isEyeInFluid(FluidTags.WATER)`。
///
/// 官方扫描整个碰撞箱找眼所在的那一列，这里只检查眼睛所在的那一格。
/// 水面高度用的是 `FluidState.getHeightForCamera`。
pub fn is_eye_in_water(level: &dyn Level, eye: Vec3) -> bool {
    let pos = BlockPos::new(
        eye.x.floor() as i32,
        eye.y.floor() as i32,
        eye.z.floor() as i32,
    );
    let fluid = level.block_state(pos).fluid();
    if !fluid.is_water() {
        return false;
    }
    let height = camera_height(fluid, level, pos);
    eye.y >= f64::from(pos.y) && eye.y <= f64::from(pos.y) + f64::from(height)
}

/// `FluidState.getHeightForCamera`：源头上面是完整的底面时算满格，否则是流体的高度。
fn camera_height(fluid: FluidState, level: &dyn Level, pos: BlockPos) -> f32 {
    if fluid.is_source()
        && level
            .block_state(pos.above())
            .is_face_sturdy(Direction::Down, SupportType::Full)
    {
        1.0
    } else {
        fluid_height(fluid, level, pos)
    }
}

/// `FluidState.getHeight`：上面一格是同一种流体时是满格。
fn fluid_height(fluid: FluidState, level: &dyn Level, pos: BlockPos) -> f32 {
    match Fluid::of(fluid) {
        Some(kind) if kind.is_same(level.block_state(pos.above()).fluid()) => 1.0,
        Some(_) => fluid.own_height(),
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, items};

    use super::*;
    use crate::{BlockState, testing::TestLevel};

    fn miner(item: i32) -> Miner {
        Miner {
            main_hand: item,
            on_ground: true,
            eye_in_water: false,
        }
    }

    #[test]
    fn unbreakable_blocks_have_no_progress() {
        assert_eq!(
            destroy_progress(blocks::BEDROCK, miner(items::DIAMOND_PICKAXE)),
            0.0
        );
        assert_eq!(destroy_progress(blocks::BEDROCK, miner(items::AIR)), 0.0);
    }

    #[test]
    fn a_correct_tool_is_thirty_times_faster_than_a_wrong_one() {
        let stone = blocks::STONE;
        // 钻石镐的挖掘速度是 8，石头硬度 1.5，带对了工具除以 30。
        assert_eq!(
            destroy_progress(stone, miner(items::DIAMOND_PICKAXE)),
            8.0 / 1.5 / 30.0
        );
        // 空手速度是 1，石头不带镐没有掉落物，除以 100。
        assert_eq!(
            destroy_progress(stone, miner(items::AIR)),
            1.0 / 1.5 / 100.0
        );
        // 剑没有镐的规则，速度还是 1。
        assert_eq!(
            destroy_progress(stone, miner(items::DIAMOND_SWORD)),
            destroy_progress(stone, miner(items::AIR))
        );
    }

    #[test]
    fn blocks_that_do_not_need_a_tool_always_count_as_correct() {
        assert!(is_correct_for_drops(blocks::DIRT, items::AIR));
        assert!(is_correct_for_drops(blocks::DIRT, items::DIAMOND_SWORD));
        assert!(!is_correct_for_drops(blocks::STONE, items::DIAMOND_SWORD));
        assert!(is_correct_for_drops(blocks::STONE, items::DIAMOND_PICKAXE));
    }

    #[test]
    fn water_halves_and_the_air_quarters_the_speed() {
        let mut level = TestLevel::new();
        let pos = BlockPos::new(0, 64, 0);
        level.set(pos, BlockState(blocks::WATER));
        let eye = Vec3::new(0.5, 64.5, 0.5);
        assert!(is_eye_in_water(&level, eye));
        assert!(!is_eye_in_water(&level, Vec3::new(0.5, 65.5, 0.5)));

        let dry = destroy_speed(blocks::STONE, miner(items::AIR));
        let wet = destroy_speed(
            blocks::STONE,
            Miner {
                eye_in_water: true,
                ..miner(items::AIR)
            },
        );
        let airborne = destroy_speed(
            blocks::STONE,
            Miner {
                on_ground: false,
                ..miner(items::AIR)
            },
        );
        assert_eq!(wet, dry * attribute_defaults::SUBMERGED_MINING_SPEED as f32);
        assert_eq!(airborne, dry / 5.0);
    }

    #[test]
    fn a_source_with_a_full_block_above_reaches_the_top() {
        let mut level = TestLevel::new();
        let pos = BlockPos::new(0, 64, 0);
        level.set(pos, BlockState(blocks::WATER));
        level.set(pos.above(), BlockState(blocks::STONE));
        // 源头上面的水面是满格，眼睛在水面处还泡着。
        assert!(is_eye_in_water(&level, Vec3::new(0.5, 64.999, 0.5)));
        assert!(!is_eye_in_water(&level, Vec3::new(0.5, 65.0, 0.5)));
    }
}

#[cfg(test)]
mod official;
