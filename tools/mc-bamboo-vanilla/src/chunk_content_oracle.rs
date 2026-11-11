//! 用官方 26.3 的类检查区块上层内容的存档 NBT。
//!
//! `ChunkContentOracle.java` 在 JVM 里直接调官方的 `BlockEntity.loadStatic` /
//! `saveWithFullMetadata`、`SavedTick.codec` / `LevelChunkTicks` 和
//! `EntityType.loadEntityRecursive` / `Entity.save`，不启动服务端、不接受 EULA。
//! Rust 侧用 `mc-bamboo-blocks::archive` 和 `mc-bamboo-storage::chunk_content`
//! 重放，按键比较规范 JSON（官方 `CompoundTag` 用 `HashMap`，键序是哈希序）。
//!
//! 命令：`cargo run -p mc-bamboo-vanilla -- chunk-content <java 路径> [--show N]`。

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use mc_bamboo_blocks::{
    BlockState, ChunkTicks,
    archive::{
        entity::{entity_uuid_tag, read_entity, write_entity},
        ticks::{decode_chunk_ticks, encode_chunk_ticks},
    },
    entity::{Entity, EntityKind, FallingBlock, GroundItem, PrimedTnt},
};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::{blocks, items::STONE, version};
use mc_bamboo_nbt::{Accounter, Tag, read_named};
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_storage::chunk_content::{EntityFile, split_entity_entries};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::oracle::collect_log;

const USAGE: &str = "usage: chunk-content <java> [--show N]";
const DEFAULT_SHOW: usize = 5;

#[derive(Debug, Deserialize)]
struct Response {
    #[allow(dead_code, reason = "命令名是对拍协议的一部分")]
    command: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    protocol: Option<i32>,
    #[serde(default)]
    #[serde(rename = "dataVersion")]
    data_version: Option<i32>,
    #[serde(default)]
    cases: Vec<Case>,
    #[serde(default)]
    original: Option<Value>,
    #[serde(default)]
    written: Option<Value>,
    #[serde(default)]
    #[serde(rename = "blockEntities")]
    block_entities: Vec<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct Case {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    original: Option<Value>,
    #[serde(default)]
    written: Option<Value>,
    #[serde(default)]
    canonical: Option<Value>,
    #[serde(default)]
    repacked: Option<Value>,
    #[serde(default)]
    encoded: Option<String>,
}

#[derive(Debug, Default)]
struct Report {
    checks: usize,
    failures: Vec<String>,
    show: usize,
}

impl Report {
    fn check(&mut self, condition: bool, what: impl Into<String>) {
        self.checks += 1;
        if !condition {
            self.failures.push(what.into());
        }
    }

    fn finish(&self) -> Result<(), String> {
        for failure in self.failures.iter().take(self.show.max(1)) {
            eprintln!("  {failure}");
        }
        if self.failures.is_empty() {
            println!("chunk-content oracle: {} checks passed", self.checks);
            Ok(())
        } else {
            Err(format!(
                "chunk-content oracle: {} of {} checks failed",
                self.failures.len(),
                self.checks
            ))
        }
    }
}

/// 启动官方进程，跑完所有题目。
pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let show = parse_show(args)?;
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let logs = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut stdin = child.stdin.take().ok_or("java has no stdin")?;
    let mut lines = BufReader::new(stdout).lines();
    let mut report = Report {
        show,
        ..Default::default()
    };
    let result = run_cases(&mut stdin, &mut lines, &mut report);
    drop(stdin);
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let logs = logs.recv().unwrap_or_default().join("\n");
    if let Err(error) = &result {
        return Err(format!("{error}\n{logs}"));
    }
    if !status.success() {
        return Err(format!("ChunkContentOracle.java failed: {status}\n{logs}"));
    }
    report.finish()
}

fn parse_show(args: &[&str]) -> Result<usize, String> {
    match args {
        [] => Ok(DEFAULT_SHOW),
        ["--show", value] => value.parse().map_err(|_| USAGE.to_owned()),
        _ => Err(USAGE.to_owned()),
    }
}

fn ask(
    stdin: &mut impl Write,
    lines: &mut impl Iterator<Item = Result<String, std::io::Error>>,
    request: Value,
) -> Result<Response, String> {
    let mut line = serde_json::to_string(&request).map_err(|error| error.to_string())?;
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .map_err(|error| format!("cannot write to java: {error}"))?;
    stdin
        .flush()
        .map_err(|error| format!("cannot flush java stdin: {error}"))?;
    let answer = lines
        .next()
        .ok_or("ChunkContentOracle.java closed stdout unexpectedly")?
        .map_err(|error| format!("cannot read from java: {error}"))?;
    let response: Response =
        serde_json::from_str(&answer).map_err(|error| format!("bad oracle response: {error}"))?;
    if let Some(error) = &response.error {
        return Err(format!("ChunkContentOracle.java: {error}"));
    }
    Ok(response)
}

