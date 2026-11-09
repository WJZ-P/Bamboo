//! 运行官方数据生成器和 `Extractor.java`，读回它们的输出。

use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;

const BLOCK_REGISTRY: &str = "minecraft:block";
const ENTITY_TYPE_REGISTRY: &str = "minecraft:entity_type";
const ITEM_REGISTRY: &str = "minecraft:item";
/// 官方数据生成器的输出目录，相对于工作目录。
const GENERATED_DIR: &str = "generated";

/// 官方报告里一个连接状态的数据包：方向 → 包名 → ID。
pub type PacketReport = BTreeMap<String, BTreeMap<String, PacketEntry>>;

#[derive(Debug, Deserialize)]
pub struct PacketEntry {
    pub protocol_id: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extracted {
    pub version: Version,
    pub known_packs: Vec<KnownPack>,
    pub enabled_features: Vec<String>,
    pub synced_registries: Vec<SyncedRegistry>,
    pub tags: Vec<TagGroup>,
    pub block_states: BlockStates,
    pub blocks: Vec<Block>,
    pub items: Vec<ExtractedItem>,
    pub entity_types: Vec<EntityTypeFacts>,
    pub state_tables: StateTables,
    pub light_tables: LightTables,
    pub block_maps: BlockMaps,
    pub block_transformers: Vec<BlockTransformer>,
    /// 带 `minecraft:item_damage` 效果的附魔，按名字排序。
    pub item_damage_enchantments: Vec<ItemDamageEnchantment>,
    pub recipes: RecipesFile,
    pub heightmaps: Vec<HeightmapType>,
    pub sound_tables: SoundTables,
    /// 数据包里的战利品表原文：带命名空间的名字 → JSON。
    ///
    /// Loot table JSON from the data pack, keyed by namespaced id.
    pub loot_tables: BTreeMap<String, String>,
    /// `predicate` 注册表条目的原文。
    ///
    /// Raw JSON of `predicate` registry entries.
    pub loot_predicates: BTreeMap<String, String>,
    /// `context_int_provider` 注册表条目的原文。
    ///
    /// Raw JSON of `context_int_provider` registry entries.
    pub context_int_providers: BTreeMap<String, String>,
    /// `context_float_provider` 注册表条目的原文。
    ///
    /// Raw JSON of `context_float_provider` registry entries.
    pub context_float_providers: BTreeMap<String, String>,
    /// 数据包里的附魔定义原文：带命名空间的名字 → JSON。
    ///
    /// Enchantment definition JSON, keyed by namespaced id.
    pub enchantments: BTreeMap<String, String>,
    /// 数据包里的烧炼配方原文，只有 `minecraft:smelting`。
    ///
    /// Smelting recipe JSON. Only `minecraft:smelting` is kept.
    pub smelting_recipes: BTreeMap<String, String>,
    /// `MobEffect.isInstantaneous()` 为真的效果。
    ///
    /// Effects for which `MobEffect.isInstantaneous()` is true.
    pub instant_mob_effects: Vec<String>,
    /// 属性名字 → 默认值，没有状态效果和附魔时的起点。
    pub attribute_defaults: BTreeMap<String, f64>,
    pub component_enums: ComponentEnums,
    /// 每个物品的默认组件，按 `DataComponentPatch.STREAM_CODEC` 编成十六进制。
    pub item_components: Vec<String>,
    /// `minecraft:jukebox_song`，按注册表 ID 排列。旧的提取结果没有这一项。
    #[serde(default)]
    pub jukebox_songs: Vec<ExtractedJukeboxSong>,
    /// 世界生成的数据：噪声参数、密度函数、维度设置和多噪声群系预设，原样是 JSON。
    pub worldgen: serde_json::Value,
    /// `Item.getCraftingRemainder()`，和物品注册表对齐。没有余料的是 `null`。
    pub crafting_remainders: Vec<Option<(i32, i32, String)>>,
    /// `BuiltInRegistries.GAME_RULE`，按注册表 ID。旧的提取结果没有这一项。
    #[serde(default)]
    pub game_rules: Vec<GameRuleFact>,
}

/// 一条游戏规则。字段和 `Extractor.gameRules` 的 JSON 对应。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRuleFact {
    pub name: String,
    pub identifier: String,
    pub category: String,
    /// `boolean` 或 `integer`，官方 `GameRuleType.getSerializedName`。
    pub kind: String,
    pub default_bool: Option<bool>,
    pub default_int: Option<i32>,
    pub min: Option<i32>,
    pub max: Option<i32>,
    /// `GameRule.isEnabled(FeatureFlags.DEFAULT_FLAGS)`。
    pub enabled: bool,
    #[serde(default)]
    pub features: Vec<String>,
}

/// `JukeboxSong`：长度（秒）和比较器输出。
#[derive(Debug, Deserialize)]
pub struct ExtractedJukeboxSong {
    pub length: f32,
    pub output: i32,
}

/// 按 ID 查表的枚举：连续的只有常量个数，稀疏的是整张 ID 表。
#[derive(Debug, Deserialize)]
pub struct ComponentEnums {
    /// 枚举名 → 常量个数。
    pub counts: BTreeMap<String, u32>,
    /// 枚举名 → 按声明顺序的 ID。
    pub sparse: BTreeMap<String, Vec<i32>>,
    /// 枚举名 → 序列化名字，下标和网络上的 ID 一致。没有存档 `Codec` 的瞬态枚举不在里面。
    pub names: BTreeMap<String, Vec<String>>,
}

/// 官方数据包的全部配方，`entries` 的顺序就是注册表的顺序。
#[derive(Debug, Deserialize)]
pub struct RecipesFile {
    /// 药水注册表的条目名，下标就是 ID。
    pub potions: Vec<String>,
    /// 默认物品栈带 `max_damage` 和 `damage` 的物品注册表 ID。修复物品配方要求这两样都在。
    /// 由 Rust 从物品组件报告里补上，官方 `Extractor` 不导出。
    #[serde(default)]
    pub damageable: Vec<i32>,
    /// 默认物品栈带 `dye` 组件的物品注册表 ID。染色类配方要求染料物品有这个组件。
    #[serde(default)]
    pub dyed: Vec<i32>,
    pub entries: Vec<ExtractedRecipe>,
}

