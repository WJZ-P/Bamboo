//! 区块 NBT 里跨类型的组装规则：方块实体条目按位置合并、计划刻放到顶层、
//! 实体文件的根 `{ DataVersion, Position, Entities }`（`docs/m3-storage.md` 5.4 / 5.5）。
//!
//! 这一层只认 [`Tag`]，不认识 `mc-bamboo-game` 或 `mc-bamboo-blocks` 的类型：各层把自己
//! 建模的键写成复合标签，交给这里合并。读进来的原始条目里不认识的键、不认识的方块实体
//! 类型、不认识的实体类型、不认识的计划刻 `i` 一律原样保留并写回（D9 选 A）。
//! 这一段是纯函数，不读写世界目录；第二段再把 `RegionColumn` 的各部件接进来。

use mc_bamboo_core::ChunkPos;
use mc_bamboo_data::{block_by_name, registries::fluid, version};
use mc_bamboo_nbt::Tag;

use crate::chunk_nbt::{ChunkEncodeExtra, field};

/// 官方 `EntityStorage.storeEntities` 认识的根键。
const ENTITY_ROOT_KEYS: [&str; 3] = ["DataVersion", "Position", "Entities"];
/// Bamboo 建模的三种实体。
const MODELED_ENTITIES: [&str; 3] = ["minecraft:falling_block", "minecraft:tnt", "minecraft:item"];

/// 区块上层内容编解码失败的原因。所有输入都当成不可信数据，不 panic。
#[derive(Debug, thiserror::Error)]
pub enum ChunkContentError {
    #[error("chunk content NBT: {0}")]
    Malformed(String),
    #[error("unsupported entity DataVersion {0} (expected {expected})", expected = version::DATA_VERSION)]
    UnsupportedDataVersion(i32),
}

/// 一条建模过的方块实体条目：官方 `saveWithFullMetadata` 加 `keepPacked` 的全部键。
///
/// `omit` 里的键是官方可省、当前状态没有值时要从原样保留的条目里删掉的。
/// `invalidate` 描述「和建模状态绑在一起的未建模键」：当 `watch` 的建模值与保留值
/// 不同时，删掉 `bound` 里的保留键，让建模值（或官方默认值）生效。目前只有唱片机的
/// `RecordItem → ticks_since_song_started` 用得上。
#[derive(Debug, Clone)]
pub struct ModeledBlockEntity {
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub fields: Vec<(String, Tag)>,
    pub omit: Vec<String>,
    pub invalidate: Vec<BoundKeys>,
}

/// 一组「触发键变了就作废」的保留键。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundKeys {
    pub watch: String,
    pub bound: Vec<String>,
}

impl ModeledBlockEntity {
    /// 没有额外字段的骨架，调用方往 `fields` 里补自己建模的键。
    pub fn new(id: &str, x: i32, y: i32, z: i32) -> Self {
        Self {
            id: id.to_owned(),
            x,
            y,
            z,
            fields: Vec::new(),
            omit: Vec::new(),
            invalidate: Vec::new(),
        }
    }

    pub fn put(&mut self, key: &str, value: Tag) {
        set(&mut self.fields, key, value);
    }
}

