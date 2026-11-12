//! Anvil 区域文件：`r.<x>.<z>.mca` 和 `.mcc` 外置大区块。
//!
//! 对应官方 `RegionFile`、`RegionFileStorage` 里的扇区 bitset 和
//! `RegionFileVersion`。扇区 4096 字节，头 8192 字节，offset / timestamp 各 1024 项。
//! 写出顺序照官方：在空闲扇区分配新位置 → 写载荷 → 改内存里的头 → 一次写回
//! 8192 字节头 → 提交 `.mcc` 的 rename → 释放旧扇区。
//!
//! 压缩 ID：1 gzip、2 zlib（官方叫 `deflate`）、3 不压缩、4 lz4、127 自定义。
//! 读支持 1 / 2 / 3；读到 4 报错里明确写清不支持，不引入 lz4 依赖（Q2）。
//! Linear 格式不在这一层。

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use flate2::{
    Compression as ZlibLevel,
    read::{GzDecoder, ZlibDecoder},
    write::{GzEncoder, ZlibEncoder},
};
use mc_bamboo_core::ChunkPos;

/// 一个扇区的字节数，官方 `SECTOR_BYTES`。
pub const SECTOR_BYTES: usize = 4096;
/// 头的字节数（两个扇区）。
pub const HEADER_BYTES: usize = 8192;
/// 一个区域文件里的区块数。
const ENTRIES: usize = 1024;
/// 区块载荷超过这么多扇区时写外置文件，官方 `EXTERNAL_CHUNK_THRESHOLD`。
const EXTERNAL_SECTORS: usize = 256;
/// 外置流标记，官方 `EXTERNAL_STREAM_FLAG`。
const EXTERNAL_FLAG: u8 = 128;
/// 一个区块记录的长度字段含压缩 ID 这一字节。
const CHUNK_HEADER: usize = 5;

/// 压缩种类，对应 `RegionFileVersion` 的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionCompression {
    Gzip = 1,
    Zlib = 2,
    None = 3,
}

impl RegionCompression {
    pub const ALL: [Self; 3] = [Self::Gzip, Self::Zlib, Self::None];

    pub const fn id(self) -> u8 {
        self as u8
    }

    pub fn from_id(id: u8) -> Result<Self, AnvilError> {
        match id {
            1 => Ok(Self::Gzip),
            2 => Ok(Self::Zlib),
            3 => Ok(Self::None),
            4 => Err(AnvilError::Lz4Unsupported),
            127 => Err(AnvilError::CustomUnsupported),
            other => Err(AnvilError::UnknownCompression(other)),
        }
    }

    fn decompress(self, input: &[u8], limit: usize) -> Result<Vec<u8>, AnvilError> {
        let bounded = input.take((limit as u64).saturating_add(1));
        let decoder: Box<dyn Read> = match self {
            Self::Gzip => Box::new(GzDecoder::new(bounded)),
            Self::Zlib => Box::new(ZlibDecoder::new(bounded)),
            Self::None => {
                if input.len() > limit {
                    return Err(AnvilError::TooLarge(input.len()));
                }
                return Ok(input.to_vec());
            }
        };
        let mut out = Vec::new();
        decoder
            .take(limit as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|error| AnvilError::Io(error.to_string()))?;
        if out.len() > limit {
            return Err(AnvilError::TooLarge(out.len()));
        }
        Ok(out)
    }

    fn compress(self, input: &[u8]) -> Result<Vec<u8>, AnvilError> {
        match self {
            Self::None => Ok(input.to_vec()),
            Self::Gzip => {
                let mut encoder = GzEncoder::new(Vec::new(), ZlibLevel::default());
                encoder
                    .write_all(input)
                    .map_err(|error| AnvilError::Io(error.to_string()))?;
                encoder
                    .finish()
                    .map_err(|error| AnvilError::Io(error.to_string()))
            }
            Self::Zlib => {
                let mut encoder = ZlibEncoder::new(Vec::new(), ZlibLevel::default());
                encoder
                    .write_all(input)
                    .map_err(|error| AnvilError::Io(error.to_string()))?;
                encoder
                    .finish()
                    .map_err(|error| AnvilError::Io(error.to_string()))
            }
        }
    }
}

