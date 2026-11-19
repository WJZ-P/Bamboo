//! 缺省的出生规则（`respawn_radius = 10`）在真实 TCP 上端到端的覆盖。
//!
//! 别的集成测试把半径定成 0，断言固定出生坐标；这里不覆盖规则，按官方缺省做：
//! 新玩家落在世界出生点 ±10 格内、坐标是整数加 0.5、脚下是草方块，照实际位置
//! 回传送确认后可以移动；死亡重生后同样落在范围内。`immediate_respawn` 打开时
//! 不经死亡界面，也走一遍同一条重生路径。
//!
//! End-to-end coverage of the default spawn rules (`respawn_radius = 10`)
//! over real TCP. Other integration tests pin the radius to 0 for fixed
//! coordinates; here the rule is left at its official default: a new player
//! lands within ±10 blocks of the world spawn on block-centred coordinates
//! with grass underfoot, can move after accepting the teleport at those real
//! coordinates, and lands within the same bounds again after a death respawn.
//! With `immediate_respawn` on the client skips the death screen and sends
//! `PERFORM_RESPAWN` itself; the server still waits for that packet.

mod common;

use std::{fs, path::PathBuf};

use common::{
    TestServer, accept_teleportation,
    player::{Answers, Player, offline_uuid},
    wire::{PlayerPosition, Pos, SetChunkCacheCenter, chat_command, client_command},
};
use mc_bamboo_data::{blocks, packets::play};
use mc_bamboo_server::ServerConfig;
use uuid::Uuid;

/// 官方 `LevelData` 的缺省出生点方块中心：x / z 都是 8.5（方块坐标 8）。
/// 候选列的整数范围是 -2..=18，落点是那一列加 0.5，所以正好是 8.5 ± 10。
/// The official `LevelData` default spawn block centre: x and z are both
/// 8.5 (block coordinate 8). Candidate columns are -2..=18 and landings add
/// 0.5, so the bounds are exactly 8.5 ± 10.
const SPAWN_XZ: f64 = 8.5;
/// 官方 `respawn_radius` 的缺省值。
/// The official default `respawn_radius`.
const RADIUS: f64 = 10.0;
/// 超平坦地面的 y；脚下那格是 y - 1。
/// The superflat ground's y; the block underfoot is at y - 1.
const GROUND_Y: f64 = -60.0;
const VIEW_DISTANCE: u8 = 2;
/// `client_command` 的 ordinal：`ClientCommandKind::PerformRespawn`。
/// Ordinal of `ClientCommandKind::PerformRespawn` in `client_command`.
const PERFORM_RESPAWN: i32 = 0;

/// 临时目录里放一份 `ops.json`，让 `Kitten` 可以用 `/kill @s` 和 `/gamerule`。
/// A temporary `ops.json` letting `Kitten` use `/kill @s` and `/gamerule`.
struct Lists(PathBuf);

impl Lists {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("bamboo-spawn-radius-{}", Uuid::new_v4()));
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

/// 官方 `PlayerSpawnFinder` 的散布：x / z 在出生点 ±10 内，且是整数加 0.5。
/// 同时核对脚下是草方块（超平坦）和落点高度。
///
/// Vanilla `PlayerSpawnFinder` scatter: x / z within ±10 of the spawn and
/// block-centred. Also check grass underfoot (superflat) and the landing y.
fn assert_spawn_landing(player: &Player, landing: &PlayerPosition, what: &str) {
    assert_eq!(
        (landing.y, landing.relatives),
        (GROUND_Y, 0),
        "{what}：落点高度和绝对坐标"
    );
    for (axis, value) in [("x", landing.x), ("z", landing.z)] {
        assert!(
            (value - SPAWN_XZ).abs() <= RADIUS,
            "{what}：{axis}={value} 不在出生点 ±{RADIUS} 内"
        );
        assert_eq!(
            value - value.floor(),
            0.5,
            "{what}：{axis}={value} 不是整数加 0.5"
        );
    }
    let under = Pos::new(
        landing.x.floor() as i32,
        (GROUND_Y - 1.0) as i32,
        landing.z.floor() as i32,
    );
    assert_eq!(
        player.block_at(under),
        Some(blocks::GRASS_BLOCK),
        "{what}：脚下是草方块"
    );
}

