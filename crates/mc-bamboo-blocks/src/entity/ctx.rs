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

use super::{
    Entity, EntityPublic, EntityUid, FallingBlock,
    lightning::LightningTick,
    mob::{skeleton_horse::TrapSpawn, throttle},
};

/// 只读的方块/碰撞视图。实体逻辑和寻路只经它读世界；并行阶段共享同一份只读快照。
///
/// 方法名和 [`World`] 的只读方法一致，实体逻辑改参数类型时函数体不用改。
pub trait BlockView {
    fn block_state(&self, pos: BlockPos) -> BlockState;
    /// `Level.getBestOwnOrNeighbourSignal(pos)`：`SulfurCube.primeWhenOnPoweredPosition`
    /// 读它判断脚下有没有红石信号。默认没有信号。
    ///
    /// `Level.getBestOwnOrNeighbourSignal(pos)`, read by the sulfur cube's
    /// powered-position prime check. Defaults to zero.
    fn best_own_or_neighbour_signal(&self, _pos: BlockPos) -> i32 {
        0
    }
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
    /// 岛级的方块变更版本戳：方块或影响射线形状的方块实体一变就换一个值。
    /// 没有版本戳的视图给 0（这时缓存不会命中，行为等同于没有缓存）。
    ///
    /// Island-level block-change stamp: changes whenever a block or a shape-affecting
    /// block entity changes. Views without a stamp return 0 (the cache then never
    /// hits, which is equivalent to no cache).
    fn block_change_stamp(&self) -> u64 {
        0
    }
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

    /// 这一格有没有活的蜂巢方块实体，以及它是不是已满（3 只）。
    /// 并行只读视图没有方块实体表，按「存在且未满」回答。
    fn beehive_full(&self, _pos: BlockPos) -> bool {
        false
    }

    /// 这一格有没有方块实体表条目。`World` 转发到 `Level`。
    fn has_block_entity(&self, _pos: BlockPos) -> bool {
        false
    }

    /// 官方 `PoiManager.getInRange(#bee_home, center, radius, ANY)` 的全部位置，
    /// 保持官方遍历序；有 POI 数据的视图覆盖它，没有的默认返回空。
    fn beehives_near(&self, _center: BlockPos, _radius: i32) -> Vec<BlockPos> {
        Vec::new()
    }

