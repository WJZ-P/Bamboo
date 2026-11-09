//! 用官方战利品表检查 `mc-bamboo-loot`。
//!
//! `LootOracle.java` 对随机方块状态、工具、爆炸半径和世界种子调用官方
//! `LootTable.getRandomItems`，箱子题再按 `fill` 把物品放进容器。每一题都带上
//! 跑完之后随机源的下一个 `nextLong`。这里用同样的参数重放，比较物品、数量、
//! 能对上的组件，以及这个 `nextLong`。
//!
//! `LootOracle.java` asks the official loot tables. This replays the same parameters
//! and compares items, counts, the components both sides can represent, and the next
//! `nextLong` from the random source.

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_blocks::{
    BlockState,
    random::{LegacyRandom, RandomSource},
};
use mc_bamboo_data::{items, registries};
use mc_bamboo_loot::{
    BlockDropParams, DamageSourceView, EntityDeathParams, EntityView, EquipmentSlot, LevelView,
    LootContainer, LootParams, LootStack, LootTable, Position, RandomSequences, block_drops,
    enchantment_id,
};
use serde::Deserialize;
use serde_json::Value;

use crate::oracle::collect_log;

const USAGE: &str = "usage: loot-oracle <java> [--cases N] [--seed S] [--show N]";
const DEFAULT_SHOW: usize = 5;

