//! 牛的右键交互：官方 `AbstractCow.mobInteract` → `Animal.mobInteract` 和
//! `AgeableMob.mobInteract`，以及 `AgeableMob.ageUp`。
//!
//! 只改生物自己的状态；玩家物品栏怎么改由调用方按 [`MobInteraction`] 落地。
//! 这样游戏路径和逐刻对拍走的是同一个函数。
//!
//! Cow right-click interactions: vanilla `AbstractCow.mobInteract` through
//! `Animal.mobInteract` and `AgeableMob.mobInteract`, plus `AgeableMob.ageUp`.
//! Only the mob changes here; the caller applies [`MobInteraction`] to the
//! player's inventory. The live path and the oracle share this function.

use mc_bamboo_core::{Hand, Vec3};
use mc_bamboo_data::{items, tags::item_tags};
use mc_bamboo_protocol::item::ItemStack;

use super::{Mob, MobEvent, MobType, chicken, living, mooshroom, pig, sheep};
use crate::entity::Entity;
use crate::{InteractionResult, state::item_is_tagged};

/// 一次右键交互的输入。
#[derive(Debug, Clone, PartialEq)]
pub struct MobInteractRequest {
    pub player_uid: i32,
    /// `EntityReference.of(player)` 存的是玩家 UUID，存档里的 `LoveCause`。
    pub player_uuid: [u8; 16],
    pub hand: Hand,
    /// 玩家手里那一堆（用之前已经复制好）。
    pub held: ItemStack,
    /// 创造模式不扣物品；`ItemUtils.createFilledResult` 也看这个。
    pub creative: bool,
    /// 玩家位置，`Player.playSound` 用它当音源坐标。
    pub position: Vec3,
    /// `isSecondaryUseActive()`：交互包里的 `usingSecondaryAction`（潜行）。
    ///
    /// `isSecondaryUseActive()`: the packet's `usingSecondaryAction` (sneaking).
    pub sneaking: bool,
}

/// 交互的后果，交给调用方改物品栏和发挥手动画。
#[derive(Debug, Clone, PartialEq)]
pub struct MobInteraction {
    pub result: InteractionResult,
    /// 生存模式从手里拿走一个（小麦、空桶）；创造模式不扣。
    pub shrink_held: bool,
    /// `ItemUtils.createFilledResult` 交回来的奶桶。背包满时调用方丢在地上。
    pub replacement: Option<ItemStack>,
    /// 马科 `openCustomInventoryScreen`：调用方给这名玩家打开坐骑物品栏。
    ///
    /// Equine `openCustomInventoryScreen`: the caller opens the mount inventory
    /// for this player.
    pub open_inventory: bool,
    /// `Sheep.shear`：调用方按剪羊毛战利品表掉落，并给剪刀扣 1 点耐久；对犰狳是
    /// `brushOffScute`，按刷子战利品表掉落并扣 16 点耐久。
    ///
    /// `Sheep.shear`: the caller drops the shearing loot and spends one durability point;
    /// for armadillos it is `brushOffScute`, dropping the brush loot and spending 16.
    pub shear: bool,
    /// `AbstractHorse.doPlayerRide`：调用方把这名玩家放到这只生物身上。
    ///
    /// `AbstractHorse.doPlayerRide`: the caller puts this player on the mob.
    pub ride: bool,
    /// `ItemStack.hurtAndBreak(1, player, slot)`：调用方给手里的物品扣 1 点耐久（打火石点苦力怕）。
    ///
    /// `ItemStack.hurtAndBreak(1, player, slot)`: the caller spends one durability point on
    /// the held item (flint and steel on a creeper).
    pub damage_held: bool,
    /// `Bucketable.bucketMobPickup`：调用方把这只生物从世界里拿掉。
    ///
    /// `Bucketable.bucketMobPickup`: the caller discards the mob.
    pub discard: bool,
    /// `ItemUtils.createFilledResult` 的 `limitCreativeStackSize`。哞菇炖菜和鱼桶拾起传假：
    /// 创造模式也消耗手里的水桶。
    ///
    /// `limitCreativeStackSize` of `ItemUtils.createFilledResult`: false for mooshroom stew
    /// and for bucketing a fish, so creative mode consumes the bucket too.
    pub limit_creative: bool,
    /// `spawnAtLocation`：熊猫换手里那份竹子或剪下马具时丢下的物品。
    ///
    /// `spawnAtLocation`: a stack dropped on interaction (e.g. a panda swapping
    /// bamboo or shearing worn equipment off a mount).
    pub drop_stack: Option<ItemStack>,
    /// `spawnAtLocation` 的相对偏移（默认脚下；剪下装备用 `PASSENGER` 附着点均值）。
    ///
    /// Offset added to the mob's position for `drop_stack` (zero by default;
    /// sheared equipment uses the average `PASSENGER` attachment point).
    pub drop_offset: mc_bamboo_core::Vec3,
}

impl MobInteraction {
    /// `PASS`：什么都没发生。
    pub const fn pass() -> Self {
        Self {
            result: InteractionResult::Pass,
            shrink_held: false,
            replacement: None,
            open_inventory: false,
            shear: false,
            ride: false,
            damage_held: false,
            discard: false,
            limit_creative: true,
            drop_stack: None,
            drop_offset: mc_bamboo_core::Vec3::new(0.0, 0.0, 0.0),
        }
    }
}

