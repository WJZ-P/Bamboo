//! 熊猫：六种基因决定七种外观、坐着吃竹子、打滚、打喷嚏、怕生和繁殖遗传。
//!
//! 目标和调度在 [`goals`](super::goals)；这里放基因、旗帜字节、每刻的动画状态和
//! 熊猫自己的交互、生成与繁殖。基因是官方的显性 / 隐性规则：显性基因直接决定外观，
//! 隐性基因（棕色、虚弱）只有两份相同时才显现。
//!
//! Pandas: six genes decide seven looks, sitting down to eat bamboo, rolling, sneezing,
//! being shy and breeding inheritance. Scheduling lives in [`goals`](super::goals); this
//! module holds the genes, the flags byte, the per-tick animation state and the panda's
//! own interactions, spawning and breeding.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::tags::item_tags;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;

use super::{
    Mob, MobEvent, MobType,
    attributes::{Attribute, Attributes},
    goals::{self, Goal, GoalSelector, WrappedGoal},
    interact::{MobInteractRequest, MobInteraction},
    living,
    navigation::Navigation,
};
use crate::{
    InteractionResult,
    entity::{Entity, EntityUid, KindPublic, ctx::EntityCtx},
    shape::Aabb,
};

/// `Panda.Gene`：显性基因按 id 直接是外观，`BROWN` / `WEAK` 是隐性。
///
/// `Panda.Gene`: a dominant gene is the look itself; `BROWN` and `WEAK` are recessive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Gene {
    #[default]
    Normal = 0,
    Lazy = 1,
    Worried = 2,
    Playful = 3,
    Brown = 4,
    Weak = 5,
    Aggressive = 6,
}

impl Gene {
    /// `ByIdMap.continuous(..., OutOfBoundsStrategy.ZERO)`：越界读成 `NORMAL`。
    ///
    /// `ByIdMap.continuous(..., OutOfBoundsStrategy.ZERO)`: out of range reads as `NORMAL`.
    pub const fn from_id(id: i32) -> Self {
        match id {
            1 => Self::Lazy,
            2 => Self::Worried,
            3 => Self::Playful,
            4 => Self::Brown,
            5 => Self::Weak,
            6 => Self::Aggressive,
            _ => Self::Normal,
        }
    }

    pub fn by_name(name: &str) -> Option<Self> {
        [
            Self::Normal,
            Self::Lazy,
            Self::Worried,
            Self::Playful,
            Self::Brown,
            Self::Weak,
            Self::Aggressive,
        ]
        .into_iter()
        .find(|gene| gene.name() == name)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Lazy => "lazy",
            Self::Worried => "worried",
            Self::Playful => "playful",
            Self::Brown => "brown",
            Self::Weak => "weak",
            Self::Aggressive => "aggressive",
        }
    }

    pub const fn id(self) -> i32 {
        self as i32
    }

    pub const fn is_recessive(self) -> bool {
        matches!(self, Self::Brown | Self::Weak)
    }

    /// `Gene.getRandom`：一次 `nextInt(16)`，按官方那棵树映射。
    ///
    /// `Gene.getRandom`: one `nextInt(16)` and vanilla's mapping tree.
    fn random(me: &mut Entity) -> Self {
        let roll = me.random.next_int_bounded(16);
        match roll {
            0 => Self::Lazy,
            1 => Self::Worried,
            2 => Self::Playful,
            4 => Self::Aggressive,
            3 | 5..=8 => Self::Weak,
            9 | 10 => Self::Brown,
            _ => Self::Normal,
        }
    }
}

/// `Panda.getVariant`：隐性基因要两份相同才显现。
///
/// `Panda.getVariant`: a recessive gene shows only when both copies match.
pub const fn variant(main: Gene, hidden: Gene) -> Gene {
    if main.is_recessive() {
        if matches!(
            (main, hidden),
            (Gene::Brown, Gene::Brown) | (Gene::Weak, Gene::Weak)
        ) {
            main
        } else {
            Gene::Normal
        }
    } else {
        main
    }
}

/// `Panda.DATA_ID_FLAGS` 的四位。
pub const FLAG_SNEEZE: u8 = 2;
pub const FLAG_ROLL: u8 = 4;
pub const FLAG_SIT: u8 = 8;
pub const FLAG_ON_BACK: u8 = 16;

/// 熊猫自己的状态，跟着实体行走。
///
/// The panda's own state, carried by the entity row.
#[derive(Debug, Clone, PartialEq)]
pub struct Panda {
    pub main_gene: Gene,
    pub hidden_gene: Gene,
    pub flags: u8,
    pub unhappy_counter: i32,
    pub sneeze_counter: i32,
    pub eat_counter: i32,
    /// `gotBamboo` / `didBite`：`PandaHurtByTargetGoal` 的停手条件。
    pub got_bamboo: bool,
    pub did_bite: bool,
    pub roll_counter: i32,
    pub roll_delta: Vec3,
    /// `PandaSitGoal.cooldown` / `PandaLieOnBackGoal.cooldown`，实体自己的刻数。
    pub sit_cooldown: i32,
    pub lie_cooldown: i32,
    /// `PandaBreedGoal.unhappyCooldown`。
    pub unhappy_cooldown: i32,
    /// `PandaBreedGoal` 直接写进 `PandaLookAtPlayerGoal.lookAt` 的目标。
    ///
    /// The target `PandaBreedGoal` writes straight into `PandaLookAtPlayerGoal.lookAt`.
    pub look_at_override: Option<EntityUid>,
}

