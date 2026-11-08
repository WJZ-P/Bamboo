//! 组件里反复出现的那些小类型，每个都对应官方的一个 `StreamCodec`。

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::component_registries::consume_effect_type;
use mc_bamboo_nbt::Tag;
use uuid::Uuid;

use crate::{
    codec::{
        DecodeError, write_bool, write_f32, write_f64, write_i32, write_i64, write_string,
        write_uuid, write_varint,
    },
    components::{
        cursor::{
            Cursor, Holder, HolderSet, MAX_STRING, OutOfBounds, Registry, continuous_id, sparse_id,
            write_holder, write_holder_set, write_identifier, write_list, write_optional,
        },
        text,
    },
    text::Component,
};

/// 一个按 ID 排列的官方枚举：常量个数和超范围时的折返策略。
///
/// 这些编解码器不会失败，所以非法的 ID 会被折成一个合法值，再写出去就和读到的不一样。
#[derive(Debug, Clone, Copy)]
pub struct EnumDef {
    pub count: i32,
    pub strategy: OutOfBounds,
}

impl EnumDef {
    const fn new(count: i32, strategy: OutOfBounds) -> Self {
        Self { count, strategy }
    }

    pub fn read(self, cursor: &mut Cursor<'_>) -> Result<i32, DecodeError> {
        Ok(continuous_id(cursor.varint()?, self.count, self.strategy))
    }
}

/// 官方 `Rarity`：common、uncommon、rare、epic。
pub const RARITY: EnumDef = EnumDef::new(4, OutOfBounds::Zero);
/// 官方 `DyeColor`，white 到 black 十六种。
pub const DYE_COLOR: EnumDef = EnumDef::new(16, OutOfBounds::Zero);
/// 官方 `ItemUseAnimation`，none 到 spear 十二种。
pub const ITEM_USE_ANIMATION: EnumDef = EnumDef::new(12, OutOfBounds::Zero);
/// 官方 `SwingAnimationType`：none、whack、stab。
pub const SWING_ANIMATION_TYPE: EnumDef = EnumDef::new(3, OutOfBounds::Zero);
/// 官方 `EquipmentSlot`，mainhand 到 saddle 八种。
pub const EQUIPMENT_SLOT: EnumDef = EnumDef::new(8, OutOfBounds::Zero);
/// 官方 `EquipmentSlotGroup`，any 到 saddle 十一种。
pub const EQUIPMENT_SLOT_GROUP: EnumDef = EnumDef::new(11, OutOfBounds::Zero);
/// 官方 `AttributeModifier.Operation`：add_value、add_multiplied_base、add_multiplied_total。
pub const ATTRIBUTE_OPERATION: EnumDef = EnumDef::new(3, OutOfBounds::Zero);
/// 官方 `ItemAttributeModifiers.Display.Type`：default、hidden、override。
pub const ATTRIBUTE_DISPLAY_TYPE: EnumDef = EnumDef::new(3, OutOfBounds::Zero);
/// 官方 `FireworkExplosion.Shape` 五种。
pub const FIREWORK_SHAPE: EnumDef = EnumDef::new(5, OutOfBounds::Zero);
/// 官方 `MapPostProcessing`：lock、scale。
pub const MAP_POST_PROCESSING: EnumDef = EnumDef::new(2, OutOfBounds::Zero);
/// 官方 `Fox.Variant`：red、snow。
pub const FOX_VARIANT: EnumDef = EnumDef::new(2, OutOfBounds::Zero);
/// 官方 `Axolotl.Variant` 五种。
pub const AXOLOTL_VARIANT: EnumDef = EnumDef::new(5, OutOfBounds::Zero);
/// 官方 `Salmon.Variant`：small、medium、large，超范围时夹住。
pub const SALMON_SIZE: EnumDef = EnumDef::new(3, OutOfBounds::Clamp);
/// 官方 `Parrot.Variant` 五种，超范围时夹住。
pub const PARROT_VARIANT: EnumDef = EnumDef::new(5, OutOfBounds::Clamp);
/// 官方 `MushroomCow.Variant`：red、brown，超范围时夹住。
pub const MOOSHROOM_VARIANT: EnumDef = EnumDef::new(2, OutOfBounds::Clamp);
/// 官方 `Llama.Variant` 四种，超范围时夹住。
pub const LLAMA_VARIANT: EnumDef = EnumDef::new(4, OutOfBounds::Clamp);
/// 官方 equine 的 `Variant` 七种，超范围时取模。
pub const HORSE_VARIANT: EnumDef = EnumDef::new(7, OutOfBounds::Wrap);