/// `Animal.isFood`：各种动物认的食物。
///
/// `Animal.isFood` per species.
pub(super) fn is_food(kind: MobType, item: i32) -> bool {
    match kind {
        MobType::Cow | MobType::Mooshroom => item_is_tagged(item, item_tags::COW_FOOD),
        MobType::Pig => item_is_tagged(item, mc_bamboo_data::tags::item_tags::PIG_FOOD),
        MobType::Sheep => sheep::is_food(item),
        MobType::Chicken => chicken::is_food(item),
        MobType::Llama => item_is_tagged(item, mc_bamboo_data::tags::item_tags::LLAMA_FOOD),
        kind if kind.is_equine() => {
            item_is_tagged(item, mc_bamboo_data::tags::item_tags::HORSE_FOOD)
        }
        MobType::Panda => super::panda::is_food(item),
        _ => kind.food_tag().is_some_and(|tag| item_is_tagged(item, tag)),
    }
}

/// `AbstractCow.mobInteract`：`BUCKET` + 成年 → 挤奶；狼走 `Wolf.mobInteract`；
/// 马科走 `equine::interact`；否则交给 `Animal.mobInteract`。`now` 是岛时钟
/// （狼要看还在不在生气），`thundering` 给熊猫的害怕判定用。
///
/// `AbstractCow.mobInteract`, `Wolf.mobInteract`, `equine::interact`, or
/// `Animal.mobInteract`. `now` is the island clock, read by the wolf's anger check;
/// `thundering` feeds the panda's scared check.
pub fn interact(
    me: &mut Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
    now: i64,
    thundering: bool,
) -> MobInteraction {
    if mob.mob_type == MobType::Panda
        && let Some(outcome) =
            super::panda::interact(me, mob, req, super::panda::is_scared(mob, thundering))
    {
        return outcome;
    }
    // `AbstractFish.mobInteract`：先试 `Bucketable.bucketMobPickup`，成功就换桶并丢弃
    // 这只生物；失败才走 `super.mobInteract`（对鱼是 `PASS`）。
    // `AbstractFish.mobInteract`: try `Bucketable.bucketMobPickup` first, then fall
    // back to `super.mobInteract` (which is `PASS` for fish).
    if req.held.item == items::WATER_BUCKET
        && living::is_alive(me, mob)
        && let Some(bucket) = super::bucketable::pickup(me, mob)
    {
        return MobInteraction {
            result: InteractionResult::Success,
            replacement: Some(bucket),
            discard: true,
            limit_creative: false,
            ..MobInteraction::pass()
        };
    }
    if mob.mob_type == MobType::Tadpole {
        // 蝌蚪不是 `Animal`：自己的两支都不中就是 `Mob.mobInteract` 的 `PASS`。
        // Tadpoles are not `Animal`s: when neither of their own branches applies the
        // result is the `PASS` of `Mob.mobInteract`.
        return super::tadpole::interact(me, mob, req).unwrap_or_else(MobInteraction::pass);
    }
    if mob.mob_type == super::MobType::Wolf {
        return super::wolf::interact(me, mob, req, now);
    }
    if mob.mob_type == MobType::Armadillo
        && let Some(outcome) = super::armadillo::interact(me, mob, req)
    {
        return outcome;
    }
    if mob.mob_type == MobType::Goat
        && let Some(outcome) = super::goat::interact(mob, req)
    {
        return outcome;
    }
    if mob.mob_type.is_equine() {
        return super::equine::interact(me, mob, req);
    }
    // `Dolphin.mobInteract`：手里是 `#fishes` 时放吃鱼声，幼崽加速长大、成年记下
    // `gotFish`，并消费一个鱼。这是它自己的 `mobInteract`，不走 `Animal.mobInteract`。
    // `Dolphin.mobInteract`: a `#fishes` item plays the eat sound, ages a baby up or
    // sets `gotFish`, and consumes one fish. This is its own `mobInteract`, not
    // `Animal.mobInteract`.
    if mob.mob_type == MobType::Dolphin
        && !req.held.is_empty()
        && item_is_tagged(req.held.item, mc_bamboo_data::tags::item_tags::FISHES)
    {
        living::play_sound(me, mob, "minecraft:entity.dolphin.eat", 1.0, 1.0);
        super::dolphin::feed(me, mob);
        return MobInteraction {
            result: InteractionResult::Success,
            shrink_held: !req.creative,
            replacement: None,
            ..MobInteraction::pass()
        };
    }
    if mob.mob_type == MobType::Mooshroom
        && let Some(outcome) = mooshroom::interact(me, mob, req)
    {
        return outcome;
    }
    if matches!(mob.mob_type, MobType::Cow | MobType::Mooshroom)
        && req.held.item == items::BUCKET
        && !mob.is_baby()
    {
        // `Player.playSound(COW_MILK, 1, 1)`：音源是玩家，位置取交互发生处。
        mob.events.push(MobEvent::MilkSound {
            position: req.position,
        });
        return MobInteraction {
            result: InteractionResult::Success,
            shrink_held: !req.creative,
            replacement: Some(ItemStack::new(items::MILK_BUCKET, 1)),
            ..MobInteraction::pass()
        };
    }
    if mob.mob_type == MobType::Creeper {
        return super::creeper::interact(me, mob, req);
    }
    animal_interact(me, mob, req)
}

