//! 实体运动和碰撞，官方 `Entity` 里和移动、落地、方块效果有关的那部分。
//!
//! 下落的方块和点燃的 TNT 走 `maxUpStep == 0` 的那条路。上台阶在 `sweep_movement` 里，
//! 只有玩家把台阶高度传进来时才发生。实体之间仍然不挡路；世界边界按官方
//! `Entity.collectCollidersIgnoringWorldBorder` 在实体离边界够近时加进碰撞体。
//!
//! Falling blocks and primed TNT take the `maxUpStep == 0` path. Step-up lives in
//! `sweep_movement` and runs only when a caller passes a step height. Entities still
//! do not block each other; the world border joins the colliders, as in vanilla
//! `Entity.collectCollidersIgnoringWorldBorder`, when the source is close enough.
//!
//! 方块碰撞按官方的 `VoxelShape.collide` 扫掠。官方的形状是体素网格，每个格子是一个
//! 体素；形状表里一个盒子就是一个格子，所以那条沿体素走的循环退化成逐盒子的判定，
//! 结果和官方逐位相同。

use std::cmp::Ordering;

use mc_bamboo_core::{Axis, BlockPos, Direction, Vec3, mth};
use mc_bamboo_data::{BlockAncestor, blocks, tags::block_tags};

use crate::{
    WorldBorder,
    entity::{Entity, Movement, axis_component, ctx::BlockView, with_axis},
    shape::{self, Aabb, BooleanOp, CollisionContext, EntityContext},
    state::BlockState,
    world,
};

/// `getDefaultGravity()`：下落方块和 TNT 都用默认重力。
const GRAVITY: f64 = 0.04;

/// `getAirDrag()`：`0.98F`，这里保存的是那个 float 变成 double 之后的值。
pub(crate) fn air_drag() -> f64 {
    f64::from(0.98_f32)
}

/// `Mth.equal`：两个 double 差得足够小就算相等。官方写的是 `1.0E-5F`。
fn nearly_equal(first: f64, second: f64) -> bool {
    (second - first).abs() < f64::from(1.0e-5_f32)
}

/// Java 的 `!(left > right)`。NaN 的比较是假，取反之后是真，不能写成 `<=`。
///
/// Java's `!(left > right)`. A NaN comparison is false, so the negation is true; `<=` is not.
fn java_not_gt<T: PartialOrd>(left: T, right: T) -> bool {
    left.partial_cmp(&right) != Some(Ordering::Greater)
}

/// Java 的 `!(left <= right)`。NaN 时为真。
///
/// Java's `!(left <= right)`. True when either side is NaN.
fn java_not_le<T: PartialOrd>(left: T, right: T) -> bool {
    !matches!(
        left.partial_cmp(&right),
        Some(Ordering::Less | Ordering::Equal)
    )
}

/// `AxisCycle.FORWARD.cycle(axis)`：X 转到 Y，Y 转到 Z，Z 转到 X。
fn forward(axis: Axis) -> Axis {
    match axis {
        Axis::X => Axis::Y,
        Axis::Y => Axis::Z,
        Axis::Z => Axis::X,
    }
}

/// `AxisCycle.BACKWARD.cycle(axis)`：X 转到 Z，Y 转到 X，Z 转到 Y。
fn backward(axis: Axis) -> Axis {
    match axis {
        Axis::X => Axis::Z,
        Axis::Y => Axis::X,
        Axis::Z => Axis::Y,
    }
}

/// `Direction.Axis.choose(x, y, z)`。
fn pick(axis: Axis, values: (i32, i32, i32)) -> i32 {
    match axis {
        Axis::X => values.0,
        Axis::Y => values.1,
        Axis::Z => values.2,
    }
}

impl Entity {
    /// `makeBoundingBox(position)`：实体在某个位置的碰撞盒。
    pub fn make_bounding_box_at(&self, position: Vec3) -> Aabb {
        let (width, height) = self.kind.dimensions();
        let half = f64::from(width / 2.0);
        let height = f64::from(height);
        Aabb::new(
            position.x - half,
            position.y,
            position.z - half,
            position.x + half,
            position.y + height,
            position.z + half,
        )
    }

    /// `CollisionContext.of(entity)`。
    pub(crate) fn collision_context(&self) -> CollisionContext {
        CollisionContext::Entity(EntityContext {
            fall_distance: self.fall_distance,
            bottom: self.position.y,
            falling_block: self.kind.is_falling_block(),
            descending: false,
            placement: false,
            powder_snow_walkable: false,
        })
    }

    /// `applyGravity`：每 tick 往速度里加一份重力。这两个实体都没有 `noGravity`。
    pub(crate) fn apply_gravity(&mut self) {
        self.delta = self.delta.add(Vec3::new(0.0, -GRAVITY, 0.0));
    }

    /// `setRequiresPrecisePosition`。
    pub(crate) fn set_requires_precise_position(&mut self, requires: bool) {
        self.requires_precise_position = requires;
    }

