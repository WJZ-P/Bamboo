//! 地形缓存冷层用的调色板压缩：一条 CARVERS 地形 414 720 B → 约 20 KiB。
//!
//! 和官方 `PalettedContainer` 同一个思路，但不受存档 / 协议格式约束（这份数据
//! 只活在内存里）：方块按 16 层一段切成 24 段，每段一张调色板 + 每格
//! `ceil(log2(调色板长度))` 位的紧排索引，只有一种方块的段 0 位。群系、高度图、
//! `PostProcessing` 很小，原样留着。解码结果和原始地形逐格相同（有测试）。
//!
//! 实测（`examples/terrain_compact_bench.rs`，`performance-optimizations.md`
//! 第 23 节）：压缩 20 倍，编码约 130 µs，解码约 57 µs。
//!
//! Palette compression for the terrain cache's cold tier: one CARVERS entry
//! shrinks from 414 720 B to about 20 KiB. Same idea as vanilla's
//! `PalettedContainer` without the save / wire format constraints (this data only
//! lives in memory): 24 sections of 16 layers, each a palette plus tightly packed
//! `ceil(log2(palette len))`-bit indices, 0 bits for a uniform section. Biomes,
//! heightmaps and `PostProcessing` are small and kept as they are. Decoding
//! returns exactly the original terrain (tested). Measured: 20x smaller, about
//! 130 µs to encode and 57 µs to decode.

use std::collections::HashMap;

use crate::worldgen::{GeneratedChunk, generated_chunk_heap_bytes};

const LAYERS_PER_SECTION: usize = 16;
const CELLS_PER_LAYER: usize = 256;
const CELLS_PER_SECTION: usize = LAYERS_PER_SECTION * CELLS_PER_LAYER;

/// 一段 16 层：调色板 + 紧排索引。
/// One 16-layer section: a palette and packed indices.
struct CompactSection {
    palette: Vec<u32>,
    bits: u32,
    data: Vec<u64>,
}

/// 一条压缩后的地形；方块以外的部分放在 `rest` 里原样保留（它的 `blocks` 是空的）。
///
/// One compacted terrain entry; everything but the blocks is kept as is in
/// `rest` (whose `blocks` is empty).
pub struct CompactTerrain {
    sections: Vec<CompactSection>,
    /// 原始地形一共多少层（最后一段可以不满 16 层）。
    /// Total layer count of the original (the last section may be short).
    layers: usize,
    rest: GeneratedChunk,
}

/// 能区分 `palette_len` 种方块的最少位数。
/// Fewest bits that tell `palette_len` states apart.
fn bits_for(palette_len: usize) -> u32 {
    if palette_len <= 1 {
        0
    } else {
        usize::BITS - (palette_len - 1).leading_zeros()
    }
}

/// 给一段的每一格找调色板下标，顺手建出调色板。
/// Find every cell's palette index, building the palette on the way.
fn palette_indices(layers: &[Vec<u32>]) -> (Vec<u32>, Vec<u32>) {
    let mut palette: Vec<u32> = Vec::new();
    let mut lookup: HashMap<u32, u32> = HashMap::new();
    let mut indices = Vec::with_capacity(CELLS_PER_SECTION);
    // 上一格的（状态, 下标）；用 `Option` 而不是哨兵值，任何 `u32` 状态都合法。
    // The previous cell's (state, index); an `Option` rather than a sentinel so
    // every `u32` state is valid.
    let mut last: Option<(u32, u32)> = None;
    for layer in layers {
        for &state in layer {
            // 相邻格大多相同：先比上一格，省一次哈希查找。
            // Neighboring cells mostly repeat: compare with the previous one first.
            let index = match last {
                Some((previous, index)) if previous == state => index,
                _ => {
                    let index = *lookup.entry(state).or_insert_with(|| {
                        palette.push(state);
                        palette.len() as u32 - 1
                    });
                    last = Some((state, index));
                    index
                }
            };
            indices.push(index);
        }
    }
    (palette, indices)
}

fn encode_section(layers: &[Vec<u32>]) -> CompactSection {
    let (palette, indices) = palette_indices(layers);
    let bits = bits_for(palette.len());
    let width = bits as usize;
    let mut data = vec![0_u64; (indices.len() * width).div_ceil(64)];
    if bits > 0 {
        for (cell, &index) in indices.iter().enumerate() {
            let offset = cell * width;
            data[offset / 64] |= u64::from(index) << (offset % 64);
            // 跨两个字的索引把高位写进下一个字。
            // An index straddling two words puts its high bits in the next one.
            if offset % 64 + width > 64 {
                data[offset / 64 + 1] |= u64::from(index) >> (64 - offset % 64);
            }
        }
    }
    CompactSection {
        palette,
        bits,
        data,
    }
}

