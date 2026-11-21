//! 马科的右键交互：`Horse.mobInteract` / `AbstractChestedHorse.mobInteract` →
//! `AbstractHorse.mobInteract`，喂食的 `handleEating`，以及箱子、鞍和身体护甲的装备。
//!
//! 骑乘（`doPlayerRide` 里的 `startRiding`）还没有，见 README 已知限制。潜行右键驯服的
//! 坐骑只在结果里要求打开物品栏，菜单由游戏层开。
//!
//! Equine right-click: `Horse.mobInteract` / `AbstractChestedHorse.mobInteract`
//! → `AbstractHorse.mobInteract`, feeding through `handleEating`, and equipping
//! chests, saddles and body armor. Riding (`startRiding`) does not exist yet; see
//! the README known limitations. Sneaking on a tamed mount only asks the caller
//! to open the inventory menu.

use mc_bamboo_data::{items, registries::data_component_type as kinds, tags::item_tags};
use mc_bamboo_protocol::{
    components::{ComponentValue, cursor::Holder, defaults, value::Equippable},
    item::ItemStack,
    packet::play::EquipmentSlot,
};

use super::{Mob, MobType, chest_size, life};
use crate::{
    InteractionResult,
    entity::{
        Entity,
        mob::{
            MobEvent,
            equipment::GUARANTEED_DROP_CHANCE,
            interact::{MobInteractRequest, MobInteraction, age_up},
            living,
        },
    },
    state::item_is_tagged,
};

fn tame(mob: &Mob) -> bool {
    mob.equine.as_ref().is_some_and(|state| state.tame)
}

fn chested(mob: &Mob) -> bool {
    mob.equine.as_ref().is_some_and(|state| state.chested)
}

fn success(shrink: bool, req: &MobInteractRequest) -> MobInteraction {
    MobInteraction {
        result: InteractionResult::Success,
        shrink_held: shrink && !req.creative,
        replacement: None,
        open_inventory: false,
    }
}

/// `Horse.mobInteract` 和 `AbstractChestedHorse.mobInteract`（两者只差箱子那一支）。
/// 没人骑（Bamboo 还没有骑乘）、不是潜行开物品栏、也不是幼崽拿金蒲公英时：
/// 拿着食物就喂，没驯服就发脾气，箱子马没箱子时装箱子；其余交给 `AbstractHorse`。
///
/// `Horse.mobInteract` and `AbstractChestedHorse.mobInteract`, which differ only
/// by the chest branch: feed, get angry when untamed, or fit a chest; everything
/// else falls through to `AbstractHorse.mobInteract`.
pub(in crate::entity::mob) fn interact(
    me: &mut Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
) -> MobInteraction {
    let outcome = mob_interact(me, mob, req);
    if outcome.result.consumes_action() || req.held.is_empty() {
        return outcome;
    }
    // `Player.interactOn`：生物自己放过时，再让手里的物品 `interactLivingEntity`。
    // 马的 `canUseSlot` 恒真，所以幼马也能这样被套上鞍。
    // `Player.interactOn`: when the mob passes, the held stack gets
    // `interactLivingEntity`; `Horse.canUseSlot` is always true, so foals take saddles.
    if let Some(equippable) = equippable(&req.held)
        && equippable.equip_on_interact
        && equip_on_target(me, mob, &equippable, &req.held)
    {
        return success(true, req);
    }
    outcome
}

fn mob_interact(me: &mut Entity, mob: &mut Mob, req: &MobInteractRequest) -> MobInteraction {
    let open_inventory = !mob.is_baby() && tame(mob) && req.sneaking;
    // `isHolding(GOLDEN_DANDELION)` 看两只手；请求里只有这一只手的物品。
    // `isHolding` checks both hands; the request only carries this hand's stack.
    let baby_dandelion = mob.is_baby() && req.held.item == items::GOLDEN_DANDELION;
    if !open_inventory && !baby_dandelion && !req.held.is_empty() {
        if is_food(mob.mob_type, &req.held) {
            return fed_food(me, mob, req);
        }
        if !tame(mob) {
            life::make_mad(me, mob);
            return success(false, req);
        }
        if mob.mob_type != MobType::Horse && !chested(mob) && req.held.item == items::CHEST {
            equip_chest(me, mob);
            return success(true, req);
        }
    }
    abstract_horse_interact(me, mob, req)
}

