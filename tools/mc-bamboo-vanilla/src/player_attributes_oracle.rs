//! 玩家属性系统、挖掘速度与属性同步包的官方对拍。
//!
//! `PlayerAttributesOracle.java` 直接调用官方 26.3 的真实 `Player.createAttributes()`、
//! `AttributeMap`、`ItemStack.forEachModifier`（含 `EnchantmentHelper.forEachModifier`）、
//! `MobEffect.addAttributeModifiers`、`Player.getDestroySpeed(BlockState)` 与
//! `ClientboundUpdateAttributesPacket.STREAM_CODEC`：
//! 1. 随机 500 组装备（含附魔 + 自定义 `attribute_modifiers`）、药水效果、游戏模式、
//!    疾跑 / 细雪 / 潜行，逐项比对全部 36 个属性的最终值（f64 原始位）与修饰符列表；
//! 2. 随机 4000 组工具、效率、急迫 / 潮涌能量 / 挖掘疲劳、水下速掘、水中 / 腾空、
//!    自定义 `block_break_speed`、方块，逐位比对 `Player.getDestroySpeed`；
//! 3. 双向线上解码并比对进服全量、速度 II 脏同步、效率 V + 创造 + 疾跑脏同步三份
//!    `UpdateAttributes` 包；
//! 4. `--break-self-test` 跑 4 处改坏确认（效率附魔、挖掘疲劳、漏发脏同步包、修饰符运算类型）。
//!
//! Official 26.3 oracle for player attributes, real `Player.getDestroySpeed`,
//! and `ClientboundUpdateAttributesPacket` wire parity, including four
//! deliberate break-it checks.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use mc_bamboo_blocks::mining::{Miner, destroy_speed};
use mc_bamboo_data::registries::mob_effect;
use mc_bamboo_game::{
    ReplayAttrEntry, ReplayAttrScene, ReplayAttrSlot, replay_player_attributes,
    replay_speed_sync_packets,
};
use serde_json::{Value, json};

use crate::oracle::collect_log;

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).copied().map_or(0, u32::from);
        let b2 = chunk.get(2).copied().map_or(0, u32::from);
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((triple >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

const USAGE: &str = "usage: player-attributes <java> [--cases N] [--mining-cases N] [--seed S] [--show N] [--break-self-test]";
const DEFAULT_ATTR_CASES: usize = 500;
const DEFAULT_MINING_CASES: usize = 4000;
const DEFAULT_SEED: i64 = 2026_0326;
const DEFAULT_SHOW: usize = 10;

struct Options {
    attr_cases: usize,
    mining_cases: usize,
    seed: i64,
    show: usize,
    break_self_test: bool,
}

fn parse_options(args: &[&str]) -> Result<Options, String> {
    let mut opts = Options {
        attr_cases: DEFAULT_ATTR_CASES,
        mining_cases: DEFAULT_MINING_CASES,
        seed: DEFAULT_SEED,
        show: DEFAULT_SHOW,
        break_self_test: false,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--cases" => {
                let raw = args.get(i + 1).ok_or(USAGE)?;
                opts.attr_cases = raw.parse().map_err(|_| USAGE.to_owned())?;
                i += 2;
            }
            "--mining-cases" => {
                let raw = args.get(i + 1).ok_or(USAGE)?;
                opts.mining_cases = raw.parse().map_err(|_| USAGE.to_owned())?;
                i += 2;
            }
            "--seed" => {
                let raw = args.get(i + 1).ok_or(USAGE)?;
                opts.seed = raw.parse().map_err(|_| USAGE.to_owned())?;
                i += 2;
            }
            "--show" => {
                let raw = args.get(i + 1).ok_or(USAGE)?;
                opts.show = raw.parse().map_err(|_| USAGE.to_owned())?;
                i += 2;
            }
            "--break-self-test" => {
                opts.break_self_test = true;
                i += 1;
            }
            _ => return Err(USAGE.to_owned()),
        }
    }
    Ok(opts)
}

