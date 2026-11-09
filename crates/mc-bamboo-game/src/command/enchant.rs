//! `/enchant`：给目标主手的物品加附魔。
//!
//! 附魔定义从 `mc-bamboo-loot` 的生成器数据里查，兼容性、最高等级和失败反馈
//! 按官方 `EnchantCommand` 的字节码。

use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_data::registries::data_component_type as kinds;
use mc_bamboo_loot::{enchantment_defs_all, enchantment_id};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry},
    packet::play::SetPlayerInventory,
    text::Component,
};

use crate::{
    command::{
        feedback::{self, CommandFail, fail, fail_with},
        players, select,
    },
    global::Global,
    inbound::PlayerId,
    region::Region,
};

pub(crate) fn enchant(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    let selector = parsed
        .get("targets")
        .and_then(Value::as_entity)
        .ok_or_else(|| fail("command.failed"))?;
    let targets = select::entities(region, id, players::level_of(region, id), selector)?;
    let name = parsed
        .get("enchantment")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("command.failed"))?;
    let level = parsed.get("level").and_then(Value::as_integer).unwrap_or(1);
    let Some(id_in_registry) = enchantment_id(name) else {
        return Err(fail("argument.resource.not_found"));
    };
    let Some(def) = enchantment_defs_all().iter().find(|def| def.name == name) else {
        return Err(fail("argument.resource.not_found"));
    };
    if level > def.max_level {
        return Err(fail_with(
            "commands.enchant.failed.level",
            [level.into(), def.max_level.into()],
        ));
    }

    let mut changed = 0usize;
    let mut single_name = String::new();
    let single = targets.len() == 1;
    for target in targets {
        let Some(player) = region.players.get_mut(&target) else {
            continue;
        };
        let held = player.inventory.held(mc_bamboo_core::Hand::Main);
        if held.is_empty() {
            if single {
                return Err(fail_with(
                    "commands.enchant.failed.itemless",
                    [player.name.clone().into()],
                ));
            }
            continue;
        }
        // 官方 `canEnchant` 检查物品是否在 supported_items 里；`EnchantmentHelper`
        // 还检查已有附魔和 exclusive_set。
        let stack_item = held.item;
        let mut patch = held.patch();
        let compatible = def.supported_items.contains(&stack_item)
            && enchantments_compatible(&patch, id_in_registry, def.exclusive_set.as_deref());
        if !compatible {
            if single {
                return Err(fail_with(
                    "commands.enchant.failed.incompatible",
                    [mc_bamboo_data::items::ITEMS
                        .get(stack_item as usize)
                        .map(|item| item.name)
                        .unwrap_or("item")
                        .into()],
                ));
            }
            continue;
        }
        let mut levels = patch
            .get(kinds::ENCHANTMENTS)
            .and_then(|value| match value {
                ComponentValue::Enchantments(levels) => Some(levels.clone()),
                _ => None,
            })
            .unwrap_or_default();
        match levels
            .iter_mut()
            .find(|(existing, _)| *existing == id_in_registry)
        {
            Some((_, value)) => *value = level,
            None => levels.push((id_in_registry, level)),
        }
        patch.put(
            kinds::ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(levels)),
        );
        let (added, removed) = patch.to_opaque();
        let slot_index = player.inventory.hand_slot(mc_bamboo_core::Hand::Main);
        let held = &mut player.inventory.slots[slot_index];
        held.added = added;
        held.removed = removed;
        let stack = held.clone();
        let slot = i32::from(player.inventory.selected);
        region.links.send(
            target,
            &SetPlayerInventory {
                slot,
                stack: &stack,
            },
        );
        single_name = player.name.clone();
        changed += 1;
    }
    if changed == 0 {
        return Err(fail("commands.enchant.failed"));
    }
    let full_name = Component::translate(
        format!("enchantment.{}.{}", namespace(name), path(name)),
        [],
    );
    let message = if single {
        Component::translate(
            "commands.enchant.success.single",
            [full_name.into(), Component::text(single_name).into()],
        )
    } else {
        Component::translate(
            "commands.enchant.success.multiple",
            [full_name.into(), (changed as i32).into()],
        )
    };
    feedback::success(region, id, message, true);
    Ok(())
}

