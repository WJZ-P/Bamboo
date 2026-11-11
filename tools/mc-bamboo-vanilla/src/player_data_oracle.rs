//! 用官方 26.3 的类检查带槽位物品、玩家数据和 `level.dat` 的编解码。
//!
//! `PlayerDataOracle.java` 在 JVM 里直接调官方的 `ItemStack.CODEC` /
//! `ItemStackWithSlot.CODEC`、`TagValueInput` 和 `PrimaryLevelData.parse` /
//! `createTag`，不启动服务端、不接受 EULA。stdin 一行一个 JSON 请求，stdout 一行
//! 一个 JSON 结果；官方的 `CompoundTag` 用 `HashMap`，键序是哈希序，所以按键
//! 排序的规范 JSON 比较。
//!
//! 每个用例四个方向都过一遍：Bamboo 写出的字节让官方读、官方写回来按键相等；
//! 官方写出的字节 Bamboo 读、再写回去按键相等；顺带逐字段比对官方
//! `TagValueInput` 读出来的值。
//!
//! Persistent item, player-data and level.dat codecs checked against the official
//! classes. No server is started and no EULA is accepted.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use mc_bamboo_data::{items::STONE, version};
use mc_bamboo_nbt::{Accounter, Tag, read_named, write_named};
use mc_bamboo_protocol::{
    components::archive::slots::{self, INVENTORY_SLOTS, SlottedItem},
    item::ItemStack,
};
use mc_bamboo_storage::{
    level_dat::{self, LevelData, SpawnData},
    player_data::{self, Abilities, PlayerData},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::oracle::collect_log;

const USAGE: &str = "usage: player-data <java> [--show N]";
const DEFAULT_SHOW: usize = 8;

#[derive(Debug, Deserialize)]
struct Response {
    #[allow(dead_code, reason = "the command name is part of the oracle protocol")]
    command: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    original: Option<Value>,
    #[serde(default)]
    written: Option<Value>,
    #[serde(default)]
    canonical: Option<Value>,
    #[serde(default)]
    nbt: Option<String>,
    #[serde(default, rename = "writtenNbt")]
    written_nbt: Option<String>,
    #[serde(default)]
    gzip: Option<String>,
    #[serde(default, rename = "gzipWritten")]
    gzip_written: Option<String>,
    #[serde(default)]
    problems: Vec<usize>,
    #[serde(default)]
    fields: Option<Value>,
}

#[derive(Debug, Default)]
struct Report {
    checks: usize,
    failures: Vec<String>,
    show: usize,
    cases: usize,
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
            println!(
                "player-data oracle: {} checks passed in {} cases",
                self.checks, self.cases
            );
            Ok(())
        } else {
            Err(format!(
                "player-data oracle: {} of {} checks failed",
                self.failures.len(),
                self.checks
            ))
        }
    }
}

/// 启动官方进程并跑完所有题目。`command` 已经是能运行 `PlayerDataOracle.java` 的命令。
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
    let mut report = Report {
        show,
        ..Default::default()
    };
    let mut lines = BufReader::new(stdout).lines();
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
        return Err(format!("PlayerDataOracle.java failed: {status}\n{logs}"));
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
        .ok_or("PlayerDataOracle.java closed stdout unexpectedly")?
        .map_err(|error| format!("cannot read from java: {error}"))?;
    let response: Response =
        serde_json::from_str(&answer).map_err(|error| format!("bad oracle response: {error}"))?;
    if let Some(error) = &response.error {
        return Err(format!("PlayerDataOracle.java: {error}"));
    }
    Ok(response)
}