#[derive(Debug, Deserialize)]
pub struct ExtractedRecipe {
    pub id: String,
    /// `RecipeSerializer` 的注册表键，比如 `minecraft:crafting_shaped`。
    #[serde(rename = "type")]
    pub serializer: String,
    #[serde(flatten)]
    pub kind: ExtractedRecipeKind,
}

/// `Extractor.java` 按官方的类分出来的配方。字段和那边的 JSON 一一对应。
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExtractedRecipeKind {
    Shaped {
        width: u8,
        height: u8,
        pattern: Vec<Option<ExtractedIngredient>>,
        result: ExtractedStack,
    },
    Shapeless {
        ingredients: Vec<ExtractedIngredient>,
        result: ExtractedStack,
    },
    Transmute {
        input: ExtractedIngredient,
        material: ExtractedIngredient,
        #[serde(rename = "minMaterial")]
        min_material: i32,
        #[serde(rename = "maxMaterial")]
        max_material: i32,
        #[serde(rename = "resultItem")]
        result_item: Option<i32>,
        #[serde(rename = "resultCount")]
        result_count: i32,
        #[serde(rename = "resultComponents")]
        result_components: String,
        #[serde(rename = "addMaterialCount")]
        add_material_count: bool,
    },
    Dye {
        target: ExtractedIngredient,
        dye: ExtractedIngredient,
        result: ExtractedStack,
    },
    Imbue {
        source: ExtractedIngredient,
        material: ExtractedIngredient,
        result: ExtractedStack,
    },
    /// 四种烹饪配方共用。`kind` 由 `Extractor.java` 写成 `smelting`、`blasting` 等。
    Smelting {
        ingredient: ExtractedIngredient,
        result: ExtractedStack,
        experience: f32,
        time: i32,
    },
    Blasting {
        ingredient: ExtractedIngredient,
        result: ExtractedStack,
        experience: f32,
        time: i32,
    },
    Smoking {
        ingredient: ExtractedIngredient,
        result: ExtractedStack,
        experience: f32,
        time: i32,
    },
    CampfireCooking {
        ingredient: ExtractedIngredient,
        result: ExtractedStack,
        experience: f32,
        time: i32,
    },
    Stonecutting {
        ingredient: ExtractedIngredient,
        result: ExtractedStack,
    },
    SmithingTransform {
        template: Option<ExtractedIngredient>,
        base: ExtractedIngredient,
        addition: Option<ExtractedIngredient>,
        result: ExtractedStack,
    },
    SmithingTrim {
        template: ExtractedIngredient,
        base: ExtractedIngredient,
        addition: ExtractedIngredient,
        /// `TrimPattern` 的注册表键。
        pattern: String,
    },
    Brewing {
        input: ExtractedPotionIngredient,
        reagent: ExtractedPotionIngredient,
        result: ExtractedStack,
    },
    FireworkRocket {
        shell: ExtractedIngredient,
        fuel: ExtractedIngredient,
        star: ExtractedIngredient,
        result: ExtractedStack,
    },
    FireworkStar {
        shapes: Vec<ExtractedShape>,
        trail: ExtractedIngredient,
        twinkle: ExtractedIngredient,
        fuel: ExtractedIngredient,
        dye: ExtractedIngredient,
        result: ExtractedStack,
    },
    FireworkStarFade {
        target: ExtractedIngredient,
        dye: ExtractedIngredient,
        result: ExtractedStack,
    },
    Mapextending {
        map: ExtractedIngredient,
        material: ExtractedIngredient,
        #[serde(rename = "resultItem")]
        result_item: Option<i32>,
        #[serde(rename = "resultCount")]
        result_count: i32,
        #[serde(rename = "resultComponents")]
        result_components: String,
    },
    BannerDuplicate {
        banner: ExtractedIngredient,
        result: ExtractedStack,
    },
    BookCloning {
        source: ExtractedIngredient,
        material: ExtractedIngredient,
        #[serde(rename = "minGeneration")]
        min_generation: i32,
        #[serde(rename = "maxGeneration")]
        max_generation: i32,
        result: ExtractedStack,
    },
    ShieldDecoration {
        banner: ExtractedIngredient,
        target: ExtractedIngredient,
        result: ExtractedStack,
    },
    RepairItem,
    DecoratedPot {
        back: ExtractedIngredient,
        left: ExtractedIngredient,
        right: ExtractedIngredient,
        front: ExtractedIngredient,
        result: ExtractedStack,
    },
}

