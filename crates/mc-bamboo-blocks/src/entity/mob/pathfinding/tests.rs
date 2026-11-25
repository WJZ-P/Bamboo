//! 寻路阶段纯逻辑的单测：去重、优先级、预算顺延、失败冷却、过期作废。

use mc_bamboo_core::{BlockPos, Vec3};

use super::{
    Action, COOLDOWN_TICKS, Config, DedupKey, FAILURE_THRESHOLD, NavRequest, Outcome, Pending,
    PhaseState, serial,
};
use crate::entity::{
    ctx::PlayerView,
    mob::path::{EvaluatorFlags, Path, PathMob, PathRequest},
};

fn request(target: BlockPos) -> PathRequest {
    PathRequest {
        mob: PathMob {
            position: Vec3::new(0.5, 64.0, 0.5),
            on_ground: true,
            in_floatable_fluid: false,
            in_water: false,
            width: 0.6,
            height: 1.8,
            max_up_step: 0.6,
            max_fall_distance: 3,
            malus: Default::default(),
            context: crate::shape::CollisionContext::Empty,
        },
        flags: EvaluatorFlags::default(),
        target,
        max_distance_from_start: 16.0,
        reach_range: 0,
        max_visited_nodes: 16,
        max_visited_nodes_multiplier: 1.0,
    }
}

fn nav_request(local: u32, target: BlockPos) -> NavRequest {
    let request = request(target);
    NavRequest {
        request,
        nav: crate::entity::mob::navigation::NavMob {
            path: request.mob,
            speed: 1.0,
            in_liquid: false,
            follow_range: 16.0,
            game_time: 0,
            owner_uid: 1,
        },
        action: Action::MoveTo { speed: 1.0 },
        at: target,
        local_serial: local,
    }
}

fn pending(uid: i32, local: u32, target: BlockPos) -> Pending {
    Pending::new(uid, nav_request(local, target), 0, false, -1)
}

fn player(x: f64, z: f64) -> PlayerView {
    PlayerView {
        uid: 1,
        uuid: None,
        position: Vec3::new(x, 64.0, z),
        delta: Vec3::default(),
        width: 0.6,
        height: 1.8,
        eye_height: 1.62,
        health: 20.0,
        look_direction: Vec3::default(),
        gaze_disguise: false,
        alive: true,
        spectator: false,
        game_mode: 0,
        mining_fatigue: None,
        creative: false,
        flying: false,
        explosion_knockback_resistance: 0.0,
        sneaking: false,
        sprinting: false,
        swimming: false,
        invisible: false,
        armor_cover: 0.0,
        main_hand: 0,
        off_hand: 0,
        team: None,
        yaw: 0.0,
        pitch: 0.0,
        combat: Default::default(),
        effects: Default::default(),
    }
}

fn empty(_: &PathRequest) -> (Option<Path>, u64) {
    (None, 1)
}

fn some_path(_: &PathRequest) -> (Option<Path>, u64) {
    (
        Some(Path {
            nodes: Vec::new(),
            next_node_index: 0,
            target: BlockPos::new(1, 64, 0),
            dist_to_target: 0.0,
            reached: true,
        }),
        1,
    )
}

#[test]
fn dedup_key_distinguishes_all_four_facts() {
    let base = DedupKey::of(&request(BlockPos::new(5, 64, 0)));
    assert_eq!(base, DedupKey::of(&request(BlockPos::new(5, 64, 0))));
    assert_ne!(base, DedupKey::of(&request(BlockPos::new(6, 64, 0))));
    let mut other = request(BlockPos::new(5, 64, 0));
    other.mob.width = 1.2;
    assert_ne!(base, DedupKey::of(&other));
    other = request(BlockPos::new(5, 64, 0));
    other.flags.fly = true;
    assert_ne!(base, DedupKey::of(&other));
    other = request(BlockPos::new(5, 64, 0));
    other.mob.position.y = 65.0;
    assert_ne!(base, DedupKey::of(&other));
}

#[test]
fn serial_is_unique_per_mob_and_tick() {
    assert_ne!(serial(2, 0), serial(3, 0));
    assert_ne!(serial(2, 0), serial(2, 1));
}

#[test]
fn duplicates_are_computed_once_and_delivered_to_every_waiter() {
    let mut state = PhaseState::default();
    state.run(
        0,
        Config {
            enabled: true,
            budget: 4096,
            fail_cooldown: false,
        },
        &[],
        vec![
            pending(2, 0, BlockPos::new(5, 64, 0)),
            pending(3, 0, BlockPos::new(5, 64, 0)),
        ],
        &some_path,
    );
    assert_eq!(state.stats().searches, 1);
    let outcomes = state.take_deliveries();
    assert_eq!(outcomes.len(), 2);
    assert_eq!(outcomes[0].uid, 2);
    assert_eq!(outcomes[1].uid, 3);
    assert!(outcomes.iter().all(|outcome| outcome.path.is_some()));
    // 结果交给生物时带的是同一条路径和各自的序号。
    let _: Outcome = outcomes[0].clone();
    assert_eq!(outcomes[0].replay().serial, serial(2, 0));
}

#[test]
fn priority_prefers_attack_targets_and_uid() {
    let mut state = PhaseState::default();
    let mut far = pending(9, 0, BlockPos::new(5, 64, 0));
    far.has_attack_target = false;
    let mut close = pending(10, 0, BlockPos::new(6, 64, 0));
    close.has_attack_target = true;
    let players = vec![player(0.5, 0.5)];
    let config = Config {
        enabled: true,
        budget: 16,
        fail_cooldown: false,
    };
    state.run(0, config, &players, vec![far, close], &empty);
    // 有攻击目标的那条先算，另一条顺延。
    assert_eq!(state.stats().searches, 1);
    let outcomes = state.take_deliveries();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].uid, 10);
    assert_eq!(state.deferred_len(), 1);
    // 下一刻顺延的请求因为等得更久先算。
    state.run(2, config, &players, Vec::new(), &empty);
    assert_eq!(state.take_deliveries()[0].uid, 9);
}

