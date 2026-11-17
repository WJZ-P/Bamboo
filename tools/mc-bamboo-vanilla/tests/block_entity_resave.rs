//! 宝库、幽匿感测器和熔炉的方块实体经 Bamboo「读盘 → 存盘」后的逐键比较。
//!
//! 官方键名、默认值和 codec 的省略规则按 `javap -c -p` 核对：
//!
//! * `AbstractFurnaceBlockEntity.saveAdditional`：`cooking_time_spent` / `cooking_total_time` /
//!   `lit_time_remaining` / `lit_total_time`（`putInt`）、`speed_multiplier`（`putFloat`）、
//!   `Items`、`RecipesUsed`；
//! * `VaultBlockEntity.saveAdditional`：`config` / `shared_data` / `server_data` 三个 codec，
//!   新建的宝库三个都写成空复合；带默认值的字段在 codec 编码时省略；
//! * `SculkSensorBlockEntity.saveAdditional`：`last_vibration_frequency`（`putInt`，默认 0）与
//!   `listener`（`VibrationSystem.Data.CODEC`，默认 `{selector:{tick:-1L},event_delay:0}`）；
//!   校准幽匿传感器沿用父类；
//! * `SculkShriekerBlockEntity.saveAdditional`：`warning_level`（`putInt`，默认 0）与同一个
//!   `listener`。
//!
//! 这里比的是 Bamboo 读盘再存盘的 NBT 键和值；`chunk-content` oracle 另外用官方
//! `BlockEntity.loadStatic → saveWithFullMetadata` 跑同一批边界条目，逐键对照官方结果。
//!
//! Per-key comparison of vault, sculk and furnace block entities after Bamboo's
//! load -> save. The `chunk-content` oracle additionally replays the same edge
//! cases through vanilla's `loadStatic -> saveWithFullMetadata`.

#![cfg(feature = "oracle")]

use mc_bamboo_core::ChunkPos;
use mc_bamboo_nbt::{Accounter, Tag, read_named};

const POS: ChunkPos = ChunkPos { x: 3, z: -9 };

fn field<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    match tag {
        Tag::Compound(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn field_mut<'a>(entries: &'a mut [(String, Tag)], key: &str) -> Option<&'a mut Tag> {
    entries.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// 官方样例区块，把 `Y = -4` 那一节换成「空气 + 六种方块实体方块」，并只留给定的
/// 方块实体条目。方块放在本区块 `(0..6, -60, 0)`。
///
/// The official sample chunk with section `Y = -4` replaced by air plus the
/// block-entity blocks at `(0..6, -60, 0)`, keeping only the given entries.
fn chunk_with(entries: &[Tag]) -> Tag {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/official-chunk.nbt"
    ))
    .expect("official sample chunk");
    let (_, mut root) = read_named(&bytes, Accounter::unlimited()).expect("sample chunk parses");
    let names = [
        "minecraft:air",
        "minecraft:vault",
        "minecraft:furnace",
        "minecraft:sculk_sensor",
        "minecraft:calibrated_sculk_sensor",
        "minecraft:sculk_shrieker",
        "minecraft:spawner",
        "minecraft:oak_sign",
        "minecraft:oak_hanging_sign",
        "minecraft:beehive",
        "minecraft:jigsaw",
        "minecraft:end_gateway",
        "minecraft:sculk_catalyst",
        "minecraft:beacon",
        "minecraft:potent_sulfur",
        "minecraft:structure_block",
        "minecraft:ender_chest",
        "minecraft:jukebox",
        "minecraft:shulker_box",
        "minecraft:decorated_pot",
        "minecraft:oak_shelf",
        "minecraft:command_block",
        "minecraft:test_block",
        "minecraft:test_instance_block",
    ];
    let palette = names
        .iter()
        .map(|name| Tag::Compound(vec![("Name".to_owned(), Tag::String((*name).to_owned()))]))
        .collect();
    // 24 个调色板项按官方磁盘配置用 5 位：每格 5 位，一个 long 装 12 格，
    // 整段 342 个 long。
    // 24 palette entries use 5 bits per cell: 12 cells per long, 342 longs.
    let base = 4 * 256;
    let mut data = vec![0_i64; 342];
    // 一行只有 16 格：前 15 个方块实体放第 0 行（下标 1..15），后 8 个放第 1 行
    // 的前 8 格。容器下标是 (y*256 + z*16 + x)。
    // A row is 16 cells: the first 15 block entities sit on row 0 (indices 1..15),
    // the last 8 on row 1. Container index = y*256 + z*16 + x.
    for (slot, palette_index) in (1_i64..=23).enumerate() {
        let (x, z) = if slot < 15 {
            (slot as i64 + 1, 0_i64)
        } else {
            ((slot - 15) as i64, 1_i64)
        };
        let index = base + z * 16 + x;
        let index = index as usize;
        data[index / 12] |= palette_index << ((index % 12) * 5);
    }
    let block_states = Tag::Compound(vec![
        ("palette".to_owned(), Tag::List(palette)),
        ("data".to_owned(), Tag::LongArray(data)),
    ]);
    let sections = field_mut(&mut root, "sections").expect("sections");
    let Tag::List(sections) = sections else {
        panic!("sections is a list");
    };
    let section = sections
        .iter_mut()
        .find(|section| matches!(field(section, "Y"), Some(Tag::Byte(-4))))
        .expect("section Y = -4");
    let Tag::Compound(section) = section else {
        panic!("section is a compound");
    };
    *field_mut(section, "block_states").expect("block_states") = block_states;
    root.retain(|(key, _)| key != "block_entities");
    root.push(("block_entities".to_owned(), Tag::List(entries.to_vec())));
    Tag::Compound(root)
}

