//! 官方 `Bucketable`：鱼桶上的组件、从水里捞起和放回世界。
//!
//! 桶里的生物数据有两个来源：`bucket_entity_data` 组件存 `saveDefaultDataToBucketTag`
//! 写下的那几个键，物种自己的花纹走各自的组件（鲑鱼 `salmon/size`、热带鱼三个、
//! 美西螈 `axolotl/variant`）。放回时先套组件（官方 `EntityType.create` 的
//! `applyImplicitComponents`），再 `loadFromBucketTag`，最后置 `FromBucket`。
//!
//! Vanilla `Bucketable`: the components on a fish bucket, picking a mob up and putting it
//! back. `bucket_entity_data` holds the keys `saveDefaultDataToBucketTag` writes; the
//! variant lives in the species component. On placement the components apply first
//! (`applyImplicitComponents`), then `loadFromBucketTag`, then `FromBucket` is set.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{items, registries::data_component_type as kinds};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry, text::component_from_checked_tag},
    item::ItemStack,
};

use super::{Mob, MobType};
use crate::{entity::Entity, world::World};

/// `SoundEvents.BUCKET_FILL_FISH`：所有可装桶的鱼共用。
///
/// `SoundEvents.BUCKET_FILL_FISH`, shared by every bucketable fish.
pub(crate) const BUCKET_FILL_FISH: &str = "minecraft:item.bucket.fill_fish";
/// `SoundEvents.BUCKET_FILL_AXOLOTL`。
///
/// `SoundEvents.BUCKET_FILL_AXOLOTL`.
pub(crate) const BUCKET_FILL_AXOLOTL: &str = "minecraft:item.bucket.fill_axolotl";

/// `getBucketItemStack()`。不是可装桶的物种时是 `None`。
///
/// `getBucketItemStack()`; `None` for a species that cannot be bucketed.
pub const fn bucket_item(kind: MobType) -> Option<i32> {
    match kind {
        MobType::Cod => Some(items::COD_BUCKET),
        MobType::Salmon => Some(items::SALMON_BUCKET),
        MobType::TropicalFish => Some(items::TROPICAL_FISH_BUCKET),
        MobType::Pufferfish => Some(items::PUFFERFISH_BUCKET),
        MobType::Axolotl => Some(items::AXOLOTL_BUCKET),
        _ => None,
    }
}

/// `saveDefaultDataToBucketTag`：只在非默认时写前面五个布尔，`Health` 总是写。
///
/// `saveDefaultDataToBucketTag`: the five booleans only when non-default, `Health` always.
pub fn save_default_data(entity: &Entity, mob: &Mob) -> Vec<(String, Tag)> {
    let _ = entity;
    let mut out = Vec::new();
    if mob.no_ai {
        out.push(("NoAI".to_owned(), Tag::Byte(1)));
    }
    if mob.silent {
        out.push(("Silent".to_owned(), Tag::Byte(1)));
    }
    if mob.no_gravity {
        out.push(("NoGravity".to_owned(), Tag::Byte(1)));
    }
    if mob.glowing {
        out.push(("Glowing".to_owned(), Tag::Byte(1)));
    }
    if mob.invulnerable {
        out.push(("Invulnerable".to_owned(), Tag::Byte(1)));
    }
    if mob.persistence_required {
        out.push(("PersistenceRequired".to_owned(), Tag::Byte(1)));
    }
    out.push(("Health".to_owned(), Tag::Float(mob.health)));
    // `Axolotl.saveToBucketTag`：把花色、年龄和锁定标志写进 `bucket_entity_data`。
    // `Axolotl.saveToBucketTag`: variant, age and age-lock into `bucket_entity_data`.
    if mob.mob_type == MobType::Axolotl {
        out.push(("Variant".to_owned(), Tag::Int(mob.axolotl.variant)));
        out.push(("Age".to_owned(), Tag::Int(mob.age)));
        out.push(("AgeLocked".to_owned(), Tag::Byte(i8::from(mob.age_locked))));
    }
    out
}