fn run_cases(
    stdin: &mut impl Write,
    lines: &mut impl Iterator<Item = Result<String, std::io::Error>>,
    report: &mut Report,
) -> Result<(), String> {
    // 1. 物品（单个复合标签）。
    let cases = item_cases();
    for (name, stack, extra_keys) in &cases {
        let mut tag = slots::encode_item(stack).map_err(|error| format!("{name}: {error}"))?;
        if *extra_keys && let Tag::Compound(entries) = &mut tag {
            entries.push(("bamboo:extra".to_owned(), Tag::Int(77)));
        }
        let nbt = named_bytes(&tag);
        let answer = ask(
            stdin,
            lines,
            json!({"command": "item-read", "nbt": b64(&nbt)}),
        )?;
        report.cases += 1;
        report.check(
            answer.original.as_ref() == Some(&canonical_tag(&tag)),
            format!("{name}: official read of a Bamboo item tag differs"),
        );
        // 官方的 MapCodec 忽略多余的键，写回时不再出现。
        let written = answer
            .written_nbt
            .as_deref()
            .ok_or("item-read has no writtenNbt")?;
        let written_tag = parse_named(&decode_b64(written)?)?;
        let clean = slots::encode_item(stack).map_err(|error| error.to_string())?;
        report.check(
            tags_equal(&written_tag, &clean),
            format!("{name}: official item write-back differs from Bamboo's item tag"),
        );
        report.check(
            answer.written.as_ref() == Some(&canonical_tag(&clean)),
            format!("{name}: official item canonical JSON differs"),
        );
    }

    // 2. 带槽位的物品列表：空、满、带组件、数量边界 1 / 99、多余键、坏条目。
    for (name, items, bad) in list_cases() {
        let list = slots::encode_list(&items).map_err(|error| format!("{name}: {error}"))?;
        let mut root = Tag::Compound(vec![("Items".to_owned(), list.clone())]);
        if bad
            && let Tag::Compound(entries) = &mut root
            && let Some((_, Tag::List(values))) = entries.iter_mut().find(|(key, _)| key == "Items")
        {
            values.push(Tag::Int(-1)); // 解不开的条目
        }
        let answer = ask(
            stdin,
            lines,
            json!({"command": "item-read", "list": true, "nbt": b64(&named_bytes(&root))}),
        )?;
        report.cases += 1;
        report.check(
            answer.original.as_ref() == Some(&canonical_tag(&root)),
            format!("{name}: official read of a Bamboo item list differs"),
        );
        report.check(
            answer.problems.len() == usize::from(bad),
            format!(
                "{name}: official reported {:?} bad entries, expected {bad}",
                answer.problems
            ),
        );
        let written = answer
            .written_nbt
            .as_deref()
            .ok_or("item-read has no writtenNbt")?;
        let written_tag = parse_named(&decode_b64(written)?)?;
        report.check(
            tags_equal(
                &written_tag,
                &Tag::Compound(vec![("Items".to_owned(), list)]),
            ),
            format!("{name}: official item-list write-back differs"),
        );
        let decoded = slots::decode_list(
            field(&written_tag, "Items").ok_or("writtenNbt has no Items")?,
            INVENTORY_SLOTS,
        );
        report.check(
            decoded.items == items,
            format!("{name}: Bamboo re-decode differs"),
        );
        report.check(
            decoded.problems.is_empty(),
            format!("{name}: Bamboo re-decode of the official write-back has problems"),
        );
    }

    // 官方写出的物品列表，Bamboo 读进来再写出去按键相等。
    for (name, specs) in official_item_specs() {
        let answer = ask(
            stdin,
            lines,
            json!({"command": "item-write", "items": specs}),
        )?;
        report.cases += 1;
        let bytes = decode_b64(answer.nbt.as_deref().unwrap_or_default())?;
        let root = parse_named(&bytes)?;
        report.check(
            answer.canonical.as_ref() == Some(&canonical_tag(&root)),
            format!("{name}: official item-list canonical JSON differs"),
        );
        let items = field(&root, "Items").ok_or("official item list has no Items")?;
        let decoded = slots::decode_list(items, INVENTORY_SLOTS);
        let rewritten = slots::encode_list(&decoded.items).map_err(|error| error.to_string())?;
        report.check(
            tags_equal(items, &rewritten),
            format!("{name}: Bamboo item-list write-back differs"),
        );
    }

    // 3. 玩家数据：双向，逐字段用官方 TagValueInput 核对。
    for (name, data) in &player_cases() {
        let bytes = data.to_gzip().map_err(|error| format!("{name}: {error}"))?;
        let answer = ask(
            stdin,
            lines,
            json!({"command": "player-read", "gzip": b64(&bytes)}),
        )?;
        report.cases += 1;
        check_gzip(report, name, &bytes, &answer, "player-read", |gzip| {
            PlayerData::from_gzip(gzip)
                .and_then(|data| data.to_gzip())
                .map_err(|error| error.to_string())
        })?;
        check_player_fields(report, name, data, answer.fields.as_ref());

        // 官方写出的同一份内容，Bamboo 读进来再写出去按键相等。
        let official = ask(
            stdin,
            lines,
            json!({"command": "player-write", "player": player_spec(data)}),
        )?;
        report.cases += 1;
        let gzip = decode_b64(official.gzip.as_deref().unwrap_or_default())?;
        let decoded = PlayerData::from_gzip(&gzip).map_err(|error| format!("{name}: {error}"))?;
        let again = decoded.to_gzip().map_err(|error| error.to_string())?;
        let official_tag = gzip_tag(&gzip)?;
        let again_tag = gzip_tag(&again)?;
        report.check(
            canonical_tag(&official_tag) == canonical_tag(&again_tag),
            format!("{name}: Bamboo's player write-back differs from the official file"),
        );
        report.check(
            official.canonical.as_ref() == Some(&canonical_tag(&official_tag)),
            format!("{name}: official player canonical JSON differs"),
        );
        // D9：官方有而 Bamboo 不认识的键原样写回。
        if official_tag_has_unknown(&official_tag) {
            report.check(
                field(&again_tag, "bamboo:unknown") == field(&official_tag, "bamboo:unknown"),
                format!("{name}: unknown top-level key was not preserved"),
            );
        }
    }

    // 4. level.dat：双向，逐字段用官方 PrimaryLevelData.parse 核对。
    for (name, level) in &level_cases() {
        let bytes = level
            .to_gzip()
            .map_err(|error| format!("{name}: {error}"))?;
        let answer = ask(
            stdin,
            lines,
            json!({"command": "level-read", "gzip": b64(&bytes)}),
        )?;
        report.cases += 1;
        check_gzip(report, name, &bytes, &answer, "level-read", |gzip| {
            LevelData::from_gzip(gzip)
                .and_then(|level| level.to_gzip())
                .map_err(|error| error.to_string())
        })?;
        check_level_fields(report, name, level, answer.fields.as_ref());

        let official = ask(
            stdin,
            lines,
            json!({"command": "level-write", "level": level_spec(level)}),
        )?;
        report.cases += 1;
        let gzip = decode_b64(official.gzip.as_deref().unwrap_or_default())?;
        let decoded = LevelData::from_gzip(&gzip).map_err(|error| format!("{name}: {error}"))?;
        let again = decoded.to_gzip().map_err(|error| error.to_string())?;
        let official_tag = gzip_tag(&gzip)?;
        let again_tag = gzip_tag(&again)?;
        report.check(
            canonical_tag(&official_tag) == canonical_tag(&again_tag),
            format!("{name}: Bamboo's level.dat write-back differs from the official file"),
        );
        for key in ["bamboo:unknown", "bamboo:root"] {
            let official_value = field(&official_tag, key);
            if official_value.is_some() {
                report.check(
                    field(&again_tag, key) == official_value,
                    format!("{name}: unknown root key {key} was not preserved"),
                );
            }
        }
        report.check(
            official.canonical.as_ref() == Some(&canonical_tag(&official_tag)),
            format!("{name}: official level.dat canonical JSON differs"),
        );
        if let (Some(Tag::Compound(official_data)), Some(Tag::Compound(again_data))) = (
            field(&official_tag, "Data").cloned(),
            field(&again_tag, "Data").cloned(),
        ) && let Some(unknown) = field_of(&official_data, "bamboo:unknown")
        {
            report.check(
                field_of(&again_data, "bamboo:unknown") == Some(unknown),
                format!("{name}: unknown Data key was not preserved"),
            );
        }
    }
    Ok(())
}