/// 把建模过的条目和读盘时的原始条目按位置合并。
///
/// - 同一位置、同一 `id` 时以原始条目为底：建模键覆盖，其余键原样保留；
/// - `id` 换了（玩家拆掉箱子原地放熔炉）时旧条目的键全部丢掉；
/// - 原始条目里 `id` 属于 `modeled_ids`（Bamboo 会建模的类型）却没有对应条目时丢掉；
///   不认识的类型整条原样保留；
/// - 每条输出都补上官方必写的 `components`、`keepPacked`，除非原条目已有值。
pub fn merge_block_entities(
    modeled: Vec<ModeledBlockEntity>,
    raw: &[Tag],
    modeled_ids: &[&str],
) -> Result<Vec<Tag>, ChunkContentError> {
    let mut out = Vec::with_capacity(modeled.len() + raw.len());
    for entry in modeled {
        let base = raw
            .iter()
            .find(|tag| position(tag) == Some((entry.x, entry.y, entry.z)))
            .filter(|tag| id_of(tag) == Some(entry.id.as_str()));
        let mut fields = Vec::new();
        if let Some(Tag::Compound(values)) = base {
            fields.extend(values.iter().cloned());
        }
        // 绑定的未建模键在触发键变了之后失效。
        for bound in &entry.invalidate {
            let changed = match (
                find(&fields, &bound.watch),
                find(&entry.fields, &bound.watch),
            ) {
                (Some(raw_value), Some(modeled_value)) => raw_value != modeled_value,
                // 新方块实体或者保留条目里没有触发键，按「变了」处理。
                _ => true,
            };
            if changed {
                for key in &bound.bound {
                    fields.retain(|(existing, _)| existing != key);
                }
            }
        }
        for key in &entry.omit {
            fields.retain(|(existing, _)| existing != key);
        }
        set(&mut fields, "id", Tag::String(entry.id.clone()));
        set(&mut fields, "x", Tag::Int(entry.x));
        set(&mut fields, "y", Tag::Int(entry.y));
        set(&mut fields, "z", Tag::Int(entry.z));
        for (key, value) in &entry.fields {
            set(&mut fields, key, value.clone());
        }
        if find(&fields, "components").is_none() {
            set(&mut fields, "components", Tag::Compound(Vec::new()));
        }
        if find(&fields, "keepPacked").is_none() {
            set(&mut fields, "keepPacked", Tag::Byte(0));
        }
        out.push(Tag::Compound(fields));
        // 用掉的原始条目后面不能再当作「不认识的条目」写回。
    }
    let used: Vec<(i32, i32, i32)> = out.iter().filter_map(position).collect();
    for tag in raw {
        let Some((x, y, z)) = position(tag) else {
            // 没有坐标的坏条目也原样留着，官方 getPosFromTag 会给个默认位置。
            out.push(tag.clone());
            continue;
        };
        if used.contains(&(x, y, z)) {
            continue;
        }
        let id = id_of(tag).unwrap_or("");
        if modeled_ids.contains(&id) {
            // Bamboo 会建模的类型：方块没了，旧数据不能挂到新方块上。
            continue;
        }
        out.push(tag.clone());
    }
    Ok(out)
}

/// 把两条计划刻列表放进 `ChunkEncodeExtra`。
///
/// 读盘原始列表里 `i` 不认识的条目原样保留在最前面（`LevelChunkTicks.pack` 先放
/// pending），其余由建模列表按执行顺序覆盖；两条列表即使为空也写。
pub fn merge_tick_lists(
    modeled_blocks: Vec<Tag>,
    modeled_fluids: Vec<Tag>,
    raw_blocks: &[Tag],
    raw_fluids: &[Tag],
    extra: &mut ChunkEncodeExtra<'_>,
) {
    extra.block_ticks = merge_one_tick_list(modeled_blocks, raw_blocks, true);
    extra.fluid_ticks = merge_one_tick_list(modeled_fluids, raw_fluids, false);
}

fn merge_one_tick_list(modeled: Vec<Tag>, raw: &[Tag], is_block: bool) -> Vec<Tag> {
    let mut out = Vec::with_capacity(raw.len() + modeled.len());
    for tag in raw {
        let known = match field(tag, "i") {
            Some(Tag::String(name)) if is_block => block_by_name(name).is_some(),
            Some(Tag::String(name)) => fluid::by_name(name).is_some(),
            _ => false,
        };
        if !known {
            out.push(tag.clone());
        }
    }
    out.extend(modeled);
    out
}

/// 实体文件的一段内容。`entities` 是 Bamboo 建模的三种实体，`preserved_entities` 是
/// 不认识的实体类型（官方世界里的生物等），`preserved` 是根复合标签里不认识的键。
#[derive(Debug, Clone, PartialEq)]
pub struct EntityFile {
    pub position: ChunkPos,
    pub entities: Vec<Tag>,
    pub preserved_entities: Vec<Tag>,
    pub preserved: Vec<(String, Tag)>,
    pub problems: Vec<String>,
}

impl EntityFile {
    pub fn empty(position: ChunkPos) -> Self {
        Self {
            position,
            entities: Vec::new(),
            preserved_entities: Vec::new(),
            preserved: Vec::new(),
            problems: Vec::new(),
        }
    }

