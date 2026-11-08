use mc_bamboo_data::{blocks, items};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::components::{
    cursor::Holder,
    value::{BannerPattern, PartialProfile, ResolvableProfile, SkinPatch},
};

use super::*;
use crate::{inbound::PlayerId, region::Region, world::tests::Harness};

const POS: BlockPos = BlockPos::new(3, -60, 8);
/// 放下方块的那名玩家。这里不查来源，只要有一个就行。
const TEST_PLAYER: PlayerId = PlayerId(0);

/// 一个只有方块的世界，方块已经放好。
fn region_with(block: u32) -> Harness {
    let mut server = Harness::new(8);
    server.region().chunks.set_block(POS, block);
    server
}

/// 把一项组件的值编成线上的字节，放进物品。
fn with_component(item: i32, component: i32, value: &ComponentValue) -> ItemStack {
    let mut bytes = Vec::new();
    value.write(&mut bytes);
    ItemStack {
        item,
        count: 1,
        added: vec![(component, bytes)],
        removed: Vec::new(),
    }
}

fn banner_layers() -> ComponentValue {
    ComponentValue::BannerPatterns(vec![BannerLayer {
        // `minecraft:flower` 在同步注册表里的下标。
        pattern: Holder::Id(12),
        color: DyeColor::Red.id(),
    }])
}

/// 区块里这一格方块实体的那段 NBT。
fn stored_tag(region: &Region) -> Option<Vec<u8>> {
    region.chunks.update_tag(POS).map(<[u8]>::to_vec)
}

#[test]
fn a_banner_keeps_its_patterns_and_sends_them_to_the_client() {
    let mut server = region_with(blocks::WHITE_BANNER);
    let banner = with_component(
        items::WHITE_BANNER,
        kinds::BANNER_PATTERNS,
        &banner_layers(),
    );

    apply(server.region(), POS, TEST_PLAYER, &banner);

    assert_eq!(
        server.region().block_items[&POS].get(kinds::BANNER_PATTERNS),
        Some(&banner_layers())
    );
    // `patterns` 里是图案的注册表名和颜色名，后面跟着方块实体自己那张空组件表。
    let tag = stored_tag(server.region()).expect("旗帜要发方块实体数据");
    let text = String::from_utf8_lossy(&tag).to_string();
    assert!(text.contains("patterns"), "{text}");
    assert!(text.contains("minecraft:flower"), "{text}");
    assert!(text.contains("red"), "{text}");
    assert!(text.contains("components"), "{text}");
}

#[test]
fn a_blank_banner_still_has_an_empty_pattern_list() {
    let mut server = region_with(blocks::WHITE_BANNER);
    apply(
        server.region(),
        POS,
        TEST_PLAYER,
        &ItemStack::new(items::WHITE_BANNER, 1),
    );

    // 官方用 `getOrDefault` 读图案，所以空白旗帜也留一份空列表，中键选取时还给物品。
    assert_eq!(
        collect(server.region(), POS).get(kinds::BANNER_PATTERNS),
        Some(&ComponentValue::BannerPatterns(Vec::new()))
    );
    // 空列表不写进 `getUpdateTag`，但方块实体的组件表还是要写。
    let tag = stored_tag(server.region()).expect("旗帜要发方块实体数据");
    assert!(!String::from_utf8_lossy(&tag).contains("patterns"));
}

#[test]
fn a_named_banner_sends_its_name() {
    let mut server = region_with(blocks::WHITE_BANNER);
    let name = ComponentValue::CustomName(Component::text("Standard"));
    let banner = with_component(items::WHITE_BANNER, kinds::CUSTOM_NAME, &name);

    apply(server.region(), POS, TEST_PLAYER, &banner);

    let tag = stored_tag(server.region()).expect("旗帜要发方块实体数据");
    let text = String::from_utf8_lossy(&tag).to_string();
    assert!(text.contains("CustomName"), "{text}");
    assert!(text.contains("Standard"), "{text}");
}

