//! 跟踪范围、更新间隔、配对包，以及空间索引改动的逐位一致性。

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use mc_bamboo_blocks::{
    entity::{
        EntityPublic, EntityRow, EntityState, EntityStore, FallingBlockState, ItemState,
        KindPublic, KindState,
    },
    state::BlockState,
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{blocks, registries::entity_type};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{ClientInformation, GameMode},
};
use uuid::Uuid;

use crate::{
    inbound::{PlayerId, PlayerSink},
    outgoing::Outgoing,
};

use super::*;

/// 蓝色危险头同步 `WitherSkull.DATA_DANGEROUS`，普通头不发元数据。
///
/// A dangerous (blue) skull syncs `WitherSkull.DATA_DANGEROUS`; a normal skull sends none.
#[test]
fn dangerous_wither_skull_syncs_metadata() {
    use mc_bamboo_blocks::entity::wither_skull::WitherSkullPublic;
    let skull = |dangerous| WitherSkullPublic {
        uuid: [0; 16],
        owner: None,
        dangerous,
        pitch: 0.0,
        needs_sync: false,
        sync_velocity: false,
    };
    assert_eq!(
        KindData::WitherSkull(skull(true)).pairing_values(),
        vec![EntityData::WitherSkullDangerous(true)]
    );
    assert!(
        KindData::WitherSkull(skull(false))
            .pairing_values()
            .is_empty()
    );
}

/// 矿车同步 `VehicleEntity` 的 8/9/10（与船共用）和自己的 11/12/13；
/// 全是默认值时一条都不发。
///
/// A minecart syncs the shared `VehicleEntity` fields 8/9/10 (same as boats) and
/// its own 11/12/13; nothing is sent while every value is the default.
#[test]
fn minecart_syncs_vehicle_and_own_metadata() {
    use mc_bamboo_blocks::entity::{Entity, EntityKind, minecart::MinecartKind};

    let mut entity = Entity::minecart(MinecartKind::Furnace, Vec3::new(0.5, 64.0, 0.5));
    if let EntityKind::Minecart(cart) = &mut entity.kind {
        cart.hurt_time = 7;
        cart.hurt_dir = -1;
        cart.damage = 12.0;
        cart.custom_display_block = Some(BlockState(blocks::STONE));
        cart.display_offset = 9;
        cart.fuel = 100;
    }
    let data = Snapshot::of(&entity.split().public).data.pairing_values();
    assert!(data.contains(&EntityData::BoatHurtTime(7)));
    assert!(data.contains(&EntityData::BoatHurtDir(-1)));
    assert!(data.contains(&EntityData::BoatDamage(12.0)));
    assert!(data.contains(&EntityData::MinecartDisplayBlock(Some(
        blocks::STONE as i32
    ))));
    assert!(data.contains(&EntityData::MinecartDisplayOffset(9)));
    assert!(data.contains(&EntityData::MinecartFuel(true)));

    // 默认矿车：只有共享项，矿车自己的四项都不发。
    let idle = Entity::minecart(MinecartKind::Rideable, Vec3::new(0.5, 64.0, 0.5));
    let data = Snapshot::of(&idle.split().public).data.pairing_values();
    assert!(
        data.iter().all(|item| !matches!(
            item,
            EntityData::BoatHurtTime(_)
                | EntityData::BoatHurtDir(_)
                | EntityData::BoatDamage(_)
                | EntityData::MinecartDisplayBlock(_)
                | EntityData::MinecartDisplayOffset(_)
                | EntityData::MinecartFuel(_)
        )),
        "a default minecart sends no vehicle metadata",
    );

    // 变回默认时发的默认值，运输矿车的展示偏移默认是 8。
    let chest_public = mc_bamboo_blocks::entity::minecart::MinecartPublic {
        entity_type: entity_type::CHEST_MINECART,
        passengers: Vec::new(),
        hurt_time: 0,
        hurt_dir: 1,
        damage: 0.0,
        custom_display_block: None,
        display_offset: 8,
        has_fuel: false,
    };
    assert_eq!(
        minecart_default_data(12, &chest_public),
        EntityData::MinecartDisplayOffset(8)
    );
    assert_eq!(
        minecart_default_data(11, &chest_public),
        EntityData::MinecartDisplayBlock(None)
    );
}

/// 带物品参数的粒子包按官方 `ItemStackTemplate.STREAM_CODEC` 追加
/// 物品 ID、数量 1、空组件补丁（两个 0 varint）。
#[test]
fn item_particles_carry_the_item_stack_template() {
    use mc_bamboo_blocks::entity::mob::MobEvent;
    use mc_bamboo_data::{items, registries::particle_type};
    let make = |item| {
        vec![(
            1,
            vec![MobEvent::Particles {
                particle: particle_type::ITEM,
                item,
                position: Vec3::new(1.0, 2.0, 3.0),
                offset: Vec3::new(0.2, 0.1, 0.2),
                speed: 0.1,
                count: 20,
            }],
        )]
    };
    let with_item = event_packets(&make(Some(items::ARMADILLO_SCUTE)), || 0);
    let without_item = event_packets(&make(None), || 0);
    assert_eq!(with_item.len(), 1);
    assert_eq!(without_item.len(), 1);
    // ITEM=57, ARMADILLO_SCUTE=1003 (varint EB 07), count=1, empty patch 0,0.
    let options = [0x39u8, 0xEB, 0x07, 0x01, 0x00, 0x00];
    assert!(
        with_item[0]
            .windows(options.len())
            .any(|window| window == options),
        "item particle options missing from {:?}",
        with_item[0]
    );
    // 物品参数正好多 5 字节：物品 ID 两字节 + 数量 1 + 两个 0。
    assert_eq!(with_item[0].len(), without_item[0].len() + 5);
}

#[test]
fn tracking_ranges_come_from_the_entity_type() {
    assert_eq!(entity_tracking_range(entity_type::FALLING_BLOCK), 160);
    assert_eq!(entity_tracking_range(entity_type::TNT), 160);
}

#[test]
fn update_intervals_come_from_the_entity_type() {
    assert_eq!(update_interval(entity_type::FALLING_BLOCK), Some(20));
    assert_eq!(update_interval(entity_type::TNT), Some(10));
}

#[test]
fn entity_uuids_are_unique() {
    assert_ne!(entity_uuid(1), entity_uuid(2));
    assert_eq!(entity_uuid(1), entity_uuid(1));
}

