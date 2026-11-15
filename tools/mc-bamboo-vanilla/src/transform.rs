//! 方块状态 `mirror` + `rotate` 的对拍：官方 `BlockState.mirror(mirror).rotate(rotation)`
//! 对 `mc-bamboo-worldgen::structure::state_transform::transform`。
//!
//! 这两步就是结构件 `StructurePiece.placeBlock` 在写方块前做的事，四组
//! `(mirror, rotation)` 来自 `setOrientation`。对拍覆盖矿井与要塞放置时用到的全部方块
//! （木板、原木、栅栏、墙、铁栏杆、玻璃板、楼梯、台阶、火把、按钮、门、铁轨、箱子、
//! 末地传送门框架、刷怪笼、铁链、蜘蛛网……）以及它们的全部状态组合。
//!
//! Oracle for the two state steps `StructurePiece.placeBlock` runs before writing:
//! vanilla `BlockState.mirror(mirror).rotate(rotation)` against
//! `state_transform::transform`. The four `(mirror, rotation)` pairs come from
//! `setOrientation`. The query set covers every block the mineshaft and stronghold place,
//! across all of their state combinations.

use std::{fs, path::Path, process::Command};

use mc_bamboo_data::{block_by_name, blocks};
use mc_bamboo_worldgen::structure::state_transform::{self, Mirror, Rotation};

/// 参与对拍的方块。矿井与要塞放置时用到，外加几种常见方块做交叉验证。
///
/// Blocks in the sweep: everything the mineshaft and stronghold place, plus a few common
/// ones for cross-checking.
const BLOCKS: &[&str] = &[
    // 矿井
    "minecraft:oak_log",
    "minecraft:oak_planks",
    "minecraft:oak_fence",
    "minecraft:dark_oak_log",
    "minecraft:dark_oak_planks",
    "minecraft:dark_oak_fence",
    "minecraft:cobweb",
    "minecraft:rail",
    "minecraft:wall_torch",
    "minecraft:torch",
    "minecraft:iron_chain",
    "minecraft:spawner",
    "minecraft:cave_air",
    // 要塞
    "minecraft:stone_bricks",
    "minecraft:cracked_stone_bricks",
    "minecraft:mossy_stone_bricks",
    "minecraft:infested_stone_bricks",
    "minecraft:smooth_stone_slab",
    "minecraft:stone_brick_slab",
    "minecraft:stone_brick_stairs",
    "minecraft:cobblestone_stairs",
    "minecraft:oak_door",
    "minecraft:iron_door",
    "minecraft:stone_button",
    "minecraft:iron_bars",
    "minecraft:bookshelf",
    "minecraft:ladder",
    "minecraft:chest",
    "minecraft:end_portal_frame",
    "minecraft:end_portal",
    "minecraft:cobblestone",
    "minecraft:water",
    "minecraft:lava",
    // 交叉验证
    "minecraft:oak_stairs",
    "minecraft:oak_slab",
    "minecraft:oak_trapdoor",
    "minecraft:oak_fence_gate",
    "minecraft:stone_brick_wall",
    "minecraft:white_stained_glass_pane",
    "minecraft:powered_rail",
    "minecraft:detector_rail",
    "minecraft:activator_rail",
    "minecraft:furnace",
    "minecraft:hopper",
    "minecraft:observer",
    "minecraft:piston",
    "minecraft:redstone_torch",
];

/// 四组 `(mirror, rotation)`，顺序与 Java 侧一致。
///
/// The four `(mirror, rotation)` pairs, in the same order as Java.
const PAIRS: [(Mirror, Rotation); 4] = [
    (Mirror::None, Rotation::None),
    (Mirror::LeftRight, Rotation::None),
    (Mirror::LeftRight, Rotation::Clockwise90),
    (Mirror::None, Rotation::Clockwise90),
];

