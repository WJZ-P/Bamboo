//! 从官方服务端 jar 原样导出结构模板 `.nbt`。
//!
//! jar 里 `data/minecraft/structure/**.nbt` 每个条目是 gzip 压缩过的 `StructureTemplate`
//! NBT，jar 本身（zip）又对这些字节做了一次 deflate。这里解掉外层 deflate，把得到的
//! gzip 字节按注册名排序拼进一个 blob，另写一份「注册名 → (起点, 长度)」的 Rust 索引，
//! 供 `mc-bamboo-data` 用 `include_bytes!` 暴露。字节不做任何重编码，重跑无 diff。
//!
//! Export vanilla structure templates `.nbt` verbatim from the official server jar.
//! Each `data/minecraft/structure/**.nbt` entry is a gzip-compressed `StructureTemplate`
//! tag, which the jar (zip) deflates once more. We strip the outer deflate, concatenate
//! the resulting gzip bytes (sorted by id) into one blob, and write a Rust
//! "id -> (offset, len)" index that `mc-bamboo-data` exposes via `include_bytes!`.
//! Bytes are never re-encoded, so rerunning produces no diff.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use flate2::read::DeflateDecoder;

const STRUCTURE_PREFIX: &str = "data/minecraft/structure/";
const HEADER: &str = "// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。\n\n";

/// 找到 `target/vanilla/versions` 下含结构模板的官方 jar，导出 blob 和索引。
///
/// Locate the vanilla jar holding structure templates under `target/vanilla/versions`
/// and write the blob and index into the generated data directory.
pub fn export(work_dir: &Path, output_dir: &Path) -> Result<(), String> {
    let mut jars = Vec::new();
    collect_jars(&work_dir.join("versions"), &mut jars)?;
    // 路径排序让多个候选 jar 时结果确定；重跑无 diff 依赖这一步。
    // Sorting paths keeps the pick deterministic when several jars are present.
    jars.sort();

    let mut templates: Option<Vec<(String, Vec<u8>)>> = None;
    for path in &jars {
        let bytes =
            fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if let Ok(found) = read_templates(&bytes)
            && !found.is_empty()
        {
            templates = Some(found);
            break;
        }
    }
    let templates = templates.ok_or("no vanilla jar contains structure templates")?;

    let mut blob = Vec::new();
    let mut rows: Vec<(String, u32, u32)> = Vec::with_capacity(templates.len());
    for (name, data) in &templates {
        let start = u32::try_from(blob.len()).map_err(|_| "structure blob too large")?;
        let len = u32::try_from(data.len()).map_err(|_| "structure template too large")?;
        blob.extend_from_slice(data);
        rows.push((name.clone(), start, len));
    }

    fs::write(output_dir.join("structures.bin"), &blob)
        .map_err(|error| format!("cannot write structures.bin: {error}"))?;
    let index = format!("{HEADER}{}", index_file(&rows));
    fs::write(output_dir.join("structures.rs"), index)
        .map_err(|error| format!("cannot write structures.rs: {error}"))?;
    println!(
        "exported {} structure templates ({} gzip bytes)",
        rows.len(),
        blob.len()
    );
    Ok(())
}

/// 递归收一个目录下的所有 `.jar`，顺序由调用方排序决定。
///
/// Recursively collect every `.jar` under a directory; the caller sorts for determinism.
fn collect_jars(directory: &Path, jars: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            collect_jars(&path, jars)?;
        } else if path.extension().is_some_and(|extension| extension == "jar") {
            jars.push(path);
        }
    }
    Ok(())
}

