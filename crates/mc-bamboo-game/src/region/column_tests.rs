//! 区块束的取放：搬过和没搬过一样，见 `docs/world-model-design.md` 第 4.2 节。
//!
//! 两层允许的差别要显式避开：放回的计划刻会排在同（时刻，优先级）的其它刻前面，
//! 放回的方块实体排到运行顺序末尾。所以计划刻按（时刻，优先级，位置）排序后比较，
//! 方块实体按位置排序后比较，再看继续跑若干刻以后的方块和容器内容。

use std::{any::TypeId, collections::BTreeSet};

use crate::{
    command_block::CommandBlockData,
    container::{ContainerEntry, ContainerKind},
    region::{
        Region, RegionTick,
        column::{NotReady, RegionColumn},
    },
    world::tests::Harness,
};
use mc_bamboo_blocks::{
    HopperEntity, LecternFacts, ScheduledFluidTick, ScheduledTick, TickPriority,
    behaviour::machines::FurnaceEntity,
};
use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};
use mc_bamboo_data::{
    CookingKind, block_id_of_state, blocks, items,
    registries::{data_component_type as kinds, fluid},
};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry},
    item::ItemStack,
    text::Component,
};

/// 要搬的区块：出生点所在的 (0, 0)。
const CHUNK: ChunkPos = ChunkPos::new(0, 0);
/// 另一个区块的一半内容，用来验证「别的区块不受影响」。
const OTHER_CHUNK: ChunkPos = ChunkPos::new(1, 0);

const Y: i32 = -60;

fn p(x: i32, z: i32) -> BlockPos {
    BlockPos::new(x, Y, z)
}

/// 场景里所有用到的位置，都在 (0, 0) 里。
const CHEST: BlockPos = BlockPos::new(2, Y, 2);
const FURNACE: BlockPos = BlockPos::new(3, Y, 2);
const HOPPER: BlockPos = BlockPos::new(4, Y, 2);
const HOPPER_2: BlockPos = BlockPos::new(5, Y, 2);
const LECTERN: BlockPos = BlockPos::new(6, Y, 2);
const COMMAND: BlockPos = BlockPos::new(7, Y, 2);
const COMPARATOR: BlockPos = BlockPos::new(8, Y, 2);
const REPEATER: BlockPos = BlockPos::new(9, Y, 2);
const REPEATER_2: BlockPos = BlockPos::new(10, Y, 2);
const LAMP: BlockPos = BlockPos::new(11, Y, 2);
/// 别的区块里的箱子，不跟着搬。
const AWAY_CHEST: BlockPos = BlockPos::new(17, Y, 2);

fn assert_is_send<T: Send>() {}

