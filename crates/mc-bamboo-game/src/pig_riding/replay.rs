//! 对拍用的猪骑乘重放：上猪、胡萝卜钓竿加速和逐刻朝向、下车落点。只在 `oracle-samples` 下编译。
//!
//! Oracle replays for pig riding: mounting, carrot-on-a-stick boosting with per-tick
//! rotation, and the dismount spot. Compiled under `oracle-samples` only.

use mc_bamboo_blocks::{
    durability::{damage_of, set_damage_value},
    entity::{
        Entity, JavaRandom,
        mob::{MobType, interact, pig},
    },
};
use mc_bamboo_core::{BlockPos, Hand, Vec3};
use mc_bamboo_data::{blocks, items};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{ClientInformation, EquipmentSlot, GameMode},
};
use mc_bamboo_world::store::ChunkStore;
use uuid::Uuid;

use crate::{
    inbound::PlayerId,
    interaction::on_use_item,
    player::Player,
    region::Region,
    riding::{controls_vehicle, mount},
};

fn new_player(position: Vec3) -> Player {
    Player::new(
        PlayerId(1),
        Uuid::from_u128(1),
        "Rider".to_owned(),
        GameMode::Survival,
        ClientInformation::default(),
        position,
    )
}

/// 一次右键上猪的结果。
///
/// The outcome of one right-click on a pig.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayPigMount {
    /// 交互结果的挥手类型：`PREDICTED` / `SERVER_ONLY` / `NONE` / `PASS`。
    pub swing: String,
    pub count: i32,
    pub vehicle: bool,
    pub controller: bool,
    pub saddle: i32,
    pub seed: i64,
}

/// 玩家对一头猪右键：`setup` 0 上鞍成年、1 没上鞍成年、2 上鞍幼年。
///
/// A player right-clicks a pig: `setup` 0 saddled adult, 1 unsaddled adult, 2 saddled baby.
pub fn replay_pig_mount(setup: i32, item: i32, sneak: bool, seed: i64) -> ReplayPigMount {
    use mc_bamboo_blocks::InteractionResult;
    let mut region = Region::new(2, 20, false);
    let id = PlayerId(1);
    let held = if item == items::AIR {
        ItemStack::empty()
    } else {
        ItemStack::new(item, 1)
    };
    let mut player = new_player(Vec3::new(1.5, 64.0, 0.5));
    player.sync.shift_key_down = sneak;
    let slot = player.inventory.hand_slot(Hand::Main);
    player.inventory.slots[slot] = held.clone();
    region.insert_player(player);
    let mut entity = Entity::by_mob_type(MobType::Pig, Vec3::new(0.5, 64.0, 0.5));
    entity.with_mob(|_, mob| {
        if setup != 1 {
            mob.equipment
                .set(EquipmentSlot::Saddle, ItemStack::new(items::SADDLE, 1));
        }
        if setup == 2 {
            mob.age = -24000;
        }
    });
    let pig_id = region.entities.spawn(entity);
    // 生成时实体表会重新给种子，对拍要的是场景给的种子。
    // Spawning reseeds the entity; the oracle wants the scene's seed.
    // 已经跑过一刻：`onEquipItem` 只在 `!firstTick` 时出声并取随机数。
    // Already ticked once: `onEquipItem` only plays and draws when `!firstTick`.
    region.entities.with_mut(pig_id, |entity| {
        entity.random = JavaRandom::new(seed);
        entity.first_tick = false;
        entity.tick_count = 1;
    });
    let mut uuid = [0u8; 16];
    uuid[..8].copy_from_slice(&1_u64.to_be_bytes());
    uuid[8..].copy_from_slice(&2_u64.to_be_bytes());
    let request = interact::MobInteractRequest {
        player_uid: 1,
        player_uuid: uuid,
        hand: Hand::Main,
        held: held.clone(),
        creative: false,
        position: Vec3::new(1.5, 64.0, 0.5),
        player_height: 1.8,
        sneaking: sneak,
        difficulty: 2,
        hit_pos: None,
    };
    let outcome = region
        .entities
        .with_mut(pig_id, |entity| {
            entity.with_mob(|me, mob| interact::interact(me, mob, &request, 0, false, &mut || 0.0))
        })
        .flatten()
        .expect("pig interaction");
    if outcome.ride {
        mount(&mut region, id, pig_id);
    }
    let swing = match outcome.result {
        InteractionResult::Success => "PREDICTED",
        InteractionResult::SuccessServer => "SERVER_ONLY",
        InteractionResult::Consume => "NONE",
        _ => "PASS",
    };
    let vehicle = region.players[&id].riding.vehicle == Some(pig_id);
    let controller = vehicle && controls_vehicle(&mut region, id, pig_id);
    let (saddle, seed) = region
        .entities
        .with_mut(pig_id, |entity| {
            let seed = entity.random.state();
            let saddle = entity
                .with_mob(|_, mob| {
                    let saddle = mob.equipment.get(EquipmentSlot::Saddle);
                    if saddle.is_empty() { 0 } else { saddle.item }
                })
                .unwrap_or(0);
            (saddle, seed)
        })
        .expect("pig row");
    ReplayPigMount {
        swing: swing.to_owned(),
        count: if held.is_empty() || outcome.shrink_held {
            0
        } else {
            1
        },
        vehicle,
        controller,
        saddle,
        seed,
    }
}