#[derive(Debug, Default)]
struct Options {
    forwarded: Vec<String>,
    show: Option<usize>,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--cases" | "--seed" => {
                    let value = args.next().ok_or(USAGE)?;
                    options
                        .forwarded
                        .extend([arg.to_owned(), (*value).to_owned()]);
                }
                "--show" => {
                    let value = args.next().ok_or(USAGE)?;
                    options.show = Some(value.parse().map_err(|_| USAGE.to_owned())?);
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

#[derive(Debug, Deserialize)]
struct Line {
    kind: String,
    #[serde(default)]
    block: Option<String>,
    #[serde(default)]
    state: u32,
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
    #[serde(default)]
    z: i32,
    #[serde(default)]
    #[serde(rename = "toolItem")]
    tool_item: i32,
    #[serde(default)]
    #[serde(rename = "toolCount")]
    tool_count: i32,
    #[serde(default)]
    enchants: Vec<EnchantJson>,
    #[serde(default)]
    explosion: Option<f32>,
    #[serde(default)]
    #[serde(rename = "worldSeed")]
    world_seed: Option<String>,
    #[serde(default)]
    table: Option<String>,
    #[serde(default)]
    seed: Option<String>,
    #[serde(default)]
    slots: usize,
    #[serde(default)]
    luck: f32,
    #[serde(default)]
    victim: Option<EntityJson>,
    #[serde(default)]
    attacker: Option<EntityJson>,
    #[serde(default, rename = "directAttacker")]
    direct_attacker: Option<EntityJson>,
    #[serde(default, rename = "damageType")]
    damage_type: i32,
    #[serde(default)]
    direct: bool,
    #[serde(default)]
    items: Vec<StackJson>,
    #[serde(default, rename = "nextLong")]
    next_long: Option<String>,
    #[serde(default, rename = "where")]
    where_: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    cases: usize,
    #[serde(default)]
    excluded: usize,
}

#[derive(Debug, Deserialize)]
struct EnchantJson {
    name: String,
    level: i32,
}

#[derive(Debug, Deserialize)]
struct StackJson {
    item: i32,
    count: i32,
    #[serde(default)]
    components: BTreeMap<String, Value>,
    /// 空物品堆的组件。函数写出的值和它相同，就不算战利品改过。
    ///
    /// Components of a fresh stack. A value a function wrote is unchanged when it matches.
    #[serde(default)]
    baseline: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct EntityJson {
    #[serde(rename = "type")]
    type_id: i32,
    living: bool,
    #[serde(rename = "onGround")]
    on_ground: bool,
    #[serde(rename = "onFire")]
    on_fire: bool,
    crouching: bool,
    sprinting: bool,
    swimming: bool,
    flying: bool,
    #[serde(rename = "fallFlying")]
    fall_flying: bool,
    baby: bool,
    #[serde(rename = "inWater")]
    in_water: bool,
    x: f64,
    y: f64,
    z: f64,
    mx: f64,
    my: f64,
    mz: f64,
    fall: f64,
    tick: i32,
    #[serde(rename = "onX")]
    on_x: i32,
    #[serde(rename = "onY")]
    on_y: i32,
    #[serde(rename = "onZ")]
    on_z: i32,
    #[serde(rename = "belowX")]
    below_x: i32,
    #[serde(rename = "belowY")]
    below_y: i32,
    #[serde(rename = "belowZ")]
    below_z: i32,
    sheep: bool,
    sheared: bool,
    raider: bool,
    #[serde(rename = "hasRaid")]
    has_raid: bool,
    captain: bool,
    #[serde(rename = "fishingHook")]
    fishing_hook: bool,
    #[serde(rename = "openWater")]
    open_water: bool,
    #[serde(default, rename = "cubeSize")]
    cube_size: Option<i32>,
    #[serde(default)]
    equipment: BTreeMap<String, StackJson>,
    /// 实体组件的 codec JSON。羊的毛色、鸡和青蛙的变种、哞菇的颜色走这里。
    ///
    /// Entity components as codec JSON. Sheep color, chicken and frog variants, and
    /// mooshroom type are compared through this map.
    #[serde(default)]
    components: BTreeMap<String, Value>,
}

#[derive(Debug, Default)]
struct Report {
    block: usize,
    chest: usize,
    entity: usize,
    excluded: usize,
    skipped: usize,
    reasons: BTreeMap<String, usize>,
    skip_reasons: BTreeMap<String, usize>,
    mismatches: Vec<String>,
    show: usize,
    summary: Option<(usize, usize)>,
}

/// 启动官方进程，边读边比较。
///
/// Starts the official process and compares each case as it arrives.
pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let mut child = command
        .args(&options.forwarded)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;

    let mut report = Report {
        show: options.show.unwrap_or(DEFAULT_SHOW),
        ..Report::default()
    };
    let read = report.read_lines(BufReader::new(stdout));
    if read.is_err() {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    read.map_err(|error| format!("{error}\n{log}"))?;
    if !status.success() {
        return Err(format!("LootOracle.java failed: {status}\n{log}"));
    }
    report.finish()
}

impl Report {
    fn read_lines(&mut self, source: BufReader<impl Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            if !line.starts_with('{') {
                continue;
            }
            let case: Line = serde_json::from_str(&line)
                .map_err(|error| format!("bad loot oracle line: {error}: {line}"))?;
            match case.kind.as_str() {
                "summary" => self.summary = Some((case.cases, case.excluded)),
                "excluded" => self.note_excluded(&case),
                "block" => {
                    if !self.check_block(&case) {
                        self.block += 1;
                    }
                }
                "chest" => {
                    if !self.check_chest(&case) {
                        self.chest += 1;
                    }
                }
                "entity" => {
                    if !self.check_entity(&case) {
                        self.entity += 1;
                    }
                }
                other => return Err(format!("unknown loot oracle kind {other}")),
            }
        }
        Ok(())
    }

    fn note_excluded(&mut self, case: &Line) {
        self.excluded += 1;
        let reason = classify(case.reason.as_deref().unwrap_or(""));
        *self.reasons.entry(reason.clone()).or_default() += 1;
        if reason == "other" {
            self.mismatch(format!(
                "excluded {} {}: {}",
                case.where_.as_deref().unwrap_or("?"),
                case.table.as_deref().unwrap_or("?"),
                case.reason.as_deref().unwrap_or("")
            ));
        }
    }

    /// 返回 `true` 表示这题依赖还没做的功能，不算差异。
    ///
    /// Returns true when the case needs a feature this crate does not implement.
    fn check_block(&mut self, case: &Line) -> bool {
        let world_seed = match parse_long(case.world_seed.as_deref(), "worldSeed") {
            Ok(seed) => seed,
            Err(error) => {
                self.mismatch(error);
                return false;
            }
        };
        let expected_next = match parse_long(case.next_long.as_deref(), "nextLong") {
            Ok(value) => value,
            Err(error) => {
                self.mismatch(error);
                return false;
            }
        };
        let tool = match tool_stack(case) {
            Ok(tool) => tool,
            Err(error) => {
                self.mismatch(error);
                return false;
            }
        };
        let sequences = RandomSequences::new(world_seed);
        let fallback = LegacyRandom::new(world_seed);
        let level = FlatLevel;
        let params = BlockDropParams {
            state: BlockState(case.state),
            pos: (case.x, case.y, case.z),
            level: &level,
            block_entity: None,
            breaker: None,
            tool: Some(tool),
            explosion_radius: case.explosion,
            luck: 0.0,
        };
        let our_block = BlockState(case.state).block().name;
        if let Some(official_block) = case.block.as_deref()
            && official_block != our_block
        {
            self.mismatch(format!(
                "state {} is {official_block} officially and {our_block} in bamboo",
                case.state
            ));
            return false;
        }
        let dropped = match block_drops(&params, Some(&sequences), &fallback) {
            Ok(dropped) => dropped,
            Err(error) => {
                return self.note_runtime(
                    &format!(
                        "block state {} at ({}, {}, {})",
                        case.state, case.x, case.y, case.z
                    ),
                    &error.to_string(),
                );
            }
        };
        let table_name = mc_bamboo_data::loot::block_loot_table(
            mc_bamboo_blocks::BlockState(case.state).block().name,
        );
        let next = match table_name.and_then(|name| LootTable::from_name(name).ok()) {
            Some(table) => match table.random_sequence() {
                Some(name) => sequences.get(name).next_long(),
                None => fallback.next_long(),
            },
            None => fallback.next_long(),
        };
        self.compare_stacks(
            &format!("block state {} seed {world_seed}", case.state),
            &case.items,
            &dropped,
            false,
        );
        if next != expected_next {
            self.mismatch(format!(
                "block state {} seed {world_seed}: nextLong official {expected_next}, bamboo {next}",
                case.state
            ));
        }
        false
    }

    fn check_chest(&mut self, case: &Line) -> bool {
        let Some(table_name) = case.table.as_deref() else {
            self.mismatch("chest case is missing the table name".to_owned());
            return false;
        };
        let seed = match parse_long(case.seed.as_deref(), "seed") {
            Ok(seed) => seed,
            Err(error) => {
                self.mismatch(error);
                return false;
            }
        };
        let expected_next = match parse_long(case.next_long.as_deref(), "nextLong") {
            Ok(value) => value,
            Err(error) => {
                self.mismatch(error);
                return false;
            }
        };
        let table = match LootTable::from_name(table_name) {
            Ok(table) => table,
            Err(error) => {
                self.mismatch(format!("{table_name}: {error}"));
                return false;
            }
        };
        let level = FlatLevel;
        let params = LootParams {
            level: Some(&level),
            origin: Some(Position::new(
                f64::from(case.x) + 0.5,
                f64::from(case.y) + 0.5,
                f64::from(case.z) + 0.5,
            )),
            luck: case.luck,
            ..LootParams::default()
        };
        let random = LegacyRandom::new(seed);
        let mut container = SlotContainer {
            slots: vec![LootStack::empty(); case.slots],
        };
        if let Err(error) = table.fill_with_source(&mut container, &params, &random) {
            return self.note_runtime(&format!("{table_name} seed {seed}"), &error.to_string());
        }
        let next = random.next_long();
        self.compare_stacks(
            &format!("{table_name} seed {seed}"),
            &case.items,
            &container.slots,
            true,
        );
        if next != expected_next {
            self.mismatch(format!(
                "{table_name} seed {seed}: nextLong official {expected_next}, bamboo {next}"
            ));
        }
        false
    }

    /// 实体死亡。`seed == 0` 走 `entity_death_drops`，非零种子用同一套参数和 `LegacyRandom`。
    ///
    /// Entity death. Seed 0 goes through `entity_death_drops`. A non-zero seed uses the same
    /// parameters and a `LegacyRandom`, which is what that entry creates internally.
    fn check_entity(&mut self, case: &Line) -> bool {
        let Some(table_name) = case.table.as_deref() else {
            self.mismatch("entity case is missing the table name".to_owned());
            return false;
        };
        let Some(victim) = case.victim.as_ref() else {
            self.mismatch(format!("{table_name}: missing victim"));
            return false;
        };
        let world_seed = match parse_long(case.world_seed.as_deref(), "worldSeed") {
            Ok(seed) => seed,
            Err(error) => {
                self.mismatch(error);
                return false;
            }
        };
        let seed = match parse_long(case.seed.as_deref(), "seed") {
            Ok(seed) => seed,
            Err(error) => {
                self.mismatch(error);
                return false;
            }
        };
        let expected_next = match parse_long(case.next_long.as_deref(), "nextLong") {
            Ok(value) => value,
            Err(error) => {
                self.mismatch(error);
                return false;
            }
        };
        let victim_view = match entity_view(victim) {
            Ok(view) => view,
            Err(error) => {
                self.mismatch(format!("{table_name}: {error}"));
                return false;
            }
        };
        let attacking = match optional_entity(case.attacker.as_ref()) {
            Ok(view) => view,
            Err(error) => {
                self.mismatch(format!("{table_name}: {error}"));
                return false;
            }
        };
        let direct = match optional_entity(case.direct_attacker.as_ref()) {
            Ok(view) => view,
            Err(error) => {
                self.mismatch(format!("{table_name}: {error}"));
                return false;
            }
        };
        let sequences = RandomSequences::new(world_seed);
        let fallback = LegacyRandom::new(world_seed);
        let level = FlatLevel;
        let params = EntityDeathParams {
            entity: victim_view,
            origin: Position::new(victim.x, victim.y, victim.z),
            damage_source: DamageSourceView {
                damage_type: case.damage_type,
                direct_entity: direct.clone().map(Box::new),
                source_entity: attacking.clone().map(Box::new),
                is_direct: case.direct,
            },
            attacking_entity: attacking,
            direct_attacking_entity: direct,
            last_damage_player: None,
            luck: 0.0,
            level: Some(&level),
        };
        let dropped = if seed == 0 {
            match mc_bamboo_loot::entity_death_drops(
                table_name,
                &params,
                seed,
                Some(&sequences),
                &fallback,
            ) {
                Ok(dropped) => dropped,
                Err(error) => {
                    return self
                        .note_runtime(&format!("{table_name} seed {seed}"), &error.to_string());
                }
            }
        } else {
            let table = match LootTable::from_name(table_name) {
                Ok(table) => table,
                Err(error) => {
                    self.mismatch(format!("{table_name}: {error}"));
                    return false;
                }
            };
            let random = LegacyRandom::new(seed);
            match table.get_random_items(&params.loot_params(), &random) {
                Ok(dropped) => {
                    let next = random.next_long();
                    self.compare_stacks(
                        &format!("{table_name} seed {seed}"),
                        &case.items,
                        &dropped,
                        false,
                    );
                    if next != expected_next {
                        self.mismatch(format!(
                            "{table_name} seed {seed}: nextLong official {expected_next}, bamboo {next}"
                        ));
                    }
                    return false;
                }
                Err(error) => {
                    return self
                        .note_runtime(&format!("{table_name} seed {seed}"), &error.to_string());
                }
            }
        };
        let table = match LootTable::from_name(table_name) {
            Ok(table) => table,
            Err(error) => {
                self.mismatch(format!("{table_name}: {error}"));
                return false;
            }
        };
        let next = match table.random_sequence() {
            Some(name) => sequences.get(name).next_long(),
            None => fallback.next_long(),
        };
        self.compare_stacks(
            &format!("{table_name} seed {seed}"),
            &case.items,
            &dropped,
            false,
        );
        if next != expected_next {
            self.mismatch(format!(
                "{table_name} seed {seed}: nextLong official {expected_next}, bamboo {next}"
            ));
        }
        false
    }

    /// 求值碰到还没做的功能时记成跳过，其它错误记成差异。返回值表示是不是跳过。
    ///
    /// An unimplemented feature skips the case. Any other error is a mismatch.
    /// The return value is true when the case was skipped.
    fn note_runtime(&mut self, label: &str, error: &str) -> bool {
        if error.contains("unsupported") {
            self.skipped += 1;
            let reason = if error.len() > 160 {
                error[..160].to_owned()
            } else {
                error.to_owned()
            };
            *self.skip_reasons.entry(reason).or_default() += 1;
            true
        } else {
            self.mismatch(format!("{label}: bamboo failed: {error}"));
            false
        }
    }

    fn compare_stacks(
        &mut self,
        label: &str,
        official: &[StackJson],
        bamboo: &[LootStack],
        slots: bool,
    ) {
        if official.len() != bamboo.len() {
            self.mismatch(format!(
                "{label}: {} official stacks, bamboo {}",
                official.len(),
                bamboo.len()
            ));
            return;
        }
        for (index, (official, bamboo)) in official.iter().zip(bamboo).enumerate() {
            let place = if slots {
                format!("slot {index}")
            } else {
                format!("stack {index}")
            };
            // 数量不大于 0 时 `ItemStack.getItem()` 返回空气，`getCount()` 返回 0，组件也不再可见。
            // A non-positive count makes `ItemStack.getItem()` return air, `getCount()` return 0,
            // and the components disappear.
            let (bamboo_item, bamboo_count, bamboo_components) = if bamboo.is_empty() {
                (items::AIR, 0, BTreeMap::new())
            } else {
                (
                    bamboo.item,
                    bamboo.count,
                    normalize_components(&bamboo.components),
                )
            };
            if official.item != bamboo_item || official.count != bamboo_count {
                self.mismatch(format!(
                    "{label} {place}: official {}:{}, bamboo {bamboo_item}:{bamboo_count}",
                    official.item, official.count
                ));
                continue;
            }
            let official_components = normalize_components(&official.components);
            let bamboo_components = strip_baseline(
                &bamboo_components,
                &normalize_components(&official.baseline),
            );
            if official_components != bamboo_components {
                self.mismatch(format!(
                    "{label} {place}: components official {official_components:?}, bamboo {bamboo_components:?}"
                ));
                continue;
            }
            // 战利品组件还要能转成真正的物品堆。转换会按存档 `Codec` 解一遍，
            // 解不开的组件在这一层就露出来。
            // The components must also convert into a real item stack through the
            // persistent codec; an unsupported component shows up here.
            let converted = mc_bamboo_game::loot_stack(bamboo);
            if converted.item != bamboo_item || converted.count != bamboo_count {
                self.mismatch(format!(
                    "{label} {place}: converted stack {}:{} differs from {bamboo_item}:{bamboo_count}",
                    converted.item, converted.count
                ));
            }
            let patch = converted.patch();
            if !official_components.is_empty() && patch.entries().is_empty() {
                self.mismatch(format!(
                    "{label} {place}: all {} loot components were dropped by the conversion",
                    official_components.len()
                ));
            }
        }
    }

    fn mismatch(&mut self, message: String) {
        if self.mismatches.len() < self.show {
            self.mismatches.push(message);
        } else {
            self.mismatches.push(String::new());
        }
    }

    fn finish(self) -> Result<(), String> {
        let shown: Vec<&str> = self
            .mismatches
            .iter()
            .filter(|line| !line.is_empty())
            .map(String::as_str)
            .collect();
        let hidden = self.mismatches.len().saturating_sub(shown.len());
        let compared = self.block + self.chest + self.entity;
        println!(
            "loot oracle: {compared} compared ({} block, {} chest, {} entity), {} excluded, {} skipped, {} mismatched",
            self.block,
            self.chest,
            self.entity,
            self.excluded,
            self.skipped,
            self.mismatches.len()
        );
        if !self.reasons.is_empty() {
            println!("excluded by reason:");
            for (reason, count) in &self.reasons {
                println!("  {count} {reason}");
            }
        }
        if !self.skip_reasons.is_empty() {
            println!("skipped by reason:");
            for (reason, count) in &self.skip_reasons {
                println!("  {count} {reason}");
            }
        }
        for line in &shown {
            println!("  {line}");
        }
        if hidden > 0 {
            println!("  ... {hidden} more");
        }
        let Some((cases, excluded)) = self.summary else {
            return Err("LootOracle.java did not print a summary".to_owned());
        };
        let received = compared + self.skipped + self.excluded;
        if cases != received || excluded != self.excluded {
            return Err(format!(
                "summary says {cases} cases and {excluded} excluded, counted {received} and {}",
                self.excluded
            ));
        }
        if !self.mismatches.is_empty() {
            return Err(format!(
                "{} loot cases differ from the official tables",
                self.mismatches.len()
            ));
        }
        Ok(())
    }
}

fn parse_long(text: Option<&str>, what: &str) -> Result<i64, String> {
    let text = text.ok_or_else(|| format!("missing {what}"))?;
    text.parse()
        .map_err(|_| format!("{what} is not a long: {text}"))
}

fn tool_stack(case: &Line) -> Result<LootStack, String> {
    let mut tool = if case.tool_count <= 0 {
        LootStack::empty()
    } else {
        LootStack::new(case.tool_item, case.tool_count)
    };
    for enchant in &case.enchants {
        let id = enchantment_id(&enchant.name)
            .ok_or_else(|| format!("unknown enchantment {}", enchant.name))?;
        tool.enchant(id, enchant.level);
    }
    Ok(tool)
}

/// 扣掉和物品原型相同的组件。官方 `changedComponents` 用的是同一条规则。
///
/// Drops components that match the item prototype. Official `changedComponents` uses the same rule.
fn strip_baseline(
    components: &BTreeMap<String, Value>,
    baseline: &BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    components
        .iter()
        .filter(|(key, value)| baseline.get(*key) != Some(*value))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// 名字组件官方是文本对象，两边都收成纯文本再比。
///
/// Name components are text objects. Both sides are reduced to plain text.
fn normalize_components(components: &BTreeMap<String, Value>) -> BTreeMap<String, Value> {
    let mut normalized = BTreeMap::new();
    for (key, value) in components {
        let value = match key.as_str() {
            "minecraft:custom_name" | "minecraft:item_name" => {
                Value::String(plain_text(value).unwrap_or_else(|| value.to_string()))
            }
            _ => value.clone(),
        };
        normalized.insert(key.clone(), value);
    }
    normalized
}

fn plain_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Object(map) => map.get("text").and_then(Value::as_str).map(str::to_owned),
        _ => None,
    }
}

