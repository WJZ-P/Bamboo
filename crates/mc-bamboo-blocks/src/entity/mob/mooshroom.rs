//! 哞菇：红 / 棕两个品种、用碗接蘑菇煲和迷之炖菜、喂花、剪成牛、繁殖变种、雷击换色。
//!
//! 目标、属性、叫声都是 `AbstractCow` 的（叫声固定 `classic`），这里只有哞菇自己的部分。
//!
//! Mooshrooms: the red and brown types, stew and suspicious stew from a bowl, feeding
//! flowers, shearing into a cow, the bred calf's type and the lightning swap. Goals,
//! attributes and sounds are `AbstractCow`'s (the sound set is always `classic`).

use mc_bamboo_core::{BlockPos, Hand, Vec3};
use mc_bamboo_data::{
    blocks, entity_types, items,
    registries::{data_component_type as kinds, mob_effect},
    tags::block_tags,
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{ComponentValue, value::StewEffect},
    item::ItemStack,
};

use super::{
    Mob, MobEvent, MobType,
    conversion::{self, ConversionParams},
    interact::{MobInteractRequest, MobInteraction},
    living,
};
use crate::{
    InteractionResult,
    entity::{Entity, JavaRandom, ctx::BlockView},
    world::World,
};

/// `MushroomCow.MUTATE_CHANCE`。
const MUTATE_CHANCE: i32 = 1024;

/// 哞菇自己的状态：`DATA_TYPE`、`stewEffects`、`lastLightningBoltUUID`。
///
/// A mooshroom's own state: `DATA_TYPE`, `stewEffects` and `lastLightningBoltUUID`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Mooshroom {
    /// `MushroomCow.Variant`：假是 `RED`（缺省），真是 `BROWN`。
    ///
    /// `MushroomCow.Variant`: false is `RED` (the default), true is `BROWN`.
    pub brown: bool,
    /// 喂过花之后存着的迷之炖菜效果，接一次炖菜就清掉。
    ///
    /// The stew effects stored after eating a flower; cleared by the next bowl.
    pub stew_effects: Option<Vec<StewEffect>>,
    /// 上一道劈中它的闪电，同一道闪电只换一次色。不存盘。
    ///
    /// The last lightning bolt that hit it, so one bolt swaps the type once. Not saved.
    pub last_lightning: Option<[u8; 16]>,
}

impl Mooshroom {
    /// `Variant.getSerializedName`，也是 `minecraft:mooshroom/variant` 组件的值。
    ///
    /// `Variant.getSerializedName`, also the `minecraft:mooshroom/variant` component value.
    pub const fn type_name(&self) -> &'static str {
        if self.brown { "brown" } else { "red" }
    }

    /// `Variant.id`：同步元数据 `DATA_TYPE` 的值。
    ///
    /// `Variant.id`, the value of the synced `DATA_TYPE`.
    pub const fn type_id(&self) -> i32 {
        self.brown as i32
    }
}

/// 成年取 `EntityTypes.MOOSHROOM`（`eyeHeight(1.3F)`），幼年是 `MushroomCow.BABY_DIMENSIONS`。
///
/// Adults use `EntityTypes.MOOSHROOM` (`eyeHeight(1.3F)`); babies use
/// `MushroomCow.BABY_DIMENSIONS`.
pub(super) const fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (0.45, 0.7, 0.69)
    } else {
        let facts = &entity_types::FACTS[MobType::Mooshroom.entity_type() as usize];
        (facts.width, facts.height, 1.3)
    }
}

/// `MushroomCow.getWalkTargetValue`：脚下是菌丝给 10，否则按亮度算路径代价（草方块不加分）。
///
/// `MushroomCow.getWalkTargetValue`: 10 on mycelium, otherwise the light-based path cost
/// (grass blocks earn nothing).
pub(super) fn walk_target_value(view: &dyn BlockView, pos: BlockPos) -> f32 {
    if view.block_state(pos.below()).is(blocks::MYCELIUM) {
        return 10.0;
    }
    super::goals::light_level_cost(view, pos)
}

