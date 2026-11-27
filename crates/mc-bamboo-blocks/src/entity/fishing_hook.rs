//! 钓鱼浮漂实体，官方 `FishingHook`。
//!
//! 照 26.3 字节码实现抛竿初速度、三态状态机（`FLYING` / `HOOKED_IN_ENTITY` / `BOBBING`）、
//! 水面浮力、碰撞与世界边界反弹、咬钩计时（`catchingFish`）、5×4×5 露天水域检查
//! （`calculateOpenWater`）以及拉拽实体（`pullEntity`）。不存档（官方 `noSave()`）。
//!
//! Fishing bobber entity, matching vanilla 26.3 `FishingHook`: cast velocity,
//! three-state machine (`FLYING` / `HOOKED_IN_ENTITY` / `BOBBING`), water buoyancy,
//! collision & world-border bounce, bite timing (`catchingFish`), 5×4×5 open-water
//! check (`calculateOpenWater`), and `pullEntity`. Not serialized (`noSave()`).

use mc_bamboo_core::{Aabb, BlockPos, Vec3, mth};
use mc_bamboo_data::{blocks, items, registries::particle_type, sounds, tags::fluid_tags};

use super::{
    Entity, EntityKind, EntityUid, JavaRandom, KindPublic,
    arrow::{clip_entity, clip_including_border, lerp_rotation, player_box, rotation},
    ctx::{BlockView, Command, EntityCtx, PlayerView},
    mob::insecure_uuid,
};
use crate::{World, shape};

/// `FishingHook.GRAVITY`：`0.03F` 转 `double`。
pub const GRAVITY: f64 = 0.029_999_999_329_447_746;

/// `FishingHook.MAX_OUT_OF_WATER_TIME`。
pub const MAX_OUT_OF_WATER_TIME: i32 = 10;

/// 离玩家超过 32 格（`1024.0` 平方）时收掉浮漂。
pub const MAX_DISTANCE_SQR: f64 = 1024.0;

/// 落地静止超过 1200 刻自动消失。
pub const MAX_GROUND_LIFE: i32 = 1200;

/// `FishingHook.FishHookState`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FishHookState {
    #[default]
    Flying,
    HookedInEntity,
    Bobbing,
}

/// `FishingHook.OpenWaterType`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenWaterType {
    AboveWater,
    InsideWater,
    Invalid,
}

