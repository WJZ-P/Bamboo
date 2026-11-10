//! 假玩家发给服务端的包的线上编码。
//!
//! 这里只写客户端方向的包。`mc-bamboo-protocol` 的解码器是这些包唯一需要读懂它们的地方，
//! 所以每个编码函数都在 [`crate::wire::tests`] 里用服务端的解码器解回同样的内容。

use mc_bamboo_data::packets::{configuration, handshake, login, play};
#[cfg(test)]
use mc_bamboo_protocol::frame::{MAX_FRAME_LEN, find_frame};
use mc_bamboo_protocol::{
    GAME_VERSION, PROTOCOL_VERSION,
    codec::{write_string, write_uuid, write_varint},
    compression,
};
use uuid::Uuid;

/// 玩家请求的服务端视距。和 Bamboo 默认视距一致，不主动缩小工作负载。
pub const VIEW_DISTANCE: i8 = 8;
/// 官方客户端皮肤外层全开的掩码。
const ALL_MODEL_PARTS: u8 = 0x7f;
/// 服务端 `PlayerChunkSender` 允许的最大速率，压测时让区块尽可能快地到齐。
pub const FAST_CHUNKS_PER_TICK: f32 = 64.0;
/// 真实客户端一秒发 20 个移动包。
pub const CLIENT_TICKS_PER_SECOND: u32 = 20;
/// `player_abilities` 里表示正在飞行的位。
pub const ABILITY_FLYING: u8 = 0x02;
/// 出生点，和 `WorldState::DEFAULT_SPAWN_POS` / `flat::surface_y()` 一致。
pub const SPAWN: (f64, f64, f64) = (8.5, -60.0, 8.5);
/// `wander` 不走出一个以出生点为心的圆。
pub const WANDER_RADIUS: f64 = 8.0;

/// 一帧：长度前缀加负载。
pub fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 3);
    write_varint(&mut out, payload.len() as i32);
    out.extend_from_slice(payload);
    out
}

/// 一批帧按当前压缩阈值重新包装；压缩关着时原样返回。
pub fn encode_outgoing(frames: &[u8], threshold: Option<i32>) -> Result<Vec<u8>, String> {
    match threshold {
        Some(threshold) => compression::encode_frames(frames, threshold)
            .map_err(|error| format!("cannot compress outgoing frames: {error}")),
        None => Ok(frames.to_vec()),
    }
}

/// 从一个完整的帧里取出负载。
#[cfg(test)]
fn payload(frame: &[u8]) -> &[u8] {
    let bounds = find_frame(frame, MAX_FRAME_LEN)
        .expect("a frame we wrote is valid")
        .expect("a frame we wrote is complete");
    &frame[bounds.header_len..bounds.header_len + bounds.payload_len]
}

fn packet(id: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    write_varint(&mut payload, id);
    payload
}

fn push_position(payload: &mut Vec<u8>, (x, y, z): (f64, f64, f64)) {
    for coordinate in [x, y, z] {
        payload.extend_from_slice(&coordinate.to_be_bytes());
    }
}

/// Handshake，声明登录意图。
pub fn handshake_frame() -> Vec<u8> {
    let mut body = packet(handshake::serverbound::INTENTION);
    write_varint(&mut body, PROTOCOL_VERSION);
    write_string(&mut body, "localhost");
    body.extend_from_slice(&25565_u16.to_be_bytes());
    write_varint(&mut body, 2); // Intent::Login
    frame(&body)
}

/// Login 的 hello：离线模式只需要一个名字和 UUID。
pub fn hello_frame(name: &str) -> Vec<u8> {
    let mut body = packet(login::serverbound::HELLO);
    write_string(&mut body, name);
    write_uuid(&mut body, Uuid::nil());
    frame(&body)
}

/// 确认登录成功，之后进入 Configuration。
pub fn login_acknowledged_frame() -> Vec<u8> {
    frame(&packet(login::serverbound::LOGIN_ACKNOWLEDGED))
}