/// `loadDefaultDataFromBucketTag`：只改写了的键，缺的键保持原样。
///
/// `loadDefaultDataFromBucketTag`: only the keys present change; absent keys keep their
/// current value.
pub fn load_default_data(entity: &mut Entity, mob: &mut Mob, tag: &Tag) {
    let Tag::Compound(entries) = tag else {
        return;
    };
    for (key, value) in entries {
        match key.as_str() {
            "NoAI" => mob.no_ai = tag_bool(value, mob.no_ai),
            "Silent" => mob.silent = tag_bool(value, mob.silent),
            "NoGravity" => mob.no_gravity = tag_bool(value, mob.no_gravity),
            "Glowing" => mob.glowing = tag_bool(value, mob.glowing),
            "Invulnerable" => mob.invulnerable = tag_bool(value, mob.invulnerable),
            "PersistenceRequired" => {
                mob.persistence_required = tag_bool(value, mob.persistence_required);
            }
            // `tag.getFloat("Health").ifPresent(entity::setHealth)`：夹到当前最大血量。
            "Health" => {
                if let Tag::Float(health) = value {
                    let max = mob
                        .attributes
                        .value(mc_bamboo_data::registries::attribute::MAX_HEALTH)
                        as f32;
                    mob.health = health.clamp(0.0, max);
                }
            }
            // `Axolotl.loadFromBucketTag`：花色、年龄和锁定标志。
            // `Axolotl.loadFromBucketTag`: variant, age and age-lock.
            "Variant" if mob.mob_type == MobType::Axolotl => {
                if let Tag::Int(variant) = value {
                    mob.axolotl.variant = *variant;
                }
            }
            "Age" if mob.mob_type == MobType::Axolotl => {
                if let Tag::Int(age) = value {
                    crate::entity::mob::living::set_age(entity, mob, *age);
                }
            }
            "AgeLocked" if mob.mob_type == MobType::Axolotl => {
                mob.age_locked = tag_bool(value, mob.age_locked);
            }
            _ => {}
        }
    }
}

const fn tag_bool(value: &Tag, fallback: bool) -> bool {
    match value {
        Tag::Byte(0) => false,
        Tag::Byte(_) => true,
        _ => fallback,
    }
}

/// `saveToBucketTag(bucket)`：物种那一层先加自己的组件，再写默认数据。
///
/// `saveToBucketTag(bucket)`: the species layer adds its components, then the defaults.
pub fn bucket_stack(entity: &Entity, mob: &Mob) -> Option<ItemStack> {
    let item = bucket_item(mob.mob_type)?;
    let mut patch = ComponentPatch::new();
    // `bucket.copyFrom(CUSTOM_NAME, entity)`：有自定义名才写。
    // `bucket.copyFrom(CUSTOM_NAME, entity)` only when the mob has a custom name.
    if let Some(name) = &mob.custom_name {
        patch.put(
            kinds::CUSTOM_NAME,
            PatchEntry::Set(ComponentValue::CustomName(component_from_checked_tag(
                name.clone(),
            ))),
        );
    }
    push_species_components(&mut patch, mob);
    patch.put(
        kinds::BUCKET_ENTITY_DATA,
        PatchEntry::Set(ComponentValue::BucketEntityData(save_default_data(
            entity, mob,
        ))),
    );
    let (added, removed) = patch.to_opaque();
    Some(ItemStack {
        item,
        count: 1,
        added,
        removed,
    })
}

/// 物种自己的 `saveToBucketTag` 追加的组件。
///
/// The components a species appends in its own `saveToBucketTag`.
fn push_species_components(patch: &mut ComponentPatch, mob: &Mob) {
    match mob.mob_type {
        MobType::Salmon => {
            // `bucket.copyFrom(DataComponents.SALMON_SIZE, this)`。
            patch.put(
                kinds::SALMON_SIZE,
                PatchEntry::Set(ComponentValue::SalmonSize(mob.fish.variant)),
            );
        }
        MobType::TropicalFish => {
            let variant = mob.fish.variant;
            let pattern = super::fish::pattern_packed_id(super::fish::pattern_from_packed(variant));
            patch.put(
                kinds::TROPICAL_FISH_PATTERN,
                PatchEntry::Set(ComponentValue::TropicalFishPattern(pattern)),
            );
            patch.put(
                kinds::TROPICAL_FISH_BASE_COLOR,
                PatchEntry::Set(ComponentValue::TropicalFishBaseColor(
                    (variant >> 16) & 0xFF,
                )),
            );
            patch.put(
                kinds::TROPICAL_FISH_PATTERN_COLOR,
                PatchEntry::Set(ComponentValue::TropicalFishPatternColor(
                    (variant >> 24) & 0xFF,
                )),
            );
        }
        _ => {}
    }
}

/// `EntityType.create` 的 `applyImplicitComponents` 加 `loadFromBucketTag`。
///
/// `applyImplicitComponents` of `EntityType.create`, then `loadFromBucketTag`.
pub fn apply_bucket_stack(entity: &mut Entity, mob: &mut Mob, stack: &ItemStack) {
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return;
    };
    if let Some(ComponentValue::CustomName(name)) = patch.get(kinds::CUSTOM_NAME) {
        mob.custom_name = Some(name.tag().clone());
    }
    apply_species_components(mob, &patch);
    if let Some(ComponentValue::BucketEntityData(entries)) = patch.get(kinds::BUCKET_ENTITY_DATA) {
        let tag = Tag::Compound(entries.clone());
        load_default_data(entity, mob, &tag);
    }
}