/// `AbstractHorse.mobInteract`：幼崽交给 `Animal.mobInteract`（这里走不到食物那一支，
/// 结果是 `PASS`）；驯服后潜行开物品栏；手里的东西先试 `interactLivingEntity`
/// （鞍），再试身体护甲；最后 `doPlayerRide`。
///
/// `AbstractHorse.mobInteract`: babies go to `Animal.mobInteract` (never with
/// food here, so `PASS`); sneaking opens the inventory once tamed; the held stack
/// tries `interactLivingEntity` (saddles) and then body armor; finally `doPlayerRide`.
fn abstract_horse_interact(
    me: &mut Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
) -> MobInteraction {
    if mob.is_baby() {
        return MobInteraction::pass();
    }
    if tame(mob) && req.sneaking {
        // `openCustomInventoryScreen`：驯服了就开坐骑物品栏，由调用方去开菜单。
        // `openCustomInventoryScreen`: tamed mounts open their inventory; the caller opens the menu.
        return MobInteraction {
            open_inventory: super::inventory::can_open(mob),
            ..success(false, req)
        };
    }
    if !req.held.is_empty()
        && let Some(equippable) = equippable(&req.held)
    {
        if equippable.equip_on_interact && equip_on_target(me, mob, &equippable, &req.held) {
            return success(true, req);
        }
        if slot_of(&equippable) == Some(EquipmentSlot::Body)
            && can_equip(mob, &equippable)
            && !wearing(mob, EquipmentSlot::Body)
        {
            equip(me, mob, EquipmentSlot::Body, &equippable, &req.held);
            return success(true, req);
        }
    }
    do_player_ride(mob);
    success(false, req)
}

/// `doPlayerRide`：停止吃草、清掉扬蹄，然后 `player.startRiding(this)`。
/// 骑乘还没有，所以只落前两步。
///
/// `doPlayerRide`: stop grazing, clear rearing, then `startRiding`; only the
/// first two steps happen while riding is missing.
fn do_player_ride(mob: &mut Mob) {
    if let Some(state) = &mut mob.equine {
        state.eating = false;
        state.standing = false;
        state.stand_counter = 0;
    }
}

/// `isFood`：马、驴、骡看 `horse_food`，羊驼看 `llama_food`。
///
/// `isFood`: `horse_food` for horses, donkeys and mules, `llama_food` for llamas.
fn is_food(kind: MobType, stack: &ItemStack) -> bool {
    let tag = if kind == MobType::Llama {
        item_tags::LLAMA_FOOD
    } else {
        item_tags::HORSE_FOOD
    };
    item_is_tagged(stack.item, tag)
}

/// `AbstractHorse.fedFood`：吃下去就扣一个、`SUCCESS_SERVER`，否则服务端 `PASS`。
///
/// `fedFood`: consume one and return `SUCCESS_SERVER`, otherwise `PASS` on the server.
fn fed_food(me: &mut Entity, mob: &mut Mob, req: &MobInteractRequest) -> MobInteraction {
    if !handle_eating(me, mob, req) {
        return MobInteraction::pass();
    }
    MobInteraction {
        result: InteractionResult::SuccessServer,
        shrink_held: !req.creative,
        replacement: None,
        open_inventory: false,
    }
}

/// 一种食物的效果：回血、长大秒数、脾气，以及能不能让驯服的成年个体发情。
///
/// One food's effect: heal, growth seconds, temper and whether it starts love.
struct Food {
    heal: f32,
    age_up: i32,
    temper: i32,
    love: bool,
}

const fn food(heal: f32, age_up: i32, temper: i32, love: bool) -> Option<Food> {
    Some(Food {
        heal,
        age_up,
        temper,
        love,
    })
}

/// `AbstractHorse.handleEating` 和 `Llama.handleEating` 的物品表。
///
/// The item tables of `AbstractHorse.handleEating` and `Llama.handleEating`.
fn food_of(kind: MobType, item: i32) -> Option<Food> {
    if kind == MobType::Llama {
        return match item {
            items::WHEAT => food(2.0, 10, 3, false),
            items::HAY_BLOCK => food(10.0, 90, 6, true),
            _ => None,
        };
    }
    match item {
        items::WHEAT => food(2.0, 20, 3, false),
        items::SUGAR => food(1.0, 30, 3, false),
        items::HAY_BLOCK => food(20.0, 180, 0, false),
        items::APPLE | items::CARROT => food(3.0, 60, 3, false),
        items::RED_MUSHROOM => food(3.0, 0, 3, false),
        items::GOLDEN_CARROT => food(4.0, 60, 5, true),
        items::GOLDEN_APPLE | items::ENCHANTED_GOLDEN_APPLE => food(10.0, 240, 10, true),
        _ => None,
    }
}

