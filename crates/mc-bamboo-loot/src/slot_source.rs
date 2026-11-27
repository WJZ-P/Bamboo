//! 战利品槽位来源：读取已捕获的实体装备，不访问或修改世界。
//!
//! Loot slot sources read captured entity equipment without accessing or mutating the world.

use serde_json::Value;

use crate::context::{EntityTarget, EquipmentSlot, LootContext};
use crate::json::{array, object, opt_string, required, string, type_name};
use crate::{LootError, LootStack, Result};

pub(crate) fn items(source: &str, context: &LootContext<'_, '_, '_>) -> Result<Vec<LootStack>> {
    if let Some(items) = context
        .params
        .level
        .and_then(|level| level.slot_items(source))
    {
        return Ok(items);
    }
    let value = serde_json::from_str(source)
        .map_err(|error| LootError::Invalid(format!("invalid slot source: {error}")))?;
    read(&value, context, 0)
}

fn read(value: &Value, context: &LootContext<'_, '_, '_>, depth: u32) -> Result<Vec<LootStack>> {
    if depth > crate::entry::MAX_DEPTH {
        return Err(LootError::Invalid(
            "slot source nests too deeply".to_owned(),
        ));
    }
    if let Some(terms) = value.as_array() {
        return group(terms, context, depth);
    }
    if value.is_string() {
        return Err(LootError::Unsupported(
            "unresolved named slot source".to_owned(),
        ));
    }
    let map = object(value, "slot source")?;
    match type_name(map, "slot source")? {
        "minecraft:empty" => Ok(Vec::new()),
        "minecraft:group" => group(
            array(required(map, "terms", "slot source")?, "slot source terms")?,
            context,
            depth,
        ),
        "minecraft:slot_range" => {
            let Some(target) =
                opt_string(map, "source", "slot range source")?.and_then(EntityTarget::from_name)
            else {
                return Err(LootError::Unsupported(
                    "slot range source needs an inventory view".to_owned(),
                ));
            };
            let slots = string(required(map, "slots", "slot range")?, "slots")?;
            let slots = equipment_range(slots).ok_or_else(|| {
                LootError::Unsupported(format!("slot range {slots} needs an inventory view"))
            })?;
            let Some(entity) = target.get(context.params).filter(|entity| entity.is_living) else {
                return Ok(Vec::new());
            };
            Ok(slots
                .iter()
                .map(|slot| {
                    entity
                        .equipment(*slot)
                        .cloned()
                        .unwrap_or_else(LootStack::empty)
                })
                .collect())
        }
        kind => Err(LootError::Unsupported(format!(
            "slot source {kind} needs a slot view"
        ))),
    }
}

fn group(terms: &[Value], context: &LootContext<'_, '_, '_>, depth: u32) -> Result<Vec<LootStack>> {
    let mut items = Vec::new();
    for term in terms {
        items.extend(read(term, context, depth + 1)?);
    }
    Ok(items)
}

fn equipment_range(name: &str) -> Option<&'static [EquipmentSlot]> {
    use EquipmentSlot::{Body, Chest, Feet, Head, Legs, Mainhand, Offhand, Saddle};
    Some(match name {
        "weapon" | "weapon.mainhand" => &[Mainhand],
        "weapon.offhand" => &[Offhand],
        "weapon.*" => &[Mainhand, Offhand],
        "armor.head" => &[Head],
        "armor.chest" => &[Chest],
        "armor.legs" => &[Legs],
        "armor.feet" => &[Feet],
        "armor.body" => &[Body],
        "armor.*" => &[Head, Chest, Legs, Feet, Body],
        "saddle" => &[Saddle],
        _ => return None,
    })
}
