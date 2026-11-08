//! 组件编解码的单元测试：往返、上限和畸形输入。

use mc_bamboo_data::{items, registries::data_component_type as kinds};

use super::*;
use crate::{
    codec::{write_f32, write_i32},
    components::{
        cursor::{HolderSet, write_nbt_tag},
        value::SignText,
    },
    text::{Color, Component},
};

/// 把一串字节解码成一个组件值，要求刚好读完。
fn decode(kind: i32, bytes: &[u8]) -> Result<ComponentValue, DecodeError> {
    let mut cursor = Cursor::new(bytes);
    let value = ComponentValue::read(&mut cursor, kind)?;
    cursor.finish()?;
    Ok(value)
}

fn encode(value: &ComponentValue) -> Vec<u8> {
    let mut out = Vec::new();
    value.write(&mut out);
    out
}

/// 一个值写出去再读回来必须一模一样，字节也必须一样。
fn round_trip(kind: i32, value: &ComponentValue) {
    let bytes = encode(value);
    assert_eq!(decode(kind, &bytes).as_ref(), Ok(value), "kind {kind}");
    assert_eq!(encode(&decode(kind, &bytes).unwrap()), bytes, "kind {kind}");
}

#[test]
fn simple_components_round_trip() {
    round_trip(kinds::MAX_STACK_SIZE, &ComponentValue::MaxStackSize(64));
    round_trip(kinds::DAMAGE, &ComponentValue::Damage(3));
    round_trip(kinds::UNBREAKABLE, &ComponentValue::Unbreakable);
    round_trip(kinds::WAXED, &ComponentValue::Waxed);
    round_trip(kinds::RARITY, &ComponentValue::Rarity(2));
    round_trip(kinds::DYED_COLOR, &ComponentValue::DyedColor(-6_265_536));
    round_trip(
        kinds::ITEM_MODEL,
        &ComponentValue::ItemModel("minecraft:stone".to_owned()),
    );
    round_trip(
        kinds::INTANGIBLE_PROJECTILE,
        &ComponentValue::IntangibleProjectile,
    );
}

#[test]
fn unit_components_take_no_bytes() {
    assert!(encode(&ComponentValue::Unbreakable).is_empty());
    assert!(encode(&ComponentValue::Glider).is_empty());
    assert!(encode(&ComponentValue::CreativeSlotLock).is_empty());
    assert!(encode(&ComponentValue::Waxed).is_empty());
    // intangible_projectile 走 NBT，官方写的是一个空的复合标签。
    assert_eq!(encode(&ComponentValue::IntangibleProjectile), [0x0a, 0x00]);
}

#[test]
fn text_components_round_trip() {
    let name = Component::text("Kitten").color(Color::Gold);
    round_trip(
        kinds::CUSTOM_NAME,
        &ComponentValue::CustomName(name.clone()),
    );
    round_trip(
        kinds::LORE,
        &ComponentValue::Lore(vec![name.clone(), Component::text("line")]),
    );
}

#[test]
fn enchantments_keep_the_order_they_came_in() {
    let value = ComponentValue::Enchantments(vec![(3, 5), (1, 2)]);
    round_trip(kinds::ENCHANTMENTS, &value);
    // 顺序倒过来是另一串字节，不能被归一化掉。
    let other = ComponentValue::Enchantments(vec![(1, 2), (3, 5)]);
    assert_ne!(encode(&value), encode(&other));
}

#[test]
fn nested_stacks_round_trip() {
    let inner = ItemStackTemplate {
        item: items::STONE,
        count: 2,
        components: ComponentPatch::default(),
    };
    let value = ComponentValue::Container(vec![Some(inner.clone()), None]);
    round_trip(kinds::CONTAINER, &value);
    round_trip(
        kinds::BUNDLE_CONTENTS,
        &ComponentValue::BundleContents(vec![inner]),
    );
}

#[test]
fn a_nested_stack_cannot_be_empty() {
    // 数量 0 和空气都不行，官方的构造方法会抛异常。
    let mut bytes = Vec::new();
    write_varint(&mut bytes, 1);
    write_varint(&mut bytes, items::STONE);
    write_varint(&mut bytes, 0);
    write_varint(&mut bytes, 0);
    write_varint(&mut bytes, 0);
    assert_eq!(
        decode(kinds::BUNDLE_CONTENTS, &bytes),
        Err(DecodeError::InvalidComponentValue(
            "nested item stack must be non-empty"
        ))
    );

    let mut bytes = Vec::new();
    write_varint(&mut bytes, 1);
    write_varint(&mut bytes, items::AIR);
    write_varint(&mut bytes, 1);
    write_varint(&mut bytes, 0);
    write_varint(&mut bytes, 0);
    assert!(decode(kinds::BUNDLE_CONTENTS, &bytes).is_err());
}

