//! `natural-spawn`：把自然刷怪的加权选择和组大小与官方逐个对拍。
//!
//! `NaturalSpawnOracle.java` 对每个群系的每个 `MobCategory` 用固定种子的官方
//! `RandomSource` 跑 8 次 `WeightedList.getRandom` + `IntProvider.sample`，再输出一个
//! 「之后 `nextInt(1000)`」的指纹；另外把 0..15 交给官方 `Util.shuffle`。
//! Bamboo 侧用生成的刷怪表和同一套取数顺序重放，挑中的生物、组大小、打乱结果和指纹
//! 都要逐个相同。`--break-self-test` 故意改坏一项确认会被抓到。
//! 随机起点和区块中心距离也由官方方法给出，Bamboo 用运行时的实现重放。
//!
//! `natural-spawn`: compare the weighted spawn selection and group sizes against
//! vanilla. `NaturalSpawnOracle.java` runs `WeightedList.getRandom` plus
//! `IntProvider.sample` eight times per biome and category with a seeded official
//! `RandomSource` and appends a `nextInt(1000)` fingerprint, and shuffles 0..15 with
//! vanilla `Util.shuffle`. Bamboo replays the same draws over the exported tables and
//! must match the picked type, the count, the shuffled order and the fingerprint.
//! `--break-self-test` corrupts one pick to prove the detector works.
//! Vanilla also supplies the random origin and chunk distance for runtime replay.

use std::process::Command;

use mc_bamboo_blocks::entity::mob::natural;
use mc_bamboo_core::{ChunkPos, Vec3};
use mc_bamboo_data::spawning::{self, MobCategory, MobSpawnerDef};
use serde_json::Value;

const USAGE: &str = "usage: natural-spawn <java> [--show N] [--break-self-test]";
const DEFAULT_SHOW: usize = 8;

#[derive(Debug)]
struct Options {
    show: usize,
    break_self_test: bool,
}

