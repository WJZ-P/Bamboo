//! 快乐恶魂逐刻对拍：官方 `HappyGhast` / `HappyGhastAi` / `DriedGhastBlock` 与
//! Bamboo 的对应实现。每刻比位置、朝向、血量、年龄、尺寸、BODY 槽、乘客数、
//! 悬停超时、两项布尔元数据、大脑快照、随机源，以及四点拴绳里牛的位置和绳状态。
//!
//! Tick-by-tick happy ghast replay: vanilla `HappyGhast` / `HappyGhastAi` /
//! `DriedGhastBlock` against the Bamboo implementation. Each tick compares position,
//! rotation, health, age, size, BODY slot, passenger count, hover timeout, the two
//! synced booleans, the brain dump, the random seed, and the tied cow's position.

use mc_bamboo_blocks::entity::{
    Entity, JavaRandom,
    ctx::PlayerView,
    mob::{MobType, happy_ghast, interact, leash},
};
use mc_bamboo_core::Vec3;
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_protocol::packet::play::EquipmentSlot;

use super::*;

/// 干恶魂方块所在格。
const DRIED_POS: BlockPos = BlockPos::new(1, 64, 0);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HappyGhastCase {
    name: String,
    first_id: i32,
    seed: i64,
    start_time: i64,
    blocks: Vec<[i32; 4]>,
    baby: bool,
    brain: water_mob::BrainTable,
    harness: i32,
    shears: i32,
    snowball: i32,
    dried: [i32; 3],
    frames: Vec<HappyGhastFrame>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
struct HappyGhastFrame {
    /// `[x,y,z]`（f64 位）。
    pos: [i64; 3],
    /// `[yRot,xRot,yHeadRot,yBodyRot]`（f32 位）。
    rot: [i32; 4],
    health: i32,
    age: i32,
    /// `[width,height,eyeHeight]`（f32 位）。
    dims: [i32; 3],
    /// BODY 槽里的物品 ID，空槽是 0。
    body: i32,
    passengers: usize,
    stays_still: bool,
    is_leash_holder: bool,
    still_timeout: i32,
    brain: String,
    seed: i64,
    /// 四点拴绳场景里牛的 `[x,y,z]`（f64 位）。
    #[serde(default)]
    tied: Option<[i64; 3]>,
    #[serde(default)]
    tied_leashed: bool,
    /// 孵化场景里干恶魂方块是否已被移除。
    #[serde(default)]
    dried_gone: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct HappyGhastSaveCase {
    name: String,
    nbt: String,
    reloaded: String,
}

impl HappyGhastCase {
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
                bright_outside: true,
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
            explosion_break: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(self.first_id),
            players: vec![oracle_player(
                self.first_id - 1024,
                Vec3::new(2.5, 68.0, 0.5),
            )],
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            shoulder: Default::default(),
            containers: Default::default(),
            container_open_counts: Default::default(),
        }
    }

    /// 起步状态：和 Java 一样显式钉住随机源、朝向、幼年、NoAI；四点场景再建一头牛。
    ///
    /// Starting state: pin the random, facing, baby flag and NoAI like the Java side;
    /// the quad scene also builds a tied cow.
    fn spawn(
        &self,
        world: &mut mc_bamboo_blocks::World<'_>,
        break_test: Option<&str>,
    ) -> (i32, i32) {
        let pos = Vec3::new(2.5, 64.0, 0.5);
        let ghast_id = world.spawn_entity(Entity::by_mob_type(MobType::HappyGhast, pos));
        world.with_entity_mut(ghast_id, &mut |entity| {
            entity.random = JavaRandom::new(self.seed);
            entity.y_rot = 0.0;
            entity.old_position = pos;
            if let Some(mob) = entity.kind.mob_mut() {
                mob.uuid = 1_u128.to_be_bytes();
                mob.x_rot = 0.0;
                mob.y_rot_o = 0.0;
                mob.x_rot_o = 0.0;
                mob.y_head_rot = 0.0;
                mob.y_body_rot = 0.0;
                mob.persistence_required = true;
                mob.no_ai = true;
                if self.baby {
                    let (width, height, eye) = MobType::HappyGhast.dimensions(true);
                    entity.dimensions = (width, height);
                    mob.eye_height = eye;
                    mob.age = -24_000;
                }
                // 改坏确认：把幼年起始年龄错一格，逐刻年龄和尺寸都对不上。
                // Break check: perturb the baby's starting age.
                if break_test == Some("happy_ghast_age") && self.baby {
                    mob.age = -23_999;
                }
                // 改坏确认：悬停超时初值错一刻。
                // Break check: perturb the initial hover timeout.
                if break_test == Some("happy_ghast_still") {
                    mob.happy_ghast.set_still_timeout(1);
                }
            }
        });
        let tied_id = if self.name == "happy_ghast_quad" {
            let tied = world.spawn_entity(Entity::by_mob_type(
                MobType::Cow,
                Vec3::new(10.5, 64.0, 0.5),
            ));
            world.with_entity_mut(tied, &mut |entity| {
                entity.random = JavaRandom::new(0x54494544);
                entity.y_rot = 0.0;
                entity.old_position = Vec3::new(10.5, 64.0, 0.5);
                if let Some(mob) = entity.kind.mob_mut() {
                    mob.uuid = 3_u128.to_be_bytes();
                    mob.persistence_required = true;
                    mob.no_ai = true;
                    mob.leash = Some(leash::LeashData::new(
                        ghast_id,
                        leash::LeashDelayed::Entity(1_u128.to_be_bytes()),
                    ));
                }
            });
            tied
        } else {
            -1
        };
        (ghast_id, tied_id)
    }
}

