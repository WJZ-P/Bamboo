//! 两名真实 TCP 客户端一起破坏和放置方块。
//!
//! 期望值全部来自 `blocks-inventory.md` 第 3 到 8、11、13 章、`placement.md` 和
//! `coverage.md` 4.3，不看实现。

mod common;

use std::{collections::BTreeSet, time::Duration};

use common::{
    TestServer, frame,
    player::{Answers, Player},
    wire::{
        AddEntity, BlockChangedAck, BlockUpdate, ContainerSetSlot, DOWN, DROP_ALL_ITEMS, DROP_ITEM,
        Disconnect, EAST, ForgetLevelChunk, INPUT_SHIFT, LEVEL_EVENT_BREAK, LevelEvent, MAIN_HAND,
        MAX_Y, MIN_Y, NORTH, Nbt, OFF_HAND, Pos, RELEASE_USE_ITEM, SLOT_MAINHAND, SLOT_OFFHAND,
        SOUTH, START_DESTROY_BLOCK, SWAP_ITEM_WITH_OFFHAND, SWING_DURATION, SWING_WHACK,
        SectionBlocksUpdate, SetEquipment, SetHeldSlot, Stack, SwingAnimation, SystemChat, UP,
        face_center, pick_item_from_block, plain_stack_bytes, player_action, player_input,
        set_carried_item, set_creative_mode_slot, set_creative_mode_slot_raw, use_item,
        use_item_on,
    },
};
use mc_bamboo_data::{
    block_of_state, blocks, items,
    packets::play::{clientbound, serverbound},
    registries::{block_entity_type, data_component_type},
};
use mc_bamboo_protocol::components::{ComponentValue, PatchEntry};
use mc_bamboo_server::ServerConfig;
use tokio::time::sleep;

/// 出生点旁边的草方块，站在出生点够得着。
const SPOT: Pos = Pos::new(10, -61, 8);
const ABOVE_SPOT: Pos = SPOT.above();
/// 出生点脚下的草方块。
const UNDER_SPAWN: Pos = Pos::new(8, -61, 8);
/// 平坦世界 4 层方块：基岩、两层泥土、草方块。
const FLAT_BLOCKS_PER_SECTION: i16 = 4 * 256;
const MAX_HOTBAR_INDEX: i16 = 8;
/// `axis` 的取值依次是 x、y、z，默认是 y。
const OAK_LOG_ALONG_Z: u32 = blocks::OAK_LOG + 1;
/// `type` 的取值依次是 top、bottom、double，`waterlogged` 变化最快，默认是 bottom 不含水。
const TOP_OAK_SLAB: u32 = blocks::OAK_SLAB - 2;
const DOUBLE_OAK_SLAB: u32 = blocks::OAK_SLAB + 2;
/// `facing` 的取值依次是 north、south、west、east。
const WALL_TORCH_FACING_SOUTH: u32 = blocks::WALL_TORCH + 1;

fn world_with_view_distance(view_distance: u8) -> ServerConfig {
    ServerConfig {
        view_distance,
        ..ServerConfig::default()
    }
}

fn small_world() -> ServerConfig {
    world_with_view_distance(2)
}

fn update(pos: Pos, state: u32) -> BlockUpdate {
    BlockUpdate { pos, state }
}

/// 这个状态属于 `block` 这个方块，属性不管。
fn is_block(state: u32, block: u32) -> bool {
    block_of_state(state) == block_of_state(block)
}

/// 两名玩家都进了世界，从现在开始检查。
async fn two_players(server: &TestServer) -> (Player, Player) {
    let mut alice = server.enter("Alice").await;
    let mut bob = server.enter("Bob").await;
    alice.skip_received();
    bob.skip_received();
    (alice, bob)
}

/// 主手拿着一个 `item` 的玩家。超过物品堆叠上限的数量官方会忽略，所以这里只放一个。
async fn holding(server: &TestServer, name: &str, item: i32) -> Player {
    let mut player = server.enter(name).await;
    player.put_in_hotbar(0, item, 1).await;
    player.skip_received();
    player
}

/// 放一个方块，等服务端确认，返回时这个方块已经写进世界。
async fn place(player: &mut Player, clicked: Pos, face: u8) {
    let sequence = player.click(clicked, face).await;
    player.expect_ack(sequence).await;
}

/// 世界现在的内容：一个新客户端从区块里读到的值。
async fn world_state(server: &TestServer, inspector: &str, pos: Pos) -> u32 {
    let player = server.enter(inspector).await;
    player
        .block_at(pos)
        .unwrap_or_else(|| panic!("{inspector} did not get the chunk of {pos:?}"))
}

/// `player` 收到的这一格的最后一个直发或广播状态，用来断言拒绝放置时的回包。
fn updates_at(player: &Player, mark: usize, pos: Pos) -> Vec<u32> {
    player
        .since::<BlockUpdate>(mark)
        .iter()
        .filter(|packet| packet.pos == pos)
        .map(|packet| packet.state)
        .collect()
}

// ---- 破坏 ----

#[tokio::test]
async fn breaking_a_block_reaches_both_players_and_the_ack_comes_last() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = two_players(&server).await;
    let (alice_mark, bob_mark) = (alice.mark(), bob.mark());

    let sequence = alice.break_block(SPOT).await;

    alice
        .expect::<BlockUpdate>(|packet| *packet == update(SPOT, blocks::AIR))
        .await;
    let ack = alice.expect::<BlockChangedAck>(|_| true).await;
    assert_eq!(ack.sequence, sequence);

    let event = bob.expect::<LevelEvent>(|_| true).await;
    assert_eq!(
        event,
        LevelEvent {
            kind: LEVEL_EVENT_BREAK,
            pos: SPOT,
            data: blocks::GRASS_BLOCK as i32,
            global: false,
        }
    );
    bob.expect::<BlockUpdate>(|packet| *packet == update(SPOT, blocks::AIR))
        .await;

    alice.expect_none::<LevelEvent>(alice_mark, |_| true).await;
    bob.expect_none::<BlockChangedAck>(bob_mark, |_| true).await;
}

#[tokio::test]
async fn a_broken_block_is_gone_from_the_world() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    let sequence = alice.break_block(SPOT).await;
    alice.expect_ack(sequence).await;

    assert_eq!(world_state(&server, "Inspector", SPOT).await, blocks::AIR);
}

