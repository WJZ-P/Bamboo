//! 方块交互的测试：驱动 `World::tick`，看客户端收到的帧。
//!
//! `blocks-inventory.md` 第 13 章的验收用例能用假客户端表达的都在这里。
//! 两名玩家的测试只看方块相关的包，实体同步的包和这里无关。

use mc_bamboo_blocks::{
    BlockState, InteractionResult, TickPriority,
    state::{AXIS, HAS_BOOK, HORIZONTAL_FACING, LEVEL, SLAB_TYPE, SlabType},
};
use mc_bamboo_core::{Axis, BlockPos, ChunkPos, Direction, Hand, Vec3};
use mc_bamboo_data::{
    block_id_of_state, blocks, items, items::ITEMS, packets::play::clientbound,
    registries::block_entity_type, registries::data_component_type as kinds,
};
use mc_bamboo_protocol::{
    components::{ComponentValue, cursor::HolderSet, predicate::BlockPredicate},
    item::ItemStack,
    packet::{
        ClientboundPacket,
        play::{
            BlockChangedAck, BlockUpdate, Disconnect, GameMode, LevelEvent, PlayRequest,
            PlayerActionKind, SectionBlock, SectionBlocksUpdate, SwingAnimation, SystemChat,
        },
    },
    text::{Color, Component},
};

use crate::{
    chunk_encoding::encode_chunk,
    links::encode_frames,
    player::Player,
    world::tests::{
        Client, Frame, Harness, SPAWN_X, SPAWN_Y, SPAWN_Z, block_changes, body_of, chunk_body,
        chunk_position, client_requesting, frames_with_id,
    },
};

/// 玩家站在 (8.5, -60, 8.5)，这一格在它东边，够得着，又不和玩家重叠。
const GROUND: BlockPos = BlockPos::new(11, -61, 8);
const ABOVE: BlockPos = BlockPos::new(11, -60, 8);
const TWO_ABOVE: BlockPos = BlockPos::new(11, -59, 8);

const TOP_CENTER: (f32, f32, f32) = (0.5, 1.0, 0.5);

fn settled() -> (Harness, Client) {
    let mut server = Harness::new(8);
    let client = server.join_and_settle("Kitten");
    client.take_frames();
    (server, client)
}

/// 同一个世界里的两名玩家，都站在出生点，互相在跟踪范围里。
fn two_players() -> (Harness, Client, Client) {
    let mut server = Harness::new(8);
    let first = server.join_and_settle("Alice");
    let second = server.join_and_settle("Bob");
    first.take_frames();
    second.take_frames();
    (server, first, second)
}

fn frame_of<P: ClientboundPacket>(packet: &P) -> Frame {
    Frame {
        id: P::ID,
        body: body_of(encode_frames(packet)),
    }
}

fn update(pos: BlockPos, state: u32) -> Frame {
    frame_of(&BlockUpdate {
        pos,
        state: state as i32,
    })
}

/// 同一段里改了几格时的那个包，`blocks` 要按官方发送的顺序写。
fn section_update(blocks: &[(BlockPos, u32)]) -> Frame {
    let (first, _) = blocks[0];
    let blocks: Vec<SectionBlock> = blocks
        .iter()
        .map(|&(pos, state)| SectionBlock {
            local: (((pos.x & 15) << 8) | ((pos.z & 15) << 4) | (pos.y & 15)) as u16,
            state: state as i32,
        })
        .collect();
    frame_of(&SectionBlocksUpdate {
        x: first.x >> 4,
        y: first.y >> 4,
        z: first.z >> 4,
        blocks: &blocks,
    })
}

fn ack(sequence: i32) -> Frame {
    frame_of(&BlockChangedAck { sequence })
}

fn build_limit(key: &str, limit: i32) -> Frame {
    let content = Component::translate(key, [limit.into()]).color(Color::Red);
    frame_of(&SystemChat {
        content: &content,
        overlay: true,
    })
}

fn destroyed(pos: BlockPos, state: u32) -> Frame {
    frame_of(&LevelEvent::destroy_block(pos, state as i32))
}

fn swing_of(client: &Client, hand: Hand) -> Frame {
    frame_of(&SwingAnimation::default_swing(client.id.0, hand))
}

fn swings(client: &Client) -> Vec<Frame> {
    frames_with_id(&client.take_frames(), clientbound::SWING_ANIMATION)
        .into_iter()
        .cloned()
        .collect()
}

fn block_frames(client: &Client) -> Vec<Frame> {
    client
        .take_frames()
        .into_iter()
        .filter(|frame| {
            matches!(
                frame.id,
                clientbound::BLOCK_UPDATE
                    | clientbound::SECTION_BLOCKS_UPDATE
                    | clientbound::BLOCK_CHANGED_ACK
                    | clientbound::LEVEL_EVENT
            )
        })
        .collect()
}

fn action(action: PlayerActionKind, pos: BlockPos, sequence: i32) -> PlayRequest {
    PlayRequest::PlayerAction {
        action,
        pos,
        face: Direction::Up,
        sequence,
    }
}

fn destroy(server: &Harness, client: &Client, pos: BlockPos, sequence: i32) {
    let request = action(PlayerActionKind::StartDestroyBlock, pos, sequence);
    server.request(client, request);
}

fn use_item_on(hand: Hand, pos: BlockPos, face: Direction, sequence: i32) -> PlayRequest {
    PlayRequest::UseItemOn {
        hand,
        pos,
        face,
        cursor: TOP_CENTER,
        inside: false,
        world_border_hit: false,
        sequence,
    }
}

fn use_item_at(pos: BlockPos, face: Direction, cursor: (f32, f32, f32)) -> PlayRequest {
    PlayRequest::UseItemOn {
        hand: Hand::Main,
        pos,
        face,
        cursor,
        inside: false,
        world_border_hit: false,
        sequence: 1,
    }
}

/// 主手右键方块的顶面。
fn click_top(server: &Harness, client: &Client, pos: BlockPos, sequence: i32) {
    let request = use_item_on(Hand::Main, pos, Direction::Up, sequence);
    server.request(client, request);
}

fn give(server: &Harness, client: &Client, slot: i16, item: i32) {
    let stack = ItemStack::new(item, 1);
    server.request(client, PlayRequest::SetCreativeModeSlot { slot, stack });
}

/// 放进主手，也就是初始选中的快捷栏第一格。
fn hold(server: &Harness, client: &Client, item: i32) {
    give(server, client, 36, item);
}

fn hold_in_off_hand(server: &Harness, client: &Client, item: i32) {
    give(server, client, 45, item);
}

/// 转身，等派生状态的阶段把头也转过去。
fn look(server: &mut Harness, client: &Client, yaw: f32, pitch: f32) {
    server.request(
        client,
        PlayRequest::MovePlayer {
            position: None,
            rotation: Some((yaw, pitch)),
            on_ground: false,
        },
    );
    server.tick();
    client.take_frames();
}

fn move_player(server: &Harness, client: &Client, x: f64, y: f64, z: f64) {
    server.request(
        client,
        PlayRequest::MovePlayer {
            position: Some(Vec3::new(x, y, z)),
            rotation: None,
            on_ground: false,
        },
    );
}

fn set_block(server: &mut Harness, pos: BlockPos, state: u32) {
    server.region().chunks.set_block(pos, state);
}

fn block_at(server: &mut Harness, pos: BlockPos) -> Option<u32> {
    server.region().chunks.block(pos)
}

fn player_mut<'a>(server: &'a mut Harness, client: &Client) -> &'a mut Player {
    server
        .region()
        .players
        .get_mut(&client.id)
        .expect("玩家应该在世界里")
}

/// 按着潜行键。`player_input` 由实体同步处理，这里直接设状态。
fn press_shift(server: &mut Harness, client: &Client) {
    player_mut(server, client).sync.shift_key_down = true;
}

fn state_at(server: &mut Harness, pos: BlockPos) -> BlockState {
    BlockState(block_at(server, pos).expect("这一格在世界里"))
}

/// 手持 `item` 右键 `pos` 顶面，返回东西是不是放到了 `pos` 上面一格。
fn places_on_top(server: &mut Harness, client: &Client, pos: BlockPos, item: i32) -> bool {
    hold(server, client, item);
    click_top(server, client, pos, 1);
    server.tick();
    client.take_frames();
    block_at(server, pos.relative(Direction::Up)) != Some(blocks::AIR)
}

/// 被点的方块是 `state` 时，手持石头右键顶面能不能放下来。
fn stone_goes_on_top_of(state: u32) -> bool {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, state);
    places_on_top(&mut server, &client, ABOVE, items::STONE)
}

#[test]
fn breaking_tells_everyone_and_the_ack_comes_last() {
    let (mut server, breaker, watcher) = two_players();
    let pos = BlockPos::new(8, -61, 8);

    destroy(&server, &breaker, pos, 1);
    server.tick();

    assert_eq!(block_at(&mut server, pos), Some(blocks::AIR));
    assert_eq!(block_frames(&breaker), [update(pos, blocks::AIR), ack(1)]);
    let frames = block_frames(&watcher);
    assert_eq!(frames.len(), 2);
    // `blocks-inventory.md` 3.2 的实测字节：破坏 (8,-61,8) 的 grass_block，状态 9。
    assert_eq!(frames[0].id, clientbound::LEVEL_EVENT);
    assert_eq!(
        frames[0].body,
        [
            0x00, 0x00, 0x07, 0xd1, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x8f, 0xc3, 0x00, 0x00,
            0x00, 0x09, 0x00
        ]
    );
    assert_eq!(frames[1], update(pos, blocks::AIR));
}

#[test]
fn holding_left_click_breaks_block_after_block_without_ghosts() {
    let (mut server, client) = settled();

    for (sequence, x) in (1..).zip(6..=11) {
        let pos = BlockPos::new(x, -61, 8);
        destroy(&server, &client, pos, sequence);
        server.tick();
        assert_eq!(
            client.take_frames(),
            [update(pos, blocks::AIR), ack(sequence)]
        );
    }
}

#[test]
fn items_that_cannot_destroy_send_the_block_back() {
    let (mut server, client) = settled();
    let unable = ITEMS
        .iter()
        .enumerate()
        .filter(|(_, item)| !item.can_destroy_blocks_in_creative);

    for (item, _) in unable {
        hold(&server, &client, item as i32);
        destroy(&server, &client, GROUND, 1);
        server.tick();

        assert_eq!(block_at(&mut server, GROUND), Some(blocks::GRASS_BLOCK));
        assert_eq!(
            client.take_frames(),
            [update(GROUND, blocks::GRASS_BLOCK), ack(1)]
        );
    }
}

