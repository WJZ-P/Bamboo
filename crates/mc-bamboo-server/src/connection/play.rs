//! Play 状态：连接任务这一半。
//!
//! 游戏逻辑都在世界线程里。这里只做三件事：读帧转发给世界线程、把出站队列写进 socket、
//! 维持心跳。心跳留在这里，世界线程卡顿或者 socket 写不动时，超时判断都不会跟着停。

use std::{
    sync::{Arc, atomic::AtomicUsize, mpsc::Sender},
    time::Duration,
};

use bytes::BytesMut;
use mc_bamboo_auth::{DecryptHalf, EncryptHalf, Encryption};
use mc_bamboo_game::{InFlight, Inbound, JoinRequest, PlayerId};
use mc_bamboo_protocol::{
    compression,
    frame::append_frame,
    packet::{
        ClientboundPacket,
        play::{ClientInformation, Disconnect, KeepAlive, PlayRequest, PongResponse},
    },
    text::Component,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    time::{Instant, MissedTickBehavior, interval_at, timeout},
};
use tokio_util::sync::CancellationToken;

use super::{
    Connection, ConnectionError, finish,
    outbound::{self, MAX_QUEUED_BYTES, Outbound, OutboundQueue},
    read_frame,
};
use crate::{players::PlayerSession, server::Shared};

/// 一条连接最多有多少条请求排在世界线程前面。正常玩家每 tick 只有几个包，
/// 到这个数说明客户端异常或者世界线程卡住了。不能靠停止读 socket 限速，那样就读不到心跳回应了。
const IN_FLIGHT_LIMIT: usize = 4096;
/// 一次写 socket 的时限。客户端不读数据时不能让连接永远挂着。
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// 关闭时等待写完和等待客户端断开的时限，不能让服务端的关闭流程被一个客户端拖住。
const SHUTDOWN_LINGER: Duration = Duration::from_secs(1);

/// 运行一名玩家的连接直到结束。
///
/// 返回时世界线程已经收到了 `Leave`，所以调用方之后才能销毁 `session`：
/// 同名玩家的下一次 `Join` 一定排在这个 `Leave` 后面。
pub(super) async fn serve(
    connection: Connection,
    session: &PlayerSession,
    client: ClientInformation,
    shared: &Shared,
    cancel: CancellationToken,
) -> Result<(), ConnectionError> {
    let Connection {
        mut stream,
        mut read_buffer,
        max_frame_len,
        compression,
        encryption,
        // 进了 Play 就没有进服时限了，阶段这时用不到。
        phase: _,
    } = connection;
    let player = PlayerId(session.entity_id);
    let leave = LeaveGuard {
        inbox: &shared.inbox,
        player,
    };

    let reader = RequestReader {
        player,
        shared,
        max_frame_len,
        compression,
    };
    let (outcome, flushed) = exchange(
        &mut stream,
        &mut read_buffer,
        reader,
        session,
        client,
        encryption,
        cancel,
    )
    .await;

    // 不用等下面的 linger，其他玩家可以马上看到这名玩家离开。
    drop(leave);
    if flushed {
        // 超时说明客户端不配合，这时直接放弃就好。
        let _ = timeout(SHUTDOWN_LINGER, finish(&mut stream)).await;
    }
    outcome
}

/// 连接结束时告诉世界线程。放在析构里，所以连接无论怎么结束都能送达。
struct LeaveGuard<'a> {
    inbox: &'a Sender<Inbound>,
    player: PlayerId,
}

impl Drop for LeaveGuard<'_> {
    fn drop(&mut self) {
        // 世界线程已经结束时发送会失败，这时也不需要通知了。
        let _ = self.inbox.send(Inbound::Leave {
            player: self.player,
        });
    }
}

/// 把一个包编码后放进出站队列。放不进去时 [`Outbound::overflowed`] 会结束连接。
fn enqueue<P: ClientboundPacket>(outbound: &Outbound, packet: &P) -> Result<(), ConnectionError> {
    let mut frame = Vec::new();
    append_frame(&mut frame, packet)?;
    outbound.push(frame);
    Ok(())
}

