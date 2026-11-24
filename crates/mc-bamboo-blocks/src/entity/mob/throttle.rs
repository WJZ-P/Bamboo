//! 三档降频：按到最近玩家的水平距离把生物分档，决定这一刻哪些子系统跑。
//!
//! 见 `docs/mob-entity-design.md` 4.2。整套是开关：`BlockGameRules` 的两个距离
//! 缺省都是 0，此时 [`Config::enabled`] 为假，生物按官方原来的逐刻路径跑，
//! 连一次距离计算都不做。打开之后：
//!
//! - 全速档：完整 tick；
//! - 降频档：只降决策（目标选择器、`GoalSelector`、大脑），间隔按距离 2–4 刻；
//!   移动物理、导航沿已有路径、所有计时器每刻照跑，按经过刻数补 `noActionTime`；
//! - 休眠档：每刻只推进计时器，每 20 刻一次完整 tick 但不主动走动；
//!   休眠太久的按类别令牌桶轮流醒一会儿。
//!
//! 分档和节奏只依赖岛的刻号、uid、距离和这一刻的实体状态；精确和并行两种驱动
//! 用同一份 [`plans_for_tick`]，唤醒名额按 uid 升序发放，结果和线程数无关。

use mc_bamboo_core::Vec3;

use crate::{
    BlockGameRules,
    entity::{
        Entity, EntityStore, EntityUid,
        ctx::{PlayerView, ThrottlePlan},
    },
};

use super::{Mob, MobType};

/// 每刻唤醒名额的倍率上限，防配置写坏。
pub const MAX_WAKEUPS_SCALE: usize = 16;
/// 休眠档每 20 刻做一次完整 tick（设计稿 4.2）。
pub const FULL_TICK_PERIOD: i64 = 20;
/// 降频档最密 / 最稀的间隔（设计稿 4.2）。
pub const MIN_DECISION_PERIOD: i64 = 2;
pub const MAX_DECISION_PERIOD: i64 = 4;

/// Bamboo 自己的三档降频配置，不是官方游戏规则，不进存档。
///
/// Bamboo's three-tier throttling config. Not a vanilla game rule, never saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// 降频档起点（推荐 32）。0 = 不启用降频档。
    pub throttle_distance: i32,
    /// 休眠档起点（推荐 64）。0 = 不启用休眠档。
    pub sleep_distance: i32,
    /// 每刻唤醒名额的倍率（1 = Paper 的缺省值）。
    pub wakeups_scale: usize,
}

impl Config {
    /// 缺省全关：精确模式。
    pub const OFF: Self = Self {
        throttle_distance: 0,
        sleep_distance: 0,
        wakeups_scale: 1,
    };

    /// 从方块代码能读到的那份规则快照取配置。
    pub const fn of(rules: &BlockGameRules) -> Self {
        Self {
            throttle_distance: rules.mob_throttle_distance,
            sleep_distance: rules.mob_sleep_distance,
            wakeups_scale: rules.mob_wakeups_scale,
        }
    }

    /// 两个距离都没设时整套关闭，走官方路径。
    pub const fn enabled(self) -> bool {
        self.throttle_distance > 0 || self.sleep_distance > 0
    }

    /// 归一化两个边界：休眠起点至少要和降频起点一样远，否则按同一个边界算。
    fn bounds(self) -> (f64, f64) {
        let throttle = self.throttle_distance.max(0) as f64;
        let sleep = self.sleep_distance.max(0) as f64;
        if self.sleep_distance > 0 && sleep < throttle {
            (throttle, throttle)
        } else {
            (throttle, sleep)
        }
    }
}

/// 唤醒类别。数字照 Paper `SpigotWorldConfig` 的 `wake-up-inactive` 缺省值：
/// 动物 4 / 1200 / 100，怪物 8 / 400 / 100，飞行怪物 8 / 200 / 100，
/// 其它 4 / 1200 / 100（每刻上限 / 休眠多久才候选 / 醒多少刻）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeCategory {
    Animal,
    Monster,
    FlyingMonster,
    Other,
}

impl WakeCategory {
    /// 飞行怪物目前只有幻翼；末影龙 / 恶魂 / 快乐恶魂还没建模。
    pub const fn of(kind: MobType) -> Self {
        if matches!(kind, MobType::Phantom) {
            Self::FlyingMonster
        } else if kind.is_enemy() {
            Self::Monster
        } else if kind.is_ageable() || kind.can_breathe_under_water() {
            Self::Animal
        } else {
            Self::Other
        }
    }