/// `client_information`：Configuration 和 Play 的格式相同。
pub fn client_information_frame() -> Vec<u8> {
    let mut body = packet(configuration::serverbound::CLIENT_INFORMATION);
    write_client_information(&mut body);
    frame(&body)
}

fn write_client_information(body: &mut Vec<u8>) {
    write_string(body, "en_us");
    body.push(VIEW_DISTANCE as u8);
    write_varint(body, 0); // chat_visibility: full
    body.push(1); // chat_colors
    body.push(ALL_MODEL_PARTS);
    write_varint(body, 1); // main_hand: right
    body.extend_from_slice(&[0, 1]); // text_filtering, allows_listing
    write_varint(body, 0); // particle_status: all
}

/// 声明认识官方内容包，否则服务端在 Configuration 阶段拒绝连接。
pub fn select_known_packs_frame() -> Vec<u8> {
    let mut body = packet(configuration::serverbound::SELECT_KNOWN_PACKS);
    write_varint(&mut body, 1);
    write_string(&mut body, "minecraft");
    write_string(&mut body, "core");
    write_string(&mut body, GAME_VERSION);
    frame(&body)
}

/// 告诉服务端配置结束，之后就是 Play。
pub fn finish_configuration_frame() -> Vec<u8> {
    frame(&packet(configuration::serverbound::FINISH_CONFIGURATION))
}

/// 心跳回应：把服务端发来的 id 原样送回。
pub fn keep_alive_frame(id: i64) -> Vec<u8> {
    frame(&packet_with_i64(play::serverbound::KEEP_ALIVE, id))
}

/// `ping_request` 的回应（服务端发的 ping 用 pong 回，此处是客户端发起的 ping）。
pub fn ping_request_frame(time: i64) -> Vec<u8> {
    frame(&packet_with_i64(play::serverbound::PING_REQUEST, time))
}

fn packet_with_i64(id: i32, value: i64) -> Vec<u8> {
    let mut body = packet(id);
    body.extend_from_slice(&value.to_be_bytes());
    body
}

/// 一批区块收完了，告诉服务端以后每 tick 可以发多少。
pub fn chunk_batch_received_frame() -> Vec<u8> {
    let mut body = packet(play::serverbound::CHUNK_BATCH_RECEIVED);
    body.extend_from_slice(&FAST_CHUNKS_PER_TICK.to_be_bytes());
    frame(&body)
}

/// 确认一次传送；包里的位置就是客户端下一次移动的位置。
pub fn accept_teleportation_frame(id: i32, position: (f64, f64, f64)) -> Vec<u8> {
    let mut body = packet(play::serverbound::ACCEPT_TELEPORTATION);
    write_varint(&mut body, id);
    push_position(&mut body, position);
    body.extend_from_slice(&0_f32.to_be_bytes()); // yaw
    body.extend_from_slice(&0_f32.to_be_bytes()); // pitch
    frame(&body)
}

/// 真实客户端画出世界之后发这个，不发服务端会丢掉移动包。
pub fn player_loaded_frame() -> Vec<u8> {
    frame(&packet(play::serverbound::PLAYER_LOADED))
}

/// 客户端 tick 结束，服务端用它清掉「这一 tick 已经收过位置」的标记。
pub fn client_tick_end_frame() -> Vec<u8> {
    frame(&packet(play::serverbound::CLIENT_TICK_END))
}

/// `move_player_pos`：位置和落地标记。
pub fn move_player_pos_frame(position: (f64, f64, f64), on_ground: bool) -> Vec<u8> {
    let mut body = packet(play::serverbound::MOVE_PLAYER_POS);
    push_position(&mut body, position);
    body.push(u8::from(on_ground));
    frame(&body)
}

/// `player_abilities`：创造模式下打开飞行，跑图场景才不会被判悬空。
pub fn player_abilities_frame(flags: u8) -> Vec<u8> {
    let mut body = packet(play::serverbound::PLAYER_ABILITIES);
    body.push(flags);
    frame(&body)
}

