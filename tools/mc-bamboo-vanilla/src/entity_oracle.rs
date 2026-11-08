//! 用官方的 `FallingBlockEntity.tick` 和 `PrimedTnt.tick` 对照 Bamboo 的实体物理。
//!
//! `EntityOracle.java` 在假世界里跑官方实体，每 tick 记下位置、速度、是否落地、方块状态和世界里的方块。
//! 这里用同一个种子重放，浮点按原始位比较。
//!
//! Compares Bamboo's entity physics with official `FallingBlockEntity.tick` and `PrimedTnt.tick`.
//! `EntityOracle.java` runs the official entities in a fake world and records position, velocity,
//! grounding, block state and the world blocks every tick. This replays the same seeds and
//! compares the floats by their raw bits.

use std::{
    collections::{BTreeSet, HashMap},
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, Level, LevelRandom, World,
    entity::{Entities, Entity, EntityKind, JavaRandom},
};
use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, sign::SignContent};
use mc_bamboo_data::version;
use serde::Deserialize;

use crate::oracle::collect_log;

const MIN_Y: i32 = -64;
const MAX_Y: i32 = 319;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Line {
    Header(Header),
    Case(Case),
    End(End),
}

#[derive(Debug, Deserialize)]
struct Header {
    version: String,
    protocol: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    kind: String,
    state: i32,
    x: i32,
    y: i32,
    z: i32,
    level_seed: i64,
    entity_seed: i64,
    hurts: Option<[f32; 2]>,
    fuse: Option<i32>,
    blocks: Vec<[i32; 4]>,
    steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
struct Step {
    entities: Vec<OfficialEntity>,
    blocks: Vec<[i32; 4]>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct OfficialEntity {
    kind: String,
    removed: bool,
    on_ground: bool,
    x: i64,
    y: i64,
    z: i64,
    dx: i64,
    dy: i64,
    dz: i64,
    block: i32,
    fuse: i32,
}

#[derive(Debug, Deserialize)]
struct End {
    cases: u32,
}

struct Report {
    cases: u32,
    steps: usize,
    mismatches: Vec<String>,
    ended: bool,
}

struct PhysicsLevel {
    blocks: HashMap<BlockPos, BlockState>,
    random: LevelRandom,
    redstone: mc_bamboo_blocks::RedstoneMemory,
    block_entities: BlockEntities,
    block_events: BlockEvents,
    entities: Entities,
}

impl PhysicsLevel {
    fn new(seed: i64) -> Self {
        Self {
            blocks: HashMap::new(),
            random: LevelRandom::new(seed),
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::new(),
        }
    }

    fn place(&mut self, blocks: &[[i32; 4]]) {
        for &[x, y, z, state] in blocks {
            self.blocks
                .insert(BlockPos::new(x, y, z), BlockState(state as u32));
        }
    }
}

impl Level for PhysicsLevel {
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

    fn block_entities(&self) -> &BlockEntities {
        &self.block_entities
    }

    fn block_entities_mut(&mut self) -> &mut BlockEntities {
        &mut self.block_entities
    }

    fn block_events(&mut self) -> &mut BlockEvents {
        &mut self.block_events
    }

    fn redstone(&self) -> &mc_bamboo_blocks::RedstoneMemory {
        &self.redstone
    }

    fn redstone_mut(&mut self) -> &mut mc_bamboo_blocks::RedstoneMemory {
        &mut self.redstone
    }

    fn entities(&self) -> &Entities {
        &self.entities
    }

