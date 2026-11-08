//! 碰撞形状的查询和运算，对应官方的 `VoxelShape`、`Shapes`、`SupportType`。
//!
//! 形状就是若干 [`Aabb`] 的并集，运算都在坐标网格上逐格判定：把两边的坐标合并，
//! 每一格取中点看它在不在形状里。官方的体素表示和这个等价，`SHAPE_FACES` 和
//! `STATE_FACTS` 里的官方结果在测试里逐项比过。

use std::borrow::Cow;

pub use mc_bamboo_core::Aabb;
use mc_bamboo_core::{Axis, BlockPos, Direction, Vec3, mth};
use mc_bamboo_data::{Block, BlockAncestor, BlockOffset};

use crate::{
    context::BlockHit,
    state::{BlockState, FACING_HOPPER, SupportType},
};

/// 比这更近的两个坐标算同一个。官方合并形状时用的是同一个阈值。
const EPSILON: f64 = 1.0e-7;
/// `VoxelShape.calculateFace` 取切片的位置：离方块的面差一点点，切在形状里面。
const FACE_NEAR: f64 = 1.0e-7;
const FACE_FAR: f64 = 0.9999999;

const FULL_BLOCK: &[Aabb] = &[Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)];
/// `SupportType.CENTER` 要求覆盖的形状：`Block.column(2.0, 0.0, 10.0)`。
const CENTER_SUPPORT: &[Aabb] = &[Aabb::new(
    7.0 / 16.0,
    0.0,
    7.0 / 16.0,
    9.0 / 16.0,
    10.0 / 16.0,
    9.0 / 16.0,
)];
/// `SupportType.RIGID` 要求覆盖的形状：完整方块减去中间 12 格宽的柱子，剩下一圈。
const RIGID_SUPPORT: &[Aabb] = &[
    Aabb::new(0.0, 0.0, 0.0, 2.0 / 16.0, 1.0, 1.0),
    Aabb::new(14.0 / 16.0, 0.0, 0.0, 1.0, 1.0, 1.0),
    Aabb::new(2.0 / 16.0, 0.0, 0.0, 14.0 / 16.0, 1.0, 2.0 / 16.0),
    Aabb::new(2.0 / 16.0, 0.0, 14.0 / 16.0, 14.0 / 16.0, 1.0, 1.0),
];

/// 一个形状，相对方块的最小角。不跟着渲染偏移走的形状直接借用生成的表。
pub type Boxes = Cow<'static, [Aabb]>;

/// `CollisionContext`。只有这两种上下文影响放置，别的上下文都和实体的运动有关。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionContext {
    /// `CollisionContext.empty()`。
    Empty,
    /// `CollisionContext.placementContext(player)`。
    Placement,
}

/// `BooleanOp` 里用得到的几个。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    OnlyFirst,
    OnlySecond,
    NotSame,
    And,
}

impl BooleanOp {
    const fn apply(self, first: bool, second: bool) -> bool {
        match self {
            Self::OnlyFirst => first && !second,
            Self::OnlySecond => second && !first,
            Self::NotSame => first != second,
            Self::And => first && second,
        }
    }
}

/// `BlockState.getOffset`：这个位置上的渲染偏移，没有偏移的方块是零。
pub fn render_offset(block: &Block, pos: BlockPos) -> Vec3 {
    block
        .offset
        .map_or(Vec3::default(), |offset| offset_at(offset, pos))
}

fn offset_at(offset: BlockOffset, pos: BlockPos) -> Vec3 {
    let seed = mth::seed(pos.x, 0, pos.z);
    let limit = f64::from(offset.max_horizontal);
    let step = |bits: i64| f64::from((bits & 15) as f32 / 15.0);
    let horizontal = |bits: i64| mth::clamp((step(bits) - 0.5) * 0.5, -limit, limit);
    let y = if offset.vertical {
        (step(seed >> 4) - 1.0) * f64::from(offset.max_vertical)
    } else {
        0.0
    };
    Vec3::new(horizontal(seed), y, horizontal(seed >> 8))
}

