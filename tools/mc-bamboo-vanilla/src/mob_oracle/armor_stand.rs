//! 盔甲架对拍：官方假世界里跑 `ArmorStand` 的存档往返与逐刻行为，Bamboo 用同一场景重放。
//!
//! Armour stand parity: the official fake level runs `ArmorStand` save round trips
//! and per-tick behaviour; Bamboo replays the same scenes. Each per-tick scene emits
//! a compact state array per tick; the meaning of the array is fixed per scene.

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, LevelRandom,
    entity::{
        Entities, Entity, EntityKind, JavaRandom,
        mob::{DamageSource, MobType, interact::MobInteractRequest, nbt},
    },
};
use mc_bamboo_core::{BlockPos, Hand, Vec3};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};
use serde::Deserialize;

use super::*;

const FIRST_ID: i32 = 1 << 24;
const START_TIME: i64 = 1000;
const ENTITY_RANDOM_SEED: i64 = 0x0DDB_1A5E_5BAD_5EED;

/// 一题存档往返：官方 `saveWithoutId` 的整份 SNBT 和读回后再写出的整份 SNBT。
///
/// One save case: vanilla's full `saveWithoutId` SNBT and the reloaded copy's SNBT.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ArmorStandSaveCase {
    name: String,
    nbt: String,
    reloaded: String,
}

/// 一个逐刻场景：`name` 决定状态数组的含义，`steps` 是官方夹具的动作脚本，
/// `frames[tick]` 是官方第 `tick` 刻的状态。
///
/// A per-tick scene: `name` fixes the meaning of the state array, `steps` is the
/// vanilla fixture's action script and `frames[tick]` is vanilla's state at that tick.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ArmorStandCase {
    name: String,
    #[serde(default)]
    steps: Vec<Vec<i64>>,
    frames: Vec<Vec<i64>>,
}

/// `saveWithoutId` 不带 `id`，而 Bamboo 的 `write_mob` 带：这是两种入口的格式差异，
/// 不是实体状态，整棵比较时略过。其余键全部逐项比对。
///
/// `saveWithoutId` omits `id` while Bamboo's `write_mob` writes it: an entry-point
/// format difference, not entity state. Every other key is compared.
const SKIP_KEYS: &[&str] = &["id"];

impl Report {
    /// 官方存档 → `load_into` → `write_mob`，再把整棵 NBT 和官方读回后写出的 NBT 逐键比。
    ///
    /// Official save → `load_into` → `write_mob`, then compare the whole NBT tree
    /// key by key against vanilla's reload-and-save output.
    pub(super) fn replay_armor_stand_save(&mut self, case: &ArmorStandSaveCase) {
        mc_bamboo_blocks::entity::mob::armor_stand::set_oracle_tamper(
            self.break_test.as_deref().unwrap_or(""),
        );
        self.cases += 1;
        self.steps += 1;
        let label = case.name.as_str();
        let official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: SNBT does not parse: {error:?}"));
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
        let mut entity = Entity::by_mob_type(MobType::ArmorStand, Vec3::default());
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
        compare_nbt(self, label, &reloaded, &ours);
    }

    /// 逐刻场景分派。
    pub(super) fn replay_armor_stand_scene(&mut self, case: &ArmorStandCase) {
        mc_bamboo_blocks::entity::mob::armor_stand::set_oracle_tamper(
            self.break_test.as_deref().unwrap_or(""),
        );
        self.cases += 1;
        match case.name.as_str() {
            "armor_stand_fall" => self.replay_armor_stand_fall(case),
            "armor_stand_equip" => self.replay_armor_stand_equip(case),
            "armor_stand_break" => self.replay_armor_stand_break(case),
            "armor_stand_arrow" => self.replay_armor_stand_arrow(case),
            name => self.mismatch(format!("unknown armour stand scene {name}")),
        }
    }

