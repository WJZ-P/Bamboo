//! 猫：可驯服（复用 [`super::tame`]）、11 种花色与 2 种叫声变体、项圈染色与繁殖混色、
//! 潜行 / 冲刺姿势、扑杀兔子与陆地幼年海龟、躺床与坐方块目标。
//!
//! Cats: tamable (reusing [`super::tame`]), 11 coat variants and 2 sound variants,
//! collar dyeing and breeding colour mix, crouching / sprinting poses, hunting rabbits
//! and baby turtles on land, and lie-on-block / sit-on-block goals.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{
    blocks, entity_types,
    registries::{SYNCED_REGISTRIES, attribute},
    tags::{block_tags, item_tags},
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;

use super::{
    Mob, MobEvent, MobType,
    attributes::{Attribute, Attributes},
    avoid::{AvoidClass, AvoidEntity},
    goals::{self, Flag, Goal, GoalSelector, TemptScare, WrappedGoal, reduced_tick_delay},
    interact::{MobInteractRequest, MobInteraction},
    living,
    navigation::Navigation,
    ocelot::{CROUCH_SPEED_MOD, SPRINT_SPEED_MOD, WALK_SPEED_MOD},
    tame::{self, TameGoal},
    wolf,
};
use crate::{
    InteractionResult,
    entity::{
        Entity, EntityUid,
        ctx::{BlockView, EntityCtx},
    },
    random::RandomSource,
    state::{BED_PART, BedPart, LIT, item_is_tagged},
};

/// `Cat.DEFAULT_COLLAR_COLOR`：红色。
pub const DEFAULT_COLLAR: i32 = 14;

/// `cat_variant` 注册表里的条目名，下标即同步用的注册表 ID。
///
/// Entries of `minecraft:cat_variant`; the index is the synced registry id.
pub fn variant_names() -> &'static [&'static str] {
    synced_entries("minecraft:cat_variant")
}

/// `cat_sound_variant` 注册表里的条目名。
///
/// Entries of `minecraft:cat_sound_variant`.
pub fn sound_variant_names() -> &'static [&'static str] {
    synced_entries("minecraft:cat_sound_variant")
}

fn synced_entries(name: &str) -> &'static [&'static str] {
    SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == name)
        .map_or(&[], |registry| registry.entries)
}

fn id_of(names: &[&str], name: &str) -> i32 {
    names
        .iter()
        .position(|entry| *entry == name)
        .and_then(|id| i32::try_from(id).ok())
        .unwrap_or(0)
}

/// 品种名对应的注册表 ID；未知名字返回 `None`。
///
/// Registry id for a cat variant name, or `None` when unknown.
pub fn variant_id(name: &str) -> Option<i32> {
    let names = variant_names();
    names.contains(&name).then(|| id_of(names, name))
}

/// 叫声变体名对应的注册表 ID；未知名字返回 `None`。
///
/// Registry id for a cat sound variant name, or `None` when unknown.
pub fn sound_variant_id(name: &str) -> Option<i32> {
    let names = sound_variant_names();
    names.contains(&name).then(|| id_of(names, name))
}

/// `Cat.DEFAULT_VARIANT`（`minecraft:black`）的注册表 ID。
///
/// Registry id of `Cat.DEFAULT_VARIANT` (`minecraft:black`).
pub fn default_variant() -> i32 {
    id_of(variant_names(), "minecraft:black")
}

/// `CatSoundVariants.CLASSIC` 的注册表 ID。
///
/// Registry id of `CatSoundVariants.CLASSIC`.
pub fn default_sound_variant() -> i32 {
    id_of(sound_variant_names(), "minecraft:classic")
}

/// 猫自己的状态。
///
/// Cat-only state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cat {
    /// `DATA_VARIANT_ID`：`cat_variant` 注册表 ID。
    ///
    /// `DATA_VARIANT_ID`: registry id in `cat_variant`.
    pub variant: i32,
    /// `DATA_SOUND_VARIANT_ID`：`cat_sound_variant` 注册表 ID。
    ///
    /// `DATA_SOUND_VARIANT_ID`: registry id in `cat_sound_variant`.
    pub sound_variant: i32,
    /// `IS_LYING`：躺在床或主人身上。
    ///
    /// `IS_LYING`: lying on a bed or sleeping owner.
    pub lying: bool,
    /// `RELAX_STATE_ONE`：靠在熟睡主人身边的过渡态。
    ///
    /// `RELAX_STATE_ONE`: transition pose next to a sleeping owner.
    pub relax_state_one: bool,
    /// `DATA_COLLAR_COLOR`：`DyeColor` ID。
    ///
    /// `DATA_COLLAR_COLOR`: `DyeColor` id.
    pub collar: i32,
    /// `Pose.CROUCHING`：移动速度修饰符为 `0.6` 时置真，不存盘。
    ///
    /// `Pose.CROUCHING`: true while move control speed modifier is `0.6`; not saved.
    pub crouching: bool,
}

impl Default for Cat {
    fn default() -> Self {
        Self {
            variant: default_variant(),
            sound_variant: default_sound_variant(),
            lying: false,
            relax_state_one: false,
            collar: DEFAULT_COLLAR,
            crouching: false,
        }
    }
}

