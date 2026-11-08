//! 把世界层的区块转换成网络格式。
//!
//! `mc-bamboo-world` 不知道网络，`mc-bamboo-protocol` 不知道世界，两者在这里对接。
//! 没改过的区块共用模板的编码，改过的区块按版本号缓存，见 [`ChunkCache`]。

mod cache;

use std::borrow::Cow;

use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer};
use mc_bamboo_data::heightmaps;
use mc_bamboo_protocol::packet::chunk::{
    BlockEntityInfo, Heightmap, LightData, Palette, encode_chunk_body, write_paletted_container,
    write_section_header,
};
use mc_bamboo_world::{
    chunk::{Chunk, HeightmapKind, Section},
    light::{LIGHT_SECTION_COUNT, LevelLight, MIN_LIGHT_SECTION, SectionPos},
    palette::{PaletteView, PalettedContainer},
    store::ChunkStore,
};

pub(crate) use cache::ChunkCache;

use crate::sign;

/// 方块和光照都没改过的区块共用模板的编码，随便取一个还没改过的位置编出来就行。
const TEMPLATE_POS: ChunkPos = ChunkPos::new(0, 0);
/// 光照段全选：区块包带上全部 26 段。
const ALL_SECTIONS: u32 = (1 << LIGHT_SECTION_COUNT) - 1;

/// 一个区块此刻的内容。没改过的区块共用 `template_body`，改过的从 `cache` 拿。
pub(crate) fn chunk_body<'a>(
    cache: &'a mut ChunkCache,
    store: &ChunkStore,
    template_body: &'a [u8],
    pos: ChunkPos,
) -> Cow<'a, [u8]> {
    if store.is_template(pos) {
        Cow::Borrowed(template_body)
    } else {
        cache.body(store, pos)
    }
}

/// 模板区块的编码，世界刚建好时调用。
pub(crate) fn encode_template(store: &ChunkStore) -> Vec<u8> {
    encode_chunk(store, TEMPLATE_POS)
}

/// 编码区块包里坐标之后的全部内容。
///
/// 只读 `store` 里这个区块和它那一列光照，[`ChunkCache`] 靠 [`ChunkStore::revision`] 判断它们变没变。
/// 以后要从别处读数据写进区块包，那里的修改也得让版本号变，不然缓存会发旧内容。
pub(crate) fn encode_chunk(store: &ChunkStore, pos: ChunkPos) -> Vec<u8> {
    let chunk = store.chunk(pos);
    let mut sections = Vec::new();
    for section in chunk.sections() {
        write_section(&mut sections, section);
    }

    let heightmap_data = client_heightmaps(chunk);
    let heightmaps: Vec<Heightmap> = heightmap_data
        .iter()
        .map(|(kind, data)| Heightmap { kind: *kind, data })
        .collect();

    // 告示牌的文字不放在区块里，要编成官方的 `getUpdateTag` 一起发。
    let block_entities: Vec<(BlockEntityInfo, Option<Vec<u8>>)> = chunk
        .block_entities()
        .map(|entity| {
            // 表里存的是区块内坐标，查告示牌内容要用世界坐标。
            let local = BlockPos::new(entity.x as i32, entity.y, entity.z as i32);
            let world = BlockPos::new(
                pos.x * 16 + entity.x as i32,
                entity.y,
                pos.z * 16 + entity.z as i32,
            );
            let data = store.sign(world).map(|content| sign::update_tag(&content));
            (
                BlockEntityInfo {
                    pos: local,
                    kind: entity.kind,
                    data: None,
                },
                data,
            )
        })
        .collect();
    let block_entities: Vec<BlockEntityInfo> = block_entities
        .iter()
        .map(|(entity, data)| BlockEntityInfo {
            data: data.as_deref(),
            ..*entity
        })
        .collect();

    let sky = LightSections::collect(store.light(), LightLayer::Sky, pos, ALL_SECTIONS);
    let block = LightSections::collect(store.light(), LightLayer::Block, pos, ALL_SECTIONS);
    with_light_data(&sky, &block, |light| {
        encode_chunk_body(&heightmaps, &sections, &block_entities, light)
    })
}

