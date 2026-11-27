//! 存档编解码的单元测试：每种方块实体写 → 按 id 读 → 再写，计划刻和实体往返回。

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{
    blocks,
    items::{STONE, WIND_CHARGE},
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;

use super::{
    block_entity::{BlockEntityKind, create_block_entity, write_block_entity_additional},
    entity::{entity_uuid_tag, read_entity, read_entity_tree, write_entity, write_entity_tree},
    tag::{self, field, put},
    ticks::{decode_chunk_ticks, encode_chunk_ticks},
};
use crate::{
    BlockEntity, BlockState, ChunkTicks, TickPriority,
    behaviour::machines::{
        BrewingStandEntity, CampfireEntity, FurnaceEntity, PistonMoving, ShulkerBoxEntity,
    },
    entity::{Entity, EntityKind, FallingBlock, GroundItem, PrimedTnt},
    hopper::HopperEntity,
    state::AXIS,
};

const POS: BlockPos = BlockPos::new(3, 64, 5);

fn entries(tag: &Tag, key: &str) -> Vec<(String, Tag)> {
    match field(tag, key) {
        Some(Tag::Compound(values)) => values.clone(),
        _ => Vec::new(),
    }
}

fn round_trip(entity: Box<dyn BlockEntity>, id: &str) -> Vec<(String, Tag)> {
    let first = write_block_entity_additional(entity.as_ref()).expect("写出成功");
    let tag = Tag::Compound(first);
    let rebuilt = create_block_entity(id, POS, BlockState(blocks::AIR), &tag)
        .expect("读回成功")
        .unwrap_or_else(|| panic!("{id} 没有构造分支"));
    assert_eq!(rebuilt.pos(), POS);
    write_block_entity_additional(rebuilt.as_ref()).expect("再写成功")
}

/// 每种会实例化的方块实体都有构造分支，写 → 读 → 写不变。
#[test]
fn every_block_entity_kind_round_trips() {
    for kind in BlockEntityKind::ALL {
        let entity: Box<dyn BlockEntity> = match kind {
            BlockEntityKind::Furnace => {
                let mut furnace = FurnaceEntity::new(POS, mc_bamboo_data::CookingKind::Smelting);
                furnace.write_item(0, ItemStack::new(STONE, 3));
                furnace.set_data(2, 77);
                Box::new(furnace)
            }
            BlockEntityKind::BlastFurnace => {
                let mut furnace = FurnaceEntity::new(POS, mc_bamboo_data::CookingKind::Blasting);
                furnace.set_data(0, 120);
                Box::new(furnace)
            }
            BlockEntityKind::Smoker => {
                let mut furnace = FurnaceEntity::new(POS, mc_bamboo_data::CookingKind::Smoking);
                furnace.set_data(3, 200);
                Box::new(furnace)
            }
            BlockEntityKind::BrewingStand => {
                let mut stand = BrewingStandEntity::new(POS);
                stand.set_item(3, ItemStack::new(STONE, 1));
                stand.set_data(0, 123);
                stand.set_data(2, 400);
                Box::new(stand)
            }
            BlockEntityKind::Hopper => {
                let mut hopper = HopperEntity::new(POS);
                hopper.set_cooldown(-1);
                Box::new(hopper)
            }
            BlockEntityKind::ShulkerBox => Box::new(ShulkerBoxEntity::new(POS)),
            BlockEntityKind::Campfire => {
                let mut fire = CampfireEntity::new(POS);
                fire.set_item(0, ItemStack::new(STONE, 1));
                fire.set_data(4, 100);
                Box::new(fire)
            }
            BlockEntityKind::Piston => {
                let mut piston = PistonMoving::new(
                    POS,
                    BlockState(blocks::STONE),
                    Direction::North,
                    true,
                    false,
                );
                piston.set_archive_progress(0.5);
                Box::new(piston)
            }
            BlockEntityKind::Beehive => {
                Box::new(crate::behaviour::machines::beehive_entity::BeehiveEntity::new(POS))
            }
            BlockEntityKind::CreakingHeart => Box::new(
                crate::behaviour::misc::creaking_heart_entity::CreakingHeartEntity::new(POS),
            ),
            BlockEntityKind::Spawner => {
                Box::new(crate::behaviour::machines::spawner::SpawnerBlockEntity::new(POS))
            }
            BlockEntityKind::Beacon => {
                let mut beacon = crate::behaviour::misc::beacon::BeaconBlockEntity::new(POS);
                beacon.set_data(
                    crate::behaviour::misc::beacon::DATA_PRIMARY,
                    crate::behaviour::misc::beacon::encode_effect(Some(
                        mc_bamboo_data::registries::mob_effect::SPEED,
                    )),
                    None,
                );
                Box::new(beacon)
            }
        };
        let first = write_block_entity_additional(entity.as_ref()).expect("写出成功");
        let second = round_trip(entity, kind.name());
        assert_eq!(first, second, "{:?}", kind);
    }
}

#[test]
fn hopper_cooldown_defaults_to_minus_one_and_absolute_time_is_not_saved() {
    let tag = Tag::Compound(Vec::new());
    let entity = create_block_entity("minecraft:hopper", POS, BlockState::AIR, &tag)
        .unwrap()
        .unwrap();
    let hopper = entity.as_any().downcast_ref::<HopperEntity>().unwrap();
    assert_eq!(hopper.cooldown_time(), -1);
    assert_eq!(hopper.ticked_game_time(), 0);
    assert_eq!(
        field(
            &Tag::Compound(round_trip(
                Box::new(HopperEntity::new(POS)),
                "minecraft:hopper"
            )),
            "TransferCooldown"
        ),
        Some(&Tag::Int(-1))
    );
}

#[test]
fn piston_writes_previous_progress_and_drops_absolute_time() {
    let mut piston = PistonMoving::new(
        POS,
        BlockState(blocks::STONE),
        Direction::South,
        false,
        true,
    );
    piston.set_archive_progress(0.5);
    let written = Tag::Compound(write_block_entity_additional(&piston).unwrap());
    assert_eq!(field(&written, "progress"), Some(&Tag::Float(0.5)));
    assert_eq!(field(&written, "facing"), Some(&Tag::Byte(3)));
    let tag = Tag::Compound(vec![
        (
            "blockState".to_owned(),
            Tag::String("minecraft:stone".to_owned()),
        ),
        ("facing".to_owned(), Tag::Byte(2)),
        ("progress".to_owned(), Tag::Float(0.5)),
        ("extending".to_owned(), Tag::Byte(1)),
        ("source".to_owned(), Tag::Byte(1)),
    ]);
    let rebuilt = create_block_entity("minecraft:piston", POS, BlockState::AIR, &tag)
        .unwrap()
        .unwrap();
    let piston = rebuilt.as_any().downcast_ref::<PistonMoving>().unwrap();
    assert_eq!(piston.observed_facts().4, 0.5f32.to_bits());
    assert_eq!(piston.observed_facts().5, 0.5f32.to_bits());
}

#[test]
fn block_state_codec_handles_properties() {
    let state = BlockState(blocks::OAK_LOG).with(AXIS, mc_bamboo_core::Axis::X);
    let tag = tag::encode_block_state(state);
    assert_eq!(tag::decode_block_state(&tag).unwrap(), state);
    assert_eq!(
        tag::decode_block_state(&Tag::String("minecraft:stone".to_owned())).unwrap(),
        BlockState(blocks::STONE)
    );
}

#[test]
fn saved_ticks_round_trip_and_clamp_large_delays() {
    let mut ticks = ChunkTicks::empty();
    ticks.push_block(super::ticks::SavedEntry::Block {
        pos: POS,
        block: 1,
        priority: TickPriority::High,
        delay: 7,
    });
    ticks.push_block(super::ticks::SavedEntry::Block {
        pos: POS.above(),
        block: 2,
        priority: TickPriority::ExtremelyLow,
        delay: i64::MAX,
    });
    ticks.push_fluid(super::ticks::SavedEntry::Fluid {
        pos: POS,
        fluid: 2,
        priority: TickPriority::Normal,
        delay: -3,
    });
    let lists = encode_chunk_ticks(&ticks).unwrap();
    assert_eq!(lists.clamped, 1);
    assert_eq!(lists.blocks.len(), 2);
    assert_eq!(field(&lists.blocks[1], "t"), Some(&Tag::Int(i32::MAX)));
    let (decoded, problems) = decode_chunk_ticks(
        &Tag::List(lists.blocks.clone()),
        &Tag::List(lists.fluids.clone()),
    )
    .unwrap();
    assert!(problems.is_empty());
    let blocks: Vec<_> = decoded.block_ticks().collect();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].0, POS);
    assert_eq!(blocks[0].2, TickPriority::High);
    assert_eq!(blocks[1].3, i64::from(i32::MAX));
    let fluids: Vec<_> = decoded.fluid_ticks().collect();
    assert_eq!(fluids[0].2, TickPriority::Normal);
    assert_eq!(fluids[0].3, -3);
}

