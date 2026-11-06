//! 把 `tracing` 日志转发到界面。
//!
//! 日志可能在短时间内大量产生，所以不会逐条发给界面，而是攒起来定时批量发送，
//! 避免高频事件把前端的渲染拖慢。

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
}

/// 日志层和转发任务共用的缓冲区。
#[derive(Default)]
pub struct LogHub {
    buffers: Mutex<LogBuffers>,
}

impl LogHub {
    fn push(&self, level: &'static str, target: String, message: String) {
        let mut buffers = self.buffers.lock().expect("日志缓冲区的锁不应该中毒");
        let line = LogLine {
            id: buffers.next_id,
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_millis() as u64),
            level,
            target,
            message,
        };
        buffers.next_id += 1;

        if buffers.recent.len() == RECENT_CAPACITY {
            buffers.recent.pop_front();
        }
        buffers.recent.push_back(line.clone());
        buffers.pending.push(line);
    }

    /// 返回最近的历史日志，界面打开时用它补齐。
    pub fn recent(&self) -> Vec<LogLine> {
        let buffers = self.buffers.lock().expect("日志缓冲区的锁不应该中毒");
        buffers.recent.iter().cloned().collect()
    }

    fn take_pending(&self) -> Vec<LogLine> {
        let mut buffers = self.buffers.lock().expect("日志缓冲区的锁不应该中毒");
        std::mem::take(&mut buffers.pending)
    }
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