/// `Animal.mobInteract`：吃的能让成年动物发情、让幼崽长大。
///
/// `Animal.mobInteract`: food puts adults in love or ages babies up.
pub(super) fn animal_interact(
    me: &mut Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
) -> MobInteraction {
    if mob.mob_type == MobType::Sheep
        && let Some(outcome) = sheep::interact_shears(mob, req)
    {
        return outcome;
    }
    if mob.mob_type == MobType::Bogged {
        return super::bogged::interact(mob, req);
    }
    let food = !req.held.is_empty() && is_food(mob.mob_type, req.held.item);
    if !food {
        return match mob.mob_type {
            MobType::Pig => pig::equip_saddle(me, mob, req),
            MobType::Sheep => sheep::interact_dye(me, mob, req),
            _ => MobInteraction::pass(),
        };
    }
    let age = mob.age;
    // `Turtle.canFallInLove`：身上有蛋时不进求爱。
    // `Turtle.canFallInLove`: no love while carrying an egg.
    let can_fall_in_love = mob.mob_type != MobType::Turtle || !mob.turtle.has_egg;
    if age == 0 && mob.in_love <= 0 && can_fall_in_love {
        // `Animal.setInLove(player)`：600 刻求爱、记住玩家 UUID、发爱心事件 18。
        mob.in_love = 600;
        mob.love_cause = Some(req.player_uuid);
        mob.events.push(MobEvent::Hearts);
        play_eating_sound(me, mob);
        return MobInteraction {
            result: InteractionResult::SuccessServer,
            shrink_held: !req.creative,
            replacement: None,
            ..MobInteraction::pass()
        };
    }
    if mob.is_baby() && !mob.age_locked {
        // `getSpeedUpSecondsWhenFeeding(-age)`：int 除法先算，再乘 0.1F 截成 int。
        let seconds = (((-age) / 20) as f32 * 0.1_f32) as i32;
        age_up(me, mob, seconds, true);
        play_eating_sound(me, mob);
        return MobInteraction {
            result: InteractionResult::Success,
            shrink_held: !req.creative,
            replacement: None,
            ..MobInteraction::pass()
        };
    }
    // 官方只有客户端这一支返回 `CONSUME`，服务端返回 `PASS`。
    MobInteraction::pass()
}

/// `Animal.playEatingSound`：缺省不出声，猪、犰狳和青蛙各有自己的。
///
/// `Animal.playEatingSound`: silent by default; pigs, armadillos and frogs have their own.
fn play_eating_sound(me: &mut Entity, mob: &mut Mob) {
    match mob.mob_type {
        MobType::Pig => pig::play_eating_sound(me, mob),
        MobType::Armadillo => living::make_sound(me, mob, "minecraft:entity.armadillo.eat"),
        // `Frog.playEatingSound`：`level.playSound(null, this, FROG_EAT, NEUTRAL, 2.0F, 1.0F)`，
        // 不看 `isSilent`、不取音高。
        // `Frog.playEatingSound`: `level.playSound(null, this, FROG_EAT, NEUTRAL, 2.0F,
        // 1.0F)`, ignoring `isSilent` and drawing no pitch.
        MobType::Frog => mob.events.push(MobEvent::Sound {
            sound: "minecraft:entity.frog.eat",
            volume: 2.0,
            pitch: 1.0,
            position: me.position,
            hostile: false,
            seed: None,
        }),
        _ => {}
    }
}

/// `Shearable.shear` 的落地：按种类分给羊或哞菇。
///
/// Applies `Shearable.shear` for the sheep or the mooshroom.
pub(crate) fn apply_shear(
    world: &mut crate::world::World,
    entity: &mut Entity,
    hand: Hand,
    tool: &ItemStack,
) {
    match entity.kind.mob().map(|mob| mob.mob_type) {
        Some(MobType::Sheep) => sheep::apply_shear(world, entity, hand, tool),
        Some(MobType::Mooshroom) => mooshroom::apply_shear(world, entity, hand, tool),
        Some(MobType::Bogged) => super::bogged::apply_shear(world, entity, hand, tool),
        _ => {}
    }
}

/// `AgeableMob.ageUp(int, boolean)`：跨过 0 时换尺寸，forcedAge 和粒子计时一起推进。
pub fn age_up(me: &mut Entity, mob: &mut Mob, seconds: i32, force: bool) {
    let before = mob.age;
    let grown = (before + seconds * 20).min(0);
    let delta = grown - before;
    living::set_age(me, mob, grown);
    if force {
        mob.forced_age += delta;
        if mob.forced_age_timer == 0 {
            mob.forced_age_timer = 40;
        }
    }
    if mob.age == 0 {
        living::set_age(me, mob, mob.forced_age);
    }
}