/// 一次胡萝卜钓竿右键后的手持物品、加速时长、使用统计和猪的随机源。
///
/// State after one carrot-on-a-stick right-click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayPigUse {
    pub boosted: bool,
    pub item: i32,
    pub damage: i32,
    pub count: i32,
    pub boost_time: i32,
    pub item_used: i32,
    pub seed: i64,
}

/// 加速结束前后每刻的速度和朝向原始位：`[速度, yRot, xRot, 身体, 头, yRotO]`。
///
/// Per-tick speed and rotation raw bits: `[speed, yRot, xRot, body, head, yRotO]`.
pub type ReplayPigTick = [i32; 6];

/// 骑着猪、手持带 `damage` 点磨损的胡萝卜钓竿：连续右键最多两次（钓竿还在才点第二次），
/// 再按固定的玩家朝向调 `total + 3` 次 `tickRidden`。
///
/// Riding a pig with a carrot on a stick worn by `damage`: up to two right-clicks (the
/// second only while it is still a carrot on a stick), then `total + 3` `tickRidden`
/// calls with fixed player rotations.
pub fn replay_pig_boost(damage: i32, seed: i64) -> (Vec<ReplayPigUse>, Vec<ReplayPigTick>) {
    let mut region = Region::new(2, 20, false);
    let id = PlayerId(1);
    let mut player = new_player(Vec3::new(0.5, 64.0, 0.5));
    let mut held = ItemStack::new(items::CARROT_ON_A_STICK, 1);
    set_damage_value(&mut held, damage);
    let slot = player.inventory.hand_slot(Hand::Main);
    player.inventory.slots[slot] = held;
    region.insert_player(player);
    let mut entity = Entity::by_mob_type(MobType::Pig, Vec3::new(0.5, 64.0, 0.5));
    entity.with_mob(|_, mob| {
        mob.equipment
            .set(EquipmentSlot::Saddle, ItemStack::new(items::SADDLE, 1));
    });
    let pig_id = region.entities.spawn(entity);
    // 生成时实体表会重新给种子，对拍要的是场景给的种子。
    // Spawning reseeds the entity; the oracle wants the scene's seed.
    // 已经跑过一刻：`onEquipItem` 只在 `!firstTick` 时出声并取随机数。
    // Already ticked once: `onEquipItem` only plays and draws when `!firstTick`.
    region.entities.with_mut(pig_id, |entity| {
        entity.random = JavaRandom::new(seed);
        entity.first_tick = false;
        entity.tick_count = 1;
    });
    mount(&mut region, id, pig_id);
    let mut uses = Vec::new();
    for _ in 0..2 {
        if region.players[&id].inventory.held(Hand::Main).item != items::CARROT_ON_A_STICK {
            break;
        }
        let before = pig_state(&mut region, pig_id);
        on_use_item(&mut region, id, Hand::Main, 1, (0.0, 0.0));
        let (boost_time, seed) = pig_state(&mut region, pig_id);
        let player = region.players.get_mut(&id).expect("player");
        let item_used = player
            .item_use_stats
            .take_dirty()
            .into_iter()
            .filter(|(item, _)| *item == items::CARROT_ON_A_STICK)
            .map(|(_, count)| count)
            .sum();
        let held = player.inventory.held(Hand::Main);
        uses.push(ReplayPigUse {
            boosted: seed != before.1,
            item: held.item,
            damage: damage_of(held).unwrap_or(-1),
            count: held.count,
            boost_time,
            item_used,
            seed,
        });
    }
    let total = pig_state(&mut region, pig_id).0;
    let mut ticks = Vec::new();
    for t in 0..(total + 3).max(0) {
        let yaw = -170.0_f32 + 7.0 * t as f32;
        let pitch = ((t % 90) - 45) as f32;
        let row = region
            .entities
            .with_mut(pig_id, |entity| {
                entity.with_mob(|me, mob| {
                    pig::tick_ridden(me, mob, yaw, pitch);
                    let speed = pig::ridden_speed(0.25, &mob.pig_steering);
                    [
                        speed.to_bits() as i32,
                        me.y_rot.to_bits() as i32,
                        mob.x_rot.to_bits() as i32,
                        mob.y_body_rot.to_bits() as i32,
                        mob.y_head_rot.to_bits() as i32,
                        mob.y_rot_o.to_bits() as i32,
                    ]
                })
            })
            .flatten()
            .expect("pig row");
        ticks.push(row);
    }
    (uses, ticks)
}