    /// 官方 `EntityStorage.storeEntities`：空的区块走 `IOWorker.STORE_EMPTY`（没有文件）。
    pub fn encode(&self) -> Option<Tag> {
        if self.entities.is_empty() && self.preserved_entities.is_empty() {
            return None;
        }
        let mut entities = self.entities.clone();
        entities.extend(self.preserved_entities.iter().cloned());
        let mut root = vec![
            ("DataVersion".to_owned(), Tag::Int(version::DATA_VERSION)),
            (
                "Position".to_owned(),
                Tag::IntArray(vec![self.position.x, self.position.z]),
            ),
            ("Entities".to_owned(), Tag::List(entities)),
        ];
        for (key, value) in &self.preserved {
            set(&mut root, key, value.clone());
        }
        Some(Tag::Compound(root))
    }

    /// 官方 `EntityStorage.loadEntities` 的根结构。`Position` 不对只记问题，
    /// 用请求的区块坐标继续；`DataVersion` 不等于 5023 明确拒绝。
    pub fn decode(root: &Tag, expected: ChunkPos) -> Result<Self, ChunkContentError> {
        let Tag::Compound(values) = root else {
            return Err(ChunkContentError::Malformed(
                "entity file root must be a compound tag".to_owned(),
            ));
        };
        let data_version = match field(root, "DataVersion") {
            Some(Tag::Int(value)) => *value,
            _ => {
                return Err(ChunkContentError::Malformed(
                    "missing DataVersion".to_owned(),
                ));
            }
        };
        if data_version != version::DATA_VERSION {
            return Err(ChunkContentError::UnsupportedDataVersion(data_version));
        }
        let mut problems = Vec::new();
        let position = match field(root, "Position") {
            Some(Tag::IntArray(values)) if values.len() == 2 => ChunkPos::new(values[0], values[1]),
            _ => {
                problems.push("entity file has no Position".to_owned());
                expected
            }
        };
        if position != expected {
            problems.push(format!(
                "entity file is at ({},{}) but the chunk is ({},{})",
                position.x, position.z, expected.x, expected.z
            ));
        }
        let entities = match field(root, "Entities") {
            Some(Tag::List(values)) => values.clone(),
            None => Vec::new(),
            _ => {
                problems.push("Entities is not a list".to_owned());
                Vec::new()
            }
        };
        let (modeled, preserved_entities) = split_entity_entries(entities);
        let preserved = values
            .iter()
            .filter(|(key, _)| !ENTITY_ROOT_KEYS.contains(&key.as_str()))
            .cloned()
            .collect();
        Ok(Self {
            position: expected,
            entities: modeled,
            preserved_entities,
            preserved,
            problems,
        })
    }
}

/// 认识的实体交给 `mc-bamboo-blocks::archive::entity::read_entity` 建模，
/// 不认识的类型整条原样保留，按区块写回。
pub fn split_entity_entries(entities: Vec<Tag>) -> (Vec<Tag>, Vec<Tag>) {
    let mut modeled = Vec::new();
    let mut preserved = Vec::new();
    for tag in entities {
        let known = match field(&tag, "id") {
            Some(Tag::String(id)) => MODELED_ENTITIES.contains(&id.as_str()),
            _ => false,
        };
        if known {
            modeled.push(tag);
        } else {
            preserved.push(tag);
        }
    }
    (modeled, preserved)
}

fn set(entries: &mut Vec<(String, Tag)>, key: &str, value: Tag) {
    entries.retain(|(existing, _)| existing != key);
    entries.push((key.to_owned(), value));
}

fn find<'a>(entries: &'a [(String, Tag)], key: &str) -> Option<&'a Tag> {
    entries
        .iter()
        .rev()
        .find(|(existing, _)| existing == key)
        .map(|(_, value)| value)
}

fn position(tag: &Tag) -> Option<(i32, i32, i32)> {
    let x = match field(tag, "x")? {
        Tag::Int(value) => *value,
        _ => return None,
    };
    let y = match field(tag, "y")? {
        Tag::Int(value) => *value,
        _ => return None,
    };
    let z = match field(tag, "z")? {
        Tag::Int(value) => *value,
        _ => return None,
    };
    Some((x, y, z))
}

