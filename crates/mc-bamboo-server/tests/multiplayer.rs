//! 多名真实 TCP 客户端互相看见。
//!
//! 期望值全部来自 `entities.md`、`coverage.md` 第 4、5、8 节，不看实现。
//! 区块、Tab 列表和进服顺序的断言不需要实体同步；其余断言要等实体同步写完才会通过。

mod common;

use std::{collections::BTreeMap, time::Duration};

use common::{
    ALL_MODEL_PARTS, TestServer, client_information_with,
    player::{Answers, Player, offline_uuid},
    wire::{
        ABILITY_FLYING, ARM_LEFT, ARM_RIGHT, AddEntity, BundleDelimiter, ContainerSetSlot, DOWN,
        DeltaStep, Disconnect, EntityPositionSync, EntityValue, FLAG_SHIFT, FLAG_SPRINTING, Frame,
        INPUT_SHIFT, MoveEntityPos, MoveEntityPosRot, MoveEntityRot, POSE_CROUCHING, POSE_STANDING,
        PlayerInfoRemove, PlayerInfoUpdate, Pos, PositionPath, RemoveEntities, RotateHead,
        SLOT_CHEST, SLOT_FEET, SLOT_HEAD, SLOT_LEGS, SLOT_MAINHAND, SLOT_OFFHAND,
        START_FALL_FLYING, START_SPRINTING, STOP_SPRINTING, SWAP_ITEM_WITH_OFFHAND, SWING_DURATION,
        SWING_WHACK, SetEntityData, SetEquipment, Stack, SwingAnimation, SystemChat, fixed,
        move_position, move_position_rotation, pack_degrees, plain_stack_bytes,
        play_client_information, player_abilities, player_action, player_command, player_input,
        punch, replay_entity, set_carried_item, set_creative_mode_slot,
    },
};
use mc_bamboo_data::{
    items,
    packets::play::{clientbound, serverbound},
    registries::entity_type,
};
use mc_bamboo_server::ServerConfig;
use tokio::time::sleep;
use uuid::Uuid;

const SPAWN: (f64, f64, f64) = (8.5, -60.0, 8.5);
const FULL_HEALTH: f32 = 20.0;
/// 客户端移动一格的定点数。
const FIXED_BLOCK: i64 = 4096;

fn world_with_view_distance(view_distance: u8) -> ServerConfig {
    ServerConfig {
        view_distance,
        ..ServerConfig::default()
    }
}

fn small_world() -> ServerConfig {
    world_with_view_distance(2)
}

/// 两名玩家都进了世界，互相看得见。
async fn paired(server: &TestServer) -> (Player, Player) {
    let mut alice = server.enter("Alice").await;
    let mut bob = server.enter("Bob").await;
    let (alice_id, bob_id) = (alice.entity_id, bob.entity_id);
    alice
        .expect::<AddEntity>(|entity| entity.id == bob_id)
        .await;
    bob.expect::<AddEntity>(|entity| entity.id == alice_id)
        .await;
    (alice, bob)
}

/// 告诉服务端 `player` 站在地上，等 `observer` 看到。之后 `onGround` 不再变，不会插进全量同步。
async fn stand_on_ground(player: &mut Player, observer: &Player) {
    let (x, y, z) = player.position;
    player.move_to(x, y, z).await;
    let id = player.entity_id;
    observer
        .wait_until("the player to stand on the ground", |frames| {
            replay_entity(frames, id).is_some_and(|tracked| tracked.on_ground == Some(true))
        })
        .await;
}

/// 等 `observer` 算出来的位置正好是这里。
async fn wait_until_seen_at(observer: &Player, entity: i32, (x, y, z): (f64, f64, f64)) {
    observer
        .wait_until("the deltas to add up to the real position", |frames| {
            replay_entity(frames, entity).is_some_and(|tracked| tracked.is_at(x, y, z))
        })
        .await;
}

/// 等到一个强制的 delta 为 0 的周期之后才返回。每 60 tick 一次的强制周期会把只改朝向的包变成
/// `move_entity_pos_rot`，把低于阈值的移动发出去，所以断言这两件事的测试要避开它。
async fn wait_for_forced_cycle(observer: &mut Player, entity: i32) {
    observer.skip_received();
    observer
        .expect::<MoveEntityPos>(|packet| {
            packet.entity == entity
                && packet
                    .steps
                    .iter()
                    .all(|step| (step.x, step.y, step.z) == (0, 0, 0))
        })
        .await;
}

/// 这个包所在的 bundle 里除了分隔符的所有包。分隔符必须紧贴着内容。
async fn pairing_bundle(observer: &mut Player, entity: i32) -> Vec<Frame> {
    observer
        .expect::<AddEntity>(|packet| packet.id == entity)
        .await;
    observer.expect::<BundleDelimiter>(|_| true).await;

    let frames = observer.frames();
    let add_entity = frames
        .iter()
        .position(|frame| frame.is::<AddEntity>() && frame.decode::<AddEntity>().id == entity)
        .expect("just received this add_entity");
    let end = add_entity
        + frames[add_entity..]
            .iter()
            .position(Frame::is::<BundleDelimiter>)
            .expect("just received the closing delimiter");
    assert!(
        frames[add_entity - 1].is::<BundleDelimiter>(),
        "add_entity must be the first packet of the bundle"
    );
    frames[add_entity..end].to_vec()
}

/// 配对 bundle 的第二个包。
fn bundle_metadata(bundle: &[Frame]) -> SetEntityData {
    let frame = bundle
        .get(1)
        .expect("set_entity_data comes after add_entity");
    assert!(frame.is::<SetEntityData>(), "second packet: {}", frame.id);
    frame.decode()
}

fn tab_names(frames: &[Frame]) -> BTreeMap<Uuid, String> {
    let mut names = BTreeMap::new();
    for frame in frames {
        if frame.is::<PlayerInfoUpdate>() {
            for entry in frame.decode::<PlayerInfoUpdate>().entries {
                if let Some(name) = entry.name {
                    names.insert(entry.uuid, name);
                }
            }
        }
        if frame.is::<PlayerInfoRemove>() {
            for uuid in frame.decode::<PlayerInfoRemove>().uuids {
                names.remove(&uuid);
            }
        }
    }
    names
}