fn exchange(
    stdin: &mut impl Write,
    stdout: &mut BufReader<std::process::ChildStdout>,
    request: &Value,
    logs: &std::sync::mpsc::Receiver<Vec<String>>,
) -> Result<Value, String> {
    writeln!(stdin, "{request}").map_err(|e| format!("write java stdin: {e}"))?;
    stdin
        .flush()
        .map_err(|e| format!("flush java stdin: {e}"))?;
    let mut line = String::new();
    let n = stdout
        .read_line(&mut line)
        .map_err(|e| format!("read java stdout: {e}"))?;
    if n == 0 {
        let stderr = logs.recv().unwrap_or_default().join("\n");
        return Err(format!("java oracle exited unexpectedly:\n{stderr}"));
    }
    let response: Value = serde_json::from_str(&line).map_err(|e| format!("bad java json: {e}"))?;
    if let Some(err) = response.get("error").and_then(Value::as_str) {
        return Err(format!("java oracle error: {err}"));
    }
    Ok(response)
}

fn parse_attr_cases(
    cases: &[Value],
) -> Result<Vec<(ReplayAttrScene, Vec<ReplayAttrEntry>)>, String> {
    let mut out = Vec::with_capacity(cases.len());
    for row in cases {
        let mut slots = Vec::new();
        for s in row["slots"].as_array().ok_or("missing slots")? {
            let enchantments: Vec<(i32, i32)> = s["enchantments"]
                .as_array()
                .ok_or("missing enchantments")?
                .iter()
                .map(|pair| {
                    let a = pair.as_array().ok_or("bad enchant pair")?;
                    Ok((
                        a[0].as_i64().ok_or("bad ench id")? as i32,
                        a[1].as_i64().ok_or("bad ench level")? as i32,
                    ))
                })
                .collect::<Result<_, String>>()?;
            let custom_modifiers = match s.get("customModifiers").and_then(Value::as_array) {
                Some(arr) => {
                    let mut mods = Vec::with_capacity(arr.len());
                    for item in arr {
                        let a = item.as_array().ok_or("bad custom mod")?;
                        mods.push((
                            a[0].as_i64().ok_or("bad attr id")? as i32,
                            a[1].as_str().ok_or("bad mod id")?.to_owned(),
                            f64::from_bits(a[2].as_i64().ok_or("bad amount bits")? as u64),
                            a[3].as_i64().ok_or("bad op")? as i32,
                            a[4].as_i64().ok_or("bad group")? as i32,
                        ));
                    }
                    Some(mods)
                }
                None => None,
            };
            slots.push(ReplayAttrSlot {
                slot: s["slot"].as_u64().ok_or("bad slot")? as u8,
                item: s["item"].as_i64().ok_or("bad item")? as i32,
                enchantments,
                custom_modifiers,
            });
        }
        let effects: Vec<(i32, i32)> = row["effects"]
            .as_array()
            .ok_or("missing effects")?
            .iter()
            .map(|pair| {
                let a = pair.as_array().ok_or("bad effect pair")?;
                Ok((
                    a[0].as_i64().ok_or("bad effect id")? as i32,
                    a[1].as_i64().ok_or("bad effect amp")? as i32,
                ))
            })
            .collect::<Result<_, String>>()?;
        let scene = ReplayAttrScene {
            game_mode: row["gameMode"].as_u64().ok_or("bad gameMode")? as u8,
            sprinting: row["sprinting"].as_bool().ok_or("bad sprinting")?,
            crouching: row["crouching"].as_bool().ok_or("bad crouching")?,
            ticks_frozen: row["ticksFrozen"].as_i64().ok_or("bad ticksFrozen")? as i32,
            on_solid_for_frost: row["onSolidForFrost"]
                .as_bool()
                .ok_or("bad onSolidForFrost")?,
            slots,
            effects,
            break_mode: 0,
        };
        let mut expected = Vec::new();
        for attr in row["expected"].as_array().ok_or("missing expected")? {
            let a = attr.as_array().ok_or("bad expected attr")?;
            let id = a[0].as_i64().ok_or("bad attr id")? as i32;
            let bits = a[1].as_i64().ok_or("bad value bits")? as u64;
            let mut mods = Vec::new();
            for m in a[2].as_array().ok_or("bad mods")? {
                let ma = m.as_array().ok_or("bad mod entry")?;
                mods.push((
                    ma[0].as_str().ok_or("bad mod id")?.to_owned(),
                    ma[1].as_i64().ok_or("bad mod amount")? as u64,
                    ma[2].as_i64().ok_or("bad mod op")? as i32,
                ));
            }
            expected.push((id, bits, mods));
        }
        out.push((scene, expected));
    }
    Ok(out)
}

