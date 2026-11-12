//! 区块变化广播的测试：直接改世界，驱动 `World::tick`，看客户端收到的帧。

use mc_bamboo_core::sign::SignContent;
use mc_bamboo_data::{blocks, packets::play::clientbound};
use mc_bamboo_protocol::codec::Reader;

use super::*;
use crate::world::tests::{Client, Frame, Harness, body_of, frames_with_id, section_blocks};

const SPAWN_CHUNK: ChunkPos = ChunkPos::new(0, 0);
/// 出生点所在的段，地面在这一段里。
const GROUND_SECTION: i32 = -4;

/// 服务端视距 8，客户端要 2，出生点的玩家跟踪到离 (0, 0) 三个区块为止。
fn settled(server_view_distance: i32) -> (Harness, Client) {
    let mut server = Harness::new(server_view_distance);
    let client = server.join_and_settle("Kitten");
    client.take_frames();
    (server, client)
}

/// 像方块逻辑那样改一格：写进区块，记下要广播的位置。
fn change(server: &mut Harness, pos: BlockPos, state: u32) {
    let region = server.region();
    assert!(region.chunks.set_block(pos, state), "{pos:?} 要真的变了");
    region.block_changes.changed.push(pos);
}

fn frame_of<P: mc_bamboo_protocol::packet::ClientboundPacket>(packet: &P) -> Frame {
    Frame {
        id: P::ID,
        body: body_of(encode_frames(packet)),
    }
}

/// 出生区块地面那一段里，段内坐标是 `local` 的那一格。
fn ground(local: u16) -> BlockPos {
    block_pos(SPAWN_CHUNK, GROUND_SECTION, local)
}

#[test]
fn local_coordinates_round_trip() {
    // `blocks-inventory.md` 2.4 的例子：(18, -61, -7) 在段内是 x2 z9 y3。
    assert_eq!(local_of(BlockPos::new(18, -61, -7)), 0x293);
    for pos in [
        BlockPos::new(18, -61, -7),
        BlockPos::new(-1, -64, -16),
        BlockPos::new(31, -49, -1),
        BlockPos::new(-30_000_000, 319, 29_999_999),
    ] {
        assert_eq!(block_pos(pos.chunk(), pos.y >> 4, local_of(pos)), pos);
    }
}

#[test]
fn one_change_in_a_section_is_a_block_update() {
    let (mut server, client) = settled(8);
    let pos = BlockPos::new(5, -60, 5);
    change(&mut server, pos, blocks::STONE);
    server.tick();

    let expected = frame_of(&BlockUpdate {
        pos,
        state: blocks::STONE as i32,
    });
    assert_eq!(client.take_frames(), [expected]);
}

#[test]
fn a_block_changed_twice_in_a_tick_goes_out_once_with_its_last_state() {
    let (mut server, client) = settled(8);
    let pos = BlockPos::new(5, -60, 5);
    change(&mut server, pos, blocks::STONE);
    change(&mut server, pos, blocks::GLOWSTONE);
    server.tick();

    let expected = frame_of(&BlockUpdate {
        pos,
        state: blocks::GLOWSTONE as i32,
    });
    assert_eq!(client.take_frames(), [expected]);
}

#[test]
fn several_changes_in_a_section_are_one_update_in_the_official_order() {
    let (mut server, client) = settled(8);
    // 段内坐标和 `changed_blocks` 的第一个测试相同，答案也来自官方的 `ShortOpenHashSet`。
    for local in [5, 0, 300, 4095, 17, 37] {
        change(&mut server, ground(local), blocks::STONE);
    }
    server.tick();

    let frames = client.take_frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].id, clientbound::SECTION_BLOCKS_UPDATE);
    let expected: Vec<(BlockPos, u32)> = [0, 37, 300, 5, 17, 4095]
        .into_iter()
        .map(|local| (ground(local), blocks::STONE))
        .collect();
    assert_eq!(section_blocks(&frames[0]), expected);
}

#[test]
fn each_section_picks_its_own_packet_from_the_bottom_up() {
    let (mut server, client) = settled(8);
    let upper = [BlockPos::new(3, -40, 3), BlockPos::new(4, -40, 3)];
    for pos in upper {
        change(&mut server, pos, blocks::STONE);
    }
    let lower = BlockPos::new(3, -60, 3);
    change(&mut server, lower, blocks::DIRT);
    server.tick();

    let frames = client.take_frames();
    let ids: Vec<i32> = frames.iter().map(|frame| frame.id).collect();
    assert_eq!(
        ids,
        [
            clientbound::BLOCK_UPDATE,
            clientbound::SECTION_BLOCKS_UPDATE
        ]
    );
    let mut received = section_blocks(&frames[1]);
    received.sort_unstable();
    assert_eq!(received, upper.map(|pos| (pos, blocks::STONE)));
}

