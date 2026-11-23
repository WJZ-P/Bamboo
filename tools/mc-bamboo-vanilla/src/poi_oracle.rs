//! `poi-oracle`：用官方 `PoiManager` 对拍 Bamboo 的 POI 存储。
//!
//! `PoiOracle.java` 在临时目录里构造官方 `PoiManager`，按 stdin 给的一组方块变化
//! 跑 `PoiTypes.forState` → `remove` / `add`，每一步之后把查询族逐项输出，再输出
//! `SectionStorage.writeChunk` 的根标签、codec 读回再编码的逐段标签、以及
//! `sectionsToVillage` 一片网格的结果。这里用 Bamboo 的 `PoiStore` 重放同一序列，
//! 逐项（含顺序、票数）比较；存档和村庄距离也逐项比。
//!
//! 命令：`cargo run -p mc-bamboo-vanilla -- poi-oracle <java> [--show N]
//! [--break-self-test tickets|meeting|remove]`。

use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_data::{blocks, poi, poi::POI_TYPES};
use mc_bamboo_nbt::Tag;
use mc_bamboo_storage::poi_nbt;
use mc_bamboo_world::{
    light::SectionPos,
    poi::{NO_VILLAGE_LEVEL, Occupancy, PoiStore, type_name},
};
use serde_json::{Value, json};

const USAGE: &str = "usage: poi-oracle <java> [--show N] [--break-self-test NAME]";
const DEFAULT_SHOW: usize = 8;
/// 三处改坏自检的名字。
const BREAK_TESTS: [&str; 3] = ["tickets", "meeting", "remove"];

struct Options {
    show: usize,
    break_self_test: Option<String>,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut out = Self {
            show: DEFAULT_SHOW,
            break_self_test: None,
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--show" => {
                    out.show = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?;
                }
                "--break-self-test" => {
                    let name = (*args.next().ok_or(USAGE)?).to_owned();
                    if !BREAK_TESTS.contains(&name.as_str()) {
                        return Err(format!("unknown break-self-test {name}"));
                    }
                    out.break_self_test = Some(name);
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Default)]
struct Report {
    checks: usize,
    failures: Vec<String>,
}

impl Report {
    fn check(&mut self, ours: &Value, theirs: &Value, what: impl Into<String>) {
        self.checks += 1;
        if ours != theirs {
            self.failures
                .push(format!("{}: ours={ours} official={theirs}", what.into()));
        }
    }

    fn finish(&self, options: &Options) -> Result<(), String> {
        for failure in self.failures.iter().take(options.show.max(1)) {
            eprintln!("  {failure}");
        }
        if let Some(name) = &options.break_self_test {
            return if self.failures.is_empty() {
                Err(format!(
                    "poi-oracle self-test {name} broke a value but reported 0 mismatches"
                ))
            } else {
                println!(
                    "poi-oracle self-test {name}: detector works ({} mismatches)",
                    self.failures.len()
                );
                Ok(())
            };
        }
        if self.failures.is_empty() {
            println!("poi-oracle: {} checks, 0 mismatches", self.checks);
            Ok(())
        } else {
            Err(format!(
                "poi-oracle: {} of {} checks failed",
                self.failures.len(),
                self.checks
            ))
        }
    }
}

/// 一组方块变化 + 查询。所有坐标 / 状态 ID 都由这里给出，Java 和 Rust 用同一份。
struct Scenario {
    ops: Vec<Value>,
    queries: Vec<Value>,
    sections: Vec<[i32; 3]>,
    chunk: ChunkPos,
}

/// 从导出的官方 POI 表里挑一个类型的第一个匹配状态。
fn first_state(name: &str) -> u32 {
    let id = poi::poi_type_id(name).expect("官方 POI 类型");
    *POI_TYPES[id as usize].states.first().expect("有匹配状态")
}

fn pos(x: i32, y: i32, z: i32) -> Value {
    json!([x, y, z])
}