/// 一个有内容的区块：箱子、在工作的熔炉、漏斗链、带书的讲台、命令方块、
/// 比较器读着箱子、几条排着队的计划刻和一个发光方块。
///
/// 直接写各张表，和区块平面交付时组装盒子的内容等价；这样不依赖玩家发包，
/// 也不受 tick 里广播和菜单同步的影响。
fn furnish(region: &mut Region) {
    let store = &mut region.chunks;
    store.set_block(CHEST, blocks::CHEST);
    store.set_block(FURNACE, blocks::FURNACE);
    store.set_block(HOPPER, blocks::HOPPER);
    store.set_block(HOPPER_2, blocks::HOPPER);
    store.set_block(LECTERN, blocks::LECTERN);
    store.set_block(COMMAND, blocks::COMMAND_BLOCK);
    store.set_block(COMPARATOR, blocks::COMPARATOR);
    store.set_block(REPEATER, blocks::REPEATER);
    store.set_block(REPEATER_2, blocks::REPEATER);
    store.set_block(LAMP, blocks::GLOWSTONE);
    store.set_block(AWAY_CHEST, blocks::CHEST);

    crate::container::entry_in(&mut region.containers, CHEST, ContainerKind::Chest).slots[0] =
        ItemStack::new(items::DIAMOND, 7);
    crate::container::entry_in(&mut region.containers, HOPPER, ContainerKind::Hopper).slots[0] =
        ItemStack::new(items::COAL, 3);
    crate::container::entry_in(&mut region.containers, HOPPER_2, ContainerKind::Hopper).slots[0] =
        ItemStack::new(items::IRON_INGOT, 1);
    let away = crate::container::entry_in(&mut region.containers, AWAY_CHEST, ContainerKind::Chest);
    away.slots[0] = ItemStack::new(items::RAW_IRON, 5);
    away.dead = true;
    away.recheck_at = Some(RegionTick(41));

    let mut patch = ComponentPatch::default();
    patch.put(
        kinds::CUSTOM_NAME,
        PatchEntry::Set(ComponentValue::CustomName(Component::text("盒子"))),
    );
    region.block_items.insert(CHEST, patch);

    let mut command = empty_command_block();
    command.command = "say hello".to_owned();
    command.track_output = false;
    command.automatic = true;
    region.command_blocks.insert(COMMAND, command);

    // 讲台上的书，按 `lectern::Book` 的形状填一份有内容的书。
    region.lecterns.books.insert(
        LECTERN,
        crate::lectern::Book::for_tests(ItemStack::new(items::BOOK, 1), 2, 6, true),
    );

    // 比较器输出、书架槽位、讲台页码、火把开关记录，都在这个区块里。
    let redstone = &mut region.redstone;
    redstone.set_comparator_output(COMPARATOR, 9);
    redstone.set_last_bookshelf_slot(p(12, 2), 3);
    redstone.set_lectern(
        LECTERN,
        LecternFacts {
            page: 2,
            page_count: 6,
            has_content: true,
        },
    );
    redstone.record_torch_toggle(p(13, 2), 90);
    redstone.record_torch_toggle(p(13, 2), 95);

    // 在工作的熔炉，和漏斗链上的两个漏斗实体。
    let mut furnace = FurnaceEntity::new(FURNACE, CookingKind::Smelting);
    furnace.set_item(0, ItemStack::new(items::RAW_IRON, 4));
    furnace.set_item(1, ItemStack::new(items::COAL, 2));
    furnace.set_data(0, 120);
    furnace.set_data(1, 160);
    furnace.set_data(2, 30);
    furnace.set_data(3, 200);
    region.block_entities.insert(Box::new(furnace));
    region
        .block_entities
        .insert(Box::new(HopperEntity::new(HOPPER)));
    region
        .block_entities
        .insert(Box::new(HopperEntity::new(HOPPER_2)));

    // 方块和流体计划刻，几条不同的到期时刻和优先级。
    let repeater = block_id_of_state(blocks::REPEATER).unwrap();
    let glowstone = block_id_of_state(blocks::GLOWSTONE).unwrap();
    region
        .block_ticks
        .schedule(REPEATER, repeater, 7, TickPriority::Normal);
    region
        .block_ticks
        .schedule(REPEATER_2, repeater, 9, TickPriority::High);
    region
        .block_ticks
        .schedule(LAMP, glowstone, 11, TickPriority::Low);
    region
        .block_ticks
        .schedule(p(14, 2), repeater, 13, TickPriority::Normal);
    region
        .block_ticks
        .schedule_fluid(p(15, 2), fluid::WATER, 8, TickPriority::Normal);
    region
        .block_ticks
        .schedule_fluid(p(15, 3), fluid::FLOWING_WATER, 12, TickPriority::Low);
    // 别的区块里也排一条，验它不跟着搬。
    region
        .block_ticks
        .schedule(p(20, 2), repeater, 6, TickPriority::Normal);
}

fn empty_command_block() -> CommandBlockData {
    // 字段私有，用一次真实 placement 路径造出默认值。
    let mut region = Region::new(2, 20, false);
    region.chunks.set_block(COMMAND, blocks::COMMAND_BLOCK);
    crate::command_block::on_placed(&mut region, COMMAND);
    region.command_blocks.remove(&COMMAND).unwrap()
}

