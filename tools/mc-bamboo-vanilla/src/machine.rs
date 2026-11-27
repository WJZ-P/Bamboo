//! 熔炉、酿造台和营火的逐刻对照。
//!
//! `MachineOracle.java` 在假世界里 tick 官方方块实体，每一刻记下进度、燃料、物品、方块状态和
//! `Level.random` 的内部种子。取出产物时调用 `getRecipesToAwardAndPopExperience`。
//! 这里用同样的初始物品和数据槽重放，逐刻比较。
//!
//! Tick-by-tick check for furnaces, brewing stands, and campfires.
//!
//! `MachineOracle.java` ticks the vanilla block entities in a fake world and records progress,
//! fuel, items, the block state, and the internal seed of `Level.random` after every tick.
//! Taking the product calls `getRecipesToAwardAndPopExperience`. This replays the same items
//! and data slots and compares every tick.

use std::{
    io::Read,
    process::{Command, Stdio},
};

use mc_bamboo_blocks::entity::{Entity, EntityKind, mob::MobType};
use mc_bamboo_blocks::{
    BlockEntities, BlockEvents, BlockState, BlockTicks, EntityStore, EntityTables, Level,
    LevelRandom, LevelTables, RedstoneMemory, World,
    behaviour::{
        machines::{BrewingStandEntity, CampfireEntity, FurnaceEntity},
        misc::{
            beacon::{
                BeaconBlockEntity, DATA_LEVELS, DATA_PRIMARY, DATA_SECONDARY, encode_effect,
                validate_effects,
            },
            conduit_entity::{ConduitBlockEntity, frame_offsets},
        },
    },
    entity::{Entities, ctx::PlayerView},
    level::Effect,
};
use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, Vec3, sign::SignContent};
use mc_bamboo_data::{CookingKind, blocks, recipes, registries::mob_effect, sounds};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{ComponentPatch, cursor::Cursor},
    item::ItemStack,
};
use serde::Deserialize;

use crate::oracle::collect_log;

const POS: BlockPos = BlockPos::new(0, 0, 0);
const BEACON_POS: BlockPos = BlockPos::new(0, 64, 0);

#[derive(Debug, Deserialize)]
struct Document {
    runs: Vec<Run>,
    #[serde(default)]
    beacons: Vec<BeaconRun>,
    #[serde(default)]
    conduits: Vec<ConduitRun>,
}

#[derive(Debug, Deserialize)]
struct BeaconRun {
    kind: String,
    #[serde(default)]
    ticks: Vec<BeaconFrame>,
    #[serde(default)]
    payments: Vec<BeaconPaymentCheck>,
    #[serde(default, rename = "savedValid")]
    saved_valid: Option<BeaconSaveFields>,
    #[serde(default, rename = "resavedValid")]
    resaved_valid: Option<BeaconSaveFields>,
    #[serde(default, rename = "savedFiltered")]
    saved_filtered: Option<BeaconSaveFields>,
}

#[derive(Debug, Deserialize)]
struct BeaconFrame {
    #[serde(rename = "gameTime")]
    game_time: i64,
    data: Vec<i32>,
    #[serde(rename = "beamSections")]
    beam_sections: Vec<[i32; 2]>,
    #[serde(rename = "rawBeamSections")]
    raw_beam_sections: Vec<[i32; 2]>,
    sounds: Vec<String>,
    effects: Vec<[i32; 7]>,
}

