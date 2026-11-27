//! 悬挂实体：官方 `BlockAttachedEntity` / `HangingEntity` / `Painting` / `ItemFrame` / `GlowItemFrame`。
//!
//! 统一维护朝向、贴附方块、碰撞箱（[`painting_bounding_box`] / [`item_frame_bounding_box`]）、
//! 存活检查（每 100 刻一次，按字节码 `ticksSinceLastCheck++ >= 100` 在第 101 刻触发）、
//! 受伤与爆炸破坏、掉落物以及展示框的放物 / 旋转 / 比较器输出。
//!
//! Hanging entities (`BlockAttachedEntity` / `HangingEntity` / `Painting` /
//! `ItemFrame` / `GlowItemFrame`): facing, attachment block, bounding box
//! calculation, periodic survival checks (`ticksSinceLastCheck++ >= 100`),
//! hurt/explosion handling, drops, and item frame interaction / analog output.

use mc_bamboo_core::{Axis, BlockPos, Direction, Vec3};
use mc_bamboo_data::{
    BlockAncestor, items, painting as painting_data, registries::data_component_type as kinds,
    registries::entity_type, sounds,
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{
        ComponentPatch, ComponentValue, PatchEntry, cursor::Holder, defaults,
        text::component_from_checked_tag,
    },
    item::ItemStack,
};

use crate::{
    BlockState, InteractionResult,
    entity::{
        Entity, EntityKind, EntityPublic, EntityUid, KindPublic,
        ctx::{BlockView, Command, EntityCtx},
        mob::DamageKind,
    },
    shape::{Aabb, CollisionContext},
    world::World,
};

/// `BlockAttachedEntity.CHECK_INTERVAL`。
const CHECK_INTERVAL: i32 = 100;
/// `ItemFrame.NUM_ROTATIONS`。
pub const NUM_ROTATIONS: i32 = 8;
/// `HangingEntity.DEFAULT_DIRECTION`。
pub const DEFAULT_DIRECTION: Direction = Direction::South;
/// `ItemFrame.DEFAULT_DROP_CHANCE`。
pub const DEFAULT_DROP_CHANCE: f32 = 1.0;

/// 全部悬挂实体类型的注册表 ID，用于 `canCoexist` 的空间查询。
///
/// Registry ids of all hanging entity types for `canCoexist` spatial queries.
pub const HANGING_ENTITY_TYPES: [i32; 3] = [
    entity_type::PAINTING,
    entity_type::ITEM_FRAME,
    entity_type::GLOW_ITEM_FRAME,
];

/// 物品展示框实体的注册表 ID（普通与荧光），用于比较器查找。
///
/// Registry ids of item frame types for comparator lookups.
pub const ITEM_FRAME_TYPES: [i32; 2] = [entity_type::ITEM_FRAME, entity_type::GLOW_ITEM_FRAME];

/// 画实体，对应官方 `Painting`。
///
/// Painting entity, vanilla `Painting`.
#[derive(Debug, Clone, PartialEq)]
pub struct Painting {
    /// 实体 UUID，由构造时的实体随机源派生。
    pub uuid: [u8; 16],
    /// `BlockAttachedEntity.pos`：贴附所在的空气格坐标。
    pub pos: BlockPos,
    /// `HangingEntity.DATA_DIRECTION`：始终为水平方向，缺省 `SOUTH`。
    pub direction: Direction,
    /// `Painting.DATA_PAINTING_VARIANT_ID`：`minecraft:painting_variant` 注册表 ID。
    pub variant: i32,
    /// `Entity.DATA_CUSTOM_NAME`：放置时从物品继承，掉落时写回物品。
    pub custom_name: Option<Tag>,
    /// `BlockAttachedEntity.ticksSinceLastCheck`。
    pub ticks_since_last_check: i32,
    /// 下一刻同步时需要立刻发包。
    pub needs_sync: bool,
}

/// 画的公开视图，供碰撞箱、共存判定、追踪同步和存档读写使用。
///
/// Public snapshot of a painting for bounds, coexistence, tracking and saving.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintingPublic {
    pub uuid: [u8; 16],
    pub pos: BlockPos,
    pub direction: Direction,
    pub variant: i32,
    pub custom_name: Option<Tag>,
    pub needs_sync: bool,
}

impl Painting {
    /// 构造默认画（朝南、变体 `0`）。位置由调用方或 [`Entity::painting`] 算好。
    pub fn new(pos: BlockPos, direction: Direction, variant: i32) -> Self {
        let direction = if direction.axis().is_horizontal() {
            direction
        } else {
            DEFAULT_DIRECTION
        };
        Self {
            uuid: [0; 16],
            pos,
            direction,
            variant,
            custom_name: None,
            ticks_since_last_check: 0,
            needs_sync: true,
        }
    }

    pub fn public(&self) -> PaintingPublic {
        PaintingPublic {
            uuid: self.uuid,
            pos: self.pos,
            direction: self.direction,
            variant: self.variant,
            custom_name: self.custom_name.clone(),
            needs_sync: self.needs_sync,
        }
    }

    pub fn bounding_box(&self) -> Aabb {
        painting_bounding_box(self.pos, self.direction, self.variant)
    }
}

impl PaintingPublic {
    pub fn bounding_box(&self) -> Aabb {
        painting_bounding_box(self.pos, self.direction, self.variant)
    }
}

/// 物品展示框 / 荧光物品展示框实体，对应官方 `ItemFrame` / `GlowItemFrame`。
///
/// Item frame / glow item frame entity, vanilla `ItemFrame` / `GlowItemFrame`.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemFrame {
    /// 实体 UUID，由构造时的实体随机源派生。
    pub uuid: [u8; 16],
    /// `BlockAttachedEntity.pos`：展示框占据的方块坐标。
    pub pos: BlockPos,
    /// `HangingEntity.DATA_DIRECTION`：支持全部六个方向。
    pub direction: Direction,
    /// 为真时是 `GlowItemFrame`，仅音效和掉落物物品不同（官方发光在客户端渲染）。
    pub glowing: bool,
    /// `ItemFrame.DATA_ITEM`：框内物品，空堆表示未放物品。
    pub item: ItemStack,
    /// `ItemFrame.DATA_ROTATION`：旋转档位 `0..8`。
    pub rotation: i32,
    /// `ItemFrame.dropChance`：框内物品的掉落概率，缺省 1.0。
    pub drop_chance: f32,
    /// `ItemFrame.fixed`：固定后不可交互、不受常规伤害、不检查支撑。
    pub fixed: bool,
    /// `Entity.isInvisible()`：隐身标志（共享标志第 5 位 `0x20`）。
    pub invisible: bool,
    /// `Entity.DATA_CUSTOM_NAME`：放置时从物品继承，掉落框本体时写回。
    pub custom_name: Option<Tag>,
    /// `BlockAttachedEntity.ticksSinceLastCheck`。
    pub ticks_since_last_check: i32,
    /// `ItemFrame.setDirection` 算出的俯仰角（水平 0，UP -90，DOWN 90）。
    pub x_rot: f32,
    /// 下一刻同步时需要立刻发包。
    pub needs_sync: bool,
}

/// 展示框的公开视图，供碰撞箱、共存判定、比较器读数、追踪同步和存档使用。
///
/// Public snapshot of an item frame for bounds, coexistence, comparators, tracking and saving.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemFramePublic {
    pub uuid: [u8; 16],
    pub pos: BlockPos,
    pub direction: Direction,
    pub glowing: bool,
    pub item: ItemStack,
    pub rotation: i32,
    pub drop_chance: f32,
    pub fixed: bool,
    pub invisible: bool,
    pub custom_name: Option<Tag>,
    pub x_rot: f32,
    pub needs_sync: bool,
}