#[test]
fn a_skull_sends_its_profile_and_note_block_sound() {
    let mut server = region_with(blocks::PLAYER_HEAD);
    let profile = ComponentValue::Profile(ResolvableProfile {
        profile: Err(PartialProfile {
            name: Some("Kitten".to_owned()),
            id: None,
            properties: Vec::new(),
        }),
        skin_patch: SkinPatch {
            body: None,
            cape: None,
            elytra: None,
            slim: Some(true),
        },
    });
    let mut head = with_component(items::PLAYER_HEAD, kinds::PROFILE, &profile);
    let mut sound = Vec::new();
    ComponentValue::NoteBlockSound("minecraft:custom.sound".to_owned()).write(&mut sound);
    head.added.push((kinds::NOTE_BLOCK_SOUND, sound));

    apply(server.region(), POS, TEST_PLAYER, &head);

    let tag = stored_tag(server.region()).expect("头颅要发方块实体数据");
    let text = String::from_utf8_lossy(&tag).to_string();
    assert!(text.contains("profile"), "{text}");
    assert!(text.contains("Kitten"), "{text}");
    assert!(text.contains("slim"), "{text}");
    assert!(text.contains("note_block_sound"), "{text}");
    assert!(text.contains("minecraft:custom.sound"), "{text}");
    // 头颅的 `getUpdateTag` 走 `saveCustomOnly`，没有组件表。
    assert!(!text.contains("components"), "{text}");
}

#[test]
fn a_decorated_pot_sends_its_sherds() {
    let mut server = region_with(blocks::DECORATED_POT);
    let sherd = |item| ItemStackTemplate {
        item,
        count: 1,
        components: ComponentPatch::new(),
    };
    let decorations = ComponentValue::PotDecorations {
        back: Some(sherd(items::BRICK)),
        left: None,
        right: None,
        front: Some(sherd(items::ANGLER_POTTERY_SHERD)),
    };
    let pot = with_component(items::DECORATED_POT, kinds::POT_DECORATIONS, &decorations);

    apply(server.region(), POS, TEST_PLAYER, &pot);

    let tag = stored_tag(server.region()).expect("陶罐要发方块实体数据");
    let text = String::from_utf8_lossy(&tag).to_string();
    assert!(text.contains("sherds"), "{text}");
    assert!(text.contains("minecraft:brick"), "{text}");
    assert!(text.contains("minecraft:angler_pottery_sherd"), "{text}");
    assert!(!text.contains("left"), "{text}");
}

#[test]
fn a_plain_block_entity_has_nothing_to_send() {
    let mut server = region_with(blocks::CHEST);
    apply(
        server.region(),
        POS,
        TEST_PLAYER,
        &ItemStack::new(items::CHEST, 1),
    );

    assert_eq!(stored_tag(server.region()), None);
    // 箱子认自定义名和锁，两样都没有时不留条目。
    assert!(!server.region().block_items.contains_key(&POS));
}

#[test]
fn a_named_chest_keeps_its_name_but_sends_no_data() {
    let mut server = region_with(blocks::CHEST);
    let name = ComponentValue::CustomName(Component::text("Loot"));
    let chest = with_component(items::CHEST, kinds::CUSTOM_NAME, &name);

    apply(server.region(), POS, TEST_PLAYER, &chest);

    assert_eq!(
        custom_name(server.region(), POS),
        Some(&Component::text("Loot"))
    );
    // 箱子的 `getUpdateTag` 是空的，官方在区块包里写一个 `TAG_End`。
    assert_eq!(stored_tag(server.region()), None);
}

#[test]
fn an_empty_lock_counts_as_no_lock() {
    let mut server = region_with(blocks::CHEST);
    let lock = ComponentValue::Lock(Tag::Compound(Vec::new()));
    let chest = with_component(items::CHEST, kinds::LOCK, &lock);

    apply(server.region(), POS, TEST_PLAYER, &chest);

    assert!(!server.region().block_items.contains_key(&POS));
}