impl Cat {
    /// 品种注册名。
    ///
    /// Registered name of this cat's variant.
    pub fn variant_name(&self) -> &'static str {
        name_at(variant_names(), self.variant, "minecraft:black")
    }

    /// 叫声变体注册名。
    ///
    /// Registered name of this cat's sound variant.
    pub fn sound_variant_name(&self) -> &'static str {
        name_at(
            sound_variant_names(),
            self.sound_variant,
            "minecraft:classic",
        )
    }
}

fn name_at(names: &'static [&'static str], id: i32, fallback: &'static str) -> &'static str {
    usize::try_from(id)
        .ok()
        .and_then(|id| names.get(id))
        .copied()
        .unwrap_or(fallback)
}

/// 成年取 `EntityTypes.CAT`（0.6×0.7，眼高 0.35），幼年取 `Cat.BABY_DIMENSIONS`。
///
/// Adults use `EntityTypes.CAT` (0.6×0.7, eye height 0.35); kittens use `Cat.BABY_DIMENSIONS`.
pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (0.3, 0.35, 0.34375)
    } else {
        let facts = &entity_types::FACTS[MobType::Cat.entity_type() as usize];
        (facts.width, facts.height, 0.35)
    }
}

/// `Cat.createAttributes`：`MAX_HEALTH` 10、`MOVEMENT_SPEED` 0.3F、`ATTACK_DAMAGE` 3。
///
/// `Cat.createAttributes`: `MAX_HEALTH` 10, `MOVEMENT_SPEED` 0.3F, `ATTACK_DAMAGE` 3.
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    table.set_base(attribute::MAX_HEALTH, 10.0);
    table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.3_f32));
    table
        .entries
        .push(Attribute::new(attribute::ATTACK_DAMAGE, 3.0));
    table
}

/// `CatSoundVariant.CatSoundSet` 里的九种声音。
///
/// The nine sound slots of `CatSoundVariant.CatSoundSet`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Ambient,
    StrayAmbient,
    Hiss,
    Hurt,
    Death,
    Eat,
    BegForFood,
    Purr,
    Purreow,
}

/// 按幼年与叫声变体取对应音效名。幼猫不分 `classic` / `royal`，一律走 `entity.baby_cat.*`。
///
/// Look up the sound event name for age and sound variant. Kittens always use `entity.baby_cat.*`.
pub fn sound(mob: &Mob, kind: Sound) -> &'static str {
    if mob.is_baby() {
        return match kind {
            Sound::Ambient => "minecraft:entity.baby_cat.ambient",
            Sound::StrayAmbient => "minecraft:entity.baby_cat.stray_ambient",
            Sound::Hiss => "minecraft:entity.baby_cat.hiss",
            Sound::Hurt => "minecraft:entity.baby_cat.hurt",
            Sound::Death => "minecraft:entity.baby_cat.death",
            Sound::Eat => "minecraft:entity.baby_cat.eat",
            Sound::BegForFood => "minecraft:entity.baby_cat.beg_for_food",
            Sound::Purr => "minecraft:entity.baby_cat.purr",
            Sound::Purreow => "minecraft:entity.baby_cat.purreow",
        };
    }
    let royal = mob.cat.sound_variant_name() == "minecraft:royal";
    match (royal, kind) {
        (false, Sound::Ambient) => "minecraft:entity.cat.ambient",
        (false, Sound::StrayAmbient) => "minecraft:entity.cat.stray_ambient",
        (false, Sound::Hiss) => "minecraft:entity.cat.hiss",
        (false, Sound::Hurt) => "minecraft:entity.cat.hurt",
        (false, Sound::Death) => "minecraft:entity.cat.death",
        (false, Sound::Eat) => "minecraft:entity.cat.eat",
        (false, Sound::BegForFood) => "minecraft:entity.cat.beg_for_food",
        (false, Sound::Purr) => "minecraft:entity.cat.purr",
        (false, Sound::Purreow) => "minecraft:entity.cat.purreow",
        (true, Sound::Ambient) => "minecraft:entity.cat_royal.ambient",
        (true, Sound::StrayAmbient) => "minecraft:entity.cat_royal.stray_ambient",
        (true, Sound::Hiss) => "minecraft:entity.cat_royal.hiss",
        (true, Sound::Hurt) => "minecraft:entity.cat_royal.hurt",
        (true, Sound::Death) => "minecraft:entity.cat_royal.death",
        (true, Sound::Eat) => "minecraft:entity.cat_royal.eat",
        (true, Sound::BegForFood) => "minecraft:entity.cat_royal.beg_for_food",
        (true, Sound::Purr) => "minecraft:entity.cat_royal.purr",
        (true, Sound::Purreow) => "minecraft:entity.cat_royal.purreow",
    }
}

/// `Cat.getAmbientSound` 加 `makeSound`：驯服且未发情时抽 `nextInt(4) == 0` 选呼噜喵。
///
/// `Cat.getAmbientSound` plus `makeSound`: a tamed non-love cat rolls `nextInt(4) == 0` for purreow.
pub(super) fn play_ambient_sound(me: &mut Entity, mob: &mut Mob) {
    let kind = if mob.tame.is_tame() {
        if mob.in_love > 0 {
            Sound::Purr
        } else if me.random.next_int_bounded(4) == 0 {
            Sound::Purreow
        } else {
            Sound::Ambient
        }
    } else {
        Sound::StrayAmbient
    };
    let name = sound(mob, kind);
    living::make_sound(me, mob, name);
}

