//! 拴绳：官方的 `Leashable` 接口。
//!
//! 生物可以拴到玩家（或拴绳结）身上。每刻 [`tick_leash`] 按距离决定弹性拉扯还是断裂；
//! 拴上、解开、断裂都要发 `set_entity_link`，客户端才画得出绳子。
//!
//! Leashes, vanilla `Leashable`. A mob can be tied to a player (or a fence knot).
//! [`tick_leash`] pulls or snaps it by distance every tick; attach, detach and snap
//! each broadcast `set_entity_link` so clients draw the rope.

use std::cell::Cell;

use mc_bamboo_core::{Aabb, BlockPos, Vec3, mth};

use super::{Mob, MobEvent, MobType, goals};
use crate::entity::{Entity, EntityUid, ctx::EntityCtx};

thread_local! {
    /// 仅供 `mob-oracle --break-self-test leash_nav`：命中时让
    /// `close_range_leash_behaviour` 跳过导航，模拟「被拴生物不自己跟着走」这个回归，
    /// 逐刻对拍应报不一致。默认 false，正式游戏零影响。
    static SKIP_LEASH_NAV: Cell<bool> = const { Cell::new(false) };
}

/// 改坏确认开关，只由 `mob-oracle` 调用；默认 false。
pub fn set_oracle_skip_leash_nav(value: bool) {
    SKIP_LEASH_NAV.with(|flag| flag.set(value));
}

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

/// `Leashable` 的两个距离覆写：`HappyGhast.leashElasticDistance()=10`、
/// `leashSnapDistance()=16`，其余沿用缺省的 6 / 12。
///
/// The two `Leashable` distance overrides: `HappyGhast.leashElasticDistance()=10`
/// and `leashSnapDistance()=16`; the rest keep the 6 / 12 defaults.
pub fn elastic_distance_of(kind: MobType) -> f64 {
    if kind == MobType::HappyGhast {
        super::happy_ghast::LEASH_ELASTIC_DISTANCE
    } else {
        LEASH_ELASTIC_DIST
    }
}

/// 按种类取 `leashSnapDistance()`。
///
/// Per-kind `leashSnapDistance()`.
pub fn snap_distance_of(kind: MobType) -> f64 {
    if kind == MobType::HappyGhast {
        super::happy_ghast::LEASH_SNAP_DISTANCE
    } else {
        LEASH_TOO_FAR_DIST
    }
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
    ) <= snap_distance_of(mob.mob_type)
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
    /// `Entity.supportQuadLeashAsHolder()`：目前只有快乐恶魂。
    quad_holder: bool,
    /// `Entity.getQuadLeashHolderOffsets()`：快乐恶魂的四个实体坐标偏移。
    quad_offsets: Option<[Vec3; 4]>,
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
            quad_holder: false,
            quad_offsets: None,
        });
    }
    let mut out = None;
    ctx.with_other(uid, &mut |public| {
        let (width, height) = public.kind.dimensions();
        let happy = matches!(
            &public.kind,
            crate::entity::KindPublic::Mob(mob)
                if mob.mob_type == MobType::HappyGhast
        );
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
            quad_holder: happy,
            quad_offsets: happy.then(|| super::happy_ghast::quad_holder_offsets(width, height)),
        });
    });
    out
}

/// 掉一根拴绳的物品。
fn lead_item() -> mc_bamboo_protocol::item::ItemStack {
    mc_bamboo_protocol::item::ItemStack::new(mc_bamboo_data::items::LEAD, 1)
}

/// `PathfinderMob.shouldStayCloseToLeashHolder()`：默认 true；`Allay` / `HappyGhast`
/// 覆盖成 false。Bamboo 只建模了快乐恶魂，悦灵不在表里。
fn should_stay_close_to_leash_holder(kind: MobType) -> bool {
    kind != MobType::HappyGhast
}

/// `PathfinderMob.followLeashSpeed()`：默认 1.0；`Llama`（含继承它的行商羊驼）覆盖成 2.0。
fn follow_leash_speed(kind: MobType) -> f64 {
    if kind == MobType::Llama { 2.0 } else { 1.0 }
}