    /// 放置后掉落：盔甲架从 (0.5, 66, 0.5) 落到 y=63 的石头地面，逐刻比
    /// `[removed, px, py, pz, dx, dy, dz, on_ground]`（浮点取原始位）。
    ///
    /// Fall after placement: a stand drops from (0.5, 66, 0.5) onto a stone floor
    /// at y=63, comparing `[removed, position bits, motion bits, on_ground]` each tick.
    fn replay_armor_stand_fall(&mut self, case: &ArmorStandCase) {
        let mut level = scene_level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(START_TIME);
        let id = spawn_stand(&mut level, &mut ticks, Vec3::new(0.5, 66.0, 0.5), false);
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                ticks.set_time(START_TIME + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
            }
            let ours = fall_frame(&level, id);
            if &ours != expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            self.steps += 1;
        }
    }

    /// 穿戴 / 取下装备：按 `steps` 逐刻做一次交互，比
    /// `[consumes, heldItem, heldCount, 八个槽的 item/count]`。
    ///
    /// Equip / unequip: one interaction per tick driven by `steps`, comparing
    /// `[consumes, held item, held count, eight equipment slots]`.
    fn replay_armor_stand_equip(&mut self, case: &ArmorStandCase) {
        let mut level = scene_level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(START_TIME);
        let id = spawn_stand(&mut level, &mut ticks, Vec3::new(0.5, 64.0, 0.5), true);
        let mut player_held = (0_i32, 0_i32);
        for (tick, expected) in case.frames.iter().enumerate() {
            let mut consumes = false;
            if tick > 0 {
                ticks.set_time(START_TIME + tick as i64);
                if let Some(step) = case.steps.iter().find(|s| s[0] == tick as i64) {
                    let held = if step[1] == 0 {
                        ItemStack::empty()
                    } else {
                        ItemStack::new(step[1] as i32, 1)
                    };
                    // 官方夹具每刻都先把手里那一堆设成步骤给的，再跑交互。
                    // The vanilla fixture sets the held stack from the step before each interaction.
                    player_held = (step[1] as i32, if step[1] == 0 { 0 } else { 1 });
                    let request = equip_request(held, step[2] as f64 / 10.0);
                    let mut world = World::new(&mut level, &mut ticks);
                    let outcome = world.interact_mob(id, &request);
                    consumes = outcome.result.consumes_action();
                    if let Some(replacement) = outcome.replacement {
                        player_held = (replacement.item, replacement.count);
                    } else if outcome.shrink_held {
                        player_held = (0, 0);
                    }
                }
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
            }
            let ours = equip_frame(&level, id, consumes, player_held);
            if &ours != expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            self.steps += 1;
        }
    }

    /// 两下打碎：tick 1 / 3 各打一下，比 `[removed, lastHit, dropCount, drops*3]`。
    ///
    /// Two-hit break: one hit at tick 1 and 3, comparing
    /// `[removed, lastHit, drop count, sorted drops]`.
    fn replay_armor_stand_break(&mut self, case: &ArmorStandCase) {
        let mut level = scene_level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(START_TIME);
        let id = spawn_stand(&mut level, &mut ticks, Vec3::new(0.5, 64.0, 0.5), true);
        {
            let mut world = World::new(&mut level, &mut ticks);
            world.with_entity_mut(id, &mut |entity| {
                entity.with_mob(|_, mob| {
                    mob.custom_name = Some(Tag::String("Hall Monitor".to_owned()));
                    mob.equipment.set(
                        EquipmentSlot::Head,
                        ItemStack::new(mc_bamboo_data::items::DIAMOND_HELMET, 1),
                    );
                    mob.equipment.set(
                        EquipmentSlot::Chest,
                        ItemStack::new(mc_bamboo_data::items::IRON_CHESTPLATE, 1),
                    );
                });
            });
        }
        let mut last_hit = 0;
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                ticks.set_time(START_TIME + tick as i64);
                if tick == 1 || tick == 3 {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.hurt_mob(
                        id,
                        DamageSource::player(FIRST_ID + 1, Vec3::new(2.5, 64.0, 0.5), false),
                        1.0,
                    );
                }
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
            }
            if let Some(entity) = level.entities.get(id) {
                last_hit = entity.kind.mob().map_or(0, |mob| mob.armor_stand.last_hit);
            }
            let ours = break_frame(&level, id, last_hit);
            if &ours != expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            self.steps += 1;
        }
    }

    /// 被箭射到：tick 1 / 2 各一箭（箭伤害不破坏也不摇晃），比 `[removed, lastHit, healthBits]`。
    ///
    /// Hit by arrows: one arrow at tick 1 and 2 (arrows neither break nor wobble a
    /// stand), comparing `[removed, lastHit, health bits]`.
    fn replay_armor_stand_arrow(&mut self, case: &ArmorStandCase) {
        let mut level = scene_level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(START_TIME);
        let id = spawn_stand(&mut level, &mut ticks, Vec3::new(0.5, 64.0, 0.5), true);
        let mut health_bits = 0;
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                ticks.set_time(START_TIME + tick as i64);
                if tick == 1 || tick == 2 {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.hurt_mob(
                        id,
                        DamageSource::environment(mc_bamboo_blocks::entity::mob::DamageKind::Arrow),
                        1.0,
                    );
                }
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
            }
            if let Some(entity) = level.entities.get(id) {
                health_bits = entity
                    .kind
                    .mob()
                    .map_or(0, |mob| mob.health.to_bits() as i32);
            }
            let ours = arrow_frame(&level, id, health_bits);
            if &ours != expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            self.steps += 1;
        }
    }
}