/// `MushroomCow.checkMushroomSpawnRules`：脚下在 `mooshrooms_spawnable_on` 里，且
/// `isBrightEnoughToSpawn`（`getRawBrightness(pos, 0) > 8`）。
///
/// `MushroomCow.checkMushroomSpawnRules`: the block below is `mooshrooms_spawnable_on` and
/// `isBrightEnoughToSpawn` holds (`getRawBrightness(pos, 0) > 8`).
pub(super) fn spawn_rules(world: &World, pos: BlockPos) -> bool {
    world
        .block_state(pos.below())
        .is_tagged(block_tags::MOOSHROOMS_SPAWNABLE_ON)
        && world.raw_brightness(pos) > 8
}

/// `MushroomCow.getEffectsFromItemStack`：`SuspiciousEffectHolder.tryGet(item)`。
///
/// `MushroomCow.getEffectsFromItemStack`: `SuspiciousEffectHolder.tryGet(item)`.
fn flower_effects(item: i32) -> Option<Vec<StewEffect>> {
    let (_, effects) = entity_types::STEW_FLOWERS
        .iter()
        .find(|(flower, _)| *flower == item)?;
    Some(
        effects
            .iter()
            .map(|&(effect, duration)| StewEffect { effect, duration })
            .collect(),
    )
}

/// `MushroomCow.mobInteract` 里哞菇自己的三个分支：碗、剪刀、棕色哞菇吃花。
/// 都不匹配时返回 `None`，调用方接着走 `AbstractCow.mobInteract`（挤奶、喂食）。
///
/// The three mooshroom branches of `MushroomCow.mobInteract`: bowl, shears and a brown
/// mooshroom eating a flower. `None` falls through to `AbstractCow.mobInteract`.
pub(super) fn interact(
    me: &mut Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
) -> Option<MobInteraction> {
    if req.held.is_empty() {
        return None;
    }
    let baby = mob.is_baby();
    if req.held.item == items::BOWL && !baby {
        return Some(fill_bowl(me, mob));
    }
    if req.held.item == items::SHEARS && !baby {
        return Some(MobInteraction {
            result: InteractionResult::Success,
            shear: true,
            ..MobInteraction::pass()
        });
    }
    if mob.mooshroom.brown && !baby {
        let effects = flower_effects(req.held.item)?;
        return Some(eat_flower(me, mob, effects, req.creative));
    }
    None
}

/// 碗分支：有存着的效果就给迷之炖菜并清掉，否则给蘑菇煲；`createFilledResult(.., false)`
/// 在创造模式也照常把炖菜放进物品栏。
///
/// The bowl branch: suspicious stew when effects are stored (then cleared), mushroom stew
/// otherwise. `createFilledResult(.., false)` hands the stew over in creative mode too.
fn fill_bowl(me: &Entity, mob: &mut Mob) -> MobInteraction {
    let (stew, sound) = match mob.mooshroom.stew_effects.take() {
        Some(effects) => {
            let mut stew = ItemStack::new(items::SUSPICIOUS_STEW, 1);
            crate::item_exchange::set_component(
                &mut stew,
                kinds::SUSPICIOUS_STEW_EFFECTS,
                ComponentValue::SuspiciousStewEffects(effects),
            );
            (stew, "minecraft:entity.mooshroom.suspicious_milk")
        }
        None => (
            ItemStack::new(items::MUSHROOM_STEW, 1),
            "minecraft:entity.mooshroom.milk",
        ),
    };
    living::play_sound(me, mob, sound, 1.0, 1.0);
    MobInteraction {
        result: InteractionResult::Success,
        replacement: Some(stew),
        limit_creative: false,
        ..MobInteraction::pass()
    }
}

/// 吃花分支：已经存着效果时只冒烟、不消耗；否则消耗一朵、记下效果、放吃东西的声音
/// （音量 2）。两种粒子（`SMOKE` / `EFFECT`）没有发，见 README 已知限制。
///
/// The flower branch: with effects already stored nothing is consumed (smoke only);
/// otherwise one flower is eaten, its effects stored and the eating sound played at
/// volume 2. The `SMOKE` / `EFFECT` particles are not sent (see the README limits).
fn eat_flower(
    me: &Entity,
    mob: &mut Mob,
    effects: Vec<StewEffect>,
    creative: bool,
) -> MobInteraction {
    if mob.mooshroom.stew_effects.is_some() {
        return MobInteraction {
            result: InteractionResult::Success,
            ..MobInteraction::pass()
        };
    }
    mob.mooshroom.stew_effects = Some(effects);
    living::play_sound(me, mob, "minecraft:entity.mooshroom.eat", 2.0, 1.0);
    MobInteraction {
        result: InteractionResult::Success,
        shrink_held: !creative,
        ..MobInteraction::pass()
    }
}

