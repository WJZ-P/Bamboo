//! 要塞环位置 / `findBiomeHorizontal` 的对拍：用官方 26.3 的
//! `ChunkGeneratorStructureState.getRingPositionsFor` 和 `BiomeSource.findBiomeHorizontal`
//! 比 `mc-bamboo-worldgen::structure::rings`。
//!
//! Java 侧 `StructureRingsOracle.java` 读官方注册表、造 `NoiseBasedChunkGenerator` 和
//! `RandomState`，输出每个种子的 128 个环位置和若干 `findBiomeHorizontal` 结果；这里用
//! 生产代码 `ring_positions` / `find_biome_horizontal` 在同一批种子上复算，逐项比较。
//!
//! Oracle for stronghold ring positions / `findBiomeHorizontal`: compare
//! `mc-bamboo-worldgen::structure::rings` against vanilla
//! `ChunkGeneratorStructureState.getRingPositionsFor` and
//! `BiomeSource.findBiomeHorizontal`. The Java side loads the official registry and
//! emits 128 ring positions plus several search results per seed; Rust recomputes with
//! the production functions and compares every item.

use std::{collections::HashMap, process::Command};

use mc_bamboo_blocks::LegacyRandom;
use mc_bamboo_worldgen::{
    Worldgen,
    settings::SeededRouter,
    structure::{
        StructureSets, StructureState,
        rings::{
            BiomeResolver, FindBiomeParams, find_biome_horizontal, resolve_allowed_biomes,
            ring_positions,
        },
    },
};

const PREFERRED_BIOMES: &str = "#minecraft:stronghold_biased_to";