impl Default for Panda {
    fn default() -> Self {
        Self {
            main_gene: Gene::Normal,
            hidden_gene: Gene::Normal,
            flags: 0,
            unhappy_counter: 0,
            sneeze_counter: 0,
            eat_counter: 0,
            got_bamboo: false,
            did_bite: false,
            roll_counter: 0,
            roll_delta: Vec3::default(),
            sit_cooldown: 0,
            lie_cooldown: 0,
            unhappy_cooldown: 0,
            look_at_override: None,
        }
    }
}

impl Panda {
    pub const fn get_flag(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    pub fn set_flag(&mut self, flag: u8, value: bool) {
        if value {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
    }

    pub const fn is_sneezing(&self) -> bool {
        self.get_flag(FLAG_SNEEZE)
    }

    pub const fn is_rolling(&self) -> bool {
        self.get_flag(FLAG_ROLL)
    }

    pub const fn is_sitting(&self) -> bool {
        self.get_flag(FLAG_SIT)
    }

    pub const fn is_on_back(&self) -> bool {
        self.get_flag(FLAG_ON_BACK)
    }

    pub fn sit(&mut self, value: bool) {
        self.set_flag(FLAG_SIT, value);
    }

    pub fn set_on_back(&mut self, value: bool) {
        self.set_flag(FLAG_ON_BACK, value);
    }

    pub fn sneeze(&mut self, value: bool) {
        self.set_flag(FLAG_SNEEZE, value);
        if !value {
            self.sneeze_counter = 0;
        }
    }

    pub fn roll(&mut self, value: bool) {
        self.set_flag(FLAG_ROLL, value);
    }

    pub fn is_eating(&self) -> bool {
        self.eat_counter > 0
    }

    pub fn eat(&mut self, value: bool) {
        self.eat_counter = if value { 1 } else { 0 };
    }

    /// `Panda.getVariant`。
    pub const fn variant(&self) -> Gene {
        variant(self.main_gene, self.hidden_gene)
    }

    pub const fn is_lazy(&self) -> bool {
        matches!(self.variant(), Gene::Lazy)
    }

    pub const fn is_worried(&self) -> bool {
        matches!(self.variant(), Gene::Worried)
    }

    pub const fn is_playful(&self) -> bool {
        matches!(self.variant(), Gene::Playful)
    }

    pub const fn is_weak(&self) -> bool {
        matches!(self.variant(), Gene::Weak)
    }

    pub const fn is_aggressive(&self) -> bool {
        matches!(self.variant(), Gene::Aggressive)
    }
}

/// `Panda.BABY_DIMENSIONS`：成年的 0.5 倍，眼高 0.28125。
///
/// `Panda.BABY_DIMENSIONS`: half the adult size, eye height 0.28125.
pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (0.65, 0.625, 0.28125)
    } else {
        let facts = &mc_bamboo_data::entity_types::FACTS[MobType::Panda.entity_type() as usize];
        (facts.width, facts.height, facts.height * 0.85)
    }
}

/// `Panda.createAttributes`：动物基础属性上加移速 0.15 和攻击 6。
///
/// `Panda.createAttributes`: animal attributes plus speed 0.15 and attack damage 6.
pub(super) fn attributes() -> Attributes {
    use mc_bamboo_data::registries::attribute;
    let mut table = Attributes::cow();
    table.set_base(attribute::MAX_HEALTH, 20.0);
    table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.15_f32));
    table
        .entries
        .push(Attribute::new(attribute::ATTACK_DAMAGE, 6.0));
    table
}

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        running: false,
        goal,
    }
}