#[cfg(feature = "oracle-samples")]
thread_local! {
    static BREAK_GRAVITY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static BREAK_OPEN_WATER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static BREAK_PULL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 由 `mob-oracle` 的 `--break-self-test fishing_gravity` 设置。
///
/// Set by `mob-oracle --break-self-test fishing_gravity`.
#[cfg(feature = "oracle-samples")]
pub fn set_oracle_break_gravity(value: bool) {
    BREAK_GRAVITY.with(|cell| cell.set(value));
}

/// 由 `mob-oracle` 的 `--break-self-test fishing_open_water` 设置。
///
/// Set by `mob-oracle --break-self-test fishing_open_water`.
#[cfg(feature = "oracle-samples")]
pub fn set_oracle_break_open_water(value: bool) {
    BREAK_OPEN_WATER.with(|cell| cell.set(value));
}

/// 由 `mob-oracle` 的 `--break-self-test fishing_pull` 设置。
///
/// Set by `mob-oracle --break-self-test fishing_pull`.
#[cfg(feature = "oracle-samples")]
pub fn set_oracle_break_pull(value: bool) {
    BREAK_PULL.with(|cell| cell.set(value));
}

/// 浮漂在 `catchingFish` 里要广播的粒子或溅水音。
///
/// Particle or splash-sound side effects emitted by `catchingFish`.
#[derive(Debug, Clone, PartialEq)]
pub enum FishingEffect {
    Particle {
        particle: i32,
        position: Vec3,
        offset: Vec3,
        speed: f32,
        count: i32,
    },
    SplashSound {
        position: Vec3,
        volume: f32,
        pitch: f32,
    },
}

/// `FishingHook` 的完整状态。
#[derive(Debug, Clone, PartialEq)]
pub struct FishingHook {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    /// `xRot`。
    pub pitch: f32,
    /// `Projectile.leftOwner` / `hasBeenShot`。
    pub left_owner: bool,
    pub has_been_shot: bool,
    /// `FishingHook.syncronizedRandom`，每刻开头以 `uuid.lsb ^ gameTime` 设种。
    pub synchronized_random: JavaRandom,
    /// `DATA_BITING` 与字段 `biting`。
    pub biting: bool,
    /// `outOfWaterTime`，0..=10。
    pub out_of_water_time: i32,
    /// `life`：落地累计刻数，达 1200 消失。
    pub life: i32,
    /// `nibble`：咬钩倒计时（20..=40）。
    pub nibble: i32,
    /// `timeUntilLured`：等待上钩前导的倒计时（100..=600 减去 `lure_speed`）。
    pub time_until_lured: i32,
    /// `timeUntilHooked`：鱼游向浮漂的倒计时（20..=80）。
    pub time_until_hooked: i32,
    /// `fishAngle`：鱼游过来的角度（度）。
    pub fish_angle: f32,
    /// `openWater`：是否在 5×4×5 露天水域内（初始为真）。
    pub open_water: bool,
    /// `hookedIn` 与 `DATA_HOOKED_ENTITY`（0 表示空，否则 `id + 1`）。
    pub hooked_in: Option<EntityUid>,
    /// `currentState`。
    pub state: FishHookState,
    /// `luck`（海之眷顾等级，>= 0）。
    pub luck: i32,
    /// `lureSpeed`（饵钓减刻数，`level * 100`，>= 0）。
    pub lure_speed: i32,
    /// 收竿拉拽实体时待发的实体事件 31。
    pub pending_pull_event: bool,
    pub needs_sync: bool,
    pub sync_velocity: bool,
}

impl Default for FishingHook {
    fn default() -> Self {
        Self {
            uuid: [0; 16],
            owner: None,
            owner_uuid: None,
            pitch: 0.0,
            left_owner: false,
            has_been_shot: false,
            synchronized_random: JavaRandom::new(0),
            biting: false,
            out_of_water_time: 0,
            life: 0,
            nibble: 0,
            time_until_lured: 0,
            time_until_hooked: 0,
            fish_angle: 0.0,
            open_water: true,
            hooked_in: None,
            state: FishHookState::Flying,
            luck: 0,
            lure_speed: 0,
            pending_pull_event: false,
            needs_sync: false,
            sync_velocity: false,
        }
    }
}

impl FishingHook {
    pub fn new(luck: i32, lure_speed: i32) -> Self {
        Self {
            luck: luck.max(0),
            lure_speed: lure_speed.max(0),
            ..Self::default()
        }
    }

    pub(crate) fn public(&self) -> FishingHookPublic {
        FishingHookPublic {
            uuid: self.uuid,
            owner: self.owner,
            pitch: self.pitch,
            hooked_entity_data: self.hooked_in.map_or(0, |id| id + 1),
            biting: self.biting,
            open_water: self.open_water,
            state: self.state,
            needs_sync: self.needs_sync,
            sync_velocity: self.sync_velocity,
        }
    }

    /// `UUID.getLeastSignificantBits()`。
    pub fn uuid_lsb(&self) -> i64 {
        let bytes: [u8; 8] = self.uuid[8..16].try_into().unwrap_or([0; 8]);
        i64::from_be_bytes(bytes)
    }

    /// `FishingHook.isOpenWaterFishing()`。
    pub const fn is_open_water_fishing(&self) -> bool {
        self.open_water
    }
}

/// 同步层和外部只读查询看到的那一半。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FishingHookPublic {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub pitch: f32,
    /// `DATA_HOOKED_ENTITY`：0 为空，否则 `hookedIn.getId() + 1`。
    pub hooked_entity_data: i32,
    /// `DATA_BITING`。
    pub biting: bool,
    pub open_water: bool,
    pub state: FishHookState,
    pub needs_sync: bool,
    pub sync_velocity: bool,
}

/// 抛竿输入，对应 `new FishingHook(Player, Level, int luck, int lureSpeed)`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FishingHookCast {
    pub owner: EntityUid,
    pub owner_uuid: [u8; 16],
    pub owner_position: Vec3,
    pub owner_eye_y: f64,
    pub owner_yaw: f32,
    pub owner_pitch: f32,
    pub luck: i32,
    pub lure_speed: i32,
}

/// `Mth.clamp(float, float, float)`，保持官方比较顺序。
fn clamp_f32(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Java `Math.signum(double)`：`0.0` 返回 `0.0`（Rust 的 `f64::signum(0.0)` 返回 `1.0`）。
fn java_signum(d: f64) -> f64 {
    if d == 0.0 || d.is_nan() {
        d
    } else {
        d.signum()
    }
}

/// `Mth.nextFloat(RandomSource, float min, float max)`。
fn next_float_range(random: &mut JavaRandom, min: f32, max: f32) -> f32 {
    if min >= max {
        min
    } else {
        random.next_float() * (max - min) + min
    }
}

/// `Mth.nextInt(RandomSource, int min, int max)`（闭区间）。
fn next_int_inclusive(random: &mut JavaRandom, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        random.next_int_bounded(max - min + 1) + min
    }
}

impl Entity {
    /// 构造一个未抽抛竿初速度的浮漂实体（供 `prepare_entity` 分配 ID、种子与 UUID）。
    pub fn fishing_hook(cast: &FishingHookCast) -> Self {
        let mut hook = FishingHook::new(cast.luck, cast.lure_speed);
        hook.owner = Some(cast.owner);
        hook.owner_uuid = Some(cast.owner_uuid);
        Self::new(
            EntityKind::FishingHook(Box::new(hook)),
            Vec3::new(
                cast.owner_position.x,
                cast.owner_eye_y,
                cast.owner_position.z,
            ),
        )
    }

