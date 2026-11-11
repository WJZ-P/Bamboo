//! 方块实体存档编解码的单元测试：每种写 → 按 id 读 → 再写不变。

use mc_bamboo_core::{Axis, BlockPos, Direction};
use mc_bamboo_data::{blocks, items::STONE};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;

use super::{
    block_entity::{BlockEntityKind, create_block_entity, write_block_entity_additional},
    tag::{self, field},
};
use crate::{
    BlockEntity, BlockState,
    behaviour::machines::{
        BrewingStandEntity, CampfireEntity, FurnaceEntity, PistonMoving, ShulkerBoxEntity,
    },
    hopper::HopperEntity,
    state::AXIS,
};

const POS: BlockPos = BlockPos::new(3, 64, 5);

fn round_trip(entity: Box<dyn BlockEntity>, id: &str) -> Vec<(String, Tag)> {
    let first = write_block_entity_additional(entity.as_ref()).expect("写出成功");
    let tag = Tag::Compound(first);
    let rebuilt = create_block_entity(id, POS, BlockState::AIR, &tag)
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
    assert_eq!(piston.observed_facts(0).4, 0.5f32.to_bits());
    assert_eq!(piston.observed_facts(0).5, 0.5f32.to_bits());
    assert_eq!(piston.observed_facts(0).6, 0);
}

#[test]
fn block_state_codec_handles_properties() {
    let state = BlockState(blocks::OAK_LOG).with(AXIS, Axis::X);
    let tag = tag::encode_block_state(state);
    assert_eq!(tag::decode_block_state(&tag).unwrap(), state);
    assert_eq!(
        tag::decode_block_state(&Tag::String("minecraft:stone".to_owned())).unwrap(),
        BlockState(blocks::STONE)
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
    assert!(matches!(field(&tag, "RecipesUsed"), Some(Tag::Compound(values)) if values.len() == 1));
}