#[tokio::test]
async fn the_ack_follows_the_updates_and_there_is_at_most_one_per_tick() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();
    let mark = alice.mark();

    let positions: Vec<Pos> = (6..=11)
        .flat_map(|x| (6..=10).map(move |z| Pos::new(x, -61, z)))
        .collect();
    let mut burst = Vec::new();
    for (index, pos) in positions.iter().enumerate() {
        burst.extend_from_slice(&player_action(
            START_DESTROY_BLOCK,
            *pos,
            UP,
            index as i32 + 1,
        ));
    }
    alice.send(&burst).await;
    alice.expect_ack(positions.len() as i32).await;

    let mut updated = BTreeSet::new();
    let mut acks = Vec::new();
    for frame in &alice.frames()[mark..] {
        if frame.is::<BlockUpdate>() {
            updated.insert(frame.decode::<BlockUpdate>().pos);
        }
        // 同一 tick 在同一段里破坏了几格时，这几格一起发。
        if frame.is::<SectionBlocksUpdate>() {
            let packet = frame.decode::<SectionBlocksUpdate>();
            updated.extend(packet.blocks.iter().map(|&(pos, _)| pos));
        }
        if frame.is::<BlockChangedAck>() {
            let sequence = frame.decode::<BlockChangedAck>().sequence;
            acks.push(sequence);
            assert!(
                positions[..sequence as usize]
                    .iter()
                    .all(|pos| updated.contains(pos)),
                "ack {sequence} arrived before the updates it confirms"
            );
        }
    }
    assert!(acks.is_sorted(), "acks go up: {acks:?}");
    assert_eq!(acks.last(), Some(&(positions.len() as i32)));
    assert!(
        acks.len() <= 3,
        "one packet in one write fits in a tick or two, got acks {acks:?}"
    );
}

#[tokio::test]
async fn a_block_out_of_reach_is_only_acked() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, bob) = two_players(&server).await;
    let (alice_mark, bob_mark) = (alice.mark(), bob.mark());
    let far = Pos::new(28, -61, 8);

    let sequence = alice.break_block(far).await;

    alice.expect_ack(sequence).await;
    alice.expect_none::<BlockUpdate>(alice_mark, |_| true).await;
    bob.expect_none::<BlockUpdate>(bob_mark, |_| true).await;
    bob.expect_none::<LevelEvent>(bob_mark, |_| true).await;
    assert_eq!(
        world_state(&server, "Inspector", far).await,
        blocks::GRASS_BLOCK
    );
}

#[tokio::test]
async fn reach_is_five_blocks_plus_a_one_block_buffer_from_the_eyes() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;

    // 从 (8.5, -58.38, 8.5) 到方块的距离：5.5 格够得着，6.5 格够不着。
    let inside = Pos::new(14, -61, 8);
    let outside = Pos::new(15, -61, 8);
    let first = alice.break_block(inside).await;
    let second = alice.break_block(outside).await;
    alice.expect_ack(second.max(first)).await;

    let inspector = server.enter("Inspector").await;
    assert_eq!(inspector.block_at(inside), Some(blocks::AIR));
    assert_eq!(inspector.block_at(outside), Some(blocks::GRASS_BLOCK));
}

#[tokio::test]
async fn breaking_air_is_only_acked() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, bob) = two_players(&server).await;
    let first = alice.break_block(SPOT).await;
    alice.expect_ack(first).await;
    // 第一次破坏的包也会发给 bob，等两个人都稳定下来再打标记。
    alice.settle().await;
    bob.settle().await;
    let alice_mark = alice.mark();
    let bob_mark = bob.mark();

    let second = alice.break_block(SPOT).await;

    alice.expect_ack(second).await;
    alice.expect_none::<BlockUpdate>(alice_mark, |_| true).await;
    bob.expect_none::<LevelEvent>(bob_mark, |_| true).await;
    bob.expect_none::<BlockUpdate>(bob_mark, |_| true).await;
}

#[tokio::test]
async fn bedrock_at_the_bottom_of_the_world_can_be_broken() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = two_players(&server).await;
    let bottom = Pos::new(8, MIN_Y, 8);

    let sequence = alice.break_block(bottom).await;

    alice.expect_ack(sequence).await;
    let event = bob.expect::<LevelEvent>(|_| true).await;
    assert_eq!(event.data, blocks::BEDROCK as i32);
    bob.expect::<BlockUpdate>(|packet| *packet == update(bottom, blocks::AIR))
        .await;
}

#[tokio::test]
async fn a_sword_cannot_break_blocks_in_creative_mode() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::DIAMOND_SWORD).await;
    let bob = server.enter("Bob").await;
    let bob_mark = bob.mark();

    let sequence = alice.break_block(SPOT).await;

    alice
        .expect::<BlockUpdate>(|packet| *packet == update(SPOT, blocks::GRASS_BLOCK))
        .await;
    alice.expect_ack(sequence).await;
    bob.expect_none::<LevelEvent>(bob_mark, |_| true).await;
    bob.expect_none::<BlockUpdate>(bob_mark, |_| true).await;
}

#[tokio::test]
async fn a_chain_of_breaks_acks_every_sequence() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;

    for x in 6..=11 {
        let sequence = alice.break_block(Pos::new(x, -61, 6)).await;
        alice.expect_ack(sequence).await;
    }

    let inspector = server.enter("Inspector").await;
    for x in 6..=11 {
        assert_eq!(inspector.block_at(Pos::new(x, -61, 6)), Some(blocks::AIR));
    }
}

// ---- 放置 ----

#[tokio::test]
async fn placing_sends_two_direct_updates_a_broadcast_and_the_ack() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    let mut bob = server.enter("Bob").await;
    bob.skip_received();
    let (alice_mark, bob_mark) = (alice.mark(), bob.mark());

    let sequence = alice.click(SPOT, UP).await;

    let clicked = alice.expect::<BlockUpdate>(|_| true).await;
    assert_eq!(clicked, update(SPOT, blocks::GRASS_BLOCK));
    let placed = alice.expect::<BlockUpdate>(|_| true).await;
    assert_eq!(placed, update(ABOVE_SPOT, blocks::STONE));
    let ack = alice.expect::<BlockChangedAck>(|_| true).await;
    assert_eq!(ack.sequence, sequence);

    let seen = bob.expect::<BlockUpdate>(|_| true).await;
    assert_eq!(seen, update(ABOVE_SPOT, blocks::STONE));
    bob.expect_none::<BlockChangedAck>(bob_mark, |_| true).await;
    bob.expect_none::<LevelEvent>(bob_mark, |_| true).await;
    alice.expect_none::<LevelEvent>(alice_mark, |_| true).await;
    assert_eq!(
        world_state(&server, "Inspector", ABOVE_SPOT).await,
        blocks::STONE
    );
}

#[tokio::test]
async fn placing_swings_the_hand_for_observers_only() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    let mut bob = server.enter("Bob").await;
    let alice_id = alice.entity_id;
    bob.expect::<AddEntity>(|packet| packet.id == alice_id)
        .await;
    let alice_mark = alice.mark();

    let sequence = alice.click(SPOT, UP).await;

    let swing = bob.expect::<SwingAnimation>(|_| true).await;
    assert_eq!(
        swing,
        SwingAnimation {
            entity: alice_id,
            hand: MAIN_HAND,
            kind: SWING_WHACK,
            duration: SWING_DURATION,
        }
    );
    alice.expect_ack(sequence).await;
    alice
        .expect_none::<SwingAnimation>(alice_mark, |_| true)
        .await;
}

