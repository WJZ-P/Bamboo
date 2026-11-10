//! 用官方实体 tick 对照 Bamboo 的下落方块、点燃的 TNT 和地上的物品。
//!
//! `EntityOracle.java` 在假世界里跑官方实体，每 tick 记下位置、速度、是否落地、方块状态和世界里的方块。
//! 物品还记下堆叠、年龄、拾取延迟和生命。这里用同一个种子重放，浮点按原始位比较。
//!
//! Compares Bamboo with official falling-block, primed-TNT and ground-item ticks.
//! `EntityOracle.java` records position, velocity, grounding, block state and the world blocks
//! every tick. Items also record the stack, age, pickup delay and health. This replays the same
//! seeds and compares the floats by their raw bits.

use std::{
    collections::{BTreeSet, HashMap},
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_blocks::{
    Aabb, BlockEntities, BlockEvents, BlockState, BlockTicks, Container, EntityStore, EntityTables,
    HopperEntity, Level, LevelRandom, LevelTables, Slots, World,
    entity::{Entities, Entity, EntityKind, JavaRandom, can_pick_up, take_stack},
};
use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, Vec3, sign::SignContent};
use mc_bamboo_data::{BlockAncestor, version};
use mc_bamboo_game::receive_like_player;
use mc_bamboo_loot::{BlockDropParams, LevelView, LootStack, RandomSequences, block_drops};
use mc_bamboo_protocol::item::ItemStack;
use serde::Deserialize;

use crate::oracle::collect_log;

const MIN_Y: i32 = -64;
const MAX_Y: i32 = 319;

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
    /// `entity_drops` 规则。旧的题不写这一项，按官方默认的真。
    ///
    /// The `entity_drops` rule. Older cases omit it and use the vanilla default of true.
    #[serde(default = "enabled_rule")]
    entity_drops: bool,
    /// 新题才写这一项：要不要把下落方块掉出来的物品也纳入比较。
    ///
    /// Only newer cases set this: whether items dropped by falling blocks are compared.
    #[serde(default)]
    compare_drops: bool,
    /// `FallingBlockEntity.time` 的初值，只有跑 600 刻超时的题才写。
    ///
    /// Initial `FallingBlockEntity.time`; only the timeout cases set it.
    #[serde(default)]
    start_time: i32,
    /// 官方掉出物品时用的第一个实体 ID。旧题不写这一项，用默认的非玩家起点。
    ///
    /// First entity id vanilla used for the dropped item. Older cases use the default.
    #[serde(default = "first_entity_id")]
    first_entity_id: i32,
    blocks: Vec<[i32; 4]>,
    steps: Vec<Step>,
    /// 物品场景才有。下落方块和 TNT 的题不写这些字段。
    ///
    /// Present on item scenes. Falling-block and TNT cases omit them.
    #[serde(default)]
    item: i32,
    #[serde(default)]
    count: i32,
    #[serde(default)]
    px: f64,
    #[serde(default)]
    py: f64,
    #[serde(default)]
    pz: f64,
    #[serde(default)]
    pickup_delay: Option<i32>,
    #[serde(default)]
    extras: Vec<ExtraItem>,
    #[serde(default)]
    hopper: bool,
    /// 玩家捡起才有。脚的位置固定在 (0.5, 64, 0.5)。
    ///
    /// Present when a player picks the item up. The feet stay at (0.5, 64, 0.5).
    #[serde(default)]
    player: Option<PlayerSetup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayerSetup {
    creative: bool,
    spectator: bool,
    /// 小于 0 时 36 格都装满 `fill_item`。大于 0 时只填快捷栏第 0 格。
    ///
    /// Below zero fills all 36 slots with `fill_item`. Above zero fills hotbar slot 0 only.
    prefill: i32,
    #[serde(default)]
    fill_item: i32,
    #[serde(default)]
    fill_count: i32,
}

