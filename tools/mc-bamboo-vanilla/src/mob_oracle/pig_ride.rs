//! 猪的骑乘对拍：右键上猪、胡萝卜钓竿加速（耐久、同步时长、逐刻速度和朝向）、下车落点。
//!
//! Pig riding comparison: right-click mounting, carrot-on-a-stick boosts (durability,
//! synced duration, per-tick speed and rotation) and the dismount spot.

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MountCase {
    setup: i32,
    item: i32,
    sneak: bool,
    seed: i64,
    swing: String,
    count: i32,
    vehicle: bool,
    controller: bool,
    saddle: i32,
    pig_seed: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UseCase {
    boosted: bool,
    item: i32,
    damage: i32,
    count: i32,
    boost_time: i32,
    item_used: i32,
    seed: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BoostCase {
    damage: i32,
    seed: i64,
    uses: Vec<UseCase>,
    ticks: Vec<[i32; 6]>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DismountCase {
    layout: i32,
    yaw: i32,
    spot: [i64; 3],
}

impl Report {
    pub(super) fn replay_pig_mount(&mut self, case: &MountCase) {
        self.cases += 1;
        let ours = mc_bamboo_game::pig_riding::replay_pig_mount(
            case.setup, case.item, case.sneak, case.seed,
        );
        let theirs = mc_bamboo_game::pig_riding::ReplayPigMount {
            swing: case.swing.clone(),
            count: case.count,
            vehicle: case.vehicle,
            controller: case.controller,
            saddle: case.saddle,
            seed: case.pig_seed,
        };
        if ours != theirs {
            self.mismatch(format!(
                "pig mount setup {} item {} sneak {}: official {theirs:?}, bamboo {ours:?}",
                case.setup, case.item, case.sneak
            ));
        }
    }

    pub(super) fn replay_pig_boost(&mut self, case: &BoostCase) {
        self.cases += 1;
        let (uses, ticks) = mc_bamboo_game::pig_riding::replay_pig_boost(case.damage, case.seed);
        let theirs: Vec<_> = case
            .uses
            .iter()
            .map(|u| mc_bamboo_game::pig_riding::ReplayPigUse {
                boosted: u.boosted,
                item: u.item,
                damage: u.damage,
                count: u.count,
                boost_time: u.boost_time,
                item_used: u.item_used,
                seed: u.seed,
            })
            .collect();
        if uses != theirs {
            self.mismatch(format!(
                "pig boost damage {}: official uses {theirs:?}, bamboo {uses:?}",
                case.damage
            ));
        }
        if ticks.len() != case.ticks.len() {
            self.mismatch(format!(
                "pig boost damage {}: official ran {} ticks, bamboo {}",
                case.damage,
                case.ticks.len(),
                ticks.len()
            ));
            return;
        }
        for (tick, (ours, theirs)) in ticks.iter().zip(&case.ticks).enumerate() {
            if ours != theirs {
                self.mismatch(format!(
                    "pig boost damage {} tick {tick}: official {theirs:?}, bamboo {ours:?}",
                    case.damage
                ));
                break;
            }
        }
    }

    pub(super) fn replay_pig_dismount(&mut self, case: &DismountCase) {
        self.cases += 1;
        let ours = mc_bamboo_game::pig_riding::replay_pig_dismount(
            case.layout,
            f32::from_bits(case.yaw as u32),
        );
        if ours != Some(case.spot) {
            self.mismatch(format!(
                "pig dismount layout {} yaw {}: official {:?}, bamboo {ours:?}",
                case.layout,
                f32::from_bits(case.yaw as u32),
                case.spot
            ));
        }
    }
}
