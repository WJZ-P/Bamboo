//! 玩家自己的随机源，官方 `Entity.random`。
//!
//! 每个实体一份，种子由 ID 决定；铁砧损坏和附魔换种子都从它取数。

use mc_bamboo_blocks::entity::{ENTITY_RANDOM_SEED, JavaRandom};

use crate::inbound::PlayerId;

/// 玩家随机源的种子，和实体一样由 ID 决定，测试才能重放。
///
/// The player's random seed is derived from the id, same as entities, so tests can replay it.
pub(crate) fn player_random(id: PlayerId) -> JavaRandom {
    JavaRandom::new(i64::from(id.0) ^ ENTITY_RANDOM_SEED)
}