/// 每刻门控的搁置表会把到期但没跑上的刻装进盒子，延迟是负数或 0；
/// 官方 SavedTick.t 本来就允许负值，编码不能把它们丢掉或夹成 0。
#[test]
fn parked_negative_delays_round_trip() {
    let mut ticks = ChunkTicks::empty();
    for delay in [i64::MIN, -100, -1, 0, 1] {
        ticks.push_block(super::ticks::SavedEntry::Block {
            pos: POS,
            block: 1,
            priority: TickPriority::Normal,
            delay,
        });
    }
    let lists = encode_chunk_ticks(&ticks).unwrap();
    assert_eq!(lists.blocks.len(), 5);
    let clamped = lists
        .blocks
        .iter()
        .filter(|tag| matches!(field(tag, "t"), Some(Tag::Int(i32::MIN))))
        .count();
    assert_eq!(clamped, 1);
    let (decoded, problems) =
        decode_chunk_ticks(&Tag::List(lists.blocks), &Tag::List(Vec::new())).unwrap();
    assert!(problems.is_empty());
    let delays: Vec<i64> = decoded.block_ticks().map(|entry| entry.3).collect();
    assert_eq!(delays, [i32::MIN as i64, -100, -1, 0, 1]);
}

#[test]
fn saved_ticks_with_unknown_ids_are_skipped() {
    let mut value = Vec::new();
    put(
        &mut value,
        "i",
        Tag::String("minecraft:not_a_block".to_owned()),
    );
    put(&mut value, "x", Tag::Int(1));
    put(&mut value, "y", Tag::Int(2));
    put(&mut value, "z", Tag::Int(3));
    put(&mut value, "t", Tag::Int(4));
    put(&mut value, "p", Tag::Int(0));
    let (decoded, problems) = decode_chunk_ticks(
        &Tag::List(vec![Tag::Compound(value)]),
        &Tag::List(Vec::new()),
    )
    .unwrap();
    assert_eq!(decoded.block_ticks().count(), 0);
    assert_eq!(problems.len(), 1);
}

