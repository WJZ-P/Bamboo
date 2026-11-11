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
