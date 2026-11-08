//! 哪些组件能发给物品主人以外的玩家。
//!
//! 客户端解组件用的是官方的 `StreamCodec`。大多数组件的 `StreamCodec` 是纯结构的：
//! 我们解得开，写回去的字节客户端也一定解得开。但有两类不是——
//!
//! - 五种没有 `networkSynchronized` 的组件（`lock`、`container_loot`、`map_decorations`、
//!   `debug_stick_state`、`recipes`）线上是 NBT，客户端要把它过一遍 DFU 的 `Codec`；
//! - 文本组件线上也是 NBT，官方会按 `ComponentSerialization.CODEC` 解析，里面有几处
//!   我们只看了字段类型，见 [`text::is_fully_checked`]。
//!
//! 这两类的内容我们没法保证客户端解得开，而解不开的客户端会断线。所以发给别人的物品里
//! 不带它们；发回给物品主人的那一份原样带着，因为字节本来就是它的客户端发来的。
//! 被滤掉的组件照旧留在服务端的物品上。
//!
//! `custom_data` 和 `intangible_projectile` 也没有 `networkSynchronized`，但官方的 `Codec`
//! 分别接受任何复合标签和任何复合标签（写回去永远是空的），我们的编解码和它们一致，所以能转发。

use mc_bamboo_data::registries::data_component_type as kinds;

use crate::components::{
    ComponentValue,
    cursor::{Cursor, Holder},
    predicate::BlockPredicate,
    stack::ItemStackTemplate,
    text,
    value::{
        AttributeDisplay, Filterable, Instrument, JukeboxSong, PaintingVariant, SignText,
        TrimMaterial, TrimPattern,
    },
};

/// 线上是 NBT、官方还要过一遍 `Codec` 的那五种组件。
const UNVERIFIED: [i32; 5] = [
    kinds::LOCK,
    kinds::CONTAINER_LOOT,
    kinds::MAP_DECORATIONS,
    kinds::DEBUG_STICK_STATE,
    kinds::RECIPES,
];

/// 这一项组件能不能发给别人。`bytes` 是它在线上的值。
///
/// 解不开的值一律不转发：合法的物品不会走到这里，调用方已经校验过了。
pub fn forwardable(kind: i32, bytes: &[u8]) -> bool {
    if UNVERIFIED.contains(&kind) {
        return false;
    }
    let mut cursor = Cursor::new(bytes);
    match ComponentValue::read(&mut cursor, kind) {
        Ok(value) => cursor.finish().is_ok() && value_is_forwardable(&value),
        Err(_) => false,
    }
}

/// 这一项组件能不能发给别人，值已经是类型化的。
///
/// 和 [`forwardable`] 同一套判定，给已经解析好的补丁用，见
/// [`ComponentPatch::filtered`](crate::components::ComponentPatch::filtered)。
pub fn forwardable_value(kind: i32, value: &ComponentValue) -> bool {
    !UNVERIFIED.contains(&kind) && value_is_forwardable(value)
}