fn entity_row(kind: EntityKind, position: Vec3) -> crate::entity::EntityRow {
    let mut entity = Entity::new(kind, position);
    entity.id = 17;
    entity.delta = Vec3::new(0.1, -0.2, 0.3);
    entity.on_ground = true;
    entity.fall_distance = 4.5;
    entity.remaining_fire_ticks = 12;
    entity.split()
}

#[test]
fn entity_uuid_matches_the_network_layer() {
    let uuid = entity_uuid_tag(17);
    // UUIDUtil.CODEC 是 [most>>32, most, least>>32, least]；
    // 0x0000_BAAB 在高 64 位的低半部分。
    assert_eq!(uuid, Tag::IntArray(vec![0x0000_baab, 0, 0, 17]));
}

#[test]
fn three_entity_kinds_round_trip() {
    let cases = [
        EntityKind::FallingBlock(FallingBlock::new(BlockState(blocks::SAND), POS)),
        EntityKind::Tnt(PrimedTnt::new()),
        EntityKind::Item(GroundItem {
            stack: ItemStack::new(STONE, 7),
            age: 123,
            pickup_delay: 10,
            health: 5,
            bob_offs: 0.0,
            roll_velocity: false,
            target: Some([7u8; 16]),
            item_destroy_pending: false,
        }),
        EntityKind::OminousItemSpawner(Box::new(
            crate::entity::ominous_item_spawner::OminousItemSpawner {
                item: ItemStack::new(WIND_CHARGE, 2),
                spawn_item_after_ticks: 87,
            },
        )),
        EntityKind::EyeOfEnder(Box::new(crate::entity::eye_of_ender::EyeOfEnder {
            uuid: [3u8; 16],
            item: crate::entity::eye_of_ender::default_item(),
            target: None,
            life: 12,
            survive_after_death: true,
        })),
    ];
    for kind in cases {
        let row = entity_row(kind, Vec3::new(1.5, 64.25, -2.0));
        let tag = write_entity(&row).unwrap();
        let rebuilt = read_entity(&tag, 99).unwrap().unwrap();
        assert_eq!(rebuilt.public.position, row.public.position);
        assert_eq!(rebuilt.public.delta, row.public.delta);
        assert!(rebuilt.public.on_ground);
        assert_eq!(rebuilt.state.fall_distance, 4.5);
        assert_eq!(rebuilt.state.remaining_fire_ticks, 12);
        let again = write_entity(&rebuilt).unwrap();
        // 读回时 ID 由生成流程重新分配，UUID 不同；其余键一致。
        let normalize = |tag: Tag| match tag {
            Tag::Compound(mut values) => {
                values.retain(|(key, _)| key != "UUID");
                Tag::Compound(values)
            }
            other => other,
        };
        assert_eq!(normalize(tag), normalize(again));
    }
}

/// `Passengers` 嵌套在载具的存档里，乘客不单独成行；读回来两边关系一致，
/// 乘客自己的 `Pos` 用载具的 x/z 加自己的 y。
///
/// Passengers nest inside their vehicle's tag instead of becoming rows of their
/// own; the links come back and a passenger's `Pos` uses the vehicle's x/z.
#[test]
fn riders_are_saved_inside_their_vehicle() {
    use std::collections::BTreeMap;

    use crate::entity::{KindPublic, mob::MobType};

    let root_id = 1 << 24;
    let mut chicken = Entity::by_mob_type(MobType::Chicken, Vec3::new(4.5, 64.0, -3.5));
    chicken.id = root_id;
    let mut zombie = Entity::by_mob_type(MobType::Zombie, Vec3::new(9.0, 64.0, 9.0));
    zombie.id = root_id + 1;
    zombie.vehicle = Some(root_id);
    chicken.with_mob(|_, mob| {
        crate::entity::riding::add_passenger(&mut mob.passengers, zombie.id, false);
    });
    let rows: BTreeMap<i32, crate::entity::EntityRow> = [chicken.split(), zombie.split()]
        .into_iter()
        .map(|row| (row.public.uid, row))
        .collect();
    let lookup = |uid: i32| rows.get(&uid).cloned();
    let tag = write_entity_tree(&rows[&root_id], &lookup).unwrap();
    let passenger = match field(&tag, "Passengers") {
        Some(Tag::List(list)) => list.clone(),
        other => panic!("no Passengers list: {other:?}"),
    };
    assert_eq!(passenger.len(), 1);
    assert_eq!(
        field(&passenger[0], "id"),
        Some(&Tag::String("minecraft:zombie".to_owned()))
    );
    assert_eq!(
        field(&passenger[0], "Pos"),
        Some(&Tag::List(vec![
            Tag::Double(4.5),
            Tag::Double(64.0),
            Tag::Double(-3.5),
        ]))
    );

    let mut next = root_id;
    let tree = read_entity_tree(&tag, &mut next).unwrap().unwrap();
    assert_eq!(tree.len(), 2);
    assert_eq!(tree[0].public.vehicle, None);
    let KindPublic::Mob(mob) = &tree[0].public.kind else {
        panic!("chicken is a mob");
    };
    assert_eq!(mob.passengers, vec![tree[1].public.uid]);
    assert_eq!(tree[1].public.vehicle, Some(tree[0].public.uid));

    // 读回来再写一遍：乘客仍然嵌在载具里，位置和 id 不变。
    let rebuilt: BTreeMap<i32, crate::entity::EntityRow> =
        tree.into_iter().map(|row| (row.public.uid, row)).collect();
    let uid = next - 2;
    let again = write_entity_tree(&rebuilt[&uid], &|id| rebuilt.get(&id).cloned()).unwrap();
    let passenger = match field(&again, "Passengers") {
        Some(Tag::List(list)) => list.clone(),
        other => panic!("no Passengers list after a reload: {other:?}"),
    };
    assert_eq!(passenger.len(), 1);
    assert_eq!(
        field(&passenger[0], "id"),
        Some(&Tag::String("minecraft:zombie".to_owned()))
    );
    assert_eq!(
        field(&passenger[0], "Pos"),
        Some(&Tag::List(vec![
            Tag::Double(4.5),
            Tag::Double(64.0),
            Tag::Double(-3.5),
        ]))
    );
}

