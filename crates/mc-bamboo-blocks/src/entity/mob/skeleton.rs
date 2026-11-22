use mc_bamboo_core::{Vec3, mth};
use mc_bamboo_data::{items, registries::attribute};
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{Mob, MobType, control, goals::CombatTarget, living};
use crate::{
    World,
    entity::{
        Entity, EntityKind, EntityUid,
        ctx::{BlockView, Command, EntityCtx},
    },
};

pub(crate) fn bow_slot(mob: &Mob) -> Option<EquipmentSlot> {
    [EquipmentSlot::MainHand, EquipmentSlot::OffHand]
        .into_iter()
        .find(|&slot| mob.equipment.get(slot).item == items::BOW)
}

#[derive(Debug, Clone, PartialEq)]
pub struct BowGoal {
    interval: i32,
    attack_time: i32,
    see_time: i32,
    strafing_time: i32,
    strafing_clockwise: bool,
    strafing_backwards: bool,
    draw_start: Option<i32>,
}

impl BowGoal {
    #[cfg(test)]
    pub(crate) fn new(difficulty: i32) -> Self {
        Self::for_kind(MobType::Skeleton, difficulty)
    }

    /// `AbstractSkeleton.reassessWeaponGoal`：困难用 `getHardAttackInterval()`，其余用
    /// `getAttackInterval()`；沼骸把两者覆写成 50 / 70。
    ///
    /// `AbstractSkeleton.reassessWeaponGoal`: `getHardAttackInterval()` on hard and
    /// `getAttackInterval()` otherwise; bogged override them to 50 / 70.
    pub(crate) fn for_kind(kind: MobType, difficulty: i32) -> Self {
        let (hard, normal) = match kind {
            MobType::Bogged => (
                super::bogged::HARD_ATTACK_INTERVAL,
                super::bogged::ATTACK_INTERVAL,
            ),
            _ => (20, 40),
        };
        Self {
            interval: if difficulty == 3 { hard } else { normal },
            attack_time: -1,
            see_time: 0,
            strafing_time: -1,
            strafing_clockwise: false,
            strafing_backwards: false,
            draw_start: None,
        }
    }

    pub(crate) fn stop(&mut self, mob: &mut Mob) {
        mob.aggressive = false;
        self.see_time = 0;
        self.attack_time = -1;
        self.stop_using(mob);
    }

    fn stop_using(&mut self, mob: &mut Mob) {
        self.draw_start = None;
        mob.using_item_flags = 0;
    }

    /// RangedBowAttackGoal.tick：先更新视线与移动，再推进拉弓。
    ///
    /// RangedBowAttackGoal.tick updates visibility and movement before the draw.
    pub(crate) fn tick(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        ctx: &mut dyn EntityCtx,
        target: CombatTarget,
    ) {
        let Some(slot) = bow_slot(mob) else { return };
        let visible =
            living::has_line_of_sight(me, mob, ctx.blocks(), target.uid, target.eye_position());
        if visible != (self.see_time > 0) {
            self.see_time = 0;
        }
        self.see_time += if visible { 1 } else { -1 };
        self.move_and_look(me, mob, ctx.blocks(), target);

        if let Some(start) = self.draw_start {
            if !visible && self.see_time < -60 {
                self.stop_using(mob);
            } else if visible && me.tick_count - start >= 20 {
                self.stop_using(mob);
                shoot(me, mob, ctx, target, slot);
                self.attack_time = self.interval;
            }
        } else {
            self.attack_time -= 1;
            if self.attack_time <= 0 && self.see_time >= -60 {
                self.draw_start = Some(me.tick_count);
                mob.using_item_flags = if slot == EquipmentSlot::OffHand { 3 } else { 1 };
            }
        }
    }

