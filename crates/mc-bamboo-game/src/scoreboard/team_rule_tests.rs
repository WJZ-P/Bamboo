//! 队伍规则接进游戏逻辑：友军伤害和死亡消息可见性。
//!
//! Team rules in game logic: friendly fire and death message visibility.

use mc_bamboo_blocks::entity::mob::{DamageKind, DamageSource};
use mc_bamboo_data::packets::play::clientbound;
use mc_bamboo_protocol::packet::play::GameMode;

use crate::world::tests::{Client, Harness, frames_with_id};

fn run(server: &mut Harness, client: &Client, text: &str) {
    if let Some(player) = server.region().players.get_mut(&client.id) {
        player.spam = Default::default();
    }
    server.request(
        client,
        mc_bamboo_protocol::packet::play::PlayRequest::ChatCommand {
            command: text.to_owned(),
            signed: None,
        },
    );
    server.tick();
}

/// 一名管理员和两名生存模式的玩家，`A`、`B` 在 `red` 队，`C` 没有队伍。
///
/// An operator and survival players: `A` and `B` in team `red`, `C` without a team.
fn scene() -> (Harness, Client, Client, Client, Client) {
    let mut server = Harness::new(2);
    let op = server.join_and_settle("Op");
    server
        .region()
        .players
        .get_mut(&op.id)
        .unwrap()
        .permission_level = 2;
    let a = server.join_and_settle("A");
    let b = server.join_and_settle("B");
    let c = server.join_and_settle("C");
    for client in [&a, &b, &c] {
        let player = server.region().players.get_mut(&client.id).unwrap();
        player.game_mode = GameMode::Survival;
        player.health.invulnerable_time = 0;
    }
    for text in ["team add red", "team join red A", "team join red B"] {
        run(&mut server, &op, text);
    }
    (server, op, a, b, c)
}

fn hit(server: &mut Harness, attacker: &Client, victim: &Client) -> bool {
    let mut source = DamageSource::environment(DamageKind::PlayerAttack);
    source.attacker = Some(attacker.id.0);
    source.direct = Some(attacker.id.0);
    source.attacker_is_player = true;
    let region = server.region();
    let player = region.players.get_mut(&victim.id).unwrap();
    player.health.invulnerable_time = 0;
    player.health.health = 20.0;
    player.health.damage_cooldown = 0;
    player.health.last_hurt = 0.0;
    crate::player_hurt::hurt_player(region, victim.id, source, 2.0, &mut None)
}

#[test]
fn friendly_fire_off_protects_teammates_only() {
    let (mut server, op, a, b, c) = scene();
    assert!(hit(&mut server, &a, &b), "friendly fire is on by default");
    run(&mut server, &op, "team modify red friendlyFire false");
    assert!(!hit(&mut server, &a, &b), "teammates are spared");
    assert!(!hit(&mut server, &b, &a));
    assert!(hit(&mut server, &a, &c), "other players are not");
    assert!(hit(&mut server, &c, &a));
    run(&mut server, &op, "team modify red friendlyFire true");
    assert!(hit(&mut server, &a, &b));
}

#[test]
fn pvp_off_blocks_every_player_source() {
    let (mut server, op, a, _b, c) = scene();
    assert!(hit(&mut server, &c, &a));
    run(&mut server, &op, "gamerule pvp false");
    assert!(!hit(&mut server, &c, &a));
}

fn saw_death(client: &Client) -> bool {
    let key = b"death.attack.genericKill";
    frames_with_id(&client.take_frames(), clientbound::SYSTEM_CHAT)
        .iter()
        .any(|frame| frame.body.windows(key.len()).any(|window| window == key))
}

/// 把 `A` 杀掉再复活，返回 `[A, B, C]` 各自有没有在聊天栏看到死亡消息。
///
/// Kill `A` and bring it back; returns whether each of `[A, B, C]` saw the death message
/// in chat.
fn kill_a(server: &mut Harness, op: &Client, clients: [&Client; 3]) -> [bool; 3] {
    for client in clients {
        client.take_frames();
    }
    run(server, op, "kill A");
    let seen = clients.map(saw_death);
    crate::player_respawn::respawn(server.region(), clients[0].id);
    server.tick();
    seen
}

#[test]
fn death_messages_follow_the_team_visibility() {
    let (mut server, op, a, b, c) = scene();
    let clients = [&a, &b, &c];
    assert_eq!(kill_a(&mut server, &op, clients), [true, true, true]);
    run(
        &mut server,
        &op,
        "team modify red deathMessageVisibility hideForOtherTeams",
    );
    assert_eq!(kill_a(&mut server, &op, clients), [false, true, false]);
    run(
        &mut server,
        &op,
        "team modify red deathMessageVisibility hideForOwnTeam",
    );
    assert_eq!(kill_a(&mut server, &op, clients), [false, false, true]);
    run(
        &mut server,
        &op,
        "team modify red deathMessageVisibility never",
    );
    assert_eq!(kill_a(&mut server, &op, clients), [false, false, false]);
    run(
        &mut server,
        &op,
        "team modify red deathMessageVisibility always",
    );
    assert_eq!(kill_a(&mut server, &op, clients), [true, true, true]);
}

#[test]
fn a_victim_without_a_team_is_announced_to_everyone() {
    let (mut server, op, a, b, c) = scene();
    run(
        &mut server,
        &op,
        "team modify red deathMessageVisibility never",
    );
    for client in [&a, &b, &c] {
        client.take_frames();
    }
    run(&mut server, &op, "kill C");
    assert_eq!([&a, &b, &c].map(saw_death), [true, true, true]);
}