#[test]
fn nesting_stops_at_the_depth_limit() {
    /// 一个 `bundle_contents` 里套一个物品，物品自己又带一个 `bundle_contents`。
    fn nest(levels: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_varint(&mut bytes, 1);
        write_varint(&mut bytes, items::BUNDLE);
        write_varint(&mut bytes, 1);
        if levels == 0 {
            // 没有组件的物品堆。
            write_varint(&mut bytes, 0);
            write_varint(&mut bytes, 0);
            return bytes;
        }
        write_varint(&mut bytes, 1);
        write_varint(&mut bytes, 0);
        write_varint(&mut bytes, kinds::BUNDLE_CONTENTS);
        bytes.extend(nest(levels - 1));
        bytes
    }

    let deepest = nest(cursor::MAX_STACK_DEPTH - 1);
    assert!(decode(kinds::BUNDLE_CONTENTS, &deepest).is_ok());

    let too_deep = nest(cursor::MAX_STACK_DEPTH);
    assert_eq!(
        decode(kinds::BUNDLE_CONTENTS, &too_deep),
        Err(DecodeError::StacksTooDeep {
            max_depth: cursor::MAX_STACK_DEPTH
        })
    );
}

#[test]
fn holder_sets_and_tags_round_trip() {
    round_trip(
        kinds::REPAIRABLE,
        &ComponentValue::Repairable(HolderSet::Ids(vec![items::STONE, items::DIRT])),
    );
    round_trip(
        kinds::REPAIRABLE,
        &ComponentValue::Repairable(HolderSet::Tag("minecraft:planks".to_owned())),
    );
}

#[test]
fn an_unknown_component_type_is_an_error() {
    let total = mc_bamboo_data::component_registries::sizes::DATA_COMPONENT_TYPE as i32;
    assert_eq!(
        decode(total, &[]),
        Err(DecodeError::UnknownComponent(total))
    );
    assert_eq!(decode(-1, &[]), Err(DecodeError::UnknownComponent(-1)));
}

#[test]
fn an_unregistered_id_inside_a_component_is_an_error() {
    let mut bytes = Vec::new();
    write_varint(&mut bytes, items::ITEMS.len() as i32);
    assert_eq!(
        decode(kinds::DAMAGE_TYPE, &bytes),
        Err(DecodeError::UnknownRegistryId {
            registry: "minecraft:damage_type",
            id: items::ITEMS.len() as i32,
        })
    );
}

#[test]
fn truncated_values_are_errors() {
    assert_eq!(
        decode(kinds::MINIMUM_ATTACK_CHARGE, &[0, 0]),
        Err(DecodeError::UnexpectedEnd)
    );
    assert_eq!(
        decode(kinds::CUSTOM_NAME, &[0x0a]),
        Err(DecodeError::Nbt(mc_bamboo_nbt::DecodeError::UnexpectedEnd))
    );
}

#[test]
fn trailing_bytes_in_a_value_are_an_error() {
    let mut bytes = Vec::new();
    write_f32(&mut bytes, 1.0);
    bytes.push(0);
    assert_eq!(
        decode(kinds::MINIMUM_ATTACK_CHARGE, &bytes),
        Err(DecodeError::TrailingBytes(1))
    );
}

#[test]
fn out_of_range_enum_ids_fold_like_the_game() {
    let mut bytes = Vec::new();
    write_varint(&mut bytes, 99);
    // Rarity 用 ZERO 策略，99 变成 0。
    assert_eq!(decode(kinds::RARITY, &bytes), Ok(ComponentValue::Rarity(0)));
    // Salmon 的大小用 CLAMP 策略，99 变成 2。
    assert_eq!(
        decode(kinds::SALMON_SIZE, &bytes),
        Ok(ComponentValue::SalmonSize(2))
    );
    // 马的花色用 WRAP 策略，99 变成 99 % 7 = 1。
    assert_eq!(
        decode(kinds::HORSE_VARIANT, &bytes),
        Ok(ComponentValue::HorseVariant(1))
    );
    // 兔子的花色是稀疏表，99 是杀手兔，自己就在表里。
    assert_eq!(
        decode(kinds::RABBIT_VARIANT, &bytes),
        Ok(ComponentValue::RabbitVariant(99))
    );
}

