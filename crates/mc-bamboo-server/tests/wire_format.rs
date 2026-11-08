//! 测试工具自己的检查：`common::wire` 解码规格里的官方字节，编出规格里的官方字节。
//!
//! 这些测试不依赖游戏功能。它们通过了，其他集成测试的失败才说明服务端有问题，不是解码器写错了。

mod common;

use common::{
    TestServer, frame,
    wire::{
        AddEntity, AwardStats, BlockChangedAck, BlockUpdate, ContainerSetSlot, DeltaStep,
        DisguisedChat, EntityPositionSync, EntityValue, FLAG_SHIFT, FLAG_SPRINTING,
        LEVEL_EVENT_BREAK, LevelChunk, LevelEvent, MoveEntityPos, MoveEntityPosRot, MoveEntityRot,
        Nbt, PlayerInfoRemove, PlayerInfoUpdate, Pos, PositionPath, RemoveEntities, RotateHead,
        SLOT_HEAD, SLOT_MAINHAND, SLOT_OFFHAND, START_DESTROY_BLOCK, SetEntityData, SetEquipment,
        SetHeldSlot, Stack, SwingAnimation, SystemChat, UP, chat, chat_command, client_command,
        decode, fixed, pack_degrees, pick_item_from_block, plain_stack_bytes,
        play_client_information, player_abilities, player_action, player_command, player_input,
        punch, set_carried_item, set_creative_mode_slot, tracked_chunks, use_item, use_item_on,
    },
};
use mc_bamboo_data::{blocks, packets::play::serverbound, registries::entity_type};
use mc_bamboo_protocol::{
    codec::write_varint,
    packet::chunk::{Palette, write_paletted_container, write_section_header},
};
use mc_bamboo_server::ServerConfig;
use uuid::Uuid;

const STEVE_UUID: [u8; 16] = [
    0x06, 0x9a, 0x79, 0xf4, 0x44, 0xe9, 0x47, 0x26, 0xa5, 0xbe, 0xfc, 0xa9, 0x0e, 0x38, 0xaa, 0xf5,
];
const SYNC_POSITION: [u8; 24] = [
    0x40, 0x21, 0, 0, 0, 0, 0, 0, 0x40, 0x50, 0x40, 0, 0, 0, 0, 0, 0xc0, 0x0a, 0, 0, 0, 0, 0, 0,
];

/// 包 ID 之后的内容。
fn body_of(framed: &[u8], id: i32) -> Vec<u8> {
    let frame = common::wire::Frame::parse(&framed[1..]);
    assert_eq!(frame.id, id);
    frame.body
}

#[test]
fn block_positions_pack_like_the_official_examples() {
    assert_eq!(
        Pos::new(18, -61, -7).pack().to_be_bytes(),
        [0x00, 0x00, 0x04, 0xbf, 0xff, 0xff, 0x9f, 0xc3]
    );
    assert_eq!(
        Pos::new(-1, -64, 1).pack().to_be_bytes(),
        [0xff, 0xff, 0xff, 0xc0, 0x00, 0x00, 0x1f, 0xc0]
    );
    let pos = Pos::new(-1, -64, 1);
    assert_eq!(Pos::unpack(pos.pack()), pos);
}

#[test]
fn tracked_chunk_counts_match_the_table() {
    for (view_distance, count) in [
        (2, 49),
        (3, 81),
        (4, 117),
        (6, 213),
        (8, 329),
        (10, 473),
        (32, 3725),
    ] {
        assert_eq!(tracked_chunks((0, 0), view_distance).len(), count);
    }
}

#[test]
fn angles_round_down_like_pack_degrees() {
    for (angle, byte) in [
        (0.0, 0x00),
        (45.0, 0x20),
        (90.0, 0x40),
        (-90.0, 0xc0),
        (180.0, 0x80),
        (-180.0, 0x80),
        (179.9, 0x7f),
        (359.9, 0xff),
        (-0.1, 0xff),
        (1.4, 0x00),
        (1.41, 0x01),
    ] {
        assert_eq!(pack_degrees(angle), byte, "{angle}");
    }
}

#[test]
fn fixed_point_rounds_half_up_like_java() {
    assert_eq!(fixed(8.5), 34816);
    assert_eq!(fixed(8.75), 35840);
    assert_eq!(fixed(-3.25), -13312);
    assert_eq!(fixed(-3.375), -13824);
    assert_eq!(fixed(-2.5 / 4096.0), -2);
}

