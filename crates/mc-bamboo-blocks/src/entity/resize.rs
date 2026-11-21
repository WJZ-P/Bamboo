//! 生物尺寸增大时，按官方规则在旧碰撞箱附近寻找空位。
//!
//! Vanilla free-position search near the old bounding box after an entity grows.

use mc_bamboo_core::{Aabb, Axis, Vec3, mth};

use super::{Entity, ctx::BlockView};
use crate::CollisionContext;

/// `Entity.fudgePositionAfterSizeChange`：先尝试新尺寸，宽高同时增大时再尝试保留旧高度。
///
/// `Entity.fudgePositionAfterSizeChange`: try the new size first and retry at the old
/// height if both dimensions grew.
pub(super) fn fudge_position(
    view: &dyn BlockView,
    entity: &Entity,
    (old_width, old_height): (f32, f32),
) -> Option<Vec3> {
    let (width, height) = entity.dimensions;
    let old_half_height = f64::from(old_height) / 2.0;
    let center = entity.position.add(Vec3::new(0.0, old_half_height, 0.0));
    let width_delta = f64::from((width - old_width).max(0.0)) + 1.0e-6;
    let height_delta = f64::from((height - old_height).max(0.0)) + 1.0e-6;
    let context = entity.collision_context();
    let (sx, sy) = (f64::from(width), f64::from(height));
    let allowed = of_size(center, width_delta, height_delta, width_delta);
    if let Some(free) = find_free_position(view, context, allowed, center, [sx, sy, sx]) {
        return Some(free.add(Vec3::new(0.0, -sy / 2.0, 0.0)));
    }
    if width <= old_width || height <= old_height {
        return None;
    }
    let allowed = of_size(center, width_delta, 1.0e-6, width_delta);
    find_free_position(
        view,
        context,
        allowed,
        center,
        [sx, f64::from(old_height), sx],
    )
    .map(|free| free.add(Vec3::new(0.0, -old_half_height + 1.0e-6, 0.0)))
}

/// `AABB.ofSize`。
fn of_size(center: Vec3, x: f64, y: f64, z: f64) -> Aabb {
    Aabb::new(
        center.x - x / 2.0,
        center.y - y / 2.0,
        center.z - z / 2.0,
        center.x + x / 2.0,
        center.y + y / 2.0,
        center.z + z / 2.0,
    )
}

/// `CollisionGetter.findFreePosition`：把边界内的方块碰撞盒各向外扩半个尺寸，从允许的
/// 中心盒里挖掉，再取离首选点最近的点。
///
/// `CollisionGetter.findFreePosition`: inflate in-border block boxes by half the size,
/// cut them from the allowed centres and take the point nearest the preferred one.
fn find_free_position(
    view: &dyn BlockView,
    context: CollisionContext,
    allowed: Aabb,
    preferred: Vec3,
    [sx, sy, sz]: [f64; 3],
) -> Option<Vec3> {
    // `Shapes.create` 对任何一边小于 1e-7 的盒子给空形状。
    // `Shapes.create` yields an empty shape when a side is below 1e-7.
    if [Axis::X, Axis::Y, Axis::Z]
        .into_iter()
        .any(|axis| allowed.max(axis) - allowed.min(axis) < 1.0e-7)
    {
        return None;
    }
    let border = view.world_border();
    let mut solids = Vec::new();
    for (_, shapes) in view.block_collisions(allowed.inflate(sx, sy, sz), context) {
        let Some(bounds) = shapes.iter().copied().reduce(union) else {
            continue;
        };
        if border.is_within_bounds_aabb(bounds) {
            solids.extend(
                shapes
                    .iter()
                    .map(|shape| shape.inflate(sx / 2.0, sy / 2.0, sz / 2.0)),
            );
        }
    }
    closest_free_point(allowed, &solids, preferred)
}

fn union(a: Aabb, b: Aabb) -> Aabb {
    Aabb::new(
        a.min_x.min(b.min_x),
        a.min_y.min(b.min_y),
        a.min_z.min(b.min_z),
        a.max_x.max(b.max_x),
        a.max_y.max(b.max_y),
        a.max_z.max(b.max_z),
    )
}

/// `Shapes.join(allowed, solids, ONLY_FIRST).closestPointTo(point)`：按所有盒子的边把
/// 允许盒切成格子，没被实心盒盖住的格子里取钳制后最近的点。
///
/// `Shapes.join(allowed, solids, ONLY_FIRST).closestPointTo(point)`: cut the allowed
/// box along every box edge and take the nearest clamped point among uncovered cells.
fn closest_free_point(allowed: Aabb, solids: &[Aabb], point: Vec3) -> Option<Vec3> {
    let cuts = |axis: Axis| {
        let (low, high) = (allowed.min(axis), allowed.max(axis));
        let mut values = vec![low, high];
        for solid in solids {
            for value in [solid.min(axis), solid.max(axis)] {
                if value > low && value < high {
                    values.push(value);
                }
            }
        }
        values.sort_by(f64::total_cmp);
        values.dedup();
        values
    };
    let (xs, ys, zs) = (cuts(Axis::X), cuts(Axis::Y), cuts(Axis::Z));
    let mut best: Option<(f64, Vec3)> = None;
    for x in xs.windows(2) {
        for y in ys.windows(2) {
            for z in zs.windows(2) {
                let mid = Vec3::new(
                    (x[0] + x[1]) / 2.0,
                    (y[0] + y[1]) / 2.0,
                    (z[0] + z[1]) / 2.0,
                );
                let covered = solids.iter().any(|solid| {
                    solid.min_x < mid.x
                        && mid.x < solid.max_x
                        && solid.min_y < mid.y
                        && mid.y < solid.max_y
                        && solid.min_z < mid.z
                        && mid.z < solid.max_z
                });
                if covered {
                    continue;
                }
                let clamped = Vec3::new(
                    mth::clamp(point.x, x[0], x[1]),
                    mth::clamp(point.y, y[0], y[1]),
                    mth::clamp(point.z, z[0], z[1]),
                );
                let distance = point.distance_to_sqr(clamped);
                if best.is_none_or(|(nearest, _)| distance < nearest) {
                    best = Some((distance, clamped));
                }
            }
        }
    }
    best.map(|(_, clamped)| clamped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_free_spot_search_lifts_the_centre_out_of_the_floor() {
        let allowed = of_size(
            Vec3::new(0.5, 64.0, 0.5),
            0.2 + 1.0e-6,
            0.35 + 1.0e-6,
            0.2 + 1.0e-6,
        );
        let floor = Aabb::new(0.0, 63.0, 0.0, 1.0, 64.0, 1.0).inflate(0.1, 0.175, 0.1);
        let free = closest_free_point(allowed, &[floor], Vec3::new(0.5, 64.0, 0.5)).unwrap();
        assert_eq!(free, Vec3::new(0.5, 64.0 + 0.175, 0.5));
        let buried = Aabb::new(-5.0, 60.0, -5.0, 5.0, 70.0, 5.0);
        assert_eq!(
            closest_free_point(allowed, &[buried], Vec3::new(0.5, 64.0, 0.5)),
            None
        );
    }
}