fn classify(reason: &str) -> String {
    // 造实体失败要先认出来。堆栈里常带 `getChunkSource`，不能算成结构搜索。
    // Entity construction fails first. Those stacks often mention `getChunkSource`
    // and are not structure searches.
    if reason.contains("entity setup") {
        return if reason.contains("no entity type") {
            "entity setup: no type".to_owned()
        } else if reason.contains("create returned null") {
            "entity setup: create returned null".to_owned()
        } else if reason.contains("NullPointerException") {
            "entity setup: NullPointerException".to_owned()
        } else {
            "entity setup: other".to_owned()
        };
    }
    if reason.contains("getRaidAt") || reason.contains("RaidManager") {
        return "raid lookup".to_owned();
    }
    if reason.contains("findNearestMapStructure")
        || reason.contains("ExplorationMapFunction")
        || reason.contains("structureManager")
        || reason.contains("StructureManager")
    {
        return "structure search".to_owned();
    }
    let snippet: String = reason.chars().take(180).collect();
    format!("other: {snippet}")
}

fn optional_entity(json: Option<&EntityJson>) -> Result<Option<EntityView>, String> {
    json.map(entity_view).transpose()
}

/// 把官方实体快照收成 `EntityView`。字段和 `EntityFlagsPredicate` 读的是同一组。
///
/// Turns the official entity snapshot into an `EntityView`. The flags are the ones
/// `EntityFlagsPredicate` reads.
fn entity_view(json: &EntityJson) -> Result<EntityView, String> {
    let mut entity = EntityView::new(json.type_id);
    entity.is_living = json.living;
    entity.is_on_ground = json.on_ground;
    entity.is_on_fire = json.on_fire;
    entity.is_sneaking = json.crouching;
    entity.is_sprinting = json.sprinting;
    entity.is_swimming = json.swimming;
    entity.is_flying = json.flying;
    entity.is_baby = json.baby;
    entity.is_in_water = json.in_water;
    entity.is_fall_flying = json.fall_flying;
    entity.position = Some(Position::new(json.x, json.y, json.z));
    entity.movement = Some(Position::new(json.mx, json.my, json.mz));
    entity.fall_distance = Some(json.fall);
    entity.tick_count = Some(json.tick);
    entity.on_pos = Some((json.on_x, json.on_y, json.on_z));
    entity.movement_affected_by_pos = Some((json.below_x, json.below_y, json.below_z));
    entity.is_sheep = json.sheep;
    entity.sheep_sheared = json.sheared;
    entity.components.clone_from(&json.components);
    entity.is_raider = json.raider;
    entity.has_raid = json.has_raid;
    entity.is_captain = json.captain;
    entity.is_fishing_hook = json.fishing_hook;
    entity.in_open_water = json.open_water;
    entity.cube_mob_size = json.cube_size;
    for (name, stack) in &json.equipment {
        let slot = equipment_slot(name)?;
        entity.equipment[slot.index()] = Some(stack_from_parts(stack)?);
    }
    Ok(entity)
}