#[tokio::test]
async fn a_failed_placement_does_not_swing() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    let mut bob = server.enter("Bob").await;
    let alice_id = alice.entity_id;
    bob.expect::<AddEntity>(|packet| packet.id == alice_id)
        .await;
    let mark = bob.mark();

    place(&mut alice, UNDER_SPAWN, UP).await;

    bob.expect_none::<SwingAnimation>(mark, |_| true).await;
}

#[tokio::test]
async fn placing_on_a_side_face_uses_the_neighbor() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    place(&mut alice, SPOT, UP).await;

    place(&mut alice, ABOVE_SPOT, EAST).await;

    assert_eq!(
        world_state(&server, "Inspector", ABOVE_SPOT.neighbor(EAST)).await,
        blocks::STONE
    );
}

#[tokio::test]
async fn a_replaceable_block_is_replaced_where_it_was_clicked() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::SHORT_GRASS).await;
    place(&mut alice, SPOT, UP).await;
    alice.put_in_hotbar(0, items::STONE, 64).await;
    alice.skip_received();
    let mark = alice.mark();

    place(&mut alice, ABOVE_SPOT, UP).await;

    assert_eq!(
        updates_at(&alice, mark, ABOVE_SPOT).first(),
        Some(&blocks::STONE)
    );
    assert_eq!(
        updates_at(&alice, mark, ABOVE_SPOT.above()),
        [blocks::AIR],
        "the block above the replaced one is only reported"
    );
    let inspector = server.enter("Inspector").await;
    assert_eq!(inspector.block_at(ABOVE_SPOT), Some(blocks::STONE));
    assert_eq!(inspector.block_at(ABOVE_SPOT.above()), Some(blocks::AIR));
}

#[tokio::test]
async fn a_replaceable_block_is_not_replaced_by_its_own_item() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::SHORT_GRASS).await;
    place(&mut alice, SPOT, UP).await;

    place(&mut alice, ABOVE_SPOT, UP).await;

    assert_eq!(
        world_state(&server, "Inspector", ABOVE_SPOT).await,
        blocks::SHORT_GRASS
    );
}

#[tokio::test]
async fn a_log_lies_along_the_axis_of_the_clicked_face() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::OAK_LOG).await;

    place(&mut alice, SPOT, UP).await;
    place(&mut alice, ABOVE_SPOT, NORTH).await;

    let inspector = server.enter("Inspector").await;
    assert_eq!(inspector.block_at(ABOVE_SPOT), Some(blocks::OAK_LOG));
    assert_eq!(
        inspector.block_at(ABOVE_SPOT.neighbor(NORTH)),
        Some(OAK_LOG_ALONG_Z)
    );
}

#[tokio::test]
async fn slabs_follow_the_clicked_half_and_merge() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    place(&mut alice, SPOT, UP).await;
    alice.put_in_hotbar(0, items::OAK_SLAB, 64).await;
    let side = ABOVE_SPOT.neighbor(EAST);

    let sequence = alice.next_sequence();
    alice
        .send(&use_item_on(
            MAIN_HAND,
            ABOVE_SPOT,
            EAST,
            (1.0, 0.75, 0.5),
            sequence,
        ))
        .await;
    alice.expect_ack(sequence).await;
    assert_eq!(world_state(&server, "Top", side).await, TOP_OAK_SLAB);

    let sequence = alice.next_sequence();
    alice
        .send(&use_item_on(
            MAIN_HAND,
            side,
            DOWN,
            (0.5, 0.5, 0.5),
            sequence,
        ))
        .await;
    alice.expect_ack(sequence).await;
    assert_eq!(world_state(&server, "Double", side).await, DOUBLE_OAK_SLAB);
}

#[tokio::test]
async fn a_torch_on_a_wall_faces_away_from_it() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    place(&mut alice, SPOT, UP).await;
    alice.put_in_hotbar(0, items::TORCH, 64).await;

    place(&mut alice, ABOVE_SPOT, SOUTH).await;

    assert_eq!(
        world_state(&server, "Inspector", ABOVE_SPOT.neighbor(SOUTH)).await,
        WALL_TORCH_FACING_SOUTH
    );
}

#[tokio::test]
async fn a_torch_goes_with_its_support_and_everyone_gets_the_event() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::TORCH).await;
    let mut bob = server.enter("Bob").await;
    place(&mut alice, SPOT, UP).await;
    assert_eq!(
        world_state(&server, "Inspector", ABOVE_SPOT).await,
        blocks::TORCH
    );
    alice.skip_received();
    bob.skip_received();
    let alice_mark = alice.mark();

    let sequence = alice.break_block(SPOT).await;

    alice.expect_ack(sequence).await;
    let torch = LevelEvent {
        kind: LEVEL_EVENT_BREAK,
        pos: ABOVE_SPOT,
        data: blocks::TORCH as i32,
        global: false,
    };
    assert_eq!(alice.since::<LevelEvent>(alice_mark), [torch]);
    bob.expect::<LevelEvent>(|packet| packet.pos == SPOT).await;
    bob.expect::<LevelEvent>(|packet| *packet == torch).await;
    // 草方块和火把在同一段里，一个 `section_blocks_update` 带上两格。
    let mut gone = bob.expect::<SectionBlocksUpdate>(|_| true).await.blocks;
    gone.sort_unstable();
    assert_eq!(gone, [(SPOT, blocks::AIR), (ABOVE_SPOT, blocks::AIR)]);
    assert_eq!(world_state(&server, "Late", ABOVE_SPOT).await, blocks::AIR);
}

#[tokio::test]
async fn a_solid_block_cannot_go_into_the_own_body() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    let bob = server.enter("Bob").await;
    let (alice_mark, bob_mark) = (alice.mark(), bob.mark());
    let feet = UNDER_SPAWN.above();

    place(&mut alice, UNDER_SPAWN, UP).await;

    assert_eq!(
        updates_at(&alice, alice_mark, UNDER_SPAWN),
        [blocks::GRASS_BLOCK]
    );
    assert_eq!(updates_at(&alice, alice_mark, feet), [blocks::AIR]);
    bob.expect_none::<BlockUpdate>(bob_mark, |_| true).await;
    assert_eq!(world_state(&server, "Inspector", feet).await, blocks::AIR);
}

#[tokio::test]
async fn a_solid_block_cannot_go_into_another_player() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    let mut bob = server.enter("Bob").await;
    bob.move_to(12.5, -60.0, 8.5).await;
    bob.settle().await;
    let target = Pos::new(12, -60, 8);

    place(&mut alice, target.offset(0, -1, 0), UP).await;

    assert_eq!(world_state(&server, "Inspector", target).await, blocks::AIR);
}

#[tokio::test]
async fn a_block_without_collision_can_go_into_a_player() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::POPPY).await;

    place(&mut alice, UNDER_SPAWN, UP).await;

    assert_eq!(
        world_state(&server, "Inspector", UNDER_SPAWN.above()).await,
        blocks::POPPY
    );
}

