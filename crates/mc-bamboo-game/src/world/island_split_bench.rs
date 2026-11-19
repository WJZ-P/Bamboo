//! 拆分、合并和拼 / 拆整个世界的耗时和岛的大小的关系（`docs/profiling.md` 第 18 节）。
//!
//! 手工跑：`cargo test -p mc-bamboo-game --release --lib island_split_bench -- --ignored
//! --nocapture --test-threads 1`。每个场景重复若干轮，打印每一段的中位数（段名见
//! `region::island_split::laps`）和总耗时的中位数 / 最小 / 最大。
//!
//! 内容比真实世界密：每个区块都有一个改过的方块、一个漏斗（方块实体 + 容器）、一个
//! 箱子、四条计划刻和一个掉落物（第 9 步微基准的同一套），所以按区块的那几张表都很满，
//! 量到的是上界。
//!
//! How split, merge and joining / dividing the whole world scale with the
//! island size. Run by hand with `--release -- --ignored --nocapture`. The
//! content is denser than a real world (every chunk has an edited block, a
//! hopper, a chest, four scheduled ticks and a dropped item), so the numbers
//! are an upper bound.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use mc_bamboo_blocks::{HopperEntity, TickPriority, entity::Entity};
use mc_bamboo_core::{BlockPos, ChunkPos, Vec3};
use mc_bamboo_data::{block_id_of_state, blocks, items};
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_world::director::HOME_REGION;

use super::island_split_tests::{Y, with_stack};
use crate::{
    container::{ContainerKind, entry_in},
    director_host::MailBudget,
    island::{Island, Islands},
    island_partition::SectionPos,
    region::{Region, island_split::laps},
};

/// 一个矩形的区块：左下角 `(x0, z0)`，`w × h`。
fn rect(x0: i32, z0: i32, w: i32, h: i32) -> Vec<ChunkPos> {
    (0..w)
        .flat_map(|x| (0..h).map(move |z| ChunkPos::new(x0 + x, z0 + z)))
        .collect()
}

/// 内容的密度：每个区块都放一套（`Dense`），或者每 4 × 4 个区块放一套（`Sparse`，
/// 更像真实世界：大片地形、少数几处机器）。
#[derive(Clone, Copy)]
enum Density {
    Dense,
    Sparse,
}

/// 一个已加载模式的 `Region`，交付 `chunks` 并按 `density` 放上内容。
fn furnished(chunks: &[ChunkPos], density: Density) -> Region {
    let mut region = Region::with_keys(2, 20, false, false, crate::secure_chat::empty_keys());
    let desired = mc_bamboo_world::director::Desired::new(31, 31);
    for &pos in chunks {
        region
            .deliver_column(pos, Box::new(crate::region::RegionColumn::from_template()))
            .unwrap_or_else(|_| unreachable!("刚建好的岛可以交付"));
    }
    region
        .tick_levels
        .deliver(chunks.iter().map(|&pos| (pos, desired)));
    let hopper = block_id_of_state(blocks::HOPPER).expect("漏斗");
    for &pos in chunks {
        if matches!(density, Density::Sparse) && (pos.x % 4 != 0 || pos.z % 4 != 0) {
            continue;
        }
        let base = BlockPos::new(pos.x * 16, Y, pos.z * 16);
        region.chunks.set_block(base, blocks::STONE);
        region
            .chunks
            .set_block(base.offset(3, 0, 3), blocks::HOPPER);
        entry_in(
            &mut region.containers,
            base.offset(3, 0, 3),
            ContainerKind::Hopper,
        );
        region
            .block_entities
            .insert(Box::new(HopperEntity::new(base.offset(3, 0, 3))));
        entry_in(
            &mut region.containers,
            base.offset(5, 0, 5),
            ContainerKind::Chest,
        )
        .slots[0] = ItemStack::new(items::DIAMOND, 1);
        for delay in 1..=4 {
            region.block_ticks.schedule(
                base.offset(delay, 1, 0),
                hopper,
                delay * 10,
                TickPriority::Normal,
            );
        }
        region.entities.spawn(Entity::ground_item_with_delta(
            Vec3::new(
                f64::from(base.x) + 8.5,
                f64::from(Y),
                f64::from(base.z) + 8.5,
            ),
            ItemStack::new(items::IRON_INGOT, 1),
            Vec3::default(),
        ));
    }
    region.chunks.finish_light_updates();
    // tick 边界上同步层已经取走了这一刻新生成的实体的快照。
    // At a tick boundary the sync layer has taken this tick's spawn snapshots.
    let _ = region.entities.take_spawn_snapshots();
    region
}

