//! 真实玩家的肩膀状态：把 [`ShoulderState`] 接到 `Player` / `Region` 上。
//!
//! Real players' shoulder state: wiring the shared [`ShoulderState`] to
//! `Player` and `Region`. The perching, removal and respawn rules themselves live
//! in `mc-bamboo-blocks`, so the oracle fake world runs the same production code.

use mc_bamboo_blocks::entity::shoulder;
use mc_bamboo_core::Vec3;
use mc_bamboo_nbt::Tag;

use crate::{player::Player, region::Region};

/// 一次离肩要造回世界的鹦鹉：位置、NBT、主人 UUID。
struct SpawnBack {
    position: Vec3,
    tags: Vec<Tag>,
    owner: Option<[u8; 16]>,
}

/// `ServerPlayer.setEntityOnShoulder`。
pub fn set_entity_on_shoulder(player: &mut Player, tag: Tag) -> bool {
    let mut state = player.shoulder_state();
    let ok = shoulder::set_entity_on_shoulder(&mut state, tag);
    player.apply_shoulder_state(state);
    ok
}

/// `ServerPlayer.handleShoulderEntities`：每刻推进相对倒计时；上肩 20 刻后遇到
/// 离肩判据就把两只鹦鹉放回世界。
///
/// `ServerPlayer.handleShoulderEntities`: advance the relative counter and, after
/// 20 ticks, respawn both parrots when a removal condition holds.
pub fn tick(region: &mut Region) {
    let mut spawn_back = Vec::new();
    for player in region.players.values_mut() {
        let mut state = player.shoulder_state();
        let facts = shoulder::RemovalFacts {
            fall_distance: player.motion.fall_distance,
            in_water: player.motion.swimming,
            flying: player.sync.flying,
            sleeping: player.sleep.is_some(),
            in_powder_snow: player.environment.in_powder_snow,
            hurt: false,
        };
        if let Some(removed) = shoulder::step(&mut state, facts, player.position()) {
            spawn_back.push(SpawnBack {
                position: removed.position,
                tags: removed.tags,
                owner: Some(*player.uuid.as_bytes()),
            });
        }
        player.apply_shoulder_state(state);
    }
    for batch in spawn_back {
        respawn_into(&mut region.entities, batch);
    }
}

/// `Player.hurtServer` 一开头就调 `removeEntitiesOnShoulder()`：受伤立刻离肩，
/// 不看 `handleShoulderEntities` 的判据，也不看 20 刻锁。
///
/// `Player.hurtServer` calls `removeEntitiesOnShoulder()` first thing: being hurt
/// removes the parrots right away, bypassing the 20-tick lock.
pub fn remove_on_hurt(player: &mut Player, entities: &mut mc_bamboo_blocks::entity::Entities) {
    let mut state = player.shoulder_state();
    let facts = shoulder::RemovalFacts {
        hurt: true,
        ..shoulder::RemovalFacts::default()
    };
    let position = player.position();
    state.since = i32::MAX;
    if let Some(removed) = shoulder::step(&mut state, facts, position) {
        player.apply_shoulder_state(state);
        respawn_into(
            entities,
            SpawnBack {
                position: removed.position,
                tags: removed.tags,
                owner: Some(*player.uuid.as_bytes()),
            },
        );
    } else {
        player.apply_shoulder_state(state);
    }
}

/// `ServerPlayer.respawnEntityOnShoulder`：从 NBT 造回鹦鹉，认回主人。
fn respawn_into(entities: &mut mc_bamboo_blocks::entity::Entities, batch: SpawnBack) {
    for tag in batch.tags {
        if let Some(entity) = shoulder::respawn(&tag, batch.position, batch.owner) {
            entities.spawn(entity);
        }
    }
}