#[test]
fn add_entity_decodes_the_official_bytes() {
    let mut body = vec![0x07];
    body.extend_from_slice(&STEVE_UUID);
    body.extend_from_slice(&[0x9f, 0x01]);
    body.extend_from_slice(&SYNC_POSITION);
    body.extend_from_slice(&[0x00, 0x00, 0x40, 0x40, 0x00]);

    let packet: AddEntity = decode(&body);

    assert_eq!(packet.id, 7);
    assert_eq!(packet.uuid, Uuid::from_bytes(STEVE_UUID));
    assert_eq!(packet.kind, entity_type::PLAYER);
    assert_eq!((packet.x, packet.y, packet.z), (8.5, 65.0, -3.25));
    assert_eq!((packet.pitch, packet.yaw, packet.head_yaw), (0, 0x40, 0x40));
    assert_eq!(packet.data, 0);
}

#[test]
fn player_info_update_decodes_the_official_bytes() {
    let mut body = vec![0xff, 0x01];
    body.extend_from_slice(&STEVE_UUID);
    body.extend_from_slice(&[0x05, b'S', b't', b'e', b'v', b'e']);
    body.extend_from_slice(&[0x00, 0x00, 0x01, 0x01, 0x2a, 0x00, 0x00, 0x01]);

    let packet: PlayerInfoUpdate = decode(&body);

    assert_eq!(packet.actions, PlayerInfoUpdate::ALL_ACTIONS);
    let entry = &packet.entries[0];
    assert_eq!(entry.name.as_deref(), Some("Steve"));
    assert_eq!(entry.game_mode, Some(1));
    assert_eq!(entry.listed, Some(true));
    assert_eq!(entry.latency, Some(42));
    assert_eq!(entry.list_order, Some(0));
    assert_eq!(entry.show_hat, Some(true));

    let mut single = vec![0x80, 0x01];
    single.extend_from_slice(&STEVE_UUID);
    single.push(0x01);
    let hat: PlayerInfoUpdate = decode(&single);
    assert_eq!(hat.entries[0].show_hat, Some(true));
    assert_eq!(hat.entries[0].name, None);
}

#[test]
fn player_info_remove_and_remove_entities_decode_the_official_bytes() {
    let mut body = vec![0x01];
    body.extend_from_slice(&STEVE_UUID);
    let packet: PlayerInfoRemove = decode(&body);
    assert_eq!(packet.uuids, [Uuid::from_bytes(STEVE_UUID)]);

    let packet: RemoveEntities = decode(&[0x02, 0x07, 0xac, 0x02]);
    assert_eq!(packet.ids, [7, 300]);
}

#[test]
fn move_packets_decode_the_official_bytes() {
    let linear: MoveEntityPos = decode(&[0x07, 0x01, 0x04, 0x00, 0x00, 0x00, 0xfe, 0x00]);
    assert!(linear.on_ground);
    assert_eq!(
        linear.steps,
        [DeltaStep {
            ticks: None,
            x: 1024,
            y: 0,
            z: -512
        }]
    );

    let stepped: MoveEntityPos = decode(&[0x07, 0x03, 0x02, 0x04, 0x00, 0x00, 0x00, 0xfe, 0x00]);
    assert_eq!(stepped.steps[0].ticks, Some(2));
    assert_eq!(stepped.steps[0].x, 1024);

    let both: MoveEntityPosRot = decode(&[
        0x07, 0x02, 0x02, 0x04, 0x00, 0x00, 0x00, 0xfe, 0x00, 0x40, 0xe0,
    ]);
    assert!(!both.on_ground);
    assert_eq!((both.yaw, both.pitch), (0x40, 0xe0));

    let rotation: MoveEntityRot = decode(&[0x07, 0x01, 0x40, 0xe0]);
    assert!(rotation.on_ground);
    assert_eq!((rotation.yaw, rotation.pitch), (0x40, 0xe0));

    let head: RotateHead = decode(&[0x07, 0x40]);
    assert_eq!(head.head_yaw, 0x40);
}

#[test]
fn entity_position_sync_decodes_both_path_types() {
    let tail = [0x42, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];

    let mut linear = vec![0x07, 0x00];
    linear.extend_from_slice(&SYNC_POSITION);
    linear.extend_from_slice(&tail);
    let packet: EntityPositionSync = decode(&linear);
    assert_eq!(packet.path, PositionPath::Linear((8.5, 65.0, -3.25)));
    assert_eq!((packet.yaw, packet.pitch), (90.0, 0.0));
    assert!(packet.on_ground);

    let mut stepped = vec![0x07, 0x01, 0x01];
    stepped.extend_from_slice(&SYNC_POSITION);
    stepped.push(0x02);
    stepped.extend_from_slice(&tail);
    let packet: EntityPositionSync = decode(&stepped);
    assert_eq!(
        packet.path,
        PositionPath::Stepped(vec![((8.5, 65.0, -3.25), 2)])
    );
}

