//! 拴绳：官方的 `Leashable` 接口。
//!
//! 生物可以拴到玩家（或拴绳结）身上。每刻 [`tick_leash`] 按距离决定弹性拉扯还是断裂；
//! 拴上、解开、断裂都要发 `set_entity_link`，客户端才画得出绳子。
//!
//! Leashes, vanilla `Leashable`. A mob can be tied to a player (or a fence knot).
//! [`tick_leash`] pulls or snaps it by distance every tick; attach, detach and snap
//! each broadcast `set_entity_link` so clients draw the rope.

use mc_bamboo_core::{Aabb, BlockPos, Vec3, mth};

use super::{Mob, MobEvent, MobType};
use crate::entity::{Entity, EntityUid, ctx::EntityCtx};

/// `LEASH_TOO_FAR_DIST`：超过这个距离绳子断掉。
pub const LEASH_TOO_FAR_DIST: f64 = 12.0;
/// `LEASH_ELASTIC_DIST`：超过这个距离开始弹性拉扯。
pub const LEASH_ELASTIC_DIST: f64 = 6.0;
/// `AXIS_SPECIFIC_ELASTICITY`：弹性力按轴衰减的分量。
const AXIS_SPECIFIC_ELASTICITY: (f64, f64, f64) = (0.8, 0.2, 0.8);
/// `TORSIONAL_ELASTICITY`：角动量的扭转系数。
const TORSIONAL_ELASTICITY: f64 = 10.0;
/// `STIFFNESS`：跟随牵引者速度的刚度。
const STIFFNESS: f64 = 0.11;
/// `computeDampenedSpringInteraction` 里牵引者顺着拉力移动时的衰减。字节码写死 0.3，
/// 不是 `SPRING_DAMPENING` 常量的 0.7。
const SPRING_DAMPENING: f64 = 0.300_000_011_920_928_96;
/// `ENTITY_ATTACHMENT_POINT`（非四方拴绳的生物侧附着点，按宽高宽归一化）。
const ENTITY_ATTACHMENT_POINT: (f64, f64, f64) = (0.0, 0.5, 0.5);
/// `LEASHER_ATTACHMENT_POINT`（牵引者侧附着点）。
const LEASHER_ATTACHMENT_POINT: (f64, f64, f64) = (0.0, 0.5, 0.0);
/// `0.017453292F`：官方角度转弧度的常量。
const DEG_TO_RAD: f32 = (std::f64::consts::PI / 180.0) as f32;

/// 拴绳的牵引数据，官方的 `Leashable$LeashData`。
#[derive(Debug, Clone, PartialEq)]
pub struct LeashData {
    /// 当前牵引者（玩家、别的实体或拴绳结的 uid）。读档后、重连前是 `None`。
    pub holder: Option<EntityUid>,
    /// 牵引者的身份（UUID 或拴绳结位置），存档用；读档后据此重连。
    pub attachment: LeashDelayed,
    /// `angularMomentum`：弹性拉扯累计的角动量。
    pub angular_momentum: f64,
}

/// 存档里 `leash` 键的牵引者身份，官方的 `Either<UUID, BlockPos>`。
#[derive(Debug, Clone, PartialEq)]
pub enum LeashDelayed {
    /// 牵引者是另一个实体，记 UUID。
    Entity([u8; 16]),
    /// 牵引者是拴绳结，记位置。
    Knot(BlockPos),
}

impl LeashData {
    pub fn new(holder: EntityUid, attachment: LeashDelayed) -> Self {
        Self {
            holder: Some(holder),
            attachment,
            angular_momentum: 0.0,
        }
    }

    pub fn from_disk(attachment: LeashDelayed) -> Self {
        Self {
            holder: None,
            attachment,
            angular_momentum: 0.0,
        }
    }
}

/// `isLeashed()`：有牵引数据且牵引者非空。
pub fn is_leashed(mob: &Mob) -> bool {
    mob.leash.as_ref().is_some_and(|data| data.holder.is_some())
}

/// `mayBeLeashed()`：有牵引数据（不管牵引者有没有重连上）。
pub fn may_be_leashed(mob: &Mob) -> bool {
    mob.leash.is_some()
}

/// `getLeashHolder()`：当前牵引者的 uid。
pub fn leash_holder(mob: &Mob) -> Option<EntityUid> {
    mob.leash.as_ref().and_then(|data| data.holder)
}

/// `leashSnapDistance()`。
pub const fn leash_snap_distance() -> f64 {
    LEASH_TOO_FAR_DIST
}

