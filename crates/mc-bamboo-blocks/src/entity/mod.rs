//! 实体：世界线程拥有、按 ID 引用的下落方块、点燃的 TNT 和地上的物品。
//!
//! 实体只有一个线程碰得到（见 `docs/folia.md`），所以这里没有锁，也没有引用：
//! [`Entities`] 按 ID 存在世界里，别的代码只拿到 ID。实体在计划刻和随机刻之后 tick。
//! 官方实体在方块实体之前，这里方块实体已经在计划刻里跑完了。
//!
//! Entities are owned by the world thread and referenced by id, with no locks.
//! Falling blocks, primed TNT, and ground items live here.
//! They tick after scheduled ticks and random ticks. Vanilla ticks entities before
//! block entities; those already ran inside the scheduled-tick step.

pub mod ctx;
pub mod parallel;
pub mod random;
pub mod store;
pub mod view;

mod explosion;
mod falling;
mod item;
mod physics;
mod tnt;

use mc_bamboo_core::{Axis, BlockPos, Vec3};
use mc_bamboo_data::entity_types;
use mc_bamboo_protocol::item::ItemStack;

use crate::{shape::Aabb, state::BlockState};

pub(crate) use item::merge_explosion;
pub use item::{can_pick_up, set_throw_velocity, take_stack};
pub use physics::{
    TravelSegment, collide_border_only, enclosed_in_air, no_block_collision, overlaps_new_blocks,
    stuck_speed, sweep_movement,
};
pub use random::JavaRandom;
pub use store::{
    ChunkEntities, EntityPublic, EntityRow, EntityState, EntityStore, EntityUid, FallingBlockState,
    ItemState, KindPublic, KindState,
};

/// 世界实体 ID 从这个数开始，玩家 ID 从 1 开始，两边不会撞上。
pub const FIRST_ENTITY_ID: i32 = 1 << 24;

/// 这个世界随机源的固定种子。官方用 `RandomSupport.generateUniqueSeed()`，每次开服都不同。
/// 每个实体的随机源的种子是这个常数和实体 ID 异或出来的。官方实体的随机源是独立的，
/// 种子同样不可复现；这里让它只由 ID 决定，测试才比得了。
pub const ENTITY_RANDOM_SEED: i64 = 0x0DDB_1A5E_5BAD_5EED;

/// 下落的方块，对应 `FallingBlockEntity`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FallingBlock {
    pub state: BlockState,
    /// `DATA_START_POS`：从哪一格掉下来的，客户端用它决定破坏粒子。
    pub start_pos: BlockPos,
    /// `time`：落了多少 tick，超过 100 或者 600 会被丢掉。
    pub time: i32,
    /// `dropItem`。为假时超时和放不回去都不生成物品。
    ///
    /// `dropItem`. When false, a timeout or a failed landing spawns nothing.
    pub drop_item: bool,
    /// `cancelDrop`：落地时不放回方块。官方仍会 `callOnBrokenAfterFall`。
    ///
    /// `cancelDrop`: the block is not placed. Vanilla still calls `callOnBrokenAfterFall`.
    pub cancel_drop: bool,
    /// `hurtEntities`、`fallDamagePerDistance`、`fallDamageMax`：砸到实体的伤害不在范围里，
    /// 但铁砧靠它决定要不要裂开，所以照官方算。
    pub hurt_entities: bool,
    pub fall_damage_per_distance: f32,
    pub fall_damage_max: i32,
}

impl FallingBlock {
    pub const fn new(state: BlockState, start_pos: BlockPos) -> Self {
        Self {
            state,
            start_pos,
            time: 0,
            drop_item: true,
            cancel_drop: false,
            hurt_entities: false,
            fall_damage_per_distance: 0.0,
            fall_damage_max: 40,
        }
    }

    /// `setHurtsEntities`。
    pub fn set_hurts_entities(&mut self, damage_per_distance: f32, damage_max: i32) {
        self.hurt_entities = true;
        self.fall_damage_per_distance = damage_per_distance;
        self.fall_damage_max = damage_max;
    }