#[test]
fn others_hear_nothing_when_the_block_survives() {
    let (mut server, breaker, watcher) = two_players();
    hold(&server, &breaker, items::DIAMOND_SWORD);

    destroy(&server, &breaker, GROUND, 1);
    server.tick();

    assert!(block_frames(&watcher).is_empty());
}

#[test]
fn game_master_blocks_cannot_be_broken() {
    let game_master_blocks = [
        blocks::COMMAND_BLOCK,
        blocks::CHAIN_COMMAND_BLOCK,
        blocks::REPEATING_COMMAND_BLOCK,
        blocks::STRUCTURE_BLOCK,
        blocks::JIGSAW,
        blocks::TEST_BLOCK,
        blocks::TEST_INSTANCE_BLOCK,
    ];
    let (mut server, client) = settled();

    for state in game_master_blocks {
        set_block(&mut server, ABOVE, state);
        destroy(&server, &client, ABOVE, 1);
        server.tick();

        assert_eq!(block_at(&mut server, ABOVE), Some(state));
        assert_eq!(client.take_frames(), [update(ABOVE, state), ack(1)]);
    }
}

#[test]
fn bedrock_at_the_bottom_of_the_world_can_be_broken() {
    let (mut server, client) = settled();
    let pos = BlockPos::new(8, -64, 8);
    move_player(&server, &client, 8.5, -63.0, 8.5);

    destroy(&server, &client, pos, 1);
    server.tick();

    assert_eq!(block_at(&mut server, pos), Some(blocks::AIR));
    assert_eq!(client.take_frames(), [update(pos, blocks::AIR), ack(1)]);
}

#[test]
fn breaking_exactly_at_the_reach_limit_only_acks() {
    let (mut server, client) = settled();
    // 眼睛在 (9, -58.38, 8.5)，高度和 z 都在这两个方块的范围里，只有 x 有距离。
    move_player(&server, &client, 9.0, -60.0, 8.5);
    let inside = BlockPos::new(14, -59, 8);
    let outside = BlockPos::new(15, -59, 8);
    set_block(&mut server, inside, blocks::STONE);
    set_block(&mut server, outside, blocks::STONE);

    destroy(&server, &client, outside, 1);
    server.tick();
    assert_eq!(block_at(&mut server, outside), Some(blocks::STONE));
    assert_eq!(client.take_frames(), [ack(1)]);

    destroy(&server, &client, inside, 2);
    server.tick();
    assert_eq!(block_at(&mut server, inside), Some(blocks::AIR));
}

#[test]
fn crouching_lowers_the_eyes_but_flying_does_not() {
    let (mut server, client) = settled();
    let pos = BlockPos::new(8, -50, 8);
    set_block(&mut server, pos, blocks::STONE);
    // 站着时眼睛离方块 5.9 格，蹲着时 6.25 格。
    move_player(&server, &client, 8.5, -57.52, 8.5);
    press_shift(&mut server, &client);

    destroy(&server, &client, pos, 1);
    server.tick();
    assert_eq!(block_at(&mut server, pos), Some(blocks::STONE));

    player_mut(&mut server, &client).sync.flying = true;
    destroy(&server, &client, pos, 2);
    server.tick();
    assert_eq!(block_at(&mut server, pos), Some(blocks::AIR));
}

#[test]
fn breaking_air_only_acks() {
    let (mut server, client) = settled();

    destroy(&server, &client, ABOVE, 1);
    server.tick();

    assert_eq!(client.take_frames(), [ack(1)]);
}

#[test]
fn breaking_above_the_world_sends_void_air_back() {
    let (mut server, client) = settled();
    move_player(&server, &client, 8.5, 318.0, 8.5);
    let pos = BlockPos::new(8, 320, 8);

    destroy(&server, &client, pos, 1);
    server.tick();

    assert_eq!(
        client.take_frames(),
        [update(pos, blocks::VOID_AIR), ack(1)]
    );
}

#[test]
fn breaking_below_the_world_only_acks() {
    let (mut server, client) = settled();
    move_player(&server, &client, 8.5, -67.0, 8.5);

    destroy(&server, &client, BlockPos::new(8, -65, 8), 1);
    server.tick();

    assert_eq!(client.take_frames(), [ack(1)]);
}

#[test]
fn level_event_stops_at_64_blocks_from_the_block_corner() {
    let mut server = Harness::new(8);
    let breaker = server.join_and_settle("Alice");
    let near = server.join_and_settle("Bob");
    let far = server.join_and_settle("Carol");
    let pos = BlockPos::new(8, -61, 8);
    move_player(&server, &near, 71.9, -61.0, 8.0);
    move_player(&server, &far, 72.0, -61.0, 8.0);
    server.tick();
    near.take_frames();
    far.take_frames();

    destroy(&server, &breaker, pos, 1);
    server.tick();

    let events =
        |client: &Client| frames_with_id(&client.take_frames(), clientbound::LEVEL_EVENT).len();
    assert_eq!(events(&near), 1);
    assert_eq!(events(&far), 0);
}

#[test]
fn negative_sequence_is_malformed_and_disconnects() {
    let requests = [
        action(PlayerActionKind::StartDestroyBlock, GROUND, -1),
        action(PlayerActionKind::AbortDestroyBlock, GROUND, -1),
        use_item_on(Hand::Main, GROUND, Direction::Up, i32::MIN),
        PlayRequest::UseItem {
            hand: Hand::Main,
            sequence: -2,
            yaw: 0.0,
            pitch: 0.0,
        },
    ];
    let reason = Component::translate("disconnect.packetError", []);
    let kicked = frame_of(&Disconnect { reason: &reason });

    for request in requests {
        let (mut server, client) = settled();
        hold(&server, &client, items::STONE);

        server.request(&client, request);
        server.tick();

        assert_eq!(client.take_frames(), std::slice::from_ref(&kicked));
        assert!(client.is_closed());
        assert_eq!(block_at(&mut server, GROUND), Some(blocks::GRASS_BLOCK));
        assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
    }
}

#[test]
fn only_destroy_actions_have_a_sequence_to_ack() {
    let actions = [
        PlayerActionKind::DropAllItems,
        PlayerActionKind::DropItem,
        PlayerActionKind::ReleaseUseItem,
        PlayerActionKind::SwapItemWithOffhand,
        PlayerActionKind::Stab,
    ];
    let (mut server, client) = settled();

    for kind in actions {
        // 官方只在四个破坏动作里校验 sequence，这里的负数不算畸形。
        server.request(&client, action(kind, GROUND, -1));
        server.request(&client, action(kind, GROUND, 5));
        server.tick();

        assert!(client.take_frames().is_empty());
        assert!(!client.is_closed());
    }
}

#[test]
fn destroy_actions_a_creative_client_never_sends_only_ack() {
    let actions = [
        PlayerActionKind::ChangeDestroyDirection,
        PlayerActionKind::AbortDestroyBlock,
        PlayerActionKind::StopDestroyBlock,
    ];
    let (mut server, client) = settled();

    for kind in actions {
        server.request(&client, action(kind, GROUND, 4));
        server.tick();

        assert_eq!(client.take_frames(), [ack(4)]);
        assert_eq!(block_at(&mut server, GROUND), Some(blocks::GRASS_BLOCK));
    }
}

#[test]
fn official_packets_place_and_break_end_to_end() {
    let (mut server, client) = settled();
    move_player(&server, &client, 10.5, -60.0, 8.5);
    hold(&server, &client, items::STONE);

    // `blocks-inventory.md` 3.1 的实测字节：主手，点 (8,-61,8) 顶面中心，seq 2。
    let place = PlayRequest::decode(&[
        0x42, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x8f, 0xc3, 0x01, 0x3f, 0x00, 0x00, 0x00,
        0x3f, 0x80, 0x00, 0x00, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
    ])
    .unwrap();
    server.request(&client, place);
    server.tick();

    let frames = client.take_frames();
    assert_eq!(frames.len(), 4);
    // 3.2 的实测字节：(8,-60,8) 变成 stone(1)。
    let stone_at_8_60_8 = [0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x8f, 0xc4, 0x01];
    assert_eq!(frames[1].body, stone_at_8_60_8);
    assert_eq!(frames[2].body, stone_at_8_60_8);
    assert_eq!(frames[3], ack(2));

    // 3.1 的实测字节：START_DESTROY_BLOCK，(8,-61,8)，UP，seq 1。
    let destroy = PlayRequest::decode(&[
        0x29, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x8f, 0xc3, 0x01, 0x01,
    ])
    .unwrap();
    server.request(&client, destroy);
    server.tick();

    assert_eq!(
        block_at(&mut server, BlockPos::new(8, -61, 8)),
        Some(blocks::AIR)
    );
}

#[test]
fn placing_sends_both_blocks_then_the_change_then_the_ack() {
    let (mut server, client) = settled();
    hold(&server, &client, items::STONE);

    click_top(&server, &client, GROUND, 2);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::STONE));
    assert_eq!(
        client.take_frames(),
        [
            update(GROUND, blocks::GRASS_BLOCK),
            update(ABOVE, blocks::STONE),
            update(ABOVE, blocks::STONE),
            ack(2),
        ]
    );
}

#[test]
fn others_only_get_the_change_of_a_placed_block() {
    let (mut server, placer, watcher) = two_players();
    hold(&server, &placer, items::STONE);

    click_top(&server, &placer, GROUND, 2);
    server.tick();

    assert_eq!(block_frames(&watcher), [update(ABOVE, blocks::STONE)]);
}

#[test]
fn placing_does_not_use_up_the_item() {
    let (mut server, client) = settled();
    hold(&server, &client, items::STONE);

    click_top(&server, &client, GROUND, 1);
    server.tick();

    assert_eq!(
        server.player(&client).inventory.slots[36],
        ItemStack::new(items::STONE, 1)
    );
}

#[test]
fn block_changed_twice_in_a_tick_is_broadcast_once() {
    let (mut server, client) = settled();
    hold(&server, &client, items::STONE);

    click_top(&server, &client, GROUND, 1);
    destroy(&server, &client, ABOVE, 2);
    server.tick();

    let frames = client.take_frames();
    assert_eq!(
        frames[2..],
        [
            // 放置和破坏都发生在第 5 阶段之前，广播的是这一 tick 结束时的状态。
            update(ABOVE, blocks::AIR),
            ack(2),
        ]
    );
}

