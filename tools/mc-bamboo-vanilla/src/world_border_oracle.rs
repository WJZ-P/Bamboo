//! 用官方 26.3 的 `WorldBorder` 检查 Bamboo 的边界数学、渐变和六个包。
//!
//! `WorldBorderOracle.java` 在 JVM 里直接调官方类，不启动服务端：随机的中心、大小、绝对
//! 最大值和渐变，配上随机点，逐项给出距离、`isWithinBounds` 的几种重载、`clampToBounds`、
//! `isInsideCloseToBorder`、`Shapes.collide` 的结果、逐刻渐变后的状态和六个包的官方编码。
//! 逐刻渐变时还给出四条边两侧方块的 `isWithinBounds(BlockPos)`（`LevelChunk.isTicking`
//! 的边界判断），和方块实体阶段用的整数方块盒逐格比较。
//! 这里用同一组输入重放 `mc_bamboo_blocks::world_border`，逐位比较。
//!
//! Check Bamboo's border maths, lerp and six packets against vanilla 26.3 `WorldBorder`.
//! `WorldBorderOracle.java` calls the official class in a JVM without starting a server;
//! this module replays the same inputs and compares every value bit for bit.

use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_blocks::{BorderStatus, WorldBorder, entity::collide_border_only};
use mc_bamboo_core::{Aabb, BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::version;
use mc_bamboo_protocol::packet::{
    ClientboundPacket,
    play::{
        InitializeBorder, SetBorderCenter, SetBorderLerpSize, SetBorderSize, SetBorderWarningDelay,
        SetBorderWarningDistance,
    },
};
use serde_json::Value;

use crate::oracle::collect_log;

const USAGE: &str = "usage: border-oracle <java> [--cases N] [--seed S] [--show N]";
const DEFAULT_SHOW: usize = 5;

#[derive(Debug, Default)]
struct Options {
    forwarded: Vec<String>,
    show: usize,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            show: DEFAULT_SHOW,
            ..Self::default()
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--cases" | "--seed" => {
                    let value = args.next().ok_or(USAGE)?;
                    options
                        .forwarded
                        .extend([arg.to_owned(), (*value).to_owned()]);
                }
                "--show" => {
                    let value = args.next().ok_or(USAGE)?;
                    options.show = value.parse().map_err(|_| USAGE)?;
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

#[derive(Debug, Default)]
struct Report {
    cases: usize,
    checks: usize,
    mismatches: usize,
    shown: usize,
    show: usize,
}

impl Report {
    fn compare(&mut self, case: usize, what: &str, expected: &Value, actual: Value) {
        self.checks += 1;
        if &actual == expected {
            return;
        }
        self.mismatches += 1;
        if self.shown < self.show {
            self.shown += 1;
            eprintln!("case {case} {what}: official {expected}, Bamboo {actual}");
        }
    }

    fn bits(&mut self, case: usize, what: &str, expected: &Value, actual: f64) {
        let actual = serde_json::json!(actual);
        self.compare(case, what, expected, actual);
    }

    fn flag(&mut self, case: usize, what: &str, expected: &Value, actual: bool) {
        self.compare(case, what, expected, Value::Bool(actual));
    }

    fn ints(&mut self, case: usize, what: &str, expected: &Value, actual: [f64; 3]) {
        self.compare(case, what, expected, serde_json::json!(actual));
    }
}

pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let mut child = command
        .args(&options.forwarded)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut report = Report {
        show: options.show,
        ..Report::default()
    };
    let read = read_lines(stdout, &mut report);
    if read.is_err() {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    if !status.success() {
        return Err(format!("WorldBorderOracle.java failed: {status}\n{log}"));
    }
    let ended = read.map_err(|error| format!("{error}\n{log}"))?;
    println!(
        "border oracle: {} cases, {} checks, {} mismatches",
        report.cases, report.checks, report.mismatches
    );
    if !ended {
        return Err("WorldBorderOracle.java did not write the end line".to_owned());
    }
    if report.cases == 0 {
        return Err("the official side produced no cases".to_owned());
    }
    if report.mismatches == 0 {
        Ok(())
    } else {
        Err(format!("{} values differ from vanilla", report.mismatches))
    }
}

fn read_lines(source: impl Read, report: &mut Report) -> Result<bool, String> {
    let mut header_ok = false;
    let mut ended = false;
    for line in BufReader::new(source).lines() {
        let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
        if !line.starts_with('{') {
            continue;
        }
        let value: Value =
            serde_json::from_str(&line).map_err(|error| format!("bad oracle line: {error}"))?;
        match value["type"].as_str() {
            Some("header") => {
                if value["version"].as_str() != Some(version::GAME_VERSION)
                    || value["protocol"].as_i64() != Some(i64::from(version::PROTOCOL_VERSION))
                {
                    return Err(format!(
                        "official is {} / {}, Bamboo is {} / {}",
                        value["version"],
                        value["protocol"],
                        version::GAME_VERSION,
                        version::PROTOCOL_VERSION
                    ));
                }
                header_ok = true;
            }
            Some("case") => {
                if !header_ok {
                    return Err("a case arrived before the header".to_owned());
                }
                check_case(report, &value)?;
            }
            Some("end") => {
                ended = true;
                let expected = value["cases"].as_u64().unwrap_or(0) as usize;
                if expected != report.cases {
                    return Err(format!(
                        "java wrote {expected} cases, {} arrived",
                        report.cases
                    ));
                }
            }
            other => return Err(format!("unknown line type: {other:?}")),
        }
    }
    Ok(ended)
}

fn check_case(report: &mut Report, case: &Value) -> Result<(), String> {
    let index = case["index"].as_u64().unwrap_or(0) as usize;
    report.cases += 1;
    let mut border = WorldBorder::default();
    border.set_center(number(case, "center_x")?, number(case, "center_z")?);
    border.set_absolute_max_size(integer(case, "absolute_max_size")?);
    border.set_damage_per_block(number(case, "damage_per_block")?);
    border.set_safe_zone(number(case, "safe_zone")?);
    border.set_warning_blocks(integer(case, "warning_blocks")?);
    border.set_warning_time(integer(case, "warning_time")?);
    if case["lerp"].as_bool().unwrap_or(false) {
        border.lerp_size_between(
            number(case, "from")?,
            number(case, "to")?,
            long(case, "ticks")?,
            long(case, "game_time")?,
        );
    } else {
        border.set_size(number(case, "size")?);
    }

    for point in array(case, "points")? {
        let x = number(point, "x")?;
        let z = number(point, "z")?;
        let y = number(point, "y")?;
        let margin = number(point, "margin")?;
        let box_ = aabb(&point["box"])?;
        let block = BlockPos::new(integer(point, "block_x")?, 0, integer(point, "block_z")?);
        let chunk = ChunkPos::new(integer(point, "chunk_x")?, integer(point, "chunk_z")?);
        report.flag(
            index,
            "isWithinBounds(x,z)",
            &point["inside"],
            border.is_within_bounds(x, z, 0.0),
        );
        report.flag(
            index,
            "isWithinBounds(x,z,margin)",
            &point["inside_margin"],
            border.is_within_bounds(x, z, margin),
        );
        report.flag(
            index,
            "isWithinBounds(BlockPos)",
            &point["inside_block"],
            border.is_within_bounds_pos(block),
        );
        report.flag(
            index,
            "isWithinBounds(AABB)",
            &point["inside_aabb"],
            border.is_within_bounds_aabb(box_),
        );
        report.flag(
            index,
            "isWithinBounds(ChunkPos)",
            &point["inside_chunk"],
            border.is_within_bounds_chunk(chunk),
        );
        report.bits(
            index,
            "getDistanceToBorder",
            &point["distance"],
            border.distance_to_border(x, z),
        );
        let clamped = border.clamp_vec3(Vec3::new(x, y, z));
        compare_triple(
            report,
            index,
            "clampVec3ToBound",
            &point["clamp"],
            [clamped.x, clamped.y, clamped.z],
        );
        let clamped_pos = border.clamp_to_bounds(Vec3::new(x, y, z));
        report.ints(
            index,
            "clampToBounds",
            &point["clamp_pos"],
            [
                f64::from(clamped_pos.x),
                f64::from(clamped_pos.y),
                f64::from(clamped_pos.z),
            ],
        );
    }

    for query in array(case, "close")? {
        let box_ = aabb(&query["box"])?;
        report.flag(
            index,
            "isInsideCloseToBorder",
            &query["inside"],
            border.is_inside_close_to_border(number(query, "x")?, number(query, "z")?, box_),
        );
    }

    for query in array(case, "collides")? {
        let box_ = aabb(&query["box"])?;
        let triple = numbers(&query["movement"])?;
        let movement = Vec3::new(triple[0], triple[1], triple[2]);
        let resolved = collide_border_only(movement, box_, &border);
        compare_triple(
            report,
            index,
            "Shapes.collide(border)",
            &query["resolved"],
            [resolved.x, resolved.y, resolved.z],
        );
    }

    for packet in array(case, "packets")? {
        let kind = packet["kind"].as_str().unwrap_or("");
        let bytes = packet_body(&border, kind);
        report.compare(
            index,
            &format!("{kind} packet"),
            &packet["bytes"],
            Value::String(hex(&bytes)),
        );
    }

    for step in array(case, "steps")? {
        border.tick();
        report.bits(index, "size after tick", &step["size"], border.size());
        report.compare(
            index,
            "lerpTime after tick",
            &step["lerp_time"],
            Value::from(border.lerp_time()),
        );
        report.bits(
            index,
            "lerpTarget after tick",
            &step["lerp_target"],
            border.lerp_target(),
        );
        report.bits(
            index,
            "lerpSpeed after tick",
            &step["lerp_speed"],
            border.lerp_speed(),
        );
        report.bits(index, "minX after tick", &step["min_x"], border.min_x());
        report.bits(index, "maxX after tick", &step["max_x"], border.max_x());
        report.bits(index, "minZ after tick", &step["min_z"], border.min_z());
        report.bits(index, "maxZ after tick", &step["max_z"], border.max_z());
        let status = match border.status() {
            BorderStatus::Stationary => "STATIONARY",
            BorderStatus::Shrinking => "SHRINKING",
            BorderStatus::Growing => "GROWING",
        };
        report.compare(
            index,
            "status after tick",
            &step["status"],
            Value::String(status.to_owned()),
        );
        let far = border.clamp_to_bounds(Vec3::new(
            number(case, "center_x")? + 1.0e9,
            64.0,
            number(case, "center_z")?,
        ));
        report.bits(
            index,
            "clampToBounds after tick",
            &step["clamp_x"],
            f64::from(far.x),
        );
        // 方块实体阶段用的整数方块盒，和官方 `isWithinBounds(BlockPos)` 逐格比较。
        let block_box = border.block_box();
        for entry in array(step, "ticking_blocks")? {
            let pos = BlockPos::new(integer(entry, "x")?, 64, integer(entry, "z")?);
            report.flag(
                index,
                &format!("block entity ticks at {} {} after tick", pos.x, pos.z),
                &entry["inside"],
                block_box.contains(pos),
            );
        }
    }
    Ok(())
}

fn compare_triple(
    report: &mut Report,
    case: usize,
    what: &str,
    expected: &Value,
    actual: [f64; 3],
) {
    report.compare(case, what, expected, serde_json::json!(actual));
}

fn packet_body(border: &WorldBorder, kind: &str) -> Vec<u8> {
    let mut out = Vec::new();
    match kind {
        "initialize" => InitializeBorder {
            center_x: border.center_x(),
            center_z: border.center_z(),
            old_size: border.size(),
            new_size: border.lerp_target(),
            lerp_time: border.lerp_time(),
            absolute_max_size: border.absolute_max_size(),
            warning_blocks: border.warning_blocks(),
            warning_time: border.warning_time(),
        }
        .write_body(&mut out),
        "center" => SetBorderCenter {
            center_x: border.center_x(),
            center_z: border.center_z(),
        }
        .write_body(&mut out),
        "lerp_size" => SetBorderLerpSize {
            old_size: border.size(),
            new_size: border.lerp_target(),
            lerp_time: border.lerp_time(),
        }
        .write_body(&mut out),
        "size" => SetBorderSize {
            size: border.lerp_target(),
        }
        .write_body(&mut out),
        "warning_delay" => SetBorderWarningDelay {
            warning_delay: border.warning_time(),
        }
        .write_body(&mut out),
        "warning_distance" => SetBorderWarningDistance {
            warning_blocks: border.warning_blocks(),
        }
        .write_body(&mut out),
        _ => {}
    }
    out
}

fn array<'a>(parent: &'a Value, key: &str) -> Result<Vec<&'a Value>, String> {
    parent[key]
        .as_array()
        .map(|items| items.iter().collect())
        .ok_or_else(|| format!("missing {key} array"))
}

