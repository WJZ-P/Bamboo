//! Status 状态：回应服务器列表请求和延迟测量。

use mc_bamboo_protocol::{
    frame::append_frame,
    packet::status::{PongResponse, StatusRequest},
};

use super::{Connection, ConnectionError};
use crate::server::Shared;

pub(super) async fn serve(
    connection: &mut Connection,
    shared: &Shared,
) -> Result<(), ConnectionError> {
    let mut status_sent = false;
    while let Some(payload) = connection.next_frame().await? {
        match StatusRequest::decode(&payload)? {
            StatusRequest::Status if status_sent => {
                return Err(ConnectionError::RepeatedStatusRequest);
            }
            StatusRequest::Status => {
                connection.write(&shared.status_frame()).await?;
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