/// 收发到连接该结束为止。第二个返回值表示已经交出的帧都写完了，可以体面地关闭 socket。
async fn exchange(
    stream: &mut TcpStream,
    read_buffer: &mut BytesMut,
    reader: RequestReader<'_>,
    session: &PlayerSession,
    client: ClientInformation,
    encryption: Option<Encryption>,
    cancel: CancellationToken,
) -> (Result<(), ConnectionError>, bool) {
    let (outbound, queue) = outbound::channel(MAX_QUEUED_BYTES);
    let (mut read_half, mut write_half) = stream.split();
    let compression = reader.compression;
    // 读和写在下面两个并发的任务里，各自拿走自己那个方向的 AES 流。
    let (encrypt, decrypt) = match encryption {
        Some(encryption) => {
            let (encrypt, decrypt) = encryption.split();
            (Some(encrypt), Some(decrypt))
        }
        None => (None, None),
    };

    let writer = write_queue(&mut write_half, queue, WRITE_TIMEOUT, compression, encrypt);
    let requests = async {
        reader.join(session, client, &outbound)?;
        reader
            .run(&mut read_half, read_buffer, &outbound, decrypt)
            .await
    };
    tokio::pin!(writer, requests);

    let outcome = tokio::select! {
        read = &mut requests => read,
        // 世界线程关闭了队列：它踢了这名玩家，原因已经写出去了。
        written = &mut writer => {
            let flushed = written.is_ok();
            return (written, flushed);
        }
        () = session.kicked() => Err(ConnectionError::DuplicateLogin),
        () = cancel.cancelled() => Err(ConnectionError::ServerStopped),
        () = outbound.overflowed() => Err(ConnectionError::OutboundOverflow),
    };

    let reason = outcome
        .as_ref()
        .err()
        .and_then(ConnectionError::disconnect_reason);
    let flushed = match reason {
        Some(reason) => {
            let reason = Component::translate(reason, []);
            let farewell = enqueue(&outbound, &Disconnect { reason: &reason });
            outbound.finish();
            // 世界线程已经关闭过队列时，这个断开包排在关闭后面，不会写出去。
            let written = timeout(SHUTDOWN_LINGER, &mut writer).await;
            farewell.is_ok() && matches!(written, Ok(Ok(())))
        }
        // 客户端自己走了，或者连接已经坏了。
        None => outcome.is_ok(),
    };
    (outcome, flushed)
}

/// 按顺序写出队列里的帧，直到队列结束。压缩启用时每个包在写之前压缩，加密启用时再加密。
///
/// 顺序和官方管线一致：压缩在长度前缀里面，加密在最外面，连长度前缀一起加密。
async fn write_queue<W: AsyncWrite + Unpin>(
    stream: &mut W,
    mut queue: OutboundQueue,
    time_limit: Duration,
    compression: Option<i32>,
    mut encrypt: Option<EncryptHalf>,
) -> Result<(), ConnectionError> {
    while let Some(frames) = queue.next().await {
        let compressed = match compression {
            Some(threshold) => Some(compression::encode_frames(&frames, threshold)?),
            None => None,
        };
        let payload = compressed.as_deref().unwrap_or(&frames);
        let encrypted = match &mut encrypt {
            Some(encrypt) => {
                let mut buffer = payload.to_vec();
                encrypt.encrypt(&mut buffer);
                Some(buffer)
            }
            None => None,
        };
        timeout(
            time_limit,
            stream.write_all(encrypted.as_deref().unwrap_or(payload)),
        )
        .await
        .map_err(|_| ConnectionError::WriteTimeout)??;
        queue.mark_written(frames.len());
    }
    Ok(())
}

/// 读帧、解码，并决定每个请求由谁处理。
struct RequestReader<'a> {
    player: PlayerId,
    shared: &'a Shared,
    max_frame_len: usize,
    compression: Option<i32>,
}