/// 物种组件套到实体上（官方 `applyImplicitComponent` 的那几条分支）。
///
/// The species components applied to the entity (`applyImplicitComponent` branches).
fn apply_species_components(mob: &mut Mob, patch: &ComponentPatch) {
    match mob.mob_type {
        MobType::Salmon => {
            if let Some(ComponentValue::SalmonSize(size)) = patch.get(kinds::SALMON_SIZE) {
                // `ByIdMap.continuous(..., CLAMP)`。
                mob.fish.variant = (*size).clamp(0, 2);
            }
        }
        MobType::TropicalFish => {
            // 官方三个 `setPattern` / `setBaseColor` / `setPatternColor` 都改实体数据，
            // 这里拼回打包值。
            let mut variant = mob.fish.variant;
            if let Some(ComponentValue::TropicalFishPattern(pattern)) =
                patch.get(kinds::TROPICAL_FISH_PATTERN)
            {
                variant = (variant & !65535) | (pattern & 65535);
            }
            if let Some(ComponentValue::TropicalFishBaseColor(color)) =
                patch.get(kinds::TROPICAL_FISH_BASE_COLOR)
            {
                variant = (variant & !(0xFF << 16)) | (color & 0xFF) << 16;
            }
            if let Some(ComponentValue::TropicalFishPatternColor(color)) =
                patch.get(kinds::TROPICAL_FISH_PATTERN_COLOR)
            {
                variant = (variant & !(0xFF << 24)) | (color & 0xFF) << 24;
            }
            mob.fish.variant = variant;
        }
        _ => {}
    }
}

/// `Bucketable.bucketMobPickup` 的前半：放 `getPickupSound()` 再交回桶。
///
/// 官方 `pickupEntity.playSound(sound, 1.0F, 1.0F)` 的音高固定 1，也不查 `isSilent()`：
/// `ServerLevel.playSound` 只看被跳过的玩家。
///
/// The first half of `Bucketable.bucketMobPickup`: play `getPickupSound()` and hand back
/// the bucket. Vanilla's `playSound(sound, 1.0F, 1.0F)` uses a fixed pitch and does not
/// consult `isSilent()`: `ServerLevel.playSound` only looks at the skipped player.
pub fn pickup(entity: &Entity, mob: &mut Mob) -> Option<ItemStack> {
    let stack = bucket_stack(entity, mob)?;
    let sound = if mob.mob_type == MobType::Axolotl {
        BUCKET_FILL_AXOLOTL
    } else {
        BUCKET_FILL_FISH
    };
    mob.events.push(super::MobEvent::Sound {
        sound,
        volume: 1.0,
        pitch: 1.0,
        position: entity.position,
        hostile: false,
        seed: None,
    });
    Some(stack)
}

/// `MobBucketItem.checkExtraContent`：只在服务端生成。
///
/// `MobBucketItem.checkExtraContent`: spawn on the server only.
pub fn check_extra_content(world: &mut World, item: i32, stack: Option<&ItemStack>, pos: BlockPos) {
    let Some(kind) = kind_of_bucket(item) else {
        // 还没建模的生物桶（蝌蚪、硫磺立方）不生成实体，但官方
        // `EntityType.create` 对 `level.random` 的取数照取。美西螈现已建模，走
        // `kind_of_bucket` 那一路，不进这里。
        // Buckets of unmodelled mobs (tadpole, sulfur cube) spawn no entity, but
        // vanilla's `EntityType.create` still draws from `level.random`. The axolotl
        // is modelled now and goes through `kind_of_bucket`, not this branch.
        if is_mob_bucket(item) {
            unmodelled_spawn_draws(world, item);
        }
        return;
    };
    // 没有物品堆时按默认组件建一个空桶；组件缺省值和官方一致。
    // Without a stack, build a bare bucket of that item; the component defaults match.
    let bare;
    let stack = match stack {
        Some(stack) => stack,
        None => {
            bare = ItemStack::new(item, 1);
            &bare
        }
    };
    spawn_from_bucket(world, kind, stack, pos);
}

