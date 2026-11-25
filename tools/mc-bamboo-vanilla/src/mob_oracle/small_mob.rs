//! 蠹虫和末影螨的逐刻对拍：位置、速度、朝向、随机源、血量、寿命、唤醒倒计时、
//! 方块表、新生成的实体和世界随机源。
//!
//! Silverfish and endermite tick comparison: motion, random, health, lifetime, the
//! wake-up countdown, the block table, freshly spawned entities and the level random.

use mc_bamboo_blocks::entity::{KindPublic, mob::MobType};

use super::{
    squid::{cow_attack, take_hurt_marked},
    *,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SmallMobCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    start: [f64; 3],
    silverfish: bool,
    melee: bool,
    persistent: bool,
    enderman: bool,
    frames: Vec<SmallFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SmallFrame {
    gone: bool,
    #[serde(flatten)]
    state: Option<SmallState>,
    /// 开局方块表里每一格当前的状态号（顺序和 blocks 一样）。
    blocks: Vec<i32>,
    spawned: Vec<String>,
    level_seed: i64,
    /// 这个夹具不比包（生物自己的配对包在两侧由不同机制产生）。
    #[allow(dead_code, reason = "只记录，不比较")]
    packets: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct SmallState {
    motion: [i64; 8],
    seed: i64,
    /// 血量、`hurtTime`、`deathTime`、`noActionTime`、`yBodyRot`、`yHeadRot`、`zza`、着地。
    state: [i32; 8],
    /// 末影螨的 `Lifetime`；蠹虫恒 0。
    life: i32,
    /// 蠹虫的 `WakeUpFriendsGoal.lookForFriends`；末影螨恒 0。
    friends: i32,
    /// 末影人的目标：0 没有，1 是末影螨，2 是别的。
    target: i32,
}

impl SmallMobCase {
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
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
        }
    }
}

fn small_state(entity: &Entity) -> SmallState {
    let mob = entity.kind.mob().expect("a small mob is a mob");
    let (p, v) = (entity.position, entity.delta);
    let friends = mob
        .goals
        .goals
        .iter()
        .find_map(|wrapped| match &wrapped.goal {
            mc_bamboo_blocks::entity::mob::goals::Goal::SilverfishWakeFriends {
                look_for_friends,
            } => Some(*look_for_friends),
            _ => None,
        })
        .unwrap_or(0);
    SmallState {
        motion: [
            p.x.to_bits() as i64,
            p.y.to_bits() as i64,
            p.z.to_bits() as i64,
            v.x.to_bits() as i64,
            v.y.to_bits() as i64,
            v.z.to_bits() as i64,
            i64::from(entity.y_rot.to_bits() as i32),
            i64::from(mob.x_rot.to_bits() as i32),
        ],
        seed: entity.random.state(),
        state: [
            mob.health.to_bits() as i32,
            mob.hurt_time,
            mob.death_time,
            mob.no_action_time,
            mob.y_body_rot.to_bits() as i32,
            mob.y_head_rot.to_bits() as i32,
            mob.zza.to_bits() as i32,
            i32::from(entity.on_ground),
        ],
        life: mob.endermite.life,
        friends,
        target: 0,
    }
}