#[test]
fn the_nbt_backed_components_check_their_outer_shape() {
    let mut bytes = Vec::new();
    write_nbt_tag(&mut bytes, &Tag::Compound(Vec::new()));
    assert!(decode(kinds::LOCK, &bytes).is_ok());

    let mut bytes = Vec::new();
    write_nbt_tag(&mut bytes, &Tag::List(Vec::new()));
    assert_eq!(
        decode(kinds::LOCK, &bytes),
        Err(DecodeError::InvalidComponentValue(
            "component NBT has the wrong shape"
        ))
    );
    assert!(decode(kinds::RECIPES, &bytes).is_ok());
}

#[test]
fn sign_text_has_four_fixed_lines() {
    let line = Component::text("a");
    let value = ComponentValue::SignTextFront(SignText {
        messages: vec![line.clone(), line.clone(), line.clone(), line.clone()],
        filtered_messages: None,
        color: 15,
        has_glowing_text: true,
    });
    round_trip(kinds::SIGN_TEXT_FRONT, &value);

    // 只写三行时后面的字节不够。
    let mut bytes = Vec::new();
    for _ in 0..3 {
        text::write(&mut bytes, &line);
    }
    assert_eq!(
        decode(kinds::SIGN_TEXT_FRONT, &bytes),
        Err(DecodeError::Nbt(mc_bamboo_nbt::DecodeError::UnexpectedEnd))
    );
}

#[test]
fn lists_respect_the_official_limit() {
    let mut bytes = Vec::new();
    write_varint(&mut bytes, 257);
    assert_eq!(
        decode(kinds::LORE, &bytes),
        Err(DecodeError::InvalidCount(257))
    );

    let mut bytes = Vec::new();
    write_varint(&mut bytes, 0);
    write_varint(&mut bytes, 257);
    assert_eq!(
        decode(kinds::FIREWORKS, &bytes),
        Err(DecodeError::InvalidCount(257))
    );
}

#[test]
fn patches_round_trip_in_the_trusted_format() {
    let mut patch_bytes = Vec::new();
    write_varint(&mut patch_bytes, 2);
    write_varint(&mut patch_bytes, 1);
    write_varint(&mut patch_bytes, kinds::MAX_STACK_SIZE);
    write_varint(&mut patch_bytes, 16);
    write_varint(&mut patch_bytes, kinds::DAMAGE);
    write_varint(&mut patch_bytes, 7);
    write_varint(&mut patch_bytes, kinds::LORE);

    let mut cursor = Cursor::new(&patch_bytes);
    let patch = ComponentPatch::read(&mut cursor).expect("合法的补丁");
    assert_eq!(cursor.finish(), Ok(()));
    assert_eq!(patch.max_stack_size(), Some(16));
    assert_eq!(patch.get(kinds::LORE), None);

    let mut again = Vec::new();
    patch.write(&mut again);
    assert_eq!(again, patch_bytes);
}

#[test]
fn a_repeated_component_type_keeps_the_last_value() {
    let mut bytes = Vec::new();
    write_varint(&mut bytes, 2);
    write_varint(&mut bytes, 0);
    write_varint(&mut bytes, kinds::MAX_STACK_SIZE);
    write_varint(&mut bytes, 16);
    write_varint(&mut bytes, kinds::MAX_STACK_SIZE);
    write_varint(&mut bytes, 32);

    let mut cursor = Cursor::new(&bytes);
    let patch = ComponentPatch::read(&mut cursor).expect("官方用 Map.put");
    assert_eq!(patch.max_stack_size(), Some(32));
    assert_eq!(patch.entries().len(), 1);
}

#[test]
fn a_patch_with_too_many_entries_is_rejected() {
    let mut bytes = Vec::new();
    write_varint(&mut bytes, MAX_PATCH_ENTRIES as i32);
    write_varint(&mut bytes, 1);
    let mut cursor = Cursor::new(&bytes);
    assert_eq!(
        ComponentPatch::read(&mut cursor),
        Err(DecodeError::InvalidCount(MAX_PATCH_ENTRIES as i32 + 1))
    );
}