/// 展开成物品 ID 的原料。`items` 升序、不重复。
#[derive(Debug, Deserialize)]
pub struct ExtractedIngredient {
    /// 原料本来是一个标签时才有。
    pub tag: Option<String>,
    pub items: Vec<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedStack {
    pub item: i32,
    pub count: i32,
    /// 官方 `DataComponentPatch.STREAM_CODEC` 的字节，十六进制。
    pub components: String,
}

#[derive(Debug, Deserialize)]
pub struct ExtractedPotionIngredient {
    pub item: ExtractedIngredient,
    /// 药水注册表 ID。官方数据包只用了直接列出药水的写法。
    pub potions: Option<Vec<i32>>,
}

#[derive(Debug, Deserialize)]
pub struct ExtractedShape {
    /// `FireworkExplosion.Shape` 的序列化名，比如 `burst`。
    pub shape: String,
    pub ingredient: ExtractedIngredient,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub name: String,
    pub protocol: i32,
    pub data_version: i32,
}

#[derive(Debug, Deserialize)]
pub struct KnownPack {
    pub namespace: String,
    pub id: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct SyncedRegistry {
    pub name: String,
    pub entries: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TagGroup {
    pub registry: String,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Deserialize)]
pub struct Tag {
    pub name: String,
    pub ids: Vec<i32>,
}

#[derive(Debug, Deserialize)]
pub struct BlockStates {
    pub total: u32,
    pub defaults: BTreeMap<String, u32>,
}

/// 一个方块。列表里的位置就是它的注册表 ID。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub name: String,
    /// 全部状态的 ID，顺序和官方一致。
    pub states: Vec<u32>,
    pub default_state: u32,
    pub item: i32,
    pub has_collision: bool,
    pub has_fluid: bool,
    pub two_blocks: bool,
    /// 活塞推动时的反应，官方 `PushReaction` 名字的小写形式。
    pub push_reaction: String,
    /// 破坏速度是 -1：活塞推不动，也打不碎。
    pub unbreakable: bool,
    pub block_entity_type: Option<i32>,
    /// 官方的类名，比如 `SlabBlock`。
    pub class: String,
    /// 这个类、它的父类和方块包里的接口。
    pub ancestors: Vec<String>,
    pub properties: Vec<Property>,
    pub fields: Vec<BlockField>,
    pub instrument: String,
    pub dynamic_shape: bool,
    /// `BlockBehaviour.getLootTable`，没有掉落表时是 `None`。
    pub loot_table: Option<String>,
    /// `Block.getSpeedFactor`。
    pub speed_factor: f32,
    /// `Block.getBounceRestitution`。
    pub bounce_restitution: f32,
    /// `Block.getExplosionResistance`。
    pub explosion_resistance: f32,
    pub offset: Option<BlockOffset>,
    /// 用桶取起这个方块时播的声音，`sound_event` 注册表 ID。
    #[serde(default)]
    pub pickup_sound: Option<i32>,
    /// `defaultDestroyTime`，也就是硬度。
    pub destroy_time: f32,
    pub requires_correct_tool_for_drops: bool,
}

/// 一个方块属性，顺序和取值都是官方 `StateDefinition` 里的。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
pub struct Property {
    pub name: String,
    pub values: Vec<String>,
    #[serde(flatten)]
    pub kind: PropertyKind,
}

/// 属性的官方类，以及属性值 `compareTo` 用的顺序。字段含义见
/// [`mc_bamboo_data::PropertyKind`]。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PropertyKind {
    /// `IntegerProperty`，取值是 `min..=max` 的连续整数。
    Integer { min: i32, max: i32 },
    /// `BooleanProperty`，取值列表是 `[true, false]`，比较时 `false < true`。
    Boolean,
    /// `EnumProperty`，和 `values` 一一对应的枚举常量声明序号。
    Enum { ordinals: Vec<u16> },
}

/// 类的一个构造参数或者只读方法的结果。方法名后面带括号。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BlockField {
    pub name: String,
    pub value: FieldValue,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldValue {
    Bool(bool),
    Int(i32),
    Float(f32),
    Double(f64),
    Block(i32),
    State(u32),
    Item(i32),
    Direction(i32),
    Tag(String),
    Name(String),
}

/// 方块的渲染偏移，形状跟着它走的方块碰撞也和位置有关。
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockOffset {
    pub vertical: bool,
    pub max_horizontal: f32,
    pub max_vertical: f32,
    pub moves_shape: bool,
}

/// 每个状态一项的表和去重后的形状表。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateTables {
    pub facts: Vec<u64>,
    pub fluid: Vec<u8>,
    pub light_dampening: Vec<u8>,
    pub collision: Vec<u16>,
    pub outline: Vec<u16>,
    pub support: Vec<u16>,
    /// `placementContext` 下和普通碰撞不同的状态：状态 ID 和形状下标。
    pub placement_collision: Vec<(u32, u16)>,
    /// 形状跟着偏移走的状态在几个位置上的 `isFaceSturdy`：状态 ID、坐标和位。
    pub offset_sturdy_samples: Vec<(u32, i32, i32, i32, u32)>,
    pub fluid_states: Vec<FluidState>,
    /// 每个形状的盒子，一个盒子是 `min_x, min_y, min_z, max_x, max_y, max_z`。
    pub shapes: Vec<Vec<[f64; 6]>>,
    pub shape_faces: Vec<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FluidState {
    pub fluid: i32,
    pub source: bool,
    pub amount: u8,
    pub falling: bool,
    pub legacy_block: u32,
    /// `FluidState.getExplosionResistance`。
    pub explosion_resistance: f32,
}

/// 光照引擎要的表，`Extractor.java` 的 `LightTables`。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LightTables {
    /// 每个状态的 `getLightEmission`。
    pub emission: Vec<u8>,
    /// 每个状态在 `occlusion_faces` 里的下标。
    pub occlusion: Vec<u16>,
    /// 去重后的 6 个面的遮挡形状，按 `Direction` 的 ID 排列，第一项全是空形状。
    pub occlusion_faces: Vec<[u16; 6]>,
    /// 去重后的面形状个数。
    pub face_count: usize,
    /// 两两之间的 `faceShapeOccludes`，Java 的 `long` 是有符号的。
    pub face_occludes: Vec<i64>,
}

/// 音效表，`Extractor.java` 的 `SoundTables`。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundTables {
    /// 每个状态的 `getSoundType` 在 [`Self::types`] 里的下标。
    pub state_types: Vec<u16>,
    /// 去重后的 `SoundType`：音量、音高、破坏、踩踏、放置、击中、落地声音的注册表 ID。
    pub types: Vec<(f32, f32, i32, i32, i32, i32, i32)>,
    /// `sound_event` 注册表，下标就是网络上的 Holder ID。
    pub events: Vec<SoundEventRow>,
    /// `NoteBlockInstrument.values()`，按枚举序号排列。
    pub instruments: Vec<InstrumentRow>,
    /// `SoundSource` 的名字，按枚举序号排列。
    pub sources: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundEventRow {
    pub name: String,
    #[serde(default)]
    pub fixed_range: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentRow {
    pub name: String,
    pub sound: i32,
    pub tunable: bool,
    pub custom_sound: bool,
}

/// `Heightmap.Types` 的一种。区块包按 `id` 排列，`id` 也是网络上的键。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeightmapType {
    /// 官方的序列化名，比如 `MOTION_BLOCKING_NO_LEAVES`。
    pub name: String,
    pub id: i32,
    /// `sendToClient`：区块包带上这一种。
    pub send_to_client: bool,
    /// `keepAfterWorldgen`：区块里一直留着这一种。
    pub keep_after_worldgen: bool,
}

