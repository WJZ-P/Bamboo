//! 区块 NBT `structures` 键的存档对拍：官方 `StructureSaveOracle` 对
//! `mc-bamboo-worldgen::structure::save` 的纯编码。
//!
//! Rust 造一批 `{chunk, starts, refs}` 写进 cases.json，Java 用官方
//! `CompoundTag` 原语和 `piece.createTag` 组装出 `structures` 并按键排序 dump；
//! Rust 用生产代码复算，逐 case 比规范 NBT。Java 还把 `Children` 交给官方
//! `PiecesContainer.load` 再 `save` 往返，证明官方能读。最后把带 `structures`
//! 的区块写进临时世界目录，用官方 `world-dir`（`SerializableChunkData.parse`）
//! 确认整块可读。
//!
//! Oracle for the chunk NBT `structures` key: vanilla `StructureSaveOracle` vs
//! `mc-bamboo-worldgen::structure::save`. Rust writes cases.json; Java assembles
//! `structures` with official primitives + `piece.createTag` and dumps key-sorted
//! canonical NBT; Rust recomputes with production code and compares. Java also
//! proves readability with an official `PiecesContainer.load`/`save` round-trip;
//! finally `world-dir` (`SerializableChunkData.parse`) reads chunks carrying the tag.

use std::{collections::BTreeMap, fs, path::Path, process::Command};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_nbt::Tag;
use mc_bamboo_storage::{
    anvil::{RegionCompression, RegionFile, RegionPos},
    chunk_nbt::{ChunkEncodeExtra, encode_bytes},
    world_dir::WorldDir,
};
use mc_bamboo_world::column::ChunkColumn;
use mc_bamboo_worldgen::structure::{
    BoundingBox, References, SavePiece, SaveStart, chunk_pos_pack, pieces::Piece, single,
    structures_tag,
};
use serde::Serialize;
use serde_json::{Value, json};

const CASES_FILE: &str = "structure_save_cases.json";
const OUT_FILE: &str = "structure_save_out.json";

#[derive(Clone, Serialize)]
struct PieceCase {
    kind: &'static str,
    seed: i64,
    px: i32,
    pz: i32,
}

#[derive(Serialize)]
struct StartCase {
    id: String,
    cx: i32,
    cz: i32,
    refs: i32,
    pieces: Vec<PieceCase>,
}

#[derive(Serialize)]
struct Case {
    chunk: [i32; 2],
    starts: Vec<StartCase>,
    refs: BTreeMap<String, Vec<String>>,
}

/// 一组种子 / 区块坐标，覆盖正负、远处与原点。
const COMBOS: [(i64, (i32, i32)); 8] = [
    (0, (0, 0)),
    (42, (3, -9)),
    (-7, (1, 1)),
    (12_345_678_901, (34, 50)),
    (-98_765_432_123, (-34, 50)),
    (2_147_483_647, (100, -100)),
    (-2_147_483_648, (1000, 1000)),
    (7, (12, -7)),
];

pub fn run(mut command: Command, java: &str, _options: &[&str]) -> Result<(), String> {
    let root = crate::workspace_root();
    let work_dir = root.join("target/vanilla");
    let cases_path = work_dir.join(CASES_FILE);
    let out_path = work_dir.join(OUT_FILE);

    let cases = build_cases();
    fs::write(
        &cases_path,
        serde_json::to_string(&json!({ "cases": cases })).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("cannot write cases file: {e}"))?;

    let status = command
        .arg(&cases_path)
        .arg(&out_path)
        .status()
        .map_err(|e| format!("cannot run StructureSaveOracle: {e}"))?;
    if !status.success() {
        return Err(format!("StructureSaveOracle failed: {status}"));
    }
    let out =
        fs::read_to_string(&out_path).map_err(|e| format!("cannot read oracle output: {e}"))?;
    let results: Vec<Value> =
        serde_json::from_str(&out).map_err(|e| format!("bad oracle JSON: {e}"))?;

    let report = compare(&cases, &results)?;
    verify_world_dir(&work_dir, java, &cases)?;
    report.finish()
}