fn decode_layer(section: &CompactSection, layer: usize) -> Vec<u32> {
    let width = section.bits as usize;
    if width == 0 {
        return vec![section.palette[0]; CELLS_PER_LAYER];
    }
    let mask = (1_u64 << width) - 1;
    let mut cells = Vec::with_capacity(CELLS_PER_LAYER);
    for column in 0..CELLS_PER_LAYER {
        let offset = (layer * CELLS_PER_LAYER + column) * width;
        let mut index = section.data[offset / 64] >> (offset % 64);
        if offset % 64 + width > 64 {
            index |= section.data[offset / 64 + 1] << (64 - offset % 64);
        }
        cells.push(section.palette[(index & mask) as usize]);
    }
    cells
}

impl CompactTerrain {
    /// 压缩一条地形。
    /// Compact one terrain entry.
    pub fn encode(chunk: &GeneratedChunk) -> Self {
        Self {
            sections: chunk
                .blocks
                .chunks(LAYERS_PER_SECTION)
                .map(encode_section)
                .collect(),
            layers: chunk.blocks.len(),
            rest: GeneratedChunk {
                blocks: Vec::new(),
                biomes: chunk.biomes.clone(),
                heightmaps: chunk.heightmaps.clone(),
                post_processing: chunk.post_processing.clone(),
            },
        }
    }

    /// 还原出和压缩前逐格相同的地形。
    /// Rebuild terrain identical to what was encoded.
    pub fn decode(&self) -> GeneratedChunk {
        let mut blocks = Vec::with_capacity(self.layers);
        for section in &self.sections {
            let layers = LAYERS_PER_SECTION.min(self.layers - blocks.len());
            blocks.extend((0..layers).map(|layer| decode_layer(section, layer)));
        }
        GeneratedChunk {
            blocks,
            biomes: self.rest.biomes.clone(),
            heightmaps: self.rest.heightmaps.clone(),
            post_processing: self.rest.post_processing.clone(),
        }
    }

    /// 这条压缩地形占的堆字节数（各 `Vec` 的容量相加）。
    /// Heap bytes held by this entry (sum of each `Vec`'s capacity).
    pub fn heap_bytes(&self) -> usize {
        let sections: usize = self
            .sections
            .iter()
            .map(|section| {
                section.palette.capacity() * size_of::<u32>()
                    + section.data.capacity() * size_of::<u64>()
            })
            .sum();
        sections
            + self.sections.capacity() * size_of::<CompactSection>()
            + generated_chunk_heap_bytes(&self.rest)
            + self.rest.post_processing.capacity() * size_of::<(i32, i32, i32)>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_options::{LevelType, WorldGenConfig};

    #[test]
    fn bits_cover_the_palette() {
        assert_eq!(bits_for(0), 0);
        assert_eq!(bits_for(1), 0);
        assert_eq!(bits_for(2), 1);
        assert_eq!(bits_for(3), 2);
        assert_eq!(bits_for(16), 4);
        assert_eq!(bits_for(17), 5);
        assert_eq!(bits_for(4096), 12);
    }

    /// 每格都不同的一段要 12 位，索引会跨 64 位字的边界。
    /// A section of all-different cells needs 12 bits, so indices straddle words.
    #[test]
    fn a_section_of_distinct_states_round_trips() {
        let layers: Vec<Vec<u32>> = (0..LAYERS_PER_SECTION as u32)
            .map(|y| {
                (0..CELLS_PER_LAYER as u32)
                    .map(|c| y * 1000 + c * 7 + 3)
                    .collect()
            })
            .collect();
        let section = encode_section(&layers);
        assert_eq!(section.bits, 12);
        for (y, layer) in layers.iter().enumerate() {
            assert_eq!(&decode_layer(&section, y), layer);
        }
    }

    /// 第一格就是 `u32::MAX` 的段也要拿到自己的调色板下标。
    /// A section whose first cell is `u32::MAX` still gets its own palette entry.
    #[test]
    fn a_max_state_first_cell_round_trips() {
        let mut layers = vec![vec![7_u32; CELLS_PER_LAYER]; LAYERS_PER_SECTION];
        layers[0][0] = u32::MAX;
        let section = encode_section(&layers);
        assert_eq!(section.palette, vec![u32::MAX, 7]);
        for (y, layer) in layers.iter().enumerate() {
            assert_eq!(&decode_layer(&section, y), layer);
        }
    }

    #[test]
    fn real_terrain_round_trips_and_shrinks() {
        let config = WorldGenConfig {
            seed: 0x5eed_c0de,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        for (x, z) in [(0, 0), (-9, 14)] {
            let chunk = crate::worldgen::generate_chunk(&config, x, z).expect("terrain");
            let compact = CompactTerrain::encode(&chunk);
            let back = compact.decode();
            assert_eq!(back.blocks, chunk.blocks);
            assert_eq!(back.biomes, chunk.biomes);
            assert_eq!(back.heightmaps, chunk.heightmaps);
            assert_eq!(back.post_processing, chunk.post_processing);
            assert!(compact.heap_bytes() * 4 < generated_chunk_heap_bytes(&chunk));
        }
    }
}
