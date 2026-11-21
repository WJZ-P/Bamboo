//! 兔子的品种、移动控制和胡萝卜目标；存活、繁殖与寻路共用生物框架。
//!
//! Rabbit variants, movement controls and carrot raiding; life cycle and
//! navigation use the shared mob machinery.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{
    blocks, component_enums::sparse, entity_types, registries::attribute, tags::block_tags,
};

use super::{
    Mob, MobEvent, MobType,
    attributes::{Attribute, Attributes},
    avoid::{AvoidClass, AvoidEntity},
    control::Operation,
    goals::{Goal, GoalSelector, WrappedGoal, reduced_tick_delay, within_home},
    living,
    navigation::Navigation,
};
use crate::{
    entity::{
        Entity,
        ctx::{BlockView, Command, EntityCtx},
    },
    shape::CollisionContext,
    state::{AGE_7, BlockState},
    world::{UPDATE_CLIENTS, World},
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    #[default]
    Brown,
    White,
    Black,
    WhiteSplotched,
    Gold,
    Salt,
    Evil,
}

impl Variant {
    pub const fn id(self) -> i32 {
        sparse::RABBIT_VARIANT[self as usize]
    }

    pub fn from_id(id: i32) -> Self {
        [
            Self::Brown,
            Self::White,
            Self::Black,
            Self::WhiteSplotched,
            Self::Gold,
            Self::Salt,
            Self::Evil,
        ]
        .into_iter()
        .find(|variant| variant.id() == id)
        .unwrap_or_default()
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Rabbit {
    pub variant: Variant,
    pub more_carrot_ticks: i32,
    jump_ticks: i32,
    jump_duration: i32,
    was_on_ground: bool,
    jump_delay_ticks: i32,
    next_jump_speed: f64,
}

pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (
            entity_types::RABBIT_BABY_WIDTH,
            entity_types::RABBIT_BABY_HEIGHT,
            entity_types::RABBIT_BABY_EYE_HEIGHT,
        )
    } else {
        let facts = &entity_types::FACTS[MobType::Rabbit.entity_type() as usize];
        (facts.width, facts.height, entity_types::RABBIT_EYE_HEIGHT)
    }
}

pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    table.set_base(attribute::MAX_HEALTH, entity_types::RABBIT_MAX_HEALTH);
    table.set_base(
        attribute::MOVEMENT_SPEED,
        entity_types::RABBIT_MOVEMENT_SPEED,
    );
    table.entries.push(Attribute::new(
        attribute::ATTACK_DAMAGE,
        entity_types::RABBIT_ATTACK_DAMAGE,
    ));
    table
}

pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.can_float = true;
    let mut selector = GoalSelector::default();
    selector.goals = [
        (1, Goal::Float),
        (1, Goal::RabbitPowderSnow),
        (
            1,
            Goal::Panic {
                speed: 2.2,
                target: Vec3::default(),
            },
        ),
        (
            2,
            Goal::Breed {
                speed: 0.8,
                partner: None,
                love_time: 0,
            },
        ),
        (
            3,
            Goal::Tempt {
                speed: 1.0,
                player: None,
                calm_down: 0,
                steering_item: false,
            },
        ),
        (
            4,
            Goal::Avoid(AvoidEntity::new(AvoidClass::Player, 8.0, 2.2, 2.2)),
        ),
        (
            4,
            Goal::Avoid(AvoidEntity::new(AvoidClass::Wolf, 10.0, 2.2, 2.2)),
        ),
        (
            4,
            Goal::Avoid(AvoidEntity::new(AvoidClass::Monster, 4.0, 2.2, 2.2)),
        ),
        (5, Goal::RabbitRaid(RaidGarden::default())),
        (
            6,
            Goal::Stroll {
                speed: 0.6,
                interval: 120,
                probability: 0.001,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        (
            11,
            Goal::LookAtPlayer {
                look_distance: 10.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
    ]
    .into_iter()
    .map(|(priority, goal)| WrappedGoal {
        priority,
        goal,
        running: false,
    })
    .collect();
    selector
}

/// `getVariant() == EVIL`。
pub(super) fn is_evil(mob: &Mob) -> bool {
    mob.mob_type == MobType::Rabbit && mob.rabbit.variant == Variant::Evil
}

pub(super) fn random_variant(biome: i32, random: &crate::random::LegacyRandom) -> Variant {
    let roll = random.next_int(100);
    if super::spawn::biome_tag_contains("minecraft:spawns_white_rabbits", biome) {
        if roll < 80 {
            Variant::White
        } else {
            Variant::WhiteSplotched
        }
    } else if super::spawn::biome_tag_contains("minecraft:spawns_gold_rabbits", biome) {
        Variant::Gold
    } else if roll < 50 {
        Variant::Brown
    } else if roll < 90 {
        Variant::Salt
    } else {
        Variant::Black
    }
}

pub(super) fn offspring_variant(
    me: &mut Entity,
    father: Variant,
    mother: Variant,
    world: &mut World,
) -> Variant {
    let biome = world.biome_id(me.block_position());
    let biome_variant = random_variant(biome, world.random());
    if me.random.next_int_bounded(20) == 0 {
        biome_variant
    } else if me.random.next_boolean() {
        mother
    } else {
        father
    }
}

/// 构造函数里的 `setSpeedModifier(0.0)`：想去的点还是原点，但操作已经是 `MOVE_TO`，
/// 第一刻 `MoveControl.tick` 会朝原点转身。
///
/// The constructor's `setSpeedModifier(0.0)`: the wanted point stays at the
/// origin but the operation becomes `MOVE_TO`, so the first `MoveControl.tick`
/// turns the rabbit towards the origin.
pub(super) fn initial_move_control() -> super::control::MoveControl {
    let mut mover = super::control::MoveControl::default();
    mover.set_wanted_position(0.0, 0.0, 0.0, 0.0);
    mover
}

pub(super) fn set_wanted(me: &Entity, mob: &mut Mob, wanted: Vec3, speed: f64) {
    let speed = if me.in_water { 1.5 } else { speed };
    mob.mover
        .set_wanted_position(wanted.x, wanted.y, wanted.z, speed);
    if speed > 0.0 {
        mob.rabbit.next_jump_speed = speed;
    }
}

pub(super) fn set_speed_modifier(me: &Entity, mob: &mut Mob, speed: f64) {
    mob.navigation.speed_modifier = speed;
    let wanted = wanted_position(mob);
    set_wanted(me, mob, wanted, speed);
}

fn wanted_position(mob: &Mob) -> Vec3 {
    Vec3::new(mob.mover.wanted_x, mob.mover.wanted_y, mob.mover.wanted_z)
}

fn next_path_position(me: &Entity, mob: &Mob) -> Option<Vec3> {
    mob.navigation
        .path
        .as_ref()
        .filter(|path| !path.is_done())
        .map(|path| path.next_entity_pos(me.dimensions.0))
}

pub(super) fn custom_server_ai_step(me: &mut Entity, mob: &mut Mob) {
    if mob.rabbit.jump_delay_ticks > 0 {
        mob.rabbit.jump_delay_ticks -= 1;
    }
    if mob.rabbit.more_carrot_ticks > 0 {
        mob.rabbit.more_carrot_ticks =
            (mob.rabbit.more_carrot_ticks - me.random.next_int_bounded(3)).max(0);
    }
    if me.on_ground {
        if !mob.rabbit.was_on_ground {
            mob.jumping = false;
            mob.rabbit.jump_delay_ticks = if mob.mover.speed_modifier < 2.2 {
                10
            } else {
                3
            };
        }
        // 官方的 canJump 标记不参与 JumpControl.tick 的判定。
        // Vanilla's canJump marker is not consulted by JumpControl.tick.
        if !mob.jump.jump && mob.mover.has_wanted() && mob.rabbit.jump_delay_ticks == 0 {
            let at = next_path_position(me, mob).unwrap_or_else(|| wanted_position(mob));
            // `facePoint`：`180.0F / (float) Math.PI` 是折好的 float 常量。
            // `facePoint`: `180.0F / (float) Math.PI` is a folded float constant.
            me.y_rot = (mth::atan2(at.z - me.position.z, at.x - me.position.x)
                * 57.295_776_367_187_5) as f32
                - 90.0;
            start_jumping(me, mob);
        }
    }
    mob.rabbit.was_on_ground = me.on_ground;
}

pub(super) fn before_move_control(me: &Entity, mob: &mut Mob) {
    if me.on_ground && !mob.jumping && !mob.jump.jump {
        set_speed_modifier(me, mob, 0.0);
    } else if mob.mover.has_wanted() || mob.mover.operation == Operation::Jumping {
        set_speed_modifier(me, mob, mob.rabbit.next_jump_speed);
    }
}

pub(super) fn tick_jump_control(me: &mut Entity, mob: &mut Mob) {
    if mob.jump.jump {
        start_jumping(me, mob);
        mob.jump.jump = false;
    }
}

fn start_jumping(me: &mut Entity, mob: &mut Mob) {
    mob.jumping = true;
    let pitch = ((me.random.next_float() - me.random.next_float()) * 0.2 + 1.0) * 0.8;
    living::play_sound(
        me,
        mob,
        "minecraft:entity.rabbit.jump",
        mob.mob_type.sound_volume(),
        pitch,
    );
    mob.rabbit.jump_duration = 15;
    mob.rabbit.jump_ticks = 0;
}

pub(super) fn ai_step(mob: &mut Mob) {
    if mob.rabbit.jump_ticks != mob.rabbit.jump_duration {
        mob.rabbit.jump_ticks += 1;
    } else if mob.rabbit.jump_duration != 0 {
        mob.rabbit.jump_ticks = 0;
        mob.rabbit.jump_duration = 0;
        mob.jumping = false;
    }
}

pub(super) fn jump_power_multiplier(me: &Entity, mob: &Mob) -> f32 {
    let step = next_path_position(me, mob).is_some_and(|pos| pos.y > me.position.y + 0.5)
        || me.horizontal_collision
        || (mob.jumping && mob.mover.wanted_y > me.position.y + 0.5);
    let power = if step {
        0.5
    } else if mob.mover.speed_modifier <= 0.6 {
        0.2
    } else {
        0.3
    };
    power / 0.42
}

pub(super) fn after_ground_jump(me: &mut Entity, mob: &mut Mob) {
    if mob.mover.speed_modifier > 0.0 && me.delta.x * me.delta.x + me.delta.z * me.delta.z < 0.01 {
        let lift = if mob.is_baby() { 0.5 } else { 1.5 };
        living::move_relative(me, 0.1, Vec3::new(0.0, lift, 1.0));
    }
    mob.events.push(MobEvent::EntityEvent { event: 1 });
}

pub(super) fn can_climb_powder_snow(me: &Entity, mob: &Mob, view: &dyn BlockView) -> bool {
    if !mob.in_powder_snow && !mob.was_in_powder_snow {
        return false;
    }
    let above = me.block_position().above();
    let state = view.block_state(above);
    state.is(blocks::POWDER_SNOW)
        || crate::shape::block_collision_boxes(
            state,
            above,
            CollisionContext::Empty,
            view.moving_piston_shape(above).as_deref(),
            view.shulker_box_shape(above),
        )
        .is_empty()
}

/// `Rabbit.RaidGardenGoal`（`MoveToBlockGoal` 的子类，速度 0.7F，水平 16 格）。
///
/// `block_pos` 在目标结束后也保留，和官方的 `blockPos` 字段一样。
///
/// `Rabbit.RaidGardenGoal` (a `MoveToBlockGoal` with speed 0.7F and a
/// horizontal range of 16). `block_pos` survives the goal stopping, like the
/// vanilla `blockPos` field.
#[derive(Debug, Clone, PartialEq)]
pub struct RaidGarden {
    next_start_tick: i32,
    try_ticks: i32,
    block_pos: BlockPos,
    reached_target: bool,
    wants_to_raid: bool,
    can_raid: bool,
}

impl Default for RaidGarden {
    fn default() -> Self {
        Self {
            next_start_tick: 0,
            try_ticks: 0,
            block_pos: BlockPos::new(0, 0, 0),
            reached_target: false,
            wants_to_raid: false,
            can_raid: false,
        }
    }
}

impl RaidGarden {
    /// `RaidGardenGoal.canUse` → `MoveToBlockGoal.canUse`。
    pub(super) fn can_use(&mut self, me: &mut Entity, mob: &Mob, view: &dyn BlockView) -> bool {
        if self.next_start_tick <= 0 {
            if !view.block_game_rules().mob_griefing {
                return false;
            }
            self.can_raid = false;
            self.wants_to_raid = mob.rabbit.more_carrot_ticks <= 0;
        }
        if self.next_start_tick > 0 {
            self.next_start_tick -= 1;
            return false;
        }
        self.next_start_tick = reduced_tick_delay(200 + me.random.next_int_bounded(200));
        if !self.wants_to_raid {
            return false;
        }
        match garden_target(me.block_position(), mob, view) {
            Some(pos) => {
                self.block_pos = pos;
                self.can_raid = true;
                true
            }
            None => false,
        }
    }

    /// `RaidGardenGoal.canContinueToUse`：`canRaid` 为真时 `isValidTarget` 必然为假，
    /// 所以这个目标从不续跑，下一个整刻就停。
    ///
    /// `RaidGardenGoal.canContinueToUse`: `isValidTarget` is always false while
    /// `canRaid` holds, so the goal never continues past the next full tick.
    pub(super) const fn can_continue(&self) -> bool {
        false
    }

    /// `MoveToBlockGoal.start`。
    pub(super) fn start(&mut self, me: &mut Entity, mob: &mut Mob, view: &dyn BlockView) {
        move_to_crop(me, mob, view, self.block_pos);
        self.try_ticks = 0;
        let bound = me.random.next_int_bounded(1200) + 1200;
        // `maxStayTicks` 只在续跑检查里用，而这个目标从不续跑。
        // `maxStayTicks` only feeds the continuation check, which never passes.
        let _ = me.random.next_int_bounded(bound);
    }

    /// `RaidGardenGoal.tick` → `MoveToBlockGoal.tick`。
    pub(super) fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        let crop = self.block_pos.above();
        let centre = Vec3::new(
            f64::from(crop.x) + 0.5,
            f64::from(crop.y) + 0.5,
            f64::from(crop.z) + 0.5,
        );
        if me.position.distance_to_sqr(centre) < 1.0 {
            self.reached_target = true;
            self.try_ticks -= 1;
        } else {
            self.reached_target = false;
            self.try_ticks += 1;
            if self.try_ticks % 40 == 0 {
                move_to_crop(me, mob, ctx.blocks(), self.block_pos);
            }
        }
        mob.look.set_look_at(
            centre.x,
            f64::from(crop.y),
            centre.z,
            10.0,
            super::control::MAX_HEAD_X_ROT as f32,
        );
        if self.reached_target {
            if self.can_raid && ctx.blocks().block_state(crop).is(blocks::CARROTS) {
                ctx.emit(Command::RabbitEatCarrots { pos: crop }, me);
                mob.rabbit.more_carrot_ticks = 40;
            }
            self.can_raid = false;
            self.next_start_tick = 10;
        }
    }
}

fn move_to_crop(me: &Entity, mob: &mut Mob, view: &dyn BlockView, farmland: BlockPos) {
    let nav = living::nav_mob(me, mob, view);
    let target = Vec3::new(
        f64::from(farmland.x) + 0.5,
        f64::from(farmland.y + 1),
        f64::from(farmland.z) + 0.5,
    );
    mob.navigation
        .move_to(view, &nav, target, f64::from(0.7_f32));
}

fn garden_target(origin: BlockPos, mob: &Mob, view: &dyn BlockView) -> Option<BlockPos> {
    for dy in [-1, 0, -2] {
        for radius in 0..16 {
            for dx in alternating_offsets(0, radius) {
                let start_z = if dx.abs() == radius { 0 } else { radius };
                for dz in alternating_offsets(start_z, radius) {
                    let pos = origin.offset(dx, dy, dz);
                    if within_home(mob, pos)
                        && view.block_state(pos).is_tagged(block_tags::SUPPORTS_CROPS)
                    {
                        let crop = view.block_state(pos.above());
                        if crop.is(blocks::CARROTS) && crop.get(AGE_7) == 7 {
                            return Some(pos);
                        }
                    }
                }
            }
        }
    }
    None
}

fn alternating_offsets(start: i32, max: i32) -> impl Iterator<Item = i32> {
    std::iter::successors(Some(start), |&n| Some(if n > 0 { -n } else { 1 - n }))
        .take_while(move |&n| n <= max)
}

pub(crate) fn eat_carrots(world: &mut World, pos: BlockPos) {
    let state = world.block_state(pos);
    if !state.is(blocks::CARROTS) {
        return;
    }
    let age = state.get(AGE_7);
    if age == 0 {
        world.set_block(pos, BlockState::AIR, UPDATE_CLIENTS);
    } else {
        // 官方不看 `setBlock` 的返回值，`2001` 照发。
        // Vanilla ignores the `setBlock` result and always sends `2001`.
        let _ = world.set_block(pos, state.with(AGE_7, age - 1), UPDATE_CLIENTS);
        world.push_effect(crate::Effect::LevelEvent {
            event: 2001,
            pos,
            data: state.0 as i32,
            except_initiator: false,
        });
    }
}
