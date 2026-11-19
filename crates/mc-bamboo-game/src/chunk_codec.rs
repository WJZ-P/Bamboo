//! 区块 NBT 的解析和序列化，放在一个以后能挪进计算池的接口后面。
//!
//! 现在用 [`InlineCodec`] 在世界线程上内联跑（设计稿 5.3 的「先用内联执行器」），
//! 调用方用剖析器量耗时。压缩/解压不在这里，在 IO 线程的区域文件里做。
//!
//! `Director` 只搬运不透明的 `Vec<u8>`，所以区块 NBT 和实体 NBT 两份字节要
//! 用 [`PackedChunk`] 装成一个容器；这是 Bamboo 内部格式，磁盘上仍然是官方的两份文件。
//!
//! Chunk NBT parsing and serialization behind an interface that can later move to
//! a compute pool. [`InlineCodec`] runs inline on the world thread (design 5.3),
//! timed by the profiler. Compression lives on the IO thread, not here. The
//! `Director` only carries opaque bytes, so chunk + entity NBT are bundled with
//! [`PackedChunk`]; on disk they remain the two official files.

use mc_bamboo_core::ChunkPos;
use mc_bamboo_nbt::{Accounter, Tag, read_named, write_named};
use mc_bamboo_world::ticket::ChunkStatus;
use tracing::warn;

use crate::region::{RegionColumn, chunk_content, chunk_content::EncodeParts};

/// 容器版本号，读的时候校验。
const MAGIC: u8 = 1;

/// 一个区块的两份未压缩 NBT 字节。`chunk` 为 `None` 表示盘上没有这个区块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedChunk {
    pub chunk: Option<Vec<u8>>,
    pub entities: Option<Vec<u8>>,
}

impl PackedChunk {
    /// 装进一个自描述的字节容器，交给 `Director` 搬运。
    pub fn pack(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(MAGIC);
        push_blob(&mut out, self.chunk.as_deref());
        push_blob(&mut out, self.entities.as_deref());
        out
    }

    /// 从字节容器拆出来。坏数据一律返回 `Err`，不 panic。
    pub fn unpack(bytes: &[u8]) -> Result<Self, String> {
        if bytes.first() != Some(&MAGIC) {
            return Err("bad chunk container magic".to_owned());
        }
        let mut rest = &bytes[1..];
        let chunk = take_blob(&mut rest)?;
        let entities = take_blob(&mut rest)?;
        if !rest.is_empty() {
            return Err("trailing bytes in chunk container".to_owned());
        }
        Ok(Self { chunk, entities })
    }
}

fn push_blob(out: &mut Vec<u8>, blob: Option<&[u8]>) {
    match blob {
        Some(bytes) => {
            out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            out.extend_from_slice(bytes);
        }
        None => out.extend_from_slice(&0_u32.to_be_bytes()),
    }
}

fn take_blob(rest: &mut &[u8]) -> Result<Option<Vec<u8>>, String> {
    let header = rest
        .get(..4)
        .ok_or_else(|| "truncated chunk container".to_owned())?;
    let len = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
    *rest = &rest[4..];
    if len == 0 {
        return Ok(None);
    }
    let bytes = rest
        .get(..len)
        .ok_or_else(|| "truncated chunk payload".to_owned())?
        .to_vec();
    *rest = &rest[len..];
    Ok(Some(bytes))
}

/// 解码出来的盒子连同盘上的生成阶段。`full` 直接交付，其余阶段要续跑流水线。
///
/// A decoded box and its on-disk generation stage. `full` is delivered as is;
/// earlier stages resume generation.
#[derive(Debug)]
pub struct DecodedColumn {
    pub column: Box<RegionColumn>,
    pub persisted_status: ChunkStatus,
}

/// 解析/序列化接口。以后可以换成把工作丢进计算池的实现。
pub trait ChunkCodec: Send {
    /// 把一个已经从 region 收回的盒子编码成两份 NBT 字节。
    fn encode(&self, pos: ChunkPos, column: &RegionColumn) -> Result<PackedChunk, String>;
    /// 把两份 NBT 字节解码成盒子 + 盘上的 `Status`。
    fn decode(&self, pos: ChunkPos, packed: &PackedChunk) -> Result<DecodedColumn, String>;
}

