//! 袭击逐刻场景：官方 `Raids` / `Raid.tick` 和 Bamboo 的正式 `raid::tick`
//! 比每一刻的存档字段、名册数、全局计数和刷出的袭击者。
//!
//! Raid tick-by-tick scenes: compare official `Raids` / `Raid.tick` against
//! Bamboo's production `raid::tick` on every saved field, roster count, global
//! counters and spawned raider each tick.

use serde::Deserialize;

use super::Report;

/// Java 一侧发来的场景。`steps` 每刻一项。
///
/// The scene from the Java side, with one entry per tick.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Case {
    name: String,
    difficulty: i32,
    omen_level: i32,
    raids_rule: bool,
    seed: i64,
    ticks: i32,
    spawn_x: i32,
    spawn_y: i32,
    spawn_z: i32,
    steps: Vec<Step>,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Step {
    tick: i32,
    raids: State,
    spawned: Vec<[i32; 4]>,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct State {
    started: bool,
    active: bool,
    ticks_active: i64,
    raid_omen_level: i32,
    groups_spawned: i32,
    cooldown_ticks: i32,
    post_raid_ticks: i32,
    total_health: i32,
    group_count: i32,
    status: String,
    center_x: i32,
    center_y: i32,
    center_z: i32,
    raiders_alive: i32,
    heroes: i32,
    next_id: i32,
    raid_tick: i32,
    map_size: i32,
}

/// 把 Bamboo 的快照转成和 Java 侧同形的行，方便逐项报错。
fn row(state: &mc_bamboo_game::raid::ReplayRaidState) -> State {
    State {
        started: state.started,
        active: state.active,
        ticks_active: state.ticks_active,
        raid_omen_level: state.raid_omen_level,
        groups_spawned: state.groups_spawned,
        cooldown_ticks: state.cooldown_ticks,
        post_raid_ticks: state.post_raid_ticks,
        total_health: state.total_health_bits,
        group_count: state.group_count,
        status: state.status.to_owned(),
        center_x: state.center_x,
        center_y: state.center_y,
        center_z: state.center_z,
        raiders_alive: state.raiders_alive,
        heroes: state.heroes,
        next_id: state.next_id,
        raid_tick: state.raid_tick,
        map_size: state.map_size,
    }
}

impl Report {
    pub(super) fn replay_raid(&mut self, case: &Case) {
        self.cases += 1;
        let scene = mc_bamboo_game::raid::ReplayRaidScene {
            difficulty: case.difficulty,
            omen_level: case.omen_level,
            raids_rule: case.raids_rule,
            seed: case.seed,
            ticks: case.ticks,
            spawn_x: case.spawn_x,
            spawn_y: case.spawn_y,
            spawn_z: case.spawn_z,
            victory: case.name.starts_with("raid_victory"),
        };
        let mut ours = mc_bamboo_game::raid::replay_raid(&scene);
        // 改坏确认：故意把冷却的初值改错、把首波刷出的数量改错、把胜利状态改错、
        // 把英雄数改错。每一处都应该被逐刻比较抓到。
        let break_test = self.break_test.as_deref();
        match break_test {
            Some("raid_no_cooldown") if ours.len() > 1 => {
                for step in &mut ours {
                    step.state.cooldown_ticks = 0;
                }
            }
            Some("raid_no_spawn") => {
                for step in &mut ours {
                    step.spawned.clear();
                }
            }
            Some("raid_no_victory") => {
                for step in &mut ours {
                    if step.state.status == "VICTORY" {
                        step.state.status = "ONGOING";
                    }
                }
            }
            Some("raid_fake_hero") => {
                for step in &mut ours {
                    step.state.heroes += 1;
                }
            }
            _ => {}
        }
        if ours.len() != case.steps.len() {
            self.mismatch(format!(
                "{}: official has {} ticks, Bamboo has {}",
                case.name,
                case.steps.len(),
                ours.len()
            ));
        }
        self.steps += ours.len();
        for (index, (expected, actual)) in case.steps.iter().zip(ours.iter()).enumerate() {
            if expected.tick != index as i32 {
                self.mismatch(format!(
                    "{} tick {index}: official tick {}, out of order",
                    case.name, expected.tick
                ));
            }
            let ours_row = row(&actual.state);
            if ours_row != expected.raids {
                self.mismatch(format!(
                    "{} tick {index}: official {:?}, Bamboo {ours_row:?}",
                    case.name, expected.raids
                ));
            }
            let spawned: Vec<[i32; 4]> = actual
                .spawned
                .iter()
                .map(|member| [member.entity_type, member.x2, member.y, member.z2])
                .collect();
            if spawned != expected.spawned {
                self.mismatch(format!(
                    "{} tick {index} spawned: official {:?}, Bamboo {spawned:?}",
                    case.name, expected.spawned
                ));
            }
        }
    }
}
