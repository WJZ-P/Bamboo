//! 生物的生成入口：`/summon`（官方 `SummonCommand.createEntity`）和刷怪蛋
//! （`SpawnEggItem.useOn` → `EntityType.spawn`），以及两者共用的 `finalizeSpawn`。
//!
//! 随机数的来源和先后照官方：构造函数里的 UUID 和朝向取实体自己的随机源
//! （[`crate::EntityStore::prepare`]），`finalizeSpawn` 和刷怪蛋的朝向取世界随机源。
//! 不做自然刷怪。
//!
//! Mob spawn entry points: `/summon` and spawn eggs, plus the shared
//! `finalizeSpawn`. Random draws follow vanilla: constructor draws come from the
//! entity's own random, `finalizeSpawn` and the egg facing from the level random.

use mc_bamboo_core::{Axis, BlockPos, Vec3, mth};
use mc_bamboo_data::registries::attribute;
use mc_bamboo_nbt::Tag;

use super::{
    CowSoundVariant, CowVariant, MobType,
    attributes::{Modifier, Operation},
    living, nbt,
};
use crate::{
    entity::{Entity, EntityUid, ctx::BlockView, physics},
    random::RandomSource,
    shape::{Aabb, CollisionContext},
    world::World,
};

/// 官方 `EntitySpawnReason` 里牛用得到的几种。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnReason {
    Command,
    SpawnItemUse,
    Breeding,
    /// `EntitySpawnReason.NATURAL`：自然生成。
    Natural,
    /// `EntitySpawnReason.BUCKET`：从桶里放出来。
    ///
    /// `EntitySpawnReason.BUCKET`: placed from a bucket.
    Bucket,
}

/// `/summon` 失败的原因，对应官方的四条错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummonError {
    /// `commands.summon.invalidPosition`。
    InvalidPosition,
    /// `commands.summon.failed`：NBT 读不进去。
    Failed(String),
    /// `commands.summon.failed.uuid`。
    DuplicateUuid,
}

/// `AgeableMob.AgeableMobGroupData`：一组自然生成的动物共享的组计数和幼年概率。
///
/// `AgeableMob.AgeableMobGroupData`: the group counter and baby odds shared by one
/// naturally spawned group of animals.
#[derive(Debug, Clone, Copy)]
pub struct AgeableGroupData {
    pub group_size: i32,
    pub should_spawn_baby: bool,
    pub baby_spawn_chance: f32,
}

impl AgeableGroupData {
    /// `new AgeableMobGroupData(babySpawnChance)`：会生幼崽，组里还没有成员。
    ///
    /// `new AgeableMobGroupData(babySpawnChance)`: babies allowed, no members yet.
    pub(super) fn new(baby_spawn_chance: f32) -> Self {
        Self {
            group_size: 0,
            should_spawn_baby: true,
            baby_spawn_chance,
        }
    }

    /// `AgeableMob.finalizeSpawn`：仅从第二只开始抽幼崽概率。
    ///
    /// `AgeableMob.finalizeSpawn`: roll for a baby only from the second member onward.
    pub(super) fn roll_baby(&mut self, random: &dyn RandomSource) -> bool {
        let baby = self.should_spawn_baby
            && self.group_size > 0
            && random.next_float() <= self.baby_spawn_chance;
        self.group_size += 1;
        baby
    }
}

/// 自然生成的组数据（`SpawnGroupData`）：动物的 `AgeableMobGroupData` 和僵尸的
/// `ZombieGroupData`。
///
/// Spawn-group data (`SpawnGroupData`): `AgeableMobGroupData` for animals and
/// `ZombieGroupData` for zombies.
#[derive(Debug, Clone, Copy)]
pub enum SpawnGroup {
    Ageable(AgeableGroupData),
    /// `Horse.HorseGroupData`：同组共享的毛色编号。
    ///
    /// `Horse.HorseGroupData`: the coat id shared by the group.
    Horse(AgeableGroupData, i32),
    /// `Llama.LlamaGroupData`：同组共享的毛色编号。
    ///
    /// `Llama.LlamaGroupData`: the colour id shared by the group.
    Llama(AgeableGroupData, i32),
    Zombie(super::zombie_spawn::ZombieGroupData),
    Spider(SpiderGroupData),
    /// 鱼群共享领头鱼；热带鱼还共享花纹。
    ///
    /// Schooling fish share a leader; tropical fish also share a variant.
    FishSchool {
        leader: crate::entity::EntityUid,
        variant: Option<i32>,
    },
    /// 兔子、狐狸、狼：组大小加共享的品种。
    ///
    /// Rabbits, foxes and wolves: the group size plus the shared variant.
    Wild(super::wild_spawn::WildGroup),
}

