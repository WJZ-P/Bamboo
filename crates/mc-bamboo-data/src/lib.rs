//! 目标游戏版本的全部数据：协议号、数据包 ID、方块、物品、注册表和标签。
//!
//! `generated` 目录由 `mc-bamboo-vanilla` 从官方服务端生成。
//! 版本相关的数字都集中在这里，其他 crate 只引用名字，升级版本时不用改逻辑代码。
//! 这里只有数据和按下标取数据的函数，方块的逻辑在 `mc-bamboo-blocks`。

#[rustfmt::skip]
mod generated;

use mc_bamboo_core::Aabb;

/// 配方表的数据结构和官方数据包导出的全部配方。
pub mod recipe;

pub use recipe::{
    CookingKind, FireworkShape, Ingredient, PotionIngredient, Recipe, RecipeKind, RecipeType,
    Stack, has_dye_component, is_damageable,
};

pub use generated::{
    block_classes::{BlockAncestor, BlockClass},
    block_maps, block_transformers, blocks, component_enums, component_registries, heightmaps,
    item_components, items, jukebox_songs, light, loot, packets, recipes, registries, shapes,
    states, tags, version, worldgen,
};

const BLOCK_TAG_REGISTRY: &str = "minecraft:block";
const ITEM_TAG_REGISTRY: &str = "minecraft:item";

/// `states::STATE_FACTS` 里每一位的含义。全部用 `EmptyBlockGetter` 在 `BlockPos.ZERO` 读取。
///
/// `Extractor.java` 里有同样的定义，两边要一起改。
pub mod facts {
    /// `isFaceSturdy(FULL)` 的起始位，加上 `Direction` 的 ID 就是这个面的位。
    pub const STURDY_FULL: u32 = 0;
    /// `isFaceSturdy(CENTER)` 的起始位。
    pub const STURDY_CENTER: u32 = 6;
    /// `isFaceSturdy(RIGID)` 的起始位。
    pub const STURDY_RIGID: u32 = 12;
    /// 全部 `isFaceSturdy` 的位。
    pub const STURDY_MASK: u64 = (1 << 18) - 1;
    /// `isAir`。
    pub const AIR: u64 = 1 << 18;
    /// `canOcclude`。
    pub const CAN_OCCLUDE: u64 = 1 << 19;
    /// `isSolid`，官方标了 `@Deprecated` 的那个粗略判定，告示牌、蛋糕之类的存活要用它。
    pub const SOLID: u64 = 1 << 20;
    /// `liquid`，方块属性里的标记，不是“含有流体”。
    pub const LIQUID: u64 = 1 << 21;
    /// `canBeReplaced()`，不带放置上下文的那个重载。
    pub const REPLACEABLE: u64 = 1 << 22;
    /// `isRedstoneConductor`。
    pub const REDSTONE_CONDUCTOR: u64 = 1 << 23;
    /// `isCollisionShapeFullBlock`。
    pub const COLLISION_FULL_BLOCK: u64 = 1 << 24;
    /// `isSolidRender`。
    pub const SOLID_RENDER: u64 = 1 << 25;
    /// `isSignalSource`。
    pub const SIGNAL_SOURCE: u64 = 1 << 26;
    /// `hasAnalogOutputSignal`。
    pub const ANALOG_OUTPUT: u64 = 1 << 27;
    /// `propagatesSkylightDown`。
    pub const PROPAGATES_SKYLIGHT: u64 = 1 << 28;
    /// `isRandomlyTicking`：随机刻会调用这个状态的 `randomTick`。
    pub const RANDOMLY_TICKING: u64 = 1 << 29;
    /// `getFluidState().isRandomlyTicking`：随机刻会调用这一格的流体的 `randomTick`。
    pub const FLUID_RANDOMLY_TICKING: u64 = 1 << 30;
    /// 下面是一个不用形状挡光的完整方块时，`LightEngine.getLightDampeningInto(下面, 这个状态, UP, _)` 是 16：
    /// 这个状态用自己的形状挡光，而且挡光形状的底面是满的，比如下半台阶。
    pub const BLOCKS_LIGHT_FROM_BELOW: u64 = 1 << 31;
    /// `useShapeForLightOcclusion`，没有和 `canOcclude` 合并的原值。
    pub const USE_SHAPE_FOR_LIGHT_OCCLUSION: u64 = 1 << 32;
    /// `is(BlockTags.BLOCKS_MOTION_IN_HEIGHTMAP)`，`MOTION_BLOCKING` 和 `OCEAN_FLOOR` 用。
    pub const MOTION_BLOCKING: u64 = 1 << 33;
    /// `is(BlockTags.BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES)`，`MOTION_BLOCKING_NO_LEAVES` 用。
    pub const MOTION_BLOCKING_NO_LEAVES: u64 = 1 << 34;
}

/// 客户端和服务端都认识的内容包。双方都有时，注册表内容就不需要通过网络传输。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownPack {
    pub namespace: &'static str,
    pub id: &'static str,
    pub version: &'static str,
}

/// 需要同步给客户端的注册表。条目的位置就是它的数字 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncedRegistry {
    pub name: &'static str,
    pub entries: &'static [&'static str],
}

/// 一个注册表下的所有标签。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TagGroup {
    pub registry: &'static str,
    pub tags: &'static [Tag],
}

/// 一个标签和它包含的条目 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag {
    pub name: &'static str,
    pub ids: &'static [i32],
}

/// 官方方块标签的包装。常量在 [`tags::block_tags`]，`index` 是它在 [`tags::TAG_GROUPS`] 这一组里的下标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockTag {
    pub name: &'static str,
    pub index: usize,
}

/// 官方物品标签的包装，常量在 [`tags::item_tags`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemTag {
    pub name: &'static str,
    pub index: usize,
}

/// 官方流体标签的包装，常量在 [`tags::fluid_tags`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FluidTag {
    pub name: &'static str,
    pub index: usize,
}

/// 方块的一个属性，顺序和官方 `StateDefinition` 相同。
///
/// 方块的状态 ID 是这些属性取值的笛卡尔积，最后一个属性变化最快，生成器校验过这条规律。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertyDef {
    pub name: &'static str,
    pub values: &'static [&'static str],
    pub kind: PropertyKind,
}