#[test]
fn set_entity_data_decodes_the_official_bytes() {
    let packet: SetEntityData = decode(&[
        0x07, 0x00, 0x00, 0x0a, 0x06, 0x14, 0x05, 0x09, 0x03, 0x41, 0xa0, 0x00, 0x00, 0x0f, 0x2a,
        0x00, 0x10, 0x00, 0x7f, 0xff,
    ]);

    assert_eq!(packet.entity, 7);
    assert_eq!(
        packet.value(0),
        Some(EntityValue::Byte(FLAG_SHIFT | FLAG_SPRINTING))
    );
    assert_eq!(packet.value(6), Some(EntityValue::Pose(5)));
    assert_eq!(packet.value(9), Some(EntityValue::Float(20.0)));
    assert_eq!(packet.value(15), Some(EntityValue::Arm(0)));
    assert_eq!(packet.value(16), Some(EntityValue::Byte(0x7f)));
}

#[test]
fn swing_and_equipment_decode_the_official_bytes() {
    let swing: SwingAnimation = decode(&[0x07, 0x00, 0x01, 0x06]);
    assert_eq!(
        swing,
        SwingAnimation {
            entity: 7,
            hand: 0,
            kind: 1,
            duration: 6
        }
    );

    let equipment: SetEquipment = decode(&[
        0x07, 0x80, 0x40, 0x01, 0x00, 0x00, 0x81, 0x00, 0x05, 0x01, 0xbc, 0x08, 0x00, 0x00,
    ]);
    assert_eq!(
        equipment.slots,
        [
            (SLOT_MAINHAND, Some(Stack { item: 1, count: 64 })),
            (SLOT_OFFHAND, None),
            (
                SLOT_HEAD,
                Some(Stack {
                    item: 1084,
                    count: 1
                })
            ),
        ]
    );
}

#[test]
fn block_packets_decode_the_official_bytes() {
    let update: BlockUpdate = decode(&[0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x8f, 0xc4, 0x8c, 0x01]);
    assert_eq!(update.pos, Pos::new(8, -60, 8));
    assert_eq!(update.state, blocks::OAK_LOG);

    let ack: BlockChangedAck = decode(&[0xac, 0x02]);
    assert_eq!(ack.sequence, 300);

    let event: LevelEvent = decode(&[
        0x00, 0x00, 0x07, 0xd1, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x8f, 0xc3, 0x00, 0x00, 0x00,
        0x09, 0x00,
    ]);
    assert_eq!(
        event,
        LevelEvent {
            kind: LEVEL_EVENT_BREAK,
            pos: Pos::new(8, -61, 8),
            data: 9,
            global: false,
        }
    );

    let held: SetHeldSlot = decode(&[0x03]);
    assert_eq!(held.slot, 3);

    let slot: ContainerSetSlot = decode(&[0x00, 0x02, 0x00, 0x24, 0x01, 0x01, 0x00, 0x00]);
    assert_eq!((slot.container, slot.state_id, slot.slot), (0, 2, 36));
    assert_eq!(slot.stack, plain_stack_bytes(1, 1));
}

#[test]
fn chat_packets_decode_the_official_bytes() {
    let mut body = vec![0x0a];
    body.extend_from_slice(b"\x08\x00\x09translate");
    body.extend_from_slice(b"\x00\x19multiplayer.player.joined");
    body.extend_from_slice(b"\x09\x00\x04with\x08\x00\x00\x00\x01\x00\x06Kitten");
    body.extend_from_slice(b"\x08\x00\x05color\x00\x06yellow");
    body.extend_from_slice(&[0x00, 0x00]);

    let joined: SystemChat = decode(&body);
    assert!(!joined.overlay);
    assert_eq!(
        joined.content.translate_keys(),
        ["multiplayer.player.joined"]
    );
    assert_eq!(
        joined.content.color_of("multiplayer.player.joined"),
        Some("yellow")
    );
    let args = joined.content.get("with").and_then(Nbt::as_list).unwrap();
    assert_eq!(args[0].as_str(), Some("Kitten"));

    let message: DisguisedChat = decode(&[
        0x08, 0x00, 0x02, 0x68, 0x69, 0x01, 0x08, 0x00, 0x06, 0x4b, 0x69, 0x74, 0x74, 0x65, 0x6e,
        0x00,
    ]);
    assert_eq!(message.message.as_str(), Some("hi"));
    assert_eq!(message.chat_type, 1);
    assert_eq!(message.name.as_str(), Some("Kitten"));
    assert_eq!(message.target, None);

    let stats: AwardStats = decode(&[0x00]);
    assert_eq!(stats.count, 0);
}