/// 改动之前的 `sync`：整表克隆成快照，每个实体扫全部玩家。只给测试当参照，
/// 生产路径用的是空间索引版；两份实现共用发包函数，所以差别只在候选集合。
///
/// The old `sync`, kept as a test oracle: clone the whole table and scan every
/// player for every entity. Both implementations share the packet builders, so
/// the only difference is the candidate set.
fn sync_full_scan(region: &mut Region) {
    prepare_sync(region);
    let snapshots: Vec<Snapshot> = region
        .entities
        .iter()
        .map(|entity| Snapshot::of(&entity.split().public))
        .collect();
    let view_distance = region.view_distance;
    let index = PassengerIndex::of(&snapshots);
    for snapshot in snapshots {
        let tracked = &mut region.entity_tracking.tracked;
        tracked
            .entry(snapshot.id)
            .or_insert_with(|| TrackedEntity::new(&snapshot));
        let tracked = tracked.get_mut(&snapshot.id).expect("刚放进去");
        update_pairings_full_scan(
            &region.players,
            &mut region.links,
            &snapshot,
            tracked,
            view_distance,
            &index,
        );
        send_changes(&snapshot, tracked, &mut region.links);
    }
}

fn update_pairings_full_scan(
    players: &BTreeMap<PlayerId, Player>,
    links: &mut Links,
    snapshot: &Snapshot,
    tracked: &mut TrackedEntity,
    view_distance: i32,
    index: &PassengerIndex,
) {
    let mut visible: Vec<PlayerId> = Vec::new();
    for player in players.values() {
        if is_visible(player, snapshot, view_distance) {
            visible.push(player.id);
        }
    }
    let lost: Vec<PlayerId> = tracked
        .seen_by
        .iter()
        .copied()
        .filter(|id| !visible.contains(id))
        .collect();
    let gained: Vec<PlayerId> = visible
        .into_iter()
        .filter(|id| !tracked.seen_by.contains(id))
        .collect();

    if !lost.is_empty() {
        let frames = encode_frames(&RemoveEntities {
            entity_ids: &[snapshot.id],
        });
        for viewer in lost {
            tracked.seen_by.remove(&viewer);
            links.send_frames(viewer, &frames);
        }
    }
    if !gained.is_empty() {
        let frames = pairing_frames(snapshot, tracked, index);
        for viewer in gained {
            tracked.seen_by.insert(viewer);
            links.send_frames(viewer, &frames);
        }
    }
}

#[derive(Default)]
struct RecordingSink(Arc<Mutex<Vec<u8>>>);

impl PlayerSink for RecordingSink {
    fn send(&mut self, frames: Outgoing) -> bool {
        self.0
            .lock()
            .expect("假 sink 不会被别的线程同时写")
            .extend(frames.to_raw_bytes());
        true
    }

    fn close(&mut self) {}
}

fn player(id: i32, x: f64, z: f64, view_distance: i8) -> Player {
    let client = ClientInformation {
        view_distance,
        ..ClientInformation::default()
    };
    Player::new(
        PlayerId(id),
        Uuid::from_u128(id as u128),
        format!("P{id}"),
        GameMode::Creative,
        client,
        Vec3::new(x, -60.0, z),
    )
}

fn entity(kind: u32, x: f64, z: f64) -> EntityRow {
    let pos = Vec3::new(x, -60.0, z);
    let (kind, state) = match kind % 3 {
        0 => (
            KindPublic::Item {
                stack: ItemStack::new(1, 1 + (kind % 5) as i32),
                pickup_delay: 0,
                age: 0,
                health: 5,
                target: None,
            },
            KindState::Item(ItemState {
                bob_offs: 0.0,
                roll_velocity: false,
            }),
        ),
        1 => (
            KindPublic::FallingBlock {
                state: BlockState(blocks::SAND),
                start_pos: BlockPos::new(x.floor() as i32, -60, z.floor() as i32),
            },
            KindState::FallingBlock(FallingBlockState {
                time: 0,
                drop_item: true,
                cancel_drop: false,
                hurt_entities: false,
                fall_damage_per_distance: 0.0,
                fall_damage_max: 40,
            }),
        ),
        _ => (
            KindPublic::Tnt {
                fuse: (kind % 90) as i32,
            },
            KindState::Tnt,
        ),
    };
    EntityRow {
        public: EntityPublic {
            uid: 0,
            kind,
            position: pos,
            delta: Vec3::default(),
            on_ground: false,
            yaw: 0.0,
            removed: false,
            precise: false,
            vehicle: None,
        },
        state: EntityState {
            kind: state,
            ..Default::default()
        },
    }
}

/// 每个玩家两个 Region 各自的出站字节缓冲。
type SinkPair = (Arc<Mutex<Vec<u8>>>, Arc<Mutex<Vec<u8>>>);
/// 逐刻的结果：每个玩家收到的字节，以及每个实体的观察者名单。
type TickResult = (Vec<(PlayerId, Vec<u8>)>, Vec<(i32, Vec<PlayerId>)>);

/// 两个 Region 的同一组玩家、区块追踪状态、实体表、追踪状态。
/// 每名玩家两边各有一个同 ID 的假 sink，逐刻比字节。
///
/// One pair of regions with identical players, chunk trackers, entities and
/// tracking state; each player has one fake sink per region so bytes can be
/// compared tick by tick.
struct Pair {
    old: Region,
    new: Region,
    sinks: BTreeMap<PlayerId, SinkPair>,
}

impl Pair {
    fn new(view_distance: i32, players: &[(i32, f64, f64, i8)]) -> Self {
        let mut old = Region::new(view_distance, 20, false);
        let mut new = Region::new(view_distance, 20, false);
        // 两张实体表各从同一个号开始分配：生成顺序一致时 uid 逐个相同。
        old.entities = EntityStore::with_next_id(mc_bamboo_blocks::entity::FIRST_ENTITY_ID);
        new.entities = EntityStore::with_next_id(mc_bamboo_blocks::entity::FIRST_ENTITY_ID);
        let mut pair = Self {
            old,
            new,
            sinks: BTreeMap::new(),
        };
        for &(id, x, z, distance) in players {
            let a = Arc::new(Mutex::new(Vec::new()));
            let b = Arc::new(Mutex::new(Vec::new()));
            pair.old
                .links
                .add(PlayerId(id), Box::new(RecordingSink(a.clone())));
            pair.new
                .links
                .add(PlayerId(id), Box::new(RecordingSink(b.clone())));
            for region in [&mut pair.old, &mut pair.new] {
                region.insert_player(player(id, x, z, distance));
            }
            pair.sinks.insert(PlayerId(id), (a, b));
        }
        pair
    }

