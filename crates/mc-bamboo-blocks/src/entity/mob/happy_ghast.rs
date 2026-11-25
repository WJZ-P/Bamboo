//! 快乐恶魂：官方 `HappyGhast` / `HappyGhastAi` 和干恶魂方块孵出来的幼崽。
//!
//! 幼年用大脑（诱惑、跟成年、飞行闲逛），成年用目标选择器；套上 BODY 槽的挽具后
//! 能载最多四名玩家，第一名玩家乘客控制飞行。四点拴绳由 `Leashable` 的共享附着点
//! 和快乐恶魂自己的 holder 附着点一起算。
//!
//! Happy ghasts: vanilla `HappyGhast` / `HappyGhastAi` and the ghastling hatched from a
//! dried ghast block. Babies run a brain (temptation, adult following, flying strolls);
//! adults use goals. A harness in the BODY slot lets up to four players ride, with the
//! first player passenger steering. Quad leashes use `Leashable`'s shared attachment
//! points plus the happy ghast's own holder offsets.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{items, registries::attribute, tags::item_tags};
use mc_bamboo_protocol::item::ItemStack;
use mc_bamboo_protocol::packet::play::EquipmentSlot;

use super::{
    Attributes, Mob, MobEvent, MobType,
    attributes::Attribute,
    brain::{
        Activity, ActivityData, Brain, BrainTemplate, TemplateCache,
        behavior::{Gate, Kind, MoveToTargetSink, Speed},
        memory::{MemoryStatus, MemoryType},
        sensor::SensorKind,
        template_cache,
    },
    interact::{MobInteraction, MobInteractRequest},
    living,
};
use crate::entity::{Entity, EntityUid, ctx::EntityCtx};
use crate::{InteractionResult, state::item_is_tagged};

/// `EntityTypes.HAPPY_GHAST`：`sized(4,4).eyeHeight(2.6)`。
/// `BABY_DIMENSIONS` 是成年尺寸 `scale(0.2375).withEyeHeight(0.46875)`。
///
/// `EntityTypes.HAPPY_GHAST`: `sized(4,4).eyeHeight(2.6)`.
/// `BABY_DIMENSIONS` is the adult size `scale(0.2375).withEyeHeight(0.46875)`.
pub(super) fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (0.95, 0.95, 0.46875)
    } else {
        (4.0, 4.0, 2.6)
    }
}

/// `HappyGhast.createAttributes()`：`Animal.createAnimalAttributes` 的覆盖值。
///
/// `HappyGhast.createAttributes()`: the overrides on `Animal.createAnimalAttributes`.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    table.set_base(attribute::MAX_HEALTH, 20.0);
    table.set_base(attribute::TEMPT_RANGE, 16.0);
    table.set_base(attribute::MOVEMENT_SPEED, 0.05);
    table.set_base(attribute::FOLLOW_RANGE, 16.0);
    table.set_base(attribute::CAMERA_DISTANCE, 8.0);
    table
        .entries
        .push(Attribute::new(attribute::FLYING_SPEED, 0.05));
    table
}

/// `HappyGhast` 自己的两个私有计数器和两项同步布尔。
///
/// The happy ghast's two private counters and two synced booleans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HappyGhastState {
    /// `leashHolderTime`：别的生物拴到它身上后留 5 刻的「我是牵引者」标志。
    ///
    /// `leashHolderTime`: five ticks of the "I am the holder" flag after another mob
    /// is leashed to it.
    pub leash_holder_time: i32,
    /// `serverStillTimeout`：头顶有玩家时悬停的超时，存档键 `still_timeout`。
    ///
    /// `serverStillTimeout`: the hover timeout while a player stands above it;
    /// saved as `still_timeout`.
    pub still_timeout: i32,
    /// `IS_LEASH_HOLDER` 元数据（index 18，BOOLEAN，缺省假）。
    ///
    /// `IS_LEASH_HOLDER` metadata (index 18, BOOLEAN, false by default).
    pub is_leash_holder: bool,
    /// `STAYS_STILL` 元数据（index 19，BOOLEAN，缺省假）。
    ///
    /// `STAYS_STILL` metadata (index 19, BOOLEAN, false by default).
    pub stays_still: bool,
}

impl HappyGhastState {
    /// `setServerStillTimeout(n)`：把同步标志一起刷新。
    ///
    /// `setServerStillTimeout(n)`: refresh the synced flag with it.
    pub fn set_still_timeout(&mut self, ticks: i32) {
        self.still_timeout = ticks;
        self.stays_still = ticks > 0;
    }

    /// `isOnStillTimeout()`：同步标志或服务端计数任一为正。
    ///
    /// `isOnStillTimeout()`: either the synced flag or the server counter is set.
    pub const fn is_on_still_timeout(self) -> bool {
        self.stays_still || self.still_timeout > 0
    }
}