    /// `disableDrop`。
    pub const fn disable_drop(&mut self) {
        self.cancel_drop = true;
    }
}

/// 点燃的 TNT，对应 `PrimedTnt`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimedTnt {
    /// `fuse`，默认 80 tick。
    pub fuse: i32,
}

impl PrimedTnt {
    pub const DEFAULT_FUSE: i32 = 80;

    pub const fn new() -> Self {
        Self {
            fuse: Self::DEFAULT_FUSE,
        }
    }
}

impl Default for PrimedTnt {
    fn default() -> Self {
        Self::new()
    }
}

/// 地上的物品，对应 `ItemEntity`。字段在 [`item`] 里读写。
///
/// A ground item, vanilla `ItemEntity`. [`item`] reads and writes the fields.
#[derive(Debug, Clone, PartialEq)]
pub struct GroundItem {
    pub stack: ItemStack,
    /// `age`。到 6000 消失，`-32768` 表示无限。
    ///
    /// `age`. The item vanishes at 6000. `-32768` means infinite.
    pub age: i32,
    /// `pickupDelay`。`32767` 表示永远捡不起来。
    ///
    /// `pickupDelay`. `32767` means the item can never be picked up.
    pub pickup_delay: i32,
    /// `health`，默认 5。
    pub health: i32,
    /// `bobOffs`。只在构造时取一次，不同步。
    ///
    /// `bobOffs`. Drawn once in the constructor and never synced.
    pub bob_offs: f32,
    /// 三参数构造要取速度，四参数构造不取。
    ///
    /// The position constructor draws a velocity. The delta constructor does not.
    pub(crate) roll_velocity: bool,
    /// `target`。只有这个 UUID 能捡。没有成就系统，丢出时可以先空着。
    ///
    /// `target`. Only this UUID can pick the item up.
    pub target: Option<[u8; 16]>,
}

/// 实体类型。物品堆不能 `Copy`，所以这个枚举也不再 `Copy`。
///
/// An item stack is not `Copy`, so neither is this enum.
#[derive(Debug, Clone, PartialEq)]
pub enum EntityKind {
    FallingBlock(FallingBlock),
    Tnt(PrimedTnt),
    Item(GroundItem),
}

impl EntityKind {
    /// 注册表 ID，发 `add_entity` 包时用。
    pub const fn entity_type(&self) -> i32 {
        match self {
            Self::FallingBlock(_) => mc_bamboo_data::registries::entity_type::FALLING_BLOCK,
            Self::Tnt(_) => mc_bamboo_data::registries::entity_type::TNT,
            Self::Item(_) => mc_bamboo_data::registries::entity_type::ITEM,
        }
    }

    /// `EntityDimensions`。
    pub fn dimensions(&self) -> (f32, f32) {
        let facts = &entity_types::FACTS[self.entity_type() as usize];
        (facts.width, facts.height)
    }

    pub const fn is_falling_block(&self) -> bool {
        matches!(self, Self::FallingBlock(_))
    }
}

/// 一次移动，官方的 `Entity.Movement`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Movement {
    pub from: Vec3,
    pub to: Vec3,
    /// `axisDependentOriginalMovement`：`move` 记下的原始位移，按轴拆开检查时用。
    pub original: Option<Vec3>,
}