impl Options {
    fn parse(args: &[&str]) -> Result<Self, String> {
        let mut options = Self {
            show: DEFAULT_SHOW,
            break_self_test: false,
        };
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            match arg {
                "--show" => {
                    options.show = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?;
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
    cases: u64,
    mismatches: Vec<String>,
}

/// 启动官方 `NaturalSpawnOracle.java`，逐行重放。
///
/// Launch the official `NaturalSpawnOracle.java` and replay every line.
pub fn run(mut command: Command, args: &[&str]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let output = command
        .output()
        .map_err(|error| format!("cannot run java: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "NaturalSpawnOracle.java failed: {}\n{}",
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
        let mismatch = match kind {
            "pick" => replay_pick(&value, options.break_self_test && first)?,
            "shuffle" => replay_shuffle(&value, options.break_self_test && first)?,
            "origin" => replay_origin(&value)?,
            "chunk_distance" => replay_chunk_distance(&value)?,
            other => return Err(format!("unexpected oracle line type {other}")),
        };
        first = false;
        stats.cases += 1;
        if let Some(message) = mismatch {
            stats.mismatches.push(message);
        }
    }
    finish(stats, &options)
}

fn replay_origin(value: &Value) -> Result<Option<String>, String> {
    let seed = value["seed"].as_i64().ok_or("origin seed missing")?;
    let empty = value["empty"]
        .as_bool()
        .ok_or("origin empty flag missing")?;
    let want = value["positions"]
        .as_array()
        .ok_or("origin positions missing")?;
    let random = mc_bamboo_blocks::LegacyRandom::new(seed);
    let ours: Vec<_> = (0..want.len())
        .map(|_| {
            let pos = natural::random_pos_within(&random, ChunkPos::new(-3, 2), -64, |x, z| {
                if empty { -64 } else { 64 + (x & 15) - (z & 15) }
            });
            serde_json::json!([pos.x, pos.y, pos.z])
        })
        .collect();
    let after = random.next_int(1000);
    Ok((ours != *want || value["after"] != after).then(|| {
        format!(
            "origin seed {seed} empty {empty}: official {want:?} then {}, game {ours:?} then {after}",
            value["after"]
        )
    }))
}

fn replay_chunk_distance(value: &Value) -> Result<Option<String>, String> {
    let x = value["x"].as_f64().ok_or("chunk distance x missing")?;
    let z = value["z"].as_f64().ok_or("chunk distance z missing")?;
    let want = value["close"]
        .as_bool()
        .ok_or("chunk distance outcome missing")?;
    let ours = natural::is_close_to_spawn_chunk(Vec3::new(x, 999.0, z), ChunkPos::new(-3, 2));
    Ok((want != ours)
        .then(|| format!("chunk distance at ({x}, {z}): official {want}, game {ours}")))
}

fn replay_pick(value: &Value, break_self_test: bool) -> Result<Option<String>, String> {
    let biome = value
        .get("biome")
        .and_then(Value::as_str)
        .ok_or("pick line missing biome")?;
    let category_name = value
        .get("category")
        .and_then(Value::as_str)
        .ok_or("pick line missing category")?;
    let seed = value
        .get("seed")
        .and_then(Value::as_i64)
        .ok_or("pick line missing seed")?;
    let category = MobCategory::ALL
        .into_iter()
        .find(|category| category.facts().name == category_name)
        .ok_or_else(|| format!("unknown mob category {category_name}"))?;
    let settings =
        spawning::by_biome_name(biome).ok_or_else(|| format!("unknown biome {biome}"))?;
    let table: &[MobSpawnerDef] = settings
        .categories
        .iter()
        .find(|row| row.category == category.index() as u8)
        .map_or(&[], |row| row.entries);

    let official = value
        .get("picks")
        .and_then(Value::as_array)
        .ok_or("pick line missing picks")?;
    let random = mc_bamboo_blocks::LegacyRandom::new(seed);
    let mut ours = Vec::new();
    for _index in 0..official.len() {
        let Some(picked) = natural::pick(table, &random) else {
            ours.push(("empty".to_owned(), 0));
            continue;
        };
        let count = natural::sample(picked, &random);
        let name = mc_bamboo_data::registries::entity_type::name_of(picked.entity_type)
            .unwrap_or("<unknown>")
            .to_owned();
        ours.push((name, count));
    }
    let after = random.next_int(1000);
    let want_after = value
        .get("after")
        .and_then(Value::as_i64)
        .ok_or("pick line missing after")?;

    let mut problems = Vec::new();
    for (index, entry) in official.iter().enumerate() {
        let (want_type, want_count) = if let Some(array) = entry.as_array() {
            (
                array
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or("empty")
                    .to_owned(),
                array.get(1).and_then(Value::as_i64).unwrap_or(0),
            )
        } else {
            ("empty".to_owned(), 0)
        };
        let (have_type, have_count) = ours.get(index).cloned().unwrap_or_default();
        let broken = break_self_test && index == 0;
        if broken || want_type != have_type || want_count != i64::from(have_count) {
            problems.push(format!(
                "{biome}/{category_name} seed {seed} pick {index}: official {want_type} x{want_count}, game {have_type} x{have_count}"
            ));
        }
    }
    if i64::from(after) != want_after {
        problems.push(format!(
            "{biome}/{category_name} seed {seed}: official after {want_after}, game {after}"
        ));
    }
    Ok(if problems.is_empty() {
        None
    } else {
        Some(problems.join("; "))
    })
}

fn replay_shuffle(value: &Value, break_self_test: bool) -> Result<Option<String>, String> {
    let seed = value
        .get("seed")
        .and_then(Value::as_i64)
        .ok_or("shuffle line missing seed")?;
    let size = value
        .get("size")
        .and_then(Value::as_u64)
        .ok_or("shuffle line missing size")? as usize;
    let want: Vec<usize> = value
        .get("order")
        .and_then(Value::as_array)
        .ok_or("shuffle line missing order")?
        .iter()
        .map(|entry| entry.as_u64().unwrap_or(0) as usize)
        .collect();
    let random = mc_bamboo_blocks::LegacyRandom::new(seed);
    let mut ours: Vec<usize> = (0..size).collect();
    mc_bamboo_blocks::shuffle(&random, &mut ours);
    if break_self_test {
        ours.swap(0, 1);
    }
    Ok((ours != want)
        .then(|| format!("shuffle size {size} seed {seed}: official {want:?}, game {ours:?}")))
}

fn finish(stats: Stats, options: &Options) -> Result<(), String> {
    let count = stats.mismatches.len();
    println!("natural-spawn: {} cases, {} mismatches", stats.cases, count);
    if options.break_self_test {
        if count == 0 {
            return Err(
                "self-test broke a pick but natural-spawn reported 0 mismatches; the detector is broken"
                    .to_owned(),
            );
        }
        println!(
            "natural-spawn self-test: detector works ({count} mismatches from the deliberate break)"
        );
        return Ok(());
    }
    if stats.cases == 0 {
        return Err("natural-spawn compared no cases".to_owned());
    }
    if count != 0 {
        for message in stats.mismatches.iter().take(options.show) {
            eprintln!("  MISMATCH {message}");
        }
        return Err(format!("natural-spawn found {count} mismatches"));
    }
    println!("natural-spawn: 0 mismatches");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_parse_and_reject_unknown() {
        assert!(Options::parse(&["--show", "3"]).unwrap().show == 3);
        assert!(
            Options::parse(&["--break-self-test"])
                .unwrap()
                .break_self_test
        );
        assert!(Options::parse(&["--bogus"]).is_err());
    }
}