/// 按 `bare_defaults` 的顺序把方块实体放进两行；返回区块内 `(x, z)`。
fn position_of(id: &str) -> (i32, i32) {
    let slot = x_of(id);
    if slot < 15 {
        (slot + 1, 0)
    } else {
        (slot - 15, 1)
    }
}

fn entry(id: &str, x: i32, extra: Vec<(String, Tag)>) -> Tag {
    let _ = x;
    let (local_x, local_z) = position_of(id);
    let mut fields = vec![
        ("id".to_owned(), Tag::String(id.to_owned())),
        ("x".to_owned(), Tag::Int(POS.x * 16 + local_x)),
        ("y".to_owned(), Tag::Int(-60)),
        ("z".to_owned(), Tag::Int(POS.z * 16 + local_z)),
    ];
    fields.extend(extra);
    Tag::Compound(fields)
}

/// 复合标签按名字排序，同名键取最后一个；`Nbt` 层写盘时也是一套 `HashMap` 语义。
fn sorted_fields(tag: &Tag) -> Vec<(String, Tag)> {
    let Tag::Compound(entries) = tag else {
        panic!("block entity entry is a compound");
    };
    let mut out: Vec<(String, Tag)> = Vec::new();
    for (key, value) in entries {
        out.retain(|(existing, _)| existing != key);
        out.push((key.clone(), value.clone()));
    }
    out.sort_by(|(a, _), (b, _)| a.cmp(b));
    out
}