/// `Cat.playEatingSound`：音量 1、音高 1，不取随机数。
///
/// `Cat.playEatingSound`: volume 1, pitch 1, no random draw.
pub(super) fn play_eating_sound(me: &Entity, mob: &mut Mob) {
    let name = sound(mob, Sound::Eat);
    living::play_sound(me, mob, name, 1.0, 1.0);
}

// ---- 目标 ----

/// 猫专属的 `NonTameRandomTargetGoal` 目标种类。
///
/// Prey kinds for `Cat`'s `NonTameRandomTargetGoal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatTarget {
    Rabbit,
    BabyTurtleOnLand,
}

/// `MoveToBlockGoal` 的共享状态，供 `CatLieOnBlockGoal` 与 `CatSitOnBlockGoal` 复用。
///
/// Shared `MoveToBlockGoal` state reused by `CatLieOnBlockGoal` and `CatSitOnBlockGoal`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveToBlock {
    pub speed: f64,
    pub next_start_tick: i32,
    pub try_ticks: i32,
    pub max_stay_ticks: i32,
    pub block_pos: BlockPos,
    pub reached_target: bool,
    pub search_range: i32,
    pub vertical_search_range: i32,
    pub vertical_search_start: i32,
}

impl MoveToBlock {
    const fn new(
        speed: f64,
        search_range: i32,
        vertical_search_range: i32,
        vertical_search_start: i32,
    ) -> Self {
        Self {
            speed,
            next_start_tick: 0,
            try_ticks: 0,
            max_stay_ticks: 0,
            block_pos: BlockPos::new(0, 0, 0),
            reached_target: false,
            search_range,
            vertical_search_range,
            vertical_search_start,
        }
    }

    fn start(&mut self, me: &mut Entity, mob: &mut Mob, view: &dyn BlockView) {
        self.move_mob_to_block(me, mob, view);
        self.try_ticks = 0;
        let first = me.random.next_int_bounded(1200) + 1200;
        self.max_stay_ticks = me.random.next_int_bounded(first) + 1200;
    }

    fn move_mob_to_block(&self, me: &Entity, mob: &mut Mob, view: &dyn BlockView) {
        let nav = living::nav_mob(me, mob, view);
        mob.navigation.move_to(
            view,
            &nav,
            Vec3::new(
                f64::from(self.block_pos.x) + 0.5,
                f64::from(self.block_pos.y + 1),
                f64::from(self.block_pos.z) + 0.5,
            ),
            self.speed,
        );
    }

    fn tick(&mut self, me: &Entity, mob: &mut Mob, view: &dyn BlockView) {
        let target = self.block_pos.above();
        let center = Vec3::new(
            f64::from(target.x) + 0.5,
            f64::from(target.y) + 0.5,
            f64::from(target.z) + 0.5,
        );
        if me.position.distance_to_sqr(center) >= 1.0 {
            self.reached_target = false;
            self.try_ticks += 1;
            if self.try_ticks % 40 == 0 {
                let nav = living::nav_mob(me, mob, view);
                mob.navigation.move_to(
                    view,
                    &nav,
                    Vec3::new(
                        f64::from(target.x) + 0.5,
                        f64::from(target.y),
                        f64::from(target.z) + 0.5,
                    ),
                    self.speed,
                );
            }
        } else {
            self.reached_target = true;
            self.try_ticks -= 1;
        }
    }

    fn find_nearest(
        &mut self,
        me: &Entity,
        mob: &Mob,
        view: &dyn BlockView,
        is_valid: impl Fn(&dyn BlockView, BlockPos) -> bool,
    ) -> bool {
        let origin = me.block_position();
        let mut y = self.vertical_search_start;
        while y <= self.vertical_search_range {
            for r in 0..self.search_range {
                let mut x = 0;
                while x <= r {
                    let mut z = if x < r && x > -r { r } else { 0 };
                    while z <= r {
                        let pos = origin.offset(x, y - 1, z);
                        if goals::within_home(mob, pos) && is_valid(view, pos) {
                            self.block_pos = pos;
                            return true;
                        }
                        z = if z > 0 { -z } else { 1 - z };
                    }
                    x = if x > 0 { -x } else { 1 - x };
                }
            }
            y = if y > 0 { -y } else { 1 - y };
        }
        false
    }
}

/// 猫专属的四个目标：`CatTemptGoal`、`CatLieOnBlockGoal`、`CatSitOnBlockGoal` 和 `NonTameRandomTargetGoal`。
///
/// The four cat-specific goals: `CatTemptGoal`, `CatLieOnBlockGoal`, `CatSitOnBlockGoal`,
/// and `NonTameRandomTargetGoal`.
#[derive(Debug, Clone, PartialEq)]
pub enum CatGoal {
    /// `Cat.CatTemptGoal`：未驯服时被 `#cat_food` 吸引，随机记下 `selectedPlayer` 暂时不怕惊吓。
    ///
    /// `Cat.CatTemptGoal`: tempted by `#cat_food` while untamed, randomly picking a
    /// `selectedPlayer` who will not scare the cat.
    Tempt {
        player: Option<EntityUid>,
        selected_player: Option<EntityUid>,
        calm_down: i32,
        scare: TemptScare,
    },
    /// `CatLieOnBlockGoal`：驯服且未命令坐下时找 `#cats_can_lie_on` 躺下。
    ///
    /// `CatLieOnBlockGoal`: find `#cats_can_lie_on` to lie down when tamed and not ordered to sit.
    LieOnBlock(MoveToBlock),
    /// `CatSitOnBlockGoal`：驯服且未命令坐下时找 `#cats_can_sit_on`（箱子、点亮的熔炉、床尾）坐下。
    ///
    /// `CatSitOnBlockGoal`: find `#cats_can_sit_on` (closed chest, lit furnace, foot of bed).
    SitOnBlock(MoveToBlock),
    /// `NonTameRandomTargetGoal`：未驯服时以 `randomInterval = 5` 捕猎兔子或陆地幼年海龟。
    ///
    /// `NonTameRandomTargetGoal`: hunt rabbits or baby turtles on land while untamed.
    NonTameTarget {
        kind: CatTarget,
        found: Option<EntityUid>,
    },
}

