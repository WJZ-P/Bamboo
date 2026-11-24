//! 末影珍珠的逐刻对拍：珍珠运动与随机源、主人位置 / 血量 / 受伤动画、末影螨生成、
//! 传送声参数，以及命中实体时的 0 点伤害。
//!
//! Ender pearl replay: the pearl motion and random, the owner's position, health and
//! hurt animation, the endermite spawn, the teleport sound arguments, and the zero
//! damage on an entity hit.

use mc_bamboo_blocks::entity::{
    JavaRandom,
    mob::MobType,
    thrown_egg::{EggShot, ThrowableKind},
};
use mc_bamboo_game::{ReplayPlayerSetup, ReplayPlayers};
use mc_bamboo_protocol::item::ItemStack;

use super::*;

/// 和官方一侧一样的主人 UUID (1, 2)。
const OWNER_UUID: u128 = (1_u128 << 64) | 2;
/// 官方 `ServerPlayer` 构造时 `Entity` 的随机源在场景里被固定成这个种子。
const OWNER_SEED: i64 = 0x0DDB_1A5E_5BAD_5EED_u64 as i64;
const OWNER_ID_OFFSET: i32 = 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PearlCase {
    name: String,
    first_id: i32,
    blocks: Vec<[i32; 4]>,
    start: [f64; 3],
    dir: [f64; 3],
    item: i32,
    /// 5% 末影螨场景里强制给珍珠的种子；0 表示用缺省种子。
    forced: i64,
    /// 主人起点坐标的位模式。
    owner: [i64; 3],
    cow: bool,
    /// 难度 ID：0 和平，2 普通。
    #[serde(default = "normal_difficulty")]
    difficulty: i32,
    frames: Vec<PearlFrame>,
}

fn normal_difficulty() -> i32 {
    2
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PearlFrame {
    pearl: Option<PearlState>,
    /// `[x 位, y 位, z 位, 血量位, hurtTime, 活着]`。
    owner: [i64; 6],
    /// 每只末影螨：`[实体号, x 位, y 位, z 位, yaw 位, pitch 位, 血量位]`。
    mites: Vec<[i64; 7]>,
    /// 被珍珠打中的牛：`[运动 8 项, 血量位, hurtTime]`。
    cow: Option<[i64; 10]>,
    /// 传送声：`[音效 ID, 音源, x 位, y 位, z 位, 音量位, 音高位]`。
    sounds: Vec<[i64; 7]>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct PearlState {
    id: i32,
    motion: [i64; 8],
    seed: i64,
}

impl PearlCase {
    fn player_id(&self) -> i32 {
        self.first_id + OWNER_ID_OFFSET
    }

    fn owner_position(&self) -> Vec3 {
        Vec3::new(
            f64::from_bits(self.owner[0] as u64),
            f64::from_bits(self.owner[1] as u64),
            f64::from_bits(self.owner[2] as u64),
        )
    }

    fn level(&self) -> MobLevel {
        let mut game = ReplayPlayers::new(
            self.difficulty,
            &[ReplayPlayerSetup {
                id: self.player_id(),
                uuid: OWNER_UUID,
                name: "PearlOracle".to_owned(),
                position: self.owner_position(),
                health: 20.0,
                seed: OWNER_SEED,
                creative: false,
            }],
        );
        // 官方场景里主人朝向固定成 yaw 45、pitch 12.5，末影螨 `snapTo` 要读。
        game.set_rotation(self.player_id(), 45.0, 12.5);
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
                ..ZombieFacts::default()
            },
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(self.first_id),
            players: Vec::new(),
            game: Some(game),
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            natural_spawn: None,
            biome: None,
        }
    }

    /// 和官方一侧同样的构造顺序：珍珠（种子、UUID、散布），强制种子，再牛。
    ///
    /// Same construction order as the official side: the pearl (seed, UUID, spread),
    /// the forced seed, then the cow.
    fn spawn(&self, world: &mut World<'_>) -> i32 {
        let [x, y, z] = self.start;
        let [dx, dy, dz] = self.dir;
        let shot = EggShot {
            position: Vec3::new(x, y, z),
            direction: Vec3::new(dx, dy, dz),
            power: 1.5,
            uncertainty: 1.0,
            source_movement: None,
            owner: Some(self.player_id()),
            owner_uuid: Some(OWNER_UUID.to_be_bytes()),
            item: ItemStack::new(self.item, 1),
            projectile: ThrowableKind::EnderPearl,
        };
        // `prepare_entity` 分配实体号、取种子并重放构造取数（UUID），和官方构造一致。
        let mut pearl = world.prepare_entity(Entity::thrown_egg(&shot));
        pearl.shoot_egg(&shot);
        if self.forced != 0 {
            pearl.random = JavaRandom::new(self.forced);
        }
        let id = world.add_prepared_entity(pearl);
        if self.cow {
            let mut cow = Entity::by_mob_type(MobType::Cow, Vec3::new(5.5, 64.0, 0.5));
            cow.with_mob(|_, mob| mob.no_ai = true);
            world.spawn_entity(cow);
        }
        id
    }
}