/// 方块属性的官方类型，以及 `StatePropertiesPredicate` 做区间比较时用的顺序。
///
/// 官方的 `Property` 只有三个子类，属性值的 `compareTo` 各按各的顺序比：
/// 整数按数值，布尔 `false < true`，枚举按枚举常量的声明序号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyKind {
    /// `IntegerProperty`：取值是 `min..=max` 的连续整数，按数值比较。
    Integer { min: i32, max: i32 },
    /// `BooleanProperty`：取值列表是 `[true, false]`，比较时 `false < true`。
    Boolean,
    /// `EnumProperty`：取值列表可能是常量的乱序子集，比较时用常量的声明序号。
    ///
    /// 下标和 [`PropertyDef::values`] 一一对应。
    Enum { ordinals: &'static [u16] },
}

/// 一个构造参数或者只读方法的结果，名字和官方的字段名相同，方法名后面带括号。
///
/// 记录类型的字段展开成多项：`type.canOpenByHand`、`type.setType.name`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockField {
    pub name: &'static str,
    pub value: FieldValue,
}

/// [`BlockField`] 的值。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldValue {
    Bool(bool),
    Int(i32),
    Float(f32),
    Double(f64),
    /// 方块的注册表 ID。官方的 `ResourceKey<Block>` 也转成这个。
    Block(i32),
    /// 方块状态 ID。
    State(u32),
    /// 物品的注册表 ID。
    Item(i32),
    /// `Direction` 的 ID。
    Direction(i32),
    Tag(BlockTag),
    /// 枚举常量的小写名字，或者 `BlockSetType`、`WoodType` 的名字。
    Name(&'static str),
}

// 表里的浮点数都来自官方的常量，没有 NaN。
impl Eq for FieldValue {}

/// 方块的渲染偏移（`OffsetType` 和它的两个上限）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockOffset {
    /// `XYZ`，否则是 `XZ`。
    pub vertical: bool,
    pub max_horizontal: f32,
    pub max_vertical: f32,
    /// 形状跟着偏移走。形状表里存的是偏移之前的形状，要按坐标加上偏移才是真实形状。
    pub moves_shape: bool,
}

impl Eq for BlockOffset {}

/// 一个流体状态，`FluidState` 的事实。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidFacts {
    /// 流体注册表 ID，见 [`registries::fluid`]。
    pub fluid: i32,
    pub source: bool,
    /// `getAmount`。
    pub amount: u8,
    /// 流体的 `falling` 属性。
    pub falling: bool,
    /// `createLegacyBlock` 的状态 ID。
    pub legacy_block: u32,
}

/// 活塞推动时的反应，官方 `PushReaction`，顺序和官方一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PushReaction {
    /// 跟着活塞走，也能被拉回来。
    PushPull,
    /// 能推，不能拉。
    Push,
    /// 推的时候直接掉下来。
    Popped,
    /// 推不动。
    Immoveable,
    /// 只对实体有意义，方块不会碰到。
    IgnoreEntity,
}

/// 一个方块。除了状态 ID，其余字段都描述默认状态。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    pub name: &'static str,
    /// 第一个状态的 ID。这个方块的状态一直连续到下一个方块的 `first_state` 之前。
    pub first_state: u32,
    pub default_state: u32,
    /// 对应物品的注册表 ID，没有物品时是 [`items::AIR`]。挂墙变体对应主方块的物品。
    pub item: i32,
    /// 放置时的碰撞形状不为空，有碰撞的方块不能放在实体身上。
    pub has_collision: bool,
    /// 含水或者本身是流体。
    pub has_fluid: bool,
    /// 官方放下时占两格：有上下两半（`DoubleBlockHalf`）或者床头床尾（`BedPart`）属性。
    ///
    /// `pitcher_crop` 不算，它有 `half` 属性，但放下时只占一格，长大后才有上半。
    pub two_blocks: bool,
    /// 活塞推动时的反应，`state.getPistonPushReaction()`。
    pub push_reaction: PushReaction,
    /// 破坏速度是 -1：活塞不会推动，也不会打碎它。
    pub unbreakable: bool,
    /// 方块实体类型的 ID，见 [`registries::block_entity_type`]。
    pub block_entity_type: Option<i32>,
    /// `block.getClass().getSimpleName()`。
    pub class: BlockClass,
    /// 按官方 `StateDefinition` 的顺序。
    pub properties: &'static [PropertyDef],
    /// 官方类的构造参数，按名字排序。
    pub fields: &'static [BlockField],
    /// `NoteBlockInstrument` 的序列化名，`state.instrument()`。
    pub instrument: &'static str,
    /// 有渲染偏移时才有。
    pub offset: Option<BlockOffset>,
    /// `hasDynamicShape`：官方不缓存这种方块的形状，读取时要带上世界和位置。
    pub dynamic_shape: bool,
    /// `defaultDestroyTime`，也就是硬度。`-1` 表示挖不动。
    pub destroy_time: f32,
    /// `requiresCorrectToolForDrops`：不带正确的工具时挖得慢，而且没有掉落物。
    pub requires_correct_tool_for_drops: bool,
}

// 表里的浮点数都来自官方的常量，没有 NaN。
impl Eq for Block {}

impl Block {
    /// 官方类的字段或者方法结果。名字写错时返回 `None`，见 [`BlockField`]。
    pub fn field(&self, name: &str) -> Option<FieldValue> {
        self.fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.value)
    }
}

/// 官方 `Item` 的类：方块物品走哪条放置路径，非方块物品的 `useOn` 走哪条路。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// `BlockItem`：放置对应的方块。
    Block,
    /// `StandingAndWallBlockItem`：官方按视线在主方块和挂墙变体之间选。
    StandingAndWall,
    /// `BannerItem`：和 [`StandingAndWall`](Self::StandingAndWall) 相同。
    Banner,
    /// `HangingSignItem`：和 [`StandingAndWall`](Self::StandingAndWall) 相同。
    HangingSign,
    /// `PlayerHeadItem`：和 [`StandingAndWall`](Self::StandingAndWall) 相同。
    PlayerHead,
    /// `DoubleHighBlockItem`：官方放置前先清空上方一格。
    DoubleHigh,
    /// `PlaceOnWaterBlockItem`：只能放在水面上，点方块时不放置。
    PlaceOnWater,
    /// `GameMasterBlockItem`：要 gamemaster 权限才能放置。
    GameMaster,
    /// `ScaffoldingBlockItem`：有自己的目标位置算法。
    Scaffolding,
    /// `SolidBucketItem`：`powder_snow_bucket`。
    SolidBucket,
    /// `FlintAndSteelItem`：点燃营火、蜡烛，或者在点中的面前生火。
    FlintAndSteel,
    /// `FireChargeItem`：和打火石相同，写方块的标记不同。
    FireCharge,
    /// `HoneycombItem`：给铜涂蜡。
    Honeycomb,
    /// `ShearsItem`：让 `GrowingPlantHeadBlock` 停止生长。
    Shears,
    /// `EnderEyeItem`：装进末地传送门框架。
    EnderEye,
    /// `DebugStickItem`：要 gamemaster 权限。
    DebugStick,
    /// `BrushItem`：开始刷，结果在后面的使用刻里。
    Brush,
    /// `BoneMealItem`：催熟能长的方块，或者在水里长海草。
    BoneMeal,
    /// 别的非方块物品，`useOn` 只看 [`Item::block_transformer`]。
    Other,
}

