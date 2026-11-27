//! 烟花火箭，官方 `FireworkRocketEntity`（`Projectile` 的子类）。
//!
//! 三种飞行方式：竖直发射（无 `shotAtAngle`，每刻 x/z 乘 1.15、y 加 0.04）、
//! 斜射（`shotAtAngle`，弩 / 发射器，匀速直线）、附着跟随（鞘翅滑翔右键加速，
//! `attached_to_target` 指向滑翔玩家）。附着时每刻跟随玩家手持位置的偏移、按官方
//! 公式给玩家加速度并同步自己速度。寿命 `lifetime = 10 * flight + nextInt(6) + nextInt(7)`，
//! `life` 每刻加一，超过寿命就 `explode`。命中实体时也 `explode`，命中方块时只有
//! 带爆炸成分（有烟花之星）才 `explode`。`explode` 广播实体事件 17、按 5 格范围和
//! 视线用 `fireworks` 伤害类型伤活体（附带着目标），然后消失。

use mc_bamboo_core::{Aabb, Vec3};
use mc_bamboo_data::{items, registries::data_component_type as kinds};
use mc_bamboo_protocol::components::{ComponentValue, defaults};
use mc_bamboo_protocol::item::ItemStack;

use super::{
    Entity, EntityKind, EntityUid, KindPublic,
    arrow::{clip_entity, clip_including_border, player_box, resolve_owner},
    ctx::{BlockView, Command, EntityCtx, PlayerView},
    mob::{DamageKind, DamageSource},
};
use crate::World;

