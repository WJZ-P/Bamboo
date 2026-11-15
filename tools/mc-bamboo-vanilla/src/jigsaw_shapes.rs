//! `Shapes` AABB 布尔运算的独立小场景对拍。
//!
//! 每个场景模拟 addPieces 里一段 `MutableObject<VoxelShape>` 的生命周期：
//! 初始 `contextFree = join(big, center, ONLY_FIRST)` 或 `sourceFree = create(sourceBB)`，
//! 然后一串 target 依次做 `joinIsNotEmpty(free, deflate(target, 0.25), ONLY_SECOND)`，
//! 通过的再 `joinUnoptimized(free, target, ONLY_FIRST)`。Java 输出每次判定，
//! Rust 用生产 `structure::jigsaw::shapes::FreeSpace` 复算，逐场景比。
//!
//! Independent small-scene oracle for `Shapes` AABB booleans. Each scene is one
//! `MutableObject<VoxelShape>` lifetime: an initial `contextFree` or `sourceFree`,
//! then a sequence of `joinIsNotEmpty(free, deflate(target, 0.25), ONLY_SECOND)`
//! calls, placing (subtracting) on each pass. Rust recomputes with the production
//! `FreeSpace` and compares per scene.

use std::{fs, path::Path, process::Command};

use mc_bamboo_worldgen::structure::jigsaw::shapes::{Box4, FreeSpace};

struct Scene {
    id: String,
    kind: &'static str,
    /// `new AABB(b[0..3], b[3..6])` 的直接值。
    big: [i32; 6],
    /// `BoundingBox` 的六个 int。
    initial: [i32; 6],
    targets: Vec<[i32; 6]>,
}

