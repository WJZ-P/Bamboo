//! `TestBlock`：游戏测试用的方块，要权限才能放和打开。世界里没有信号，只剩到期的计划刻。

use mc_bamboo_core::BlockPos;

use crate::{
    behaviour::Behaviour,
    state::{BlockState, TEST_BLOCK_MODE, TestBlockMode},
    world::World,
};

/// `TestBlock`。
pub(super) struct TestBlock;

impl Behaviour for TestBlock {
    /// 方块实体的 `reset`：`start` 模式的方块把信号收回来，要告诉邻居。
    fn tick(&self, state: BlockState, world: &mut World, pos: BlockPos) {
        if state.get(TEST_BLOCK_MODE) == TestBlockMode::Start {
            world.update_neighbors_at(pos, state.block());
        }
    }
}
