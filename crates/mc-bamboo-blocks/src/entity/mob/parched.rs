//! 焦骸：`AbstractSkeleton` 一支的第四种。目标、装备、日晒和存档都复用骷髅，
//! 官方字节码里只覆写生命 16、射击间隔 70 / 50（困难）、四种音效、箭上的
//! 600 刻虚弱，以及对虚弱免疫（`canBeAffected`）。
//!
//! Parched: the fourth `AbstractSkeleton`. Goals, equipment, sunburn and saves
//! reuse the skeleton. The bytecode overrides only 16 health, 70 / 50-tick attack
//! intervals, four sounds, a 600-tick weakness arrow, and weakness immunity.

use mc_bamboo_data::registries::{attribute, mob_effect};

use super::attributes::Attributes;

/// `Parched.getHardAttackInterval()` / `getAttackInterval()`。
pub(super) const HARD_ATTACK_INTERVAL: i32 = 50;
pub(super) const ATTACK_INTERVAL: i32 = 70;

/// `Parched.getArrow`：`new MobEffectInstance(MobEffects.WEAKNESS, 600)`。
pub(super) const ARROW_EFFECT: (i32, i32) = (mob_effect::WEAKNESS, 600);

/// `Parched.createAttributes`：骷髅的属性，生命改成 16。
///
/// `Parched.createAttributes`: the skeleton attributes with 16 max health.
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::skeleton();
    attributes.set_base(attribute::MAX_HEALTH, 16.0);
    attributes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parched_has_bogged_health_and_weakness_arrows() {
        assert_eq!(attributes().value(attribute::MAX_HEALTH), 16.0);
        assert_eq!(ARROW_EFFECT, (mob_effect::WEAKNESS, 600));
        assert_eq!((HARD_ATTACK_INTERVAL, ATTACK_INTERVAL), (50, 70));
    }
}
