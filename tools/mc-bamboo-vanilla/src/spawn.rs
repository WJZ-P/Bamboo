//! `spawn`：多种子下把新世界的出生区块和出生点与官方逐个对拍。
//!
//! `SpawnOracle.java` 在 JVM 里跑官方 `MinecraftServer.setInitialSpawn`（`getOrigin`
//! + 11×11 区块螺旋 + `PlayerSpawnFinder.getSpawnPosInChunk`，地形到 CARVERS），
//! 每个种子输出出生区块 `[cx, cz]` 和出生点 `[x, y, z]`。Bamboo 侧调
//! `mc-bamboo-game::fresh_spawn_sample` —— 游戏里 `setInitialSpawn` 那份代码 ——
//! 逐个比较，0 不一致才算过。`--break-self-test` 故意改坏一个出生点确认会失败。
//!
//! `spawn`: compare the fresh-world spawn chunk and spawn position against
//! vanilla across seeds. `SpawnOracle.java` runs the official
//! `MinecraftServer.setInitialSpawn` (`getOrigin` + the 11x11 chunk spiral +
//! `PlayerSpawnFinder.getSpawnPosInChunk`, terrain to CARVERS) and emits the
//! spawn chunk `[cx, cz]` and spawn `[x, y, z]` per seed. Bamboo calls the same
//! in-game `setInitialSpawn` code via `fresh_spawn_sample` and compares, 0
//! mismatches to pass. `--break-self-test` corrupts one spawn to confirm the
//! detector fails.

use std::process::Command;

use mc_bamboo_game::world_options::{LevelType, WorldGenConfig};
use serde_json::Value;

const USAGE: &str = "usage: spawn <java> [--seeds N] [--salt S] [--show N] \
     [--level-type normal|large_biomes|amplified] [--break-self-test]";
const DEFAULT_SHOW: usize = 8;

#[derive(Debug)]
struct Options {
    forwarded: Vec<String>,
    show: usize,
    level_type: LevelType,
    settings_bare: &'static str,
    break_self_test: bool,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            forwarded: Vec::new(),
            show: DEFAULT_SHOW,
            level_type: LevelType::Normal,
            settings_bare: "overworld",
            break_self_test: false,
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--seeds" | "--salt" => {
                    let value = args.next().ok_or(USAGE)?;
                    options
                        .forwarded
                        .extend([arg.to_owned(), (*value).to_owned()]);
                }
                "--show" => {
                    options.show = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?;
                }
                "--level-type" => {
                    let value = args.next().ok_or(USAGE)?;
                    let level = LevelType::from_level_id(value)
                        .filter(|l| !l.is_flat())
                        .ok_or(USAGE)?;
                    let bare = level
                        .settings_id()
                        .and_then(|id| id.strip_prefix("minecraft:"))
                        .ok_or(USAGE)?;
                    options.level_type = level;
                    options.settings_bare = bare;
                    options
                        .forwarded
                        .extend(["--settings".to_owned(), bare.to_owned()]);
                }
                "--break-self-test" => options.break_self_test = true,
                _ => return Err(USAGE.to_owned()),
            }
        }
        Ok(options)
    }
}

#[derive(Debug, Default)]
struct Stats {
    seeds: u64,
    mismatches: Vec<String>,
}

