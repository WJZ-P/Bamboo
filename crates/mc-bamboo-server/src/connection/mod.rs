//! 单个客户端连接，从握手到关闭的全过程。
//!
//! 连接会依次经过 Handshake、Status 或 Login、Configuration、Play 这些状态。
//! 每个状态放在一个子模块里，这里只负责分发和帧的收发。

mod configuration;
mod login;
mod outbound;
mod play;
mod status;

use std::{io, net::SocketAddr, sync::Arc};

use bytes::{Buf, Bytes, BytesMut};
use mc_bamboo_protocol::{
    GAME_VERSION, PROTOCOL_VERSION,
    codec::DecodeError,
    compression::{self, CompressionError},
    frame::{FrameError, MAX_FRAME_LEN, append_frame, find_frame},
    packet::{
        handshake::{Handshake, Intent},
        login::LoginDisconnect,
        play::ClientInformation,
    },
};
use serde_json::json;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::OwnedSemaphorePermit,
    time::timeout,
};
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::{players::PlayerSession, server::Shared};

#[cfg(test)]
pub(crate) use outbound::MAX_QUEUED_BYTES;

/// 登录之前的帧都很小：最大的合法握手包只有 780 字节。
/// 把上限压到远低于协议允许的 2 MiB，未认证的连接就占不了多少内存。
const MAX_PRE_LOGIN_FRAME_LEN: usize = 1024;

#[derive(Debug, thiserror::Error)]
enum ConnectionError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error(transparent)]
    Compression(#[from] CompressionError),
    #[error("connection closed in the middle of a frame")]
    TruncatedFrame,
    #[error("connection closed before joining the game")]
    ClosedBeforeJoining,
    #[error("client requested the status twice")]
    RepeatedStatusRequest,
    #[error("invalid player name")]
    InvalidPlayerName,
    #[error("packet is not allowed at this point")]
    UnexpectedPacket,
    #[error("client does not know the official data packs")]
    UnknownDataPacks,
    #[error("client did not answer the keep-alive")]
    KeepAliveTimeout,
    #[error("writing to the client took too long")]
    WriteTimeout,
    #[error("client sent requests faster than the world can handle them")]
    TooManyRequests,
    #[error("client does not read fast enough, its outbound queue is full")]
    OutboundOverflow,
    #[error("the same player logged in again")]
    DuplicateLogin,
    #[error("the server is stopping")]
    ServerStopped,
}

impl ConnectionError {
    /// 在 Play 状态告诉客户端的断开原因，是客户端语言文件里的翻译键。
    /// 返回 `None` 时直接断开：要么写不出去了，要么是客户端自己的问题。
    fn disconnect_reason(&self) -> Option<&'static str> {
        match self {
            Self::KeepAliveTimeout => Some("disconnect.timeout"),
            Self::TooManyRequests => Some("disconnect.exceeded_packet_rate"),
            Self::DuplicateLogin => Some("multiplayer.disconnect.duplicate_login"),
            Self::ServerStopped => Some("multiplayer.disconnect.server_shutdown"),
            _ => None,
        }
    }
}

/// 走完进入游戏之前的流程的玩家。
struct Joined {
    session: PlayerSession,
    /// Configuration 阶段收到的客户端设置，没收到时是默认值。
    client: ClientInformation,
}

/// 运行一条连接直到结束。
///
/// 进入游戏之前的时限覆盖整个过程，而不是每次读取，
/// 这样客户端没法靠一个字节一个字节地慢慢发来占住名额。
pub(crate) async fn handle(
    stream: TcpStream,
    peer: SocketAddr,
    shared: Arc<Shared>,
    slot: OwnedSemaphorePermit,
    cancel: CancellationToken,
) {
    if let Err(error) = stream.set_nodelay(true) {
        debug!(%peer, %error, "failed to disable Nagle's algorithm");
    }

    let mut connection = Connection::new(stream);
    let time_limit = shared.config.pre_play_timeout;
    let pre_play = timeout(time_limit, pre_play(&mut connection, &shared));
    let joined = tokio::select! {
        () = cancel.cancelled() => {
            debug!(%peer, "connection closed by shutdown");
            return;
        }
        outcome = pre_play => match outcome {
            Ok(Ok(joined)) => joined,
            Ok(Err(error)) => {
                debug!(%peer, %error, "dropping connection");
                return;
            }
            Err(_) => {
                debug!(%peer, "connection timed out");
                return;
            }
        },
    };

    // 名额只限制进入游戏之前的连接，玩家数量由玩家表限制。
    drop(slot);
    let Some(Joined { session, client }) = joined else {
        debug!(%peer, "connection finished");
        return;
    };

    if let Err(error) = play::serve(connection, &session, client, &shared, cancel).await {
        debug!(%peer, %error, "player connection ended");
    }
}

