//! 统计类和击杀类准则的自动加分。
//!
//! Automatic scores for stat and kill criteria.

use mc_bamboo_data::{items, registries::entity_type};
use mc_bamboo_protocol::text::Component;

use crate::{
    player_death::KillCredit,
    world::tests::{Client, Harness},
};

/// 一刻跑一条命令。准备阶段的命令很多，先把刷屏计数清掉，免得被当成刷屏踢出去。
///
/// Run one command per tick. Setup needs many commands, so the spam counter is cleared
/// first to avoid a spam kick.
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

fn operator(name: &str) -> (Harness, Client) {
    let mut server = Harness::new(2);
    let client = server.join_and_settle(name);
    server
        .region()
        .players
        .get_mut(&client.id)
        .expect("the player is in the world")
        .permission_level = 2;
    (server, client)
}

fn score(server: &Harness, player: &str, objective: &str) -> Option<i32> {
    server.global().scoreboard().score_value(player, objective)
}

#[test]
fn a_used_item_and_dealt_damage_add_to_their_objectives() {
    let (mut server, client) = operator("Kitten");
    for text in [
        "scoreboard objectives add bow minecraft.used:minecraft.bow",
        "scoreboard objectives add bow2 minecraft.used:minecraft.bow",
        "scoreboard objectives add egg minecraft.used:minecraft.egg",
        "scoreboard objectives add dealt minecraft.custom:minecraft.damage_dealt",
    ] {
        run(&mut server, &client, text);
    }
    crate::item_use_stats::award(server.region(), client.id, items::BOW);
    crate::item_use_stats::award(server.region(), client.id, items::BOW);
    crate::item_use_stats::award_damage_dealt(server.region(), client.id, 35);
    server.tick();
    assert_eq!(score(&server, "Kitten", "bow"), Some(2));
    assert_eq!(score(&server, "Kitten", "bow2"), Some(2));
    assert_eq!(score(&server, "Kitten", "egg"), None);
    assert_eq!(score(&server, "Kitten", "dealt"), Some(35));
}

#[test]
fn awards_before_the_objective_exists_are_not_counted() {
    let (mut server, client) = operator("Kitten");
    crate::item_use_stats::award(server.region(), client.id, items::BOW);
    server.tick();
    run(
        &mut server,
        &client,
        "scoreboard objectives add bow minecraft.used:minecraft.bow",
    );
    server.tick();
    assert_eq!(score(&server, "Kitten", "bow"), None);
}

#[test]
fn play_time_counts_every_tick() {
    let (mut server, client) = operator("Kitten");
    run(
        &mut server,
        &client,
        "scoreboard objectives add played minecraft.custom:minecraft.play_time",
    );
    let before = score(&server, "Kitten", "played").unwrap_or(0);
    for _ in 0..5 {
        server.tick();
    }
    assert_eq!(score(&server, "Kitten", "played"), Some(before + 5));
}

#[test]
fn health_food_and_air_follow_the_player() {
    let (mut server, client) = operator("Kitten");
    for text in [
        "scoreboard objectives add hp health",
        "scoreboard objectives add hunger food",
        "scoreboard objectives add breath air",
        "scoreboard objectives add plates armor",
    ] {
        run(&mut server, &client, text);
    }
    server.tick();
    // 进服第一刻 `lastRecorded*` 已经记过，之后新建的目标要等值变了才写。
    //
    // `lastRecorded*` was set on the first tick after join; an objective created later
    // is written only once the value changes.
    assert_eq!(score(&server, "Kitten", "hp"), None);
    {
        let player = server.region().players.get_mut(&client.id).unwrap();
        player.game_mode = mc_bamboo_protocol::packet::play::GameMode::Creative;
        player.health.health = 13.5;
        player.health.absorption = 2.25;
        player.health.food.food_level = 7;
        player.environment.air_supply = 120;
    }
    server.tick();
    // `Mth.ceil(13.5F + 2.25F)`。
    assert_eq!(score(&server, "Kitten", "hp"), Some(16));
    assert_eq!(score(&server, "Kitten", "hunger"), Some(7));
    // 在空气里每刻回 4 点氧气。
    // Air comes back 4 per tick out of water.
    assert_eq!(score(&server, "Kitten", "breath"), Some(124));
    assert_eq!(score(&server, "Kitten", "plates"), None);
}

