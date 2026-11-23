//! 生物用弩：官方 `CrossbowAttackMob` + `RangedCrossbowAttackGoal`，以及 `CrossbowItem` /
//! `ProjectileWeaponItem` 里生物会走到的那几支（装填、`performShooting`、朝目标的射击向量）。
//! 玩家的弩在 `mc-bamboo-game` 的 `player_weapon.rs`；两边共用箭实体（[`ArrowShot`]）。
//!
//! 写成独立模块，以后的弩手（卫道士之外的掠夺兽骑手、猪灵）可以直接用。
//!
//! Crossbows in mob hands: vanilla `CrossbowAttackMob` + `RangedCrossbowAttackGoal`, plus
//! the branches of `CrossbowItem` / `ProjectileWeaponItem` a mob reaches (loading,
//! `performShooting`, the shot vector towards a target). The player crossbow lives in
//! `player_weapon.rs` of `mc-bamboo-game`; both share the arrow entity ([`ArrowShot`]).
//! A separate module so later crossbow users (piglins) can reuse it.

use mc_bamboo_core::{Vec3, mth};
use mc_bamboo_data::{items, registries::data_component_type as kinds, tags::item_tags};
use mc_bamboo_protocol::{
    components::{ComponentValue, defaults, stack::ItemStackTemplate},
    item::ItemStack,
    packet::play::EquipmentSlot,
};

use super::{Mob, MobEvent, goals, living};
use crate::entity::{
    Entity,
    arrow::{ArrowShot, Pickup, enchantment_level},
    ctx::{Command, EntityCtx},
};

/// `CrossbowItem.getUseDuration`。
const USE_DURATION: i32 = 72000;

/// `LivingEntity` 用物品的那几个字段里弩用得到的，加上 `CrossbowItem` 的两个音效标记。
/// 官方那两个标记挂在物品单例上、所有使用者共用；这里每只生物各记各的（只影响音效）。
///
/// The `LivingEntity` use-item state a crossbow needs, plus `CrossbowItem`'s two sound
/// flags. Vanilla keeps those flags on the item singleton, shared by every user; here
/// each mob has its own (sounds only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CrossbowUse {
    /// `useItemRemaining`；没在用时是 0。
    ///
    /// `useItemRemaining`; 0 while not using.
    pub remaining: i32,
    start_sound: bool,
    mid_sound: bool,
    /// `Pillager.IS_CHARGING_CROSSBOW`。
    pub charging: bool,
}

/// `RangedCrossbowAttackGoal.CrossbowState`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum CrossbowState {
    #[default]
    Uncharged,
    Charging,
    Charged,
    ReadyToAttack,
}

/// `RangedCrossbowAttackGoal(mob, speedModifier, attackRadius)`。
#[derive(Debug, Clone, PartialEq)]
pub struct CrossbowAttack {
    state: CrossbowState,
    speed: f64,
    attack_radius_sqr: f32,
    see_time: i32,
    attack_delay: i32,
    update_path_delay: i32,
}

/// `mob.isHolding(Items.CROSSBOW)`：主手或副手。
///
/// `mob.isHolding(Items.CROSSBOW)`: either hand.
pub(crate) fn is_holding(mob: &Mob) -> bool {
    [EquipmentSlot::MainHand, EquipmentSlot::OffHand]
        .into_iter()
        .any(|slot| mob.equipment.get(slot).item == items::CROSSBOW)
}

/// `ProjectileUtil.getWeaponHoldingHand(mob, Items.CROSSBOW)`：主手是弩就是主手，否则副手。
///
/// `ProjectileUtil.getWeaponHoldingHand(mob, Items.CROSSBOW)`: the main hand when it
/// holds a crossbow, otherwise the off hand.
fn weapon_hand(mob: &Mob) -> EquipmentSlot {
    if mob.equipment.get(EquipmentSlot::MainHand).item == items::CROSSBOW {
        EquipmentSlot::MainHand
    } else {
        EquipmentSlot::OffHand
    }
}

/// `LivingEntity.isUsingItem()`。
pub const fn is_using(mob: &Mob) -> bool {
    mob.using_item_flags & 1 != 0
}

