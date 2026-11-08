//! 光照引擎在超平坦世界里的行为。逐格和官方比较的检查在 `mc-bamboo-vanilla light` 里。

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};
use mc_bamboo_data::blocks;

use super::{DataLayer, LIGHT_SECTION_COUNT, LightChanges, MIN_LIGHT_SECTION, SectionPos};
use crate::store::ChunkStore;

const GROUND: i32 = -60;

fn sky(store: &ChunkStore, x: i32, y: i32, z: i32) -> u8 {
    store.light_value(LightLayer::Sky, BlockPos::new(x, y, z))
}

fn block(store: &ChunkStore, x: i32, y: i32, z: i32) -> u8 {
    store.light_value(LightLayer::Block, BlockPos::new(x, y, z))
}

fn place(store: &mut ChunkStore, x: i32, y: i32, z: i32, state: u32) {
    store.set_block(BlockPos::new(x, y, z), state);
}

fn fill(
    store: &mut ChunkStore,
    (x0, y0, z0): (i32, i32, i32),
    (x1, y1, z1): (i32, i32, i32),
    state: u32,
) {
    for x in x0..=x1 {
        for y in y0..=y1 {
            for z in z0..=z1 {
                place(store, x, y, z, state);
            }
        }
    }
}

fn section(store: &ChunkStore, layer: LightLayer, y: i32) -> Option<&DataLayer> {
    store.light().section(layer, SectionPos::new(0, y, 0))
}

#[test]
fn flat_template_is_lit_from_the_grass_up() {
    let store = ChunkStore::flat();

    assert_eq!(sky(&store, 3, GROUND, 3), 15);
    assert_eq!(sky(&store, 3, GROUND - 1, 3), 0);
    assert_eq!(sky(&store, 3, -64, 3), 0);
    assert_eq!(sky(&store, 3, 300, 3), 15);
    assert_eq!(block(&store, 3, GROUND, 3), 0);

    // 只有地面那段和它上下两段存光照：下面那段没写过，是空的；上面那段整段写成了 15。
    assert!(section(&store, LightLayer::Sky, -5).is_some_and(DataLayer::is_empty));
    assert!(matches!(
        section(&store, LightLayer::Sky, -4),
        Some(DataLayer::Data(_))
    ));
    let above = section(&store, LightLayer::Sky, -3).expect("地面上面一段存着光照");
    assert!(above.bytes().iter().all(|&byte| byte == 0xff));
    assert!(section(&store, LightLayer::Sky, -2).is_none());
    for y in -5..=-3 {
        assert!(section(&store, LightLayer::Block, y).is_some_and(DataLayer::is_empty));
    }
}

#[test]
fn a_torch_lights_its_surroundings() {
    let mut store = ChunkStore::flat();
    place(&mut store, 8, GROUND, 8, blocks::TORCH);
    // 算之前读到的还是原来的光照。
    assert_eq!(block(&store, 8, GROUND, 8), 0);

    store.finish_light_updates();
    assert_eq!(block(&store, 8, GROUND, 8), 14);
    assert_eq!(block(&store, 9, GROUND, 8), 13);
    assert_eq!(block(&store, 8, GROUND + 3, 8), 11);
    assert_eq!(block(&store, 21, GROUND, 8), 1);
    assert_eq!(block(&store, 22, GROUND, 8), 0);
    // 草方块不透光。
    assert_eq!(block(&store, 8, GROUND - 1, 8), 0);

    place(&mut store, 8, GROUND, 8, blocks::AIR);
    store.finish_light_updates();
    assert_eq!(block(&store, 8, GROUND, 8), 0);
    assert_eq!(block(&store, 21, GROUND, 8), 0);
}

#[test]
fn a_roof_shades_the_ground_and_light_comes_back_in_from_the_sides() {
    let mut store = ChunkStore::flat();
    fill(
        &mut store,
        (0, GROUND + 3, 0),
        (20, GROUND + 3, 20),
        blocks::STONE,
    );
    store.finish_light_updates();

    // 屋顶下面的光从四边漏进来，往里每格减 1。
    assert_eq!(sky(&store, 10, GROUND, 10), 4);
    assert_eq!(sky(&store, 0, GROUND, 10), 14);
    assert_eq!(sky(&store, 10, GROUND + 4, 10), 15);

    fill(
        &mut store,
        (0, GROUND + 3, 0),
        (20, GROUND + 3, 20),
        blocks::AIR,
    );
    store.finish_light_updates();
    assert_eq!(sky(&store, 10, GROUND, 10), 15);
}

