//! 兔子、狐狸、狼的 `finalizeSpawn`：各自那一层加 `AgeableMob` 和 `Mob` 两层，
//! 以及一组个体之间共享的组数据（`RabbitGroupData`、`FoxGroupData`、`WolfPackData`）。
//!
//! 随机源由调用方给：运行时是 `Level.random`，生成期是 `WorldGenRegion.random`。
//!
//! `finalizeSpawn` for rabbits, foxes and wolves: the species layer plus the
//! `AgeableMob` and `Mob` layers, with the group data shared inside one group
//! (`RabbitGroupData`, `FoxGroupData`, `WolfPackData`).
//!
//! The caller supplies the random: `Level.random` at run time and
//! `WorldGenRegion.random` during generation.

use super::{MobType, fox, living, rabbit, spawn, spawn::SpawnGroup, wolf};
use crate::{entity::Entity, random::RandomSource};

/// 一组野生动物共享的品种。
///
/// The variant shared by one group of wild animals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pack {
    /// `Rabbit.RabbitGroupData.variant`。
    Rabbit(rabbit::Variant),
    /// `Fox.FoxGroupData.variant`。
    Fox(fox::Variant),
    /// `Wolf.WolfPackData.type`：`wolf_variant` 注册表里的下标。
    Wolf(i32),
}

/// 三种组数据共有的部分：`AgeableMobGroupData.groupSize` 和共享的品种。
///
/// What the three group-data classes share: `AgeableMobGroupData.groupSize` and
/// the shared variant.
#[derive(Debug, Clone, Copy)]
pub struct WildGroup {
    pub group_size: i32,
    pub pack: Pack,
}

/// 三种动物的 `finalizeSpawn`。
///
/// `finalizeSpawn` of the three animals.
pub(super) fn finalize(
    kind: MobType,
    entity: &mut Entity,
    biome: i32,
    random: &dyn RandomSource,
    group: &mut Option<SpawnGroup>,
) {
    match kind {
        MobType::Rabbit => finalize_rabbit(entity, biome, random, group),
        MobType::Fox => finalize_fox(entity, biome, random, group),
        MobType::Wolf => finalize_wolf(entity, biome, random, group),
        _ => unreachable!("只有兔子、狐狸、狼走这里"),
    }
    entity.with_mob(|_, mob| spawn::finalize_mob(random, mob));
}

/// 取出同种的组数据（没有就用 `fresh` 新建），把这一只计进去
/// （`AgeableMobGroupData.increaseGroupSizeByOne`）。返回共享的品种和计入之前的组大小。
///
/// Takes the group data of the same species (or creates it from `fresh`) and counts
/// this mob in. Returns the shared variant and the group size before counting.
fn enter(
    group: &mut Option<SpawnGroup>,
    same_species: fn(Pack) -> bool,
    fresh: impl FnOnce() -> Pack,
) -> (Pack, i32) {
    let current = match group {
        Some(SpawnGroup::Wild(data)) if same_species(data.pack) => *data,
        _ => WildGroup {
            group_size: 0,
            pack: fresh(),
        },
    };
    *group = Some(SpawnGroup::Wild(WildGroup {
        group_size: current.group_size + 1,
        pack: current.pack,
    }));
    (current.pack, current.group_size)
}

/// `Rabbit.finalizeSpawn`：先按群系抽一次品种（有组数据时也抽，只是不用），
/// 再走 `AgeableMob` 那层：`RabbitGroupData` 的幼年概率是 1.0，第二只起都是幼崽，
/// 但 `nextFloat() <= 1.0` 那一次数照取。
///
/// `Rabbit.finalizeSpawn`: the biome variant is always rolled, even when the group
/// data overrides it. `RabbitGroupData` has a baby chance of 1.0, so every rabbit
/// after the first is a baby, and the `nextFloat() <= 1.0` draw still happens.
fn finalize_rabbit(
    entity: &mut Entity,
    biome: i32,
    random: &dyn RandomSource,
    group: &mut Option<SpawnGroup>,
) {
    let rolled = rabbit::random_variant(biome, random);
    let (pack, size) = enter(
        group,
        |pack| matches!(pack, Pack::Rabbit(_)),
        || Pack::Rabbit(rolled),
    );
    let Pack::Rabbit(variant) = pack else {
        unreachable!("兔子的组数据");
    };
    let baby = size > 0 && random.next_float() <= 1.0;
    entity.with_mob(|me, mob| {
        mob.rabbit.variant = variant;
        if baby {
            living::set_age(me, mob, spawn::BABY_START_AGE);
        }
    });
}

/// `Fox.finalizeSpawn`：品种跟组里第一只；组里已有两只时是幼崽；然后
/// `setTargetGoals` 和嘴里的东西。`FoxGroupData` 是 `AgeableMobGroupData(false)`，
/// `AgeableMob` 那层不取数。
///
/// `Fox.finalizeSpawn`: the variant follows the first fox of the group; from the
/// third fox on they are babies; then `setTargetGoals` and the mouth item.
/// `FoxGroupData` is `AgeableMobGroupData(false)`, so `AgeableMob` draws nothing.
fn finalize_fox(
    entity: &mut Entity,
    biome: i32,
    random: &dyn RandomSource,
    group: &mut Option<SpawnGroup>,
) {
    let (pack, size) = enter(
        group,
        |pack| matches!(pack, Pack::Fox(_)),
        || Pack::Fox(fox::Variant::by_biome(biome)),
    );
    let Pack::Fox(variant) = pack else {
        unreachable!("狐狸的组数据");
    };
    entity.with_mob(|me, mob| {
        if size >= 2 {
            living::set_age(me, mob, spawn::BABY_START_AGE);
        }
        fox::finalize_spawn(mob, variant, random);
    });
}

/// `Wolf.finalizeSpawn`：狼群共用第一只选出的品种；叫声每只自己抽。
/// `WolfPackData` 是 `AgeableMobGroupData(false)`：不生幼崽，也不取那一次数。
///
/// `Wolf.finalizeSpawn`: the pack shares the first wolf's variant; each wolf rolls
/// its own sound. `WolfPackData` never spawns babies, so `AgeableMob` draws nothing.
fn finalize_wolf(
    entity: &mut Entity,
    biome: i32,
    random: &dyn RandomSource,
    group: &mut Option<SpawnGroup>,
) {
    let shared = match group {
        Some(SpawnGroup::Wild(WildGroup {
            pack: Pack::Wolf(variant),
            ..
        })) => Some(*variant),
        _ => None,
    };
    let variant = entity
        .with_mob(|_, mob| wolf::finalize_spawn(mob, shared, biome, random))
        .unwrap_or_default();
    enter(
        group,
        |pack| matches!(pack, Pack::Wolf(_)),
        || Pack::Wolf(variant),
    );
}

#[cfg(test)]
mod tests;