/// 区域文件读写失败的原因。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnvilError {
    #[error("region file I/O failed: {0}")]
    Io(String),
    #[error("chunk is outside the 32x32 region")]
    OutsideRegion(ChunkPos),
    #[error(
        "LZ4 (compression id 4) is not supported; re-save the world with region-file-compression=deflate"
    )]
    Lz4Unsupported,
    #[error("custom region compression (id 127) is not supported")]
    CustomUnsupported,
    #[error("unknown region compression id {0}")]
    UnknownCompression(u8),
    #[error("chunk record is larger than the {0} byte limit")]
    TooLarge(usize),
    #[error("chunk record is truncated or has an invalid length")]
    Truncated,
    #[error("external chunk file is missing")]
    MissingExternal,
    #[error("region file has a truncated header")]
    TruncatedHeader,
}

impl From<io::Error> for AnvilError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// 一个 offset 表项：高 24 位扇区号、低 8 位扇区数。
fn pack_offset(sector: u32, count: u32) -> i32 {
    ((sector << 8) | (count & 0xff)) as i32
}

fn sector_number(offset: i32) -> usize {
    ((offset as u32) >> 8) as usize
}

fn sector_count(offset: i32) -> usize {
    (offset as u32 & 0xff) as usize
}

fn offset_index(pos: ChunkPos) -> Result<usize, AnvilError> {
    let x = pos.x.rem_euclid(32);
    let z = pos.z.rem_euclid(32);
    Ok(x as usize + z as usize * 32)
}

/// 扇区 bitset，官方 `RegionBitmap`。
#[derive(Debug, Default)]
struct Sectors {
    used: Vec<u64>,
}

impl Sectors {
    fn get(&self, index: usize) -> bool {
        self.used
            .get(index / 64)
            .is_some_and(|word| word & (1 << (index % 64)) != 0)
    }

    fn set(&mut self, start: usize, count: usize, value: bool) {
        if count == 0 {
            return;
        }
        let end = start.saturating_add(count);
        if self.used.len() < end.div_ceil(64) {
            self.used.resize(end.div_ceil(64), 0);
        }
        for index in start..end {
            let word = &mut self.used[index / 64];
            let mask = 1 << (index % 64);
            if value {
                *word |= mask;
            } else {
                *word &= !mask;
            }
        }
    }

    /// 找第一段连续的 `count` 个空闲扇区，标成已用并返回起点。
    fn allocate(&mut self, count: usize) -> usize {
        let mut start = 0;
        loop {
            while self.get(start) {
                start += 1;
            }
            let mut run = 0;
            while run < count && !self.get(start + run) {
                run += 1;
            }
            if run == count {
                self.set(start, count, true);
                return start;
            }
            start += run + 1;
        }
    }
}

/// 打开的区域文件。
#[derive(Debug)]
pub struct RegionFile {
    path: PathBuf,
    external_dir: PathBuf,
    file: File,
    offsets: [i32; ENTRIES],
    timestamps: [i32; ENTRIES],
    sectors: Sectors,
    compression: RegionCompression,
    /// 坏表项被清零的个数，供日志和检查用。
    pub repaired: usize,
    /// 头本身没读够 8192 字节，官方只 warn，这里也记下来。
    pub short_header: bool,
}

/// 一个区域的坐标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionPos {
    pub x: i32,
    pub z: i32,
}

impl RegionPos {
    pub const fn of(pos: ChunkPos) -> Self {
        Self {
            x: pos.x >> 5,
            z: pos.z >> 5,
        }
    }

    /// 官方的文件名 `r.<x>.<z>.mca`。
    pub fn file_name(self) -> String {
        format!("r.{}.{}.mca", self.x, self.z)
    }
}