/// `HappyGhast.MAX_PASSANGERS`。
pub const MAX_PASSENGERS: usize = 4;
/// `HappyGhast.MAX_STILL_TIMEOUT`。
pub const MAX_STILL_TIMEOUT: i32 = 10;
/// `HappyGhast.STILL_TIMEOUT_ON_LOAD_GRACE_PERIOD`。
pub const STILL_TIMEOUT_ON_LOAD_GRACE_PERIOD: i32 = 60;
/// `HappyGhast.SMALL_RESTRICTION_RADIUS` / `LARGE_RESTRICTION_RADIUS`。
pub const SMALL_RESTRICTION_RADIUS: i32 = 32;
pub const LARGE_RESTRICTION_RADIUS: i32 = 64;
/// `HappyGhast.RESTRICTION_RADIUS_BUFFER`。
pub const RESTRICTION_RADIUS_BUFFER: i32 = 16;
/// `HappyGhast.FAST_HEALING_TICKS` / `SLOW_HEALING_TICKS`。
pub const FAST_HEALING_TICKS: i32 = 20;
pub const SLOW_HEALING_TICKS: i32 = 600;

/// `EntityTypes.HAPPY_GHAST` 注册的四个 `PASSENGER` 附着点。
///
/// The four `PASSENGER` attachment points registered for `EntityTypes.HAPPY_GHAST`.
pub const PASSENGER_ATTACHMENTS: [Vec3; 4] = [
    Vec3::new(0.0, 4.0, 1.7),
    Vec3::new(-1.7, 4.0, 0.0),
    Vec3::new(0.0, 4.0, -1.7),
    Vec3::new(1.7, 4.0, 0.0),
];

/// BODY 槽里有没有挽具（`isWearingBodyArmor`）。
///
/// Whether the BODY slot holds a harness (`isWearingBodyArmor`).
pub fn is_wearing_harness(mob: &Mob) -> bool {
    !mob.equipment.get(EquipmentSlot::Body).is_empty()
}

/// 十六种挽具物品：从 `minecraft:white_harness` 起连续 16 个 ID。
///
/// The sixteen harness items: sixteen consecutive IDs from `minecraft:white_harness`.
pub const FIRST_HARNESS: i32 = items::WHITE_HARNESS;
pub const HARNESS_COUNT: i32 = 16;

/// 这个物品是不是挽具（`ItemTags.HARNESSES` 的数据表就是这 16 项）。
///
/// Whether an item is a harness (`ItemTags.HARNESSES` is exactly these sixteen items).
pub const fn is_harness(item: i32) -> bool {
    item >= FIRST_HARNESS && item < FIRST_HARNESS + HARNESS_COUNT
}

/// `HappyGhast.canUseSlot(BODY)`：只有活着且成年的快乐恶魂能穿挽具。
///
/// `HappyGhast.canUseSlot(BODY)`: only a living adult happy ghast can wear a harness.
pub fn can_equip_harness(mob: &Mob) -> bool {
    mob.mob_type == MobType::HappyGhast && !mob.dead && !mob.is_baby()
}

/// `EntityTypes.HAPPY_GHAST` 的四个乘客点按索引 clamp。
///
/// The four passenger points of `EntityTypes.HAPPY_GHAST`, clamped by index.
pub fn passenger_attachment(index: usize) -> Vec3 {
    PASSENGER_ATTACHMENTS[index.min(PASSENGER_ATTACHMENTS.len() - 1)]
}

/// `Leashable.createQuadLeashOffsets(e,a,b,c,d)`：
/// `A=a*w, B=b*w, C=c*w, D=d*h`，返回四个角
/// `[(-C,D,B+A),(-C,D,-B+A),(C,D,-B+A),(C,D,B+A)]`。
///
/// `Leashable.createQuadLeashOffsets(e,a,b,c,d)`: with
/// `A=a*w, B=b*w, C=c*w, D=d*h`, the four corners are
/// `[(-C,D,B+A),(-C,D,-B+A),(C,D,-B+A),(C,D,B+A)]`.
pub fn create_quad_leash_offsets(
    width: f32,
    height: f32,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
) -> [Vec3; 4] {
    let w = f64::from(width);
    let h = f64::from(height);
    let (big_a, big_b, big_c, big_d) = (a * w, b * w, c * w, d * h);
    [
        Vec3::new(-big_c, big_d, big_b + big_a),
        Vec3::new(-big_c, big_d, -big_b + big_a),
        Vec3::new(big_c, big_d, -big_b + big_a),
        Vec3::new(big_c, big_d, big_b + big_a),
    ]
}

/// `HappyGhast.getQuadLeashHolderOffsets()`：成年 4×4 时的四个角。
///
/// `HappyGhast.getQuadLeashHolderOffsets()`: the four corners for the 4x4 adult.
pub fn quad_holder_offsets(width: f32, height: f32) -> [Vec3; 4] {
    create_quad_leash_offsets(width, height, -0.03125, 0.4375, 0.46875, 0.03125)
}

