//! 大脑对拍的 Bamboo 一侧：按官方报过来的传感器延迟和掉鳞甲计时摆好生物、重设洗牌
//! 随机源，逐刻照 Java 一侧 `BrainOracle.dump` 的格式写大脑快照。
//!
//! The Bamboo side of the brain oracle: set the sensor delays and scute timer the
//! official side reported, reseed the shuffle randoms, and dump the brain every tick in
//! the format of the Java `BrainOracle.dump`.

use mc_bamboo_blocks::entity::{
    Entity, EntityKind,
    ctx::PlayerView,
    mob::{
        Mob,
        brain::memory::{MemoryValue, PositionTracker},
    },
};
use mc_bamboo_core::{BlockPos, Vec3};

use super::MobLevel;

/// 场景开始时：洗牌种子取场景种子设好之后的随机源状态，传感器延迟和掉鳞甲计时照官方。
/// 改坏确认：`brain_shuffle` 换掉洗牌种子，`brain_sensors` 把传感器延迟各加一。
///
/// At scene start: the shuffle seed is the random state after the scene seed is set;
/// the sensor delays and scute timer follow the official side. Break tests:
/// `brain_shuffle` changes the shuffle seed, `brain_sensors` delays every sensor by one.
pub(super) fn setup(
    random_state: i64,
    mob: &mut Mob,
    sensors: &[i64],
    scute: i32,
    break_test: Option<&str>,
) {
    if let Some(brain) = mob.brain.as_mut() {
        let shuffle_break = i64::from(break_test == Some("brain_shuffle"));
        brain.seed_shuffles(random_state.wrapping_add(shuffle_break));
        let sensor_break = i64::from(break_test == Some("brain_sensors"));
        for (sensor, &delay) in brain.sensors.iter_mut().zip(sensors) {
            sensor.time_to_tick = delay + sensor_break;
        }
    }
    if scute > 0 {
        mob.armadillo.scute_time = scute;
    }
}

/// 玩家 0 在 `[起, 止)` 这几刻疾跑。
///
/// Player 0 sprints during `[start, end)`.
pub(super) fn set_sprint(players: &mut [PlayerView], sprint: Option<[i32; 2]>, tick: i32) {
    if let (Some([start, end]), Some(player)) = (sprint, players.first_mut()) {
        player.sprinting = tick >= start && tick < end;
    }
}

/// `活动|正在跑的行为|记忆`，和 Java 一侧逐字相同；不跑大脑的生物是空串。
///
/// `activities|running|memories`, character for character like the Java side; empty
/// for goal-driven mobs.
pub(super) fn dump(level: &MobLevel, entity: &Entity) -> String {
    let EntityKind::Mob(mob) = &entity.kind else {
        return String::new();
    };
    let Some(brain) = mob.brain.as_deref() else {
        return String::new();
    };
    let mut out = brain.active_names().join(",");
    out.push('|');
    out.push_str(&brain.running_names().join(","));
    out.push('|');
    let mut memories: Vec<(&str, String)> = brain
        .memories
        .filled()
        .map(|(kind, value, ttl)| {
            let mut text = value_text(level, value);
            // `Unit` 值的记忆（在水里、怀孕）Java 一侧打印类名；路径记忆也用这个变体。
            // Java prints the class name for `Unit` memories (in water, pregnant); the
            // path memory shares the variant.
            if matches!(value, MemoryValue::Unit)
                && kind != mc_bamboo_blocks::entity::mob::brain::memory::MemoryType::Path
            {
                text = "Unit".to_owned();
            }
            if let Some(ttl) = ttl {
                text.push_str(&format!("@{ttl}"));
            }
            (kind.name(), text)
        })
        .collect();
    memories.sort_by(|a, b| a.0.cmp(b.0));
    let memories: Vec<String> = memories
        .into_iter()
        .map(|(name, text)| format!("{name}={text}"))
        .collect();
    out.push_str(&memories.join(" "));
    out
}

fn pos_text(pos: BlockPos) -> String {
    format!("{};{};{}", pos.x, pos.y, pos.z)
}

fn ids_text(ids: &[i32]) -> String {
    ids.iter().map(i32::to_string).collect::<Vec<_>>().join(";")
}

fn value_text(level: &MobLevel, value: &MemoryValue) -> String {
    match value {
        MemoryValue::Uuid(uuid) => {
            let hex: String = uuid.iter().map(|byte| format!("{byte:02x}")).collect();
            format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
        }
        MemoryValue::Bool(value) => value.to_string(),
        MemoryValue::Int(value) => value.to_string(),
        MemoryValue::Long(value) => value.to_string(),
        MemoryValue::Unit => "path".to_owned(),
        MemoryValue::Entity(uid) => uid.to_string(),
        MemoryValue::WantedItem { uid, .. } => uid.to_string(),
        MemoryValue::Entities(list) => ids_text(&list[..]),
        MemoryValue::Visible(visible) => ids_text(&visible.entities[..]),
        MemoryValue::Walk(walk) => format!(
            "{}/{}/{}",
            tracker_block(level, walk.target),
            walk.close_enough,
            walk.speed.to_bits() as i32
        ),
        MemoryValue::Look(tracker) => tracker_block(level, *tracker),
        MemoryValue::Damage(source) => source.kind.name().to_owned(),
        MemoryValue::Vec3(point) => format!(
            "{};{};{}",
            point.x.to_bits() as i64,
            point.y.to_bits() as i64,
            point.z.to_bits() as i64
        ),
        MemoryValue::GlobalPos(gp) => {
            format!("{}@{}:{}:{}", gp.dimension, gp.pos.x, gp.pos.y, gp.pos.z)
        }
        MemoryValue::GlobalPosList(list) => list
            .iter()
            .map(|gp| format!("{}@{}:{}:{}", gp.dimension, gp.pos.x, gp.pos.y, gp.pos.z))
            .collect::<Vec<_>>()
            .join(";"),
        MemoryValue::BlockPos(pos) => pos_text(*pos),
    }
}

/// `PositionTracker.currentBlockPosition`，按场景里的实体和玩家算。
///
/// `PositionTracker.currentBlockPosition` against the scene's entities and players.
fn tracker_block(level: &MobLevel, tracker: PositionTracker) -> String {
    match tracker {
        PositionTracker::Block { pos, .. } => pos_text(pos),
        PositionTracker::Item { uid, .. } => {
            // 官方 tracker 仍持有已移除的 ItemEntity，BrainOracle 记成 `?`。
            // The official tracker still holds the removed ItemEntity, printed as `?`.
            match level.entities.get(uid) {
                Some(entity) if !entity.removed => pos_text(BlockPos::containing(entity.position)),
                _ => "?".to_owned(),
            }
        }
        PositionTracker::Entity {
            uid, target_eye, ..
        } => {
            let found = level
                .players
                .iter()
                .find(|player| player.uid == uid)
                .map(|player| (player.position, player.eye_height))
                .or_else(|| {
                    level.entities.get(uid).map(|entity| {
                        let eye = entity.kind.mob().map_or(0.0, |mob| mob.eye_height);
                        (entity.position, eye)
                    })
                });
            match found {
                Some((position, eye)) if target_eye => pos_text(BlockPos::containing(
                    position.add(Vec3::new(0.0, f64::from(eye), 0.0)),
                )),
                Some((position, _)) => pos_text(BlockPos::containing(position)),
                None => "?".to_owned(),
            }
        }
    }
}