    /// `Entity.move`：先按轴解算碰撞，再落地判定、摔落伤害、反弹，最后按方块速度系数收尾。
    pub(crate) fn move_entity(&mut self, world: &dyn BlockView, movement: Vec3) {
        let mut delta = movement;
        if self.stuck_speed_multiplier.length_sqr() > 1.0e-7 {
            delta = delta.multiply(
                self.stuck_speed_multiplier.x,
                self.stuck_speed_multiplier.y,
                self.stuck_speed_multiplier.z,
            );
            self.stuck_speed_multiplier = Vec3::default();
            self.delta = Vec3::default();
        }
        // `maybeBackOffFromEdge` 只有生物会改位移，这两个实体原样返回。
        let movement = self.collide(world, delta);
        let movement_length = movement.length_sqr();
        if movement_length > 1.0e-7 || delta.length_sqr() - movement_length < 1.0e-7 {
            if self.fall_distance != 0.0 && movement_length >= 1.0 {
                let check_distance = movement.length().min(8.0);
                let check_to = self
                    .position
                    .add(movement.normalize().scale(check_distance));
                if world.clip_fall_damage_resetting(self.position, check_to) {
                    self.reset_fall_distance();
                }
            }
            let from = self.position;
            let to = from.add(movement);
            self.movement_this_tick.push(Movement {
                from,
                to,
                original: Some(delta),
            });
            self.position = to;
        }
        let x_collision = !nearly_equal(delta.x, movement.x);
        let z_collision = !nearly_equal(delta.z, movement.z);
        self.horizontal_collision = x_collision || z_collision;
        let moved_vertically = delta.y.abs() > 0.0;
        // 服务端的 `isLocalInstanceAuthoritative()` 恒为 true，所以这里没有那个条件。
        self.vertical_collision = delta.y != movement.y;
        self.vertical_collision_below = self.vertical_collision && delta.y < 0.0;
        let on_ground = self.vertical_collision_below;
        let horizontal_collision = self.horizontal_collision;
        self.set_on_ground_with_movement(world, on_ground, horizontal_collision, movement);
        // `minorHorizontalCollision` 只影响脚步声，不在范围里。
        let effect_pos = self.on_pos(world, 0.2);
        let effect_state = world.block_state(effect_pos);
        self.check_fall_damage(movement.y);
        if self.removed {
            return;
        }
        if moved_vertically && self.vertical_collision || self.horizontal_collision {
            self.restitute_movement_after_collisions(
                effect_state,
                x_collision,
                z_collision,
                movement,
            );
        }
        // 移动的音效和 `gameEvent` 不在范围里。
        let factor = f64::from(self.block_speed_factor(world));
        self.delta = self.delta.multiply(factor, 1.0, factor);
    }

    /// `Entity.collide`：把位移按轴拆开，撞到方块就停在方块前面。
    ///
    /// 这两个实体的 `maxUpStep` 是 0，所以不上台阶。玩家走 [`sweep_movement`]。
    ///
    /// `Entity.collide`. These entities have `maxUpStep` 0, so they do not step up.
    /// Players use [`sweep_movement`].
    fn collide(&self, world: &dyn BlockView, movement: Vec3) -> Vec3 {
        sweep_movement(
            movement,
            self.bounding_box(),
            0.0,
            self.on_ground,
            self.collision_context(),
            world::CollisionBlocks {
                block_at: |pos| world.block_state(pos),
                piston_at: |pos| world.moving_piston_shape(pos),
                shulker_at: |pos| world.shulker_box_shape(pos),
            },
            &world.world_border(),
        )
    }

    /// `setOnGroundWithMovement`。
    fn set_on_ground_with_movement(
        &mut self,
        world: &dyn BlockView,
        on_ground: bool,
        horizontal_collision: bool,
        movement: Vec3,
    ) {
        self.on_ground = on_ground;
        self.horizontal_collision = horizontal_collision;
        self.check_supporting_block(world, on_ground, Some(movement));
    }

    /// `Entity.checkSupportingBlock`：脚下这一小片里哪个方块撑着实体。
    ///
    /// 站着不动时用整块实体盒子，被推着走时把这片区域往回挪，看挪之前的位置。
    fn check_supporting_block(
        &mut self,
        world: &dyn BlockView,
        on_ground: bool,
        movement: Option<Vec3>,
    ) {
        if on_ground {
            let bounds = self.bounding_box();
            let test_area = Aabb::new(
                bounds.min_x,
                bounds.min_y - 1.0e-6,
                bounds.min_z,
                bounds.max_x,
                bounds.min_y,
                bounds.max_z,
            );
            let context = self.collision_context();
            let mut supporting = world.find_supporting_block(self.position, test_area, context);
            if supporting.is_some() || self.on_ground_no_blocks {
                self.main_supporting_block = supporting;
            } else if let Some(movement) = movement {
                let moved = test_area.moved(-movement.x, 0.0, -movement.z);
                supporting = world.find_supporting_block(self.position, moved, context);
                self.main_supporting_block = supporting;
            }
            self.on_ground_no_blocks = supporting.is_none();
        } else {
            self.on_ground_no_blocks = false;
            self.main_supporting_block = None;
        }
    }

    /// `getOnPos(offset)`：脚下往下找 `offset` 格落在哪一块。
    ///
    /// 有撑着的方块时优先用它；篱笆、墙、栅栏门在低的偏移下也算自己的位置。
    pub(crate) fn on_pos(&self, world: &dyn BlockView, offset: f32) -> BlockPos {
        if let Some(main) = self.main_supporting_block {
            if java_not_gt(offset, 1.0e-5_f32) {
                return main;
            }
            let below = world.block_state(main);
            let drops_down = (java_not_le(offset, 0.5_f32) || !below.is_tagged(block_tags::FENCES))
                && !below.is_tagged(block_tags::WALLS)
                && !below.is_a(BlockAncestor::FenceGateBlock);
            return if drops_down {
                BlockPos::new(
                    main.x,
                    mth::floor(self.position.y - f64::from(offset)),
                    main.z,
                )
            } else {
                main
            };
        }
        BlockPos::new(
            mth::floor(self.position.x),
            mth::floor(self.position.y - f64::from(offset)),
            mth::floor(self.position.z),
        )
    }

    /// `getBlockSpeedFactor`：当前方块的速度系数，是 1 时看脚下那一格。
    fn block_speed_factor(&self, world: &dyn BlockView) -> f32 {
        let state = world.block_state(self.block_position());
        let here = state.block().speed_factor;
        if !state.is(blocks::WATER) && !state.is(blocks::BUBBLE_COLUMN) && here == 1.0 {
            world
                .block_state(self.on_pos(world, 0.500001))
                .block()
                .speed_factor
        } else {
            here
        }
    }

    /// `checkFallDamage`：往下走了多少就记多少，落地时按脚下的方块算伤害。
    ///
    /// 官方把脚下的状态交给 `Block.fallOn`，覆写过的只有滴水石和硫磺刺那几个；
    /// 它们只影响摔落距离和伤害倍率，这两个实体都不吃倍率，所以这里直接用默认实现。
    fn check_fall_damage(&mut self, ya: f64) {
        if !self.is_in_water() && ya < 0.0 {
            // 官方写的是 `fallDistance -= (float)ya`：先截成 `float` 再减进 `double`。
            self.fall_distance -= f64::from(ya as f32);
        }
        if self.on_ground && self.fall_distance > 0.0 {
            self.cause_fall_damage(self.fall_distance);
            self.reset_fall_distance();
        }
    }

