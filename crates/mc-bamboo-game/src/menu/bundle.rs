//! 菜单里的收纳袋点击：官方 `BundleItem.overrideStackedOnOther` /
//! `overrideOtherStackedOnMe`，由 [`super::do_click`] 在普通拾取逻辑之前调用。
//!
//! 光标拿收纳袋点槽里非空物品是「装进去」，空光标右键点收纳袋是「取出来」。
//! 两种方向、左右键和四种音效都在这一个文件里。
//!
//! Bundle clicks in a menu: vanilla `BundleItem.overrideStackedOnOther` /
//! `overrideOtherStackedOnMe`, called before the ordinary pickup logic.

use mc_bamboo_protocol::{components::bundle, item::ItemStack, packet::play::ContainerInput};

use super::Ctx;

/// 音效的音高：`0.8 + level.random.nextFloat() * 0.4`。
/// 音量是 `0.8`；失败音效固定 `1.0 / 1.0`，不取随机数。
///
/// `0.8 + level.random.nextFloat() * 0.4`; failures use `1.0 / 1.0` and draw nothing.
#[derive(Clone, Copy)]
pub(crate) enum Sound {
    Insert,
    InsertFail,
    RemoveOne,
    DropContents,
}

impl Sound {
    pub(crate) fn id(self) -> i32 {
        let name = match self {
            Self::Insert => "minecraft:item.bundle.insert",
            Self::InsertFail => "minecraft:item.bundle.insert_fail",
            Self::RemoveOne => "minecraft:item.bundle.remove_one",
            Self::DropContents => "minecraft:item.bundle.drop_contents",
        };
        i32::from(mc_bamboo_data::sound_event(name))
    }
}

/// 官方 `AbstractContainerMenu.tryItemClickBehaviourOverride` 的收纳袋部分。
///
/// 返回 `true` 表示这次点击已经处理完，不能再走普通的拾取 / 交换逻辑。
/// `sounds` 收集要播的音效，调用方按官方 `Entity.playSound` / `Level.playSound`
/// 的位置和音量发出去。
pub(crate) fn try_click(
    ctx: &mut Ctx<'_>,
    slot_index: i32,
    button: i32,
    input: ContainerInput,
    sounds: &mut Vec<Sound>,
) -> bool {
    if !matches!(input, ContainerInput::Pickup | ContainerInput::QuickMove)
        || !is_container_click_button(button)
    {
        return false;
    }
    let Some(index) = super::positive_index(ctx, slot_index) else {
        return false;
    };
    let primary = button == 0;
    let clicked = ctx.get(index);
    let carried = ctx.carried.clone();
    // 官方先试「光标上的物品 overrideStackedOnOther(slot)」，
    // 再试「槽里的物品 overrideOtherStackedOnMe(carried, slot, carriedSlotAccess)」。
    // `ItemStack.overrideStackedOnOther` 由物品类分派：只有收纳袋物品才覆盖。
    // `ItemStack.overrideOtherStackedOnMe` 同理。
    if bundle::is_bundle_item(carried.item)
        && bundle::contents(&carried).is_some()
        && stacked_on_other(ctx, index, &carried, &clicked, primary, sounds)
    {
        return true;
    }
    if bundle::is_bundle_item(clicked.item) && bundle::contents(&clicked).is_some() {
        return other_stacked_on_me(ctx, index, &clicked, &carried, primary, sounds);
    }
    false
}

