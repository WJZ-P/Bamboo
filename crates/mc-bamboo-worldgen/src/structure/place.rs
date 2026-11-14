//! `StructurePiece.postProcess` 写方块用的公共原语：`placeBlock` / `generateBox` /
//! `generateAirBox` / `fillColumnDown` / `getWorldPos` 等。
//!
//! 官方 `StructurePiece` 的这些辅助函数都建立在 `getWorldX/Y/Z` 之上：结构件自己的
//! 包围盒最小角 + 局部坐标，再按 `orientation` 决定 X / Z 是否翻转（见 `getWorldX` /
//! `getWorldZ` 的 switch）。`placeBlock` 还按结构件的 `mirror` / `rotation` 变换方块状态，
//! 再 `setBlock(pos, state, 2)`。这里把这一层抽成 [`PieceContext`]，让各结构的
//! `postProcess` 只写「相对坐标 + 方块状态」。
//!
//! Writing primitives of `StructurePiece.postProcess`: `placeBlock` / `generateBox` /
//! `generateAirBox` / `fillColumnDown` / `getWorldPos` and friends. Vanilla builds all of
//! them on `getWorldX/Y/Z` (the piece box's min corner plus local coordinates, with X / Z
//! flipped according to `orientation`, see the `getWorldX` / `getWorldZ` switches).
//! `placeBlock` also transforms the state by the piece's `mirror` / `rotation` before
//! `setBlock(pos, state, 2)`. [`PieceContext`] wraps that layer so each structure's
//! `postProcess` only deals in local coordinates and states.

use mc_bamboo_data::blocks;

use crate::{
    feature::world::FeatureWorld,
    structure::pieces::{Box, HDir},
};

/// 官方 `StructurePiece.postProcess` 的 `level` 和 `chunkBB` 两件参数，外加结构件自己的
/// 朝向、rotation 和 mirror。
///
/// The `level` and `chunkBB` parameters of vanilla `StructurePiece.postProcess`, plus the
/// piece's own orientation, rotation and mirror.
/// 一次写入：`(x, y, z, state)`。
///
/// One write: `(x, y, z, state)`.
pub type Write = (i32, i32, i32, u32);

/// 写入日志（可选），给对拍用；`None` 时零开销。
///
/// Optional write log for the oracle; `None` costs nothing.
pub type WriteLog = Option<Vec<Write>>;

/// 一个  的产物：高度校正结果和写入日志。
///
/// A  result: the height adjustment and the write log.
pub type Placed = (Option<(i32, [i32; 6])>, WriteLog);

pub struct PieceContext<'a, 'b> {
    pub world: &'a mut FeatureWorld<'b>,
    /// `chunkBB`：只在这个盒子里写字，出了盒子 `placeBlock` 直接丢弃。
    ///
    /// `chunkBB`: writes are dropped outside this box.
    pub chunk_bb: Box,
    /// 结构件自己的包围盒（`getWorldX/Y/Z` 的基准：`boundingBox.minX()` / `maxX()`）。
    ///
    /// The piece's own bounding box (`getWorldX/Y/Z` base: `boundingBox.minX()` /
    /// `maxX()`).
    pub piece_bb: Box,
    /// `getOrientation()`：`None` 时局部坐标直接当世界坐标（`O = -1` 的结构件）。
    ///
    /// `getOrientation()`: when `None` local coordinates are world coordinates (`O = -1`).
    pub orientation: Option<HDir>,
    /// `getRotation().ordinal()` 和 `getMirror().ordinal()`，`-1` 表示 null。
    ///
    /// `getRotation().ordinal()` and `getMirror().ordinal()`, `-1` for null.
    pub rotation: i32,
    pub mirror: i32,
    /// 每次写入的 `(x, y, z, state)`，给对拍用。`None` 时不记录（生产路径零开销）。
    ///
    /// Every write as `(x, y, z, state)` for the oracle. `None` disables recording (zero cost
    /// on the production path).
    pub writes: WriteLog,
}

impl PieceContext<'_, '_> {
    /// `getWorldX(x, z)`。
    ///
    /// `getWorldX(x, z)`.
    pub fn world_x(&self, x: i32, z: i32) -> i32 {
        match self.orientation {
            None => x,
            Some(HDir::North | HDir::South) => self.piece_bb[0] + x,
            Some(HDir::West) => self.piece_bb[3] - z,
            Some(HDir::East) => self.piece_bb[0] + z,
        }
    }

    /// `getWorldY(y)`。
    ///
    /// `getWorldY(y)`.
    pub fn world_y(&self, y: i32) -> i32 {
        if self.orientation.is_none() {
            y
        } else {
            y + self.piece_bb[1]
        }
    }

    /// `getWorldZ(x, z)`。
    ///
    /// `getWorldZ(x, z)`.
    pub fn world_z(&self, x: i32, z: i32) -> i32 {
        match self.orientation {
            None => z,
            Some(HDir::North) => self.piece_bb[5] - z,
            Some(HDir::South) => self.piece_bb[2] + z,
            Some(HDir::West | HDir::East) => self.piece_bb[2] + x,
        }
    }

    /// `StructurePiece.getWorldPos(x, y, z)`。
    ///
    /// `StructurePiece.getWorldPos(x, y, z)`.
    pub fn world_pos(&self, x: i32, y: i32, z: i32) -> (i32, i32, i32) {
        (self.world_x(x, z), self.world_y(y), self.world_z(x, z))
    }