    /// `resetFallDistance`。
    pub(crate) fn reset_fall_distance(&mut self) {
        self.fall_distance = 0.0;
    }

    /// `restituteMovementAfterCollisions`：撞上弹力方块时反弹。
    ///
    /// `isSuppressingBounce()` 和 `getEntityBounciness()` 对这两个实体恒为 false 和 0，
    /// 只有脚下方块的 `getBounceRestitution()` 会起作用。
    fn restitute_movement_after_collisions(
        &mut self,
        effect_state: BlockState,
        x_collision: bool,
        z_collision: bool,
        movement: Vec3,
    ) {
        let mut restitution = 0.0_f64;
        let current = self.delta;
        let mut after = current;
        if x_collision {
            after = with_axis(after, Axis::X, -current.x * restitution);
        }
        if z_collision {
            after = with_axis(after, Axis::Z, -current.z * restitution);
        }
        if self.vertical_collision {
            if self.vertical_collision_below {
                restitution = if java_not_le(-current.y, GRAVITY)
                    && !effect_state.is_tagged(block_tags::SUPPRESSES_BOUNCE)
                {
                    // `getBlockBounciness`：非生物只拿方块的八成。
                    let bounciness = effect_state.block().bounce_restitution * 0.8_f32;
                    restitution.max(f64::from(bounciness))
                } else {
                    0.0
                };
            }
            let (gravity_compensation, effective_drag) = if restitution > 0.0 {
                let portion = movement.y / current.y;
                (portion * GRAVITY, mth::lerp(portion, 1.0, air_drag()))
            } else {
                (0.0, 1.0)
            };
            after = with_axis(
                after,
                Axis::Y,
                (gravity_compensation - current.y) * effective_drag * restitution,
            );
        }
        // 反弹成功后官方还会放音效、发 `gameEvent` 和同步一次位置；前两样不在范围里，
        // 位置同步由常规同步兜住。
        self.delta = after;
    }

    /// `makeStuckInBlock`：蜘蛛网和细雪把接下来这个 tick 的位移压扁。
    fn make_stuck_in_block(&mut self, multiplier: Vec3) {
        self.reset_fall_distance();
        self.stuck_speed_multiplier = multiplier;
    }

    /// `applyEffectsFromBlocks()`：把这一 tick 的移动整理成段，再看穿过了哪些方块。
    pub(crate) fn apply_effects_from_blocks(&mut self, world: &dyn BlockView) {
        if self.removed {
            return;
        }
        self.final_movements.clear();
        self.final_movements.append(&mut self.movement_this_tick);
        if self.final_movements.is_empty() {
            self.final_movements.push(Movement {
                from: self.old_position,
                to: self.position,
                original: None,
            });
        } else if let Some(last) = self.final_movements.last() {
            // 官方写的是 `9.9999994E-11F`，那个 float 变成 double 之后才是这个数。
            if last.to.distance_to_sqr(self.position) > f64::from(9.9999994e-11_f32) {
                let from = last.to;
                let to = self.position;
                self.final_movements.push(Movement {
                    from,
                    to,
                    original: None,
                });
            }
        }
        // `stepOn` 是台阶上方的方块给实体施加的效果，这两个实体用不到。
        let movements = std::mem::take(&mut self.final_movements);
        self.check_inside_blocks(world, &movements);
        self.final_movements = movements;
    }

    /// `checkInsideBlocks`：按轴把每段移动拆开，逐个方块看实体有没有穿过去。
    fn check_inside_blocks(&mut self, world: &dyn BlockView, movements: &[Movement]) {
        if self.removed {
            return;
        }
        let mut visited: Vec<BlockPos> = Vec::new();
        for movement in movements {
            let mut pos = movement.from;
            let delta = movement.to.subtract(movement.from);
            let mut max_iterations = 16;
            match movement.original {
                Some(original) if delta.length_sqr() > 0.0 => {
                    for axis in Direction::axis_step_order(original) {
                        let axis_move = axis_component(delta, axis);
                        if axis_move != 0.0 {
                            let to = offset_along(pos, axis, axis_move);
                            max_iterations -= self.check_inside_between(
                                world,
                                pos,
                                to,
                                &mut visited,
                                max_iterations,
                            );
                            pos = to;
                        }
                    }
                }
                _ => {
                    max_iterations -= self.check_inside_between(
                        world,
                        movement.from,
                        movement.to,
                        &mut visited,
                        16,
                    );
                }
            }
            if max_iterations <= 0 {
                self.check_inside_between(world, movement.to, movement.to, &mut visited, 1);
            }
        }
    }

    /// `checkInsideBlocks(from, to, ...)`：一段直线移动，返回访问道的最后一个迭代号。
    fn check_inside_between(
        &mut self,
        world: &dyn BlockView,
        from: Vec3,
        to: Vec3,
        visited: &mut Vec<BlockPos>,
        max_iterations: i32,
    ) -> i32 {
        let deflated = self
            .make_bounding_box_at(to)
            .deflate(1.0e-5, 1.0e-5, 1.0e-5);
        let mut iterations = 0;
        let mut visit = |pos: BlockPos, iteration: i32| -> bool {
            if self.removed || iteration >= max_iterations {
                return false;
            }
            iterations = iteration;
            let state = world.block_state(pos);
            if state.is_air() {
                return true;
            }
            let inside_boxes = shape::entity_inside_boxes(state, pos, self.entity_context());
            let inside_block = shape::is_unit_cube(&inside_boxes)
                || self.collided_with_shape_moving_from(from, to, &world_boxes(&inside_boxes, pos));
            if inside_block && !visited.contains(&pos) {
                visited.push(pos);
                self.block_inside(state);
            }
            true
        };
        for_each_block_intersected_between(from, to, deflated, &mut visit);
        iterations
    }

    /// `entityInside`：只有蜘蛛网和细雪对这两个实体有影响。
    ///
    /// 细雪在实体水平移动时还会消耗一个世界随机来生成粒子，那个不在范围里。
    fn block_inside(&mut self, state: BlockState) {
        if state.is_a(BlockAncestor::WebBlock) {
            self.make_stuck_in_block(Vec3::new(0.25, f64::from(0.05_f32), 0.25));
        } else if state.is_a(BlockAncestor::PowderSnowBlock) {
            self.make_stuck_in_block(Vec3::new(f64::from(0.9_f32), 1.5, f64::from(0.9_f32)));
        }
    }

