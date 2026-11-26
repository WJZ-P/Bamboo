//! 悦灵逐刻对拍：Bamboo 一侧在假世界里真跑大脑、飞行移动、振动分发、交互和存档，
//! 逐刻比官方输出的大脑、坐标 / 速度 / 朝向、随机源、跳舞、复制冷却、背包 / 手持、
//! 玩家手持数量、世界里的悦灵和掉落物数量。

use mc_bamboo_blocks::{
    entity::{
        Entity, EntityUid, JavaRandom,
        mob::{MobType, allay, interact::MobInteractRequest},
    },
    game_event::{GameEvent, dispatch::EventContext},
};
use mc_bamboo_core::{BlockPos, Hand, Vec3};
use mc_bamboo_data::{blocks, items};
use mc_bamboo_protocol::item::ItemStack;
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AllayCase {
    name: String,
    first_id: i32,
    start_time: i64,
    mob_seed: i64,
    #[allow(dead_code, reason = "场景起始种子只用于官方一侧的夹具说明")]
    level_seed: i64,
    has_player: bool,
    frames: Vec<AllayFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AllayFrame {
    brain: String,
    motion: [i64; 8],
    dancing: bool,
    can_duplicate: bool,
    cooldown: i64,
    held: i32,
    inventory: i32,
    player_held: i32,
    allays: i32,
    items: i32,
    mob_seed: i64,
    #[allow(dead_code, reason = "场景起始种子只用于官方一侧的夹具说明")]
    level_seed: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AllaySaveCase {
    filled: bool,
    nbt: String,
    reloaded: String,
}

const SAVE_KEYS: &[&str] = &[
    "Inventory",
    "listener",
    "DuplicationCooldown",
    "Health",
    "Pos",
    "Motion",
    "Rotation",
    "UUID",
];

/// 官方场景固定的事件时刻和位置；Java 与 Rust 两侧共用这张表。
struct Script {
    note: Option<(i32, BlockPos)>,
    jukebox_start: Option<(i32, BlockPos)>,
    jukebox_stop: Option<(i32, BlockPos)>,
    interact: Option<(i32, i32)>,
    drop: Option<(i32, Vec3, i32)>,
    /// 悦灵旁边的一头静止牛（`allay_look_cow`）：验证 `SetEntityLookTargetSometimes`
    /// 的「任意活体」变体会把 `LOOK_TARGET` 指向牛而不是只有玩家。
    cow: Option<Vec3>,
}

fn script(name: &str) -> Script {
    let mut out = Script {
        note: None,
        jukebox_start: None,
        jukebox_stop: None,
        interact: None,
        drop: None,
        cow: None,
    };
    match name {
        "allay_give_player" => {
            out.interact = Some((1, 0));
            out.drop = Some((30, Vec3::new(6.0, 64.2, 0.0), 2));
        }
        "allay_note_block" => {
            out.note = Some((20, BlockPos::new(4, 64, 0)));
            out.interact = Some((1, 0));
            out.drop = Some((60, Vec3::new(6.0, 64.2, 0.0), 3));
        }
        "allay_dance" => {
            out.jukebox_start = Some((10, BlockPos::new(2, 64, 0)));
            out.jukebox_stop = Some((90, BlockPos::new(2, 64, 0)));
            out.interact = Some((1, 0));
        }
        "allay_duplicate" => {
            out.jukebox_start = Some((10, BlockPos::new(2, 64, 0)));
            out.interact = Some((30, 1));
        }
        "allay_look_cow" => {
            out.cow = Some(Vec3::new(1.5, 64.0, 0.5));
        }
        _ => {}
    }
    out
}

fn level(case: &AllayCase, script: &Script) -> MobLevel {
    let stone = BlockState(mc_bamboo_data::blocks::STONE);
    let mut blocks = HashMap::new();
    for x in -15..=15 {
        for z in -15..=15 {
            blocks.insert(BlockPos::new(x, 63, z), stone);
        }
    }
    if let Some((_, pos)) = script.note {
        blocks.insert(pos, BlockState(blocks::NOTE_BLOCK));
    }
    if let Some((_, pos)) = script.jukebox_start {
        blocks.insert(pos, BlockState(blocks::JUKEBOX));
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
        entities: Entities::with_next_id(case.first_id),
        players: if case.has_player {
            vec![player_view(case.first_id + 1, 0)]
        } else {
            Vec::new()
        },
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

fn player_view(uid: i32, held: i32) -> mc_bamboo_blocks::entity::ctx::PlayerView {
    use mc_bamboo_blocks::entity::ctx::PlayerView;
    PlayerView {
        uid,
        uuid: Some(2_u128.to_be_bytes()),
        position: Vec3::new(2.5, 64.0, 0.5),
        delta: Vec3::default(),
        width: 0.6,
        height: 1.8,
        eye_height: 1.62,
        health: 20.0,
        look_direction: Vec3::view_vector(0.0, 0.0),
        gaze_disguise: false,
        alive: true,
        spectator: false,
        game_mode: 0,
        mining_fatigue: None,
        creative: false,
        flying: false,
        explosion_knockback_resistance: 0.0,
        sneaking: false,
        sprinting: false,
        swimming: false,
        invisible: false,
        armor_cover: 0.0,
        main_hand: held,
        off_hand: 0,
        team: None,
        combat: Default::default(),
        effects: Default::default(),
        shoulder_left: None,
        shoulder_right: None,
        yaw: 0.0,
        pitch: 0.0,
    }
}

/// 场景开始时的悦灵：与官方夹具同位置、随机源、UUID，传感器延迟定成 1，洗牌种子固定。
/// 先走 `spawn_entity` 让动态振动监听者登记进监听者表（官方 `setRandom` 之前构造时
/// 就登记了），再把构造后被 `prepare` 重取的随机源 / 朝向 / UUID / 大脑洗牌覆盖成
/// 官方 `setRandom(allay, 5)` 之后的终态。
fn spawn_allay(world: &mut mc_bamboo_blocks::World<'_>, case: &AllayCase) -> i32 {
    let entity = Entity::by_mob_type(MobType::Allay, Vec3::new(0.5, 64.5, 0.5));
    let id = world.spawn_entity(entity);
    world.with_entity_mut(id, &mut |entity| {
        entity.random = JavaRandom::from_state(case.mob_seed);
        entity.y_rot = 0.0;
        entity.with_mob(|_, mob| {
            mob.uuid = 1_u128.to_be_bytes();
            mob.x_rot = 0.0;
            mob.y_head_rot = 0.0;
            mob.y_body_rot = 0.0;
            mob.y_rot_o = 0.0;
            if let Some(brain) = mob.brain.as_mut() {
                for sensor in &mut brain.sensors {
                    sensor.time_to_tick = 1;
                }
                brain.seed_shuffles(case.mob_seed);
            }
        });
    });
    id
}

fn center(pos: BlockPos) -> Vec3 {
    Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    )
}

fn motion_of(entity: &Entity) -> [i64; 8] {
    let xr = entity
        .kind
        .mob()
        .map_or(0, |mob| mob.x_rot.to_bits() as i32);
    [
        entity.position.x.to_bits() as i64,
        entity.position.y.to_bits() as i64,
        entity.position.z.to_bits() as i64,
        entity.delta.x.to_bits() as i64,
        entity.delta.y.to_bits() as i64,
        entity.delta.z.to_bits() as i64,
        i64::from(entity.y_rot.to_bits() as i32),
        i64::from(xr),
    ]
}

impl Report {
    pub(super) fn replay_allay(&mut self, case: &AllayCase) {
        self.cases += 1;
        let label = case.name.as_str();
        let script = script(label);
        let broken = self.break_test.clone();
        allay::set_oracle_tamper(broken.as_deref().unwrap_or(""));
        let mut level = level(case, &script);
        let mut ticks = BlockTicks::default();
        ticks.set_time(case.start_time);
        let allay_id = {
            let mut world = World::new(&mut level, &mut ticks);
            spawn_allay(&mut world, case)
        };
        // `allay_look_cow`：悦灵旁边放一头静止的牛（官方 `world.tracked.add`），
        // 无玩家；悦灵的 `SetEntityLookTargetSometimes`（任意活体）应把 `LOOK_TARGET`
        // 指向牛。牛 NoAI、随机源固定，和 Java 一侧 `setRandom(cow, 7)` 一致。
        if let Some(pos) = script.cow {
            let mut world = World::new(&mut level, &mut ticks);
            let id = world.spawn_entity(Entity::by_mob_type(MobType::Cow, pos));
            world.with_entity_mut(id, &mut |entity| {
                entity.random = JavaRandom::new(7);
                entity.y_rot = 0.0;
                entity.old_position = pos;
                entity.with_mob(|_, mob| {
                    mob.uuid = 2_u128.to_be_bytes();
                    mob.x_rot = 0.0;
                    mob.y_head_rot = 0.0;
                    mob.y_body_rot = 0.0;
                    mob.y_rot_o = 0.0;
                    mob.x_rot_o = 0.0;
                    mob.no_ai = true;
                });
            });
        }
        // 悦灵自己在官方夹具里是 `world.tracked.add`（不是 `addFreshEntity`），
        // 清掉 `spawn_entity` 留下的生成快照，后面才不会被当成 spawned 排除。
        let _ = level.entities.take_spawn_snapshots();
        // 官方 `playerHeld` 无玩家时是 -1（`player == null ? -1 : ...`）。
        let mut player_held = if case.has_player { 0 } else { -1 };
        let mut level_seed;
        let mut player_main_hand = 0;
        // 官方夹具里 `addFreshEntity`（投出的物品、复制的悦灵）进 `spawned` 而不是
        // `tracked`，`items` / `allays` 只数 `tracked`；Bamboo 一侧用生成快照区分。
        let mut spawned: std::collections::HashSet<EntityUid> = std::collections::HashSet::new();
        for (index, expected) in case.frames.iter().enumerate() {
            let tick = index as i32;
            if tick > 0 {
                ticks.set_time(case.start_time + i64::from(tick));
                // 掉落物按官方夹具固定成 `FIRST_ID + 10 + tick`，直接原样进表，
                // 不走 `spawn_entity`（那会重新分配 ID）。
                if let Some((at, pos, count)) = script.drop
                    && at == tick
                {
                    let mut drop = Entity::ground_item_with_delta(
                        pos,
                        ItemStack::new(items::WHEAT_SEEDS, count),
                        Vec3::default(),
                    );
                    drop.id = case.first_id + 10 + tick;
                    level.entities.insert_row_preserving(drop.split());
                }
                let mut world = World::new(&mut level, &mut ticks);
                if let Some((at, pos)) = script.note
                    && at == tick
                {
                    world.game_event(
                        GameEvent::NoteBlockPlay,
                        center(pos),
                        EventContext::default(),
                    );
                }
                if let Some((at, pos)) = script.jukebox_start
                    && at == tick
                {
                    world.game_event(GameEvent::JukeboxPlay, center(pos), EventContext::default());
                }
                if let Some((at, pos)) = script.jukebox_stop
                    && at == tick
                {
                    world.game_event(
                        GameEvent::JukeboxStopPlay,
                        center(pos),
                        EventContext::default(),
                    );
                }
                if let Some((at, which)) = script.interact
                    && at == tick
                {
                    let held = if which == 0 {
                        items::WHEAT_SEEDS
                    } else {
                        items::AMETHYST_SHARD
                    };
                    let request = MobInteractRequest {
                        player_uid: case.first_id + 1,
                        player_uuid: 2_u128.to_be_bytes(),
                        hand: Hand::Main,
                        held: ItemStack::new(held, 4),
                        creative: false,
                        position: Vec3::new(2.5, 64.0, 0.5),
                        player_height: 1.8,
                        sneaking: false,
                        difficulty: 2,
                    };
                    let outcome = world.interact_mob(allay_id, &request);
                    if outcome.shrink_held {
                        player_held = 3;
                    }
                    if let Some(stack) = outcome.replacement {
                        player_held = stack.count;
                    }
                    player_main_hand = held;
                    // 交互里 `addFreshEntity` 出来的（复制的悦灵）照官方只进 `spawned`，
                    // 不参与本 tick 的推进、传感器也看不到；在 `tick_entities` 之前就
                    // 从表里整个拿掉，否则它会被当成可见活体、挤进
                    // `NEAREST_VISIBLE_LIVING_ENTITIES`（而且 uid 会和玩家撞号）。
                    // Entities `addFreshEntity`-ed during the interaction (the duplicate
                    // allay) only reach `spawned` in vanilla, are not ticked this tick and
                    // are invisible to sensors; remove them from the table entirely before
                    // `tick_entities` so they do not sneak into
                    // `NEAREST_VISIBLE_LIVING_ENTITIES` (and their uid would collide with
                    // the player's).
                    let fresh = world
                        .entities_mut()
                        .entities_table_mut()
                        .take_spawn_snapshots();
                    for fresh in fresh {
                        spawned.insert(fresh.uid);
                        world.entities_mut().remove_entity(fresh.uid);
                    }
                }
                world.tick_entities();
            }
            // 这一拍 `addFreshEntity` 出来的实体（投出的物品 / 复制的悦灵）记成 spawned，
            // 并且当场丢弃：官方假世界的 `addFreshEntity` 只进 `spawned` 不进 `tracked`，
            // 所以它们不参与 tick、传感器也看不到；`count_items` / `count_allays` 也不数。
            let fresh_spawned = level.entities.take_spawn_snapshots();
            if !fresh_spawned.is_empty() {
                let mut world = World::new(&mut level, &mut ticks);
                for fresh in &fresh_spawned {
                    spawned.insert(fresh.uid);
                    world.with_entity_mut(fresh.uid, &mut |entity| entity.discard());
                }
            }
            if case.has_player {
                level.players = vec![player_view(case.first_id + 1, player_main_hand)];
            }
            level_seed = level.random.source().seed();
            let entity = {
                let world = World::new(&mut level, &mut ticks);
                world.entity(allay_id)
            };
            let Some(entity) = entity else {
                self.mismatch(format!("{label} tick {tick}: allay missing"));
                return;
            };
            let Some(mob) = entity.kind.mob() else {
                self.mismatch(format!("{label} tick {tick}: allay is not a mob"));
                return;
            };
            let brain = super::brain::dump(&level, &entity);
            let state = (
                motion_of(&entity),
                mob.allay.dancing,
                mob.allay.can_duplicate,
                mob.allay.duplication_cooldown,
                mob.equipment
                    .get(mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand)
                    .count,
                mob.allay.inventory.first().map_or(0, |stack| stack.count),
                player_held,
                count_allays(&level, &spawned),
                count_items(&level, &spawned),
                entity.random.state(),
                level_seed,
            );
            let want = (
                expected.motion,
                expected.dancing,
                expected.can_duplicate,
                expected.cooldown,
                expected.held,
                expected.inventory,
                expected.player_held,
                expected.allays,
                expected.items,
                expected.mob_seed,
                expected.level_seed,
            );
            // 改坏确认：让送物品的目标不看音符盒（只认玩家）。
            let brain_want =
                if broken.as_deref() == Some("allay_no_noteblock") && label == "allay_note_block" {
                    expected
                        .brain
                        .replace("liked_noteblock=minecraft:overworld@4:64:0", "")
                } else {
                    expected.brain.clone()
                };
            if brain != brain_want {
                self.mismatch(format!(
                    "{label} tick {tick} brain: official {brain_want}, bamboo {brain}"
                ));
            }
            if state != want {
                self.mismatch(format!(
                    "{label} tick {tick} state: official {want:?}, bamboo {state:?}"
                ));
            }
        }
    }

    pub(super) fn replay_allay_save(&mut self, case: &AllaySaveCase) {
        use mc_bamboo_blocks::entity::mob::nbt;
        self.cases += 1;
        let label = format!("allay save filled {}", case.filled);
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
        let mut entity = Entity::by_mob_type(MobType::Allay, Vec3::default());
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
        for key in SAVE_KEYS {
            let expected =
                super::small_mob::save_field(&reloaded, key).map(super::small_mob::canonical_tag);
            let actual =
                super::small_mob::save_field(&ours, key).map(super::small_mob::canonical_tag);
            if expected != actual {
                self.mismatch(format!(
                    "{label} {key}: official {expected:?}, bamboo {actual:?}"
                ));
            }
        }
    }
}

fn count_allays(level: &MobLevel, spawned: &std::collections::HashSet<EntityUid>) -> i32 {
    level
        .entities
        .iter()
        .filter(|entity| {
            !entity.removed
                && !spawned.contains(&entity.id)
                && matches!(&entity.kind, mc_bamboo_blocks::entity::EntityKind::Mob(mob) if mob.mob_type == MobType::Allay)
        })
        .count() as i32
}

fn count_items(level: &MobLevel, spawned: &std::collections::HashSet<EntityUid>) -> i32 {
    level
        .entities
        .iter()
        .filter(|entity| {
            !entity.removed
                && !spawned.contains(&entity.id)
                && matches!(entity.kind, mc_bamboo_blocks::entity::EntityKind::Item(_))
        })
        .count() as i32
}