fn numbers(value: &Value) -> Result<[f64; 3], String> {
    let items = value
        .as_array()
        .ok_or_else(|| format!("expected three numbers, got {value}"))?;
    let mut out = [0.0; 3];
    for (index, item) in items.iter().enumerate().take(3) {
        out[index] = item
            .as_f64()
            .ok_or_else(|| format!("expected a number, got {item}"))?;
    }
    Ok(out)
}

fn aabb(value: &Value) -> Result<Aabb, String> {
    let items = value
        .as_array()
        .ok_or_else(|| format!("expected an AABB, got {value}"))?;
    if items.len() != 6 {
        return Err(format!("expected six AABB coordinates, got {value}"));
    }
    let mut out = [0.0; 6];
    for (index, item) in items.iter().enumerate() {
        out[index] = item
            .as_f64()
            .ok_or_else(|| format!("expected a number, got {item}"))?;
    }
    Ok(Aabb::new(out[0], out[1], out[2], out[3], out[4], out[5]))
}

fn number(parent: &Value, key: &str) -> Result<f64, String> {
    parent[key]
        .as_f64()
        .ok_or_else(|| format!("missing {key} number"))
}

fn integer(parent: &Value, key: &str) -> Result<i32, String> {
    i32::try_from(long(parent, key)?).map_err(|_| format!("{key} does not fit in i32"))
}

fn long(parent: &Value, key: &str) -> Result<i64, String> {
    parent[key]
        .as_i64()
        .ok_or_else(|| format!("missing {key} integer"))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        text.push(char::from(DIGITS[(byte >> 4) as usize]));
        text.push(char::from(DIGITS[(byte & 0xf) as usize]));
    }
    text
}
