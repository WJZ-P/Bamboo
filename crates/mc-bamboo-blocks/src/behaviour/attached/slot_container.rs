//! `SelectableSlotContainer`：架子和雕纹书架按点击点选格子。

use mc_bamboo_core::Direction;

use crate::context::BlockHit;

/// `getHitSlot`：点在方块朝外的那一面时，点中的是第几格。
pub(super) fn hit_slot(
    hit: &BlockHit,
    facing: Direction,
    rows: usize,
    columns: usize,
) -> Option<usize> {
    let (x, y) = relative_hit(hit, facing)?;
    Some(section(x, columns) + section(1.0 - y, rows) * columns)
}

/// `getRelativeHitCoordinatesForBlockFace`：点击点在这一面上的坐标，从面对这一面的人看过去。
fn relative_hit(hit: &BlockHit, facing: Direction) -> Option<(f32, f32)> {
    if facing != hit.face {
        return None;
    }
    let corner = hit.pos.relative(hit.face).corner();
    let relative = hit.location.subtract(corner);
    let y = relative.y as f32;
    match hit.face {
        Direction::North => Some(((1.0 - relative.x) as f32, y)),
        Direction::South => Some((relative.x as f32, y)),
        Direction::West => Some((relative.z as f32, y)),
        Direction::East => Some(((1.0 - relative.z) as f32, y)),
        Direction::Down | Direction::Up => None,
    }
}

/// `getSection`：把 0 到 1 的坐标分成 `sections` 段，按像素算，浮点的舍入要和官方一样。
fn section(relative: f32, sections: usize) -> usize {
    let pixel = relative * 16.0;
    let size = 16.0 / sections as f32;
    ((pixel / size).floor().max(0.0) as usize).min(sections - 1)
}