/// `Spider.SpiderEffectsGroupData`：同一组蜘蛛共享的药水效果。
///
/// `Spider.SpiderEffectsGroupData`: the potion effect shared by one spider group.
#[derive(Debug, Clone, Copy)]
pub struct SpiderGroupData {
    pub effect: Option<i32>,
}

/// `Mob.RANDOM_SPAWN_BONUS_ID`。
pub(super) const RANDOM_SPAWN_BONUS: &str = "minecraft:random_spawn_bonus";

/// `Level.isInSpawnableBounds`：竖直 `[-2e7, 2e7)`，水平绝对值小于 3e7。
pub fn is_in_spawnable_bounds(pos: BlockPos) -> bool {
    (-20_000_000..20_000_000).contains(&pos.y)
        && (-30_000_000..30_000_000).contains(&pos.x)
        && (-30_000_000..30_000_000).contains(&pos.z)
}

/// `SummonCommand.createEntity` 加 `tryAddFreshEntityWithPassengers`，只认牛。
/// `nbt` 为 `None` 时（命令没带 NBT）跑 `finalizeSpawn`；带了 NBT 就不跑，和官方一样。
///
/// `/summon` for a cow. Without NBT `finalizeSpawn` runs; with NBT it does not.
pub fn summon_cow(
    world: &mut World,
    pos: Vec3,
    nbt: Option<&Tag>,
) -> Result<EntityUid, SummonError> {
    summon_mob(world, MobType::Cow, pos, nbt)
}

/// `/summon`：按实体类型构造、读 NBT、`snapTo`，不带 NBT 时跑 `finalizeSpawn`。
pub fn summon_mob(
    world: &mut World,
    kind: MobType,
    pos: Vec3,
    nbt: Option<&Tag>,
) -> Result<EntityUid, SummonError> {
    if !is_in_spawnable_bounds(BlockPos::containing(pos)) {
        return Err(SummonError::InvalidPosition);
    }
    let randomize = nbt.is_none();
    let mut entries = match nbt {
        Some(Tag::Compound(entries)) => entries.clone(),
        _ => Vec::new(),
    };
    entries.retain(|(key, _)| key != "id");
    entries.push(("id".to_owned(), Tag::String(kind.name().to_owned())));
    let tag = Tag::Compound(entries);
    // 构造（ID、种子、UUID、朝向）→ `load` → `snapTo(pos, yRot, xRot)`。
    // Construct, load, then snap to the target with the loaded rotation.
    let mut entity = world.prepare_entity(Entity::by_mob_type(kind, Vec3::default()));
    nbt::load_into(&mut entity, &tag).map_err(|error| SummonError::Failed(error.0))?;
    let x_rot = entity.kind.mob().map_or(0.0, |mob| mob.x_rot);
    let y_rot = entity.y_rot;
    snap_to(&mut entity, pos, y_rot, x_rot);
    if randomize {
        finalize_spawn(world, kind, &mut entity, SpawnReason::Command);
    }
    let uuid = entity.kind.mob().map(|mob| mob.uuid).unwrap_or_default();
    if world.has_mob_uuid(&uuid) {
        return Err(SummonError::DuplicateUuid);
    }
    Ok(world.add_prepared_entity(entity))
}

/// `SpawnEggItem.useOn` 走到 `EntityType.spawn(level, stack, player, pos, SPAWN_ITEM_USE,
/// true, offset_more)`：在 `pos` 方块的中心放一头牛，朝向随机，然后叫一声。
/// `custom_name` 是刷怪蛋上的 `custom_name` 组件（NBT 形式）。返回生成的 ID。
///
/// A spawn egg used on a block: a cow centred on `pos`, random facing, then the
/// ambient sound. Returns the new id.
pub fn spawn_cow_from_egg(
    world: &mut World,
    pos: BlockPos,
    offset_more: bool,
    custom_name: Option<Tag>,
) -> EntityUid {
    spawn_egg(world, MobType::Cow, pos, offset_more, custom_name)
}