#[tokio::test]
async fn the_collision_box_shrinks_when_crouching_but_not_when_flying() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    // 脚在 -60.6 会插进草方块。官方 `handleMovePlayer` 把人拉回去，这个位置从移动包到不了。
    // 蹲下变矮、飞行时不变矮，由游戏测试 `only_a_real_crouch_shortens_the_player` 直接放人来查。
    //
    // Feet at -60.6 intersect the grass. Official handleMovePlayer pulls that back, so a move
    // packet cannot reach it. The shorter crouch box, and flying not crouching, is checked by
    // the game test only_a_real_crouch_shortens_the_player, which places the player directly.
    let head = Pos::new(12, -59, 8);
    alice.fly_to(12.5, -60.6, 8.5).await;

    place(&mut alice, head, UP).await;
    assert_eq!(
        world_state(&server, "PulledBack", head).await,
        blocks::STONE
    );
}

#[tokio::test]
async fn a_block_cannot_be_placed_out_of_reach() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    let mark = alice.mark();
    let far = Pos::new(28, -61, 8);

    place(&mut alice, far, UP).await;

    alice.expect_none::<BlockUpdate>(mark, |_| true).await;
    assert_eq!(
        world_state(&server, "Inspector", far.above()).await,
        blocks::AIR
    );
}

#[tokio::test]
async fn a_click_point_off_the_block_is_only_acked() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    let mark = alice.mark();

    for cursor in [(1.6, 0.5, 0.5), (0.5, f32::NAN, 0.5), (0.5, 0.5, -0.6)] {
        let sequence = alice.next_sequence();
        alice
            .send(&use_item_on(MAIN_HAND, SPOT, UP, cursor, sequence))
            .await;
        alice.expect_ack(sequence).await;
    }

    alice.expect_none::<BlockUpdate>(mark, |_| true).await;
    assert_eq!(
        world_state(&server, "Inspector", ABOVE_SPOT).await,
        blocks::AIR
    );
}

#[tokio::test]
async fn an_empty_hand_places_nothing_but_still_reports_both_blocks() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();
    let mark = alice.mark();

    place(&mut alice, SPOT, UP).await;

    assert_eq!(updates_at(&alice, mark, SPOT), [blocks::GRASS_BLOCK]);
    assert_eq!(updates_at(&alice, mark, ABOVE_SPOT), [blocks::AIR]);
}

#[tokio::test]
async fn an_item_that_is_not_a_block_places_nothing() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::DIAMOND_SWORD).await;

    place(&mut alice, SPOT, UP).await;

    assert_eq!(
        world_state(&server, "Inspector", ABOVE_SPOT).await,
        blocks::AIR
    );
}

#[tokio::test]
async fn items_that_cannot_go_on_a_block_place_nothing() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    let mut bob = server.enter("Bob").await;
    bob.skip_received();
    let mark = bob.mark();

    // 只能放水面、要权限。
    let refused = [items::LILY_PAD, items::FROGSPAWN, items::COMMAND_BLOCK];
    for (index, item) in refused.into_iter().enumerate() {
        let spot = Pos::new(6 + index as i32, -61, 10);
        alice.put_in_hotbar(0, item, 1).await;
        alice.skip_received();
        let own_mark = alice.mark();
        place(&mut alice, spot, UP).await;

        assert!(
            updates_at(&alice, own_mark, spot.above())
                .iter()
                .all(|state| *state == blocks::AIR),
            "item {item} was placed"
        );
    }

    bob.expect_none::<BlockUpdate>(mark, |_| true).await;
}

#[tokio::test]
async fn nothing_is_placed_while_a_teleport_is_waiting_for_its_confirmation() {
    let server = TestServer::start(small_world()).await;
    let answers = Answers {
        teleports: false,
        ..Answers::default()
    };
    let mut alice = server.spawn_player("Alice", 2, answers).await;
    alice.wait_for_chunks((0, 0), 2).await;
    alice.put_in_hotbar(0, items::STONE, 64).await;

    place(&mut alice, SPOT, UP).await;

    assert_eq!(
        world_state(&server, "Inspector", ABOVE_SPOT).await,
        blocks::AIR
    );
}

// ---- 世界边界 ----

#[tokio::test]
async fn the_top_of_the_world_takes_a_block_and_refuses_the_next_one() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    alice.fly_to(8.5, 317.0, 8.5).await;
    let top = Pos::new(10, MAX_Y, 8);

    // 点的是空气，它可以被替换，所以方块放在 Y=319。
    let sequence = alice.click(top, UP).await;
    alice
        .expect::<BlockUpdate>(|packet| *packet == update(top, blocks::STONE))
        .await;
    alice.expect_ack(sequence).await;
    alice.settle().await;
    let mark = alice.mark();
    alice.skip_received();

    // 再放一次目标就是 Y=320。
    place(&mut alice, top, UP).await;
    alice.settle().await;

    assert_eq!(updates_at(&alice, mark, top), [blocks::STONE]);
    for hint in alice.since::<SystemChat>(mark) {
        assert!(hint.overlay, "the height hint goes to the action bar");
        assert_eq!(hint.content.translate_keys(), ["build.tooHigh"]);
    }
    alice.assert_world_ticks().await;
}

#[tokio::test]
async fn clicks_and_breaks_outside_the_world_only_get_an_ack() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    let below = Pos::new(8, MIN_Y - 1, 8);

    place(&mut alice, below, UP).await;
    let sequence = alice.break_block(below).await;
    alice.expect_ack(sequence).await;

    alice.fly_to(8.5, 317.0, 8.5).await;
    let above = Pos::new(8, MAX_Y + 1, 8);
    place(&mut alice, above, UP).await;
    let sequence = alice.break_block(above).await;
    alice.expect_ack(sequence).await;

    alice.assert_world_ticks().await;
}

#[tokio::test]
async fn packets_with_hostile_values_do_not_stop_the_world() {
    let server = TestServer::start(small_world()).await;
    let alice = holding(&server, "Alice", items::STONE).await;
    let bob = server.enter("Bob").await;

    let far = 33_554_431;
    let hostile = [
        Pos::new(far, 2047, far),
        Pos::new(-far, -2048, -far),
        Pos::new(0, 2047, 0),
        Pos::new(0, -2048, 0),
        Pos::new(far, 64, 0),
    ];
    let mut burst = Vec::new();
    for pos in hostile {
        for face in [0, 1, 5, 6, 255] {
            burst.extend_from_slice(&player_action(START_DESTROY_BLOCK, pos, face, 1));
            burst.extend_from_slice(&use_item_on(MAIN_HAND, pos, face, (0.5, 0.5, 0.5), 2));
            burst.extend_from_slice(&use_item_on(7, pos, face, (f32::MAX, 0.0, f32::MIN), 3));
        }
        burst.extend_from_slice(&pick_item_from_block(pos));
    }
    for slot in [i16::MIN, -2, -1, 0, 9, 46, 47, i16::MAX] {
        burst.extend_from_slice(&set_carried_item(slot));
        burst.extend_from_slice(&set_creative_mode_slot(slot, items::STONE, 1));
    }
    alice.send(&burst).await;

    alice.assert_world_ticks().await;
    assert!(!alice.is_closed());
    bob.assert_world_ticks().await;
}

