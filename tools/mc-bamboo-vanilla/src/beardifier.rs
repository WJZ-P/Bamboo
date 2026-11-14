//! `Beardifier` 纯密度贡献对拍：官方 `BeardifierOracle` 对
//! `mc-bamboo-worldgen::structure::beardifier` 的纯函数实现。
//!
//! 每条查询用官方 `@VisibleForTesting public` 构造器自造 `Beardifier`（`Rigid` / `Junction` /
//! `affectedBox`），再逐点比 `sampleValue` 与整块 `sampleVolume` 的 float 位。覆盖五个
//! `TerrainAdjustment`、junction、EMPTY、affectedBox 内外、步长 >1 的体积。任一 float 位不同
//! 即报不一致。
//!
//! Oracle for the pure `Beardifier` density contribution: vanilla `BeardifierOracle` vs
//! `mc-bamboo-worldgen::structure::beardifier`. Each query builds a `Beardifier` via the public
//! test constructor and compares `sampleValue` / `sampleVolume` float bits, covering all five
//! `TerrainAdjustment`s, junctions, EMPTY, inside/outside `affectedBox`, step >1 volumes.

use std::{fs, path::Path, process::Command};

use mc_bamboo_worldgen::{
    structure::beardifier::{Beardifier, Junction, Rigid, TerrainAdjustment},
    volume::Volume,
};

/// 一个用例：`Rigid` / `Junction` / `affectedBox` + 采样点 + 一块体积。
///
/// One case: `Rigid` / `Junction` / `affectedBox` + sample points + a volume.
struct Case {
    rigids: Vec<Rigid>,
    junctions: Vec<Junction>,
    affected_box: Option<[i32; 6]>,
    points: Vec<(i32, i32, i32)>,
    volume: Volume,
}

pub fn run(mut command: Command, queries_path: &Path, _options: &[&str]) -> Result<(), String> {
    let cases = build_cases();
    write_queries(queries_path, &cases)?;

    let output = command
        .arg(queries_path)
        .output()
        .map_err(|error| format!("cannot run BeardifierOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "BeardifierOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare(&cases, &String::from_utf8_lossy(&output.stdout))
}

/// 覆盖各分支的确定性用例集。
///
/// Deterministic case set covering all branches.
fn build_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let boxes: [[i32; 6]; 6] = [
        [0, 60, 0, 10, 70, 10],
        [-8, 55, -8, 8, 65, 8],
        [20, 40, 20, 35, 50, 35],
        [-30, 70, -30, -20, 90, -20],
        [0, 62, 0, 15, 62, 15],
        [-5, 63, -5, 5, 72, 5],
    ];
    let glds = [0, 3, -2, 5];

    // 单个 rigid × 五种 adjustment × 若干 groundLevelDelta。
    // One rigid × five adjustments × several groundLevelDelta values.
    for (ai, adj) in TerrainAdjustment::ALL.iter().enumerate() {
        for (gi, &gld) in glds.iter().enumerate() {
            let b = boxes[(ai + gi) % boxes.len()];
            cases.push(Case {
                rigids: vec![Rigid {
                    box_: b,
                    terrain_adjustment: *adj,
                    ground_level_delta: gld,
                }],
                junctions: Vec::new(),
                affected_box: Some(inflated(b, 24)),
                points: sample_points(b),
                volume: Volume::new((4, 4, 4), (b[0] - 6, b[1] - 6, b[2] - 6), (2, 2, 2)),
            });
        }
    }

    // 多 rigid + junction，affectedBox 覆盖并集外扩。
    // Multiple rigids + junctions, affectedBox = union inflated.
    let b0 = boxes[0];
    let b1 = boxes[2];
    let union = encapsulating(b0, b1);
    cases.push(Case {
        rigids: vec![
            Rigid {
                box_: b0,
                terrain_adjustment: TerrainAdjustment::BeardThin,
                ground_level_delta: 2,
            },
            Rigid {
                box_: b1,
                terrain_adjustment: TerrainAdjustment::Encapsulate,
                ground_level_delta: -1,
            },
            Rigid {
                box_: boxes[5],
                terrain_adjustment: TerrainAdjustment::Bury,
                ground_level_delta: 0,
            },
        ],
        junctions: vec![
            Junction {
                source_x: 5,
                source_ground_y: 62,
                source_z: 5,
            },
            Junction {
                source_x: 30,
                source_ground_y: 45,
                source_z: 30,
            },
        ],
        affected_box: Some(inflated(union, 24)),
        points: sample_points(union),
        volume: Volume::new((5, 5, 5), (-10, 40, -10), (3, 4, 3)),
    });

    // EMPTY：没有结构件、affectedBox 为 None，处处 0。
    // EMPTY: no pieces, None affectedBox, zero everywhere.
    cases.push(Case {
        rigids: Vec::new(),
        junctions: Vec::new(),
        affected_box: None,
        points: vec![(0, 64, 0), (100, 30, -100), (-50, 80, 50)],
        volume: Volume::new((3, 3, 3), (0, 60, 0), (1, 1, 1)),
    });

    // 有 junction 但 affectedBox 不含全部点（测 sampleValue 的 isInside 门）。
    // Junctions with an affectedBox excluding some points (tests the isInside gate).
    let b = boxes[1];
    let small_box = [b[0] + 4, b[1] + 4, b[2] + 4, b[3] - 4, b[4] - 4, b[5] - 4];
    cases.push(Case {
        rigids: vec![Rigid {
            box_: b,
            terrain_adjustment: TerrainAdjustment::BeardBox,
            ground_level_delta: 1,
        }],
        junctions: vec![Junction {
            source_x: 0,
            source_ground_y: 60,
            source_z: 0,
        }],
        affected_box: Some(small_box),
        points: sample_points(b),
        volume: Volume::new((6, 6, 6), (b[0] - 2, b[1] - 2, b[2] - 2), (1, 1, 1)),
    });

    cases
}

fn inflated(b: [i32; 6], n: i32) -> [i32; 6] {
    [b[0] - n, b[1] - n, b[2] - n, b[3] + n, b[4] + n, b[5] + n]
}

fn encapsulating(a: [i32; 6], b: [i32; 6]) -> [i32; 6] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].min(b[2]),
        a[3].max(b[3]),
        a[4].max(b[4]),
        a[5].max(b[5]),
    ]
}