/// 23 种方块实体的官方裸条目默认键（按 `javap -c -p` 与官方
/// `loadStatic → saveWithFullMetadata` 逐值核对）。键序按键名排序，和写出顺序无关。
/// Official bare-entry defaults for the 23 block entity blocks, verified value by
/// value against `loadStatic → saveWithFullMetadata`.
fn bare_defaults() -> Vec<(&'static str, Vec<(String, Tag)>)> {
    let short = |value: i16| Tag::Short(value);
    let int = |value: i32| Tag::Int(value);
    let byte = |value: i8| Tag::Byte(value);
    let text = || {
        Tag::Compound(vec![
            (
                "messages".to_owned(),
                Tag::List(vec![Tag::String(String::new()); 4]),
            ),
            ("color".to_owned(), Tag::String("black".to_owned())),
            ("has_glowing_text".to_owned(), byte(0)),
        ])
    };
    let mut out = vec![
        (
            "minecraft:vault",
            vec![
                ("config".to_owned(), default_vault_config()),
                ("shared_data".to_owned(), empty()),
                ("server_data".to_owned(), empty()),
            ],
        ),
        ("minecraft:furnace", furnace_defaults()),
        (
            "minecraft:sculk_sensor",
            vec![
                ("last_vibration_frequency".to_owned(), int(0)),
                ("listener".to_owned(), default_listener()),
            ],
        ),
        (
            "minecraft:calibrated_sculk_sensor",
            vec![
                ("last_vibration_frequency".to_owned(), int(0)),
                ("listener".to_owned(), default_listener()),
            ],
        ),
        (
            "minecraft:sculk_shrieker",
            vec![
                ("warning_level".to_owned(), int(0)),
                ("listener".to_owned(), default_listener()),
            ],
        ),
        (
            "minecraft:mob_spawner",
            vec![
                ("Delay".to_owned(), short(20)),
                ("MinSpawnDelay".to_owned(), short(200)),
                ("MaxSpawnDelay".to_owned(), short(800)),
                ("SpawnCount".to_owned(), short(4)),
                ("MaxNearbyEntities".to_owned(), short(6)),
                ("RequiredPlayerRange".to_owned(), short(16)),
                ("SpawnRange".to_owned(), short(4)),
                (
                    "SpawnPotentials".to_owned(),
                    Tag::List(vec![Tag::Compound(vec![
                        (
                            "data".to_owned(),
                            Tag::Compound(vec![("entity".to_owned(), empty())]),
                        ),
                        ("weight".to_owned(), int(1)),
                    ])]),
                ),
            ],
        ),
        (
            "minecraft:sign",
            vec![
                ("front_text".to_owned(), text()),
                ("back_text".to_owned(), text()),
                ("is_waxed".to_owned(), byte(0)),
            ],
        ),
        (
            "minecraft:hanging_sign",
            vec![
                ("front_text".to_owned(), text()),
                ("back_text".to_owned(), text()),
                ("is_waxed".to_owned(), byte(0)),
            ],
        ),
        (
            "minecraft:beehive",
            vec![("bees".to_owned(), Tag::List(Vec::new()))],
        ),
        (
            "minecraft:jigsaw",
            vec![
                ("name".to_owned(), Tag::String("minecraft:empty".to_owned())),
                (
                    "target".to_owned(),
                    Tag::String("minecraft:empty".to_owned()),
                ),
                ("pool".to_owned(), Tag::String("minecraft:empty".to_owned())),
                (
                    "final_state".to_owned(),
                    Tag::String("minecraft:air".to_owned()),
                ),
                ("joint".to_owned(), Tag::String("aligned".to_owned())),
                ("placement_priority".to_owned(), int(0)),
                ("selection_priority".to_owned(), int(0)),
            ],
        ),
        (
            "minecraft:end_gateway",
            vec![("Age".to_owned(), Tag::Long(0))],
        ),
        (
            "minecraft:sculk_catalyst",
            vec![("cursors".to_owned(), Tag::List(Vec::new()))],
        ),
        ("minecraft:beacon", vec![("Levels".to_owned(), int(0))]),
        (
            "minecraft:potent_sulfur",
            vec![("countdown".to_owned(), int(-1))],
        ),
        (
            "minecraft:structure_block",
            vec![
                ("name".to_owned(), Tag::String(String::new())),
                ("author".to_owned(), Tag::String(String::new())),
                ("metadata".to_owned(), Tag::String(String::new())),
                ("posX".to_owned(), int(0)),
                ("posY".to_owned(), int(1)),
                ("posZ".to_owned(), int(0)),
                ("sizeX".to_owned(), int(0)),
                ("sizeY".to_owned(), int(0)),
                ("sizeZ".to_owned(), int(0)),
                ("rotation".to_owned(), Tag::String("NONE".to_owned())),
                ("mirror".to_owned(), Tag::String("NONE".to_owned())),
                ("mode".to_owned(), Tag::String("DATA".to_owned())),
                ("ignoreEntities".to_owned(), byte(1)),
                ("strict".to_owned(), byte(0)),
                ("powered".to_owned(), byte(0)),
                ("showair".to_owned(), byte(0)),
                ("showboundingbox".to_owned(), byte(1)),
                ("integrity".to_owned(), Tag::Float(1.0)),
                ("seed".to_owned(), Tag::Long(0)),
            ],
        ),
        ("minecraft:ender_chest", Vec::new()),
        ("minecraft:jukebox", Vec::new()),
        ("minecraft:shulker_box", Vec::new()),
        ("minecraft:decorated_pot", Vec::new()),
        (
            "minecraft:shelf",
            vec![
                ("Items".to_owned(), Tag::List(Vec::new())),
                ("align_items_to_bottom".to_owned(), byte(0)),
            ],
        ),
        (
            "minecraft:command_block",
            vec![
                ("Command".to_owned(), Tag::String(String::new())),
                ("SuccessCount".to_owned(), int(0)),
                ("TrackOutput".to_owned(), byte(1)),
                ("UpdateLastExecution".to_owned(), byte(1)),
                ("powered".to_owned(), byte(0)),
                ("conditionMet".to_owned(), byte(0)),
                ("auto".to_owned(), byte(0)),
            ],
        ),
        (
            "minecraft:test_block",
            vec![
                ("mode".to_owned(), Tag::String("fail".to_owned())),
                ("message".to_owned(), Tag::String(String::new())),
                ("powered".to_owned(), byte(0)),
            ],
        ),
        (
            "minecraft:test_instance_block",
            vec![(
                "data".to_owned(),
                Tag::Compound(vec![
                    ("size".to_owned(), Tag::IntArray(vec![0, 0, 0])),
                    ("rotation".to_owned(), Tag::String("none".to_owned())),
                    ("ignore_entities".to_owned(), byte(0)),
                    ("status".to_owned(), Tag::String("cleared".to_owned())),
                ]),
            )],
        ),
    ];
    for (_, fields) in &mut out {
        fields.sort_by(|(a, _), (b, _)| a.cmp(b));
    }
    out
}

