//! 下马的落点：官方 `AbstractHorse.getDismountLocationForPassenger`，也就是
//! `DismountHelper` 那一套。玩家先往主手一侧、再往另一侧找能站的地方，
//! 两个方向都失败就落在载具的脚底（官方返回 `this.position()`）。
//!
//! Where a dismounting player lands: vanilla
//! `AbstractHorse.getDismountLocationForPassenger`, the `DismountHelper` path.
//! The player is tried on the main-hand side first and then the other side; if
//! neither works the vehicle's own position is used (`this.position()`).

use mc_bamboo_blocks::{
    BlockState,
    shape::{self, Aabb, CollisionContext, EntityContext},
};
use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::blocks;
use mc_bamboo_world::store::ChunkStore;

use crate::player::Player;

/// `Player.getDismountPoses()` 的三套尺寸（宽，高），按官方顺序：
/// 站立、潜行、游泳。
///
/// The three `Player.getDismountPoses()` sizes (width, height) in vanilla order:
/// standing, crouching, swimming.
const PLAYER_POSES: [(f64, f64); 3] = [(0.6, 1.8), (0.6, 1.5), (0.6, 0.6)];

/// 官方 `Entity.getCollisionHorizontalEscapeVector`。
///
/// Vanilla `Entity.getCollisionHorizontalEscapeVector`.
fn escape_vector(vehicle_width: f64, passenger_width: f64, degrees: f32) -> Vec3 {
    let distance = (vehicle_width + passenger_width + 1.0e-5) / 2.0;
    let radians = degrees * (std::f32::consts::PI / 180.0);
    let direction_x = -f64::from(mth::sin(f64::from(radians)));
    let direction_z = f64::from(mth::cos(f64::from(radians)));
    let scale = direction_x.abs().max(direction_z.abs());
    Vec3::new(
        direction_x * distance / scale,
        0.0,
        direction_z * distance / scale,
    )
}

/// 一块方块的碰撞形状在这个位置上的最高点（相对这一格），空形状算 0。
///
/// The highest point of a block's collision shape at this position, relative to
/// the block; an empty shape counts as 0.
fn shape_max_y(state: BlockState, pos: BlockPos) -> f64 {
    shape::collision_boxes(state, pos, CollisionContext::Empty)
        .iter()
        .map(|part| part.max_y)
        .fold(0.0, f64::max)
}

/// 官方 `BlockGetter.getBlockFloorHeight(BlockPos)`。
///
/// Vanilla `BlockGetter.getBlockFloorHeight(BlockPos)`.
fn block_floor_height(state_at: &mut dyn FnMut(BlockPos) -> BlockState, pos: BlockPos) -> f64 {
    if shape::collision_boxes(state_at(pos), pos, CollisionContext::Empty).is_empty() {
        let below = shape_max_y(state_at(pos.below()), pos.below());
        return if below >= 1.0 {
            below - 1.0
        } else {
            f64::NEG_INFINITY
        };
    }
    shape_max_y(state_at(pos), pos)
}

/// 官方 `DismountHelper.isBlockFloorValid`。
///
/// Vanilla `DismountHelper.isBlockFloorValid`.
fn floor_valid(height: f64) -> bool {
    !height.is_infinite() && height < 1.0
}

/// 这个盒子有没有碰到方块的碰撞形状，`level.getBlockCollisions(entity, box)` 为空。
///
/// Whether the box touches any block collision shape, i.e. vanilla
/// `level.getBlockCollisions(entity, box)` is empty.
fn collides(
    state_at: &mut dyn FnMut(BlockPos) -> BlockState,
    context: CollisionContext,
    bounds: Aabb,
) -> bool {
    let from = BlockPos::containing(Vec3::new(bounds.min_x, bounds.min_y, bounds.min_z));
    let to = BlockPos::containing(Vec3::new(bounds.max_x, bounds.max_y, bounds.max_z));
    for y in from.y..=to.y {
        for z in from.z..=to.z {
            for x in from.x..=to.x {
                let pos = BlockPos::new(x, y, z);
                if shape::collision_boxes(state_at(pos), pos, context)
                    .iter()
                    .any(|part| part.at(pos).intersects(bounds))
                {
                    return true;
                }
            }
        }
    }
    false
}

/// 某个姿势的局部碰撞箱，`LivingEntity.getLocalBoundsForPose`。
///
/// A pose's local box, `LivingEntity.getLocalBoundsForPose`.
fn pose_bounds(width: f64, height: f64, location: Vec3) -> Aabb {
    let half = width / 2.0;
    Aabb::new(
        location.x - half,
        location.y,
        location.z - half,
        location.x + half,
        location.y + height,
        location.z + half,
    )
}

/// 官方的下马落点。载具位置取自公开快照；找不到返回 `None`，调用方保持原位。
///
/// The vanilla dismount location. The vehicle comes from the public snapshot; a
/// `None` return means the caller keeps the player where it is.
pub(crate) fn dismount_location(
    chunks: &ChunkStore,
    player: &Player,
    vehicle_position: Vec3,
    vehicle_width: f64,
    vehicle_height: f64,
    vehicle_yaw: f32,
) -> Option<Vec3> {
    let context = CollisionContext::Entity(EntityContext {
        fall_distance: player.motion.fall_distance,
        bottom: player.position().y,
        falling_block: false,
        descending: player.sync.shift_key_down,
        placement: false,
        powder_snow_walkable: false,
    });
    let mut state_at = |pos: BlockPos| BlockState(chunks.block(pos).unwrap_or(blocks::VOID_AIR));
    // `getDismountLocationInDirection` 的搜索上限：载具顶再高 0.75。
    //
    // The search ceiling in `getDismountLocationInDirection`: 0.75 above the top.
    let limit = vehicle_position.y + vehicle_height + 0.75;
    let base = vehicle_position.y.floor();
    // 主手是右手（Bamboo 没有左手玩家），官方先试 +90 再试 -90。
    //
    // The main arm is the right one (no left-handed players), so vanilla tries
    // +90 first and -90 second.
    for degrees in [vehicle_yaw + 90.0, vehicle_yaw - 90.0] {
        let direction = escape_vector(vehicle_width, PLAYER_POSES[0].0, degrees);
        let target_x = vehicle_position.x + direction.x;
        let target_z = vehicle_position.z + direction.z;
        for &(width, height) in &PLAYER_POSES {
            let mut y = base as i32;
            loop {
                let pos = BlockPos::new(target_x.floor() as i32, y, target_z.floor() as i32);
                let floor = block_floor_height(&mut state_at, pos);
                if f64::from(y) + floor > limit {
                    break;
                }
                if floor_valid(floor) {
                    let location = Vec3::new(target_x, f64::from(y) + floor, target_z);
                    if !collides(&mut state_at, context, pose_bounds(width, height, location)) {
                        return Some(location);
                    }
                }
                y += 1;
                if f64::from(y) >= limit {
                    break;
                }
            }
        }
    }
    Some(vehicle_position)
}
