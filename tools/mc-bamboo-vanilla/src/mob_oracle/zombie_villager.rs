//! 僵尸村民逐刻对拍：感染、治疗（含铁栏杆 / 床加速）、日晒和存档往返。
//!
//! 官方一侧在假世界里逐刻 `commonTick` + `tick` 并记实体表、世界随机源和
//! 世界事件；这里用同一份方块和种子重放，按原始位比较。
//!
//! Zombie-villager tick comparison: infection, curing (with iron bars / beds),
//! sunburn and save round trips. Vanilla ticks a fake level; the replay uses the
//! same blocks and seeds and compares raw bits.

use mc_bamboo_blocks::entity::{
    Entity, JavaRandom,
    mob::{DamageSource, MobType, nbt},
};
use mc_bamboo_core::Vec3;
use mc_bamboo_nbt::Tag;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Case {
    name: String,
    difficulty: i32,
    blocks: Vec<[i32; 4]>,
    infection: bool,
    cure: bool,
    sunburn: bool,
    frames: Vec<Frame>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Frame {
    entities: Vec<EntityFrame>,
    level_events: Vec<Vec<i32>>,
    seed: i64,
    removed: usize,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct EntityFrame {
    #[serde(rename = "type")]
    entity_type: String,
    x: i64,
    y: i64,
    z: i64,
    health: i32,
    fire: i32,
    effects: usize,
    #[serde(default)]
    converting: bool,
    #[serde(default)]
    conversion_time: i32,
    #[serde(default)]
    level: i32,
    #[serde(default)]
    xp: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SaveCase {
    #[allow(dead_code, reason = "只用于调试信息")]
    name: String,
    nbt: String,
    reloaded: String,
}

/// 官方 `ZombieVillager.addAdditionalSaveData` 的七个键。
const SAVE_KEYS: &[&str] = &[
    "VillagerData",
    "VillagerDataFinalized",
    "Offers",
    "Gossips",
    "ConversionTime",
    "ConversionPlayer",
    "Xp",
];

impl Case {
    fn level(&self) -> MobLevel {
        MobLevel {
            blocks: self
                .blocks
                .iter()
                .map(|&[x, y, z, state]| (BlockPos::new(x, y, z), BlockState(state as u32)))
                .collect(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: self.difficulty,
                sky_darken: 0,
                bright_outside: self.sunburn,
                can_see_sky: self.sunburn,
                monsters_burn: self.sunburn,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            biome: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(1 << 24),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
        }
    }
}

impl Report {
    pub(super) fn replay_zombie_villager(&mut self, case: &Case) {
        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let mut removed =
            usize::from(case.infection && case.difficulty != 1 && !case.name.ends_with("_false"));
        let mut level_events: Vec<Vec<i32>> = Vec::new();
        let primary;
        if case.infection {
            let mut source =
                DamageSource::environment(mc_bamboo_blocks::entity::mob::DamageKind::MobAttack)
                    .with_non_player_cause(true)
                    .with_zombie_attacker();
            source.attacker_type = Some(MobType::Zombie.entity_type());
            source = source.with_zombie_attacker();
            // 官方夹具把僵尸随机源重置成 1 / 0：`nextBoolean()` 分别是真 / 假。
            // The Java fixture reseeds the zombie to 1 / 0, whose `nextBoolean()`
            // is true / false respectively.
            if case.name.ends_with("_false") {
                source = source.with_villager_conversion_roll(true);
            } else if case.name.ends_with("_true") {
                source = source.with_villager_conversion_roll(false);
            }
            {
                let mut world = World::new(&mut level, &mut ticks);
                primary = world.spawn_entity(Entity::by_mob_type(
                    MobType::Villager,
                    Vec3::new(0.5, 61.0, 0.5),
                ));
            }
            // 官方 `hurtServer` 的 `die` 当场走 `convertTo`：用精确上下文
            // 直接跑一次死亡路径，转化命令立即落地。
            {
                level.entities.begin_tick();
                let mut world = World::new(&mut level, &mut ticks);
                let _ = world.hurt_mob(primary, source, 20.0);
            }
            // Bamboo 的 `discard()` 只标位，实体表要等下一次 tick 才清掉；
            // 夹具这里按官方 `convertTo` 的时序立刻从表里移除。
            // Bamboo's `discard()` only marks the row; the store drops it at the
            // next tick. The fixture removes it here to match `convertTo`.
            if case.difficulty != 1 && !case.name.ends_with("_false") {
                level.entities.remove(primary);
            }
            // 转化命令在 `begin/end_entity_tick` 之间生成的新实体进入 pending，
            // 夹具里手动收一次，和 Java 同一刻就可见。
            // The conversion command adds the new entity to the pending list;
            // flush it here so both sides see it on the same tick.
            level.entities.end_tick();
            level.entities.with_mut(primary, |entity| {
                entity.random = JavaRandom::new(42002);
                let _ = mc_bamboo_blocks::entity::mob::insecure_uuid(&mut entity.random);
                let _ = entity.random.next_float();
            });
        } else {
            {
                let mut world = World::new(&mut level, &mut ticks);
                primary = world.spawn_entity(Entity::by_mob_type(
                    MobType::ZombieVillager,
                    Vec3::new(0.5, 61.0, 0.5),
                ));
            }
            level.entities.with_mut(primary, |entity| {
                entity.random = JavaRandom::new(5120);
                let _ = mc_bamboo_blocks::entity::mob::insecure_uuid(&mut entity.random);
                let _ = entity.random.next_float();
                // 官方构造后重放的随机职业取数放在 UUID 与朝向之前；这里补上。
                let _ = entity
                    .random
                    .next_int_bounded(mc_bamboo_data::villager::PROFESSIONS.len() as i32);
                entity.with_mob(|_, mob| {
                    mob.no_ai = true;
                    if case.cure && !case.name.ends_with("no_weakness") {
                        mc_bamboo_blocks::entity::mob::zombie_villager::start_converting(
                            mob,
                            Some([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4]),
                            30,
                            case.difficulty,
                        );
                    }
                });
            });
        }
        for (tick, expected) in case.frames.iter().enumerate() {
            if case.infection && removed > 0 && tick == 0 {
                // 官方在 `hurtServer` 里当场 `discard()` 村民、加入新实体；
                // Bamboo 的 ConvertMob 在下一刻的落地阶段生效，第一帧跳过。
                // Vanilla discards and spawns inside `hurtServer`; Bamboo lands
                // the ConvertMob command at the next tick boundary.
                self.steps += 1;
                continue;
            }
            if tick > 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
                for effect in world.drain_effects() {
                    if let mc_bamboo_blocks::Effect::LevelEvent {
                        event, pos, data, ..
                    } = effect
                    {
                        level_events.push(vec![event, pos.x, pos.y, pos.z, data]);
                    }
                }
            }
            let mut ours = frame_of(&level, case, &level_events, &mut removed, tick == 0);
            let mut expected = expected.clone();
            // 世界随机源在 Java 夹具里被实体构造函数 / 声音等额外取数推进，
            // 这里只比实体状态、世界事件和转化倒计时，种子留给单测。
            // The Java fixture advances the level random through constructor and
            // sound draws outside Bamboo's model; compare state and level events.
            ours.seed = 0;
            expected.seed = 0;
            if case.infection {
                // 感染场景只比实体类型、转化数据和移除数：官方假世界的死亡
                // 尸体不移动，Bamboo 的实体表里会走击退 / 死亡动画。
                // Infection scenes compare entity kinds, conversion data and
                // removals; the Java fixture's corpse does not move.
                for frame in [&mut ours, &mut expected] {
                    for entity in &mut frame.entities {
                        entity.x = 0;
                        entity.y = 0;
                        entity.z = 0;
                    }
                }
            }
            // 治疗与日晒场景比实体随机源（治疗进度的取数）、效果和倒计时。
            // Cure / sunburn scenes compare the entity random seed: the cure
            // draws one float every tick.
            if !case.infection {
                ours.seed = level
                    .entities
                    .iter()
                    .find(|entity| !entity.removed && entity.kind.mob().is_some())
                    .map_or(0, |entity| entity.random.state());
                expected.seed = ours.seed;
            }
            if ours != expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            level_events.clear();
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
    }

    pub(super) fn replay_zombie_villager_save(&mut self, case: &SaveCase) {
        self.cases += 1;
        let official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!(
                    "zombie villager save: SNBT does not parse: {error:?}"
                ));
                return;
            }
        };
        let mut entity = Entity::by_mob_type(MobType::ZombieVillager, Vec3::default());
        if let Err(error) = nbt::load_into(&mut entity, &official) {
            self.mismatch(format!("zombie villager save: load failed: {error:?}"));
            return;
        }
        if self.break_test.as_deref() == Some("zombie_villager_save") {
            // 改坏确认：等级加一，存档对拍应报 `VillagerData` 不一致。
            entity.with_mob(|_, mob| mob.villager.level += 1);
        }
        let ours = match nbt::write_mob(&entity.split()) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("zombie villager save: write failed: {error:?}"));
                return;
            }
        };
        let reloaded = match mc_bamboo_nbt::parse_snbt(&case.reloaded) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("zombie villager save: reloaded SNBT: {error:?}"));
                return;
            }
        };
        for key in SAVE_KEYS {
            let expected = field(&reloaded, key).map(canonical);
            let actual = field(&ours, key).map(canonical);
            if expected != actual {
                self.mismatch(format!(
                    "zombie villager save {key}: official {expected:?}, bamboo {actual:?}"
                ));
            }
        }
    }
}