#[test]
fn a_whole_section_goes_out_as_one_packet() {
    let (mut server, client) = settled(8);
    let origin = BlockPos::new(0, -48, 0);
    let mut filled = Vec::new();
    for x in 0..16 {
        for y in 0..16 {
            for z in 0..16 {
                let pos = origin.offset(x, y, z);
                change(&mut server, pos, blocks::STONE);
                filled.push((pos, blocks::STONE));
            }
        }
    }
    server.tick();

    let frames = client.take_frames();
    assert!(frames_with_id(&frames, clientbound::BLOCK_UPDATE).is_empty());
    let updates = frames_with_id(&frames, clientbound::SECTION_BLOCKS_UPDATE);
    assert_eq!(updates.len(), 1);
    let mut received = section_blocks(updates[0]);
    received.sort_unstable();
    filled.sort_unstable();
    assert_eq!(received, filled);
}

/// 一面写着 `text` 的告示牌。
fn place_sign(server: &mut Harness, pos: BlockPos, text: &str) -> SignContent {
    let mut content = SignContent::BLANK;
    content.front.messages[0] = text.to_owned();
    change(server, pos, blocks::OAK_SIGN);
    assert!(server.region().chunks.set_sign(pos, content.clone()));
    content
}

#[test]
fn signs_in_a_section_update_send_their_data_in_the_same_order() {
    let (mut server, client) = settled(8);
    let first = place_sign(&mut server, BlockPos::new(2, -60, 2), "first");
    change(&mut server, BlockPos::new(4, -60, 2), blocks::STONE);
    let second = place_sign(&mut server, BlockPos::new(6, -60, 2), "second");
    server.tick();

    let frames = client.take_frames();
    let ids: Vec<i32> = frames.iter().map(|frame| frame.id).collect();
    assert_eq!(
        ids,
        [
            clientbound::SECTION_BLOCKS_UPDATE,
            clientbound::BLOCK_ENTITY_DATA,
            clientbound::BLOCK_ENTITY_DATA
        ]
    );
    let signs: Vec<BlockPos> = section_blocks(&frames[0])
        .into_iter()
        .filter(|&(_, state)| state == blocks::OAK_SIGN)
        .map(|(pos, _)| pos)
        .collect();
    let contents = [
        (BlockPos::new(2, -60, 2), first),
        (BlockPos::new(6, -60, 2), second),
    ];
    for (frame, pos) in frames[1..].iter().zip(signs) {
        let (_, content) = contents
            .iter()
            .find(|(sign, _)| *sign == pos)
            .expect("只有这两块告示牌");
        let expected = frame_of(&BlockEntityData {
            pos,
            kind: block_of_state(blocks::OAK_SIGN)
                .and_then(|block| block.block_entity_type)
                .expect("告示牌有方块实体"),
            data: &sign::update_tag(content),
        });
        assert_eq!(*frame, expected);
    }
}

/// 光照包和方块包所在的区块。
fn chunk_of(frame: &Frame) -> Option<ChunkPos> {
    let mut reader = Reader::new(&frame.body);
    match frame.id {
        clientbound::LIGHT_UPDATE => Some(ChunkPos::new(
            reader.varint().unwrap(),
            reader.varint().unwrap(),
        )),
        clientbound::BLOCK_UPDATE => Some(reader.block_pos().unwrap().chunk()),
        _ => None,
    }
}

#[test]
fn light_goes_right_before_the_blocks_of_its_own_chunk() {
    // 视距 2 时出生点的玩家跟踪到 x = ±3 为止，这两个区块都在边上，光照要服务端发。
    let (mut server, client) = settled(2);
    let west = ChunkPos::new(-3, 0);
    let east = ChunkPos::new(3, 0);
    change(&mut server, BlockPos::new(-40, -60, 8), blocks::GLOWSTONE);
    change(&mut server, BlockPos::new(56, -60, 8), blocks::GLOWSTONE);
    server.tick();

    let tagged: Vec<(i32, ChunkPos)> = client
        .take_frames()
        .iter()
        .filter_map(|frame| Some((frame.id, chunk_of(frame)?)))
        .collect();
    let index = |id, chunk| {
        tagged
            .iter()
            .position(|&entry| entry == (id, chunk))
            .expect("这个包发了")
    };
    for chunk in [west, east] {
        assert_eq!(
            index(clientbound::BLOCK_UPDATE, chunk),
            index(clientbound::LIGHT_UPDATE, chunk) + 1
        );
    }
    // 官方逐个区块发，不是先发完所有区块的光照。
    assert!(index(clientbound::BLOCK_UPDATE, west) < index(clientbound::LIGHT_UPDATE, east));
}

#[test]
fn changes_nobody_tracks_are_dropped() {
    let mut server = Harness::new(8);
    let far = BlockPos::new(1000, -60, 1000);
    // 远处的区块也要真的交付，改动才记进 `block_changes`；玩家不在那里，
    // 所以「没人跟踪」这条前提不变。
    server.ensure_loaded(&[far.chunk()]);
    let client = server.join_and_settle("Kitten");
    client.take_frames();
    change(&mut server, far, blocks::STONE);
    server.tick();
    assert!(client.take_frames().is_empty());

    // 官方没人收时也清掉这一段的记录，以后来的玩家拿到的是区块本身。
    assert!(server.region().block_changes.changed.is_empty());
    server.tick();
    assert!(client.take_frames().is_empty());
}
