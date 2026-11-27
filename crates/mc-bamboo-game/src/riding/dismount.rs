//! 下马的落点：官方 `AbstractHorse.getDismountLocationForPassenger`，也就是
//! `DismountHelper` 那一套。玩家先往主手一侧、再往另一侧找能站的地方，
//! 两个方向都失败就落在载具的脚底（官方返回 `this.position()`）。
//!
//! Where a dismounting player lands: vanilla
//! `AbstractHorse.getDismountLocationForPassenger`, the `DismountHelper` path.
//! The player is tried on the main-hand side first and then the other side; if
//! neither works the vehicle's own position is used (`this.position()`).

use mc_bamboo_blocks::{
    BlockState, WorldBorder,
    shape::{self, Aabb, CollisionContext, EntityContext},
};
use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::blocks;
use mc_bamboo_protocol::packet::play::HumanoidArm;
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
    border: &WorldBorder,
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
    // `getDismountLocationForPassenger` 先试主手一侧、再试另一侧：
    // 主手一侧是 `yRot + (mainArm == RIGHT ? 90 : -90)`，另一侧是
    // `yRot + (mainArm == LEFT ? 90 : -90)`。两个方向恒为 `+90` / `-90`，
    // 只有先后顺序随主手变。
    //
    // `getDismountLocationForPassenger` tries the main-hand side first, then the
    // other: the main-hand side is `yRot + (mainArm == RIGHT ? 90 : -90)` and the
    // other is `yRot + (mainArm == LEFT ? 90 : -90)`. The two directions are
    // always `+90` / `-90`; only their order follows the main arm.
    let first = if player.client.main_hand == HumanoidArm::Right {
        90.0
    } else {
        -90.0
    };
    for degrees in [vehicle_yaw + first, vehicle_yaw - first] {
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
                    let bounds = pose_bounds(width, height, location);
                    if !collides(&mut state_at, context, bounds)
                        && border.is_within_bounds_aabb(bounds)
                    {
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

/// 官方 `AbstractBoat.getDismountLocationForPassenger`：用
/// `getCollisionHorizontalEscapeVector(bbWidth * SQRT_OF_TWO, passengerWidth,
/// passenger.getYRot())` 找一点，检查该格与下一格的地面高度，按三种姿势试
/// `DismountHelper.canDismountTo`；脚下是水、或都不行时退回
/// `VehicleEntity.getDismountLocationForPassenger`（船的 AABB 顶）。
///
/// Vanilla `AbstractBoat.getDismountLocationForPassenger`.
pub(crate) fn boat_dismount_location(
    chunks: &ChunkStore,
    border: &WorldBorder,
    player: &Player,
    vehicle_position: Vec3,
    vehicle_width: f64,
    vehicle_box_max_y: f64,
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
    // `Mth.SQRT_OF_TWO` 是 f32；`getBbWidth() * SQRT_OF_TWO` 先按 f32 乘再提升。
    let escape = escape_vector(
        f64::from((vehicle_width as f32) * std::f32::consts::SQRT_2),
        PLAYER_POSES[0].0,
        player.yaw,
    );
    let target_x = vehicle_position.x + escape.x;
    let target_z = vehicle_position.z + escape.z;
    let pos = BlockPos::containing(Vec3::new(target_x, vehicle_box_max_y, target_z));
    let below = pos.below();
    // `level.isWaterAt(below)`。
    let water_below = state_at(below)
        .fluid()
        .is_tagged(mc_bamboo_data::tags::fluid_tags::WATER);
    if !water_below {
        let mut candidates = Vec::new();
        let floor = block_floor_height(&mut state_at, pos);
        if floor_valid(floor) {
            candidates.push(Vec3::new(target_x, f64::from(pos.y) + floor, target_z));
        }
        let floor_below = block_floor_height(&mut state_at, below);
        if floor_valid(floor_below) {
            candidates.push(Vec3::new(
                target_x,
                f64::from(below.y) + floor_below,
                target_z,
            ));
        }
        for &(width, height) in &PLAYER_POSES {
            for location in &candidates {
                let bounds = pose_bounds(width, height, *location);
                if !collides(&mut state_at, context, bounds) && border.is_within_bounds_aabb(bounds)
                {
                    return Some(*location);
                }
            }
        }
    }
    Some(Vec3::new(
        vehicle_position.x,
        vehicle_box_max_y,
        vehicle_position.z,
    ))
}

/// 官方 `Animal.getDismountLocationForPassenger`：朝向 `Direction.fromYRot(yRot)`，按
/// `DismountHelper.offsetsForDirection` 的八个相邻格（右、左、后右、后左、前右、前左、后、前）
/// 依次试，每格对三种姿势取格子中心上方的落点；全都不行落在载具包围盒顶上。
///
/// Vanilla `Animal.getDismountLocationForPassenger`: face `Direction.fromYRot(yRot)` and try
/// the eight `DismountHelper.offsetsForDirection` cells in order, three poses each at the
/// centre of the cell; if none fits the passenger lands on top of the vehicle's box.
pub(crate) fn animal_dismount_location(
    chunks: &ChunkStore,
    border: &WorldBorder,
    player: &Player,
    vehicle_position: Vec3,
    vehicle_height: f64,
    vehicle_yaw: f32,
) -> Vec3 {
    // 水平方向的 2D 数据值：南 0、西 1、北 2、东 3；顺时针就是加一。
    //
    // Horizontal 2D data values: south 0, west 1, north 2, east 3; clockwise is plus one.
    const STEPS: [(i32, i32); 4] = [(0, 1), (-1, 0), (0, -1), (1, 0)];
    let facing = ((f64::from(vehicle_yaw) / 90.0 + 0.5).floor() as i64 & 3) as usize;
    let direction = STEPS[facing];
    let clockwise = STEPS[(facing + 1) & 3];
    let counter = (-clockwise.0, -clockwise.1);
    let back = (-direction.0, -direction.1);
    let add = |a: (i32, i32), b: (i32, i32)| (a.0 + b.0, a.1 + b.1);
    let offsets = [
        clockwise,
        counter,
        add(back, clockwise),
        add(back, counter),
        add(direction, clockwise),
        add(direction, counter),
        back,
        direction,
    ];
    let context = CollisionContext::Entity(EntityContext {
        fall_distance: player.motion.fall_distance,
        bottom: player.position().y,
        falling_block: false,
        descending: player.sync.shift_key_down,
        placement: false,
        powder_snow_walkable: false,
    });
    let mut state_at = |pos: BlockPos| BlockState(chunks.block(pos).unwrap_or(blocks::VOID_AIR));
    let base = BlockPos::containing(vehicle_position);
    for &(width, height) in &PLAYER_POSES {
        for &(dx, dz) in &offsets {
            let pos = BlockPos::new(base.x + dx, base.y, base.z + dz);
            let floor = block_floor_height(&mut state_at, pos);
            if !floor_valid(floor) {
                continue;
            }
            let location = Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y) + floor,
                f64::from(pos.z) + 0.5,
            );
            let bounds = pose_bounds(width, height, location);
            if !collides(&mut state_at, context, bounds) && border.is_within_bounds_aabb(bounds) {
                return location;
            }
        }
    }
    Vec3::new(
        vehicle_position.x,
        vehicle_position.y + vehicle_height,
        vehicle_position.z,
    )
}