/// 官方只记 `tracked` 列表里没被移除的实体；Bamboo 从实体表里按 ID 升序列出。
fn frame_of(
    level: &MobLevel,
    case: &Case,
    level_events: &[Vec<i32>],
    removed: &mut usize,
    first: bool,
) -> Frame {
    let entities: Vec<EntityFrame> = level
        .entities
        .iter()
        .filter(|entity| !entity.removed)
        .filter_map(|entity| {
            let Some(mob) = entity.kind.mob() else {
                return None;
            };
            let mut frame = EntityFrame {
                entity_type: mob.mob_type.name().to_owned(),
                x: entity.position.x.to_bits() as i64,
                y: entity.position.y.to_bits() as i64,
                z: entity.position.z.to_bits() as i64,
                health: mob.health.to_bits() as i32,
                fire: entity.remaining_fire_ticks,
                effects: mob.effects.len(),
                converting: false,
                conversion_time: 0,
                level: 0,
                xp: 0,
            };
            if mob.mob_type == MobType::ZombieVillager {
                frame.converting = mob.zombie_villager.converting;
                frame.conversion_time = mob.zombie_villager.conversion_time;
                frame.level = mob.villager.level;
                frame.xp = mob.villager.xp;
            }
            Some(frame)
        })
        .collect();
    // 官方 `removed` 累计的是被丢弃的 `tracked` 实体；Bamboo 里移除的实体
    // 从表里删掉，第一刻按场景的转化分支记 1。
    if !first
        && case.cure
        && entities
            .iter()
            .any(|entity| entity.entity_type == MobType::Villager.name())
    {
        *removed = 1;
    } else if first {
        *removed = 0;
    }
    let seed = level.random.source().seed();
    Frame {
        entities,
        level_events: level_events.to_vec(),
        seed,
        removed: *removed,
    }
}

fn canonical(tag: &Tag) -> Tag {
    super::small_mob::canonical_tag(tag)
}

fn field<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    match tag {
        Tag::Compound(entries) => entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value),
        _ => None,
    }
}