/// `SpawnEggItem.useOn`：按刷怪蛋对应的实体类型生成。
pub fn spawn_egg(
    world: &mut World,
    kind: MobType,
    pos: BlockPos,
    offset_more: bool,
    custom_name: Option<Tag>,
) -> EntityUid {
    let mut entity = world.prepare_entity(Entity::by_mob_type(kind, Vec3::default()));
    // `shouldOffsetY`：先摆到上一格，再按碰撞算往下落多少。
    // Place it one block up first, then drop it onto the collisions.
    let centre = Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y + 1),
        f64::from(pos.z) + 0.5,
    );
    entity.position = centre;
    let offset = spawn_y_offset(&*world, pos, offset_more, entity.bounding_box());
    let yaw = mth::wrap_degrees(world.random().next_float() * 360.0);
    snap_to(
        &mut entity,
        Vec3::new(centre.x, f64::from(pos.y) + offset, centre.z),
        yaw,
        0.0,
    );
    entity.with_mob(|me, mob| {
        mob.y_head_rot = me.y_rot;
        mob.y_body_rot = me.y_rot;
    });
    finalize_spawn(world, kind, &mut entity, SpawnReason::SpawnItemUse);
    // 刷怪蛋自带 `entity_data`：官方 `saveWithoutId` → 合并 → `load`，`setHealth`
    // 把血量夹到 `finalizeSpawn` 之后的最大血量（马科的最大血量是随机的）。
    // The egg's `entity_data` round-trips the mob through save and load, and
    // `setHealth` clamps to the post-`finalizeSpawn` maximum (random for equines).
    entity.with_mob(|_, mob| living::clamp_health(mob));
    // `appendComponentsConfig`：刷怪蛋的 `custom_name` 套到实体上。
    if let Some(name) = custom_name {
        entity.with_mob(|_, mob| mob.custom_name = Some(name));
    }
    let uid = world.add_prepared_entity(entity);
    // `Mob.playAmbientSound`：放进世界之后叫一声，用生物自己的随机源取音高。
    if kind == MobType::Fox {
        // `Fox.playAmbientSound` 要看天色和 16 格内的玩家，先在世界里取好。
        // `Fox.playAmbientSound` reads the sky and nearby players, gathered first.
        let bright = crate::entity::ctx::BlockView::is_bright_outside(&*world);
        let mut players = Vec::new();
        crate::entity::ctx::BlockView::for_each_player(&*world, &mut |player| {
            players.push(*player)
        });
        world.with_entity_mut(uid, &mut |entity| {
            entity
                .with_mob(|me, mob| super::fox::play_ambient_sound_with(me, mob, bright, &players));
        });
        return uid;
    }
    world.with_entity_mut(uid, &mut |entity| {
        entity.with_mob(living::play_ambient_sound);
    });
    uid
}

/// `EntityType.getYOffset`：脚下那一格（`offset_more` 时再往下一格）的碰撞形状，
/// 让站在上一格的实体沿 Y 往下扫 1（或 2）格。
pub(crate) fn spawn_y_offset(
    view: &dyn BlockView,
    pos: BlockPos,
    offset_more: bool,
    moving: Aabb,
) -> f64 {
    let min_y = if offset_more {
        f64::from(pos.y) - 1.0
    } else {
        f64::from(pos.y)
    };
    let area = Aabb::new(
        f64::from(pos.x),
        min_y,
        f64::from(pos.z),
        f64::from(pos.x + 1),
        f64::from(pos.y + 1),
        f64::from(pos.z + 1),
    );
    let boxes: Vec<Aabb> = view
        .block_collisions(area, CollisionContext::Empty)
        .into_iter()
        .flat_map(|(_, boxes)| boxes)
        .collect();
    let distance = if offset_more { -2.0 } else { -1.0 };
    1.0 + physics::collide_along(Axis::Y, moving, &boxes, distance)
}

