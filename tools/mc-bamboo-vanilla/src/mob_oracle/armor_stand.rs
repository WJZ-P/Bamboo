//! 盔甲架对拍：官方假世界里跑 `ArmorStand` 的存档往返与逐刻行为，Bamboo 用同一场景重放。
//!
//! Armour stand parity: the official fake level runs `ArmorStand` save round trips
//! and per-tick behaviour; Bamboo replays the same scenes.

use mc_bamboo_blocks::entity::{
    Entity,
    mob::{MobType, nbt},
};
use mc_bamboo_core::Vec3;
use mc_bamboo_nbt::Tag;
use serde::Deserialize;

use super::*;

/// 一题存档往返：官方 `saveWithoutId` 的整份 SNBT 和读回后再写出的整份 SNBT。
///
/// One save case: vanilla's full `saveWithoutId` SNBT and the reloaded copy's SNBT.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ArmorStandSaveCase {
    name: String,
    nbt: String,
    reloaded: String,
}

/// `saveWithoutId` 不带 `id`，而 Bamboo 的 `write_mob` 带：这是两种入口的格式差异，
/// 不是实体状态，整棵比较时略过。其余键全部逐项比对。
///
/// `saveWithoutId` omits `id` while Bamboo's `write_mob` writes it: an entry-point
/// format difference, not entity state. Every other key is compared.
const SKIP_KEYS: &[&str] = &["id"];

impl Report {
    /// 官方存档 → `load_into` → `write_mob`，再把整棵 NBT 和官方读回后写出的 NBT 逐键比。
    ///
    /// Official save → `load_into` → `write_mob`, then compare the whole NBT tree
    /// key by key against vanilla's reload-and-save output.
    pub(super) fn replay_armor_stand_save(&mut self, case: &ArmorStandSaveCase) {
        self.cases += 1;
        self.steps += 1;
        let label = case.name.as_str();
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
        let mut entity = Entity::by_mob_type(MobType::ArmorStand, Vec3::default());
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
        compare_nbt(self, label, &reloaded, &ours);
    }
}

/// 两棵复合标签按键名并集逐项比较，复合键顺序按 `canonical_tag` 规范化。
///
/// Compare two compounds over the union of their keys, normalising compound key
/// order with `canonical_tag`.
fn compare_nbt(report: &mut Report, label: &str, expected: &Tag, actual: &Tag) {
    let (Tag::Compound(e), Tag::Compound(a)) = (expected, actual) else {
        report.mismatch(format!("{label}: NBT is not a compound"));
        return;
    };
    let mut keys: Vec<&str> = e
        .iter()
        .map(|(name, _)| name.as_str())
        .chain(a.iter().map(|(name, _)| name.as_str()))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    for key in keys {
        if SKIP_KEYS.contains(&key) {
            continue;
        }
        let want = field(e, key).map(super::small_mob::canonical_tag);
        let got = field(a, key).map(super::small_mob::canonical_tag);
        if want != got {
            report.mismatch(format!("{label} {key}: official {want:?}, bamboo {got:?}"));
        }
    }
}

fn field<'a>(entries: &'a [(String, Tag)], key: &str) -> Option<&'a Tag> {
    entries
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}