fn check_attribute_cases(
    cases: &[(ReplayAttrScene, Vec<ReplayAttrEntry>)],
    break_mode: u8,
) -> (usize, Vec<String>) {
    let mut checks = 0;
    let mut failures = Vec::new();
    for (idx, (base_scene, expected)) in cases.iter().enumerate() {
        let mut scene = base_scene.clone();
        scene.break_mode = break_mode;
        let got = replay_player_attributes(&scene);
        checks += 1;
        if got.attributes.len() != expected.len() {
            failures.push(format!(
                "attr case {idx}: count mismatch bamboo={} vanilla={}",
                got.attributes.len(),
                expected.len()
            ));
            continue;
        }
        for (b, v) in got.attributes.iter().zip(expected.iter()) {
            checks += 1;
            // 修饰符列表必须逐位完全一致；同一运算类型下有 >=2 个修饰符时，
            // 官方 `Object2ObjectOpenHashMap` 的哈希遍历顺序可能带来 1 ULP 浮点舍入差。
            let mut op_counts = [0usize; 3];
            for (_, _, op) in &v.2 {
                if let Some(slot) = op_counts.get_mut(*op as usize) {
                    *slot += 1;
                }
            }
            let same_op_multi = op_counts.iter().any(|&n| n >= 2);
            let value_ok = if b.1 == v.1 {
                true
            } else if same_op_multi {
                let bf = f64::from_bits(b.1);
                let vf = f64::from_bits(v.1);
                bf.is_finite()
                    && vf.is_finite()
                    && bf.is_sign_positive() == vf.is_sign_positive()
                    && b.1.abs_diff(v.1) <= 1
            } else {
                false
            };
            if b.0 != v.0 || !value_ok || b.2 != v.2 {
                failures.push(format!(
                    "attr case {idx} attr {}: bamboo=({:.15} / 0x{:016x}, {:?}) vanilla=({:.15} / 0x{:016x}, {:?})",
                    v.0,
                    f64::from_bits(b.1),
                    b.1,
                    b.2,
                    f64::from_bits(v.1),
                    v.1,
                    v.2
                ));
            }
        }
    }
    (checks, failures)
}

fn check_mining_cases(
    cases: &[Value],
    efficiency_id: i32,
    aqua_affinity_id: i32,
    break_efficiency: bool,
    break_fatigue: bool,
) -> Result<(usize, Vec<String>), String> {
    let mut checks = 0;
    let mut failures = Vec::new();
    for (idx, row) in cases.iter().enumerate() {
        let a = row.as_array().ok_or("bad mining row")?;
        let tool = a[0].as_i64().ok_or("bad tool")? as i32;
        let state_id = a[1].as_i64().ok_or("bad state")? as u32;
        let eff_level = a[2].as_i64().ok_or("bad eff")? as i32;
        let has_aqua = a[3].as_i64().ok_or("bad aqua")? != 0;
        let haste_amp = a[4].as_i64().ok_or("bad haste")? as i32;
        let conduit_amp = a[5].as_i64().ok_or("bad conduit")? as i32;
        let fatigue_amp = a[6].as_i64().ok_or("bad fatigue")? as i32;
        let eye_in_water = a[7].as_i64().ok_or("bad water")? != 0;
        let on_ground = a[8].as_i64().ok_or("bad ground")? != 0;
        let break_speed_base = f64::from_bits(a[9].as_i64().ok_or("bad break_speed")? as u64);
        let expected_bits = a[10].as_i64().ok_or("bad expected speed")? as i32 as u32;

        let mut slots = Vec::new();
        if tool != 0 {
            let mut ench = Vec::new();
            if eff_level > 0 {
                ench.push((efficiency_id, eff_level));
            }
            slots.push(ReplayAttrSlot {
                slot: 0,
                item: tool,
                enchantments: ench,
                custom_modifiers: None,
            });
        }
        if has_aqua {
            slots.push(ReplayAttrSlot {
                slot: 5,
                item: mc_bamboo_data::items::DIAMOND_HELMET,
                enchantments: vec![(aqua_affinity_id, 1)],
                custom_modifiers: None,
            });
        }
        let mut effects = Vec::new();
        if haste_amp >= 0 {
            effects.push((mob_effect::HASTE, haste_amp));
        }
        if conduit_amp >= 0 {
            effects.push((mob_effect::CONDUIT_POWER, conduit_amp));
        }
        if fatigue_amp >= 0 {
            effects.push((mob_effect::MINING_FATIGUE, fatigue_amp));
        }
        let scene = ReplayAttrScene {
            game_mode: 0,
            sprinting: false,
            crouching: false,
            ticks_frozen: 0,
            on_solid_for_frost: false,
            slots,
            effects,
            break_mode: 0,
        };
        let resolved = replay_player_attributes(&scene);
        // `break_speed_base` 覆盖 BLOCK_BREAK_SPEED 基础值（Java 端通过 setBaseValue 设置）。
        let block_break_speed = (break_speed_base
            * (f64::from(resolved.block_break_speed)
                / mc_bamboo_data::registries::attribute_defaults::BLOCK_BREAK_SPEED))
            as f32;
        let mut miner = Miner {
            main_hand: tool,
            on_ground,
            eye_in_water,
            mining_efficiency: if break_efficiency {
                0.0
            } else {
                resolved.mining_efficiency
            },
            block_break_speed,
            submerged_mining_speed: resolved.submerged_mining_speed,
            has_dig_speed: resolved.has_dig_speed,
            dig_speed_amplification: resolved.dig_speed_amplification,
            mining_fatigue: if break_fatigue {
                None
            } else {
                resolved.mining_fatigue
            },
        };
        if break_efficiency && eff_level > 0 {
            miner.mining_efficiency = 0.0;
        }
        let got = destroy_speed(state_id, miner);
        checks += 1;
        if got.to_bits() != expected_bits {
            failures.push(format!(
                "mining case {idx}: tool={tool} state={state_id} eff={eff_level} aqua={has_aqua} haste={haste_amp} conduit={conduit_amp} fatigue={fatigue_amp} water={eye_in_water} ground={on_ground} base={break_speed_base}: bamboo={} (0x{:08x}) vanilla={} (0x{:08x})",
                got,
                got.to_bits(),
                f32::from_bits(expected_bits),
                expected_bits
            ));
        }
    }
    Ok((checks, failures))
}