    /// 给两个 Region 各生成一份相同的公开数据；返回 uid。
    fn spawn(&mut self, kind: u32, x: f64, z: f64) -> i32 {
        let source = entity(kind, x, z);
        let uid = self.old.entities.spawn_row(source.clone());
        let new_uid = self.new.entities.spawn_row(source);
        assert_eq!(uid, new_uid, "两边必须拿到同一个 uid");
        uid
    }

    /// 直接改公开数据：拿整行出来，改 public 半边再放回去。
    fn mutate(&mut self, uid: i32, f: impl Fn(&mut EntityPublic)) {
        for store in [&mut self.old.entities, &mut self.new.entities] {
            let mut row = store
                .row_at(store.row_of(uid).expect("实体还在表里"))
                .expect("实体还在表里")
                .clone();
            f(&mut row.public);
            let mut entity = row.clone().into_entity();
            entity.id = uid;
            store
                .with_mut(uid, |existing| *existing = entity)
                .expect("实体还在表里");
            let _ = row;
        }
    }

    fn remove(&mut self, uid: i32) {
        self.old.entities.remove(uid);
        self.new.entities.remove(uid);
    }

    fn move_player(&mut self, id: PlayerId, x: f64, z: f64) {
        let pos = Vec3::new(x, -60.0, z);
        for region in [&mut self.old, &mut self.new] {
            region
                .players
                .get_mut(&id)
                .expect("玩家已进服")
                .set_position(&mut region.player_grid, pos);
        }
    }

    fn set_client_view(&mut self, id: PlayerId, view_distance: i8) {
        for region in [&mut self.old, &mut self.new] {
            region
                .players
                .get_mut(&id)
                .expect("玩家已进服")
                .client
                .view_distance = view_distance;
        }
    }

    /// 用正式的 `update_views` / `send_chunks` 把玩家附近的区块发完。
    /// 视图包和区块包是双方一致但和追踪无关的背景流量，比完丢掉。
    fn settle_chunks(&mut self) {
        for region in [&mut self.old, &mut self.new] {
            for _ in 0..200 {
                crate::chunk_tracking::update_views(region);
                crate::chunk_tracking::send_chunks(region);
                region.links.flush();
                // 正式路径靠客户端确认推进配额；测试直接把「在途批次」清零。
                for player in region.players.values_mut() {
                    player.chunks.sender.fill_quota_for_test();
                }
                if region
                    .players
                    .values()
                    .all(|player| player.chunks.sender.pending_is_empty_for_test())
                {
                    break;
                }
            }
        }
        self.clear_sinks();
    }

    fn clear_sinks(&self) {
        for (a, b) in self.sinks.values() {
            a.lock().expect("假 sink 的锁").clear();
            b.lock().expect("假 sink 的锁").clear();
        }
    }

    /// 两边都推进一格：逐刻比对每个玩家收到的全部字节和每个实体的观察者集合。
    fn step(&mut self) -> TickResult {
        sync_full_scan(&mut self.old);
        sync(&mut self.new);
        self.old.links.flush();
        self.new.links.flush();
        let mut bytes = Vec::new();
        for (&id, (a, b)) in &self.sinks {
            let first = std::mem::take(&mut *a.lock().expect("假 sink 的锁"));
            let second = std::mem::take(&mut *b.lock().expect("假 sink 的锁"));
            assert_eq!(&first, &second, "玩家 {id:?} 这一刻收到的字节必须逐位相同");
            bytes.push((id, first));
        }
        let old_seen: Vec<_> = self
            .old
            .entity_tracking
            .tracked
            .iter()
            .map(|(&id, tracked)| (id, tracked.seen_by.iter().copied().collect::<Vec<_>>()))
            .collect();
        let new_seen: Vec<_> = self
            .new
            .entity_tracking
            .tracked
            .iter()
            .map(|(&id, tracked)| (id, tracked.seen_by.iter().copied().collect::<Vec<_>>()))
            .collect();
        assert_eq!(old_seen, new_seen, "配对集合必须逐刻相同");
        (bytes, old_seen)
    }

    /// 双方各自推进一格，返回「(实体, 玩家, 开始/停止)」的变化序列。
    fn changes(&mut self) -> (Vec<PairingChange>, Vec<PairingChange>) {
        let before_old: BTreeMap<i32, BTreeSet<PlayerId>> = self
            .old
            .entity_tracking
            .tracked
            .iter()
            .map(|(&id, tracked)| (id, tracked.seen_by.clone()))
            .collect();
        let before_new: BTreeMap<i32, BTreeSet<PlayerId>> = self
            .new
            .entity_tracking
            .tracked
            .iter()
            .map(|(&id, tracked)| (id, tracked.seen_by.clone()))
            .collect();
        self.step();
        (
            pairing_changes(&before_old, &self.old.entity_tracking.tracked),
            pairing_changes(&before_new, &self.new.entity_tracking.tracked),
        )
    }
}

/// 一对「开始 / 停止追踪」的变化，按 (实体 uid, 玩家 ID) 排好序。
type PairingChange = (i32, PlayerId, bool);

fn pairing_changes(
    before: &BTreeMap<i32, BTreeSet<PlayerId>>,
    after: &BTreeMap<i32, TrackedEntity>,
) -> Vec<PairingChange> {
    let mut out = Vec::new();
    for (&id, tracked) in after {
        let was = before.get(&id).cloned().unwrap_or_default();
        for &player in &tracked.seen_by {
            if !was.contains(&player) {
                out.push((id, player, true));
            }
        }
        for player in was.iter().copied() {
            if !tracked.seen_by.contains(&player) {
                out.push((id, player, false));
            }
        }
    }
    for (id, was) in before {
        if !after.contains_key(id) {
            for player in was.iter().copied() {
                out.push((*id, player, false));
            }
        }
    }
    out.sort_unstable();
    out
}