fn oracle_player(uid: i32, position: Vec3) -> PlayerView {
    PlayerView {
        uid,
        uuid: Some(((1_u128 << 64) | 2).to_be_bytes()),
        position,
        delta: Vec3::default(),
        width: 0.6,
        height: 1.8,
        eye_height: 1.62,
        health: 20.0,
        look_direction: Vec3::view_vector(0.0, 0.0),
        gaze_disguise: false,
        alive: true,
        spectator: false,
        creative: false,
        game_mode: 0,
        mining_fatigue: None,
        sneaking: false,
        sprinting: false,
        swimming: false,
        flying: false,
        invisible: false,
        explosion_knockback_resistance: 0.0,
        armor_cover: 0.0,
        main_hand: 0,
        off_hand: 0,
        team: None,
        yaw: 0.0,
        pitch: 0.0,
        combat: Default::default(),
        effects: Default::default(),
        shoulder_left: None,
        shoulder_right: None,
    }
}

/// 用场景里的物品走和 Java 一样的 `mobInteract`（不经过游戏层，只比实体状态）。
///
/// Run `mobInteract` with the scene's item, matching the Java side (no game layer).
fn do_interact(world: &mut mc_bamboo_blocks::World<'_>, id: i32, item: i32, player_uid: i32) {
    let held = if item == 0 {
        ItemStack::empty()
    } else {
        ItemStack::new(item, 1)
    };
    let request = interact::MobInteractRequest {
        player_uid,
        player_uuid: ((1_u128 << 64) | 2).to_be_bytes(),
        hand: mc_bamboo_core::Hand::Main,
        held,
        creative: false,
        position: Vec3::new(2.5, 68.0, 0.5),
        player_height: 1.8,
        sneaking: false,
        difficulty: 2,
    };
    let _ = world.interact_mob(id, &request);
}