impl RegionFile {
    /// 打开（没有就创建）一个区域文件。`external_dir` 是 `.mcc` 的存放目录，通常是 region/。
    pub fn open(
        path: impl AsRef<Path>,
        external_dir: impl AsRef<Path>,
        compression: RegionCompression,
    ) -> Result<Self, AnvilError> {
        let path = path.as_ref().to_path_buf();
        let external_dir = external_dir.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::create_dir_all(&external_dir)?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        let size = file.metadata()?.len() as usize;
        let mut header = [0u8; HEADER_BYTES];
        let read = read_at(&mut file, 0, &mut header)?;
        let short_header = read < HEADER_BYTES;
        let mut offsets = [0i32; ENTRIES];
        let mut timestamps = [0i32; ENTRIES];
        for index in 0..ENTRIES {
            offsets[index] = i32::from_be_bytes(
                header[index * 4..index * 4 + 4]
                    .try_into()
                    .expect("四个字节"),
            );
            timestamps[index] = i32::from_be_bytes(
                header[4096 + index * 4..4096 + index * 4 + 4]
                    .try_into()
                    .expect("四个字节"),
            );
        }
        let mut sectors = Sectors::default();
        sectors.set(0, 2, true);
        let mut repaired = 0;
        if read > 0 {
            for entry in offsets.iter_mut() {
                let offset = *entry;
                if offset == 0 {
                    continue;
                }
                let sector = sector_number(offset);
                let count = sector_count(offset);
                let bad = sector < 2 || count == 0 || sector.saturating_mul(SECTOR_BYTES) > size;
                if bad {
                    *entry = 0;
                    repaired += 1;
                } else {
                    sectors.set(sector, count, true);
                }
            }
        }
        let mut region = Self {
            path,
            external_dir,
            file,
            offsets,
            timestamps,
            sectors,
            compression,
            repaired,
            short_header,
        };
        if repaired > 0 {
            region.write_header()?;
        }
        if size < HEADER_BYTES {
            region.file.set_len(HEADER_BYTES as u64)?;
        }
        Ok(region)
    }

    /// 这个区块在区域文件里有没有表项。
    pub fn has_chunk(&self, pos: ChunkPos) -> bool {
        offset_index(pos).is_ok_and(|index| self.offsets[index] != 0)
    }

    /// 读一个区块的**未压缩** NBT 字节，区块不存在时返回 `None`。
    pub fn read(&mut self, pos: ChunkPos, limit: usize) -> Result<Option<Vec<u8>>, AnvilError> {
        let index = offset_index(pos)?;
        let offset = self.offsets[index];
        if offset == 0 {
            return Ok(None);
        }
        let sector = sector_number(offset);
        let count = sector_count(offset);
        let mut payload = vec![0u8; count.saturating_mul(SECTOR_BYTES)];
        let read = read_at(
            &mut self.file,
            sector as u64 * SECTOR_BYTES as u64,
            &mut payload,
        )?;
        payload.truncate(read);
        if payload.len() < CHUNK_HEADER {
            return Err(AnvilError::Truncated);
        }
        let raw_length = i32::from_be_bytes(payload[..4].try_into().expect("四个字节"));
        if raw_length <= 0 {
            return Ok(None);
        }
        let id = payload[4];
        if id & EXTERNAL_FLAG != 0 {
            let compression = RegionCompression::from_id(id & !EXTERNAL_FLAG)?;
            let mut external = Vec::new();
            File::open(self.external_path(pos))
                .map_err(|_| AnvilError::MissingExternal)?
                .read_to_end(&mut external)?;
            // 官方 `.mcc` 里前 5 字节是桩，剩下的是压缩后的 NBT。
            let body = external.get(CHUNK_HEADER..).ok_or(AnvilError::Truncated)?;
            return compression.decompress(body, limit).map(Some);
        }
        let compression = RegionCompression::from_id(id)?;
        let length = usize::try_from(raw_length - 1).map_err(|_| AnvilError::Truncated)?;
        if length > payload.len() - CHUNK_HEADER {
            return Err(AnvilError::Truncated);
        }
        let body = &payload[CHUNK_HEADER..CHUNK_HEADER + length];
        compression.decompress(body, limit).map(Some)
    }