fn pig_state(region: &mut Region, pig_id: i32) -> (i32, i64) {
    region
        .entities
        .with_mut(pig_id, |entity| {
            let seed = entity.random.state();
            let total = entity
                .with_mob(|_, mob| mob.pig_steering.boost_time_total)
                .unwrap_or(0);
            (total, seed)
        })
        .expect("pig row")
}

/// 摆在猪周围的方块：0 空地、1 一圈三格高石墙、2 只有 -x 一面墙、3 一圈坑、4 一圈低天花板。
///
/// Blocks around the pig: 0 open, 1 a three-high ring wall, 2 a wall on -x only, 3 a
/// ring of pits, 4 a ring of low ceiling.
fn layout(chunks: &mut ChunkStore, layout: i32) {
    for x in -1..=1 {
        for z in -1..=1 {
            if x == 0 && z == 0 {
                continue;
            }
            match layout {
                1 => {
                    for y in -60..=-58 {
                        chunks.set_block(BlockPos::new(x, y, z), blocks::STONE);
                    }
                }
                2 if x == -1 => {
                    for y in -60..=-58 {
                        chunks.set_block(BlockPos::new(x, y, z), blocks::STONE);
                    }
                }
                3 => {
                    chunks.set_block(BlockPos::new(x, -61, z), blocks::AIR);
                }
                4 => {
                    chunks.set_block(BlockPos::new(x, -59, z), blocks::STONE);
                }
                _ => {}
            }
        }
    }
}

/// 猪站在超平坦地面上，朝向 `yaw`，玩家从它背上下来的落点原始位。
///
/// Raw bits of where a player lands when dismounting a pig facing `yaw` on flat ground.
pub fn replay_pig_dismount(layout_id: i32, yaw: f32) -> Option<[i64; 3]> {
    let mut region = Region::new(2, 20, false);
    region.use_infinite_flat_chunks();
    layout(&mut region.chunks, layout_id);
    let id = PlayerId(1);
    let position = Vec3::new(0.5, -60.0, 0.5);
    region.insert_player(new_player(position));
    let mut entity = Entity::by_mob_type(MobType::Pig, position);
    entity.y_rot = yaw;
    let pig_id = region.entities.spawn(entity);
    region
        .entities
        .with_mut(pig_id, |entity| entity.y_rot = yaw);
    mount(&mut region, id, pig_id);
    crate::riding::dismount(&mut region, id);
    let spot = region.players.get(&id)?.position();
    Some([
        spot.x.to_bits() as i64,
        spot.y.to_bits() as i64,
        spot.z.to_bits() as i64,
    ])
}

/// 骑着上鞍的猪时 Bamboo 写进玩家档的 `RootVehicle`，给 `player-data` 让官方读回。
///
/// The `RootVehicle` Bamboo writes for a player riding a saddled pig, for `player-data`
/// to have the official side read back.
pub fn replay_pig_root_vehicle() -> Option<mc_bamboo_nbt::Tag> {
    let mut region = Region::new(2, 20, false);
    let id = PlayerId(1);
    region.insert_player(new_player(Vec3::new(2.5, 64.0, -8.75)));
    let mut entity = Entity::by_mob_type(MobType::Pig, Vec3::new(2.5, 64.0, -8.75));
    entity.with_mob(|_, mob| {
        mob.equipment
            .set(EquipmentSlot::Saddle, ItemStack::new(items::SADDLE, 1));
        mob.health = 4.0;
    });
    let pig_id = region.entities.spawn(entity);
    mount(&mut region, id, pig_id);
    crate::riding::root_vehicle_save(&region, id)
}