#[test]
fn a_negative_patch_count_is_rejected() {
    let mut bytes = Vec::new();
    write_varint(&mut bytes, -1);
    write_varint(&mut bytes, 0);
    let mut cursor = Cursor::new(&bytes);
    assert_eq!(
        ComponentPatch::read(&mut cursor),
        Err(DecodeError::InvalidCount(-1))
    );
}

#[test]
fn opaque_bytes_round_trip_through_the_typed_form() {
    let mut value = Vec::new();
    write_varint(&mut value, 16);
    let stack = ItemStack {
        item: items::STONE,
        count: 1,
        added: vec![(kinds::MAX_STACK_SIZE, value)],
        removed: vec![kinds::LORE],
    };

    let patch = ComponentPatch::from_opaque(&stack).expect("合法的组件");
    assert_eq!(patch.max_stack_size(), Some(16));
    let (added, removed) = patch.to_opaque();
    assert_eq!(added, stack.added);
    assert_eq!(removed, stack.removed);
}

#[test]
fn opaque_bytes_must_be_used_up_exactly() {
    let stack = ItemStack {
        item: items::STONE,
        count: 1,
        added: vec![(kinds::MAX_STACK_SIZE, vec![16, 0])],
        removed: Vec::new(),
    };
    assert_eq!(
        ComponentPatch::from_opaque(&stack),
        Err(DecodeError::TrailingBytes(1))
    );
}

#[test]
fn the_delimited_format_drops_the_lengths() {
    let mut value = Vec::new();
    write_varint(&mut value, 16);

    let mut bytes = Vec::new();
    write_varint(&mut bytes, 1);
    write_varint(&mut bytes, 0);
    write_varint(&mut bytes, kinds::MAX_STACK_SIZE);
    write_varint(&mut bytes, value.len() as i32);
    bytes.extend_from_slice(&value);

    let mut cursor = Cursor::new(&bytes);
    let patch = ComponentPatch::read_delimited(&mut cursor).expect("合法的补丁");
    assert_eq!(cursor.finish(), Ok(()));
    assert_eq!(patch.max_stack_size(), Some(16));

    let mut trusted = Vec::new();
    patch.write(&mut trusted);
    let mut expected = Vec::new();
    write_varint(&mut expected, 1);
    write_varint(&mut expected, 0);
    write_varint(&mut expected, kinds::MAX_STACK_SIZE);
    expected.extend_from_slice(&value);
    assert_eq!(trusted, expected);
}

#[test]
fn validation_follows_the_official_rules() {
    use crate::components::validate::validate_stack;

    // 一组 65 个石头超过了堆叠上限。
    let stack = ItemStack::new(items::STONE, 65);
    assert_eq!(
        validate_stack(&stack),
        Err(DecodeError::InvalidItemCount(65))
    );
    assert_eq!(validate_stack(&ItemStack::new(items::STONE, 64)), Ok(()));

    // 容器里的物品也要查数量。
    let inner = ItemStackTemplate {
        item: items::DIAMOND_SWORD,
        count: 2,
        components: ComponentPatch::default(),
    };
    let mut value = Vec::new();
    ComponentValue::Container(vec![Some(inner)]).write(&mut value);
    let stack = ItemStack {
        item: items::SHULKER_BOX,
        count: 1,
        added: vec![(kinds::CONTAINER, value)],
        removed: Vec::new(),
    };
    assert_eq!(
        validate_stack(&stack),
        Err(DecodeError::InvalidItemCount(2))
    );
}

#[test]
fn a_patched_max_stack_size_lifts_the_limit() {
    use crate::components::validate::validate_stack;

    let mut value = Vec::new();
    write_varint(&mut value, 99);
    let stack = ItemStack {
        item: items::DIAMOND_SWORD,
        count: 2,
        added: vec![(kinds::MAX_STACK_SIZE, value)],
        removed: Vec::new(),
    };
    assert_eq!(validate_stack(&stack), Ok(()));
}

#[test]
fn damageable_and_stackable_at_once_is_rejected() {
    use crate::components::validate::validate_stack;

    let mut max_damage = Vec::new();
    write_varint(&mut max_damage, 100);
    let mut max_stack = Vec::new();
    write_varint(&mut max_stack, 16);
    let stack = ItemStack {
        item: items::STONE,
        count: 1,
        added: vec![
            (kinds::MAX_DAMAGE, max_damage),
            (kinds::MAX_STACK_SIZE, max_stack),
        ],
        removed: Vec::new(),
    };
    assert!(validate_stack(&stack).is_err());
}