/// 区块包里要带的高度图：官方 `Types.sendToClient` 的那几种，键用官方的 `id`。
fn client_heightmaps(chunk: &Chunk) -> Vec<(i32, Vec<u64>)> {
    heightmaps::TYPES
        .iter()
        .filter(|heightmap| heightmap.send_to_client)
        .map(|heightmap| {
            let kind = HeightmapKind::of_type(heightmap).expect("发给客户端的高度图在区块里都留着");
            (heightmap.id, chunk.heightmap(kind))
        })
        .collect()
}

/// 一层光照里选中的段，官方 `ClientboundLightUpdatePacketData.prepareSectionData` 的结果。
pub(crate) struct LightSections<'a> {
    mask: u64,
    empty_mask: u64,
    updates: Vec<Cow<'a, [u8]>>,
}

impl<'a> LightSections<'a> {
    /// `selected` 的第 n 位选中第 n 段。不存光照的段两个掩码里都没有，空数据只标在空掩码里。
    pub(crate) fn collect(
        light: &'a LevelLight,
        layer: LightLayer,
        pos: ChunkPos,
        selected: u32,
    ) -> Self {
        let mut sections = Self {
            mask: 0,
            empty_mask: 0,
            updates: Vec::new(),
        };
        for index in 0..LIGHT_SECTION_COUNT {
            if selected & 1 << index == 0 {
                continue;
            }
            let section = SectionPos::new(pos.x, MIN_LIGHT_SECTION + index as i32, pos.z);
            match light.section(layer, section) {
                None => {}
                Some(data) if data.is_empty() => sections.empty_mask |= 1 << index,
                Some(data) => {
                    sections.mask |= 1 << index;
                    sections.updates.push(data.bytes());
                }
            }
        }
        sections
    }

    fn updates(&self) -> Vec<&[u8]> {
        self.updates.iter().map(AsRef::as_ref).collect()
    }
}

