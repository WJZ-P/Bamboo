//! 熔炉和酿造台的逐刻对照。
//!
//! `MachineOracle.java` 在假世界里 tick 官方方块实体，每一刻记下进度、燃料、物品、方块状态和
//! `Level.random` 的内部种子。取出产物时调用 `getRecipesToAwardAndPopExperience`。
//! 这里用同样的初始物品和数据槽重放，逐刻比较。
//!
//! Tick-by-tick check for furnaces and brewing stands.
//!
//! `MachineOracle.java` ticks the vanilla block entities in a fake world and records progress,
//! fuel, items, the block state, and the internal seed of `Level.random` after every tick.
//! Taking the product calls `getRecipesToAwardAndPopExperience`. This replays the same items
//! and data slots and compares every tick.

use std::{
    io::Read,
    process::{Command, Stdio},
};

use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, Level, LevelRandom, RedstoneMemory, World,
    behaviour::machines::{BrewingStandEntity, FurnaceEntity},
    entity::Entities,
};
use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, sign::SignContent};
use mc_bamboo_data::{CookingKind, blocks};
use mc_bamboo_protocol::{
    components::{ComponentPatch, cursor::Cursor},
    item::ItemStack,
};
use serde::Deserialize;

use crate::oracle::collect_log;

const POS: BlockPos = BlockPos::new(0, 0, 0);

#[derive(Debug, Deserialize)]
struct Document {
    runs: Vec<Run>,
}

#[derive(Debug, Deserialize)]
struct Run {
    kind: String,
    seed: i64,
    block: u32,
    slots: Vec<(i32, i32, String)>,
    data: Vec<i32>,
    ticks: Vec<Frame>,
    #[serde(rename = "beforeTake")]
    before_take: Option<i64>,
    #[serde(rename = "afterTake")]
    after_take: Option<i64>,
    #[serde(rename = "afterItems")]
    after_items: Option<Vec<(i32, i32, String)>>,
}

#[derive(Debug, Deserialize)]
struct Frame {
    data: Vec<i32>,
    block: u32,
    items: Vec<(i32, i32, String)>,
    seed: i64,
}

/// 假世界只放一台机器。随机源和官方用同一个种子。
///
/// The fake world holds one machine. Its random source uses the same seed as vanilla.
struct MachineLevel {
    blocks: std::collections::HashMap<BlockPos, BlockState>,
    random: LevelRandom,
    block_entities: BlockEntities,
    block_events: BlockEvents,
    redstone: RedstoneMemory,
    /// 熔炉不生成实体。比较器在 tick 里可能问到这张表，空着就行。
    ///
    /// Furnaces spawn no entities. A tick may still ask for the table; it stays empty.
    entities: Entities,
}

impl MachineLevel {
    fn new(seed: i64, state: BlockState) -> Self {
        let mut blocks = std::collections::HashMap::new();
        blocks.insert(POS, state);
        Self {
            blocks,
            random: LevelRandom::new(seed),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            redstone: RedstoneMemory::default(),
            entities: Entities::default(),
        }
    }
}

impl Level for MachineLevel {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        self.blocks
            .get(&pos)
            .copied()
            .unwrap_or(BlockState(blocks::AIR))
    }

    fn write_block(&mut self, pos: BlockPos, state: BlockState, _flags: u32) -> bool {
        self.blocks.insert(pos, state);
        true
    }

    fn is_unobstructed(&self, _boxes: &[mc_bamboo_blocks::Aabb]) -> bool {
        true
    }

    fn min_y(&self) -> i32 {
        -64
    }

    fn max_y(&self) -> i32 {
        319
    }

    fn random(&self) -> &LevelRandom {
        &self.random
    }

    fn is_randomly_ticking_section(&self, _chunk: ChunkPos, _section_y: i32) -> bool {
        false
    }

    fn any_player_close_enough_to(&self, _pos: BlockPos, _max_distance: i32) -> bool {
        false
    }

    fn brightness(&self, _layer: LightLayer, _pos: BlockPos) -> i32 {
        15
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

    fn redstone(&self) -> &RedstoneMemory {
        &self.redstone
    }

    fn redstone_mut(&mut self) -> &mut RedstoneMemory {
        &mut self.redstone
    }

    fn entities(&self) -> &Entities {
        &self.entities
    }

    fn entities_mut(&mut self) -> &mut Entities {
        &mut self.entities
    }
}

enum Machine {
    Furnace(FurnaceEntity),
    Brewing(BrewingStandEntity),
}

impl Machine {
    fn open(kind: &str, state: BlockState) -> Self {
        if kind == "brewing" {
            return Self::Brewing(BrewingStandEntity::new(POS));
        }
        let cooking = match kind {
            "blast_furnace" => CookingKind::Blasting,
            "smoker" => CookingKind::Smoking,
            _ => FurnaceEntity::kind_of(state.class()).unwrap_or(CookingKind::Smelting),
        };
        Self::Furnace(FurnaceEntity::new(POS, cooking))
    }

    fn set_item(&mut self, slot: usize, stack: ItemStack) {
        match self {
            Self::Furnace(entity) => entity.set_item(slot, stack),
            Self::Brewing(entity) => entity.set_item(slot, stack),
        }
    }

    fn set_data(&mut self, id: usize, value: i32) {
        match self {
            Self::Furnace(entity) => entity.set_data(id, value),
            Self::Brewing(entity) => entity.set_data(id, value),
        }
    }

    fn tick(&mut self, world: &mut World) {
        match self {
            Self::Furnace(entity) => entity.server_tick(world),
            Self::Brewing(entity) => entity.server_tick(world),
        }
    }