/// 方块状态放在两行的 23 格；下标 0 是空气，后面按 [`bare_defaults`] 的顺序。
fn x_of(id: &str) -> i32 {
    bare_defaults()
        .iter()
        .position(|(name, _)| *name == id)
        .unwrap_or_else(|| panic!("{id} is in the test palette")) as i32
}

/// 读盘再存盘，按 `id` 取回每个条目的键和值。
/// Load and save, returning every entry's fields by `id`.
fn resave(entries: &[Tag]) -> Vec<(String, Vec<(String, Tag)>)> {
    let chunk = chunk_with(entries);
    let decoded = mc_bamboo_game::region::chunk_content::decode(POS, &chunk, None)
        .expect("Bamboo decodes the chunk");
    assert!(decoded.problems.is_empty(), "{:?}", decoded.problems);
    let encoded = mc_bamboo_game::region::chunk_content::encode(&decoded.column, POS)
        .expect("Bamboo encodes the chunk");
    let Some(Tag::List(written)) = field(&encoded.chunk, "block_entities") else {
        panic!("written chunk has block_entities");
    };
    let mut out: Vec<(String, Vec<(String, Tag)>)> = written
        .iter()
        .map(|tag| {
            let id = match field(tag, "id") {
                Some(Tag::String(id)) => id.clone(),
                _ => String::new(),
            };
            (id, sorted_fields(tag))
        })
        .collect();
    out.sort_by(|(a, _), (b, _)| a.cmp(b));
    out
}

/// 读盘 → 在运行时表里改内容 → 存盘。运行时改过的值必须压过读盘的旧值。
/// Load a chunk, mutate a runtime table, then save it. Runtime values win.
fn resave_after(
    mut edit: impl FnMut(&mut mc_bamboo_game::region::RegionColumnParts),
) -> Vec<(String, Vec<(String, Tag)>)> {
    let entries: Vec<Tag> = bare_defaults()
        .iter()
        .map(|(id, _)| entry(id, x_of(id), Vec::new()))
        .collect();
    let chunk = chunk_with(&entries);
    let decoded = mc_bamboo_game::region::chunk_content::decode(POS, &chunk, None)
        .expect("Bamboo decodes the chunk");
    assert!(decoded.problems.is_empty(), "{:?}", decoded.problems);
    let lower = mc_bamboo_world::column::ChunkColumnParts::read_from(decoded.column.chunk_column());
    let rebuilt =
        mc_bamboo_world::column::ChunkColumn::from_parts(lower).expect("lower column rebuilds");
    let mut parts = mc_bamboo_game::region::RegionColumnParts {
        column: Box::new(rebuilt),
        ticks: None,
        block_entities: Default::default(),
        redstone: Default::default(),
        containers: Default::default(),
        block_items: Default::default(),
        command_blocks: Default::default(),
        books: Default::default(),
        entities: Default::default(),
    };
    edit(&mut parts);
    let column = mc_bamboo_game::region::RegionColumn::from_parts(parts);
    let encoded = mc_bamboo_game::region::chunk_content::encode(&column, POS)
        .expect("Bamboo encodes the chunk");
    let Some(Tag::List(written)) = field(&encoded.chunk, "block_entities") else {
        panic!("written chunk has block_entities");
    };
    let mut out: Vec<(String, Vec<(String, Tag)>)> = written
        .iter()
        .map(|tag| {
            let id = match field(tag, "id") {
                Some(Tag::String(id)) => id.clone(),
                _ => String::new(),
            };
            (id, sorted_fields(tag))
        })
        .collect();
    out.sort_by(|(a, _), (b, _)| a.cmp(b));
    out
}

