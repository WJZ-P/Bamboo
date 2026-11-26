//! 凋灵存档往返：官方 `WitherBoss.addAdditionalSaveData` / `readAdditionalSaveData`
//! 的 `Invul` 键（无敌刻数）以及通用键逐键对拍。
//!
//! Wither save parity: round-trip the `Invul` key (invulnerability ticks) of
//! vanilla `WitherBoss.addAdditionalSaveData` / `readAdditionalSaveData` plus
//! the common keys.

use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WitherSaveCase {
    invul: i32,
    nbt: String,
    reloaded: String,
}

const WITHER_SAVE_KEYS: &[&str] = &["Invul", "Health", "Pos", "Motion", "Rotation", "UUID"];

impl Report {
    pub(super) fn replay_wither_save(&mut self, case: &WitherSaveCase) {
        use mc_bamboo_blocks::entity::{
            Entity,
            mob::{MobType, nbt},
        };
        use mc_bamboo_core::Vec3;

        self.cases += 1;
        let label = format!("wither save Invul {}", case.invul);
        let official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: SNBT does not parse: {error:?}"));
                return;
            }
        };
        let reloaded = match mc_bamboo_nbt::parse_snbt(&case.reloaded) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: reloaded SNBT does not parse: {error:?}"));
                return;
            }
        };
        let mut entity = Entity::by_mob_type(MobType::Wither, Vec3::default());
        if let Err(error) = nbt::load_into(&mut entity, &official) {
            self.mismatch(format!("{label}: load failed: {error:?}"));
            return;
        }
        let ours = match nbt::write_mob(&entity.split()) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{label}: save failed: {error:?}"));
                return;
            }
        };
        // 四处改坏确认：分别把生产代码算出的 `Invul` / `Health` / `Pos` / `Rotation`
        // 写坏，逐键比较都要报差异。
        // Four break checks: corrupt each of the production values `Invul`,
        // `Health`, `Pos` and `Rotation`; every key comparison must report a mismatch.
        let broken = self.break_test.clone();
        for key in WITHER_SAVE_KEYS {
            let expected =
                super::small_mob::save_field(&reloaded, key).map(super::small_mob::canonical_tag);
            let mut actual =
                super::small_mob::save_field(&ours, key).map(super::small_mob::canonical_tag);
            match (broken.as_deref(), *key) {
                (Some("wither_save_invul"), "Invul") => {
                    actual = Some(mc_bamboo_nbt::Tag::Int(case.invul.wrapping_add(1)));
                }
                (Some("wither_save_health"), "Health") => {
                    actual = Some(mc_bamboo_nbt::Tag::Float(0.0));
                }
                (Some("wither_save_pos"), "Pos") => actual = None,
                (Some("wither_save_rotation"), "Rotation") => actual = None,
                _ => {}
            }
            if expected != actual {
                self.mismatch(format!(
                    "{label} {key}: official {expected:?}, bamboo {actual:?}"
                ));
            }
        }
        println!("{label}: checked");
    }
}