#[test]
fn budget_defers_whole_requests_and_waited_ticks_raise_priority() {
    let mut state = PhaseState::default();
    let requests: Vec<_> = (0..4)
        .map(|index| pending(index + 2, 0, BlockPos::new(5 + index, 64, 0)))
        .collect();
    let config = Config {
        enabled: true,
        budget: 32,
        fail_cooldown: false,
    };
    state.run(0, config, &[], requests, &empty);
    assert_eq!(state.stats().searches, 2);
    assert_eq!(state.deferred_len(), 2);
    let first = state.take_deliveries();
    assert_eq!(first.len(), 2);
    // 顺延的两条下一刻等得更久，先算。
    state.run(2, config, &[], Vec::new(), &empty);
    let second = state.take_deliveries();
    assert_eq!(second.len(), 2);
    assert!(second.iter().all(|outcome| outcome.uid >= 4));
}

#[test]
fn failure_cooldown_stops_retrying_after_ten_failures() {
    let mut state = PhaseState::default();
    let config = Config {
        enabled: true,
        budget: 4096,
        fail_cooldown: true,
    };
    for tick in 0..=FAILURE_THRESHOLD as i64 {
        let mut item = pending(7, tick as u32, BlockPos::new(5, 64, 0));
        item.issued_at = tick;
        state.run(tick, config, &[], vec![item], &empty);
        assert_eq!(state.take_deliveries().len(), 1);
    }
    assert_eq!(state.failure_count(7), FAILURE_THRESHOLD + 1);
    assert!(state.cooling(7, FAILURE_THRESHOLD as i64 + 1, config));
    assert!(!state.cooling(7, FAILURE_THRESHOLD as i64 + COOLDOWN_TICKS, config));
    assert!(!state.may_request(7, FAILURE_THRESHOLD as i64 + 1, config));
}

#[test]
fn cancellation_drops_deferred_requests_and_deliveries() {
    let mut state = PhaseState::default();
    state.run(
        0,
        Config {
            enabled: true,
            budget: 16,
            fail_cooldown: false,
        },
        &[],
        vec![
            pending(3, 0, BlockPos::new(5, 64, 0)),
            pending(4, 0, BlockPos::new(6, 64, 0)),
        ],
        &empty,
    );
    assert_eq!(state.delivery_count(), 1);
    assert_eq!(state.deferred_len(), 1);
    state.cancel(3);
    state.cancel(4);
    assert_eq!(state.delivery_count(), 0);
    assert_eq!(state.deferred_len(), 0);
}

#[test]
fn success_clears_the_failure_count() {
    let mut state = PhaseState::default();
    let config = Config {
        enabled: true,
        budget: 4096,
        fail_cooldown: true,
    };
    for tick in 0..5 {
        let mut item = pending(1, tick as u32, BlockPos::new(5, 64, 0));
        item.issued_at = tick;
        state.run(tick, config, &[], vec![item], &empty);
        let _ = state.take_deliveries();
    }
    assert_eq!(state.failure_count(1), 5);
    let mut item = pending(1, 5, BlockPos::new(5, 64, 0));
    item.issued_at = 5;
    state.run(5, config, &[], vec![item], &some_path);
    assert_eq!(state.failure_count(1), 0);
}

#[test]
fn disabled_config_clears_pending_state() {
    let mut state = PhaseState::default();
    state.run(
        0,
        Config::OFF,
        &[],
        vec![pending(1, 0, BlockPos::new(5, 64, 0))],
        &empty,
    );
    assert_eq!(state.deferred_len(), 0);
    assert_eq!(state.delivery_count(), 0);
}

/// 挂起请求按导航 owner 记账，不是按恰好存下它的那一行 uid。控制载具的骑手借
/// 载具导航发请求，语义归属是载具（`NavMob.owner_uid`）；`take_path_requests`
/// 必须据此记账，结果才会挂回载具的导航、下一刻骑手再借到它。
///
/// A pending request is keyed by its navigation owner, not by the row that
/// happens to hold it. A controlling rider borrows the vehicle's navigation,
/// so the request belongs to the vehicle (`NavMob.owner_uid`).
#[test]
fn take_path_requests_keys_by_navigation_owner() {
    let mut level = crate::testing::TestLevel::new();
    let mut ticks = crate::BlockTicks::new();
    let uid = {
        let mut world = crate::World::new(&mut level, &mut ticks);
        world.spawn_entity(crate::entity::Entity::cow(Vec3::new(0.5, 64.0, 0.5)))
    };
    // 造一条 owner 与本行 uid 不同的挂起请求：模拟「请求挂在别人的导航上」。
    let owner = uid + 7;
    let mut pending = nav_request(1, BlockPos::new(2, 64, 3));
    pending.nav.owner_uid = owner;
    crate::World::new(&mut level, &mut ticks).with_entity_mut(uid, &mut |entity| {
        entity.kind.mob_mut().unwrap().navigation.pending_request = Some(pending);
    });

    let requests = level.entities_mut_for_test().take_path_requests(0);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].uid, owner, "必须按导航 owner 记账");
    assert_ne!(requests[0].uid, uid, "不能按存下请求的行 uid 记账");
    assert_eq!(requests[0].nav.owner_uid, owner);
}
