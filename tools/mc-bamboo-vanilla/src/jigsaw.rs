//! Jigsaw 官方对拍（Rust 侧）。
//!
//! `jigsaw` 子命令跑 `JigsawOracle.java`：Java 端造真实的
//! `StructureTemplateManager`（从 vanilla 数据包加载模板），这里用生产代码
//! `mc-bamboo-worldgen::structure::template` 复算并逐模板比较。
//!
//! 第一步（templates）只验证模板被正确加载：size、`save()` 的 blocks 数、
//! `getJigsaws(ZERO, NONE)` 的拼图方块数。多 palette 模板的 `getRandomPalette`
//! 按位置随机选取尚未移植，这一步只放单 palette 模板。
//!
//! Rust side of the vanilla jigsaw oracle. The `jigsaw` subcommand runs
//! `JigsawOracle.java`, which builds a real `StructureTemplateManager` from the
//! vanilla data pack; the production `structure::template` parser recomputes and
//! compares per template. Step one checks size, saved block count and jigsaw
//! count; multi-palette templates are excluded until `getRandomPalette` is ported.

use std::{fs, path::Path, process::Command};

use mc_bamboo_data::structures as blob;
use mc_bamboo_worldgen::structure::template::StructureTemplate;

pub fn run(mut command: Command, work_dir: &Path, _options: &[&str]) -> Result<(), String> {
    let queries_path = work_dir.join("jigsaw_queries.txt");
    let ids = template_queries();
    fs::write(&queries_path, ids.join("\n"))
        .map_err(|error| format!("cannot write queries: {error}"))?;

    let output = command
        .arg("templates")
        .arg(&queries_path)
        .output()
        .map_err(|error| format!("cannot run JigsawOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "JigsawOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare(&String::from_utf8_lossy(&output.stdout), ids.len())
}

/// 全部单 palette 模板 id，按名字排序（blob 索引已排序）。
///
/// Every single-palette template id, sorted by name (the blob index is sorted).
fn template_queries() -> Vec<String> {
    blob::ENTRIES
        .iter()
        .filter(|(name, _, _)| {
            StructureTemplate::load(name).is_ok_and(|template| template.palettes.len() == 1)
        })
        .map(|(name, _, _)| (*name).to_owned())
        .collect()
}

/// Java 每行 `id|sx|sy|sz|blocks|jigsaws`；用生产解析器复算，逐行比。
///
/// Java emits `id|sx|sy|sz|blocks|jigsaws` per line; recompute and compare.
fn compare(stdout: &str, expected_count: usize) -> Result<(), String> {
    let mut checked = 0usize;
    let mut mismatches = 0usize;
    let mut first: Option<(String, String)> = None;
    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let id = line.split('|').next().unwrap_or("");
        let template =
            StructureTemplate::load(id).map_err(|error| format!("cannot load {id}: {error}"))?;
        let want = format!(
            "{id}|{}|{}|{}|{}|{}",
            template.size.0,
            template.size.1,
            template.size.2,
            template.blocks.len(),
            template.jigsaws.len()
        );
        checked += 1;
        if line != want {
            mismatches += 1;
            if first.is_none() {
                first = Some((line.to_owned(), want));
            }
        }
    }
    if checked != expected_count {
        return Err(format!(
            "Java reported {checked} templates, expected {expected_count}"
        ));
    }
    if mismatches > 0 {
        let (got, want) = first.unwrap_or_default();
        return Err(format!(
            "{mismatches}/{checked} templates differ\n  java: {got}\n  rust: {want}"
        ));
    }
    println!("jigsaw templates: {checked} templates, size/blocks/jigsaws all identical");
    Ok(())
}
