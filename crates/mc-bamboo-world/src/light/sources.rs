//! 每一列天空光从哪里开始，官方的 `ChunkSkyLightSources`。
//!
//! 一列里从天上往下走，第一次被挡住的那条边以上都是“光源”，天空光是满的 15。
//! 这张表记的是那条边的高度，也就是最低的光源格。一整列都通到世界底下时是负无穷。

use mc_bamboo_data::blocks;

use super::properties::is_edge_occluded;
use crate::{MIN_Y, SECTION_SIZE, chunk::Chunk};

/// 表里能存的最低值，比世界底部再低一格，读出来时换成负无穷。
const SOURCES_MIN_Y: i32 = MIN_Y - 1;
/// `ChunkSkyLightSources.NEGATIVE_INFINITY`。
pub const NEGATIVE_INFINITY: i32 = i32::MIN;

/// 一个区块 256 列的最低光源高度。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkyLightSources {
    lowest: [i16; 256],
}

impl Default for SkyLightSources {
    /// 新区块还没算过，官方的位存储全是 0，也就是最低值。
    fn default() -> Self {
        Self {
            lowest: [SOURCES_MIN_Y as i16; 256],
        }
    }
}

impl SkyLightSources {
    const fn index(x: usize, z: usize) -> usize {
        x + z * 16
    }

    fn stored(&self, x: usize, z: usize) -> i32 {
        i32::from(self.lowest[Self::index(x, z)])
    }

    pub(crate) fn set(&mut self, x: usize, z: usize, y: i32) {
        self.lowest[Self::index(x, z)] = y as i16;
    }

    /// `fillFrom`：从区块的方块算出整张表。
    pub fn fill_from(chunk: &Chunk) -> Self {
        let mut sources = Self::default();
        let Some(top) = highest_filled_section(chunk) else {
            return sources;
        };
        for z in 0..16 {
            for x in 0..16 {
                let lowest = find_lowest_source_y(chunk, top, x, z).max(SOURCES_MIN_Y);
                sources.set(x, z, lowest);
            }
        }
        sources
    }

    /// `getLowestSourceY`，区块内坐标。整列都是光源时返回 [`NEGATIVE_INFINITY`]。
    pub fn lowest_source_y(&self, x: usize, z: usize) -> i32 {
        extend_sources_below_world(self.stored(x, z))
    }

    /// `getHighestLowestSourceY`：256 列里最高的那个最低光源。
    pub fn highest_lowest_source_y(&self) -> i32 {
        let highest = self
            .lowest
            .iter()
            .copied()
            .max()
            .unwrap_or(SOURCES_MIN_Y as i16);
        extend_sources_below_world(i32::from(highest))
    }
}

/// `update`：`(x, y, z)` 这一格换了方块之后这一列的新值，没变时返回 `None`。
///
/// 只看这一格的上下两条边。上面的边被挡住了，光源就停在那里；原来停在这里的边不再挡光，就往下找新的边。
pub(crate) fn updated_lowest_source(chunk: &Chunk, x: usize, y: i32, z: usize) -> Option<i32> {
    let current = chunk.sky_light_sources().stored(x, z);
    if y + 1 < current {
        return None;
    }
    let top = chunk.block_or_air(x, y + 1, z);
    let middle = chunk.block_or_air(x, y, z);
    if let Some(updated) = updated_edge(chunk, current, (y + 1, top), (y, middle), x, z) {
        return Some(updated);
    }
    let bottom = chunk.block_or_air(x, y - 1, z);
    updated_edge(chunk, current, (y, middle), (y - 1, bottom), x, z)
}

/// `updateEdge`：检查一条边，边上面那一格的高度就是这条边的高度。
fn updated_edge(
    chunk: &Chunk,
    current: i32,
    (top_y, top): (i32, u32),
    (bottom_y, bottom): (i32, u32),
    x: usize,
    z: usize,
) -> Option<i32> {
    if is_edge_occluded(top, bottom) {
        (top_y > current).then_some(top_y)
    } else if top_y == current {
        Some(find_lowest_source_below(chunk, bottom_y, bottom, x, z))
    } else {
        None
    }
}

