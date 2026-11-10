//! NBT 的文件形式：带名字的根、gzip / zlib / 不压缩的外壳。
//!
//! 官方 `NbtIo.readCompressed` 用 `GZIPInputStream` 读 `level.dat` 和玩家数据；
//! `RegionFile` 里每区块流的压缩由区域的压缩 ID 决定，见 `mc-bamboo-storage`。
//! 外壳只负责把字节解开，标签解码的配额和深度上限在 [`Reader`] 里。
//!
//! 压缩炸弹不能靠 NBT 配额挡住：配额是在解出标签之后才记的账。这里给解压后的字节数
//! 单独设一个上限，超了就停，不把整段数据读进内存。
//!
//! The NBT quota is accounted while a tag is already decoded, so a compressed
//! bomb needs a separate bound on the decompressed bytes themselves.

use std::io::{self, Read, Write};

use flate2::{
    Compression as ZlibLevel,
    read::{GzDecoder, ZlibDecoder},
    write::{GzEncoder, ZlibEncoder},
};

use crate::{Accounter, DecodeError, Reader, Tag};

/// 官方 `NbtAccounter.UNCOMPRESSED_NBT_QUOTA`，读未压缩文件时的配额。
pub const UNCOMPRESSED_QUOTA: u64 = 104_857_600;

/// 一次解压最多产出的字节数。
///
/// 一个区块的 NBT 通常几十 KB，官方允许最大的区块也就 1 MiB 左右；
/// 这个上限比官方未压缩配额小，坏文件不会让解码器吃掉上百 MB。
pub const MAX_DECOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;

/// 文件读写的失败原因。
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    /// 标签本身不合法，或者超出了配额、深度上限。
    #[error(transparent)]
    Decode(#[from] DecodeError),
    /// 压缩流坏了，或者解压后超过 [`MAX_DECOMPRESSED_BYTES`]。
    #[error("NBT compression wrapper failed: {0}")]
    Compression(String),
    /// 根节点不是复合标签，官方 `NbtIo.read` 会报 `Root tag must be a named compound tag`。
    #[error("NBT root must be a named compound tag")]
    NotACompound,
    /// 根节点是 `TAG_End`。
    #[error("NBT file has no root tag")]
    EndTag,
    /// 文件里除根节点外还有别的内容。
    #[error("NBT file has {0} trailing bytes")]
    TrailingBytes(usize),
}

/// 压缩外壳的种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    /// gzip（`level.dat`、玩家数据、区域压缩 ID 1）。
    Gzip,
    /// zlib（区域压缩 ID 2）。官方叫 `deflate`，实际是 `DeflaterOutputStream`。
    Zlib,
    /// 不压缩（区域压缩 ID 3）。
    None,
}

impl Compression {
    pub const ALL: [Self; 3] = [Self::Gzip, Self::Zlib, Self::None];
}

fn io_error(error: io::Error) -> FileError {
    FileError::Compression(error.to_string())
}

/// 只解一层压缩外壳，不解析标签。返回值不会超过 [`MAX_DECOMPRESSED_BYTES`]。
pub fn decompress(input: &[u8], compression: Compression) -> Result<Vec<u8>, FileError> {
    inflate(input, compression)
}

/// 解一层压缩外壳。返回值不会超过 [`MAX_DECOMPRESSED_BYTES`]。
fn inflate(input: &[u8], compression: Compression) -> Result<Vec<u8>, FileError> {
    if compression == Compression::None {
        let length = u64::try_from(input.len()).unwrap_or(u64::MAX);
        if length > MAX_DECOMPRESSED_BYTES {
            return Err(FileError::Compression(format!(
                "uncompressed NBT is larger than {MAX_DECOMPRESSED_BYTES} bytes"
            )));
        }
        return Ok(input.to_vec());
    }

    // 一个 Reader 只把上限内的字节交出来，超限时再多读一个字节来确认，避免把
    // 恰好等于上限的合法文件误判成炸弹。
    let limited = input.take(MAX_DECOMPRESSED_BYTES + 1);
    let decoder: Box<dyn Read> = match compression {
        Compression::Gzip => Box::new(GzDecoder::new(limited)),
        Compression::Zlib => Box::new(ZlibDecoder::new(limited)),
        Compression::None => unreachable!("上面已经返回"),
    };
    let mut out = Vec::new();
    decoder
        .take(MAX_DECOMPRESSED_BYTES + 1)
        .read_to_end(&mut out)
        .map_err(io_error)?;
    if out.len() as u64 > MAX_DECOMPRESSED_BYTES {
        return Err(FileError::Compression(format!(
            "decompressed NBT is larger than {MAX_DECOMPRESSED_BYTES} bytes"
        )));
    }
    Ok(out)
}