    fn items(&self) -> Vec<ItemStack> {
        match self {
            Self::Furnace(entity) => entity.items().to_vec(),
            Self::Brewing(entity) => entity.items().to_vec(),
        }
    }

    fn data(&self) -> Vec<i32> {
        match self {
            Self::Furnace(entity) => entity.data_values().to_vec(),
            Self::Brewing(entity) => entity.data_values().to_vec(),
        }
    }

    fn award(&mut self, world: &mut World) {
        if let Self::Furnace(entity) = self {
            entity.award_experience(world);
        }
    }
}

pub fn run(mut command: Command) -> Result<(), String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .ok_or("java has no stdout")?
        .read_to_string(&mut stdout)
        .map_err(|error| format!("cannot read from java: {error}"))?;
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    if !status.success() {
        return Err(format!("MachineOracle.java failed: {status}\n{log}"));
    }
    let json = stdout.trim();
    let json = json
        .find('{')
        .map(|start| &json[start..])
        .ok_or_else(|| format!("machine oracle wrote no JSON\n{log}"))?;
    let document: Document = serde_json::from_str(json)
        .map_err(|error| format!("bad machine oracle output: {error}\n{log}"))?;
    compare(&document)
}

fn compare(document: &Document) -> Result<(), String> {
    let mut ticks = 0usize;
    let mut mismatches = Vec::new();
    for run in &document.runs {
        replay(run, &mut ticks, &mut mismatches)?;
    }
    println!("machine: {} runs, {ticks} ticks", document.runs.len());
    if mismatches.is_empty() {
        println!("MACHINE RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in mismatches.iter().take(12) {
            println!("MACHINE FAIL: {line}");
        }
        Err(format!("MACHINE RESULT: {} mismatches", mismatches.len()))
    }
}

fn replay(run: &Run, ticks: &mut usize, mismatches: &mut Vec<String>) -> Result<(), String> {
    let state = BlockState(run.block);
    let mut level = MachineLevel::new(run.seed, state);
    let mut machine = Machine::open(&run.kind, state);
    for (slot, triple) in run.slots.iter().enumerate() {
        machine.set_item(slot, stack_from(triple)?);
    }
    for (id, value) in run.data.iter().copied().enumerate() {
        machine.set_data(id, value);
    }
    let mut scheduled = BlockTicks::new();
    let mut world = World::new(&mut level, &mut scheduled);
    for (index, frame) in run.ticks.iter().enumerate() {
        machine.tick(&mut world);
        *ticks += 1;
        let label = format!("{} tick {index}", run.kind);
        if machine.data() != frame.data {
            mismatches.push(format!(
                "{label} data {:?} != {:?}",
                machine.data(),
                frame.data
            ));
        }
        if world.block_state(POS).0 != frame.block {
            mismatches.push(format!(
                "{label} block {} != {}",
                world.block_state(POS).0,
                frame.block
            ));
        }
        if world.random().seed() != frame.seed {
            mismatches.push(format!(
                "{label} seed {} != {}",
                world.random().seed(),
                frame.seed
            ));
        }
        push_items(&label, &machine.items(), &frame.items, mismatches)?;
        if mismatches.len() > 40 {
            return Ok(());
        }
    }
    if let Some(before) = run.before_take {
        let label = format!("{} before take", run.kind);
        if world.random().seed() != before {
            mismatches.push(format!(
                "{label} seed {} != {before}",
                world.random().seed()
            ));
        }
        machine.award(&mut world);
        if let Some(after) = run.after_take
            && world.random().seed() != after
        {
            mismatches.push(format!(
                "{} after take seed {} != {after}",
                run.kind,
                world.random().seed()
            ));
        }
        if let Some(items) = &run.after_items {
            push_items(
                &format!("{} after take", run.kind),
                &machine.items(),
                items,
                mismatches,
            )?;
        }
    }
    Ok(())
}

fn push_items(
    label: &str,
    ours: &[ItemStack],
    official: &[(i32, i32, String)],
    mismatches: &mut Vec<String>,
) -> Result<(), String> {
    if ours.len() != official.len() {
        mismatches.push(format!(
            "{label} slot count {} != {}",
            ours.len(),
            official.len()
        ));
        return Ok(());
    }
    for (slot, (stack, expected)) in ours.iter().zip(official).enumerate() {
        let got = triple(stack);
        if got != *expected {
            mismatches.push(format!("{label} slot {slot} {got:?} != {expected:?}"));
        }
    }
    Ok(())
}

fn triple(stack: &ItemStack) -> (i32, i32, String) {
    if stack.is_empty() {
        return (0, 0, String::new());
    }
    let mut bytes = Vec::new();
    stack.patch().write(&mut bytes);
    (stack.item, stack.count, hex(&bytes))
}

fn stack_from(triple: &(i32, i32, String)) -> Result<ItemStack, String> {
    let (item, count, patch) = triple;
    if *item == 0 || *count <= 0 {
        return Ok(ItemStack::empty());
    }
    let mut stack = ItemStack::new(*item, *count);
    if patch.is_empty() {
        return Ok(stack);
    }
    let bytes = decode_hex(patch)?;
    let mut cursor = Cursor::new(&bytes);
    let parsed = ComponentPatch::read(&mut cursor).map_err(|error| format!("{error:?}"))?;
    cursor.finish().map_err(|error| format!("{error:?}"))?;
    let (added, removed) = parsed.to_opaque();
    stack.added = added;
    stack.removed = removed;
    Ok(stack)
}

fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err(format!("odd hex length {text}"));
    }
    (0..text.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&text[index..index + 2], 16)
                .map_err(|error| format!("bad hex: {error}"))
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xF) as usize] as char);
    }
    out
}