/// 线性同余的伪随机序列，固定种子，不依赖外部 crate。
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 随机场景：每刻随机生成 / 移除实体、挪动实体、让玩家进出追踪范围、
/// 改客户端视距、改变已发到客户端的区块。逐刻比对两个实现的发包字节和配对集合。
///
/// Random scenario: spawn/remove/move entities, move players in and out of range,
/// change client view distances and tracked chunks. Compare each tick's bytes and
/// pairing sets against the old full scan.
#[test]
fn spatial_pairing_matches_the_full_scan_tick_by_tick() {
    for seed in [1_u64, 7, 42, 1337] {
        let mut rng = Rng(seed);
        let mut pair = Pair::new(
            8,
            &[
                (1, 0.0, 0.0, 8),
                (2, 0.5, 0.5, 8),
                (3, 1.0, 1.0, 8),
                (4, 1.5, 1.5, 8),
                (5, 2.0, 2.0, 8),
                (6, 2.5, 2.5, 8),
                (7, 95.0, 0.0, 8),
                (8, -100.0, -100.0, 2),
            ],
        );
        pair.settle_chunks();
        let mut live: Vec<i32> = Vec::new();
        let mut observed = 0;
        for _tick in 0..40 {
            // 每次都放几个挤在原点的实体，保证同一刻里多名玩家同时对多个实体
            // 开始追踪；这样「候选名单顺序」会体现在发包顺序里。
            for offset in 0..3 {
                live.push(pair.spawn(
                    rng.below(30) as u32,
                    8.5 + f64::from(offset) * 0.3,
                    8.5 + f64::from(offset) * 0.3,
                ));
            }
            for _ in 0..rng.below(4) {
                let kind = rng.below(30) as u32;
                let x = f64::from((rng.below(400) as i32) - 200) + 0.5;
                let z = f64::from((rng.below(400) as i32) - 200) + 0.5;
                live.push(pair.spawn(kind, x, z));
            }
            for _ in 0..rng.below(3) {
                if !live.is_empty() {
                    let index = rng.below(live.len() as u64) as usize;
                    let uid = live.swap_remove(index);
                    pair.remove(uid);
                }
            }
            if let Some(&uid) = live.first() {
                pair.mutate(uid, |public| {
                    public.delta = Vec3::new(0.01, -0.02, 0.03);
                    public.on_ground = !public.on_ground;
                    public.position.x += 0.125;
                });
            }
            let x = f64::from((rng.below(500) as i32) - 250);
            let z = f64::from((rng.below(500) as i32) - 250);
            let id = PlayerId(1 + rng.below(8) as i32);
            pair.move_player(id, x, z);
            pair.set_client_view(id, (2 + rng.below(7)) as i8);
            // 每 5 刻把几名近处的玩家同时送回原点附近，制造多个玩家同一刻
            // 一起开始 / 停止追踪：候选顺序反了，包在每位玩家缓冲区里的相对
            // 顺序不会变，但多个实体之间的发送顺序会变，对拍能抓到。
            if _tick % 5 == 0 {
                for n in 2..=6 {
                    pair.move_player(PlayerId(n), f64::from(n) * 0.5, f64::from(n) * 0.5);
                }
            }
            pair.settle_chunks();
            let (old_changes, new_changes) = pair.changes();
            assert_eq!(
                new_changes, old_changes,
                "种子 {seed} 第 {_tick} 刻的配对变化序列必须逐项相同"
            );
            if !old_changes.is_empty() {
                observed += 1;
            }
        }
        assert!(
            observed > 5,
            "随机场景要真的产生配对变化（种子 {seed}，共 {observed} 刻）"
        );
    }
}

/// 改坏确认之一：空间索引漏掉「已经追踪着、但走出了查询半径」的玩家时，
/// 停止追踪包会丢。把候选里的 `seen_by` 并集去掉，这个测试必须失败。
///
/// Sabotage check: dropping the already-tracked players from the candidate set
/// loses stop-tracking packets when a player teleports away.
#[test]
fn tracked_players_still_get_stop_tracking_after_a_long_teleport() {
    let mut pair = Pair::new(8, &[(1, 0.0, 0.0, 8)]);
    pair.settle_chunks();
    let uid = pair.spawn(0, 8.5, 8.5);
    let (_bytes, seen) = pair.step();
    assert_eq!(seen, vec![(uid, vec![PlayerId(1)])], "第一刻要配对成功");

    // 传送到 1024 格外：不在任何网格候选里，但必须收到停止追踪包。
    pair.move_player(PlayerId(1), 1024.0, 1024.0);
    let (bytes, seen) = pair.step();
    assert_eq!(seen, vec![(uid, vec![])], "传送之后配对必须清掉");
    assert_eq!(bytes.len(), 1);
    let remove = encode_frames(&RemoveEntities { entity_ids: &[uid] });
    assert!(
        bytes[0]
            .1
            .windows(remove.len())
            .any(|window| window == remove.as_slice()),
        "传送走的那名玩家必须收到 remove_entities，实际字节 {:?}",
        bytes[0].1
    );
}

/// 改坏确认之二：查询半径不能按「这个实体类型的跟踪范围」取。
/// 视距变小、玩家走出小半径但仍在实体类型的跟踪范围内时必须停止追踪。
/// 用 TNT（160 格）和视距 8（128 格）的交叉区间钉住这一点。
#[test]
fn candidate_radius_covers_the_largest_visible_range() {
    let mut pair = Pair::new(8, &[(1, 0.0, 0.0, 8)]);
    pair.settle_chunks();
    let uid = pair.spawn(2, 0.5, 0.5); // TNT
    pair.step();
    // 玩家在 100 格外：仍落在 128 格的查询半径内，但客户端视距改成 2（32 格）后不可见。
    pair.move_player(PlayerId(1), 100.0, 0.0);
    pair.set_client_view(PlayerId(1), 2);
    pair.settle_chunks();
    let (bytes, seen) = pair.step();
    assert_eq!(seen, vec![(uid, vec![])], "视距变小后必须停止追踪");
    assert!(!bytes[0].1.is_empty(), "停止追踪包不能丢");
}