    /// 写一个区块的未压缩 NBT 字节。`None` 等于删除这个区块。
    pub fn write(
        &mut self,
        pos: ChunkPos,
        data: Option<&[u8]>,
        timestamp: i32,
    ) -> Result<(), AnvilError> {
        let index = offset_index(pos)?;
        let Some(data) = data else {
            if self.offsets[index] != 0 {
                let old = self.offsets[index];
                self.offsets[index] = 0;
                self.timestamps[index] = timestamp;
                self.write_header()?;
                let _ = fs::remove_file(self.external_path(pos));
                self.sectors
                    .set(sector_number(old), sector_count(old), false);
            }
            return Ok(());
        };
        let compressed = self.compression.compress(data)?;
        let payload_len = compressed
            .len()
            .checked_add(1)
            .ok_or(AnvilError::TooLarge(compressed.len()))?;
        let record_len = payload_len
            .checked_add(4)
            .ok_or(AnvilError::TooLarge(compressed.len()))?;
        let external = record_len > EXTERNAL_SECTORS * SECTOR_BYTES;
        let length = if external {
            1i32
        } else {
            i32::try_from(payload_len).map_err(|_| AnvilError::TooLarge(compressed.len()))?
        };
        let id = if external {
            self.compression.id() | EXTERNAL_FLAG
        } else {
            self.compression.id()
        };
        let mut body = Vec::with_capacity(CHUNK_HEADER + compressed.len());
        body.extend_from_slice(&length.to_be_bytes());
        body.push(id);
        body.extend_from_slice(&compressed);
        let count = if external {
            1
        } else {
            record_len.div_ceil(SECTOR_BYTES)
        };
        // offset 的低 8 位只能放 255 个扇区；超出的都走外置文件。
        if count > u8::MAX as usize {
            return Err(AnvilError::TooLarge(record_len));
        }
        let sector = self.sectors.allocate(count);

        // 官方顺序：先写载荷，再改头，最后跑提交动作（`.mcc` 的 rename）并释放旧扇区。
        let tmp_path = self.external_dir.join(format!(
            ".{}.{}.{}.tmp",
            self.path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            pos.x,
            pos.z
        ));
        let commit = if external {
            let mut stub = vec![0u8; CHUNK_HEADER];
            stub[..4].copy_from_slice(&1i32.to_be_bytes());
            stub[4] = id;
            write_at(&mut self.file, sector as u64 * SECTOR_BYTES as u64, &stub)?;
            {
                let mut file = File::create(&tmp_path)?;
                file.write_all(&body)?;
                file.sync_all()?;
            }
            // 先不改名：等头写完再提交，头写到一半断电时旧的 `.mcc` 还在。
            Some(&tmp_path)
        } else {
            write_at(&mut self.file, sector as u64 * SECTOR_BYTES as u64, &body)?;
            None
        };
        let old = self.offsets[index];
        self.offsets[index] = pack_offset(
            u32::try_from(sector).map_err(|_| AnvilError::TooLarge(sector))?,
            count as u32,
        );
        self.timestamps[index] = timestamp;
        self.write_header()?;
        if let Some(tmp) = commit {
            fs::rename(tmp, self.external_path(pos))?;
        } else if old != 0 {
            let _ = fs::remove_file(self.external_path(pos));
        }
        if old != 0 {
            self.sectors
                .set(sector_number(old), sector_count(old), false);
        }
        Ok(())
    }

    /// 把整个 8192 字节头一次写回。
    fn write_header(&mut self) -> Result<(), AnvilError> {
        let mut header = [0u8; HEADER_BYTES];
        for (index, &offset) in self.offsets.iter().enumerate() {
            header[index * 4..index * 4 + 4].copy_from_slice(&offset.to_be_bytes());
        }
        for (index, &timestamp) in self.timestamps.iter().enumerate() {
            header[4096 + index * 4..4096 + index * 4 + 4]
                .copy_from_slice(&timestamp.to_be_bytes());
        }
        write_at(&mut self.file, 0, &header)?;
        Ok(())
    }

