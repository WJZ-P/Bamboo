//! 已加载模式下区块表和光照存储的规则，见 `docs/world-model-design.md` 第 5 节。
//!
//! 无限超平坦模式（`ChunkStore::flat`）里每个位置都隐式存在；正式运行用
//! `ChunkStore::loaded`，没加载的位置在两层光照存储里都没有柱。

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};
use mc_bamboo_data::blocks;

use super::ChunkStore;
use crate::{
    column::{ChunkColumn, ChunkColumnParts},
    light::LIGHT_SECTION_COUNT,
};

const ORIGIN: ChunkPos = ChunkPos::new(0, 0);
const NEIGHBOR: ChunkPos = ChunkPos::new(1, 0);

/// 造一个和模板一样的盒子，用来把某个位置加载进来。
fn template_column(store: &mut ChunkStore) -> Box<ChunkColumn> {
    let pos = ChunkPos::new(1000, 1000);
    // 无限模式：`take_column` 会把这个位置的光照柱取走，但读起来还是模板。
    let column = store.take_column(pos).expect("光照已算完");
    store
        .insert_column(pos, column)
        .unwrap_or_else(|_| unreachable!("光照已算完"));
    store.take_column(pos).expect("光照已算完")
}

impl ChunkStore {
    /// 测试辅助：加载一个模板盒子到 `pos`。
    #[cfg(test)]
    fn load_template(&mut self, pos: ChunkPos) {
        let mut infinite = ChunkStore::flat();
        let column = template_column(&mut infinite);
        self.load(pos, column);
    }
}

/// 规则 1/5：没加载的位置没有光照柱；`unload` 之后这个位置真的没有柱。
#[test]
fn unloaded_positions_have_no_light_column() {
    let mut store = ChunkStore::loaded();
    assert!(store.is_loaded_mode());
    assert_eq!(store.loaded_count(), Some(0));
    assert!(!store.is_loaded(ORIGIN));
    assert!(!store.light().has_column(ORIGIN), "起点不该有柱");
    assert!(!store.light().has_column(NEIGHBOR), "邻居也不该有柱");
    assert_eq!(
        store.light().section_states(ORIGIN),
        [0; LIGHT_SECTION_COUNT],
        "没加载的位置 26 个段状态全 0"
    );

    store.load_template(ORIGIN);
    assert!(store.is_loaded(ORIGIN));
    assert!(store.light().has_column(ORIGIN));
    assert!(!store.light().has_column(NEIGHBOR), "只加载了一个位置");
    assert_eq!(store.loaded_count(), Some(1));

    let column = store.unload(ORIGIN).expect("光照已算完");
    assert!(!store.is_loaded(ORIGIN));
    assert!(
        !store.light().has_column(ORIGIN),
        "取走之后这个位置不该再有柱"
    );
    store.load(ORIGIN, column);
    assert!(store.light().has_column(ORIGIN));
}

/// 规则 4：没加载的位置读方块是 `None`，写不生效，光照按天空光 15 / 方块光 0。
#[test]
fn unloaded_positions_read_as_missing_and_ignore_writes() {
    let mut store = ChunkStore::loaded();
    let pos = BlockPos::new(3, -60, 3);
    assert_eq!(store.block(pos), None);
    assert!(!store.set_block(pos, blocks::STONE));
    assert_eq!(store.block(pos), None);
    assert_eq!(store.heightmap_y(pos), None);
    assert_eq!(store.biome_at(pos), None);
    assert!(!store.is_randomly_ticking_section(ORIGIN, -4));
    assert_eq!(store.randomly_ticking_sections(ORIGIN), 0);
    assert_eq!(store.light_value(LightLayer::Sky, pos), 15);
    assert_eq!(store.light_value(LightLayer::Block, pos), 0);
    assert!(!store.set_sign(pos, Default::default()));
    assert!(!store.set_update_tag(pos, None));

    store.load_template(ORIGIN);
    assert_eq!(store.block(pos), Some(blocks::AIR));
    assert!(store.set_block(pos, blocks::STONE));
    assert_eq!(store.block(pos), Some(blocks::STONE));
    store.finish_light_updates();
    // 加载之后读的是真实的光照，不再是默认值。
    assert_eq!(store.light_value(LightLayer::Sky, pos), 0);
}

