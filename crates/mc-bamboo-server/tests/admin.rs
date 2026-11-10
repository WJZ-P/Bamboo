//! 管理命令端到端：登录被封禁/白名单拒绝，以及 `/stop` 走现有关服流程。
//!
//! 名单文件放在临时目录，测试结束删掉，不在仓库里留文件。

mod common;

use std::{fs, path::PathBuf};

use common::{
    LOGIN_INTENT, TEST_DEADLINE, TestServer, handshake, hello, packet_id,
    player::{Player, offline_uuid},
    wire::{Disconnect, SystemChat, chat_command},
};
use mc_bamboo_data::packets::login;
use mc_bamboo_protocol::codec::Reader;
use mc_bamboo_server::ServerConfig;
use tokio::time::{sleep, timeout};
use uuid::Uuid;

/// 每个测试一份独立的临时目录，里面放好名单文件。
struct Lists(PathBuf);

impl Lists {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("bamboo-admin-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("临时目录应该能建");
        Self(path)
    }

    fn write(&self, name: &str, text: &str) {
        fs::write(self.0.join(name), text).expect("名单文件应该能写");
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.0.join(name)).unwrap_or_default()
    }

    fn config(&self) -> ServerConfig {
        ServerConfig {
            view_distance: 2,
            list_directory: self.0.clone(),
            ..ServerConfig::default()
        }
    }
}

