use mc_bamboo_blocks::state::{BERRIES, BlockState, DOWN};
use mc_bamboo_core::ChunkPos;
use mc_bamboo_data::blocks;
use mc_bamboo_world::light::{
    MIN_LIGHT_SECTION,
    generate::{GeneratedLight, generate_light_from_layers},
};

use super::*;

fn empty_run() -> FeatureRun {
    FeatureRun {
        deltas: Default::default(),
        block_entities: Default::default(),
        marks: Default::default(),
        entities: Default::default(),
    }
}

fn set(layers: &mut [Vec<u32>], x: i32, y: i32, z: i32, block: u32) {
    apply_block_deltas(layers, &[(x, y, z, block)]);
}

fn light(layers: &[Vec<Vec<u32>>; 9]) -> GeneratedLight {
    let columns: Vec<_> = (-1..=1)
        .flat_map(|z| (-1..=1).map(move |x| ChunkPos::new(x, z)))
        .zip(layers.iter().map(Vec::as_slice))
        .collect();
    generate_light_from_layers(ChunkPos::new(0, 0), &columns).unwrap()
}

fn probe(layers: &[Option<mc_bamboo_world::light::DataLayer>]) -> u8 {
    let index = (63 / 16 - MIN_LIGHT_SECTION) as usize;
    layers[index].as_ref().expect("probe layer").get(15, 15, 8)
}

#[test]
fn outer_feature_spill_changes_light_at_target_edge() {
    let ground: Vec<_> = (MIN_Y..MIN_Y + WORLD_HEIGHT)
        .map(|y| vec![if y <= 64 { blocks::STONE } else { blocks::AIR }; 256])
        .collect();
    let mut terrain = std::array::from_fn(|_| ground.clone());
    // 目标边缘的通道直达邻居的竖井（x=20）；(2,0) 来源不在光照目标的 3×3 内。
    // The target-edge tunnel leads to a shaft in the neighbor (x=20); source
    // (2,0) lies outside the lighting target's 3x3.
    set(&mut terrain[4], 15, 63, 8, blocks::AIR);
    for x in 16..=20 {
        set(&mut terrain[5], x, 63, 8, blocks::AIR);
    }
    set(&mut terrain[5], 20, 64, 8, blocks::AIR);

    let bare = light(&terrain);
    let mut compared = 0;
    for (name, x, y, state, sky) in [
        ("lava", 20, 63, blocks::LAVA, false),
        (
            "glow_lichen",
            20,
            63,
            BlockState(blocks::GLOW_LICHEN).with(DOWN, true).0,
            false,
        ),
        (
            "cave_vines",
            19,
            63,
            BlockState(blocks::CAVE_VINES).with(BERRIES, true).0,
            false,
        ),
        ("opaque", 20, 64, blocks::STONE, true),
    ] {
        let delta = (x, y, 8, state);
        let mut run = empty_run();
        run.deltas[3].push(delta);
        let outer = Arc::new(run);
        let empty = Arc::new(empty_run());
        let mut folded = terrain.clone();
        folded[5] = fold_block_layers(1, 0, &terrain[5], |x, z| {
            Ok(Arc::clone(if (x, z) == (2, 0) { &outer } else { &empty }))
        })
        .unwrap();
        let mut full = terrain.clone();
        set(&mut full[5], delta.0, delta.1, delta.2, delta.3);
        let exact = light(&folded);
        let old = light(&full);
        assert_eq!(exact.sky, old.sky, "{name} sky");
        assert_eq!(exact.block, old.block, "{name} block");
        let (present, omitted) = if sky {
            (probe(&exact.sky), probe(&bare.sky))
        } else {
            (probe(&exact.block), probe(&bare.block))
        };
        assert_ne!(present, omitted, "{name} omission went undetected");
        println!("{name}: light at (15,63,8) exact={present}, spill omitted={omitted}");
        compared += 1;
    }
    println!("spill_fixtures={compared}, exact_raw_layer_diffs=0, omissions_detected={compared}");
}

#[test]
fn light_fold_preserves_source_precedence_and_last_write() {
    let base = [vec![0; 256]];
    let mut own = empty_run();
    own.deltas[4] = vec![(0, MIN_Y, 0, 1), (0, MIN_Y, 0, 2)];
    let mut first_neighbor = empty_run();
    first_neighbor.deltas[8] = vec![(0, MIN_Y, 0, 3), (1, MIN_Y, 0, 4)];
    let mut last_neighbor = empty_run();
    last_neighbor.deltas[0] = vec![(0, MIN_Y, 0, 5), (1, MIN_Y, 0, 6)];
    let own = Arc::new(own);
    let first_neighbor = Arc::new(first_neighbor);
    let last_neighbor = Arc::new(last_neighbor);
    let empty = Arc::new(empty_run());
    let result = fold_block_layers(0, 0, &base, |x, z| {
        Ok(Arc::clone(match (x, z) {
            (0, 0) => &own,
            (-1, -1) => &first_neighbor,
            (1, 1) => &last_neighbor,
            _ => &empty,
        }))
    })
    .unwrap();
    assert_eq!((result[0][0], result[0][16]), (2, 4));
}