    /// 照 `FishingHook(Player, Level, int, int)` 字节码算初始位置、初速度和朝向。
    ///
    /// Computes the initial position, velocity (3× `triangle(0.5, 0.0103365)`),
    /// and rotation matching `FishingHook(Player, Level, int, int)`.
    pub fn cast_fishing_hook(&mut self, cast: &FishingHookCast) {
        let x_rot = cast.owner_pitch;
        let y_rot = cast.owner_yaw;
        let yaw_rad = f64::from(-y_rot * 0.017_453_292_f32 - std::f32::consts::PI);
        let cos = mth::cos(yaw_rad);
        let sin = mth::sin(yaw_rad);
        let pitch_rad = f64::from(-x_rot * 0.017_453_292_f32);
        let x_cos = -mth::cos(pitch_rad);
        let x_sin = mth::sin(pitch_rad);

        let x = cast.owner_position.x - f64::from(sin) * 0.3;
        let y = cast.owner_eye_y;
        let z = cast.owner_position.z - f64::from(cos) * 0.3;
        self.position = Vec3::new(x, y, z);
        self.old_position = self.position;

        let vy = clamp_f32(-(x_sin / x_cos), -5.0_f32, 5.0_f32);
        let mut movement = Vec3::new(f64::from(-sin), f64::from(vy), f64::from(-cos));
        let dist = movement.length();
        movement = movement.multiply(
            0.6 / dist + self.random.triangle(0.5, 0.010_336_5),
            0.6 / dist + self.random.triangle(0.5, 0.010_336_5),
            0.6 / dist + self.random.triangle(0.5, 0.010_336_5),
        );
        self.delta = movement;

        let (yaw, pitch) = rotation(movement);
        self.y_rot = yaw;
        if let EntityKind::FishingHook(hook) = &mut self.kind {
            hook.pitch = pitch;
        }
    }

    /// 供 `EntityStore::prepare` 初始化 UUID（`Entity.<init>` 的两次 `nextLong`）。
    pub(crate) fn finish_fishing_hook_random(&mut self) {
        if let EntityKind::FishingHook(hook) = &mut self.kind {
            hook.uuid = insecure_uuid(&mut self.random);
        }
    }
}

impl World<'_> {
    /// 生成一枚抛出的浮漂：先分配 ID / 种子与 UUID，再算抛竿初速度和朝向。
    ///
    /// Spawns a cast fishing hook: allocates id/seed/UUID first, then rolls cast velocity.
    pub fn spawn_fishing_hook_cast(&mut self, cast: &FishingHookCast) -> EntityUid {
        let mut entity = self.prepare_entity(Entity::fishing_hook(cast));
        entity.cast_fishing_hook(cast);
        self.add_prepared_entity(entity)
    }

    /// `FishingHook.pullEntity`：把钩住的生物或掉落物拉向主人，返回拉拽增量。
    ///
    /// Pulls a hooked mob or item entity toward the owner; players are client-authoritative
    /// and are pulled client-side via entity event 31.
    pub fn pull_hooked_entity(&mut self, hook_pos: Vec3, owner_pos: Vec3, target: EntityUid) {
        let delta = pull_entity_delta(owner_pos, hook_pos);
        if let Some(mut entity) = self.take_entity_for_hanging(target) {
            let can_simulate = match &entity.kind {
                EntityKind::Mob(mob) => mob
                    .controlling_passenger()
                    .is_none_or(|passenger| passenger >= super::FIRST_ENTITY_ID),
                EntityKind::Item(_) => true,
                _ => false,
            };
            if can_simulate {
                entity.delta = entity.delta.add(delta);
            }
            self.put_entity_for_hanging(entity);
        }
    }
}

/// `FishingHook.pullEntity` 的速度增量：`(owner - hook) * 0.1`。
pub fn pull_entity_delta(owner_pos: Vec3, hook_pos: Vec3) -> Vec3 {
    #[cfg(feature = "oracle-samples")]
    if BREAK_PULL.with(|cell| cell.get()) {
        return owner_pos.subtract(hook_pos).scale(0.2);
    }
    owner_pos.subtract(hook_pos).scale(0.1)
}

/// 钓上物品朝玩家飞去的初速度，照 `FishingHook.retrieve` 字节码。
///
/// Velocity of a fished `ItemEntity` flying toward the player (`FishingHook.retrieve`).
pub fn caught_item_velocity(owner_pos: Vec3, hook_pos: Vec3) -> Vec3 {
    let dx = owner_pos.x - hook_pos.x;
    let dy = owner_pos.y - hook_pos.y;
    let dz = owner_pos.z - hook_pos.z;
    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
    Vec3::new(dx * 0.1, dy * 0.1 + dist.sqrt() * 0.08, dz * 0.1)
}