fn stack_from_parts(stack: &StackJson) -> Result<LootStack, String> {
    let mut item = if stack.count <= 0 {
        LootStack::empty()
    } else {
        LootStack::new(stack.item, stack.count)
    };
    for (name, value) in &stack.components {
        item.set_component(name, value.clone());
    }
    Ok(item)
}

fn equipment_slot(name: &str) -> Result<EquipmentSlot, String> {
    Ok(match name {
        "mainhand" => EquipmentSlot::Mainhand,
        "offhand" => EquipmentSlot::Offhand,
        "feet" => EquipmentSlot::Feet,
        "legs" => EquipmentSlot::Legs,
        "chest" => EquipmentSlot::Chest,
        "head" => EquipmentSlot::Head,
        "body" => EquipmentSlot::Body,
        "saddle" => EquipmentSlot::Saddle,
        other => return Err(format!("unknown equipment slot {other}")),
    })
}

/// 空世界：空气、平原、不下雨、亮度 15。和 `LootOracle.java` 的假世界一致。
///
/// Empty world: air, plains, no weather, light 15. Matches the fake level in `LootOracle.java`.
struct FlatLevel;

impl LevelView for FlatLevel {
    fn is_loaded(&self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        Some(true)
    }

    fn block_state(&self, _x: i32, _y: i32, _z: i32) -> Option<BlockState> {
        Some(BlockState::AIR)
    }