/// 官方 `BundleItem.overrideStackedOnOther`：光标上是收纳袋，点的槽里有东西时装进去，
/// 点的槽是空的时右键取出一件放回那个槽。
fn stacked_on_other(
    ctx: &mut Ctx<'_>,
    index: usize,
    carried: &ItemStack,
    clicked: &ItemStack,
    primary: bool,
    sounds: &mut Vec<Sound>,
) -> bool {
    let mut bag = carried.clone();
    let Some(contents) = bundle::contents(&bag) else {
        return false;
    };
    if primary && !clicked.is_empty() {
        let mut slot = clicked.clone();
        let inserted = bundle::try_transfer(&mut bag, &mut slot);
        sounds.push(if inserted > 0 {
            Sound::Insert
        } else {
            Sound::InsertFail
        });
        ctx.set(index, slot);
        *ctx.carried = bag;
        return true;
    }
    if !primary && clicked.is_empty() {
        if contents.is_empty() {
            return false;
        }
        let taken = bundle::remove_one(&mut bag);
        if let Some(taken) = taken {
            let amount = taken.count;
            let leftover = ctx.safe_insert(index, taken, amount);
            if leftover.count > 0 {
                // `Slot.safeInsert` 没放完的部分按官方重新装回袋里（`addFirst`）。
                let mut remaining = leftover;
                bundle::try_insert(&mut bag, &mut remaining);
            } else {
                sounds.push(Sound::RemoveOne);
            }
        }
        *ctx.carried = bag;
        return true;
    }
    false
}

/// 官方 `BundleItem.overrideOtherStackedOnMe`：槽里是收纳袋。
/// 左键空光标时清掉选中格并返回 `false`（普通拾取会把它拿到光标上）；
/// 左键有物品时装进袋里；右键空光标时取一件放到光标上。
fn other_stacked_on_me(
    ctx: &mut Ctx<'_>,
    index: usize,
    clicked: &ItemStack,
    carried: &ItemStack,
    primary: bool,
    sounds: &mut Vec<Sound>,
) -> bool {
    let mut bag = clicked.clone();
    if primary && carried.is_empty() {
        bundle::toggle_selected(&mut bag, -1);
        ctx.set(index, bag);
        return false;
    }
    if primary && !carried.is_empty() {
        if ctx.may_place(index, carried) {
            let mut input = carried.clone();
            let inserted = bundle::try_insert(&mut bag, &mut input);
            sounds.push(if inserted > 0 {
                Sound::Insert
            } else {
                Sound::InsertFail
            });
            *ctx.carried = input;
        } else {
            sounds.push(Sound::InsertFail);
        }
        ctx.set(index, bag);
        return true;
    }
    if !primary && carried.is_empty() {
        if ctx.may_place(index, &ItemStack::empty())
            && let Some(taken) = bundle::remove_one(&mut bag)
        {
            sounds.push(Sound::RemoveOne);
            *ctx.carried = taken;
        }
        ctx.set(index, bag);
        return true;
    }
    // 其它组合（比如右键时光标上有东西）：官方 `toggleSelectedItem(-1)` 后返回 `false`。
    bundle::toggle_selected(&mut bag, -1);
    ctx.set(index, bag);
    false
}

fn is_container_click_button(button: i32) -> bool {
    button == 0 || button == 1
}

/// 官方音效的随机音高。`level.random.nextFloat()` 由调用方给出。
pub(crate) fn pitch(kind: Sound, roll: f32) -> f32 {
    if matches!(kind, Sound::InsertFail) {
        1.0
    } else {
        0.8 + roll * 0.4
    }
}

/// 收纳袋的 `use` 时长，官方 `BundleItem.getUseDuration`。
pub(crate) const USE_DURATION: i32 = 200;

/// 官方 `BundleItem.onUseTick`：第一刻倒一次，之后剩余小于 `USE_DURATION - 10`
/// 且为偶数的刻每刻倒一次。
pub(crate) fn should_drop(remaining: i32) -> bool {
    remaining == USE_DURATION || (remaining < USE_DURATION - 10 && remaining % 2 == 0)
}

/// 官方 `BundleItem.dropContent` 的装袋改动：成功倒出一件时返回掉出来的物品。
pub(crate) fn drop_one(bag: &mut ItemStack) -> Option<ItemStack> {
    if bundle::contents(bag).is_some_and(|items| items.is_empty()) {
        return None;
    }
    let taken = bundle::remove_one(bag)?;
    Some(taken)
}

