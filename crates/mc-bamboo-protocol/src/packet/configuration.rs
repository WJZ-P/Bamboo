//! Configuration 状态：进入世界之前同步注册表和标签。

use mc_bamboo_data::{
    KnownPack, SyncedRegistry, TagGroup,
    packets::configuration::{clientbound, serverbound},
};

use super::{ClientboundPacket, play::ClientInformation};
use crate::{
    codec::{DecodeError, Reader, write_bool, write_string, write_varint},
    text::Component,
};

/// 官方允许客户端声明的内容包数量上限。
const MAX_KNOWN_PACKS: usize = 64;
const MAX_STRING_CHARS: usize = 32767;

/// 客户端在 Configuration 状态发送的数据包。
///
/// 这里只解码服务端关心的包。其他合法的包归入 `Other`，由调用方忽略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigurationRequest<'a> {
    /// 客户端确认了哪些内容包，内容是 (命名空间, ID, 版本)。
    SelectKnownPacks(Vec<(&'a str, &'a str, &'a str)>),
    /// 客户端的设置，进入 Play 状态后要用。
    ClientInformation(ClientInformation),
    /// 客户端已经收到全部配置，准备进入 Play 状态。
    FinishConfiguration,
    Other(i32),
}

impl<'a> ConfigurationRequest<'a> {
    /// 解码一帧的负载，负载以数据包 ID 开头。
    pub fn decode(payload: &'a [u8]) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(payload);
        match reader.varint()? {
            serverbound::SELECT_KNOWN_PACKS => {
                let count = reader.count(MAX_KNOWN_PACKS)?;
                let mut packs = Vec::with_capacity(count);
                for _ in 0..count {
                    packs.push((
                        reader.string(MAX_STRING_CHARS)?,
                        reader.string(MAX_STRING_CHARS)?,
                        reader.string(MAX_STRING_CHARS)?,
                    ));
                }
                reader.finish()?;
                Ok(Self::SelectKnownPacks(packs))
            }
            serverbound::CLIENT_INFORMATION => {
                let information = ClientInformation::read(&mut reader)?;
                reader.finish()?;
                Ok(Self::ClientInformation(information))
            }
            serverbound::FINISH_CONFIGURATION => {
                reader.finish()?;
                Ok(Self::FinishConfiguration)
            }
            other => Ok(Self::Other(other)),
        }
    }
}

/// 自定义数据通道，这里用来告诉客户端服务端的名字。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Brand<'a> {
    pub name: &'a str,
}

impl ClientboundPacket for Brand<'_> {
    const ID: i32 = clientbound::CUSTOM_PAYLOAD;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_string(out, "minecraft:brand");
        write_string(out, self.name);
    }
}

/// 告诉客户端启用了哪些功能开关。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateEnabledFeatures<'a> {
    pub features: &'a [&'a str],
}

impl ClientboundPacket for UpdateEnabledFeatures<'_> {
    const ID: i32 = clientbound::UPDATE_ENABLED_FEATURES;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.features.len() as i32);
        for feature in self.features {
            write_string(out, feature);
        }
    }
}

/// 询问客户端认识哪些内容包。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectKnownPacks<'a> {
    pub packs: &'a [KnownPack],
}

impl ClientboundPacket for SelectKnownPacks<'_> {
    const ID: i32 = clientbound::SELECT_KNOWN_PACKS;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.packs.len() as i32);
        for pack in self.packs {
            write_string(out, pack.namespace);
            write_string(out, pack.id);
            write_string(out, pack.version);
        }
    }
}

/// 同步一个注册表。
///
/// 双方都认识官方内容包时，客户端可以从本地读取内容，这里只需要发送条目名和顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryData<'a> {
    pub registry: &'a SyncedRegistry,
}

impl ClientboundPacket for RegistryData<'_> {
    const ID: i32 = clientbound::REGISTRY_DATA;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_string(out, self.registry.name);
        write_varint(out, self.registry.entries.len() as i32);
        for entry in self.registry.entries {
            write_string(out, entry);
            // 不附带内容。
            write_bool(out, false);
        }
    }
}

/// 同步所有标签。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateTags<'a> {
    pub groups: &'a [TagGroup],
}

impl ClientboundPacket for UpdateTags<'_> {
    const ID: i32 = clientbound::UPDATE_TAGS;

    fn write_body(&self, out: &mut Vec<u8>) {
        write_varint(out, self.groups.len() as i32);
        for group in self.groups {
            write_string(out, group.registry);
            write_varint(out, group.tags.len() as i32);
            for tag in group.tags {
                write_string(out, tag.name);
                write_varint(out, tag.ids.len() as i32);
                for &id in tag.ids {
                    write_varint(out, id);
                }
            }
        }
    }
}

