//! 直接构造世界状态来测菜单，不开 socket。

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use mc_bamboo_blocks::{
    BlockState,
    state::{CHEST_TYPE, ChestType, HORIZONTAL_FACING},
};
use mc_bamboo_core::{BlockPos, Direction, Vec3};
use mc_bamboo_data::{blocks, items, packets::play::clientbound, registries::menu as menu_types};
use mc_bamboo_protocol::{
    codec::Reader,
    frame::{MAX_FRAME_LEN, find_frame},
    packet::play::{ClientInformation, ContainerInput, GameMode},
};
use mc_bamboo_world::flat;
use uuid::Uuid;

use super::*;
use crate::{Outgoing, PlayerSink, container::ContainerEntry, player::Player};

mod property;

#[derive(Default)]
struct SinkState {
    bytes: Vec<u8>,
    closed: bool,
}

#[derive(Clone, Default)]
struct TestSink(Arc<Mutex<SinkState>>);

impl PlayerSink for TestSink {
    fn send(&mut self, frames: Outgoing) -> bool {
        self.0.lock().unwrap().bytes.extend(frames.to_raw_bytes());
        true
    }

    fn close(&mut self) {
        self.0.lock().unwrap().closed = true;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Frame {
    id: i32,
    body: Vec<u8>,
}

impl Frame {
    fn reader(&self) -> Reader<'_> {
        Reader::new(&self.body)
    }
}

struct Fixture {
    region: Region,
    id: PlayerId,
    sink: TestSink,
}

impl Fixture {
    fn new() -> Self {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let sink = TestSink::default();
        region.links.add(id, Box::new(sink.clone()));
        spawn(&mut region, id, 0.5, 0.5);
        Self { region, id, sink }
    }

    fn chest_pos() -> BlockPos {
        BlockPos::new(0, flat::surface_y(), 0)
    }

    fn place(&mut self, pos: BlockPos, state: u32) {
        assert!(self.region.chunks.set_block(pos, state), "位置要能写进区块");
    }

    fn open(&mut self, pos: BlockPos) {
        open_block(&mut self.region, self.id, pos);
    }

    fn menu(&self) -> &Menu {
        self.region.menus.get(&self.id).expect("菜单应该开着")
    }

    fn entry(&self, pos: BlockPos) -> &ContainerEntry {
        self.region.containers.get(&pos).expect("容器应该有条目")
    }

    fn player(&self) -> &crate::player::Player {
        self.region.players.get(&self.id).expect("玩家应该在区里")
    }

    fn inventory_slot(&mut self, slot: usize) -> &mut ItemStack {
        &mut self
            .region
            .players
            .get_mut(&self.id)
            .expect("玩家应该在区里")
            .inventory
            .slots[slot]
    }

    /// 快捷栏第 `index` 格在菜单里的槽位。
    fn hotbar(index: usize) -> i16 {
        (27 + 27 + index) as i16
    }

    fn click(&mut self, slot: i16, button: i8, input: ContainerInput) {
        let packet = ClickPacket {
            container_id: self.menu().container_id,
            state_id: self.menu().state_id,
            slot,
            button,
            input,
            changed_slots: BTreeMap::new(),
            carried: HashedStack::Empty,
        };
        on_click(&mut self.region, self.id, packet);
    }

    fn take_frames(&mut self) -> Vec<Frame> {
        self.region.links.flush();
        let bytes = std::mem::take(&mut self.sink.0.lock().unwrap().bytes);
        let mut rest = bytes.as_slice();
        let mut frames = Vec::new();
        while !rest.is_empty() {
            let bounds = find_frame(rest, MAX_FRAME_LEN)
                .expect("服务端只发合法的帧")
                .expect("flush 的都是完整的帧");
            let end = bounds.header_len + bounds.payload_len;
            let mut reader = Reader::new(&rest[bounds.header_len..end]);
            frames.push(Frame {
                id: reader.varint().unwrap(),
                body: reader.rest().to_vec(),
            });
            rest = &rest[end..];
        }
        frames
    }

    fn take_ids(&mut self) -> Vec<i32> {
        self.take_frames()
            .into_iter()
            .map(|frame| frame.id)
            .collect()
    }
}

fn spawn(region: &mut Region, id: PlayerId, x: f64, z: f64) {
    let position = Vec3::new(x, f64::from(flat::surface_y()), z);
    let player = Player::new(
        id,
        Uuid::from_u128(id.0 as u128),
        format!("player{}", id.0),
        GameMode::Creative,
        ClientInformation::default(),
        position,
    );
    region.insert_player(player);
}

/// 护甲槽在菜单里是 5 到 8，官方 `Inventory` 里是 39 到 36，顺序相反。
#[test]
fn armour_menu_slots_map_to_the_official_inventory_indices() {
    assert_eq!(official_index(5), Some(39), "头盔");
    assert_eq!(official_index(6), Some(38), "胸甲");
    assert_eq!(official_index(7), Some(37), "护腿");
    assert_eq!(official_index(8), Some(36), "靴子");
    assert_eq!(official_index(45), Some(40), "副手");
    assert_eq!(official_index(0), None, "合成结果和光标不在物品栏里");
}

/// 单箱打开后发 `open_screen` 和完整的 `container_set_content`。
#[test]
fn opening_a_chest_sends_the_screen_then_the_content() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);

    let frames = fixture.take_frames();
    let ids: Vec<i32> = frames.iter().map(|frame| frame.id).collect();
    // `ChestMenu` 的构造里 `startOpen` 先响，再发 `open_screen`。
    assert_eq!(
        ids,
        [
            clientbound::SOUND,
            clientbound::OPEN_SCREEN,
            clientbound::CONTAINER_SET_CONTENT
        ]
    );

    let mut open = frames[1].reader();
    assert_eq!(open.varint(), Ok(1));
    assert_eq!(open.varint(), Ok(menu_types::GENERIC_9X3));

    let mut content = frames[2].reader();
    assert_eq!(content.varint(), Ok(1));
    assert_eq!(content.varint(), Ok(1), "第一个包的 state_id 是 1");
    assert_eq!(content.varint(), Ok(63), "27 格容器加 36 格物品栏");

    assert_eq!(fixture.menu().slots_len(), 63);
    assert_eq!(fixture.menu().state_id, 1);
    assert_eq!(fixture.entry(pos).open_count, 1);
}

/// 打开箱子会排一个方块事件，同一 tick 的计划刻阶段把人数发给附近的人。
#[test]
fn opening_a_chest_sends_a_block_event() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);
    fixture.take_ids();

    crate::interaction::run_block_events(&mut fixture.region);
    let frames = fixture.take_frames();
    let events: Vec<&Frame> = frames
        .iter()
        .filter(|frame| frame.id == clientbound::BLOCK_EVENT)
        .collect();
    assert_eq!(events.len(), 1);

    let mut reader = events[0].reader();
    assert_eq!(reader.i64(), Ok(pos.pack()));
    assert_eq!(reader.u8(), Ok(1), "箱子用 action 1");
    assert_eq!(reader.u8(), Ok(1), "开盖人数是一");
    assert_eq!(
        reader.varint(),
        Ok(BlockState(blocks::CHEST).block_id() as i32),
        "官方这里带的是方块注册表 ID"
    );
}

/// 双箱的标题和格数都按官方走。
#[test]
fn a_double_chest_has_fifty_four_slots_and_its_own_title() {
    let mut fixture = Fixture::new();
    let y = flat::surface_y();
    let right = BlockState(blocks::CHEST)
        .with(HORIZONTAL_FACING, Direction::North)
        .with(CHEST_TYPE, ChestType::Right)
        .0;
    let left = BlockState(blocks::CHEST)
        .with(HORIZONTAL_FACING, Direction::North)
        .with(CHEST_TYPE, ChestType::Left)
        .0;
    fixture.place(BlockPos::new(0, y, 0), right);
    fixture.place(BlockPos::new(-1, y, 0), left);
    fixture.open(BlockPos::new(0, y, 0));

    assert_eq!(fixture.menu().container_size, 54);
    assert_eq!(fixture.menu().slots_len(), 90);
    assert_eq!(fixture.entry(BlockPos::new(0, y, 0)).open_count, 1);
    assert_eq!(fixture.entry(BlockPos::new(-1, y, 0)).open_count, 1);

    let frames = fixture.take_frames();
    // 开箱音效、打开界面、然后才是内容。右半边响一声，左半边不响。
    let content_frame = frames
        .iter()
        .find(|frame| frame.id == clientbound::CONTAINER_SET_CONTENT)
        .expect("content");
    let mut content = content_frame.reader();
    content.varint().unwrap();
    content.varint().unwrap();
    assert_eq!(content.varint(), Ok(90));
}