fn expected(mut extra: Vec<(String, Tag)>) -> Vec<(String, Tag)> {
    extra.extend([
        ("components".to_owned(), Tag::Compound(Vec::new())),
        ("keepPacked".to_owned(), Tag::Byte(0)),
    ]);
    extra.sort_by(|(a, _), (b, _)| a.cmp(b));
    extra
}

fn empty() -> Tag {
    Tag::Compound(Vec::new())
}

fn default_vault_config() -> Tag {
    Tag::Compound(vec![(
        "key_item".to_owned(),
        Tag::Compound(vec![
            (
                "id".to_owned(),
                Tag::String("minecraft:trial_key".to_owned()),
            ),
            ("count".to_owned(), Tag::Int(1)),
        ]),
    )])
}

fn default_listener() -> Tag {
    Tag::Compound(vec![
        (
            "selector".to_owned(),
            Tag::Compound(vec![("tick".to_owned(), Tag::Long(-1))]),
        ),
        ("event_delay".to_owned(), Tag::Int(0)),
    ])
}

fn furnace_defaults() -> Vec<(String, Tag)> {
    vec![
        ("Items".to_owned(), Tag::List(Vec::new())),
        ("RecipesUsed".to_owned(), empty()),
        ("cooking_time_spent".to_owned(), Tag::Int(0)),
        ("cooking_total_time".to_owned(), Tag::Int(0)),
        ("lit_time_remaining".to_owned(), Tag::Int(0)),
        ("lit_total_time".to_owned(), Tag::Int(0)),
        ("speed_multiplier".to_owned(), Tag::Float(1.0)),
    ]
}

/// 只有 `id` / 坐标的条目（世界生成期的放置、玩家新放的方块都会落到这种形状）：
/// 四种新类型和熔炉都写出官方 `saveAdditional` 的默认值，逐键逐值相等。
///
/// Bare entries (generation-time placement and freshly placed blocks) all write
/// vanilla's constructor defaults, key by key and value by value.
/// 每条只有 `id` / 坐标的条目（世界生成期的放置、玩家新放的方块都会落到这种形状）：
/// 23 种方块实体都写出官方 `saveAdditional` 的默认值，逐键逐值相等。
///
/// Bare entries (generation-time placement and freshly placed blocks) all write
/// vanilla's constructor defaults, key by key and value by value.
#[test]
fn bare_entries_resave_against_vanilla_save_additional() {
    let defaults = bare_defaults();
    let entries: Vec<Tag> = defaults
        .iter()
        .map(|(id, _)| entry(id, x_of(id), Vec::new()))
        .collect();
    let written = resave(&entries);
    assert_eq!(written.len(), 23, "each entry survives the round trip");
    for (id, fields) in &defaults {
        let _ = position_of(id);
        let mut want = expected(fields.clone());
        want.extend([
            ("id".to_owned(), Tag::String((*id).to_owned())),
            ("x".to_owned(), Tag::Int(POS.x * 16 + position_of(id).0)),
            ("y".to_owned(), Tag::Int(-60)),
            ("z".to_owned(), Tag::Int(POS.z * 16 + position_of(id).1)),
        ]);
        want.sort_by(|(a, _), (b, _)| a.cmp(b));
        let got = written
            .iter()
            .find(|(written_id, fields)| {
                written_id == id
                    && fields.iter().any(|(key, value)| {
                        key == "x" && *value == Tag::Int(POS.x * 16 + position_of(id).0)
                    })
            })
            .map(|(_, fields)| fields.clone())
            .unwrap_or_else(|| panic!("{id} was written"));
        assert_eq!(got, want, "{id}");
    }
    // 改坏确认：删掉每种类型的一个默认键，断言立刻失败。
    // Break-it check: dropping one default key makes the comparison fail.
    for (id, fields) in &defaults {
        if fields.is_empty() {
            continue;
        }
        let mut broken = fields.clone();
        let key = broken.remove(0).0;
        let mut want = expected(broken);
        want.extend([
            ("id".to_owned(), Tag::String((*id).to_owned())),
            ("x".to_owned(), Tag::Int(POS.x * 16 + position_of(id).0)),
            ("y".to_owned(), Tag::Int(-60)),
            ("z".to_owned(), Tag::Int(POS.z * 16 + position_of(id).1)),
        ]);
        want.sort_by(|(a, _), (b, _)| a.cmp(b));
        let got = written
            .iter()
            .find(|(written_id, _)| written_id == id)
            .map(|(_, fields)| fields.clone())
            .unwrap();
        assert_ne!(got, want, "{id} must write {key}");
    }
}