#[test]
fn nbt_strings_use_modified_utf8() {
    let body = [0x08, 0x00, 0x06, 0xe7, 0xab, 0xb9, 0xe5, 0xad, 0x90, 0x00];
    let packet: SystemChat = decode(&body);
    assert_eq!(packet.content.as_str(), Some("竹子"));

    // 😀 写成一对代理项，各占 3 字节。
    let body = [0x08, 0x00, 0x06, 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80, 0x00];
    let packet: SystemChat = decode(&body);
    assert_eq!(packet.content.as_str(), Some("😀"));
}

#[test]
fn colors_are_inherited_by_child_components() {
    let error = Nbt::Compound(vec![
        ("color".to_owned(), Nbt::String("red".to_owned())),
        ("text".to_owned(), Nbt::String(String::new())),
        (
            "extra".to_owned(),
            Nbt::List(vec![Nbt::Compound(vec![(
                "translate".to_owned(),
                Nbt::String("command.unknown.command".to_owned()),
            )])]),
        ),
    ]);

    assert_eq!(error.translate_keys(), ["command.unknown.command"]);
    assert_eq!(error.color_of("command.unknown.command"), Some("red"));
}

#[test]
fn packets_to_the_server_match_the_official_bytes() {
    let cases: [(Vec<u8>, i32, &[u8]); 8] = [
        (
            player_action(START_DESTROY_BLOCK, Pos::new(8, -61, 8), UP, 1),
            serverbound::PLAYER_ACTION,
            &[
                0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x8f, 0xc3, 0x01, 0x01,
            ],
        ),
        (
            use_item_on(0, Pos::new(8, -61, 8), UP, (0.5, 1.0, 0.5), 2),
            serverbound::USE_ITEM_ON,
            &[
                0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x8f, 0xc3, 0x01, 0x3f, 0x00, 0x00, 0x00,
                0x3f, 0x80, 0x00, 0x00, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
            ],
        ),
        (
            use_item(1, 3, 90.0, -15.5),
            serverbound::USE_ITEM,
            &[0x01, 0x03, 0x42, 0xb4, 0x00, 0x00, 0xc1, 0x78, 0x00, 0x00],
        ),
        (
            set_carried_item(4),
            serverbound::SET_CARRIED_ITEM,
            &[0x00, 0x04],
        ),
        (
            set_creative_mode_slot(36, 1, 64),
            serverbound::SET_CREATIVE_MODE_SLOT,
            &[0x00, 0x24, 0x40, 0x01, 0x00, 0x00],
        ),
        (
            set_creative_mode_slot(36, 0, 0),
            serverbound::SET_CREATIVE_MODE_SLOT,
            &[0x00, 0x24, 0x00],
        ),
        (
            pick_item_from_block(Pos::new(8, -61, 8)),
            serverbound::PICK_ITEM_FROM_BLOCK,
            &[0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x8f, 0xc3, 0x00],
        ),
        (
            chat_command("foo bar"),
            serverbound::CHAT_COMMAND,
            &[0x07, 0x66, 0x6f, 0x6f, 0x20, 0x62, 0x61, 0x72],
        ),
    ];
    for (framed, id, expected) in cases {
        assert_eq!(body_of(&framed, id), expected, "packet {id}");
    }

    assert_eq!(
        body_of(&player_input(0x61), serverbound::PLAYER_INPUT),
        [0x61]
    );
    assert_eq!(
        body_of(&player_command(7, 1), serverbound::PLAYER_COMMAND),
        [0x07, 0x01, 0x00]
    );
    assert_eq!(
        body_of(&player_abilities(0x02), serverbound::PLAYER_ABILITIES),
        [0x02]
    );
    assert_eq!(body_of(&punch(), serverbound::PUNCH), [] as [u8; 0]);
    assert_eq!(
        body_of(&client_command(1), serverbound::CLIENT_COMMAND),
        [0x01]
    );
    assert_eq!(
        body_of(&chat("hi"), serverbound::CHAT),
        [
            0x02, 0x68, 0x69, 0x00, 0x00, 0x01, 0x8b, 0xcf, 0xe5, 0x68, 0x00, 0x11, 0x22, 0x33,
            0x44, 0x55, 0x66, 0x77, 0x88, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        ]
    );
    assert_eq!(
        body_of(
            &play_client_information(12, 0x7f, 1),
            serverbound::CLIENT_INFORMATION
        ),
        [
            0x05, b'e', b'n', b'_', b'u', b's', 0x0c, 0x00, 0x01, 0x7f, 0x01, 0x00, 0x01, 0x00,
        ]
    );
}

