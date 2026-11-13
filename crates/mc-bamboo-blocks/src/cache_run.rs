//! 带红石缓存的触发执行。
//!
//! 现有驱动循环原样不变；带缓存的循环在每条触发边界调 [`run_cached_trigger`]：
//! 先问一次缓存，不划算的点直接解释执行，连录制包装都不开；划算的点先守卫，
//! 命中就按记录顺序套用效果，未命中就打开录制包装解释执行并收成 [`RuntimeRecord`]。
//! 守卫阶段零副作用，命中不了不留半套效果。
//!
//! 守卫在 `World` 层做，因为它同时借着 `Level` 和 `BlockTicks`
//! （`has_scheduled_tick` / `will_tick_this_tick` 不属于 `Level`）。

use mc_bamboo_core::BlockPos;

use crate::{
    Level,
    cache::{RedstoneCache, ReplayEffect, RuntimeRecord, guard::GuardRead, guard::ReadMethod},
    level::{RecordMode, Trigger},
    recording::ChunkSection,
    state::BlockState,
    ticks::BlockTicks,
    world::World,
};

/// 守卫重算：计划刻查询由 `BlockTicks` 给，其余读项由 `Level` 给。
fn guard_value(
    read: &GuardRead,
    level: &dyn Level,
    ticks: &BlockTicks,
    trigger_state: BlockState,
) -> Option<u64> {
    let hash = match read.method {
        ReadMethod::Block | ReadMethod::Trigger => crate::recording::stable_hash(&trigger_state.0),
        ReadMethod::MinY => crate::recording::stable_hash(&level.min_y()),
        ReadMethod::MaxY => crate::recording::stable_hash(&level.max_y()),
        ReadMethod::Comparator(pos) => crate::recording::stable_hash(&level.comparator_output(pos)),
        ReadMethod::TickQuery(pos, kind, will_tick_this_tick) => {
            let result = if will_tick_this_tick {
                ticks.will_tick_this_tick(pos, kind)
            } else {
                ticks.has_scheduled_tick(pos, kind)
            };
            crate::recording::stable_hash(&(kind, will_tick_this_tick, result))
        }
        ReadMethod::Dedup(kind, fluid) => {
            // 守卫不能只看 hash：这一格这一种刻现在是不是已经排着，要重查队列。
            let pos = read.pos?;
            let existed = if fluid {
                ticks.has_scheduled_fluid(pos, i32::try_from(kind).ok()?)
            } else {
                ticks.has_scheduled_tick(pos, usize::try_from(kind).ok()?)
            };
            return (read.value == crate::recording::stable_hash(&existed)).then_some(read.value);
        }
        ReadMethod::BlockEventDedup(event) => {
            let existed = level.block_event_queued(event);
            return (read.value == crate::recording::stable_hash(&existed)).then_some(read.value);
        }
        ReadMethod::Unsupported(_) => return None,
    };
    Some(hash)
}