/// `getUsedItemHand()`。
const fn used_hand(mob: &Mob) -> EquipmentSlot {
    if mob.using_item_flags & 2 != 0 {
        EquipmentSlot::OffHand
    } else {
        EquipmentSlot::MainHand
    }
}

/// `LivingEntity.startUsingItem(hand)`。
fn start_using(mob: &mut Mob, hand: EquipmentSlot) {
    if mob.equipment.get(hand).is_empty() || is_using(mob) {
        return;
    }
    mob.crossbow.remaining = USE_DURATION;
    // `updatePlayersWithNewEquipment`：开始用的时候把这只手的物品再发一遍。
    // `updatePlayersWithNewEquipment`: the hand's item is sent again when use starts.
    mob.events.push(MobEvent::Equipment(vec![(
        hand,
        mob.equipment.get(hand).clone(),
    )]));
    mob.using_item_flags = if hand == EquipmentSlot::OffHand { 3 } else { 1 };
}

/// `LivingEntity.stopUsingItem()`：只清「正在用」那一位，副手位留着。
///
/// `LivingEntity.stopUsingItem()`: clears the using bit only; the off-hand bit stays.
pub(crate) fn stop_using(mob: &mut Mob) {
    mob.using_item_flags &= !1;
    mob.crossbow.remaining = 0;
}

/// `LivingEntity.getTicksUsingItem()`。
pub const fn ticks_using(mob: &Mob) -> i32 {
    if is_using(mob) {
        USE_DURATION - mob.crossbow.remaining
    } else {
        0
    }
}

/// `CrossbowItem.getChargeDuration`：1.25 秒，快速装填每级减 0.25 秒。
///
/// `CrossbowItem.getChargeDuration`: 1.25 seconds, minus 0.25 per Quick Charge level.
pub(crate) fn charge_duration(crossbow: &ItemStack) -> i32 {
    let quick = enchantment_level(crossbow, "minecraft:quick_charge").max(0) as f32;
    let seconds = 1.25 + (-0.25 * quick);
    mth::floor(f64::from(seconds * 20.0))
}

/// `CrossbowItem.isCharged`。
pub(crate) fn is_charged(stack: &ItemStack) -> bool {
    !charged_projectiles(stack).is_empty()
}

/// 弩里装着几支弹药（对拍用）。
///
/// How many projectiles the crossbow holds (for the oracle).
pub fn charged_count(stack: &ItemStack) -> usize {
    charged_projectiles(stack).len()
}

fn charged_projectiles(stack: &ItemStack) -> Vec<ItemStackTemplate> {
    match defaults::resolve(stack.item, &stack.patch(), kinds::CHARGED_PROJECTILES) {
        Some(ComponentValue::ChargedProjectiles(projectiles)) => projectiles.clone(),
        _ => Vec::new(),
    }
}

/// `LivingEntity.updatingUsingItem`：每刻在 `aiStep` 之前。手里那件不再是弩就停。
///
/// `LivingEntity.updatingUsingItem`, each tick ahead of `aiStep`. Stops when the hand no
/// longer holds the crossbow.
pub(super) fn use_tick(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    if !is_using(mob) {
        return;
    }
    if mob.equipment.get(used_hand(mob)).item == items::CROSSBOW {
        update_using(me, mob, ctx);
    } else {
        stop_using(mob);
    }
}

/// `LivingEntity.updateUsingItem`：先 `onUseTick`，再把剩余刻数减一。弩是 `useOnRelease`，
/// 减到 0 也不会 `completeUsingItem`。
///
/// `LivingEntity.updateUsingItem`: `onUseTick`, then one tick less. A crossbow is
/// `useOnRelease`, so reaching 0 never calls `completeUsingItem`.
fn update_using(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    on_use_tick(me, mob, ctx);
    mob.crossbow.remaining -= 1;
}