/// 在盒子内外取一串采样点，覆盖 kernel 命中与越界、junction 的负 dx/dy/dz。
///
/// Sample points inside/outside the box, covering kernel hits/misses and negative junction deltas.
fn sample_points(b: [i32; 6]) -> Vec<(i32, i32, i32)> {
    let cx = (b[0] + b[3]) / 2;
    let cy = (b[1] + b[4]) / 2;
    let cz = (b[2] + b[5]) / 2;
    let mut pts = Vec::new();
    for &dx in &[-30, -13, -12, -6, -1, 0, 1, 6, 12, 13, 30] {
        for &dy in &[-30, -12, -6, 0, 6, 12, 30] {
            for &dz in &[-30, -12, -6, 0, 6, 12, 30] {
                pts.push((cx + dx, cy + dy, cz + dz));
            }
        }
    }
    pts
}

fn write_queries(path: &Path, cases: &[Case]) -> Result<(), String> {
    let mut s = String::new();
    for (id, c) in cases.iter().enumerate() {
        s.push_str(&format!(
            "Q {} {} {}\n",
            id,
            c.rigids.len(),
            c.junctions.len()
        ));
        for r in &c.rigids {
            let b = r.box_;
            s.push_str(&format!(
                "R {} {} {} {} {} {} {} {}\n",
                b[0],
                b[1],
                b[2],
                b[3],
                b[4],
                b[5],
                r.terrain_adjustment as i32,
                r.ground_level_delta
            ));
        }
        for j in &c.junctions {
            s.push_str(&format!(
                "J {} {} {}\n",
                j.source_x, j.source_ground_y, j.source_z
            ));
        }
        match c.affected_box {
            Some(b) => s.push_str(&format!(
                "A {} {} {} {} {} {}\n",
                b[0], b[1], b[2], b[3], b[4], b[5]
            )),
            None => s.push_str("A none\n"),
        }
        for &(x, y, z) in &c.points {
            s.push_str(&format!("S {x} {y} {z}\n"));
        }
        let v = c.volume;
        s.push_str(&format!(
            "V {} {} {} {} {} {} {} {} {}\n",
            v.size_x, v.size_y, v.size_z, v.min_x, v.min_y, v.min_z, v.step_x, v.step_y, v.step_z
        ));
    }
    fs::write(path, s).map_err(|error| format!("cannot write queries file: {error}"))
}

/// 解析 Java 输出：每个用例的 `SV` 单点值和 `VOL` 体积缓冲（都是 float 位）。
///
/// Parse Java output: per-case `SV` point values and `VOL` volume buffers, all float bits.
struct JavaCase {
    sv: Vec<i32>,
    vol: Vec<i32>,
}

