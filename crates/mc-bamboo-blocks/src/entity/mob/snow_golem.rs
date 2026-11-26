//! 雪傀儡：官方 `AbstractGolem` → `SnowGolem`，实现 `RangedAttackMob` 和 `Shearable`。
//!
//! 每刻在脚下四角铺一层雪（`mobGriefing`、这一格能不能放雪都按官方）；热的群系由
//! `SNOW_GOLEM_MELTS` 环境属性判，水/雨里受伤是 `LivingEntity.aiStep` 尾部
//! `isSensitiveToWater` 的通用规则。远程目标每 20 刻扔一颗雪球。
//!
//! Snow golems: vanilla `AbstractGolem` → `SnowGolem`, implementing `RangedAttackMob`
//! and `Shearable`. Each tick they may place a snow layer at four nearby cells;
//! hot biomes melt them through `SNOW_GOLEM_MELTS`, and water/rain damage comes
//! from `LivingEntity.aiStep`'s `isSensitiveToWater` rule.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{
    entity_types, items,
    registries::{attribute, entity_type},
    tags::block_tags,
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::item::ItemStack;

use super::{
    DamageKind, DamageSource, Mob, MobEvent,
    attributes::Attributes,
    goals::{CombatTarget, Goal, GoalSelector, TargetType, WrappedGoal, reduced_tick_delay},
    interact::{MobInteractRequest, MobInteraction},
    living,
    navigation::Navigation,
};
use crate::{
    InteractionResult,
    entity::{Entity, ctx::EntityCtx},
};

/// `SnowGolem.DATA_PUMPKIN_ID` 的第 4 位：戴着南瓜。
const PUMPKIN_BIT: u8 = 16;

/// `RangedAttackGoal(this, 1.25, 20, 10.0F)` 的射程。
const ATTACK_RADIUS: f64 = 10.0;

/// 雪傀儡自己的同步状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnowGolem {
    /// `DATA_PUMPKIN_ID`：默认 16，剪掉后为 0。
    pub flags: u8,
}

impl Default for SnowGolem {
    fn default() -> Self {
        Self { flags: PUMPKIN_BIT }
    }
}

impl SnowGolem {
    pub const fn has_pumpkin(self) -> bool {
        self.flags & PUMPKIN_BIT != 0
    }

    pub fn set_pumpkin(&mut self, value: bool) {
        if value {
            self.flags |= PUMPKIN_BIT;
        } else {
            self.flags &= !PUMPKIN_BIT;
        }
    }
}

/// `EntityTypes.SNOW_GOLEM`：尺寸取生成器导出的表，眼高单独导出。
pub fn dimensions() -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[entity_type::SNOW_GOLEM as usize];
    (
        facts.width,
        facts.height,
        entity_types::SNOW_GOLEM_EYE_HEIGHT,
    )
}

/// `SnowGolem.createAttributes()`：`Mob.createMobAttributes` 加 4 血、0.2 速度。
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    table.set_base(attribute::MAX_HEALTH, entity_types::SNOW_GOLEM_MAX_HEALTH);
    table.set_base(
        attribute::MOVEMENT_SPEED,
        entity_types::SNOW_GOLEM_MOVEMENT_SPEED,
    );
    table
        .entries
        .retain(|entry| entry.id != attribute::TEMPT_RANGE);
    table
}

/// `SnowGolem.registerGoals`：远程攻击、避水闲逛、看玩家、随便看看。
pub(super) fn goals(navigation: &mut Navigation) -> GoalSelector {
    navigation.flags.can_float = true;
    let wrap = |priority, goal| WrappedGoal {
        priority,
        running: false,
        goal,
    };
    GoalSelector::from_goals(vec![
        wrap(
            1,
            Goal::SnowGolemRangedAttack {
                target: None,
                attack_time: -1,
                see_time: 0,
            },
        ),
        wrap(
            2,
            Goal::Stroll {
                speed: 1.0,
                interval: 120,
                probability: 1.000_000_1e-5,
                wanted: Vec3::default(),
                force_trigger: false,
            },
        ),
        wrap(
            3,
            Goal::LookAtPlayer {
                look_distance: 6.0,
                probability: 0.02,
                look_at: None,
                look_time: 0,
            },
        ),
        wrap(
            4,
            Goal::RandomLookAround {
                rel_x: 0.0,
                rel_z: 0.0,
                look_time: 0,
            },
        ),
    ])
}