impl Report {
    pub(super) fn replay_small_mob(&mut self, case: &SmallMobCase) {
        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let kind = if case.silverfish {
            MobType::Silverfish
        } else {
            MobType::Endermite
        };
        let [x, y, z] = case.start;
        let mut world = World::new(&mut level, &mut ticks);
        let id = world.spawn_entity(Entity::by_mob_type(kind, Vec3::new(x, y, z)));
        if case.persistent {
            world.with_entity_mut(id, &mut |entity| {
                entity.with_mob(|_, mob| mob.persistence_required = true);
            });
        }
        let cow = case.melee.then(|| {
            let mut cow = Entity::by_mob_type(MobType::Cow, Vec3::new(3.5, 61.0, 0.5));
            cow.with_mob(|_, mob| mob.no_ai = true);
            world.spawn_entity(cow)
        });
        let enderman = case.enderman.then(|| {
            world.spawn_entity(Entity::by_mob_type(
                MobType::Enderman,
                Vec3::new(4.5, 61.0, 0.5),
            ))
        });
        // 官方夹具的第 0 帧不含刚构造的实体：把开局快照丢掉。
        // The official fixture's first frame does not include the constructed mob.
        let _ = level.entities.take_spawn_snapshots();
        let mut tracking = EntityTracking::default();
        let sound_source = mc_bamboo_blocks::LegacyRandom::new(12345);
        let break_test = self.break_test.clone();
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                // 改坏确认：受伤晚一刻发生，唤醒同伴的整条时间线都要错开。
                let hurt_tick = 5 + i32::from(break_test.as_deref() == Some("small_mob_hurt"));
                let melee_source = (case.melee && tick == 60)
                    .then(|| cow.map(|cow| cow_attack(&level, cow)))
                    .flatten();
                let mut world = World::new(&mut level, &mut ticks);
                if case.name == "silverfish_wake" && tick == hurt_tick as usize {
                    world.hurt_mob(
                        id,
                        DamageSource::environment(mc_bamboo_blocks::entity::mob::DamageKind::Magic),
                        1.0,
                    );
                }
                if let Some(source) = melee_source {
                    world.hurt_mob(id, source, 20.0);
                }
                world.tick_entities();
            }
            // 两侧都不比包：把取数消费掉，保持事件队列为空。
            let events = level.entities.take_mob_events();
            let _ = event_packets(&events, || sound_source.next_long());
            let public = level.entities.public_at(id).cloned();
            let mut spawned = Vec::new();
            for fresh in level.entities.take_spawn_snapshots() {
                spawned.push(
                    mc_bamboo_data::registries::entity_type::name_of(fresh.kind.entity_type())
                        .unwrap_or("minecraft:unknown")
                        .to_owned(),
                );
            }
            if let Some(public) = &public
                && tick != 0
            {
                let snapshot = Snapshot::of(public);
                let hurt_marked = take_hurt_marked(&mut level, &mut ticks, id);
                let _ = tracking.tracked_for_oracle(&snapshot).step(
                    &snapshot,
                    snapshot.needs_sync,
                    hurt_marked,
                );
            }
            let label = format!("{} tick {tick}", case.name);
            if public.is_none() != expected.gone {
                self.mismatch(format!(
                    "{label} gone: official {}, bamboo {}",
                    expected.gone,
                    public.is_none()
                ));
            }
            let mut ours = level.entities.get(id).map(|entity| small_state(&entity));
            if let (Some(state), Some(enderman)) = (&mut ours, enderman) {
                let target = level
                    .entities
                    .get(enderman)
                    .and_then(|entity| entity.kind.mob().map(|mob| mob.target))
                    .unwrap_or(None);
                state.target = match target {
                    None => 0,
                    Some(uid) if uid == id => 1,
                    Some(_) => 2,
                };
            }
            if let Some(state) = &mut ours
                && break_test.as_deref() == Some("small_mob_life")
            {
                // 改坏确认：寿命多算一刻，逐刻比较必须抓到。
                state.life = state.life.saturating_add(1);
            }
            if ours != expected.state {
                self.mismatch(format!(
                    "{label} state: official {:?}, bamboo {ours:?}",
                    expected.state
                ));
            }
            let mut blocks = Vec::new();
            for &[x, y, z, _] in &case.blocks {
                let world = World::new(&mut level, &mut ticks);
                blocks.push(world.block_state(BlockPos::new(x, y, z)).0 as i32);
            }
            if blocks != expected.blocks {
                self.mismatch(format!(
                    "{label} blocks: official {:?}, bamboo {blocks:?}",
                    expected.blocks
                ));
            }
            if spawned != expected.spawned {
                self.mismatch(format!(
                    "{label} spawned: official {:?}, bamboo {spawned:?}",
                    expected.spawned
                ));
            }
            let level_seed = level.random.source().seed();
            if level_seed != expected.level_seed {
                self.mismatch(format!(
                    "{label} level random: official {}, bamboo {level_seed}",
                    expected.level_seed
                ));
            }
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        println!(
            "{}: {} ticks, {} items dropped",
            case.name,
            case.frames.len(),
            level
                .entities
                .take_spawn_snapshots()
                .iter()
                .filter(|entity| matches!(entity.kind, KindPublic::Item { .. }))
                .count()
        );
    }
}

/// 存档往返：官方 `saveWithoutId` 的 SNBT 和读回再写出的 SNBT。
///
/// Save round trip: vanilla's `saveWithoutId` SNBT and the reloaded copy.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SmallMobSaveCase {
    silverfish: bool,
    life: i32,
    nbt: String,
    reloaded: String,
}

/// 死亡掉落：两种生物的战利品表都是空的，比较移除状态和物品列表。
///
/// Death drops: both loot tables are empty; compare the removal flag and item list.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SmallMobDeathCase {
    silverfish: bool,
    by_player: bool,
    removed: bool,
    items: Vec<String>,
}

