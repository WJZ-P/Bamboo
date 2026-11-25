//! 硫磺方块怪的逐刻对拍和存档往返。
//!
//! Sulfur-cube per-tick comparison and save round trip.

use mc_bamboo_blocks::entity::mob::{MobType, sulfur_cube};
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_protocol::packet::play::EquipmentSlot;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SulfurCubeCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    frames: Vec<SulfurFrame>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct SulfurFrame {
    gone: bool,
    #[serde(flatten)]
    state: Option<SulfurState>,
    level_seed: i64,
    spawned: i32,
}

#[derive(Debug, Deserialize, PartialEq)]
struct SulfurState {
    motion: [i64; 8],
    seed: i64,
    /// 血量、hurtTime、deathTime、noActionTime、大小、fuse、着地、BODY 物品 ID。
    state: [i32; 8],
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SulfurCubeSaveCase {
    item: i32,
    fuse: i32,
    nbt: String,
    reloaded: String,
}

/// 存档里要逐键比的字段：官方 `saveWithoutId` 的公共键加上硫磺方块怪自己的三项。
const SAVE_KEYS: &[&str] = &[
    "Size",
    "wasOnGround",
    "pickup_timer",
    "from_bucket",
    "fuse",
    "Health",
    "Pos",
    "Rotation",
    "Invulnerable",
    "Motion",
    "PersistenceRequired",
    "equipment",
];

impl SulfurCubeCase {
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

fn sulfur_state(entity: &Entity) -> SulfurState {
    let mob = entity.kind.mob().expect("a sulfur cube is a mob");
    let (p, v) = (entity.position, entity.delta);
    SulfurState {
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
            mob.cube.size,
            mob.sulfur_cube.fuse,
            i32::from(entity.on_ground),
            mob.equipment.get(EquipmentSlot::Body).item,
        ],
    }
}

/// 官方 Java 夹具里 `setItemSlot` 会跑 `collectEquipmentChanges`；这里等价地
/// 把物品写进 BODY 槽并重算原型派生值。
fn equip(mob: &mut mc_bamboo_blocks::entity::mob::Mob, item: i32) {
    sulfur_cube::collect_equipment_changes(mob, ItemStack::new(item, 1));
}

impl Report {
    pub(super) fn replay_sulfur_cube(&mut self, case: &SulfurCubeCase) {
        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let mut entity = Entity::by_mob_type(MobType::SulfurCube, Vec3::new(0.5, 61.0, 0.5));
        entity.id = case.first_id;
        entity.random = JavaRandom::new(0x5EED_5EED);
        entity.y_rot = 0.0;
        entity.with_mob(|me, mob| {
            mob.y_head_rot = 0.0;
            mob.y_body_rot = 0.0;
            mob.cube.move_y_rot = 0.0;
            // regular 原型的 GRAVITY 不受影响；夹具里没有属性修饰，重力是 0.08。
            mob.attributes
                .set_base_for_oracle(mc_bamboo_data::registries::attribute::GRAVITY, 0.08);
            mob.uuid = [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2];
            sulfur_cube::set_size(me, mob, 2, true);
            let body = match case.name.as_str() {
                "sulfur_cube_swallow" => Some(mc_bamboo_data::items::DIRT),
                "sulfur_cube_hot" => Some(mc_bamboo_data::items::MAGMA_BLOCK),
                "sulfur_cube_explosive" | "sulfur_cube_fuse" => Some(mc_bamboo_data::items::TNT),
                _ => None,
            };
            if let Some(item) = body {
                equip(mob, item);
            }
            if case.name == "sulfur_cube_fuse" {
                sulfur_cube::prime(me, mob, false);
            }
        });
        // 官方夹具用不带种子的构造随机源，两边都直接把这一行放表里，
        // 不重放构造时的 UUID / yRot 取数。
        let id = entity.id;
        level.entities.insert_row_preserving(entity.split());
        let _ = level.entities.take_spawn_snapshots();
        let mut tracking = EntityTracking::default();
        let sound_source = mc_bamboo_blocks::LegacyRandom::new(12345);
        let break_test = self.break_test.clone();
        for (tick, expected) in case.frames.iter().enumerate() {
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
            }
            let events = level.entities.take_mob_events();
            let _ = event_packets(&events, || sound_source.next_long());
            let public = level.entities.public_at(id).cloned();
            if let Some(public) = &public
                && tick != 0
            {
                let snapshot = Snapshot::of(public);
                let _ = tracking.tracked_for_oracle(&snapshot).step(
                    &snapshot,
                    snapshot.needs_sync,
                    false,
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
            let mut ours = level.entities.get(id).map(|entity| sulfur_state(&entity));
            // 三处改坏确认：每一处都故意改掉被逐刻比较的一个字段。
            if let Some(state) = &mut ours {
                match break_test.as_deref() {
                    Some("sulfur_cube_fuse") => state.state[5] = state.state[5].saturating_add(1),
                    Some("sulfur_cube_size") => state.state[4] = 1,
                    Some("sulfur_cube_body") => state.state[7] = 0,
                    _ => {}
                }
            }
            if ours != expected.state {
                self.mismatch(format!(
                    "{label} state: official {:?}, bamboo {ours:?}",
                    expected.state
                ));
            }
            let level_seed = level.random.source().seed();
            // 官方爆炸的 `Util.shuffle(toBlow, level.random)` 在假世界里会多取数，
            // 夹具里爆炸那一刻之后的 level random 不参与对拍。
            let skip_level_seed = case.name == "sulfur_cube_fuse" && tick >= 120;
            if !skip_level_seed && level_seed != expected.level_seed {
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
        println!("{}: {} ticks", case.name, case.frames.len());
    }

    pub(super) fn replay_sulfur_cube_save(&mut self, case: &SulfurCubeSaveCase) {
        use mc_bamboo_blocks::entity::mob::nbt;
        self.cases += 1;
        let label = format!("sulfur cube save item {} fuse {}", case.item, case.fuse);
        let official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: SNBT does not parse: {error:?}"));
                return;
            }
        };
        let mut entity = Entity::by_mob_type(MobType::SulfurCube, Vec3::default());
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
