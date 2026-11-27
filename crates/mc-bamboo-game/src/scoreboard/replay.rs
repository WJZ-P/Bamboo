//! `mc-bamboo-vanilla scoreboard-oracle` 的重放：在和 `ExecuteOracle.java` 的 `Scene`
//! 一样的小场景里（管理员玩家 `Kitten`、两头牛、`result` 目标）逐步执行一道题，
//! 返回每一步的反馈展开文本和最后整张计分板。只在 `oracle-samples` feature 下编进库里。
//!
//! The replay behind `mc-bamboo-vanilla scoreboard-oracle`. In the same small scene as
//! `Scene` in `ExecuteOracle.java` (the operator `Kitten`, two cows, the `result`
//! objective) it runs one case step by step and returns every step's decomposed feedback
//! and the final scoreboard. Gated behind the `oracle-samples` feature.

use mc_bamboo_blocks::entity::Entity;
use mc_bamboo_core::Vec3;
use mc_bamboo_data::registries::{custom_stat, entity_type};
use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
use uuid::Uuid;

use crate::{
    command,
    global::Global,
    global_port::GlobalPort,
    inbound::{PlayerId, PlayerSink},
    outgoing::Outgoing,
    player::Player,
    region::Region,
};

/// 什么都不收的连接。
///
/// A connection that discards everything.
struct Discard;

impl PlayerSink for Discard {
    fn send(&mut self, _frames: Outgoing) -> bool {
        true
    }

    fn close(&mut self) {}
}

/// 一道题的场景。
///
/// The scene of one case.
struct Scene {
    global: Global,
    region: Region,
    id: PlayerId,
    /// 离玩家近的那头牛（6.5, -60, 6.5）的 UUID。
    ///
    /// The UUID of the cow nearer to the player, at (6.5, -60, 6.5).
    cow: [u8; 16],
    /// 远处那头牛（2.5, -60, 2.5）的 UUID。
    ///
    /// The UUID of the far cow, at (2.5, -60, 2.5).
    far: [u8; 16],
}

impl Scene {
    fn new() -> Self {
        let mut global = Global::default();
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let mut player = Player::new(
            id,
            Uuid::from_u128(1),
            "Kitten".to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            Vec3::new(8.5, -60.0, 8.5),
        );
        player.permission_level = 2;
        player.is_op = true;
        region.links.add(id, Box::new(Discard));
        region.insert_player(player);
        let far = region
            .entities
            .spawn(Entity::cow(Vec3::new(2.5, -60.0, 2.5)));
        let near = region
            .entities
            .spawn(Entity::cow(Vec3::new(6.5, -60.0, 6.5)));
        let uuid_of = |uid| {
            region
                .entities
                .get(uid)
                .and_then(|entity| entity.kind.mob().map(|mob| mob.uuid))
                .unwrap_or_default()
        };
        let (cow, far) = (uuid_of(near), uuid_of(far));
        for setup in [
            "scoreboard objectives add result dummy",
            "scoreboard players set #total result 0",
        ] {
            command::on_command_public(&mut global, &mut region, id, setup);
        }
        global.publish(&mut region);
        region.links.flush();
        Self {
            global,
            region,
            id,
            cow,
            far,
        }
    }

    /// 题目里的实体名对应的记分板名字。
    ///
    /// The scoreboard name behind an entity name used in the cases.
    fn holder(&self, name: &str) -> String {
        match name {
            "player" => "Kitten".to_owned(),
            "other" => "Other".to_owned(),
            "cow" => Uuid::from_bytes(self.cow).to_string(),
            "far" => Uuid::from_bytes(self.far).to_string(),
            other => panic!("unknown entity {other}"),
        }
    }

    /// 只问计分板、不改状态的步骤：结果按 Java 侧同样的写法记进反馈。
    ///
    /// Steps that only ask the scoreboard and change nothing; the answer is recorded as
    /// feedback in the Java side's format.
    fn ask(&self, words: &[&str]) -> Option<String> {
        let board = self.global.scoreboard();
        let pair = || (self.holder(words[1]), self.holder(words[2]));
        match words.first().copied() {
            Some("teamsallow") => {
                let (attacker, target) = pair();
                Some(format!(
                    "L[teamsallow:{}]",
                    board.holders_allow_damage(&attacker, &target)
                ))
            }
            Some("pushable") => {
                let (pusher, pushed) = pair();
                Some(format!(
                    "L[pushable:{}]",
                    board.holders_can_push(&pusher, &pushed)
                ))
            }
            _ => None,
        }
    }

    /// 一条 `!` 开头的直接调用，和 Java 侧 `scoreboardOp` 一一对应，走正式的记分入口。
    ///
    /// One direct call starting with `!`, matching `scoreboardOp` on the Java side and
    /// going through the production scoring entry points.
    fn direct(&mut self, words: &[&str]) {
        let count = || words.get(2).and_then(|word| word.parse().ok()).unwrap_or(0);
        let named = |index: usize| format!("minecraft:{}", words.get(index).copied().unwrap_or(""));
        match words.first().copied() {
            Some("used") => {
                let name = named(1);
                let item = mc_bamboo_data::items::ITEMS
                    .iter()
                    .position(|item| item.name == name)
                    .expect("a known item") as i32;
                for _ in 0..count() {
                    crate::item_use_stats::award(&mut self.region, self.id, item);
                }
            }
            Some("custom") => {
                let stat = custom_stat_id(&named(1));
                if stat == custom_stat::DAMAGE_DEALT {
                    crate::item_use_stats::award_damage_dealt(&mut self.region, self.id, count());
                } else {
                    crate::item_use_stats::award_custom(&mut self.region, self.id, stat, count());
                }
            }
            Some("reset") => {
                let stat = custom_stat_id(&named(1));
                if let Some(player) = self.region.players.get_mut(&self.id) {
                    player.item_use_stats.reset_custom(stat);
                }
            }
            Some("killscore") => {
                let uuid = *self.region.players[&self.id].uuid.as_bytes();
                crate::player_death::award_mob_kill(
                    &mut self.region,
                    Some(uuid),
                    None,
                    entity_type::COW,
                    self.cow,
                );
            }
            Some("killed") => crate::player_death::award_mob_kill(
                &mut self.region,
                None,
                Some(self.id.0),
                entity_type::COW,
                self.cow,
            ),
            other => panic!("unknown step {other:?}"),
        }
        super::flush_awards(&mut GlobalPort::Direct(&mut self.global), &mut self.region);
    }
}

fn custom_stat_id(name: &str) -> i32 {
    custom_stat::NAMES
        .iter()
        .position(|stat| *stat == name)
        .expect("a known custom stat") as i32
}

/// 重放一道计分板题，返回 `(反馈, 计分板)`。
///
/// Replay one scoreboard case and return `(feedback, scoreboard)`.
pub fn replay_scoreboard(line: &str) -> (String, String) {
    let mut scene = Scene::new();
    let mut feedback = Vec::new();
    for step in line.split(" ;; ") {
        if let Some(direct) = step.strip_prefix('!') {
            let words: Vec<&str> = direct.split(' ').collect();
            match scene.ask(&words) {
                Some(answer) => feedback.push(answer),
                None => scene.direct(&words),
            }
            continue;
        }
        let mut lines = Vec::new();
        command::on_command_public_feedback(
            &mut scene.global,
            &mut scene.region,
            scene.id,
            step,
            &mut lines,
        );
        scene.region.links.flush();
        feedback.extend(lines);
    }
    (feedback.join("\n"), scene.global.scoreboard().dump())
}