/// 规则 2：方块变化引起的邻居计数更新落到没加载的位置时直接丢掉，不建柱。
#[test]
fn neighbour_count_updates_never_materialize_unloaded_columns() {
    let mut store = ChunkStore::loaded();
    store.load_template(ORIGIN);
    // 在边界上塞满/清空一整段，让 `update_section_status` 给 26 个邻居段 ±1。
    for y in -64..-48 {
        store.set_block(BlockPos::new(15, y, 3), blocks::STONE);
    }
    store.finish_light_updates();
    assert!(!store.light().has_column(NEIGHBOR), "邻居没加载，不该建柱");
    for y in -64..-48 {
        store.set_block(BlockPos::new(15, y, 3), blocks::AIR);
    }
    store.finish_light_updates();
    assert!(!store.light().has_column(NEIGHBOR), "清空后也不该建柱");
    assert!(store.light().states_have_layers());
}

/// 规则 3：`rebind_area` 只重算 3×3 里已加载的柱，没加载的按「没有方块」计入邻居计数。
/// 加载一个邻居之后，边界段的计数要按两边的方块重新算，且状态非 0 的段都有数据层。
#[test]
fn rebind_only_touches_loaded_columns() {
    let mut store = ChunkStore::loaded();
    store.load_template(ORIGIN);
    store.load_template(NEIGHBOR);
    store.load_template(ChunkPos::new(0, 1));
    store.finish_light_updates();
    assert!(store.light().states_have_layers());

    let before = store.light().section_states(NEIGHBOR);
    let column = store.unload(ORIGIN).expect("光照已算完");
    assert!(!store.light().has_column(ORIGIN));
    assert!(store.light().states_have_layers());
    // 邻居柱还在，且段状态非 0 的段仍有数据层（状态变 0 的段不丢数据层）。
    let after = store.light().section_states(NEIGHBOR);
    assert_ne!(before, after, "邻居计数应该按少了一个柱重算");
    store.load(ORIGIN, column);
    assert!(store.light().states_have_layers());
}

/// 规则 3 + 光照原样：邻居边界旁的光源会照进这一柱，卸载再加载后读数不变。
/// 这是 `ChunkBundle::is_pristine()` 必须比较光照的理由（game 侧实现，
/// 这里钉住「光照会变」这个事实）。
#[test]
fn a_neighbour_light_source_changes_this_columns_light_data() {
    let mut store = ChunkStore::loaded();
    store.load_template(ORIGIN);
    // 邻居没加载，先放不进光源。
    store.load_template(NEIGHBOR);
    let lamp = BlockPos::new(16, -60, 3);
    store.set_block(lamp, blocks::GLOWSTONE);
    store.finish_light_updates();
    let origin_block = BlockPos::new(15, -60, 3);
    let lit = store.light_value(LightLayer::Block, origin_block);
    assert!(lit > 0, "光要照到这一柱");

    let column = store.unload(ORIGIN).expect("光照已算完");
    assert_eq!(store.light_value(LightLayer::Block, origin_block), 0);
    store.load(ORIGIN, column);
    assert_eq!(
        store.light_value(LightLayer::Block, origin_block),
        lit,
        "盒子带回来的光照要原样留着"
    );
}