/// 启动官方 `SpawnOracle.java`，逐行比较出生区块和出生点。
/// Launch the official `SpawnOracle.java` and compare spawn chunk / position
/// line by line.
pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let output = command
        .args(&options.forwarded)
        .output()
        .map_err(|error| format!("cannot run java: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "SpawnOracle.java failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|error| format!("oracle stdout is not UTF-8: {error}"))?;

    let mut stats = Stats::default();
    let mut first = true;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value =
            serde_json::from_str(line).map_err(|error| format!("bad oracle line: {error}"))?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or("oracle line has no type")?;
        if kind != "spawn" {
            return Err(format!("unexpected oracle line type {kind}"));
        }
        let seed = value
            .get("seed")
            .and_then(Value::as_i64)
            .ok_or("spawn line missing seed")?;
        let settings = value
            .get("settings")
            .and_then(Value::as_str)
            .ok_or("spawn line missing settings")?;
        if settings != options.settings_bare {
            return Err(format!(
                "spawn oracle used settings {settings}, expected {}",
                options.settings_bare
            ));
        }
        let origin = int_array(&value, "origin", 2)?;
        let spawn = int_array(&value, "spawn", 3)?;

        let config = WorldGenConfig {
            seed,
            level_type: options.level_type,
            ..WorldGenConfig::default()
        };
        let (ours_origin, mut ours_spawn) = mc_bamboo_game::fresh_spawn_sample(&config)
            .ok_or_else(|| format!("game spawn selection failed for seed {seed}"))?;
        if options.break_self_test && first {
            // 自检：把第一个出生点的 y 改坏，确认下面会报不一致。
            // Self-test: corrupt the first spawn's y so the comparison must fail.
            ours_spawn.1 += 1;
            first = false;
        }
        stats.seeds += 1;
        let want_origin = (origin[0] as i32, origin[1] as i32);
        let want_spawn = (spawn[0] as i32, spawn[1] as i32, spawn[2] as i32);
        if ours_origin != want_origin {
            stats.mismatches.push(format!(
                "seed {seed} origin: official {want_origin:?}, game {ours_origin:?}"
            ));
        }
        if ours_spawn != want_spawn {
            stats.mismatches.push(format!(
                "seed {seed} spawn: official {want_spawn:?}, game {ours_spawn:?}"
            ));
        }
    }
    finish(stats, &options)
}

fn int_array(value: &Value, key: &str, len: usize) -> Result<Vec<i64>, String> {
    let array = value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("spawn line missing {key}"))?;
    let parsed: Vec<i64> = array
        .iter()
        .map(|entry| {
            entry
                .as_i64()
                .ok_or_else(|| format!("{key} entry is not an int"))
        })
        .collect::<Result<_, _>>()?;
    if parsed.len() != len {
        return Err(format!(
            "{key} has {} entries, expected {len}",
            parsed.len()
        ));
    }
    Ok(parsed)
}

fn finish(stats: Stats, options: &Options) -> Result<(), String> {
    let count = stats.mismatches.len();
    println!("spawn: {} seeds, {} mismatches", stats.seeds, count);
    if options.break_self_test {
        if count == 0 {
            return Err(
                "self-test broke a spawn but spawn reported 0 mismatches; the detector is broken"
                    .to_owned(),
            );
        }
        println!("spawn self-test: detector works ({count} mismatches from the deliberate break)");
        return Ok(());
    }
    if stats.seeds == 0 {
        return Err("spawn compared no seeds".to_owned());
    }
    if count != 0 {
        for message in stats.mismatches.iter().take(options.show) {
            eprintln!("  MISMATCH {message}");
        }
        return Err(format!("spawn found {count} mismatches"));
    }
    println!("spawn: 0 mismatches");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_validate_and_forward_seed_args() {
        let options = Options::parse(&["--seeds", "4", "--salt", "9"]).unwrap();
        assert_eq!(options.forwarded, vec!["--seeds", "4", "--salt", "9"]);
        assert_eq!(options.level_type, LevelType::Normal);
        assert!(Options::parse(&["--bogus"]).is_err());
        assert!(
            Options::parse(&["--break-self-test"])
                .unwrap()
                .break_self_test
        );
    }

    #[test]
    fn level_type_forwards_the_settings_id() {
        let large = Options::parse(&["--level-type", "large_biomes"]).unwrap();
        assert_eq!(large.level_type, LevelType::LargeBiomes);
        assert_eq!(large.settings_bare, "large_biomes");
        assert!(
            large
                .forwarded
                .windows(2)
                .any(|w| w[0] == "--settings" && w[1] == "large_biomes")
        );
        let amplified = Options::parse(&["--level-type", "minecraft:amplified"]).unwrap();
        assert_eq!(amplified.level_type, LevelType::Amplified);
        // flat 不是噪声世界，不接受。/ flat is not a noise world, so it is rejected.
        assert!(Options::parse(&["--level-type", "flat"]).is_err());
    }

    #[test]
    fn int_array_checks_length() {
        let value: Value = serde_json::json!({"origin": [1, 2], "spawn": [3, 4, 5]});
        assert_eq!(int_array(&value, "origin", 2).unwrap(), vec![1, 2]);
        assert_eq!(int_array(&value, "spawn", 3).unwrap(), vec![3, 4, 5]);
        assert!(int_array(&value, "origin", 3).is_err());
    }
}