    /// `CollisionContext.of(entity)`，给 `checkInsideBlocks` 用。
    fn entity_context(&self) -> EntityContext {
        EntityContext {
            fall_distance: self.fall_distance,
            bottom: self.position.y,
            falling_block: self.kind.is_falling_block(),
            descending: false,
            placement: false,
            powder_snow_walkable: false,
        }
    }

    /// `collidedWithShapeMovingFrom`：从 `from` 移到 `to` 时实体盒子有没有碰到这些形状。
    fn collided_with_shape_moving_from(&self, from: Vec3, to: Vec3, aabbs: &[Aabb]) -> bool {
        let bounds = self.make_bounding_box_at(from);
        bounds.collided_along_vector(to.subtract(from), aabbs)
    }
}

/// 一段移动，给 `checkInsideBlocks` 用。
///
/// One movement segment for `checkInsideBlocks`.
#[derive(Debug, Clone, Copy)]
pub struct TravelSegment {
    pub from: Vec3,
    pub to: Vec3,
    /// `Entity.Movement.original`。没有时整段走直线。
    ///
    /// `Entity.Movement.original`. Absent means the segment is one straight line.
    pub original: Option<Vec3>,
}

/// `Entity.collide`，带 `maxUpStep`。`max_up_step` 为 0 时和下落方块用的那条路相同。
///
/// 没有实体碰撞，玩家之间互不阻挡。世界边界按 `collectCollidersIgnoringWorldBorder`
/// 的条件加进碰撞体：`source` 取碰撞盒的水平中心。
///
/// `Entity.collide` with `maxUpStep`. A zero step height matches the falling-block path.
///
/// There are no entity collisions and players do not block each other. The world border
/// joins the colliders under `collectCollidersIgnoringWorldBorder`'s condition; the
/// source is the horizontal centre of the bounding box.
pub fn sweep_movement(
    movement: Vec3,
    bounds: Aabb,
    max_up_step: f32,
    on_ground: bool,
    context: CollisionContext,
    mut blocks: world::CollisionBlocks<
        impl FnMut(BlockPos) -> BlockState,
        impl FnMut(BlockPos) -> Option<Vec<Aabb>>,
        impl FnMut(BlockPos) -> Option<Aabb>,
    >,
    border: &WorldBorder,
) -> Vec3 {
    let (source_x, source_z) = centre(bounds);
    let movement_step = collide_bounding_box(
        movement,
        bounds,
        context,
        &mut blocks,
        border,
        source_x,
        source_z,
    );
    let x_collision = movement.x != movement_step.x;
    let y_collision = movement.y != movement_step.y;
    let z_collision = movement.z != movement_step.z;
    let landed = y_collision && movement.y < 0.0;
    if max_up_step > 0.0 && (landed || on_ground) && (x_collision || z_collision) {
        let grounded = if landed {
            bounds.moved(0.0, movement_step.y, 0.0)
        } else {
            bounds
        };
        let mut step_box =
            grounded.expanded_towards(movement.x, f64::from(max_up_step), movement.z);
        if !landed {
            step_box = step_box.expanded_towards(0.0, -f64::from(1.0e-5_f32), 0.0);
        }
        let mut colliders = flatten(&world::collect_block_collisions(
            step_box,
            context,
            &mut blocks,
        ));
        colliders.extend(border.collision_colliders(source_x, source_z, step_box));
        let skip = movement_step.y as f32;
        for height in step_heights(grounded.min_y, &colliders, max_up_step, skip) {
            let stepped = collide_with_shapes(
                Vec3::new(movement.x, f64::from(height), movement.z),
                grounded,
                &colliders,
            );
            if horizontal_distance_sqr(stepped) > horizontal_distance_sqr(movement_step) {
                let down = bounds.min_y - grounded.min_y;
                return Vec3::new(stepped.x, stepped.y - down, stepped.z);
            }
        }
    }
    movement_step
}

/// 官方 `Entity.collideWithShapes` 只带世界边界形状时的结果。
/// `world-border-oracle` 用它和 `Shapes.collide` 对拍四堵墙的表示；游戏逻辑走
/// [`sweep_movement`]，那里还带上方块碰撞。
///
/// Vanilla `Entity.collideWithShapes` with only the world-border shape.
/// `world-border-oracle` compares it with `Shapes.collide` to validate the four-wall
/// representation; gameplay uses [`sweep_movement`], which also adds block collisions.
pub fn collide_border_only(movement: Vec3, bounds: Aabb, border: &WorldBorder) -> Vec3 {
    let boxes = border.collision_shape();
    collide_with_shapes(movement, bounds, &boxes)
}

/// `CollisionGetter.noBlockCollision`。不算实体和世界边界。
///
/// `CollisionGetter.noBlockCollision`. Entities and the world border are not included.
pub fn no_block_collision(
    bounds: Aabb,
    context: CollisionContext,
    blocks: world::CollisionBlocks<
        impl FnMut(BlockPos) -> BlockState,
        impl FnMut(BlockPos) -> Option<Vec<Aabb>>,
        impl FnMut(BlockPos) -> Option<Aabb>,
    >,
) -> bool {
    world::collect_block_collisions(bounds, context, blocks).is_empty()
}

/// `ServerGamePacketListenerImpl.isEntityCollidingWithAnythingNew` 的方块部分。
///
/// 预移动上下文的 `placement` 必须是 true。实体碰撞没有接上。
///
/// The block half of `isEntityCollidingWithAnythingNew`.
///
/// The pre-move context must have `placement` set. Entity collisions are not included.
pub fn overlaps_new_blocks(
    old_box: Aabb,
    current_box: Aabb,
    current: Vec3,
    target: Vec3,
    pre_move: CollisionContext,
    blocks: world::CollisionBlocks<
        impl FnMut(BlockPos) -> BlockState,
        impl FnMut(BlockPos) -> Option<Vec<Aabb>>,
        impl FnMut(BlockPos) -> Option<Aabb>,
    >,
) -> bool {
    let new_box = current_box.moved(
        target.x - current.x,
        target.y - current.y,
        target.z - current.z,
    );
    let shrink = f64::from(1.0e-5_f32);
    let new_deflated = new_box.deflate(shrink, shrink, shrink);
    let old_deflated = old_box.deflate(shrink, shrink, shrink);
    for (_, boxes) in world::collect_block_collisions(new_deflated, pre_move, blocks) {
        if !shape::join_is_not_empty(&boxes, &[old_deflated], BooleanOp::And) {
            return true;
        }
    }
    false
}

