//! 传感器：官方 `Sensor` 的扫描节奏（每 `scanRate` 刻一次，起始延迟随机）和 Bamboo
//! 用到的几种具体传感器，以及读别的活体（玩家或生物）的公共查询。
//!
//! Sensors: vanilla `Sensor` scheduling (every `scanRate` ticks after a random start
//! delay), the concrete sensors Bamboo uses, and the shared lookups of other living
//! entities (players or mobs).

use std::sync::Arc;

use mc_bamboo_core::{Aabb, Vec3, mth};
use mc_bamboo_data::registries::attribute;

use super::{
    super::{MobPublic as KindPublicMob, MobType, camel, goals, living},
    behavior::Cx,
    memory::{MemoryType, MemoryValue, NearestVisible},
};
use crate::entity::{EntityUid, KindPublic, ctx::Command};

/// Bamboo 建模的传感器种类。
///
/// The sensor types Bamboo models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorKind {
    /// `NearestLivingEntitySensor`。
    NearestLivingEntities,
    /// `HurtBySensor`。
    HurtBy,
    /// `TemptingSensor.forAnimal()`。
    FoodTemptations,
    /// `AdultSensor`。
    NearestAdult,
    /// `MobSensor(5, Armadillo::isScaredBy, Armadillo::canStayRolledUp, DANGER_DETECTED_RECENTLY, 80)`。
    ArmadilloScareDetected,
    /// `PlayerSensor`。
    NearestPlayers,
    /// `NearestItemSensor`。
    NearestItems,
    /// `FrogAttackablesSensor`（`NearestVisibleLivingEntitySensor` 子类）。
    ///
    /// `FrogAttackablesSensor` (a `NearestVisibleLivingEntitySensor` subclass).
    FrogAttackables,
    /// `TemptingSensor(FrogAi.getTemptations())`，即 `SensorType.FROG_TEMPTATIONS`。
    ///
    /// `TemptingSensor(FrogAi.getTemptations())`, i.e. `SensorType.FROG_TEMPTATIONS`.
    FrogTemptations,
    /// `IsInWaterSensor`。
    ///
    /// `IsInWaterSensor`.
    IsInWater,
    /// `AxolotlAttackablesSensor`（`NearestVisibleLivingEntitySensor` 子类）。
    ///
    /// `AxolotlAttackablesSensor` (a `NearestVisibleLivingEntitySensor` subclass).
    AxolotlAttackables,
}

impl SensorKind {
    /// `Sensor.scanRate`。
    ///
    /// `Sensor.scanRate`.
    pub const fn scan_rate(self) -> i32 {
        match self {
            Self::ArmadilloScareDetected => 5,
            _ => 20,
        }
    }