thread_local! {
    /// 对拍的改坏确认开关：0 正常，1 寿命、2 竖直加速、3 爆炸伤害、4 滑翔加速、
    /// 5 附着偏移、6 主手判断。正式构建里 `set_oracle_break` 不会被调用，恒为 0。
    static ORACLE_BREAK: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

/// 由 `mob-oracle` 按 `--break-self-test firework_*` 设置，其他驱动不调用。为真时
/// 复刻改坏后的行为，确认对拍能抓到。
///
/// Set by `mob-oracle` from `--break-self-test firework_*`; no other driver calls
/// this. When set, the broken behaviour is reproduced so the comparison catches it.
pub fn set_oracle_break(name: &str) {
    let value = match name {
        "firework_lifetime" => 1,
        "firework_shot" => 2,
        "firework_damage" => 3,
        "firework_boost" => 4,
        "firework_attached" => 5,
        "firework_hand" => 6,
        _ => 0,
    };
    ORACLE_BREAK.with(|cell| cell.set(value));
}

fn oracle_break() -> u8 {
    ORACLE_BREAK.with(|cell| cell.get())
}

/// `FireworkRocketEntity` 自己的状态，加上 `Projectile` 里的主人和标志。
///
/// The `FireworkRocketEntity` fields plus the owner and flags from `Projectile`.
#[derive(Debug, Clone, PartialEq)]
pub struct FireworkRocket {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    /// `DATA_ID_FIREWORKS_ITEM`：构造时存的一份物品堆拷贝。
    pub item: ItemStack,
    /// `DATA_ATTACHED_TO_TARGET`（`OptionalInt`，实体 id）。鞘翅滑翔加速时指向
    /// 附着的玩家；其余生成路径为 `None`。
    pub attached_to_target: Option<EntityUid>,
    /// `DATA_SHOT_AT_ANGLE`。
    pub shot_at_angle: bool,
    /// `life`：已经飞了多少 tick。
    pub life: i32,
    /// `lifetime`：飞多少 tick 后爆炸。
    pub lifetime: i32,
    /// `Projectile.leftOwner` / `hasBeenShot`。
    pub left_owner: bool,
    pub has_been_shot: bool,
    /// `xRot`。
    pub pitch: f32,
    pub(crate) needs_sync: bool,
    pub(crate) sync_velocity: bool,
    /// 这一刻 `explode` 广播过实体事件 17，等同步层取走。
    pub(crate) exploded: bool,
}

impl FireworkRocket {
    pub fn new(shot: &FireworkShot) -> Self {
        Self {
            uuid: [0; 16],
            owner: shot.owner,
            owner_uuid: shot.owner_uuid,
            item: shot.item.clone(),
            attached_to_target: shot.attached_to_target,
            shot_at_angle: shot.shot_at_angle,
            life: 0,
            lifetime: 0,
            left_owner: false,
            has_been_shot: false,
            pitch: 0.0,
            needs_sync: false,
            sync_velocity: false,
            exploded: false,
        }
    }

    pub(crate) fn public(&self) -> FireworkRocketPublic {
        FireworkRocketPublic {
            uuid: self.uuid,
            owner: self.owner,
            pitch: self.pitch,
            item: self.item.clone(),
            attached_to_target: self.attached_to_target,
            shot_at_angle: self.shot_at_angle,
            needs_sync: self.needs_sync,
            sync_velocity: self.sync_velocity,
        }
    }

    /// 这一刻 `explode` 广播过实体事件 17（对拍读它）。
    ///
    /// Whether `explode` broadcast entity event 17 this tick (read by the oracle).
    pub const fn exploded(&self) -> bool {
        self.exploded
    }
}

/// 同步层要读的那一半。
///
/// The half the sync layer reads.
#[derive(Debug, Clone, PartialEq)]
pub struct FireworkRocketPublic {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub pitch: f32,
    pub item: ItemStack,
    pub attached_to_target: Option<EntityUid>,
    pub shot_at_angle: bool,
    pub needs_sync: bool,
    pub sync_velocity: bool,
}

/// 一次生成烟花的输入：`FireworkRocketItem.useOn`、发射器和弩共用。
///
/// One spawn's inputs, shared by `FireworkRocketItem.useOn`, the dispenser and the
/// crossbow.
#[derive(Debug, Clone, PartialEq)]
pub struct FireworkShot {
    pub position: Vec3,
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    pub item: ItemStack,
    pub shot_at_angle: bool,
    /// `DATA_ATTACHED_TO_TARGET`：鞘翅滑翔右键加速时附着在玩家身上的目标。其余
    /// 生成路径（`useOn`、发射器、弩）恒为 `None`。
    ///
    /// `DATA_ATTACHED_TO_TARGET`: the target a gliding boost rocket is attached to.
    /// The other spawn paths (`useOn`, dispenser, crossbow) always leave this `None`.
    pub attached_to_target: Option<EntityUid>,
    /// 生成后再走一次 `Projectile.shoot`（发射器 / 弩）。
    ///
    /// An optional `Projectile.shoot` after spawning (dispenser / crossbow).
    pub shoot: Option<FireworkShoot>,
}

/// `Projectile.shoot` 的输入。
///
/// The inputs of `Projectile.shoot`.
#[derive(Debug, Clone, PartialEq)]
pub struct FireworkShoot {
    pub direction: Vec3,
    pub power: f32,
    pub uncertainty: f32,
}

impl Entity {
    /// `new FireworkRocketEntity(level, x, y, z, stack)` 加上主人和 `shotAtAngle`。
    pub fn firework_rocket(shot: &FireworkShot) -> Self {
        Self::new(
            EntityKind::FireworkRocket(Box::new(FireworkRocket::new(shot))),
            shot.position,
        )
    }

    /// 构造函数的随机取数：竖直初速（`triangle(0, 0.002297)`、0.05）和
    /// `lifetime = 10 * flight + nextInt(6) + nextInt(7)`。`prepare` 已经设好实体随机源。
    ///
    /// The constructor's draws: the vertical velocity (`triangle(0, 0.002297)`, 0.05) and
    /// `lifetime = 10 * flight + nextInt(6) + nextInt(7)`. `prepare` seeded the entity random.
    pub fn finish_firework_spawn(&mut self) {
        let EntityKind::FireworkRocket(firework) = &mut self.kind else {
            return;
        };
        // `flight` 从 1 起，有 FIREWORKS 组件时再加 `flightDuration()`。
        let flight =
            match defaults::resolve(firework.item.item, &firework.item.patch(), kinds::FIREWORKS) {
                Some(ComponentValue::Fireworks {
                    flight_duration, ..
                }) => 1 + *flight_duration,
                _ => 1,
            };
        let spread = 0.002_297;
        self.delta = Vec3::new(
            self.random.triangle(0.0, spread),
            0.05,
            self.random.triangle(0.0, spread),
        );
        let flight_duration = self.random.next_int_bounded(6);
        let flight_jitter = self.random.next_int_bounded(7);
        firework.lifetime = if oracle_break() == 1 {
            // 改坏确认：丢掉两次 `nextInt` 的随机寿命。
            10 * flight
        } else {
            10 * flight + flight_duration + flight_jitter
        };
    }

    /// `Projectile.shoot`：归一化 → 按 XYZ 抽散布 → 乘力度 → 设朝向。
    pub fn shoot_firework(&mut self, shoot: &FireworkShoot) {
        let spread = 0.017_227_5 * f64::from(shoot.uncertainty);
        let movement = shoot
            .direction
            .normalize()
            .add(Vec3::new(
                self.random.triangle(0.0, spread),
                self.random.triangle(0.0, spread),
                self.random.triangle(0.0, spread),
            ))
            .scale(f64::from(shoot.power));
        self.delta = movement;
        let EntityKind::FireworkRocket(firework) = &mut self.kind else {
            return;
        };
        firework.needs_sync = true;
        firework.sync_velocity = true;
        let (yaw, pitch) = rotation(movement);
        self.y_rot = yaw;
        firework.pitch = pitch;
    }
}

impl World<'_> {
    /// 生成一枚烟花：先分配 ID / 种子（构造函数的 UUID 取数），再取构造随机数，
    /// 可选再按 `shoot` 取散布。
    ///
    /// Spawns one firework: the id and seed first (the constructor's UUID draw), then the
    /// constructor draws and, optionally, the `shoot` spread.
    pub fn spawn_firework(&mut self, shot: &FireworkShot) -> i32 {
        let mut entity = self.prepare_entity(Entity::firework_rocket(shot));
        entity.finish_firework_spawn();
        if let Some(shoot) = &shot.shoot {
            entity.shoot_firework(shoot);
        }
        self.add_prepared_entity(entity)
    }
}

