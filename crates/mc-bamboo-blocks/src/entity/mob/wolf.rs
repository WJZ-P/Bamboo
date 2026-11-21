//! 狼：品种和叫声（`wolf_variant` / `wolf_sound_variant` 注册表）、项圈颜色、持续仇恨
//! （`NeutralMob`）、抖水，以及右键交互、繁殖和挂在共用生物框架上的钩子。
//! 狼专属的目标在 [`super::wolf_goals`]，驯服的公共部分在 [`super::tame`]。
//!
//! Wolves: variants and sound variants, the collar colour, persistent anger
//! (`NeutralMob`), shaking off water, right-click interactions, breeding and
//! the hooks into the shared mob machinery. Wolf-only goals live in
//! [`super::wolf_goals`]; the shared taming part in [`super::tame`].

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{entity_types, items, registries::attribute, tags::item_tags};
use mc_bamboo_protocol::item::ItemStack;

use super::{
    Mob, MobEvent, MobType,
    attributes::{Attribute, Attributes},
    goals::{self, Goal, GoalSelector, WrappedGoal},
    interact::{MobInteractRequest, MobInteraction},
    living,
    navigation::Navigation,
    tame::{self, TameGoal},
    wolf_goals::WolfGoal,
};
use crate::{
    InteractionResult,
    entity::{
        Entity, EntityUid,
        ctx::{BlockView, EntityCtx, PlayerView},
        store::KindPublic,
    },
    state::item_is_tagged,
};

/// `Wolf.DEFAULT_COLLAR_COLOR`：红色。
const DEFAULT_COLLAR: i32 = 14;
/// `Wolf.TAME_HEALTH`。
const TAME_HEALTH: f32 = 40.0;
/// `Wolf.START_HEALTH`。
const START_HEALTH: f64 = 8.0;

/// `wolf_variant` 注册表里的条目名，下标就是同步用的注册表 ID。
///
/// The `wolf_variant` entries; the index is the synced registry id.
pub fn variant_names() -> &'static [&'static str] {
    synced_entries("minecraft:wolf_variant")
}

/// `wolf_sound_variant` 注册表里的条目名。
pub fn sound_variant_names() -> &'static [&'static str] {
    synced_entries("minecraft:wolf_sound_variant")
}

fn synced_entries(name: &str) -> &'static [&'static str] {
    mc_bamboo_data::registries::SYNCED_REGISTRIES
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
pub fn variant_id(name: &str) -> Option<i32> {
    let names = variant_names();
    names.contains(&name).then(|| id_of(names, name))
}

/// 叫声名对应的注册表 ID；未知名字返回 `None`。
pub fn sound_variant_id(name: &str) -> Option<i32> {
    let names = sound_variant_names();
    names.contains(&name).then(|| id_of(names, name))
}

/// `WolfVariants.DEFAULT`（苍白狼）的注册表 ID。
pub fn default_variant() -> i32 {
    id_of(variant_names(), "minecraft:pale")
}

/// `WolfSoundVariants.CLASSIC` 的注册表 ID。
pub fn default_sound_variant() -> i32 {
    id_of(sound_variant_names(), "minecraft:classic")
}

/// 狼自己的状态。
///
/// The wolf-only state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wolf {
    /// `DATA_VARIANT_ID`：`wolf_variant` 注册表 ID。
    pub variant: i32,
    /// `DATA_SOUND_VARIANT_ID`：`wolf_sound_variant` 注册表 ID。
    pub sound_variant: i32,
    /// `DATA_COLLAR_COLOR`：`DyeColor` 的 ID。
    pub collar: i32,
    /// `DATA_INTERESTED_ID`：讨食时歪头。
    pub interested: bool,
    /// `DATA_ANGER_END_TIME`：岛时钟上的绝对时刻，-1 表示没有。
    ///
    /// An absolute island-clock tick, or -1.
    pub anger_end_time: i64,
    /// `persistentAngerTarget` 的 UUID（存档 `angry_at`）。
    pub angry_at: Option<[u8; 16]>,
    /// `persistentAngerTarget` 在本岛的 uid；读盘后不知道，按 UUID 再找。
    ///
    /// The island uid of the anger target; unknown after a load.
    pub angry_at_uid: Option<EntityUid>,
    pub is_wet: bool,
    pub is_shaking: bool,
    pub shake_anim: f32,
    pub shake_anim_o: f32,
}

impl Default for Wolf {
    fn default() -> Self {
        Self {
            variant: default_variant(),
            sound_variant: default_sound_variant(),
            collar: DEFAULT_COLLAR,
            interested: false,
            anger_end_time: -1,
            angry_at: None,
            angry_at_uid: None,
            is_wet: false,
            is_shaking: false,
            shake_anim: 0.0,
            shake_anim_o: 0.0,
        }
    }
}

impl Wolf {
    /// `NeutralMob.isAngry`。
    pub const fn is_angry(&self, now: i64) -> bool {
        self.anger_end_time > 0 && self.anger_end_time - now > 0
    }