fn check_sync_packets(
    stdin: &mut impl Write,
    stdout: &mut BufReader<std::process::ChildStdout>,
    logs: &std::sync::mpsc::Receiver<Vec<String>>,
    omit_dirty: bool,
) -> Result<(usize, Vec<String>), String> {
    let (full, speed, complex) = replay_speed_sync_packets(omit_dirty);
    let resp = exchange(
        stdin,
        stdout,
        &json!({
            "command": "sync-packets",
            "full": encode_base64(&full),
            "speedDirty": encode_base64(&speed),
            "complexDirty": encode_base64(&complex),
        }),
        logs,
    )?;
    let mut checks = 0;
    let mut failures = Vec::new();
    for key in ["full", "speedDirty", "complexDirty"] {
        checks += 1;
        let section = &resp[key];
        let equal = section["equal"].as_bool().unwrap_or(false);
        if !equal {
            let err = section
                .get("decodeError")
                .and_then(Value::as_str)
                .unwrap_or("snapshot mismatch");
            failures.push(format!(
                "sync packet {key} mismatch ({err}): bamboo={} vanilla={}",
                section["bamboo"], section["vanilla"]
            ));
        }
    }
    Ok((checks, failures))
}

pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let opts = parse_options(args)?;
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run java: {e}"))?;
    let logs = collect_log(&mut child)?;
    let stdout_pipe = child.stdout.take().ok_or("java has no stdout")?;
    let mut stdout = BufReader::new(stdout_pipe);
    let mut stdin = child.stdin.take().ok_or("java has no stdin")?;

    let attr_resp = exchange(
        &mut stdin,
        &mut stdout,
        &json!({
            "command": "attributes",
            "cases": opts.attr_cases,
            "seed": opts.seed,
        }),
        &logs,
    )?;
    let attr_cases = parse_attr_cases(
        attr_resp["cases"]
            .as_array()
            .ok_or("missing attributes cases")?,
    )?;

    let mining_resp = exchange(
        &mut stdin,
        &mut stdout,
        &json!({
            "command": "mining-speed",
            "cases": opts.mining_cases,
            "seed": opts.seed.wrapping_add(1),
        }),
        &logs,
    )?;
    let mining_cases = mining_resp["cases"]
        .as_array()
        .ok_or("missing mining cases")?;

    let enchantment_id = |name: &str| -> i32 {
        mc_bamboo_data::registries::SYNCED_REGISTRIES
            .iter()
            .find(|r| r.name == "minecraft:enchantment")
            .and_then(|r| r.entries.iter().position(|e| *e == name))
            .expect("enchantment exists") as i32
    };
    let efficiency_id = enchantment_id("minecraft:efficiency");
    let aqua_affinity_id = enchantment_id("minecraft:aqua_affinity");

    let (attr_checks, attr_failures) = check_attribute_cases(&attr_cases, 0);
    let (mining_checks, mining_failures) =
        check_mining_cases(mining_cases, efficiency_id, aqua_affinity_id, false, false)?;
    let (sync_checks, sync_failures) = check_sync_packets(&mut stdin, &mut stdout, &logs, false)?;

    let total_checks = attr_checks + mining_checks + sync_checks;
    let mut all_failures = Vec::new();
    all_failures.extend(attr_failures);
    all_failures.extend(mining_failures);
    all_failures.extend(sync_failures);

    if opts.break_self_test {
        let mut break_results = Vec::new();
        // 1. 改坏效率附魔（MINING_EFFICIENCY 偏 1 + 忽略效率加成）。
        let (_, b_eff_attr) = check_attribute_cases(&attr_cases, 1);
        let (_, b_eff_mine) =
            check_mining_cases(mining_cases, efficiency_id, aqua_affinity_id, true, false)?;
        break_results.push((
            "efficiency",
            !b_eff_attr.is_empty() && !b_eff_mine.is_empty(),
            format!(
                "attr_failures={}, mining_failures={}",
                b_eff_attr.len(),
                b_eff_mine.len()
            ),
        ));
        // 2. 改坏挖掘疲劳（忽略 MINING_FATIGUE 衰减）。
        let (_, b_fat) =
            check_mining_cases(mining_cases, efficiency_id, aqua_affinity_id, false, true)?;
        break_results.push((
            "mining-fatigue",
            !b_fat.is_empty(),
            format!("mining_failures={}", b_fat.len()),
        ));
        // 3. 改坏属性同步（漏发脏 UpdateAttributes 包）。
        let (_, b_sync) = check_sync_packets(&mut stdin, &mut stdout, &logs, true)?;
        break_results.push((
            "sync-omit",
            !b_sync.is_empty(),
            format!("sync_failures={}", b_sync.len()),
        ));
        // 4. 改坏修饰符运算类型（AddMultipliedTotal -> AddMultipliedBase）。
        let (_, b_op) = check_attribute_cases(&attr_cases, 2);
        break_results.push((
            "modifier-op",
            !b_op.is_empty(),
            format!("attr_failures={}", b_op.len()),
        ));

        drop(stdin);
        let status = child.wait().map_err(|e| format!("wait java: {e}"))?;
        let _ = logs.recv();
        if !status.success() {
            return Err(format!("java exited with {status}"));
        }
        let mut failed_breaks = Vec::new();
        for (name, caught, detail) in &break_results {
            println!("  break-self-test [{name}]: caught={caught} ({detail})");
            if !caught {
                failed_breaks.push(*name);
            }
        }
        if !all_failures.is_empty() {
            return Err(format!(
                "player-attributes baseline had {} unexpected failures before break-self-test",
                all_failures.len()
            ));
        }
        if !failed_breaks.is_empty() {
            return Err(format!(
                "break-self-test missed deliberate breaks: {failed_breaks:?}"
            ));
        }
        println!(
            "player-attributes oracle (--break-self-test): all {} deliberate breaks caught; baseline {total_checks} checks passed ({} attr cases, {} mining cases, {sync_checks} sync packets)",
            break_results.len(),
            opts.attr_cases,
            opts.mining_cases
        );
        return Ok(());
    }

    drop(stdin);
    let status = child.wait().map_err(|e| format!("wait java: {e}"))?;
    let _ = logs.recv();
    if !status.success() {
        return Err(format!("java exited with {status}"));
    }

    for failure in all_failures.iter().take(opts.show.max(1)) {
        eprintln!("  {failure}");
    }
    if all_failures.is_empty() {
        println!(
            "player-attributes oracle: {total_checks} checks passed ({} attr cases / {attr_checks} checks, {} real getDestroySpeed cases / {mining_checks} checks, {sync_checks} sync packets)",
            opts.attr_cases, opts.mining_cases
        );
        Ok(())
    } else {
        Err(format!(
            "player-attributes oracle: {} of {total_checks} checks failed",
            all_failures.len()
        ))
    }
}