/// 官方写在静态表里的方块对应关系，都是方块注册表 ID。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockMaps {
    pub flower_pots: Vec<(i32, i32)>,
    pub candle_cakes: Vec<(i32, i32)>,
    pub weathering: Vec<(i32, i32)>,
    pub waxables: Vec<(i32, i32)>,
    /// 方块、`igniteOdds`、`burnOdds`。
    pub fire_odds: Vec<(i32, i32, i32)>,
    /// `ignitedByLava` 的方块 ID，升序。
    pub ignited_by_lava: Vec<i32>,
    /// 物品注册表 ID。
    pub campfire_inputs: Vec<i32>,
    /// 数据包 `context_int_provider/compostable` 下的提供器原文，键是名字。
    pub compost_layers: BTreeMap<String, serde_json::Value>,
}

/// 世界层注册表里的一个 `BlockTransformer`，只有改方块的部分。
#[derive(Debug, Deserialize)]
pub struct BlockTransformer {
    pub name: String,
    pub transforms: Vec<BlockTransform>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockTransform {
    pub provider: StateProvider,
    /// `Direction` 的 ID。
    pub disallowed_faces: Vec<i32>,
    pub update_from_neighbors: bool,
    /// `consumeOnUse`。可堆叠物品用完时是否减 1。
    pub consume_on_use: bool,
    /// `itemDamagePerUse`。不可堆叠时 `hurtAndBreak` 的点数。codec 缺省是 0，生成器导出加载后的值。
    pub item_damage_per_use: i32,

    /// `sound`：音效事件的注册表 ID，没配时官方给 `SoundEvents.EMPTY`，那边不导出。
    #[serde(default)]
    pub sound: Option<i32>,
    /// `particle`：`TransformParticle` 的等级事件 ID，`NONE` 是 0。
    #[serde(default)]
    pub particle: i32,
    /// `transform_type == COPPER_CHEST`：改完方块还要给大箱子的另一半发一次粒子。
    #[serde(default)]
    pub copper_chest: bool,
}

/// 一个附魔上的 `minecraft:item_damage` 效果。
#[derive(Debug, Deserialize)]
pub struct ItemDamageEnchantment {
    pub name: String,
    pub effects: Vec<ConditionalValueEffect>,
}

/// `ConditionalEffect`：效果，以及可选的战利品条件。
#[derive(Debug, Deserialize)]
pub struct ConditionalValueEffect {
    pub effect: ValueEffectJson,
    #[serde(default)]
    pub requirements: Option<LootConditionJson>,
}

/// `EnchantmentValueEffect` 的 codec。不认识的 `type` 让反序列化失败，生成器因此停下来。
#[derive(Debug, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ValueEffectJson {
    #[serde(rename = "minecraft:add")]
    Add { value: LevelValueJson },
    #[serde(rename = "minecraft:all_of")]
    AllOf { effects: Vec<ValueEffectJson> },
    #[serde(rename = "minecraft:multiply")]
    Multiply { factor: LevelValueJson },
    #[serde(rename = "minecraft:remove_binomial")]
    RemoveBinomial { chance: LevelValueJson },
    #[serde(rename = "minecraft:exponential")]
    Exponential {
        base: LevelValueJson,
        exponent: LevelValueJson,
    },
    #[serde(rename = "minecraft:set")]
    Set { value: LevelValueJson },
}

/// `LevelBasedValue`。裸数字是 `Constant`，其余按 `type` 分派。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum LevelValueJson {
    Constant(f32),
    Typed(TypedLevelJson),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum TypedLevelJson {
    #[serde(rename = "minecraft:clamped")]
    Clamped {
        value: Box<LevelValueJson>,
        min: f32,
        max: f32,
    },
    #[serde(rename = "minecraft:constant")]
    Constant { value: f32 },
    #[serde(rename = "minecraft:exponent")]
    Exponent {
        base: Box<LevelValueJson>,
        power: Box<LevelValueJson>,
    },
    #[serde(rename = "minecraft:fraction")]
    Fraction {
        numerator: Box<LevelValueJson>,
        denominator: Box<LevelValueJson>,
    },
    #[serde(rename = "minecraft:levels_squared")]
    LevelsSquared { added: f32 },
    #[serde(rename = "minecraft:linear")]
    Linear {
        base: f32,
        per_level_above_first: f32,
    },
    #[serde(rename = "minecraft:lookup")]
    Lookup {
        values: Vec<f32>,
        fallback: Box<LevelValueJson>,
    },
}

/// `item_damage` 用到的战利品条件。别的条件类型会让反序列化失败。
#[derive(Debug, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum LootConditionJson {
    #[serde(rename = "minecraft:all_of")]
    AllOf { terms: Vec<LootConditionJson> },
    #[serde(rename = "minecraft:any_of")]
    AnyOf { terms: Vec<LootConditionJson> },
    #[serde(rename = "minecraft:inverted")]
    Inverted { term: Box<LootConditionJson> },
    #[serde(rename = "minecraft:match_tool")]
    MatchTool {
        #[serde(default)]
        predicate: Option<ItemPredicateJson>,
    },
}

/// `MatchTool` 的物品谓词。出现 `count` 或 `components` 时拒绝，避免把没实现的比较当成通过。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemPredicateJson {
    #[serde(default)]
    pub items: Option<serde_json::Value>,
}

/// `Extractor.java` 只导出这几种状态提供器，别的在那边就报错。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StateProvider {
    /// 状态 ID。
    Simple(u32),
    CopyProperties(Box<StateProvider>),
    RuleBased {
        rules: Vec<StateRule>,
        fallback: Option<Box<StateProvider>>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateRule {
    pub if_true: BlockPredicate,
    pub then: StateProvider,
}

/// `Extractor.java` 只导出这几种方块判定，别的在那边就报错。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockPredicate {
    /// 方块注册表 ID。
    MatchingBlocks {
        offset: [i32; 3],
        blocks: Vec<i32>,
    },
    MatchingBlockTag {
        offset: [i32; 3],
        tag: String,
    },
    AllOf(Vec<BlockPredicate>),
}