fn parse_output(stdout: &str, case_count: usize) -> Result<Vec<JavaCase>, String> {
    let mut cases: Vec<JavaCase> = (0..case_count)
        .map(|_| JavaCase {
            sv: Vec::new(),
            vol: Vec::new(),
        })
        .collect();
    let mut expected_points = vec![0usize; case_count];
    let mut vol_seen = vec![false; case_count];
    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        match f[0] {
            "PTS" => {
                let id: usize = f[1].parse().map_err(|_| format!("bad PTS id: {line}"))?;
                expected_points[id] = f[2].parse().map_err(|_| format!("bad PTS count: {line}"))?;
            }
            "SV" => {
                let id: usize = f[1].parse().map_err(|_| format!("bad SV id: {line}"))?;
                let p: usize = f[2].parse().map_err(|_| format!("bad SV idx: {line}"))?;
                let bits: i32 = f[3].parse().map_err(|_| format!("bad SV bits: {line}"))?;
                if p != cases[id].sv.len() {
                    return Err(format!(
                        "SV out of order for case {id}: {p} vs {}",
                        cases[id].sv.len()
                    ));
                }
                cases[id].sv.push(bits);
            }
            "VOL" => {
                let id: usize = f[1].parse().map_err(|_| format!("bad VOL id: {line}"))?;
                let count: usize = f[2].parse().map_err(|_| format!("bad VOL count: {line}"))?;
                for k in 0..count {
                    cases[id].vol.push(
                        f[3 + k]
                            .parse()
                            .map_err(|_| format!("bad VOL bits: {line}"))?,
                    );
                }
                vol_seen[id] = true;
            }
            other => return Err(format!("unknown oracle line kind {other}: {line}")),
        }
    }
    for (id, c) in cases.iter().enumerate() {
        if c.sv.len() != expected_points[id] {
            return Err(format!(
                "case {id}: {} SV lines, expected {}",
                c.sv.len(),
                expected_points[id]
            ));
        }
        if !vol_seen[id] {
            return Err(format!("case {id}: missing VOL"));
        }
    }
    Ok(cases)
}

fn compare(cases: &[Case], stdout: &str) -> Result<(), String> {
    let java = parse_output(stdout, cases.len())?;
    let mut mismatches: Vec<String> = Vec::new();
    let mut mismatch_count = 0usize;
    let mut total = 0usize;

    for (id, (c, j)) in cases.iter().zip(java.iter()).enumerate() {
        let beard = Beardifier {
            pieces: c.rigids.clone(),
            junctions: c.junctions.clone(),
            affected_box: c.affected_box,
        };
        // sampleValue 逐点比 float 位。
        // Compare sampleValue float bits point by point.
        for (p, &(x, y, z)) in c.points.iter().enumerate() {
            total += 1;
            let ours = beard.sample_value(x, y, z).to_bits() as i32;
            if ours != j.sv[p] {
                mismatch_count += 1;
                push(
                    &mut mismatches,
                    format!(
                        "case {id} point {p} ({x},{y},{z}): rust {ours} vs java {}",
                        j.sv[p]
                    ),
                );
            }
        }
        // sampleVolume：把贡献加到清零的缓冲上（对应 final_density 顶层的 add）。
        // sampleVolume: add onto a zeroed buffer (the top-level add in final_density).
        let mut buf = vec![0.0_f32; c.volume.len()];
        beard.add_to_volume(&mut buf, &c.volume);
        if buf.len() != j.vol.len() {
            return Err(format!(
                "case {id}: volume size {} vs {}",
                buf.len(),
                j.vol.len()
            ));
        }
        for (k, &cell) in buf.iter().enumerate() {
            total += 1;
            let ours = cell.to_bits() as i32;
            if ours != j.vol[k] {
                mismatch_count += 1;
                push(
                    &mut mismatches,
                    format!("case {id} vol cell {k}: rust {ours} vs java {}", j.vol[k]),
                );
            }
        }
    }

    let points: usize = cases.iter().map(|c| c.points.len()).sum();
    let vol_cells: usize = cases.iter().map(|c| c.volume.len()).sum();
    println!(
        "beardifier: {} cases, {} sampleValue points, {} volume cells, {} bit comparisons",
        cases.len(),
        points,
        vol_cells,
        total
    );
    if mismatches.is_empty() {
        println!("BEARDIFIER RESULT: 0 mismatches");
        Ok(())
    } else {
        for line in &mismatches {
            println!("BEARDIFIER FAIL: {line}");
        }
        Err(format!("BEARDIFIER RESULT: {mismatch_count} mismatches"))
    }
}

fn push(list: &mut Vec<String>, line: String) {
    if list.len() < 24 {
        list.push(line);
    }
}
