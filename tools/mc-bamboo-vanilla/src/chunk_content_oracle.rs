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
        entity::{entity_uuid_tag, read_entity, read_entity_tree, write_entity, write_entity_tree},
        ticks::{decode_chunk_ticks, encode_chunk_ticks},
    },
    entity::{Entity, EntityKind, FallingBlock, GroundItem, PrimedTnt, arrow::Pickup},
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
    #[serde(default)]
    #[serde(rename = "writtenNbt")]
    written_nbt: Option<String>,
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
    if !report.failures.is_empty() && !logs.is_empty() {
        eprintln!("{logs}");
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
        answer.cases.len() >= 14,
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

    // 2b. 边界状态：官方 loadStatic 读带边界值的 NBT，再写回来；Bamboo 的
    //     create_block_entity 读同一份 NBT 能造出具体类型，写出的键在官方那一份里都出现。
    let load_cases = block_entity_load_cases()?;
    let answer = ask(
        stdin,
        lines,
        json!({"command": "block-entities-load", "entities": load_cases.iter().map(|case| {
            json!({"name": case.0, "state": case.1, "tag": canonical_tag(&case.2)})
        }).collect::<Vec<_>>()}),
    )?;
    report.check(
        answer.cases.len() == load_cases.len(),
        "official sent the wrong number of load cases",
    );
    for (index, case) in answer.cases.iter().enumerate() {
        let name = &load_cases[index].0;
        report.check(
            case.error.is_none(),
            format!("{name}: {}", case.error.as_deref().unwrap_or("")),
        );
        // 容器/战利品表由游戏层第二段负责；方块层这一段的工厂只认会动的六种。
        // 振动事件的注册表校验在 `passive.rs`，用官方 load → save 检查这两题。
        if name.starts_with("chest-") || name.starts_with("sculk-sensor-") {
            continue;
        }
        let rebuilt = mc_bamboo_blocks::archive::create_block_entity(
            match name.as_str() {
                "hopper-negative" | "hopper-large" => "minecraft:hopper",
                _ if name.starts_with("chest-") => "minecraft:chest",
                "furnace-mid" => "minecraft:furnace",
                "brewing-mid" => "minecraft:brewing_stand",
                "campfire-mid" => "minecraft:campfire",
                "piston-mid" => "minecraft:piston",
                _ => load_cases[index].1,
            },
            BlockPos::new(3, 64, 5),
            BlockState(blocks::AIR),
            &load_cases[index].2,
        )
        .map_err(|error| error.to_string())?;
        report.check(
            rebuilt.is_some(),
            format!("{name}: Bamboo has no factory branch"),
        );
        if let (Some(entity), Some(written)) = (rebuilt, &case.written) {
            let keys = mc_bamboo_blocks::archive::write_block_entity_additional(entity.as_ref())
                .map_err(|error| error.to_string())?;
            let official_keys = &written["values"];
            for (key, value) in &keys {
                report.check(
                    official_keys.get(key).is_some(),
                    format!("{name}: official did not write {key}"),
                );
                // 宝库 / 幽匿这四种的键由 codec 完整往返，逐值也要和官方一致。
                // The four passive types own their keys: compare values too.
                if name.starts_with("vault-")
                    || name.starts_with("sculk-")
                    || name.starts_with("calibrated-")
                {
                    report.check(
                        official_keys.get(key) == Some(&canonical_tag(value)),
                        format!(
                            "{name}: Bamboo wrote {key} as {}, official {}",
                            canonical_tag(value),
                            official_keys.get(key).cloned().unwrap_or(Value::Null)
                        ),
                    );
                }
            }
            // 这四种类型的 `saveAdditional` 键都由方块层独占：官方写出的非元数据键
            // 和 Bamboo 写出的键必须完全相同，缺一个也不算对上。
            // These four types own all their `saveAdditional` keys: the official
            // non-metadata keys must match Bamboo's exactly.
            if name.starts_with("vault-")
                || name.starts_with("sculk-")
                || name.starts_with("calibrated-")
            {
                let metadata = ["id", "x", "y", "z", "components", "keepPacked"];
                let mut official_keys: Vec<&str> = written["values"]
                    .as_object()
                    .map(|map| {
                        map.keys()
                            .map(String::as_str)
                            .filter(|key| !metadata.contains(key))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut bamboo_keys: Vec<&str> = keys.iter().map(|(key, _)| key.as_str()).collect();
                // 官方 `CompoundTag` 是 HashMap，键序不确定：集合相等即可。
                official_keys.sort_unstable();
                bamboo_keys.sort_unstable();
                report.check(
                    official_keys == bamboo_keys,
                    format!("{name}: official keys {official_keys:?}, Bamboo keys {bamboo_keys:?}"),
                );
            }
        }
    }

    // 2d. `VibrationInfo.game_event` 是注册表 holder codec：语法合法但没注册的
    //     名字解不开，官方把整个 event 丢掉（`selector.tick` 也折回 -1L）。
    //     Bamboo 用同一个 `PassiveBlockEntity` 逐值对照官方结果。
    for (index, case) in answer.cases.iter().enumerate() {
        let name = &load_cases[index].0;
        if !name.starts_with("sculk-sensor-") {
            continue;
        }
        report.check(
            case.error.is_none(),
            format!("{name}: {}", case.error.as_deref().unwrap_or("")),
        );
        let rebuilt = mc_bamboo_blocks::archive::create_block_entity(
            "minecraft:sculk_sensor",
            BlockPos::new(3, 64, 5),
            BlockState(blocks::AIR),
            &load_cases[index].2,
        )
        .map_err(|error| error.to_string())?
        .expect("sensor factory");
        let keys = mc_bamboo_blocks::archive::write_block_entity_additional(rebuilt.as_ref())
            .map_err(|error| error.to_string())?;
        let official = case.written.as_ref().map(|v| &v["values"]);
        let bamboo = canonical_tag(&Tag::Compound(keys))["values"].clone();
        if let Some(official) = official {
            for (key, value) in bamboo.as_object().unwrap() {
                report.check(
                    official.get(key) == Some(value),
                    format!(
                        "{name}: Bamboo wrote {key} as {value}, official {:?}",
                        official.get(key)
                    ),
                );
            }
            let metadata = ["id", "x", "y", "z", "components", "keepPacked"];
            let mut official_keys: Vec<&str> = official
                .as_object()
                .map(|map| {
                    map.keys()
                        .map(String::as_str)
                        .filter(|key| !metadata.contains(key))
                        .collect()
                })
                .unwrap_or_default();
            let mut bamboo_keys: Vec<&str> = bamboo
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            official_keys.sort_unstable();
            bamboo_keys.sort_unstable();
            report.check(
                official_keys == bamboo_keys,
                format!("{name}: official keys {official_keys:?}, Bamboo keys {bamboo_keys:?}"),
            );
        }
    }

    // 2c. 只保留存档态的方块实体：官方 loadStatic → saveWithFullMetadata 与
    //     Bamboo 正式编码路径共用的 `resave_additional` 逐键逐值对照。
    //     裸条目 + 富字段两种边界题；未知键不进题目（D9 单测另行固定）。
    let resave_cases = resave_load_cases();
    let answer = ask(
        stdin,
        lines,
        json!({"command": "block-entities-load", "entities": resave_cases.iter().map(|case| {
            json!({"name": case.0, "state": case.1, "tag": canonical_tag(&case.3)})
        }).collect::<Vec<_>>()}),
    )?;
    report.check(
        answer.cases.len() == resave_cases.len(),
        "official sent the wrong number of resave cases",
    );
    for (index, case) in answer.cases.iter().enumerate() {
        let (name, _, id, tag) = &resave_cases[index];
        report.check(
            case.error.is_none(),
            format!("{name}: {}", case.error.as_deref().unwrap_or("")),
        );
        let Some(written) = &case.written else {
            continue;
        };
        let state = match mc_bamboo_data::block_by_name(resave_cases[index].1) {
            Some(block) => BlockState(block.default_state),
            None => BlockState::AIR,
        };
        let saved = mc_bamboo_blocks::archive::resave_additional(id, state, tag)
            .unwrap_or_else(|| panic!("{name}: Bamboo has no resave branch"));
        let official = &written["values"];
        let metadata = ["id", "x", "y", "z", "components", "keepPacked"];
        for (key, value) in &saved.fields {
            report.check(
                official.get(key) == Some(&canonical_tag(value)),
                format!(
                    "{name}: Bamboo wrote {key} as {}, official {}",
                    canonical_tag(value),
                    official.get(key).cloned().unwrap_or(Value::Null)
                ),
            );
        }
        // 官方写出的非元数据键必须就是 Bamboo 写出的键（可省键按官方 omit）。
        let mut official_keys: Vec<&str> = official
            .as_object()
            .map(|map| {
                map.keys()
                    .map(String::as_str)
                    .filter(|key| !metadata.contains(key))
                    .collect()
            })
            .unwrap_or_default();
        let mut bamboo_keys: Vec<&str> = saved.fields.iter().map(|(key, _)| key.as_str()).collect();
        official_keys.sort_unstable();
        bamboo_keys.sort_unstable();
        report.check(
            official_keys == bamboo_keys,
            format!("{name}: official keys {official_keys:?}, Bamboo keys {bamboo_keys:?}"),
        );
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

    // 4. 实体：Bamboo 写出实体 NBT，官方 FakeLevel 加载并重存，按键比较。
    // 4. Entities: write NBT in Bamboo, then load and resave in the official
    // FakeLevel and compare by key.
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
        // 僵尸村民的官方 loader 会补/删 `attributes` / `active_effects` 的懒实例
        // 与空表；只比其它键，僵尸村民的 7 个自有键仍逐键比到。
        // The official loader adds / drops lazy attribute instances and empty
        // effect lists; compare the rest, including all seven own keys.
        let is_zombie_villager = entities.get(label).is_some_and(|tag| {
            matches!(tag, Tag::Compound(fields) if fields.iter().any(|(key, value)| {
                key == "id" && matches!(value, Tag::String(id) if id == "minecraft:zombie_villager")
            }))
        });
        // 官方 `AttributeMap` 按哈希表顺序写 `attributes`，Bamboo 按注册表 ID；
        // 两边都按属性名排序再比。僵尸村民的官方 loader 还会补/删懒实例。
        let is_sulfur_cube = entities.get(label).is_some_and(|tag| {
            matches!(tag, Tag::Compound(fields) if fields.iter().any(|(key, value)| {
                key == "id" && matches!(value, Tag::String(id) if id == "minecraft:sulfur_cube")
            }))
        });
        let comparable = |value: &Value| -> Value {
            let mut sorted = sorted_attributes(value.clone());
            if !is_zombie_villager {
                return sorted;
            }
            let Some(map) = sorted.as_object_mut() else {
                return sorted;
            };
            let Some(inner) = map.get_mut("values").and_then(Value::as_object_mut) else {
                return sorted;
            };
            inner.remove("attributes");
            inner.remove("active_effects");
            let _ = is_sulfur_cube;
            sorted
        };
        let original = case.original.as_ref().map(comparable);
        let written = case.written.as_ref().map(comparable);
        report.check(
            original.is_some() && original == written,
            format!(
                "entity {label}: official load -> save changed the NBT: {}",
                first_difference(
                    original.as_ref().unwrap_or(&Value::Null),
                    written.as_ref().unwrap_or(&Value::Null)
                )
            ),
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
            if matches!(&tag, Tag::Compound(fields) if fields.iter().any(|(key, value)| {
                key == "id"
                    && matches!(
                        value,
                        Tag::String(id)
                            if id == "minecraft:arrow"
                                || id == "minecraft:trident"
                                || id == "minecraft:egg"
                                || id == "minecraft:llama_spit"
                                || id == "minecraft:painting"
                                || id == "minecraft:item_frame"
                                || id == "minecraft:glow_item_frame"
                    )
            })) && let Some(row) = rebuilt
            {
                let saved = write_entity(&row).map_err(|error| error.to_string())?;
                report.check(
                    canonical_tag(&saved) == *written,
                    format!("entity {label}: Bamboo projectile load -> save changed the NBT"),
                );
            }
            // 嵌套 `Passengers`：Bamboo 读树，行数、两边关系和官方格式都对得上。
            if let Tag::Compound(fields) = &tag
                && fields.iter().any(|(key, _)| key == "Passengers")
            {
                let mut next = 1 << 24;
                let tree = read_entity_tree(&tag, &mut next)
                    .map_err(|error| error.to_string())?
                    .expect("jockey root is modelled");
                let root_uid = tree[0].public.uid;
                let passengers = match &tree[0].public.kind {
                    mc_bamboo_blocks::entity::KindPublic::Mob(mob) => mob.passengers.clone(),
                    _ => unreachable!("a jockey's vehicle is a mob"),
                };
                // 一名（鸡骑士）或两名（尸壳骆驼）乘客都要逐位对上。
                // Compare one (chicken jockey) or two (camel husk) passengers.
                report.check(
                    tree.len() == passengers.len() + 1,
                    format!(
                        "entity {label}: Bamboo read {} rows, expected {}",
                        tree.len(),
                        passengers.len() + 1
                    ),
                );
                if tree.len() == passengers.len() + 1 {
                    report.check(
                        tree[0].public.vehicle.is_none()
                            && tree[1..]
                                .iter()
                                .all(|row| row.public.vehicle == Some(root_uid)),
                        format!("entity {label}: Bamboo lost the rider's vehicle link"),
                    );
                    let rider_ids: Vec<_> = tree[1..].iter().map(|row| row.public.uid).collect();
                    report.check(
                        passengers == rider_ids,
                        format!("entity {label}: Bamboo lost the vehicle's passenger list"),
                    );
                    let rows: std::collections::BTreeMap<_, _> = tree
                        .iter()
                        .cloned()
                        .map(|row| (row.public.uid, row))
                        .collect();
                    let saved = write_entity_tree(&rows[&root_uid], &|uid| rows.get(&uid).cloned())
                        .map_err(|error| error.to_string())?;
                    report.check(
                        canonical_tag(&saved) == *written,
                        format!("entity {label}: Bamboo jockey load -> save changed the NBT"),
                    );
                }
            }
        }
    }

    // 5. 实体文件的根与不认识的类型：Bamboo 纯函数往返。
    let mut file = EntityFile::empty(ChunkPos::new(3, -9));
    file.entities = entities.iter().take(3).cloned().collect();
    file.preserved_entities = vec![Tag::Compound(vec![
        ("id".to_owned(), Tag::String("minecraft:pig".to_owned())),
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
    // 牛和经验球现在都是建模实体，读盘后要进实体表跑起来。
    let (modeled_new, preserved_new) = split_entity_entries(vec![
        Tag::Compound(vec![(
            "id".to_owned(),
            Tag::String("minecraft:cow".to_owned()),
        )]),
        Tag::Compound(vec![(
            "id".to_owned(),
            Tag::String("minecraft:experience_orb".to_owned()),
        )]),
        Tag::Compound(vec![(
            "id".to_owned(),
            Tag::String("minecraft:pig".to_owned()),
        )]),
    ]);
    report.check(
        modeled_new.len() == 2 && preserved_new.len() == 1,
        "cow and experience orb are not modeled",
    );

    // 5b. 牛的重存（照方块实体那套）：同一份条目，官方 `loadEntityRecursive -> save`
    //     和 Bamboo `read_entity -> write_entity` 逐键逐值比较；属性列表官方按身份哈希序
    //     遍历，每次开服都可能不同，按 `id` 排好再比。另加「刚生成的」：官方新造一头牛
    //     直接存盘，和 Bamboo 新造的一头逐键比较。
    // Cow resave: the same entries through vanilla load -> save and Bamboo read ->
    // write, key by key. Attribute lists are compared sorted by id.
    let cows = cow_resave_cases();
    let request = json!({"command": "entities", "entities": cows.iter().map(|(_, tag)| canonical_tag(tag)).collect::<Vec<_>>()});
    let answer = ask(stdin, lines, request)?;
    report.check(
        answer.cases.len() == cows.len(),
        "official sent the wrong number of cow cases",
    );
    for ((name, input), case) in cows.iter().zip(&answer.cases) {
        report.check(
            case.error.is_none(),
            format!("cow {name}: {}", case.error.as_deref().unwrap_or("")),
        );
        let Some(written) = &case.written else {
            continue;
        };
        let ours = read_entity(input, 99)
            .map_err(|error| error.to_string())?
            .map(|row| write_entity(&row));
        let Some(Ok(ours)) = ours else {
            report.check(false, format!("cow {name}: Bamboo cannot read or write it"));
            continue;
        };
        let official = sorted_attributes(written.clone());
        let bamboo = sorted_attributes(canonical_tag(&ours));
        report.check(
            official == bamboo,
            format!(
                "cow {name}: Bamboo read -> write differs from the official resave: {}",
                first_difference(&official, &bamboo)
            ),
        );
    }
    use mc_bamboo_blocks::entity::mob::MobType;
    for kind in [
        MobType::Cow,
        MobType::Mooshroom,
        MobType::Pig,
        MobType::Sheep,
        MobType::Chicken,
        MobType::Goat,
        MobType::Frog,
        MobType::Tadpole,
        MobType::PolarBear,
        MobType::Ocelot,
        MobType::Cat,
        MobType::Creeper,
        MobType::Spider,
        MobType::CaveSpider,
        MobType::Enderman,
        MobType::Squid,
        MobType::GlowSquid,
        MobType::Cod,
        MobType::Salmon,
        MobType::TropicalFish,
        MobType::Pufferfish,
        MobType::Witch,
        MobType::Pillager,
        MobType::Sniffer,
        MobType::HappyGhast,
    ] {
        let mut store = mc_bamboo_blocks::EntityStore::new();
        let uid = store.spawn(Entity::by_mob_type(kind, Vec3::new(0.5, 64.0, -3.5)));
        let mut extra = (0_u8, 0_i32);
        store.with_mut(uid, |entity| {
            entity.y_rot = 33.5;
            entity.with_mob(|me, mob| {
                mob.y_head_rot = me.y_rot;
                mob.y_body_rot = me.y_rot;
                if kind == MobType::Sheep {
                    // 浅蓝、已剪毛：非默认的羊毛字节。
                    // Light blue and sheared: a non-default wool byte.
                    mob.wool = 3 | 16;
                }
                extra = (mob.wool, mob.egg_time);
            });
        });
        let row = store.get(uid).expect("刚生成").split();
        let ours = write_entity(&row).map_err(|error| error.to_string())?;
        let uuid = mc_bamboo_blocks::archive::tag::int_array(&ours, "UUID");
        let answer = ask(
            stdin,
            lines,
            json!({"command": "fresh-cow", "type": kind.name(), "wool": extra.0, "egg": extra.1,
                "x": 0.5, "y": 64.0, "z": -3.5, "yaw": 33.5, "uuid": uuid}),
        )?;
        let official = sorted_attributes(answer.written.clone().unwrap_or(Value::Null));
        let bamboo = sorted_attributes(canonical_tag(&ours));
        report.check(
            official == bamboo,
            format!(
                "fresh {}: Bamboo save differs from the official one: {}",
                kind.name(),
                first_difference(&official, &bamboo)
            ),
        );
    }

    // 6. 整块对拍：官方生成（parse -> write）一份区块 NBT，Bamboo 用组装函数
    //    读成 RegionColumn 再写出，和官方对同一份内容读进去再写出来的结果按键比较。
    let whole = sample_chunk_nbt();
    let answer = ask(
        stdin,
        lines,
        json!({"command": "official-chunk", "nbt": encode_b64(&whole)}),
    )?;
    report.check(
        answer.original.is_some() && answer.original == answer.written,
        "official whole-chunk parse -> write changed the NBT",
    );
    let official_bytes = decode_b64(answer.written_nbt.as_deref().unwrap_or_default())?;
    let (_, official_root) =
        read_named(&official_bytes, Accounter::unlimited()).map_err(|error| error.to_string())?;
    let official_tag = Tag::Compound(official_root);
    let decoded =
        mc_bamboo_game::region::chunk_content::decode(ChunkPos::new(3, -9), &official_tag, None)
            .map_err(|error| error.to_string())?;
    report.check(
        decoded.problems.is_empty(),
        format!("whole-chunk decode reported {:?}", decoded.problems),
    );
    let reencoded =
        mc_bamboo_game::region::chunk_content::encode(&decoded.column, ChunkPos::new(3, -9))
            .map_err(|error| error.to_string())?;
    let canonical_official = canonical_tag(&official_tag);
    let canonical_bamboo = canonical_tag(&reencoded.chunk);
    report.check(
        canonical_official == canonical_bamboo,
        format!(
            "Bamboo whole-chunk decode -> encode differs from the official result: {}",
            first_difference(&canonical_official, &canonical_bamboo)
        ),
    );
    report.check(
        reencoded.entities.is_none(),
        "empty entity file should not be written",
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

/// 只保留存档态的方块实体：裸条目 / 富字段边界题。
/// 每项：(题名, 方块状态, 官方 `id`, 读盘条目)。`chunk-content` 对拍用同一份
/// `resave_additional` 逐键逐值对照官方 `loadStatic → saveWithFullMetadata`。
fn resave_load_cases() -> Vec<(String, &'static str, &'static str, Tag)> {
    let meta = |id: &str| {
        vec![
            ("id".to_owned(), Tag::String(id.to_owned())),
            ("x".to_owned(), Tag::Int(3)),
            ("y".to_owned(), Tag::Int(64)),
            ("z".to_owned(), Tag::Int(5)),
            ("components".to_owned(), Tag::Compound(Vec::new())),
            ("keepPacked".to_owned(), Tag::Byte(0)),
        ]
    };
    let bare: &[(&str, &str, &str)] = &[
        (
            "mob_spawner-bare",
            "minecraft:spawner",
            "minecraft:mob_spawner",
        ),
        (
            "trial_spawner-bare",
            "minecraft:trial_spawner",
            "minecraft:trial_spawner",
        ),
        ("sign-bare", "minecraft:oak_sign", "minecraft:sign"),
        (
            "hanging_sign-bare",
            "minecraft:oak_hanging_sign",
            "minecraft:hanging_sign",
        ),
        ("beehive-bare", "minecraft:beehive", "minecraft:beehive"),
        (
            "creaking_heart-bare",
            "minecraft:creaking_heart",
            "minecraft:creaking_heart",
        ),
        ("jigsaw-bare", "minecraft:jigsaw", "minecraft:jigsaw"),
        (
            "end_gateway-bare",
            "minecraft:end_gateway",
            "minecraft:end_gateway",
        ),
        (
            "sculk_catalyst-bare",
            "minecraft:sculk_catalyst",
            "minecraft:sculk_catalyst",
        ),
        ("beacon-bare", "minecraft:beacon", "minecraft:beacon"),
        (
            "conduit-bare",
            "minecraft:conduit",
            "minecraft:conduit",
        ),
        (
            "potent_sulfur-bare",
            "minecraft:potent_sulfur",
            "minecraft:potent_sulfur",
        ),
        (
            "structure_block-bare",
            "minecraft:structure_block",
            "minecraft:structure_block",
        ),
        (
            "ender_chest-bare",
            "minecraft:ender_chest",
            "minecraft:ender_chest",
        ),
        ("jukebox-bare", "minecraft:jukebox", "minecraft:jukebox"),
        (
            "shulker_box-bare",
            "minecraft:shulker_box",
            "minecraft:shulker_box",
        ),
        (
            "decorated_pot-bare",
            "minecraft:decorated_pot",
            "minecraft:decorated_pot",
        ),
        ("shelf-bare", "minecraft:oak_shelf", "minecraft:shelf"),
        (
            "command_block-bare",
            "minecraft:command_block",
            "minecraft:command_block",
        ),
        (
            "test_block-bare",
            "minecraft:test_block",
            "minecraft:test_block",
        ),
        (
            "test_instance_block-bare",
            "minecraft:test_instance_block",
            "minecraft:test_instance_block",
        ),
        (
            "copper_golem_statue-bare",
            "minecraft:copper_golem_statue",
            "minecraft:copper_golem_statue",
        ),
    ];
    let mut out: Vec<_> = bare
        .iter()
        .map(|(name, state, id)| {
            let tag = Tag::Compound(meta(id));
            (name.to_string(), *state, *id, tag)
        })
        .collect();
    // 富字段：非默认数值、可省字段、带列表的 codec。
    let mut full =
        |name: &str, state: &'static str, id: &'static str, extra: Vec<(String, Tag)>| {
            let mut fields = meta(id);
            fields.extend(extra);
            out.push((name.to_owned(), state, id, Tag::Compound(fields)));
        };
    full(
        "conduit-full",
        "minecraft:conduit",
        "minecraft:conduit",
        vec![("Target".to_owned(), Tag::IntArray(vec![1, 2, 3, 4]))],
    );
    full(
        "mob_spawner-full",
        "minecraft:spawner",
        "minecraft:mob_spawner",
        vec![
            ("Delay".to_owned(), Tag::Short(7)),
            ("SpawnCount".to_owned(), Tag::Short(9)),
            ("MinSpawnDelay".to_owned(), Tag::Short(100)),
            ("MaxSpawnDelay".to_owned(), Tag::Short(900)),
            (
                "SpawnPotentials".to_owned(),
                Tag::List(vec![Tag::Compound(vec![
                    (
                        "data".to_owned(),
                        Tag::Compound(vec![(
                            "entity".to_owned(),
                            Tag::Compound(vec![(
                                "id".to_owned(),
                                Tag::String("minecraft:zombie".to_owned()),
                            )]),
                        )]),
                    ),
                    ("weight".to_owned(), Tag::Int(3)),
                ])]),
            ),
        ],
    );
    full(
        "trial_spawner-full",
        "minecraft:trial_spawner",
        "minecraft:trial_spawner",
        vec![
            (
                "registered_players".to_owned(),
                Tag::List(vec![Tag::IntArray(vec![1, 2, 3, 4])]),
            ),
            (
                "current_mobs".to_owned(),
                Tag::List(vec![Tag::IntArray(vec![5, 6, 7, 8])]),
            ),
            ("cooldown_ends_at".to_owned(), Tag::Long(700)),
            ("next_mob_spawns_at".to_owned(), Tag::Long(80)),
            ("total_mobs_spawned".to_owned(), Tag::Int(3)),
            (
                "spawn_data".to_owned(),
                Tag::Compound(vec![(
                    "entity".to_owned(),
                    Tag::Compound(vec![(
                        "id".to_owned(),
                        Tag::String("minecraft:zombie".to_owned()),
                    )]),
                )]),
            ),
            (
                "ejecting_loot_table".to_owned(),
                Tag::String("minecraft:spawners/trial_chamber/key".to_owned()),
            ),
            ("target_cooldown_length".to_owned(), Tag::Int(100)),
            ("required_player_range".to_owned(), Tag::Int(5)),
        ],
    );
    full(
        "trial_spawner-inline-config",
        "minecraft:trial_spawner",
        "minecraft:trial_spawner",
        vec![(
            "normal_config".to_owned(),
            Tag::Compound(vec![
                ("spawn_range".to_owned(), Tag::Int(3)),
                ("total_mobs".to_owned(), Tag::Float(5.0)),
            ]),
        )],
    );
    full(
        "sign-full",
        "minecraft:oak_sign",
        "minecraft:sign",
        vec![
            (
                "front_text".to_owned(),
                Tag::Compound(vec![
                    (
                        "messages".to_owned(),
                        Tag::List(vec![
                            Tag::String("hi".to_owned()),
                            Tag::String(String::new()),
                            Tag::String(String::new()),
                            Tag::String(String::new()),
                        ]),
                    ),
                    ("color".to_owned(), Tag::String("red".to_owned())),
                    ("has_glowing_text".to_owned(), Tag::Byte(1)),
                ]),
            ),
            ("is_waxed".to_owned(), Tag::Byte(1)),
        ],
    );
    full(
        "beehive-full",
        "minecraft:beehive",
        "minecraft:beehive",
        vec![
            (
                "bees".to_owned(),
                Tag::List(vec![Tag::Compound(vec![
                    (
                        "entity_data".to_owned(),
                        Tag::Compound(vec![(
                            "id".to_owned(),
                            Tag::String("minecraft:bee".to_owned()),
                        )]),
                    ),
                    ("ticks_in_hive".to_owned(), Tag::Int(12)),
                    ("min_ticks_in_hive".to_owned(), Tag::Int(20)),
                ])]),
            ),
            ("flower_pos".to_owned(), Tag::IntArray(vec![1, 2, 3])),
        ],
    );
    full(
        "jigsaw-full",
        "minecraft:jigsaw",
        "minecraft:jigsaw",
        vec![
            (
                "name".to_owned(),
                Tag::String("minecraft:target".to_owned()),
            ),
            ("pool".to_owned(), Tag::String("minecraft:empty".to_owned())),
            (
                "final_state".to_owned(),
                Tag::String("minecraft:stone".to_owned()),
            ),
            ("joint".to_owned(), Tag::String("rollable".to_owned())),
            ("placement_priority".to_owned(), Tag::Int(5)),
            ("selection_priority".to_owned(), Tag::Int(-2)),
        ],
    );
    full(
        "end_gateway-full",
        "minecraft:end_gateway",
        "minecraft:end_gateway",
        vec![
            ("Age".to_owned(), Tag::Long(99)),
            ("exit_portal".to_owned(), Tag::IntArray(vec![4, 5, 6])),
            ("ExactTeleport".to_owned(), Tag::Byte(1)),
        ],
    );
    full(
        "sculk_catalyst-full",
        "minecraft:sculk_catalyst",
        "minecraft:sculk_catalyst",
        vec![(
            "cursors".to_owned(),
            Tag::List(vec![Tag::Compound(vec![
                ("pos".to_owned(), Tag::IntArray(vec![1, 2, 3])),
                ("charge".to_owned(), Tag::Int(7)),
                (
                    "facings".to_owned(),
                    Tag::List(vec![Tag::String("up".to_owned())]),
                ),
            ])]),
        )],
    );
    full(
        "potent_sulfur-full",
        "minecraft:potent_sulfur",
        "minecraft:potent_sulfur",
        vec![("countdown".to_owned(), Tag::Int(42))],
    );
    full(
        "structure_block-full",
        "minecraft:structure_block",
        "minecraft:structure_block",
        vec![
            ("name".to_owned(), Tag::String("minecraft:test".to_owned())),
            ("author".to_owned(), Tag::String("builder".to_owned())),
            ("posX".to_owned(), Tag::Int(9)),
            ("sizeY".to_owned(), Tag::Int(4)),
            (
                "rotation".to_owned(),
                Tag::String("CLOCKWISE_90".to_owned()),
            ),
            ("mode".to_owned(), Tag::String("LOAD".to_owned())),
            ("ignoreEntities".to_owned(), Tag::Byte(0)),
            ("integrity".to_owned(), Tag::Float(0.5)),
            ("seed".to_owned(), Tag::Long(123)),
        ],
    );
    full(
        "shelf-full",
        "minecraft:oak_shelf",
        "minecraft:shelf",
        vec![
            (
                "Items".to_owned(),
                Tag::List(vec![Tag::Compound(vec![
                    ("Slot".to_owned(), Tag::Byte(1)),
                    ("id".to_owned(), Tag::String("minecraft:stone".to_owned())),
                    ("count".to_owned(), Tag::Int(3)),
                ])]),
            ),
            ("align_items_to_bottom".to_owned(), Tag::Byte(1)),
        ],
    );
    full(
        "test_block-full",
        "minecraft:test_block",
        "minecraft:test_block",
        vec![
            ("mode".to_owned(), Tag::String("accept".to_owned())),
            ("message".to_owned(), Tag::String("done".to_owned())),
            ("powered".to_owned(), Tag::Byte(1)),
        ],
    );
    full(
        "test_instance_block-full",
        "minecraft:test_instance_block",
        "minecraft:test_instance_block",
        vec![(
            "data".to_owned(),
            Tag::Compound(vec![
                ("size".to_owned(), Tag::IntArray(vec![3, 4, 5])),
                (
                    "rotation".to_owned(),
                    Tag::String("clockwise_90".to_owned()),
                ),
                ("ignore_entities".to_owned(), Tag::Byte(1)),
                ("status".to_owned(), Tag::String("running".to_owned())),
            ]),
        )],
    );
    // 有内容的条目：这些键没有游戏层写入者，Bamboo 必须自己按官方 codec 带回。
    // Rich entries that no game-layer table owns: the archive codec must carry them.
    let record = mc_bamboo_protocol::components::archive::slots::encode_item(&ItemStack::new(
        mc_bamboo_data::items::MUSIC_DISC_13,
        1,
    ))
    .map_err(|e| e.to_string());
    if let Ok(record) = record {
        full(
            "jukebox-rich",
            "minecraft:jukebox",
            "minecraft:jukebox",
            vec![
                ("RecordItem".to_owned(), record),
                ("ticks_since_song_started".to_owned(), Tag::Long(42)),
            ],
        );
    }
    let pot_item =
        mc_bamboo_protocol::components::archive::slots::encode_item(&ItemStack::new(STONE, 3))
            .map_err(|e| e.to_string());
    if let Ok(pot_item) = pot_item {
        full(
            "decorated_pot-rich",
            "minecraft:decorated_pot",
            "minecraft:decorated_pot",
            vec![("item".to_owned(), pot_item)],
        );
    }
    let shulker_items = mc_bamboo_protocol::components::archive::slots::encode_list(&[
        mc_bamboo_protocol::components::archive::slots::SlottedItem {
            slot: 3,
            stack: ItemStack::new(STONE, 7),
        },
    ])
    .map_err(|e| e.to_string());
    if let Ok(shulker_items) = shulker_items {
        full(
            "shulker_box-rich",
            "minecraft:shulker_box",
            "minecraft:shulker_box",
            vec![("Items".to_owned(), shulker_items)],
        );
    }
    full(
        "shulker_box-loot-rich",
        "minecraft:shulker_box",
        "minecraft:shulker_box",
        vec![
            (
                "LootTable".to_owned(),
                Tag::String("minecraft:chests/simple_dungeon".to_owned()),
            ),
            ("LootTableSeed".to_owned(), Tag::Long(99)),
        ],
    );
    full(
        "beehive-rich",
        "minecraft:beehive",
        "minecraft:beehive",
        vec![
            (
                "bees".to_owned(),
                Tag::List(vec![Tag::Compound(vec![
                    (
                        "entity_data".to_owned(),
                        Tag::Compound(vec![(
                            "id".to_owned(),
                            Tag::String("minecraft:bee".to_owned()),
                        )]),
                    ),
                    ("ticks_in_hive".to_owned(), Tag::Int(12)),
                    ("min_ticks_in_hive".to_owned(), Tag::Int(20)),
                ])]),
            ),
            ("flower_pos".to_owned(), Tag::IntArray(vec![1, 2, 3])),
        ],
    );
    full(
        "sign-rich",
        "minecraft:oak_sign",
        "minecraft:sign",
        vec![
            ("allow_op_features".to_owned(), Tag::Byte(1)),
            (
                "front_text".to_owned(),
                Tag::Compound(vec![
                    (
                        "messages".to_owned(),
                        Tag::List(vec![
                            Tag::String("hi".to_owned()),
                            Tag::String(String::new()),
                            Tag::String(String::new()),
                            Tag::String(String::new()),
                        ]),
                    ),
                    ("color".to_owned(), Tag::String("red".to_owned())),
                    ("has_glowing_text".to_owned(), Tag::Byte(1)),
                ]),
            ),
            ("is_waxed".to_owned(), Tag::Byte(1)),
        ],
    );
    full(
        "command_block-rich",
        "minecraft:command_block",
        "minecraft:command_block",
        vec![
            ("Command".to_owned(), Tag::String("say hi".to_owned())),
            ("LastOutput".to_owned(), Tag::String("last".to_owned())),
            ("LastExecution".to_owned(), Tag::Long(77)),
            ("SuccessCount".to_owned(), Tag::Int(3)),
        ],
    );
    full(
        "spawner-rich-subcodecs",
        "minecraft:spawner",
        "minecraft:mob_spawner",
        vec![(
            "SpawnPotentials".to_owned(),
            Tag::List(vec![Tag::Compound(vec![
                (
                    "data".to_owned(),
                    Tag::Compound(vec![
                        (
                            "entity".to_owned(),
                            Tag::Compound(vec![(
                                "id".to_owned(),
                                Tag::String("minecraft:zombie".to_owned()),
                            )]),
                        ),
                        ("custom_spawn_rules".to_owned(), Tag::Compound(Vec::new())),
                    ]),
                ),
                ("weight".to_owned(), Tag::Int(2)),
            ])]),
        )],
    );
    // `custom_spawn_rules` 的 `block_light_limit` / `sky_light_limit` 走官方
    // `InclusiveRange.INT`：单 int、[min,max] 列表、{min_inclusive,max_inclusive}
    // 复合标签三种输入形状，逐一核对 load -> save 之后写回的形状。
    full(
        "spawner-custom-light-limits",
        "minecraft:spawner",
        "minecraft:mob_spawner",
        vec![(
            "SpawnPotentials".to_owned(),
            Tag::List(vec![
                Tag::Compound(vec![
                    (
                        "data".to_owned(),
                        Tag::Compound(vec![
                            (
                                "entity".to_owned(),
                                Tag::Compound(vec![(
                                    "id".to_owned(),
                                    Tag::String("minecraft:zombie".to_owned()),
                                )]),
                            ),
                            (
                                "custom_spawn_rules".to_owned(),
                                Tag::Compound(vec![
                                    ("block_light_limit".to_owned(), Tag::Int(7)),
                                    ("sky_light_limit".to_owned(), Tag::Int(9)),
                                ]),
                            ),
                        ]),
                    ),
                    ("weight".to_owned(), Tag::Int(1)),
                ]),
                Tag::Compound(vec![
                    (
                        "data".to_owned(),
                        Tag::Compound(vec![
                            (
                                "entity".to_owned(),
                                Tag::Compound(vec![(
                                    "id".to_owned(),
                                    Tag::String("minecraft:skeleton".to_owned()),
                                )]),
                            ),
                            (
                                "custom_spawn_rules".to_owned(),
                                Tag::Compound(vec![
                                    (
                                        "block_light_limit".to_owned(),
                                        Tag::List(vec![Tag::Int(3), Tag::Int(10)]),
                                    ),
                                    (
                                        "sky_light_limit".to_owned(),
                                        Tag::List(vec![Tag::Int(5), Tag::Int(12)]),
                                    ),
                                ]),
                            ),
                        ]),
                    ),
                    ("weight".to_owned(), Tag::Int(2)),
                ]),
                Tag::Compound(vec![
                    (
                        "data".to_owned(),
                        Tag::Compound(vec![
                            (
                                "entity".to_owned(),
                                Tag::Compound(vec![(
                                    "id".to_owned(),
                                    Tag::String("minecraft:husk".to_owned()),
                                )]),
                            ),
                            (
                                "custom_spawn_rules".to_owned(),
                                Tag::Compound(vec![
                                    (
                                        "block_light_limit".to_owned(),
                                        Tag::Compound(vec![
                                            ("min_inclusive".to_owned(), Tag::Int(2)),
                                            ("max_inclusive".to_owned(), Tag::Int(8)),
                                        ]),
                                    ),
                                    (
                                        "sky_light_limit".to_owned(),
                                        Tag::Compound(vec![
                                            ("min_inclusive".to_owned(), Tag::Int(4)),
                                            ("max_inclusive".to_owned(), Tag::Int(4)),
                                        ]),
                                    ),
                                ]),
                            ),
                        ]),
                    ),
                    ("weight".to_owned(), Tag::Int(3)),
                ]),
            ]),
        )],
    );
    full(
        "beehive-rich-entity-data",
        "minecraft:beehive",
        "minecraft:beehive",
        vec![(
            "bees".to_owned(),
            Tag::List(vec![Tag::Compound(vec![
                (
                    "entity_data".to_owned(),
                    Tag::Compound(vec![
                        ("id".to_owned(), Tag::String("minecraft:bee".to_owned())),
                        ("HasNectar".to_owned(), Tag::Byte(1)),
                        ("CustomName".to_owned(), Tag::String("bee".to_owned())),
                    ]),
                ),
                ("ticks_in_hive".to_owned(), Tag::Int(1)),
                ("min_ticks_in_hive".to_owned(), Tag::Int(2)),
            ])]),
        )],
    );
    full(
        "creaking_heart-bound",
        "minecraft:creaking_heart",
        "minecraft:creaking_heart",
        vec![("creaking".to_owned(), Tag::IntArray(vec![1, 2, 3, 4]))],
    );
    full(
        "command_block-full",
        "minecraft:command_block",
        "minecraft:command_block",
        vec![
            ("Command".to_owned(), Tag::String("say hi".to_owned())),
            ("SuccessCount".to_owned(), Tag::Int(7)),
            ("TrackOutput".to_owned(), Tag::Byte(0)),
            ("auto".to_owned(), Tag::Byte(1)),
        ],
    );
    // `CopperGolemStatueBlockEntity`：不覆写 `saveAdditional`，自定义名只在
    // `components` 组件表里。`components` 是元数据键，重存比对不看它的值，
    // 但这一题让官方 `loadStatic` 真的读回名字再写出。
    let mut statue = meta("minecraft:copper_golem_statue");
    statue.retain(|(key, _)| key != "components");
    statue.push((
        "components".to_owned(),
        Tag::Compound(vec![(
            "minecraft:custom_name".to_owned(),
            Tag::String("Copper Nick".to_owned()),
        )]),
    ));
    out.push((
        "copper_golem_statue-named".to_owned(),
        "minecraft:copper_golem_statue",
        "minecraft:copper_golem_statue",
        Tag::Compound(statue),
    ));
    out
}

fn block_entity_load_cases() -> Result<Vec<(String, &'static str, Tag)>, String> {
    use mc_bamboo_protocol::components::archive::slots::{SlottedItem, encode_list};

    let item = ItemStack::new(STONE, 7);
    let items = encode_list(&[SlottedItem {
        slot: 0,
        stack: item.clone(),
    }])
    .map_err(|error| error.to_string())?;
    let empty_items = Tag::List(Vec::new());
    let meta = |id: &str| {
        vec![
            ("id".to_owned(), Tag::String(id.to_owned())),
            ("x".to_owned(), Tag::Int(3)),
            ("y".to_owned(), Tag::Int(64)),
            ("z".to_owned(), Tag::Int(5)),
            ("components".to_owned(), Tag::Compound(Vec::new())),
            ("keepPacked".to_owned(), Tag::Byte(0)),
        ]
    };
    let mut out = Vec::new();
    // 空容器、满容器、带战利品表、冷却为负或很大的漏斗。
    for (name, items_tag, table) in [
        ("chest-empty", empty_items.clone(), None),
        ("chest-full", items.clone(), None),
        (
            "chest-loot",
            empty_items.clone(),
            Some("minecraft:chests/simple_dungeon"),
        ),
    ] {
        let mut fields = meta("minecraft:chest");
        fields.push(("Items".to_owned(), items_tag));
        if let Some(table) = table {
            fields.push(("LootTable".to_owned(), Tag::String(table.to_owned())));
            fields.push(("LootTableSeed".to_owned(), Tag::Long(12345)));
        }
        out.push((name.to_owned(), "minecraft:chest", Tag::Compound(fields)));
    }
    for (name, cooldown) in [("hopper-negative", -1), ("hopper-large", i32::MAX)] {
        let mut fields = meta("minecraft:hopper");
        fields.push(("Items".to_owned(), items.clone()));
        fields.push(("TransferCooldown".to_owned(), Tag::Int(cooldown)));
        out.push((name.to_owned(), "minecraft:hopper", Tag::Compound(fields)));
    }
    // 机器工作中途、一半的活塞。
    let mut furnace = meta("minecraft:furnace");
    for (key, value) in [
        ("cooking_time_spent", Tag::Int(77)),
        ("cooking_total_time", Tag::Int(200)),
        ("lit_time_remaining", Tag::Int(120)),
        ("lit_total_time", Tag::Int(200)),
        ("speed_multiplier", Tag::Float(1.5)),
        ("Items", items.clone()),
        ("RecipesUsed", Tag::Compound(Vec::new())),
    ] {
        furnace.push((key.to_owned(), value));
    }
    out.push((
        "furnace-mid".to_owned(),
        "minecraft:furnace",
        Tag::Compound(furnace),
    ));
    let mut brewing = meta("minecraft:brewing_stand");
    for (key, value) in [
        ("BrewTime", Tag::Int(123)),
        ("total_brew_time", Tag::Int(400)),
        ("Fuel", Tag::Int(7)),
        ("total_fuel", Tag::Int(20)),
        ("speed_multiplier", Tag::Float(1.25)),
        ("Items", items.clone()),
    ] {
        brewing.push((key.to_owned(), value));
    }
    out.push((
        "brewing-mid".to_owned(),
        "minecraft:brewing_stand",
        Tag::Compound(brewing),
    ));
    let mut campfire = meta("minecraft:campfire");
    campfire.push(("Items".to_owned(), items.clone()));
    campfire.push(("CookingTimes".to_owned(), Tag::IntArray(vec![1, 2, 3, 4])));
    campfire.push((
        "CookingTotalTimes".to_owned(),
        Tag::IntArray(vec![100, 200, 300, 400]),
    ));
    out.push((
        "campfire-mid".to_owned(),
        "minecraft:campfire",
        Tag::Compound(campfire),
    ));
    let mut piston = meta("minecraft:piston");
    for (key, value) in [
        ("blockState", Tag::String("minecraft:stone".to_owned())),
        ("facing", Tag::Byte(2)),
        ("progress", Tag::Float(0.5)),
        ("extending", Tag::Byte(1)),
        ("source", Tag::Byte(0)),
    ] {
        piston.push((key.to_owned(), value));
    }
    out.push((
        "piston-mid".to_owned(),
        "minecraft:moving_piston",
        Tag::Compound(piston),
    ));
    let mut beacon = meta("minecraft:beacon");
    for (key, value) in [
        ("primary_effect", Tag::String("minecraft:speed".to_owned())),
        (
            "secondary_effect",
            Tag::String("minecraft:regeneration".to_owned()),
        ),
        ("Levels", Tag::Int(4)),
    ] {
        beacon.push((key.to_owned(), value));
    }
    out.push((
        "beacon-full".to_owned(),
        "minecraft:beacon",
        Tag::Compound(beacon),
    ));
    let mut conduit = meta("minecraft:conduit");
    conduit.push(("Target".to_owned(), Tag::IntArray(vec![1, 2, 3, 4])));
    out.push((
        "conduit-full".to_owned(),
        "minecraft:conduit",
        Tag::Compound(conduit),
    ));
    // 宝库 / 幽匿方块实体：只有坐标的裸条目、codec 默认值省略、非空列表和监听器。
    // Vault and sculk block entities: bare entries, codec default omission,
    // non-empty lists and vibration listeners.
    let uuid = Tag::IntArray(vec![1, 2, 3, 4]);
    for (name, id) in [
        ("vault-bare", "minecraft:vault"),
        ("sculk-sensor-bare", "minecraft:sculk_sensor"),
        (
            "calibrated-sculk-sensor-bare",
            "minecraft:calibrated_sculk_sensor",
        ),
        ("sculk-shrieker-bare", "minecraft:sculk_shrieker"),
    ] {
        out.push((name.to_owned(), id, Tag::Compound(meta(id))));
    }
    // 铜傀儡雕像：`create_block_entity` 要认得 `minecraft:copper_golem_statue`，
    // 且 `write_additional` 为空（官方不覆写 `saveAdditional`）。
    out.push((
        "copper_golem_statue-bare".to_owned(),
        "minecraft:copper_golem_statue",
        Tag::Compound(meta("minecraft:copper_golem_statue")),
    ));
    let mut statue_named = meta("minecraft:copper_golem_statue");
    statue_named.retain(|(key, _)| key != "components");
    statue_named.push((
        "components".to_owned(),
        Tag::Compound(vec![(
            "minecraft:custom_name".to_owned(),
            Tag::String("Copper Nick".to_owned()),
        )]),
    ));
    out.push((
        "copper_golem_statue-named".to_owned(),
        "minecraft:copper_golem_statue",
        Tag::Compound(statue_named),
    ));
    let mut vault = meta("minecraft:vault");
    vault.push((
        "config".to_owned(),
        Tag::Compound(vec![
            (
                "loot_table".to_owned(),
                Tag::String("minecraft:chests/trial_chambers/reward".to_owned()),
            ),
            ("activation_range".to_owned(), Tag::Double(3.0)),
            ("deactivation_range".to_owned(), Tag::Double(4.5)),
        ]),
    ));
    vault.push((
        "shared_data".to_owned(),
        Tag::Compound(vec![(
            "connected_players".to_owned(),
            Tag::List(vec![uuid.clone()]),
        )]),
    ));
    vault.push((
        "server_data".to_owned(),
        Tag::Compound(vec![
            ("items_to_eject".to_owned(), Tag::List(Vec::new())),
            ("total_ejections_needed".to_owned(), Tag::Int(7)),
            ("state_updating_resumes_at".to_owned(), Tag::Long(99)),
        ]),
    ));
    out.push((
        "vault-full".to_owned(),
        "minecraft:vault",
        Tag::Compound(vault),
    ));
    let mut sensor = meta("minecraft:sculk_sensor");
    sensor.push(("last_vibration_frequency".to_owned(), Tag::Int(5)));
    sensor.push((
        "listener".to_owned(),
        Tag::Compound(vec![
            (
                "selector".to_owned(),
                Tag::Compound(vec![("tick".to_owned(), Tag::Long(42))]),
            ),
            ("event_delay".to_owned(), Tag::Int(3)),
        ]),
    ));
    out.push((
        "sculk-sensor-full".to_owned(),
        "minecraft:sculk_sensor",
        Tag::Compound(sensor),
    ));
    let mut shrieker = meta("minecraft:sculk_shrieker");
    shrieker.push(("warning_level".to_owned(), Tag::Int(2)));
    out.push((
        "sculk-shrieker-full".to_owned(),
        "minecraft:sculk_shrieker",
        Tag::Compound(shrieker),
    ));
    // 振动事件：`game_event` 是注册表 holder codec，注册过的名字保留，
    // 没注册过的名字让 `selector.event` 回到可省字段的缺省（连同 tick 折成 -1L）。
    for (name, event) in [
        ("sculk-sensor-registered-event", "minecraft:step"),
        ("sculk-sensor-unknown-event", "minecraft:not_a_game_event"),
    ] {
        let mut sensor = meta("minecraft:sculk_sensor");
        sensor.push((
            "listener".to_owned(),
            Tag::Compound(vec![
                (
                    "selector".to_owned(),
                    Tag::Compound(vec![
                        (
                            "event".to_owned(),
                            Tag::Compound(vec![
                                ("game_event".to_owned(), Tag::String(event.to_owned())),
                                ("distance".to_owned(), Tag::Float(2.5)),
                                (
                                    "pos".to_owned(),
                                    Tag::List(vec![
                                        Tag::Double(1.0),
                                        Tag::Double(2.0),
                                        Tag::Double(3.0),
                                    ]),
                                ),
                            ]),
                        ),
                        ("tick".to_owned(), Tag::Long(42)),
                    ]),
                ),
                ("event_delay".to_owned(), Tag::Int(3)),
            ]),
        ));
        out.push((
            name.to_owned(),
            "minecraft:sculk_sensor",
            Tag::Compound(sensor),
        ));
    }
    Ok(out)
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
        out.push(
            write_entity(&base(
                EntityKind::Tnt(PrimedTnt { fuse, owner: None }),
                22,
                pos,
            ))
            .unwrap(),
        );
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
    out.extend(arrow_archive_cases());
    out.extend(trident_archive_cases());
    out.extend(spit_archive_cases());
    out.extend(egg_archive_cases());
    out.extend(fish_archive_cases());
    out.extend(jockey_archive_cases());
    out.extend(guardian_archive_cases());
    out.extend(sulfur_cube_archive_cases());
    out.extend(nautilus_archive_cases());
    out.extend(zombie_villager_archive_cases());
    out.extend(parched_jockey_archive_cases());
    out.extend(hanging_archive_cases());
    out
}

/// 画、物品展示框和荧光物品展示框的存档往返：`block_pos`、`facing`/`Facing`、
/// `variant`、`Item`、`ItemRotation`、`ItemDropChance`、`Fixed`、`Invisible` 与 `CustomName`。
///
/// Save round-trip for paintings, item frames, and glow item frames.
fn hanging_archive_cases() -> Vec<Tag> {
    use mc_bamboo_blocks::entity::hanging::{recalculate_item_frame, recalculate_painting};
    use mc_bamboo_core::Direction;
    use mc_bamboo_data::{items, painting as painting_data};

    let mut out = Vec::new();
    let mut store = mc_bamboo_blocks::EntityStore::new();

    // 1. Painting with non-default facing, variant, and custom name.
    let mut p1 = Entity::painting(
        BlockPos::new(2, 65, -3),
        Direction::West,
        painting_data::id_of("minecraft:donkey_kong").expect("donkey_kong"),
    );
    if let EntityKind::Painting(p) = &mut p1.kind {
        p.custom_name = Some(
            mc_bamboo_protocol::text::Component::text("挂画")
                .tag()
                .clone(),
        );
    }
    recalculate_painting(&mut p1);
    let uid = store.spawn(p1);
    out.push(write_entity(&store.get(uid).expect("p1").split()).expect("write p1"));

    // 2. Painting with default facing (South) and default variant (alban).
    let p2 = Entity::painting(
        BlockPos::new(-1, 64, 4),
        Direction::South,
        painting_data::DEFAULT_VARIANT,
    );
    let uid = store.spawn(p2);
    out.push(write_entity(&store.get(uid).expect("p2").split()).expect("write p2"));

    // 3. Empty ItemFrame with default flags and facing North.
    let f1 = Entity::item_frame(BlockPos::new(0, 64, 1), Direction::North, false);
    let uid = store.spawn(f1);
    out.push(write_entity(&store.get(uid).expect("f1").split()).expect("write f1"));

    // 4. ItemFrame facing Up with item, rotation, drop chance, Fixed=true, Invisible=true.
    let mut f2 = Entity::item_frame(BlockPos::new(3, 66, -2), Direction::Up, false);
    if let EntityKind::ItemFrame(f) = &mut f2.kind {
        f.item = ItemStack::new(items::DIAMOND, 1);
        f.rotation = 3;
        f.drop_chance = 0.5;
        f.fixed = true;
        f.invisible = true;
    }
    recalculate_item_frame(&mut f2);
    let uid = store.spawn(f2);
    out.push(write_entity(&store.get(uid).expect("f2").split()).expect("write f2"));

    // 5. GlowItemFrame facing East with item, rotation=6, and custom name.
    let mut f3 = Entity::item_frame(BlockPos::new(-2, 67, 5), Direction::East, true);
    if let EntityKind::ItemFrame(f) = &mut f3.kind {
        f.item = ItemStack::new(items::GOLD_INGOT, 1);
        f.rotation = 6;
        f.custom_name = Some(
            mc_bamboo_protocol::text::Component::text("荧光框")
                .tag()
                .clone(),
        );
    }
    recalculate_item_frame(&mut f3);
    let uid = store.spawn(f3);
    out.push(write_entity(&store.get(uid).expect("f3").split()).expect("write f3"));

    out
}

/// 鹦鹉螺 / 僵尸鹦鹉螺的存档往返：驯服的主人、鞍、铠甲、大脑记忆、
/// 冲刺标志和僵尸变种。官方 `EntityType.loadEntityRecursive` 逐键重存。
///
/// Nautilus / zombie-nautilus save round-trip: tame owner, saddle, armor,
/// brain memories, dash flag and the zombie variant, resaved by vanilla.
fn nautilus_archive_cases() -> Vec<Tag> {
    use mc_bamboo_blocks::entity::mob::{MobType, brain::memory::MemoryType, tame};
    use mc_bamboo_data::items;

    [MobType::Nautilus, MobType::ZombieNautilus]
        .into_iter()
        .map(|kind| {
            let mut store = mc_bamboo_blocks::EntityStore::new();
            let uid = store.spawn(Entity::by_mob_type(kind, Vec3::new(1.5, 63.0, -2.5)));
            store.with_mut(uid, |entity| {
                entity.with_mob(|_, mob| {
                    tame::set_tame(mob, true, false);
                    mob.tame.owner = Some((uid as u128 + 1).to_be_bytes());
                    mob.equipment.set(
                        mc_bamboo_protocol::packet::play::EquipmentSlot::Saddle,
                        ItemStack::new(items::SADDLE, 1),
                    );
                    mob.equipment.set(
                        mc_bamboo_protocol::packet::play::EquipmentSlot::Body,
                        ItemStack::new(items::IRON_NAUTILUS_ARMOR, 1),
                    );
                    mob.nautilus.dashing = true;
                    mob.nautilus.dash_cooldown = 31;
                    if kind == MobType::ZombieNautilus {
                        mob.nautilus.variant = 1;
                    }
                    if let Some(brain) = mob.brain.as_mut() {
                        brain.memories.set(
                            MemoryType::AttackTargetCooldown,
                            mc_bamboo_blocks::entity::mob::brain::memory::MemoryValue::Int(2700),
                        );
                    }
                });
            });
            write_entity(&store.get(uid).expect("fresh nautilus").split())
                .expect("nautilus archive")
        })
        .collect()
}

/// 尸壳骆驼载两名乘客（尸壳、焦骸）的存档树：`Passengers` 嵌在载具里，
/// 官方 `loadEntityRecursive` 逐层重存。
///
/// A camel husk carrying two passengers (husk and parched): `Passengers` is
/// nested in the vehicle and the official recursive loader resaves each layer.
fn parched_jockey_archive_cases() -> Vec<Tag> {
    use mc_bamboo_blocks::entity::mob::MobType;

    let mut store = mc_bamboo_blocks::EntityStore::new();
    let camel = store.spawn(Entity::by_mob_type(
        MobType::CamelHusk,
        Vec3::new(1.5, 64.0, -2.5),
    ));
    let husk = store.spawn(Entity::by_mob_type(
        MobType::Husk,
        Vec3::new(1.5, 64.0, -2.0),
    ));
    let parched = store.spawn(Entity::by_mob_type(
        MobType::Parched,
        Vec3::new(1.5, 64.0, -3.2),
    ));
    // 官方 `loadEntityRecursive` 用 `startRiding` 把乘客的朝向归零；夹具先对齐。
    // The official recursive loader zeroes passenger facing via `startRiding`;
    // align the fixture up front.
    for uid in [camel, husk, parched] {
        store.with_mut(uid, |entity| {
            entity.y_rot = 0.0;
            entity.with_mob(|_, mob| {
                mob.y_head_rot = 0.0;
                mob.y_body_rot = 0.0;
                mob.x_rot = 0.0;
            });
        });
    }
    store.with_mut(camel, |entity| {
        entity.with_mob(|_, mob| mob.passengers.extend([husk, parched]));
    });
    for rider in [husk, parched] {
        store.with_mut(rider, |entity| {
            entity.vehicle = Some(camel);
        });
    }
    let rows: BTreeMap<i32, mc_bamboo_blocks::entity::EntityRow> =
        [store.get(camel), store.get(husk), store.get(parched)]
            .into_iter()
            .flatten()
            .map(|entity| {
                let row = entity.split();
                (row.public.uid, row)
            })
            .collect();
    // 再读再写一遍：官方 `loadEntityRecursive` 会把乘客的朝向归零并补上懒创建的
    // 属性实例，和鸡骑士夹具一样先归一化，两边比的才是存档层。
    // Read and write once more: the official recursive loader zeroes passenger
    // rotation and creates lazy attribute instances, so normalize first just
    // like the chicken-jockey fixture.
    let first = write_entity_tree(&rows[&camel], &|uid| rows.get(&uid).cloned()).unwrap();
    let mut next = 1 << 24;
    let tree = read_entity_tree(&first, &mut next).unwrap().unwrap();
    let root = tree[0].public.uid;
    let rebuilt: BTreeMap<i32, mc_bamboo_blocks::entity::EntityRow> =
        tree.into_iter().map(|row| (row.public.uid, row)).collect();
    vec![write_entity_tree(&rebuilt[&root], &|uid| rebuilt.get(&uid).cloned()).unwrap()]
}

/// 僵尸村民的存档：`VillagerData`、`VillagerDataFinalized`、`Offers`、`Gossips`、
/// `ConversionTime`、`ConversionPlayer`、`Xp`，加上僵尸一系的 `IsBaby` /
/// `CanBreakDoors` / 溺水计时。官方 `EntityType.loadEntityRecursive` 逐键重存。
///
/// Zombie-villager archive: the six villager keys plus the zombie branch's
/// baby / door / drowning keys, resaved by the official entity loader.
fn zombie_villager_archive_cases() -> Vec<Tag> {
    use mc_bamboo_blocks::entity::mob::MobType;
    use mc_bamboo_nbt::Tag;

    let mut out = Vec::new();
    for converting in [false, true] {
        let mut store = mc_bamboo_blocks::EntityStore::new();
        let uid = store.spawn(Entity::by_mob_type(
            MobType::ZombieVillager,
            Vec3::new(1.5, 64.0, -3.5),
        ));
        store.with_mut(uid, |entity| {
            entity.with_mob(|_, mob| {
                mob.villager.kind =
                    mc_bamboo_data::villager::type_id("minecraft:taiga").unwrap_or(0);
                mob.villager.profession =
                    mc_bamboo_data::villager::profession_id("minecraft:farmer").unwrap_or(0);
                mob.villager.level = 3;
                mob.villager.finalized = true;
                mob.villager.xp = 42;
                // `Offers` / `Gossips` 是原样保留的原文，官方 codec 会规范化
                // 空列表；这里留空，由 mob-oracle 的存档场景逐键比。
                // `Offers` / `Gossips` are verbatim tags; the official codec
                // normalizes empty lists, so the dedicated mob-oracle scene
                // compares those two keys instead.
                mob.zombie_villager.converting = converting;
                mob.zombie_villager.conversion_time = if converting { 123 } else { -1 };
                mob.zombie_villager.conversion_player = if converting {
                    Some([0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0])
                } else {
                    None
                };
                mob.zombie_baby = false;
                mob.can_break_doors = true;
            });
        });
        let mut tag = write_entity(&store.get(uid).expect("fresh zombie villager").split())
            .expect("zombie villager archive");
        // 官方 loader 会把懒创建的属性实例写回去（空的 `attributes` 键被补成
        // 已实例化的条目），这里把 Bamboo 的空表去掉，避免把已知的懒创建差异
        // 算成僵尸村民存档键的差异。
        // The official loader writes back lazily instantiated attributes; drop
        // Bamboo's empty list so the known lazy-attribute difference does not
        // obscure the zombie-villager keys under test.
        if let Tag::Compound(entries) = &mut tag {
            entries.retain(|(key, _)| key != "attributes");
        }
        out.push(tag);
    }
    out
}

/// 守卫者 / 远古守卫者的存档往返：两者都没有自有存档字段，远古靠
/// `PersistenceRequired` 和家；元数据状态（`moving` / `attack_target`）不写进存档。
///
/// Guardian / elder guardian archive round trip: neither has its own saved fields,
/// the elder relies on `PersistenceRequired` and a home; metadata state
/// (`moving` / `attack_target`) is not saved.
fn guardian_archive_cases() -> [Tag; 2] {
    use mc_bamboo_blocks::entity::mob::MobType;

    [MobType::Guardian, MobType::ElderGuardian].map(|kind| {
        let mut store = mc_bamboo_blocks::EntityStore::new();
        let uid = store.spawn(Entity::by_mob_type(kind, Vec3::new(1.5, 63.0, -2.5)));
        store.with_mut(uid, |entity| {
            entity.with_mob(|_, mob| {
                // 元数据状态不属于存档字段。
                // Metadata state is not part of the saved fields.
                mob.guardian.moving = true;
                mob.guardian.attack_target = 42;
                if kind == MobType::ElderGuardian {
                    mob.persistence_required = true;
                    mob.home = Some((BlockPos::new(1, 63, -2), 16));
                }
            });
        });
        write_entity(&store.get(uid).expect("fresh guardian").split()).expect("guardian archive")
    })
}

/// 硫磺方块怪存档：`Size` / `wasOnGround` / `pickup_timer` / `from_bucket` /
/// `fuse` 五项自有键，加上吞下的方块写进 `equipment.body`。
fn sulfur_cube_archive_cases() -> Vec<Tag> {
    use mc_bamboo_blocks::entity::mob::{MobType, sulfur_cube};
    use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

    let mut cases = Vec::new();
    for (size, body, fuse, from_bucket) in [
        (2, 0, -1, false),
        (1, mc_bamboo_data::items::TNT, 120, true),
        (2, mc_bamboo_data::items::MAGMA_BLOCK, 17, false),
    ] {
        let mut store = mc_bamboo_blocks::EntityStore::new();
        let uid = store.spawn(Entity::by_mob_type(
            MobType::SulfurCube,
            Vec3::new(1.5, 63.0, -2.5),
        ));
        store.with_mut(uid, |entity| {
            entity.with_mob(|me, mob| {
                sulfur_cube::set_size(me, mob, size, true);
                mob.sulfur_cube.pickup_timer = 37;
                mob.sulfur_cube.fuse = fuse;
                mob.sulfur_cube.max_fuse = fuse;
                mob.sulfur_cube.from_bucket = from_bucket;
                mob.cube.was_on_ground = true;
                if body != 0 {
                    mob.equipment
                        .set(EquipmentSlot::Body, ItemStack::new(body, 1));
                    sulfur_cube::collect_equipment_changes(mob, ItemStack::new(body, 1));
                }
            });
        });
        cases.push(
            write_entity(&store.get(uid).expect("fresh sulfur cube").split())
                .expect("sulfur cube archive"),
        );
    }
    cases
}

/// 一只带乘客的鸡：`Passengers` 嵌在载具的存档里，乘客的 `Pos` 用载具的 x/z。
///
/// A chicken with a rider: `Passengers` nests inside the vehicle and the
/// passenger's `Pos` uses the vehicle's x/z.
fn jockey_archive_cases() -> Vec<Tag> {
    use std::collections::BTreeMap;

    use mc_bamboo_blocks::entity::mob::MobType;

    let mut store = mc_bamboo_blocks::EntityStore::new();
    let chicken = store.spawn(Entity::by_mob_type(
        MobType::Chicken,
        Vec3::new(1.5, 64.0, -2.5),
    ));
    let zombie = store.spawn(Entity::by_mob_type(
        MobType::Zombie,
        Vec3::new(1.5, 64.0, -1.5),
    ));
    store.with_mut(chicken, |entity| {
        entity.with_mob(|_, mob| mob.passengers.push(zombie));
    });
    store.with_mut(zombie, |entity| {
        entity.vehicle = Some(chicken);
        entity.position.x = 1.5;
        entity.position.z = -2.5;
    });
    let rows: BTreeMap<i32, mc_bamboo_blocks::entity::EntityRow> =
        [store.get(chicken), store.get(zombie)]
            .into_iter()
            .flatten()
            .map(|entity| {
                let row = entity.split();
                (row.public.uid, row)
            })
            .collect();
    let first = write_entity_tree(&rows[&chicken], &|uid| rows.get(&uid).cloned()).unwrap();
    // 再读再写一遍：生物读档会补上构造时懒创建的属性（`movement_speed`），官方
    // 重存也是这个形状；用它当输入，两边比的才是存档层而不是属性惰性创建。
    let mut next = 1 << 24;
    let tree = read_entity_tree(&first, &mut next).unwrap().unwrap();
    let root = tree[0].public.uid;
    let rebuilt: BTreeMap<i32, mc_bamboo_blocks::entity::EntityRow> =
        tree.into_iter().map(|row| (row.public.uid, row)).collect();
    vec![write_entity_tree(&rebuilt[&root], &|uid| rebuilt.get(&uid).cloned()).unwrap()]
}

fn fish_archive_cases() -> [Tag; 4] {
    use mc_bamboo_blocks::entity::mob::{MobType, fish};

    [
        MobType::Cod,
        MobType::Salmon,
        MobType::TropicalFish,
        MobType::Pufferfish,
    ]
    .map(|kind| {
        let mut store = mc_bamboo_blocks::EntityStore::new();
        let uid = store.spawn(Entity::by_mob_type(kind, Vec3::new(1.5, 63.0, -2.5)));
        store.with_mut(uid, |entity| {
            entity.with_mob(|me, mob| {
                mob.fish.from_bucket = true;
                match kind {
                    MobType::Salmon => mob.fish.variant = 2,
                    MobType::TropicalFish => mob.fish.variant = fish::pack_variant(9, 10, 4),
                    MobType::Pufferfish => mob.fish.puff_state = 2,
                    _ => {}
                }
                fish::refresh_dimensions(me, mob);
            });
        });
        write_entity(&store.get(uid).expect("fresh fish").split()).expect("fish archive")
    })
}

/// 扔出的鸡蛋：带主人、离开主人、无重力的蓝蛋，和一枚刚生成的普通蛋。
///
/// Thrown eggs: a blue egg with an owner, left owner and no gravity, and a fresh plain egg.
fn egg_archive_cases() -> [Tag; 2] {
    use mc_bamboo_blocks::entity::thrown_egg::{EggShot, ThrowableKind};
    let shot = |item, position| EggShot {
        position,
        direction: Vec3::new(1.0, 0.0, 0.0),
        power: 1.5,
        uncertainty: 1.0,
        source_movement: None,
        owner: None,
        owner_uuid: None,
        item: ItemStack::new(item, 16),
        projectile: ThrowableKind::Egg,
    };
    let mut blue = Entity::thrown_egg(&shot(
        mc_bamboo_data::items::BLUE_EGG,
        Vec3::new(1.5, 65.5, -2.0),
    ));
    blue.id = 26;
    blue.delta = Vec3::new(0.9, -0.3, 0.1);
    blue.y_rot = 40.0;
    let EntityKind::Egg(egg) = &mut blue.kind else {
        unreachable!()
    };
    egg.uuid = 26_u128.to_be_bytes();
    egg.owner_uuid = Some(17_u128.to_be_bytes());
    egg.x_rot = -12.5;
    egg.no_gravity = true;
    egg.left_owner = true;
    egg.has_been_shot = true;
    let mut plain = Entity::thrown_egg(&shot(
        mc_bamboo_data::items::EGG,
        Vec3::new(2.5, 66.0, -2.0),
    ));
    plain.id = 27;
    if let EntityKind::Egg(egg) = &mut plain.kind {
        egg.uuid = 27_u128.to_be_bytes();
    }
    [blue, plain].map(|entity| write_entity(&entity.split()).unwrap())
}

fn arrow_archive_cases() -> [Tag; 2] {
    let mut flight = Entity::arrow(Vec3::new(1.5, 65.25, -2.0));
    flight.id = 24;
    flight.remaining_fire_ticks = 73;
    flight.delta = Vec3::new(1.4, -0.25, 0.2);
    flight.y_rot = 75.0;
    let EntityKind::Arrow(arrow) = &mut flight.kind else {
        unreachable!()
    };
    arrow.uuid = 24_u128.to_be_bytes();
    arrow.owner_uuid = Some(17_u128.to_be_bytes());
    arrow.pitch = -10.0;
    arrow.no_gravity = true;
    arrow.critical = true;
    arrow.pierce = 2;
    arrow.pickup = Pickup::CreativeOnly;
    arrow.base_damage = 3.25;
    arrow.weapon = Some(ItemStack::new(mc_bamboo_data::items::BOW, 1));
    arrow.has_been_shot = true;
    arrow.left_owner = true;

    let mut embedded = Entity::arrow(Vec3::new(1.5, 64.25, -2.0));
    embedded.id = 25;
    let EntityKind::Arrow(arrow) = &mut embedded.kind else {
        unreachable!()
    };
    arrow.uuid = 25_u128.to_be_bytes();
    arrow.in_ground = true;
    arrow.last_state = Some(BlockState(blocks::STONE));
    arrow.shake_time = 6;
    arrow.life = 1199;
    arrow.pickup = Pickup::Allowed;

    [flight, embedded].map(|entity| write_entity(&entity.split()).unwrap())
}

/// 三叉戟的两道存档题：飞行中带忠诚 / 光效 / 主人，和插地、已命中实体的。
/// `DealtDamage` 是 `ThrownTrident` 独有的存档键；忠诚 / 光效是元数据不进存档。
///
/// Two trident save cases: one in flight with loyalty / foil / owner, and one
/// embedded in the ground that has already dealt damage. `DealtDamage` is
/// `ThrownTrident`'s own save key; loyalty / foil are metadata and not saved.
fn trident_archive_cases() -> [Tag; 2] {
    let ground = mc_bamboo_data::sounds::sound_event("minecraft:item.trident.hit_ground");
    let mut flight = Entity::trident(Vec3::new(1.5, 65.25, -2.0));
    flight.id = 30;
    flight.delta = Vec3::new(0.3, 0.2, -0.1);
    flight.y_rot = -20.0;
    let EntityKind::Arrow(arrow) = &mut flight.kind else {
        unreachable!()
    };
    arrow.uuid = 30_u128.to_be_bytes();
    arrow.owner_uuid = Some(17_u128.to_be_bytes());
    arrow.pitch = 12.0;
    arrow.pickup = Pickup::Allowed;
    arrow.pickup_item = ItemStack::new(mc_bamboo_data::items::TRIDENT, 1);
    arrow.sound = ground;
    arrow.loyalty = 3;
    arrow.foil = true;
    arrow.dealt_damage = false;

    let mut spent = Entity::trident(Vec3::new(1.5, 64.25, -2.0));
    spent.id = 31;
    let EntityKind::Arrow(arrow) = &mut spent.kind else {
        unreachable!()
    };
    arrow.uuid = 31_u128.to_be_bytes();
    arrow.pickup_item = ItemStack::new(mc_bamboo_data::items::TRIDENT, 1);
    arrow.sound = ground;
    arrow.in_ground = true;
    arrow.last_state = Some(BlockState(blocks::STONE));
    arrow.shake_time = 4;
    arrow.life = 1199;
    arrow.dealt_damage = true;

    [flight, spent].map(|entity| write_entity(&entity.split()).unwrap())
}

/// 羊驼口水的两道存档题：飞行中、带主人且已离开主人的，和刚造出来什么都没设的。
///
/// Two llama spit save cases: one in flight with an owner it has left, and a bare new one.
fn spit_archive_cases() -> [Tag; 2] {
    let mut flight = Entity::new(
        EntityKind::LlamaSpit(Box::default()),
        Vec3::new(1.5, 65.25, -2.0),
    );
    flight.id = 26;
    flight.delta = Vec3::new(1.2, -0.06, 0.4);
    flight.y_rot = 71.5;
    let EntityKind::LlamaSpit(spit) = &mut flight.kind else {
        unreachable!()
    };
    spit.uuid = 26_u128.to_be_bytes();
    spit.owner_uuid = Some(17_u128.to_be_bytes());
    spit.pitch = -3.5;
    spit.left_owner = true;
    spit.has_been_shot = true;

    let mut bare = Entity::new(
        EntityKind::LlamaSpit(Box::default()),
        Vec3::new(1.5, 64.25, -2.0),
    );
    bare.id = 27;
    if let EntityKind::LlamaSpit(spit) = &mut bare.kind {
        spit.uuid = 27_u128.to_be_bytes();
    }
    [flight, bare].map(|entity| write_entity(&entity.split()).unwrap())
}

/// 一段最小合法的区块 NBT，只为检查 `block_entities` 的键能原样通过官方 parse/write。
/// 牛的重存题：裸条目、带富字段、Bamboo 刚生成又写出的。
///
/// Cow resave cases: bare, rich, and one Bamboo just spawned and wrote.
fn cow_resave_cases() -> Vec<(String, Tag)> {
    let compound = |entries: Vec<(&str, Tag)>| {
        Tag::Compound(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    };
    let uuid = Tag::IntArray(vec![0x1234_5678, -0x1234_5679, 0x0bad_cafe, 0x7357_1001]);
    let doubles = |values: [f64; 3]| Tag::List(values.into_iter().map(Tag::Double).collect());
    let mut out = vec![(
        "bare".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:cow".to_owned())),
            ("Pos", doubles([3.5, 64.0, -7.25])),
            ("UUID", uuid.clone()),
        ]),
    )];
    // 哞菇：缺省（红色、没有炖菜效果）和棕色带两条效果（一条时长等于缺省 160，存盘时省略）。
    // Mooshrooms: the default (red, no stew effects) and a brown one with two effects, one
    // at the default duration of 160 that the save omits.
    out.push((
        "mooshroom bare".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:mooshroom".to_owned())),
            ("Pos", doubles([3.5, 64.0, -7.25])),
            ("UUID", uuid.clone()),
        ]),
    ));
    let stew = |id: &str, duration: Option<i32>| {
        let mut entry = vec![("id", Tag::String(id.to_owned()))];
        if let Some(duration) = duration {
            entry.push(("duration", Tag::Int(duration)));
        }
        compound(entry)
    };
    out.push((
        "mooshroom brown stew".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:mooshroom".to_owned())),
            ("Pos", doubles([3.5, 64.0, -7.25])),
            ("UUID", uuid.clone()),
            ("Age", Tag::Int(-1200)),
            ("Type", Tag::String("brown".to_owned())),
            (
                "stew_effects",
                Tag::List(vec![
                    stew("minecraft:regeneration", Some(140)),
                    stew("minecraft:blindness", Some(160)),
                    stew("minecraft:saturation", None),
                ]),
            ),
        ]),
    ));
    out.push((
        "polar bear with persistent anger".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:polar_bear".to_owned())),
            ("Pos", doubles([3.5, 64.0, -7.25])),
            ("UUID", uuid.clone()),
            ("anger_end_time", Tag::Long(9876543210)),
            ("angry_at", Tag::IntArray(vec![1, 2, 3, 4])),
        ]),
    ));
    // 嗅探兽：`Brain` 里两条可序列化记忆——`sniffer_explored_positions`（GlobalPos 列表）
    // 和 `sniff_cooldown`（Unit 带 ttl）。官方 load->save 和 Bamboo read->write 逐键比。
    out.push((
        "sniffer explored positions and cooldown".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:sniffer".to_owned())),
            ("Pos", doubles([3.5, 64.0, -7.25])),
            ("UUID", uuid.clone()),
            (
                "Brain",
                compound(vec![(
                    "memories",
                    compound(vec![
                        (
                            "minecraft:sniffer_explored_positions",
                            compound(vec![(
                                "value",
                                Tag::List(vec![
                                    compound(vec![
                                        (
                                            "dimension",
                                            Tag::String("minecraft:overworld".to_owned()),
                                        ),
                                        ("pos", Tag::IntArray(vec![3, 63, -5])),
                                    ]),
                                    compound(vec![
                                        (
                                            "dimension",
                                            Tag::String("minecraft:overworld".to_owned()),
                                        ),
                                        ("pos", Tag::IntArray(vec![8, 62, 2])),
                                    ]),
                                ]),
                            )]),
                        ),
                        (
                            "minecraft:sniff_cooldown",
                            compound(vec![("value", compound(vec![])), ("ttl", Tag::Long(9600))]),
                        ),
                    ]),
                )]),
            ),
        ]),
    ));
    out.push((
        "cat tamed rich".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:cat".to_owned())),
            ("Pos", doubles([3.5, 64.0, -7.25])),
            ("UUID", uuid.clone()),
            ("variant", Tag::String("minecraft:calico".to_owned())),
            ("sound_variant", Tag::String("minecraft:royal".to_owned())),
            ("CollarColor", Tag::Byte(11)),
            ("Sitting", Tag::Byte(1)),
            ("Owner", Tag::IntArray(vec![1, 2, 3, 4])),
        ]),
    ));
    out.push((
        "rich".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:cow".to_owned())),
            ("Pos", doubles([-12.75, 70.5, 300.125])),
            ("Motion", doubles([0.05, -0.0784000015258789, 11.0])),
            (
                "Rotation",
                Tag::List(vec![Tag::Float(-123.5), Tag::Float(12.25)]),
            ),
            ("UUID", uuid.clone()),
            ("fall_distance", Tag::Double(2.5)),
            ("Fire", Tag::Short(37)),
            ("Air", Tag::Short(120)),
            ("OnGround", Tag::Byte(1)),
            ("Invulnerable", Tag::Byte(1)),
            ("PortalCooldown", Tag::Int(9)),
            ("invulnerable_time", Tag::Int(4)),
            ("CustomName", Tag::String("Bessie".to_owned())),
            ("CustomNameVisible", Tag::Byte(1)),
            ("Silent", Tag::Byte(1)),
            ("NoGravity", Tag::Byte(1)),
            ("Glowing", Tag::Byte(1)),
            ("TicksFrozen", Tag::Int(55)),
            ("Tags", Tag::List(vec![Tag::String("milked".to_owned())])),
            ("Health", Tag::Float(7.5)),
            ("HurtTime", Tag::Short(3)),
            ("DeathTime", Tag::Short(0)),
            ("AbsorptionAmount", Tag::Float(1.5)),
            (
                "attributes",
                Tag::List(vec![
                    compound(vec![
                        ("id", Tag::String("minecraft:movement_speed".to_owned())),
                        ("base", Tag::Double(0.25)),
                    ]),
                    compound(vec![
                        ("id", Tag::String("minecraft:follow_range".to_owned())),
                        ("base", Tag::Double(16.0)),
                        (
                            "modifiers",
                            Tag::List(vec![compound(vec![
                                ("id", Tag::String("minecraft:random_spawn_bonus".to_owned())),
                                ("amount", Tag::Double(0.0412)),
                                ("operation", Tag::String("add_multiplied_base".to_owned())),
                            ])]),
                        ),
                    ]),
                ]),
            ),
            ("FallFlying", Tag::Byte(0)),
            ("last_hurt_by_player", Tag::IntArray(vec![1, 2, 3, 4])),
            ("last_hurt_by_player_memory_time", Tag::Int(60)),
            ("last_hurt_by_mob", Tag::IntArray(vec![1, 2, 3, 4])),
            ("ticks_since_last_hurt_by_mob", Tag::Int(40)),
            ("CanPickUpLoot", Tag::Byte(0)),
            ("PersistenceRequired", Tag::Byte(1)),
            ("home_radius", Tag::Int(6)),
            ("home_pos", Tag::IntArray(vec![10, 64, -10])),
            ("LeftHanded", Tag::Byte(1)),
            ("DeathLootTableSeed", Tag::Long(-77)),
            ("NoAI", Tag::Byte(1)),
            ("Age", Tag::Int(-1200)),
            ("ForcedAge", Tag::Int(0)),
            ("AgeLocked", Tag::Byte(1)),
            ("InLove", Tag::Int(0)),
            ("LoveCause", Tag::IntArray(vec![9, 8, 7, 6])),
            ("variant", Tag::String("minecraft:warm".to_owned())),
            ("sound_variant", Tag::String("minecraft:moody".to_owned())),
            (
                "data",
                compound(vec![("owner", Tag::String("bamboo".to_owned()))]),
            ),
            ("Unknown", Tag::Int(5)),
        ]),
    ));
    out.push((
        "in_love_adult".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:cow".to_owned())),
            ("Pos", doubles([0.5, 64.0, 0.5])),
            ("UUID", uuid.clone()),
            ("InLove", Tag::Int(512)),
            ("Age", Tag::Int(4000)),
            ("variant", Tag::String("minecraft:cold".to_owned())),
        ]),
    ));
    // 快乐恶魂：`still_timeout` 无条件写；幼年 `Age`、BODY 挽具和元数据状态
    // 在重存时都要保持原样。
    // Happy ghasts: `still_timeout` is always written; baby `Age`, the BODY
    // harness and the synced flags survive a resave.
    out.push((
        "happy ghast bare".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:happy_ghast".to_owned())),
            ("Pos", doubles([3.5, 64.0, -7.25])),
            ("UUID", uuid.clone()),
        ]),
    ));
    out.push((
        "happy ghast harnessed and hovering".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:happy_ghast".to_owned())),
            ("Pos", doubles([3.5, 78.0, -7.25])),
            ("UUID", uuid.clone()),
            ("still_timeout", Tag::Int(10)),
            ("Health", Tag::Float(13.0)),
            ("NoAI", Tag::Byte(1)),
            (
                "equipment",
                compound(vec![(
                    "body",
                    compound(vec![
                        ("id", Tag::String("minecraft:red_harness".to_owned())),
                        ("count", Tag::Int(1)),
                    ]),
                )]),
            ),
        ]),
    ));
    out.push((
        "happy ghast baby".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:happy_ghast".to_owned())),
            ("Pos", doubles([3.5, 64.0, -7.25])),
            ("UUID", uuid.clone()),
            ("Age", Tag::Int(-24000)),
            ("ForcedAge", Tag::Int(1200)),
            ("still_timeout", Tag::Int(0)),
        ]),
    ));
    out.extend(brain_resave_cases());
    // 焦骸没有自己的存档键（`AbstractSkeleton.readAdditionalSaveData` 只调 super
    // 再重挑武器目标），尸壳骆驼和普通骆驼共用 `LastPoseTick`。
    // Parched has no own save keys; camel husks share `LastPoseTick` with camels.
    out.push((
        "parched_bare".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:parched".to_owned())),
            ("Pos", doubles([3.5, 64.0, -7.25])),
            ("UUID", uuid.clone()),
        ]),
    ));
    out.push((
        "parched_equipped".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:parched".to_owned())),
            ("Pos", doubles([1.5, 64.0, 2.5])),
            ("UUID", uuid.clone()),
            (
                "Rotation",
                Tag::List(vec![Tag::Float(37.0), Tag::Float(-12.0)]),
            ),
            ("Health", Tag::Float(9.0)),
            ("LeftHanded", Tag::Byte(1)),
            ("CanPickUpLoot", Tag::Byte(1)),
            ("PersistenceRequired", Tag::Byte(1)),
            (
                "HandItems",
                Tag::List(vec![
                    compound(vec![
                        ("id", Tag::String("minecraft:bow".to_owned())),
                        ("count", Tag::Int(1)),
                    ]),
                    compound(vec![]),
                ]),
            ),
        ]),
    ));
    out.push((
        "camel_husk_bare".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:camel_husk".to_owned())),
            ("Pos", doubles([3.5, 64.0, -7.25])),
            ("UUID", uuid.clone()),
        ]),
    ));
    out.push((
        "camel_husk_sitting".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:camel_husk".to_owned())),
            ("Pos", doubles([3.5, 64.0, -7.25])),
            ("UUID", uuid),
            ("LastPoseTick", Tag::Long(-1000)),
        ]),
    ));
    // 拴绳存档往返：官方 `loadEntityRecursive -> save` 与 Bamboo `read_entity ->
    // write_entity` 逐键比。`LeashData.CODEC` 的两种形态都要还原：
    // 实体牵引者 `{UUID:[I;...]}`，拴绳结 `[I;x,y,z]`。
    let leash_uuid = Tag::IntArray(vec![0x1111_2222, -0x3333_4444, 0x5555_6666, 0x0777_0888]);
    let leash_cow = |leash: Tag| {
        compound(vec![
            ("id", Tag::String("minecraft:cow".to_owned())),
            ("Pos", doubles([3.5, 64.0, -7.25])),
            ("UUID", mc_bamboo_blocks::archive::entity_uuid_tag(99)),
            ("leash", leash),
        ])
    };
    out.push((
        "leash entity holder".to_owned(),
        leash_cow(compound(vec![("UUID", leash_uuid.clone())])),
    ));
    out.push((
        "leash knot holder".to_owned(),
        leash_cow(Tag::IntArray(vec![7, 64, -3])),
    ));
    // 一头 Bamboo 生成后写出的拴绳牛，验证 `leash_tag` 的写出格式能被官方逐键还原。
    {
        let mut store = mc_bamboo_blocks::EntityStore::new();
        let uid = store.spawn(Entity::cow(Vec3::new(1.5, 64.0, 2.5)));
        let mut entity = store.get(uid).expect("leash cow");
        entity.kind.mob_mut().unwrap().leash =
            Some(mc_bamboo_blocks::entity::mob::leash::LeashData::new(
                1,
                mc_bamboo_blocks::entity::mob::leash::LeashDelayed::Entity([0x11; 16]),
            ));
        out.push((
            "leash spawned entity".to_owned(),
            write_entity(&entity.split()).expect("leash cow writes"),
        ));
        let uid2 = store.spawn(Entity::cow(Vec3::new(1.5, 64.0, 2.5)));
        let mut entity2 = store.get(uid2).expect("leash cow");
        entity2.kind.mob_mut().unwrap().leash =
            Some(mc_bamboo_blocks::entity::mob::leash::LeashData::new(
                2,
                mc_bamboo_blocks::entity::mob::leash::LeashDelayed::Knot(BlockPos::new(9, 70, -5)),
            ));
        out.push((
            "leash spawned knot".to_owned(),
            write_entity(&entity2.split()).expect("leash cow writes"),
        ));
    }
    let mut store = mc_bamboo_blocks::EntityStore::new();
    let uid = store.spawn(Entity::cow(Vec3::new(8.5, 65.0, 8.5)));
    let row = store.get(uid).expect("刚生成").split();
    out.push(("spawned".to_owned(), write_entity(&row).expect("牛能写出")));
    // 经验球的重存：裸条目、带富字段、Bamboo 刚生成又写出。
    // 非生物实体的 UUID 由网络 ID 派生（存档和网络层共用），所以题里用同一个推导。
    let orb_uuid = mc_bamboo_blocks::archive::entity_uuid_tag(99);
    out.push((
        "orb_bare".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:experience_orb".to_owned())),
            ("Pos", doubles([1.5, 70.0, -2.25])),
            ("UUID", orb_uuid.clone()),
        ]),
    ));
    out.push((
        "orb_rich".to_owned(),
        compound(vec![
            ("id", Tag::String("minecraft:experience_orb".to_owned())),
            ("Pos", doubles([-3.25, 66.5, 12.75])),
            ("Motion", doubles([0.02, -0.05, 0.01])),
            (
                "Rotation",
                Tag::List(vec![Tag::Float(17.5), Tag::Float(0.0)]),
            ),
            ("UUID", orb_uuid),
            ("fall_distance", Tag::Double(0.75)),
            ("Fire", Tag::Short(11)),
            ("OnGround", Tag::Byte(1)),
            ("Health", Tag::Short(3)),
            ("Age", Tag::Short(1200)),
            ("Value", Tag::Short(37)),
            ("Count", Tag::Int(4)),
        ]),
    ));
    let orb_uid = store.spawn(Entity::experience_orb(Vec3::new(2.5, 64.0, 2.5), 7));
    let mut orb_row = store.get(orb_uid).expect("刚生成").split();
    // 重存题用 uid 99 读回，UUID 由 uid 派生，写题时也用同一个 uid。
    orb_row.public.uid = 99;
    out.push((
        "orb_spawned".to_owned(),
        write_entity(&orb_row).expect("经验球能写出"),
    ));
    out
}

