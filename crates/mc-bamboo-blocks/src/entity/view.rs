//! 只读方块视图的公共实现：算阶段只需要「读一格方块」这一个能力。
//!
//! 精确模式的 [`World`](crate::World) 和游戏层的只读区块视图共用这套形状、射线、
//! 流体查询，保证两种世界实现在算阶段读到同样的结果。除了方块状态，视图还要带
//! 高度范围和游戏规则这些不写在区块表里的只读事实。

use mc_bamboo_core::{BlockPos, ChunkPos, Direction, LightLayer, Vec3};
use mc_bamboo_data::{BlockAncestor, blocks};

use crate::{
    Aabb, BlockGameRules, BlockHit, WorldBorder,
    shape::{self, CollisionContext},
    state::{BlockState, FluidState, SupportType},
    world::{collect_block_collisions, traverse_blocks},
};

use super::ctx::BlockView;

/// 算阶段要用的只读事实。所有字段按值传入，视图不借用世界。
#[derive(Debug, Clone, Copy)]
pub struct ViewFacts {
    pub min_y: i32,
    pub max_y: i32,
    pub rules: BlockGameRules,
    /// 天空光和方块光的常量值。有逐格光照的视图覆盖 [`BlockSource::brightness`]。
    pub sky_brightness: i32,
    pub block_brightness: i32,
}

impl Default for ViewFacts {
    fn default() -> Self {
        Self {
            min_y: -64,
            max_y: 319,
            rules: BlockGameRules::vanilla(),
            sky_brightness: 15,
            block_brightness: 0,
        }
    }
}

/// 一个只需要 `block_state` 的只读视图。形状、射线、流体、边界都从这里推出。
///
/// `Sync` 由实现者的字段保证：算阶段的线程共享同一个引用。
pub trait BlockSource: Sync {
    fn block_state(&self, pos: BlockPos) -> BlockState;
    fn facts(&self) -> ViewFacts;

    /// 亮度：默认用 [`ViewFacts`] 里的常量。区块表有逐格光照时覆盖它。
    fn brightness(&self, layer: LightLayer, _pos: BlockPos) -> i32 {
        match layer {
            LightLayer::Sky => self.facts().sky_brightness,
            LightLayer::Block => self.facts().block_brightness,
        }
    }

    /// `Level.getBlockEntity(...) instanceof ShulkerBoxEntity` 的盖子状态。
    /// 默认没有方块实体；带方块实体表的视图要覆盖它，`isFaceSturdy` 才准。
    fn shulker_box_open(&self, _pos: BlockPos) -> bool {
        false
    }

    /// `Level.getBlockEntity(...) instanceof PistonMovingBlockEntity` 时的碰撞形状，
    /// 也就是官方 `MovingPistonBlock.getCollisionShape` 的结果。
    ///
    /// `None` 表示这一格没有这样的方块实体：官方这时给的是 `Shapes.empty()`。
    /// 算阶段借不到不是 `Sync` 的方块实体表，带这份表的视图在开头按位置拷出形状。
    /// `moving_piston` 标了 `dynamicShape`，`isFaceSturdy` 每次都要读这份形状，
    /// 不能用状态表里那份恒为空的形状。
    fn moving_piston_shape(&self, _pos: BlockPos) -> Option<&[Aabb]> {
        None
    }

    /// 这个区块里的实体现在 tick 不 tick。默认都 tick。
    ///
    /// 和 [`crate::Level::chunk_ticks_entities`] 必须给出同样的答案：精确驱动的
    /// `World` 转发到 `Level`，并行驱动的 `SourceView` 转发到这里，游戏层以后把
    /// 同一份判断拷进只读视图。
    fn chunk_ticks_entities(&self, _chunk: ChunkPos) -> bool {
        true
    }

    /// 这片格子里有没有漏斗。默认按格读方块表；游戏层用预先拷出的位置集合覆盖它。
    /// 判定和 `World` 完全一致，所以两种视图、两种驱动发出的命令完全相同。
    fn any_hopper_in(&self, min: BlockPos, max: BlockPos) -> bool {
        (min.y..=max.y).any(|y| {
            (min.z..=max.z).any(|z| {
                (min.x..=max.x).any(|x| {
                    self.block_state(BlockPos::new(x, y, z))
                        .is_a(BlockAncestor::HopperBlock)
                })
            })
        })
    }

    /// `Level.getWorldBorder()`。算阶段的只读视图默认给官方默认边界；游戏层覆盖它，
    /// 让三种实体在 shrink 过的边界里也被挡住。
    ///
    /// `Level.getWorldBorder()`. The compute-phase read-only view defaults to the
    /// vanilla default border; the game layer overrides it so the three entity
    /// kinds also collide with a shrunken border.
    fn world_border(&self) -> WorldBorder {
        WorldBorder::default()
    }
}

