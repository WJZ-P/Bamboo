//! 粒子参数。官方 `ParticleArgument` 读一个注册名，后面可选跟一段 SNBT 选项。
//!
//! 注册表里的 128 个类型都能解析；带选项的 19 种按官方各自的 `MapCodec` 校验，
//! 再把结果写成 `ParticleTypes.STREAM_CODEC` 用的字节。没有选项的简单类型只写
//! 注册表 ID（VarInt）。
//!
//! The particle argument. All 128 registry names parse; the 19 option-bearing
//! types are validated and encoded the way `ParticleTypes.STREAM_CODEC` does.

use mc_bamboo_blocks::BlockState;
use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{block_by_name, item_by_name, registries::particle_type};
use mc_bamboo_nbt::{Tag, parse_snbt_argument};
use mc_bamboo_protocol::components::{ComponentPatch, stack::ItemStackTemplate};

use crate::{
    ErrorArg, ParseError, Value, identifier::Identifier, reader::StringReader, value::ParticleInput,
};

/// `ParticleArgument.readParticle`：类型名加可选的 `{...}`。
pub fn parse_particle(reader: &mut StringReader) -> Result<Value, ParseError> {
    let id = Identifier::read(reader)?;
    let name = id.to_registry_name();
    let Some(id) = particle_id(&name) else {
        return Err(ParseError::with_args(
            "particle.notFound",
            [ErrorArg::Text(name)],
        ));
    };
    let options = if reader.can_read() && reader.peek() == '{' {
        let (tag, used) = match parse_snbt_argument(&reader.remaining()) {
            Ok(parsed) => parsed,
            Err(error) => {
                return Err(ParseError::text("particle.invalidOptions", error.message));
            }
        };
        reader.set_cursor(reader.cursor() + used);
        let Tag::Compound(entries) = tag else {
            return Err(ParseError::text(
                "particle.invalidOptions",
                "options must be a compound",
            ));
        };
        entries
    } else {
        Vec::new()
    };
    let bytes = match encode_options(id, &options) {
        Ok(bytes) => bytes,
        Err(message) => return Err(ParseError::text("particle.invalidOptions", message)),
    };
    Ok(Value::Particle(ParticleInput { id, bytes, name }))
}

/// 注册表里的数字 ID，按名字二分查找。
pub fn particle_id(name: &str) -> Option<i32> {
    particle_type::NAMES
        .iter()
        .position(|candidate| *candidate == name)
        .map(|index| index as i32)
}

fn encode_options(id: i32, entries: &[(String, Tag)]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    mc_bamboo_protocol::codec::write_varint(&mut out, id);
    let name = particle_type::NAMES
        .get(id as usize)
        .copied()
        .unwrap_or_default();
    match name {
        "minecraft:block"
        | "minecraft:block_marker"
        | "minecraft:block_crumble"
        | "minecraft:dust_pillar"
        | "minecraft:falling_dust" => {
            let state = block_state(entries)?;
            write_varint(&mut out, state.0 as i32);
        }
        "minecraft:item" => write_item(entries, &mut out)?,
        "minecraft:dust" | "minecraft:dust_color_transition" => {
            write_dust(name, entries, &mut out)?;
        }
        "minecraft:effect" | "minecraft:instant_effect" => {
            // `SpellParticleOption`：颜色和缩放都有默认值。
            let color = color(entries, "color").unwrap_or(-1);
            write_i32(&mut out, color);
            write_f32(&mut out, float(entries, "power").unwrap_or(1.0));
        }
        "minecraft:entity_effect" | "minecraft:flash" | "minecraft:tinted_leaves" => {
            let color = color(entries, "color").ok_or("No key color")?;
            write_i32(&mut out, color);
        }
        "minecraft:sculk_charge" => {
            write_f32(&mut out, float(entries, "roll").ok_or("No key roll")?);
        }
        "minecraft:shriek" => {
            let delay = int(entries, "delay").ok_or("No key delay")?;
            write_varint(&mut out, delay.max(0));
        }
        "minecraft:geyser" | "minecraft:geyser_plume" => {
            let blocks = int(entries, "water_blocks").ok_or("No key water_blocks")?;
            if blocks <= 0 {
                return Err("water_blocks must be positive".into());
            }
            write_i32(&mut out, blocks);
        }
        "minecraft:geyser_base" | "minecraft:geyser_poof" => {
            let blocks = int(entries, "water_blocks").ok_or("No key water_blocks")?;
            if blocks <= 0 {
                return Err("water_blocks must be positive".into());
            }
            write_i32(&mut out, blocks);
            write_f32(
                &mut out,
                float(entries, "burst_impulse_base").ok_or("No key burst_impulse_base")?,
            );
        }
        "minecraft:trail" => write_trail(entries, &mut out)?,
        "minecraft:vibration" => write_vibration(entries, &mut out)?,
        // 简单类型会忽略额外字段，和官方 `SimpleParticleType.codec()` 一样。
        _ => {}
    }
    Ok(out)
}