/// 跑大脑的生物的重存题：山羊的三个布尔值、犰狳的状态和掉鳞甲计时，以及 `Brain` 里
/// 带和不带 `ttl` 的记忆；没有编解码器的记忆（`ram_target`）读档时丢掉。
///
/// Resave cases for brain-driven mobs: the goat's three booleans, the armadillo state
/// and scute timer, and `Brain` memories with and without `ttl`; memories without a codec
/// (`ram_target`) are dropped on load.
fn brain_resave_cases() -> Vec<(String, Tag)> {
    let compound = |entries: Vec<(&str, Tag)>| {
        Tag::Compound(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    };
    let uuid = Tag::IntArray(vec![0x0bad_f00d, 0x1357_9bdf, -0x2468_ace0, 0x600d_0001]);
    let pos = Tag::List(vec![Tag::Double(4.5), Tag::Double(64.0), Tag::Double(-2.5)]);
    let memory = |value: Tag, ttl: Option<i64>| {
        let mut entries = vec![("value", value)];
        if let Some(ttl) = ttl {
            entries.push(("ttl", Tag::Long(ttl)));
        }
        compound(entries)
    };
    let goat_memories = compound(vec![(
        "memories",
        compound(vec![
            ("minecraft:ram_cooldown_ticks", memory(Tag::Int(321), None)),
            (
                "minecraft:long_jump_cooling_down",
                memory(Tag::Int(45), None),
            ),
            (
                "minecraft:ram_target",
                memory(
                    Tag::List(vec![Tag::Double(1.0), Tag::Double(2.0), Tag::Double(3.0)]),
                    None,
                ),
            ),
        ]),
    )]);
    let armadillo_memories = compound(vec![(
        "memories",
        compound(vec![
            (
                "minecraft:danger_detected_recently",
                memory(Tag::Byte(1), Some(57)),
            ),
            (
                "minecraft:temptation_cooldown_ticks",
                memory(Tag::Int(90), None),
            ),
        ]),
    )]);
    let frog_memories = compound(vec![(
        "memories",
        compound(vec![
            ("minecraft:is_pregnant", memory(compound(vec![]), None)),
            ("minecraft:is_in_water", memory(compound(vec![]), None)),
            (
                "minecraft:long_jump_cooling_down",
                memory(Tag::Int(77), None),
            ),
        ]),
    )]);
    vec![
        // 青蛙：品种存带命名空间的名字；`canBeABaby` 为假，`Age` 三个键不读也不写。
        // Frogs: the variant is saved namespaced; `canBeABaby` is false, so the three
        // `Age` keys are neither read nor written.
        (
            "frog_bare".to_owned(),
            compound(vec![
                ("id", Tag::String("minecraft:frog".to_owned())),
                ("Pos", pos.clone()),
                ("UUID", uuid.clone()),
            ]),
        ),
        (
            "frog_warm_pregnant".to_owned(),
            compound(vec![
                ("id", Tag::String("minecraft:frog".to_owned())),
                ("Pos", pos.clone()),
                ("UUID", uuid.clone()),
                ("variant", Tag::String("minecraft:warm".to_owned())),
                ("Age", Tag::Int(-600)),
                ("Brain", frog_memories),
            ]),
        ),
        // 蝌蚪：自己的 `Age` 和 `AgeLocked`；`FromBucket` 写出来恒为真。
        // Tadpoles: their own `Age` and `AgeLocked`; `FromBucket` always saves as true.
        (
            "tadpole_bare".to_owned(),
            compound(vec![
                ("id", Tag::String("minecraft:tadpole".to_owned())),
                ("Pos", pos.clone()),
                ("UUID", uuid.clone()),
            ]),
        ),
        (
            "tadpole_aged_locked".to_owned(),
            compound(vec![
                ("id", Tag::String("minecraft:tadpole".to_owned())),
                ("Pos", pos.clone()),
                ("UUID", uuid.clone()),
                ("Age", Tag::Int(12345)),
                ("AgeLocked", Tag::Byte(1)),
                ("FromBucket", Tag::Byte(0)),
            ]),
        ),
        (
            "goat_bare".to_owned(),
            compound(vec![
                ("id", Tag::String("minecraft:goat".to_owned())),
                ("Pos", pos.clone()),
                ("UUID", uuid.clone()),
            ]),
        ),
        (
            "goat_screaming_one_horn".to_owned(),
            compound(vec![
                ("id", Tag::String("minecraft:goat".to_owned())),
                ("Pos", pos.clone()),
                (
                    "Rotation",
                    Tag::List(vec![Tag::Float(140.0), Tag::Float(-5.0)]),
                ),
                ("UUID", uuid.clone()),
                ("IsScreamingGoat", Tag::Byte(1)),
                ("HasLeftHorn", Tag::Byte(0)),
                ("HasRightHorn", Tag::Byte(1)),
                ("Age", Tag::Int(-600)),
                ("Brain", goat_memories),
            ]),
        ),
        // 骆驼：`LastPoseTick` 负数表示坐着，正数表示上次换姿势的 `gameTime`。
        // Camels: a negative `LastPoseTick` means sitting; a positive one is the
        // `gameTime` of the last pose change.
        (
            "camel_bare".to_owned(),
            compound(vec![
                ("id", Tag::String("minecraft:camel".to_owned())),
                ("Pos", pos.clone()),
                ("UUID", uuid.clone()),
            ]),
        ),
        (
            "camel_sitting".to_owned(),
            compound(vec![
                ("id", Tag::String("minecraft:camel".to_owned())),
                ("Pos", pos.clone()),
                ("UUID", uuid.clone()),
                ("LastPoseTick", Tag::Long(-1000)),
            ]),
        ),
        (
            "armadillo_scared".to_owned(),
            compound(vec![
                ("id", Tag::String("minecraft:armadillo".to_owned())),
                ("Pos", pos),
                ("UUID", uuid),
                ("state", Tag::String("scared".to_owned())),
                ("scute_time", Tag::Int(1234)),
                ("Brain", armadillo_memories),
            ]),
        ),
    ]
}

/// 把规范 JSON 里 `attributes` 列表按 `id` 排序。
fn sorted_attributes(mut value: Value) -> Value {
    if let Some(Value::Array(list)) = value
        .get_mut("values")
        .and_then(|values| values.get_mut("attributes"))
        .and_then(|attributes| attributes.get_mut("values"))
    {
        list.sort_by_key(|entry| {
            entry["values"]["id"]["value"]
                .as_str()
                .unwrap_or("")
                .to_owned()
        });
    }
    value
}

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
fn first_difference(left: &Value, right: &Value) -> String {
    if left == right {
        return "equal".to_owned();
    }
    match (left, right) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys()) {
                match (a.get(key), b.get(key)) {
                    (Some(a), Some(b)) if a != b => {
                        return format!("{key}: {}", first_difference(a, b));
                    }
                    (None, _) => return format!("{key} missing on the official side"),
                    (_, None) => return format!("{key} missing on the Bamboo side"),
                    _ => {}
                }
            }
            "objects differ".to_owned()
        }
        (Value::Array(a), Value::Array(b)) => {
            format!("lists of {} vs {}", a.len(), b.len())
        }
        _ => format!("{left} vs {right}"),
    }
}