/// `Extractor.java` 导出的物品。物品组件在官方报告里，见 [`Item`]。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedItem {
    pub name: String,
    /// 官方的类名，比如 `BlockItem`。
    pub class: String,
    pub block: Option<i32>,
    pub wall_block: Option<i32>,
    pub attachment: Option<i32>,
    /// `SolidBucketItem.placeSound`，只有细雪桶有。
    /// `SolidBucketItem.placeSound`. Only the powder-snow bucket has one.
    #[serde(default)]
    pub place_sound: Option<i32>,
    /// `BucketItem.playEmptySound` 或 `MobBucketItem.emptySound`。
    /// `BucketItem.playEmptySound`, or `MobBucketItem.emptySound`.
    #[serde(default)]
    pub empty_sound: Option<i32>,
    /// 倒空时 `SoundSource` 的序号。生物桶是 `NEUTRAL`，其余桶是 `BLOCKS`。
    /// `SoundSource` ordinal when emptying. `NEUTRAL` for a mob bucket, `BLOCKS` otherwise.
    #[serde(default)]
    pub empty_source: Option<i32>,
    /// `MobBucketItem.type` 的注册表名，比如 `minecraft:salmon`。
    /// Registry name of `MobBucketItem.type`, for example `minecraft:salmon`.
    #[serde(default)]
    pub bucket_entity: Option<String>,
    pub overrides_can_destroy_block: bool,
}

/// 一个实体类型。列表里的位置就是它的注册表 ID。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityTypeFacts {
    pub name: String,
    pub client_tracking_range: i32,
    /// -1 表示官方写的是 `Integer.MAX_VALUE`，也就是 `UpdateInterval.NEVER`。
    pub update_interval: i32,
    pub track_deltas: bool,
    /// `EntityDimensions.width`。
    pub width: f32,
    /// `EntityDimensions.height`。
    pub height: f32,
}

/// 一个物品。列表里的位置就是它的注册表 ID。
pub struct Item {
    pub name: String,
    pub class: String,
    pub block: Option<i32>,
    pub wall_block: Option<i32>,
    pub attachment: Option<i32>,
    pub max_stack_size: i32,
    /// `max_damage` 组件，没有这个组件时物品不能受损。
    pub max_damage: Option<i32>,
    /// `enchantable` 组件的 `value`，附魔台的等级计算要用。
    pub enchantable: Option<i32>,
    pub can_destroy_blocks_in_creative: bool,
    pub consumable: bool,
    /// `compostable` 组件的 `layers`。
    pub compostable: Option<CompostLayers>,
    pub jukebox_playable: bool,
    /// `jukebox_playable` 组件指向的 `jukebox_song` 注册表条目。
    pub jukebox_song: Option<String>,
    /// `block_transformer` 组件指向的注册表条目。
    pub block_transformer: Option<String>,
    pub blocks_attacks: bool,
    /// `SolidBucketItem.placeSound`，放下时播放的声音。
    /// `SolidBucketItem.placeSound`, played when the bucket places its block.
    pub place_sound: Option<i32>,
    /// 倒空时的音效。只有 `BucketItem`（含生物桶）有。
    /// Sound played when the bucket empties. Only `BucketItem`, including mob buckets.
    pub empty_sound: Option<i32>,
    /// 倒空时 `SoundSource` 的序号。
    /// `SoundSource` ordinal when the bucket empties.
    pub empty_source: Option<i32>,
    /// `MobBucketItem.type` 的注册表名。别的物品是 `None`。
    /// Registry name of `MobBucketItem.type`. `None` for every other item.
    pub bucket_entity: Option<String>,
}

/// `reports/minecraft/components/item` 里一个物品的报告，只取用到的组件。
#[derive(Debug, Deserialize)]
struct ItemReport {
    components: ItemComponents,
}

#[derive(Debug, Deserialize)]
struct ItemComponents {
    #[serde(rename = "minecraft:max_stack_size")]
    max_stack_size: i32,
    #[serde(rename = "minecraft:max_damage")]
    max_damage: Option<i32>,
    #[serde(rename = "minecraft:enchantable")]
    enchantable: Option<EnchantableComponent>,
    #[serde(rename = "minecraft:tool")]
    tool: Option<ToolComponent>,
    #[serde(rename = "minecraft:consumable")]
    consumable: Option<serde_json::Value>,
    #[serde(rename = "minecraft:compostable")]
    compostable: Option<CompostableComponent>,
    /// 报告里写歌典的名字。直接内联的写法读不了，生成会失败。
    #[serde(rename = "minecraft:jukebox_playable")]
    jukebox_playable: Option<String>,
    /// 报告里写注册表条目的名字。直接内联的写法读不了，生成会失败。
    #[serde(rename = "minecraft:block_transformer")]
    block_transformer: Option<String>,
    #[serde(rename = "minecraft:blocks_attacks")]
    blocks_attacks: Option<serde_json::Value>,
    /// 染色类配方要求染料物品带 `dye` 组件。
    #[serde(rename = "minecraft:dye")]
    dye: Option<serde_json::Value>,
}

/// 只取 `can_destroy_blocks_in_creative`：规则本身由协议 crate 从官方导出的组件字节里解。
/// 字段等于默认值时不写，所以缺省是 `true`。
#[derive(Debug, Deserialize)]
struct ToolComponent {
    can_destroy_blocks_in_creative: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct EnchantableComponent {
    value: i32,
}

#[derive(Debug, Deserialize)]
struct CompostableComponent {
    layers: serde_json::Value,
}

/// 往堆肥桶里放一个物品加几层。只认原版数据包用到的两种写法，别的写法生成失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompostLayers {
    /// 固定的层数，不取随机数。
    Constant(i32),
    /// 堆肥桶空着时加 `when_empty` 层，否则按权重抽一项：（层数，权重）。
    Weighted {
        when_empty: i32,
        distribution: Vec<(i32, i32)>,
    },
}

