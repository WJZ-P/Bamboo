//! `mob-oracle` 的袭击逐刻回放：用正式的 [`crate::raid::tick`] 驱动一份
//! `Region`，每刻导出一份和官方 `Raids` / `Raid` 字段对齐的快照。场景夹具
//! 在 Java 侧，见 `tools/mc-bamboo-vanilla/java/MobOracle.java`。
//!
//! Raid tick-by-tick replay for `mob-oracle`: runs the production
//! [`crate::raid::tick`] over a `Region` and exports a snapshot aligned with
//! the official `Raids` / `Raid` fields each tick. The Java-side fixture lives
//! in `tools/mc-bamboo-vanilla/java/MobOracle.java`.

use mc_bamboo_blocks::entity::raid::{RaidStatus, Raids};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
use uuid::Uuid;

use crate::{inbound::PlayerId, player::Player, region::Region, world_state::Difficulty};

/// 一个回放场景的初始条件。
///
/// Initial conditions of one replay scene.
#[derive(Debug, Clone)]
pub struct ReplayRaidScene {
    pub difficulty: i32,
    pub omen_level: i32,
    pub raids_rule: bool,
    pub seed: i64,
    pub ticks: i32,
    pub spawn_x: i32,
    pub spawn_y: i32,
    pub spawn_z: i32,
    pub victory: bool,
    /// 起始已刷波数（`raid_wave7_hard` 设为 6，测困难第 7 波骑手）；0 表示首波开始。
    pub start_wave: i32,
    /// 起始波间冷却覆盖；空表示用默认 300 刻。
    pub start_cooldown: Option<i32>,
}

/// 一名袭击者的比较行：实体类型 ID、x*2、y、z*2、载具实体类型 ID（未骑乘为 -1）。
///
/// One spawned raider's comparison row: entity type id, x*2, y, z*2, vehicle type id (-1 if none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayRaidSpawn {
    pub entity_type: i32,
    pub x2: i32,
    pub y: i32,
    pub z2: i32,
    pub vehicle_type: i32,
}

/// 官方 `Raid` / `Raids` 的逐刻快照，字段名和 Java 侧 `raidStateJson` 对齐。
///
/// Per-tick snapshot of official `Raid` / `Raids`, with field names matching
/// the Java side's `raidStateJson`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayRaidState {
    pub started: bool,
    pub active: bool,
    pub ticks_active: i64,
    pub raid_omen_level: i32,
    pub groups_spawned: i32,
    pub cooldown_ticks: i32,
    pub post_raid_ticks: i32,
    pub total_health_bits: i32,
    pub group_count: i32,
    pub status: &'static str,
    pub center_x: i32,
    pub center_y: i32,
    pub center_z: i32,
    pub raiders_alive: i32,
    pub heroes: i32,
    pub next_id: i32,
    pub raid_tick: i32,
    pub map_size: i32,
}

/// 一刻的回放结果。
///
/// One tick of replay output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayRaidStep {
    pub state: ReplayRaidState,
    pub spawned: Vec<ReplayRaidSpawn>,
}

fn status_name(status: RaidStatus) -> &'static str {
    match status {
        RaidStatus::Ongoing => "ONGOING",
        RaidStatus::Victory => "VICTORY",
        RaidStatus::Loss => "LOSS",
        RaidStatus::Stopped => "STOPPED",
    }
}

fn snapshot(raids: &Raids, id: i32, omen_level: i32) -> ReplayRaidState {
    let Some(raid) = raids.raids.get(&id) else {
        return ReplayRaidState {
            started: false,
            active: true,
            ticks_active: 0,
            raid_omen_level: omen_level,
            groups_spawned: 0,
            cooldown_ticks: 300,
            post_raid_ticks: 0,
            total_health_bits: 0,
            group_count: 0,
            status: "ONGOING",
            center_x: 0,
            center_y: 0,
            center_z: 0,
            raiders_alive: 0,
            heroes: 0,
            next_id: raids.next_id,
            raid_tick: raids.tick,
            map_size: 0,
        };
    };
    ReplayRaidState {
        started: raid.started,
        // FakeLevel 的 hasChunkAt 恒为真；这里按官方场景恒真。
        active: true,
        ticks_active: raid.ticks_active,
        raid_omen_level: omen_level,
        groups_spawned: raid.groups_spawned,
        cooldown_ticks: raid.cooldown_ticks,
        post_raid_ticks: raid.post_raid_ticks,
        total_health_bits: raid.total_health.to_bits() as i32,
        group_count: raid.group_count,
        status: status_name(raid.status),
        center_x: raid.center.x,
        center_y: raid.center.y,
        center_z: raid.center.z,
        raiders_alive: raid.total_raiders(),
        heroes: raid.heroes_of_the_village.len() as i32,
        next_id: raids.next_id,
        raid_tick: raids.tick,
        map_size: raids.raids.len() as i32,
    }
}

