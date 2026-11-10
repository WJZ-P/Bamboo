//! 客户端一侧的统计。所有假玩家共用一个 tokio 运行时，统计只在每秒边界汇总。

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

/// 一秒里收到/发出的数据。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Second {
    pub bytes_in: u64,
    pub packets_in: u64,
    pub chunks_in: u64,
    pub bytes_out: u64,
    pub packets_out: u64,
}

/// 一次掉线。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drop {
    pub bot: usize,
    pub at: Duration,
    pub reason: String,
}

/// 全体假玩家的共享统计。锁只在一秒一次和连接结束时碰，热路径不拿锁。
#[derive(Debug, Default)]
pub struct Stats {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    connected: u64,
    login_failures: BTreeMap<String, u64>,
    drops: Vec<Drop>,
    seconds: BTreeMap<u64, Second>,
    rtt_ms: Vec<u64>,
    join_times: Vec<Duration>,
}

/// 收集器的可克隆句柄。
pub type SharedStats = Arc<Stats>;

impl Stats {
    pub fn shared() -> SharedStats {
        Arc::new(Self::default())
    }

    /// 一名假玩家进入 Play 状态。
    pub fn connected(&self, elapsed: Duration, join_time: Duration) {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        inner.connected += 1;
        inner.join_times.push(join_time);
        let _ = elapsed;
    }

    /// 一名假玩家没走到 Play。
    pub fn login_failure(&self, reason: String) {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        *inner.login_failures.entry(reason).or_default() += 1;
    }

    /// 一名假玩家中途掉线。
    pub fn dropped(&self, bot: usize, at: Duration, reason: String) {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        inner.drops.push(Drop { bot, at, reason });
    }

    /// 累加一秒的计数。
    pub fn second(&self, index: u64, sample: Second) {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let slot = inner.seconds.entry(index).or_default();
        slot.bytes_in += sample.bytes_in;
        slot.packets_in += sample.packets_in;
        slot.chunks_in += sample.chunks_in;
        slot.bytes_out += sample.bytes_out;
        slot.packets_out += sample.packets_out;
    }

    /// 记一次心跳往返，单位毫秒。
    pub fn keep_alive_rtt(&self, millis: u64) {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        inner.rtt_ms.push(millis);
    }

    /// 打印人读的报告。基线脚本用 `--json` 再拿一份机器可读的结果。
    pub fn report(&self, bots: usize, duration: Duration) -> Report {
        let inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let seconds: Vec<Second> = inner.seconds.values().copied().collect();
        let rtt = percentiles(&inner.rtt_ms);
        Report {
            bots,
            connected: inner.connected,
            login_failures: inner.login_failures.clone(),
            drops: inner.drops.clone(),
            seconds: seconds.clone(),
            mean_bytes_in: mean(seconds.iter().map(|s| s.bytes_in)),
            mean_packets_in: mean(seconds.iter().map(|s| s.packets_in)),
            mean_chunks_in: mean(seconds.iter().map(|s| s.chunks_in)),
            mean_bytes_out: mean(seconds.iter().map(|s| s.bytes_out)),
            rtt_p50: rtt[0],
            rtt_p95: rtt[1],
            rtt_p99: rtt[2],
            rtt_max: inner.rtt_ms.iter().copied().max().unwrap_or(0),
            rtt_samples: inner.rtt_ms.len(),
            duration_secs: duration.as_secs_f64(),
        }
    }
}

/// 一份完整的客户端报告。
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub bots: usize,
    pub connected: u64,
    pub login_failures: BTreeMap<String, u64>,
    pub drops: Vec<Drop>,
    pub seconds: Vec<Second>,
    pub mean_bytes_in: f64,
    pub mean_packets_in: f64,
    pub mean_chunks_in: f64,
    pub mean_bytes_out: f64,
    pub rtt_p50: u64,
    pub rtt_p95: u64,
    pub rtt_p99: u64,
    pub rtt_max: u64,
    pub rtt_samples: usize,
    pub duration_secs: f64,
}

impl Report {
    /// 人读的多行报告。
    pub fn text(&self) -> String {
        use std::fmt::Write;

        let mut out = String::new();
        let _ = writeln!(
            out,
            "loadtest clients: {} bots, {:.1}s",
            self.bots, self.duration_secs
        );
        let _ = writeln!(
            out,
            "connected={} login_failures={} dropped={}",
            self.connected,
            self.login_failures.values().sum::<u64>(),
            self.drops.len()
        );
        for (reason, count) in &self.login_failures {
            let _ = writeln!(out, "  login failure x{count}: {reason}");
        }
        for drop in &self.drops {
            let _ = writeln!(
                out,
                "  drop bot {} at {:.1}s: {}",
                drop.bot,
                drop.at.as_secs_f64(),
                drop.reason
            );
        }
        let _ = writeln!(
            out,
            "receive: mean {:.0} bytes/s, {:.0} packets/s, {:.0} chunks/s",
            self.mean_bytes_in, self.mean_packets_in, self.mean_chunks_in
        );
        let _ = writeln!(out, "send: mean {:.0} bytes/s", self.mean_bytes_out);
        let _ = writeln!(
            out,
            "keep-alive rtt: p50={} p95={} p99={} max={} ms ({} samples)",
            self.rtt_p50, self.rtt_p95, self.rtt_p99, self.rtt_max, self.rtt_samples
        );
        let _ = writeln!(out, "per-second receive:");
        for (index, second) in self.seconds.iter().enumerate() {
            let _ = writeln!(
                out,
                "  {:>3}s  bytes={:<12} packets={:<9} chunks={:<9} sent={}",
                index, second.bytes_in, second.packets_in, second.chunks_in, second.bytes_out
            );
        }
        out.pop();
        out
    }