/// 一个物品。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub name: &'static str,
    pub kind: ItemKind,
    /// 放置的方块的注册表 ID，只有方块物品才有。
    pub block: Option<i32>,
    /// `StandingAndWallBlockItem.wallBlock`：挂墙变体的方块注册表 ID。
    pub wall_block: Option<i32>,
    /// `StandingAndWallBlockItem.attachmentDirection` 的 ID：主方块朝这个方向贴着支撑。
    pub attachment: Option<i32>,
    pub max_stack_size: i32,
    /// `max_damage` 组件，没有这个组件时物品不能受损。
    ///
    /// The `max_damage` component. The item cannot take damage when this is absent.
    pub max_damage: Option<i32>,
    /// `enchantable` 组件的 `value`，附魔台算等级时用；没有这个组件就不能附魔。
    ///
    /// The `value` of the `enchantable` component. Enchanting cost uses it; items without
    /// the component cannot be enchanted.
    pub enchantable: Option<i32>,
    /// 手持这个物品时，创造模式玩家能不能破坏方块。
    pub can_destroy_blocks_in_creative: bool,
    /// 有 `consumable` 组件：放置失败时官方会开始吃。
    pub consumable: bool,
    /// `compostable` 组件：能放进堆肥桶，放一个加几层。
    pub compostable: Option<CompostLayers>,
    /// 有 `jukebox_playable` 组件。
    pub jukebox_playable: bool,
    /// `block_transformer` 组件：斧子、锄头、锹右键时改方块的规则。
    pub block_transformer: Option<&'static BlockTransformer>,
    /// 有 `blocks_attacks` 组件，也就是盾牌。拿在副手时主手的 `block_transformer` 不起作用。
    pub blocks_attacks: bool,
}

/// `compostable` 组件的 `layers`，数据包 `context_int_provider/compostable` 里的整数提供器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompostLayers {
    /// 固定加这么多层，不取随机数。
    Constant(i32),
    /// 堆肥桶空着时加 `when_empty` 层；否则 `nextInt(总权重)` 按权重抽一项，元素是（层数，权重）。
    Weighted {
        when_empty: i32,
        distribution: &'static [(i32, i32)],
    },
}

/// 数据驱动的 `BlockTransformer`，条目在 [`block_transformers`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockTransformer {
    pub name: &'static str,
    /// 按顺序试，第一项给出新状态时就用它。
    pub transforms: &'static [BlockTransform],
}

/// `BlockTransformer.BlockTransformData` 里改方块的部分。音效、粒子、耐久和掉落物不在这里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockTransform {
    pub provider: StateProvider,
    /// `disallowedFaces`：点中这些面时跳过这一项，是 `Direction` 的 ID。
    pub disallowed_faces: &'static [i32],
    /// `updateFromNeighbors`：写入前先按六个邻居更新一遍状态。
    pub update_from_neighbors: bool,
}

/// 官方的 `BlockStateProvider`，只有官方数据用到的几种，生成器遇到别的会报错。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateProvider {
    /// `SimpleStateProvider`：固定的状态 ID。
    Simple(u32),
    /// `CopyPropertiesProvider`：`source` 给的状态，复制上这一格现有状态的属性。
    CopyProperties(&'static StateProvider),
    /// `RuleBasedStateProvider`：第一条成立并且给出状态的规则，都不成立时问 `fallback`。
    RuleBased {
        rules: &'static [StateRule],
        fallback: Option<&'static StateProvider>,
    },
}

/// `RuleBasedStateProvider.Rule`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRule {
    pub if_true: BlockPredicate,
    pub then: StateProvider,
}

/// 官方的 `BlockPredicate`，只有官方数据用到的几种。`offset` 是相对被判定位置的偏移。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockPredicate {
    /// `matching_blocks`：那一格是这些方块之一，都是方块注册表 ID。
    MatchingBlocks {
        offset: [i32; 3],
        blocks: &'static [i32],
    },
    /// `matching_block_tag`。
    MatchingBlockTag { offset: [i32; 3], tag: BlockTag },
    /// `all_of`。
    AllOf(&'static [BlockPredicate]),
}

/// 查找一个状态所属的方块，状态 ID 不存在时返回 `None`。
pub fn block_of_state(state: u32) -> Option<&'static Block> {
    blocks::BLOCKS.get(block_id_of_state(state)?)
}

/// 一个状态所属方块的注册表 ID，状态 ID 不存在时返回 `None`。
pub fn block_id_of_state(state: u32) -> Option<usize> {
    if state >= blocks::TOTAL_STATES {
        return None;
    }
    // 生成器保证了状态 ID 从 0 开始，按方块 ID 连续递增。
    let next = blocks::BLOCKS.partition_point(|block| block.first_state <= state);
    next.checked_sub(1)
}

/// 一个方块有多少个状态。
pub fn state_count(block_id: usize) -> u32 {
    let next = blocks::BLOCKS
        .get(block_id + 1)
        .map_or(blocks::TOTAL_STATES, |block| block.first_state);
    next - blocks::BLOCKS[block_id].first_state
}

/// 状态的事实，位的含义见 [`facts`]。状态 ID 不存在时 panic。
pub fn state_facts(state: u32) -> u64 {
    states::STATE_FACTS[state as usize]
}

/// 状态的流体。状态 ID 不存在时 panic。
pub fn state_fluid(state: u32) -> &'static FluidFacts {
    &states::FLUID_STATES[usize::from(states::STATE_FLUID[state as usize])]
}

/// 状态的 `getLightDampening`。状态 ID 不存在时 panic。
pub fn light_dampening(state: u32) -> u8 {
    states::LIGHT_DAMPENING[state as usize]
}