/// `Entity.snapTo(x, y, z, yRot, xRot)`：位置、朝向、上一刻的位置和朝向一起设。
pub(crate) fn snap_to(entity: &mut Entity, pos: Vec3, y_rot: f32, x_rot: f32) {
    entity.position = pos;
    entity.old_position = pos;
    entity.y_rot = y_rot;
    entity.with_mob(|_, mob| {
        mob.x_rot = x_rot;
        mob.y_rot_o = y_rot;
        mob.x_rot_o = x_rot;
    });
}

/// `Cow.finalizeSpawn` → `AgeableMob.finalizeSpawn` → `Mob.finalizeSpawn`，`groupData` 为空。
///
/// 1. 花色按脚下群系挑（只有一个最高优先级的候选，但官方仍 `nextInt(1)` 取一次）；
/// 2. 叫声在两种里 `nextInt(2)`；
/// 3. `AgeableMobGroupData(true)` 的组大小是 0，`getGroupSize() > 0` 为假，不取数也不变小；
/// 4. 跟随距离加一个 `triangle(0, 0.11485)` 的修饰符（没有时才加）；
/// 5. `nextFloat() < 0.05` 左撇子。
pub(crate) fn finalize_spawn(
    world: &mut World,
    kind: MobType,
    entity: &mut Entity,
    reason: SpawnReason,
) {
    let mut group = None;
    finalize_spawn_in_group(world, kind, entity, reason, &mut group);
}

/// `finalizeSpawn` 的自然生成入口：组数据在一组的多个个体之间共享。
///
/// The natural-spawn entry of `finalizeSpawn`: the group data is shared by every mob
/// of one group.
pub fn finalize_spawn_with_group(
    world: &mut World,
    kind: MobType,
    entity: &mut Entity,
    group: &mut Option<SpawnGroup>,
) {
    finalize_spawn_in_group(world, kind, entity, SpawnReason::Natural, group);
}

/// `AgeableMob.getBabyStartAge()`。
pub(super) const BABY_START_AGE: i32 = -24000;

fn finalize_spawn_in_group(
    world: &mut World,
    kind: MobType,
    entity: &mut Entity,
    reason: SpawnReason,
    group: &mut Option<SpawnGroup>,
) {
    if kind == MobType::Skeleton {
        super::skeleton::finalize_spawn(world, entity);
        return;
    }
    if kind == MobType::Panda {
        super::panda::finalize_spawn(entity, world.random(), group);
        return;
    }
    if matches!(kind, MobType::Zombie | MobType::Husk) {
        super::zombie_spawn::finalize_spawn(world, kind, entity, reason, group);
        return;
    }
    if matches!(kind, MobType::Creeper | MobType::Enderman) {
        super::creeper::finalize_spawn(entity, world.random());
        return;
    }
    if kind == MobType::Spider {
        super::spider::finalize_spawn(world, entity, reason, group);
        return;
    }
    // `CaveSpider.finalizeSpawn` 只把组数据原样返回，连 `Mob.finalizeSpawn` 都不走：
    // 没有跟随距离加成、没有左撇子，也没有骑士和药水效果，一个随机数都不取。
    // `CaveSpider.finalizeSpawn` returns the group data untouched without calling
    // `Mob.finalizeSpawn`: no follow-range bonus, no left-handedness, no jockey, no
    // effect, and no random draws at all.
    if kind == MobType::CaveSpider {
        return;
    }
    if super::squid::is_squid(kind) {
        super::squid::finalize_spawn(entity, world.random(), group);
        return;
    }
    if super::fish::is_fish(kind) {
        super::fish::finalize_spawn(world, entity, group);
        return;
    }
    if super::turtle::is_turtle(kind) {
        super::turtle::finalize_spawn(entity, world.random(), group);
        return;
    }
    if kind.is_equine() {
        super::equine::finalize_spawn(entity, world.random(), group);
        return;
    }
    let biome = world.biome_id(entity.block_position());
    let random = world.random();
    if matches!(kind, MobType::Rabbit | MobType::Fox | MobType::Wolf) {
        super::wild_spawn::finalize(kind, entity, biome, random, group);
        return;
    }
    finalize_cow(kind, entity, biome, world.random(), group);
}

