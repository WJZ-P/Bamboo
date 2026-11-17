//! 宝库 / 熔炉 / 幽匿传感器的方块实体经 Bamboo 运行时「读盘 → 存盘」后写出哪些键，
//! 和官方 `saveAdditional` 必写的键对照（官方键名与默认值按 `javap -c -p` 核对）：
//!
//! * `AbstractFurnaceBlockEntity.saveAdditional`：`cooking_time_spent` / `cooking_total_time` /
//!   `lit_time_remaining` / `lit_total_time`（`putInt`）、`speed_multiplier`（`putFloat`）、
//!   `Items`、`RecipesUsed`；
//! * `VaultBlockEntity.saveAdditional`：`config` / `shared_data` / `server_data` 三个 codec；
//! * `SculkSensorBlockEntity.saveAdditional`：`last_vibration_frequency`（`putInt`）与
//!   `listener`（`VibrationSystem.Data.CODEC`）；校准幽匿传感器沿用父类。
//!
//! 这里只记录现状，不修：熔炉有实例，七个键全写；宝库和幽匿传感器在 Bamboo 里没有
//! 实例，读盘条目原样保留，所以条目里缺什么、存盘就缺什么（例如只有 `id` / 坐标的
//! 条目，官方会补出上面那些键，Bamboo 不补）。
//!
//! What Bamboo's runtime load -> save writes for vault / furnace / sculk sensor block
//! entities, against the keys vanilla `saveAdditional` always writes (checked with
//! `javap -c -p`). This records the current behaviour only: furnaces are live entities
//! and write all seven keys; vaults and sculk sensors have no live entity, so the stored
//! entry is kept verbatim and whatever it lacks stays missing.

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