/// 拿起一格再放下，东西进了箱子，光标空了。
#[test]
fn picking_up_and_placing_moves_the_stack_into_the_chest() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);
    *fixture.inventory_slot(36) = ItemStack::new(items::STONE, 10);

    fixture.click(Fixture::hotbar(0), 0, ContainerInput::Pickup);
    assert_eq!(fixture.menu().carried, ItemStack::new(items::STONE, 10));
    assert!(fixture.player().inventory.slots[36].is_empty());

    fixture.click(0, 0, ContainerInput::Pickup);
    assert_eq!(
        fixture.entry(pos).slots[0],
        ItemStack::new(items::STONE, 10)
    );
    assert!(fixture.menu().carried.is_empty());
}

/// Shift 点击把容器里的东西移到物品栏。
#[test]
fn quick_move_moves_a_whole_stack_out_of_the_chest() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);
    fixture.entry_mut(pos).slots[0] = ItemStack::new(items::STONE, 10);

    fixture.click(0, 0, ContainerInput::QuickMove);

    assert!(fixture.entry(pos).slots[0].is_empty());
    let carried = fixture
        .player()
        .inventory
        .slots
        .iter()
        .filter(|stack| stack.item == items::STONE)
        .map(|stack| stack.count)
        .sum::<i32>();
    assert_eq!(carried, 10);
}

impl Fixture {
    fn entry_mut(&mut self, pos: BlockPos) -> &mut ContainerEntry {
        self.region
            .containers
            .get_mut(&pos)
            .expect("容器应该有条目")
    }
}

/// 数字键交换：光标上的东西和快捷栏那一格互换。
#[test]
fn a_number_key_swaps_with_the_hotbar() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);
    fixture.entry_mut(pos).slots[0] = ItemStack::new(items::STONE, 10);
    *fixture.inventory_slot(36) = ItemStack::new(items::DIRT, 5);

    fixture.click(0, 0, ContainerInput::Swap);

    assert_eq!(fixture.entry(pos).slots[0], ItemStack::new(items::DIRT, 5));
    assert_eq!(
        fixture.player().inventory.slots[36],
        ItemStack::new(items::STONE, 10)
    );
}

/// 创造模式中键克隆出一整堆，原格不动。
#[test]
fn middle_click_clones_a_full_stack() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);
    fixture.entry_mut(pos).slots[0] = ItemStack::new(items::STONE, 10);

    fixture.click(0, 0, ContainerInput::Clone);

    assert_eq!(fixture.menu().carried, ItemStack::new(items::STONE, 64));
    assert_eq!(
        fixture.entry(pos).slots[0],
        ItemStack::new(items::STONE, 10)
    );
}

/// Q 丢弃，光标空时丢一格里的东西。丢出的物品生成物品实体。
///
/// Throwing with an empty cursor drops the slot. The thrown stack becomes an item entity.
#[test]
fn throwing_empties_the_slot() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);
    fixture.entry_mut(pos).slots[0] = ItemStack::new(items::STONE, 10);

    fixture.click(0, 1, ContainerInput::Throw);

    assert!(fixture.entry(pos).slots[0].is_empty());
}

/// 拖动分配：点住、扫过三格、松开，每格分到均分的一份。
#[test]
fn dragging_divides_the_stack_between_the_slots() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);
    *fixture.inventory_slot(36) = ItemStack::new(items::STONE, 10);
    fixture.click(Fixture::hotbar(0), 0, ContainerInput::Pickup);

    // 开始、扫过三个箱子格、松开。
    fixture.click(-999, 0, ContainerInput::QuickCraft);
    for slot in 0..3 {
        fixture.click(slot, 1, ContainerInput::QuickCraft);
    }
    fixture.click(-999, 2, ContainerInput::QuickCraft);

    for slot in 0..3 {
        assert_eq!(
            fixture.entry(pos).slots[slot],
            ItemStack::new(items::STONE, 3)
        );
    }
    assert_eq!(fixture.menu().carried, ItemStack::new(items::STONE, 1));
}

/// 双击空格把同种物品收进光标。
#[test]
fn double_clicking_collects_similar_stacks() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);
    fixture.entry_mut(pos).slots[0] = ItemStack::new(items::STONE, 10);
    fixture.entry_mut(pos).slots[1] = ItemStack::new(items::DIRT, 3);
    *fixture.inventory_slot(36) = ItemStack::new(items::STONE, 5);
    fixture.click(Fixture::hotbar(0), 0, ContainerInput::Pickup);

    fixture.click(2, 0, ContainerInput::PickupAll);

    assert_eq!(fixture.menu().carried, ItemStack::new(items::STONE, 15));
    assert!(fixture.entry(pos).slots[0].is_empty());
    assert_eq!(fixture.entry(pos).slots[1], ItemStack::new(items::DIRT, 3));
}

/// 每次改槽都换一个 state_id，`set_cursor_item` 不占号。
#[test]
fn state_ids_advance_with_every_slot_change() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);
    fixture.take_frames();

    // 光标上的东西放进空槽：只有槽位变化，state_id 加一。
    fixture.menu_mut().carried = ItemStack::new(items::STONE, 10);
    fixture.click(0, 0, ContainerInput::Pickup);
    assert_eq!(fixture.menu().state_id, 2);
    assert_eq!(fixture.take_ids(), [clientbound::CONTAINER_SET_SLOT]);
    assert!(fixture.take_frames().is_empty());

    // 再把整格拿回光标：槽位和光标都变了，但光标不占 state_id。
    fixture.click(0, 0, ContainerInput::Pickup);
    assert_eq!(fixture.menu().state_id, 3);
    assert_eq!(
        fixture.take_ids(),
        [
            clientbound::CONTAINER_SET_SLOT,
            clientbound::SET_CURSOR_ITEM
        ]
    );
    assert!(fixture.take_frames().is_empty());
}

/// 关菜单时手上的东西放回物品栏，先叠到快满的那一格。
#[test]
fn closing_puts_the_carried_stack_back_into_the_inventory() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);
    fixture.entry_mut(pos).slots[0] = ItemStack::new(items::STONE, 10);
    *fixture.inventory_slot(36) = ItemStack::new(items::STONE, 60);
    fixture.click(0, 0, ContainerInput::Pickup);
    fixture.take_frames();

    on_close_request(&mut fixture.region, fixture.id);

    assert!(!fixture.region.menus.contains_key(&fixture.id));
    assert_eq!(
        fixture.player().inventory.slots[36],
        ItemStack::new(items::STONE, 64)
    );
    assert_eq!(
        fixture.player().inventory.slots[37],
        ItemStack::new(items::STONE, 6)
    );
    assert_eq!(
        fixture.take_ids(),
        [
            clientbound::SET_PLAYER_INVENTORY,
            clientbound::SET_PLAYER_INVENTORY,
            clientbound::SOUND
        ],
        "每一格都单独告诉客户端"
    );
    assert_eq!(fixture.entry(pos).open_count, 0);
}

/// 方块被破坏后，菜单下一 tick 自己关掉，内容随之丢掉。
#[test]
fn breaking_the_chest_closes_the_menu_and_discards_the_contents() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);
    fixture.entry_mut(pos).slots[0] = ItemStack::new(items::STONE, 10);
    fixture.take_frames();

    container::on_block_destroyed(&mut fixture.region, pos);
    fixture.place(pos, blocks::AIR);
    tick(&mut fixture.region);

    assert!(!fixture.region.menus.contains_key(&fixture.id));
    // 官方 `ServerPlayer.tick` 先广播、后关：上面直接改的那一格先发出去，再关菜单。
    // Vanilla broadcasts before closing: the slot changed above goes out, then the menu closes.
    assert_eq!(
        fixture.take_ids(),
        [
            clientbound::CONTAINER_SET_SLOT,
            clientbound::CONTAINER_CLOSE
        ]
    );
    container::tick(&mut fixture.region);
    assert!(!fixture.region.containers.contains_key(&pos));
}

/// 两名玩家开同一个箱子，第二个人看到第一个人放进去的东西。
#[test]
fn a_second_player_sees_what_the_first_player_put_in() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);
    *fixture.inventory_slot(36) = ItemStack::new(items::STONE, 10);
    fixture.click(Fixture::hotbar(0), 0, ContainerInput::Pickup);
    fixture.click(0, 0, ContainerInput::Pickup);

    let other = PlayerId(2);
    spawn(&mut fixture.region, other, 2.5, 2.5);
    open_block(&mut fixture.region, other, pos);

    let menu = fixture
        .region
        .menus
        .get(&other)
        .expect("第二个菜单应该开着");
    let slots = display(&fixture.region, other, menu);
    assert_eq!(slots[0], ItemStack::new(items::STONE, 10));
    assert_eq!(fixture.entry(pos).open_count, 2);
}

