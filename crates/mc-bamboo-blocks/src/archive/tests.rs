//! 存档编解码的单元测试：每种方块实体写 → 按 id 读 → 再写，计划刻和实体往返回。

use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{blocks, items::STONE};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;

use super::{
    block_entity::{BlockEntityKind, create_block_entity, write_block_entity_additional},
    entity::{entity_uuid_tag, read_entity, write_entity},
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
        }),
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

#[test]
fn unknown_entity_ids_are_left_to_the_caller() {
    let tag = Tag::Compound(vec![(
        "id".to_owned(),
        Tag::String("minecraft:cow".to_owned()),
    )]);
    assert!(read_entity(&tag, 1).unwrap().is_none());
    assert_eq!(
        field(&tag, "id"),
        Some(&Tag::String("minecraft:cow".to_owned()))
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
    }
    // 读一读「下一刻」：age 变大，还原出来的绝对时刻按官方仍等于现在。
    set_block_entity_clock(15);
    assert_eq!(hopper.ticked_game_time_age(), Some(3));
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