/// `Leashable.SHARED_QUAD_ATTACHMENT_POINTS`：按宽 / 高 / 宽归一化的四角。
///
/// `Leashable.SHARED_QUAD_ATTACHMENT_POINTS`: four corners normalised by width /
/// height / width.
pub const SHARED_QUAD_ATTACHMENT_POINTS: [Vec3; 4] = [
    Vec3::new(-0.5, 0.5, 0.5),
    Vec3::new(-0.5, 0.5, -0.5),
    Vec3::new(0.5, 0.5, -0.5),
    Vec3::new(0.5, 0.5, 0.5),
];

/// `Leashable.ENTITY_ATTACHMENT_POINT`（单点，非四点时用）。
///
/// `Leashable.ENTITY_ATTACHMENT_POINT` (the single point used outside quad leashes).
pub const ENTITY_ATTACHMENT_POINT: Vec3 = Vec3::new(0.0, 0.5, 0.5);
/// `Leashable.LEASHER_ATTACHMENT_POINT`。
pub const LEASHER_ATTACHMENT_POINT: Vec3 = Vec3::new(0.0, 0.5, 0.0);

/// 快乐恶魂的弹性 / 断裂距离（`leashElasticDistance` / `leashSnapDistance` 覆盖）。
///
/// The happy ghast's elastic / snap distance overrides.
pub const LEASH_ELASTIC_DISTANCE: f64 = 10.0;
pub const LEASH_SNAP_DISTANCE: f64 = 16.0;

/// `HappyGhast.BRAIN_PROVIDER`：五个传感器和 `HappyGhastAi.getActivities()` 的三张活动表。
///
/// `HappyGhast.BRAIN_PROVIDER`: five sensors and `HappyGhastAi.getActivities()`'s
/// three activity tables.
pub(in crate::entity::mob) fn brain() -> Brain {
    static TEMPLATE: TemplateCache = template_cache();
    Brain::cached(&TEMPLATE, || {
        BrainTemplate::new(
            &[
                SensorKind::NearestLivingEntities,
                SensorKind::HurtBy,
                SensorKind::FoodTemptations,
                SensorKind::NearestAdult,
                SensorKind::NearestPlayers,
            ],
            vec![core_activity(), idle_activity(), panic_activity()],
        )
    })
}

/// `HappyGhastAi.initCoreActivity`：优先级从 0 起，五个行为。
///
/// `HappyGhastAi.initCoreActivity`: five behaviours starting at priority 0.
fn core_activity() -> ActivityData {
    const SPEED_PANIC: f32 = 2.0;
    ActivityData::sequential(
        Activity::Core,
        0,
        vec![
            Kind::Swim { chance: 0.8 },
            Kind::AnimalPanic {
                speed: SPEED_PANIC,
                environmental: false,
                armadillo: false,
                camel: false,
            },
            Kind::LookAtTargetSink { min: 45, max: 90 },
            Kind::MoveToTargetSink(MoveToTargetSink::new(false)),
            Kind::CountDownCooldown(MemoryType::TemptationCooldownTicks),
        ],
    )
}

/// `HappyGhastAi.initIdleActivity`：诱惑、两种跟成年、飞行闲逛。
///
/// `HappyGhastAi.initIdleActivity`: temptation, two adult-follow variants, flying stroll.
fn idle_activity() -> ActivityData {
    const SPEED_TEMPTED: f32 = 1.25;
    const SPEED_FOLLOWING_ADULT: f32 = 1.1;
    const BABY_CLOSE_ENOUGH_DIST: f64 = 3.0;
    const ADULT_FOLLOW_MIN: i32 = 3;
    const ADULT_FOLLOW_MAX: i32 = 16;
    ActivityData::with_priorities(
        Activity::Idle,
        vec![
            (
                1,
                Kind::FollowTemptation {
                    speed: Speed::Fixed(SPEED_TEMPTED),
                    close_adult: BABY_CLOSE_ENOUGH_DIST,
                    close_baby: BABY_CLOSE_ENOUGH_DIST,
                },
            ),
            (
                2,
                Kind::BabyFollowAdult {
                    min: ADULT_FOLLOW_MIN,
                    max: ADULT_FOLLOW_MAX,
                    speed: Speed::Fixed(SPEED_FOLLOWING_ADULT),
                },
            ),
            (
                3,
                Kind::BabyFollowAdult {
                    min: ADULT_FOLLOW_MIN,
                    max: ADULT_FOLLOW_MAX,
                    speed: Speed::Fixed(SPEED_FOLLOWING_ADULT),
                },
            ),
            (
                4,
                Kind::Gate(Box::new(Gate::run_one(
                    Vec::new(),
                    vec![
                        (Kind::RandomStroll { speed: 1.0 }, 1),
                        (
                            Kind::WalkTargetFromLook {
                                speed: Speed::Fixed(1.0),
                                close_enough: 3,
                            },
                            1,
                        ),
                    ],
                ))),
            ),
        ],
    )
}