    /// `Sensor.requires()`：构造大脑时要登记的记忆。
    ///
    /// `Sensor.requires()`: the memories registered when the brain is built.
    pub fn requires(self) -> &'static [MemoryType] {
        match self {
            Self::NearestLivingEntities => &[
                MemoryType::NearestLivingEntities,
                MemoryType::NearestVisibleLivingEntities,
            ],
            Self::HurtBy => &[MemoryType::HurtBy, MemoryType::HurtByEntity],
            Self::FoodTemptations => &[MemoryType::TemptingPlayer],
            Self::NearestAdult => &[
                MemoryType::NearestVisibleAdult,
                MemoryType::NearestVisibleLivingEntities,
            ],
            Self::ArmadilloScareDetected => &[MemoryType::NearestLivingEntities],
            Self::NearestPlayers => &[
                MemoryType::NearestPlayers,
                MemoryType::NearestVisiblePlayer,
                MemoryType::NearestVisibleAttackablePlayer,
                MemoryType::NearestVisibleAttackablePlayers,
            ],
            Self::NearestItems => &[MemoryType::NearestVisibleWantedItem],
            // `FrogAttackablesSensor.requires()` = 视线表 + `NEAREST_ATTACKABLE`
            // + `UNREACHABLE_TONGUE_TARGETS`（`Sets.union(super.requires(), ...)`）。
            //
            // `FrogAttackablesSensor.requires()` = the visible-entity table +
            // `NEAREST_ATTACKABLE` + `UNREACHABLE_TONGUE_TARGETS`.
            Self::FrogAttackables => &[
                MemoryType::NearestVisibleLivingEntities,
                MemoryType::NearestAttackable,
                MemoryType::UnreachableTongueTargets,
            ],
            Self::FrogTemptations => &[MemoryType::TemptingPlayer],
            Self::IsInWater => &[MemoryType::IsInWater],
            // `AxolotlAttackablesSensor.requires()` = 视线表 + `NEAREST_ATTACKABLE`
            // + `HAS_HUNTING_COOLDOWN`。
            //
            // `AxolotlAttackablesSensor.requires()` = the visible-entity table +
            // `NEAREST_ATTACKABLE` + `HAS_HUNTING_COOLDOWN`.
            Self::AxolotlAttackables => &[
                MemoryType::NearestVisibleLivingEntities,
                MemoryType::NearestAttackable,
                MemoryType::HuntingCooldown,
            ],
        }
    }

    /// 官方传感器类的简单类名，对拍时和 Java 的 `getSimpleName()` 一致。
    ///
    /// The vanilla sensor class's simple name, matching Java's `getSimpleName()`.
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::NearestLivingEntities => "NearestLivingEntitySensor",
            Self::HurtBy => "HurtBySensor",
            Self::FoodTemptations | Self::FrogTemptations => "TemptingSensor",
            Self::NearestAdult => "AdultSensor",
            Self::ArmadilloScareDetected => "MobSensor",
            Self::NearestPlayers => "PlayerSensor",
            Self::NearestItems => "NearestItemSensor",
            Self::FrogAttackables => "FrogAttackablesSensor",
            Self::IsInWater => "IsInWaterSensor",
            Self::AxolotlAttackables => "AxolotlAttackablesSensor",
        }
    }
}

/// 一个传感器：种类和下次扫描的倒计时。
///
/// One sensor: its type and the countdown to its next scan.
#[derive(Debug, Clone, PartialEq)]
pub struct Sensor {
    pub kind: SensorKind,
    pub time_to_tick: i64,
}

impl Sensor {
    pub fn new(kind: SensorKind) -> Self {
        Self {
            kind,
            time_to_tick: 0,
        }
    }

    /// `Sensor.tick`：倒计时到 0 就重置成 `scanRate` 并扫描一次。
    ///
    /// `Sensor.tick`: when the countdown reaches 0, reset it to `scanRate` and scan.
    pub(super) fn tick(&mut self, cx: &mut Cx) {
        self.time_to_tick -= 1;
        if self.time_to_tick > 0 {
            return;
        }
        self.time_to_tick = i64::from(self.kind.scan_rate());
        match self.kind {
            SensorKind::NearestLivingEntities => sense_nearest_living(cx),
            SensorKind::HurtBy => sense_hurt_by(cx),
            SensorKind::FoodTemptations => sense_temptations(cx),
            SensorKind::NearestAdult => sense_nearest_adult(cx),
            SensorKind::ArmadilloScareDetected => {
                super::super::armadillo::sense_scare(cx);
            }
            SensorKind::NearestPlayers => sense_players(cx),
            SensorKind::NearestItems => sense_items(cx),
            SensorKind::FrogAttackables => super::super::frog::sense_attackables(cx),
            // `FROG_TEMPTATIONS` 和 `TemptingSensor.forAnimal()` 同形：青蛙的
            // `isFood` 就是 `#frog_food`，`goals::nearest_tempting_player` 走同一张表。
            //
            // `FROG_TEMPTATIONS` has the same shape as `TemptingSensor.forAnimal()`:
            // the frog's `isFood` is `#frog_food`, the same table this lookup uses.
            SensorKind::FrogTemptations => sense_temptations(cx),
            SensorKind::IsInWater => super::super::frog::sense_is_in_water(cx),
            SensorKind::AxolotlAttackables => super::super::axolotl::sense_attackables(cx),
        }
    }
}

