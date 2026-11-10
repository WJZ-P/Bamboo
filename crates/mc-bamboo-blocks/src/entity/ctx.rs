//! 实体的一刻：对一个上下文接口的函数，配两种驱动。
//!
//! 见 `docs/world-model-design.md` 第 3 节和第 8 节。实体逻辑不直接拿世界的 `&mut`：
//! 只读方块走 [`BlockView`]，读写自己走显式的 `me: &mut Entity`，对世界和别的实体的
//! 影响走 [`EntityCtx::emit`]。
//!
//! `me` 是驱动把存储里的 [`EntityRow`] 拼出来的完整视图。没有把它放进 `EntityCtx` 的
//! `me()` / `me_mut()` 方法里：物理代码要同时读写 `me`、读 `ctx.blocks()`、再调
//! `ctx.emit()`，把自己的引用挂到 `ctx` 上会让 `&mut ctx` 和 `me` 的借用纠缠在一起。
//! `me` 作为显式的 tick 参数，借用检查器能看到它和 `ctx` 不相交，也不需要 `unsafe`。
//! `ctx.blocks()` 只在单次调用的期间借用 `ctx`，不能跨过 `emit`。
//!
//! 两种驱动共用同一个 [`apply`]：精确模式在 `emit` 里当场调用它，并行模式把命令
//! 缓冲起来，落地阶段按顺序调用。**tick 函数不能根据 `emit` 之后自己的状态分支**；
//! 「看结果再决定」的逻辑全在 [`apply`] 里，那里能直接改发出者。

use mc_bamboo_core::{BlockPos, Direction, LightLayer, Vec3};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    BlockGameRules, BlockHit,
    shape::{Aabb, CollisionContext},
    state::{BlockState, FluidState, SupportType},
    world::World,
};

use super::{Entity, EntityPublic, EntityUid, FallingBlock};

/// 只读的方块/碰撞视图。实体逻辑和寻路只经它读世界；并行阶段共享同一份只读快照。
///
/// 方法名和 [`World`] 的只读方法一致，实体逻辑改参数类型时函数体不用改。
pub trait BlockView {
    fn block_state(&self, pos: BlockPos) -> BlockState;
    fn fluid_state(&self, pos: BlockPos) -> FluidState;
    fn block_collisions(&self, box_: Aabb, context: CollisionContext)
    -> Vec<(BlockPos, Vec<Aabb>)>;
    fn find_supporting_block(
        &self,
        position: Vec3,
        box_: Aabb,
        context: CollisionContext,
    ) -> Option<BlockPos>;
    fn clip_fall_damage_resetting(&self, from: Vec3, to: Vec3) -> bool;
    fn clip_collider_source_water(
        &self,
        from: Vec3,
        to: Vec3,
        context: CollisionContext,
    ) -> Option<BlockHit>;
    fn clip_collider(&self, from: Vec3, to: Vec3, context: CollisionContext) -> Option<BlockHit>;
    fn is_face_sturdy(&self, pos: BlockPos, face: Direction, support: SupportType) -> bool;
    fn is_loaded(&self, pos: BlockPos) -> bool;
    fn is_in_world_bounds(&self, pos: BlockPos) -> bool;
    fn min_y(&self) -> i32;
    fn max_y(&self) -> i32;
    fn brightness(&self, layer: LightLayer, pos: BlockPos) -> i32;
    fn block_game_rules(&self) -> BlockGameRules;
    /// `FlowingFluid` 的流体顶面高度。
    fn fluid_height(&self, fluid: FluidState, pos: BlockPos) -> f32;

    /// 一次读一片方块状态，默认逐格调 [`BlockView::block_state`]。
    ///
    /// 以后碰撞扫掠和寻路的热循环用它省掉逐格虚调用；这次物理代码先不改。
    fn read_states(&self, min: BlockPos, max: BlockPos, out: &mut Vec<BlockState>) {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                for x in min.x..=max.x {
                    out.push(self.block_state(BlockPos::new(x, y, z)));
                }
            }
        }
    }

    /// 这片格子里有没有漏斗方块。物品只在这时发 `SuckIntoHopper`：漏斗不存在时
    /// 那条命令的整个处理函数什么也不会做，省掉落地阶段的虚调用和扫描。
    fn any_hopper_in(&self, min: BlockPos, max: BlockPos) -> bool;
}

