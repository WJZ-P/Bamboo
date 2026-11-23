//! 完工后处理：区块加载等级跨入 `BLOCK_TICKING` 时消费生成期的 `PostProcessing` 表。
//!
//! 官方 `ChunkMap.prepareTickingChunk` 等半径 1 的区块都到 `FULL`，在主线程依次做
//! `LevelChunk.postProcessGeneration`、`ServerLevel.startTickingChunk`，然后才
//! `onChunkReadyToSend`。触发只看加载等级，不看模拟等级。Bamboo 里
//! [`TickLevels`](crate::tick_levels::TickLevels) 按同一条件把区块放进「待处理」，
//! 拥有这个区块的岛线程在交付之后、光照和计划刻之前调 [`prepare_ticking_chunks`]；
//! 处理完才算「可发送」，`send_chunks` 只发可发送的区块，所以客户端看不到没连好的栅栏。
//!
//! 处理本身逐条照 `postProcessGeneration`：读原状态，流体非空先 `FluidState.tick`，
//! 再按原状态分派：`LiquidBlock` 走 `BlockState.tick`，其余 `updateFromNeighbourShapes`，
//! 变了就以标志位 276（`UPDATE_NONE | UPDATE_KNOWN_SHAPE`）写回。处理完清表并标脏，
//! 存档里的表随之变空；老存档留着表的 full 区块下次变成可运行时补做。
//!
//! 半径 1 的邻居都已交付这一条件保证读写的格子都在同一个岛上：岛拆分要求两部分已交付
//! 区块的切比雪夫距离至少 3。`pendingBlockEntities` 的转正对应 Bamboo 读盘 / 组装时就
//! 建好的方块实体，没有单独的一步；`UpgradeData` 只出现在旧版本存档里，Bamboo 只读
//! 同版本存档，这里也没有。
//!
//! Generation post-processing: consume a chunk's `PostProcessing` table when its
//! loading level enters `BLOCK_TICKING` with its radius-1 ring delivered, on the
//! owning island before light, scheduled ticks and chunk sending, exactly like
//! vanilla's `prepareTickingChunk` (`postProcessGeneration`, then
//! `startTickingChunk`, then `onChunkReadyToSend`).

use mc_bamboo_core::BlockPos;

use crate::{interaction::send_effects, region::Region};

/// 对拍改坏开关：故意做错一处，确认后处理对拍会报出来。
/// Oracle break switches: corrupt one step on purpose so the oracle must report it.
#[doc(hidden)]
pub mod oracle_break {
    use std::sync::atomic::{AtomicU8, Ordering};

    /// 一种故意的错误。
    /// A deliberate corruption.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum PostProcessBreak {
        /// 漏掉一类：带流体的条目整条不处理，其余照做。
        /// Miss one kind: entries holding a fluid are skipped; the rest as usual.
        SkipFluid,
        /// 处理完不清表。
        /// Keep the table after processing.
        KeepTable,
        /// 不等处理就算可发送。
        /// Count chunks as sendable before they are processed.
        SendEarly,
    }

    /// 0 表示关；查询只是一次原子读，放在发包判断里也不加锁。
    /// 0 means off; a query is a single atomic load, lock-free on the send path.
    static BREAK: AtomicU8 = AtomicU8::new(0);

    /// 设置或清掉改坏开关（全进程，只给对拍用）。
    /// Set or clear the break switch (process-wide, oracle only).
    pub fn set_post_process_break(mode: Option<PostProcessBreak>) {
        let value = match mode {
            None => 0,
            Some(PostProcessBreak::SkipFluid) => 1,
            Some(PostProcessBreak::KeepTable) => 2,
            Some(PostProcessBreak::SendEarly) => 3,
        };
        BREAK.store(value, Ordering::Relaxed);
    }

    pub(crate) fn current() -> Option<PostProcessBreak> {
        match BREAK.load(Ordering::Relaxed) {
            1 => Some(PostProcessBreak::SkipFluid),
            2 => Some(PostProcessBreak::KeepTable),
            3 => Some(PostProcessBreak::SendEarly),
            _ => None,
        }
    }
}

/// 消费这一刻新跨入可运行的区块：按完工表做后处理、清表，然后标成可发送。
/// 待处理集合按坐标升序，齐步、精确和并行模式下同一批区块的处理顺序相同。
///
/// Process the chunks that just became ticking-ready: run the table, clear it,
/// then mark them sendable. The pending set is ordered by position, so every
/// island mode processes a batch in the same order.
pub(crate) fn prepare_ticking_chunks(region: &mut Region) {
    let pending = region.tick_levels.take_preparing();
    if pending.is_empty() {
        return;
    }
    let broken = oracle_break::current();
    for pos in pending {
        if region.chunks.is_loaded(pos) {
            let marks = crate::worldgen::post_processing_from_archive(
                region.chunks.chunk(pos).archive(),
                pos,
            );
            if !marks.is_empty() {
                let effects = {
                    let (mut level, ticks) = region.block_level();
                    let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
                    for (x, y, z) in marks {
                        let at = BlockPos::new(x, y, z);
                        if broken == Some(oracle_break::PostProcessBreak::SkipFluid)
                            && !world.block_state(at).fluid().is_empty()
                        {
                            continue;
                        }
                        world.post_process_generation_at(at);
                    }
                    world.into_effects()
                };
                send_effects(region, None, &effects);
                if broken != Some(oracle_break::PostProcessBreak::KeepTable) {
                    region.chunks.clear_post_processing(pos);
                }
            }
        }
        region.tick_levels.finish_preparing(pos);
    }
}

/// 改坏开关「不等处理就可发送」是否打开。
/// Whether the "send before processing" break is on.
pub(crate) fn send_early() -> bool {
    oracle_break::current() == Some(oracle_break::PostProcessBreak::SendEarly)
}