/// 官方 `TropicalFish.Pattern` 的 ID：`base.id | index << 8`，稀疏表，默认 kob。
pub const TROPICAL_FISH_PATTERNS: [i32; 12] =
    [0, 256, 512, 768, 1024, 1280, 1, 257, 513, 769, 1025, 1281];
/// 官方 `Rabbit.Variant` 的 ID：0 到 5 加上 99（杀手兔）。
pub const RABBIT_VARIANTS: [i32; 7] = [0, 1, 2, 3, 4, 5, 99];

/// 读一个稀疏表里的 ID，不在表里时用 `default`。
pub fn read_sparse(cursor: &mut Cursor<'_>, ids: &[i32], default: i32) -> Result<i32, DecodeError> {
    Ok(sparse_id(cursor.varint()?, ids, default))
}

/// 官方 `SoundEvent`：可以是注册表里的音效，也可以内联一份定义。
#[derive(Debug, Clone, PartialEq)]
pub struct SoundEvent {
    pub location: String,
    /// `fixed_range`：有值时固定这个听得见的距离。
    pub fixed_range: Option<f32>,
}

impl SoundEvent {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Holder<Self>, DecodeError> {
        cursor.holder(Registry::SOUND_EVENT, |cursor| {
            Ok(Self {
                location: cursor.identifier()?,
                fixed_range: cursor.optional(Cursor::f32)?,
            })
        })
    }

    pub fn write(out: &mut Vec<u8>, holder: &Holder<Self>) {
        write_holder(out, holder, |out, sound| {
            write_identifier(out, &sound.location);
            write_optional(out, sound.fixed_range.as_ref(), |out, &range| {
                write_f32(out, range);
            });
        });
    }
}

/// 官方 `ConsumeEffect`，按 `minecraft:consume_effect_type` 的 ID 分派。
#[derive(Debug, Clone, PartialEq)]
pub enum ConsumeEffect {
    /// `apply_effects`。
    ApplyEffects {
        effects: Vec<MobEffectInstance>,
        probability: f32,
    },
    /// `remove_effects`。
    RemoveEffects { effects: HolderSet },
    /// `clear_all_effects`，没有内容。
    ClearAllEffects,
    /// `teleport_randomly`。
    TeleportRandomly {
        diameter: f32,
        directional_particles: bool,
    },
    /// `play_sound`。
    PlaySound { sound: Holder<SoundEvent> },
}

impl ConsumeEffect {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        let kind = cursor.registry_id(Registry::CONSUME_EFFECT_TYPE)?;
        match kind {
            consume_effect_type::APPLY_EFFECTS => Ok(Self::ApplyEffects {
                effects: cursor.list(usize::MAX, MobEffectInstance::read)?,
                probability: cursor.f32()?,
            }),
            consume_effect_type::REMOVE_EFFECTS => Ok(Self::RemoveEffects {
                effects: cursor.holder_set(Registry::MOB_EFFECT)?,
            }),
            consume_effect_type::CLEAR_ALL_EFFECTS => Ok(Self::ClearAllEffects),
            consume_effect_type::TELEPORT_RANDOMLY => Ok(Self::TeleportRandomly {
                diameter: cursor.f32()?,
                directional_particles: cursor.bool()?,
            }),
            consume_effect_type::PLAY_SOUND => Ok(Self::PlaySound {
                sound: SoundEvent::read(cursor)?,
            }),
            id => Err(DecodeError::UnknownRegistryId {
                registry: Registry::CONSUME_EFFECT_TYPE.name,
                id,
            }),
        }
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::ApplyEffects {
                effects,
                probability,
            } => {
                write_varint(out, consume_effect_type::APPLY_EFFECTS);
                write_list(out, effects, MobEffectInstance::write);
                write_f32(out, *probability);
            }
            Self::RemoveEffects { effects } => {
                write_varint(out, consume_effect_type::REMOVE_EFFECTS);
                write_holder_set(out, effects);
            }
            Self::ClearAllEffects => write_varint(out, consume_effect_type::CLEAR_ALL_EFFECTS),
            Self::TeleportRandomly {
                diameter,
                directional_particles,
            } => {
                write_varint(out, consume_effect_type::TELEPORT_RANDOMLY);
                write_f32(out, *diameter);
                write_bool(out, *directional_particles);
            }
            Self::PlaySound { sound } => {
                write_varint(out, consume_effect_type::PLAY_SOUND);
                SoundEvent::write(out, sound);
            }
        }
    }
}