/// 整数提供器里堆肥用到的写法：内联常数、引用数据包条目、带 `type` 的对象。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum IntProvider {
    Constant(i32),
    Reference(String),
    Typed(TypedIntProvider),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum TypedIntProvider {
    #[serde(rename = "minecraft:constant")]
    Constant { value: i32 },
    #[serde(rename = "minecraft:number_dispatcher")]
    Dispatcher {
        cases: Vec<DispatcherCase>,
        default: Box<IntProvider>,
    },
    #[serde(rename = "minecraft:weighted_list")]
    WeightedList { distribution: Vec<WeightedEntry> },
}

#[derive(Debug, Deserialize)]
struct DispatcherCase {
    condition: serde_json::Value,
    value: IntProvider,
}

#[derive(Debug, Deserialize)]
struct WeightedEntry {
    data: IntProvider,
    weight: i32,
}

/// `compostable` 组件的 `layers` 解析成 [`CompostLayers`]。
fn compost_layers(
    layers: &serde_json::Value,
    providers: &BTreeMap<String, serde_json::Value>,
) -> Result<CompostLayers, String> {
    match resolve_int_provider(parse_int_provider(layers)?, providers)? {
        IntProvider::Typed(TypedIntProvider::Dispatcher { cases, default }) => {
            let [case] = <[DispatcherCase; 1]>::try_from(cases)
                .map_err(|_| format!("unsupported compostable layers: {layers}"))?;
            if !is_empty_composter(&case.condition) {
                return Err(format!("unsupported compostable condition: {layers}"));
            }
            let IntProvider::Typed(TypedIntProvider::WeightedList { distribution }) =
                resolve_int_provider(*default, providers)?
            else {
                return Err(format!("unsupported compostable default: {layers}"));
            };
            let distribution = distribution
                .into_iter()
                .map(|entry| Ok((constant_int(entry.data, providers)?, entry.weight)))
                .collect::<Result<_, String>>()?;
            Ok(CompostLayers::Weighted {
                when_empty: constant_int(case.value, providers)?,
                distribution,
            })
        }
        provider => Ok(CompostLayers::Constant(constant_int(provider, providers)?)),
    }
}

fn parse_int_provider(value: &serde_json::Value) -> Result<IntProvider, String> {
    IntProvider::deserialize(value).map_err(|error| format!("bad int provider {value}: {error}"))
}

/// 引用换成数据包里的条目，别的原样返回。
fn resolve_int_provider(
    provider: IntProvider,
    providers: &BTreeMap<String, serde_json::Value>,
) -> Result<IntProvider, String> {
    let IntProvider::Reference(name) = provider else {
        return Ok(provider);
    };
    let value = providers
        .get(&name)
        .ok_or_else(|| format!("unknown int provider {name}"))?;
    resolve_int_provider(parse_int_provider(value)?, providers)
}

fn constant_int(
    provider: IntProvider,
    providers: &BTreeMap<String, serde_json::Value>,
) -> Result<i32, String> {
    match resolve_int_provider(provider, providers)? {
        IntProvider::Constant(value) | IntProvider::Typed(TypedIntProvider::Constant { value }) => {
            Ok(value)
        }
        other => Err(format!("expected a constant int provider, got {other:?}")),
    }
}

/// 条件是“方块是 `level` 为 0 的堆肥桶”。
fn is_empty_composter(condition: &serde_json::Value) -> bool {
    *condition
        == serde_json::json!({
            "type": "minecraft:match_block",
            "blocks": "minecraft:composter",
            "state": { "level": "0" },
        })
}

/// `registries.json` 里的一个注册表：条目名 → ID。
#[derive(Debug, Deserialize)]
struct RegistryReport {
    entries: BTreeMap<String, RegistryEntry>,
}

#[derive(Debug, Deserialize)]
struct RegistryEntry {
    protocol_id: usize,
}

/// 生成代码需要的全部输入。
pub struct VersionData {
    pub version: Version,
    pub known_packs: Vec<KnownPack>,
    pub enabled_features: Vec<String>,
    pub synced_registries: Vec<SyncedRegistry>,
    pub tags: Vec<TagGroup>,
    pub block_states: BlockStates,
    pub blocks: Vec<Block>,
    pub items: Vec<Item>,
    pub entity_types: Vec<EntityTypeFacts>,
    pub state_tables: StateTables,
    pub light_tables: LightTables,
    pub block_maps: BlockMaps,
    /// 按名字排序。
    pub block_transformers: Vec<BlockTransformer>,
    /// 带 `minecraft:item_damage` 效果的附魔，按名字排序。
    pub item_damage_enchantments: Vec<ItemDamageEnchantment>,
    pub recipes: RecipesFile,
    /// 按官方 `Heightmap.Types.values()` 的顺序。
    pub heightmaps: Vec<HeightmapType>,
    pub sound_tables: SoundTables,
    /// 属性名字 → 默认值。
    pub attribute_defaults: BTreeMap<String, f64>,
    /// 内置注册表名 → 按 ID 排列的条目。
    pub builtin_registries: BTreeMap<String, Vec<String>>,
    /// 连接状态 → 方向 → 包名 → ID。
    pub packets: BTreeMap<String, PacketReport>,
    /// 战利品表原文：带命名空间的名字 → JSON。
    ///
    /// Loot table JSON, keyed by namespaced id.
    pub loot_tables: BTreeMap<String, String>,
    pub loot_predicates: BTreeMap<String, String>,
    pub context_int_providers: BTreeMap<String, String>,
    pub context_float_providers: BTreeMap<String, String>,
    /// 附魔定义原文：带命名空间的名字 → JSON。
    ///
    /// Enchantment definition JSON, keyed by namespaced id.
    pub enchantments: BTreeMap<String, String>,
    /// 烧炼配方原文，只有 `minecraft:smelting`。
    ///
    /// Smelting recipe JSON. Only `minecraft:smelting` is kept.
    pub smelting_recipes: BTreeMap<String, String>,
    /// `MobEffect.isInstantaneous()` 为真的效果。
    ///
    /// Effects for which `MobEffect.isInstantaneous()` is true.
    pub instant_mob_effects: Vec<String>,
    /// 组件里按 ID 查表的枚举。
    pub component_enums: ComponentEnums,
    /// 每个物品的默认组件，按 `DataComponentPatch.STREAM_CODEC` 编成十六进制。
    pub item_components: Vec<String>,
    /// `minecraft:jukebox_song`，按注册表 ID 排列。
    pub jukebox_songs: Vec<ExtractedJukeboxSong>,
    /// 世界生成的数据，原样写进 `mc-bamboo-data` 的 `worldgen.json`。
    pub worldgen: serde_json::Value,
    /// `Item.getCraftingRemainder()`，和物品注册表对齐。
    pub crafting_remainders: Vec<Option<(i32, i32, String)>>,
    /// `BuiltInRegistries.GAME_RULE`，按注册表 ID。
    pub game_rules: Vec<GameRuleFact>,
}

