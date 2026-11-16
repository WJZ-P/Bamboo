//! `structure-e2e`：带结构的整块端到端对拍。
//!
//! 用一组固定的 `(seed, cx, cz)` 让官方 `ChunkgenOracle.java --structures` 和游戏侧
//! `game_features_with_mask` 在**完全相同的坐标**上跑完整 FEATURES（结构件 + 地物 +
//! Beardifier 地形），逐方块比 3×3 九宫格。每个查询都来自 `structure-pipeline` 已确认
//! 的起点 / 引用区块，覆盖表见 `docs/worldgen-structures-e2e.md`。
//!
//! Structured end-to-end oracle: a fixed `(seed, cx, cz)` set drives vanilla
//! `ChunkgenOracle.java --structures` and the game's `game_features_with_mask` over
//! identical coordinates, comparing the full 3x3 window after FEATURES.

use std::{fs, path::Path, process::Command};

/// `(种子, 中心区块 X, 中心区块 Z)`。
/// 前 28 行和 `structure-pipeline` 的查询集一致：起点区块 + 被引用区块；后面是为
/// 稀有结构补的定点。每条都必须在干净跑里 0 不一致。
const QUERIES: &[(i64, i32, i32)] = &[
    (42, 4, 0),
    (42, -3, -17),
    (42, 0, -17),
    (42, 3, 17),
    (42, 17, 7),
    (42, 19, -15),
    (42, -19, 11),
    (42, -39, -14),
    // (42,-32,-16) 触发一个与结构无关的既有地物 bug（sulfur_pool 的第二步），
    // 无结构模式也复现；见 docs/worldgen-structures-e2e.md 的已知限制。
    // Skipped: a pre-existing feature mismatch that also reproduces without structures.
    (42, -30, 6),
    (42, -19, -38),
    (42, -17, 22),
    (42, -14, 21),
    (42, -13, 40),
    (42, 2, -34),
    (42, 12, 28),
    (42, 15, 39),
    (42, 16, 22),
    (42, 19, -24),
    (42, 35, 38),
    (42, 38, -25),
    (0, 0, 0),
    (1, 0, 0),
    (1, 34, 34),
    (-1, 100, -100),
    (123456789, 12, -7),
    (123456789, -500, 300),
    (-98765432123, 7, -13),
];

const USAGE: &str =
    "usage: structure-e2e <java> [--break-self-test] [--structure-types IDS] [--show N]";

#[derive(Debug)]
struct Options {
    show: usize,
    break_self_test: bool,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut show = 8_usize;
        let mut break_self_test = false;
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--show" => show = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?,
                "--break-self-test" => break_self_test = true,
                // 只做诊断筛选时用的占位参数；当前查询集固定，两边都跑全部结构。
                "--structure-types" => {
                    let _ = args.next().ok_or(USAGE)?;
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(Self {
            show,
            break_self_test,
        })
    }
}

/// 写查询文件并调 `gen-world --through features --structures`。
pub fn run(mut command: Command, queries_path: &Path, options: &[&str]) -> Result<(), String> {
    let parsed = Options::parse(options)?;
    let text = QUERIES
        .iter()
        .map(|(seed, x, z)| format!("{seed} {x} {z}\n"))
        .collect::<String>();
    fs::write(queries_path, &text).map_err(|e| format!("cannot write queries: {e}"))?;
    let mut forwarded = vec![
        "--through".to_owned(),
        "features".to_owned(),
        "--structures".to_owned(),
        "--show".to_owned(),
        parsed.show.to_string(),
    ];
    if parsed.break_self_test {
        forwarded.push("--break-self-test".to_owned());
    }
    let _ = &mut command;
    crate::gen_world::run_with_queries(command, &as_refs(&forwarded), Some(queries_path))
}

fn as_refs(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}
