//! 相距很远的玩家：多岛缺省打开时他们在两个岛上，聊天、命令、进出服和传送回来照样
//! 对每个人都成立。期望值只看玩家收到什么，不看服务端里有几个岛，所以精确模式下也跑、
//! 也要过。跨岛的东西最多晚一两刻（总设计稿第 13 节第 11 条），这里都是「等到」，不数刻。
//!
//! Players far apart: with multi-island mode on (the default) they sit on two
//! islands, and chat, commands, joining, leaving and teleporting back still
//! work for everyone. The expectations only look at what the players receive,
//! not at how many islands the server has, so exact mode runs and passes them
//! too. Anything crossing islands may be a tick or two late (design section
//! 13, item 11), so every check waits instead of counting ticks.

mod common;

use std::{fs, path::PathBuf};

use common::{
    TestServer,
    player::{Player, offline_uuid},
    wire::{
        AddEntity, BlockUpdate, PlayerChat, PlayerPosition, Pos, ROTATION_RELATIVE, RemoveEntities,
        SystemChat, chat, chat_command, replay_entity,
    },
};
use mc_bamboo_server::ServerConfig;
use uuid::Uuid;

/// 远处的落脚点：离出生点几千格，两边的视野和票的范围隔着几百个区块，一定不在一个岛上。
/// A far spot: thousands of blocks from spawn, so the two views and their
/// tickets are hundreds of chunks apart and never on one island.
const FAR: (f64, f64, f64) = (4000.5, -60.0, -3000.5);
const FAR_CHUNK: (i32, i32) = (250, -188);
const VIEW_DISTANCE: u8 = 2;

/// 临时目录里放一份 `ops.json`，`Admin` 是 4 级管理员。
/// A temporary directory with an `ops.json` making `Admin` a level 4 operator.
struct Lists(PathBuf);

