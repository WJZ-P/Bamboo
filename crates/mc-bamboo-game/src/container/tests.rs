use mc_bamboo_blocks::{
    BlockState, ShulkerStatus,
    state::{CHEST_TYPE, ChestType, FACING, HORIZONTAL_FACING},
};
use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{blocks, registries::menu};

use super::*;

#[test]
fn every_container_block_has_a_kind() {
    let kinds = [
        (blocks::CHEST, ContainerKind::Chest),
        (blocks::TRAPPED_CHEST, ContainerKind::Chest),
        (blocks::COPPER_CHEST, ContainerKind::Chest),
        (blocks::BARREL, ContainerKind::Barrel),
        (blocks::ENDER_CHEST, ContainerKind::EnderChest),
        (blocks::SHULKER_BOX, ContainerKind::ShulkerBox),
        (blocks::WHITE_SHULKER_BOX, ContainerKind::ShulkerBox),
        (blocks::HOPPER, ContainerKind::Hopper),
        (blocks::DISPENSER, ContainerKind::Dispenser),
        (blocks::DROPPER, ContainerKind::Dropper),
        (blocks::CHISELED_BOOKSHELF, ContainerKind::ChiseledBookshelf),
        (blocks::JUKEBOX, ContainerKind::Jukebox),
        (blocks::DECORATED_POT, ContainerKind::DecoratedPot),
        (blocks::CRAFTER, ContainerKind::Crafter),
        (blocks::OAK_SHELF, ContainerKind::Shelf),
    ];
    for (block, kind) in kinds {
        assert_eq!(kind_of(BlockState(block)), Some(kind));
    }
    assert_eq!(kind_of(BlockState(blocks::STONE)), None);
    assert_eq!(kind_of(BlockState(blocks::FURNACE)), None);
}

#[test]
fn kinds_carry_the_official_sizes_and_menu_types() {
    assert_eq!(ContainerKind::Chest.slots(), 27);
    assert_eq!(ContainerKind::Chest.menu_type(), menu::GENERIC_9X3);
    assert_eq!(ContainerKind::Chest.title(), "container.chest");
    assert_eq!(ContainerKind::Barrel.title(), "container.barrel");
    assert_eq!(ContainerKind::EnderChest.slots(), 0);
    assert_eq!(ContainerKind::EnderChest.menu_type(), menu::GENERIC_9X3);
    assert_eq!(ContainerKind::EnderChest.title(), "container.enderchest");
    assert_eq!(ContainerKind::ShulkerBox.slots(), 27);
    assert_eq!(ContainerKind::ShulkerBox.menu_type(), menu::SHULKER_BOX);
    assert_eq!(ContainerKind::ShulkerBox.title(), "container.shulkerBox");
    assert_eq!(ContainerKind::Hopper.slots(), 5);
    assert_eq!(ContainerKind::Hopper.menu_type(), menu::HOPPER);
    assert_eq!(ContainerKind::Hopper.title(), "container.hopper");
    assert_eq!(ContainerKind::Dispenser.slots(), 9);
    assert_eq!(ContainerKind::Dispenser.menu_type(), menu::GENERIC_3X3);
    assert_eq!(ContainerKind::Dispenser.title(), "container.dispenser");
    assert_eq!(ContainerKind::Dropper.title(), "container.dropper");
    assert_eq!(ContainerKind::ChiseledBookshelf.slots(), 6);
    assert_eq!(ContainerKind::Jukebox.slots(), 1);
    assert_eq!(ContainerKind::DecoratedPot.slots(), 1);
    assert_eq!(ContainerKind::Crafter.slots(), 9);
    assert_eq!(ContainerKind::Crafter.menu_type(), menu::CRAFTER_3X3);
    assert_eq!(ContainerKind::Crafter.title(), "container.crafter");
    assert_eq!(ContainerKind::Shelf.slots(), 3);
    assert_eq!(ContainerKind::Shelf.title(), "container.shelf");
}

#[test]
fn chest_halves_point_at_each_other_the_official_way() {
    let right = BlockState(blocks::CHEST)
        .with(HORIZONTAL_FACING, Direction::North)
        .with(CHEST_TYPE, ChestType::Right);
    let left = BlockState(blocks::CHEST)
        .with(HORIZONTAL_FACING, Direction::North)
        .with(CHEST_TYPE, ChestType::Left);

    assert_eq!(connected_direction(right), Direction::West);
    assert_eq!(connected_direction(left), Direction::East);
}

#[test]
fn the_lid_box_of_an_upright_shulker_is_the_half_block_above_it() {
    let pos = BlockPos::new(3, 64, -2);
    let lid = open_lid_box(pos, Direction::Up);
    assert_eq!(
        lid,
        Aabb::new(
            f64::from(pos.x),
            f64::from(pos.y) + 1.0,
            f64::from(pos.z),
            f64::from(pos.x) + 1.0,
            f64::from(pos.y) + 1.5,
            f64::from(pos.z) + 1.0
        )
        .deflated(1.0e-6)
    );
}

#[test]
fn a_shulker_cannot_open_into_a_solid_block() {
    let mut region = Region::new(2, 20, false);
    let pos = BlockPos::new(0, 100, 0);
    // 官方只在这格真的是潜影盒方块实体时才问 `canOpen`，所以先把方块放上。
    region.chunks.set_block(pos, blocks::SHULKER_BOX);
    assert!(can_open_shulker(&mut region, pos));

    region.chunks.set_block(pos.above(), blocks::STONE);
    assert!(!can_open_shulker(&mut region, pos));
}