#[test]
fn glass_and_leaves_let_sky_light_through() {
    let mut store = ChunkStore::flat();
    fill(
        &mut store,
        (0, GROUND + 2, 0),
        (20, GROUND + 2, 20),
        blocks::GLASS,
    );
    fill(
        &mut store,
        (30, GROUND + 2, 0),
        (50, GROUND + 2, 20),
        blocks::OAK_LEAVES,
    );
    store.finish_light_updates();

    assert_eq!(sky(&store, 10, GROUND, 10), 15);
    // 树叶让光减 1，所以树叶下面不再是光源，往下每格再减 1。
    assert_eq!(sky(&store, 40, GROUND + 2, 10), 14);
    assert_eq!(sky(&store, 40, GROUND + 1, 10), 13);
    assert_eq!(sky(&store, 40, GROUND, 10), 12);
}

#[test]
fn building_in_the_sky_stores_light_around_the_new_section() {
    let mut store = ChunkStore::flat();
    place(&mut store, 4, 200, 4, blocks::STONE);
    store.finish_light_updates();

    let top = (200 >> 4) + 1;
    assert!(section(&store, LightLayer::Sky, top).is_some());
    assert!(section(&store, LightLayer::Sky, top + 1).is_none());
    assert_eq!(sky(&store, 4, 199, 4), 14);
    assert_eq!(sky(&store, 4, 201, 4), 15);
    // 方块下面整列都不再是光源，光从旁边的列照进来。
    assert_eq!(sky(&store, 4, GROUND, 4), 14);
    // 中间没有方块的段不存光照，读到的是上面最近一段的最底一层。
    assert_eq!(sky(&store, 4, 100, 4), 14);
    assert_eq!(sky(&store, 5, 100, 4), 15);

    place(&mut store, 4, 200, 4, blocks::AIR);
    store.finish_light_updates();
    assert!(section(&store, LightLayer::Sky, top).is_none());
}

#[test]
fn changed_sections_are_reported_per_chunk() {
    let mut store = ChunkStore::flat();
    store.take_light_changes();
    place(&mut store, 15, GROUND, 8, blocks::GLOWSTONE);
    store.finish_light_updates();

    let changes = store.take_light_changes();
    let ground = 1 << (-4 - MIN_LIGHT_SECTION);
    let at = |x, z| {
        changes
            .get(&ChunkPos::new(x, z))
            .copied()
            .unwrap_or_default()
    };
    assert_ne!(at(0, 0).block & ground, 0);
    assert_ne!(at(1, 0).block & ground, 0);
    assert_eq!(at(5, 5), LightChanges::default());
    assert!(store.take_light_changes().is_empty());
}

/// 一片区域里每一格的天空光和方块光。
fn snapshot(
    store: &ChunkStore,
    (x0, y0, z0): (i32, i32, i32),
    (x1, y1, z1): (i32, i32, i32),
) -> Vec<u8> {
    let mut values = Vec::new();
    for x in x0..=x1 {
        for y in y0..=y1 {
            for z in z0..=z1 {
                values.extend([sky(store, x, y, z), block(store, x, y, z)]);
            }
        }
    }
    values
}

#[test]
fn a_round_can_stop_halfway_and_readers_keep_the_last_finished_light() {
    let mut store = ChunkStore::flat();
    // 1100 格石头：前 1000 个任务排成一轮，剩下的到算光照时再排一轮。改方块时一轮也不算。
    fill(
        &mut store,
        (0, GROUND, 0),
        (10, GROUND + 9, 9),
        blocks::STONE,
    );
    let area = ((-2, GROUND - 1, -2), (12, GROUND + 11, 11));
    let before = snapshot(&store, area.0, area.1);
    assert!(store.has_pending_light_updates());

    // 每次只做一点。第一轮算完以前，读到的一直是改之前的光照，也报不出变了的段。
    let mut calls = 0;
    while sky(&store, 0, GROUND, 0) == 15 {
        store.run_light_updates(500);
        calls += 1;
        let changes = store.take_light_changes();
        if sky(&store, 0, GROUND, 0) == 15 {
            assert_eq!(snapshot(&store, area.0, area.1), before);
            assert!(changes.is_empty());
        } else {
            assert!(!changes.is_empty());
        }
    }
    assert!(calls > 1);
    // 第一轮算完了，第二轮那 100 格还没算。
    assert_eq!(sky(&store, 10, GROUND + 9, 9), 15);
    assert!(store.has_pending_light_updates());

    store.finish_light_updates();
    assert_eq!(sky(&store, 10, GROUND + 9, 9), 0);
    assert!(!store.has_pending_light_updates());
}