    fn move_and_look(
        &mut self,
        me: &mut Entity,
        mob: &mut Mob,
        view: &dyn BlockView,
        target: CombatTarget,
    ) {
        const RADIUS_SQR: f64 = 15.0 * 15.0;
        let distance = target.position.distance_to_sqr(me.position);
        if distance <= RADIUS_SQR && self.see_time >= 20 {
            mob.navigation.stop();
            self.strafing_time += 1;
        } else {
            let nav = living::nav_mob(me, mob, view);
            mob.navigation
                .move_to_entity(view, &nav, target.position, 1.0, 1);
            self.strafing_time = -1;
        }
        if self.strafing_time >= 20 {
            if me.random.next_float() < 0.3 {
                self.strafing_clockwise = !self.strafing_clockwise;
            }
            if me.random.next_float() < 0.3 {
                self.strafing_backwards = !self.strafing_backwards;
            }
            self.strafing_time = 0;
        }
        if self.strafing_time >= 0 {
            if distance > RADIUS_SQR * 0.75 {
                self.strafing_backwards = false;
            } else if distance < RADIUS_SQR * 0.25 {
                self.strafing_backwards = true;
            }
            mob.mover.strafe(
                if self.strafing_backwards { -0.5 } else { 0.5 },
                if self.strafing_clockwise { 0.5 } else { -0.5 },
            );
            look_at_now(me, mob, target.eye_position());
        } else {
            mob.look.set_look_at(
                target.position.x,
                target.eye_y(),
                target.position.z,
                30.0,
                30.0,
            );
        }
    }
}

fn look_at_now(me: &mut Entity, mob: &mut Mob, eye: Vec3) {
    let dx = eye.x - me.position.x;
    let dy = eye.y - (me.position.y + f64::from(mob.eye_height));
    let dz = eye.z - me.position.z;
    let horizontal = (dx * dx + dz * dz).sqrt();
    let degrees = |rad: f64| rad * f64::from(180.0_f32 / std::f32::consts::PI);
    let yaw = degrees(mth::atan2(dz, dx)) as f32 - 90.0;
    let pitch = -degrees(mth::atan2(dy, horizontal)) as f32;
    me.y_rot = control::rotate_towards(me.y_rot, yaw, 30.0);
    mob.x_rot = control::rotate_towards(mob.x_rot, pitch, 30.0);
}

fn shoot(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    target: CombatTarget,
    slot: EquipmentSlot,
) {
    let position = Vec3::new(
        me.position.x,
        me.position.y + f64::from(mob.eye_height) - f64::from(0.1_f32),
        me.position.z,
    );
    let relative = target.position.subtract(position);
    let distance = (relative.x * relative.x + relative.z * relative.z).sqrt();
    let direction = Vec3::new(
        relative.x,
        (target.position.y + target.height * 0.333_333_333_333_333_3 - position.y)
            + distance * f64::from(0.2_f32),
        relative.z,
    );
    let difficulty = ctx.blocks().difficulty();
    ctx.emit(
        Command::ShootMobArrow {
            position,
            direction,
            owner_uuid: mob.uuid,
            weapon: mob.equipment.get(slot).clone(),
            difficulty,
            effect: arrow_effect(mob.mob_type),
        },
        me,
    );
    let pitch = 1.0 / (me.random.next_float() * 0.4 + 0.8);
    living::play_sound(me, mob, "minecraft:entity.skeleton.shoot", 1.0, pitch);
}

/// `Stray.getArrow` / `Bogged.getArrow` 给箭加的效果：（效果 ID，时长），等级 0。
///
/// The effect `Stray.getArrow` / `Bogged.getArrow` add to the arrow: (effect id,
/// duration) at amplifier 0.
const fn arrow_effect(kind: MobType) -> Option<(i32, i32)> {
    match kind {
        MobType::Stray => Some(super::stray::ARROW_EFFECT),
        MobType::Bogged => Some(super::bogged::ARROW_EFFECT),
        _ => None,
    }
}