/// 配置完成，通知客户端进入 Play 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinishConfiguration;

impl ClientboundPacket for FinishConfiguration {
    const ID: i32 = clientbound::FINISH_CONFIGURATION;

    fn write_body(&self, _out: &mut Vec<u8>) {}
}

/// 在 Configuration 状态断开连接。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConfigurationDisconnect<'a> {
    pub reason: &'a Component,
}

impl ClientboundPacket for ConfigurationDisconnect<'_> {
    const ID: i32 = clientbound::DISCONNECT;

    fn write_body(&self, out: &mut Vec<u8>) {
        self.reason.write(out);
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::Tag;

    use super::*;
    use crate::{
        frame::{MAX_FRAME_LEN, find_frame},
        packet::{
            play::{ChatVisibility, HumanoidArm, ParticleStatus},
            test_util::{body, hex},
        },
    };

    #[test]
    fn decodes_known_packs() {
        let mut payload = vec![serverbound::SELECT_KNOWN_PACKS as u8];
        write_varint(&mut payload, 1);
        write_string(&mut payload, "minecraft");
        write_string(&mut payload, "core");
        write_string(&mut payload, "26.3");

        assert_eq!(
            ConfigurationRequest::decode(&payload),
            Ok(ConfigurationRequest::SelectKnownPacks(vec![(
                "minecraft",
                "core",
                "26.3"
            )]))
        );
    }

    #[test]
    fn rejects_too_many_known_packs() {
        let mut payload = vec![serverbound::SELECT_KNOWN_PACKS as u8];
        write_varint(&mut payload, MAX_KNOWN_PACKS as i32 + 1);

        assert_eq!(
            ConfigurationRequest::decode(&payload),
            Err(DecodeError::InvalidCount(MAX_KNOWN_PACKS as i32 + 1))
        );
    }

    #[test]
    fn decodes_client_information() {
        let frame = hex("0f 00 05 7a 68 5f 63 6e 0c 00 01 7f 01 00 01 00");
        let bounds = find_frame(&frame, MAX_FRAME_LEN).unwrap().unwrap();
        assert_eq!(frame.len(), bounds.header_len + bounds.payload_len);

        assert_eq!(
            ConfigurationRequest::decode(&frame[bounds.header_len..]),
            Ok(ConfigurationRequest::ClientInformation(ClientInformation {
                language: "zh_cn".to_owned(),
                view_distance: 12,
                chat_visibility: ChatVisibility::Full,
                chat_colors: true,
                model_customisation: 0x7f,
                main_hand: HumanoidArm::Right,
                text_filtering_enabled: false,
                allows_listing: true,
                particle_status: ParticleStatus::All,
            }))
        );
    }

    #[test]
    fn rejects_trailing_bytes_after_client_information() {
        let payload = hex("00 05 7a 68 5f 63 6e 0c 00 01 7f 01 00 01 00 ff");
        assert_eq!(
            ConfigurationRequest::decode(&payload),
            Err(DecodeError::TrailingBytes(1))
        );
    }

    #[test]
    fn leaves_unhandled_packets_to_the_caller() {
        let payload = [serverbound::PONG as u8, 1, 2, 3];
        assert_eq!(
            ConfigurationRequest::decode(&payload),
            Ok(ConfigurationRequest::Other(serverbound::PONG))
        );
    }

    #[test]
    fn disconnect_reason_is_a_component() {
        let reason = Component::text("hi");
        assert_eq!(
            body(&ConfigurationDisconnect { reason: &reason }),
            [8, 0, 2, b'h', b'i']
        );
    }

    #[test]
    fn registry_entries_carry_no_data() {
        let registry = SyncedRegistry {
            name: "a:b",
            entries: &["c:d"],
        };
        let mut body = Vec::new();
        RegistryData {
            registry: &registry,
        }
        .write_body(&mut body);

        assert_eq!(body, [3, b'a', b':', b'b', 1, 3, b'c', b':', b'd', 0]);
    }

    #[test]
    fn tags_list_their_entry_ids() {
        let groups = [TagGroup {
            registry: "a:b",
            tags: &[Tag {
                name: "c:d",
                ids: &[1, 300],
            }],
        }];
        let mut body = Vec::new();
        UpdateTags { groups: &groups }.write_body(&mut body);

        assert_eq!(
            body,
            [
                1, 3, b'a', b':', b'b', 1, 3, b'c', b':', b'd', 2, 1, 0xac, 0x02
            ]
        );
    }
}