/// 一件带 `lock` 的物品：放进共享箱子以后，两名玩家看到的都是完整的。
///
/// An item with `lock` stays intact for every player who opens the shared chest.
#[test]
fn every_player_sees_the_lock_component() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);

    *fixture.inventory_slot(36) = locked_chest();
    fixture.click(Fixture::hotbar(0), 0, ContainerInput::Pickup);
    fixture.click(0, 0, ContainerInput::Pickup);
    assert_eq!(
        fixture.entry(pos).slots[0],
        locked_chest(),
        "世界里留着完整的"
    );
    // 放的人自己看到的那一份是完整的。
    let menu = fixture.menu();
    assert_eq!(
        network_slots(&fixture.region, fixture.id, menu)[0],
        locked_chest()
    );

    // 另一名玩家看到的也是完整的。
    // The other player sees the same lock.
    let other = PlayerId(2);
    spawn(&mut fixture.region, other, 2.5, 2.5);
    open_block(&mut fixture.region, other, pos);
    let other_menu = fixture
        .region
        .menus
        .get(&other)
        .expect("第二个菜单应该开着");
    assert_eq!(
        network_slots(&fixture.region, other, other_menu)[0],
        locked_chest()
    );
}

/// 别人把这件物品拿出箱子时，`lock` 跟着走。
///
/// Taking the item out keeps `lock` on the carried stack.
#[test]
fn taking_an_item_out_of_a_chest_keeps_its_components() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    container::entry_at(&mut fixture.region, pos, ContainerKind::Chest).slots[0] = locked_chest();
    fixture.open(pos);

    fixture.click(0, 0, ContainerInput::Pickup);

    assert_eq!(fixture.menu().carried, locked_chest());
    assert!(fixture.entry(pos).slots[0].is_empty());
}

/// 叠上去只改数量，组件还在。
///
/// Merging only changes the count. The components stay.
#[test]
fn merging_into_a_stack_keeps_the_components() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    container::entry_at(&mut fixture.region, pos, ContainerKind::Chest).slots[0] =
        ItemStack::new(items::STONE, 1);
    fixture.open(pos);

    fixture.menu_mut().carried = ItemStack::new(items::STONE, 1);
    fixture.click(0, 0, ContainerInput::Pickup);

    assert_eq!(fixture.entry(pos).slots[0], ItemStack::new(items::STONE, 2));

    fixture.menu_mut().carried = locked_chest();
    fixture.click(0, 0, ContainerInput::Pickup);
    assert_eq!(fixture.entry(pos).slots[0], locked_chest());
    assert_eq!(fixture.menu().carried, ItemStack::new(items::STONE, 2));
}

/// 一个带 `lock` 的箱子物品。`lock` 线上是一个空复合标签。
///
/// A chest item whose `lock` component is an empty compound.
fn locked_chest() -> ItemStack {
    let mut lock = Vec::new();
    mc_bamboo_nbt::Tag::Compound(Vec::new()).write_network(&mut lock);
    ItemStack {
        item: items::CHEST,
        count: 1,
        added: vec![(mc_bamboo_data::registries::data_component_type::LOCK, lock)],
        removed: Vec::new(),
        bundle_selected: None,
    }
}

/// 末影箱的内容跟着玩家走，换个方块位置再打开还是那些东西。
#[test]
fn the_ender_chest_contents_belong_to_the_player() {
    let mut fixture = Fixture::new();
    let y = flat::surface_y();
    let first = BlockPos::new(0, y, 0);
    let second = BlockPos::new(3, y, 0);
    fixture.place(first, blocks::ENDER_CHEST);
    fixture.place(second, blocks::ENDER_CHEST);
    fixture.open(first);

    assert_eq!(fixture.player().ender_chest.active, Some(first));
    *fixture.inventory_slot(36) = ItemStack::new(items::DIAMOND, 2);
    fixture.click(Fixture::hotbar(0), 0, ContainerInput::Pickup);
    fixture.click(0, 0, ContainerInput::Pickup);
    on_close_request(&mut fixture.region, fixture.id);

    assert_eq!(
        fixture.player().ender_chest.slots[0],
        ItemStack::new(items::DIAMOND, 2)
    );
    assert_eq!(fixture.player().ender_chest.active, None);

    fixture.open(second);
    assert_eq!(fixture.player().ender_chest.active, Some(second));
    let menu = fixture.menu();
    let slots = display(&fixture.region, fixture.id, menu);
    assert_eq!(slots[0], ItemStack::new(items::DIAMOND, 2));
}

/// 改了名的箱子打开时标题就是那个名字，官方 `Nameable.getDisplayName`。
#[test]
fn a_named_chest_opens_with_its_name_as_the_title() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    let name = Component::text("小猫的箱子");
    name_block(&mut fixture, pos, &name);
    fixture.open(pos);

    let frames = fixture.take_frames();
    let open_frame = frames
        .iter()
        .find(|frame| frame.id == clientbound::OPEN_SCREEN)
        .expect("open screen");
    let mut open = open_frame.reader();
    assert_eq!(open.varint(), Ok(1));
    assert_eq!(open.varint(), Ok(menu_types::GENERIC_9X3));
    // 标题是一段 NBT 文本组件，内容就是那个名字。
    let mut title = Vec::new();
    name.write(&mut title);
    assert_eq!(open.rest(), title);
}

/// 双箱的标题先看主箱，再看另一半，官方 `ChestBlock.MENU_PROVIDER_COMBINER`。
#[test]
fn a_double_chest_takes_the_name_of_either_half() {
    let mut fixture = Fixture::new();
    let (left, right) = double_chest(&mut fixture);
    let name = Component::text("大箱子");
    name_block(&mut fixture, left, &name);

    fixture.open(right);
    let frames = fixture.take_frames();
    let open_frame = frames
        .iter()
        .find(|frame| frame.id == clientbound::OPEN_SCREEN)
        .expect("open screen");
    let mut open = open_frame.reader();
    assert_eq!(open.varint(), Ok(1));
    assert_eq!(open.varint(), Ok(menu_types::GENERIC_9X6));
    let mut title = Vec::new();
    name.write(&mut title);
    assert_eq!(open.rest(), title);
}

/// 放一个双箱，返回左右两半的位置。主箱（官方的 FIRST）是 `RIGHT` 那一半。
fn double_chest(fixture: &mut Fixture) -> (BlockPos, BlockPos) {
    let y = flat::surface_y();
    let right = BlockPos::new(0, y, 0);
    let left = BlockPos::new(-1, y, 0);
    fixture.place(
        right,
        BlockState(blocks::CHEST)
            .with(HORIZONTAL_FACING, Direction::North)
            .with(CHEST_TYPE, ChestType::Right)
            .0,
    );
    fixture.place(
        left,
        BlockState(blocks::CHEST)
            .with(HORIZONTAL_FACING, Direction::North)
            .with(CHEST_TYPE, ChestType::Left)
            .0,
    );
    (left, right)
}

/// 给这一格的方块实体一个自定义名，和放下一个改过名的方块一样。
fn name_block(fixture: &mut Fixture, pos: BlockPos, name: &Component) {
    let mut patch = mc_bamboo_protocol::components::ComponentPatch::new();
    patch.put(
        mc_bamboo_data::registries::data_component_type::CUSTOM_NAME,
        mc_bamboo_protocol::components::PatchEntry::Set(
            mc_bamboo_protocol::components::ComponentValue::CustomName(name.clone()),
        ),
    );
    fixture.region.block_items.insert(pos, patch);
}

/// 堆叠上限来自组件：改小上限的物品叠不到一起。
#[test]
fn the_stack_limit_comes_from_the_components() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);

    // 把上限改成 2，官方 `Slot.getMaxStackSize` 取的就是这个值。
    let mut limited = ItemStack::new(items::STONE, 2);
    limited.added.push((
        mc_bamboo_data::registries::data_component_type::MAX_STACK_SIZE,
        vec![2],
    ));
    *fixture.inventory_slot(36) = limited.clone();
    container::entry_at(&mut fixture.region, pos, ContainerKind::Chest).slots[0] = limited.clone();

    // 拿起两个再点进已经满了的那一格：放不进去，光标里的东西不变。
    fixture.click(Fixture::hotbar(0), 0, ContainerInput::Pickup);
    fixture.click(0, 0, ContainerInput::Pickup);
    assert_eq!(fixture.menu().carried, limited);
    assert_eq!(fixture.entry(pos).slots[0], limited);

    // 同一个物品不改上限的时候就能叠到 4 个。
    let plain = ItemStack::new(items::STONE, 2);
    fixture.menu_mut().carried = plain.clone();
    fixture.entry_mut(pos).slots[0] = plain;
    fixture.click(0, 0, ContainerInput::Pickup);
    assert_eq!(fixture.entry(pos).slots[0], ItemStack::new(items::STONE, 4));
    assert!(fixture.menu().carried.is_empty());
}