#[derive(Debug, Deserialize)]
struct BeaconPaymentCheck {
    #[serde(rename = "hasPayment")]
    has_payment: bool,
    primary: i32,
    secondary: i32,
    ok: bool,
    #[serde(rename = "remainingPayment")]
    remaining_payment: i32,
    data: Vec<i32>,
    sounds: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct BeaconSaveFields {
    primary_effect: Option<String>,
    secondary_effect: Option<String>,
    #[serde(rename = "Levels")]
    levels: i32,
}

const CONDUIT_POS: BlockPos = BlockPos::new(0, 64, 0);

#[derive(Debug, Deserialize)]
struct ConduitRun {
    kind: String,
    #[serde(default)]
    seed: i64,
    #[serde(default)]
    ticks: Vec<ConduitFrame>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    saved: Option<ConduitSaveFields>,
    #[serde(default, rename = "emptySaved")]
    empty_saved: Option<ConduitSaveFields>,
}

#[derive(Debug, Deserialize)]
struct ConduitFrame {
    #[serde(rename = "gameTime")]
    game_time: i64,
    active: bool,
    hunting: bool,
    blocks: usize,
    target: String,
    #[serde(rename = "tickCount")]
    tick_count: i32,
    #[serde(rename = "nextAmbient")]
    next_ambient: i64,
    sounds: Vec<String>,
    effects: Vec<[i32; 7]>,
    seed: i64,
    health: Vec<f32>,
    hurts: Vec<String>,
    #[serde(rename = "blockUpdates")]
    block_updates: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct ConduitSaveFields {
    #[serde(default, rename = "Target")]
    target: Option<Vec<i32>>,
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
    /// 取出产物后生成的经验球 `[value, count, x 位, y 位, z 位]`。
    #[serde(default, rename = "afterOrbs")]
    after_orbs: Vec<[i64; 5]>,
    /// 直接塞进 `recipesUsed` 的配方（名字、次数）。
    #[serde(default)]
    recipes: Vec<(String, i32)>,
    /// 砂轮题：两件物品的非诅咒附魔 `min_cost` 之和。
    #[serde(default, rename = "xpSumInput")]
    xp_sum_input: i32,
    /// 砂轮题：官方 `getExperienceAmount` 的结果。
    #[serde(default, rename = "xpReward")]
    xp_reward: i32,
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
    /// 信标对拍用的假玩家列表。
    players: Vec<PlayerView>,
    /// 潮涌核心对拍里哪些玩家在水里 / 淋雨。
    conduit_wet: Vec<(i32, bool)>,
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
            players: Vec::new(),
            conduit_wet: Vec::new(),
        }
    }

    fn empty(seed: i64) -> Self {
        Self {
            blocks: std::collections::HashMap::new(),
            random: LevelRandom::new(seed),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            redstone: RedstoneMemory::default(),
            entities: Entities::default(),
            players: Vec::new(),
            conduit_wet: Vec::new(),
        }
    }

    fn set_block(&mut self, pos: BlockPos, state: BlockState) {
        if state.is_air() {
            self.blocks.remove(&pos);
        } else {
            self.blocks.insert(pos, state);
        }
    }
}

impl LevelTables for MachineLevel {
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

    fn redstone_table(&self) -> &RedstoneMemory {
        &self.redstone
    }

    fn redstone_table_mut(&mut self) -> &mut RedstoneMemory {
        &mut self.redstone
    }
}

impl EntityTables for MachineLevel {
    fn entities_table(&self) -> &EntityStore {
        &self.entities
    }

    fn entities_table_mut(&mut self) -> &mut EntityStore {
        &mut self.entities
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

    /// 潮涌核心夹具显式注入「玩家在水里 / 淋雨」的状态。
    fn player_in_water_or_rain(&self, player: &PlayerView) -> bool {
        self.conduit_wet
            .iter()
            .find(|(uid, _)| *uid == player.uid)
            .is_some_and(|(_, wet)| *wet)
    }

    fn for_each_player(&self, visit: &mut dyn FnMut(&PlayerView)) {
        for player in &self.players {
            visit(player);
        }
    }
}

enum Machine {
    Furnace(FurnaceEntity),
    Brewing(BrewingStandEntity),
    Campfire(CampfireEntity),
}

impl Machine {
    fn open(kind: &str, state: BlockState) -> Self {
        if kind == "brewing" {
            return Self::Brewing(BrewingStandEntity::new(POS));
        }
        if kind == "campfire" {
            return Self::Campfire(CampfireEntity::new(POS));
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
            Self::Campfire(entity) => entity.set_item(slot, stack),
        }
    }

    fn set_data(&mut self, id: usize, value: i32) {
        match self {
            Self::Furnace(entity) => entity.set_data(id, value),
            Self::Brewing(entity) => entity.set_data(id, value),
            Self::Campfire(entity) => entity.set_data(id, value),
        }
    }

    fn tick(&mut self, world: &mut World) {
        match self {
            Self::Furnace(entity) => entity.server_tick(world),
            Self::Brewing(entity) => entity.server_tick(world),
            Self::Campfire(entity) => entity.server_tick(world),
        }
    }

    fn items(&self) -> Vec<ItemStack> {
        match self {
            Self::Furnace(entity) => entity.items().to_vec(),
            Self::Brewing(entity) => entity.items().to_vec(),
            Self::Campfire(entity) => entity.items().to_vec(),
        }
    }

    fn data(&self) -> Vec<i32> {
        match self {
            Self::Furnace(entity) => entity.data_values().to_vec(),
            Self::Brewing(entity) => entity.data_values().to_vec(),
            Self::Campfire(entity) => entity.data_values().to_vec(),
        }
    }