/// 官方 `MobEffectInstance`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobEffectInstance {
    /// `minecraft:mob_effect` 的注册表 ID。
    pub effect: i32,
    pub details: EffectDetails,
}

impl MobEffectInstance {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            effect: cursor.registry_id(Registry::MOB_EFFECT)?,
            details: EffectDetails::read(cursor, 0)?,
        })
    }

    pub fn write(out: &mut Vec<u8>, value: &Self) {
        write_varint(out, value.effect);
        value.details.write(out);
    }
}

/// 官方 `MobEffectInstance.Details`，`hidden_effect` 会递归。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectDetails {
    pub amplifier: i32,
    pub duration: i32,
    pub ambient: bool,
    pub show_particles: bool,
    pub show_icon: bool,
    /// 被更强的同种效果盖住的那一份。
    pub hidden_effect: Option<Box<EffectDetails>>,
}

/// `hidden_effect` 的层数上限。官方不限，几千层会在 Java 里爆栈。
const MAX_HIDDEN_EFFECTS: u32 = 64;

impl EffectDetails {
    fn read(cursor: &mut Cursor<'_>, depth: u32) -> Result<Self, DecodeError> {
        if depth >= MAX_HIDDEN_EFFECTS {
            return Err(DecodeError::InvalidComponentValue(
                "too many hidden mob effects",
            ));
        }
        Ok(Self {
            amplifier: cursor.varint()?,
            duration: cursor.varint()?,
            ambient: cursor.bool()?,
            show_particles: cursor.bool()?,
            show_icon: cursor.bool()?,
            hidden_effect: cursor
                .optional(|cursor| Self::read(cursor, depth + 1))?
                .map(Box::new),
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        write_varint(out, self.amplifier);
        write_varint(out, self.duration);
        write_bool(out, self.ambient);
        write_bool(out, self.show_particles);
        write_bool(out, self.show_icon);
        write_optional(out, self.hidden_effect.as_deref(), |out, hidden| {
            hidden.write(out);
        });
    }
}

/// 官方 `Filterable`：原文加上过滤后的文本。
#[derive(Debug, Clone, PartialEq)]
pub struct Filterable<T> {
    pub raw: T,
    pub filtered: Option<T>,
}

impl<T> Filterable<T> {
    pub fn read(
        cursor: &mut Cursor<'_>,
        mut read: impl FnMut(&mut Cursor<'_>) -> Result<T, DecodeError>,
    ) -> Result<Self, DecodeError> {
        Ok(Self {
            raw: read(cursor)?,
            filtered: cursor.optional(|cursor| read(cursor))?,
        })
    }

    pub fn write(out: &mut Vec<u8>, value: &Self, mut write: impl FnMut(&mut Vec<u8>, &T)) {
        write(out, &value.raw);
        write_optional(out, value.filtered.as_ref(), |out, filtered| {
            write(out, filtered);
        });
    }
}

/// 官方 `ResolvableInt`：一个常量，或者 `minecraft:context_int_provider` 里的一个键。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvableInt {
    Constant(i32),
    Reference(String),
}

impl ResolvableInt {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        match cursor.either(Cursor::i32, Cursor::identifier)? {
            Ok(value) => Ok(Self::Constant(value)),
            Err(key) => Ok(Self::Reference(key)),
        }
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Constant(value) => {
                write_bool(out, true);
                write_i32(out, *value);
            }
            Self::Reference(key) => {
                write_bool(out, false);
                write_identifier(out, key);
            }
        }
    }
}

/// 官方 `ResolvableFloat`。
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvableFloat {
    Constant(f32),
    Reference(String),
}

impl ResolvableFloat {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        match cursor.either(Cursor::f32, Cursor::identifier)? {
            Ok(value) => Ok(Self::Constant(value)),
            Err(key) => Ok(Self::Reference(key)),
        }
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Constant(value) => {
                write_bool(out, true);
                write_f32(out, *value);
            }
            Self::Reference(key) => {
                write_bool(out, false);
                write_identifier(out, key);
            }
        }
    }
}

