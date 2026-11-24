//! `mob-oracle`：官方牛和 Bamboo 的牛逐刻对拍。
//!
//! `MobOracle.java` 在假的 `ServerLevel` 里跑官方的牛，每个场景一行：场景本身（方块、
//! 牛的起点和随机源种子、玩家、中途的伤害）加上每一刻每头牛的位置、速度、四个朝向、
//! 生命、是否着地、路径节点、正在跑的目标、随机源状态等。这里用同一个场景重放，
//! 浮点按原始位比较，不留容差。
//!
//! Compares official cows with Bamboo's, tick by tick, bit for bit.
//!
//! 鱿鱼和发光鱿鱼有自己的场景（`--only squid_` / `--only glow`，见 `mob_oracle/squid.rs`）。
//!
//! Squid and glow squid have their own scenes (`--only squid_` / `--only glow`, see
//! `mob_oracle/squid.rs`).

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, Container, EntityStore, EntityTables,
    Level, LevelRandom, LevelTables, World,
    entity::{
        Entities, Entity, EntityKind, JavaRandom,
        ctx::PlayerView,
        mob::{DamageSource, Mob, spawn},
    },
};
use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, Vec3, sign::SignContent};
use mc_bamboo_data::version;
use mc_bamboo_game::entity_tracking::{EntityTracking, Snapshot, event_packets};
use mc_bamboo_protocol::item::ItemStack;
use serde::Deserialize;

use crate::oracle::collect_log;

mod bat;
mod bee;
mod bee_ai;
mod bee_release;
mod brain;
mod egg;
mod equine;
mod fish;
mod gen_spawn;
mod golem;
mod grazing;
mod lightning;
mod llama_spit;
mod natural_spawn;
mod patrol_spawn;
mod phantom;
mod pillager;
mod projectile;
mod shot;
mod skeleton_death;
mod skeleton_spawn;
mod small_mob;
mod squid;
mod structure_spawn;
mod thunder;
mod undead_horse;
mod water_mob;
mod witch_throw;

const MIN_Y: i32 = -64;
const MAX_Y: i32 = 319;
const USAGE: &str = "usage: mob-oracle <java> [--only PREFIX] [--show N]";

/// 自带的 base64 解码，避免为这个开发工具再引入一个依赖。
mod base64_stub {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn decode(input: &str) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut buffer = 0_u32;
        let mut bits = 0;
        for byte in input.bytes() {
            if byte == b'=' {
                break;
            }
            let value = ALPHABET
                .iter()
                .position(|&candidate| candidate == byte)
                .ok_or_else(|| format!("bad base64 byte {byte}"))? as u32;
            buffer = buffer << 6 | value;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buffer >> bits) as u8);
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Line {
    Header(Header),
    Case(Box<Case>),
    Projectile(Box<projectile::ProjectileCase>),
    Shot(shot::ShotCase),
    ArrowPickup(projectile::ArrowPickupCase),
    SkeletonDeath(skeleton_death::DeathCase),
    SkeletonSpawn(skeleton_spawn::SkeletonSpawnCase),
    EquineSpawn(equine::SpawnCase),
    EquineGroup(equine::GroupCase),
    EquineBreed(equine::BreedCase),
    EquineInteract(equine::InteractCase),
    EquineDeath(equine::DeathCase),
    EquineSave(equine::SaveCase),
    LlamaSpit(Box<llama_spit::LlamaSpitCase>),
    WitchThrow(Box<witch_throw::WitchThrowCase>),
    NaturalSpawn(natural_spawn::Case),
    PatrolSpawn(patrol_spawn::Case),
    StructureSpawn(structure_spawn::Case),
    GenSpawn(gen_spawn::Case),
    PandaGenes(PandaGenesCase),
    Egg(Box<egg::EggCase>),
    EggThrow(egg::EggThrowCase),
    EggDispense(egg::EggDispenseCase),
    Squid(Box<squid::SquidCase>),
    Bat(Box<bat::BatCase>),
    Lightning(Box<lightning::LightningCase>),
    SkeletonTrap(Box<lightning::TrapCase>),
    ThunderTarget(Box<thunder::ThunderTargetCase>),
    ThunderHit(Box<thunder::ThunderHitCase>),
    TrapGear(Box<lightning::TrapGearCase>),
    UndeadHorse(Box<undead_horse::UndeadHorseCase>),
    UndeadHorseSave(undead_horse::UndeadHorseSaveCase),
    UndeadHorseDeath(undead_horse::UndeadHorseDeathCase),
    Bee(Box<bee::BeeCase>),
    BeeAi(Box<bee_ai::BeeAiCase>),
    BeeRelease(Box<bee_release::BeeReleaseCase>),
    Golem(Box<golem::GolemCase>),
    GolemAttackRoll(golem::GolemAttackRollCase),
    GolemSave(Box<golem::GolemSaveCase>),
    SmallMob(Box<small_mob::SmallMobCase>),
    SmallMobSave(small_mob::SmallMobSaveCase),
    SmallMobDeath(small_mob::SmallMobDeathCase),
    Fish(Box<fish::FishCase>),
    PhantomSpawn(Box<phantom::PhantomSpawnCase>),
    Phantom(Box<phantom::PhantomCase>),
    WaterMob(Box<water_mob::WaterMobCase>),
    AxolotlBreed(water_mob::AxolotlBreedCase),
    End(End),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PandaGenesCase {
    father_main: u8,
    father_hidden: u8,
    mother_main: u8,
    mother_hidden: u8,
    father_seed: i64,
    mother_seed: i64,
    baby_seed: i64,
    father_after: i64,
    mother_after: i64,
    baby_after: i64,
    baby_main: u8,
    baby_hidden: u8,
}

#[derive(Debug, Deserialize)]
struct Header {
    version: String,
    protocol: i32,
    tables: [i64; 2],
    asin: Vec<i64>,
    cos: Vec<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    first_id: i32,
    start_time: i64,
    blocks: Vec<[i32; 4]>,
    cows: Vec<CowSetup>,
    players: Vec<PlayerSetup>,
    /// 马科场景：让这名玩家骑上 0 号生物，-1 表示没人骑。
    ///
    /// Equine scenes: this player rides mob 0; -1 means nobody rides.
    #[serde(default = "minus_one")]
    rider: i32,
    /// 骑乘场景：这名下标上的生物骑上另一名下标的生物，-1 表示没有生物骑乘。
    ///
    /// Riding scenes: this mob rides another; -1 means no mob-on-mob riding.
    #[serde(default = "minus_one")]
    ride_passenger: i32,
    #[serde(default = "minus_one")]
    ride_vehicle: i32,
    /// 骑乘场景：所有生物都持久化，避开 `checkDespawn` 取数顺序的差异。
    ///
    /// Riding scenes: every mob is persistent, avoiding `checkDespawn` draw order.
    #[serde(default)]
    persisted: bool,
    /// 骑乘场景：所有生物都 `setNoAi(true)`。
    ///
    /// Riding scenes: every mob has `setNoAi(true)`.
    #[serde(default)]
    no_ai: bool,
    hurts: Vec<[i32; 4]>,
    #[serde(default)]
    summons: Vec<SummonSetup>,
    /// 刷怪蛋：（刻、生成格 x、y、z、`shouldOffsetYMore`）。
    #[serde(default)]
    eggs: Vec<[i32; 5]>,
    steps: Vec<Vec<CowStep>>,
    /// 每刻每头牛的全部出站包字节（base64，ID + 包体，不含帧长）。
    ///
    /// Per-tick outbound packet bytes for each cow, base64 of "id + body".
    #[serde(default)]
    packets: Option<Vec<Vec<String>>>,
    /// 每刻每头牛的同步标志位：`needsSync`、`syncPosition`、`syncVelocity`、
    /// `requiresPrecisePosition`、`isFallFlying`。
    #[serde(default)]
    sync_flags: Option<Vec<Vec<String>>>,
    /// 每刻新生成的掉落物和经验球。
    #[serde(default)]
    spawns: Option<Vec<Vec<Spawn>>>,
    /// 吃草场景：每刻 `blocks` 里每一格此刻的状态号，顺序相同。
    ///
    /// Grazing scenes: the current state id of every `blocks` cell per tick, same order.
    #[serde(default)]
    block_steps: Option<Vec<Vec<u32>>>,
    /// 致死伤害的种类：0 玩家攻击，1 摔落，2 着火。
    #[serde(default)]
    death_kind: i32,
    /// 场景开始时给每头牛设的着火刻数。
    #[serde(default)]
    fire_ticks: i32,
    /// 逐刻右键交互的结果（JSON 对象数组，直接序列化成 [`Interaction`]）。
    #[serde(default)]
    interactions: Option<Vec<Vec<Interaction>>>,
    /// 每项是（刻、牛下标、玩家下标、物品 ID、创造模式）。
    #[serde(default)]
    interact_actions: Vec<[i32; 5]>,
    /// 牛的效果动作：（刻、牛下标、0=add/1=clear-all、效果 ID、时长、等级）。
    #[serde(default)]
    cow_effect_actions: Vec<[i32; 6]>,
    /// 玩家受伤场景：难度、起始血量 / 吸收 / 消耗、装备和动作。
    #[serde(default)]
    difficulty: i32,
    #[serde(default)]
    player_health: i32,
    #[serde(default)]
    player_absorption: i32,
    #[serde(default)]
    player_exhaustion: i32,
    #[serde(default)]
    player_actions: Vec<PlayerActionSetup>,
    #[serde(default)]
    player_equipment: Vec<[i32; 5]>,
    #[serde(default)]
    player_protection: Vec<[i32; 4]>,
    #[serde(default)]
    player_steps: Option<Vec<Vec<String>>>,
    #[serde(default)]
    player_packets: Option<Vec<Vec<String>>>,
    /// 攻击场景：玩家 `attackStrengthTicker` 初值、疾跑 / 下落 / 着地状态、
    /// 主手附魔，以及横扫用的额外牛位置。
    ///
    /// Attack scenes: initial `attackStrengthTicker`, sprint / fall / ground,
    /// main-hand enchantments and extra cows for sweeping.
    #[serde(default)]
    attack_charge: i32,
    #[serde(default)]
    attack_sprint: bool,
    #[serde(default)]
    attack_fall: i64,
    #[serde(default)]
    attack_ground: bool,
    #[serde(default)]
    attack_enchants: Vec<(String, i32)>,
    /// 1b 玩家生命场景的规则与动作。
    #[serde(default)]
    life: Option<LifeSetup>,
    /// 1b：重生序列里逐字节对照的关键包。
    #[serde(default)]
    key_packets: Option<Vec<Vec<String>>>,
    /// 1c：官方 `PlayerSpawnFinder.findSpawn` 的落位结果。
    #[serde(default)]
    spawn_result: Option<SpawnResult>,
    /// 僵尸场景注入的世界事实。
    #[serde(default)]
    sky_darken: i32,
    #[serde(default)]
    raining: bool,
    #[serde(default)]
    bright_outside: bool,
    #[serde(default)]
    monsters_burn: bool,
    /// 假世界的海平面，缺省是超平坦的 -63；溺尸的上浮和上岸目标读它。
    ///
    /// The fake world's sea level, -63 for superflat unless a scene overrides it;
    /// the drowned's swim-up and go-to-beach goals read it.
    #[serde(default = "minus_63")]
    sea_level: i32,
    /// 狐狸场景：像刷出来的狐狸一样带上捕猎目标（`Fox.setTargetGoals`）。
    ///
    /// Fox scenes: add the hunting goals like a spawned fox.
    #[serde(default)]
    fox_hunts: bool,
    /// 僵尸场景：玩家走游戏层真实的受伤链路（[`mc_bamboo_game::ReplayPlayers`]）。
    #[serde(default)]
    real_players: bool,
    /// 僵尸场景：主世界时钟的起始刻，`>= 0` 时天色、`MONSTERS_BURN` 和月相按真实时间线逐刻算。
    #[serde(default = "minus_one_i64")]
    clock_start: i64,
    /// 真实时间模式的雨强（f32 位）。
    #[serde(default)]
    rain: i32,
    /// 每只生物头上戴的物品 ID（0 是空）和初始耐久损耗。
    #[serde(default)]
    helmet: i32,
    #[serde(default)]
    helmet_damage: i32,
    /// 每刻生物发出的 `entity_event` 和 `set_equipment`（base64，按发生顺序）。
    #[serde(default)]
    mob_event_packets: Option<Vec<Vec<String>>>,
    /// 僵尸场景：（刻、x、y、z、状态 ID），那一刻实体 tick 之前直接写进方块表。
    #[serde(default)]
    place_at: Vec<[i32; 5]>,
    /// 僵尸场景：第 `sleep_tick` 刻开头 0 号玩家右键床头 `bed`，官方结果是 `sleep_result`。
    #[serde(default = "minus_one")]
    sleep_tick: i32,
    #[serde(default)]
    bed: Option<[i32; 3]>,
    #[serde(default)]
    sleep_result: String,
    /// 0 号玩家是创造模式。
    #[serde(default)]
    player_creative: bool,
    /// 史莱姆场景：每刻分裂出来的小史莱姆（x、y、z、朝向的位，大小，血量的位）。
    ///
    /// Slime scenes: the children split off each tick (x, y, z and yaw bits, size, health bits).
    #[serde(default)]
    cube_splits: Option<Vec<Vec<[i64; 6]>>>,
    /// 僵尸场景每刻的 `levelEvent`（`[0, 事件, x, y, z, 数据]`）和
    /// `destroyBlockProgress`（`[1, 实体, x, y, z, 阶段]`），按发生顺序。
    #[serde(default)]
    level_events: Option<Vec<Vec<[i32; 6]>>>,
    /// 每刻 0 号玩家的状态：血量位、受伤冷却、受伤动画、死亡、着火刻数。
    #[serde(default)]
    zombie_player_states: Option<Vec<[i64; 5]>>,
    /// 每刻 0 号玩家自己收到的那几种包（受伤动画、战斗进出、死亡界面、血量）。
    #[serde(default)]
    zombie_player_packets: Option<Vec<Vec<String>>>,
    /// S2 玩家效果场景的规则与动作。
    #[serde(default)]
    effect_scene: Option<EffectScene>,
    /// S3 环境伤害场景的方块与起始状态。
    #[serde(default)]
    env_scene: Option<EnvScene>,
    /// S4 吃东西场景。
    #[serde(default)]
    consume_scene: Option<ConsumeScene>,
    /// 大脑场景：玩家 0 疾跑的 [起, 止) 刻。
    ///
    /// Brain scenes: the [start, end) ticks in which player 0 sprints.
    #[serde(default)]
    sprint: Option<[i32; 2]>,
}

/// 1c 落脚点对拍的官方答案：坐标按 f64 原始位传，随机源末态逐位比。
/// Official answer of one landing case: positions as raw f64 bits and the
/// final random state.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpawnResult {
    x: i64,
    y: i64,
    z: i64,
    radius: i32,
    seed: i64,
    #[serde(default)]
    offset: i32,
    random_state: i64,
    game_type: i32,
    personal: Option<[i32; 3]>,
    missing: bool,
    spawn_at: [i32; 3],
}

/// Java 一侧 `playLife` 的场景描述。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LifeSetup {
    keep_inventory: bool,
    immediate_respawn: bool,
    show_death_messages: bool,
    natural_regeneration: bool,
    xp_level: i32,
    inventory_items: Vec<i32>,
    vanishing_slots: Vec<i32>,
    death_tick: i32,
    respawn_tick: i32,
    death_kind: String,
    death_amount: i32,
    #[serde(default)]
    death_attacker: i32,
    #[allow(dead_code, reason = "start health also arrives as playerHealth bits")]
    #[serde(default)]
    start_health: f32,
    #[serde(default)]
    start_food: i32,
    #[serde(default)]
    start_saturation: i32,
    #[allow(
        dead_code,
        reason = "start exhaustion also arrives as playerExhaustion bits"
    )]
    #[serde(default)]
    start_exhaustion: f32,
    #[serde(default)]
    start_food_timer: i32,
    #[serde(default)]
    credit_hurt_tick: i32,
    #[serde(default)]
    death_fall_distance: i32,
    #[allow(dead_code, reason = "noDeath is represented by a negative deathTick")]
    #[serde(default)]
    no_death: bool,
}

/// Java 一侧 `lifeStepJson`。
#[derive(Debug, Deserialize)]
struct PlayerLifeStepJson {
    hp: i32,
    abs: i32,
    ht: i32,
    hd: i32,
    cd: i32,
    lh: i32,
    food: i32,
    sat: i32,
    exh: i32,
    timer: i32,
    #[serde(rename = "xpLevel")]
    xp_level: i32,
    #[serde(rename = "xpTotal")]
    xp_total: i32,
    score: i32,
    dead: bool,
    #[serde(rename = "deathTime")]
    death_time: i32,
    #[serde(rename = "lastDeath")]
    last_death: Option<[i32; 3]>,
    #[allow(dead_code, reason = "entity random draws differ for death drops")]
    seed: i64,
}

impl PlayerLifeStepJson {
    fn matches(&self, ours: &mc_bamboo_game::PlayerLifeStep) -> bool {
        self.hp == ours.hp
            && self.abs == ours.abs
            && self.ht == ours.ht
            && self.hd == ours.hd
            && self.cd == ours.cd
            && self.lh == ours.lh
            && self.food == ours.food
            && self.sat == ours.sat
            && self.exh == ours.exh
            && self.timer == ours.timer
            && self.xp_level == ours.xp_level
            && self.xp_total == ours.xp_total
            && self.score == ours.score
            && self.dead == ours.dead
            && self.death_time == ours.death_time
            && self.last_death == ours.last_death
        // `random` 的种子在死亡时消耗的次数两边不完全一样
        // （官方 `Entity.spawnAtLocation` 用实体自己的随机源），
        // 逐刻状态里不比它；掉落物位置由 `spawns` 单独比。
    }
}