fn namespace(name: &str) -> &str {
    name.split_once(':').map_or("minecraft", |(ns, _)| ns)
}

fn path(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, path)| path)
}

/// 官方 `isEnchantmentCompatible`：同一条附魔可以升级，exclusive_set 里的不能共存。
fn enchantments_compatible(patch: &ComponentPatch, id: i32, exclusive: Option<&[i32]>) -> bool {
    let Some(ComponentValue::Enchantments(levels)) = patch.get(kinds::ENCHANTMENTS) else {
        return true;
    };
    let exclusive = exclusive.unwrap_or(&[]);
    levels
        .iter()
        .all(|(existing, _)| *existing == id || !exclusive.contains(existing))
}

#[cfg(test)]
mod tests {
    use mc_bamboo_protocol::{
        components::ComponentValue,
        item::ItemStack,
        packet::play::{ClientInformation, GameMode},
    };
    use uuid::Uuid;

    use crate::{
        command::{self, commands},
        global::Global,
        inbound::PlayerId,
        player::Player,
        region::Region,
    };

    fn world() -> (Global, Region, PlayerId) {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let mut player = Player::new(
            id,
            Uuid::from_u128(1),
            "Kitten".to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            mc_bamboo_core::Vec3::new(8.5, -59.0, 8.5),
        );
        player.permission_level = 2;
        player.is_op = true;
        region.insert_player(player);
        (Global::default(), region, id)
    }

    #[test]
    fn holding_a_sword_receives_the_enchantment() {
        let (mut global, mut region, id) = world();
        command::on_command(
            &mut global,
            &mut region,
            id,
            "give Kitten diamond_sword",
            None,
        );
        command::on_command(
            &mut global,
            &mut region,
            id,
            "enchant Kitten minecraft:sharpness 3",
            None,
        );
        let held = region.players[&id]
            .inventory
            .held(mc_bamboo_core::Hand::Main);
        let patch = held.patch();
        let Some(ComponentValue::Enchantments(levels)) =
            patch.get(mc_bamboo_data::registries::data_component_type::ENCHANTMENTS)
        else {
            panic!("附魔组件没有写进去");
        };
        let id_in_registry = mc_bamboo_loot::enchantment_id("minecraft:sharpness").unwrap();
        assert!(levels.contains(&(id_in_registry, 3)));
    }

    #[test]
    fn level_above_max_and_empty_hand_fail() {
        let (mut global, mut region, id) = world();
        assert!(
            commands()
                .graph
                .parse("enchant Kitten minecraft:sharpness 99", 2)
                .is_ok()
        );
        command::on_command(
            &mut global,
            &mut region,
            id,
            "enchant Kitten minecraft:sharpness 99",
            None,
        );
        let held = region.players[&id]
            .inventory
            .held(mc_bamboo_core::Hand::Main);
        assert_eq!(held, &ItemStack::empty());
    }

    #[test]
    fn an_unsupported_item_is_incompatible() {
        let (mut global, mut region, id) = world();
        let slot = region.players[&id]
            .inventory
            .hand_slot(mc_bamboo_core::Hand::Main);
        region.players.get_mut(&id).unwrap().inventory.slots[slot] =
            ItemStack::new(mc_bamboo_data::items::STONE, 1);
        command::on_command(
            &mut global,
            &mut region,
            id,
            "enchant Kitten minecraft:sharpness",
            None,
        );
        assert!(
            region.players[&id]
                .inventory
                .held(mc_bamboo_core::Hand::Main)
                .patch()
                .get(mc_bamboo_data::registries::data_component_type::ENCHANTMENTS)
                .is_none()
        );
    }
}