#[tokio::test]
async fn a_malformed_action_only_kicks_its_sender() {
    let server = TestServer::start(small_world()).await;
    let alice = server.enter("Alice").await;
    let bob = server.enter("Bob").await;

    // `player_action` 的 action 超出枚举是畸形包。
    alice.send(&player_action(99, SPOT, UP, 1)).await;

    alice.wait_closed().await;
    server.wait_for_players(&["Bob"]).await;
    bob.assert_world_ticks().await;
}

#[tokio::test]
async fn an_unregistered_item_never_reaches_other_players() {
    let server = TestServer::start(small_world()).await;
    let alice = server.enter("Alice").await;
    let mut bob = server.enter("Bob").await;
    let alice_id = alice.entity_id;
    bob.expect::<AddEntity>(|packet| packet.id == alice_id)
        .await;
    let mark = bob.mark();

    // 官方断开这名玩家，规格说 Bamboo 可以只忽略这个包，两种都行。
    // 这个 ID 进了 `set_equipment` 的话，收到的客户端会断线。
    alice
        .send_ignoring_errors(&set_creative_mode_slot(36, 99_999, 1))
        .await;

    bob.expect_none::<SetEquipment>(mark, |_| true).await;
    bob.assert_world_ticks().await;
}

// ---- use_item ----

#[tokio::test]
async fn use_item_is_acked_and_does_nothing_else() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    let mark = alice.mark();

    let sequence = alice.next_sequence();
    alice
        .send(&use_item(MAIN_HAND, sequence, 90.0, -15.5))
        .await;

    let ack = alice.expect::<BlockChangedAck>(|_| true).await;
    assert_eq!(ack.sequence, sequence);
    alice.expect_none::<BlockUpdate>(mark, |_| true).await;
}

#[tokio::test]
async fn actions_that_are_not_block_breaking_are_not_acked() {
    let server = TestServer::start(small_world()).await;
    let alice = holding(&server, "Alice", items::STONE).await;
    let mark = alice.mark();

    alice
        .send(&player_action(RELEASE_USE_ITEM, Pos::new(0, 0, 0), DOWN, 9))
        .await;

    alice.expect_none::<BlockChangedAck>(mark, |_| true).await;
}

// ---- 右键被方块吃掉 ----

#[tokio::test]
async fn right_clicking_a_chest_does_not_place_unless_crouching() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::CHEST).await;
    place(&mut alice, SPOT, UP).await;
    alice.put_in_hotbar(0, items::STONE, 64).await;
    alice.skip_received();
    let mark = alice.mark();
    let on_chest = ABOVE_SPOT.above();

    place(&mut alice, ABOVE_SPOT, UP).await;

    assert_eq!(updates_at(&alice, mark, ABOVE_SPOT), [blocks::CHEST]);
    assert_eq!(updates_at(&alice, mark, on_chest), [blocks::AIR]);
    assert_eq!(
        world_state(&server, "Inspector", on_chest).await,
        blocks::AIR
    );

    alice.send(&player_input(INPUT_SHIFT)).await;
    place(&mut alice, ABOVE_SPOT, UP).await;

    assert_eq!(
        world_state(&server, "Crouched", on_chest).await,
        blocks::STONE
    );
}

#[tokio::test]
async fn blocks_with_a_function_eat_the_right_click() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;

    let consuming = [
        items::CRAFTING_TABLE,
        items::FURNACE,
        items::LEVER,
        items::OAK_TRAPDOOR,
        items::OAK_FENCE_GATE,
        items::NOTE_BLOCK,
    ];
    let mut tops = Vec::new();
    for (index, item) in consuming.into_iter().enumerate() {
        let ground = Pos::new(6 + index as i32, -61, 10);
        alice.put_in_hotbar(0, item, 1).await;
        place(&mut alice, ground, UP).await;
        tops.push(ground.above());
    }

    alice.put_in_hotbar(0, items::STONE, 64).await;
    for top in &tops {
        place(&mut alice, *top, UP).await;
    }

    let inspector = server.enter("Inspector").await;
    for top in tops {
        assert_eq!(
            inspector.block_at(top.above()),
            Some(blocks::AIR),
            "right click on {top:?} must not place"
        );
    }
}

#[tokio::test]
async fn blocks_without_a_function_take_the_block_on_top() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;

    let plain = [
        items::OAK_FENCE,
        items::PUMPKIN,
        items::TNT,
        items::IRON_TRAPDOOR,
    ];
    let mut tops = Vec::new();
    for (index, item) in plain.into_iter().enumerate() {
        let ground = Pos::new(6 + index as i32, -61, 10);
        alice.put_in_hotbar(0, item, 1).await;
        place(&mut alice, ground, UP).await;
        tops.push(ground.above());
    }

    alice.put_in_hotbar(0, items::STONE, 64).await;
    for top in &tops {
        place(&mut alice, *top, UP).await;
    }

    let inspector = server.enter("Inspector").await;
    for top in tops {
        assert_eq!(
            inspector.block_at(top.above()),
            Some(blocks::STONE),
            "right click on {top:?} must place"
        );
    }
}

// ---- 物品栏 ----

#[tokio::test]
async fn placing_uses_the_selected_hotbar_slot() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.put_in_hotbar(0, items::STONE, 64).await;
    alice
        .put_in_hotbar(MAX_HOTBAR_INDEX, items::OAK_LOG, 64)
        .await;

    alice.send(&set_carried_item(MAX_HOTBAR_INDEX)).await;
    place(&mut alice, SPOT, UP).await;

    alice.send(&set_carried_item(0)).await;
    let beside = Pos::new(11, -61, 8);
    place(&mut alice, beside, UP).await;

    let inspector = server.enter("Inspector").await;
    assert_eq!(inspector.block_at(ABOVE_SPOT), Some(blocks::OAK_LOG));
    assert_eq!(inspector.block_at(beside.above()), Some(blocks::STONE));
}

#[tokio::test]
async fn a_selected_slot_out_of_range_is_ignored() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;

    alice.send(&set_carried_item(9)).await;
    alice.send(&set_carried_item(-1)).await;
    place(&mut alice, SPOT, UP).await;

    assert_eq!(
        world_state(&server, "Inspector", ABOVE_SPOT).await,
        blocks::STONE
    );
}

#[tokio::test]
async fn a_cleared_slot_places_nothing() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;

    alice.send(&set_creative_mode_slot(36, 0, 0)).await;
    place(&mut alice, SPOT, UP).await;

    assert_eq!(
        world_state(&server, "Inspector", ABOVE_SPOT).await,
        blocks::AIR
    );
}

#[tokio::test]
async fn creative_slots_are_not_echoed() {
    let server = TestServer::start(small_world()).await;
    let alice = server.enter("Alice").await;
    let mark = alice.mark();

    alice.put_in_hotbar(0, items::STONE, 64).await;
    alice.send(&set_creative_mode_slot(9, items::DIRT, 3)).await;
    alice.send(&set_creative_mode_slot(0, items::DIRT, 3)).await;
    alice
        .send(&set_creative_mode_slot(46, items::DIRT, 3))
        .await;
    alice
        .send(&set_creative_mode_slot(-1, items::DIRT, 3))
        .await;

    alice.expect_none::<ContainerSetSlot>(mark, |_| true).await;
}