impl RequestReader<'_> {
    /// 把玩家交给世界线程，从这一刻起所有发给客户端的包都由世界线程写。
    fn join(
        &self,
        session: &PlayerSession,
        client: ClientInformation,
        outbound: &Outbound,
    ) -> Result<(), ConnectionError> {
        let op = self.shared.ops.get(session.uuid);
        let request = JoinRequest {
            id: self.player,
            uuid: session.uuid,
            name: session.name.clone(),
            properties: session.properties.clone(),
            client,
            // 官方 `PlayerList.getPermissions` 只按 UUID 查 OP 表，没列出来的都是 0。
            // `is_op` 供刷屏豁免使用，等级 0 的 OP 也算 OP。
            //
            // Official `PlayerList.getPermissions` looks the UUID up in the op list.
            // Anyone missing from it is level 0. `is_op` is the spam exemption, so a level-0 op still counts.
            permission_level: op.map_or(0, |entry| entry.level),
            is_op: op.is_some(),
            sink: Box::new(outbound.clone()),
        };
        self.send_to_world(Inbound::Join(Box::new(request)))
    }

    fn send_to_world(&self, message: Inbound) -> Result<(), ConnectionError> {
        self.shared
            .inbox
            .send(message)
            .map_err(|_| ConnectionError::ServerStopped)
    }

    /// 读到连接结束。客户端正常关闭连接时返回 `Ok`。
    async fn run<R: AsyncRead + Unpin>(
        &self,
        stream: &mut R,
        buffer: &mut BytesMut,
        outbound: &Outbound,
        mut decrypt: Option<DecryptHalf>,
    ) -> Result<(), ConnectionError> {
        let period = self.shared.config.keep_alive_interval;
        let mut ticks = interval_at(Instant::now() + period, period);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut keep_alive = KeepAliveState::default();
        let in_flight = Arc::new(AtomicUsize::new(0));

        loop {
            tokio::select! {
                _ = ticks.tick() => {
                    // 放进队列就算发出，计时从这一刻开始，不等 socket 写完。
                    let id = keep_alive
                        .challenge(Instant::now())
                        .ok_or(ConnectionError::KeepAliveTimeout)?;
                    enqueue(outbound, &KeepAlive { id })?;
                }
                frame = read_frame(
                    stream,
                    buffer,
                    self.max_frame_len,
                    self.compression,
                    decrypt.as_mut(),
                ) => {
                    let Some(payload) = frame? else {
                        return Ok(());
                    };
                    let request = PlayRequest::decode(&payload)?;
                    self.handle(request, &mut keep_alive, &in_flight, outbound)?;
                }
            }
        }
    }

    fn handle(
        &self,
        request: PlayRequest,
        keep_alive: &mut KeepAliveState,
        in_flight: &Arc<AtomicUsize>,
        outbound: &Outbound,
    ) -> Result<(), ConnectionError> {
        match request {
            PlayRequest::KeepAlive(id) => {
                // 官方 `handleKeepAlive`：只有正在等的那个 id 才更新延迟。
                // 对不上，或者当前没在等，都用 `disconnect.timeout` 断开。
                // 单人房主是例外，Bamboo 没有单人模式。
                //
                // Official `handleKeepAlive` updates latency only for the pending id.
                // A mismatch, or a response while none is pending, disconnects with
                // `disconnect.timeout`. The singleplayer-owner exception does not apply.
                let millis = keep_alive
                    .answer(id, Instant::now())
                    .ok_or(ConnectionError::KeepAliveTimeout)?;
                self.send_to_world(Inbound::Latency {
                    player: self.player,
                    millis,
                })
            }
            PlayRequest::PingRequest(time) => enqueue(outbound, &PongResponse { time }),
            // 客户端每 tick 都有这种包，比如 `client_tick_end`，不要转发也不要记日志。
            PlayRequest::Other(_) => Ok(()),
            request => {
                let in_flight = InFlight::acquire(in_flight, IN_FLIGHT_LIMIT)
                    .ok_or(ConnectionError::TooManyRequests)?;
                self.send_to_world(Inbound::Request {
                    player: self.player,
                    request,
                    in_flight,
                })
            }
        }
    }
}

/// 心跳的状态。
#[derive(Debug, Default)]
struct KeepAliveState {
    /// 等着回应的心跳和它进队列的时刻。
    pending: Option<(i64, Instant)>,
    last_id: i64,
    /// 毫秒。
    latency: i32,
}

impl KeepAliveState {
    /// 到了发下一个心跳的时候。上一个还没回应时返回 `None`。
    fn challenge(&mut self, now: Instant) -> Option<i64> {
        if self.pending.is_some() {
            return None;
        }
        self.last_id += 1;
        self.pending = Some((self.last_id, now));
        Some(self.last_id)
    }

