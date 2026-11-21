//! 狐狸：品种、标志位、信任名单、叼在嘴里的物品，以及挂在共用生物框架上的钩子。
//! 狐狸专属的目标在 [`super::fox_goals`]。
//!
//! Foxes: variants, flags, the trusted list, the item in the mouth and the
//! hooks into the shared mob machinery. Fox-only goals live in
//! [`super::fox_goals`].

use mc_bamboo_core::{Aabb, BlockPos, Vec3, mth};
use mc_bamboo_data::{entity_types, items, registries::attribute};
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{
    Mob, MobEvent, MobType,
    attributes::{Attribute, Attributes},
    avoid::{AvoidClass, AvoidEntity, AvoidFilter},
    fox_goals::FoxGoal,
    goals::{Goal, GoalSelector, TargetType, WrappedGoal},
    living,
    navigation::Navigation,
};
use crate::entity::{
    Entity,
    ctx::{BlockView, Command, EntityCtx, PlayerView},
    store::KindPublic,
};

/// `Fox.FLAG_SITTING`。
pub const FLAG_SITTING: u8 = 1;
/// `Fox.FLAG_CROUCHING`。
pub const FLAG_CROUCHING: u8 = 4;
/// `Fox.FLAG_INTERESTED`。
pub const FLAG_INTERESTED: u8 = 8;
/// `Fox.FLAG_POUNCING`。
pub const FLAG_POUNCING: u8 = 16;
/// `Fox.FLAG_SLEEPING`。
pub const FLAG_SLEEPING: u8 = 32;
/// `Fox.FLAG_FACEPLANTED`。
pub const FLAG_FACEPLANTED: u8 = 64;
/// `Fox.FLAG_DEFENDING`。
pub const FLAG_DEFENDING: u8 = 128;

/// `Fox.Variant`：红狐和雪狐。
///
/// `Fox.Variant`: red and snow.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    #[default]
    Red,
    Snow,
}

impl Variant {
    pub const fn id(self) -> i32 {
        match self {
            Self::Red => 0,
            Self::Snow => 1,
        }
    }

    /// `ByIdMap.continuous(..., ZERO)`：越界读成红狐。
    pub const fn from_id(id: i32) -> Self {
        if id == 1 { Self::Snow } else { Self::Red }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Snow => "snow",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Red, Self::Snow]
            .into_iter()
            .find(|variant| variant.name() == name)
    }

    /// `Fox.Variant.byBiome`：`#spawns_snow_foxes` 里是雪狐。
    pub fn by_biome(biome: i32) -> Self {
        if super::spawn::biome_tag_contains("minecraft:spawns_snow_foxes", biome) {
            Self::Snow
        } else {
            Self::Red
        }
    }
}

/// 狐狸自己的状态。
///
/// The fox-only state.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Fox {
    pub variant: Variant,
    /// `DATA_FLAGS_ID`。
    pub flags: u8,
    /// `DATA_TRUSTED_ID_0` / `DATA_TRUSTED_ID_1`，存 UUID。
    pub trusted: [Option<[u8; 16]>; 2],
    /// `ticksSinceEaten`：不存盘。
    pub ticks_since_eaten: i32,
    /// `crouchAmount`：`isFullyCrouched` 要看它。
    pub crouch_amount: f32,
    /// `setTargetGoals` 跑过没有：官方把同一个目标对象加两次时第二次不生效。
    ///
    /// Whether `setTargetGoals` ran: adding the same goal object twice is a no-op.
    pub target_goals_set: bool,
}