fn moved(packet: &MoveEntityPos) -> bool {
    packet
        .steps
        .iter()
        .any(|step| (step.x, step.y, step.z) != (0, 0, 0))
}

fn single_step(steps: &[DeltaStep]) -> DeltaStep {
    assert_eq!(steps.len(), 1, "Bamboo always sends one step");
    steps[0]
}

#[tokio::test]
async fn offline_uuid_helper_matches_the_spec_example() {
    assert_eq!(
        offline_uuid("Kitten").as_bytes(),
        &[
            0xdb, 0x80, 0x81, 0x6f, 0xce, 0x8f, 0x33, 0x40, 0xb6, 0xe5, 0x6d, 0xef, 0xcb, 0x8c,
            0x28, 0xef
        ]
    );
}

// ---- 进服和 Tab 列表 ----

#[tokio::test]
async fn second_player_gets_the_merged_join_sequence() {
    let server = TestServer::start(small_world()).await;
    let alice = server.enter("Alice").await;
    let bob = server.enter("Bob").await;

    let ids = bob.ids_since(0);
    let center = ids
        .iter()
        .position(|id| *id == clientbound::SET_CHUNK_CACHE_CENTER)
        .expect("the join ends with set_chunk_cache_center");
    assert_eq!(
        ids[..=center],
        [
            clientbound::LOGIN,
            clientbound::CHANGE_DIFFICULTY,
            clientbound::PLAYER_ABILITIES,
            clientbound::ENTITY_EVENT,
            clientbound::COMMANDS,
            clientbound::PLAYER_POSITION,
            clientbound::PLAYER_INFO_UPDATE,
            clientbound::PLAYER_INFO_UPDATE,
            clientbound::SET_ENTITY_DATA,
            clientbound::UPDATE_ATTRIBUTES,
            clientbound::SET_TIME,
            clientbound::SET_DEFAULT_SPAWN_POSITION,
            clientbound::GAME_EVENT,
            clientbound::SET_CHUNK_CACHE_CENTER,
        ]
    );

    let tabs: Vec<PlayerInfoUpdate> = bob.since(0);
    assert_eq!(tabs[0].actions, PlayerInfoUpdate::ALL_ACTIONS);
    assert_eq!(
        tabs[0].entries.len(),
        1,
        "players already online come first"
    );
    assert_eq!(tabs[0].entries[0].uuid, alice.uuid);
    assert_eq!(tabs[1].actions, PlayerInfoUpdate::ALL_ACTIONS);
    assert_eq!(tabs[1].entries.len(), 1, "then the new player itself");
    assert_eq!(tabs[1].entries[0].uuid, bob.uuid);

    let own_data = &bob.since::<SetEntityData>(0)[0];
    assert_eq!(own_data.entity, bob.entity_id);
    assert_eq!(
        own_data.items,
        [
            (15, EntityValue::Arm(ARM_RIGHT)),
            (16, EntityValue::Byte(ALL_MODEL_PARTS)),
        ],
        "only the server tells the client its own skin parts and main hand"
    );
}

#[tokio::test]
async fn tab_entry_of_a_new_player_has_every_field() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();
    let bob = server.enter("Bob").await;

    let update = alice
        .expect::<PlayerInfoUpdate>(|packet| packet.adds(bob.uuid))
        .await;

    assert_eq!(update.actions, PlayerInfoUpdate::ALL_ACTIONS);
    let entry = update.entry(bob.uuid).expect("the update adds this player");
    assert_eq!(entry.name.as_deref(), Some("Bob"));
    assert_eq!(entry.game_mode, Some(1), "creative");
    assert_eq!(entry.listed, Some(true));
    assert_eq!(entry.latency, Some(0));
    assert_eq!(entry.list_order, Some(0));
    assert_eq!(entry.show_hat, Some(true));
}

#[tokio::test]
async fn hat_in_the_tab_entry_follows_the_client_information() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();
    let information = client_information_with(2, 0x3f, ARM_RIGHT);
    let bob = server
        .spawn_player_with("Bob", &information, Answers::default())
        .await;

    let update = alice
        .expect::<PlayerInfoUpdate>(|packet| packet.adds(bob.uuid))
        .await;
    assert_eq!(update.entries[0].show_hat, Some(false));
}

#[tokio::test]
async fn three_players_list_each_other_in_every_tab_list() {
    let server = TestServer::start(small_world()).await;
    let alice = server.enter("Alice").await;
    let bob = server.enter("Bob").await;
    let carol = server.enter("Carol").await;

    let everyone: BTreeMap<Uuid, String> = [&alice, &bob, &carol]
        .iter()
        .map(|player| (player.uuid, player.name.clone()))
        .collect();
    for player in [&alice, &bob, &carol] {
        player
            .wait_until("the tab list to hold all three players", |frames| {
                tab_names(frames) == everyone
            })
            .await;
    }

    // 先发已在线的两人，再发自己。
    let tabs: Vec<PlayerInfoUpdate> = carol.since(0);
    assert_eq!(tabs[0].entries.len(), 2);
    assert_eq!(tabs[1].entries.len(), 1);
    assert_eq!(tabs[1].entries[0].uuid, carol.uuid);
}

// ---- 配对 ----

#[tokio::test]
async fn tab_entry_comes_before_the_entity_in_a_bundle() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();
    let bob = server.enter("Bob").await;
    let (bob_uuid, bob_id) = (bob.uuid, bob.entity_id);

    alice
        .expect::<PlayerInfoUpdate>(|packet| packet.adds(bob_uuid))
        .await;
    alice.expect::<BundleDelimiter>(|_| true).await;
    let entity = alice
        .expect::<AddEntity>(|packet| packet.id == bob_id)
        .await;
    alice.expect::<BundleDelimiter>(|_| true).await;

    assert_eq!(entity.uuid, bob_uuid);
    alice.assert_stream_is_valid();
}

