//! 求值时用到的上下文，对应官方的 `LootParams` 和 `LootContext`。
//!
//! 官方上下文里一定有 `ServerLevel`，本实现的 `mc-bamboo-loot` 不依赖世界线程，
//! 所以世界查询放在 [`LevelView`] 后面，由调用方实现。没提供视图时，依赖世界的
//! 条件会报 [`LootError::Unsupported`]，而不是悄悄按 false 算。
//!
//! 随机源和官方一样可以显式传（箱子种子）或按序列名取（`random_sequence`）。

use std::cell::RefCell;
use std::collections::BTreeMap;

use mc_bamboo_blocks::BlockState;
use mc_bamboo_blocks::random::RandomSource;
use serde_json::Value;

use crate::names;
use crate::stack::LootStack;
use crate::{LootError, Result};

/// 世界坐标，和官方 `Vec3` 一样用 `f64`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Position {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// `BlockPos.containing`：三个坐标各自向下取整。
    pub fn block_pos(&self) -> (i32, i32, i32) {
        (
            self.x.floor() as i32,
            self.y.floor() as i32,
            self.z.floor() as i32,
        )
    }

    pub fn offset(self, x: i32, y: i32, z: i32) -> Self {
        Self::new(
            self.x + f64::from(x),
            self.y + f64::from(y),
            self.z + f64::from(z),
        )
    }
}

/// `EquipmentSlot` 的枚举顺序，也是官方 `EquipmentSlot.VALUES` 的顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipmentSlot {
    Mainhand = 0,
    Offhand = 1,
    Feet = 2,
    Legs = 3,
    Chest = 4,
    Head = 5,
    Body = 6,
    Saddle = 7,
}

impl EquipmentSlot {
    pub const ALL: [Self; 8] = [
        Self::Mainhand,
        Self::Offhand,
        Self::Feet,
        Self::Legs,
        Self::Chest,
        Self::Head,
        Self::Body,
        Self::Saddle,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// `EquipmentSlot.getSerializedName`，不带 `minecraft:` 前缀。
    pub fn serialized_name(self) -> &'static str {
        match self {
            Self::Mainhand => "mainhand",
            Self::Offhand => "offhand",
            Self::Feet => "feet",
            Self::Legs => "legs",
            Self::Chest => "chest",
            Self::Head => "head",
            Self::Body => "body",
            Self::Saddle => "saddle",
        }
    }

    /// 这个槽在不在 `EquipmentSlotGroup` 里。附魔的 `slots` 字段用组名。
    pub fn in_group(self, group: &str) -> bool {
        match group {
            "any" => true,
            "hand" => matches!(self, Self::Mainhand | Self::Offhand),
            // 官方 `armor` 组包含全部人形盔甲槽和身体槽，马鞍不在内。
            "armor" => matches!(
                self,
                Self::Feet | Self::Legs | Self::Chest | Self::Head | Self::Body
            ),
            other => self.serialized_name() == other,
        }
    }
}