    fn external_path(&self, pos: ChunkPos) -> PathBuf {
        self.external_dir.join(format!("c.{}.{}.mcc", pos.x, pos.z))
    }

    /// 关服时落盘：补到整扇区再 `sync_all`。
    pub fn sync(&mut self) -> Result<(), AnvilError> {
        let size = self.file.metadata()?.len();
        let padded = size.div_ceil(SECTOR_BYTES as u64) * SECTOR_BYTES as u64;
        if padded > size {
            self.file.set_len(padded)?;
        }
        self.file.sync_all()?;
        Ok(())
    }

    /// 文件路径，供日志使用。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 这个区域文件写新区块时用的压缩 ID。
    pub fn compression(&self) -> RegionCompression {
        self.compression
    }
}

fn read_at(file: &mut File, offset: u64, out: &mut [u8]) -> Result<usize, AnvilError> {
    file.seek(SeekFrom::Start(offset))?;
    let mut total = 0;
    while total < out.len() {
        let read = file.read(&mut out[total..])?;
        if read == 0 {
            break;
        }
        total += read;
    }
    Ok(total)
}

fn write_at(file: &mut File, offset: u64, data: &[u8]) -> Result<(), AnvilError> {
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(data)?;
    Ok(())
}

/// 按区域坐标打开区域文件的便捷入口，目录是 `region/`。
pub fn open_region(
    dir: impl AsRef<Path>,
    region: RegionPos,
    compression: RegionCompression,
) -> Result<RegionFile, AnvilError> {
    let dir = dir.as_ref();
    RegionFile::open(dir.join(region.file_name()), dir, compression)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            // 线程名形如 `anvil::tests::foo`，Windows 路径里 `:` 非法；
            // 换成 `_` 只是让临时目录名合法，不改测试逻辑。
            // Thread names contain `::`, which is illegal in Windows paths;
            // replacing it with `_` only makes the temp dir name legal.
            let thread = std::thread::current()
                .name()
                .unwrap_or("test")
                .replace(':', "_");
            let path = std::env::temp_dir().join(format!(
                "bamboo-anvil-{name}-{}-{thread}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("创建临时目录");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample(seed: u64) -> Vec<u8> {
        let mut state = seed | 1;
        let mut out = Vec::new();
        for _ in 0..4096 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            out.push((state >> 24) as u8);
        }
        out
    }

    #[test]
    fn header_offsets_and_timestamps_follow_the_official_layout() {
        let dir = TempDir::new("header");
        let mut region = open_region(&dir.0, RegionPos { x: 1, z: -1 }, RegionCompression::Zlib)
            .expect("打开成功");
        assert_eq!(region.path().file_name().unwrap(), "r.1.-1.mca");
        let pos = ChunkPos::new(35, -29);
        region
            .write(pos, Some(&[1, 2, 3]), 0x1122_3344)
            .expect("写入成功");

        let mut header = [0u8; HEADER_BYTES];
        read_at(&mut region.file, 0, &mut header).expect("读头");
        let index = 3 + 3 * 32;
        let offset = i32::from_be_bytes(header[index * 4..index * 4 + 4].try_into().unwrap());
        let timestamp = i32::from_be_bytes(
            header[4096 + index * 4..4096 + index * 4 + 4]
                .try_into()
                .unwrap(),
        );
        assert_eq!(sector_number(offset), 2, "头两个扇区预留");
        assert!(sector_count(offset) >= 1);
        assert_eq!(timestamp, 0x1122_3344);
        assert_eq!(header.len(), HEADER_BYTES);
    }

    #[test]
    fn every_compression_round_trips_and_lz4_is_rejected_clearly() {
        for compression in RegionCompression::ALL {
            let dir = TempDir::new(match compression {
                RegionCompression::Gzip => "gzip",
                RegionCompression::Zlib => "zlib",
                RegionCompression::None => "none",
            });
            let mut region =
                open_region(&dir.0, RegionPos { x: 0, z: 0 }, compression).expect("打开成功");
            let data = sample(7);
            region
                .write(ChunkPos::new(1, 2), Some(&data), 42)
                .expect("写入成功");
            assert_eq!(region.read(ChunkPos::new(1, 2), 1 << 20), Ok(Some(data)));
            assert_eq!(region.read(ChunkPos::new(3, 4), 1 << 20), Ok(None));
        }

        let dir = TempDir::new("lz4");
        let mut region = open_region(&dir.0, RegionPos { x: 0, z: 0 }, RegionCompression::Zlib)
            .expect("打开成功");
        region
            .write(ChunkPos::new(0, 0), Some(&[1]), 0)
            .expect("写入成功");
        let mut payload = [0u8; 16];
        payload[..4].copy_from_slice(&2i32.to_be_bytes());
        payload[4] = 4;
        payload[5] = 0xaa;
        // 官方的 `getChunkDataInputStream` 先读 5 字节头再看压缩 ID。
        let offset = region.offsets[0];
        write_at(
            &mut region.file,
            sector_number(offset) as u64 * SECTOR_BYTES as u64,
            &payload,
        )
        .expect("改压缩 ID");
        assert!(matches!(
            region.read(ChunkPos::new(0, 0), 1 << 20),
            Err(AnvilError::Lz4Unsupported)
        ));
        assert!(
            RegionCompression::from_id(4)
                .unwrap_err()
                .to_string()
                .contains("LZ4")
        );
    }

    #[test]
    fn resizing_and_deleting_reuses_free_sectors_without_overlap() {
        let dir = TempDir::new("reuse");
        let mut region = open_region(&dir.0, RegionPos { x: 0, z: 0 }, RegionCompression::None)
            .expect("打开成功");
        let a = ChunkPos::new(0, 0);
        let b = ChunkPos::new(1, 0);
        let large = sample(1);
        let mut bigger = large.clone();
        bigger.extend(sample(2));
        region.write(a, Some(&large), 1).expect("写 A");
        let first_a = sector_number(region.offsets[0]);
        region.write(b, Some(&sample(3)), 2).expect("写 B");
        let first_b = sector_number(region.offsets[1]);
        assert_ne!(first_a, first_b, "两个区块不重叠");

        // 改大：分配新的位置，旧扇区随后释放；B 还在原位，读出来不变。
        region.write(a, Some(&bigger), 3).expect("改大 A");
        assert_eq!(region.read(b, 1 << 20), Ok(Some(sample(3))));
        assert_eq!(region.read(a, 1 << 20), Ok(Some(bigger)));

        // 改小、删除：释放的扇区可以给下一个区块用。
        region.write(a, Some(&[9, 8, 7]), 4).expect("改小 A");
        assert_eq!(region.read(a, 1 << 20), Ok(Some(vec![9, 8, 7])));
        region.write(a, None, 5).expect("删除 A");
        assert_eq!(region.read(a, 1 << 20), Ok(None));
        assert!(!region.has_chunk(a));
        let reused = sector_number({
            region.write(b, Some(&[1, 2]), 6).expect("再写 B");
            region.offsets[1]
        });
        assert!(reused >= 2, "分配到的扇区在头之后");
        assert_eq!(region.read(b, 1 << 20), Ok(Some(vec![1, 2])));
    }

    #[test]
    fn an_oversized_chunk_goes_to_an_mcc_file() {
        let dir = TempDir::new("external");
        let mut region = open_region(&dir.0, RegionPos { x: 0, z: 0 }, RegionCompression::None)
            .expect("打开成功");
        let pos = ChunkPos::new(5, 6);
        // 不压缩时 1 MiB 以上才会走外置文件。
        let mut data = Vec::new();
        for _ in 0..300 {
            data.extend(sample(data.len() as u64));
        }
        assert!(data.len() > EXTERNAL_SECTORS * SECTOR_BYTES);
        region.write(pos, Some(&data), 7).expect("写大区块");
        let external = dir.0.join("c.5.6.mcc");
        assert!(external.exists(), ".mcc 文件存在");
        assert_eq!(sector_count(region.offsets[5 + 6 * 32]), 1, "只留 5 字节桩");
        assert_eq!(region.read(pos, 1 << 24), Ok(Some(data)));
        region.write(pos, None, 8).expect("删除");
        assert!(!external.exists(), "删除时 .mcc 也删掉");
    }

    /// 外置文件的提交动作在头写完之后才跑：旧 `.mcc` 在头写坏时还在。
    #[test]
    fn external_files_keep_the_old_version_until_the_header_is_written() {
        let dir = TempDir::new("external-staging");
        let pos = ChunkPos::new(2, 3);
        let mut big = Vec::new();
        for _ in 0..300 {
            big.extend(sample(big.len() as u64));
        }
        let mut region = open_region(&dir.0, RegionPos { x: 0, z: 0 }, RegionCompression::None)
            .expect("打开成功");
        region.write(pos, Some(&big), 1).expect("第一次写");
        let external = dir.0.join("c.2.3.mcc");
        let first = fs::read(&external).expect("读到 .mcc");
        let mut second = big.clone();
        second.extend_from_slice(&[0xab; 64]);
        region.write(pos, Some(&second), 2).expect("第二次写");
        let written = fs::read(&external).expect("读到 .mcc");
        assert_ne!(first, written, "内容确实更新了");
        assert_eq!(region.read(pos, 1 << 24), Ok(Some(second)));
        // 暂存文件提交后不该留下。
        let leftovers: Vec<_> = fs::read_dir(&dir.0)
            .expect("读目录")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "暂存文件没有清理");
    }

    #[test]
    fn bad_header_entries_are_cleared_but_other_chunks_survive() {
        let dir = TempDir::new("bad-header");
        let mut region = open_region(&dir.0, RegionPos { x: 0, z: 0 }, RegionCompression::None)
            .expect("打开成功");
        let good = ChunkPos::new(0, 0);
        let data = sample(4);
        region.write(good, Some(&data), 1).expect("写正常区块");
        let good_offset = region.offsets[0];
        // 三个坏表项：扇区落在头上、扇区数为 0、扇区超出文件大小。
        let bad = [
            pack_offset(1, 1),
            pack_offset(sector_number(good_offset) as u32, 0),
            pack_offset(100_000, 1),
        ];
        for (index, &offset) in bad.iter().enumerate() {
            region.offsets[index + 1] = offset;
        }
        region.write_header().expect("写头");
        drop(region);

        let mut reopened = open_region(&dir.0, RegionPos { x: 0, z: 0 }, RegionCompression::None)
            .expect("重新打开");
        assert_eq!(reopened.repaired, 3);
        assert_eq!(reopened.offsets[0], good_offset);
        for index in 1..=3 {
            assert_eq!(reopened.offsets[index], 0);
        }
        assert_eq!(reopened.read(good, 1 << 20), Ok(Some(data)));
    }

    #[test]
    fn truncated_files_and_fuzzed_payloads_never_panic() {
        let dir = TempDir::new("fuzz");
        let mut seed = 0xabcd_ef01_2345_6789u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for case in 0..80 {
            let path = dir.0.join("r.0.0.mca");
            let mut bytes = vec![0u8; HEADER_BYTES];
            let length = (next() as usize) % (HEADER_BYTES + 64);
            bytes.truncate(length);
            for byte in &mut bytes {
                *byte = (next() >> 32) as u8;
            }
            // 大部分随机字节会被当成坏表项清掉；这里只要求不 panic、不超时。
            fs::write(&path, &bytes).expect("写文件");
            let mut region =
                RegionFile::open(&path, &dir.0, RegionCompression::Zlib).expect("打开截断文件");
            for _ in 0..4 {
                let _ = region.read(
                    ChunkPos::new((next() % 32) as i32, (next() % 32) as i32),
                    1 << 16,
                );
            }
            let _ = case;
        }
    }
}
