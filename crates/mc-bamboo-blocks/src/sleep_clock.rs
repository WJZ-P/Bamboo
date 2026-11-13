//! 事件休眠的安静刻公式：一段睡眠里数值按线性增量推进，到点真跑一次 `tick`。
//!
//! 熔炉是两条字段，营火是四格进度，酿造台是一条倒计时；它们共用这里的
//! 基准时刻、到点时刻和「按公式算到 `now`」的骨架。

use std::array;

/// 一段睡眠的基准：`since` 那一刻的数值已经是真跑完的值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sleep {
    pub since: i64,
    pub wake_at: i64,
}

impl Sleep {
    /// 从 `now` 起排一段到 `wake_at` 的睡眠。`i64::MAX` 表示等到外部叫醒。
    pub fn new(now: i64, wake_at: i64) -> Self {
        let wake_at = if wake_at == i64::MAX {
            i64::MAX
        } else {
            wake_at.max(now.saturating_add(1))
        };
        Self {
            since: now,
            wake_at,
        }
    }

    /// 到 `tick` 为止跨过了几个安静刻。到点那一刻之前就停，让 `tick` 自己加这一刻。
    pub fn steps(&self, tick: i64) -> i64 {
        (tick
            .min(self.wake_at.saturating_sub(1))
            .saturating_sub(self.since))
        .max(0)
    }

    /// 读路径看到的是 `now` 这一刻也真跑过之后的值，所以不截到 `wake_at - 1`。
    pub fn read_steps(&self, now: i64) -> i64 {
        now.saturating_sub(self.since).max(0)
    }
}

/// `i32` 的线性增量。正数按官方越界也继续加（营火找不到配方时进度一直涨），
/// 负数回落到 `[0, total]`（营火 / 熔炉的 `Mth.clamp`）。
pub fn advance(base: i32, step: i32, steps: i64, total: i32) -> i32 {
    let steps = i32::try_from(steps).unwrap_or(i32::MAX);
    if step < 0 {
        base.saturating_sub((-step).saturating_mul(steps))
            .clamp(0, total.max(0))
    } else {
        base.saturating_add(step.saturating_mul(steps))
    }
}

/// 四格进度一起按公式推进。营火点燃时每格 +1、熄灭时每格 -2。
/// 点燃那一支的 `total` 只是回落用的上限，不加到进度上。
pub fn advance_four<const N: usize>(
    base: [i32; N],
    step: [i32; N],
    steps: i64,
    total: [i32; N],
) -> [i32; N] {
    array::from_fn(|slot| {
        if step[slot] < 0 {
            advance(base[slot], step[slot], steps, total[slot])
        } else {
            advance(base[slot], step[slot], steps, i32::MAX)
        }
    })
}

/// 外部写入、存档回读、装盒之前把睡眠结束，数值按公式算到「第 `tick` 刻结束时」。
/// `tick` 用 `now - 1` 时，这一刻的增量留给真跑的 `tick` 自己加。
pub fn materialize<const N: usize>(
    sleep: &mut Option<Sleep>,
    base: &mut [i32; N],
    step: [i32; N],
    total: [i32; N],
    tick: i64,
) {
    let Some(armed) = sleep.take() else {
        return;
    };
    *base = advance_four(*base, step, armed.steps(tick), total);
}

/// 读路径按公式算到 `now`，不改实体本身。
pub fn values_at<const N: usize>(
    sleep: Option<Sleep>,
    base: [i32; N],
    step: [i32; N],
    total: [i32; N],
    now: i64,
) -> [i32; N] {
    match sleep {
        Some(armed) => advance_four(base, step, armed.read_steps(now), total),
        None => base,
    }
}

/// 单条数值的线性推进。酿造台倒计时只减到 0，不按总时间夹取。
pub fn advance_unclamped(base: i32, step: i32, steps: i64) -> i32 {
    let steps = i32::try_from(steps).unwrap_or(i32::MAX);
    base.saturating_add(step.saturating_mul(steps))
}

/// 单条数值的睡眠推进；`step` 为负时回落到 0。
pub fn materialize_one(sleep: &mut Option<Sleep>, base: &mut i32, step: i32, tick: i64) {
    let Some(armed) = sleep.take() else {
        return;
    };
    *base = advance_unclamped(*base, step, armed.steps(tick)).max(0);
}

/// 单条数值的读路径；到点那一刻一定已经真跑过，所以不会读到负数。
pub fn value_at(sleep: Option<Sleep>, base: i32, step: i32, now: i64) -> i32 {
    match sleep {
        Some(armed) => advance_unclamped(base, step, armed.read_steps(now)).max(0),
        None => base,
    }
}
