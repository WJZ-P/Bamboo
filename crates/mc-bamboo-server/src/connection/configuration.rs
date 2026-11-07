//! Configuration 状态：进入世界之前同步注册表和标签。

use mc_bamboo_data::registries::KNOWN_PACKS;
use mc_bamboo_protocol::{
    GAME_VERSION,
    frame::append_frame,
    packet::{
        configuration::{ConfigurationDisconnect, ConfigurationRequest},
        play::ClientInformation,
    },
    text::Component,
};

use super::{Connection, ConnectionError};
use crate::server::Shared;

/// 走完 Configuration，返回客户端的设置。客户端没发时是默认值，和官方相同。
pub(super) async fn serve(
    connection: &mut Connection,
    shared: &Shared,
) -> Result<ClientInformation, ConnectionError> {
    connection
        .write(&shared.responses.configuration_start)
        .await?;

    let mut client = ClientInformation::default();
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
            ConfigurationRequest::ClientInformation(information) => client = information,
            ConfigurationRequest::FinishConfiguration if data_sent => return Ok(client),
            ConfigurationRequest::FinishConfiguration => {
                return Err(ConnectionError::UnexpectedPacket);
            }
            // 自定义通道之类的包，现在都用不上。
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
    let reason = Component::text(format!("客户端缺少 Minecraft {GAME_VERSION} 的官方数据"));
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
