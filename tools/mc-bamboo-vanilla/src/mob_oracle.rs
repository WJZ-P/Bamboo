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
        mob::{DamageSource, Mob},
    },
};
use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, Vec3, sign::SignContent};
use mc_bamboo_data::version;
use serde::Deserialize;

use crate::oracle::collect_log;

const MIN_Y: i32 = -64;
const MAX_Y: i32 = 319;
const USAGE: &str = "usage: mob-oracle <java> [--only PREFIX] [--show N]";

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
    steps: Vec<Vec<CowStep>>,
}

#[derive(Debug, Deserialize)]
struct CowSetup {
    x: i64,
    y: i64,
    z: i64,
    yaw: i32,
    seed: i64,
}

#[derive(Debug, Deserialize)]
struct PlayerSetup {
    id: i32,
    x: i64,
    y: i64,
    z: i64,
    eye: i32,
    item: i32,
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
}

struct Report {
    cases: u32,
    steps: usize,
    mismatches: Vec<String>,
    ended: bool,
    show: usize,
}

/// 启动官方进程，逐个场景重放。
pub fn run(mut command: Command, options: &[&str]) -> Result<(), String> {
    let mut show = 12;
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
        let mut level = MobLevel {
            blocks: HashMap::new(),
            random: LevelRandom::new(1),
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
        {
            let mut world = World::new(&mut level, &mut ticks);
            for cow in &case.cows {
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
                        mob.x_rot = 0.0;
                        mob.y_rot_o = yaw;
                        mob.x_rot_o = 0.0;
                        mob.y_head_rot = yaw;
                        mob.y_body_rot = yaw;
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
        for (index, expected) in case.steps.iter().enumerate().skip(1) {
            ticks.set_time(case.start_time + index as i64);
            {
                let mut world = World::new(&mut level, &mut ticks);
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
                    world.hurt_mob(
                        ids[cow as usize],
                        DamageSource::player(player.id, position, false),
                        f32::from_bits(amount as u32),
                    );
                }
                world.tick_entities();
            }
            self.steps += 1;
            let ours = snapshot(&level, &ids);
            self.compare(&case.name, index, &ours, expected);
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
            plain!(goals);
            plain!(path);
            self.mismatch(format!("{place}: {}", diffs.join("; ")));
        }
    }

    fn mismatch(&mut self, message: String) {
        if self.mismatches.len() < self.show.max(1) {
            self.mismatches.push(message);
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
    ids.iter()
        .map(|&id| level.entities.get(id).and_then(|entity| step_of(&entity)))
        .collect()
}

fn step_of(entity: &Entity) -> Option<CowStep> {
    let EntityKind::Mob(mob) = &entity.kind else {
        return None;
    };
    Some(step_of_mob(entity, mob))
}

fn step_of_mob(entity: &Entity, mob: &Mob) -> CowStep {
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
        goals: mob.goals.running_names().join(" "),
        path,
    }
}