/// `HappyGhastAi.initPanicActivity`：没有行为，入口条件 `IS_PANICKING`。
///
/// `HappyGhastAi.initPanicActivity`: no behaviours, gated on `IS_PANICKING`.
fn panic_activity() -> ActivityData {
    ActivityData::with_priorities(Activity::Panic, Vec::new())
        .when(vec![(MemoryType::IsPanicking, MemoryStatus::ValuePresent)])
}

/// `HappyGhast.customServerAiStep`：只有幼年跑大脑，随后 `checkRestriction()`。
///
/// `HappyGhast.customServerAiStep`: only babies tick the brain, then
/// `checkRestriction()` runs for both ages.
pub(in crate::entity::mob) fn custom_server_ai_step(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
) {
    if mob.is_baby()
        && let Some(mut brain) = mob.brain.take()
    {
        let gap = ctx.throttle_plan().decision_gap;
        brain.tick_by(me, mob, ctx, gap);
        brain.set_active_activity_to_first_valid(&[Activity::Panic, Activity::Idle]);
        mob.brain = Some(brain);
    }
    check_restriction(me, mob);
}

/// `HappyGhast.checkRestriction()`：被拴或载人时不改家；否则把家设到当前方块，
/// 半径按「成年且穿了挽具」取 64，其余 32。
///
/// `HappyGhast.checkRestriction()`: leashed or ridden mobs keep their home; otherwise
/// the home is set to the current block with radius 64 for a harnessed adult, 32 else.
fn check_restriction(me: &Entity, mob: &mut Mob) {
    if super::leash::is_leashed(mob) || !mob.passengers.is_empty() {
        return;
    }
    let radius = restriction_radius(mob);
    let pos = me.block_position();
    let limit = f64::from(radius + RESTRICTION_RADIUS_BUFFER);
    if let Some((home, home_radius)) = mob.home
        && home_dist_sqr(home, pos) <= limit * limit
        && home_radius == radius
    {
        return;
    }
    mob.home = Some((pos, radius));
}

/// `HappyGhast.getHappyGhastRestrictionRadius()`。
pub fn restriction_radius(mob: &Mob) -> i32 {
    if !mob.is_baby() && is_wearing_harness(mob) {
        LARGE_RESTRICTION_RADIUS
    } else {
        SMALL_RESTRICTION_RADIUS
    }
}

/// `HappyGhast.tick` 的服务端尾部：`leashHolderTime` 倒数、悬停超时倒数、
/// 扫描头顶玩家。`me.tick_count` 是官方 `Entity.tickCount`（`aiStep` 里已自增）。
///
/// The server tail of `HappyGhast.tick`: count down `leashHolderTime` and the hover
/// timeout and scan for players above. `me.tick_count` is `Entity.tickCount`.
pub(in crate::entity::mob) fn tick_end(
    me: &Entity,
    mob: &mut Mob,
    player_above: bool,
) {
    let state = &mut mob.happy_ghast;
    if state.leash_holder_time > 0 {
        state.leash_holder_time -= 1;
    }
    state.is_leash_holder = state.leash_holder_time > 0;
    if state.still_timeout > 0 {
        if me.tick_count > STILL_TIMEOUT_ON_LOAD_GRACE_PERIOD {
            state.still_timeout -= 1;
        }
        state.set_still_timeout(state.still_timeout);
    }
    if player_above {
        state.set_still_timeout(MAX_STILL_TIMEOUT);
    }
}

/// `HappyGhast.notifyLeashHolder(leashable)`：被拴的一方支持四点拴绳时记 5 刻。
///
/// `HappyGhast.notifyLeashHolder(leashable)`: five ticks when the leashed side
/// supports quad leashes.
pub fn notify_leash_holder(mob: &mut Mob, leashed_supports_quad: bool) {
    if leashed_supports_quad {
        mob.happy_ghast.leash_holder_time = 5;
    }
}

/// `HappyGhast.continuousHeal()`：不满血、活着、没在死亡动画时，云里或降水群系
/// 每 20 刻回 1 点，否则每 600 刻回 1 点。
///
/// `HappyGhast.continuousHeal()`: while alive, not dying and hurt, heal 1 point every
/// 20 ticks in clouds or precipitating biomes, or every 600 ticks otherwise.
pub(in crate::entity::mob) fn continuous_heal(
    me: &Entity,
    mob: &mut Mob,
    fast: bool,
) {
    if mob.dead || mob.death_time != 0 {
        return;
    }
    let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
    if !(max > mob.health) {
        return;
    }
    let period = if fast { FAST_HEALING_TICKS } else { SLOW_HEALING_TICKS };
    if me.tick_count % period == 0 {
        mob.health = (mob.health + 1.0).min(max);
    }
}