fn block_state(entries: &[(String, Tag)]) -> Result<BlockState, String> {
    let tag = field(entries, "block_state").ok_or("No key block_state")?;
    let name = match tag {
        Tag::String(text) => text.clone(),
        Tag::Compound(parts) => match field(parts, "id") {
            Some(Tag::String(text)) => text.clone(),
            _ => return Err("No key id".into()),
        },
        _ => return Err("block_state is not a block".into()),
    };
    let name = Identifier::parse(&name)
        .ok_or("invalid block id")?
        .to_registry_name();
    let block = block_by_name(&name).ok_or("unknown block")?;
    Ok(BlockState(block.default_state))
}

fn write_item(entries: &[(String, Tag)], out: &mut Vec<u8>) -> Result<(), String> {
    let tag = field(entries, "item").ok_or("No key item")?;
    let (name, count) = match tag {
        Tag::String(text) => (text.clone(), 1),
        Tag::Compound(parts) => {
            let name = match field(parts, "id") {
                Some(Tag::String(text)) => text.clone(),
                _ => return Err("No key id".into()),
            };
            let count = match field(parts, "count") {
                Some(tag) => int_in(tag).ok_or("count is not a number")?,
                None => 1,
            };
            (name, count)
        }
        _ => return Err("item is not an item".into()),
    };
    if count <= 0 {
        return Err("item count must be positive".into());
    }
    let name = Identifier::parse(&name)
        .ok_or("invalid item id")?
        .to_registry_name();
    let item = item_by_name(&name).ok_or("unknown item")?;
    let id = mc_bamboo_data::items::ITEMS
        .iter()
        .position(|candidate| candidate.name == item.name)
        .ok_or("unknown item id")? as i32;
    let template = ItemStackTemplate {
        item: id,
        count,
        components: ComponentPatch::new(),
    };
    ItemStackTemplate::write(out, &template);
    Ok(())
}

fn write_dust(name: &str, entries: &[(String, Tag)], out: &mut Vec<u8>) -> Result<(), String> {
    if name == "minecraft:dust_color_transition" {
        write_i32(
            out,
            color(entries, "from_color").ok_or("No key from_color")?,
        );
        write_i32(out, color(entries, "to_color").ok_or("No key to_color")?);
    } else {
        write_i32(out, color(entries, "color").ok_or("No key color")?);
    }
    let scale = float(entries, "scale").ok_or("No key scale")?;
    if !(0.01..=4.0).contains(&scale) {
        return Err("scale out of range".into());
    }
    write_f32(out, scale);
    Ok(())
}

fn write_trail(entries: &[(String, Tag)], out: &mut Vec<u8>) -> Result<(), String> {
    let target = field(entries, "target").ok_or("No key target")?;
    let Tag::List(values) = target else {
        return Err("target must be a list".into());
    };
    if values.len() != 3 {
        return Err("target must have three doubles".into());
    }
    for value in values {
        write_f64(out, number(value).ok_or("target is not a number")?);
    }
    let color = color(entries, "color").ok_or("No key color")?;
    write_i32(out, color);
    let duration = int(entries, "duration").ok_or("No key duration")?;
    write_varint(out, duration.max(0));
    Ok(())
}

fn write_vibration(entries: &[(String, Tag)], out: &mut Vec<u8>) -> Result<(), String> {
    let destination = compound(field(entries, "destination").ok_or("No key destination")?)?;
    let kind = match field(destination, "type") {
        Some(Tag::String(text)) => text.as_str(),
        _ => return Err("destination needs a type".into()),
    };
    if kind != "block" {
        return Err("only block position sources are supported".into());
    }
    // PositionSource.STREAM_CODEC：来源类型 VarInt，再写内容。
    write_varint(out, 0);
    let pos = field(destination, "pos").ok_or("No key pos")?;
    let Tag::List(values) = pos else {
        return Err("pos must be a list".into());
    };
    if values.len() != 3 {
        return Err("pos must have three ints".into());
    }
    let mut coordinates = [0; 3];
    for (index, value) in values.iter().enumerate() {
        coordinates[index] = int_in(value).ok_or("pos is not an int")?;
    }
    mc_bamboo_protocol::codec::write_i64(
        out,
        BlockPos::new(coordinates[0], coordinates[1], coordinates[2]).pack(),
    );
    write_varint(
        out,
        int(entries, "arrival_in_ticks")
            .ok_or("No key arrival_in_ticks")?
            .max(0),
    );
    Ok(())
}