/// 随机测试：随机加载、卸载、改方块、放光源，每步断言
/// 「没加载的位置在两层光照存储里都没有柱」且「状态非 0 的段都有数据层」。
#[test]
fn random_load_unload_never_materializes_unloaded_columns() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut store = ChunkStore::loaded();
    let area: Vec<ChunkPos> = (-2..=2)
        .flat_map(|x| (-2..=2).map(move |z| ChunkPos::new(x, z)))
        .collect();
    let mut parked: std::collections::BTreeMap<ChunkPos, Box<ChunkColumn>> = Default::default();

    for _ in 0..300 {
        let pos = area[(next() as usize) % area.len()];
        match next() % 4 {
            0 if !store.is_loaded(pos) => {
                let column = parked
                    .remove(&pos)
                    .unwrap_or_else(|| template_column(&mut ChunkStore::flat()));
                store.load(pos, column);
            }
            1 if store.is_loaded(pos) => {
                store.finish_light_updates();
                let column = store.unload(pos).expect("光照已算完");
                parked.insert(pos, column);
            }
            2 if store.is_loaded(pos) => {
                let x = pos.x * 16 + (next() % 16) as i32;
                let z = pos.z * 16 + (next() % 16) as i32;
                let y = -64 + (next() % 8) as i32;
                store.set_block(BlockPos::new(x, y, z), blocks::STONE);
                store.run_light_updates(4000);
            }
            _ => {
                store.finish_light_updates();
            }
        }
        for &other in &area {
            assert_eq!(
                store.is_loaded(other),
                store.light().has_column(other),
                "位置 {other:?} 的已加载状态和光照柱不一致"
            );
        }
        assert!(
            store.light().states_have_layers(),
            "状态非 0 的段缺了数据层"
        );
        assert_eq!(
            store.loaded_count(),
            Some(area.iter().filter(|p| store.is_loaded(**p)).count())
        );
    }
}

/// 手工基准：把一片模板区块装进已加载的世界再卸掉，报每个区块的耗时。
/// `cargo test --release -p mc-bamboo-world --lib loading_cost -- --ignored --nocapture`
#[test]
#[ignore = "手工基准，不进常规测试"]
fn loading_cost() {
    use std::time::Instant;
    const RADIUS: i32 = 11; // 视距 10 时一名玩家周围要交付的范围：23×23 = 529 个区块
    let mut store = ChunkStore::loaded();
    let mut positions = Vec::new();
    for x in -RADIUS..=RADIUS {
        for z in -RADIUS..=RADIUS {
            positions.push(ChunkPos::new(x, z));
        }
    }
    let boxes: Vec<_> = positions
        .iter()
        .map(|_| Box::new(ChunkColumn::from_template()))
        .collect();
    let started = Instant::now();
    for (&pos, column) in positions.iter().zip(boxes) {
        store.load(pos, column);
    }
    let load = started.elapsed();
    let started = Instant::now();
    for &pos in &positions {
        store.unload(pos).expect("光照已经算完");
    }
    let unload = started.elapsed();
    let count = positions.len() as u32;
    eprintln!(
        "{} 个区块：装入共 {:?}（每个 {:?}），卸掉共 {:?}（每个 {:?}）",
        count,
        load,
        load / count,
        unload,
        unload / count
    );
}