/// 表里存的是偏移之前的形状，这里加上这个位置的偏移。
fn at_position(shape: &'static [Aabb], block: &Block, pos: BlockPos) -> Boxes {
    match block.offset {
        Some(offset) if offset.moves_shape && !shape.is_empty() => {
            let Vec3 { x, y, z } = offset_at(offset, pos);
            Cow::Owned(moved(shape, x, y, z))
        }
        _ => Cow::Borrowed(shape),
    }
}

/// 每个盒子都平移后的形状，`VoxelShape.move`。
pub fn moved(shape: &[Aabb], dx: f64, dy: f64, dz: f64) -> Vec<Aabb> {
    shape
        .iter()
        .map(|&bounds| bounds.moved(dx, dy, dz))
        .collect()
}

/// 表里的碰撞形状，`Behaviour::collision_boxes` 的默认实现。
pub fn collision_boxes(state: BlockState, pos: BlockPos, context: CollisionContext) -> Boxes {
    let shape = match context {
        CollisionContext::Empty => mc_bamboo_data::collision_shape(state.0),
        CollisionContext::Placement => mc_bamboo_data::placement_collision_shape(state.0),
    };
    at_position(shape, state.block(), pos)
}

/// `getShape`，也就是选中框。
pub fn outline_boxes(state: BlockState, pos: BlockPos) -> Boxes {
    at_position(mc_bamboo_data::outline_shape(state.0), state.block(), pos)
}

/// `getBlockSupportShape`。
pub fn support_boxes(state: BlockState, pos: BlockPos) -> Boxes {
    at_position(mc_bamboo_data::support_shape(state.0), state.block(), pos)
}

/// `Block.box`：以十六分之一格为单位的盒子。
const fn sixteenths(
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
) -> Aabb {
    Aabb::new(
        min_x / 16.0,
        min_y / 16.0,
        min_z / 16.0,
        max_x / 16.0,
        max_y / 16.0,
        max_z / 16.0,
    )
}

/// `AbstractCauldronBlock.SHAPE_INSIDE`：`Block.column(12.0, 4.0, 16.0)`。
const CAULDRON_INSIDE: Aabb = sixteenths(2.0, 4.0, 2.0, 14.0, 16.0, 14.0);
/// 漏斗构造函数里的 `inside`：`Block.column(12.0, 11.0, 16.0)`。
const HOPPER_INSIDE: Aabb = sixteenths(2.0, 11.0, 2.0, 14.0, 16.0, 14.0);

/// `getInteractionShape`：射线碰到选中框后，再碰到这个形状更近时报告它的面。
///
/// 只有炼药锅、堆肥桶、漏斗和脚手架覆盖了它，形状是这几个类的常量。
pub fn interaction_boxes(state: BlockState) -> Vec<Aabb> {
    if state.is_a(BlockAncestor::AbstractCauldronBlock) {
        return vec![CAULDRON_INSIDE];
    }
    if state.is_a(BlockAncestor::ComposterBlock) || state.is_a(BlockAncestor::ScaffoldingBlock) {
        return FULL_BLOCK.to_vec();
    }
    if state.is_a(BlockAncestor::HopperBlock) {
        // 朝北的出口上面那段是 `Block.boxZ(4.0, 8.0, 10.0, 0.0, 4.0)`，别的朝向绕方块中心转过去。
        let spout = match state.get(FACING_HOPPER) {
            Direction::North => sixteenths(6.0, 8.0, 0.0, 10.0, 10.0, 4.0),
            Direction::East => sixteenths(12.0, 8.0, 6.0, 16.0, 10.0, 10.0),
            Direction::South => sixteenths(6.0, 8.0, 12.0, 10.0, 10.0, 16.0),
            Direction::West => sixteenths(0.0, 8.0, 6.0, 4.0, 10.0, 10.0),
            Direction::Down | Direction::Up => return vec![HOPPER_INSIDE],
        };
        return vec![HOPPER_INSIDE, spout];
    }
    Vec::new()
}