/// Bamboo 写出的 gzip 文件：官方读、官方写回、Bamboo 再读再写，按键比较。
fn check_gzip(
    report: &mut Report,
    name: &str,
    original: &[u8],
    answer: &Response,
    command: &str,
    round_trip: impl Fn(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    let original_tag = gzip_tag(original)?;
    report.check(
        answer.original.as_ref() == Some(&canonical_tag(&original_tag)),
        format!("{name}: official read of Bamboo's gzip differs ({command})"),
    );
    let written = answer
        .gzip_written
        .as_deref()
        .ok_or(format!("{command} has no gzipWritten"))?;
    let written_bytes = decode_b64(written)?;
    let written_tag = gzip_tag(&written_bytes)?;
    report.check(
        answer.written.as_ref() == Some(&canonical_tag(&written_tag)),
        format!("{name}: official write-back canonical JSON differs ({command})"),
    );
    let again = round_trip(&written_bytes)?;
    let again_tag = gzip_tag(&again)?;
    let left = canonical_tag(&written_tag);
    let right = canonical_tag(&again_tag);
    report.check(
        left == right,
        format!(
            "{name}: Bamboo write-back of the official {command} differs: official={} again={}",
            serde_json::to_string(&left).unwrap_or_default(),
            serde_json::to_string(&right).unwrap_or_default()
        ),
    );
    Ok(())
}

fn check_player_fields(report: &mut Report, name: &str, data: &PlayerData, fields: Option<&Value>) {
    let Some(fields) = fields else {
        report.check(false, format!("{name}: player-read has no fields"));
        return;
    };
    let get = |key: &str| fields.get(key).cloned().unwrap_or(Value::Null);
    report.check(
        get("data_version") == json!(data.data_version),
        format!("{name}: DataVersion"),
    );
    report.check(get("air") == json!(data.air), format!("{name}: Air"));
    report.check(get("fire") == json!(data.fire), format!("{name}: Fire"));
    report.check(
        get("on_ground") == json!(data.on_ground),
        format!("{name}: OnGround"),
    );
    report.check(
        get("health").as_f64() == Some(f64::from(data.health)),
        format!(
            "{name}: Health official={:?} bamboo={:?}",
            get("health").as_f64(),
            f64::from(data.health)
        ),
    );
    report.check(
        get("xp_level") == json!(data.xp_level),
        format!("{name}: XpLevel"),
    );
    report.check(
        get("food_level") == json!(data.food_level),
        format!("{name}: foodLevel"),
    );
    report.check(
        get("selected_item_slot") == json!(data.selected_item_slot),
        format!("{name}: SelectedItemSlot"),
    );
    report.check(
        get("dimension") == json!(data.dimension),
        format!("{name}: Dimension"),
    );
    report.check(
        get("game_type") == json!(data.game_type),
        format!("{name}: playerGameType"),
    );
    report.check(
        get("seen_credits") == json!(data.seen_credits),
        format!("{name}: seenCredits"),
    );
    report.check(
        get("spawn_extra_particles_on_fall") == json!(data.spawn_extra_particles_on_fall),
        format!("{name}: spawn_extra_particles_on_fall"),
    );
    report.check(
        get("inventory")
            == canonical_tag(&slots::encode_list(&data.inventory).unwrap_or(Tag::List(Vec::new()))),
        format!("{name}: Inventory"),
    );
    report.check(
        get("ender_items")
            == canonical_tag(
                &slots::encode_list(&data.ender_items).unwrap_or(Tag::List(Vec::new())),
            ),
        format!("{name}: EnderItems"),
    );
    report.check(
        get("pos")
            == canonical_tag(&Tag::List(
                data.position.iter().map(|v| Tag::Double(*v)).collect(),
            )),
        format!("{name}: Pos"),
    );
    report.check(
        get("rotation")
            == canonical_tag(&Tag::List(
                data.rotation.iter().map(|v| Tag::Float(*v)).collect(),
            )),
        format!("{name}: Rotation"),
    );
    let abilities = canonical_tag(&Tag::Compound(vec![
        (
            "invulnerable".into(),
            Tag::Byte(i8::from(data.abilities.invulnerable)),
        ),
        ("flying".into(), Tag::Byte(i8::from(data.abilities.flying))),
        ("mayfly".into(), Tag::Byte(i8::from(data.abilities.may_fly))),
        (
            "instabuild".into(),
            Tag::Byte(i8::from(data.abilities.instabuild)),
        ),
        (
            "mayBuild".into(),
            Tag::Byte(i8::from(data.abilities.may_build)),
        ),
        ("flySpeed".into(), Tag::Float(data.abilities.fly_speed)),
        ("walkSpeed".into(), Tag::Float(data.abilities.walk_speed)),
    ]));
    report.check(get("abilities") == abilities, format!("{name}: abilities"));
}

fn check_level_fields(report: &mut Report, name: &str, level: &LevelData, fields: Option<&Value>) {
    let Some(fields) = fields else {
        report.check(false, format!("{name}: level-read has no fields"));
        return;
    };
    let get = |key: &str| fields.get(key).cloned().unwrap_or(Value::Null);
    report.check(
        get("level_name") == json!(level.level_name),
        format!("{name}: LevelName"),
    );
    report.check(
        get("game_type") == json!(level.game_type),
        format!("{name}: GameType"),
    );
    report.check(get("time") == json!(level.time), format!("{name}: Time"));
    report.check(
        get("allow_commands") == json!(level.allow_commands),
        format!("{name}: allowCommands"),
    );
    report.check(
        get("initialized") == json!(level.initialized),
        format!("{name}: initialized"),
    );
    let spawn = canonical_tag(&Tag::Compound(vec![
        (
            "dimension".into(),
            Tag::String(level.spawn.dimension.clone()),
        ),
        (
            "pos".into(),
            Tag::IntArray(vec![level.spawn.x, level.spawn.y, level.spawn.z]),
        ),
        ("yaw".into(), Tag::Float(level.spawn.yaw)),
        ("pitch".into(), Tag::Float(level.spawn.pitch)),
    ]));
    report.check(get("spawn") == spawn, format!("{name}: spawn"));
    let difficulty = canonical_tag(&Tag::Compound(vec![
        (
            "difficulty".into(),
            Tag::String(level.difficulty.difficulty.clone()),
        ),
        (
            "hardcore".into(),
            Tag::Byte(i8::from(level.difficulty.hardcore)),
        ),
        (
            "locked".into(),
            Tag::Byte(i8::from(level.difficulty.locked)),
        ),
    ]));
    report.check(
        get("difficulty_settings") == difficulty,
        format!("{name}: difficulty_settings"),
    );
}

fn item_cases() -> Vec<(String, ItemStack, bool)> {
    vec![
        ("item count 1".into(), ItemStack::new(STONE, 1), false),
        ("item count 99".into(), ItemStack::new(STONE, 99), false),
        ("item with damage component".into(), damaged(7), false),
        ("item with extra key".into(), ItemStack::new(STONE, 1), true),
    ]
}

fn damaged(count: i32) -> ItemStack {
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};
    let mut patch = ComponentPatch::new();
    patch.put(kinds::DAMAGE, PatchEntry::Set(ComponentValue::Damage(3)));
    let (added, removed) = patch.to_opaque();
    ItemStack {
        item: STONE,
        count,
        added,
        removed,
    }
}

