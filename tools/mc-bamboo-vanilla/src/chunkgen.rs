//! 区块级对拍：官方在 JVM 里生成到 `BIOMES` 和 `NOISE`，Bamboo 用 `mc-bamboo-worldgen`
//! 重算同一个区块，逐 quart 格、逐方块比较。
//!
//! `ChunkgenOracle.java` 不启动服务端：它按官方的加载流程读数据包注册表，造一个
//! `ProtoChunk`，调 `ChunkGenerator.createBiomes` 和（反射）`NoiseBasedChunkGenerator.doFill`。
//! 26.3 的 `terrain` 状态把噪声、地表规则、雕刻器合在一步里，所以 NOISE 阶段就是
//! `doFill` 跑完那一刻，不是 `ChunkStatus` 的一个独立状态。
//!
//! 官方的方块按名字给（`BlockState.toString()`），Bamboo 存的是默认状态 ID，
//! `mc-bamboo-data` 里查回名字再比；这一步只有石头、空气、水、岩浆几个默认状态。

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_data::{registries, version};
use mc_bamboo_worldgen::{Worldgen, chunk::Generator};
use serde::Deserialize;

use crate::oracle::collect_log;

const USAGE: &str = "usage: chunkgen <java> [--seeds N] [--chunks N] [--salt S] [--show N]";
const DEFAULT_SHOW: usize = 8;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Line {
    Header {
        version: String,
        protocol: i32,
        settings: String,
        preset: String,
        #[serde(rename = "minY")]
        min_y: i32,
        height: i32,
        #[serde(rename = "seaLevel")]
        sea_level: i32,
    },
    /// 一个区块的群系：24 段，每段 64 个 quart 格，顺序 x、y、z。
    Biomes {
        seed: i64,
        x: i32,
        z: i32,
        sections: Vec<Vec<String>>,
    },
    /// 一个区块的方块：调色板加 16×高度×16 的下标，顺序 y、x、z。
    Noise {
        seed: i64,
        x: i32,
        z: i32,
        palette: Vec<String>,
        blocks: Vec<u16>,
    },
    End {
        checks: u64,
    },
}

#[derive(Debug, Default)]
struct Options {
    forwarded: Vec<String>,
    show: Option<usize>,
    /// 只比 BIOMES，跳过 NOISE。
    biomes_only: bool,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--seeds" | "--chunks" | "--salt" => {
                    let value = args.next().ok_or(USAGE)?;
                    options
                        .forwarded
                        .extend([arg.to_owned(), (*value).to_owned()]);
                }
                "--biomes-only" => {
                    options.biomes_only = true;
                    options.forwarded.push(arg.to_owned());
                }
                "--show" => {
                    let value = args.next().ok_or(USAGE)?;
                    options.show = Some(value.parse().map_err(|_| USAGE)?);
                }
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

#[derive(Debug)]
struct Report<'a> {
    worldgen: &'a Worldgen,
    generators: HashMap<i64, Generator<'a>>,
    biomes: u64,
    noise: u64,
    chunks: u64,
    checks: u64,
    mismatches: Vec<String>,
    show: usize,
    ended: bool,
}

/// 启动官方进程，一边读一边比较。`command` 已经是能运行 `ChunkgenOracle.java` 的命令。
pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let worldgen = Worldgen::load()?;
    let mut child = command
        .args(&options.forwarded)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run java: {error}"))?;
    let log = collect_log(&mut child)?;
    let stdout = child.stdout.take().ok_or("java has no stdout")?;

    let mut report = Report {
        worldgen: &worldgen,
        generators: HashMap::new(),
        biomes: 0,
        noise: 0,
        chunks: 0,
        checks: 0,
        mismatches: Vec::new(),
        show: options.show.unwrap_or(DEFAULT_SHOW),
        ended: false,
    };
    let read = report.read_lines(BufReader::new(stdout));
    if read.is_err() {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    read.map_err(|error| format!("{error}\n{log}"))?;
    if !status.success() {
        return Err(format!("ChunkgenOracle.java failed: {status}\n{log}"));
    }
    report.finish()
}