/// `CrossbowItem.onUseTick`：20% / 50% 的装填音效（玩家音源、音量 0.5），满了装箭并放
/// 装填完成音效，音高取一次世界随机数。
///
/// `CrossbowItem.onUseTick`: the 20% / 50% loading sounds (player category, volume 0.5);
/// at 100% load the arrow and play the end sound, whose pitch draws one level random.
fn on_use_tick(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    let hand = used_hand(mob);
    let stack = mob.equipment.get(hand).clone();
    let quick = enchantment_level(&stack, "minecraft:quick_charge");
    let percent = (USE_DURATION - mob.crossbow.remaining) as f32 / charge_duration(&stack) as f32;
    if percent < 0.2 {
        mob.crossbow.start_sound = false;
        mob.crossbow.mid_sound = false;
    }
    if percent >= 0.2 && !mob.crossbow.start_sound {
        mob.crossbow.start_sound = true;
        loading_sound(me, mob, start_sound(quick));
    }
    if percent >= 0.5 && !mob.crossbow.mid_sound {
        mob.crossbow.mid_sound = true;
        // 快速装填的音效组没有中段音效。
        // The Quick Charge sound sets have no middle sound.
        if quick <= 0 {
            loading_sound(me, mob, "minecraft:item.crossbow.loading_middle");
        }
    }
    if percent >= 1.0 && !is_charged(&stack) && load_projectiles(mob, hand) {
        let pitch = 1.0 / (ctx.blocks().level_random_float() * 0.5 + 1.0) + 0.2;
        mob.events.push(MobEvent::Sound {
            sound: "minecraft:item.crossbow.loading_end",
            volume: 1.0,
            pitch,
            position: me.position,
            hostile: mob.mob_type.is_enemy(),
            seed: None,
        });
    }
}

/// `CrossbowItem.getChargingSounds` 的开始音效：快速装填按等级换。
///
/// The start sound of `CrossbowItem.getChargingSounds`, which Quick Charge replaces by level.
const fn start_sound(quick: i32) -> &'static str {
    match quick {
        i32::MIN..=0 => "minecraft:item.crossbow.loading_start",
        1 => "minecraft:item.crossbow.quick_charge_1",
        2 => "minecraft:item.crossbow.quick_charge_2",
        _ => "minecraft:item.crossbow.quick_charge_3",
    }
}

fn loading_sound(me: &Entity, mob: &mut Mob, sound: &'static str) {
    mob.events.push(MobEvent::PlayerSoundAt {
        sound,
        position: me.position,
        volume: 0.5,
    });
}

/// `CrossbowItem.tryLoadProjectiles` → `ProjectileWeaponItem.draw` → `useAmmo`，射手是怪物：
/// `Monster.getProjectile` 先看副手、主手有没有箭，没有就凭空给一支普通箭。第一支是真箭，
/// 多重射击多出来的带 `intangible_projectile`。副手拿烟花火箭的情形没有建模。
///
/// `CrossbowItem.tryLoadProjectiles` → `ProjectileWeaponItem.draw` → `useAmmo` for a
/// monster: `Monster.getProjectile` checks the off and main hand for arrows and otherwise
/// conjures a plain arrow. The first one is real; Multishot extras carry
/// `intangible_projectile`. An off-hand firework rocket is not modelled.
fn load_projectiles(mob: &mut Mob, hand: EquipmentSlot) -> bool {
    let weapon = mob.equipment.get(hand).clone();
    let held = [EquipmentSlot::OffHand, EquipmentSlot::MainHand]
        .into_iter()
        .find(|&slot| {
            let stack = mob.equipment.get(slot);
            !stack.is_empty() && crate::state::item_is_tagged(stack.item, item_tags::ARROWS)
        });
    let projectile = held.map_or_else(
        || ItemStack::new(items::ARROW, 1),
        |slot| mob.equipment.get(slot).clone(),
    );
    let count = 1 + 2 * enchantment_level(&weapon, "minecraft:multishot").max(0);
    let mut drawn = Vec::new();
    for index in 0..count {
        let mut used = crate::container::copy_with_count(&projectile, 1);
        if index == 0 {
            // `projectile.split(1)`：手里真有箭时扣掉一支。
            // `projectile.split(1)`: a held arrow stack loses one.
            if let Some(slot) = held {
                mob.equipment.edit(slot, |stack| stack.count -= 1);
            }
        } else {
            crate::item_exchange::set_component(
                &mut used,
                kinds::INTANGIBLE_PROJECTILE,
                ComponentValue::IntangibleProjectile,
            );
        }
        drawn.push(ItemStackTemplate {
            item: used.item,
            count: used.count,
            components: used.patch(),
        });
    }
    if drawn.is_empty() {
        return false;
    }
    mob.equipment.edit(hand, |stack| {
        crate::item_exchange::set_component(
            stack,
            kinds::CHARGED_PROJECTILES,
            ComponentValue::ChargedProjectiles(drawn),
        );
    });
    true
}