/// `ServerGamePacketListenerImpl.noBlocksAround`。
///
/// `ServerGamePacketListenerImpl.noBlocksAround`.
pub fn enclosed_in_air(bounds: Aabb, mut block_at: impl FnMut(BlockPos) -> BlockState) -> bool {
    let area = bounds
        .inflate(0.0625, 0.0625, 0.0625)
        .expanded_towards(0.0, -0.55, 0.0);
    let from = BlockPos::containing(Vec3::new(area.min_x, area.min_y, area.min_z));
    let to = BlockPos::containing(Vec3::new(area.max_x, area.max_y, area.max_z));
    let mut y = from.y;
    while y <= to.y {
        let mut z = from.z;
        while z <= to.z {
            let mut x = from.x;
            while x <= to.x {
                if !block_at(BlockPos::new(x, y, z)).is_air() {
                    return false;
                }
                x += 1;
            }
            z += 1;
        }
        y += 1;
    }
    true
}

/// `checkInsideBlocks` 里蜘蛛网和细雪的 `makeStuckInBlock`。最后一个碰到的乘数留下来。
///
/// 压力板、绊线和甜浆果丛的 `entityInside` 没有接。碰到蜘蛛网或细雪时摔落距离要清掉。
///
/// `makeStuckInBlock` for cobwebs and powder snow inside `checkInsideBlocks`. The last multiplier wins.
///
/// `entityInside` for pressure plates, tripwires, and sweet berry bushes is not wired.
/// Touching a cobweb or powder snow clears the fall distance.
pub fn stuck_speed(
    segments: &[TravelSegment],
    mut box_at: impl FnMut(Vec3) -> Aabb,
    context: EntityContext,
    mut block_at: impl FnMut(BlockPos) -> BlockState,
) -> Option<Vec3> {
    let mut inside = InsideBlocks::default();
    for segment in segments {
        let mut pos = segment.from;
        let delta = segment.to.subtract(segment.from);
        let mut max_iterations = 16;
        match segment.original {
            Some(original) if delta.length_sqr() > 0.0 => {
                for axis in Direction::axis_step_order(original) {
                    let axis_move = axis_component(delta, axis);
                    if axis_move != 0.0 {
                        let to = offset_along(pos, axis, axis_move);
                        max_iterations -= stuck_between(
                            pos,
                            to,
                            &mut box_at,
                            context,
                            &mut block_at,
                            &mut inside,
                            max_iterations,
                        );
                        pos = to;
                    }
                }
            }
            _ => {
                max_iterations -= stuck_between(
                    segment.from,
                    segment.to,
                    &mut box_at,
                    context,
                    &mut block_at,
                    &mut inside,
                    16,
                );
            }
        }
        if max_iterations <= 0 {
            stuck_between(
                segment.to,
                segment.to,
                &mut box_at,
                context,
                &mut block_at,
                &mut inside,
                1,
            );
        }
    }
    inside.multiplier
}

/// 这一段位移里已经数过的方块，以及最后一个卡住乘数。
///
/// Blocks already counted on this move, and the last stuck multiplier.
#[derive(Default)]
struct InsideBlocks {
    visited: Vec<BlockPos>,
    multiplier: Option<Vec3>,
}

fn stuck_between(
    from: Vec3,
    to: Vec3,
    box_at: &mut impl FnMut(Vec3) -> Aabb,
    context: EntityContext,
    block_at: &mut impl FnMut(BlockPos) -> BlockState,
    inside: &mut InsideBlocks,
    max_iterations: i32,
) -> i32 {
    let deflated = box_at(to).deflate(1.0e-5, 1.0e-5, 1.0e-5);
    let mut iterations = 0;
    let mut visit = |pos: BlockPos, iteration: i32| -> bool {
        if iteration >= max_iterations {
            return false;
        }
        iterations = iteration;
        let state = block_at(pos);
        if state.is_air() {
            return true;
        }
        let inside_boxes = shape::entity_inside_boxes(state, pos, context);
        let bounds = box_at(from);
        let inside_block = shape::is_unit_cube(&inside_boxes)
            || bounds.collided_along_vector(to.subtract(from), &world_boxes(&inside_boxes, pos));
        if inside_block && !inside.visited.contains(&pos) {
            inside.visited.push(pos);
            if let Some(multiplier) = stuck_multiplier(state) {
                inside.multiplier = Some(multiplier);
            }
        }
        true
    };
    for_each_block_intersected_between(from, to, deflated, &mut visit);
    iterations
}

fn stuck_multiplier(state: BlockState) -> Option<Vec3> {
    if state.is_a(BlockAncestor::WebBlock) {
        Some(Vec3::new(0.25, f64::from(0.05_f32), 0.25))
    } else if state.is_a(BlockAncestor::PowderSnowBlock) {
        Some(Vec3::new(f64::from(0.9_f32), 1.5, f64::from(0.9_f32)))
    } else {
        None
    }
}

fn collide_bounding_box(
    movement: Vec3,
    bounds: Aabb,
    context: CollisionContext,
    blocks: &mut world::CollisionBlocks<
        impl FnMut(BlockPos) -> BlockState,
        impl FnMut(BlockPos) -> Option<Vec<Aabb>>,
        impl FnMut(BlockPos) -> Option<Aabb>,
    >,
    border: &WorldBorder,
    source_x: f64,
    source_z: f64,
) -> Vec3 {
    if movement.length_sqr() == 0.0 {
        return movement;
    }
    let target = bounds.expanded_towards(movement.x, movement.y, movement.z);
    let mut boxes = flatten(&world::collect_block_collisions(target, context, blocks));
    boxes.extend(border.collision_colliders(source_x, source_z, target));
    collide_with_shapes(movement, bounds, &boxes)
}