#[tokio::test]
async fn dropping_one_item_and_a_stack_mirror_the_client() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    let mut bob = server.enter("Bob").await;
    let alice_id = alice.entity_id;
    bob.expect::<AddEntity>(|packet| packet.id == alice_id)
        .await;
    alice.put_in_hotbar(0, items::STONE, 64).await;
    bob.expect::<SetEquipment>(|packet| packet.entity == alice_id)
        .await;
    let own_mark = alice.mark();

    alice
        .send(&player_action(DROP_ITEM, Pos::new(0, 0, 0), DOWN, 0))
        .await;
    let equipment = bob
        .expect::<SetEquipment>(|packet| packet.entity == alice_id)
        .await;
    assert_eq!(
        equipment.slots,
        [(
            SLOT_MAINHAND,
            Some(Stack {
                item: items::STONE,
                count: 63
            })
        )]
    );

    alice
        .send(&player_action(DROP_ALL_ITEMS, Pos::new(0, 0, 0), DOWN, 0))
        .await;
    let equipment = bob
        .expect::<SetEquipment>(|packet| packet.entity == alice_id)
        .await;
    assert_eq!(equipment.slots, [(SLOT_MAINHAND, None)]);

    // 客户端已经本地扣掉了，不用回包。
    alice
        .expect_none::<ContainerSetSlot>(own_mark, |_| true)
        .await;
    alice
        .expect_none::<BlockChangedAck>(own_mark, |_| true)
        .await;
    place(&mut alice, SPOT, UP).await;
    assert_eq!(
        world_state(&server, "Inspector", ABOVE_SPOT).await,
        blocks::AIR,
        "the hand is empty on the server too"
    );
}

#[tokio::test]
async fn swapped_hands_are_used_for_placing() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;

    alice
        .send(&player_action(
            SWAP_ITEM_WITH_OFFHAND,
            Pos::new(0, 0, 0),
            DOWN,
            0,
        ))
        .await;
    let first = alice.expect::<ContainerSetSlot>(|_| true).await;
    let second = alice.expect::<ContainerSetSlot>(|_| true).await;
    let mut slots = [first, second];
    slots.sort_by_key(|packet| packet.slot);
    assert_eq!(slots[0].slot, 36);
    assert_eq!(slots[0].stack, plain_stack_bytes(0, 0));
    assert_eq!(slots[1].slot, 45);
    assert_eq!(slots[1].stack, plain_stack_bytes(items::STONE, 1));

    // 主手空了，副手放得下。
    place(&mut alice, SPOT, UP).await;
    assert_eq!(world_state(&server, "Main", ABOVE_SPOT).await, blocks::AIR);

    let sequence = alice.next_sequence();
    alice
        .send(&use_item_on(OFF_HAND, SPOT, UP, face_center(UP), sequence))
        .await;
    alice.expect_ack(sequence).await;
    assert_eq!(world_state(&server, "Off", ABOVE_SPOT).await, blocks::STONE);
}

#[tokio::test]
async fn off_hand_placing_swings_the_off_hand() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    let mut bob = server.enter("Bob").await;
    let alice_id = alice.entity_id;
    bob.expect::<AddEntity>(|packet| packet.id == alice_id)
        .await;
    alice
        .send(&set_creative_mode_slot(45, items::STONE, 64))
        .await;

    let sequence = alice.next_sequence();
    alice
        .send(&use_item_on(OFF_HAND, SPOT, UP, face_center(UP), sequence))
        .await;

    let swing = bob.expect::<SwingAnimation>(|_| true).await;
    assert_eq!(swing.hand, OFF_HAND);
    alice.expect_ack(sequence).await;
}

#[tokio::test]
async fn components_come_back_to_the_owner_and_reach_other_players() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    let mut bob = server.enter("Bob").await;
    let alice_id = alice.entity_id;
    bob.expect::<AddEntity>(|packet| packet.id == alice_id)
        .await;

    // 2 个石头，`max_stack_size` = 16，移除 `rarity`，`blocks-inventory.md` 2.5 的实测字节。
    let delimited = [0x02, 0x01, 0x01, 0x01, 0x01, 0x01, 0x10, 0x0c];
    let trusted = [0x02, 0x01, 0x01, 0x01, 0x01, 0x10, 0x0c];
    alice
        .send(&set_creative_mode_slot_raw(36, &delimited))
        .await;
    bob.expect::<SetEquipment>(|packet| packet.entity == alice_id)
        .await;
    alice.skip_received();
    let mark = bob.mark();

    alice
        .send(&player_action(
            SWAP_ITEM_WITH_OFFHAND,
            Pos::new(0, 0, 0),
            DOWN,
            0,
        ))
        .await;

    let slot = alice
        .expect::<ContainerSetSlot>(|packet| packet.slot == 45)
        .await;
    assert_eq!(slot.stack, trusted);
    bob.wait_until("the off hand to change", |frames| {
        frames[mark..]
            .iter()
            .filter(|frame| frame.is::<SetEquipment>())
            .any(|frame| {
                let packet = frame.decode::<SetEquipment>();
                // 别人也看得到组件：上限改成了 16，`rarity` 被移除。
                packet.slot(SLOT_OFFHAND)
                    == Some(Some(Stack {
                        item: items::STONE,
                        count: 2,
                    }))
                    && packet.patch(SLOT_OFFHAND).is_some_and(|patch| {
                        patch.get(data_component_type::MAX_STACK_SIZE)
                            == Some(&ComponentValue::MaxStackSize(16))
                            && patch.entry(data_component_type::RARITY)
                                == Some(&PatchEntry::Removed)
                    })
            })
    })
    .await;
}

#[tokio::test]
async fn picking_a_block_that_is_not_in_the_inventory_fills_the_selected_empty_slot() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();

    alice.send(&pick_item_from_block(SPOT)).await;

    let held = alice.expect::<SetHeldSlot>(|_| true).await;
    assert_eq!(held.slot, 0);
    let slot = alice.expect::<ContainerSetSlot>(|_| true).await;
    assert_eq!(slot.container, 0);
    assert_eq!(slot.slot, 36);
    assert_eq!(slot.stack, plain_stack_bytes(items::GRASS_BLOCK, 1));

    place(&mut alice, SPOT, UP).await;
    assert_eq!(
        world_state(&server, "Inspector", ABOVE_SPOT).await,
        blocks::GRASS_BLOCK
    );
}