/// `Panda.registerGoals` 的 `goalSelector`，按注册顺序。
///
/// The `goalSelector` of `Panda.registerGoals`, in registration order.
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.can_float = true;
    GoalSelector::with_goals(vec![
        wrap(0, Goal::Float),
        wrap(
            2,
            Goal::Panic {
                speed: 2.0,
                target: Vec3::default(),
            },
        ),
        wrap(
            2,
            Goal::Breed {
                speed: 1.0,
                partner: None,
                love_time: 0,
            },
        ),
        wrap(
            3,
            goals::melee_attack(super::panda::MELEE_SPEED, false, "PandaAttackGoal"),
        ),
        wrap(
            4,
            Goal::Tempt {
                steering_item: false,
                speed: 1.0,
                player: None,
                calm_down: 0,
            },
        ),
        wrap(
            6,
            Goal::AvoidEntity {
                target: goals::AvoidTarget::Player,
                max_dist: 8.0,
                walk_speed: 2.0,
                sprint_speed: 2.0,
                avoid: None,
                path: None,
            },
        ),
        wrap(
            6,
            Goal::AvoidEntity {
                target: goals::AvoidTarget::Monster,
                max_dist: 4.0,
                walk_speed: 2.0,
                sprint_speed: 2.0,
                avoid: None,
                path: None,
            },
        ),
        wrap(7, Goal::PandaSit),
        wrap(8, Goal::PandaLieOnBack),
        wrap(8, Goal::PandaSneeze),
        wrap(
            9,
            Goal::LookAtPlayer {
                look_distance: 6.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        wrap(
            10,
            Goal::RandomLookAround {
                rel_x: 0.0,
                rel_z: 0.0,
                look_time: 0,
            },
        ),
        wrap(12, Goal::PandaRoll),
        wrap(
            13,
            Goal::FollowParent {
                speed: 1.25,
                parent: None,
                time_to_recalc_path: 0,
            },
        ),
        wrap(
            14,
            Goal::Stroll {
                speed: 1.0,
                interval: 120,
                probability: 0.001,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
    ])
}

/// `PandaAttackGoal` 的速度；[`Goal::name_for`] 靠它和熊猫种类认出子类。
///
/// The `PandaAttackGoal` speed; [`Goal::name_for`] recognises the subclass by it.
pub(super) const MELEE_SPEED: f64 = 1.2;

/// `Panda.registerGoals` 的 `targetSelector`。
///
/// The `targetSelector` of `Panda.registerGoals`.
pub(super) fn target_goals() -> GoalSelector {
    GoalSelector::with_goals(vec![wrap(
        1,
        Goal::HurtByTarget {
            timestamp: 0,
            unseen_ticks: 0,
        },
    )])
}

/// `Panda.canPerformAction`：仰躺、害怕、吃东西、打滚、坐着时都不做别的。
///
/// `Panda.canPerformAction`: false when on its back, scared, eating, rolling or sitting.
pub fn can_perform_action(mob: &Mob, scared: bool) -> bool {
    let panda = &mob.panda;
    !(panda.is_on_back() || scared || panda.is_eating() || panda.is_rolling() || panda.is_sitting())
}

/// `Panda.isScared`：担心基因的熊猫在雷雨天会坐下。
///
/// `Panda.isScared`: a worried panda sits down during a thunderstorm.
pub fn is_scared(mob: &Mob, thundering: bool) -> bool {
    mob.panda.is_worried() && thundering
}

/// `Panda.tick` 在 `super.tick()` 之后的部分。
///
/// The part of `Panda.tick` after `super.tick()`.
pub(super) fn tick_end(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let thundering = ctx.blocks().is_thundering();
    let scared = is_scared(mob, thundering);
    let in_water = me.in_water;
    if mob.panda.is_worried() {
        if thundering && !in_water {
            mob.panda.sit(true);
            mob.panda.eat(false);
        } else if !mob.panda.is_eating() {
            mob.panda.sit(false);
        }
    }
    if mob.target.is_none() {
        mob.panda.got_bamboo = false;
        mob.panda.did_bite = false;
    }
    if mob.panda.unhappy_counter > 0 {
        if let Some(target) = mob.target
            && let Some(other) = ctx.other(target)
        {
            let (_, height) = other.kind.dimensions();
            let eye = other.position.y + f64::from(height) * 0.85;
            mob.look
                .set_look_at(other.position.x, eye, other.position.z, 90.0, 90.0);
        }
        if mob.panda.unhappy_counter == 29 || mob.panda.unhappy_counter == 14 {
            living::play_sound(me, mob, "minecraft:entity.panda.cant_breed", 1.0, 1.0);
        }
        mob.panda.unhappy_counter -= 1;
    }
    if mob.panda.is_sneezing() {
        mob.panda.sneeze_counter += 1;
        if mob.panda.sneeze_counter > 20 {
            mob.panda.sneeze(false);
            after_sneeze(me, mob, ctx);
        } else if mob.panda.sneeze_counter == 1 {
            living::play_sound(me, mob, "minecraft:entity.panda.pre_sneeze", 1.0, 1.0);
        }
    }
    if mob.panda.is_rolling() {
        handle_roll(me, mob);
    } else {
        mob.panda.roll_counter = 0;
    }
    if mob.panda.is_sitting() {
        mob.x_rot = 0.0;
    }
    handle_eating(me, mob, scared);
}

/// `Panda.handleRoll`：起手加一段朝向前方的速度，第 7 / 15 / 23 刻再弹一下。
///
/// `Panda.handleRoll`: the opening shove along the facing and a hop every 8th step.
fn handle_roll(me: &mut Entity, mob: &mut Mob) {
    mob.panda.roll_counter += 1;
    if mob.panda.roll_counter > 32 {
        mob.panda.roll(false);
        return;
    }
    let movement = me.delta;
    if mob.panda.roll_counter == 1 {
        // 官方这里是 `float angle` 加 `Mth.sin` / `Mth.cos`，结果先按 float 算再抬成 double。
        // Vanilla uses a `float` angle and `Mth.sin` / `Mth.cos`; the product is float first.
        let angle = me.y_rot * (std::f32::consts::PI / 180.0);
        let multiplier = if mob.is_baby() { 0.1_f32 } else { 0.2_f32 };
        let sin = mth::sin(f64::from(angle));
        let cos = mth::cos(f64::from(angle));
        let delta = Vec3::new(
            movement.x + f64::from(-sin * multiplier),
            0.0,
            movement.z + f64::from(cos * multiplier),
        );
        mob.panda.roll_delta = delta;
        me.delta = delta.add(Vec3::new(0.0, 0.27, 0.0));
    } else if mob.panda.roll_counter != 7
        && mob.panda.roll_counter != 15
        && mob.panda.roll_counter != 23
    {
        me.delta = Vec3::new(mob.panda.roll_delta.x, movement.y, mob.panda.roll_delta.z);
    } else {
        let y = if me.on_ground { 0.27 } else { movement.y };
        me.delta = Vec3::new(0.0, y, 0.0);
    }
}

/// `Panda.afterSneeze`：喷嚏声、附近成年熊猫跳一下、按礼物表掉黏液球。
///
/// `Panda.afterSneeze`: the sound, nearby adult pandas hopping, and the gift table.
fn after_sneeze(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    living::play_sound(me, mob, "minecraft:entity.panda.sneeze", 1.0, 1.0);
    let mut others = Vec::new();
    ctx.for_each_other_in(me.bounding_box().inflate(10.0, 10.0, 10.0), &mut |other| {
        if let KindPublic::Mob(public) = &other.kind
            && public.mob_type == MobType::Panda
            && !public.baby
            && other.on_ground
        {
            others.push(other.uid);
        }
    });
    if !others.is_empty() {
        ctx.emit(crate::entity::ctx::Command::JumpMobs { others }, me);
    }
    if ctx.blocks().block_game_rules().mob_drops {
        ctx.emit(
            crate::entity::ctx::Command::DropEntityLoot(super::EntityLootRequest {
                table: "minecraft:gameplay/panda_sneeze",
                kind: MobType::Panda,
                origin: me.position,
                wool: 0,
                variant: super::CowVariant::Temperate,
                tool: None,
            }),
            me,
        );
    }
}

/// `Panda.handleEating`：坐着吃手里那份，80 刻以后按 1/20 吃完；每 5 刻出一次声和粒子。
///
/// `Panda.handleEating`: eat the held stack while sitting; after 80 ticks finish at
/// 1/20; a sound and particles every fifth tick.
fn handle_eating(me: &mut Entity, mob: &mut Mob, scared: bool) {
    use mc_bamboo_protocol::packet::play::EquipmentSlot;
    let slot = EquipmentSlot::MainHand;
    let held = mob.equipment.get(slot).clone();
    if !mob.panda.is_eating()
        && mob.panda.is_sitting()
        && !scared
        && !held.is_empty()
        && me.random.next_int_bounded(80) == 1
    {
        mob.panda.eat(true);
    } else if held.is_empty() || !mob.panda.is_sitting() {
        mob.panda.eat(false);
    }
    if !mob.panda.is_eating() {
        return;
    }
    add_eating_particles(me, mob, &held);
    if mob.panda.eat_counter > 80 && me.random.next_int_bounded(20) == 1 {
        if mob.panda.eat_counter > 100
            && crate::state::item_is_tagged(held.item, item_tags::PANDA_EATS_FROM_GROUND)
        {
            mob.equipment
                .edit(slot, |stack| *stack = ItemStack::empty());
            mob.panda.sit(false);
        }
        mob.panda.eat(false);
        return;
    }
    mob.panda.eat_counter += 1;
}

/// `Panda.addEatingParticles`：声音的音量和音高各取数，粒子每颗六个浮点。
///
/// `Panda.addEatingParticles`: the sound draws its volume and pitch, and each particle
/// draws six floats.
fn add_eating_particles(me: &mut Entity, mob: &mut Mob, held: &ItemStack) {
    if mob.panda.eat_counter % 5 != 0 {
        return;
    }
    let volume = 0.5 + 0.5 * me.random.next_int_bounded(2) as f32;
    let pitch = (me.random.next_float() - me.random.next_float()) * 0.2 + 1.0;
    living::play_sound(me, mob, "minecraft:entity.panda.eat", volume, pitch);
    if held.is_empty() {
        return;
    }
    for _ in 0..6 {
        // 每颗粒子的速度三个、位置三个浮点，服务端不发粒子但取数照旧。
        // Six floats per particle; the server sends no particles but still draws them.
        me.random.next_float();
        me.random.next_float();
        me.random.next_float();
        me.random.next_float();
        me.random.next_float();
        me.random.next_float();
    }
}

/// `Panda.mobInteract`。放在通用的喂食分支之前。
///
/// `Panda.mobInteract`, ahead of the shared feeding branch.
pub(super) fn interact(
    _me: &mut Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
    scared: bool,
) -> Option<MobInteraction> {
    if scared {
        return Some(MobInteraction::pass());
    }
    if mob.panda.is_on_back() {
        mob.panda.set_on_back(false);
        return Some(MobInteraction {
            result: InteractionResult::Success,
            ..MobInteraction::pass()
        });
    }
    let food = !req.held.is_empty() && is_food(req.held.item);
    if !food {
        return None;
    }
    if mob.target.is_some() {
        mob.panda.got_bamboo = true;
    }
    if mob.is_baby() && !mob.age_locked {
        let seconds = (((-mob.age) / 20) as f32 * 0.1_f32) as i32;
        super::interact::age_up(_me, mob, seconds, true);
        return Some(MobInteraction {
            result: InteractionResult::Success,
            shrink_held: !req.creative,
            ..MobInteraction::pass()
        });
    }
    if mob.is_baby() {
        return Some(MobInteraction::pass());
    }
    if mob.age == 0 && mob.in_love <= 0 {
        mob.in_love = 600;
        mob.love_cause = Some(req.player_uuid);
        mob.events.push(MobEvent::Hearts);
        return Some(MobInteraction {
            result: InteractionResult::SuccessServer,
            shrink_held: !req.creative,
            ..MobInteraction::pass()
        });
    }
    if mob.panda.is_sitting() || _me.in_water {
        return Some(MobInteraction::pass());
    }
    try_to_sit(_me, mob);
    mob.panda.eat(true);
    let mut drop = None;
    mob.equipment.edit(
        mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand,
        |stack| {
            if !stack.is_empty() && !req.creative {
                drop = Some(stack.clone());
            }
            *stack = ItemStack::new(req.held.item, 1);
        },
    );
    Some(MobInteraction {
        result: InteractionResult::SuccessServer,
        shrink_held: !req.creative,
        drop_stack: drop,
        ..MobInteraction::pass()
    })
}

/// `Panda.tryToSit`。
fn try_to_sit(_me: &mut Entity, mob: &mut Mob) {
    if !_me.in_water {
        mob.zza = 0.0;
        mob.navigation.stop();
        mob.panda.sit(true);
    }
}

/// `Panda.isFood`。
pub fn is_food(item: i32) -> bool {
    crate::state::item_is_tagged(item, item_tags::PANDA_FOOD)
}

/// `Panda.getAmbientSound`：好斗、担心和普通三种。
///
/// `Panda.getAmbientSound`: aggressive, worried and the plain one.
pub(super) const fn ambient_sound(panda: &Panda) -> &'static str {
    if panda.is_aggressive() {
        "minecraft:entity.panda.aggressive_ambient"
    } else if panda.is_worried() {
        "minecraft:entity.panda.worried_ambient"
    } else {
        "minecraft:entity.panda.ambient"
    }
}

/// `Panda.getBreedOffspring` 里的 `setGeneFromParents`，随机源是幼崽自己的。
///
/// `setGeneFromParents` from `Panda.getBreedOffspring`, drawing from the calf's random.
pub fn offspring_genes(
    baby: &mut Entity,
    father: &mut crate::entity::JavaRandom,
    father_genes: (Gene, Gene),
    mother: &mut crate::entity::JavaRandom,
    mother_genes: (Gene, Gene),
) -> (Gene, Gene) {
    // `getOneOfGenesRandomly` 是父 / 母自己的方法，取的是**父母各自的**随机源。
    // `getOneOfGenesRandomly` runs on each parent, so it draws from that parent's random.
    let one_of = |random: &mut crate::entity::JavaRandom, genes: (Gene, Gene)| {
        if random.next_boolean() {
            genes.0
        } else {
            genes.1
        }
    };
    let (mut main, mut hidden) = if baby.random.next_boolean() {
        (one_of(father, father_genes), one_of(mother, mother_genes))
    } else {
        (one_of(mother, mother_genes), one_of(father, father_genes))
    };
    if baby.random.next_int_bounded(32) == 0 {
        main = Gene::random(baby);
    }
    if baby.random.next_int_bounded(32) == 0 {
        hidden = Gene::random(baby);
    }
    (main, hidden)
}

/// `Panda.setAttributes`：虚弱 10 血、懒惰 0.07 移速。
///
/// `Panda.setAttributes`: weak gives 10 health, lazy gives speed 0.07.
pub fn set_attributes(mob: &mut Mob) {
    use mc_bamboo_data::registries::attribute;
    if mob.panda.is_weak() {
        mob.attributes.set_base(attribute::MAX_HEALTH, 10.0);
    }
    if mob.panda.is_lazy() {
        mob.attributes
            .set_base(attribute::MOVEMENT_SPEED, f64::from(0.07_f32));
    }
}

/// `Panda.finalizeSpawn`：两个基因各取一次 `nextInt(16)`，然后才是 `AgeableMob` 那层。
///
/// `Panda.finalizeSpawn`: one `nextInt(16)` per gene, then the `AgeableMob` layer.
pub(super) fn finalize_spawn(
    entity: &mut Entity,
    random: &dyn crate::random::RandomSource,
    group: &mut Option<super::spawn::SpawnGroup>,
) {
    let main = super::panda::random_gene(random);
    let hidden = super::panda::random_gene(random);
    entity.with_mob(|_me, mob| {
        mob.panda.main_gene = main;
        mob.panda.hidden_gene = hidden;
        set_attributes(mob);
        if !mob.is_baby() {
            mob.can_pick_up_loot = true;
        }
    });
    super::spawn::finalize_ageable(entity, random, group, 0.2);
}

/// `Gene.getRandom` 的世界随机源版本。
///
/// `Gene.getRandom` on the world random.
fn random_gene(random: &dyn crate::random::RandomSource) -> Gene {
    let roll = random.next_int(16);
    match roll {
        0 => Gene::Lazy,
        1 => Gene::Worried,
        2 => Gene::Playful,
        4 => Gene::Aggressive,
        3 | 5..=8 => Gene::Weak,
        9 | 10 => Gene::Brown,
        _ => Gene::Normal,
    }
}

/// `Panda.checkPandaSpawnRules` 走的是 `Animal.checkAnimalSpawnRules`。
///
/// `Panda.checkPandaSpawnRules` uses `Animal.checkAnimalSpawnRules`.
pub(super) fn spawn_rules(world: &crate::world::World, pos: BlockPos) -> bool {
    super::natural::animal_spawn_rules(world, pos)
}

/// `Panda.pickUpItem`：空着手且脚下那份能吃时直接拿进主手。
///
/// `Panda.pickUpItem`: take an edible stack into the empty main hand.
pub(crate) fn wants_pick_up(mob: &Mob, stack: &ItemStack, pickup_delay: i32) -> bool {
    let empty = mob
        .equipment
        .get(mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand)
        .is_empty();
    empty
        && pickup_delay <= 0
        && crate::state::item_is_tagged(stack.item, item_tags::PANDA_EATS_FROM_GROUND)
}

/// 落地 [`crate::entity::ctx::Command::PickUpItem`]。
///
/// Applies [`crate::entity::ctx::Command::PickUpItem`].
pub(crate) fn apply_pick_up(world: &mut crate::world::World, item: EntityUid, me: &mut Entity) {
    let Some(entity) = world.entity(item) else {
        return;
    };
    let crate::entity::EntityKind::Item(ground) = &entity.kind else {
        return;
    };
    if ground.stack.is_empty() {
        return;
    }
    let stack = ground.stack.clone();
    me.with_mob(|_me, mob| {
        mob.equipment.set(
            mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand,
            stack,
        );
        mob.equipment.set_drop_chance(
            mc_bamboo_protocol::packet::play::EquipmentSlot::MainHand,
            2.0,
        );
    });
    world.with_entity_mut(item, &mut |entity| entity.discard());
}

/// `PandaSitGoal.canUse`。
///
/// `PandaSitGoal.canUse`.
pub(super) fn sit_can_use(me: &Entity, mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    use mc_bamboo_protocol::packet::play::EquipmentSlot;
    let thundering = ctx.blocks().is_thundering();
    if mob.panda.sit_cooldown > me.tick_count
        || mob.is_baby()
        || me.in_water
        || !can_perform_action(mob, is_scared(mob, thundering))
        || mob.panda.unhappy_counter > 0
    {
        return false;
    }
    if !mob.equipment.get(EquipmentSlot::MainHand).is_empty() {
        return true;
    }
    nearest_edible(me, ctx, 6.0).is_some()
}

/// `PandaSitGoal.canContinueToUse`。
///
/// `PandaSitGoal.canContinueToUse`.
pub(super) fn sit_can_continue(me: &mut Entity, mob: &Mob) -> bool {
    if me.in_water || (!mob.panda.is_lazy() && me.random.next_int_bounded(300) == 1) {
        return false;
    }
    me.random.next_int_bounded(1000) != 1
}

/// `PandaSitGoal.start`。
///
/// `PandaSitGoal.start`.
pub(super) fn sit_start(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    use mc_bamboo_protocol::packet::play::EquipmentSlot;
    if mob.equipment.get(EquipmentSlot::MainHand).is_empty() {
        if let Some((_, at)) = nearest_edible(me, ctx, 8.0) {
            let nav = living::nav_mob(me, mob, ctx.blocks());
            mob.navigation.move_to(ctx.blocks(), &nav, at, 1.2);
        }
    } else {
        try_to_sit(me, mob);
    }
    mob.panda.sit_cooldown = 0;
}

/// `PandaSitGoal.tick`。
///
/// `PandaSitGoal.tick`.
pub(super) fn sit_tick(me: &mut Entity, mob: &mut Mob) {
    use mc_bamboo_protocol::packet::play::EquipmentSlot;
    if !mob.panda.is_sitting() && !mob.equipment.get(EquipmentSlot::MainHand).is_empty() {
        try_to_sit(me, mob);
    }
}

/// `PandaSitGoal.stop`：把手里的东西丢下，按懒惰与否等 10–160 秒。
///
/// `PandaSitGoal.stop`: drop the held stack and wait 10–160 seconds.
pub(super) fn sit_stop(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    use mc_bamboo_protocol::packet::play::EquipmentSlot;
    let held = mob.equipment.get(EquipmentSlot::MainHand).clone();
    if !held.is_empty() {
        ctx.emit(crate::entity::ctx::Command::DropItem { stack: held }, me);
        mob.equipment
            .edit(EquipmentSlot::MainHand, |stack| *stack = ItemStack::empty());
        let wait = if mob.panda.is_lazy() {
            me.random.next_int_bounded(50) + 10
        } else {
            me.random.next_int_bounded(150) + 10
        };
        mob.panda.sit_cooldown = me.tick_count + wait * 20;
    }
    mob.panda.sit(false);
}

/// `PandaLieOnBackGoal.canUse`。
///
/// `PandaLieOnBackGoal.canUse`.
pub(super) fn lie_can_use(me: &mut Entity, mob: &Mob, thundering: bool) -> bool {
    mob.panda.lie_cooldown < me.tick_count
        && mob.panda.is_lazy()
        && can_perform_action(mob, is_scared(mob, thundering))
        && me.random.next_int_bounded(200) == 1
}

/// `PandaLieOnBackGoal.canContinueToUse`。
///
/// `PandaLieOnBackGoal.canContinueToUse`.
pub(super) fn lie_can_continue(me: &mut Entity, mob: &Mob) -> bool {
    if me.in_water || (!mob.panda.is_lazy() && me.random.next_int_bounded(300) == 1) {
        return false;
    }
    me.random.next_int_bounded(1000) != 1
}

/// `PandaLieOnBackGoal.start` / `stop`。
///
/// `PandaLieOnBackGoal.start` / `stop`.
pub(super) fn lie_start(_me: &mut Entity, mob: &mut Mob) {
    mob.panda.set_on_back(true);
    mob.panda.lie_cooldown = 0;
}

pub(super) fn lie_stop(me: &mut Entity, mob: &mut Mob) {
    mob.panda.set_on_back(false);
    mob.panda.lie_cooldown = me.tick_count + 200;
}

/// `PandaSneezeGoal.canUse`：虚弱熊猫先抽 1/500，再抽 1/6000。
///
/// `PandaSneezeGoal.canUse`: a weak cub rolls 1/500 first, then 1/6000.
pub(super) fn sneeze_can_use(me: &mut Entity, mob: &Mob, thundering: bool) -> bool {
    if !mob.is_baby() || !can_perform_action(mob, is_scared(mob, thundering)) {
        return false;
    }
    if mob.panda.is_weak() && me.random.next_int_bounded(250) == 1 {
        return true;
    }
    me.random.next_int_bounded(3000) == 1
}

/// `PandaRollGoal.canUse`：朝向前面一格是空气就滚，否则按顽皮与否抽一次。
///
/// `PandaRollGoal.canUse`: roll when the cell ahead is air, otherwise a playful roll.
pub(super) fn roll_can_use(me: &mut Entity, mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    let thundering = ctx.blocks().is_thundering();
    if !(mob.is_baby() || mob.panda.is_playful()) || !me.on_ground {
        return false;
    }
    if !can_perform_action(mob, is_scared(mob, thundering)) {
        return false;
    }
    let angle = me.y_rot * (std::f32::consts::PI / 180.0);
    let x_dir = -mth::sin(f64::from(angle));
    let z_dir = mth::cos(f64::from(angle));
    let x_step = if x_dir.abs() > 0.5 {
        x_dir.signum() as i32
    } else {
        0
    };
    let z_step = if z_dir.abs() > 0.5 {
        z_dir.signum() as i32
    } else {
        0
    };
    let ahead = me.block_position().offset(x_step, -1, z_step);
    if ctx.blocks().block_state(ahead).is_air() {
        return true;
    }
    if mob.panda.is_playful() && me.random.next_int_bounded(30) == 1 {
        return true;
    }
    me.random.next_int_bounded(250) == 1
}

/// `PandaBreedGoal.canUse` 的后半段：不乐意、找不到竹子、30 秒冷却。
///
/// The tail of `PandaBreedGoal.canUse`: unhappy, no bamboo, and the 30 second cooldown.
pub(super) fn breed_can_use(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
    if mob.panda.unhappy_counter != 0 {
        return false;
    }
    if !bamboo_nearby(me, ctx.blocks()) {
        if mob.panda.unhappy_cooldown <= me.tick_count {
            mob.panda.unhappy_counter = 32;
            mob.panda.unhappy_cooldown = me.tick_count + 600;
            if !mob.no_ai {
                mob.panda.look_at_override = nearest_player(me, mob, ctx, 8.0);
            }
        }
        return false;
    }
    true
}

/// `PandaBreedGoal.canFindBamboo`：脚下三格高、半径八格的方螺旋。
///
/// `PandaBreedGoal.canFindBamboo`: a square spiral three blocks up and eight out.
fn bamboo_nearby(me: &Entity, view: &dyn crate::entity::ctx::BlockView) -> bool {
    let origin = me.block_position();
    for y in 0..3 {
        for r in 0..8 {
            let mut x = 0;
            while x <= r {
                let from = if x < r && x > -r { r } else { 0 };
                let mut z = from;
                while z <= r {
                    if view
                        .block_state(origin.offset(x, y, z))
                        .is(mc_bamboo_data::blocks::BAMBOO)
                    {
                        return true;
                    }
                    z = if z > 0 { -z } else { 1 - z };
                }
                x = if x > 0 { -x } else { 1 - x };
            }
        }
    }
    false
}

/// `PandaHurtByTargetGoal.alertOther`：只叫好斗的熊猫。
///
/// `PandaHurtByTargetGoal.alertOther`: calls aggressive pandas only.
pub(super) fn alert_pandas(me: &mut Entity, mob: &Mob, ctx: &mut dyn EntityCtx) {
    let Some(enemy) = mob.last_hurt_by_mob else {
        return;
    };
    let range = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE);
    let mut others = Vec::new();
    ctx.for_each_other_in(
        me.bounding_box().inflate(range, 10.0, range),
        &mut |other| {
            if let KindPublic::Mob(public) = &other.kind
                && public.mob_type == MobType::Panda
                && aggressive_kind(&public.panda)
            {
                others.push(other.uid);
            }
        },
    );
    if !others.is_empty() {
        ctx.emit(crate::entity::ctx::Command::AlertMobs { others, enemy }, me);
    }
}

fn aggressive_kind(public: &PandaPublic) -> bool {
    variant(
        Gene::from_id(i32::from(public.main_gene)),
        Gene::from_id(i32::from(public.hidden_gene)),
    ) == Gene::Aggressive
}

/// 最近的非旁观玩家，`PandaBreedGoal` 的 `getNearestPlayer(forNonCombat().range(8))`。
///
/// The nearest non-spectator player, the panda's `getNearestPlayer(forNonCombat().range(8))`.
pub(super) fn nearest_player(
    me: &Entity,
    _mob: &Mob,
    ctx: &dyn EntityCtx,
    range: f64,
) -> Option<EntityUid> {
    let mut best = range * range;
    let mut found = None;
    ctx.blocks().for_each_player(&mut |player| {
        if player.spectator {
            return;
        }
        let distance = me.position.distance_to_sqr(player.position);
        if distance <= best {
            best = distance;
            found = Some(player.uid);
        }
    });
    found
}

/// `PandaSitGoal.canUse` 的 `canPickUpAndEat` 判定。
///
/// The `canPickUpAndEat` test of `PandaSitGoal.canUse`.
/// `Panda.addAdditionalSaveData`。
///
/// `Panda.addAdditionalSaveData`.
pub(super) fn write_nbt(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    out.push((
        "MainGene".to_owned(),
        Tag::String(mob.panda.main_gene.name().to_owned()),
    ));
    out.push((
        "HiddenGene".to_owned(),
        Tag::String(mob.panda.hidden_gene.name().to_owned()),
    ));
}

/// `Panda.readAdditionalSaveData`。
///
/// `Panda.readAdditionalSaveData`.
pub(super) fn read_nbt(mob: &mut Mob, tag: &Tag) {
    if let Some(name) = crate::archive::tag::string_or(tag, "MainGene")
        && let Some(gene) = Gene::by_name(name)
    {
        mob.panda.main_gene = gene;
    }
    if let Some(name) = crate::archive::tag::string_or(tag, "HiddenGene")
        && let Some(gene) = Gene::by_name(name)
    {
        mob.panda.hidden_gene = gene;
    }
}

/// `Panda.canPickUpAndEat` 的包围盒查询，`PandaSitGoal` 用。
///
/// The `Panda.canPickUpAndEat` box query used by `PandaSitGoal`.
pub(super) fn nearest_edible(
    me: &Entity,
    ctx: &dyn EntityCtx,
    inflate: f64,
) -> Option<(EntityUid, Vec3)> {
    let area = Aabb::new(
        me.position.x - inflate,
        me.position.y - inflate,
        me.position.z - inflate,
        me.position.x + inflate,
        me.position.y + inflate,
        me.position.z + inflate,
    );
    let mut found = None;
    ctx.for_each_other_in(area, &mut |other| {
        if let KindPublic::Item {
            stack,
            pickup_delay,
            ..
        } = &other.kind
            && found.is_none()
            && *pickup_delay <= 0
            && crate::state::item_is_tagged(stack.item, item_tags::PANDA_EATS_FROM_GROUND)
        {
            found = Some((other.uid, other.position));
        }
    });
    found
}

/// `Panda` 的同步字段。
///
/// The synced panda fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PandaPublic {
    pub main_gene: u8,
    pub hidden_gene: u8,
    pub flags: u8,
    pub unhappy_counter: i32,
    pub sneeze_counter: i32,
    pub eat_counter: i32,
}