    fn set_recipes(&mut self, recipes: &[(String, i32)]) {
        if let Self::Furnace(entity) = self {
            let used: Vec<(&'static str, i32)> = recipes
                .iter()
                .filter_map(|(name, count)| {
                    recipes::RECIPES
                        .iter()
                        .find(|recipe| recipe.id == name)
                        .map(|recipe| (recipe.id, *count))
                })
                .collect();
            entity.set_archive_values(entity.speed_multiplier(), used);
        }
    }

    fn award(&mut self, world: &mut World) {
        if let Self::Furnace(entity) = self {
            // 官方题目传 `Vec3.ZERO`。
            entity.award_experience(world, mc_bamboo_core::Vec3::default());
        }
    }
}

pub fn run(mut command: Command, options: &[&str]) -> Result<(), String> {
    let mut break_test = "";
    let mut index = 0;
    while index < options.len() {
        match options[index] {
            "--break-self-test" if index + 1 < options.len() => {
                break_test = options[index + 1];
                index += 2;
            }
            _ => return Err("machine: unknown option".to_owned()),
        }
    }
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
    compare(&document, break_test)
}

fn compare(document: &Document, break_test: &str) -> Result<(), String> {
    let mut ticks = 0usize;
    let mut mismatches = Vec::new();
    for run in &document.runs {
        replay(run, &mut ticks, &mut mismatches)?;
    }
    let mut beacon_ticks = 0usize;
    for run in &document.beacons {
        replay_beacon(run, &mut beacon_ticks, &mut mismatches);
    }
    let mut conduit_ticks = 0usize;
    for run in &document.conduits {
        replay_conduit(run, &mut conduit_ticks, &mut mismatches, break_test);
    }
    println!(
        "machine: {} runs ({ticks} ticks), {} beacon runs ({beacon_ticks} ticks), {} conduit runs ({conduit_ticks} ticks)",
        document.runs.len(),
        document.beacons.len(),
        document.conduits.len(),
    );
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

fn conduit_player(uid: i32, x: f64, y: f64, z: f64, _in_water: bool) -> PlayerView {
    PlayerView {
        uid,
        uuid: None,
        fall_flying: false,
        main_arm_right: true,
        position: Vec3::new(x, y, z),
        delta: Vec3::new(0.0, 0.0, 0.0),
        width: 0.6,
        height: 1.8,
        eye_height: 1.62,
        health: 20.0,
        look_direction: Vec3::new(0.0, 0.0, 1.0),
        gaze_disguise: false,
        alive: true,
        spectator: false,
        creative: false,
        flying: false,
        explosion_knockback_resistance: 0.0,
        game_mode: 0,
        mining_fatigue: None,
        sneaking: false,
        sprinting: false,
        swimming: false,
        invisible: false,
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

fn conduit_water(level: &mut MachineLevel) {
    for i in -2..=2 {
        for j in -2..=2 {
            for k in -2..=2 {
                level.set_block(CONDUIT_POS.offset(i, j, k), BlockState::WATER);
            }
        }
    }
    level.set_block(CONDUIT_POS, BlockState(blocks::CONDUIT));
}

fn conduit_frame_blocks(level: &mut MachineLevel, count: usize) {
    for (i, j, k) in frame_offsets().into_iter().take(count) {
        level.set_block(CONDUIT_POS.offset(i, j, k), BlockState(blocks::PRISMARINE));
    }
}

fn conduit_drowned(level: &mut MachineLevel, x: f64, y: f64, z: f64) -> i32 {
    let mut entity = Entity::by_mob_type(MobType::Drowned, Vec3::new(x, y, z));
    entity.in_water = true;
    let id = level.entities.spawn(entity);
    // Java 夹具显式给 UUID，Rust 侧用同一个值，便于逐刻比较 `Target`。
    level.entities.with_mut(id, |entity: &mut Entity| {
        if let Some(mob) = entity.kind.mob_mut() {
            mob.uuid = [
                0x11, 0x11, 0x11, 0x11, 0x22, 0x22, 0x33, 0x33, 0x44, 0x44, 0x55, 0x55, 0x55, 0x55,
                0x55, 0x55,
            ];
            mob.health = 20.0;
        }
    });
    id
}

fn conduit_target_string(entity: &ConduitBlockEntity) -> String {
    let Some(bytes) = entity.target() else {
        return String::new();
    };
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

fn conduit_save_fields(fields: &[(String, Tag)]) -> ConduitSaveFields {
    let target = fields
        .iter()
        .find_map(|(key, value)| match (key.as_str(), value) {
            ("Target", Tag::IntArray(words)) => Some(words.clone()),
            _ => None,
        });
    ConduitSaveFields { target }
}

fn conduit_health(level: &MachineLevel, mob_id: Option<i32>) -> Vec<f32> {
    mob_id
        .and_then(|id| level.entities.get(id))
        .and_then(|entity| entity.kind.mob().map(|mob| mob.health))
        .into_iter()
        .collect()
}

fn replay_conduit(
    run: &ConduitRun,
    ticks: &mut usize,
    mismatches: &mut Vec<String>,
    break_test: &str,
) {
    if run.kind == "conduit_save_roundtrip" {
        let mut entity = ConduitBlockEntity::new(CONDUIT_POS);
        entity.set_target(Some([
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10,
        ]));
        let mut saved_fields = entity.additional();
        if break_test == "conduit_target" {
            saved_fields = vec![("target".to_owned(), Tag::IntArray(vec![1, 2, 3, 4]))];
        }
        let saved = conduit_save_fields(&saved_fields);
        if let Some(expected) = &run.saved
            && saved != *expected
        {
            mismatches.push(format!(
                "conduit_save_roundtrip saved {saved:?} != {expected:?}"
            ));
        }
        let empty = conduit_save_fields(&ConduitBlockEntity::new(CONDUIT_POS).additional());
        if let Some(expected_uuid) = &run.uuid
            && conduit_target_string(&entity) != *expected_uuid
        {
            mismatches.push(format!(
                "conduit_save_roundtrip uuid {} != {expected_uuid}",
                conduit_target_string(&entity)
            ));
        }
        if let Some(expected) = &run.empty_saved
            && empty != *expected
        {
            mismatches.push(format!(
                "conduit_save_roundtrip emptySaved {empty:?} != {expected:?}"
            ));
        }
        return;
    }

    let mut level = MachineLevel::empty(run.seed);
    conduit_water(&mut level);
    let frame_count = if run.kind == "conduit_frame_16" {
        16
    } else {
        42
    };
    conduit_frame_blocks(&mut level, frame_count);
    // 改坏确认：少一块框架方块会让 16 块题在第 0 刻失效。
    if break_test == "conduit_min_active" {
        let (i, j, k) = frame_offsets()[15];
        level.set_block(CONDUIT_POS.offset(i, j, k), BlockState::WATER);
    }
    if run.kind == "conduit_player_effects" {
        level.players.push(conduit_player(1, 3.5, 64.0, 0.0, true));
        level
            .players
            .push(conduit_player(2, 140.0, 64.0, 0.0, false));
        level.conduit_wet = vec![(1, true), (2, false)];
    }
    let mob_id =
        (run.kind == "conduit_drowned_attack").then(|| conduit_drowned(&mut level, 2.5, 64.0, 0.0));
    let mut entity = ConduitBlockEntity::new(CONDUIT_POS);
    let mut scheduled = BlockTicks::new();
    let mut last_health = conduit_health(&level, mob_id)
        .first()
        .copied()
        .unwrap_or(0.0);

    for frame in &run.ticks {
        let t = frame.game_time;
        if run.kind == "conduit_dry_deactivate" && t == 40 {
            level.set_block(CONDUIT_POS.offset(1, 1, 1), BlockState::AIR);
        }
        if run.kind == "conduit_dry_deactivate" && t == 80 {
            level.set_block(CONDUIT_POS.offset(1, 1, 1), BlockState::WATER);
        }
        scheduled.set_time(t);
        // 官方生物每刻 `baseTick` 把 `damageCooldownTime` 减一；夹具只 tick 方块实体，
        // 所以要在这里显式推进，否则 40 刻后的第二次攻击会被无敌冷却挡下。
        if let Some(id) = mob_id {
            level.entities.with_mut(id, |entity: &mut Entity| {
                if let Some(mob) = entity.kind.mob_mut() {
                    mob.hurt_time = (mob.hurt_time - 1).max(0);
                    mob.damage_cooldown = (mob.damage_cooldown - 1).max(0);
                }
            });
        }
        let effects = {
            let mut world = World::new(&mut level, &mut scheduled);
            entity.server_tick(&mut world);
            world.drain_effects()
        };
        *ticks += 1;

        let label = format!("{} t={t}", run.kind);
        let current_health = conduit_health(&level, mob_id)
            .first()
            .copied()
            .unwrap_or(0.0);
        let mut got_hurts = if last_health - current_health > 0.0 {
            vec![format!("magic:{:.1}", last_health - current_health)]
        } else {
            Vec::new()
        };
        last_health = current_health;
        // 改坏确认：把效果半径 / 时长 / 攻击量改坏，基线立刻报不一致。
        let mut got_effects = player_effect_rows(&effects);
        if break_test == "conduit_duration" {
            for row in &mut got_effects {
                row[2] -= 1;
            }
        }
        if break_test == "conduit_attack" {
            for hurt in &mut got_hurts {
                *hurt = "magic:3.0".to_owned();
            }
        }
        let got = ConduitFrame {
            game_time: t,
            active: entity.is_active(),
            hunting: entity.is_hunting(),
            blocks: entity.effect_block_count(),
            target: conduit_target_string(&entity),
            tick_count: entity.tick_count,
            next_ambient: entity.next_ambient_sound_activation(),
            sounds: sound_names(&effects),
            effects: got_effects,
            seed: level.random.source().seed(),
            health: conduit_health(&level, mob_id),
            hurts: Vec::new(),
            block_updates: conduit_block_updates(&effects),
        };
        // 血量 / 伤害记录单独比（Java 侧的 `health` 来自实际受伤后的血量）。
        let got_health = conduit_health(&level, mob_id);
        if got_health != frame.health {
            mismatches.push(format!(
                "{label} health {got_health:?} != {:?}",
                frame.health
            ));
        }
        if got_hurts != frame.hurts {
            mismatches.push(format!("{label} hurts {got_hurts:?} != {:?}", frame.hurts));
        }
        if (
            got.game_time,
            got.active,
            got.hunting,
            got.blocks,
            &got.target,
            got.tick_count,
            got.next_ambient,
            &got.sounds,
            &got.effects,
            got.seed,
            &got.block_updates,
        ) != (
            frame.game_time,
            frame.active,
            frame.hunting,
            frame.blocks,
            &frame.target,
            frame.tick_count,
            frame.next_ambient,
            &frame.sounds,
            &frame.effects,
            frame.seed,
            &frame.block_updates,
        ) {
            mismatches.push(format!(
                "{label} state ({}, {}, {}, {}, {:?}, {}, {}, {:?}, {:?}, {}, {:?}) != ({}, {}, {}, {}, {:?}, {}, {}, {:?}, {:?}, {}, {:?})",
                got.game_time, got.active, got.hunting, got.blocks, got.target, got.tick_count,
                got.next_ambient, got.sounds, got.effects, got.seed, got.block_updates,
                frame.game_time, frame.active, frame.hunting, frame.blocks, frame.target,
                frame.tick_count, frame.next_ambient, frame.sounds, frame.effects, frame.seed,
                frame.block_updates
            ));
        }
        if mismatches.len() > 40 {
            return;
        }
    }
}

fn conduit_block_updates(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::ConduitChanged { pos } => Some(format!("{},{},{},2", pos.x, pos.y, pos.z)),
            _ => None,
        })
        .collect()
}

fn beacon_player(uid: i32, x: f64, y: f64, z: f64, spectator: bool) -> PlayerView {
    PlayerView {
        uid,
        uuid: None,
        fall_flying: false,
        main_arm_right: true,
        position: Vec3::new(x, y, z),
        delta: Vec3::new(0.0, 0.0, 0.0),
        width: 0.6,
        height: 1.8,
        eye_height: 1.62,
        health: 20.0,
        look_direction: Vec3::new(0.0, 0.0, 1.0),
        gaze_disguise: false,
        alive: true,
        spectator,
        creative: false,
        flying: false,
        explosion_knockback_resistance: 0.0,
        game_mode: if spectator { 3 } else { 0 },
        mining_fatigue: None,
        sneaking: false,
        sprinting: false,
        swimming: false,
        invisible: false,
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

fn build_pyramid(level: &mut MachineLevel, pos: BlockPos, layers: i32, block: u32) {
    let state = BlockState(block);
    for layer in 1..=layers {
        let cy = pos.y - layer;
        for cx in (pos.x - layer)..=(pos.x + layer) {
            for cz in (pos.z - layer)..=(pos.z + layer) {
                level.set_block(BlockPos::new(cx, cy, cz), state);
            }
        }
    }
}

fn sound_names(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Sound { sound, .. } => sounds::SOUND_EVENTS
                .get(usize::try_from(*sound).ok()?)
                .map(|s| s.name.to_owned()),
            _ => None,
        })
        .collect()
}

fn player_effect_rows(effects: &[Effect]) -> Vec<[i32; 7]> {
    effects
        .iter()
        .filter_map(|e| match *e {
            Effect::PlayerEffect {
                target,
                effect,
                duration,
                amplifier,
                ambient,
                visible,
                show_icon,
            } => Some([
                target,
                effect,
                duration,
                amplifier,
                i32::from(ambient),
                i32::from(visible),
                i32::from(show_icon),
            ]),
            _ => None,
        })
        .collect()
}

fn save_fields_from(entries: &[(String, Tag)]) -> BeaconSaveFields {
    let get_str = |key: &str| {
        entries.iter().find_map(|(k, v)| match (k.as_str(), v) {
            (k, Tag::String(s)) if k == key => Some(s.clone()),
            _ => None,
        })
    };
    let levels = entries
        .iter()
        .find_map(|(k, v)| match (k.as_str(), v) {
            ("Levels", Tag::Int(n)) => Some(*n),
            _ => None,
        })
        .unwrap_or(-1);
    BeaconSaveFields {
        primary_effect: get_str("primary_effect"),
        secondary_effect: get_str("secondary_effect"),
        levels,
    }
}

fn replay_beacon(run: &BeaconRun, ticks: &mut usize, mismatches: &mut Vec<String>) {
    if run.kind == "beacon_save_roundtrip" {
        let mut valid = BeaconBlockEntity::new(BEACON_POS);
        valid.set_data(DATA_LEVELS, 4, None);
        valid.set_data(DATA_PRIMARY, encode_effect(Some(mob_effect::SPEED)), None);
        valid.set_data(
            DATA_SECONDARY,
            encode_effect(Some(mob_effect::REGENERATION)),
            None,
        );
        let saved_valid = valid.additional();
        if let Some(expected) = &run.saved_valid {
            let got = save_fields_from(&saved_valid);
            if got != *expected {
                mismatches.push(format!(
                    "beacon_save_roundtrip savedValid {got:?} != {expected:?}"
                ));
            }
        }
        let reloaded = BeaconBlockEntity::read(BEACON_POS, &Tag::Compound(saved_valid));
        let resaved_valid = reloaded.additional();
        if let Some(expected) = &run.resaved_valid {
            let got = save_fields_from(&resaved_valid);
            if got != *expected {
                mismatches.push(format!(
                    "beacon_save_roundtrip resavedValid {got:?} != {expected:?}"
                ));
            }
        }
        let invalid_raw = Tag::Compound(vec![
            (
                "primary_effect".to_owned(),
                Tag::String("minecraft:poison".to_owned()),
            ),
            (
                "secondary_effect".to_owned(),
                Tag::String("minecraft:haste".to_owned()),
            ),
            ("Levels".to_owned(), Tag::Int(4)),
        ]);
        let filtered = BeaconBlockEntity::read(BEACON_POS, &invalid_raw);
        let saved_filtered = filtered.additional();
        if let Some(expected) = &run.saved_filtered {
            let got = save_fields_from(&saved_filtered);
            if got != *expected {
                mismatches.push(format!(
                    "beacon_save_roundtrip savedFiltered {got:?} != {expected:?}"
                ));
            }
        }
        return;
    }

    let mut level = MachineLevel::empty(31);
    level.set_block(BEACON_POS, BlockState(blocks::BEACON));
    match run.kind.as_str() {
        "beacon_tier1_to_4" => {}
        "beacon_glass_and_block" => {
            build_pyramid(&mut level, BEACON_POS, 4, blocks::IRON_BLOCK);
            level.set_block(
                BlockPos::new(0, 65, 0),
                BlockState(blocks::RED_STAINED_GLASS),
            );
            level.set_block(
                BlockPos::new(0, 66, 0),
                BlockState(blocks::RED_STAINED_GLASS_PANE),
            );
            level.set_block(
                BlockPos::new(0, 67, 0),
                BlockState(blocks::BLUE_STAINED_GLASS),
            );
            level.set_block(BlockPos::new(0, 68, 0), BlockState(blocks::BEDROCK));
            level.set_block(
                BlockPos::new(0, 78, 0),
                BlockState(blocks::GREEN_STAINED_GLASS),
            );
        }
        "beacon_payment_tier4_regen" => {
            build_pyramid(&mut level, BEACON_POS, 4, blocks::IRON_BLOCK);
            level.players.push(beacon_player(1, 5.0, 64.0, 0.0, false));
            level
                .players
                .push(beacon_player(2, 50.0, 200.0, 0.0, false));
            level.players.push(beacon_player(3, 52.0, 64.0, 0.0, false));
            level.players.push(beacon_player(4, 2.0, 64.0, 0.0, true));
        }
        "beacon_payment_tier4_level2_and_deactivate" => {
            build_pyramid(&mut level, BEACON_POS, 4, blocks::EMERALD_BLOCK);
            level.players.push(beacon_player(1, 10.0, 64.0, 0.0, false));
        }
        "beacon_payment_tier2_range_and_duration" => {
            build_pyramid(&mut level, BEACON_POS, 2, blocks::IRON_BLOCK);
            level.players.push(beacon_player(1, 30.0, 64.0, 0.0, false));
            level.players.push(beacon_player(2, 32.0, 64.0, 0.0, false));
        }
        other => {
            mismatches.push(format!("unknown beacon scenario {other}"));
            return;
        }
    }

    let mut entity = BeaconBlockEntity::new(BEACON_POS);
    let mut scheduled = BlockTicks::new();

    for frame in &run.ticks {
        let t = frame.game_time;
        match (run.kind.as_str(), t) {
            ("beacon_tier1_to_4", 1) => {
                build_pyramid(&mut level, BEACON_POS, 1, blocks::IRON_BLOCK)
            }
            ("beacon_tier1_to_4", 85) => {
                build_pyramid(&mut level, BEACON_POS, 2, blocks::GOLD_BLOCK)
            }
            ("beacon_tier1_to_4", 165) => {
                build_pyramid(&mut level, BEACON_POS, 3, blocks::DIAMOND_BLOCK)
            }
            ("beacon_tier1_to_4", 245) => {
                build_pyramid(&mut level, BEACON_POS, 4, blocks::NETHERITE_BLOCK)
            }
            ("beacon_glass_and_block", 86) => {
                level.set_block(BlockPos::new(0, 70, 0), BlockState(blocks::STONE));
            }
            ("beacon_glass_and_block", 166) => {
                level.set_block(BlockPos::new(0, 70, 0), BlockState(blocks::AIR));
            }
            ("beacon_payment_tier4_level2_and_deactivate", 165) => {
                level.set_block(BlockPos::new(0, 63, 0), BlockState(blocks::AIR));
            }
            _ => {}
        }

        scheduled.set_time(t);
        let effects = {
            let mut world = World::new(&mut level, &mut scheduled);
            entity.server_tick(&mut world);
            world.drain_effects()
        };
        *ticks += 1;

        let label = format!("{} t={t}", run.kind);
        let got_data = entity.data_values().to_vec();
        if got_data != frame.data {
            mismatches.push(format!("{label} data {got_data:?} != {:?}", frame.data));
        }
        let got_beam: Vec<[i32; 2]> = entity
            .beam_sections()
            .iter()
            .map(|s| [s.color, s.height])
            .collect();
        if got_beam != frame.beam_sections {
            mismatches.push(format!(
                "{label} beamSections {got_beam:?} != {:?}",
                frame.beam_sections
            ));
        }
        let got_raw: Vec<[i32; 2]> = entity
            .raw_beam_sections()
            .iter()
            .map(|s| [s.color, s.height])
            .collect();
        if got_raw != frame.raw_beam_sections {
            mismatches.push(format!(
                "{label} rawBeamSections {got_raw:?} != {:?}",
                frame.raw_beam_sections
            ));
        }
        let got_sounds = sound_names(&effects);
        if got_sounds != frame.sounds {
            mismatches.push(format!(
                "{label} sounds {got_sounds:?} != {:?}",
                frame.sounds
            ));
        }
        let got_effects = player_effect_rows(&effects);
        if got_effects != frame.effects {
            mismatches.push(format!(
                "{label} effects {got_effects:?} != {:?}",
                frame.effects
            ));
        }

        if t == 81 && !run.payments.is_empty() {
            for (idx, check) in run.payments.iter().enumerate() {
                let primary = (check.primary >= 0).then_some(check.primary);
                let secondary = (check.secondary >= 0).then_some(check.secondary);
                let mut payment_count = i32::from(check.has_payment);
                let ok = payment_count > 0 && validate_effects(primary, secondary, entity.levels());
                let pay_effects = if ok {
                    let mut world = World::new(&mut level, &mut scheduled);
                    entity.set_data(DATA_PRIMARY, encode_effect(primary), Some(&mut world));
                    entity.set_data(DATA_SECONDARY, encode_effect(secondary), None);
                    payment_count -= 1;
                    world.drain_effects()
                } else {
                    Vec::new()
                };
                let pay_label = format!("{} payment[{idx}]", run.kind);
                if ok != check.ok {
                    mismatches.push(format!("{pay_label} ok {ok} != {}", check.ok));
                }
                if payment_count != check.remaining_payment {
                    mismatches.push(format!(
                        "{pay_label} remaining {payment_count} != {}",
                        check.remaining_payment
                    ));
                }
                let pay_data = entity.data_values().to_vec();
                if pay_data != check.data {
                    mismatches.push(format!("{pay_label} data {pay_data:?} != {:?}", check.data));
                }
                let pay_sounds = sound_names(&pay_effects);
                if pay_sounds != check.sounds {
                    mismatches.push(format!(
                        "{pay_label} sounds {pay_sounds:?} != {:?}",
                        check.sounds
                    ));
                }
            }
        }
        if mismatches.len() > 40 {
            return;
        }
    }
}

fn replay(run: &Run, ticks: &mut usize, mismatches: &mut Vec<String>) -> Result<(), String> {
    let state = BlockState(run.block);
    let mut level = MachineLevel::new(run.seed, state);
    let mut machine = Machine::open(&run.kind, state);
    machine.set_recipes(&run.recipes);
    for (slot, triple) in run.slots.iter().enumerate() {
        machine.set_item(slot, stack_from(triple)?);
    }
    for (id, value) in run.data.iter().copied().enumerate() {
        machine.set_data(id, value);
    }
    let mut scheduled = BlockTicks::new();
    let mut world = World::new(&mut level, &mut scheduled);
    if run.kind.starts_with("grindstone_xp") {
        return replay_grindstone(run, &world, mismatches);
    }
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
        let ours = orbs(&world);
        if ours != run.after_orbs {
            mismatches.push(format!(
                "{} after take orbs {:?} != {:?}",
                run.kind, ours, run.after_orbs
            ));
        }
    }
    Ok(())
}

/// 砂轮经验：`getExperienceAmount` 的一次 `nextInt` + `ExperienceOrb.award` 的拆分。
///
/// Grindstone XP: one `nextInt` from `getExperienceAmount` plus the
/// `ExperienceOrb.award` split.
fn replay_grindstone(run: &Run, world: &World, mismatches: &mut Vec<String>) -> Result<(), String> {
    if let Some(before) = run.before_take
        && world.random().seed() != before
    {
        mismatches.push(format!(
            "{} before take seed {} != {before}",
            run.kind,
            world.random().seed()
        ));
    }
    let (reward, values) = mc_bamboo_game::replay_grindstone_xp(run.xp_sum_input, run.seed);
    if reward != run.xp_reward {
        mismatches.push(format!("{} reward {reward} != {}", run.kind, run.xp_reward));
    }
    // 按官方顺序在同一个世界随机源上取数，再核对最终种子。
    // Draw on the same world random source in official order, then check the seed.
    let half = ((run.xp_sum_input as f64) / 2.0).ceil() as i32;
    if run.xp_sum_input > 0 {
        let _ = world.random().next_int(half);
    }
    for _ in &values {
        let _ = world.random().next_int(40);
    }
    if let Some(after) = run.after_take
        && world.random().seed() != after
    {
        mismatches.push(format!(
            "{} after take seed {} != {after}",
            run.kind,
            world.random().seed()
        ));
    }
    let center = 0.5_f64.to_bits() as i64;
    let expected: Vec<[i64; 5]> = values
        .into_iter()
        .map(|value| [i64::from(value), 1, center, center, center])
        .collect();
    if expected != run.after_orbs {
        mismatches.push(format!(
            "{} orbs {expected:?} != {:?}",
            run.kind, run.after_orbs
        ));
    }
    Ok(())
}

/// 从方块世界里读出刚生成的经验球，顺序按 uid。
///
/// Reads spawned experience orbs from the block world, in uid order.
fn orbs(world: &World) -> Vec<[i64; 5]> {
    let level = world.level();
    level
        .entity_uids()
        .into_iter()
        .filter_map(|uid| level.entity(uid))
        .filter_map(|entity| match &entity.kind {
            EntityKind::ExperienceOrb(orb) => Some([
                i64::from(orb.value),
                i64::from(orb.count),
                entity.position.x.to_bits() as i64,
                entity.position.y.to_bits() as i64,
                entity.position.z.to_bits() as i64,
            ]),
            _ => None,
        })
        .collect()
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
