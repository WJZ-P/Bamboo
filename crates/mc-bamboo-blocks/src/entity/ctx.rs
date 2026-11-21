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

use mc_bamboo_core::{BlockPos, ChunkPos, Direction, LightLayer, Vec3};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    BlockGameRules, BlockHit, WorldBorder,
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
    /// `MovingPistonBlock.getCollisionShape`：这一格移动中活塞的方块实体算出的形状，
    /// 相对这一格的最小角。`None` 表示没有这样的方块实体，官方给 `Shapes.empty()`。
    ///
    /// `MovingPistonBlock.getCollisionShape`: the shape computed by the moving-piston block
    /// entity at this cell, relative to the cell's minimum corner. `None` means there is
    /// no such block entity, for which vanilla returns `Shapes.empty()`.
    fn moving_piston_shape(&self, pos: BlockPos) -> Option<Vec<Aabb>>;
    /// `ShulkerBoxBlock.getShape`：这一格潜影盒实体按开盖进度算出的碰撞形状。
    /// `None` 表示没有实体，官方这时给 `Shapes.block()`。
    fn shulker_box_shape(&self, pos: BlockPos) -> Option<Aabb>;
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
    /// `Level.getWorldBorder()`：实体碰撞要读的边界快照。默认是官方默认边界。
    ///
    /// `Level.getWorldBorder()`: the border snapshot entity collisions read.
    /// The default is the vanilla default border.
    fn world_border(&self) -> WorldBorder {
        WorldBorder::default()
    }

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

    /// 这个区块里的实体现在 tick 不 tick。默认都 tick。
    ///
    /// 算阶段靠这一份只读事实跳过不在实体 tick 范围的实体；它们仍在世界里，
    /// 别的实体照常查得到。和 [`crate::Level::chunk_ticks_entities`] 必须给出同样的答案：
    /// [`World`] 实现本方法时转发到后者，游戏层的只读视图以后也从同一个判断取值。
    ///
    /// Whether entities in this chunk tick now. All do by default. The compute phase uses this
    /// read-only fact to skip entities outside the entity-ticking range; they stay in the world
    /// and are still visible to others. This must agree with
    /// [`crate::Level::chunk_ticks_entities`], which [`World`] forwards to.
    fn chunk_ticks_entities(&self, _chunk: ChunkPos) -> bool {
        true
    }

    /// `getGameTime()`：岛的时钟。没有时钟的视图给 0。
    fn game_time(&self) -> i64 {
        0
    }

    /// 世界里的玩家，按官方 `ServerLevel.players()` 的顺序。生物的视线、诱惑、
    /// 消失判定都从这里读。默认没有玩家。
    ///
    /// The players, in `ServerLevel.players()` order. Default: none.
    fn for_each_player(&self, _visit: &mut dyn FnMut(&PlayerView)) {}

    /// `getSkyDarken()`：昼夜和天气让天空光暗了多少。
    fn sky_darken(&self) -> i32 {
        0
    }

    /// `getDifficulty().getId()`：0 和平到 3 困难。默认普通。
    ///
    /// `getDifficulty().getId()`, 0 peaceful through 3 hard. Normal by default.
    fn difficulty(&self) -> i32 {
        2
    }

    /// `canSeeSky(pos)`：天空光是 15 就看得见天。
    fn can_see_sky(&self, _pos: BlockPos) -> bool {
        false
    }

    /// `getMaxLocalRawBrightness(pos, skyDarken)`：方块光和减去天色后的天空光里亮的那个。
    fn max_local_raw_brightness(&self, _pos: BlockPos, _darken: i32) -> i32 {
        0
    }

    /// `Level.isRainingAt(pos)`。
    fn is_raining_at(&self, _pos: BlockPos) -> bool {
        false
    }

    /// `Level.isBrightOutside()`：主世界没有固定时间，天色暗于 4 才算夜里。
    ///
    /// `Level.isBrightOutside()`: the overworld has no fixed time, so night
    /// starts when `skyDarken` reaches 4.
    fn is_bright_outside(&self) -> bool {
        self.sky_darken() < 4
    }

    /// `EnvironmentAttributes.MONSTERS_BURN`：主世界按 `day` 时间线求值。
    fn monsters_burn(&self) -> bool {
        crate::sky::monsters_burn(self.clock_ticks())
    }

    /// `getOverworldClockTime()`：主世界时钟的整数刻。没有时钟的视图给 0。
    fn clock_ticks(&self) -> i64 {
        0
    }

    /// `getCurrentDifficultyAt(pos).getEffectiveDifficulty()`：区域难度。
    /// 默认把区块的 `InhabitedTime` 当 0。
    fn effective_difficulty(&self, _pos: BlockPos) -> f32 {
        let clock = self.clock_ticks();
        crate::sky::effective_difficulty(
            self.difficulty(),
            clock,
            0,
            crate::sky::moon_brightness(clock),
        )
    }

    /// `level.random.nextFloat()`。只有精确模式的世界能当场取世界随机数；
    /// 并行的只读视图给 0，取数的地方列在设计稿的性能模式差异里。
    fn level_random_float(&self) -> f32 {
        0.0
    }

    /// `level.random.nextBoolean()`，并行视图同上。
    fn level_random_boolean(&self) -> bool {
        false
    }

    /// 生物身上装备的 `hurtAndBreak(amount, entity, slot)`：耐久附魔用 `level.random`。
    /// 返回装备是不是坏了。并行的只读视图没有世界随机源，按没有耐久附魔算
    /// （带耐久附魔的装备扣耐久的次数和原版不同，是性能模式差异）。
    fn hurt_mob_equipment(
        &self,
        stack: &mut mc_bamboo_protocol::item::ItemStack,
        amount: i32,
    ) -> bool {
        crate::durability::hurt_equipment(stack, amount, &crate::random::LegacyRandom::new(0))
    }
}