/// 把光照跑完，保证两个世界都在同样的前提下比较。
fn settle_light(region: &mut Region) {
    region.chunks.finish_light_updates();
    assert!(region.ready_to_move_columns());
}

/// 方块、两层光照、告示牌/方块实体 NBT 的逐格读数。
fn readings(region: &Region) -> Vec<u8> {
    let mut values = Vec::new();
    for x in 0..32 {
        for z in 0..8 {
            for y in [Y - 1, Y, Y + 1] {
                let pos = BlockPos::new(x, y, z);
                let state = region.chunks.block(pos).unwrap_or_default();
                values.extend(state.to_be_bytes());
                values.push(region.chunks.light_value(LightLayer::Sky, pos));
                values.push(region.chunks.light_value(LightLayer::Block, pos));
                values.extend(
                    region
                        .chunks
                        .heightmap_y(pos)
                        .unwrap_or_default()
                        .to_be_bytes(),
                );
                if let Some(tag) = region.chunks.update_tag(pos) {
                    values.extend(tag.iter().map(|byte| byte.wrapping_add(1)));
                }
                values.push(0xaa);
            }
        }
    }
    values
}

/// 一条计划刻的读数：时刻、优先级、位置、种类。
type Tick = (i64, i32, BlockPos, usize);
type FluidTick = (i64, i32, BlockPos, i32);
/// 一格红石记忆的公开读数。
type RedstoneRow = (BlockPos, i32, Option<usize>, Option<LecternFacts>, usize);
/// 一个容器条目的读数。
type ContainerRow = (BlockPos, Vec<ItemStack>, bool, Option<RegionTick>);

/// 全部容器内容，按位置排序。`Region` 里的表和盒子里的表都能用。
fn containers(entries: impl Iterator<Item = (BlockPos, ContainerEntry)>) -> Vec<ContainerRow> {
    let mut rows: Vec<_> = entries
        .map(|(pos, entry)| (pos, entry.slots.clone(), entry.dead, entry.recheck_at))
        .collect();
    rows.sort_by_key(|(pos, ..)| *pos);
    rows
}

/// 计划刻按（时刻，优先级，位置）排序：避开放入时负数子序号带来的先后差别。
fn scheduled(region: &Region) -> (Vec<Tick>, Vec<FluidTick>) {
    let mut blocks: Vec<_> = region
        .block_ticks
        .pending()
        .map(
            |ScheduledTick {
                 pos,
                 block,
                 trigger_tick,
                 priority,
             }| { (trigger_tick, priority as i32, pos, block) },
        )
        .collect();
    blocks.sort_by_key(|&(at, priority, pos, _)| (at, priority, pos));
    let mut fluids: Vec<_> = region
        .block_ticks
        .pending_fluids()
        .map(
            |ScheduledFluidTick {
                 pos,
                 fluid,
                 trigger_tick,
                 priority,
             }| { (trigger_tick, priority as i32, pos, fluid) },
        )
        .collect();
    fluids.sort_by_key(|&(at, priority, pos, _)| (at, priority, pos));
    (blocks, fluids)
}

/// 红石记忆的公开读数，按位置排序。
fn redstone(region: &Region) -> Vec<RedstoneRow> {
    let mut rows = Vec::new();
    for x in 0..32 {
        for z in 0..8 {
            let pos = BlockPos::new(x, Y, z);
            rows.push((
                pos,
                region.redstone.comparator_output(pos),
                region.redstone.last_bookshelf_slot(pos),
                region.redstone.lectern(pos),
                region.redstone.torch_toggle_count(pos),
            ));
        }
    }
    rows
}