fn list_cases() -> Vec<(String, Vec<SlottedItem>, bool)> {
    let full: Vec<_> = (0..INVENTORY_SLOTS)
        .map(|slot| SlottedItem {
            slot: slot as i32,
            stack: ItemStack::new(STONE, 1 + (slot % 99) as i32),
        })
        .collect();
    let mut with_component = full.clone();
    with_component[0] = SlottedItem {
        slot: 0,
        stack: damaged(99),
    };
    vec![
        ("empty inventory".into(), Vec::new(), false),
        (
            "count boundary 1".into(),
            vec![SlottedItem {
                slot: 0,
                stack: ItemStack::new(STONE, 1),
            }],
            false,
        ),
        (
            "count boundary 99".into(),
            vec![SlottedItem {
                slot: 35,
                stack: ItemStack::new(STONE, 99),
            }],
            false,
        ),
        ("full 36-slot inventory".into(), full, false),
        ("inventory with components".into(), with_component, false),
        (
            "inventory with one bad entry".into(),
            vec![
                SlottedItem {
                    slot: 0,
                    stack: ItemStack::new(STONE, 1),
                },
                SlottedItem {
                    slot: 8,
                    stack: damaged(99),
                },
            ],
            true,
        ),
    ]
}

fn official_item_specs() -> Vec<(String, Value)> {
    let mut full = Vec::new();
    for slot in 0..INVENTORY_SLOTS {
        full.push(json!({"slot": slot, "count": 1 + (slot % 99), "component": slot % 3 == 0}));
    }
    vec![
        ("official empty list".into(), json!([])),
        (
            "official count boundaries".into(),
            json!([{"slot": 0, "count": 1}, {"slot": 35, "count": 99, "component": true}]),
        ),
        ("official full list".into(), json!(full)),
    ]
}

