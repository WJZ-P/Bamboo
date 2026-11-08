//! 物品数据组件的完整编解码，照官方 `DataComponents` 里每一种组件的 `StreamCodec`。
//!
//! 这一层是附加的：[`crate::item::ItemStack`] 里的组件还是不透明的字节，
//! [`ComponentPatch::from_opaque`] 把那些字节解析成类型化的组件，
//! [`ComponentPatch::to_opaque`] 再变回去，[`validate::validate_stack`] 校验一个物品堆。
//!
//! 几点和官方不一样的地方，都是官方靠 Java 的异常和数据包大小兜着、Rust 必须自己挡住的：
//!
//! - 嵌套物品堆的层数上限 [`cursor::MAX_STACK_DEPTH`]；
//! - 组件补丁的条目数上限 [`MAX_PATCH_ENTRIES`]；
//! - 六种没有 `networkSynchronized` 的组件（`lock`、`container_loot`、`map_decorations`、
//!   `debug_stick_state`、`recipes`、`intangible_projectile`）线上是 NBT，官方会把 NBT 过一遍
//!   DFU 的 `Codec` 再写回去。这里只保留 NBT 并校验最外层的形状，不复刻 DFU。

pub mod cursor;
pub mod predicate;
pub mod stack;
pub mod text;
pub mod validate;
pub mod value;

use mc_bamboo_data::registries::data_component_type as kinds;
use mc_bamboo_nbt::Tag;

use crate::{
    codec::{DecodeError, write_bool, write_f32, write_i32, write_string, write_varint},
    item::ItemStack,
    text::Component,
};
use cursor::{
    Cursor, Holder, HolderSet, MAX_STRING, Registry, write_holder_set, write_identifier,
    write_list, write_optional,
};
use predicate::BlockPredicate;
use stack::ItemStackTemplate;
use value::{
    AXOLOTL_VARIANT, AttributeEntry, BannerLayer, BeeOccupant, ConsumeEffect, CustomModelData,
    DYE_COLOR, DamageReduction, Equippable, FOX_VARIANT, Filterable, FireworkExplosion, GlobalPos,
    HORSE_VARIANT, ITEM_USE_ANIMATION, Instrument, ItemDamageFunction, JukeboxSong,
    KineticCondition, LLAMA_VARIANT, MAP_POST_PROCESSING, MOOSHROOM_VARIANT, MobEffectInstance,
    PARROT_VARIANT, PaintingVariant, RABBIT_VARIANTS, RARITY, ResolvableFloat, ResolvableInt,
    ResolvableProfile, SALMON_SIZE, SignText, SoundEvent, StewEffect, SwingAnimation,
    TROPICAL_FISH_PATTERNS, ToolRule, TrimMaterial, TrimPattern, TypedEntityData, read_sparse,
};

/// 一个组件补丁里增加项和移除项的总数上限。
///
/// 官方不限制，但组件类型一共只有 122 种，合法的补丁不会有重复项。
/// 没有这个上限时，畸形包里每两个字节就能换来一个要递归解析的条目。
pub const MAX_PATCH_ENTRIES: usize = 256;

/// 官方 `ItemLore.MAX_LINES`。
const MAX_LORE_LINES: usize = 256;
/// 官方 `Fireworks.MAX_EXPLOSIONS`。
const MAX_EXPLOSIONS: usize = 256;
/// 官方 `ChargedProjectiles` 的上限。
const MAX_CHARGED_PROJECTILES: usize = 1024;
/// 官方 `ItemContainerContents.MAX_SIZE`。
const MAX_CONTAINER_SLOTS: usize = 256;
/// 官方 `WritableBookContent.MAX_PAGES`。
const MAX_BOOK_PAGES: usize = 100;
/// 官方 `WritableBookContent.PAGE_EDIT_LENGTH`。
const MAX_WRITABLE_PAGE: usize = 1024;
/// 官方 `WrittenBookContent.TITLE_MAX_LENGTH`。
const MAX_BOOK_TITLE: usize = 32;