#[test]
fn the_shulker_animation_follows_the_block_event() {
    let mut entity = ShulkerBoxEntity::new(BlockPos::new(0, 100, 0));
    assert!(entity.is_closed());

    assert!(entity.trigger_event(1, 1));
    assert_eq!(entity.open_count(), 1);
    assert_eq!(entity.status(), ShulkerStatus::Opening);

    assert!(entity.trigger_event(1, 0));
    assert_eq!(entity.open_count(), 0);
    assert_eq!(entity.status(), ShulkerStatus::Closing);

    assert!(!entity.trigger_event(2, 0));
}

#[test]
fn chests_accept_the_open_event_but_hoppers_do_not() {
    let mut region = Region::new(2, 20, false);
    let pos = BlockPos::new(0, 100, 0);
    let (mut level, ticks) = region.block_level();
    let mut world = World::new(&mut level, ticks);
    assert!(BlockState(blocks::CHEST).trigger_event(&mut world, pos, 1, 1));
    assert!(BlockState(blocks::ENDER_CHEST).trigger_event(&mut world, pos, 1, 1));
    assert!(!BlockState(blocks::HOPPER).trigger_event(&mut world, pos, 1, 1));
}

#[test]
fn a_new_chest_starts_with_empty_slots() {
    let chest = ContainerEntry::new(ContainerKind::Chest);
    assert_eq!(chest.slots.len(), 27);
    assert!(chest.slots.iter().all(ItemStack::is_empty));
    assert_eq!(chest.open_count, 0);
    assert!(chest.recheck_at.is_none());
}

#[test]
fn every_player_gets_their_own_ender_chest() {
    let ender = EnderChest::default();
    assert_eq!(ender.slots.len(), 27);
    assert!(ender.active.is_none());
    // 箱子的默认朝向，测试里用得上。
    assert_eq!(BlockState(blocks::CHEST).get(FACING), Direction::North);
}

/// 平移测试的容器脚本：几只装着东西的箱子随机被「打开」（开合人数 1、几刻后重查），
/// 没有玩家，所以到期重查会把它关上。观测每只箱子的开合人数和距下次重查的刻数。
/// `shift` 给出时第 `mid` 刻后按 `parts` 平移岛的时钟。
///
/// Container script for the rebase test: chests are randomly "opened" with a
/// recheck a few ticks later; with no players the recheck closes them.
fn container_script(
    seed: u64,
    base: i64,
    shift: Option<(i64, i64, crate::region::ClockParts)>,
) -> Vec<Vec<(i32, Option<u64>)>> {
    const CHESTS: [BlockPos; 4] = [
        BlockPos::new(0, -60, 0),
        BlockPos::new(3, -60, 0),
        BlockPos::new(20, -60, 5),
        BlockPos::new(40, -60, 9),
    ];
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut region = Region::new(2, 20, false);
    region.start_clock(crate::global::GameTime(base));
    for pos in CHESTS {
        region.chunks.set_block(pos, blocks::CHEST);
        let entry = entry_in(&mut region.containers, pos, ContainerKind::Chest);
        entry.slots[0] = ItemStack::new(1, 1);
    }
    let mut out = Vec::new();
    for step in 1..=150_i64 {
        region.advance_tick();
        if next() % 3 == 0 {
            let pos = CHESTS[(next() % 4) as usize];
            let at = region.tick.plus(next() % 12);
            let entry = region.containers.get_mut(&pos).expect("chest entry");
            entry.open_count = 1;
            entry.recheck_at = Some(at);
        }
        tick(&mut region);
        let now = region.tick;
        out.push(
            CHESTS
                .iter()
                .map(|pos| {
                    let entry = &region.containers[pos];
                    (
                        entry.open_count,
                        entry.recheck_at.map(|at| now.remaining(at)),
                    )
                })
                .collect(),
        );
        if let Some((mid, delta, parts)) = shift
            && step == mid
        {
            if parts == crate::region::ClockParts::ALL {
                region.rebase_clock(delta);
            } else {
                region.rebase_clock_parts(delta, parts);
            }
        }
    }
    out
}

/// 容器 `recheck_at` 一类的平移：「中途平移 d 再跑」等于「岛的时钟本来就差 d」；
/// 改坏确认：漏平移容器这一类会被抓到。
///
/// Container rechecks: shifting mid-run equals a clock that was always d
/// apart; leaving containers out is caught.
#[test]
fn rebasing_container_rechecks_equals_a_clock_that_was_always_apart() {
    use crate::region::ClockParts;
    let mut caught = false;
    for seed in 1..=30_u64 {
        let base = (seed as i64 * 977) % 4000;
        let delta = (seed as i64 * 7919) % 6000 - base.min(3000);
        let apart = container_script(seed, base + delta, None);
        assert_eq!(
            container_script(seed, base, Some((70, delta, ClockParts::ALL))),
            apart,
            "seed {seed} base {base} delta {delta}"
        );
        let broken = ClockParts {
            containers: false,
            ..ClockParts::ALL
        };
        caught |= container_script(seed, base, Some((70, delta, broken))) != apart;
    }
    assert!(
        caught,
        "leaving container rechecks out of the rebase went unnoticed"
    );
}