fn player_cases() -> Vec<(String, PlayerData)> {
    let mut cases = Vec::new();
    for (name, game_type) in [
        ("survival", 0),
        ("creative", 1),
        ("adventure", 2),
        ("spectator", 3),
    ] {
        let data = PlayerData {
            data_version: version::DATA_VERSION,
            uuid: [0x1234_5678, -0x6543_2110, 0x0fed_cba9, -0x789a_bcdf],
            game_type,
            abilities: Abilities::for_game_type(game_type),
            dimension: player_data::OVERWORLD.to_owned(),
            ..PlayerData::default()
        };
        cases.push((format!("player {name}"), data));
    }
    let mut empty = cases[0].1.clone();
    empty.inventory = Vec::new();
    empty.ender_items = Vec::new();
    cases.push(("player empty inventory".into(), empty));

    let mut full = cases[1].1.clone();
    full.inventory = (0..INVENTORY_SLOTS)
        .map(|slot| SlottedItem {
            slot: slot as i32,
            stack: if slot % 3 == 0 {
                damaged(99)
            } else {
                ItemStack::new(STONE, 1)
            },
        })
        .collect();
    full.ender_items = vec![SlottedItem {
        slot: 26,
        stack: damaged(1),
    }];
    full.selected_item_slot = 8;
    cases.push(("player full inventory with components".into(), full));

    let mut extreme = cases[2].1.clone();
    extreme.position = [1.0e300, -64.0, -1.0e300];
    extreme.motion = [-0.5, 0.0, 1.0 / 3.0];
    extreme.rotation = [-179.5, 89.5];
    extreme.fall_distance = 1.0e100;
    extreme.on_ground = true;
    extreme.portal_cooldown = i32::MAX;
    extreme.xp_total = 12345;
    extreme.xp_seed = -77;
    extreme.score = -99;
    extreme.abilities.fly_speed = 0.25;
    extreme.abilities.walk_speed = 0.5;
    cases.push(("player extreme coordinates".into(), extreme));

    let mut unknown = cases[3].1.clone();
    let mut tag = unknown.encode();
    if let Tag::Compound(entries) = &mut tag {
        entries.push(("bamboo:unknown".into(), Tag::String("keep".into())));
        if let Some((_, Tag::Compound(abilities))) =
            entries.iter_mut().find(|(key, _)| key == "abilities")
        {
            abilities.push(("bamboo:nested".into(), Tag::Long(9)));
        }
    }
    let mut plain = Vec::new();
    write_named("", &tag, &mut plain);
    unknown = PlayerData::from_named(&plain).expect("unknown keys decode");
    cases.push(("player unknown keys".into(), unknown));
    cases
}