impl Drop for Lists {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// 官方 `banned-players.json` 的一项。
fn player_ban(uuid: Uuid, name: &str) -> String {
    format!(
        r#"[{{"uuid":"{uuid}","name":"{name}","created":"2026-09-15 11:20:48 +0000","source":"Server","expires":"forever","reason":"testing"}}]"#
    )
}

fn whitelist(uuid: Uuid, name: &str) -> String {
    format!(r#"[{{"uuid":"{uuid}","name":"{name}"}}]"#)
}

/// Login 阶段的断开包原因还是 JSON 文本，不是 NBT。
fn login_disconnect_reason(payload: &[u8]) -> String {
    let mut reader = Reader::new(payload);
    assert_eq!(
        reader.varint().unwrap(),
        login::clientbound::LOGIN_DISCONNECT
    );
    let len = reader.varint().unwrap() as usize;
    let bytes = reader.rest();
    assert_eq!(bytes.len(), len);
    String::from_utf8(bytes.to_vec()).expect("登录断开原因应该是 JSON")
}

/// 走到 Login 阶段并读一条断开原因。
async fn login_refusal(server: &TestServer, name: &str) -> String {
    let mut client = server.connect().await;
    client.send(&handshake(common::LOGIN_INTENT)).await;
    client.send(&hello(name)).await;
    let payload = client.read_frame().await;
    assert_eq!(packet_id(&payload), login::clientbound::LOGIN_DISCONNECT);
    login_disconnect_reason(&payload)
}

#[tokio::test]
async fn a_banned_player_is_refused_at_login() {
    let lists = Lists::new();
    let uuid = offline_uuid("Kitten");
    lists.write("banned-players.json", &player_ban(uuid, "Kitten"));
    let server = TestServer::start(lists.config()).await;

    let reason = login_refusal(&server, "Kitten").await;
    assert!(
        reason.contains("multiplayer.disconnect.banned.reason"),
        "{reason}"
    );
    assert!(reason.contains("testing"), "{reason}");
    assert!(!reason.contains("multiplayer.disconnect.banned.reason.default"));
}

#[tokio::test]
async fn a_banned_ip_is_refused_at_login() {
    let lists = Lists::new();
    lists.write(
        "banned-ips.json",
        r#"[{"ip":"127.0.0.1","created":"2026-09-15 11:20:48 +0000","source":"Server","expires":"forever","reason":"testing"}]"#,
    );
    let server = TestServer::start(lists.config()).await;

    let reason = login_refusal(&server, "Kitten").await;
    assert!(
        reason.contains("multiplayer.disconnect.banned_ip.reason"),
        "{reason}"
    );
}

#[tokio::test]
async fn ban_order_precedes_the_ip_ban() {
    let lists = Lists::new();
    let uuid = offline_uuid("Kitten");
    lists.write("banned-players.json", &player_ban(uuid, "Kitten"));
    lists.write(
        "banned-ips.json",
        r#"[{"ip":"127.0.0.1","created":"2026-09-15 11:20:48 +0000","source":"Server","expires":"forever","reason":"testing"}]"#,
    );
    let server = TestServer::start(lists.config()).await;

    let reason = login_refusal(&server, "Kitten").await;
    assert!(
        reason.contains("multiplayer.disconnect.banned.reason"),
        "{reason}"
    );
}

#[tokio::test]
async fn whitelist_on_refuses_a_player_not_on_the_list() {
    let lists = Lists::new();
    // 白名单默认关闭，`on` 由 `/whitelist on` 打开；这里用一名白名单玩家
    // 触发白名单开启后，再让没在名单里的玩家登录。
    let admin = offline_uuid("Admin");
    lists.write(
        "ops.json",
        &format!(r#"[{{"uuid":"{admin}","name":"Admin","level":4}}]"#),
    );
    lists.write("whitelist.json", &whitelist(admin, "Admin"));
    let server = TestServer::start(lists.config()).await;

    let mut admin_player: Player = server.enter("Admin").await;
    admin_player.send(&chat_command("whitelist on")).await;
    admin_player
        .expect::<SystemChat>(|packet| {
            packet
                .content
                .translate_keys()
                .contains(&"commands.whitelist.enabled")
        })
        .await;

    let reason = login_refusal(&server, "Stranger").await;
    assert!(
        reason.contains("multiplayer.disconnect.not_whitelisted"),
        "{reason}"
    );
}

#[tokio::test]
async fn op_writes_the_file_and_the_new_level_reaches_a_later_join() {
    let lists = Lists::new();
    let admin = offline_uuid("Admin");
    lists.write(
        "ops.json",
        &format!(r#"[{{"uuid":"{admin}","name":"Admin","level":4}}]"#),
    );
    let server = TestServer::start(lists.config()).await;

    let mut player: Player = server.enter("Admin").await;
    player.send(&chat_command("op Mochi")).await;
    player
        .expect::<SystemChat>(|packet| {
            packet
                .content
                .translate_keys()
                .contains(&"commands.op.success")
        })
        .await;
    // 写盘是连接层那个线程做的，轮询到文件里出现名字为止。
    timeout(TEST_DEADLINE, async {
        loop {
            if lists.read("ops.json").contains("Mochi") {
                break;
            }
            sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("/op 应该写回 ops.json");
    let text = lists.read("ops.json");
    assert!(text.contains("\"level\": 4"), "{text}");
    assert!(text.contains(&offline_uuid("Mochi").to_string()), "{text}");
}

/// `/op`、`/deop` 改的是世界线程的权威名单，发布出去的新快照马上被
/// 连接层的满员判断读到：被 /deop 的玩家不再能越过人数上限。
#[tokio::test]
async fn deop_takes_away_the_full_server_bypass() {
    let lists = Lists::new();
    let admin = offline_uuid("Admin");
    let latecomer = offline_uuid("Latecomer");
    lists.write(
        "ops.json",
        &format!(
            r#"[{{"uuid":"{admin}","name":"Admin","level":4}},{{"uuid":"{latecomer}","name":"Latecomer","level":4,"bypassesPlayerLimit":true}}]"#
        ),
    );
    let config = ServerConfig {
        max_players: 1,
        ..lists.config()
    };
    let server = TestServer::start(config).await;
    let mut player: Player = server.enter("Admin").await;

    // 一开始这名 OP 能越过人数上限。
    let mut bypassing = server.connect().await;
    bypassing.send(&handshake(LOGIN_INTENT)).await;
    bypassing.send(&hello("Latecomer")).await;
    assert!(login::clientbound::LOGIN_FINISHED == packet_id(&bypassing.read_frame().await));
    drop(bypassing);

    player.send(&chat_command("deop Latecomer")).await;
    player
        .expect::<SystemChat>(|packet| {
            packet
                .content
                .translate_keys()
                .contains(&"commands.deop.success")
        })
        .await;
    timeout(TEST_DEADLINE, async {
        loop {
            if lists
                .read("ops.json")
                .contains("\"bypassesPlayerLimit\": false")
                || !lists.read("ops.json").contains("Latecomer")
            {
                break;
            }
            sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("/deop 应该写回 ops.json");

    let mut refused = server.connect().await;
    refused.send(&handshake(LOGIN_INTENT)).await;
    refused.send(&hello("Latecomer")).await;
    let payload = refused.read_frame().await;
    let reason = login_disconnect_reason(&payload);
    assert!(
        reason.contains("multiplayer.disconnect.server_full"),
        "{reason}"
    );
}

#[tokio::test]
async fn stop_uses_the_existing_shutdown_flow() {
    let lists = Lists::new();
    let admin = offline_uuid("Admin");
    lists.write(
        "ops.json",
        &format!(r#"[{{"uuid":"{admin}","name":"Admin","level":4}}]"#),
    );
    let mut server = TestServer::start(lists.config()).await;

    let mut player: Player = server.enter("Admin").await;
    player.send(&chat_command("stop")).await;
    player
        .expect::<SystemChat>(|packet| {
            packet
                .content
                .translate_keys()
                .contains(&"commands.stop.stopping")
        })
        .await;
    let bye = player.expect::<Disconnect>(|_| true).await;
    assert!(
        format!("{:?}", bye.reason).contains("multiplayer.disconnect.server_shutdown"),
        "关服理由要按官方的翻译键"
    );
    // 世界线程结束后 `Server::run` 自己退出，`shutdown()` 等它结束。
    timeout(TEST_DEADLINE, server.shutdown())
        .await
        .expect("/stop 之后服务端应该已经结束");
}