#[test]
fn unknown_entity_ids_are_left_to_the_caller() {
    // 官方注册表里存在但 Bamboo 没有建模的类型。
    // A real registry id for an entity Bamboo does not model.
    let tag = Tag::Compound(vec![(
        "id".to_owned(),
        Tag::String("minecraft:blaze".to_owned()),
    )]);
    assert!(read_entity(&tag, 1).unwrap().is_none());
    assert_eq!(
        field(&tag, "id"),
        Some(&Tag::String("minecraft:blaze".to_owned()))
    );
}

/// 牛是已经建模的实体：读盘进实体表，写盘时按官方的键写回。
///
/// Cows are modelled now: a disk entry becomes a live entity and is written
/// back with the vanilla keys.
#[test]
fn cow_entity_ids_are_read_and_written() {
    let tag = Tag::Compound(vec![
        ("id".to_owned(), Tag::String("minecraft:cow".to_owned())),
        (
            "Pos".to_owned(),
            Tag::List(vec![
                Tag::Double(1.5),
                Tag::Double(64.0),
                Tag::Double(-2.25),
            ]),
        ),
        ("UUID".to_owned(), Tag::IntArray(vec![1, 2, 3, 4])),
    ]);
    let row = read_entity(&tag, 1).unwrap().expect("牛能读进来");
    assert_eq!(row.public.position, Vec3::new(1.5, 64.0, -2.25));
    assert!(matches!(row.public.kind, crate::entity::KindPublic::Mob(_)));
    let again = write_entity(&row).unwrap();
    assert_eq!(
        field(&again, "id"),
        Some(&Tag::String("minecraft:cow".to_owned()))
    );
    assert_eq!(
        field(&again, "UUID"),
        Some(&Tag::IntArray(vec![1, 2, 3, 4]))
    );
}