/// 实体侧面，只放战利品条件会读的字段。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntityView {
    /// 实体类型注册表 ID。
    pub entity_type: i32,
    /// `entityTags()`：实体类型标签的名字。
    pub tags: Vec<String>,
    /// 是不是 `LivingEntity`，装备谓词对非生物直接判 false。
    pub is_living: bool,
    pub is_on_ground: bool,
    pub is_on_fire: bool,
    pub is_sneaking: bool,
    pub is_sprinting: bool,
    pub is_swimming: bool,
    /// `isFallFlying() || 玩家在飞`。
    pub is_flying: bool,
    pub is_baby: bool,
    pub is_in_water: bool,
    pub is_fall_flying: bool,
    /// 实体自身的组件，`copy_components` 和组件谓词用。
    pub components: BTreeMap<String, Value>,
    /// 按 [`EquipmentSlot`] 顺序排列的装备。
    pub equipment: [Option<LootStack>; 8],
    /// `SlotProvider.getSlot` 视图：槽位编号到那一格物品（在表里＝这一格存在，
    /// 物品可能是空的）。`minecraft:slots` 子谓词和 `if items|slots` 用它。
    ///
    /// `SlotProvider.getSlot` view: slot id to that slot's stack (present in the
    /// map means the slot exists, possibly empty). Used by the `minecraft:slots`
    /// sub-predicate and `if items|slots`.
    pub slots: BTreeMap<i32, LootStack>,
    /// 实体坐标，`distance` 这类谓词用。没给时报不支持。
    pub position: Option<Position>,
    /// `getKnownMovement()` 的原始速度，谓词里乘 20 才是每秒距离。
    pub movement: Option<Position>,
    pub fall_distance: Option<f64>,
    /// `getOnPos()`。
    pub on_pos: Option<(i32, i32, i32)>,
    /// `getBlockPosBelowThatAffectsMyMovement()`。
    pub movement_affected_by_pos: Option<(i32, i32, i32)>,
    /// 积分板标签，`entity_tags` 谓词用，不是实体类型标签。
    pub scoreboard_tags: Vec<String>,
    /// `tickCount`，`periodic_tick` 谓词用。
    pub tick_count: Option<i32>,
    pub vehicle: Option<Box<EntityView>>,
    pub passenger: Option<Box<EntityView>>,
    pub targeted_entity: Option<Box<EntityView>>,
    /// `AbstractCubeMob.getSize()`。
    pub cube_mob_size: Option<i32>,
    pub is_raider: bool,
    pub has_raid: bool,
    pub is_captain: bool,
    pub is_sheep: bool,
    pub sheep_sheared: bool,
    pub is_fishing_hook: bool,
    pub in_open_water: bool,
    /// `LivingEntity.getActiveEffectsMap()`；非生物留空，`effects` 谓词先看 [`Self::is_living`]。
    ///
    /// `LivingEntity.getActiveEffectsMap()`. Empty for non-living entities; the `effects`
    /// predicate checks [`Self::is_living`] first.
    pub effects: Vec<EffectView>,
    /// `NbtPredicate.getEntityTagToCompare` 的结果。没给时 `nbt` 谓词报不支持。
    ///
    /// The result of `NbtPredicate.getEntityTagToCompare`. The `nbt` predicate is
    /// unsupported while this is missing.
    pub nbt: Option<mc_bamboo_nbt::Tag>,
    /// `Entity.getTeam()` 的队伍名，没有队伍是 `None`。
    ///
    /// The name of `Entity.getTeam()`, or `None` without a team.
    pub team: Option<String>,
    /// `Entity.getScoreboardName()`：玩家是名字，其它实体是 UUID 字符串。
    /// `entity_scores` 条件和玩家谓词用它向 [`LevelView`] 查数据。
    ///
    /// `Entity.getScoreboardName()`: the player name, or the UUID string for other
    /// entities. `entity_scores` and the player predicate use it to query [`LevelView`].
    pub scoreboard_name: Option<String>,
    /// 只有 `LightningBolt` 才有。
    ///
    /// Present only for a `LightningBolt`.
    pub lightning: Option<LightningView>,
    /// 只有 `ServerPlayer` 才有。
    ///
    /// Present only for a `ServerPlayer`.
    pub player: Option<Box<PlayerView>>,
}

/// 一条 `MobEffectInstance`。
///
/// One `MobEffectInstance`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectView {
    /// 药水效果注册表 ID。
    pub effect: i32,
    pub amplifier: i32,
    /// `getDuration()`，无限时长是 -1。
    pub duration: i32,
    pub ambient: bool,
    pub visible: bool,
}

/// `LightningBolt` 里谓词会读的两项。
///
/// The two things the predicate reads from a `LightningBolt`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LightningView {
    /// `getBlocksSetOnFire()`。
    pub blocks_set_on_fire: i32,
    /// `getHitEntities()`。
    pub hit_entities: Vec<EntityView>,
}