#[tokio::test]
async fn own_entity_is_never_added_to_the_own_client() {
    let server = TestServer::start(small_world()).await;
    let (alice, bob) = paired(&server).await;
    alice.settle().await;

    for player in [&alice, &bob] {
        let own = player.entity_id;
        assert!(
            player
                .since::<AddEntity>(0)
                .iter()
                .all(|entity| entity.id != own),
            "{} was told to add itself, which removes the local player",
            player.name
        );
    }
}

#[tokio::test]
async fn add_entity_reports_the_last_synced_state() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    let position = (10.5, -60.0, 9.25);
    alice
        .send(&move_position_rotation(position, 90.0, -45.0, true))
        .await;
    alice.position = position;
    alice.settle().await;

    let mut bob = server.enter("Bob").await;
    let alice_id = alice.entity_id;
    let entity = bob
        .expect::<AddEntity>(|packet| packet.id == alice_id)
        .await;

    assert_eq!(entity.uuid, alice.uuid);
    assert_eq!(entity.kind, entity_type::PLAYER);
    assert_eq!((entity.x, entity.y, entity.z), position);
    assert_eq!(entity.yaw, 0x40);
    assert_eq!(entity.pitch, 0xe0);
    assert_eq!(entity.head_yaw, 0x40);
    assert_eq!(entity.data, 0);
}

#[tokio::test]
async fn pairing_bundle_carries_the_non_default_metadata() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    let mut bob = server.enter("Bob").await;
    let (alice_id, bob_id) = (alice.entity_id, bob.entity_id);

    let bundle = pairing_bundle(&mut alice, bob_id).await;
    pairing_bundle(&mut bob, alice_id).await;

    let data = bundle_metadata(&bundle);
    assert_eq!(data.entity, bob_id);
    assert_eq!(
        data.items,
        [
            (9, EntityValue::Float(FULL_HEALTH)),
            (16, EntityValue::Byte(ALL_MODEL_PARTS)),
        ],
        "right handed players only differ from the defaults in health and skin"
    );
}

#[tokio::test]
async fn pairing_metadata_reports_a_left_hand_and_skin_parts() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    let information = client_information_with(2, 0x15, ARM_LEFT);
    let bob = server
        .spawn_player_with("Bob", &information, Answers::default())
        .await;
    let bob_id = bob.entity_id;

    let bundle = pairing_bundle(&mut alice, bob_id).await;

    let data = bundle_metadata(&bundle);
    assert_eq!(
        data.items,
        [
            (9, EntityValue::Float(FULL_HEALTH)),
            (15, EntityValue::Arm(ARM_LEFT)),
            (16, EntityValue::Byte(0x15)),
        ]
    );
}

#[tokio::test]
async fn pairing_bundle_carries_the_equipment() {
    let server = TestServer::start(small_world()).await;
    let alice = server.enter("Alice").await;
    alice
        .send(&set_creative_mode_slot(36, items::STONE, 64))
        .await;
    alice
        .send(&set_creative_mode_slot(45, items::DIRT, 3))
        .await;
    alice.settle().await;

    let mut bob = server.enter("Bob").await;
    let bundle = pairing_bundle(&mut bob, alice.entity_id).await;

    let equipment = bundle
        .iter()
        .find(|frame| frame.is::<SetEquipment>())
        .expect("the pairing bundle lists the non-empty slots")
        .decode::<SetEquipment>();
    assert_eq!(equipment.entity, alice.entity_id);
    assert_eq!(
        equipment.slots,
        [
            (
                SLOT_MAINHAND,
                Some(Stack {
                    item: items::STONE,
                    count: 64
                })
            ),
            (
                SLOT_OFFHAND,
                Some(Stack {
                    item: items::DIRT,
                    count: 3
                })
            ),
        ]
    );
    assert_eq!(
        bundle_metadata(&bundle).entity,
        alice.entity_id,
        "metadata comes before equipment"
    );
}

#[tokio::test]
async fn pairing_bundle_has_no_equipment_when_the_hands_are_empty() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    let bob = server.enter("Bob").await;

    let bundle = pairing_bundle(&mut alice, bob.entity_id).await;

    assert!(bundle.iter().all(|frame| !frame.is::<SetEquipment>()));
}

// ---- 持续同步 ----

#[tokio::test]
async fn walking_deltas_add_up_to_the_real_position() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    stand_on_ground(&mut alice, &bob).await;
    let id = alice.entity_id;
    let mark = bob.mark();

    // 走过区块边界，步长不是 1/4096 的整数倍，这样四舍五入错了就对不上。
    let mut x = SPAWN.0;
    for _ in 0..40 {
        x += 0.2137;
        alice.move_to(x, SPAWN.1, SPAWN.2).await;
        sleep(Duration::from_millis(50)).await;
    }
    wait_until_seen_at(&bob, id, (x, SPAWN.1, SPAWN.2)).await;

    let moves: Vec<MoveEntityPos> = bob.since(mark);
    assert!(moves.len() >= 10, "walking must go out as deltas");
    assert!(moves.iter().all(|packet| packet.entity == id));
    assert!(moves.iter().all(|packet| packet.on_ground));
    assert!(
        bob.since::<EntityPositionSync>(mark).is_empty(),
        "nothing here needs a full sync"
    );
    bob.skip_received();
    bob.assert_stream_is_valid();
}

#[tokio::test]
async fn after_the_first_cycle_deltas_are_single_steps_of_one_or_two_ticks() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, bob) = paired(&server).await;
    stand_on_ground(&mut alice, &bob).await;
    let mark = bob.mark();

    let mut x = SPAWN.0;
    for _ in 0..20 {
        x += 0.25;
        alice.move_to(x, SPAWN.1, SPAWN.2).await;
        sleep(Duration::from_millis(50)).await;
    }
    wait_until_seen_at(&bob, alice.entity_id, (x, SPAWN.1, SPAWN.2)).await;

    for packet in bob.since::<MoveEntityPos>(mark) {
        let ticks = single_step(&packet.steps).ticks;
        assert!(matches!(ticks, Some(1 | 2)), "ticks of a step: {ticks:?}");
    }
}