/// `SupportType.isSupporting`：`shape` 是方块的支撑形状。
pub fn is_supporting(shape: &[Aabb], face: Direction, support: SupportType) -> bool {
    let required = match support {
        SupportType::Full => return is_face_full(shape, face),
        SupportType::Center => CENTER_SUPPORT,
        SupportType::Rigid => RIGID_SUPPORT,
    };
    !join_is_not_empty(&face_shape(shape, face), required, BooleanOp::OnlySecond)
}

/// `Block.isShapeFullBlock`。
pub fn is_full_block(shape: &[Aabb]) -> bool {
    !join_is_not_empty(FULL_BLOCK, shape, BooleanOp::NotSame)
}

/// `Block.isFaceFull`。
pub fn is_face_full(shape: &[Aabb], face: Direction) -> bool {
    is_full_block(&face_shape(shape, face))
}

/// `VoxelShape.getFaceShape`：形状贴着这个面的那一层，沿着面的法线拉成一格厚。
pub fn face_shape(shape: &[Aabb], face: Direction) -> Vec<Aabb> {
    let axis = face.axis();
    let plane = if face.is_positive() {
        FACE_FAR
    } else {
        FACE_NEAR
    };
    shape
        .iter()
        .filter(|bounds| bounds.min(axis) <= plane && plane < bounds.max(axis))
        .map(|&bounds| stretched(bounds, axis))
        .collect()
}

/// `Shapes.mergedFaceOccludes`：`shape` 朝 `direction` 的面和 `occluder` 朝回来的面合起来盖满了整个面。
///
/// 官方比的是“整格减去两个面还剩不剩”，不是“和整格一样”：墙和栅栏的碰撞盒比一格高，伸出去的部分不算。
pub fn merged_face_occludes(shape: &[Aabb], occluder: &[Aabb], direction: Direction) -> bool {
    let mut faces = face_shape(shape, direction);
    faces.extend(face_shape(occluder, direction.opposite()));
    !join_is_not_empty(FULL_BLOCK, &faces, BooleanOp::OnlyFirst)
}

/// 这个盒子在 `axis` 上撑满 0 到 1。
const fn stretched(bounds: Aabb, axis: Axis) -> Aabb {
    let mut result = bounds;
    match axis {
        Axis::X => (result.min_x, result.max_x) = (0.0, 1.0),
        Axis::Y => (result.min_y, result.max_y) = (0.0, 1.0),
        Axis::Z => (result.min_z, result.max_z) = (0.0, 1.0),
    }
    result
}

/// `Shapes.joinIsNotEmpty`：两个形状按 `op` 合并后还有没有东西。
pub fn join_is_not_empty(first: &[Aabb], second: &[Aabb], op: BooleanOp) -> bool {
    if first.is_empty() || second.is_empty() {
        return op.apply(!first.is_empty(), !second.is_empty());
    }
    let xs = merged_coordinates(first, second, Axis::X);
    let ys = merged_coordinates(first, second, Axis::Y);
    let zs = merged_coordinates(first, second, Axis::Z);
    xs.windows(2).any(|x| {
        ys.windows(2).any(|y| {
            zs.windows(2).any(|z| {
                let point = Vec3::new(middle(x), middle(y), middle(z));
                op.apply(contains(first, point), contains(second, point))
            })
        })
    })
}

fn middle(pair: &[f64]) -> f64 {
    (pair[0] + pair[1]) / 2.0
}

fn merged_coordinates(first: &[Aabb], second: &[Aabb], axis: Axis) -> Vec<f64> {
    let mut coordinates: Vec<f64> = first
        .iter()
        .chain(second)
        .flat_map(|bounds| [bounds.min(axis), bounds.max(axis)])
        .collect();
    coordinates.sort_by(f64::total_cmp);
    coordinates.dedup_by(|next, kept| *next - *kept < EPSILON);
    coordinates
}