/// `Input`：玩家最近一次上报的按键。
///
/// `Input`: the keys the player last reported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputView {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

/// `PlayerPredicate` 直接从玩家身上读的字段。统计、配方和进度按
/// [`EntityView::scoreboard_name`] 走 [`LevelView`] 查。
///
/// What `PlayerPredicate` reads straight off the player. Statistics, recipes and
/// advancements are queried through [`LevelView`] by [`EntityView::scoreboard_name`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerView {
    pub experience_level: i32,
    pub food_level: i32,
    pub saturation: f32,
    /// `GameType.getSerializedName()`：`survival`、`creative`、`adventure`、`spectator`。
    pub game_mode: &'static str,
    /// `getLastClientInput()`。没给时 `input` 谓词报不支持。
    pub input: Option<InputView>,
    /// `looking_at` 的射线结果：外层 `None` 是调用方没算，内层 `None` 是没看着实体
    /// 或者没有视线。
    ///
    /// The `looking_at` ray result. The outer `None` means the caller did not compute it;
    /// the inner `None` means no entity is hit or there is no line of sight.
    pub looking_at: Option<Option<Box<EntityView>>>,
}

/// 一个进度的完成情况，`PlayerPredicate` 的 `advancements` 用。
///
/// The progress of one advancement, for `advancements` in `PlayerPredicate`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdvancementView {
    /// `AdvancementProgress.isDone()`。
    pub done: bool,
    /// 每个判据是否完成；不在表里的判据官方当作不存在。
    ///
    /// Whether each criterion is done. A criterion missing here does not exist.
    pub criteria: BTreeMap<String, bool>,
}

impl EntityView {
    pub fn new(entity_type: i32) -> Self {
        Self {
            entity_type,
            ..Self::default()
        }
    }

    pub fn equipment(&self, slot: EquipmentSlot) -> Option<&LootStack> {
        self.equipment[slot.index()].as_ref()
    }

    /// 一件装备换成空栈，和官方 `ItemStack.EMPTY` 语义一致。
    pub fn equipment_stack(&self, slot: EquipmentSlot) -> Option<&LootStack> {
        self.equipment(slot).filter(|stack| !stack.is_empty())
    }
}

/// 伤害来源侧面。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DamageSourceView {
    /// 伤害类型注册表 ID。
    pub damage_type: i32,
    /// 直接造成伤害的实体（箭）。
    pub direct_entity: Option<Box<EntityView>>,
    /// 真正下手的实体（射手）。
    pub source_entity: Option<Box<EntityView>>,
    /// `DamageSource.isDirect()`。
    pub is_direct: bool,
}

/// 方块实体侧面，`copy_components` 的 `block_entity` 来源用。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlockEntityView {
    pub components: BTreeMap<String, Value>,
}

/// `ExplorationMapFunction` 交给世界侧的参数，对应官方 `run` 里读取的字段。
///
/// Parameters `ExplorationMapFunction.run` hands to the world side.
/// `destination` is the raw `HolderSet` reference: a `#tag` or a single structure name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorationMapRequest<'a> {
    /// `LootContextParams.ORIGIN` 经 `BlockPos.containing` 取整后的坐标。
    ///
    /// `LootContextParams.ORIGIN` snapped by `BlockPos.containing`.
    pub origin: (i32, i32, i32),
    pub destination: &'a str,
    /// `map_decoration_type` 注册表名字，缺省 `minecraft:mansion`。
    ///
    /// `map_decoration_type` name, default `minecraft:mansion`.
    pub decoration: &'a str,
    pub zoom: i8,
    pub search_radius: i32,
    pub skip_existing_chunks: bool,
}