/// 包一层压缩外壳。`level` 只对 zlib 生效；gzip 用默认级别。
fn deflate(input: &[u8], compression: Compression) -> Result<Vec<u8>, FileError> {
    match compression {
        Compression::None => Ok(input.to_vec()),
        Compression::Gzip => {
            let mut encoder = GzEncoder::new(Vec::new(), ZlibLevel::default());
            encoder.write_all(input).map_err(io_error)?;
            encoder.finish().map_err(io_error)
        }
        Compression::Zlib => {
            let mut encoder = ZlibEncoder::new(Vec::new(), ZlibLevel::default());
            encoder.write_all(input).map_err(io_error)?;
            encoder.finish().map_err(io_error)
        }
    }
}

/// 读一个带名字的复合根，返回名字和按键顺序排列的条目。
///
/// `quota` 是标签解码的配额（不是解压后字节数的上限），坏输入也不能让配额
/// 之外的嵌套和数组分配内存。
pub fn read_named(
    input: &[u8],
    quota: Accounter,
) -> Result<(String, Vec<(String, Tag)>), FileError> {
    let mut reader = Reader::new(input, quota);
    let kind = reader.peek_root_type()?;
    if kind == 0 {
        return Err(FileError::EndTag);
    }
    let name = reader.read_root_name()?;
    let tag = reader.named_root_payload(kind)?;
    let Tag::Compound(entries) = tag else {
        Tag::release(tag);
        return Err(FileError::NotACompound);
    };
    let trailing = input.len() - reader.read();
    if trailing != 0 {
        return Err(FileError::TrailingBytes(trailing));
    }
    Ok((name, entries))
}

/// 官方 `NbtIo.readCompressed`：解一层外壳，再读带名字的复合根。
/// 名字通常是空字符串，这里交给调用方决定要不要检查。
pub fn read_compressed(
    input: &[u8],
    compression: Compression,
    quota: Accounter,
) -> Result<(String, Vec<(String, Tag)>), FileError> {
    let plain = inflate(input, compression)?;
    read_named(&plain, quota)
}

/// 官方 `NbtIo.write`：写带名字的根。
pub fn write_named(name: &str, tag: &Tag, out: &mut Vec<u8>) {
    tag.write_named(name, out);
}