    /// 官方 `EnvironmentAttributeSystem.getValue(BEES_STAY_IN_HIVE, pos)`。
    ///
    /// 主世界按时间线的两条关键帧给值；`RAIN` / `THUNDER` 两张天气覆盖层在雨强
    /// 或雷暴强度大于 0 时把它设为真。别的维度没有这套时间线，按缺省假处理。
    ///
    /// Vanilla `EnvironmentAttributeSystem.getValue(BEES_STAY_IN_HIVE, pos)`: the
    /// overworld timeline contributes the two keyframes, and the rain/thunder weather
    /// layers force it true while their intensity is positive.
    fn bees_stay_in_hive(&self, _pos: BlockPos) -> bool {
        false
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

    /// 一个实体（按 UUID 找它的计分板名字）所在的队伍，和 [`PlayerView::team`]
    /// 用同一套下标。默认谁都不在队伍里。
    ///
    /// The team of an entity looked up by UUID, indexed like [`PlayerView::team`].
    fn entity_team(&self, _uuid: &[u8; 16]) -> Option<u32> {
        None
    }

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

    /// `Level.getSeaLevel()`：生成器的海平面。默认是噪声主世界的 63；超平坦是 -63，
    /// 由游戏层的只读视图按世界类型给出。
    ///
    /// `Level.getSeaLevel()`: the generator's sea level. Defaults to the noise
    /// overworld's 63; superflat is -63, which the game's read-only view supplies.
    fn sea_level(&self) -> i32 {
        63
    }

    /// `canSeeSky(pos)`：天空光是 15 就看得见天。
    fn can_see_sky(&self, _pos: BlockPos) -> bool {
        false
    }

    /// `getHeightmapPos(MOTION_BLOCKING, pos).getY()`：这一列挡运动的最高方块上面一格，整列
    /// 都不挡时是世界底部下面一格。缺省实现逐格读方块；有真实高度图的视图覆盖它。
    ///
    /// `getHeightmapPos(MOTION_BLOCKING, pos).getY()`: one above the highest
    /// motion-blocking block of the column, or one below the world bottom when nothing
    /// blocks. The default reads blocks one by one; views with a real heightmap override it.
    fn motion_blocking_y(&self, pos: BlockPos) -> i32 {
        for y in (self.min_y()..=self.max_y()).rev() {
            let state = self.block_state(BlockPos::new(pos.x, y, pos.z));
            if state.is_tagged(mc_bamboo_data::tags::block_tags::BLOCKS_MOTION_IN_HEIGHTMAP)
                || !state.fluid().is_empty()
            {
                return y + 1;
            }
        }
        self.min_y() - 1
    }

    /// `getMaxLocalRawBrightness(pos, skyDarken)`：方块光和减去天色后的天空光里亮的那个。
    fn max_local_raw_brightness(&self, _pos: BlockPos, _darken: i32) -> i32 {
        0
    }

    /// `Level.isRaining()`：全局下雨（蜜蜂授粉目标按它停）。
    ///
    /// `Level.isRaining()`: global rain (stops bee pollination).
    fn is_raining(&self) -> bool {
        false
    }

    /// `Level.isRainingAt(pos)`。
    fn is_raining_at(&self, _pos: BlockPos) -> bool {
        false
    }

    /// `Level.isThundering()`：担心基因的熊猫在雷雨天会坐下。
    ///
    /// `Level.isThundering()`: a worried panda sits down during a thunderstorm.
    fn is_thundering(&self) -> bool {
        false
    }

    /// `EnvironmentAttributes.SNOW_GOLEM_MELTS`：热群系和下界为真。
    /// 没有群系表的视图按主世界缺省（假）。
    ///
    /// `EnvironmentAttributes.SNOW_GOLEM_MELTS`: true in hot biomes and the Nether.
    /// Views without biome data use the overworld default (false).
    fn snow_golem_melts(&self, _pos: BlockPos) -> bool {
        false
    }

    /// `SnowLayerBlock.canSurvive`：脚下不能托住一层雪时返回假。
    /// 默认按方块表里的碰撞形状判断，带动态方块实体的视图可覆盖。
    ///
    /// `SnowLayerBlock.canSurvive`: false when the block below cannot support
    /// a snow layer. The default reads the generated collision shape table.
    fn snow_layer_can_survive(&self, pos: BlockPos) -> bool {
        use mc_bamboo_core::Direction;
        let below_pos = pos.below();
        let below = self.block_state(below_pos);
        if below.is_tagged(mc_bamboo_data::tags::block_tags::CANNOT_SUPPORT_SNOW_LAYER) {
            return false;
        }
        if below.is_tagged(mc_bamboo_data::tags::block_tags::SUPPORT_OVERRIDE_SNOW_LAYER) {
            return true;
        }
        let boxes =
            crate::shape::collision_boxes(below, below_pos, crate::shape::CollisionContext::Empty);
        crate::shape::is_face_full(&boxes, Direction::Up)
            || below.is(mc_bamboo_data::blocks::SNOW) && below.get(crate::state::LAYERS) == 8
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

    /// `EnvironmentAttributes.VILLAGER_ACTIVITY`：成年村民当前日程活动。
    ///
    /// `EnvironmentAttributes.VILLAGER_ACTIVITY`: current scheduled activity for adult villagers.
    fn villager_activity(&self) -> crate::entity::mob::brain::Activity {
        crate::sky::villager_activity(self.clock_ticks())
    }

    /// `EnvironmentAttributes.BABY_VILLAGER_ACTIVITY`：幼年村民当前日程活动。
    ///
    /// `EnvironmentAttributes.BABY_VILLAGER_ACTIVITY`: current scheduled activity for baby villagers.
    fn baby_villager_activity(&self) -> crate::entity::mob::brain::Activity {
        crate::sky::baby_villager_activity(self.clock_ticks())
    }

    /// POI 查询族，和 [`crate::Level`] 上的同名方法给同一份答案
    /// （精确 / 并行驱动不分叉）。默认没有 POI 数据。
    fn poi_type_at(&self, _pos: BlockPos) -> Option<u8> {
        None
    }

    /// `occupancy`：0 = HAS_SPACE，1 = IS_OCCUPIED，2 = ANY。
    fn poi_count_in_range(
        &self,
        _center: BlockPos,
        _radius: i32,
        _kind: u8,
        _occupancy: u8,
    ) -> usize {
        0
    }

    fn poi_find_closest(
        &self,
        _center: BlockPos,
        _radius: i32,
        _kind: u8,
        _occupancy: u8,
        _position_filter: &dyn Fn(BlockPos) -> bool,
    ) -> Option<BlockPos> {
        None
    }

    /// `PoiManager.getInRange`：按官方遍历序返回半径内匹配类型位掩码与占用口径的全部 `(kind, pos)`。
    ///
    /// `PoiManager.getInRange`: all `(kind, pos)` within `radius` matching `type_mask`
    /// and `occupancy`, in vanilla traversal order.
    fn poi_in_range(
        &self,
        _center: BlockPos,
        _radius: i32,
        _type_mask: u32,
        _occupancy: u8,
    ) -> Vec<(u8, BlockPos)> {
        Vec::new()
    }

    fn poi_sections_to_village(&self, _pos: BlockPos) -> i32 {
        7
    }

    fn poi_is_close_to_village(&self, _pos: BlockPos, _sections: i32) -> bool {
        false
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

    /// `level.random.nextInt(bound)`：大脑行为的时长、偶尔看人的间隔用它。并行视图同上给 0。
    ///
    /// `level.random.nextInt(bound)`, for brain behaviour durations and look intervals;
    /// 0 on parallel views as above.
    fn level_random_int(&self, _bound: i32) -> i32 {
        0
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

/// 生物选药或决策时要看的目标药水效果（女巫的 `performRangedAttack` 读这三项）。
///
/// Target potion effects inspected by mob AI (`Witch.performRangedAttack` reads these three).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TargetEffects {
    pub slowness: bool,
    pub poison: bool,
    pub weakness: bool,
}

impl TargetEffects {
    /// 从效果表提取这三项标志。
    ///
    /// Extracts the three flags from an active effect map.
    pub fn from_map(effects: &super::mob::effects::EffectMap) -> Self {
        use mc_bamboo_data::registries::mob_effect;
        Self {
            slowness: effects.has(mob_effect::SLOWNESS),
            poison: effects.has(mob_effect::POISON),
            weakness: effects.has(mob_effect::WEAKNESS),
        }
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
    /// `Entity.getDeltaMovement()`，女巫计算投掷提前量时读它。
    ///
    /// `Entity.getDeltaMovement()`, read by witches to lead thrown potions.
    pub delta: Vec3,
    pub eye_height: f32,
    /// `LivingEntity.getHealth()`，女巫选毒药还是伤害药水时读它。
    ///
    /// `LivingEntity.getHealth()`, read by witches when picking poison vs harming.
    pub health: f32,
    pub look_direction: Vec3,
    pub gaze_disguise: bool,
    pub alive: bool,
    pub spectator: bool,
    pub creative: bool,
    /// 官方 `GameType` 的 ID：0 生存、1 创造、2 冒险、3 旁观。
    /// `isSurvival()` 为真的是生存（0）和冒险（2）。
    ///
    /// Vanilla `GameType` id: 0 survival, 1 creative, 2 adventure, 3 spectator.
    /// `isSurvival()` holds for survival (0) and adventure (2).
    pub game_mode: i32,
    /// 身上的挖掘疲劳 `(等级, 剩余刻数)`；没有该效果时是 `None`。
    ///
    /// The active mining fatigue `(amplifier, remaining ticks)`; `None` when absent.
    pub mining_fatigue: Option<(i32, i32)>,
    /// `isDiscrete()`，也就是潜行。
    pub sneaking: bool,
    /// `isSprinting()`：犰狳会被疾跑的玩家吓到。
    ///
    /// `isSprinting()`; sprinting players scare armadillos.
    pub sprinting: bool,
    /// `Entity.isSwimming()`（共享标志位 4）：海豚会跟着游泳的玩家游。
    ///
    /// `Entity.isSwimming()` (shared flag 4); dolphins swim with a swimming player.
    pub swimming: bool,
    pub invisible: bool,
    /// `getArmorCoverPercentage()`，隐身时才用。
    pub armor_cover: f32,
    /// 主手和副手的物品 ID，空手是 0。
    pub main_hand: i32,
    pub off_hand: i32,
    /// 所在队伍（`getTeam()`）在计分板队伍表里的下标，同一刻的快照里一队一个值；
    /// 不在队伍里是 `None`。只用来判断和生物是不是同队。
    pub team: Option<u32>,
    /// 朝向，骑乘时用作骑手的朝向（`AbstractHorse.getRiddenRotation`）。
    ///
    /// The facing, used as the rider's rotation while riding
    /// (`AbstractHorse.getRiddenRotation`).
    pub yaw: f32,
    pub pitch: f32,
    /// 战斗记忆，被驯服的狼保护主人时读。
    ///
    /// Combat memory, read by tamed wolves guarding their owner.
    pub combat: PlayerCombat,
    /// 目标身上的缓慢 / 中毒 / 虚弱状态，女巫选药时读它。
    ///
    /// Whether the target has slowness, poison or weakness, read by witches.
    pub effects: TargetEffects,
}

/// 玩家的 `getLastHurtByMob` / `getLastHurtMob` 和两个时间戳（玩家自己的 tick 数），
/// 以及 `getLastDamageSource(100)` 存在且不在 `#no_wolf_retaliation` 里。
///
/// A player's `getLastHurtByMob` / `getLastHurtMob` with their timestamps (the
/// player's own tick count), and whether `getLastDamageSource(100)` exists and
/// is outside `#no_wolf_retaliation`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerCombat {
    pub last_hurt_by_mob: Option<EntityUid>,
    pub last_hurt_by_mob_timestamp: i32,
    pub last_hurt_mob: Option<EntityUid>,
    pub last_hurt_mob_timestamp: i32,
    pub retaliation_damage: bool,
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

    fn best_own_or_neighbour_signal(&self, pos: BlockPos) -> i32 {
        World::best_own_or_neighbour_signal(self, pos)
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

    fn block_change_stamp(&self) -> u64 {
        self.level().block_change_stamp()
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

    fn sea_level(&self) -> i32 {
        World::sea_level(self)
    }

    fn motion_blocking_y(&self, pos: BlockPos) -> i32 {
        self.level().heightmap_y(pos)
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

    fn poi_type_at(&self, pos: BlockPos) -> Option<u8> {
        self.level().poi_type_at(pos)
    }

    fn poi_count_in_range(&self, center: BlockPos, radius: i32, kind: u8, occupancy: u8) -> usize {
        self.level()
            .poi_count_in_range(center, radius, kind, occupancy)
    }

    fn poi_find_closest(
        &self,
        center: BlockPos,
        radius: i32,
        kind: u8,
        occupancy: u8,
        position_filter: &dyn Fn(BlockPos) -> bool,
    ) -> Option<BlockPos> {
        self.level()
            .poi_find_closest(center, radius, kind, occupancy, position_filter)
    }

    fn poi_in_range(
        &self,
        center: BlockPos,
        radius: i32,
        type_mask: u32,
        occupancy: u8,
    ) -> Vec<(u8, BlockPos)> {
        self.level()
            .poi_in_range(center, radius, type_mask, occupancy)
    }

    fn poi_sections_to_village(&self, pos: BlockPos) -> i32 {
        self.level().poi_sections_to_village(pos)
    }

    fn poi_is_close_to_village(&self, pos: BlockPos, sections: i32) -> bool {
        self.level().poi_is_close_to_village(pos, sections)
    }

    fn chunk_ticks_entities(&self, chunk: ChunkPos) -> bool {
        self.level().chunk_ticks_entities(chunk)
    }

    fn game_time(&self) -> i64 {
        World::game_time(self)
    }

    fn beehives_near(&self, center: BlockPos, radius: i32) -> Vec<BlockPos> {
        World::beehives_near(self, center, radius)
    }

    fn beehive_full(&self, pos: BlockPos) -> bool {
        self.block_entity::<crate::behaviour::machines::beehive_entity::BeehiveEntity>(pos)
            .is_some_and(|hive| hive.is_full())
    }

    fn has_block_entity(&self, pos: BlockPos) -> bool {
        World::has_block_entity(self, pos)
    }

    fn bees_stay_in_hive(&self, pos: BlockPos) -> bool {
        World::bees_stay_in_hive(self, pos)
    }

    fn for_each_player(&self, visit: &mut dyn FnMut(&PlayerView)) {
        self.level().for_each_player(visit);
    }

    fn entity_team(&self, uuid: &[u8; 16]) -> Option<u32> {
        self.level().entity_team(uuid)
    }

    fn sky_darken(&self) -> i32 {
        self.level().sky_darken()
    }

    fn villager_activity(&self) -> crate::entity::mob::brain::Activity {
        self.level().villager_activity()
    }

    fn baby_villager_activity(&self) -> crate::entity::mob::brain::Activity {
        self.level().baby_villager_activity()
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

    fn is_raining(&self) -> bool {
        World::is_raining(self)
    }

    fn is_raining_at(&self, pos: BlockPos) -> bool {
        self.level().is_raining_at(pos)
    }

    fn is_thundering(&self) -> bool {
        World::is_thundering(self)
    }

    fn snow_golem_melts(&self, pos: BlockPos) -> bool {
        self.level().snow_golem_melts(pos)
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

    fn level_random_int(&self, bound: i32) -> i32 {
        self.random().next_int(bound)
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

/// 驱动在每刻开头按 uid 升序算好的降频计划。只由刻号、uid、距离和令牌桶决定，
/// 精确和并行两种驱动构造出同一份；豁免由实体自己的 tick 覆盖成全速档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThrottlePlan {
    pub tier: throttle::Tier,
    /// 这一刻决策（目标选择器、目标、大脑）跑不跑。
    pub decisions_run: bool,
    /// 距离上次决策运行过了几刻（至少 1）。全速档 / 精确模式下恒为 1；
    /// 降频 / 休眠档按这个间隔推进大脑的记忆过期与传感器倒计时。
    ///
    /// Ticks elapsed since the last decision run (at least 1). Always 1 in full-speed /
    /// precise mode; throttled / sleeping brains advance memory expiry and sensor
    /// countdowns by this gap.
    pub decision_gap: i64,
    /// 休眠档每 20 刻的完整 tick 里不主动走动（AI 停摆，物理照跑）。
    pub no_active_move: bool,
    /// 限量唤醒的有效期（绝对刻号）；不大于当前刻号表示没被唤醒。
    pub wake_until: i64,
}

impl ThrottlePlan {
    /// 开关关着、或名单里没有这个实体：官方完整 tick。
    pub const FULL: Self = Self {
        tier: throttle::Tier::Full,
        decisions_run: true,
        decision_gap: 1,
        no_active_move: false,
        wake_until: i64::MIN,
    };
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
    /// 驱动给这一刻这个实体算好的降频计划。缺省是完整 tick（精确模式）。
    fn throttle_plan(&self) -> ThrottlePlan {
        ThrottlePlan::FULL
    }
    /// 按 ID 读别的实体的公开数据。自己已经被驱动从表里取出来，查不到是正常的。
    fn other(&self, uid: EntityUid) -> Option<EntityPublic>;
    /// 借用别的实体的公开数据，找到时返回 `true`。热路径里用它省掉整份
    /// [`EntityPublic`] 的克隆。默认退回 [`Self::other`] 的克隆实现；精确和并行
    /// 两种真实驱动都覆盖成直接借用表里的那一行。
    ///
    /// Borrows another entity's public data; `true` when found. Hot paths use it to
    /// avoid cloning a whole [`EntityPublic`]. The default falls back to the cloning
    /// [`Self::other`]; both real drivers override it to borrow the stored row directly.
    fn with_other(&self, uid: EntityUid, visit: &mut dyn FnMut(&EntityPublic)) -> bool {
        match self.other(uid) {
            Some(public) => {
                visit(&public);
                true
            }
            None => false,
        }
    }
    /// 按 ID 读一名玩家（骑手）。不在本岛时返回 `None`。
    ///
    /// Reads one player (a rider) by id. `None` when they are not on this island.
    fn player(&self, uid: EntityUid) -> Option<PlayerView> {
        let mut found = None;
        self.blocks().for_each_player(&mut |player| {
            if player.uid == uid {
                found = Some(*player);
            }
        });
        found
    }

    /// 这一刻参与 tick 的实体 ID，按 uid 升序。
    fn entity_uids(&self) -> Vec<EntityUid>;
    /// 范围查询，按 uid 升序访问。空间索引查出来之后要排序。
    fn for_each_other_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic));
    /// 同 [`Self::for_each_other_in`]，但只访问 `types` 里那几种注册表实体类型；
    /// 顺序仍是 uid 升序，结果与全量扫描后过滤完全相同。
    ///
    /// Like [`Self::for_each_other_in`] but only the given registry entity types;
    /// still in ascending uid order, identical to filtering a full scan.
    fn for_each_other_of_types_in(
        &self,
        area: Aabb,
        types: &[i32],
        visit: &mut dyn FnMut(&EntityPublic),
    ) {
        self.for_each_other_in(area, &mut |public| {
            if types.contains(&public.kind.entity_type()) {
                visit(public);
            }
        });
    }
    /// 同 [`Self::for_each_other_in`]，但 `visit` 返回 `true` 才算一个够数的
    /// 结果：挤压上限下推到查询里，凑够 `limit` 个就停止枚举。
    ///
    /// Like [`Self::for_each_other_in`], but only `true` returns count towards
    /// `limit`; the push cap stops enumeration once it has enough candidates.
    fn pushable_others(
        &self,
        area: Aabb,
        limit: usize,
        visit: &mut dyn FnMut(&EntityPublic) -> bool,
    ) {
        let mut seen = 0;
        self.for_each_other_in(area, &mut |public| {
            if seen < limit && visit(public) {
                seen += 1;
            }
        });
    }
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
    /// 实体移动 / 死亡等发出的游戏事件。并行驱动攒到落地阶段按 uid 顺序投递。
    GameEvent {
        event: crate::game_event::GameEvent,
        position: Vec3,
        context: crate::game_event::dispatch::EventContext,
    },
    ShootMobArrow {
        position: Vec3,
        direction: Vec3,
        owner_uuid: [u8; 16],
        weapon: ItemStack,
        difficulty: i32,
        /// `Stray.getArrow` / `Bogged.getArrow` 加的效果：（效果 ID，时长）。
        ///
        /// The effect added by `Stray.getArrow` / `Bogged.getArrow`: (effect id, duration).
        effect: Option<(i32, i32)>,
    },
    ArrowImpact {
        block: Option<BlockHit>,
        hits: Vec<super::arrow::ArrowEntityHit>,
        end: Vec3,
    },
    /// `Llama.spit` 生成口水（`Projectile.spawnProjectileUsingShoot`）。
    ///
    /// `Llama.spit` spawns a spit (`Projectile.spawnProjectileUsingShoot`).
    ShootLlamaSpit(super::spit::SpitShot),
    /// `Drowned.performRangedAttack`：溺尸朝目标扔一支不可捡的三叉戟。
    ///
    /// `Drowned.performRangedAttack`: a drowned throws a non-pickupable trident.
    ShootTrident(super::arrow::TridentShot),
    /// `LlamaSpit.onHitEntity`：口水打到一个实体。
    ///
    /// `LlamaSpit.onHitEntity`: the spit hit an entity.
    SpitHit(super::spit::SpitHit),
    /// 鸡蛋这一刻撞到东西：`ThrownEgg.onHit`（打目标、孵小鸡、碎掉）。
    ///
    /// The egg hit something this tick: `ThrownEgg.onHit` (hurt, hatch, break).
    EggImpact {
        hit: super::thrown_egg::EggHit,
    },
    /// 玩家 / 雪傀儡 / 发射器扔出的一枚 `ThrowableItemProjectile`。
    ///
    /// A `ThrowableItemProjectile` thrown by a player, snow golem or dispenser.
    ThrowThrowable {
        shot: Box<super::thrown_egg::EggShot>,
    },
    /// 铁傀儡命中后把目标抛起来：`setDeltaMovement(add(0, 0.4 * (1 - resistance), 0))`。
    ///
    /// An iron golem's hit lifts the target by `0.4 * (1 - knockback resistance)`.
    LiftMob {
        target: EntityUid,
        lift: f64,
    },
    /// 雪傀儡脚下四角铺一层雪。
    ///
    /// A snow golem places a snow layer at the four cells under its feet.
    PlaceSnowTrail {
        cells: Vec<BlockPos>,
    },
    /// 冰霜行者 `replace_disk`：把算阶段选中的水面格逐格换成 `frosted_ice`（age 0）。
    ///
    /// Frost walker `replace_disk`: replace the selected water cells with `frosted_ice`.
    FreezeWater {
        cells: Vec<BlockPos>,
    },
    /// 灵魂疾行 `TICK` 的 `minecraft:soul` 粒子：服务端沿用既有粒子命令的做法，
    /// 消耗实体随机数但不单独发粒子包。
    ///
    /// Soul speed `TICK` soul particles: as with existing particle commands, consume
    /// the entity random draws on the server without emitting a separate packet.
    SoulSpeedParticles {
        position: Vec3,
    },
    /// 灵魂疾行 `TICK` 的 `minecraft:particle.soul_escape` 音效。
    ///
    /// Soul speed `TICK` `particle.soul_escape` sound.
    SoulSpeedSound {
        position: Vec3,
    },
    /// 扔出的药水这一刻撞到东西：`AbstractThrownPotion.onHit`。
    ///
    /// The splash potion hit something this tick: `AbstractThrownPotion.onHit`.
    SplashPotionImpact {
        hit: super::splash_potion::SplashHit,
    },
    /// 闪电这一刻的落地事实：引火、铜除锈、命中实体。
    ///
    /// One lightning tick: fires, copper cleaning and entity hits.
    LightningTick(LightningTick),
    /// 骷髅陷阱马被玩家触发：视觉闪电和四名骷髅骑士。
    ///
    /// A skeleton trap triggered by a player: a visual bolt and four riders.
    SkeletonTrap(TrapSpawn),
    /// 女巫扔一瓶药水：`Projectile.spawnProjectileUsingShoot`。
    ///
    /// The witch throws one potion: `Projectile.spawnProjectileUsingShoot`.
    ThrowSplashPotion {
        shot: Box<super::splash_potion::SplashShot>,
    },
    /// 生物的弩 `CrossbowItem.performShooting`：装着的箭逐支生成、射出。
    ///
    /// A mob's crossbow `CrossbowItem.performShooting`: spawn and shoot each loaded arrow.
    ShootCrossbow {
        shots: Vec<super::arrow::ArrowShot>,
    },
    /// `Raider.HoldGroundAttackGoal` 的喊话：同伴 `setTarget(target)`，`stop` 那次还
    /// `setAggressive(true)`。
    ///
    /// The shout of `Raider.HoldGroundAttackGoal`: allies `setTarget(target)`, and the
    /// `stop` call also does `setAggressive(true)`.
    RaiderShout {
        others: Vec<EntityUid>,
        target: EntityUid,
        aggressive: bool,
    },
    /// `LongDistancePatrolGoal` 的队长给同伴 `setPatrolTarget(pathTarget)`。
    ///
    /// A patrol leader's `setPatrolTarget(pathTarget)` on its companions.
    SetPatrolTarget {
        others: Vec<EntityUid>,
        target: BlockPos,
    },
    /// TNT 引信归零：整段爆炸，包括射线、洗牌、逐格破坏、连锁点燃、掉落和伤害物品。
    Explode {
        center: Vec3,
        radius: f32,
    },
    /// 生物引发的爆炸（`Level.ExplosionInteraction.MOB`）：射线、伤害生物和玩家、
    /// 按 `mobGriefing` 破坏方块。发出者就是爆炸的来源实体。
    ///
    /// A mob-caused explosion (`Level.ExplosionInteraction.MOB`): rays, damage to mobs and
    /// players, and block damage under `mobGriefing`. The emitter is the source entity.
    ExplodeMob {
        center: Vec3,
        radius: f32,
        /// 来源生物这一刻的公开数据：它正在 tick，`me.kind` 是占位，死亡消息要靠它拿名字。
        ///
        /// The source mob's public data: it is mid-tick with a placeholder `me.kind`, and
        /// death messages need its name.
        source: Box<EntityPublic>,
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
    /// `ThrownTrident` 主人不可接受时的 `spawnAtLocation(stack, 0.1f)`：比通用
    /// `DropItem` 多抬高 0.1 格（`(double) 0.1f`）。
    ///
    /// `ThrownTrident`'s `spawnAtLocation(stack, 0.1f)` for an unacceptable
    /// owner: 0.1 above the entity (`(double) 0.1f`), unlike the plain `DropItem`.
    DropTridentItem {
        stack: ItemStack,
    },
    /// `Entity.playSound(TRIDENT_RETURN, 10, 1)`：忠诚三叉戟第一刻的返回音效。
    ///
    /// `Entity.playSound(TRIDENT_RETURN, 10, 1)`: the first-tick return sound of a
    /// loyal trident.
    TridentReturnSound {
        position: Vec3,
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
    EndermanBlock {
        origin: BlockPos,
        pos: BlockPos,
        place: bool,
    },
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
        variant: super::mob::CowVariant,
        /// `AgeableMob.getLoveCause()`：官方把进度判据发给这名玩家。
        ///
        /// `AgeableMob.getLoveCause()`: vanilla credits this player's criterion.
        lover: Option<[u8; 16]>,
        wool: u8,
        partner: EntityUid,
        position: mc_bamboo_core::Vec3,
        /// 熊猫父亲的显性 / 隐性基因（落地时父亲不在实体表里）。
        ///
        /// The panda father's main and hidden genes (it is not in the table when this lands).
        father_genes: Option<(u8, u8)>,
        /// 父亲的种类和兔子品种：落地时父亲正被拆出来跑 tick，读不到。
        ///
        /// The father's type and rabbit variant; it is detached while the command lands.
        kind: super::mob::MobType,
        father_rabbit: super::mob::rabbit::Variant,
        /// 父亲的狐狸品种和求爱者：狐狸幼崽要信任两边的求爱者。
        ///
        /// The father's fox variant and love cause; a fox kit trusts both causes.
        father_fox: super::mob::fox::Variant,
        father_love: Option<[u8; 16]>,
        /// 父亲的狼状态和驯服状态：幼崽的品种、主人和项圈色要用。
        ///
        /// The father's wolf and taming state, for the pup's variant, owner and collar.
        father_wolf: Box<(super::mob::wolf::Wolf, super::mob::tame::Tamable)>,
        /// 猫父亲的状态（落地时父亲正被拆出来跑 tick）。
        ///
        /// The cat father's state (detached while the command lands).
        father_cat: super::mob::cat::Cat,
        /// 父亲是不是尖叫山羊：山羊幼崽可能随它。
        ///
        /// Whether the father is a screaming goat; a kid may take after it.
        father_screaming: bool,
        /// 父亲的美西螈花色（落地时父亲不在实体表里）。
        ///
        /// The axolotl father's variant (it is not in the table when this lands).
        father_axolotl: i32,
    },
    /// `HurtByTargetGoal.alertOthers` 在狼身上：别的狼 `setTarget(target)`。
    ///
    /// `HurtByTargetGoal.alertOthers` for wolves: the others target the attacker.
    WolfAlert {
        others: Vec<EntityUid>,
        target: EntityUid,
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
    /// `Mob.doHurtTarget` 打到另一头生物。`on_hit` 是命中后立刻挂上的效果
    /// （洞穴蜘蛛的中毒），只有伤害真的落地才生效。
    ///
    /// `Mob.doHurtTarget` against another mob. `on_hit` is applied only if the hurt lands
    /// (the cave spider's poison).
    HurtMob {
        target: EntityUid,
        source: super::mob::DamageSource,
        amount: f32,
        on_hit: Option<super::mob::effects::EffectInstance>,
    },
    /// `LivingEntity.addEffect` 对玩家：洞穴蜘蛛打中玩家后的中毒。
    ///
    /// `LivingEntity.addEffect` on a player: the cave spider's poison after a hit.
    AddPlayerEffect {
        target: EntityUid,
        effect: i32,
        duration: i32,
        amplifier: i32,
    },
    /// `ElderGuardian.customServerAiStep`：给挂上挖掘疲劳的玩家发
    /// `GUARDIAN_ELDER_EFFECT` 游戏事件包。
    ///
    /// `ElderGuardian.customServerAiStep`: send the `GUARDIAN_ELDER_EFFECT` game
    /// event packet to a player who received mining fatigue.
    GuardianElderEffect {
        target: EntityUid,
        silent: bool,
    },
    /// `BreakDoorGoal` / `RemoveBlockGoal` 的 `levelEvent(event, pos, data)`。
    MobLevelEvent {
        event: i32,
        pos: mc_bamboo_core::BlockPos,
        data: i32,
    },
    /// `TurtleLayEggGoal` 的 `level.setBlockAndUpdate` 加产蛋音效。
    ///
    /// `TurtleLayEggGoal`'s `level.setBlockAndUpdate` plus the egg-laying sound.
    LayTurtleEgg {
        pos: mc_bamboo_core::BlockPos,
        eggs: i32,
    },
    /// `ServerLevel.destroyBlockProgress(breaker, pos, progress)`：`BreakDoorGoal` 的
    /// 门上裂纹，`progress` 为 -1 时清掉。
    DestroyProgress {
        pos: mc_bamboo_core::BlockPos,
        progress: i32,
    },
    /// `BreakDoorGoal.tick` 到点：`removeBlock(pos, false)`，再 `levelEvent(1021)`
    /// 和 `levelEvent(2001, 拆完以后这一格的状态)`。
    BreakDoor {
        pos: mc_bamboo_core::BlockPos,
    },
    /// `level.removeBlock(pos, false)`：`BreakDoorGoal` 破门、`RemoveBlockGoal` 啄碎海龟蛋。
    /// 只换成这一格的流体或空气，`2001` 之类的事件由调用方另发。
    RemoveMobBlock {
        pos: mc_bamboo_core::BlockPos,
    },
    /// 兔子吃掉一龄胡萝卜作物。
    ///
    /// A rabbit removes one growth stage from a carrot crop.
    RabbitEatCarrots {
        pos: mc_bamboo_core::BlockPos,
    },
    /// `SilverfishWakeUpFriendsGoal`：`Level.destroyBlock(pos, true, silverfish)`。
    /// 破坏虫蚀方块会再走 `spawnAfterBreak`，也就是又生成一只蠹虫。
    ///
    /// `SilverfishWakeUpFriendsGoal`'s griefing branch: destroying an infested
    /// block runs `spawnAfterBreak` and spawns another silverfish.
    SilverfishBreak {
        pos: mc_bamboo_core::BlockPos,
    },
    /// `mobGriefing` 关掉时 `setBlockAndUpdate(pos, hostStateByInfested(state))`。
    ///
    /// With `mobGriefing` off, replace the infested block by its host state.
    SilverfishReveal {
        pos: mc_bamboo_core::BlockPos,
        host: crate::BlockState,
    },
    /// `SilverfishMergeWithStoneGoal.start`：把宿主方块变成虫蚀方块，蠹虫消失。
    ///
    /// `SilverfishMergeWithStoneGoal.start`: infest the host block and remove the mob.
    SilverfishMerge {
        pos: mc_bamboo_core::BlockPos,
    },
    /// `Fox.pickUpItem` 的世界一侧：多出来的放回地上，原来叼着的吐出去，捡起的那件消失。
    ///
    /// The level side of `Fox.pickUpItem`: drop the extra, spit the old item
    /// and discard the picked entity.
    FoxPickUpItem {
        item: EntityUid,
        rest: Option<ItemStack>,
        spit: Option<ItemStack>,
        spit_at: mc_bamboo_core::Vec3,
    },
    /// `FoxBreedGoal.start` 对伴侣调的 `clearStates`。
    FoxClearStates {
        target: EntityUid,
    },
    /// `FoxEatBerriesGoal.pickSweetBerries` 的世界一侧。
    FoxPickBerries {
        pos: mc_bamboo_core::BlockPos,
        age: i32,
        took_one: bool,
    },
    /// `CaveVines.use` 被狐狸调用。
    FoxPickGlowBerries {
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
    ProjectileExtinguished {
        pitch: f32,
    },
    /// `Pufferfish.touch`：先 `hurtServer`，打中了才下毒并放蜇刺声。
    ///
    /// `Pufferfish.touch`: hurt first, and only on a hit add the poison and the sting
    /// sound.
    PufferSting {
        target: EntityUid,
        source: super::mob::DamageSource,
        amount: f32,
        seconds: i32,
        position: mc_bamboo_core::Vec3,
    },
    /// `AbstractSchoolingFish.startFollowing`：跟随者记住领头并给领头的群计数加一。
    ///
    /// `AbstractSchoolingFish.startFollowing`: the follower remembers its leader and the
    /// leader's school count goes up by one.
    SchoolFollow {
        leader: EntityUid,
        follower: EntityUid,
    },
    /// `AbstractSchoolingFish.stopFollowing`：跟随者离开，领头群计数减一。
    ///
    /// `AbstractSchoolingFish.stopFollowing`: the follower leaves and the leader's count
    /// goes down by one.
    SchoolLeave {
        leader: EntityUid,
        follower: EntityUid,
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
    /// `Mob.convertTo(type, ConversionParams.single(..))`：按 `from` 这一刻的状态造出目标
    /// 生物；发出者自己随后 `discard`。`level_event` 是转化音效的世界事件。
    ///
    /// `Mob.convertTo(type, ConversionParams.single(..))`: create the target from the state
    /// `from` has this tick; the emitter discards itself. `level_event` is the conversion
    /// sound.
    ConvertMob {
        from: Box<super::mob::Mob>,
        to: super::mob::MobType,
        level_event: i32,
        /// 新生物要不要跑 `Zombie.handleAttributes`（僵尸变溺尸的 `postConversionStep`）。
        ///
        /// Whether the new mob runs `Zombie.handleAttributes`, the zombie's
        /// `postConversionStep`.
        zombie_attributes: bool,
    },
    /// `AbstractCubeMob.remove`：死掉的史莱姆被移除时，按 `from` 这一刻的状态分裂出小一号的
    /// 几只。数量和朝向已经在发出者的随机源上取好。
    ///
    /// `AbstractCubeMob.remove`: a dead slime splits into smaller cubes built from the state
    /// `from` has this tick. The count and facings were already drawn on the emitter's random.
    SplitCubeMob {
        from: Box<super::mob::Mob>,
        half_size: i32,
        children: Vec<super::mob::slime::SplitChild>,
    },
    /// 改另一只跑大脑的生物的记忆（`AnimalMakeLove` 给伴侣设繁殖、走路和看的目标）。
    ///
    /// Writes another brain-driven mob's memory (`AnimalMakeLove` setting the partner's
    /// breed, walk and look targets).
    SetBrainMemory {
        target: EntityUid,
        memory: super::mob::brain::memory::MemoryType,
        value: Option<super::mob::brain::memory::MemoryValue>,
    },
    /// `RamTarget.tick` 撞到活体：`hurtServer(noAggroMobAttack)`，然后不管打没打中都
    /// `knockback(strength, dx, dz)`。
    ///
    /// `RamTarget.tick` hitting a living entity: `hurtServer(noAggroMobAttack)`, then
    /// `knockback(strength, dx, dz)` whether or not the hit landed.
    RamHit {
        target: EntityUid,
        player: bool,
        source: super::mob::DamageSource,
        amount: f32,
        strength: f64,
        dx: f64,
        dz: f64,
    },
    /// `Tadpole.ageUp`：`convertTo(FROG, ConversionParams.single(this, false, false), ..)`，
    /// 按 `from` 这一刻的状态造出青蛙；发出者自己随后 `discard`。
    ///
    /// `Tadpole.ageUp`: `convertTo(FROG, ConversionParams.single(this, false, false), ..)`
    /// builds the frog from the state `from` has this tick; the emitter discards itself.
    TadpoleGrowUp {
        from: Box<super::mob::Mob>,
    },
    /// 蜜蜂进巢：把当前生物存进 `BeehiveBlockEntity`，再把它从实体表里移除。
    BeeEnterHive {
        hive: BlockPos,
        occupant: Box<crate::behaviour::machines::beehive_entity::Occupant>,
        saved_flower: Option<BlockPos>,
    },
    /// 蜜蜂授粉作物：写方块状态并发 2011 事件。
    BeeGrowCrop {
        pos: BlockPos,
        state: BlockState,
    },
    /// `Bee.tick` 的花蜜粒子：每次两发 `level.random.nextDouble()`。
    BeeNectarParticles {
        count: usize,
    },
    /// `TryLaySpawnOnFluidNearLand`：`setBlockAndUpdate(pos, FROGSPAWN)`。
    ///
    /// `TryLaySpawnOnFluidNearLand`: `setBlockAndUpdate(pos, FROGSPAWN)`.
    LayFrogspawn {
        pos: BlockPos,
    },
    /// `ShootTongue.tick` 伸舌头的那一刻：`target.setDeltaMovement(delta)`。
    ///
    /// The tongue shot of `ShootTongue.tick`: `target.setDeltaMovement(delta)`.
    SetMobDelta {
        target: EntityUid,
        delta: Vec3,
    },
    /// `ShootTongue.eatEntity`：`doHurtTarget`，打死了当场 `remove(KILLED)`。
    ///
    /// `ShootTongue.eatEntity`: `doHurtTarget`, then `remove(KILLED)` at once on a kill.
    FrogEat {
        target: EntityUid,
        source: super::mob::DamageSource,
        amount: f32,
    },
    /// `new ItemEntity(level, x, y, z, stack, dx, dy, dz)` 加 `addFreshEntity`：
    /// 山羊掉角，速度由调用方给。
    ///
    /// `new ItemEntity(level, x, y, z, stack, dx, dy, dz)` plus `addFreshEntity`: a goat
    /// dropping a horn with the caller's velocity.
    DropItemWithDelta {
        stack: ItemStack,
        delta: Vec3,
    },
    /// `HurtByTargetGoal.alertOthers`：把 `enemy` 设成这些同伴的目标（已有目标的跳过）。
    ///
    /// `HurtByTargetGoal.alertOthers`: make `enemy` the target of these allies, skipping
    /// those that already have one.
    AlertMobs {
        others: Vec<EntityUid>,
        enemy: EntityUid,
    },
    /// `Panda.pickUpItem`：把脚下那份吃的拿进主手。
    ///
    /// `Panda.pickUpItem`: take the edible stack into the main hand.
    PickUpItem {
        item: EntityUid,
    },
    /// `Panda.afterSneeze`：附近的成年熊猫原地跳一下。
    ///
    /// `Panda.afterSneeze`: nearby adult pandas hop.
    JumpMobs {
        others: Vec<EntityUid>,
    },
    /// `PoiManager.take`：在 `pos` 处扣一张匹配 `type_mask` 的 POI 票。
    ///
    /// `PoiManager.take`: consumes one ticket for a matching POI at `pos`.
    PoiTake {
        pos: BlockPos,
        type_mask: u32,
    },
    /// `PoiManager.release`：归还 `pos` 处 POI 的一张票。
    ///
    /// `PoiManager.release`: releases one ticket for the POI at `pos`.
    PoiRelease {
        pos: BlockPos,
    },
    /// `Villager.pickUpItem`（`InventoryCarrier.pickUpItem`）：消耗或更新地上物品（背包已在 tick 内更新）。
    ///
    /// `Villager.pickUpItem` (`InventoryCarrier.pickUpItem`): discards or shrinks the
    /// ground item after the villager's inventory has been updated.
    VillagerPickUpItem {
        item: EntityUid,
        remaining_count: i32,
    },
    /// `WorkAtComposter.compostItems`：先清空已满的堆肥桶产出骨粉。
    ///
    /// `WorkAtComposter.compostItems`: empties a ready composter to drop bone meal.
    VillagerCompostExtract {
        pos: BlockPos,
    },
    /// `WorkAtComposter.compostItems`：落地堆肥桶的新层数并发 `levelEvent(1500)`。
    ///
    /// `WorkAtComposter.compostItems`: commits the new composter level and emits `levelEvent(1500)`.
    VillagerCompostFinish {
        pos: BlockPos,
        level: i32,
        success: bool,
    },
    /// `ServerLevel.destroyBlock(pos, drop, entity)`：破坏方块并按需掉落。
    ///
    /// `ServerLevel.destroyBlock(pos, drop, entity)`: destroys a block, optionally dropping loot.
    DestroyBlock {
        pos: BlockPos,
        drop: bool,
    },
    /// `HarvestFarmland` 播种：放置作物默认状态并播放 `minecraft:item.crop.plant`。
    ///
    /// `HarvestFarmland` planting: sets crop default state and plays `minecraft:item.crop.plant`.
    PlantCrop {
        pos: BlockPos,
        state: BlockState,
    },
    /// `UseBonemeal`：`BoneMealItem.growCrop` + `Effect::BoneMealUsed`。
    ///
    /// `UseBonemeal`: `BoneMealItem.growCrop` + `Effect::BoneMealUsed`.
    GrowCrop {
        pos: BlockPos,
    },
    /// `Entity.spawnAtLocation(level, stack, yOffset)`。
    ///
    /// `Entity.spawnAtLocation(level, stack, yOffset)`.
    DropAtLocation {
        pos: Vec3,
        stack: mc_bamboo_protocol::item::ItemStack,
    },
    /// `LivingEntity.startSleeping` / `stopSleeping`：切换床的 `OCCUPIED` 属性。
    ///
    /// `LivingEntity.startSleeping` / `stopSleeping`: toggles a bed's `OCCUPIED` property.
    SetBedOccupied {
        pos: BlockPos,
        occupied: bool,
    },
}

/// 所有命令共用的落地入口。两种驱动调的是同一个函数。
///
/// `me` 是发出者，`world` 有整个 region 的写权限。处理函数直接改世界、改别的实体、
/// 改发出者，没有任何回写补丁。
pub fn apply(command: Command, me: &mut Entity, world: &mut World) {
    match command {
        Command::GameEvent {
            event,
            position,
            context,
        } => world.game_event(event, position, context),
        Command::Explode { center, radius } => world.explode_tnt(center, radius),
        Command::ExplodeMob {
            center,
            radius,
            source,
        } => world.explode_mob(&source, center, radius),
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
            effect,
        } => {
            let shot = super::mob::skeleton::MobArrowShot {
                position,
                direction,
                owner_uuid,
                weapon,
                difficulty,
                effect,
            };
            super::mob::skeleton::spawn_arrow(world, me.id, shot);
        }
        Command::ArrowImpact { block, hits, end } => {
            super::arrow::impact(me, world, hits, block, end)
        }
        Command::ShootLlamaSpit(shot) => super::spit::spawn(world, me.id, shot),
        Command::ShootTrident(shot) => {
            world.spawn_trident_shot(shot);
        }
        Command::SpitHit(hit) => super::spit::hit_entity(me, world, hit),
        Command::EggImpact { hit } => super::thrown_egg::impact(me, world, hit),
        Command::ThrowThrowable { shot } => {
            world.spawn_egg_shot(&shot);
        }
        Command::LiftMob { target, lift } => {
            world.with_entity_mut(target, &mut |entity| {
                entity.delta = entity.delta.add(mc_bamboo_core::Vec3::new(0.0, lift, 0.0));
                entity.with_mob(|_, mob| mob.hurt_marked = true);
            });
        }
        Command::PlaceSnowTrail { cells } => {
            let snow = crate::BlockState(mc_bamboo_data::blocks::SNOW);
            for pos in cells {
                world.set_block_and_update(pos, snow);
            }
        }
        Command::FreezeWater { cells } => {
            let ice = BlockState(mc_bamboo_data::blocks::FROSTED_ICE);
            for pos in cells {
                let _ = world.set_block_and_update(pos, ice);
            }
        }
        Command::SoulSpeedParticles { .. } => {
            let _ = me.random.next_double();
            let _ = me.random.next_double();
        }
        Command::SoulSpeedSound { position } => {
            // pitch 是 uniform(0.6, 1.0)，用实体随机源取样。
            // pitch is uniform(0.6, 1.0), sampled from the entity random source.
            let pitch = 0.6 + me.random.next_float() * 0.4;
            world.play_sound(
                position,
                mc_bamboo_data::sound_event("minecraft:particle.soul_escape"),
                mc_bamboo_data::sounds::NEUTRAL,
                0.6,
                pitch,
                false,
            );
        }
        Command::SplashPotionImpact { hit } => super::splash_potion::impact(me, world, hit),
        Command::LightningTick(facts) => super::lightning::apply(me, world, facts),
        Command::SkeletonTrap(facts) => super::mob::skeleton_horse::spawn_trap(world, me, facts),
        Command::ThrowSplashPotion { shot } => {
            world.spawn_splash_shot(&shot);
        }
        Command::ShootCrossbow { shots } => {
            for shot in shots {
                world.spawn_arrow_shot(shot);
            }
        }
        Command::RaiderShout {
            others,
            target,
            aggressive,
        } => super::mob::raider::apply_shout(world, &others, target, aggressive),
        Command::SetPatrolTarget { others, target } => {
            super::mob::raider::apply_patrol_target(world, &others, target);
        }
        Command::SuckIntoHopper => crate::hopper::suck_touching_item(world, me),
        Command::DropItem { stack } => {
            world.spawn_at_location(me.position, stack);
        }
        Command::DropTridentItem { stack } => {
            world.spawn_at_location(
                me.position.add(Vec3::new(0.0, f64::from(0.1_f32), 0.0)),
                stack,
            );
        }
        Command::TridentReturnSound { position } => {
            world.play_sound(
                position,
                mc_bamboo_data::sounds::sound_event("minecraft:item.trident.return"),
                mc_bamboo_data::sounds::NEUTRAL,
                10.0,
                1.0,
                false,
            );
        }
        Command::PushMob {
            other,
            from,
            self_pushable,
        } => super::mob::living::apply_push(me, other, from, self_pushable, world),
        Command::EndermanBlock { origin, pos, place } => {
            super::mob::enderman::apply_block_goal(me, world, origin, pos, place)
        }
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
            variant,
            lover,
            wool,
            partner,
            position,
            father_genes,
            kind,
            father_rabbit,
            father_fox,
            father_love,
            father_wolf,
            father_cat,
            father_screaming,
            father_axolotl,
        } => super::mob::living::apply_breed(
            me,
            partner,
            position,
            super::mob::living::BreedFather {
                kind,
                variant,
                wool,
                rabbit: father_rabbit,
                fox: father_fox,
                love: father_love,
                wolf: father_wolf.0,
                tame: father_wolf.1,
                cat: father_cat,
                screaming: father_screaming,
                genes: father_genes,
                axolotl: father_axolotl,
            },
            lover,
            world,
        ),
        Command::WolfAlert { others, target } => {
            super::mob::wolf_goals::apply_alert(world, &others, target);
        }
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
            on_hit,
        } => {
            if world.hurt_mob(target, source, amount)
                && let Some(effect) = on_hit
            {
                world.give_mob_effect(target, effect);
            }
        }
        Command::AddPlayerEffect {
            target,
            effect,
            duration,
            amplifier,
        } => {
            world.give_player_effect(
                target,
                super::mob::effects::EffectInstance::with_amplifier(effect, duration, amplifier),
            );
        }
        Command::GuardianElderEffect { target, silent } => {
            world.push_effect(crate::Effect::GuardianElderEffect { target, silent });
        }
        Command::MobLevelEvent { event, pos, data } => {
            world.push_effect(crate::Effect::LevelEvent {
                event,
                pos,
                data,
                except_initiator: false,
            });
        }
        Command::RabbitEatCarrots { pos } => super::mob::rabbit::eat_carrots(world, pos),
        Command::FoxPickUpItem {
            item,
            rest,
            spit,
            spit_at,
        } => super::mob::fox::apply_pick_up(me, world, item, rest, spit, spit_at),
        Command::FoxClearStates { target } => {
            world.with_entity_mut(target, &mut |entity| {
                entity.with_mob(|_, mob| mob.fox.clear_states());
            });
        }
        Command::FoxPickBerries { pos, age, took_one } => {
            super::mob::fox::apply_pick_berries(world, pos, age, took_one)
        }
        Command::FoxPickGlowBerries { pos } => super::mob::fox::apply_pick_glow_berries(world, pos),
        Command::DestroyProgress { pos, progress } => {
            world.push_effect(crate::Effect::DestroyProgress {
                breaker: me.id,
                pos,
                progress,
            });
        }
        Command::BreakDoor { pos } => {
            let _ = world.remove_block(pos);
            for (event, data) in [(1021, 0), (2001, world.block_state(pos).0 as i32)] {
                world.push_effect(crate::Effect::LevelEvent {
                    event,
                    pos,
                    data,
                    except_initiator: false,
                });
            }
        }
        Command::RemoveMobBlock { pos } => {
            let _ = world.remove_block(pos);
        }
        Command::SilverfishBreak { pos } => {
            let _ = world.destroy_block(pos, true, crate::world::UPDATE_LIMIT);
        }
        Command::SilverfishReveal { pos, host } => {
            let _ = world.set_block_and_update(pos, host);
        }
        Command::SilverfishMerge { pos } => {
            let state = world.block_state(pos);
            if let Some(infested) = super::mob::infested::infested_state_by_host(state) {
                let _ = world.set_block_and_update(pos, infested);
                // `PathfinderMob.spawnAnim` 发实体事件 20，然后 `discard()`。
                if let Some(mob) = me.kind.mob_mut() {
                    mob.events
                        .push(super::mob::MobEvent::EntityEvent { event: 20 });
                }
                me.discard();
            }
        }
        Command::SetBrainMemory {
            target,
            memory,
            value,
        } => {
            let mut value = value;
            world.with_entity_mut(target, &mut |entity| {
                entity.with_mob(|_, mob| {
                    if let Some(brain) = mob.brain.as_mut() {
                        brain.memories.set_or_erase(memory, value.take());
                    }
                });
            });
        }
        Command::RamHit {
            target,
            player,
            source,
            amount,
            strength,
            dx,
            dz,
        } => super::mob::goat::apply_ram_hit(
            me,
            world,
            target,
            player,
            source,
            amount,
            (strength, dx, dz),
        ),
        Command::DropItemWithDelta { stack, delta } => {
            world.spawn_entity(Entity::ground_item_with_delta(me.position, stack, delta));
        }
        Command::DropEntityLoot(request) => {
            let origin = request.origin;
            for stack in world.entity_loot_drops(request) {
                world.spawn_at_location(origin, stack);
            }
        }
        Command::AlertMobs { others, enemy } => {
            super::mob::polar_bear::apply_alert(world, &others, enemy);
        }
        Command::PickUpItem { item } => {
            super::mob::panda::apply_pick_up(world, item, me);
        }
        Command::JumpMobs { others } => {
            for other in others {
                world.with_entity_mut(other, &mut |entity| {
                    let power = entity.kind.mob().map_or(0.0, |mob| {
                        mob.attributes
                            .value(mc_bamboo_data::registries::attribute::JUMP_STRENGTH)
                            as f32
                    });
                    if power > 1.0e-5 {
                        entity.delta.y = entity.delta.y.max(f64::from(power));
                    }
                });
            }
        }
        Command::EatBlock { pos, grass_below } => {
            super::mob::living::apply_eat_block(world, pos, grass_below);
        }
        Command::ConvertMob {
            from,
            to,
            level_event,
            zombie_attributes,
        } => super::mob::conversion::apply_convert(
            me,
            &from,
            to,
            level_event,
            zombie_attributes,
            world,
        ),
        Command::SplitCubeMob {
            from,
            half_size,
            children,
        } => super::mob::slime::apply_split(me, &from, half_size, &children, world),
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
        Command::ProjectileExtinguished { pitch } => {
            super::arrow::play_extinguished_sound(me, world, pitch);
        }
        Command::PufferSting {
            target,
            source,
            amount,
            seconds,
            position,
        } => {
            if world.hurt_mob(target, source, amount) {
                let poison = mc_bamboo_data::registries::mob_effect::POISON;
                world.with_entity_mut(target, &mut |entity| {
                    entity.with_mob(|_, mob| {
                        super::mob::living::add_effect(
                            mob,
                            super::mob::effects::EffectInstance::new(poison, seconds),
                        )
                    });
                });
                world.play_sound(
                    position,
                    mc_bamboo_data::sound_event("minecraft:entity.puffer_fish.sting"),
                    mc_bamboo_data::sounds::NEUTRAL,
                    1.0,
                    1.0,
                    false,
                );
            }
        }
        Command::SchoolFollow { leader, follower } => {
            world.with_entity_mut(follower, &mut |entity| {
                entity.with_mob(|_, mob| mob.fish.leader = Some(leader));
            });
            world.with_entity_mut(leader, &mut |entity| {
                entity.with_mob(|_, mob| mob.fish.school_size += 1);
            });
        }
        Command::SchoolLeave { leader, follower } => {
            world.with_entity_mut(follower, &mut |entity| {
                entity.with_mob(|_, mob| mob.fish.leader = None);
            });
            world.with_entity_mut(leader, &mut |entity| {
                entity.with_mob(|_, mob| mob.fish.school_size -= 1);
            });
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
        Command::TadpoleGrowUp { from } => super::mob::tadpole::apply_grow_up(me, &from, world),
        Command::LayFrogspawn { pos } => {
            let state = crate::state::BlockState(mc_bamboo_data::blocks::FROGSPAWN);
            world.set_block(pos, state, 3);
        }
        Command::SetMobDelta { target, delta } => {
            world.with_listed_entity_mut(target, &mut |entity| entity.delta = delta);
        }
        Command::BeeEnterHive {
            hive,
            occupant,
            saved_flower,
        } => super::mob::bee_goals::apply_enter_hive(me, world, hive, *occupant, saved_flower),
        Command::BeeGrowCrop { pos, state } => {
            world.level_event(2011, pos, 15);
            let _ = world.set_block_and_update(pos, state);
        }
        Command::BeeNectarParticles { count } => {
            for _ in 0..count {
                let _ = world.random().next_double();
                let _ = world.random().next_double();
            }
        }
        Command::FrogEat {
            target,
            source,
            amount,
        } => super::mob::frog::apply_eat(world, target, source, amount),
        Command::LayTurtleEgg { pos, eggs } => {
            let pitch = 0.9 + world.random().next_float() * 0.2;
            world.play_block_sound(
                pos,
                mc_bamboo_data::sounds::sound_event("minecraft:entity.turtle.lay_egg"),
                0.3,
                pitch,
            );
            // `blocks::TURTLE_EGG` 已经是默认状态 ID，不是方块注册表下标。
            // `blocks::TURTLE_EGG` is already the default state id, not a block registry index.
            let state = crate::state::BlockState(mc_bamboo_data::blocks::TURTLE_EGG);
            world.set_block(pos, state.with(crate::state::EGGS, eggs), 3);
        }
        Command::PoiTake { pos, type_mask } => {
            let _ = world.poi_take(pos, type_mask);
        }
        Command::PoiRelease { pos } => {
            let _ = world.poi_release(pos);
        }
        Command::VillagerPickUpItem {
            item,
            remaining_count,
        } => {
            super::mob::villager::apply_pick_up(world, item, remaining_count);
        }
        Command::VillagerCompostExtract { pos } => {
            super::mob::villager::apply_compost_extract(world, pos);
        }
        Command::VillagerCompostFinish {
            pos,
            level,
            success,
        } => {
            super::mob::villager::apply_compost_finish(world, pos, level, success);
        }
        Command::DestroyBlock { pos, drop } => {
            let _ = world.destroy_block(pos, drop, 512);
        }
        Command::PlantCrop { pos, state } => {
            let _ = world.set_block_and_update(pos, state);
            world.play_sound(
                Vec3::new(f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)),
                mc_bamboo_data::sounds::sound_event("minecraft:item.crop.plant"),
                mc_bamboo_data::sounds::BLOCKS,
                1.0,
                1.0,
                false,
            );
        }
        Command::GrowCrop { pos } => {
            if crate::item_use::grow_crop(world, pos) {
                world.push_effect(crate::level::Effect::BoneMealUsed { pos });
            }
        }
        Command::DropAtLocation { pos, stack } => {
            world.spawn_at_location(pos, stack);
        }
        Command::SetBedOccupied { pos, occupied } => {
            let state = world.block_state(pos);
            if state.is_tagged(mc_bamboo_data::tags::block_tags::BEDS) {
                let next = state.with(crate::state::OCCUPIED, occupied);
                world.set_block_and_update(pos, next);
            }
        }
    }
}

/// 精确模式的上下文：读当前世界，`emit` 立刻生效。
///
/// 和并行模式一样，实体逻辑只看得到 [`EntityCtx`]；差别只在 `emit` 的实现。
pub struct PreciseCtx<'w, 'a> {
    world: &'w mut World<'a>,
    uid: EntityUid,
    plan: ThrottlePlan,
}

impl<'w, 'a> PreciseCtx<'w, 'a> {
    pub fn new(world: &'w mut World<'a>, uid: EntityUid) -> Self {
        Self {
            world,
            uid,
            plan: ThrottlePlan::FULL,
        }
    }

    /// 只给实体驱动用：带上这一刻算好的降频计划。
    pub fn with_plan(world: &'w mut World<'a>, uid: EntityUid, plan: ThrottlePlan) -> Self {
        Self { world, uid, plan }
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

    fn throttle_plan(&self) -> ThrottlePlan {
        self.plan
    }

    fn blocks(&self) -> &dyn BlockView {
        &*self.world
    }

    fn other(&self, uid: EntityUid) -> Option<EntityPublic> {
        self.world.level().entity_public(uid)
    }

    fn with_other(&self, uid: EntityUid, visit: &mut dyn FnMut(&EntityPublic)) -> bool {
        self.world.level().with_entity_public(uid, visit)
    }

    fn entity_uids(&self) -> Vec<EntityUid> {
        self.world.level().entity_uids()
    }

    fn for_each_other_in(&self, area: Aabb, visit: &mut dyn FnMut(&EntityPublic)) {
        self.world.level().for_each_entity_in(area, visit);
    }

    fn for_each_other_of_types_in(
        &self,
        area: Aabb,
        types: &[i32],
        visit: &mut dyn FnMut(&EntityPublic),
    ) {
        self.world
            .level()
            .for_each_entity_of_types_in(area, types, visit);
    }

    fn pushable_others(
        &self,
        area: Aabb,
        limit: usize,
        visit: &mut dyn FnMut(&EntityPublic) -> bool,
    ) {
        self.world.level().for_each_pushable_in(area, limit, visit);
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
