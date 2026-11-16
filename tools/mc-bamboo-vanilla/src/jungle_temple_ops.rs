//! 从官方 `JungleTemplePiece.postProcess` 的 helper 调用序列生成丛林神殿的指令表。
//!
//! `java/JungleTempleOpsExtractor.java` 用子类拦截 `generateBox` / `generateAirBox` /
//! `placeBlock` / `createChest` / `createDispenser`，输出调用序列；这里把它写成
//! `mc-bamboo-worldgen` 的 `ops()` 函数，再交给 `rustfmt` 排版。导出的是调用序列，
//! 不是官方源码。
//!
//! Generate the jungle temple instruction table from vanilla's
//! `JungleTemplePiece.postProcess` helper calls. `JungleTempleOpsExtractor.java` intercepts
//! the paint helpers with a subclass; here we write them out as the `ops()` function and
//! let `rustfmt` lay it out.

use std::{fmt::Write as _, fs, path::Path, process::Command};

use serde::Deserialize;

const HEADER: &str = "// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。\n\n";
const OUTPUT: &str = "crates/mc-bamboo-worldgen/src/structure/post_process/jungle_temple_ops.rs";

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Call {
    Moss {
        from: [i32; 3],
        to: [i32; 3],
        #[serde(rename = "skipAir")]
        skip_air: bool,
    },
    Air {
        from: [i32; 3],
        to: [i32; 3],
    },
    One {
        state: String,
        at: [i32; 3],
    },
    Chest {
        at: [i32; 3],
        table: String,
    },
    Dispenser {
        at: [i32; 3],
        facing: String,
        table: String,
    },
}