/// `HappyGhast.mobInteract`：幼年走 `Animal.mobInteract`（喂雪球加速长大），成年按
/// 挽具物品装上 / 剪刀脱下 / 空手且穿了挽具就上车 / 其余交给 `Animal.mobInteract`。
///
/// `HappyGhast.mobInteract`: babies use `Animal.mobInteract` (snowballs speed up growth);
/// adults equip a harness, shear it off, ride when bare-handed and harnessed, or fall
/// through to `Animal.mobInteract`.
pub fn interact(
    me: &mut Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
    level_next_float: &mut dyn FnMut() -> f32,
) -> MobInteraction {
    if mob.is_baby() {
        return super::interact::animal_interact(me, mob, req, level_next_float);
    }
    // `ItemStack.interactLivingEntity` 的 equippable 组件：挽具穿上或剪刀脱下都由物品处理。
    // The equippable component of `ItemStack.interactLivingEntity`: equipping a harness
    // and shearing one off are both handled by the item.
    if !req.held.is_empty() {
        if is_harness(req.held.item) && can_equip_harness(mob) {
            return equip_harness(me, mob, req);
        }
        if req.held.item == items::SHEARS
            && is_wearing_harness(mob)
            && let Some(outcome) = shear_harness(me, mob, req)
        {
            return outcome;
        }
    }
    if is_wearing_harness(mob) && !req.sneaking {
        return MobInteraction {
            result: InteractionResult::Success,
            ride: true,
            ..MobInteraction::pass()
        };
    }
    super::interact::animal_interact(me, mob, req, level_next_float)
}

/// `Equippable.equipOnInteract`：装进 BODY 槽、消费一个、放装备音。
///
/// `Equippable.equipOnInteract`: put it in the BODY slot, consume one, play the equip sound.
fn equip_harness(
    me: &mut Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
) -> MobInteraction {
    // 官方 `Equippable.equipOnInteract` 在目标槽非空时换下旧物品；这里同时保留
    // 生存模式消费一个手里的挽具。
    // Vanilla `Equippable.equipOnInteract` swaps out the old item when the slot is
    // occupied; survival mode also consumes one harness from the hand.
    let worn = mob.equipment.get(EquipmentSlot::Body).clone();
    mob.equipment
        .set(EquipmentSlot::Body, ItemStack::new(req.held.item, 1));
    living::play_sound(
        me,
        mob,
        "minecraft:entity.happy_ghast.equip",
        sound_volume(mob),
        1.0,
    );
    MobInteraction {
        result: InteractionResult::Success,
        shrink_held: !req.creative,
        drop_stack: (!worn.is_empty()).then_some(worn),
        ..MobInteraction::pass()
    }
}

/// 剪刀脱下挽具：BODY 槽清空、放 unequip 音、把原物品丢出来、扣一点耐久。
///
/// Shear a harness off: clear BODY, play the unequip sound, drop the stack and spend
/// one durability point.
fn shear_harness(
    me: &mut Entity,
    mob: &mut Mob,
    _req: &MobInteractRequest,
) -> Option<MobInteraction> {
    let worn = mob.equipment.get(EquipmentSlot::Body).clone();
    if worn.is_empty() {
        return None;
    }
    mob.equipment.set(EquipmentSlot::Body, ItemStack::empty());
    living::play_sound(
        me,
        mob,
        "minecraft:entity.happy_ghast.unequip",
        sound_volume(mob),
        1.0,
    );
    Some(MobInteraction {
        result: InteractionResult::Success,
        damage_held: true,
        drop_stack: Some(worn),
        drop_offset: average_passenger_attachment(),
        ..MobInteraction::pass()
    })
}

/// `getPassengerRidingPosition` 的四个点均值（剪马具时掉落物的偏移）。
///
/// The mean of the four passenger points (drop offset when shearing equipment off).
fn average_passenger_attachment() -> Vec3 {
    let mut out = Vec3::default();
    for point in PASSENGER_ATTACHMENTS {
        out = out.add(point);
    }
    out.scale(0.25)
}

/// `HappyGhast.getSoundVolume()`：幼年 1、成年 4。
///
/// `HappyGhast.getSoundVolume()`: 1 for babies, 4 for adults.
pub fn sound_volume(mob: &Mob) -> f32 {
    if mob.is_baby() { 1.0 } else { 4.0 }
}

/// `HappyGhast.getAmbientSoundInterval()`：有人在骑时是 `Animal` 的六倍。
///
/// `HappyGhast.getAmbientSoundInterval()`: six times the animal interval while ridden.
pub fn ambient_sound_interval(mob: &Mob) -> i32 {
    let base = MobType::HappyGhast.ambient_sound_interval();
    if mob.passengers.is_empty() {
        base
    } else {
        base * 6
    }
}

/// `HappyGhast.getVoicePitch()`：恒 1.0，不取随机数。
///
/// `HappyGhast.getVoicePitch()`: always 1.0 and draws no random numbers.
pub const VOICE_PITCH: f32 = 1.0;