/// `DismountHelper.nonClimbableShape` 过滤可攀爬方块后的碰撞最高点。
fn non_climbable_shape_max_y(state: BlockState, pos: BlockPos) -> Option<f64> {
    if state.is_tagged(mc_bamboo_data::tags::block_tags::CLIMBABLE) {
        return None;
    }
    let boxes = shape::collision_boxes(state, pos, CollisionContext::Empty);
    if boxes.is_empty() {
        return None;
    }
    Some(boxes.iter().map(|part| part.max_y).fold(0.0, f64::max))
}

/// `AbstractMinecart` 用的 `level.getBlockFloorHeight(nonClimbableShape(pos), nonClimbableShape(pos.below()))`。
fn non_climbable_floor_height(
    state_at: &mut dyn FnMut(BlockPos) -> BlockState,
    pos: BlockPos,
) -> f64 {
    if let Some(top) = non_climbable_shape_max_y(state_at(pos), pos) {
        return top;
    }
    match non_climbable_shape_max_y(state_at(pos.below()), pos.below()) {
        Some(below) if below >= 1.0 => below - 1.0,
        _ => f64::NEG_INFINITY,
    }
}

/// 官方 `AbstractMinecart.getDismountLocationForPassenger`：按
/// `OldMinecartBehavior.getMotionDirection`（`flipped ? dir.opposite.clockWise : dir.clockWise`）
/// 取 `DismountHelper.offsetsForDirection` 八邻格，按三种姿势和 `POSE_DISMOUNT_HEIGHTS`
/// （站立 / 潜行 `[0, 1, -1]`、游泳 `[0, 1]`）依次找落点；全都不行时落在车顶。
///
/// Vanilla `AbstractMinecart.getDismountLocationForPassenger`: scan the eight
/// `offsetsForDirection(getMotionDirection())` cells across `POSE_DISMOUNT_HEIGHTS`
/// (`[0, 1, -1]` for standing/crouching, `[0, 1]` for swimming), falling back to
/// the top of the cart's bounding box.
pub(crate) fn minecart_dismount_location(
    chunks: &ChunkStore,
    border: &WorldBorder,
    player: &Player,
    vehicle_position: Vec3,
    vehicle_box_max_y: f64,
    vehicle_yaw: f32,
    flipped: bool,
) -> Option<Vec3> {
    const STEPS: [(i32, i32); 4] = [(0, 1), (-1, 0), (0, -1), (1, 0)];
    let facing = ((f64::from(vehicle_yaw) / 90.0 + 0.5).floor() as i64 & 3) as usize;
    let motion = if flipped {
        (facing + 3) & 3
    } else {
        (facing + 1) & 3
    };
    let direction = STEPS[motion];
    let clockwise = STEPS[(motion + 1) & 3];
    let counter = (-clockwise.0, -clockwise.1);
    let back = (-direction.0, -direction.1);
    let add = |a: (i32, i32), b: (i32, i32)| (a.0 + b.0, a.1 + b.1);
    let offsets = [
        clockwise,
        counter,
        add(back, clockwise),
        add(back, counter),
        add(direction, clockwise),
        add(direction, counter),
        back,
        direction,
    ];
    let context = CollisionContext::Entity(EntityContext {
        fall_distance: player.motion.fall_distance,
        bottom: player.position().y,
        falling_block: false,
        descending: player.sync.shift_key_down,
        placement: false,
        powder_snow_walkable: false,
    });
    let mut state_at = |pos: BlockPos| BlockState(chunks.block(pos).unwrap_or(blocks::VOID_AIR));
    let origin = BlockPos::containing(vehicle_position);
    let pose_heights: [(f64, f64, &[i32]); 3] = [
        (PLAYER_POSES[0].0, PLAYER_POSES[0].1, &[0, 1, -1]),
        (PLAYER_POSES[1].0, PLAYER_POSES[1].1, &[0, 1, -1]),
        (PLAYER_POSES[2].0, PLAYER_POSES[2].1, &[0, 1]),
    ];
    for (width, height, heights) in pose_heights {
        let clamped_width = width.min(1.0);
        for &dy in heights {
            for &(dx, dz) in &offsets {
                let pos = BlockPos::new(origin.x + dx, origin.y + dy, origin.z + dz);
                let floor = non_climbable_floor_height(&mut state_at, pos);
                if !floor_valid(floor) {
                    continue;
                }
                let location = Vec3::new(
                    f64::from(pos.x) + 0.5,
                    f64::from(pos.y) + floor,
                    f64::from(pos.z) + 0.5,
                );
                let bounds = pose_bounds(clamped_width, height, location);
                if !collides(&mut state_at, context, bounds) && border.is_within_bounds_aabb(bounds)
                {
                    return Some(location);
                }
            }
        }
    }
    Some(Vec3::new(
        vehicle_position.x,
        vehicle_box_max_y,
        vehicle_position.z,
    ))
}