#[test]
fn items_in_block_entities_are_slotted() {
    let mut fire = CampfireEntity::new(POS);
    fire.set_item(2, ItemStack::new(STONE, 4));
    let tag = Tag::Compound(write_block_entity_additional(&fire).unwrap());
    let Tag::List(items) = field(&tag, "Items").unwrap() else {
        panic!("campfire did not write an Items list");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(field(&items[0], "Slot"), Some(&Tag::Byte(2)));
    assert_eq!(
        field(&items[0], "id"),
        Some(&Tag::String("minecraft:stone".to_owned()))
    );
    assert_eq!(field(&items[0], "count"), Some(&Tag::Int(4)));
}

#[test]
fn furnace_recipes_used_is_a_compound() {
    let mut furnace = FurnaceEntity::new(POS, mc_bamboo_data::CookingKind::Smelting);
    furnace.set_archive_values(1.5, vec![("minecraft:iron_ingot", 3)]);
    let tag = Tag::Compound(write_block_entity_additional(&furnace).unwrap());
    assert_eq!(field(&tag, "speed_multiplier"), Some(&Tag::Float(1.5)));
    assert_eq!(entries(&tag, "RecipesUsed").len(), 1);
}

/// 漏斗 `tickedGameTime` 改成相对量之后，跨刻、跨装盒、读盘都和官方的
/// 「每刻写世界时间 / 读盘回来是 0」完全一致。
#[test]
fn hopper_ticked_game_time_stays_relative_across_ticks_and_archive() {
    use crate::block_entity::set_block_entity_clock;
    use crate::hopper::push_items_tick;
    use mc_bamboo_core::BlockPos;

    let pos = BlockPos::new(1, 64, 2);
    let mut hopper = HopperEntity::new(pos);
    for now in 1..=12 {
        set_block_entity_clock(now);
        // `pushItemsTick` 只需要 `World` 来送 / 吸物品；这里直接验时间字段，
        // 用一个没有容器的假世界，冷却路径不会碰任何容器。
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::new();
        ticks.set_time(now);
        let mut world = crate::World::new(&mut level, &mut ticks);
        world.set_block(
            pos,
            BlockState(mc_bamboo_data::blocks::HOPPER),
            crate::world::UPDATE_ALL,
        );
        push_items_tick(&mut world, pos, &mut hopper);
        assert_eq!(hopper.ticked_game_time(), now, "tick {now}");
        assert_eq!(hopper.ticked_game_time_age(), Some(0), "tick {now}");
        // 官方同一刻的字段等于世界时间；相对量在后面几刻会按公式增长。
    }
    // 官方安静刻里每刻都真跑过（时间字段跟世界时间）。休眠期间由公式现算，
    // 不落到字段里；这里显式推进到 15 再核对还原值。
    set_block_entity_clock(15);
    let mut level = crate::testing::TestLevel::new();
    let mut ticks = crate::BlockTicks::new();
    ticks.set_time(15);
    let mut world = crate::World::new(&mut level, &mut ticks);
    world.set_block(
        pos,
        BlockState(mc_bamboo_data::blocks::HOPPER),
        crate::world::UPDATE_ALL,
    );
    push_items_tick(&mut world, pos, &mut hopper);
    assert_eq!(hopper.ticked_game_time_age(), Some(0));
    assert_eq!(hopper.ticked_game_time(), 15);

    let tag = Tag::Compound(write_block_entity_additional(&hopper).unwrap());
    let rebuilt = create_block_entity("minecraft:hopper", pos, BlockState::AIR, &tag)
        .unwrap()
        .unwrap();
    let rebuilt = rebuilt.as_any().downcast_ref::<HopperEntity>().unwrap();
    assert_eq!(rebuilt.ticked_game_time(), 0, "官方不存这个字段");
    assert_eq!(rebuilt.ticked_game_time_age(), None);
    assert_eq!(field(&tag, "TransferCooldown"), Some(&Tag::Int(0)));
}

/// 四种只保留存档态的方块实体：只有 `id` / 坐标的条目也写全官方构造器的默认键，
/// 写 → 读 → 写不变；类型不会串。
#[test]
fn passive_block_entities_round_trip_their_default_keys() {
    use super::passive::PassiveKind;

    for kind in PassiveKind::ALL {
        let empty = Tag::Compound(Vec::new());
        let entity = create_block_entity(kind.name(), POS, BlockState::AIR, &empty)
            .expect("读回成功")
            .unwrap_or_else(|| panic!("{} 没有构造分支", kind.name()));
        let first = write_block_entity_additional(entity.as_ref()).expect("写出成功");
        assert_eq!(
            first,
            super::default_additional(kind.name()).expect("有默认键"),
            "{kind:?}"
        );
        let rebuilt = create_block_entity(
            kind.name(),
            POS,
            BlockState::AIR,
            &Tag::Compound(first.clone()),
        )
        .expect("再读成功")
        .unwrap_or_else(|| panic!("{} 再读没有构造分支", kind.name()));
        assert_eq!(
            write_block_entity_additional(rebuilt.as_ref()).expect("再写成功"),
            first,
            "{kind:?}"
        );
    }
    assert_eq!(
        super::default_additional("minecraft:vault"),
        Some(vec![
            (
                "config".to_owned(),
                Tag::Compound(vec![(
                    "key_item".to_owned(),
                    Tag::Compound(vec![
                        (
                            "id".to_owned(),
                            Tag::String("minecraft:trial_key".to_owned()),
                        ),
                        ("count".to_owned(), Tag::Int(1)),
                    ]),
                )]),
            ),
            ("shared_data".to_owned(), Tag::Compound(Vec::new())),
            ("server_data".to_owned(), Tag::Compound(Vec::new())),
        ])
    );
    for id in [
        "minecraft:sculk_sensor",
        "minecraft:calibrated_sculk_sensor",
    ] {
        assert_eq!(
            super::default_additional(id),
            Some(vec![
                ("last_vibration_frequency".to_owned(), Tag::Int(0)),
                (
                    "listener".to_owned(),
                    Tag::Compound(vec![
                        (
                            "selector".to_owned(),
                            Tag::Compound(vec![("tick".to_owned(), Tag::Long(-1))]),
                        ),
                        ("event_delay".to_owned(), Tag::Int(0)),
                    ]),
                ),
            ])
        );
    }
    assert_eq!(
        super::default_additional("minecraft:sculk_shrieker"),
        Some(vec![
            ("warning_level".to_owned(), Tag::Int(0)),
            (
                "listener".to_owned(),
                Tag::Compound(vec![
                    (
                        "selector".to_owned(),
                        Tag::Compound(vec![("tick".to_owned(), Tag::Long(-1))]),
                    ),
                    ("event_delay".to_owned(), Tag::Int(0)),
                ]),
            ),
        ])
    );
    assert_eq!(super::default_additional("minecraft:chest"), None);
}

/// `VaultConfig.CODEC` 的默认值省略、非默认值保留；`VaultServerData.set` 不搬
/// `totalEjectionsNeeded`，空列表和计数都被省掉。
#[test]
fn vault_codec_omits_defaults_and_keeps_non_defaults() {
    let uuid = Tag::IntArray(vec![1, 2, 3, 4]);
    let config = Tag::Compound(vec![
        (
            "loot_table".to_owned(),
            Tag::String("minecraft:chests/trial_chambers/reward".to_owned()),
        ),
        ("activation_range".to_owned(), Tag::Double(3.0)),
        ("deactivation_range".to_owned(), Tag::Double(4.5)),
        (
            "override_loot_table_to_display".to_owned(),
            Tag::String("minecraft:chests/trial_chambers/reward_ominous".to_owned()),
        ),
    ]);
    let shared = Tag::Compound(vec![
        (
            "connected_players".to_owned(),
            Tag::List(vec![uuid.clone()]),
        ),
        ("connected_particles_range".to_owned(), Tag::Double(4.5)),
    ]);
    let server = Tag::Compound(vec![
        ("state_updating_resumes_at".to_owned(), Tag::Long(123)),
        ("items_to_eject".to_owned(), Tag::List(Vec::new())),
        ("total_ejections_needed".to_owned(), Tag::Int(7)),
    ]);
    let tag = Tag::Compound(vec![
        ("config".to_owned(), config),
        ("shared_data".to_owned(), shared),
        ("server_data".to_owned(), server),
    ]);
    let entity = create_block_entity("minecraft:vault", POS, BlockState::AIR, &tag)
        .unwrap()
        .unwrap();
    let written = Tag::Compound(write_block_entity_additional(entity.as_ref()).unwrap());
    assert_eq!(
        field(&written, "config"),
        Some(&Tag::Compound(vec![
            ("activation_range".to_owned(), Tag::Double(3.0)),
            (
                "override_loot_table_to_display".to_owned(),
                Tag::String("minecraft:chests/trial_chambers/reward_ominous".to_owned()),
            ),
        ]))
    );
    assert_eq!(
        field(&written, "shared_data"),
        Some(&Tag::Compound(vec![(
            "connected_players".to_owned(),
            Tag::List(vec![uuid]),
        )]))
    );
    assert_eq!(
        field(&written, "server_data"),
        Some(&Tag::Compound(vec![(
            "state_updating_resumes_at".to_owned(),
            Tag::Long(123)
        ),]))
    );
    // 再读再写仍逐键相同。
    let rebuilt = create_block_entity("minecraft:vault", POS, BlockState::AIR, &written)
        .unwrap()
        .unwrap();
    assert_eq!(
        Tag::Compound(write_block_entity_additional(rebuilt.as_ref()).unwrap()),
        written
    );
}

/// `VibrationSystem.Data.CODEC`：`event_delay` 永远写、`selector.tick` 必写，
/// `event` 可省；坏条目让整个监听器回到新建默认值。
#[test]
fn vibration_listener_always_writes_its_required_keys() {
    let tag = Tag::Compound(vec![
        ("last_vibration_frequency".to_owned(), Tag::Int(3)),
        (
            "listener".to_owned(),
            Tag::Compound(vec![(
                "selector".to_owned(),
                Tag::Compound(vec![("tick".to_owned(), Tag::Long(42))]),
            )]),
        ),
    ]);
    let entity = create_block_entity("minecraft:sculk_sensor", POS, BlockState::AIR, &tag)
        .unwrap()
        .unwrap();
    let written = Tag::Compound(write_block_entity_additional(entity.as_ref()).unwrap());
    assert_eq!(
        field(&written, "last_vibration_frequency"),
        Some(&Tag::Int(3))
    );
    assert_eq!(
        field(&written, "listener"),
        Some(&Tag::Compound(vec![
            (
                "selector".to_owned(),
                Tag::Compound(vec![("tick".to_owned(), Tag::Long(-1))]),
            ),
            ("event_delay".to_owned(), Tag::Int(0)),
        ])),
        "`VibrationSelector.CODEC` 没有 `event` 时把 tick 折成 -1L"
    );
    let broken = Tag::Compound(vec![(
        "listener".to_owned(),
        Tag::Compound(vec![("event_delay".to_owned(), Tag::Int(-1))]),
    )]);
    let rebuilt = create_block_entity("minecraft:sculk_sensor", POS, BlockState::AIR, &broken)
        .unwrap()
        .unwrap();
    let default = Tag::Compound(write_block_entity_additional(rebuilt.as_ref()).unwrap());
    assert_eq!(
        field(&default, "listener"),
        super::default_additional("minecraft:sculk_sensor")
            .unwrap()
            .last()
            .map(|(_, value)| value)
    );
}

/// `GameEvent.CODEC` 是注册表 holder codec：语法合法但没注册过的 `game_event`
/// 解不开，整个 `VibrationInfo` 回到 `lenientOptionalFieldOf` 的缺省（不写 event）。
#[test]
fn vibration_events_are_looked_up_in_the_game_event_registry() {
    let selector = |event: &str| {
        Tag::Compound(vec![
            (
                "event".to_owned(),
                Tag::Compound(vec![
                    ("game_event".to_owned(), Tag::String(event.to_owned())),
                    ("distance".to_owned(), Tag::Float(2.0)),
                    (
                        "pos".to_owned(),
                        Tag::List(vec![Tag::Double(1.0), Tag::Double(2.0), Tag::Double(3.0)]),
                    ),
                ]),
            ),
            ("tick".to_owned(), Tag::Long(9)),
        ])
    };
    let tag = Tag::Compound(vec![(
        "listener".to_owned(),
        Tag::Compound(vec![
            ("selector".to_owned(), selector("minecraft:step")),
            ("event_delay".to_owned(), Tag::Int(0)),
        ]),
    )]);
    let entity = create_block_entity("minecraft:sculk_sensor", POS, BlockState::AIR, &tag)
        .unwrap()
        .unwrap();
    let written = Tag::Compound(write_block_entity_additional(entity.as_ref()).unwrap());
    let listener = Tag::Compound(
        field(&written, "listener")
            .and_then(|tag| match tag {
                Tag::Compound(values) => Some(values.clone()),
                _ => None,
            })
            .expect("listener is a compound"),
    );
    let selector_written = Tag::Compound(
        field(&listener, "selector")
            .and_then(|tag| match tag {
                Tag::Compound(values) => Some(values.clone()),
                _ => None,
            })
            .expect("selector is a compound"),
    );
    assert!(
        field(&selector_written, "event").is_some(),
        "a registered game event survives the round trip"
    );

    let unknown = Tag::Compound(vec![(
        "listener".to_owned(),
        Tag::Compound(vec![
            (
                "selector".to_owned(),
                selector("minecraft:not_a_real_event"),
            ),
            ("event_delay".to_owned(), Tag::Int(0)),
        ]),
    )]);
    let rebuilt = create_block_entity("minecraft:sculk_sensor", POS, BlockState::AIR, &unknown)
        .unwrap()
        .unwrap();
    let written = Tag::Compound(write_block_entity_additional(rebuilt.as_ref()).unwrap());
    let listener = Tag::Compound(
        field(&written, "listener")
            .and_then(|tag| match tag {
                Tag::Compound(values) => Some(values.clone()),
                _ => None,
            })
            .expect("listener is a compound"),
    );
    let selector_written = Tag::Compound(
        field(&listener, "selector")
            .and_then(|tag| match tag {
                Tag::Compound(values) => Some(values.clone()),
                _ => None,
            })
            .expect("selector is a compound"),
    );
    assert!(
        field(&selector_written, "event").is_none(),
        "an unregistered game event is dropped like vanilla's lenient codec"
    );
    assert_eq!(field(&selector_written, "tick"), Some(&Tag::Long(-1)));
}

/// 读盘的牛不是静止的展品：进实体表后每刻继续跑 AI、年龄和求爱计时。
#[test]
fn a_loaded_cow_keeps_running_from_its_saved_state() {
    use crate::{
        BlockTicks, World,
        entity::{ChunkEntities, EntityKind},
        testing::TestLevel,
    };

    let cow = |age: i32, love: i32, x: f64| {
        Tag::Compound(vec![
            ("id".to_owned(), Tag::String("minecraft:cow".to_owned())),
            (
                "Pos".to_owned(),
                Tag::List(vec![Tag::Double(x), Tag::Double(64.0), Tag::Double(-2.25)]),
            ),
            ("UUID".to_owned(), Tag::IntArray(vec![1, 2, 3, age + 9])),
            ("Age".to_owned(), Tag::Int(age)),
            ("InLove".to_owned(), Tag::Int(love)),
            (
                "Rotation".to_owned(),
                Tag::List(vec![Tag::Float(45.0), Tag::Float(0.0)]),
            ),
        ])
    };
    // 一头成年牛在求爱，一头幼年牛在长大；两者都要接着跑。
    let adult = read_entity(&cow(0, 500, 1.5), 99)
        .unwrap()
        .expect("成年牛能读进来");
    let baby = read_entity(&cow(-24000, 0, 5.5), 98)
        .unwrap()
        .expect("幼年牛能读进来");
    let mut level = TestLevel::new();
    level
        .entities_mut_for_test()
        .insert_chunk(ChunkEntities::from_parts(vec![adult, baby], Vec::new()));
    let ids = level.entities().ids();
    let (adult_id, baby_id) = (ids[0], ids[1]);
    let adult_before = level.entities().get(adult_id).expect("成年牛在表里");
    let baby_before = level.entities().get(baby_id).expect("幼年牛在表里");
    let EntityKind::Mob(adult_mob) = &adult_before.kind else {
        panic!("读进来的不是生物");
    };
    assert_eq!(adult_mob.in_love, 500);
    assert_eq!(adult_mob.age, 0);
    let EntityKind::Mob(baby_mob) = &baby_before.kind else {
        panic!("读进来的不是生物");
    };
    assert_eq!(baby_mob.age, -24000);
    // `Age != 0` 时官方把求爱计时清零，幼年牛的 InLove 本来就是 0。
    let adult_start = adult_before.position;

    let mut ticks = BlockTicks::new();
    for _ in 0..8 {
        let mut world = World::new(&mut level, &mut ticks);
        world.tick_entities();
    }
    let adult_after = level.entities().get(adult_id).expect("成年牛还活着");
    let baby_after = level.entities().get(baby_id).expect("幼年牛还活着");
    let EntityKind::Mob(adult_mob) = &adult_after.kind else {
        panic!("tick 之后还是生物");
    };
    let EntityKind::Mob(baby_mob) = &baby_after.kind else {
        panic!("tick 之后还是生物");
    };
    // 求爱计时每刻减一，幼年年龄每刻加一，位置被 AI 推着走。
    assert_eq!(adult_mob.in_love, 500 - 8);
    assert_eq!(baby_mob.age, -24000 + 8);
    assert!(adult_after.tick_count > 0);
    assert_ne!(adult_after.position, adult_start);
}

/// 风弹的官方存档键（Projectile + AbstractHurtingProjectile）写 → 读 → 写不变，
/// 两种注册表类型分别走 `minecraft:wind_charge` / `minecraft:breeze_wind_charge`。
#[test]
fn wind_charge_save_round_trip_keeps_variant_and_owner() {
    for variant in [
        crate::entity::wind_charge::WindChargeVariant::Player,
        crate::entity::wind_charge::WindChargeVariant::Breeze,
    ] {
        let mut charge = crate::entity::wind_charge::WindCharge::new(variant);
        charge.uuid = [7_u8; 16];
        charge.owner_uuid = Some([9_u8; 16]);
        charge.left_owner = true;
        charge.has_been_shot = true;
        charge.pitch = 12.5;
        let row = entity_row(
            EntityKind::WindCharge(Box::new(charge)),
            Vec3::new(1.5, 70.25, -2.0),
        );
        let tag = write_entity(&row).unwrap();
        let rebuilt = read_entity(&tag, 99).unwrap().unwrap();
        let again = write_entity(&rebuilt).unwrap();
        let normalize = |tag: Tag| match tag {
            Tag::Compound(mut values) => {
                values.retain(|(key, _)| key != "UUID");
                Tag::Compound(values)
            }
            other => other,
        };
        assert_eq!(normalize(tag), normalize(again));
        let restored = rebuilt.clone().into_entity();
        if let EntityKind::WindCharge(charge) = &restored.kind {
            assert_eq!(charge.variant, variant);
            assert!(charge.left_owner && charge.has_been_shot);
            assert_eq!(charge.pitch, 12.5);
        } else {
            panic!("wind charge round trip lost its kind");
        }
    }
}

/// 旋风人走通用生物存档，额外字段（Pose 之类客户端状态）不写。
#[test]
fn breeze_mob_save_round_trip_keeps_pose_default() {
    let mut entity = crate::entity::Entity::by_mob_type(
        crate::entity::mob::MobType::Breeze,
        Vec3::new(2.5, 64.0, 0.5),
    );
    entity.with_mob(|_, mob| mob.breeze.pose = crate::entity::mob::breeze::Pose::Shooting);
    let row = entity.split();
    let tag = write_entity(&row).unwrap();
    let rebuilt = read_entity(&tag, 99).unwrap().unwrap();
    let again = write_entity(&rebuilt).unwrap();
    let normalize = |tag: Tag| match tag {
        Tag::Compound(mut values) => {
            values.retain(|(key, _)| key != "UUID");
            Tag::Compound(values)
        }
        other => other,
    };
    assert_eq!(normalize(tag), normalize(again));
}

/// 运输船的物品栏存档往返：27 格里非零的槽位、物品和数量都回来。
#[test]
fn chest_boat_save_round_trip_keeps_items() {
    let mut entity = Entity::boat(
        mc_bamboo_data::registries::entity_type::OAK_CHEST_BOAT,
        Vec3::new(1.5, 63.0, -2.0),
    );
    if let EntityKind::Boat(boat) = &mut entity.kind {
        let chest = boat.chest.as_mut().unwrap();
        chest.items[0] = ItemStack::new(STONE, 3);
        chest.items[13] = ItemStack::new(WIND_CHARGE, 7);
    } else {
        panic!("boat constructor lost its kind");
    }
    let row = entity.split();
    let tag = write_entity(&row).unwrap();
    let rebuilt = read_entity(&tag, 99).unwrap().unwrap();
    let again = write_entity(&rebuilt).unwrap();
    let normalize = |tag: Tag| match tag {
        Tag::Compound(mut values) => {
            values.retain(|(key, _)| key != "UUID");
            Tag::Compound(values)
        }
        other => other,
    };
    assert_eq!(normalize(tag), normalize(again));
    let restored = rebuilt.into_entity();
    if let EntityKind::Boat(boat) = &restored.kind {
        let chest = boat.chest.as_ref().unwrap();
        assert_eq!(chest.items[0], ItemStack::new(STONE, 3));
        assert_eq!(chest.items[13], ItemStack::new(WIND_CHARGE, 7));
        assert!(chest.items[1..13].iter().all(ItemStack::is_empty));
        assert!(chest.loot_table.is_none());
    } else {
        panic!("boat round trip lost its kind");
    }
}

/// 运输船有战利品表时只写 LootTable / LootTableSeed，不写 Items。
#[test]
fn chest_boat_save_round_trip_keeps_loot_table() {
    let mut entity = Entity::boat(
        mc_bamboo_data::registries::entity_type::SPRUCE_CHEST_BOAT,
        Vec3::new(-4.0, 62.5, 8.0),
    );
    if let EntityKind::Boat(boat) = &mut entity.kind {
        let chest = boat.chest.as_mut().unwrap();
        chest.loot_table = Some("minecraft:chests/shipwreck_supply".to_owned());
        chest.loot_table_seed = 123_456_789;
    }
    let tag = write_entity(&entity.split()).unwrap();
    assert!(field(&tag, "Items").is_none());
    let rebuilt = read_entity(&tag, 99).unwrap().unwrap();
    let restored = rebuilt.into_entity();
    if let EntityKind::Boat(boat) = &restored.kind {
        let chest = boat.chest.as_ref().unwrap();
        assert_eq!(
            chest.loot_table.as_deref(),
            Some("minecraft:chests/shipwreck_supply")
        );
        assert_eq!(chest.loot_table_seed, 123_456_789);
    } else {
        panic!("boat round trip lost its kind");
    }
}

/// 船载着动物乘客时 `Passengers` 往返：船的乘客名单和乘客的 `vehicle` 都回来。
#[test]
fn boat_passenger_save_round_trip() {
    use std::collections::BTreeMap;

    let root_id = crate::entity::FIRST_ENTITY_ID;
    let mut boat = Entity::boat(
        mc_bamboo_data::registries::entity_type::OAK_BOAT,
        Vec3::new(4.5, 63.0, -3.5),
    );
    boat.id = root_id;
    let mut cow = Entity::by_mob_type(crate::entity::mob::MobType::Cow, Vec3::new(9.0, 64.0, 9.0));
    cow.id = root_id + 1;
    cow.vehicle = Some(root_id);
    if let EntityKind::Boat(state) = &mut boat.kind {
        crate::entity::riding::add_passenger(&mut state.passengers, cow.id, false);
    }
    let rows: BTreeMap<i32, crate::entity::EntityRow> = [boat.split(), cow.split()]
        .into_iter()
        .map(|row| (row.public.uid, row))
        .collect();
    let lookup = |uid: i32| rows.get(&uid).cloned();
    let tag = write_entity_tree(&rows[&root_id], &lookup).unwrap();
    let passenger = match field(&tag, "Passengers") {
        Some(Tag::List(list)) => list.clone(),
        other => panic!("no Passengers list: {other:?}"),
    };
    assert_eq!(passenger.len(), 1);
    assert_eq!(
        field(&passenger[0], "id"),
        Some(&Tag::String("minecraft:cow".to_owned()))
    );

    let mut next = root_id;
    let tree = read_entity_tree(&tag, &mut next).unwrap().unwrap();
    assert_eq!(tree.len(), 2);
    let crate::entity::KindPublic::Boat(state) = &tree[0].public.kind else {
        panic!("root is a boat");
    };
    assert_eq!(state.passengers, vec![tree[1].public.uid]);
    assert_eq!(tree[1].public.vehicle, Some(tree[0].public.uid));
}