    /// （每刻补充的令牌上限，休眠多久才候选，醒多少刻）。
    pub const fn paper_defaults(self) -> (usize, i64, i64) {
        match self {
            Self::Animal => (4, 1200, 100),
            Self::Monster => (8, 400, 100),
            Self::FlyingMonster => (8, 200, 100),
            Self::Other => (4, 1200, 100),
        }
    }

    /// 应用 `mob_wakeups_scale` 之后的每刻上限。
    pub const fn max_per_tick(self, scale: usize) -> usize {
        self.paper_defaults().0 * scale
    }

    pub const fn inactive_for(self) -> i64 {
        self.paper_defaults().1
    }

    pub const fn awake_for(self) -> i64 {
        self.paper_defaults().2
    }

    const fn index(self) -> usize {
        match self {
            Self::Animal => 0,
            Self::Monster => 1,
            Self::FlyingMonster => 2,
            Self::Other => 3,
        }
    }

    const fn of_index(index: usize) -> Self {
        match index {
            1 => Self::Monster,
            2 => Self::FlyingMonster,
            3 => Self::Other,
            _ => Self::Animal,
        }
    }
}

/// 四种唤醒令牌桶。每刻各补一个、按类别上限封顶，由岛的实体表在算阶段之前推进。
///
/// 拆岛时**两边都从满桶开始**：令牌数只由刻号和类别决定，和岛上生物数无关，
/// 拆合不会让某一边的唤醒节奏错位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WakeTokens {
    /// 属于哪一刻；刻号变了就按经过的刻数补令牌，岛时钟平移也跟着走。
    pub time: i64,
    values: [usize; 4],
    caps: [usize; 4],
}

impl WakeTokens {
    /// 每刻的令牌从满桶开始。
    pub fn full(time: i64, config: Config) -> Self {
        let scale = config.wakeups_scale.clamp(1, MAX_WAKEUPS_SCALE);
        let caps = std::array::from_fn(|i| WakeCategory::of_index(i).max_per_tick(scale));
        Self {
            time,
            values: caps,
            caps,
        }
    }

    /// 推进到 `time`：每过一刻各补一个，封顶在类别上限。
    pub fn advance(&mut self, time: i64, config: Config) {
        let scale = config.wakeups_scale.clamp(1, MAX_WAKEUPS_SCALE);
        self.caps = std::array::from_fn(|i| WakeCategory::of_index(i).max_per_tick(scale));
        let elapsed = time.saturating_sub(self.time);
        if elapsed <= 0 {
            return;
        }
        self.time = time;
        let extra = usize::try_from(elapsed).unwrap_or(usize::MAX);
        for ((value, cap), index) in self.values.iter_mut().zip(self.caps).zip(0..4) {
            *value = (*value).saturating_add(extra).min(cap);
            let _ = index;
        }
    }

    /// 岛时钟平移 `delta`：只改刻号，令牌数不动。
    pub fn rebase(&mut self, delta: i64) {
        self.time = self.time.saturating_add(delta);
    }

    /// 拆岛：新表从满桶开始，刻号和上限跟原表一致。
    pub fn split_full(&self) -> Self {
        Self {
            time: self.time,
            values: self.caps,
            caps: self.caps,
        }
    }

    /// 岛合并：两份令牌桶的刻号已经对齐，按类别取剩余令牌较多的那份。
    pub fn absorb(&mut self, other: Self) {
        self.time = self.time.max(other.time);
        self.caps = other.caps;
        for (value, theirs) in self.values.iter_mut().zip(other.values) {
            *value = (*value).max(theirs);
        }
    }

    /// 还有没有名额。
    pub fn available(&self, category: WakeCategory) -> bool {
        self.values[category.index()] > 0
    }

    /// 消耗一个名额。没有名额时返回假，不改状态。
    pub fn take(&mut self, category: WakeCategory) -> bool {
        let slot = &mut self.values[category.index()];
        if *slot == 0 {
            return false;
        }
        *slot -= 1;
        true
    }

    /// 测试用：各类别当前令牌数。
    #[cfg(test)]
    pub fn values(&self) -> [usize; 4] {
        self.values
    }
}

/// 一档生物这一刻怎么跑。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// 完整 tick（官方行为）。
    Full,
    /// 决策降频；`period` 是两次决策之间的刻数。
    Reduced { period: i64 },
    /// 休眠：只推进计时器，`full_tick` 为真时做一次完整 tick 但不主动走动。
    Sleep { full_tick: bool },
}

