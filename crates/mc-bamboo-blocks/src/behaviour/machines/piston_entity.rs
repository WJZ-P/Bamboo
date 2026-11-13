//! `PistonMovingBlockEntity`：活塞动画期间那一格上的方块实体。
//!
//! 活塞把要移动的方块换成 `moving_piston` 并挂上这个实体，实体每刻推进半格，
//! 到一格时把真正的方块放回去。世界里没有实体，所以官方的推实体和粘实体两段没写。

use std::any::Any;

use mc_bamboo_core::{BlockPos, Direction};
use mc_bamboo_data::{BlockClass, blocks};
use mc_bamboo_nbt::Tag;

use crate::{
    block_entity::BlockEntity,
    shape::{self, Boxes, CollisionContext},
    state::{BlockState, EXTENDED, FACING, SHORT, WATERLOGGED},
    world::{
        UPDATE_ALL, UPDATE_CLIENTS, UPDATE_INVISIBLE, UPDATE_KNOWN_SHAPE, UPDATE_LIMIT,
        UPDATE_MOVE_BY_PISTON, UPDATE_NEIGHBORS, UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS, World,
    },
};

/// 每刻推进的进度，官方的 `progress += 0.5F`。
const PROGRESS_PER_TICK: f32 = 0.5;
/// `SHORT` 的分界线，`1.0F - progress < 0.25F`。
const SHORT_THRESHOLD: f32 = 0.25;

/// 一次移动。
pub(crate) struct PistonMoving {
    pos: BlockPos,
    moved_state: BlockState,
    direction: Direction,
    extending: bool,
    is_source_piston: bool,
    progress: f32,
    progress_o: f32,
}

impl PistonMoving {
    /// `PistonMovingBlockEntity(worldPosition, blockState, movedState, direction, extending, isSourcePiston)`。
    ///
    /// 官方还会存 `blockState`（`moving_piston` 自己的状态），这里不读它，不存。
    pub(crate) fn new(
        pos: BlockPos,
        moved_state: BlockState,
        direction: Direction,
        extending: bool,
        is_source_piston: bool,
    ) -> Self {
        Self {
            pos,
            moved_state,
            direction,
            extending,
            is_source_piston,
            progress: 0.0,
            progress_o: 0.0,
        }
    }

    /// `isExtending`。
    pub(super) const fn is_extending(&self) -> bool {
        self.extending
    }

    /// `getDirection`。
    pub(super) const fn direction(&self) -> Direction {
        self.direction
    }

    /// `getProgress(a)`：上一次和这一次的进度之间插值。
    pub(super) fn progress(&self, a: f32) -> f32 {
        let factor = a.min(1.0);
        self.progress_o + factor * (self.progress - self.progress_o)
    }

    /// 录制层要看到的全部内部状态；不再含绝对时刻。
    pub fn observed_facts(&self) -> (u32, u8, bool, bool, u32, u32) {
        (
            self.moved_state.0,
            self.direction.id() as u8,
            self.extending,
            self.is_source_piston,
            self.progress.to_bits(),
            self.progress_o.to_bits(),
        )
    }

    /// 存档读入：官方写的是 `progressO`，读回时 `progress` 和 `progressO` 相等。
    pub fn set_archive_progress(&mut self, progress: f32) {
        self.progress = progress;
        self.progress_o = progress;
    }

    /// `getExtendedProgress`：伸长时从 -1 走到 0，缩回时从 1 走到 0。
    fn extended_progress(&self, progress: f32) -> f32 {
        if self.extending {
            progress - 1.0
        } else {
            1.0 - progress
        }
    }