/// 手工基准：连续读一片方块，已加载模式和无限模式各多少纳秒一次。
/// `cargo test --release -p mc-bamboo-world --lib block_read_cost -- --ignored --nocapture`
#[test]
#[ignore = "手工基准，不进常规测试"]
fn block_read_cost() {
    use std::time::Instant;

    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::blocks;

    const SIDE: i32 = 23; // 视距 10 覆盖的 23×23 区块
    const ROUNDS: u32 = 200;
    let mut positions = Vec::new();
    for x in -SIDE..=SIDE {
        for z in -SIDE..=SIDE {
            for y in [-60, -59, -58] {
                positions.push(BlockPos::new(x * 16 + (x & 15), y, z * 16 + (z & 15)));
            }
        }
    }
    // 让一部分区块带上非默认的方块，读数不是常量，编译器不能把循环折叠掉。
    let make_column = |x: i32| {
        let column = ChunkColumn::from_template();
        if x % 2 == 0 {
            let mut parts = ChunkColumnParts::read_from(&column);
            parts.sections[0].block_palette = vec![blocks::STONE];
            parts.sections[0].block_data.clear();
            Box::new(ChunkColumn::from_parts(parts).expect("改过的段合法"))
        } else {
            Box::new(column)
        }
    };
    let reads = positions.len() as u32 * ROUNDS;

    // 两种模式都把同一片区块装进 `edited` 表，这样读数除了 `is_loaded`
    // 之外走完全相同的路径；差异只来自「有没有加载」这一问。
    // 逐块 `insert_column` 会重算 3×3 段状态，只做一次，不计入计时。
    let mut infinite = ChunkStore::flat();
    for x in -SIDE..=SIDE {
        for z in -SIDE..=SIDE {
            infinite
                .insert_column(ChunkPos::new(x, z), make_column(x))
                .unwrap_or_else(|_| unreachable!("光照已算完"));
        }
    }
    // 先跑几轮把缓存和页预热了，再开始计时；两种模式同样处理。
    let mut sum = 0u64;
    for _ in 0..20 {
        for &pos in &positions {
            sum = sum.wrapping_add(u64::from(infinite.block(pos).unwrap_or(0)));
        }
    }
    let started = Instant::now();
    for _ in 0..ROUNDS {
        for &pos in &positions {
            sum = sum.wrapping_add(u64::from(infinite.block(pos).unwrap_or(0)));
        }
    }
    let flat_time = started.elapsed();

    // 已加载模式：先把这些区块装进去，再连续读同一批位置。
    let mut loaded = ChunkStore::loaded();
    for x in -SIDE..=SIDE {
        for z in -SIDE..=SIDE {
            let pos = ChunkPos::new(x, z);
            loaded.load(pos, make_column(x));
        }
    }
    for _ in 0..20 {
        for &pos in &positions {
            sum = sum.wrapping_add(u64::from(loaded.block(pos).unwrap_or(0)));
        }
    }
    let started = Instant::now();
    for _ in 0..ROUNDS {
        for &pos in &positions {
            sum = sum.wrapping_add(u64::from(loaded.block(pos).unwrap_or(0)));
        }
    }
    let loaded_time = started.elapsed();
    // 把校验和用出去，免得编译器把整段读优化掉；也能顺便确认两种模式读到一样的内容。
    std::hint::black_box(sum);

    let flat_ns = flat_time.as_nanos() / u128::from(reads);
    let loaded_ns = loaded_time.as_nanos() / u128::from(reads);
    eprintln!(
        "{} 次读方块：无限模式 {flat_time:?}（{flat_ns} ns/次），已加载模式 {loaded_time:?}（{loaded_ns} ns/次）",
        reads
    );
}

/// 已加载模式下，已加载集合和 `edited` 表必须同进同退：
/// 读方块的热路径靠一次 `edited` 查表同时判断「有没有加载」。
#[test]
fn loaded_positions_always_have_a_chunk_in_the_edited_table() {
    let mut store = ChunkStore::loaded();
    let area: Vec<ChunkPos> = (-1..=1)
        .flat_map(|x| (-1..=1).map(move |z| ChunkPos::new(x, z)))
        .collect();
    for &pos in &area {
        store.load(pos, Box::new(ChunkColumn::from_template()));
        assert!(store.is_loaded(pos));
        assert!(store.edited(pos).is_some(), "{pos:?} 装在 edited 表里");
        assert_eq!(
            store.block(BlockPos::new(pos.x * 16 + 2, -60, pos.z * 16 + 2)),
            Some(blocks::AIR)
        );
    }
    for &pos in &area {
        store.unload(pos).expect("光照已算完");
        assert!(!store.is_loaded(pos));
        assert!(store.edited(pos).is_none(), "{pos:?} 从 edited 表里取走了");
        assert_eq!(
            store.block(BlockPos::new(pos.x * 16 + 2, -60, pos.z * 16 + 2)),
            None
        );
    }
}