/// Java `PlayerAction` 的线上格式。`amount` / `at` 用原始位。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayerActionSetup {
    tick: usize,
    action: i32,
    kind: Option<String>,
    amount: i32,
    at: Option<[i64; 3]>,
    #[serde(default)]
    #[allow(
        dead_code,
        reason = "the Java action also names the cow for attack scenes"
    )]
    cow: i32,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Interaction {
    result: String,
    item: i32,
    count: i32,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Spawn {
    id: i32,
    x: i64,
    y: i64,
    z: i64,
    kind: String,
    #[serde(default)]
    item: i32,
    #[serde(default)]
    count: i32,
    #[serde(default)]
    value: i32,
    /// 幼崽的构造随机数、头部朝向和 UUID，作为场景起点原样交给 Bamboo。
    #[serde(default)]
    seed: Option<i64>,
    #[serde(default)]
    hr: Option<i32>,
    /// 身体朝向：幼崽是 0，哞菇剪成的牛抄哞菇的。
    ///
    /// Body yaw: 0 for a calf, copied from the mooshroom for a converted cow.
    #[serde(default)]
    yr: Option<i32>,
    #[serde(default)]
    uuid: Option<String>,
    /// 幼鸡构造时抽的 `eggTime`，同样是场景起点。
    ///
    /// A chick's constructor `eggTime`, also a scene starting point.
    #[serde(default)]
    egg: Option<i32>,
    /// 新生熊猫在 `getBreedOffspring` 用没播种的随机源抽出的双基因，作为场景起点。
    ///
    /// A newborn panda's genes rolled on its unseeded random in `getBreedOffspring`,
    /// passed as a scene starting point.
    #[serde(default)]
    genes: Option<[u8; 2]>,
}

#[derive(Debug, Deserialize)]
struct SummonSetup {
    tick: usize,
    x: i64,
    y: i64,
    z: i64,
    nbt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CowSetup {
    x: i64,
    y: i64,
    z: i64,
    yaw: i32,
    seed: i64,
    #[serde(default)]
    age: i32,
    /// 0 牛 / 1 僵尸 / 2 尸壳 / 3 猪 / 4 羊 / 5 鸡 / 6 兔 / 7 狐 / 8 狼；缺省按牛（旧场景）。
    ///
    /// 0 cow / 1 zombie / 2 husk / 3 pig / 4 sheep / 5 chicken / 6 rabbit / 7 fox / 8 wolf;
    /// cow when absent.
    #[serde(default)]
    kind: i32,
    /// 僵尸场景的队伍编号，-1 不在队伍里。
    #[serde(default = "minus_one")]
    team: i32,
    /// 狐狸 `SleepGoal` 构造时取的倒计时，官方那一边报过来。
    ///
    /// The fox `SleepGoal` countdown drawn at construction, from the official side.
    #[serde(default)]
    sleep: i32,
    /// 羊的 `DATA_WOOL_ID` 起点。
    ///
    /// The sheep's starting `DATA_WOOL_ID`.
    #[serde(default)]
    wool: u8,
    /// 苦力怕的起始标志：第 0 位带电，第 1 位已点燃。
    ///
    /// Creeper start flags: bit 0 powered, bit 1 ignited.
    #[serde(default)]
    creeper: i32,
    /// 鸡的 `eggTime` 起点（官方在构造函数里用重设种子之前的随机源抽）。
    ///
    /// The chicken's starting `eggTime` (vanilla rolls it before the scene reseed).
    #[serde(default)]
    egg: i32,
    /// 大脑传感器构造时取的起始延迟和犰狳的掉鳞甲计时（官方报过来）。
    ///
    /// The brain sensor start delays and armadillo scute timer drawn at construction.
    #[serde(default)]
    sensors: Vec<i64>,
    #[serde(default)]
    scute: i32,
    /// 马科的初始脾气，-1 表示用构造时的值。
    ///
    /// The equine's starting temper; -1 keeps the constructed value.
    #[serde(default = "minus_one")]
    temper: i32,
    /// 熊猫的显性 / 隐性基因 id。
    ///
    /// The panda's main and hidden gene ids.
    #[serde(default)]
    genes: Vec<u8>,
    /// 熊猫主手一开始拿的物品。
    ///
    /// The item a panda holds at the start.
    #[serde(default)]
    held: i32,
    /// 末影人搬着的方块状态 ID，0 表示空手。
    ///
    /// The block state an enderman carries; 0 means empty-handed.
    #[serde(default)]
    carried: i32,
    /// 海龟场景：开局带蛋标志。
    ///
    /// Turtle scenes: the starting has-egg flag.
    #[serde(default, rename = "turtleEgg")]
    turtle_egg: bool,
    /// 海龟场景：家方块（x, y, z）。
    ///
    /// Turtle scenes: the home block (x, y, z).
    #[serde(default, rename = "turtleHome")]
    turtle_home: [i32; 3],
}

#[derive(Debug, Deserialize)]
struct PlayerSetup {
    id: i32,
    x: i64,
    y: i64,
    z: i64,
    eye: i32,
    #[serde(default)]
    yaw: i32,
    #[serde(default)]
    pitch: i32,
    item: i32,
    #[serde(default)]
    looting: i32,
    /// 僵尸场景的队伍编号，-1 不在队伍里。
    #[serde(default = "minus_one")]
    team: i32,
}

#[derive(Debug, Deserialize, PartialEq)]
struct CowStep {
    x: i64,
    y: i64,
    z: i64,
    dx: i64,
    dy: i64,
    dz: i64,
    yr: i32,
    xr: i32,
    hr: i32,
    br: i32,
    hp: i32,
    g: bool,
    rm: bool,
    w: bool,
    #[serde(default)]
    swimming: bool,
    fd: i64,
    nat: i32,
    ht: i32,
    dt: i32,
    fire: i32,
    air: i32,
    seed: i64,
    age: i32,
    love: i32,
    lh: bool,
    fr: i64,
    var: String,
    snd: String,
    uuid: String,
    lr: i64,
    goals: String,
    path: Option<Vec<i32>>,
    /// 僵尸场景专属；旧牛场景缺省 -1/false/0，比较时按类型跳过。
    #[serde(default = "minus_one")]
    target: i32,
    #[serde(default)]
    baby: bool,
    #[serde(default)]
    aggr: bool,
    #[serde(default)]
    doors: bool,
    #[serde(default)]
    ph: i32,
    #[serde(default)]
    pht: i32,
    #[serde(default)]
    pcd: i32,
    /// 僵尸场景：整张方块表的散列，见 [`block_hash`]。
    #[serde(default)]
    bh: i64,
    /// 僵尸场景：头盔的耐久损耗（空是 -1）、数量和 `ARMOR` 属性值（f64 位）。
    #[serde(default)]
    hd: i32,
    #[serde(default)]
    hc: i32,
    #[serde(default)]
    armor: i64,
    #[serde(default, rename = "rabbitCarrotTicks")]
    rabbit_carrot_ticks: i32,
    #[serde(default, rename = "foxFlags")]
    fox_flags: i32,
    #[serde(default, rename = "foxMouth")]
    fox_mouth: i32,
    /// 山羊：尖叫、左角、右角、长跳姿势各一位；其余生物是 0。
    ///
    /// Goats: screaming, left horn, right horn and the long-jump pose, one bit each; 0
    /// for other mobs.
    #[serde(default)]
    goat: i32,
    /// 狼：驯服标志、坐下命令、有没有主人、仇恨结束时刻、讨食、项圈、最大生命（位）。
    ///
    /// Wolves: tame flags, sit order, owner, anger end, begging, collar, max health bits.
    #[serde(default, rename = "wolfTame")]
    wolf_tame: i32,
    #[serde(default, rename = "wolfSit")]
    wolf_sit: bool,
    #[serde(default, rename = "wolfOwned")]
    wolf_owned: bool,
    #[serde(default, rename = "wolfAnger")]
    wolf_anger: i64,
    #[serde(default, rename = "wolfBeg")]
    wolf_beg: bool,
    #[serde(default, rename = "wolfCollar")]
    wolf_collar: i32,
    #[serde(default, rename = "wolfMaxHp")]
    wolf_max_hp: i64,
    /// 马科场景：`DATA_ID_FLAGS` 字节和脾气值，其余生物是 0。
    ///
    /// Equine scenes: the `DATA_ID_FLAGS` byte and the temper; 0 for other mobs.
    #[serde(default)]
    eq: i32,
    #[serde(default)]
    tmp: i32,
    /// 大脑快照：`活动|正在跑的行为|记忆`，不跑大脑的生物是空串。
    ///
    /// The brain dump `activities|running|memories`; empty for goal-driven mobs.
    #[serde(default)]
    brain: String,
    #[serde(default)]
    rider_count: usize,
    #[serde(default)]
    owner: Option<String>,
    /// 海龟逐刻状态；别的生物官方那侧不写这几个键，缺省全是 0 / false。
    ///
    /// Turtle per-tick state; the official side omits these keys for other mobs,
    /// so they default to 0 / false.
    #[serde(default, rename = "turtleEgg")]
    turtle_egg: i32,
    #[serde(default, rename = "turtleLaying")]
    turtle_laying: i32,
    #[serde(default, rename = "turtleLayCount")]
    turtle_lay_count: i32,
    /// `getCurrentDifficultyAt(pos).getEffectiveDifficulty()` 的 f32 位；
    /// `InhabitedTime` 场景逐刻比区域难度。
    #[serde(default)]
    ed: i32,
    #[serde(default, rename = "turtleGoingHome")]
    turtle_going_home: bool,
    #[serde(default, rename = "turtleHomeX")]
    turtle_home_x: i32,
    #[serde(default, rename = "turtleHomeY")]
    turtle_home_y: i32,
    #[serde(default, rename = "turtleHomeZ")]
    turtle_home_z: i32,
}

/// 走 Java 一侧 `playZombie` 的场景：僵尸，兔子、狐狸、狼，犰狳、山羊、青蛙、蝌蚪、
/// 骆驼，以及苦力怕、蜘蛛、洞穴蜘蛛、末影人、史莱姆、女巫和溺尸。
///
/// Scenes played by the Java `playZombie` driver: zombies, the wild animals,
/// armadillos, goats, frogs, tadpoles and camels, plus creepers, spiders, cave spiders,
/// endermen, slimes, witches and drowned.
fn zombie_style(name: &str) -> bool {
    [
        "zombie_",
        "rabbit_",
        "fox_",
        "wolf_",
        "armadillo_",
        "goat_",
        "frog_",
        "tadpole_",
        "camel_",
        "polar_bear_fight_",
        "creeper_",
        "spider_",
        "cave_spider_",
        "enderman_",
        "slime_",
        "witch_drink_",
        "pillager_",
        "drowned_",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

const fn minus_one_i64() -> i64 {
    -1
}

const fn minus_one() -> i32 {
    -1
}

const fn minus_63() -> i32 {
    -63
}

#[derive(Debug, Deserialize)]
struct End {
    cases: u32,
}

#[derive(Debug, Clone, Copy, Default)]
struct ZombieFacts {
    halloween: bool,
    /// 主世界时钟（真实时间模式逐刻推进，否则 0）。
    clock: i64,
    /// `WorldGenLevel.getSeed()`，史莱姆区块读它。
    ///
    /// `WorldGenLevel.getSeed()`, read for slime chunks.
    world_seed: i64,
    inhabited_time: i64,
    /// `InhabitedTime` 场景：逐刻按候选区块集合累加，并输出区域难度 `ed`。
    accumulates_inhabited: bool,
    can_see_sky: bool,
    sky_darken: i32,
    difficulty: i32,
    bright_outside: bool,
    monsters_burn: bool,
    sea_level: i32,
    raining: bool,
    /// 蜂巢对拍直接注入官方的 `BEES_STAY_IN_HIVE` 常量层。
    bees_stay: Option<bool>,
    /// 热群系 / 下界场景注入的 `SNOW_GOLEM_MELTS`。
    snow_golem_melts: bool,
}

/// 假世界：方块表、玩家表、世界随机源。和 `entity-oracle` 的那个同一个做法。
struct MobLevel {
    blocks: HashMap<BlockPos, BlockState>,
    random: LevelRandom,
    /// 战利品表的随机序列，世界种子为 0，和官方一侧一样。
    loot_sequences: mc_bamboo_loot::RandomSequences,
    /// 死亡时攻击者的主手抢夺等级，按玩家下标。
    looting: Vec<i32>,
    rules: mc_bamboo_blocks::BlockGameRules,
    /// 僵尸场景注入的天色 / 天气 / 环境属性；旧场景恒为缺省。
    facts: ZombieFacts,
    natural_spawn: Option<natural_spawn::SceneFacts>,
    redstone: mc_bamboo_blocks::RedstoneMemory,
    block_entities: BlockEntities,
    block_events: BlockEvents,
    entities: Entities,
    players: Vec<PlayerView>,
    /// 走真实受伤链路的玩家；有它时 `for_each_player` 和生物打玩家都交给它。
    game: Option<mc_bamboo_game::ReplayPlayers>,
    /// 逐刻累加的 `InhabitedTime`，和 Java 一侧同规则；区域难度读它。
    inhabited: HashMap<ChunkPos, i64>,
    /// 僵尸场景里生物的队伍，按 UUID。
    mob_teams: HashMap<[u8; 16], u32>,
    /// 改坏确认：`wasHurt` 一律当真。
    wrong_was_hurt: bool,
    /// 改坏确认 `zombie_hunger`：尸壳给的饥饿少一刻。
    short_hunger: bool,
    /// 改坏确认 `zombie_bed`：床的怪物检查一律放行。
    ignore_bed_monsters: bool,
}

impl LevelTables for MobLevel {
    fn block_entities_table(&self) -> &BlockEntities {
        &self.block_entities
    }

    fn block_entities_table_mut(&mut self) -> &mut BlockEntities {
        &mut self.block_entities
    }

    fn block_events_table(&self) -> &BlockEvents {
        &self.block_events
    }

    fn block_events_table_mut(&mut self) -> &mut BlockEvents {
        &mut self.block_events
    }

    fn redstone_table(&self) -> &mc_bamboo_blocks::RedstoneMemory {
        &self.redstone
    }

    fn redstone_table_mut(&mut self) -> &mut mc_bamboo_blocks::RedstoneMemory {
        &mut self.redstone
    }
}

impl EntityTables for MobLevel {
    fn entities_table(&self) -> &EntityStore {
        &self.entities
    }

    fn entities_table_mut(&mut self) -> &mut EntityStore {
        &mut self.entities
    }
}

impl Level for MobLevel {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        if !(MIN_Y..=MAX_Y).contains(&pos.y) {
            return BlockState(mc_bamboo_data::blocks::VOID_AIR);
        }
        self.blocks.get(&pos).copied().unwrap_or(BlockState::AIR)
    }

    fn write_block(&mut self, pos: BlockPos, state: BlockState, _flags: u32) -> bool {
        if !(MIN_Y..=MAX_Y).contains(&pos.y) {
            return false;
        }
        if state.is_air() {
            self.blocks.remove(&pos);
        } else {
            self.blocks.insert(pos, state);
        }
        true
    }

    fn is_unobstructed(&self, _boxes: &[mc_bamboo_blocks::Aabb]) -> bool {
        true
    }

    fn min_y(&self) -> i32 {
        MIN_Y
    }

    fn max_y(&self) -> i32 {
        MAX_Y
    }

    /// 官方夹具的 `FakeLevel.getSeaLevel()`：缺省超平坦 -63，场景可以覆盖。
    ///
    /// The Java fixture's `FakeLevel.getSeaLevel()`: superflat's -63 unless the scene
    /// overrides it.
    fn sea_level(&self) -> i32 {
        self.facts.sea_level
    }

    fn random(&self) -> &LevelRandom {
        &self.random
    }

    fn block_game_rules(&self) -> mc_bamboo_blocks::BlockGameRules {
        self.rules
    }

    fn is_randomly_ticking_section(&self, _chunk: ChunkPos, _section_y: i32) -> bool {
        false
    }

    fn any_player_close_enough_to(&self, _pos: BlockPos, _max_distance: i32) -> bool {
        true
    }

    fn brightness(&self, layer: LightLayer, _pos: BlockPos) -> i32 {
        match layer {
            LightLayer::Sky => self.natural_spawn.as_ref().map_or(15, |scene| scene.sky),
            LightLayer::Block => self.natural_spawn.as_ref().map_or(0, |scene| scene.block),
        }
    }

    fn biome_id(&self, _pos: BlockPos) -> i32 {
        self.natural_spawn
            .as_ref()
            .map_or(mc_bamboo_data::registries::biome::PLAINS, |scene| {
                scene.biome
            })
    }

    fn natural_spawn_table(
        &self,
        pos: BlockPos,
        category: mc_bamboo_data::spawning::MobCategory,
    ) -> &'static [mc_bamboo_data::spawning::MobSpawnerDef] {
        match &self.natural_spawn {
            Some(scene) if category == scene.category => std::slice::from_ref(scene.entry),
            Some(_) => &[],
            None => {
                mc_bamboo_data::spawning::for_biome_id(self.biome_id(pos)).mobs_to_spawn(category)
            }
        }
    }

    fn sign(&self, _pos: BlockPos) -> Option<SignContent> {
        None
    }

    fn set_sign(&mut self, _pos: BlockPos, _content: SignContent) -> bool {
        false
    }

    fn note_block_sound(&self, _pos: BlockPos) -> Option<String> {
        None
    }

    fn with_container(&mut self, _pos: BlockPos, _f: &mut dyn FnMut(&mut dyn Container)) -> bool {
        false
    }

    fn for_each_player(&self, visit: &mut dyn FnMut(&PlayerView)) {
        if let Some(game) = &self.game {
            for player in game.views() {
                visit(&player);
            }
            return;
        }
        for player in &self.players {
            visit(player);
        }
    }

    fn hurt_player_now(
        &mut self,
        target: mc_bamboo_blocks::entity::EntityUid,
        source: DamageSource,
        amount: f32,
        attacker: Option<&mc_bamboo_blocks::entity::EntityPublic>,
    ) -> Option<mc_bamboo_blocks::PlayerHurtNow> {
        let game = self.game.as_mut()?;
        let mut outcome = game.hurt(target, source, amount, attacker);
        if self.wrong_was_hurt {
            outcome.hurt = true;
        }
        Some(outcome)
    }

    fn ignite_player_now(
        &mut self,
        target: mc_bamboo_blocks::entity::EntityUid,
        ticks: i32,
    ) -> bool {
        let Some(game) = self.game.as_mut() else {
            return false;
        };
        game.ignite(target, ticks);
        true
    }

    fn give_player_effect_now(
        &mut self,
        target: mc_bamboo_blocks::entity::EntityUid,
        instance: &mc_bamboo_blocks::entity::mob::effects::EffectInstance,
    ) -> bool {
        let Some(game) = self.game.as_mut() else {
            return false;
        };
        if mc_bamboo_blocks::entity::mob::effects::is_instantaneous(instance.effect) {
            return false;
        }
        let mut instance = instance.clone();
        if self.short_hunger {
            // 改坏确认：尸壳的饥饿少给一刻。
            instance.duration -= 1;
        }
        game.give_effect(target, instance);
        true
    }

    fn heal_player_now(
        &mut self,
        target: mc_bamboo_blocks::entity::EntityUid,
        amount: f32,
    ) -> bool {
        let Some(game) = self.game.as_mut() else {
            return false;
        };
        game.heal(target, amount);
        true
    }

    fn sky_darken(&self) -> i32 {
        self.facts.sky_darken
    }

    fn difficulty_id(&self) -> i32 {
        self.facts.difficulty
    }

    fn is_bright_outside(&self) -> bool {
        self.facts.bright_outside
    }

    fn monsters_burn(&self) -> bool {
        self.facts.monsters_burn
    }

    fn block_weather(&self) -> mc_bamboo_blocks::BlockWeather {
        mc_bamboo_blocks::BlockWeather {
            rain_level: if self.facts.raining { 1.0 } else { 0.0 },
            thunder_level: if self
                .natural_spawn
                .as_ref()
                .is_some_and(|scene| scene.thunder)
            {
                1.0
            } else {
                0.0
            },
        }
    }
    fn can_see_sky(&self, _pos: BlockPos) -> bool {
        self.facts.can_see_sky
    }

    fn is_raining_at(&self, _pos: BlockPos) -> bool {
        self.facts.raining
    }

    fn snow_golem_melts(&self, _pos: BlockPos) -> bool {
        self.facts.snow_golem_melts
    }

    fn clock_ticks(&self) -> i64 {
        self.facts.clock
    }

    fn world_seed(&self) -> i64 {
        self.facts.world_seed
    }

    fn entity_team(&self, uuid: &[u8; 16]) -> Option<u32> {
        self.mob_teams.get(uuid).copied()
    }

    fn inhabited_time(&self, chunk: ChunkPos) -> i64 {
        self.inhabited
            .get(&chunk)
            .copied()
            .unwrap_or(self.facts.inhabited_time)
    }

    fn monster_prevents_rest(&self, pos: BlockPos) -> bool {
        if self.ignore_bed_monsters {
            return false;
        }
        mc_bamboo_blocks::monster_prevents_rest_in(self, pos)
    }

    fn bees_stay_in_hive(&self) -> bool {
        self.facts.bees_stay.unwrap_or_else(|| {
            self.facts.raining || mc_bamboo_blocks::sky::bees_stay_in_hive(self.facts.clock)
        })
    }

    fn enchant_mob_spawn_equipment(&self, stack: &mut ItemStack, special: f32) {
        skeleton_spawn::enchant_equipment(stack, special, self.random.source());
    }

    fn enchant_mob_spawn_equipment_from(
        &self,
        stack: &mut ItemStack,
        special: f32,
        random: &dyn mc_bamboo_blocks::random::RandomSource,
    ) {
        skeleton_spawn::enchant_equipment(stack, special, random);
    }

    fn is_halloween(&self) -> bool {
        self.facts.halloween
    }

    /// `Block.getDrops`：破门时门的下半被形状更新拆掉，掉一扇门（官方 `dropResources`）。
    fn block_loot_drops(
        &mut self,
        state: BlockState,
        pos: BlockPos,
    ) -> Vec<mc_bamboo_protocol::item::ItemStack> {
        let view = LootBlocks(&self.blocks);
        let params = mc_bamboo_loot::BlockDropParams {
            state,
            pos: (pos.x, pos.y, pos.z),
            level: &view,
            block_entity: None,
            breaker: None,
            tool: None,
            explosion_radius: None,
            luck: 0.0,
        };
        match mc_bamboo_loot::block_drops(&params, Some(&self.loot_sequences), self.random.source())
        {
            Ok(stacks) => stacks
                .iter()
                .map(|stack| mc_bamboo_protocol::item::ItemStack::new(stack.item, stack.count))
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// `LivingEntity.dropFromLootTable`：官方一侧用真的 `minecraft:entities/cow` 表，
    /// 世界种子 0 的随机序列，攻击者的主手带抢夺等级。
    ///
    /// Matches `LivingEntity.dropFromLootTable` on the official side: the real
    /// cow table, a world-seed-0 random sequence and the killer's looting level.
    fn mob_loot_drops(
        &mut self,
        loot: mc_bamboo_blocks::entity::mob::MobDeathLoot,
        origin: Vec3,
    ) -> Vec<mc_bamboo_protocol::item::ItemStack> {
        use mc_bamboo_data::registries::entity_type;
        use mc_bamboo_loot::{
            DamageSourceView, EntityDeathParams, EntityView, LootStack, Position,
            entity_death_drops,
        };
        let mut victim = EntityView::new(loot.victim.entity_type());
        victim.is_living = true;
        victim.is_on_fire = loot.on_fire;
        describe_farm_animal(&mut victim, loot.victim, loot.wool, None);
        let player = |uid| {
            let index = self.players.iter().position(|p| p.uid == uid)?;
            let mut view = EntityView::new(mc_bamboo_data::registries::entity_type::PLAYER);
            view.is_living = true;
            if let Some(&level) = self.looting.get(index)
                && level > 0
            {
                let mut held = LootStack::new(mc_bamboo_data::items::DIAMOND_SWORD, 1);
                let mut enchants = std::collections::BTreeMap::new();
                enchants.insert("minecraft:looting".to_owned(), level);
                held.set_enchantment_levels(mc_bamboo_loot::ENCHANTMENTS_COMPONENT, enchants);
                view.equipment[mc_bamboo_loot::EquipmentSlot::Mainhand.index()] = Some(held);
            }
            Some(view)
        };
        let source = loot.source;
        if loot.victim == mc_bamboo_blocks::entity::mob::MobType::Slime {
            victim.cube_mob_size = Some(loot.cube_size);
        }
        // `type_specific/raider`：没有袭击，`has_raid` 恒假；队长由生物一侧算好带过来。
        // `type_specific/raider`: no raids, so `has_raid` stays false; the mob side
        // supplies the captain flag.
        victim.is_raider = loot.victim.is_raider();
        victim.is_captain = loot.captain;
        // 生物打死生物：攻击者按伤害来源带的类型建。
        // A mob killing a mob: the attacker comes from the type carried by the source.
        let typed = source.attacker_type.map(|kind| {
            let mut view = EntityView::new(kind);
            view.is_living = true;
            view
        });
        let attacker = source.attacker.and_then(player).or(typed);
        let direct = source.direct.and_then(player).or_else(|| {
            source
                .projectile_motion
                .map(|_| EntityView::new(entity_type::ARROW))
        });
        let last_damage_player = match loot.recent_player {
            Some(uuid) => self
                .players
                .iter()
                .find(|p| p.uuid == Some(uuid))
                .map(|p| p.uid),
            None => loot.recent_player_id,
        }
        .and_then(player);
        let params = EntityDeathParams {
            entity: victim,
            origin: Position::new(origin.x, origin.y, origin.z),
            damage_source: DamageSourceView {
                damage_type: mc_bamboo_data::damage_type_id(source.kind.name()).unwrap_or_default(),
                direct_entity: direct.clone().map(Box::new),
                source_entity: attacker.clone().map(Box::new),
                is_direct: source.direct == source.attacker,
            },
            attacking_entity: attacker,
            direct_attacking_entity: direct,
            last_damage_player,
            luck: 0.0,
            level: None,
        };
        match entity_death_drops(
            loot.victim.loot_table(),
            &params,
            0,
            Some(&self.loot_sequences),
            self.random.source(),
        ) {
            Ok(stacks) => stacks
                .iter()
                .map(|stack| mc_bamboo_protocol::item::ItemStack::new(stack.item, stack.count))
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    fn entity_loot_drops(
        &mut self,
        request: mc_bamboo_blocks::entity::mob::EntityLootRequest,
    ) -> Vec<mc_bamboo_protocol::item::ItemStack> {
        use mc_bamboo_loot::{EntityLootParams, EntityView, LootStack, Position};
        let mut entity = EntityView::new(request.kind.entity_type());
        entity.is_living = true;
        let origin = Position::new(request.origin.x, request.origin.y, request.origin.z);
        entity.position = Some(origin);
        describe_farm_animal(
            &mut entity,
            request.kind,
            request.wool,
            Some(request.variant),
        );
        let params = EntityLootParams {
            entity,
            origin,
            tool: request
                .tool
                .as_ref()
                .map(|stack| LootStack::new(stack.item, stack.count)),
            level: None,
        };
        match mc_bamboo_loot::entity_loot_drops(
            request.table,
            &params,
            Some(&self.loot_sequences),
            self.random.source(),
        ) {
            Ok(stacks) => stacks
                .iter()
                .map(|stack| mc_bamboo_protocol::item::ItemStack::new(stack.item, stack.count))
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    fn player_mob_attack(
        &self,
        uid: i32,
    ) -> Option<mc_bamboo_blocks::entity::mob::PlayerMobAttack> {
        let index = self.players.iter().position(|player| player.uid == uid)?;
        Some(mc_bamboo_blocks::entity::mob::PlayerMobAttack {
            uuid: self.players[index].uuid?,
            looting: self.looting.get(index).copied().unwrap_or(0),
        })
    }
}

/// 方块掉落的战利品表能读到的那点世界：假世界的方块表，晴天，主世界。
struct LootBlocks<'a>(&'a HashMap<BlockPos, BlockState>);

impl mc_bamboo_loot::LevelView for LootBlocks<'_> {
    fn is_loaded(&self, _: i32, _: i32, _: i32) -> Option<bool> {
        Some(true)
    }
    fn block_state(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        Some(
            self.0
                .get(&BlockPos::new(x, y, z))
                .copied()
                .unwrap_or(BlockState(mc_bamboo_data::blocks::AIR)),
        )
    }
    fn biome(&self, _: i32, _: i32, _: i32) -> Option<i32> {
        None
    }
    fn can_see_sky(&self, _: i32, _: i32, _: i32) -> Option<bool> {
        None
    }
    fn is_raining(&self) -> Option<bool> {
        Some(false)
    }
    fn is_thundering(&self) -> Option<bool> {
        Some(false)
    }
    fn day_time(&self) -> Option<i64> {
        None
    }
    fn game_time(&self) -> Option<i64> {
        None
    }
    fn dimension(&self) -> Option<&'static str> {
        Some("minecraft:overworld")
    }
}

struct Report {
    cases: u32,
    steps: usize,
    skeleton_spawns: skeleton_spawn::Coverage,
    natural_spawns: natural_spawn::Coverage,
    patrol_spawns: patrol_spawn::Coverage,
    structure_spawns: structure_spawn::Coverage,
    gen_spawns: gen_spawn::Coverage,
    mismatches: Vec<String>,
    ended: bool,
    show: usize,
    /// 故意改坏一处，确认对拍真的抓得到。
    break_test: Option<String>,
}

/// 启动官方进程，逐个场景重放。
///
/// 玩家场景的重放函数在 debug 构建里一帧就有几十 KB，主线程默认的 1 MB 栈不够，
/// 所以挪到一条大栈线程上跑。
///
/// Runs the official process and replays every scene. The player replays use frames
/// dozens of KB deep in debug builds, which overflows the main thread's default 1 MB
/// stack, so they run on a thread with a larger one.
pub fn run(command: Command, options: &[&str]) -> Result<(), String> {
    let options: Vec<String> = options.iter().map(|option| (*option).to_owned()).collect();
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            let options: Vec<&str> = options.iter().map(String::as_str).collect();
            run_with_options(command, &options)
        })
        .map_err(|error| format!("cannot start the oracle thread: {error}"))?
        .join()
        .map_err(|_| "the oracle thread panicked".to_owned())?
}

fn run_with_options(mut command: Command, options: &[&str]) -> Result<(), String> {
    let mut show = 12;
    let mut break_test: Option<String> = None;
    let mut index = 0;
    while index < options.len() {
        match options[index] {
            "--only" if index + 1 < options.len() => {
                command.arg("--only").arg(options[index + 1]);
                index += 2;
            }
            "--show" if index + 1 < options.len() => {
                show = options[index + 1].parse().map_err(|_| USAGE.to_owned())?;
                index += 2;
            }
            "--break-self-test" if index + 1 < options.len() => {
                break_test = Some(options[index + 1].to_owned());
                index += 2;
            }
            _ => return Err(USAGE.to_owned()),
        }
    }
    command.env(
        "JAVA_TOOL_OPTIONS",
        "-Dstdout.encoding=UTF-8 -Dstderr.encoding=UTF-8",
    );
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut report = Report {
        cases: 0,
        steps: 0,
        skeleton_spawns: skeleton_spawn::Coverage::default(),
        natural_spawns: natural_spawn::Coverage::default(),
        patrol_spawns: patrol_spawn::Coverage::default(),
        structure_spawns: structure_spawn::Coverage::default(),
        gen_spawns: gen_spawn::Coverage::default(),
        mismatches: Vec::new(),
        ended: false,
        show,
        break_test,
    };
    let read = report.read(BufReader::new(stdout));
    if read.is_err() {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    read.map_err(|error| format!("{error}\n{log}"))?;
    if !status.success() {
        return Err(format!("MobOracle.java failed: {status}\n{log}"));
    }
    report.finish()
}

impl Report {
    fn read(&mut self, source: BufReader<impl Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            if line.is_empty() {
                continue;
            }
            let parsed: Line = serde_json::from_str(&line).map_err(|error| {
                format!(
                    "bad mob oracle line: {error}: {}",
                    &line[..line.len().min(200)]
                )
            })?;
            match parsed {
                Line::Header(header) => self.check_header(&header)?,
                Line::Case(case) => self.replay(&case),
                Line::Projectile(case) => self.replay_projectile(&case),
                Line::Shot(case) => self.replay_shot(&case),
                Line::Egg(case) => self.replay_egg(&case),
                Line::EggThrow(case) => self.replay_egg_throw(&case),
                Line::EggDispense(case) => self.replay_egg_dispense(&case),
                Line::Squid(case) => self.replay_squid(&case),
                Line::Bat(case) => self.replay_bat(&case),
                Line::Lightning(case) => self.replay_lightning(&case),
                Line::SkeletonTrap(case) => self.replay_trap(&case),
                Line::ThunderTarget(case) => self.replay_thunder_target(&case),
                Line::ThunderHit(case) => self.replay_thunder_hit(&case),
                Line::TrapGear(case) => self.replay_trap_gear(&case),
                Line::UndeadHorse(case) => self.replay_undead_horse(&case),
                Line::UndeadHorseSave(case) => self.replay_undead_horse_save(&case),
                Line::UndeadHorseDeath(case) => self.replay_undead_horse_death(&case),
                Line::Bee(case) => self.replay_bee(&case),
                Line::BeeAi(case) => self.replay_bee_ai(&case),
                Line::BeeRelease(case) => self.replay_bee_release(&case),
                Line::Golem(case) => self.replay_golem(&case),
                Line::GolemAttackRoll(case) => self.replay_golem_attack_roll(&case),
                Line::GolemSave(case) => self.replay_golem_save(&case),
                Line::SmallMob(case) => self.replay_small_mob(&case),
                Line::SmallMobSave(case) => self.replay_small_mob_save(&case),
                Line::SmallMobDeath(case) => self.replay_small_mob_death(&case),
                Line::Fish(case) => self.replay_fish(&case),
                Line::PhantomSpawn(case) => self.replay_phantom_spawn(&case),
                Line::Phantom(case) => self.replay_phantom(&case),
                Line::WaterMob(case) => self.replay_water_mob(&case),
                Line::AxolotlBreed(case) => self.replay_axolotl_breed(&case),
                Line::ArrowPickup(case) => self.replay_arrow_pickup(&case),
                Line::SkeletonDeath(case) => self.replay_skeleton_death(&case),
                Line::SkeletonSpawn(case) => self.replay_skeleton_spawn(&case),
                Line::EquineSpawn(case) => self.replay_equine_spawn(case),
                Line::EquineGroup(case) => self.replay_equine_group(case),
                Line::EquineBreed(case) => self.replay_equine_breed(case),
                Line::EquineInteract(case) => self.replay_equine_interact(case),
                Line::EquineDeath(case) => self.replay_equine_death(case),
                Line::EquineSave(case) => self.replay_equine_save(case),
                Line::LlamaSpit(case) => self.replay_llama_spit(&case),
                Line::WitchThrow(case) => self.replay_witch_throw(&case),
                Line::NaturalSpawn(case) => self.replay_natural_spawn(&case)?,
                Line::PatrolSpawn(case) => self.replay_patrol_spawn(&case)?,
                Line::StructureSpawn(case) => self.replay_structure_spawn(&case)?,
                Line::GenSpawn(case) => self.replay_gen_spawn(&case),
                Line::PandaGenes(case) => self.replay_panda_genes(&case),
                Line::End(end) => {
                    self.ended = true;
                    if end.cases != self.cases {
                        return Err(format!(
                            "java wrote {} cases, {} arrived",
                            end.cases, self.cases
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn check_header(&mut self, header: &Header) -> Result<(), String> {
        if header.version != version::GAME_VERSION || header.protocol != version::PROTOCOL_VERSION {
            return Err(format!(
                "the generated data is for {} (protocol {}), the official jar is {} (protocol {})",
                version::GAME_VERSION,
                version::PROTOCOL_VERSION,
                header.version,
                header.protocol,
            ));
        }
        let [sin, atan] = table_hashes();
        if sin != header.tables[0] {
            self.mismatch(format!(
                "Mth.SIN table hash: official {}, bamboo {sin}",
                header.tables[0]
            ));
        }
        if std::env::var_os("MOB_ORACLE_DUMP").is_some() {
            eprintln!(
                "ASIN {:?}",
                header.asin.iter().map(|v| *v as u64).collect::<Vec<_>>()
            );
            eprintln!(
                "COS {:?}",
                header.cos.iter().map(|v| *v as u64).collect::<Vec<_>>()
            );
        }
        let (asin, cos) = mc_bamboo_core::mth::atan_tables();
        for (index, (&ours, &official)) in asin.iter().zip(&header.asin).enumerate() {
            if ours.to_bits() as i64 != official {
                self.mismatch(format!(
                    "Mth.ASIN_TAB[{index}]: official {}, bamboo {ours}",
                    f64::from_bits(official as u64)
                ));
            }
        }
        for (index, (&ours, &official)) in cos.iter().zip(&header.cos).enumerate() {
            if ours.to_bits() as i64 != official {
                self.mismatch(format!(
                    "Mth.COS_TAB[{index}]: official {}, bamboo {ours}",
                    f64::from_bits(official as u64)
                ));
            }
        }
        if atan != header.tables[1] {
            self.mismatch(format!(
                "Mth.ASIN_TAB/COS_TAB hash: official {}, bamboo {atan}",
                header.tables[1]
            ));
        }
        Ok(())
    }

    fn replay(&mut self, case: &Case) {
        self.cases += 1;
        if case.spawn_result.is_some() {
            self.replay_spawn_landing(case);
            return;
        }
        if case.life.is_some() {
            self.replay_player_life(case);
            return;
        }
        if case.effect_scene.is_some() {
            self.replay_player_effects(case);
            return;
        }
        if case.env_scene.is_some() {
            self.replay_player_env(case);
            return;
        }
        if case.consume_scene.is_some() {
            self.replay_player_consume(case);
            return;
        }
        if case.player_steps.is_some() {
            self.replay_player_hurt(case);
            return;
        }
        // 吃草场景先确认官方那一侧真的发生了场景想测的事。
        // Grazing scenes first confirm the official side did what the scene is for.
        if let Some(steps) = &case.block_steps
            && let Some(error) = grazing::coverage_error(&case.name, &case.blocks, steps)
        {
            self.mismatch(error);
        }
        // 改坏确认：故意改一处生产参数，对拍必须报不一致。
        let break_test = self.break_test.as_deref();
        let mut level = MobLevel {
            blocks: HashMap::new(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: case.players.iter().map(|player| player.looting).collect(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                halloween: false,
                clock: 0,
                world_seed: 0,
                inhabited_time: 0,
                accumulates_inhabited: case.name.starts_with("zombie_inhabited_"),
                can_see_sky: case.monsters_burn,
                snow_golem_melts: false,
                sky_darken: case.sky_darken,
                difficulty: case.difficulty,
                bright_outside: case.bright_outside || !zombie_style(&case.name),
                monsters_burn: if zombie_style(&case.name) {
                    case.monsters_burn
                } else {
                    true
                },
                sea_level: case.sea_level,
                raining: case.raining,
                bees_stay: None,
            },
            natural_spawn: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(case.first_id),
            players: case
                .players
                .iter()
                .enumerate()
                .map(|(index, player)| PlayerView {
                    // 官方一侧的玩家 UUID 是 (1, 2 + 下标)；狼认主人要用它。
                    // The official players use UUID (1, 2 + index); wolves match owners by it.
                    uuid: Some(((1_u128 << 64) | (2 + index as u128)).to_be_bytes()),
                    uid: player.id,
                    yaw: 0.0,
                    pitch: 0.0,
                    position: Vec3::new(
                        f64::from_bits(player.x as u64),
                        f64::from_bits(player.y as u64),
                        f64::from_bits(player.z as u64),
                    ),
                    delta: Vec3::default(),
                    eye_height: f32::from_bits(player.eye as u32),
                    health: 20.0,
                    look_direction: Vec3::view_vector(
                        f32::from_bits(player.yaw as u32),
                        f32::from_bits(player.pitch as u32),
                    ),
                    gaze_disguise: false,
                    alive: true,
                    spectator: false,
                    creative: false,
                    sneaking: false,
                    sprinting: false,
                    swimming: false,
                    invisible: false,
                    armor_cover: 0.0,
                    main_hand: player.item,
                    off_hand: 0,
                    team: u32::try_from(player.team).ok(),
                    combat: Default::default(),
                    effects: Default::default(),
                })
                .collect(),
            game: case.real_players.then(|| {
                let health = f32::from_bits(case.player_health as u32);
                let setups: Vec<_> = case
                    .players
                    .iter()
                    .enumerate()
                    .map(|(index, player)| mc_bamboo_game::ReplayPlayerSetup {
                        id: player.id,
                        uuid: (1_u128 << 64) | (2 + index as u128),
                        name: format!("ZombieOracle{index}"),
                        position: Vec3::new(
                            f64::from_bits(player.x as u64),
                            f64::from_bits(player.y as u64),
                            f64::from_bits(player.z as u64),
                        ),
                        health,
                        seed: 0x0DDB_1A5E_5BAD_5EED_i64.wrapping_add(index as i64),
                        creative: index == 0 && case.player_creative,
                    })
                    .collect();
                mc_bamboo_game::ReplayPlayers::new(case.difficulty, &setups)
            }),
            inhabited: Default::default(),
            mob_teams: if matches!(break_test, Some("zombie_team")) {
                // 改坏确认：生物不进队伍，同队的玩家也会被当成目标。
                HashMap::new()
            } else {
                case.cows
                    .iter()
                    .enumerate()
                    .filter_map(|(index, cow)| {
                        let team = u32::try_from(cow.team).ok()?;
                        Some(((index as u128 + 1).to_be_bytes(), team))
                    })
                    .collect()
            },
            ignore_bed_monsters: matches!(break_test, Some("zombie_bed")),
            short_hunger: matches!(break_test, Some("zombie_hunger")),
            wrong_was_hurt: matches!(break_test, Some("zombie_was_hurt")),
        };
        for &[x, y, z, state] in &case.blocks {
            level
                .blocks
                .insert(BlockPos::new(x, y, z), BlockState(state as u32));
        }
        if case.name.ends_with("_no_grief") {
            // 和 `MobOracle.play` 一样按场景名关掉 `mobGriefing`。
            // Turn `mobGriefing` off by scene name, like `MobOracle.play`.
            level.rules.mob_griefing = false;
        }
        if matches!(break_test, Some("zombie_egg")) {
            // 改坏确认：关掉 `mobGriefing`，海龟蛋既不会被踩坏也不会被啄碎。
            level.rules.mob_griefing = false;
        }
        let mut ticks = BlockTicks::new();
        ticks.set_time(case.start_time);
        let mut ids = Vec::new();
        let mut tracking = EntityTracking::default();
        let sound_source = mc_bamboo_blocks::LegacyRandom::new(12345);
        {
            let mut world = World::new(&mut level, &mut ticks);
            for (index, cow) in case.cows.iter().enumerate() {
                let position = Vec3::new(
                    f64::from_bits(cow.x as u64),
                    f64::from_bits(cow.y as u64),
                    f64::from_bits(cow.z as u64),
                );
                let yaw = f32::from_bits(cow.yaw as u32);
                // 场景编号（两侧同一套，别撞号）：1 僵尸、2 尸壳、3 猪、4 羊、5 鸡、
                // 6 兔子、7 狐狸、8 狼、9..=12 马科、13 犰狳、14 山羊、15 熊猫、
                // 16 北极熊、17 哞菇、18 青蛙、19 蝌蚪、20 骆驼、
                // 40..=43 敌对生物、60 海龟，其余是牛。
                // Scene kind ids (one shared set, no collisions): 1 zombie, 2 husk,
                // 3 pig, 4 sheep, 5 chicken, 6 rabbit, 7 fox, 8 wolf, 9..=12 equines,
                // 13 armadillo, 14 goat, 15 panda, 16 polar bear, 17 mooshroom,
                // 18 frog, 19 tadpole, 20 camel, 40..=43 hostiles, 60 turtle;
                // anything else is a cow.
                let kind = match cow.kind {
                    1 => mc_bamboo_blocks::entity::mob::MobType::Zombie,
                    2 => mc_bamboo_blocks::entity::mob::MobType::Husk,
                    3 => mc_bamboo_blocks::entity::mob::MobType::Pig,
                    4 => mc_bamboo_blocks::entity::mob::MobType::Sheep,
                    5 => mc_bamboo_blocks::entity::mob::MobType::Chicken,
                    6 => mc_bamboo_blocks::entity::mob::MobType::Rabbit,
                    7 => mc_bamboo_blocks::entity::mob::MobType::Fox,
                    8 => mc_bamboo_blocks::entity::mob::MobType::Wolf,
                    9..=12 => equine::scene_kind(cow.kind),
                    13 => mc_bamboo_blocks::entity::mob::MobType::Armadillo,
                    14 => mc_bamboo_blocks::entity::mob::MobType::Goat,
                    18 => mc_bamboo_blocks::entity::mob::MobType::Frog,
                    19 => mc_bamboo_blocks::entity::mob::MobType::Tadpole,
                    20 => mc_bamboo_blocks::entity::mob::MobType::Camel,
                    15 => mc_bamboo_blocks::entity::mob::MobType::Panda,
                    16 => mc_bamboo_blocks::entity::mob::MobType::PolarBear,
                    17 => mc_bamboo_blocks::entity::mob::MobType::Mooshroom,
                    21 => mc_bamboo_blocks::entity::mob::MobType::Ocelot,
                    22 => mc_bamboo_blocks::entity::mob::MobType::Cat,
                    23 => mc_bamboo_blocks::entity::mob::MobType::Parrot,
                    44 => mc_bamboo_blocks::entity::mob::MobType::Skeleton,
                    40 => mc_bamboo_blocks::entity::mob::MobType::Creeper,
                    41 => mc_bamboo_blocks::entity::mob::MobType::Spider,
                    42 => mc_bamboo_blocks::entity::mob::MobType::CaveSpider,
                    43 => mc_bamboo_blocks::entity::mob::MobType::Enderman,
                    50 => mc_bamboo_blocks::entity::mob::MobType::Slime,
                    51 => mc_bamboo_blocks::entity::mob::MobType::Witch,
                    52 => mc_bamboo_blocks::entity::mob::MobType::Drowned,
                    53 => mc_bamboo_blocks::entity::mob::MobType::Pillager,
                    60 => mc_bamboo_blocks::entity::mob::MobType::Turtle,
                    _ => mc_bamboo_blocks::entity::mob::MobType::Cow,
                };
                // 幼年标志和破门是僵尸一系专属的起始状态。
                // The baby flag and door breaking are zombie-only start state.
                let zombie_kind = matches!(
                    kind,
                    mc_bamboo_blocks::entity::mob::MobType::Zombie
                        | mc_bamboo_blocks::entity::mob::MobType::Husk
                );
                let id = world.spawn_entity(Entity::by_mob_type(kind, position));
                world.with_entity_mut(id, &mut |entity| {
                    entity.random = JavaRandom::new(cow.seed);
                    entity.y_rot = yaw;
                    entity.old_position = position;
                    if let Some(mob) = entity.kind.mob_mut() {
                        // 和官方一侧一样，场景里的牛 UUID 是 (0, 下标 + 1)。
                        mob.uuid = (index as u128 + 1).to_be_bytes();
                        mob.x_rot = 0.0;
                        mob.y_rot_o = yaw;
                        mob.x_rot_o = 0.0;
                        mob.y_head_rot = yaw;
                        mob.y_body_rot = yaw;
                        if kind == mc_bamboo_blocks::entity::mob::MobType::Mooshroom {
                            // 哞菇场景借 wool 字段选品种。
                            // Mooshroom scenes reuse the wool field for the type.
                            mob.mooshroom.brown = cow.wool != 0;
                        } else {
                            mob.wool = cow.wool;
                        }
                        brain::setup(
                            entity.random.state(),
                            mob,
                            &cow.sensors,
                            cow.scute,
                            break_test,
                        );
                        // 鸡的蛋计时由 Java 一侧报过来（构造时抽的值）；缺省保留 Bamboo 的。
                        // The chicken's egg timer comes from Java (drawn in its constructor).
                        if cow.egg > 0 {
                            mob.egg_time = cow.egg;
                        }
                        if cow.temper >= 0
                            && let Some(state) = mob.equine.as_mut()
                        {
                            state.temper = cow.temper;
                        }
                        if kind == mc_bamboo_blocks::entity::mob::MobType::Panda {
                            use mc_bamboo_blocks::entity::mob::panda::Gene;
                            let main = cow.genes.first().copied().unwrap_or(0);
                            let hidden = cow.genes.get(1).copied().unwrap_or(0);
                            mob.panda.main_gene = Gene::from_id(i32::from(main));
                            mob.panda.hidden_gene = Gene::from_id(i32::from(hidden));
                            if cow.held != 0 {
                                mob.equipment.set(
                                    mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand,
                                    ItemStack::new(cow.held, 1),
                                );
                            }
                        }
                        pillager::setup(&case.name, index, mob);
                        mob.creeper.powered = cow.creeper & 1 != 0;
                        mob.creeper.ignited = cow.creeper & 2 != 0;
                        if cow.carried != 0 {
                            mob.set_carried_block(Some(BlockState(cow.carried as u32)));
                        }
                        // 海龟场景：把开局带蛋和家方块写进状态，和官方那一侧一致。
                        // Turtle scenes: seed the starting has-egg flag and home position,
                        // matching the official side.
                        if kind == mc_bamboo_blocks::entity::mob::MobType::Turtle {
                            mob.turtle.home_pos = BlockPos::new(
                                cow.turtle_home[0],
                                cow.turtle_home[1],
                                cow.turtle_home[2],
                            );
                            mob.turtle.has_egg = cow.turtle_egg;
                            // 改坏确认：把带蛋标志取反，产卵场景从第一刻起状态就不一致。
                            // Break-self-test: flip the has-egg flag so the lay scenes mismatch.
                            if matches!(break_test, Some("turtle_egg")) {
                                mob.turtle.has_egg = !mob.turtle.has_egg;
                            }
                        }
                    }
                    entity.remaining_fire_ticks = case.fire_ticks;
                    if let Some(mob) = entity.kind.mob_mut()
                        && kind == mc_bamboo_blocks::entity::mob::MobType::Fox
                    {
                        mc_bamboo_blocks::entity::mob::fox::set_sleep_countdown(mob, cow.sleep);
                        if case.fox_hunts {
                            mc_bamboo_blocks::entity::mob::fox::set_target_goals(mob);
                        }
                    }
                    let slime = kind == mc_bamboo_blocks::entity::mob::MobType::Slime;
                    if slime {
                        // 史莱姆场景用「年龄」那一列带大小；移动控制的目标角度对齐成场景的朝向。
                        // Slime scenes carry the size in the age column; the move control's
                        // target angle is aligned with the scene yaw.
                        entity.with_mob(|me, mob| {
                            mc_bamboo_blocks::entity::mob::slime::set_size(me, mob, cow.age, true);
                            mob.cube.move_y_rot = yaw;
                        });
                        entity.y_rot = yaw;
                        if let Some(mob) = entity.kind.mob_mut() {
                            mob.y_body_rot = yaw;
                        }
                    }
                    let tadpole = kind == mc_bamboo_blocks::entity::mob::MobType::Tadpole;
                    if tadpole {
                        // 蝌蚪的「年龄」那一列是它自己的 `Tadpole.age`。
                        // For tadpoles the age column is their own `Tadpole.age`.
                        entity.with_mob(|_, mob| mob.tadpole_age = cow.age);
                    }
                    if cow.age != 0 && !slime && !tadpole {
                        // 牛按 `AgeableMob` 的负年龄判幼年；僵尸场景沿用
                        // Java 一侧的 `setBaby(age != 0)`，正数也表示幼年。
                        let baby = zombie_kind || cow.age < 0;
                        let (width, height, eye) = kind.dimensions(baby);
                        entity.dimensions = (width, height);
                        if let Some(mob) = entity.kind.mob_mut() {
                            if zombie_kind {
                                mob.set_zombie_baby(true);
                                mob.can_break_doors =
                                    case.difficulty == 3 && !self.no_break_doors();
                                if mob.can_break_doors {
                                    mob.navigation.flags.can_open_doors = true;
                                    mob.goals.insert_break_door();
                                }
                            } else {
                                mob.age = cow.age;
                            }
                            if kind == mc_bamboo_blocks::entity::mob::MobType::Goat {
                                // `setAge` 跨过 0 时 `Goat.ageBoundaryReached` 换攻击力。
                                // Crossing zero in `setAge` runs `Goat.ageBoundaryReached`.
                                mc_bamboo_blocks::entity::mob::goat::age_boundary_reached(mob);
                            }
                            mob.eye_height = eye;
                        }
                    } else if zombie_kind
                        && case.difficulty == 3
                        && !self.no_break_doors()
                        && let Some(mob) = entity.kind.mob_mut()
                    {
                        mob.can_break_doors = true;
                        mob.navigation.flags.can_open_doors = true;
                        mob.goals.insert_break_door();
                    }
                });
                if case.name.starts_with("zombie_") {
                    world.with_entity_mut(id, &mut |entity| {
                        if let Some(mob) = entity.kind.mob_mut() {
                            if matches!(break_test, Some("zombie_goal_order")) {
                                // 改坏确认：按优先级排序会打乱官方注册顺序。
                                mob.goals.sort_by_priority_for_oracle();
                            }
                            match break_test {
                                Some("zombie_speed") => {
                                    mob.attributes.set_base_for_oracle(
                                        mc_bamboo_data::registries::attribute::MOVEMENT_SPEED,
                                        0.2,
                                    );
                                }
                                Some("zombie_attack") => {
                                    mob.attributes.set_base_for_oracle(
                                        mc_bamboo_data::registries::attribute::ATTACK_DAMAGE,
                                        2.0,
                                    );
                                }
                                Some("zombie_burn") => {
                                    // 燃烧阈值改成永不触发。
                                    mob.ticks_frozen = 140;
                                }
                                Some("zombie_xp") => {
                                    mob.health = 1.0;
                                }
                                Some("zombie_baby") => {
                                    mob.set_zombie_baby(!mob.zombie_baby);
                                }
                                _ => {}
                            }
                            if case.helmet != 0 {
                                let mut helmet =
                                    mc_bamboo_protocol::item::ItemStack::new(case.helmet, 1);
                                // 改坏确认：头盔多掉一点耐久，烧坏的时刻和装备包都会提前。
                                let damage = case.helmet_damage
                                    + i32::from(matches!(break_test, Some("zombie_helmet")));
                                if damage != 0 {
                                    mc_bamboo_blocks::durability::set_damage_for_oracle(
                                        &mut helmet,
                                        damage,
                                    );
                                }
                                mob.equipment.set(
                                    mc_bamboo_protocol::packet::play::EquipmentSlot::Head,
                                    helmet,
                                );
                            }
                        }
                    });
                }
                if case.persisted || case.no_ai {
                    // 骑乘场景：和官方 `setPersistenceRequired` / `setNoAi` 一样设置。
                    world.with_entity_mut(id, &mut |entity| {
                        if let Some(mob) = entity.kind.mob_mut() {
                            if case.persisted {
                                mob.persistence_required = true;
                            }
                            if case.no_ai {
                                mob.no_ai = true;
                            }
                        }
                    });
                }
                ids.push(id);
            }
            // 骑乘场景：官方 `startRiding` 之后，0 号生物身上坐着这名玩家。
            //
            // Riding scene: after vanilla's `startRiding` this player sits on mob 0.
            if case.rider >= 0
                && let Some(&ridden) = ids.first()
                && let Some(player) = case.players.get(case.rider as usize)
            {
                let rider = player.id;
                world.with_entity_mut(ridden, &mut |entity| {
                    if let Some(mob) = entity.kind.mob_mut() {
                        mob.passengers.push(rider);
                    }
                });
            }
            // 骑乘场景：官方 `startRiding(vehicle, false, false)`。
            // Riding scenes: vanilla `startRiding(vehicle, false, false)`.
            if case.ride_passenger >= 0
                && case.ride_vehicle >= 0
                && let (Some(&passenger), Some(&vehicle)) = (
                    ids.get(case.ride_passenger as usize),
                    ids.get(case.ride_vehicle as usize),
                )
            {
                world.start_riding(passenger, vehicle);
            }
        }
        let Some(first) = case.steps.first() else {
            self.mismatch(format!("{}: no steps", case.name));
            return;
        };
        set_zombie_clock(&mut level, case, 0, break_test);
        accumulate_inhabited(&mut level, 0, self.break_test.as_deref());
        let ours = snapshot(&level, &ids);
        self.compare(&case.name, 0, &ours, first);
        self.compare_zombie_player(case, &mut level, 0);
        // 场景开始时的牛也算「刚生成」，但它们在 tick 0 已经建过同步状态，
        // 不该出现在后面逐刻的新实体比对里。
        let _ = level.entities.take_spawn_snapshots();
        // 已经发过 `remove_entities` 的牛，按 `ids` 下标。
        let mut removed_ids = std::collections::HashSet::new();
        // 出站包逐刻比字节：官方一侧每头牛一个 `ServerEntity`，这里用游戏层
        // 同一份 `TrackedEntity` 逻辑，绝不在对拍里另写一份发包。
        if let Some(packets) = &case.packets {
            let flags = case.sync_flags.as_ref().expect("有包就有标志位");
            let tick0 = flags.first().expect("第 0 刻的标志位");
            let mut ours_packets = Vec::new();
            for (cow, &id) in ids.iter().enumerate() {
                let Some(public) = level.entities.public_at(id) else {
                    continue;
                };
                let mut snapshot = Snapshot::of(public);
                apply_flags(
                    &mut snapshot,
                    tick0.get(cow).map(String::as_str).unwrap_or("00000"),
                );
                let tracked = tracking.tracked_for_oracle(&snapshot);
                ours_packets.extend(tracked.pairing_packets(&snapshot));
            }
            compare_packet_list(
                &mut self.mismatches,
                &self.show,
                &case.name,
                0,
                &ours_packets,
                &packets[0],
            );
        }
        for (index, expected) in case.steps.iter().enumerate().skip(1) {
            ticks.set_time(case.start_time + index as i64);
            set_zombie_clock(&mut level, case, index as i64, self.break_test.as_deref());
            accumulate_inhabited(&mut level, index as i64, self.break_test.as_deref());
            for &[tick, x, y, z, state] in &case.place_at {
                if tick as usize == index {
                    level
                        .blocks
                        .insert(BlockPos::new(x, y, z), BlockState(state as u32));
                }
            }
            if case.sleep_tick >= 0
                && case.sleep_tick as usize == index
                && let Some([x, y, z]) = case.bed
            {
                let result = try_sleep(&mut level, &mut ticks, case, BlockPos::new(x, y, z));
                if result != case.sleep_result {
                    self.mismatches.push(format!(
                        "{} tick {index} sleep: official {:?} bamboo {result:?}",
                        case.name, case.sleep_result
                    ));
                }
            }
            if matches!(self.break_test.as_deref(), Some("zombie_sight")) {
                // 改坏确认：视线记忆永不过期，僵尸隔着墙也不放弃目标。
                let mut world = World::new(&mut level, &mut ticks);
                for &id in &ids {
                    world.with_entity_mut(id, &mut |entity| {
                        if let Some(mob) = entity.kind.mob_mut() {
                            for wrapped in &mut mob.target_goals.goals {
                                if let mc_bamboo_blocks::entity::mob::goals::Goal::NearestAttackableTarget {
                                    unseen_ticks,
                                    ..
                                } = &mut wrapped.goal
                                {
                                    *unseen_ticks = 0;
                                }
                            }
                        }
                    });
                }
            }
            if let Some(game) = level.game.as_mut() {
                game.tick_cooldowns();
            }
            brain::set_sprint(&mut level.players, case.sprint, index as i32);
            let mut ours_interactions = Vec::new();
            let mut seeded_early = Vec::new();
            let tick_effects;
            {
                let mut world = World::new(&mut level, &mut ticks);
                for &[tick, cow, kind, effect, duration, amplifier] in &case.cow_effect_actions {
                    if tick != index as i32 || cow as usize >= ids.len() {
                        continue;
                    }
                    let uid = ids[cow as usize];
                    // 改坏确认：故意改掉效果的时长 / 等级 / 种类，逐刻位置和速度会不同。
                    let (effect, duration, amplifier) = match self.break_test.as_deref() {
                        Some("cow_effect_duration") => {
                            (effect, duration.saturating_add(20), amplifier)
                        }
                        Some("cow_effect_amplifier") => {
                            (effect, duration, amplifier.saturating_add(1))
                        }
                        Some("cow_effect_kind") => (0, duration, amplifier),
                        _ => (effect, duration, amplifier),
                    };
                    if kind == 0 {
                        world.add_mob_effect(
                            uid,
                            mc_bamboo_blocks::entity::mob::effects::EffectInstance::with_amplifier(
                                effect, duration, amplifier,
                            ),
                        );
                    } else {
                        world.clear_mob_effects(uid, None);
                    }
                }
                for &[tick, cow, player_index, item, creative] in &case.interact_actions {
                    if tick != index as i32 {
                        continue;
                    }
                    let player = &case.players[player_index as usize];
                    let mut uuid = [0u8; 16];
                    uuid[..8].copy_from_slice(&1_u64.to_be_bytes());
                    uuid[8..].copy_from_slice(&(2_u64 + player_index as u64).to_be_bytes());
                    let held = if item == 0 {
                        mc_bamboo_protocol::item::ItemStack::empty()
                    } else {
                        mc_bamboo_protocol::item::ItemStack::new(item, 1)
                    };
                    let request = mc_bamboo_blocks::entity::mob::interact::MobInteractRequest {
                        player_uid: player.id,
                        player_uuid: uuid,
                        hand: mc_bamboo_core::Hand::Main,
                        held: held.clone(),
                        creative: creative != 0,
                        position: Vec3::new(
                            f64::from_bits(player.x as u64),
                            f64::from_bits(player.y as u64),
                            f64::from_bits(player.z as u64),
                        ),
                        sneaking: false,
                    };
                    let outcome = world.interact_mob(ids[cow as usize], &request);
                    let result = if outcome.result.consumes_action() {
                        "Success"
                    } else {
                        "Pass"
                    };
                    // 官方 `usePlayerItem` / `ItemUtils.createFilledResult` 之后手里剩什么。
                    let after = if let Some(replacement) = &outcome.replacement {
                        if creative != 0 {
                            held.clone()
                        } else {
                            replacement.clone()
                        }
                    } else if outcome.shrink_held {
                        mc_bamboo_protocol::item::ItemStack::empty()
                    } else {
                        held.clone()
                    };
                    ours_interactions.push(Interaction {
                        result: result.to_owned(),
                        item: if after.is_empty() { 0 } else { after.item },
                        count: after.count,
                    });
                }
                if case.name.starts_with("slime_") {
                    touch_slimes(&mut world, &ids, &case.players);
                }
                for &[tick, cow, amount, attacker] in &case.hurts {
                    if tick as usize != index {
                        continue;
                    }
                    // 负数攻击者编号是场景注入的环境 / 投掷物伤害：-1 箭，-2 溺水。
                    // A negative attacker index injects environment or projectile damage:
                    // -1 an arrow, -2 drowning.
                    let source = if attacker < 0 {
                        let kind = if attacker == -1 {
                            mc_bamboo_blocks::entity::mob::DamageKind::Arrow
                        } else {
                            mc_bamboo_blocks::entity::mob::DamageKind::Drown
                        };
                        DamageSource::environment(kind)
                    } else {
                        let player = &case.players[attacker as usize];
                        let position = Vec3::new(
                            f64::from_bits(player.x as u64),
                            f64::from_bits(player.y as u64),
                            f64::from_bits(player.z as u64),
                        );
                        match case.death_kind {
                            1 => DamageSource::environment(
                                mc_bamboo_blocks::entity::mob::DamageKind::Fall,
                            ),
                            2 => DamageSource::environment(
                                mc_bamboo_blocks::entity::mob::DamageKind::OnFire,
                            ),
                            _ => DamageSource::player(player.id, position, false),
                        }
                    };
                    world.hurt_mob(ids[cow as usize], source, f32::from_bits(amount as u32));
                }
                for summon in &case.summons {
                    if summon.tick != index {
                        continue;
                    }
                    let position = Vec3::new(
                        f64::from_bits(summon.x as u64),
                        f64::from_bits(summon.y as u64),
                        f64::from_bits(summon.z as u64),
                    );
                    let nbt = summon
                        .nbt
                        .as_deref()
                        .map(|text| mc_bamboo_nbt::parse_snbt(text).expect("scene SNBT parses"));
                    if let Ok(id) = spawn::summon_cow(&mut world, position, nbt.as_ref()) {
                        ids.push(id);
                    }
                }
                // 官方 `ChunkMap.addEntity` 在实体刚进世界时建 `ServerEntity`：
                // 配对包的速度取生成那一刻的速度，不是这一刻 tick 之后的速度。
                // Vanilla's `ChunkMap.addEntity` creates `ServerEntity` as the
                // entity joins the world, before this tick's entity logic.
                for &[tick, x, y, z, more] in &case.eggs {
                    if tick as usize != index {
                        continue;
                    }
                    let id = spawn::spawn_cow_from_egg(
                        &mut world,
                        BlockPos::new(x, y, z),
                        more != 0,
                        None,
                    );
                    ids.push(id);
                }
                // 交互里换出来的生物（哞菇剪成的牛）这一刻就会 tick：先写进官方给的场景起点。
                // A mob created by an interaction (the cow a mooshroom turns into) ticks this
                // very tick, so give it the official starting point first.
                if let Some(step) = case.spawns.as_ref().and_then(|spawns| spawns.get(index)) {
                    for setup in step.iter().filter(|spawn| spawn.kind == "cow") {
                        if apply_born_setup(&mut world, setup.id, setup) {
                            seeded_early.push(setup.id);
                        }
                    }
                }
                world.tick_entities();
                tick_effects = world.into_effects();
            }
            // 这一刻新生成的实体：快照是在 `addFreshEntity` 时记下的，官方
            // `ChunkMap.addEntity` 也在同一时刻建 `ServerEntity`。
            // Newly spawned entities: the snapshot is taken at `addFreshEntity`,
            // exactly when vanilla's `ChunkMap.addEntity` builds `ServerEntity`.
            let mut spawn_public = level.entities.take_spawn_snapshots();
            // 繁殖生成的幼崽也是牛：加进 `ids`，并把官方给的构造随机数、朝向和
            // UUID 作为场景起点写进实体，后面每刻比对它自己的行为。
            for public in &mut spawn_public {
                if !matches!(public.kind, mc_bamboo_blocks::entity::KindPublic::Mob(_)) {
                    continue;
                }
                if !ids.contains(&public.uid) {
                    ids.push(public.uid);
                }
                let setup = case
                    .spawns
                    .as_ref()
                    .and_then(|spawns| spawns.get(index))
                    .and_then(|step| step.iter().find(|spawn| spawn.kind == "cow"));
                let Some(setup) = setup else {
                    continue;
                };
                let (hr, yr) = born_rotation(setup);
                let uid = public.uid;
                if !seeded_early.contains(&uid) {
                    let mut world = World::new(&mut level, &mut ticks);
                    apply_born_setup(&mut world, uid, setup);
                }
                // 配对快照在 `addFreshEntity` 时记下，这里同样换成场景起点的朝向。
                // The pairing snapshot predates the override; give it the same start rotation.
                public.yaw = yr;
                if let mc_bamboo_blocks::entity::KindPublic::Mob(mob) = &mut public.kind {
                    mob.y_head_rot = hr;
                    if let Some([main, hidden]) = setup.genes {
                        mob.panda.main_gene = main;
                        mob.panda.hidden_gene = hidden;
                    }
                }
            }
            // 场景开始时就存在的牛在 tick 0 已经建过同步状态，这里只处理
            // 这一刻真正新生成的实体。
            // Cows present at scene start already got their tracking state at
            // tick 0; only genuinely new entities belong here.
            spawn_public.retain(|public| !tracking.has_tracked_for_oracle(public.uid));
            let mut spawn_pairings = Vec::new();
            for public in &spawn_public {
                let snapshot = Snapshot::of(public);
                let tracked = tracking.tracked_for_oracle(&snapshot);
                spawn_pairings.extend(tracked.pairing_packets(&snapshot));
            }
            if let Some(expected_interactions) = &case.interactions {
                compare_interactions(
                    &mut self.mismatches,
                    &self.show,
                    &case.name,
                    index,
                    &ours_interactions,
                    &expected_interactions[index],
                );
            }
            // 死亡场景：逐刻比新生成的掉落物和经验球（种类、数量、位置）。
            if let Some(expected_spawns) = &case.spawns {
                let ours_spawns = spawn_snapshot(&level, &spawn_public);
                compare_spawns(
                    &mut self.mismatches,
                    &self.show,
                    &case.name,
                    index,
                    &ours_spawns,
                    &expected_spawns[index],
                );
            }
            if let Some(expected) = &case.cube_splits
                && let Some(expected) = expected.get(index)
            {
                let ours = take_cube_splits(&mut level, &mut ticks, &spawn_public);
                if &ours != expected && self.mismatches.len() < self.show {
                    self.mismatches.push(format!(
                        "{} tick {index} splits: official {expected:?} bamboo {ours:?}",
                        case.name
                    ));
                }
                spawn_public.clear();
            }
            self.steps += 1;
            let mut ours = snapshot(&level, &ids);
            // 官方一侧输出的是 `Mob.getTarget()`：打不了的目标（死了的玩家）读成 null。
            if let Some(game) = &level.game {
                for step in ours.iter_mut().flatten() {
                    if step.target >= 0 && game.state(step.target)[3] != 0 {
                        step.target = -1;
                    }
                }
            }
            self.compare(&case.name, index, &ours, expected);
            self.compare_blocks(case, &level, index);
            self.compare_zombie_player(case, &mut level, index);
            if let Some(expected) = case.level_events.as_ref().and_then(|all| all.get(index)) {
                let ours: Vec<[i32; 6]> = tick_effects
                    .iter()
                    .filter_map(|effect| match *effect {
                        mc_bamboo_blocks::Effect::LevelEvent {
                            event, pos, data, ..
                        } => Some([0, event, pos.x, pos.y, pos.z, data]),
                        mc_bamboo_blocks::Effect::Destroyed { pos, state, .. } => {
                            Some([0, 2001, pos.x, pos.y, pos.z, state.0 as i32])
                        }
                        mc_bamboo_blocks::Effect::DestroyProgress {
                            breaker,
                            pos,
                            progress,
                        } => Some([1, breaker, pos.x, pos.y, pos.z, progress]),
                        _ => None,
                    })
                    .collect();
                if &ours != expected && self.mismatches.len() < self.show {
                    self.mismatches.push(format!(
                        "{} tick {index} level events: official {expected:?} bamboo {ours:?}",
                        case.name
                    ));
                }
            }
            if let Some(expected) = &case.mob_event_packets
                && let Some(expected) = expected.get(index)
            {
                // 只比 `entity_event`（34）和 `set_equipment`（104）；音效不取种子。
                let events = level.entities.take_mob_events();
                let ours: Vec<Vec<u8>> = event_packets(&events, || 0)
                    .into_iter()
                    .filter(|body| matches!(body.first(), Some(34 | 104)))
                    .collect();
                compare_packet_list(
                    &mut self.mismatches,
                    &self.show,
                    &case.name,
                    index,
                    &ours,
                    expected,
                );
            }
            // 官方 `ChunkMap.removeEntity` 在被丢弃的那一刻发移除包。
            let mut gone = Vec::new();
            for &id in &ids {
                if level.entities.public_at(id).is_none() && removed_ids.insert(id) {
                    gone.push(id);
                }
            }
            if let Some(packets) = &case.packets {
                let flags = case.sync_flags.as_ref().expect("有包就有标志位");
                let tick = &flags[index];
                // 官方的顺序：中途生成的配对包 → 实体 tick 里的事件（音效等）→
                // 每头牛按 uid 顺序的同步包。
                // Official order: mid-tick pairings, events from entity ticks,
                // then each cow's sync packets in id order.
                let mut ours_packets = spawn_pairings;
                let mut sync_packets = Vec::new();
                for (cow, &id) in ids.iter().enumerate() {
                    let Some(public) = level.entities.public_at(id) else {
                        continue;
                    };
                    let mut snapshot = Snapshot::of(public);
                    let bits = tick.get(cow).map(String::as_str).unwrap_or("00000");
                    let (needs_sync, _sync_position, sync_velocity) =
                        apply_flags(&mut snapshot, bits);
                    let tracked = tracking.tracked_for_oracle(&snapshot);
                    sync_packets.extend(tracked.step(&snapshot, needs_sync, sync_velocity));
                }
                let events = level.entities.take_mob_events();
                ours_packets.extend(event_packets(&events, || sound_source.next_long()));
                if !gone.is_empty() {
                    ours_packets.push(mc_bamboo_protocol::frame::packet_body(
                        &mc_bamboo_protocol::packet::play::RemoveEntities { entity_ids: &gone },
                    ));
                }
                ours_packets.extend(sync_packets);
                compare_packet_list(
                    &mut self.mismatches,
                    &self.show,
                    &case.name,
                    index,
                    &ours_packets,
                    &packets[index],
                );
            }
            if self.mismatches.len() >= self.show {
                return;
            }
        }
    }

    /// 吃草场景：这一刻场景里每一格的方块状态和官方逐格相同。
    ///
    /// Grazing scenes: every scene cell holds the official block state this tick.
    fn compare_blocks(&mut self, case: &Case, level: &MobLevel, index: usize) {
        let Some(expected) = case.block_steps.as_ref().and_then(|all| all.get(index)) else {
            return;
        };
        for (&[x, y, z, _], &official) in case.blocks.iter().zip(expected) {
            let ours = level.block_state(BlockPos::new(x, y, z)).0;
            if ours != official {
                self.mismatch(format!(
                    "{} tick {index} block {x},{y},{z}: official {official} bamboo {ours}",
                    case.name
                ));
            }
        }
    }

    /// 真实玩家的僵尸场景：比 0 号玩家的状态和它自己收到的那几种包。
    fn compare_zombie_player(&mut self, case: &Case, level: &mut MobLevel, index: usize) {
        const SELF_PACKETS: [u8; 6] = [43, 67, 68, 69, 106, 135];
        let (Some(game), Some(player)) = (level.game.as_mut(), case.players.first()) else {
            return;
        };
        // 第 0 刻是场景起点，官方还没跑玩家阶段的 `doTick`。
        if index > 0 {
            game.sync_health();
        }
        let ours: Vec<Vec<u8>> = game
            .take_packets(player.id)
            .into_iter()
            .filter(|body| body.first().is_some_and(|id| SELF_PACKETS.contains(id)))
            .collect();
        if let Some(states) = &case.zombie_player_states
            && let Some(expected) = states.get(index)
        {
            let state = game.state(player.id);
            if &state != expected && self.mismatches.len() < self.show {
                self.mismatches.push(format!(
                    "{} tick {index} player: official {expected:?} bamboo {state:?}",
                    case.name
                ));
            }
        }
        if let Some(packets) = &case.zombie_player_packets
            && let Some(expected) = packets.get(index)
        {
            compare_packet_list(
                &mut self.mismatches,
                &self.show,
                &case.name,
                index,
                &ours,
                expected,
            );
        }
    }

    fn compare(
        &mut self,
        name: &str,
        index: usize,
        ours: &[Option<CowStep>],
        official: &[CowStep],
    ) {
        for (cow, expected) in official.iter().enumerate() {
            let place = format!("{name} tick {index} cow {cow}");
            if std::env::var_os("MOB_ORACLE_BRAIN").is_some() && !expected.brain.is_empty() {
                eprintln!("{place} official brain: {}", expected.brain);
            }
            let Some(Some(ours)) = ours.get(cow) else {
                if !expected.rm {
                    self.mismatch(format!("{place}: missing in bamboo"));
                }
                continue;
            };
            if ours == expected {
                continue;
            }
            let mut diffs = Vec::new();
            macro_rules! bits64 {
                ($field:ident) => {
                    if ours.$field != expected.$field {
                        diffs.push(format!(
                            "{}: official {} bamboo {}",
                            stringify!($field),
                            f64::from_bits(expected.$field as u64),
                            f64::from_bits(ours.$field as u64)
                        ));
                    }
                };
            }
            macro_rules! bits32 {
                ($field:ident) => {
                    if ours.$field != expected.$field {
                        diffs.push(format!(
                            "{}: official {} bamboo {}",
                            stringify!($field),
                            f32::from_bits(expected.$field as u32),
                            f32::from_bits(ours.$field as u32)
                        ));
                    }
                };
            }
            macro_rules! plain {
                ($field:ident) => {
                    if ours.$field != expected.$field {
                        diffs.push(format!(
                            "{}: official {:?} bamboo {:?}",
                            stringify!($field),
                            expected.$field,
                            ours.$field
                        ));
                    }
                };
            }
            bits64!(x);
            bits64!(y);
            bits64!(z);
            bits64!(dx);
            bits64!(dy);
            bits64!(dz);
            bits32!(yr);
            bits32!(xr);
            bits32!(hr);
            bits32!(br);
            bits32!(hp);
            plain!(g);
            plain!(rm);
            plain!(w);
            if name.starts_with("drowned_") {
                plain!(swimming);
            }
            bits64!(fd);
            plain!(nat);
            plain!(ht);
            plain!(dt);
            plain!(fire);
            plain!(air);
            plain!(seed);
            plain!(age);
            plain!(love);
            plain!(lh);
            bits64!(fr);
            plain!(uuid);
            plain!(lr);
            plain!(path);
            plain!(var);
            plain!(snd);
            plain!(goals);
            plain!(brain);
            if zombie_style(name) {
                plain!(rabbit_carrot_ticks);
                plain!(fox_flags);
                plain!(fox_mouth);
                plain!(goat);
                plain!(wolf_tame);
                plain!(wolf_sit);
                plain!(wolf_owned);
                plain!(wolf_anger);
                plain!(wolf_beg);
                plain!(wolf_collar);
                plain!(wolf_max_hp);
            }
            if name.starts_with("zombie_") || name.starts_with("drowned_") {
                if name.starts_with("zombie_inhabited_") {
                    plain!(ed);
                }
                plain!(target);
                plain!(baby);
                plain!(aggr);
                plain!(doors);
                plain!(ph);
                plain!(pht);
                plain!(pcd);
                plain!(bh);
                plain!(hd);
                plain!(hc);
                bits64!(armor);
                plain!(eq);
                plain!(tmp);
                plain!(rider_count);
                plain!(owner);
                plain!(turtle_egg);
                plain!(turtle_laying);
                plain!(turtle_lay_count);
                plain!(turtle_going_home);
                plain!(turtle_home_x);
                plain!(turtle_home_y);
                plain!(turtle_home_z);
            }
            if diffs.is_empty() {
                continue;
            }
            self.mismatch(format!("{place}: {}", diffs.join("; ")));
        }
    }

    /// 1c：用生产代码的落脚点搜索重放官方场景，逐位比坐标、半径和随机源末态。
    fn replay_spawn_landing(&mut self, case: &Case) {
        use mc_bamboo_game::{ReplaySpawnLanding, spawn_landing_sample};

        let Some(expected) = &case.spawn_result else {
            return;
        };
        let scene = ReplaySpawnLanding {
            blocks: case.blocks.clone(),
            spawn: expected.spawn_at,
            radius: expected.radius,
            thread_seed: expected.seed,
            game_type: expected.game_type,
            personal: expected.personal,
            break_radius: matches!(self.break_test.as_deref(), Some("spawn_radius")),
            break_safety: matches!(self.break_test.as_deref(), Some("spawn_safety")),
            break_fallback: matches!(self.break_test.as_deref(), Some("spawn_fallback")),
        };
        let ours = spawn_landing_sample(&scene);
        self.steps += 1;
        let bit = |value: f64| value.to_bits() as i64;
        if bit(ours.x) != expected.x || bit(ours.y) != expected.y || bit(ours.z) != expected.z {
            self.mismatch(format!(
                "{} landing: official {:?}, bamboo {:?}",
                case.name,
                (
                    f64::from_bits(expected.x as u64),
                    f64::from_bits(expected.y as u64),
                    f64::from_bits(expected.z as u64),
                ),
                (ours.x, ours.y, ours.z)
            ));
        }
        if ours.offset != expected.offset {
            self.mismatch(format!(
                "{} candidate offset: official {}, bamboo {}",
                case.name, expected.offset, ours.offset
            ));
        }
        if ours.radius != expected.radius {
            self.mismatch(format!(
                "{} radius: official {}, bamboo {}",
                case.name, expected.radius, ours.radius
            ));
        }
        if ours.random_state != expected.random_state && expected.personal.is_none() {
            self.mismatch(format!(
                "{} random state: official {}, bamboo {}",
                case.name, expected.random_state, ours.random_state
            ));
        }
        if ours.missing != expected.missing {
            self.mismatch(format!(
                "{} personal missing: official {}, bamboo {}",
                case.name, expected.missing, ours.missing
            ));
        }
    }

    fn mismatch(&mut self, message: String) {
        if self.mismatches.len() < self.show.max(1) {
            self.mismatches.push(message);
        }
    }

    /// 玩家受伤场景：走正式的 `Region` / `Player` / `player_hurt` 重放，
    /// 逐刻比状态原始位和包字节。
    fn replay_player_hurt(&mut self, case: &Case) {
        use mc_bamboo_game::{ReplayPlayerAction, ReplayPlayerHurt, replay_player_hurt};

        let Some(expected_steps) = &case.player_steps else {
            return;
        };
        let seed = case
            .player_steps
            .as_ref()
            .and_then(|steps| steps.first())
            .and_then(|step| step.first())
            .and_then(|text| serde_json::from_str::<PlayerStepJson>(text).ok())
            .map(|step| step.seed)
            .unwrap_or(0);
        let mut equipment = [0_i32; 6];
        if let Some(items) = case.player_equipment.first() {
            equipment[..5].copy_from_slice(items);
        }
        let protection = case.player_protection.first().copied().unwrap_or([0; 4]);
        let actions: Vec<ReplayPlayerAction> = case
            .player_actions
            .iter()
            .map(|action| ReplayPlayerAction {
                tick: action.tick,
                kind: action
                    .kind
                    .as_deref()
                    .and_then(|name| name.strip_prefix("minecraft:"))
                    .unwrap_or("generic")
                    .to_owned(),
                amount: action.amount,
                at: action.at,
                attack_cow: action.action == 1,
            })
            .collect();
        let mut scene = ReplayPlayerHurt {
            ticks: expected_steps.len() - 1,
            difficulty: case.difficulty,
            health: f32::from_bits(case.player_health as u32),
            absorption: f32::from_bits(case.player_absorption as u32),
            exhaustion: f32::from_bits(case.player_exhaustion as u32),
            seed,
            equipment,
            protection,
            attack_cow: actions.iter().any(|action| action.attack_cow),
            actions,
            no_cooldown: false,
            no_knockback: false,
            attack_charge: case.attack_charge,
            attack_sprint: case.attack_sprint,
            attack_fall: f64::from_bits(case.attack_fall as u64),
            attack_ground: case.attack_ground,
            attack_enchants: case.attack_enchants.clone(),
            extra_cow_positions: case
                .cows
                .iter()
                .skip(1)
                .map(|cow| {
                    (
                        f64::from_bits(cow.x as u64),
                        f64::from_bits(cow.y as u64),
                        f64::from_bits(cow.z as u64),
                    )
                })
                .collect(),
            cow_seeds: case.cows.iter().map(|cow| cow.seed).collect(),
        };
        // 故意改坏：难度缩放、护甲公式（用空装备）、保护附魔等级、
        // 无敌帧（每次伤害前清冷却）、击退（去掉来源位置）、攻击充能值
        // 少算一刻，以及去掉武器上的火焰附加（模拟漏掉攻击后附魔效果，
        // 横扫目标的 fire 即刻不一致）。每一项都让逐刻状态或包字节失败，
        // 证明比较没有放松。
        //
        // Deliberate breakage: difficulty, armor, protection, invulnerability,
        // knockback, an attack charge off by one tick, and dropping Fire Aspect
        // from the weapon (a stand-in for missing post-attack effects, so the
        // swept target's fire ticks diverge).
        match self.break_test.as_deref() {
            Some("difficulty") => scene.difficulty = 2,
            Some("armor") => scene.equipment = [0; 6],
            Some("protection") => scene.protection = [0; 4],
            Some("cooldown") => scene.no_cooldown = true,
            Some("knockback") => {
                for action in &mut scene.actions {
                    action.at = None;
                }
                scene.no_knockback = true;
            }
            Some("attack_charge") => scene.attack_charge = scene.attack_charge.saturating_sub(1),
            Some("sweep_effects") => scene
                .attack_enchants
                .retain(|(name, _)| name != "minecraft:fire_aspect"),
            _ => {}
        }
        let output = replay_player_hurt(&scene);
        let (steps, packets, observer_packets, attacker_packets, cow_states) = (
            output.steps,
            output.packets,
            output.observer_packets,
            output.attacker_packets,
            output.cow_states,
        );
        // 攻击场景逐刻比牛的血量、击退速度、着火刻数、受伤时间和是否被移除。
        //
        // Attack scenes compare each cow's health, knockback velocity, fire
        // ticks, hurt time and removal per tick.
        if scene.attack_cow {
            for (tick, expected) in case.steps.iter().enumerate() {
                let Some(ours) = cow_states.get(tick) else {
                    self.mismatch(format!("{} tick {tick}: no Bamboo cow state", case.name));
                    continue;
                };
                if ours.len() != expected.len() {
                    self.mismatch(format!(
                        "{} tick {tick} cows: official {} Bamboo {}",
                        case.name,
                        expected.len(),
                        ours.len()
                    ));
                    continue;
                }
                for (index, (ours, official)) in ours.iter().zip(expected).enumerate() {
                    let fields: [(&str, i64, i64); 7] = [
                        ("hp", i64::from(ours.hp), i64::from(official.hp)),
                        ("dx", ours.dx, official.dx),
                        ("dy", ours.dy, official.dy),
                        ("dz", ours.dz, official.dz),
                        ("fire", i64::from(ours.fire), i64::from(official.fire)),
                        ("ht", i64::from(ours.hurt_time), i64::from(official.ht)),
                        ("seed", ours.seed, official.seed),
                    ];
                    for (field, a, b) in fields {
                        if a != b {
                            self.mismatch(format!(
                                "{} tick {tick} cow {index} {field}: official {b} Bamboo {a}",
                                case.name
                            ));
                        }
                    }
                    if ours.removed != official.rm {
                        self.mismatch(format!(
                            "{} tick {tick} cow {index} removed: official {} Bamboo {}",
                            case.name, official.rm, ours.removed
                        ));
                    }
                }
            }
        }
        for (tick, expected) in expected_steps.iter().enumerate() {
            self.steps += 1;
            let Some(ours) = steps.get(tick) else {
                self.mismatch(format!("{} tick {tick}: no Bamboo player step", case.name));
                continue;
            };
            let Some(text) = expected.first() else {
                self.mismatch(format!(
                    "{} tick {tick}: no official player step",
                    case.name
                ));
                continue;
            };
            let Ok(official) = serde_json::from_str::<PlayerStepJson>(text) else {
                self.mismatch(format!(
                    "{} tick {tick}: bad official player JSON",
                    case.name
                ));
                continue;
            };
            if !official.matches(ours) {
                self.mismatch(format!(
                    "{} tick {tick} player state: official {official:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            // 官方的两组包：自己收到的 + 跟踪者收到的，合起来按发生顺序比。
            // `playerPackets` 是 `connection.send`，`packets` 是 Chunks/events。
            // 生产代码把两类包都写进同一条链路，按 ID 分类后再和官方逐项比。
            let official_self = case
                .player_packets
                .as_ref()
                .and_then(|steps| steps.get(tick))
                .cloned()
                .unwrap_or_default();
            let official_tracking = case
                .packets
                .as_ref()
                .and_then(|steps| steps.get(tick))
                .cloned()
                .unwrap_or_default();
            let mut ours_self = Vec::new();
            let mut ours_tracking = Vec::new();
            // 官方收集器是整个服务端一份流，每个广播包只记一次；Bamboo 每个收件人
            // 一份。攻击场景取两名收件人里的最大份数：等于「至少一个客户端收到、
            // 且没有多发」。自己收到的包仍从合成流里挑。
            //
            // The official collector is one server-wide stream that records each
            // broadcast once; Bamboo records one per recipient. Attack scenes take
            // the maximum count across the two recipients: "at least one client got
            // it, and there are no extras". Self packets still come from the merged
            // stream.
            let attack = is_attack_scene(&case.name);
            for body in packets.get(tick).cloned().unwrap_or_default() {
                if is_self_packet(&body) {
                    ours_self.push(body);
                }
            }
            if attack {
                let attacker = attacker_packets.get(tick).cloned().unwrap_or_default();
                let observer = observer_packets.get(tick).cloned().unwrap_or_default();
                ours_tracking = merge_max(&attacker, &observer)
                    .into_iter()
                    .filter(|body| !is_self_packet(body))
                    .collect();
            } else {
                for body in packets.get(tick).cloned().unwrap_or_default() {
                    if !is_self_packet(&body) {
                        ours_tracking.push(body);
                    }
                }
            }
            compare_player_packets(
                &mut self.mismatches,
                &self.show,
                &case.name,
                tick,
                "self",
                &ours_self,
                &official_self,
            );
            // 牛的同步包留在 `packets` 里，这一期只比玩家受伤产生的包。
            // 攻击场景里牛自己也会发出带种子的环境音/受伤音；官方假世界也不捕获
            // crit 动画 / 粒子 / 挥手 / 耐久槽这些包，所以跟踪包只比 `damage_event`。
            // 牛的血量 / 击退速度 / 着火 / 受伤时间由上面的 `cow_states` 逐刻比。
            //
            // Attack scenes also emit seeded cow sounds, and the official fake
            // world does not capture crit animations / particles / swing /
            // durability-slot packets, so tracking comparison keeps only
            // `damage_event`. Cow health / knockback / fire / hurt time are
            // compared above via `cow_states`.
            if attack {
                // 攻击场景比音效 / 粒子 / 动画 / 伤害事件。Bamboo 把这些效果
                // 攒到一拍末尾统一发，官方当场逐个发，所以按包排序后做多重集
                // 比较：数量、字节（含音效和粒子的种子）仍逐个对上。
                //
                // Attack scenes compare sounds / particles / animations / damage
                // events. Bamboo batches them to the end of the tick while vanilla
                // sends each immediately, so compare as a multiset after sorting:
                // counts and bytes (including the sound and particle seeds) still
                // match one by one.
                let mut ours: Vec<Vec<u8>> = ours_tracking
                    .iter()
                    .filter(|bytes| packet_id(bytes).is_some_and(is_attack_effect_packet))
                    .map(|bytes| normalize_attack_effect(bytes))
                    .collect();
                let mut official: Vec<Vec<u8>> = official_tracking
                    .iter()
                    .filter_map(|text| base64_stub::decode(text).ok())
                    .filter(|bytes| packet_id(bytes).is_some_and(is_attack_effect_packet))
                    .map(|bytes| normalize_attack_effect(&bytes))
                    .collect();
                ours.sort();
                official.sort();
                compare_attack_packets(
                    &mut self.mismatches,
                    &self.show,
                    &case.name,
                    tick,
                    &ours,
                    &official,
                );
                continue;
            }
            let tracking: Vec<String> = official_tracking
                .iter()
                .filter(|text| {
                    base64_stub::decode(text)
                        .ok()
                        .is_some_and(|bytes| is_tracking_hurt_packet(&bytes))
                })
                .cloned()
                .collect();
            let ours_tracking: Vec<Vec<u8>> = ours_tracking
                .into_iter()
                .filter(|bytes| is_tracking_hurt_packet(bytes))
                .collect();
            let ours_tracking = &ours_tracking;
            compare_player_packets(
                &mut self.mismatches,
                &self.show,
                &case.name,
                tick,
                "tracking",
                ours_tracking,
                &tracking,
            );
        }
    }

    /// 1b 场景：正式 `Region` 重放，逐刻比状态和筛选后的包字节。
    /// S4：吃东西场景逐刻比状态原始位和出站包字节。
    fn replay_player_consume(&mut self, case: &Case) {
        use mc_bamboo_game::{ReplayPlayerConsume, replay_player_consume};

        let Some(setup) = &case.consume_scene else {
            return;
        };
        let Some(expected_steps) = &case.player_steps else {
            return;
        };
        let scene = ReplayPlayerConsume {
            ticks: expected_steps.len() - 1,
            item: setup.item,
            start_food: setup.food,
            break_duration: matches!(self.break_test.as_deref(), Some("consume_duration")),
            break_food: matches!(self.break_test.as_deref(), Some("consume_food")),
            break_remainder: matches!(self.break_test.as_deref(), Some("consume_remainder")),
        };
        let (steps, packets) = replay_player_consume(&scene);
        for (tick, expected) in expected_steps.iter().enumerate() {
            self.steps += 1;
            let Some(ours) = steps.get(tick) else {
                self.mismatch(format!("{} tick {tick}: no Bamboo consume step", case.name));
                continue;
            };
            let Some(text) = expected.first() else {
                self.mismatch(format!(
                    "{} tick {tick}: no official consume step",
                    case.name
                ));
                continue;
            };
            let Ok(official) = serde_json::from_str::<PlayerConsumeStepJson>(text) else {
                self.mismatch(format!(
                    "{} tick {tick}: bad official consume JSON: {text}",
                    case.name
                ));
                continue;
            };
            if official != *ours {
                self.mismatch(format!(
                    "{} tick {tick} consume state: official {official:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            if tick > 0 {
                let expected_packets = case.packets.as_ref().and_then(|p| p.get(tick));
                let Some(expected_packets) = expected_packets else {
                    self.mismatch(format!("{} tick {tick}: no official packets", case.name));
                    continue;
                };
                compare_packet_list(
                    &mut self.mismatches,
                    &self.show,
                    &case.name,
                    tick,
                    &packets[tick],
                    expected_packets,
                );
            }
            if self.mismatches.len() >= self.show {
                return;
            }
        }
    }

    /// S3：环境伤害场景逐刻比状态原始位和出站包字节。
    fn replay_player_env(&mut self, case: &Case) {
        use mc_bamboo_game::{ReplayPlayerEnv, replay_player_env};

        let Some(setup) = &case.env_scene else {
            return;
        };
        let Some(expected_steps) = &case.player_steps else {
            return;
        };
        let scene = ReplayPlayerEnv {
            ticks: expected_steps.len() - 1,
            start_health: f32::from_bits(setup.start_health as u32),
            start_absorption: f32::from_bits(case.player_absorption as u32),
            block: setup.block,
            fire: setup.fire,
            air: setup.air,
            frozen: setup.frozen,
            fall: setup.fall,
            y: f64::from_bits(setup.y as u64),
            difficulty: setup.difficulty,
            break_fire: matches!(self.break_test.as_deref(), Some("env_fire")),
            break_air: matches!(self.break_test.as_deref(), Some("env_air")),
            break_fall: matches!(self.break_test.as_deref(), Some("env_fall")),
        };
        let (steps, packets) = replay_player_env(&scene);
        for (tick, expected) in expected_steps.iter().enumerate() {
            self.steps += 1;
            let Some(ours) = steps.get(tick) else {
                self.mismatch(format!("{} tick {tick}: no Bamboo env step", case.name));
                continue;
            };
            let Some(text) = expected.first() else {
                self.mismatch(format!("{} tick {tick}: no official env step", case.name));
                continue;
            };
            let Ok(official) = serde_json::from_str::<PlayerEnvStepJson>(text) else {
                self.mismatch(format!("{} tick {tick}: bad official env JSON", case.name));
                continue;
            };
            if official != *ours {
                self.mismatch(format!(
                    "{} tick {tick} env state: official {official:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            if tick > 0 {
                let expected_packets = case.packets.as_ref().and_then(|p| p.get(tick));
                let Some(expected_packets) = expected_packets else {
                    self.mismatch(format!("{} tick {tick}: no official packets", case.name));
                    continue;
                };
                compare_packet_list(
                    &mut self.mismatches,
                    &self.show,
                    &case.name,
                    tick,
                    &packets[tick],
                    expected_packets,
                );
            }
            if self.mismatches.len() >= self.show {
                return;
            }
        }
    }

    /// S2：玩家效果场景逐刻比状态原始位和出站包字节。
    fn replay_player_effects(&mut self, case: &Case) {
        use mc_bamboo_game::{ReplayPlayerEffects, replay_player_effects};

        let Some(setup) = &case.effect_scene else {
            return;
        };
        let Some(expected_steps) = &case.player_steps else {
            return;
        };
        let scene = ReplayPlayerEffects {
            ticks: expected_steps.len() - 1,
            start_health: f32::from_bits(setup.start_health as u32),
            start_absorption: f32::from_bits(case.player_absorption as u32),
            actions: setup.actions.clone(),
            break_duration: matches!(self.break_test.as_deref(), Some("effect_duration")),
            break_flags: matches!(self.break_test.as_deref(), Some("effect_flags")),
            break_clear: matches!(self.break_test.as_deref(), Some("effect_clear")),
        };
        let (steps, packets) = replay_player_effects(&scene);
        for (tick, expected) in expected_steps.iter().enumerate() {
            self.steps += 1;
            let Some(ours) = steps.get(tick) else {
                self.mismatch(format!("{} tick {tick}: no Bamboo effect step", case.name));
                continue;
            };
            let Some(text) = expected.first() else {
                self.mismatch(format!(
                    "{} tick {tick}: no official effect step",
                    case.name
                ));
                continue;
            };
            let Ok(official) = serde_json::from_str::<PlayerEffectStepJson>(text) else {
                self.mismatch(format!(
                    "{} tick {tick}: bad official effect JSON",
                    case.name
                ));
                continue;
            };
            if official != *ours {
                self.mismatch(format!(
                    "{} tick {tick} effect state: official {official:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            if tick > 0 {
                let expected_packets = case.packets.as_ref().and_then(|packets| packets.get(tick));
                let Some(expected_packets) = expected_packets else {
                    self.mismatch(format!("{} tick {tick}: no official packets", case.name));
                    continue;
                };
                compare_packet_list(
                    &mut self.mismatches,
                    &self.show,
                    &case.name,
                    tick,
                    &packets[tick],
                    expected_packets,
                );
            }
            if self.mismatches.len() >= self.show {
                return;
            }
        }
    }

    fn replay_player_life(&mut self, case: &Case) {
        use mc_bamboo_game::{ReplayPlayerLife, replay_player_life};

        let Some(life) = &case.life else {
            return;
        };
        let Some(expected_steps) = &case.player_steps else {
            return;
        };
        let scene = ReplayPlayerLife {
            ticks: expected_steps.len() - 1,
            difficulty: case.difficulty,
            keep_inventory: life.keep_inventory,
            immediate_respawn: life.immediate_respawn,
            show_death_messages: life.show_death_messages,
            natural_regeneration: life.natural_regeneration,
            xp_level: life.xp_level,
            inventory_items: life.inventory_items.clone(),
            vanishing_slots: life.vanishing_slots.clone(),
            death_tick: life.death_tick,
            respawn_tick: life.respawn_tick,
            death_kind: life.death_kind.clone(),
            death_amount: life.death_amount,
            death_attacker: life.death_attacker >= 0,
            start_health: f32::from_bits(case.player_health as u32),
            start_food: life.start_food,
            start_saturation: f32::from_bits(life.start_saturation as u32),
            start_exhaustion: f32::from_bits(case.player_exhaustion as u32),
            start_food_timer: life.start_food_timer,
            credit_hurt_tick: life.credit_hurt_tick,
            death_fall_distance: f32::from_bits(life.death_fall_distance as u32),
            break_credit_expiry: matches!(self.break_test.as_deref(), Some("credit")),
            break_xp: matches!(self.break_test.as_deref(), Some("xp")),
            break_keep: matches!(self.break_test.as_deref(), Some("keep")),
            break_vanishing: matches!(self.break_test.as_deref(), Some("vanishing")),
            break_respawn_order: matches!(self.break_test.as_deref(), Some("respawn")),
            break_regen: matches!(self.break_test.as_deref(), Some("regen")),
        };
        let (steps, packets, life_spawns) = replay_player_life(&scene);
        for (tick, expected) in expected_steps.iter().enumerate() {
            self.steps += 1;
            let Some(ours) = steps.get(tick) else {
                self.mismatch(format!("{} tick {tick}: no Bamboo life step", case.name));
                continue;
            };
            let Some(text) = expected.first() else {
                self.mismatch(format!("{} tick {tick}: no official life step", case.name));
                continue;
            };
            let Ok(official) = serde_json::from_str::<PlayerLifeStepJson>(text) else {
                self.mismatch(format!("{} tick {tick}: bad official life JSON", case.name));
                continue;
            };
            if !official.matches(ours) {
                self.mismatch(format!(
                    "{} tick {tick} life state: official {official:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            // 比关键包：`player_combat_kill`(69)、`system_chat`(124)、
            // `entity_event`(34)、`respawn` 之后那整套。Bamboo 的广播包和
            // 自发包都写在同一条链路上，按 ID 分类再比。
            let official_all = case
                .packets
                .as_ref()
                .and_then(|steps| steps.get(tick))
                .cloned()
                .unwrap_or_default();
            let official_key = case
                .key_packets
                .as_ref()
                .and_then(|steps| steps.get(tick))
                .cloned()
                .unwrap_or_default();
            let ours_packets = packets.get(tick).cloned().unwrap_or_default();
            // 假世界的命令树 / 配对包由桩构造，逐位比包 ID 序列；
            // 关键包（死亡界面、系统消息、重生、传送、出生点、难度、
            // 经验、能力、血量、死亡动画）再逐字节比。
            // 命令树（16）由各自的命令图生成：假世界没有命令派发器，
            // 这一包不进 ID 序列比较，其余逐包比顺序。
            let not_comparable =
                |id: i32| id == mc_bamboo_data::packets::play::clientbound::COMMANDS;
            let official_ids: Vec<i32> = official_all
                .iter()
                .filter_map(|text| base64_stub::decode(text).ok().and_then(|b| packet_id(&b)))
                .filter(|id| !not_comparable(*id))
                .collect();
            let ours_ids: Vec<i32> = ours_packets
                .iter()
                .filter_map(|b| packet_id(b))
                .filter(|id| !not_comparable(*id))
                .collect();
            if official_ids != ours_ids {
                self.mismatch(format!(
                    "{} tick {tick} life packet ids: official {official_ids:?}, bamboo {ours_ids:?}",
                    case.name
                ));
            }
            let ours_key: Vec<Vec<u8>> = ours_packets
                .into_iter()
                .filter(|body| is_life_key_packet(body))
                .collect();
            compare_player_packets(
                &mut self.mismatches,
                &self.show,
                &case.name,
                tick,
                "life key",
                &ours_key,
                &official_key,
            );
        }
        // 掉落物与经验球：逐刻比条数 / 种类 / 物品 / 值。
        if let Some(spawns) = &case.spawns {
            for (tick, official) in spawns.iter().enumerate() {
                self.steps += 1;
                let expected = official;
                let ours: Vec<Spawn> = life_spawns
                    .get(tick)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(kind, item, count, value)| Spawn {
                        id: 0,
                        x: 0,
                        y: 0,
                        z: 0,
                        kind,
                        item,
                        count,
                        value,
                        seed: None,
                        hr: None,
                        yr: None,
                        uuid: None,
                        egg: None,
                        genes: None,
                    })
                    .collect();
                // 只比数量 / 种类 / 物品 / 经验值，不比实体 ID 和速度随机源。
                let shape = |list: &[Spawn]| -> Vec<(String, i32, i32, i32)> {
                    let mut rows: Vec<_> = list
                        .iter()
                        .map(|spawn| (spawn.kind.clone(), spawn.item, spawn.count, spawn.value))
                        .collect();
                    rows.sort();
                    rows
                };
                if shape(expected) != shape(&ours) {
                    self.mismatch(format!(
                        "{} tick {tick} spawns: official {:?}, bamboo {:?}",
                        case.name,
                        shape(expected),
                        shape(&ours)
                    ));
                }
            }
        }
    }

    fn finish(self) -> Result<(), String> {
        if !self.ended {
            return Err("mob oracle stopped before the end line".to_owned());
        }
        self.skeleton_spawns.print();
        self.natural_spawns.print();
        self.patrol_spawns.print();
        self.structure_spawns.print();
        self.gen_spawns.print();
        if self.break_test.as_deref() == Some("gen_spawn") && self.gen_spawns.cases == 0 {
            return Err("gen spawn break-self-test ran no cases".to_owned());
        }
        if self.break_test.as_deref() == Some("natural_spawn") && self.natural_spawns.ticks == 0 {
            return Err("natural spawn break-self-test ran no natural spawn ticks".to_owned());
        }
        if self.break_test.as_deref() == Some("patrol_spawn") && self.patrol_spawns.ticks == 0 {
            return Err("patrol spawn break-self-test ran no patrol ticks".to_owned());
        }
        if !self.mismatches.is_empty() {
            return Err(format!(
                "mob oracle: {} mismatches\n{}",
                self.mismatches.len(),
                self.mismatches.join("\n")
            ));
        }
        self.natural_spawns.check()?;
        self.patrol_spawns.check()?;
        self.structure_spawns.check()?;
        self.gen_spawns.check()?;
        println!(
            "MOB ORACLE: {} cases, {} ticks, 0 mismatches",
            self.cases, self.steps
        );
        Ok(())
    }
}

/// S4 吃东西场景。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsumeScene {
    item: i32,
    food: i32,
}

/// Java 一侧 `consumeStepJson` 的逐刻状态。
#[derive(Debug, Deserialize, PartialEq)]
struct PlayerConsumeStepJson {
    hp: i32,
    food: i32,
    sat: i32,
    exh: i32,
    item: i32,
    count: i32,
    #[serde(rename = "use")]
    using: i32,
    effects: Vec<[i32; 3]>,
}

impl PartialEq<mc_bamboo_game::PlayerConsumeStep> for PlayerConsumeStepJson {
    fn eq(&self, other: &mc_bamboo_game::PlayerConsumeStep) -> bool {
        self.hp == other.hp
            && self.food == other.food
            && self.sat == other.sat
            && self.exh == other.exh
            && self.item == other.item
            && self.count == other.count
            && self.using == other.using
            && self.effects == other.effects
    }
}

/// S3 环境场景的方块与起始状态。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvScene {
    start_health: i32,
    block: i32,
    fire: i32,
    air: i32,
    frozen: i32,
    fall: i32,
    difficulty: i32,
    y: i64,
}

/// Java 一侧 `envStepJson` 的逐刻状态。
#[derive(Debug, Deserialize, PartialEq)]
struct PlayerEnvStepJson {
    hp: i32,
    abs: i32,
    ht: i32,
    hd: i32,
    cd: i32,
    lh: i32,
    food: i32,
    sat: i32,
    exh: i32,
    fire: i32,
    air: i32,
    frozen: i32,
    fall: i64,
    seed: i64,
}

impl PartialEq<mc_bamboo_game::PlayerEnvStep> for PlayerEnvStepJson {
    fn eq(&self, other: &mc_bamboo_game::PlayerEnvStep) -> bool {
        self.hp == other.hp
            && self.abs == other.abs
            && self.ht == other.ht
            && self.hd == other.hd
            && self.cd == other.cd
            && self.lh == other.lh
            && self.food == other.food
            && self.sat == other.sat
            && self.exh == other.exh
            && self.fire == other.fire
            && self.air == other.air
            && self.frozen == other.frozen
            && self.fall == other.fall
            && self.seed == other.seed
    }
}

/// S2 效果场景的动作与起始血量。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EffectScene {
    start_health: i32,
    #[serde(default)]
    actions: Vec<[i32; 5]>,
}

/// Java 一侧 `effectStepJson` 的逐刻状态。
#[derive(Debug, Deserialize, PartialEq)]
struct PlayerEffectStepJson {
    hp: i32,
    abs: i32,
    ht: i32,
    hd: i32,
    cd: i32,
    lh: i32,
    food: i32,
    sat: i32,
    exh: i32,
    timer: i32,
    seed: i64,
    effects: Vec<[i32; 6]>,
}

impl PartialEq<mc_bamboo_game::PlayerEffectStep> for PlayerEffectStepJson {
    fn eq(&self, other: &mc_bamboo_game::PlayerEffectStep) -> bool {
        self.hp == other.hp
            && self.abs == other.abs
            && self.ht == other.ht
            && self.hd == other.hd
            && self.cd == other.cd
            && self.lh == other.lh
            && self.food == other.food
            && self.sat == other.sat
            && self.exh == other.exh
            && self.timer == other.timer
            && self.seed == other.seed
            && self.effects == other.effects
    }
}

/// Java 一侧的 `playerStepJson`，整数都是原始位。
#[derive(Debug, Deserialize)]
struct PlayerStepJson {
    hp: i32,
    abs: i32,
    ht: i32,
    hd: i32,
    cd: i32,
    lh: i32,
    food: i32,
    sat: i32,
    exh: i32,
    dx: i64,
    dy: i64,
    dz: i64,
    seed: i64,
}

impl PlayerStepJson {
    fn matches(&self, ours: &mc_bamboo_game::PlayerHurtStep) -> bool {
        self.hp == ours.hp
            && self.abs == ours.abs
            && self.ht == ours.ht
            && self.hd == ours.hd
            && self.cd == ours.cd
            && self.lh == ours.lh
            && self.food == ours.food
            && self.sat == ours.sat
            && self.exh == ours.exh
            && self.dx == ours.dx
            && self.dy == ours.dy
            && self.dz == ours.dz
            && self.seed == ours.seed
    }
}

/// 包 ID + 包体（不含帧长头）的 ID。
fn packet_id(bytes: &[u8]) -> Option<i32> {
    let mut value = 0_i32;
    let mut shift = 0;
    for (index, byte) in bytes.iter().enumerate() {
        value |= i32::from(byte & 0x7f).checked_shl(shift).unwrap_or(0);
        if byte & 0x80 == 0 {
            return (index < 5).then_some(value);
        }
        shift += 7;
    }
    None
}

/// 玩家近战攻击场景：跟踪包只比 `damage_event`，牛的状态另由 `cow_states` 比。
///
/// Player melee-attack scenes: tracking comparison keeps only `damage_event`;
/// cow state is compared separately via `cow_states`.
/// 玩家近战攻击场景：跟踪包只比这几类效果包，牛的状态另由 `cow_states` 比。
///
/// Player melee-attack scenes: tracking comparison keeps these effect packets;
/// cow state is compared separately via `cow_states`.
fn is_attack_effect_packet(id: i32) -> bool {
    use mc_bamboo_data::packets::play::clientbound as ids;
    matches!(
        id,
        ids::DAMAGE_EVENT | ids::SOUND | ids::LEVEL_PARTICLES | ids::ANIMATE | ids::ENTITY_EVENT
    )
}

/// 两名收件人的流按包取最大份数：同一个包两人都收到算一份，只有一人收到也算一份，
/// 一人收到两份才算两份。
///
/// Take the maximum count per packet across two recipient streams: a packet both
/// received counts once, one only one received counts once, and two copies to one
/// recipient counts twice.
fn merge_max(first: &[Vec<u8>], second: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut counts: std::collections::BTreeMap<&[u8], (usize, usize)> =
        std::collections::BTreeMap::new();
    for bytes in first {
        counts.entry(bytes.as_slice()).or_default().0 += 1;
    }
    for bytes in second {
        counts.entry(bytes.as_slice()).or_default().1 += 1;
    }
    let mut out = Vec::new();
    for (bytes, (a, b)) in counts {
        for _ in 0..a.max(b) {
            out.push(bytes.to_vec());
        }
    }
    out
}

/// 攻击场景比较前去掉音效包末尾 8 字节的随机种子。
///
/// 官方每发一个音效就当场从世界随机源取一个种子，Bamboo 把一拍的效果攒到
/// 末尾统一发，取种子的顺序因此不同。客户端只用这个种子给音高加一点随机
/// 扰动，不改变服务端状态；其余字段仍逐字节比。
///
/// Drop the trailing 8-byte random seed of sound packets before comparing
/// attack scenes. Vanilla draws one seed from the level random as each sound is
/// played, while Bamboo batches a tick's effects and sends them at the end, so
/// the draw order differs. The client only uses the seed to add a little pitch
/// variation; every other field is still compared byte for byte.
fn normalize_attack_effect(bytes: &[u8]) -> Vec<u8> {
    if packet_id(bytes) == Some(mc_bamboo_data::packets::play::clientbound::SOUND)
        && bytes.len() >= 8
    {
        bytes[..bytes.len() - 8].to_vec()
    } else {
        bytes.to_vec()
    }
}

/// 攻击场景的跟踪包比较：两侧都已按包排序，逐项比字节。
///
/// Tracking comparison for attack scenes: both sides are sorted by packet, then
/// compared byte for byte.
fn compare_attack_packets(
    mismatches: &mut Vec<String>,
    show: &usize,
    name: &str,
    tick: usize,
    ours: &[Vec<u8>],
    official: &[Vec<u8>],
) {
    if mismatches.len() >= *show {
        return;
    }
    let place = format!("{name} tick {tick} effect packets");
    if ours.len() != official.len() {
        mismatches.push(format!(
            "{place}: official {} packets {:?}, bamboo {} packets {:?}",
            official.len(),
            official.iter().map(|b| hex(b)).collect::<Vec<_>>(),
            ours.len(),
            ours.iter().map(|b| hex(b)).collect::<Vec<_>>(),
        ));
        return;
    }
    for (index, (ours, expected)) in ours.iter().zip(official).enumerate() {
        if ours != expected {
            mismatches.push(format!(
                "{place}[{index}]: official {}, bamboo {}",
                hex(expected),
                hex(ours)
            ));
            return;
        }
    }
}

fn is_attack_scene(name: &str) -> bool {
    name.ends_with("attack_cow") || name.starts_with("player_attack_")
}

/// 只发给玩家自己的包：`player_combat_enter` / `hurt_animation` / `set_health`。
fn is_self_packet(bytes: &[u8]) -> bool {
    matches!(
        packet_id(bytes),
        Some(
            mc_bamboo_data::packets::play::clientbound::PLAYER_COMBAT_ENTER
                | mc_bamboo_data::packets::play::clientbound::HURT_ANIMATION
                | mc_bamboo_data::packets::play::clientbound::SET_HEALTH
        )
    )
}

/// 1b 重生序列里逐字节对照的关键包 ID。
fn is_life_key_packet(bytes: &[u8]) -> bool {
    use mc_bamboo_data::packets::play::clientbound as ids;
    matches!(
        packet_id(bytes),
        Some(
            ids::PLAYER_COMBAT_KILL
                | ids::SYSTEM_CHAT
                | ids::RESPAWN
                | ids::PLAYER_POSITION
                | ids::SET_DEFAULT_SPAWN_POSITION
                | ids::CHANGE_DIFFICULTY
                | ids::SET_EXPERIENCE
                | ids::PLAYER_ABILITIES
                | ids::SET_HEALTH
                | ids::ENTITY_EVENT
                | ids::DAMAGE_EVENT
                | ids::SOUND
        )
    )
}

/// 玩家受伤产生的、发给跟踪者的包：`damage_event` 和带种子的 hurt 音效。
fn is_tracking_hurt_packet(bytes: &[u8]) -> bool {
    matches!(
        packet_id(bytes),
        Some(
            mc_bamboo_data::packets::play::clientbound::DAMAGE_EVENT
                | mc_bamboo_data::packets::play::clientbound::SOUND
        )
    )
}

/// 逐刻比两组包字节。`official_b64` 是官方编码后的 base64 串。
fn compare_player_packets(
    mismatches: &mut Vec<String>,
    show: &usize,
    name: &str,
    tick: usize,
    group: &str,
    ours: &[Vec<u8>],
    official_b64: &[String],
) {
    if mismatches.len() >= *show {
        return;
    }
    let place = format!("{name} tick {tick} {group} packets");
    if ours.len() != official_b64.len() {
        mismatches.push(format!(
            "{place}: official {} packets {:?}, bamboo {} packets {:?}",
            official_b64.len(),
            official_b64
                .iter()
                .map(|s| hex(&base64_stub::decode(s).unwrap_or_default()))
                .collect::<Vec<_>>(),
            ours.len(),
            ours.iter().map(|b| hex(b)).collect::<Vec<_>>(),
        ));
        return;
    }
    for (index, (ours, text)) in ours.iter().zip(official_b64).enumerate() {
        let Ok(expected) = base64_stub::decode(text) else {
            mismatches.push(format!(
                "{place}[{index}]: bad base64 from the official side"
            ));
            return;
        };
        if ours != &expected {
            mismatches.push(format!(
                "{place}[{index}]: official {}, bamboo {}",
                hex(&expected),
                hex(ours)
            ));
            return;
        }
    }
}

/// 把官方一侧的五个标志位填进快照，返回 `step` 要用的三个布尔。
fn apply_flags(snapshot: &mut Snapshot, bits: &str) -> (bool, bool, bool) {
    let bytes = bits.as_bytes();
    let flag = |index: usize| bytes.get(index) == Some(&b'1');
    snapshot.sync_position = flag(1);
    snapshot.fall_flying = flag(4);
    if flag(3) {
        // `getRequiresPrecisePosition()` 为真时官方发全量同步。
        snapshot.precise = true;
    }
    (flag(0), flag(1), flag(2))
}

/// 把新生成的掉落物和经验球转成可比的一条记录。
fn spawn_snapshot(
    _level: &MobLevel,
    spawned: &[mc_bamboo_blocks::entity::EntityPublic],
) -> Vec<Spawn> {
    use mc_bamboo_blocks::entity::KindPublic;
    spawned
        .iter()
        .filter_map(|public| {
            let (kind, item, count, value) = match &public.kind {
                KindPublic::Item { stack, .. } => ("item".to_owned(), stack.item, stack.count, 0),
                KindPublic::ExperienceOrb { value } => ("orb".to_owned(), 0, 0, *value),
                KindPublic::Mob(mob) => ("cow".to_owned(), 0, 0, mob.age),
                _ => return None,
            };
            Some(Spawn {
                id: public.uid,
                x: public.position.x.to_bits() as i64,
                y: public.position.y.to_bits() as i64,
                z: public.position.z.to_bits() as i64,
                kind,
                item,
                count,
                value,
                seed: None,
                hr: None,
                yr: None,
                uuid: None,
                egg: None,
                genes: None,
            })
        })
        .collect()
}

/// 右键交互逐刻比对：结果类别和交回玩家手里的物品。
fn compare_interactions(
    mismatches: &mut Vec<String>,
    show: &usize,
    name: &str,
    tick: usize,
    ours: &[Interaction],
    official: &[Interaction],
) {
    if mismatches.len() >= *show {
        return;
    }
    let place = format!("{name} tick {tick} interactions");
    if ours.len() != official.len() {
        mismatches.push(format!(
            "{place}: official {} results, bamboo {} results",
            official.len(),
            ours.len()
        ));
        return;
    }
    for (index, (ours, expected)) in ours.iter().zip(official).enumerate() {
        if ours != expected {
            mismatches.push(format!(
                "{place}[{index}]: official {expected:?}, bamboo {ours:?}"
            ));
            return;
        }
    }
}

/// 新生生物的场景起点朝向：（头，身体）。
///
/// A born mob's starting rotation: (head, body).
fn born_rotation(setup: &Spawn) -> (f32, f32) {
    let angle = |bits: Option<i32>| bits.map_or(0.0, |bits| f32::from_bits(bits as u32));
    (angle(setup.hr), angle(setup.yr))
}

/// 把官方给的构造随机数、朝向和 UUID 写进一头新生的生物；实体不在世界里时返回假。
///
/// Writes the official constructor random, rotation and UUID into a born mob; false when
/// the entity is not in the world.
fn apply_born_setup(world: &mut World, uid: i32, setup: &Spawn) -> bool {
    let seed = setup.seed.unwrap_or(0);
    let (hr, yr) = born_rotation(setup);
    let uuid = setup
        .uuid
        .as_deref()
        .and_then(parse_uuid)
        .unwrap_or([0; 16]);
    let egg = setup.egg;
    let mut found = false;
    world.with_entity_mut(uid, &mut |entity| {
        let Some(mob) = entity.kind.mob_mut() else {
            return;
        };
        found = true;
        mob.uuid = uuid;
        if let Some(egg) = egg {
            mob.egg_time = egg;
        }
        if let Some([main, hidden]) = setup.genes {
            use mc_bamboo_blocks::entity::mob::panda;
            mob.panda.main_gene = panda::Gene::from_id(i32::from(main));
            mob.panda.hidden_gene = panda::Gene::from_id(i32::from(hidden));
            panda::set_attributes(mob);
        }
        mob.y_head_rot = hr;
        mob.y_head_rot_o = hr;
        mob.y_rot_o = yr;
        entity.random = JavaRandom::from_state(seed);
        entity.y_rot = yr;
    });
    found
}

/// 死亡掉落逐刻比对：顺序、种类、物品数量、经验值和位置都按位比。
fn compare_spawns(
    mismatches: &mut Vec<String>,
    show: &usize,
    name: &str,
    tick: usize,
    ours: &[Spawn],
    official: &[Spawn],
) {
    if mismatches.len() >= *show {
        return;
    }
    let place = format!("{name} tick {tick} spawns");
    if ours.len() != official.len() {
        mismatches.push(format!(
            "{place}: official {} entities {:?}, bamboo {} entities {:?}",
            official.len(),
            official,
            ours.len(),
            ours
        ));
        return;
    }
    for (index, (ours, expected)) in ours.iter().zip(official).enumerate() {
        // 幼崽的构造随机数 / 朝向 / UUID 是场景起点，不作为行为结果比对。
        let same = if expected.kind == "cow" {
            ours.id == expected.id
                && ours.kind == expected.kind
                && ours.value == expected.value
                && ours.x == expected.x
                && ours.y == expected.y
                && ours.z == expected.z
        } else {
            ours == expected
        };
        if same {
            continue;
        }
        mismatches.push(format!(
            "{place}[{index}]: official {expected:?}, bamboo {ours:?}"
        ));
        return;
    }
}

/// 逐刻比字节：每个包单独比，长度不同或任意字节不同都记一条不一致。
fn compare_packet_list(
    mismatches: &mut Vec<String>,
    show: &usize,
    name: &str,
    tick: usize,
    ours: &[Vec<u8>],
    official_b64: &[String],
) {
    if mismatches.len() >= *show {
        return;
    }
    let place = format!("{name} tick {tick} packets");
    if ours.len() != official_b64.len() {
        mismatches.push(format!(
            "{place}: official {} packets, bamboo {} packets",
            official_b64.len(),
            ours.len()
        ));
        if std::env::var_os("MOB_ORACLE_PACKET_DUMP").is_some() {
            for (i, body) in ours.iter().enumerate() {
                eprintln!("{place} bamboo[{i}] {}", hex(body));
            }
            for (i, text) in official_b64.iter().enumerate() {
                eprintln!(
                    "{place} official[{i}] {}",
                    hex(&base64_stub::decode(text).unwrap_or_default())
                );
            }
        }
        return;
    }
    for (index, (ours, text)) in ours.iter().zip(official_b64).enumerate() {
        let Ok(expected) = base64_stub::decode(text) else {
            mismatches.push(format!(
                "{place}[{index}]: bad base64 from the official side"
            ));
            return;
        };
        if ours != &expected {
            let first = ours
                .iter()
                .zip(&expected)
                .position(|(a, b)| a != b)
                .unwrap_or(ours.len().min(expected.len()));
            mismatches.push(format!(
                "{place}[{index}]: official {} bytes, bamboo {} bytes, first difference at {}: official {}, bamboo {}",
                expected.len(),
                ours.len(),
                first,
                hex(&expected[first..expected.len().min(first + 12)]),
                hex(&ours[first..ours.len().min(first + 12)]),
            ));
            if std::env::var_os("MOB_ORACLE_PACKET_DUMP").is_some() {
                eprintln!("{place} official {}", hex(&expected));
                eprintln!("{place} bamboo   {}", hex(ours));
            }
            return;
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 两张查表的 FNV-1a 哈希，和 `MobOracle.tableHashes` 同一个算法。
fn table_hashes() -> [i64; 2] {
    let mut sin = 0xcbf2_9ce4_8422_2325_u64;
    for value in mc_bamboo_core::mth::sin_table() {
        sin = (sin ^ u64::from(value.to_bits())).wrapping_mul(0x0100_0000_01b3);
    }
    let (asin, cos) = mc_bamboo_core::mth::atan_tables();
    let mut atan = 0xcbf2_9ce4_8422_2325_u64;
    for (first, second) in asin.iter().zip(cos) {
        atan = (atan ^ first.to_bits()).wrapping_mul(0x0100_0000_01b3);
        atan = (atan ^ second.to_bits()).wrapping_mul(0x0100_0000_01b3);
    }
    [sin as i64, atan as i64]
}

/// 真实时间模式：这一刻的主世界时钟和由它算出的天色、白天、`MONSTERS_BURN`。
/// 和官方一侧一样，在实体 tick 之前更新。
impl Report {
    fn no_break_doors(&self) -> bool {
        self.break_test.as_deref() == Some("zombie_door")
    }
}

/// `ServerChunkCache.tickSpawningChunk` 第一句：候选区块的 `InhabitedTime` 加一。
/// 官方一刻只加一次，在实体 tick 之前；和游戏层共用候选区块的纯计算。
/// 改坏确认 `zombie_inhabited`：跳过一刻（第 3 刻不加）。
fn accumulate_inhabited(level: &mut MobLevel, tick: i64, break_test: Option<&str>) {
    if !level.facts.accumulates_inhabited || (break_test == Some("zombie_inhabited") && tick == 3) {
        return;
    }
    let players = level
        .players
        .iter()
        .map(|player| (player.position, player.spectator));
    for chunk in mc_bamboo_game::spawning_chunk_candidates(players, |_| true) {
        *level.inhabited.entry(chunk).or_default() += 1;
    }
}

/// 0 号玩家右键床头：走正式的 `use_item_on`，从 `Effect::SleepInBed` 读结果，
/// 名字和官方 `BedSleepingProblem` 一样，成功是 `OK`。
fn try_sleep(level: &mut MobLevel, ticks: &mut BlockTicks, case: &Case, head: BlockPos) -> String {
    use mc_bamboo_blocks::{
        BedProblem, Effect,
        context::{Actor, BlockHit},
    };

    let Some(player) = case.players.first() else {
        return String::new();
    };
    let position = Vec3::new(
        f64::from_bits(player.x as u64),
        f64::from_bits(player.y as u64),
        f64::from_bits(player.z as u64),
    );
    let creative = case.player_creative;
    let actor = Actor {
        uuid: ((1_u128 << 64) | 2).to_be_bytes(),
        position,
        yaw: 0.0,
        head_yaw: 0.0,
        pitch: 0.0,
        secondary_use: false,
        main_hand: mc_bamboo_data::items::AIR,
        off_hand: mc_bamboo_data::items::AIR,
        eye_height: f64::from(f32::from_bits(player.eye as u32)),
        block_interaction_range: if creative { 5.0 } else { 4.5 },
        may_build: true,
        prevents_block_drops: creative,
        can_use_game_master_blocks: false,
    };
    let hit = BlockHit {
        pos: head,
        face: mc_bamboo_core::Direction::Up,
        location: Vec3::new(
            f64::from(head.x) + 0.5,
            f64::from(head.y) + 0.5625,
            f64::from(head.z) + 0.5,
        ),
        inside: false,
    };
    let outcome = mc_bamboo_blocks::interact::use_item_on(
        level,
        ticks,
        &actor,
        mc_bamboo_core::Hand::Main,
        &hit,
    );
    for effect in &outcome.effects {
        if let Effect::SleepInBed { problem, .. } = effect {
            return match problem {
                None => "OK",
                Some(BedProblem::NotPossibleNow) => "NOT_POSSIBLE_NOW",
                Some(BedProblem::NotSafe) => "NOT_SAFE",
                Some(BedProblem::TooFarAway) => "TOO_FAR_AWAY",
                Some(BedProblem::Obstructed) => "OBSTRUCTED",
                Some(BedProblem::OtherProblem) => "OTHER_PROBLEM",
            }
            .to_owned();
        }
    }
    "NO_SLEEP".to_owned()
}

fn set_zombie_clock(level: &mut MobLevel, case: &Case, tick: i64, break_test: Option<&str>) {
    if case.clock_start < 0 {
        return;
    }
    let mut clock = case.clock_start + tick;
    if break_test == Some("zombie_clock") {
        // 改坏确认：时钟慢半天，白天的场景变成夜里。
        clock += 12_000;
    }
    let rain = f32::from_bits(case.rain as u32);
    level.facts.clock = clock;
    level.facts.raining = rain > 0.2;
    level.facts.sky_darken = mc_bamboo_blocks::sky::sky_darken(clock, rain, 0.0);
    level.facts.bright_outside = level.facts.sky_darken < 4;
    level.facts.monsters_burn = mc_bamboo_blocks::sky::monsters_burn(clock);
}

fn snapshot(level: &MobLevel, ids: &[i32]) -> Vec<Option<CowStep>> {
    let lr = level.random.source().seed();
    let bh = block_hash(level);
    ids.iter()
        .map(|&id| {
            level.entities.get(id).and_then(|entity| {
                step_of(&entity, lr).map(|mut step| {
                    step.bh = bh;
                    // 官方 `ServerLevel.getCurrentDifficultyAt(mob.blockPosition())`：
                    // 时钟、该区块的 `InhabitedTime`、月相。只在 `InhabitedTime`
                    // 场景逐刻比；其余场景官方不写 `ed`，Bamboo 也填 0。
                    step.ed = if level.facts.accumulates_inhabited {
                        let chunk = ChunkPos::containing(entity.position.x, entity.position.z);
                        mc_bamboo_blocks::sky::effective_difficulty(
                            level.facts.difficulty,
                            level.facts.clock,
                            level.inhabited_time(chunk),
                            mc_bamboo_blocks::sky::moon_brightness(level.facts.clock),
                        )
                        .to_bits() as i32
                    } else {
                        0
                    };
                    // `Frog.getTongueTarget()`：目标已经不在世界里时是空的。
                    // `Frog.getTongueTarget()` is empty once the target has left the world.
                    let frog = entity.kind.mob().is_some_and(|mob| {
                        mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Frog
                    });
                    if frog
                        && step.goat >> 4 != 0
                        && level.entities.get((step.goat >> 4) - 1).is_none()
                    {
                        step.goat &= 15;
                    }
                    step.brain = brain::dump(level, &entity);
                    step
                })
            })
        })
        .collect()
}

/// 和 Java 一侧 `blockHash` 同一个算法：非空气格子按 (x, y, z) 排序后做 FNV-1a。
fn block_hash(level: &MobLevel) -> i64 {
    let mut cells: Vec<[i64; 4]> = level
        .blocks
        .iter()
        .filter(|(_, state)| !state.is_air())
        .map(|(pos, state)| {
            [
                i64::from(pos.x),
                i64::from(pos.y),
                i64::from(pos.z),
                i64::from(state.0),
            ]
        })
        .collect();
    cells.sort_unstable();
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 as i64;
    for cell in cells {
        for value in cell {
            hash = (hash ^ value).wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

/// 史莱姆场景里 `Player.aiStep` 的 `touch(entity)`：玩家活着时，包围盒外扩 (1, 0.5, 1) 碰到的
/// 史莱姆各调一次 `playerTouch`。玩家的包围盒是站立的 0.6 × 1.8。
///
/// `touch(entity)` from `Player.aiStep` for slime scenes: a living player calls `playerTouch`
/// on each slime within its box inflated by (1, 0.5, 1). The player box is the standing
/// 0.6 × 1.8.
fn touch_slimes(world: &mut World, ids: &[i32], players: &[PlayerSetup]) {
    for player in players {
        let feet = Vec3::new(
            f64::from_bits(player.x as u64),
            f64::from_bits(player.y as u64),
            f64::from_bits(player.z as u64),
        );
        let half = f64::from(0.6_f32) / 2.0;
        let reach = mc_bamboo_core::Aabb::new(
            feet.x - half,
            feet.y,
            feet.z - half,
            feet.x + half,
            feet.y + f64::from(1.8_f32),
            feet.z + half,
        )
        .inflate(1.0, 0.5, 1.0);
        for &id in ids {
            let touching = world
                .entity(id)
                .is_some_and(|entity| !entity.removed && entity.bounding_box().intersects(reach));
            if touching {
                mc_bamboo_blocks::entity::mob::slime::player_touch(world, id, player.id);
            }
        }
    }
}

/// 这一刻新生成的小史莱姆：记下和官方一侧同样的六个数，然后丢掉，不让它们继续 tick。
///
/// The slimes spawned this tick: record the same six numbers as the vanilla side, then
/// discard them so they never tick.
fn take_cube_splits(
    level: &mut MobLevel,
    ticks: &mut BlockTicks,
    spawned: &[mc_bamboo_blocks::entity::EntityPublic],
) -> Vec<[i64; 6]> {
    let mut out = Vec::new();
    let mut world = World::new(level, ticks);
    for public in spawned {
        let mc_bamboo_blocks::entity::KindPublic::Mob(mob) = &public.kind else {
            continue;
        };
        if mob.mob_type != mc_bamboo_blocks::entity::mob::MobType::Slime {
            continue;
        }
        out.push([
            public.position.x.to_bits() as i64,
            public.position.y.to_bits() as i64,
            public.position.z.to_bits() as i64,
            i64::from(public.yaw.to_bits() as i32),
            i64::from(mob.cube_size),
            i64::from(mob.health.to_bits() as i32),
        ]);
        world.with_entity_mut(public.uid, &mut |entity| entity.discard());
    }
    out
}

fn step_of(entity: &Entity, lr: i64) -> Option<CowStep> {
    let EntityKind::Mob(mob) = &entity.kind else {
        return None;
    };
    Some(step_of_mob(entity, mob, lr))
}

/// 解析 Java `UUID.toString()` 的 8-4-4-4-12 位十六进制。
fn parse_uuid(text: &str) -> Option<[u8; 16]> {
    let mut hex = String::with_capacity(32);
    for part in text.split('-') {
        hex.push_str(part);
    }
    if hex.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// Java `UUID.toString()`：8-4-4-4-12 位小写十六进制。
fn uuid_text(bytes: &[u8; 16]) -> String {
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn is_wolf(mob: &Mob) -> bool {
    mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Wolf
}

fn step_of_mob(entity: &Entity, mob: &Mob, lr: i64) -> CowStep {
    let path = mob.navigation.path.as_ref().map(|path| {
        let mut values = vec![path.next_node_index as i32];
        for node in &path.nodes {
            values.extend([node.x, node.y, node.z]);
        }
        values
    });
    CowStep {
        x: entity.position.x.to_bits() as i64,
        y: entity.position.y.to_bits() as i64,
        z: entity.position.z.to_bits() as i64,
        dx: entity.delta.x.to_bits() as i64,
        dy: entity.delta.y.to_bits() as i64,
        dz: entity.delta.z.to_bits() as i64,
        yr: entity.y_rot.to_bits() as i32,
        xr: mob.x_rot.to_bits() as i32,
        hr: mob.y_head_rot.to_bits() as i32,
        br: mob.y_body_rot.to_bits() as i32,
        hp: mob.health.to_bits() as i32,
        g: entity.on_ground,
        rm: entity.removed,
        w: entity.in_water,
        swimming: mob.swimming,
        fd: entity.fall_distance.to_bits() as i64,
        nat: mob.no_action_time,
        ht: mob.hurt_time,
        dt: mob.death_time,
        fire: entity.remaining_fire_ticks,
        air: mob.air_supply,
        seed: entity.random.state(),
        age: if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Tadpole {
            // 蝌蚪：自己的年龄，锁住时再加 `1 << 20`。
            // Tadpoles: their own age, plus `1 << 20` while locked.
            mob.tadpole_age + if mob.age_locked { 1 << 20 } else { 0 }
        } else {
            mob.age
        },
        love: mob.in_love,
        lh: mob.left_handed,
        fr: mob
            .attributes
            .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE)
            .to_bits() as i64,
        var: farm_variant(mob),
        snd: farm_sound(mob),
        uuid: uuid_text(&mob.uuid),
        lr,
        goals: mob.goals.running_names_of(mob).join(" "),
        path,
        target: mob.target.unwrap_or(-1),
        // 僵尸式场景额外比较僵尸和马科的幼年标志；其他动物比较 age。
        // Zombie-style scenes compare the baby flag for zombies and equines; other animals use age.
        baby: (mob.mob_type.uses_baby_metadata() || mob.mob_type.is_equine()) && mob.is_baby(),
        aggr: mob.aggressive,
        doors: mob.can_break_doors,
        ph: 0,
        pht: 0,
        pcd: 0,
        bh: 0,
        rabbit_carrot_ticks: mob.rabbit.more_carrot_ticks,
        fox_flags: if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Fox {
            i32::from(mob.fox.flags)
        } else {
            0
        },
        fox_mouth: if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Fox {
            mob.equipment
                .get(mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand)
                .item
        } else {
            0
        },
        goat: goat_flags(mob),
        wolf_tame: if is_wolf(mob) {
            i32::from(mob.tame.flags)
        } else {
            0
        },
        wolf_sit: is_wolf(mob) && mob.tame.ordered_to_sit,
        wolf_owned: is_wolf(mob) && mob.tame.owner.is_some(),
        wolf_anger: if is_wolf(mob) {
            mob.wolf.anger_end_time
        } else {
            0
        },
        wolf_beg: is_wolf(mob) && mob.wolf.interested,
        wolf_collar: if is_wolf(mob) { mob.wolf.collar } else { 0 },
        wolf_max_hp: if is_wolf(mob) {
            let max = mob
                .attributes
                .value(mc_bamboo_data::registries::attribute::MAX_HEALTH);
            i64::from((max as f32).to_bits())
        } else {
            0
        },
        hd: {
            let head = mob
                .equipment
                .get(mc_bamboo_protocol::packet::play::EquipmentSlot::Head);
            if head.is_empty() {
                -1
            } else {
                mc_bamboo_blocks::durability::damage_of(head).unwrap_or(0)
            }
        },
        hc: mob
            .equipment
            .get(mc_bamboo_protocol::packet::play::EquipmentSlot::Head)
            .count
            .max(0),
        armor: mob
            .attributes
            .value(mc_bamboo_data::registries::attribute::ARMOR)
            .to_bits() as i64,
        eq: mob
            .equine
            .as_ref()
            .map_or(0, |state| i32::from(state.flags())),
        tmp: mob.equine.as_ref().map_or(0, |state| state.temper),
        brain: String::new(),
        // 官方只对马科打印 `rider_count`，其它生物缺省是 0。
        // Vanilla prints `rider_count` only for equines; other mobs default to 0.
        rider_count: if mob.mob_type.is_equine() {
            mob.passengers.len()
        } else {
            0
        },
        owner: mob
            .equine
            .as_ref()
            .and_then(|state| state.owner.as_ref())
            .map(uuid_text),
        turtle_egg: i32::from(mob.turtle.has_egg),
        turtle_laying: i32::from(mob.turtle.laying_egg),
        turtle_lay_count: mob.turtle.lay_egg_counter,
        turtle_going_home: mob.turtle.going_home,
        turtle_home_x: mob.turtle.home_pos.x,
        turtle_home_y: mob.turtle.home_pos.y,
        turtle_home_z: mob.turtle.home_pos.z,
        ed: 0,
    }
}

/// 山羊的四个标志位，和 Java 一侧 `zombieStepJson` 的 `goat` 一致。
///
/// The goat's four flag bits, matching `goat` in the Java `zombieStepJson`.
fn goat_flags(mob: &mc_bamboo_blocks::entity::mob::Mob) -> i32 {
    if mob.mob_type == mc_bamboo_blocks::entity::mob::MobType::Frog {
        // 青蛙借这一列：姿势占低 4 位，舌头目标的实体 ID 加一放在高位。
        // Frogs borrow this column: the pose in the low four bits and the tongue
        // target's entity id plus one above them.
        use mc_bamboo_blocks::entity::mob::frog::Pose;
        let pose = match mob.frog.pose {
            Pose::Standing => 0,
            Pose::Croaking => 1,
            Pose::LongJumping => 2,
            Pose::UsingTongue => 3,
        };
        return pose | mob.frog.tongue_target.map_or(0, |uid| uid + 1) << 4;
    }
    if mob.mob_type != mc_bamboo_blocks::entity::mob::MobType::Goat {
        return 0;
    }
    let goat = mob.goat;
    i32::from(goat.screaming)
        | i32::from(goat.left_horn) << 1
        | i32::from(goat.right_horn) << 2
        | i32::from(goat.long_jumping) << 3
}

/// 对拍里的花纹列：羊是颜色加剪毛标记，鸡还带下蛋计时。
///
/// The oracle's variant column: colour and shear mark for sheep, plus the egg timer for chickens.
fn farm_variant(mob: &mc_bamboo_blocks::entity::mob::Mob) -> String {
    use mc_bamboo_blocks::entity::mob::{MobType, sheep};
    match mob.mob_type {
        // 史莱姆把自己的状态塞进这一列：大小、跳跃延迟、移动控制的目标角度、上一刻是否在地上。
        // Slimes pack their own state here: size, jump delay, the move control's target angle
        // and whether they were on the ground.
        MobType::Slime => format!(
            "slime:{}:{}:{}:{}",
            mob.cube.size,
            mob.cube.jump_delay,
            mob.cube.move_y_rot.to_bits() as i32,
            mob.cube.was_on_ground
        ),
        MobType::Witch => {
            use mc_bamboo_data::registries::mob_effect;
            let duration = |effect| mob.effects.get(effect).map_or(0, |entry| entry.duration);
            format!(
                "witch:{}:{}:{}:{}:{}:{}",
                mob.witch_drinking,
                mob.witch.using_time,
                duration(mob_effect::FIRE_RESISTANCE),
                duration(mob_effect::INSTANT_HEALTH),
                duration(mob_effect::WATER_BREATHING),
                duration(mob_effect::SPEED)
            )
        }
        MobType::Pillager => pillager::state(mob),
        MobType::Sheep => format!(
            "{}{}",
            sheep::color(mob).name(),
            if sheep::is_sheared(mob) {
                "|sheared"
            } else {
                ""
            }
        ),
        MobType::Chicken => format!("{}|egg{}", mob.variant.name(), mob.egg_time),
        MobType::PolarBear => format!(
            "s{}|w{}|e{}|t{}",
            u8::from(mob.polar_bear.standing),
            mob.polar_bear.warning_sound_ticks,
            mob.anger.end_time,
            mob.anger
                .target
                .map_or_else(|| "-".to_owned(), |uuid| uuid_text(&uuid)),
        ),
        MobType::Mooshroom => format!(
            "{}{}",
            mob.mooshroom.type_name(),
            if mob.mooshroom.stew_effects.is_some() {
                "|stew"
            } else {
                ""
            }
        ),
        MobType::Panda => format!(
            "g{}/{}|u{}|n{}|e{}|f{}",
            mob.panda.main_gene.id(),
            mob.panda.hidden_gene.id(),
            mob.panda.unhappy_counter,
            mob.panda.sneeze_counter,
            mob.panda.eat_counter,
            mob.panda.flags,
        ),
        MobType::Ocelot => format!(
            "t{}|c{}|s{}",
            u8::from(mob.ocelot.trusting),
            u8::from(mob.ocelot.crouching),
            u8::from(mob.sprinting),
        ),
        MobType::Cat => format!(
            "t{}|o{}|p{}|l{}|r{}|c{}|v{}|cr{}|s{}|u{}",
            u8::from(mob.tame.is_tame()),
            u8::from(mob.tame.ordered_to_sit),
            u8::from(mob.tame.in_sitting_pose()),
            u8::from(mob.cat.lying),
            u8::from(mob.cat.relax_state_one),
            mob.cat.collar,
            mob.cat.variant_name(),
            u8::from(mob.cat.crouching),
            u8::from(mob.sprinting),
            mob.tame
                .owner
                .map_or_else(|| "-".to_owned(), |uuid| uuid_text(&uuid)),
        ),
        MobType::Parrot => format!(
            "t{}|o{}|s{}|v{}|r{}",
            u8::from(mob.tame.is_tame()),
            u8::from(mob.tame.ordered_to_sit),
            u8::from(mob.tame.in_sitting_pose()),
            mob.parrot.variant,
            mob.parrot.ride_cooldown,
        ),
        MobType::Creeper => mc_bamboo_blocks::entity::mob::creeper::describe(mob),
        MobType::Enderman => mob.describe_enderman(),
        MobType::Spider | MobType::CaveSpider => {
            mc_bamboo_blocks::entity::mob::spider::describe(mob)
        }
        _ => mob.variant.name().to_owned(),
    }
}

/// 刷怪对拍的花色 / 羊毛颜色和叫声；鸡不带下蛋计时（官方取自没播种的实体随机源），
/// 不是农场动物时为 `None`。和 `MobOracle.spawnVariants` 对应。
///
/// The spawn oracles' variant or wool colour and sound; chickens omit the egg timer
/// (vanilla draws it from an unseeded entity random), and other mobs give `None`.
/// Mirrors `MobOracle.spawnVariants`.
fn spawn_variants(mob: &mc_bamboo_blocks::entity::mob::Mob) -> Option<[String; 2]> {
    use mc_bamboo_blocks::entity::mob::MobType;
    if mob.mob_type == MobType::Frog {
        // 青蛙：品种和 `FrogAi.initMemories` 抽的长跳冷却。
        // Frogs: the variant and the long-jump cooldown `FrogAi.initMemories` drew.
        let cooldown = mob
            .brain
            .as_deref()
            .and_then(|brain| {
                brain.memories.int(
                    mc_bamboo_blocks::entity::mob::brain::memory::MemoryType::LongJumpCooldownTicks,
                )
            })
            .unwrap_or(-1);
        return Some([mob.frog.variant.key().to_owned(), cooldown.to_string()]);
    }
    let variant = match mob.mob_type {
        MobType::Chicken => mob.variant.name().to_owned(),
        MobType::Cow | MobType::Pig | MobType::Sheep => farm_variant(mob),
        _ => return None,
    };
    Some([variant, farm_sound(mob)])
}

/// 对拍里的叫声列。
///
/// The oracle's sound variant column.
fn farm_sound(mob: &mc_bamboo_blocks::entity::mob::Mob) -> String {
    use mc_bamboo_blocks::entity::mob::MobType;
    match mob.mob_type {
        MobType::Pig => mob.pig_sound_variant.name().to_owned(),
        MobType::Chicken => mob.chicken_sound_variant.name().to_owned(),
        MobType::Cat => mob.cat.sound_variant_name().to_owned(),
        MobType::Parrot => String::new(),
        MobType::Sheep
        | MobType::Mooshroom
        | MobType::PolarBear
        | MobType::Panda
        | MobType::Ocelot => String::new(),
        _ => mob.sound_variant.name().to_owned(),
    }
}

/// 和游戏层一样给羊 / 鸡的实体谓词补组件（`interaction/level/farm_loot.rs`）。
///
/// Adds the sheep and chicken predicate fields, as the game layer does.
fn describe_farm_animal(
    view: &mut mc_bamboo_loot::EntityView,
    kind: mc_bamboo_blocks::entity::mob::MobType,
    wool: u8,
    variant: Option<mc_bamboo_blocks::entity::mob::CowVariant>,
) {
    use mc_bamboo_blocks::entity::mob::MobType;
    match (kind, variant) {
        (MobType::Sheep, _) => {
            let color = mc_bamboo_core::sign::DyeColor::from_id(i32::from(wool & 15));
            view.is_sheep = true;
            view.sheep_sheared = wool & 16 != 0;
            view.components.insert(
                "minecraft:sheep/color".to_owned(),
                serde_json::Value::String(color.name().to_owned()),
            );
        }
        (MobType::Mooshroom, _) => {
            let name = if wool == 0 { "red" } else { "brown" };
            view.components.insert(
                "minecraft:mooshroom/variant".to_owned(),
                serde_json::Value::String(name.to_owned()),
            );
        }
        (MobType::Chicken, Some(variant)) => {
            view.components.insert(
                "minecraft:chicken/variant".to_owned(),
                serde_json::Value::String(variant.name().to_owned()),
            );
        }
        _ => {}
    }
}

impl Report {
    /// 熊猫基因遗传：三方随机源的取数顺序和结果。
    ///
    /// Panda gene inheritance: the draw order over the three random sources and the result.
    fn replay_panda_genes(&mut self, case: &PandaGenesCase) {
        use mc_bamboo_blocks::entity::mob::{MobType, panda};
        self.cases += 1;
        let mut father = JavaRandom::new(case.father_seed);
        let mut mother = JavaRandom::new(case.mother_seed);
        let mut baby = Entity::by_mob_type(MobType::Panda, Vec3::default());
        baby.random = JavaRandom::new(case.baby_seed);
        let genes = panda::offspring_genes(
            &mut baby,
            &mut father,
            (
                panda::Gene::from_id(i32::from(case.father_main)),
                panda::Gene::from_id(i32::from(case.father_hidden)),
            ),
            &mut mother,
            (
                panda::Gene::from_id(i32::from(case.mother_main)),
                panda::Gene::from_id(i32::from(case.mother_hidden)),
            ),
        );
        let actual = (
            genes.0.id() as u8,
            genes.1.id() as u8,
            father.state(),
            mother.state(),
            baby.random.state(),
        );
        let expected = (
            case.baby_main,
            case.baby_hidden,
            case.father_after,
            case.mother_after,
            case.baby_after,
        );
        if actual != expected {
            self.mismatch(format!(
                "panda genes father {}/{} mother {}/{}: official {expected:?}, bamboo {actual:?}",
                case.father_main, case.father_hidden, case.mother_main, case.mother_hidden
            ));
        }
    }
}