/// 生成期的 `finalizeSpawn(..., CHUNK_GENERATION, groupData)`：随机源是
/// `WorldGenRegion.random`。牛、猪、羊、鸡走 [`finalize_cow`]，马科走
/// `equine::finalize_spawn`，兔子、狐狸、狼走 `wild_spawn::finalize`；以后加进来、
/// 还没接区域随机源的动物暂时退回世界随机源，只影响它们的花色和幼年，不影响落点。
///
/// World-generation `finalizeSpawn(..., CHUNK_GENERATION, groupData)` on
/// `WorldGenRegion.random`. Cows, pigs, sheep and chickens use [`finalize_cow`];
/// equines use `equine::finalize_spawn`; rabbits, foxes and wolves use
/// `wild_spawn::finalize`. Animals added later fall back to the level random until
/// wired, which only affects their variants and age.
pub(super) fn finalize_generated(
    world: &mut World,
    kind: MobType,
    entity: &mut Entity,
    biome: i32,
    region_random: &dyn RandomSource,
    group: &mut Option<SpawnGroup>,
) {
    if matches!(
        kind,
        MobType::Cow
            | MobType::Mooshroom
            | MobType::Pig
            | MobType::Sheep
            | MobType::Chicken
            | MobType::PolarBear
    ) {
        finalize_cow(kind, entity, biome, region_random, group);
    } else if kind == MobType::Panda {
        super::panda::finalize_spawn(entity, region_random, group);
    } else if matches!(kind, MobType::Rabbit | MobType::Fox | MobType::Wolf) {
        super::wild_spawn::finalize(kind, entity, biome, region_random, group);
    } else if kind.is_equine() {
        super::equine::finalize_spawn(entity, region_random, group);
    } else {
        finalize_spawn_in_group(world, kind, entity, SpawnReason::Natural, group);
    }
}

/// `Cow.finalizeSpawn` 加 `AgeableMob` / `Mob` 那两层，随机源由调用方给：运行时是
/// `Level.random`，生成期是 `WorldGenRegion.random`。
///
/// `Cow.finalizeSpawn` plus the `AgeableMob` / `Mob` layers with a caller-supplied
/// random: `Level.random` at run time, `WorldGenRegion.random` during generation.
pub(super) fn finalize_cow(
    kind: MobType,
    entity: &mut Entity,
    biome: i32,
    random: &dyn RandomSource,
    group: &mut Option<SpawnGroup>,
) {
    let variant = cow_variant_for_biome(biome);
    // 羊和哞菇没有按群系选的花纹；哞菇没有自己的 `finalizeSpawn`，也不抽叫声。
    // Sheep and mooshrooms have no biome variant; mooshrooms have no `finalizeSpawn` of
    // their own and draw no sound either.
    // 北极熊也一样：`PolarBear.finalizeSpawn` 只把组里的幼年概率换成 1。
    // Polar bears too: `PolarBear.finalizeSpawn` only raises the group's baby odds to 1.
    let has_variant = !matches!(
        kind,
        MobType::Sheep | MobType::Mooshroom | MobType::PolarBear
    );
    if has_variant {
        // `VariantUtils.selectVariantToSpawn`：唯一的最高优先级候选也取一次 `nextInt(1)`。
        // `selectVariantToSpawn` draws `nextInt(1)` even for a single best candidate.
        let _ = random.next_int(1);
    }
    // 各物种自己那层 `finalizeSpawn` 的取数：羊的毛色、鸡 / 猪 / 牛的叫声，都在
    // `AgeableMob` 那层之前。
    // Each species' own `finalizeSpawn` draws (sheep colour, chicken / pig / cow sound)
    // come before the `AgeableMob` layer.
    let sheep_color = (kind == MobType::Sheep).then(|| super::sheep::spawn_color(variant, random));
    let chicken_sound =
        (kind == MobType::Chicken).then(|| super::chicken::SoundVariant::pick(random));
    let pig_sound = (kind == MobType::Pig).then(|| super::pig::SoundVariant::pick(random));
    let cow_sound = (!matches!(
        kind,
        MobType::Sheep | MobType::Chicken | MobType::Pig | MobType::Mooshroom | MobType::PolarBear
    ))
    .then(|| {
        if random.next_int(2) == 0 {
            CowSoundVariant::Classic
        } else {
            CowSoundVariant::Moody
        }
    });
    // `AgeableMob.finalizeSpawn`：组里第二只起按概率变小，然后把组计数加一。
    // `AgeableMob.finalizeSpawn`: from the second mob on, roll for a baby, then count
    // the mob into the group.
    // `AgeableMob.finalizeSpawn`：不是动物组数据时新建一份（官方 `groupData == null`）。
    // `AgeableMob.finalizeSpawn`: start a fresh animal group when there is none.
    if !matches!(group, Some(SpawnGroup::Ageable(_))) {
        // `PolarBear.finalizeSpawn` 的同组后续成员都是幼崽。
        // In `PolarBear.finalizeSpawn` later group members are always cubs.
        let baby_chance = if kind == MobType::PolarBear {
            1.0
        } else {
            0.05
        };
        *group = Some(SpawnGroup::Ageable(AgeableGroupData::new(baby_chance)));
    }
    let Some(SpawnGroup::Ageable(data)) = group else {
        unreachable!("animal spawn group was just initialized");
    };
    let baby = data.roll_baby(random);
    entity.with_mob(|me, mob| {
        if has_variant {
            mob.variant = variant;
        }
        if let Some(color) = sheep_color {
            super::sheep::set_color(mob, color);
        }
        if let Some(sound) = chicken_sound {
            mob.chicken_sound_variant = sound;
        }
        if let Some(sound) = pig_sound {
            mob.pig_sound_variant = sound;
        }
        if let Some(sound) = cow_sound {
            mob.sound_variant = sound;
        }
        if baby {
            living::set_age(me, mob, BABY_START_AGE);
        }
        finalize_mob(random, mob);
    });
}