#[test]
fn block_predicates_round_trip() {
    let predicate = predicate::BlockPredicate {
        blocks: Some(HolderSet::Ids(vec![1, 2])),
        properties: Some(predicate::StateProperties(vec![
            predicate::PropertyMatcher {
                name: "facing".to_owned(),
                matcher: predicate::ValueMatcher::Exact("north".to_owned()),
            },
        ])),
        nbt: Some(vec![("a".to_owned(), Tag::Byte(1))]),
        components: predicate::ComponentMatchers::default(),
    };
    round_trip(kinds::CAN_BREAK, &ComponentValue::CanBreak(vec![predicate]));
}

#[test]
fn exact_component_predicates_nest_component_values() {
    let matchers = predicate::ComponentMatchers {
        exact: vec![predicate::TypedComponent {
            kind: kinds::DAMAGE,
            value: ComponentValue::Damage(5),
        }],
        partial: vec![predicate::PartialPredicate {
            kind: predicate::PredicateType::AnyValue(kinds::ENCHANTMENTS),
            value: Tag::Compound(Vec::new()),
        }],
    };
    let predicate = predicate::BlockPredicate {
        blocks: None,
        properties: None,
        nbt: None,
        components: matchers,
    };
    round_trip(
        kinds::CAN_PLACE_ON,
        &ComponentValue::CanPlaceOn(vec![predicate]),
    );
}

#[test]
fn profiles_round_trip_in_both_forms() {
    use crate::components::value::{
        GameProfile, PartialProfile, Property, ResolvableProfile, SkinPatch,
    };

    let resolved = ResolvableProfile {
        profile: Ok(GameProfile {
            id: uuid::Uuid::from_u128(1),
            name: "Kitten".to_owned(),
            properties: vec![Property {
                name: "textures".to_owned(),
                value: "abc".to_owned(),
                signature: Some("sig".to_owned()),
            }],
        }),
        skin_patch: SkinPatch {
            body: Some("minecraft:body".to_owned()),
            cape: None,
            elytra: None,
            slim: Some(true),
        },
    };
    round_trip(kinds::PROFILE, &ComponentValue::Profile(resolved));

    let unresolved = ResolvableProfile {
        profile: Err(PartialProfile {
            name: Some("Kitten".to_owned()),
            id: None,
            properties: Vec::new(),
        }),
        skin_patch: SkinPatch {
            body: None,
            cape: None,
            elytra: None,
            slim: None,
        },
    };
    round_trip(kinds::PROFILE, &ComponentValue::Profile(unresolved));
}

#[test]
fn every_component_type_has_a_decoder() {
    // 没有实现的类型会报 UnknownComponent，其余的错误说明解码器存在但字节不够。
    for kind in 0..mc_bamboo_data::component_registries::sizes::DATA_COMPONENT_TYPE as i32 {
        let result = decode(kind, &[]);
        assert_ne!(
            result,
            Err(DecodeError::UnknownComponent(kind)),
            "组件类型 {kind} 还没有解码器"
        );
    }
}

#[test]
fn hidden_mob_effects_stop_at_the_limit() {
    /// `MobEffectInstance.Details`：放大等级、持续时间、三个开关，再跟一个可选的下一层。
    fn details(out: &mut Vec<u8>, levels: u32) {
        write_varint(out, 0);
        write_varint(out, 0);
        for _ in 0..3 {
            write_bool(out, false);
        }
        match levels {
            0 => write_bool(out, false),
            _ => {
                write_bool(out, true);
                details(out, levels - 1);
            }
        }
    }

    /// 一个 `potion_contents`，里面只有一个自定义效果。
    fn nest(levels: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_bool(&mut bytes, false);
        write_bool(&mut bytes, false);
        write_varint(&mut bytes, 1);
        write_varint(&mut bytes, 0);
        details(&mut bytes, levels);
        write_bool(&mut bytes, false);
        bytes
    }

    assert!(decode(kinds::POTION_CONTENTS, &nest(10)).is_ok());
    assert!(decode(kinds::POTION_CONTENTS, &nest(100)).is_err());
}

#[test]
fn colors_are_big_endian_ints() {
    let mut bytes = Vec::new();
    write_i32(&mut bytes, 0x0012_3456);
    assert_eq!(
        decode(kinds::DYED_COLOR, &bytes),
        Ok(ComponentValue::DyedColor(0x0012_3456))
    );
}