/// `leashElasticDistance()`。
pub const fn leash_elastic_distance() -> f64 {
    LEASH_ELASTIC_DIST
}

/// `Leashable.leashableInArea` 的 32×32×32 区域判定：实体的碰撞盒和
/// `AABB.ofSize(center, 32, 32, 32)` 相交。
pub fn in_leashable_area(bounds: Aabb, center: Vec3) -> bool {
    let area = Aabb::new(
        center.x - 16.0,
        center.y - 16.0,
        center.z - 16.0,
        center.x + 16.0,
        center.y + 16.0,
        center.z + 16.0,
    );
    bounds.intersects(area)
}

/// `Mob.canBeLeashed()` 和各生物的覆盖。`now` 是岛时钟（狼的愤怒要看还剩多久）。
///
/// `Mob.canBeLeashed()` and each mob's override. `now` is the island clock (a wolf's
/// anger reads how much remains).
pub fn can_be_leashed(mob: &Mob, now: i64) -> bool {
    match mob.mob_type {
        // `AmbientCreature`（蝙蝠）/ `AgeableWaterCreature`（海龟）/ `WaterAnimal`（鱼、
        // 蝌蚪）/ `Panda` / `AbstractVillager`（村民）：不能拴。
        MobType::Bat
        | MobType::Turtle
        | MobType::Tadpole
        | MobType::Panda
        | MobType::Villager
        | MobType::Cod
        | MobType::Salmon
        | MobType::TropicalFish
        | MobType::Pufferfish => false,
        // `Wolf.canBeLeashed()`：`!isAngry()`。
        MobType::Wolf => !super::wolf::Wolf::is_angry(&mob.wolf, now),
        // `Nautilus.canBeLeashed()`：`!isAggravated()`；僵尸鹦鹉螺还要求不被骑。
        MobType::Nautilus => !nautilus_is_aggravated(mob),
        MobType::ZombieNautilus => !nautilus_is_aggravated(mob) && !is_mob_controlled(mob),
        // `SulfurCube.canBeLeashed()`：`hasBodyItem()`。
        MobType::SulfurCube => super::sulfur_cube::has_body_item(mob),
        // `CamelHusk` / `ZombieHorse` 的覆盖是 `!isMobControlled()`；Bamboo 没有生物骑
        // 生物，`isMobControlled()` 恒假，所以总能拴。
        MobType::CamelHusk | MobType::ZombieHorse => !is_mob_controlled(mob),
        // `Mob.canBeLeashed()`：`!(this instanceof Enemy)`。
        _ => !mob.mob_type.is_enemy(),
    }
}

/// `AbstractNautilus.isAggravated()`：`ANGRY_AT` 或 `ATTACK_TARGET` 记忆里有值。
/// Bamboo 没建模 `ANGRY_AT`，这里只认 `ATTACK_TARGET`。
fn nautilus_is_aggravated(mob: &Mob) -> bool {
    mob.brain.as_ref().is_some_and(|brain| {
        brain
            .memories
            .has(super::brain::memory::MemoryType::AttackTarget)
    })
}

/// `Entity.isMobControlled()`：第一名乘客是生物。Bamboo 只有玩家乘客，恒假。
fn is_mob_controlled(_mob: &Mob) -> bool {
    false
}

/// `canHaveALeashAttachedTo(holder)`：距离在断裂距离内且能拴。
pub fn can_have_a_leash_attached_to(
    mob: &Mob,
    entity_position: Vec3,
    entity_height: f32,
    holder_position: Vec3,
    holder_height: f32,
    now: i64,
) -> bool {
    leash_distance_to(
        entity_position,
        entity_height,
        holder_position,
        holder_height,
    ) <= leash_snap_distance()
        && can_be_leashed(mob, now)
}

/// `leashDistanceTo(holder)`：两个碰撞盒中心的距离。
pub fn leash_distance_to(
    entity_position: Vec3,
    entity_height: f32,
    holder_position: Vec3,
    holder_height: f32,
) -> f64 {
    let entity_center = Vec3::new(
        entity_position.x,
        entity_position.y + f64::from(entity_height) / 2.0,
        entity_position.z,
    );
    let holder_center = Vec3::new(
        holder_position.x,
        holder_position.y + f64::from(holder_height) / 2.0,
        holder_position.z,
    );
    distance_to(entity_center, holder_center)
}