/// 给任意 [`BlockSource`] 套上 [`BlockView`]。精确模式、并行模式、游戏层的
/// 只读视图共用这一份查询实现。
pub struct SourceView<T: BlockSource>(pub T);

impl<T: BlockSource> SourceView<T> {
    pub const fn new(source: T) -> Self {
        Self(source)
    }

    fn block_collisions_inner(
        &self,
        box_: Aabb,
        context: CollisionContext,
    ) -> Vec<(BlockPos, Vec<Aabb>)> {
        collect_block_collisions(
            box_,
            context,
            |pos| self.0.block_state(pos),
            |pos| {
                self.0
                    .block_state(pos)
                    .is(blocks::MOVING_PISTON)
                    .then(|| self.moving_piston_shape(pos))
                    .flatten()
            },
        )
    }

    fn clip_collider_impl(
        &self,
        from: Vec3,
        to: Vec3,
        context: CollisionContext,
        fluid: ClipFluid,
    ) -> Option<BlockHit> {
        traverse_blocks(from, to, |pos| {
            let state = self.block_state(pos);
            let piston = state
                .is(blocks::MOVING_PISTON)
                .then(|| self.moving_piston_shape(pos))
                .flatten();
            let collider = shape::block_collision_boxes(state, pos, context, piston.as_deref());
            let block_hit = clip_with_interaction_override(&collider, state, from, to, pos);
            let fluid_hit = match fluid {
                ClipFluid::SourceOnly => clip_source(state.fluid(), pos, from, to),
                ClipFluid::None => None,
            };
            let distance = |hit: &Option<BlockHit>| {
                hit.map_or(f64::MAX, |hit| from.distance_to_sqr(hit.location))
            };
            if distance(&block_hit) <= distance(&fluid_hit) {
                block_hit
            } else {
                fluid_hit
            }
        })
    }
}