/// 组件不同的物品不是同一种东西，官方 `isSameItemSameComponents`。
#[test]
fn stacks_with_different_components_do_not_merge() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);

    let mut named = ItemStack::new(items::STONE, 1);
    let mut name = Vec::new();
    Component::text("Kitten").write(&mut name);
    named.added.push((
        mc_bamboo_data::registries::data_component_type::CUSTOM_NAME,
        name,
    ));
    container::entry_at(&mut fixture.region, pos, ContainerKind::Chest).slots[0] = named.clone();
    fixture.menu_mut().carried = ItemStack::new(items::STONE, 1);

    // 点在改过名的那一格上：两边换位置，不合并。
    fixture.click(0, 0, ContainerInput::Pickup);
    assert_eq!(fixture.entry(pos).slots[0], ItemStack::new(items::STONE, 1));
    assert_eq!(fixture.menu().carried, named);
}

/// 潜影盒里放不进潜影盒。
#[test]
fn a_shulker_box_refuses_another_shulker_box() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::SHULKER_BOX);
    fixture.open(pos);
    *fixture.inventory_slot(36) = ItemStack::new(items::SHULKER_BOX, 1);
    fixture.click(Fixture::hotbar(0), 0, ContainerInput::Pickup);
    fixture.click(0, 0, ContainerInput::Pickup);

    assert!(fixture.entry(pos).slots[0].is_empty());
    assert_eq!(
        fixture.menu().carried,
        ItemStack::new(items::SHULKER_BOX, 1)
    );

    // 同样的槽，石头就能放进去。
    fixture.menu_mut().carried = ItemStack::new(items::STONE, 1);
    fixture.click(0, 0, ContainerInput::Pickup);
    assert_eq!(fixture.entry(pos).slots[0], ItemStack::new(items::STONE, 1));
}

impl Fixture {
    fn menu_mut(&mut self) -> &mut Menu {
        self.region.menus.get_mut(&self.id).expect("菜单应该开着")
    }
}

/// 够不到的时候点不动，菜单下一 tick 也会关掉。
#[test]
fn walking_away_closes_the_menu() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture.open(pos);
    fixture.take_frames();

    fixture.region.move_player(
        fixture.id,
        Vec3::new(100.0, f64::from(flat::surface_y()), 100.0),
    );
    tick(&mut fixture.region);

    assert!(!fixture.region.menus.contains_key(&fixture.id));
    assert_eq!(fixture.take_ids(), [clientbound::CONTAINER_CLOSE]);
}

/// 合成器打开 `CrafterMenu`：46 格（九格材料、36 格物品栏、一个只读结果格）
/// 和 10 个数据槽（九个禁用位加 `triggered`）。
#[test]
fn opening_a_crafter_sends_the_screen_content_and_ten_data_slots() {
    use mc_bamboo_blocks::state::{FrontAndTop, ORIENTATION, TRIGGERED};

    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    let state = BlockState(blocks::CRAFTER)
        .with(ORIENTATION, FrontAndTop::NorthUp)
        .with(TRIGGERED, true);
    fixture.place(pos, state.0);
    fixture
        .region
        .redstone
        .set_crafter_disabled(pos, 1 << 3 | 1 << 7);
    container::entry_at(&mut fixture.region, pos, ContainerKind::Crafter).slots[0] =
        ItemStack::new(items::OAK_LOG, 1);
    assert!(open_block(&mut fixture.region, fixture.id, pos));

    let frames = fixture.take_frames();
    let ids: Vec<i32> = frames.iter().map(|frame| frame.id).collect();
    assert_eq!(ids[0], clientbound::OPEN_SCREEN);
    assert_eq!(ids[1], clientbound::CONTAINER_SET_CONTENT);
    assert_eq!(ids.len(), 12, "open_screen、content 加 10 个 data");
    assert!(
        ids[2..]
            .iter()
            .all(|&id| id == clientbound::CONTAINER_SET_DATA)
    );

    let mut open = frames[0].reader();
    assert_eq!(open.varint(), Ok(1));
    assert_eq!(open.varint(), Ok(menu_types::CRAFTER_3X3));

    let mut content = frames[1].reader();
    assert_eq!(content.varint(), Ok(1));
    assert_eq!(content.varint(), Ok(1), "打开的 state_id 是 1");
    assert_eq!(content.varint(), Ok(46), "九格加 36 格加结果格");
    assert_eq!(content.varint(), Ok(1), "第一格有一件物品");
    assert_eq!(content.varint(), Ok(items::OAK_LOG), "第一格是原木");

    // 数据槽按顺序发，禁用位是 1，triggered 是 1。id 和 value 是 short。
    for (index, frame) in frames[2..].iter().enumerate() {
        let mut reader = frame.reader();
        assert_eq!(reader.varint(), Ok(1), "container_id");
        assert_eq!(reader.i16(), Ok(index as i16), "数据槽下标");
        let expected = i16::from(index == 3 || index == 7 || index == 9);
        assert_eq!(reader.i16(), Ok(expected), "数据槽 {index} 的值");
    }
}

/// `container_slot_state_changed` 只有打开着这个合成器菜单、非旁观者、container_id 对、
/// 下标在 0..9 且这一格是空的才生效；`new_state=false` 是禁用。
#[test]
fn a_slot_state_packet_only_toggles_empty_slots_of_the_open_crafter() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CRAFTER);
    assert!(open_block(&mut fixture.region, fixture.id, pos));
    fixture.take_frames();
    container::entry_at(&mut fixture.region, pos, ContainerKind::Crafter).slots[0] =
        ItemStack::new(items::STONE, 1);
    let id = fixture.menu().container_id;

    // 有东西的格子不能禁用。
    on_slot_state_changed(&mut fixture.region, fixture.id, id, 0, false);
    assert_eq!(fixture.region.redstone.crafter_disabled(pos), 0);

    // 越界下标和错误的 container_id 都不动。
    on_slot_state_changed(&mut fixture.region, fixture.id, id, 9, false);
    on_slot_state_changed(&mut fixture.region, fixture.id, id, -1, false);
    on_slot_state_changed(&mut fixture.region, fixture.id, id + 1, 4, false);
    assert_eq!(fixture.region.redstone.crafter_disabled(pos), 0);

    // 空格可以禁用，再启用。
    on_slot_state_changed(&mut fixture.region, fixture.id, id, 4, false);
    assert_eq!(fixture.region.redstone.crafter_disabled(pos), 1 << 4);
    tick(&mut fixture.region);
    let data: Vec<(i16, i16)> = fixture
        .take_frames()
        .iter()
        .filter(|frame| frame.id == clientbound::CONTAINER_SET_DATA)
        .map(|frame| {
            let mut reader = frame.reader();
            let _container = reader.varint().unwrap();
            let slot = reader.i16().unwrap();
            (slot, reader.i16().unwrap())
        })
        .collect();
    assert_eq!(data, vec![(4, 1)]);
    on_slot_state_changed(&mut fixture.region, fixture.id, id, 4, true);
    assert_eq!(fixture.region.redstone.crafter_disabled(pos), 0);
}

/// 结果格取不出东西；九格变化时它会跟着刷新。
#[test]
fn a_crafter_result_slot_is_display_only() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CRAFTER);
    assert!(open_block(&mut fixture.region, fixture.id, pos));
    fixture.take_frames();
    container::entry_at(&mut fixture.region, pos, ContainerKind::Crafter).slots[0] =
        ItemStack::new(items::OAK_LOG, 1);
    tick(&mut fixture.region);

    let frames = fixture.take_frames();
    let slots: Vec<i16> = frames
        .iter()
        .filter(|frame| frame.id == clientbound::CONTAINER_SET_SLOT)
        .map(|frame| {
            let mut reader = frame.reader();
            let _container = reader.varint().unwrap();
            let _state = reader.varint().unwrap();
            reader.i16().unwrap()
        })
        .collect();
    assert!(slots.contains(&0), "第一格同步给客户端");
    assert!(slots.contains(&45), "结果格同步给客户端");

    // 点结果格什么都拿不到。
    fixture.click(45, 0, ContainerInput::Pickup);
    assert!(fixture.menu().carried.is_empty());
    assert_eq!(
        fixture.entry(pos).slots[0],
        ItemStack::new(items::OAK_LOG, 1)
    );
}