/// 一种组件的值。变体的顺序和 `minecraft:data_component_type` 注册表一致。
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentValue {
    /// `custom_data`：数据包自己用的 NBT。
    CustomData(Vec<(String, Tag)>),
    /// `max_stack_size`。
    MaxStackSize(i32),
    /// `max_damage`。
    MaxDamage(i32),
    /// `damage`。
    Damage(i32),
    /// `unbreakable`。
    Unbreakable,
    /// `use_effects`。
    UseEffects {
        can_sprint: bool,
        interact_vibrations: bool,
        speed_multiplier: f32,
    },
    /// `custom_name`：玩家改过的名字。
    CustomName(Component),
    /// `minimum_attack_charge`。
    MinimumAttackCharge(f32),
    /// `damage_type`：`minecraft:damage_type` 的注册表 ID。
    DamageType(i32),
    /// `item_name`：物品自带的名字。
    ItemName(Component),
    /// `item_model`。
    ItemModel(String),
    /// `lore`。
    Lore(Vec<Component>),
    /// `rarity`，见 [`RARITY`]。
    Rarity(i32),
    /// `enchantments`：附魔的注册表 ID 和等级，按线上的顺序。
    Enchantments(Vec<(i32, i32)>),
    /// `can_place_on`。
    CanPlaceOn(Vec<BlockPredicate>),
    /// `can_break`。
    CanBreak(Vec<BlockPredicate>),
    /// `attribute_modifiers`。
    AttributeModifiers(Vec<AttributeEntry>),
    /// `custom_model_data`。
    CustomModelData(CustomModelData),
    /// `tooltip_display`。
    TooltipDisplay {
        hide_tooltip: bool,
        /// `minecraft:data_component_type` 的注册表 ID。
        hidden_components: Vec<i32>,
    },
    /// `repair_cost`。
    RepairCost(i32),
    /// `creative_slot_lock`。
    CreativeSlotLock,
    /// `enchantment_glint_override`。
    EnchantmentGlintOverride(bool),
    /// `intangible_projectile`。线上是一个内容无关的 `TAG_Compound`。
    IntangibleProjectile,
    /// `food`。
    Food {
        nutrition: i32,
        saturation: f32,
        can_always_eat: bool,
    },
    /// `consumable`。
    Consumable {
        consume_seconds: f32,
        /// 见 [`ITEM_USE_ANIMATION`]。
        animation: i32,
        sound: Holder<SoundEvent>,
        has_consume_particles: bool,
        on_consume_effects: Vec<ConsumeEffect>,
    },
    /// `use_remainder`。
    UseRemainder(ItemStackTemplate),
    /// `use_cooldown`。
    UseCooldown {
        seconds: f32,
        cooldown_group: Option<String>,
    },
    /// `damage_resistant`。
    DamageResistant(HolderSet),
    /// `tool`。
    Tool {
        rules: Vec<ToolRule>,
        default_mining_speed: f32,
        damage_per_block: i32,
        can_destroy_blocks_in_creative: bool,
    },
    /// `weapon`。
    Weapon {
        item_damage_per_attack: i32,
        disable_blocking_for_seconds: f32,
    },
    /// `attack_range`。
    AttackRange {
        min_reach: f32,
        max_reach: f32,
        min_creative_reach: f32,
        max_creative_reach: f32,
        hitbox_margin: f32,
        mob_factor: f32,
    },
    /// `enchantable`。
    Enchantable(i32),
    /// `equippable`。
    Equippable(Equippable),
    /// `repairable`。
    Repairable(HolderSet),
    /// `glider`。
    Glider,
    /// `tooltip_style`。
    TooltipStyle(String),
    /// `death_protection`。
    DeathProtection(Vec<ConsumeEffect>),
    /// `blocks_attacks`：盾牌。
    BlocksAttacks {
        block_delay_seconds: f32,
        disable_cooldown_scale: f32,
        damage_reductions: Vec<DamageReduction>,
        item_damage: ItemDamageFunction,
        bypassed_by: Option<HolderSet>,
        block_sound: Option<Holder<SoundEvent>>,
        disabled_sound: Option<Holder<SoundEvent>>,
    },
    /// `piercing_weapon`。
    PiercingWeapon {
        deals_knockback: bool,
        dismounts: bool,
        sound: Option<Holder<SoundEvent>>,
        hit_sound: Option<Holder<SoundEvent>>,
    },
    /// `kinetic_weapon`。
    KineticWeapon {
        contact_cooldown_ticks: i32,
        delay_ticks: i32,
        dismount_conditions: Option<KineticCondition>,
        knockback_conditions: Option<KineticCondition>,
        damage_conditions: Option<KineticCondition>,
        forward_movement: f32,
        damage_multiplier: f32,
        sound: Option<Holder<SoundEvent>>,
        hit_sound: Option<Holder<SoundEvent>>,
    },
    /// `attack_animation`。
    AttackAnimation(SwingAnimation),
    /// `interact_animation`。
    InteractAnimation(SwingAnimation),
    /// `additional_trade_cost`。
    AdditionalTradeCost(i32),
    /// `block_transformer`：`minecraft:block_transformer` 的注册表 ID。
    BlockTransformer(i32),
    /// `villager_food`。
    VillagerFood(i32),
    /// `stored_enchantments`：附魔书里的附魔。
    StoredEnchantments(Vec<(i32, i32)>),
    /// `dye`，见 [`DYE_COLOR`]。
    Dye(i32),
    /// `dyed_color`：皮革装备的颜色。
    DyedColor(i32),
    /// `map_id`。
    MapId(i32),
    /// `map_decorations`。线上是 NBT，只校验最外层是个复合标签。
    MapDecorations(Tag),
    /// `map_post_processing`，见 [`MAP_POST_PROCESSING`]。
    MapPostProcessing(i32),
    /// `charged_projectiles`：弩上的箭。
    ChargedProjectiles(Vec<ItemStackTemplate>),
    /// `bundle_contents`：收纳袋里的东西。
    BundleContents(Vec<ItemStackTemplate>),
    /// `potion_contents`。
    PotionContents {
        /// `minecraft:potion` 的注册表 ID。
        potion: Option<i32>,
        custom_color: Option<i32>,
        custom_effects: Vec<MobEffectInstance>,
        custom_name: Option<String>,
    },
    /// `potion_duration_scale`。
    PotionDurationScale(f32),
    /// `suspicious_stew_effects`。
    SuspiciousStewEffects(Vec<StewEffect>),
    /// `writable_book_content`：书与笔。
    WritableBookContent(Vec<Filterable<String>>),
    /// `written_book_content`：成书。
    WrittenBookContent {
        title: Filterable<String>,
        author: String,
        generation: i32,
        pages: Vec<Filterable<Component>>,
        resolved: bool,
    },
    /// `trim`：盔甲纹饰。
    Trim {
        material: Holder<TrimMaterial>,
        pattern: Holder<TrimPattern>,
    },
    /// `debug_stick_state`。线上是 NBT，只校验最外层是个复合标签。
    DebugStickState(Tag),
    /// `entity_data`：刷怪蛋这类物品里的实体数据。
    EntityData(TypedEntityData),
    /// `bucket_entity_data`：鱼桶里的鱼。
    BucketEntityData(Vec<(String, Tag)>),
    /// `block_entity_data`。
    BlockEntityData(TypedEntityData),
    /// `instrument`：山羊角。
    Instrument(Holder<Instrument>),
    /// `provides_trim_material`。
    ProvidesTrimMaterial(Holder<TrimMaterial>),
    /// `ominous_bottle_amplifier`：不祥之瓶。
    OminousBottleAmplifier(i32),
    /// `jukebox_playable`：唱片。
    JukeboxPlayable(Holder<JukeboxSong>),
    /// `provides_banner_patterns`。
    ProvidesBannerPatterns(HolderSet),
    /// `recipes`。线上是 NBT，只校验最外层是个列表。
    Recipes(Tag),
    /// `lodestone_tracker`：磁石指针。
    LodestoneTracker {
        target: Option<GlobalPos>,
        tracked: bool,
    },
    /// `firework_explosion`：烟花之星。
    FireworkExplosion(FireworkExplosion),
    /// `fireworks`：烟花火箭。
    Fireworks {
        flight_duration: i32,
        explosions: Vec<FireworkExplosion>,
    },
    /// `profile`：玩家头颅的档案。
    Profile(ResolvableProfile),
    /// `note_block_sound`。
    NoteBlockSound(String),
    /// `banner_patterns`：旗帜的图案。
    BannerPatterns(Vec<BannerLayer>),
    /// `base_color`：旗帜和盾牌的底色，见 [`DYE_COLOR`]。
    BaseColor(i32),
    /// `pot_decorations`：装饰罐四面的碎片。
    PotDecorations {
        back: Option<ItemStackTemplate>,
        left: Option<ItemStackTemplate>,
        right: Option<ItemStackTemplate>,
        front: Option<ItemStackTemplate>,
    },
    /// `container`：潜影盒这类容器里的物品，下标就是槽位。
    Container(Vec<Option<ItemStackTemplate>>),
    /// `block_state`：放下去时要覆盖的方块属性。
    BlockState(Vec<(String, String)>),
    /// `bees`：蜂巢里的蜜蜂。
    Bees(Vec<BeeOccupant>),
    /// `sulfur_cube_content`：硫磺块吞下的方块。
    SulfurCubeContent(ItemStackTemplate),
    /// `lock`。线上是 NBT，只校验最外层是个复合标签。
    Lock(Tag),
    /// `container_loot`。线上是 NBT，只校验最外层是个复合标签。
    ContainerLoot(Tag),
    /// `break_sound`。
    BreakSound(Holder<SoundEvent>),
    /// `compostable`：放进堆肥桶能加几层。
    Compostable(ResolvableInt),
    /// `cooking_fuel`。
    CookingFuel {
        burn_time: ResolvableInt,
        speed_multiplier: ResolvableFloat,
    },
    /// `brewing_fuel`。
    BrewingFuel {
        uses: ResolvableInt,
        speed_multiplier: ResolvableFloat,
    },
    /// `mob_visibility`：戴着它时怪物看得多远。
    MobVisibility {
        targeting_entity_types: HolderSet,
        visibility: f32,
    },
    /// `villager/variant`：`minecraft:villager_type` 的注册表 ID。
    VillagerVariant(i32),
    /// `wolf/variant`。
    WolfVariant(i32),
    /// `wolf/sound_variant`。
    WolfSoundVariant(i32),
    /// `wolf/collar`，见 [`DYE_COLOR`]。
    WolfCollar(i32),
    /// `fox/variant`，见 [`FOX_VARIANT`]。
    FoxVariant(i32),
    /// `salmon/size`，见 [`SALMON_SIZE`]。
    SalmonSize(i32),
    /// `parrot/variant`，见 [`PARROT_VARIANT`]。
    ParrotVariant(i32),
    /// `tropical_fish/pattern`，见 [`TROPICAL_FISH_PATTERNS`]。
    TropicalFishPattern(i32),
    /// `tropical_fish/base_color`，见 [`DYE_COLOR`]。
    TropicalFishBaseColor(i32),
    /// `tropical_fish/pattern_color`，见 [`DYE_COLOR`]。
    TropicalFishPatternColor(i32),
    /// `mooshroom/variant`，见 [`MOOSHROOM_VARIANT`]。
    MooshroomVariant(i32),
    /// `rabbit/variant`，见 [`RABBIT_VARIANTS`]。
    RabbitVariant(i32),
    /// `pig/variant`。
    PigVariant(i32),
    /// `pig/sound_variant`。
    PigSoundVariant(i32),
    /// `cow/variant`。
    CowVariant(i32),
    /// `cow/sound_variant`。
    CowSoundVariant(i32),
    /// `chicken/variant`。
    ChickenVariant(i32),
    /// `chicken/sound_variant`。
    ChickenSoundVariant(i32),
    /// `zombie_nautilus/variant`。
    ZombieNautilusVariant(i32),
    /// `frog/variant`。
    FrogVariant(i32),
    /// `horse/variant`，见 [`HORSE_VARIANT`]。
    HorseVariant(i32),
    /// `painting/variant`：画。
    PaintingVariant(Holder<PaintingVariant>),
    /// `llama/variant`，见 [`LLAMA_VARIANT`]。
    LlamaVariant(i32),
    /// `axolotl/variant`，见 [`AXOLOTL_VARIANT`]。
    AxolotlVariant(i32),
    /// `cat/variant`。
    CatVariant(i32),
    /// `cat/sound_variant`。
    CatSoundVariant(i32),
    /// `cat/collar`，见 [`DYE_COLOR`]。
    CatCollar(i32),
    /// `sheep/color`，见 [`DYE_COLOR`]。
    SheepColor(i32),
    /// `shulker/color`，见 [`DYE_COLOR`]。
    ShulkerColor(i32),
    /// `provides_pottery_pattern`：`minecraft:decorated_pot_pattern` 的注册表 ID。
    ProvidesPotteryPattern(i32),
    /// `sign_text_front`。
    SignTextFront(SignText),
    /// `sign_text_back`。
    SignTextBack(SignText),
    /// `waxed`。
    Waxed,
    /// `cushion/color`，见 [`DYE_COLOR`]。
    CushionColor(i32),
}