#[test]
fn full_entries_resave_with_vanilla_codec_rules() {
    let uuid = Tag::IntArray(vec![1, 2, 3, 4]);
    let written = resave(&[
        entry(
            "minecraft:vault",
            x_of("minecraft:vault"),
            vec![
                (
                    "config".to_owned(),
                    Tag::Compound(vec![
                        (
                            "loot_table".to_owned(),
                            Tag::String("minecraft:chests/trial_chambers/reward".to_owned()),
                        ),
                        ("activation_range".to_owned(), Tag::Double(3.0)),
                        ("deactivation_range".to_owned(), Tag::Double(4.5)),
                    ]),
                ),
                (
                    "shared_data".to_owned(),
                    Tag::Compound(vec![(
                        "connected_players".to_owned(),
                        Tag::List(vec![uuid.clone()]),
                    )]),
                ),
                (
                    "server_data".to_owned(),
                    Tag::Compound(vec![
                        ("items_to_eject".to_owned(), Tag::List(Vec::new())),
                        ("total_ejections_needed".to_owned(), Tag::Int(7)),
                        ("state_updating_resumes_at".to_owned(), Tag::Long(99)),
                    ]),
                ),
            ],
        ),
        entry(
            "minecraft:furnace",
            x_of("minecraft:furnace"),
            vec![("BurnTime".to_owned(), Tag::Short(5))],
        ),
        entry(
            "minecraft:sculk_sensor",
            x_of("minecraft:sculk_sensor"),
            vec![
                ("last_vibration_frequency".to_owned(), Tag::Int(5)),
                (
                    "listener".to_owned(),
                    Tag::Compound(vec![
                        (
                            "selector".to_owned(),
                            Tag::Compound(vec![("tick".to_owned(), Tag::Long(42))]),
                        ),
                        ("event_delay".to_owned(), Tag::Int(3)),
                    ]),
                ),
            ],
        ),
        entry(
            "minecraft:calibrated_sculk_sensor",
            x_of("minecraft:calibrated_sculk_sensor"),
            vec![(
                "listener".to_owned(),
                Tag::Compound(vec![(
                    "selector".to_owned(),
                    Tag::Compound(vec![("tick".to_owned(), Tag::Long(-1))]),
                )]),
            )],
        ),
        entry(
            "minecraft:sculk_shrieker",
            x_of("minecraft:sculk_shrieker"),
            vec![("warning_level".to_owned(), Tag::Int(2))],
        ),
    ]);
    let by_id = |id: &str| {
        written
            .iter()
            .find(|(written_id, _)| written_id == id)
            .map(|(_, fields)| fields.clone())
            .unwrap_or_else(|| panic!("{id} was written"))
    };
    assert_eq!(written.len(), 23, "each entry survives the round trip");

    assert_eq!(
        by_id("minecraft:vault"),
        expected(vec![
            (
                "config".to_owned(),
                Tag::Compound(vec![("activation_range".to_owned(), Tag::Double(3.0))]),
            ),
            (
                "shared_data".to_owned(),
                Tag::Compound(vec![(
                    "connected_players".to_owned(),
                    Tag::List(vec![uuid]),
                )]),
            ),
            (
                "server_data".to_owned(),
                Tag::Compound(vec![(
                    "state_updating_resumes_at".to_owned(),
                    Tag::Long(99)
                )]),
            ),
            ("id".to_owned(), Tag::String("minecraft:vault".to_owned())),
            (
                "x".to_owned(),
                Tag::Int(POS.x * 16 + position_of("minecraft:vault").0)
            ),
            ("y".to_owned(), Tag::Int(-60)),
            (
                "z".to_owned(),
                Tag::Int(POS.z * 16 + position_of("minecraft:vault").1)
            ),
        ])
    );
    let mut furnace = furnace_defaults();
    furnace.extend([
        ("BurnTime".to_owned(), Tag::Short(5)),
        ("id".to_owned(), Tag::String("minecraft:furnace".to_owned())),
        (
            "x".to_owned(),
            Tag::Int(POS.x * 16 + position_of("minecraft:furnace").0),
        ),
        ("y".to_owned(), Tag::Int(-60)),
        (
            "z".to_owned(),
            Tag::Int(POS.z * 16 + position_of("minecraft:furnace").1),
        ),
    ]);
    assert_eq!(by_id("minecraft:furnace"), expected(furnace));
    assert_eq!(
        by_id("minecraft:sculk_sensor"),
        expected(vec![
            ("last_vibration_frequency".to_owned(), Tag::Int(5)),
            (
                "listener".to_owned(),
                Tag::Compound(vec![
                    (
                        "selector".to_owned(),
                        Tag::Compound(vec![("tick".to_owned(), Tag::Long(-1))]),
                    ),
                    ("event_delay".to_owned(), Tag::Int(3)),
                ]),
            ),
            (
                "id".to_owned(),
                Tag::String("minecraft:sculk_sensor".to_owned()),
            ),
            (
                "x".to_owned(),
                Tag::Int(POS.x * 16 + position_of("minecraft:sculk_sensor").0)
            ),
            ("y".to_owned(), Tag::Int(-60)),
            (
                "z".to_owned(),
                Tag::Int(POS.z * 16 + position_of("minecraft:sculk_sensor").1)
            ),
        ])
    );
    // 缺省的 `event_delay` 仍按 `optionalAlwaysPresentFieldOf` 写 0。
    assert_eq!(
        by_id("minecraft:calibrated_sculk_sensor"),
        expected(vec![
            ("last_vibration_frequency".to_owned(), Tag::Int(0)),
            ("listener".to_owned(), default_listener()),
            (
                "id".to_owned(),
                Tag::String("minecraft:calibrated_sculk_sensor".to_owned()),
            ),
            (
                "x".to_owned(),
                Tag::Int(POS.x * 16 + position_of("minecraft:calibrated_sculk_sensor").0)
            ),
            ("y".to_owned(), Tag::Int(-60)),
            (
                "z".to_owned(),
                Tag::Int(POS.z * 16 + position_of("minecraft:calibrated_sculk_sensor").1)
            ),
        ])
    );
    assert_eq!(
        by_id("minecraft:sculk_shrieker"),
        expected(vec![
            ("warning_level".to_owned(), Tag::Int(2)),
            ("listener".to_owned(), default_listener()),
            (
                "id".to_owned(),
                Tag::String("minecraft:sculk_shrieker".to_owned()),
            ),
            (
                "x".to_owned(),
                Tag::Int(POS.x * 16 + position_of("minecraft:sculk_shrieker").0)
            ),
            ("y".to_owned(), Tag::Int(-60)),
            (
                "z".to_owned(),
                Tag::Int(POS.z * 16 + position_of("minecraft:sculk_shrieker").1)
            ),
        ])
    );
}