/// 这个值里的文本组件和嵌套物品堆都是我们完全校验过的。
fn value_is_forwardable(value: &ComponentValue) -> bool {
    use ComponentValue as V;

    match value {
        // 文本组件。
        V::CustomName(text) | V::ItemName(text) => text::is_fully_checked(text),
        V::Lore(lines) => lines.iter().all(text::is_fully_checked),
        V::AttributeModifiers(entries) => entries.iter().all(|entry| match &entry.display {
            AttributeDisplay::Override(text) => text::is_fully_checked(text),
            AttributeDisplay::Default | AttributeDisplay::Hidden => true,
        }),
        V::WrittenBookContent { pages, .. } => pages.iter().all(filterable_text),
        V::SignTextFront(text) | V::SignTextBack(text) => sign_text(text),
        // 内联定义里带说明文字的注册表条目。
        V::Trim { material, pattern } => {
            inline(material, |material: &TrimMaterial| {
                text::is_fully_checked(&material.description)
            }) && inline(pattern, |pattern: &TrimPattern| {
                text::is_fully_checked(&pattern.description)
            })
        }
        V::ProvidesTrimMaterial(material) => inline(material, |material: &TrimMaterial| {
            text::is_fully_checked(&material.description)
        }),
        V::Instrument(instrument) => inline(instrument, |instrument: &Instrument| {
            text::is_fully_checked(&instrument.description)
        }),
        V::JukeboxPlayable(song) => inline(song, |song: &JukeboxSong| {
            text::is_fully_checked(&song.description)
        }),
        V::PaintingVariant(variant) => inline(variant, |variant: &PaintingVariant| {
            variant
                .title
                .iter()
                .chain(&variant.author)
                .all(text::is_fully_checked)
        }),
        // 嵌套的物品堆，它自己又带一份组件补丁。
        V::UseRemainder(stack) | V::SulfurCubeContent(stack) => nested(stack),
        V::ChargedProjectiles(stacks) | V::BundleContents(stacks) => stacks.iter().all(nested),
        V::PotDecorations {
            back,
            left,
            right,
            front,
        } => [back, left, right, front].into_iter().flatten().all(nested),
        V::Container(slots) => slots.iter().flatten().all(nested),
        // 方块谓词里有一整张组件表，还有一组内容是 NBT 的谓词。
        V::CanPlaceOn(predicates) | V::CanBreak(predicates) => {
            predicates.iter().all(block_predicate)
        }
        // 其余组件的值只有数字、字符串、注册表 ID、`HolderSet` 和原样转发的 NBT。
        V::CustomData(..)
        | V::MaxStackSize(..)
        | V::MaxDamage(..)
        | V::Damage(..)
        | V::Unbreakable
        | V::UseEffects { .. }
        | V::MinimumAttackCharge(..)
        | V::DamageType(..)
        | V::ItemModel(..)
        | V::Rarity(..)
        | V::Enchantments(..)
        | V::CustomModelData(..)
        | V::TooltipDisplay { .. }
        | V::RepairCost(..)
        | V::CreativeSlotLock
        | V::EnchantmentGlintOverride(..)
        | V::IntangibleProjectile
        | V::Food { .. }
        | V::Consumable { .. }
        | V::UseCooldown { .. }
        | V::DamageResistant(..)
        | V::Tool { .. }
        | V::Weapon { .. }
        | V::AttackRange { .. }
        | V::Enchantable(..)
        | V::Equippable(..)
        | V::Repairable(..)
        | V::Glider
        | V::TooltipStyle(..)
        | V::DeathProtection(..)
        | V::BlocksAttacks { .. }
        | V::PiercingWeapon { .. }
        | V::KineticWeapon { .. }
        | V::AttackAnimation(..)
        | V::InteractAnimation(..)
        | V::AdditionalTradeCost(..)
        | V::BlockTransformer(..)
        | V::VillagerFood(..)
        | V::StoredEnchantments(..)
        | V::Dye(..)
        | V::DyedColor(..)
        | V::MapId(..)
        | V::MapPostProcessing(..)
        | V::PotionContents { .. }
        | V::PotionDurationScale(..)
        | V::SuspiciousStewEffects(..)
        | V::WritableBookContent(..)
        | V::EntityData(..)
        | V::BucketEntityData(..)
        | V::BlockEntityData(..)
        | V::OminousBottleAmplifier(..)
        | V::ProvidesBannerPatterns(..)
        | V::LodestoneTracker { .. }
        | V::FireworkExplosion(..)
        | V::Fireworks { .. }
        | V::Profile(..)
        | V::NoteBlockSound(..)
        | V::BannerPatterns(..)
        | V::BaseColor(..)
        | V::BlockState(..)
        | V::Bees(..)
        | V::BreakSound(..)
        | V::Compostable(..)
        | V::CookingFuel { .. }
        | V::BrewingFuel { .. }
        | V::MobVisibility { .. }
        | V::VillagerVariant(..)
        | V::WolfVariant(..)
        | V::WolfSoundVariant(..)
        | V::WolfCollar(..)
        | V::FoxVariant(..)
        | V::SalmonSize(..)
        | V::ParrotVariant(..)
        | V::TropicalFishPattern(..)
        | V::TropicalFishBaseColor(..)
        | V::TropicalFishPatternColor(..)
        | V::MooshroomVariant(..)
        | V::RabbitVariant(..)
        | V::PigVariant(..)
        | V::PigSoundVariant(..)
        | V::CowVariant(..)
        | V::CowSoundVariant(..)
        | V::ChickenVariant(..)
        | V::ChickenSoundVariant(..)
        | V::ZombieNautilusVariant(..)
        | V::FrogVariant(..)
        | V::HorseVariant(..)
        | V::LlamaVariant(..)
        | V::AxolotlVariant(..)
        | V::CatVariant(..)
        | V::CatSoundVariant(..)
        | V::CatCollar(..)
        | V::SheepColor(..)
        | V::ShulkerColor(..)
        | V::ProvidesPotteryPattern(..)
        | V::Waxed
        | V::CushionColor(..) => true,
        // 这五种在 `forwardable` 里就挡掉了，解不开的值也到不了这里。
        V::MapDecorations(..)
        | V::DebugStickState(..)
        | V::Recipes(..)
        | V::Lock(..)
        | V::ContainerLoot(..) => false,
    }
}

/// 内联的注册表条目才看内容，引用只有一个 ID。
fn inline<T>(holder: &Holder<T>, check: impl FnOnce(&T) -> bool) -> bool {
    match holder {
        Holder::Id(_) => true,
        Holder::Inline(value) => check(value),
    }
}

fn filterable_text(text: &Filterable<crate::text::Component>) -> bool {
    text::is_fully_checked(&text.raw) && text.filtered.as_ref().is_none_or(text::is_fully_checked)
}

fn sign_text(text: &SignText) -> bool {
    text.messages.iter().all(text::is_fully_checked)
        && text
            .filtered_messages
            .iter()
            .flatten()
            .all(text::is_fully_checked)
}

/// 嵌套物品堆自己带的那份补丁，每一项都要能转发。
fn nested(stack: &ItemStackTemplate) -> bool {
    stack.components.entries().iter().all(|(kind, entry)| {
        match entry {
            crate::components::PatchEntry::Set(value) => {
                !UNVERIFIED.contains(kind) && value_is_forwardable(value)
            }
            // 移除项只有一个组件类型。
            crate::components::PatchEntry::Removed => true,
        }
    })
}

/// 官方 `BlockPredicate`：`components` 里是完整的组件值，`partial` 里的谓词内容是 NBT。
fn block_predicate(predicate: &BlockPredicate) -> bool {
    predicate.components.exact.iter().all(|component| {
        !UNVERIFIED.contains(&component.kind) && value_is_forwardable(&component.value)
    }) && predicate.components.partial.is_empty()
}
