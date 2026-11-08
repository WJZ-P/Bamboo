//! [`mc_bamboo_blocks::Level`] 的实现：方块逻辑通过它读写区块、记下改动、看玩家挡不挡路。

use std::{collections::BTreeMap, slice};

use mc_bamboo_blocks::{
    Aabb, BlockState, Level, LevelRandom,
    shape::{self, BooleanOp},
};
use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::blocks;
use mc_bamboo_world::{MIN_Y, store::ChunkStore};

use super::{BlockChanges, MAX_Y};
use crate::{inbound::PlayerId, player::Player};

/// 一次方块操作借用的 [`Region`](crate::region::Region) 字段。
pub(crate) struct RegionLevel<'a> {
    pub(crate) chunks: &'a mut ChunkStore,
    pub(crate) changes: &'a mut BlockChanges,
    pub(crate) players: &'a BTreeMap<PlayerId, Player>,
    pub(crate) random: &'a LevelRandom,
}

impl Level for RegionLevel<'_> {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        BlockState(self.chunks.block(pos).unwrap_or(blocks::VOID_AIR))
    }

    fn write_block(&mut self, pos: BlockPos, state: BlockState) -> bool {
        let written = self.chunks.set_block(pos, state.0);
        if written {
            self.changes.changed.push(pos);
        }
        written
    }

    /// 发起者自己也算：客户端预测失败时也会发这个包，不查的话方块会放进玩家身体里。
    fn is_unobstructed(&self, boxes: &[Aabb]) -> bool {
        !self.players.values().any(|player| {
            let body = player.bounding_box();
            shape::join_is_not_empty(boxes, slice::from_ref(&body), BooleanOp::And)
        })
    }

    fn min_y(&self) -> i32 {
        MIN_Y
    }

    fn max_y(&self) -> i32 {
        MAX_Y
    }

    fn random(&self) -> &LevelRandom {
        self.random
    }

    fn is_randomly_ticking_section(&self, chunk: ChunkPos, section_y: i32) -> bool {
        self.chunks.is_randomly_ticking_section(chunk, section_y)
    }

    /// 这个世界只有创造模式，没有旁观者。
    fn any_player_close_enough_to(&self, pos: BlockPos, max_distance: i32) -> bool {
        let corner = pos.corner();
        self.players
            .values()
            .any(|player| player.position.distance_to_sqr(corner).sqrt() < f64::from(max_distance))
    }
}