/// 手写一段区块包：一个 4 位调色板段，一个全局调色板段，其余单值。
fn handmade_chunk() -> Vec<u8> {
    let mut sections = Vec::new();
    // 第 0 段：下标 0 是 9，其余是 0。每个 Long 放 16 个条目。
    write_section_header(&mut sections, 1, 0);
    let mut words = vec![0u64; 256];
    words[0] = 1;
    write_paletted_container(&mut sections, 4, Palette::Indirect(&[0, 9]), &words);
    write_paletted_container(&mut sections, 0, Palette::Single(40), &[]);
    // 第 1 段：15 位全局，每个 Long 放 4 个条目，下标 5 是 12345。
    write_section_header(&mut sections, 4096, 0);
    let mut words = vec![0u64; 1024];
    words[1] = 12345 << 15;
    write_paletted_container(&mut sections, 15, Palette::Global, &words);
    write_paletted_container(&mut sections, 0, Palette::Single(40), &[]);
    for _ in 2..24 {
        write_section_header(&mut sections, 0, 0);
        write_paletted_container(&mut sections, 0, Palette::Single(0), &[]);
        write_paletted_container(&mut sections, 0, Palette::Single(40), &[]);
    }

    let mut body = Vec::new();
    body.extend_from_slice(&1i32.to_be_bytes());
    body.extend_from_slice(&(-1i32).to_be_bytes());
    write_varint(&mut body, 1);
    write_varint(&mut body, 1);
    write_varint(&mut body, 1);
    body.extend_from_slice(&[0; 8]);
    write_varint(&mut body, sections.len() as i32);
    body.extend_from_slice(&sections);
    // 区块内 (3, -60, 5) 的箱子，`blocks-inventory.md` 第 11 章的实测字节。
    body.extend_from_slice(&[0x01, 0x35, 0xff, 0xc4, 0x01, 0x00]);
    // 四个空掩码，没有光照数据。
    body.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    body
}

#[test]
fn chunks_decode_every_kind_of_palette() {
    let chunk: LevelChunk = decode(&handmade_chunk());

    assert_eq!((chunk.x, chunk.z), (1, -1));
    assert_eq!(chunk.state_at(Pos::new(16, -64, -16)), 9);
    assert_eq!(chunk.state_at(Pos::new(17, -64, -16)), 0);
    assert_eq!(chunk.state_at(Pos::new(21, -48, -16)), 12345);
    assert_eq!(chunk.sections[0].block_count, 1);

    let entry = &chunk.block_entities[0];
    assert_eq!((entry.packed_xz, entry.y, entry.kind), (0x35, -60, 1));
    assert_eq!(entry.data, None);
    assert_eq!(chunk.block_entity_at(Pos::new(19, -60, -11)), Some(entry));
}

#[test]
#[should_panic(expected = "trailing bytes")]
fn a_trailing_byte_fails_the_decoder() {
    let _: RemoveEntities = decode(&[0x01, 0x07, 0x00]);
}

#[test]
fn frames_keep_their_id_apart_from_the_body() {
    let framed = frame(&[0x7b, 0x07, 0x00, 0x01, 0x06]);
    let parsed = common::wire::Frame::parse(&framed[1..]);
    assert!(parsed.is::<SwingAnimation>());
    assert_eq!(parsed.body, [0x07, 0x00, 0x01, 0x06]);
}

#[tokio::test]
async fn the_flat_world_decodes_to_the_official_layers() {
    let server = TestServer::start(ServerConfig {
        view_distance: 2,
        ..ServerConfig::default()
    })
    .await;

    let player = server.enter("Kitten").await;

    let chunk = player.chunk_at(Pos::new(3, -64, 5));
    assert_eq!(chunk.state_at(Pos::new(3, -64, 5)), blocks::BEDROCK);
    assert_eq!(chunk.state_at(Pos::new(3, -63, 5)), blocks::DIRT);
    assert_eq!(chunk.state_at(Pos::new(3, -62, 5)), blocks::DIRT);
    assert_eq!(chunk.state_at(Pos::new(3, -61, 5)), blocks::GRASS_BLOCK);
    assert_eq!(chunk.state_at(Pos::new(3, -60, 5)), blocks::AIR);
    assert_eq!(chunk.sections.len(), 24);
    assert_eq!(chunk.sections[0].block_count, 4 * 256);
    assert!(chunk.block_entities.is_empty());
    player.assert_stream_is_valid();
}