#[test]
fn stone_replaces_short_grass_where_it_was_clicked() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::SHORT_GRASS);
    hold(&server, &client, items::STONE);

    click_top(&server, &client, ABOVE, 1);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::STONE));
    assert_eq!(block_at(&mut server, TWO_ABOVE), Some(blocks::AIR));
}

#[test]
fn short_grass_does_not_replace_short_grass() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::SHORT_GRASS);
    hold(&server, &client, items::SHORT_GRASS);

    click_top(&server, &client, ABOVE, 1);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::SHORT_GRASS));
}

#[test]
fn clicking_the_side_of_a_block_places_outside_that_face() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::STONE);
    hold(&server, &client, items::DIRT);

    let east = ABOVE.relative(Direction::East);
    let request = use_item_on(Hand::Main, ABOVE, Direction::East, 1);
    server.request(&client, request);
    server.tick();

    assert_eq!(block_at(&mut server, east), Some(blocks::DIRT));
    assert_eq!(
        client.take_frames(),
        [
            update(ABOVE, blocks::STONE),
            update(east, blocks::DIRT),
            update(east, blocks::DIRT),
            ack(1),
        ]
    );
}

#[test]
fn relative_position_can_replace_too() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::SHORT_GRASS);
    hold(&server, &client, items::STONE);

    click_top(&server, &client, GROUND, 1);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::STONE));
}

#[test]
fn occupied_target_fails_but_both_blocks_are_sent() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::OAK_PLANKS);
    hold(&server, &client, items::STONE);

    click_top(&server, &client, GROUND, 1);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::OAK_PLANKS));
    assert_eq!(
        client.take_frames(),
        [
            update(GROUND, blocks::GRASS_BLOCK),
            update(ABOVE, blocks::OAK_PLANKS),
            ack(1),
        ]
    );
}

#[test]
fn other_items_replace_sculk_vein_even_though_the_tag_lacks_it() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::SCULK_VEIN);
    hold(&server, &client, items::STONE);

    click_top(&server, &client, ABOVE, 1);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::STONE));
    assert_eq!(block_at(&mut server, TWO_ABOVE), Some(blocks::AIR));
}

#[test]
fn a_solid_block_cannot_go_into_the_player_placing_it() {
    let (mut server, client) = settled();
    let ground = BlockPos::new(8, -61, 8);
    let feet = BlockPos::new(8, -60, 8);
    hold(&server, &client, items::STONE);

    click_top(&server, &client, ground, 1);
    server.tick();

    assert_eq!(block_at(&mut server, feet), Some(blocks::AIR));
    assert_eq!(
        client.take_frames(),
        [
            update(ground, blocks::GRASS_BLOCK),
            update(feet, blocks::AIR),
            ack(1),
        ]
    );
}

#[test]
fn a_solid_block_cannot_go_into_another_player() {
    let (mut server, placer, standing) = two_players();
    move_player(&server, &standing, 11.5, SPAWN_Y, 8.5);
    hold(&server, &placer, items::STONE);

    click_top(&server, &placer, GROUND, 1);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
}

#[test]
fn a_flower_can_go_into_a_player() {
    let (mut server, client) = settled();
    let feet = BlockPos::new(8, -60, 8);
    hold(&server, &client, items::POPPY);

    click_top(&server, &client, BlockPos::new(8, -61, 8), 1);
    server.tick();

    assert_eq!(block_at(&mut server, feet), Some(blocks::POPPY));
}

#[test]
fn touching_the_side_of_a_player_is_not_an_overlap() {
    let (mut server, client) = settled();
    // 碰撞盒从 x = 11.0 开始，正好贴着 x = 10 这一格的东面。
    move_player(&server, &client, 11.3, SPAWN_Y, 8.5);
    hold(&server, &client, items::STONE);

    click_top(&server, &client, BlockPos::new(10, -61, 8), 1);
    server.tick();

    assert_eq!(
        block_at(&mut server, BlockPos::new(10, -60, 8)),
        Some(blocks::STONE)
    );
}

#[test]
fn only_a_real_crouch_shortens_the_player() {
    let (mut server, client) = settled();
    let target = BlockPos::new(8, -58, 8);
    let ceiling = target.relative(Direction::Up);
    set_block(&mut server, ceiling, blocks::STONE);
    // 站着时头顶到 -57.9，伸进目标格 0.1。蹲着时到 -58.2，碰不到。
    move_player(&server, &client, 8.5, -59.7, 8.5);
    hold(&server, &client, items::DIRT);
    let place = use_item_on(Hand::Main, ceiling, Direction::Down, 1);

    server.request(&client, place.clone());
    server.tick();
    assert_eq!(block_at(&mut server, target), Some(blocks::AIR));

    // 飞行时按潜行键是下降，姿势还是站着。
    press_shift(&mut server, &client);
    player_mut(&mut server, &client).sync.flying = true;
    server.request(&client, place.clone());
    server.tick();
    assert_eq!(block_at(&mut server, target), Some(blocks::AIR));

    player_mut(&mut server, &client).sync.flying = false;
    server.request(&client, place);
    server.tick();
    assert_eq!(block_at(&mut server, target), Some(blocks::DIRT));
}

#[test]
fn the_top_of_the_world_refuses_with_a_message() {
    let (mut server, client) = settled();
    let top = BlockPos::new(8, 319, 8);
    set_block(&mut server, top, blocks::STONE);
    move_player(&server, &client, 10.5, 317.0, 8.5);
    hold(&server, &client, items::DIRT);

    click_top(&server, &client, top, 1);
    server.tick();

    // 上面一格在世界外，所以只有一个 `block_update`。
    assert_eq!(
        client.take_frames(),
        [
            build_limit("build.tooHigh", 319),
            update(top, blocks::STONE),
            ack(1),
        ]
    );
}

#[test]
fn too_high_message_matches_the_official_content() {
    let frame = build_limit("build.tooHigh", 319);

    assert_eq!(frame.id, clientbound::SYSTEM_CHAT);
    // `blocks-inventory.md` 3.2 的实测字节，只是复合标签里的键换了顺序。
    let mut expected = vec![0x0a];
    expected.extend([0x08, 0x00, 0x09]);
    expected.extend(b"translate");
    expected.extend([0x00, 0x0d]);
    expected.extend(b"build.tooHigh");
    expected.extend([0x09, 0x00, 0x04]);
    expected.extend(b"with");
    expected.extend([0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x3f]);
    expected.extend([0x08, 0x00, 0x05]);
    expected.extend(b"color");
    expected.extend([0x00, 0x03]);
    expected.extend(b"red");
    expected.extend([0x00, 0x01]);
    assert_eq!(frame.body, expected);
}

#[test]
fn replacing_at_the_top_of_the_world_works_without_a_message() {
    let (mut server, client) = settled();
    let top = BlockPos::new(8, 319, 8);
    set_block(&mut server, top, blocks::SHORT_GRASS);
    move_player(&server, &client, 10.5, 317.0, 8.5);
    hold(&server, &client, items::STONE);

    click_top(&server, &client, top, 1);
    server.tick();

    assert_eq!(block_at(&mut server, top), Some(blocks::STONE));
    assert_eq!(
        client.take_frames(),
        [
            update(top, blocks::STONE),
            update(top, blocks::STONE),
            ack(1)
        ]
    );
}

#[test]
fn the_side_of_the_highest_block_works_as_usual() {
    let (mut server, client) = settled();
    let top = BlockPos::new(8, 319, 8);
    let east = top.relative(Direction::East);
    set_block(&mut server, top, blocks::STONE);
    move_player(&server, &client, 11.5, 317.0, 8.5);
    hold(&server, &client, items::DIRT);

    let request = use_item_on(Hand::Main, top, Direction::East, 1);
    server.request(&client, request);
    server.tick();

    assert_eq!(block_at(&mut server, east), Some(blocks::DIRT));
    assert!(frames_with_id(&client.take_frames(), clientbound::SYSTEM_CHAT).is_empty());
}

#[test]
fn the_bottom_face_of_the_lowest_block_refuses_with_a_message() {
    let (mut server, client) = settled();
    let bottom = BlockPos::new(8, -64, 8);
    move_player(&server, &client, 8.5, -67.0, 8.5);
    hold(&server, &client, items::DIRT);

    let request = use_item_on(Hand::Main, bottom, Direction::Down, 1);
    server.request(&client, request);
    server.tick();

    assert_eq!(
        client.take_frames(),
        [
            build_limit("build.tooLow", -64),
            update(bottom, blocks::BEDROCK),
            ack(1),
        ]
    );
}

#[test]
fn clicks_above_or_below_the_world_only_get_the_message() {
    let cases = [
        (
            BlockPos::new(8, 320, 8),
            318.0,
            build_limit("build.tooHigh", 319),
        ),
        (
            BlockPos::new(8, -65, 8),
            -67.0,
            build_limit("build.tooLow", -64),
        ),
    ];

    for (pos, player_y, message) in cases {
        let (mut server, client) = settled();
        move_player(&server, &client, 8.5, player_y, 8.5);
        hold(&server, &client, items::STONE);

        click_top(&server, &client, pos, 1);
        server.tick();

        assert_eq!(client.take_frames(), [message, ack(1)]);
    }
}

#[test]
fn top_of_the_world_message_needs_a_placement_attempt() {
    let top = BlockPos::new(8, 319, 8);
    let cases = [
        (items::WATER_BUCKET, true),
        // 官方 `BucketItem` 的内容是空的。
        (items::BUCKET, false),
        (items::SULFUR_CUBE_BUCKET, false),
        (items::DIAMOND_SWORD, false),
        // 放不下的方块物品也算尝试放置。
        (items::COMMAND_BLOCK, true),
        (items::OAK_DOOR, true),
    ];

    for (item, attempt) in cases {
        let (mut server, client) = settled();
        set_block(&mut server, top, blocks::STONE);
        move_player(&server, &client, 10.5, 317.0, 8.5);
        hold(&server, &client, item);

        click_top(&server, &client, top, 1);
        server.tick();

        let frames = client.take_frames();
        let messages = frames_with_id(&frames, clientbound::SYSTEM_CHAT).len();
        assert_eq!(messages, usize::from(attempt), "item {item}");
    }
}

#[test]
fn an_empty_hand_gets_no_top_of_the_world_message() {
    let (mut server, client) = settled();
    let top = BlockPos::new(8, 319, 8);
    set_block(&mut server, top, blocks::STONE);
    move_player(&server, &client, 10.5, 317.0, 8.5);

    click_top(&server, &client, top, 1);
    server.tick();

    assert_eq!(client.take_frames(), [update(top, blocks::STONE), ack(1)]);
}