impl ItemFrame {
    pub fn new(pos: BlockPos, direction: Direction, glowing: bool) -> Self {
        let (_, x_rot) = item_frame_rotation(direction);
        Self {
            uuid: [0; 16],
            pos,
            direction,
            glowing,
            item: ItemStack::empty(),
            rotation: 0,
            drop_chance: DEFAULT_DROP_CHANCE,
            fixed: false,
            invisible: false,
            custom_name: None,
            ticks_since_last_check: 0,
            x_rot,
            needs_sync: true,
        }
    }

    pub const fn entity_type(&self) -> i32 {
        if self.glowing {
            entity_type::GLOW_ITEM_FRAME
        } else {
            entity_type::ITEM_FRAME
        }
    }

    pub fn has_framed_map(&self) -> bool {
        stack_has_map_id(&self.item)
    }

    pub fn bounding_box(&self) -> Aabb {
        item_frame_bounding_box(self.pos, self.direction, self.has_framed_map())
    }

    pub fn pop_box(&self) -> Aabb {
        item_frame_bounding_box(self.pos, self.direction, false)
    }

    /// `ItemFrame.getAnalogOutput`：空框为 0，有物品为 `rotation % 8 + 1`（1..=8）。
    ///
    /// `ItemFrame.getAnalogOutput`: 0 when empty, otherwise `rotation % 8 + 1` (1..=8).
    pub fn analog_output(&self) -> i32 {
        if self.item.is_empty() {
            0
        } else {
            self.rotation.rem_euclid(NUM_ROTATIONS) + 1
        }
    }

    pub fn public(&self) -> ItemFramePublic {
        ItemFramePublic {
            uuid: self.uuid,
            pos: self.pos,
            direction: self.direction,
            glowing: self.glowing,
            item: self.item.clone(),
            rotation: self.rotation,
            drop_chance: self.drop_chance,
            fixed: self.fixed,
            invisible: self.invisible,
            custom_name: self.custom_name.clone(),
            x_rot: self.x_rot,
            needs_sync: self.needs_sync,
        }
    }

    pub fn place_sound(&self) -> u16 {
        item_frame_sound(self.glowing, FrameSound::Place)
    }

    pub fn break_sound(&self) -> u16 {
        item_frame_sound(self.glowing, FrameSound::Break)
    }

    pub fn add_item_sound(&self) -> u16 {
        item_frame_sound(self.glowing, FrameSound::AddItem)
    }

    pub fn remove_item_sound(&self) -> u16 {
        item_frame_sound(self.glowing, FrameSound::RemoveItem)
    }

    pub fn rotate_item_sound(&self) -> u16 {
        item_frame_sound(self.glowing, FrameSound::RotateItem)
    }
}

impl ItemFramePublic {
    pub const fn entity_type(&self) -> i32 {
        if self.glowing {
            entity_type::GLOW_ITEM_FRAME
        } else {
            entity_type::ITEM_FRAME
        }
    }

    pub fn has_framed_map(&self) -> bool {
        stack_has_map_id(&self.item)
    }

    pub fn bounding_box(&self) -> Aabb {
        item_frame_bounding_box(self.pos, self.direction, self.has_framed_map())
    }

    /// `ItemFrame.getAnalogOutput`。
    pub fn analog_output(&self) -> i32 {
        if self.item.is_empty() {
            0
        } else {
            self.rotation.rem_euclid(NUM_ROTATIONS) + 1
        }
    }
}

/// 物品展示框的五种音效。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameSound {
    Place,
    Break,
    AddItem,
    RemoveItem,
    RotateItem,
}

/// 按是否荧光框取官方音效 ID。
pub fn item_frame_sound(glowing: bool, kind: FrameSound) -> u16 {
    let name = match (glowing, kind) {
        (false, FrameSound::Place) => "minecraft:entity.item_frame.place",
        (false, FrameSound::Break) => "minecraft:entity.item_frame.break",
        (false, FrameSound::AddItem) => "minecraft:entity.item_frame.add_item",
        (false, FrameSound::RemoveItem) => "minecraft:entity.item_frame.remove_item",
        (false, FrameSound::RotateItem) => "minecraft:entity.item_frame.rotate_item",
        (true, FrameSound::Place) => "minecraft:entity.glow_item_frame.place",
        (true, FrameSound::Break) => "minecraft:entity.glow_item_frame.break",
        (true, FrameSound::AddItem) => "minecraft:entity.glow_item_frame.add_item",
        (true, FrameSound::RemoveItem) => "minecraft:entity.glow_item_frame.remove_item",
        (true, FrameSound::RotateItem) => "minecraft:entity.glow_item_frame.rotate_item",
    };
    sounds::sound_event(name)
}

/// 物品堆是否带有 `minecraft:map_id` 组件（`ItemFrame.hasFramedMap`）。
///
/// Whether `stack` carries `minecraft:map_id` (`ItemFrame.hasFramedMap`).
pub fn stack_has_map_id(stack: &ItemStack) -> bool {
    if stack.is_empty() {
        return false;
    }
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return false;
    };
    defaults::resolve(stack.item, &patch, kinds::MAP_ID).is_some()
}

/// `AABB.ofSize(center, xSize, ySize, zSize)`。
fn aabb_of_size(center: Vec3, sx: f64, sy: f64, sz: f64) -> Aabb {
    let hx = sx / 2.0;
    let hy = sy / 2.0;
    let hz = sz / 2.0;
    Aabb::new(
        center.x - hx,
        center.y - hy,
        center.z - hz,
        center.x + hx,
        center.y + hy,
        center.z + hz,
    )
}

/// `AABB.getCenter()`。
fn aabb_center(box_: Aabb) -> Vec3 {
    Vec3::new(
        (box_.min_x + box_.max_x) * 0.5,
        (box_.min_y + box_.max_y) * 0.5,
        (box_.min_z + box_.max_z) * 0.5,
    )
}

/// `Painting.offsetForPaintingSize`：偶数宽/高偏 0.5 格，奇数不偏。
const fn painting_size_offset(size: i32) -> f64 {
    if size % 2 == 0 { 0.5 } else { 0.0 }
}

/// `Painting.calculateBoundingBox(pos, direction)`。
pub fn painting_bounding_box(pos: BlockPos, direction: Direction, variant: i32) -> Aabb {
    let (width, height) = painting_data::size(variant).unwrap_or((1, 1));
    let base = Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    )
    .relative(direction, -0.46875);
    let ccw = if direction.axis().is_horizontal() {
        direction.counter_clockwise()
    } else {
        Direction::East
    };
    let center = base
        .relative(ccw, painting_size_offset(width))
        .relative(Direction::Up, painting_size_offset(height));
    let axis = direction.axis();
    let sx = if axis == Axis::X {
        0.0625
    } else {
        f64::from(width)
    };
    let sy = f64::from(height);
    let sz = if axis == Axis::Z {
        0.0625
    } else {
        f64::from(width)
    };
    aabb_of_size(center, sx, sy, sz)
}

/// `ItemFrame.createBoundingBox(pos, direction, hasFramedMap)`。
pub fn item_frame_bounding_box(pos: BlockPos, direction: Direction, has_framed_map: bool) -> Aabb {
    let center = Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    )
    .relative(direction, -0.46875);
    let wh = if has_framed_map { 1.0 } else { 0.75 };
    let axis = direction.axis();
    let sx = if axis == Axis::X { 0.0625 } else { wh };
    let sy = if axis == Axis::Y { 0.0625 } else { wh };
    let sz = if axis == Axis::Z { 0.0625 } else { wh };
    aabb_of_size(center, sx, sy, sz)
}

