//! 用官方代码检查 `mc-bamboo-worldgen` 的世界生成数学。
//!
//! `WorldgenOracle.java` 按官方的加载流程把数据包注册表读进来，对一批种子和一批坐标问官方要数值：
//! 每条噪声的两条求值路径、每条注册表密度函数和七套维度设置的 `NoiseRouter` 八个输出的两条路径，
//! 还有主世界、下界、末地的群系。这里边读边用 `mc-bamboo-worldgen` 算同样的东西，浮点逐位比较。
//!
//! 成批那条路径（`sampleVolume`）对含 `interpolated` 的函数不是纯的单点函数：官方按整格插值，
//! 用到了体积的边界。这些函数的单点路径照常比，成批路径跳过，跳过的条数会报出来。
//! 它们的单点求值本来就会读输入的成批路径，所以那一段照样覆盖到了。

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use mc_bamboo_data::version;
use mc_bamboo_worldgen::{
    Worldgen,
    climate::TargetPoint,
    sampler::Path,
    settings::{RouterOutput, SeededRouter},
};
use serde::Deserialize;

use crate::oracle::collect_log;

const USAGE: &str =
    "usage: worldgen <java> [--seeds N] [--salt S] [--points N] [--biome-points N] [--show N]";
/// `--show` 的默认值。
const DEFAULT_SHOW: usize = 5;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Line {
    Header {
        version: String,
        protocol: i32,
    },
    /// 一条噪声在一批坐标上的两条路径。
    Noise {
        settings: String,
        seed: i64,
        id: String,
        samples: Vec<Sample>,
    },
    /// 一条注册表密度函数。
    Function {
        settings: String,
        seed: i64,
        id: String,
        samples: Vec<Sample>,
    },
    /// 一套设置的噪声路由的一个输出。
    Router {
        settings: String,
        seed: i64,
        output: String,
        samples: Vec<Sample>,
    },
    /// 一个维度的群系。
    Biome {
        settings: String,
        preset: String,
        seed: i64,
        samples: Vec<BiomeSample>,
    },
    End {
        checks: u64,
    },
}

/// x、y、z、单点路径的 float 位、成批路径的 float 位。
#[derive(Debug, Deserialize)]
struct Sample(i32, i32, i32, i32, i32);

/// quart x、y、z、群系 ID。
#[derive(Debug, Deserialize)]
struct BiomeSample(i32, i32, i32, String);

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
                "--seeds" | "--salt" | "--points" | "--biome-points" => {
                    let value = args.next().ok_or(USAGE)?;
                    options
                        .forwarded
                        .extend([arg.to_owned(), (*value).to_owned()]);
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

/// 一次运行的计数和不一致。
#[derive(Debug)]
struct Report {
    worldgen: Worldgen,
    /// 当前这个种子下，设置 ID → 编译好的路由。换种子时清空。
    routers: HashMap<String, SeededRouter>,
    /// 一共编译过多少个（维度，种子）。
    dimension_states: u64,
    noise_checks: u64,
    function_checks: u64,
    biome_checks: u64,
    skipped_volume: u64,
    seeds: Vec<i64>,
    mismatches: Vec<String>,
    show: usize,
    ended: bool,
}

/// 启动官方进程，一边读一边比较。`command` 已经是能运行 `WorldgenOracle.java` 的命令。
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
        worldgen,
        routers: HashMap::new(),
        dimension_states: 0,
        noise_checks: 0,
        function_checks: 0,
        biome_checks: 0,
        skipped_volume: 0,
        seeds: Vec::new(),
        mismatches: Vec::new(),
        show: options.show.unwrap_or(DEFAULT_SHOW),
        ended: false,
    };
    let read = report.read_lines(BufReader::new(stdout));
    if read.is_err() {
        // 这边不再读了，JVM 会一直等着管道。
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for java: {error}"))?;
    let log = log.recv().unwrap_or_default().join("\n");
    read.map_err(|error| format!("{error}\n{log}"))?;
    if !status.success() {
        return Err(format!("WorldgenOracle.java failed: {status}\n{log}"));
    }
    report.finish()
}

impl Report {
    fn read_lines(&mut self, source: BufReader<impl Read>) -> Result<(), String> {
        for line in source.lines() {
            let line = line.map_err(|error| format!("cannot read from java: {error}"))?;
            let parsed: Line = serde_json::from_str(&line)
                .map_err(|error| format!("bad worldgen oracle line: {error}"))?;
            match parsed {
                Line::Header { version, protocol } => self.check_header(&version, protocol)?,
                Line::Noise {
                    settings,
                    seed,
                    id,
                    samples,
                } => self.compare_noise(&settings, seed, &id, &samples)?,
                Line::Function {
                    settings,
                    seed,
                    id,
                    samples,
                } => self.compare_function(&settings, seed, &id, &samples)?,
                Line::Router {
                    settings,
                    seed,
                    output,
                    samples,
                } => self.compare_router(&settings, seed, &output, &samples)?,
                Line::Biome {
                    settings,
                    preset,
                    seed,
                    samples,
                } => self.compare_biomes(&settings, &preset, seed, &samples)?,
                Line::End { checks } => {
                    self.ended = true;
                    let ours = self.noise_checks
                        + self.function_checks
                        + self.biome_checks
                        + self.skipped_volume;
                    if checks != ours {
                        return Err(format!("java wrote {checks} checks, {ours} arrived"));
                    }
                }
            }
        }
        Ok(())
    }