impl Tier {
    /// 决策这一刻跑不跑。`phase` 是 `(game_time + uid)`。
    pub const fn decisions_run(self, phase: i64) -> bool {
        match self {
            Self::Full => true,
            Self::Reduced { period } => phase.rem_euclid(if period > 1 { period } else { 1 }) == 0,
            Self::Sleep { full_tick } => full_tick,
        }
    }

    /// 是不是休眠档的「只推进计时器」那一刻。每 20 刻的完整 tick 不算。
    pub const fn is_sleep(self) -> bool {
        matches!(self, Self::Sleep { full_tick: false })
    }
}

/// 生物身上和降频有关的几个时刻。全是岛时钟的绝对刻号，拆岛平移跟着实体走；
/// 不存档、不同步。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobThrottle {
    /// 上次决策真正运行过的刻号。
    pub last_decision: i64,
    /// 上次非休眠（全速 / 降频 / 被唤醒）的刻号，用来判「休眠太久」。
    pub last_active: i64,
    /// 限量唤醒的有效期，刻号大于当前刻号时按全速档跑。
    pub awake_until: i64,
}

impl MobThrottle {
    pub const NEW: Self = Self {
        last_decision: i64::MIN,
        last_active: i64::MIN,
        awake_until: i64::MIN,
    };

    /// 岛合并：绝对时刻整体平移。
    pub fn rebase(&mut self, delta: i64) {
        self.last_decision = self.last_decision.saturating_add(delta);
        self.last_active = self.last_active.saturating_add(delta);
        self.awake_until = self.awake_until.saturating_add(delta);
    }

    /// 跳过的决策间隔：这次运行距上次运行过了几刻（至少 1）。
    pub fn gap(&self, game_time: i64) -> i64 {
        if self.last_decision == i64::MIN {
            1
        } else {
            game_time.saturating_sub(self.last_decision).max(1)
        }
    }
}

/// 到最近非旁观玩家的水平距离平方。`None` 表示没有合格玩家，按无限远算。
///
/// 两种驱动读的是同一份玩家名单：精确模式来自 `Level::for_each_player`，
/// 并行模式来自算阶段开始时的只读快照。
pub fn nearest_player_distance_sqr(position: Vec3, players: &[PlayerView]) -> Option<f64> {
    let mut best = f64::INFINITY;
    for player in players {
        if player.spectator {
            continue;
        }
        let dx = player.position.x - position.x;
        let dz = player.position.z - position.z;
        best = best.min(dx * dx + dz * dz);
    }
    best.is_finite().then_some(best)
}

/// 4.2 的豁免清单，命中就算全速档。
///
/// 还没建模的状态：村民恐慌 / 袭击状态（Bamboo 没有村民和袭击系统）、拴绳
/// （没有拴绳实体）、活塞推动（实体没有 `PISTON` 移动）。「在水流里」用
/// 「浸水且会被水流推」近似：水流矢量不在实体状态里。这几项在注释和报告里写明。
pub fn exempt(
    mob: &Mob,
    now: i64,
    on_ground: bool,
    in_water: bool,
    remaining_fire_ticks: i32,
) -> bool {
    // 有攻击目标、刚受伤、着火。
    mob.target.is_some()
        || now.saturating_sub(mob.last_damage_stamp) <= 40
        || mob.hurt_time > 0
        || remaining_fire_ticks > 0
        // 没落地、在水流里。
        || !on_ground
        || (in_water && mob.mob_type.is_pushed_by_fluid())
        // 幼年或求爱中。
        || mob.is_baby()
        || mob.in_love > 0
        // 点燃的苦力怕。
        || (mob.mob_type == MobType::Creeper && mob.creeper.ignited)
}

/// 按距离和刻号分档；豁免和限量唤醒由调用方盖成全速档。
pub fn classify(config: Config, game_time: i64, nearest_sqr: Option<f64>) -> Tier {
    if !config.enabled() {
        return Tier::Full;
    }
    let distance = nearest_sqr.map(f64::sqrt).unwrap_or(f64::INFINITY);
    let (throttle, sleep) = config.bounds();
    // 降频档起点之前一律全速。
    if config.throttle_distance > 0 && distance <= throttle {
        return Tier::Full;
    }
    if config.sleep_distance > 0 && distance > sleep {
        return Tier::Sleep {
            full_tick: game_time.rem_euclid(FULL_TICK_PERIOD) == 0,
        };
    }
    // 降频档：两个边界之间按距离从 2 线性增长到 4；只开降频档时超出部分按 4。
    let period = if config.sleep_distance > 0 && sleep > throttle {
        let fraction = ((distance - throttle) / (sleep - throttle)).clamp(0.0, 1.0);
        let steps = (MAX_DECISION_PERIOD - MIN_DECISION_PERIOD) as f64 * fraction;
        MIN_DECISION_PERIOD + steps.round() as i64
    } else {
        MAX_DECISION_PERIOD
    }
    .clamp(MIN_DECISION_PERIOD, MAX_DECISION_PERIOD);
    Tier::Reduced { period }
}