/// 官方 `GlobalPos`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalPos {
    /// 维度的注册表键，比如 `minecraft:overworld`。
    pub dimension: String,
    pub pos: BlockPos,
}

impl GlobalPos {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            dimension: cursor.identifier()?,
            pos: BlockPos::unpack(cursor.i64()?),
        })
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        write_identifier(out, &self.dimension);
        write_i64(out, self.pos.pack());
    }
}

/// 官方 `AttributeModifier`。
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeModifier {
    pub id: String,
    pub amount: f64,
    /// [`ATTRIBUTE_OPERATION`] 的 ID。
    pub operation: i32,
}

impl AttributeModifier {
    fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            id: cursor.identifier()?,
            amount: cursor.f64()?,
            operation: ATTRIBUTE_OPERATION.read(cursor)?,
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        write_identifier(out, &self.id);
        write_f64(out, self.amount);
        write_varint(out, self.operation);
    }
}

/// 官方 `ItemAttributeModifiers.Display`。
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeDisplay {
    /// 按属性修饰符本身显示。
    Default,
    /// 不显示。
    Hidden,
    /// 用这段文字代替。
    Override(Component),
}

impl AttributeDisplay {
    fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        match ATTRIBUTE_DISPLAY_TYPE.read(cursor)? {
            0 => Ok(Self::Default),
            1 => Ok(Self::Hidden),
            _ => Ok(Self::Override(text::read(cursor)?)),
        }
    }

    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Default => write_varint(out, 0),
            Self::Hidden => write_varint(out, 1),
            Self::Override(component) => {
                write_varint(out, 2);
                text::write(out, component);
            }
        }
    }
}

/// 官方 `ItemAttributeModifiers.Entry`。
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeEntry {
    /// `minecraft:attribute` 的注册表 ID。
    pub attribute: i32,
    pub modifier: AttributeModifier,
    /// [`EQUIPMENT_SLOT_GROUP`] 的 ID。
    pub slot: i32,
    pub display: AttributeDisplay,
}

impl AttributeEntry {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            attribute: cursor.registry_id(Registry::ATTRIBUTE)?,
            modifier: AttributeModifier::read(cursor)?,
            slot: EQUIPMENT_SLOT_GROUP.read(cursor)?,
            display: AttributeDisplay::read(cursor)?,
        })
    }

    pub fn write(out: &mut Vec<u8>, value: &Self) {
        write_varint(out, value.attribute);
        value.modifier.write(out);
        write_varint(out, value.slot);
        value.display.write(out);
    }
}

/// 官方 `SwingAnimation`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwingAnimation {
    /// [`SWING_ANIMATION_TYPE`] 的 ID。
    pub kind: i32,
    pub duration: i32,
}

impl SwingAnimation {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            kind: SWING_ANIMATION_TYPE.read(cursor)?,
            duration: cursor.varint()?,
        })
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        write_varint(out, self.kind);
        write_varint(out, self.duration);
    }
}

/// 官方 `FireworkExplosion`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireworkExplosion {
    /// [`FIREWORK_SHAPE`] 的 ID。
    pub shape: i32,
    pub colors: Vec<i32>,
    pub fade_colors: Vec<i32>,
    pub has_trail: bool,
    pub has_twinkle: bool,
}

impl FireworkExplosion {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            shape: FIREWORK_SHAPE.read(cursor)?,
            colors: cursor.list(usize::MAX, Cursor::i32)?,
            fade_colors: cursor.list(usize::MAX, Cursor::i32)?,
            has_trail: cursor.bool()?,
            has_twinkle: cursor.bool()?,
        })
    }

    pub fn write(out: &mut Vec<u8>, value: &Self) {
        write_varint(out, value.shape);
        write_list(out, &value.colors, |out, &color| write_i32(out, color));
        write_list(out, &value.fade_colors, |out, &color| write_i32(out, color));
        write_bool(out, value.has_trail);
        write_bool(out, value.has_twinkle);
    }
}

/// 官方 `Tool.Rule`。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRule {
    pub blocks: HolderSet,
    pub speed: Option<f32>,
    pub correct_for_drops: Option<bool>,
}

impl ToolRule {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            blocks: cursor.holder_set(Registry::BLOCK)?,
            speed: cursor.optional(Cursor::f32)?,
            correct_for_drops: cursor.optional(Cursor::bool)?,
        })
    }

    pub fn write(out: &mut Vec<u8>, value: &Self) {
        write_holder_set(out, &value.blocks);
        write_optional(out, value.speed.as_ref(), |out, &speed| {
            write_f32(out, speed);
        });
        write_optional(out, value.correct_for_drops.as_ref(), |out, &correct| {
            write_bool(out, correct);
        });
    }
}

