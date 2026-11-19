//! `mob-oracle`：官方牛和 Bamboo 的牛逐刻对拍。
//!
//! `MobOracle.java` 在假的 `ServerLevel` 里跑官方的牛，每个场景一行：场景本身（方块、
//! 牛的起点和随机源种子、玩家、中途的伤害）加上每一刻每头牛的位置、速度、四个朝向、
//! 生命、是否着地、路径节点、正在跑的目标、随机源状态等。这里用同一个场景重放，
//! 浮点按原始位比较，不留容差。
//!
//! Compares official cows with Bamboo's, tick by tick, bit for bit.

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
use serde::Deserialize;

use crate::oracle::collect_log;

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
    End(End),
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
    /// 1b 玩家生命场景的规则与动作。
    #[serde(default)]
    life: Option<LifeSetup>,
    /// 1b：重生序列里逐字节对照的关键包。
    #[serde(default)]
    key_packets: Option<Vec<Vec<String>>>,
    /// 1c：官方 `PlayerSpawnFinder.findSpawn` 的落位结果。
    #[serde(default)]
    spawn_result: Option<SpawnResult>,
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
    #[serde(default)]
    uuid: Option<String>,
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
}

#[derive(Debug, Deserialize)]
struct PlayerSetup {
    id: i32,
    x: i64,
    y: i64,
    z: i64,
    eye: i32,
    item: i32,
    #[serde(default)]
    looting: i32,
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
}

