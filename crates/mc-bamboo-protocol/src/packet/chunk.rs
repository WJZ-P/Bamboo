//! 区块数据的网络格式。
//!
//! 这里只描述字节怎么排列。区块本身是什么样由 `mc-bamboo-world` 决定，
//! 两者在 `mc-bamboo-game` 里对接。

use mc_bamboo_core::BlockPos;

use crate::codec::{write_i16, write_varint};

/// 调色板容器里的调色板。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Palette<'a> {
    /// 整个容器只有一种值，后面不需要索引数据。
    Single(u32),
    /// 一张值表，索引数据里存的是表内下标。
    Indirect(&'a [u32]),
    /// 没有值表，索引数据里直接存全局 ID。
    Global,
}

/// 写入一个调色板容器：位数、调色板、固定长度的打包数据。
pub fn write_paletted_container(out: &mut Vec<u8>, bits: u8, palette: Palette<'_>, data: &[u64]) {
    out.push(bits);
    match palette {
        Palette::Single(value) => write_varint(out, value as i32),
        Palette::Indirect(values) => {
            write_varint(out, values.len() as i32);
            for &value in values {
                write_varint(out, value as i32);
            }
        }
        Palette::Global => {}
    }
    // 长度由位数算得出来，所以不写长度前缀。
    for word in data {
        out.extend_from_slice(&word.to_be_bytes());
    }
}

/// 写入区块段的开头：非空气方块数和流体方块数。流体数是 26.x 新增的。
pub fn write_section_header(out: &mut Vec<u8>, block_count: u16, fluid_count: u16) {
    out.extend_from_slice(&block_count.to_be_bytes());
    out.extend_from_slice(&fluid_count.to_be_bytes());
}

/// 发给客户端的高度图。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Heightmap<'a> {
    /// 高度图类型的编号。
    pub kind: i32,
    pub data: &'a [u64],
}

/// 区块包里的一个方块实体。
///
/// 收到区块时客户端只创建列表里的方块实体，漏掉的方块（比如箱子）就看不见了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockEntityInfo {
    /// 世界坐标。水平方向只取区块内的低 4 位。
    pub pos: BlockPos,
    /// `minecraft:block_entity_type` 注册表 ID。
    pub kind: i32,
}

/// 区块的光照数据。掩码的第 n 位对应从世界底部下一层算起的第 n 个区块段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightData<'a> {
    /// 有天空光数据的区块段。
    pub sky_mask: u64,
    /// 有方块光数据的区块段。
    pub block_mask: u64,
    /// 天空光全为零的区块段。
    pub empty_sky_mask: u64,
    /// 方块光全为零的区块段。
    pub empty_block_mask: u64,
    /// 每段 2048 字节，顺序对应掩码里置位的区块段。
    pub sky_updates: &'a [&'a [u8]],
    pub block_updates: &'a [&'a [u8]],
}

/// 写入一个 BitSet。26.3 起用字节数组表示，末尾的零字节会被省略。
fn write_bit_set(out: &mut Vec<u8>, mask: u64) {
    let used_bytes = 8 - (mask.leading_zeros() / 8) as usize;
    write_varint(out, used_bytes as i32);
    out.extend_from_slice(&mask.to_le_bytes()[..used_bytes]);
}

fn write_block_entities(out: &mut Vec<u8>, block_entities: &[BlockEntityInfo]) {
    write_varint(out, block_entities.len() as i32);
    for block_entity in block_entities {
        let BlockPos { x, y, z } = block_entity.pos;
        out.push(((x & 15) << 4 | (z & 15)) as u8);
        write_i16(out, y as i16);
        write_varint(out, block_entity.kind);
        // 没有 NBT。客户端按方块状态创建默认的方块实体。
        out.push(0);
    }
}

fn write_light_layers(out: &mut Vec<u8>, layers: &[&[u8]]) {
    write_varint(out, layers.len() as i32);
    for layer in layers {
        write_varint(out, layer.len() as i32);
        out.extend_from_slice(layer);
    }
}

