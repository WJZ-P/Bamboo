//! 把 `tracing` 日志转发到界面。
//!
//! 日志可能在短时间内大量产生，所以不会逐条发给界面，而是攒起来定时批量发送，
//! 避免高频事件把前端的渲染拖慢。
//!
//! 发日志的线程（世界线程、岛的 tick 线程）只在一把很短的锁里把一行放进缓冲区，不碰
//! 界面。待发的行有上限：转发任务卡住时超出的行丢掉并计数，恢复后补发一行说明丢了几行；
//! `logs/latest.log` 那一路不受影响。
//!
//! Logging threads (the world thread, island tick threads) only put a line in
//! a buffer under a short lock and never touch the window. Pending lines are
//! capped: while the forwarder is stuck, lines past the cap are dropped and
//! counted, and one line reporting the count is sent once it recovers;
//! `logs/latest.log` is unaffected.

use std::{
    collections::VecDeque,
    fmt::{self, Write},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, async_runtime};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{Layer, layer::Context};

const LOG_EVENT: &str = "bamboo://log";
const FLUSH_INTERVAL: Duration = Duration::from_millis(100);
/// 打开界面时能看到的历史日志条数。
const RECENT_CAPACITY: usize = 1000;
/// 最多攒多少行等着发给界面。正常每 100 毫秒清空一次，远到不了这个数。
///
/// How many lines may wait for the window at most. It is emptied every 100 ms
/// normally, far below this.
const PENDING_CAPACITY: usize = 10_000;

/// 发给界面的一行日志。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    id: u64,
    timestamp_ms: u64,
    level: &'static str,
    target: String,
    message: String,
}

#[derive(Default)]
struct LogBuffers {
    next_id: u64,
    recent: VecDeque<LogLine>,
    pending: Vec<LogLine>,
    /// 待发的行满了之后丢掉的行数，下一次发送时报出来。
    ///
    /// Lines dropped since pending was full, reported with the next batch.
    dropped: u64,
}

/// 日志层和转发任务共用的缓冲区。
#[derive(Default)]
pub struct LogHub {
    buffers: Mutex<LogBuffers>,
}

impl LogHub {
    fn push(&self, level: &'static str, target: String, message: String) {
        let timestamp_ms = now_ms();
        let mut buffers = self.buffers.lock().expect("日志缓冲区的锁不应该中毒");
        let line = buffers.line(timestamp_ms, level, target, message);

        if buffers.recent.len() == RECENT_CAPACITY {
            buffers.recent.pop_front();
        }
        if buffers.pending.len() < PENDING_CAPACITY {
            buffers.recent.push_back(line.clone());
            buffers.pending.push(line);
        } else {
            buffers.recent.push_back(line);
            buffers.dropped += 1;
        }
    }

    /// 返回最近的历史日志，界面打开时用它补齐。
    pub fn recent(&self) -> Vec<LogLine> {
        let buffers = self.buffers.lock().expect("日志缓冲区的锁不应该中毒");
        buffers.recent.iter().cloned().collect()
    }

    fn take_pending(&self) -> Vec<LogLine> {
        let mut buffers = self.buffers.lock().expect("日志缓冲区的锁不应该中毒");
        let mut lines = std::mem::take(&mut buffers.pending);
        if buffers.dropped > 0 {
            let dropped = std::mem::take(&mut buffers.dropped);
            let note = buffers.line(
                now_ms(),
                "WARN",
                "bamboo-log".to_owned(),
                format!(
                    "{dropped} log lines were not sent to the window: it fell more than \
                     {PENDING_CAPACITY} lines behind (logs/latest.log has them)"
                ),
            );
            lines.push(note);
        }
        lines
    }
}

impl LogBuffers {
    /// 编号递增地造一行。
    ///
    /// Build a line with the next id.
    fn line(
        &mut self,
        timestamp_ms: u64,
        level: &'static str,
        target: String,
        message: String,
    ) -> LogLine {
        let id = self.next_id;
        self.next_id += 1;
        LogLine {
            id,
            timestamp_ms,
            level,
            target,
            message,
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

/// 收集 `tracing` 事件的日志层。
pub struct LogLayer {
    hub: Arc<LogHub>,
}

impl LogLayer {
    pub fn new(hub: Arc<LogHub>) -> Self {
        Self { hub }
    }
}

impl<S: Subscriber> Layer<S> for LogLayer {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut message = MessageVisitor::default();
        event.record(&mut message);
        let metadata = event.metadata();
        self.hub.push(
            metadata.level().as_str(),
            metadata.target().to_owned(),
            message.finish(),
        );
    }
}

/// 把事件字段拼成一行文字：`message` 在前，其余字段按 `名字=值` 跟在后面。
#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: String,
}

impl MessageVisitor {
    fn finish(self) -> String {
        if self.fields.is_empty() {
            self.message
        } else {
            format!("{}{}", self.message, self.fields)
        }
    }
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
}

/// 启动定时批量发送日志的后台任务。
pub fn spawn_forwarder(app: AppHandle, hub: &Arc<LogHub>) {
    let hub = Arc::clone(hub);
    async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(FLUSH_INTERVAL);
        loop {
            ticker.tick().await;
            let lines = hub.take_pending();
            // 这里出错也不能再写日志，否则失败本身又会产生新的日志。
            if !lines.is_empty() {
                let _ = app.emit(LOG_EVENT, lines);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 转发任务一直不来取时，待发的行停在上限，多出的计数；下一批末尾补一行丢了几行，
    /// 历史日志照常滚动。
    ///
    /// While the forwarder never takes lines, pending stops at the cap and the
    /// rest are counted; the next batch ends with a line reporting the count,
    /// and the history keeps rolling.
    #[test]
    fn pending_lines_are_capped_and_the_drop_count_is_reported() {
        let hub = LogHub::default();
        for index in 0..PENDING_CAPACITY + 5 {
            hub.push("INFO", "test".to_owned(), format!("line {index}"));
        }
        let batch = hub.take_pending();
        assert_eq!(batch.len(), PENDING_CAPACITY + 1);
        let note = batch.last().unwrap();
        assert_eq!(note.level, "WARN");
        assert!(
            note.message.starts_with("5 log lines were not sent"),
            "{}",
            note.message
        );
        assert_eq!(hub.recent().len(), RECENT_CAPACITY);
        assert_eq!(
            hub.recent().last().unwrap().message,
            format!("line {}", PENDING_CAPACITY + 4)
        );

        hub.push("INFO", "test".to_owned(), "after".to_owned());
        let batch = hub.take_pending();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].message, "after");
    }
}