/// `FishingHook.tick()`。
pub(crate) fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    // 1. 重设 `syncronizedRandom` 种子：`uuid.lsb ^ gameTime`。
    let game_time = ctx.blocks().game_time();
    let (owner_uid, mut hooked_in, current_state) = {
        let EntityKind::FishingHook(hook) = &mut me.kind else {
            return;
        };
        hook.needs_sync = false;
        hook.sync_velocity = false;
        let lsb = hook.uuid_lsb();
        hook.synchronized_random = JavaRandom::new(lsb ^ game_time);
        hook.has_been_shot = true;
        (hook.owner, hook.hooked_in, hook.state)
    };

    // 2. `Projectile.tick()`：`checkLeftOwner()` + `Entity.baseTick()`（流体交互、溅水随机、清 `firstTick`）。
    check_left_owner(me, ctx, owner_uid);
    me.update_projectile_fluid(ctx.blocks());
    me.first_tick = false;
    if me.position.y < f64::from(ctx.blocks().min_y() - 64) {
        me.discard();
        return;
    }

    // 3. `getPlayerOwner()`：找不到主人直接消失。
    let Some(owner) = owner_uid.and_then(|uid| ctx.player(uid)) else {
        me.discard();
        return;
    };

    // 4. `shouldStopFishing(owner)`（服务端）。
    if should_stop_fishing(me.position, &owner) {
        me.discard();
        return;
    }

    // 5. `onGround()` 生命周期计数（1200 刻）。
    if me.on_ground {
        let EntityKind::FishingHook(hook) = &mut me.kind else {
            return;
        };
        hook.life += 1;
        if hook.life >= MAX_GROUND_LIFE {
            me.discard();
            return;
        }
    } else if let EntityKind::FishingHook(hook) = &mut me.kind {
        hook.life = 0;
    }

    // 6. 检查浮漂所在格子的水深。
    let block_pos = me.block_position();
    let fluid = ctx.blocks().fluid_state(block_pos);
    let is_water_fluid = fluid.is_tagged(fluid_tags::WATER);
    let water_height = if is_water_fluid {
        ctx.blocks().fluid_height(fluid, block_pos)
    } else {
        0.0
    };
    let in_water_block = water_height > 0.0;

    // 7. 三态状态机。
    let mut effects: Vec<FishingEffect> = Vec::new();
    match current_state {
        FishHookState::Flying => {
            if hooked_in.is_some() {
                me.delta = Vec3::default();
                if let EntityKind::FishingHook(hook) = &mut me.kind {
                    hook.state = FishHookState::HookedInEntity;
                }
                return;
            }
            if in_water_block {
                me.delta = me.delta.multiply(0.3, 0.2, 0.3);
                if let EntityKind::FishingHook(hook) = &mut me.kind {
                    hook.state = FishHookState::Bobbing;
                }
                return;
            }
            check_collision(me, ctx, owner_uid);
            if let EntityKind::FishingHook(hook) = &me.kind {
                hooked_in = hook.hooked_in;
            }
        }
        FishHookState::HookedInEntity => {
            if let Some(target_uid) = hooked_in {
                if let Some((x, y_08, z)) = resolve_hooked_position(ctx, target_uid) {
                    me.position = Vec3::new(x, y_08, z);
                } else if let EntityKind::FishingHook(hook) = &mut me.kind {
                    hook.hooked_in = None;
                    hook.state = FishHookState::Flying;
                }
            }
            return;
        }
        FishHookState::Bobbing => {
            let delta = me.delta;
            let mut d = me.position.y + delta.y - f64::from(block_pos.y) - f64::from(water_height);
            if d.abs() < 0.01 {
                d += java_signum(d) * 0.1;
            }
            let rand_f = me.random.next_float();
            me.delta = Vec3::new(
                delta.x * 0.9,
                delta.y - d * f64::from(rand_f) * 0.2,
                delta.z * 0.9,
            );
            let need_open_check = matches!(
                &me.kind,
                EntityKind::FishingHook(hook)
                    if (hook.nibble > 0 || hook.time_until_hooked > 0)
                        && hook.open_water
                        && hook.out_of_water_time < MAX_OUT_OF_WATER_TIME
            );
            let computed_open = if need_open_check {
                calculate_open_water(ctx.blocks(), block_pos)
            } else {
                false
            };
            if let EntityKind::FishingHook(hook) = &mut me.kind {
                if hook.nibble > 0 || hook.time_until_hooked > 0 {
                    hook.open_water = hook.open_water
                        && hook.out_of_water_time < MAX_OUT_OF_WATER_TIME
                        && computed_open;
                } else {
                    hook.open_water = true;
                }
            }
            if in_water_block {
                let biting_plunge = {
                    let EntityKind::FishingHook(hook) = &mut me.kind else {
                        return;
                    };
                    hook.out_of_water_time = (hook.out_of_water_time - 1).max(0);
                    if hook.biting {
                        let a = hook.synchronized_random.next_float();
                        let b = hook.synchronized_random.next_float();
                        Some(-0.1 * f64::from(a) * f64::from(b))
                    } else {
                        None
                    }
                };
                if let Some(dy) = biting_plunge {
                    me.delta = me.delta.add(Vec3::new(0.0, dy, 0.0));
                }
                catching_fish(me, ctx.blocks(), block_pos, &mut effects);
            } else if let EntityKind::FishingHook(hook) = &mut me.kind {
                hook.out_of_water_time = (hook.out_of_water_time + 1).min(MAX_OUT_OF_WATER_TIME);
            }
        }
    }

    // 8. 非水格且未落地且未钩住实体时施加重力（0.03F）。
    if !is_water_fluid && !me.on_ground && hooked_in.is_none() {
        #[cfg(feature = "oracle-samples")]
        let g = if BREAK_GRAVITY.with(|cell| cell.get()) {
            0.05
        } else {
            GRAVITY
        };
        #[cfg(not(feature = "oracle-samples"))]
        let g = GRAVITY;
        me.delta = me.delta.add(Vec3::new(0.0, -g, 0.0));
    }

    // 9-10. `move(SELF, delta)` + `applyEffectsFromBlocks()`。
    let movement = me.delta;
    me.move_entity(ctx, movement);
    me.apply_effects_from_blocks(ctx.blocks());

    // 11. `Projectile.updateRotation()`（用本刻开始时的 `yRotO` / `xRotO` 做 0.2 插值）。
    let (target_yaw, target_pitch) = rotation(me.delta);
    let old_yaw = me.y_rot;
    me.y_rot = lerp_rotation(old_yaw, target_yaw);
    if let EntityKind::FishingHook(hook) = &mut me.kind {
        let old_pitch = hook.pitch;
        hook.pitch = lerp_rotation(old_pitch, target_pitch);
    }

    // 12. 飞行中落地或水平撞墙时速度归零。
    if matches!(&me.kind, EntityKind::FishingHook(hook) if hook.state == FishHookState::Flying)
        && (me.on_ground || me.horizontal_collision)
    {
        me.delta = Vec3::default();
    }

    // 13. 阻尼 `0.92`。
    me.delta = me.delta.scale(0.92);

    if !effects.is_empty() {
        ctx.emit(Command::FishingEffects(effects), me);
    }
}