/// 复合标签的键按名字排序，便于和官方的哈希表顺序比较。
pub(super) fn canonical_tag(tag: &mc_bamboo_nbt::Tag) -> mc_bamboo_nbt::Tag {
    use mc_bamboo_nbt::Tag;
    match tag {
        Tag::Compound(entries) => {
            let mut sorted: Vec<_> = entries
                .iter()
                .map(|(name, value)| (name.clone(), canonical_tag(value)))
                .collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            Tag::Compound(sorted)
        }
        Tag::List(items) => Tag::List(items.iter().map(canonical_tag).collect()),
        other => other.clone(),
    }
}

pub(super) fn save_field<'a>(
    tag: &'a mc_bamboo_nbt::Tag,
    key: &str,
) -> Option<&'a mc_bamboo_nbt::Tag> {
    match tag {
        mc_bamboo_nbt::Tag::Compound(entries) => entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value),
        _ => None,
    }
}

/// 这两种生物自己要存的关键：`Lifetime`（末影螨）和共通的位置 / 朝向 / 血量 /
/// 持久化标记。别的共通键由 `nbt` 模块既有的对拍覆盖。
const SAVE_KEYS: &[&str] = &[
    "Lifetime",
    "PersistenceRequired",
    "Health",
    "Pos",
    "Rotation",
    "Invulnerable",
    "Motion",
];

impl Report {
    pub(super) fn replay_small_mob_save(&mut self, case: &SmallMobSaveCase) {
        use mc_bamboo_blocks::entity::mob::nbt;
        self.cases += 1;
        let kind = if case.silverfish {
            MobType::Silverfish
        } else {
            MobType::Endermite
        };
        let label = format!("{kind:?} save life {}", case.life);
        let official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: SNBT does not parse: {error:?}"));
                return;
            }
        };
        let mut entity = Entity::by_mob_type(kind, Vec3::default());
        if let Err(error) = nbt::load_into(&mut entity, &official) {
            self.mismatch(format!("{label}: load failed: {error:?}"));
            return;
        }
        let ours = match nbt::write_mob(&entity.split()) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: save failed: {error:?}"));
                return;
            }
        };
        let reloaded = match mc_bamboo_nbt::parse_snbt(&case.reloaded) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: reloaded SNBT does not parse: {error:?}"));
                return;
            }
        };
        for key in SAVE_KEYS {
            let expected = save_field(&reloaded, key).map(canonical_tag);
            let actual = save_field(&ours, key).map(canonical_tag);
            if expected != actual {
                self.mismatch(format!(
                    "{label} {key}: official {expected:?}, bamboo {actual:?}"
                ));
            }
        }
    }

    pub(super) fn replay_small_mob_death(&mut self, case: &SmallMobDeathCase) {
        self.cases += 1;
        let kind = if case.silverfish {
            MobType::Silverfish
        } else {
            MobType::Endermite
        };
        let mut level = MobLevel {
            blocks: std::collections::HashMap::new(),
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
            entities: Entities::with_next_id(1 << 24),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
        };
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let mut source =
            DamageSource::environment(mc_bamboo_blocks::entity::mob::DamageKind::Generic);
        if case.by_player {
            source =
                DamageSource::environment(mc_bamboo_blocks::entity::mob::DamageKind::PlayerAttack);
            source.attacker_is_player = true;
        }
        let id;
        {
            let mut world = World::new(&mut level, &mut ticks);
            id = world.spawn_entity(Entity::by_mob_type(kind, Vec3::new(0.5, 64.0, 0.5)));
            world.hurt_mob(id, source, 100.0);
        }
        for tick in 0..25 {
            ticks.set_time(1001 + tick);
            let mut world = World::new(&mut level, &mut ticks);
            world.tick_entities();
        }
        let removed = level.entities.public_at(id).is_none();
        if removed != case.removed {
            self.mismatch(format!(
                "{kind:?} death by_player {}: removed official {}, bamboo {removed}",
                case.by_player, case.removed
            ));
        }
        let items: Vec<String> = level
            .entities
            .iter()
            .filter_map(|entity| {
                entity
                    .ground()
                    .map(|ground| format!("minecraft:itemx{}", ground.stack.count))
            })
            .collect();
        if items != case.items {
            self.mismatch(format!(
                "{kind:?} death by_player {}: items official {:?}, bamboo {items:?}",
                case.by_player, case.items
            ));
        }
    }
}