/// 同一刻里六名玩家同时开始追踪六个实体：逐刻对拍会比较每名玩家收到的字节
/// 序列，实体外循环反序时会在这里失败。
///
/// Six players start tracking six entities in one tick: the per-tick byte
/// comparison catches a reversed entity loop.
#[test]
fn simultaneous_pairings_keep_the_player_and_entity_order() {
    let players: Vec<(i32, f64, f64, i8)> = (1..=6)
        .map(|id| (id, f64::from(id) * 0.5, f64::from(id) * 0.5, 8))
        .collect();
    let mut pair = Pair::new(8, &players);
    pair.settle_chunks();
    const FIRST: i32 = mc_bamboo_blocks::entity::FIRST_ENTITY_ID;
    let mut uids = Vec::new();
    for offset in 0..6 {
        uids.push(pair.spawn(offset, 8.5 + f64::from(offset) * 0.3, 8.5));
    }
    assert_eq!(uids, (0..6).map(|n| FIRST + n).collect::<Vec<_>>());
    let (bytes, seen) = pair.step();
    assert!(
        seen.iter().all(|(_, viewers)| viewers.len() == 6),
        "六名玩家都要看到六个实体"
    );
    // 每名玩家收到的六个 add_entity 里的实体 ID 必须按 uid 升序；
    // 玩家之间按 PlayerId 升序，因为 `links` 是每名玩家自己的缓冲，
    // 这里的顺序由实体外循环决定，不能因为候选反序而错位。
    for (id, frames) in bytes {
        assert!(!frames.is_empty(), "玩家 {id:?} 应收到六个配对 bundle");
    }
}

/// 格子登记要覆盖相邻格：玩家在 x=200（第 1 格）、TNT 在 x=100.5
/// （第 0 格），距离 100 < 视距 128，仍应配对成功。这是改坏确认之四：
/// 只登记玩家自己那一格时这个测试和随机对拍都会失败。
///
/// Player registration must cover neighbouring cells: a player at x=200 (cell 1)
/// still pairs with a TNT at x=100.5 (cell 0), 100 < 128 blocks away. Registering
/// only the player's own cell fails this test and the random one.
#[test]
fn cell_registration_covers_the_full_view_distance() {
    // 视图中心放在原点，让 T(vd=8) 覆盖 x=100.5 的区块（第 6 格）。
    let mut pair = Pair::new(8, &[(1, 0.5, 0.5, 8)]);
    pair.settle_chunks();
    pair.move_player(PlayerId(1), 200.0, 0.0);
    let uid = pair.spawn(2, 100.5, 0.5); // TNT
    let (_bytes, seen) = pair.step();
    assert_eq!(
        seen,
        vec![(uid, vec![PlayerId(1)])],
        "相邻格里的 TNT 也要配对成功"
    );
}

/// 空间索引的快照和原来整行克隆的快照逐项相同（uid 顺序、跨段、三种实体）。
#[test]
fn public_snapshots_match_the_full_table_in_uid_order() {
    let mut server = crate::world::tests::Harness::new(8);
    for index in 0..80 {
        let x = 8.5 + f64::from(index % 5) * 7.0;
        let z = 8.5 + f64::from(index / 5) * 19.0;
        server
            .region()
            .entities
            .spawn_row(entity(index as u32 * 3 + 1, x, z));
    }
    let region = server.region();
    let old: Vec<Snapshot> = region
        .entities
        .iter()
        .map(|entity| Snapshot::of(&entity.split().public))
        .collect();
    let new: Vec<Snapshot> = region.entities.iter_public().map(Snapshot::of).collect();
    assert_eq!(new.len(), 80);
    assert_eq!(new.len(), old.len());
    for (a, b) in old.iter().zip(&new) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.entity_type, b.entity_type);
        assert_eq!(a.position, b.position);
        assert_eq!(a.delta, b.delta);
        assert_eq!(a.on_ground, b.on_ground);
        assert_eq!(a.precise, b.precise);
        assert_eq!(a.yaw, b.yaw);
        assert_eq!(a.chunk, b.chunk);
        assert_eq!(a.data, b.data);
    }
}

/// 手工跑的微基准：N 组相距 1024 格的物品和玩家，同一份世界里交替量
/// 全表扫描和空间索引两版 `sync`。用法：
/// `cargo test -p mc-bamboo-game --release --lib entity_tracking::tests::stress -- --ignored --nocapture`。
///
/// Manual microbenchmark: N far-apart groups of items and players, measuring
/// the full scan and the spatial index in the same world, interleaved.
#[test]
#[ignore = "手工跑的微基准"]
fn stress_reports_sync_cost_against_group_count() {
    use std::time::{Duration, Instant};

    fn setup(groups: usize) -> Pair {
        let mut players = Vec::new();
        for group in 0..groups {
            let ox = group as f64 * 1024.0;
            for bot in 0..10 {
                players.push((1 + (group * 10 + bot) as i32, ox + 8.5 + bot as f64, 8.5, 8));
            }
        }
        let mut pair = Pair::new(8, &players);
        pair.settle_chunks();
        for group in 0..groups {
            let ox = group as f64 * 1024.0;
            for index in 0..256 {
                let x = ox + 40.0 + 2.0 * f64::from(index % 16) + 0.5;
                let z = 40.0 + 2.0 * f64::from(index / 16) + 0.5;
                pair.spawn(0, x, z);
            }
        }
        pair.clear_sinks();
        pair
    }

    fn run(pair: &mut Pair, spatial: bool) -> Duration {
        let start = Instant::now();
        for _ in 0..8 {
            if spatial {
                sync(&mut pair.new);
                pair.new.links.flush();
            } else {
                sync_full_scan(&mut pair.old);
                pair.old.links.flush();
            }
            pair.clear_sinks();
        }
        start.elapsed()
    }

    let groups = [1usize, 2, 4, 8, 16];
    let mut old: Vec<Vec<f64>> = vec![Vec::new(); groups.len()];
    let mut new: Vec<Vec<f64>> = vec![Vec::new(); groups.len()];
    for _ in 0..3 {
        for (i, &g) in groups.iter().enumerate() {
            let mut pair = setup(g);
            run(&mut pair, false);
            run(&mut pair, true);
            // 同轮内 A-B-A 顺序，取两份里的较小值抵消机器负载尖峰。
            let a = run(&mut pair, false).as_secs_f64() * 1e6 / 8.0;
            let b = run(&mut pair, true).as_secs_f64() * 1e6 / 8.0;
            let a2 = run(&mut pair, false).as_secs_f64() * 1e6 / 8.0;
            old[i].push(a.min(a2));
            new[i].push(b);
        }
    }
    for (i, &g) in groups.iter().enumerate() {
        let mut a = old[i].clone();
        let mut b = new[i].clone();
        a.sort_by(f64::total_cmp);
        b.sort_by(f64::total_cmp);
        let items = (g * 256) as f64;
        println!(
            "groups={g:2} items={:5} full_scan={:10.1}us/tick ({:.4}us/item)              spatial={:9.1}us/tick ({:.4}us/item) speedup={:.2}x",
            g * 256,
            a[1],
            a[1] / items,
            b[1],
            b[1] / items,
            a[1] / b[1],
        );
    }
}