/// 一轮的各段耗时（同名的段相加）和总耗时。
#[derive(Default)]
struct Rounds {
    totals: Vec<Duration>,
    laps: BTreeMap<&'static str, Vec<Duration>>,
}

impl Rounds {
    fn record(&mut self, total: Duration, laps: Vec<(&'static str, Duration)>) {
        self.totals.push(total);
        let mut summed: BTreeMap<&'static str, Duration> = BTreeMap::new();
        for (name, time) in laps {
            *summed.entry(name).or_default() += time;
        }
        for (name, time) in summed {
            self.laps.entry(name).or_default().push(time);
        }
    }

    fn print(&self, title: &str) {
        let ms = |time: Duration| time.as_secs_f64() * 1000.0;
        let median = |values: &[Duration]| {
            let mut sorted = values.to_vec();
            sorted.sort();
            sorted[sorted.len() / 2]
        };
        let mut sorted = self.totals.clone();
        sorted.sort();
        println!(
            "{title}: total median {:.3} ms (min {:.3}, max {:.3}, {} rounds)",
            ms(median(&self.totals)),
            ms(sorted[0]),
            ms(sorted[sorted.len() - 1]),
            sorted.len(),
        );
        let mut rows: Vec<(&str, Duration)> = self
            .laps
            .iter()
            .map(|(name, values)| (*name, median(values)))
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.1));
        for (name, time) in rows {
            if ms(time) >= 0.01 {
                println!("    {name:<44} {:>9.3} ms", ms(time));
            }
        }
    }
}

const ROUNDS: usize = 9;

/// 把 `moved` 从 `rest ∪ moved` 那个岛上拆出去（走运行时拆分的同一段：按 section 收集
/// 区块和玩家、岛这一层的拆分），再合回来；各自量 [`ROUNDS`] 轮。
fn split_and_merge(title: &str, rest: &[ChunkPos], moved: &[ChunkPos], density: Density) {
    let all: Vec<ChunkPos> = rest.iter().chain(moved).copied().collect();
    let mut island = Island::new(HOME_REGION, furnished(&all, density), MailBudget::default());
    let sections: BTreeSet<SectionPos> = moved.iter().map(|&pos| SectionPos::of(pos)).collect();
    assert!(
        rest.iter()
            .all(|pos| !sections.contains(&SectionPos::of(*pos))),
        "拆出去的部分按整 section 划"
    );
    let mut splits = Rounds::default();
    let mut merges = Rounds::default();
    for round in 0..ROUNDS {
        let random = island
            .region
            .derive_island_random(round as i64, SectionPos::new(0, 0));
        laps::start();
        let started = std::time::Instant::now();
        let (chunks, players) = super::island_runtime::split_parts(&island, &sections);
        crate::region::island_split::lap("collect");
        let child = island
            .split_off_island_boxed(1, &chunks, &players, random)
            .unwrap_or_else(|_| unreachable!("tick 边界上可以拆"));
        let split = started.elapsed();
        splits.record(split, laps::take());
        assert_eq!(child.region.chunks.loaded_positions().len(), moved.len());
        laps::start();
        let started = std::time::Instant::now();
        island.absorb_island_boxed(child, 0);
        merges.record(started.elapsed(), laps::take());
        assert_eq!(island.region.chunks.loaded_positions().len(), all.len());
    }
    splits.print(&format!("{title} split"));
    merges.print(&format!("{title} merge"));
}

