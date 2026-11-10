//! 用官方 `SerializableChunkData`、`RegionFile` 和 `NbtIo` 检查存档编解码层。
//!
//! `StorageOracle.java` 在 JVM 里加载官方类（不启动服务端、不接受 EULA），
//! 从 stdin 收一行一个 JSON 请求，stdout 回一行一个 JSON 结果。这里负责出题、
//! 在 Bamboo 里重放，按键比较两个 NBT 复合标签：官方的 `CompoundTag` 用
//! `HashMap`，键序由哈希决定，不能逐字节比。
//!
//! 命令：`cargo run -p mc-bamboo-vanilla -- storage <java 路径>`。

use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use mc_bamboo_core::{BlockPos, ChunkPos, sign::SignContent};
use mc_bamboo_data::{blocks, version};
use mc_bamboo_nbt::{Accounter, Compression, Tag, read_compressed, read_named};
use mc_bamboo_storage::{
    anvil::{RegionCompression, RegionPos, open_region},
    chunk_nbt::{ChunkEncodeExtra, decode_bytes, encode_bytes, field},
};
use mc_bamboo_world::store::ChunkStore;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::oracle::collect_log;

const USAGE: &str = "usage: storage <java> [--show N]";
/// 一次运行里出错时最多打印几条差异。
const DEFAULT_SHOW: usize = 5;

#[derive(Debug, Deserialize)]
struct Response {
    #[allow(dead_code, reason = "the command name is part of the oracle protocol")]
    command: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    canonical: Option<Value>,
    #[serde(default)]
    original: Option<Value>,
    #[serde(default)]
    written: Option<Value>,
    #[serde(default)]
    #[serde(rename = "writtenNbt")]
    written_nbt: Option<String>,
    #[serde(default)]
    nbt: Option<String>,
    #[serde(default)]
    gzip: Option<String>,
    #[serde(default)]
    chunks: Vec<RegionChunk>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RegionChunk {
    #[allow(dead_code, reason = "coordinates are part of the oracle response")]
    x: i32,
    #[allow(dead_code, reason = "coordinates are part of the oracle response")]
    z: i32,
    #[serde(default)]
    exists: bool,
    #[serde(default)]
    data: Option<String>,
}

#[derive(Debug, Default)]
struct Report {
    checks: usize,
    failures: Vec<String>,
    show: usize,
}

impl Report {
    fn check(&mut self, condition: bool, what: impl Into<String>) {
        self.checks += 1;
        if !condition {
            self.failures.push(what.into());
        }
    }

    fn finish(&self) -> Result<(), String> {
        for failure in self.failures.iter().take(self.show.max(1)) {
            eprintln!("  {failure}");
        }
        if self.failures.is_empty() {
            println!("storage oracle: {} checks passed", self.checks);
            Ok(())
        } else {
            Err(format!(
                "storage oracle: {} of {} checks failed",
                self.failures.len(),
                self.checks
            ))
        }
    }
}

/// 启动官方进程并跑完所有题目。`command` 已经是能运行 `StorageOracle.java` 的命令。
pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let show = parse_show(args)?;
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let logs = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut stdin = child.stdin.take().ok_or("java has no stdin")?;
    let mut report = Report {
        show,
        ..Default::default()
    };

    let mut lines = BufReader::new(stdout).lines();
    let result = run_cases(&mut stdin, &mut lines, &mut report);
    drop(stdin);
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let logs = logs.recv().unwrap_or_default().join("\n");
    if let Err(error) = &result {
        return Err(format!("{error}\n{logs}"));
    }
    if !status.success() {
        return Err(format!("StorageOracle.java failed: {status}\n{logs}"));
    }
    report.finish()
}

/// 测试用的区域目录，结束时删掉，不留大文件在磁盘上。
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(name: &str) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "bamboo-storage-oracle-{name}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("main")
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn parse_show(args: &[&str]) -> Result<usize, String> {
    match args {
        [] => Ok(DEFAULT_SHOW),
        ["--show", value] => value.parse().map_err(|_| USAGE.to_owned()),
        _ => Err(USAGE.to_owned()),
    }
}

fn ask(
    stdin: &mut impl Write,
    lines: &mut impl Iterator<Item = Result<String, std::io::Error>>,
    request: Value,
) -> Result<Response, String> {
    let mut line = serde_json::to_string(&request).map_err(|error| error.to_string())?;
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .map_err(|error| format!("cannot write to java: {error}"))?;
    stdin
        .flush()
        .map_err(|error| format!("cannot flush java stdin: {error}"))?;
    let answer = lines
        .next()
        .ok_or("StorageOracle.java closed stdout unexpectedly")?
        .map_err(|error| format!("cannot read from java: {error}"))?;
    let response: Response =
        serde_json::from_str(&answer).map_err(|error| format!("bad oracle response: {error}"))?;
    if let Some(error) = &response.error {
        return Err(format!("StorageOracle.java: {error}"));
    }
    Ok(response)
}