pub fn run(
    java: &str,
    server_jar: &Path,
    work_dir: &Path,
    workspace_root: &Path,
) -> Result<VersionData, String> {
    run_reports(java, server_jar, work_dir)?;
    read_version_data(java, work_dir, workspace_root)
}

/// 不重新跑官方数据生成器，直接用上一次留下的报告重新运行 `Extractor.java`。
///
/// 报告只由服务端 jar 决定，换 jar 时必须先跑一遍 `generate`。
pub fn extract_existing(
    java: &str,
    work_dir: &Path,
    workspace_root: &Path,
) -> Result<VersionData, String> {
    let reports = work_dir.join(GENERATED_DIR).join("reports");
    if !reports.is_dir() {
        return Err(format!("{} 不存在，先运行一次 generate", reports.display()));
    }
    read_version_data(java, work_dir, workspace_root)
}

fn read_version_data(
    java: &str,
    work_dir: &Path,
    workspace_root: &Path,
) -> Result<VersionData, String> {
    let extracted_path = work_dir.join("extracted.json");
    run_extractor(java, work_dir, workspace_root, &extracted_path)?;

    let extracted: Extracted = read_json(&extracted_path)?;
    let reports = work_dir.join(GENERATED_DIR).join("reports");
    let packets = read_packet_report(&reports.join("packets.json"))?;
    let builtin_registries = read_registry_report(&reports.join("registries.json"))?;
    check_registry_order(
        &builtin_registries,
        BLOCK_REGISTRY,
        extracted.blocks.iter().map(|block| &block.name),
    )?;
    check_registry_order(
        &builtin_registries,
        ITEM_REGISTRY,
        extracted.items.iter().map(|item| &item.name),
    )?;
    check_registry_order(
        &builtin_registries,
        ENTITY_TYPE_REGISTRY,
        extracted.entity_types.iter().map(|type_| &type_.name),
    )?;
    let (items, damageable, dyed) = read_items(
        extracted.items,
        &reports.join("minecraft/components/item"),
        &extracted.block_maps.compost_layers,
    )?;
    Ok(VersionData {
        version: extracted.version,
        known_packs: extracted.known_packs,
        enabled_features: extracted.enabled_features,
        synced_registries: extracted.synced_registries,
        tags: extracted.tags,
        block_states: extracted.block_states,
        blocks: extracted.blocks,
        items,
        entity_types: extracted.entity_types,
        state_tables: extracted.state_tables,
        light_tables: extracted.light_tables,
        block_maps: extracted.block_maps,
        block_transformers: extracted.block_transformers,
        item_damage_enchantments: extracted.item_damage_enchantments,
        recipes: RecipesFile {
            damageable,
            dyed,
            ..extracted.recipes
        },
        heightmaps: extracted.heightmaps,
        sound_tables: extracted.sound_tables,
        attribute_defaults: extracted.attribute_defaults,
        builtin_registries,
        packets,
        loot_tables: extracted.loot_tables,
        loot_predicates: extracted.loot_predicates,
        context_int_providers: extracted.context_int_providers,
        context_float_providers: extracted.context_float_providers,
        enchantments: extracted.enchantments,
        smelting_recipes: extracted.smelting_recipes,
        instant_mob_effects: extracted.instant_mob_effects,
        component_enums: extracted.component_enums,
        item_components: extracted.item_components,
        crafting_remainders: extracted.crafting_remainders,
        jukebox_songs: extracted.jukebox_songs,
        worldgen: extracted.worldgen,
        game_rules: extracted.game_rules,
    })
}

/// 运行官方的数据生成器。它同时会把依赖库解压到工作目录里。
fn run_reports(java: &str, server_jar: &Path, work_dir: &Path) -> Result<(), String> {
    // 传入解压出来的内层 jar 时，这条命令进的是服务端入口，什么也不生成，退出码却是 0。
    // 先删掉旧输出，否则后面的步骤会拿旧报告生成数据，还报告成功。
    let output = work_dir.join(GENERATED_DIR);
    if output.exists() {
        fs::remove_dir_all(&output)
            .map_err(|error| format!("无法清理 {}：{error}", output.display()))?;
    }

    let status = Command::new(java)
        .current_dir(work_dir)
        .arg("-DbundlerMainClass=net.minecraft.data.Main")
        .arg("-jar")
        .arg(server_jar)
        .args(["--reports", "--output", GENERATED_DIR])
        .status()
        .map_err(|error| format!("无法运行 {java}：{error}"))?;
    if !status.success() {
        return Err(format!("官方数据生成器退出失败：{status}"));
    }
    if output.is_dir() {
        Ok(())
    } else {
        Err(format!(
            "{} 没有生成报告，这里要用官方下载的 server.jar，不是解压出来的内层 jar",
            server_jar.display()
        ))
    }
}

fn run_extractor(
    java: &str,
    work_dir: &Path,
    workspace_root: &Path,
    output: &Path,
) -> Result<(), String> {
    let source = workspace_root.join("tools/mc-bamboo-vanilla/java/Extractor.java");
    let status = Command::new(java)
        .current_dir(work_dir)
        .arg("-cp")
        .arg(classpath(work_dir)?)
        .arg(source)
        .arg(output)
        .status()
        .map_err(|error| format!("无法运行 {java}：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Extractor.java 退出失败：{status}"))
    }
}