/// `handleEating`：先看发情，再回血、长大（服务端也照取三次粒子坐标）、加脾气；
/// 吃了就出声。马还会张嘴（`eating()`），羊驼只出声。
///
/// `handleEating`: love first, then heal, grow (the three particle coordinates
/// are drawn on the server too) and temper; eating plays a sound. Horses also
/// open their mouth (`eating()`), llamas only play the sound.
fn handle_eating(me: &mut Entity, mob: &mut Mob, req: &MobInteractRequest) -> bool {
    let Some(food) = food_of(mob.mob_type, req.held.item) else {
        return false;
    };
    let tame = tame(mob);
    let mut used = food.love && tame && mob.age == 0 && mob.in_love <= 0;
    if used {
        // `Animal.setInLove`：600 刻、记住玩家、广播爱心事件 18。
        // `Animal.setInLove`: 600 ticks, remember the player, broadcast event 18.
        mob.in_love = 600;
        mob.love_cause = Some(req.player_uuid);
        mob.events.push(MobEvent::Hearts);
    }
    if mob.health
        < mob
            .attributes
            .value(mc_bamboo_data::registries::attribute::MAX_HEALTH) as f32
        && food.heal > 0.0
    {
        life::heal(mob, food.heal);
        used = true;
    }
    if mob.is_baby() && food.age_up > 0 && !mob.age_locked {
        for _ in 0..3 {
            me.random.next_double();
        }
        age_up(me, mob, food.age_up, false);
        used = true;
    }
    let max_temper = max_temper(mob.mob_type);
    if let Some(state) = &mut mob.equine
        && food.temper > 0
        && (used || !tame)
        && state.temper < max_temper
    {
        // `modifyTemper`：夹到 `[0, getMaxTemper()]`。
        // `modifyTemper` clamps to `[0, getMaxTemper()]`.
        state.temper = state
            .temper
            .saturating_add(food.temper)
            .clamp(0, max_temper);
        used = true;
    }
    if used {
        eat_sound(me, mob);
    }
    used
}

/// `getMaxTemper`：马、驴、骡 100，羊驼 30。
///
/// `getMaxTemper`: 100 for horses, donkeys and mules, 30 for llamas.
const fn max_temper(kind: MobType) -> i32 {
    if matches!(kind, MobType::Llama) {
        30
    } else {
        100
    }
}

/// `AbstractHorse.eating`（张嘴再出声）或 `Llama.handleEating` 末尾（只出声）。
/// 静音时官方不取音高。
///
/// `AbstractHorse.eating` (open the mouth, then the sound) or the end of
/// `Llama.handleEating` (sound only). Silent mobs draw no pitch.
fn eat_sound(me: &mut Entity, mob: &mut Mob) {
    if mob.mob_type != MobType::Llama {
        life::open_mouth(mob);
    }
    if mob.silent {
        return;
    }
    let sound = life::voice(mob, life::Voice::Eat);
    let pitch = 1.0 + (me.random.next_float() - me.random.next_float()) * 0.2;
    living::play_sound(me, mob, sound, 1.0, pitch);
}

/// `AbstractChestedHorse.equipChest`：装上箱子、出声（先取音高）、建好空物品栏。
///
/// `equipChest`: fit the chest, play the sound (pitch drawn first), build the inventory.
fn equip_chest(me: &mut Entity, mob: &mut Mob) {
    let kind = mob.mob_type;
    let state = mob.equine.as_mut().expect("chested equine state");
    state.chested = true;
    state.chest_items = vec![ItemStack::empty(); chest_size(kind, state.strength)];
    let sound = match kind {
        MobType::Donkey => "minecraft:entity.donkey.chest",
        MobType::Mule => "minecraft:entity.mule.chest",
        _ => "minecraft:entity.llama.chest",
    };
    let pitch = (me.random.next_float() - me.random.next_float()) * 0.2 + 1.0;
    living::play_sound(me, mob, sound, 1.0, pitch);
}

pub(super) fn equippable(stack: &ItemStack) -> Option<Equippable> {
    match defaults::resolve(stack.item, &stack.patch(), kinds::EQUIPPABLE) {
        Some(ComponentValue::Equippable(value)) => Some(value.clone()),
        _ => None,
    }
}

/// 组件里的槽位 ID（`EquipmentSlot.id`）：6 是身体，7 是鞍；马科只用这两个。
///
/// The component slot id: 6 is body and 7 is saddle, the only two equines use.
pub(super) fn slot_of(equippable: &Equippable) -> Option<EquipmentSlot> {
    match equippable.slot {
        6 => Some(EquipmentSlot::Body),
        7 => Some(EquipmentSlot::Saddle),
        _ => None,
    }
}

fn wearing(mob: &Mob, slot: EquipmentSlot) -> bool {
    !mob.equipment.get(slot).is_empty()
}

