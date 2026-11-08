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
use crate::{PlayerSink, container::ContainerEntry, player::Player};

#[derive(Default)]
struct SinkState {
    bytes: Vec<u8>,
    closed: bool,
}

#[derive(Clone, Default)]
struct TestSink(Arc<Mutex<SinkState>>);

impl PlayerSink for TestSink {
    fn send(&mut self, frames: Vec<u8>) -> bool {
        self.0.lock().unwrap().bytes.extend(frames);
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
    region.players.insert(id, player);
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
    assert_eq!(
        ids,
        [clientbound::OPEN_SCREEN, clientbound::CONTAINER_SET_CONTENT]
    );

    let mut open = frames[0].reader();
    assert_eq!(open.varint(), Ok(1));
    assert_eq!(open.varint(), Ok(menu_types::GENERIC_9X3));

    let mut content = frames[1].reader();
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

    crate::interaction::run_block_ticks(&mut fixture.region, crate::global::GameTime(0));
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
    let mut content = frames[1].reader();
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

/// Q 丢弃，光标空时丢一格里的东西。没有物品实体，东西直接消失。
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
            clientbound::SET_PLAYER_INVENTORY
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
    assert_eq!(fixture.take_ids(), [clientbound::CONTAINER_CLOSE]);
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
    let (slots, sources) = display(&fixture.region, other, menu);
    assert_eq!(slots[0], ItemStack::new(items::STONE, 10));
    assert_eq!(sources[0], Some(fixture.id));
    assert_eq!(fixture.entry(pos).open_count, 2);
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
    let (slots, _) = display(&fixture.region, fixture.id, menu);
    assert_eq!(slots[0], ItemStack::new(items::DIAMOND, 2));
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

    fixture
        .region
        .players
        .get_mut(&fixture.id)
        .unwrap()
        .position = Vec3::new(100.0, f64::from(flat::surface_y()), 100.0);
    tick(&mut fixture.region);

    assert!(!fixture.region.menus.contains_key(&fixture.id));
    assert_eq!(fixture.take_ids(), [clientbound::CONTAINER_CLOSE]);
}