/// 生物看得见的一个玩家。
///
/// A player as seen by mobs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerView {
    pub uid: EntityUid,
    pub uuid: Option<[u8; 16]>,
    pub position: Vec3,
    pub eye_height: f32,
    pub alive: bool,
    pub spectator: bool,
    pub creative: bool,
    /// `isDiscrete()`，也就是潜行。
    pub sneaking: bool,
    pub invisible: bool,
    /// `getArmorCoverPercentage()`，隐身时才用。
    pub armor_cover: f32,
    /// 主手和副手的物品 ID，空手是 0。
    pub main_hand: i32,
    pub off_hand: i32,
}

impl PlayerView {
    pub fn eye_y(&self) -> f64 {
        self.position.y + f64::from(self.eye_height)
    }

    pub fn eye_position(&self) -> Vec3 {
        Vec3::new(self.position.x, self.eye_y(), self.position.z)
    }
}

impl BlockView for World<'_> {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        World::block_state(self, pos)
    }

    fn moving_piston_shape(&self, pos: BlockPos) -> Option<Vec<Aabb>> {
        World::moving_piston_shape(self, pos)
    }

    fn shulker_box_shape(&self, pos: BlockPos) -> Option<Aabb> {
        World::shulker_box_shape(self, pos)
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

    fn world_border(&self) -> WorldBorder {
        World::world_border(self)
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

    fn chunk_ticks_entities(&self, chunk: ChunkPos) -> bool {
        self.level().chunk_ticks_entities(chunk)
    }

    fn game_time(&self) -> i64 {
        World::game_time(self)
    }

    fn for_each_player(&self, visit: &mut dyn FnMut(&PlayerView)) {
        self.level().for_each_player(visit);
    }

    fn sky_darken(&self) -> i32 {
        self.level().sky_darken()
    }

    fn difficulty(&self) -> i32 {
        self.level().difficulty_id()
    }

    fn can_see_sky(&self, pos: BlockPos) -> bool {
        self.level().can_see_sky(pos)
    }

    fn max_local_raw_brightness(&self, pos: BlockPos, darken: i32) -> i32 {
        self.brightness(LightLayer::Block, pos)
            .max(self.brightness(LightLayer::Sky, pos) - darken)
    }

    fn is_raining_at(&self, pos: BlockPos) -> bool {
        World::is_raining_at(self, pos)
    }

    fn is_bright_outside(&self) -> bool {
        self.level().is_bright_outside()
    }

    fn monsters_burn(&self) -> bool {
        self.level().monsters_burn()
    }

    fn clock_ticks(&self) -> i64 {
        self.level().clock_ticks()
    }

    fn effective_difficulty(&self, pos: BlockPos) -> f32 {
        let clock = self.level().clock_ticks();
        crate::sky::effective_difficulty(
            self.level().difficulty_id(),
            clock,
            self.level()
                .inhabited_time(ChunkPos::new(pos.x >> 4, pos.z >> 4)),
            crate::sky::moon_brightness(clock),
        )
    }

    fn level_random_float(&self) -> f32 {
        self.random().next_float()
    }

    fn level_random_boolean(&self) -> bool {
        self.random().next_boolean()
    }

    fn hurt_mob_equipment(
        &self,
        stack: &mut mc_bamboo_protocol::item::ItemStack,
        amount: i32,
    ) -> bool {
        crate::durability::hurt_equipment(stack, amount, self.random())
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
    /// 生物打一名玩家（官方 `Mob.doHurtTarget` 里的 `target.hurtServer`），返回 `wasHurt`。
    ///
    /// 精确驱动当场落地，返回真实结果；并行驱动缓冲成 [`Command::HurtPlayer`]，
    /// 一律按打中算（性能模式的已知差异）。
    ///
    /// `attacker` 是打人的生物这一刻完整的公开数据（tick 期间 `me.kind` 被临时取走，
    /// 死亡消息要从这里拿名字和 UUID）。
    ///
    /// A mob hits a player. The precise driver applies it at once and returns the real
    /// `wasHurt`; the parallel driver buffers a command and reports a hit.
    fn hurt_player(
        &mut self,
        target: EntityUid,
        source: crate::entity::mob::DamageSource,
        amount: f32,
        me: &mut Entity,
        attacker: &EntityPublic,
    ) -> bool {
        let _ = attacker;
        self.emit(
            Command::HurtPlayer {
                target,
                source,
                amount,
            },
            me,
        );
        true
    }
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
/// 给以后生物和 AI 预留的命令名：`ChangeBlock`、`TriggerBlock`、
/// `Spawn`、`Remove`、`ScheduleTick`、`Depart`。这次不实现。
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    ShootMobArrow {
        position: Vec3,
        direction: Vec3,
        owner_uuid: [u8; 16],
        weapon: ItemStack,
        difficulty: i32,
    },
    ArrowImpact {
        block: Option<BlockHit>,
        hits: Vec<super::arrow::ArrowEntityHit>,
        end: Vec3,
    },
    /// TNT 引信归零：整段爆炸，包括射线、洗牌、逐格破坏、连锁点燃、掉落和伤害物品。
    Explode {
        center: Vec3,
        radius: f32,
    },
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
    MergeWithNeighbours {
        others: Vec<EntityUid>,
    },
    /// 漏斗吸取：跑 `HopperBlock.entityInside` 的整段，写容器也写自己。
    SuckIntoHopper,
    /// `Entity.spawnAtLocation`：生成一件掉落物。
    DropItem {
        stack: ItemStack,
    },
    /// `LivingEntity.pushEntities` 里的一次 `doPush`：对方被推开，自己被推回来。
    /// 落地时按双方当前的位置重算，`from` 是算阶段看到的对方位置。
    PushMob {
        other: EntityUid,
        from: Vec3,
        self_pushable: bool,
    },
    /// 死亡战利品表，与装备和经验分开，保留原版的生成顺序。
    ///
    /// Death loot table, separate from equipment and XP to preserve vanilla order.
    DropMobLoot(super::mob::MobDeathLoot),
    AwardDeathExperience {
        amount: i32,
    },
    /// `ExperienceOrb.scanForMerges`：周围同值的球合并到自己身上。
    MergeExperienceOrbs {
        others: Vec<EntityUid>,
        value: i32,
    },
    /// 给伴侣设冷却，生成幼崽和经验球；发起繁殖的亲本已进入冷却。
    ///
    /// Reset the other parent and spawn offspring and XP; the initiating parent is on cooldown.
    BreedAnimals {
        kind: super::mob::MobType,
        variant: super::mob::CowVariant,
        wool: u8,
        partner: EntityUid,
        position: mc_bamboo_core::Vec3,
    },
    /// `Mob.doHurtTarget` 打到玩家：本刻实体循环内按玩家 `hurtServer` 的正式入口落地。
    HurtPlayer {
        target: EntityUid,
        source: super::mob::DamageSource,
        amount: f32,
    },
    /// `Zombie.doHurtTarget` 的着火传染（打到玩家）。
    IgnitePlayer {
        target: EntityUid,
        ticks: i32,
    },
    /// `Mob.doHurtTarget` 打到另一头生物。
    HurtMob {
        target: EntityUid,
        source: super::mob::DamageSource,
        amount: f32,
    },
    /// `BreakDoorGoal` / `RemoveBlockGoal` 的 `levelEvent(event, pos, data)`。
    MobLevelEvent {
        event: i32,
        pos: mc_bamboo_core::BlockPos,
        data: i32,
    },
    /// `level.removeBlock(pos, false)`：`BreakDoorGoal` 破门、`RemoveBlockGoal` 啄碎海龟蛋。
    /// 只换成这一格的流体或空气，`2001` 之类的事件由调用方另发。
    RemoveMobBlock {
        pos: mc_bamboo_core::BlockPos,
    },
    /// `TurtleEggBlock.destroyEgg` 里取世界随机数之后的部分：`nextInt(chance) == 0`
    /// 时 `decreaseEggs`。世界随机数只在落地时取，两种驱动取数顺序一样。
    TrampleTurtleEgg {
        pos: mc_bamboo_core::BlockPos,
        chance: i32,
    },
    /// `ZombieAttackTurtleEggGoal.playBreakSound`：音高取世界随机数。
    TurtleEggBreakSound {
        pos: mc_bamboo_core::BlockPos,
    },
    /// `level.playSound(null, pos, sound, source, volume, pitch)`：方块中心的音效。
    MobBlockSound {
        pos: mc_bamboo_core::BlockPos,
        sound: &'static str,
        source: mc_bamboo_data::sounds::SoundSource,
        volume: f32,
        pitch: f32,
    },
    ArrowExtinguished {
        pitch: f32,
    },
    /// `EatBlockGoal.tick` 在 `mobGriefing` 下的改方块：吃掉脚下的草，或把下面的草方块变成泥土。
    ///
    /// `EatBlockGoal.tick` under `mobGriefing`: eat the plant at the feet, or turn the grass
    /// block below into dirt.
    EatBlock {
        pos: mc_bamboo_core::BlockPos,
        grass_below: bool,
    },
    /// `dropFromGiftLootTable(.., spawnAtLocation)`：鸡下蛋这类礼物表，掉在原点。
    ///
    /// `dropFromGiftLootTable(.., spawnAtLocation)`: gift tables such as egg laying, dropped at
    /// the origin.
    DropEntityLoot(super::mob::EntityLootRequest),
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
        Command::ShootMobArrow {
            position,
            direction,
            owner_uuid,
            weapon,
            difficulty,
        } => {
            super::mob::skeleton::spawn_arrow(
                world, position, direction, me.id, owner_uuid, weapon, difficulty,
            );
        }
        Command::ArrowImpact { block, hits, end } => {
            super::arrow::impact(me, world, hits, block, end)
        }
        Command::SuckIntoHopper => crate::hopper::suck_touching_item(world, me),
        Command::DropItem { stack } => {
            world.spawn_at_location(me.position, stack);
        }
        Command::PushMob {
            other,
            from,
            self_pushable,
        } => super::mob::living::apply_push(me, other, from, self_pushable, world),
        Command::DropMobLoot(loot) => {
            for stack in world.mob_loot_drops(loot, me.position) {
                world.spawn_at_location(me.position, stack);
            }
        }
        Command::AwardDeathExperience { amount } => {
            super::orb::award(world, me.position, amount);
        }
        Command::MergeExperienceOrbs { others, value } => {
            super::orb::apply_merge(me, &others, value, world)
        }
        Command::BreedAnimals {
            kind,
            variant,
            wool,
            partner,
            position,
        } => super::mob::living::apply_breed(me, partner, position, world, kind, variant, wool),
        Command::HurtPlayer {
            target,
            source,
            amount,
        } => {
            super::mob::living::apply_hurt_player(world, me, target, source, amount);
        }
        Command::IgnitePlayer { target, ticks } => world.ignite_player_from_mob(target, ticks),
        Command::HurtMob {
            target,
            source,
            amount,
        } => {
            let _ = world.hurt_mob(target, source, amount);
        }
        Command::MobLevelEvent { event, pos, data } => {
            world.push_effect(crate::Effect::LevelEvent {
                event,
                pos,
                data,
                except_initiator: false,
            });
        }
        Command::RemoveMobBlock { pos } => {
            let _ = world.remove_block(pos);
        }
        Command::DropEntityLoot(request) => {
            let origin = request.origin;
            for stack in world.entity_loot_drops(request) {
                world.spawn_at_location(origin, stack);
            }
        }
        Command::EatBlock { pos, grass_below } => {
            super::mob::living::apply_eat_block(world, pos, grass_below);
        }
        Command::TrampleTurtleEgg { pos, chance } => {
            super::mob::living::apply_trample_turtle_egg(world, pos, chance);
        }
        Command::TurtleEggBreakSound { pos } => {
            let pitch = 0.9 + world.random().next_float() * 0.2;
            world.play_block_sound(
                pos,
                mc_bamboo_data::sounds::sound_event("minecraft:entity.turtle.egg_break"),
                0.7,
                pitch,
            );
        }
        Command::ArrowExtinguished { pitch } => {
            super::arrow::play_extinguished_sound(me, world, pitch);
        }
        Command::MobBlockSound {
            pos,
            sound,
            source,
            volume,
            pitch,
        } => {
            world.play_sound(
                crate::world::block_center_public(pos),
                mc_bamboo_data::sounds::sound_event(sound),
                source,
                volume,
                pitch,
                false,
            );
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

    fn hurt_player(
        &mut self,
        target: EntityUid,
        source: crate::entity::mob::DamageSource,
        amount: f32,
        _me: &mut Entity,
        attacker: &EntityPublic,
    ) -> bool {
        // 打人的生物正被驱动拿在手里，不在表里；把它这一刻的公开数据带过去。
        self.world
            .hurt_player_from_mob(target, source, amount, Some(attacker))
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