/// `FishingHook.shouldStopFishing(Player)`。
pub fn should_stop_fishing(hook_pos: Vec3, owner: &PlayerView) -> bool {
    if !owner.alive || owner.spectator {
        return true;
    }
    let holds_rod = owner.main_hand == items::FISHING_ROD || owner.off_hand == items::FISHING_ROD;
    if !holds_rod {
        return true;
    }
    hook_pos.distance_to_sqr(owner.position) > MAX_DISTANCE_SQR
}

/// `Projectile.checkLeftOwner()`。
fn check_left_owner(me: &mut Entity, ctx: &dyn EntityCtx, owner_uid: Option<EntityUid>) {
    let already_left = matches!(&me.kind, EntityKind::FishingHook(hook) if hook.left_owner);
    if already_left {
        return;
    }
    let outside = match owner_uid.and_then(|uid| ctx.player(uid)) {
        None => true,
        Some(owner) => {
            let area = me
                .bounding_box()
                .expanded_towards(me.delta.x, me.delta.y, me.delta.z)
                .inflated(1.0);
            let owner_bb = player_box(&owner);
            !owner.alive || owner.spectator || !area.intersects(owner_bb)
        }
    };
    if outside && let EntityKind::FishingHook(hook) = &mut me.kind {
        hook.left_owner = true;
    }
}

/// `FishingHook.checkCollision()`：`ProjectileUtil.getHitResultOnMoveVector` + `hitTargetOrDeflectSelf`。
fn check_collision(me: &mut Entity, ctx: &dyn EntityCtx, owner_uid: Option<EntityUid>) {
    let left_owner = matches!(&me.kind, EntityKind::FishingHook(hook) if hook.left_owner);
    let blocks = ctx.blocks();
    let from = me.position;
    let movement = me.delta;
    let to_full = from.add(movement);
    let block_hit = clip_including_border(blocks, from, to_full, me.collision_context());
    let is_border_hit = block_hit.is_some_and(|hit| {
        let border = blocks.world_border();
        border.is_within_bounds(from.x, from.z, 0.0)
            && !border.is_within_bounds(to_full.x, to_full.z, 0.0)
            && blocks
                .clip_collider(from, to_full, me.collision_context())
                .is_none_or(|b| {
                    from.distance_to_sqr(hit.location) < from.distance_to_sqr(b.location)
                })
    });
    let end = block_hit.map_or(to_full, |hit| hit.location);
    let area = me
        .bounding_box()
        .expanded_towards(movement.x, movement.y, movement.z)
        .inflated(1.0);
    let margin = f64::from(((me.tick_count - 2) as f32 / 20.0).clamp(0.0, 0.3));

    let mut best_entity: Option<(f64, EntityUid)> = None;
    let mut consider = |uid: EntityUid, bb: Aabb| {
        let Some(location) = clip_entity(bb.inflated(margin), from, end) else {
            return;
        };
        let dist = from.distance_to_sqr(location);
        if best_entity.is_none_or(|(nearest, _)| dist < nearest) {
            best_entity = Some((dist, uid));
        }
    };

    ctx.for_each_other_in(area, &mut |other| {
        if other.removed {
            return;
        }
        let can_hit = match &other.kind {
            KindPublic::Mob(mob) => mob.health > 0.0,
            KindPublic::Item { .. } => true,
            _ => false,
        };
        if can_hit {
            consider(other.uid, other.bounding_box());
        }
    });
    blocks.for_each_player(&mut |player: &PlayerView| {
        if !player.alive || player.spectator {
            return;
        }
        if !left_owner && owner_uid == Some(player.uid) {
            return;
        }
        let bb = player_box(player);
        if bb.intersects(area) {
            consider(player.uid, bb);
        }
    });

    if let Some((_, target_uid)) = best_entity {
        // `onHitEntity`：`setHookedEntity(entity)`。
        if let EntityKind::FishingHook(hook) = &mut me.kind {
            hook.hooked_in = Some(target_uid);
        }
        return;
    }

    if let Some(hit) = block_hit {
        if is_border_hit {
            // `shouldBounceOnWorldBorder() == true`：`REVERSE` 折射倍率 0.2。
            me.delta = me.delta.scale(-0.2);
            return;
        }
        if blocks.block_state(hit.pos).is_air() {
            return;
        }
        // `onHitBlock`：`delta.normalize().scale(hit.distanceTo(this))`。
        let dist = hit.location.subtract(me.position).length();
        me.delta = me.delta.normalize().scale(dist);
    }
}