impl ComponentValue {
    /// 按组件类型 ID 解码一个值，格式和官方的 `streamCodec` 一致。
    pub fn read(cursor: &mut Cursor<'_>, kind: i32) -> Result<Self, DecodeError> {
        Ok(match kind {
            kinds::CUSTOM_DATA => Self::CustomData(cursor.nbt_compound()?),
            kinds::MAX_STACK_SIZE => Self::MaxStackSize(cursor.varint()?),
            kinds::MAX_DAMAGE => Self::MaxDamage(cursor.varint()?),
            kinds::DAMAGE => Self::Damage(cursor.varint()?),
            kinds::UNBREAKABLE => Self::Unbreakable,
            kinds::USE_EFFECTS => Self::UseEffects {
                can_sprint: cursor.bool()?,
                interact_vibrations: cursor.bool()?,
                speed_multiplier: cursor.f32()?,
            },
            kinds::CUSTOM_NAME => Self::CustomName(text::read(cursor)?),
            kinds::MINIMUM_ATTACK_CHARGE => Self::MinimumAttackCharge(cursor.f32()?),
            kinds::DAMAGE_TYPE => Self::DamageType(cursor.registry_id(Registry::DAMAGE_TYPE)?),
            kinds::ITEM_NAME => Self::ItemName(text::read(cursor)?),
            kinds::ITEM_MODEL => Self::ItemModel(cursor.identifier()?),
            kinds::LORE => Self::Lore(cursor.list(MAX_LORE_LINES, text::read)?),
            kinds::RARITY => Self::Rarity(RARITY.read(cursor)?),
            kinds::ENCHANTMENTS => Self::Enchantments(read_enchantments(cursor)?),
            kinds::CAN_PLACE_ON => Self::CanPlaceOn(cursor.list(usize::MAX, BlockPredicate::read)?),
            kinds::CAN_BREAK => Self::CanBreak(cursor.list(usize::MAX, BlockPredicate::read)?),
            kinds::ATTRIBUTE_MODIFIERS => {
                Self::AttributeModifiers(cursor.list(usize::MAX, AttributeEntry::read)?)
            }
            kinds::CUSTOM_MODEL_DATA => Self::CustomModelData(CustomModelData::read(cursor)?),
            kinds::TOOLTIP_DISPLAY => Self::TooltipDisplay {
                hide_tooltip: cursor.bool()?,
                hidden_components: cursor.list(usize::MAX, |cursor| {
                    cursor.registry_id(Registry::DATA_COMPONENT_TYPE)
                })?,
            },
            kinds::REPAIR_COST => Self::RepairCost(cursor.varint()?),
            kinds::CREATIVE_SLOT_LOCK => Self::CreativeSlotLock,
            kinds::ENCHANTMENT_GLINT_OVERRIDE => Self::EnchantmentGlintOverride(cursor.bool()?),
            kinds::INTANGIBLE_PROJECTILE => {
                // 官方走 `Unit.CODEC`：必须是复合标签，内容不看，写回去永远是空的。
                cursor.nbt_compound()?;
                Self::IntangibleProjectile
            }
            kinds::FOOD => Self::Food {
                nutrition: cursor.varint()?,
                saturation: cursor.f32()?,
                can_always_eat: cursor.bool()?,
            },
            kinds::CONSUMABLE => Self::Consumable {
                consume_seconds: cursor.f32()?,
                animation: ITEM_USE_ANIMATION.read(cursor)?,
                sound: SoundEvent::read(cursor)?,
                has_consume_particles: cursor.bool()?,
                on_consume_effects: cursor.list(usize::MAX, ConsumeEffect::read)?,
            },
            kinds::USE_REMAINDER => Self::UseRemainder(ItemStackTemplate::read(cursor)?),
            kinds::USE_COOLDOWN => Self::UseCooldown {
                seconds: cursor.f32()?,
                cooldown_group: cursor.optional(Cursor::identifier)?,
            },
            kinds::DAMAGE_RESISTANT => {
                Self::DamageResistant(cursor.holder_set(Registry::DAMAGE_TYPE)?)
            }
            kinds::TOOL => Self::Tool {
                rules: cursor.list(usize::MAX, ToolRule::read)?,
                default_mining_speed: cursor.f32()?,
                damage_per_block: cursor.varint()?,
                can_destroy_blocks_in_creative: cursor.bool()?,
            },
            kinds::WEAPON => Self::Weapon {
                item_damage_per_attack: cursor.varint()?,
                disable_blocking_for_seconds: cursor.f32()?,
            },
            kinds::ATTACK_RANGE => Self::AttackRange {
                min_reach: cursor.f32()?,
                max_reach: cursor.f32()?,
                min_creative_reach: cursor.f32()?,
                max_creative_reach: cursor.f32()?,
                hitbox_margin: cursor.f32()?,
                mob_factor: cursor.f32()?,
            },
            kinds::ENCHANTABLE => Self::Enchantable(cursor.varint()?),
            kinds::EQUIPPABLE => Self::Equippable(Equippable::read(cursor)?),
            kinds::REPAIRABLE => Self::Repairable(cursor.holder_set(Registry::ITEM)?),
            kinds::GLIDER => Self::Glider,
            kinds::TOOLTIP_STYLE => Self::TooltipStyle(cursor.identifier()?),
            kinds::DEATH_PROTECTION => {
                Self::DeathProtection(cursor.list(usize::MAX, ConsumeEffect::read)?)
            }
            kinds::BLOCKS_ATTACKS => Self::BlocksAttacks {
                block_delay_seconds: cursor.f32()?,
                disable_cooldown_scale: cursor.f32()?,
                damage_reductions: cursor.list(usize::MAX, DamageReduction::read)?,
                item_damage: ItemDamageFunction::read(cursor)?,
                bypassed_by: cursor.optional(|cursor| cursor.holder_set(Registry::DAMAGE_TYPE))?,
                block_sound: cursor.optional(SoundEvent::read)?,
                disabled_sound: cursor.optional(SoundEvent::read)?,
            },
            kinds::PIERCING_WEAPON => Self::PiercingWeapon {
                deals_knockback: cursor.bool()?,
                dismounts: cursor.bool()?,
                sound: cursor.optional(SoundEvent::read)?,
                hit_sound: cursor.optional(SoundEvent::read)?,
            },
            kinds::KINETIC_WEAPON => Self::KineticWeapon {
                contact_cooldown_ticks: cursor.varint()?,
                delay_ticks: cursor.varint()?,
                dismount_conditions: cursor.optional(KineticCondition::read)?,
                knockback_conditions: cursor.optional(KineticCondition::read)?,
                damage_conditions: cursor.optional(KineticCondition::read)?,
                forward_movement: cursor.f32()?,
                damage_multiplier: cursor.f32()?,
                sound: cursor.optional(SoundEvent::read)?,
                hit_sound: cursor.optional(SoundEvent::read)?,
            },
            kinds::ATTACK_ANIMATION => Self::AttackAnimation(SwingAnimation::read(cursor)?),
            kinds::INTERACT_ANIMATION => Self::InteractAnimation(SwingAnimation::read(cursor)?),
            kinds::ADDITIONAL_TRADE_COST => Self::AdditionalTradeCost(cursor.varint()?),
            kinds::BLOCK_TRANSFORMER => {
                Self::BlockTransformer(cursor.registry_id(Registry::BLOCK_TRANSFORMER)?)
            }
            kinds::VILLAGER_FOOD => Self::VillagerFood(cursor.varint()?),
            kinds::STORED_ENCHANTMENTS => Self::StoredEnchantments(read_enchantments(cursor)?),
            kinds::DYE => Self::Dye(DYE_COLOR.read(cursor)?),
            kinds::DYED_COLOR => Self::DyedColor(cursor.i32()?),
            kinds::MAP_ID => Self::MapId(cursor.varint()?),
            kinds::MAP_DECORATIONS => {
                Self::MapDecorations(read_nbt_shaped(cursor, Shape::Compound)?)
            }
            kinds::MAP_POST_PROCESSING => {
                Self::MapPostProcessing(MAP_POST_PROCESSING.read(cursor)?)
            }
            kinds::CHARGED_PROJECTILES => Self::ChargedProjectiles(
                cursor.list(MAX_CHARGED_PROJECTILES, ItemStackTemplate::read)?,
            ),
            kinds::BUNDLE_CONTENTS => {
                Self::BundleContents(cursor.list(usize::MAX, ItemStackTemplate::read)?)
            }
            kinds::POTION_CONTENTS => Self::PotionContents {
                potion: cursor.optional(|cursor| cursor.registry_id(Registry::POTION))?,
                custom_color: cursor.optional(Cursor::i32)?,
                custom_effects: cursor.list(usize::MAX, MobEffectInstance::read)?,
                custom_name: cursor.optional(|cursor| cursor.string(MAX_STRING))?,
            },
            kinds::POTION_DURATION_SCALE => Self::PotionDurationScale(cursor.f32()?),
            kinds::SUSPICIOUS_STEW_EFFECTS => {
                Self::SuspiciousStewEffects(cursor.list(usize::MAX, StewEffect::read)?)
            }
            kinds::WRITABLE_BOOK_CONTENT => {
                Self::WritableBookContent(cursor.list(MAX_BOOK_PAGES, |cursor| {
                    Filterable::read(cursor, |cursor| cursor.string(MAX_WRITABLE_PAGE))
                })?)
            }
            kinds::WRITTEN_BOOK_CONTENT => Self::WrittenBookContent {
                title: Filterable::read(cursor, |cursor| cursor.string(MAX_BOOK_TITLE))?,
                author: cursor.string(MAX_STRING)?,
                generation: cursor.varint()?,
                pages: cursor.list(usize::MAX, |cursor| Filterable::read(cursor, text::read))?,
                resolved: cursor.bool()?,
            },
            kinds::TRIM => Self::Trim {
                material: TrimMaterial::read(cursor)?,
                pattern: TrimPattern::read(cursor)?,
            },
            kinds::DEBUG_STICK_STATE => {
                Self::DebugStickState(read_nbt_shaped(cursor, Shape::Compound)?)
            }
            kinds::ENTITY_DATA => {
                Self::EntityData(TypedEntityData::read(cursor, Registry::ENTITY_TYPE)?)
            }
            kinds::BUCKET_ENTITY_DATA => Self::BucketEntityData(cursor.nbt_compound()?),
            kinds::BLOCK_ENTITY_DATA => {
                Self::BlockEntityData(TypedEntityData::read(cursor, Registry::BLOCK_ENTITY_TYPE)?)
            }
            kinds::INSTRUMENT => Self::Instrument(Instrument::read(cursor)?),
            kinds::PROVIDES_TRIM_MATERIAL => {
                Self::ProvidesTrimMaterial(TrimMaterial::read(cursor)?)
            }
            kinds::OMINOUS_BOTTLE_AMPLIFIER => Self::OminousBottleAmplifier(cursor.varint()?),
            kinds::JUKEBOX_PLAYABLE => Self::JukeboxPlayable(JukeboxSong::read(cursor)?),
            kinds::PROVIDES_BANNER_PATTERNS => {
                Self::ProvidesBannerPatterns(cursor.holder_set(Registry::BANNER_PATTERN)?)
            }
            kinds::RECIPES => Self::Recipes(read_nbt_shaped(cursor, Shape::List)?),
            kinds::LODESTONE_TRACKER => Self::LodestoneTracker {
                target: cursor.optional(GlobalPos::read)?,
                tracked: cursor.bool()?,
            },
            kinds::FIREWORK_EXPLOSION => Self::FireworkExplosion(FireworkExplosion::read(cursor)?),
            kinds::FIREWORKS => Self::Fireworks {
                flight_duration: cursor.varint()?,
                explosions: cursor.list(MAX_EXPLOSIONS, FireworkExplosion::read)?,
            },
            kinds::PROFILE => Self::Profile(ResolvableProfile::read(cursor)?),
            kinds::NOTE_BLOCK_SOUND => Self::NoteBlockSound(cursor.identifier()?),
            kinds::BANNER_PATTERNS => {
                Self::BannerPatterns(cursor.list(usize::MAX, BannerLayer::read)?)
            }
            kinds::BASE_COLOR => Self::BaseColor(DYE_COLOR.read(cursor)?),
            kinds::POT_DECORATIONS => Self::PotDecorations {
                back: cursor.optional(ItemStackTemplate::read)?,
                left: cursor.optional(ItemStackTemplate::read)?,
                right: cursor.optional(ItemStackTemplate::read)?,
                front: cursor.optional(ItemStackTemplate::read)?,
            },
            kinds::CONTAINER => Self::Container(cursor.list(MAX_CONTAINER_SLOTS, |cursor| {
                cursor.optional(ItemStackTemplate::read)
            })?),
            kinds::BLOCK_STATE => Self::BlockState(cursor.map(
                usize::MAX,
                |cursor| cursor.string(MAX_STRING),
                |cursor| cursor.string(MAX_STRING),
            )?),
            kinds::BEES => Self::Bees(cursor.list(usize::MAX, BeeOccupant::read)?),
            kinds::SULFUR_CUBE_CONTENT => Self::SulfurCubeContent(ItemStackTemplate::read(cursor)?),
            kinds::LOCK => Self::Lock(read_nbt_shaped(cursor, Shape::Compound)?),
            kinds::CONTAINER_LOOT => Self::ContainerLoot(read_nbt_shaped(cursor, Shape::Compound)?),
            kinds::BREAK_SOUND => Self::BreakSound(SoundEvent::read(cursor)?),
            kinds::COMPOSTABLE => Self::Compostable(ResolvableInt::read(cursor)?),
            kinds::COOKING_FUEL => Self::CookingFuel {
                burn_time: ResolvableInt::read(cursor)?,
                speed_multiplier: ResolvableFloat::read(cursor)?,
            },
            kinds::BREWING_FUEL => Self::BrewingFuel {
                uses: ResolvableInt::read(cursor)?,
                speed_multiplier: ResolvableFloat::read(cursor)?,
            },
            kinds::MOB_VISIBILITY => Self::MobVisibility {
                targeting_entity_types: cursor.holder_set(Registry::ENTITY_TYPE)?,
                visibility: cursor.f32()?,
            },
            kinds::VILLAGER_VARIANT => {
                Self::VillagerVariant(cursor.registry_id(Registry::VILLAGER_TYPE)?)
            }
            kinds::WOLF_VARIANT => Self::WolfVariant(cursor.registry_id(Registry::WOLF_VARIANT)?),
            kinds::WOLF_SOUND_VARIANT => {
                Self::WolfSoundVariant(cursor.registry_id(Registry::WOLF_SOUND_VARIANT)?)
            }
            kinds::WOLF_COLLAR => Self::WolfCollar(DYE_COLOR.read(cursor)?),
            kinds::FOX_VARIANT => Self::FoxVariant(FOX_VARIANT.read(cursor)?),
            kinds::SALMON_SIZE => Self::SalmonSize(SALMON_SIZE.read(cursor)?),
            kinds::PARROT_VARIANT => Self::ParrotVariant(PARROT_VARIANT.read(cursor)?),
            kinds::TROPICAL_FISH_PATTERN => {
                Self::TropicalFishPattern(read_sparse(cursor, &TROPICAL_FISH_PATTERNS, 0)?)
            }
            kinds::TROPICAL_FISH_BASE_COLOR => Self::TropicalFishBaseColor(DYE_COLOR.read(cursor)?),
            kinds::TROPICAL_FISH_PATTERN_COLOR => {
                Self::TropicalFishPatternColor(DYE_COLOR.read(cursor)?)
            }
            kinds::MOOSHROOM_VARIANT => Self::MooshroomVariant(MOOSHROOM_VARIANT.read(cursor)?),
            kinds::RABBIT_VARIANT => Self::RabbitVariant(read_sparse(cursor, &RABBIT_VARIANTS, 0)?),
            kinds::PIG_VARIANT => Self::PigVariant(cursor.registry_id(Registry::PIG_VARIANT)?),
            kinds::PIG_SOUND_VARIANT => {
                Self::PigSoundVariant(cursor.registry_id(Registry::PIG_SOUND_VARIANT)?)
            }
            kinds::COW_VARIANT => Self::CowVariant(cursor.registry_id(Registry::COW_VARIANT)?),
            kinds::COW_SOUND_VARIANT => {
                Self::CowSoundVariant(cursor.registry_id(Registry::COW_SOUND_VARIANT)?)
            }
            kinds::CHICKEN_VARIANT => {
                Self::ChickenVariant(cursor.registry_id(Registry::CHICKEN_VARIANT)?)
            }
            kinds::CHICKEN_SOUND_VARIANT => {
                Self::ChickenSoundVariant(cursor.registry_id(Registry::CHICKEN_SOUND_VARIANT)?)
            }
            kinds::ZOMBIE_NAUTILUS_VARIANT => {
                Self::ZombieNautilusVariant(cursor.registry_id(Registry::ZOMBIE_NAUTILUS_VARIANT)?)
            }
            kinds::FROG_VARIANT => Self::FrogVariant(cursor.registry_id(Registry::FROG_VARIANT)?),
            kinds::HORSE_VARIANT => Self::HorseVariant(HORSE_VARIANT.read(cursor)?),
            kinds::PAINTING_VARIANT => Self::PaintingVariant(PaintingVariant::read(cursor)?),
            kinds::LLAMA_VARIANT => Self::LlamaVariant(LLAMA_VARIANT.read(cursor)?),
            kinds::AXOLOTL_VARIANT => Self::AxolotlVariant(AXOLOTL_VARIANT.read(cursor)?),
            kinds::CAT_VARIANT => Self::CatVariant(cursor.registry_id(Registry::CAT_VARIANT)?),
            kinds::CAT_SOUND_VARIANT => {
                Self::CatSoundVariant(cursor.registry_id(Registry::CAT_SOUND_VARIANT)?)
            }
            kinds::CAT_COLLAR => Self::CatCollar(DYE_COLOR.read(cursor)?),
            kinds::SHEEP_COLOR => Self::SheepColor(DYE_COLOR.read(cursor)?),
            kinds::SHULKER_COLOR => Self::ShulkerColor(DYE_COLOR.read(cursor)?),
            kinds::PROVIDES_POTTERY_PATTERN => {
                Self::ProvidesPotteryPattern(cursor.registry_id(Registry::DECORATED_POT_PATTERN)?)
            }
            kinds::SIGN_TEXT_FRONT => Self::SignTextFront(SignText::read(cursor)?),
            kinds::SIGN_TEXT_BACK => Self::SignTextBack(SignText::read(cursor)?),
            kinds::WAXED => Self::Waxed,
            kinds::CUSHION_COLOR => Self::CushionColor(DYE_COLOR.read(cursor)?),
            id => return Err(DecodeError::UnknownComponent(id)),
        })
    }