fn canonical_tag(tag: &Tag) -> Value {
    match tag {
        Tag::Byte(value) => json!({"type": 1, "value": value.to_string()}),
        Tag::Short(value) => json!({"type": 2, "value": value.to_string()}),
        Tag::Int(value) => json!({"type": 3, "value": value.to_string()}),
        Tag::Long(value) => json!({"type": 4, "value": value.to_string()}),
        // Java 的 `Float.toString()` 对整数也带小数点，Rust 的 `1.0` 是 "1"。
        // Java's `Float.toString()` keeps the decimal point for whole values too.
        Tag::Float(value) => {
            let mut text = value.to_string();
            if !text.contains('.') && !text.contains('e') && !text.contains('E') {
                text.push_str(".0");
            }
            json!({"type": 5, "value": text})
        }
        // Java 的 `Double.toString()` 总带小数点；Rust 的 `3.0.to_string()` 是 "3"。
        // Java's `Double.toString()` keeps the decimal point even for whole values.
        Tag::Double(value) => {
            let mut text = value.to_string();
            if !text.contains('.') && !text.contains('e') && !text.contains('E') {
                text.push_str(".0");
            }
            json!({"type": 6, "value": text})
        }
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

fn decode_b64(text: &str) -> Result<Vec<u8>, String> {
    crate::storage_oracle::decode_b64(text)
}

#[allow(dead_code, reason = "展示时统计用")]
fn _unused(_map: &BTreeMap<String, usize>, _ticks: &ChunkTicks) {}