/// 把只读部件（`RegionColumn` 或 `BorrowedParts`）编成两份 NBT 字节。
/// 卸载存盘和已加载区块的只读快照共用这一条路径（补 11）。
/// Encode read-only parts (`RegionColumn` or `BorrowedParts`) into both NBT
/// blobs; unload saves and loaded-chunk read-only snapshots share it (item 11).
pub(crate) fn encode_parts(pos: ChunkPos, parts: &impl EncodeParts) -> Result<PackedChunk, String> {
    let encoded = chunk_content::encode_parts(parts, pos).map_err(|e| e.to_string())?;
    Ok(PackedChunk {
        chunk: Some(named_bytes(&encoded.chunk)),
        entities: encoded.entities.as_ref().map(named_bytes),
    })
}

/// 内联执行器：直接在调用它的线程上跑完编码/解码。
pub struct InlineCodec;

impl ChunkCodec for InlineCodec {
    fn encode(&self, pos: ChunkPos, column: &RegionColumn) -> Result<PackedChunk, String> {
        encode_parts(pos, column)
    }

    fn decode(&self, pos: ChunkPos, packed: &PackedChunk) -> Result<DecodedColumn, String> {
        let chunk_bytes = packed
            .chunk
            .as_deref()
            .ok_or_else(|| "missing chunk NBT".to_owned())?;
        // `chunk_content::decode` → `chunk_nbt::decode` 会检查 DataVersion；
        // 非 full 的 ProtoChunk 也按盘上的方块 / 群系 / 高度图 / 光照读回来，
        // `persisted_status` 交给 Director 决定从哪一步续跑。
        // Vanilla parses every non-full ProtoChunk and resumes from its status.
        let chunk_tag = parse_named(chunk_bytes)?;
        let entities_tag = match &packed.entities {
            Some(bytes) => Some(parse_named(bytes)?),
            None => None,
        };
        let decoded = chunk_content::decode(pos, &chunk_tag, entities_tag.as_ref())
            .map_err(|e| e.to_string())?;
        for problem in &decoded.problems {
            warn!(at = %problem.at, message = %problem.message, "chunk decode problem");
        }
        Ok(DecodedColumn {
            column: decoded.column,
            persisted_status: decoded.persisted_status,
        })
    }
}

fn named_bytes(tag: &Tag) -> Vec<u8> {
    let mut out = Vec::new();
    write_named("", tag, &mut out);
    out
}