/// `LivingEntity.releaseUsingItem()`：弩是 `useOnRelease`，松手时再跑一次
/// `updatingUsingItem`（正好满 100%，装箭），然后 `stopUsingItem`。
///
/// `LivingEntity.releaseUsingItem()`: a crossbow is `useOnRelease`, so releasing runs
/// `updatingUsingItem` once more (exactly 100%, which loads it) before `stopUsingItem`.
fn release_using(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    if is_using(mob) && mob.equipment.get(used_hand(mob)).item == items::CROSSBOW {
        use_tick(me, mob, ctx);
    }
    stop_using(mob);
}

impl CrossbowAttack {
    pub(super) const fn new(speed: f64, attack_radius: f32) -> Self {
        Self {
            state: CrossbowState::Uncharged,
            speed,
            attack_radius_sqr: attack_radius * attack_radius,
            see_time: 0,
            attack_delay: 0,
            update_path_delay: 0,
        }
    }

    /// `canUse`：目标活着，手里有弩。`canContinueToUse` 化简之后是同一个条件。
    ///
    /// `canUse`: a living target and a crossbow in hand. `canContinueToUse` reduces to the
    /// same condition.
    pub(super) fn can_use(mob: &Mob, ctx: &dyn EntityCtx) -> bool {
        goals::current_combat_target(mob, ctx).is_some() && is_holding(mob)
    }

    /// `stop`：不再进攻、清目标、清视线计数；正在装填就停下。状态机和攻击延迟不清。
    /// 官方最后那句 `getUseItem().set(CHARGED_PROJECTILES, EMPTY)` 在 `stopUsingItem` 之后，
    /// `getUseItem()` 已经是空物品，什么也不改。
    ///
    /// `stop`: no longer aggressive, target and sight counter cleared, loading stopped.
    /// The state machine and attack delay stay. Vanilla's final
    /// `getUseItem().set(CHARGED_PROJECTILES, EMPTY)` runs after `stopUsingItem`, when
    /// `getUseItem()` is already the empty stack, so it changes nothing.
    pub(super) fn stop(&mut self, mob: &mut Mob) {
        mob.aggressive = false;
        mob.target = None;
        self.see_time = 0;
        if is_using(mob) {
            stop_using(mob);
            mob.crossbow.charging = false;
        }
    }

    /// `RangedCrossbowAttackGoal.tick`。
    pub(super) fn tick(&mut self, me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
        let Some(target) = goals::current_combat_target(mob, ctx) else {
            return;
        };
        let visible =
            living::has_line_of_sight(me, mob, ctx.blocks(), target.uid, target.eye_position());
        if visible != (self.see_time > 0) {
            self.see_time = 0;
        }
        self.see_time += if visible { 1 } else { -1 };
        let distance = me.position.distance_to_sqr(target.position);
        let needs_to_move = (distance > f64::from(self.attack_radius_sqr) || self.see_time < 5)
            && self.attack_delay == 0;
        if needs_to_move {
            self.update_path_delay -= 1;
            if self.update_path_delay <= 0 {
                let speed = if self.state == CrossbowState::Uncharged {
                    self.speed
                } else {
                    self.speed * 0.5
                };
                let nav = living::nav_mob(me, mob, ctx.blocks());
                mob.navigation
                    .move_to_entity(ctx.blocks(), &nav, target.position, speed, 1);
                // `PATHFINDING_DELAY_RANGE = TimeUtil.rangeOfSeconds(1, 2)`：`UniformInt(20, 40)`。
                // `PATHFINDING_DELAY_RANGE = TimeUtil.rangeOfSeconds(1, 2)`: `UniformInt(20, 40)`.
                self.update_path_delay = 20 + me.random.next_int_bounded(21);
            }
        } else {
            self.update_path_delay = 0;
            mob.navigation.stop();
        }
        mob.look.set_look_at(
            target.position.x,
            target.eye_y(),
            target.position.z,
            30.0,
            30.0,
        );
        match self.state {
            CrossbowState::Uncharged => {
                if !needs_to_move {
                    start_using(mob, weapon_hand(mob));
                    self.state = CrossbowState::Charging;
                    mob.crossbow.charging = true;
                }
            }
            CrossbowState::Charging => {
                if !is_using(mob) {
                    self.state = CrossbowState::Uncharged;
                }
                let weapon = if is_using(mob) {
                    mob.equipment.get(used_hand(mob)).clone()
                } else {
                    ItemStack::empty()
                };
                if ticks_using(mob) >= charge_duration(&weapon) {
                    release_using(me, mob, ctx);
                    self.state = CrossbowState::Charged;
                    self.attack_delay = 20 + me.random.next_int_bounded(20);
                    mob.crossbow.charging = false;
                }
            }
            CrossbowState::Charged => {
                self.attack_delay -= 1;
                if self.attack_delay == 0 {
                    self.state = CrossbowState::ReadyToAttack;
                }
            }
            CrossbowState::ReadyToAttack => {
                if visible {
                    perform_crossbow_attack(me, mob, ctx, target, 1.6);
                    self.state = CrossbowState::Uncharged;
                }
            }
        }
    }
}