/// 幼年 / 成年四种声音的分派（环境、受伤、死亡）。
///
/// Baby / adult sound variants (ambient, hurt and death).
pub fn voice(kind: Voice, baby: bool) -> &'static str {
    match (kind, baby) {
        (Voice::Ambient, true) => "minecraft:entity.ghastling.ambient",
        (Voice::Ambient, false) => "minecraft:entity.happy_ghast.ambient",
        (Voice::Hurt, true) => "minecraft:entity.ghastling.hurt",
        (Voice::Hurt, false) => "minecraft:entity.happy_ghast.hurt",
        (Voice::Death, true) => "minecraft:entity.ghastling.death",
        (Voice::Death, false) => "minecraft:entity.happy_ghast.death",
        (Voice::Spawn, _) => "minecraft:entity.ghastling.spawn",
        (Voice::GogglesDown, _) => "minecraft:entity.happy_ghast.harness_goggles_down",
        (Voice::GogglesUp, _) => "minecraft:entity.happy_ghast.harness_goggles_up",
    }
}

/// 快乐恶魂用到的声音种类。
///
/// The sound kinds a happy ghast uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    Ambient,
    Hurt,
    Death,
    Spawn,
    GogglesDown,
    GogglesUp,
}

/// `HappyGhast.addPassenger` / `removePassenger` 的护目镜音效和悬停超时。
///
/// The goggle sounds and hover timeout of `HappyGhast.addPassenger` / `removePassenger`.
pub fn after_add_passenger(
    me: &mut Entity,
    mob: &mut Mob,
    was_vehicle: bool,
    player_above: bool,
) {
    if !was_vehicle {
        living::play_sound(
            me,
            mob,
            voice(Voice::GogglesDown, false),
            sound_volume(mob),
            1.0,
        );
    }
    if !player_above {
        mob.happy_ghast.set_still_timeout(0);
    } else if mob.happy_ghast.still_timeout > MAX_STILL_TIMEOUT {
        mob.happy_ghast.set_still_timeout(MAX_STILL_TIMEOUT);
    }
}

/// `HappyGhast.removePassenger`：设 10 刻悬停，没人时清家并放护目镜抬起声。
///
/// `HappyGhast.removePassenger`: ten ticks of hover, and when empty clear the home
/// and play the goggle-up sound.
pub fn after_remove_passenger(me: &mut Entity, mob: &mut Mob) {
    mob.happy_ghast.set_still_timeout(MAX_STILL_TIMEOUT);
    if mob.passengers.is_empty() {
        mob.home = None;
        living::play_sound(
            me,
            mob,
            voice(Voice::GogglesUp, false),
            sound_volume(mob),
            1.0,
        );
    }
}

/// `HappyGhast.canAddPassenger`：最多四名。
///
/// `HappyGhast.canAddPassenger`: at most four.
pub const fn can_add_passenger(count: usize) -> bool {
    count < MAX_PASSENGERS
}

/// `HappyGhast.getControllingPassenger()`：穿了挽具、不在悬停超时、第一名乘客是玩家。
///
/// `HappyGhast.getControllingPassenger()`: harnessed, not in the hover timeout, and
/// the first passenger is a player.
pub fn controls(mob: &Mob, first: Option<EntityUid>) -> bool {
    is_wearing_harness(mob)
        && !mob.happy_ghast.is_on_still_timeout()
        && first.is_some_and(|uid| uid < crate::entity::FIRST_ENTITY_ID)
}

/// `HappyGhast.getRiddenInput(player, input)` 的飞行向量：
/// `Vec3(xxa, y, z).scale(3.9000000953674316 * FLYING_SPEED)`。
///
/// `HappyGhast.getRiddenInput(player, input)`'s flight vector:
/// `Vec3(xxa, y, z).scale(3.9000000953674316 * FLYING_SPEED)`.
pub fn ridden_input(input: RiddenInput, flying_speed: f64) -> Vec3 {
    let mut y = 0.0_f32;
    let mut z = 0.0_f32;
    if input.forward != 0.0 {
        let radians = f64::from(input.pitch * 0.017_453_292_f32);
        let mut cos_x = mth::cos(radians) as f32;
        let mut sin_x = -mth::sin(radians) as f32;
        if input.forward < 0.0 {
            cos_x *= -0.5;
            sin_x *= -0.5;
        }
        y = sin_x;
        z = cos_x;
    }
    if input.jumping {
        y += 0.5;
    }
    Vec3::new(
        f64::from(input.strafe),
        f64::from(y),
        f64::from(z),
    )
    .scale(3.900_000_095_367_431_6 * flying_speed)
}

/// 服务端算骑乘输入时用到的玩家按键 / 朝向快照。
///
/// The player's keys / facing snapshot used to compute the ridden input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiddenInput {
    pub strafe: f32,
    pub forward: f32,
    pub pitch: f32,
    pub jumping: bool,
}