/// `Projectile.shoot` / `updateRotation` 的目标朝向，和鸡蛋共用同一份算法。
pub(super) use super::thrown_egg::rotation;

/// 一次命中。`target` 是命中的实体。
///
/// One hit; `target` is the hit entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FireworkHit {
    pub location: Vec3,
    pub target: Option<EntityUid>,
}

/// `FireworkRocketEntity.tick`，在 `commonTick` 之后。飞行、命中、寿命和爆炸都在这里。
///
/// `FireworkRocketEntity.tick`, after `commonTick`. Flight, hits, lifetime and the
/// explosion all live here.
pub fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let (owner, owner_uuid, attached, shot_at_angle) = match &mut me.kind {
        EntityKind::FireworkRocket(firework) => {
            firework.needs_sync = false;
            firework.sync_velocity = false;
            firework.has_been_shot = true;
            (
                firework.owner,
                firework.owner_uuid,
                firework.attached_to_target,
                firework.shot_at_angle,
            )
        }
        _ => return,
    };
    // `Projectile.tick` 先解析主人、`checkLeftOwner`，再 `Entity.tick`（流体 / 着火 / 掉出世界）。
    let owner = owner.or_else(|| owner_uuid.and_then(|uuid| resolve_owner(ctx, uuid)));
    if let EntityKind::FireworkRocket(firework) = &mut me.kind {
        firework.owner = owner;
    }
    check_left_owner(me, ctx, owner);
    base_tick(me, ctx.blocks());

    let hit = if let Some(attached) = attached {
        // `FireworkRocketEntity.tick` 的 `isAttachedToEntity()` 分支：读附着玩家这一刻的
        // 公开视图，滑翔时按官方公式给玩家加速、按手持位置算偏移，跟随玩家并取新速度，
        // 再做一次碰撞查询。目标已不在本岛时官方只保留原位置并照样查碰撞。
        //
        // The `isAttachedToEntity()` branch of `FireworkRocketEntity.tick`: read the
        // attached player's public view, boost them with the vanilla formula while
        // gliding, follow to the hand-held offset, adopt the new velocity, then run one
        // collision query. When the target has left, vanilla keeps the old position and
        // still queries collisions.
        if let Some(player) = ctx.player(attached) {
            let (new_delta, offset, boosted) = attached_motion(&player);
            if boosted {
                ctx.emit(
                    Command::SetPlayerDelta {
                        target: attached,
                        delta: new_delta,
                    },
                    me,
                );
            }
            me.position = player.position.add(offset);
            me.delta = new_delta;
        }
        find_hit(me, ctx, owner)
    } else {
        if !shot_at_angle && oracle_break() != 2 {
            // 竖直发射：撞墙时 x/z 不再加速，否则每刻乘 1.15；y 每刻加 0.04。
            // 改坏确认 `firework_shot`：跳过竖直加速。
            let d = if me.horizontal_collision { 1.0 } else { 1.15 };
            me.delta = me.delta.multiply(d, 1.0, d).add(Vec3::new(0.0, 0.04, 0.0));
        }
        let movement = me.delta;
        let hit = find_hit(me, ctx, owner);
        // `move(MoverType.SELF, movement)` 碰方块、设碰撞标志，然后恢复速度。
        me.move_entity(ctx, movement);
        me.apply_effects_from_blocks(ctx.blocks());
        me.delta = movement;
        hit
    };

    // 命中任何目标（方块或实体）都置 `needsSync`；命中实体就爆，命中方块只有带爆炸成分才爆。
    // Any hit sets `needsSync`; hitting an entity always explodes, hitting a block only explodes with stars.
    if let Some(hit) = hit {
        if hit.target.is_some() || has_explosion(me) {
            ctx.emit(Command::FireworkRocketExplode { hit: Some(hit) }, me);
        }
        if let EntityKind::FireworkRocket(firework) = &mut me.kind {
            firework.needs_sync = true;
        }
    }

    update_rotation(me);

    // `life == 0` 放发射音，然后 `life++`，超过寿命就爆。
    let life = match &me.kind {
        EntityKind::FireworkRocket(firework) => firework.life,
        _ => return,
    };
    if life == 0 {
        ctx.emit(
            Command::FireworkRocketLaunch {
                position: me.position,
            },
            me,
        );
    }
    let EntityKind::FireworkRocket(firework) = &mut me.kind else {
        return;
    };
    firework.life += 1;
    if firework.life > firework.lifetime {
        ctx.emit(Command::FireworkRocketExplode { hit: None }, me);
    }
}