/// 官方数据生成器解压出来的全部 jar，运行 Java 工具时用作 classpath。
pub fn classpath(work_dir: &Path) -> Result<OsString, String> {
    let mut jars = Vec::new();
    collect_jars(&work_dir.join("libraries"), &mut jars)?;
    collect_jars(&work_dir.join("versions"), &mut jars)?;
    env::join_paths(jars).map_err(|error| error.to_string())
}

fn collect_jars(directory: &Path, jars: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("无法读取 {}：{error}", directory.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            collect_jars(&path, jars)?;
        } else if path.extension().is_some_and(|extension| extension == "jar") {
            jars.push(path);
        }
    }
    Ok(())
}

fn read_packet_report(path: &Path) -> Result<BTreeMap<String, PacketReport>, String> {
    read_json(path)
}

fn read_registry_report(path: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
    let report: BTreeMap<String, RegistryReport> = read_json(path)?;
    report
        .into_iter()
        .map(|(name, registry)| {
            let entries = entries_by_id(&name, registry)?;
            Ok((name, entries))
        })
        .collect()
}

/// 按 ID 排列条目。生成的代码把条目的位置当作 ID，所以 ID 必须从 0 开始连续。
fn entries_by_id(name: &str, registry: RegistryReport) -> Result<Vec<String>, String> {
    let mut entries: Vec<_> = registry.entries.into_iter().collect();
    entries.sort_by_key(|(_, entry)| entry.protocol_id);
    if entries
        .iter()
        .enumerate()
        .any(|(id, (_, entry))| entry.protocol_id != id)
    {
        return Err(format!("{name} 的 ID 不是从 0 开始连续的"));
    }
    Ok(entries.into_iter().map(|(entry, _)| entry).collect())
}

/// `Extractor.java` 是按遍历顺序导出的，这里确认这个顺序就是注册表 ID。
fn check_registry_order<'a>(
    registries: &BTreeMap<String, Vec<String>>,
    registry: &str,
    names: impl Iterator<Item = &'a String>,
) -> Result<(), String> {
    let entries = registries
        .get(registry)
        .ok_or_else(|| format!("注册表报告里没有 {registry}"))?;
    if names.eq(entries) {
        Ok(())
    } else {
        Err(format!("{registry} 的导出顺序和注册表报告不一致"))
    }
}

/// `read_items` 的三份结果：物品表、带默认 `max_damage` 的 ID、带默认 `dye` 的 ID。
type ItemTables = (Vec<Item>, Vec<i32>, Vec<i32>);

/// 给物品补上组件报告里的数据。
///
/// 顺便收两份配方要用的物品 ID：带默认 `max_damage` 的，和带默认 `dye` 的。
/// 列表下标就是注册表 ID，天然升序。
fn read_items(
    extracted: Vec<ExtractedItem>,
    reports: &Path,
    compost_providers: &BTreeMap<String, serde_json::Value>,
) -> Result<ItemTables, String> {
    let mut items = Vec::with_capacity(extracted.len());
    let mut damageable = Vec::new();
    let mut dyed = Vec::new();
    for (id, item) in extracted.into_iter().enumerate() {
        let file = item.name.strip_prefix("minecraft:").unwrap_or(&item.name);
        let report: ItemReport = read_json(&reports.join(format!("{file}.json")))?;
        let components = report.components;
        if components.max_damage.is_some() {
            damageable.push(id as i32);
        }
        if components.dye.is_some() {
            dyed.push(id as i32);
        }
        let tool_allows = components
            .tool
            .as_ref()
            .is_none_or(|tool| tool.can_destroy_blocks_in_creative != Some(false));
        items.push(Item {
            max_stack_size: components.max_stack_size,
            max_damage: components.max_damage,
            enchantable: components.enchantable.map(|component| component.value),
            can_destroy_blocks_in_creative: tool_allows && !item.overrides_can_destroy_block,
            consumable: components.consumable.is_some(),
            compostable: components
                .compostable
                .map(|component| compost_layers(&component.layers, compost_providers))
                .transpose()?,
            jukebox_playable: components.jukebox_playable.is_some(),
            jukebox_song: components.jukebox_playable.clone(),
            block_transformer: components.block_transformer,
            place_sound: item.place_sound,
            empty_sound: item.empty_sound,
            empty_source: item.empty_source,
            bucket_entity: item.bucket_entity,
            blocks_attacks: components.blocks_attacks.is_some(),
            name: item.name,
            class: item.class,
            block: item.block,
            wall_block: item.wall_block,
            attachment: item.attachment,
        });
    }
    Ok((items, damageable, dyed))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{} 格式不对：{error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(entries: &[(&str, usize)]) -> RegistryReport {
        let entries = entries
            .iter()
            .map(|&(name, protocol_id)| (name.to_owned(), RegistryEntry { protocol_id }))
            .collect();
        RegistryReport { entries }
    }

    #[test]
    fn sorts_registry_entries_by_id() {
        let entries = entries_by_id("test", registry(&[("a", 1), ("b", 0)]));
        assert_eq!(entries, Ok(vec!["b".to_owned(), "a".to_owned()]));
    }

    #[test]
    fn rejects_registry_ids_that_are_not_positions() {
        assert!(entries_by_id("test", registry(&[("a", 0), ("b", 2)])).is_err());
        assert!(entries_by_id("test", registry(&[("a", 1)])).is_err());
    }

    #[test]
    fn export_order_has_to_match_the_report() {
        let first = "a".to_owned();
        let second = "b".to_owned();
        let report = BTreeMap::from([("test".to_owned(), vec![first.clone(), second.clone()])]);

        let check = |registry, names: &[&String]| {
            check_registry_order(&report, registry, names.iter().copied())
        };
        assert_eq!(check("test", &[&first, &second]), Ok(()));
        assert!(check("test", &[&second, &first]).is_err());
        assert!(check("test", &[&first]).is_err());
        assert!(check("missing", &[&first, &second]).is_err());
    }
}