#[tokio::test]
async fn a_delta_is_the_difference_of_two_fixed_point_positions() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    stand_on_ground(&mut alice, &bob).await;
    bob.skip_received();
    let id = alice.entity_id;

    alice.move_to(8.75, -60.0, 8.375).await;

    let packet = bob
        .expect::<MoveEntityPos>(|packet| packet.entity == id && moved(packet))
        .await;
    let step = single_step(&packet.steps);
    assert_eq!(i64::from(step.x), fixed(8.75) - fixed(8.5));
    assert_eq!(step.y, 0);
    assert_eq!(i64::from(step.z), fixed(8.375) - fixed(8.5));
    assert_eq!(i64::from(step.x), FIXED_BLOCK / 4);
    assert!(packet.on_ground);
}

#[tokio::test]
async fn turning_sends_rotation_and_head_yaw() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    stand_on_ground(&mut alice, &bob).await;
    let id = alice.entity_id;
    wait_for_forced_cycle(&mut bob, id).await;

    alice.turn(90.0, -45.0).await;

    let rotation = bob
        .expect::<MoveEntityRot>(|packet| packet.entity == id)
        .await;
    assert_eq!(
        rotation,
        MoveEntityRot {
            entity: id,
            on_ground: true,
            yaw: pack_degrees(90.0),
            pitch: pack_degrees(-45.0),
        }
    );
    let head = bob.expect::<RotateHead>(|packet| packet.entity == id).await;
    assert_eq!(head.head_yaw, 0x40);
}

#[tokio::test]
async fn moving_and_turning_goes_out_as_one_packet_then_the_head() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    stand_on_ground(&mut alice, &bob).await;
    bob.skip_received();
    let id = alice.entity_id;

    alice
        .send(&move_position_rotation(
            (9.0, -60.0, 8.5),
            -90.0,
            30.0,
            true,
        ))
        .await;

    let packet = bob
        .expect::<MoveEntityPosRot>(|packet| packet.entity == id)
        .await;
    assert_eq!(i64::from(single_step(&packet.steps).x), FIXED_BLOCK / 2);
    assert_eq!(packet.yaw, 0xc0);
    assert_eq!(packet.pitch, pack_degrees(30.0));
    assert!(packet.on_ground);
    let head = bob.expect::<RotateHead>(|packet| packet.entity == id).await;
    assert_eq!(head.head_yaw, 0xc0);
}

#[tokio::test]
async fn pitch_is_clamped_and_yaw_is_wrapped_before_it_is_packed() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    stand_on_ground(&mut alice, &bob).await;
    bob.skip_received();
    let id = alice.entity_id;

    alice.turn(450.0, 120.0).await;

    bob.wait_until("the rotation to arrive", |frames| {
        replay_entity(frames, id).is_some_and(|tracked| {
            tracked.yaw == pack_degrees(90.0) && tracked.pitch == pack_degrees(90.0)
        })
    })
    .await;
}

#[tokio::test]
async fn jumping_and_landing_each_send_a_full_position_sync() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    stand_on_ground(&mut alice, &bob).await;
    bob.skip_received();
    let id = alice.entity_id;

    alice.fly_to(8.5, -58.75, 8.5).await;
    let jump = bob
        .expect::<EntityPositionSync>(|packet| packet.entity == id)
        .await;
    assert_eq!(jump.path.end(), (8.5, -58.75, 8.5));
    assert!(!jump.on_ground);

    alice
        .send(&move_position_rotation((8.5, -60.0, 8.5), 90.0, 0.0, true))
        .await;
    let landing = bob
        .expect::<EntityPositionSync>(|packet| packet.entity == id)
        .await;
    assert_eq!(landing.path.end(), (8.5, -60.0, 8.5));
    assert!(landing.on_ground);
    assert_eq!(landing.yaw, 90.0, "the sync carries real angles, not bytes");
    assert_eq!(landing.pitch, 0.0);
}

#[tokio::test]
async fn position_sync_after_the_first_cycle_is_a_single_step_path() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    stand_on_ground(&mut alice, &bob).await;
    bob.skip_received();
    let id = alice.entity_id;

    alice.fly_to(8.5, -59.0, 8.5).await;

    let sync = bob
        .expect::<EntityPositionSync>(|packet| packet.entity == id)
        .await;
    let PositionPath::Stepped(steps) = sync.path else {
        panic!("expected a stepped path, got {:?}", sync.path);
    };
    assert_eq!(steps.len(), 1);
    assert!(
        matches!(steps[0].1, 1 | 2),
        "ticks of a step: {}",
        steps[0].1
    );
}

#[tokio::test]
async fn a_move_too_big_for_a_delta_is_a_position_sync_and_resets_the_base() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    stand_on_ground(&mut alice, &bob).await;
    bob.skip_received();
    let mark = bob.mark();
    let id = alice.entity_id;

    alice.move_to(18.5, -60.0, 8.5).await;
    let sync = bob
        .expect::<EntityPositionSync>(|packet| packet.entity == id)
        .await;
    assert_eq!(sync.path.end(), (18.5, -60.0, 8.5));
    assert!(sync.on_ground);

    // 下一个 delta 从新的基准算起。
    alice.move_to(18.75, -60.0, 8.5).await;
    let delta = bob
        .expect::<MoveEntityPos>(|packet| packet.entity == id && moved(packet))
        .await;
    assert_eq!(i64::from(single_step(&delta.steps).x), FIXED_BLOCK / 4);
    wait_until_seen_at(&bob, id, (18.75, -60.0, 8.5)).await;

    assert!(
        !bob.ids_since(mark).contains(&clientbound::TELEPORT_ENTITY),
        "teleport_entity does not update the client's delta base"
    );
}

#[tokio::test]
async fn a_player_standing_still_gets_a_zero_delta_every_sixty_ticks() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    stand_on_ground(&mut alice, &bob).await;
    bob.skip_received();
    let id = alice.entity_id;

    let packet = bob
        .expect::<MoveEntityPos>(|packet| packet.entity == id)
        .await;

    let step = single_step(&packet.steps);
    assert_eq!((step.x, step.y, step.z), (0, 0, 0));
}