/// 一个牵引者的公开数据，供每刻拉扯读取。
struct HolderInfo {
    position: Vec3,
    delta: Vec3,
    width: f32,
    height: f32,
    /// `canInteractWithLevel()`。
    interactable: bool,
    /// `getHolderMovement` 返回零：NoAI 的生物。
    no_ai_mob: bool,
}

/// 按 uid 找牵引者。玩家优先（uid 可能同时指向玩家和实体表里的某一行）。
fn resolve_holder(ctx: &dyn EntityCtx, uid: EntityUid) -> Option<HolderInfo> {
    let mut player = None;
    ctx.blocks().for_each_player(&mut |view| {
        if view.uid == uid {
            player = Some(*view);
        }
    });
    if let Some(view) = player {
        return Some(HolderInfo {
            position: view.position,
            // `getHolderMovement(player)` 走 `player.getKnownMovement()`。26.3 服务端的
            // 玩家移动是客户端权威，`getKnownMovement()` 返回 ZERO，不是 `getDeltaMovement()`；
            // 拴绳的弹性力因此不把玩家的上报速度算进去（逐刻对拍确认）。
            //
            // `getHolderMovement(player)` uses `player.getKnownMovement()`, which on the
            // 26.3 server is ZERO (client-authoritative movement), not `getDeltaMovement()`.
            // The elastic force therefore ignores the player's reported velocity.
            delta: Vec3::default(),
            width: view.width,
            height: view.height,
            interactable: view.alive && !view.spectator,
            no_ai_mob: false,
        });
    }
    let mut out = None;
    ctx.with_other(uid, &mut |public| {
        let (width, height) = public.kind.dimensions();
        out = Some(HolderInfo {
            position: public.position,
            delta: public.delta,
            width,
            height,
            interactable: !public.removed,
            no_ai_mob: matches!(
                &public.kind,
                crate::entity::KindPublic::Mob(mob) if mob.no_ai
            ),
        });
    });
    out
}

/// 掉一根拴绳的物品。
fn lead_item() -> mc_bamboo_protocol::item::ItemStack {
    mc_bamboo_protocol::item::ItemStack::new(mc_bamboo_data::items::LEAD, 1)
}

/// 每刻的拉扯：`Leashable.tickLeash`。
pub fn tick_leash(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    // `restoreLeashFromSave`：读档进来的牵引者身份每刻试着重连成 uid。
    let attachment = mob.leash.as_ref().map(|data| data.attachment.clone());
    if let Some(attachment) = attachment {
        if let Some(uid) = resolve_attachment(ctx, attachment) {
            if let Some(data) = mob.leash.as_mut() {
                data.holder = Some(uid);
                mob.events.push(MobEvent::LeashLink { holder: Some(uid) });
            }
        } else if me.tick_count > 100 {
            // 100 刻后还没找到：掉一根拴绳，绳子清空。
            mob.leash = None;
            ctx.emit(
                crate::entity::ctx::Command::DropItem { stack: lead_item() },
                me,
            );
            return;
        }
    }
    let Some(holder_uid) = leash_holder(mob) else {
        return;
    };
    // 自己和牵引者都「还能和世界交互」；否则按游戏规则掉拴绳或直接解掉。
    let entity_interactable = super::living::is_alive(me, mob);
    let holder = resolve_holder(ctx, holder_uid);
    let Some(holder) = holder else {
        // 牵引者不在这张表里（跨岛或已消失）：掉拴绳。
        if let Some(old) = drop_leash(mob) {
            ctx.emit(crate::entity::ctx::Command::NotifyLeasheeRemoved(old), me);
        }
        ctx.emit(
            crate::entity::ctx::Command::DropItem { stack: lead_item() },
            me,
        );
        return;
    };
    if !entity_interactable || !holder.interactable {
        if ctx.blocks().block_game_rules().entity_drops {
            if let Some(old) = drop_leash(mob) {
                ctx.emit(crate::entity::ctx::Command::NotifyLeasheeRemoved(old), me);
            }
            ctx.emit(
                crate::entity::ctx::Command::DropItem { stack: lead_item() },
                me,
            );
        } else if let Some(old) = remove_leash(mob) {
            ctx.emit(crate::entity::ctx::Command::NotifyLeasheeRemoved(old), me);
        }
        return;
    }
    // 换维度不在这里处理：同一岛上没有别的维度，绳子原样保留。
    let dist = leash_distance_to(me.position, me.dimensions.1, holder.position, holder.height);
    if dist > leash_snap_distance() {
        // `level.playSound(null, holder.pos, LEAD_BREAK, NEUTRAL, 1, 1)`。
        mob.events.push(MobEvent::Sound {
            sound: "minecraft:item.lead.break",
            volume: 1.0,
            pitch: 1.0,
            position: holder.position,
            hostile: false,
            seed: None,
        });
        // `Mob.leashTooFarBehaviour()`：`dropLeash()` + `disableControlFlag(MOVE)`。
        // 官方 `tickLeash` 在断绳分支用 `goto` 跳到尾部，仍用局部 leashData 执行
        // `setYRot(yRot - angularMomentum)`——这一帧照样把累计角动量应用到朝向。
        // 之后 leashData 已弃用，`ang *= friction` 无副作用，可省。
        if let Some(data) = mob.leash.as_ref() {
            me.y_rot = (f64::from(me.y_rot) - data.angular_momentum) as f32;
        }
        if let Some(old) = drop_leash(mob) {
            ctx.emit(crate::entity::ctx::Command::NotifyLeasheeRemoved(old), me);
        }
        ctx.emit(
            crate::entity::ctx::Command::DropItem { stack: lead_item() },
            me,
        );
        mob.goals.set_control_flag(super::goals::Flag::Move, false);
        return;
    }
    let elastic_threshold =
        leash_elastic_distance() - f64::from(holder.width) - f64::from(me.dimensions.0);
    if dist > elastic_threshold && elastic_interactions(me, mob, &holder) {
        // `onElasticLeashPull()`：`checkFallDistanceAccumulation()`。
        if me.delta.y > -0.5 && me.fall_distance > 1.0 {
            me.fall_distance = 1.0;
        }
    }
    // `entity.setYRot((float)(yRot - angularMomentum))`，再按摩擦衰减角动量。
    if let Some(data) = mob.leash.as_mut() {
        me.y_rot = (f64::from(me.y_rot) - data.angular_momentum) as f32;
        data.angular_momentum *= f64::from(angular_friction(me, ctx));
    }
}