/// 一个实体。
///
/// 官方 `Entity` 里和这两种实体有关的字段都在这里。方向、乘客、载具、火这些没有。
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub id: i32,
    pub kind: EntityKind,
    pub position: Vec3,
    pub delta: Vec3,
    /// 上一 tick 的位置，官方的 `xo`、`yo`、`zo`。
    pub old_position: Vec3,
    pub on_ground: bool,
    pub horizontal_collision: bool,
    pub vertical_collision: bool,
    pub vertical_collision_below: bool,
    /// `fallDistance`。官方是 `double`，每次只把这一格的位移截成 `float` 再加进来。
    pub fall_distance: f64,
    /// `tickCount`。
    pub tick_count: i32,
    /// `mainSupportingBlockPos`。
    pub main_supporting_block: Option<BlockPos>,
    /// `onGroundNoBlocks`。
    pub on_ground_no_blocks: bool,
    /// `wasTouchingWater`。
    pub in_water: bool,
    /// `isRemoved()`。
    pub removed: bool,
    /// `stuckSpeedMultiplier`：蜘蛛网、细雪压扁位移用的。
    pub(crate) stuck_speed_multiplier: Vec3,
    /// `movementThisTick`：这一 tick 里 `move` 记下的移动。
    pub(crate) movement_this_tick: Vec<Movement>,
    /// `finalMovementsThisTick`。
    pub(crate) final_movements: Vec<Movement>,
    /// `requiresPrecisePosition`：这个 tick 撞到过东西，同步时不能偷懒。
    pub(crate) requires_precise_position: bool,
    /// 这个实体自己的随机源，官方的 `this.random`。
    pub random: JavaRandom,
    /// `yRot`。物品构造时取一次，配对时发给客户端。
    ///
    /// `yRot`. An item draws it once in the constructor and sends it when pairing.
    pub y_rot: f32,
    /// `firstTick`。物品的 `baseTick` 用它判断岩浆和溅水。
    ///
    /// `firstTick`. An item's `baseTick` uses it for lava and the splash.
    pub first_tick: bool,
    /// `remainingFireTicks`。
    pub remaining_fire_ticks: i32,
}

impl Entity {
    /// 新建一个实体。ID 在 [`Entities::spawn`] 里分配。
    pub fn new(kind: EntityKind, position: Vec3) -> Self {
        Self {
            id: 0,
            kind,
            position,
            delta: Vec3::default(),
            old_position: position,
            on_ground: false,
            horizontal_collision: false,
            vertical_collision: false,
            vertical_collision_below: false,
            fall_distance: 0.0,
            tick_count: 0,
            main_supporting_block: None,
            on_ground_no_blocks: false,
            in_water: false,
            removed: false,
            stuck_speed_multiplier: Vec3::default(),
            movement_this_tick: Vec::new(),
            final_movements: Vec::new(),
            requires_precise_position: false,
            random: JavaRandom::new(ENTITY_RANDOM_SEED),
            y_rot: 0.0,
            first_tick: true,
            remaining_fire_ticks: 0,
        }
    }