/// `HangingEntity.setDirection`（画）的角度：`(yRot, xRot)`。
pub const fn painting_rotation(direction: Direction) -> (f32, f32) {
    ((direction.data_2d() * 90) as f32, 0.0)
}

/// `ItemFrame.setDirection` 的角度：`(yRot, xRot)`。
pub const fn item_frame_rotation(direction: Direction) -> (f32, f32) {
    if direction.axis().is_horizontal() {
        ((direction.data_2d() * 90) as f32, 0.0)
    } else {
        let step = if matches!(direction, Direction::Up) {
            1
        } else {
            -1
        };
        (0.0, (-90 * step) as f32)
    }
}

/// `HangingEntity.spawnAtLocation` 的生成位置：沿朝向的 X/Z 偏移 `0.15F`。
///
/// Spawn position of `HangingEntity.spawnAtLocation`: offset by `0.15F` along facing X/Z.
pub fn drop_position(position: Vec3, direction: Direction) -> Vec3 {
    let (nx, _, nz) = direction.normal();
    Vec3::new(
        position.x + f64::from(nx as f32 * 0.15_f32),
        position.y,
        position.z + f64::from(nz as f32 * 0.15_f32),
    )
}

/// `HangingEntity.calculateSupportBox`：沿朝向反向挪半格再内缩 `1.0E-7`。
fn painting_support_box(bb: Aabb, direction: Direction) -> Aabb {
    let (nx, ny, nz) = direction.normal();
    bb.moved(
        f64::from(nx as f32 * -0.5_f32),
        f64::from(ny as f32 * -0.5_f32),
        f64::from(nz as f32 * -0.5_f32),
    )
    .deflate(1.0e-7, 1.0e-7, 1.0e-7)
}

/// `HangingEntity.isSupportingBlock`：`isSolid() || DiodeBlock.isDiode(state)`。
fn is_supporting_block(state: crate::state::BlockState) -> bool {
    state.is_solid() || state.is_a(BlockAncestor::DiodeBlock)
}

/// `HangingEntity.hasLevelCollision`：方块碰撞或越出世界边界。
fn has_level_collision(blocks: &dyn BlockView, pop_box: Aabb, context: CollisionContext) -> bool {
    !blocks.block_collisions(pop_box, context).is_empty()
        || !blocks.world_border().is_within_bounds_aabb(pop_box)
}

/// 判断另一条公开实体记录是否与当前悬挂实体冲突（`HangingEntity.canCoexist` 的谓词）。
///
/// Conflict predicate of `HangingEntity.canCoexist`.
fn conflicts_with(
    self_uid: EntityUid,
    self_type: i32,
    self_dir: Direction,
    allow_different_types: bool,
    pop_box: Aabb,
    other: &EntityPublic,
) -> bool {
    if other.uid == self_uid || other.removed {
        return false;
    }
    let other_dir = match &other.kind {
        KindPublic::Painting(p) => p.direction,
        KindPublic::ItemFrame(f) => f.direction,
        _ => return false,
    };
    if !other.bounding_box().intersects(pop_box) {
        return false;
    }
    let same_type_blocks = !allow_different_types && other.kind.entity_type() == self_type;
    let same_direction = other_dir == self_dir;
    same_type_blocks || same_direction
}

/// 画在只读方块视图和共存判断下的 `survives()`。
fn painting_survives_with(
    blocks: &dyn BlockView,
    painting: &Painting,
    context: CollisionContext,
    can_coexist: impl FnOnce(Aabb) -> bool,
) -> bool {
    let bb = painting.bounding_box();
    if has_level_collision(blocks, bb, context) {
        return false;
    }
    let support = painting_support_box(bb, painting.direction);
    let min = BlockPos::containing(Vec3::new(support.min_x, support.min_y, support.min_z));
    let max = BlockPos::containing(Vec3::new(support.max_x, support.max_y, support.max_z));
    for y in min.y..=max.y {
        for z in min.z..=max.z {
            for x in min.x..=max.x {
                if !is_supporting_block(blocks.block_state(BlockPos::new(x, y, z))) {
                    return false;
                }
            }
        }
    }
    can_coexist(bb)
}

/// 展示框在只读方块视图和共存判断下的 `survives()`。
fn item_frame_survives_with(
    blocks: &dyn BlockView,
    frame: &ItemFrame,
    context: CollisionContext,
    can_coexist: impl FnOnce(Aabb) -> bool,
) -> bool {
    if frame.fixed {
        return true;
    }
    let pop = frame.pop_box();
    if has_level_collision(blocks, pop, context) {
        return false;
    }
    let behind = blocks.block_state(frame.pos.relative(frame.direction.opposite()));
    if !behind.is_solid()
        && !(frame.direction.axis().is_horizontal() && behind.is_a(BlockAncestor::DiodeBlock))
    {
        return false;
    }
    can_coexist(pop)
}

/// 画在 `World` 里的 `survives()`（用于放置与测试）。
pub fn painting_survives(world: &World, entity: &Entity, painting: &Painting) -> bool {
    let context = entity.collision_context();
    painting_survives_with(world, painting, context, |pop_box| {
        !world.live_entities().iter().any(|other| {
            conflicts_with(
                entity.id,
                entity_type::PAINTING,
                painting.direction,
                false,
                pop_box,
                &other.clone().split().public,
            )
        })
    })
}

/// 展示框在 `World` 里的 `survives()`（用于放置与测试）。
pub fn item_frame_survives(world: &World, entity: &Entity, frame: &ItemFrame) -> bool {
    let context = entity.collision_context();
    let self_type = frame.entity_type();
    item_frame_survives_with(world, frame, context, |pop_box| {
        !world.live_entities().iter().any(|other| {
            conflicts_with(
                entity.id,
                self_type,
                frame.direction,
                true,
                pop_box,
                &other.clone().split().public,
            )
        })
    })
}

/// 在实体随机源已经播好之后，同步画的位置与朝向。
pub fn recalculate_painting(entity: &mut Entity) {
    let EntityKind::Painting(p) = &entity.kind else {
        return;
    };
    let bb = p.bounding_box();
    let (y_rot, _) = painting_rotation(p.direction);
    entity.position = aabb_center(bb);
    entity.y_rot = y_rot;
}

/// 同步展示框的位置与朝向。
pub fn recalculate_item_frame(entity: &mut Entity) {
    let EntityKind::ItemFrame(f) = &mut entity.kind else {
        return;
    };
    let bb = f.bounding_box();
    let (y_rot, x_rot) = item_frame_rotation(f.direction);
    f.x_rot = x_rot;
    entity.position = aabb_center(bb);
    entity.y_rot = y_rot;
}

impl Entity {
    /// 构造一张画实体并按当前方向和变体算好中心坐标。
    pub fn painting(pos: BlockPos, direction: Direction, variant: i32) -> Self {
        let painting = Painting::new(pos, direction, variant);
        let bb = painting.bounding_box();
        let (y_rot, _) = painting_rotation(painting.direction);
        let mut entity = Self::new(EntityKind::Painting(Box::new(painting)), aabb_center(bb));
        entity.y_rot = y_rot;
        entity
    }

    /// 构造一个物品展示框（或荧光物品展示框）实体并算好中心坐标和角度。
    pub fn item_frame(pos: BlockPos, direction: Direction, glowing: bool) -> Self {
        let frame = ItemFrame::new(pos, direction, glowing);
        let bb = frame.bounding_box();
        let (y_rot, _) = item_frame_rotation(frame.direction);
        let mut entity = Self::new(EntityKind::ItemFrame(Box::new(frame)), aabb_center(bb));
        entity.y_rot = y_rot;
        entity
    }
}

