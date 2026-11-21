//! 只在本 crate 的测试里编译的分段计时：每头牛每刻花在移动（行走、方块效果、推挤）、
//! 目标、导航和寻路上的时间。
//! 正式构建里 [`span!`] 展开成空，不占任何开销。
//!
//! Test-only split timing of a cow's tick into movement, goals, navigation and
//! pathfinding. In real builds [`span!`] expands to nothing.

#[cfg(test)]
pub(crate) mod imp {
    use std::cell::RefCell;
    use std::time::Instant;

    /// 计时的几段。每段记「自身」时间，里面嵌套的段从外层扣掉。
    ///
    /// Timed sections. Each records self time; nested sections are deducted from the outer one.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Part {
        /// 整个生物 tick 里没归到下面几段的部分（基础 tick、受伤、年龄……）。
        Other,
        /// `travel`：走、重力、碰撞。
        Travel,
        /// `applyEffectsFromBlocks`：踩到和身处的方块效果。
        BlockEffects,
        /// `pushEntities`。
        Push,
        /// 目标选择器（不含它引起的寻路）。
        Goals,
        /// 导航跟随、移动 / 视线控制（不含重算路径的寻路）。
        Navigation,
        /// `PathFinder.findPath`。
        Path,
    }

    pub(crate) const PARTS: usize = 7;

    #[derive(Default)]
    struct State {
        totals: [u64; PARTS],
        /// 打开着的段：(哪段, 开始时刻, 子段已用纳秒)。
        stack: Vec<(usize, Instant, u64)>,
    }

    thread_local! {
        static STATE: RefCell<State> = RefCell::default();
    }

    /// 离开作用域时结束一段。
    pub(crate) struct Span(());

    pub(crate) fn begin(part: Part) -> Span {
        STATE.with(|state| {
            state
                .borrow_mut()
                .stack
                .push((part as usize, Instant::now(), 0));
        });
        Span(())
    }

    impl Drop for Span {
        fn drop(&mut self) {
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                let (part, start, children) = state.stack.pop().expect("span stack");
                let elapsed = start.elapsed().as_nanos() as u64;
                state.totals[part] += elapsed.saturating_sub(children);
                if let Some(parent) = state.stack.last_mut() {
                    parent.2 += elapsed;
                }
            });
        }
    }

    /// 取走累计的自身时间（纳秒），按 [`Part`] 的顺序。
    pub(crate) fn take() -> [u64; PARTS] {
        STATE.with(|state| std::mem::take(&mut state.borrow_mut().totals))
    }
}

/// `span!(Movement)`：测试构建里计时到作用域结束，正式构建里什么也不做。
macro_rules! span {
    ($part:ident) => {
        #[cfg(test)]
        let _span =
            $crate::entity::mob::timing::imp::begin($crate::entity::mob::timing::imp::Part::$part);
    };
}

pub(crate) use span;

#[cfg(test)]
mod bench {
    use std::time::Instant;

    use mc_bamboo_core::{BlockPos, Vec3};
    use mc_bamboo_data::blocks;

    use super::imp::{self, PARTS};
    use crate::{
        BlockTicks,
        entity::{Entity, ctx::PlayerView},
        state::BlockState,
        testing::TestLevel,
        world::World,
    };

    /// 平地草方块上放 `count` 头牛，先跑 `WARM` 刻让它们散开，再量 `MEASURE` 刻。
    /// 测试世界的方块在一张散列表里，查方块比正式区块存储慢，数字偏保守。
    ///
    /// `count` cows on flat grass; warm up, then measure. The test level keeps
    /// blocks in a hash map, slower than real chunk storage, so numbers lean high.
    fn run(count: usize) {
        const WARM: usize = 200;
        const MEASURE: usize = 600;
        let side = (count as f64).sqrt().ceil() as i32;
        let spacing = 3;
        let half = side * spacing / 2 + 24;
        let mut level = TestLevel::new();
        for x in -half..half {
            for z in -half..half {
                level.set(BlockPos::new(x, 62, z), BlockState(blocks::DIRT));
                level.set(BlockPos::new(x, 63, z), BlockState(blocks::GRASS_BLOCK));
            }
        }
        // 每 32 格一名玩家站着不动：离玩家 32 格以外的牛 `noActionTime` 一直涨，
        // 到 100 就不再闲逛，那样量不到寻路。
        // A still player every 32 blocks; cows farther than 32 blocks from any
        // player stop strolling once `noActionTime` reaches 100.
        for px in (-half..half).step_by(32) {
            for pz in (-half..half).step_by(32) {
                level.players.push(PlayerView {
                    uuid: None,
                    uid: 1_000_000 + level.players.len() as i32,
                    position: Vec3::new(f64::from(px) + 16.5, 64.0, f64::from(pz) + 16.5),
                    delta: Vec3::default(),
                    eye_height: 1.62,
                    health: 20.0,
                    alive: true,
                    spectator: false,
                    creative: false,
                    sneaking: false,
                    invisible: false,
                    armor_cover: 0.0,
                    main_hand: 0,
                    off_hand: 0,
                    combat: Default::default(),
                    effects: Default::default(),
                });
            }
        }
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        for i in 0..count as i32 {
            let x = (i % side) * spacing - side * spacing / 2;
            let z = (i / side) * spacing - side * spacing / 2;
            world.spawn_entity(Entity::cow(Vec3::new(
                f64::from(x) + 0.5,
                64.0,
                f64::from(z) + 0.5,
            )));
        }
        for _ in 0..WARM {
            world.tick_entities();
        }
        imp::take();
        let start = Instant::now();
        for _ in 0..MEASURE {
            world.tick_entities();
        }
        let wall = start.elapsed().as_nanos() as f64;
        let parts = imp::take();
        let per = |ns: f64| ns / (count * MEASURE) as f64 / 1000.0;
        let names = [
            "other",
            "travel",
            "block-effects",
            "push",
            "goals",
            "navigation",
            "path",
        ];
        let timed: u64 = parts.iter().sum();
        let mut line = format!(
            "{count} cows: wall {:.3} us/cow/tick, timed {:.3}",
            per(wall),
            per(timed as f64)
        );
        for i in 0..PARTS {
            line += &format!(", {} {:.3}", names[i], per(parts[i] as f64));
        }
        println!("{line}");
    }

    /// `cargo test --release -p mc-bamboo-blocks cow_tick_cost -- --ignored --nocapture`
    #[test]
    #[ignore = "timing run, not a check"]
    fn cow_tick_cost() {
        run(100);
        run(1000);
    }
}