/// `CrossbowAttackMob.performCrossbowAttack(body, power)`：拿弩的那只手 `performShooting`，
/// 散布是 `rangedAttackUncertainty`（`14 - 难度 * 4`），然后 `onCrossbowAttackPerformed`
/// 把 `noActionTime` 清零。
///
/// `CrossbowAttackMob.performCrossbowAttack(body, power)`: `performShooting` from the
/// crossbow hand with `rangedAttackUncertainty` (`14 - difficulty * 4`), then
/// `onCrossbowAttackPerformed` clears `noActionTime`.
pub(super) fn perform_crossbow_attack(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    target: goals::CombatTarget,
    power: f32,
) {
    let hand = weapon_hand(mob);
    if mob.equipment.get(hand).item == items::CROSSBOW {
        let uncertainty = (14 - ctx.blocks().difficulty() * 4) as f32;
        perform_shooting(me, mob, ctx, hand, target, power, uncertainty);
    }
    mob.no_action_time = 0;
}

/// `CrossbowItem.performShooting` → `ProjectileWeaponItem.shoot`：清掉装着的弹药，逐支射出；
/// 每支射完扣 1 点耐久，弩坏了就停。箭由 [`Command::ShootCrossbow`] 落地生成。
///
/// `CrossbowItem.performShooting` → `ProjectileWeaponItem.shoot`: clear the charged
/// projectiles and fire them one by one; each costs 1 durability and a broken crossbow
/// ends the volley. [`Command::ShootCrossbow`] spawns the arrows.
fn perform_shooting(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    hand: EquipmentSlot,
    target: goals::CombatTarget,
    power: f32,
    uncertainty: f32,
) {
    let charged = charged_projectiles(mob.equipment.get(hand));
    mob.equipment.edit(hand, |stack| {
        crate::item_exchange::set_component(
            stack,
            kinds::CHARGED_PROJECTILES,
            ComponentValue::ChargedProjectiles(Vec::new()),
        );
    });
    if charged.is_empty() {
        return;
    }
    let max_angle =
        enchantment_level(mob.equipment.get(hand), "minecraft:multishot").max(0) as f32 * 10.0;
    let step = if charged.len() == 1 {
        0.0
    } else {
        2.0 * max_angle / (charged.len() - 1) as f32
    };
    let offset = ((charged.len() - 1) % 2) as f32 * step / 2.0;
    let position = Vec3::new(
        me.position.x,
        living::eye_y(me, mob) - f64::from(0.1_f32),
        me.position.z,
    );
    let mut direction = 1.0_f32;
    let mut shots = Vec::new();
    for (index, template) in charged.iter().enumerate() {
        let angle = offset + direction * index.div_ceil(2) as f32 * step;
        direction = -direction;
        let (added, removed) = template.components.to_opaque();
        let mut ammo = ItemStack {
            item: template.item,
            count: template.count,
            added,
            removed,
        };
        if ammo.is_empty() {
            continue;
        }
        // `AbstractArrow` 的构造函数：带 `intangible_projectile` 的摘掉组件，只有创造能捡；
        // 生物射的其余的箭捡不了。
        // The `AbstractArrow` constructor strips `intangible_projectile` and makes such an
        // arrow creative-only; a mob's other arrows cannot be picked up.
        let intangible = matches!(
            defaults::resolve(ammo.item, &ammo.patch(), kinds::INTANGIBLE_PROJECTILE),
            Some(ComponentValue::IntangibleProjectile)
        );
        if intangible {
            crate::item_exchange::remove_component(&mut ammo, kinds::INTANGIBLE_PROJECTILE);
        }
        shots.push(ArrowShot {
            position,
            direction: shot_vector(me, mob, target, position.y, angle),
            power,
            uncertainty,
            source_movement: None,
            owner: Some(me.id),
            owner_uuid: Some(mob.uuid),
            ammo,
            weapon: Some(mob.equipment.get(hand).clone()),
            critical: false,
            pickup: if intangible {
                Pickup::CreativeOnly
            } else {
                Pickup::Disallowed
            },
        });
        // `CrossbowItem.getShotPitch`：第一支 1.0，其余按奇偶取一次射手的随机数。
        // `CrossbowItem.getShotPitch`: 1.0 for the first, otherwise one shooter draw.
        let pitch = if index == 0 {
            1.0
        } else {
            let range = if index & 1 == 1 { 0.63 } else { 0.43 };
            1.0 / (me.random.next_float() * 0.5 + 1.8) + range
        };
        mob.events.push(MobEvent::Sound {
            sound: "minecraft:item.crossbow.shoot",
            volume: 1.0,
            pitch,
            position: me.position,
            hostile: mob.mob_type.is_enemy(),
            seed: None,
        });
        if hurt_weapon(mob, ctx, hand) {
            break;
        }
    }
    if !shots.is_empty() {
        ctx.emit(Command::ShootCrossbow { shots }, me);
    }
}

