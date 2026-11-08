//! 代理转发：现代（Velocity）和旧式（BungeeCord）两种登录，以及关闭时的原有行为。

mod common;

use common::{
    Client, LOGIN_INTENT, TestServer, frame, handshake, hello, json_packet, official_known_packs,
    packet_id,
    player::{Answers, Player, offline_uuid},
    wire::{PlayerInfoUpdate, TabProperty},
};
use hmac::{Hmac, Mac};
use mc_bamboo_data::packets::{configuration, login, play};
use mc_bamboo_protocol::{
    PROTOCOL_VERSION,
    codec::{Reader, write_bool, write_string, write_uuid, write_varint},
    packet::{
        login::MAX_PLAYER_NAME_CHARS,
        profile::{
            MAX_PROFILE_PROPERTIES, MAX_PROPERTY_NAME_CHARS, MAX_PROPERTY_SIGNATURE_CHARS,
            MAX_PROPERTY_VALUE_CHARS, ProfileProperty,
        },
    },
};
use mc_bamboo_server::{ForwardingMode, ServerConfig};
use sha2::Sha256;
use uuid::Uuid;

const SECRET: &str = "bamboo-forwarding-secret";
/// 代理给出的 UUID 和客户端自己报的名字都不同，能看出服务端用了哪一份。
const PROXY_UUID: Uuid = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
/// 旁观者也要走代理登录：打开转发后，服务端会问每个连接要玩家信息。
const WATCHER_UUID: Uuid = Uuid::from_u128(0xfedc_ba98_7654_3210_fedc_ba98_7654_3210);
const PROXY_NAME: &str = "ProxiedKitten";
const SKIN_VALUE: &str = "skin-data";
const SKIN_SIGNATURE: &str = "skin-signature";

fn forwarding(mode: ForwardingMode, secret: &str) -> ServerConfig {
    ServerConfig {
        view_distance: 2,
        forwarding: mode,
        forwarding_secret: secret.to_owned(),
        ..ServerConfig::default()
    }
}

fn textures(value: &str) -> ProfileProperty {
    ProfileProperty {
        name: "textures".to_owned(),
        value: value.to_owned(),
        signature: Some(SKIN_SIGNATURE.to_owned()),
    }
}

fn tab_textures(value: &str) -> TabProperty {
    TabProperty {
        name: "textures".to_owned(),
        value: value.to_owned(),
        signature: Some(SKIN_SIGNATURE.to_owned()),
    }
}

/// 按 Velocity 的格式拼出回应内容：版本、地址、UUID、名字、属性。
fn modern_data(version: i32, uuid: Uuid, name: &str, properties: &[ProfileProperty]) -> Vec<u8> {
    let mut data = Vec::new();
    write_varint(&mut data, version);
    write_string(&mut data, "203.0.113.7");
    write_uuid(&mut data, uuid);
    write_string(&mut data, name);
    write_varint(&mut data, properties.len() as i32);
    for property in properties {
        write_string(&mut data, &property.name);
        write_string(&mut data, &property.value);
        write_bool(&mut data, property.signature.is_some());
        if let Some(signature) = &property.signature {
            write_string(&mut data, signature);
        }
    }
    data
}

/// 签名在前、内容在后，和代理发来的一样。
fn signed(secret: &str, data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(data);
    let mut payload = mac.finalize().into_bytes().to_vec();
    payload.extend_from_slice(data);
    payload
}

fn plugin_answer(message_id: i32, data: Option<&[u8]>) -> Vec<u8> {
    let mut payload = vec![login::serverbound::CUSTOM_QUERY_ANSWER as u8];
    write_varint(&mut payload, message_id);
    match data {
        Some(data) => {
            payload.push(1);
            payload.extend_from_slice(data);
        }
        None => payload.push(0),
    }
    frame(&payload)
}

fn handshake_with_address(address: &str) -> Vec<u8> {
    let mut payload = vec![0x00];
    write_varint(&mut payload, PROTOCOL_VERSION);
    write_string(&mut payload, address);
    payload.extend_from_slice(&25565u16.to_be_bytes());
    write_varint(&mut payload, LOGIN_INTENT);
    frame(&payload)
}

/// 发出登录请求，读出服务端的插件询问，检查频道和版本。
async fn ask_for_velocity(client: &mut Client, name: &str) -> i32 {
    client.send(&handshake(LOGIN_INTENT)).await;
    client.send(&hello(name)).await;

    let payload = client.read_frame().await;
    assert_eq!(packet_id(&payload), login::clientbound::CUSTOM_QUERY);
    let mut reader = Reader::new(&payload);
    reader.varint().unwrap();
    let message_id = reader.varint().unwrap();
    assert_eq!(reader.string(32767), Ok("velocity:player_info"));
    assert_eq!(reader.rest(), [4], "询问里带着支持的最高版本");
    message_id
}

struct Finished {
    uuid: Uuid,
    name: String,
    properties: Vec<ProfileProperty>,
}