#[test]
fn clicking_out_of_reach_only_acks() {
    let (mut server, client) = settled();
    hold(&server, &client, items::STONE);

    click_top(&server, &client, BlockPos::new(30, -61, 8), 1);
    server.tick();

    assert_eq!(client.take_frames(), [ack(1)]);
}

#[test]
fn a_cursor_outside_the_limit_only_acks() {
    let bad_cursors = [
        (1.5 + f32::EPSILON, 0.5, 0.5),
        (0.5, -0.5 - f32::EPSILON, 0.5),
        (0.5, 0.5, 1.6),
        (f32::NAN, 0.5, 0.5),
        (0.5, f32::NAN, 0.5),
        (0.5, 0.5, f32::NAN),
        (f32::INFINITY, 0.5, 0.5),
        (0.5, f32::NEG_INFINITY, 0.5),
    ];
    let (mut server, client) = settled();
    hold(&server, &client, items::STONE);

    for cursor in bad_cursors {
        server.request(
            &client,
            PlayRequest::UseItemOn {
                hand: Hand::Main,
                pos: GROUND,
                face: Direction::Up,
                cursor,
                inside: false,
                world_border_hit: false,
                sequence: 1,
            },
        );
        server.tick();

        assert_eq!(client.take_frames(), [ack(1)], "cursor {cursor:?}");
        assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
    }
}

#[test]
fn a_cursor_just_inside_the_limit_is_accepted() {
    let (mut server, client) = settled();
    hold(&server, &client, items::STONE);

    server.request(
        &client,
        PlayRequest::UseItemOn {
            hand: Hand::Main,
            pos: GROUND,
            face: Direction::Up,
            cursor: (1.5, -0.5, 0.0),
            inside: false,
            world_border_hit: false,
            sequence: 1,
        },
    );
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::STONE));
}

#[test]
fn clicks_before_the_teleport_is_confirmed_change_nothing() {
    let mut server = Harness::new(8);
    let client = server.join("Kitten");
    server.tick();
    client.take_frames();
    hold(&server, &client, items::STONE);

    click_top(&server, &client, GROUND, 1);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
    assert_eq!(
        client.take_frames(),
        [
            update(GROUND, blocks::GRASS_BLOCK),
            update(ABOVE, blocks::AIR),
            ack(1),
        ]
    );
}

#[test]
fn game_master_items_need_permissions_nobody_has() {
    let refused = [
        items::COMMAND_BLOCK,
        items::CHAIN_COMMAND_BLOCK,
        items::REPEATING_COMMAND_BLOCK,
        items::STRUCTURE_BLOCK,
        items::JIGSAW,
        items::TEST_BLOCK,
        items::TEST_INSTANCE_BLOCK,
    ];
    let (mut server, client) = settled();

    for item in refused {
        hold(&server, &client, item);
        click_top(&server, &client, GROUND, 1);
        server.tick();

        assert_eq!(
            block_at(&mut server, ABOVE),
            Some(blocks::AIR),
            "item {item}"
        );
        assert_eq!(
            client.take_frames(),
            [
                update(GROUND, blocks::GRASS_BLOCK),
                update(ABOVE, blocks::AIR),
                ack(1),
            ]
        );
    }
}

#[test]
fn lily_pads_ignore_the_block_that_was_clicked() {
    let (mut server, client) = settled();
    hold(&server, &client, items::LILY_PAD);

    click_top(&server, &client, GROUND, 1);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
}

#[test]
fn items_that_are_not_blocks_place_nothing() {
    let (mut server, client) = settled();

    for item in [items::DIAMOND_SWORD, items::WATER_BUCKET, items::STICK] {
        hold(&server, &client, item);
        click_top(&server, &client, GROUND, 1);
        server.tick();

        assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
        assert_eq!(client.take_frames().len(), 3);
    }
}

#[test]
fn an_empty_hand_places_nothing_and_still_gets_both_blocks() {
    let (mut server, client) = settled();

    click_top(&server, &client, GROUND, 1);
    server.tick();

    assert_eq!(
        client.take_frames(),
        [
            update(GROUND, blocks::GRASS_BLOCK),
            update(ABOVE, blocks::AIR),
            ack(1),
        ]
    );
}

#[test]
fn items_with_another_name_place_their_block() {
    let (mut server, client) = settled();
    hold(&server, &client, items::REDSTONE);

    click_top(&server, &client, GROUND, 1);
    server.tick();

    assert!(state_at(&mut server, ABOVE).is(blocks::REDSTONE_WIRE));
}

#[test]
fn right_clicking_a_chest_places_nothing() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::CHEST);
    hold(&server, &client, items::STONE);

    click_top(&server, &client, ABOVE, 1);
    server.tick();

    assert_eq!(block_at(&mut server, TWO_ABOVE), Some(blocks::AIR));
    // 右键打开箱子：菜单包在请求里就发，方块事件和 ack 等到这一 tick 后面的阶段。
    let ids: Vec<i32> = client.take_frames().iter().map(|frame| frame.id).collect();
    assert_eq!(
        ids,
        [
            clientbound::OPEN_SCREEN,
            clientbound::CONTAINER_SET_CONTENT,
            clientbound::BLOCK_UPDATE,
            clientbound::BLOCK_UPDATE,
            clientbound::BLOCK_EVENT,
            clientbound::BLOCK_CHANGED_ACK,
        ],
        "frames: {ids:?}"
    );
}

#[test]
fn sneaking_on_a_chest_places_the_block() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::CHEST);
    press_shift(&mut server, &client);

    assert!(places_on_top(&mut server, &client, ABOVE, items::STONE));
}

#[test]
fn shift_counts_for_the_chest_even_while_flying() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::CHEST);
    press_shift(&mut server, &client);
    player_mut(&mut server, &client).sync.flying = true;

    assert!(places_on_top(&mut server, &client, ABOVE, items::STONE));
}

#[test]
fn sneaking_with_something_in_the_other_hand_still_suppresses_the_chest() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::CHEST);
    press_shift(&mut server, &client);
    hold_in_off_hand(&server, &client, items::STONE);

    let request = use_item_on(Hand::Off, ABOVE, Direction::Up, 1);
    server.request(&client, request);
    server.tick();

    assert_eq!(block_at(&mut server, TWO_ABOVE), Some(blocks::STONE));
}

#[test]
fn stone_goes_on_a_fence() {
    assert!(stone_goes_on_top_of(blocks::OAK_FENCE));
}

#[test]
fn blocks_of_section_seven_that_eat_the_click() {
    let eaten = [
        blocks::CHEST,
        blocks::TRAPPED_CHEST,
        blocks::COPPER_CHEST,
        blocks::ENDER_CHEST,
        blocks::SHULKER_BOX,
        blocks::RED_SHULKER_BOX,
        blocks::ANVIL,
        blocks::DAMAGED_ANVIL,
        blocks::BARREL,
        blocks::FURNACE,
        blocks::CRAFTING_TABLE,
        blocks::HOPPER,
        blocks::OAK_DOOR,
        blocks::OAK_TRAPDOOR,
        blocks::OAK_FENCE_GATE,
        blocks::STONE_BUTTON,
        blocks::OAK_BUTTON,
        blocks::LEVER,
        blocks::REPEATER,
        blocks::NOTE_BLOCK,
        blocks::RED_BED,
        blocks::STRAW_BED,
        blocks::FLOWER_POT,
        blocks::POTTED_POPPY,
        blocks::OAK_SIGN,
        blocks::OAK_WALL_SIGN,
        blocks::OAK_HANGING_SIGN,
        blocks::OAK_WALL_HANGING_SIGN,
        blocks::LECTERN,
        blocks::CAKE,
        blocks::CANDLE_CAKE,
        blocks::RED_CANDLE_CAKE,
        blocks::DRAGON_EGG,
        blocks::LIGHT,
        blocks::DECORATED_POT,
        blocks::COPPER_GOLEM_STATUE,
        blocks::REDSTONE_WIRE,
    ];

    for state in eaten {
        assert!(!stone_goes_on_top_of(state), "state {state}");
    }
}

#[test]
fn blocks_that_only_react_to_other_items_let_the_block_through() {
    let passing = [
        blocks::IRON_DOOR,
        blocks::IRON_TRAPDOOR,
        blocks::COMMAND_BLOCK,
        blocks::STRUCTURE_BLOCK,
        blocks::JIGSAW,
        blocks::PUMPKIN,
        blocks::TNT,
        blocks::CAMPFIRE,
        blocks::BEE_NEST,
        blocks::CAULDRON,
        blocks::VAULT,
        blocks::CANDLE,
        blocks::REDSTONE_ORE,
        blocks::JUKEBOX,
        blocks::COMPOSTER,
        blocks::RESPAWN_ANCHOR,
        blocks::SWEET_BERRY_BUSH,
        blocks::CAVE_VINES,
    ];

    for state in passing {
        assert!(stone_goes_on_top_of(state), "state {state}");
    }
}

#[test]
fn the_off_hand_never_reaches_use_without_item() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::CHEST);
    hold_in_off_hand(&server, &client, items::STONE);

    let request = use_item_on(Hand::Off, ABOVE, Direction::Up, 1);
    server.request(&client, request);
    server.tick();

    assert_eq!(block_at(&mut server, TWO_ABOVE), Some(blocks::STONE));
}

#[test]
fn pots_and_statues_eat_the_click_from_either_hand() {
    for state in [blocks::DECORATED_POT, blocks::COPPER_GOLEM_STATUE] {
        let (mut server, client) = settled();
        set_block(&mut server, ABOVE, state);
        hold_in_off_hand(&server, &client, items::STONE);

        let request = use_item_on(Hand::Off, ABOVE, Direction::Up, 1);
        server.request(&client, request);
        server.tick();

        assert_eq!(block_at(&mut server, TWO_ABOVE), Some(blocks::AIR));
    }
}

/// 手持石头从 `face` 右键 `state`，返回石头有没有放下来。
fn stone_goes_next_to(state: u32, hand: Hand, face: Direction, cursor_y: f32) -> bool {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, state);
    let slot = if hand == Hand::Main { 36 } else { 45 };
    give(&server, &client, slot, items::STONE);

    server.request(
        &client,
        PlayRequest::UseItemOn {
            hand,
            pos: ABOVE,
            face,
            cursor: (0.5, cursor_y, 0.5),
            inside: false,
            world_border_hit: false,
            sequence: 1,
        },
    );
    server.tick();

    block_at(&mut server, ABOVE.relative(face)) == Some(blocks::STONE)
}

