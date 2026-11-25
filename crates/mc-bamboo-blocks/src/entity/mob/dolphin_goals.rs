//! 海豚专属目标：官方 `Dolphin.registerGoals` 里那几个没有通用版本的目标。
//! 通用目标（`RandomSwimmingGoal`、`RandomLookAroundGoal`、`LookAtPlayerGoal`、
//! `MeleeAttackGoal`、`AvoidEntityGoal`、`HurtByTargetGoal`）直接复用 `goals.rs` 的变体，
//! 这里只放海豚自己的：`BreathAirGoal`、`TryFindLiquidGoal`、
//! `DolphinSwimToTreasureGoal`、`DolphinSwimWithPlayerGoal`、`DolphinJumpGoal`、
//! `MoveToItemGoal`、`PlayWithItemsGoal`、`FollowPlayerRiddenEntityGoal`。
//!
//! 有几个目标要结构定位、船 / 鹦鹉螺、掉落物实体、玩家药水效果等仓库还没接的设施。
//! 照 `fox_goals.rs` 里 `DefendTrusted` 的既有做法：把 `canUse` 的取数（次数和顺序）
//! 一一保住，设施缺的部分让这一支走不到底、返回假；README 已知限制里逐条记下。
//!
//! Dolphin-specific goals: the ones in `Dolphin.registerGoals` with no shared version.
//! Shared goals (`RandomSwimmingGoal`, `RandomLookAroundGoal`, `LookAtPlayerGoal`,
//! `MeleeAttackGoal`, `AvoidEntityGoal`, `HurtByTargetGoal`) reuse the variants in
//! `goals.rs`; this module holds the dolphin's own: `BreathAirGoal`,
//! `TryFindLiquidGoal`, `DolphinSwimToTreasureGoal`, `DolphinSwimWithPlayerGoal`,
//! `DolphinJumpGoal`, `MoveToItemGoal`, `PlayWithItemsGoal`,
//! `FollowPlayerRiddenEntityGoal`.
//!
//! Several need structures, boats / nautiluses, item entities or player effects that
//! are not wired here. Following the existing `DefendTrusted` precedent in
//! `fox_goals.rs`: keep every `canUse` random draw (count and order) faithful, and let
//! the infra-dependent branch return false. Each is listed in the README limitations.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::registries::mob_effect;

use super::{
    Mob,
    goals::{self, Flag, reduced_tick_delay},
    living,
};
use crate::entity::{
    Entity, EntityUid,
    ctx::{Command, EntityCtx},
};

/// 海豚专属目标和它们自己的字段。
///
/// The dolphin-specific goals and their own state.
#[derive(Debug, Clone, PartialEq)]
pub enum DolphinGoal {
    /// `BreathAirGoal`：氧气低于 140 时找水面。MOVE|LOOK、不可打断。
    ///
    /// `BreathAirGoal`: surface for air below 140. MOVE|LOOK, not interruptable.
    BreathAir,
    /// `TryFindLiquidGoal`：着地且脚下不是水时找一格水。不锁标志位。
    ///
    /// `TryFindLiquidGoal`: find water when grounded out of water. No flags.
    TryFindLiquid,
    /// `DolphinSwimToTreasureGoal`：被喂鱼后带路去沉船 / 遗迹。MOVE|LOOK、不可打断。
    ///
    /// `DolphinSwimToTreasureGoal`: lead to a shipwreck / ruins after being fed.
    /// MOVE|LOOK, not interruptable.
    SwimToTreasure { stuck: bool },
    /// `DolphinSwimWithPlayerGoal`：跟着游泳的玩家、给海豚恩惠。MOVE|LOOK。
    ///
    /// `DolphinSwimWithPlayerGoal`: follow a swimming player and grant Dolphin's Grace.
    /// MOVE|LOOK.
    SwimWithPlayer { player: Option<EntityUid> },
    /// `DolphinJumpGoal`：在水里隔一阵跃出水面。继承 `JumpGoal`，锁 MOVE|JUMP、不可打断。
    ///
    /// `DolphinJumpGoal`: breach now and then. Inherits `JumpGoal`, locking MOVE|JUMP;
    /// not interruptable.
    Jump { breached: bool },
    /// `MoveToItemGoal`：游向附近的掉落物。MOVE。
    ///
    /// `MoveToItemGoal`: swim towards a nearby item entity. MOVE.
    MoveToItem { cooldown: i32 },
    /// `PlayWithItemsGoal`：手里有东西就一直抛着玩。不锁标志位。
    ///
    /// `PlayWithItemsGoal`: toss the held item around. No flags.
    PlayWithItems,
    /// `FollowPlayerRiddenEntityGoal`（船 / 鹦鹉螺）：跟着被玩家骑的实体。不锁标志位。
    ///
    /// `FollowPlayerRiddenEntityGoal` (boat / nautilus): follow a player-ridden entity.
    /// No flags.
    FollowRidden,
}