impl Fox {
    pub const fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    pub fn set(&mut self, flag: u8, value: bool) {
        if value {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
    }

    pub const fn is_sleeping(&self) -> bool {
        self.has(FLAG_SLEEPING)
    }

    pub const fn is_defending(&self) -> bool {
        self.has(FLAG_DEFENDING)
    }

    /// `Fox.canMove`。
    pub const fn can_move(&self) -> bool {
        !self.has(FLAG_SLEEPING) && !self.has(FLAG_SITTING) && !self.has(FLAG_FACEPLANTED)
    }

    /// `Fox.isFullyCrouched`。
    pub fn is_fully_crouched(&self) -> bool {
        self.crouch_amount == 5.0
    }

    /// `Fox.clearStates`。
    pub fn clear_states(&mut self) {
        for flag in [
            FLAG_INTERESTED,
            FLAG_CROUCHING,
            FLAG_SITTING,
            FLAG_SLEEPING,
            FLAG_DEFENDING,
            FLAG_FACEPLANTED,
        ] {
            self.set(flag, false);
        }
    }

    /// `Fox.trusts`。
    pub fn trusts(&self, uuid: Option<[u8; 16]>) -> bool {
        uuid.is_some_and(|uuid| self.trusted.contains(&Some(uuid)))
    }

    /// `Fox.addTrustedEntity`：第一格有人时写进第二格。
    pub fn add_trusted(&mut self, uuid: [u8; 16]) {
        if self.trusted[0].is_some() {
            self.trusted[1] = Some(uuid);
        } else {
            self.trusted[0] = Some(uuid);
        }
    }
}

pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (
            entity_types::FOX_BABY_WIDTH,
            entity_types::FOX_BABY_HEIGHT,
            entity_types::FOX_BABY_EYE_HEIGHT,
        )
    } else {
        let facts = &entity_types::FACTS[MobType::Fox.entity_type() as usize];
        (facts.width, facts.height, entity_types::FOX_EYE_HEIGHT)
    }
}

/// `Fox.createAttributes`。
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    table.set_base(attribute::MAX_HEALTH, entity_types::FOX_MAX_HEALTH);
    table.set_base(attribute::MOVEMENT_SPEED, entity_types::FOX_MOVEMENT_SPEED);
    table.set_base(
        attribute::SAFE_FALL_DISTANCE,
        entity_types::FOX_SAFE_FALL_DISTANCE,
    );
    table.set_base(attribute::FOLLOW_RANGE, entity_types::FOX_FOLLOW_RANGE);
    table.entries.push(Attribute::new(
        attribute::ATTACK_DAMAGE,
        entity_types::FOX_ATTACK_DAMAGE,
    ));
    table
}

/// `Fox` 构造函数的导航设置和寻路代价：伤害方块不避，路径长度至少 32。
///
/// The `Fox` constructor's navigation and pathfinding malus settings.
pub(super) fn configure(mob: &mut Mob) {
    mob.malus
        .set(mc_bamboo_data::PathType::DamagingInNeighbor, 0.0);
    mob.malus.set(mc_bamboo_data::PathType::Damaging, 0.0);
    mob.can_pick_up_loot = true;
    mob.navigation.required_path_length = 32.0;
}

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        goal,
        running: false,
    }
}

