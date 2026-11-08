//! 性能监控：每秒读一次本进程和整机的 CPU、内存，推给界面。
//!
//! 服务端跑在这个进程里，所以“本进程”就是 Bamboo 服务端加上界面的宿主。
//! WebView2 的渲染进程是单独的进程，不算在里面。

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tauri::{AppHandle, Emitter};

const METRICS_EVENT: &str = "bamboo://metrics";
/// 采样间隔，和任务管理器默认的更新速度一样。
const INTERVAL: Duration = Duration::from_secs(1);
/// 界面的曲线显示最近 60 秒，打开界面时用这些补齐。
const RECENT_CAPACITY: usize = 60;

/// 一次采样。CPU 都按整机算，0 到 100；内存是字节。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSample {
    timestamp_ms: u64,
    process_cpu: f32,
    system_cpu: f32,
    /// 本进程的工作集，Windows 上是 `WorkingSetSize`，其它系统是常驻内存。
    process_memory: u64,
    system_memory_used: u64,
    system_memory_total: u64,
    /// 逻辑处理器个数。
    cpu_count: usize,
}

/// 采样线程和命令共用的最近采样。
#[derive(Default)]
pub struct MetricsHub {
    recent: Mutex<VecDeque<MetricsSample>>,
}

impl MetricsHub {
    fn push(&self, sample: MetricsSample) {
        let mut recent = self.recent.lock().expect("性能采样的锁不应该中毒");
        if recent.len() == RECENT_CAPACITY {
            recent.pop_front();
        }
        recent.push_back(sample);
    }

    /// 最近的采样，从旧到新。
    pub fn recent(&self) -> Vec<MetricsSample> {
        let recent = self.recent.lock().expect("性能采样的锁不应该中毒");
        recent.iter().cloned().collect()
    }
}

/// 读系统信息的状态。CPU 使用率要两次读数相减才算得出来，所以要一直留着。
struct Sampler {
    system: System,
    pid: Pid,
}

impl Sampler {
    fn new() -> Result<Self, sysinfo::Error> {
        let pid = sysinfo::get_current_pid()?;
        let mut sampler = Self {
            system: System::new()?,
            pid,
        };
        // 先读一次打底，第一次真正的采样才有差值可算。
        sampler.refresh();
        Ok(sampler)
    }

    fn refresh(&mut self) {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            false,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
    }

    fn sample(&mut self) -> MetricsSample {
        self.refresh();
        let cpu_count = self.system.cpus().len().max(1);
        let process = self.system.process(self.pid);
        MetricsSample {
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_millis() as u64),
            // sysinfo 的进程 CPU 以单核为 100%，换成占整机的比例，和任务管理器的进程页一样。
            process_cpu: process.map_or(0.0, |process| process.cpu_usage() / cpu_count as f32),
            system_cpu: self.system.global_cpu_usage(),
            process_memory: process.map_or(0, |process| process.memory()),
            system_memory_used: self.system.used_memory(),
            system_memory_total: self.system.total_memory(),
            cpu_count,
        }
    }
}

/// 启动采样线程。读不了系统信息时只记一条警告，界面上的面板保持空白。
pub fn spawn_sampler(app: AppHandle, hub: &Arc<MetricsHub>) {
    let hub = Arc::clone(hub);
    let mut sampler = match Sampler::new() {
        Ok(sampler) => sampler,
        Err(error) => {
            tracing::warn!("读不了系统的 CPU 和内存信息，性能面板不会更新：{error}");
            return;
        }
    };
    // 读系统信息是阻塞调用，放在自己的线程里，不占异步运行时。
    let spawned = thread::Builder::new()
        .name("metrics".to_owned())
        .spawn(move || {
            loop {
                thread::sleep(INTERVAL);
                let sample = sampler.sample();
                hub.push(sample.clone());
                // 这里出错也不写日志：每秒一次，界面关着时会刷满日志。
                let _ = app.emit(METRICS_EVENT, sample);
            }
        });
    if let Err(error) = spawned {
        tracing::warn!("性能采样线程启动失败：{error}");
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn a_sample_reads_this_process_and_the_machine() {
        let mut sampler = Sampler::new().expect("测试机器上应该读得到系统信息");
        // 忙一会儿，本进程的 CPU 占用才不会是 0。
        let busy = Instant::now();
        let mut spin = 0_u64;
        while busy.elapsed() < Duration::from_millis(400) {
            spin = std::hint::black_box(spin.wrapping_add(1));
        }
        let sample = sampler.sample();

        assert!(sample.cpu_count >= 1);
        assert!(sample.process_cpu > 0.0, "{sample:?}");
        assert!(sample.process_cpu <= 100.5, "{sample:?}");
        assert!((0.0..=100.5).contains(&sample.system_cpu), "{sample:?}");
        assert!(sample.process_memory > 0, "{sample:?}");
        assert!(
            sample.system_memory_used <= sample.system_memory_total,
            "{sample:?}"
        );
        assert!(
            sample.process_memory < sample.system_memory_total,
            "{sample:?}"
        );
    }

    #[test]
    fn the_hub_keeps_the_last_minute() {
        let hub = MetricsHub::default();
        for timestamp_ms in 0..(RECENT_CAPACITY as u64 + 5) {
            hub.push(MetricsSample {
                timestamp_ms,
                process_cpu: 0.0,
                system_cpu: 0.0,
                process_memory: 0,
                system_memory_used: 0,
                system_memory_total: 0,
                cpu_count: 1,
            });
        }
        let recent = hub.recent();
        assert_eq!(recent.len(), RECENT_CAPACITY);
        assert_eq!(recent[0].timestamp_ms, 5);
        assert_eq!(recent.last().map(|sample| sample.timestamp_ms), Some(64));
    }
}