/// 官方 `BlocksAttacks.DamageReduction`。
#[derive(Debug, Clone, PartialEq)]
pub struct DamageReduction {
    pub horizontal_blocking_angle: f32,
    /// 只对这些伤害类型生效，没有时对所有类型生效。
    pub damage_types: Option<HolderSet>,
    pub base: f32,
    pub factor: f32,
}

impl DamageReduction {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            horizontal_blocking_angle: cursor.f32()?,
            damage_types: cursor.optional(|cursor| cursor.holder_set(Registry::DAMAGE_TYPE))?,
            base: cursor.f32()?,
            factor: cursor.f32()?,
        })
    }

    pub fn write(out: &mut Vec<u8>, value: &Self) {
        write_f32(out, value.horizontal_blocking_angle);
        write_optional(out, value.damage_types.as_ref(), write_holder_set);
        write_f32(out, value.base);
        write_f32(out, value.factor);
    }
}

/// 官方 `BlocksAttacks.ItemDamageFunction`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemDamageFunction {
    pub threshold: f32,
    pub base: f32,
    pub factor: f32,
}

impl ItemDamageFunction {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            threshold: cursor.f32()?,
            base: cursor.f32()?,
            factor: cursor.f32()?,
        })
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        write_f32(out, self.threshold);
        write_f32(out, self.base);
        write_f32(out, self.factor);
    }
}

/// 官方 `KineticWeapon.Condition`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KineticCondition {
    pub max_duration_ticks: i32,
    pub min_speed: f32,
    pub min_relative_speed: f32,
}

impl KineticCondition {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            max_duration_ticks: cursor.varint()?,
            min_speed: cursor.f32()?,
            min_relative_speed: cursor.f32()?,
        })
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        write_varint(out, self.max_duration_ticks);
        write_f32(out, self.min_speed);
        write_f32(out, self.min_relative_speed);
    }
}

/// 官方 `TrimMaterial` 的内联定义。
#[derive(Debug, Clone, PartialEq)]
pub struct TrimMaterial {
    pub palette_id: String,
    pub description: Component,
}

impl TrimMaterial {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Holder<Self>, DecodeError> {
        cursor.holder(Registry::TRIM_MATERIAL, |cursor| {
            Ok(Self {
                palette_id: cursor.identifier()?,
                description: text::read(cursor)?,
            })
        })
    }

    pub fn write(out: &mut Vec<u8>, holder: &Holder<Self>) {
        write_holder(out, holder, |out, material| {
            write_identifier(out, &material.palette_id);
            text::write(out, &material.description);
        });
    }
}

/// 官方 `TrimPattern` 的内联定义。
#[derive(Debug, Clone, PartialEq)]
pub struct TrimPattern {
    pub asset_id: String,
    pub description: Component,
    pub decal: bool,
}

impl TrimPattern {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Holder<Self>, DecodeError> {
        cursor.holder(Registry::TRIM_PATTERN, |cursor| {
            Ok(Self {
                asset_id: cursor.identifier()?,
                description: text::read(cursor)?,
                decal: cursor.bool()?,
            })
        })
    }

    pub fn write(out: &mut Vec<u8>, holder: &Holder<Self>) {
        write_holder(out, holder, |out, pattern| {
            write_identifier(out, &pattern.asset_id);
            text::write(out, &pattern.description);
            write_bool(out, pattern.decal);
        });
    }
}

/// 官方 `BannerPattern` 的内联定义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BannerPattern {
    pub asset_id: String,
    pub translation_key: String,
}

impl BannerPattern {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Holder<Self>, DecodeError> {
        cursor.holder(Registry::BANNER_PATTERN, |cursor| {
            Ok(Self {
                asset_id: cursor.identifier()?,
                translation_key: cursor.string(MAX_STRING)?,
            })
        })
    }

    pub fn write(out: &mut Vec<u8>, holder: &Holder<Self>) {
        write_holder(out, holder, |out, pattern| {
            write_identifier(out, &pattern.asset_id);
            write_string(out, &pattern.translation_key);
        });
    }
}