fn id_of(tag: &Tag) -> Option<&str> {
    match field(tag, "id") {
        Some(Tag::String(value)) => Some(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compound(entries: Vec<(String, Tag)>) -> Tag {
        Tag::Compound(entries)
    }

    fn raw_entity(id: &str, extra: &[(&str, Tag)]) -> Tag {
        let mut entries = vec![("id".to_owned(), Tag::String(id.to_owned()))];
        entries.extend(
            extra
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone())),
        );
        compound(entries)
    }

    #[test]
    fn modeled_keys_win_and_unknown_keys_are_preserved() {
        let pos = (3, 64, 5);
        let raw = raw_entity(
            "minecraft:chest",
            &[
                ("x", Tag::Int(pos.0)),
                ("y", Tag::Int(pos.1)),
                ("z", Tag::Int(pos.2)),
                ("CustomName", Tag::String("旧名字".to_owned())),
                ("Bees", Tag::List(vec![Tag::Byte(1)])),
                ("Items", Tag::List(vec![Tag::Byte(7)])),
                (
                    "components",
                    compound(vec![("kept".to_owned(), Tag::Byte(1))]),
                ),
                ("keepPacked", Tag::Byte(1)),
            ],
        );
        let mut modeled = ModeledBlockEntity::new("minecraft:chest", pos.0, pos.1, pos.2);
        modeled.put("Items", Tag::List(Vec::new()));
        modeled.put("CustomName", Tag::String("新名字".to_owned()));
        let out = merge_block_entities(vec![modeled], &[raw], &["minecraft:chest"]).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(field(&out[0], "Items"), Some(&Tag::List(Vec::new())));
        assert_eq!(
            field(&out[0], "CustomName"),
            Some(&Tag::String("新名字".to_owned()))
        );
        assert_eq!(field(&out[0], "Bees"), Some(&Tag::List(vec![Tag::Byte(1)])));
        assert_eq!(field(&out[0], "keepPacked"), Some(&Tag::Byte(1)));
        assert!(
            matches!(field(&out[0], "components"), Some(Tag::Compound(values)) if values.len() == 1)
        );
    }

    #[test]
    fn changing_the_id_discards_old_state_and_unknown_ids_survive() {
        let chest = raw_entity(
            "minecraft:chest",
            &[
                ("x", Tag::Int(1)),
                ("y", Tag::Int(2)),
                ("z", Tag::Int(3)),
                ("Items", Tag::List(vec![Tag::Byte(9)])),
            ],
        );
        let furnace = ModeledBlockEntity::new("minecraft:furnace", 1, 2, 3);
        let alien = raw_entity(
            "minecraft:alien",
            &[
                ("x", Tag::Int(4)),
                ("y", Tag::Int(5)),
                ("z", Tag::Int(6)),
                ("Whatever", Tag::Byte(3)),
            ],
        );
        let out = merge_block_entities(
            vec![furnace],
            &[chest, alien],
            &["minecraft:chest", "minecraft:furnace"],
        )
        .unwrap();
        assert_eq!(out.len(), 2);
        let furnace_tag = &out[0];
        assert_eq!(id_of(furnace_tag), Some("minecraft:furnace"));
        assert!(field(furnace_tag, "Items").is_none());
        assert_eq!(
            field(furnace_tag, "components"),
            Some(&compound(Vec::new()))
        );
        assert_eq!(field(furnace_tag, "keepPacked"), Some(&Tag::Byte(0)));
        assert_eq!(id_of(&out[1]), Some("minecraft:alien"));
        assert_eq!(field(&out[1], "Whatever"), Some(&Tag::Byte(3)));
    }

    #[test]
    fn a_removed_modeled_entity_drops_its_raw_entry() {
        let raw = raw_entity(
            "minecraft:chest",
            &[
                ("x", Tag::Int(1)),
                ("y", Tag::Int(2)),
                ("z", Tag::Int(3)),
                ("Items", Tag::List(vec![Tag::Byte(9)])),
            ],
        );
        let out = merge_block_entities(Vec::new(), &[raw], &["minecraft:chest"]).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn bound_unmodeled_keys_are_invalidated_when_the_record_changes() {
        let pos = (0, 64, 0);
        let raw = raw_entity(
            "minecraft:jukebox",
            &[
                ("x", Tag::Int(pos.0)),
                ("y", Tag::Int(pos.1)),
                ("z", Tag::Int(pos.2)),
                ("RecordItem", Tag::String("old".to_owned())),
                ("ticks_since_song_started", Tag::Long(900)),
            ],
        );
        let mut modeled = ModeledBlockEntity::new("minecraft:jukebox", pos.0, pos.1, pos.2);
        modeled.put("RecordItem", Tag::String("new".to_owned()));
        modeled.put("ticks_since_song_started", Tag::Long(0));
        modeled.invalidate.push(BoundKeys {
            watch: "RecordItem".to_owned(),
            bound: vec!["ticks_since_song_started".to_owned()],
        });
        let out = merge_block_entities(vec![modeled], &[raw], &["minecraft:jukebox"]).unwrap();
        assert_eq!(
            field(&out[0], "ticks_since_song_started"),
            Some(&Tag::Long(0))
        );
    }

    #[test]
    fn tick_lists_keep_unknown_ids_and_write_empty_lists() {
        let mut extra = ChunkEncodeExtra::default();
        let unknown = compound(vec![
            (
                "i".to_owned(),
                Tag::String("minecraft:not_a_block".to_owned()),
            ),
            ("t".to_owned(), Tag::Int(7)),
        ]);
        let known = compound(vec![
            ("i".to_owned(), Tag::String("minecraft:stone".to_owned())),
            ("t".to_owned(), Tag::Int(1)),
        ]);
        merge_tick_lists(
            Vec::new(),
            vec![known.clone()],
            &[unknown.clone(), known],
            &[],
            &mut extra,
        );
        assert_eq!(extra.block_ticks, vec![unknown]);
        assert_eq!(
            extra.fluid_ticks,
            vec![compound(vec![
                ("i".to_owned(), Tag::String("minecraft:stone".to_owned())),
                ("t".to_owned(), Tag::Int(1)),
            ])]
        );
    }

    #[test]
    fn entity_file_round_trips_and_preserves_unknown_data() {
        let position = ChunkPos::new(3, -9);
        let mut file = EntityFile::empty(position);
        file.entities
            .push(raw_entity("minecraft:tnt", &[("fuse", Tag::Short(80))]));
        file.preserved_entities
            .push(raw_entity("minecraft:cow", &[("Health", Tag::Float(10.0))]));
        file.preserved
            .push(("ModData".to_owned(), Tag::String("keep".to_owned())));
        let tag = file.encode().expect("非空实体文件");
        assert_eq!(
            field(&tag, "DataVersion"),
            Some(&Tag::Int(version::DATA_VERSION))
        );
        assert_eq!(field(&tag, "Position"), Some(&Tag::IntArray(vec![3, -9])));
        let decoded = EntityFile::decode(&tag, position).unwrap();
        assert_eq!(decoded.position, position);
        assert_eq!(decoded.entities, file.entities);
        assert_eq!(decoded.preserved_entities, file.preserved_entities);
        assert_eq!(decoded.preserved, file.preserved);
        assert!(decoded.problems.is_empty());
        assert_eq!(decoded.encode(), Some(tag));
        assert_eq!(EntityFile::empty(position).encode(), None);
    }

    #[test]
    fn entity_file_reports_bad_positions_and_rejects_other_versions() {
        let position = ChunkPos::new(1, 2);
        let root = compound(vec![
            ("DataVersion".to_owned(), Tag::Int(version::DATA_VERSION)),
            ("Entities".to_owned(), Tag::List(Vec::new())),
        ]);
        let decoded = EntityFile::decode(&root, position).unwrap();
        assert_eq!(decoded.position, position);
        assert_eq!(decoded.problems.len(), 1);
        let wrong = compound(vec![("DataVersion".to_owned(), Tag::Int(1))]);
        assert!(matches!(
            EntityFile::decode(&wrong, position),
            Err(ChunkContentError::UnsupportedDataVersion(1))
        ));
    }
}