/// `AgeableMob.finalizeSpawn` 加 `Mob.finalizeSpawn` 那两层，组数据由调用方给。
///
/// The `AgeableMob.finalizeSpawn` and `Mob.finalizeSpawn` layers with caller group data.
pub(super) fn finalize_ageable(
    entity: &mut Entity,
    random: &dyn RandomSource,
    group: &mut Option<SpawnGroup>,
    baby_chance: f32,
) {
    if !matches!(group, Some(SpawnGroup::Ageable(_))) {
        *group = Some(SpawnGroup::Ageable(AgeableGroupData::new(baby_chance)));
    }
    let Some(SpawnGroup::Ageable(data)) = group else {
        unreachable!("刚放进去的动物组数据");
    };
    let baby = data.roll_baby(random);
    entity.with_mob(|me, mob| {
        if baby {
            living::set_age(me, mob, BABY_START_AGE);
        }
        finalize_mob(random, mob);
    });
}

pub(super) fn finalize_mob(random: &dyn RandomSource, mob: &mut super::Mob) {
    // `Mob.finalizeSpawn`：跟随距离的随机加成和左撇子。
    // `Mob.finalizeSpawn`: the random follow-range bonus and left-handedness.
    if let Some(range) = mob.attributes.instance(attribute::FOLLOW_RANGE)
        && range.modifier(RANDOM_SPAWN_BONUS).is_none()
    {
        let amount = random.triangle(0.0, 0.114_850_000_000_000_01);
        range.modifiers.push(Modifier {
            id: RANDOM_SPAWN_BONUS.to_owned(),
            amount,
            operation: Operation::AddMultipliedBase,
            permanent: true,
        });
    }
    mob.left_handed = random.next_float() < 0.05;
}

/// `cow_variant` 的生成条件：暖和冷各按一个群系标签，优先级 1；其余是温带。
fn cow_variant_for_biome(biome: i32) -> CowVariant {
    if biome_tag_contains("minecraft:spawns_warm_variant_farm_animals", biome) {
        CowVariant::Warm
    } else if biome_tag_contains("minecraft:spawns_cold_variant_farm_animals", biome) {
        CowVariant::Cold
    } else {
        CowVariant::Temperate
    }
}

pub(super) fn biome_tag_contains(name: &str, biome: i32) -> bool {
    mc_bamboo_data::tags::TAG_GROUPS
        .iter()
        .filter(|group| group.registry == "minecraft:worldgen/biome")
        .flat_map(|group| group.tags.iter())
        .any(|tag| tag.name == name && tag.ids.contains(&biome))
}