/// 旁观者和没开着合成器菜单的玩家发的 `container_slot_state_changed` 一律丢掉。
#[test]
fn a_slot_state_packet_from_a_spectator_or_without_a_crafter_menu_is_ignored() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CRAFTER);

    // 菜单没开时不生效。
    on_slot_state_changed(&mut fixture.region, fixture.id, 1, 4, false);
    assert_eq!(fixture.region.redstone.crafter_disabled(pos), 0);

    assert!(open_block(&mut fixture.region, fixture.id, pos));
    fixture.take_frames();
    let id = fixture.menu().container_id;
    fixture
        .region
        .players
        .get_mut(&fixture.id)
        .unwrap()
        .game_mode = GameMode::Spectator;
    on_slot_state_changed(&mut fixture.region, fixture.id, id, 4, false);
    assert_eq!(fixture.region.redstone.crafter_disabled(pos), 0);

    fixture
        .region
        .players
        .get_mut(&fixture.id)
        .unwrap()
        .game_mode = GameMode::Survival;
    on_slot_state_changed(&mut fixture.region, fixture.id, id, 4, false);
    assert_eq!(fixture.region.redstone.crafter_disabled(pos), 1 << 4);
}

/// 官方 `createMenu` 打开带战利品表的箱子时解包，槽位里出现表里的东西。
#[test]
fn opening_a_loot_chest_fills_its_slots_before_syncing_them() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    crate::container_loot::set(
        &mut fixture.region.containers,
        pos,
        ContainerKind::Chest,
        Some(crate::container_loot::ContainerLoot {
            table: "minecraft:chests/simple_dungeon".to_owned(),
            seed: 42,
        }),
    );
    fixture.open(pos);
    let entry = fixture.entry(pos);
    assert!(entry.loot_table.is_none(), "表已经解包");
    assert_eq!(entry.loot_table_seed, 0);
    let filled = entry.slots.iter().filter(|slot| !slot.is_empty()).count();
    assert!(filled > 0, "表里至少有一件东西");
    let menu = fixture.menu();
    let shown = display(&fixture.region, fixture.id, menu);
    assert_eq!(&shown[..entry.slots.len()], entry.slots.as_slice());
}

/// 旁观者能打开，但官方 `canOpen` 在有表时不让他解包。
#[test]
fn a_spectator_does_not_unpack_a_loot_chest() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    fixture
        .region
        .players
        .get_mut(&fixture.id)
        .unwrap()
        .game_mode = GameMode::Spectator;
    crate::container_loot::set(
        &mut fixture.region.containers,
        pos,
        ContainerKind::Chest,
        Some(crate::container_loot::ContainerLoot {
            table: "minecraft:chests/simple_dungeon".to_owned(),
            seed: 42,
        }),
    );
    fixture.open(pos);
    let entry = fixture.entry(pos);
    assert_eq!(
        entry.loot_table.as_deref(),
        Some("minecraft:chests/simple_dungeon")
    );
    assert!(entry.slots.iter().all(ItemStack::is_empty));
}

/// 同一个种子在两个空箱子里得到同样的内容；解包之后的表不再求值。
#[test]
fn the_same_loot_seed_repeats_and_unpacks_only_once() {
    let mut fixture = Fixture::new();
    let a = Fixture::chest_pos();
    let b = BlockPos::new(2, a.y, 0);
    for pos in [a, b] {
        fixture.place(pos, blocks::CHEST);
        crate::container_loot::set(
            &mut fixture.region.containers,
            pos,
            ContainerKind::Chest,
            Some(crate::container_loot::ContainerLoot {
                table: "minecraft:chests/simple_dungeon".to_owned(),
                seed: 12345,
            }),
        );
        crate::container_loot::unpack_at(&mut fixture.region, pos);
    }
    let first = fixture.entry(a).slots.clone();
    assert_eq!(fixture.entry(b).slots, first);

    // 表已经清掉，再解包不会改写内容。
    crate::container_loot::unpack_at(&mut fixture.region, a);
    assert_eq!(fixture.entry(a).slots, first);
}

/// 漏斗和比较器读同一格时先解包：Bamboo 的 `Level` 钩子从这里走。
#[test]
fn a_comparator_read_unpacks_the_loot_table() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    crate::container_loot::set(
        &mut fixture.region.containers,
        pos,
        ContainerKind::Chest,
        Some(crate::container_loot::ContainerLoot {
            table: "minecraft:chests/simple_dungeon".to_owned(),
            seed: 7,
        }),
    );
    let (mut level, _ticks) = fixture.region.block_level();
    mc_bamboo_blocks::Level::unpack_loot_table(&mut level, pos);
    assert!(fixture.entry(pos).loot_table.is_none());
    assert!(
        fixture
            .entry(pos)
            .slots
            .iter()
            .any(|stack| !stack.is_empty())
    );
}

/// 物品上的 `container_loot` 组件放下后留在方块实体上，解包前中键还给物品。
#[test]
fn a_placed_container_loot_component_survives_until_the_first_read() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::CHEST);
    let mut stack = ItemStack::new(mc_bamboo_data::items::CHEST, 1);
    stack.added.push((
        mc_bamboo_data::registries::data_component_type::CONTAINER_LOOT,
        {
            let tag = mc_bamboo_nbt::Tag::Compound(vec![
                (
                    "loot_table".to_owned(),
                    mc_bamboo_nbt::Tag::String("minecraft:chests/simple_dungeon".to_owned()),
                ),
                ("seed".to_owned(), mc_bamboo_nbt::Tag::Long(9)),
            ]);
            let mut bytes = Vec::new();
            tag.write_network(&mut bytes);
            bytes
        },
    ));
    crate::block_items::apply(&mut fixture.region, pos, fixture.id, &stack);
    assert_eq!(
        fixture.entry(pos).loot_table.as_deref(),
        Some("minecraft:chests/simple_dungeon")
    );
    let collected = crate::block_items::collect(&fixture.region, pos);
    assert!(matches!(
        collected.get(mc_bamboo_data::registries::data_component_type::CONTAINER_LOOT),
        Some(mc_bamboo_protocol::components::ComponentValue::ContainerLoot(_))
    ));
    crate::container_loot::unpack_at(&mut fixture.region, pos);
    let collected = crate::block_items::collect(&fixture.region, pos);
    assert!(
        collected
            .get(mc_bamboo_data::registries::data_component_type::CONTAINER_LOOT)
            .is_none()
    );
}

/// 官方 `Block.dropResources` 把 `BLOCK_ENTITY` 传进战利品表：装了东西、改过名的
/// 潜影盒掉出来时带 `container` 和 `custom_name`。
#[test]
fn breaking_a_named_shulker_box_keeps_its_contents_and_name() {
    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::SHULKER_BOX);
    let entry = crate::container::entry_in(
        &mut fixture.region.containers,
        pos,
        ContainerKind::ShulkerBox,
    );
    entry.slots[0] = ItemStack::new(items::DIAMOND, 7);
    let name = mc_bamboo_protocol::text::Component::text("宝藏".to_owned());
    fixture.region.block_items.insert(pos, {
        let mut patch = mc_bamboo_protocol::components::ComponentPatch::new();
        patch.put(
            mc_bamboo_data::registries::data_component_type::CUSTOM_NAME,
            mc_bamboo_protocol::components::PatchEntry::Set(
                mc_bamboo_protocol::components::ComponentValue::CustomName(name),
            ),
        );
        patch
    });
    let block_entity = crate::block_drops::collect_from_region(&fixture.region, pos)
        .expect("潜影盒的组件要能收集");
    let json = &block_entity.components;
    assert!(json.contains_key("minecraft:container"));
    assert!(json.contains_key("minecraft:custom_name"));

    let view = crate::block_drops::ChunkLevel {
        chunks: &fixture.region.chunks,
        weather: fixture.region.global.block_weather,
        global: None,
    };
    let stacks = mc_bamboo_loot::block_drops(
        &mc_bamboo_loot::BlockDropParams {
            state: BlockState(blocks::SHULKER_BOX),
            pos: (pos.x, pos.y, pos.z),
            level: &view,
            block_entity: Some(block_entity),
            breaker: None,
            tool: None,
            explosion_radius: None,
            luck: 0.0,
        },
        Some(&fixture.region.loot_sequences),
        fixture.region.random.source(),
    )
    .expect("潜影盒的战利品表能求值");
    assert_eq!(stacks.len(), 1);
    let stack = crate::loot_stacks::loot_stack(&stacks[0]);
    assert_eq!(stack.item, items::SHULKER_BOX);
    let patch = stack.patch();
    assert!(
        patch
            .get(mc_bamboo_data::registries::data_component_type::CONTAINER)
            .is_some()
    );
    assert!(
        patch
            .get(mc_bamboo_data::registries::data_component_type::CUSTOM_NAME)
            .is_some()
    );
    // 内容确实带上了，不是空盒子。
    let mc_bamboo_protocol::components::ComponentValue::Container(slots) = patch
        .get(mc_bamboo_data::registries::data_component_type::CONTAINER)
        .expect("容器组件")
    else {
        panic!("container 组件类型不对");
    };
    let held = slots[0].as_ref().expect("第 0 格有东西");
    assert_eq!(held.item, items::DIAMOND);
    assert_eq!(held.count, 7);
}

