//! 光照引擎在超平坦世界里的行为。逐格和官方比较的检查在 `mc-bamboo-vanilla light` 里。

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};
use mc_bamboo_data::blocks;

use super::{DataLayer, LightChanges, MIN_LIGHT_SECTION, SectionPos};
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

    store.run_light_updates();
    assert_eq!(block(&store, 8, GROUND, 8), 14);
    assert_eq!(block(&store, 9, GROUND, 8), 13);
    assert_eq!(block(&store, 8, GROUND + 3, 8), 11);
    assert_eq!(block(&store, 21, GROUND, 8), 1);
    assert_eq!(block(&store, 22, GROUND, 8), 0);
    // 草方块不透光。
    assert_eq!(block(&store, 8, GROUND - 1, 8), 0);

    place(&mut store, 8, GROUND, 8, blocks::AIR);
    store.run_light_updates();
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
    store.run_light_updates();

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
    store.run_light_updates();
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
    store.run_light_updates();

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
    store.run_light_updates();

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
    store.run_light_updates();
    assert!(section(&store, LightLayer::Sky, top).is_none());
}

#[test]
fn changed_sections_are_reported_per_chunk() {
    let mut store = ChunkStore::flat();
    store.take_light_changes();
    place(&mut store, 15, GROUND, 8, blocks::GLOWSTONE);
    store.run_light_updates();

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

#[test]
fn many_edits_run_in_batches_of_a_thousand_tasks() {
    let mut store = ChunkStore::flat();
    // 1100 格石头：前 1000 个任务当场算掉，剩下的等到下一次。
    fill(
        &mut store,
        (0, GROUND, 0),
        (10, GROUND + 9, 9),
        blocks::STONE,
    );
    assert_eq!(sky(&store, 0, GROUND, 0), 0);
    assert_eq!(sky(&store, 10, GROUND + 9, 9), 15);

    store.run_light_updates();
    assert_eq!(sky(&store, 10, GROUND + 9, 9), 0);
}