/// 在 `HOOKED_IN_ENTITY` 状态下查询被钩实体的 `(x, y + 0.8 * height, z)`。
fn resolve_hooked_position(ctx: &dyn EntityCtx, target_uid: EntityUid) -> Option<(f64, f64, f64)> {
    if let Some(player) = ctx.player(target_uid) {
        if !player.alive || player.spectator {
            return None;
        }
        let height = if player.sneaking {
            1.5_f32
        } else {
            player.height
        };
        return Some((
            player.position.x,
            player.position.y + f64::from(height) * 0.8,
            player.position.z,
        ));
    }
    let other = ctx.other(target_uid)?;
    if other.removed {
        return None;
    }
    if let KindPublic::Mob(mob) = &other.kind
        && mob.health <= 0.0
    {
        return None;
    }
    let (_, height) = other.kind.dimensions();
    Some((
        other.position.x,
        other.position.y + f64::from(height) * 0.8,
        other.position.z,
    ))
}

/// `FishingHook.catchingFish(BlockPos)`。
fn catching_fish(
    me: &mut Entity,
    blocks: &dyn BlockView,
    block_pos: BlockPos,
    effects: &mut Vec<FishingEffect>,
) {
    let mut rate = 1;
    let above = block_pos.above();
    if me.random.next_float() < 0.25 && blocks.is_raining_at(above) {
        rate += 1;
    }
    if me.random.next_float() < 0.5 && !blocks.can_see_sky(above) {
        rate -= 1;
    }

    let pos = me.position;
    let width = me.dimensions.0;

    let mut biting_changed_to_true = false;
    {
        let EntityKind::FishingHook(hook) = &mut me.kind else {
            return;
        };
        if hook.nibble > 0 {
            hook.nibble -= 1;
            if hook.nibble <= 0 {
                hook.time_until_lured = 0;
                hook.time_until_hooked = 0;
                hook.biting = false;
            }
        } else if hook.time_until_hooked > 0 {
            hook.time_until_hooked -= rate;
            if hook.time_until_hooked > 0 {
                hook.fish_angle += me.random.triangle(0.0, 9.188) as f32;
                let rad = f64::from(hook.fish_angle * 0.017_453_292_f32);
                let sin = mth::sin(rad);
                let cos = mth::cos(rad);
                let px = pos.x + f64::from(sin * hook.time_until_hooked as f32 * 0.1_f32);
                let py = f64::from(mth::floor(pos.y) as f32 + 1.0_f32);
                let pz = pos.z + f64::from(cos * hook.time_until_hooked as f32 * 0.1_f32);
                let below = BlockPos::containing(Vec3::new(px, py - 1.0, pz));
                if blocks.block_state(below).is(blocks::WATER) {
                    if me.random.next_float() < 0.15 {
                        effects.push(FishingEffect::Particle {
                            particle: particle_type::BUBBLE,
                            position: Vec3::new(px, py - f64::from(0.1_f32), pz),
                            offset: Vec3::new(f64::from(sin), 0.1, f64::from(cos)),
                            speed: 0.0,
                            count: 1,
                        });
                    }
                    let f1 = sin * 0.04_f32;
                    let f2 = cos * 0.04_f32;
                    effects.push(FishingEffect::Particle {
                        particle: particle_type::FISHING,
                        position: Vec3::new(px, py, pz),
                        offset: Vec3::new(f64::from(f2), 0.01, f64::from(-f1)),
                        speed: 1.0,
                        count: 0,
                    });
                    effects.push(FishingEffect::Particle {
                        particle: particle_type::FISHING,
                        position: Vec3::new(px, py, pz),
                        offset: Vec3::new(f64::from(-f2), 0.01, f64::from(f1)),
                        speed: 1.0,
                        count: 0,
                    });
                }
            } else {
                let pitch = 1.0 + (me.random.next_float() - me.random.next_float()) * 0.4;
                effects.push(FishingEffect::SplashSound {
                    position: pos,
                    volume: 0.25,
                    pitch,
                });
                let py = pos.y + 0.5;
                let count = (1.0_f32 + width * 20.0_f32) as i32;
                let w = f64::from(width);
                effects.push(FishingEffect::Particle {
                    particle: particle_type::BUBBLE,
                    position: Vec3::new(pos.x, py, pos.z),
                    offset: Vec3::new(w, 0.0, w),
                    speed: 0.2_f32,
                    count,
                });
                effects.push(FishingEffect::Particle {
                    particle: particle_type::FISHING,
                    position: Vec3::new(pos.x, py, pos.z),
                    offset: Vec3::new(w, 0.0, w),
                    speed: 0.2_f32,
                    count,
                });
                hook.nibble = next_int_inclusive(&mut me.random, 20, 40);
                if !hook.biting {
                    hook.biting = true;
                    biting_changed_to_true = true;
                }
            }
        } else if hook.time_until_lured > 0 {
            hook.time_until_lured -= rate;
            let mut chance = 0.15_f32;
            if hook.time_until_lured < 20 {
                chance += (20 - hook.time_until_lured) as f32 * 0.05_f32;
            } else if hook.time_until_lured < 40 {
                chance += (40 - hook.time_until_lured) as f32 * 0.02_f32;
            } else if hook.time_until_lured < 60 {
                chance += (60 - hook.time_until_lured) as f32 * 0.01_f32;
            }
            if me.random.next_float() < chance {
                let rad = f64::from(
                    next_float_range(&mut me.random, 0.0_f32, 360.0_f32) * 0.017_453_292_f32,
                );
                let dist = next_float_range(&mut me.random, 25.0_f32, 60.0_f32);
                let px = pos.x + f64::from(mth::sin(rad) * dist) * 0.1;
                let py = f64::from(mth::floor(pos.y) as f32 + 1.0_f32);
                let pz = pos.z + f64::from(mth::cos(rad) * dist) * 0.1;
                let below = BlockPos::containing(Vec3::new(px, py - 1.0, pz));
                if blocks.block_state(below).is(blocks::WATER) {
                    let count = 2 + me.random.next_int_bounded(2);
                    effects.push(FishingEffect::Particle {
                        particle: particle_type::SPLASH,
                        position: Vec3::new(px, py, pz),
                        offset: Vec3::new(f64::from(0.1_f32), 0.0, f64::from(0.1_f32)),
                        speed: 0.0,
                        count,
                    });
                }
            }
            if hook.time_until_lured <= 0 {
                hook.fish_angle = next_float_range(&mut me.random, 0.0_f32, 360.0_f32);
                hook.time_until_hooked = next_int_inclusive(&mut me.random, 20, 80);
            }
        } else {
            hook.time_until_lured = next_int_inclusive(&mut me.random, 100, 600);
            hook.time_until_lured -= hook.lure_speed;
        }
    }

    // `SynchedEntityData.set(DATA_BITING, true)` 在服务端同样会调 `onSyncedDataUpdated`：
    // 立刻用 `syncronizedRandom` 给浮漂一个向下的速度冲量。
    // `SynchedEntityData.set(DATA_BITING, true)` calls `onSyncedDataUpdated` on the server too,
    // immediately plunging the bobber downward with `syncronizedRandom`.
    if biting_changed_to_true {
        let vy = {
            let EntityKind::FishingHook(hook) = &mut me.kind else {
                return;
            };
            f64::from(-0.4_f32 * next_float_range(&mut hook.synchronized_random, 0.6_f32, 1.0_f32))
        };
        me.delta = Vec3::new(me.delta.x, vy, me.delta.z);
    }
}

