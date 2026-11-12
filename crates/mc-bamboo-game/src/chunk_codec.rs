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
use tracing::warn;

use crate::region::{RegionColumn, chunk_content};

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

/// 解析/序列化接口。以后可以换成把工作丢进计算池的实现。
pub trait ChunkCodec: Send {
    /// 把一个已经从 region 收回的盒子编码成两份 NBT 字节。
    fn encode(&self, pos: ChunkPos, column: &RegionColumn) -> Result<PackedChunk, String>;
    /// 把两份 NBT 字节解码成可以交付给 region 的盒子。
    fn decode(&self, pos: ChunkPos, packed: &PackedChunk) -> Result<Box<RegionColumn>, String>;
}

/// 内联执行器：直接在调用它的线程上跑完编码/解码。
pub struct InlineCodec;

impl ChunkCodec for InlineCodec {
    fn encode(&self, pos: ChunkPos, column: &RegionColumn) -> Result<PackedChunk, String> {
        let encoded = chunk_content::encode(column, pos).map_err(|e| e.to_string())?;
        Ok(PackedChunk {
            chunk: Some(named_bytes(&encoded.chunk)),
            entities: encoded.entities.as_ref().map(named_bytes),
        })
    }

    fn decode(&self, pos: ChunkPos, packed: &PackedChunk) -> Result<Box<RegionColumn>, String> {
        let chunk_bytes = packed
            .chunk
            .as_deref()
            .ok_or_else(|| "missing chunk NBT".to_owned())?;
        // `chunk_content::decode` → `chunk_nbt::decode` 会检查 DataVersion 和
        // Status = minecraft:full；不是 full 或读坏都走 BadChunk（warn + 模板）。
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
        Ok(decoded.column)
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
        assert_eq!(
            column.column().chunk().block(0, -60, 0),
            back.column().chunk().block(0, -60, 0)
        );
    }

    #[test]
    fn a_bad_container_is_rejected_without_panicking() {
        assert!(PackedChunk::unpack(&[]).is_err());
        assert!(PackedChunk::unpack(&[9, 0, 0, 0, 0]).is_err());
        assert!(PackedChunk::unpack(&[MAGIC, 0, 0, 0, 2, b'x']).is_err());
        assert!(PackedChunk::unpack(&[MAGIC, 0, 0, 0, 0, 0, 0, 0, 0, 7]).is_err());
    }

    #[test]
    fn a_non_full_chunk_is_rejected() {
        // 手工造一个 Status 不是 full 的区块 NBT，decode 必须报错而不是交付。
        let root = Tag::Compound(vec![
            (
                "DataVersion".to_owned(),
                Tag::Int(mc_bamboo_data::version::DATA_VERSION),
            ),
            (
                "Status".to_owned(),
                Tag::String("minecraft:empty".to_owned()),
            ),
        ]);
        let bytes = named_bytes(&root);
        let packed = PackedChunk {
            chunk: Some(bytes),
            entities: None,
        };
        assert!(InlineCodec.decode(ChunkPos::new(0, 0), &packed).is_err());
    }
}