/// `angularFriction`：着地看脚下方块摩擦，在水里 0.8，否则 0.91。
fn angular_friction(me: &Entity, ctx: &dyn EntityCtx) -> f32 {
    if me.on_ground {
        let below = me.on_pos(ctx.blocks(), 0.500_000_1);
        crate::entity::item::friction(ctx.blocks().block_state(below)) * 0.91
    } else if me.in_water {
        0.8
    } else {
        0.91
    }
}

/// `checkElasticInteractions`：弹性拉扯，返回有没有拉。
fn elastic_interactions(me: &mut Entity, mob: &mut Mob, holder: &HolderInfo) -> bool {
    // `getHolderMovement(holder)`：STIFFNESS 项用。NoAI 生物 / 服务端玩家都是 ZERO。
    let holder_movement = if holder.no_ai_mob {
        Vec3::default()
    } else {
        holder.delta
    };
    // `computeDampenedSpringInteraction` 的 dampening 判断用「被拴生物」自己的
    // `getHolderMovement`（NoAI→ZERO），不是 holder 的。
    let leashed_movement = if mob.no_ai { Vec3::default() } else { me.delta };
    // 单点附着，不做四方拴绳（快乐恶魂、船之间的四点拴绳还没建模）。
    let entity_offset = y_rot(
        Vec3::new(
            ENTITY_ATTACHMENT_POINT.0 * f64::from(me.dimensions.0),
            ENTITY_ATTACHMENT_POINT.1 * f64::from(me.dimensions.1),
            ENTITY_ATTACHMENT_POINT.2 * f64::from(me.dimensions.0),
        ),
        -(me.y_rot * DEG_TO_RAD),
    );
    let holder_offset = y_rot(
        Vec3::new(
            LEASHER_ATTACHMENT_POINT.0 * f64::from(holder.width),
            LEASHER_ATTACHMENT_POINT.1 * f64::from(holder.height),
            LEASHER_ATTACHMENT_POINT.2 * f64::from(holder.width),
        ),
        0.0,
    );
    let entity_point = me.position.add(entity_offset);
    let holder_point = holder.position.add(holder_offset);
    let Some((force, torque)) =
        dampened_spring(entity_point, holder_point, leashed_movement, entity_offset)
    else {
        return false;
    };
    let Some(data) = mob.leash.as_mut() else {
        return false;
    };
    data.angular_momentum += TORSIONAL_ELASTICITY * torque;
    let delta = Vec3::new(
        force.x * AXIS_SPECIFIC_ELASTICITY.0,
        force.y * AXIS_SPECIFIC_ELASTICITY.1,
        force.z * AXIS_SPECIFIC_ELASTICITY.2,
    )
    .add(holder_movement.subtract(me.delta).scale(STIFFNESS));
    me.delta = me.delta.add(delta);
    true
}