/// `FireworkRocketEntity.tick` 附着分支这一步：滑翔玩家按官方公式得到的新速度、烟花
/// 相对玩家的偏移，以及要不要把新速度写回玩家（没在滑翔就不写）。
///
/// One attached-branch step of `FireworkRocketEntity.tick`: the boosted velocity for a
/// gliding player, the firework's offset from the player, and whether the velocity is
/// actually written back (it is not once the player stops gliding).
fn attached_motion(player: &PlayerView) -> (Vec3, Vec3, bool) {
    if !player.fall_flying {
        return (player.delta, Vec3::default(), false);
    }
    let look = player.look_direction;
    let delta = player.delta;
    // 官方逐轴：`delta + look*0.1 + (look*1.5 - delta)*0.5`。化简虽然等于
    // `0.5*delta + 0.85*look`，但按原式逐项算，浮点末位才和字节码一致。
    //
    // Vanilla, per axis: `delta + look*0.1 + (look*1.5 - delta)*0.5`. The last-bit
    // rounding only matches the bytecode when computed term by term like this.
    let boost = if oracle_break() == 4 {
        // 改坏确认：丢掉 `0.5` 收敛项。
        Vec3::new(look.x * 0.1, look.y * 0.1, look.z * 0.1)
    } else {
        Vec3::new(
            look.x * 0.1 + (look.x * 1.5 - delta.x) * 0.5,
            look.y * 0.1 + (look.y * 1.5 - delta.y) * 0.5,
            look.z * 0.1 + (look.z * 1.5 - delta.z) * 0.5,
        )
    };
    // `Entity.getHandHoldingItemAngle(FIREWORK_ROCKET)`：
    // `calculateViewVector(0, yRot + (arm == RIGHT ? 80 : -80))` 乘 0.5。
    //
    // `Entity.getHandHoldingItemAngle(FIREWORK_ROCKET)`:
    // `calculateViewVector(0, yRot + (arm == RIGHT ? 80 : -80))` scaled by 0.5.
    let offset = if oracle_break() == 5 {
        // 改坏确认：丢掉手持偏移。
        Vec3::default()
    } else {
        Vec3::view_vector(player.yaw + hand_holding_angle(player), 0.0).scale(0.5)
    };
    (delta.add(boost), offset, true)
}

