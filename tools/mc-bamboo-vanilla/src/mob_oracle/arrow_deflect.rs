//! 箭射旋风人被弹回逐刻对拍：一支固定起点 / 速度的箭朝静止的旋风人飞去，逐刻比
//! 位置、速度、朝向、是否消失、旋风人血量（不该受伤）、箭的随机源、世界随机源和
//! 这一刻的 `BREEZE_DEFLECT` 音效（在旋风人身上）。
//!
//! Tick-by-tick comparison of an arrow flying into a static breeze and being
//! deflected: position, velocity, rotation, removal, the breeze's health (must not
//! drop), the arrow random, the level random and the per-tick `BREEZE_DEFLECT` sound.

use mc_bamboo_blocks::{
    BlockState, BlockTicks, LevelRandom, World,
    entity::{Entity, EntityKind, JavaRandom, mob::MobType},
};
use mc_bamboo_core::{BlockPos, Vec3};
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ArrowDeflectCase {
    pub(super) name: String,
    pub(super) first_id: i32,
    pub(super) start: [f64; 3],
    pub(super) velocity: [f64; 3],
    pub(super) frames: Vec<ArrowDeflectFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ArrowDeflectFrame {
    pub(super) x: i64,
    pub(super) y: i64,
    pub(super) z: i64,
    pub(super) dx: i64,
    pub(super) dy: i64,
    pub(super) dz: i64,
    pub(super) y_rot: i64,
    pub(super) x_rot: i64,
    pub(super) removed: i64,
    pub(super) breeze_health: i64,
    pub(super) arrow_seed: i64,
    pub(super) level_seed: i64,
    pub(super) sounds: Vec<[i64; 7]>,
}

impl ArrowDeflectCase {
    fn level(&self) -> MobLevel {
        let mut blocks = Vec::new();
        for x in -4..=4 {
            for z in -4..=4 {
                blocks.push((
                    BlockPos::new(x, 63, z),
                    BlockState(mc_bamboo_data::blocks::STONE),
                ));
            }
        }
        MobLevel {
            blocks: blocks.into_iter().collect(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: 2,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            biome: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(self.first_id),
            players: Vec::new(),
            game: None,
            wrong_was_hurt: false,
            inhabited: HashMap::new(),
            mob_teams: HashMap::new(),
            short_hunger: false,
            ignore_bed_monsters: false,
            explosion_break: 0,
            containers: Default::default(),
            container_open_counts: Default::default(),
            shoulder: mc_bamboo_blocks::entity::shoulder::ShoulderState::empty(),
            raid: None,
        }
    }
}

impl Report {
    pub(super) fn replay_arrow_deflect(&mut self, case: &ArrowDeflectCase) {
        self.cases += 1;
        let broken = self.break_test.as_deref() == Some("arrow_deflect");
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let velocity = Vec3::new(case.velocity[0], case.velocity[1], case.velocity[2]);
        let start = Vec3::new(case.start[0], case.start[1], case.start[2]);
        let (arrow_id, breeze_id) = {
            let mut world = World::new(&mut level, &mut ticks);
            let breeze = Entity::by_mob_type(MobType::Breeze, Vec3::new(3.5, 64.0, 0.5));
            let breeze_id = world.spawn_entity(breeze);
            let mut arrow = Entity::arrow(start);
            arrow.delta = velocity;
            let arrow_id = world.spawn_entity(arrow);
            // 官方场景在生成后显式把箭的随机源设成 `RandomSource.create(11)`，
            // 再把速度 / 朝向钉死；这里照做（`spawn_entity` 里 UUID 取数用的是旧源）。
            world.with_entity_mut(arrow_id, &mut |entity| {
                entity.random = JavaRandom::new(11);
                entity.y_rot = 0.0;
                if let EntityKind::Arrow(arrow) = &mut entity.kind {
                    arrow.pitch = 0.0;
                }
            });
            (arrow_id, breeze_id)
        };
        let mut last_arrow: Option<Entity> = None;
        for (tick, expected) in case.frames.iter().enumerate() {
            let mut sounds: Vec<[i64; 7]> = Vec::new();
            if tick > 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                if let Some(kept) = world.tick_one_entity_kept(arrow_id) {
                    last_arrow = Some(kept);
                }
                let effects = world.drain_effects();
                sounds.extend(effects.iter().filter_map(|effect| {
                    let mc_bamboo_blocks::Effect::Sound {
                        sound,
                        source,
                        x,
                        y,
                        z,
                        volume,
                        pitch,
                        ..
                    } = effect
                    else {
                        return None;
                    };
                    Some([
                        i64::from(*sound),
                        i64::from(source.id()),
                        x.to_bits() as i64,
                        y.to_bits() as i64,
                        z.to_bits() as i64,
                        i64::from(volume.to_bits()),
                        i64::from(pitch.to_bits()),
                    ])
                }));
            }
            let entity = level.entities.get(arrow_id).or(last_arrow.clone());
            let (position, delta, yaw, pitch, removed, arrow_seed) = match &entity {
                Some(entity) => {
                    let pitch = match &entity.kind {
                        EntityKind::Arrow(arrow) => arrow.pitch,
                        _ => 0.0,
                    };
                    (
                        entity.position,
                        entity.delta,
                        entity.y_rot,
                        pitch,
                        i64::from(entity.removed),
                        entity.random.state(),
                    )
                }
                None => (Vec3::default(), Vec3::default(), 0.0, 0.0, 1, 0),
            };
            let breeze_health = level
                .entities
                .get(breeze_id)
                .and_then(|entity| entity.kind.mob().map(|mob| mob.health))
                .map_or(0, |health| health.to_bits() as i32 as i64);
            let level_seed = level.random.source().seed();

            let mut frame = ArrowDeflectFrame {
                x: position.x.to_bits() as i64,
                y: position.y.to_bits() as i64,
                z: position.z.to_bits() as i64,
                dx: delta.x.to_bits() as i64,
                dy: delta.y.to_bits() as i64,
                dz: delta.z.to_bits() as i64,
                y_rot: i64::from(yaw.to_bits() as i32),
                x_rot: i64::from(pitch.to_bits() as i32),
                removed,
                breeze_health,
                arrow_seed,
                level_seed,
                sounds,
            };
            if broken {
                frame.dx = frame.dx.wrapping_add(1);
            }
            if frame.x != expected.x
                || frame.y != expected.y
                || frame.z != expected.z
                || frame.dx != expected.dx
                || frame.dy != expected.dy
                || frame.dz != expected.dz
                || frame.y_rot != expected.y_rot
                || frame.x_rot != expected.x_rot
                || frame.removed != expected.removed
                || frame.breeze_health != expected.breeze_health
                || frame.arrow_seed != expected.arrow_seed
                || frame.level_seed != expected.level_seed
                || frame.sounds != expected.sounds
            {
                self.mismatch(format!(
                    "{} tick {tick}: official x{} y{} z{} dx{} dy{} dz{} yRot{} xRot{} removed{} health{} arrowSeed{} levelSeed{} sounds{:?}, bamboo x{} y{} z{} dx{} dy{} dz{} yRot{} xRot{} removed{} health{} arrowSeed{} levelSeed{} sounds{:?}",
                    case.name,
                    expected.x, expected.y, expected.z, expected.dx, expected.dy, expected.dz,
                    expected.y_rot, expected.x_rot, expected.removed, expected.breeze_health,
                    expected.arrow_seed, expected.level_seed, expected.sounds,
                    frame.x, frame.y, frame.z, frame.dx, frame.dy, frame.dz,
                    frame.y_rot, frame.x_rot, frame.removed, frame.breeze_health,
                    frame.arrow_seed, frame.level_seed, frame.sounds,
                ));
            }
            self.steps += 1;
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }
}