    fn check_header(&mut self, game_version: &str, protocol: i32) -> Result<(), String> {
        if game_version != version::GAME_VERSION || protocol != version::PROTOCOL_VERSION {
            return Err(format!(
                "the generated data is for {} (protocol {}), the official jar is {game_version} (protocol {protocol})",
                version::GAME_VERSION,
                version::PROTOCOL_VERSION,
            ));
        }
        Ok(())
    }

    /// 按设置 ID 拿一个编译好的路由，没有就新建一个。
    ///
    /// 官方那边把一个种子的全部题目一次写完才换下一个种子，所以换种子时把上一批丢掉：
    /// 一个路由连着几十条噪声实例，几十个种子一起留着要几百兆。
    /// R 树的「上一次结果」在 `worldgen` 里，不跟着丢，群系的顺序照样连着。
    fn router(&mut self, settings: &str, seed: i64) -> Result<&mut SeededRouter, String> {
        if self.seeds.last() != Some(&seed) {
            self.routers.clear();
            self.seeds.push(seed);
        }
        if !self.routers.contains_key(settings) {
            let found = self
                .worldgen
                .settings(settings)
                .ok_or_else(|| format!("unknown noise settings {settings}"))?;
            self.routers
                .insert(settings.to_owned(), SeededRouter::new(found, seed));
            self.dimension_states += 1;
        }
        Ok(self.routers.get_mut(settings).expect("刚插进去"))
    }

    fn compare_noise(
        &mut self,
        settings: &str,
        seed: i64,
        id: &str,
        samples: &[Sample],
    ) -> Result<(), String> {
        // `WorldgenOracle` 里的两个系数，两边要一样。
        const XZ_SCALE: f64 = 0.3125;
        const Y_SCALE: f64 = 1.25;

        let parameters = self
            .worldgen
            .noises()
            .get(id)
            .ok_or_else(|| format!("unknown noise {id}"))?
            .clone();
        let noise = self.router(settings, seed)?.noise(id, &parameters);
        let mut found = Vec::with_capacity(samples.len() * 2);
        for sample in samples {
            let (x, y, z) = (sample.0, sample.1, sample.2);
            let direct = noise.get(
                f64::from(x) * XZ_SCALE,
                f64::from(y) * Y_SCALE,
                f64::from(z) * XZ_SCALE,
            );
            let volume = noise.add_to_point((x, y, z), XZ_SCALE, Y_SCALE, 1.0);
            found.push((sample, direct, volume));
        }
        for (sample, direct, volume) in found {
            self.noise_checks += 2;
            self.check_bits(
                sample.3,
                direct,
                &format!("noise {id} seed {seed} get"),
                sample,
            );
            self.check_bits(
                sample.4,
                volume,
                &format!("noise {id} seed {seed} addToVolume"),
                sample,
            );
        }
        Ok(())
    }

    fn compare_function(
        &mut self,
        settings: &str,
        seed: i64,
        id: &str,
        samples: &[Sample],
    ) -> Result<(), String> {
        let function = self
            .worldgen
            .functions()
            .get(id)
            .ok_or_else(|| format!("unknown density function {id}"))?
            .clone();
        let compare_volume = !function.has_interpolated();
        let router = self.router(settings, seed)?;
        let sampler = router.compile(&function);
        let mut found = Vec::with_capacity(samples.len());
        for sample in samples {
            let point = router.sample_with(&sampler, Path::Point, sample.0, sample.1, sample.2);
            let volume = if compare_volume {
                Some(router.sample_with(&sampler, Path::Volume, sample.0, sample.1, sample.2))
            } else {
                None
            };
            found.push((sample, point, volume));
        }
        for (sample, point, volume) in found {
            self.function_checks += 1;
            self.check_bits(
                sample.3,
                point,
                &format!("function {id} seed {seed} sampleValue"),
                sample,
            );
            match volume {
                Some(value) => {
                    self.function_checks += 1;
                    self.check_bits(
                        sample.4,
                        value,
                        &format!("function {id} seed {seed} sampleVolume"),
                        sample,
                    );
                }
                None => self.skipped_volume += 1,
            }
        }
        Ok(())
    }