/// 跑一个场景，逐刻返回状态快照和这一刻刷出的袭击者。
///
/// Runs one scene, returning the state snapshot and raiders spawned each tick.
pub fn replay_raid(scene: &ReplayRaidScene) -> Vec<ReplayRaidStep> {
    let mut region = Region::new(2, 20, false);
    let mut global = crate::global::Global::default();
    global.state_mut().difficulty = match scene.difficulty {
        1 => Difficulty::Easy,
        3 => Difficulty::Hard,
        _ => Difficulty::Normal,
    };
    let raids_index = mc_bamboo_data::gamerules::find("minecraft:raids").unwrap();
    global.rules_mut().set_saved(
        raids_index,
        crate::gamerules::Stored::Bool(scene.raids_rule),
    );
    global.publish(&mut region);
    let id = PlayerId(1);
    let player = Player::new(
        id,
        Uuid::from_u128(1),
        "RaidOracle".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        Vec3::new(0.5, -60.0, 0.5),
    );
    region.insert_player(player);
    let bell = BlockPos::new(0, -62, 0);
    let meeting = mc_bamboo_data::poi::poi_type_id("meeting").unwrap();
    region.poi.add(bell, meeting);
    let _ = region.poi.take(|_| true, |_, _| true, bell, 8);

    let mut raids = Raids::new();
    let raid_id = raids.unique_id();
    let mut raid = mc_bamboo_blocks::entity::raid::Raid::new(bell, scene.difficulty);
    raid.rng_state = (scene.seed as u64 ^ 0x5DEE_CE66D) & ((1u64 << 48) - 1);
    raid.raid_omen_level = scene.omen_level;
    raid.started = scene.victory || scene.start_wave > 0;
    raid.groups_spawned = if scene.victory {
        raid.group_count
    } else {
        scene.start_wave
    };
    if let Some(cd) = scene.start_cooldown {
        raid.cooldown_ticks = cd;
    }
    let spawn_at = BlockPos::new(scene.spawn_x, scene.spawn_y, scene.spawn_z);
    raid.wave_spawn_pos = Some(spawn_at);
    raids.raids.insert(raid_id, raid);
    region.raids = raids;

    let mut out = Vec::with_capacity(scene.ticks.max(0) as usize);
    for _ in 0..scene.ticks.max(0) {
        // 和 Java 夹具一样：每刻把 waveSpawnPos 重新预设，绕开
        // findRandomSpawnPos 的地形 / 高度图差异。
        if let Some(raid) = region.raids.raids.get_mut(&raid_id) {
            raid.wave_spawn_pos = Some(spawn_at);
        }
        let before: std::collections::BTreeSet<i32> = region
            .entities
            .iter_public()
            .map(|public| public.uid)
            .collect();
        crate::raid::tick(&mut region);
        let mut spawned = Vec::new();
        for public in region.entities.iter_public() {
            if before.contains(&public.uid) {
                continue;
            }
            if let mc_bamboo_blocks::entity::KindPublic::Mob(mob) = &public.kind {
                let vehicle_type = public
                    .vehicle
                    .and_then(|v| region.entities.public_at(v))
                    .and_then(|vp| match &vp.kind {
                        mc_bamboo_blocks::entity::KindPublic::Mob(vm) => {
                            Some(vm.mob_type.entity_type())
                        }
                        _ => None,
                    })
                    .unwrap_or(-1);
                spawned.push(ReplayRaidSpawn {
                    entity_type: mob.mob_type.entity_type(),
                    x2: (public.position.x * 2.0) as i32,
                    y: public.position.y as i32,
                    z2: (public.position.z * 2.0) as i32,
                    vehicle_type,
                });
            }
        }
        out.push(ReplayRaidStep {
            state: snapshot(&region.raids, raid_id, scene.omen_level),
            spawned,
        });
    }
    out
}