pub fn run(mut command: Command, _options: &[&str]) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|error| format!("cannot run StructureRingsOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "StructureRingsOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let worldgen = Worldgen::load()?;
    let settings = worldgen
        .settings("minecraft:overworld")
        .ok_or("missing minecraft:overworld settings")?;
    let source = worldgen
        .preset("minecraft:overworld")
        .ok_or("missing minecraft:overworld preset")?;
    let allowed = resolve_allowed_biomes(PREFERRED_BIOMES)?;
    let sets = StructureSets::parse(
        worldgen
            .raw()
            .get("structure_set")
            .ok_or("worldgen.json missing structure_set")?,
    )?;

    let mut router: Option<(i64, SeededRouter)> = None;
    let mut rings_by_seed: HashMap<i64, Vec<(i32, i32)>> = HashMap::new();
    let mut mismatches = Vec::new();
    let mut total_mismatches = 0usize;
    let mut ring_checks = 0usize;
    let mut biome_checks = 0usize;
    let mut isc_checks = 0usize;
    let mut ring_seeds = 0usize;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines().peekable();
    while let Some(line) = lines.next() {
        if let Some(rest) = line.strip_prefix("RINGS ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let seed: i64 = parts[0].parse().map_err(|_| format!("bad RINGS: {line}"))?;
            let count: usize = parts[1]
                .parse()
                .map_err(|_| format!("bad RINGS count: {line}"))?;
            let mut official = Vec::with_capacity(count);
            for _ in 0..count {
                let pos = lines.next().ok_or("missing ring position")?;
                let coords: Vec<&str> = pos.split_whitespace().collect();
                official.push((
                    coords[0]
                        .parse::<i32>()
                        .map_err(|_| format!("bad x: {pos}"))?,
                    coords[1]
                        .parse::<i32>()
                        .map_err(|_| format!("bad z: {pos}"))?,
                ));
            }
            // 每个种子新建 router：气候采样器依赖种子。
            // A fresh router per seed: the climate sampler depends on the seed.
            let mut fresh = SeededRouter::new(settings, seed);
            source.reset();
            let mut resolver = BiomeResolver {
                router: &mut fresh,
                source,
            };
            let ours = ring_positions(seed, &mut resolver, &allowed, 32, 3, 128);
            ring_seeds += 1;
            if ours.len() != official.len() {
                return Err(format!(
                    "seed {seed}: ring count {} != official {}",
                    ours.len(),
                    official.len()
                ));
            }
            for (index, (got, want)) in ours.iter().zip(&official).enumerate() {
                ring_checks += 1;
                if got != want {
                    total_mismatches += 1;
                    if mismatches.len() < 24 {
                        mismatches.push(format!(
                            "RINGS seed={seed} [{index}]: ours {got:?} vs {want:?}"
                        ));
                    }
                }
            }
            router = Some((seed, fresh));
            rings_by_seed.insert(seed, ours);
            continue;
        }
        if let Some(rest) = line.strip_prefix("ISC ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let seed: i64 = parts[0].parse().map_err(|_| format!("bad ISC: {line}"))?;
            let count: usize = parts[1]
                .parse()
                .map_err(|_| format!("bad ISC count: {line}"))?;
            let positions = rings_by_seed
                .get(&seed)
                .ok_or_else(|| format!("ISC before RINGS for seed {seed}"))?;
            let mut state = StructureState::new(sets.clone());
            state.set_ring_positions("minecraft:strongholds", positions.iter().copied());
            for _ in 0..count {
                let check = lines.next().ok_or("missing ISC entry")?;
                let fields: Vec<&str> = check.split_whitespace().collect();
                let x: i32 = fields[0].parse().map_err(|_| format!("bad x: {check}"))?;
                let z: i32 = fields[1].parse().map_err(|_| format!("bad z: {check}"))?;
                let want = fields[2] == "1";
                let got = state.is_structure_chunk("minecraft:strongholds", seed, x, z);
                isc_checks += 1;
                if got != want {
                    total_mismatches += 1;
                    if mismatches.len() < 24 {
                        mismatches.push(format!("ISC seed={seed} ({x},{z}): ours {got} vs {want}"));
                    }
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("B ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let seed: i64 = parts[0].parse().map_err(|_| format!("bad B: {line}"))?;
            let ox: i32 = parts[1].parse().map_err(|_| format!("bad ox: {line}"))?;
            let oz: i32 = parts[2].parse().map_err(|_| format!("bad oz: {line}"))?;
            // 搜索随机源：和 Java 侧 `seed ^ (ox*341873128712 + oz*132897987541)` 一致。
            // Search source: matches Java `seed ^ (ox*341873128712 + oz*132897987541)`.
            let mixed = i64::from(ox)
                .wrapping_mul(341_873_128_712)
                .wrapping_add(i64::from(oz).wrapping_mul(132_897_987_541));
            let mut random = LegacyRandom::new(seed ^ mixed);
            if router.as_ref().is_none_or(|(s, _)| *s != seed) {
                let fresh = SeededRouter::new(settings, seed);
                source.reset();
                router = Some((seed, fresh));
            }
            let (_, active) = router.as_mut().ok_or("missing router")?;
            let mut resolver = BiomeResolver {
                router: active,
                source,
            };
            let params = FindBiomeParams {
                origin_x: ox,
                origin_y: 0,
                origin_z: oz,
                search_radius: 112,
                skip_steps: 1,
                find_closest: false,
            };
            let ours = find_biome_horizontal(&mut resolver, params, &allowed, &mut random);
            biome_checks += 1;
            let want = if parts[3] == "NONE" {
                None
            } else {
                Some((
                    parts[3]
                        .parse::<i32>()
                        .map_err(|_| format!("bad rx: {line}"))?,
                    parts[4]
                        .parse::<i32>()
                        .map_err(|_| format!("bad rz: {line}"))?,
                    parts[5].to_owned(),
                ))
            };
            let ours_pos = ours.as_ref().map(|(x, z, biome)| (*x, *z, biome.clone()));
            if ours_pos != want {
                total_mismatches += 1;
                if mismatches.len() < 24 {
                    mismatches.push(format!(
                        "B seed={seed} ({ox},{oz}): ours {ours_pos:?} vs {want:?}"
                    ));
                }
            }
        }
    }

    println!(
        "structure-rings: {ring_seeds} seeds, {ring_checks} ring positions, {biome_checks} findBiomeHorizontal checks, {isc_checks} isStructureChunk checks"
    );
    if total_mismatches == 0 {
        println!("STRUCTURE-RINGS RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in &mismatches {
            println!("STRUCTURE-RINGS FAIL: {line}");
        }
        Err(format!(
            "STRUCTURE-RINGS RESULT: {total_mismatches} mismatches"
        ))
    }
}