/// 读到的一个活体（玩家或生物）在这一刻的样子。
///
/// One living entity (player or mob) as seen right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LivingView {
    pub uid: EntityUid,
    pub player: bool,
    pub mob_type: Option<MobType>,
    pub position: Vec3,
    pub eye_height: f32,
    pub bounding_box: Aabb,
    /// `isAlive()`：没被移除且血量大于 0。
    ///
    /// `isAlive()`: not removed and health above 0.
    pub alive: bool,
    pub spectator: bool,
    /// 创造模式的玩家（`canBeSeenAsEnemy` 为假）。
    ///
    /// A creative player, for whom `canBeSeenAsEnemy` is false.
    pub creative: bool,
    pub sprinting: bool,
    pub baby: bool,
    pub in_love: bool,
    pub panicking: bool,
    /// `getVisibilityPercent`：潜行 0.8、隐身按盔甲覆盖。
    ///
    /// `getVisibilityPercent`: 0.8 when sneaking, armour cover when invisible.
    pub visibility: f64,
}

impl LivingView {
    fn eye_position(&self) -> Vec3 {
        self.position
            .add(Vec3::new(0.0, f64::from(self.eye_height), 0.0))
    }
}

/// 按 uid 读一个活体。先借用实体表：命中就一定是生物，省掉为了「是不是玩家」而
/// 遍历全部玩家；没命中才去玩家名单里找。自己读不到（tick 期间被取出来了）。
///
/// Reads a living entity by uid. It borrows the entity table first — a hit is
/// definitely a mob, so it avoids scanning every player just to decide whether the
/// entity is a player; only a miss falls back to the player list. The mob itself is
/// not visible (it is taken out while ticking).
pub fn living(cx: &Cx, uid: EntityUid) -> Option<LivingView> {
    let mut mob = None;
    cx.ctx.with_other(uid, &mut |public| {
        if let KindPublic::Mob(m) = &public.kind {
            mob = Some(mob_view(public.uid, public.position, public.removed, m));
        }
    });
    if mob.is_some() {
        return mob;
    }
    let mut found = None;
    cx.ctx.blocks().for_each_player(&mut |player| {
        if player.uid == uid {
            found = Some(player_view(player));
        }
    });
    found
}

fn player_view(player: &crate::entity::ctx::PlayerView) -> LivingView {
    // `Player` 的碰撞箱：站着 0.6×1.8，潜行 0.6×1.5。
    // The player box: 0.6 by 1.8 standing, 0.6 by 1.5 crouching.
    let height = if player.sneaking { 1.5 } else { 1.8 };
    let mut visibility = 1.0_f64;
    if player.sneaking {
        visibility *= 0.8;
    }
    if player.invisible {
        visibility *= 0.7 * f64::from(mth::java_max_f32(player.armor_cover, 0.1));
    }
    LivingView {
        uid: player.uid,
        player: true,
        mob_type: None,
        position: player.position,
        eye_height: player.eye_height,
        bounding_box: Aabb::new(
            player.position.x - 0.3,
            player.position.y,
            player.position.z - 0.3,
            player.position.x + 0.3,
            player.position.y + height,
            player.position.z + 0.3,
        ),
        alive: player.alive,
        spectator: player.spectator,
        creative: player.creative,
        sprinting: player.sprinting,
        baby: false,
        in_love: false,
        panicking: false,
        visibility: mth::clamp(visibility, 0.0, 10.0),
    }
}

fn mob_view(uid: EntityUid, position: Vec3, removed: bool, mob: &KindPublicMob) -> LivingView {
    let (width, height) = mob.dimensions();
    let half = f64::from(width) / 2.0;
    // 骆驼坐下时 `getEyeHeight()` 跟着姿势变（2.275 → 0.845），看它的幼崽的
    // `LookControl` 读的就是这个动态眼高，不能用按种类算出来的静态值。
    // A sitting camel's `getEyeHeight()` follows the pose (2.275 → 0.845); a
    // watching baby's `LookControl` reads that live eye height, not a static
    // per-species value.
    let eye_height = if mob.mob_type == MobType::Camel {
        camel::dimensions(mob.baby, mob.camel.pose == camel::Pose::Sitting).2
    } else {
        mob.mob_type.dimensions(mob.baby).2
    };
    LivingView {
        uid,
        player: false,
        mob_type: Some(mob.mob_type),
        position,
        eye_height,
        bounding_box: Aabb::new(
            position.x - half,
            position.y,
            position.z - half,
            position.x + half,
            position.y + f64::from(height),
            position.z + half,
        ),
        alive: !removed && mob.is_alive(),
        spectator: false,
        creative: false,
        sprinting: false,
        baby: mob.baby,
        in_love: mob.in_love > 0,
        panicking: mob.panicking,
        visibility: 1.0,
    }
}