impl Lists {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("bamboo-islands-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("临时目录应该能建");
        let admin = offline_uuid("Admin");
        fs::write(
            path.join("ops.json"),
            format!(r#"[{{"uuid":"{admin}","name":"Admin","level":4}}]"#),
        )
        .expect("名单文件应该能写");
        Self(path)
    }

    fn config(&self) -> ServerConfig {
        ServerConfig {
            view_distance: VIEW_DISTANCE,
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

/// `Admin` 传送到远处，等他收齐那边的区块、`Bob` 看着他的实体消失。
/// `Admin` teleports far away; wait until he has the chunks there and `Bob`
/// has seen his entity go.
async fn send_far(admin: &mut Player, bob: &mut Player) {
    let (x, y, z) = FAR;
    admin
        .send(&chat_command(&format!("tp Admin {x} {y} {z}")))
        .await;
    let landed = admin
        .expect::<PlayerPosition>(|packet| packet.x > 1000.0)
        .await;
    assert_eq!((landed.x, landed.y, landed.z), FAR);
    assert_eq!(landed.relatives, ROTATION_RELATIVE, "不写朝向时朝向不变");
    admin.position = FAR;
    admin
        .wait_for_chunks(FAR_CHUNK, i32::from(VIEW_DISTANCE))
        .await;
    let gone = admin.entity_id;
    bob.expect::<RemoveEntities>(|packet| packet.ids.contains(&gone))
        .await;
}

fn is_chat(content: &'static str) -> impl Fn(&PlayerChat) -> bool {
    move |packet: &PlayerChat| packet.content == content
}

fn system_message_with(key: &'static str) -> impl Fn(&SystemChat) -> bool {
    move |packet: &SystemChat| packet.content.translate_keys().contains(&key)
}

#[tokio::test]
async fn chat_and_commands_reach_a_player_far_away() {
    let lists = Lists::new();
    let server = TestServer::start(lists.config()).await;
    let mut admin = server.enter("Admin").await;
    let mut bob = server.enter("Bob").await;
    send_far(&mut admin, &mut bob).await;

    // 聊天两个方向都到，发的人自己也收到。
    // Chat goes both ways, and the sender gets his own line too.
    bob.send(&chat("from spawn")).await;
    admin.expect::<PlayerChat>(is_chat("from spawn")).await;
    bob.expect::<PlayerChat>(is_chat("from spawn")).await;
    admin.send(&chat("from afar")).await;
    bob.expect::<PlayerChat>(is_chat("from afar")).await;
    admin.expect::<PlayerChat>(is_chat("from afar")).await;

    // 远处的人改出生点旁边的方块：方块命令在拼起来的世界上执行，旁边的人看得见。
    // The far player edits a block next to spawn: block commands run on the
    // joined world, and the player there sees it.
    let target = Pos::new(1, -60, 1);
    admin
        .send(&chat_command("setblock 1 -60 1 minecraft:stone"))
        .await;
    admin
        .expect::<SystemChat>(system_message_with("commands.setblock.success"))
        .await;
    let update = bob
        .expect::<BlockUpdate>(|packet| packet.pos == target)
        .await;
    assert_ne!(update.state, 0, "那一格不再是空气");

    // 对别的岛上的玩家用的命令：反馈回到发令的人，效果落到对方身上。
    // A command aimed at a player on another island: the feedback returns to
    // the sender and the effect lands on the target.
    admin.send(&chat_command("tp Bob 8.5 -60 20.5")).await;
    admin
        .expect::<SystemChat>(system_message_with(
            "commands.teleport.success.location.single",
        ))
        .await;
    let moved = bob.expect::<PlayerPosition>(|_| true).await;
    assert_eq!((moved.x, moved.y, moved.z), (8.5, -60.0, 20.5));
    bob.position = (moved.x, moved.y, moved.z);

    // 传送回对方身边：两边又互相看得见。
    // Teleport back next to the other: both see each other again.
    admin.send(&chat_command("tp Admin Bob")).await;
    let back = admin
        .expect::<PlayerPosition>(|packet| packet.x < 1000.0)
        .await;
    assert_eq!((back.x, back.y, back.z), (8.5, -60.0, 20.5));
    admin.position = (back.x, back.y, back.z);
    // 配对包按上次同步的位置发（官方 `ServerEntity.getPositionBase`），之后的同步把它挪到
    // 新位置：等客户端算出来的位置到了就行。
    // The pairing packet carries the last synced position (official
    // `ServerEntity.getPositionBase`) and the following sync moves it: wait
    // until the client's replayed position gets there.
    let admin_id = admin.entity_id;
    bob.expect::<AddEntity>(|packet| packet.id == admin_id)
        .await;
    bob.wait_until("Admin to be seen next to Bob", |frames| {
        replay_entity(frames, admin_id).is_some_and(|seen| seen.is_at(8.5, -60.0, 20.5))
    })
    .await;
    let bob_id = bob.entity_id;
    admin
        .expect::<AddEntity>(|packet| packet.id == bob_id)
        .await;
}

#[tokio::test]
async fn a_far_player_leaving_and_joining_is_announced_to_everyone() {
    let lists = Lists::new();
    let server = TestServer::start(lists.config()).await;
    let mut admin = server.enter("Admin").await;
    let mut bob = server.enter("Bob").await;
    send_far(&mut admin, &mut bob).await;

    drop(admin);
    let left = bob
        .expect::<SystemChat>(system_message_with("multiplayer.player.left"))
        .await;
    assert!(format!("{:?}", left.content).contains("Admin"));
    server.wait_for_players(&["Bob"]).await;

    // 重新进服的人回到出生点（测试服务端不存玩家数据），两边又是一个视野。
    // The returning player is back at spawn (the test server keeps no player
    // data), so both share a view again.
    let admin = server.enter("Admin").await;
    bob.expect::<SystemChat>(system_message_with("multiplayer.player.joined"))
        .await;
    let admin_id = admin.entity_id;
    bob.expect::<AddEntity>(|packet| packet.id == admin_id)
        .await;
    admin.send(&chat("back")).await;
    bob.expect::<PlayerChat>(is_chat("back")).await;
}