/// 方块实体按位置排序，不看 ticker 的运行顺序；熔炉的四个数据槽也读出来。
fn entities(region: &Region) -> Vec<(BlockPos, &'static str, Vec<i32>)> {
    let mut rows = Vec::new();
    region.block_entities.visit(|pos, entity| {
        let type_id = entity.as_any().type_id();
        let name = if type_id == TypeId::of::<FurnaceEntity>() {
            "furnace"
        } else if type_id == TypeId::of::<HopperEntity>() {
            "hopper"
        } else {
            "other"
        };
        let data = entity
            .as_any()
            .downcast_ref::<FurnaceEntity>()
            .map(|furnace| {
                let mut values = furnace.data_values().to_vec();
                values.extend(
                    furnace
                        .items()
                        .iter()
                        .map(|stack| stack.item * 31 + stack.count),
                );
                values
            })
            .unwrap_or_default();
        rows.push((pos, name, data));
    });
    rows.sort_by_key(|(pos, ..)| *pos);
    rows
}

/// 除了版本号之外的完整快照，用来判「搬过和没搬过一样」。
fn snapshot(region: &Region) -> String {
    let mut out = String::new();
    // 版本号在取放时会换新的，它是缓存失效用的计数器，不进快照。
    out.push_str(&format!("{:?}", readings(region)));
    out.push_str(&format!(
        "{:?}",
        containers(region.containers.iter().map(|(&p, e)| (p, e.clone())))
    ));
    out.push_str(&format!("{:?}", region.block_items));
    out.push_str(&format!("{:?}", region.command_blocks));
    out.push_str(&format!("{:?}", region.lecterns.books));
    out.push_str(&format!("{:?}", scheduled(region)));
    out.push_str(&format!("{:?}", redstone(region)));
    out.push_str(&format!("{:?}", entities(region)));
    out.push_str(&format!("{:?}", region.block_ticks.now()));
    out
}

/// 两个世界用同一套摆法，然后一起跑若干刻。
fn pair() -> (Harness, Harness) {
    let mut a = Harness::new(2);
    let mut b = Harness::new(2);
    furnish(a.region());
    furnish(b.region());
    settle_light(a.region());
    settle_light(b.region());
    (a, b)
}

/// 跑几刻，让计划刻、漏斗和熔炉都真正动起来。
fn run(server: &mut Harness, ticks: usize) {
    for _ in 0..ticks {
        server.tick();
    }
}

#[test]
fn region_column_is_send() {
    assert_is_send::<RegionColumn>();
    assert_is_send::<Box<RegionColumn>>();
    assert_is_send::<NotReady>();
}

#[test]
fn taking_and_inserting_leaves_the_world_as_it_was() {
    let (mut moved, mut direct) = pair();
    // 跑到 tick 边界：正常的光照预算已经跑完，各张表都不在 tick 中途。
    run(&mut moved, 3);
    run(&mut direct, 3);
    settle_light(moved.region());
    settle_light(direct.region());

    let before = snapshot(moved.region());
    let mut boxes = moved
        .region()
        .take_columns(&[CHUNK].into_iter().collect())
        .expect("tick 边界、光照算完了");
    let column = boxes.remove(&CHUNK).expect("这个区块有盒子");
    moved
        .region()
        .insert_column(CHUNK, column)
        .unwrap_or_else(|_| panic!("前提已经检查过"));

    assert_eq!(snapshot(moved.region()), before, "取放本身不能改变任何读数");

    // 继续跑，和没搬过的对照世界比。
    run(&mut moved, 6);
    run(&mut direct, 6);
    settle_light(moved.region());
    settle_light(direct.region());
    assert_eq!(snapshot(moved.region()), snapshot(direct.region()));
}