    fn entities_mut(&mut self) -> &mut Entities {
        &mut self.entities
    }
}

/// 启动官方进程，逐题重放。
pub fn run(mut command: Command) -> Result<(), String> {
    // 官方编译器在中文 Windows 上用系统代码页写错误。固定成 UTF-8，失败日志才读得出来。
    // The official compiler writes errors in the system code page on Chinese Windows.
    // Pin UTF-8 so a failure log can be read.
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
        return Err(format!("EntityOracle.java failed: {status}\n{log}"));
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
            let parsed: Line = serde_json::from_str(&line)
                .map_err(|error| format!("bad entity oracle line: {error}: {line}"))?;
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

    fn check_header(&self, header: &Header) -> Result<(), String> {
        if header.version != version::GAME_VERSION || header.protocol != version::PROTOCOL_VERSION {
            return Err(format!(
                "the generated data is for {} (protocol {}), the official jar is {} (protocol {})",
                version::GAME_VERSION,
                version::PROTOCOL_VERSION,
                header.version,
                header.protocol,
            ));
        }
        Ok(())
    }

    fn replay(&mut self, case: &Case) {
        self.cases += 1;
        let mut level = PhysicsLevel::new(case.level_seed);
        level.place(&case.blocks);
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        let id = match case.kind.as_str() {
            "falling" => world.spawn_falling_block(
                BlockPos::new(case.x, case.y, case.z),
                BlockState(case.state as u32),
            ),
            "tnt" => world.spawn_tnt(BlockPos::new(case.x, case.y, case.z)),
            other => {
                self.mismatch(format!("{}: unknown kind {other}", case.name));
                return;
            }
        };
        if let Some(entity) = world.entity_mut(id) {
            if let Some([per_distance, max]) = case.hurts {
                entity.set_hurts_entities(per_distance, max as i32);
            }
            if let Some(fuse) = case.fuse {
                entity.set_fuse(fuse);
            }
            entity.random = JavaRandom::new(case.entity_seed);
        }
        if case.steps.is_empty() {
            self.mismatch(format!("{}: no steps", case.name));
            return;
        }
        self.compare_step(&case.name, 0, &world, &[], &case.steps[0]);
        for (index, step) in case.steps.iter().enumerate().skip(1) {
            let ticked = world.tick_entities_kept();
            self.steps += 1;
            self.compare_step(&case.name, index, &world, &ticked, step);
            if self.mismatches.len() >= 12 {
                return;
            }
        }
    }

    fn compare_step(
        &mut self,
        name: &str,
        index: usize,
        world: &World,
        ticked: &[Entity],
        official: &Step,
    ) {
        let mut ours = ours_entities(world, ticked);
        let mut expected = official.entities.clone();
        ours.sort_by_key(entity_key);
        expected.sort_by_key(entity_key);
        if ours.len() != expected.len() {
            self.mismatch(format!(
                "{name} step {index}: entity count: official {}, bamboo {}",
                expected.len(),
                ours.len()
            ));
        }
        for (ours, expected) in ours.iter().zip(expected.iter()) {
            self.compare_entity(name, index, ours, expected);
        }
        self.compare_blocks(name, index, world, &official.blocks);
    }

    fn compare_entity(
        &mut self,
        name: &str,
        index: usize,
        ours: &OfficialEntity,
        official: &OfficialEntity,
    ) {
        let place = format!("{name} step {index} {}", official.kind);
        if ours.kind != official.kind {
            self.mismatch(format!(
                "{place}: kind: official {}, bamboo {}",
                official.kind, ours.kind
            ));
        }
        if ours.removed != official.removed {
            self.mismatch(format!(
                "{place}: removed: official {}, bamboo {}",
                official.removed, ours.removed
            ));
        }
        if ours.on_ground != official.on_ground {
            self.mismatch(format!(
                "{place}: onGround: official {}, bamboo {}",
                official.on_ground, ours.on_ground
            ));
        }
        self.bits(&place, "x", official.x, ours.x);
        self.bits(&place, "y", official.y, ours.y);
        self.bits(&place, "z", official.z, ours.z);
        self.bits(&place, "dx", official.dx, ours.dx);
        self.bits(&place, "dy", official.dy, ours.dy);
        self.bits(&place, "dz", official.dz, ours.dz);
        if ours.block != official.block {
            self.mismatch(format!(
                "{place}: block: official {}, bamboo {}",
                official.block, ours.block
            ));
        }
        if ours.fuse != official.fuse {
            self.mismatch(format!(
                "{place}: fuse: official {}, bamboo {}",
                official.fuse, ours.fuse
            ));
        }
    }

    fn bits(&mut self, place: &str, field: &str, official: i64, ours: i64) {
        if official != ours {
            self.mismatch(format!(
                "{place}: {field}: official {official:#x} ({}), bamboo {ours:#x} ({})",
                f64::from_bits(official as u64),
                f64::from_bits(ours as u64)
            ));
        }
    }

    fn compare_blocks(&mut self, name: &str, index: usize, world: &World, official: &[[i32; 4]]) {
        let expected: BTreeSet<[i32; 4]> = official.iter().copied().collect();
        for block in &expected {
            let state = world.block_state(BlockPos::new(block[0], block[1], block[2]));
            if state.0 as i32 != block[3] {
                self.mismatch(format!(
                    "{name} step {index}: block {},{},{}: official {}, bamboo {}",
                    block[0], block[1], block[2], block[3], state.0
                ));
            }
        }
        let (min, max) = scan_box(official);
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                for x in min[0]..=max[0] {
                    let state = world.block_state(BlockPos::new(x, y, z));
                    if state.is_air() {
                        continue;
                    }
                    if !expected
                        .iter()
                        .any(|block| block[0] == x && block[1] == y && block[2] == z)
                    {
                        self.mismatch(format!(
                            "{name} step {index}: extra block {x},{y},{z} state {}",
                            state.0
                        ));
                    }
                }
            }
        }
    }

