//! 凋灵的实体 Boss 血条：官方 `WitherBoss.bossEvent`（`ServerBossEvent`）。
//!
//! 每只凋灵一根栏。玩家进入 / 离开实体跟踪范围（`EntityTracking.seen_by`）时发
//! add / remove 包，进度变化时发 update 包；栏消失（凋灵死亡 / 被移除）时给所有
//! 观众发 remove。颜色 PURPLE、样式 PROGRESS、`setDarkenScreen(true)`。
//!
//! Entity boss bars for withers: vanilla `WitherBoss.bossEvent`
//! (`ServerBossEvent`). One bar per wither. Players entering/leaving the entity
//! tracking range (`EntityTracking.seen_by`) get add/remove packets, progress
//! changes get update packets, and a disappearing bar sends remove to all its
//! viewers. PURPLE / PROGRESS with `setDarkenScreen(true)`.

use std::collections::{BTreeMap, BTreeSet};

use mc_bamboo_blocks::entity::{KindPublic, mob::MobType};
use mc_bamboo_protocol::{
    packet::play::{BossAction, BossEvent},
    text::Component,
};
use uuid::Uuid;

use crate::{inbound::PlayerId, links::encode_frames, region::Region};

/// `BossEvent.BossBarColor.PURPLE` 的序号。
///
/// Ordinal of `BossEvent.BossBarColor.PURPLE`.
const WITHER_BAR_COLOR: i32 = 5;
/// `BossEvent.BossBarOverlay.PROGRESS` 的序号。
///
/// Ordinal of `BossEvent.BossBarOverlay.PROGRESS`.
const WITHER_BAR_OVERLAY: i32 = 0;
/// `ServerBossEvent.setDarkenScreen(true)`：bit 0。
///
/// `ServerBossEvent.setDarkenScreen(true)`: bit 0.
const DARKEN_SCREEN: u8 = 1;

/// 一根凋灵血条的运行时状态（`ServerBossEvent` 不存档）。
///
/// Runtime state of one wither boss bar (`ServerBossEvent` is not saved).
#[derive(Debug, Clone, PartialEq)]
struct WitherBar {
    uuid: [u8; 16],
    /// 收到过 add 包的玩家（`ServerBossEvent.players`）。
    ///
    /// Players that received the add packet (`ServerBossEvent.players`).
    players: BTreeSet<PlayerId>,
    progress: f32,
}

/// 全服所有凋灵的实体 Boss 血条，按实体网络 ID 索引。
///
/// All wither entity boss bars in the region, keyed by network entity id.
#[derive(Debug, Clone, Default)]
pub(crate) struct WitherBossBars {
    bars: BTreeMap<i32, WitherBar>,
}

impl WitherBossBars {
    /// 拆岛：跟着搬走的凋灵实体把对应的血条带走。
    ///
    /// Island split: bars of the withers that move go with them.
    pub(crate) fn split_off(&mut self, moved: &BTreeSet<i32>) -> Self {
        let mut bars = BTreeMap::new();
        self.bars.retain(|uid, bar| {
            if moved.contains(uid) {
                bars.insert(*uid, bar.clone());
                false
            } else {
                true
            }
        });
        Self { bars }
    }

    /// 合岛：把另一边的血条并回来。
    ///
    /// Island merge: absorb the other side's bars.
    pub(crate) fn absorb(&mut self, other: Self) {
        self.bars.extend(other.bars);
    }
}

/// `bossEvent` 的 add 包：名字是 `getDisplayName()`（缺省 `entity.minecraft.wither`）。
///
/// The `bossEvent` add packet: the name is `getDisplayName()` (default
/// `entity.minecraft.wither`).
fn add_frame(uuid: [u8; 16], progress: f32) -> Vec<u8> {
    let name = Component::translate("entity.minecraft.wither", []);
    encode_frames(&BossEvent {
        id: Uuid::from_bytes(uuid),
        action: BossAction::Add {
            name: &name,
            progress,
            color: WITHER_BAR_COLOR,
            overlay: WITHER_BAR_OVERLAY,
            flags: DARKEN_SCREEN,
        },
    })
}

/// `ServerBossEvent.removePlayer` / 栏消失时的 remove 包。
///
/// The remove packet of `ServerBossEvent.removePlayer` / a disappearing bar.
fn remove_frame(uuid: [u8; 16]) -> Vec<u8> {
    encode_frames(&BossEvent {
        id: Uuid::from_bytes(uuid),
        action: BossAction::Remove,
    })
}

/// `ServerBossEvent.setProgress`：值变了才广播。
///
/// `ServerBossEvent.setProgress`: broadcast only when the value changes.
fn progress_frame(uuid: [u8; 16], progress: f32) -> Vec<u8> {
    encode_frames(&BossEvent {
        id: Uuid::from_bytes(uuid),
        action: BossAction::UpdateProgress(progress),
    })
}