/// `HappyGhast.getRiddenRotation(player)`：`(xRot * 0.5, yRot)`。
///
/// `HappyGhast.getRiddenRotation(player)`: `(xRot * 0.5, yRot)`.
pub const fn ridden_rotation(pitch: f32, yaw: f32) -> (f32, f32) {
    (pitch * 0.5, yaw)
}

/// `HappyGhast.tickRidden`：朝向按 0.08 插值，头 / 身 / 上一刻朝向都跟着设。
///
/// `HappyGhast.tickRidden`: lerp the facing by 0.08, then copy it into the head,
/// body and previous yaw.
pub fn tick_ridden(me: &mut Entity, mob: &mut Mob, pitch: f32, yaw: f32) {
    let (target_pitch, target_yaw) = ridden_rotation(pitch, yaw);
    let current = me.y_rot;
    let delta = mth::wrap_degrees(target_yaw - current);
    let new_yaw = current + delta * 0.08;
    me.y_rot = new_yaw;
    mob.x_rot = target_pitch;
    mob.y_head_rot = new_yaw;
    mob.y_body_rot = new_yaw;
    mob.y_rot_o = new_yaw;
}

/// `HappyGhast.scanPlayerAboveGhast()` 的判定盒（x/z 各扩 1，y 从盒顶到顶 + 半高）。
///
/// `HappyGhast.scanPlayerAboveGhast()`'s box (x/z expanded by one, y from the top
/// of the bounding box to the top plus half its height).
pub fn player_above_box(
    position: Vec3,
    width: f32,
    height: f32,
) -> (Vec3, Vec3) {
    let half = f64::from(width) / 2.0;
    let top = position.y + f64::from(height);
    (
        Vec3::new(
            position.x - half - 1.0,
            top - 0.00001,
            position.z - half - 1.0,
        ),
        Vec3::new(
            position.x + half + 1.0,
            top + f64::from(height) / 2.0,
            position.z + half + 1.0,
        ),
    )
}

/// 玩家在不在悬停扫描盒里（调用方负责排除旁观者、坐在快乐恶魂身上的玩家）。
///
/// Whether a player is inside the hover scan box (the caller excludes spectators
/// and players already riding a happy ghast).
pub fn is_above(root_position: Vec3, min: Vec3, max: Vec3) -> bool {
    root_position.x >= min.x
        && root_position.x <= max.x
        && root_position.y >= min.y
        && root_position.y <= max.y
        && root_position.z >= min.z
        && root_position.z <= max.z
}

/// `HappyGhast.getWalkTargetValue`：方块非空 0；下方空且下下方非空 10；其余 5。
///
/// `HappyGhast.getWalkTargetValue`: 0 for solid cells, 10 for an empty cell with a
/// solid cell two below, and 5 otherwise.
pub fn walk_target_value(
    empty: bool,
    empty_below: bool,
    solid_two_below: bool,
) -> f32 {
    if !empty {
        0.0
    } else if empty_below && solid_two_below {
        10.0
    } else {
        5.0
    }
}

/// `HappyGhast.travel`：`FLYING_SPEED * 5 / 3` 传给 `travelFlying`。
///
/// `HappyGhast.travel`: `FLYING_SPEED * 5 / 3` goes to `travelFlying`.
pub const fn travel_speed(flying_speed: f32) -> f32 {
    flying_speed * 5.0 / 3.0
}

/// `HappyGhast.isFood`：`#happy_ghast_food`（雪球）。
///
/// `HappyGhast.isFood`: `#happy_ghast_food` (snowballs).
pub fn is_food(item: i32) -> bool {
    item_is_tagged(item, item_tags::HAPPY_GHAST_FOOD)
}

/// `HappyGhast` 的成年诱惑物品（`#happy_ghast_tempt_items`）和幼年雪球的区分。
///
/// Adult temptation items (`#happy_ghast_tempt_items`) vs baby snowballs.
pub const fn tempt_item_tag(harnessed: bool) -> mc_bamboo_data::ItemTag {
    if harnessed {
        item_tags::HAPPY_GHAST_TEMPT_ITEMS
    } else {
        item_tags::HAPPY_GHAST_FOOD
    }
}

/// `HappyGhast.canFallInLove()` 恒假：不繁殖、不求爱。
///
/// `HappyGhast.canFallInLove()` is always false: no breeding, no love mode.
pub const CAN_FALL_IN_LOVE: bool = false;

/// `HappyGhast.getMaxSpawnClusterSize()`。
pub const MAX_SPAWN_CLUSTER_SIZE: i32 = 1;

/// `HappyGhast.getAgeScale()`：幼年 0.2375（不是通常的 0.5）。
///
/// `HappyGhast.getAgeScale()`: 0.2375 for babies (not the usual 0.5).
pub const fn age_scale(baby: bool) -> f32 {
    if baby { 0.2375 } else { 1.0 }
}