#[tokio::test]
async fn a_move_below_the_threshold_is_not_sent_until_it_adds_up() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    stand_on_ground(&mut alice, &bob).await;
    let id = alice.entity_id;

    // 阈值是距离平方 2^-17，大约 0.00276 格。
    wait_for_forced_cycle(&mut bob, id).await;
    let mark = bob.mark();
    alice.move_to(8.502, -60.0, 8.5).await;
    bob.expect_none::<MoveEntityPos>(mark, moved).await;

    alice.move_to(8.51, -60.0, 8.5).await;
    wait_until_seen_at(&bob, id, (8.51, -60.0, 8.5)).await;
}

#[tokio::test]
async fn two_players_walking_at_once_stay_in_step() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    stand_on_ground(&mut alice, &bob).await;
    stand_on_ground(&mut bob, &alice).await;

    let (mut alice_x, mut bob_x) = (SPAWN.0, SPAWN.0);
    for _ in 0..40 {
        alice_x += 0.31;
        bob_x -= 0.27;
        alice.move_to(alice_x, SPAWN.1, SPAWN.2).await;
        bob.move_to(bob_x, SPAWN.1, SPAWN.2).await;
        sleep(Duration::from_millis(50)).await;
    }

    wait_until_seen_at(&bob, alice.entity_id, alice.position).await;
    wait_until_seen_at(&alice, bob.entity_id, bob.position).await;
    alice.assert_stream_is_valid();
    bob.assert_stream_is_valid();
}

// ---- 元数据 ----

#[tokio::test]
async fn sneaking_reaches_the_player_and_its_observers() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    alice.skip_received();
    bob.skip_received();
    let id = alice.entity_id;

    alice.send(&player_input(INPUT_SHIFT)).await;
    for viewer in [&mut alice, &mut bob] {
        let data = viewer
            .expect::<SetEntityData>(|packet| packet.entity == id)
            .await;
        assert_eq!(
            data.items,
            [
                (0, EntityValue::Byte(FLAG_SHIFT)),
                (6, EntityValue::Pose(POSE_CROUCHING)),
            ],
            "{} must get the changed indices only",
            viewer.name
        );
    }

    alice.send(&player_input(0)).await;
    for viewer in [&mut alice, &mut bob] {
        let data = viewer
            .expect::<SetEntityData>(|packet| packet.entity == id)
            .await;
        assert_eq!(
            data.items,
            [
                (0, EntityValue::Byte(0)),
                (6, EntityValue::Pose(POSE_STANDING)),
            ]
        );
    }
}

#[tokio::test]
async fn other_input_keys_change_nothing() {
    let server = TestServer::start(small_world()).await;
    let (alice, bob) = paired(&server).await;
    let id = alice.entity_id;
    let mark = bob.mark();

    // 前进 + 跳 + 冲刺键。
    alice.send(&player_input(0x51)).await;

    bob.expect_none::<SetEntityData>(mark, |packet| packet.entity == id)
        .await;
}

#[tokio::test]
async fn metadata_comes_after_the_move_of_the_same_cycle() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    stand_on_ground(&mut alice, &bob).await;
    bob.skip_received();
    let id = alice.entity_id;

    let mut both = move_position(8.75, -60.0, 8.5, true);
    both.extend_from_slice(&player_input(INPUT_SHIFT));
    alice.send(&both).await;

    bob.expect::<MoveEntityPos>(|packet| packet.entity == id && moved(packet))
        .await;
    bob.expect::<SetEntityData>(|packet| packet.entity == id)
        .await;
}

#[tokio::test]
async fn shift_while_flying_is_descending_not_crouching() {
    let server = TestServer::start(small_world()).await;
    let (alice, mut bob) = paired(&server).await;
    bob.skip_received();
    let id = alice.entity_id;

    alice.send(&player_abilities(ABILITY_FLYING)).await;
    alice.send(&player_input(INPUT_SHIFT)).await;
    let data = bob
        .expect::<SetEntityData>(|packet| packet.entity == id)
        .await;
    assert_eq!(
        data.items,
        [(0, EntityValue::Byte(FLAG_SHIFT))],
        "the shift bit is set but the pose stays standing"
    );

    // 落地时还按着潜行键，姿势才变。
    alice.send(&player_abilities(0)).await;
    let data = bob
        .expect::<SetEntityData>(|packet| packet.entity == id)
        .await;
    assert_eq!(data.items, [(6, EntityValue::Pose(POSE_CROUCHING))]);
}

#[tokio::test]
async fn sprinting_sets_and_clears_its_flag() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    alice.skip_received();
    bob.skip_received();
    let id = alice.entity_id;

    alice.send(&player_command(id, START_SPRINTING)).await;
    for viewer in [&mut alice, &mut bob] {
        let data = viewer
            .expect::<SetEntityData>(|packet| packet.entity == id)
            .await;
        assert_eq!(data.items, [(0, EntityValue::Byte(FLAG_SPRINTING))]);
    }

    // 两个标志在同一个字节里。
    alice.send(&player_input(INPUT_SHIFT)).await;
    let data = bob
        .expect::<SetEntityData>(|packet| packet.entity == id)
        .await;
    assert_eq!(
        data.value(0),
        Some(EntityValue::Byte(FLAG_SPRINTING | FLAG_SHIFT))
    );

    alice.send(&player_command(id, STOP_SPRINTING)).await;
    let data = bob
        .expect::<SetEntityData>(|packet| packet.entity == id)
        .await;
    assert_eq!(data.items, [(0, EntityValue::Byte(FLAG_SHIFT))]);
}

#[tokio::test]
async fn fall_flying_is_refused_to_the_sender() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    alice.skip_received();
    let id = alice.entity_id;

    alice.send(&player_command(id, START_FALL_FLYING)).await;

    let data = alice
        .expect::<SetEntityData>(|packet| packet.entity == id)
        .await;
    assert_eq!(data.items, [(0, EntityValue::Byte(0))]);
}

#[tokio::test]
async fn fall_flying_refusal_keeps_the_other_flags_and_stays_private() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    let id = alice.entity_id;
    alice.send(&player_command(id, START_SPRINTING)).await;
    bob.expect::<SetEntityData>(|packet| packet.entity == id)
        .await;
    alice
        .expect::<SetEntityData>(|packet| packet.entity == id)
        .await;
    alice.settle().await;
    alice.skip_received();
    let mark = bob.mark();

    alice.send(&player_command(id, START_FALL_FLYING)).await;

    let data = alice
        .expect::<SetEntityData>(|packet| packet.entity == id)
        .await;
    assert_eq!(data.items, [(0, EntityValue::Byte(FLAG_SPRINTING))]);
    bob.expect_none::<SetEntityData>(mark, |packet| packet.entity == id)
        .await;
}