/// 在假世界里放好盔甲架并对齐官方夹具的初值；`pinned` 时开手臂、无重力。
fn spawn_stand(level: &mut MobLevel, ticks: &mut BlockTicks, start: Vec3, pinned: bool) -> i32 {
    let mut entity = Entity::by_mob_type(MobType::ArmorStand, start);
    entity.random = JavaRandom::new(ENTITY_RANDOM_SEED);
    let mut world = World::new(level, ticks);
    let id = world.spawn_entity(entity);
    world.with_entity_mut(id, &mut |entity| {
        entity.y_rot = 0.0;
        entity.old_position = start;
        entity.random = JavaRandom::new(ENTITY_RANDOM_SEED);
        entity.with_mob(|_, mob| {
            mob.uuid = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3];
            mob.x_rot = 0.0;
            mob.y_head_rot = 0.0;
            mob.y_body_rot = 0.0;
            mob.y_rot_o = 0.0;
            mob.x_rot_o = 0.0;
            if pinned {
                mob.no_gravity = true;
                mob.armor_stand.set_show_arms(true);
            }
        });
    });
    id
}

/// 装备场景的一次交互请求。
fn equip_request(held: ItemStack, click_y: f64) -> MobInteractRequest {
    MobInteractRequest {
        player_uid: FIRST_ID + 1,
        player_uuid: ((1_u128 << 64) | 2).to_be_bytes(),
        hand: Hand::Main,
        held,
        creative: false,
        position: Vec3::new(2.5, 64.0, 0.5),
        player_height: 1.8,
        sneaking: false,
        difficulty: 2,
        hit_pos: Some(Vec3::new(0.0, click_y, 0.0)),
        spectator: false,
    }
}

/// 逐刻场景共用的假世界：y=63 的石头地面，无玩家。
fn scene_level() -> MobLevel {
    let stone = BlockState(mc_bamboo_data::blocks::STONE);
    let mut blocks = HashMap::new();
    for x in -4..=4 {
        for z in -4..=4 {
            blocks.insert(BlockPos::new(x, 63, z), stone);
        }
    }
    MobLevel {
        raid: None,
        blocks,
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
        shoulder: Default::default(),
        entities: Entities::with_next_id(FIRST_ID),
        players: Vec::new(),
        game: None,
        inhabited: HashMap::new(),
        mob_teams: HashMap::new(),
        wrong_was_hurt: false,
        short_hunger: false,
        ignore_bed_monsters: false,
        containers: Default::default(),
        container_open_counts: Default::default(),
        explosion_break: 0,
    }
}