/// 读出所有 `data/minecraft/structure/**.nbt`，返回 (注册名, gzip 字节)，按注册名排序。
///
/// Read all `data/minecraft/structure/**.nbt` entries as (id, gzip bytes), sorted by id.
fn read_templates(jar: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let entries = zip_entries(jar)?;
    let mut out = Vec::new();
    for entry in entries {
        let Some(relative) = entry.name.strip_prefix(STRUCTURE_PREFIX) else {
            continue;
        };
        let Some(stem) = relative.strip_suffix(".nbt") else {
            continue;
        };
        let data = entry_bytes(jar, &entry)?;
        // 解掉 zip 的 deflate 后应当正好是官方 `NbtIo.writeCompressed` 的 gzip 流。
        // After stripping the zip deflate the bytes must be the official gzip NBT stream.
        if !data.starts_with(&[0x1f, 0x8b]) {
            return Err(format!("{} is not gzip NBT", entry.name));
        }
        out.push((format!("minecraft:{stem}"), data));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

struct ZipEntry {
    name: String,
    method: u16,
    compressed_size: u32,
    local_offset: u32,
}

/// 读 zip 中央目录，只取后续要用的四个字段。
///
/// Parse the zip central directory, keeping only the four fields we need.
fn zip_entries(jar: &[u8]) -> Result<Vec<ZipEntry>, String> {
    let eocd = find_eocd(jar)?;
    let total = u16le(jar, eocd + 10)? as usize;
    let cd_offset = u32le(jar, eocd + 16)? as usize;
    let mut position = cd_offset;
    let mut out = Vec::with_capacity(total);
    for _ in 0..total {
        if !has_signature(jar, position, [0x50, 0x4b, 0x01, 0x02]) {
            return Err(format!("bad central directory signature at {position}"));
        }
        let method = u16le(jar, position + 10)?;
        let compressed_size = u32le(jar, position + 20)?;
        let name_len = u16le(jar, position + 28)? as usize;
        let extra_len = u16le(jar, position + 30)? as usize;
        let comment_len = u16le(jar, position + 32)? as usize;
        let local_offset = u32le(jar, position + 42)?;
        let name_bytes = jar
            .get(position + 46..position + 46 + name_len)
            .ok_or("truncated central directory name")?;
        let name = String::from_utf8(name_bytes.to_vec())
            .map_err(|error| format!("non-UTF8 zip entry name: {error}"))?;
        out.push(ZipEntry {
            name,
            method,
            compressed_size,
            local_offset,
        });
        position += 46 + name_len + extra_len + comment_len;
    }
    Ok(out)
}

/// 取一个条目的原始字节：method 0 直接取，method 8 解一层 raw deflate。
///
/// Read one entry's bytes: method 0 is stored, method 8 is a raw deflate stream.
fn entry_bytes(jar: &[u8], entry: &ZipEntry) -> Result<Vec<u8>, String> {
    let position = entry.local_offset as usize;
    if !has_signature(jar, position, [0x50, 0x4b, 0x03, 0x04]) {
        return Err(format!("bad local header for {}", entry.name));
    }
    let name_len = u16le(jar, position + 26)? as usize;
    let extra_len = u16le(jar, position + 28)? as usize;
    let start = position + 30 + name_len + extra_len;
    let end = start + entry.compressed_size as usize;
    let raw = jar
        .get(start..end)
        .ok_or_else(|| format!("truncated data for {}", entry.name))?;
    match entry.method {
        0 => Ok(raw.to_vec()),
        8 => {
            let mut out = Vec::new();
            DeflateDecoder::new(raw)
                .read_to_end(&mut out)
                .map_err(|error| format!("cannot inflate {}: {error}", entry.name))?;
            Ok(out)
        }
        other => Err(format!("unsupported zip method {other} for {}", entry.name)),
    }
}

/// 从文件尾往前找 EOCD（End of Central Directory）。
///
/// Scan backwards from the end of the file for the End of Central Directory record.
fn find_eocd(jar: &[u8]) -> Result<usize, String> {
    if jar.len() < 22 {
        return Err("jar is too small to be a zip".to_owned());
    }
    let lower = jar.len().saturating_sub(22 + u16::MAX as usize);
    let mut position = jar.len() - 22;
    loop {
        if has_signature(jar, position, [0x50, 0x4b, 0x05, 0x06]) {
            return Ok(position);
        }
        if position == lower {
            break;
        }
        position -= 1;
    }
    Err("zip EOCD not found".to_owned())
}

fn has_signature(jar: &[u8], position: usize, signature: [u8; 4]) -> bool {
    jar.get(position..position + 4) == Some(signature.as_slice())
}

fn u16le(jar: &[u8], position: usize) -> Result<u16, String> {
    let bytes = jar.get(position..position + 2).ok_or("truncated zip u16")?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32le(jar: &[u8], position: usize) -> Result<u32, String> {
    let bytes = jar.get(position..position + 4).ok_or("truncated zip u32")?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// 生成 `structures.rs`：blob 常量、注册名排序的索引表和按名字查字节的函数。
///
/// Generate `structures.rs`: the blob constant, the id-sorted index and a lookup function.
fn index_file(rows: &[(String, u32, u32)]) -> String {
    let mut out = String::new();
    out.push_str("/// 官方 jar 里全部结构模板的 gzip 字节，按注册名排序后拼成一块。\n");
    out.push_str("///\n");
    out.push_str(
        "/// Concatenated gzip bytes of every vanilla structure template, sorted by id.\n",
    );
    out.push_str("pub const BLOB: &[u8] = include_bytes!(\"structures.bin\");\n\n");
    out.push_str("/// `(注册名, 在 BLOB 里的起点, 字节长度)`，按注册名排序，供二分查找。\n");
    out.push_str("///\n");
    out.push_str("/// `(id, offset into BLOB, byte length)`, sorted by id for binary search.\n");
    out.push_str("pub static ENTRIES: &[(&str, u32, u32)] = &[\n");
    for (name, start, len) in rows {
        out.push_str(&format!("    ({name:?}, {start}, {len}),\n"));
    }
    out.push_str("];\n\n");
    out.push_str("/// 按注册名取模板的 gzip 字节，交给 `mc-bamboo-nbt::read_compressed` 解析。\n");
    out.push_str("///\n");
    out.push_str("/// Look up a template's gzip bytes by id; feed them to `read_compressed`.\n");
    out.push_str("pub fn get(name: &str) -> Option<&'static [u8]> {\n");
    out.push_str("    let index = ENTRIES.binary_search_by(|(id, _, _)| (*id).cmp(name)).ok()?;\n");
    out.push_str("    let (_, start, len) = ENTRIES[index];\n");
    out.push_str("    Some(&BLOB[start as usize..(start + len) as usize])\n");
    out.push_str("}\n");
    out
}