/// 大岛拆出小块 / 对半拆，各自合回来（第 1 项）。
#[test]
#[ignore = "手工跑的微基准"]
fn island_split_scaling_bench() {
    with_stack(|| {
        // 约两万区块：主体 132 × 148，旁边隔 4 个区块一块 20 × 20（400 个区块）。
        let body = rect(0, 0, 132, 148);
        let small = rect(136, 0, 20, 20);
        split_and_merge(
            &format!("{}+{} chunks, small piece", body.len(), small.len()),
            &body,
            &small,
            Density::Dense,
        );
        split_and_merge(
            &format!("{}+{} chunks, small piece, sparse", body.len(), small.len()),
            &body,
            &small,
            Density::Sparse,
        );
        // 小块在大岛中间拆出去（四周都是留下的区块，隔 4 个区块）。
        let ring: Vec<ChunkPos> = rect(0, 0, 148, 148)
            .into_iter()
            .filter(|pos| !(60..88).contains(&pos.x) || !(60..88).contains(&pos.z))
            .collect();
        let inner = rect(64, 64, 20, 20);
        split_and_merge(
            &format!(
                "{}+{} chunks, small piece in the middle",
                ring.len(),
                inner.len()
            ),
            &ring,
            &inner,
            Density::Dense,
        );
        // 对半：两块 100 × 100，隔 4 个区块。
        let left = rect(0, 0, 100, 100);
        let right = rect(104, 0, 100, 100);
        split_and_merge(
            &format!("{}+{} chunks, halves", left.len(), right.len()),
            &left,
            &right,
            Density::Dense,
        );
        split_and_merge(
            &format!("{}+{} chunks, halves, sparse", left.len(), right.len()),
            &left,
            &right,
            Density::Sparse,
        );
        // 对照：只有两千区块的岛拆出同样的 400 个区块。
        let body = rect(0, 0, 40, 40);
        split_and_merge(
            &format!("{}+{} chunks, small piece", body.len(), small.len()),
            &body,
            &small,
            Density::Dense,
        );
    });
}

/// 把 `parts` 各自造成一个岛（第一个是家岛），拼成一个再拆回去，各量 [`ROUNDS`] 轮
/// （第 2 项；`into` 是家岛，和控制台发的命令一样）。
fn unify_and_divide(title: &str, parts: &[Vec<ChunkPos>], density: Density) {
    let all: Vec<ChunkPos> = parts.iter().flatten().copied().collect();
    let mut home = Box::new(Island::new(
        HOME_REGION,
        furnished(&all, density),
        MailBudget::default(),
    ));
    let mut others = Vec::new();
    for (index, part) in parts.iter().enumerate().skip(1) {
        let chunks: BTreeSet<ChunkPos> = part.iter().copied().collect();
        let random = home
            .region
            .derive_island_random(index as i64, SectionPos::of(part[0]));
        others.push(
            home.split_off_island_boxed(index as u32, &chunks, &BTreeSet::new(), random)
                .unwrap_or_else(|_| unreachable!("tick 边界上可以拆")),
        );
    }
    let mut islands = Islands::new(home);
    for island in others {
        islands.insert(island);
    }
    let mut unifies = Rounds::default();
    let mut divides = Rounds::default();
    for _ in 0..ROUNDS {
        laps::start();
        let started = std::time::Instant::now();
        let unified = islands.unify(HOME_REGION);
        unifies.record(started.elapsed(), laps::take());
        assert_eq!(
            islands
                .get(HOME_REGION)
                .expect("家岛")
                .region
                .chunks
                .loaded_positions()
                .len(),
            all.len()
        );
        laps::start();
        let started = std::time::Instant::now();
        islands.divide(HOME_REGION, unified);
        divides.record(started.elapsed(), laps::take());
        assert_eq!(islands.len(), parts.len());
    }
    unifies.print(&format!("{title} unify"));
    divides.print(&format!("{title} divide"));
}

/// 两万区块、三四个岛时拼 / 拆整个世界（第 2 项）。
#[test]
#[ignore = "手工跑的微基准"]
fn island_unify_scaling_bench() {
    with_stack(|| {
        // 四个岛，各 70 × 70（共 19600 个区块），隔 4 个区块。
        let four: Vec<Vec<ChunkPos>> = (0..4).map(|index| rect(index * 74, 0, 70, 70)).collect();
        unify_and_divide("4 islands x 4900 chunks", &four, Density::Dense);
        unify_and_divide("4 islands x 4900 chunks, sparse", &four, Density::Sparse);
        // 三个岛：一个大的（14000）和两个小的（各 3000）。
        let three = vec![
            rect(0, 0, 140, 100),
            rect(144, 0, 60, 50),
            rect(144, 54, 60, 50),
        ];
        unify_and_divide("3 islands 14000+3000+3000 chunks", &three, Density::Dense);
        unify_and_divide(
            "3 islands 14000+3000+3000 chunks, sparse",
            &three,
            Density::Sparse,
        );
        // 发起人的岛小：小的在前（家岛），大的被并进来。
        let small_home = vec![
            rect(144, 0, 60, 50),
            rect(0, 0, 140, 100),
            rect(144, 54, 60, 50),
        ];
        unify_and_divide(
            "3 islands 3000(home)+14000+3000 chunks",
            &small_home,
            Density::Dense,
        );
        unify_and_divide(
            "3 islands 3000(home)+14000+3000 chunks, sparse",
            &small_home,
            Density::Sparse,
        );
    });
}