/// 跑 Java 提取器，把指令表写进 `mc-bamboo-worldgen`。
///
/// Run the Java extractor and write the instruction table into `mc-bamboo-worldgen`.
pub fn export(java: &str, work_dir: &Path, workspace_root: &Path) -> Result<(), String> {
    let source = workspace_root.join("tools/mc-bamboo-vanilla/java/JungleTempleOpsExtractor.java");
    let report = work_dir.join("jungle_temple_ops.json");
    let output = Command::new(java)
        .current_dir(work_dir)
        .arg("-cp")
        .arg(crate::extract::classpath(work_dir)?)
        .arg(&source)
        .arg(&report)
        .output()
        .map_err(|error| format!("无法运行 {java}：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "JungleTempleOpsExtractor.java 退出失败：{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = fs::read_to_string(&report)
        .map_err(|error| format!("无法读取 {}：{error}", report.display()))?;
    let calls: Vec<Call> = serde_json::from_str(&text)
        .map_err(|error| format!("丛林神殿调用序列解析失败：{error}"))?;
    let content = ops_file(&calls)?;
    let path = workspace_root.join(OUTPUT);
    fs::write(&path, format!("{HEADER}{content}"))
        .map_err(|error| format!("无法写入 {}：{error}", path.display()))?;
    // 生成的是 Rust 源码，排版交给仓库同一个 rustfmt，重跑才是逐字节无 diff。
    // Generated Rust source: let the same rustfmt as the repository format it so reruns
    // are byte-identical.
    let status = Command::new("rustfmt")
        .arg("--edition")
        .arg("2024")
        .arg(&path)
        .status()
        .map_err(|error| format!("无法运行 rustfmt：{error}"))?;
    if !status.success() {
        return Err(format!("rustfmt 排版 {} 失败：{status}", path.display()));
    }
    println!("exported {} jungle temple instructions", calls.len() - 2);
    Ok(())
}

/// 生成 `ops()`：一个 `include!` 进 `jungle_temple` 模块的函数。
fn ops_file(calls: &[Call]) -> Result<String, String> {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "/// 官方 `postProcess` 的全部绘制指令，顺序照调用序列。"
    );
    let _ = writeln!(out, "///");
    let _ = writeln!(
        out,
        "/// Every paint instruction of vanilla `postProcess`, in call order."
    );
    let _ = writeln!(out, "fn ops() -> Vec<Op> {{");
    let _ = writeln!(out, "    vec![");
    let mut index = 0usize;
    let mut merged = 0usize;
    while index < calls.len() {
        let call = &calls[index];
        // `createDispenser` 内部的 `placeBlock` 也进报告， Bamboo 把它归入同一条指令。
        // createDispenser's internal placeBlock is in the report too; Bamboo keeps one op.
        if let Call::Dispenser {
            at,
            facing: own_facing,
            table,
        } = call
        {
            let Some(Call::One { state, at: inner }) = calls.get(index + 1) else {
                return Err(format!("第 {index} 条发射器后面没有 placeBlock"));
            };
            if at != inner {
                return Err(format!("第 {index} 条发射器的 placeBlock 坐标不同"));
            }
            let (_, props) = parse_state(state)?;
            let facing = props
                .iter()
                .find(|(name, _)| name == "facing")
                .map(|(_, value)| value.as_str())
                .ok_or_else(|| format!("发射器状态没有 facing：{state}"))?;
            if facing != own_facing {
                return Err(format!(
                    "第 {index} 条发射器朝向不一致：{facing} / {own_facing}"
                ));
            }
            let _ = writeln!(out, "        Op::Dispenser {{");
            let _ = writeln!(out, "            at: ({}, {}, {}),", at[0], at[1], at[2]);
            let _ = writeln!(out, "            facing: {own_facing:?},");
            let _ = writeln!(out, "            table: {table:?},");
            let _ = writeln!(out, "        }},");
            merged += 1;
            index += 2;
            continue;
        }
        index += 1;
        match call {
            Call::Moss { from, to, skip_air } => {
                let _ = writeln!(out, "        Op::Moss {{");
                let _ = writeln!(
                    out,
                    "            from: ({}, {}, {}),",
                    from[0], from[1], from[2]
                );
                let _ = writeln!(out, "            to: ({}, {}, {}),", to[0], to[1], to[2]);
                let _ = writeln!(out, "            skip_air: {skip_air},");
                let _ = writeln!(out, "        }},");
            }
            Call::Air { from, to } => {
                let _ = writeln!(out, "        Op::Air {{");
                let _ = writeln!(
                    out,
                    "            from: ({}, {}, {}),",
                    from[0], from[1], from[2]
                );
                let _ = writeln!(out, "            to: ({}, {}, {}),", to[0], to[1], to[2]);
                let _ = writeln!(out, "        }},");
            }
            Call::One { state, at } => {
                let (block, props) = parse_state(state)?;
                let _ = writeln!(out, "        Op::One {{");
                if props.is_empty() {
                    let _ = writeln!(out, "            state: blocks::{block},");
                } else {
                    let pairs = props
                        .iter()
                        .map(|(name, value)| format!("({name:?}, {value:?})"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let _ = writeln!(
                        out,
                        "            state: state_with(blocks::{block}, &[{pairs}]),"
                    );
                }
                let _ = writeln!(out, "            at: ({}, {}, {}),", at[0], at[1], at[2]);
                let _ = writeln!(out, "        }},");
            }
            Call::Chest { at, table } => {
                let _ = writeln!(out, "        Op::Chest {{");
                let _ = writeln!(out, "            at: ({}, {}, {}),", at[0], at[1], at[2]);
                let _ = writeln!(out, "            table: {table:?},");
                let _ = writeln!(out, "        }},");
            }
            Call::Dispenser { .. } => unreachable!(),
        }
    }
    let _ = writeln!(out, "    ]");
    let _ = writeln!(out, "}}");
    if merged != 2 {
        return Err(format!(
            "报告里应有两只发射器的内部 placeBlock，实际 {merged} 条"
        ));
    }
    Ok(out)
}

/// `minecraft:x[k=v,...]` → (`blocks::X` 的常量名, 属性列表)。
fn parse_state(state: &str) -> Result<(String, Vec<(String, String)>), String> {
    let (name, rest) = state
        .split_once('[')
        .ok_or_else(|| format!("状态没有属性表：{state}"))?;
    let block = name
        .strip_prefix("minecraft:")
        .ok_or_else(|| format!("状态名字没有命名空间：{state}"))?
        .to_ascii_uppercase();
    let props = rest
        .strip_suffix(']')
        .ok_or_else(|| format!("状态属性表没有收尾：{state}"))?;
    let mut out = Vec::new();
    if !props.is_empty() {
        for pair in props.split(',') {
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| format!("状态属性不是 k=v：{pair}"))?;
            out.push((key.to_owned(), value.to_owned()));
        }
    }
    Ok((block, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_block_states_in_official_property_order() {
        let (block, props) = parse_state(
            "minecraft:redstone_wire[east=none,north=side,power=0,south=side,west=none]",
        )
        .unwrap();
        assert_eq!(block, "REDSTONE_WIRE");
        assert_eq!(
            props,
            vec![
                ("east".to_owned(), "none".to_owned()),
                ("north".to_owned(), "side".to_owned()),
                ("power".to_owned(), "0".to_owned()),
                ("south".to_owned(), "side".to_owned()),
                ("west".to_owned(), "none".to_owned()),
            ]
        );
        assert_eq!(
            parse_state("minecraft:air[]").unwrap(),
            ("AIR".to_owned(), vec![])
        );
        assert!(parse_state("stone").is_err());
    }
}