/// `findLowestSourceBelow`：从 `start_y` 往下找第一条挡光的边。
fn find_lowest_source_below(chunk: &Chunk, start_y: i32, start: u32, x: usize, z: usize) -> i32 {
    let mut top_y = start_y;
    let mut top = start;
    let mut bottom_y = start_y - 1;
    while bottom_y >= SOURCES_MIN_Y {
        let bottom = chunk.block_or_air(x, bottom_y, z);
        if is_edge_occluded(top, bottom) {
            return top_y;
        }
        top = bottom;
        top_y = bottom_y;
        bottom_y -= 1;
    }
    SOURCES_MIN_Y
}

/// `findLowestSourceY`：从最高的非空区块段往下找。全是空气的区块段整段跳过。
fn find_lowest_source_y(chunk: &Chunk, top_section: usize, x: usize, z: usize) -> i32 {
    let mut top_y = section_bottom(top_section + 1);
    let mut bottom_y = top_y - 1;
    let mut top = blocks::AIR;
    for index in (0..=top_section).rev() {
        if chunk.sections()[index].is_empty() {
            top = blocks::AIR;
            top_y = section_bottom(index);
            bottom_y = top_y - 1;
            continue;
        }
        for _ in 0..SECTION_SIZE {
            let bottom = chunk.block(x, bottom_y, z);
            if is_edge_occluded(top, bottom) {
                return top_y;
            }
            top = bottom;
            top_y = bottom_y;
            bottom_y -= 1;
        }
    }
    SOURCES_MIN_Y
}

/// `getHighestFilledSectionIndex`。
fn highest_filled_section(chunk: &Chunk) -> Option<usize> {
    chunk
        .sections()
        .iter()
        .rposition(|section| !section.is_empty())
}

fn section_bottom(index: usize) -> i32 {
    MIN_Y + index as i32 * SECTION_SIZE
}

/// `extendSourcesBelowWorld`。
fn extend_sources_below_world(value: i32) -> i32 {
    if value == SOURCES_MIN_Y {
        NEGATIVE_INFINITY
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flat;

    fn update(chunk: &mut Chunk, x: usize, y: i32, z: usize, state: u32) {
        chunk.set_block(x, y, z, state);
        chunk.update_sky_light_sources(x, y, z);
    }

    #[test]
    fn flat_world_sources_start_above_the_grass() {
        let sources = SkyLightSources::fill_from(&flat::generate_chunk());
        assert_eq!(sources.lowest_source_y(0, 0), -60);
        assert_eq!(sources.lowest_source_y(15, 15), -60);
        assert_eq!(sources.highest_lowest_source_y(), -60);
    }

    #[test]
    fn an_empty_chunk_is_open_to_the_bottom() {
        let sources = SkyLightSources::fill_from(&Chunk::empty(0));
        assert_eq!(sources.lowest_source_y(3, 3), NEGATIVE_INFINITY);
    }

    #[test]
    fn placing_and_removing_a_roof_moves_the_source() {
        let mut chunk = flat::generate_chunk();
        chunk.initialize_light_sources();

        update(&mut chunk, 2, 10, 3, blocks::STONE);
        assert_eq!(chunk.sky_light_sources().lowest_source_y(2, 3), 11);

        // 玻璃不挡天空光，换成玻璃以后光源回到草方块上面。
        update(&mut chunk, 2, 10, 3, blocks::GLASS);
        assert_eq!(chunk.sky_light_sources().lowest_source_y(2, 3), -60);
    }

    #[test]
    fn digging_through_the_bottom_opens_the_column() {
        let mut chunk = flat::generate_chunk();
        chunk.initialize_light_sources();
        for y in -64..=-61 {
            update(&mut chunk, 7, y, 7, blocks::AIR);
        }
        assert_eq!(
            chunk.sky_light_sources().lowest_source_y(7, 7),
            NEGATIVE_INFINITY
        );
    }
}
