//! `poi/r.X.Z.mca` 里一个区块的 NBT 编解码（官方 `SectionStorage.writeChunk`）。
//!
//! 根标签：`{ "Sections": { "<y>": { "Valid": bool, "Records": [...] } }, "DataVersion": int }`。
//! `PoiSection$Packed.CODEC` 的 `Valid` 是 `lenientOptionalFieldOf`（缺省 false），
//! `Records` 必填；`PoiRecord$Packed.CODEC` 的 `pos` 是 `BlockPos.CODEC`
//! （3 元 `IntArray`），`type` 是 POI 类型的注册表名字，`free_tickets` 缺省 0。
//!
//! 盘上的数据一律当成不可信输入：坏标签、未知类型、越界坐标都跳过或报错，
//! 不 panic。官方 `SectionStorage$PackedChunk.parse` 只认 `minSectionY..maxSectionY`
//! 的十进制键，其余忽略；读到的旧 `DataVersion` 交给调用方决定要不要重建无效段。

use std::fmt::Write as _;

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::version;
use mc_bamboo_nbt::Tag;
use mc_bamboo_world::poi::{
    MAX_SECTION_Y, MIN_SECTION_Y, PoiRecord, PoiSection, type_from_name, type_name,
};

use crate::chunk_nbt::field;

/// 编码/解码失败的原因。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PoiNbtError {
    #[error("POI root must be a compound tag")]
    NotCompound,
    #[error("POI DataVersion must be an int")]
    BadDataVersion,
    #[error("unsupported POI DataVersion {0} (expected {expected})", expected = version::DATA_VERSION)]
    NewerDataVersion(i32),
    #[error("POI Sections must be a compound tag")]
    BadSections,
}

/// 一个区块读回来的 POI 数据。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoiChunk {
    /// 按 y 升序；只含盘上写了的段。
    pub sections: Vec<(i32, PoiSection)>,
    /// `DataVersion != 当前版本`：官方 `PackedChunk.versionChanged` 会给读到的段
    /// 调 `setDirty`；`Valid=false` 的段随后由 `checkConsistencyWithBlocks` 重建。
    pub version_changed: bool,
}

/// 编出官方 `SectionStorage.writeChunk` 的根标签。
///
/// `sections` 只含已加载（非 `Optional.empty()`）的段，按 y 升序；空段也照样写
/// `{Valid:true, Records:[]}`，和官方一致。
pub fn encode(sections: &[(i32, &PoiSection)]) -> Tag {
    let mut packed = Vec::with_capacity(sections.len());
    for &(y, section) in sections {
        let mut key = String::new();
        let _ = write!(key, "{y}");
        packed.push((key, encode_section(section)));
    }
    Tag::Compound(vec![
        ("Sections".to_owned(), Tag::Compound(packed)),
        ("DataVersion".to_owned(), Tag::Int(version::DATA_VERSION)),
    ])
}

fn encode_section(section: &PoiSection) -> Tag {
    let records = section
        .records()
        .map(encode_record)
        .map(Tag::Compound)
        .collect();
    Tag::Compound(vec![
        ("Valid".to_owned(), Tag::Byte(i8::from(section.is_valid()))),
        ("Records".to_owned(), Tag::List(records)),
    ])
}

fn encode_record(record: &PoiRecord) -> Vec<(String, Tag)> {
    let mut out = vec![
        (
            "pos".to_owned(),
            Tag::IntArray(vec![record.pos.x, record.pos.y, record.pos.z]),
        ),
        (
            "type".to_owned(),
            Tag::String(
                type_name(record.kind)
                    .map(|name| format!("minecraft:{name}"))
                    .unwrap_or_else(|| "minecraft:unknown".to_owned()),
            ),
        ),
    ];
    // 官方 `Codec.INT.optionalFieldOf("free_tickets", 0)`：等于缺省值时不写这个键
    // （0 票类型和扣光票的记录都省掉）。
    if record.free_tickets != 0 {
        out.push(("free_tickets".to_owned(), Tag::Int(record.free_tickets)));
    }
    out
}

