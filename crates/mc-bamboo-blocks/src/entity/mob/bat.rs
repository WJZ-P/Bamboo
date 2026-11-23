//! 蝙蝠：官方 `Bat`（`AmbientCreature` → `Mob`）。
//!
//! 蝙蝠没有目标也不寻路：`customServerAiStep` 里要么倒挂着等玩家靠近，要么朝一个随机
//! 方块飞；`tick` 的尾部把倒挂的蝙蝠钉在方块顶下面，把飞行的竖直速度乘 0.6。不推人、
//! 不被推，没有摔落伤害，移动不出声。
//!
//! Bats: vanilla `Bat` (`AmbientCreature` → `Mob`). No goals and no path-finding:
//! `customServerAiStep` either hangs waiting for a player or flies at a random block, and
//! the tail of `tick` pins a hanging bat under its block and scales the vertical flying
//! speed by 0.6. Bats neither push nor get pushed, take no fall damage and move silently.

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::{registries::attribute, tags::block_tags};

use super::{DamageSource, Mob, MobType, attributes::Attributes, goals, living};
use crate::{
    entity::{
        Entity,
        ctx::{Command, EntityCtx, PlayerView},
    },
    world::World,
};

/// `Bat.FLAG_RESTING`。
const FLAG_RESTING: u8 = 1;
/// `BAT_RESTING_TARGETING`：`forNonCombat().range(4.0)`。
const RESTING_RANGE: f64 = 4.0;
/// `Mth.RAD_TO_DEG`：`180.0F / (float) Math.PI`。
const RAD_TO_DEG: f32 = 180.0 / std::f32::consts::PI;
/// `LevelEvent.SOUND_BAT_LIFTOFF`。
const SOUND_BAT_LIFTOFF: i32 = 1025;

/// 蝙蝠专属的状态：`DATA_ID_FLAGS` 整个字节（存档原样写回）和飞行目标方块。
///
/// Bat-only state: the whole `DATA_ID_FLAGS` byte (saved verbatim) and the flight target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BatState {
    /// `Bat.DATA_ID_FLAGS`，第 0 位是倒挂。
    ///
    /// `Bat.DATA_ID_FLAGS`; bit 0 is resting.
    pub flags: u8,
    /// `Bat.targetPosition`，不存档。
    ///
    /// `Bat.targetPosition`; not saved.
    pub target: Option<BlockPos>,
}

impl BatState {
    /// `Bat.isResting`。
    pub const fn is_resting(self) -> bool {
        self.flags & FLAG_RESTING != 0
    }

    /// `Bat.setResting`。
    pub const fn set_resting(&mut self, resting: bool) {
        if resting {
            self.flags |= FLAG_RESTING;
        } else {
            self.flags &= !FLAG_RESTING;
        }
    }
}

/// `Bat.createAttributes`：`Mob.createMobAttributes()` 加 6 点血。
///
/// `Bat.createAttributes`: the mob defaults with six health.
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::mob();
    attributes.set_base(attribute::MAX_HEALTH, 6.0);
    attributes
}

/// `Bat.getAmbientSound` 返回 null 的那一支：倒挂时 `nextInt(4) != 0` 就不叫。飞行时
/// 短路，不取随机数。
///
/// The null branch of `Bat.getAmbientSound`: a hanging bat stays quiet unless
/// `nextInt(4)` is 0. A flying bat short-circuits and draws nothing.
pub(super) fn ambient_is_muted(me: &mut Entity, mob: &Mob) -> bool {
    mob.bat.is_resting() && me.random.next_int_bounded(4) != 0
}

/// `Bat.customServerAiStep`。
pub(super) fn custom_server_ai_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    let pos = me.block_position();
    let above = pos.above();
    if mob.bat.is_resting() {
        rest_step(me, mob, ctx, pos, above);
    } else {
        fly_step(me, mob, ctx, above);
    }
}

/// 倒挂的一刻：头上不是导体就起飞；否则 1/200 转一下头，4 格内有看得见的玩家就起飞。
///
/// One hanging tick: take off when the block above stops conducting; otherwise turn the
/// head on a 1-in-200 roll and take off when a visible player is within four blocks.
fn rest_step(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    pos: BlockPos,
    above: BlockPos,
) {
    if ctx.blocks().block_state(above).is_redstone_conductor() {
        if me.random.next_int_bounded(200) == 0 {
            mob.y_head_rot = me.random.next_int_bounded(360) as f32;
        }
        if !player_nearby(me, mob, ctx) {
            return;
        }
    }
    mob.bat.set_resting(false);
    if !mob.silent {
        ctx.emit(
            Command::MobLevelEvent {
                event: SOUND_BAT_LIFTOFF,
                pos,
                data: 0,
            },
            me,
        );
    }
}