    /// 按官方格式写回去。
    #[expect(clippy::too_many_lines, reason = "一百多种组件，一种一条分支")]
    pub fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::CustomData(tag) | Self::BucketEntityData(tag) => {
                Tag::Compound(tag.clone()).write_network(out);
            }
            Self::MaxStackSize(value)
            | Self::MaxDamage(value)
            | Self::Damage(value)
            | Self::RepairCost(value)
            | Self::Enchantable(value)
            | Self::AdditionalTradeCost(value)
            | Self::VillagerFood(value)
            | Self::MapId(value)
            | Self::OminousBottleAmplifier(value) => write_varint(out, *value),
            Self::Unbreakable | Self::CreativeSlotLock | Self::Glider | Self::Waxed => {}
            Self::IntangibleProjectile => Tag::Compound(Vec::new()).write_network(out),
            Self::UseEffects {
                can_sprint,
                interact_vibrations,
                speed_multiplier,
            } => {
                write_bool(out, *can_sprint);
                write_bool(out, *interact_vibrations);
                write_f32(out, *speed_multiplier);
            }
            Self::CustomName(component) | Self::ItemName(component) => {
                text::write(out, component);
            }
            Self::MinimumAttackCharge(value) | Self::PotionDurationScale(value) => {
                write_f32(out, *value)
            }
            Self::DamageType(id)
            | Self::BlockTransformer(id)
            | Self::VillagerVariant(id)
            | Self::WolfVariant(id)
            | Self::WolfSoundVariant(id)
            | Self::PigVariant(id)
            | Self::PigSoundVariant(id)
            | Self::CowVariant(id)
            | Self::CowSoundVariant(id)
            | Self::ChickenVariant(id)
            | Self::ChickenSoundVariant(id)
            | Self::ZombieNautilusVariant(id)
            | Self::FrogVariant(id)
            | Self::CatVariant(id)
            | Self::CatSoundVariant(id)
            | Self::ProvidesPotteryPattern(id)
            | Self::Rarity(id)
            | Self::Dye(id)
            | Self::BaseColor(id)
            | Self::WolfCollar(id)
            | Self::FoxVariant(id)
            | Self::SalmonSize(id)
            | Self::ParrotVariant(id)
            | Self::TropicalFishPattern(id)
            | Self::TropicalFishBaseColor(id)
            | Self::TropicalFishPatternColor(id)
            | Self::MooshroomVariant(id)
            | Self::RabbitVariant(id)
            | Self::HorseVariant(id)
            | Self::LlamaVariant(id)
            | Self::AxolotlVariant(id)
            | Self::CatCollar(id)
            | Self::SheepColor(id)
            | Self::ShulkerColor(id)
            | Self::CushionColor(id)
            | Self::MapPostProcessing(id) => write_varint(out, *id),
            Self::ItemModel(id) | Self::TooltipStyle(id) | Self::NoteBlockSound(id) => {
                write_identifier(out, id)
            }
            Self::Lore(lines) => write_list(out, lines, text::write),
            Self::Enchantments(levels) | Self::StoredEnchantments(levels) => {
                write_enchantments(out, levels);
            }
            Self::CanPlaceOn(predicates) | Self::CanBreak(predicates) => {
                write_list(out, predicates, BlockPredicate::write);
            }
            Self::AttributeModifiers(entries) => write_list(out, entries, AttributeEntry::write),
            Self::CustomModelData(data) => data.write(out),
            Self::TooltipDisplay {
                hide_tooltip,
                hidden_components,
            } => {
                write_bool(out, *hide_tooltip);
                write_list(out, hidden_components, |out, &id| write_varint(out, id));
            }
            Self::EnchantmentGlintOverride(value) => write_bool(out, *value),
            Self::Food {
                nutrition,
                saturation,
                can_always_eat,
            } => {
                write_varint(out, *nutrition);
                write_f32(out, *saturation);
                write_bool(out, *can_always_eat);
            }
            Self::Consumable {
                consume_seconds,
                animation,
                sound,
                has_consume_particles,
                on_consume_effects,
            } => {
                write_f32(out, *consume_seconds);
                write_varint(out, *animation);
                SoundEvent::write(out, sound);
                write_bool(out, *has_consume_particles);
                write_list(out, on_consume_effects, |out, effect| effect.write(out));
            }
            Self::UseRemainder(stack) | Self::SulfurCubeContent(stack) => {
                ItemStackTemplate::write(out, stack);
            }
            Self::UseCooldown {
                seconds,
                cooldown_group,
            } => {
                write_f32(out, *seconds);
                write_optional(out, cooldown_group.as_ref(), |out, id| {
                    write_identifier(out, id);
                });
            }
            Self::DamageResistant(set)
            | Self::Repairable(set)
            | Self::ProvidesBannerPatterns(set) => write_holder_set(out, set),
            Self::Tool {
                rules,
                default_mining_speed,
                damage_per_block,
                can_destroy_blocks_in_creative,
            } => {
                write_list(out, rules, ToolRule::write);
                write_f32(out, *default_mining_speed);
                write_varint(out, *damage_per_block);
                write_bool(out, *can_destroy_blocks_in_creative);
            }
            Self::Weapon {
                item_damage_per_attack,
                disable_blocking_for_seconds,
            } => {
                write_varint(out, *item_damage_per_attack);
                write_f32(out, *disable_blocking_for_seconds);
            }
            Self::AttackRange {
                min_reach,
                max_reach,
                min_creative_reach,
                max_creative_reach,
                hitbox_margin,
                mob_factor,
            } => {
                for value in [
                    min_reach,
                    max_reach,
                    min_creative_reach,
                    max_creative_reach,
                    hitbox_margin,
                    mob_factor,
                ] {
                    write_f32(out, *value);
                }
            }
            Self::Equippable(equippable) => equippable.write(out),
            Self::DeathProtection(effects) => {
                write_list(out, effects, |out, effect| effect.write(out));
            }
            Self::BlocksAttacks {
                block_delay_seconds,
                disable_cooldown_scale,
                damage_reductions,
                item_damage,
                bypassed_by,
                block_sound,
                disabled_sound,
            } => {
                write_f32(out, *block_delay_seconds);
                write_f32(out, *disable_cooldown_scale);
                write_list(out, damage_reductions, DamageReduction::write);
                item_damage.write(out);
                write_optional(out, bypassed_by.as_ref(), write_holder_set);
                write_optional(out, block_sound.as_ref(), SoundEvent::write);
                write_optional(out, disabled_sound.as_ref(), SoundEvent::write);
            }
            Self::PiercingWeapon {
                deals_knockback,
                dismounts,
                sound,
                hit_sound,
            } => {
                write_bool(out, *deals_knockback);
                write_bool(out, *dismounts);
                write_optional(out, sound.as_ref(), SoundEvent::write);
                write_optional(out, hit_sound.as_ref(), SoundEvent::write);
            }
            Self::KineticWeapon {
                contact_cooldown_ticks,
                delay_ticks,
                dismount_conditions,
                knockback_conditions,
                damage_conditions,
                forward_movement,
                damage_multiplier,
                sound,
                hit_sound,
            } => {
                write_varint(out, *contact_cooldown_ticks);
                write_varint(out, *delay_ticks);
                for condition in [dismount_conditions, knockback_conditions, damage_conditions] {
                    write_optional(out, condition.as_ref(), |out, condition| {
                        condition.write(out);
                    });
                }
                write_f32(out, *forward_movement);
                write_f32(out, *damage_multiplier);
                write_optional(out, sound.as_ref(), SoundEvent::write);
                write_optional(out, hit_sound.as_ref(), SoundEvent::write);
            }
            Self::AttackAnimation(animation) | Self::InteractAnimation(animation) => {
                animation.write(out);
            }
            Self::DyedColor(color) => write_i32(out, *color),
            Self::MapDecorations(tag)
            | Self::DebugStickState(tag)
            | Self::Recipes(tag)
            | Self::Lock(tag)
            | Self::ContainerLoot(tag) => tag.write_network(out),
            Self::ChargedProjectiles(stacks) | Self::BundleContents(stacks) => {
                write_list(out, stacks, ItemStackTemplate::write);
            }
            Self::PotionContents {
                potion,
                custom_color,
                custom_effects,
                custom_name,
            } => {
                write_optional(out, potion.as_ref(), |out, &id| write_varint(out, id));
                write_optional(out, custom_color.as_ref(), |out, &color| {
                    write_i32(out, color);
                });
                write_list(out, custom_effects, MobEffectInstance::write);
                write_optional(out, custom_name.as_ref(), |out, name| {
                    write_string(out, name);
                });
            }
            Self::SuspiciousStewEffects(effects) => write_list(out, effects, StewEffect::write),
            Self::WritableBookContent(pages) => {
                write_list(out, pages, |out, page| {
                    Filterable::write(out, page, |out, text| write_string(out, text));
                });
            }
            Self::WrittenBookContent {
                title,
                author,
                generation,
                pages,
                resolved,
            } => {
                Filterable::write(out, title, |out, text| write_string(out, text));
                write_string(out, author);
                write_varint(out, *generation);
                write_list(out, pages, |out, page| {
                    Filterable::write(out, page, text::write);
                });
                write_bool(out, *resolved);
            }
            Self::Trim { material, pattern } => {
                TrimMaterial::write(out, material);
                TrimPattern::write(out, pattern);
            }
            Self::EntityData(data) | Self::BlockEntityData(data) => data.write(out),
            Self::Instrument(instrument) => Instrument::write(out, instrument),
            Self::ProvidesTrimMaterial(material) => TrimMaterial::write(out, material),
            Self::JukeboxPlayable(song) => JukeboxSong::write(out, song),
            Self::LodestoneTracker { target, tracked } => {
                write_optional(out, target.as_ref(), |out, pos| pos.write(out));
                write_bool(out, *tracked);
            }
            Self::FireworkExplosion(explosion) => FireworkExplosion::write(out, explosion),
            Self::Fireworks {
                flight_duration,
                explosions,
            } => {
                write_varint(out, *flight_duration);
                write_list(out, explosions, FireworkExplosion::write);
            }
            Self::Profile(profile) => profile.write(out),
            Self::BannerPatterns(layers) => write_list(out, layers, BannerLayer::write),
            Self::PotDecorations {
                back,
                left,
                right,
                front,
            } => {
                for side in [back, left, right, front] {
                    write_optional(out, side.as_ref(), ItemStackTemplate::write);
                }
            }
            Self::Container(slots) => {
                write_list(out, slots, |out, slot| {
                    write_optional(out, slot.as_ref(), ItemStackTemplate::write);
                });
            }
            Self::BlockState(properties) => {
                write_varint(out, properties.len() as i32);
                for (name, value) in properties {
                    write_string(out, name);
                    write_string(out, value);
                }
            }
            Self::Bees(occupants) => write_list(out, occupants, BeeOccupant::write),
            Self::BreakSound(sound) => SoundEvent::write(out, sound),
            Self::Compostable(layers) => layers.write(out),
            Self::CookingFuel {
                burn_time,
                speed_multiplier,
            } => {
                burn_time.write(out);
                speed_multiplier.write(out);
            }
            Self::BrewingFuel {
                uses,
                speed_multiplier,
            } => {
                uses.write(out);
                speed_multiplier.write(out);
            }
            Self::MobVisibility {
                targeting_entity_types,
                visibility,
            } => {
                write_holder_set(out, targeting_entity_types);
                write_f32(out, *visibility);
            }
            Self::PaintingVariant(variant) => PaintingVariant::write(out, variant),
            Self::SignTextFront(text) | Self::SignTextBack(text) => text.write(out),
        }
    }
}