    /// 品种的注册名。
    pub fn variant_name(&self) -> &'static str {
        name_at(variant_names(), self.variant, "minecraft:pale")
    }

    /// 叫声的注册名。
    pub fn sound_variant_name(&self) -> &'static str {
        name_at(
            sound_variant_names(),
            self.sound_variant,
            "minecraft:classic",
        )
    }

    /// 岛合并时平移持续仇恨的结束时刻。
    ///
    /// Shift the anger end time when islands merge.
    pub fn rebase(&mut self, delta: i64) {
        if self.anger_end_time > 0 {
            self.anger_end_time = self.anger_end_time.saturating_add(delta);
        }
    }
}

fn name_at(names: &'static [&'static str], id: i32, fallback: &'static str) -> &'static str {
    usize::try_from(id)
        .ok()
        .and_then(|id| names.get(id))
        .copied()
        .unwrap_or(fallback)
}

pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (
            entity_types::WOLF_BABY_WIDTH,
            entity_types::WOLF_BABY_HEIGHT,
            entity_types::WOLF_BABY_EYE_HEIGHT,
        )
    } else {
        let facts = &entity_types::FACTS[MobType::Wolf.entity_type() as usize];
        (facts.width, facts.height, entity_types::WOLF_EYE_HEIGHT)
    }
}

/// `Wolf.createAttributes`。
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    table.set_base(attribute::MAX_HEALTH, entity_types::WOLF_MAX_HEALTH);
    table.set_base(attribute::MOVEMENT_SPEED, entity_types::WOLF_MOVEMENT_SPEED);
    table.entries.push(Attribute::new(
        attribute::ATTACK_DAMAGE,
        entity_types::WOLF_ATTACK_DAMAGE,
    ));
    table
}

/// `Wolf` 构造函数：`setTame(false, false)`，细雪和细雪上方都不走。
///
/// The `Wolf` constructor: untamed, and powder snow is off limits.
pub(super) fn configure(mob: &mut Mob) {
    tame::set_tame(mob, false, false);
    mob.malus.set(mc_bamboo_data::PathType::PowderSnow, -1.0);
    mob.malus
        .set(mc_bamboo_data::PathType::OnTopOfPowderSnow, -1.0);
}

fn wrap(priority: i32, goal: Goal) -> WrappedGoal {
    WrappedGoal {
        priority,
        goal,
        running: false,
    }
}