fn run_cases(
    stdin: &mut impl Write,
    lines: &mut impl Iterator<Item = Result<String, std::io::Error>>,
    report: &mut Report,
) -> Result<(), String> {
    // 1. 版本头。
    let header = ask(stdin, lines, json!({"command": "header"}))?;
    report.check(
        header.version.as_deref() == Some(version::GAME_VERSION),
        "official game version differs from Bamboo",
    );
    report.check(
        header.protocol == Some(version::PROTOCOL_VERSION),
        "official protocol version differs from Bamboo",
    );
    report.check(
        header.data_version == Some(version::DATA_VERSION),
        "official DataVersion differs from Bamboo",
    );

    // 2. 方块实体逐类型：官方 loadStatic → saveWithFullMetadata 按键不变，
    //    并检查 Bamboo 必写的 components / keepPacked 都在官方写出的键里。
    let answer = ask(stdin, lines, json!({"command": "block-entities"}))?;
    report.check(
        answer.cases.len() >= 13,
        "official sent too few block entity cases",
    );
    for case in &answer.cases {
        let name = case.name.as_deref().unwrap_or("?");
        report.check(
            case.error.is_none(),
            format!("{name}: {}", case.error.as_deref().unwrap_or("")),
        );
        report.check(
            case.original.is_some() && case.original == case.written,
            format!("{name}: official load -> save changed the NBT"),
        );
        if let Some(value) = &case.written {
            let keys = value["values"].as_object();
            report.check(
                keys.is_some_and(|keys| keys.contains_key("components")),
                format!("{name}: official does not write components"),
            );
            report.check(
                keys.is_some_and(|keys| keys.contains_key("keepPacked")),
                format!("{name}: official does not write keepPacked"),
            );
        }
    }

    // 3. SavedTick：官方 codec 写出的 NBT，Bamboo 能解回同样的键，再编回去按键一致；
    //    官方 LevelChunkTicks.unpack -> pack 后仍是同一批刻。
    let answer = ask(stdin, lines, json!({"command": "ticks"}))?;
    report.check(
        answer.cases.len() == 2,
        "official sent the wrong number of tick cases",
    );
    for case in &answer.cases {
        let name = case.name.as_deref().unwrap_or("?");
        report.check(
            case.error.is_none(),
            format!("{name}: {}", case.error.as_deref().unwrap_or("")),
        );
        let Some(canonical) = &case.canonical else {
            continue;
        };
        let Some(ticks) = canonical_from_json(&canonical["values"]["Ticks"]) else {
            report.check(
                false,
                format!("{name}: official tick list is not valid NBT"),
            );
            continue;
        };
        let empty = Tag::List(Vec::new());
        let (blocks, fluids) = if name == "blocks" {
            (&ticks, &empty)
        } else {
            (&empty, &ticks)
        };
        let (decoded, problems) =
            decode_chunk_ticks(blocks, fluids).map_err(|error| error.to_string())?;
        report.check(
            problems.is_empty(),
            format!("{name}: Bamboo reported {problems:?}"),
        );
        let lists = encode_chunk_ticks(&decoded).map_err(|error| error.to_string())?;
        let rebuilt = if name == "blocks" {
            Tag::List(lists.blocks)
        } else {
            Tag::List(lists.fluids)
        };
        report.check(
            Some(canonical_tag(&rebuilt)) == Some(canonical["values"]["Ticks"].clone()),
            format!("{name}: Bamboo decode -> encode differs from the official list"),
        );
        report.check(
            case.repacked.as_ref() == Some(&canonical["values"]["Ticks"]),
            format!("{name}: official unpack -> pack changed the list"),
        );
        // 空列表和满列表、到期/没到期混在一起都由官方这一批题目覆盖。
        let _ = case.encoded.as_ref();
    }

    // 4. 实体：Bamboo 写出三种实体（含官方默认值、极端值、带组件的物品），
    //    官方 FakeLevel 真的 load 再 save，按键比较。
    let entities = entity_cases();
    let request = json!({"command": "entities", "entities": entities.iter().map(canonical_tag).collect::<Vec<_>>()});
    let answer = ask(stdin, lines, request)?;
    report.check(
        answer.cases.len() == entities.len(),
        "official sent the wrong number of entity cases",
    );
    for (index, case) in answer.cases.iter().enumerate() {
        let label = case.index.unwrap_or(index);
        report.check(
            case.error.is_none(),
            format!("entity {label}: {}", case.error.as_deref().unwrap_or("")),
        );
        report.check(
            case.original.is_some() && case.original == case.written,
            format!("entity {label}: official load -> save changed the NBT"),
        );
        // Bamboo 也能读回官方写出的 NBT；读回时 uid 重新分配，UUID 不比。
        if let Some(written) = &case.written {
            let Some(tag) = canonical_from_json(written) else {
                continue;
            };
            let rebuilt = read_entity(&tag, 99).map_err(|error| error.to_string())?;
            report.check(
                rebuilt.is_some(),
                format!("entity {label}: Bamboo cannot read the official NBT"),
            );
        }
    }

    // 5. 实体文件的根与不认识的类型：Bamboo 纯函数往返。
    let mut file = EntityFile::empty(ChunkPos::new(3, -9));
    file.entities = entities.iter().take(3).cloned().collect();
    file.preserved_entities = vec![Tag::Compound(vec![
        ("id".to_owned(), Tag::String("minecraft:cow".to_owned())),
        ("Health".to_owned(), Tag::Float(10.0)),
    ])];
    file.preserved
        .push(("ModData".to_owned(), Tag::String("keep".to_owned())));
    let root = file.encode().expect("非空实体文件");
    let decoded =
        EntityFile::decode(&root, ChunkPos::new(3, -9)).map_err(|error| error.to_string())?;
    report.check(
        decoded.encode() == Some(root),
        "entity file round trip changed the NBT",
    );
    report.check(
        decoded.preserved_entities.len() == 1,
        "unknown entity types were not preserved",
    );
    let (modeled, preserved) = split_entity_entries(decoded.entities.clone());
    report.check(
        modeled.len() == 3 && preserved.is_empty(),
        "entity split lost modeled entities",
    );

    // 6. 官方区块 NBT：Bamboo 写的 `components`/`keepPacked` 能被
    //    `SerializableChunkData.parse -> write` 原样带回。
    let chunk = sample_chunk_nbt();
    let answer = ask(
        stdin,
        lines,
        json!({"command": "chunk-read", "nbt": encode_b64(&chunk)}),
    )?;
    report.check(
        answer.original.is_some() && answer.original == answer.written,
        "official chunk parse -> write changed the NBT",
    );
    for keys in &answer.block_entities {
        report.check(
            keys.iter().any(|key| key == "components"),
            "official chunk lost components",
        );
        report.check(
            keys.iter().any(|key| key == "keepPacked"),
            "official chunk lost keepPacked",
        );
    }
    Ok(())
}