#[test]
fn shelves_only_eat_clicks_on_their_front_from_the_main_hand() {
    for state in [blocks::CHISELED_BOOKSHELF, blocks::OAK_SHELF] {
        assert!(!stone_goes_next_to(
            state,
            Hand::Main,
            Direction::North,
            0.5
        ));
        assert!(stone_goes_next_to(state, Hand::Off, Direction::North, 0.5));
        for face in [Direction::South, Direction::East, Direction::Up] {
            assert!(stone_goes_next_to(state, Hand::Main, face, 0.5));
        }
    }
}

#[test]
fn a_bell_rings_on_the_lower_part_of_its_two_faces() {
    let bell = blocks::BELL;
    assert!(!stone_goes_next_to(bell, Hand::Main, Direction::North, 0.5));
    assert!(!stone_goes_next_to(bell, Hand::Main, Direction::South, 0.8));
    assert!(stone_goes_next_to(bell, Hand::Main, Direction::South, 0.9));
    assert!(stone_goes_next_to(bell, Hand::Main, Direction::East, 0.5));
    assert!(stone_goes_next_to(bell, Hand::Main, Direction::Up, 0.5));
    assert!(stone_goes_next_to(bell, Hand::Off, Direction::North, 0.5));
}

#[test]
fn a_composter_takes_what_can_be_composted() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::COMPOSTER);
    assert!(!places_on_top(
        &mut server,
        &client,
        ABOVE,
        items::OAK_SAPLING
    ));
    assert!(places_on_top(&mut server, &client, ABOVE, items::DIRT));
}

#[test]
fn a_respawn_anchor_takes_glowstone() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::RESPAWN_ANCHOR);
    assert!(!places_on_top(
        &mut server,
        &client,
        ABOVE,
        items::GLOWSTONE
    ));
    // 上一步充了能，充能的锚会吃掉点击，所以换回没充能的。
    set_block(&mut server, ABOVE, blocks::RESPAWN_ANCHOR);
    assert!(places_on_top(&mut server, &client, ABOVE, items::DIRT));
}

#[test]
fn a_campfire_cooks_the_potato_instead_of_planting_it() {
    for campfire in [blocks::CAMPFIRE, blocks::SOUL_CAMPFIRE] {
        let (mut server, client) = settled();
        set_block(&mut server, ABOVE, campfire);
        assert!(!places_on_top(&mut server, &client, ABOVE, items::POTATO));
        assert!(places_on_top(&mut server, &client, ABOVE, items::DIRT));
    }
}

#[test]
fn a_cauldron_takes_powder_snow_from_the_bucket() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::CAULDRON);
    assert!(!places_on_top(
        &mut server,
        &client,
        ABOVE,
        items::POWDER_SNOW_BUCKET
    ));
    assert!(places_on_top(&mut server, &client, ABOVE, items::DIRT));
}

#[test]
fn a_block_that_only_consumes_the_click_does_not_swing() {
    let (mut server, placer, watcher) = two_players();

    super::swing(
        server.region(),
        placer.id,
        Hand::Main,
        InteractionResult::Consume,
    );
    server.tick();

    assert!(swings(&watcher).is_empty());
    assert!(swings(&placer).is_empty());
}

#[test]
fn a_swing_the_client_did_not_predict_also_reaches_the_player() {
    let (mut server, placer, watcher) = two_players();

    super::swing(
        server.region(),
        placer.id,
        Hand::Off,
        InteractionResult::SuccessServer,
    );
    server.tick();

    let expected = swing_of(&placer, Hand::Off);
    assert_eq!(swings(&watcher), std::slice::from_ref(&expected));
    assert_eq!(swings(&placer), std::slice::from_ref(&expected));
}

#[test]
fn a_placed_block_swings_for_the_others_only() {
    let (mut server, placer, watcher) = two_players();
    hold(&server, &placer, items::STONE);

    click_top(&server, &placer, GROUND, 1);
    server.tick();

    assert_eq!(swings(&watcher), [swing_of(&placer, Hand::Main)]);
    assert!(swings(&placer).is_empty());
}

#[test]
fn a_refused_placement_does_not_swing() {
    let (mut server, placer, watcher) = two_players();
    hold(&server, &placer, items::COMMAND_BLOCK);

    click_top(&server, &placer, GROUND, 1);
    server.tick();

    assert!(swings(&watcher).is_empty());
}

/// 这一 tick 的第一步是处理请求，所以延迟 1 的刻在同一 tick 的时间加一之后就到期了。
#[test]
fn scheduled_ticks_run_when_the_game_time_reaches_them() {
    let (mut server, _client) = settled();
    let block = BlockState(blocks::GRASS_BLOCK).block_id();
    let ticks = &mut server.region().block_ticks;
    ticks.schedule(GROUND, block, 1, TickPriority::Normal);
    ticks.schedule(ABOVE, block, 2, TickPriority::Normal);

    server.tick();

    let ticks = &server.region().block_ticks;
    let waiting: Vec<_> = ticks.pending().map(|tick| tick.pos).collect();
    assert_eq!(waiting, [ABOVE]);

    server.tick();

    assert_eq!(server.region().block_ticks.pending().count(), 0);
}

#[test]
fn use_item_only_acks() {
    let (mut server, client) = settled();
    hold(&server, &client, items::STONE);

    // `blocks-inventory.md` 3.1 的实测字节：副手，seq 3，yaw 90，pitch -15.5。
    let request = PlayRequest::decode(&[
        0x43, 0x01, 0x03, 0x42, 0xb4, 0x00, 0x00, 0xc1, 0x78, 0x00, 0x00,
    ])
    .unwrap();
    server.request(&client, request);
    server.tick();

    assert_eq!(client.take_frames(), [ack(3)]);
}

#[test]
fn use_item_turns_the_player_to_where_the_packet_looks() {
    let (mut server, client) = settled();
    hold(&server, &client, items::STONE);

    let request = PlayRequest::UseItem {
        hand: Hand::Main,
        sequence: 1,
        yaw: 450.0,
        pitch: 100.0,
    };
    server.request(&client, request);
    server.tick();

    let player = server.player(&client);
    assert_eq!((player.yaw, player.pitch), (90.0, 90.0));
}

#[test]
fn use_item_discards_angles_that_are_not_finite() {
    let (mut server, client) = settled();
    hold(&server, &client, items::STONE);

    let request = PlayRequest::UseItem {
        hand: Hand::Main,
        sequence: 1,
        yaw: f32::NAN,
        pitch: 45.0,
    };
    server.request(&client, request);
    server.tick();

    let player = server.player(&client);
    assert_eq!((player.yaw, player.pitch), (0.0, 45.0));
    assert!(!client.is_closed());
}

#[test]
fn use_item_with_an_empty_hand_does_not_turn_the_player() {
    let (mut server, client) = settled();

    let request = PlayRequest::UseItem {
        hand: Hand::Main,
        sequence: 1,
        yaw: 90.0,
        pitch: 45.0,
    };
    server.request(&client, request);
    server.tick();

    let player = server.player(&client);
    assert_eq!((player.yaw, player.pitch), (0.0, 0.0));
    assert_eq!(client.take_frames(), [ack(1)]);
}

#[test]
fn a_lily_pad_lands_on_the_water_in_view() {
    let (mut server, placer, watcher) = two_players();
    let water = BlockPos::new(11, -61, 8);
    set_block(&mut server, water, blocks::WATER);
    // 荷叶有碰撞形状，脚要在它上面。
    move_player(&server, &placer, 11.5, -59.0, 8.5);
    hold(&server, &placer, items::LILY_PAD);
    server.tick();
    placer.take_frames();
    watcher.take_frames();

    let request = PlayRequest::UseItem {
        hand: Hand::Main,
        sequence: 1,
        yaw: 0.0,
        pitch: 90.0,
    };
    server.request(&placer, request);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::LILY_PAD));
    assert_eq!(
        block_frames(&placer),
        [update(ABOVE, blocks::LILY_PAD), ack(1)]
    );
    let frames = watcher.take_frames();
    assert_eq!(
        frames_with_id(&frames, clientbound::SWING_ANIMATION),
        [&swing_of(&placer, Hand::Main)]
    );
}

/// 浮在 `GROUND` 正上方往下看，拿着 `item` 用一下。
fn use_looking_down(server: &mut Harness, client: &Client, item: i32) {
    move_player(server, client, 11.5, -59.0, 8.5);
    hold(server, client, item);
    server.tick();
    client.take_frames();
    let request = PlayRequest::UseItem {
        hand: Hand::Main,
        sequence: 1,
        yaw: 0.0,
        pitch: 90.0,
    };
    server.request(client, request);
    server.tick();
}

#[test]
fn a_water_bucket_pours_a_source_that_flows_on_later_ticks() {
    let (mut server, client) = settled();
    use_looking_down(&mut server, &client, items::WATER_BUCKET);

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::WATER));
    assert_eq!(
        block_frames(&client),
        [update(ABOVE, blocks::WATER), ack(1)]
    );
    // 创造模式倒完手里还是满桶。
    let held = server.player(&client).inventory.held(Hand::Main).item;
    assert_eq!(held, items::WATER_BUCKET);

    // 水的计划刻 5 刻以后到期，往四周流一格。
    for _ in 0..4 {
        server.tick();
    }
    let flowing = BlockState::WATER.with(LEVEL, 1).0;
    assert_eq!(block_at(&mut server, ABOVE.east()), Some(flowing));
    // 四格在同一段，同一 tick 流过去，一起发。
    let frames = client.take_frames();
    assert_eq!(
        frames_with_id(&frames, clientbound::SECTION_BLOCKS_UPDATE).len(),
        1
    );
    assert!(block_changes(&frames).contains(&(ABOVE.east(), flowing)));
}

#[test]
fn an_empty_bucket_scoops_a_source_into_the_inventory() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::LAVA);
    use_looking_down(&mut server, &client, items::BUCKET);

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
    let inventory = &server.player(&client).inventory;
    assert_eq!(inventory.held(Hand::Main).item, items::BUCKET);
    // 主手占着快捷栏第一格，装满的桶进了第二格。
    assert_eq!(inventory.slots[37].item, items::LAVA_BUCKET);
    let frames = client.take_frames();
    assert_eq!(
        frames_with_id(&frames, clientbound::CONTAINER_SET_SLOT).len(),
        1
    );
}