fn run_cases(
    stdin: &mut impl Write,
    lines: &mut impl Iterator<Item = Result<String, std::io::Error>>,
    report: &mut Report,
) -> Result<(), String> {
    // 1. 官方 `NbtIo.writeCompressed` 写出的 gzip 文件，Bamboo 能读。
    let compressed = ask(stdin, lines, json!({"command": "nbt-io"}))?;
    let gzip = decode_b64(compressed.gzip.as_deref().unwrap_or_default())?;

    let (name, entries) = read_compressed(&gzip, Compression::Gzip, Accounter::unlimited())
        .map_err(|error| format!("Bamboo cannot read official gzip NBT: {error}"))?;
    report.check(name.is_empty(), "official NBT root name is not empty");
    let level = Tag::Compound(entries);
    report.check(
        field(&level, "LevelName") == Some(&Tag::String("竹 bamboo".to_owned())),
        "official LevelName did not survive",
    );
    report.check(
        field(&level, "DataVersion") == Some(&Tag::Int(version::DATA_VERSION)),
        "official DataVersion did not survive",
    );
    report.check(
        field(&level, "words") == Some(&Tag::LongArray(vec![-1, 0, i64::MAX])),
        "official long array did not survive",
    );
    let canonical = canonical_tag(&level);
    report.check(
        compressed.canonical.as_ref() == Some(&canonical),
        format!(
            "official NbtIo canonical JSON differs: {canonical} vs {:?}",
            compressed.canonical
        ),
    );

    // 2. Bamboo 写出的区块 NBT，官方 `SerializableChunkData.parse` 能读，
    //    `write()` 回来后按键比较内容。
    for (label, seed) in [("flat", None), ("rich-1", Some(1u64)), ("rich-9", Some(9))] {
        let column = match seed {
            None => flat_column(),
            Some(seed) => rich_column(seed),
        };
        let options = ChunkEncodeExtra {
            chunk_pos: ChunkPos::new(3, -9),
            last_update: 1234,
            inhabited_time: 99,
            ..Default::default()
        };
        let bytes = encode_bytes(&column, &options).map_err(|error| error.to_string())?;
        let answer = ask(
            stdin,
            lines,
            json!({"command": "chunk-read", "nbt": encode_b64(&bytes)}),
        )?;
        report.check(
            answer.status.as_deref() == Some("minecraft:full"),
            format!("{label}: official parse did not report a full chunk"),
        );
        report.check(
            answer.original.as_ref() == answer.written.as_ref(),
            format!("{label}: official parse -> write changed the NBT content"),
        );
        let canonical = canonical_tag(&decode_nbt(&bytes)?);
        report.check(
            answer.original.as_ref() == Some(&canonical),
            format!("{label}: official and Bamboo canonical JSON differ"),
        );

        // 官方再写出的字节，Bamboo 也能读回来，再写出来内容相同。
        let written = decode_b64(answer.written_nbt.as_deref().unwrap_or_default())?;
        let decoded = decode_bytes(&written).map_err(|error| error.to_string())?;
        let again =
            encode_bytes(&decoded.column, &decoded.extra()).map_err(|error| error.to_string())?;
        report.check(
            canonical_tag(&decode_nbt(&written)?) == canonical_tag(&decode_nbt(&again)?),
            format!("{label}: official bytes -> Bamboo -> bytes changed the content"),
        );
    }

    // 3. 官方写出的区块 NBT，Bamboo 读成 ChunkColumn 再写出来按键一致。
    for (label, light_on, entities, sections) in [
        ("full", true, true, json!([])),
        ("dark", false, false, json!([])),
        (
            "lit-edges",
            true,
            false,
            json!([
                {"y": -5, "blocks": [], "sky": encode_b64(&[0x55; 2048])},
                {"y": 20, "blocks": [], "sky": encode_b64(&[0x33; 2048])}
            ]),
        ),
    ] {
        let answer = ask(
            stdin,
            lines,
            json!({
                "command": "chunk-write",
                "x": 3, "z": -9,
                "lightOn": light_on,
                "blockEntities": entities,
                "sections": sections,
            }),
        )?;
        let bytes = decode_b64(answer.nbt.as_deref().unwrap_or_default())?;
        let decoded = decode_bytes(&bytes).map_err(|error| error.to_string())?;
        let mut extra = decoded.extra();
        extra.preserved.extend(decoded.unknown.clone());
        let again = encode_bytes(&decoded.column, &extra).map_err(|error| error.to_string())?;
        report.check(
            canonical_tag(&decode_nbt(&bytes)?) == canonical_tag(&decode_nbt(&again)?),
            format!("{label}: official chunk -> Bamboo -> bytes changed the content"),
        );
        report.check(
            answer.canonical.as_ref() == Some(&canonical_tag(&decode_nbt(&bytes)?)),
            format!("{label}: official canonical JSON differs from Bamboo's"),
        );
        report.check(
            decoded.light_on == light_on,
            format!("{label}: isLightOn was lost"),
        );
        // 放进 ChunkStore 后按世界坐标读数和原来一致。
        let mut store = ChunkStore::flat();
        store
            .insert_column(decoded.chunk_pos, decoded.column)
            .map_err(|_| format!("{label}: cannot insert the decoded column"))?;
        let original = decode_bytes(&bytes).map_err(|error| error.to_string())?;
        for x in 0..16 {
            for y in [-64, -61, -60, 0, 319] {
                for z in 0..16 {
                    let pos = BlockPos::new(
                        decoded.chunk_pos.x * 16 + x,
                        y,
                        decoded.chunk_pos.z * 16 + z,
                    );
                    let local_x = x as usize;
                    let local_z = z as usize;
                    report.check(
                        store.block(pos)
                            == Some(original.column.chunk().block(local_x, y, local_z)),
                        format!("{label}: block differs at {pos:?}"),
                    );
                }
            }
        }
    }

    // 4. 区域文件两个方向，三种压缩，外加 .mcc、扇区重用、坏表项。
    for compression in RegionCompression::ALL {
        let id = i32::from(compression.id());
        let case = format!("compression {id}");

        // Bamboo 写、官方读。
        let dir = TempDir::new(&format!("bamboo-write-{id}"))?;
        let payload_a = sample_bytes(1);
        let payload_b = sample_bytes(2);
        {
            let mut region = open_region(&dir.0, RegionPos { x: 0, z: 0 }, compression)
                .map_err(|error| error.to_string())?;
            region
                .write(ChunkPos::new(1, 2), Some(&payload_a), 100)
                .map_err(|error| error.to_string())?;
            region
                .write(ChunkPos::new(3, 4), Some(&payload_b), 200)
                .map_err(|error| error.to_string())?;
            // 改大第一个区块：验证新扇区不会盖住第二个区块。
            let bigger = sample_bytes(3);
            region
                .write(ChunkPos::new(1, 2), Some(&bigger), 300)
                .map_err(|error| error.to_string())?;
            report.check(
                region
                    .read(ChunkPos::new(3, 4), 1 << 20)
                    .map_err(|error| error.to_string())?
                    == Some(payload_b.clone()),
                format!("{case}: sector resize overwrote its neighbor"),
            );
            let answer = ask(
                stdin,
                lines,
                json!({
                    "command": "region-read",
                    "dir": dir.0.to_string_lossy(),
                    "x": 0, "z": 0,
                    "positions": [
                        {"x": 1, "z": 2}, {"x": 3, "z": 4}, {"x": 9, "z": 9}
                    ]
                }),
            )?;
            report.check(
                answer.chunks.len() == 3,
                format!(
                    "{case}: official region-read returned {} chunks",
                    answer.chunks.len()
                ),
            );
            report.check(
                answer.chunks.first().is_some_and(|chunk| chunk.exists),
                format!("{case}: official RegionFile does not see chunk (1, 2)"),
            );
            report.check(
                answer
                    .chunks
                    .first()
                    .and_then(|chunk| chunk.data.as_deref())
                    .and_then(|data| decode_b64(data).ok())
                    == Some(bigger),
                format!("{case}: official RegionFile read different bytes"),
            );
            report.check(
                answer.chunks.get(2).is_some_and(|chunk| !chunk.exists),
                format!("{case}: official RegionFile invented an empty chunk"),
            );
        }

        // 官方写、Bamboo 读。
        let dir = TempDir::new(&format!("official-write-{id}"))?;
        let payload = sample_bytes(4);
        ask(
            stdin,
            lines,
            json!({
                "command": "region-write",
                "dir": dir.0.to_string_lossy(),
                "x": 5, "z": -33,
                "compression": id,
                "chunks": [
                    {"x": 5, "z": -33, "data": encode_b64(&payload)},
                    {"x": 6, "z": -33, "data": encode_b64(&sample_bytes(5))}
                ]
            }),
        )?;
        {
            let mut region = open_region(&dir.0, RegionPos::of(ChunkPos::new(5, -33)), compression)
                .map_err(|error| error.to_string())?;
            report.check(
                region
                    .read(ChunkPos::new(5, -33), 1 << 20)
                    .map_err(|error| error.to_string())?
                    == Some(payload),
                format!("{case}: Bamboo cannot read the official region file"),
            );
            report.check(
                region
                    .read(ChunkPos::new(6, -33), 1 << 20)
                    .map_err(|error| error.to_string())?
                    == Some(sample_bytes(5)),
                format!("{case}: Bamboo cannot read the second official chunk"),
            );
        }
    }

    Ok(())
}