/// 决策相位：`(game_time + uid)`，两种驱动同一份公式。
pub fn decision_phase(game_time: i64, uid: EntityUid) -> i64 {
    game_time.saturating_add(i64::from(uid))
}

/// 一刻开头给全部实体算降频计划，按 uid 升序。精确和并行两种驱动都调这里：
/// 同一份规则、玩家名单、刻号和令牌桶 ⇒ 同一份计划，和线程数无关。
///
/// 令牌桶放在实体表上（每岛一份），拆合时跟着表走。命中豁免的不休眠、不占令牌。
pub fn plans_for_tick(
    store: &mut EntityStore,
    game_time: i64,
    config: Config,
    players: &[PlayerView],
) -> Vec<ThrottlePlan> {
    if !config.enabled() {
        return Vec::new();
    }
    let (uids, rows) = store.tick_rows();
    let roots = store.riding_roots();
    let mut tokens = store.wake_tokens();
    tokens.advance(game_time, config);
    let mut plans = Vec::with_capacity(rows.len());
    for (&uid, &row) in uids.iter().zip(&rows) {
        let mut plan = ThrottlePlan::FULL;
        store.with_throttle_row_mut(row, &mut |public, state| {
            if let crate::entity::KindState::Mob(mob) = &mut state.kind {
                // 4.2 的豁免：算全速档，不占令牌，也不累计「休眠太久」。
                let kind = mob.mob_type;
                let spared = exempt(
                    mob,
                    game_time,
                    public.on_ground,
                    state.in_water,
                    state.remaining_fire_ticks,
                );
                if spared {
                    mob.throttle.last_active = game_time;
                    plan.tier = Tier::Full;
                } else {
                    let nearest = nearest_player_distance_sqr(public.position, players);
                    let mut tier = classify(config, game_time, nearest);
                    let awake = mob.throttle.awake_until > game_time;
                    if awake {
                        tier = Tier::Full;
                    }
                    // 刚生成的生物没有「上次活跃时刻」：这一刻当起点，不能因为
                    // `i64::MIN` 的初值被当成休眠了无限久、第一刻就领走唤醒令牌。
                    let fresh = mob.throttle.last_active == i64::MIN;
                    if fresh {
                        mob.throttle.last_active = game_time;
                    }
                    // 休眠太久的按类别轮流醒一会儿；令牌按 uid 升序发放。
                    let category = WakeCategory::of(kind);
                    if !fresh
                        && !awake
                        && matches!(tier, Tier::Sleep { .. })
                        && game_time.saturating_sub(mob.throttle.last_active)
                            >= category.inactive_for()
                        && tokens.available(category)
                        && tokens.take(category)
                    {
                        let until = game_time.saturating_add(category.awake_for());
                        mob.throttle.awake_until = until;
                        tier = Tier::Full;
                        plan.wake_until = until;
                    }
                    if !matches!(tier, Tier::Sleep { .. }) {
                        mob.throttle.last_active = game_time;
                    }
                    if let Tier::Sleep { full_tick } = tier
                        && full_tick
                    {
                        // 每 20 刻一次完整 tick，但这一刻不主动走动。
                        plan.no_active_move = true;
                    }
                    plan.tier = tier;
                }
                // 决策这一刻跑不跑；上次运行时刻决定补偿的刻数。
                let phase = decision_phase(game_time, uid);
                plan.decisions_run = plan.tier.decisions_run(phase);
                if plan.decisions_run {
                    let gap = mob.throttle.gap(game_time);
                    if gap > 1 {
                        // 按经过的刻数补 `noActionTime`；随机数不重放。
                        let extra = (gap - 1).min(i64::from(i32::MAX)) as i32;
                        mob.no_action_time = mob.no_action_time.saturating_add(extra);
                    }
                    mob.seen.clear();
                    mob.throttle.last_decision = game_time;
                }
            } else {
                let tier = classify(
                    config,
                    game_time,
                    nearest_player_distance_sqr(public.position, players),
                );
                // 非生物实体（掉落物、经验球等）也按距离休眠，只推进计时器。
                if let Tier::Sleep { full_tick } = tier {
                    plan.tier = Tier::Sleep { full_tick };
                    plan.decisions_run = full_tick;
                    plan.no_active_move = full_tick;
                }
            }
        });
        plans.push(plan);
    }
    // 乘客跟根实体用同一档（主代理确认的约定）：根在 uid 升序名单里的下标由
    // `riding_roots` 给出，乘客自己的那份计划被根的计划覆盖。
    // Passengers reuse their root's tier: `riding_roots` gives the root index.
    let mut shared = plans.clone();
    for (index, &root) in roots.iter().enumerate() {
        if root != index
            && let (Some(plan), Some(&root_plan)) = (shared.get_mut(index), plans.get(root))
        {
            *plan = root_plan;
        }
    }
    store.set_wake_tokens(tokens);
    shared
}