fn build_scenario() -> Scenario {
    let mut ops = Vec::new();
    let mut placed: Vec<(BlockPos, u32)> = Vec::new();
    // 13 种工作站 + 床头 + 钟 + 蜂巢 + 避雷针，铺在 3 个区块、3 个高度段里。
    let names = [
        "armorer",
        "butcher",
        "cartographer",
        "cleric",
        "farmer",
        "fisherman",
        "fletcher",
        "leatherworker",
        "librarian",
        "mason",
        "shepherd",
        "toolsmith",
        "weaponsmith",
        "home",
        "meeting",
        "beehive",
        "lightning_rod",
    ];
    for (index, name) in names.iter().enumerate() {
        let x = 1 + (index as i32 % 5) * 3;
        let z = 1 + (index as i32 / 5) * 3;
        let y = -60 + (index as i32 / 8) * 17;
        let at = BlockPos::new(x, y, z);
        let state = first_state(name);
        ops.push(json!({"at": pos(x, y, z), "old": blocks::AIR, "new": state}));
        placed.push((at, state));
    }
    // 跨区块的几张床和避雷针，让 `getInSquare` 的区块环和并列距离都参与。
    for (index, &(x, z)) in [(17, 2), (34, 2), (1, 18), (-15, 2)].iter().enumerate() {
        let at = BlockPos::new(x, -60, z);
        let state = first_state(if index % 2 == 0 {
            "home"
        } else {
            "lightning_rod"
        });
        ops.push(json!({"at": pos(x, -60, z), "old": blocks::AIR, "new": state}));
        placed.push((at, state));
    }
    // 拆掉两个工作站再放回一个，验证 remove / add 与「同类型换状态不动记录」。
    let removed = placed[2].0;
    ops.push(
        json!({"at": pos(removed.x, removed.y, removed.z), "old": placed[2].1, "new": blocks::AIR}),
    );
    let changed = placed[3].0;
    ops.push(json!({"at": pos(changed.x, changed.y, changed.z), "old": placed[3].1, "new": first_state("armorer")}));
    // 扣一张票、还一张票（钟有 32 张，取完还有）。
    let bell = placed[14].0;
    ops.push(json!({"take": pos(bell.x, bell.y, bell.z), "radius": 8, "poi": "minecraft:meeting"}));
    ops.push(json!({"release": pos(bell.x, bell.y, bell.z)}));
    // 一个 bed 的 HAS_SPACE 查询：take 之后再 release。
    let bed = placed[13].0;
    ops.push(json!({"take": pos(bed.x, bed.y, bed.z), "radius": 8, "poi": "minecraft:home"}));

    let center = pos(8, -60, 8);
    let mut queries = Vec::new();
    for name in ["home", "meeting", "armorer", "beehive", "lightning_rod"] {
        for occupancy in ["HAS_SPACE", "IS_OCCUPIED", "ANY"] {
            for radius in [8, 24, 48, 128] {
                queries.push(json!({
                    "kind": "countInRange", "poi": format!("minecraft:{name}"),
                    "center": center, "radius": radius, "occupancy": occupancy,
                }));
                queries.push(json!({
                    "kind": "getInSquare", "poi": format!("minecraft:{name}"),
                    "center": center, "radius": radius, "occupancy": occupancy,
                }));
                queries.push(json!({
                    "kind": "getInRange", "poi": format!("minecraft:{name}"),
                    "center": center, "radius": radius, "occupancy": occupancy,
                }));
                queries.push(json!({
                    "kind": "findAllClosestFirstWithType", "poi": format!("minecraft:{name}"),
                    "center": center, "radius": radius, "occupancy": occupancy,
                }));
                queries.push(json!({
                    "kind": "findClosest", "poi": format!("minecraft:{name}"),
                    "center": center, "radius": radius, "occupancy": occupancy,
                }));
                queries.push(json!({
                    "kind": "findClosestWithType", "poi": format!("minecraft:{name}"),
                    "center": center, "radius": radius, "occupancy": occupancy,
                }));
            }
        }
    }
    for &(at, _) in &placed[..6] {
        for occupancy in ["HAS_SPACE", "ANY"] {
            queries.push(json!({
                "kind": "exists", "poi": "minecraft:home",
                "at": pos(at.x, at.y, at.z), "center": center,
                "radius": 48, "occupancy": occupancy,
            }));
            queries.push(json!({
                "kind": "getType", "poi": "minecraft:home",
                "at": pos(at.x, at.y, at.z), "center": center,
                "radius": 48, "occupancy": occupancy,
            }));
            queries.push(json!({
                "kind": "freeTickets", "poi": "minecraft:home",
                "at": pos(at.x, at.y, at.z), "center": center,
                "radius": 48, "occupancy": occupancy,
            }));
        }
    }
    let mut sections = Vec::new();
    for x in -2..=9 {
        for y in -6..=-1 {
            for z in -2..=3 {
                sections.push([x, y, z]);
            }
        }
    }
    Scenario {
        ops,
        queries,
        sections,
        chunk: ChunkPos::new(0, 0),
    }
}