    fn compare_router(
        &mut self,
        settings: &str,
        seed: i64,
        output: &str,
        samples: &[Sample],
    ) -> Result<(), String> {
        let which = RouterOutput::ALL
            .into_iter()
            .find(|candidate| candidate.field() == output)
            .ok_or_else(|| format!("unknown router output {output}"))?;
        let compare_volume = !self
            .worldgen
            .settings(settings)
            .ok_or_else(|| format!("unknown noise settings {settings}"))?
            .router
            .get(which)
            .has_interpolated();
        let router = self.router(settings, seed)?;
        let mut found = Vec::with_capacity(samples.len());
        for sample in samples {
            let point = router.sample_router(which, Path::Point, sample.0, sample.1, sample.2);
            let volume = if compare_volume {
                Some(router.sample_router(which, Path::Volume, sample.0, sample.1, sample.2))
            } else {
                None
            };
            found.push((sample, point, volume));
        }
        for (sample, point, volume) in found {
            self.function_checks += 1;
            let place = format!("router {settings} {output} seed {seed} sampleValue");
            self.check_bits(sample.3, point, &place, sample);
            match volume {
                Some(value) => {
                    self.function_checks += 1;
                    let place = format!("router {settings} {output} seed {seed} sampleVolume");
                    self.check_bits(sample.4, value, &place, sample);
                }
                None => self.skipped_volume += 1,
            }
        }
        Ok(())
    }

    /// 群系按到达顺序重放：官方 R 树的「上一次结果」会影响平局，顺序有意义。
    fn compare_biomes(
        &mut self,
        settings: &str,
        preset: &str,
        seed: i64,
        samples: &[BiomeSample],
    ) -> Result<(), String> {
        let mut found = Vec::with_capacity(samples.len());
        if preset == "the_end" {
            let end = self.worldgen.end_biomes().clone();
            let router = self.router(settings, seed)?;
            for sample in samples {
                found.push(
                    end.biome_at(router, (sample.0, sample.1, sample.2))
                        .to_owned(),
                );
            }
        } else {
            // 借用检查器不让同时借 worldgen 的预设和 routers，所以先把气候参数算出来。
            let router = self.router(settings, seed)?;
            let targets: Vec<TargetPoint> = samples
                .iter()
                .map(|sample| router.climate(sample.0, sample.1, sample.2))
                .collect();
            let source = self
                .worldgen
                .preset(preset)
                .ok_or_else(|| format!("unknown preset {preset}"))?;
            for target in targets {
                found.push(source.biome(target).to_owned());
            }
        }
        for (sample, ours) in samples.iter().zip(found) {
            self.biome_checks += 1;
            if ours != sample.3 {
                self.mismatch(format!(
                    "biome {settings} seed {seed} at quart ({}, {}, {}): official {}, bamboo {ours}",
                    sample.0, sample.1, sample.2, sample.3
                ));
            }
        }
        Ok(())
    }

    fn check_bits(&mut self, official: i32, ours: f32, place: &str, sample: &Sample) {
        let bits = ours.to_bits() as i32;
        if bits != official {
            self.mismatch(format!(
                "{place} at ({}, {}, {}): official {} ({:#010x}), bamboo {ours} ({:#010x})",
                sample.0,
                sample.1,
                sample.2,
                f32::from_bits(official as u32),
                official,
                bits
            ));
        }
    }

    fn mismatch(&mut self, message: String) {
        if self.mismatches.len() < self.show {
            println!("MISMATCH {message}");
        }
        self.mismatches.push(message);
    }

    fn finish(&self) -> Result<(), String> {
        if !self.ended {
            return Err("WorldgenOracle.java stopped before its end line".to_owned());
        }
        println!(
            "worldgen: {} seeds, {} dimension states; compared {} noise values, {} density function values, {} biomes",
            self.seeds.len(),
            self.dimension_states,
            self.noise_checks,
            self.function_checks,
            self.biome_checks
        );
        println!(
            "skipped {} sampleVolume checks on functions that contain interpolated (not a point function)",
            self.skipped_volume
        );
        println!("WORLDGEN RESULT: {} mismatches", self.mismatches.len());
        if self.mismatches.is_empty() {
            Ok(())
        } else {
            Err(format!("{} worldgen mismatches", self.mismatches.len()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_forward_the_seed_selection() {
        let options =
            Options::parse(&["--seeds", "4", "--points", "8", "--show", "2"]).expect("合法的参数");
        assert_eq!(options.forwarded, ["--seeds", "4", "--points", "8"]);
        assert_eq!(options.show, Some(2));
        assert!(Options::parse(&["--bogus"]).is_err());
    }

    /// 嵌着的官方数据能解析，三个维度的路由都编译得出来。
    #[test]
    fn the_embedded_data_builds_every_dimension() {
        let worldgen = Worldgen::load().expect("数据能解析");
        for id in ["minecraft:overworld", "minecraft:nether", "minecraft:end"] {
            let settings = worldgen.settings(id).expect("有这个维度");
            let mut router = SeededRouter::new(settings, 0);
            for output in RouterOutput::ALL {
                assert!(
                    router.sample(output, 0, 0, 0).is_finite(),
                    "{id} {output:?}"
                );
            }
        }
    }
}
