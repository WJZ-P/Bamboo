//! 吃草场景的覆盖检查：官方那一侧确实发生了场景想测的事。
//!
//! Coverage checks for the grazing scenes: the official side really did what the scene
//! is meant to exercise.

/// 一格方块在某一刻换了状态。
///
/// One cell changing state on one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub tick: usize,
    pub pos: [i32; 3],
    pub to: u32,
}

/// 官方逐刻方块状态里的全部变化，按时间顺序。
///
/// Every change in the official per-tick block states, in time order.
pub fn changes(blocks: &[[i32; 4]], steps: &[Vec<u32>]) -> Vec<Change> {
    let mut found = Vec::new();
    for (tick, pair) in steps.windows(2).enumerate() {
        for ((cell, before), after) in blocks.iter().zip(&pair[0]).zip(&pair[1]) {
            if before != after {
                found.push(Change {
                    tick: tick + 1,
                    pos: [cell[0], cell[1], cell[2]],
                    to: *after,
                });
            }
        }
    }
    found
}

/// `sheep_eat_plant` 的本意：同一列先吃掉脚下的植物，之后再吃它下面的草方块。
///
/// What `sheep_eat_plant` is for: one column loses the plant at the feet first and the
/// grass block under it later.
pub fn plant_then_block(found: &[Change]) -> bool {
    found.iter().any(|plant| {
        found.iter().any(|block| {
            block.tick > plant.tick && block.pos == [plant.pos[0], plant.pos[1] - 1, plant.pos[2]]
        })
    })
}

/// 场景没有覆盖到本意时的报错；别的场景不检查。
///
/// The error for a scene that missed its purpose; other scenes are not checked.
pub fn coverage_error(name: &str, blocks: &[[i32; 4]], steps: &[Vec<u32>]) -> Option<String> {
    let found = changes(blocks, steps);
    match name {
        "sheep_eat_plant" if !plant_then_block(&found) => Some(format!(
            "{name}: no column lost its plant and then its grass block; changes {found:?}"
        )),
        "sheep_eat" if found.is_empty() => Some(format!("{name}: no block changed")),
        "sheep_eat_no_grief" if !found.is_empty() => Some(format!(
            "{name}: blocks changed with mobGriefing off: {found:?}"
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCKS: [[i32; 4]; 2] = [[0, 63, 0, 9], [0, 64, 0, 2048]];

    #[test]
    fn plant_then_block_needs_both_in_order() {
        let both = [vec![9, 2048], vec![9, 0], vec![10, 0]];
        assert_eq!(coverage_error("sheep_eat_plant", &BLOCKS, &both), None);
        let plant_only = [vec![9, 2048], vec![9, 0]];
        assert!(coverage_error("sheep_eat_plant", &BLOCKS, &plant_only).is_some());
        let block_first = [vec![9, 2048], vec![10, 2048], vec![10, 0]];
        assert!(coverage_error("sheep_eat_plant", &BLOCKS, &block_first).is_some());
    }

    #[test]
    fn no_grief_scene_must_not_change_blocks() {
        let changed = [vec![9, 2048], vec![10, 2048]];
        assert!(coverage_error("sheep_eat_no_grief", &BLOCKS, &changed).is_some());
        assert_eq!(coverage_error("other", &BLOCKS, &changed), None);
    }
}