/// `MapItem.applyNewSavedData` 分配到的 `map_id` 和 `findNearestMapStructure` 的落点。
///
/// The `map_id` allocated by `MapItem.applyNewSavedData` and the located structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorationMapResult {
    pub map_id: i32,
    pub target_x: i32,
    pub target_z: i32,
}

/// 世界查询。官方条件里所有 `ServerLevel` 的调用都走这里。
///
/// 返回 `None` 表示本实现拿不到这个信息，求值会以 `Unsupported` 结束。
/// 不能拿默认值冒充：比如把生物群系当成 0，背鳍箱子的条件就会悄悄变错。
pub trait LevelView {
    fn is_loaded(&self, x: i32, y: i32, z: i32) -> Option<bool>;
    fn block_state(&self, x: i32, y: i32, z: i32) -> Option<BlockState>;
    fn biome(&self, x: i32, y: i32, z: i32) -> Option<i32>;
    fn can_see_sky(&self, x: i32, y: i32, z: i32) -> Option<bool>;
    fn is_raining(&self) -> Option<bool>;
    fn is_thundering(&self) -> Option<bool>;
    fn day_time(&self) -> Option<i64>;
    fn game_time(&self) -> Option<i64>;
    /// 维度注册表名字，比如 `minecraft:overworld`。
    fn dimension(&self) -> Option<&'static str>;

    /// 方块实体的组件，`block_entity` 条件用。返回 `Ok(None)` 表示这个位置没有方块实体；
    /// 默认实现表示调用方没提供这个信息，求值会报不支持。
    fn block_entity_components(
        &self,
        _x: i32,
        _y: i32,
        _z: i32,
    ) -> Result<Option<BTreeMap<String, Value>>> {
        Err(LootError::Unsupported(
            "block entity components are not provided by the level view".to_owned(),
        ))
    }

    /// `getMaxLocalRawBrightness`。
    fn light(&self, _x: i32, _y: i32, _z: i32) -> Option<i32> {
        None
    }

    /// `CampfireBlock.isSmokeyPos`。
    fn is_smokey(&self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        None
    }

    /// 世界时钟的总刻数，`time_check` 用。参数是时钟名字，比如 `minecraft:overworld`。
    fn clock_time(&self, _clock: &str) -> Option<i64> {
        None
    }

    /// `StructureManager.getStructureWithPieceAt(pos, structures).isValid()`。
    /// `structures` 是谓词里写的结构名字和 `#标签`，命名空间已经补齐。
    ///
    /// `StructureManager.getStructureWithPieceAt(pos, structures).isValid()`.
    /// `structures` holds the structure names and `#tags` as written, namespaces filled in.
    fn structure_with_piece_at(
        &self,
        _x: i32,
        _y: i32,
        _z: i32,
        _structures: &[String],
    ) -> Option<bool> {
        None
    }

    /// 积分板上的一项分数。外层 `None` 是调用方没接积分板；内层 `None` 是目标不存在
    /// 或者这个持有者在目标下没有分数（官方两种情况都判 false）。
    ///
    /// One scoreboard score. The outer `None` means no scoreboard is wired in; the inner
    /// `None` means the objective is missing or the holder has no score in it, both of
    /// which vanilla treats as false.
    fn score(&self, _holder: &str, _objective: &str) -> Option<Option<i32>> {
        None
    }

    /// `StatsCounter.getValue`。`stat_type` 和 `stat` 都是带命名空间的名字。
    ///
    /// `StatsCounter.getValue`. `stat_type` and `stat` are namespaced names.
    fn player_stat(&self, _player: &str, _stat_type: &str, _stat: &str) -> Option<i32> {
        None
    }

    /// `ServerRecipeBook.contains`。
    fn player_has_recipe(&self, _player: &str, _recipe: &str) -> Option<bool> {
        None
    }

    /// 玩家在一个进度上的完成情况。外层 `None` 是调用方没接进度；内层 `None` 是服务器
    /// 没有这个进度。
    ///
    /// A player's progress on one advancement. The outer `None` means advancements are
    /// not wired in; the inner `None` means the server has no such advancement.
    fn player_advancement(&self, _player: &str, _id: &str) -> Option<Option<AdvancementView>> {
        None
    }