/// `immediate_respawn` 关着时客户端从死亡界面发 PERFORM_RESPAWN，开着时收到
/// `player_combat_kill` 后直接发同一个包；服务端两种情况下都等这个包。
/// 重生包里的落点都按缺省半径散布。
///
/// With `immediate_respawn` off the client sends PERFORM_RESPAWN from the
/// death screen; with it on, right after `player_combat_kill`. The server
/// waits for the packet in both cases. Both landings use the default-radius
/// scatter.
async fn join_move_and_respawn(immediate: bool) {
    let lists = Lists::new();
    let server = TestServer::start_with_default_spawn_radius(lists.config()).await;
    // 关掉假客户端的自动传送确认，按实际落点自己回一次（和真实客户端一样）。
    // Turn off the fake client's automatic teleport answer and send one
    // ourselves at the real landing coordinates, like a real client.
    let answers = Answers {
        teleports: false,
        ..Default::default()
    };
    let mut player: Player = server.spawn_player("Kitten", 2, answers).await;
    let join = player.expect::<PlayerPosition>(|_| true).await;
    assert_eq!(join.id, 1, "第一次传送的 id 是 1");
    player
        .send(&accept_teleportation(join.x, join.y, join.z))
        .await;
    let center = (
        (join.x.floor() as i32).div_euclid(16),
        (join.z.floor() as i32).div_euclid(16),
    );
    player.wait_for_chunks(center, 2).await;
    assert_spawn_landing(&player, &join, "进服");

    // 确认之后正常移动：走到东边下一个区块，服务端按新位置发
    // `set_chunk_cache_center`。没确认传送时移动包会被丢掉、中心不会变，
    // 所以这条断言同时证明按实际落点回的确认生效了。
    // After the acceptance, move into the next chunk east: the server sends
    // `set_chunk_cache_center` for the new position. An unaccepted teleport
    // drops movement packets and the centre would not move, so this also
    // proves the acceptance at the real landing coordinates took effect.
    player.position = (join.x, join.y, join.z);
    let next_x = f64::from((center.0 + 1) * 16) + 0.5;
    let mark = player.mark();
    player.move_to(next_x, join.y, join.z).await;
    let moved_center = player
        .expect::<SetChunkCacheCenter>(|packet| packet.x == center.0 + 1 && packet.z == center.1)
        .await;
    assert_eq!((moved_center.x, moved_center.z), (center.0 + 1, center.1));
    assert!(
        !player
            .ids_since(mark)
            .contains(&play::clientbound::DISCONNECT),
        "按实际落点确认后可以移动，没有被判为非法移动"
    );

    if immediate {
        player
            .send(&chat_command("gamerule immediate_respawn true"))
            .await;
        player.settle().await;
    }
    // `/kill @s` 走完整的死亡流程（genericKill）；客户端收到死亡界面包后
    // 由自己发 PERFORM_RESPAWN（规则开着只是不显示那个界面）。
    // `/kill @s` runs the full death flow (genericKill); the client sends
    // PERFORM_RESPAWN after the death-screen packet (with the rule on it
    // just skips showing that screen).
    let before_death = player.mark();
    player.send(&chat_command("kill @s")).await;
    player
        .wait_until("the death screen packet", |frames| {
            frames[before_death..]
                .iter()
                .any(|frame| frame.id == play::clientbound::PLAYER_COMBAT_KILL)
        })
        .await;
    // 服务端不主动重生：客户端不发就收不到重生包和新的 player_position。
    // The server never respawns on its own: without a client packet there
    // is no respawn packet and no new player_position.
    player.settle().await;
    assert!(
        !player
            .ids_since(before_death)
            .contains(&play::clientbound::RESPAWN),
        "客户端还没发 PERFORM_RESPAWN，服务端没有自己重生"
    );
    assert!(
        player.since::<PlayerPosition>(before_death).is_empty(),
        "客户端还没发 PERFORM_RESPAWN，没有新的落点"
    );
    player.send(&client_command(PERFORM_RESPAWN)).await;
    // 重生包里的落点：`PlayerList.respawn` 已经发过 `player_position`。
    // The landing in the respawn sequence: `PlayerList.respawn` already
    // sent `player_position`.
    let respawn = player.expect::<PlayerPosition>(|_| true).await;
    assert_spawn_landing(&player, &respawn, "死亡重生");
    player
        .send(&accept_teleportation(respawn.x, respawn.y, respawn.z))
        .await;
    player.settle().await;
    assert!(!player.is_closed(), "重生后按实际落点确认，连接仍然开着");
    // 已经重生后再发一次：官方 `handleClientCommand` 在血量 > 0 时忽略，
    // 不会收到第二个重生包 / 第二个落点。
    // One more PERFORM_RESPAWN after respawning: vanilla
    // `handleClientCommand` ignores it while health > 0, so there is no
    // second respawn packet or landing.
    let after_respawn = player.mark();
    player.send(&client_command(PERFORM_RESPAWN)).await;
    player.settle().await;
    assert!(
        !player
            .ids_since(after_respawn)
            .contains(&play::clientbound::RESPAWN),
        "血量大于 0 时 PERFORM_RESPAWN 被忽略"
    );
    assert!(
        player.since::<PlayerPosition>(after_respawn).is_empty(),
        "被忽略的 PERFORM_RESPAWN 没有产生新的落点"
    );
}

#[tokio::test]
async fn the_default_spawn_radius_scatters_join_and_respawn_landings() {
    join_move_and_respawn(false).await;
}

/// `immediate_respawn` 打开时客户端不经死亡界面，收到 `player_combat_kill`
/// 后自己发 `PERFORM_RESPAWN`，服务端照常走同一个 `respawn()`。
/// With `immediate_respawn` on, the client skips the death screen and sends
/// `PERFORM_RESPAWN` itself; the server takes the same `respawn()` as usual.
#[tokio::test]
async fn immediate_respawn_uses_the_same_default_radius_landing() {
    join_move_and_respawn(true).await;
}