/// 把两层光照合成网络上的光照数据，交给 `write`。
pub(crate) fn with_light_data<R>(
    sky: &LightSections<'_>,
    block: &LightSections<'_>,
    write: impl FnOnce(&LightData<'_>) -> R,
) -> R {
    let sky_updates = sky.updates();
    let block_updates = block.updates();
    write(&LightData {
        sky_mask: sky.mask,
        block_mask: block.mask,
        empty_sky_mask: sky.empty_mask,
        empty_block_mask: block.empty_mask,
        sky_updates: &sky_updates,
        block_updates: &block_updates,
    })
}

fn write_section(out: &mut Vec<u8>, section: &Section) {
    write_section_header(out, section.block_count(), section.fluid_count());
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
    use mc_bamboo_core::sign::SignContent;
    use mc_bamboo_data::blocks;

    use super::*;

    /// `blocks-inventory.md` 第 11 章的实测字节：区块内 (3, -60, 5) 的箱子。
    const CHEST_ENTRY: [u8; 5] = [0x35, 0xff, 0xc4, 0x01, 0x00];

    /// 区块内 (3, -60, 5) 的告示牌：类型 ID 之后是 NBT 根 Compound，网络格式的根没有名字，
    /// 所以接着就是第一个条目 `front_text` 的类型和名字。
    const SIGN_ENTRY: [u8; 8] = [0x35, 0xff, 0xc4, 0x07, 0x0a, 0x0a, 0x00, 0x0a];

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|part| part == needle)
    }

    #[test]
    fn untouched_chunks_share_the_template_bytes() {
        let store = ChunkStore::flat();
        let template_body = encode_template(&store);
        let mut cache = ChunkCache::default();

        let body = chunk_body(&mut cache, &store, &template_body, ChunkPos::new(3, -9));

        assert!(matches!(body, Cow::Borrowed(_)));
        assert_eq!(*body, *template_body);
    }

    #[test]
    fn edited_chunks_are_encoded_with_their_current_content() {
        let mut store = ChunkStore::flat();
        let template_body = encode_template(&store);
        let mut cache = ChunkCache::default();
        let pos = BlockPos::new(1, -60, 1);

        store.set_block(pos, blocks::STONE);
        let edited = chunk_body(&mut cache, &store, &template_body, pos.chunk());
        assert_ne!(*edited, *template_body);
        assert_eq!(*edited, *encode_chunk(&store, pos.chunk()));
    }

    #[test]
    fn chunks_lit_from_a_neighbor_are_encoded_on_their_own() {
        let mut store = ChunkStore::flat();
        let template_body = encode_template(&store);
        let mut cache = ChunkCache::default();

        store.set_block(BlockPos::new(15, -60, 3), blocks::GLOWSTONE);
        store.finish_light_updates();

        // 方块在 (0, 0)，光照进了 (1, 0)。
        let neighbor = ChunkPos::new(1, 0);
        let lit = chunk_body(&mut cache, &store, &template_body, neighbor);
        assert_ne!(*lit, *template_body);
        assert_eq!(*lit, *encode_chunk(&store, neighbor));
    }

    #[test]
    fn flat_chunk_light_matches_the_official_layout() {
        let store = ChunkStore::flat();
        let sky =
            LightSections::collect(store.light(), LightLayer::Sky, TEMPLATE_POS, ALL_SECTIONS);
        let block =
            LightSections::collect(store.light(), LightLayer::Block, TEMPLATE_POS, ALL_SECTIONS);

        // 世界下面那段空着，地面那段和它上面那段有数据，再往上不存。方块光三段都是空的。
        assert_eq!((sky.mask, sky.empty_mask), (0b110, 0b001));
        assert_eq!((block.mask, block.empty_mask), (0, 0b111));
        assert_eq!(sky.updates.len(), 2);
    }

    #[test]
    fn block_entities_are_written_with_positions_inside_the_chunk() {
        let mut store = ChunkStore::flat();
        let template_body = encode_template(&store);
        let mut cache = ChunkCache::default();
        // 这个箱子在 (1, -1) 区块，世界坐标不是区块内坐标。
        let pos = BlockPos::new(19, -60, -11);

        store.set_block(pos, blocks::CHEST);

        let body = chunk_body(&mut cache, &store, &template_body, pos.chunk());
        assert!(contains(&body, &CHEST_ENTRY));
    }

    #[test]
    fn flat_chunk_has_no_block_entities() {
        let body = encode_template(&ChunkStore::flat());
        assert!(!contains(&body, &CHEST_ENTRY));
    }

    #[test]
    fn blank_signs_are_written_with_an_empty_tag() {
        let mut store = ChunkStore::flat();
        let template_body = encode_template(&store);
        let mut cache = ChunkCache::default();
        let pos = BlockPos::new(19, -60, -11);

        store.set_block(pos, blocks::OAK_SIGN);

        let body = chunk_body(&mut cache, &store, &template_body, pos.chunk());
        assert!(contains(&body, &SIGN_ENTRY));
    }

    #[test]
    fn written_signs_carry_their_text_in_the_chunk() {
        let mut store = ChunkStore::flat();
        let template_body = encode_template(&store);
        let mut cache = ChunkCache::default();
        let pos = BlockPos::new(19, -60, -11);
        let mut content = SignContent::default();
        content.front.messages[0] = "hello".to_owned();

        store.set_block(pos, blocks::OAK_SIGN);
        store.set_sign(pos, content);

        let body = chunk_body(&mut cache, &store, &template_body, pos.chunk());
        assert!(contains(&body, &SIGN_ENTRY));
        // `messages` 列表里的第一行，字符串标签的长度前缀是两字节。
        assert!(contains(&body, b"\x00\x05hello"));
    }
}