/// 几个 tick 的改动：屋顶、火把、天上的方块（新建区块段）和拆掉它（丢掉区块段）。
/// 每个 tick 改完都调一次 `step`，它决定这个 tick 算多少光照。
fn mixed_ticks(mut step: impl FnMut(&mut ChunkStore)) -> ChunkStore {
    let mut store = ChunkStore::flat();
    fill(
        &mut store,
        (0, GROUND + 3, 0),
        (20, GROUND + 3, 20),
        blocks::STONE,
    );
    place(&mut store, 5, GROUND, 5, blocks::TORCH);
    step(&mut store);
    place(&mut store, 10, 200, 10, blocks::STONE);
    fill(
        &mut store,
        (8, GROUND, 8),
        (12, GROUND + 2, 12),
        blocks::GLASS,
    );
    place(&mut store, 15, GROUND + 3, 15, blocks::GLOWSTONE);
    step(&mut store);
    place(&mut store, 10, 200, 10, blocks::AIR);
    fill(
        &mut store,
        (0, GROUND + 3, 0),
        (20, GROUND + 3, 10),
        blocks::AIR,
    );
    step(&mut store);
    store.finish_light_updates();
    store
}

#[test]
fn stopping_after_every_step_gives_the_same_light() {
    let mut at_once = mixed_ticks(ChunkStore::finish_light_updates);
    // 每个 tick 只做一步，一轮要分好多个 tick 算，后面的改动在它算到一半时就来了。
    let mut step_by_step = mixed_ticks(|store| {
        for _ in 0..50 {
            store.run_light_updates(1);
        }
    });

    assert_eq!(
        step_by_step.take_light_changes(),
        at_once.take_light_changes()
    );
    let area = ((-16, GROUND - 2, -16), (36, GROUND + 20, 36));
    assert_eq!(
        snapshot(&step_by_step, area.0, area.1),
        snapshot(&at_once, area.0, area.1)
    );
    for y in MIN_LIGHT_SECTION..MIN_LIGHT_SECTION + LIGHT_SECTION_COUNT as i32 {
        for layer in [LightLayer::Sky, LightLayer::Block] {
            let section = SectionPos::new(0, y, 0);
            assert_eq!(
                step_by_step.light().section(layer, section),
                at_once.light().section(layer, section)
            );
        }
    }
}

/// 999 块远处的石头加一支火把，正好 1000 个任务排成一轮，然后马上把火把围死。
/// `eager` 时排好就算，像官方的光照线程一直跟得上；不然等全部改完才算。
fn caged_torch(eager: bool) -> ChunkStore {
    let mut store = ChunkStore::flat();
    for x in 0..999 {
        place(&mut store, 1000 + x, GROUND, 0, blocks::STONE);
    }
    // 火把在区块段正中间，光照出这一段，被围死以后照不出去。
    let (x, y, z) = (8, GROUND + 4, 8);
    place(&mut store, x, y, z, blocks::TORCH);
    if eager {
        store.finish_light_updates();
    }
    for (dx, dy, dz) in [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ] {
        place(&mut store, x + dx, y + dy, z + dz, blocks::STONE);
    }
    store.finish_light_updates();
    store
}

#[test]
fn a_late_round_reads_the_blocks_from_when_it_was_formed() {
    let mut eager = caged_torch(true);
    let mut late = caged_torch(false);

    let changes = late.take_light_changes();
    // 那一轮照出去过，说明它没看到后来的笼子。
    assert!(
        changes
            .get(&ChunkPos::new(1, 0))
            .is_some_and(|change| change.block != 0)
    );
    assert_eq!(changes, eager.take_light_changes());
    for x in -8..24 {
        for y in GROUND - 2..GROUND + 20 {
            for z in -8..24 {
                assert_eq!(sky(&late, x, y, z), sky(&eager, x, y, z));
                assert_eq!(block(&late, x, y, z), block(&eager, x, y, z));
            }
        }
    }
    assert_eq!(block(&late, 8, GROUND + 4, 8), 14);
    assert_eq!(block(&late, 8, GROUND + 4, 10), 0);
}