#[test]
fn one_ack_covers_the_largest_sequence_of_the_tick() {
    let (mut server, client) = settled();

    for sequence in [5, 300, 7] {
        let request = PlayRequest::UseItem {
            hand: Hand::Main,
            sequence,
            yaw: 0.0,
            pitch: 0.0,
        };
        server.request(&client, request);
    }
    server.tick();

    let frames = client.take_frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].id, clientbound::BLOCK_CHANGED_ACK);
    // `blocks-inventory.md` 3.2 的实测字节。
    assert_eq!(frames[0].body, [0xac, 0x02]);
}

#[test]
fn the_ack_is_sent_once_and_resets() {
    let (mut server, client) = settled();

    destroy(&server, &client, GROUND, 9);
    server.tick();
    client.take_frames();
    server.tick();

    assert!(client.take_frames().is_empty());
    assert_eq!(server.player(&client).prediction.pending_ack, -1);
}

#[test]
fn sequence_zero_and_the_largest_one_are_acked() {
    let (mut server, client) = settled();

    for sequence in [0, i32::MAX] {
        destroy(&server, &client, ABOVE, sequence);
        server.tick();
        assert_eq!(client.take_frames(), [ack(sequence)]);
    }
}

#[test]
fn every_refused_prediction_is_acked_after_the_blocks_it_touched() {
    let (mut server, client) = settled();
    hold(&server, &client, items::DIAMOND_SWORD);

    // 破坏被拒绝，放置也被拒绝，够不到的再来一次，一个 ack 就要盖住这三个。
    destroy(&server, &client, GROUND, 1);
    click_top(&server, &client, GROUND, 2);
    click_top(&server, &client, BlockPos::new(30, -61, 8), 3);
    server.tick();

    let frames = client.take_frames();
    assert_eq!(frames.last(), Some(&ack(3)));
    let acks = frames_with_id(&frames, clientbound::BLOCK_CHANGED_ACK);
    assert_eq!(acks.len(), 1);
    assert!(
        frames[..frames.len() - 1]
            .iter()
            .all(|frame| frame.id == clientbound::BLOCK_UPDATE)
    );
}

#[test]
fn players_far_away_do_not_get_the_change() {
    let (mut server, near, far) = two_players();
    move_player(&server, &far, 2000.0, SPAWN_Y, 2000.0);
    server.tick();
    far.take_frames();

    destroy(&server, &near, GROUND, 1);
    server.tick();

    assert!(block_frames(&far).is_empty());
}

#[test]
fn a_pending_chunk_arrives_with_the_change_instead_of_an_update() {
    let mut server = Harness::new(8);
    let breaker = server.join_and_settle("Alice");
    move_player(&server, &breaker, 40.5, SPAWN_Y, 40.5);
    server.receive_all_chunks(&breaker);
    breaker.take_frames();

    // 第一批只有出生点周围的 3×3，确认之前 (2, 2) 还在 `pending` 里。
    let late = server.join_with("Bob", client_requesting(8));
    server.confirm_teleport(&late);
    server.tick();
    late.take_frames();

    let pos = BlockPos::new(40, -61, 40);
    let edited = ChunkPos::new(2, 2);
    destroy(&server, &breaker, pos, 1);
    server.tick();

    assert!(frames_with_id(&late.take_frames(), clientbound::BLOCK_UPDATE).is_empty());
    assert_eq!(
        frames_with_id(&breaker.take_frames(), clientbound::BLOCK_UPDATE).len(),
        1
    );

    server.acknowledge_batch(&late, 64.0);
    server.tick();

    let frames = late.take_frames();
    let chunks = frames_with_id(&frames, clientbound::LEVEL_CHUNK_WITH_LIGHT);
    let chunk = chunks
        .iter()
        .find(|frame| chunk_position(frame) == edited)
        .expect("这一批最多 64 个，近的先发");
    let region = server.region();
    assert!(region.chunks.edited(edited).is_some(), "这个区块改过了");
    let current = encode_chunk(&region.chunks, edited);
    assert_eq!(chunk_body(chunk), current);
    assert_ne!(chunk_body(chunk), region.template_body);
}

/// 走到视距外再走回来，返回重新发来的 `pos` 区块。
fn walk_away_and_back(server: &mut Harness, client: &Client, pos: ChunkPos) -> Vec<u8> {
    server.move_to(client, 1000.0, 1000.0);
    server.tick();
    client.take_frames();
    server.acknowledge_batch(client, 64.0);
    server.tick();
    client.take_frames();

    server.move_to(client, SPAWN_X, SPAWN_Z);
    server.acknowledge_batch(client, 64.0);
    server.tick();

    let frames = client.take_frames();
    let chunks = frames_with_id(&frames, clientbound::LEVEL_CHUNK_WITH_LIGHT);
    let resent = chunks
        .iter()
        .find(|frame| chunk_position(frame) == pos)
        .expect("这个区块又进入了视图");
    chunk_body(resent).to_vec()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|part| part == needle)
}

#[test]
fn a_chunk_sent_again_has_the_edit_and_the_block_entity() {
    /// `blocks-inventory.md` 第 11 章的实测字节：区块内 (3, -60, 5) 的箱子。
    const CHEST_ENTRY: [u8; 5] = [0x35, 0xff, 0xc4, 0x01, 0x00];

    let (mut server, client) = settled();
    let chunk = ChunkPos::new(0, 0);
    let ground = BlockPos::new(3, -61, 5);
    let chest = BlockPos::new(3, -60, 5);
    let hole = BlockPos::new(4, -61, 5);
    hold(&server, &client, items::CHEST);

    click_top(&server, &client, ground, 1);
    destroy(&server, &client, hole, 2);
    server.tick();
    assert_eq!(block_at(&mut server, chest), Some(blocks::CHEST));
    client.take_frames();

    let resent = walk_away_and_back(&mut server, &client, chunk);

    let region = server.region();
    assert!(region.chunks.edited(chunk).is_some(), "这个区块改过了");
    let current = encode_chunk(&region.chunks, chunk);
    assert_eq!(resent, current);
    assert_ne!(resent, region.template_body);
    assert!(contains(&resent, &CHEST_ENTRY));
}

#[test]
fn breaking_a_chest_removes_it_from_the_chunk() {
    let (mut server, client) = settled();
    let chest = BlockPos::new(3, -60, 5);
    hold(&server, &client, items::CHEST);

    click_top(&server, &client, BlockPos::new(3, -61, 5), 1);
    server.tick();
    destroy(&server, &client, chest, 2);
    server.tick();

    let chunks = &server.region().chunks;
    let chunk = chunks.edited(chest.chunk()).expect("这个区块改过了");
    assert_eq!(chunk.block_entities().count(), 0);
}

#[test]
fn absurd_positions_never_reach_the_world() {
    let (mut server, client) = settled();
    hold(&server, &client, items::STONE);
    let positions = [
        BlockPos::new(8, 2047, 8),
        BlockPos::new(8, -2048, 8),
        BlockPos::new(33_554_431, 2047, -33_554_432),
    ];

    for (sequence, pos) in (1..).zip(positions) {
        destroy(&server, &client, pos, sequence);
        click_top(&server, &client, pos, sequence);
        server.request(
            &client,
            PlayRequest::PickItemFromBlock {
                pos,
                include_data: true,
            },
        );
        server.tick();

        assert_eq!(client.take_frames(), [ack(sequence)]);
    }
}

#[test]
fn a_log_lies_along_the_axis_of_the_clicked_face() {
    let cases = [
        (Direction::East, Axis::X),
        (Direction::North, Axis::Z),
        (Direction::Up, Axis::Y),
    ];

    for (face, axis) in cases {
        let (mut server, client) = settled();
        set_block(&mut server, ABOVE, blocks::STONE);
        hold(&server, &client, items::OAK_LOG);

        let request = use_item_on(Hand::Main, ABOVE, face, 1);
        server.request(&client, request);
        server.tick();

        let placed = BlockState(blocks::OAK_LOG).with(AXIS, axis);
        assert_eq!(state_at(&mut server, ABOVE.relative(face)), placed);
    }
}

#[test]
fn a_chain_keeps_the_axis_too() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::STONE);
    hold(&server, &client, items::IRON_CHAIN);

    let request = use_item_on(Hand::Main, ABOVE, Direction::East, 1);
    server.request(&client, request);
    server.tick();

    let placed = BlockState(blocks::IRON_CHAIN).with(AXIS, Axis::X);
    assert_eq!(state_at(&mut server, ABOVE.east()), placed);
}

#[test]
fn a_slab_goes_to_the_half_that_was_clicked() {
    let cases = [(0.25, SlabType::Bottom), (0.75, SlabType::Top)];

    for (height, half) in cases {
        let (mut server, client) = settled();
        set_block(&mut server, ABOVE, blocks::STONE);
        hold(&server, &client, items::OAK_SLAB);

        let request = use_item_at(ABOVE, Direction::East, (1.0, height, 0.5));
        server.request(&client, request);
        server.tick();

        let placed = BlockState(blocks::OAK_SLAB).with(SLAB_TYPE, half);
        assert_eq!(state_at(&mut server, ABOVE.east()), placed);
    }
}

#[test]
fn a_second_slab_fills_the_first_one() {
    let (mut server, client) = settled();
    hold(&server, &client, items::OAK_SLAB);

    click_top(&server, &client, GROUND, 1);
    server.tick();
    click_top(&server, &client, ABOVE, 2);
    server.tick();

    let double = BlockState(blocks::OAK_SLAB).with(SLAB_TYPE, SlabType::Double);
    assert_eq!(state_at(&mut server, ABOVE), double);
    assert_eq!(block_at(&mut server, TWO_ABOVE), Some(blocks::AIR));
}

#[test]
fn a_slab_goes_under_a_player_floating_above_its_real_shape() {
    let (mut server, client) = settled();
    let ground = BlockPos::new(8, -61, 8);
    let target = BlockPos::new(8, -60, 8);
    // 脚在这一格的上半，下半格的台阶碰不到，整格的方块碰得到。
    move_player(&server, &client, 8.5, -59.4, 8.5);

    hold(&server, &client, items::STONE);
    click_top(&server, &client, ground, 1);
    server.tick();
    assert_eq!(block_at(&mut server, target), Some(blocks::AIR));

    hold(&server, &client, items::OAK_SLAB);
    click_top(&server, &client, ground, 2);
    server.tick();
    assert_eq!(block_at(&mut server, target), Some(blocks::OAK_SLAB));
}