/// 实体碰撞盒的水平中心，也就是 `Entity.getX()` / `getZ()`。
///
/// The horizontal centre of an entity box, i.e. `Entity.getX()` / `getZ()`.
fn centre(bounds: Aabb) -> (f64, f64) {
    (
        (bounds.min_x + bounds.max_x) / 2.0,
        (bounds.min_z + bounds.max_z) / 2.0,
    )
}

fn collide_with_shapes(movement: Vec3, bounds: Aabb, boxes: &[Aabb]) -> Vec3 {
    if boxes.is_empty() {
        return movement;
    }
    let mut resolved = Vec3::default();
    for axis in Direction::axis_step_order(movement) {
        let axis_movement = axis_component(movement, axis);
        if axis_movement == 0.0 {
            continue;
        }
        let moved = bounds.moved(resolved.x, resolved.y, resolved.z);
        let distance = collide_axis(axis, moved, boxes, axis_movement);
        resolved = with_axis(resolved, axis, distance);
    }
    resolved
}

fn flatten(collisions: &[(BlockPos, Vec<Aabb>)]) -> Vec<Aabb> {
    let mut boxes = Vec::new();
    for (_, shapes) in collisions {
        boxes.extend(shapes.iter().copied());
    }
    boxes
}

fn horizontal_distance_sqr(movement: Vec3) -> f64 {
    movement.x * movement.x + movement.z * movement.z
}

/// `Entity.collectCandidateStepUpHeights`。
fn step_heights(ground_min_y: f64, boxes: &[Aabb], max_step: f32, skip: f32) -> Vec<f32> {
    let mut heights = Vec::new();
    for shape in boxes {
        for coord in [shape.min_y, shape.max_y] {
            let relative = (coord - ground_min_y) as f32;
            if relative < 0.0 || relative == skip {
                continue;
            }
            if relative > max_step {
                break;
            }
            if !heights.contains(&relative) {
                heights.push(relative);
            }
        }
    }
    heights.sort_by(f32::total_cmp);
    heights
}

/// `Vec3.pos.relative(axis.getPositive(), distance)`。
fn offset_along(pos: Vec3, axis: Axis, distance: f64) -> Vec3 {
    let (x, y, z) = axis.positive().normal();
    pos.add(Vec3::new(
        f64::from(x) * distance,
        f64::from(y) * distance,
        f64::from(z) * distance,
    ))
}

/// 形状表里的盒子是相对方块的，挪到世界坐标。
fn world_boxes(boxes: &[Aabb], pos: BlockPos) -> Vec<Aabb> {
    shape::moved(boxes, f64::from(pos.x), f64::from(pos.y), f64::from(pos.z))
}

/// `Shapes.collide`：沿一根轴扫掠，返回实际能走的距离。
///
/// 每次换形状前都要再看一眼距离是不是已经归零，官方就是这么写的。
fn collide_axis(axis: Axis, moving: Aabb, boxes: &[Aabb], mut distance: f64) -> f64 {
    for &shape in boxes {
        if distance.abs() < 1.0e-7 {
            return 0.0;
        }
        distance = collide_box(axis, moving, shape, distance);
    }
    distance
}

/// `VoxelShape.collide`：形状只有一个体素时，按轴扫掠就是两根轴看重叠、一根轴算距离。
fn collide_box(axis: Axis, moving: Aabb, shape: Aabb, mut distance: f64) -> f64 {
    if distance.abs() < 1.0e-7 {
        return 0.0;
    }
    let other_b = forward(axis);
    let other_c = backward(axis);
    let max_a = moving.max(axis);
    let min_a = moving.min(axis);
    let a_max = find_index(shape, axis, max_a - 1.0e-7);
    let a_min = find_index(shape, axis, min_a + 1.0e-7);
    let b_min = 0.max(find_index(shape, other_b, moving.min(other_b) + 1.0e-7));
    let b_max = 1.min(find_index(shape, other_b, moving.max(other_b) - 1.0e-7) + 1);
    let c_min = 0.max(find_index(shape, other_c, moving.min(other_c) + 1.0e-7));
    let c_max = 1.min(find_index(shape, other_c, moving.max(other_c) - 1.0e-7) + 1);
    let overlaps = b_min < b_max && c_min < c_max;
    if distance > 0.0 {
        if a_max < 0 && overlaps {
            let new_distance = shape.min(axis) - max_a;
            if new_distance >= -1.0e-7 {
                distance = distance.min(new_distance);
            }
        }
    } else if distance < 0.0 && a_min > 0 && overlaps {
        let new_distance = shape.max(axis) - min_a;
        if new_distance <= 1.0e-7 {
            distance = distance.max(new_distance);
        }
    }
    distance
}

/// `VoxelShape.findIndex`：坐标落在形状的第几个格子里。一个盒子的形状只有一格。
fn find_index(shape: Aabb, axis: Axis, value: f64) -> i32 {
    let index = if value < shape.min(axis) {
        0
    } else if value < shape.max(axis) {
        1
    } else {
        2
    };
    index - 1
}

/// `BlockGetter.forEachBlockIntersectedBetween`：沿实体盒子最远的角扫过所有碰到的方块。
///
/// `visit` 返回 `false` 时提前结束，整个函数也返回 `false`。
fn for_each_block_intersected_between(
    from: Vec3,
    to: Vec3,
    aabb_at_target: Aabb,
    visit: &mut impl FnMut(BlockPos, i32) -> bool,
) -> bool {
    let travel = to.subtract(from);
    let small = f64::from(1.0e-5_f32);
    if travel.length_sqr() < small * small {
        for pos in between_closed(aabb_at_target) {
            if !visit(pos, 0) {
                return false;
            }
        }
        return true;
    }
    let mut visited: Vec<BlockPos> = Vec::new();
    let behind = aabb_at_target.moved(-travel.x, -travel.y, -travel.z);
    for pos in between_corners_in_direction_box(behind, travel) {
        if !visit(pos, 0) {
            return false;
        }
        visited.push(pos);
    }
    let Some(iterations) = add_collisions_along_travel(&mut visited, travel, aabb_at_target, visit)
    else {
        return false;
    };
    for pos in between_corners_in_direction_box(aabb_at_target, travel) {
        if !visited.contains(&pos) {
            visited.push(pos);
            if !visit(pos, iterations + 1) {
                return false;
            }
        }
    }
    true
}