/// `computeDampenedSpringInteraction`：超过弹性距离时给一个（力，力矩）。
fn dampened_spring(
    entity_point: Vec3,
    holder_point: Vec3,
    holder_movement: Vec3,
    attachment_point: Vec3,
) -> Option<(Vec3, f64)> {
    let dist = distance_to(entity_point, holder_point);
    if dist < leash_elastic_distance() {
        return None;
    }
    // `computeDampenedSpringInteraction(holderPoint, entityPoint, ...)`：
    // force = (holderPoint - entityPoint) 归一化后乘 (dist - elastic)，指向 holder。
    let mut force = holder_point
        .subtract(entity_point)
        .normalize()
        .scale(dist - leash_elastic_distance());
    let torque = attachment_point.z * force.x - attachment_point.x * force.z;
    if dot(holder_movement, force) >= 0.0 {
        force = force.scale(SPRING_DAMPENING);
    }
    Some((force, torque))
}

/// `Vec3.yRot(angle)`：绕 Y 轴旋转。
fn y_rot(vector: Vec3, angle: f32) -> Vec3 {
    let cos = mth::cos(f64::from(angle));
    let sin = mth::sin(f64::from(angle));
    Vec3::new(
        vector.x * f64::from(cos) + vector.z * f64::from(sin),
        vector.y,
        vector.z * f64::from(cos) - vector.x * f64::from(sin),
    )
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn distance_to(a: Vec3, b: Vec3) -> f64 {
    a.subtract(b).length()
}

/// 按身份（UUID 或拴绳结位置）把读档的牵引者重连成 uid。
fn resolve_attachment(ctx: &dyn EntityCtx, attachment: LeashDelayed) -> Option<EntityUid> {
    match attachment {
        LeashDelayed::Entity(uuid) => crate::entity::arrow::resolve_owner(ctx, uuid),
        LeashDelayed::Knot(_pos) => None,
    }
}

/// `setLeashedTo(entity, holder)`：把牵引者换成 `holder`，广播 `set_entity_link`。
/// `attachment` 是牵引者身份（玩家 / 生物的 UUID），存档时原样写回。返回旧牵引者
/// （`setLeashedTo` 里旧牵引者非空且不同时要 `notifyLeasheeRemoved`）。
pub fn set_leashed_to(
    mob: &mut Mob,
    holder: EntityUid,
    attachment: LeashDelayed,
) -> Option<EntityUid> {
    let old = leash_holder(mob);
    // 官方 `setLeashedTo`：已有 `LeashData` 时复用同一个对象（保留 `angularMomentum`），
    // 只 `setLeashHolder` 并清掉 delayed 身份；只有原来没有牵引数据时才新建。
    if let Some(data) = mob.leash.as_mut() {
        data.holder = Some(holder);
        data.attachment = attachment;
    } else {
        mob.leash = Some(LeashData::new(holder, attachment));
    }
    mob.events.push(MobEvent::LeashLink {
        holder: Some(holder),
    });
    old.filter(|&old| old != holder)
}

/// `dropLeash()`：解开并掉一根拴绳，广播 `set_entity_link` 断开。掉落物由调用方 emit。
/// 返回旧牵引者（要给 `notifyLeasheeRemoved`）。
pub fn drop_leash(mob: &mut Mob) -> Option<EntityUid> {
    let old = leash_holder(mob);
    clear_leash(mob);
    mob.events.push(MobEvent::LeashLink { holder: None });
    old
}

/// `removeLeash()`：解开但**不**掉拴绳，广播 `set_entity_link` 断开。返回旧牵引者。
pub fn remove_leash(mob: &mut Mob) -> Option<EntityUid> {
    let old = leash_holder(mob);
    clear_leash(mob);
    mob.events.push(MobEvent::LeashLink { holder: None });
    old
}

/// 清掉牵引数据，并跑 `Mob.onLeashRemoved()`（`getLeashData() == null` 时 `clearHome()`）。
fn clear_leash(mob: &mut Mob) {
    mob.leash = None;
    mob.home = None;
}