impl BlockView for World<'_> {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        World::block_state(self, pos)
    }

    fn fluid_state(&self, pos: BlockPos) -> FluidState {
        World::fluid_state(self, pos)
    }

    fn block_collisions(
        &self,
        box_: Aabb,
        context: CollisionContext,
    ) -> Vec<(BlockPos, Vec<Aabb>)> {
        World::block_collisions(self, box_, context)
    }

    fn find_supporting_block(
        &self,
        position: Vec3,
        box_: Aabb,
        context: CollisionContext,
    ) -> Option<BlockPos> {
        World::find_supporting_block(self, position, box_, context)
    }

    fn clip_fall_damage_resetting(&self, from: Vec3, to: Vec3) -> bool {
        World::clip_fall_damage_resetting(self, from, to)
    }

    fn clip_collider_source_water(
        &self,
        from: Vec3,
        to: Vec3,
        context: CollisionContext,
    ) -> Option<BlockHit> {
        World::clip_collider_source_water(self, from, to, context)
    }

    fn clip_collider(&self, from: Vec3, to: Vec3, context: CollisionContext) -> Option<BlockHit> {
        World::clip_collider(self, from, to, context)
    }

    fn is_face_sturdy(&self, pos: BlockPos, face: Direction, support: SupportType) -> bool {
        World::is_face_sturdy(self, pos, face, support)
    }

    fn is_loaded(&self, pos: BlockPos) -> bool {
        World::is_loaded(self, pos)
    }

    fn is_in_world_bounds(&self, pos: BlockPos) -> bool {
        World::is_in_world_bounds(self, pos)
    }

    fn min_y(&self) -> i32 {
        World::min_y(self)
    }

    fn max_y(&self) -> i32 {
        World::max_y(self)
    }

    fn brightness(&self, layer: LightLayer, pos: BlockPos) -> i32 {
        World::brightness(self, layer, pos)
    }

    fn block_game_rules(&self) -> BlockGameRules {
        World::block_game_rules(self)
    }

    fn fluid_height(&self, fluid: FluidState, pos: BlockPos) -> f32 {
        crate::fluid::height(fluid, self, pos)
    }

    fn any_hopper_in(&self, min: BlockPos, max: BlockPos) -> bool {
        (min.y..=max.y).any(|y| {
            (min.z..=max.z).any(|z| {
                (min.x..=max.x).any(|x| {
                    self.block_state(BlockPos::new(x, y, z))
                        .is_a(mc_bamboo_data::BlockAncestor::HopperBlock)
                })
            })
        })
    }
}

/// 实体一刻能用到的全部上下文。精确和并行各有一个实现。
///
/// 读别的实体只能读到 [`EntityPublic`]，访问顺序按 uid 升序。世界随机源不在这里：
/// 意图命令在 [`apply`] 里跑完整逻辑，所有 `level.random` 取数都发生在那里，
/// 两种驱动的取数顺序因此相同。
pub trait EntityCtx {
    /// 当前 tick 的实体 ID。
    fn uid(&self) -> EntityUid;
    /// 只读的方块视图。调用方只在这个方法调用的期间借用，不能跨过 `emit`。
    fn blocks(&self) -> &dyn BlockView;
    /// 按 ID 读别的实体的公开数据。自己已经被驱动从表里取出来，查不到是正常的。
    fn other(&self, uid: EntityUid) -> Option<EntityPublic>;
    /// 这一刻参与 tick 的实体 ID，按 uid 升序。
    fn entity_uids(&self) -> Vec<EntityUid>;
    /// 范围查询，按 uid 升序访问。空间索引查出来之后要排序。
    fn for_each_other_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic));
    /// 范围查询里只需要 ID 时用这个：不复制公开数据，共享表里那一份。
    fn for_each_other_uid_in(&self, area: Aabb, visit: &mut dyn FnMut(EntityUid));
    /// 提交一条对外影响。精确模式当场落地，并行模式进缓冲。
    ///
    /// `me` 是发出者的完整视图；落地函数可以直接改它，tick 函数在 `emit` 之后
    /// 看到的就是改完的自己。tick 函数不能根据 `emit` 之后自己的状态分支。
    fn emit(&mut self, command: Command, me: &mut Entity);
}

/// 下落方块「算」阶段得到的落地判定事实，随命令带进落地阶段。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LandFacts {
    pub pos: BlockPos,
    pub current_state: BlockState,
    pub is_concrete: bool,
    pub stuck_in_water: bool,
}