/// 被拴的生物是不是 `PathfinderMob`：只有这一类覆盖 `whenLeashedTo` /
/// `closeRangeLeashBehaviour`；直接继承 `Mob` 的（蝙蝠、史莱姆、鱿鱼、悦灵等）走
/// `Leashable` 的空默认。官方类层次里 `Animal` / `Monster` / `AgeableMob` /
/// `TamableAnimal` / `AbstractHorse` / `Villager` / `Allay` / 傀儡都挂在
/// `PathfinderMob` 下；这里按能被拴的建模类型判定，非寻路生物不跟着牵引者走。
fn is_pathfinder_mob(kind: MobType) -> bool {
    !matches!(
        kind,
        MobType::Bat | MobType::Slime | MobType::Squid | MobType::GlowSquid
    )
}

/// `PathfinderMob.isPanicking()`：大脑的 `IS_PANICKING` 记忆或正在跑的 `PanicGoal`。
/// Bamboo 的目标系统里 `goals.is_panicking()` 覆盖后者；有大脑的生物（村民、悦灵等）
/// 的恐慌由 `goals` 里的对应目标体现，这里沿用目标系统的判断。
fn is_panicking(mob: &Mob) -> bool {
    mob.goals.is_panicking()
}

/// `PathfinderMob.whenLeashedTo(holder)`：每刻把家设到牵引者所在方块，半径
/// `leashElasticDistance() - 1`（6-1=5）。父类 `Mob.whenLeashedTo` →
/// `Leashable.whenLeashedTo` → `holder.notifyLeashHolder(this)` 是空操作。
///
/// 官方 `tickLeash` 在算完 `leashDistanceTo` 之后、判断 snap / elastic 之前无条件调它。
fn when_leashed_to(mob: &mut Mob, holder: &HolderInfo) {
    if !is_pathfinder_mob(mob.mob_type) {
        return;
    }
    mob.home = Some((
        BlockPos::containing(holder.position),
        elastic_distance_of(mob.mob_type) as i32 - 1,
    ));
}

/// `PathfinderMob.closeRangeLeashBehaviour(holder)`：近距离（没断绳、没弹性拉扯）时
/// 打开 `MOVE` 控制位并朝牵引者导航，停在离牵引者 2 格处。玩家牵着动物走时动物就是
/// 靠这个自己走过来的，弹性拉扯只是兜底。
fn close_range_leash_behaviour(
    me: &mut Entity,
    mob: &mut Mob,
    holder: &HolderInfo,
    ctx: &dyn EntityCtx,
) {
    // `super.closeRangeLeashBehaviour` 是 `Leashable` 的空默认。
    if !is_pathfinder_mob(mob.mob_type)
        || !should_stay_close_to_leash_holder(mob.mob_type)
        || is_panicking(mob)
    {
        return;
    }
    // `goalSelector.enableControlFlag(MOVE)`。
    mob.goals.set_control_flag(goals::Flag::Move, true);
    // `Entity.distanceTo(Entity)`：每格差值 d2f 后算平方和，再 `Mth.sqrt(F)F`。
    let dx = (me.position.x - holder.position.x) as f32;
    let dy = (me.position.y - holder.position.y) as f32;
    let dz = (me.position.z - holder.position.z) as f32;
    let distance = mth::sqrt_f32(dx * dx + dy * dy + dz * dz);
    // 朝牵引者的方向，长度 `max(distance - 2, 0)`；目标是 `position + offset`。
    let offset = Vec3::new(
        holder.position.x - me.position.x,
        holder.position.y - me.position.y,
        holder.position.z - me.position.z,
    )
    .normalize()
    .scale(f64::from((distance - 2.0).max(0.0)));
    let at = me.position.add(offset);
    let view = ctx.blocks();
    let nav = super::living::nav_mob(me, mob, view);
    // `mob-oracle --break-self-test leash_nav`：故意不调导航（其余逻辑照旧）。
    if SKIP_LEASH_NAV.with(|flag| flag.get()) {
        return;
    }
    mob.navigation
        .move_to(view, &nav, at, follow_leash_speed(mob.mob_type));
}

