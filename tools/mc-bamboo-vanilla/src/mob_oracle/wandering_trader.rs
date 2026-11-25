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

use std::collections::HashMap;

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, LevelRandom, World,
    entity::{
        Entities, Entity, JavaRandom,
        mob::{MobType, nbt, wandering_trader_spawner as spawner},
    },
};
use mc_bamboo_core::{BlockPos, Vec3};
use serde::Deserialize;

use super::{MobLevel, Report, ZombieFacts};
use mc_bamboo_blocks::entity::mob::wandering_trader::Drink;

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
            containers: Default::default(),
            container_open_counts: Default::default(),
            shoulder: Default::default(),
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

/// 流浪商人逐刻场景（白天/夜里饮水、游走、躲僵尸、交易、消失）。
/// Java 一侧直接在假世界放商人和两只拴着的羊驼，逐刻比位置、效果、
/// 饮水状态、目标表、实体随机源、`despawnDelay`、存在与否和交易表摘要。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SceneCase {
    name: String,
    trader_id: i32,
    ticks: i32,
    frames: Vec<SceneFrame>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
struct SceneFrame {
    trader: EntityFrame,
    trader_gone: bool,
    despawn: i32,
    drink: String,
    bright: bool,
    goals: String,
    offers: String,
    llama0: EntityFrame,
    llama0_gone: bool,
    llama0_delay: i32,
    llama1: EntityFrame,
    llama1_gone: bool,
    llama1_delay: i32,
    #[serde(default)]
    zombie: Option<EntityFrame>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
struct EntityFrame {
    /// `[x, y, z, yRot, xRot]`（前 3 个 f64 位，后 2 个 f32 位）。
    pos: [i64; 5],
    #[serde(default)]
    seed: Option<i64>,
    #[serde(default)]
    effects: Vec<[i32; 3]>,
    #[serde(default)]
    invisible: bool,
}

fn entity_frame(entity: &mc_bamboo_blocks::entity::Entity) -> Option<EntityFrame> {
    let mob = entity.kind.mob()?;
    let mut effects: Vec<[i32; 3]> = mob
        .effects
        .entries()
        .iter()
        .map(|e| [e.effect, e.duration, e.amplifier])
        .collect();
    effects.sort_by_key(|e| e[0]);
    let mut pos = [0_i64; 5];
    pos[0] = entity.position.x.to_bits() as i64;
    pos[1] = entity.position.y.to_bits() as i64;
    pos[2] = entity.position.z.to_bits() as i64;
    pos[3] = i64::from(entity.y_rot.to_bits() as i32);
    pos[4] = i64::from(mob.x_rot.to_bits() as i32);
    Some(EntityFrame {
        pos,
        seed: Some(entity.random.state()),
        effects,
        invisible: mob
            .effects
            .has(mc_bamboo_data::registries::mob_effect::INVISIBILITY),
    })
}

impl SceneCase {
    fn level(&self) -> MobLevel {
        let mut blocks = HashMap::new();
        for x in -16..=16 {
            for z in -16..=16 {
                blocks.insert(
                    BlockPos::new(x, 63, z),
                    BlockState(mc_bamboo_data::blocks::STONE),
                );
            }
        }
        MobLevel {
            blocks,
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: 2,
                bright_outside: !self.name.starts_with("wt_drink_night"),
                sky_darken: 0,
                sea_level: 63,
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
            entities: Entities::with_next_id(self.trader_id),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            explosion_break: 0,
            containers: Default::default(),
            container_open_counts: Default::default(),
            shoulder: Default::default(),
        }
    }