/// 原始出站字节里每一帧的包 ID。
fn packet_ids(bytes: &[u8]) -> Vec<i32> {
    use mc_bamboo_protocol::codec::decode_varint;
    let mut rest = bytes;
    let mut ids = Vec::new();
    while !rest.is_empty() {
        let (len, header) = decode_varint(rest).expect("帧长");
        let payload = &rest[header..header + len as usize];
        ids.push(decode_varint(payload).expect("包 ID").0);
        rest = &rest[header + len as usize..];
    }
    ids
}

/// 生物事件按官方的收件人发：伤害和实体事件只给正在跟踪这个实体的玩家
/// （`sendToTrackingPlayersAndSelf`），音效给范围内的玩家（`PlayerList.broadcast`，
/// 音量 1 时 16 格、严格小于）。同一岛上的远处玩家两样都收不到。改坏确认：发给全体
/// （原来的做法）时远处的人会收到，这条测试失败。
///
/// Mob events go to vanilla's recipients: damage and entity events only to the
/// players tracking the entity, sounds to players in range (16 blocks at
/// volume 1, strict). A distant player on the same island gets neither.
#[test]
fn mob_events_reach_trackers_and_sounds_reach_listeners_in_range() {
    use mc_bamboo_blocks::entity::mob::{DamageKind, MobEvent};
    use mc_bamboo_data::packets::play::clientbound::{DAMAGE_EVENT, ENTITY_EVENT, SOUND};

    let mut region = Region::new(2, 20, false);
    let mut sinks = BTreeMap::new();
    // 1：跟踪着；2：没跟踪、离音源 15.9 格；3：没跟踪、离音源 16 格整；4：很远。
    // 1 tracks; 2 does not, 15.9 blocks from the sound; 3 exactly 16; 4 far.
    for (id, x) in [(1, 0.5), (2, 16.4), (3, 16.5), (4, 500.5)] {
        let sink = Arc::new(Mutex::new(Vec::new()));
        region
            .links
            .add(PlayerId(id), Box::new(RecordingSink(sink.clone())));
        let mut player = player(id, x, 0.5, 2);
        player.set_position(&mut region.player_grid, Vec3::new(x, -60.0, 0.5));
        region.insert_player(player);
        sinks.insert(id, sink);
    }
    let uid = region.entities.spawn_row(entity(0, 0.5, 0.5));
    let public = region.entities.public_at(uid).expect("刚生成").clone();
    let mut tracked = TrackedEntity::new(&Snapshot::of(&public));
    tracked.seen_by.insert(PlayerId(1));
    region.entity_tracking.tracked.insert(uid, tracked);
    let events = vec![(
        uid,
        vec![
            MobEvent::Damage {
                kind: DamageKind::Generic,
                attacker: None,
                direct: None,
            },
            MobEvent::Sound {
                sound: "minecraft:entity.cow.ambient",
                volume: 1.0,
                pitch: 1.0,
                position: Vec3::new(0.5, -60.0, 0.5),
                hostile: false,
                seed: None,
            },
            MobEvent::Death,
        ],
    )];
    send_mob_events(&mut region, &events);
    region.links.flush();
    let received = |id: i32| packet_ids(&sinks[&id].lock().expect("没有别人写"));
    assert_eq!(received(1), [DAMAGE_EVENT, SOUND, ENTITY_EVENT]);
    assert_eq!(received(2), [SOUND], "范围内没跟踪的人只听得到声音");
    assert!(received(3).is_empty(), "正好 16 格听不到（严格小于）");
    assert!(received(4).is_empty(), "远处的人什么都收不到");
}

/// 戴头盔的僵尸：配对包最后是 `set_equipment`（只有头盔一个槽），头盔的护甲修饰符是临时的，
/// 也跟着 `update_attributes` 发出去（官方 `AttributeSnapshot` 带全部修饰符）。
#[test]
fn a_zombie_pairs_with_its_equipment_and_transient_armor() {
    use mc_bamboo_blocks::entity::{Entity, mob::equipment::EquipmentUpdate};
    use mc_bamboo_protocol::packet::play::{EquipmentSlot, SetEquipment};

    let mut zombie = Entity::zombie(Vec3::new(0.5, 64.0, 0.5));
    zombie.id = 7;
    let helmet = ItemStack::new(mc_bamboo_data::items::IRON_HELMET, 1);
    if let Some(mob) = zombie.kind.mob_mut() {
        mob.equipment.set(EquipmentSlot::Head, helmet.clone());
        let update = mob.equipment.detect_updates(&mut mob.attributes);
        assert_eq!(
            update,
            EquipmentUpdate {
                hand_swap: false,
                changes: vec![(EquipmentSlot::Head, helmet.clone())],
            }
        );
    }
    let row = zombie.split();
    let snapshot = Snapshot::of(&row.public);
    let tracked = TrackedEntity::new(&snapshot);
    let packets = tracked.pairing_packets(&snapshot);
    let slots = [(EquipmentSlot::Head, &helmet)];
    let expected = mc_bamboo_protocol::frame::packet_body(&SetEquipment::new(7, &slots).unwrap());
    assert_eq!(packets.last(), Some(&expected));
    let attributes = &packets[packets.len() - 2];
    let needle = b"minecraft:armor.helmet";
    assert!(
        attributes
            .windows(needle.len())
            .any(|window| window == needle),
        "the helmet's armor modifier is synced"
    );
}