    /// 下落方块，位置是方块的中心、底部。
    pub fn falling_block(pos: BlockPos, state: BlockState) -> Self {
        let position = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y),
            f64::from(pos.z) + 0.5,
        );
        Self::new(
            EntityKind::FallingBlock(FallingBlock::new(state, pos)),
            position,
        )
    }

    /// 点燃的 TNT，位置是方块的底心。`angle` 是 `level.getRandom().nextDouble()`，
    /// 调用方负责取数，这样和方块逻辑共用同一个随机源。
    ///
    /// `angle` is `level.getRandom().nextDouble()`. The caller draws it so block logic
    /// and the entity share one random source.
    pub fn primed_tnt(position: Vec3, angle: f64) -> Self {
        let mut entity = Self::new(EntityKind::Tnt(PrimedTnt::new()), position);
        // `(float) (Math.PI * 2)`，官方先算成 `float` 再乘。
        // Official casts `Math.PI * 2` to `float` before multiplying.
        let full_turn = f64::from((std::f64::consts::PI * 2.0) as f32);
        let rotation = angle * full_turn;
        entity.delta = Vec3::new(
            -rotation.sin() * 0.02,
            f64::from(0.2_f32),
            -rotation.cos() * 0.02,
        );
        entity
    }

    /// `getDimensions` 对应的碰撞盒。
    pub fn bounding_box(&self) -> Aabb {
        self.make_bounding_box_at(self.position)
    }

    /// `blockPosition()`。
    pub fn block_position(&self) -> BlockPos {
        BlockPos::containing(self.position)
    }

    /// `getY(progress)`。
    pub fn y_at(&self, progress: f64) -> f64 {
        self.position.y + f64::from(self.kind.dimensions().1) * progress
    }

    /// `commonTick()`：`ServerLevel.tickNonPassenger` 在 `tick()` 之前调它。
    ///
    /// 两个实体都不走 `baseTick`，所以这里只剩记下旧位置和数 tick。`invulnerableTime`
    /// 和客户端插值都和这两个实体无关。
    pub(crate) fn common_tick(&mut self) {
        self.old_position = self.position;
        self.tick_count += 1;
    }

    /// 这个 tick 的 `tick()`。逻辑只看得到上下文接口，见 [`ctx`]。
    pub(crate) fn tick(&mut self, ctx: &mut dyn ctx::EntityCtx) {
        self.common_tick();
        match self.kind.clone() {
            EntityKind::FallingBlock(_) => falling::tick_falling_block(self, ctx),
            EntityKind::Tnt(data) => tnt::tick_tnt(self, data, ctx),
            EntityKind::Item(_) => Entity::tick_item(self, ctx),
        }
    }

    /// `isInWater()`。
    pub const fn is_in_water(&self) -> bool {
        self.in_water
    }

    /// `getRequiresPrecisePosition()`：上一 tick 撞到过东西，同步时必须发精确位置。
    pub const fn requires_precise_position(&self) -> bool {
        self.requires_precise_position
    }

    /// `FallingBlockEntity.setHurtsEntities`。`FallingBlock.falling(entity)` 钩子用它。
    pub fn set_hurts_entities(&mut self, damage_per_distance: f32, damage_max: i32) {
        if let EntityKind::FallingBlock(data) = &mut self.kind {
            data.set_hurts_entities(damage_per_distance, damage_max);
        }
    }

    /// `FallingBlockEntity.disableDrop`。刷子方块掉下去时不留掉落物。
    pub fn disable_drop(&mut self) {
        if let EntityKind::FallingBlock(data) = &mut self.kind {
            data.disable_drop();
        }
    }

    /// `PrimedTnt.setFuse`。连锁点燃的引信比默认的短。
    pub fn set_fuse(&mut self, fuse: i32) {
        if let EntityKind::Tnt(data) = &mut self.kind {
            data.fuse = fuse;
        }
    }

    /// `discard()`。
    pub const fn discard(&mut self) {
        self.removed = true;
    }

    /// `causeFallDamage`。这两个实体都不受伤，但下落方块会覆写它：铁砧摔下来可能裂开。
    /// TNT 用的是基类实现，它只扣实体自己的血，这里没有生命值，所以直接跳过。
    pub(crate) fn cause_fall_damage(&mut self, fall_distance: f64) -> bool {
        let EntityKind::FallingBlock(data) = &self.kind else {
            return false;
        };
        let data = *data;
        self.falling_block_cause_fall_damage(data, fall_distance)
    }
}

/// 一个世界里的全部实体。公开和私有数据分开存，见 [`store`]。
///
/// 和官方的 `EntityTickList` 一样：tick 期间新生成的实体进 `pending`，下一 tick 才开始 tick。
pub type Entities = EntityStore;

/// `Mth.floor` 的轴向分量。碰撞计算里到处都是。
pub(crate) fn axis_component(vector: Vec3, axis: Axis) -> f64 {
    match axis {
        Axis::X => vector.x,
        Axis::Y => vector.y,
        Axis::Z => vector.z,
    }
}

/// 把轴向分量换掉，官方的 `Vec3.with(axis, value)`。
pub(crate) fn with_axis(vector: Vec3, axis: Axis, value: f64) -> Vec3 {
    match axis {
        Axis::X => Vec3::new(value, vector.y, vector.z),
        Axis::Y => Vec3::new(vector.x, value, vector.z),
        Axis::Z => Vec3::new(vector.x, vector.y, value),
    }
}