fn player_spec(data: &PlayerData) -> Value {
    let abilities = json!({
        "invulnerable": data.abilities.invulnerable,
        "flying": data.abilities.flying,
        "may_fly": data.abilities.may_fly,
        "instabuild": data.abilities.instabuild,
        "may_build": data.abilities.may_build,
        "fly_speed": data.abilities.fly_speed,
        "walk_speed": data.abilities.walk_speed,
    });
    let items = |items: &[SlottedItem]| -> Value {
        items
            .iter()
            .map(|item| {
                let component = !item.stack.added.is_empty() || !item.stack.removed.is_empty();
                json!({"slot": item.slot, "count": item.stack.count, "component": component})
            })
            .collect::<Vec<_>>()
            .into()
    };
    let mut spec = json!({
        "pos": data.position,
        "motion": data.motion,
        "rotation": data.rotation,
        "fall_distance": data.fall_distance,
        "fire": data.fire,
        "air": data.air,
        "on_ground": data.on_ground,
        "invulnerable": data.invulnerable,
        "portal_cooldown": data.portal_cooldown,
        "uuid_high": uuid_high(data.uuid),
        "uuid_low": uuid_low(data.uuid),
        "health": data.health,
        "hurt_time": data.hurt_time,
        "death_time": data.death_time,
        "absorption": data.absorption,
        "fall_flying": data.fall_flying,
        "inventory": items(&data.inventory),
        "selected_item_slot": data.selected_item_slot,
        "sleep_timer": data.sleep_timer,
        "xp_p": data.xp_p,
        "xp_level": data.xp_level,
        "xp_total": data.xp_total,
        "xp_seed": data.xp_seed,
        "score": data.score,
        "food_level": data.food_level,
        "food_tick_timer": data.food_tick_timer,
        "food_saturation": data.food_saturation,
        "food_exhaustion": data.food_exhaustion,
        "abilities": abilities,
        "ender_items": items(&data.ender_items),
        "dimension": data.dimension,
        "game_type": data.game_type,
        "seen_credits": data.seen_credits,
        "spawn_extra_particles_on_fall": data.spawn_extra_particles_on_fall,
    });
    if let Some(previous) = data.previous_game_type {
        spec["previous_game_type"] = json!(previous);
    }
    if !data.preserved.is_empty() {
        spec["unknown_tag"] = json!("keep");
    }
    spec
}