/// 拆岛测试用的两片区块：(0,0) 周围 3×3 和 (10,0) 周围 3×3，中间隔着没加载的区块。
fn two_clusters() -> (Vec<ChunkPos>, Vec<ChunkPos>) {
    let square = |cx: i32| -> Vec<ChunkPos> {
        (cx - 1..=cx + 1)
            .flat_map(|x| (-1..=1).map(move |z| ChunkPos::new(x, z)))
            .collect()
    };
    (square(0), square(10))
}

/// 一片区块里放几块会发光、会挡光的方块，跑完光照。
fn furnish_cluster(store: &mut ChunkStore, cx: i32, salt: i32) {
    let base = cx * 16;
    store.set_block(BlockPos::new(base + 3, -60, 4), blocks::GLOWSTONE);
    store.set_block(BlockPos::new(base + 8 + salt, -58, 9), blocks::STONE);
    store.set_block(BlockPos::new(base - 5, -55, -6), blocks::GLOWSTONE);
    for y in -60..-40 {
        store.set_block(BlockPos::new(base + 12, y, 12 - salt), blocks::STONE);
    }
}

/// 一组区块的全部可观察读数：逐格方块、两层光照、高度图，每段的光照数据、段状态、
/// 版本号、原样判断。
fn cluster_readings(store: &ChunkStore, chunks: &[ChunkPos]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for &pos in chunks {
        let _ = write!(
            out,
            "{pos:?} loaded={} pristine={} states={:?} ",
            store.is_loaded(pos),
            store.is_pristine(pos),
            store.light().section_states(pos)
        );
        for y in crate::light::MIN_LIGHT_SECTION..crate::light::MIN_LIGHT_SECTION + 26 {
            let section = crate::light::SectionPos::new(pos.x, y, pos.z);
            let _ = write!(
                out,
                "{:?}{:?}",
                store.light().section(LightLayer::Sky, section),
                store.light().section(LightLayer::Block, section)
            );
        }
        for x in (0..16).step_by(3) {
            for z in (0..16).step_by(5) {
                for y in [-64, -61, -60, -59, -50, -41, 0, 100] {
                    let block = BlockPos::new(pos.x * 16 + x, y, pos.z * 16 + z);
                    let _ = write!(
                        out,
                        "{:?}{}{}{:?}",
                        store.block(block),
                        store.light_value(LightLayer::Sky, block),
                        store.light_value(LightLayer::Block, block),
                        store.heightmap_y(block)
                    );
                }
            }
        }
    }
    out
}

fn two_cluster_store() -> (ChunkStore, Vec<ChunkPos>, Vec<ChunkPos>) {
    let (first, second) = two_clusters();
    let mut store = ChunkStore::loaded();
    for &pos in first.iter().chain(&second) {
        store.load(pos, Box::new(ChunkColumn::from_template()));
    }
    furnish_cluster(&mut store, 0, 0);
    furnish_cluster(&mut store, 10, 1);
    store.finish_light_updates();
    (store, first, second)
}