/// 一条对外影响，按「意图」而不是底层操作划分。
///
/// 底层操作（写方块、发事件、生成实体、取世界随机数）都在 [`apply`] 的内部发生。
/// 给以后生物和 AI 预留的命令名：`ChangeBlock`、`TriggerBlock`、`Damage`、`Push`、
/// `Spawn`、`Remove`、`ScheduleTick`、`Depart`。这次不实现。
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// TNT 引信归零：整段爆炸，包括射线、洗牌、逐格破坏、连锁点燃、掉落和伤害物品。
    Explode { center: Vec3, radius: f32 },
    /// 下落方块这一刻的落地判定和后果：能不能放回去、发 1031 还是 1029、
    /// 要不要掉成物品、自己消不消失。需要 `&mut World` 的只读判定也在这里。
    ///
    /// 位置、当前状态、是否浸水这些是「算」阶段用自己这一刻的移动算出来的只读事实，
    /// 不是世界写入；带在命令里，落地阶段不用重算（混凝土粉末的水源射线要用旧位置）。
    LandFallingBlock {
        data: FallingBlock,
        entity_drops: bool,
        land: LandFacts,
    },
    /// 与附近的物品合并。候选 ID 按 uid 升序，落地时重新读双方当前状态。
    MergeWithNeighbours { others: Vec<EntityUid> },
    /// 漏斗吸取：跑 `HopperBlock.entityInside` 的整段，写容器也写自己。
    SuckIntoHopper,
    /// `Entity.spawnAtLocation`：生成一件掉落物。
    DropItem { stack: ItemStack },
}

/// 所有命令共用的落地入口。两种驱动调的是同一个函数。
///
/// `me` 是发出者，`world` 有整个 region 的写权限。处理函数直接改世界、改别的实体、
/// 改发出者，没有任何回写补丁。
pub fn apply(command: Command, me: &mut Entity, world: &mut World) {
    match command {
        Command::Explode { center, radius } => world.explode_tnt(center, radius),
        Command::LandFallingBlock {
            data,
            entity_drops,
            land,
        } => {
            super::falling::land_falling_block(me, data, entity_drops, land, world);
        }
        Command::MergeWithNeighbours { others } => {
            Entity::apply_merge_with_neighbours(me, &others, world);
        }
        Command::SuckIntoHopper => crate::hopper::suck_touching_item(world, me),
        Command::DropItem { stack } => {
            world.spawn_at_location(me.position, stack);
        }
    }
}

/// 精确模式的上下文：读当前世界，`emit` 立刻生效。
///
/// 和并行模式一样，实体逻辑只看得到 [`EntityCtx`]；差别只在 `emit` 的实现。
pub struct PreciseCtx<'w, 'a> {
    world: &'w mut World<'a>,
    uid: EntityUid,
}

impl<'w, 'a> PreciseCtx<'w, 'a> {
    pub fn new(world: &'w mut World<'a>, uid: EntityUid) -> Self {
        Self { world, uid }
    }

    /// 取回世界，给驱动在 tick 之间用。
    pub fn into_world(self) -> &'w mut World<'a> {
        self.world
    }
}

impl EntityCtx for PreciseCtx<'_, '_> {
    fn uid(&self) -> EntityUid {
        self.uid
    }

    fn blocks(&self) -> &dyn BlockView {
        &*self.world
    }

    fn other(&self, uid: EntityUid) -> Option<EntityPublic> {
        self.world.level().entity_public(uid)
    }

    fn entity_uids(&self) -> Vec<EntityUid> {
        self.world.level().entity_uids()
    }

    fn for_each_other_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic)) {
        self.world.level().for_each_entity_in(area, visit);
    }

    fn for_each_other_uid_in(&self, area: Aabb, visit: &mut dyn FnMut(EntityUid)) {
        self.world
            .level()
            .for_each_entity_in(area, &mut |public| visit(public.uid));
    }

    fn emit(&mut self, command: Command, me: &mut Entity) {
        apply(command, me, self.world);
    }
}

/// 并行模式的命令缓冲：每个实体任务一份，落地阶段按（实体顺序，实体内序号）串起来。
#[derive(Debug, Default)]
pub struct CommandBuffer {
    pub commands: Vec<Command>,
}

impl CommandBuffer {
    pub fn push(&mut self, command: Command) {
        self.commands.push(command);
    }
}