impl Report<'_> {
    fn read_lines(&mut self, source: BufReader<impl Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            let parsed: Line = serde_json::from_str(&line)
                .map_err(|error| format!("bad chunkgen oracle line: {error}"))?;
            match parsed {
                Line::Header {
                    version: reported,
                    protocol,
                    settings,
                    preset,
                    min_y,
                    height,
                    sea_level,
                } => {
                    if reported != version::GAME_VERSION || protocol != version::PROTOCOL_VERSION {
                        return Err(format!(
                            "oracle is {reported}/{protocol}, bamboo is {}/{}",
                            version::GAME_VERSION,
                            version::PROTOCOL_VERSION
                        ));
                    }
                    if settings != "minecraft:overworld" || preset != "minecraft:overworld" {
                        return Err(format!("unexpected settings {settings} / {preset}"));
                    }
                    let ours = self
                        .worldgen
                        .settings(&settings)
                        .ok_or("missing overworld settings")?;
                    if ours.min_y != min_y || ours.height != height || ours.sea_level != sea_level {
                        return Err(format!(
                            "settings differ: official {min_y}/{height}/{sea_level}, bamboo {}/{}/{}",
                            ours.min_y, ours.height, ours.sea_level
                        ));
                    }
                }
                Line::Biomes {
                    seed,
                    x,
                    z,
                    sections,
                } => self.compare_biomes(seed, x, z, &sections),
                Line::Noise {
                    seed,
                    x,
                    z,
                    palette,
                    blocks,
                } => self.compare_noise(seed, x, z, &palette, &blocks),
                Line::End { checks } => {
                    self.checks = checks;
                    self.ended = true;
                }
            }
        }
        Ok(())
    }

    fn generator(&mut self, seed: i64) -> Result<&Generator<'_>, String> {
        if !self.generators.contains_key(&seed) {
            let generator = Generator::new(self.worldgen, seed)?;
            self.generators.insert(seed, generator);
        }
        Ok(self.generators.get(&seed).expect("刚插进去"))
    }

    fn compare_biomes(&mut self, seed: i64, x: i32, z: i32, sections: &[Vec<String>]) {
        let ours = match self
            .generator(seed)
            .and_then(|generator| generator.biomes(x, z))
        {
            Ok(sections) => sections,
            Err(error) => {
                self.mismatch(format!("biomes seed {seed} chunk ({x},{z}): {error}"));
                return;
            }
        };
        if ours.len() != sections.len() {
            self.mismatch(format!(
                "biomes seed {seed} chunk ({x},{z}): {} sections vs official {}",
                ours.len(),
                sections.len()
            ));
            return;
        }
        for (section, (actual, expected)) in ours.iter().zip(sections).enumerate() {
            if actual.len() != expected.len() {
                self.mismatch(format!(
                    "biomes seed {seed} chunk ({x},{z}) section {section}: {} cells vs {}",
                    actual.len(),
                    expected.len()
                ));
                return;
            }
            for (cell, (id, name)) in actual.iter().zip(expected).enumerate() {
                let ours = registries::biome::NAMES
                    .get(*id as usize)
                    .copied()
                    .unwrap_or("<unknown>");
                self.biomes += 1;
                if ours != name {
                    self.mismatch(format!(
                        "biomes seed {seed} chunk ({x},{z}) section {section} cell {cell}: official {name}, bamboo {ours}"
                    ));
                }
            }
        }
    }

    fn compare_noise(&mut self, seed: i64, x: i32, z: i32, palette: &[String], expected: &[u16]) {
        let _ = (seed, x, z, palette, expected);
        self.noise += expected.len() as u64;
        self.chunks += 1;
        // NOISE 阶段的实现和对拍在下一步接上。
        self.mismatch(format!(
            "noise seed {seed} chunk ({x},{z}): not implemented yet"
        ));
    }

    fn mismatch(&mut self, text: String) {
        if self.mismatches.len() < self.show * 20 {
            self.mismatches.push(text);
        }
    }

    fn finish(&self) -> Result<(), String> {
        println!(
            "chunkgen: {} chunks, compared {} biome cells and {} blocks",
            self.chunks, self.biomes, self.noise
        );
        if !self.ended {
            return Err("ChunkgenOracle.java stopped before its end line".to_owned());
        }
        println!("oracle reported {} checks", self.checks);
        if !self.mismatches.is_empty() {
            for text in &self.mismatches[..self.mismatches.len().min(self.show)] {
                println!("  {text}");
            }
            return Err(format!(
                "CHUNKGEN RESULT: {} mismatches",
                self.mismatches.len()
            ));
        }
        println!("CHUNKGEN RESULT: 0 mismatches");
        Ok(())
    }
}
