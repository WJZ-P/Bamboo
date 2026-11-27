//! 战利品对拍使用的只读游戏世界夹具。
//!
//! Read-only game world fixture used by the loot oracle.

use mc_bamboo_loot::LevelView;
use mc_bamboo_protocol::text::Component;

use crate::{
    block_drops::ChunkLevel,
    region::Region,
    scoreboard::{Holder, Scoreboard},
};

/// 用真实的区域世界视图求值，计分板上只有一个已记分的持有人。
///
/// Runs against the real region view with one populated scoreboard entry.
pub fn with_world<T>(evaluate: impl FnOnce(&dyn LevelView) -> T) -> T {
    let mut region = Region::new(2, 20, false);
    let mut board = Scoreboard::default();
    assert!(board.add_objective(
        "oracle",
        "dummy",
        false,
        false,
        Component::text("Oracle")
    ));
    board.set_score(
        &Holder {
            name: "LootOracle".to_owned(),
            display: None,
        },
        "oracle",
        6,
        &mut region,
    );
    region.global.scoreboard = std::sync::Arc::new(board);
    evaluate(&ChunkLevel::new(&region.chunks, &region.global))
}

#[cfg(test)]
mod tests {
    use super::with_world;

    #[test]
    fn loot_world_scores_preserve_missing_entries() {
        with_world(|level| {
            assert_eq!(level.score("LootOracle", "oracle"), Some(Some(6)));
            assert_eq!(level.score("Unscored", "oracle"), Some(None));
            assert_eq!(level.score("LootOracle", "unknown"), Some(None));
        });
    }
}
