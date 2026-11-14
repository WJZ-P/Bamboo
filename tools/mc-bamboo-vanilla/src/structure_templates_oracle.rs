//! 结构模板导出对拍：用官方 `NbtIo` 和 `mc-bamboo-nbt` 各自读整棵 NBT 树，逐节点比。
//!
//! Rust 侧把导出的 blob 拆成独立 `.nbt` 文件交给 Java `StructureTemplateOracle.java`，
//! Java 用官方 `NbtIo.readCompressed` 读导出文件和 jar 原文、校验字节一致，并输出每个
//! 模板的规范化 NBT 树哈希与字段计数。这里用 `read_compressed` 和生产
//! `StructureTemplate::parse` 复算同一行，逐模板比较。
//!
//! Compare exported structure templates: vanilla `NbtIo` and `mc-bamboo-nbt` each decode
//! the whole NBT tree, node by node. Rust splits the blob into standalone `.nbt` files for
//! Java `StructureTemplateOracle.java`, which verifies the exported bytes against the jar
//! and emits a canonical-tree hash plus field counts. This side recomputes the same line
//! with `read_compressed` and the production `StructureTemplate::parse`.

use std::{collections::HashSet, fs, path::Path, process::Command};

use mc_bamboo_data::structures as blob;
use mc_bamboo_nbt::{Accounter, Compression, Tag, read_compressed};
use mc_bamboo_worldgen::structure::template::StructureTemplate;

use crate::structure_templates;

pub fn run(mut command: Command, work_dir: &Path, _options: &[&str]) -> Result<(), String> {
    let templates_dir = work_dir.join("structure_templates");
    if templates_dir.exists() {
        fs::remove_dir_all(&templates_dir)
            .map_err(|error| format!("cannot clear {}: {error}", templates_dir.display()))?;
    }
    fs::create_dir_all(&templates_dir)
        .map_err(|error| format!("cannot create {}: {error}", templates_dir.display()))?;

    // 把 blob 按索引拆成 jar 里一样的相对路径，Java 侧才能按名字找原文。
    // Split the blob into the same relative paths as the jar so Java can find the originals.
    for &(name, start, len) in blob::ENTRIES {
        let relative = name
            .strip_prefix("minecraft:")
            .ok_or_else(|| format!("structure id has no namespace: {name}"))?;
        let path = templates_dir.join(format!("{relative}.nbt"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        fs::write(&path, &blob::BLOB[start as usize..(start + len) as usize])
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }

    let jar = structure_templates::vanilla_jar(work_dir)?;
    let output = command
        .arg(&templates_dir)
        .arg(&jar)
        .output()
        .map_err(|error| format!("cannot run StructureTemplateOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "StructureTemplateOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare(&String::from_utf8_lossy(&output.stdout))
}

/// 逐行比 Java 输出，并确认模板数量正好等于导出数量。
///
/// Compare Java's output line by line and confirm the template count matches the export.
fn compare(stdout: &str) -> Result<(), String> {
    let total = blob::ENTRIES.len();
    let mut seen = HashSet::new();
    let mut mismatches = 0usize;
    let mut byte_mismatches = 0usize;
    let mut first: Option<(String, String, String)> = None;
    for line in stdout.lines() {
        let id = line.split('\t').next().unwrap_or("");
        if id.is_empty() {
            continue;
        }
        seen.insert(id.to_owned());
        if line.contains("X=0") {
            byte_mismatches += 1;
        }
        let expected = expected_line(id)?;
        if line != expected {
            mismatches += 1;
            if first.is_none() {
                first = Some((id.to_owned(), line.to_owned(), expected));
            }
        }
    }
    if seen.len() != total {
        return Err(format!(
            "Java reported {} templates, exported {total}",
            seen.len()
        ));
    }
    if byte_mismatches > 0 {
        return Err(format!(
            "{byte_mismatches} templates differ from the official jar bytes"
        ));
    }
    if mismatches > 0 {
        let (id, got, want) = first.unwrap_or_default();
        return Err(format!(
            "{mismatches}/{total} templates differ; first is {id}\n  java: {got}\n  rust: {want}"
        ));
    }
    println!(
        "structure-templates: {total} templates, all NBT trees, counts and byte checks identical"
    );
    Ok(())
}

/// 用生产代码复算一行：X=1（期望导出字节等于 jar）、NBT 树哈希、字段计数。
///
/// Recompute one line with production code: X=1 (bytes must match the jar), tree hash,
/// and field counts.
fn expected_line(id: &str) -> Result<String, String> {
    let bytes = blob::get(id).ok_or_else(|| format!("unknown structure template {id}"))?;
    let (_, root) = read_compressed(bytes, Compression::Gzip, Accounter::unlimited())
        .map_err(|error| format!("cannot read {id}: {error}"))?;
    let hash = fnv(&norm_compound(&root));
    let template = StructureTemplate::parse(bytes)?;
    let palette_entries = template.palettes.first().map_or(0, Vec::len);
    let counts = format!(
        "S={},{},{};P={};Pe={};B={};E={};J={}",
        template.size.0,
        template.size.1,
        template.size.2,
        template.palettes.len(),
        palette_entries,
        template.blocks.len(),
        template.entities.len(),
        template.jigsaws.len()
    );
    Ok(format!("{id}\tX=1\t{hash:x}\t{counts}"))
}

/// 官方 `NbtIo` 和 `read_compressed` 两边都用这套规范化：键排序、浮点用位模式。
///
/// Both decoders use this canonicalization: sorted keys and bit patterns for floats.
fn norm_compound(entries: &[(String, Tag)]) -> String {
    let mut sorted: Vec<&(String, Tag)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::from("{");
    for (key, value) in sorted {
        out.push_str(key);
        out.push('=');
        out.push_str(&norm(value));
        out.push(';');
    }
    out.push('}');
    out
}

fn norm(tag: &Tag) -> String {
    match tag {
        Tag::Byte(v) => format!("b{v}"),
        Tag::Short(v) => format!("s{v}"),
        Tag::Int(v) => format!("i{v}"),
        Tag::Long(v) => format!("l{v}"),
        Tag::Float(v) => format!("f{:x}", v.to_bits()),
        Tag::Double(v) => format!("d{:x}", v.to_bits()),
        Tag::ByteArray(v) => array(v.iter().map(|value| value.to_string())),
        Tag::IntArray(v) => array(v.iter().map(|value| value.to_string())),
        Tag::LongArray(v) => array(v.iter().map(|value| value.to_string())),
        Tag::String(v) => format!("q{v}"),
        Tag::List(v) => {
            let mut out = String::from("[");
            for element in v {
                out.push_str(&norm(element));
                out.push(',');
            }
            out.push(']');
            out
        }
        Tag::Compound(v) => norm_compound(v),
    }
}

fn array(values: impl Iterator<Item = String>) -> String {
    let mut out = String::from("A[");
    for value in values {
        out.push_str(&value);
        out.push(',');
    }
    out.push(']');
    out
}

/// FNV-1a 64 位，和 Java 的 `StructureTemplateOracle.fnv` 一致。
///
/// FNV-1a 64-bit, matching Java `StructureTemplateOracle.fnv`.
fn fnv(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