fn read_login_finished(payload: &[u8]) -> Finished {
    let mut reader = Reader::new(payload);
    assert_eq!(reader.varint(), Ok(login::clientbound::LOGIN_FINISHED));
    let uuid = reader.uuid().unwrap();
    let name = reader.string(MAX_PLAYER_NAME_CHARS).unwrap().to_owned();
    let count = reader.count(MAX_PROFILE_PROPERTIES).unwrap();
    let properties = (0..count)
        .map(|_| {
            let name = reader.string(MAX_PROPERTY_NAME_CHARS).unwrap().to_owned();
            let value = reader.string(MAX_PROPERTY_VALUE_CHARS).unwrap().to_owned();
            let signature = if reader.bool().unwrap() {
                Some(
                    reader
                        .string(MAX_PROPERTY_SIGNATURE_CHARS)
                        .unwrap()
                        .to_owned(),
                )
            } else {
                None
            };
            ProfileProperty {
                name,
                value,
                signature,
            }
        })
        .collect();
    // 会话 ID 用不到，但格式检查要求读干净。
    reader.uuid().unwrap();
    reader.finish().unwrap();
    Finished {
        uuid,
        name,
        properties,
    }
}

/// 收到登录成功之后的 Configuration 流程，结束时服务端开始发 Play 包。
async fn complete_configuration(client: &mut Client) {
    client
        .send(&frame(&[login::serverbound::LOGIN_ACKNOWLEDGED as u8]))
        .await;
    client
        .read_until(configuration::clientbound::SELECT_KNOWN_PACKS)
        .await;
    client.send(&official_known_packs()).await;
    client
        .read_until(configuration::clientbound::FINISH_CONFIGURATION)
        .await;
    client.finish_configuration().await;
}

/// 走完 Configuration，返回第一个 Play 包的 ID。
async fn enter_play(client: &mut Client) -> i32 {
    complete_configuration(client).await;
    packet_id(&client.read_frame().await)
}

#[tokio::test]
async fn modern_forwarding_takes_the_uuid_name_and_skin_from_the_proxy() {
    let server = TestServer::start(forwarding(ForwardingMode::Modern, SECRET)).await;
    let mut client = server.connect().await;
    // 客户端自己报的名字和代理给的不一样，最后要采用代理的。
    let message_id = ask_for_velocity(&mut client, "LocalName").await;
    let data = modern_data(4, PROXY_UUID, PROXY_NAME, &[textures(SKIN_VALUE)]);
    client
        .send(&plugin_answer(message_id, Some(&signed(SECRET, &data))))
        .await;

    let finished = read_login_finished(&client.read_frame().await);
    assert_eq!(finished.uuid, PROXY_UUID);
    assert_eq!(finished.name, PROXY_NAME);
    assert_eq!(finished.properties, [textures(SKIN_VALUE)]);

    assert_eq!(enter_play(&mut client).await, play::clientbound::LOGIN);
    let players = server.handle.online_players();
    assert_eq!(players.len(), 1);
    assert_eq!(players[0].name, PROXY_NAME);
    assert_eq!(players[0].uuid, PROXY_UUID.to_string());
}

#[tokio::test]
async fn other_players_see_the_forwarded_skin() {
    let server = TestServer::start(forwarding(ForwardingMode::Modern, SECRET)).await;
    let mut watcher_client = server.connect().await;
    let message_id = ask_for_velocity(&mut watcher_client, "LocalName").await;
    let data = modern_data(4, WATCHER_UUID, "Watcher", &[]);
    watcher_client
        .send(&plugin_answer(message_id, Some(&signed(SECRET, &data))))
        .await;
    read_login_finished(&watcher_client.read_frame().await);
    // 不先读 `login` 包：`Player::attach` 要自己从缓冲区里看到它。
    complete_configuration(&mut watcher_client).await;
    let mut watcher = Player::attach(watcher_client, "Watcher", Answers::default()).await;
    watcher.wait_for_chunks((0, 0), 2).await;

    let mut client = server.connect().await;
    let message_id = ask_for_velocity(&mut client, "LocalName").await;
    let data = modern_data(4, PROXY_UUID, PROXY_NAME, &[textures(SKIN_VALUE)]);
    client
        .send(&plugin_answer(message_id, Some(&signed(SECRET, &data))))
        .await;
    read_login_finished(&client.read_frame().await);
    complete_configuration(&mut client).await;
    // 接管连接，让区块有人读，免得服务端写卡住。
    let _player = Player::attach(client, PROXY_NAME, Answers::default()).await;

    let update = watcher
        .expect::<PlayerInfoUpdate>(|packet| packet.adds(PROXY_UUID))
        .await;
    let entry = update.entry(PROXY_UUID).unwrap();
    assert_eq!(entry.name.as_deref(), Some(PROXY_NAME));
    assert_eq!(entry.properties, [tab_textures(SKIN_VALUE)]);
}

#[tokio::test]
async fn modern_forwarding_refuses_a_bad_signature() {
    let server = TestServer::start(forwarding(ForwardingMode::Modern, SECRET)).await;
    let mut client = server.connect().await;
    let message_id = ask_for_velocity(&mut client, "Kitten").await;
    let data = modern_data(4, PROXY_UUID, PROXY_NAME, &[]);
    client
        .send(&plugin_answer(
            message_id,
            Some(&signed("wrong-secret", &data)),
        ))
        .await;

    let (id, reason) = json_packet(&client.read_frame().await);
    assert_eq!(id, login::clientbound::LOGIN_DISCONNECT);
    assert_eq!(reason["text"], "Unable to verify player details");
    client.assert_closed().await;
    assert!(server.handle.online_players().is_empty());
}