/// 悬挂实体掉落命令的载荷，落地时播音效、掉物品并在展示框时刷新比较器。
///
/// Payload for [`Command::DropHanging`] applied in the serial phase.
#[derive(Debug, Clone, PartialEq)]
pub struct HangingDrop {
    pub kind: HangingDropKind,
    pub position: Vec3,
    pub pos: BlockPos,
    pub direction: Direction,
    pub custom_name: Option<Tag>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HangingDropKind {
    Painting,
    ItemFrame {
        glowing: bool,
        item: ItemStack,
        drop_chance: f32,
        fixed: bool,
    },
}

/// `BlockAttachedEntity.tick`：检查掉出世界下界，每 100 刻检查一次 `survives()`。
///
/// 注意官方字节码是 `if (this.ticksSinceLastCheck++ >= 100)`：比较自增前的旧值，
/// 因此新建实体在第 101 刻第一次触发检查。
pub(crate) fn tick_painting(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    // `Entity.checkBelowWorld`: `getY() < minY - 64`。
    if me.position.y < f64::from(ctx.blocks().min_y() - 64) {
        me.discard();
        return;
    }
    let old = match &mut me.kind {
        EntityKind::Painting(p) => {
            if me.tick_count > 1 {
                p.needs_sync = false;
            }
            let old = p.ticks_since_last_check;
            p.ticks_since_last_check += 1;
            old
        }
        _ => return,
    };
    if old < CHECK_INTERVAL {
        return;
    }
    let EntityKind::Painting(p) = &mut me.kind else {
        return;
    };
    p.ticks_since_last_check = 0;
    if me.removed {
        return;
    }
    let p_clone = p.clone();
    let context = me.collision_context();
    let alive = painting_survives_with(ctx.blocks(), &p_clone, context, |pop_box| {
        let mut blocked = false;
        ctx.for_each_other_of_types_in(pop_box, &HANGING_ENTITY_TYPES, &mut |other| {
            if !blocked
                && conflicts_with(
                    me.id,
                    entity_type::PAINTING,
                    p_clone.direction,
                    false,
                    pop_box,
                    other,
                )
            {
                blocked = true;
            }
        });
        !blocked
    });
    if !alive {
        me.discard();
        ctx.emit(
            Command::DropHanging(HangingDrop {
                kind: HangingDropKind::Painting,
                position: me.position,
                pos: p_clone.pos,
                direction: p_clone.direction,
                custom_name: p_clone.custom_name,
            }),
            me,
        );
    }
}

/// 物品展示框的 `BlockAttachedEntity.tick`。
pub(crate) fn tick_item_frame(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    if me.position.y < f64::from(ctx.blocks().min_y() - 64) {
        me.discard();
        return;
    }
    let old = match &mut me.kind {
        EntityKind::ItemFrame(f) => {
            if me.tick_count > 1 {
                f.needs_sync = false;
            }
            let old = f.ticks_since_last_check;
            f.ticks_since_last_check += 1;
            old
        }
        _ => return,
    };
    if old < CHECK_INTERVAL {
        return;
    }
    let EntityKind::ItemFrame(f) = &mut me.kind else {
        return;
    };
    f.ticks_since_last_check = 0;
    if me.removed {
        return;
    }
    let f_clone = f.clone();
    let context = me.collision_context();
    let self_type = f_clone.entity_type();
    let alive = item_frame_survives_with(ctx.blocks(), &f_clone, context, |pop_box| {
        let mut blocked = false;
        ctx.for_each_other_of_types_in(pop_box, &HANGING_ENTITY_TYPES, &mut |other| {
            if !blocked && conflicts_with(me.id, self_type, f_clone.direction, true, pop_box, other)
            {
                blocked = true;
            }
        });
        !blocked
    });
    if !alive {
        me.discard();
        let item = if let EntityKind::ItemFrame(f) = &mut me.kind {
            std::mem::replace(&mut f.item, ItemStack::empty())
        } else {
            ItemStack::empty()
        };
        ctx.emit(
            Command::DropHanging(HangingDrop {
                kind: HangingDropKind::ItemFrame {
                    glowing: f_clone.glowing,
                    item,
                    drop_chance: f_clone.drop_chance,
                    fixed: f_clone.fixed,
                },
                position: me.position,
                pos: f_clone.pos,
                direction: f_clone.direction,
                custom_name: f_clone.custom_name,
            }),
            me,
        );
    }
}

/// [`Command::DropHanging`] 的串行落地（自然脱落，`breaker = None`）。
pub(crate) fn apply_drop_command(world: &mut World, me: &mut Entity, drop: HangingDrop) {
    match drop.kind {
        HangingDropKind::Painting => {
            if !world.block_game_rules().entity_drops {
                return;
            }
            world.play_sound(
                drop.position,
                sounds::sound_event("minecraft:entity.painting.break"),
                sounds::NEUTRAL,
                1.0,
                1.0,
                false,
            );
            let stack = item_with_custom_name(items::PAINTING, drop.custom_name.as_ref());
            world.spawn_at_location(drop_position(drop.position, drop.direction), stack);
        }
        HangingDropKind::ItemFrame {
            glowing,
            item,
            drop_chance,
            fixed,
        } => {
            world.play_sound(
                drop.position,
                item_frame_sound(glowing, FrameSound::Break),
                sounds::NEUTRAL,
                1.0,
                1.0,
                false,
            );
            if fixed {
                return;
            }
            // `dropItem` 会 `setItem(ItemStack.EMPTY)` 并更新比较器。
            world.update_neighbour_for_output_signal(drop.pos, BlockState::AIR.block());
            if !world.block_game_rules().entity_drops {
                return;
            }
            let pos = drop_position(drop.position, drop.direction);
            let frame_item = if glowing {
                items::GLOW_ITEM_FRAME
            } else {
                items::ITEM_FRAME
            };
            let frame_stack = item_with_custom_name(frame_item, drop.custom_name.as_ref());
            world.spawn_at_location(pos, frame_stack);
            if !item.is_empty() && me.random.next_float() < drop_chance {
                world.spawn_at_location(pos, item);
            }
        }
    }
}

/// 给掉落的画或展示框物品附加上实体的 `CustomName`。
fn item_with_custom_name(item: i32, custom_name: Option<&Tag>) -> ItemStack {
    let Some(name) = custom_name else {
        return ItemStack::new(item, 1);
    };
    let mut patch = ComponentPatch::new();
    patch.put(
        kinds::CUSTOM_NAME,
        PatchEntry::Set(ComponentValue::CustomName(component_from_checked_tag(
            name.clone(),
        ))),
    );
    let (added, removed) = patch.to_opaque();
    ItemStack {
        item,
        count: 1,
        bundle_selected: None,
        added,
        removed,
    }
}

/// 从物品堆读取 `CustomName` 组件（放置时继承到实体上）。
pub fn stack_custom_name(stack: &ItemStack) -> Option<Tag> {
    let patch = ComponentPatch::from_opaque(stack).ok()?;
    match patch.get(kinds::CUSTOM_NAME) {
        Some(ComponentValue::CustomName(component)) => Some(component.tag().clone()),
        _ => None,
    }
}

/// 从画物品堆读取显式指定的 `DataComponents.PAINTING_VARIANT`。
pub fn stack_painting_variant(stack: &ItemStack) -> Option<i32> {
    let patch = ComponentPatch::from_opaque(stack).ok()?;
    match patch.get(kinds::PAINTING_VARIANT) {
        Some(ComponentValue::PaintingVariant(Holder::Id(id))) => Some(*id),
        _ => None,
    }
}

/// `Painting.create(level, pos, direction)`：从 `#placeable` 筛出能活下来的画，
/// 只留最大面积的一组，再用实体自己的随机源挑一张。
///
/// `Painting.create(level, pos, direction)`: filter `#placeable` to surviving
/// variants, keep only those with the maximum area, and pick one with the
/// painting's own random source.
pub fn create_painting(world: &mut World, pos: BlockPos, direction: Direction) -> Option<Entity> {
    if !direction.axis().is_horizontal() {
        return None;
    }
    let placeable = painting_data::placeable_ids();
    if placeable.is_empty() {
        return None;
    }
    let mut entity = world.prepare_entity(Entity::painting(
        pos,
        direction,
        painting_data::DEFAULT_VARIANT,
    ));
    let mut candidates: Vec<i32> = Vec::with_capacity(placeable.len());
    for &variant in placeable {
        if let EntityKind::Painting(p) = &mut entity.kind {
            p.variant = variant;
        }
        recalculate_painting(&mut entity);
        let survives = if let EntityKind::Painting(p) = &entity.kind {
            painting_survives(world, &entity, p)
        } else {
            false
        };
        if survives {
            candidates.push(variant);
        }
    }
    if candidates.is_empty() {
        return None;
    }
    let max_area = candidates
        .iter()
        .map(|&v| painting_data::area(v))
        .max()
        .unwrap_or(0);
    candidates.retain(|&v| painting_data::area(v) >= max_area);
    if candidates.is_empty() {
        return None;
    }
    let chosen = candidates[entity.random.next_int_bounded(candidates.len() as i32) as usize];
    if let EntityKind::Painting(p) = &mut entity.kind {
        p.variant = chosen;
        p.direction = direction;
    }
    recalculate_painting(&mut entity);
    Some(entity)
}

/// 伤害悬挂实体时的来源分类，对应官方 `BlockAttachedEntity.hurtServer` / `ItemFrame.hurtServer`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HangingHurt {
    /// 攻击者是否是创造模式玩家（不掉物品，且能拆 `Fixed` 展示框）。
    pub creative_player: bool,
    /// 攻击者是否是生物（受 `mob_griefing` 门控）。
    pub attacker_is_mob: bool,
    /// 是否是爆炸伤害（展示框受爆炸时直接拆框，不只掉框内物品）。
    pub is_explosion: bool,
    /// 是否绕过无敌（如 `/kill`、虚空；可拆 `Fixed` 展示框）。
    pub bypasses_invulnerability: bool,
}