impl<T: BlockSource> BlockView for SourceView<T> {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        self.0.block_state(pos)
    }

    fn moving_piston_shape(&self, pos: BlockPos) -> Option<Vec<Aabb>> {
        self.0.moving_piston_shape(pos).map(<[Aabb]>::to_vec)
    }

    fn fluid_state(&self, pos: BlockPos) -> FluidState {
        self.block_state(pos).fluid()
    }

    fn block_collisions(
        &self,
        box_: Aabb,
        context: CollisionContext,
    ) -> Vec<(BlockPos, Vec<Aabb>)> {
        self.block_collisions_inner(box_, context)
    }

    fn find_supporting_block(
        &self,
        position: Vec3,
        box_: Aabb,
        context: CollisionContext,
    ) -> Option<BlockPos> {
        let mut main = None;
        let mut main_distance = f64::MAX;
        for (pos, _) in self.block_collisions_inner(box_, context) {
            let center = Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y) + 0.5,
                f64::from(pos.z) + 0.5,
            );
            let distance = position.distance_to_sqr(center);
            let larger = main.is_none_or(|main: BlockPos| {
                (main.y, main.z, main.x).cmp(&(pos.y, pos.z, pos.x)).is_lt()
            });
            if distance < main_distance || distance == main_distance && larger {
                main = Some(pos);
                main_distance = distance;
            }
        }
        main
    }

    fn clip_fall_damage_resetting(&self, from: Vec3, to: Vec3) -> bool {
        const FULL: [Aabb; 1] = [Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)];
        traverse_blocks(from, to, |pos| {
            let state = self.block_state(pos);
            let block_hit =
                if state.is_tagged(mc_bamboo_data::tags::block_tags::FALL_DAMAGE_RESETTING) {
                    shape::clip(&FULL, from, to, pos)
                } else {
                    None
                };
            let fluid = state.fluid();
            let fluid_hit = if fluid.is_water() && !fluid.is_empty() {
                let bounds = Aabb::new(0.0, 0.0, 0.0, 1.0, f64::from(fluid.own_height()), 1.0);
                shape::clip(&[bounds], from, to, pos)
            } else {
                None
            };
            let distance = |hit: Option<BlockHit>| {
                hit.map_or(f64::MAX, |hit| from.distance_to_sqr(hit.location))
            };
            if distance(block_hit) <= distance(fluid_hit) {
                block_hit.map(|_| ())
            } else {
                fluid_hit.map(|_| ())
            }
        })
        .is_some()
    }

    fn clip_collider_source_water(
        &self,
        from: Vec3,
        to: Vec3,
        context: CollisionContext,
    ) -> Option<BlockHit> {
        self.clip_collider_impl(from, to, context, ClipFluid::SourceOnly)
    }

    fn clip_collider(&self, from: Vec3, to: Vec3, context: CollisionContext) -> Option<BlockHit> {
        self.clip_collider_impl(from, to, context, ClipFluid::None)
    }

    fn is_face_sturdy(&self, pos: BlockPos, face: Direction, support: SupportType) -> bool {
        let state = self.block_state(pos);
        if state.is(blocks::MOVING_PISTON) {
            // 官方 `isFaceSturdy` 在 `dynamicShape` 方块上每次问 `getBlockSupportShape`，
            // 也就是方块实体的碰撞形状。快照里没有实体时那一份形状是空的。
            let empty: [Aabb; 0] = [];
            let boxes = self.0.moving_piston_shape(pos).unwrap_or(&empty);
            return shape::is_supporting(boxes, face, support);
        }
        if state.is_a(BlockAncestor::ShulkerBoxBlock) {
            if self.0.shulker_box_open(pos) {
                let boxes = [crate::behaviour::machines::open_support_shape(
                    state.get(crate::state::FACING),
                )];
                return shape::is_supporting(&boxes, face, support);
            }
            return shape::is_supporting(&shape::support_boxes(state, pos), face, support);
        }
        if state
            .block()
            .offset
            .is_some_and(|offset| offset.moves_shape)
        {
            return shape::is_supporting(&shape::support_boxes(state, pos), face, support);
        }
        state.is_face_sturdy(face, support)
    }

    fn is_loaded(&self, pos: BlockPos) -> bool {
        let facts = self.0.facts();
        (facts.min_y..=facts.max_y).contains(&pos.y)
            && (pos.x >> 4).abs() <= 2_097_061
            && (pos.z >> 4).abs() <= 2_097_061
    }

    fn is_in_world_bounds(&self, pos: BlockPos) -> bool {
        let facts = self.0.facts();
        (facts.min_y..=facts.max_y).contains(&pos.y)
            && (-30_000_000..30_000_000).contains(&pos.x)
            && (-30_000_000..30_000_000).contains(&pos.z)
    }

    fn min_y(&self) -> i32 {
        self.0.facts().min_y
    }

    fn max_y(&self) -> i32 {
        self.0.facts().max_y
    }

    fn brightness(&self, layer: LightLayer, pos: BlockPos) -> i32 {
        self.0.brightness(layer, pos)
    }

    fn block_game_rules(&self) -> BlockGameRules {
        self.0.facts().rules
    }

    fn fluid_height(&self, fluid: FluidState, pos: BlockPos) -> f32 {
        if crate::fluid::Fluid::of(fluid)
            .is_some_and(|kind| kind.is_same(self.fluid_state(pos.above())))
        {
            1.0
        } else {
            fluid.own_height()
        }
    }

    fn any_hopper_in(&self, min: BlockPos, max: BlockPos) -> bool {
        self.0.any_hopper_in(min, max)
    }

    fn chunk_ticks_entities(&self, chunk: ChunkPos) -> bool {
        self.0.chunk_ticks_entities(chunk)
    }

    fn world_border(&self) -> WorldBorder {
        self.0.world_border()
    }
}

#[derive(Clone, Copy)]
enum ClipFluid {
    None,
    SourceOnly,
}

fn clip_with_interaction_override(
    outline: &[Aabb],
    state: BlockState,
    from: Vec3,
    to: Vec3,
    pos: BlockPos,
) -> Option<BlockHit> {
    let hit = shape::clip(outline, from, to, pos)?;
    let interaction = shape::clip(&shape::interaction_boxes(state), from, to, pos);
    match interaction {
        Some(closer)
            if from.distance_to_sqr(closer.location) < from.distance_to_sqr(hit.location) =>
        {
            Some(BlockHit {
                face: closer.face,
                ..hit
            })
        }
        _ => Some(hit),
    }
}

fn clip_source(fluid: FluidState, pos: BlockPos, from: Vec3, to: Vec3) -> Option<BlockHit> {
    if !fluid.is_source() {
        return None;
    }
    let bounds = Aabb::new(0.0, 0.0, 0.0, 1.0, f64::from(fluid.own_height()), 1.0);
    shape::clip(&[bounds], from, to, pos)
}

/// 测试用：一块平地，脚下是石头，没有方块实体。
#[cfg(test)]
pub(crate) struct FlatSource {
    pub floor_y: i32,
    pub facts: ViewFacts,
}

#[cfg(test)]
impl BlockSource for FlatSource {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        if pos.y == self.floor_y {
            BlockState(blocks::STONE)
        } else {
            BlockState::AIR
        }
    }

    fn facts(&self) -> ViewFacts {
        self.facts
    }
}