impl DolphinGoal {
    /// `DolphinJumpGoal(this, 10)` 的间隔：`reducedTickDelay(10)`。
    ///
    /// The interval of `DolphinJumpGoal(this, 10)`: `reducedTickDelay(10)`.
    const JUMP_INTERVAL: i32 = reduced_tick_delay(10);

    pub(crate) const fn flags(&self) -> &'static [Flag] {
        match self {
            Self::BreathAir | Self::SwimToTreasure { .. } | Self::SwimWithPlayer { .. } => {
                &[Flag::Move, Flag::Look]
            }
            Self::MoveToItem { .. } => &[Flag::Move],
            // `JumpGoal` 构造函数：`setFlags(MOVE, JUMP)`。
            // `JumpGoal` constructor: `setFlags(MOVE, JUMP)`.
            Self::Jump { .. } => &[Flag::Move, Flag::Jump],
            Self::TryFindLiquid | Self::PlayWithItems | Self::FollowRidden => &[],
        }
    }

    pub(crate) const fn name(&self) -> &'static str {
        match self {
            Self::BreathAir => "BreathAirGoal",
            Self::TryFindLiquid => "TryFindLiquidGoal",
            Self::SwimToTreasure { .. } => "DolphinSwimToTreasureGoal",
            Self::SwimWithPlayer { .. } => "DolphinSwimWithPlayerGoal",
            Self::Jump { .. } => "DolphinJumpGoal",
            Self::MoveToItem { .. } => "MoveToItemGoal",
            Self::PlayWithItems => "PlayWithItemsGoal",
            Self::FollowRidden => "FollowPlayerRiddenEntityGoal",
        }
    }

    /// `Goal.isInterruptable`：找气、找宝藏、跃水三支官方返回假。
    ///
    /// `Goal.isInterruptable`: breath, treasure and jump return false in vanilla.
    pub(crate) const fn interruptable(&self) -> bool {
        !matches!(
            self,
            Self::BreathAir | Self::SwimToTreasure { .. } | Self::Jump { .. }
        )
    }

    pub(crate) fn can_use(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &mut dyn EntityCtx,
    ) -> bool {
        match self {
            // `BreathAirGoal.canUse`：`getAirSupply() < 140`；不取随机数。
            // `BreathAirGoal.canUse`: `getAirSupply() < 140`; no random draw.
            Self::BreathAir => mob.air_supply < 140,
            // `TryFindLiquidGoal.canUse`：着地且脚下方块不是水；不取随机数。
            // 夹具里用 `!in_water` 近似脚下方块的流体判定（见 README 已知差异）。
            // `TryFindLiquidGoal.canUse`: grounded with non-water at the feet; no draw.
            // The fixture approximates the feet-fluid test with `!in_water` (README).
            Self::TryFindLiquid => me.on_ground && !me.in_water,
            // `DolphinSwimToTreasureGoal.canUse`：`gotFish && air >= 100`；不取随机数。
            // `DolphinSwimToTreasureGoal.canUse`: `gotFish && air >= 100`; no draw.
            Self::SwimToTreasure { .. } => mob.dolphin.got_fish && mob.air_supply >= 100,
            // `DolphinSwimWithPlayerGoal.canUse`：`getNearestPlayer(SWIM_WITH_PLAYER_TARGETING,
            // this)`，条件是玩家在游泳且不是自己的攻击目标。
            // `DolphinSwimWithPlayerGoal.canUse`: `getNearestPlayer(SWIM_WITH_PLAYER_TARGETING,
            // this)`, requiring a swimming player that is not the dolphin's target.
            Self::SwimWithPlayer { player } => {
                let found = nearest_swim_player(me, mob, ctx);
                *player = found;
                found.is_some_and(|uid| {
                    ctx.player(uid).is_some_and(|view| view.swimming) && mob.target != Some(uid)
                })
            }
            // `DolphinJumpGoal.canUse`：先取一次 `nextInt(reducedTickDelay(10))`，
            // 再看 6 个步长上的水与水面是否清空。封闭水柱里上方仍是水，几何
            // 判定恒假；照官方保住这一取数后返回假。
            // `DolphinJumpGoal.canUse`: draw `nextInt(reducedTickDelay(10))`, then
            // check water and surface clearance at six steps. A closed water column
            // keeps water above, so geometry fails; keep the draw and return false.
            Self::Jump { .. } => {
                me.random.next_int_bounded(Self::JUMP_INTERVAL) == 0 && jump_geometry_clear(me, ctx)
            }
            // `MoveToItemGoal.canUse`：冷却过了才扫 8 格内的掉落物。
            // 夹具里没有掉落物，`getEntitiesOfClass` 返回空；不取随机数。
            // `MoveToItemGoal.canUse`: past cooldown, scan for item entities within
            // eight blocks. The fixture has none; `getEntitiesOfClass` draws nothing.
            Self::MoveToItem { cooldown } => *cooldown <= me.tick_count && has_nearby_item(),
            // `PlayWithItemsGoal.canUse`：主手有东西；夹具里没有。
            // `PlayWithItemsGoal.canUse`: a held item; the fixture has none.
            Self::PlayWithItems => false,
            // `FollowPlayerRiddenEntityGoal.canUse`：5 格内被玩家骑的船 / 鹦鹉螺。
            // 夹具里没有这类实体，官方的 `getEntitiesOfClass` 也不取随机数。
            // `FollowPlayerRiddenEntityGoal.canUse`: a player-ridden boat / nautilus
            // within five blocks. The fixture has none, and vanilla draws nothing.
            Self::FollowRidden => false,
        }
    }

    pub(crate) fn can_continue(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &mut dyn EntityCtx,
    ) -> bool {
        match self {
            // `BreathAirGoal.canContinueToUse` == `canUse`。
            // `BreathAirGoal.canContinueToUse` == `canUse`.
            Self::BreathAir => mob.air_supply < 140,
            // `DolphinSwimToTreasureGoal.canContinueToUse`：没到宝藏、没卡住、氧气够。
            // `DolphinSwimToTreasureGoal.canContinueToUse`: not at the treasure, not
            // stuck, enough air.
            Self::SwimToTreasure { stuck } => {
                !*stuck && mob.dolphin.treasure_pos.is_some() && mob.air_supply >= 100
            }
            // `DolphinSwimWithPlayerGoal.canContinueToUse`：玩家还在游泳、距离 < 256。
            // `DolphinSwimWithPlayerGoal.canContinueToUse`: the player still swims and is
            // within 256 (squared, so a 16-block radius).
            Self::SwimWithPlayer { player } => player.is_some_and(|uid| {
                ctx.player(uid).is_some_and(|view| {
                    view.swimming && me.position.distance_to_sqr(view.position) < 256.0
                })
            }),
            // `DolphinJumpGoal.canContinueToUse`：还在上冲且没着地。
            // 官方是 `!(yd*yd < 0.03)` / `!(|xRot| < 10)`；NaN 时这两个比较都假、取反为真，
            // 所以用 `partial_cmp` 而不是 `>=` 保住 NaN 语义。
            // `DolphinJumpGoal.canContinueToUse`: still rising and not on the ground.
            // Vanilla uses `!(yd*yd < 0.03)` / `!(|xRot| < 10)`; on NaN both comparisons
            // are false and negate to true, so `partial_cmp` (not `>=`) keeps that.
            Self::Jump { .. } => {
                let yd = me.delta.y;
                let not_slow =
                    !matches!((yd * yd).partial_cmp(&0.03), Some(std::cmp::Ordering::Less));
                let not_level = !matches!(
                    mob.x_rot.abs().partial_cmp(&10.0),
                    Some(std::cmp::Ordering::Less)
                );
                (not_slow || mob.x_rot == 0.0 || not_level || !me.in_water) && !me.on_ground
            }
            // 其余目标官方没有覆写 `canContinueToUse`，交给 `canUse` 的语义。
            // The rest do not override `canContinueToUse`; fall back to `canUse`.
            Self::TryFindLiquid
            | Self::MoveToItem { .. }
            | Self::PlayWithItems
            | Self::FollowRidden => self.can_use(me, mob, ctx),
        }
    }

    pub(crate) fn start(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        match self {
            // `BreathAirGoal.start`：停导航、找一格给气的方块、游过去。
            // `BreathAirGoal.start`: stop, find a breathable spot, swim to it.
            Self::BreathAir => {
                mob.navigation.stop();
                breath_destination(me, mob, ctx);
            }
            // `TryFindLiquidGoal.start`：脚下 5×5×3 里找第一格水，设成想去的位置。
            // `TryFindLiquidGoal.start`: find the first water block in the 5×5×3
            // neighbourhood and set it as the wanted position.
            Self::TryFindLiquid => find_liquid_start(me, mob, ctx),
            // `DolphinSwimToTreasureGoal.start`：清 stuck、停导航、定位结构。
            // 仓库没有结构定位，按官方找不到结构时把 `stuck` 置真、位置留空。
            // `DolphinSwimToTreasureGoal.start`: clear stuck, stop, locate the
            // structure. With no structure lookup, vanilla sets `stuck` true.
            Self::SwimToTreasure { stuck } => {
                *stuck = true;
                mob.navigation.stop();
                mob.dolphin.treasure_pos = None;
            }
            // `DolphinSwimWithPlayerGoal.start`：给玩家 100 刻 `DOLPHINS_GRACE`。
            // `DolphinSwimWithPlayerGoal.start`: grant the player 100 ticks of
            // `DOLPHINS_GRACE`.
            Self::SwimWithPlayer { player } => {
                if let Some(uid) = *player {
                    give_dolphins_grace(me, ctx, uid, 100);
                }
            }
            // `DolphinJumpGoal.start`：按运动方向给一个上冲速度、停导航。
            // `DolphinJumpGoal.start`: add an upward impulse along the motion
            // direction and stop navigating.
            Self::Jump { breached } => {
                *breached = false;
                let (sx, sz) = motion_step(me.delta);
                me.delta = me
                    .delta
                    .add(Vec3::new(f64::from(sx) * 0.6, 0.7, f64::from(sz) * 0.6));
                mob.navigation.stop();
            }
            // `MoveToItemGoal.start`：朝第一个掉落物游、播放玩耍音效、清冷却。
            // 夹具里没有掉落物，官方的 `items.isEmpty()` 短路，不取随机数。
            // `MoveToItemGoal.start`: swim to the first item, play a sound, clear the
            // cooldown. With no items vanilla short-circuits; no random draw.
            Self::MoveToItem { cooldown } => *cooldown = 0,
            // 其余目标 `start` 为空。
            // The rest have an empty `start`.
            Self::PlayWithItems | Self::FollowRidden => {}
        }
    }

    pub(crate) fn stop(&mut self, me: &mut Entity, mob: &mut Mob, _ctx: &mut dyn EntityCtx) {
        match self {
            // `BreathAirGoal` / `TryFindLiquidGoal` 的 `stop` 为空。
            // `BreathAirGoal` / `TryFindLiquidGoal` have an empty `stop`.
            Self::BreathAir | Self::TryFindLiquid => {}
            // `DolphinSwimToTreasureGoal.stop`：到点 / 卡住时清掉 `gotFish`。
            // `DolphinSwimToTreasureGoal.stop`: clear `gotFish` on arrival / stuck.
            Self::SwimToTreasure { stuck } => {
                if *stuck || mob.dolphin.treasure_pos.is_none() {
                    mob.dolphin.got_fish = false;
                }
            }
            // `DolphinSwimWithPlayerGoal.stop`：清玩家、停导航。
            // `DolphinSwimWithPlayerGoal.stop`: clear the player and stop navigating.
            Self::SwimWithPlayer { player } => {
                *player = None;
                mob.navigation.stop();
            }
            // `DolphinJumpGoal.stop`：俯仰归零。
            // `DolphinJumpGoal.stop`: reset pitch.
            Self::Jump { .. } => mob.x_rot = 0.0,
            // `MoveToItemGoal.stop`：抛掉手里的东西并抽冷却 `nextInt(100)`。
            // 夹具里没有物品，官方的 `dropItem()` 返回假、不取这个随机数。
            // `MoveToItemGoal.stop`: drop the held item and draw `nextInt(100)`. With
            // no item vanilla's `dropItem()` returns false and skips the draw.
            Self::MoveToItem { .. } => {}
            // `PlayWithItemsGoal.stop` / `FollowPlayerRiddenEntityGoal.stop` 为空或清字段。
            // `PlayWithItemsGoal.stop` / `FollowPlayerRiddenEntityGoal.stop` are empty.
            Self::PlayWithItems | Self::FollowRidden => {}
        }
        let _ = me;
    }

    pub(crate) fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        match self {
            // `DolphinJumpGoal.tick`：出水时记 breached、按速度调俯仰。
            // `DolphinJumpGoal.tick`: track breaching and set pitch from the velocity.
            Self::Jump { breached } => {
                if !*breached && me.in_water {
                    *breached = true;
                }
                let movement = me.delta;
                if movement.y * movement.y < 0.03 && mob.x_rot != 0.0 {
                    // `Mth.rotLerp(0.2F, xRot, 0)`。
                    // `Mth.rotLerp(0.2F, xRot, 0)`.
                    mob.x_rot *= 0.8;
                } else if movement.length() > 1.0e-5 {
                    let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
                    // `180.0F / (float) Math.PI` 是 float 除法，再变成 double。
                    // `180.0F / (float) Math.PI` is a float division promoted to double.
                    let rotation = (-movement.y).atan2(horizontal)
                        * f64::from(180.0_f32 / std::f32::consts::PI);
                    mob.x_rot = rotation as f32;
                }
            }
            // `DolphinSwimWithPlayerGoal.tick`：盯住玩家；2.5 格内停下，否则游过去
            // （速度倍率 4.0）；玩家还在游泳时每刻按世界随机源 1/6 的概率续上恩惠。
            //
            // `DolphinSwimWithPlayerGoal.tick`: look at the player, stop within 2.5
            // blocks or swim over at speed 4.0, and refresh the grace with a 1-in-6
            // world-random roll each tick while the player swims.
            Self::SwimWithPlayer { player } => {
                let Some(uid) = *player else {
                    return;
                };
                let Some(target) = ctx.player(uid) else {
                    return;
                };
                // `getMaxHeadYRot() + 20` 与 `getMaxHeadXRot()`：海豚两个都是 1。
                // `getMaxHeadYRot() + 20` and `getMaxHeadXRot()`: both are 1 on a dolphin.
                mob.look.set_look_at(
                    target.position.x,
                    target.eye_y(),
                    target.position.z,
                    21.0,
                    1.0,
                );
                if me.position.distance_to_sqr(target.position) < 6.25 {
                    mob.navigation.stop();
                } else {
                    let view = ctx.blocks();
                    let nav = living::nav_mob(me, mob, view);
                    mob.navigation
                        .move_to_entity(view, &nav, target.position, 4.0, 1);
                }
                if target.swimming && ctx.blocks().level_random_int(6) == 0 {
                    give_dolphins_grace(me, ctx, uid, 100);
                }
            }
            // 其余目标的 `tick` 要么依赖缺失设施、要么官方为空，无随机取数。
            // The rest depend on missing infra or have an empty `tick`; no draws.
            Self::BreathAir
            | Self::TryFindLiquid
            | Self::SwimToTreasure { .. }
            | Self::MoveToItem { .. }
            | Self::PlayWithItems
            | Self::FollowRidden => {}
        }
    }
}