impl HangingHurt {
    pub const fn player(creative: bool) -> Self {
        Self {
            creative_player: creative,
            attacker_is_mob: false,
            is_explosion: false,
            bypasses_invulnerability: false,
        }
    }

    pub const fn explosion(attacker_is_mob: bool) -> Self {
        Self {
            creative_player: false,
            attacker_is_mob,
            is_explosion: true,
            bypasses_invulnerability: false,
        }
    }

    pub const fn from_damage_kind(
        kind: DamageKind,
        creative_player: bool,
        attacker_is_mob: bool,
    ) -> Self {
        let is_explosion = matches!(
            kind,
            DamageKind::Explosion | DamageKind::PlayerExplosion | DamageKind::BadRespawnPoint
        );
        let bypasses_invulnerability =
            matches!(kind, DamageKind::GenericKill | DamageKind::OutOfWorld);
        Self {
            creative_player,
            attacker_is_mob,
            is_explosion,
            bypasses_invulnerability,
        }
    }
}

/// 对悬挂实体施加一次伤害，返回是否生效（`hurtServer` 返回值）。
///
/// Apply `hurtServer` to a hanging entity (painting or item frame).
pub fn hurt(world: &mut World, uid: EntityUid, source: HangingHurt) -> bool {
    let Some(mut entity) = world.take_entity_for_hanging(uid) else {
        return false;
    };
    if entity.removed {
        world.put_entity_for_hanging(entity);
        return false;
    }
    let result = match &entity.kind {
        EntityKind::Painting(_) => hurt_painting(world, &mut entity, source),
        EntityKind::ItemFrame(_) => hurt_item_frame(world, &mut entity, source),
        _ => false,
    };
    let removed = entity.removed;
    world.put_entity_for_hanging(entity);
    if removed {
        world.remove_entity(uid);
    }
    result
}

fn hurt_painting(world: &mut World, entity: &mut Entity, source: HangingHurt) -> bool {
    if !world.block_game_rules().mob_griefing && source.attacker_is_mob {
        return false;
    }
    let EntityKind::Painting(p) = &entity.kind else {
        return false;
    };
    let position = entity.position;
    let direction = p.direction;
    let custom_name = p.custom_name.clone();
    entity.discard();
    // `Painting.dropItem`：`ENTITY_DROPS` 为假时连音效都不播。
    if !world.block_game_rules().entity_drops {
        return true;
    }
    world.play_sound(
        position,
        sounds::sound_event("minecraft:entity.painting.break"),
        sounds::NEUTRAL,
        1.0,
        1.0,
        false,
    );
    if !source.creative_player {
        let stack = item_with_custom_name(items::PAINTING, custom_name.as_ref());
        world.spawn_at_location(drop_position(position, direction), stack);
    }
    true
}

fn hurt_item_frame(world: &mut World, entity: &mut Entity, source: HangingHurt) -> bool {
    let (fixed, has_item, glowing, pos, direction, custom_name, drop_chance) = match &entity.kind {
        EntityKind::ItemFrame(f) => (
            f.fixed,
            !f.item.is_empty(),
            f.glowing,
            f.pos,
            f.direction,
            f.custom_name.clone(),
            f.drop_chance,
        ),
        _ => return false,
    };
    if fixed {
        let can_hurt = source.bypasses_invulnerability || source.creative_player;
        if !can_hurt {
            return false;
        }
        if !world.block_game_rules().mob_griefing && source.attacker_is_mob {
            return false;
        }
        entity.discard();
        // `ItemFrame.dropItem(ServerLevel, Entity)` 先播破坏音，再因 `fixed` 不掉落。
        world.play_sound(
            entity.position,
            item_frame_sound(glowing, FrameSound::Break),
            sounds::NEUTRAL,
            1.0,
            1.0,
            false,
        );
        return true;
    }
    // 第一下：非爆炸且框里有物品时只打掉框内物品，框留在墙上。
    if !source.is_explosion && has_item {
        let dropped = if let EntityKind::ItemFrame(f) = &mut entity.kind {
            std::mem::replace(&mut f.item, ItemStack::empty())
        } else {
            ItemStack::empty()
        };
        recalculate_item_frame(entity);
        world.update_neighbour_for_output_signal(pos, BlockState::AIR.block());
        if world.block_game_rules().entity_drops
            && !source.creative_player
            && !dropped.is_empty()
            && entity.random.next_float() < drop_chance
        {
            world.spawn_at_location(drop_position(entity.position, direction), dropped);
        }
        world.play_sound(
            entity.position,
            item_frame_sound(glowing, FrameSound::RemoveItem),
            sounds::NEUTRAL,
            1.0,
            1.0,
            false,
        );
        return true;
    }
    // 第二下（空框）或爆炸：打掉整个展示框。
    if !world.block_game_rules().mob_griefing && source.attacker_is_mob {
        return false;
    }
    let dropped = if let EntityKind::ItemFrame(f) = &mut entity.kind {
        std::mem::replace(&mut f.item, ItemStack::empty())
    } else {
        ItemStack::empty()
    };
    let position = entity.position;
    entity.discard();
    world.play_sound(
        position,
        item_frame_sound(glowing, FrameSound::Break),
        sounds::NEUTRAL,
        1.0,
        1.0,
        false,
    );
    world.update_neighbour_for_output_signal(pos, BlockState::AIR.block());
    if world.block_game_rules().entity_drops && !source.creative_player {
        let drop_pos = drop_position(position, direction);
        let frame_item = if glowing {
            items::GLOW_ITEM_FRAME
        } else {
            items::ITEM_FRAME
        };
        let frame_stack = item_with_custom_name(frame_item, custom_name.as_ref());
        world.spawn_at_location(drop_pos, frame_stack);
        if !dropped.is_empty() && entity.random.next_float() < drop_chance {
            world.spawn_at_location(drop_pos, dropped);
        }
    }
    true
}