/// 官方 `ItemEnchantments.STREAM_CODEC` 的映射表。
fn read_enchantments(cursor: &mut Cursor<'_>) -> Result<Vec<(i32, i32)>, DecodeError> {
    cursor.map(
        usize::MAX,
        |cursor| cursor.registry_id(Registry::ENCHANTMENT),
        Cursor::varint,
    )
}

fn write_enchantments(out: &mut Vec<u8>, levels: &[(i32, i32)]) {
    write_varint(out, levels.len() as i32);
    for &(enchantment, level) in levels {
        write_varint(out, enchantment);
        write_varint(out, level);
    }
}

/// 没有 `networkSynchronized` 的那几种组件在线上的 NBT 最外层应该是什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// 官方的 `Codec` 最外层是个 map。
    Compound,
    /// 官方的 `Codec` 最外层是个 list。
    List,
}

/// 读一个 NBT 标签并校验最外层的形状。
///
/// 这几种组件官方会把 NBT 过一遍 DFU 的 `Codec`，再按解出来的值写回去。
/// 我们保留原来的标签，所以官方写出来的字节能原样写回；但只校验到最外层。
fn read_nbt_shaped(cursor: &mut Cursor<'_>, shape: Shape) -> Result<Tag, DecodeError> {
    let tag = cursor.nbt_tag()?;
    let matches = match shape {
        Shape::Compound => matches!(tag, Tag::Compound(_)),
        Shape::List => matches!(
            tag,
            Tag::List(_) | Tag::ByteArray(_) | Tag::IntArray(_) | Tag::LongArray(_)
        ),
    };
    if matches {
        Ok(tag)
    } else {
        Err(DecodeError::InvalidComponentValue(
            "component NBT has the wrong shape",
        ))
    }
}