/// `Sensor.isEntityTargetable` 用的 `TARGET_CONDITIONS`（非战斗、看视线、测隐身，
/// 距离是跟随距离）。官方那个距离是全局静态的、跟着最后一个扫描的生物变；
/// 这里用自己的跟随距离。
///
/// The `TARGET_CONDITIONS` behind `Sensor.isEntityTargetable` (non-combat, line of
/// sight, invisibility test, follow range). Vanilla keeps that range in a global static
/// updated by whichever mob scanned last; Bamboo uses the mob's own follow range.
fn is_entity_targetable(cx: &mut Cx, view: &LivingView) -> bool {
    if view.uid == cx.me.id || !view.alive || view.spectator {
        return false;
    }
    let range = cx.mob.attributes.value(attribute::FOLLOW_RANGE);
    if range > 0.0 {
        let limit = mth::java_max(range * view.visibility, 2.0);
        if cx.me.position.distance_to_sqr(view.position) > limit * limit {
            return false;
        }
    }
    living::has_line_of_sight(
        cx.me,
        cx.mob,
        cx.ctx.blocks(),
        view.uid,
        view.eye_position(),
    )
}

/// `NearestVisibleLivingEntities` 的视线缓存：第一次问到某个实体时才测。
///
/// The `NearestVisibleLivingEntities` visibility cache: tested on first query.
fn visible_at(cx: &mut Cx, index: usize, view: &LivingView) -> bool {
    if let Some(MemoryValue::Visible(visible)) =
        cx.memories.get(MemoryType::NearestVisibleLivingEntities)
        && let Some(Some(cached)) = visible.visible.get(index)
    {
        return *cached;
    }
    let result = is_entity_targetable(cx, view);
    if let Some(MemoryValue::Visible(visible)) = cx
        .memories
        .get_mut(MemoryType::NearestVisibleLivingEntities)
        && let Some(slot) = visible.visible.get_mut(index)
    {
        *slot = Some(result);
    }
    result
}

fn visible_list(cx: &Cx) -> Option<Arc<[EntityUid]>> {
    match cx.memories.get(MemoryType::NearestVisibleLivingEntities) {
        Some(MemoryValue::Visible(visible)) => Some(Arc::clone(&visible.entities)),
        _ => None,
    }
}

/// `NearestVisibleLivingEntities.contains(entity)`。
///
/// `NearestVisibleLivingEntities.contains(entity)`.
pub fn visible_contains(cx: &mut Cx, uid: EntityUid) -> bool {
    let Some(list) = visible_list(cx) else {
        return false;
    };
    let Some(index) = list.iter().position(|&other| other == uid) else {
        return false;
    };
    let Some(view) = living(cx, uid) else {
        return false;
    };
    visible_at(cx, index, &view)
}

/// `NearestVisibleLivingEntities.findClosest(filter)`：先过滤再测视线，按距离顺序。
///
/// `NearestVisibleLivingEntities.findClosest(filter)`: filter first, then the
/// visibility test, in distance order.
pub fn find_closest_visible(
    cx: &mut Cx,
    filter: &mut dyn FnMut(&LivingView) -> bool,
) -> Option<EntityUid> {
    let list = visible_list(cx)?;
    for (index, uid) in list.iter().copied().enumerate() {
        let Some(view) = living(cx, uid) else {
            continue;
        };
        if filter(&view) && visible_at(cx, index, &view) {
            return Some(uid);
        }
    }
    None
}