/// `LivingEntity.isEquippableInSlot` 对马科的身体和鞍两格：先 `canUseSlot`，再看允许的实体类型。
///
/// `isEquippableInSlot` for the body and saddle slots: `canUseSlot`, then the
/// allowed entity types.
fn can_equip(mob: &Mob, equippable: &Equippable) -> bool {
    slot_of(equippable).is_some_and(|slot| can_use_slot(mob, slot))
        && crate::dispense::equippable_allows(
            equippable.allowed_entities.as_ref(),
            mob.mob_type.entity_type(),
        )
}

/// `canUseSlot`：马和羊驼覆写成恒真；驴和骡用 `AbstractHorse` 的版本，鞍位要活着、
/// 成年、驯服，其余格子恒真。
///
/// `canUseSlot`: always true for horses and llamas; donkeys and mules need an
/// alive, adult, tamed mount for the saddle slot only.
pub(super) fn can_use_slot(mob: &Mob, slot: EquipmentSlot) -> bool {
    if matches!(mob.mob_type, MobType::Horse | MobType::Llama) || slot != EquipmentSlot::Saddle {
        return true;
    }
    !mob.is_dead_or_dying() && !mob.is_baby() && tame(mob)
}

/// `Equippable.equipOnTarget`：能穿、那一格空着、活着才穿上一个，掉落改成必掉。
///
/// `Equippable.equipOnTarget`: equip one if allowed, the slot is free and the
/// mount is alive, and make the slot a guaranteed drop.
fn equip_on_target(
    me: &mut Entity,
    mob: &mut Mob,
    equippable: &Equippable,
    held: &ItemStack,
) -> bool {
    let Some(slot) = slot_of(equippable) else {
        return false;
    };
    if !can_equip(mob, equippable) || wearing(mob, slot) || !living::is_alive(me, mob) {
        return false;
    }
    equip(me, mob, slot, equippable, held);
    true
}

/// `setItemSlot` + `setGuaranteedDrop`。`onEquipItem` 的装备音效用生物随机源取
/// `nextLong` 当种子；鞍位的声音被 `AbstractHorse.getEquipSound` 换成 `HORSE_SADDLE`。
///
/// `setItemSlot` plus `setGuaranteedDrop`. The `onEquipItem` sound draws a
/// `nextLong` seed from the mob random; the saddle slot plays `HORSE_SADDLE`.
fn equip(
    me: &mut Entity,
    mob: &mut Mob,
    slot: EquipmentSlot,
    equippable: &Equippable,
    held: &ItemStack,
) {
    let mut one = held.clone();
    one.count = 1;
    mob.equipment.set(slot, one);
    mob.equipment.set_drop_chance(slot, GUARANTEED_DROP_CHANCE);
    play_equip_sound(me, mob, slot, equippable);
}

/// `LivingEntity.onEquipItem`：新旧不是同一物品同一组件、新物品的可装备组件正好指向
/// 这一格、生物不静音时，出装备声并取 `nextLong` 当种子。`firstTick` 不建模（见疑点）。
///
/// `LivingEntity.onEquipItem`: when the stacks differ, the new stack's equippable
/// component names this slot and the mob is not silent, play the equip sound with
/// a `nextLong` seed. `firstTick` is not modelled.
pub(super) fn on_equip_item(
    me: &mut Entity,
    mob: &mut Mob,
    slot: EquipmentSlot,
    old: &ItemStack,
    new: &ItemStack,
) {
    if old.is_same_item_same_components(new) {
        return;
    }
    if let Some(equippable) = equippable(new)
        && slot_of(&equippable) == Some(slot)
    {
        play_equip_sound(me, mob, slot, &equippable);
    }
}

/// `onEquipItem` 的发声部分；鞍位的声音被 `AbstractHorse.getEquipSound` 换成 `HORSE_SADDLE`。
///
/// The sound half of `onEquipItem`; `AbstractHorse.getEquipSound` swaps in
/// `HORSE_SADDLE` for the saddle slot.
fn play_equip_sound(me: &mut Entity, mob: &mut Mob, slot: EquipmentSlot, equippable: &Equippable) {
    if mob.silent {
        return;
    }
    let sound = if slot == EquipmentSlot::Saddle {
        Some("minecraft:entity.horse.saddle")
    } else {
        equip_sound(equippable)
    };
    let _seed = me.random.next_long();
    if let Some(sound) = sound {
        living::play_sound(me, mob, sound, 1.0, 1.0);
    }
}

fn equip_sound(equippable: &Equippable) -> Option<&'static str> {
    match &equippable.equip_sound {
        Holder::Id(id) => usize::try_from(*id)
            .ok()
            .and_then(|id| mc_bamboo_data::sounds::SOUND_EVENTS.get(id))
            .map(|sound| sound.name),
        Holder::Inline(_) => None,
    }
}