/// `SnowGolem` 的 target 选择器：`NearestAttackableTargetGoal<Mob>(10, true, false, Enemy)`。
pub(super) fn target_goals() -> GoalSelector {
    GoalSelector::from_ordered_pairs(vec![(
        1,
        Goal::NearestAttackableTarget {
            kind: TargetType::SnowGolemPrey,
            interval: reduced_tick_delay(10),
            unseen_ticks: 0,
            found: None,
            must_see: true,
            light_gate: false,
        },
    )])
}

/// `SnowGolem.aiStep`：热环境扣血 + `mobGriefing` 开时脚下铺雪。
pub(super) fn ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let blocks = ctx.blocks();
    if blocks.snow_golem_melts(me.block_position()) {
        let _ = living::hurt_server(
            me,
            mob,
            ctx,
            DamageSource::environment(DamageKind::Drown),
            1.0,
        );
    }
    if !ctx.blocks().block_game_rules().mob_griefing {
        return;
    }
    let mut placed = Vec::new();
    for i in 0..4_i32 {
        let x = me.position.x + f64::from((i % 2 * 2 - 1) as f32 * 0.25_f32);
        let y = me.position.y;
        let z = me.position.z + f64::from(((i / 2) % 2 * 2 - 1) as f32 * 0.25_f32);
        let pos = BlockPos::containing(Vec3::new(x, y, z));
        if ctx.blocks().block_state(pos).is_air() && ctx.blocks().snow_layer_can_survive(pos) {
            placed.push(pos);
        }
    }
    if !placed.is_empty() {
        ctx.emit(
            crate::entity::ctx::Command::PlaceSnowTrail { cells: placed },
            me,
        );
    }
}

/// `SnowGolem.performRangedAttack`：抛一颗 `Snowball`，音高取实体随机源一次 `nextFloat`。
pub(super) fn perform_ranged_attack(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    target: CombatTarget,
) {
    let dx = target.position.x - me.position.x;
    let dy = target.eye_y() - f64::from(1.1_f32) - me.position.y;
    let dz = target.position.z - me.position.z;
    let gravity = (dx * dx + dz * dz).sqrt() * entity_types::SNOW_GOLEM_MOVEMENT_SPEED;
    let shot = crate::entity::thrown_egg::EggShot {
        position: Vec3::new(
            me.position.x,
            living::eye_y(me, mob) - f64::from(0.1_f32),
            me.position.z,
        ),
        direction: Vec3::new(dx, dy + gravity, dz),
        power: 1.6,
        uncertainty: 12.0,
        source_movement: None,
        owner: Some(me.id),
        owner_uuid: Some(mob.uuid),
        item: ItemStack::new(items::SNOWBALL, 1),
        projectile: crate::entity::thrown_egg::ThrowableKind::Snowball,
    };
    ctx.emit(
        crate::entity::ctx::Command::ThrowThrowable {
            shot: Box::new(shot),
        },
        me,
    );
    if !mob.silent {
        let roll = me.random.next_float();
        let pitch = 1.0 / (0.8 + roll * 0.4);
        living::play_sound(me, mob, "minecraft:entity.snow_golem.shoot", 1.0, pitch);
    }
}

/// `SnowGolem.mobInteract`：剪刀剪掉南瓜，音效和掉落交给调用方落地。
pub(super) fn interact(mob: &Mob, req: &MobInteractRequest) -> MobInteraction {
    if req.held.item != items::SHEARS || !mob.snow_golem.has_pumpkin() {
        return MobInteraction::pass();
    }
    MobInteraction {
        result: InteractionResult::Success,
        shear: true,
        damage_held: true,
        ..MobInteraction::pass()
    }
}

