//! 马科的组生成数据和 `finalizeSpawn` 调用顺序。
//!
//! Equine spawn-group data and `finalizeSpawn` order.

use super::{EquineState, MobType, horse, llama, randomize_attributes};
use crate::{
    entity::{
        Entity,
        mob::{
            living,
            spawn::{AgeableGroupData, BABY_START_AGE, SpawnGroup, finalize_mob},
        },
    },
    random::RandomSource,
};

/// `finalizeSpawn` 的整条覆写链：子类（花色 / 力量）→ `AbstractHorse.randomizeAttributes`
/// → `AgeableMob` 的幼崽判定 → `Mob.finalizeSpawn`。`group` 是同一组生成共享的组数据。
///
/// The whole `finalizeSpawn` override chain: subclass (variant / strength) →
/// `AbstractHorse.randomizeAttributes` → the `AgeableMob` baby roll → `Mob.finalizeSpawn`.
/// `group` is the group data shared by one spawn group.
pub(in crate::entity::mob) fn finalize_spawn(
    entity: &mut Entity,
    random: &dyn RandomSource,
    group: &mut Option<SpawnGroup>,
) {
    entity.with_mob(|me, mob| {
        let state = mob.equine.as_mut().expect("equine spawn state");
        let age = match mob.mob_type {
            MobType::Horse => horse_group(state, random, group),
            MobType::Llama => llama_group(state, random, group),
            _ => chested_horse_group(group),
        };
        randomize_attributes(mob, random);
        if age.roll_baby(random) {
            living::set_age(me, mob, BABY_START_AGE);
        }
        finalize_mob(random, mob);
    });
}

/// `Horse.finalizeSpawn`：不是 `HorseGroupData` 时先抽毛色并新建组，斑纹每只都抽。
///
/// `Horse.finalizeSpawn`: without a `HorseGroupData`, draw the coat and start a group;
/// markings are drawn for every horse.
fn horse_group<'a>(
    state: &mut EquineState,
    random: &dyn RandomSource,
    group: &'a mut Option<SpawnGroup>,
) -> &'a mut AgeableGroupData {
    if !matches!(group, Some(SpawnGroup::Horse(..))) {
        *group = Some(SpawnGroup::Horse(
            AgeableGroupData::new(0.05),
            horse::roll_coat(random),
        ));
    }
    let Some(SpawnGroup::Horse(age, coat)) = group else {
        unreachable!("horse group just initialized");
    };
    horse::apply_coat(state, *coat, random);
    age
}

/// `Llama.finalizeSpawn`：先抽力量，不是 `LlamaGroupData` 时再抽毛色并新建组。
///
/// `Llama.finalizeSpawn`: strength first, then the colour and a new group when there
/// is no `LlamaGroupData`.
fn llama_group<'a>(
    state: &mut EquineState,
    random: &dyn RandomSource,
    group: &'a mut Option<SpawnGroup>,
) -> &'a mut AgeableGroupData {
    llama::roll_strength(state, random);
    if !matches!(group, Some(SpawnGroup::Llama(..))) {
        *group = Some(SpawnGroup::Llama(
            AgeableGroupData::new(0.05),
            random.next_int(4),
        ));
    }
    let Some(SpawnGroup::Llama(age, variant)) = group else {
        unreachable!("llama group just initialized");
    };
    state.variant = *variant;
    age
}

/// 驴和骡走 `AbstractHorse.finalizeSpawn`：没有组数据时新建幼崽概率 0.2 的动物组。
///
/// Donkeys and mules use `AbstractHorse.finalizeSpawn`: a fresh animal group with a
/// 0.2 baby chance when there is none.
fn chested_horse_group(group: &mut Option<SpawnGroup>) -> &mut AgeableGroupData {
    let data = group.get_or_insert_with(|| SpawnGroup::Ageable(AgeableGroupData::new(0.2)));
    match data {
        SpawnGroup::Ageable(age) | SpawnGroup::Horse(age, _) | SpawnGroup::Llama(age, _) => age,
        SpawnGroup::Zombie(_) => unreachable!("equine with a zombie spawn group"),
        SpawnGroup::Spider(_) => unreachable!("equine with a spider spawn group"),
        SpawnGroup::FishSchool { .. } => unreachable!("equine with a fish spawn group"),
        SpawnGroup::Wild(_) => unreachable!("equine with a wild animal group"),
        SpawnGroup::Axolotl { .. } => unreachable!("equine with an axolotl spawn group"),
    }
}