/// `level.getNearestPlayer(BAT_RESTING_TARGETING, this) != null`。每个玩家都测一遍，
/// 和官方一样让视线缓存记下所有人。
///
/// `level.getNearestPlayer(BAT_RESTING_TARGETING, this) != null`. Every player is tested,
/// so the sight cache remembers all of them as in vanilla.
fn player_nearby(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) -> bool {
    let view = ctx.blocks();
    let mut players: Vec<PlayerView> = Vec::new();
    view.for_each_player(&mut |player| players.push(*player));
    let mut found = false;
    for player in players {
        found |= goals::targeting_test(me, mob, view, &player, RESTING_RANGE, true);
    }
    found
}

/// 飞行的一刻：目标方块不是空气或落到世界底就丢掉；没有目标、1/30、或离目标中心不到
/// 2 格时重抽；朝目标加速、转身；1/100 且头上是导体时倒挂。
///
/// One flying tick: drop a target that is no longer air or sits at the world bottom;
/// re-roll it when missing, on a 1-in-30 roll, or within two blocks of its centre;
/// accelerate and turn towards it; hang on a 1-in-100 roll under a conductor.
fn fly_step(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, above: BlockPos) {
    let view = ctx.blocks();
    if let Some(target) = mob.bat.target
        && (!view.block_state(target).is_air() || target.y <= view.min_y())
    {
        mob.bat.target = None;
    }
    let reroll = match mob.bat.target {
        None => true,
        Some(target) => {
            me.random.next_int_bounded(30) == 0 || closer_to_center_than(target, me.position, 2.0)
        }
    };
    if reroll {
        mob.bat.target = Some(random_target(me));
    }
    let Some(target) = mob.bat.target else {
        return;
    };
    let dx = f64::from(target.x) + 0.5 - me.position.x;
    let dy = f64::from(target.y) + 0.1 - me.position.y;
    let dz = f64::from(target.z) + 0.5 - me.position.z;
    let movement = me.delta;
    let pull = f64::from(0.1_f32);
    let new_movement = movement.add(Vec3::new(
        (signum(dx) * 0.5 - movement.x) * pull,
        (signum(dy) * f64::from(0.7_f32) - movement.y) * pull,
        (signum(dz) * 0.5 - movement.z) * pull,
    ));
    me.delta = new_movement;
    // 反编译出来的 `180.0F / (float)Math.PI` 是一个折叠好的浮点常量（`Mth.RAD_TO_DEG`）。
    // The decompiled `180.0F / (float)Math.PI` is one folded float constant.
    let degrees = mth::atan2(new_movement.z, new_movement.x) * f64::from(RAD_TO_DEG);
    let wanted = degrees as f32 - 90.0;
    let turn = mth::wrap_degrees(wanted - me.y_rot);
    mob.zza = 0.5;
    me.y_rot += turn;
    if me.random.next_int_bounded(100) == 0 && view.block_state(above).is_redstone_conductor() {
        mob.bat.set_resting(true);
    }
}

/// `Math.signum(double)`：0 和 NaN 原样返回。
///
/// `Math.signum(double)`: zero and NaN pass through.
fn signum(value: f64) -> f64 {
    if value == 0.0 || value.is_nan() {
        value
    } else {
        value.signum()
    }
}

/// `Vec3i.closerToCenterThan`。
fn closer_to_center_than(target: BlockPos, position: Vec3, distance: f64) -> bool {
    let dx = f64::from(target.x) + 0.5 - position.x;
    let dy = f64::from(target.y) + 0.5 - position.y;
    let dz = f64::from(target.z) + 0.5 - position.z;
    dx * dx + dy * dy + dz * dz < distance * distance
}

/// 新的飞行目标：x、z 各 `nextInt(7) - nextInt(7)`，y 是 `nextInt(6) - 2`，取数顺序是
/// x 两次、y 一次、z 两次。
///
/// A fresh flight target: `nextInt(7) - nextInt(7)` for x and z, `nextInt(6) - 2` for y,
/// drawn as two for x, one for y and two for z.
fn random_target(me: &mut Entity) -> BlockPos {
    let random = &mut me.random;
    let x = me.position.x + f64::from(random.next_int_bounded(7))
        - f64::from(random.next_int_bounded(7));
    let y = me.position.y + f64::from(random.next_int_bounded(6)) - 2.0;
    let z = me.position.z + f64::from(random.next_int_bounded(7))
        - f64::from(random.next_int_bounded(7));
    BlockPos::containing(Vec3::new(x, y, z))
}