#[test]
fn a_torch_needs_something_to_hold_on_to() {
    let (mut server, client) = settled();
    hold(&server, &client, items::TORCH);

    click_top(&server, &client, TWO_ABOVE, 1);
    server.tick();

    assert_eq!(block_at(&mut server, TWO_ABOVE), Some(blocks::AIR));
    assert_eq!(block_at(&mut server, TWO_ABOVE.above()), Some(blocks::AIR));
}

#[test]
fn a_torch_stands_on_the_floor_and_hangs_on_a_wall() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::STONE);
    hold(&server, &client, items::TORCH);

    click_top(&server, &client, ABOVE, 1);
    let request = use_item_on(Hand::Main, ABOVE, Direction::East, 2);
    server.request(&client, request);
    server.tick();

    assert_eq!(block_at(&mut server, TWO_ABOVE), Some(blocks::TORCH));
    let hanging = BlockState(blocks::WALL_TORCH).with(HORIZONTAL_FACING, Direction::East);
    assert_eq!(state_at(&mut server, ABOVE.east()), hanging);
}

/// 视线朝 `yaw` 时往两面墙的夹角里放火把，返回它贴在哪面墙上。
fn torch_in_a_corner(yaw: f32) -> Direction {
    let (mut server, client) = settled();
    let corner = BlockPos::new(11, -59, 8);
    set_block(&mut server, corner.east(), blocks::STONE);
    set_block(&mut server, corner.south(), blocks::STONE);
    hold(&server, &client, items::TORCH);
    look(&mut server, &client, yaw, 0.0);

    click_top(&server, &client, corner, 1);
    server.tick();

    state_at(&mut server, corner)
        .get(HORIZONTAL_FACING)
        .opposite()
}

#[test]
fn the_view_decides_which_wall_a_torch_hangs_on() {
    assert_eq!(torch_in_a_corner(0.0), Direction::South);
    assert_eq!(torch_in_a_corner(-90.0), Direction::East);
}

#[test]
fn a_torch_goes_with_its_support_and_everyone_sees_it() {
    let (mut server, breaker, watcher) = two_players();
    hold(&server, &breaker, items::TORCH);
    click_top(&server, &breaker, GROUND, 1);
    server.tick();
    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::TORCH));
    breaker.take_frames();
    watcher.take_frames();
    hold(&server, &breaker, items::STONE);

    destroy(&server, &breaker, GROUND, 2);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
    // 两格在同一段，合成一个包，顺序是官方 `ShortOpenHashSet` 的遍历顺序。
    let gone = section_update(&[(GROUND, blocks::AIR), (ABOVE, blocks::AIR)]);
    // 官方的连锁里破坏的是世界，不是发起者，所以火把的事件发起者也要收到。
    let torch = destroyed(ABOVE, blocks::TORCH);
    let grass = destroyed(GROUND, blocks::GRASS_BLOCK);
    assert_eq!(
        block_frames(&breaker),
        [torch.clone(), gone.clone(), ack(2)]
    );
    assert_eq!(block_frames(&watcher), [grass, torch, gone]);
}

#[test]
fn replacing_short_grass_is_not_a_break() {
    let (mut server, placer, watcher) = two_players();
    hold(&server, &placer, items::SHORT_GRASS);
    click_top(&server, &placer, GROUND, 1);
    server.tick();
    placer.take_frames();
    watcher.take_frames();
    hold(&server, &placer, items::STONE);

    click_top(&server, &placer, ABOVE, 2);
    server.tick();

    let frames = watcher.take_frames();
    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::STONE));
    assert!(frames_with_id(&frames, clientbound::LEVEL_EVENT).is_empty());
}

#[test]
fn every_item_can_be_used_on_a_block_and_in_the_air() {
    let (mut server, client) = settled();
    let stack = |item: usize| PlayRequest::SetCreativeModeSlot {
        slot: 36,
        stack: ItemStack::new(item as i32, 1),
    };

    for item in 0..ITEMS.len() {
        server.request(&client, stack(item));
        click_top(&server, &client, GROUND, 1);
        destroy(&server, &client, ABOVE, 2);
        server.request(
            &client,
            PlayRequest::UseItem {
                hand: Hand::Main,
                sequence: 3,
                yaw: 0.0,
                pitch: 90.0,
            },
        );
        server.tick();
        client.take_frames();
        set_block(&mut server, ABOVE, blocks::AIR);
        set_block(&mut server, TWO_ABOVE, blocks::AIR);
    }

    assert!(!client.is_closed());
}

#[test]
fn a_player_arriving_later_gets_chests_and_banners_in_the_chunk() {
    let (mut server, placer) = settled();
    hold(&server, &placer, items::CHEST);
    click_top(&server, &placer, BlockPos::new(3, -61, 5), 1);
    hold(&server, &placer, items::WHITE_BANNER);
    click_top(&server, &placer, BlockPos::new(4, -61, 5), 2);
    server.tick();

    let late = server.join("Bob");
    server.tick();

    let frames = late.take_frames();
    let chunks = frames_with_id(&frames, clientbound::LEVEL_CHUNK_WITH_LIGHT);
    let chunk = chunks
        .iter()
        .find(|frame| chunk_position(frame) == ChunkPos::new(0, 0))
        .expect("第一批里有出生点的区块");
    let banner = block_entity_type::BANNER as u8;
    // 箱子的 `getUpdateTag` 是空的，官方写一个 `TAG_End`。
    assert!(contains(chunk_body(chunk), &[0x35, 0xff, 0xc4, 0x01, 0x00]));
    // 旗帜的 `getUpdateTag` 走 `saveWithoutMetadata`，至少带一张空的组件表，
    // 所以后面跟的是一个 `TAG_Compound`。
    assert!(contains(
        chunk_body(chunk),
        &[0x45, 0xff, 0xc4, banner, 0x0a, 0x0a, 0x00, 0x0a]
    ));
}

/// 确定性的伪随机数，失败时可以重现。
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }

    fn between(&mut self, low: i32, high: i32) -> i32 {
        low + self.below((high - low + 1) as usize) as i32
    }

    fn pos(&mut self) -> BlockPos {
        // 大多落在够得着的范围里，少数在世界的上下边界外面。
        let y = match self.below(8) {
            0 => self.between(-2048, 2047),
            1 => self.between(315, 325),
            _ => self.between(-68, -52),
        };
        BlockPos::new(self.between(2, 15), y, self.between(2, 15))
    }

    fn cursor(&mut self) -> f32 {
        const VALUES: [f32; 7] = [0.0, 0.5, 1.0, 1.5, -0.5, f32::NAN, f32::INFINITY];
        VALUES[self.below(VALUES.len())]
    }

    fn request(&mut self) -> PlayRequest {
        let hand = Hand::from_id(self.between(0, 1));
        let face = Direction::from_id(self.between(0, 5));
        let sequence = self.between(0, i32::MAX - 1);
        match self.below(8) {
            0 => action(PlayerActionKind::StartDestroyBlock, self.pos(), sequence),
            1 => action(PlayerActionKind::SwapItemWithOffhand, self.pos(), sequence),
            2 => action(PlayerActionKind::DropItem, self.pos(), sequence),
            3 => PlayRequest::SetCarriedItem {
                slot: self.between(-2, 10) as i16,
            },
            4 => PlayRequest::SetCreativeModeSlot {
                slot: self.between(-2, 47) as i16,
                stack: ItemStack::new(self.below(ITEMS.len()) as i32, self.between(0, 99)),
            },
            5 => PlayRequest::PickItemFromBlock {
                pos: self.pos(),
                include_data: false,
            },
            _ => PlayRequest::UseItemOn {
                hand,
                pos: self.pos(),
                face,
                cursor: (self.cursor(), self.cursor(), self.cursor()),
                inside: false,
                world_border_hit: false,
                sequence,
            },
        }
    }
}

#[test]
fn arbitrary_requests_never_panic_and_never_end_in_a_ghost() {
    let (mut server, client) = settled();
    let mut random = Random(0x2545_f491_4f6c_dd1d);
    let mut updates = 0;

    for _ in 0..500 {
        for _ in 0..64 {
            let request = random.request();
            server.request(&client, request);
        }
        let y = [-66.0, -60.0, -60.0, 316.0][random.below(4)];
        move_player(&server, &client, 8.5, y, 8.5);
        let sync = &mut player_mut(&mut server, &client).sync;
        sync.shift_key_down = random.below(2) == 0;
        sync.flying = random.below(2) == 0;
        server.tick();

        // 每一 tick 的 ack 都排在所有方块更新后面。
        let frames = client.take_frames();
        let last_update = frames.iter().rposition(|frame| {
            matches!(
                frame.id,
                clientbound::BLOCK_UPDATE | clientbound::SECTION_BLOCKS_UPDATE
            )
        });
        let ack = frames
            .iter()
            .position(|frame| frame.id == clientbound::BLOCK_CHANGED_ACK);
        if let (Some(last_update), Some(ack)) = (last_update, ack) {
            assert!(last_update < ack);
        }
        updates += block_changes(&frames).len();
    }

    assert!(!client.is_closed());
    // 真的走到了改世界的路径，不是全被距离挡掉了。
    assert!(updates > 1000, "only {updates} block updates");
    let origin = ChunkPos::new(0, 0);
    assert!(server.region().chunks.edited(origin).is_some());
    let inventory = &server.player(&client).inventory;
    assert!(inventory.selected < 9);
    assert_eq!(inventory.slots.len(), 46);
}

/// 直接放进主手，不走 `SetCreativeModeSlot`：冒险模式收不了那个包。
fn hold_stack(server: &mut Harness, client: &Client, stack: ItemStack) {
    player_mut(server, client).inventory.slots[36] = stack;
}

fn set_adventure(server: &mut Harness, client: &Client) {
    player_mut(server, client).game_mode = GameMode::Adventure;
}

/// 只包含 `state` 这一种方块的谓词。
fn block_predicate(state: u32) -> BlockPredicate {
    BlockPredicate {
        blocks: Some(HolderSet::Ids(vec![
            block_id_of_state(state).unwrap() as i32
        ])),
        properties: None,
        nbt: None,
        components: Default::default(),
    }
}

/// 一个物品，挂上 `kind` 组件里的一条谓词。
fn with_component(item: i32, kind: i32, state: u32) -> ItemStack {
    let mut value = Vec::new();
    let predicates = vec![block_predicate(state)];
    match kind {
        kinds::CAN_BREAK => ComponentValue::CanBreak(predicates).write(&mut value),
        kinds::CAN_PLACE_ON => ComponentValue::CanPlaceOn(predicates).write(&mut value),
        _ => unreachable!("测试只用这两个组件"),
    }
    ItemStack {
        item,
        count: 1,
        added: vec![(kind, value)],
        removed: Vec::new(),
    }
}