/// `Wolf.registerGoals` 的 goalSelector，按注册顺序。
///
/// The `Wolf.registerGoals` goal selector, in registration order.
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.can_float = true;
    let mut selector = GoalSelector::default();
    selector.goals = vec![
        wrap(1, Goal::Float),
        wrap(
            1,
            Goal::Panic {
                speed: 1.5,
                target: Vec3::default(),
            },
        ),
        wrap(2, Goal::Tame(TameGoal::Sit)),
        // 羊驼还没有，`WolfAvoidEntityGoal` 找不到要躲的，不取随机数。
        // Llamas do not exist yet; the goal finds nothing and draws nothing.
        wrap(3, Goal::Unavailable("WolfAvoidEntityGoal")),
        wrap(4, Goal::LeapAtTarget { yd: 0.4 }),
        wrap(5, goals::melee_attack(1.0, true, "MeleeAttackGoal")),
        wrap(6, Goal::Tame(TameGoal::follow_owner(1.0, 10.0, 2.0))),
        wrap(
            7,
            Goal::Breed {
                speed: 1.0,
                partner: None,
                love_time: 0,
            },
        ),
        wrap(
            8,
            Goal::Stroll {
                speed: 1.0,
                interval: 120,
                probability: 0.001,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        wrap(9, Goal::Wolf(WolfGoal::beg())),
        wrap(
            10,
            Goal::LookAtPlayer {
                look_distance: 8.0,
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
    ];
    selector
}

/// `Wolf.registerGoals` 的 targetSelector。
pub(super) fn target_goals() -> GoalSelector {
    use super::wolf_goals::WolfTarget;
    let mut selector = GoalSelector::default();
    selector.goals = vec![
        wrap(
            1,
            Goal::Tame(TameGoal::OwnerHurtBy {
                attacker: None,
                timestamp: 0,
            }),
        ),
        wrap(
            2,
            Goal::Tame(TameGoal::OwnerHurt {
                victim: None,
                timestamp: 0,
            }),
        ),
        wrap(3, Goal::Wolf(WolfGoal::hurt_by())),
        wrap(4, Goal::Wolf(WolfGoal::nearest(WolfTarget::AngryPlayer))),
        wrap(5, Goal::Wolf(WolfGoal::nearest(WolfTarget::Prey))),
        wrap(6, Goal::Wolf(WolfGoal::nearest(WolfTarget::BabyTurtle))),
        wrap(7, Goal::Wolf(WolfGoal::nearest(WolfTarget::Skeleton))),
        wrap(8, Goal::Wolf(WolfGoal::ResetUniversalAnger)),
    ];
    selector
}

/// `Wolf.applyTamingSideEffects`：驯服后最大生命 40 并回满，否则回到 8。
pub(super) fn apply_taming_side_effects(mob: &mut Mob) {
    // `getAttribute(MAX_HEALTH).setBaseValue(...)`：实例化，存盘和同步都会带上。
    // `getAttribute` instantiates the entry, so it is saved and synced.
    let base = if mob.tame.is_tame() {
        f64::from(TAME_HEALTH)
    } else {
        START_HEALTH
    };
    if let Some(max) = mob.attributes.instance(attribute::MAX_HEALTH) {
        max.base = base;
    }
    if mob.tame.is_tame() {
        mob.health = TAME_HEALTH;
    }
}

// ---- 生成和繁殖 ----

/// `Wolf.finalizeSpawn` 在 `super` 之前：按群系选品种（`Util.getRandomSafe` 取一次
/// `nextInt(候选数)`），再在七种叫声里 `nextInt(7)`。
///
/// The part of `Wolf.finalizeSpawn` before `super`: the biome variant and a
/// random sound variant, both from the level random.
///
/// `pack` 是狼群共用的品种（`WolfPackData`）；有它就不再选、也不取那一次数。
/// 返回这只狼的品种，给调用方记进狼群数据。
///
/// `pack` is the variant shared by the pack (`WolfPackData`); with it nothing is
/// selected or drawn. Returns the variant for the caller to keep as pack data.
pub(super) fn finalize_spawn(
    mob: &mut Mob,
    pack: Option<i32>,
    biome: i32,
    random: &dyn crate::random::RandomSource,
) -> i32 {
    let variant = pack.unwrap_or_else(|| {
        let _ = random.next_int(1);
        id_of(variant_names(), spawn_variant(biome))
    });
    mob.wolf.variant = variant;
    mob.wolf.sound_variant = random_sound_variant(|bound| random.next_int(bound));
    variant
}

/// `wolf_variant` 的生成条件：八种各认一个群系或群系标签（优先级 1），其余是苍白狼。
///
/// The `wolf_variant` spawn conditions: eight biome checks at priority 1,
/// with the pale wolf as the fallback.
fn spawn_variant(biome: i32) -> &'static str {
    use mc_bamboo_data::registries::biome;
    let tagged = |tag: &str| super::spawn::biome_tag_contains(tag, biome);
    if tagged("minecraft:is_savanna") {
        "minecraft:spotted"
    } else if biome == biome::GROVE {
        "minecraft:snowy"
    } else if biome == biome::OLD_GROWTH_PINE_TAIGA {
        "minecraft:black"
    } else if biome == biome::SNOWY_TAIGA {
        "minecraft:ashen"
    } else if tagged("minecraft:is_jungle") {
        "minecraft:rusty"
    } else if biome == biome::FOREST {
        "minecraft:woods"
    } else if biome == biome::OLD_GROWTH_SPRUCE_TAIGA {
        "minecraft:chestnut"
    } else if tagged("minecraft:is_badlands") {
        "minecraft:striped"
    } else {
        "minecraft:pale"
    }
}

/// `WolfSoundVariants.pickRandomSoundVariant`：注册表 `getRandom`。
fn random_sound_variant(mut next_int: impl FnMut(i32) -> i32) -> i32 {
    let count = i32::try_from(sound_variant_names().len()).unwrap_or(1);
    next_int(count)
}

/// `Wolf.getBreedOffspring`：父亲随机源 `nextBoolean` 选品种；驯服的父亲把主人和
/// 混出来的项圈色传下去；最后父亲随机源 `nextInt(7)` 选叫声。
///
/// `Wolf.getBreedOffspring`: the father's random picks a parent's variant;
/// a tamed father passes on its owner and the mixed collar; then a sound variant.
pub(crate) fn offspring(
    me: &mut Entity,
    father: (&Wolf, &tame::Tamable),
    mother: Option<Wolf>,
    world: &mut crate::world::World,
) -> (Wolf, tame::Tamable) {
    let (father_wolf, father_tame) = father;
    let mother_variant = mother.map_or(father_wolf.variant, |wolf| wolf.variant);
    let mut wolf = Wolf {
        variant: if me.random.next_boolean() {
            father_wolf.variant
        } else {
            mother_variant
        },
        ..Wolf::default()
    };
    let mut tamable = tame::Tamable::default();
    if father_tame.is_tame() {
        // `setOwnerReference` 加 `setTame(true, true)`：幼崽的最大生命在落地时设。
        // The owner plus `setTame(true, true)`; health is applied on landing.
        tamable.owner = father_tame.owner;
        tamable.flags |= tame::FLAG_TAME;
        let mother_collar = mother.map_or(father_wolf.collar, |wolf| wolf.collar);
        wolf.collar = mixed_dye(world, father_wolf.collar, mother_collar);
    }
    wolf.sound_variant = random_sound_variant(|bound| me.random.next_int_bounded(bound));
    (wolf, tamable)
}

/// `DyeColor.getMixedColor`：拿两种染料按 2×1 的合成格查配方，产物带 `dye` 组件就用它；
/// 查不到时世界随机源 `nextBoolean` 二选一。
///
/// `DyeColor.getMixedColor`: look the two dyes up as a 2×1 crafting grid;
/// otherwise the level random picks one of them.
fn mixed_dye(world: &mut crate::world::World, first: i32, second: i32) -> i32 {
    if let Some(mixed) = dye_from_recipes(first, second) {
        return mixed;
    }
    if world.random().next_boolean() {
        first
    } else {
        second
    }
}

fn dye_from_recipes(first: i32, second: i32) -> Option<i32> {
    use mc_bamboo_recipes::{CraftingInput, RecipeManager, assemble};
    let firsts = items_with_dye(first);
    let seconds = items_with_dye(second);
    for &a in &firsts {
        for &b in &seconds {
            let slots = [ItemStack::new(a, 1), ItemStack::new(b, 1)];
            let input = CraftingInput::of(2, 1, &slots);
            let Some(recipe) = RecipeManager::global().get_recipe_for(
                mc_bamboo_data::RecipeType::Crafting,
                &input,
                None,
            ) else {
                continue;
            };
            if let Some(result) = assemble(recipe, input.slots())
                && let Some(color) = dye_of(&result)
            {
                return Some(color);
            }
        }
    }
    None
}

/// 物品注册表里默认带这种 `dye` 组件的物品，按注册表顺序。
fn items_with_dye(color: i32) -> Vec<i32> {
    (0..items::ITEMS.len())
        .filter_map(|item| i32::try_from(item).ok())
        .filter(|&item| dye_of(&ItemStack::new(item, 1)) == Some(color))
        .collect()
}

/// 物品的 `dye` 组件。
pub(super) fn dye_of(stack: &ItemStack) -> Option<i32> {
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, defaults};
    if stack.is_empty() {
        return None;
    }
    let patch = ComponentPatch::from_opaque(stack).ok()?;
    match defaults::resolve(stack.item, &patch, kinds::DYE) {
        Some(ComponentValue::Dye(color)) => Some(*color),
        _ => None,
    }
}

/// 物品 `food` 组件的营养值。
fn nutrition_of(stack: &ItemStack) -> Option<i32> {
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, defaults};
    let patch = ComponentPatch::from_opaque(stack).ok()?;
    match defaults::resolve(stack.item, &patch, kinds::FOOD) {
        Some(ComponentValue::Food { nutrition, .. }) => Some(*nutrition),
        _ => None,
    }
}