fn build_cases() -> Vec<Case> {
    COMBOS
        .iter()
        .map(|&(seed, (px, pz))| {
            let one = |kind| PieceCase { kind, seed, px, pz };
            let mut refs = BTreeMap::new();
            refs.insert(
                "minecraft:desert_pyramid".to_owned(),
                vec![
                    chunk_pos_pack(px, pz).to_string(),
                    chunk_pos_pack(px + 1, pz).to_string(),
                ],
            );
            refs.insert(
                "minecraft:village_plains".to_owned(),
                vec![
                    chunk_pos_pack(px - 8, pz + 8).to_string(),
                    chunk_pos_pack(px + 20, pz - 20).to_string(),
                ],
            );
            // 空列表按官方 packStructureData 跳过。
            // Empty list is skipped by vanilla packStructureData.
            refs.insert("minecraft:stronghold".to_owned(), Vec::new());
            Case {
                chunk: [px, pz],
                starts: vec![
                    StartCase {
                        id: "minecraft:desert_pyramid".to_owned(),
                        cx: px,
                        cz: pz,
                        refs: 1,
                        pieces: vec![one("desert_pyramid")],
                    },
                    StartCase {
                        id: "minecraft:jungle_temple".to_owned(),
                        cx: px,
                        cz: pz,
                        refs: 0,
                        pieces: vec![one("jungle_temple")],
                    },
                    StartCase {
                        id: "minecraft:swamp_hut".to_owned(),
                        cx: px,
                        cz: pz,
                        refs: 2,
                        pieces: vec![one("swamp_hut")],
                    },
                    StartCase {
                        id: "minecraft:buried_treasure".to_owned(),
                        cx: px,
                        cz: pz,
                        refs: 0,
                        pieces: vec![one("buried_treasure")],
                    },
                    // 多件 Children：pieces 列表里放三种不同结构件。
                    // Multi-piece Children: three different piece kinds.
                    StartCase {
                        id: "minecraft:village_plains".to_owned(),
                        cx: px,
                        cz: pz,
                        refs: 1,
                        pieces: vec![
                            one("desert_pyramid"),
                            PieceCase {
                                kind: "swamp_hut",
                                seed: seed ^ 0x55,
                                px,
                                pz,
                            },
                            one("buried_treasure"),
                        ],
                    },
                    // INVALID 起点：无结构件。
                    // INVALID start: no pieces.
                    StartCase {
                        id: "minecraft:stronghold".to_owned(),
                        cx: px,
                        cz: pz,
                        refs: 0,
                        pieces: Vec::new(),
                    },
                ],
                refs,
            }
        })
        .collect()
}

/// 官方 `addAdditionalSaveData` 的字段类型：四个代码生成单件结构。
/// Int 为 Width/Height/Depth/HPos，布尔标记为 Byte(0)。
///
/// Official `addAdditionalSaveData` field types for the four code-built single
/// pieces: Int for Width/Height/Depth/HPos, Byte(0) for boolean flags.
fn official_extras(kind: &str) -> Vec<(String, Tag)> {
    let ints: &[(&str, i32)] = match kind {
        "desert_pyramid" => &[("Width", 21), ("Height", 15), ("Depth", 21), ("HPos", -1)],
        "jungle_temple" => &[("Width", 12), ("Height", 10), ("Depth", 15), ("HPos", -1)],
        "swamp_hut" => &[("Width", 7), ("Height", 7), ("Depth", 9), ("HPos", -1)],
        "buried_treasure" => &[],
        other => panic!("no extras for {other}"),
    };
    let bools: &[&str] = match kind {
        "desert_pyramid" => &[
            "hasPlacedChest0",
            "hasPlacedChest1",
            "hasPlacedChest2",
            "hasPlacedChest3",
        ],
        "jungle_temple" => &[
            "placedMainChest",
            "placedHiddenChest",
            "placedTrap1",
            "placedTrap2",
        ],
        "swamp_hut" => &["Witch", "Cat"],
        _ => &[],
    };
    let mut extras: Vec<(String, Tag)> = ints
        .iter()
        .map(|&(k, v)| (k.to_owned(), Tag::Int(v)))
        .collect();
    extras.extend(bools.iter().map(|&k| (k.to_owned(), Tag::Byte(0))));
    extras
}

fn to_bb(b: [i32; 6]) -> BoundingBox {
    BoundingBox {
        min_x: b[0],
        min_y: b[1],
        min_z: b[2],
        max_x: b[3],
        max_y: b[4],
        max_z: b[5],
    }
}

/// 用生产代码从布局 `Piece` 造一条存档结构件。
fn piece_save(p: &PieceCase) -> SavePiece {
    let layout: Piece = single::generate(p.kind, p.seed, p.px, p.pz)
        .unwrap()
        .pop()
        .unwrap();
    SavePiece {
        id: layout.id,
        bb: to_bb(layout.bb),
        orientation: layout.o,
        gen_depth: layout.gd,
        extras: official_extras(p.kind),
    }
}

/// 用生产 `structures_tag` 编出该 case 的 `structures` 键。
fn rust_tag(c: &Case) -> Tag {
    let mut saves = BTreeMap::new();
    for s in &c.starts {
        let children: Vec<SavePiece> = s.pieces.iter().map(piece_save).collect();
        saves.insert(
            s.id.clone(),
            SaveStart {
                structure: s.id.clone(),
                chunk_x: s.cx,
                chunk_z: s.cz,
                references: s.refs,
                children,
            },
        );
    }
    let references: References = c
        .refs
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                v.iter().map(|s| s.parse::<i64>().unwrap()).collect(),
            )
        })
        .collect();
    structures_tag(&saves, &references)
}

struct Report {
    cases: usize,
    starts: usize,
    pieces: usize,
    canonical_comparisons: usize,
    failures: Vec<String>,
}

