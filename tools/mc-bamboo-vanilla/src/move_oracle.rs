//! 用官方的 `ServerGamePacketListenerImpl.handleMovePlayer` 对照玩家移动校验。
//!
//! `MovementOracle.java` 在假世界里跑官方监听器，一题一行写下接受、拉回还是断开，以及最终位置。
//! 这里用同样的起点、方块和移动包重放，按原始位比较位置和摔落距离。
//!
//! Compares player movement checks with official `ServerGamePacketListenerImpl.handleMovePlayer`.
//! `MovementOracle.java` runs the official listener in a fake world and writes whether each case
//! was accepted, rubber-banded, or disconnected, plus the final position. This replays the same
//! start, blocks, and move packets and compares position and fall distance by their raw bits.

use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::version;
use mc_bamboo_game::{MoveReplay, MoveReplayPacket, replay_movement};
use mc_bamboo_protocol::packet::play::GameMode;
use serde::Deserialize;

use crate::oracle::collect_log;

const USAGE: &str = "usage: move-oracle <java> [--cases N] [--seed S] [--show N]";
const DEFAULT_SHOW: usize = 8;

#[derive(Debug, Default)]
struct Options {
    forwarded: Vec<String>,
    show: Option<usize>,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self::default();
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
                    options.show = Some(value.parse().map_err(|_| USAGE.to_owned())?);
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Line {
    #[serde(rename = "header")]
    Header(Header),
    #[serde(rename = "case")]
    Case(Case),
    #[serde(rename = "end")]
    End(End),
}

#[derive(Debug, Deserialize)]
struct Header {
    version: String,
    protocol: i32,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    outcome: String,
    kicked: bool,
    teleports: u32,
    #[serde(rename = "xBits")]
    x_bits: i64,
    #[serde(rename = "yBits")]
    y_bits: i64,
    #[serde(rename = "zBits")]
    z_bits: i64,
    #[serde(rename = "fallBits")]
    fall_bits: i64,
    mode: String,
    shift: bool,
    sprint: bool,
    flying: bool,
    loaded: bool,
    #[serde(rename = "onGround")]
    on_ground: bool,
    sx: i64,
    sy: i64,
    sz: i64,
    blocks: Vec<[i32; 4]>,
    packets: Vec<Packet>,
}

#[derive(Debug, Deserialize)]
struct Packet {
    kind: String,
    #[serde(default)]
    x: i64,
    #[serde(default)]
    y: i64,
    #[serde(default)]
    z: i64,
    #[serde(default, rename = "onGround")]
    on_ground: bool,
    #[serde(default)]
    horizontal: bool,
}

#[derive(Debug, Deserialize)]
struct End {
    cases: u32,
}

struct Report {
    show: usize,
    cases: u32,
    mismatches: Vec<String>,
    ended: bool,
}

/// 启动官方进程，逐题重放。
///
/// Starts the official process and replays each case.
pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    command.env(
        "JAVA_TOOL_OPTIONS",
        "-Dstdout.encoding=UTF-8 -Dstderr.encoding=UTF-8",
    );
    let mut child = command
        .args(&options.forwarded)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;
    let mut report = Report {
        show: options.show.unwrap_or(DEFAULT_SHOW),
        cases: 0,
        mismatches: Vec::new(),
        ended: false,
    };
    let read = report.read(BufReader::new(stdout));
    if read.is_err() {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    read.map_err(|error| format!("{error}\n{log}"))?;
    if !status.success() {
        return Err(format!("MovementOracle.java failed: {status}\n{log}"));
    }
    report.finish()
}

impl Report {
    fn read(&mut self, source: BufReader<impl Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            if line.is_empty() {
                continue;
            }
            let parsed: Line = serde_json::from_str(&line)
                .map_err(|error| format!("bad movement oracle line: {error}: {line}"))?;
            match parsed {
                Line::Header(header) => self.check_header(&header)?,
                Line::Case(case) => self.replay(&case)?,
                Line::End(end) => {
                    self.ended = true;
                    if end.cases != self.cases {
                        return Err(format!(
                            "java wrote {} cases, {} arrived",
                            end.cases, self.cases
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn check_header(&self, header: &Header) -> Result<(), String> {
        if header.version != version::GAME_VERSION || header.protocol != version::PROTOCOL_VERSION {
            return Err(format!(
                "the generated data is for {} (protocol {}), the official jar is {} (protocol {})",
                version::GAME_VERSION,
                version::PROTOCOL_VERSION,
                header.version,
                header.protocol,
            ));
        }
        Ok(())
    }

    fn replay(&mut self, case: &Case) -> Result<(), String> {
        self.cases += 1;
        let sample = MoveReplay {
            mode: game_mode(&case.mode)?,
            shift: case.shift,
            sprint: case.sprint,
            flying: case.flying,
            loaded: case.loaded,
            blocks: case
                .blocks
                .iter()
                .map(|block| (BlockPos::new(block[0], block[1], block[2]), block[3] as u32))
                .collect(),
            start: Vec3::new(
                f64::from_bits(case.sx as u64),
                f64::from_bits(case.sy as u64),
                f64::from_bits(case.sz as u64),
            ),
            yaw: 0.0,
            pitch: 0.0,
            on_ground: case.on_ground,
            packets: case
                .packets
                .iter()
                .map(replay_packet)
                .collect::<Result<Vec<_>, _>>()?,
        };
        let report = replay_movement(&sample);
        let official_position = Vec3::new(
            f64::from_bits(case.x_bits as u64),
            f64::from_bits(case.y_bits as u64),
            f64::from_bits(case.z_bits as u64),
        );
        let official_fall = f64::from_bits(case.fall_bits as u64);
        let same = report.outcome == case.outcome
            && report.kicked == case.kicked
            && report.teleports == case.teleports
            && report.position.x.to_bits() == case.x_bits as u64
            && report.position.y.to_bits() == case.y_bits as u64
            && report.position.z.to_bits() == case.z_bits as u64
            && report.fall_distance.to_bits() == case.fall_bits as u64;
        if !same {
            self.mismatches.push(format!(
                "{}: official {} kick {} tp {} pos {:?} fall {}, bamboo {} kick {} tp {} pos {:?} fall {}",
                case.name,
                case.outcome,
                case.kicked,
                case.teleports,
                official_position,
                official_fall,
                report.outcome,
                report.kicked,
                report.teleports,
                report.position,
                report.fall_distance,
            ));
        }
        Ok(())
    }

    fn finish(self) -> Result<(), String> {
        if !self.ended {
            return Err(format!(
                "movement oracle stopped after {} cases, before the end line",
                self.cases
            ));
        }
        println!(
            "MOVE ORACLE: {} cases, {} mismatches",
            self.cases,
            self.mismatches.len()
        );
        for mismatch in self.mismatches.iter().take(self.show) {
            println!("MOVE ORACLE FAIL: {mismatch}");
        }
        if self.mismatches.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "movement oracle: {} mismatches in {} cases",
                self.mismatches.len(),
                self.cases
            ))
        }
    }
}

fn game_mode(name: &str) -> Result<GameMode, String> {
    match name {
        "survival" => Ok(GameMode::Survival),
        "creative" => Ok(GameMode::Creative),
        "adventure" => Ok(GameMode::Adventure),
        "spectator" => Ok(GameMode::Spectator),
        other => Err(format!("unknown game mode {other}")),
    }
}

fn replay_packet(packet: &Packet) -> Result<MoveReplayPacket, String> {
    match packet.kind.as_str() {
        "move" => Ok(MoveReplayPacket::Move {
            position: Some(Vec3::new(
                f64::from_bits(packet.x as u64),
                f64::from_bits(packet.y as u64),
                f64::from_bits(packet.z as u64),
            )),
            rotation: None,
            on_ground: packet.on_ground,
            horizontal_collision: packet.horizontal,
        }),
        "tickEnd" => Ok(MoveReplayPacket::TickEnd),
        "serverTick" => Ok(MoveReplayPacket::ServerTick),
        other => Err(format!("unknown movement packet {other}")),
    }
}