/// `weapon.hurtAndBreak(1, shooter, slot)`；坏了返回真（手里空了）。
///
/// `weapon.hurtAndBreak(1, shooter, slot)`; true when it broke and the hand is empty.
fn hurt_weapon(mob: &mut Mob, ctx: &dyn EntityCtx, hand: EquipmentSlot) -> bool {
    let before = mob.equipment.get(hand).clone();
    let mut broke = false;
    let blocks = ctx.blocks();
    mob.equipment.edit(hand, |stack| {
        broke = blocks.hurt_mob_equipment(stack, 1);
    });
    if broke {
        mob.events.push(MobEvent::EntityEvent {
            event: super::equipment::break_event(hand),
        });
        super::equipment::remove_stack_modifiers(&before, hand, &mut mob.attributes);
    }
    mob.equipment.get(hand).is_empty()
}

/// `CrossbowItem.shootProjectile` 有目标的那一支：朝目标三分之一高处抬 `距离 * 0.2`，
/// 再交给 `getProjectileShotVector` 按角度偏转。
///
/// The targeted branch of `CrossbowItem.shootProjectile`: aim a third of the way up the
/// target, lifted by `distance * 0.2`, then let `getProjectileShotVector` apply the angle.
fn shot_vector(
    me: &Entity,
    mob: &Mob,
    target: goals::CombatTarget,
    projectile_y: f64,
    angle: f32,
) -> Vec3 {
    let xd = target.position.x - me.position.x;
    let zd = target.position.z - me.position.z;
    let distance = (xd * xd + zd * zd).sqrt();
    let yd = target.position.y + target.height * 0.333_333_333_333_333_3 - projectile_y
        + distance * f64::from(0.2_f32);
    let [x, y, z] = projectile_shot_vector(
        [xd as f32, yd as f32, zd as f32],
        up_vector(mob.x_rot, mob.y_head_rot),
        angle,
    );
    Vec3::new(f64::from(x), f64::from(y), f64::from(z))
}

/// `Entity.getUpVector(1.0F)`：`calculateViewVector(xRot - 90, yHeadRot)`，全部按单精度。
///
/// `Entity.getUpVector(1.0F)`: `calculateViewVector(xRot - 90, yHeadRot)` in single precision.
fn up_vector(x_rot: f32, y_rot: f32) -> [f32; 3] {
    let real_x = (x_rot - 90.0) * (std::f32::consts::PI / 180.0);
    let real_y = -y_rot * (std::f32::consts::PI / 180.0);
    let y_cos = mth::cos(f64::from(real_y));
    let y_sin = mth::sin(f64::from(real_y));
    let x_cos = mth::cos(f64::from(real_x));
    let x_sin = mth::sin(f64::from(real_x));
    [
        (f64::from(y_sin * x_cos)) as f32,
        (f64::from(-x_sin)) as f32,
        (f64::from(y_cos * x_cos)) as f32,
    ]
}