/// 旗帜和陶罐的方块实体组件也按 `collectImplicitComponents` 传给 `copy_components`。
#[test]
fn banners_and_pots_copy_their_block_entity_components() {
    let mut fixture = Fixture::new();
    let banner = Fixture::chest_pos();
    let pot = BlockPos::new(3, banner.y, 0);
    fixture.place(banner, blocks::WHITE_BANNER);
    fixture.place(pot, blocks::DECORATED_POT);
    let patterns = mc_bamboo_protocol::components::ComponentValue::BannerPatterns(vec![]);
    fixture.region.block_items.insert(banner, {
        let mut patch = mc_bamboo_protocol::components::ComponentPatch::new();
        patch.put(
            mc_bamboo_data::registries::data_component_type::BANNER_PATTERNS,
            mc_bamboo_protocol::components::PatchEntry::Set(patterns),
        );
        patch
    });
    fixture.region.block_items.insert(pot, {
        let mut patch = mc_bamboo_protocol::components::ComponentPatch::new();
        patch.put(
            mc_bamboo_data::registries::data_component_type::POT_DECORATIONS,
            mc_bamboo_protocol::components::PatchEntry::Set(
                mc_bamboo_protocol::components::ComponentValue::PotDecorations {
                    back: None,
                    left: None,
                    right: None,
                    front: None,
                },
            ),
        );
        patch
    });
    for (pos, state, name) in [
        (
            banner,
            BlockState(blocks::WHITE_BANNER),
            "minecraft:banner_patterns",
        ),
        (
            pot,
            BlockState(blocks::DECORATED_POT),
            "minecraft:pot_decorations",
        ),
    ] {
        let view = crate::block_drops::ChunkLevel {
            chunks: &fixture.region.chunks,
            weather: fixture.region.global.block_weather,
            global: None,
        };
        let block_entity = crate::block_drops::collect_from_region(&fixture.region, pos);
        let stacks = mc_bamboo_loot::block_drops(
            &mc_bamboo_loot::BlockDropParams {
                state,
                pos: (pos.x, pos.y, pos.z),
                level: &view,
                block_entity,
                breaker: None,
                tool: None,
                explosion_radius: None,
                luck: 0.0,
            },
            Some(&fixture.region.loot_sequences),
            fixture.region.random.source(),
        )
        .unwrap_or_default();
        let stack = stacks
            .first()
            .map(crate::loot_stacks::loot_stack)
            .unwrap_or_else(ItemStack::empty);
        assert!(
            stack.patch().entries().iter().any(|(kind, _)| {
                mc_bamboo_protocol::components::archive::read::id_name(
                    mc_bamboo_protocol::components::cursor::Registry::DATA_COMPONENT_TYPE,
                    *kind,
                )
                .is_ok_and(|found| found == name)
            }),
            "{name} 要复制到掉落物"
        );
    }
}

/// 创造模式挖非空潜影盒掉带内容的盒子，空盒子不掉。
#[test]
fn creative_breaks_only_drop_non_empty_shulker_boxes() {
    for filled in [true, false] {
        let mut fixture = Fixture::new();
        let pos = Fixture::chest_pos();
        fixture.place(pos, blocks::SHULKER_BOX);
        let entry = crate::container::entry_in(
            &mut fixture.region.containers,
            pos,
            ContainerKind::ShulkerBox,
        );
        if filled {
            entry.slots[0] = ItemStack::new(items::EMERALD, 3);
        }
        // 创造模式（`preventsBlockDrops`）空手。
        let actor = mc_bamboo_blocks::Actor {
            uid: 0,
            uuid: [0; 16],
            position: Vec3::new(0.5, f64::from(pos.y) + 2.0, 0.5),
            yaw: 0.0,
            head_yaw: 0.0,
            pitch: 0.0,
            secondary_use: false,
            main_hand: items::AIR,
            off_hand: items::AIR,
            eye_height: 1.62,
            block_interaction_range: 5.0,
            may_build: true,
            prevents_block_drops: true,
            can_use_game_master_blocks: false,
        };
        let (mut level, ticks) = fixture.region.block_level();
        let result = mc_bamboo_blocks::destroy_block(&mut level, ticks, &actor, pos);
        assert_eq!(result.result, mc_bamboo_blocks::InteractionResult::Success);
        let items: Vec<_> = fixture
            .region
            .entities
            .iter()
            .filter_map(|entity| entity.ground().cloned())
            .collect();
        assert_eq!(!items.is_empty(), filled, "filled={filled}");
        if filled {
            let stack = &items[0].stack;
            assert_eq!(stack.item, items::SHULKER_BOX);
            assert!(
                stack
                    .patch()
                    .get(mc_bamboo_data::registries::data_component_type::CONTAINER)
                    .is_some()
            );
        }
    }
}