/// `Wolf.canMate`：双方都驯服、对方没坐着、都在发情。
///
/// `Wolf.canMate`: both tame, the partner not sitting, both in love.
pub(super) fn can_mate(mob: &Mob, partner: &super::MobPublic) -> bool {
    mob.mob_type != MobType::Wolf
        || (mob.tame.is_tame()
            && partner.tame_flags & tame::FLAG_TAME != 0
            && partner.tame_flags & tame::FLAG_SITTING == 0)
}

// ---- 交互 ----

/// `Wolf.mobInteract`。`now` 是岛时钟，判断还在不在生气。
///
/// `Wolf.mobInteract`; `now` is the island clock for the anger check.
pub(super) fn interact(
    me: &mut Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
    now: i64,
) -> MobInteraction {
    let held = &req.held;
    let consume = !req.creative;
    if mob.tame.is_tame() {
        let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
        if is_food(held) && mob.health < max {
            // `TamableAnimal.feed(player, hand, stack, 2, 2)`。
            let heal = nutrition_of(held).map_or(2.0, |nutrition| 2.0 * nutrition as f32);
            if mob.health > 0.0 {
                mob.health = (mob.health + heal).clamp(0.0, max);
            }
            return outcome(InteractionResult::Success, consume);
        }
        let owned = mob.tame.is_owned_by(Some(req.player_uuid));
        let dye = item_is_tagged(held.item, item_tags::WOLF_COLLAR_DYES);
        if !dye || !owned {
            let animal = super::interact::animal_interact(me, mob, req);
            if !animal.result.consumes_action() && owned {
                mob.tame.ordered_to_sit = !mob.tame.ordered_to_sit;
                mob.jumping = false;
                mob.navigation.stop();
                mob.target = None;
                return outcome(InteractionResult::Success, false);
            }
            return animal;
        }
        if let Some(color) = dye_of(held)
            && color != mob.wolf.collar
        {
            mob.wolf.collar = color;
            return outcome(InteractionResult::Success, consume);
        }
    } else if held.item == items::BONE && !mob.wolf.is_angry(now) {
        try_to_tame(me, mob, req.player_uuid);
        return outcome(InteractionResult::SuccessServer, consume);
    }
    super::interact::animal_interact(me, mob, req)
}

fn outcome(result: InteractionResult, shrink_held: bool) -> MobInteraction {
    MobInteraction {
        result,
        shrink_held,
        ..MobInteraction::pass()
    }
}

/// `Wolf.isFood`：`#wolf_food`。
pub(super) fn is_food(stack: &ItemStack) -> bool {
    !stack.is_empty() && item_is_tagged(stack.item, item_tags::WOLF_FOOD)
}

/// `Wolf.tryToTame`：狼自己的随机源 `nextInt(3) == 0` 驯服，事件 7；否则事件 6。
fn try_to_tame(me: &mut Entity, mob: &mut Mob, player: [u8; 16]) {
    if me.random.next_int_bounded(3) == 0 {
        tame::tame(mob, player);
        mob.navigation.stop();
        mob.target = None;
        mob.tame.ordered_to_sit = true;
        mob.events.push(MobEvent::EntityEvent { event: 7 });
    } else {
        mob.events.push(MobEvent::EntityEvent { event: 6 });
    }
}