#[test]
fn a_player_kill_scores_both_sides() {
    let (mut server, killer) = operator("Killer");
    let victim = server.join_and_settle("Victim");
    for text in [
        "scoreboard objectives add deaths deathCount",
        "scoreboard objectives add pk playerKillCount",
        "scoreboard objectives add all totalKillCount",
        "scoreboard objectives add redkills teamkill.red",
        "scoreboard objectives add byblue killedByTeam.blue",
        "scoreboard objectives add died minecraft.custom:minecraft.deaths",
        "scoreboard objectives add pkstat minecraft.custom:minecraft.player_kills",
        "scoreboard objectives add byplayer minecraft.killed_by:minecraft.player",
        "scoreboard objectives add rest minecraft.custom:minecraft.time_since_rest",
        "team add red",
        "team modify red color red",
        "team add blue",
        "team modify blue color blue",
        "team join red Victim",
        "team join blue Killer",
    ] {
        run(&mut server, &killer, text);
    }
    assert!(score(&server, "Victim", "rest").unwrap_or(0) > 0);
    server
        .region()
        .players
        .get_mut(&victim.id)
        .unwrap()
        .hurt_by_player = Some(KillCredit::Player {
        display: Component::text("Killer"),
        memory: 100,
        id: killer.id,
    });
    run(&mut server, &killer, "kill Victim");
    assert_eq!(score(&server, "Victim", "deaths"), Some(1));
    assert_eq!(score(&server, "Victim", "died"), Some(1));
    assert_eq!(score(&server, "Victim", "byplayer"), Some(1));
    assert_eq!(score(&server, "Victim", "byblue"), Some(1));
    assert_eq!(score(&server, "Killer", "pk"), Some(1));
    assert_eq!(score(&server, "Killer", "all"), Some(1));
    assert_eq!(score(&server, "Killer", "pkstat"), Some(1));
    assert_eq!(score(&server, "Killer", "redkills"), Some(1));
    assert_eq!(score(&server, "Killer", "deaths"), None);
    // `resetStat(TIME_SINCE_REST)` 之后同一刻的 `doTick` 不再加（死者这一刻已经
    // 过了加分的地方），之后每刻照常加。
    assert!(score(&server, "Victim", "rest").unwrap_or(i32::MAX) <= 1);
}

#[test]
fn a_death_without_a_killer_only_counts_the_death() {
    let (mut server, client) = operator("Kitten");
    for text in [
        "scoreboard objectives add deaths deathCount",
        "scoreboard objectives add all totalKillCount",
        "scoreboard objectives add byplayer minecraft.killed_by:minecraft.player",
    ] {
        run(&mut server, &client, text);
    }
    run(&mut server, &client, "kill Kitten");
    assert_eq!(score(&server, "Kitten", "deaths"), Some(1));
    assert_eq!(score(&server, "Kitten", "all"), None);
    assert_eq!(score(&server, "Kitten", "byplayer"), None);
}

#[test]
fn a_mob_kill_scores_the_credited_player_and_the_killing_blow() {
    let (mut server, client) = operator("Kitten");
    let mob = [7; 16];
    let mob_name = uuid::Uuid::from_bytes(mob).to_string();
    for text in [
        "scoreboard objectives add all totalKillCount".to_owned(),
        "scoreboard objectives add pk playerKillCount".to_owned(),
        "scoreboard objectives add cows minecraft.killed:minecraft.cow".to_owned(),
        "scoreboard objectives add pigs minecraft.killed:minecraft.pig".to_owned(),
        "scoreboard objectives add mobs minecraft.custom:minecraft.mob_kills".to_owned(),
        "scoreboard objectives add green teamkill.green".to_owned(),
        "team add herd".to_owned(),
        "team modify herd color green".to_owned(),
        format!("team join herd {mob_name}"),
    ] {
        run(&mut server, &client, &text);
    }
    let uuid = *server.region().players[&client.id].uuid.as_bytes();
    crate::player_death::award_mob_kill(
        server.region(),
        Some(uuid),
        Some(client.id.0),
        entity_type::COW,
        mob,
    );
    server.tick();
    assert_eq!(score(&server, "Kitten", "all"), Some(1));
    assert_eq!(score(&server, "Kitten", "pk"), None);
    assert_eq!(score(&server, "Kitten", "cows"), Some(1));
    assert_eq!(score(&server, "Kitten", "pigs"), None);
    assert_eq!(score(&server, "Kitten", "mobs"), Some(1));
    assert_eq!(score(&server, "Kitten", "green"), Some(1));
    // 只有致命一击、没有击杀归属：只记 `ENTITY_KILLED`。
    crate::player_death::award_mob_kill(
        server.region(),
        None,
        Some(client.id.0),
        entity_type::COW,
        mob,
    );
    server.tick();
    assert_eq!(score(&server, "Kitten", "all"), Some(1));
    assert_eq!(score(&server, "Kitten", "cows"), Some(2));
}