/// `Dolphin.SWIM_WITH_PLAYER_TARGETING`：`forNonCombat().range(10.0).ignoreLineOfSight()`。
/// 返回最近的合格玩家；官方 `getNearestEntity` 不取随机数。
///
/// `Dolphin.SWIM_WITH_PLAYER_TARGETING`: `forNonCombat().range(10.0).ignoreLineOfSight()`.
/// Returns the nearest qualifying player; vanilla's `getNearestEntity` draws nothing.
fn nearest_swim_player(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> Option<EntityUid> {
    let view = ctx.blocks();
    let mut players = Vec::new();
    view.for_each_player(&mut |player| players.push(*player));
    let mut best = -1.0_f64;
    let mut found = None;
    for player in players {
        if !goals::targeting_test(me, mob, view, &player, 10.0, false, false) {
            continue;
        }
        let distance = me.position.distance_to_sqr(player.position);
        if best == -1.0 || distance < best {
            best = distance;
            found = Some(player.uid);
        }
    }
    found
}

/// `player.addEffect(new MobEffectInstance(MobEffects.DOLPHINS_GRACE, ticks), this.dolphin)`。
///
/// `player.addEffect(new MobEffectInstance(MobEffects.DOLPHINS_GRACE, ticks), this.dolphin)`.
fn give_dolphins_grace(me: &mut Entity, ctx: &mut dyn EntityCtx, target: EntityUid, ticks: i32) {
    ctx.emit(
        Command::AddPlayerEffect {
            target,
            effect: mob_effect::DOLPHINS_GRACE,
            duration: ticks,
            amplifier: 0,
        },
        me,
    );
}

/// `Entity.getMotionDirection()` 的水平步向：水平位移较大的轴决定正负，
/// 近零时给 (0, 0)。
///
/// `Entity.getMotionDirection()`'s horizontal step: the dominant horizontal axis
/// decides the sign, near-zero gives (0, 0).
fn motion_step(delta: Vec3) -> (i32, i32) {
    if delta.x == 0.0 && delta.z == 0.0 {
        (0, 0)
    } else if delta.x.abs() > delta.z.abs() {
        (if delta.x > 0.0 { 1 } else { -1 }, 0)
    } else {
        (0, if delta.z > 0.0 { 1 } else { -1 })
    }
}

/// `DolphinJumpGoal` 的 `waterIsClear` / `surfaceIsClear`：6 个步长上，
/// 水平方向那一格必须是水、且上面两格是空气。封闭水柱上方仍是水，故恒假。
/// `BLOCKS_DOLPHIN_JUMP` 标签里的方块这一期没有，普通水柱里不会命中，故略过
/// （README 已知差异）。
///
/// `DolphinJumpGoal`'s `waterIsClear` / `surfaceIsClear`: at six steps the horizontal
/// block must be water and the two blocks above it air. A closed water column keeps
/// water above, so this is always false. The `BLOCKS_DOLPHIN_JUMP` tag is not wired;
/// a plain water column never hits it, so it is skipped (README known difference).
fn jump_geometry_clear(me: &Entity, ctx: &dyn EntityCtx) -> bool {
    const STEPS: [i32; 6] = [0, 1, 4, 5, 6, 7];
    let view = ctx.blocks();
    let (sx, sz) = motion_step(me.delta);
    let base = me.block_position();
    for step in STEPS {
        let x = base.x + sx * step;
        let z = base.z + sz * step;
        let ahead = BlockPos::new(x, base.y, z);
        if !view.fluid_state(ahead).is_water() {
            return false;
        }
        if !view.block_state(BlockPos::new(x, base.y + 1, z)).is_air()
            || !view.block_state(BlockPos::new(x, base.y + 2, z)).is_air()
        {
            return false;
        }
    }
    true
}

/// 夹具里没有掉落物实体。
///
/// The fixture has no item entities.
fn has_nearby_item() -> bool {
    false
}

/// `BreathAirGoal.findAirPosition`：从脚往上 8 格找第一格给气的方块，否则用 +8 的备点，
/// 再让导航游过去（+1 格）。
///
/// `BreathAirGoal.findAirPosition`: the first breathable block in the eight-block
/// column above, or a +8 fallback, then navigate there (one block up).
fn breath_destination(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    let view = ctx.blocks();
    let base = me.block_position();
    let mut destination = None;
    for y in base.y..=base.y + 8 {
        let pos = BlockPos::new(base.x, y, base.z);
        if !view.fluid_state(pos).is_water() && view.block_state(pos).is_pathfindable_land() {
            destination = Some(pos);
            break;
        }
    }
    let target = destination.unwrap_or(BlockPos::new(base.x, base.y + 8, base.z));
    let nav = living::nav_mob(me, mob, view);
    mob.navigation.move_to(
        view,
        &nav,
        Vec3::new(target.x as f64, target.y as f64 + 1.0, target.z as f64),
        1.0,
    );
}

/// `TryFindLiquidGoal.start`：5×5×3 里找第一格水，`setWantedPosition(..., 1.0)`。
///
/// `TryFindLiquidGoal.start`: the first water block in the 5×5×3 neighbourhood,
/// `setWantedPosition(..., 1.0)`.
fn find_liquid_start(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    let view = ctx.blocks();
    let base = me.block_position();
    for dy in -2..=0 {
        for dx in -2..=2 {
            for dz in -2..=2 {
                let pos = BlockPos::new(base.x + dx, base.y + dy, base.z + dz);
                if view.fluid_state(pos).is_water() {
                    mob.mover
                        .set_wanted_position(pos.x as f64, pos.y as f64, pos.z as f64, 1.0);
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::{blocks, registries::mob_effect};

    use super::*;
    use crate::{
        BlockState, BlockTicks, World, entity::Entity, entity::ctx::PlayerView,
        entity::mob::MobType, testing::TestLevel,
    };

    /// 一名在 (x, y, z) 游泳的玩家。
    fn swimmer(uid: EntityUid, position: Vec3) -> PlayerView {
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
            game_mode: 0,
            mining_fatigue: None,
            creative: false,
            sneaking: false,
            sprinting: true,
            swimming: true,
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

    /// 一圈水，海豚在原点、玩家在 6 格外的游泳状态：目标是启动、给一次恩惠、
    /// 把导航指向玩家。
    ///
    /// Water all around, a dolphin at the origin and a swimming player six blocks
    /// away: the goal starts, grants one grace and points the navigation at the player.
    #[test]
    fn a_swimming_player_starts_the_goal_and_gets_grace() {
        let mut level = TestLevel::new();
        for x in -4..=10 {
            for y in 60..=68 {
                for z in -3..=3 {
                    level.set(BlockPos::new(x, y, z), BlockState(blocks::WATER));
                }
            }
        }
        level.players.push(swimmer(77, Vec3::new(6.5, 64.0, 0.5)));
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let mut dolphin = Entity::by_mob_type(MobType::Dolphin, Vec3::new(0.5, 64.0, 0.5));
        let outcome = dolphin
            .with_mob(|me, mob| {
                mob.air_supply = 4800;
                me.in_water = true;
                let mut ctx = crate::entity::ctx::PreciseCtx::new(&mut world, me.id);
                let mut goal = DolphinGoal::SwimWithPlayer { player: None };
                let started = goal.can_use(me, mob, &mut ctx);
                goal.start(me, mob, &mut ctx);
                goal.tick(me, mob, &mut ctx);
                (started, mob.look.wanted_x, mob.navigation.is_done())
            })
            .expect("a dolphin is a mob");
        assert!(
            outcome.0,
            "a swimming player within ten blocks starts the goal"
        );
        assert_eq!(outcome.1, 6.5, "the look control aims at the player");
        assert!(!outcome.2, "the navigation has a path towards the player");
        let effects = world.into_effects();
        let graces: Vec<_> = effects
            .iter()
            .filter(|effect| {
                matches!(
                    effect,
                    crate::Effect::PlayerEffect {
                        effect,
                        duration: 100,
                        amplifier: 0,
                        ..
                    } if *effect == mob_effect::DOLPHINS_GRACE
                )
            })
            .collect();
        // `start` 给一次；`tick` 里 `nextInt(6)` 在测试视图恒为 0，所以再给一次。
        // One from `start`; the test view always answers 0 to `nextInt(6)`, so `tick`
        // grants another.
        assert_eq!(graces.len(), 2, "{effects:?}");
    }

    /// 不游泳的玩家不触发目标：`canUse` 要求 `isSwimming()`。
    ///
    /// A player who is not swimming does not start the goal: `canUse` requires
    /// `isSwimming()`.
    #[test]
    fn a_walking_player_does_not_start_the_goal() {
        let mut level = TestLevel::new();
        level.players.push(PlayerView {
            swimming: false,
            sprinting: false,
            ..swimmer(78, Vec3::new(6.5, 64.0, 0.5))
        });
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let mut dolphin = Entity::by_mob_type(MobType::Dolphin, Vec3::new(0.5, 64.0, 0.5));
        let started = dolphin
            .with_mob(|me, mob| {
                let mut ctx = crate::entity::ctx::PreciseCtx::new(&mut world, me.id);
                let mut goal = DolphinGoal::SwimWithPlayer { player: None };
                goal.can_use(me, mob, &mut ctx)
            })
            .expect("a dolphin is a mob");
        assert!(!started);
    }
}