impl Report {
    pub(super) fn replay_ender_pearl(&mut self, case: &PearlCase) {
        use mc_bamboo_blocks::Effect;

        self.cases += 1;
        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let pearl_id = case.spawn(&mut World::new(&mut level, &mut ticks));
        let owner_id = case.player_id();
        for (tick, expected) in case.frames.iter().enumerate() {
            let mut sounds = Vec::new();
            if tick != 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
                for effect in world.drain_effects() {
                    if let Effect::Sound {
                        sound,
                        source,
                        x,
                        y,
                        z,
                        volume,
                        pitch,
                        ..
                    } = effect
                    {
                        // 只比珍珠发的传送声；受伤声走各自的受伤包路径。
                        if sound
                            != i32::from(mc_bamboo_data::sounds::sound_event(
                                "minecraft:entity.player.teleport",
                            ))
                        {
                            continue;
                        }
                        sounds.push([
                            i64::from(sound),
                            i64::from(source.id()),
                            x.to_bits() as i64,
                            y.to_bits() as i64,
                            z.to_bits() as i64,
                            i64::from(volume.to_bits() as i32),
                            i64::from(pitch.to_bits() as i32),
                        ]);
                    }
                }
            }
            let label = format!("{} tick {tick}", case.name);
            let pearl = level.entities.get(pearl_id);
            let ours = pearl.as_ref().map(|entity| {
                let EntityKind::Egg(egg) = &entity.kind else {
                    unreachable!()
                };
                PearlState {
                    id: pearl_id,
                    motion: super::egg::projectile_motion(entity, egg.x_rot),
                    seed: entity.random.state(),
                }
            });
            if ours != expected.pearl {
                self.mismatch(format!(
                    "{label} pearl: official {:?}, bamboo {ours:?}",
                    expected.pearl
                ));
            }
            let game = level.game.as_ref().expect("末影珍珠场景有玩家");
            let position = game.position(owner_id);
            let state = game.state(owner_id);
            let owner = [
                position.x.to_bits() as i64,
                position.y.to_bits() as i64,
                position.z.to_bits() as i64,
                state[0],
                state[2],
                i64::from(state[3] == 0),
            ];
            if owner != expected.owner {
                self.mismatch(format!(
                    "{label} owner: official {:?}, bamboo {owner:?}",
                    expected.owner
                ));
            }
            let mites: Vec<[i64; 7]> = level
                .entities
                .ids()
                .into_iter()
                .filter_map(|uid| {
                    let entity = level.entities.get(uid)?;
                    let mob = entity.kind.mob()?;
                    if mob.mob_type != MobType::Endermite {
                        return None;
                    }
                    Some([
                        i64::from(uid),
                        entity.position.x.to_bits() as i64,
                        entity.position.y.to_bits() as i64,
                        entity.position.z.to_bits() as i64,
                        i64::from(entity.y_rot.to_bits() as i32),
                        i64::from(mob.x_rot.to_bits() as i32),
                        i64::from(mob.health.to_bits() as i32),
                    ])
                })
                .collect();
            if mites != expected.mites {
                self.mismatch(format!(
                    "{label} endermites: official {:?}, bamboo {mites:?}",
                    expected.mites
                ));
            }
            let cow = case.cow.then(|| {
                let entity = level.entities.get(case.first_id + 1).expect("牛还在");
                let mob = entity.kind.mob().expect("牛是生物");
                let motion = super::egg::projectile_motion(&entity, mob.x_rot);
                let mut out = [0_i64; 10];
                out[..8].copy_from_slice(&motion);
                out[8] = i64::from(mob.health.to_bits() as i32);
                out[9] = i64::from(mob.hurt_time);
                out
            });
            if cow != expected.cow {
                self.mismatch(format!(
                    "{label} cow: official {:?}, bamboo {cow:?}",
                    expected.cow
                ));
            }
            if sounds != expected.sounds {
                self.mismatch(format!(
                    "{label} sounds: official {:?}, bamboo {sounds:?}",
                    expected.sounds
                ));
            }
            self.steps += 1;
        }
        println!(
            "{}: {} ticks, {} sounds",
            case.name,
            case.frames.len(),
            case.frames
                .iter()
                .map(|frame| frame.sounds.len())
                .sum::<usize>()
        );
    }
}

/// 玩家扔珍珠的物品使用：和 egg 的 `EggThrowCase` 一样比扔出那一刻，另比冷却。
///
/// A player's pearl use: the same throw-moment comparison as the egg case, plus the
/// cooldown.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PearlThrowCase {
    name: String,
    yaw: i32,
    pitch: i32,
    moving: bool,
    creative: bool,
    motion: [i64; 8],
    seed: i64,
    item: [i32; 2],
    left: i32,
    level_seed: i64,
    item_used: Vec<(i32, i32)>,
    cooldown: bool,
}

impl Report {
    pub(super) fn replay_pearl_throw(&mut self, case: &PearlThrowCase) {
        self.cases += 1;
        let movement = case.moving.then(|| Vec3::new(0.125, -0.25, 0.0625));
        let Some(ours) = mc_bamboo_game::player_egg::replay_pearl_throw(
            f32::from_bits(case.yaw as u32),
            f32::from_bits(case.pitch as u32),
            movement,
            case.creative,
        ) else {
            self.mismatch(format!("{}: bamboo threw no pearl", case.name));
            return;
        };
        let theirs = mc_bamboo_game::player_egg::ReplayEggThrow {
            motion: case.motion,
            seed: case.seed,
            item: case.item,
            left: case.left,
            level_seed: case.level_seed,
            item_used: case.item_used.clone(),
            cooldown: case.cooldown,
        };
        if ours != theirs {
            self.mismatch(format!(
                "{}: official {theirs:?}, bamboo {ours:?}",
                case.name
            ));
        }
        self.steps += 1;
        println!("{}: pearl thrown, {} left", case.name, ours.left);
    }
}