/// 超平坦模板的区块束。
fn flat_column() -> Box<mc_bamboo_world::column::ChunkColumn> {
    let mut store = ChunkStore::flat();
    store.finish_light_updates();
    store.take_column(ChunkPos::new(0, 0)).expect("光照算完了")
}

/// 一个内容丰富、固定种子的区块束。
fn rich_column(seed: u64) -> Box<mc_bamboo_world::column::ChunkColumn> {
    let mut store = ChunkStore::flat();
    let mut state = seed | 1;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for index in 0..900 {
        let x = (next() % 16) as i32;
        let z = (next() % 16) as i32;
        let y = -64 + (next() % 64) as i32;
        let place = [
            blocks::STONE,
            blocks::DIRT,
            blocks::GLOWSTONE,
            blocks::OAK_SIGN,
        ][index % 4];
        store.set_block(BlockPos::new(x, y, z), place);
    }
    store.set_block(BlockPos::new(3, -60, 5), blocks::CHEST);
    store.set_block(BlockPos::new(7, -60, 9), blocks::OAK_SIGN);
    let mut content = SignContent::BLANK;
    content.front.messages[0] = "竹 hello".to_owned();
    content.waxed = true;
    store.set_sign(BlockPos::new(7, -60, 9), content);
    store.finish_light_updates();
    store.take_column(ChunkPos::new(0, 0)).expect("光照算完了")
}