/// 这个点在形状里。下界算在里面，上界不算，和官方的 `findIndex` 一致。
pub fn contains(shape: &[Aabb], point: Vec3) -> bool {
    shape.iter().any(|bounds| {
        bounds.min_x <= point.x
            && point.x < bounds.max_x
            && bounds.min_y <= point.y
            && point.y < bounds.max_y
            && bounds.min_z <= point.z
            && point.z < bounds.max_z
    })
}

/// `VoxelShape.clip`：从 `from` 到 `to` 的线段碰到 `pos` 这一格里的形状时，返回碰撞点。
pub fn clip(shape: &[Aabb], from: Vec3, to: Vec3, pos: BlockPos) -> Option<BlockHit> {
    if shape.is_empty() {
        return None;
    }
    let delta = to.subtract(from);
    if delta.distance_to_sqr(Vec3::default()) < EPSILON {
        return None;
    }
    let test_point = from.add(delta.scale(0.001));
    if contains(shape, test_point.subtract(pos.corner())) {
        let face = Direction::nearest_to(delta.x, delta.y, delta.z).opposite();
        return Some(BlockHit {
            pos,
            face,
            location: test_point,
            inside: true,
        });
    }
    clip_boxes(shape, from, delta, pos)
}

/// `AABB.clip`：线段穿过的最近的盒子表面。
fn clip_boxes(shape: &[Aabb], from: Vec3, delta: Vec3, pos: BlockPos) -> Option<BlockHit> {
    let mut nearest = Nearest {
        scale: 1.0,
        face: None,
    };
    for bounds in shape {
        nearest.check_box(bounds.at(pos), from, delta);
    }
    let face = nearest.face?;
    Some(BlockHit {
        pos,
        face,
        location: from.add(delta.scale(nearest.scale)),
        inside: false,
    })
}

/// `AABB.getDirection` 的累积状态：目前最近的碰撞点在线段的哪个比例上，碰到了哪个面。
struct Nearest {
    scale: f64,
    face: Option<Direction>,
}

impl Nearest {
    fn check_box(&mut self, bounds: Aabb, from: Vec3, delta: Vec3) {
        let (x, y, z) = (Axis::X, Axis::Y, Axis::Z);
        self.check_axis(bounds, from, delta, x, y, z);
        self.check_axis(bounds, from, delta, y, z, x);
        self.check_axis(bounds, from, delta, z, x, y);
    }

    /// 线段朝 `axis` 的正方向走，只可能穿过这根轴上较小的那个面。
    fn check_axis(&mut self, bounds: Aabb, from: Vec3, delta: Vec3, axis: Axis, b: Axis, c: Axis) {
        let step = component(delta, axis);
        if step > EPSILON {
            self.check_face(
                bounds,
                from,
                delta,
                bounds.min(axis),
                axis.negative(),
                [axis, b, c],
            );
        } else if step < -EPSILON {
            self.check_face(
                bounds,
                from,
                delta,
                bounds.max(axis),
                axis.positive(),
                [axis, b, c],
            );
        }
    }

    /// `AABB.clipPoint`。
    fn check_face(
        &mut self,
        bounds: Aabb,
        from: Vec3,
        delta: Vec3,
        plane: f64,
        face: Direction,
        [a, b, c]: [Axis; 3],
    ) {
        let scale = (plane - component(from, a)) / component(delta, a);
        let point_b = component(from, b) + scale * component(delta, b);
        let point_c = component(from, c) + scale * component(delta, c);
        if 0.0 < scale
            && scale < self.scale
            && bounds.min(b) - EPSILON < point_b
            && point_b < bounds.max(b) + EPSILON
            && bounds.min(c) - EPSILON < point_c
            && point_c < bounds.max(c) + EPSILON
        {
            self.scale = scale;
            self.face = Some(face);
        }
    }
}