// ---- 受伤 ----

/// `Wolf.hurtServer`：没有免疫这次伤害就先站起来。
///
/// `Wolf.hurtServer`: stand up unless the damage is ignored.
pub(super) fn before_hurt(mob: &mut Mob) {
    if mob.mob_type == MobType::Wolf {
        mob.tame.ordered_to_sit = false;
    }
}

/// `Wolf.wantsToAttack`：不打驯服的动物（同一个主人的狼也不打）。
/// 苦力怕、恶魂、盔甲架和马在 Bamboo 里还没有。
///
/// `Wolf.wantsToAttack`: tamed animals are spared.
pub(super) fn wants_to_attack(ctx: &dyn EntityCtx, target: EntityUid, owner: &PlayerView) -> bool {
    let Some(other) = ctx.other(target) else {
        // 玩家：没有 PVP 规则，`canHarmPlayer` 恒真。
        // Players: no PVP rules, so `canHarmPlayer` holds.
        return true;
    };
    let KindPublic::Mob(public) = &other.kind else {
        return true;
    };
    let tame = public.tame_flags & tame::FLAG_TAME != 0;
    if public.mob_type == MobType::Wolf {
        return !tame || public.owner != owner.uuid;
    }
    !(tame::is_tamable(public.mob_type) && tame)
}

// ---- 持续仇恨（NeutralMob）----

/// `Wolf.startPersistentAngerTimer`：`TimeUtil.rangeOfSeconds(20, 39)` 取一次。
fn start_persistent_anger_timer(me: &mut Entity, mob: &mut Mob, now: i64) {
    let ticks = me.random.next_int_bounded(39 * 20 - 20 * 20 + 1) + 20 * 20;
    mob.wolf.anger_end_time = now + i64::from(ticks);
}

/// `NeutralMob.stopBeingAngry`：`setLastHurtByMob(null)` 会把时间戳设成现在。
///
/// `NeutralMob.stopBeingAngry`; `setLastHurtByMob(null)` stamps the time.
pub(super) fn stop_being_angry(me: &Entity, mob: &mut Mob) {
    mob.last_hurt_by_mob = None;
    mob.last_hurt_by_mob_ref = None;
    mob.last_hurt_by_mob_timestamp = me.tick_count;
    mob.wolf.angry_at = None;
    mob.wolf.angry_at_uid = None;
    mob.target = None;
    mob.wolf.anger_end_time = -1;
}

/// 实体当前的 UUID（玩家或者生物）。
fn uuid_of(ctx: &dyn EntityCtx, uid: EntityUid) -> Option<[u8; 16]> {
    if let Some(player) = goals::player_by_uid(ctx.blocks(), uid) {
        return player.uuid;
    }
    match ctx.other(uid)?.kind {
        KindPublic::Mob(public) => Some(public.uuid),
        _ => None,
    }
}

/// `persistentAngerTarget.matches(entity)`。
fn anger_matches(mob: &Mob, ctx: &dyn EntityCtx, uid: EntityUid) -> bool {
    if mob.wolf.angry_at.is_none() {
        return false;
    }
    if mob.wolf.angry_at_uid == Some(uid) {
        return true;
    }
    uuid_of(ctx, uid).is_some_and(|uuid| mob.wolf.angry_at == Some(uuid))
}

/// `NeutralMob.updatePersistentAnger(level, true)`，在 `Wolf.aiStep` 的最后。
///
/// `NeutralMob.updatePersistentAnger(level, true)` at the end of `Wolf.aiStep`.
fn update_persistent_anger(me: &mut Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    let view = ctx.blocks();
    let now = view.game_time();
    let original = mob.wolf.angry_at;
    let had_anger_target = original.is_some();
    if let Some(previous) = mob.target
        && had_anger_target
        && goals::player_by_uid(view, previous).is_none()
        && previous_mob_dead(ctx, previous)
        && anger_matches(mob, ctx, previous)
    {
        stop_being_angry(me, mob);
        return;
    }
    let target = goals::combat_target(mob, ctx);
    if let Some(target) = target {
        let new_target = !anger_matches(mob, ctx, target.uid);
        if new_target {
            mob.wolf.angry_at = uuid_of(ctx, target.uid);
            mob.wolf.angry_at_uid = Some(target.uid);
        }
        start_persistent_anger_timer(me, mob, now);
    }
    let valid_player = target
        .and_then(|target| goals::player_by_uid(view, target.uid))
        .is_some_and(|player| !player.creative && !player.spectator && view.difficulty() != 0);
    if had_anger_target && !mob.wolf.is_angry(now) && !valid_player {
        stop_being_angry(me, mob);
    }
    // 官方这里读的是更新之前的 `persistentAngerTarget`。
    // Vanilla reads the anger target from before this update.
    let excused = original
        .and_then(|uuid| player_by_uuid(view, uuid))
        .is_some_and(|player| player.creative || player.spectator || view.difficulty() == 0);
    if excused {
        stop_being_angry(me, mob);
    }
}