fn color(entries: &[(String, Tag)], name: &str) -> Option<i32> {
    let tag = field(entries, name)?;
    if let Some(value) = int_in(tag) {
        return Some(value);
    }
    let Tag::List(values) = tag else {
        return None;
    };
    // `RGB_COLOR_CODEC` 的列表形式严格是三个 float；`ARGB_COLOR_CODEC` 才是四个。
    if values.len() != 3 {
        return None;
    }
    let mut rgb = 0;
    for (index, value) in values.iter().enumerate() {
        // `ARGB.colorFromFloat(1, r, g, b)`：每种通道 floor(x*255)，alpha 固定 255。
        let channel = ((number(value)? as f32) * 255.0).floor() as i32;
        rgb |= (channel & 0xff) << (16 - index * 8);
    }
    Some(0xff00_0000u32 as i32 | rgb)
}

fn float(entries: &[(String, Tag)], name: &str) -> Option<f32> {
    number(field(entries, name)?).map(|value| value as f32)
}

fn int(entries: &[(String, Tag)], name: &str) -> Option<i32> {
    int_in(field(entries, name)?)
}

fn int_in(tag: &Tag) -> Option<i32> {
    match tag {
        Tag::Byte(value) => Some(i32::from(*value)),
        Tag::Short(value) => Some(i32::from(*value)),
        Tag::Int(value) => Some(*value),
        Tag::Long(value) => i32::try_from(*value).ok(),
        Tag::Float(value) => Some(*value as i32),
        Tag::Double(value) => Some(*value as i32),
        _ => None,
    }
}

fn number(tag: &Tag) -> Option<f64> {
    match tag {
        Tag::Byte(value) => Some(f64::from(*value)),
        Tag::Short(value) => Some(f64::from(*value)),
        Tag::Int(value) => Some(f64::from(*value)),
        Tag::Long(value) => Some(*value as f64),
        Tag::Float(value) => Some(f64::from(*value)),
        Tag::Double(value) => Some(*value),
        _ => None,
    }
}

fn compound(tag: &Tag) -> Result<&[(String, Tag)], String> {
    match tag {
        Tag::Compound(entries) => Ok(entries),
        _ => Err("expected a compound".into()),
    }
}

fn field<'a>(entries: &'a [(String, Tag)], name: &str) -> Option<&'a Tag> {
    entries
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

fn write_varint(out: &mut Vec<u8>, value: i32) {
    mc_bamboo_protocol::codec::write_varint(out, value);
}

fn write_i32(out: &mut Vec<u8>, value: i32) {
    mc_bamboo_protocol::codec::write_i32(out, value);
}

fn write_f32(out: &mut Vec<u8>, value: f32) {
    mc_bamboo_protocol::codec::write_f32(out, value);
}

fn write_f64(out: &mut Vec<u8>, value: f64) {
    mc_bamboo_protocol::codec::write_f64(out, value);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> ParticleInput {
        match parse_particle(&mut StringReader::new(input)).unwrap() {
            Value::Particle(value) => value,
            other => panic!("不是粒子参数：{other:?}"),
        }
    }

    #[test]
    fn every_registry_name_parses() {
        for name in particle_type::NAMES {
            // 带选项的类型单独测；这里只保证简单类型都能过。
            if matches!(
                *name,
                "minecraft:block"
                    | "minecraft:block_marker"
                    | "minecraft:block_crumble"
                    | "minecraft:dust_pillar"
                    | "minecraft:falling_dust"
                    | "minecraft:item"
                    | "minecraft:dust"
                    | "minecraft:dust_color_transition"
                    | "minecraft:entity_effect"
                    | "minecraft:flash"
                    | "minecraft:tinted_leaves"
                    | "minecraft:sculk_charge"
                    | "minecraft:shriek"
                    | "minecraft:geyser"
                    | "minecraft:geyser_base"
                    | "minecraft:geyser_poof"
                    | "minecraft:geyser_plume"
                    | "minecraft:trail"
                    | "minecraft:vibration"
            ) {
                continue;
            }
            let value = parse(name);
            assert_eq!(value.name, *name);
            assert!(!value.bytes.is_empty(), "{name} 至少要写出 ID");
        }
    }

    #[test]
    fn simple_particle_writes_just_its_registry_id() {
        let flame = parse("minecraft:flame");
        assert_eq!(flame.id, 39);
        assert_eq!(flame.bytes, vec![0x27]);
        assert_eq!(parse("flame").bytes, vec![0x27]);
        assert_eq!(parse("flame{unused:1}").bytes, vec![0x27]);
    }

    #[test]
    fn options_match_the_official_stream_codec() {
        assert_eq!(
            parse("dust{color:[1.0,0.0,0.0],scale:1.0}").bytes,
            vec![0x15, 0xff, 0xff, 0x00, 0x00, 0x3f, 0x80, 0x00, 0x00]
        );
        assert_eq!(parse("block{block_state:stone}").bytes, vec![0x01, 0x01]);
        assert_eq!(
            parse("item{item:stone}").bytes,
            vec![0x39, 0x01, 0x01, 0x00, 0x00]
        );
        assert_eq!(parse("shriek{delay:10}").bytes, vec![0x73, 0x0a]);
        assert_eq!(
            parse("entity_effect{color:16711680}").bytes,
            vec![0x1c, 0x00, 0xff, 0x00, 0x00]
        );
    }

    #[test]
    fn unknown_particles_and_missing_options_report_the_official_keys() {
        let error = parse_particle(&mut StringReader::new("not_a_particle")).unwrap_err();
        assert_eq!(error.key, "particle.notFound");
        let error = parse_particle(&mut StringReader::new("dust")).unwrap_err();
        assert_eq!(error.key, "particle.invalidOptions");
        let error = parse_particle(&mut StringReader::new("dust{scale:1}")).unwrap_err();
        assert_eq!(error.key, "particle.invalidOptions");
    }
}