/// 官方 `BannerPatternLayers.Layer`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BannerLayer {
    pub pattern: Holder<BannerPattern>,
    /// [`DYE_COLOR`] 的 ID。
    pub color: i32,
}

impl BannerLayer {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            pattern: BannerPattern::read(cursor)?,
            color: DYE_COLOR.read(cursor)?,
        })
    }

    pub fn write(out: &mut Vec<u8>, value: &Self) {
        BannerPattern::write(out, &value.pattern);
        write_varint(out, value.color);
    }
}

/// 官方 `Instrument` 的内联定义。
#[derive(Debug, Clone, PartialEq)]
pub struct Instrument {
    pub sound: Holder<SoundEvent>,
    pub use_duration: f32,
    pub range: f32,
    pub durability_damage: i32,
    pub description: Component,
}

impl Instrument {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Holder<Self>, DecodeError> {
        cursor.holder(Registry::INSTRUMENT, |cursor| {
            Ok(Self {
                sound: SoundEvent::read(cursor)?,
                use_duration: cursor.f32()?,
                range: cursor.f32()?,
                durability_damage: cursor.varint()?,
                description: text::read(cursor)?,
            })
        })
    }

    pub fn write(out: &mut Vec<u8>, holder: &Holder<Self>) {
        write_holder(out, holder, |out, instrument| {
            SoundEvent::write(out, &instrument.sound);
            write_f32(out, instrument.use_duration);
            write_f32(out, instrument.range);
            write_varint(out, instrument.durability_damage);
            text::write(out, &instrument.description);
        });
    }
}

/// 官方 `JukeboxSong` 的内联定义。
#[derive(Debug, Clone, PartialEq)]
pub struct JukeboxSong {
    pub sound: Holder<SoundEvent>,
    pub description: Component,
    pub length_in_seconds: f32,
    pub comparator_output: i32,
}

impl JukeboxSong {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Holder<Self>, DecodeError> {
        cursor.holder(Registry::JUKEBOX_SONG, |cursor| {
            Ok(Self {
                sound: SoundEvent::read(cursor)?,
                description: text::read(cursor)?,
                length_in_seconds: cursor.f32()?,
                comparator_output: cursor.varint()?,
            })
        })
    }

    pub fn write(out: &mut Vec<u8>, holder: &Holder<Self>) {
        write_holder(out, holder, |out, song| {
            SoundEvent::write(out, &song.sound);
            text::write(out, &song.description);
            write_f32(out, song.length_in_seconds);
            write_varint(out, song.comparator_output);
        });
    }
}

/// 官方 `PaintingVariant` 的内联定义。标题和作者用的是可信编解码器，上限是整个堆。
#[derive(Debug, Clone, PartialEq)]
pub struct PaintingVariant {
    pub width: i32,
    pub height: i32,
    pub asset_id: String,
    pub title: Option<Component>,
    pub author: Option<Component>,
}

impl PaintingVariant {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Holder<Self>, DecodeError> {
        cursor.holder(Registry::PAINTING_VARIANT, |cursor| {
            Ok(Self {
                width: cursor.varint()?,
                height: cursor.varint()?,
                asset_id: cursor.identifier()?,
                title: text::read_optional(cursor)?,
                author: text::read_optional(cursor)?,
            })
        })
    }

    pub fn write(out: &mut Vec<u8>, holder: &Holder<Self>) {
        write_holder(out, holder, |out, variant| {
            write_varint(out, variant.width);
            write_varint(out, variant.height);
            write_identifier(out, &variant.asset_id);
            write_optional(out, variant.title.as_ref(), text::write);
            write_optional(out, variant.author.as_ref(), text::write);
        });
    }
}

/// 官方 `TypedEntityData`：一个类型 ID 加一段 NBT，NBT 里不含 `id`。
#[derive(Debug, Clone, PartialEq)]
pub struct TypedEntityData {
    /// 注册表 ID，实体类型或者方块实体类型。
    pub kind: i32,
    pub tag: Vec<(String, Tag)>,
}

impl TypedEntityData {
    pub fn read(cursor: &mut Cursor<'_>, registry: Registry) -> Result<Self, DecodeError> {
        Ok(Self {
            kind: cursor.registry_id(registry)?,
            tag: cursor.nbt_compound()?,
        })
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        write_varint(out, self.kind);
        Tag::Compound(self.tag.clone()).write_network(out);
    }
}