/// 拆开一批未压缩帧，返回每个包的负载。
#[cfg(test)]
pub fn packets(frames: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let mut out = Vec::new();
    let mut rest = frames;
    while !rest.is_empty() {
        let bounds = find_frame(rest, MAX_FRAME_LEN)
            .map_err(|error| format!("invalid frame: {error}"))?
            .ok_or_else(|| "incomplete frame".to_owned())?;
        let end = bounds.header_len + bounds.payload_len;
        out.push(rest[bounds.header_len..end].to_vec());
        rest = &rest[end..];
    }
    Ok(out)
}

/// 一个已知的客户端包 ID，用来按 ID 分发回包。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ServerPacket {
    LoginCompression(i32),
    LoginFinished,
    LoginDisconnect(&'static str),
    SelectKnownPacks,
    FinishConfiguration,
    ConfigurationDisconnect,
    ConfigurationKeepAlive(i64),
    KeepAlive(i64),
    ChunkBatchFinished,
    PlayerPosition(PlayerPosition),
    LevelChunkWithLight,
    Disconnect(&'static str),
    Other,
}

/// `player_position` 里压测要用的几个字段。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerPosition {
    pub id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// 只读包 ID 和少数必须回应的字段；其余包一律只统计，不解析。
///
/// 同一个 ID 在不同状态里含义不同（比如 2 在 Login 是 `login_finished`、
/// 在 Configuration 是断开），所以按当前状态解释。
pub fn classify(state: State, payload: &[u8]) -> ServerPacket {
    use mc_bamboo_protocol::codec::Reader;

    let mut reader = Reader::new(payload);
    let Ok(id) = reader.varint() else {
        return ServerPacket::Other;
    };
    match state {
        State::Login => match id {
            login::clientbound::LOGIN_COMPRESSION => reader
                .varint()
                .map(ServerPacket::LoginCompression)
                .unwrap_or(ServerPacket::Other),
            login::clientbound::LOGIN_FINISHED => ServerPacket::LoginFinished,
            login::clientbound::LOGIN_DISCONNECT => ServerPacket::LoginDisconnect("login"),
            _ => ServerPacket::Other,
        },
        State::Configuration => match id {
            configuration::clientbound::SELECT_KNOWN_PACKS => ServerPacket::SelectKnownPacks,
            configuration::clientbound::FINISH_CONFIGURATION => ServerPacket::FinishConfiguration,
            configuration::clientbound::DISCONNECT => ServerPacket::ConfigurationDisconnect,
            configuration::clientbound::KEEP_ALIVE => reader
                .i64()
                .map(ServerPacket::ConfigurationKeepAlive)
                .unwrap_or(ServerPacket::Other),
            _ => ServerPacket::Other,
        },
        State::Play => match id {
            play::clientbound::KEEP_ALIVE => reader
                .i64()
                .map(ServerPacket::KeepAlive)
                .unwrap_or(ServerPacket::Other),
            play::clientbound::CHUNK_BATCH_FINISHED => ServerPacket::ChunkBatchFinished,
            play::clientbound::PLAYER_POSITION => {
                let Ok(teleport_id) = reader.varint() else {
                    return ServerPacket::Other;
                };
                let (Ok(x), Ok(y), Ok(z)) = (reader.f64(), reader.f64(), reader.f64()) else {
                    return ServerPacket::Other;
                };
                ServerPacket::PlayerPosition(PlayerPosition {
                    id: teleport_id,
                    x,
                    y,
                    z,
                })
            }
            play::clientbound::LEVEL_CHUNK_WITH_LIGHT => ServerPacket::LevelChunkWithLight,
            play::clientbound::DISCONNECT => ServerPacket::Disconnect("disconnect"),
            _ => ServerPacket::Other,
        },
    }
}

/// 一条连接走到协议状态机的哪一段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Login,
    Configuration,
    Play,
}

