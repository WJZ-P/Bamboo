//! Configuration 状态：进入世界之前同步注册表和标签。

use mc_bamboo_data::registries::KNOWN_PACKS;
use mc_bamboo_protocol::{
    GAME_VERSION,
    frame::append_frame,
    packet::configuration::{ConfigurationDisconnect, ConfigurationRequest},
};

use super::{Connection, ConnectionError};
use crate::server::Shared;

pub(super) async fn serve(
    connection: &mut Connection,
    shared: &Shared,
) -> Result<(), ConnectionError> {
    connection
        .write(&shared.responses.configuration_start)
        .await?;

    let mut data_sent = false;
    loop {
        let Some(payload) = connection.next_frame().await? else {
            return Err(ConnectionError::ClosedBeforeJoining);
        };
        match ConfigurationRequest::decode(&payload)? {
            ConfigurationRequest::SelectKnownPacks(_) if data_sent => {
                return Err(ConnectionError::UnexpectedPacket);
            }
            ConfigurationRequest::SelectKnownPacks(packs) => {
                // 我们只发送条目名，内容要靠客户端本地的官方数据。
                if !knows_official_packs(&packs) {
                    refuse(connection).await?;
                    return Err(ConnectionError::UnknownDataPacks);
                }
                connection
                    .write(&shared.responses.configuration_data)
                    .await?;
                data_sent = true;
            }
            ConfigurationRequest::FinishConfiguration if data_sent => return Ok(()),
            ConfigurationRequest::FinishConfiguration => {
                return Err(ConnectionError::UnexpectedPacket);
            }
            // 客户端设置、自定义通道之类的包，现在都用不上。
            ConfigurationRequest::Other(_) => {}
        }
    }
}

fn knows_official_packs(packs: &[(&str, &str, &str)]) -> bool {
    packs.len() == KNOWN_PACKS.len()
        && packs.iter().zip(KNOWN_PACKS).all(|(client, official)| {
            *client == (official.namespace, official.id, official.version)
        })
}

async fn refuse(connection: &mut Connection) -> Result<(), ConnectionError> {
    let reason = format!("客户端缺少 Minecraft {GAME_VERSION} 的官方数据");
    let mut frame = Vec::new();
    append_frame(&mut frame, &ConfigurationDisconnect { reason: &reason })?;
    connection.write(&frame).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_exactly_the_official_packs() {
        assert!(knows_official_packs(&[("minecraft", "core", GAME_VERSION)]));

        assert!(!knows_official_packs(&[]));
        assert!(!knows_official_packs(&[("minecraft", "core", "1.0")]));
    }
}
