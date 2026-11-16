//! 客户端一侧的「区块进入视距 → 区块包到达」测量。
//!
//! 服务端的 `chunk_latency_us` 只量待发送集合里的排队时间，不含读盘、生成、交付。
//! 这里按假玩家自己的位置和视距算出 `T(vd)`（和服务端 `ChunkView::contains` 同一套
//! 圆角方形），记下每个区块第一次进入视距的时刻和它相对移动方向的位置；收到
//! `level_chunk_with_light` 时按包里的 x/z 配对，得到完整的到达延迟。
//!
//! 每名假玩家各记一份，多玩家场景可以看有没有人被别人挤在后面。
//!
//! Client-side "chunk entered view → chunk packet arrived" measurement. The
//! server's `chunk_latency_us` covers only the send queue, not loading or
//! generation. Here the bot reconstructs `T(vd)` from its own position (same
//! rounded square as the server's `ChunkView::contains`), notes when each chunk
//! first enters the view, and pairs it with the x/z in the received chunk packet.
//! Each bot has its own samples so a multi-player run exposes unfairness.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

/// 和服务端 `ChunkView::contains` 一样的集合 `T(vd)`。
fn in_view(dx: i32, dz: i32, view_distance: i32) -> bool {
    let axis = |distance: i32| i64::from((distance.abs() - 2).max(0));
    let x = axis(dx);
    let z = axis(dz);
    let radius = i64::from(view_distance);
    x * x + z * z < radius * radius
}

/// 一次进入视距的记录。
#[derive(Debug, Clone, Copy)]
struct Entered {
    at: Instant,
    /// 进入时是不是在玩家移动方向的前半边。
    forward: bool,
    /// 进入时离玩家不到 4 个区块。
    near: bool,
    /// 收到区块包没有。
    arrived: bool,
}

#[derive(Debug, Default)]
struct BotSamples {
    entered: HashMap<(i32, i32), Entered>,
    all_ms: Vec<u64>,
    front_ms: Vec<u64>,
    near_ms: Vec<u64>,
    /// 到达时区块相对玩家在移动方向上的投影（区块数），负数表示玩家已经飞过去了。
    ahead_at_arrival: Vec<i64>,
    /// 每刻「在视距里但还没收到」的区块数。
    gaps: Vec<u64>,
    /// 收到的包里没记录过进入视距的区块数。
    untracked_arrivals: u64,
}

impl BotSamples {
    fn report(&self) -> Value {
        json!({
            "latency_ms": percentiles(&mut self.all_ms.clone()),
            "front_latency_ms": percentiles(&mut self.front_ms.clone()),
            "near_latency_ms": percentiles(&mut self.near_ms.clone()),
            "ahead_at_arrival_chunks": signed_percentiles(&mut self.ahead_at_arrival.clone()),
            "gap_chunks": percentiles(&mut self.gaps.clone()),
            "chunks_entered": self.entered.len(),
            "chunks_arrived": self.all_ms.len(),
            "chunks_unarrived": self.entered.values().filter(|entry| !entry.arrived).count(),
            "untracked_arrivals": self.untracked_arrivals,
        })
    }
}

/// 全部假玩家共用的收集器。只在打开 `--chunk-trace` 时创建。
#[derive(Debug, Default)]
pub struct ChunkTrace {
    bots: Mutex<BTreeMap<usize, BotSamples>>,
}

/// 可克隆的句柄。
pub type SharedChunkTrace = Option<Arc<ChunkTrace>>;