impl CatGoal {
    pub const fn flags(&self) -> &'static [Flag] {
        match self {
            Self::Tempt { .. } => &[Flag::Move, Flag::Look],
            Self::LieOnBlock(_) | Self::SitOnBlock(_) => &[Flag::Jump, Flag::Move],
            Self::NonTameTarget { .. } => &[Flag::Target],
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::Tempt { .. } => "CatTemptGoal",
            Self::LieOnBlock(_) => "CatLieOnBlockGoal",
            Self::SitOnBlock(_) => "CatSitOnBlockGoal",
            Self::NonTameTarget { .. } => "NonTameRandomTargetGoal",
        }
    }

    pub const fn every_tick(&self) -> bool {
        matches!(self, Self::LieOnBlock(_) | Self::SitOnBlock(_))
    }

    pub(super) fn can_use(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
        match self {
            Self::Tempt {
                player, calm_down, ..
            } => {
                // `CatTemptGoal.canUse`：先跑 `super.canUse()`（含 `calmDown` 递减），再要求 `!isTame()`。
                // `CatTemptGoal.canUse`: runs `super.canUse()` (including `calmDown--`) before `!isTame()`.
                if *calm_down > 0 {
                    *calm_down -= 1;
                    return false;
                }
                *player = goals::nearest_tempting_player(me, mob, ctx, false);
                player.is_some() && !mob.tame.is_tame()
            }
            Self::LieOnBlock(move_to) => {
                if !mob.tame.is_tame() || mob.tame.ordered_to_sit || mob.cat.lying {
                    return false;
                }
                if move_to.next_start_tick > 0 {
                    move_to.next_start_tick -= 1;
                    return false;
                }
                // `CatLieOnBlockGoal.nextStartTick` 恒为 40，不取随机数。
                // `CatLieOnBlockGoal.nextStartTick` is always 40 with no random draw.
                move_to.next_start_tick = 40;
                move_to.find_nearest(me, mob, ctx.blocks(), is_valid_lie_target)
            }
            Self::SitOnBlock(move_to) => {
                if !mob.tame.is_tame() || mob.tame.ordered_to_sit {
                    return false;
                }
                if move_to.next_start_tick > 0 {
                    move_to.next_start_tick -= 1;
                    return false;
                }
                move_to.next_start_tick = reduced_tick_delay(200 + me.random.next_int_bounded(200));
                move_to.find_nearest(me, mob, ctx.blocks(), is_valid_sit_target)
            }
            Self::NonTameTarget { kind, found } => {
                if mob.tame.is_tame() {
                    return false;
                }
                if me.random.next_int_bounded(reduced_tick_delay(10)) != 0 {
                    return false;
                }
                *found = find_cat_prey(me, mob, ctx, *kind);
                found.is_some()
            }
        }
    }

    pub(super) fn can_continue(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &dyn EntityCtx,
    ) -> bool {
        match self {
            Self::Tempt {
                player,
                selected_player,
                scare,
                ..
            } => {
                // `CatTemptGoal.canScare`：只有当 `selectedPlayer == player` 时才不怕惊吓。
                // `CatTemptGoal.canScare`: false only while `selectedPlayer == player`.
                let can_scare = !(selected_player.is_some() && *selected_player == *player);
                if can_scare {
                    let Some(found) =
                        player.and_then(|uid| goals::player_by_uid(ctx.blocks(), uid))
                    else {
                        return false;
                    };
                    if me.position.distance_to_sqr(found.position) < 36.0 {
                        let prev = Vec3::new(scare.px, scare.py, scare.pz);
                        if found.position.distance_to_sqr(prev) > 0.010_000_000_000_000_002 {
                            return false;
                        }
                        if (f64::from(found.pitch) - scare.p_rot_x).abs() > 5.0
                            || (f64::from(found.yaw) - scare.p_rot_y).abs() > 5.0
                        {
                            return false;
                        }
                    } else {
                        scare.px = found.position.x;
                        scare.py = found.position.y;
                        scare.pz = found.position.z;
                    }
                    scare.p_rot_x = f64::from(found.pitch);
                    scare.p_rot_y = f64::from(found.yaw);
                }
                self.can_use(me, mob, ctx)
            }
            Self::LieOnBlock(move_to) => {
                move_to.try_ticks >= -move_to.max_stay_ticks
                    && move_to.try_ticks <= 1200
                    && is_valid_lie_target(ctx.blocks(), move_to.block_pos)
            }
            Self::SitOnBlock(move_to) => {
                move_to.try_ticks >= -move_to.max_stay_ticks
                    && move_to.try_ticks <= 1200
                    && is_valid_sit_target(ctx.blocks(), move_to.block_pos)
            }
            Self::NonTameTarget { kind, found } => {
                // `NonTameRandomTargetGoal.canContinueToUse`：用 `targetConditions.test` 检验当前目标。
                // `NonTameRandomTargetGoal.canContinueToUse`: re-tests `targetConditions` on `this.target`.
                let Some(uid) = *found else {
                    return false;
                };
                cat_prey_valid(me, mob, ctx, uid, *kind)
            }
        }
    }

    pub(super) fn start(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
        match self {
            Self::Tempt { player, scare, .. } => {
                if let Some(found) = player.and_then(|uid| goals::player_by_uid(ctx.blocks(), uid))
                {
                    scare.px = found.position.x;
                    scare.py = found.position.y;
                    scare.pz = found.position.z;
                }
            }
            Self::LieOnBlock(move_to) | Self::SitOnBlock(move_to) => {
                move_to.start(me, mob, ctx.blocks());
                mob.tame.set_in_sitting_pose(false);
            }
            Self::NonTameTarget { found, .. } => {
                mob.target = *found;
            }
        }
    }

    pub(super) fn stop(&mut self, mob: &mut Mob) {
        match self {
            Self::Tempt {
                player, calm_down, ..
            } => {
                *player = None;
                mob.navigation.stop();
                *calm_down = reduced_tick_delay(100);
            }
            Self::LieOnBlock(_) => {
                mob.cat.lying = false;
            }
            Self::SitOnBlock(_) => {
                mob.tame.set_in_sitting_pose(false);
            }
            Self::NonTameTarget { .. } => {
                mob.target = None;
            }
        }
    }

    pub(super) fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
        match self {
            Self::Tempt {
                player,
                selected_player,
                ..
            } => {
                if let Some(found) = player.and_then(|uid| goals::player_by_uid(ctx.blocks(), uid))
                {
                    mob.look.set_look_at(
                        found.position.x,
                        found.eye_y(),
                        found.position.z,
                        super::control::MAX_HEAD_Y_ROT as f32 + 20.0,
                        super::control::MAX_HEAD_X_ROT as f32,
                    );
                    if me.position.distance_to_sqr(found.position) < 6.25 {
                        mob.navigation.stop();
                    } else {
                        let nav = living::nav_mob(me, mob, ctx.blocks());
                        mob.navigation.move_to_entity(
                            ctx.blocks(),
                            &nav,
                            found.position,
                            CROUCH_SPEED_MOD,
                            1,
                        );
                    }
                }
                // `CatTemptGoal.tick`：不是每刻目标，`adjustedTickDelay` 折半成 300 / 250。
                // `CatTemptGoal.tick`: not every-tick, so `adjustedTickDelay` halves 600/500 to 300/250.
                if selected_player.is_none()
                    && me.random.next_int_bounded(reduced_tick_delay(600)) == 0
                {
                    *selected_player = *player;
                } else if me.random.next_int_bounded(reduced_tick_delay(500)) == 0 {
                    *selected_player = None;
                }
            }
            Self::LieOnBlock(move_to) => {
                move_to.tick(me, mob, ctx.blocks());
                mob.tame.set_in_sitting_pose(false);
                if !move_to.reached_target {
                    mob.cat.lying = false;
                } else if !mob.cat.lying {
                    mob.cat.lying = true;
                }
            }
            Self::SitOnBlock(move_to) => {
                move_to.tick(me, mob, ctx.blocks());
                mob.tame.set_in_sitting_pose(move_to.reached_target);
            }
            Self::NonTameTarget { .. } => {}
        }
    }
}