/// `Entity.getHandHoldingItemAngle(Item)` 加在 `yRot` 上的角度（度）。
///
/// 官方：`offhand = getOffhandItem().is(item) && !getMainHandItem().is(item)`；
/// `arm = offhand ? getMainArm().getOpposite() : getMainArm()`；
/// 返回 `arm == RIGHT ? 80 : -80`。`getOpposite()` 只翻转左右手，所以
/// `arm == RIGHT` 等价于 `main_arm_right != offhand`。
///
/// The angle (degrees) `Entity.getHandHoldingItemAngle` adds to `yRot`. Vanilla:
/// `offhand = getOffhandItem().is(item) && !getMainHandItem().is(item)`;
/// `arm = offhand ? getMainArm().getOpposite() : getMainArm()`;
/// returns `arm == RIGHT ? 80 : -80`. `getOpposite()` only flips handedness, so
/// `arm == RIGHT` is exactly `main_arm_right != offhand`.
fn hand_holding_angle(player: &PlayerView) -> f32 {
    let offhand =
        player.off_hand == items::FIREWORK_ROCKET && player.main_hand != items::FIREWORK_ROCKET;
    // 改坏确认：忽略主手/副手判断，恒当右手。
    if oracle_break() == 6 || player.main_arm_right != offhand {
        80.0
    } else {
        -80.0
    }
}

/// `Projectile.checkLeftOwner`：主人（含玩家）不再碰到「当前碰撞盒沿速度展开再外扩 1」时离开。
fn check_left_owner(me: &mut Entity, ctx: &dyn EntityCtx, owner: Option<EntityUid>) {
    if matches!(&me.kind, EntityKind::FireworkRocket(fw) if fw.left_owner) {
        return;
    }
    let area = me
        .bounding_box()
        .expanded_towards(me.delta.x, me.delta.y, me.delta.z)
        .inflated(1.0);
    let mut in_range = owner
        .and_then(|uid| ctx.other(uid))
        .is_some_and(|entity| entity.bounding_box().intersects(area));
    ctx.blocks().for_each_player(&mut |player| {
        if player.alive && !player.spectator && player_box(player).intersects(area) {
            in_range |= owner == Some(player.uid);
        }
    });
    if let EntityKind::FireworkRocket(firework) = &mut me.kind {
        firework.left_owner = !in_range;
    }
}

/// `Projectile.tick` 的其余部分和 `Entity.baseTick`：流体、着火、掉出世界。
fn base_tick(me: &mut Entity, blocks: &dyn BlockView) {
    let in_lava = me.update_projectile_fluid(blocks);
    let sync_velocity =
        me.remaining_fire_ticks > 0 && me.remaining_fire_ticks % 20 == 0 && !in_lava;
    if me.remaining_fire_ticks > 0 {
        me.remaining_fire_ticks -= 1;
    }
    if me.position.y < f64::from(blocks.min_y() - 64) {
        me.removed = true;
    }
    me.first_tick = false;
    if let EntityKind::FireworkRocket(firework) = &mut me.kind {
        firework.has_been_shot = true;
        firework.sync_velocity = sync_velocity;
    }
}