/// 状态的 `getLightEmission`。状态 ID 不存在时 panic。
pub fn light_emission(state: u32) -> u8 {
    light::LIGHT_EMISSION[state as usize]
}

/// 光照引擎看到的 6 个面的遮挡形状编号，官方的 `LightEngine.getOcclusionShape`，按 `Direction` 的 ID 排列。
///
/// 编号 0 是空形状，1 是完整方块，两个编号之间能不能挡住光看 [`face_shape_occludes`]。状态 ID 不存在时 panic。
pub fn light_occlusion_faces(state: u32) -> &'static [u16; 6] {
    &light::OCCLUSION_FACES[usize::from(light::LIGHT_OCCLUSION[state as usize])]
}

/// 官方的 `Shapes.faceShapeOccludes`：两个面形状合起来盖满整个面。参数是 [`light_occlusion_faces`] 的编号。
pub fn face_shape_occludes(shape: u16, occluder: u16) -> bool {
    let bit = usize::from(shape) * light::FACE_SHAPE_COUNT + usize::from(occluder);
    light::FACE_OCCLUDES[bit / 64] >> (bit % 64) & 1 == 1
}

/// `CollisionContext.empty()` 下的碰撞形状。[`BlockOffset::moves_shape`] 的方块要自己加上偏移。
pub fn collision_shape(state: u32) -> &'static [Aabb] {
    shape_at(shapes::COLLISION_SHAPE[state as usize])
}

/// `CollisionContext.placementContext` 下的碰撞形状。
pub fn placement_collision_shape(state: u32) -> &'static [Aabb] {
    match shapes::PLACEMENT_COLLISION.binary_search_by_key(&state, |&(id, _)| id) {
        Ok(index) => shape_at(shapes::PLACEMENT_COLLISION[index].1),
        Err(_) => collision_shape(state),
    }
}

/// `getShape` 的形状，也就是选中框。
pub fn outline_shape(state: u32) -> &'static [Aabb] {
    shape_at(shapes::OUTLINE_SHAPE[state as usize])
}

/// `getBlockSupportShape` 的形状。
pub fn support_shape(state: u32) -> &'static [Aabb] {
    shape_at(shapes::SUPPORT_SHAPE[state as usize])
}

fn shape_at(index: u16) -> &'static [Aabb] {
    shapes::SHAPES[usize::from(index)]
}

/// 一个方块标签包含的方块注册表 ID，标签不存在时返回 `None`。
///
/// `name` 要带命名空间，比如 `minecraft:replaceable`。
pub fn block_tag(name: &str) -> Option<&'static [i32]> {
    let group = tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == BLOCK_TAG_REGISTRY)?;
    let tag = group.tags.iter().find(|tag| tag.name == name)?;
    Some(tag.ids)
}