/// 官方 `BeehiveBlockEntity.Occupant`。
#[derive(Debug, Clone, PartialEq)]
pub struct BeeOccupant {
    pub entity_data: TypedEntityData,
    pub ticks_in_hive: i32,
    pub min_ticks_in_hive: i32,
}

impl BeeOccupant {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            entity_data: TypedEntityData::read(cursor, Registry::ENTITY_TYPE)?,
            ticks_in_hive: cursor.varint()?,
            min_ticks_in_hive: cursor.varint()?,
        })
    }

    pub fn write(out: &mut Vec<u8>, value: &Self) {
        value.entity_data.write(out);
        write_varint(out, value.ticks_in_hive);
        write_varint(out, value.min_ticks_in_hive);
    }
}

/// 官方 `SuspiciousStewEffects.Entry`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StewEffect {
    /// `minecraft:mob_effect` 的注册表 ID。
    pub effect: i32,
    pub duration: i32,
}

impl StewEffect {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            effect: cursor.registry_id(Registry::MOB_EFFECT)?,
            duration: cursor.varint()?,
        })
    }

    pub fn write(out: &mut Vec<u8>, value: &Self) {
        write_varint(out, value.effect);
        write_varint(out, value.duration);
    }
}

/// authlib 的 `Property`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

/// 官方 `ByteBufCodecs.GAME_PROFILE_PROPERTIES` 的上限。
const MAX_PROPERTIES: usize = 16;

fn read_properties(cursor: &mut Cursor<'_>) -> Result<Vec<Property>, DecodeError> {
    cursor.list(MAX_PROPERTIES, |cursor| {
        Ok(Property {
            name: cursor.string(64)?,
            value: cursor.string(MAX_STRING)?,
            signature: cursor.optional(|cursor| cursor.string(1024))?,
        })
    })
}

fn write_properties(out: &mut Vec<u8>, properties: &[Property]) {
    write_list(out, properties, |out, property| {
        write_string(out, &property.name);
        write_string(out, &property.value);
        write_optional(out, property.signature.as_ref(), |out, signature| {
            write_string(out, signature);
        });
    });
}

/// authlib 的 `GameProfile`，已经解析好的档案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameProfile {
    pub id: Uuid,
    pub name: String,
    pub properties: Vec<Property>,
}

/// 官方 `ResolvableProfile.Partial`，还没解析的档案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialProfile {
    pub name: Option<String>,
    pub id: Option<Uuid>,
    pub properties: Vec<Property>,
}

/// 官方 `PlayerSkin.Patch`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinPatch {
    pub body: Option<String>,
    pub cape: Option<String>,
    pub elytra: Option<String>,
    /// `PlayerModelType`：`true` 是 slim，`false` 是 wide。
    pub slim: Option<bool>,
}

impl SkinPatch {
    fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            body: cursor.optional(Cursor::identifier)?,
            cape: cursor.optional(Cursor::identifier)?,
            elytra: cursor.optional(Cursor::identifier)?,
            slim: cursor.optional(Cursor::bool)?,
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        for asset in [&self.body, &self.cape, &self.elytra] {
            write_optional(out, asset.as_ref(), |out, id| write_identifier(out, id));
        }
        write_optional(out, self.slim.as_ref(), |out, &slim| write_bool(out, slim));
    }
}

/// 官方 `ResolvableProfile`：一份完整档案或者只有名字、UUID 的半份档案，外加皮肤补丁。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvableProfile {
    pub profile: Result<GameProfile, PartialProfile>,
    pub skin_patch: SkinPatch,
}

impl ResolvableProfile {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        let profile = cursor.either(
            |cursor| {
                Ok(GameProfile {
                    id: cursor.uuid()?,
                    name: cursor.string(16)?,
                    properties: read_properties(cursor)?,
                })
            },
            |cursor| {
                Ok(PartialProfile {
                    name: cursor.optional(|cursor| cursor.string(16))?,
                    id: cursor.optional(Cursor::uuid)?,
                    properties: read_properties(cursor)?,
                })
            },
        )?;
        Ok(Self {
            profile,
            skin_patch: SkinPatch::read(cursor)?,
        })
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        match &self.profile {
            Ok(profile) => {
                write_bool(out, true);
                write_uuid(out, profile.id);
                write_string(out, &profile.name);
                write_properties(out, &profile.properties);
            }
            Err(partial) => {
                write_bool(out, false);
                write_optional(out, partial.name.as_ref(), |out, name| {
                    write_string(out, name);
                });
                write_optional(out, partial.id.as_ref(), |out, &id| write_uuid(out, id));
                write_properties(out, &partial.properties);
            }
        }
        self.skin_patch.write(out);
    }
}