/// 补丁里的一项：设成某个值，或者移除。
#[derive(Debug, Clone, PartialEq)]
pub enum PatchEntry {
    Set(ComponentValue),
    Removed,
}

/// 官方 `DataComponentPatch`：一串「设成这个值」和「移除」。
///
/// 条目按线上的顺序存。官方的 `Reference2ObjectArrayMap` 也是插入序，
/// 写回去时先写全部增加项再写全部移除项，顺序和官方一致。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComponentPatch {
    entries: Vec<(i32, PatchEntry)>,
}

impl ComponentPatch {
    /// 官方 `DataComponentPatch.STREAM_CODEC`：值前面没有长度。
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        let added = cursor.varint()?;
        let removed = cursor.varint()?;
        let added = count(added)?;
        let removed = count(removed)?;
        if added + removed > MAX_PATCH_ENTRIES {
            return Err(DecodeError::InvalidCount((added + removed) as i32));
        }

        let mut patch = Self::default();
        for _ in 0..added {
            let kind = cursor.registry_id(Registry::DATA_COMPONENT_TYPE)?;
            let value = ComponentValue::read(cursor, kind)?;
            patch.put(kind, PatchEntry::Set(value));
        }
        for _ in 0..removed {
            let kind = cursor.registry_id(Registry::DATA_COMPONENT_TYPE)?;
            patch.put(kind, PatchEntry::Removed);
        }
        Ok(patch)
    }

    /// 官方 `DataComponentPatch.DELIMITED_STREAM_CODEC`：`set_creative_mode_slot` 用的格式，
    /// 每个值前面有字节长度。
    pub fn read_delimited(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        let added = cursor.count(MAX_PATCH_ENTRIES)?;
        let removed = cursor.count(MAX_PATCH_ENTRIES)?;
        if added + removed > MAX_PATCH_ENTRIES {
            return Err(DecodeError::InvalidCount((added + removed) as i32));
        }

        let mut patch = Self::default();
        for _ in 0..added {
            let kind = cursor.registry_id(Registry::DATA_COMPONENT_TYPE)?;
            let len = cursor.count(usize::MAX)?;
            let bytes = cursor.bytes(len)?;
            let mut inner = Cursor::new(bytes);
            let value = ComponentValue::read(&mut inner, kind)?;
            inner.finish()?;
            patch.put(kind, PatchEntry::Set(value));
        }
        for _ in 0..removed {
            let kind = cursor.registry_id(Registry::DATA_COMPONENT_TYPE)?;
            patch.put(kind, PatchEntry::Removed);
        }
        Ok(patch)
    }

    /// 把 [`ItemStack`] 里不透明的字节解析成类型化的组件。
    ///
    /// 每一项必须刚好用完自己那段字节，多出来的字节算畸形。
    pub fn from_opaque(stack: &ItemStack) -> Result<Self, DecodeError> {
        let mut patch = Self::default();
        for (kind, bytes) in &stack.added {
            if !u32::try_from(*kind).is_ok_and(|id| id < Registry::DATA_COMPONENT_TYPE.size) {
                return Err(DecodeError::UnknownComponent(*kind));
            }
            let mut cursor = Cursor::new(bytes);
            let value = ComponentValue::read(&mut cursor, *kind)?;
            cursor.finish()?;
            patch.put(*kind, PatchEntry::Set(value));
        }
        for kind in &stack.removed {
            if !u32::try_from(*kind).is_ok_and(|id| id < Registry::DATA_COMPONENT_TYPE.size) {
                return Err(DecodeError::UnknownComponent(*kind));
            }
            patch.put(*kind, PatchEntry::Removed);
        }
        Ok(patch)
    }

    /// 变回 [`ItemStack`] 里那两个列表。
    pub fn to_opaque(&self) -> (Vec<(i32, Vec<u8>)>, Vec<i32>) {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        for (kind, entry) in &self.entries {
            match entry {
                PatchEntry::Set(value) => {
                    let mut bytes = Vec::new();
                    value.write(&mut bytes);
                    added.push((*kind, bytes));
                }
                PatchEntry::Removed => removed.push(*kind),
            }
        }
        (added, removed)
    }

    /// 按可信格式写入，也就是官方 `DataComponentPatch.STREAM_CODEC`。
    pub fn write(&self, out: &mut Vec<u8>) {
        let added = self
            .entries
            .iter()
            .filter(|(_, entry)| matches!(entry, PatchEntry::Set(_)));
        let removed = self
            .entries
            .iter()
            .filter(|(_, entry)| matches!(entry, PatchEntry::Removed));

        write_varint(out, added.clone().count() as i32);
        write_varint(out, removed.clone().count() as i32);
        for (kind, entry) in added {
            write_varint(out, *kind);
            if let PatchEntry::Set(value) = entry {
                value.write(out);
            }
        }
        for (kind, _) in removed {
            write_varint(out, *kind);
        }
    }

    /// 补丁里的条目，按线上的顺序。
    pub fn entries(&self) -> &[(i32, PatchEntry)] {
        &self.entries
    }

    /// 一个组件类型在这个补丁里的值。
    pub fn get(&self, kind: i32) -> Option<&ComponentValue> {
        self.entries
            .iter()
            .find(|(id, _)| *id == kind)
            .and_then(|(_, entry)| match entry {
                PatchEntry::Set(value) => Some(value),
                PatchEntry::Removed => None,
            })
    }

    /// 补丁里的 `max_stack_size`。
    pub fn max_stack_size(&self) -> Option<i32> {
        match self.get(kinds::MAX_STACK_SIZE) {
            Some(ComponentValue::MaxStackSize(size)) => Some(*size),
            _ => None,
        }
    }

    /// 官方用 `Map.put`：同一个组件类型出现两次时后面的赢，位置不变。
    fn put(&mut self, kind: i32, entry: PatchEntry) {
        match self.entries.iter_mut().find(|(id, _)| *id == kind) {
            Some(existing) => existing.1 = entry,
            None => self.entries.push((kind, entry)),
        }
    }
}

fn count(value: i32) -> Result<usize, DecodeError> {
    usize::try_from(value).map_err(|_| DecodeError::InvalidCount(value))
}

#[cfg(test)]
mod tests;