fn is_valid_lie_target(view: &dyn BlockView, pos: BlockPos) -> bool {
    view.block_state(pos.above()).is_air()
        && view.block_state(pos).is_tagged(block_tags::CATS_CAN_LIE_ON)
}

fn is_valid_sit_target(view: &dyn BlockView, pos: BlockPos) -> bool {
    if !view.block_state(pos.above()).is_air() {
        return false;
    }
    let state = view.block_state(pos);
    if !state.is_tagged(block_tags::CATS_CAN_SIT_ON) {
        return false;
    }
    // 箱子打开人数（`ChestBlockEntity.getOpenCount`）暂未暴露在 `BlockView` 上，按关闭处理。
    // `ChestBlockEntity.getOpenCount` is not exposed on `BlockView` yet; treat chests as closed.
    if state.is(blocks::CHEST) {
        return true;
    }
    if state.is(blocks::FURNACE) {
        return state.get(LIT);
    }
    if state.is_tagged(block_tags::BEDS) {
        return state.get(BED_PART) != BedPart::Head;
    }
    true
}

fn find_cat_prey(
    me: &Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    kind: CatTarget,
) -> Option<EntityUid> {
    goals::nearest_mob_target_with(me, mob, ctx, |public| match kind {
        CatTarget::Rabbit => public.mob_type == MobType::Rabbit,
        CatTarget::BabyTurtleOnLand => {
            public.mob_type == MobType::Turtle && public.baby && !public.in_water
        }
    })
}