#[tokio::test]
async fn changing_skin_and_hand_reaches_everyone() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    alice.skip_received();
    bob.skip_received();
    let id = alice.entity_id;

    alice
        .send(&play_client_information(2, 0x3f, ARM_LEFT))
        .await;

    for viewer in [&mut alice, &mut bob] {
        let data = viewer
            .expect::<SetEntityData>(|packet| packet.entity == id)
            .await;
        assert_eq!(
            data.items,
            [
                (15, EntityValue::Arm(ARM_LEFT)),
                (16, EntityValue::Byte(0x3f)),
            ]
        );
    }
}

#[tokio::test]
async fn changing_the_hat_updates_every_tab_list() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    let mut bob = server.enter("Bob").await;
    alice.skip_received();
    bob.skip_received();
    let uuid = alice.uuid;

    alice
        .send(&play_client_information(2, 0x3f, ARM_RIGHT))
        .await;

    for viewer in [&mut alice, &mut bob] {
        let update = viewer
            .expect::<PlayerInfoUpdate>(|packet| packet.actions == 0x80)
            .await;
        assert_eq!(
            update.entry(uuid).and_then(|entry| entry.show_hat),
            Some(false)
        );
    }
}

// ---- 挥手 ----

#[tokio::test]
async fn punch_swings_the_main_hand_for_observers_only() {
    let server = TestServer::start(small_world()).await;
    let (alice, mut bob) = paired(&server).await;
    bob.skip_received();
    let own_mark = alice.mark();
    let id = alice.entity_id;

    alice.send(&punch()).await;

    let swing = bob
        .expect::<SwingAnimation>(|packet| packet.entity == id)
        .await;
    assert_eq!(
        swing,
        SwingAnimation {
            entity: id,
            hand: 0,
            kind: SWING_WHACK,
            duration: SWING_DURATION,
        }
    );
    alice
        .expect_none::<SwingAnimation>(own_mark, |_| true)
        .await;
}

#[tokio::test]
async fn punch_is_silent_when_nobody_is_watching() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;

    alice.send(&punch()).await;

    alice.assert_world_ticks().await;
}

// ---- 装备 ----

#[tokio::test]
async fn held_item_changes_reach_observers_only() {
    let server = TestServer::start(small_world()).await;
    let (alice, mut bob) = paired(&server).await;
    bob.skip_received();
    let own_mark = alice.mark();
    let id = alice.entity_id;

    alice
        .send(&set_creative_mode_slot(36, items::STONE, 64))
        .await;
    let equipment = bob
        .expect::<SetEquipment>(|packet| packet.entity == id)
        .await;
    assert_eq!(
        equipment.slots,
        [(
            SLOT_MAINHAND,
            Some(Stack {
                item: items::STONE,
                count: 64
            })
        )]
    );

    // 换到空的格子，主手变空，count 写 0。
    alice.send(&set_carried_item(1)).await;
    let equipment = bob
        .expect::<SetEquipment>(|packet| packet.entity == id)
        .await;
    assert_eq!(equipment.slots, [(SLOT_MAINHAND, None)]);

    alice.send(&set_carried_item(0)).await;
    let equipment = bob
        .expect::<SetEquipment>(|packet| packet.entity == id)
        .await;
    assert_eq!(
        equipment.slot(SLOT_MAINHAND),
        Some(Some(Stack {
            item: items::STONE,
            count: 64
        }))
    );

    alice.expect_none::<SetEquipment>(own_mark, |_| true).await;
}

#[tokio::test]
async fn armor_and_off_hand_use_the_enum_ordinals() {
    let server = TestServer::start(small_world()).await;
    let (alice, mut bob) = paired(&server).await;
    bob.skip_received();
    let id = alice.entity_id;
    let stack = Stack {
        item: items::DIAMOND_HELMET,
        count: 1,
    };

    // 菜单槽位：5 头、6 胸、7 腿、8 脚、45 副手。
    for (menu_slot, ordinal) in [
        (5, SLOT_HEAD),
        (6, SLOT_CHEST),
        (7, SLOT_LEGS),
        (8, SLOT_FEET),
        (45, SLOT_OFFHAND),
    ] {
        alice
            .send(&set_creative_mode_slot(menu_slot, stack.item, stack.count))
            .await;
        let equipment = bob
            .expect::<SetEquipment>(|packet| packet.entity == id)
            .await;
        assert_eq!(equipment.slots, [(ordinal, Some(stack))]);
    }
}

#[tokio::test]
async fn several_slots_changing_at_once_are_sorted_by_ordinal() {
    let server = TestServer::start(small_world()).await;
    let (alice, bob) = paired(&server).await;
    let mark = bob.mark();
    let id = alice.entity_id;

    let mut all = set_creative_mode_slot(5, items::DIAMOND_HELMET, 1);
    all.extend_from_slice(&set_creative_mode_slot(45, items::DIRT, 2));
    all.extend_from_slice(&set_creative_mode_slot(36, items::STONE, 3));
    alice.send(&all).await;

    bob.wait_until("all three slots to arrive", |frames| {
        let seen: Vec<u8> = frames[mark..]
            .iter()
            .filter(|frame| frame.is::<SetEquipment>())
            .flat_map(|frame| frame.decode::<SetEquipment>().slots)
            .map(|(slot, _)| slot)
            .collect();
        [SLOT_MAINHAND, SLOT_OFFHAND, SLOT_HEAD]
            .iter()
            .all(|slot| seen.contains(slot))
    })
    .await;

    for packet in bob.since::<SetEquipment>(mark) {
        assert_eq!(packet.entity, id);
        let slots: Vec<u8> = packet.slots.iter().map(|(slot, _)| *slot).collect();
        assert!(
            slots.is_sorted(),
            "slots must be in ordinal order: {slots:?}"
        );
    }
}

