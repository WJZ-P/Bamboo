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
    (42, -32, -16),
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
    // 按结构类型补的起点区块 / 被引用区块（见 docs/worldgen-structures-e2e.md）。
    // 村庄五种、前哨、古城、古迹废墟、沙漠神殿、雪屋、要塞等。
    (42, -383, -289), // village_plains
    (42, -366, -319), // village_desert
    (42, -393, -227), // village_savanna
    (42, 364, -29),   // village_snowy
    (42, -351, 378),  // village_taiga
    (42, -379, -303), // pillager_outpost
    (42, -165, 80),   // ancient_city
    (42, -405, 273),  // trail_ruins
    (42, 208, -305),  // igloo
    (42, 40, 55),     // shipwreck_beached
    (42, -19, 11),    // ruined_portal_swamp
    (42, -14, 21),    // abandoned_camp_swamp
    // 沙漠神殿起点已修好，和它的被引用邻区块一起进默认集。要塞起点还有已知
    // 差异（见下），用被引用邻区块覆盖跨区块放置。
    // The desert-pyramid start is now clean; the stronghold start still has a known
    // gap, so its referenced neighbours cover cross-chunk placement.
    (42, -375, -22), // desert_pyramid start
    (42, -374, -22), // desert_pyramid referenced neighbour
    (42, -121, 97),  // stronghold referenced neighbour
    (42, -120, 96),  // stronghold referenced neighbour
    // 按 `random_spread` 电位起点定位找到的四种稀有结构起点。
    // Rare-structure starts located via the official `random_spread` planner.
    (42, -6380, -6052), // woodland_mansion
    (42, -6380, -2374), // woodland_mansion (second layout)
    (42, -12, -148),    // ocean_monument
    (42, 49, -11),      // ocean_monument (second layout)
    (42, -85, 180),     // jungle_temple
    (42, 3, 10),        // ocean_ruin_cold
    (42, -32, -79),     // ocean_ruin_warm
    // 海底遗迹真的放下可疑的沙砾并带战利品表 / 种子（5 个刷扫 + 1 个箱子）。
    // Ocean ruin pieces that actually place suspicious gravel with loot tables
    // and seeds (5 brushable blocks + 1 chest).
    (42, -50, -197),
];

/// 已定位但尚未修好的整块差异，用 `--known-gaps` 单独跑，不进默认干净跑。
/// 要塞起点的矿井支柱在真实 `WorldGenRegion` 上还差 4 格；`structure-place`
/// 用合成地形逐件对拍它是 0 不一致。
/// Known-but-unfixed whole-chunk gaps, run with `--known-gaps`; the default clean
/// run excludes them. `structure-place` still covers this piece type block-for-block.
const KNOWN_GAPS: &[(i64, i32, i32)] = &[
    (42, -121, 96), // stronghold
];

const USAGE: &str = "usage: structure-e2e <java> [--break-self-test] [--break-structure KIND] \
     [--structure-types IDS] [--show N] [--known-gaps] [--chunks-file FILE]";

#[derive(Debug)]
struct Options {
    show: usize,
    break_self_test: bool,
    /// 改坏确认：跳过这个结构类型 ID 的结构件放置。
    break_structure: Option<String>,
    /// 只跑已知差异区块，供继续排查用。
    known_gaps: bool,
    /// 用外部 `seed cx cz` 文件代替内置查询集（诊断新区块用）。
    /// Use an external `seed cx cz` file instead of the built-in query set.
    chunks_file: Option<String>,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut show = 8_usize;
        let mut break_self_test = false;
        let mut break_structure = None;
        let mut known_gaps = false;
        let mut chunks_file = None;
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--show" => show = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?,
                "--break-self-test" => break_self_test = true,
                "--break-structure" => {
                    break_structure = Some((*args.next().ok_or(USAGE)?).to_owned());
                }
                // 只做诊断筛选时用的占位参数；当前查询集固定，两边都跑全部结构。
                "--structure-types" => {
                    let _ = args.next().ok_or(USAGE)?;
                }
                "--known-gaps" => known_gaps = true,
                "--chunks-file" => {
                    chunks_file = Some((*args.next().ok_or(USAGE)?).to_owned());
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(Self {
            show,
            break_self_test,
            break_structure,
            known_gaps,
            chunks_file,
        })
    }
}

/// 写查询文件并调 `gen-world --through features --structures`。
pub fn run(mut command: Command, queries_path: &Path, options: &[&str]) -> Result<(), String> {
    let parsed = Options::parse(options)?;
    let queries: &[(i64, i32, i32)] = if parsed.known_gaps {
        KNOWN_GAPS
    } else {
        QUERIES
    };
    if let Some(path) = &parsed.chunks_file {
        let text =
            fs::read_to_string(path).map_err(|e| format!("cannot read chunks file {path}: {e}"))?;
        let rows: Vec<(i64, i32, i32)> = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() != 3 {
                    return Err(format!("bad chunks file line: {line}"));
                }
                Ok((
                    parts[0].parse().map_err(|_| format!("bad seed: {line}"))?,
                    parts[1].parse().map_err(|_| format!("bad x: {line}"))?,
                    parts[2].parse().map_err(|_| format!("bad z: {line}"))?,
                ))
            })
            .collect::<Result<_, String>>()?;
        let text = rows
            .iter()
            .map(|(seed, x, z)| format!("{seed} {x} {z}\n"))
            .collect::<String>();
        fs::write(queries_path, &text).map_err(|e| format!("cannot write queries: {e}"))?;
    } else {
        let text = queries
            .iter()
            .map(|(seed, x, z)| format!("{seed} {x} {z}\n"))
            .collect::<String>();
        fs::write(queries_path, &text).map_err(|e| format!("cannot write queries: {e}"))?;
    }
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
    if let Some(kind) = &parsed.break_structure {
        forwarded.push("--break-structure".to_owned());
        forwarded.push(kind.clone());
    }
    let _ = &mut command;
    crate::gen_world::run_with_queries(command, &as_refs(&forwarded), Some(queries_path))
}

fn as_refs(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}