/// 配对一个骑手：载具的 bundle 以 `set_passengers(自己)` 结尾，骑手的 bundle 以
/// `set_passengers(载具)` 结尾，名单来自本刻全表索引。
///
/// Pairing a jockey: the vehicle ends with `set_passengers(self)` and the rider
/// with `set_passengers(vehicle)`, using the tick-wide index for the list.
#[test]
fn a_jockey_pairs_with_set_passengers_in_both_directions() {
    use mc_bamboo_blocks::entity::{Entity, mob::MobType};
    use mc_bamboo_protocol::packet::play::SetPassengers;

    let vehicle_id = 1 << 24;
    let rider_id = vehicle_id + 1;
    let mut chicken = Entity::by_mob_type(MobType::Chicken, Vec3::new(0.5, 64.0, 0.5));
    chicken.id = vehicle_id;
    let mut zombie = Entity::by_mob_type(MobType::Zombie, Vec3::new(0.5, 64.0, 0.5));
    zombie.id = rider_id;
    zombie.vehicle = Some(vehicle_id);
    if let Some(mob) = chicken.kind.mob_mut() {
        mob.passengers.push(rider_id);
    }
    let chicken = chicken.split();
    let zombie = zombie.split();
    let snapshots = vec![Snapshot::of(&chicken.public), Snapshot::of(&zombie.public)];
    let index = PassengerIndex::of(&snapshots);
    let expected = mc_bamboo_protocol::frame::packet_body(&SetPassengers {
        vehicle: vehicle_id,
        passengers: &[rider_id],
    });
    for snapshot in &snapshots {
        let tracked = TrackedEntity::new(snapshot);
        let packets = tracked.pairing_packets_in(snapshot, &index);
        assert_eq!(packets.last(), Some(&expected));
    }
    // 没有全表索引时骑手那一侧不知道载具的完整名单，宁可不发也不发错。
    let tracked = TrackedEntity::new(&snapshots[1]);
    let packets = tracked.pairing_packets(&snapshots[1]);
    assert!(packets.last() != Some(&expected));
}

/// 海龟的 18、19 变回默认时要发两条布尔，不能落到牛的花色上。
///
/// A turtle's 18 and 19 must fall back to the two booleans, not to the cow
/// variants.
#[test]
fn turtle_metadata_defaults_are_the_two_booleans() {
    use mc_bamboo_blocks::entity::mob::MobType;
    use mc_bamboo_data::entity_types as facts;
    use mc_bamboo_protocol::packet::play::EntityData;

    assert_eq!(
        default_data_for_type(facts::TURTLE_HAS_EGG, MobType::Turtle),
        EntityData::TurtleHasEgg(false)
    );
    assert_eq!(
        default_data_for_type(facts::TURTLE_LAYING_EGG, MobType::Turtle),
        EntityData::TurtleLayingEgg(false)
    );
}

/// 嗅探兽的两项元数据按官方下标同步：非默认时出现在配对包里，变回默认时
/// `default_data_for_type` 发 0，而不是落到牛的花色/叫声上。
///
/// The sniffer's two metadata items sync at the vanilla indices: they appear in
/// the pairing packet when non-default, and `default_data_for_type` sends 0 when
/// they return to default instead of falling through to the cow variants.
#[test]
fn sniffer_metadata_uses_state_and_drop_seed_indices() {
    use mc_bamboo_blocks::entity::{
        Entity,
        mob::{MobType, sniffer},
    };
    use mc_bamboo_data::entity_types as facts;
    use mc_bamboo_protocol::packet::play::EntityData;

    // 非默认：状态 DIGGING(5)、掉种子时刻 120。
    // Non-default: state DIGGING (5) and drop-seed tick 120.
    let mut entity = Entity::by_mob_type(MobType::Sniffer, Vec3::new(0.5, 64.0, 0.5));
    entity.with_mob(|_, mob| {
        mob.sniffer.state = sniffer::State::Digging;
        mob.sniffer.drop_seed_at_tick = 120;
    });
    let data = Snapshot::of(&entity.split().public).data.pairing_values();
    assert!(data.contains(&EntityData::SnifferState(5)));
    assert!(data.contains(&EntityData::SnifferDropSeedAtTick(120)));
    // 没有落到牛的 18/19。
    // They do not fall through to the cow variants at 18/19.
    assert!(!data.iter().any(|item| matches!(
        item,
        EntityData::CowVariant(_) | EntityData::CowSoundVariant(_)
    )));

    // 默认：只有通用项，两项都不发。
    // Default: only the shared items; neither sniffer item is sent.
    let idle = Entity::by_mob_type(MobType::Sniffer, Vec3::new(0.5, 64.0, 0.5));
    let data = Snapshot::of(&idle.split().public).data.pairing_values();
    assert!(!data.iter().any(|item| matches!(
        item,
        EntityData::SnifferState(_) | EntityData::SnifferDropSeedAtTick(_)
    )));

    // 变回默认时发的两条默认值。
    // The two defaults sent when they return to default.
    assert_eq!(
        default_data_for_type(facts::SNIFFER_STATE, MobType::Sniffer),
        EntityData::SnifferState(0)
    );
    assert_eq!(
        default_data_for_type(facts::SNIFFER_DROP_SEED_AT_TICK, MobType::Sniffer),
        EntityData::SnifferDropSeedAtTick(0)
    );
}

/// `ServerEntity.sendChanges`：玩家和生物共用一份乘客名单时，名单变化只发一次
/// `set_passengers`，内容就是这一份名单；名单不变就不再发。
#[test]
fn passenger_list_changes_send_exactly_one_set_passengers() {
    use mc_bamboo_blocks::entity::{Entity, FIRST_ENTITY_ID, mob::MobType};
    use mc_bamboo_protocol::frame::packet_body;
    use mc_bamboo_protocol::packet::play::SetPassengers;

    use crate::{
        inbound::{PlayerId, PlayerSink},
        links::Links,
        outgoing::Outgoing,
    };

    #[derive(Clone, Default)]
    struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl PlayerSink for Sink {
        fn send(&mut self, frames: Outgoing) -> bool {
            self.0.lock().unwrap().extend(frames.to_raw_bytes());
            true
        }
        fn close(&mut self) {}
        fn second_handle(&self) -> Option<Box<dyn PlayerSink>> {
            Some(Box::new(self.clone()))
        }
    }

    let vehicle = 1 << 24;
    let player = 7;
    let mob = FIRST_ENTITY_ID + 1;
    let mut horse = Entity::by_mob_type(MobType::Horse, Vec3::new(8.5, 64.0, 8.5));
    horse.id = vehicle;
    horse.with_mob(|_, m| {
        m.passengers.push(player);
        m.passengers.push(mob);
    });
    let public = horse.split().public;
    let mut links = Links::default();
    let sink = Sink::default();
    links.add(PlayerId(1), Box::new(sink.clone()));

    let full = packet_body(&SetPassengers {
        vehicle,
        passengers: &[player, mob],
    });
    let snapshot = Snapshot::of(&public);
    let mut tracked = TrackedEntity::new(&snapshot);
    tracked.seen_by.insert(PlayerId(1));
    // 从「空名单」到「玩家 + 生物」：发一次，内容含两份乘客。
    tracked.last_passengers.clear();
    send_changes(&snapshot, &mut tracked, &mut links);
    links.flush();
    let bytes = sink.0.lock().unwrap().clone();
    assert_eq!(
        bytes.windows(full.len()).filter(|w| *w == full).count(),
        1,
        "one set_passengers with the shared list"
    );
    // 名单没变时不再发。
    sink.0.lock().unwrap().clear();
    send_changes(&snapshot, &mut tracked, &mut links);
    links.flush();
    assert!(
        sink.0
            .lock()
            .unwrap()
            .windows(full.len())
            .all(|w| w != full)
    );
}