/// 在**同一个 `World`** 里跑一条触发：命中就套用效果，否则解释并按需录制。
///
/// `world` 的 `level` 常驻一层 [`crate::recording::Recording`]，但记账模式默认 `Off`：
/// 不划算的触发点只付一次查表 + 一次虚调用。试探中的点开 `Count`（只累加一个
/// 计数器），确认划算的点才开 `Full`，产出一条运行时记录。
pub fn run_cached_trigger(
    world: &mut World<'_>,
    cache: &mut RedstoneCache,
    trigger: Trigger,
    now: i64,
    interpret: impl FnOnce(&mut World<'_>),
) {
    if !cacheable_trigger(trigger) {
        interpret(world);
        return;
    }
    cache.set_time(now);
    if !cache.admits(trigger) {
        // 常驻开销：一次查表，连 RecordMode 都不用切（驱动循环已经设成 Off）。
        interpret(world);
        return;
    }

    // 守卫和重放都不记账；解释执行前再按试探结果打开。
    world.set_record_mode(RecordMode::Off);
    let effects = {
        let level = world.level();
        let ticks = world.ticks_ref();
        // 触发点这一格的状态只在守卫真的要逐项复核时才读：绝大多数触发点没有
        // 记录可查，不该为它们付这一次读。
        let pos = trigger_position(trigger);
        let mut state = None;
        let mut guard = |read: &GuardRead| {
            let state = *state.get_or_insert_with(|| level.block_state(pos));
            guard_value(read, level, ticks, state)
        };
        let section = |s: ChunkSection| level.section_hash(s.chunk, s.section_y);
        cache.try_replay(trigger, &mut guard, &section)
    };
    if let Some(effects) = effects {
        apply_effects(world, &effects);
        return;
    }

    // 试探阶段：用上一次 `Count` 得到的读写项数问缓存要不要开录。
    // 活跃的点每次都开 Full；试探的点按累计工作量决定。
    let mode = if cache.begin_probe() {
        RecordMode::Full
    } else {
        RecordMode::Count
    };
    world.set_record_mode(mode);
    if mode == RecordMode::Full {
        world.begin_trigger(trigger);
    }
    interpret(world);
    if mode == RecordMode::Full {
        world.end_trigger();
        if let Some(record) = world.take_runtime_records().into_iter().next() {
            cache.record(&record);
        }
        // 下一条触发可能是「不划算、直接解释」的点，记账必须已经关掉。
        world.set_record_mode(RecordMode::Off);
    } else {
        let counted = world.take_counted_reads();
        world.set_record_mode(RecordMode::Off);
        cache.note_probe_work(counted);
    }
}

/// 按记录顺序落地效果。底层写入不触发邻居更新，也不报观察事件。
fn apply_effects(world: &mut World<'_>, effects: &[ReplayEffect]) {
    for effect in effects {
        match *effect {
            ReplayEffect::Block {
                pos,
                state,
                flags,
                applied,
            } => {
                if applied {
                    world.replay_block(pos, state, flags);
                }
            }
            ReplayEffect::Comparator { pos, signal } => world.replay_comparator(pos, signal),
            ReplayEffect::BlockEvent(event) => world.replay_block_event(event),
            ReplayEffect::BlockEventPacket {
                pos,
                block,
                param_a,
                param_b,
            } => world.push_effect(crate::level::Effect::BlockEvent {
                pos,
                block,
                param_a,
                param_b,
            }),
            ReplayEffect::ScheduledTick {
                pos,
                block,
                delay,
                priority,
            } => world.replay_scheduled_tick(pos, block, delay, priority),
            ReplayEffect::ScheduledFluidTick { pos, fluid, delay } => {
                world.replay_scheduled_fluid_tick(pos, fluid, delay);
            }
        }
    }
}

/// `Trigger` 的位置。
pub const fn trigger_position(trigger: Trigger) -> BlockPos {
    match trigger {
        Trigger::ScheduledTick { pos, .. }
        | Trigger::ScheduledFluidTick { pos, .. }
        | Trigger::BlockEvent { pos, .. }
        | Trigger::BlockEntityTick { pos, .. } => pos,
    }
}

/// 第一版只接方块计划刻和方块事件。
pub const fn cacheable_trigger(trigger: Trigger) -> bool {
    matches!(
        trigger,
        Trigger::ScheduledTick { .. } | Trigger::BlockEvent { .. }
    )
}

/// 守卫用的探针记录，供缓存的 `try_replay` 之外做诊断。
pub fn probe_record(trigger: Trigger) -> RuntimeRecord {
    RuntimeRecord {
        trigger,
        tick: 0,
        nanos: 0,
        interpreted_items: 0,
        written_items: 0,
        reads: Vec::new(),
        effects: Vec::new(),
        sections: Vec::new(),
        uncacheable: Vec::new(),
        boundary_writes: false,
        rejected: false,
    }
}