    /// 机器可读的报告，基线脚本用它生成汇总表。
    pub fn json(&self) -> serde_json::Value {
        let failures: Vec<serde_json::Value> = self
            .login_failures
            .iter()
            .map(|(reason, count)| serde_json::json!({ "reason": reason, "count": count }))
            .collect();
        let drops: Vec<serde_json::Value> = self
            .drops
            .iter()
            .map(|drop| {
                serde_json::json!({
                    "bot": drop.bot,
                    "at_secs": drop.at.as_secs_f64(),
                    "reason": drop.reason,
                })
            })
            .collect();
        let seconds: Vec<serde_json::Value> = self
            .seconds
            .iter()
            .map(|second| {
                serde_json::json!({
                    "bytes_in": second.bytes_in,
                    "packets_in": second.packets_in,
                    "chunks_in": second.chunks_in,
                    "bytes_out": second.bytes_out,
                    "packets_out": second.packets_out,
                })
            })
            .collect();
        serde_json::json!({
            "bots": self.bots,
            "connected": self.connected,
            "login_failures": failures,
            "drops": drops,
            "seconds": seconds,
            "mean_bytes_in": self.mean_bytes_in,
            "mean_packets_in": self.mean_packets_in,
            "mean_chunks_in": self.mean_chunks_in,
            "mean_bytes_out": self.mean_bytes_out,
            "rtt_p50_ms": self.rtt_p50,
            "rtt_p95_ms": self.rtt_p95,
            "rtt_p99_ms": self.rtt_p99,
            "rtt_max_ms": self.rtt_max,
            "rtt_samples": self.rtt_samples,
            "duration_secs": self.duration_secs,
        })
    }
}

fn mean(values: impl Iterator<Item = u64>) -> f64 {
    let mut sum = 0_u64;
    let mut count = 0_u64;
    for value in values {
        sum = sum.saturating_add(value);
        count += 1;
    }
    if count == 0 {
        0.0
    } else {
        sum as f64 / count as f64
    }
}

/// 返回 p50 / p95 / p99，样本为空时全是 0。
fn percentiles(samples: &[u64]) -> [u64; 3] {
    if samples.is_empty() {
        return [0; 3];
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let at = |percent: usize| {
        let rank = (sorted.len() * percent).div_ceil(100).saturating_sub(1);
        sorted[rank.min(sorted.len() - 1)]
    };
    [at(50), at(95), at(99)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_are_nearest_rank() {
        assert_eq!(percentiles(&[]), [0, 0, 0]);
        assert_eq!(percentiles(&[7]), [7, 7, 7]);
        let values: Vec<u64> = (1..=100).collect();
        assert_eq!(percentiles(&values), [50, 95, 99]);
    }

    #[test]
    fn report_aggregates_seconds_and_drops() {
        let stats = Stats::shared();
        stats.connected(Duration::ZERO, Duration::from_millis(50));
        stats.connected(Duration::from_secs(1), Duration::from_millis(70));
        stats.second(
            0,
            Second {
                bytes_in: 100,
                packets_in: 5,
                chunks_in: 2,
                ..Second::default()
            },
        );
        stats.second(
            0,
            Second {
                bytes_in: 300,
                packets_in: 7,
                chunks_in: 3,
                ..Second::default()
            },
        );
        stats.second(
            1,
            Second {
                bytes_in: 200,
                packets_in: 4,
                ..Second::default()
            },
        );
        stats.keep_alive_rtt(10);
        stats.keep_alive_rtt(30);
        stats.dropped(3, Duration::from_secs(5), "closed".to_owned());
        stats.login_failure("full".to_owned());

        let report = stats.report(5, Duration::from_secs(2));
        assert_eq!(report.connected, 2);
        assert_eq!(report.seconds.len(), 2);
        assert_eq!(report.seconds[0].bytes_in, 400);
        assert_eq!(report.mean_bytes_in, 300.0);
        assert_eq!(report.mean_chunks_in, 2.5);
        assert_eq!(report.rtt_p50, 10);
        assert_eq!(report.rtt_p99, 30);
        assert_eq!(report.drops.len(), 1);
        assert_eq!(report.login_failures["full"], 1);

        let json = report.json();
        assert_eq!(json["connected"], 2);
        assert_eq!(json["mean_bytes_in"], 300.0);
        assert!(report.text().contains("dropped=1"));
    }
}