/// 信标端到端流程：建 4 层铁块金字塔 → 激活拿到进度 → 打开菜单放铁锭确认效果 →
/// 80 刻后获得主/副效果；非法选效果触发断开；关闭时未花掉的付款物品掉在地上。
#[test]
fn beacon_menu_payment_effects_kick_and_advancement() {
    use mc_bamboo_blocks::behaviour::misc::beacon::{BeaconBlockEntity, encode_effect};
    use mc_bamboo_data::registries::mob_effect;

    let mut fixture = Fixture::new();
    let pos = BlockPos::new(0, flat::surface_y() + 4, 0);
    // 4 层铁块金字塔。
    for layer in 1..=4 {
        let cy = pos.y - layer;
        for cx in (pos.x - layer)..=(pos.x + layer) {
            for cz in (pos.z - layer)..=(pos.z + layer) {
                fixture.place(BlockPos::new(cx, cy, cz), blocks::IRON_BLOCK);
            }
        }
    }
    fixture.place(pos, blocks::BEACON);
    fixture
        .region
        .block_entities
        .insert(Box::new(BeaconBlockEntity::new(pos)));
    if let Some(player) = fixture.region.players.get_mut(&fixture.id) {
        player.set_position(
            &mut fixture.region.player_grid,
            Vec3::new(0.5, f64::from(pos.y) + 1.0, 0.5),
        );
        player
            .advancements
            .rebuild_listeners(mc_bamboo_advancements::AdvancementTree::global());
        // 背包第一格（菜单槽 1 = Inventory 槽 9）放 1 个铁锭。
        player.inventory.slots[9] = ItemStack::new(items::IRON_INGOT, 1);
    }

    // 第 1 刻（gameTime = 0）扫描光柱；第 80 刻计算底座层数并激活。
    for t in [0_i64, 80_i64] {
        fixture.region.block_ticks.set_time(t);
        let effects = {
            let (mut level, ticks) = fixture.region.block_level();
            let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
            let _ = world.with_block_entity::<BeaconBlockEntity, _>(pos, |entity, world| {
                entity.server_tick(world);
            });
            world.drain_effects()
        };
        crate::interaction::send_effects(&mut fixture.region, None, &effects);
        crate::advancement_triggers::drain(&mut fixture.region);
    }

    // 确认底座 4 层，并且触发了 create_beacon 与 create_full_beacon 进度。
    let levels = fixture
        .region
        .block_entities
        .get::<BeaconBlockEntity>(pos)
        .unwrap()
        .levels();
    assert_eq!(levels, 4);
    let tree = mc_bamboo_advancements::AdvancementTree::global();
    let player = fixture.region.players.get(&fixture.id).unwrap();
    for adv in [
        "minecraft:nether/create_beacon",
        "minecraft:nether/create_full_beacon",
    ] {
        let idx = tree.index(adv).expect("advancement exists");
        assert!(
            player
                .advancements
                .progress
                .get(&idx)
                .is_some_and(|p| p.is_done()),
            "{adv} should be granted"
        );
    }

    // 打开信标菜单，把铁锭 Shift 进付款槽 0，确认选择 SPEED + REGENERATION。
    fixture.open(pos);
    let container_id = fixture.menu().container_id;
    on_click(
        &mut fixture.region,
        fixture.id,
        ClickPacket {
            container_id,
            state_id: 1,
            slot: 1,
            button: 0,
            input: ContainerInput::QuickMove,
            changed_slots: BTreeMap::new(),
            carried: HashedStack::Empty,
        },
    );
    on_set_beacon(
        &mut fixture.region,
        fixture.id,
        Some(mob_effect::SPEED),
        Some(mob_effect::REGENERATION),
    );
    let beacon = fixture
        .region
        .block_entities
        .get::<BeaconBlockEntity>(pos)
        .unwrap();
    assert_eq!(beacon.primary_power(), Some(mob_effect::SPEED));
    assert_eq!(beacon.secondary_power(), Some(mob_effect::REGENERATION));
    assert_eq!(
        beacon.data_values(),
        [
            4,
            encode_effect(Some(mob_effect::SPEED)),
            encode_effect(Some(mob_effect::REGENERATION)),
        ]
    );
    assert!(!fixture.sink.0.lock().unwrap().closed);

    // 再推进到第 160 刻，玩家拿到 SPEED I 与 REGENERATION I（340 刻）。
    fixture.region.block_ticks.set_time(160);
    let effects = {
        let (mut level, ticks) = fixture.region.block_level();
        let mut world = mc_bamboo_blocks::World::new(&mut level, ticks);
        let _ = world.with_block_entity::<BeaconBlockEntity, _>(pos, |entity, world| {
            entity.server_tick(world);
        });
        world.drain_effects()
    };
    crate::interaction::send_effects(&mut fixture.region, None, &effects);
    let player = fixture.region.players.get(&fixture.id).unwrap();
    let speed = player.effects.get(mob_effect::SPEED).expect("speed effect");
    assert_eq!((speed.amplifier, speed.duration), (0, 340));
    let regen = player
        .effects
        .get(mob_effect::REGENERATION)
        .expect("regeneration effect");
    assert_eq!((regen.amplifier, regen.duration), (0, 340));

    // 关闭时未花掉的付款物品掉在地上。
    if let Some(player) = fixture.region.players.get_mut(&fixture.id) {
        player.inventory.slots[9] = ItemStack::new(items::DIAMOND, 1);
    }
    on_click(
        &mut fixture.region,
        fixture.id,
        ClickPacket {
            container_id,
            state_id: 2,
            slot: 1,
            button: 0,
            input: ContainerInput::QuickMove,
            changed_slots: BTreeMap::new(),
            carried: HashedStack::Empty,
        },
    );
    on_close_request(&mut fixture.region, fixture.id);
    assert!(
        fixture
            .region
            .entities
            .iter()
            .filter_map(|e| e.ground().cloned())
            .any(|item| item.stack.item == items::DIAMOND),
        "closing beacon menu drops unspent payment item"
    );

    // 没放付款物品直接发 SetBeacon：照官方断开连接。
    fixture.open(pos);
    on_set_beacon(
        &mut fixture.region,
        fixture.id,
        Some(mob_effect::HASTE),
        None,
    );
    fixture.region.links.flush();
    assert!(fixture.sink.0.lock().unwrap().closed);
}

/// 带锁与自定义名的信标：无钥匙时拒绝打开并发送 `container.isLocked`，拿钥匙后打开并显示自定义名；
/// 区块存档往返保留 `primary_effect` / `secondary_effect` / `CustomName` / `lock` 且 `Levels` 读盘归零。
#[test]
fn beacon_lock_custom_name_and_chunk_archive_round_trip() {
    use mc_bamboo_blocks::behaviour::misc::beacon::{
        BeaconBlockEntity, DATA_LEVELS, DATA_PRIMARY, DATA_SECONDARY, encode_effect,
    };
    use mc_bamboo_data::registries::{data_component_type as kinds, mob_effect};
    use mc_bamboo_nbt::Tag;
    use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};

    let mut fixture = Fixture::new();
    let pos = Fixture::chest_pos();
    fixture.place(pos, blocks::BEACON);
    let mut beacon = BeaconBlockEntity::new(pos);
    beacon.set_data(DATA_LEVELS, 4, None);
    beacon.set_data(DATA_PRIMARY, encode_effect(Some(mob_effect::SPEED)), None);
    beacon.set_data(
        DATA_SECONDARY,
        encode_effect(Some(mob_effect::REGENERATION)),
        None,
    );
    fixture.region.block_entities.insert(Box::new(beacon));

    let mut patch = ComponentPatch::new();
    patch.put(
        kinds::CUSTOM_NAME,
        PatchEntry::Set(ComponentValue::CustomName(Component::text("My Beacon"))),
    );
    patch.put(
        kinds::LOCK,
        PatchEntry::Set(ComponentValue::Lock(Tag::Compound(vec![(
            "items".to_owned(),
            Tag::String("minecraft:tripwire_hook".to_owned()),
        )]))),
    );
    fixture.region.block_items.insert(pos, patch);
    crate::block_items::refresh_update_tag(&mut fixture.region, pos);
    assert!(fixture.region.chunks.update_tag(pos).is_some());

    // 生存模式空手打开带锁信标：被拒。
    if let Some(player) = fixture.region.players.get_mut(&fixture.id) {
        player.game_mode = GameMode::Survival;
    }
    assert!(!open_block(&mut fixture.region, fixture.id, pos));
    assert!(!fixture.region.menus.contains_key(&fixture.id));

    // 主手拿绊线钩：能打开，且 OpenScreen 带自定义名。
    if let Some(player) = fixture.region.players.get_mut(&fixture.id) {
        player.inventory.slots[HOTBAR_START] = ItemStack::new(items::TRIPWIRE_HOOK, 1);
    }
    assert!(open_block(&mut fixture.region, fixture.id, pos));
    assert!(fixture.region.menus.contains_key(&fixture.id));
    on_close_request(&mut fixture.region, fixture.id);

    // 整块区块存档往返（encode_parts -> decode -> encode）。
    let chunk_pos = pos.chunk();
    let parts = crate::region::content_snapshot::BorrowedParts::build(&fixture.region, chunk_pos)
        .expect("snapshot chunk");
    let encoded = crate::region::chunk_content::encode_parts(&parts, chunk_pos).expect("encode");
    let decoded =
        crate::region::chunk_content::decode(chunk_pos, &encoded.chunk, encoded.entities.as_ref())
            .expect("decode");
    let reencoded =
        crate::region::chunk_content::encode(&decoded.column, chunk_pos).expect("reencode");

    let find_beacon = |root: &Tag| -> Vec<(String, Tag)> {
        let Tag::Compound(entries) = root else {
            panic!("root is compound");
        };
        let (_, Tag::List(bes)) = entries
            .iter()
            .find(|(k, _)| k == "block_entities")
            .expect("block_entities")
        else {
            panic!("block_entities is list");
        };
        bes.iter()
            .find_map(|tag| {
                let Tag::Compound(fields) = tag else {
                    return None;
                };
                let is_beacon = fields
                    .iter()
                    .any(|(k, v)| k == "id" && v == &Tag::String("minecraft:beacon".to_owned()));
                is_beacon.then(|| fields.clone())
            })
            .expect("beacon entry")
    };

    let first = find_beacon(&encoded.chunk);
    let get = |fields: &[(String, Tag)], key: &str| -> Option<Tag> {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(
        get(&first, "primary_effect"),
        Some(Tag::String("minecraft:speed".to_owned()))
    );
    assert_eq!(
        get(&first, "secondary_effect"),
        Some(Tag::String("minecraft:regeneration".to_owned()))
    );
    assert_eq!(get(&first, "Levels"), Some(Tag::Int(4)));
    assert!(get(&first, "CustomName").is_some());
    assert!(get(&first, "lock").is_some());

    // 读盘后再存盘：效果、自定义名和锁都保留，`Levels` 按官方 `loadAdditional` 重置为 0。
    let second = find_beacon(&reencoded.chunk);
    assert_eq!(
        get(&second, "primary_effect"),
        Some(Tag::String("minecraft:speed".to_owned()))
    );
    assert_eq!(
        get(&second, "secondary_effect"),
        Some(Tag::String("minecraft:regeneration".to_owned()))
    );
    assert_eq!(get(&second, "Levels"), Some(Tag::Int(0)));
    assert_eq!(get(&second, "CustomName"), get(&first, "CustomName"));
    assert_eq!(get(&second, "lock"), get(&first, "lock"));
}