#[test]
fn taking_removes_every_positional_table_but_leaves_other_chunks_alone() {
    let mut server = Harness::new(2);
    furnish(server.region());
    settle_light(server.region());

    let away_before = server.region().containers[&AWAY_CHEST].slots.clone();
    let away_block = server.region().chunks.block(AWAY_CHEST);
    let boxes = server
        .region()
        .take_columns(&[CHUNK].into_iter().collect())
        .expect("前提满足");
    assert_eq!(boxes.len(), 1);
    let column = &boxes[&CHUNK];

    // 盒子装着这个区块的全部带位置状态。
    assert_eq!(column.column().chunk().block(2, Y, 2), blocks::CHEST);
    assert!(!column.containers().is_empty());
    assert!(column.command_blocks().contains_key(&COMMAND));
    assert!(column.books().contains_key(&LECTERN));
    assert!(column.block_items().contains_key(&CHEST));
    assert!(column.has_ticks());

    let region = server.region();
    assert!(!region.containers.contains_key(&CHEST));
    assert!(!region.containers.contains_key(&FURNACE));
    assert!(!region.containers.contains_key(&HOPPER));
    assert!(!region.containers.contains_key(&HOPPER_2));
    assert!(!region.block_items.contains_key(&CHEST));
    assert!(!region.command_blocks.contains_key(&COMMAND));
    assert!(!region.lecterns.books.contains_key(&LECTERN));
    assert!(!region.block_entities.contains(CHEST));
    assert!(!region.block_entities.contains(FURNACE));
    assert!(!region.block_entities.contains(HOPPER));
    assert!(!region.redstone.lectern(LECTERN).is_some());
    assert_eq!(region.redstone.comparator_output(COMPARATOR), 0);
    assert_eq!(region.redstone.torch_toggle_count(p(13, 2)), 0);
    let (blocks, fluids) = scheduled(region);
    assert_eq!(blocks.len(), 1, "只剩别的区块那条方块计划刻");
    assert_eq!(blocks[0].2, p(20, 2));
    assert!(fluids.is_empty());

    // 别的区块一点没动。
    assert_eq!(region.containers[&AWAY_CHEST].slots, away_before);
    assert_eq!(region.chunks.block(AWAY_CHEST), away_block);
    assert!(region.containers[&AWAY_CHEST].dead);
}

#[test]
fn a_column_moves_between_regions_with_different_clocks() {
    let (mut source, mut direct) = pair();
    run(&mut source, 2);
    run(&mut direct, 2);
    settle_light(source.region());
    settle_light(direct.region());

    // 目标是个全新的 region，模拟时间和世界时间都往前拨过。
    let mut target = Harness::new(2);
    {
        let region = target.region();
        region.tick = RegionTick(37);
        region.block_ticks.set_time(101);
        region.chunks.finish_light_updates();
        assert!(region.ready_to_move_columns());
    }
    let mut boxes = source
        .region()
        .take_columns(&[CHUNK].into_iter().collect())
        .expect("前提满足");
    target
        .region()
        .insert_column(CHUNK, boxes.remove(&CHUNK).unwrap())
        .unwrap_or_else(|_| panic!("前提满足"));

    // 计划刻和红石火把记录存的是相对值，所以两边到期时刻各自按自己的 now 换算。
    // 比较时把绝对时刻减去各自的 now。
    // 只比较跟着区块走的那些刻；别的区块的刻留在源 region 里。
    let in_chunk = |rows: Vec<Tick>| {
        rows.into_iter()
            .filter(|&(_, _, pos, _)| pos.chunk() == CHUNK)
            .collect::<Vec<_>>()
    };
    let source_blocks = in_chunk(scheduled(direct.region()).0);
    let target_blocks = in_chunk(scheduled(target.region()).0);
    let source_now = direct.region().block_ticks.now();
    let target_now = target.region().block_ticks.now();
    let normalize = |rows: &[(i64, i32, BlockPos, usize)], now: i64| {
        rows.iter()
            .map(|&(at, priority, pos, block)| (at - now, priority, pos, block))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        normalize(&source_blocks, source_now),
        normalize(&target_blocks, target_now)
    );
    assert_eq!(
        target.region().redstone.torch_toggle_count(p(13, 2)),
        direct.region().redstone.torch_toggle_count(p(13, 2)),
    );

    // 继续跑相同的刻数，方块和容器内容一致（时钟不同导致随机刻的取数不同，
    // 所以这里只比较不受随机源影响的那些表）。
    run(&mut target, 5);
    assert_eq!(
        containers(
            target
                .region()
                .containers
                .iter()
                .map(|(&p, e)| (p, e.clone()))
        ),
        containers(
            direct
                .region()
                .containers
                .iter()
                .map(|(&p, e)| (p, e.clone()))
        ),
    );
    assert_eq!(
        target.region().command_blocks,
        direct.region().command_blocks
    );
    assert_eq!(target.region().block_items, direct.region().block_items);
    assert_eq!(
        target.region().lecterns.books,
        direct.region().lecterns.books
    );
}