/// 原始目标是不是一头已经死了（或者已经离开实体表）的生物。
fn previous_mob_dead(ctx: &dyn EntityCtx, uid: EntityUid) -> bool {
    match ctx.other(uid) {
        Some(other) => match &other.kind {
            KindPublic::Mob(public) => !public.is_alive() || other.removed,
            _ => false,
        },
        None => true,
    }
}

fn player_by_uuid(view: &dyn BlockView, uuid: [u8; 16]) -> Option<PlayerView> {
    let mut found = None;
    view.for_each_player(&mut |player| {
        if found.is_none() && player.uuid == Some(uuid) {
            found = Some(*player);
        }
    });
    found
}

/// `NeutralMob.isAngryAt`（`universalAnger` 规则 Bamboo 没有，按关闭算）。
///
/// `NeutralMob.isAngryAt`; the `universalAnger` rule is treated as off.
pub(super) fn is_angry_at(mob: &Mob, ctx: &dyn EntityCtx, uid: EntityUid) -> bool {
    anger_matches(mob, ctx, uid)
}

// ---- 每刻的钩子 ----

/// `Wolf.aiStep` 在 `super.aiStep()` 之后：湿了、没在抖、没在寻路、站在地上就开始抖
/// （事件 8），然后更新持续仇恨。
///
/// The part of `Wolf.aiStep` after `super.aiStep()`.
pub(super) fn ai_step_after(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if mob.wolf.is_wet && !mob.wolf.is_shaking && mob.navigation.is_done() && me.on_ground {
        mob.wolf.is_shaking = true;
        mob.wolf.shake_anim = 0.0;
        mob.wolf.shake_anim_o = 0.0;
        mob.events.push(MobEvent::EntityEvent { event: 8 });
    }
    update_persistent_anger(me, mob, ctx);
}

/// `Wolf.tick` 在 `super.tick()` 之后：沾水变湿（正在抖就发 56 取消），否则把抖水
/// 动画推进 0.05，过了 0.4 按 `Mth.sin` 撒水花（每颗两个 `nextFloat`）。
///
/// The part of `Wolf.tick` after `super.tick()`: get wet, or advance the shake.
pub(super) fn tick_after(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if !living::is_alive(me, mob) {
        return;
    }
    let pos = me.block_position();
    let rained_on = ctx.blocks().is_raining_at(pos)
        || ctx.blocks().is_raining_at(BlockPos::containing(Vec3::new(
            me.position.x,
            me.bounding_box().max_y,
            me.position.z,
        )));
    if me.in_water || rained_on {
        mob.wolf.is_wet = true;
        if mob.wolf.is_shaking {
            mob.events.push(MobEvent::EntityEvent { event: 56 });
            mob.wolf.is_shaking = false;
            mob.wolf.shake_anim = 0.0;
            mob.wolf.shake_anim_o = 0.0;
        }
        return;
    }
    if !mob.wolf.is_shaking {
        return;
    }
    if mob.wolf.shake_anim == 0.0 {
        let pitch = (me.random.next_float() - me.random.next_float()) * 0.2 + 1.0;
        living::play_sound(me, mob, "minecraft:entity.wolf.shake", 0.4, pitch);
    }
    mob.wolf.shake_anim_o = mob.wolf.shake_anim;
    mob.wolf.shake_anim += 0.05;
    if mob.wolf.shake_anim_o >= 2.0 {
        mob.wolf.is_wet = false;
        mob.wolf.is_shaking = false;
        mob.wolf.shake_anim_o = 0.0;
        mob.wolf.shake_anim = 0.0;
    }
    if mob.wolf.shake_anim > 0.4 {
        let angle = (mob.wolf.shake_anim - 0.4) * std::f32::consts::PI;
        let count = (mth::sin(f64::from(angle)) * 7.0) as i32;
        // 水花只在客户端显示，偏移照取。
        // Splash particles are client-side; the offsets are still drawn.
        for _ in 0..count {
            me.random.next_float();
            me.random.next_float();
        }
    }
}

/// `Wolf.die`：清掉湿和抖。
pub(super) fn on_death(mob: &mut Mob) {
    mob.wolf.is_wet = false;
    mob.wolf.is_shaking = false;
    mob.wolf.shake_anim_o = 0.0;
    mob.wolf.shake_anim = 0.0;
}

// ---- 声音 ----

/// 一套叫声（`WolfSoundVariant.WolfSoundSet`）里的一种。
///
/// One sound of a `WolfSoundSet`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Ambient,
    Death,
    Growl,
    Hurt,
    Pant,
    Whine,
    Step,
}

macro_rules! sound_set {
    ($id:literal) => {
        [
            concat!("minecraft:entity.", $id, ".ambient"),
            concat!("minecraft:entity.", $id, ".death"),
            concat!("minecraft:entity.", $id, ".growl"),
            concat!("minecraft:entity.", $id, ".hurt"),
            concat!("minecraft:entity.", $id, ".pant"),
            concat!("minecraft:entity.", $id, ".whine"),
            "minecraft:entity.wolf.step",
        ]
    };
}