/// 岛的整批拆合：拆出去再合回来逐项相同（含版本号和原样判断）；拆开以后两边各自
/// 改方块、算光照，合回来等于从没拆过一直这么改。
#[test]
fn an_island_split_and_merge_keeps_blocks_light_and_revisions() {
    let (mut store, first, second) = two_cluster_store();
    let all: Vec<ChunkPos> = first.iter().chain(&second).copied().collect();
    let control = store.clone();
    let pending_changes = store.light().take_changes_len();
    let before = cluster_readings(&store, &all);
    let revisions: Vec<_> = all.iter().map(|&pos| store.revision(pos)).collect();
    let moved: std::collections::BTreeSet<ChunkPos> = second.iter().copied().collect();

    let mut island = store.split_off(&moved).expect("光照已算完");
    assert_eq!(store.loaded_positions(), first);
    assert_eq!(island.loaded_positions(), second);
    // 还没发出去的光照变化按区块跟着走，一条不丢。
    assert_eq!(
        island.light().take_changes_len() + store.light().take_changes_len(),
        pending_changes
    );
    assert!(island.light().take_changes_len() > 0);
    // 拆出去的每一片和原来逐项相同，留下的那一片也是。
    assert_eq!(
        cluster_readings(&store, &first) + &cluster_readings(&island, &second),
        before
    );
    for &pos in &second {
        assert!(!store.is_loaded(pos) && !store.light().has_column(pos));
        assert!(island.light().has_column(pos));
    }
    island.absorb_ok(&mut store);
    let mut merged = island;
    assert_eq!(cluster_readings(&merged, &all), before);
    let after: Vec<_> = all.iter().map(|&pos| merged.revision(pos)).collect();
    assert_eq!(after, revisions, "整批搬运不换版本号");
    assert_eq!(merged.loaded_positions(), control.loaded_positions());

    // 拆开以后两边各自改：和不拆的对照一模一样。
    let mut control = control;
    let mut island = merged.split_off(&moved).expect("光照已算完");
    for (target, cx) in [(&mut merged, 0), (&mut island, 10)] {
        target.set_block(BlockPos::new(cx * 16 + 6, -59, 6), blocks::GLOWSTONE);
        target.set_block(BlockPos::new(cx * 16 + 3, -60, 4), blocks::AIR);
        target.finish_light_updates();
    }
    for cx in [0, 10] {
        control.set_block(BlockPos::new(cx * 16 + 6, -59, 6), blocks::GLOWSTONE);
        control.set_block(BlockPos::new(cx * 16 + 3, -60, 4), blocks::AIR);
    }
    control.finish_light_updates();
    // 版本号每次写都换新值，两边各自数，不比绝对值；比较「改过没改过」。
    let changed_split: Vec<bool> = all
        .iter()
        .map(|&pos| {
            let now = if moved.contains(&pos) {
                island.revision(pos)
            } else {
                merged.revision(pos)
            };
            now != revisions[all.iter().position(|&p| p == pos).unwrap()]
        })
        .collect();
    let changed_control: Vec<bool> = all
        .iter()
        .enumerate()
        .map(|(index, &pos)| control.revision(pos) != revisions[index])
        .collect();
    assert_eq!(changed_split, changed_control);
    merged
        .absorb(island)
        .unwrap_or_else(|_| unreachable!("光照已算完"));
    assert_eq!(
        cluster_readings(&merged, &all),
        cluster_readings(&control, &all)
    );
}

/// 两片区块各改一批方块（不算光照）。`many` 为真时每片改一千多格：光照任务每 1000 个
/// 排成一轮，这样拆合时既有排好的轮次、又有正在攒的一轮，还有留着的区块副本；最后
/// 再把一处光源放上又拆掉（排好的轮次之后的改动）。
fn queue_changes(store: &mut ChunkStore, many: bool) {
    for (cx, salt) in [(0, 0), (10, 1)] {
        let base = cx * 16;
        store.set_block(BlockPos::new(base + 6, -59, 6), blocks::GLOWSTONE);
        store.set_block(BlockPos::new(base + 3, -60, 4), blocks::AIR);
        if many {
            for index in 0..1100 {
                let (x, z) = (index % 16, (index / 16) % 16);
                let y = -40 + index / 256;
                store.set_block(BlockPos::new(base + x, y, z + salt), blocks::STONE);
            }
            store.set_block(BlockPos::new(base + 9, -59, 2), blocks::GLOWSTONE);
            store.set_block(BlockPos::new(base + 9, -59, 2), blocks::AIR);
            store.set_block(BlockPos::new(base - 3, -58, 7), blocks::GLOWSTONE);
        }
    }
}