/// 一个物品标签包含的物品注册表 ID，标签不存在时返回 `None`。
///
/// `name` 要带命名空间，比如 `minecraft:swords`。
pub fn item_tag(name: &str) -> Option<&'static [i32]> {
    let group = tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == ITEM_TAG_REGISTRY)?;
    let tag = group.tags.iter().find(|tag| tag.name == name)?;
    Some(tag.ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn short(name: &str) -> &str {
        name.strip_prefix("minecraft:").unwrap_or(name)
    }

    fn block(id: i32) -> &'static Block {
        &blocks::BLOCKS[usize::try_from(id).expect("注册表 ID 不是负数")]
    }

    fn item(id: i32) -> &'static Item {
        &items::ITEMS[usize::try_from(id).expect("注册表 ID 不是负数")]
    }

    /// 全部方块物品和它们放置的方块，规格里的数量都按这个口径统计。
    fn block_items() -> impl Iterator<Item = (&'static Item, &'static Block)> {
        items::ITEMS
            .iter()
            .filter_map(|item| Some((item, block(item.block?))))
    }

    #[test]
    fn describes_the_target_version() {
        assert_eq!(version::GAME_VERSION, "26.3");
        assert_eq!(version::PROTOCOL_VERSION, 777);
        assert_eq!(version::DATA_VERSION, 5023);
    }

    #[test]
    fn packet_ids_match_the_official_report() {
        assert_eq!(packets::login::clientbound::LOGIN_FINISHED, 2);
        assert_eq!(packets::configuration::clientbound::REGISTRY_DATA, 7);
        assert_eq!(packets::play::clientbound::LEVEL_CHUNK_WITH_LIGHT, 46);
    }

    #[test]
    fn air_is_block_state_zero() {
        assert_eq!(blocks::AIR, 0);
        const { assert!(blocks::GRASS_BLOCK < blocks::TOTAL_STATES) };
    }

    #[test]
    fn every_entry_list_is_filled() {
        assert!(!registries::SYNCED_REGISTRIES.is_empty());
        assert!(
            registries::SYNCED_REGISTRIES
                .iter()
                .all(|registry| !registry.name.is_empty())
        );
        const { assert!(registries::biome::TOTAL > 0) };
        assert_eq!(
            registries::KNOWN_PACKS,
            [KnownPack {
                namespace: "minecraft",
                id: "core",
                version: "26.3",
            }]
        );
    }

    #[test]
    fn static_and_dynamic_registries_have_tags() {
        let has_group = |name| tags::TAG_GROUPS.iter().any(|group| group.registry == name);
        assert!(has_group("minecraft:block"));
        assert!(has_group("minecraft:item"));
        assert!(has_group("minecraft:worldgen/biome"));
    }

    #[test]
    fn items_are_indexed_by_registry_id() {
        assert_eq!(items::ITEMS.len(), 1658);
        assert_eq!(items::ITEMS[0].name, "minecraft:air");
        assert_eq!(items::ITEMS[1].name, "minecraft:stone");
        assert_eq!(items::AIR, 0);
        assert_eq!(items::STONE, 1);
        assert_eq!(item(items::OMINOUS_BOTTLE).name, "minecraft:ominous_bottle");
    }

    #[test]
    fn item_kinds_follow_the_official_classes() {
        let count = |kind| items::ITEMS.iter().filter(|item| item.kind == kind).count();
        assert_eq!(count(ItemKind::Block), 1038);
        assert_eq!(count(ItemKind::StandingAndWall), 33);
        assert_eq!(count(ItemKind::Banner), 16);
        assert_eq!(count(ItemKind::HangingSign), 13);
        assert_eq!(count(ItemKind::PlayerHead), 1);
        assert_eq!(count(ItemKind::DoubleHigh), 29);
        assert_eq!(count(ItemKind::PlaceOnWater), 2);
        assert_eq!(count(ItemKind::GameMaster), 7);
        assert_eq!(count(ItemKind::Scaffolding), 1);
        assert_eq!(count(ItemKind::SolidBucket), 1);
        assert_eq!(count(ItemKind::Brush), 1);
        assert_eq!(count(ItemKind::BoneMeal), 1);
        assert_eq!(count(ItemKind::Other), 509);

        assert_eq!(item(items::LILY_PAD).kind, ItemKind::PlaceOnWater);
        assert_eq!(item(items::COMMAND_BLOCK).kind, ItemKind::GameMaster);
        assert_eq!(item(items::POWDER_SNOW_BUCKET).kind, ItemKind::SolidBucket);
        assert_eq!(item(items::FLINT_AND_STEEL).kind, ItemKind::FlintAndSteel);
        assert_eq!(item(items::FIRE_CHARGE).kind, ItemKind::FireCharge);
        assert_eq!(item(items::HONEYCOMB).kind, ItemKind::Honeycomb);
        assert_eq!(item(items::SHEARS).kind, ItemKind::Shears);
        assert_eq!(item(items::ENDER_EYE).kind, ItemKind::EnderEye);
        assert_eq!(item(items::DEBUG_STICK).kind, ItemKind::DebugStick);
        assert_eq!(item(items::BRUSH).kind, ItemKind::Brush);
        assert_eq!(item(items::BONE_MEAL).kind, ItemKind::BoneMeal);
    }

    #[test]
    fn only_block_items_have_a_block() {
        const NOT_BLOCK_ITEMS: [ItemKind; 9] = [
            ItemKind::FlintAndSteel,
            ItemKind::FireCharge,
            ItemKind::Honeycomb,
            ItemKind::Shears,
            ItemKind::EnderEye,
            ItemKind::DebugStick,
            ItemKind::Brush,
            ItemKind::BoneMeal,
            ItemKind::Other,
        ];
        assert_eq!(block_items().count(), 1141);
        for item in items::ITEMS {
            assert_eq!(
                item.block.is_none(),
                NOT_BLOCK_ITEMS.contains(&item.kind),
                "{}",
                item.name
            );
        }
    }

    #[test]
    fn tools_point_at_their_block_transformer() {
        let transformer = |id| item(id).block_transformer.map(|found| found.name);
        assert_eq!(transformer(items::WOODEN_AXE), Some("minecraft:axe"));
        assert_eq!(transformer(items::NETHERITE_HOE), Some("minecraft:hoe"));
        assert_eq!(transformer(items::STONE_SHOVEL), Some("minecraft:shovel"));
        assert_eq!(transformer(items::SHEARS), None);
        let with_transformer = items::ITEMS
            .iter()
            .filter(|item| item.block_transformer.is_some())
            .count();
        assert_eq!(with_transformer, 21);
    }

    #[test]
    fn only_the_shield_blocks_attacks() {
        let blocking: Vec<_> = items::ITEMS
            .iter()
            .filter(|item| item.blocks_attacks)
            .map(|item| item.name)
            .collect();
        assert_eq!(blocking, ["minecraft:shield"]);
    }

    #[test]
    fn the_shovel_makes_paths_only_under_air() {
        let [path] = block_transformers::SHOVEL.transforms else {
            panic!("the shovel has one transform");
        };
        assert_eq!(path.disallowed_faces, [0]);
        assert!(path.update_from_neighbors);
        let StateProvider::RuleBased { rules, fallback } = path.provider else {
            panic!("the shovel uses rules");
        };
        assert_eq!(fallback, None);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].then, StateProvider::Simple(blocks::DIRT_PATH));
        assert_eq!(
            rules[0].if_true,
            BlockPredicate::AllOf(&[
                BlockPredicate::MatchingBlockTag {
                    offset: [0, 0, 0],
                    tag: tags::block_tags::TURNS_INTO_DIRT_PATH,
                },
                BlockPredicate::MatchingBlockTag {
                    offset: [0, 1, 0],
                    tag: tags::block_tags::AIR,
                },
            ])
        );
    }

    #[test]
    fn fourteen_items_place_a_block_with_another_name() {
        let mut found: Vec<_> = block_items()
            .filter(|(item, block)| item.name != block.name)
            .map(|(item, block)| (short(item.name), short(block.name)))
            .collect();
        found.sort_unstable();

        let mut expected = [
            ("redstone", "redstone_wire"),
            ("string", "tripwire"),
            ("wheat_seeds", "wheat"),
            ("powder_snow_bucket", "powder_snow"),
            ("cocoa_beans", "cocoa"),
            ("pumpkin_seeds", "pumpkin_stem"),
            ("melon_seeds", "melon_stem"),
            ("carrot", "carrots"),
            ("potato", "potatoes"),
            ("torchflower_seeds", "torchflower_crop"),
            ("pitcher_pod", "pitcher_crop"),
            ("beetroot_seeds", "beetroots"),
            ("sweet_berries", "sweet_berry_bush"),
            ("glow_berries", "cave_vines"),
        ];
        expected.sort_unstable();
        assert_eq!(found, expected);
    }

    #[test]
    fn ten_items_cannot_destroy_blocks_in_creative() {
        let mut found: Vec<_> = items::ITEMS
            .iter()
            .filter(|item| !item.can_destroy_blocks_in_creative)
            .map(|item| short(item.name))
            .collect();
        found.sort_unstable();

        let mut expected = [
            "wooden_sword",
            "stone_sword",
            "copper_sword",
            "golden_sword",
            "iron_sword",
            "diamond_sword",
            "netherite_sword",
            "mace",
            "trident",
            "debug_stick",
        ];
        expected.sort_unstable();
        assert_eq!(found, expected);
    }

    #[test]
    fn max_stack_size_comes_from_the_components() {
        assert_eq!(item(items::STONE).max_stack_size, 64);
        assert_eq!(item(items::ENDER_PEARL).max_stack_size, 16);
        assert_eq!(item(items::DIAMOND_SWORD).max_stack_size, 1);
    }

    #[test]
    fn compostable_comes_from_the_components() {
        let compostable = items::ITEMS
            .iter()
            .filter(|item| item.compostable.is_some());
        assert_eq!(compostable.count(), 122);
        assert!(item(items::OAK_LEAVES).compostable.is_some());
        assert!(item(items::STONE).compostable.is_none());
        assert!(item(items::AIR).compostable.is_none());
        assert_eq!(
            item(items::WHEAT_SEEDS).compostable,
            Some(CompostLayers::Weighted {
                when_empty: 1,
                distribution: &[(1, 30), (0, 70)],
            })
        );
        assert_eq!(
            item(items::CAKE).compostable,
            Some(CompostLayers::Constant(1))
        );
    }

    #[test]
    fn component_type_ids_match_the_official_bytes() {
        // `blocks-inventory.md` 2.5 的实测字节里，`max_stack_size` 是 1，`rarity` 是 12。
        assert_eq!(registries::data_component_type::MAX_STACK_SIZE, 1);
        assert_eq!(registries::data_component_type::RARITY, 12);
        assert_eq!(registries::data_component_type::ENCHANTMENTS, 13);
    }

    #[test]
    fn blocks_are_indexed_by_registry_id() {
        assert_eq!(blocks::BLOCKS.len(), 1286);
        assert_eq!(blocks::BLOCKS[0].name, "minecraft:air");
        assert_eq!(blocks::BLOCKS[1].name, "minecraft:stone");
        assert_eq!(blocks::BLOCKS[1].default_state, blocks::STONE);
        assert_eq!(item(items::STONE).block, Some(1));
    }

    #[test]
    fn a_block_without_an_item_points_at_air() {
        let without_item = blocks::BLOCKS
            .iter()
            .filter(|block| block.item == items::AIR && block.name != "minecraft:air")
            .count();
        assert_eq!(without_item, 77);
        assert_eq!(
            block_of_state(blocks::WATER).map(|block| block.item),
            Some(items::AIR)
        );
    }

    #[test]
    fn wall_variants_use_the_item_of_the_main_block() {
        assert_eq!(
            block_of_state(blocks::WALL_TORCH).map(|block| block.item),
            Some(items::TORCH)
        );
    }

    #[test]
    fn collision_uses_the_placement_context() {
        let without_collision = blocks::BLOCKS
            .iter()
            .filter(|block| !block.has_collision)
            .count();
        assert_eq!(without_collision, 253);

        let has_collision = |state| block_of_state(state).map(|block| block.has_collision);
        assert_eq!(has_collision(blocks::STONE), Some(true));
        assert_eq!(has_collision(blocks::AIR), Some(false));
        // 普通上下文里脚手架有碰撞，放置时没有，所以可以放在玩家身上。
        assert_eq!(has_collision(blocks::SCAFFOLDING), Some(false));
    }

    #[test]
    fn two_blocks_covers_doors_beds_and_tall_plants() {
        let found: Vec<_> = block_items()
            .filter(|(_, block)| block.two_blocks)
            .map(|(item, _)| short(item.name))
            .collect();
        let doors = found.iter().filter(|name| name.ends_with("_door")).count();
        let beds = found.iter().filter(|name| name.ends_with("_bed")).count();
        assert_eq!(doors, 22);
        assert_eq!(beds, 17);

        // 规格第 10 章的 47 种。pitcher_pod 不在里面：pitcher_crop 有 half 属性，
        // 但默认状态 age=0 只占一格，官方放下时也不放上半。
        assert_eq!(found.len(), 47);
        let others: Vec<_> = found
            .into_iter()
            .filter(|name| !name.ends_with("_door") && !name.ends_with("_bed"))
            .collect();
        assert_eq!(
            others,
            [
                "pitcher_plant",
                "small_dripleaf",
                "sunflower",
                "lilac",
                "rose_bush",
                "peony",
                "tall_grass",
                "large_fern",
            ]
        );
    }

    #[test]
    fn has_fluid_covers_the_blocks_that_are_waterlogged_by_default() {
        let found: Vec<_> = block_items()
            .filter(|(_, block)| block.has_fluid)
            .map(|(item, _)| short(item.name))
            .collect();
        assert_eq!(found.len(), 24);
        assert!(found.contains(&"seagrass"));
        assert!(found.contains(&"conduit"));
        assert!(found.contains(&"dead_tube_coral_fan"));
        assert!(!found.contains(&"stone"));
    }

    #[test]
    fn block_entity_types_use_the_registry_ids() {
        let with_block_entity = blocks::BLOCKS
            .iter()
            .filter(|block| block.block_entity_type.is_some())
            .count();
        assert_eq!(with_block_entity, 191);

        let block_entity_type =
            |state| block_of_state(state).and_then(|block| block.block_entity_type);
        assert_eq!(
            block_entity_type(blocks::CHEST),
            Some(registries::block_entity_type::CHEST)
        );
        assert_eq!(
            block_entity_type(blocks::OAK_WALL_SIGN),
            Some(registries::block_entity_type::SIGN)
        );
        assert_eq!(block_entity_type(blocks::STONE), None);
        // 26.3 的床已经不是方块实体了。
        assert_eq!(block_entity_type(blocks::RED_BED), None);
    }

    #[test]
    fn push_reactions_and_unbreakable_blocks_come_from_the_block_properties() {
        let block = |state| block_of_state(state).expect("状态属于某个方块");
        assert_eq!(
            block(blocks::PISTON).push_reaction,
            PushReaction::Immoveable
        );
        assert_eq!(
            block(blocks::STICKY_PISTON).push_reaction,
            PushReaction::Immoveable
        );
        assert_eq!(
            block(blocks::SLIME_BLOCK).push_reaction,
            PushReaction::PushPull
        );
        assert_eq!(
            block(blocks::OAK_LEAVES).push_reaction,
            PushReaction::Popped
        );
        assert_eq!(block(blocks::WATER).push_reaction, PushReaction::Popped);
        assert_eq!(block(blocks::STONE).push_reaction, PushReaction::PushPull);

        assert!(block(blocks::BEDROCK).unbreakable);
        assert!(block(blocks::MOVING_PISTON).unbreakable);
        assert!(!block(blocks::PISTON).unbreakable);
        assert!(!block(blocks::OBSIDIAN).unbreakable);
    }

    #[test]
    fn state_ids_are_contiguous_and_increase_with_the_block_id() {
        assert_eq!(blocks::BLOCKS[0].first_state, 0);
        for pair in blocks::BLOCKS.windows(2) {
            assert!(pair[0].first_state < pair[1].first_state);
            assert!((pair[0].first_state..pair[1].first_state).contains(&pair[0].default_state));
        }
    }

    #[test]
    fn finds_the_block_at_both_ends_of_its_states() {
        for pair in blocks::BLOCKS.windows(2) {
            assert_eq!(block_of_state(pair[0].first_state), Some(&pair[0]));
            assert_eq!(block_of_state(pair[0].default_state), Some(&pair[0]));
            assert_eq!(block_of_state(pair[1].first_state - 1), Some(&pair[0]));
        }
    }

    #[test]
    fn state_outside_the_table_has_no_block() {
        assert_eq!(
            block_of_state(blocks::TOTAL_STATES - 1),
            blocks::BLOCKS.last()
        );
        assert_eq!(block_of_state(blocks::TOTAL_STATES), None);
        assert_eq!(block_of_state(u32::MAX), None);
    }

    #[test]
    fn every_block_has_a_class_and_its_properties() {
        assert_eq!(block(1).class, BlockClass::Block);
        let slab = block_of_state(blocks::OAK_SLAB).expect("橡木台阶是官方方块");
        assert_eq!(slab.class, BlockClass::SlabBlock);
        let names: Vec<_> = slab
            .properties
            .iter()
            .map(|property| property.name)
            .collect();
        assert_eq!(names, ["type", "waterlogged"]);
        assert_eq!(slab.properties[0].values, ["top", "bottom", "double"]);
        assert_eq!(slab.properties[1].values, ["true", "false"]);
    }

    #[test]
    fn state_count_is_the_product_of_the_properties() {
        for (id, block) in blocks::BLOCKS.iter().enumerate() {
            let product: usize = block
                .properties
                .iter()
                .map(|property| property.values.len())
                .product();
            assert_eq!(state_count(id) as usize, product, "{}", block.name);
        }
    }

    #[test]
    fn ancestors_cover_interfaces_and_parent_classes() {
        let ancestors = BlockClass::PoweredRailBlock.ancestors();
        assert!(ancestors.contains(&BlockAncestor::BaseRailBlock));
        assert!(ancestors.contains(&BlockAncestor::SimpleWaterloggedBlock));
        assert!(
            BlockClass::CommandBlock
                .ancestors()
                .contains(&BlockAncestor::GameMasterBlock)
        );
        assert!(BlockClass::Block.ancestors().is_empty());
    }

    #[test]
    fn facts_describe_the_well_known_blocks() {
        let stone = state_facts(blocks::STONE);
        assert_eq!(stone & facts::STURDY_MASK, facts::STURDY_MASK);
        assert_eq!(stone & facts::AIR, 0);
        assert_ne!(stone & facts::SOLID, 0);
        assert_ne!(stone & facts::SOLID_RENDER, 0);
        assert_ne!(stone & facts::REDSTONE_CONDUCTOR, 0);
        assert_ne!(stone & facts::COLLISION_FULL_BLOCK, 0);
        assert_ne!(stone & facts::CAN_OCCLUDE, 0);

        let air = state_facts(blocks::AIR);
        assert_ne!(air & facts::AIR, 0);
        assert_ne!(air & facts::REPLACEABLE, 0);
        assert_eq!(air & facts::STURDY_MASK, 0);

        assert_ne!(state_facts(blocks::WATER) & facts::LIQUID, 0);
        assert_ne!(state_facts(blocks::LEVER) & facts::SIGNAL_SOURCE, 0);
        assert_ne!(state_facts(blocks::COMPARATOR) & facts::SIGNAL_SOURCE, 0);
        assert_ne!(state_facts(blocks::COMPOSTER) & facts::ANALOG_OUTPUT, 0);
    }

    #[test]
    fn fluid_table_follows_the_official_states() {
        assert_eq!(state_fluid(blocks::STONE).fluid, registries::fluid::EMPTY);
        let water = state_fluid(blocks::WATER);
        assert_eq!(water.fluid, registries::fluid::WATER);
        assert!(water.source);
        assert_eq!(water.amount, 8);
        assert_eq!(water.legacy_block, blocks::WATER);
        // 含水方块被破坏后留下的就是这个状态。
        assert_eq!(state_fluid(blocks::SEAGRASS).legacy_block, blocks::WATER);
    }

    #[test]
    fn light_dampening_comes_from_the_official_state() {
        assert_eq!(light_dampening(blocks::STONE), 15);
        assert_eq!(light_dampening(blocks::AIR), 0);
        assert_eq!(light_dampening(blocks::WATER), 1);
    }

    #[test]
    fn shape_tables_map_every_state() {
        assert_eq!(shapes::SHAPES[0], &[]);
        assert_eq!(
            shapes::SHAPES[1],
            &[Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)]
        );
        assert!(collision_shape(blocks::AIR).is_empty());
        assert_eq!(collision_shape(blocks::STONE), shapes::SHAPES[1]);
        assert_eq!(
            outline_shape(blocks::OAK_SLAB),
            &[Aabb::new(0.0, 0.0, 0.0, 1.0, 0.5, 1.0)]
        );
        // 普通上下文里站得住的脚手架，放置时不挡路。
        assert!(!collision_shape(blocks::SCAFFOLDING).is_empty());
        assert!(placement_collision_shape(blocks::SCAFFOLDING).is_empty());
        assert_eq!(
            placement_collision_shape(blocks::STONE),
            collision_shape(blocks::STONE)
        );
    }

    #[test]
    fn offset_shapes_are_stored_before_the_offset() {
        let bamboo = block_of_state(blocks::BAMBOO).expect("竹子是官方方块");
        let offset = bamboo.offset.expect("竹子有渲染偏移");
        assert!(offset.moves_shape);
        assert_eq!(offset.max_horizontal, 0.25);
        assert_eq!(
            collision_shape(blocks::BAMBOO),
            &[Aabb::new(0.40625, 0.0, 0.40625, 0.59375, 1.0, 0.59375)]
        );
        // 高草的偏移只影响渲染。
        let grass = block_of_state(blocks::TALL_GRASS).expect("高草是官方方块");
        assert!(!grass.offset.expect("高草有渲染偏移").moves_shape);
    }

    #[test]
    fn constructor_parameters_are_readable_by_name() {
        let door = block_of_state(blocks::IRON_DOOR).expect("铁门是官方方块");
        assert_eq!(
            door.field("type.canOpenByHand"),
            Some(FieldValue::Bool(false))
        );
        assert_eq!(door.field("no_such_field"), None);

        let coral = block_of_state(blocks::TUBE_CORAL_BLOCK).expect("珊瑚块是官方方块");
        let dead = block_of_state(blocks::DEAD_TUBE_CORAL_BLOCK).expect("死珊瑚块是官方方块");
        let Some(FieldValue::Block(id)) = coral.field("deadBlock") else {
            panic!("珊瑚块要有 deadBlock");
        };
        assert_eq!(block(id), dead);

        let kelp = block_of_state(blocks::KELP).expect("海带是官方方块");
        let plant = block_of_state(blocks::KELP_PLANT).expect("海带茎是官方方块");
        let Some(FieldValue::Block(body)) = kelp.field("getBodyBlock()") else {
            panic!("海带头要有 getBodyBlock");
        };
        assert_eq!(block(body), plant);

        let stem = block_of_state(blocks::PUMPKIN_STEM).expect("南瓜茎是官方方块");
        assert_eq!(
            stem.field("fruitSupportBlocks"),
            Some(FieldValue::Tag(
                tags::block_tags::SUPPORTS_PUMPKIN_STEM_FRUIT
            ))
        );
    }

    #[test]
    fn block_maps_hold_the_official_pairs() {
        let id_of = |state| block_id_of_state(state).expect("官方状态") as i32;
        assert!(block_maps::FLOWER_POTS.contains(&(id_of(blocks::AIR), id_of(blocks::FLOWER_POT))));
        assert!(
            block_maps::FLOWER_POTS.contains(&(id_of(blocks::POPPY), id_of(blocks::POTTED_POPPY)))
        );
        assert!(
            block_maps::CANDLE_CAKES.contains(&(id_of(blocks::CANDLE), id_of(blocks::CANDLE_CAKE)))
        );
        assert!(
            block_maps::WEATHERING
                .contains(&(id_of(blocks::COPPER_BLOCK), id_of(blocks::EXPOSED_COPPER)))
        );
        assert!(block_maps::WAXABLES.contains(&(
            id_of(blocks::COPPER_BLOCK),
            id_of(blocks::WAXED_COPPER_BLOCK)
        )));
        assert!(
            block_maps::FIRE_ODDS
                .iter()
                .any(|&(id, ignite, _)| id == id_of(blocks::OAK_PLANKS) && ignite > 0)
        );
    }

    #[test]
    fn wall_items_know_their_wall_block() {
        let torch = item(items::TORCH);
        assert_eq!(torch.kind, ItemKind::StandingAndWall);
        let wall = torch.wall_block.expect("火把有挂墙变体");
        assert_eq!(block(wall).default_state, blocks::WALL_TORCH);
        assert_eq!(torch.attachment, Some(0));
        assert_eq!(item(items::OAK_HANGING_SIGN).attachment, Some(1));
        assert_eq!(item(items::STONE).wall_block, None);
    }

    #[test]
    fn item_components_are_summarised() {
        assert!(item(items::SWEET_BERRIES).consumable);
        assert!(item(items::SWEET_BERRIES).compostable.is_some());
        assert!(!item(items::STONE).consumable);
        assert!(item(items::MUSIC_DISC_CAT).jukebox_playable);
    }

    #[test]
    fn tag_constants_point_at_their_group() {
        let group = &tags::TAG_GROUPS[tags::block_tags::GROUP];
        assert_eq!(group.registry, "minecraft:block");
        let tag = tags::block_tags::REPLACEABLE;
        assert_eq!(group.tags[tag.index].name, tag.name);
        assert_eq!(block_tag(tag.name), Some(group.tags[tag.index].ids));

        let items = &tags::TAG_GROUPS[tags::item_tags::GROUP];
        assert_eq!(
            items.tags[tags::item_tags::CANDLES.index].name,
            "minecraft:candles"
        );

        let fluids = &tags::TAG_GROUPS[tags::fluid_tags::GROUP];
        assert_eq!(
            fluids.tags[tags::fluid_tags::WATER.index].name,
            "minecraft:water"
        );
    }

    #[test]
    fn item_tags_resolve_by_name() {
        let swords = item_tag("minecraft:swords").expect("swords 是物品标签");
        assert!(!swords.is_empty());
        assert_eq!(
            swords,
            tags::TAG_GROUPS[tags::item_tags::GROUP]
                .tags
                .iter()
                .find(|tag| tag.name == "minecraft:swords")
                .expect("swords 在物品标签组里")
                .ids
        );
        assert!(item_tag("minecraft:no_such_tag").is_none());
    }

    #[test]
    fn builtin_registry_ids_match_the_official_report() {
        assert_eq!(registries::entity_type::PLAYER, 159);
        assert_eq!(registries::attribute::BLOCK_INTERACTION_RANGE, 8);
        assert_eq!(registries::attribute::ENTITY_INTERACTION_RANGE, 13);
        assert_eq!(registries::block_entity_type::CHEST, 1);
        assert_eq!(registries::block_entity_type::TOTAL, 49);
    }

    #[test]
    fn chat_types_follow_the_synced_registry() {
        let synced = registries::SYNCED_REGISTRIES
            .iter()
            .find(|registry| registry.name == "minecraft:chat_type")
            .expect("chat_type 是同步注册表");

        assert_eq!(registries::chat_type::CHAT, 0);
        assert_eq!(synced.entries[0], "minecraft:chat");
        assert_eq!(registries::chat_type::TOTAL as usize, synced.entries.len());
    }

    #[test]
    fn replaceable_tag_holds_the_thirty_blocks_of_the_spec() {
        let ids = block_tag("minecraft:replaceable").expect("这个标签是官方的");
        let mut found: Vec<_> = ids.iter().map(|&id| short(block(id).name)).collect();
        found.sort_unstable();

        let mut expected = [
            "air",
            "cave_air",
            "void_air",
            "water",
            "lava",
            "bubble_column",
            "fire",
            "soul_fire",
            "light",
            "structure_void",
            "short_grass",
            "tall_grass",
            "fern",
            "large_fern",
            "dead_bush",
            "bush",
            "red_shrub",
            "short_dry_grass",
            "tall_dry_grass",
            "seagrass",
            "tall_seagrass",
            "snow",
            "vine",
            "glow_lichen",
            "resin_clump",
            "warped_roots",
            "crimson_roots",
            "nether_sprouts",
            "leaf_litter",
            "hanging_roots",
        ];
        expected.sort_unstable();
        assert_eq!(found, expected);
    }

    #[test]
    fn block_tag_only_looks_at_block_tags() {
        assert_eq!(block_tag("minecraft:no_such_tag"), None);
        // 这是物品标签，方块标签里没有同名的。
        assert_eq!(block_tag("minecraft:swords"), None);
    }
}