impl ChunkTrace {
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 一个客户端 tick：把新进入视距的区块记下来，并数一次当前的缺口。
    ///
    /// `direction` 是水平移动方向的单位向量；`view_distance` 用服务端配置的视距。
    pub fn tick(
        &self,
        bot: usize,
        now: Instant,
        position: (f64, f64, f64),
        direction: (f64, f64),
        view_distance: i32,
    ) {
        let center_x = (position.0 / 16.0).floor() as i32;
        let center_z = (position.2 / 16.0).floor() as i32;
        let radius = view_distance + 1;
        let mut bots = self.bots.lock().unwrap_or_else(|error| error.into_inner());
        let samples = bots.entry(bot).or_default();
        let mut gap = 0_u64;
        // 一个客户端 tick 只看视距里的几百格；进入过的区块不再重新计时。
        for x in center_x.saturating_sub(radius)..=center_x.saturating_add(radius) {
            for z in center_z.saturating_sub(radius)..=center_z.saturating_add(radius) {
                let (dx, dz) = (x - center_x, z - center_z);
                if !in_view(dx, dz, view_distance) {
                    continue;
                }
                let entry = samples.entered.entry((x, z)).or_insert_with(|| Entered {
                    at: now,
                    forward: f64::from(dx) * direction.0 + f64::from(dz) * direction.1 > 0.0,
                    near: dx.abs() < 4 && dz.abs() < 4,
                    arrived: false,
                });
                if !entry.arrived {
                    gap += 1;
                }
            }
        }
        samples.gaps.push(gap);
    }

    /// 收到一个 `level_chunk_with_light`。`position` 是收到时玩家自己的位置。
    pub fn chunk(
        &self,
        bot: usize,
        now: Instant,
        x: i32,
        z: i32,
        position: (f64, f64, f64),
        direction: (f64, f64),
    ) {
        let center_x = (position.0 / 16.0).floor() as i32;
        let center_z = (position.2 / 16.0).floor() as i32;
        let mut bots = self.bots.lock().unwrap_or_else(|error| error.into_inner());
        let samples = bots.entry(bot).or_default();
        let Some(entry) = samples.entered.get_mut(&(x, z)) else {
            samples.untracked_arrivals += 1;
            return;
        };
        if entry.arrived {
            return;
        }
        entry.arrived = true;
        let millis = millis(now.saturating_duration_since(entry.at));
        let (forward, near) = (entry.forward, entry.near);
        samples.all_ms.push(millis);
        if forward {
            samples.front_ms.push(millis);
        }
        if near {
            samples.near_ms.push(millis);
        }
        let ahead = f64::from(x - center_x) * direction.0 + f64::from(z - center_z) * direction.1;
        samples.ahead_at_arrival.push(ahead.round() as i64);
    }

