//! 把世界层的区块转换成网络格式。
//!
//! `mc-bamboo-world` 不知道网络，`mc-bamboo-protocol` 不知道世界，两者在这里对接。

use mc_bamboo_protocol::packet::chunk::{
    Heightmap, LightData, Palette, encode_chunk_body, write_paletted_container,
    write_section_header,
};
use mc_bamboo_world::{
    chunk::{Chunk, Section},
    light::{LIGHT_LAYER_BYTES, SkyLight, sky_light},
    palette::{PaletteView, PalettedContainer},
};

/// 发给客户端的高度图类型：WORLD_SURFACE、MOTION_BLOCKING、MOTION_BLOCKING_NO_LEAVES。
const CLIENT_HEIGHTMAP_KINDS: [i32; 3] = [1, 4, 5];
const FULL_SKY_LAYER: [u8; LIGHT_LAYER_BYTES] = [0xff; LIGHT_LAYER_BYTES];

/// 编码区块包里坐标之后的全部内容。
pub(crate) fn encode_chunk(chunk: &Chunk) -> Vec<u8> {
    let mut sections = Vec::new();
    for section in chunk.sections() {
        write_section(&mut sections, section);
    }

    let heightmap = chunk.heightmap();
    // 目前的世界只有不透明地表，三种高度图的内容相同。
    let heightmaps = CLIENT_HEIGHTMAP_KINDS.map(|kind| Heightmap {
        kind,
        data: &heightmap,
    });

    let sky = sky_light(chunk);
    let mut sky_mask = 0;
    let mut empty_sky_mask = 0;
    let mut sky_updates: Vec<&[u8]> = Vec::new();
    for (layer, light) in sky.iter().enumerate() {
        match light {
            SkyLight::Dark => empty_sky_mask |= 1 << layer,
            SkyLight::Full => {
                sky_mask |= 1 << layer;
                sky_updates.push(&FULL_SKY_LAYER);
            }
            SkyLight::Partial(data) => {
                sky_mask |= 1 << layer;
                sky_updates.push(data.as_slice());
            }
        }
    }

    let light = LightData {
        sky_mask,
        block_mask: 0,
        empty_sky_mask,
        empty_block_mask: 0,
        sky_updates: &sky_updates,
        block_updates: &[],
    };
    encode_chunk_body(&heightmaps, &sections, &light)
}

fn write_section(out: &mut Vec<u8>, section: &Section) {
    // 目前的世界里没有流体。
    write_section_header(out, section.block_count(), 0);
    write_container(out, section.blocks());
    write_container(out, section.biomes());
}

fn write_container(out: &mut Vec<u8>, container: &PalettedContainer) {
    let palette = match container.palette() {
        PaletteView::Single(value) => Palette::Single(value),
        PaletteView::Indirect(values) => Palette::Indirect(values),
        PaletteView::Global => Palette::Global,
    };
    write_paletted_container(out, container.bits(), palette, container.data());
}
