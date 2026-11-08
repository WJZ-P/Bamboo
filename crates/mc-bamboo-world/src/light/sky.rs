//! 天空光，官方的 `SkyLightEngine`。
//!
//! 每一列最低光源以上都是 15，这些格子叫光源。光源往下和往旁边照，照到被挡住的地方一格格变暗。
//! 一个区块段整段都不存光照时，光会直接穿过它：照到这种段上面的边界时，下面整列都跟着变。

use mc_bamboo_core::{BlockPos, ChunkPos, Direction};

use super::{
    Chunks, SectionPos,
    block::propagating_state,
    engine::{LightEngine, MAX_LEVEL, PULL_LIGHT_IN_ENTRY, entry},
    properties::{is_empty_shape, shape_occludes},
    sources::SkyLightSources,
};

/// `REMOVE_TOP_SKY_SOURCE_ENTRY`。
const REMOVE_TOP_SKY_SOURCE_ENTRY: u64 = entry::decrease_all_directions(MAX_LEVEL);
/// `REMOVE_SKY_SOURCE_ENTRY`。
const REMOVE_SKY_SOURCE_ENTRY: u64 = entry::decrease_skip_one_direction(MAX_LEVEL, Direction::Up);
/// `ADD_SKY_SOURCE_ENTRY`。
const ADD_SKY_SOURCE_ENTRY: u64 =
    entry::increase_skip_one_direction(MAX_LEVEL, false, Direction::Up);

/// 区块段的最低一格。
const fn section_bottom(section_y: i32) -> i32 {
    section_y << 4
}

impl LightEngine {
    /// `SkyLightEngine.checkNode`：先让这一列的光源跟上新的最低光源，再看这一格自己是不是光源。
    pub(super) fn check_sky_node(&mut self, chunks: Chunks<'_>, node: BlockPos) {
        let section = SectionPos::of(node);
        let lowest = if self.storage.light_on(node.chunk()) {
            chunks.lowest_source_y(node.x, node.z)
        } else {
            i32::MAX
        };
        if lowest != i32::MAX {
            self.update_sources_in_column(chunks, node.x, node.z, lowest);
        }
        if !self.storage.storing_light_for_section(section) {
            return;
        }
        if node.y >= lowest {
            self.enqueue_decrease(node, REMOVE_SKY_SOURCE_ENTRY);
            self.enqueue_increase(node, ADD_SKY_SOURCE_ENTRY);
            return;
        }
        let old = self.storage.stored_level(node);
        if old > 0 {
            self.storage.set_stored_level(node, 0);
            self.enqueue_decrease(node, entry::decrease_all_directions(old));
        } else {
            self.enqueue_decrease(node, PULL_LIGHT_IN_ENTRY);
        }
    }

    /// `updateSourcesInColumn`。
    fn update_sources_in_column(&mut self, chunks: Chunks<'_>, x: i32, z: i32, lowest: i32) {
        let world_bottom = section_bottom(self.storage.lowest_section());
        self.remove_sources_below(x, z, lowest, world_bottom);
        self.add_sources_above(chunks, x, z, lowest, world_bottom);
    }

    /// `removeSourcesBelow`：最低光源往上挪了，下面原来是 15 的格子清零，碰到不是 15 的就停。
    fn remove_sources_below(&mut self, x: i32, z: i32, lowest: i32, world_bottom: i32) {
        if lowest <= world_bottom {
            return;
        }
        let start_y = lowest - 1;
        let mut section = SectionPos::of(BlockPos::new(x, start_y, z));
        while self.storage.has_light_data_at_or_below(section.y) {
            if self.storage.storing_light_for_section(section) {
                let bottom = section_bottom(section.y);
                for y in (bottom..=start_y.min(bottom + 15)).rev() {
                    let node = BlockPos::new(x, y, z);
                    if self.storage.stored_level(node) != MAX_LEVEL {
                        return;
                    }
                    self.storage.set_stored_level(node, 0);
                    let data = if y == lowest - 1 {
                        REMOVE_TOP_SKY_SOURCE_ENTRY
                    } else {
                        REMOVE_SKY_SOURCE_ENTRY
                    };
                    self.enqueue_decrease(node, data);
                }
            }
            section = section.offset(0, -1, 0);
        }
    }