/// 改另一只生物的记忆（`AnimalMakeLove` 给伴侣设目标）：精确模式当场落地。
///
/// Writes another mob's memory (`AnimalMakeLove` setting the partner's targets);
/// applied at once in precise mode.
pub fn set_partner_memory(
    cx: &mut Cx,
    target: EntityUid,
    memory: MemoryType,
    value: Option<MemoryValue>,
) {
    cx.ctx.emit(
        Command::SetBrainMemory {
            target,
            memory,
            value,
        },
        cx.me,
    );
}

/// `NearestLivingEntitySensor.doTick`：跟随距离方盒里活着的活体，按距离排好。
/// 玩家排在生物前面（只影响等距时的先后）。
///
/// `NearestLivingEntitySensor.doTick`: living entities in the follow-range box, by
/// distance. Players come before mobs (only matters for equal distances).
fn sense_nearest_living(cx: &mut Cx) {
    let range = cx.mob.attributes.value(attribute::FOLLOW_RANGE);
    let area = cx.me.bounding_box().inflate(range, range, range);
    let me = cx.me.id;
    let mut found: Vec<(f64, EntityUid)> = Vec::new();
    let position = cx.me.position;
    cx.ctx.blocks().for_each_player(&mut |player| {
        let view = player_view(player);
        if view.alive && view.bounding_box.intersects(area) {
            found.push((position.distance_to_sqr(view.position), view.uid));
        }
    });
    cx.ctx.for_each_other_in(area, &mut |other| {
        let KindPublic::Mob(mob) = &other.kind else {
            return;
        };
        if other.uid != me && !other.removed && mob.is_alive() {
            found.push((position.distance_to_sqr(other.position), other.uid));
        }
    });
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    // 两条记忆共用同一份实体表：一次分配，不再整表克隆一遍。
    // Both memories share one uid list: one allocation, no second full clone.
    let list: Arc<[EntityUid]> = found.into_iter().map(|(_, uid)| uid).collect();
    cx.memories.set(
        MemoryType::NearestLivingEntities,
        MemoryValue::Entities(Arc::clone(&list)),
    );
    cx.memories.set(
        MemoryType::NearestVisibleLivingEntities,
        MemoryValue::Visible(NearestVisible::new(list)),
    );
}

/// `HurtBySensor.doTick`。
///
/// `HurtBySensor.doTick`.
fn sense_hurt_by(cx: &mut Cx) {
    match living::last_damage_source(cx.mob, cx.time) {
        Some(source) => {
            cx.memories
                .set(MemoryType::HurtBy, MemoryValue::Damage(source));
            if let Some(attacker) = source.attacker
                && living(cx, attacker).is_some()
            {
                cx.memories
                    .set(MemoryType::HurtByEntity, MemoryValue::Entity(attacker));
            }
        }
        None => cx.memories.erase(MemoryType::HurtBy),
    }
    if let Some(attacker) = cx.memories.entity(MemoryType::HurtByEntity)
        && !living(cx, attacker).is_some_and(|view| view.alive)
    {
        cx.memories.erase(MemoryType::HurtByEntity);
    }
}

/// `TemptingSensor.doTick`：拿着食物的最近玩家（不看视线）。
///
/// `TemptingSensor.doTick`: the nearest player holding food (no line of sight).
fn sense_temptations(cx: &mut Cx) {
    match goals::nearest_tempting_player(cx.me, cx.mob, cx.ctx, false) {
        Some(player) => cx
            .memories
            .set(MemoryType::TemptingPlayer, MemoryValue::Entity(player)),
        None => cx.memories.erase(MemoryType::TemptingPlayer),
    }
}

/// `AdultSensor.doTick`：看得见的最近同种成年个体。
///
/// `AdultSensor.doTick`: the nearest visible adult of the same type.
fn sense_nearest_adult(cx: &mut Cx) {
    if !cx.memories.has(MemoryType::NearestVisibleLivingEntities) {
        return;
    }
    let kind = cx.mob.mob_type;
    let adult = find_closest_visible(cx, &mut |view| view.mob_type == Some(kind) && !view.baby);
    cx.memories.set_or_erase(
        MemoryType::NearestVisibleAdult,
        adult.map(MemoryValue::Entity),
    );
}