pub fn run(mut command: Command, queries_path: &Path, _options: &[&str]) -> Result<(), String> {
    let queries = build_queries()?;
    fs::write(queries_path, queries.join("\n") + "\n")
        .map_err(|error| format!("cannot write queries file: {error}"))?;

    // 内存紧，给这个 java 对拍进程设上限。
    // Memory is tight; cap this java oracle process.
    command.env("JAVA_TOOL_OPTIONS", "-Xmx3g");

    let output = command
        .arg(queries_path)
        .output()
        .map_err(|error| format!("cannot run TransformOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "TransformOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare(&queries, &String::from_utf8_lossy(&output.stdout))
}

/// 每个方块的全部状态串，用 `BlockStateParser` 的语法（`minecraft:x[a=1,b=true]`）。
///
/// Every state string of every block, in `BlockStateParser` syntax.
fn build_queries() -> Result<Vec<String>, String> {
    let mut queries = Vec::new();
    for name in BLOCKS {
        let block = block_by_name(name).ok_or_else(|| format!("unknown block {name}"))?;
        let count = mc_bamboo_data::state_count(
            mc_bamboo_data::block_id_of_state(block.default_state)
                .ok_or_else(|| format!("no id for {name}"))?,
        );
        for offset in 0..count {
            let state = block.first_state + offset;
            queries.push(state_text(state));
        }
    }
    Ok(queries)
}

/// 状态 ID 的 `BlockStateParser` 形式：`minecraft:x` 加按属性顺序的 `[k=v,...]`。
///
/// A state id in `BlockStateParser` form: `minecraft:x` plus `[k=v,...]` in property order.
fn state_text(state: u32) -> String {
    let Some(id) = mc_bamboo_data::block_id_of_state(state) else {
        return "<unknown>".to_owned();
    };
    let block = &blocks::BLOCKS[id];
    if block.properties.is_empty() {
        return block.name.to_owned();
    }
    let mut stride = 1_u32;
    let mut parts = Vec::with_capacity(block.properties.len());
    for property in block.properties.iter().rev() {
        let count = property.values.len() as u32;
        let index = (state - block.first_state) / stride % count;
        parts.push(format!(
            "{}={}",
            property.name, property.values[index as usize]
        ));
        stride *= count;
    }
    parts.reverse();
    format!("{}[{}]", block.name, parts.join(","))
}

fn compare(queries: &[String], stdout: &str) -> Result<(), String> {
    // `T <pair> <input> <output>`，按输入串分组。
    // `T <pair> <input> <output>`, grouped by the input text.
    let mut theirs: std::collections::BTreeMap<(String, usize), String> =
        std::collections::BTreeMap::new();
    for line in stdout.lines() {
        let Some(rest) = line.strip_prefix("T ") else {
            continue;
        };
        let mut parts = rest.splitn(3, ' ');
        let pair: usize = parts
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| format!("bad pair in {line}"))?;
        let input = parts.next().ok_or_else(|| format!("bad input in {line}"))?;
        let output = parts
            .next()
            .ok_or_else(|| format!("bad output in {line}"))?;
        theirs.insert((input.to_owned(), pair), output.to_owned());
    }

    let mut failures: Vec<String> = Vec::new();
    let mut mismatch_count = 0usize;
    let mut comparisons = 0usize;
    let mut by_block: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();

    for text in queries {
        let state = parse_state_text(text)?;
        for (pair, (mirror, rotation)) in PAIRS.iter().enumerate() {
            comparisons += 1;
            let ours = state_transform::transform(state, *mirror, *rotation);
            match theirs.get(&(text.clone(), pair)) {
                Some(expected) if expected == &block_state_name(ours) => {}
                Some(expected) => {
                    mismatch_count += 1;
                    let block = text.split('[').next().unwrap_or(text);
                    *by_block.entry(block.to_owned()).or_default() += 1;
                    if failures.len() < 40 {
                        failures.push(format!(
                            "pair {pair} {text}: java {expected}, rust {}",
                            block_state_name(ours)
                        ));
                    }
                }
                None => {
                    return Err(format!("oracle did not answer for {text} pair {pair}"));
                }
            }
        }
    }

    println!(
        "transform: {} states, {comparisons} comparisons",
        queries.len()
    );
    if mismatch_count == 0 {
        println!("TRANSFORM RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in &failures {
            println!("TRANSFORM FAIL: {line}");
        }
        println!("-- by block --");
        for (block, count) in &by_block {
            println!("  {count:5} {block}");
        }
        Err(format!("TRANSFORM RESULT: {mismatch_count} mismatches"))
    }
}

/// 状态 ID 的 `BlockState.toString()` 形式：`Block{minecraft:x}[k=v,...]`，和 Java 一致。
///
/// A state id in `BlockState.toString()` form, matching Java.
fn block_state_name(state: u32) -> String {
    let Some(id) = mc_bamboo_data::block_id_of_state(state) else {
        return format!("<state {state}>");
    };
    let block = &blocks::BLOCKS[id];
    let mut out = format!("Block{{{}}}", block.name);
    if block.properties.is_empty() {
        return out;
    }
    let mut stride = 1_u32;
    let mut parts = Vec::with_capacity(block.properties.len());
    for property in block.properties.iter().rev() {
        let count = property.values.len() as u32;
        let index = (state - block.first_state) / stride % count;
        parts.push(format!(
            "{}={}",
            property.name, property.values[index as usize]
        ));
        stride *= count;
    }
    parts.reverse();
    out.push('[');
    out.push_str(&parts.join(","));
    out.push(']');
    out
}

/// 把 `state_text` 的输出形式解析回状态 ID（自己生成的，格式已知）。
///
/// Parse `state_text`'s output back into a state id.
fn parse_state_text(text: &str) -> Result<u32, String> {
    let (name, props) = match text.split_once('[') {
        Some((name, rest)) => (name, Some(rest.trim_end_matches(']'))),
        None => (text, None),
    };
    let block = block_by_name(name).ok_or_else(|| format!("unknown block {name}"))?;
    let mut state = block.default_state;
    if let Some(props) = props {
        for entry in props.split(',') {
            let (key, value) = entry
                .split_once('=')
                .ok_or_else(|| format!("bad property in {text}"))?;
            state = mc_bamboo_worldgen::structure::piece_place::with_property(state, key, value)
                .ok_or_else(|| format!("cannot set {key}={value} in {text}"))?;
        }
    }
    Ok(state)
}