/// `SoundEvents.registerWolfSoundVariants` 的成年声音，按叫声名排。
const ADULT_SOUNDS: [(&str, [&str; 7]); 7] = [
    ("minecraft:classic", sound_set!("wolf")),
    ("minecraft:puglin", sound_set!("wolf_puglin")),
    ("minecraft:sad", sound_set!("wolf_sad")),
    ("minecraft:angry", sound_set!("wolf_angry")),
    ("minecraft:grumpy", sound_set!("wolf_grumpy")),
    ("minecraft:big", sound_set!("wolf_big")),
    ("minecraft:cute", sound_set!("wolf_cute")),
];

/// 所有叫声共用的幼崽声音。
const BABY_SOUNDS: [&str; 7] = [
    "minecraft:entity.baby_wolf.ambient",
    "minecraft:entity.baby_wolf.death",
    "minecraft:entity.baby_wolf.growl",
    "minecraft:entity.baby_wolf.hurt",
    "minecraft:entity.baby_wolf.pant",
    "minecraft:entity.baby_wolf.whine",
    "minecraft:entity.baby_wolf.step",
];

/// `Wolf.getSoundSet()` 里的一种声音。
pub fn sound(mob: &Mob, which: Sound) -> &'static str {
    let index = which as usize;
    if mob.is_baby() {
        return BABY_SOUNDS[index];
    }
    let name = mob.wolf.sound_variant_name();
    ADULT_SOUNDS
        .iter()
        .find(|(variant, _)| *variant == name)
        .map_or(ADULT_SOUNDS[0].1[index], |(_, set)| set[index])
}

/// `Wolf.getAmbientSound`：生气时低吼；否则 `nextInt(3) == 0` 时喘气（驯服且血少于 20
/// 时呜咽），其余是普通叫声。
pub(super) fn ambient_sound(me: &mut Entity, mob: &Mob, now: i64) -> &'static str {
    if mob.wolf.is_angry(now) {
        return sound(mob, Sound::Growl);
    }
    if me.random.next_int_bounded(3) == 0 {
        return if mob.tame.is_tame() && mob.health < 20.0 {
            sound(mob, Sound::Whine)
        } else {
            sound(mob, Sound::Pant)
        };
    }
    sound(mob, Sound::Ambient)
}

/// `Mob.playAmbientSound` 在狼身上：先问叫声（可能取一次随机数），再取音高。
pub(super) fn play_ambient_sound(me: &mut Entity, mob: &mut Mob, now: i64) {
    let sound = ambient_sound(me, mob, now);
    let pitch = living::voice_pitch(me, mob);
    living::play_sound(me, mob, sound, 0.4, pitch);
}

