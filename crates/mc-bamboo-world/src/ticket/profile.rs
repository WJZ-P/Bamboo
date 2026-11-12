//! `TICKET_PROFILE=1` 时给传播的各个阶段累计耗时，基准跑完打印一次。
//!
//! 平时只有一个 `enabled()` 的静态判断，不参与热路径计数；打点本身用 `Instant::now()`，
//! 所以打开后绝对值会偏慢，只用来看各阶段的比例。

use std::{
    env,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Instant,
};

/// 阶段编号，和 `LABELS` 的下标对应。
pub const LOADING_BFS: usize = 0;
pub const LOADING_SEEDS: usize = 1;
pub const LOADING_RELAX: usize = 2;
pub const LOADING_COLLECT: usize = 3;
pub const SIMULATION_BFS: usize = 4;
pub const SIMULATION_SEEDS: usize = 5;
pub const SIMULATION_RELAX: usize = 6;
pub const SIMULATION_COLLECT: usize = 7;
pub const MERGE: usize = 8;
pub const PENDING: usize = 9;
pub const FOLD: usize = 10;

pub const LABELS: [&str; 11] = [
    "加载 升高锥BFS",
    "加载 建种子",
    "加载 分桶松弛",
    "加载 收变化列表",
    "模拟 升高锥BFS",
    "模拟 建种子",
    "模拟 分桶松弛",
    "模拟 收变化列表",
    "合并两平面",
    "分流 pending",
    "折叠源变化",
];

static ENABLED: AtomicBool = AtomicBool::new(false);
static NANOS: [AtomicU64; 11] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// 每个阶段只查一次环境变量，之后走原子布尔。
#[inline]
pub fn enabled() -> bool {
    if ENABLED.load(Ordering::Relaxed) {
        return true;
    }
    let on = env::var_os("TICKET_PROFILE").is_some_and(|value| value == "1");
    if on {
        ENABLED.store(true, Ordering::Relaxed);
    }
    on
}

/// 一次计时的作用域 guard；`enabled()` 为假时不会读时钟。
pub struct Guard(Option<(usize, Instant)>);

impl Guard {
    #[inline]
    pub fn new(stage: usize) -> Self {
        if enabled() {
            Self(Some((stage, Instant::now())))
        } else {
            Self(None)
        }
    }
}

impl Drop for Guard {
    #[inline]
    fn drop(&mut self) {
        if let Some((stage, start)) = self.0 {
            NANOS[stage].fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
        }
    }
}

/// 基准预热之后清掉累计值，只量正式计时的部分。
pub fn reset() {
    for nanos in &NANOS {
        nanos.store(0, Ordering::Relaxed);
    }
}

/// 基准结束时调用，打印每个阶段平均到一次玩家移动的耗时。
pub fn report(moves: u64) {
    if !enabled() || moves == 0 {
        return;
    }
    eprintln!("TICKET_PROFILE: {moves} 次玩家移动");
    for (stage, label) in LABELS.iter().enumerate() {
        let nanos = NANOS[stage].load(Ordering::Relaxed);
        eprintln!(
            "  {label:<16} {:>8.2} µs/移动",
            nanos as f64 / moves as f64 / 1000.0
        );
    }
}