fn level_cases() -> Vec<(String, LevelData)> {
    let mut cases = Vec::new();
    for (name, game_type) in [
        ("survival", 0),
        ("creative", 1),
        ("adventure", 2),
        ("spectator", 3),
    ] {
        let level = LevelData {
            game_type,
            level_name: format!("bamboo-{name}"),
            time: 12345 + i64::from(game_type),
            last_played: 1_700_000_000_000,
            spawn: SpawnData {
                dimension: level_dat::OVERWORLD.to_owned(),
                x: 8,
                y: -60,
                z: 8,
                yaw: 0.0,
                pitch: 0.0,
            },
            ..LevelData::default()
        };
        cases.push((format!("level {name}"), level));
    }
    let mut extreme = cases[2].1.clone();
    extreme.spawn = SpawnData {
        x: 30_000_000,
        y: -2032,
        z: -30_000_000,
        yaw: 179.5,
        pitch: -90.0,
        dimension: level_dat::OVERWORLD.to_owned(),
    };
    extreme.time = 1_000_000_000_007;
    extreme.last_played = 1_700_000_000_123;
    extreme.level_name = "竹 bamboo".to_owned();
    extreme.difficulty = level_dat::DifficultySettings {
        difficulty: "hard".into(),
        hardcore: true,
        locked: true,
    };
    extreme.version_history = vec![4000, 5023];
    extreme.allow_commands = true;
    extreme.initialized = false;
    cases.push(("level extreme values".into(), extreme));

    // 官方有而 Bamboo 不认识的键：顶层、Data、Version 各放一个。
    let mut unknown = cases[0].1.clone();
    let mut root = unknown.encode_root();
    if let Tag::Compound(entries) = &mut root {
        entries.push(("bamboo:root".into(), Tag::String("keep".into())));
        if let Some((_, Tag::Compound(data))) = entries.iter_mut().find(|(key, _)| key == "Data") {
            data.push(("bamboo:data".into(), Tag::Long(-7)));
            if let Some((_, Tag::Compound(version))) =
                data.iter_mut().find(|(key, _)| key == "Version")
            {
                version.push(("bamboo:version".into(), Tag::Byte(3)));
            }
        }
    }
    let mut plain = Vec::new();
    write_named("", &root, &mut plain);
    unknown = LevelData::from_named(&plain).expect("unknown keys decode");
    cases.push(("level unknown keys".into(), unknown));
    cases
}