/// 休眠档只推进的计时器清单（设计稿 4.2）：实体计数、无敌时间、着火刻数、
/// 年龄 / 繁殖冷却、消失计时、受伤与死亡计时、药水效果和空气。
///
/// 掉落物 / 经验球的拾取延迟和年龄在 [`sleep_other_timers`] 里按同一份清单推进。
pub fn sleep_mob_timers(me: &mut Entity, mob: &mut Mob) {
    me.common_tick();
    if me.remaining_fire_ticks > 0 {
        me.remaining_fire_ticks -= 1;
    }
    if mob.invulnerable_time > 0 {
        mob.invulnerable_time -= 1;
    }
    if mob.hurt_time > 0 {
        mob.hurt_time -= 1;
    }
    if mob.damage_cooldown > 0 {
        mob.damage_cooldown -= 1;
    }
    if mob.death_time > 0 {
        mob.death_time += 1;
    }
    if mob.last_hurt_by_player_memory_time > 0 {
        mob.last_hurt_by_player_memory_time -= 1;
    } else {
        mob.last_hurt_by_player = None;
        mob.last_hurt_by_player_ref = None;
    }
    // 年龄和长大后的尺寸 / 眼高照官方 `AgeableMob.aiStep`。
    if mob.mob_type.is_ageable() {
        if mob.is_baby() && !mob.age_locked {
            super::living::set_age(me, mob, mob.age.saturating_add(1));
        } else if mob.age > 0 {
            super::living::set_age(me, mob, mob.age - 1);
        }
    }
    if mob.in_love > 0 {
        mob.in_love -= 1;
    }
    if mob.forced_age_timer > 0 {
        mob.forced_age_timer -= 1;
    }
    if mob.age_lock_particle_timer > 0 {
        mob.age_lock_particle_timer -= 1;
    }
    mob.no_action_time = mob.no_action_time.saturating_add(1);
    // 效果倒计时和空气都是相对量；伤害动作在休眠档不施加（不跑物理、不取随机数）。
    let state = super::effects::EffectState {
        health: mob.health,
        max_health: mob
            .attributes
            .value(mc_bamboo_data::registries::attribute::MAX_HEALTH) as f32,
        absorption: mob.absorption,
    };
    let events = mob.effects.tick(me.tick_count, state);
    if events.attributes_changed {
        mob.effects.rebuild_attributes(&mut mob.attributes);
    }
    if mob.air_supply < mob.mob_type.max_air_supply() {
        mob.air_supply = mob.mob_type.increase_air_supply(mob.air_supply);
    }
    mob.seen.clear();
}

/// 测试用：拼一名站在 `position` 的非旁观玩家视图。
#[cfg(test)]
pub fn test_player(uid: EntityUid, position: Vec3) -> PlayerView {
    PlayerView {
        uid,
        uuid: None,
        position,
        delta: Vec3::default(),
        eye_height: 1.62,
        health: 20.0,
        look_direction: Vec3::default(),
        gaze_disguise: false,
        alive: true,
        spectator: false,
        creative: false,
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

/// 休眠档里非生物实体只推进计时器：掉落物的拾取延迟与年龄、经验球的年龄，
/// 以及着火刻数。位置、速度和随机数都不动。
pub fn sleep_other_timers(me: &mut Entity) {
    me.common_tick();
    if me.remaining_fire_ticks > 0 {
        me.remaining_fire_ticks -= 1;
    }
    if let Some(item) = me.ground_mut() {
        if item.pickup_delay > 0 && item.pickup_delay != 32767 {
            item.pickup_delay -= 1;
        }
        if item.age != -32768 {
            item.age += 1;
        }
    }
    if let crate::entity::EntityKind::ExperienceOrb(orb) = &mut me.kind {
        orb.age = orb.age.saturating_add(1);
    }
}

#[cfg(test)]
mod tests;