#[derive(Debug, Deserialize)]
struct ExtraItem {
    item: i32,
    count: i32,
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, Deserialize)]
struct Step {
    entities: Vec<OfficialEntity>,
    blocks: Vec<[i32; 4]>,
    /// 漏斗槽里非空的物品。没有漏斗时是空表。
    ///
    /// Non-empty hopper slots. Empty when the scene has no hopper.
    #[serde(default)]
    containers: Vec<[i32; 5]>,
    /// 物品栏里非空的格子，`[物品栏下标, 物品, 数量]`。
    ///
    /// Non-empty inventory slots: `[inventory index, item, count]`.
    #[serde(default)]
    inventory: Vec<[i32; 3]>,
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
    /// 物品 id。不是物品时是 -1。
    ///
    /// Item id. `-1` when this entity is not an item.
    #[serde(default = "unknown_item")]
    item: i32,
    #[serde(default)]
    count: i32,
    #[serde(default = "unknown_item")]
    age: i32,
    #[serde(default = "unknown_item")]
    pickup: i32,
    #[serde(default = "unknown_item")]
    health: i32,
}

fn unknown_item() -> i32 {
    -1
}

fn enabled_rule() -> bool {
    true
}

fn first_entity_id() -> i32 {
    1 << 24
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
    rules: mc_bamboo_blocks::BlockGameRules,
    redstone: mc_bamboo_blocks::RedstoneMemory,
    block_entities: BlockEntities,
    block_events: BlockEvents,
    entities: Entities,
    containers: HashMap<BlockPos, Vec<ItemStack>>,
}

impl PhysicsLevel {
    fn new(seed: i64) -> Self {
        Self {
            blocks: HashMap::new(),
            random: LevelRandom::new(seed),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::new(),
            containers: HashMap::new(),
        }
    }

    fn place(&mut self, blocks: &[[i32; 4]]) {
        for &[x, y, z, state] in blocks {
            self.blocks
                .insert(BlockPos::new(x, y, z), BlockState(state as u32));
        }
    }
}

impl LevelTables for PhysicsLevel {
    fn block_entities_table(&self) -> &BlockEntities {
        &self.block_entities
    }