fn entity_cases() -> Vec<Tag> {
    let base = |kind: EntityKind, id: i32, position: Vec3| {
        let mut entity = Entity::new(kind, position);
        entity.id = id;
        entity.delta = Vec3::new(0.1, -0.2, 0.3);
        entity.on_ground = true;
        entity.fall_distance = 4.5;
        entity.remaining_fire_ticks = 12;
        entity.split()
    };
    let mut out = Vec::new();
    let pos = Vec3::new(1.5, 64.25, -2.0);
    // 空容器 / 满容器、带组件的物品、极端值。
    let mut with_component = ItemStack::new(STONE, 7);
    let mut patch = mc_bamboo_protocol::components::ComponentPatch::new();
    patch.put(
        mc_bamboo_data::registries::data_component_type::CUSTOM_NAME,
        mc_bamboo_protocol::components::PatchEntry::Set(
            mc_bamboo_protocol::components::ComponentValue::CustomName(
                mc_bamboo_protocol::text::Component::text("竹"),
            ),
        ),
    );
    let (added, removed) = patch.to_opaque();
    with_component.added = added;
    with_component.removed = removed;
    for (index, stack) in [
        ItemStack::new(STONE, 1),
        with_component,
        ItemStack::new(STONE, 3),
    ]
    .into_iter()
    .enumerate()
    {
        let mut item = GroundItem::new(stack, false);
        item.age = [0, 123, 6000][index];
        item.pickup_delay = [0, 10, 32767][index];
        item.health = [5, 1, i16::MAX as i32][index];
        out.push(write_entity(&base(EntityKind::Item(item), 17 + index as i32, pos)).unwrap());
    }
    let mut falling = FallingBlock::new(BlockState(blocks::SAND), BlockPos::new(1, 64, -3));
    falling.time = 99;
    falling.drop_item = false;
    falling.hurt_entities = true;
    falling.fall_damage_per_distance = 2.5;
    falling.fall_damage_max = 40;
    falling.cancel_drop = true;
    out.push(write_entity(&base(EntityKind::FallingBlock(falling), 21, pos)).unwrap());
    for fuse in [0, 80, i16::MAX as i32] {
        out.push(write_entity(&base(EntityKind::Tnt(PrimedTnt { fuse }), 22, pos)).unwrap());
    }
    // 官方 UUIDUtil.CODEC 会不会把我们的 UUID 原样带回。
    let uuid = entity_uuid_tag(23);
    let mut with_uuid = match &out[0] {
        Tag::Compound(values) => values.clone(),
        _ => unreachable!("write_entity returns a compound"),
    };
    with_uuid.retain(|(key, _)| key != "UUID");
    with_uuid.push(("UUID".to_owned(), uuid));
    out.push(Tag::Compound(with_uuid));
    out
}