/// `Fox.registerGoals` 的 goalSelector，按注册顺序。
///
/// The `Fox.registerGoals` goal selector, in registration order.
pub(super) fn goals(navigation: &mut Navigation, sleep_countdown: i32) -> GoalSelector {
    navigation.flags.can_float = true;
    let mut selector = GoalSelector::default();
    selector.goals = vec![
        wrap(0, Goal::Fox(FoxGoal::Float)),
        wrap(0, Goal::RabbitPowderSnow),
        wrap(1, Goal::Fox(FoxGoal::Faceplant { countdown: 0 })),
        wrap(
            2,
            Goal::Panic {
                speed: 2.2,
                target: Vec3::default(),
            },
        ),
        wrap(
            3,
            Goal::Breed {
                speed: 1.0,
                partner: None,
                love_time: 0,
            },
        ),
        wrap(4, avoid(AvoidClass::Player, 16.0, AvoidFilter::FoxPlayer)),
        wrap(4, avoid(AvoidClass::Wolf, 8.0, AvoidFilter::FoxWolf)),
        wrap(
            4,
            avoid(AvoidClass::PolarBear, 8.0, AvoidFilter::FoxDefending),
        ),
        wrap(5, Goal::Fox(FoxGoal::StalkPrey)),
        wrap(6, Goal::Fox(FoxGoal::Pounce)),
        wrap(6, Goal::Fox(FoxGoal::seek_shelter())),
        wrap(
            7,
            super::goals::melee_attack(1.2, true, "FoxMeleeAttackGoal"),
        ),
        wrap(
            7,
            Goal::Fox(FoxGoal::Sleep {
                countdown: sleep_countdown,
            }),
        ),
        wrap(
            8,
            Goal::FollowParent {
                speed: 1.25,
                parent: None,
                time_to_recalc_path: 0,
            },
        ),
        wrap(9, Goal::Fox(FoxGoal::StrollThroughVillage)),
        wrap(10, Goal::Fox(FoxGoal::eat_berries())),
        wrap(10, Goal::LeapAtTarget { yd: 0.4 }),
        wrap(
            11,
            Goal::Stroll {
                speed: 1.0,
                interval: 120,
                probability: 0.001,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        wrap(11, Goal::Fox(FoxGoal::SearchForItems)),
        wrap(
            12,
            Goal::LookAtPlayer {
                look_distance: 24.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        wrap(13, Goal::Fox(FoxGoal::perch())),
    ];
    selector
}

fn avoid(class: AvoidClass, max_dist: f32, filter: AvoidFilter) -> Goal {
    Goal::Avoid(AvoidEntity::new(class, max_dist, 1.6, 1.4).filtered(filter))
}

/// `Fox.registerGoals` 的 targetSelector：只有保护信任对象的那一个。
pub(super) fn target_goals() -> GoalSelector {
    let mut selector = GoalSelector::default();
    selector.goals = vec![wrap(3, Goal::Fox(FoxGoal::DefendTrusted { timestamp: 0 }))];
    selector
}

fn hunt(kind: TargetType, interval: i32) -> Goal {
    Goal::NearestAttackableTarget {
        kind,
        interval: super::goals::reduced_tick_delay(interval),
        unseen_ticks: 0,
        found: None,
    }
}

/// `Fox.setTargetGoals`：红狐先追陆上猎物，雪狐先追鱼。
/// 官方加的是同一批目标对象，第二次调用不会再加。
///
/// `Fox.setTargetGoals`: red foxes prefer land prey, snow foxes fish. The same
/// goal objects are added, so a second call changes nothing.
pub fn set_target_goals(mob: &mut Mob) {
    if mob.fox.target_goals_set {
        return;
    }
    mob.fox.target_goals_set = true;
    let land = hunt(TargetType::FoxPrey, 10);
    let turtle = hunt(TargetType::Turtle, 10);
    let fish = hunt(TargetType::Fish, 20);
    let added = if mob.fox.variant == Variant::Red {
        [(4, land), (4, turtle), (6, fish)]
    } else {
        [(4, fish), (6, land), (6, turtle)]
    };
    for (priority, goal) in added {
        mob.target_goals.goals.push(wrap(priority, goal));
    }
}

/// `Fox.finalizeSpawn`：群系定品种，`setTargetGoals`，再按世界随机源配嘴里的东西。
///
/// `Fox.finalizeSpawn`: biome variant, `setTargetGoals`, then the mouth item
/// from the level random source.
pub(super) fn finalize_spawn(mob: &mut Mob, biome: i32, random: &crate::random::LegacyRandom) {
    mob.fox.variant = Variant::by_biome(biome);
    set_target_goals(mob);
    if random.next_float() < 0.2 {
        let odds = random.next_float();
        let item = if odds < 0.05 {
            items::EMERALD
        } else if odds < 0.2 {
            items::EGG
        } else if odds < 0.4 {
            if random.next_boolean() {
                items::RABBIT_FOOT
            } else {
                items::RABBIT_HIDE
            }
        } else if odds < 0.6 {
            items::WHEAT
        } else if odds < 0.8 {
            items::LEATHER
        } else {
            items::FEATHER
        };
        mob.equipment
            .set(EquipmentSlot::MainHand, ItemStack::new(item, 1));
    }
}

// ---- 每刻的钩子 ----

/// `Fox.aiStep` 在 `super.aiStep()` 之前的那一段：嘴里有吃的就吃，没有目标就不蹲，
/// 睡觉时清掉输入。
///
/// The part of `Fox.aiStep` before `super.aiStep()`.
pub(super) fn ai_step_before(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if living::is_alive(me, mob) && !mob.no_ai {
        mob.fox.ticks_since_eaten += 1;
        let mouth = mob.equipment.get(EquipmentSlot::MainHand).clone();
        if can_eat(me, mob, &mouth) {
            if mob.fox.ticks_since_eaten > 600 {
                eat(me, mob, mouth);
                mob.fox.ticks_since_eaten = 0;
            } else if mob.fox.ticks_since_eaten > 560 && me.random.next_float() < 0.1 {
                living::play_sound(me, mob, "minecraft:entity.fox.eat", 1.0, 1.0);
                mob.events.push(MobEvent::EntityEvent { event: 45 });
            }
        }
        if super::goals::combat_target(mob, ctx).is_none() {
            mob.fox.set(FLAG_CROUCHING, false);
            mob.fox.set(FLAG_INTERESTED, false);
        }
    }
    if mob.fox.is_sleeping() || mob.is_dead_or_dying() {
        mob.jumping = false;
        mob.xxa = 0.0;
        mob.zza = 0.0;
    }
}

/// `Fox.aiStep` 在 `super.aiStep()` 之后：保护时 5% 叫一声。
pub(super) fn ai_step_after(me: &mut Entity, mob: &mut Mob) {
    if mob.fox.is_defending() && me.random.next_float() < 0.05 {
        living::play_sound(me, mob, "minecraft:entity.fox.aggro", 1.0, 1.0);
    }
}

/// `Fox.tick` 在 `super.tick()` 之后的那一段。
pub(super) fn tick_after(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if !mob.no_ai {
        let in_water = me.in_water;
        if in_water || mob.target.is_some() || ctx.blocks().is_thundering() {
            mob.fox.set(FLAG_SLEEPING, false);
        }
        if in_water || mob.fox.is_sleeping() {
            mob.fox.set(FLAG_SITTING, false);
        }
        if mob.fox.has(FLAG_FACEPLANTED) && ctx.blocks().level_random_float() < 0.2 {
            let pos = me.block_position();
            let state = ctx.blocks().block_state(pos);
            ctx.emit(
                Command::MobLevelEvent {
                    event: 2001,
                    pos,
                    data: state.0 as i32,
                },
                me,
            );
        }
    }
    if mob.fox.has(FLAG_CROUCHING) {
        mob.fox.crouch_amount = (mob.fox.crouch_amount + 0.2).min(5.0);
    } else {
        mob.fox.crouch_amount = 0.0;
    }
}

/// `Fox.setTarget`：保护状态下目标被清空时顺便结束保护。
pub(super) fn after_target_change(mob: &mut Mob) {
    if mob.target.is_none() && mob.fox.is_defending() {
        mob.fox.set(FLAG_DEFENDING, false);
    }
}

/// 是狐狸而且在保护信任对象（`FoxPanicGoal` / `FoxFollowParentGoal` 的前置条件）。
///
/// A fox defending a trusted entity (the guard of its panic and follow goals).
pub(super) fn is_defending_fox(mob: &Mob) -> bool {
    mob.mob_type == MobType::Fox && mob.fox.is_defending()
}

/// `FoxLookAtPlayerGoal`：趴倒或者感兴趣时不看人。
pub(super) fn blocks_look(mob: &Mob) -> bool {
    mob.mob_type == MobType::Fox && (mob.fox.has(FLAG_FACEPLANTED) || mob.fox.has(FLAG_INTERESTED))
}

/// `FoxMeleeAttackGoal.canUse` 的前置条件。
pub(super) fn blocks_melee(mob: &Mob) -> bool {
    mob.mob_type == MobType::Fox
        && (mob.fox.has(FLAG_SITTING)
            || mob.fox.is_sleeping()
            || mob.fox.has(FLAG_CROUCHING)
            || mob.fox.has(FLAG_FACEPLANTED))
}

/// 狐狸的目标在 `start` 里先 `clearStates`。
pub(super) fn clear_states_if_fox(mob: &mut Mob) {
    if mob.mob_type == MobType::Fox {
        mob.fox.clear_states();
    }
}

/// `FoxLookControl.resetXRotOnTick`。
pub(super) const fn resets_x_rot(mob: &Mob) -> bool {
    !mob.fox.has(FLAG_POUNCING)
        && !mob.fox.has(FLAG_CROUCHING)
        && !mob.fox.has(FLAG_INTERESTED)
        && !mob.fox.has(FLAG_FACEPLANTED)
}

// ---- 声音 ----

/// `Fox.getAmbientSound`：睡着打呼；夜里 10% 且 16 格内没有玩家时尖叫。
fn ambient_sound(me: &mut Entity, mob: &Mob, bright: bool, players: &[PlayerView]) -> &'static str {
    if mob.fox.is_sleeping() {
        return "minecraft:entity.fox.sleep";
    }
    if !bright && me.random.next_float() < 0.1 {
        let area = me.bounding_box().inflate(16.0, 16.0, 16.0);
        let alone = !players
            .iter()
            .any(|player| !player.spectator && player_box(player).intersects(area));
        if alone {
            return "minecraft:entity.fox.screech";
        }
    }
    "minecraft:entity.fox.ambient"
}

/// `Fox.playAmbientSound`：尖叫用 2 倍音量；否则走 `Mob.playAmbientSound`，
/// 它会再问一次 `getAmbientSound()`。
///
/// `Fox.playAmbientSound`: screeches are twice as loud; otherwise
/// `Mob.playAmbientSound` asks `getAmbientSound()` a second time.
pub(super) fn play_ambient_sound(me: &mut Entity, mob: &mut Mob, view: &dyn BlockView) {
    let mut players = Vec::new();
    view.for_each_player(&mut |player| players.push(*player));
    play_ambient_sound_with(me, mob, view.is_bright_outside(), &players);
}

/// [`play_ambient_sound`] 的本体：天色和玩家由调用方给。
///
/// The body of [`play_ambient_sound`], with the sky and players given.
pub(super) fn play_ambient_sound_with(
    me: &mut Entity,
    mob: &mut Mob,
    bright: bool,
    players: &[PlayerView],
) {
    let first = ambient_sound(me, mob, bright, players);
    if first == "minecraft:entity.fox.screech" {
        let pitch = living::voice_pitch(me, mob);
        living::play_sound(me, mob, first, 2.0, pitch);
        return;
    }
    let sound = ambient_sound(me, mob, bright, players);
    let pitch = living::voice_pitch(me, mob);
    living::play_sound(me, mob, sound, 1.0, pitch);
}

fn player_box(player: &PlayerView) -> Aabb {
    let height = if player.sneaking { 1.5_f32 } else { 1.8_f32 };
    let half = f64::from(0.6_f32) / 2.0;
    Aabb::new(
        player.position.x - half,
        player.position.y,
        player.position.z - half,
        player.position.x + half,
        player.position.y + f64::from(height),
        player.position.z + half,
    )
}

// ---- 嘴里的东西 ----

/// `Fox.isConsumableFood`：同时有 `food` 和 `consumable` 组件。
pub(super) fn is_consumable_food(stack: &ItemStack) -> bool {
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::components::{ComponentPatch, defaults};
    if stack.is_empty() {
        return false;
    }
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return false;
    };
    defaults::resolve(stack.item, &patch, kinds::FOOD).is_some()
        && defaults::resolve(stack.item, &patch, kinds::CONSUMABLE).is_some()
}

/// `Fox.canEat`。
fn can_eat(me: &Entity, mob: &Mob, stack: &ItemStack) -> bool {
    is_consumable_food(stack) && mob.target.is_none() && me.on_ground && !mob.fox.is_sleeping()
}

/// `Fox.canHoldItem`。
pub(super) fn can_hold(mob: &Mob, stack: &ItemStack) -> bool {
    let held = mob.equipment.get(EquipmentSlot::MainHand);
    held.is_empty()
        || (mob.fox.ticks_since_eaten > 0 && is_consumable_food(stack) && !is_consumable_food(held))
}

/// `ItemStack.finishUsingItem` 在狐狸身上：`Consumable.emitParticlesAndSounds` 和
/// `FoodProperties.onConsume` 的随机数和声音，然后减一个。
/// 吃完的效果（`on_consume_effects`）和 `use_remainder` 这一期不建模。
///
/// `ItemStack.finishUsingItem` on a fox: the draws and sounds of
/// `Consumable.emitParticlesAndSounds` and `FoodProperties.onConsume`, then
/// one item is consumed. Consume effects and `use_remainder` are not modelled.
fn eat(me: &mut Entity, mob: &mut Mob, mouth: ItemStack) {
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, defaults};
    let Ok(patch) = ComponentPatch::from_opaque(&mouth) else {
        return;
    };
    let Some(ComponentValue::Consumable {
        animation,
        sound,
        has_consume_particles,
        ..
    }) = defaults::resolve(mouth.item, &patch, kinds::CONSUMABLE).cloned()
    else {
        return;
    };
    let eat_volume = if me.random.next_boolean() { 0.5 } else { 1.0 };
    let eat_pitch = triangle(me, 1.0, 0.2);
    let drink_pitch = me.random.next_float() * (1.0 - 0.9) + 0.9;
    // `ItemUseAnimation.DRINK` 是第 2 项。
    // `ItemUseAnimation.DRINK` is index 2.
    let drink = animation == 2;
    if has_consume_particles {
        // `LivingEntity.spawnItemParticles(stack, 16)`：每颗四个 `nextFloat`。
        // `LivingEntity.spawnItemParticles(stack, 16)`: four `nextFloat`s each.
        for _ in 0..16 * 4 {
            me.random.next_float();
        }
    }
    let sound_name = match &sound {
        mc_bamboo_protocol::components::cursor::Holder::Id(id) => usize::try_from(*id)
            .ok()
            .and_then(|id| mc_bamboo_data::SOUND_EVENTS.get(id))
            .map_or("minecraft:entity.generic.eat", |def| def.name),
        mc_bamboo_protocol::components::cursor::Holder::Inline(_) => "minecraft:entity.generic.eat",
    };
    let (volume, pitch) = if drink {
        (0.5, drink_pitch)
    } else {
        (eat_volume, eat_pitch)
    };
    living::play_sound(me, mob, sound_name, volume, pitch);
    // `FoodProperties.onConsume`：`level.playSound(null, ...)`，音高 `triangle(1, 0.4)`。
    // `FoodProperties.onConsume`: a level sound pitched by `triangle(1, 0.4)`.
    let food_pitch = triangle(me, 1.0, 0.4);
    living::play_sound(me, mob, sound_name, 1.0, food_pitch);
    let mut rest = mouth;
    rest.count -= 1;
    if rest.count <= 0 {
        rest = ItemStack::empty();
    }
    mob.equipment.set(EquipmentSlot::MainHand, rest);
}

/// `RandomSource.triangle(mode, deviation)` 的 float 版本。
fn triangle(me: &mut Entity, mode: f32, deviation: f32) -> f32 {
    mode + deviation * (me.random.next_float() - me.random.next_float())
}

/// `Mob.aiStep` 的捡东西那一段在狐狸身上：`getPickupReach` 是 (1, 0, 1)，
/// 每件满足条件的物品交给 `Fox.pickUpItem`。
///
/// The `Mob.aiStep` pickup loop for a fox: reach (1, 0, 1), and each eligible
/// item goes to `Fox.pickUpItem`.
pub(super) fn pick_up_items(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if !mob.can_pick_up_loot
        || !living::is_alive(me, mob)
        || mob.dead
        || !ctx.blocks().block_game_rules().mob_griefing
    {
        return;
    }
    let area = me.bounding_box().inflate(1.0, 0.0, 1.0);
    let mut found = Vec::new();
    ctx.for_each_other_in(area, &mut |other| {
        if let KindPublic::Item {
            stack,
            pickup_delay,
            ..
        } = &other.kind
            && !other.removed
            && !stack.is_empty()
            && *pickup_delay <= 0
        {
            found.push((other.uid, stack.clone()));
        }
    });
    for (uid, stack) in found {
        if can_hold(mob, &stack) {
            pick_up(me, mob, ctx, uid, stack);
        }
    }
}

/// `Fox.pickUpItem`。
fn pick_up(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    item: crate::entity::EntityUid,
    stack: ItemStack,
) {
    let mut rest = None;
    if stack.count > 1 {
        let mut extra = stack.clone();
        extra.count = stack.count - 1;
        rest = Some(extra);
    }
    let old = mob.equipment.get(EquipmentSlot::MainHand).clone();
    if !old.is_empty() {
        living::play_sound(me, mob, "minecraft:entity.fox.spit", 1.0, 1.0);
    }
    let look = look_angle(me, mob);
    let mut held = stack;
    held.count = 1;
    mob.equipment.set(EquipmentSlot::MainHand, held);
    mob.equipment.set_drop_chance(EquipmentSlot::MainHand, 2.0);
    mob.fox.ticks_since_eaten = 0;
    ctx.emit(
        Command::FoxPickUpItem {
            item,
            rest,
            spit: (!old.is_empty()).then_some(old),
            spit_at: Vec3::new(
                me.position.x + look.x,
                me.position.y + 1.0,
                me.position.z + look.z,
            ),
        },
        me,
    );
}

/// [`Command::FoxPickUpItem`] 的落地，顺序照 `Fox.pickUpItem`：
/// `dropItemStack`（无拾取延迟）→ `spitOutItem`（延迟 40）→ `entity.discard()`。
///
/// Lands [`Command::FoxPickUpItem`] in `Fox.pickUpItem` order.
pub(crate) fn apply_pick_up(
    me: &Entity,
    world: &mut crate::world::World,
    item: crate::entity::EntityUid,
    rest: Option<ItemStack>,
    spit: Option<ItemStack>,
    spit_at: Vec3,
) {
    if let Some(rest) = rest {
        world.spawn_item_with_delay(me.position, rest, 0);
    }
    if let Some(spit) = spit {
        world.spawn_item_with_delay(spit_at, spit, 40);
    }
    world.with_entity_mut(item, &mut |entity| entity.discard());
}

/// [`Command::FoxPickBerries`] 的落地：`1 + level.random.nextInt(2)`，三龄再加一，
/// 嘴里空着的那一个已经叼走；剩下的 `popResource`，灌木回到一龄。
///
/// Lands [`Command::FoxPickBerries`].
pub(crate) fn apply_pick_berries(
    world: &mut crate::world::World,
    pos: BlockPos,
    age: i32,
    took_one: bool,
) {
    let mut count = 1 + world.random().next_int(2) + i32::from(age == 3);
    if took_one {
        count -= 1;
    }
    if count > 0 {
        world.pop_resource(pos, ItemStack::new(items::SWEET_BERRIES, count));
    }
    let state = world.block_state(pos);
    if state.is(mc_bamboo_data::blocks::SWEET_BERRY_BUSH) {
        let _ = world.set_block(
            pos,
            state.with(crate::state::AGE_3, 1),
            crate::world::UPDATE_CLIENTS,
        );
    }
}

/// [`Command::FoxPickGlowBerries`] 的落地：`CaveVines.use` 掉一颗发光浆果
/// （`harvest/cave_vine` 战利品表），取音高、出声，再去掉浆果。
///
/// Lands [`Command::FoxPickGlowBerries`] as `CaveVines.use`.
pub(crate) fn apply_pick_glow_berries(world: &mut crate::world::World, pos: BlockPos) {
    let state = world.block_state(pos);
    if state.try_get(crate::state::BERRIES) != Some(true) {
        return;
    }
    world.pop_resource(pos, ItemStack::new(items::GLOW_BERRIES, 1));
    let pitch = world.random().next_float() * (1.2 - 0.8) + 0.8;
    world.play_block_sound(
        pos,
        mc_bamboo_data::sound_event("minecraft:block.cave_vines.pick_berries"),
        1.0,
        pitch,
    );
    let _ = world.set_block(
        pos,
        state.with(crate::state::BERRIES, false),
        crate::world::UPDATE_CLIENTS,
    );
}

/// `Entity.getLookAngle` → `calculateViewVector(xRot, yRot)`。
fn look_angle(me: &Entity, mob: &Mob) -> Vec3 {
    let x_rad = mob.x_rot * (std::f32::consts::PI / 180.0);
    let y_rad = -me.y_rot * (std::f32::consts::PI / 180.0);
    let y_cos = mth::cos(f64::from(y_rad));
    let y_sin = mth::sin(f64::from(y_rad));
    let x_cos = mth::cos(f64::from(x_rad));
    let x_sin = mth::sin(f64::from(x_rad));
    Vec3::new(
        f64::from(y_sin * x_cos),
        f64::from(-x_sin),
        f64::from(y_cos * x_cos),
    )
}

/// `Fox.dropAllDeathLoot` 的前半：嘴里的东西先 `spawnAtLocation`，再清空槽位。
pub(super) fn drop_mouth_item(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let mouth = mob.equipment.get(EquipmentSlot::MainHand).clone();
    if mouth.is_empty() {
        return;
    }
    mob.equipment
        .set(EquipmentSlot::MainHand, ItemStack::empty());
    ctx.emit(Command::DropItem { stack: mouth }, me);
}

/// `Fox.isPathClear`：朝目标 6 段，每段头顶 1..3 格都得能被替换。
pub(super) fn is_path_clear(me: &Entity, view: &dyn BlockView, target: Vec3) -> bool {
    let zdiff = target.z - me.position.z;
    let xdiff = target.x - me.position.x;
    let slope = zdiff / xdiff;
    for i in 0..6 {
        let step = f64::from(i as f32 / 6.0);
        let z = if slope == 0.0 { 0.0 } else { zdiff * step };
        let x = if slope == 0.0 {
            xdiff * step
        } else {
            z / slope
        };
        for j in 1..4 {
            let pos = BlockPos::containing(Vec3::new(
                me.position.x + x,
                me.position.y + f64::from(j),
                me.position.z + z,
            ));
            if !view.block_state(pos).is_replaceable() {
                return false;
            }
        }
    }
    true
}

/// 生下来的狐狸：`getBreedOffspring` 的 `nextBoolean` 选父亲或母亲的品种，
/// `FoxBreedGoal.breed` 让两边的 `loveCause` 都成为信任对象。
///
/// A bred fox: `nextBoolean` picks the father's or mother's variant and both
/// love causes become trusted.
pub(crate) fn offspring(
    me: &mut Entity,
    father: Variant,
    mother: Variant,
    causes: [Option<[u8; 16]>; 2],
) -> Fox {
    let variant = if me.random.next_boolean() {
        father
    } else {
        mother
    };
    let mut fox = Fox {
        variant,
        ..Fox::default()
    };
    let [father_cause, mother_cause] = causes;
    if let Some(cause) = father_cause {
        fox.add_trusted(cause);
    }
    if let Some(cause) = mother_cause
        && father_cause != Some(cause)
    {
        fox.add_trusted(cause);
    }
    fox
}

/// `SleepGoal` 字段初始化：`random.nextInt(WAIT_TIME_BEFORE_SLEEP)`，构造时取。
///
/// The `SleepGoal` field initialiser, drawn while the fox is constructed.
pub(crate) fn sleep_countdown(random: &mut crate::entity::JavaRandom) -> i32 {
    random.next_int_bounded(super::goals::reduced_tick_delay(140))
}

/// 改 `SleepGoal` 的倒计时：构造时取的值，或者对拍时照官方那一边填。
///
/// Set the `SleepGoal` countdown: the constructor draw, or the official
/// value when the oracle mirrors it.
pub fn set_sleep_countdown(mob: &mut Mob, value: i32) {
    for wrapped in &mut mob.goals.goals {
        if let Goal::Fox(FoxGoal::Sleep { countdown }) = &mut wrapped.goal {
            *countdown = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_ids_and_names_round_trip() {
        assert_eq!(Variant::from_id(Variant::Snow.id()), Variant::Snow);
        assert_eq!(Variant::from_id(7), Variant::Red);
        assert_eq!(Variant::from_name("snow"), Some(Variant::Snow));
        assert_eq!(Variant::from_name("arctic"), None);
    }

    #[test]
    fn the_second_trusted_entity_overwrites_slot_one() {
        let mut fox = Fox::default();
        fox.add_trusted([1; 16]);
        fox.add_trusted([2; 16]);
        fox.add_trusted([3; 16]);
        assert_eq!(fox.trusted, [Some([1; 16]), Some([3; 16])]);
        assert!(fox.trusts(Some([3; 16])));
        assert!(!fox.trusts(Some([2; 16])));
    }

    #[test]
    fn a_kit_trusts_both_distinct_love_causes() {
        let mut me = Entity::by_mob_type(MobType::Fox, Vec3::default());
        let kit = offspring(
            &mut me,
            Variant::Red,
            Variant::Snow,
            [Some([4; 16]), Some([4; 16])],
        );
        assert_eq!(kit.trusted, [Some([4; 16]), None]);
        let kit = offspring(&mut me, Variant::Red, Variant::Snow, [None, Some([5; 16])]);
        assert_eq!(kit.trusted, [Some([5; 16]), None]);
    }

    #[test]
    fn clearing_states_keeps_pouncing() {
        let mut fox = Fox {
            flags: 0xff & !2,
            ..Fox::default()
        };
        fox.clear_states();
        assert_eq!(fox.flags, FLAG_POUNCING);
    }

    #[test]
    fn hunting_goals_are_added_once_in_variant_order() {
        let mut mob = Mob::new(MobType::Fox);
        mob.fox.variant = Variant::Snow;
        set_target_goals(&mut mob);
        set_target_goals(&mut mob);
        let priorities: Vec<i32> = mob
            .target_goals
            .goals
            .iter()
            .map(|goal| goal.priority)
            .collect();
        assert_eq!(priorities, [3, 4, 6, 6]);
    }
}