/// `Bat.tick` 在 `super.tick()` 之后的部分：倒挂时速度清零、`setPosRaw` 到这一格顶部
/// 下面一个身高；飞行时竖直速度乘 0.6。
///
/// The part of `Bat.tick` after `super.tick()`: a hanging bat has its velocity zeroed and
/// is `setPosRaw`-ed one body height below the top of its block; a flying bat scales its
/// vertical velocity by 0.6.
pub(super) fn tick_after(me: &mut Entity, mob: &Mob) {
    if mob.bat.is_resting() {
        me.delta = Vec3::default();
        let y = f64::from(mth::floor(me.position.y)) + 1.0 - f64::from(me.dimensions.1);
        me.position = Vec3::new(me.position.x, y, me.position.z);
    } else {
        me.delta = me.delta.multiply(1.0, 0.6, 1.0);
    }
}

/// `Bat.hurtServer`：没有免疫就先醒过来，再走 `LivingEntity.hurtServer`。
///
/// `Bat.hurtServer`: wake up unless invulnerable, then `LivingEntity.hurtServer`.
pub(super) fn hurt_server(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    source: DamageSource,
    amount: f32,
) -> bool {
    if living::is_invulnerable_to(me, mob, &source) {
        return false;
    }
    mob.bat.set_resting(false);
    living::living_hurt_server(me, mob, ctx, source, amount)
}

/// `Bat.checkBatSpawnRules`：在 `WORLD_SURFACE` 高度图之下，世界随机源 `nextBoolean`
/// 为假，亮度不超过 `nextInt(4)`，脚下在 `#bats_spawnable_on` 里，再加
/// `Mob.checkMobSpawnRules`。26.3 里没有万圣节的亮度窗口。
///
/// `Bat.checkBatSpawnRules`: below the `WORLD_SURFACE` heightmap, a false `nextBoolean`
/// from the level random, light at most `nextInt(4)`, a `#bats_spawnable_on` block below,
/// then `Mob.checkMobSpawnRules`. 26.3 has no Halloween light window.
pub(super) fn check_spawn_rules(world: &World, pos: BlockPos) -> bool {
    if pos.y >= world.world_surface_y(pos) {
        return false;
    }
    let random = world.random();
    if random.next_boolean() {
        return false;
    }
    if world.max_local_raw_brightness(pos) > random.next_int(4) {
        return false;
    }
    let below = world.block_state(pos.below());
    below.is_tagged(block_tags::BATS_SPAWNABLE_ON)
        && below.is_valid_spawn_surface(MobType::Bat.entity_type())
}

#[cfg(test)]
mod tests {
    use mc_bamboo_nbt::Tag;

    use super::*;
    use crate::{archive::tag, entity::mob::nbt};

    #[test]
    fn a_new_bat_hangs_and_has_six_health() {
        let bat = Mob::new(MobType::Bat);
        assert!(bat.bat.is_resting());
        assert_eq!(bat.health, 6.0);
        assert!(!MobType::Bat.is_ageable());
        assert!(!MobType::Bat.is_animal());
        assert!(MobType::Bat.removes_when_far_away());
        assert!(MobType::Bat.allowed_in_peaceful());
    }

    #[test]
    fn bat_flags_survive_a_save_and_load() {
        let mut entity = Entity::by_mob_type(MobType::Bat, Vec3::new(0.5, 62.0, 0.5));
        entity.with_mob(|_, mob| mob.bat.set_resting(false));
        let saved: Tag = nbt::write_mob(&entity.split()).expect("a bat saves");
        assert_eq!(tag::byte_or(&saved, "BatFlags", 9), 0);
        assert!(tag::field(&saved, "Age").is_none());
        assert!(tag::field(&saved, "InLove").is_none());
        let mut resting = saved.clone();
        if let Tag::Compound(entries) = &mut resting {
            for (name, value) in entries.iter_mut() {
                if name == "BatFlags" {
                    *value = Tag::Byte(1);
                }
            }
        }
        let row = nbt::read_mob(&resting, "minecraft:bat", 7)
            .expect("the tag loads")
            .expect("bats are modelled");
        let entity = row.into_entity();
        assert!(entity.kind.mob().expect("a mob").bat.is_resting());
    }

    #[test]
    fn signum_keeps_zero() {
        assert_eq!(signum(0.0), 0.0);
        assert_eq!(signum(-3.0), -1.0);
        assert_eq!(signum(2.5), 1.0);
    }
}