/// `FishingHook.calculateOpenWater(BlockPos)`：5×4×5 检查（`dy ∈ -1..=2`, `dx,dz ∈ -2..=2`）。
pub fn calculate_open_water(blocks: &dyn BlockView, center: BlockPos) -> bool {
    #[cfg(feature = "oracle-samples")]
    if BREAK_OPEN_WATER.with(|cell| cell.get()) {
        return false;
    }
    let mut previous = OpenWaterType::Invalid;
    for dy in -1..=2 {
        let area_type = get_open_water_type_for_area(
            blocks,
            BlockPos::new(center.x - 2, center.y + dy, center.z - 2),
            BlockPos::new(center.x + 2, center.y + dy, center.z + 2),
        );
        match area_type {
            OpenWaterType::Invalid => return false,
            OpenWaterType::AboveWater => {
                if previous == OpenWaterType::Invalid {
                    return false;
                }
            }
            OpenWaterType::InsideWater => {
                if previous == OpenWaterType::AboveWater {
                    return false;
                }
            }
        }
        previous = area_type;
    }
    true
}

/// `FishingHook.getOpenWaterTypeForArea`：整片 5×5 格子类型完全一致则返回该类型，否则 `Invalid`。
fn get_open_water_type_for_area(
    blocks: &dyn BlockView,
    min: BlockPos,
    max: BlockPos,
) -> OpenWaterType {
    let mut acc: Option<OpenWaterType> = None;
    for y in min.y..=max.y {
        for z in min.z..=max.z {
            for x in min.x..=max.x {
                let kind = get_open_water_type_for_block(blocks, BlockPos::new(x, y, z));
                acc = Some(match acc {
                    None => kind,
                    Some(prev) if prev == kind => prev,
                    Some(_) => OpenWaterType::Invalid,
                });
            }
        }
    }
    acc.unwrap_or(OpenWaterType::Invalid)
}