/// `MushroomCow.shear` 加上 `mobInteract` 之后的 `hurtAndBreak(1)`：剪毛声，`convertTo(COW)`
/// 先构造牛（拿 ID 和种子）并抄状态，回调里按 `shearing/mooshroom` 表每个蘑菇生成一个物品
/// 实体（`getY(1.0)`，直接 `new ItemEntity`，拾取延迟 0），然后牛进世界、哞菇丢弃。
/// 爆炸粒子没有发。
///
/// `MushroomCow.shear` plus the later `hurtAndBreak(1)`: the shear sound, then
/// `convertTo(COW)` builds the cow first (id and seed) and copies state; its callback drops
/// one item entity per mushroom of `shearing/mooshroom` at `getY(1.0)` (a plain
/// `new ItemEntity`, pickup delay 0); the cow joins the world and the mooshroom is
/// discarded. The explosion particle is not sent.
pub(crate) fn apply_shear(world: &mut World, entity: &mut Entity, hand: Hand, tool: &ItemStack) {
    let Some(request) = entity.with_mob(|me, mob| {
        super::EntityLootRequest::new("minecraft:shearing/mooshroom", me, mob, Some(tool.clone()))
    }) else {
        return;
    };
    let mut cow = world.prepare_entity(Entity::by_mob_type(MobType::Cow, Vec3::default()));
    conversion::convert_single(
        entity,
        &mut cow,
        ConversionParams {
            keep_equipment: false,
            preserve_can_pick_up_loot: false,
        },
    );
    // 剪毛声（玩家音源）挂在新牛的事件上：哞菇这一刻就丢弃了，事件跟着活下来的实体走。
    // The shear sound (player category) rides on the new cow's events, because the
    // mooshroom is discarded this very tick.
    let position = entity.position;
    cow.with_mob(|_, mob| {
        mob.events.push(MobEvent::PlayerSound {
            sound: "minecraft:entity.mooshroom.shear",
            position,
        });
    });
    let at = Vec3::new(
        entity.position.x,
        entity.position.y + f64::from(entity.dimensions.1),
        entity.position.z,
    );
    for drop in world.entity_loot_drops(request) {
        for _ in 0..drop.count {
            let mut single = drop.clone();
            single.count = 1;
            world.spawn_item_with_delay(at, single, 0);
        }
    }
    world.add_prepared_entity(cow);
    entity.discard();
    crate::durability::hurt_hand(world, hand, 1);
}

/// `MushroomCow.getOffspringVariant`：父母同色时先抽 `nextInt(1024)`，中了就变成另一种；
/// 否则（或父母不同色）`nextBoolean` 选一方。随机源是发起繁殖那一方自己的。
///
/// `MushroomCow.getOffspringVariant`: same-typed parents first draw `nextInt(1024)` for a
/// mutation; otherwise `nextBoolean` picks a parent. Draws come from the breeding parent.
pub(super) fn offspring_brown(mine: bool, mate: bool, random: &mut JavaRandom) -> bool {
    if mine == mate && random.next_int_bounded(MUTATE_CHANCE) == 0 {
        !mine
    } else if random.next_boolean() {
        mine
    } else {
        mate
    }
}

/// `MushroomCow.thunderHit`：同一道闪电只算一次，红棕互换并放转换声（音量 2）。
/// Bamboo 还没有闪电实体，暂时没有调用方。
///
/// `MushroomCow.thunderHit`: one bolt counts once; the type flips and the convert sound
/// plays at volume 2. Bamboo has no lightning entity yet, so nothing calls this.
pub fn thunder_hit(me: &Entity, mob: &mut Mob, bolt: [u8; 16]) {
    if mob.mooshroom.last_lightning == Some(bolt) {
        return;
    }
    mob.mooshroom.brown = !mob.mooshroom.brown;
    mob.mooshroom.last_lightning = Some(bolt);
    living::play_sound(me, mob, "minecraft:entity.mooshroom.convert", 2.0, 1.0);
}