fn cat_prey_valid(
    me: &Entity,
    mob: &mut Mob,
    ctx: &dyn EntityCtx,
    uid: EntityUid,
    kind: CatTarget,
) -> bool {
    let Some(other) = ctx.other(uid) else {
        return false;
    };
    let crate::entity::KindPublic::Mob(public) = &other.kind else {
        return false;
    };
    if other.removed || !public.is_alive() {
        return false;
    }
    let matches_kind = match kind {
        CatTarget::Rabbit => public.mob_type == MobType::Rabbit,
        CatTarget::BabyTurtleOnLand => {
            public.mob_type == MobType::Turtle && public.baby && !public.in_water
        }
    };
    if !matches_kind {
        return false;
    }
    let range = mob.attributes.value(attribute::FOLLOW_RANGE).max(2.0);
    if me.position.distance_to_sqr(other.position) > range * range {
        return false;
    }
    let (_, _, eye) = public.mob_type.dimensions(public.baby);
    let target_eye = Vec3::new(
        other.position.x,
        other.position.y + f64::from(eye),
        other.position.z,
    );
    living::has_line_of_sight(me, mob, ctx.blocks(), uid, target_eye)
}

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        running: false,
        goal,
    }
}

/// `Cat.registerGoals` 加上构造函数末尾的 `reassessTameGoals`。
///
/// `Cat.registerGoals` followed by the constructor's `reassessTameGoals`.
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.can_float = true;
    GoalSelector::from_goals(vec![
        wrap(1, Goal::Float),
        wrap(
            1,
            Goal::Panic {
                speed: 1.5,
                target: Vec3::default(),
            },
        ),
        wrap(2, Goal::Tame(TameGoal::Sit)),
        wrap(3, Goal::Unavailable("CatRelaxOnOwnerGoal")),
        wrap(
            4,
            Goal::Cat(CatGoal::Tempt {
                player: None,
                selected_player: None,
                calm_down: 0,
                scare: TemptScare::default(),
            }),
        ),
        wrap(
            5,
            Goal::Cat(CatGoal::LieOnBlock(MoveToBlock::new(1.1, 8, 6, -2))),
        ),
        wrap(6, Goal::Tame(TameGoal::follow_owner(1.0, 10.0, 5.0))),
        wrap(
            7,
            Goal::Cat(CatGoal::SitOnBlock(MoveToBlock::new(0.8, 8, 1, 0))),
        ),
        wrap(8, Goal::LeapAtTarget { yd: 0.3 }),
        wrap(
            9,
            Goal::OcelotAttack {
                target: None,
                attack_time: 0,
            },
        ),
        wrap(
            10,
            Goal::Breed {
                speed: WALK_SPEED_MOD,
                partner: None,
                love_time: 0,
            },
        ),
        wrap(
            11,
            Goal::Stroll {
                speed: WALK_SPEED_MOD,
                interval: 120,
                probability: 1.000_000_1e-5,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        wrap(
            12,
            Goal::LookAtPlayer {
                look_distance: 10.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        avoid_players_goal(),
    ])
}

fn avoid_players_goal() -> WrappedGoal {
    wrap(
        4,
        Goal::Avoid(AvoidEntity::new(
            AvoidClass::Player,
            16.0,
            WALK_SPEED_MOD,
            SPRINT_SPEED_MOD,
        )),
    )
}

/// `Cat.registerGoals` 的 `targetSelector`：兔子和陆地幼年海龟（未驯服才启动）。
///
/// `Cat.registerGoals`'s `targetSelector`: rabbits and baby turtles on land (untamed only).
pub(super) fn target_goals() -> GoalSelector {
    GoalSelector::from_goals(vec![
        wrap(
            1,
            Goal::Cat(CatGoal::NonTameTarget {
                kind: CatTarget::Rabbit,
                found: None,
            }),
        ),
        wrap(
            1,
            Goal::Cat(CatGoal::NonTameTarget {
                kind: CatTarget::BabyTurtleOnLand,
                found: None,
            }),
        ),
    ])
}

/// `Cat.reassessTameGoals`：移掉旧的躲玩家目标，未驯服时重新追加到末尾。
///
/// `Cat.reassessTameGoals`: drop any existing avoid-players goal and re-append it when untamed.
pub(super) fn reassess_tame_goals(mob: &mut Mob) {
    mob.goals
        .remove_goals(|goal| matches!(goal, Goal::Avoid(_)));
    if !mob.tame.is_tame() {
        mob.goals.goals.push(avoid_players_goal());
    }
}

/// `Cat.customServerAiStep`：按移动控制的速度修饰符切潜行 / 疾跑 / 站立。
///
/// `Cat.customServerAiStep`: switch between crouching, sprinting and standing from move speed.
pub(super) fn custom_server_ai_step(mob: &mut Mob) {
    if mob.mover.has_wanted() {
        let speed = mob.mover.speed_modifier;
        if speed == CROUCH_SPEED_MOD {
            mob.cat.crouching = true;
            living::set_sprinting(mob, false);
        } else if speed == SPRINT_SPEED_MOD {
            mob.cat.crouching = false;
            living::set_sprinting(mob, true);
        } else {
            mob.cat.crouching = false;
            living::set_sprinting(mob, false);
        }
    } else {
        mob.cat.crouching = false;
        living::set_sprinting(mob, false);
    }
}

/// `Cat.tick` 在 `super.tick()` 之后：每 100 刻乞食声、每 5 刻躺卧呼噜声。
///
/// The part of `Cat.tick` after `super.tick()`: beg sound every 100 ticks and lying purr every 5 ticks.
pub(super) fn tick_after(me: &mut Entity, mob: &mut Mob) {
    let tempt_running =
        mob.goals.goals.iter().any(|wrapped| {
            wrapped.running && matches!(wrapped.goal, Goal::Cat(CatGoal::Tempt { .. }))
        });
    if tempt_running && !mob.tame.is_tame() && me.tick_count % 100 == 0 {
        let name = sound(mob, Sound::BegForFood);
        living::play_sound(me, mob, name, 1.0, 1.0);
    }
    if (mob.cat.lying || mob.cat.relax_state_one) && me.tick_count % 5 == 0 {
        let volume = 0.6 + 0.4 * (me.random.next_float() - me.random.next_float());
        let name = sound(mob, Sound::Purr);
        living::play_sound(me, mob, name, volume, 1.0);
    }
}

/// `Cat.removeWhenFarAway`：未驯服且存活超过 2400 刻才允许远离消失。
///
/// `Cat.removeWhenFarAway`: only untamed cats older than 2400 ticks despawn.
pub(super) const fn removes_when_far_away(mob: &Mob, tick_count: i32) -> bool {
    !mob.tame.is_tame() && tick_count > 2400
}

// ---- 生成、繁殖与交互 ----

/// `Cat.finalizeSpawn`：先跑 `AgeableMob.finalizeSpawn`（缺省 5% 幼崽），再按月相选花色
/// 并随机挑一个叫声变体。
///
/// `Cat.finalizeSpawn`: run `AgeableMob.finalizeSpawn` (default 5% baby chance), pick the
/// variant using moon brightness, and pick a random sound variant.
pub(super) fn finalize_spawn(
    entity: &mut Entity,
    random: &dyn RandomSource,
    moon_brightness: f32,
    group: &mut Option<super::spawn::SpawnGroup>,
) {
    super::spawn::finalize_ageable(entity, random, group, 0.05);
    let names = variant_names();
    let all_black = id_of(names, "minecraft:all_black");
    let full_moon = moon_brightness >= 0.9;
    let candidates: Vec<i32> = (0..names.len() as i32)
        .filter(|&id| full_moon || id != all_black)
        .collect();
    let variant = if candidates.is_empty() {
        default_variant()
    } else {
        let pick = random.next_int(candidates.len() as i32) as usize;
        candidates[pick]
    };
    let sound_count = i32::try_from(sound_variant_names().len()).unwrap_or(1);
    let sound_variant = random.next_int(sound_count);
    entity.with_mob(|_, mob| {
        mob.cat.variant = variant;
        mob.cat.sound_variant = sound_variant;
    });
}

/// `Cat.canMate`：双方都已驯服。
///
/// `Cat.canMate`: both cats must be tamed.
pub(super) fn can_mate(mob: &Mob, partner: &super::MobPublic) -> bool {
    mob.mob_type != MobType::Cat
        || (mob.tame.is_tame() && partner.tame_flags & tame::FLAG_TAME != 0)
}

/// `Cat.getBreedOffspring`：父亲随机源 `nextBoolean` 选品种；已驯服时继承主人并混合项圈色。
/// 官方不在此重抽 `sound_variant`（幼猫保持默认 `classic`）。
///
/// `Cat.getBreedOffspring`: father's random picks a parent's variant; if tamed, copies owner
/// and mixes collar dyes. Vanilla does not reroll `sound_variant` here.
pub(crate) fn offspring(
    me: &mut Entity,
    father: (&Cat, &tame::Tamable),
    mother: Option<Cat>,
    world: &mut crate::world::World,
) -> (Cat, tame::Tamable) {
    let (father_cat, father_tame) = father;
    let mother_variant = mother.map_or(father_cat.variant, |cat| cat.variant);
    let mut cat = Cat {
        variant: if me.random.next_boolean() {
            father_cat.variant
        } else {
            mother_variant
        },
        ..Cat::default()
    };
    let mut tamable = tame::Tamable::default();
    if father_tame.is_tame() {
        tamable.owner = father_tame.owner;
        tamable.flags |= tame::FLAG_TAME;
        let mother_collar = mother.map_or(father_cat.collar, |cat| cat.collar);
        cat.collar = wolf::mixed_dye(world, father_cat.collar, mother_collar);
    }
    (cat, tamable)
}

/// `Cat.isFood`：`#cat_food`。
///
/// `Cat.isFood`: `#cat_food`.
pub(super) fn is_food(stack: &ItemStack) -> bool {
    !stack.is_empty() && item_is_tagged(stack.item, item_tags::CAT_FOOD)
}

/// `Cat.mobInteract`：已驯服时主人可染项圈、喂鱼回血（1×营养或 1）或切换坐下；未驯服时
/// 喂鱼有 1/3 概率驯服并立刻坐下、标持久化并播吃食声。
///
/// `Cat.mobInteract`: owners can dye the collar, heal with fish (1×nutrition or 1), or toggle
/// sitting; untamed cats roll 1/3 to tame on fish, persist, and play the eating sound.
pub(super) fn interact(me: &mut Entity, mob: &mut Mob, req: &MobInteractRequest) -> MobInteraction {
    let held = &req.held;
    let consume = !req.creative;
    if mob.tame.is_tame() {
        if mob.tame.is_owned_by(Some(req.player_uuid)) {
            if !held.is_empty() && item_is_tagged(held.item, item_tags::CAT_COLLAR_DYES) {
                if let Some(color) = wolf::dye_of(held)
                    && color != mob.cat.collar
                {
                    mob.cat.collar = color;
                    mob.persistence_required = true;
                    return outcome(InteractionResult::Success, consume);
                }
            } else if is_food(held) {
                let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
                if mob.health < max {
                    let heal = wolf::nutrition_of(held).map_or(1.0, |n| n as f32);
                    if mob.health > 0.0 {
                        mob.health = (mob.health + heal).clamp(0.0, max);
                    }
                    play_eating_sound(me, mob);
                    return outcome(InteractionResult::Success, consume);
                }
            }
            let parent = super::interact::animal_interact(me, mob, req);
            if !parent.result.consumes_action() {
                mob.tame.ordered_to_sit = !mob.tame.ordered_to_sit;
                return outcome(InteractionResult::Success, false);
            }
            return parent;
        }
    } else if is_food(held) {
        let tamed = me.random.next_int_bounded(3) == 0;
        if tamed {
            tame::tame(mob, req.player_uuid);
            mob.tame.ordered_to_sit = true;
            mob.events.push(MobEvent::EntityEvent { event: 7 });
        } else {
            mob.events.push(MobEvent::EntityEvent { event: 6 });
        }
        mob.persistence_required = true;
        play_eating_sound(me, mob);
        return MobInteraction {
            tamed,
            ..outcome(InteractionResult::Success, consume)
        };
    }
    let result = super::interact::animal_interact(me, mob, req);
    if result.result.consumes_action() {
        mob.persistence_required = true;
    }
    result
}

fn outcome(result: InteractionResult, shrink_held: bool) -> MobInteraction {
    MobInteraction {
        result,
        shrink_held,
        ..MobInteraction::pass()
    }
}

/// `Cat.addAdditionalSaveData`：写 `"variant"`、`"sound_variant"`、`"CollarColor"`。
///
/// `Cat.addAdditionalSaveData`: write `"variant"`, `"sound_variant"`, and `"CollarColor"`.
pub(super) fn write_nbt(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    out.push((
        "variant".to_owned(),
        Tag::String(mob.cat.variant_name().to_owned()),
    ));
    out.push((
        "sound_variant".to_owned(),
        Tag::String(mob.cat.sound_variant_name().to_owned()),
    ));
    out.push(("CollarColor".to_owned(), Tag::Byte(mob.cat.collar as i8)));
}

/// `Cat.readAdditionalSaveData`：读 `"variant"`、`"sound_variant"`、`"CollarColor"`。
///
/// `Cat.readAdditionalSaveData`: read `"variant"`, `"sound_variant"`, and `"CollarColor"`.
pub(super) fn read_nbt(mob: &mut Mob, tag: &Tag) {
    use crate::archive::tag;
    if let Some(variant) = tag::string_or(tag, "variant").and_then(variant_id) {
        mob.cat.variant = variant;
    }
    if let Some(sound) = tag::string_or(tag, "sound_variant").and_then(sound_variant_id) {
        mob.cat.sound_variant = sound;
    }
    mob.cat.collar = match tag::field(tag, "CollarColor") {
        Some(Tag::Byte(id)) if (0..16).contains(id) => i32::from(*id),
        Some(Tag::Byte(_)) => 0,
        _ => DEFAULT_COLLAR,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_attributes_and_defaults_match_vanilla() {
        assert_eq!(dimensions(true), (0.3, 0.35, 0.34375));
        assert_eq!(dimensions(false), (0.6, 0.7, 0.35));

        let attrs = attributes();
        assert_eq!(attrs.value(attribute::MAX_HEALTH), 10.0);
        assert_eq!(attrs.value(attribute::MOVEMENT_SPEED), f64::from(0.3_f32));
        assert_eq!(attrs.value(attribute::ATTACK_DAMAGE), 3.0);

        let cat = Cat::default();
        assert_eq!(cat.variant_name(), "minecraft:black");
        assert_eq!(cat.sound_variant_name(), "minecraft:classic");
        assert_eq!(cat.collar, DEFAULT_COLLAR);
    }

    #[test]
    fn taming_removes_and_restores_avoid_players_goal() {
        let mut mob = Mob::new(MobType::Cat);
        assert!(
            mob.goals
                .goals
                .iter()
                .any(|w| matches!(w.goal, Goal::Avoid(_)))
        );
        tame::set_tame(&mut mob, true, true);
        assert!(mob.tame.is_tame());
        assert!(
            !mob.goals
                .goals
                .iter()
                .any(|w| matches!(w.goal, Goal::Avoid(_)))
        );
        tame::set_tame(&mut mob, false, true);
        assert!(!mob.tame.is_tame());
        assert!(matches!(
            mob.goals.goals.last().map(|w| &w.goal),
            Some(Goal::Avoid(_))
        ));
    }

    #[test]
    fn save_data_round_trips_variant_sound_and_collar() {
        let mut mob = Mob::new(MobType::Cat);
        mob.cat.variant = variant_id("minecraft:calico").unwrap();
        mob.cat.sound_variant = sound_variant_id("minecraft:royal").unwrap();
        mob.cat.collar = 11;
        let mut entries = Vec::new();
        write_nbt(&mob, &mut entries);
        let mut loaded = Mob::new(MobType::Cat);
        read_nbt(&mut loaded, &Tag::Compound(entries));
        assert_eq!(loaded.cat.variant_name(), "minecraft:calico");
        assert_eq!(loaded.cat.sound_variant_name(), "minecraft:royal");
        assert_eq!(loaded.cat.collar, 11);
    }
}