/// `Projectile.updateRotation`：从当前朝向往速度方向插值五分之一。
fn update_rotation(me: &mut Entity) {
    let (yaw, pitch) = rotation(me.delta);
    let EntityKind::FireworkRocket(firework) = &mut me.kind else {
        return;
    };
    firework.pitch = super::thrown_egg::lerp_rotation(firework.pitch, pitch);
    me.y_rot = super::thrown_egg::lerp_rotation(me.y_rot, yaw);
}

/// `getExplosions()` 非空：带烟花之星。
fn has_explosion(me: &Entity) -> bool {
    let EntityKind::FireworkRocket(firework) = &me.kind else {
        return false;
    };
    matches!(
        defaults::resolve(firework.item.item, &firework.item.patch(), kinds::FIREWORKS),
        Some(ComponentValue::Fireworks { explosions, .. }) if !explosions.is_empty()
    )
}

/// `ProjectileUtil.getHitResultOnMoveVector`：方块裁剪（含世界边界）之后，在缩短的线段上
/// 找最近的实体；有实体就取实体。
fn find_hit(me: &Entity, ctx: &dyn EntityCtx, owner: Option<EntityUid>) -> Option<FireworkHit> {
    let blocks = ctx.blocks();
    let from = me.position;
    let block = clip_including_border(blocks, from, from.add(me.delta), me.collision_context());
    let to = block.map_or_else(|| from.add(me.delta), |hit| hit.location);
    let area = me
        .bounding_box()
        .expanded_towards(me.delta.x, me.delta.y, me.delta.z)
        .inflated(1.0);
    let margin = f64::from(((me.tick_count - 2) as f32 / 20.0).clamp(0.0, 0.3));
    let left_owner = matches!(&me.kind, EntityKind::FireworkRocket(fw) if fw.left_owner);
    let mut nearest = f64::MAX;
    let mut best: Option<(f64, Vec3, EntityUid)> = None;
    let mut consider = |uid: EntityUid, bb: Aabb| {
        // `canHitEntity`：没离开主人之前不打主人自己（没有载具，同载具就是它本身）。
        if !left_owner && Some(uid) == owner {
            return;
        }
        if let Some(location) = clip_entity(bb.inflated(margin), from, to) {
            let distance = from.distance_to_sqr(location);
            if distance < nearest {
                nearest = distance;
                best = Some((distance, location, uid));
            }
        }
    };
    ctx.for_each_other_in(area, &mut |other| {
        if other.removed || matches!(other.kind, KindPublic::FireworkRocket(_)) {
            return;
        }
        if matches!(&other.kind, KindPublic::Mob(mob) if mob.health > 0.0) {
            consider(other.uid, other.bounding_box());
        }
    });
    blocks.for_each_player(&mut |player| {
        let bb = player_box(player);
        if player.alive && !player.spectator && bb.intersects(area) {
            consider(player.uid, bb);
        }
    });
    match best {
        Some((_, location, uid)) => Some(FireworkHit {
            location,
            target: Some(uid),
        }),
        None if block.is_some() => Some(FireworkHit {
            location: block.expect("刚查过方块命中").location,
            target: None,
        }),
        None => None,
    }
}