/// 容器条目的 `recheck_at` 在盒子里存相对剩余刻数：目标 region 的时刻不同也能对上。
#[test]
fn container_recheck_times_are_relative_to_the_target_clock() {
    let (mut source, mut direct) = pair();
    {
        let region = source.region();
        // 一条马上要到、一条还差几刻的绝对时刻。
        region.containers.get_mut(&CHEST).unwrap().recheck_at = Some(region.tick.plus(5));
        region.containers.get_mut(&HOPPER).unwrap().recheck_at = Some(region.tick);
        region.containers.get_mut(&AWAY_CHEST).unwrap().recheck_at = Some(RegionTick(41));
    }
    let mut target = Harness::new(2);
    target.region().tick = RegionTick(97);
    target.region().block_ticks.set_time(200);
    target.region().chunks.finish_light_updates();

    let mut boxes = source
        .region()
        .take_columns(&[CHUNK].into_iter().collect())
        .expect("前提满足");
    let column = boxes.remove(&CHUNK).unwrap();
    // 盒子里不带绝对时刻，只带剩余刻数。
    assert_eq!(
        column.container(CHEST).and_then(|entry| entry.recheck_at),
        None
    );
    target
        .region()
        .insert_column(CHUNK, column)
        .unwrap_or_else(|_| panic!("前提满足"));

    assert_eq!(
        target.region().containers[&CHEST].recheck_at,
        Some(RegionTick(102))
    );
    assert_eq!(
        target.region().containers[&HOPPER].recheck_at,
        Some(RegionTick(97))
    );
    // 别的区块的条目原地不动。
    assert_eq!(
        source.region().containers[&AWAY_CHEST].recheck_at,
        Some(RegionTick(41))
    );
    assert_eq!(
        direct.region().containers[&AWAY_CHEST].recheck_at,
        Some(RegionTick(41))
    );
}

#[test]
fn pending_light_blocks_taking_and_returns_the_box_on_insert() {
    let (mut moved, mut target) = pair();
    settle_light(moved.region());
    let column = moved
        .region()
        .take_columns(&[CHUNK].into_iter().collect())
        .unwrap()
        .remove(&CHUNK)
        .unwrap();

    // 目标刚改了方块，光照还没算完：整批不动。
    target.region().chunks.set_block(p(1, 1), blocks::STONE);
    assert!(!target.region().ready_to_move_columns());
    let snapshot_before = snapshot(target.region());
    let wanted: BTreeSet<ChunkPos> = [CHUNK, OTHER_CHUNK].into_iter().collect();
    assert!(matches!(
        target.region().take_columns(&wanted),
        Err(NotReady)
    ));
    assert_eq!(snapshot(target.region()), snapshot_before);

    // 放入也拒绝，盒子原样还回来。
    let Err((NotReady, returned)) = target.region().insert_column(CHUNK, column) else {
        panic!("光照没算完时应该拒绝放入");
    };
    assert_eq!(snapshot(target.region()), snapshot_before);
    settle_light(target.region());
    target
        .region()
        .insert_column(CHUNK, returned)
        .unwrap_or_else(|_| panic!("跑完光照就能放入"));
}