/// `ItemFrame.interact` 的结果：交互结果 + 是否消耗玩家手上 1 个物品（生存模式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemFrameInteractOutcome {
    pub result: InteractionResult,
    pub consume_held: bool,
}

/// `ItemFrame.interact(player, hand)`：空框放物品并消耗 1 个，有物品则顺时针旋转一档。
///
/// `ItemFrame.interact(player, hand)`: place one item into an empty frame, or rotate
/// an existing item by one step (mod 8).
pub fn interact_item_frame(
    world: &mut World,
    uid: EntityUid,
    held: &ItemStack,
    creative: bool,
) -> ItemFrameInteractOutcome {
    let Some(mut entity) = world.take_entity_for_hanging(uid) else {
        return ItemFrameInteractOutcome {
            result: InteractionResult::Pass,
            consume_held: false,
        };
    };
    let EntityKind::ItemFrame(frame) = &mut entity.kind else {
        world.put_entity_for_hanging(entity);
        return ItemFrameInteractOutcome {
            result: InteractionResult::Pass,
            consume_held: false,
        };
    };
    if frame.fixed || entity.removed {
        world.put_entity_for_hanging(entity);
        return ItemFrameInteractOutcome {
            result: InteractionResult::Pass,
            consume_held: false,
        };
    }
    let has_item = !frame.item.is_empty();
    let has_held = !held.is_empty();
    if !has_item {
        if !has_held {
            world.put_entity_for_hanging(entity);
            return ItemFrameInteractOutcome {
                result: InteractionResult::Pass,
                consume_held: false,
            };
        }
        let mut single = held.clone();
        single.count = 1;
        let pos = frame.pos;
        let sound = frame.add_item_sound();
        frame.item = single;
        recalculate_item_frame(&mut entity);
        let position = entity.position;
        world.put_entity_for_hanging(entity);
        world.play_sound(position, sound, sounds::NEUTRAL, 1.0, 1.0, false);
        world.update_neighbour_for_output_signal(pos, BlockState::AIR.block());
        return ItemFrameInteractOutcome {
            result: InteractionResult::Success,
            consume_held: !creative,
        };
    }
    // 已有物品：播旋转音、旋转 +1（mod 8）、刷新比较器。
    let pos = frame.pos;
    let sound = frame.rotate_item_sound();
    frame.rotation = (frame.rotation + 1).rem_euclid(NUM_ROTATIONS);
    let position = entity.position;
    world.put_entity_for_hanging(entity);
    world.play_sound(position, sound, sounds::NEUTRAL, 1.0, 1.0, false);
    world.update_neighbour_for_output_signal(pos, BlockState::AIR.block());
    ItemFrameInteractOutcome {
        result: InteractionResult::Success,
        consume_held: false,
    }
}

/// 爆炸命中范围内的悬挂实体（`BlockAttachedEntity.ignoreExplosion` + `hurtServer`）。
///
/// Damage hanging entities inside an explosion blast (`shouldAffectBlocklikeEntities`).
pub(crate) fn hurt_hanging_by_explosion(
    world: &mut World,
    center: Vec3,
    radius: f32,
    source_in_water: bool,
    attacker_is_mob: bool,
) {
    if radius < 1.0e-5 || source_in_water {
        return;
    }
    let double_radius = f64::from(radius * 2.0);
    let area = Aabb::new(
        f64::from(mc_bamboo_core::mth::floor(center.x - double_radius - 1.0)),
        f64::from(mc_bamboo_core::mth::floor(center.y - double_radius - 1.0)),
        f64::from(mc_bamboo_core::mth::floor(center.z - double_radius - 1.0)),
        f64::from(mc_bamboo_core::mth::floor(center.x + double_radius + 1.0)),
        f64::from(mc_bamboo_core::mth::floor(center.y + double_radius + 1.0)),
        f64::from(mc_bamboo_core::mth::floor(center.z + double_radius + 1.0)),
    );
    let mut victims = Vec::new();
    for id in world.entity_ids() {
        let Some(entity) = world.entity(id) else {
            continue;
        };
        if entity.removed
            || !matches!(
                entity.kind,
                EntityKind::Painting(_) | EntityKind::ItemFrame(_)
            )
        {
            continue;
        }
        let bounds = entity.bounding_box();
        if !bounds.intersects(area) {
            continue;
        }
        let dist = entity.position.distance_to_sqr(center).sqrt() / double_radius;
        if dist > 1.0 {
            continue;
        }
        victims.push(id);
    }
    for uid in victims {
        let _ = hurt(world, uid, HangingHurt::explosion(attacker_is_mob));
    }
}