    /// `getCollisionShape`：源活塞缩回时还有一段活塞头的形状。
    ///
    /// 官方的 `NOCLIP` 只在推实体时设置，这里没有实体，所以永远走普通分支。
    pub(super) fn collision_boxes(&self) -> Boxes {
        let mut boxes = Vec::new();
        if !self.extending
            && self.is_source_piston
            && self.moved_state.class() == BlockClass::PistonBaseBlock
        {
            let extended = self.moved_state.with(EXTENDED, true);
            boxes.extend(
                shape::collision_boxes(extended, self.pos, CollisionContext::Empty).into_owned(),
            );
        }

        let short = self.extending != (1.0 - self.progress < SHORT_THRESHOLD);
        let block_state = if self.is_source_piston {
            BlockState(blocks::PISTON_HEAD)
                .with(FACING, self.direction)
                .with(SHORT, short)
        } else {
            self.moved_state
        };

        let extended = self.extended_progress(self.progress);
        let (dx, dy, dz) = self.direction.normal();
        let moved = shape::moved(
            &shape::collision_boxes(block_state, self.pos, CollisionContext::Empty),
            f64::from(dx) * f64::from(extended),
            f64::from(dy) * f64::from(extended),
            f64::from(dz) * f64::from(extended),
        );
        boxes.extend(moved);
        std::borrow::Cow::Owned(boxes)
    }

    /// `finalTick`：立刻结束这一次移动，在方块被别的操作替换之前把方块放回去。
    pub(super) fn final_tick(&mut self, world: &mut World) {
        if self.progress_o >= 1.0 {
            return;
        }
        self.progress = 1.0;
        self.progress_o = 1.0;
        world.remove_block_entity(self.pos);
        if !world.block_state(self.pos).is(blocks::MOVING_PISTON) {
            return;
        }
        let new_state = if self.is_source_piston {
            BlockState::AIR
        } else {
            world.update_from_neighbour_shapes(self.moved_state, self.pos)
        };
        world.set_block_and_update(self.pos, new_state);
        world.neighbor_changed(self.pos, new_state.block());
    }

    /// `tick`：推进半格，走完一格时把方块放回去。官方每刻还推一遍实体，这里没有。
    fn tick(&mut self, world: &mut World) {
        self.progress_o = self.progress;
        if self.progress_o >= 1.0 {
            world.remove_block_entity(self.pos);
            if !world.block_state(self.pos).is(blocks::MOVING_PISTON) {
                return;
            }
            let new_state = world.update_from_neighbour_shapes(self.moved_state, self.pos);
            if new_state.is_air() {
                // 340：`UPDATE_INVISIBLE | UPDATE_MOVE_BY_PISTON | UPDATE_KNOWN_SHAPE |
                // UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS`。
                world.set_block(
                    self.pos,
                    self.moved_state,
                    UPDATE_INVISIBLE
                        | UPDATE_MOVE_BY_PISTON
                        | UPDATE_KNOWN_SHAPE
                        | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS,
                );
                world.update_or_destroy(
                    self.moved_state,
                    new_state,
                    self.pos,
                    UPDATE_ALL,
                    UPDATE_LIMIT,
                );
            } else {
                let new_state = if new_state.has(WATERLOGGED) && new_state.get(WATERLOGGED) {
                    new_state.with(WATERLOGGED, false)
                } else {
                    new_state
                };
                world.set_block(
                    self.pos,
                    new_state,
                    UPDATE_MOVE_BY_PISTON | UPDATE_CLIENTS | UPDATE_NEIGHBORS,
                );
                world.neighbor_changed(self.pos, new_state.block());
            }
            return;
        }

        self.progress += PROGRESS_PER_TICK;
        if self.progress >= 1.0 {
            self.progress = 1.0;
        }
    }
}

impl BlockEntity for PistonMoving {
    fn pos(&self) -> BlockPos {
        self.pos
    }

    fn tick(&mut self, world: &mut World) {
        self.tick(world);
    }

    fn pre_remove_side_effects(&mut self, world: &mut World, _pos: BlockPos, _state: BlockState) {
        self.final_tick(world);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn write_additional(&self) -> Result<Vec<(String, Tag)>, crate::archive::Error> {
        use crate::archive::tag::{encode_block_state, put};
        let mut out = Vec::new();
        put(&mut out, "blockState", encode_block_state(self.moved_state));
        put(&mut out, "facing", Tag::Byte(self.direction.id() as i8));
        // 官方 saveAdditional 写的是 progressO，不是这一刻的 progress。
        put(&mut out, "progress", Tag::Float(self.progress_o));
        put(&mut out, "extending", Tag::Byte(i8::from(self.extending)));
        put(
            &mut out,
            "source",
            Tag::Byte(i8::from(self.is_source_piston)),
        );
        Ok(out)
    }
}