    /// `addSourcesAbove`：最低光源以上还不是 15 的格子点成 15，碰到已经是 15 的就停。
    /// 比四个邻居列的最低光源低的格子才需要往旁边照。
    fn add_sources_above(
        &mut self,
        chunks: Chunks<'_>,
        x: i32,
        z: i32,
        lowest: i32,
        world_bottom: i32,
    ) {
        let neighbor_lowest = chunks
            .lowest_source_y(x - 1, z)
            .max(chunks.lowest_source_y(x + 1, z))
            .max(
                chunks
                    .lowest_source_y(x, z - 1)
                    .max(chunks.lowest_source_y(x, z + 1)),
            );
        let start_y = lowest.max(world_bottom);
        let mut section = SectionPos::of(BlockPos::new(x, start_y, z));
        while !self.storage.is_above_data(section) {
            if self.storage.storing_light_for_section(section) {
                let bottom = section_bottom(section.y);
                for y in bottom.max(start_y)..=bottom + 15 {
                    let node = BlockPos::new(x, y, z);
                    if self.storage.stored_level(node) == MAX_LEVEL {
                        return;
                    }
                    self.storage.set_stored_level(node, MAX_LEVEL);
                    if y < neighbor_lowest || y == lowest {
                        self.enqueue_increase(node, ADD_SKY_SOURCE_ENTRY);
                    }
                }
            }
            section = section.above();
        }
    }

    /// `SkyLightEngine.propagateIncrease`。
    pub(super) fn propagate_sky_increase(
        &mut self,
        chunks: Chunks<'_>,
        from: BlockPos,
        data: u64,
        level: u8,
    ) {
        let mut from_state = None;
        let empty_below = self.count_empty_sections_below_if_at_border(from);
        for direction in Direction::ALL {
            if !entry::should_propagate_in_direction(data, direction) {
                continue;
            }
            let to = from.relative(direction);
            let Some((new_level, to_state)) = self.brighter_level(chunks, to, level) else {
                continue;
            };
            let from_state =
                *from_state.get_or_insert_with(|| propagating_state(chunks, from, data));
            if shape_occludes(from_state, to_state, direction) {
                continue;
            }
            self.storage.set_stored_level(to, new_level);
            if new_level > 1 {
                let data = entry::increase_skip_one_direction(
                    new_level,
                    is_empty_shape(to_state),
                    direction.opposite(),
                );
                self.enqueue_increase(to, data);
            }
            self.propagate_from_empty_sections(to, direction, new_level, true, empty_below);
        }
    }

    /// `SkyLightEngine.propagateDecrease`。
    pub(super) fn propagate_sky_decrease(&mut self, from: BlockPos, data: u64) {
        let empty_below = self.count_empty_sections_below_if_at_border(from);
        let old_level = entry::from_level(data);
        for direction in Direction::ALL {
            if !entry::should_propagate_in_direction(data, direction) {
                continue;
            }
            let to = from.relative(direction);
            if !self.storage.storing_light_for_section(SectionPos::of(to)) {
                continue;
            }
            let level = self.storage.stored_level(to);
            if level == 0 {
                continue;
            }
            if level < old_level {
                self.storage.set_stored_level(to, 0);
                let data = entry::decrease_skip_one_direction(level, direction.opposite());
                self.enqueue_decrease(to, data);
                self.propagate_from_empty_sections(to, direction, level, false, empty_below);
            } else {
                let data = entry::increase_only_one_direction(level, false, direction.opposite());
                self.enqueue_increase(to, data);
            }
        }
    }

    /// `countEmptySectionsBelowIfAtBorder`：站在区块段底面的边上时，下面连着几段不存光照。
    fn count_empty_sections_below_if_at_border(&self, node: BlockPos) -> i32 {
        let (x, z) = (node.x & 15, node.z & 15);
        let on_border = x == 0 || x == 15 || z == 0 || z == 15;
        if node.y & 15 != 0 || !on_border {
            return 0;
        }
        let section = SectionPos::of(node);
        let mut empty = 0;
        loop {
            let below = section.offset(0, -empty - 1, 0);
            if self.storage.storing_light_for_section(below)
                || !self.storage.has_light_data_at_or_below(below.y)
            {
                return empty;
            }
            empty += 1;
        }
    }

    /// `propagateFromEmptySections`：光横着跨进相邻的区块柱，而起点下面有不存光照的段，
    /// 光会直穿那些段，所以下面那几段里同一列的存光照的段整列跟着变。
    fn propagate_from_empty_sections(
        &mut self,
        to: BlockPos,
        direction: Direction,
        level: u8,
        increase: bool,
        empty_below: i32,
    ) {
        if empty_below == 0 || !crossed_section_edge(direction, to.x & 15, to.z & 15) {
            return;
        }
        let top = SectionPos::of(to).y - 1;
        for section_y in (top - empty_below + 1..=top).rev() {
            let section = SectionPos::new(to.x >> 4, section_y, to.z >> 4);
            if !self.storage.storing_light_for_section(section) {
                continue;
            }
            for y in (section_bottom(section_y)..=section_bottom(section_y) + 15).rev() {
                let node = BlockPos::new(to.x, y, to.z);
                if increase {
                    self.storage.set_stored_level(node, level);
                    if level > 1 {
                        let data =
                            entry::increase_skip_one_direction(level, true, direction.opposite());
                        self.enqueue_increase(node, data);
                    }
                } else {
                    self.storage.set_stored_level(node, 0);
                    let data = entry::decrease_skip_one_direction(level, direction.opposite());
                    self.enqueue_decrease(node, data);
                }
            }
        }
    }