/// 服务端断开原因的翻译键，能从 NBT 文本组件里读出来时返回它。
///
/// 只认 `translate` 字段，其他形状的包一律显示通用原因。
pub fn disconnect_key(state: State, payload: &[u8]) -> Option<String> {
    use mc_bamboo_protocol::codec::Reader;

    let mut reader = Reader::new(payload);
    let id = reader.varint().ok()?;
    match state {
        State::Login => {
            // Login 阶段的原因是 JSON 文本组件，不是 NBT。
            let reason = reader.string(32767).ok()?;
            let value = serde_json::from_str::<serde_json::Value>(reason).ok()?;
            return value
                .get("translate")
                .and_then(|key| key.as_str())
                .or_else(|| value.get("text").and_then(|text| text.as_str()))
                .map(str::to_owned);
        }
        State::Configuration | State::Play => {
            let expected = if state == State::Configuration {
                configuration::clientbound::DISCONNECT
            } else {
                play::clientbound::DISCONNECT
            };
            if id != expected {
                return None;
            }
        }
    }
    let rest = reader.remaining();
    let mut nbt = mc_bamboo_nbt::Reader::new(rest, mc_bamboo_nbt::Accounter::default_quota());
    let tag = nbt.any_tag().ok()??;
    nbt_tag_key(&tag)
}

fn nbt_tag_key(tag: &mc_bamboo_nbt::Tag) -> Option<String> {
    match tag {
        mc_bamboo_nbt::Tag::Compound(entries) => entries
            .iter()
            .find(|(name, _)| name == "translate")
            .and_then(|(_, value)| match value {
                mc_bamboo_nbt::Tag::String(key) => Some(key.clone()),
                _ => None,
            })
            .or_else(|| {
                entries
                    .iter()
                    .find(|(name, _)| name == "text")
                    .and_then(|(_, value)| match value {
                        mc_bamboo_nbt::Tag::String(text) => Some(text.clone()),
                        _ => None,
                    })
            }),
        mc_bamboo_nbt::Tag::String(text) => Some(text.clone()),
        _ => None,
    }
}

/// 当前帧用的包 ID。压缩帧要先解压再调用。
pub fn packet_id(payload: &[u8]) -> Option<i32> {
    mc_bamboo_protocol::codec::Reader::new(payload)
        .varint()
        .ok()
}

/// 从一帧里取出压缩阈值。供单元测试和连接状态检查使用。
pub fn login_compression_threshold(payload: &[u8]) -> Option<i32> {
    let mut reader = mc_bamboo_protocol::codec::Reader::new(payload);
    (reader.varint().ok()? == login::clientbound::LOGIN_COMPRESSION)
        .then(|| reader.varint().ok())
        .flatten()
}

/// 供测试使用：把一个服务端方向的已知包编成帧。
#[cfg(test)]
pub fn server_frame<P: mc_bamboo_protocol::packet::ClientboundPacket>(packet: &P) -> Vec<u8> {
    let mut out = Vec::new();
    mc_bamboo_protocol::frame::append_frame(&mut out, packet).expect("a test frame should fit");
    out
}