/// `FishingHook.getOpenWaterTypeForBlock`：
/// 空气或睡莲 → `AboveWater`；水源且无碰撞形状 → `InsideWater`；其余 → `Invalid`。
fn get_open_water_type_for_block(blocks: &dyn BlockView, pos: BlockPos) -> OpenWaterType {
    let state = blocks.block_state(pos);
    if state.is_air() || state.is(blocks::LILY_PAD) {
        return OpenWaterType::AboveWater;
    }
    let fluid = state.fluid();
    if fluid.is_tagged(fluid_tags::WATER)
        && fluid.is_source()
        && shape::block_collision_boxes(
            state,
            pos,
            shape::CollisionContext::Empty,
            blocks.moving_piston_shape(pos).as_deref(),
            blocks.shulker_box_shape(pos),
        )
        .is_empty()
    {
        return OpenWaterType::InsideWater;
    }
    OpenWaterType::Invalid
}

/// 落地 [`Command::FishingEffects`]：推送粒子与溅水音效。
pub(crate) fn apply_effects(world: &mut World<'_>, effects: Vec<FishingEffect>) {
    for effect in effects {
        match effect {
            FishingEffect::Particle {
                particle,
                position,
                offset,
                speed,
                count,
            } => {
                world.push_effect(crate::Effect::Particles {
                    particle,
                    position,
                    offset,
                    speed,
                    count,
                });
            }
            FishingEffect::SplashSound {
                position,
                volume,
                pitch,
            } => {
                world.play_sound(
                    position,
                    sounds::sound_event("minecraft:entity.fishing_bobber.splash"),
                    sounds::NEUTRAL,
                    volume,
                    pitch,
                    false,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{
        BlockState,
        entity::view::{BlockSource, SourceView, ViewFacts},
    };

    use super::*;

    struct MapSource(HashMap<BlockPos, BlockState>);

    impl BlockSource for MapSource {
        fn block_state(&self, pos: BlockPos) -> BlockState {
            self.0.get(&pos).copied().unwrap_or(BlockState(blocks::AIR))
        }

        fn facts(&self) -> ViewFacts {
            ViewFacts::default()
        }
    }

    fn open_water_pool(center: BlockPos) -> HashMap<BlockPos, BlockState> {
        let mut map = HashMap::new();
        for y in (center.y - 1)..=center.y {
            for z in (center.z - 2)..=(center.z + 2) {
                for x in (center.x - 2)..=(center.x + 2) {
                    map.insert(BlockPos::new(x, y, z), BlockState(blocks::WATER));
                }
            }
        }
        map
    }

    #[test]
    fn open_water_check_accepts_5x4x5_pool_and_lily_pad_but_rejects_stone() {
        let center = BlockPos::new(0, 64, 0);
        let mut map = open_water_pool(center);
        let view = SourceView::new(MapSource(map.clone()));
        assert!(calculate_open_water(&view, center));

        // 睡莲属于 ABOVE_WATER，不破坏露天水域。
        // Lily pad counts as ABOVE_WATER and keeps open-water fishing true.
        map.insert(BlockPos::new(2, 65, 2), BlockState(blocks::LILY_PAD));
        let view_lily = SourceView::new(MapSource(map.clone()));
        assert!(calculate_open_water(&view_lily, center));

        // 5×4×5 范围内任意一格放石头都会破坏露天水域。
        // Any solid block in the 5×4×5 area breaks open-water fishing.
        map.insert(BlockPos::new(2, 66, 2), BlockState(blocks::STONE));
        let view_blocked = SourceView::new(MapSource(map));
        assert!(!calculate_open_water(&view_blocked, center));
    }

    #[test]
    fn open_water_rejects_flowing_water_and_air_below() {
        let center = BlockPos::new(0, 64, 0);
        let mut map = open_water_pool(center);
        // 下面一层空气 -> dy = -1 为 ABOVE_WATER 而 previous 为 INVALID，返回假。
        map.remove(&BlockPos::new(0, 63, 0));
        let view = SourceView::new(MapSource(map));
        assert!(!calculate_open_water(&view, center));

        // 流动水（level = 1，非水源）不是 INSIDE_WATER。
        let mut flowing = open_water_pool(center);
        let flowing_state = BlockState(blocks::WATER).with(crate::state::LEVEL, 1);
        flowing.insert(BlockPos::new(-2, 64, -2), flowing_state);
        let view_flow = SourceView::new(MapSource(flowing));
        assert!(!calculate_open_water(&view_flow, center));
    }

    #[test]
    fn caught_item_velocity_matches_vanilla_formula() {
        let owner = Vec3::new(0.0, 64.0, 0.0);
        let hook = Vec3::new(10.0, 64.0, 0.0);
        let vel = caught_item_velocity(owner, hook);
        assert!((vel.x - (-1.0)).abs() < 1e-12);
        assert!((vel.y - (10.0_f64.sqrt() * 0.08)).abs() < 1e-12);
        assert!((vel.z - 0.0).abs() < 1e-12);
    }

    #[test]
    fn fishing_hook_does_not_serialize_to_archive() {
        let cast = FishingHookCast {
            owner: 1,
            owner_uuid: [1; 16],
            owner_position: Vec3::new(0.0, 64.0, 0.0),
            owner_eye_y: 65.62,
            owner_yaw: 0.0,
            owner_pitch: 0.0,
            luck: 0,
            lure_speed: 0,
        };
        let row = Entity::fishing_hook(&cast).split();
        let tag = crate::archive::entity::write_entity(&row).expect("write_entity");
        assert_eq!(tag, mc_bamboo_nbt::Tag::Compound(Vec::new()));
    }
}