    /// `ExplorationMapFunction.run`：`ServerLevel.findNearestMapStructure`、
    /// `MapItem.applyNewSavedData`、`MapItem.renderBiomePreviewMap` 和
    /// `MapItemSavedData.addTargetDecoration` 都在这里落地；世界侧还要建好对应的
    /// `MapItemSavedData`（分配 `map_id`、写群系预览颜色、登记目标标记）。
    ///
    /// 返回 `Ok(None)` 表示找不到结构：官方原样退回输入物品堆，调用方再靠
    /// `filtered` 把它丢掉。默认实现就返回 `None`，对应假世界和关掉结构生成的世界；
    /// 真实世界的实现要按官方顺序做上面四步。
    ///
    /// `ExplorationMapFunction.run`: `ServerLevel.findNearestMapStructure`,
    /// `MapItem.applyNewSavedData`, `MapItem.renderBiomePreviewMap` and
    /// `MapItemSavedData.addTargetDecoration` all land here, along with creating
    /// the backing `MapItemSavedData`. `Ok(None)` means no structure was found,
    /// so vanilla returns the input stack unchanged (usually discarded by
    /// `filtered`). The default returns `None` for fake worlds; a real world
    /// implementation performs all four steps in vanilla order.
    fn exploration_map(
        &self,
        _request: &ExplorationMapRequest<'_>,
    ) -> Result<Option<ExplorationMapResult>> {
        Ok(None)
    }
}

/// `LootParams`：一次求值带的参数，缺哪个由条件自己判断。
#[derive(Default)]
pub struct LootParams<'w> {
    pub level: Option<&'w dyn LevelView>,
    pub origin: Option<Position>,
    pub block_state: Option<BlockState>,
    pub block_entity: Option<BlockEntityView>,
    pub tool: Option<LootStack>,
    pub this_entity: Option<EntityView>,
    pub interacting_entity: Option<EntityView>,
    pub target_entity: Option<EntityView>,
    pub last_damage_player: Option<EntityView>,
    pub damage_source: Option<DamageSourceView>,
    pub attacking_entity: Option<EntityView>,
    pub direct_attacking_entity: Option<EntityView>,
    pub explosion_radius: Option<f32>,
    pub enchantment_level: Option<i32>,
    pub enchantment_active: Option<bool>,
    pub additional_cost_component_allowed: Option<bool>,
    /// 动态掉落，按 `DynamicLoot` 里的名字索引。
    pub dynamic_drops: BTreeMap<String, Vec<LootStack>>,
    pub luck: f32,
}

impl<'w> LootParams<'w> {
    pub fn new(luck: f32) -> Self {
        Self {
            luck,
            ..Self::default()
        }
    }

    pub fn with_origin(mut self, origin: Position) -> Self {
        self.origin = Some(origin);
        self
    }
}

/// 求值时的上下文。随机源和世界参数都是借用，生命周期不要求相同。
pub struct LootContext<'p, 'w, 'r> {
    pub params: &'p LootParams<'w>,
    pub random: &'r dyn RandomSource,
    /// `LootContext.visitedElements`：正在求值的表，防止嵌套引用绕回去。
    ///
    /// Tables currently being evaluated. Matches `LootContext.visitedElements`.
    visited: RefCell<Vec<*const ()>>,
}

impl<'p, 'w, 'r> LootContext<'p, 'w, 'r> {
    pub fn new(params: &'p LootParams<'w>, random: &'r dyn RandomSource) -> Self {
        Self {
            params,
            random,
            visited: RefCell::new(Vec::new()),
        }
    }