    /// 机器可读的一份报告，写进 `--trace-json` 指定的文件。
    /// 顶层是全体假玩家合起来的分位数，`per_bot` 是每名玩家各自的一份。
    pub fn report(&self) -> Value {
        let bots = self.bots.lock().unwrap_or_else(|error| error.into_inner());
        let mut total = BotSamples::default();
        let mut per_bot = serde_json::Map::new();
        for (index, samples) in bots.iter() {
            per_bot.insert(index.to_string(), samples.report());
            total.all_ms.extend_from_slice(&samples.all_ms);
            total.front_ms.extend_from_slice(&samples.front_ms);
            total.near_ms.extend_from_slice(&samples.near_ms);
            total
                .ahead_at_arrival
                .extend_from_slice(&samples.ahead_at_arrival);
            total.gaps.extend_from_slice(&samples.gaps);
            total.untracked_arrivals += samples.untracked_arrivals;
            for (&pos, entry) in &samples.entered {
                total
                    .entered
                    .entry(pos)
                    .and_modify(|old| {
                        old.arrived &= entry.arrived;
                    })
                    .or_insert(*entry);
            }
        }
        let mut report = total.report();
        let object = report.as_object_mut().expect("report is an object");
        object.insert("gap_samples".to_owned(), json!(total.gaps.len()));
        object.insert("per_bot".to_owned(), Value::Object(per_bot));
        report
    }
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn percentiles(samples: &mut [u64]) -> Value {
    if samples.is_empty() {
        return json!({ "samples": 0 });
    }
    samples.sort_unstable();
    let at = |percent: usize| {
        let rank = (samples.len() * percent).div_ceil(100).saturating_sub(1);
        samples[rank.min(samples.len() - 1)]
    };
    json!({
        "samples": samples.len(),
        "mean": samples.iter().sum::<u64>() / samples.len() as u64,
        "p50": at(50),
        "p95": at(95),
        "p99": at(99),
        "max": samples[samples.len() - 1],
    })
}

fn signed_percentiles(samples: &mut [i64]) -> Value {
    if samples.is_empty() {
        return json!({ "samples": 0 });
    }
    samples.sort_unstable();
    let at = |percent: usize| {
        let rank = (samples.len() * percent).div_ceil(100).saturating_sub(1);
        samples[rank.min(samples.len() - 1)]
    };
    json!({
        "samples": samples.len(),
        "p50": at(50),
        "p95": at(95),
        "min": samples[0],
        "max": samples[samples.len() - 1],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_matches_the_server_tracking_set() {
        // 视距 8 的跟踪集合是 329 个区块（和 chunk_tracking.rs 的表一致）。
        let count = (-11..=11)
            .flat_map(|x| (-11..=11).map(move |z| (x, z)))
            .filter(|&(x, z)| in_view(x, z, 8))
            .count();
        assert_eq!(count, 329);
        assert!(!in_view(10, 0, 8));
        assert!(!in_view(9, 6, 8));
        assert!(in_view(9, 5, 8));
    }

    #[test]
    fn a_chunk_is_timed_from_its_first_entry_only() {
        let trace = ChunkTrace::default();
        let start = Instant::now();
        let position = (8.5, 0.0, 8.5);
        trace.tick(0, start, position, (1.0, 0.0), 8);
        trace.tick(
            0,
            start + Duration::from_secs(1),
            (200.5, 0.0, 8.5),
            (1.0, 0.0),
            8,
        );
        trace.chunk(
            0,
            start + Duration::from_millis(250),
            0,
            0,
            position,
            (1.0, 0.0),
        );
        trace.chunk(
            0,
            start + Duration::from_millis(400),
            0,
            0,
            position,
            (1.0, 0.0),
        );
        let report = trace.report();
        assert_eq!(report["latency_ms"]["samples"], 1);
        assert_eq!(report["latency_ms"]["p50"], 250);
        assert_eq!(report["front_latency_ms"]["samples"], 0);
        assert_eq!(report["near_latency_ms"]["samples"], 1);
        // 第二次 tick 玩家飞出去了，视野里多了新的区块；中心区块仍然只记一次。
        assert!(report["per_bot"]["0"]["chunks_entered"].as_u64().unwrap() > 329);
    }

    #[test]
    fn bots_keep_separate_samples_and_the_total_merges_them() {
        let trace = ChunkTrace::default();
        let start = Instant::now();
        let position = (8.5, 0.0, 8.5);
        for bot in 0..3 {
            trace.tick(bot, start, position, (1.0, 0.0), 8);
        }
        trace.chunk(
            0,
            start + Duration::from_millis(100),
            3,
            0,
            position,
            (1.0, 0.0),
        );
        trace.chunk(
            1,
            start + Duration::from_millis(300),
            3,
            0,
            position,
            (1.0, 0.0),
        );
        let report = trace.report();
        assert_eq!(report["latency_ms"]["samples"], 2);
        assert_eq!(report["per_bot"]["0"]["latency_ms"]["p50"], 100);
        assert_eq!(report["per_bot"]["1"]["latency_ms"]["p50"], 300);
        assert_eq!(report["per_bot"]["2"]["latency_ms"]["samples"], 0);
        assert_eq!(report["gap_samples"], 3);
    }

    #[test]
    fn percentiles_are_nearest_rank() {
        let mut values: Vec<u64> = (1..=100).collect();
        let report = percentiles(&mut values);
        assert_eq!(report["p50"], 50);
        assert_eq!(report["p95"], 95);
        assert_eq!(report["p99"], 99);
    }
}