#[test]
fn a_busy_table_blocks_the_whole_batch() {
    let mut server = Harness::new(2);
    furnish(server.region());
    settle_light(server.region());

    // 计划刻队列正在跑这一刻到期的刻（`collect_due` 是 pub(crate)，
    // 这里用下层的公开查询把它摆成「忙」的状态做不了，改用方块实体表）。
    let region = server.region();
    region.block_entities.begin_tick();
    assert!(!region.ready_to_move_columns());
    let before = snapshot(region);
    let wanted: BTreeSet<ChunkPos> = [CHUNK, OTHER_CHUNK].into_iter().collect();
    assert!(matches!(region.take_columns(&wanted), Err(NotReady)));
    assert_eq!(snapshot(region), before, "整批都不动");
    region.block_entities.end_tick();
    assert!(region.ready_to_move_columns());
}

#[test]
fn taking_several_chunks_keeps_each_box_separate() {
    let mut server = Harness::new(2);
    furnish(server.region());
    // 第二个区块里也放一点内容，验证按 x 带扫能分对盒子。
    let region = server.region();
    region.chunks.set_block(p(17, 4), blocks::FURNACE);
    crate::container::entry_in(&mut region.containers, p(18, 4), ContainerKind::Hopper).slots[0] =
        ItemStack::new(items::COAL, 2);
    region
        .block_items
        .insert(p(18, 4), ComponentPatch::default());
    region.block_ticks.schedule(
        p(19, 4),
        block_id_of_state(blocks::REPEATER).unwrap(),
        5,
        TickPriority::Normal,
    );
    settle_light(region);

    let wanted: BTreeSet<ChunkPos> = [CHUNK, OTHER_CHUNK].into_iter().collect();
    let boxes = region.take_columns(&wanted).expect("前提满足");
    assert_eq!(boxes.len(), 2);
    assert_eq!(boxes[&CHUNK].containers().len(), 3);
    assert_eq!(boxes[&OTHER_CHUNK].containers().len(), 2);
    assert!(boxes[&CHUNK].books().contains_key(&LECTERN));
    assert!(!boxes[&OTHER_CHUNK].books().contains_key(&LECTERN));
    assert_eq!(boxes[&CHUNK].command_blocks().len(), 1);
    assert!(boxes[&OTHER_CHUNK].command_blocks().is_empty());

    // 两个盒子都放回去，和没搬过一样。
    let mut other = Harness::new(2);
    furnish(other.region());
    other.region().chunks.set_block(p(17, 4), blocks::FURNACE);
    crate::container::entry_in(
        &mut other.region().containers,
        p(18, 4),
        ContainerKind::Hopper,
    )
    .slots[0] = ItemStack::new(items::COAL, 2);
    other
        .region()
        .block_items
        .insert(p(18, 4), ComponentPatch::default());
    other.region().block_ticks.schedule(
        p(19, 4),
        block_id_of_state(blocks::REPEATER).unwrap(),
        5,
        TickPriority::Normal,
    );
    settle_light(other.region());
    let before = snapshot(other.region());

    for (pos, column) in boxes {
        server
            .region()
            .insert_column(pos, column)
            .unwrap_or_else(|_| panic!("前提满足"));
    }
    // 版本号不同，用除版本号外的读数比。
    assert_eq!(snapshot(server.region()), before);
}

#[test]
fn a_taken_chunk_reads_the_template_and_caches_are_dropped() {
    let mut server = Harness::new(2);
    furnish(server.region());
    settle_light(server.region());
    // 让区块包缓存里有这个坐标的条目。
    {
        let region = server.region();
        let template = &region.template_body;
        crate::chunk_encoding::chunk_frame(
            &mut region.chunk_cache,
            &region.chunks,
            template,
            CHUNK,
        );
        assert_eq!(region.chunk_cache.frame_count(), 1);
    }

    let _boxes = server
        .region()
        .take_columns(&[CHUNK].into_iter().collect())
        .expect("前提满足");
    let region = server.region();
    assert!(region.chunks.is_template(CHUNK));
    assert_eq!(region.chunk_cache.frame_count(), 0);
    assert!(region.light_changes.is_empty());
    assert!(region.block_changes.changed.is_empty());
    // 别的区块的方块还在。
    assert_eq!(region.chunks.block(AWAY_CHEST), Some(blocks::CHEST));
}