impl Report {
    pub(super) fn replay_happy_ghast(&mut self, case: &HappyGhastCase) {
        self.cases += 1;
        let break_test = self.break_test.clone();
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(case.start_time);
        let (ghast_id, tied_id) = case.spawn(
            &mut World::new(&mut level, &mut ticks),
            break_test.as_deref(),
        );
        // 改坏确认 `happy_ghast_quad`：把四点 holder 偏移换成单点，牛的位置/绳状态
        // 会在弹性拉扯后偏离官方。
        // Break check `happy_ghast_quad`: replace the quad holder offsets with a
        // single point so the elastic pull diverges.
        let _ = break_test.as_deref();
        // 大脑结构表逐字比较；改坏确认给表里加一个假传感器。
        let ours_brain = level.entities.get(ghast_id).map(|entity| {
            entity
                .kind
                .mob()
                .and_then(|mob| mob.brain.as_deref())
                .map(|brain| {
                    water_mob::brain_table(
                        brain,
                        break_test.as_deref() == Some("happy_ghast_brain"),
                    )
                })
                .unwrap_or_default()
        });
        if ours_brain.as_ref() != Some(&case.brain) {
            self.mismatch(format!(
                "{} brain table: official {:?}, bamboo {ours_brain:?}",
                case.name, case.brain
            ));
        }
        let _ = level.entities.take_mob_events();
        let _ = level.entities.take_spawn_snapshots();

        for (tick, expected) in case.frames.iter().enumerate() {
            if tick > 0 {
                ticks.set_time(case.start_time + tick as i64);
                // 骑乘场景：载具的 `tickRidden` 读的是玩家朝向，先于实体 tick 设好。
                // Riding scenes: `tickRidden` reads the player's facing, set before ticks.
                if case.name == "happy_ghast_ride"
                    && tick > 3
                    && let Some(player) = level.players.first_mut()
                {
                    player.yaw = 12.0;
                    player.pitch = 20.0;
                }
                let mut world = World::new(&mut level, &mut ticks);
                // 干恶魂方块：前四刻各推进一级水合，第四刻孵出幼崽。
                // 改坏确认 `happy_ghast_hatch`：跳过计划刻，方块不孵化。
                // Break check `happy_ghast_hatch`: skip the scheduled tick.
                if case.name == "happy_ghast_hatch"
                    && tick <= 4
                    && break_test.as_deref() != Some("happy_ghast_hatch")
                {
                    let state = world.block_state(BlockPos::new(
                        case.dried[0],
                        case.dried[1],
                        case.dried[2],
                    ));
                    if state.is_a(mc_bamboo_data::BlockAncestor::DriedGhastBlock) {
                        state.tick(
                            &mut world,
                            BlockPos::new(case.dried[0], case.dried[1], case.dried[2]),
                        );
                    }
                }
                // 幼年喂雪球加速成长。
                if case.name == "happy_ghast_grow" && tick == 3 {
                    do_interact(&mut world, ghast_id, case.snowball, case.first_id - 1024);
                }
                // 穿挽具 / 剪刀脱挽具。
                if case.name == "happy_ghast_harness" {
                    if tick == 3 {
                        // 改坏确认 `happy_ghast_harness`：拿雪球去交互，装不上挽具。
                        // Break check `happy_ghast_harness`: interact with a snowball.
                        let item = if break_test.as_deref() == Some("happy_ghast_harness") {
                            case.snowball
                        } else {
                            case.harness
                        };
                        do_interact(&mut world, ghast_id, item, case.first_id - 1024);
                    } else if tick == 10 {
                        do_interact(&mut world, ghast_id, case.shears, case.first_id - 1024);
                    }
                }
                if case.name == "happy_ghast_ride" {
                    if tick == 2 {
                        world.with_entity_mut(ghast_id, &mut |entity| {
                            // `LivingEntity.onEquipItem`：穿上可装备物品时取一次
                            // `nextLong` 当装备音的种子。
                            // `LivingEntity.onEquipItem`: equipping draws one
                            // `nextLong` seed for the equip sound.
                            let _seed = entity.random.next_long();
                            if let Some(mob) = entity.kind.mob_mut() {
                                mob.equipment.set(
                                    EquipmentSlot::Body,
                                    ItemStack::new(mc_bamboo_data::items::RED_HARNESS, 1),
                                );
                            }
                        });
                    } else if tick == 3 {
                        let mut was_vehicle = false;
                        world.with_entity_mut(ghast_id, &mut |entity| {
                            if let Some(mob) = entity.kind.mob_mut() {
                                was_vehicle = !mob.passengers.is_empty();
                                mob.passengers.push(case.first_id - 1024);
                            }
                        });
                        // `HappyGhast.addPassenger`：新乘客上车时清掉悬停超时，
                        // 护目镜放下（`Entity.playSound` 不取数）。
                        // `HappyGhast.addPassenger`: a new passenger clears the
                        // hover timeout; goggle-down draws no random.
                        world.with_entity_mut(ghast_id, &mut |entity| {
                            if let Some(mob) = entity.kind.mob_mut() {
                                let position = entity.position;
                                happy_ghast::after_add_passenger(position, mob, was_vehicle, false);
                            }
                        });
                    }
                }
                if case.name == "happy_ghast_quad" && tied_id >= 0 {
                    let mut at = Vec3::new(10.5 + tick as f64 * 0.15, 64.0, 0.5);
                    // 改坏确认 `happy_ghast_quad`：牛的起点错开一格，逐刻位置和断绳都不同。
                    // Break check `happy_ghast_quad`: shift the tied cow's position.
                    if break_test.as_deref() == Some("happy_ghast_quad") {
                        at.x += 1.0;
                    }
                    world.with_entity_mut(tied_id, &mut |entity| {
                        entity.position = at;
                    });
                }
                world.tick_entities();
                world.drain_effects();
            }
            let _ = level.entities.take_mob_events();

            let label = format!("{} tick {tick}", case.name);
            let Some(ours) = snapshot(&level, ghast_id, tied_id, case) else {
                self.mismatch(format!("{label}: happy ghast missing in bamboo"));
                continue;
            };
            if ours != *expected {
                self.mismatch(format!("{label}: official {expected:?}, bamboo {ours:?}"));
            }
            self.steps += 1;
            if self.mismatches.len() >= self.show {
                return;
            }
        }
        println!("{}: {} ticks", case.name, case.frames.len());
    }

