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

use super::{Mob, MobEvent, living};
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
}

impl MobInteraction {
    /// `PASS`：什么都没发生。
    pub const fn pass() -> Self {
        Self {
            result: InteractionResult::Pass,
            shrink_held: false,
            replacement: None,
            open_inventory: false,
        }
    }
}

/// `AbstractCow.mobInteract`：`BUCKET` + 成年 → 挤奶；否则交给 `Animal.mobInteract`。
pub fn interact(me: &mut Entity, mob: &mut Mob, req: &MobInteractRequest) -> MobInteraction {
    if mob.mob_type.is_equine() {
        return super::equine::interact(me, mob, req);
    }
    let _ = me;
    if req.held.item == items::BUCKET && !mob.is_baby() {
        // `Player.playSound(COW_MILK, 1, 1)`：音源是玩家，位置取交互发生处。
        mob.events.push(MobEvent::MilkSound {
            position: req.position,
        });
        return MobInteraction {
            result: InteractionResult::Success,
            shrink_held: !req.creative,
            replacement: Some(ItemStack::new(items::MILK_BUCKET, 1)),
            open_inventory: false,
        };
    }
    let food = !req.held.is_empty() && item_is_tagged(req.held.item, item_tags::COW_FOOD);
    if !food {
        return MobInteraction::pass();
    }
    let age = mob.age;
    if age == 0 && mob.in_love <= 0 {
        // `Animal.setInLove(player)`：600 刻求爱、记住玩家 UUID、发爱心事件 18。
        mob.in_love = 600;
        mob.love_cause = Some(req.player_uuid);
        mob.events.push(MobEvent::Hearts);
        return MobInteraction {
            result: InteractionResult::SuccessServer,
            shrink_held: !req.creative,
            replacement: None,
            open_inventory: false,
        };
    }
    if mob.is_baby() && !mob.age_locked {
        // `getSpeedUpSecondsWhenFeeding(-age)`：int 除法先算，再乘 0.1F 截成 int。
        let seconds = (((-age) / 20) as f32 * 0.1_f32) as i32;
        age_up(me, mob, seconds, true);
        return MobInteraction {
            result: InteractionResult::Success,
            shrink_held: !req.creative,
            replacement: None,
            open_inventory: false,
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