#[cfg(test)]
mod oracle_tests {
    use super::*;

    fn parse(input: &str) -> ParticleInput {
        match parse_particle(&mut StringReader::new(input)).unwrap() {
            Value::Particle(value) => value,
            other => panic!("不是粒子参数：{other:?}"),
        }
    }

    /// 官方 `ParticleArgument` 加 `ParticleTypes.STREAM_CODEC` 写出的字节，见工作笔记。
    const CASES: &[(&str, &[u8])] = &[
        (
            "dust{color:16711680,scale:2.0}",
            &[0x15, 0, 0xff, 0, 0, 0x40, 0, 0, 0],
        ),
        (
            "dust_color_transition{from_color:[1,0,0],to_color:[0,1,0],scale:1.0}",
            &[0x16, 0xff, 0xff, 0, 0, 0xff, 0, 0xff, 0, 0x3f, 0x80, 0, 0],
        ),
        ("block_marker{block_state:stone}", &[0x02, 0x01]),
        ("block_crumble{block_state:stone}", &[0x7d, 0x01]),
        ("dust_pillar{block_state:stone}", &[0x79, 0x01]),
        ("falling_dust{block_state:stone}", &[0x24, 0x01]),
        ("item{item:{id:stone,count:2}}", &[0x39, 1, 2, 0, 0]),
        ("effect", &[0x17, 0xff, 0xff, 0xff, 0xff, 0x3f, 0x80, 0, 0]),
        (
            "effect{color:[1,0,0],power:2.0}",
            &[0x17, 0xff, 0xff, 0, 0, 0x40, 0, 0, 0],
        ),
        (
            "instant_effect",
            &[0x38, 0xff, 0xff, 0xff, 0xff, 0x3f, 0x80, 0, 0],
        ),
        ("flash{color:0}", &[0x34, 0, 0, 0, 0]),
        ("tinted_leaves{color:16711680}", &[0x2e, 0, 0xff, 0, 0]),
        ("sculk_charge{roll:0.25}", &[0x30, 0x3e, 0x80, 0, 0]),
        ("shriek{delay:255}", &[0x73, 0xff, 0x01]),
        ("geyser{water_blocks:3}", &[0x07, 0, 0, 0, 3]),
        (
            "geyser_base{water_blocks:3,burst_impulse_base:0.5}",
            &[0x08, 0, 0, 0, 3, 0x3f, 0, 0, 0],
        ),
        (
            "geyser_poof{water_blocks:3,burst_impulse_base:0.5}",
            &[0x09, 0, 0, 0, 3, 0x3f, 0, 0, 0],
        ),
        ("geyser_plume{water_blocks:3}", &[0x0a, 0, 0, 0, 3]),
        (
            "trail{target:[1,2,3],color:16711680,duration:40}",
            &[
                0x3b, 0x3f, 0xf0, 0, 0, 0, 0, 0, 0, 0x40, 0, 0, 0, 0, 0, 0, 0, 0x40, 0x08, 0, 0, 0,
                0, 0, 0, 0, 0xff, 0, 0, 0x28,
            ],
        ),
        (
            "vibration{destination:{type:block,pos:[4,5,6]},arrival_in_ticks:10}",
            &[0x3a, 0, 0, 0, 1, 0, 0, 0, 0x60, 0x05, 0x0a],
        ),
    ];

    #[test]
    fn option_bytes_match_the_official_encoder() {
        for (input, expected) in CASES {
            let value = parse(input);
            assert_eq!(value.bytes, *expected, "{input}");
        }
    }
}
