//! 羊驼的远程攻击：`RangedAttackGoal(this, 1.25, 40, 20.0F)` 的 `tick` 和
//! `Llama.performRangedAttack` → `spit`。口水实体本身在 `entity::spit`。
//!
//! The llama's ranged attack: the `tick` of `RangedAttackGoal(this, 1.25, 40,
//! 20.0F)` and `Llama.performRangedAttack` → `spit`. The spit entity lives in
//! `entity::spit`.

use mc_bamboo_core::mth;

use super::Mob;
use crate::entity::{
    Entity,
    ctx::{Command, EntityCtx},
    mob::{goals::CombatTarget, living},
    spit::SpitShot,
};

const SPEED: f64 = 1.25;
const INTERVAL_MIN: i32 = 40;
const INTERVAL_MAX: i32 = 40;
const RADIUS: f32 = 20.0;

/// `RangedAttackGoal.tick`：先算距离和视线，近且看见够 5 刻就停下，否则追；
/// 盯住目标；计时到 0 且看得见就吐，计时为负就按距离重设。
///
/// `RangedAttackGoal.tick`: distance and sight first; stop when close and
/// seen for 5 ticks, otherwise chase; look at the target; spit when the timer
/// reaches 0 with sight, or reset it from the distance when negative.
pub(in crate::entity::mob) fn ranged_attack_tick(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    target: CombatTarget,
    attack_time: &mut i32,
    see_time: &mut i32,
) {
    let distance_sqr = me.position.distance_to_sqr(target.position);
    let seen = living::has_line_of_sight(me, mob, ctx.blocks(), target.uid, target.eye_position());
    *see_time = if seen { *see_time + 1 } else { 0 };
    let radius_sqr = f64::from(RADIUS * RADIUS);
    if distance_sqr <= radius_sqr && *see_time >= 5 {
        mob.navigation.stop();
    } else {
        let nav = living::nav_mob(me, mob, ctx.blocks());
        mob.navigation
            .move_to_entity(ctx.blocks(), &nav, target.position, SPEED, 1);
    }
    mob.look.set_look_at(
        target.position.x,
        target.eye_y(),
        target.position.z,
        30.0,
        30.0,
    );
    *attack_time -= 1;
    if *attack_time == 0 {
        if !seen {
            return;
        }
        let dist = distance_sqr.sqrt() as f32 / RADIUS;
        spit(me, mob, ctx, target);
        *attack_time = mth::floor(f64::from(
            dist * (INTERVAL_MAX - INTERVAL_MIN) as f32 + INTERVAL_MIN as f32,
        ));
    } else if *attack_time < 0 {
        *attack_time = mth::floor(mth::lerp(
            distance_sqr.sqrt() / f64::from(RADIUS),
            f64::from(INTERVAL_MIN),
            f64::from(INTERVAL_MAX),
        ));
    }
}

/// `Llama.spit`：生成口水交给落地，自己播音效（不静音时才取两次 `nextFloat`），记下 `didSpit`。
///
/// `Llama.spit`: the spawn goes to a command; the llama plays the sound (two
/// `nextFloat` draws only when not silent) and records `didSpit`.
fn spit(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, target: CombatTarget) {
    let shot = SpitShot {
        owner_position: me.position,
        owner_body_yaw: mob.y_body_rot,
        owner_width: me.dimensions.0,
        owner_eye_height: mob.eye_height,
        owner_uuid: mob.uuid,
        target: mc_bamboo_core::Vec3::new(
            target.position.x,
            target.position.y + target.height * 0.333_333_333_333_333_3,
            target.position.z,
        ),
    };
    ctx.emit(Command::ShootLlamaSpit(shot), me);
    if !mob.silent {
        let pitch = 1.0 + (me.random.next_float() - me.random.next_float()) * 0.2;
        living::play_sound(me, mob, "minecraft:entity.llama.spit", 1.0, pitch);
    }
    if let Some(state) = mob.equine.as_mut() {
        state.did_spit = true;
    }
}

/// 读出并清掉 `didSpit`（`LlamaHurtByTargetGoal.canContinueToUse` 的开头）。
///
/// Read and clear `didSpit`, as `LlamaHurtByTargetGoal.canContinueToUse` starts.
pub(in crate::entity::mob) fn take_did_spit(mob: &mut Mob) -> bool {
    mob.equine
        .as_mut()
        .is_some_and(|state| std::mem::take(&mut state.did_spit))
}