    /// 按 Java 夹具的 UID / 随机源 / 初始状态建三只实体，返回羊驼 uid。
    fn setup(&self, world: &mut mc_bamboo_blocks::World<'_>) -> [i32; 2] {
        let mut ids = [0; 2];
        let trader_pos = Vec3::new(0.5, 64.0, 0.5);
        let trader_id =
            world.spawn_entity(Entity::by_mob_type(MobType::WanderingTrader, trader_pos));
        world.with_entity_mut(trader_id, &mut |e| {
            e.random = JavaRandom::new(71);
            e.y_rot = 0.0;
            if let Some(mob) = e.kind.mob_mut() {
                mob.uuid = 1_u128.to_be_bytes();
                mob.wandering_trader.despawn_delay =
                    if self.name == "wt_despawn" { 40 } else { 48000 };
                if self.name == "wt_wander" {
                    mob.wandering_trader.wander_target = Some(BlockPos::new(8, 64, 0));
                }
                mob.no_ai = self.name == "wt_despawn";
                if self.name == "wt_drink_day" {
                    add_invisibility(mob);
                    // 官方夹具在 tick 0 之前 `addEffect` + `updateDataBeforeSync`，
                    // 共享标志位已经是真，牛奶的谓词从第一刻就命中。
                    mob.wandering_trader.invisible = true;
                }
                // 官方夹具构造时调了 `getOffers()`（三张集共 9 笔）。这里只比数量，
                // 用 9 个空的 `MerchantOffer` 占位，实际内容由 trade-oracle 逐项对拍。
                mob.villager.offers = Some(vec![
                    mc_bamboo_protocol::merchant::MerchantOffer {
                        base_cost_a: mc_bamboo_protocol::merchant::ItemCost {
                            item: mc_bamboo_data::items::EMERALD,
                            count: 1,
                            components: Vec::new(),
                        },
                        cost_b: None,
                        result: mc_bamboo_protocol::item::ItemStack::new(
                            mc_bamboo_data::items::APPLE,
                            1
                        ),
                        uses: 0,
                        max_uses: 12,
                        reward_exp: true,
                        special_price_diff: 0,
                        demand: 0,
                        price_multiplier: 0.05,
                        xp: 0,
                    };
                    9
                ]);
            }
        });
        for (i, slot) in ids.iter_mut().enumerate() {
            let i = i as i64;
            let pos = Vec3::new(2.5 + i as f64, 64.0, 0.5);
            *slot = world.spawn_entity(Entity::by_mob_type(MobType::TraderLlama, pos));
            let uid = *slot;
            world.with_entity_mut(uid, &mut |e| {
                e.random = JavaRandom::new(100 + i);
                e.y_rot = 0.0;
                if let Some(mob) = e.kind.mob_mut() {
                    mob.uuid = (2 + i as u128).to_be_bytes();
                    mob.no_ai = true;
                    mc_bamboo_blocks::entity::mob::leash::set_leashed_to(
                        mob,
                        trader_id,
                        mc_bamboo_blocks::entity::mob::leash::LeashDelayed::Entity(
                            1_u128.to_be_bytes(),
                        ),
                    );
                }
            });
        }
        ids
    }
}

/// `INVISIBILITY` 效果：官方 `MobEffectInstance(MobEffects.INVISIBILITY, 3600)`。
fn add_invisibility(mob: &mut mc_bamboo_blocks::entity::mob::Mob) {
    mc_bamboo_blocks::entity::mob::living::add_effect(
        mob,
        mc_bamboo_blocks::entity::mob::effects::EffectInstance::new(
            mc_bamboo_data::registries::mob_effect::INVISIBILITY,
            3600,
        ),
    );
}

impl Report {
    pub(super) fn replay_wandering_trader_scene(&mut self, case: &SceneCase) {
        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let llamas = case.setup(&mut mc_bamboo_blocks::World::new(&mut level, &mut ticks));
        let _ = level.entities.take_mob_events();
        let _ = level.entities.take_spawn_snapshots();

        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                // 夜里 / 白天切换：`MobLevel` 的注入天色。
                if case.name == "wt_drink_night" && tick == 1 {
                    level.facts.bright_outside = false;
                }
                if case.name == "wt_drink_day" && tick == 40 {
                    level.facts.bright_outside = true;
                }
                ticks.set_time(1000 + tick as i64);
                let mut world = mc_bamboo_blocks::World::new(&mut level, &mut ticks);
                world.tick_entities();
                world.drain_effects();
            }
            let _ = level.entities.take_mob_events();
            let trader = level.entities.get(case.trader_id);
            let gone = trader.as_ref().is_none_or(|e| e.removed);
            let Some(frame) = trader.as_ref().and_then(entity_frame) else {
                self.mismatch(format!("{} tick {tick}: trader row missing", case.name));
                continue;
            };
            let mob = trader.as_ref().and_then(|e| e.kind.mob());
            let drink = drink_label(mob);
            let goals = mob.map_or(String::new(), |m| m.goals.running_names_of(m).join(" "));
            let offers = offers_label(mob);
            let despawn = mob.map_or(0, |m| m.wandering_trader.despawn_delay);
            let llama_frames: Vec<(EntityFrame, bool, i32)> = llamas
                .iter()
                .map(|&uid| {
                    let e = level.entities.get(uid);
                    let removed = e.as_ref().is_none_or(|e| e.removed);
                    let frame = e
                        .as_ref()
                        .and_then(entity_frame)
                        .unwrap_or_else(|| EntityFrame {
                            pos: [0; 5],
                            seed: None,
                            effects: Vec::new(),
                            invisible: false,
                        });
                    let delay = e
                        .as_ref()
                        .and_then(|e| e.kind.mob())
                        .map_or(0, |m| m.trader_llama.despawn_delay);
                    let mut frame = frame;
                    frame.seed = None;
                    (frame, removed, delay)
                })
                .collect();
            let ours = SceneFrame {
                trader: frame,
                trader_gone: gone,
                despawn,
                drink,
                bright: level.facts.bright_outside,
                goals,
                offers,
                llama0: llama_frames[0].0.clone(),
                llama0_gone: llama_frames[0].1,
                llama0_delay: llama_frames[0].2,
                llama1: llama_frames[1].0.clone(),
                llama1_gone: llama_frames[1].1,
                llama1_delay: llama_frames[1].2,
                zombie: None,
            };
            // 两处改坏确认：故意改掉倒计时 / 效果，对拍必须报不一致。
            let mut compared = ours;
            match self.break_test.as_deref() {
                Some("wt_despawn_delay") => compared.despawn += 1,
                Some("wt_invisibility") => compared.trader.effects.clear(),
                _ => {}
            }
            if &compared != expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?} bamboo {compared:?}",
                    case.name
                ));
            }
            self.steps += 1;
        }
        println!(
            "{}: {} ticks (case ticks {})",
            case.name,
            case.frames.len(),
            case.ticks
        );
    }
}

fn drink_label(mob: Option<&mc_bamboo_blocks::entity::mob::Mob>) -> String {
    let Some(mob) = mob else {
        return "none".to_owned();
    };
    match mob.wandering_trader.drink {
        Drink::None => "none".to_owned(),
        Drink::Invisibility { remaining } => {
            format!("invisibility:{remaining}")
        }
        Drink::Milk { remaining } => format!("milk:{remaining}"),
    }
}

fn offers_label(mob: Option<&mc_bamboo_blocks::entity::mob::Mob>) -> String {
    let offers = mob
        .and_then(|m| m.villager.offers.as_ref())
        .map_or(0, Vec::len);
    format!("[{offers},0,0,0]")
}