/// `CrossbowItem.getProjectileShotVector`：JOML 的单精度向量运算，照 `Vector3f` 的写法逐步算
/// （JOML 缺省不用融合乘加）。先归一化，和竖直方向叉乘得到「右」，绕它转 90 度得到偏转轴，
/// 再绕偏转轴转 `angle` 度。右向量没有归一化，官方就是这样。
///
/// `CrossbowItem.getProjectileShotVector` with JOML's single-precision `Vector3f` steps
/// (JOML uses no fused multiply-add by default): normalize, cross with up for the right
/// vector, rotate 90 degrees about it for the spread axis, then rotate by `angle` about
/// that axis. The right vector is not normalized, exactly as in vanilla.
fn projectile_shot_vector(original: [f32; 3], up: [f32; 3], angle: f32) -> [f32; 3] {
    let view = normalize(original);
    let mut right = cross(view, [0.0, 1.0, 0.0]);
    if f64::from(length_squared(right)) <= 1.0e-7 {
        right = cross(view, up);
    }
    let axis = rotate_axis(view, std::f32::consts::FRAC_PI_2, right);
    rotate_axis(view, angle * (std::f32::consts::PI / 180.0), axis)
}

fn length_squared([x, y, z]: [f32; 3]) -> f32 {
    x * x + (y * y + z * z)
}

/// `Vector3f.normalize`：`1.0f / (float) Math.sqrt(lengthSquared)`。
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let scalar = 1.0 / f64::from(length_squared(v)).sqrt() as f32;
    [v[0] * scalar, v[1] * scalar, v[2] * scalar]
}

/// `Vector3f.cross`。
fn cross([x, y, z]: [f32; 3], [vx, vy, vz]: [f32; 3]) -> [f32; 3] {
    [y * vz + -z * vy, z * vx + -x * vz, x * vy + -y * vx]
}

/// `Math.sin(float)`：`(float) java.lang.Math.sin(rad)`。
fn sin(angle: f32) -> f32 {
    f64::from(angle).sin() as f32
}

/// `Math.cosFromSin(sin, angle)` 的非快速路径。
///
/// The non-fast path of `Math.cosFromSin(sin, angle)`.
fn cos_from_sin(sin: f32, angle: f32) -> f32 {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    let cos = f64::from(1.0 - sin * sin).sqrt() as f32;
    let a = angle + FRAC_PI_2;
    let mut b = a - ((a / TAU) as i32) as f32 * TAU;
    if f64::from(b) < 0.0 {
        b += TAU;
    }
    if b >= PI { -cos } else { cos }
}

/// `Math.absEqualsOne`。
const fn abs_equals_one(value: f32) -> bool {
    value.to_bits() & 0x7fff_ffff == 0x3f80_0000
}