/// 有生物、但 Bamboo 还没建模的桶。
///
/// Mob buckets whose species Bamboo does not model yet.
const fn is_mob_bucket(item: i32) -> bool {
    // 美西螈现已建模，走 `kind_of_bucket`；蝌蚪和硫磺立方还没建模。
    // The axolotl is modelled now and goes through `kind_of_bucket`; the tadpole and
    // sulfur cube are not modelled yet.
    matches!(item, items::TADPOLE_BUCKET | items::SULFUR_CUBE_BUCKET)
}

/// `MobBucketItem.spawn` 里 `EntityType.create` 对 `level.random` 的取数，用在没建模的生物上。
///
/// 先是 `snapTo` 的一次 `nextFloat`。美西螈在 `EntitySpawnReason.BUCKET` 时
/// `finalizeSpawn` 直接返回，其余都进 `Mob.finalizeSpawn`：一次 `triangle` 再一次
/// `nextFloat`。体型和 UUID 用实体自己的随机源，不占这里的数。
///
/// The `level.random` draws in `EntityType.create` from `MobBucketItem.spawn`, for the
/// species Bamboo does not model. `snapTo` draws one `nextFloat` first. An axolotl's
/// `finalizeSpawn` returns immediately for `EntitySpawnReason.BUCKET`; the rest reach
/// `Mob.finalizeSpawn`: one `triangle`, then one `nextFloat`. Size and UUID use the
/// entity's own random.
fn unmodelled_spawn_draws(world: &mut World, item: i32) {
    let _yaw = world.random().next_float();
    if item == items::AXOLOTL_BUCKET {
        return;
    }
    let _follow = world.random().triangle(0.0, 0.114_850_000_000_000_01);
    let _hand = world.random().next_float();
}

/// 桶物品对应的物种（`MobBucketItem.type`），不是生物桶时是 `None`。
///
/// The species of a mob bucket (`MobBucketItem.type`); `None` for other buckets.
pub const fn kind_of_bucket(item: i32) -> Option<MobType> {
    match item {
        items::COD_BUCKET => Some(MobType::Cod),
        items::SALMON_BUCKET => Some(MobType::Salmon),
        items::TROPICAL_FISH_BUCKET => Some(MobType::TropicalFish),
        items::PUFFERFISH_BUCKET => Some(MobType::Pufferfish),
        items::AXOLOTL_BUCKET => Some(MobType::Axolotl),
        _ => None,
    }
}