    /// `SkyLightEngine.propagateLightSources`：打开光照，从最高的存光照的段往下把光源点成 15，
    /// 光源下沿和比邻居列低的光源往外照。一段里没有更低的光源时就停。
    pub(super) fn propagate_sky_light_sources(&mut self, chunks: Chunks<'_>, pos: ChunkPos) {
        self.storage.set_light_enabled(pos, true);
        let sources = Neighborhood::of(chunks, pos);
        let top = self.storage.top_section_y(pos);
        let bottom = self.storage.lowest_section();
        for section_y in (bottom..top).rev() {
            let section = SectionPos::new(pos.x, section_y, pos.z);
            if !self.storage.storing_light_for_section(section) {
                continue;
            }
            if !self.light_section_sources(&sources, section) {
                break;
            }
        }
    }

    /// 一段里的光源。返回这一段下面还有没有光源。
    fn light_section_sources(&mut self, sources: &Neighborhood<'_>, section: SectionPos) -> bool {
        let section_min = section_bottom(section.y);
        let section_max = section_min + 15;
        let mut sources_below = false;
        for z in 0..16 {
            for x in 0..16 {
                let lowest = sources.center.lowest_source_y(x, z);
                if lowest > section_max {
                    continue;
                }
                let around = sources.around(x, z);
                let neighbor_lowest = around.into_iter().max().unwrap_or(i32::MIN);
                let [north, south, west, east] = around;
                for y in (section_min.max(lowest)..=section_max).rev() {
                    let layer = self
                        .storage
                        .layer_to_write(section)
                        .expect("调用方确认过这一段存着光照");
                    layer.set(x, (y & 15) as usize, z, MAX_LEVEL);
                    if y == lowest || y < neighbor_lowest {
                        let node =
                            BlockPos::new(section.x * 16 + x as i32, y, section.z * 16 + z as i32);
                        let data = entry::increase_sky_source_in_directions(
                            y == lowest,
                            y < north,
                            y < south,
                            y < west,
                            y < east,
                        );
                        self.enqueue_increase(node, data);
                    }
                }
                sources_below |= lowest < section_min;
            }
        }
        sources_below
    }
}

/// `crossedSectionEdge`：往这个方向走一格，正好跨进了相邻的区块柱。
fn crossed_section_edge(direction: Direction, x: i32, z: i32) -> bool {
    match direction {
        Direction::North => z == 15,
        Direction::South => z == 0,
        Direction::West => x == 15,
        Direction::East => x == 0,
        Direction::Down | Direction::Up => false,
    }
}

/// 一个区块和四个邻居的天空光源表。
struct Neighborhood<'a> {
    center: &'a SkyLightSources,
    north: &'a SkyLightSources,
    south: &'a SkyLightSources,
    west: &'a SkyLightSources,
    east: &'a SkyLightSources,
}

impl<'a> Neighborhood<'a> {
    fn of(chunks: Chunks<'a>, pos: ChunkPos) -> Self {
        let sources = |x, z| chunks.chunk(ChunkPos::new(x, z)).sky_light_sources();
        Self {
            center: sources(pos.x, pos.z),
            north: sources(pos.x, pos.z - 1),
            south: sources(pos.x, pos.z + 1),
            west: sources(pos.x - 1, pos.z),
            east: sources(pos.x + 1, pos.z),
        }
    }

    /// 北、南、西、东四列的最低光源，跨出区块时读邻居的表。
    fn around(&self, x: usize, z: usize) -> [i32; 4] {
        let center = self.center;
        [
            if z == 0 {
                self.north.lowest_source_y(x, 15)
            } else {
                center.lowest_source_y(x, z - 1)
            },
            if z == 15 {
                self.south.lowest_source_y(x, 0)
            } else {
                center.lowest_source_y(x, z + 1)
            },
            if x == 0 {
                self.west.lowest_source_y(15, z)
            } else {
                center.lowest_source_y(x - 1, z)
            },
            if x == 15 {
                self.east.lowest_source_y(0, z)
            } else {
                center.lowest_source_y(x + 1, z)
            },
        ]
    }
}
