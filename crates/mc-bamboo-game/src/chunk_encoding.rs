//! 把世界层的区块转换成网络格式。
//!
//! `mc-bamboo-world` 不知道网络，`mc-bamboo-protocol` 不知道世界，两者在这里对接。

use std::borrow::Cow;

use mc_bamboo_core::{BlockPos, ChunkPos};
use mc_bamboo_protocol::packet::chunk::{
    BlockEntityInfo, Heightmap, LightData, Palette, encode_chunk_body, write_paletted_container,
    write_section_header,
};
use mc_bamboo_world::{
    chunk::{Chunk, Section},
    light::{LIGHT_LAYER_BYTES, SkyLight, sky_light},
    palette::{PaletteView, PalettedContainer},
    store::ChunkStore,
};

/// 发给客户端的高度图类型：WORLD_SURFACE、MOTION_BLOCKING、MOTION_BLOCKING_NO_LEAVES。
const CLIENT_HEIGHTMAP_KINDS: [i32; 3] = [1, 4, 5];
const FULL_SKY_LAYER: [u8; LIGHT_LAYER_BYTES] = [0xff; LIGHT_LAYER_BYTES];

/// 一个区块此刻的内容。没改过的区块共用 `template_body`，改过的每次重新编码。
pub(crate) fn chunk_body<'a>(
    store: &ChunkStore,
    template_body: &'a [u8],
    pos: ChunkPos,
) -> Cow<'a, [u8]> {
    store
        .edited(pos)
        .map_or(Cow::Borrowed(template_body), |chunk| {
            Cow::Owned(encode_chunk(chunk))
        })
}

/// 编码区块包里坐标之后的全部内容。
pub(crate) fn encode_chunk(chunk: &Chunk) -> Vec<u8> {
    let mut sections = Vec::new();
    for section in chunk.sections() {
        write_section(&mut sections, section);
    }

    let heightmap = chunk.heightmap();
    // 高度图把所有非空气方块都算上，不区分方块种类，所以三种的内容相同。
    let heightmaps = CLIENT_HEIGHTMAP_KINDS.map(|kind| Heightmap {
        kind,
        data: &heightmap,
    });

    let block_entities: Vec<BlockEntityInfo> = chunk
        .block_entities()
        .map(|entity| BlockEntityInfo {
            pos: BlockPos::new(entity.x as i32, entity.y, entity.z as i32),
            kind: entity.kind,
        })
        .collect();

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
    encode_chunk_body(&heightmaps, &sections, &block_entities, &light)
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

#[cfg(test)]
mod tests {
    use mc_bamboo_data::blocks;

    use super::*;

    /// `blocks-inventory.md` 第 11 章的实测字节：区块内 (3, -60, 5) 的箱子。
    const CHEST_ENTRY: [u8; 5] = [0x35, 0xff, 0xc4, 0x01, 0x00];

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|part| part == needle)
    }

    #[test]
    fn untouched_chunks_share_the_template_bytes() {
        let store = ChunkStore::flat();
        let template_body = encode_chunk(store.template());

        let body = chunk_body(&store, &template_body, ChunkPos::new(3, -9));

        assert!(matches!(body, Cow::Borrowed(_)));
        assert_eq!(*body, *template_body);
    }

    #[test]
    fn edited_chunks_are_encoded_with_their_current_content() {
        let mut store = ChunkStore::flat();
        let template_body = encode_chunk(store.template());
        let pos = BlockPos::new(1, -60, 1);

        store.set_block(pos, blocks::STONE);
        let edited = chunk_body(&store, &template_body, pos.chunk());
        assert_ne!(*edited, *template_body);
        assert_eq!(
            *edited,
            *encode_chunk(store.edited(pos.chunk()).expect("这个区块改过了"))
        );
    }

    #[test]
    fn block_entities_are_written_with_positions_inside_the_chunk() {
        let mut store = ChunkStore::flat();
        let template_body = encode_chunk(store.template());
        // 这个箱子在 (1, -1) 区块，世界坐标不是区块内坐标。
        let pos = BlockPos::new(19, -60, -11);

        store.set_block(pos, blocks::CHEST);

        let body = chunk_body(&store, &template_body, pos.chunk());
        assert!(contains(&body, &CHEST_ENTRY));
    }

    #[test]
    fn flat_chunk_has_no_block_entities() {
        let body = encode_chunk(ChunkStore::flat().template());
        assert!(!contains(&body, &CHEST_ENTRY));
    }
}