/// 村民入睡时元数据发出 `Pose::Sleeping` 与 `SleepingPos(Some(pos))`，醒来时回退到
/// `Pose::Standing` 与 `SleepingPos(None)`，客户端能看到村民躺床和起床。
#[test]
fn villager_sleeping_metadata_syncs_pose_and_bed_pos() {
    use mc_bamboo_blocks::entity::{Entity, mob::MobType};
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_protocol::packet::play::{EntityData, Pose};

    let bed = BlockPos::new(4, 64, 2);
    let mut villager = Entity::by_mob_type(MobType::Villager, Vec3::new(4.5, 64.6875, 2.5));
    villager.id = 42;
    villager.with_mob(|_, mob| {
        mob.villager.sleeping = true;
        mob.villager.sleeping_pos = Some(bed);
    });
    let sleeping_public = villager.clone().split().public;
    let sleeping_snap = Snapshot::of(&sleeping_public);
    let sleeping_data = sleeping_snap.data.pairing_values();
    assert!(sleeping_data.contains(&EntityData::Pose(Pose::Sleeping)));
    assert!(sleeping_data.contains(&EntityData::SleepingPos(Some(bed))));

    let mut tracked = TrackedEntity::new(&sleeping_snap);
    // 先消费出生首刻的 `pending_dirty`（含初始血量），再测试起床时的增量元数据。
    let _ = tracked.step(&sleeping_snap, false, false);
    villager.with_mob(|_, mob| {
        mob.villager.sleeping = false;
        mob.villager.sleeping_pos = None;
    });
    let awake_public = villager.split().public;
    let awake_snap = Snapshot::of(&awake_public);
    let packets = tracked.step(&awake_snap, false, false);
    let expected =
        mc_bamboo_protocol::frame::packet_body(&mc_bamboo_protocol::packet::play::SetEntityData {
            entity_id: 42,
            items: &[
                EntityData::Pose(Pose::Standing),
                EntityData::SleepingPos(None),
            ],
        });
    assert!(
        packets.contains(&expected),
        "waking villager should send Pose::Standing + SleepingPos(None)"
    );
    assert_eq!(
        default_data_for_type(14, MobType::Villager),
        EntityData::SleepingPos(None)
    );
}

/// 画和展示框的 `AddEntity` 位置（方块角点）与朝向 `data`、配对元数据以及回到默认值的脏项更新。
#[test]
fn hanging_entities_sync_add_packet_and_metadata_deltas() {
    use mc_bamboo_blocks::entity::{Entity, EntityKind};
    use mc_bamboo_core::{BlockPos, Direction};
    use mc_bamboo_data::{items, painting as painting_data};
    use mc_bamboo_protocol::packet::play::{AddEntity, ENTITY_FLAG_INVISIBLE_BIT, EntityData};

    let pos = BlockPos::new(3, 65, -2);
    let variant = painting_data::id_of("minecraft:kebab").unwrap();
    let mut painting = Entity::painting(pos, Direction::North, variant);
    painting.id = 77;
    let p_public = painting.split().public;
    let p_snap = Snapshot::of(&p_public);
    let p_tracked = TrackedEntity::new(&p_snap);
    let p_pairing = p_tracked.pairing_packets(&p_snap);
    let expected_add = mc_bamboo_protocol::frame::packet_body(&AddEntity {
        entity_id: 77,
        uuid: Uuid::from_bytes(p_snap.data.stable_uuid().expect("painting uuid")),
        entity_type: mc_bamboo_data::registries::entity_type::PAINTING,
        position: pos.corner(),
        movement: Vec3::default(),
        pitch: 0,
        yaw: pack_degrees(180.0),
        head_yaw: 0,
        data: Direction::North.id(),
    });
    assert!(
        p_pairing.contains(&expected_add),
        "painting AddEntity uses block corner and facing id"
    );
    let p_values = p_snap.data.pairing_values();
    assert!(p_values.contains(&EntityData::HangingDirection(Direction::North)));
    assert!(p_values.contains(&EntityData::PaintingVariant(variant)));

    // ItemFrame: non-default direction, item, rotation, invisible -> reset back to empty & visible.
    let mut frame = Entity::item_frame(pos, Direction::Up, true);
    frame.id = 78;
    if let EntityKind::ItemFrame(f) = &mut frame.kind {
        f.item = ItemStack::new(items::GOLD_INGOT, 1);
        f.rotation = 3;
        f.invisible = true;
    }
    let f_public = frame.clone().split().public;
    let f_snap = Snapshot::of(&f_public);
    let f_values = f_snap.data.pairing_values();
    assert!(f_values.contains(&EntityData::SharedFlags(ENTITY_FLAG_INVISIBLE_BIT)));
    assert!(f_values.contains(&EntityData::HangingDirection(Direction::Up)));
    assert!(f_values.contains(&EntityData::ItemFrameItem(ItemStack::new(
        items::GOLD_INGOT,
        1
    ))));
    assert!(f_values.contains(&EntityData::ItemFrameRotation(3)));

    let mut f_tracked = TrackedEntity::new(&f_snap);
    // 先消费构造首刻的 `pending_dirty`（含非默认朝向），再测清空物品与隐身时的回退包。
    let _ = f_tracked.step(&f_snap, false, false);
    if let EntityKind::ItemFrame(f) = &mut frame.kind {
        f.item = ItemStack::empty();
        f.rotation = 0;
        f.invisible = false;
    }
    let f_cleared_public = frame.split().public;
    let f_cleared_snap = Snapshot::of(&f_cleared_public);
    let packets = f_tracked.step(&f_cleared_snap, false, false);
    let expected_meta =
        mc_bamboo_protocol::frame::packet_body(&mc_bamboo_protocol::packet::play::SetEntityData {
            entity_id: 78,
            items: &[
                EntityData::SharedFlags(0),
                EntityData::ItemFrameItem(ItemStack::empty()),
                EntityData::ItemFrameRotation(0),
            ],
        });
    assert!(
        packets.contains(&expected_meta),
        "clearing item frame must emit default metadata values"
    );
}