/// `onElasticLeashPull()`：默认 `checkFallDistanceAccumulation()`；`AbstractHorse` 系
/// 额外停下吃草，`Camel` 系额外从坐姿站起来。这些只在弹性拉扯那一刻发生。
fn on_elastic_leash_pull(me: &mut Entity, mob: &mut Mob, time: i64) {
    // `Entity.checkFallDistanceAccumulation()`。
    if me.delta.y > -0.5 && me.fall_distance > 1.0 {
        me.fall_distance = 1.0;
    }
    // `AbstractHorse.onElasticLeashPull`：`if (isEating()) setEating(false)`。
    // 马科 / 羊驼 / 骆驼都继承 `AbstractHorse`。
    if mob.mob_type.is_equine()
        && let Some(equine) = mob.equine.as_mut()
        && equine.eating
    {
        equine.eating = false;
    }
    // `HappyGhast.onElasticLeashPull`：`super` 之后让移动控制停下。
    // `HappyGhast.onElasticLeashPull`: after `super`, stop the move control.
    if mob.mob_type == MobType::HappyGhast {
        mob.mover.operation = super::control::Operation::Wait;
    }
    // `Camel.onElasticLeashPull`：坐着、不在换姿势、能换姿势时站起来。`Camel` 的子类
    // `CamelHusk` 一并覆盖。Bamboo 的 `canCamelChangePose` 在开阔平地恒真。
    if matches!(mob.mob_type, MobType::Camel | MobType::CamelHusk)
        && super::camel::is_camel_sitting(mob)
        && !super::camel::is_in_pose_transition(mob, time)
    {
        super::camel::stand_up_instantly(mob, time);
    }
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
    // `whenLeashedTo(holder)`：官方在算完 `leashDistanceTo` 之后、判断 snap / elastic
    // 之前每刻无条件调用（`setHomeTo`），让被拴的寻路生物把游荡目标限制在牵引者附近。
    when_leashed_to(mob, &holder);
    if dist > snap_distance_of(mob.mob_type) {
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
        elastic_distance_of(mob.mob_type) - f64::from(holder.width) - f64::from(me.dimensions.0);
    if dist > elastic_threshold && elastic_interactions(me, mob, &holder) {
        // `onElasticLeashPull()`：默认 `checkFallDistanceAccumulation()`，马科 / 骆驼
        // 另有附加行为。
        on_elastic_leash_pull(me, mob, ctx.blocks().game_time());
    } else {
        // `closeRangeLeashBehaviour(holder)`：没断绳、没弹性拉扯时，被拴的寻路生物
        // 自己朝牵引者走过来（打开 MOVE 控制位并导航）。
        close_range_leash_behaviour(me, mob, &holder, ctx);
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

/// `Leashable.checkElasticInteractions`：四方拴绳时按四个角分别算弹簧再累加，
/// 合力乘 0.25；单点拴绳沿用老路径。
///
/// `Leashable.checkElasticInteractions`: with a quad leash each of the four corners
/// contributes a spring whose accumulated wrench is scaled by 0.25; single-point
/// leashes keep the old path.
fn elastic_interactions(me: &mut Entity, mob: &mut Mob, holder: &HolderInfo) -> bool {
    // `getHolderMovement(holder)`：STIFFNESS 项用。NoAI 生物 / 服务端玩家都是 ZERO。
    let holder_movement = if holder.no_ai_mob {
        Vec3::default()
    } else {
        holder.delta
    };
    // `computeDampenedSpringInteraction` 的 dampening 判断用「被拴生物」自己的
    // `getKnownMovement`（NoAI→ZERO），不是 holder 的。
    let leashed_movement = if mob.no_ai { Vec3::default() } else { me.delta };
    // `Entity.supportQuadLeashAsHolder() && this.supportQuadLeash()`：
    // 只有「牵引者是快乐恶魂且被拴的一方也支持四点」时走四点。26.3 里
    // `HappyGhast` 没有覆盖 `supportQuadLeash`，所以被拴的一方恒为单点。
    let quad = holder.quad_holder && super::happy_ghast::SUPPORTS_QUAD_AS_LEASHED;
    let elastic = elastic_distance_of(mob.mob_type);
    let mut force = Vec3::default();
    let mut torque = 0.0;
    let mut pulled = false;
    if quad {
        let holder_scale = Vec3::new(
            f64::from(holder.width),
            f64::from(holder.height),
            f64::from(holder.width),
        );
        let leashed_scale = Vec3::new(
            f64::from(me.dimensions.0),
            f64::from(me.dimensions.1),
            f64::from(me.dimensions.0),
        );
        for index in 0..4 {
            let entity_offset = y_rot(
                super::happy_ghast::SHARED_QUAD_ATTACHMENT_POINTS[index].multiply(
                    leashed_scale.x,
                    leashed_scale.y,
                    leashed_scale.z,
                ),
                -(me.y_rot * DEG_TO_RAD),
            );
            let holder_offset = y_rot(
                holder.quad_offsets.map_or_else(
                    || {
                        super::happy_ghast::SHARED_QUAD_ATTACHMENT_POINTS[index].multiply(
                            holder_scale.x,
                            holder_scale.y,
                            holder_scale.z,
                        )
                    },
                    |offsets| offsets[index],
                ),
                0.0,
            );
            let Some((point_force, point_torque)) = quad_spring(
                me.position.add(entity_offset),
                holder.position.add(holder_offset),
                leashed_movement,
                entity_offset,
                elastic,
            ) else {
                continue;
            };
            force = force.add(point_force);
            torque += point_torque;
            pulled = true;
        }
        // `Wrench.scale(quad ? 0.25 : 1.0)`。
        force = force.scale(0.25);
        torque *= 0.25;
    } else {
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
        let Some((single_force, single_torque)) = dampened_spring(
            me.position.add(entity_offset),
            holder.position.add(holder_offset),
            leashed_movement,
            entity_offset,
            elastic,
        ) else {
            return false;
        };
        force = single_force;
        torque = single_torque;
        pulled = true;
    }
    if !pulled {
        return false;
    }
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

/// `Leashable.computeElasticInteraction` 里每个角的弹簧：
/// `holderPoint.distanceTo(entityPoint) >= elastic` 才出力，力指向 holder；
/// dampening 判 `dot(getHolderMovement(holder), force) >= 0`，力矩按附着点算。
///
/// One corner spring in `Leashable.computeElasticInteraction`: it only pushes when
/// `holderPoint.distanceTo(entityPoint) >= elastic`, the force points to the holder,
/// dampening reads `dot(getHolderMovement(holder), force) >= 0`, and the torque
/// comes from the attachment point.
fn quad_spring(
    entity_point: Vec3,
    holder_point: Vec3,
    holder_movement: Vec3,
    attachment_point: Vec3,
    elastic: f64,
) -> Option<(Vec3, f64)> {
    let dist = distance_to(holder_point, entity_point);
    if dist < elastic {
        return None;
    }
    let mut force = entity_point
        .subtract(holder_point)
        .normalize()
        .scale(dist - elastic);
    let torque = attachment_point.z * force.x - attachment_point.x * force.z;
    if dot(holder_movement, force) >= 0.0 {
        force = force.scale(SPRING_DAMPENING);
    }
    Some((force, torque))
}

/// `computeDampenedSpringInteraction`（单点路径）：超过弹性距离时给一个（力，力矩）。
fn dampened_spring(
    entity_point: Vec3,
    holder_point: Vec3,
    holder_movement: Vec3,
    attachment_point: Vec3,
    elastic: f64,
) -> Option<(Vec3, f64)> {
    let dist = distance_to(entity_point, holder_point);
    if dist < elastic {
        return None;
    }
    // `computeDampenedSpringInteraction(holderPoint, entityPoint, ...)`：
    // force = (holderPoint - entityPoint) 归一化后乘 (dist - elastic)，指向 holder。
    let mut force = holder_point
        .subtract(entity_point)
        .normalize()
        .scale(dist - elastic);
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