    fn mismatch(&mut self, message: String) {
        if self.mismatches.len() < 12 {
            self.mismatches.push(message);
        }
    }

    fn finish(self) -> Result<(), String> {
        if !self.ended {
            return Err("entity oracle stopped before the end line".to_owned());
        }
        if !self.mismatches.is_empty() {
            return Err(format!(
                "entity oracle: {} mismatches\n{}",
                self.mismatches.len(),
                self.mismatches.join("\n")
            ));
        }
        println!(
            "ENTITY ORACLE: {} cases, {} steps, 0 mismatches",
            self.cases, self.steps
        );
        Ok(())
    }
}

fn entity_key(entity: &OfficialEntity) -> (String, i64, i64, i64, bool) {
    (
        entity.kind.clone(),
        entity.x,
        entity.y,
        entity.z,
        entity.removed,
    )
}

fn ours_entities(world: &World, ticked: &[Entity]) -> Vec<OfficialEntity> {
    // 空列表表示还没 tick，用世界上活着的实体。tick 之后的列表里有刚被丢掉的。
    // An empty list means nothing has ticked yet, so use the live entities.
    // After a tick the list also contains the ones just discarded.
    let listed = if ticked.is_empty() {
        world.live_entities()
    } else {
        ticked.to_vec()
    };
    listed.into_iter().map(snap_of).collect()
}

fn snap_of(entity: Entity) -> OfficialEntity {
    let (kind, block, fuse) = match entity.kind {
        EntityKind::FallingBlock(data) => ("falling".to_owned(), data.state.0 as i32, -1),
        EntityKind::Tnt(data) => ("tnt".to_owned(), -1, data.fuse),
    };
    OfficialEntity {
        kind,
        removed: entity.removed,
        on_ground: entity.on_ground,
        x: entity.position.x.to_bits() as i64,
        y: entity.position.y.to_bits() as i64,
        z: entity.position.z.to_bits() as i64,
        dx: entity.delta.x.to_bits() as i64,
        dy: entity.delta.y.to_bits() as i64,
        dz: entity.delta.z.to_bits() as i64,
        block,
        fuse,
    }
}

fn scan_box(blocks: &[[i32; 4]]) -> ([i32; 3], [i32; 3]) {
    let mut min = [i32::MAX, i32::MAX, i32::MAX];
    let mut max = [i32::MIN, i32::MIN, i32::MIN];
    if blocks.is_empty() {
        return ([-8, 48, -8], [8, 96, 8]);
    }
    for block in blocks {
        min[0] = min[0].min(block[0]);
        min[1] = min[1].min(block[1]);
        min[2] = min[2].min(block[2]);
        max[0] = max[0].max(block[0]);
        max[1] = max[1].max(block[1]);
        max[2] = max[2].max(block[2]);
    }
    min[0] -= 2;
    min[1] -= 2;
    min[2] -= 2;
    max[0] += 2;
    max[1] += 2;
    max[2] += 2;
    (min, max)
}
