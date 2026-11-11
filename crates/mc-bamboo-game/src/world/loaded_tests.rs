//! 已加载模式下的世界：进服、走动、走远再走回来内容还在、视距变化、离开、关服。
//!
//! 这里不用 Director（提交 5 才接进 `World::tick`），测试自己扮演 Director：
//! 用 `ChunkStore::load` 交付盒子、用 `Region::take_columns` + [`Parking`] 收回并停放、
//! 再用 `Parking::take` 装回去。这样提交 3 能单独验证「一局之内不丢内容」。

use std::collections::BTreeSet;

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};
use mc_bamboo_data::blocks;
use mc_bamboo_world::{column::ChunkColumn, director::ChunkBundle};

use crate::{
    region::{Region, parking::Parking},
    world::tests::Harness,
};

const ORIGIN: ChunkPos = ChunkPos::new(0, 0);
const FAR: ChunkPos = ChunkPos::new(4, 0);
const EDIT: BlockPos = BlockPos::new(2, -60, 2);

/// 把一个区块加载进世界（相当于 Director 的 `Deliver`）。
/// 停放表里没有就现造一个模板盒子；有的话先把世界层装进去，再还原上层盒子。
fn load(region: &mut Region, _parking: &Parking, pos: ChunkPos) {
    region
        .chunks
        .load(pos, Box::new(ChunkColumn::from_template()));
}

/// 把一个区块从世界收回并停放（相当于 Director 的 `Reclaim` + `Park`）。
fn unload(region: &mut Region, parking: &mut Parking, pos: ChunkPos) {
    assert!(region.chunks.is_loaded(pos));
    region.chunks.finish_light_updates();
    let box_ = region
        .take_columns(&[pos].into_iter().collect())
        .expect("前提满足")
        .remove(&pos)
        .expect("区块盒子在");
    let _ = region.chunks.unload(pos).expect("光照已算完");
    if !box_.is_pristine() {
        parking.park(pos, box_);
    }
}

/// 从停放表还原整个上层盒子（相当于 Director 重新交付）。
fn restore(region: &mut Region, parking: &mut Parking, pos: ChunkPos) {
    let box_ = match parking.take(pos) {
        Some(box_) => box_,
        None => return,
    };
    region
        .chunks
        .load(pos, Box::new(ChunkColumn::from_template()));
    region.insert_column(pos, box_).expect("前提满足");
}

#[test]
fn a_loaded_world_starts_empty_and_delivers_chunks() {
    let mut server = Harness::new_loaded(2);
    assert!(server.region().chunks.is_loaded_mode());
    assert_eq!(server.region().chunks.loaded_count(), Some(0));
    assert_eq!(server.region().chunks.block(EDIT), None);

    let parking = Parking::default();
    load(server.region(), &parking, ORIGIN);
    assert_eq!(server.region().chunks.loaded_count(), Some(1));
    assert_eq!(server.region().chunks.block(EDIT), Some(blocks::AIR));
    assert_eq!(
        server.region().chunks.light_value(LightLayer::Sky, EDIT),
        15
    );
}

#[test]
fn walking_away_and_back_keeps_block_and_light_content() {
    let mut server = Harness::new_loaded(2);
    let mut parking = Parking::default();
    load(server.region(), &parking, ORIGIN);
    load(server.region(), &parking, FAR);

    // 玩家在出生点改了一格。
    assert!(server.region().chunks.set_block(EDIT, blocks::STONE));
    server.region().chunks.finish_light_updates();
    assert_eq!(server.region().chunks.block(EDIT), Some(blocks::STONE));

    // 走远：收回出生点区块。
    unload(server.region(), &mut parking, ORIGIN);
    assert_eq!(server.region().chunks.block(EDIT), None);
    assert_eq!(parking.len(), 1, "改过的盒子要停放");

    // 走回来：从停放表装回去。
    load(server.region(), &parking, ORIGIN);
    restore(server.region(), &mut parking, ORIGIN);
    assert_eq!(server.region().chunks.block(EDIT), Some(blocks::STONE));
    assert_eq!(server.region().chunks.loaded_count(), Some(2));
}

#[test]
fn a_pristine_column_is_dropped_not_parked() {
    let mut server = Harness::new_loaded(2);
    let mut parking = Parking::default();
    load(server.region(), &parking, ORIGIN);
    unload(server.region(), &mut parking, ORIGIN);
    assert!(parking.is_empty(), "没改过的盒子不用停放");
    load(server.region(), &parking, ORIGIN);
    assert_eq!(server.region().chunks.block(EDIT), Some(blocks::AIR));
}

#[test]
fn view_distance_change_and_leave_keep_the_loaded_set_bounded() {
    let mut server = Harness::new_loaded(2);
    let mut parking = Parking::default();
    let area: Vec<ChunkPos> = (-2..=2)
        .flat_map(|x| (-2..=2).map(move |z| ChunkPos::new(x, z)))
        .collect();
    for &pos in &area {
        load(server.region(), &parking, pos);
    }
    assert_eq!(server.region().chunks.loaded_count(), Some(area.len()));

    // 模拟视距变小：只保留中心一圈。
    let keep: BTreeSet<ChunkPos> = area
        .iter()
        .copied()
        .filter(|pos| pos.x.abs() <= 1 && pos.z.abs() <= 1)
        .collect();
    for &pos in &area {
        if !keep.contains(&pos) {
            unload(server.region(), &mut parking, pos);
        }
    }
    assert_eq!(server.region().chunks.loaded_count(), Some(keep.len()));

    // 离开/关服：全部收回，停放表里只有改过的盒子（这里一个都没改过）。
    let remaining: Vec<ChunkPos> = area
        .iter()
        .copied()
        .filter(|pos| server.region().chunks.is_loaded(*pos))
        .collect();
    for pos in remaining {
        unload(server.region(), &mut parking, pos);
    }
    assert_eq!(server.region().chunks.loaded_count(), Some(0));
    assert!(parking.is_empty(), "没改过的区块都不该占用停放表");
    assert_eq!(parking.take_all().len(), 0);
}