/// 掉落场景的一帧。
fn fall_frame(level: &MobLevel, id: i32) -> Vec<i64> {
    match level.entities.get(id) {
        Some(entity) => vec![
            0,
            entity.position.x.to_bits() as i64,
            entity.position.y.to_bits() as i64,
            entity.position.z.to_bits() as i64,
            entity.delta.x.to_bits() as i64,
            entity.delta.y.to_bits() as i64,
            entity.delta.z.to_bits() as i64,
            i64::from(entity.on_ground),
        ],
        None => vec![1, 0, 0, 0, 0, 0, 0, 0],
    }
}

/// 装备场景的一帧：`[consumes, heldItem, heldCount, 八槽的 item/count]`。
fn equip_frame(level: &MobLevel, id: i32, consumes: bool, player_held: (i32, i32)) -> Vec<i64> {
    let mut out = vec![
        i64::from(consumes),
        i64::from(player_held.0),
        i64::from(player_held.1),
    ];
    let Some(entity) = level.entities.get(id) else {
        out.extend([0; 16]);
        return out;
    };
    let Some(mob) = entity.kind.mob() else {
        out.extend([0; 16]);
        return out;
    };
    for slot in [
        EquipmentSlot::MainHand,
        EquipmentSlot::OffHand,
        EquipmentSlot::Feet,
        EquipmentSlot::Legs,
        EquipmentSlot::Chest,
        EquipmentSlot::Head,
        EquipmentSlot::Body,
        EquipmentSlot::Saddle,
    ] {
        let stack = mob.equipment.get(slot);
        out.push(i64::from(stack.item));
        out.push(i64::from(stack.count));
    }
    out
}

/// 打碎场景的一帧：`[removed, lastHit, dropCount, 排序后的 (item,count,customName)]`。
/// 实体移除后 `lastHit` 由调用方缓存传入（官方保留破坏前的值）。
fn break_frame(level: &MobLevel, id: i32, last_hit: i64) -> Vec<i64> {
    let removed = i64::from(level.entities.get(id).is_none());
    let mut drops: Vec<[i64; 3]> = level
        .entities
        .iter()
        .filter(|entity| !entity.removed)
        .filter_map(|entity| match &entity.kind {
            EntityKind::Item(item) => {
                let stack = &item.stack;
                let custom_name = stack.added.iter().any(|(kind, _)| {
                    *kind == mc_bamboo_data::registries::data_component_type::CUSTOM_NAME
                });
                Some([
                    i64::from(stack.item),
                    i64::from(stack.count),
                    i64::from(custom_name),
                ])
            }
            _ => None,
        })
        .collect();
    drops.sort_unstable();
    let mut out = vec![removed, last_hit, drops.len() as i64];
    for drop in drops {
        out.extend(drop);
    }
    out
}

/// 箭场景的一帧：`[removed, lastHit, healthBits]`。实体移除后血量由调用方缓存传入。
fn arrow_frame(level: &MobLevel, id: i32, health_bits: i32) -> Vec<i64> {
    match level.entities.get(id) {
        Some(entity) => vec![
            0,
            entity.kind.mob().map_or(0, |mob| mob.armor_stand.last_hit),
            i64::from(health_bits),
        ],
        None => vec![1, 0, i64::from(health_bits)],
    }
}

/// 两棵复合标签按键名并集逐项比较，复合键顺序按 `canonical_tag` 规范化。
///
/// Compare two compounds over the union of their keys, normalising compound key
/// order with `canonical_tag`.
fn compare_nbt(report: &mut Report, label: &str, expected: &Tag, actual: &Tag) {
    let (Tag::Compound(e), Tag::Compound(a)) = (expected, actual) else {
        report.mismatch(format!("{label}: NBT is not a compound"));
        return;
    };
    let mut keys: Vec<&str> = e
        .iter()
        .map(|(name, _)| name.as_str())
        .chain(a.iter().map(|(name, _)| name.as_str()))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    for key in keys {
        if SKIP_KEYS.contains(&key) {
            continue;
        }
        let want = field(e, key).map(super::small_mob::canonical_tag);
        let got = field(a, key).map(super::small_mob::canonical_tag);
        if want != got {
            report.mismatch(format!("{label} {key}: official {want:?}, bamboo {got:?}"));
        }
    }
}

fn field<'a>(entries: &'a [(String, Tag)], key: &str) -> Option<&'a Tag> {
    entries
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}