#[cfg(test)]
pub fn frame_payload(frame: &[u8]) -> &[u8] {
    payload(frame)
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::{Vec3, mth};
    use mc_bamboo_protocol::packet::{
        configuration::ConfigurationRequest,
        handshake::{Handshake, Intent},
        login::LoginRequest,
        play::{ClientInformation, PlayRequest},
    };

    use super::*;

    /// 每种客户端方向包的编码都要能被服务端的解码器解回同样的内容。
    fn single(frames: &[u8]) -> Vec<u8> {
        let packets = packets(frames).expect("frames");
        assert_eq!(packets.len(), 1, "exactly one packet per frame");
        packets.into_iter().next().unwrap()
    }

    /// 解码器返回借用负载的值，负载要活到断言结束。
    macro_rules! body {
        ($frames:expr) => {{
            let body = single(&$frames);
            body
        }};
    }

    #[test]
    fn handshake_decodes_back() {
        let body = single(&handshake_frame());
        let handshake = Handshake::decode(&body).unwrap();
        assert_eq!(handshake.protocol_version, PROTOCOL_VERSION);
        assert_eq!(handshake.server_address, "localhost");
        assert_eq!(handshake.server_port, 25565);
        assert_eq!(handshake.intent, Intent::Login);
    }

    #[test]
    fn hello_decodes_back() {
        let body = body!(hello_frame("Bot7"));
        let hello = LoginRequest::decode(&body).unwrap();
        assert_eq!(
            hello,
            LoginRequest::Hello {
                name: "Bot7",
                uuid: Uuid::nil()
            }
        );
    }

    #[test]
    fn login_acknowledged_decodes_back() {
        let body = body!(login_acknowledged_frame());
        let request = LoginRequest::decode(&body).unwrap();
        assert_eq!(request, LoginRequest::Acknowledged);
    }

    #[test]
    fn client_information_decodes_back() {
        let body = body!(client_information_frame());
        let request = ConfigurationRequest::decode(&body).unwrap();
        let ConfigurationRequest::ClientInformation(info) = request else {
            panic!("expected client_information, got {request:?}");
        };
        assert_eq!(
            info,
            ClientInformation {
                view_distance: VIEW_DISTANCE,
                model_customisation: ALL_MODEL_PARTS,
                chat_colors: true,
                allows_listing: true,
                ..ClientInformation::default()
            }
        );
    }

    #[test]
    fn select_known_packs_decodes_back() {
        let body = body!(select_known_packs_frame());
        let request = ConfigurationRequest::decode(&body).unwrap();
        assert_eq!(
            request,
            ConfigurationRequest::SelectKnownPacks(vec![("minecraft", "core", GAME_VERSION)])
        );
    }

    #[test]
    fn finish_configuration_decodes_back() {
        let body = body!(finish_configuration_frame());
        let request = ConfigurationRequest::decode(&body).unwrap();
        assert_eq!(request, ConfigurationRequest::FinishConfiguration);
    }

    #[test]
    fn keep_alive_decodes_back() {
        let request =
            PlayRequest::decode(&single(&keep_alive_frame(0x0102_0304_0506_0708))).unwrap();
        assert_eq!(request, PlayRequest::KeepAlive(0x0102_0304_0506_0708));
    }

    #[test]
    fn ping_request_decodes_back() {
        let request = PlayRequest::decode(&single(&ping_request_frame(-7))).unwrap();
        assert_eq!(request, PlayRequest::PingRequest(-7));
    }

    #[test]
    fn chunk_batch_received_decodes_back() {
        let request = PlayRequest::decode(&single(&chunk_batch_received_frame())).unwrap();
        assert_eq!(
            request,
            PlayRequest::ChunkBatchReceived {
                desired_chunks_per_tick: FAST_CHUNKS_PER_TICK
            }
        );
    }

    #[test]
    fn accept_teleportation_decodes_back() {
        let request =
            PlayRequest::decode(&single(&accept_teleportation_frame(3, (1.5, -60.0, -2.25))))
                .unwrap();
        assert_eq!(
            request,
            PlayRequest::AcceptTeleportation {
                id: 3,
                x: 1.5,
                y: -60.0,
                z: -2.25,
                yaw: 0.0,
                pitch: 0.0,
            }
        );
    }

    #[test]
    fn move_player_pos_decodes_back() {
        let request =
            PlayRequest::decode(&single(&move_player_pos_frame((12.5, -60.0, -30.25), true)))
                .unwrap();
        assert_eq!(
            request,
            PlayRequest::MovePlayer {
                position: Some(Vec3::new(12.5, -60.0, -30.25)),
                rotation: None,
                on_ground: true,
                horizontal_collision: false,
            }
        );
    }

    #[test]
    fn player_abilities_decodes_back() {
        let request =
            PlayRequest::decode(&single(&player_abilities_frame(ABILITY_FLYING))).unwrap();
        assert_eq!(
            request,
            PlayRequest::PlayerAbilities {
                flags: ABILITY_FLYING
            }
        );
    }

    #[test]
    fn bodyless_play_packets_decode_back() {
        for (frames, expected) in [
            (player_loaded_frame(), play::serverbound::PLAYER_LOADED),
            (client_tick_end_frame(), play::serverbound::CLIENT_TICK_END),
        ] {
            let request = PlayRequest::decode(&single(&frames)).unwrap();
            assert_eq!(request, PlayRequest::Other(expected));
        }
    }

    /// 压缩层在两个方向都要能往返：服务端用 `compression::decode` 读我们写的字节。
    #[test]
    fn compressed_outgoing_frames_round_trip() {
        let frames = [
            client_information_frame(),
            hello_frame("Bot7"),
            move_player_pos_frame(SPAWN, true),
        ]
        .concat();
        for threshold in [0, 1, 256, 1024] {
            let encoded = encode_outgoing(&frames, Some(threshold)).unwrap();
            let encoded_packets = packets(&encoded).unwrap();
            let original = packets(&frames).unwrap();
            assert_eq!(
                encoded_packets.len(),
                original.len(),
                "threshold {threshold}"
            );
            for (compressed, expected) in encoded_packets.iter().zip(&original) {
                assert_eq!(
                    compression::decode(compressed, threshold).unwrap(),
                    *expected,
                    "threshold {threshold}"
                );
            }
        }
        assert_eq!(encode_outgoing(&frames, None).unwrap(), frames);
    }

    #[test]
    fn classify_reads_the_packets_that_need_an_answer() {
        use mc_bamboo_protocol::packet::{
            login::{LoginCompression, LoginDisconnect, LoginFinished},
            play::{Disconnect, KeepAlive, PlayerPosition as ServerPlayerPosition},
        };
        use mc_bamboo_protocol::text::Component;

        let compression = server_frame(&LoginCompression { threshold: 256 });
        assert_eq!(
            classify(State::Login, frame_payload(&compression)),
            ServerPacket::LoginCompression(256)
        );
        assert_eq!(
            login_compression_threshold(frame_payload(&compression)),
            Some(256)
        );

        let finished = server_frame(&LoginFinished {
            uuid: Uuid::nil(),
            name: "Bot7",
            properties: &[],
            session_id: Uuid::nil(),
        });
        assert_eq!(
            classify(State::Login, frame_payload(&finished)),
            ServerPacket::LoginFinished
        );

        let refused = server_frame(&LoginDisconnect { reason_json: "{}" });
        assert!(matches!(
            classify(State::Login, frame_payload(&refused)),
            ServerPacket::LoginDisconnect(_)
        ));

        let keep_alive = server_frame(&KeepAlive { id: 99 });
        assert_eq!(
            classify(State::Play, frame_payload(&keep_alive)),
            ServerPacket::KeepAlive(99)
        );

        let position = server_frame(&ServerPlayerPosition {
            teleport_id: 1,
            x: 8.5,
            y: -60.0,
            z: 8.5,
            yaw: 0.0,
            pitch: 0.0,
            relative: 0,
        });
        assert_eq!(
            classify(State::Play, frame_payload(&position)),
            ServerPacket::PlayerPosition(super::PlayerPosition {
                id: 1,
                x: 8.5,
                y: -60.0,
                z: 8.5
            })
        );

        let reason = Component::translate("disconnect.timeout", []);
        let disconnect = server_frame(&Disconnect { reason: &reason });
        assert!(matches!(
            classify(State::Play, frame_payload(&disconnect)),
            ServerPacket::Disconnect(_)
        ));
        assert_eq!(
            disconnect_key(State::Play, frame_payload(&disconnect)).as_deref(),
            Some("disconnect.timeout")
        );
    }

    #[test]
    fn unknown_packets_are_only_counted() {
        assert_eq!(
            classify(State::Play, &[0xff, 0xff, 0x03]),
            ServerPacket::Other
        );
        assert_eq!(classify(State::Play, &[]), ServerPacket::Other);
        assert_eq!(packet_id(&[0x2d, 1, 2]), Some(45));
        assert_eq!(packet_id(&[]), None);
    }

    #[test]
    fn spawn_constants_match_the_server() {
        let _ = mth::floor(SPAWN.0);
        assert_eq!(SPAWN, (8.5, -60.0, 8.5));
    }
}