/// `MushroomCow.addAdditionalSaveData`：`Type` 一定写，`stew_effects` 有才写。
///
/// `MushroomCow.addAdditionalSaveData`: `Type` always, `stew_effects` only when present.
pub(super) fn write_nbt(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    out.push((
        "Type".to_owned(),
        Tag::String(mob.mooshroom.type_name().to_owned()),
    ));
    if let Some(effects) = &mob.mooshroom.stew_effects {
        out.push((
            "stew_effects".to_owned(),
            Tag::List(effects.iter().filter_map(stew_effect_tag).collect()),
        ));
    }
}

/// `SuspiciousStewEffects.Entry.CODEC`：`id` 加 `duration`（等于缺省 160 时省略）。
///
/// `SuspiciousStewEffects.Entry.CODEC`: `id` plus `duration`, omitted at the default 160.
fn stew_effect_tag(effect: &StewEffect) -> Option<Tag> {
    let name = mob_effect::name_of(effect.effect)?;
    let mut entry = vec![("id".to_owned(), Tag::String(name.to_owned()))];
    if effect.duration != 160 {
        entry.push(("duration".to_owned(), Tag::Int(effect.duration)));
    }
    Some(Tag::Compound(entry))
}

/// `MushroomCow.readAdditionalSaveData`：`Type` 缺省 `red`；`stew_effects` 里有一条解不出来，
/// 整个列表就按没有处理（官方 codec 整体失败后 `orElse(null)`）。
///
/// `MushroomCow.readAdditionalSaveData`: `Type` defaults to `red`; one undecodable
/// `stew_effects` entry drops the whole list, as the failed vanilla codec does.
pub(super) fn read_nbt(mob: &mut Mob, tag: &Tag) {
    mob.mooshroom.brown = crate::archive::tag::string_or(tag, "Type") == Some("brown");
    mob.mooshroom.stew_effects = match crate::archive::tag::field(tag, "stew_effects") {
        Some(Tag::List(entries)) => entries.iter().map(read_stew_effect).collect(),
        _ => None,
    };
}

fn read_stew_effect(tag: &Tag) -> Option<StewEffect> {
    let effect = crate::archive::tag::string_or(tag, "id").and_then(mob_effect::by_name)?;
    Some(StewEffect {
        effect,
        duration: crate::archive::tag::int_or(tag, "duration", 160),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offspring_type_follows_the_parent_draws() {
        // 父母不同色：只取一个 `nextBoolean`。
        // Different parents: a single `nextBoolean`.
        let mut random = JavaRandom::new(7);
        let mut expected = JavaRandom::new(7);
        let pick = expected.next_boolean();
        assert_eq!(offspring_brown(false, true, &mut random), !pick);
        assert_eq!(random.next_long(), expected.next_long());
        // 父母同色：先 `nextInt(1024)`，没中再 `nextBoolean`。
        // Same parents: `nextInt(1024)` first, then `nextBoolean` unless it mutated.
        let mut random = JavaRandom::new(9);
        let mut expected = JavaRandom::new(9);
        let mutated = expected.next_int_bounded(1024) == 0;
        if !mutated {
            expected.next_boolean();
        }
        assert_eq!(offspring_brown(true, true, &mut random), !mutated);
        assert_eq!(random.next_long(), expected.next_long());
    }

    #[test]
    fn save_data_round_trips_type_and_stew_effects() {
        let mut mob = Mob::new(MobType::Mooshroom);
        mob.mooshroom.brown = true;
        mob.mooshroom.stew_effects = flower_effects(items::OXEYE_DAISY);
        assert!(mob.mooshroom.stew_effects.is_some());
        let mut entries = Vec::new();
        write_nbt(&mob, &mut entries);
        let mut loaded = Mob::new(MobType::Mooshroom);
        read_nbt(&mut loaded, &Tag::Compound(entries));
        assert_eq!(loaded.mooshroom, mob.mooshroom);
    }

    #[test]
    fn one_lightning_bolt_swaps_the_type_once() {
        let entity = Entity::by_mob_type(MobType::Mooshroom, Vec3::default());
        let mut mob = Mob::new(MobType::Mooshroom);
        thunder_hit(&entity, &mut mob, [1; 16]);
        thunder_hit(&entity, &mut mob, [1; 16]);
        assert!(mob.mooshroom.brown);
        thunder_hit(&entity, &mut mob, [2; 16]);
        assert!(!mob.mooshroom.brown);
    }
}