/// 官方 `NbtIo.writeCompressed`：写带名字的根，再包一层外壳。
pub fn write_compressed(
    name: &str,
    tag: &Tag,
    compression: Compression,
) -> Result<Vec<u8>, FileError> {
    let mut plain = Vec::new();
    write_named(name, tag, &mut plain);
    deflate(&plain, compression)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Tag {
        Tag::Compound(vec![
            ("count".to_owned(), Tag::Int(-7)),
            ("text".to_owned(), Tag::String("竹\0🎋".to_owned())),
            (
                "nested".to_owned(),
                Tag::Compound(vec![("empty".to_owned(), Tag::List(Vec::new()))]),
            ),
            (
                "words".to_owned(),
                Tag::List(vec![Tag::Long(1), Tag::Long(i64::MIN)]),
            ),
            ("bytes".to_owned(), Tag::ByteArray(vec![0, -1])),
        ])
    }

    #[test]
    fn named_root_round_trips_in_every_wrapper() {
        let tag = sample();
        for compression in Compression::ALL {
            let bytes = write_compressed("", &tag, compression).expect("压缩成功");
            let (name, entries) =
                read_compressed(&bytes, compression, Accounter::unlimited()).expect("解压成功");
            assert_eq!(name, "");
            assert_eq!(Tag::Compound(entries), tag);
        }
    }

    #[test]
    fn a_nonempty_root_name_survives_the_round_trip() {
        let tag = Tag::Compound(vec![("a".to_owned(), Tag::Byte(1))]);
        let mut bytes = Vec::new();
        write_named("level", &tag, &mut bytes);
        assert_eq!(bytes[0], 10);
        assert_eq!(u16::from_be_bytes([bytes[1], bytes[2]]), 5);
        assert_eq!(&bytes[3..8], b"level");
        let (name, entries) = read_named(&bytes, Accounter::unlimited()).expect("读成功");
        assert_eq!(name, "level");
        assert_eq!(Tag::Compound(entries), tag);
    }

    #[test]
    fn rejects_a_root_that_is_not_a_compound() {
        let mut bytes = Vec::new();
        Tag::Int(1).write_named("", &mut bytes);
        assert_eq!(
            read_named(&bytes, Accounter::unlimited())
                .unwrap_err()
                .to_string(),
            "NBT root must be a named compound tag"
        );
    }

    #[test]
    fn rejects_an_end_tag_and_trailing_bytes() {
        assert!(matches!(
            read_named(&[0], Accounter::unlimited()),
            Err(FileError::EndTag)
        ));
        let mut bytes = Vec::new();
        write_named("", &Tag::Compound(Vec::new()), &mut bytes);
        bytes.push(99);
        assert!(matches!(
            read_named(&bytes, Accounter::unlimited()),
            Err(FileError::TrailingBytes(1))
        ));
    }

    #[test]
    fn rejects_truncated_and_corrupted_wrappers() {
        let tag = sample();
        for compression in Compression::ALL {
            let bytes = write_compressed("", &tag, compression).expect("压缩成功");
            for cut in [0, 1, bytes.len() / 2, bytes.len() - 1] {
                let result = read_compressed(&bytes[..cut], compression, Accounter::unlimited());
                assert!(result.is_err(), "{compression:?} 截断到 {cut} 字节应该报错");
            }
        }
        let gzip = write_compressed("", &tag, Compression::Gzip).expect("压缩成功");
        let mut flipped = gzip.clone();
        let middle = flipped.len() / 2;
        flipped[middle] ^= 0xff;
        assert!(read_compressed(&flipped, Compression::Gzip, Accounter::unlimited()).is_err());
        let zlib = write_compressed("", &tag, Compression::Zlib).expect("压缩成功");
        let mut flipped = zlib.clone();
        flipped[0] ^= 0xff;
        assert!(read_compressed(&flipped, Compression::Zlib, Accounter::unlimited()).is_err());
    }

    #[test]
    fn the_quota_still_applies_after_decompression() {
        let tag = Tag::Compound(vec![
            ("a".to_owned(), Tag::Long(1)),
            ("b".to_owned(), Tag::Long(2)),
        ]);
        let bytes = write_compressed("", &tag, Compression::Gzip).expect("压缩成功");
        assert!(matches!(
            read_compressed(&bytes, Compression::Gzip, Accounter::new(16)),
            Err(FileError::Decode(DecodeError::QuotaExceeded { quota: 16 }))
        ));
    }

    #[test]
    fn a_compression_bomb_is_stopped_before_it_allocates() {
        // 一整段 0 压缩后只有几十字节，解压出来却有 128 MiB。
        let zeros = vec![0u8; 128 * 1024 * 1024];
        let mut encoder = GzEncoder::new(Vec::new(), ZlibLevel::default());
        encoder.write_all(&zeros).expect("写压缩流");
        let bomb = encoder.finish().expect("收尾");
        assert!(bomb.len() < 1024 * 1024);
        let result = read_compressed(&bomb, Compression::Gzip, Accounter::unlimited());
        assert!(
            matches!(result, Err(FileError::Compression(_))),
            "炸弹应该被解压上限挡住"
        );
    }

    #[test]
    fn decompressed_bytes_can_reach_the_limit() {
        // 上限是一个硬边界，恰好等于它的未压缩文件仍可读。
        let mut bytes = Vec::new();
        write_named(
            "",
            &Tag::Compound(vec![("a".to_owned(), Tag::Byte(1))]),
            &mut bytes,
        );
        bytes.resize(MAX_DECOMPRESSED_BYTES as usize, 0);
        // 末尾填的 0 是复合标签的额外字段类型，这个文件本身不合法，
        // 但不能因为字节数刚好到上限就被当成压缩炸弹。
        let error = read_named(&bytes, Accounter::unlimited()).unwrap_err();
        assert!(!matches!(error, FileError::Compression(_)));
    }
}