#[tokio::test]
async fn picking_a_block_already_on_the_hotbar_only_selects_it() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.put_in_hotbar(3, items::GRASS_BLOCK, 1).await;
    alice.settle().await;
    alice.skip_received();
    let mark = alice.mark();

    alice.send(&pick_item_from_block(SPOT)).await;

    let held = alice.expect::<SetHeldSlot>(|_| true).await;
    assert_eq!(held.slot, 3);
    alice.expect_none::<ContainerSetSlot>(mark, |_| true).await;

    // 客户端切过去之后回发同样的值，服务端照样接受。
    alice.send(&set_carried_item(3)).await;
    place(&mut alice, SPOT, UP).await;
    assert_eq!(
        world_state(&server, "Inspector", ABOVE_SPOT).await,
        blocks::GRASS_BLOCK
    );
}

#[tokio::test]
async fn picking_with_a_full_selected_slot_uses_the_next_empty_one() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;

    alice.send(&pick_item_from_block(SPOT)).await;

    let held = alice.expect::<SetHeldSlot>(|_| true).await;
    assert_eq!(held.slot, 1);
    let slot = alice.expect::<ContainerSetSlot>(|_| true).await;
    assert_eq!(slot.slot, 37);
    assert_eq!(slot.stack, plain_stack_bytes(items::GRASS_BLOCK, 1));
}

#[tokio::test]
async fn picking_a_block_from_the_backpack_swaps_it_into_the_hotbar() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    alice
        .send(&set_creative_mode_slot(20, items::GRASS_BLOCK, 7))
        .await;
    alice.settle().await;
    alice.skip_received();

    alice.send(&pick_item_from_block(SPOT)).await;

    let held = alice.expect::<SetHeldSlot>(|_| true).await;
    assert_eq!(held.slot, 1, "the next empty hotbar slot");
    let first = alice.expect::<ContainerSetSlot>(|_| true).await;
    let second = alice.expect::<ContainerSetSlot>(|_| true).await;
    let mut slots = [first, second];
    slots.sort_by_key(|packet| packet.slot);
    assert_eq!(slots[0].slot, 20);
    assert_eq!(slots[0].stack, plain_stack_bytes(0, 0));
    assert_eq!(slots[1].slot, 37);
    assert_eq!(slots[1].stack, plain_stack_bytes(items::GRASS_BLOCK, 7));
}

#[tokio::test]
async fn picking_a_block_out_of_reach_or_outside_the_world_does_nothing() {
    let server = TestServer::start(small_world()).await;
    let alice = server.enter("Alice").await;
    let mark = alice.mark();

    alice
        .send(&pick_item_from_block(Pos::new(28, -61, 8)))
        .await;
    alice
        .send(&pick_item_from_block(Pos::new(8, MIN_Y - 1, 8)))
        .await;
    alice.send(&pick_item_from_block(ABOVE_SPOT)).await;

    alice.expect_none::<SetHeldSlot>(mark, |_| true).await;
    alice.expect_none::<ContainerSetSlot>(mark, |_| true).await;
}

// ---- 改动和区块 ----

#[tokio::test]
async fn a_late_joiner_gets_the_edited_chunk_and_the_block_entities() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::CHEST).await;
    // `blocks-inventory.md` 第 11 章的实测字节就是这个位置：`35 FF C4 01 00`。
    let chest = Pos::new(3, -60, 5);
    let banner = Pos::new(4, -60, 6);
    let broken = Pos::new(7, -61, 7);
    place(&mut alice, chest.offset(0, -1, 0), UP).await;
    alice.put_in_hotbar(0, items::WHITE_BANNER, 1).await;
    place(&mut alice, banner.offset(0, -1, 0), UP).await;
    let sequence = alice.break_block(broken).await;
    alice.expect_ack(sequence).await;

    let bob = server.enter("Bob").await;

    let chunk = bob.chunk_at(chest);
    assert_eq!(chunk.state_at(chest), blocks::CHEST);
    assert!(is_block(chunk.state_at(banner), blocks::WHITE_BANNER));
    assert_eq!(chunk.state_at(broken), blocks::AIR);
    assert_eq!(chunk.sections[0].block_count, FLAT_BLOCKS_PER_SECTION + 1);

    let entry = chunk
        .block_entity_at(chest)
        .expect("the chest must be listed");
    assert_eq!(entry.packed_xz, 0x35);
    assert_eq!(entry.y, -60);
    assert_eq!(entry.kind, block_entity_type::CHEST);
    // 箱子的 `getUpdateTag` 是空的，官方不写 NBT。
    assert_eq!(entry.data, None);
    let entry = chunk
        .block_entity_at(banner)
        .expect("the banner must be listed");
    assert_eq!(entry.kind, block_entity_type::BANNER);
    // 旗帜走 `saveWithoutMetadata`，至少带一张空的组件表。
    assert_eq!(
        entry.data,
        Some(Nbt::Compound(vec![(
            "components".to_owned(),
            Nbt::Compound(Vec::new())
        )]))
    );
    assert_eq!(chunk.block_entities.len(), 2);
}

#[tokio::test]
async fn a_broken_chest_is_no_longer_listed() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::CHEST).await;
    place(&mut alice, SPOT, UP).await;
    let sequence = alice.break_block(ABOVE_SPOT).await;
    alice.expect_ack(sequence).await;

    let bob = server.enter("Bob").await;

    assert!(bob.chunk_at(ABOVE_SPOT).block_entities.is_empty());
}

#[tokio::test]
async fn a_player_walking_in_from_afar_gets_the_block_entities() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::CHEST).await;
    let mut bob = server.enter("Bob").await;
    bob.walk_to(308.5, 8.5, 8.0).await;
    bob.expect::<ForgetLevelChunk>(|packet| (packet.x, packet.z) == (0, 0))
        .await;

    place(&mut alice, SPOT, UP).await;
    alice.put_in_hotbar(0, items::WHITE_BANNER, 1).await;
    let beside = Pos::new(11, -61, 8);
    place(&mut alice, beside, UP).await;
    bob.wait_for_chunks((19, 0), 2).await;
    bob.walk_to(8.5, 8.5, 8.0).await;
    bob.wait_for_chunks((0, 0), 2).await;

    let chunk = bob.chunk_at(ABOVE_SPOT);
    assert_eq!(chunk.state_at(ABOVE_SPOT), blocks::CHEST);
    assert!(is_block(
        chunk.state_at(beside.above()),
        blocks::WHITE_BANNER
    ));
    let kinds: BTreeSet<i32> = chunk
        .block_entities
        .iter()
        .map(|entry| entry.kind)
        .collect();
    assert_eq!(
        kinds,
        BTreeSet::from([block_entity_type::CHEST, block_entity_type::BANNER])
    );
}

#[tokio::test]
async fn changes_survive_walking_away_and_coming_back() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    place(&mut alice, SPOT, UP).await;
    let hole = Pos::new(11, -61, 8);
    let sequence = alice.break_block(hole).await;
    alice.expect_ack(sequence).await;

    alice.walk_to(308.5, 8.5, 8.0).await;
    alice.wait_for_chunks((19, 0), 2).await;
    alice.walk_to(8.5, 8.5, 8.0).await;
    alice.wait_for_chunks((0, 0), 2).await;

    let chunk = alice.chunk_at(ABOVE_SPOT);
    assert_eq!(chunk.state_at(ABOVE_SPOT), blocks::STONE);
    assert_eq!(chunk.state_at(hole), blocks::AIR);
}