    /// 收到回应，返回新的平均延迟。内容不对或者没在等回应时返回 `None`。
    fn answer(&mut self, id: i64, now: Instant) -> Option<i32> {
        let (expected, sent_at) = self.pending?;
        if id != expected {
            return None;
        }
        self.pending = None;
        let round_trip = now.saturating_duration_since(sent_at).as_millis() as i64;
        self.latency = ((i64::from(self.latency) * 3 + round_trip) / 4) as i32;
        Some(self.latency)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_auth::SHARED_SECRET_LEN;
    use mc_bamboo_protocol::frame::{MAX_FRAME_LEN, find_frame};
    use tokio::io::{AsyncReadExt, duplex};

    use super::*;

    #[test]
    fn keep_alive_ids_increase_and_wait_for_the_answer() {
        let mut state = KeepAliveState::default();
        let now = Instant::now();

        assert_eq!(state.challenge(now), Some(1));
        assert_eq!(state.challenge(now), None);

        assert!(state.answer(1, now).is_some());
        assert_eq!(state.challenge(now), Some(2));
    }

    #[test]
    fn wrong_or_unsolicited_answers_are_refused() {
        let mut state = KeepAliveState::default();
        let now = Instant::now();

        assert_eq!(state.answer(1, now), None);
        state.challenge(now);
        assert_eq!(state.answer(9, now), None);
        assert!(state.answer(1, now).is_some());
        assert_eq!(state.answer(1, now), None);
    }

    #[test]
    fn latency_is_a_rolling_average_in_integer_math() {
        let mut state = KeepAliveState::default();
        let start = Instant::now();

        let mut answer_after = |millis: u64| {
            state.challenge(start);
            state.answer(state.last_id, start + Duration::from_millis(millis))
        };

        assert_eq!(answer_after(100), Some(25));
        assert_eq!(answer_after(100), Some(43));
        assert_eq!(answer_after(0), Some(32));
    }

    #[tokio::test]
    async fn writer_stops_when_the_client_does_not_read() {
        let (outbound, queue) = outbound::channel(1024);
        let (mut near, _far) = duplex(4);

        outbound.push(vec![0; 16]);
        let result = write_queue(&mut near, queue, Duration::from_millis(20), None, None).await;

        assert!(matches!(result, Err(ConnectionError::WriteTimeout)));
    }

    #[tokio::test]
    async fn writer_writes_in_order_and_ends_at_close() {
        let (outbound, queue) = outbound::channel(1024);
        let (mut near, mut far) = duplex(64);

        outbound.push(vec![1, 2]);
        outbound.push(vec![3]);
        outbound.finish();
        outbound.push(vec![4]);
        write_queue(&mut near, queue, Duration::from_secs(1), None, None)
            .await
            .unwrap();
        drop(near);

        let mut written = Vec::new();
        far.read_to_end(&mut written).await.unwrap();
        assert_eq!(written, [1, 2, 3]);
    }

    /// 加密是最外的一层：长度前缀也在里面，而且一条连接上的字节是一条流，
    /// 所以要连着两批帧一起解密才能还原。
    #[tokio::test]
    async fn writer_encrypts_the_whole_stream_including_the_length() {
        let secret = [7; SHARED_SECRET_LEN];
        let (outbound, queue) = outbound::channel(1024);
        let (mut near, mut far) = duplex(1024);
        let mut batch = Vec::new();
        append_frame(&mut batch, &KeepAlive { id: 7 }).unwrap();
        outbound.push(batch.clone());
        outbound.push(batch.clone());
        outbound.finish();

        let (encrypt, mut decrypt) = Encryption::new(&secret).split();
        write_queue(
            &mut near,
            queue,
            Duration::from_secs(1),
            None,
            Some(encrypt),
        )
        .await
        .unwrap();
        drop(near);

        let mut written = Vec::new();
        far.read_to_end(&mut written).await.unwrap();
        assert_ne!(written[..batch.len()], batch[..]);
        decrypt.decrypt(&mut written);
        assert_eq!(written, [batch.clone(), batch].concat());
    }

    #[tokio::test]
    async fn writer_compresses_every_packet_when_compression_is_on() {
        let (outbound, queue) = outbound::channel(1024);
        let (mut near, mut far) = duplex(1024);
        let mut batch = Vec::new();
        append_frame(&mut batch, &KeepAlive { id: 7 }).unwrap();
        outbound.push(batch.clone());
        outbound.finish();

        write_queue(&mut near, queue, Duration::from_secs(1), Some(0), None)
            .await
            .unwrap();
        drop(near);

        let mut written = Vec::new();
        far.read_to_end(&mut written).await.unwrap();
        let frame = find_frame(&written, MAX_FRAME_LEN).unwrap().unwrap();
        let decoded = compression::decode(
            &written[frame.header_len..frame.header_len + frame.payload_len],
            0,
        )
        .unwrap();
        let original = find_frame(&batch, MAX_FRAME_LEN).unwrap().unwrap();
        let payload = &batch[original.header_len..original.header_len + original.payload_len];
        assert_eq!(decoded, payload);
    }
}