    fn biome(&self, _x: i32, _y: i32, _z: i32) -> Option<i32> {
        Some(
            registries::biome::NAMES
                .iter()
                .position(|name| *name == "minecraft:plains")
                .unwrap_or(0) as i32,
        )
    }

    fn can_see_sky(&self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        Some(true)
    }

    fn is_raining(&self) -> Option<bool> {
        Some(false)
    }

    fn is_thundering(&self) -> Option<bool> {
        Some(false)
    }

    fn day_time(&self) -> Option<i64> {
        Some(0)
    }

    fn game_time(&self) -> Option<i64> {
        Some(0)
    }

    fn dimension(&self) -> Option<&'static str> {
        Some("minecraft:overworld")
    }

    fn light(&self, _x: i32, _y: i32, _z: i32) -> Option<i32> {
        Some(15)
    }

    fn is_smokey(&self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        Some(false)
    }
}

struct SlotContainer {
    slots: Vec<LootStack>,
}

impl LootContainer for SlotContainer {
    fn slot_count(&self) -> usize {
        self.slots.len()
    }

    fn is_slot_empty(&self, slot: usize) -> bool {
        self.slots[slot].is_empty()
    }

    fn set_slot(&mut self, slot: usize, stack: LootStack) {
        self.slots[slot] = stack;
    }
}