pub fn run(mut command: Command, work_dir: &Path, count_arg: Option<&&str>) -> Result<(), String> {
    let count = count_arg
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(400)
        .clamp(1, 1000);
    let scenes = build_scenes(count);
    let queries_path = work_dir.join("jigsaw_shapes_queries.txt");
    fs::write(&queries_path, serialize(&scenes))
        .map_err(|error| format!("cannot write queries: {error}"))?;

    let output = command
        .arg("shapes")
        .arg(&queries_path)
        .output()
        .map_err(|error| format!("cannot run JigsawOracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "JigsawOracle failed: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    compare(&String::from_utf8_lossy(&output.stdout), &scenes)
}

fn serialize(scenes: &[Scene]) -> String {
    let mut text = String::new();
    for scene in scenes {
        text.push_str(&scene.id);
        text.push('|');
        text.push_str(scene.kind);
        for value in scene.big.iter().chain(scene.initial.iter()) {
            text.push('|');
            text.push_str(&value.to_string());
        }
        text.push('|');
        text.push_str(&scene.targets.len().to_string());
        for target in &scene.targets {
            for value in target {
                text.push('|');
                text.push_str(&value.to_string());
            }
        }
        text.push('\n');
    }
    text
}

/// 手工边界场景 + 确定性 LCG 的随机场景，覆盖贴边 / 重叠 / 超出 / 连续放置。
fn build_scenes(count: usize) -> Vec<Scene> {
    let mut scenes = Vec::new();
    // 手工：贴 center 边、与 center 重叠、超出 big、完全包含、分离、连续放置。
    let big = [0, 0, 0, 20, 20, 20];
    let center = [5, 5, 5, 9, 9, 9];
    let hand = [
        vec![[10, 5, 5, 12, 9, 9], [4, 5, 5, 6, 9, 9]], // 贴边 / 重叠
        vec![[0, 0, 0, 1, 1, 1], [0, 0, 0, 1, 1, 1]],   // 重复放置同一格
        vec![[-2, 5, 5, 1, 9, 9], [18, 5, 5, 22, 9, 9]], // 超出 big 两侧
        vec![[5, 5, 5, 9, 9, 9], [5, 5, 5, 9, 9, 9]],   // 正好等于 center
        vec![[10, 10, 10, 11, 11, 11], [10, 10, 10, 11, 11, 11]], // 连续：第二次应被挡
        vec![[9, 5, 5, 10, 9, 9], [10, 5, 5, 11, 9, 9]], // deflate 后恰好贴边
    ];
    for (index, targets) in hand.into_iter().enumerate() {
        scenes.push(Scene {
            id: format!("hand_context_{index}"),
            kind: "context",
            big,
            initial: center,
            targets: targets.clone(),
        });
        scenes.push(Scene {
            id: format!("hand_source_{index}"),
            kind: "source",
            big,
            initial: center,
            targets,
        });
    }

    let mut lcg = Lcg(0x9e37_79b9_7f4a_7c15);
    while scenes.len() < count {
        let kind = if lcg.next() & 1 == 0 {
            "context"
        } else {
            "source"
        };
        let initial = random_box(&mut lcg, 0, 12, 1, 5);
        let targets: Vec<[i32; 6]> = (0..3 + (lcg.next() as usize % 4))
            .map(|_| random_box(&mut lcg, 0, 16, 1, 5))
            .collect();
        scenes.push(Scene {
            id: format!("lcg_{}", scenes.len()),
            kind,
            big,
            initial,
            targets,
        });
    }
    scenes
}

fn random_box(lcg: &mut Lcg, origin: i32, span: i32, min_size: i32, max_size: i32) -> [i32; 6] {
    let min_x = origin + (lcg.next() % span);
    let min_y = origin + (lcg.next() % span);
    let min_z = origin + (lcg.next() % span);
    let size = min_size + (lcg.next() % (max_size - min_size + 1));
    [
        min_x,
        min_y,
        min_z,
        min_x + size,
        min_y + size,
        min_z + size,
    ]
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> i32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        (self.0 >> 33) as i32
    }
}

fn bounds_to_box4(bounds: &[i32; 6]) -> Box4 {
    Box4::from_aabb(
        [bounds[0], bounds[1], bounds[2]],
        [bounds[3] + 1, bounds[4] + 1, bounds[5] + 1],
    )
}

fn simulate(scene: &Scene) -> String {
    let big = Box4::from_aabb(
        [scene.big[0], scene.big[1], scene.big[2]],
        [scene.big[3], scene.big[4], scene.big[5]],
    );
    let initial = bounds_to_box4(&scene.initial);
    let mut free = if scene.kind == "context" {
        FreeSpace::context(big, initial)
    } else {
        FreeSpace::source(initial)
    };
    let mut results = String::new();
    for target in &scene.targets {
        let box4 = bounds_to_box4(target);
        let blocked = free.join_is_not_empty(&box4.deflate_quarter());
        results.push(if blocked { '1' } else { '0' });
        if !blocked {
            free.subtract(box4);
        }
    }
    results
}

/// 和 Java `deflateCases` 一致，用来直接验证 `deflate(0.25)` 的乘 4 边界。
const DEFLATE_CASES: [[i32; 6]; 4] = [
    [0, 0, 0, 0, 0, 0],
    [5, 5, 5, 9, 9, 9],
    [-3, -2, -1, 4, 6, 8],
    [10, 0, 0, 10, 0, 0],
];

fn compare(stdout: &str, scenes: &[Scene]) -> Result<(), String> {
    let mut java: Vec<(&str, &str)> = Vec::new();
    let mut deflate_lines: Vec<[i64; 6]> = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.first() == Some(&"D") {
            let mut values = [0i64; 6];
            for (index, value) in values.iter_mut().enumerate() {
                *value = parts
                    .get(index + 1)
                    .and_then(|text| text.parse::<i64>().ok())
                    .ok_or("bad D line")?;
            }
            deflate_lines.push(values);
        } else if parts.len() >= 2 {
            java.push((parts[0], parts[1]));
        }
    }
    if java.len() != scenes.len() {
        return Err(format!(
            "Java reported {} scenes, generated {}",
            java.len(),
            scenes.len()
        ));
    }
    let mut mismatches = 0usize;
    let mut first: Option<String> = None;
    for ((id, got), scene) in java.iter().zip(scenes) {
        let want = simulate(scene);
        if *got != want {
            mismatches += 1;
            if first.is_none() {
                first = Some(format!("{id}: java {got} rust {want}"));
            }
        }
    }
    if mismatches > 0 {
        return Err(format!(
            "{mismatches}/{} scenes differ; first: {}",
            scenes.len(),
            first.unwrap_or_default()
        ));
    }

    // D 行：`AABB.of(bb).deflate(0.25)` 的乘 4 边界。
    if deflate_lines.len() != DEFLATE_CASES.len() {
        return Err(format!(
            "Java reported {} D lines, expected {}",
            deflate_lines.len(),
            DEFLATE_CASES.len()
        ));
    }
    for (got, case) in deflate_lines.iter().zip(DEFLATE_CASES) {
        let box4 = bounds_to_box4(&case).deflate_quarter();
        let want = [
            box4.min[0],
            box4.min[1],
            box4.min[2],
            box4.max[0],
            box4.max[1],
            box4.max[2],
        ];
        if got != &want {
            return Err(format!("deflate mismatch: java {got:?} rust {want:?}"));
        }
    }

    println!(
        "jigsaw shapes: {} scenes + {} deflate cases, all join/joinIsNotEmpty/joinUnoptimized results identical",
        scenes.len(),
        deflate_lines.len()
    );
    Ok(())
}