fn parse_named(bytes: &[u8]) -> Result<Tag, String> {
    let (name, entries) =
        read_named(bytes, Accounter::uncompressed_quota()).map_err(|e| e.to_string())?;
    if !name.is_empty() {
        return Err(format!("chunk NBT root name is {name:?}, expected empty"));
    }
    Ok(Tag::Compound(entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_column_round_trips() {
        let codec = InlineCodec;
        let pos = ChunkPos::new(0, 0);
        let column = RegionColumn::from_template();
        let packed = codec.encode(pos, &column).unwrap();
        assert!(packed.chunk.is_some());
        assert!(packed.entities.is_none());
        let wire = packed.pack();
        let decoded = PackedChunk::unpack(&wire).unwrap();
        assert_eq!(decoded, packed);
        let back = codec.decode(pos, &decoded).unwrap();
        // 从盘组装的盒子 `untouched` 为 false，所以比方块层内容而不是 pristine。
        // A box rebuilt from disk is not "untouched", so compare block content.
        assert_eq!(back.persisted_status, ChunkStatus::Full);
        assert_eq!(
            column.column().chunk().block(0, -60, 0),
            back.column.column().chunk().block(0, -60, 0)
        );
    }

    #[test]
    fn a_bad_container_is_rejected_without_panicking() {
        assert!(PackedChunk::unpack(&[]).is_err());
        assert!(PackedChunk::unpack(&[9, 0, 0, 0, 0]).is_err());
        assert!(PackedChunk::unpack(&[MAGIC, 0, 0, 0, 2, b'x']).is_err());
        assert!(PackedChunk::unpack(&[MAGIC, 0, 0, 0, 0, 0, 0, 0, 0, 7]).is_err());
    }

    /// 官方 `SerializableChunkData.parse` 把非 full 的 Status 读成 ProtoChunk，
    /// 从原阶段接着跑到 Full。读盘必须把阶段交给 Director，也不能把没写出的
    /// 方块层换成超平坦模板；下一次写盘仍写盘上的真实阶段。
    ///
    /// Vanilla parses a non-full Status into a ProtoChunk and resumes from it.
    /// Decoding must hand the stage to the Director and keep the on-disk
    /// sections; the next save writes the real (not full) status again.
    #[test]
    fn a_non_full_chunk_keeps_its_status_and_blocks() {
        let pos = ChunkPos::new(0, 0);
        let mut column = RegionColumn::from_template();
        // 模拟一个还没跑 terrain 的 ProtoChunk：状态是 structure_starts。
        // Model a ProtoChunk still before terrain: status structure_starts.
        let mut archive = column.chunk_column().chunk().archive().clone();
        archive.status = ChunkStatus::StructureStarts;
        column.column_mut().chunk_mut().set_archive(archive);
        let packed = InlineCodec.encode(pos, &column).unwrap();
        let (_, entries) = read_named(
            packed.chunk.as_deref().unwrap(),
            Accounter::uncompressed_quota(),
        )
        .unwrap();
        assert_eq!(
            entries
                .iter()
                .find(|(key, _)| key == "Status")
                .map(|(_, value)| value.clone()),
            Some(Tag::String("minecraft:structure_starts".to_owned())),
            "半成品区块写盘要写真实 Status"
        );

        let decoded = InlineCodec
            .decode(pos, &packed)
            .expect("非 full 的 ProtoChunk 要能读回并继续生成");
        assert_eq!(decoded.persisted_status, ChunkStatus::StructureStarts);
        assert_eq!(
            decoded.column.column().chunk().block(3, -61, 5),
            column.column().chunk().block(3, -61, 5),
            "读回时不能把盘上的方块换成模板"
        );
        // 读回后再写盘，Status 仍是原阶段；只有走完流水线才写 full。
        let written = InlineCodec.encode(pos, &decoded.column).unwrap();
        let (_, written_entries) = read_named(
            written.chunk.as_deref().unwrap(),
            Accounter::uncompressed_quota(),
        )
        .unwrap();
        assert_eq!(
            written_entries
                .iter()
                .find(|(key, _)| key == "Status")
                .map(|(_, value)| value.clone()),
            Some(Tag::String("minecraft:structure_starts".to_owned()))
        );
        assert!(
            written_entries.iter().any(|(key, _)| key == "sections"),
            "半成品区块仍要写回 sections"
        );
        // Bamboo 写出 → Bamboo 读回：盘上的阶段和方块层都不丢，下一次写盘
        // 仍写同一阶段（官方 `ChunkMap.save` 对 ProtoChunk 的行为）。
        let again = InlineCodec.decode(pos, &written).unwrap();
        assert_eq!(again.persisted_status, ChunkStatus::StructureStarts);
        assert_eq!(
            again.column.column().chunk().block(3, -61, 5),
            column.column().chunk().block(3, -61, 5),
        );
        let again_written = InlineCodec.encode(pos, &again.column).unwrap();
        let (_, again_entries) = read_named(
            again_written.chunk.as_deref().unwrap(),
            Accounter::uncompressed_quota(),
        )
        .unwrap();
        assert_eq!(
            again_entries
                .iter()
                .find(|(key, _)| key == "Status")
                .map(|(_, value)| value.clone()),
            Some(Tag::String("minecraft:structure_starts".to_owned()))
        );
    }
}