/// `SnowGolem.shear` + `hurtAndBreak(1)`：关南瓜、播玩家音源、按战利品表掉南瓜。
pub(crate) fn apply_shear(
    world: &mut crate::world::World,
    entity: &mut Entity,
    hand: mc_bamboo_core::Hand,
    tool: &ItemStack,
) {
    let Some(request) = entity.with_mob(|me, mob| {
        mob.events.push(MobEvent::PlayerSound {
            sound: "minecraft:entity.snow_golem.shear",
            position: me.position,
        });
        super::EntityLootRequest::new("minecraft:shearing/snow_golem", me, mob, Some(tool.clone()))
    }) else {
        return;
    };
    let height = f64::from(entity.dimensions.1);
    let at = request.origin.add(Vec3::new(0.0, height, 0.0));
    for drop in world.entity_loot_drops(request) {
        world.spawn_at_location(at, drop);
    }
    entity.with_mob(|_, mob| mob.snow_golem.set_pumpkin(false));
    crate::durability::hurt_hand(world, hand, 1);
}

/// `SnowGolem.addAdditionalSaveData` / `readAdditionalSaveData`。
pub(super) fn write_nbt(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    out.push((
        "Pumpkin".to_owned(),
        Tag::Byte(i8::from(mob.snow_golem.has_pumpkin())),
    ));
}

pub(super) fn read_nbt(mob: &mut Mob, tag: &Tag) {
    let pumpkin = crate::archive::tag::bool_or(tag, "Pumpkin", true);
    mob.snow_golem.set_pumpkin(pumpkin);
}

/// `SnowGolem.rangedAttackUncertainty` 和攻击半径，给远程目标用。
pub(super) const fn attack_radius() -> f64 {
    ATTACK_RADIUS
}

/// `SNOW_GOLEM_IMMUNE_TO` 标签：热群系靠环境属性判断，这里保留标签常量给方块层。
pub const MELT_IMMUNE_BLOCKS: mc_bamboo_data::BlockTag = block_tags::SNOW_GOLEM_IMMUNE_TO;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::mob::MobType;

    #[test]
    fn attributes_follow_the_generated_facts() {
        let table = attributes();
        assert_eq!(
            table.value(attribute::MAX_HEALTH),
            entity_types::SNOW_GOLEM_MAX_HEALTH
        );
        assert_eq!(
            table.value(attribute::MOVEMENT_SPEED),
            entity_types::SNOW_GOLEM_MOVEMENT_SPEED
        );
        assert!(table.get(attribute::TEMPT_RANGE).is_none());
        assert!(table.get(attribute::ATTACK_DAMAGE).is_none());
    }

    #[test]
    fn pumpkin_defaults_on_and_shears_it_off() {
        let mut mob = Mob::new(MobType::SnowGolem);
        assert!(mob.snow_golem.has_pumpkin());
        let req = MobInteractRequest {
            player_uid: 1,
            player_uuid: [1; 16],
            hand: mc_bamboo_core::Hand::Main,
            held: ItemStack::new(items::SHEARS, 1),
            creative: false,
            player_height: 1.8,
            sneaking: false,
            difficulty: 2,
            hit_pos: None,
            position: Vec3::new(0.0, 64.0, 0.0),
        };
        let outcome = interact(&mob, &req);
        assert_eq!(outcome.result, InteractionResult::Success);
        mob.snow_golem.set_pumpkin(false);
        assert_eq!(interact(&mob, &req), MobInteraction::pass());
    }

    #[test]
    fn four_trail_cells_are_the_official_offsets() {
        let offsets: Vec<_> = (0..4_i32)
            .map(|i| {
                (
                    (i % 2 * 2 - 1) as f32 * 0.25,
                    ((i / 2) % 2 * 2 - 1) as f32 * 0.25,
                )
            })
            .collect();
        assert_eq!(
            offsets,
            vec![(-0.25, -0.25), (0.25, -0.25), (-0.25, 0.25), (0.25, 0.25)]
        );
    }
}
