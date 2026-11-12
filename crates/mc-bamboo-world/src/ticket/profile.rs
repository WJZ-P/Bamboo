//! `TICKET_PROFILE=1` 时给传播的各个阶段累计耗时，基准跑完打印一次。
//!
//! 平时每阶段和每格计数只有一次 `enabled()` 的原子布尔判断（环境变量只查一次），
//! 不读时钟也不做原子加法；打点本身用 `Instant::now()`，所以打开后绝对值会偏慢，
//! 只用来看各阶段的比例和每轮被访问的格数。

use std::{
    env,
    sync::atomic::{AtomicU8, AtomicU64, Ordering},
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

/// 计数器下标，和 `COUNTER_LABELS` 对应。
pub const C_BFS_VISIT: usize = 0;
pub const C_BFS_RESET: usize = 1;
pub const C_SEED_NEIGHBOR: usize = 2;
pub const C_RELAX_PUSH: usize = 3;
pub const C_RELAX_POP: usize = 4;
pub const C_RELAX_UPDATE: usize = 5;
pub const C_COLLECT_SCAN: usize = 6;
pub const C_CHANGES: usize = 7;

pub const COUNTER_LABELS: [&str; 8] = [
    "BFS 访问格",
    "BFS 清空格",
    "建种子查邻居",
    "松弛入队",
    "松弛出队",
    "松弛真更新",
    "收变化扫描格",
    "输出变化数",
];

static COUNTS: [AtomicU64; 8] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// 只在 `TICKET_PROFILE=1` 时计数，普通构建里就是一个原子布尔判断。
#[inline]
pub fn count(stage: usize, by: u64) {
    if enabled() {
        COUNTS[stage].fetch_add(by, Ordering::Relaxed);
    }
}

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

/// 0 = 还没查过环境变量，1 = 关，2 = 开。计数器在热循环里调用 `enabled()`，
/// 每次都去查环境变量会贵到失真，所以查一次就缓存。
static ENABLED: AtomicU8 = AtomicU8::new(0);
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
    match ENABLED.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let on = env::var_os("TICKET_PROFILE").is_some_and(|value| value == "1");
            ENABLED.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
    }
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
    for count in &COUNTS {
        count.store(0, Ordering::Relaxed);
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
    for (stage, label) in COUNTER_LABELS.iter().enumerate() {
        let total = COUNTS[stage].load(Ordering::Relaxed);
        eprintln!("  {label:<16} {:>8.1} 次/移动", total as f64 / moves as f64);
    }
}