#[tokio::test]
async fn modern_forwarding_refuses_a_client_that_does_not_answer() {
    let server = TestServer::start(forwarding(ForwardingMode::Modern, SECRET)).await;
    let mut client = server.connect().await;
    let message_id = ask_for_velocity(&mut client, "Kitten").await;
    client.send(&plugin_answer(message_id, None)).await;

    let (id, reason) = json_packet(&client.read_frame().await);
    assert_eq!(id, login::clientbound::LOGIN_DISCONNECT);
    assert_eq!(
        reason["text"],
        "This server requires you to connect with Velocity."
    );
    client.assert_closed().await;
}

#[tokio::test]
async fn modern_forwarding_drops_a_newer_version() {
    let server = TestServer::start(forwarding(ForwardingMode::Modern, SECRET)).await;
    let mut client = server.connect().await;
    let message_id = ask_for_velocity(&mut client, "Kitten").await;
    // 签名是对的，只是服务端还不认识版本 5。
    let data = modern_data(5, PROXY_UUID, PROXY_NAME, &[]);
    client
        .send(&plugin_answer(message_id, Some(&signed(SECRET, &data))))
        .await;

    client.assert_closed().await;
    assert!(server.handle.online_players().is_empty());
}

#[tokio::test]
async fn legacy_forwarding_takes_the_uuid_and_skin_from_the_handshake() {
    let server = TestServer::start(forwarding(ForwardingMode::Legacy, "")).await;
    let mut client = server.connect().await;
    let json =
        format!(r#"[{{"name":"textures","value":"{SKIN_VALUE}","signature":"{SKIN_SIGNATURE}"}}]"#);
    let address = format!("localhost\x00203.0.113.7\0{PROXY_UUID}\0{json}");
    client.send(&handshake_with_address(&address)).await;
    client.send(&hello("LocalName")).await;

    let finished = read_login_finished(&client.read_frame().await);
    assert_eq!(finished.uuid, PROXY_UUID);
    // 旧式转发不转发名字，名字仍然来自客户端的登录包。
    assert_eq!(finished.name, "LocalName");
    assert_eq!(finished.properties, [textures(SKIN_VALUE)]);

    assert_eq!(enter_play(&mut client).await, play::clientbound::LOGIN);
    assert_eq!(
        server.handle.online_players()[0].uuid,
        PROXY_UUID.to_string()
    );
}

#[tokio::test]
async fn legacy_forwarding_refuses_a_handshake_without_the_forwarded_fields() {
    let server = TestServer::start(forwarding(ForwardingMode::Legacy, "")).await;
    let mut client = server.connect().await;
    client.send(&handshake(LOGIN_INTENT)).await;
    client.send(&hello("Kitten")).await;

    let (id, reason) = json_packet(&client.read_frame().await);
    assert_eq!(id, login::clientbound::LOGIN_DISCONNECT);
    assert_eq!(
        reason["text"],
        "If you wish to use IP forwarding, please enable it in your BungeeCord config as well!"
    );
    client.assert_closed().await;
    assert!(server.handle.online_players().is_empty());
}

/// 原版握手地址最多 255 个字符，Spigot 为转发提高到了 32767。
#[tokio::test]
async fn legacy_forwarding_accepts_longer_addresses_than_a_normal_handshake() {
    let server = TestServer::start(forwarding(ForwardingMode::Legacy, "")).await;
    let value = "x".repeat(1200);
    let json = format!(r#"[{{"name":"textures","value":"{value}"}}]"#);
    let address = format!("localhost\x00203.0.113.7\0{PROXY_UUID}\0{json}");
    assert!(address.len() > 255);

    let mut client = server.connect().await;
    client.send(&handshake_with_address(&address)).await;
    client.send(&hello("Kitten")).await;

    let finished = read_login_finished(&client.read_frame().await);
    assert_eq!(finished.uuid, PROXY_UUID);
    assert_eq!(finished.properties[0].value, value);
}

#[tokio::test]
async fn forwarding_off_ignores_the_forwarded_address() {
    let server = TestServer::start(ServerConfig {
        view_distance: 2,
        ..ServerConfig::default()
    })
    .await;
    let mut client = server.connect().await;
    // 关闭转发时，地址里的 NUL 只是普通文本。
    let address = format!("localhost\x00203.0.113.7\0{PROXY_UUID}\0[]");
    client.send(&handshake_with_address(&address)).await;
    client.send(&hello("Kitten")).await;

    let finished = read_login_finished(&client.read_frame().await);
    assert_eq!(finished.uuid, offline_uuid("Kitten"));
    assert_eq!(finished.name, "Kitten");
    assert!(finished.properties.is_empty());

    assert_eq!(enter_play(&mut client).await, play::clientbound::LOGIN);
}