impl Report {
    fn check(&mut self, ok: bool, what: impl Into<String>) {
        if !ok {
            self.failures.push(what.into());
        }
    }

    fn finish(&self) -> Result<(), String> {
        for f in self.failures.iter().take(24) {
            println!("STRUCTURE-SAVE FAIL: {f}");
        }
        println!(
            "structure-save: {} cases, {} starts, {} piece tags, {} canonical comparisons",
            self.cases, self.starts, self.pieces, self.canonical_comparisons
        );
        if self.failures.is_empty() {
            println!("STRUCTURE-SAVE RESULT: 0 mismatches");
            Ok(())
        } else {
            Err(format!(
                "STRUCTURE-SAVE RESULT: {} mismatches",
                self.failures.len()
            ))
        }
    }
}

fn compare(cases: &[Case], results: &[Value]) -> Result<Report, String> {
    if results.len() != cases.len() {
        return Err(format!(
            "oracle emitted {} results, expected {}",
            results.len(),
            cases.len()
        ));
    }
    let mut report = Report {
        cases: cases.len(),
        starts: 0,
        pieces: 0,
        canonical_comparisons: 0,
        failures: Vec::new(),
    };
    for (i, (c, r)) in cases.iter().zip(results.iter()).enumerate() {
        let tag = rust_tag(c);
        report.canonical_comparisons += 1;
        report.starts += c.starts.len();
        let expected_pieces: usize = c
            .starts
            .iter()
            .filter(|s| !s.pieces.is_empty())
            .map(|s| s.pieces.len())
            .sum();
        report.pieces += expected_pieces;

        let ours = canonical_tag(&tag);
        let theirs = r.get("canonical").unwrap_or(&Value::Null);
        report.check(
            &ours == theirs,
            format!("case {i}: canonical NBT differs\n  java: {theirs}\n  rust: {ours}"),
        );
        report.check(
            r.get("reloadEqual") == Some(&Value::Bool(true)),
            format!("case {i}: official PiecesContainer load/save round-trip failed"),
        );
        report.check(
            r.get("reloadedPieces").and_then(Value::as_u64) == Some(expected_pieces as u64),
            format!("case {i}: reloadedPieces != {expected_pieces}"),
        );
    }
    Ok(report)
}

/// 把带 `structures` 的区块写进临时世界目录，再用官方 `world-dir` 逐块解析。
/// Write chunks carrying the tag into a temp world dir, then verify with `world-dir`.
fn verify_world_dir(work_dir: &Path, java: &str, cases: &[Case]) -> Result<(), String> {
    let dir = work_dir.join("world-dir-structure-save");
    let _ = fs::remove_dir_all(&dir);
    let world = WorldDir::new(&dir);
    world
        .create_layout()
        .map_err(|e| format!("cannot create world layout: {e}"))?;
    let region_dir = world.region_dir();
    let template = ChunkColumn::from_template();

    let region_path = region_dir.join(RegionPos::of(ChunkPos::new(0, 0)).file_name());
    let mut region = RegionFile::open(&region_path, &region_dir, RegionCompression::Zlib)
        .map_err(|e| format!("cannot open region: {e}"))?;
    for (i, c) in cases.iter().take(4).enumerate() {
        let pos = ChunkPos::new(i as i32, 0);
        let bytes = encode_bytes(
            &template,
            &ChunkEncodeExtra {
                chunk_pos: pos,
                structures: Some(rust_tag(c)),
                ..ChunkEncodeExtra::default()
            },
        )
        .map_err(|e| format!("cannot encode chunk {i}: {e}"))?;
        region
            .write(pos, Some(&bytes), 1)
            .map_err(|e| format!("cannot write chunk {i}: {e}"))?;
    }
    region
        .sync()
        .map_err(|e| format!("cannot sync region: {e}"))?;
    drop(region);

    crate::world_dir::run(dir.to_string_lossy().as_ref(), java)
}

/// 把 `Tag` 转成按键排序的规范 JSON，和 `StorageOracle.java` 的 `canonical` 一致。
fn canonical_tag(tag: &Tag) -> Value {
    match tag {
        Tag::Byte(v) => json!({"type": 1, "value": v.to_string()}),
        Tag::Short(v) => json!({"type": 2, "value": v.to_string()}),
        Tag::Int(v) => json!({"type": 3, "value": v.to_string()}),
        Tag::Long(v) => json!({"type": 4, "value": v.to_string()}),
        Tag::Float(v) => json!({"type": 5, "value": v.to_string()}),
        Tag::Double(v) => json!({"type": 6, "value": v.to_string()}),
        Tag::String(v) => json!({"type": 8, "value": v}),
        Tag::ByteArray(v) => json!({"type": 7, "values": v}),
        Tag::IntArray(v) => json!({"type": 11, "values": v}),
        Tag::LongArray(v) => json!({"type": 12, "values": v}),
        Tag::List(v) => json!({
            "type": 9,
            "values": v.iter().map(canonical_tag).collect::<Vec<_>>()
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