#[test]
fn adventure_mode_places_stone_where_can_place_on_matches() {
    let (mut server, client) = settled();
    set_adventure(&mut server, &client);
    // 点的是脚下的草方块，`can_place_on` 写的也是草方块。
    hold_stack(
        &mut server,
        &client,
        with_component(items::STONE, kinds::CAN_PLACE_ON, blocks::GRASS_BLOCK),
    );

    click_top(&server, &client, GROUND, 1);
    server.tick();

    // 草方块换成别的方块后谓词就不再匹配，这里看的是点击那一刻的草方块。
    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::STONE));
}

#[test]
fn adventure_mode_refuses_placement_where_can_place_on_does_not_match() {
    let (mut server, client) = settled();
    set_adventure(&mut server, &client);
    hold_stack(
        &mut server,
        &client,
        with_component(items::STONE, kinds::CAN_PLACE_ON, blocks::DIRT),
    );

    click_top(&server, &client, GROUND, 1);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
    // 被拒绝的预测照样要 ack。
    assert_eq!(client.take_frames().last(), Some(&ack(1)));
}

#[test]
fn creative_mode_ignores_can_place_on() {
    let (mut server, client) = settled();
    // 创造模式能建造，谓词匹配不上也照放。
    hold_stack(
        &mut server,
        &client,
        with_component(items::STONE, kinds::CAN_PLACE_ON, blocks::DIRT),
    );

    click_top(&server, &client, GROUND, 1);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::STONE));
}

#[test]
fn adventure_mode_breaks_a_block_that_can_break_lists() {
    let (mut server, client) = settled();
    set_adventure(&mut server, &client);
    // 矮草没有硬度，进度一到就挖穿。
    set_block(&mut server, ABOVE, blocks::SHORT_GRASS);
    hold_stack(
        &mut server,
        &client,
        with_component(items::STICK, kinds::CAN_BREAK, blocks::SHORT_GRASS),
    );

    destroy(&server, &client, ABOVE, 1);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
    assert_eq!(client.take_frames(), [update(ABOVE, blocks::AIR), ack(1)]);
}

#[test]
fn adventure_mode_cannot_break_a_block_outside_can_break() {
    let (mut server, client) = settled();
    set_adventure(&mut server, &client);
    set_block(&mut server, ABOVE, blocks::SHORT_GRASS);
    // 组件里写的是泥土，矮草不在列表里。
    hold_stack(
        &mut server,
        &client,
        with_component(items::STICK, kinds::CAN_BREAK, blocks::DIRT),
    );

    destroy(&server, &client, ABOVE, 1);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::SHORT_GRASS));
    assert_eq!(
        client.take_frames(),
        [update(ABOVE, blocks::SHORT_GRASS), ack(1)]
    );
}

fn set_spectator(server: &mut Harness, client: &Client) {
    player_mut(server, client).game_mode = GameMode::Spectator;
}

#[test]
fn spectator_can_place_on_does_not_place() {
    let (mut server, client) = settled();
    set_spectator(&mut server, &client);
    hold_stack(
        &mut server,
        &client,
        with_component(items::STONE, kinds::CAN_PLACE_ON, blocks::GRASS_BLOCK),
    );

    click_top(&server, &client, GROUND, 1);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
    assert_eq!(client.take_frames().last(), Some(&ack(1)));
}

#[test]
fn spectator_opens_a_chest_instead_of_placing_while_sneaking() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::CHEST);
    set_spectator(&mut server, &client);
    press_shift(&mut server, &client);
    // 潜行状态自己会同步一个包，先清掉，后面只看这次右键。
    server.tick();
    client.take_frames();
    // 潜行加 `can_place_on` 在别的模式会跳过箱子、把方块放上去。旁观者先看菜单。
    hold_stack(
        &mut server,
        &client,
        with_component(items::STONE, kinds::CAN_PLACE_ON, blocks::CHEST),
    );

    click_top(&server, &client, ABOVE, 1);
    server.tick();

    assert_eq!(block_at(&mut server, TWO_ABOVE), Some(blocks::AIR));
    let ids: Vec<i32> = client.take_frames().iter().map(|frame| frame.id).collect();
    assert_eq!(
        ids,
        [
            clientbound::OPEN_SCREEN,
            clientbound::CONTAINER_SET_CONTENT,
            clientbound::BLOCK_UPDATE,
            clientbound::BLOCK_UPDATE,
            clientbound::BLOCK_CHANGED_ACK,
        ],
        "frames: {ids:?}"
    );
    // `ChestBlockEntity.startOpen` 跳过旁观者：不计数，所以没有开盖的方块事件。
    assert_eq!(
        server
            .region()
            .containers
            .get(&ABOVE)
            .map_or(0, |entry| entry.open_count),
        0
    );
}

#[test]
fn spectator_opens_workstation_menus() {
    let (mut server, client) = settled();
    set_spectator(&mut server, &client);
    let stations = [
        blocks::CRAFTING_TABLE,
        blocks::FURNACE,
        blocks::BLAST_FURNACE,
        blocks::SMOKER,
        blocks::BREWING_STAND,
        blocks::STONECUTTER,
        blocks::SMITHING_TABLE,
    ];
    for state in stations {
        set_block(&mut server, ABOVE, state);
        client.take_frames();
        click_top(&server, &client, ABOVE, 1);
        server.tick();
        let ids: Vec<i32> = client.take_frames().iter().map(|frame| frame.id).collect();
        assert!(
            ids.contains(&clientbound::OPEN_SCREEN),
            "state {state} frames: {ids:?}"
        );
    }

    set_block(&mut server, ABOVE, blocks::LECTERN);
    client.take_frames();
    click_top(&server, &client, ABOVE, 1);
    server.tick();
    let ids: Vec<i32> = client.take_frames().iter().map(|frame| frame.id).collect();
    assert!(
        !ids.contains(&clientbound::OPEN_SCREEN),
        "an empty lectern frames: {ids:?}"
    );

    set_block(
        &mut server,
        ABOVE,
        BlockState(blocks::LECTERN).with(HAS_BOOK, true).0,
    );
    client.take_frames();
    click_top(&server, &client, ABOVE, 1);
    server.tick();
    let ids: Vec<i32> = client.take_frames().iter().map(|frame| frame.id).collect();
    assert!(
        ids.contains(&clientbound::OPEN_SCREEN),
        "a lectern with a book frames: {ids:?}"
    );

    set_block(&mut server, ABOVE, blocks::OAK_SHELF);
    client.take_frames();
    click_top(&server, &client, ABOVE, 1);
    server.tick();
    let ids: Vec<i32> = client.take_frames().iter().map(|frame| frame.id).collect();
    assert!(
        !ids.contains(&clientbound::OPEN_SCREEN),
        "a shelf frames: {ids:?}"
    );
}

#[test]
fn spectator_passes_a_blocked_chest_without_placing() {
    let (mut server, client) = settled();
    set_block(&mut server, ABOVE, blocks::CHEST);
    // 上面是红石导体，官方 `ChestBlock.getMenuProvider` 返回 null。
    set_block(&mut server, TWO_ABOVE, blocks::STONE);
    set_spectator(&mut server, &client);
    hold_stack(
        &mut server,
        &client,
        with_component(items::STONE, kinds::CAN_PLACE_ON, blocks::CHEST),
    );

    let beside = ABOVE.relative(Direction::North);
    server.request(
        &client,
        use_item_at(ABOVE, Direction::North, (0.5, 0.5, 0.0)),
    );
    server.tick();

    assert_eq!(block_at(&mut server, beside), Some(blocks::AIR));
    let ids: Vec<i32> = client.take_frames().iter().map(|frame| frame.id).collect();
    assert!(!ids.contains(&clientbound::OPEN_SCREEN), "frames: {ids:?}");
    assert_eq!(ids.last(), Some(&clientbound::BLOCK_CHANGED_ACK));
}

#[test]
fn spectator_portals_say_they_cannot_teleport() {
    let (mut server, client) = settled();
    set_spectator(&mut server, &client);
    for portal in [
        blocks::NETHER_PORTAL,
        blocks::END_PORTAL,
        blocks::END_GATEWAY,
    ] {
        set_block(&mut server, GROUND, portal);
        hold_stack(
            &mut server,
            &client,
            with_component(items::STONE, kinds::CAN_PLACE_ON, portal),
        );

        click_top(&server, &client, GROUND, 1);
        server.tick();

        assert_eq!(block_at(&mut server, GROUND), Some(portal));
        assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
        let frames = client.take_frames();
        let chat = frames
            .iter()
            .find(|frame| frame.id == clientbound::SYSTEM_CHAT)
            .expect("传送失败要有快捷栏上方的提示");
        assert!(
            chat.body
                .windows(b"spectator.cannot_teleport".len())
                .any(|window| window == b"spectator.cannot_teleport"),
            "portal {portal}"
        );
        assert_eq!(frames.last(), Some(&ack(1)));
    }
}

#[test]
fn spectator_use_item_does_not_place_a_lily_pad() {
    let (mut server, client) = settled();
    set_block(&mut server, GROUND, blocks::WATER);
    move_player(&server, &client, 11.5, -59.0, 8.5);
    set_spectator(&mut server, &client);
    hold_stack(&mut server, &client, ItemStack::new(items::LILY_PAD, 1));
    server.tick();
    client.take_frames();

    server.request(
        &client,
        PlayRequest::UseItem {
            hand: Hand::Main,
            sequence: 1,
            yaw: 0.0,
            pitch: 90.0,
        },
    );
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
    let ids: Vec<i32> = client.take_frames().iter().map(|frame| frame.id).collect();
    assert!(
        !ids.contains(&clientbound::SWING_ANIMATION),
        "frames: {ids:?}"
    );
    assert!(
        ids.contains(&clientbound::BLOCK_CHANGED_ACK),
        "frames: {ids:?}"
    );
}

#[test]
fn adventure_mode_can_break_does_not_allow_placing() {
    let (mut server, client) = settled();
    set_adventure(&mut server, &client);
    // `can_break` 只管挖，放置要看 `can_place_on`。
    hold_stack(
        &mut server,
        &client,
        with_component(items::STONE, kinds::CAN_BREAK, blocks::GRASS_BLOCK),
    );

    click_top(&server, &client, GROUND, 1);
    server.tick();

    assert_eq!(block_at(&mut server, ABOVE), Some(blocks::AIR));
}