/// `Arrow.addEffect`：`getPotionContents().withEffectAdded(effect)` 写回拾取物品的
/// `potion_contents`，效果用 `MobEffectInstance(effect, duration)` 的缺省值（等级 0、
/// 显示粒子和图标）。箭的颜色从这个组件算。
///
/// `Arrow.addEffect`: `getPotionContents().withEffectAdded(effect)` written back to the
/// pickup item's `potion_contents`, with `MobEffectInstance(effect, duration)` defaults
/// (amplifier 0, particles and icon shown). The arrow colour derives from the component.
fn add_arrow_effect(pickup_item: &mut ItemStack, effect: i32, duration: i32) {
    use mc_bamboo_data::registries::data_component_type::POTION_CONTENTS;
    use mc_bamboo_protocol::components::{
        ComponentValue, PatchEntry, defaults,
        value::{EffectDetails, MobEffectInstance},
    };
    let mut patch = pickup_item.patch();
    let (potion, custom_color, mut custom_effects, custom_name) =
        match defaults::resolve(pickup_item.item, &patch, POTION_CONTENTS) {
            Some(ComponentValue::PotionContents {
                potion,
                custom_color,
                custom_effects,
                custom_name,
            }) => (
                *potion,
                *custom_color,
                custom_effects.clone(),
                custom_name.clone(),
            ),
            _ => (None, None, Vec::new(), None),
        };
    custom_effects.push(MobEffectInstance {
        effect,
        details: EffectDetails {
            amplifier: 0,
            duration,
            ambient: false,
            show_particles: true,
            show_icon: true,
            hidden_effect: None,
        },
    });
    patch.put(
        POTION_CONTENTS,
        PatchEntry::Set(ComponentValue::PotionContents {
            potion,
            custom_color,
            custom_effects,
            custom_name,
        }),
    );
    (pickup_item.added, pickup_item.removed) = patch.to_opaque();
}

/// [`Command::ShootMobArrow`] 带的一次射击。
///
/// One shot carried by [`Command::ShootMobArrow`].
pub(crate) struct MobArrowShot {
    pub position: Vec3,
    pub direction: Vec3,
    pub owner_uuid: [u8; 16],
    pub weapon: ItemStack,
    pub difficulty: i32,
    pub effect: Option<(i32, i32)>,
}

/// ProjectileUtil.getMobArrow：箭用自身随机源算伤害和散布，不用射手的随机源。
///
/// ProjectileUtil.getMobArrow uses the newly constructed arrow's RNG for damage and spread.
pub(crate) fn spawn_arrow(world: &mut World<'_>, owner: EntityUid, shot: MobArrowShot) {
    let MobArrowShot {
        position,
        direction,
        owner_uuid,
        weapon,
        difficulty,
        effect,
    } = shot;
    let mut entity = world.prepare_entity(Entity::arrow(position));
    let EntityKind::Arrow(arrow) = &mut entity.kind else {
        unreachable!()
    };
    arrow.owner = Some(owner);
    arrow.owner_uuid = Some(owner_uuid);
    arrow.pierce = crate::entity::arrow::enchantment_level(&weapon, "minecraft:piercing") as i8;
    if crate::entity::arrow::enchantment_level(&weapon, "minecraft:flame") > 0 {
        entity.remaining_fire_ticks = 2000;
    }
    arrow.weapon = Some(weapon);
    if let Some((effect, duration)) = effect {
        add_arrow_effect(&mut arrow.pickup_item, effect, duration);
    }
    arrow.base_damage = 2.0
        + entity
            .random
            .triangle(f64::from(difficulty) * 0.11, 0.57425);
    entity.shoot_arrow(direction, 1.6, (14 - 4 * difficulty) as f32);
    world.add_prepared_entity(entity);
}