/// 官方 `SignText`。
#[derive(Debug, Clone, PartialEq)]
pub struct SignText {
    /// 固定四行。
    pub messages: Vec<Component>,
    /// 过滤后的四行，和原文一样时官方不写。
    pub filtered_messages: Option<Vec<Component>>,
    /// [`DYE_COLOR`] 的 ID。
    pub color: i32,
    pub has_glowing_text: bool,
}

/// 官方 `SignText.LINES`。
pub const SIGN_LINES: usize = 4;

impl SignText {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            messages: cursor.fixed_list(SIGN_LINES, text::read)?,
            filtered_messages: cursor
                .optional(|cursor| cursor.fixed_list(SIGN_LINES, text::read))?,
            color: DYE_COLOR.read(cursor)?,
            has_glowing_text: cursor.bool()?,
        })
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        for message in &self.messages {
            text::write(out, message);
        }
        write_optional(out, self.filtered_messages.as_ref(), |out, messages| {
            for message in messages {
                text::write(out, message);
            }
        });
        write_varint(out, self.color);
        write_bool(out, self.has_glowing_text);
    }
}

/// 官方 `CustomModelData`。
#[derive(Debug, Clone, PartialEq)]
pub struct CustomModelData {
    pub floats: Vec<f32>,
    pub flags: Vec<bool>,
    pub strings: Vec<String>,
    pub colors: Vec<i32>,
}

impl CustomModelData {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            floats: cursor.list(usize::MAX, Cursor::f32)?,
            flags: cursor.list(usize::MAX, Cursor::bool)?,
            strings: cursor.list(usize::MAX, |cursor| cursor.string(MAX_STRING))?,
            colors: cursor.list(usize::MAX, Cursor::i32)?,
        })
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        write_list(out, &self.floats, |out, &value| write_f32(out, value));
        write_list(out, &self.flags, |out, &value| write_bool(out, value));
        write_list(out, &self.strings, |out, value| write_string(out, value));
        write_list(out, &self.colors, |out, &value| write_i32(out, value));
    }
}

/// 官方 `Equippable`。
#[derive(Debug, Clone, PartialEq)]
pub struct Equippable {
    /// [`EQUIPMENT_SLOT`] 的 ID。
    pub slot: i32,
    pub equip_sound: Holder<SoundEvent>,
    /// `minecraft:equipment_asset` 的注册表键。
    pub asset_id: Option<String>,
    pub camera_overlay: Option<String>,
    pub allowed_entities: Option<HolderSet>,
    pub dispensable: bool,
    pub swappable: bool,
    pub damage_on_hurt: bool,
    pub equip_on_interact: bool,
    pub can_be_sheared: bool,
    pub shearing_sound: Holder<SoundEvent>,
}

impl Equippable {
    pub fn read(cursor: &mut Cursor<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            slot: EQUIPMENT_SLOT.read(cursor)?,
            equip_sound: SoundEvent::read(cursor)?,
            asset_id: cursor.optional(Cursor::identifier)?,
            camera_overlay: cursor.optional(Cursor::identifier)?,
            allowed_entities: cursor.optional(|cursor| cursor.holder_set(Registry::ENTITY_TYPE))?,
            dispensable: cursor.bool()?,
            swappable: cursor.bool()?,
            damage_on_hurt: cursor.bool()?,
            equip_on_interact: cursor.bool()?,
            can_be_sheared: cursor.bool()?,
            shearing_sound: SoundEvent::read(cursor)?,
        })
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        write_varint(out, self.slot);
        SoundEvent::write(out, &self.equip_sound);
        write_optional(out, self.asset_id.as_ref(), |out, id| {
            write_identifier(out, id);
        });
        write_optional(out, self.camera_overlay.as_ref(), |out, id| {
            write_identifier(out, id);
        });
        write_optional(out, self.allowed_entities.as_ref(), write_holder_set);
        for flag in [
            self.dispensable,
            self.swappable,
            self.damage_on_hurt,
            self.equip_on_interact,
            self.can_be_sheared,
        ] {
            write_bool(out, flag);
        }
        SoundEvent::write(out, &self.shearing_sound);
    }
}