/// 官方样例区块，把 `Y = -4` 那一节换成「空气 + 四种方块实体方块」，并只留给定的
/// 方块实体条目。四个方块放在本区块 `(0..4, -60, 0)`。
///
/// The official sample chunk with section `Y = -4` replaced by air plus the four
/// block-entity blocks at `(0..4, -60, 0)`, keeping only the given entries.
fn chunk_with(entries: Vec<Tag>) -> Tag {
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
    ];
    let palette = names
        .iter()
        .map(|name| Tag::Compound(vec![("Name".to_owned(), Tag::String((*name).to_owned()))]))
        .collect();
    // 5 个调色板项 → 每格 4 位，一个 long 装 16 格；第 4 层（y = -60）第 0 行前四格。
    // Five palette entries -> 4 bits per cell, 16 cells per long; layer 4 (y = -60), row 0.
    let mut data = vec![0_i64; 256];
    let base = 4 * 256;
    for (x, palette_index) in [1_i64, 2, 3, 4].into_iter().enumerate() {
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
    root.push(("block_entities".to_owned(), Tag::List(entries)));
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

fn keys_of(tag: &Tag) -> Vec<String> {
    match tag {
        Tag::Compound(entries) => {
            let mut keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();
            keys.sort();
            keys
        }
        _ => Vec::new(),
    }
}

/// 读盘再存盘，按 `id` 取回每个条目的键。
/// Load and save, returning each entry's keys by `id`.
fn resave(entries: Vec<Tag>) -> Vec<(String, Vec<String>)> {
    let chunk = chunk_with(entries);
    let decoded = mc_bamboo_game::region::chunk_content::decode(POS, &chunk, None)
        .expect("Bamboo decodes the chunk");
    assert!(decoded.problems.is_empty(), "{:?}", decoded.problems);
    let encoded = mc_bamboo_game::region::chunk_content::encode(&decoded.column, POS)
        .expect("Bamboo encodes the chunk");
    let Some(Tag::List(written)) = field(&encoded.chunk, "block_entities") else {
        panic!("written chunk has block_entities");
    };
    let mut out: Vec<(String, Vec<String>)> = written
        .iter()
        .map(|tag| {
            let id = match field(tag, "id") {
                Some(Tag::String(id)) => id.clone(),
                _ => String::new(),
            };
            (id, keys_of(tag))
        })
        .collect();
    out.sort();
    out
}

fn keys(list: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = list.iter().map(|key| (*key).to_owned()).collect();
    out.sort();
    out
}

const METADATA: [&str; 6] = ["components", "id", "keepPacked", "x", "y", "z"];

fn with_metadata(extra: &[&str]) -> Vec<String> {
    let mut all: Vec<&str> = METADATA.to_vec();
    all.extend_from_slice(extra);
    keys(&all)
}

/// 只有 `id` / 坐标的条目（世界生成期的放置、玩家新放的方块都会落到这种形状）。
/// Entries with only `id` and coordinates (what generation-time placement and freshly
/// placed blocks reduce to).
#[test]
fn bare_entries_resave_against_vanilla_save_additional() {
    let written = resave(vec![
        entry("minecraft:vault", 0, Vec::new()),
        entry("minecraft:furnace", 1, Vec::new()),
        entry("minecraft:sculk_sensor", 2, Vec::new()),
        entry("minecraft:calibrated_sculk_sensor", 3, Vec::new()),
    ]);
    let by_id = |id: &str| {
        written
            .iter()
            .find(|(written_id, _)| written_id == id)
            .map(|(_, keys)| keys.clone())
            .unwrap_or_else(|| panic!("{id} was written"))
    };
    // 熔炉：七个 `saveAdditional` 键都在，和官方一致。
    // Furnace: all seven `saveAdditional` keys, matching vanilla.
    assert_eq!(
        by_id("minecraft:furnace"),
        with_metadata(&[
            "Items",
            "RecipesUsed",
            "cooking_time_spent",
            "cooking_total_time",
            "lit_time_remaining",
            "lit_total_time",
            "speed_multiplier",
        ])
    );
    // 已知差异：官方会写 `config` / `shared_data` / `server_data`，Bamboo 只有元数据。
    // Known gap: vanilla writes `config` / `shared_data` / `server_data`; Bamboo only
    // the metadata.
    assert_eq!(by_id("minecraft:vault"), with_metadata(&[]));
    // 已知差异：官方会写 `last_vibration_frequency` / `listener`，Bamboo 只有元数据。
    // Known gap: vanilla writes `last_vibration_frequency` / `listener`; Bamboo only
    // the metadata.
    assert_eq!(by_id("minecraft:sculk_sensor"), with_metadata(&[]));
    assert_eq!(
        by_id("minecraft:calibrated_sculk_sensor"),
        with_metadata(&[])
    );
}

/// 官方存过的完整条目：宝库 / 幽匿传感器原样带回，熔炉的未知旧键也原样带回
/// （官方 `loadAdditional` 不读的键，`saveAdditional` 不会再写）。
///
/// Full entries as vanilla saves them: vaults and sculk sensors come back verbatim, and
/// so does an unknown legacy key on the furnace (which vanilla would drop because
/// `loadAdditional` never reads it).
#[test]
fn full_entries_keep_their_keys() {
    let empty = || Tag::Compound(Vec::new());
    let written = resave(vec![
        entry(
            "minecraft:vault",
            0,
            vec![
                ("config".to_owned(), empty()),
                ("shared_data".to_owned(), empty()),
                ("server_data".to_owned(), empty()),
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
                ("last_vibration_frequency".to_owned(), Tag::Int(0)),
                (
                    "listener".to_owned(),
                    Tag::Compound(vec![
                        ("event_delay".to_owned(), Tag::Int(0)),
                        (
                            "selector".to_owned(),
                            Tag::Compound(vec![("tick".to_owned(), Tag::Long(-1))]),
                        ),
                    ]),
                ),
            ],
        ),
    ]);
    let by_id = |id: &str| {
        written
            .iter()
            .find(|(written_id, _)| written_id == id)
            .map(|(_, keys)| keys.clone())
            .unwrap_or_else(|| panic!("{id} was written"))
    };
    assert_eq!(
        by_id("minecraft:vault"),
        with_metadata(&["config", "server_data", "shared_data"])
    );
    assert_eq!(
        by_id("minecraft:sculk_sensor"),
        with_metadata(&["last_vibration_frequency", "listener"])
    );
    // 已知差异：`BurnTime` 是官方不认识的旧键，Bamboo 合并时保留了它。
    // Known gap: `BurnTime` is a legacy key vanilla ignores; Bamboo's merge keeps it.
    assert_eq!(
        by_id("minecraft:furnace"),
        with_metadata(&[
            "BurnTime",
            "Items",
            "RecipesUsed",
            "cooking_time_spent",
            "cooking_total_time",
            "lit_time_remaining",
            "lit_total_time",
            "speed_multiplier",
        ])
    );
}
