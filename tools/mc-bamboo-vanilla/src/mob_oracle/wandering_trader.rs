//! 流浪商人刷新器 / 存档逐刻对拍。
//!
//! Java 一侧注入官方 `WanderingTraderSpawner` 的私有随机源、`WanderingTraderData`
//! 和 `tickDelay`，逐刻跑 24000 刻、每 1200 刻记一次存档的两个字段；这里用
//! Bamboo 的同一个刷新器重放同一条 `LegacyRandom`，逐帧比对。存档场景用
//! 官方 `saveWithoutId` 的 SNBT 往返，按官方会写的键逐键比对。
//!
//! Wandering-trader spawner / archive oracle. The Java side injects the
//! official spawner's private RNG, saved data and `tickDelay`, runs 24000 ticks
//! and records the two saved fields every 1200 ticks; Bamboo replays the same
//! `LegacyRandom` and compares every frame. Save cases compare the official
//! `saveWithoutId` round-trip key by key.

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockTicks, World,
    entity::{
        Entities, Entity,
        mob::{MobType, nbt, wandering_trader_spawner as spawner},
    },
};
use mc_bamboo_core::Vec3;
use serde::Deserialize;

use super::{MobLevel, Report, ZombieFacts};

/// Java 一侧的刷新器场景。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpawnerCase {
    name: String,
    seed: i64,
    enabled: bool,
    frames: Vec<[i32; 2]>,
}

/// Java 一侧的存档场景。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SaveCase {
    name: String,
    llama: bool,
    nbt: String,
    reloaded: String,
}

/// 官方 `WanderingTrader.addAdditionalSaveData` 的专属键（加共用层的键）。
const TRADER_SAVE_KEYS: &[&str] = &[
    "Offers",
    "Inventory",
    "DespawnDelay",
    "wander_target",
    "VillagerData",
    "VillagerDataFinalized",
    "FoodLevel",
    "Gossips",
    "Xp",
    "LastRestock",
    "LastGossipDecay",
    "RestocksToday",
    "Brain",
];

impl Report {
    pub(super) fn replay_wandering_trader_spawner(&mut self, case: &SpawnerCase) {
        self.cases += 1;
        let mut level = MobLevel {
            blocks: std::collections::HashMap::new(),
            random: mc_bamboo_blocks::LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts::default(),
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
            wrong_was_hurt: false,
            short_hunger: false,
            ignore_bed_monsters: false,
            explosion_break: 0,
        };
        let mut ticks = BlockTicks::new();
        let mut spawner = spawner::WanderingTraderSpawner::with_random_seed(
            spawner::WanderingTraderData::default(),
            spawner::TICK_DELAY,
            case.seed,
        );
        let mut ours = Vec::new();
        for tick in 0..24_000 {
            {
                let mut world = World::new(&mut level, &mut ticks);
                // 假世界没有玩家；Bamboo 侧也传空表，`spawn` 因此恒返回 true。
                spawner.tick(&mut world, &[], case.enabled);
            }
            if tick % spawner::TICK_DELAY == spawner::TICK_DELAY - 1 {
                ours.push([spawner.data.spawn_delay, spawner.data.spawn_chance]);
            }
        }
        match self.break_test.as_deref() {
            // 三处改坏确认：spawnDelay / spawnChance / 存档键都必须被抓到。
            Some("wt_spawner_delay") => {
                if let Some(first) = ours.first_mut() {
                    first[0] += 1;
                }
            }
            Some("wt_spawner_chance") => {
                if let Some(last) = ours.last_mut() {
                    last[1] += 1;
                }
            }
            _ => {}
        }
        if ours != case.frames {
            self.mismatch(format!(
                "{} seed {}: official {:?}, bamboo {ours:?}",
                case.name, case.seed, case.frames
            ));
        }
        self.steps += ours.len();
    }

    pub(super) fn replay_wandering_trader_save(&mut self, case: &SaveCase) {
        self.cases += 1;
        let official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{}: SNBT does not parse: {error:?}", case.name));
                return;
            }
        };
        let kind = if case.llama {
            MobType::TraderLlama
        } else {
            MobType::WanderingTrader
        };
        let mut entity = Entity::by_mob_type(kind, Vec3::default());
        if let Err(error) = nbt::load_into(&mut entity, &official) {
            self.mismatch(format!("{}: load failed: {error:?}", case.name));
            return;
        }
        match self.break_test.as_deref() {
            Some("wt_save") => {
                // 改坏确认：改掉 `despawnDelay`，存档对拍应当报 `DespawnDelay` 不一致。
                entity.with_mob(|_, mob| mob.wandering_trader.despawn_delay += 1);
            }
            Some("wt_save_target") => {
                entity.with_mob(|_, mob| {
                    mob.wandering_trader.wander_target =
                        Some(mc_bamboo_core::BlockPos::new(1, 2, 3));
                });
            }
            _ => {}
        }
        let ours = match nbt::write_mob(&entity.split()) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{}: save failed: {error:?}", case.name));
                return;
            }
        };
        let reloaded = match mc_bamboo_nbt::parse_snbt(&case.reloaded) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!(
                    "{}: reloaded SNBT does not parse: {error:?}",
                    case.name
                ));
                return;
            }
        };
        for key in TRADER_SAVE_KEYS {
            let expected = field(&reloaded, key).map(canonical);
            let actual = field(&ours, key).map(canonical);
            if expected != actual {
                self.mismatch(format!(
                    "{}: {key} official {expected:?}, bamboo {actual:?}",
                    case.name
                ));
            }
        }
    }
}

/// 复合标签按名字排序，便于和官方的哈希表顺序比较。
fn canonical(tag: &mc_bamboo_nbt::Tag) -> mc_bamboo_nbt::Tag {
    use mc_bamboo_nbt::Tag;
    match tag {
        Tag::Compound(entries) => {
            let mut sorted: Vec<_> = entries
                .iter()
                .map(|(name, value)| (name.clone(), canonical(value)))
                .collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            Tag::Compound(sorted)
        }
        Tag::List(items) => Tag::List(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

fn field<'a>(tag: &'a mc_bamboo_nbt::Tag, key: &str) -> Option<&'a mc_bamboo_nbt::Tag> {
    match tag {
        mc_bamboo_nbt::Tag::Compound(entries) => entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value),
        _ => None,
    }
}