/// [`Command::FireworkRocketExplode`] 的落地：`explode`。实体事件 17 由
/// `keep_departed_events` 在实体被丢弃后取走。
///
/// The landing of [`Command::FireworkRocketExplode`]: `explode`. Entity event 17 is
/// taken by `keep_departed_events` once the entity is discarded.
pub fn explode(me: &mut Entity, world: &mut World<'_>, hit: Option<FireworkHit>) {
    if me.removed {
        return;
    }
    let EntityKind::FireworkRocket(firework) = &mut me.kind else {
        return;
    };
    // `onHitBlock` 里命中方块还要先跑一遍 `entityInside`（营地火、炼药锅这些）。
    // 对拍场景只有石墙，方块实体为空，这里按无效果跳过。
    let _ = hit;
    firework.exploded = true;
    deal_explosion_damage(me, world);
    me.discard();
}

/// `Entity.distanceTo`：先按 `f32` 做三轴差再开方。
///
/// `Entity.distanceTo`: cast each axis delta to `f32` before taking the square root.
fn distance_to(a: Vec3, b: Vec3) -> f32 {
    let dx = (a.x - b.x) as f32;
    let dy = (a.y - b.y) as f32;
    let dz = (a.z - b.z) as f32;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// `dealExplosionDamage`：`f = 5 + explosions.size() * 2`；带星的烟花先伤附着目标，
/// 再在 5 格内按视线和距离衰减伤活体，伤害类型是 `fireworks`。
///
/// `dealExplosionDamage`: `f = 5 + explosions.size() * 2`. A star firework first hurts
/// its attached target, then hurts living entities within 5 blocks scaled by distance and
/// line of sight, with the `fireworks` damage type.
fn deal_explosion_damage(me: &mut Entity, world: &mut World<'_>) {
    let (f, owner, attached) = match &me.kind {
        EntityKind::FireworkRocket(firework) => {
            let size = match defaults::resolve(
                firework.item.item,
                &firework.item.patch(),
                kinds::FIREWORKS,
            ) {
                Some(ComponentValue::Fireworks { explosions, .. }) => explosions.len(),
                _ => 0,
            };
            (
                if size == 0 {
                    0.0
                } else if oracle_break() == 3 {
                    // 改坏确认：丢掉 `5 +` 的基础伤害。
                    size as f32 * 2.0
                } else {
                    5.0 + size as f32 * 2.0
                },
                firework.owner,
                firework.attached_to_target,
            )
        }
        _ => return,
    };
    if f <= 0.0 {
        return;
    }
    let source = |world: &World<'_>, owner: Option<EntityUid>| {
        let mut damage = DamageSource::environment(DamageKind::Fireworks);
        damage.direct = Some(me.id);
        damage.attacker = owner;
        damage.position = Some(me.position);
        // `FireworkRocketEntity.calculateHorizontalHurtKnockbackDirection` 返回
        // `(victim - rocket)`，`LivingEntity` 取反后为 `(rocket - victim)`，
        // 与 `projectile_motion == None` 时走 `damage.position` 的方向一致。
        // `FireworkRocketEntity.calculateHorizontalHurtKnockbackDirection` returns
        // `(victim - rocket)`, which `LivingEntity` negates to `(rocket - victim)` —
        // the exact radial direction `damage.position` produces when `projectile_motion` is `None`.
        damage.projectile_motion = None;
        damage.attacker_is_player = owner.is_some_and(|uid| {
            let mut found = false;
            world
                .level()
                .for_each_player(&mut |player| found |= player.uid == uid);
            found
        });
        damage.scale_when_non_player = owner
            .and_then(|uid| world.entity(uid))
            .is_some_and(|entity| matches!(entity.kind, EntityKind::Mob(_)));
        damage
    };
    // 附着的滑翔玩家先吃满额伤害（官方 `attachedToEntity.hurtServer`）。
    if let Some(target) = attached {
        let _ = world.hurt_player_from_mob(target, source(world, owner), f, None);
    }
    // 半径 5 内的活体（生物与玩家），跳过附着目标，距离平方超过 25 的跳过，视线被挡的跳过。
    let area = me.bounding_box().inflate(5.0, 5.0, 5.0);
    let mut targets = Vec::new();
    for id in world.entity_ids() {
        let Some(entity) = world.entity(id) else {
            continue;
        };
        let EntityKind::Mob(mob) = &entity.kind else {
            continue;
        };
        if mob.health <= 0.0 || Some(id) == attached || !entity.bounding_box().intersects(area) {
            continue;
        }
        if me.position.distance_to_sqr(entity.position) > 25.0 {
            continue;
        }
        targets.push(id);
    }
    for target in targets {
        let Some(entity) = world.entity(target) else {
            continue;
        };
        let height = f64::from(entity.dimensions.1);
        let mut visible = false;
        for step in 0..2 {
            let point = Vec3::new(
                entity.position.x,
                entity.position.y + height * 0.5 * f64::from(step),
                entity.position.z,
            );
            if world
                .clip_collider(me.position, point, me.collision_context())
                .is_none()
            {
                visible = true;
                break;
            }
        }
        if !visible {
            continue;
        }
        let distance = distance_to(me.position, entity.position);
        let amount = f * ((5.0 - f64::from(distance)) / 5.0).sqrt() as f32;
        let _ = world.hurt_mob(target, source(world, owner), amount);
    }
    // 范围内的玩家同样属于 `LivingEntity`。
    // Players in range are also `LivingEntity` targets.
    let mut player_targets = Vec::new();
    world.level().for_each_player(&mut |player| {
        if !player.alive || player.spectator || Some(player.uid) == attached {
            return;
        }
        let bb = player_box(player);
        if !bb.intersects(area) || me.position.distance_to_sqr(player.position) > 25.0 {
            return;
        }
        player_targets.push((player.uid, player.position, bb.max_y - bb.min_y));
    });
    for (uid, position, height) in player_targets {
        let mut visible = false;
        for step in 0..2 {
            let point = Vec3::new(
                position.x,
                position.y + height * 0.5 * f64::from(step),
                position.z,
            );
            if world
                .clip_collider(me.position, point, me.collision_context())
                .is_none()
            {
                visible = true;
                break;
            }
        }
        if !visible {
            continue;
        }
        let distance = distance_to(me.position, position);
        let amount = f * ((5.0 - f64::from(distance)) / 5.0).sqrt() as f32;
        let _ = world.hurt_player_from_mob(uid, source(world, owner), amount, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::mob::throttle::test_player;

    /// `Entity.getHandHoldingItemAngle(FIREWORK_ROCKET)` 的四组合，照官方
    /// `offhand = 副手是烟花 && 主手不是`、`arm = offhand ? 反手 : 主手`、
    /// `arm == RIGHT ? +80 : -80`。
    #[test]
    fn hand_holding_angle_follows_main_arm_and_offhand() {
        let rocket = items::FIREWORK_ROCKET;
        let air = 0;
        let mut player = test_player(1, Vec3::default());

        // 右手主手、副手空：+80。
        player.main_arm_right = true;
        player.main_hand = air;
        player.off_hand = air;
        assert_eq!(hand_holding_angle(&player), 80.0);
        assert_eq!(
            attached_motion(&{
                let mut p = player;
                p.fall_flying = true;
                p
            })
            .1,
            Vec3::view_vector(80.0, 0.0).scale(0.5)
        );

        // 左手主手、副手空：-80。
        player.main_arm_right = false;
        assert_eq!(hand_holding_angle(&player), -80.0);

        // 右手主手、只有副手拿烟花：反手成左手 → -80。
        player.main_arm_right = true;
        player.main_hand = air;
        player.off_hand = rocket;
        assert_eq!(hand_holding_angle(&player), -80.0);

        // 左手主手、只有副手拿烟花：反手成右手 → +80。
        player.main_arm_right = false;
        assert_eq!(hand_holding_angle(&player), 80.0);

        // 主手也拿着烟花时 `offhand` 不成立，仍看主手：右手 +80、左手 -80。
        player.main_hand = rocket;
        player.off_hand = rocket;
        player.main_arm_right = true;
        assert_eq!(hand_holding_angle(&player), 80.0);
        player.main_arm_right = false;
        assert_eq!(hand_holding_angle(&player), -80.0);
    }
}