#[tokio::test]
async fn swapping_hands_updates_the_owner_and_the_observers() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, bob) = paired(&server).await;
    let id = alice.entity_id;
    alice
        .send(&set_creative_mode_slot(36, items::STONE, 64))
        .await;
    alice
        .send(&set_creative_mode_slot(45, items::DIRT, 3))
        .await;
    alice.settle().await;
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

    // 客户端不预测 F，必须收到这两个槽。
    let first = alice.expect::<ContainerSetSlot>(|_| true).await;
    let second = alice.expect::<ContainerSetSlot>(|_| true).await;
    let mut slots = [first, second];
    slots.sort_by_key(|packet| packet.slot);
    assert_eq!(slots[0].slot, 36);
    assert_eq!(slots[0].stack, plain_stack_bytes(items::DIRT, 3));
    assert_eq!(slots[1].slot, 45);
    assert_eq!(slots[1].stack, plain_stack_bytes(items::STONE, 64));
    assert!(slots.iter().all(|packet| packet.container == 0));

    bob.wait_until("both hands to change", |frames| {
        let hands: BTreeMap<u8, Option<Stack>> = frames[mark..]
            .iter()
            .filter(|frame| frame.is::<SetEquipment>())
            .flat_map(|frame| frame.decode::<SetEquipment>().slots)
            .collect();
        hands.get(&SLOT_MAINHAND)
            == Some(&Some(Stack {
                item: items::DIRT,
                count: 3,
            }))
            && hands.get(&SLOT_OFFHAND)
                == Some(&Some(Stack {
                    item: items::STONE,
                    count: 64,
                }))
    })
    .await;
    assert!(
        bob.since::<SetEquipment>(mark)
            .iter()
            .all(|packet| packet.entity == id)
    );
}

// ---- 可见范围 ----

#[tokio::test]
async fn walking_out_of_range_and_back_removes_and_adds_the_entity() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    stand_on_ground(&mut alice, &bob).await;
    bob.skip_received();
    let id = alice.entity_id;

    alice.walk_to(308.5, 8.5, 8.0).await;
    bob.expect::<RemoveEntities>(|packet| packet.ids == [id])
        .await;
    alice.wait_for_chunks((19, 0), 2).await;

    alice.walk_to(8.5, 8.5, 8.0).await;
    let entity = bob.expect::<AddEntity>(|packet| packet.id == id).await;
    assert_eq!(entity.uuid, alice.uuid);
    alice.wait_for_chunks((0, 0), 2).await;
    wait_until_seen_at(&bob, id, (8.5, -60.0, 8.5)).await;

    alice.assert_stream_is_valid();
    bob.assert_stream_is_valid();
}

#[tokio::test]
async fn the_tab_list_keeps_a_player_that_walked_out_of_range() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    let mark = bob.mark();
    bob.skip_received();
    let id = alice.entity_id;

    alice.walk_to(108.5, 8.5, 8.0).await;
    bob.expect::<RemoveEntities>(|packet| packet.ids == [id])
        .await;

    assert!(bob.since::<PlayerInfoRemove>(mark).is_empty());
}

#[tokio::test]
async fn range_depends_on_the_view_distance_of_the_observer() {
    let server = TestServer::start(world_with_view_distance(4)).await;
    let mut alice = server.enter_with_view_distance("Alice", 2).await;
    let mut bob = server.enter_with_view_distance("Bob", 4).await;
    let (alice_id, bob_id) = (alice.entity_id, bob.entity_id);
    alice
        .expect::<AddEntity>(|packet| packet.id == bob_id)
        .await;
    bob.expect::<AddEntity>(|packet| packet.id == alice_id)
        .await;
    let mark = bob.mark();

    // 40 格：Alice 的范围是 2 * 16，Bob 的范围是 4 * 16。
    alice.walk_to(48.5, 8.5, 4.0).await;

    alice
        .expect::<RemoveEntities>(|packet| packet.ids == [bob_id])
        .await;
    bob.expect_none::<RemoveEntities>(mark, |packet| packet.ids.contains(&alice_id))
        .await;
    wait_until_seen_at(&bob, alice_id, alice.position).await;
}

// ---- 离开 ----

#[tokio::test]
async fn leaving_removes_the_entity_before_the_tab_entry() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, bob) = paired(&server).await;
    alice.skip_received();
    let (bob_id, bob_uuid) = (bob.entity_id, bob.uuid);

    drop(bob);

    let left = alice.expect::<SystemChat>(|_| true).await;
    assert_eq!(left.content.translate_keys(), ["multiplayer.player.left"]);
    alice
        .expect::<RemoveEntities>(|packet| packet.ids == [bob_id])
        .await;
    alice
        .expect::<PlayerInfoRemove>(|packet| packet.uuids == [bob_uuid])
        .await;
    server.wait_for_players(&["Alice"]).await;
    alice.assert_stream_is_valid();
}

#[tokio::test]
async fn a_player_out_of_range_leaves_without_remove_entities() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut bob) = paired(&server).await;
    let id = alice.entity_id;
    alice.walk_to(208.5, 8.5, 8.0).await;
    bob.expect::<RemoveEntities>(|packet| packet.ids == [id])
        .await;
    let mark = bob.mark();
    bob.skip_received();
    let uuid = alice.uuid;

    drop(alice);

    bob.expect::<PlayerInfoRemove>(|packet| packet.uuids == [uuid])
        .await;
    assert!(
        bob.since::<RemoveEntities>(mark).is_empty(),
        "the client already forgot this entity"
    );
}

#[tokio::test]
async fn everyone_else_sees_a_leaver_go() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    let mut bob = server.enter("Bob").await;
    let carol = server.enter("Carol").await;
    let (carol_id, carol_uuid) = (carol.entity_id, carol.uuid);
    alice
        .expect::<AddEntity>(|packet| packet.id == carol_id)
        .await;
    bob.expect::<AddEntity>(|packet| packet.id == carol_id)
        .await;

    drop(carol);

    for watcher in [&mut alice, &mut bob] {
        watcher
            .expect::<RemoveEntities>(|packet| packet.ids == [carol_id])
            .await;
        watcher
            .expect::<PlayerInfoRemove>(|packet| packet.uuids == [carol_uuid])
            .await;
    }
}