    fn block_entities_table_mut(&mut self) -> &mut BlockEntities {
        &mut self.block_entities
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

impl EntityTables for PhysicsLevel {
    fn entities_table(&self) -> &EntityStore {
        &self.entities
    }

    fn entities_table_mut(&mut self) -> &mut EntityStore {
        &mut self.entities
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

    fn with_container(&mut self, pos: BlockPos, f: &mut dyn FnMut(&mut dyn Container)) -> bool {
        if !self.block_state(pos).is_a(BlockAncestor::HopperBlock) {
            return false;
        }
        let slots = self
            .containers
            .entry(pos)
            .or_insert_with(|| vec![ItemStack::empty(); 5]);
        let mut view = Slots::new(slots);
        f(&mut view);
        true
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
        level.rules.entity_drops = case.entity_drops;
        level.entities = Entities::with_next_id(case.first_entity_id);
        level.place(&case.blocks);
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        let id = match case.kind.as_str() {
            "falling" => world.spawn_falling_block(
                BlockPos::new(case.x, case.y, case.z),
                BlockState(case.state as u32),
            ),
            "tnt" => world.spawn_tnt(BlockPos::new(case.x, case.y, case.z)),
            "item" => {
                spawn_ground_item(
                    &mut world,
                    case.px,
                    case.py,
                    case.pz,
                    case.item,
                    case.count,
                    case.pickup_delay.unwrap_or(10),
                );
                for extra in &case.extras {
                    spawn_ground_item(
                        &mut world,
                        extra.x,
                        extra.y,
                        extra.z,
                        extra.item,
                        extra.count,
                        case.pickup_delay.unwrap_or(10),
                    );
                }
                for &[x, y, z, state] in &case.blocks {
                    let pos = BlockPos::new(x, y, z);
                    if BlockState(state as u32).is_a(BlockAncestor::HopperBlock) {
                        world.set_block_entity(Box::new(HopperEntity::new(pos)));
                    }
                }
                0
            }
            other => {
                self.mismatch(format!("{}: unknown kind {other}", case.name));
                return;
            }
        };
        if case.kind != "item" {
            world.with_entity_mut(id, &mut |entity| {
                if let Some([per_distance, max]) = case.hurts {
                    entity.set_hurts_entities(per_distance, max as i32);
                }
                if let Some(fuse) = case.fuse {
                    entity.set_fuse(fuse);
                }
                entity.random = JavaRandom::new(case.entity_seed);
                if case.start_time != 0
                    && let EntityKind::FallingBlock(data) = &mut entity.kind
                {
                    data.time = case.start_time;
                }
            });
        }
        if case.steps.is_empty() {
            self.mismatch(format!("{}: no steps", case.name));
            return;
        }
        let hoppers = hopper_cells(case);
        let mut slots = case.player.as_ref().map(player_slots);
        self.compare_step(&case.name, 0, &mut world, None, &case.steps[0], &hoppers);
        if let Some(slots) = slots.as_deref() {
            self.compare_inventory(&case.name, 0, slots, &case.steps[0].inventory);
        }
        for (index, step) in case.steps.iter().enumerate().skip(1) {
            if let Some(player) = &case.player
                && !player.spectator
                && let Some(slots) = slots.as_mut()
            {
                touch_player(&mut world, slots, player.creative);
            }
            let mut ticked = world.tick_entities_kept();
            // 旧的下落方块和 TNT 题只比那两种实体；新的掉落物题带 `compareDrops`。
            // Older falling-block and TNT cases compared only those two kinds; the new
            // drop cases opt into comparing the spawned item entities.
            if case.kind != "item" && !case.compare_drops {
                ticked.retain(|entity| !matches!(entity.kind, EntityKind::Item(_)));
            }
            if case.hopper {
                // 官方先 tick 实体，再 tick 漏斗。漏斗吸走的物品这一拍也要留在快照里。
                // Vanilla ticks entities, then hoppers. An item pulled this tick stays in the snapshot.
                world.tick_block_entities();
                refresh_after_hopper(&mut world, &mut ticked);
            }
            self.steps += 1;
            self.compare_step(&case.name, index, &mut world, Some(&ticked), step, &hoppers);
            if let Some(slots) = slots.as_deref() {
                self.compare_inventory(&case.name, index, slots, &step.inventory);
            }
            if self.mismatches.len() >= 12 {
                return;
            }
        }
    }

    fn compare_step(
        &mut self,
        name: &str,
        index: usize,
        world: &mut World,
        ticked: Option<&[Entity]>,
        official: &Step,
        hoppers: &[BlockPos],
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
        self.compare_containers(name, index, world, hoppers, &official.containers);
    }

    fn compare_inventory(
        &mut self,
        name: &str,
        index: usize,
        slots: &[ItemStack],
        official: &[[i32; 3]],
    ) {
        let mut ours = inventory_slots(slots);
        let mut expected = official.to_vec();
        ours.sort();
        expected.sort();
        if ours != expected {
            self.mismatch(format!(
                "{name} step {index}: inventory: official {expected:?}, bamboo {ours:?}"
            ));
        }
    }

    fn compare_containers(
        &mut self,
        name: &str,
        index: usize,
        world: &mut World,
        hoppers: &[BlockPos],
        official: &[[i32; 5]],
    ) {
        let mut ours = Vec::new();
        for pos in hoppers {
            for (item, count) in world.container_contents(*pos) {
                ours.push([pos.x, pos.y, pos.z, item, count]);
            }
        }
        let mut expected = official.to_vec();
        ours.sort();
        expected.sort();
        if ours != expected {
            self.mismatch(format!(
                "{name} step {index}: containers: official {expected:?}, bamboo {ours:?}"
            ));
        }
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
        if ours.item != official.item {
            self.mismatch(format!(
                "{place}: item: official {}, bamboo {}",
                official.item, ours.item
            ));
        }
        if ours.count != official.count {
            self.mismatch(format!(
                "{place}: count: official {}, bamboo {}",
                official.count, ours.count
            ));
        }
        if ours.age != official.age {
            self.mismatch(format!(
                "{place}: age: official {}, bamboo {}",
                official.age, ours.age
            ));
        }
        if ours.pickup != official.pickup {
            self.mismatch(format!(
                "{place}: pickup: official {}, bamboo {}",
                official.pickup, ours.pickup
            ));
        }
        if ours.health != official.health {
            self.mismatch(format!(
                "{place}: health: official {}, bamboo {}",
                official.health, ours.health
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

fn ours_entities(world: &World, ticked: Option<&[Entity]>) -> Vec<OfficialEntity> {
    // `None` 表示还没 tick，用世界上活着的实体。tick 之后的列表里有刚被丢掉的，空列表就是真的没有。
    // `None` means nothing has ticked yet, so use the live entities.
    // After a tick the list also contains the ones just discarded. An empty list means none.
    let listed = match ticked {
        None => world.live_entities(),
        Some(ticked) => ticked.to_vec(),
    };
    listed.into_iter().map(snap_of).collect()
}

fn snap_of(entity: Entity) -> OfficialEntity {
    let (kind, block, fuse, item, count, age, pickup, health) = match entity.kind {
        EntityKind::FallingBlock(data) => (
            "falling".to_owned(),
            data.state.0 as i32,
            -1,
            -1,
            0,
            -1,
            -1,
            -1,
        ),
        EntityKind::Tnt(data) => ("tnt".to_owned(), -1, data.fuse, -1, 0, -1, -1, -1),
        EntityKind::Item(data) => (
            "item".to_owned(),
            -1,
            -1,
            data.stack.item,
            data.stack.count,
            data.age,
            data.pickup_delay,
            data.health,
        ),
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
        item,
        count,
        age,
        pickup,
        health,
    }
}

/// `spawnAtLocation`：实体随机源在分配 ID 之后取朝向和初速度，拾取延迟 10。
///
/// `spawnAtLocation`: facing and velocity are drawn after the id is assigned. Pickup delay is 10.
fn spawn_ground_item(world: &mut World, x: f64, y: f64, z: f64, item: i32, count: i32, delay: i32) {
    let Some(id) = world.spawn_at_location(Vec3::new(x, y, z), ItemStack::new(item, count)) else {
        return;
    };
    if delay != 10 {
        world.with_entity_mut(id, &mut |entity| {
            if let Some(ground) = entity.ground_mut() {
                ground.set_pickup_delay(delay);
            }
        });
    }
}

/// 玩家脚在 (0.5, 64, 0.5)，身体宽 0.6、高 1.8，再按 `Player.aiStep` 膨胀 (1, 0.5, 1)。
///
/// Feet at (0.5, 64, 0.5), body 0.6 by 1.8, then the `Player.aiStep` inflate of (1, 0.5, 1).
fn player_reach() -> Aabb {
    Aabb::new(0.2, 64.0, 0.2, 0.8, 65.8, 0.8).inflate(1.0, 0.5, 1.0)
}

fn player_slots(player: &PlayerSetup) -> Vec<ItemStack> {
    let mut slots = vec![ItemStack::empty(); 46];
    if player.prefill < 0 {
        let fill = ItemStack::new(player.fill_item, player.fill_count);
        for index in 0..36 {
            slots[menu_index(index)] = fill.clone();
        }
    } else if player.prefill > 0 {
        slots[menu_index(0)] = ItemStack::new(player.fill_item, player.fill_count);
    }
    slots
}

/// 官方 `Inventory.items` 的下标换成菜单槽位。
///
/// An `Inventory.items` index as a menu slot.
fn menu_index(index: usize) -> usize {
    if index < 9 { 36 + index } else { index }
}

fn inventory_slots(slots: &[ItemStack]) -> Vec<[i32; 3]> {
    let mut listed = Vec::new();
    for index in 0..36 {
        let stack = &slots[menu_index(index)];
        if !stack.is_empty() {
            listed.push([index as i32, stack.item, stack.count]);
        }
    }
    let off_hand = &slots[45];
    if !off_hand.is_empty() {
        listed.push([40, off_hand.item, off_hand.count]);
    }
    listed
}

/// `ItemEntity.playerTouch`，发生在这一拍的实体 tick 之前。
///
/// `ItemEntity.playerTouch`, before this tick's entity ticks.
fn touch_player(world: &mut World, slots: &mut [ItemStack], creative: bool) {
    let reach = player_reach();
    let owner = [0_u8; 16];
    for id in world.entity_ids() {
        let Some(entity) = world.entity(id) else {
            continue;
        };
        if !entity.bounding_box().intersects(reach) || !can_pick_up(&entity, &owner) {
            continue;
        }
        let stack = entity
            .ground()
            .map(|item| item.stack.clone())
            .unwrap_or_else(ItemStack::empty);
        let original = stack.count;
        let left = receive_like_player(slots, 0, stack, creative);
        let picked = left.count < original || (creative && left.is_empty() && original > 0);
        if !picked {
            continue;
        }
        world.with_entity_mut(id, &mut |entity| {
            take_stack(entity, left.clone());
        });
    }
}

/// 生存挖掘：工具对了才 `getDrops` 再 `popResource`。返回掉落和之后的 `level.random` 种子。
///
/// Survival mining: `getDrops` then `popResource` only with the right tool. Returns the drops and
/// the `level.random` seed afterwards.
pub(crate) fn mined_outcome(
    seed: i64,
    state: u32,
    tool: i32,
) -> Result<(Vec<(i32, i32)>, i64), String> {
    let pos = BlockPos::new(0, 64, 0);
    let mut level = PhysicsLevel::new(seed);
    level.blocks.insert(pos, BlockState(state));
    let stacks = if mc_bamboo_blocks::mining::is_correct_for_drops(state, tool) {
        let view = OneBlock {
            state: BlockState(state),
            pos,
        };
        let tool_stack = if tool == 0 {
            None
        } else {
            Some(LootStack::new(tool, 1))
        };
        let params = BlockDropParams {
            state: BlockState(state),
            pos: (pos.x, pos.y, pos.z),
            level: &view,
            block_entity: None,
            breaker: None,
            tool: tool_stack,
            explosion_radius: None,
            luck: 0.0,
        };
        let sequences = RandomSequences::new(seed);
        block_drops(&params, Some(&sequences), level.random.source())
            .map_err(|error| error.to_string())?
    } else {
        Vec::new()
    };
    let drops = {
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        for stack in stacks {
            let item = if stack.is_empty() {
                ItemStack::empty()
            } else {
                ItemStack::new(stack.item, stack.count)
            };
            world.pop_resource(pos, item);
        }
        let mut drops = Vec::new();
        for id in world.entity_ids() {
            if let Some(entity) = world.entity(id)
                && let Some(ground) = entity.ground()
                && !ground.stack.is_empty()
            {
                drops.push((ground.stack.item, ground.stack.count));
            }
        }
        drops.sort();
        drops
    };
    Ok((drops, level.random.source().seed()))
}

/// 战利品条件只看这一格。
///
/// Loot conditions only see this one block.
struct OneBlock {
    state: BlockState,
    pos: BlockPos,
}

impl LevelView for OneBlock {
    fn is_loaded(&self, _: i32, _: i32, _: i32) -> Option<bool> {
        Some(true)
    }

    fn block_state(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        if x == self.pos.x && y == self.pos.y && z == self.pos.z {
            Some(self.state)
        } else {
            Some(BlockState::AIR)
        }
    }

    fn biome(&self, _: i32, _: i32, _: i32) -> Option<i32> {
        None
    }

    fn can_see_sky(&self, _: i32, _: i32, _: i32) -> Option<bool> {
        None
    }

    fn is_raining(&self) -> Option<bool> {
        None
    }

    fn is_thundering(&self) -> Option<bool> {
        None
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

fn hopper_cells(case: &Case) -> Vec<BlockPos> {
    case.blocks
        .iter()
        .filter(|block| BlockState(block[3] as u32).is_a(BlockAncestor::HopperBlock))
        .map(|block| BlockPos::new(block[0], block[1], block[2]))
        .collect()
}

/// 漏斗 tick 在实体 tick 之后。被吸走的物品从世界上拿掉，快照里记成丢掉、堆叠清空。
///
/// The hopper ticks after the entities. A pulled item leaves the world; the snapshot keeps it
/// removed, with an empty stack.
fn refresh_after_hopper(world: &mut World, ticked: &mut [Entity]) {
    for entity in ticked {
        if let Some(current) = world.entity(entity.id) {
            *entity = current;
        } else {
            entity.removed = true;
            if let EntityKind::Item(item) = &mut entity.kind {
                item.stack = ItemStack::empty();
            }
        }
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