    /// `BoundingBox.isInside(pos)`。
    ///
    /// `BoundingBox.isInside(pos)`.
    pub fn inside(bb: &Box, pos: (i32, i32, i32)) -> bool {
        pos.0 >= bb[0]
            && pos.0 <= bb[3]
            && pos.1 >= bb[1]
            && pos.1 <= bb[4]
            && pos.2 >= bb[2]
            && pos.2 <= bb[5]
    }

    /// `placeBlock(level, state, x, y, z, chunkBB)`：按 `mirror` / `rotation` 变换后写方块。
    ///
    /// `placeBlock(level, state, x, y, z, chunkBB)`: transform by `mirror` / `rotation`
    /// and write.
    pub fn place_block(&mut self, state: u32, x: i32, y: i32, z: i32) {
        let pos = self.world_pos(x, y, z);
        if !Self::inside(&self.chunk_bb, pos) {
            return;
        }
        let state = self.transform(state);
        if let Some(writes) = self.writes.as_mut() {
            writes.push((pos.0, pos.1, pos.2, state));
        }
        self.world.set_block(pos.0, pos.1, pos.2, state);
    }

    /// `StructurePiece.placeBlock` 里的状态变换：先 `mirror` 再 `rotation`。
    ///
    /// The state transform inside `StructurePiece.placeBlock`: `mirror` first, then
    /// `rotation`.
    fn transform(&self, state: u32) -> u32 {
        let mut state = state;
        if self.mirror > 0 {
            state = mc_bamboo_data::state_mirror(state, self.mirror as u32);
        }
        if self.rotation > 0 {
            state = mc_bamboo_data::state_rotation(state, self.rotation as u32);
        }
        state
    }

    /// `getBlock(level, x, y, z, chunkBB)`：盒子外返回空气。
    ///
    /// `getBlock(level, x, y, z, chunkBB)`: air outside the box.
    pub fn block(&self, x: i32, y: i32, z: i32) -> u32 {
        let pos = self.world_pos(x, y, z);
        if !Self::inside(&self.chunk_bb, pos) {
            return blocks::AIR;
        }
        self.world.get_block(pos.0, pos.1, pos.2)
    }

    /// `generateBox(level, chunkBB, x0..z1, edge, fill, skipAir)`：逐格按是否为边界选
    /// edge / fill。遍历顺序是 y → x → z，和官方一致（会影响 `placeBlock` 的顺序，
    /// 后续有随机取数的结构件依赖它）。
    ///
    /// `generateBox`: walk y → x → z (the vanilla order, which later random-consuming
    /// pieces rely on) and pick edge or fill by whether the cell is on the boundary.
    #[allow(clippy::too_many_arguments)]
    pub fn generate_box(
        &mut self,
        (x0, y0, z0): (i32, i32, i32),
        (x1, y1, z1): (i32, i32, i32),
        edge: u32,
        fill: u32,
        skip_air: bool,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if skip_air && self.block(x, y, z) == blocks::AIR {
                        continue;
                    }
                    let state = if y != y0 && y != y1 && x != x0 && x != x1 && z != z0 && z != z1 {
                        fill
                    } else {
                        edge
                    };
                    self.place_block(state, x, y, z);
                }
            }
        }
    }

    /// `generateAirBox(level, chunkBB, x0..z1)`。
    ///
    /// `generateAirBox(level, chunkBB, x0..z1)`.
    pub fn generate_air_box(
        &mut self,
        (x0, y0, z0): (i32, i32, i32),
        (x1, y1, z1): (i32, i32, i32),
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    self.place_block(blocks::AIR, x, y, z);
                }
            }
        }
    }

    /// `fillColumnDown(level, state, x, startY, z, chunkBB)`：从 `startY` 往下填到
    /// 「不再可被结构替换」或 `minY + 1` 为止。
    ///
    /// `fillColumnDown`: fill downward from `startY` until the block is no longer
    /// structure-replaceable or `minY + 1` is reached.
    pub fn fill_column_down(&mut self, state: u32, x: i32, start_y: i32, z: i32) {
        let pos = self.world_pos(x, start_y, z);
        if !Self::inside(&self.chunk_bb, pos) {
            return;
        }
        let min_y = self.world.min_y;
        let mut y = pos.1;
        while y > min_y + 1 {
            let current = self.world.get_block(pos.0, y, pos.2);
            if !replaceable_by_structures(current) {
                break;
            }
            if let Some(writes) = self.writes.as_mut() {
                writes.push((pos.0, y, pos.2, state));
            }
            self.world.set_block(pos.0, y, pos.2, state);
            y -= 1;
        }
    }
}

/// `StructurePiece.isReplaceableByStructures(state)`：空气、液体、发光地衣、海草、高海草。
///
/// `StructurePiece.isReplaceableByStructures(state)`: air, liquids, glow lichen,
/// seagrass and tall seagrass.
pub fn replaceable_by_structures(state: u32) -> bool {
    state == blocks::AIR
        || state == blocks::GLOW_LICHEN
        || state == blocks::SEAGRASS
        || state == blocks::TALL_SEAGRASS
        || mc_bamboo_data::state_fluid(state).fluid != mc_bamboo_data::registries::fluid::EMPTY
}