/// `BlockGetter.addCollisionsAlongTravel`：从最远的角沿移动方向走格子，碰到就整块记下。
fn add_collisions_along_travel(
    visited: &mut Vec<BlockPos>,
    delta_move: Vec3,
    aabb_at_target: Aabb,
    visit: &mut impl FnMut(BlockPos, i32) -> bool,
) -> Option<i32> {
    let size_x = aabb_at_target.size(Axis::X);
    let size_y = aabb_at_target.size(Axis::Y);
    let size_z = aabb_at_target.size(Axis::Z);
    let corner = furthest_corner(delta_move);
    let center = aabb_at_target.center();
    let to_corner = Vec3::new(
        center.x + size_x * 0.5 * f64::from(corner.0),
        center.y + size_y * 0.5 * f64::from(corner.1),
        center.z + size_z * 0.5 * f64::from(corner.2),
    );
    let from_corner = to_corner.subtract(delta_move);
    let mut x = mth::floor(from_corner.x);
    let mut y = mth::floor(from_corner.y);
    let mut z = mth::floor(from_corner.z);
    let sign_x = mth::sign(delta_move.x);
    let sign_y = mth::sign(delta_move.y);
    let sign_z = mth::sign(delta_move.z);
    let t_delta_x = if sign_x == 0 {
        f64::MAX
    } else {
        f64::from(sign_x) / delta_move.x
    };
    let t_delta_y = if sign_y == 0 {
        f64::MAX
    } else {
        f64::from(sign_y) / delta_move.y
    };
    let t_delta_z = if sign_z == 0 {
        f64::MAX
    } else {
        f64::from(sign_z) / delta_move.z
    };
    let mut t_x = t_delta_x
        * if sign_x > 0 {
            1.0 - mth::frac(from_corner.x)
        } else {
            mth::frac(from_corner.x)
        };
    let mut t_y = t_delta_y
        * if sign_y > 0 {
            1.0 - mth::frac(from_corner.y)
        } else {
            mth::frac(from_corner.y)
        };
    let mut t_z = t_delta_z
        * if sign_z > 0 {
            1.0 - mth::frac(from_corner.z)
        } else {
            mth::frac(from_corner.z)
        };
    let mut iterations = 0;
    while t_x <= 1.0 || t_y <= 1.0 || t_z <= 1.0 {
        if t_x < t_y {
            if t_x < t_z {
                x += sign_x;
                t_x += t_delta_x;
            } else {
                z += sign_z;
                t_z += t_delta_z;
            }
        } else if t_y < t_z {
            y += sign_y;
            t_y += t_delta_y;
        } else {
            z += sign_z;
            t_z += t_delta_z;
        }
        let min = Vec3::new(x as f64, y as f64, z as f64);
        let max = Vec3::new(x as f64 + 1.0, y as f64 + 1.0, z as f64 + 1.0);
        let Some(hit) = clip_box(from_corner, to_corner, min, max) else {
            continue;
        };
        iterations += 1;
        let edge = f64::from(1.0e-5_f32);
        let hit_x = mth::clamp(hit.x, x as f64 + edge, x as f64 + 1.0 - edge);
        let hit_y = mth::clamp(hit.y, y as f64 + edge, y as f64 + 1.0 - edge);
        let hit_z = mth::clamp(hit.z, z as f64 + edge, z as f64 + 1.0 - edge);
        let opposite = BlockPos::new(
            mth::floor(hit_x - size_x * f64::from(corner.0)),
            mth::floor(hit_y - size_y * f64::from(corner.1)),
            mth::floor(hit_z - size_z * f64::from(corner.2)),
        );
        for pos in between_corners_in_direction(BlockPos::new(x, y, z), opposite, delta_move) {
            if !visited.contains(&pos) {
                visited.push(pos);
                if !visit(pos, iterations) {
                    return None;
                }
            }
        }
    }
    Some(iterations)
}

/// `BlockGetter.getFurthestCorner`：盒子朝移动方向最远的那个角。
fn furthest_corner(direction: Vec3) -> (i32, i32, i32) {
    let x_dot = direction.x.abs();
    let y_dot = direction.y.abs();
    let z_dot = direction.z.abs();
    let x_sign = if direction.x >= 0.0 { 1 } else { -1 };
    let y_sign = if direction.y >= 0.0 { 1 } else { -1 };
    let z_sign = if direction.z >= 0.0 { 1 } else { -1 };
    if x_dot <= y_dot && x_dot <= z_dot {
        (-x_sign, -z_sign, y_sign)
    } else if y_dot <= z_dot {
        (z_sign, -y_sign, -x_sign)
    } else {
        (-y_sign, x_sign, -z_sign)
    }
}

/// `AABB.clip(from, to, min, max)`：线段进盒子的那个点，进不去时是 `None`。
///
/// 官方的 `getDirection` 按 X、Y、Z 的顺序看六个面，取最靠前的交点。
fn clip_box(from: Vec3, to: Vec3, min: Vec3, max: Vec3) -> Option<Vec3> {
    let delta = to.subtract(from);
    let mut scale = 1.0_f64;
    let mut hit = false;
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        let da = axis_component(delta, axis);
        if da > 1.0e-7 {
            hit |= clip_face(&mut scale, from, delta, axis, min, max, true);
        } else if da < -1.0e-7 {
            hit |= clip_face(&mut scale, from, delta, axis, min, max, false);
        }
    }
    if hit {
        Some(from.add(delta.scale(scale)))
    } else {
        None
    }
}

/// `AABB.clipPoint`：射线打到某根轴的一个面上时，参数是不是比之前那个更靠前。
fn clip_face(
    scale: &mut f64,
    from: Vec3,
    delta: Vec3,
    axis: Axis,
    min: Vec3,
    max: Vec3,
    positive: bool,
) -> bool {
    let other_b = forward(axis);
    let other_c = backward(axis);
    let plane = if positive {
        axis_component(min, axis)
    } else {
        axis_component(max, axis)
    };
    let da = axis_component(delta, axis);
    let s = (plane - axis_component(from, axis)) / da;
    let pb = axis_component(from, other_b) + s * axis_component(delta, other_b);
    let pc = axis_component(from, other_c) + s * axis_component(delta, other_c);
    if 0.0 < s
        && s < *scale
        && axis_component(min, other_b) - 1.0e-7 < pb
        && pb < axis_component(max, other_b) + 1.0e-7
        && axis_component(min, other_c) - 1.0e-7 < pc
        && pc < axis_component(max, other_c) + 1.0e-7
    {
        *scale = s;
        true
    } else {
        false
    }
}