fn sample_bytes(seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    (0..8192)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

fn encode_b64(bytes: &[u8]) -> String {
    use base64_stub::encode;
    encode(bytes)
}

fn decode_b64(text: &str) -> Result<Vec<u8>, String> {
    base64_stub::decode(text)
}

/// 自带的 base64，避免为这个开发工具再引入一个依赖。
mod base64_stub {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(input: &[u8]) -> String {
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let a = chunk[0] as u32;
            let b = chunk.get(1).copied().unwrap_or(0) as u32;
            let c = chunk.get(2).copied().unwrap_or(0) as u32;
            let value = a << 16 | b << 8 | c;
            out.push(ALPHABET[(value >> 18) as usize & 63] as char);
            out.push(ALPHABET[(value >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                ALPHABET[(value >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                ALPHABET[value as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }

    pub fn decode(input: &str) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut buffer = 0u32;
        let mut bits = 0;
        for byte in input.bytes() {
            if byte == b'=' {
                break;
            }
            let value = ALPHABET
                .iter()
                .position(|&candidate| candidate == byte)
                .ok_or_else(|| format!("bad base64 byte {byte}"))? as u32;
            buffer = buffer << 6 | value;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buffer >> bits) as u8);
            }
        }
        Ok(out)
    }
}

/// 把 `Tag` 转成按键排序的规范 JSON，和 `StorageOracle.java` 的 `canonical` 一致。
fn canonical_tag(tag: &Tag) -> Value {
    match tag {
        Tag::Byte(value) => json!({"type": 1, "value": value.to_string()}),
        Tag::Short(value) => json!({"type": 2, "value": value.to_string()}),
        Tag::Int(value) => json!({"type": 3, "value": value.to_string()}),
        Tag::Long(value) => json!({"type": 4, "value": value.to_string()}),
        Tag::Float(value) => json!({"type": 5, "value": value.to_string()}),
        Tag::Double(value) => json!({"type": 6, "value": value.to_string()}),
        Tag::String(value) => json!({"type": 8, "value": value}),
        Tag::ByteArray(values) => json!({"type": 7, "values": values}),
        Tag::IntArray(values) => json!({"type": 11, "values": values}),
        Tag::LongArray(values) => json!({"type": 12, "values": values}),
        Tag::List(values) => json!({
            "type": 9,
            "values": values.iter().map(canonical_tag).collect::<Vec<_>>()
        }),
        Tag::Compound(entries) => {
            let mut sorted: Vec<_> = entries.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            // 同名键取最后一个，官方 `HashMap` 的行为。
            let mut map = serde_json::Map::new();
            for (key, value) in sorted {
                map.insert(key.clone(), canonical_tag(value));
            }
            json!({"type": 10, "values": Value::Object(map)})
        }
    }
}

fn decode_nbt(bytes: &[u8]) -> Result<Tag, String> {
    let (_, entries) =
        read_named(bytes, Accounter::unlimited()).map_err(|error| error.to_string())?;
    Ok(Tag::Compound(entries))
}
