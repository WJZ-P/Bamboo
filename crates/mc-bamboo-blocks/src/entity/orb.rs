//! 经验球，官方 `ExperienceOrb`。
//!
//! 构造时的位置偏移、重力 / 阻力、朝玩家加速、岩浆里的随机位移、每隔 20 刻合并
//! 同值球、6000 刻消失都按官方字节码的顺序。玩家碰到球之后的拾取要等玩家经验
//! 系统，入口留在 [`pick_up`]，现在什么也不做。
//!
//! The experience orb, vanilla `ExperienceOrb`. Constructor offset, gravity and
//! drag, player attraction, lava movement, 20-tick merging and the 6000-tick
//! lifetime follow the official bytecode. Pickup waits for the player
//! experience system; [`pick_up`] is the entry and currently does nothing.

use mc_bamboo_core::{Axis, BlockPos, Vec3};
use mc_bamboo_data::tags::fluid_tags;

use super::{
    Entity, EntityKind, EntityUid,
    ctx::{BlockView, Command, EntityCtx},
    item::friction,
};
/// `LIFETIME`。
const LIFETIME: i32 = 6000;
/// `getAirDrag()` 的 `0.98F`。
const AIR_DRAG: f32 = 0.98;
/// `MAX_FOLLOW_DIST` 是 8；官方还用 64 判断已跟随的玩家有没有走远。
const FOLLOW_RANGE: f64 = 8.0;
const FOLLOW_RANGE_SQR: f64 = 64.0;
/// 岩浆里那两个 `0.2F` 差值的字面量，`0.2F` 转 double 之后的值。
const LAVA_Y: f64 = 0.200_000_002_980_232_24;

/// `ExperienceOrb.getExperienceValue`。
pub fn experience_value(amount: i32) -> i32 {
    [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3]
        .into_iter()
        .find(|&value| amount >= value)
        .unwrap_or(1)
}

/// 构造函数的取数和位置偏移，见 `ExperienceOrb(Level, Vec3, Vec3, int)`。
///
/// `motion` 是朝哪个方向推开新生球的向量；官方的 `award` 传 `Vec3.ZERO`，
/// `awardWithDirection` 才传非零值。
///
/// The constructor's draws and positional offset. Vanilla's `award` passes
/// `Vec3.ZERO`; only `awardWithDirection` passes a non-zero `motion`.
pub fn finish_spawn(me: &mut Entity, motion: Vec3) {
    let EntityKind::ExperienceOrb(_) = &me.kind else {
        return;
    };
    me.y_rot = me.random.next_float() * 360.0;
    let mut velocity = Vec3::new(
        (me.random.next_double() * 0.2 - 0.1) * 2.0,
        me.random.next_double() * 0.2 * 2.0,
        (me.random.next_double() * 0.2 - 0.1) * 2.0,
    );
    let dot = motion.x * velocity.x + motion.y * velocity.y + motion.z * velocity.z;
    if motion.length_sqr() > 0.0 && dot < 0.0 {
        velocity = velocity.scale(-1.0);
    }
    let box_ = me.bounding_box();
    let size = (box_.size(Axis::X) + box_.size(Axis::Y) + box_.size(Axis::Z)) / 3.0;
    me.position = me.position.add(motion.normalize().scale(size * 0.5));
    me.old_position = me.position;
    me.delta = velocity;
}

/// `Entity.spawnAtLocation` 之外的另一条生成路径：`ExperienceOrb.award`。
///
/// 每次拆出的球先取一次 `level.random.nextInt(40)` 找可合并的旧球；没有旧球时，
/// 新球的构造函数取实体自己的四个随机数。
///
/// `ExperienceOrb.award` splits the amount into orbs. Each split draws one
/// `level.random.nextInt(40)`; an orb that cannot merge draws four numbers from
/// its own random source in [`finish_spawn`].
pub fn award(world: &mut crate::world::World, position: Vec3, amount: i32) {
    let mut left = amount;
    while left > 0 {
        let value = experience_value(left);
        left -= value;
        // `tryMergeToExisting` 的 `level.random.nextInt(40)`。生成的球都进实体表；
        // 同值旧球在 `scan_for_merges` 里合并。
        // Vanilla's `tryMergeToExisting` draws `level.random.nextInt(40)`. Every
        // orb enters the entity table; same-valued ones merge in `scan_for_merges`.
        let _ = world.random().next_int(40);
        let entity = Entity::experience_orb(position, value);
        world.spawn_entity(entity);
    }
}