pub(crate) fn strafe(me: &Entity, mob: &mut Mob, view: &dyn BlockView) {
    use super::path::static_path_type;
    use mc_bamboo_data::PathType;
    let control = &mut mob.mover;
    let speed =
        control.speed_modifier as f32 * mob.attributes.value(attribute::MOVEMENT_SPEED) as f32;
    let mut forward = control.strafe_forwards;
    let mut right = control.strafe_right;
    let scale = speed / (forward * forward + right * right).sqrt().max(1.0);
    let (dx, dz) = control::strafe_projection(me.y_rot, forward * scale, right * scale);
    if static_path_type(
        view,
        control::walkable_probe(me.position.x, me.block_position().y, me.position.z, dx, dz),
    ) != PathType::Walkable
    {
        forward = 1.0;
        right = 0.0;
        control.strafe_forwards = forward;
        control.strafe_right = right;
    }
    mob.speed = speed;
    mob.zza = forward;
    mob.xxa = right;
    control.operation = super::control::Operation::Wait;
}

pub(super) fn finalize_spawn(world: &mut World<'_>, entity: &mut Entity) {
    let difficulty = BlockView::difficulty(world);
    let special = ((BlockView::effective_difficulty(world, entity.block_position()) - 2.0) / 2.0)
        .clamp(0.0, 1.0);
    let random = world.random();
    entity.with_mob(|_, mob| {
        if let Some(range) = mob.attributes.instance(attribute::FOLLOW_RANGE)
            && range.modifier(super::spawn::RANDOM_SPAWN_BONUS).is_none()
        {
            range.modifiers.push(super::attributes::Modifier {
                id: super::spawn::RANDOM_SPAWN_BONUS.to_owned(),
                amount: random.triangle(0.0, 0.114_850_000_000_000_01),
                operation: super::attributes::Operation::AddMultipliedBase,
                permanent: true,
            });
        }
        mob.left_handed = random.next_float() < 0.05;
        populate_armor(mob, random, difficulty, special);
        mob.equipment
            .set(EquipmentSlot::MainHand, ItemStack::new(items::BOW, 1));
        enchant_spawn_equipment(world, mob, random, special);
        mob.can_pick_up_loot = random.next_float() < 0.55 * special;
        if mob.equipment.get(EquipmentSlot::Head).is_empty() && world.level().is_halloween() {
            equip_halloween_head(mob, random);
        }
    });
}

/// `Mob.populateDefaultEquipmentEnchantments`：主手 0.25，再按 `EquipmentSlot.VALUES` 的
/// 脚、腿、胸、头各 0.5，乘特殊倍率；只给非空的槽取数。
///
/// `Mob.populateDefaultEquipmentEnchantments`: main hand at 0.25, then feet, legs, chest
/// and head at 0.5 in `EquipmentSlot.VALUES` order, scaled by the special multiplier;
/// empty slots draw nothing.
pub(super) fn enchant_spawn_equipment(
    world: &World<'_>,
    mob: &mut Mob,
    random: &crate::random::LegacyRandom,
    special: f32,
) {
    for (slot, chance) in [
        (EquipmentSlot::MainHand, 0.25),
        (EquipmentSlot::Feet, 0.5),
        (EquipmentSlot::Legs, 0.5),
        (EquipmentSlot::Chest, 0.5),
        (EquipmentSlot::Head, 0.5),
    ] {
        let stack = mob.equipment.get(slot);
        if !stack.is_empty() && random.next_float() < chance * special {
            let mut stack = stack.clone();
            world
                .level()
                .enchant_mob_spawn_equipment(&mut stack, special);
            mob.equipment.set(slot, stack);
        }
    }
}

pub(super) fn equip_halloween_head(mob: &mut Mob, random: &crate::random::LegacyRandom) {
    if random.next_float() < 0.25 {
        let item = if random.next_float() < 0.1 {
            items::JACK_O_LANTERN
        } else {
            items::CARVED_PUMPKIN
        };
        mob.equipment
            .set(EquipmentSlot::Head, ItemStack::new(item, 1));
        mob.equipment.set_drop_chance(EquipmentSlot::Head, 0.0);
    }
}

