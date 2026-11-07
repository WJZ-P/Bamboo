//! 发给客户端的帧在进入 socket 之前排的队。
//!
//! 世界线程和连接任务都往这里写，写的一方永不等待，所以一个不读数据的客户端拖不住世界线程。
//! 队列本身不设条数上限，上限按字节算：一批区块就有几 MB，按条数限制会误踢刚进服的玩家。

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use mc_bamboo_game::PlayerSink;
use tokio::sync::mpsc;
use tokio_util::sync::{CancellationToken, WaitForCancellationFuture};

/// 队列里还没写进 socket 的字节上限。未确认的区块批次最多约 35 MB，这里要容得下这个峰值。
pub(crate) const MAX_QUEUED_BYTES: usize = 64 * 1024 * 1024;

enum Item {
    Frames(Vec<u8>),
    /// 写完之前的帧就结束，排在它后面的帧不再写。
    Close,
}

/// 队列的写入端。
#[derive(Clone)]
pub(super) struct Outbound {
    items: mpsc::UnboundedSender<Item>,
    queued_bytes: Arc<AtomicUsize>,
    limit: usize,
    overflow: CancellationToken,
}

/// 队列的读取端，由连接任务持有。
pub(super) struct OutboundQueue {
    items: mpsc::UnboundedReceiver<Item>,
    queued_bytes: Arc<AtomicUsize>,
}

/// 创建一条队列。积压超过 `limit` 字节时 [`Outbound::overflowed`] 完成。
pub(super) fn channel(limit: usize) -> (Outbound, OutboundQueue) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let queued_bytes = Arc::new(AtomicUsize::new(0));
    let outbound = Outbound {
        items: sender,
        queued_bytes: Arc::clone(&queued_bytes),
        limit,
        overflow: CancellationToken::new(),
    };
    let queue = OutboundQueue {
        items: receiver,
        queued_bytes,
    };
    (outbound, queue)
}

impl Outbound {
    /// 放进一批完整的帧。积压太多或者连接任务已经结束时返回 `false`。
    pub(super) fn push(&self, frames: Vec<u8>) -> bool {
        let len = frames.len();
        let queued = self.queued_bytes.fetch_add(len, Ordering::Relaxed) + len;
        if queued > self.limit {
            self.overflow.cancel();
            return false;
        }
        self.items.send(Item::Frames(frames)).is_ok()
    }

    /// 让连接任务写完已有的帧之后结束。
    pub(super) fn finish(&self) {
        // 连接任务已经结束时发送会失败，这也正是调用方要的结果。
        let _ = self.items.send(Item::Close);
    }

    /// 积压超过上限时完成，这时要立刻中止连接。
    pub(super) fn overflowed(&self) -> WaitForCancellationFuture<'_> {
        self.overflow.cancelled()
    }
}

impl PlayerSink for Outbound {
    fn send(&mut self, frames: Vec<u8>) -> bool {
        self.push(frames)
    }

    fn close(&mut self) {
        self.finish();
    }
}

impl OutboundQueue {
    /// 下一批要写的帧。队列结束时返回 `None`。
    pub(super) async fn next(&mut self) -> Option<Vec<u8>> {
        match self.items.recv().await? {
            Item::Frames(frames) => Some(frames),
            Item::Close => None,
        }
    }

    /// 这些字节已经写进 socket 了。
    pub(super) fn mark_written(&self, len: usize) {
        self.queued_bytes.fetch_sub(len, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::timeout;

    use super::*;

    const DEADLINE: Duration = Duration::from_secs(5);

    #[tokio::test]
    async fn frames_come_out_in_order_until_close() {
        let (outbound, mut queue) = channel(100);

        assert!(outbound.push(vec![1]));
        assert!(outbound.push(vec![2]));
        outbound.finish();
        // 关闭之后写入的帧不会出现在队列里。
        outbound.push(vec![3]);

        assert_eq!(queue.next().await, Some(vec![1]));
        assert_eq!(queue.next().await, Some(vec![2]));
        assert_eq!(queue.next().await, None);
    }

    #[tokio::test]
    async fn dropping_every_writer_ends_the_queue() {
        let (outbound, mut queue) = channel(100);

        drop(outbound);

        assert_eq!(queue.next().await, None);
    }

    #[tokio::test]
    async fn overflow_is_refused_and_reported_at_once() {
        let (outbound, _queue) = channel(10);

        assert!(outbound.push(vec![0; 10]));
        assert!(!outbound.push(vec![0; 1]));

        timeout(DEADLINE, outbound.overflowed())
            .await
            .expect("积压超过上限时要立刻通知连接任务");
        // 一旦溢出，之后的写入都被拒绝。
        assert!(!outbound.push(Vec::new()));
    }

    #[tokio::test]
    async fn written_bytes_free_room_in_the_queue() {
        let (outbound, mut queue) = channel(10);
        assert!(outbound.push(vec![0; 10]));

        let frames = queue.next().await.expect("队列里有一批帧");
        queue.mark_written(frames.len());

        assert!(outbound.push(vec![0; 10]));
    }

    #[test]
    fn sink_refuses_frames_once_the_connection_task_is_gone() {
        let (mut sink, queue) = channel(10);

        drop(queue);

        assert!(!sink.send(vec![0]));
        // 连接任务已经结束时关闭也不算错误。
        sink.close();
    }

    #[test]
    fn world_thread_can_write_without_a_runtime() {
        let (mut sink, mut queue) = channel(10);

        assert!(sink.send(vec![7]));
        sink.close();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert_eq!(runtime.block_on(queue.next()), Some(vec![7]));
        assert_eq!(runtime.block_on(queue.next()), None);
    }
}
