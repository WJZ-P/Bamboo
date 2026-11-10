//! 钩子开销基准：不套录制包装，跑一万刻，打印每刻耗时。
//!
//! 用法：`cargo run --release -p mc-bamboo-hit-rate --bin bench -- [重复次数]`
//! 在加钩子前后的提交上各跑几次，比较最小值。

use mc_bamboo_hit_rate::machines;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rounds = args
        .first()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(5);
    let ticks = 10_000_i64;
    for (name, build) in [
        ("中继器时钟", machines::repeater_clock as fn() -> _),
        ("红石线阵列", machines::redstone_wire_array),
    ] {
        let mut best = u128::MAX;
        for _ in 0..rounds {
            let mut m = build();
            let started = std::time::Instant::now();
            for now in 1..=ticks {
                m.plain_step(now);
            }
            best = best.min(started.elapsed().as_nanos());
        }
        println!(
            "{name}: {:.3} ms / {ticks} ticks ({:.1} ns/tick)",
            best as f64 / 1e6,
            best as f64 / ticks as f64
        );
    }
}