#[tokio::test]
async fn a_killed_client_can_rejoin_at_once_as_a_new_entity() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, bob) = paired(&server).await;
    alice.skip_received();
    let (old_id, uuid) = (bob.entity_id, bob.uuid);

    drop(bob);
    let bob = server.enter("Bob").await;
    let new_id = bob.entity_id;

    assert_ne!(new_id, old_id, "every login gets a new entity id");
    alice
        .expect::<RemoveEntities>(|packet| packet.ids == [old_id])
        .await;
    alice
        .expect::<PlayerInfoRemove>(|packet| packet.uuids == [uuid])
        .await;
    alice
        .expect::<PlayerInfoUpdate>(|packet| packet.adds(uuid))
        .await;
    let entity = alice
        .expect::<AddEntity>(|packet| packet.id == new_id)
        .await;
    assert_eq!(entity.uuid, uuid);
    server.wait_for_players(&["Alice", "Bob"]).await;
}

#[tokio::test]
async fn a_duplicate_login_replaces_the_entity_for_observers() {
    let server = TestServer::start(small_world()).await;
    let (mut alice, mut old_bob) = paired(&server).await;
    alice.skip_received();
    old_bob.skip_received();
    let (old_id, uuid) = (old_bob.entity_id, old_bob.uuid);

    let new_bob = server.enter("Bob").await;

    let kicked = old_bob.expect::<Disconnect>(|_| true).await;
    assert_eq!(
        kicked.reason.translate_keys(),
        ["multiplayer.disconnect.duplicate_login"]
    );
    alice
        .expect::<RemoveEntities>(|packet| packet.ids == [old_id])
        .await;
    alice
        .expect::<PlayerInfoRemove>(|packet| packet.uuids == [uuid])
        .await;
    alice
        .expect::<PlayerInfoUpdate>(|packet| packet.adds(uuid))
        .await;
    let new_id = new_bob.entity_id;
    alice
        .expect::<AddEntity>(|packet| packet.id == new_id)
        .await;
}

// ---- 不回应的客户端 ----

fn world_with_fast_keep_alive(view_distance: u8) -> ServerConfig {
    ServerConfig {
        keep_alive_interval: Duration::from_millis(300),
        ..world_with_view_distance(view_distance)
    }
}

#[tokio::test]
async fn a_client_that_stops_reading_is_dropped_and_the_other_keeps_playing() {
    let server = TestServer::start(world_with_fast_keep_alive(8)).await;
    let mut reader = server.enter("Reader").await;
    reader.skip_received();

    let _stuck = server.join_without_reading("Stuck").await;
    let stuck_uuid = offline_uuid("Stuck");
    reader
        .expect::<PlayerInfoUpdate>(|packet| packet.adds(stuck_uuid))
        .await;

    // 它回不了心跳，离开的顺序和正常离开相同。
    reader
        .expect::<SystemChat>(|packet| {
            packet.content.translate_keys() == ["multiplayer.player.left"]
        })
        .await;
    reader
        .expect::<PlayerInfoRemove>(|packet| packet.uuids == [stuck_uuid])
        .await;
    server.wait_for_players(&["Reader"]).await;

    reader.assert_world_ticks().await;
    reader.assert_stream_is_valid();
}

#[tokio::test]
async fn a_client_that_stops_reading_loses_its_entity_for_observers() {
    let server = TestServer::start(world_with_fast_keep_alive(8)).await;
    let mut reader = server.enter("Reader").await;
    reader.skip_received();

    let _stuck = server.join_without_reading("Stuck").await;
    let uuid = offline_uuid("Stuck");

    let entity = reader
        .expect::<AddEntity>(|packet| packet.uuid == uuid)
        .await;
    let id = entity.id;
    reader
        .expect::<RemoveEntities>(|packet| packet.ids == [id])
        .await;
    reader
        .expect::<PlayerInfoRemove>(|packet| packet.uuids == [uuid])
        .await;
}

#[tokio::test]
async fn a_silent_client_is_timed_out_and_the_other_is_not() {
    let server = TestServer::start(world_with_fast_keep_alive(2)).await;
    let mut alice = server.enter("Alice").await;
    let mut bob = server.enter("Bob").await;
    alice.skip_received();
    bob.skip_received();
    let uuid = alice.uuid;

    alice.stop_answering_keep_alives();

    let reason = alice.expect::<Disconnect>(|_| true).await;
    assert_eq!(reason.reason.translate_keys(), ["disconnect.timeout"]);
    alice.wait_closed().await;
    bob.expect::<PlayerInfoRemove>(|packet| packet.uuids == [uuid])
        .await;
    assert!(!bob.is_closed());
    bob.assert_world_ticks().await;
}

#[tokio::test]
async fn a_flood_of_packets_does_not_stop_the_other_player() {
    let server = TestServer::start(small_world()).await;
    let flooder = server.enter("Flooder").await;
    let mut bob = server.enter("Bob").await;

    // 比入站配额多得多。超了就断开这名玩家，配额够用时世界线程也要处理完。
    let one = punch();
    let flood = one.repeat(200_000);
    flooder.send_ignoring_errors(&flood).await;

    bob.assert_world_ticks().await;
    assert!(!bob.is_closed());
}

#[tokio::test]
async fn packets_the_server_does_not_decode_are_dropped_quietly() {
    let server = TestServer::start(small_world()).await;
    let mut alice = server.enter("Alice").await;
    let mark = alice.mark();

    // `coverage.md` 1.3：真实客户端会发、服务端可以丢弃的包，内容是什么都不算错。
    // `container_close` 不在里面：它现在会被解码，坏包和官方一样让连接断开。
    for id in [
        serverbound::CLIENT_TICK_END,
        serverbound::PLAYER_LOADED,
        serverbound::SEEN_ADVANCEMENTS,
        serverbound::ATTACK,
        serverbound::INTERACT,
    ] {
        alice
            .send(&common::frame(&[id as u8, 0xff, 0xff, 0xff]))
            .await;
    }

    alice.assert_world_ticks().await;
    assert!(alice.since::<Disconnect>(mark).is_empty());
}
