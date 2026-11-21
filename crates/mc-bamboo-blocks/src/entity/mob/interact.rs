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

use super::{Mob, MobEvent, MobType, chicken, living, pig, sheep};
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
}

/// 交互的后果，交给调用方改物品栏和发挥手动画。
#[derive(Debug, Clone, PartialEq)]
pub struct MobInteraction {
    pub result: InteractionResult,
    /// 生存模式从手里拿走一个（小麦、空桶）；创造模式不扣。
    pub shrink_held: bool,
    /// `ItemUtils.createFilledResult` 交回来的奶桶。背包满时调用方丢在地上。
    pub replacement: Option<ItemStack>,
    /// `Sheep.shear`：调用方按剪羊毛战利品表掉落，并给剪刀扣 1 点耐久。
    ///
    /// `Sheep.shear`: the caller drops the shearing loot and spends one durability point.
    pub shear: bool,
}

impl MobInteraction {
    /// `PASS`：什么都没发生。
    pub const fn pass() -> Self {
        Self {
            result: InteractionResult::Pass,
            shrink_held: false,
            replacement: None,
            shear: false,
        }
    }
}

pub(super) fn is_food(kind: MobType, item: i32) -> bool {
    match kind {
        MobType::Cow => item_is_tagged(item, item_tags::COW_FOOD),
        MobType::Pig => item_is_tagged(item, item_tags::PIG_FOOD),
        MobType::Sheep => sheep::is_food(item),
        MobType::Chicken => chicken::is_food(item),
        _ => false,
    }
}

/// `AbstractCow.mobInteract`：`BUCKET` + 成年 → 挤奶；否则交给 `Animal.mobInteract`。
pub fn interact(me: &mut Entity, mob: &mut Mob, req: &MobInteractRequest) -> MobInteraction {
    if mob.mob_type == MobType::Cow && req.held.item == items::BUCKET && !mob.is_baby() {
        // `Player.playSound(COW_MILK, 1, 1)`：音源是玩家，位置取交互发生处。
        mob.events.push(MobEvent::MilkSound {
            position: req.position,
        });
        return MobInteraction {
            result: InteractionResult::Success,
            shrink_held: !req.creative,
            replacement: Some(ItemStack::new(items::MILK_BUCKET, 1)),
            shear: false,
        };
    }
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
    if age == 0 && mob.in_love <= 0 {
        // `Animal.setInLove(player)`：600 刻求爱、记住玩家 UUID、发爱心事件 18。
        mob.in_love = 600;
        mob.love_cause = Some(req.player_uuid);
        mob.events.push(MobEvent::Hearts);
        if mob.mob_type == MobType::Pig {
            pig::play_eating_sound(me, mob);
        }
        return MobInteraction {
            result: InteractionResult::SuccessServer,
            shrink_held: !req.creative,
            replacement: None,
            shear: false,
        };
    }
    if mob.is_baby() && !mob.age_locked {
        // `getSpeedUpSecondsWhenFeeding(-age)`：int 除法先算，再乘 0.1F 截成 int。
        let seconds = (((-age) / 20) as f32 * 0.1_f32) as i32;
        age_up(me, mob, seconds, true);
        if mob.mob_type == MobType::Pig {
            pig::play_eating_sound(me, mob);
        }
        return MobInteraction {
            result: InteractionResult::Success,
            shrink_held: !req.creative,
            replacement: None,
            shear: false,
        };
    }
    // 官方只有客户端这一支返回 `CONSUME`，服务端返回 `PASS`。
    MobInteraction::pass()
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