/// 世界生成只放下方块、没有任何 `block_entities` 条目时，存盘也要补出
/// 官方构造器写出的默认键。去掉这条补键路径，这个断言会立刻失败（改坏确认）。
///
/// When worldgen places a block without any `block_entities` entry, the save
/// must still write vanilla's constructor defaults. A break-self-check removes
/// that path and this assertion fails.
#[test]
fn generated_block_entities_write_defaults_without_a_disk_entry() {
    let written = resave(&[]);
    let defaults = bare_defaults();
    assert_eq!(written.len(), 23, "every block entity state is written");
    for (id, fields) in &defaults {
        let _ = position_of(id);
        let mut want = expected(fields.clone());
        want.extend([
            ("id".to_owned(), Tag::String((*id).to_owned())),
            ("x".to_owned(), Tag::Int(POS.x * 16 + position_of(id).0)),
            ("y".to_owned(), Tag::Int(-60)),
            ("z".to_owned(), Tag::Int(POS.z * 16 + position_of(id).1)),
        ]);
        want.sort_by(|(a, _), (b, _)| a.cmp(b));
        let got = written
            .iter()
            .find(|(written_id, fields)| {
                written_id == id
                    && fields.iter().any(|(key, value)| {
                        key == "x" && *value == Tag::Int(POS.x * 16 + position_of(id).0)
                    })
            })
            .map(|(_, fields)| fields.clone())
            .unwrap_or_else(|| panic!("{id} was written"));
        assert_eq!(got, want, "{id} with no disk entry");
    }
}