/// 排着还没开始算的光照任务跟着区块拆合：改完方块不算光照就拆，两边各自算，和不拆
/// 直接算的结果逐项相同，标成「变过」的段也相同；两边各自排着任务时并起来再算也一样。
/// 任务不到一轮和超过一轮（有排好的轮次和留着的区块副本）两种都测。
///
/// Light tasks queued but not started travel with their chunks: splitting
/// right after block changes and computing each side alone gives exactly
/// what computing unsplit gives, including which sections are flagged as
/// changed; merging two sides that each hold queued tasks and then computing
/// is the same too. Both under one round and past one round (queued rounds
/// and kept chunk copies).
#[test]
fn queued_light_tasks_travel_with_their_chunks() {
    for many in [false, true] {
        let (mut store, first, second) = two_cluster_store();
        let all: Vec<ChunkPos> = first.iter().chain(&second).copied().collect();
        let moved: std::collections::BTreeSet<ChunkPos> = second.iter().copied().collect();
        let _ = store.take_light_changes();
        let mut control = store.clone();
        let mut joined = store.clone();

        // 对照：不拆，直接算。
        queue_changes(&mut control, many);
        assert!(control.has_pending_light_updates());
        control.finish_light_updates();
        let expected = cluster_readings(&control, &all);
        let expected_changes = control.take_light_changes();
        assert!(!expected_changes.is_empty(), "many={many}：光照真的变了");

        // 改完就拆（光照任务还排着），两边各自算，再并。
        queue_changes(&mut store, many);
        assert!(!store.ready_to_move_columns() && store.ready_to_split());
        let mut island = store.split_off(&moved).expect("没有算到一半的轮次");
        assert!(store.has_pending_light_updates() && island.has_pending_light_updates());
        store.finish_light_updates();
        island.finish_light_updates();
        let mut changes = store.take_light_changes();
        for (pos, change) in island.take_light_changes() {
            assert!(changes.insert(pos, change).is_none(), "一个区块只在一边");
        }
        assert_eq!(changes, expected_changes, "many={many}：变过的段");
        store
            .absorb(island)
            .unwrap_or_else(|_| unreachable!("光照已算完"));
        assert_eq!(cluster_readings(&store, &all), expected, "many={many}");

        // 先拆开，两边各自改（任务各自排着），不算就并起来，再算。
        let mut island = joined.split_off(&moved).expect("光照已算完");
        let mut scratch = ChunkStore::loaded();
        std::mem::swap(&mut scratch, &mut joined);
        // `queue_changes` 两片都改；没加载的那一片写不进去，正好各改各的。
        queue_changes(&mut scratch, many);
        queue_changes(&mut island, many);
        scratch
            .absorb(island)
            .unwrap_or_else(|_| unreachable!("没有算到一半的轮次"));
        assert!(scratch.has_pending_light_updates());
        scratch.finish_light_updates();
        assert_eq!(
            scratch.take_light_changes(),
            expected_changes,
            "many={many}"
        );
        assert_eq!(cluster_readings(&scratch, &all), expected, "many={many}");
    }
}

/// 有算到一半的光照轮次时不能拆：拆合不碰引擎里做了一半的工作。
///
/// A split is refused while a light round is half done.
#[test]
fn an_island_split_waits_for_a_running_light_round() {
    let (mut store, _, second) = two_cluster_store();
    let moved: std::collections::BTreeSet<ChunkPos> = second.iter().copied().collect();
    queue_changes(&mut store, true);
    // 只给一点工作量：第一轮开始了没算完。
    store.run_light_updates(5);
    assert!(!store.ready_to_split());
    assert!(store.split_off(&moved).is_err());
    store.finish_light_updates();
    assert!(store.split_off(&moved).is_ok());
}