const fn component(vector: Vec3, axis: Axis) -> f64 {
    match axis {
        Axis::X => vector.x,
        Axis::Y => vector.y,
        Axis::Z => vector.z,
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, facts, shapes, state_facts};

    use super::*;

    const SUPPORT_TYPES: [SupportType; 3] =
        [SupportType::Full, SupportType::Center, SupportType::Rigid];

    fn sturdy_bits(shape: &[Aabb]) -> u64 {
        let mut bits = 0;
        for (index, support) in SUPPORT_TYPES.into_iter().enumerate() {
            for face in Direction::ALL {
                if is_supporting(shape, face, support) {
                    bits |= 1 << (index as u32 * 6 + face.id() as u32);
                }
            }
        }
        bits
    }

    /// 官方的 `isFaceFull` 和 `getFaceShape` 在全部形状上的结果。
    #[test]
    fn face_queries_match_the_official_table() {
        for (index, shape) in shapes::SHAPES.iter().enumerate() {
            let official = shapes::SHAPE_FACES[index];
            for face in Direction::ALL {
                let id = face.id() as u32;
                assert_eq!(
                    is_face_full(shape, face),
                    official & (1 << id) != 0,
                    "第 {index} 个形状的 {face:?} 面"
                );
                assert_eq!(
                    !face_shape(shape, face).is_empty(),
                    official & (1 << (6 + id)) != 0,
                    "第 {index} 个形状的 {face:?} 切片"
                );
            }
        }
    }

    /// 全部状态的 18 个 `isFaceSturdy` 位都能从支撑形状算出来。
    #[test]
    fn sturdy_facts_match_the_support_shapes() {
        for id in 0..blocks::TOTAL_STATES {
            let state = BlockState(id);
            if state
                .block()
                .offset
                .is_some_and(|offset| offset.moves_shape)
            {
                continue;
            }
            let shape = mc_bamboo_data::support_shape(id);
            assert_eq!(
                sturdy_bits(shape),
                state_facts(id) & facts::STURDY_MASK,
                "{} ({id})",
                state.block().name
            );
        }
    }

    /// 形状跟着偏移走的方块，官方在几个位置上的结果。
    #[test]
    fn offset_shapes_match_the_official_samples() {
        for &(id, x, y, z, bits) in shapes::OFFSET_STURDY_SAMPLES {
            let state = BlockState(id);
            let pos = BlockPos::new(x, y, z);
            assert_eq!(
                sturdy_bits(&support_boxes(state, pos)),
                u64::from(bits),
                "{} ({id}) 在 {pos:?}",
                state.block().name
            );
        }
    }

    #[test]
    fn full_block_is_only_the_exact_cube() {
        assert!(is_full_block(FULL_BLOCK));
        assert!(!is_full_block(&[]));
        assert!(!is_full_block(&[Aabb::new(0.0, 0.0, 0.0, 1.0, 0.5, 1.0)]));
        // 栅栏的碰撞盒比一格高，所以不算完整方块。
        assert!(!is_full_block(&[Aabb::new(0.0, 0.0, 0.0, 1.0, 1.5, 1.0)]));
    }

    #[test]
    fn collision_follows_the_context() {
        let scaffolding = BlockState(blocks::SCAFFOLDING);
        let pos = BlockPos::new(3, 4, 5);
        assert!(!collision_boxes(scaffolding, pos, CollisionContext::Empty).is_empty());
        assert!(collision_boxes(scaffolding, pos, CollisionContext::Placement).is_empty());
    }

    #[test]
    fn offset_shapes_move_with_the_position() {
        let bamboo = BlockState(blocks::BAMBOO);
        let first = collision_boxes(bamboo, BlockPos::new(1, 0, 1), CollisionContext::Empty);
        let second = collision_boxes(bamboo, BlockPos::new(5, 0, -7), CollisionContext::Empty);
        assert_ne!(first, second);

        // 高草只有渲染偏移，形状不动。
        let grass = BlockState(blocks::TALL_GRASS);
        let shape = outline_boxes(grass, BlockPos::new(1, 0, 1));
        assert_eq!(shape, outline_boxes(grass, BlockPos::new(5, 0, -7)));
    }

    #[test]
    fn join_handles_empty_shapes_like_the_game() {
        assert!(join_is_not_empty(FULL_BLOCK, &[], BooleanOp::OnlyFirst));
        assert!(!join_is_not_empty(&[], FULL_BLOCK, BooleanOp::OnlyFirst));
        assert!(join_is_not_empty(&[], FULL_BLOCK, BooleanOp::OnlySecond));
        assert!(!join_is_not_empty(&[], &[], BooleanOp::And));
    }

    #[test]
    fn clip_finds_the_nearest_face() {
        let pos = BlockPos::new(2, 0, 0);
        let from = Vec3::new(0.5, 0.5, 0.5);
        let hit = clip(FULL_BLOCK, from, Vec3::new(5.5, 0.5, 0.5), pos).expect("线段穿过这个方块");
        assert_eq!(hit.face, Direction::West);
        assert_eq!(hit.location, Vec3::new(2.0, 0.5, 0.5));
        assert!(!hit.inside);

        assert_eq!(clip(FULL_BLOCK, from, Vec3::new(0.5, 5.5, 0.5), pos), None);
    }

    #[test]
    fn clip_from_inside_reports_the_start() {
        let pos = BlockPos::new(0, 0, 0);
        let from = Vec3::new(0.5, 0.5, 0.5);
        let hit = clip(FULL_BLOCK, from, Vec3::new(0.5, 3.5, 0.5), pos).expect("起点在方块里");
        assert!(hit.inside);
        assert_eq!(hit.face, Direction::Down);
    }

    #[test]
    fn merged_faces_occlude_when_together_they_cover_the_face() {
        let bottom_slab = [Aabb::new(0.0, 0.0, 0.0, 1.0, 0.5, 1.0)];
        let top_slab = [Aabb::new(0.0, 0.5, 0.0, 1.0, 1.0, 1.0)];
        let west_half = [Aabb::new(0.0, 0.0, 0.0, 0.5, 1.0, 1.0)];
        // 下半台阶的顶面不贴着上面一格。
        assert!(!merged_face_occludes(&bottom_slab, &[], Direction::Up));
        assert!(merged_face_occludes(&bottom_slab, &[], Direction::Down));
        assert!(merged_face_occludes(&[], &top_slab, Direction::Down));
        // 一边盖住西半，另一边盖住东半，合起来就堵死了。
        let east_half = [Aabb::new(0.5, 0.0, 0.0, 1.0, 1.0, 1.0)];
        assert!(!merged_face_occludes(&west_half, &[], Direction::North));
        assert!(merged_face_occludes(
            &west_half,
            &east_half,
            Direction::North
        ));
        assert!(merged_face_occludes(FULL_BLOCK, &[], Direction::East));
        assert!(!merged_face_occludes(&[], &[], Direction::East));
        // 墙伸向东边的那段比一格高，挨着一个西面是整面的方块，照样堵死。
        let wall_arm = [Aabb::new(0.5, 0.0, 5.0 / 16.0, 1.0, 1.5, 11.0 / 16.0)];
        assert!(merged_face_occludes(&wall_arm, FULL_BLOCK, Direction::East));
    }

    #[test]
    fn interaction_shapes_belong_to_four_classes() {
        assert_eq!(
            interaction_boxes(BlockState(blocks::CAULDRON)),
            [CAULDRON_INSIDE]
        );
        assert_eq!(interaction_boxes(BlockState(blocks::COMPOSTER)), FULL_BLOCK);
        assert_eq!(
            interaction_boxes(BlockState(blocks::HOPPER)),
            [HOPPER_INSIDE]
        );
        let east = BlockState(blocks::HOPPER).with(FACING_HOPPER, Direction::East);
        assert_eq!(interaction_boxes(east).len(), 2);
        assert!(interaction_boxes(BlockState(blocks::STONE)).is_empty());
    }

    #[test]
    fn render_offset_is_limited_by_the_block() {
        let bamboo = BlockState(blocks::BAMBOO).block();
        let offset = render_offset(bamboo, BlockPos::new(0, 0, 0));
        assert_eq!(offset, Vec3::new(-0.25, 0.0, -0.25));
        let stone = BlockState(blocks::STONE).block();
        assert_eq!(
            render_offset(stone, BlockPos::new(9, 9, 9)),
            Vec3::default()
        );
    }
}