/// 处理进入游戏之前的全部流程。玩家成功进入游戏时返回它的在线状态。
async fn pre_play(
    connection: &mut Connection,
    shared: &Arc<Shared>,
) -> Result<Option<Joined>, ConnectionError> {
    let Some(payload) = connection.next_frame().await? else {
        return Ok(None);
    };
    let handshake = Handshake::decode(&payload)?;
    debug!(?handshake, "received handshake");

    match handshake.intent {
        Intent::Status => {
            status::serve(connection, shared).await?;
            connection.finish().await?;
            Ok(None)
        }
        Intent::Transfer => {
            connection.refuse_login("这个服务器不接受转移连接").await?;
            Ok(None)
        }
        Intent::Login if handshake.protocol_version != PROTOCOL_VERSION => {
            let reason = format!("请使用 Minecraft {GAME_VERSION} 连接这个服务器");
            connection.refuse_login(&reason).await?;
            Ok(None)
        }
        Intent::Login => {
            let Some(session) = login::admit(connection, shared).await? else {
                return Ok(None);
            };
            // 还没进入 Play 时被同名玩家顶掉，连接直接结束就好，客户端看到的是连接断开。
            let client = tokio::select! {
                () = session.kicked() => return Err(ConnectionError::DuplicateLogin),
                client = configure(connection, &session, shared) => client?,
            };
            Ok(Some(Joined { session, client }))
        }
    }
}

async fn configure(
    connection: &mut Connection,
    session: &PlayerSession,
    shared: &Shared,
) -> Result<ClientInformation, ConnectionError> {
    login::finish(connection, session, shared).await?;
    configuration::serve(connection, shared).await
}

/// 返回下一帧的负载。客户端关闭连接后返回 `None`。
///
/// 这个函数是取消安全的：放进 `select!` 被打断时，已读到的数据都留在缓冲区里。
async fn read_frame<R: AsyncRead + Unpin>(
    stream: &mut R,
    buffer: &mut BytesMut,
    max_frame_len: usize,
    compression: Option<i32>,
) -> Result<Option<Bytes>, ConnectionError> {
    loop {
        if let Some(frame) = find_frame(buffer, max_frame_len)? {
            buffer.advance(frame.header_len);
            let payload = buffer.split_to(frame.payload_len).freeze();
            return Ok(Some(match compression {
                Some(threshold) => compression::decode(&payload, threshold)?.into(),
                None => payload,
            }));
        }
        if stream.read_buf(buffer).await? == 0 {
            return if buffer.is_empty() {
                Ok(None)
            } else {
                Err(ConnectionError::TruncatedFrame)
            };
        }
    }
}

/// 写完最后一个包之后关闭连接。
///
/// 如果 socket 里还有没读的数据，直接关闭会让系统发送 reset，
/// 客户端可能还没读到最后一个包就被丢弃了。握手后马上被拒绝的客户端通常还在发数据，
/// 所以这里要等客户端先关闭。
async fn finish(stream: &mut TcpStream) -> io::Result<()> {
    stream.shutdown().await?;
    let mut discarded = [0; 256];
    while stream.read(&mut discarded).await? != 0 {}
    Ok(())
}

/// 切分成协议帧的 TCP 流。
struct Connection {
    stream: TcpStream,
    read_buffer: BytesMut,
    max_frame_len: usize,
    /// 压缩启用后是服务端声明的阈值，两个方向都换成压缩帧。
    compression: Option<i32>,
}

impl Connection {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            read_buffer: BytesMut::with_capacity(MAX_PRE_LOGIN_FRAME_LEN),
            max_frame_len: MAX_PRE_LOGIN_FRAME_LEN,
            compression: None,
        }
    }

    /// 登录完成后放开帧大小限制，客户端可能发送较大的配置数据。
    fn allow_full_size_frames(&mut self) {
        self.max_frame_len = MAX_FRAME_LEN;
    }

    /// 从下一个包开始两个方向都用压缩帧。调用前要保证登录压缩包已经写进 socket。
    fn enable_compression(&mut self, threshold: i32) {
        self.compression = Some(threshold);
    }

    async fn next_frame(&mut self) -> Result<Option<Bytes>, ConnectionError> {
        read_frame(
            &mut self.stream,
            &mut self.read_buffer,
            self.max_frame_len,
            self.compression,
        )
        .await
    }

    /// 写一批帧。压缩启用后逐个包压缩，世界线程交来的帧本身不压缩。
    async fn write(&mut self, frames: &[u8]) -> Result<(), ConnectionError> {
        match self.compression {
            Some(threshold) => {
                let compressed = compression::encode_frames(frames, threshold)?;
                Ok(self.stream.write_all(&compressed).await?)
            }
            None => Ok(self.stream.write_all(frames).await?),
        }
    }

    /// 发送登录阶段的断开消息并关闭连接。
    async fn refuse_login(&mut self, reason: &str) -> Result<(), ConnectionError> {
        let reason_json = json!({ "text": reason }).to_string();
        let mut frame = Vec::new();
        append_frame(
            &mut frame,
            &LoginDisconnect {
                reason_json: &reason_json,
            },
        )?;
        self.write(&frame).await?;
        self.finish().await?;
        Ok(())
    }

    async fn finish(&mut self) -> io::Result<()> {
        finish(&mut self.stream).await
    }
}