/// `BlockPos.betweenClosed(AABB)`。访问顺序只影响同一个方块被访问几次，没有副作用。
fn between_closed(bounds: Aabb) -> Vec<BlockPos> {
    let min_x = mth::floor(bounds.min_x);
    let min_y = mth::floor(bounds.min_y);
    let min_z = mth::floor(bounds.min_z);
    let max_x = mth::floor(bounds.max_x);
    let max_y = mth::floor(bounds.max_y);
    let max_z = mth::floor(bounds.max_z);
    let mut positions = Vec::new();
    for z in min_z..=max_z {
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                positions.push(BlockPos::new(x, y, z));
            }
        }
    }
    positions
}

/// `BlockPos.betweenCornersInDirection(AABB, direction)`。
fn between_corners_in_direction_box(bounds: Aabb, direction: Vec3) -> Vec<BlockPos> {
    let first = BlockPos::new(
        mth::floor(bounds.min_x),
        mth::floor(bounds.min_y),
        mth::floor(bounds.min_z),
    );
    let second = BlockPos::new(
        mth::floor(bounds.max_x),
        mth::floor(bounds.max_y),
        mth::floor(bounds.max_z),
    );
    between_corners_in_direction(first, second, direction)
}

/// `BlockPos.betweenCornersInDirection`：按 `axisStepOrder` 的顺序走一个盒子里的所有方块。
fn between_corners_in_direction(
    first: BlockPos,
    second: BlockPos,
    direction: Vec3,
) -> Vec<BlockPos> {
    let min = BlockPos::new(
        first.x.min(second.x),
        first.y.min(second.y),
        first.z.min(second.z),
    );
    let max = BlockPos::new(
        first.x.max(second.x),
        first.y.max(second.y),
        first.z.max(second.z),
    );
    let diff = (max.x - min.x, max.y - min.y, max.z - min.z);
    let start = BlockPos::new(
        if direction.x >= 0.0 { min.x } else { max.x },
        if direction.y >= 0.0 { min.y } else { max.y },
        if direction.z >= 0.0 { min.z } else { max.z },
    );
    let axes = Direction::axis_step_order(direction);
    let steps = axes.map(|axis| {
        let step = if axis_component(direction, axis) >= 0.0 {
            axis.positive()
        } else {
            axis.negative()
        };
        step.normal()
    });
    let limits = axes.map(|axis| pick(axis, diff));
    let mut positions = Vec::new();
    let (mut first_index, mut second_index, mut third_index) = (0_i32, 0_i32, 0_i32);
    loop {
        positions.push(BlockPos::new(
            start.x
                + steps[0].0 * first_index
                + steps[1].0 * second_index
                + steps[2].0 * third_index,
            start.y
                + steps[0].1 * first_index
                + steps[1].1 * second_index
                + steps[2].1 * third_index,
            start.z
                + steps[0].2 * first_index
                + steps[1].2 * second_index
                + steps[2].2 * third_index,
        ));
        if third_index < limits[2] {
            third_index += 1;
        } else if second_index < limits[1] {
            second_index += 1;
            third_index = 0;
        } else if first_index < limits[0] {
            first_index += 1;
            third_index = 0;
            second_index = 0;
        } else {
            break;
        }
    }
    positions
}

#[cfg(test)]
mod step_tests {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::blocks;

    use super::*;
    use crate::state::BlockState;

    fn player_box(x: f64) -> Aabb {
        Aabb::new(x - 0.3, 0.0, -0.3, x + 0.3, 1.8, 0.3)
    }

    fn context() -> CollisionContext {
        CollisionContext::Entity(EntityContext {
            fall_distance: 0.0,
            bottom: 0.0,
            falling_block: false,
            descending: false,
            placement: false,
            powder_snow_walkable: false,
        })
    }

    fn no_pistons(_pos: BlockPos) -> Option<Vec<Aabb>> {
        None
    }

    fn no_shulkers(_pos: BlockPos) -> Option<Aabb> {
        None
    }

    /// 半砖能迈上去，整块和零台阶高度都不能。
    ///
    /// A slab can be stepped onto. A full block, and a zero step height, cannot.
    #[test]
    fn a_step_climbs_a_slab_and_stops_at_a_full_block() {
        let slab = BlockState(blocks::STONE_SLAB);
        let stone = BlockState(blocks::STONE);
        let air = BlockState(blocks::AIR);
        let movement = Vec3::new(0.5, 0.0, 0.0);
        let bounds = player_box(0.6);
        let at = |state: BlockState| {
            move |pos: BlockPos| {
                if pos == BlockPos::new(1, 0, 0) {
                    state
                } else {
                    air
                }
            }
        };
        let default_border = WorldBorder::default();
        let climbed = sweep_movement(
            movement,
            bounds,
            0.6,
            true,
            context(),
            world::CollisionBlocks {
                block_at: at(slab),
                piston_at: no_pistons,
                shulker_at: no_shulkers,
            },
            &default_border,
        );
        let blocked = sweep_movement(
            movement,
            bounds,
            0.6,
            true,
            context(),
            world::CollisionBlocks {
                block_at: at(stone),
                piston_at: no_pistons,
                shulker_at: no_shulkers,
            },
            &default_border,
        );
        let no_step = sweep_movement(
            movement,
            bounds,
            0.0,
            true,
            context(),
            world::CollisionBlocks {
                block_at: at(slab),
                piston_at: no_pistons,
                shulker_at: no_shulkers,
            },
            &default_border,
        );
        assert!(climbed.y > 0.4, "{climbed:?}");
        assert!(climbed.x > blocked.x, "slab {climbed:?} block {blocked:?}");
        assert_eq!(blocked.y, 0.0);
        assert_eq!(no_step.y, 0.0);
        assert!(no_step.x < climbed.x);
    }
}