/// 存档键 `still_timeout`。
///
/// The `still_timeout` save key.
pub const STILL_TIMEOUT_KEY: &str = "still_timeout";

/// `HappyGhast.supportQuadLeashAsHolder()` 恒真；
/// `Leashable.supportQuadLeash()` 默认假（快乐恶魂没有覆盖）。
///
/// `HappyGhast.supportQuadLeashAsHolder()` is true; `Leashable.supportQuadLeash()`
/// stays false (happy ghasts do not override it).
pub const SUPPORTS_QUAD_AS_HOLDER: bool = true;
pub const SUPPORTS_QUAD_AS_LEASHED: bool = false;

/// `HappyGhast.getLeashOffset()` = Vec3.ZERO。
///
/// `HappyGhast.getLeashOffset()` = Vec3.ZERO.
pub const LEASH_OFFSET: Vec3 = Vec3::new(0.0, 0.0, 0.0);

/// `HappyGhast.getDismountLocationForPassenger` = `(x, bb.maxY, z)`。
///
/// `HappyGhast.getDismountLocationForPassenger` = `(x, bb.maxY, z)`.
pub fn dismount_location(position: Vec3, height: f32) -> Vec3 {
    Vec3::new(position.x, position.y + f64::from(height), position.z)
}

/// `HappyGhast.isFlyingVehicle()` = `!isBaby`。
///
/// `HappyGhast.isFlyingVehicle()` = `!isBaby`.
pub const fn is_flying_vehicle(baby: bool) -> bool {
    !baby
}

/// 幼年的 BODY 槽不能穿挽具，交互里直接跳过；这里只保留事件包装。
///
/// Babies cannot wear a harness, so interactions skip it; this keeps the event wrapper.
pub fn push_goggle_event(mob: &mut Mob, down: bool) {
    mob.events.push(MobEvent::EntityEvent {
        event: u8::from(down),
    });
}

/// 把新的 `HappyGhastState` 挂到 `Mob` 上时用的缺省值（`Mob::new` 里调）。
///
/// Default state attached to a `Mob` (called from `Mob::new`).
pub const fn initial_state() -> HappyGhastState {
    HappyGhastState {
        leash_holder_time: 0,
        still_timeout: 0,
        is_leash_holder: false,
        stays_still: false,
    }
}

/// `BlockPos.closerThan(Vec3i, double)`：`distSqr <= distance * distance`。
///
/// `BlockPos.closerThan(Vec3i, double)`: `distSqr <= distance * distance`.
fn home_dist_sqr(a: BlockPos, b: BlockPos) -> f64 {
    let dx = f64::from(a.x - b.x);
    let dy = f64::from(a.y - b.y);
    let dz = f64::from(a.z - b.z);
    dx * dx + dy * dy + dz * dz
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baby_dimensions_are_the_adult_scaled() {
        assert_eq!(dimensions(false), (4.0, 4.0, 2.6));
        assert_eq!(dimensions(true), (0.95, 0.95, 0.46875));
        assert_eq!(age_scale(true), 0.2375);
    }

    #[test]
    fn quad_holder_offsets_follow_the_official_formula() {
        // 成年 4×4：A=-0.125、B=1.75、C=1.875、D=0.125。
        // Adult 4x4: A=-0.125, B=1.75, C=1.875, D=0.125.
        let points = quad_holder_offsets(4.0, 4.0);
        assert_eq!(points[0], Vec3::new(-1.875, 0.125, 1.625));
        assert_eq!(points[1], Vec3::new(-1.875, 0.125, -1.875));
        assert_eq!(points[2], Vec3::new(1.875, 0.125, -1.875));
        assert_eq!(points[3], Vec3::new(1.875, 0.125, 1.625));
    }

    #[test]
    fn ridden_input_uses_the_pitch_formula() {
        let flat = ridden_input(
            RiddenInput {
                strafe: 1.0,
                forward: 1.0,
                pitch: 0.0,
                jumping: false,
            },
            0.05,
        );
        assert!((flat.z - 3.900_000_095_367_431_6 * 0.05).abs() < 1e-12);
        assert_eq!(flat.y, 0.0);
        let jump = ridden_input(
            RiddenInput {
                strafe: 0.0,
                forward: 0.0,
                pitch: 0.0,
                jumping: true,
            },
            0.05,
        );
        assert!((jump.y - 0.5 * 3.900_000_095_367_431_6 * 0.05).abs() < 1e-12);
    }

    #[test]
    fn harness_ids_and_food_tag_cover_the_generated_tables() {
        assert!(is_harness(items::WHITE_HARNESS));
        assert!(is_harness(items::BLACK_HARNESS));
        assert!(!is_harness(items::WHITE_HARNESS - 1));
        assert!(!is_harness(items::BLACK_HARNESS + 1));
        assert!(is_food(items::SNOWBALL));
    }
}