    /// `pushVisitedElement`。已经在栈上时返回 `false`，调用方要跳过这张表。
    ///
    /// `pushVisitedElement`. Returns false when the table is already on the stack;
    /// the caller skips it, matching the official infinite-loop guard.
    pub(crate) fn enter_table(&self, table: *const ()) -> bool {
        let mut visited = self.visited.borrow_mut();
        if visited.contains(&table) {
            return false;
        }
        visited.push(table);
        true
    }

    /// `popVisitedElement`。
    pub(crate) fn leave_table(&self, table: *const ()) {
        let mut visited = self.visited.borrow_mut();
        if let Some(index) = visited.iter().rposition(|entry| *entry == table) {
            visited.remove(index);
        }
    }

    pub fn random(&self) -> &'r dyn RandomSource {
        self.random
    }

    pub fn luck(&self) -> f32 {
        self.params.luck
    }

    /// 世界视图，没提供时报不支持。
    pub fn level(&self, what: &str) -> Result<&'w dyn LevelView> {
        self.params
            .level
            .ok_or_else(|| LootError::Unsupported(format!("{what} needs a level view")))
    }

    pub fn origin(&self, what: &str) -> Result<Position> {
        self.params
            .origin
            .ok_or_else(|| LootError::Unsupported(format!("{what} needs the origin parameter")))
    }

    /// `LootContext.getOptional(LootContextParams.ENCHANTMENT_LEVEL)`。
    pub fn enchantment_level(&self) -> i32 {
        self.params.enchantment_level.unwrap_or(0)
    }

    /// `LootContext.addDynamicDrops`：按名字把调用方给的动态掉落塞进输出。
    /// 调用方没提供这个名字时官方什么都不做。
    ///
    /// `LootContext.addDynamicDrops`. A missing name is a no-op, matching
    /// `LootParams.addDynamicDrops`.
    pub fn add_dynamic_drops(&self, name: &str, output: &mut Vec<LootStack>) {
        if let Some(drops) = self.params.dynamic_drops.get(name) {
            output.extend(drops.iter().cloned());
        }
    }
}

/// 官方 `LootContext.EntityTarget`，条件里只写名字。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityTarget {
    This,
    Attacker,
    DirectAttacker,
    AttackingPlayer,
    TargetEntity,
    InteractingEntity,
}

impl EntityTarget {
    /// `LootItemEntityPropertyCondition` 的 `entity` 字段。
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "this" => Some(Self::This),
            "attacker" => Some(Self::Attacker),
            "direct_attacker" => Some(Self::DirectAttacker),
            "attacking_player" => Some(Self::AttackingPlayer),
            "target_entity" => Some(Self::TargetEntity),
            "interacting_entity" => Some(Self::InteractingEntity),
            _ => None,
        }
    }

    pub fn get<'a>(self, params: &'a LootParams<'_>) -> Option<&'a EntityView> {
        match self {
            Self::This => params.this_entity.as_ref(),
            Self::Attacker => params.attacking_entity.as_ref(),
            Self::DirectAttacker => params.direct_attacking_entity.as_ref(),
            Self::AttackingPlayer => params.last_damage_player.as_ref(),
            Self::TargetEntity => params.target_entity.as_ref(),
            Self::InteractingEntity => params.interacting_entity.as_ref(),
        }
    }
}

/// 实体装备上某个附魔的最高等级。
///
/// `EnchantmentHelper.getEnchantmentLevel(Holder, LivingEntity)`：按 `slots` 组
/// 遍历装备槽，取所有匹配非空物品里的最大等级。
pub fn entity_enchantment_level(entity: &EntityView, enchantment: i32) -> i32 {
    let Some(definition) = names::enchantment_def(enchantment) else {
        return 0;
    };
    let mut level = 0;
    for slot in EquipmentSlot::ALL {
        if !definition.slots.iter().any(|group| slot.in_group(group)) {
            continue;
        }
        if let Some(stack) = entity.equipment_stack(slot) {
            level = level.max(stack.item_enchantment_level(enchantment));
        }
    }
    level
}