/// `ComparatorBlock.getItemFrame`：在 `pos` 这一格内查找面向 `facing` 的唯一展示框，
/// 恰好找到 1 个时返回其 `getAnalogOutput()`。
///
/// `ComparatorBlock.getItemFrame`: return `getAnalogOutput()` when exactly one
/// item frame facing `facing` intersects cell `pos`.
pub(crate) fn comparator_item_frame_signal(
    world: &World,
    facing: Direction,
    pos: BlockPos,
) -> Option<i32> {
    world.level().comparator_item_frame_signal(facing, pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BlockTicks,
        archive::entity::{read_entity, write_entity},
        context::{BlockHit, UseContext},
        testing::{TestLevel, actor_holding},
    };
    use mc_bamboo_core::Hand;
    use mc_bamboo_data::blocks;

    fn build_wall(level: &mut TestLevel, x0: i32, x1: i32, y0: i32, y1: i32, z: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                level.set(BlockPos::new(x, y, z), BlockState(blocks::STONE));
            }
        }
    }

    #[test]
    fn painting_and_item_frame_bounding_boxes_match_vanilla_formulas() {
        // 1x1 kebab facing South at (0, 64, 1)
        let kebab = painting_data::id_of("minecraft:kebab").unwrap();
        let bb = painting_bounding_box(BlockPos::new(0, 64, 1), Direction::South, kebab);
        assert_eq!(bb, Aabb::new(0.0, 64.0, 1.0, 1.0, 65.0, 1.0625));

        // 2x2 bust facing South at (0, 64, 1): ccw(South)=East (+x), offset +0.5 on x and y
        let bust = painting_data::id_of("minecraft:bust").unwrap();
        let bb2 = painting_bounding_box(BlockPos::new(0, 64, 1), Direction::South, bust);
        assert_eq!(bb2, Aabb::new(0.0, 64.0, 1.0, 2.0, 66.0, 1.0625));

        // ItemFrame facing South without map: 0.75 x 0.75 x 0.0625
        let fb = item_frame_bounding_box(BlockPos::new(0, 64, 1), Direction::South, false);
        assert_eq!(fb, Aabb::new(0.125, 64.125, 1.0, 0.875, 64.875, 1.0625));
        // With map: 1.0 x 1.0 x 0.0625
        let fb_map = item_frame_bounding_box(BlockPos::new(0, 64, 1), Direction::South, true);
        assert_eq!(fb_map, Aabb::new(0.0, 64.0, 1.0, 1.0, 65.0, 1.0625));
    }

    #[test]
    fn painting_place_picks_max_area_and_drops_on_tick_101_when_wall_breaks() {
        let mut level = TestLevel::new();
        // 2x1 wall at z=0: (0,64,0) and (1,64,0)
        build_wall(&mut level, 0, 1, 64, 64, 0);
        let actor = actor_holding(items::PAINTING);
        let mut ticks = BlockTicks::default();
        let uid = {
            let mut world = World::new(&mut level, &mut ticks);
            let hit = BlockHit {
                pos: BlockPos::new(0, 64, 0),
                face: Direction::South,
                location: Vec3::new(0.5, 64.5, 1.0),
                inside: false,
            };
            let ctx = UseContext::new(&actor, Hand::Main, hit);
            let res = crate::item_use::use_on(
                &mut world,
                &mc_bamboo_data::items::ITEMS[items::PAINTING as usize],
                &ctx,
            );
            assert_eq!(res, InteractionResult::Success);
            let ids = world.entity_ids();
            assert_eq!(ids.len(), 1);
            let ent = world.entity(ids[0]).unwrap();
            let EntityKind::Painting(p) = &ent.kind else {
                panic!("expected painting");
            };
            // On a 2x1 wall, max area is 2 (width=2, height=1).
            assert_eq!(painting_data::size(p.variant), Some((2, 1)));
            assert_eq!(p.direction, Direction::South);
            assert_eq!(p.pos, BlockPos::new(0, 64, 1));
            ids[0]
        };

        // Break one backing block and verify painting survives ticks 1..=100, drops on tick 101.
        level.set(BlockPos::new(1, 64, 0), BlockState::AIR);
        for tick in 1..=100 {
            let mut world = World::new(&mut level, &mut ticks);
            world.tick_entities();
            assert!(
                world.entity(uid).is_some(),
                "painting should still exist at tick {tick}"
            );
        }
        {
            let mut world = World::new(&mut level, &mut ticks);
            world.tick_entities();
            assert!(
                world.entity(uid).is_none(),
                "painting should drop on tick 101"
            );
            // Dropped painting item entity spawned.
            let dropped: Vec<_> = world
                .live_entities()
                .into_iter()
                .filter(
                    |e| matches!(&e.kind, EntityKind::Item(i) if i.stack.item == items::PAINTING),
                )
                .collect();
            assert_eq!(dropped.len(), 1);
        }
    }

    #[test]
    fn item_frame_interact_rotate_comparator_and_two_step_break() {
        let mut level = TestLevel::new();
        let wall = BlockPos::new(0, 64, 0);
        let frame_pos = BlockPos::new(0, 64, 1);
        let comp_pos = BlockPos::new(0, 64, -1);
        level.set(wall, BlockState(blocks::STONE));
        level.set(BlockPos::new(0, 63, -1), BlockState(blocks::STONE));
        // Comparator facing South (points into wall at z=0, reads frame at z=1 facing South).
        let comp_state = BlockState(blocks::COMPARATOR)
            .default_state()
            .with(crate::state::HORIZONTAL_FACING, Direction::South);
        level.set(comp_pos, comp_state);
        let mut ticks = BlockTicks::default();

        let frame_id = {
            let mut world = World::new(&mut level, &mut ticks);
            world.spawn_entity(Entity::item_frame(frame_pos, Direction::South, false))
        };

        // Empty frame -> comparator signal 0.
        {
            let world = World::new(&mut level, &mut ticks);
            assert_eq!(
                comparator_item_frame_signal(&world, Direction::South, frame_pos),
                Some(0)
            );
        }

        // Put stone into frame -> rotation 0 -> analog output 1.
        {
            let mut world = World::new(&mut level, &mut ticks);
            let out = world.interact_item_frame(frame_id, &ItemStack::new(items::STONE, 5), false);
            assert_eq!(out.result, InteractionResult::Success);
            assert!(out.consume_held);
            assert_eq!(
                comparator_item_frame_signal(&world, Direction::South, frame_pos),
                Some(1)
            );
        }

        // Rotate 3 times -> rotation 3 -> analog output 4.
        for expected in 2..=4 {
            let mut world = World::new(&mut level, &mut ticks);
            let out = world.interact_item_frame(frame_id, &ItemStack::empty(), false);
            assert_eq!(out.result, InteractionResult::Success);
            assert!(!out.consume_held);
            assert_eq!(
                comparator_item_frame_signal(&world, Direction::South, frame_pos),
                Some(expected)
            );
        }

        // First punch drops only the item; frame stays empty.
        {
            let mut world = World::new(&mut level, &mut ticks);
            assert!(world.hurt_hanging(frame_id, HangingHurt::player(false)));
            let frame = world.entity(frame_id).expect("frame stays after first hit");
            let EntityKind::ItemFrame(f) = &frame.kind else {
                panic!("expected frame");
            };
            assert!(f.item.is_empty());
            assert_eq!(f.analog_output(), 0);
            let stones = world
                .live_entities()
                .into_iter()
                .filter(|e| matches!(&e.kind, EntityKind::Item(i) if i.stack.item == items::STONE))
                .count();
            assert_eq!(stones, 1);
        }

        // Second punch breaks the frame and drops the frame item.
        {
            let mut world = World::new(&mut level, &mut ticks);
            assert!(world.hurt_hanging(frame_id, HangingHurt::player(false)));
            assert!(world.entity(frame_id).is_none());
            let frames = world
                .live_entities()
                .into_iter()
                .filter(
                    |e| matches!(&e.kind, EntityKind::Item(i) if i.stack.item == items::ITEM_FRAME),
                )
                .count();
            assert_eq!(frames, 1);
        }
    }

    #[test]
    fn fixed_item_frame_blocks_survival_and_nbt_roundtrips() {
        let mut frame = Entity::item_frame(BlockPos::new(2, 65, -3), Direction::Up, true);
        if let EntityKind::ItemFrame(f) = &mut frame.kind {
            f.item = ItemStack::new(items::DIAMOND, 1);
            f.rotation = 5;
            f.drop_chance = 0.5;
            f.fixed = true;
            f.invisible = true;
        }
        recalculate_item_frame(&mut frame);
        let row = frame.clone().split();
        let tag = write_entity(&row).expect("write glow item frame");
        let loaded = read_entity(&tag, 99).expect("read").expect("known");
        let loaded_ent = loaded.into_entity();
        let EntityKind::ItemFrame(f) = &loaded_ent.kind else {
            panic!("expected glow item frame");
        };
        assert!(f.glowing);
        assert_eq!(f.pos, BlockPos::new(2, 65, -3));
        assert_eq!(f.direction, Direction::Up);
        assert_eq!(f.item.item, items::DIAMOND);
        assert_eq!(f.rotation, 5);
        assert_eq!(f.drop_chance, 0.5);
        assert!(f.fixed);
        assert!(f.invisible);
        assert_eq!(f.x_rot, -90.0);

        // Painting NBT roundtrip.
        let variant = painting_data::id_of("minecraft:donkey_kong").unwrap();
        let painting = Entity::painting(BlockPos::new(-4, 70, 8), Direction::West, variant);
        let p_tag = write_entity(&painting.clone().split()).expect("write painting");
        let p_loaded = read_entity(&p_tag, 100)
            .expect("read")
            .expect("known")
            .into_entity();
        let EntityKind::Painting(p) = &p_loaded.kind else {
            panic!("expected painting");
        };
        assert_eq!(p.pos, BlockPos::new(-4, 70, 8));
        assert_eq!(p.direction, Direction::West);
        assert_eq!(p.variant, variant);
        assert_eq!(p_loaded.position, painting.position);
        assert_eq!(p_loaded.y_rot, painting.y_rot);
    }

    #[test]
    fn coexistence_diode_support_and_real_comparator_match_vanilla() {
        use crate::{
            interact::run_due_ticks,
            level::LevelTables,
            state::{ComparatorMode, HORIZONTAL_FACING, MODE_COMPARATOR, POWERED},
            world::UPDATE_ALL,
        };
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        // Corner walls at (0,64,0) and (-1,64,1) so cell (0,64,1) can hold both a South-facing
        // frame (backed by z=0) and an East-facing frame (backed by x=-1).
        level.set(BlockPos::new(0, 64, 0), BlockState(blocks::STONE));
        level.set(BlockPos::new(-1, 64, 1), BlockState(blocks::STONE));
        level.set(BlockPos::new(0, 63, -1), BlockState(blocks::STONE));
        level.set(BlockPos::new(0, 63, 0), BlockState(blocks::STONE));

        // Two ItemFrames with different facings coexist in the same block cell.
        let (frame_s, frame_e) = {
            let mut world = World::new(&mut level, &mut ticks);
            let s = world.spawn_entity(Entity::item_frame(
                BlockPos::new(0, 64, 1),
                Direction::South,
                false,
            ));
            let e = Entity::item_frame(BlockPos::new(0, 64, 1), Direction::East, true);
            let EntityKind::ItemFrame(ref f_e) = e.kind else {
                panic!("frame");
            };
            assert!(
                item_frame_survives(&world, &e, f_e),
                "frames facing different directions coexist"
            );
            let e_id = world.spawn_entity(e);
            // A second South-facing frame in the same cell conflicts.
            let dup = Entity::item_frame(BlockPos::new(0, 64, 1), Direction::South, false);
            let EntityKind::ItemFrame(ref f_dup) = dup.kind else {
                panic!("frame");
            };
            assert!(
                !item_frame_survives(&world, &dup, f_dup),
                "two frames with same direction conflict"
            );
            // A Painting facing East conflicts with any existing hanging entity (even different facing).
            let p = Entity::painting(
                BlockPos::new(0, 64, 1),
                Direction::East,
                painting_data::id_of("minecraft:kebab").unwrap(),
            );
            let EntityKind::Painting(ref p_data) = p.kind else {
                panic!("painting");
            };
            assert!(
                !painting_survives(&world, &p, p_data),
                "painting conflicts with hanging entities of any direction"
            );
            (s, e_id)
        };
        let _ = frame_e;

        // Place a real Comparator at (0, 64, -1) facing South, reading through stone at (0, 64, 0)
        // to the South-facing item frame at (0, 64, 1).
        {
            let mut world = World::new(&mut level, &mut ticks);
            let comp = BlockState(blocks::COMPARATOR)
                .with(HORIZONTAL_FACING, Direction::South)
                .with(MODE_COMPARATOR, ComparatorMode::Compare);
            world.set_block(BlockPos::new(0, 64, -1), comp, UPDATE_ALL);
            // Put an item into frame_s and rotate twice (rotation = 2 -> output = 3).
            world.interact_item_frame(frame_s, &ItemStack::new(items::APPLE, 1), false);
            world.interact_item_frame(frame_s, &ItemStack::empty(), false);
            world.interact_item_frame(frame_s, &ItemStack::empty(), false);
        }
        for now in 1..=4 {
            ticks.set_time(now);
            let _ = run_due_ticks(&mut level, &mut ticks, now);
        }
        assert_eq!(
            level
                .redstone_table()
                .comparator_output(BlockPos::new(0, 64, -1)),
            3
        );
        assert!(level.get(BlockPos::new(0, 64, -1)).get(POWERED));

        // Diode support: horizontal frame survives on a repeater; vertical (Up) frame does not.
        level.set(BlockPos::new(5, 64, 5), BlockState(blocks::REPEATER));
        {
            let world = World::new(&mut level, &mut ticks);
            let horiz = Entity::item_frame(BlockPos::new(5, 64, 6), Direction::South, false);
            let EntityKind::ItemFrame(ref f_h) = horiz.kind else {
                panic!("frame");
            };
            assert!(item_frame_survives(&world, &horiz, f_h));
            let vert = Entity::item_frame(BlockPos::new(5, 65, 5), Direction::Up, false);
            let EntityKind::ItemFrame(ref f_v) = vert.kind else {
                panic!("frame");
            };
            assert!(!item_frame_survives(&world, &vert, f_v));
        }
    }

    #[test]
    fn explosion_and_arrow_damage_hanging_entities() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        build_wall(&mut level, 0, 2, 64, 65, 0);
        let (painting_id, frame_id, fixed_id) = {
            let mut world = World::new(&mut level, &mut ticks);
            let p = world.spawn_entity(Entity::painting(
                BlockPos::new(0, 64, 1),
                Direction::South,
                painting_data::id_of("minecraft:kebab").unwrap(),
            ));
            let mut f_ent = Entity::item_frame(BlockPos::new(1, 64, 1), Direction::South, false);
            if let EntityKind::ItemFrame(f) = &mut f_ent.kind {
                f.item = ItemStack::new(items::DIAMOND, 1);
            }
            let f = world.spawn_entity(f_ent);
            let mut fix_ent = Entity::item_frame(BlockPos::new(2, 64, 1), Direction::South, true);
            if let EntityKind::ItemFrame(fx) = &mut fix_ent.kind {
                fx.fixed = true;
            }
            let fx = world.spawn_entity(fix_ent);
            (p, f, fx)
        };

        // Submerged explosion does nothing (`source_in_water = true`).
        {
            let mut world = World::new(&mut level, &mut ticks);
            hurt_hanging_by_explosion(&mut world, Vec3::new(1.0, 64.5, 2.0), 4.0, true, false);
            assert!(world.entity(painting_id).is_some());
            assert!(world.entity(frame_id).is_some());
            assert!(world.entity(fixed_id).is_some());
        }

        // Mob explosion with mob_griefing=false does nothing.
        level.rules.mob_griefing = false;
        {
            let mut world = World::new(&mut level, &mut ticks);
            hurt_hanging_by_explosion(&mut world, Vec3::new(1.0, 64.5, 2.0), 4.0, false, true);
            assert!(world.entity(painting_id).is_some());
            assert!(world.entity(frame_id).is_some());
        }
        level.rules.mob_griefing = true;

        // Dry TNT explosion breaks painting + non-fixed frame (dropping frame + diamond), spares fixed frame.
        {
            let mut world = World::new(&mut level, &mut ticks);
            hurt_hanging_by_explosion(&mut world, Vec3::new(1.0, 64.5, 2.0), 4.0, false, false);
            assert!(world.entity(painting_id).is_none());
            assert!(world.entity(frame_id).is_none());
            assert!(world.entity(fixed_id).is_some());
            let dropped: Vec<i32> = world
                .live_entities()
                .into_iter()
                .filter_map(|e| match e.kind {
                    EntityKind::Item(i) => Some(i.stack.item),
                    _ => None,
                })
                .collect();
            assert!(dropped.contains(&items::PAINTING));
            assert!(dropped.contains(&items::ITEM_FRAME));
            assert!(dropped.contains(&items::DIAMOND));
        }
    }
}