/// 运输船的 27 格菜单：槽位读写落在船实体上，打开时解包战利品表，船没了就失效。
#[test]
fn chest_boat_menu_uses_the_entity_inventory() {
    use mc_bamboo_blocks::entity::{Entity, EntityKind};
    use mc_bamboo_data::registries::entity_type as ids;

    let mut fixture = Fixture::new();
    let position = Vec3::new(0.5, f64::from(flat::surface_y()), 0.5);
    let boat = fixture
        .region
        .entities
        .spawn(Entity::boat(ids::OAK_CHEST_BOAT, position));
    // 往船里放一件东西，并挂一张未解包的战利品表。
    fixture.region.entities.with_mut(boat, |entity| {
        if let EntityKind::Boat(data) = &mut entity.kind {
            let chest = data.chest.as_mut().unwrap();
            chest.items[0] = ItemStack::new(7, 3);
            chest.loot_table = Some("minecraft:chests/simple_dungeon".to_owned());
            chest.loot_table_seed = 42;
        }
    });

    assert!(open_entity_container(&mut fixture.region, fixture.id, boat));
    {
        let menu = fixture.menu();
        assert_eq!(menu.container_size, 27);
        assert_eq!(menu.target.entity_container(), Some(boat));
        assert_eq!(menu.target.positions(), Vec::<BlockPos>::new());
        let slots = display(&fixture.region, fixture.id, menu);
        assert_eq!(slots.len(), 27 + 36);
        assert_eq!(slots[0], ItemStack::new(7, 3));
    }
    // `unpackChestVehicleLootTable(player)`：打开菜单就解包并清表。
    let data = fixture.region.entities.get(boat).unwrap();
    if let EntityKind::Boat(data) = &data.kind {
        let chest = data.chest.as_ref().unwrap();
        assert!(chest.loot_table.is_none());
        assert_eq!(chest.loot_table_seed, 0);
    } else {
        panic!("船实体还在");
    }

    // 点击写回船的 `itemStacks`。
    let mut changed = display(&fixture.region, fixture.id, fixture.menu());
    changed[0] = ItemStack::new(9, 5);
    let menu = fixture.region.menus.remove(&fixture.id).unwrap();
    write_back_containers(&mut fixture.region, &menu, &changed);
    fixture.region.menus.insert(fixture.id, menu);
    let data = fixture.region.entities.get(boat).unwrap();
    if let EntityKind::Boat(data) = &data.kind {
        assert_eq!(data.chest.as_ref().unwrap().items[0], ItemStack::new(9, 5));
    } else {
        panic!("船实体还在");
    }

    // `isChestVehicleStillValid`：玩家在范围内为真，船被移除后为假。
    assert!(still_valid(&fixture.region, fixture.id, fixture.menu()));
    fixture
        .region
        .entities
        .with_mut(boat, |entity| entity.discard());
    assert!(!still_valid(&fixture.region, fixture.id, fixture.menu()));

    // 普通船没有物品栏菜单。
    let plain = fixture
        .region
        .entities
        .spawn(Entity::boat(ids::OAK_BOAT, position));
    assert!(!open_entity_container(
        &mut fixture.region,
        fixture.id,
        plain
    ));
}

/// 运输矿车（27 格）和漏斗矿车（5 格）共用实体容器菜单，打开时解包战利品表并按各自格数同步。
#[test]
fn chest_and_hopper_minecart_menus_use_entity_inventories() {
    use mc_bamboo_blocks::entity::{Entity, EntityKind, minecart::MinecartKind};

    let mut fixture = Fixture::new();
    let position = Vec3::new(0.5, f64::from(flat::surface_y()) + 0.0625, 0.5);
    let chest_cart = fixture
        .region
        .entities
        .spawn(Entity::minecart(MinecartKind::Chest, position));
    fixture.region.entities.with_mut(chest_cart, |entity| {
        if let EntityKind::Minecart(cart) = &mut entity.kind {
            cart.items[2] = ItemStack::new(7, 4);
            cart.loot_table = Some("minecraft:chests/simple_dungeon".to_owned());
            cart.loot_table_seed = 99;
        }
    });

    assert!(open_entity_container(
        &mut fixture.region,
        fixture.id,
        chest_cart
    ));
    {
        let menu = fixture.menu();
        assert_eq!(menu.kind, ContainerKind::Chest);
        assert_eq!(menu.container_size, 27);
        assert_eq!(menu.target.entity_container(), Some(chest_cart));
        let slots = display(&fixture.region, fixture.id, menu);
        assert_eq!(slots.len(), 27 + 36);
        assert_eq!(slots[2], ItemStack::new(7, 4));
    }
    let data = fixture.region.entities.get(chest_cart).unwrap();
    if let EntityKind::Minecart(cart) = &data.kind {
        assert!(cart.loot_table.is_none());
        assert_eq!(cart.loot_table_seed, 0);
    } else {
        panic!("运输矿车还在");
    }

    // 漏斗矿车开 5 格的漏斗菜单。
    let hopper_cart = fixture
        .region
        .entities
        .spawn(Entity::minecart(MinecartKind::Hopper, position));
    fixture.region.entities.with_mut(hopper_cart, |entity| {
        if let EntityKind::Minecart(cart) = &mut entity.kind {
            cart.items[1] = ItemStack::new(11, 2);
        }
    });
    assert!(open_entity_container(
        &mut fixture.region,
        fixture.id,
        hopper_cart
    ));
    {
        let menu = fixture.menu();
        assert_eq!(menu.kind, ContainerKind::Hopper);
        assert_eq!(menu.container_size, 5);
        assert_eq!(menu.target.entity_container(), Some(hopper_cart));
        let slots = display(&fixture.region, fixture.id, menu);
        assert_eq!(slots.len(), 5 + 36);
        assert_eq!(slots[1], ItemStack::new(11, 2));
    }
    let mut changed = display(&fixture.region, fixture.id, fixture.menu());
    changed[4] = ItemStack::new(12, 6);
    let menu = fixture.region.menus.remove(&fixture.id).unwrap();
    write_back_containers(&mut fixture.region, &menu, &changed);
    fixture.region.menus.insert(fixture.id, menu);
    let data = fixture.region.entities.get(hopper_cart).unwrap();
    if let EntityKind::Minecart(cart) = &data.kind {
        assert_eq!(cart.items[4], ItemStack::new(12, 6));
    } else {
        panic!("漏斗矿车还在");
    }

    // 普通矿车打不开容器菜单。
    let plain = fixture
        .region
        .entities
        .spawn(Entity::minecart(MinecartKind::Rideable, position));
    assert!(!open_entity_container(
        &mut fixture.region,
        fixture.id,
        plain
    ));
}

/// 运输船菜单标题是船实体自己的显示名（`Entity.getDisplayName()`），不是
/// `container.chest`。官方 26.3 `AbstractChestBoat.createMenu` 返回
/// `ChestMenu.threeRows`，`ServerPlayer.openMenu` 用 provider（船）的
/// `getDisplayName()` 发 OpenScreen；船没覆写，就是 `EntityType.getDescriptionId()`。
#[test]
fn chest_boat_menu_uses_the_boat_display_name_as_title() {
    use mc_bamboo_blocks::entity::Entity;
    use mc_bamboo_data::registries::entity_type as ids;

    let mut fixture = Fixture::new();
    let position = Vec3::new(0.5, f64::from(flat::surface_y()), 0.5);
    for (entity_type, key) in [
        (ids::OAK_CHEST_BOAT, "entity.minecraft.oak_chest_boat"),
        (ids::SPRUCE_CHEST_BOAT, "entity.minecraft.spruce_chest_boat"),
        (ids::BAMBOO_CHEST_RAFT, "entity.minecraft.bamboo_chest_raft"),
    ] {
        let boat = fixture
            .region
            .entities
            .spawn(Entity::boat(entity_type, position));
        assert!(open_entity_container(&mut fixture.region, fixture.id, boat));
        let container_id = fixture.menu().container_id;
        let frames = fixture.take_frames();
        let open_frame = frames
            .iter()
            .find(|frame| frame.id == clientbound::OPEN_SCREEN)
            .expect("open screen");
        let mut open = open_frame.reader();
        assert_eq!(open.varint(), Ok(container_id));
        assert_eq!(open.varint(), Ok(menu_types::GENERIC_9X3));
        let expected = crate::vehicle_display_name::styled(
            &fixture.region.global.scoreboard,
            boat,
            ids::name_of(entity_type).unwrap(),
            Component::translate(key, []),
        );
        let mut title = Vec::new();
        expected.write(&mut title);
        assert_eq!(open.rest(), title, "标题应该是船自己的显示名 {key}");
    }
}