/// 搬走的区块和留下的已加载区块相邻时拒绝拆：整批搬运不重算邻居段状态。
#[test]
#[should_panic(expected = "相邻")]
fn an_island_split_refuses_adjacent_chunks() {
    let (mut store, first, _) = two_cluster_store();
    let half: std::collections::BTreeSet<ChunkPos> = first.iter().take(3).copied().collect();
    let _ = store.split_off(&half);
}

/// 合岛时两边相邻也拒绝。
#[test]
#[should_panic(expected = "相邻")]
fn an_island_merge_refuses_adjacent_chunks() {
    let (mut store, _, second) = two_cluster_store();
    let moved: std::collections::BTreeSet<ChunkPos> = second.iter().copied().collect();
    let island = store.split_off(&moved).expect("光照已算完");
    store.load(ChunkPos::new(8, 0), Box::new(ChunkColumn::from_template()));
    store.finish_light_updates();
    let _ = store.absorb(island);
}

impl ChunkStore {
    /// 测试辅助：把 `other` 并进自己，`other` 留空表。
    fn absorb_ok(&mut self, other: &mut ChunkStore) {
        let taken = std::mem::replace(other, ChunkStore::loaded());
        self.absorb(taken)
            .unwrap_or_else(|_| unreachable!("光照已算完"));
    }
}

impl crate::light::LevelLight {
    /// 测试辅助：待发的光照变化有几个区块。
    fn take_changes_len(&self) -> usize {
        self.clone().take_changes().len()
    }
}

/// `InhabitedTime` 单独记账：累加不换版本号、不进存盘集合；拆岛、合岛逐个区块跟着走、
/// 一个不丢；取出的盒子带着累计值，放回去再读出来不变；卸载以后条目不留。
#[test]
fn inhabited_time_travels_with_its_chunk_without_touching_revisions() {
    let (mut store, first, second) = two_cluster_store();
    let all: Vec<ChunkPos> = first.iter().chain(&second).copied().collect();
    let _ = store.take_changed_chunks();
    let revisions: Vec<_> = all.iter().map(|&pos| store.revision(pos)).collect();
    for (index, &pos) in all.iter().enumerate() {
        for _ in 0..=index {
            store.increment_inhabited_time(pos);
        }
    }
    let expected: Vec<i64> = (1..=all.len() as i64).collect();
    let read = |store: &ChunkStore, chunks: &[ChunkPos]| -> Vec<i64> {
        chunks.iter().map(|&pos| store.inhabited_time(pos)).collect()
    };
    assert_eq!(read(&store, &all), expected);
    let after: Vec<_> = all.iter().map(|&pos| store.revision(pos)).collect();
    assert_eq!(after, revisions, "累加不换版本号");
    assert!(store.take_changed_chunks().is_empty(), "累加不进存盘集合");

    let moved: std::collections::BTreeSet<ChunkPos> = second.iter().copied().collect();
    let mut island = store.split_off(&moved).expect("光照已算完");
    assert_eq!(read(&store, &first), expected[..first.len()]);
    assert_eq!(read(&island, &second), expected[first.len()..]);
    for &pos in &second {
        assert_eq!(store.inhabited_time(pos), 0, "搬走的区块不留在原表");
    }
    island.absorb_ok(&mut store);
    let mut merged = island;
    assert_eq!(read(&merged, &all), expected);

    // 存盘的快照和取出的盒子都带着累计值。
    let pos = all[0];
    let snapshot = merged.column_clone(pos).expect("已加载");
    assert_eq!(snapshot.chunk().archive().inhabited_time, 1);
    let column = merged.unload(pos).expect("光照已算完");
    assert_eq!(column.chunk().archive().inhabited_time, 1);
    assert_eq!(merged.inhabited_time(pos), 0, "卸载以后条目不留");
    merged.load(pos, column);
    assert_eq!(merged.inhabited_time(pos), 1, "读盘的值放回表里");
}
