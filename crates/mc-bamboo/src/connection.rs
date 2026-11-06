//! One client connection, from the handshake until it is closed.

use std::{io, net::SocketAddr, time::Duration};

use bytes::{Buf, Bytes, BytesMut};
use mc_bamboo_protocol::{
    codec::DecodeError,
    frame::{FrameError, append_frame, find_frame},
    packet::{Handshake, Intent, PongResponse, StatusRequest},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use tracing::debug;

use crate::PreEncodedResponses;

/// Frames before Play are tiny: the largest legal handshake is 780 bytes. Staying far below
/// the protocol's 2 MiB limit keeps a connection cheap while it is still unauthenticated.
const MAX_PRE_PLAY_FRAME_LEN: usize = 1024;

#[derive(Debug, thiserror::Error)]
enum ConnectionError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error("connection closed in the middle of a frame")]
    TruncatedFrame,
    #[error("client requested the status twice")]
    RepeatedStatusRequest,
}

/// Runs a connection to its end, giving up after `time_limit`.
///
/// The limit covers the whole exchange rather than each read, so a client cannot hold a slot
/// by trickling in one byte at a time.
pub(crate) async fn handle(
    stream: TcpStream,
    peer: SocketAddr,
    responses: &PreEncodedResponses,
    time_limit: Duration,
) {
    if let Err(error) = stream.set_nodelay(true) {
        debug!(%peer, %error, "failed to disable Nagle's algorithm");
    }

    match timeout(time_limit, serve(Connection::new(stream), responses)).await {
        Ok(Ok(())) => debug!(%peer, "connection finished"),
        Ok(Err(error)) => debug!(%peer, %error, "dropping connection"),
        Err(_) => debug!(%peer, "connection timed out"),
    }
}

async fn serve(
    mut connection: Connection,
    responses: &PreEncodedResponses,
) -> Result<(), ConnectionError> {
    let Some(payload) = connection.next_frame().await? else {
        return Ok(());
    };
    let handshake = Handshake::decode(&payload)?;
    debug!(?handshake, "received handshake");

    match handshake.intent {
        Intent::Status => serve_status(&mut connection, responses).await?,
        Intent::Login | Intent::Transfer => connection.write(&responses.login_rejected).await?,
    }
    connection.finish().await?;
    Ok(())
}

async fn serve_status(
    connection: &mut Connection,
    responses: &PreEncodedResponses,
) -> Result<(), ConnectionError> {
    let mut status_sent = false;
    while let Some(payload) = connection.next_frame().await? {
        match StatusRequest::decode(&payload)? {
            StatusRequest::Status if status_sent => {
                return Err(ConnectionError::RepeatedStatusRequest);
            }
            StatusRequest::Status => {
                connection.write(&responses.status).await?;
                status_sent = true;
            }
            StatusRequest::Ping(payload) => {
                let mut pong = Vec::new();
                append_frame(&mut pong, &PongResponse { payload })?;
                connection.write(&pong).await?;
                return Ok(());
            }
        }
    }
    Ok(())
}

/// A TCP stream split into protocol frames.
struct Connection {
    stream: TcpStream,
    read_buffer: BytesMut,
}

impl Connection {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            read_buffer: BytesMut::with_capacity(MAX_PRE_PLAY_FRAME_LEN),
        }
    }

    /// Returns the next frame's payload, or `None` once the client has closed the connection.
    async fn next_frame(&mut self) -> Result<Option<Bytes>, ConnectionError> {
        loop {
            if let Some(frame) = find_frame(&self.read_buffer, MAX_PRE_PLAY_FRAME_LEN)? {
                self.read_buffer.advance(frame.header_len);
                return Ok(Some(self.read_buffer.split_to(frame.payload_len).freeze()));
            }
            if self.stream.read_buf(&mut self.read_buffer).await? == 0 {
                return if self.read_buffer.is_empty() {
                    Ok(None)
                } else {
                    Err(ConnectionError::TruncatedFrame)
                };
            }
        }
    }

    async fn write(&mut self, frames: &[u8]) -> io::Result<()> {
        self.stream.write_all(frames).await
    }

    /// Closes the connection after the last write.
    ///
    /// Closing a socket with unread input makes the OS send a reset, which can destroy our
    /// last packet before the client reads it. A client that is refused right after the
    /// handshake is typically still sending, so this waits for it to close first.
    async fn finish(&mut self) -> io::Result<()> {
        self.stream.shutdown().await?;
        let mut discarded = [0; 256];
        while self.stream.read(&mut discarded).await? != 0 {}
        Ok(())
    }
}