impl PandaPublic {
    pub fn of(panda: &Panda) -> Self {
        Self {
            main_gene: panda.main_gene.id() as u8,
            hidden_gene: panda.hidden_gene.id() as u8,
            flags: panda.flags,
            unhappy_counter: panda.unhappy_counter,
            sneeze_counter: panda.sneeze_counter,
            eat_counter: panda.eat_counter,
        }
    }
}

impl Mob {
    /// 供 [`goals`](super::goals) 的熊猫分支读的快捷方式。
    ///
    /// A shortcut for the panda branches of [`goals`](super::goals).
    pub fn panda_can_perform_action(&self, thundering: bool) -> bool {
        can_perform_action(self, is_scared(self, thundering))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genes_follow_the_recessive_rule() {
        assert_eq!(variant(Gene::Brown, Gene::Brown), Gene::Brown);
        assert_eq!(variant(Gene::Brown, Gene::Normal), Gene::Normal);
        assert_eq!(variant(Gene::Weak, Gene::Weak), Gene::Weak);
        assert_eq!(variant(Gene::Lazy, Gene::Weak), Gene::Lazy);
    }

    #[test]
    fn attributes_start_from_the_panda_table() {
        let panda = Mob::new(MobType::Panda);
        assert_eq!(
            panda
                .attributes
                .value(mc_bamboo_data::registries::attribute::MAX_HEALTH),
            20.0
        );
        assert_eq!(
            panda
                .attributes
                .value(mc_bamboo_data::registries::attribute::MOVEMENT_SPEED),
            f64::from(0.15_f32)
        );
        assert_eq!(
            panda
                .attributes
                .value(mc_bamboo_data::registries::attribute::ATTACK_DAMAGE),
            6.0
        );
        assert_eq!(panda.health, 20.0);
    }

    #[test]
    fn weak_and_lazy_lower_the_attributes() {
        let mut panda = Mob::new(MobType::Panda);
        panda.panda.main_gene = Gene::Weak;
        panda.panda.hidden_gene = Gene::Weak;
        set_attributes(&mut panda);
        assert_eq!(
            panda
                .attributes
                .value(mc_bamboo_data::registries::attribute::MAX_HEALTH),
            10.0
        );
        panda.panda.main_gene = Gene::Lazy;
        set_attributes(&mut panda);
        assert_eq!(
            panda
                .attributes
                .value(mc_bamboo_data::registries::attribute::MOVEMENT_SPEED),
            f64::from(0.07_f32)
        );
    }

    #[test]
    fn gene_ids_round_trip() {
        for gene in [
            Gene::Normal,
            Gene::Lazy,
            Gene::Worried,
            Gene::Playful,
            Gene::Brown,
            Gene::Weak,
            Gene::Aggressive,
        ] {
            assert_eq!(Gene::from_id(gene.id()), gene);
            assert_eq!(Gene::by_name(gene.name()), Some(gene));
        }
        assert_eq!(Gene::from_id(99), Gene::Normal);
    }

    #[test]
    fn baby_dimensions_are_half_the_adult_ones() {
        assert_eq!(MobType::Panda.dimensions(true), (0.65, 0.625, 0.28125));
        let (width, height, eye) = MobType::Panda.dimensions(false);
        assert_eq!((width, height), (1.3, 1.25));
        assert_eq!(eye, 1.25_f32 * 0.85);
    }

    #[test]
    fn genes_survive_a_save_and_load() {
        let mut mob = Mob::new(MobType::Panda);
        mob.panda.main_gene = Gene::Lazy;
        mob.panda.hidden_gene = Gene::Brown;
        let mut out = Vec::new();
        write_nbt(&mob, &mut out);
        let tag = Tag::Compound(out);
        let mut loaded = Mob::new(MobType::Panda);
        read_nbt(&mut loaded, &tag);
        assert_eq!(loaded.panda.main_gene, Gene::Lazy);
        assert_eq!(loaded.panda.hidden_gene, Gene::Brown);
    }

    #[test]
    fn goals_follow_the_official_registration() {
        let panda = Mob::new(MobType::Panda);
        let priorities: Vec<i32> = panda.goals.goals.iter().map(|goal| goal.priority).collect();
        assert_eq!(
            priorities,
            [0, 2, 2, 3, 4, 6, 6, 7, 8, 8, 9, 10, 12, 13, 14]
        );
        let names: Vec<&str> = panda
            .goals
            .goals
            .iter()
            .map(|goal| goal.goal.name_for(MobType::Panda))
            .collect();
        assert_eq!(names[1], "PandaPanicGoal");
        assert_eq!(names[2], "PandaBreedGoal");
        assert_eq!(names[3], "PandaAttackGoal");
        assert_eq!(names[5], "PandaAvoidGoal");
        assert_eq!(names[7], "PandaSitGoal");
        assert_eq!(names[12], "PandaRollGoal");
        assert_eq!(panda.target_goals.goals.len(), 1);
        assert_eq!(
            panda.target_goals.goals[0].goal.name_for(MobType::Panda),
            "PandaHurtByTargetGoal"
        );
    }
}