/// `MobBucketItem.spawn`：`EntityType.create(..., BUCKET, true, false)` 加
/// `loadFromBucketTag`、`setFromBucket(true)`、`addFreshEntity`、`playAmbientSound`。
///
/// `MobBucketItem.spawn`: the same steps vanilla runs after `emptyContents` succeeds.
pub fn spawn_from_bucket(
    world: &mut World,
    kind: MobType,
    stack: &ItemStack,
    pos: BlockPos,
) -> i32 {
    let mut entity =
        world.prepare_entity(Entity::by_mob_type(kind, mc_bamboo_core::Vec3::default()));
    entity.position = mc_bamboo_core::Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 1.0,
        f64::from(pos.z) + 0.5,
    );
    let offset = super::spawn::spawn_y_offset(&*world, pos, false, entity.bounding_box());
    let yaw = mc_bamboo_core::mth::wrap_degrees(world.random().next_float() * 360.0);
    super::spawn::snap_to(
        &mut entity,
        mc_bamboo_core::Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + offset,
            f64::from(pos.z) + 0.5,
        ),
        yaw,
        0.0,
    );
    entity.with_mob(|me, mob| {
        mob.y_head_rot = me.y_rot;
        mob.y_body_rot = me.y_rot;
    });
    super::spawn::finalize_spawn(world, kind, &mut entity, super::spawn::SpawnReason::Bucket);
    // `postSpawnConfig` 在 `finalizeSpawn` 之后套组件。
    // `postSpawnConfig` applies the components after `finalizeSpawn`.
    entity.with_mob(|me, mob| apply_bucket_stack(me, mob, stack));
    entity.with_mob(|_, mob| {
        // 鱼写 `fish.from_bucket`，美西螈写 `axolotl.from_bucket`。
        // Fish set `fish.from_bucket`; the axolotl sets `axolotl.from_bucket`.
        if kind == MobType::Axolotl {
            mob.axolotl.from_bucket = true;
        } else {
            mob.fish.from_bucket = true;
        }
        crate::entity::mob::living::clamp_health(mob);
    });
    let uid = world.add_prepared_entity(entity);
    world.with_entity_mut(uid, &mut |entity| {
        entity.with_mob(crate::entity::mob::living::play_ambient_sound);
    });
    uid
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::Vec3;

    use super::*;
    use crate::entity::mob::nbt;

    fn fish(kind: MobType) -> Entity {
        Entity::by_mob_type(kind, Vec3::new(0.5, 62.0, 0.5))
    }

    #[test]
    fn a_cod_bucket_carries_health_and_the_bucket_flag() {
        let mut entity = fish(MobType::Cod);
        entity.with_mob(|_, mob| mob.health = 2.0);
        let stack = entity
            .with_mob(|me, mob| bucket_stack(me, mob))
            .expect("a cod is bucketable")
            .expect("the cod has a bucket item");
        assert_eq!(stack.item, items::COD_BUCKET);
        let patch = ComponentPatch::from_opaque(&stack).expect("the patch decodes");
        let Some(ComponentValue::BucketEntityData(entries)) = patch.get(kinds::BUCKET_ENTITY_DATA)
        else {
            panic!("the bucket carries entity data");
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "Health");
        assert_eq!(entries[0].1, Tag::Float(2.0));
    }

    #[test]
    fn a_salmon_bucket_keeps_its_size() {
        let mut entity = fish(MobType::Salmon);
        entity.with_mob(|_, mob| mob.fish.variant = 2);
        let stack = entity
            .with_mob(|me, mob| bucket_stack(me, mob))
            .flatten()
            .expect("a salmon is bucketable");
        let patch = ComponentPatch::from_opaque(&stack).expect("the patch decodes");
        assert_eq!(
            patch.get(kinds::SALMON_SIZE),
            Some(&ComponentValue::SalmonSize(2))
        );
        // 放回时先按组件改成小鲑鱼。
        // Putting it back applies the component first.
        let mut restored = fish(MobType::Salmon);
        restored.with_mob(|me, mob| apply_bucket_stack(me, mob, &stack));
        assert_eq!(restored.kind.mob().expect("a mob").fish.variant, 2);
    }

    #[test]
    fn a_tropical_fish_bucket_splits_the_packed_variant() {
        let mut entity = fish(MobType::TropicalFish);
        let packed = crate::entity::mob::fish::pack_variant(7, 1, 7);
        entity.with_mob(|_, mob| mob.fish.variant = packed);
        let stack = entity
            .with_mob(|me, mob| bucket_stack(me, mob))
            .flatten()
            .expect("a tropical fish is bucketable");
        let patch = ComponentPatch::from_opaque(&stack).expect("the patch decodes");
        assert_eq!(
            patch.get(kinds::TROPICAL_FISH_BASE_COLOR),
            Some(&ComponentValue::TropicalFishBaseColor(1))
        );
        assert_eq!(
            patch.get(kinds::TROPICAL_FISH_PATTERN_COLOR),
            Some(&ComponentValue::TropicalFishPatternColor(7))
        );
        let mut restored = fish(MobType::TropicalFish);
        restored.with_mob(|me, mob| apply_bucket_stack(me, mob, &stack));
        assert_eq!(restored.kind.mob().expect("a mob").fish.variant, packed);
    }

    #[test]
    fn bucket_data_overrides_only_the_keys_it_has() {
        let mut entity = fish(MobType::Pufferfish);
        entity.with_mob(|_, mob| {
            mob.health = 3.0;
            mob.no_ai = true;
        });
        let tag = Tag::Compound(vec![
            ("Health".to_owned(), Tag::Float(1.0)),
            ("NoAI".to_owned(), Tag::Byte(0)),
        ]);
        entity.with_mob(|me, mob| load_default_data(me, mob, &tag));
        let mob = entity.kind.mob().expect("a mob");
        assert_eq!(mob.health, 1.0);
        assert!(!mob.no_ai);
    }

    #[test]
    fn a_bucket_round_trips_through_the_mob_save() {
        // 桶里的数据只是实体存档键的一个子集，键名和类型要和 `write_mob` 一致。
        // The bucket keys are a subset of the mob save; names and types must match.
        let mut entity = fish(MobType::Cod);
        entity.with_mob(|_, mob| {
            mob.health = 1.5;
            mob.custom_name = Some(Tag::String("Bubbles".to_owned()));
        });
        let stack = entity
            .with_mob(|me, mob| bucket_stack(me, mob))
            .flatten()
            .expect("a cod is bucketable");
        let mut restored = fish(MobType::Cod);
        restored.with_mob(|me, mob| apply_bucket_stack(me, mob, &stack));
        let mob = restored.kind.mob().expect("a mob");
        assert_eq!(mob.health, 1.5);
        assert_eq!(mob.custom_name, Some(Tag::String("Bubbles".to_owned())));
        assert!(nbt::write_mob(&restored.split()).is_ok());
    }
}