pub(super) fn populate_armor(
    mob: &mut Mob,
    random: &crate::random::LegacyRandom,
    difficulty: i32,
    special: f32,
) {
    use EquipmentSlot::{Chest, Feet, Head, Legs};
    if random.next_float() >= 0.15 * special {
        return;
    }
    let mut material = random.next_int(3) as usize;
    for _ in 0..3 {
        if random.next_float() < 0.1087 {
            material += 1;
        }
    }
    let partial = if difficulty == 3 { 0.1 } else { 0.25 };
    let armor = [
        [
            items::LEATHER_HELMET,
            items::LEATHER_CHESTPLATE,
            items::LEATHER_LEGGINGS,
            items::LEATHER_BOOTS,
        ],
        [
            items::COPPER_HELMET,
            items::COPPER_CHESTPLATE,
            items::COPPER_LEGGINGS,
            items::COPPER_BOOTS,
        ],
        [
            items::GOLDEN_HELMET,
            items::GOLDEN_CHESTPLATE,
            items::GOLDEN_LEGGINGS,
            items::GOLDEN_BOOTS,
        ],
        [
            items::CHAINMAIL_HELMET,
            items::CHAINMAIL_CHESTPLATE,
            items::CHAINMAIL_LEGGINGS,
            items::CHAINMAIL_BOOTS,
        ],
        [
            items::IRON_HELMET,
            items::IRON_CHESTPLATE,
            items::IRON_LEGGINGS,
            items::IRON_BOOTS,
        ],
        [
            items::DIAMOND_HELMET,
            items::DIAMOND_CHESTPLATE,
            items::DIAMOND_LEGGINGS,
            items::DIAMOND_BOOTS,
        ],
    ];
    for (index, (slot, item)) in [Head, Chest, Legs, Feet]
        .into_iter()
        .zip(armor[material])
        .enumerate()
    {
        if index != 0 && random.next_float() < partial {
            break;
        }
        if mob.equipment.get(slot).is_empty() {
            mob.equipment.set(slot, ItemStack::new(item, 1));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn halloween_head_draws_the_variant_only_after_a_successful_roll() {
        use super::*;
        use crate::entity::mob::MobType;
        use crate::random::LegacyRandom;
        for (seed, item, draws) in [
            (0, items::AIR, 1),
            (4096, items::CARVED_PUMPKIN, 2),
            (4104, items::JACK_O_LANTERN, 2),
        ] {
            let mut mob = Mob::new(MobType::Skeleton);
            let random = LegacyRandom::new(seed);
            equip_halloween_head(&mut mob, &random);
            assert_eq!(mob.equipment.get(EquipmentSlot::Head).item, item);
            assert_eq!(
                mob.equipment.drop_chance(EquipmentSlot::Head),
                if draws == 1 {
                    super::super::equipment::DEFAULT_DROP_CHANCE
                } else {
                    0.0
                }
            );
            let expected = LegacyRandom::new(seed);
            for _ in 0..draws {
                expected.next_float();
            }
            assert_eq!(random.seed(), expected.seed());
        }
    }
    use super::*;
    use crate::{
        BlockTicks,
        entity::ctx::{PlayerView, PreciseCtx},
    };

    #[test]
    fn bogged_shoot_on_the_slower_interval_and_tip_their_arrows() {
        use mc_bamboo_data::registries::{data_component_type::POTION_CONTENTS, mob_effect};
        use mc_bamboo_protocol::components::{ComponentValue, defaults};

        let interval = |kind, difficulty| BowGoal::for_kind(kind, difficulty).interval;
        assert_eq!(interval(MobType::Bogged, 3), 50);
        assert_eq!(interval(MobType::Bogged, 2), 70);
        assert_eq!(interval(MobType::Stray, 3), 20);
        assert_eq!(interval(MobType::Stray, 1), 40);
        assert_eq!(arrow_effect(MobType::Skeleton), None);
        for (kind, effect, duration) in [
            (MobType::Stray, mob_effect::SLOWNESS, 600),
            (MobType::Bogged, mob_effect::POISON, 100),
        ] {
            let mut stack = ItemStack::new(items::ARROW, 1);
            let (id, ticks) = arrow_effect(kind).unwrap();
            add_arrow_effect(&mut stack, id, ticks);
            let patch = stack.patch();
            let Some(ComponentValue::PotionContents {
                potion,
                custom_effects,
                ..
            }) = defaults::resolve(stack.item, &patch, POTION_CONTENTS)
            else {
                panic!("the arrow has no potion contents");
            };
            assert_eq!(*potion, None);
            assert_eq!(custom_effects.len(), 1);
            let details = &custom_effects[0].details;
            assert_eq!(
                (
                    custom_effects[0].effect,
                    details.duration,
                    details.amplifier
                ),
                (effect, duration, 0)
            );
            assert!(details.show_particles && details.show_icon && !details.ambient);
        }
    }
    #[test]
    fn draw_takes_twenty_ticks_in_addition_to_difficulty_cooldown() {
        assert_eq!(BowGoal::new(3).interval, 20);
        assert_eq!(BowGoal::new(2).interval, 40);
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let mut shooter =
            Entity::by_mob_type(super::super::MobType::Skeleton, Vec3::new(0.5, 64.0, 0.5));
        let player = PlayerView {
            uid: 123,
            yaw: 0.0,
            pitch: 0.0,
            uuid: None,
            position: Vec3::new(10.5, 64.0, 0.5),
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
            invisible: false,
            armor_cover: 0.0,
            main_hand: 0,
            off_hand: 0,
            combat: Default::default(),
            effects: Default::default(),
        };
        shooter.with_mob(|shooter, mob| {
            mob.equipment
                .set(EquipmentSlot::MainHand, ItemStack::new(items::BOW, 1));
            let mut goal = BowGoal::new(2);
            let mut previous = 0;
            let mut shots = Vec::new();
            for tick in 1..=100 {
                shooter.tick_count = tick;
                mob.seen.clear();
                let mut ctx = PreciseCtx::new(&mut world, shooter.id);
                goal.tick(shooter, mob, &mut ctx, CombatTarget::player(player, true));
                let count = world.entity_ids().len();
                if count > previous {
                    shots.push(tick);
                    previous = count;
                }
            }
            assert_eq!(shots, vec![21, 81]);
        });
    }

    #[test]
    fn skeleton_command_and_egg_equip_the_bow_without_changing_nbt_summon() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        use super::super::{
            MobType,
            spawn::{spawn_egg, summon_mob},
        };
        use mc_bamboo_core::BlockPos;

        let position = Vec3::new(0.5, 64.0, 0.5);
        let summon = summon_mob(&mut world, MobType::Skeleton, position, None).unwrap();
        let egg = spawn_egg(
            &mut world,
            MobType::Skeleton,
            BlockPos::new(1, 64, 0),
            false,
            None,
        );
        let tag = mc_bamboo_nbt::Tag::Compound(Vec::new());
        let nbt = summon_mob(&mut world, MobType::Skeleton, position, Some(&tag)).unwrap();
        for uid in [summon, egg] {
            let entity = world.entity(uid).unwrap();
            let mob = entity.kind.mob().unwrap();
            assert_eq!(bow_slot(mob), Some(EquipmentSlot::MainHand));
            assert_eq!(mob.health, 20.0);
            assert_eq!(mob.attributes.value(attribute::MOVEMENT_SPEED), 0.25);
        }
        let entity = world.entity(nbt).unwrap();
        assert!(bow_slot(entity.kind.mob().unwrap()).is_none());
    }
}