/// 读回一个区块的 POI 根标签。
///
/// 坏数据照官方处理：`Sections` 缺失 / 类型错时整个区块读成「没有段」，
/// 单段或单条记录坏了就跳过那一段 / 那一条（官方 codec 的 `resultOrPartial`）。
/// 新版本直接报错，旧版本标记 `version_changed`。
pub fn decode(root: &Tag) -> Result<PoiChunk, PoiNbtError> {
    if !matches!(root, Tag::Compound(_)) {
        return Err(PoiNbtError::NotCompound);
    }
    let Some(Tag::Int(data_version)) = field(root, "DataVersion") else {
        return Err(PoiNbtError::BadDataVersion);
    };
    let data_version = *data_version;
    if data_version > version::DATA_VERSION {
        return Err(PoiNbtError::NewerDataVersion(data_version));
    }
    let mut out = PoiChunk {
        version_changed: data_version != version::DATA_VERSION,
        ..PoiChunk::default()
    };
    let empty_sections = Tag::Compound(Vec::new());
    let Tag::Compound(entries) = field(root, "Sections").unwrap_or(&empty_sections) else {
        return Ok(out);
    };
    for (key, tag) in entries {
        let Ok(y) = key.parse::<i32>() else {
            continue;
        };
        if !(MIN_SECTION_Y..=MAX_SECTION_Y).contains(&y) {
            continue;
        }
        if let Some(section) = decode_section(tag) {
            // 同名键官方 `CompoundTag` 取最后一个；这里按后出现的覆盖。
            if let Some(slot) = out.sections.iter_mut().find(|(existing, _)| *existing == y) {
                slot.1 = section;
            } else {
                out.sections.push((y, section));
            }
        }
    }
    out.sections.sort_by_key(|&(y, _)| y);
    Ok(out)
}

fn decode_section(tag: &Tag) -> Option<PoiSection> {
    let Tag::Compound(_) = tag else {
        return None;
    };
    let valid = matches!(field(tag, "Valid"), Some(Tag::Byte(value)) if *value != 0);
    let Tag::List(records) = field(tag, "Records")? else {
        return None;
    };
    let mut out = Vec::with_capacity(records.len());
    for tag in records {
        if let Some(record) = decode_record(tag) {
            out.push(record);
        }
    }
    Some(PoiSection::from_packed(valid, out))
}