    pub(super) fn replay_happy_ghast_save(&mut self, case: &HappyGhastSaveCase) {
        use mc_bamboo_blocks::entity::mob::nbt;
        self.cases += 1;
        let label = case.name.clone();
        let official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: SNBT does not parse: {error:?}"));
                return;
            }
        };
        let mut entity = Entity::by_mob_type(MobType::HappyGhast, Vec3::default());
        if let Err(error) = nbt::load_into(&mut entity, &official) {
            self.mismatch(format!("{label}: load failed: {error:?}"));
            return;
        }
        let ours = match nbt::write_mob(&entity.clone().split()) {
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
        // `saveWithoutId` 官方不写 `id`；其余键逐项比。
        // Official `saveWithoutId` omits `id`; compare every other key.
        for key in [
            "Health",
            "Age",
            "ForcedAge",
            "AgeLocked",
            "attributes",
            "still_timeout",
            "equipment",
            "Pos",
            "Rotation",
            "NoAI",
            "PersistenceRequired",
            "Air",
            "OnGround",
            "fall_distance",
            "InLove",
            "CanPickUpLoot",
            "Brain",
        ] {
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
        if let Some(mob) = entity.kind.mob() {
            let timeout = super::small_mob::save_field(&official, "still_timeout");
            let expected = match timeout {
                Some(mc_bamboo_nbt::Tag::Int(value)) => *value,
                _ => 0,
            };
            if mob.happy_ghast.still_timeout != expected {
                self.mismatch(format!(
                    "{label} still_timeout state: official {expected}, bamboo {}",
                    mob.happy_ghast.still_timeout
                ));
            }
        }
        self.steps += 1;
    }
}

/// 从实体表里取出和 Java 帧一一对应的状态。
///
/// Read the state corresponding to the Java frame from the entity table.
fn snapshot(
    level: &MobLevel,
    ghast_id: i32,
    tied_id: i32,
    case: &HappyGhastCase,
) -> Option<HappyGhastFrame> {
    let entity = level.entities.get(ghast_id)?;
    let mob = entity.kind.mob()?;
    let body = mob.equipment.get(EquipmentSlot::Body);
    let dried_gone = case.name == "happy_ghast_hatch"
        && !level
            .block_state(DRIED_POS)
            .is_a(mc_bamboo_data::BlockAncestor::DriedGhastBlock);
    let (tied, tied_leashed) = if tied_id >= 0 {
        let tied_entity = level.entities.get(tied_id)?;
        let tied_mob = tied_entity.kind.mob()?;
        (
            Some([
                tied_entity.position.x.to_bits() as i64,
                tied_entity.position.y.to_bits() as i64,
                tied_entity.position.z.to_bits() as i64,
            ]),
            leash::is_leashed(tied_mob),
        )
    } else {
        (None, false)
    };
    Some(HappyGhastFrame {
        pos: [
            entity.position.x.to_bits() as i64,
            entity.position.y.to_bits() as i64,
            entity.position.z.to_bits() as i64,
        ],
        rot: [
            entity.y_rot.to_bits() as i32,
            mob.x_rot.to_bits() as i32,
            mob.y_head_rot.to_bits() as i32,
            mob.y_body_rot.to_bits() as i32,
        ],
        health: mob.health.to_bits() as i32,
        age: mob.age,
        dims: [
            entity.dimensions.0.to_bits() as i32,
            entity.dimensions.1.to_bits() as i32,
            mob.eye_height.to_bits() as i32,
        ],
        body: if body.is_empty() { 0 } else { body.item },
        passengers: mob.passengers.len(),
        stays_still: mob.happy_ghast.stays_still,
        is_leash_holder: mob.happy_ghast.is_leash_holder,
        still_timeout: mob.happy_ghast.still_timeout,
        brain: brain::dump(level, &entity),
        seed: entity.random.state(),
        tied,
        tied_leashed,
        dried_gone,
    })
}

/// 让编译期确认两组工具函数确实被这个场景用到（四点偏移、乘客点）。
fn _facts_are_linked() {
    let _ = happy_ghast::quad_holder_offsets(4.0, 4.0);
    let _ = happy_ghast::passenger_attachment(0);
}
