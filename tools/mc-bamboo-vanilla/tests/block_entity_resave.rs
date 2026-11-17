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
    ];
    let palette = names
        .iter()
        .map(|name| Tag::Compound(vec![("Name".to_owned(), Tag::String((*name).to_owned()))]))
        .collect();
    // 6 个调色板项仍用官方 `FOUR_BITS_LINEAR`：每格 4 位，一个 long 装 16 格，
    // 整段 256 个 long；第 4 层（y = -60）第 0 行前五格。
    // Six palette entries still use vanilla's `FOUR_BITS_LINEAR`: 4 bits per cell,
    // 16 cells per long, 256 longs per section; layer 4 (y = -60), row 0.
    let mut data = vec![0_i64; 256];
    let base = 4 * 256;
    for (x, palette_index) in (1_i64..=5).enumerate() {
        let index = base + x;
        data[index / 16] |= palette_index << ((index % 16) * 4);
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

fn entry(id: &str, x: i32, extra: Vec<(String, Tag)>) -> Tag {
    let mut fields = vec![
        ("id".to_owned(), Tag::String(id.to_owned())),
        ("x".to_owned(), Tag::Int(POS.x * 16 + x)),
        ("y".to_owned(), Tag::Int(-60)),
        ("z".to_owned(), Tag::Int(POS.z * 16)),
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
#[test]
fn bare_entries_resave_against_vanilla_save_additional() {
    let written = resave(&[
        entry("minecraft:vault", 0, Vec::new()),
        entry("minecraft:furnace", 1, Vec::new()),
        entry("minecraft:sculk_sensor", 2, Vec::new()),
        entry("minecraft:calibrated_sculk_sensor", 3, Vec::new()),
        entry("minecraft:sculk_shrieker", 4, Vec::new()),
    ]);
    let by_id = |id: &str| {
        written
            .iter()
            .find(|(written_id, _)| written_id == id)
            .map(|(_, fields)| fields.clone())
            .unwrap_or_else(|| panic!("{id} was written"))
    };
    assert_eq!(written.len(), 5, "each entry survives the round trip");

    let mut furnace = furnace_defaults();
    furnace.extend([
        ("id".to_owned(), Tag::String("minecraft:furnace".to_owned())),
        ("x".to_owned(), Tag::Int(POS.x * 16 + 1)),
        ("y".to_owned(), Tag::Int(-60)),
        ("z".to_owned(), Tag::Int(POS.z * 16)),
    ]);
    assert_eq!(by_id("minecraft:furnace"), expected(furnace));

    let mut vault = vec![
        ("config".to_owned(), default_vault_config()),
        ("shared_data".to_owned(), empty()),
        ("server_data".to_owned(), empty()),
        ("id".to_owned(), Tag::String("minecraft:vault".to_owned())),
        ("x".to_owned(), Tag::Int(POS.x * 16)),
        ("y".to_owned(), Tag::Int(-60)),
        ("z".to_owned(), Tag::Int(POS.z * 16)),
    ];
    assert_eq!(by_id("minecraft:vault"), expected(vault.clone()));

    for (x, id) in [
        (2, "minecraft:sculk_sensor"),
        (3, "minecraft:calibrated_sculk_sensor"),
    ] {
        let mut sensor = vec![
            ("last_vibration_frequency".to_owned(), Tag::Int(0)),
            ("listener".to_owned(), default_listener()),
            ("id".to_owned(), Tag::String(id.to_owned())),
            ("x".to_owned(), Tag::Int(POS.x * 16 + x)),
            ("y".to_owned(), Tag::Int(-60)),
            ("z".to_owned(), Tag::Int(POS.z * 16)),
        ];
        assert_eq!(by_id(id), expected(sensor.clone()), "{id}");
        // 改坏确认：删掉默认键或换掉默认值，断言立刻失败。
        // Break-it check: dropping a default key or changing its value fails.
        sensor.retain(|(key, _)| key != "listener");
        assert_ne!(by_id(id), expected(sensor), "{id} listener is required");
    }
    vault[0].1 = empty();
    assert_ne!(
        by_id("minecraft:vault"),
        expected(vault),
        "the default config must carry the trial key"
    );

    let mut shrieker = vec![
        ("warning_level".to_owned(), Tag::Int(0)),
        ("listener".to_owned(), default_listener()),
        (
            "id".to_owned(),
            Tag::String("minecraft:sculk_shrieker".to_owned()),
        ),
        ("x".to_owned(), Tag::Int(POS.x * 16 + 4)),
        ("y".to_owned(), Tag::Int(-60)),
        ("z".to_owned(), Tag::Int(POS.z * 16)),
    ];
    assert_eq!(
        by_id("minecraft:sculk_shrieker"),
        expected(shrieker.clone())
    );
    shrieker.retain(|(key, _)| key != "warning_level");
    assert_ne!(
        by_id("minecraft:sculk_shrieker"),
        expected(shrieker),
        "warning_level is required"
    );
}

/// 官方存过的完整条目：codec 会把等于默认值的字段省略、把默认值写成正确类型；
/// 空列表不写，非空列表和 UUID 集合原样带回；熔炉的未知旧键按 Bamboo 的合并规则保留。
///
/// Full entries as vanilla saves them: the codecs omit default-valued fields,
/// keep non-default lists and UUID sets, and preserve an unknown legacy furnace key.
#[test]
fn full_entries_resave_with_vanilla_codec_rules() {
    let uuid = Tag::IntArray(vec![1, 2, 3, 4]);
    let written = resave(&[
        entry(
            "minecraft:vault",
            0,
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
            1,
            vec![("BurnTime".to_owned(), Tag::Short(5))],
        ),
        entry(
            "minecraft:sculk_sensor",
            2,
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
            3,
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
            4,
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
    assert_eq!(written.len(), 5, "each entry survives the round trip");

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
            ("x".to_owned(), Tag::Int(POS.x * 16)),
            ("y".to_owned(), Tag::Int(-60)),
            ("z".to_owned(), Tag::Int(POS.z * 16)),
        ])
    );
    let mut furnace = furnace_defaults();
    furnace.extend([
        ("BurnTime".to_owned(), Tag::Short(5)),
        ("id".to_owned(), Tag::String("minecraft:furnace".to_owned())),
        ("x".to_owned(), Tag::Int(POS.x * 16 + 1)),
        ("y".to_owned(), Tag::Int(-60)),
        ("z".to_owned(), Tag::Int(POS.z * 16)),
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
            ("x".to_owned(), Tag::Int(POS.x * 16 + 2)),
            ("y".to_owned(), Tag::Int(-60)),
            ("z".to_owned(), Tag::Int(POS.z * 16)),
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
            ("x".to_owned(), Tag::Int(POS.x * 16 + 3)),
            ("y".to_owned(), Tag::Int(-60)),
            ("z".to_owned(), Tag::Int(POS.z * 16)),
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
            ("x".to_owned(), Tag::Int(POS.x * 16 + 4)),
            ("y".to_owned(), Tag::Int(-60)),
            ("z".to_owned(), Tag::Int(POS.z * 16)),
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
    let by_id = |id: &str| {
        written
            .iter()
            .find(|(written_id, _)| written_id == id)
            .map(|(_, fields)| fields.clone())
            .unwrap_or_else(|| panic!("{id} was written"))
    };
    assert_eq!(written.len(), 5, "every block entity state is written");
    assert_eq!(
        by_id("minecraft:vault"),
        expected(vec![
            ("config".to_owned(), default_vault_config()),
            ("shared_data".to_owned(), empty()),
            ("server_data".to_owned(), empty()),
            ("id".to_owned(), Tag::String("minecraft:vault".to_owned())),
            ("x".to_owned(), Tag::Int(POS.x * 16)),
            ("y".to_owned(), Tag::Int(-60)),
            ("z".to_owned(), Tag::Int(POS.z * 16)),
        ])
    );
    for (x, id, key) in [
        (2, "minecraft:sculk_sensor", "last_vibration_frequency"),
        (
            3,
            "minecraft:calibrated_sculk_sensor",
            "last_vibration_frequency",
        ),
        (4, "minecraft:sculk_shrieker", "warning_level"),
    ] {
        assert_eq!(
            by_id(id),
            expected(vec![
                (key.to_owned(), Tag::Int(0)),
                ("listener".to_owned(), default_listener()),
                ("id".to_owned(), Tag::String(id.to_owned())),
                ("x".to_owned(), Tag::Int(POS.x * 16 + x)),
                ("y".to_owned(), Tag::Int(-60)),
                ("z".to_owned(), Tag::Int(POS.z * 16)),
            ]),
            "{id}"
        );
    }
    // 熔炉没有读盘条目时，旧路径仍然补出七个默认键。
    let mut furnace = furnace_defaults();
    furnace.extend([
        ("id".to_owned(), Tag::String("minecraft:furnace".to_owned())),
        ("x".to_owned(), Tag::Int(POS.x * 16 + 1)),
        ("y".to_owned(), Tag::Int(-60)),
        ("z".to_owned(), Tag::Int(POS.z * 16)),
    ]);
    assert_eq!(by_id("minecraft:furnace"), expected(furnace));
}