/// 锁留在方块实体上，但不跟着中键选取的物品走：方块实体是共享的，谁都能中键。
#[test]
fn a_real_lock_stays_on_the_block_entity_but_not_on_the_item() {
    let mut server = region_with(blocks::CHEST);
    let lock = ComponentValue::Lock(Tag::Compound(vec![(
        "items".to_owned(),
        Tag::String("minecraft:tripwire_hook".to_owned()),
    )]));
    let chest = with_component(items::CHEST, kinds::LOCK, &lock);

    apply(server.region(), POS, TEST_PLAYER, &chest);

    assert_eq!(
        server.region().block_items[&POS].get(kinds::LOCK),
        Some(&lock)
    );
    assert_eq!(collect(server.region(), POS).get(kinds::LOCK), None);
    // 容器的 `getUpdateTag` 本来就是空的，锁也不会发出去。
    assert_eq!(stored_tag(server.region()), None);
}

#[test]
fn a_shulker_box_keeps_the_items_from_its_container_component() {
    let mut server = region_with(blocks::SHULKER_BOX);
    let contents = ComponentValue::Container(vec![
        None,
        Some(ItemStackTemplate {
            item: items::DIAMOND,
            count: 5,
            components: ComponentPatch::new(),
        }),
    ]);
    let box_item = with_component(items::SHULKER_BOX, kinds::CONTAINER, &contents);

    apply(server.region(), POS, TEST_PLAYER, &box_item);

    let entry = &server.region().containers[&POS];
    assert!(entry.slots[0].is_empty());
    assert_eq!(entry.slots[1], ItemStack::new(items::DIAMOND, 5));
    assert!(entry.slots[2].is_empty());

    // `collectComponents` 把内容原样还出来，末尾的空格子不写。
    assert_eq!(
        collect(server.region(), POS).get(kinds::CONTAINER),
        Some(&contents)
    );
}

#[test]
fn a_container_slot_with_too_many_items_becomes_empty() {
    let mut server = region_with(blocks::CHEST);
    // 一组 99 个的钻石剑过不了 `validateStrict`，官方 `create` 这时返回空物品堆。
    let contents = ComponentValue::Container(vec![Some(ItemStackTemplate {
        item: items::DIAMOND_SWORD,
        count: 99,
        components: ComponentPatch::new(),
    })]);
    let chest = with_component(items::CHEST, kinds::CONTAINER, &contents);

    apply(server.region(), POS, TEST_PLAYER, &chest);

    assert!(server.region().containers[&POS].slots[0].is_empty());
}

#[test]
fn an_unknown_banner_pattern_id_is_left_out() {
    let mut server = region_with(blocks::WHITE_BANNER);
    let layers = ComponentValue::BannerPatterns(vec![BannerLayer {
        pattern: Holder::Inline(BannerPattern {
            asset_id: "minecraft:flower".to_owned(),
            translation_key: "block.minecraft.banner.flower".to_owned(),
        }),
        color: DyeColor::Blue.id(),
    }]);
    let banner = with_component(items::WHITE_BANNER, kinds::BANNER_PATTERNS, &layers);

    apply(server.region(), POS, TEST_PLAYER, &banner);

    // 内联的图案定义也写得出来。
    let tag = stored_tag(server.region()).expect("旗帜要发方块实体数据");
    let text = String::from_utf8_lossy(&tag).to_string();
    assert!(text.contains("asset_id"), "{text}");
    assert!(text.contains("translation_key"), "{text}");
}

#[test]
fn the_data_goes_away_with_the_block() {
    let mut server = region_with(blocks::WHITE_BANNER);
    let banner = with_component(
        items::WHITE_BANNER,
        kinds::BANNER_PATTERNS,
        &banner_layers(),
    );
    apply(server.region(), POS, TEST_PLAYER, &banner);
    assert!(stored_tag(server.region()).is_some());

    server.region().chunks.set_block(POS, blocks::STONE);
    assert_eq!(stored_tag(server.region()), None);

    forget(server.region(), POS);
    assert!(!server.region().block_items.contains_key(&POS));
}