/// 启动官方程序，把题目写进 stdin，收回 stdout 的 JSON 行。
fn run_official(mut command: Command, scenario: &Scenario) -> Result<Vec<Value>, String> {
    let request = json!({
        "ops": scenario.ops,
        "queries": scenario.queries,
        "sections": scenario.sections,
        "chunk": {"x": scenario.chunk.x, "z": scenario.chunk.z},
    });
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run java: {e}"))?;
    {
        let stdin = child.stdin.as_mut().ok_or("java stdin is closed")?;
        writeln!(stdin, "{request}").map_err(|e| format!("cannot write request: {e}"))?;
    }
    let mut lines = Vec::new();
    for line in BufReader::new(child.stdout.take().ok_or("java stdout is closed")?).lines() {
        let line = line.map_err(|e| format!("bad java output: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        lines.push(
            serde_json::from_str(&line).map_err(|e| format!("bad oracle line: {e}: {line}"))?,
        );
    }
    let status = child
        .wait()
        .map_err(|e| format!("cannot wait for java: {e}"))?;
    if !status.success() {
        return Err(format!("PoiOracle.java failed: {status}"));
    }
    Ok(lines)
}

/// 把 `Tag` 按官方 oracle 的 canonical() 规则转成 JSON：复合标签按键排序，
/// 数组 / 记录顺序保留（顺序本身要对拍），Byte / Int 都写成数字。
fn canonical(tag: &Tag) -> Value {
    match tag {
        Tag::Compound(entries) => {
            let mut sorted: Vec<(&String, Value)> = entries
                .iter()
                .map(|(name, value)| (name, canonical(value)))
                .collect();
            sorted.sort_by(|a, b| a.0.cmp(b.0));
            let mut out = serde_json::Map::new();
            for (name, value) in sorted {
                out.insert(name.clone(), value);
            }
            Value::Object(out)
        }
        Tag::List(values) => Value::Array(values.iter().map(canonical).collect()),
        Tag::IntArray(values) => Value::Array(values.iter().map(|&v| Value::from(v)).collect()),
        Tag::String(value) => Value::String(value.clone()),
        Tag::Byte(value) => Value::from(*value),
        Tag::Short(value) => Value::from(*value),
        Tag::Int(value) => Value::from(*value),
        Tag::Long(value) => Value::from(*value),
        Tag::Float(value) => Value::from(*value),
        Tag::Double(value) => Value::from(*value),
        _ => Value::Null,
    }
}

/// Rust 侧重放一步操作。三处改坏自检都在这里注入：
/// `tickets` 让新记录从 0 张票开始，`meeting` 跳过钟的登记，
/// `remove` 让拆方块只 add 不 remove。
fn apply_op(store: &mut PoiStore, op: &Value, sabotage: Option<&str>) -> Value {
    if let Some(at) = op.get("at") {
        let at = json_pos(at);
        let old = op["old"].as_u64().unwrap_or(0) as u32;
        let new = op["new"].as_u64().unwrap_or(0) as u32;
        let before = poi::for_state(old);
        let after = poi::for_state(new);
        if sabotage == Some("meeting") && after == poi::poi_type_id("meeting") {
            return Value::Null;
        }
        if before == after {
            return Value::Null;
        }
        if sabotage != Some("remove") {
            store.remove(at);
        }
        if let Some(kind) = after {
            store.add(at, kind);
            if sabotage == Some("tickets") {
                // 故意把 `free_tickets` 初值写成 0（官方是 maxTickets）。
                if let Some(section) = store.section_mut(SectionPos::of(at)) {
                    if let Some(record) = section.get_mut(at) {
                        record.free_tickets = 0;
                    }
                }
            }
        }
        Value::Null
    } else if let Some(take) = op.get("take") {
        let center = json_pos(take);
        let radius = op["radius"].as_i64().unwrap_or(0) as i32;
        let wanted = op["poi"]
            .as_str()
            .and_then(poi::poi_type_id)
            .unwrap_or(u8::MAX);
        match store.take(|kind| kind == wanted, |_, _| true, center, radius) {
            Some(at) => pos(at.x, at.y, at.z),
            None => Value::Null,
        }
    } else if let Some(release) = op.get("release") {
        store.release(json_pos(release));
        Value::Null
    } else {
        Value::Null
    }
}

fn json_pos(value: &Value) -> BlockPos {
    BlockPos::new(
        value[0].as_i64().unwrap_or(0) as i32,
        value[1].as_i64().unwrap_or(0) as i32,
        value[2].as_i64().unwrap_or(0) as i32,
    )
}

fn occupancy(name: &str) -> Occupancy {
    match name {
        "HAS_SPACE" => Occupancy::HasSpace,
        "IS_OCCUPIED" => Occupancy::IsOccupied,
        _ => Occupancy::Any,
    }
}

fn answer_query(store: &PoiStore, query: &Value) -> Value {
    let kind = query["kind"].as_str().unwrap_or("");
    let name = query["poi"].as_str().unwrap_or("");
    let kind_id = poi::poi_type_id(name).unwrap_or(u8::MAX);
    let predicate = move |found: u8| found == kind_id;
    let center = json_pos(&query["center"]);
    let radius = query["radius"].as_i64().unwrap_or(0) as i32;
    let occupancy = occupancy(query["occupancy"].as_str().unwrap_or("ANY"));
    match kind {
        "exists" => Value::Bool(store.exists(json_pos(&query["at"]), predicate)),
        "getType" => match store.get_type(json_pos(&query["at"])) {
            Some(kind) => Value::String(format!("minecraft:{}", type_name(kind).unwrap_or("?"))),
            None => Value::Null,
        },
        "freeTickets" => Value::from(store.free_tickets(json_pos(&query["at"]))),
        "countInRange" => {
            Value::from(store.get_count_in_range(predicate, center, radius, occupancy) as u64)
        }
        "getInSquare" | "getInRange" => {
            let records = if kind == "getInSquare" {
                store.get_in_square(predicate, center, radius, occupancy)
            } else {
                store.get_in_range(predicate, center, radius, occupancy)
            };
            Value::Array(
                records
                    .iter()
                    .map(|record| {
                        json!([
                            format!("minecraft:{}", type_name(record.kind).unwrap_or("?")),
                            pos(record.pos.x, record.pos.y, record.pos.z),
                            record.free_tickets,
                        ])
                    })
                    .collect(),
            )
        }
        "findAllClosestFirstWithType" => Value::Array(
            store
                .find_all_closest_first_with_type(predicate, |_| true, center, radius, occupancy)
                .iter()
                .map(|&(kind, at)| {
                    json!([
                        format!("minecraft:{}", type_name(kind).unwrap_or("?")),
                        pos(at.x, at.y, at.z),
                    ])
                })
                .collect(),
        ),
        "findClosest" => match store.find_closest(predicate, |_| true, center, radius, occupancy) {
            Some(at) => pos(at.x, at.y, at.z),
            None => Value::Null,
        },
        "findClosestWithType" => {
            match store.find_closest_with_type(predicate, center, radius, occupancy) {
                Some((kind, at)) => json!([
                    format!("minecraft:{}", type_name(kind).unwrap_or("?")),
                    pos(at.x, at.y, at.z),
                ]),
                None => Value::Null,
            }
        }
        _ => Value::Null,
    }
}

/// 从 Bamboo 的 `PoiStore` 编出官方 `SectionStorage.writeChunk` 的根标签。
fn encode_store(store: &PoiStore, chunk: ChunkPos) -> Tag {
    let sections = store.column_sections(chunk);
    poi_nbt::encode(&sections)
}

/// 解码再编码：每段按 `PoiSection$Packed.CODEC` 往返后的内容。
fn roundtrip_sections(tag: &Tag) -> Value {
    let decoded = match poi_nbt::decode(tag) {
        Ok(chunk) => chunk,
        Err(_) => return Value::Null,
    };
    let mut out = serde_json::Map::new();
    for (y, section) in decoded.sections {
        let encoded = poi_nbt::encode(&[(y, &section)]);
        if let Tag::Compound(entries) = &encoded
            && let Some((_, Tag::Compound(sections))) =
                entries.iter().find(|(name, _)| name == "Sections")
            && let Some((_, packed)) = sections.iter().find(|(name, _)| name == &y.to_string())
        {
            out.insert(y.to_string(), canonical(packed));
        }
    }
    Value::Object(out)
}

pub fn run(command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let scenario = build_scenario();
    let lines = run_official(command, &scenario)?;
    let sabotage = options.break_self_test.as_deref();
    let mut report = Report::default();
    let mut store = PoiStore::new();
    // Java 侧在第 step 步之后先跑一遍查询（step 0 = 没动过）。
    let mut step = 0usize;
    let mut query_index = 0usize;
    let mut take_results: Vec<Value> = Vec::new();
    for line in lines {
        let kind = line["type"].as_str().unwrap_or("");
        match kind {
            "query" => {
                let expected_step = line["step"].as_u64().unwrap_or(0) as usize;
                while step < expected_step {
                    let result = apply_op(&mut store, &scenario.ops[step], sabotage);
                    if scenario.ops[step].get("take").is_some() {
                        take_results.push(result);
                    }
                    step += 1;
                    query_index = 0;
                }
                let q = line["query"].as_u64().unwrap_or(0) as usize;
                let query = &scenario.queries[q];
                let ours = answer_query(&store, query);
                report.check(
                    &ours,
                    &line["value"],
                    format!("step {step} query {q} ({})", query["kind"]),
                );
                query_index += 1;
            }
            "takes" => {
                report.check(
                    &Value::Array(take_results.clone()),
                    &line["values"],
                    "take() choices",
                );
            }
            "save" => {
                while step < scenario.ops.len() {
                    let result = apply_op(&mut store, &scenario.ops[step], sabotage);
                    if scenario.ops[step].get("take").is_some() {
                        take_results.push(result);
                    }
                    step += 1;
                }
                let tag = encode_store(&store, scenario.chunk);
                report.check(&canonical(&tag), &line["nbt"], "poi/r.0.0.mca root tag");
            }
            "roundtrip" => {
                let tag = encode_store(&store, scenario.chunk);
                report.check(
                    &roundtrip_sections(&tag),
                    &line["sections"],
                    "NBT round trip per section",
                );
            }
            "village" => {
                let x = line["x"].as_i64().unwrap_or(0) as i32;
                let y = line["y"].as_i64().unwrap_or(0) as i32;
                let z = line["z"].as_i64().unwrap_or(0) as i32;
                let ours = store.sections_to_village(SectionPos::new(x, y, z));
                report.check(
                    &Value::from(ours),
                    &line["level"],
                    format!("sectionsToVillage({x},{y},{z})"),
                );
            }
            other => return Err(format!("unexpected oracle line type {other}")),
        }
    }
    let _ = query_index;
    if report.checks == 0 {
        return Err("poi-oracle compared no cases".to_owned());
    }
    // 确认题目里既有可达段也有不可达段，避免两边一起给 7 却没测到扩散。
    if sabotage.is_none() {
        let mut levels = std::collections::BTreeSet::new();
        for &[x, y, z] in &scenario.sections {
            levels.insert(store.sections_to_village(SectionPos::new(x, y, z)));
        }
        if !levels.contains(&NO_VILLAGE_LEVEL) || !levels.iter().any(|&v| v < NO_VILLAGE_LEVEL) {
            return Err(format!(
                "the village-distance scenario has no contrast: {levels:?}"
            ));
        }
    }
    report.finish(&options)
}