#[tokio::test]
async fn a_chunk_still_waiting_to_be_sent_gets_the_change_in_the_chunk() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    alice.walk_to(40.5, 8.5, 8.0).await;

    // Bob 不确认批次，只会收到第一批的 3×3，(2, 0) 还在 `pending` 里。
    let answers = Answers {
        chunk_batches: false,
        ..Answers::default()
    };
    let bob = server.spawn_player("Bob", 2, answers).await;
    bob.wait_until("the first batch to arrive", |frames| {
        frames
            .iter()
            .any(|frame| frame.id == clientbound::CHUNK_BATCH_FINISHED)
    })
    .await;
    let mark = bob.mark();
    let target = Pos::new(42, -60, 8);

    place(&mut alice, target.offset(0, -1, 0), UP).await;
    bob.expect_none::<BlockUpdate>(mark, |_| true).await;

    bob.start_acknowledging_chunks().await;
    bob.wait_for_chunks((0, 0), 2).await;
    assert_eq!(bob.block_at(target), Some(blocks::STONE));
}

#[tokio::test]
async fn players_that_do_not_track_the_chunk_get_no_update() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    let mut bob = server.enter("Bob").await;
    bob.walk_to(308.5, 8.5, 8.0).await;
    bob.wait_for_chunks((19, 0), 2).await;
    bob.skip_received();
    let mark = bob.mark();

    place(&mut alice, SPOT, UP).await;
    let sequence = alice.break_block(ABOVE_SPOT).await;
    alice.expect_ack(sequence).await;

    bob.expect_none::<BlockUpdate>(mark, |_| true).await;
    bob.expect_none::<LevelEvent>(mark, |_| true).await;
}

#[tokio::test]
async fn the_break_event_only_reaches_players_within_64_blocks() {
    let server = TestServer::start(world_with_view_distance(6)).await;
    let mut alice = server.enter_with_view_distance("Alice", 6).await;
    let mut near = server.enter_with_view_distance("Near", 6).await;
    let mut far = server.enter_with_view_distance("Far", 6).await;
    near.walk_to(60.5, 8.5, 8.0).await;
    far.walk_to(98.5, 8.5, 8.0).await;
    near.wait_for_chunks((3, 0), 6).await;
    far.wait_for_chunks((6, 0), 6).await;
    near.skip_received();
    far.skip_received();
    let far_mark = far.mark();

    let sequence = alice.break_block(SPOT).await;
    alice.expect_ack(sequence).await;

    near.expect::<LevelEvent>(|packet| packet.pos == SPOT).await;
    near.expect::<BlockUpdate>(|packet| *packet == update(SPOT, blocks::AIR))
        .await;
    far.expect::<BlockUpdate>(|packet| *packet == update(SPOT, blocks::AIR))
        .await;
    far.expect_none::<LevelEvent>(far_mark, |_| true).await;
}

#[tokio::test]
async fn breaking_and_placing_the_same_block_at_once_ends_the_same_for_everyone() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    let mut bob = server.enter("Bob").await;

    for _ in 0..20 {
        let place_sequence = alice.next_sequence();
        let break_sequence = bob.next_sequence();
        let placing = use_item_on(MAIN_HAND, SPOT, UP, face_center(UP), place_sequence);
        let breaking = player_action(START_DESTROY_BLOCK, ABOVE_SPOT, UP, break_sequence);
        tokio::join!(alice.send(&placing), bob.send(&breaking));
        sleep(Duration::from_millis(30)).await;
    }
    alice.settle().await;

    let inspector = server.enter("Inspector").await;
    let world = inspector.block_at(ABOVE_SPOT);
    assert_eq!(alice.block_at(ABOVE_SPOT), world);
    assert_eq!(bob.block_at(ABOVE_SPOT), world);
    alice.assert_stream_is_valid();
    bob.assert_stream_is_valid();
}

#[tokio::test]
async fn a_held_item_change_from_another_player_is_visible_on_top_of_block_traffic() {
    let server = TestServer::start(small_world()).await;
    let mut alice = holding(&server, "Alice", items::STONE).await;
    let mut bob = server.enter("Bob").await;
    let alice_id = alice.entity_id;
    bob.expect::<AddEntity>(|packet| packet.id == alice_id)
        .await;

    alice.put_in_hotbar(1, items::DIRT, 5).await;
    alice.send(&set_carried_item(1)).await;
    place(&mut alice, SPOT, UP).await;

    let equipment = bob
        .expect::<SetEquipment>(|packet| {
            packet.entity == alice_id
                && packet.slot(SLOT_MAINHAND)
                    == Some(Some(Stack {
                        item: items::DIRT,
                        count: 5,
                    }))
        })
        .await;
    assert_eq!(equipment.entity, alice_id);
    assert_eq!(
        world_state(&server, "Inspector", ABOVE_SPOT).await,
        blocks::DIRT
    );
}

#[tokio::test]
async fn the_official_block_packets_are_accepted_byte_for_byte() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();

    // `blocks-inventory.md` 3.1 的例子：破坏 (8, -61, 8) 顶面，seq 1。
    let mut packet = vec![serverbound::PLAYER_ACTION as u8];
    packet.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x8f, 0xc3, 0x01, 0x01,
    ]);
    alice.send(&frame(&packet)).await;

    alice
        .expect::<BlockUpdate>(|packet| *packet == update(UNDER_SPAWN, blocks::AIR))
        .await;
    alice.expect_ack(1).await;

    // 放石头：主手，点 (8, -61, 8) 顶面中心，seq 2。先把脚挪开。
    alice.move_to(12.5, -60.0, 12.5).await;
    alice.put_in_hotbar(0, items::STONE, 64).await;
    let mut packet = vec![serverbound::USE_ITEM_ON as u8];
    packet.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x8f, 0xc3, 0x01, 0x3f, 0x00, 0x00, 0x00, 0x3f,
        0x80, 0x00, 0x00, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
    ]);
    alice.send(&frame(&packet)).await;
    alice.expect_ack(2).await;

    assert_eq!(
        world_state(&server, "Inspector", UNDER_SPAWN).await,
        blocks::STONE,
        "the clicked block was air, so the stone goes right there"
    );
}

#[tokio::test]
async fn a_picked_block_shows_up_in_the_main_hand_of_the_observers() {
    let server = TestServer::start(small_world()).await;
    let alice = server.enter("Alice").await;
    let mut bob = server.enter("Bob").await;
    let alice_id = alice.entity_id;
    bob.expect::<AddEntity>(|packet| packet.id == alice_id)
        .await;

    alice.send(&pick_item_from_block(SPOT)).await;

    let equipment = bob
        .expect::<SetEquipment>(|packet| packet.entity == alice_id)
        .await;
    assert_eq!(
        equipment.slots,
        [(
            SLOT_MAINHAND,
            Some(Stack {
                item: items::GRASS_BLOCK,
                count: 1
            })
        )]
    );
    assert!(bob.since::<Disconnect>(0).is_empty());
}