/// 读盘 → 游戏里改内容 → 存盘：运行时表的值优先，读盘旧值不能盖回去。
/// Load, change a runtime table, then save: runtime values win over disk values.
#[test]
fn runtime_edits_win_over_the_disk_entry() {
    use mc_bamboo_core::sign::{DyeColor, SignContent};
    use mc_bamboo_data::items::STONE;
    use mc_bamboo_protocol::item::ItemStack;

    let written = resave_after(|parts| {
        // 告示牌：游戏里写了字、换颜色、上蜡，存盘写运行时内容而不是读盘的空告示牌。
        let mut content = SignContent::BLANK;
        content.front.messages[0] = "hello".to_owned();
        content.front.color = DyeColor::Red;
        content.waxed = true;
        parts.column.chunk_mut().set_sign(
            position_of("minecraft:sign").0 as usize,
            -60,
            position_of("minecraft:sign").1 as usize,
            content,
        );
        // 架子：运行时往第一格放一块石头，存盘写 `[{Slot:0,...}]`。
        let shelf = mc_bamboo_core::BlockPos::new(
            POS.x * 16 + position_of("minecraft:shelf").0,
            -60,
            POS.z * 16 + position_of("minecraft:shelf").1,
        );
        let filled = mc_bamboo_protocol::components::archive::slots::encode_list(&[
            mc_bamboo_protocol::components::archive::slots::SlottedItem {
                slot: 0,
                stack: ItemStack::new(STONE, 3),
            },
        ])
        .expect("a stone stack encodes");
        let entry = mc_bamboo_game::region::ContainerEntry::with_items(
            mc_bamboo_game::region::ContainerKind::Shelf,
            &filled,
            3,
        );
        parts
            .containers
            .insert(shelf, mc_bamboo_game::region::PackedContainer::new(entry));
        // 命令方块：运行时改了命令，存盘写新命令。
        let command = mc_bamboo_core::BlockPos::new(
            POS.x * 16 + position_of("minecraft:command_block").0,
            -60,
            POS.z * 16 + position_of("minecraft:command_block").1,
        );
        let data = mc_bamboo_game::region::ArchiveCommandBlock {
            command: "say hi".to_owned(),
            track_output: false,
            automatic: true,
            success_count: 7,
            powered: true,
            condition_met: true,
            update_last_execution: false,
        };
        parts.command_blocks.insert(
            command,
            mc_bamboo_game::region::CommandBlockData::from_archive(data),
        );
    });
    let by_id = |id: &str| {
        written
            .iter()
            .find(|(written_id, _)| written_id == id)
            .map(|(_, fields)| fields.clone())
            .unwrap_or_else(|| panic!("{id} was written"))
    };
    let sign = by_id("minecraft:sign");
    let front = sign.iter().find(|(key, _)| key == "front_text").unwrap();
    assert_eq!(
        mc_bamboo_storage::chunk_nbt::field(&front.1, "messages"),
        Some(&Tag::List(vec![
            Tag::String("hello".to_owned()),
            Tag::String(String::new()),
            Tag::String(String::new()),
            Tag::String(String::new()),
        ]))
    );
    assert_eq!(
        mc_bamboo_storage::chunk_nbt::field(&front.1, "color"),
        Some(&Tag::String("red".to_owned()))
    );
    assert_eq!(
        sign.iter().find(|(key, _)| key == "is_waxed").unwrap().1,
        Tag::Byte(1)
    );
    let shelf = by_id("minecraft:shelf");
    let items = shelf.iter().find(|(key, _)| key == "Items").unwrap();
    assert!(matches!(items.1, Tag::List(ref list) if list.len() == 1));
    assert_eq!(
        shelf
            .iter()
            .find(|(key, _)| key == "align_items_to_bottom")
            .unwrap()
            .1,
        Tag::Byte(0)
    );
    let command = by_id("minecraft:command_block");
    assert_eq!(
        command.iter().find(|(key, _)| key == "Command").unwrap().1,
        Tag::String("say hi".to_owned())
    );
    assert_eq!(
        command
            .iter()
            .find(|(key, _)| key == "SuccessCount")
            .unwrap()
            .1,
        Tag::Int(7)
    );
    assert_eq!(
        command
            .iter()
            .find(|(key, _)| key == "TrackOutput")
            .unwrap()
            .1,
        Tag::Byte(0)
    );
}

/// D9：未知键原样保留（当前数据版本下官方自己写出的存档不会带这种键，
/// 这是和官方逐键比较时唯一有意的差别）。
#[test]
fn unknown_legacy_keys_stay_preserved() {
    let written = resave(&[entry(
        "minecraft:furnace",
        x_of("minecraft:furnace"),
        vec![("BurnTime".to_owned(), Tag::Short(5))],
    )]);
    let furnace = written
        .iter()
        .find(|(id, _)| id == "minecraft:furnace")
        .map(|(_, fields)| fields.clone())
        .unwrap();
    assert_eq!(
        furnace.iter().find(|(key, _)| key == "BurnTime"),
        Some(&("BurnTime".to_owned(), Tag::Short(5)))
    );
}