/// `PlayerSensor.doTick`：跟随距离内的非旁观玩家按距离排好，再依次筛出看得见的和
/// 能当敌人的。
///
/// `PlayerSensor.doTick`: non-spectator players within the follow range by distance, then
/// the visible ones and the attackable ones.
fn sense_players(cx: &mut Cx) {
    let range = cx.mob.attributes.value(attribute::FOLLOW_RANGE);
    let position = cx.me.position;
    let mut players: Vec<LivingView> = Vec::new();
    cx.ctx.blocks().for_each_player(&mut |player| {
        let view = player_view(player);
        if !view.spectator && position.distance_to_sqr(view.position) < range * range {
            players.push(view);
        }
    });
    players.sort_by(|a, b| {
        position
            .distance_to_sqr(a.position)
            .total_cmp(&position.distance_to_sqr(b.position))
    });
    let ids =
        |list: &[LivingView]| -> Arc<[EntityUid]> { list.iter().map(|view| view.uid).collect() };
    cx.memories.set(
        MemoryType::NearestPlayers,
        MemoryValue::Entities(ids(&players)),
    );
    players.retain(|view| is_entity_targetable(cx, view));
    cx.memories.set_or_erase(
        MemoryType::NearestVisiblePlayer,
        players.first().map(|view| MemoryValue::Entity(view.uid)),
    );
    // `isEntityAttackable`：`forCombat` 多一条 `canAttack`——和平难度不打玩家，创造玩家不算敌人。
    // `isEntityAttackable`: `forCombat` adds `canAttack` — no players on peaceful, and
    // creative players are not enemies.
    let peaceful = cx.ctx.blocks().difficulty() == 0;
    players.retain(|view| !peaceful && !view.creative);
    cx.memories.set(
        MemoryType::NearestVisibleAttackablePlayers,
        MemoryValue::Entities(ids(&players)),
    );
    cx.memories.set_or_erase(
        MemoryType::NearestVisibleAttackablePlayer,
        players.first().map(|view| MemoryValue::Entity(view.uid)),
    );
}

/// `EntityTypes.ITEM` 的眼高。
///
/// The eye height of `EntityTypes.ITEM`.
const ITEM_EYE_HEIGHT: f32 = 0.2125;

/// `NearestItemSensor.doTick`：32×16×32 里的掉落物按距离排好，取第一个想捡
/// （`Mob.wantsToPickUp` 缺省恒真）、32 格内、看得见的。
///
/// `NearestItemSensor.doTick`: dropped items within 32×16×32 by distance; the first that
/// is wanted (`Mob.wantsToPickUp` defaults to true), within 32 blocks and in sight.
fn sense_items(cx: &mut Cx) {
    let area = cx.me.bounding_box().inflate(32.0, 16.0, 32.0);
    let position = cx.me.position;
    let mut items: Vec<(f64, EntityUid, Vec3)> = Vec::new();
    cx.ctx.for_each_other_in(area, &mut |other| {
        if matches!(other.kind, KindPublic::Item { .. }) && !other.removed {
            items.push((
                position.distance_to_sqr(other.position),
                other.uid,
                other.position,
            ));
        }
    });
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    let eye = Vec3::new(position.x, living::eye_y(cx.me, cx.mob), position.z);
    let found = items.into_iter().find(|&(distance, _, at)| {
        if distance >= 32.0 * 32.0 {
            return false;
        }
        // `LivingEntity.hasLineOfSight(Entity)`：不走 `Sensing` 的缓存。
        // `LivingEntity.hasLineOfSight(Entity)`, which bypasses the `Sensing` cache.
        let to = Vec3::new(at.x, at.y + f64::from(ITEM_EYE_HEIGHT), at.z);
        to.distance_to_sqr(eye).sqrt() <= 128.0
            && cx
                .ctx
                .blocks()
                .clip_collider(eye, to, cx.me.mob_collision_context(cx.mob))
                .is_none()
    });
    cx.memories.set_or_erase(
        MemoryType::NearestVisibleWantedItem,
        found.map(|(_, uid, _)| MemoryValue::Entity(uid)),
    );
}