/// `Wolf.getMaxHeadXRot`：坐着时 20。
pub(super) const fn max_head_x_rot(mob: &Mob) -> f32 {
    if mob.tame.in_sitting_pose() {
        20.0
    } else {
        40.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_registry_ids_follow_the_synced_order() {
        assert_eq!(variant_id("minecraft:pale"), Some(default_variant()));
        assert_eq!(variant_id("minecraft:husky"), None);
        assert_eq!(Wolf::default().sound_variant_name(), "minecraft:classic");
        assert_eq!(sound_variant_names().len(), 7);
    }

    #[test]
    fn biomes_pick_the_matching_variant() {
        use mc_bamboo_data::registries::biome;
        assert_eq!(spawn_variant(biome::FOREST), "minecraft:woods");
        assert_eq!(spawn_variant(biome::SAVANNA), "minecraft:spotted");
        assert_eq!(spawn_variant(biome::PLAINS), "minecraft:pale");
        assert_eq!(spawn_variant(biome::BADLANDS), "minecraft:striped");
    }

    #[test]
    fn anger_lasts_until_the_end_time() {
        let wolf = Wolf {
            anger_end_time: 100,
            ..Wolf::default()
        };
        assert!(wolf.is_angry(99));
        assert!(!wolf.is_angry(100));
        assert!(!Wolf::default().is_angry(0));
    }

    #[test]
    fn red_and_yellow_dyes_mix_to_orange() {
        // `DyeColor` IDs: red 14, yellow 4, orange 1.
        assert_eq!(dye_from_recipes(14, 4), Some(1));
        assert_eq!(dye_from_recipes(14, 14), None);
    }

    fn request(item: i32, player: u8) -> MobInteractRequest {
        MobInteractRequest {
            player_uid: 1,
            player_uuid: [player; 16],
            hand: mc_bamboo_core::Hand::Main,
            held: if item == items::AIR {
                ItemStack::empty()
            } else {
                ItemStack::new(item, 1)
            },
            creative: false,
            position: Vec3::default(),
            sneaking: false,
        }
    }

    /// 一直喂骨头直到驯服：驯服后满血 40、坐下、事件 7；之前每次失败都是事件 6。
    fn tamed_wolf() -> (Entity, Mob) {
        let mut me = Entity::by_mob_type(MobType::Wolf, Vec3::default());
        let mut mob = Mob::new(MobType::Wolf);
        for _ in 0..64 {
            let outcome = interact(&mut me, &mut mob, &request(items::BONE, 7), 0);
            assert_eq!(outcome.result, InteractionResult::SuccessServer);
            assert!(outcome.shrink_held);
            if mob.tame.is_tame() {
                break;
            }
            assert_eq!(mob.events.pop(), Some(MobEvent::EntityEvent { event: 6 }));
        }
        assert_eq!(mob.events.pop(), Some(MobEvent::EntityEvent { event: 7 }));
        (me, mob)
    }

    #[test]
    fn a_bone_tames_and_seats_the_wolf() {
        let (_, mob) = tamed_wolf();
        assert!(mob.tame.is_owned_by(Some([7; 16])));
        assert!(mob.tame.ordered_to_sit);
        assert_eq!(mob.health, 40.0);
        assert_eq!(mob.attributes.value(attribute::MAX_HEALTH), 40.0);
    }

    #[test]
    fn the_owner_toggles_sitting_but_strangers_do_not() {
        let (mut me, mut mob) = tamed_wolf();
        let outcome = interact(&mut me, &mut mob, &request(items::AIR, 7), 0);
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(!outcome.shrink_held);
        assert!(!mob.tame.ordered_to_sit);
        let outcome = interact(&mut me, &mut mob, &request(items::AIR, 8), 0);
        assert_eq!(outcome.result, InteractionResult::Pass);
        assert!(!mob.tame.ordered_to_sit);
    }

    #[test]
    fn dyes_recolour_the_collar_once() {
        let (mut me, mut mob) = tamed_wolf();
        let blue = items_with_dye(11)[0];
        let outcome = interact(&mut me, &mut mob, &request(blue, 7), 0);
        assert!(outcome.shrink_held);
        assert_eq!(mob.wolf.collar, 11);
        // 同色染料走 `Animal.mobInteract`，什么都不做，也不切换坐姿。
        let sitting = mob.tame.ordered_to_sit;
        let outcome = interact(&mut me, &mut mob, &request(blue, 7), 0);
        assert_eq!(outcome.result, InteractionResult::Pass);
        assert_eq!(mob.tame.ordered_to_sit, sitting);
    }

    #[test]
    fn meat_heals_twice_its_nutrition() {
        let (mut me, mut mob) = tamed_wolf();
        mob.health = 10.0;
        let outcome = interact(&mut me, &mut mob, &request(items::BEEF, 9), 0);
        assert_eq!(outcome.result, InteractionResult::Success);
        assert_eq!(mob.health, 16.0);
    }

    #[test]
    fn an_angry_wolf_refuses_bones() {
        let mut me = Entity::by_mob_type(MobType::Wolf, Vec3::default());
        let mut mob = Mob::new(MobType::Wolf);
        mob.wolf.anger_end_time = 50;
        let outcome = interact(&mut me, &mut mob, &request(items::BONE, 7), 10);
        assert_eq!(outcome.result, InteractionResult::Pass);
        assert!(!mob.tame.is_tame());
    }

    #[test]
    fn wolves_survive_a_save_and_load() {
        let (mut me, mut mob) = tamed_wolf();
        mob.wolf.collar = 3;
        mob.wolf.variant = variant_id("minecraft:snowy").unwrap();
        mob.wolf.sound_variant = sound_variant_id("minecraft:cute").unwrap();
        mob.wolf.anger_end_time = 1234;
        mob.wolf.angry_at = Some([5; 16]);
        me.kind = crate::entity::EntityKind::Mob(Box::new(mob.clone()));
        let tag = super::super::nbt::write_mob(&me.split()).expect("wolf saves");
        let row = super::super::nbt::read_mob(&tag, "minecraft:wolf", 4)
            .expect("reads")
            .expect("a wolf");
        let entity = row.into_entity();
        let loaded = entity.kind.mob().expect("a mob");
        // 读盘时 `setInSittingPose(orderedToSit)`：坐姿位直接跟着命令。
        // Loading sets the sitting pose straight from the order.
        assert_eq!(loaded.tame.owner, mob.tame.owner);
        assert!(loaded.tame.is_tame() && loaded.tame.ordered_to_sit);
        assert!(loaded.tame.in_sitting_pose());
        assert_eq!(loaded.wolf.collar, 3);
        assert_eq!(loaded.wolf.variant, mob.wolf.variant);
        assert_eq!(loaded.wolf.sound_variant, mob.wolf.sound_variant);
        assert_eq!(loaded.wolf.anger_end_time, 1234);
        assert_eq!(loaded.wolf.angry_at, Some([5; 16]));
        assert_eq!(loaded.health, 40.0);
    }

    #[test]
    fn puppies_use_the_shared_baby_sounds() {
        let mut mob = Mob::new(MobType::Wolf);
        assert_eq!(sound(&mob, Sound::Pant), "minecraft:entity.wolf.pant");
        mob.wolf.sound_variant = sound_variant_id("minecraft:big").unwrap();
        assert_eq!(sound(&mob, Sound::Growl), "minecraft:entity.wolf_big.growl");
        mob.age = -1;
        assert_eq!(sound(&mob, Sound::Step), "minecraft:entity.baby_wolf.step");
    }
}