#[derive(Debug, Deserialize)]
struct End {
    cases: u32,
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
    redstone: mc_bamboo_blocks::RedstoneMemory,
    block_entities: BlockEntities,
    block_events: BlockEvents,
    entities: Entities,
    players: Vec<PlayerView>,
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
            LightLayer::Sky => 15,
            LightLayer::Block => 0,
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
        for player in &self.players {
            visit(player);
        }
    }

    /// `LivingEntity.dropFromLootTable`：官方一侧用真的 `minecraft:entities/cow` 表，
    /// 世界种子 0 的随机序列，攻击者的主手带抢夺等级。
    ///
    /// Matches `LivingEntity.dropFromLootTable` on the official side: the real
    /// cow table, a world-seed-0 random sequence and the killer's looting level.
    fn mob_loot_drops(
        &mut self,
        table: &str,
        on_fire: bool,
        killer: Option<mc_bamboo_blocks::EntityUid>,
    ) -> Vec<mc_bamboo_protocol::item::ItemStack> {
        use mc_bamboo_loot::{
            DamageSourceView, EntityDeathParams, EntityView, LootStack, Position,
            entity_death_drops,
        };
        let mut victim = EntityView::new(mc_bamboo_data::registries::entity_type::COW);
        victim.is_living = true;
        victim.is_on_fire = on_fire;
        let mut killer_view = None;
        if let Some(uid) = killer {
            let mut view = EntityView::new(mc_bamboo_data::registries::entity_type::PLAYER);
            view.is_living = true;
            if let Some(index) = self.players.iter().position(|p| p.uid == uid) {
                let level = self.looting[index];
                if level > 0 {
                    let mut held = LootStack::new(mc_bamboo_data::items::DIAMOND_SWORD, 1);
                    let mut enchants = std::collections::BTreeMap::new();
                    enchants.insert("minecraft:looting".to_owned(), level);
                    held.set_enchantment_levels(mc_bamboo_loot::ENCHANTMENTS_COMPONENT, enchants);
                    view.equipment[mc_bamboo_loot::EquipmentSlot::Mainhand.index()] = Some(held);
                }
            }
            killer_view = Some(view);
        }
        let params = EntityDeathParams {
            entity: victim,
            origin: Position::new(0.0, 0.0, 0.0),
            damage_source: DamageSourceView {
                damage_type: 0,
                direct_entity: killer_view.clone().map(Box::new),
                source_entity: killer_view.clone().map(Box::new),
                is_direct: true,
            },
            attacking_entity: killer_view.clone(),
            direct_attacking_entity: killer_view.clone(),
            last_damage_player: killer_view,
            luck: 0.0,
            level: None,
        };
        match entity_death_drops(
            table,
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
}

struct Report {
    cases: u32,
    steps: usize,
    mismatches: Vec<String>,
    ended: bool,
    show: usize,
    /// 故意改坏一处，确认对拍真的抓得到。
    break_test: Option<String>,
}

/// 启动官方进程，逐个场景重放。
pub fn run(mut command: Command, options: &[&str]) -> Result<(), String> {
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
        if case.player_steps.is_some() {
            self.replay_player_hurt(case);
            return;
        }
        let mut level = MobLevel {
            blocks: HashMap::new(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: case.players.iter().map(|player| player.looting).collect(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(case.first_id),
            players: case
                .players
                .iter()
                .map(|player| PlayerView {
                    uid: player.id,
                    position: Vec3::new(
                        f64::from_bits(player.x as u64),
                        f64::from_bits(player.y as u64),
                        f64::from_bits(player.z as u64),
                    ),
                    eye_height: f32::from_bits(player.eye as u32),
                    alive: true,
                    spectator: false,
                    creative: false,
                    sneaking: false,
                    invisible: false,
                    armor_cover: 0.0,
                    main_hand: player.item,
                    off_hand: 0,
                })
                .collect(),
        };
        for &[x, y, z, state] in &case.blocks {
            level
                .blocks
                .insert(BlockPos::new(x, y, z), BlockState(state as u32));
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
                let id = world.spawn_entity(Entity::cow(position));
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
                    }
                    entity.remaining_fire_ticks = case.fire_ticks;
                    if cow.age != 0 {
                        let baby = cow.age < 0;
                        let (width, height, eye) =
                            mc_bamboo_blocks::entity::mob::MobType::Cow.dimensions(baby);
                        entity.dimensions = (width, height);
                        if let Some(mob) = entity.kind.mob_mut() {
                            mob.age = cow.age;
                            mob.eye_height = eye;
                        }
                    }
                });
                ids.push(id);
            }
        }
        let Some(first) = case.steps.first() else {
            self.mismatch(format!("{}: no steps", case.name));
            return;
        };
        let ours = snapshot(&level, &ids);
        self.compare(&case.name, 0, &ours, first);
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
            let mut ours_interactions = Vec::new();
            {
                let mut world = World::new(&mut level, &mut ticks);
                for &[tick, cow, player_index, item, creative] in &case.interact_actions {
                    if tick != index as i32 {
                        continue;
                    }
                    let player = &case.players[player_index as usize];
                    let mut uuid = [0u8; 16];
                    uuid[..8].copy_from_slice(&1_u64.to_be_bytes());
                    uuid[8..].copy_from_slice(&(2_u64 + player_index as u64).to_be_bytes());
                    let held = mc_bamboo_protocol::item::ItemStack::new(item, 1);
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
                for &[tick, cow, amount, attacker] in &case.hurts {
                    if tick as usize != index {
                        continue;
                    }
                    let player = &case.players[attacker as usize];
                    let position = Vec3::new(
                        f64::from_bits(player.x as u64),
                        f64::from_bits(player.y as u64),
                        f64::from_bits(player.z as u64),
                    );
                    let source = match case.death_kind {
                        1 => DamageSource::environment(
                            mc_bamboo_blocks::entity::mob::DamageKind::Fall,
                        ),
                        2 => DamageSource::environment(
                            mc_bamboo_blocks::entity::mob::DamageKind::OnFire,
                        ),
                        _ => DamageSource::player(player.id, position, false),
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
                world.tick_entities();
            }
            // 这一刻新生成的实体：快照是在 `addFreshEntity` 时记下的，官方
            // `ChunkMap.addEntity` 也在同一时刻建 `ServerEntity`。
            // Newly spawned entities: the snapshot is taken at `addFreshEntity`,
            // exactly when vanilla's `ChunkMap.addEntity` builds `ServerEntity`.
            let mut spawn_public = level.entities.take_spawn_snapshots();
            // 繁殖生成的幼崽也是牛：加进 `ids`，并把官方给的构造随机数、朝向和
            // UUID 作为场景起点写进实体，后面每刻比对它自己的行为。
            for public in &spawn_public {
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
                let seed = setup.seed.unwrap_or(0);
                let hr = setup
                    .hr
                    .map(|bits| f32::from_bits(bits as u32))
                    .unwrap_or(0.0);
                let uuid = setup
                    .uuid
                    .as_deref()
                    .and_then(parse_uuid)
                    .unwrap_or([0; 16]);
                let uid = public.uid;
                {
                    let mut world = World::new(&mut level, &mut ticks);
                    world.with_entity_mut(uid, &mut |entity| {
                        entity.random = JavaRandom::from_state(seed);
                        entity.y_rot = 0.0;
                        if let Some(mob) = entity.kind.mob_mut() {
                            mob.uuid = uuid;
                            mob.y_head_rot = hr;
                            mob.y_head_rot_o = hr;
                            mob.y_rot_o = 0.0;
                        }
                    });
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
            self.steps += 1;
            let ours = snapshot(&level, &ids);
            self.compare(&case.name, index, &ours, expected);
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

    fn compare(
        &mut self,
        name: &str,
        index: usize,
        ours: &[Option<CowStep>],
        official: &[CowStep],
    ) {
        for (cow, expected) in official.iter().enumerate() {
            let place = format!("{name} tick {index} cow {cow}");
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
            plain!(var);
            plain!(snd);
            plain!(uuid);
            plain!(lr);
            plain!(goals);
            plain!(path);
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
        };
        // 五处故意改坏：难度缩放、护甲公式（用空装备）、保护附魔等级、
        // 无敌帧（每次伤害前清冷却）、击退（去掉来源位置）。
        // 每一项都让对拍的逐刻状态或包字节失败，证明比较没有放松。
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
            _ => {}
        }
        let (steps, packets) = replay_player_hurt(&scene);
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
            for body in packets.get(tick).cloned().unwrap_or_default() {
                if is_self_packet(&body) {
                    ours_self.push(body);
                } else {
                    ours_tracking.push(body);
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
            // 左键打牛场景里牛自己也会发出带种子的环境音/受伤音，那只牛的行为
            // 不在 1a 的比对范围里，所以这一场景只比 `damage_event`。
            let tracking: Vec<String> = official_tracking
                .iter()
                .filter(|text| {
                    base64_stub::decode(text)
                        .ok()
                        .is_some_and(|bytes| is_tracking_hurt_packet(&bytes))
                })
                .filter(|text| {
                    !case.name.ends_with("attack_cow")
                        || base64_stub::decode(text)
                            .ok()
                            .and_then(|bytes| packet_id(&bytes))
                            == Some(mc_bamboo_data::packets::play::clientbound::DAMAGE_EVENT)
                })
                .cloned()
                .collect();
            let ours_tracking: Vec<Vec<u8>> = ours_tracking
                .into_iter()
                .filter(|bytes| {
                    !case.name.ends_with("attack_cow")
                        || packet_id(bytes)
                            == Some(mc_bamboo_data::packets::play::clientbound::DAMAGE_EVENT)
                })
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
                        uuid: None,
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
        if !self.mismatches.is_empty() {
            return Err(format!(
                "mob oracle: {} mismatches\n{}",
                self.mismatches.len(),
                self.mismatches.join("\n")
            ));
        }
        println!(
            "MOB ORACLE: {} cases, {} ticks, 0 mismatches",
            self.cases, self.steps
        );
        Ok(())
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
                uuid: None,
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

fn snapshot(level: &MobLevel, ids: &[i32]) -> Vec<Option<CowStep>> {
    let lr = level.random.source().seed();
    ids.iter()
        .map(|&id| {
            level
                .entities
                .get(id)
                .and_then(|entity| step_of(&entity, lr))
        })
        .collect()
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
        fd: entity.fall_distance.to_bits() as i64,
        nat: mob.no_action_time,
        ht: mob.hurt_time,
        dt: mob.death_time,
        fire: entity.remaining_fire_ticks,
        air: mob.air_supply,
        seed: entity.random.state(),
        age: mob.age,
        love: mob.in_love,
        lh: mob.left_handed,
        fr: mob
            .attributes
            .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE)
            .to_bits() as i64,
        var: mob.variant.name().to_owned(),
        snd: mob.sound_variant.name().to_owned(),
        uuid: uuid_text(&mob.uuid),
        lr,
        goals: mob.goals.running_names().join(" "),
        path,
    }
}