/// 每刻跟着 `EntityTracking` 刷新所有凋灵血条的观众和进度。
///
/// Refresh every wither bar's audience and progress against `EntityTracking` each tick.
pub(crate) fn tick(region: &mut Region) {
    let mut bars = std::mem::take(&mut region.wither_boss_bars);
    // 当前活着的凋灵：`uid -> (栏 UUID, 进度, 正在跟踪的玩家)`。
    // Living withers: `uid -> (bar UUID, progress, tracking players)`.
    let mut living: BTreeMap<i32, ([u8; 16], f32, Vec<PlayerId>)> = BTreeMap::new();
    for public in region.entities.iter_public() {
        if let KindPublic::Mob(mob) = &public.kind
            && mob.mob_type == MobType::Wither
        {
            let seen = region.entity_tracking.seen_by(public.uid);
            living.insert(public.uid, (mob.uuid, mob.wither_boss_progress, seen));
        }
    }

    for (&uid, (uuid, progress, seen)) in &living {
        let bar = bars.bars.entry(uid).or_insert_with(|| WitherBar {
            uuid: *uuid,
            players: BTreeSet::new(),
            progress: f32::NEG_INFINITY,
        });
        bar.uuid = *uuid;
        // 新进入跟踪范围的玩家：add（带当前进度）。
        // Players newly in tracking range: add (with the current progress).
        let mut pending_add: Option<Vec<u8>> = None;
        for player in seen {
            if !bar.players.contains(player) {
                let frame = pending_add.get_or_insert_with(|| add_frame(*uuid, *progress));
                region.links.send_frames(*player, frame);
            }
        }
        // 离开跟踪范围的玩家：remove。
        // Players that left tracking range: remove.
        for player in &bar.players {
            if !seen.contains(player) {
                let frame = remove_frame(bar.uuid);
                region.links.send_frames(*player, &frame);
            }
        }
        // 进度变化：给当前所有观众发 update。
        // Progress changed: update every current viewer.
        if bar.progress != *progress {
            let frame = progress_frame(*uuid, *progress);
            for player in seen {
                region.links.send_frames(*player, &frame);
            }
        }
        bar.players = seen.iter().copied().collect();
        bar.progress = *progress;
    }

    // 凋灵没了（死亡 / 移除）：给所有观众发 remove，删掉这根栏。
    // A wither is gone (dead / removed): remove for all viewers and drop the bar.
    bars.bars.retain(|uid, bar| {
        if living.contains_key(uid) {
            true
        } else {
            let frame = remove_frame(bar.uuid);
            for player in &bar.players {
                region.links.send_frames(*player, &frame);
            }
            false
        }
    });
    region.wither_boss_bars = bars;
}

#[cfg(test)]
mod tests {
    use mc_bamboo_protocol::{
        codec::Reader,
        frame::{MAX_FRAME_LEN, find_frame},
    };

    use super::*;

    /// 拆掉帧头和包 id，只留下 UUID 之后的 body。
    ///
    /// Strip the frame header and packet id, leaving the body after the UUID.
    fn body(frame: &[u8]) -> Vec<u8> {
        let bounds = find_frame(frame, MAX_FRAME_LEN)
            .expect("valid frame")
            .expect("complete frame");
        let end = bounds.header_len + bounds.payload_len;
        let mut reader = Reader::new(&frame[bounds.header_len..end]);
        let _id = reader.varint().unwrap();
        reader.rest().to_vec()
    }

    /// add 包：动作 0、进度 0.5、PURPLE / PROGRESS / darken-screen。
    ///
    /// Add packet: action 0, progress 0.5, PURPLE / PROGRESS / darken-screen.
    #[test]
    fn add_packet_uses_purple_progress_and_darken_screen() {
        let frame = body(&add_frame([7; 16], 0.5));
        assert_eq!(frame[16], 0, "ADD action");
        let tail = frame.len() - 4 - 1 - 1 - 1;
        assert_eq!(frame[tail..tail + 4], 0.5_f32.to_be_bytes());
        assert_eq!(frame[tail + 4], WITHER_BAR_COLOR as u8, "PURPLE");
        assert_eq!(frame[tail + 5], WITHER_BAR_OVERLAY as u8, "PROGRESS");
        assert_eq!(frame[tail + 6], DARKEN_SCREEN, "darken screen");
    }

    /// remove 包：动作 1，只带 UUID。
    ///
    /// Remove packet: action 1, UUID only.
    #[test]
    fn remove_packet_is_action_one() {
        let frame = body(&remove_frame([7; 16]));
        assert_eq!(frame[16], 1, "REMOVE action");
        assert_eq!(frame.len(), 17);
    }

    /// update 包：动作 2 + 新进度。
    ///
    /// Update packet: action 2 plus the new progress.
    #[test]
    fn progress_packet_carries_the_value() {
        let frame = body(&progress_frame([7; 16], 0.25));
        assert_eq!(frame[16], 2, "UPDATE_PROGRESS action");
        assert_eq!(frame[17..21], 0.25_f32.to_be_bytes());
    }

    /// 拆岛带走指定的凋灵血条，合岛并回来。
    ///
    /// Island split takes the requested bars; merge absorbs them back.
    #[test]
    fn split_off_and_absorb_keep_bar_state() {
        let mut bars = WitherBossBars::default();
        bars.bars.insert(
            3,
            WitherBar {
                uuid: [1; 16],
                players: BTreeSet::new(),
                progress: 0.5,
            },
        );
        bars.bars.insert(
            9,
            WitherBar {
                uuid: [2; 16],
                players: BTreeSet::new(),
                progress: 0.75,
            },
        );
        let moved: BTreeSet<i32> = [9].into_iter().collect();
        let taken = bars.split_off(&moved);
        assert_eq!(bars.bars.len(), 1);
        assert!(bars.bars.contains_key(&3));
        assert_eq!(taken.bars[&9].progress, 0.75);
        bars.absorb(taken);
        assert_eq!(bars.bars.len(), 2);
        assert!(bars.bars.contains_key(&9));
    }
}