/// 一段最小合法的区块 NBT，只为检查 `block_entities` 的键能原样通过官方 parse/write。
fn sample_chunk_nbt() -> Vec<u8> {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/official-chunk.nbt"
    ))
    .expect("官方区块样例存在");
    let (_, entries) = read_named(&bytes, Accounter::unlimited()).expect("官方区块样例能读");
    let mut entries = entries;
    entries.retain(|(key, _)| key != "block_entities");
    let entity = Tag::Compound(vec![
        ("id".to_owned(), Tag::String("minecraft:chest".to_owned())),
        ("x".to_owned(), Tag::Int(0)),
        ("y".to_owned(), Tag::Int(-60)),
        ("z".to_owned(), Tag::Int(0)),
        ("components".to_owned(), Tag::Compound(Vec::new())),
        ("keepPacked".to_owned(), Tag::Byte(0)),
        ("Items".to_owned(), Tag::List(Vec::new())),
    ]);
    entries.push(("block_entities".to_owned(), Tag::List(vec![entity])));
    let mut out = Vec::new();
    Tag::Compound(entries).write_named("", &mut out);
    out
}

/// 按 NBT 类型还原官方 canonical JSON，复合标签按键排序（serde_json::Map 默认 BTreeMap）。
fn canonical_tag(tag: &Tag) -> Value {
    match tag {
        Tag::Byte(value) => json!({"type": 1, "value": value.to_string()}),
        Tag::Short(value) => json!({"type": 2, "value": value.to_string()}),
        Tag::Int(value) => json!({"type": 3, "value": value.to_string()}),
        Tag::Long(value) => json!({"type": 4, "value": value.to_string()}),
        Tag::Float(value) => json!({"type": 5, "value": value.to_string()}),
        Tag::Double(value) => json!({"type": 6, "value": value.to_string()}),
        Tag::String(value) => json!({"type": 8, "value": value}),
        Tag::ByteArray(values) => json!({"type": 7, "values": values}),
        Tag::IntArray(values) => json!({"type": 11, "values": values}),
        Tag::LongArray(values) => json!({"type": 12, "values": values}),
        Tag::List(values) => json!({
            "type": 9,
            "values": values.iter().map(canonical_tag).collect::<Vec<_>>()
        }),
        Tag::Compound(entries) => {
            let mut map = serde_json::Map::new();
            for (key, value) in entries {
                map.insert(key.clone(), canonical_tag(value));
            }
            json!({"type": 10, "values": Value::Object(map)})
        }
    }
}

fn canonical_from_json(value: &Value) -> Option<Tag> {
    let id = value.get("type")?.as_i64()? as i32;
    let number = |key: &str| value.get(key)?.as_str()?.parse::<f64>().ok();
    Some(match id {
        1 => Tag::Byte(number("value")? as i8),
        2 => Tag::Short(number("value")? as i16),
        3 => Tag::Int(number("value")? as i32),
        4 => Tag::Long(number("value")? as i64),
        5 => Tag::Float(number("value")? as f32),
        6 => Tag::Double(number("value")?),
        8 => Tag::String(value.get("value")?.as_str()?.to_owned()),
        7 => Tag::ByteArray(
            value
                .get("values")?
                .as_array()?
                .iter()
                .map(|value| value.as_i64().unwrap_or(0) as i8)
                .collect(),
        ),
        11 => Tag::IntArray(
            value
                .get("values")?
                .as_array()?
                .iter()
                .map(|value| value.as_i64().unwrap_or(0) as i32)
                .collect(),
        ),
        12 => Tag::LongArray(
            value
                .get("values")?
                .as_array()?
                .iter()
                .map(|value| value.as_i64().unwrap_or(0))
                .collect(),
        ),
        9 => Tag::List(
            value
                .get("values")?
                .as_array()?
                .iter()
                .map(canonical_from_json)
                .collect::<Option<Vec<_>>>()?,
        ),
        10 => Tag::Compound(
            value
                .get("values")?
                .as_object()?
                .iter()
                .map(|(key, value)| Some((key.clone(), canonical_from_json(value)?)))
                .collect::<Option<Vec<_>>>()?,
        ),
        _ => return None,
    })
}

fn encode_b64(bytes: &[u8]) -> String {
    crate::storage_oracle::encode_b64(bytes)
}

#[allow(dead_code, reason = "展示时统计用")]
fn _unused(_map: &BTreeMap<String, usize>, _ticks: &ChunkTicks) {}
