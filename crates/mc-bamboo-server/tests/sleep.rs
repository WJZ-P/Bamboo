//! 睡觉 / 醒来的 `player_position` 端到端覆盖：真实 TCP 客户端关掉自动传送确认，
//! 按收到的 teleport_id 回确认后能正常移动，不会被服务端拉回。
//!
//! End-to-end coverage of the sleep / wake `player_position` packets: a real
//! TCP client with automatic teleport answers disabled accepts each packet by
//! its id and can then move normally instead of being pulled back.

mod common;

use std::{fs, path::PathBuf, time::Duration};

use common::{
    TestServer,
    player::{Answers, offline_uuid},
    wire::{
        PlayerPosition, Pos, SetChunkCacheCenter, accept_teleportation_with_id, chat_command,
        client_tick_end,
    },
};
use mc_bamboo_server::ServerConfig;
use tokio::time::sleep;
use uuid::Uuid;

/// 每刻 50 ms 的客户端节拍；睡够 100 刻再留几刻余量。
const CLIENT_TICK: Duration = Duration::from_millis(50);
const BED_FOOT: Pos = Pos::new(8, -59, 8);
const UP: u8 = 1;

/// 临时目录里放一份 `ops.json`，让玩家可以 `/setblock` / `/time` / `/gamerule`。
struct Lists(PathBuf);

impl Lists {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("bamboo-sleep-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("临时目录应该能建");
        let op = offline_uuid("Kitten");
        fs::write(
            path.join("ops.json"),
            format!(r#"[{{"uuid":"{op}","name":"Kitten","level":4}}]"#),
        )
        .expect("名单文件应该能写");
        Self(path)
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

/// 睡下、跳过夜晚醒来：三次传送（进服、躺下、起身）的 id 递增且都要确认；
/// 确认起身之后移动，服务端按新位置推进区块中心，不再回一个拉回包。
#[tokio::test]
async fn wake_up_teleport_id_is_accepted_and_movement_is_not_pulled_back() {
    let lists = Lists::new();
    let server = TestServer::start(lists.config()).await;
    // 关掉自动确认，全部按实际收到的 id 自己回。
    let answers = Answers {
        teleports: false,
        ..Answers::default()
    };
    let mut player = server.spawn_player("Kitten", 2, answers).await;
    let join = player.expect::<PlayerPosition>(|_| true).await;
    assert_eq!(join.id, 1, "进服的 teleport id 是 1");
    player
        .send(&accept_teleportation_with_id(
            join.id, join.x, join.y, join.z,
        ))
        .await;
    player.position = (join.x, join.y, join.z);
    player.wait_for_chunks((0, 0), 2).await;
    player.settle().await;

    // 一张朝南的完整床，就在出生点旁边；把时间设到午夜，一个人睡就够跳早。
    player
        .send(&chat_command(
            "setblock 8 -59 8 minecraft:red_bed[facing=south,part=foot]",
        ))
        .await;
    player
        .send(&chat_command(
            "setblock 8 -59 9 minecraft:red_bed[facing=south,part=head]",
        ))
        .await;
    player.send(&chat_command("time set 18000")).await;
    player
        .send(&chat_command("gamerule players_sleeping_percentage 0"))
        .await;
    player.settle().await;
    // 命令成功提示和方块更新都读掉，免得干扰后面找传送包。
    player.skip_received();

    // 右键床尾：官方 `startSleeping` 走连接层 teleport，id 必须比进服的大。
    let clicked = player.click(BED_FOOT, UP).await;
    let _ = clicked;
    let bed = player
        .expect::<PlayerPosition>(|packet| packet.id > join.id)
        .await;
    assert_eq!(bed.id, join.id + 1, "躺下是下一次传送");
    // 官方 `AbstractBedBlock.useWithoutItem` 把 pos 换到床头，睡姿传送到床头。
    assert_eq!((bed.x.floor(), bed.z.floor()), (8.0, 9.0), "躺在床头");
    player
        .send(&accept_teleportation_with_id(bed.id, bed.x, bed.y, bed.z))
        .await;
    player.position = (bed.x, bed.y, bed.z);

    // 睡满 100 刻再留余量，跳早那一刻会再发一次 teleport（醒来）。
    for _ in 0..110 {
        player.send(&client_tick_end()).await;
        sleep(CLIENT_TICK).await;
    }
    player.settle().await;
    let wake = player
        .expect::<PlayerPosition>(|packet| packet.id > bed.id)
        .await;
    assert_eq!(wake.id, bed.id + 1, "醒来是再下一次传送");
    assert_ne!(wake.id, 0, "teleport_id 不能写死 0");
    player
        .send(&accept_teleportation_with_id(
            wake.id, wake.x, wake.y, wake.z,
        ))
        .await;
    player.position = (wake.x, wake.y, wake.z);

    // 确认之后正常移动：走进东边下一个区块，服务端按新位置发
    // `set_chunk_cache_center`。没确认传送时移动包会被丢掉、中心不会变，
    // 所以这条断言同时证明按实际 id 回的确认生效了。
    let mark = player.mark();
    // 一次走 16 格会被速度检查拒绝；`move_to` 会拆成每步不超过 10 格。
    player.move_to(24.5, wake.y, wake.z).await;
    let center = player
        .expect::<SetChunkCacheCenter>(|packet| packet.x == 1 && packet.z == 0)
        .await;
    assert_eq!((center.x, center.z), (1, 0));
    // 服务端没有把玩家拉回床上（拉回会再发一个 player_position）。
    let pull_backs = player
        .since::<PlayerPosition>(mark)
        .into_iter()
        .filter(|packet| packet.id > wake.id)
        .count();
    assert_eq!(pull_backs, 0, "确认之后不该被拉回");
}