fn decode_record(tag: &Tag) -> Option<PoiRecord> {
    let Tag::IntArray(values) = field(tag, "pos")? else {
        return None;
    };
    let [x, y, z] = values.as_slice() else {
        return None;
    };
    let Tag::String(name) = field(tag, "type")? else {
        return None;
    };
    let kind = type_from_name(name)?;
    let free_tickets = match field(tag, "free_tickets") {
        Some(Tag::Int(value)) => *value,
        _ => 0,
    };
    Some(PoiRecord {
        pos: BlockPos::new(*x, *y, *z),
        kind,
        free_tickets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::poi;
    use mc_bamboo_world::poi::Occupancy;

    fn sample() -> PoiSection {
        let home = poi::poi_type_id("home").unwrap();
        let meeting = poi::poi_type_id("meeting").unwrap();
        let mut section = PoiSection::empty();
        section.add(BlockPos::new(1, -60, 2), home);
        section.add(BlockPos::new(3, -60, 5), meeting);
        assert!(
            section
                .get_mut(BlockPos::new(1, -60, 2))
                .unwrap()
                .acquire_ticket()
        );
        section
    }

    #[test]
    fn the_official_keys_round_trip() {
        let section = sample();
        let root = encode(&[(-4, &section)]);
        let Tag::Compound(entries) = &root else {
            unreachable!()
        };
        assert_eq!(entries[0].0, "Sections");
        assert_eq!(entries[1].0, "DataVersion");
        assert_eq!(entries[1].1, Tag::Int(version::DATA_VERSION));
        let Tag::Compound(sections) = &entries[0].1 else {
            unreachable!()
        };
        assert_eq!(sections[0].0, "-4");
        let Tag::Compound(packed) = &sections[0].1 else {
            unreachable!()
        };
        assert_eq!(packed[0].0, "Valid");
        assert_eq!(packed[0].1, Tag::Byte(1));
        assert_eq!(packed[1].0, "Records");
        let Tag::List(records) = &packed[1].1 else {
            unreachable!()
        };
        assert_eq!(records.len(), 2);

        let decoded = decode(&root).unwrap();
        assert!(!decoded.version_changed);
        assert_eq!(decoded.sections.len(), 1);
        assert_eq!(decoded.sections[0].0, -4);
        let back = &decoded.sections[0].1;
        assert!(back.is_valid());
        assert_eq!(back.records().count(), 2);
        assert_eq!(back.get(BlockPos::new(1, -60, 2)).unwrap().free_tickets, 0);
        assert_eq!(back.get(BlockPos::new(3, -60, 5)).unwrap().free_tickets, 32);
        assert_eq!(back.find(|_| true, Occupancy::IsOccupied).len(), 1);
    }

    #[test]
    fn bad_data_is_skipped_without_panicking() {
        // 根不是 compound。
        assert_eq!(decode(&Tag::Int(1)), Err(PoiNbtError::NotCompound));
        // 新版本。
        let newer = Tag::Compound(vec![
            ("Sections".to_owned(), Tag::Compound(Vec::new())),
            (
                "DataVersion".to_owned(),
                Tag::Int(version::DATA_VERSION + 1),
            ),
        ]);
        assert!(matches!(
            decode(&newer),
            Err(PoiNbtError::NewerDataVersion(_))
        ));
        // 旧版本：标记重建，段照样读回来。
        let older = Tag::Compound(vec![
            (
                "Sections".to_owned(),
                Tag::Compound(vec![(
                    "-4".to_owned(),
                    encode_section(&PoiSection::empty()),
                )]),
            ),
            (
                "DataVersion".to_owned(),
                Tag::Int(version::DATA_VERSION - 1),
            ),
        ]);
        let decoded = decode(&older).unwrap();
        assert!(decoded.version_changed);
        assert_eq!(decoded.sections.len(), 1);
        // 坏键 / 坏段 / 未知类型 / 坏 pos 都跳过。
        let bad = Tag::Compound(vec![
            (
                "Sections".to_owned(),
                Tag::Compound(vec![
                    (
                        "not a number".to_owned(),
                        encode_section(&PoiSection::empty()),
                    ),
                    ("999".to_owned(), encode_section(&PoiSection::empty())),
                    ("-4".to_owned(), Tag::Int(7)),
                    (
                        "-3".to_owned(),
                        Tag::Compound(vec![
                            ("Valid".to_owned(), Tag::Byte(0)),
                            (
                                "Records".to_owned(),
                                Tag::List(vec![
                                    Tag::Compound(vec![
                                        ("pos".to_owned(), Tag::IntArray(vec![1, -60, 2])),
                                        (
                                            "type".to_owned(),
                                            Tag::String("minecraft:nope".to_owned()),
                                        ),
                                    ]),
                                    Tag::Compound(vec![
                                        ("pos".to_owned(), Tag::IntArray(vec![1, -60])),
                                        (
                                            "type".to_owned(),
                                            Tag::String("minecraft:home".to_owned()),
                                        ),
                                    ]),
                                ]),
                            ),
                        ]),
                    ),
                ]),
            ),
            ("DataVersion".to_owned(), Tag::Int(version::DATA_VERSION)),
        ]);
        let decoded = decode(&bad).unwrap();
        assert_eq!(decoded.sections.len(), 1);
        assert_eq!(decoded.sections[0].0, -3);
        assert!(!decoded.sections[0].1.is_valid(), "Valid 缺省 false");
        assert_eq!(decoded.sections[0].1.records().count(), 0);
    }
}
