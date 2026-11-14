//! 26.3 地物配置里的方块状态，以及 `BlockState.toString()` 的对拍名字。
//!
//! `Feature`/`RuleTest` 的 codec 用两种形式：注册名字符串（`"minecraft:stone"`），或
//! `{"id": "minecraft:grass_block", "properties": {"snowy": "false"}}`。
//! `BlockState.toString()` 是 `Block{minecraft:grass_block}[snowy=false]`。

use mc_bamboo_data::{block_by_name, blocks};
use serde_json::Value;

/// 解析一个地物配置里的方块状态，返回 Bamboo 的全局状态 ID。
pub fn parse_state(value: &Value) -> Result<u32, String> {
    let (name, properties) = match value {
        Value::String(name) => (name.as_str(), None),
        Value::Object(object) => {
            let name = object
                .get("id")
                .or_else(|| object.get("Name"))
                .and_then(Value::as_str)
                .ok_or("block state has no id")?;
            (name, object.get("properties"))
        }
        _ => return Err("block state must be a string or an object".to_owned()),
    };
    let block = block_by_name(name).ok_or_else(|| format!("unknown block {name}"))?;
    let mut state = block.default_state;
    if let Some(Value::Object(props)) = properties {
        for (key, value) in props {
            let text = value
                .as_str()
                .ok_or_else(|| format!("block state {name} property {key} is not a string"))?;
            state = apply_property(state, key, text)
                .ok_or_else(|| format!("cannot set {name} {key}={text}"))?;
        }
    }
    Ok(state)
}

/// 按官方 `StateDefinition` 的属性顺序改一个属性的值。
///
/// 状态 ID 是属性值笛卡尔积，最后一个属性变化最快（`blocks.rs` 的生成注释）。
fn apply_property(state: u32, name: &str, value: &str) -> Option<u32> {
    let block = mc_bamboo_data::block_of_state(state)?;
    let mut stride = 1_u32;
    for property in block.properties.iter().rev() {
        let count = property.values.len() as u32;
        if property.name == name {
            let index = property.values.iter().position(|&v| v == value)?;
            let current = (state - block.first_state) / stride % count;
            return Some(state - current * stride + index as u32 * stride);
        }
        stride *= count;
    }
    None
}

/// 把 `Block{minecraft:x}[a=1,b=true]` 精确还原成状态 ID。
///
/// terrain 三步只有默认状态，`chunkgen.rs` 里那个忽略属性的简易解析不够用；
/// 地物会放带属性的状态（水、原木、草地、红石矿……）。
pub fn state_from_official_name(name: &str) -> Result<u32, String> {
    let body = name
        .strip_prefix("Block{")
        .ok_or_else(|| format!("not a BlockState.toString(): {name}"))?;
    let (block_name, rest) = body
        .split_once('}')
        .ok_or_else(|| format!("not a BlockState.toString(): {name}"))?;
    let block = block_by_name(block_name).ok_or_else(|| format!("unknown block {block_name}"))?;
    let mut state = block.default_state;
    let props = rest.strip_prefix('[').and_then(|s| s.strip_suffix(']'));
    if let Some(props) = props {
        for entry in props.split(',') {
            let (key, value) = entry
                .split_once('=')
                .ok_or_else(|| format!("bad property in {name}"))?;
            state = apply_property(state, key, value)
                .ok_or_else(|| format!("cannot set {block_name} {key}={value}"))?;
        }
    }
    Ok(state)
}

/// 状态 ID 的 `BlockState.toString()` 等价形式，报告里用。
pub fn official_name(state: u32) -> String {
    let Some(id) = mc_bamboo_data::block_id_of_state(state) else {
        return format!("<state {state}>");
    };
    let block = &blocks::BLOCKS[id];
    let mut out = format!("Block{{{}}}", block.name);
    if !block.properties.is_empty() {
        out.push('[');
        let mut stride = 1_u32;
        let mut parts = Vec::with_capacity(block.properties.len());
        for property in block.properties.iter().rev() {
            let count = property.values.len() as u32;
            let index = (state - block.first_state) / stride % count;
            parts.push(format!(
                "{}={}",
                property.name, property.values[index as usize]
            ));
            stride *= count;
        }
        parts.reverse();
        out.push_str(&parts.join(","));
        out.push(']');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_states() {
        let air: Value = Value::String("minecraft:air".to_owned());
        assert_eq!(parse_state(&air).unwrap(), blocks::AIR);
        let grass: Value = serde_json::from_str(
            r#"{"id":"minecraft:grass_block","properties":{"snowy":"false"}}"#,
        )
        .unwrap();
        assert_eq!(
            parse_state(&grass).unwrap(),
            state_from_official_name("Block{minecraft:grass_block}[snowy=false]").unwrap()
        );
    }

    #[test]
    fn parses_official_names_with_properties() {
        let grass = state_from_official_name("Block{minecraft:grass_block}[snowy=false]").unwrap();
        assert_eq!(
            official_name(grass),
            "Block{minecraft:grass_block}[snowy=false]"
        );
        let water = state_from_official_name("Block{minecraft:water}[level=0]").unwrap();
        assert_eq!(official_name(water), "Block{minecraft:water}[level=0]");
        let stone = state_from_official_name("Block{minecraft:stone}").unwrap();
        assert_eq!(official_name(stone), "Block{minecraft:stone}");
        assert_eq!(stone, blocks::STONE);
    }

    #[test]
    fn round_trips_every_state_of_a_multi_property_block() {
        // 找一个有多个属性的方块，验证属性顺序与 stride。
        let block = blocks::BLOCKS
            .iter()
            .find(|b| b.properties.len() >= 2)
            .expect("there are multi-property blocks");
        let count = mc_bamboo_data::state_count(
            blocks::BLOCKS
                .iter()
                .position(|b| b.name == block.name)
                .unwrap(),
        );
        for offset in 0..count.min(64) {
            let state = block.first_state + offset;
            let text = official_name(state);
            assert_eq!(state_from_official_name(&text).unwrap(), state, "{text}");
        }
    }
}