/// `Vector3f.rotateAxis(angle, aX, aY, aZ)`：轴正好是坐标轴时走 `rotateX/Y/Z`，
/// 否则走 `rotateAxisInternal` 的四元数展开式。
///
/// `Vector3f.rotateAxis(angle, aX, aY, aZ)`: `rotateX/Y/Z` for an exact coordinate axis,
/// otherwise the quaternion expansion of `rotateAxisInternal`.
fn rotate_axis([x, y, z]: [f32; 3], angle: f32, [ax, ay, az]: [f32; 3]) -> [f32; 3] {
    if ay == 0.0 && az == 0.0 && abs_equals_one(ax) {
        let sin = sin(ax * angle);
        let cos = cos_from_sin(sin, ax * angle);
        return [x, y * cos - z * sin, y * sin + z * cos];
    }
    if ax == 0.0 && az == 0.0 && abs_equals_one(ay) {
        let sin = sin(ay * angle);
        let cos = cos_from_sin(sin, ay * angle);
        return [x * cos + z * sin, y, -x * sin + z * cos];
    }
    if ax == 0.0 && ay == 0.0 && abs_equals_one(az) {
        let sin = sin(az * angle);
        let cos = cos_from_sin(sin, az * angle);
        return [x * cos - y * sin, x * sin + y * cos, z];
    }
    let half = angle * 0.5;
    let sin_angle = sin(half);
    let (qx, qy, qz) = (ax * sin_angle, ay * sin_angle, az * sin_angle);
    let qw = cos_from_sin(sin_angle, half);
    let (w2, x2, y2, z2) = (qw * qw, qx * qx, qy * qy, qz * qz);
    let (zw, xy, xz, yw, yz, xw) = (qz * qw, qx * qy, qx * qz, qy * qw, qy * qz, qx * qw);
    [
        (w2 + x2 - z2 - y2) * x + (-zw + xy - zw + xy) * y + (yw + xz + xz + yw) * z,
        (xy + zw + zw + xy) * x + (y2 - z2 + w2 - x2) * y + (yz + yz - xw - xw) * z,
        (xz - yw + xz - yw) * x + (yz + yz + xw + xw) * y + (z2 - y2 - x2 + w2) * z,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::mob::MobType;

    fn pillager_with_crossbow() -> Mob {
        let mut mob = Mob::new(MobType::Pillager);
        mob.equipment
            .set(EquipmentSlot::MainHand, ItemStack::new(items::CROSSBOW, 1));
        mob
    }

    /// 装填时长是 25 刻，快速装填每级少 5 刻。
    ///
    /// Loading takes 25 ticks, 5 fewer per Quick Charge level.
    #[test]
    fn charge_duration_follows_quick_charge() {
        assert_eq!(charge_duration(&ItemStack::new(items::CROSSBOW, 1)), 25);
        assert_eq!(charge_duration(&ItemStack::empty()), 25);
    }

    /// 没有偏转角时射击向量就是归一化后的方向。
    ///
    /// Without an angle the shot vector is the normalized direction.
    #[test]
    fn zero_angle_keeps_the_normalized_direction() {
        let original = [3.0_f32, 1.5, -4.0];
        let shot = projectile_shot_vector(original, [0.0, 1.0, 0.0], 0.0);
        assert_eq!(shot, normalize(original));
    }

    /// 多重射击的两支偏转箭在水平面上左右各偏 10 度。
    ///
    /// The two Multishot side arrows turn 10 degrees left and right in the horizontal plane.
    #[test]
    fn angled_shots_turn_about_the_vertical_axis() {
        let left = projectile_shot_vector([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 10.0);
        let right = projectile_shot_vector([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], -10.0);
        let degrees = |v: [f32; 3]| f64::from(v[2]).atan2(f64::from(v[0])).to_degrees();
        assert!((degrees(left).abs() - 10.0).abs() < 1.0e-3);
        assert!((degrees(left) + degrees(right)).abs() < 1.0e-3);
        assert!(left[1].abs() < 1.0e-6 && right[1].abs() < 1.0e-6);
    }

    /// 装箭：没拿箭的怪物凭空得到一支普通箭，装进 `charged_projectiles`。
    ///
    /// Loading: a monster without arrows conjures one plain arrow into `charged_projectiles`.
    #[test]
    fn loading_conjures_one_arrow() {
        let mut mob = pillager_with_crossbow();
        assert!(!is_charged(mob.equipment.get(EquipmentSlot::MainHand)));
        assert!(load_projectiles(&mut mob, EquipmentSlot::MainHand));
        let charged = charged_projectiles(mob.equipment.get(EquipmentSlot::MainHand));
        assert_eq!(charged.len(), 1);
        assert_eq!((charged[0].item, charged[0].count), (items::ARROW, 1));
    }

    /// 开始和停止使用只动「正在用」那一位和剩余刻数。
    ///
    /// Starting and stopping only touch the using bit and the remaining ticks.
    #[test]
    fn use_state_tracks_the_flags() {
        let mut mob = pillager_with_crossbow();
        start_using(&mut mob, EquipmentSlot::MainHand);
        assert!(is_using(&mob));
        assert_eq!(ticks_using(&mob), 0);
        mob.crossbow.remaining -= 7;
        assert_eq!(ticks_using(&mob), 7);
        stop_using(&mut mob);
        assert!(!is_using(&mob));
        assert_eq!(ticks_using(&mob), 0);
    }
}