fn level_spec(level: &LevelData) -> Value {
    let mut spec = json!({
        "level_name": level.level_name,
        "game_type": level.game_type,
        "difficulty": level.difficulty.difficulty,
        "hardcore": level.difficulty.hardcore,
        "locked": level.difficulty.locked,
        "allow_commands": level.allow_commands,
        "initialized": level.initialized,
        "time": level.time,
        "last_played": level.last_played,
        "spawn": {
            "x": level.spawn.x, "y": level.spawn.y, "z": level.spawn.z,
            "yaw": level.spawn.yaw, "pitch": level.spawn.pitch,
        },
    });
    if !level.preserved.is_empty() {
        spec["unknown_tag"] = json!("keep");
    }
    spec
}

fn uuid_high(uuid: [i32; 4]) -> i64 {
    (i64::from(uuid[0]) << 32) | (i64::from(uuid[1]) & 0xffff_ffff)
}

fn uuid_low(uuid: [i32; 4]) -> i64 {
    (i64::from(uuid[2]) << 32) | (i64::from(uuid[3]) & 0xffff_ffff)
}

fn official_tag_has_unknown(tag: &Tag) -> bool {
    field(tag, "bamboo:unknown").is_some()
}

fn field_of<'a>(entries: &'a [(String, Tag)], key: &str) -> Option<&'a Tag> {
    entries
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn field<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    match tag {
        Tag::Compound(entries) => field_of(entries, key),
        _ => None,
    }
}

fn named_bytes(tag: &Tag) -> Vec<u8> {
    let mut out = Vec::new();
    write_named("", tag, &mut out);
    out
}

fn parse_named(bytes: &[u8]) -> Result<Tag, String> {
    let (name, entries) = read_named(bytes, Accounter::unlimited()).map_err(|e| e.to_string())?;
    if !name.is_empty() {
        return Err(format!("root name {name:?} is not empty"));
    }
    Ok(Tag::Compound(entries))
}

fn gzip_tag(bytes: &[u8]) -> Result<Tag, String> {
    let (name, entries) = mc_bamboo_nbt::read_compressed(
        bytes,
        mc_bamboo_nbt::Compression::Gzip,
        Accounter::unlimited(),
    )
    .map_err(|e| e.to_string())?;
    if !name.is_empty() {
        return Err(format!("gzip root name {name:?} is not empty"));
    }
    Ok(Tag::Compound(entries))
}

/// 官方 `String.valueOf(float)` 出来的文本，和 `NumericTag.box().toString()` 一致。
fn java_f32(value: f32) -> String {
    mc_bamboo_nbt::java_float_to_string(value)
}

fn tags_equal(left: &Tag, right: &Tag) -> bool {
    canonical_tag(left) == canonical_tag(right)
}

fn canonical_tag(tag: &Tag) -> Value {
    match tag {
        Tag::Byte(value) => json!({"type": 1, "value": value.to_string()}),
        Tag::Short(value) => json!({"type": 2, "value": value.to_string()}),
        Tag::Int(value) => json!({"type": 3, "value": value.to_string()}),
        Tag::Long(value) => json!({"type": 4, "value": value.to_string()}),
        Tag::Float(value) => json!({"type": 5, "value": java_f32(*value)}),
        Tag::Double(value) => {
            json!({"type": 6, "value": mc_bamboo_nbt::java_double_to_string(*value)})
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
            let mut sorted: Vec<_> = entries.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            let mut map = serde_json::Map::new();
            for (key, value) in sorted {
                map.insert(key.clone(), canonical_tag(value));
            }
            json!({"type": 10, "values": Value::Object(map)})
        }
    }
}

fn b64(bytes: &[u8]) -> String {
    use base64_stub::encode;
    encode(bytes)
}

fn decode_b64(text: &str) -> Result<Vec<u8>, String> {
    base64_stub::decode(text)
}

/// 自带的 base64，避免为这个开发工具再引入一个依赖。
mod base64_stub {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(input: &[u8]) -> String {
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let a = chunk[0] as u32;
            let b = chunk.get(1).copied().unwrap_or(0) as u32;
            let c = chunk.get(2).copied().unwrap_or(0) as u32;
            let value = a << 16 | b << 8 | c;
            out.push(ALPHABET[(value >> 18) as usize & 63] as char);
            out.push(ALPHABET[(value >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                ALPHABET[(value >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                ALPHABET[value as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }

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
