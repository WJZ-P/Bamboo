use super::{Duration, Instant, World, thread_cpu_ms};
use mc_bamboo_core::ChunkPos;
use mc_bamboo_game::pipeline_bench;

#[derive(Clone, Copy, Debug)]
pub(super) enum Mode {
    Reply,
    Paced,
}

#[derive(Default)]
struct Settle {
    delivered: usize,
    ticks: usize,
    no_reply_waits: usize,
    wait: Duration,
}

fn settle(world: &mut World, positions: &[ChunkPos], mode: Mode) -> Settle {
    let mut stats = Settle::default();
    let mut next_tick = Instant::now();
    let deadline = next_tick + Duration::from_secs(1800);
    loop {
        let before_wait = Instant::now();
        match mode {
            Mode::Reply => {
                if !world.pipeline_bench_wait(Duration::from_millis(100)) {
                    stats.no_reply_waits += 1;
                }
            }
            Mode::Paced => {
                std::thread::sleep(next_tick.saturating_duration_since(before_wait));
                next_tick += Duration::from_millis(50);
            }
        }
        stats.wait += before_wait.elapsed();
        stats.delivered += world.tick_unpaced();
        stats.ticks += 1;
        if positions.iter().all(|&pos| world.is_chunk_loaded(pos)) && world.compute_in_flight() == 0
        {
            return stats;
        }
        assert!(Instant::now() < deadline, "pipeline did not settle");
    }
}

pub(super) fn warm(world: &mut World, positions: &[ChunkPos]) {
    world.force_chunks_unpaced(positions);
    settle(world, positions, Mode::Reply);
}

pub(super) fn run(
    world: &mut World,
    positions: &[ChunkPos],
    threads: usize,
    label: &str,
    mode: Mode,
) {
    let cpu_before = thread_cpu_ms();
    let (empty_before, job_before) = world.pipeline_bench_pool_stats();
    let started = Instant::now();
    pipeline_bench::start();
    world.force_chunks_unpaced(positions);
    let stats = settle(world, positions, mode);
    let wall = started.elapsed();
    let (empty, job) = world.pipeline_bench_pool_stats();
    let fields = pipeline_bench::stop().fields();
    let world_cpu_ms = thread_cpu_ms() - cpu_before;
    let work = wall - stats.wait;
    let other = work - fields.iter().map(|(_, time)| *time).sum::<Duration>();
    let worker_capacity = wall * u32::try_from(threads).expect("worker count");
    let empty = empty - empty_before;
    let job = job - job_before;
    let residual = worker_capacity.saturating_sub(empty + job);
    let n = stats.delivered as f64;
    let per_chunk = |d: Duration| 1000.0 * d.as_secs_f64() / n;
    print!(
        "{label},mode={mode:?},threads={threads},requested={},delivered={},ticks={},\
         no_reply_waits={},wall_s={:.6},delivered_per_s={:.3},world_work_ms={:.4},\
         world_other_ms={:.4},world_cpu_ms={:.4},world_wait_ms={:.4}",
        positions.len(),
        stats.delivered,
        stats.ticks,
        stats.no_reply_waits,
        wall.as_secs_f64(),
        n / wall.as_secs_f64(),
        per_chunk(work),
        per_chunk(other),
        world_cpu_ms / n,
        per_chunk(stats.wait),
    );
    for (label, time) in fields {
        print!(",{label}_ms={:.4}", per_chunk(time));
    }
    println!(
        ",pool_job_ms={:.4},pool_empty_ms={:.4},pool_residual_ms={:.4},\
         pool_job_pct={:.3},pool_empty_pct={:.3}",
        per_chunk(job),
        per_chunk(empty),
        per_chunk(residual),
        job.as_secs_f64() / worker_capacity.as_secs_f64() * 100.0,
        empty.as_secs_f64() / worker_capacity.as_secs_f64() * 100.0,
    );
}