/// 编码区块包里坐标之后的全部内容。
///
/// `sections` 是所有区块段依次写好的结果。内容和坐标无关，相同的区块可以共用这段字节。
pub fn encode_chunk_body(
    heightmaps: &[Heightmap<'_>],
    sections: &[u8],
    block_entities: &[BlockEntityInfo],
    light: &LightData<'_>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(sections.len() + 4096);

    write_varint(&mut out, heightmaps.len() as i32);
    for heightmap in heightmaps {
        write_varint(&mut out, heightmap.kind);
        write_varint(&mut out, heightmap.data.len() as i32);
        for word in heightmap.data {
            out.extend_from_slice(&word.to_be_bytes());
        }
    }

    write_varint(&mut out, sections.len() as i32);
    out.extend_from_slice(sections);

    write_block_entities(&mut out, block_entities);

    write_bit_set(&mut out, light.sky_mask);
    write_bit_set(&mut out, light.block_mask);
    write_bit_set(&mut out, light.empty_sky_mask);
    write_bit_set(&mut out, light.empty_block_mask);
    write_light_layers(&mut out, light.sky_updates);
    write_light_layers(&mut out, light.block_updates);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_value_container_has_no_data() {
        let mut out = Vec::new();
        write_paletted_container(&mut out, 0, Palette::Single(300), &[]);
        assert_eq!(out, [0, 0xac, 0x02]);
    }

    #[test]
    fn indirect_container_lists_its_palette_before_the_data() {
        let mut out = Vec::new();
        write_paletted_container(&mut out, 4, Palette::Indirect(&[0, 9]), &[1]);
        assert_eq!(out, [4, 2, 0, 9, 0, 0, 0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn global_container_has_no_palette() {
        let mut out = Vec::new();
        write_paletted_container(&mut out, 16, Palette::Global, &[1]);
        assert_eq!(out, [16, 0, 0, 0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn bit_set_drops_trailing_zero_bytes() {
        let cases: [(u64, &[u8]); 4] = [
            (0, &[0]),
            (1, &[1, 1]),
            (0x0100, &[2, 0, 1]),
            (1 << 25, &[4, 0, 0, 0, 2]),
        ];
        for (mask, expected) in cases {
            let mut out = Vec::new();
            write_bit_set(&mut out, mask);
            assert_eq!(out, expected, "mask {mask:#x}");
        }
    }

    #[test]
    fn block_entity_matches_official_bytes() {
        let block_entity = BlockEntityInfo {
            pos: BlockPos::new(3, -60, 5),
            kind: 1,
        };
        let mut out = Vec::new();
        write_block_entities(&mut out, &[block_entity]);
        assert_eq!(out, [1, 0x35, 0xff, 0xc4, 0x01, 0x00]);
    }

    #[test]
    fn block_entity_keeps_only_the_position_inside_the_chunk() {
        let block_entity = BlockEntityInfo {
            pos: BlockPos::new(-13, 70, 37),
            kind: 300,
        };
        let mut out = Vec::new();
        write_block_entities(&mut out, &[block_entity]);
        assert_eq!(out, [1, 0x35, 0x00, 70, 0xac, 0x02, 0x00]);
    }

    #[test]
    fn body_keeps_the_official_field_order() {
        let heightmaps = [Heightmap {
            kind: 1,
            data: &[2],
        }];
        let light = LightData {
            sky_mask: 1,
            block_mask: 0,
            empty_sky_mask: 2,
            empty_block_mask: 0,
            sky_updates: &[&[0xaa, 0xbb]],
            block_updates: &[],
        };
        let block_entities = [BlockEntityInfo {
            pos: BlockPos::new(3, -60, 5),
            kind: 1,
        }];
        let body = encode_chunk_body(&heightmaps, &[7, 8], &block_entities, &light);

        assert_eq!(
            body,
            [
                1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 2, // 高度图
                2, 7, 8, // 区块段
                1, 0x35, 0xff, 0xc4, 1, 0, // 方块实体
                1, 1, 0, 1, 2, 0, // 四个掩码
                1, 2, 0xaa, 0xbb, // 天空光
                0,    // 方块光
            ]
        );
    }
}