/// `ExperienceOrb.tick()`。
pub(crate) fn tick_orb(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let EntityKind::ExperienceOrb(_) = &me.kind else {
        return;
    };
    // 官方先调 `Entity.tick()`；经验球不调 `baseTick`，所以只把 `firstTick` 清掉。
    me.first_tick = false;
    // 玩家和合并候选都从上下文抄出来，后面借用只读方块视图时不再碰 `ctx`。
    // Copy players and merge candidates out of the context before the block view is borrowed.
    let mut players = Vec::new();
    ctx.blocks()
        .for_each_player(&mut |player| players.push(*player));
    let merge = if me.tick_count % 20 == 1 {
        let area = me.bounding_box().inflate(0.5, 0.5, 0.5);
        let mut others = Vec::new();
        ctx.for_each_other_uid_in(area, &mut |uid| {
            if (uid - me.id).rem_euclid(40) == 0 {
                others.push(uid);
            }
        });
        others
    } else {
        Vec::new()
    };
    let view = ctx.blocks();
    let collides = !view
        .block_collisions(me.bounding_box(), me.collision_context())
        .is_empty();
    if eye_in_water(me, view) {
        set_underwater_movement(me);
    } else if !collides {
        me.apply_gravity();
    }
    if view
        .fluid_state(me.block_position())
        .is_tagged(fluid_tags::LAVA)
    {
        // 两个 `nextFloat` 之差乘 `0.2F`，y 是固定的 `0.2F` 转 double。
        let x = f64::from((me.random.next_float() - me.random.next_float()) * 0.2_f32);
        let z = f64::from((me.random.next_float() - me.random.next_float()) * 0.2_f32);
        me.delta = Vec3::new(x, LAVA_Y, z);
    }
    follow_nearby_player(me, &players);
    let before_y = me.delta.y;
    let movement = me.delta;
    me.move_entity(view, movement);
    me.apply_effects_from_blocks(view);
    let mut drag = AIR_DRAG;
    if me.on_ground {
        // `getBlockPosBelowThatAffectsMyMovement` 用 0.999999F 的偏移。
        let below = view.block_state(me.on_pos(view, 0.999999));
        drag *= friction(below);
    }
    me.delta = me.delta.scale(f64::from(drag));
    // 官方的条件是 `beforeY >= -getGravity()` 时不弹；这里只处理弹的那一支。
    if me.vertical_collision_below && before_y < -0.04 {
        me.delta = Vec3::new(me.delta.x, -before_y * 0.4, me.delta.z);
    }
    let EntityKind::ExperienceOrb(orb) = &mut me.kind else {
        return;
    };
    orb.age += 1;
    let value = orb.value;
    if orb.age >= LIFETIME {
        me.discard();
    }
    if !merge.is_empty() {
        ctx.emit(
            Command::MergeExperienceOrbs {
                others: merge,
                value,
            },
            me,
        );
    }
}

/// `isEyeInFluid(WATER)`：眼睛那一点的流体。
fn eye_in_water(me: &Entity, view: &dyn BlockView) -> bool {
    let eye = me.position.y + f64::from(me.dimensions.1) * 0.85;
    let pos = BlockPos::containing(Vec3::new(me.position.x, eye, me.position.z));
    let fluid = view.fluid_state(pos);
    fluid.is_tagged(fluid_tags::WATER) && !fluid.is_empty()
}

/// `setUnderwaterMovement()`。
fn set_underwater_movement(me: &mut Entity) {
    let y = me.delta.y;
    let lift = if (y as f32) < 0.06 {
        f64::from(5.0e-4_f32)
    } else {
        0.0
    };
    me.delta = Vec3::new(me.delta.x * 0.99, y + lift, me.delta.z * 0.99);
}

/// `followNearbyPlayer()`：8 格内找最近的非旁观者玩家，按距离加权加速过去。
fn follow_nearby_player(me: &mut Entity, players: &[super::ctx::PlayerView]) {
    let EntityKind::ExperienceOrb(orb) = &me.kind else {
        return;
    };
    let mut following = orb.following;
    if let Some(uid) = following {
        let still = players.iter().any(|player| {
            player.uid == uid
                && !player.spectator
                && player.position.distance_to_sqr(me.position) <= FOLLOW_RANGE_SQR
        });
        if !still {
            following = None;
        }
    }
    if following.is_none() {
        let mut best = f64::MAX;
        for player in players {
            if player.spectator || !player.alive {
                continue;
            }
            let distance = player.position.distance_to_sqr(me.position);
            if distance < best && distance <= FOLLOW_RANGE_SQR {
                best = distance;
                following = Some(player.uid);
            }
        }
    }
    if let Some(uid) = following {
        for player in players {
            if player.uid != uid {
                continue;
            }
            let direction = Vec3::new(
                player.position.x - me.position.x,
                player.position.y + f64::from(player.eye_height) / 2.0 - me.position.y,
                player.position.z - me.position.z,
            );
            let strength = 1.0 - direction.length_sqr().sqrt() / FOLLOW_RANGE;
            me.delta = me
                .delta
                .add(direction.normalize().scale(strength * strength * 0.1));
            break;
        }
    }
    if let EntityKind::ExperienceOrb(orb) = &mut me.kind {
        orb.following = following;
    }
}

/// 落地阶段的合并：把同值候选的 `count` 加进来，对方消失。
pub(crate) fn apply_merge(
    me: &mut Entity,
    others: &[EntityUid],
    value: i32,
    world: &mut crate::world::World,
) {
    let EntityKind::ExperienceOrb(orb) = &mut me.kind else {
        return;
    };
    if orb.value != value {
        return;
    }
    for &uid in others {
        if uid == me.id {
            continue;
        }
        let Some(other) = world.entity(uid) else {
            continue;
        };
        let EntityKind::ExperienceOrb(other_orb) = &other.kind else {
            continue;
        };
        if other.removed || other_orb.value != value {
            continue;
        }
        orb.count += other_orb.count;
        orb.age = orb.age.min(other_orb.age);
        world.remove_entity(uid);
    }
}

/// 玩家碰到经验球的入口：官方 `ExperienceOrb.playerTouch`。
/// 玩家经验系统（等级、进度条、`set_experience` 包）还没有实现，所以现在不消失、不发经验。
///
/// The `playerTouch` entry. Bamboo has no player experience system yet, so the
/// orb stays until its 6000-tick lifetime expires.
pub fn pick_up(_me: &mut Entity, _player: EntityUid) -> bool {
    false
}