/// 官方 `Entity.playSound`：以玩家自己的位置为原点，`PLAYERS` 音源，音高从
/// `level.random` 取一次 `nextFloat`。失败音效固定音高，不取随机数。
pub(crate) fn send_sounds(
    region: &mut crate::region::Region,
    id: crate::inbound::PlayerId,
    sounds: &[Sound],
) {
    if sounds.is_empty() {
        return;
    }
    let Some(position) = region.players.get(&id).map(|player| player.position()) else {
        return;
    };
    for &kind in sounds {
        let (volume, pitch) = if matches!(kind, Sound::InsertFail) {
            (1.0, 1.0)
        } else {
            let roll = region.sound_seed_generator.source().next_float();
            (0.8, pitch(kind, roll))
        };
        let seed = region.sound_seed_generator.source().next_long();
        let packet = mc_bamboo_protocol::packet::play::Sound {
            sound: kind.id(),
            inline: None,
            source: mc_bamboo_data::sounds::PLAYERS,
            x: position.x,
            y: position.y,
            z: position.z,
            volume,
            pitch,
            seed,
        };
        crate::interaction::send_sound(region, None, &packet);
    }
}

/// 玩家物品栏 / 工作站菜单里的收纳袋点击，和 [`try_click`] 同规则。
/// 官方的 `InventoryMenu` 也是 `AbstractContainerMenu`，同一条 `doClick` 路径。
pub(crate) fn try_station_click(
    station: &mut super::station::Station,
    slot: i32,
    button: i32,
    input: ContainerInput,
) -> bool {
    if input != ContainerInput::Pickup || !matches!(button, 0 | 1) || slot < 0 {
        return false;
    }
    let index = match usize::try_from(slot) {
        Ok(index) if station.has_slot(index) => index,
        _ => return false,
    };
    let clicked = station.slot_item(index);
    let carried = station.carried().clone();
    let primary = button == 0;
    if bundle::is_bundle_item(carried.item) && bundle::contents(&carried).is_some() {
        let mut bag = carried.clone();
        let Some(contents) = bundle::contents(&bag) else {
            return false;
        };
        if primary && !clicked.is_empty() {
            let mut slot_stack = clicked;
            let inserted = bundle::try_transfer(&mut bag, &mut slot_stack);
            station.set_slot(index, slot_stack);
            station.set_carried(bag);
            station.note_bundle_sound(if inserted > 0 {
                Sound::Insert
            } else {
                Sound::InsertFail
            });
            return true;
        }
        if !primary && clicked.is_empty() && !contents.is_empty() {
            if let Some(taken) = bundle::remove_one(&mut bag) {
                let amount = taken.count;
                let leftover = station.safe_insert_item(index, taken, amount);
                if leftover.count > 0 {
                    let mut remaining = leftover;
                    bundle::try_insert(&mut bag, &mut remaining);
                } else {
                    station.note_bundle_sound(Sound::RemoveOne);
                }
            }
            station.set_carried(bag);
            return true;
        }
    }
    if bundle::is_bundle_item(clicked.item) && bundle::contents(&clicked).is_some() {
        let mut bag = clicked;
        if primary && carried.is_empty() {
            bundle::toggle_selected(&mut bag, -1);
            station.set_slot(index, bag);
            return false;
        }
        if primary && !carried.is_empty() {
            let mut input_stack = carried.clone();
            let inserted = if station.may_place_item(index, &input_stack) {
                bundle::try_insert(&mut bag, &mut input_stack)
            } else {
                0
            };
            station.set_carried(input_stack);
            station.set_slot(index, bag);
            station.note_bundle_sound(if inserted > 0 {
                Sound::Insert
            } else {
                Sound::InsertFail
            });
            return true;
        }
        if !primary && carried.is_empty() {
            if station.may_place_item(index, &ItemStack::empty())
                && let Some(taken) = bundle::remove_one(&mut bag)
            {
                station.set_carried(taken);
                station.note_bundle_sound(Sound::RemoveOne);
            }
            station.set_slot(index, bag);
            return true;
        }
        bundle::toggle_selected(&mut bag, -1);
        station.set_slot(index, bag);
    }
    false
}
