//! `JigsawPlacement.addPieces` 的递归：`tryPlacingChildren` + `SequencedPriorityIterator`。
//!
//! 中心结构件之后，官方用 `Placer` 反复从 `SequencedPriorityIterator`（高
//! `placementPriority` 先、同优先级 FIFO）取 `PieceState`，跑 `tryPlacingChildren`。
//! 每个 `PieceState` 带一个 `MutableObject<VoxelShape>`（`Rc<RefCell<FreeSpace>>`），
//! 兄弟 piece 之间共享；`attachInsideSource` 时改用局部 `sourceFree`。
//!
//! The recursion of `JigsawPlacement.addPieces`: `tryPlacingChildren` + a
//! `SequencedPriorityIterator` (higher `placementPriority` first, FIFO within a
//! priority). Each `PieceState` carries a `MutableObject<VoxelShape>` shared
//! between siblings; `attachInsideSource` switches to a local `sourceFree`.

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashSet, VecDeque},
    rc::Rc,
};

use mc_bamboo_blocks::LegacyRandom;

use crate::{
    chunk::Generator,
    random::{Random, WorldgenRandom},
    structure::BoundingBox,
};

use super::{
    alias::PoolAliasLookup,
    center_piece_with_random,
    config::JigsawConfig,
    element::{JigsawBlock, PoolElement, Projection, can_attach},
    pool::TemplatePools,
    rotation::Rotation,
    shapes::{Box4, FreeSpace},
};

/// 一个结构件（`PoolElementStructurePiece`），放置阶段可直接用这些字段。
///
/// 存档相关的两个基类字段是常量，所以不单独存：`PoolElementStructurePiece` 的构造器
/// 是 `super(StructurePieceType.JIGSAW, 0, boundingBox)`，即 `GD` 恒为 0；它从不调用
/// `setOrientation`，所以 `StructurePiece.getOrientation()` 是 null，`O` 写 -1。
///
/// A structure piece (`PoolElementStructurePiece`) for the placement stage.
/// The two save-only base fields are constants and thus not stored: the
/// `PoolElementStructurePiece` constructor is
/// `super(StructurePieceType.JIGSAW, 0, boundingBox)`, so `GD` is always 0; it never
/// calls `setOrientation`, so `StructurePiece.getOrientation()` stays null and `O`
/// is written as -1.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub element: PoolElement,
    pub position: (i32, i32, i32),
    pub rotation: Rotation,
    pub bounding_box: BoundingBox,
    pub ground_level_delta: i32,
    pub junctions: Vec<Junction>,
    /// `PoolElementStructurePiece.liquidSettings`：结构配置的 `liquid_settings`。
    /// 放置时 `SinglePoolElement` 用 `overrideLiquidSettings.orElse(liquidSettings)`。
    ///
    /// `PoolElementStructurePiece.liquidSettings`: the structure's `liquid_settings`.
    /// At placement `SinglePoolElement` uses
    /// `overrideLiquidSettings.orElse(liquidSettings)`.
    pub liquid_settings: LiquidSettings,
}

/// `LiquidSettings`：放置模板时是否给含水方块灌水。
///
/// `LiquidSettings`: whether waterloggable blocks get filled during placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquidSettings {
    IgnoreWaterlogging,
    ApplyWaterlogging,
}

impl LiquidSettings {
    /// `LiquidSettings.CODEC` 的名字。
    ///
    /// The names of `LiquidSettings.CODEC`.
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "ignore_waterlogging" => Ok(Self::IgnoreWaterlogging),
            "apply_waterlogging" => Ok(Self::ApplyWaterlogging),
            other => Err(format!("bad liquid_settings {other}")),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::IgnoreWaterlogging => "ignore_waterlogging",
            Self::ApplyWaterlogging => "apply_waterlogging",
        }
    }

    /// `JigsawStructure.DEFAULT_LIQUID_SETTINGS`。
    ///
    /// `JigsawStructure.DEFAULT_LIQUID_SETTINGS`.
    pub const DEFAULT: Self = Self::ApplyWaterlogging;
}

/// `JigsawJunction`：结构件之间的地形适配连接点。
///
/// A `JigsawJunction`: a terrain-adaptation link between pieces.
#[derive(Debug, Clone, PartialEq)]
pub struct Junction {
    pub source_x: i32,
    pub source_ground_y: i32,
    pub source_z: i32,
    pub delta_y: i32,
    pub dest_projection: Projection,
}

impl Piece {
    /// 转成存档用的 `StructurePiece`（`PiecesContainer.save` 的 `Children` 一项）。
    ///
    /// 官方 `PoolElementStructurePiece.addAdditionalSaveData` 的 `extras`：
    /// `PosX`/`PosY`/`PosZ`、`ground_level_delta`、`pool_element`、`rotation`
    /// （`Rotation.LEGACY_CODEC`，名字 `none`/`clockwise_90`/`180`/`counterclockwise_90`）、
    /// `junctions`（每项 `{source_x, source_ground_y, source_z, delta_y, dest_proj}`）。
    ///
    /// Convert to the save-side `StructurePiece` (one `Children` entry written by
    /// `PiecesContainer.save`). The `extras` mirror vanilla
    /// `PoolElementStructurePiece.addAdditionalSaveData`: `PosX`/`PosY`/`PosZ`,
    /// `ground_level_delta`, `pool_element`, `rotation` (`Rotation.LEGACY_CODEC`, names
    /// `none`/`clockwise_90`/`180`/`counterclockwise_90`) and `junctions` (each
    /// `{source_x, source_ground_y, source_z, delta_y, dest_proj}`).
    pub fn to_save_piece(&self) -> crate::structure::start::StructurePiece {
        crate::structure::start::StructurePiece {
            piece_type: "minecraft:jigsaw".to_owned(),
            bounding_box: self.bounding_box,
            // `setOrientation` 从未被调用，`O` 写 -1。
            // `setOrientation` is never called, so `O` is written as -1.
            orientation: -1,
            // `super(StructurePieceType.JIGSAW, 0, ...)`：`GD` 恒为 0。
            // `super(StructurePieceType.JIGSAW, 0, ...)`: `GD` is always 0.
            ground_level_delta: 0,
        }
    }
}

/// `addPieces` 的完整产出：stub 位置 + 全部结构件（含中心件）。
///
/// Full `addPieces` output: stub position + every piece (center included).
#[derive(Debug, Clone, PartialEq)]
pub struct StructurePieces {
    pub position: (i32, i32, i32),
    pub pieces: Vec<Piece>,
}

/// `SequencedPriorityIterator`：高优先级先，同优先级 FIFO。
///
/// `SequencedPriorityIterator`: higher priority first, FIFO within a priority.
struct PriorityQueue<T> {
    queues: BTreeMap<i32, VecDeque<T>>,
}

impl<T> Default for PriorityQueue<T> {
    fn default() -> Self {
        Self {
            queues: BTreeMap::new(),
        }
    }
}

impl<T> PriorityQueue<T> {
    fn add(&mut self, value: T, priority: i32) {
        self.queues.entry(priority).or_default().push_back(value);
    }

    fn next(&mut self) -> Option<T> {
        let key = *self.queues.iter().rev().find(|(_, q)| !q.is_empty())?.0;
        self.queues.get_mut(&key)?.pop_front()
    }
}

struct PieceState {
    piece_index: usize,
    free: Rc<RefCell<FreeSpace>>,
    depth: i32,
}

struct Placer<'a> {
    pools: &'a TemplatePools,
    generator: &'a Generator<'a>,
    random: &'a WorldgenRandom,
    config: &'a JigsawConfig,
    aliases: &'a PoolAliasLookup,
    liquid_settings: LiquidSettings,
    pieces: Vec<Piece>,
    placing: PriorityQueue<PieceState>,
}

/// `JigsawPlacement.addPieces`：中心结构件 + 完整递归。
///
/// `None` 对应官方 `Optional.empty()`（中心件生成阶段就失败）。
#[allow(clippy::too_many_arguments)]
pub fn add_pieces(
    config: &JigsawConfig,
    pools: &TemplatePools,
    generator: &Generator<'_>,
    allowed: &HashSet<String>,
    dims: (i32, i32, i32),
    seed: i64,
    chunk: (i32, i32),
) -> Result<Option<StructurePieces>, String> {
    let random = WorldgenRandom::new(Random::Legacy(LegacyRandom::new(0)));
    random.set_large_feature_seed(seed, chunk.0, chunk.1);
    let Some(point) = center_piece_with_random(
        config, pools, generator, allowed, dims, chunk, seed, &random,
    )?
    else {
        return Ok(None);
    };
    let center = point.center;
    let liquid_settings = LiquidSettings::parse(&config.liquid_settings)?;
    let mut pieces = vec![Piece {
        element: center.element,
        position: center.position,
        rotation: center.rotation,
        bounding_box: center.bounding_box,
        ground_level_delta: center.ground_level_delta,
        junctions: Vec::new(),
        liquid_settings,
    }];

    if config.max_depth > 0 {
        let (min_y, height, _) = dims;
        let max_y = min_y + height - 1;
        let (horizontal, vertical) = config.max_distance;
        let (pad_bottom, pad_top) = config.dimension_padding;
        let (cx, cy, cz) = point.position;
        let big = Box4::from_aabb(
            [
                cx - horizontal,
                (cy - vertical).max(min_y + pad_bottom),
                cz - horizontal,
            ],
            [
                cx + horizontal + 1,
                (cy + vertical + 1).min(max_y + 1 - pad_top),
                cz + horizontal + 1,
            ],
        );
        let context_free = Rc::new(RefCell::new(FreeSpace::context(
            big,
            Box4::from_bounds(pieces[0].bounding_box),
        )));
        // `PoolAliasLookup.create(poolAliases, position, seed)` 在 `findGenerationPoint`
        // 里就算好了，`addPieces` 的递归共用同一个映射；`start_position` 只是留档。
        // `PoolAliasLookup.create(poolAliases, position, seed)` is built once in
        // `findGenerationPoint`; the recursion shares it. `start_position` is kept only
        // for reference.
        let aliases = point.aliases.clone();
        let mut placer = Placer {
            pools,
            generator,
            random: &random,
            config,
            aliases: &aliases,
            liquid_settings,
            pieces,
            placing: PriorityQueue::default(),
        };
        placer.try_placing_children(0, context_free, 0)?;
        while let Some(state) = placer.placing.next() {
            placer.try_placing_children(state.piece_index, state.free, state.depth)?;
        }
        pieces = placer.pieces;
    }

    Ok(Some(StructurePieces {
        position: point.position,
        pieces,
    }))
}

impl<'a> Placer<'a> {
    /// `Placer.tryPlacingChildren`：`continue label129` 用 `'sources` 标签实现。
    fn try_placing_children(
        &mut self,
        piece_index: usize,
        context_free: Rc<RefCell<FreeSpace>>,
        depth: i32,
    ) -> Result<(), String> {
        let source = self.pieces[piece_index].clone();
        let source_element = source.element.clone();
        let source_box_position = source.position;
        let source_rotation = source.rotation;
        let source_projection = source_element.projection();
        let source_rigid = source_projection == Projection::Rigid;
        let source_bb = source.bounding_box;
        let source_box_y = source_bb.min_y;
        let source_gld = source.ground_level_delta;
        let mut source_free: Option<Rc<RefCell<FreeSpace>>> = None;
        let source_jigsaws =
            source_element.shuffled_jigsaws(source_box_position, source_rotation, self.random)?;

        'sources: for source_jigsaw in &source_jigsaws {
            let (dx, dy, dz) = source_jigsaw.front.step();
            let source_jigsaw_pos = source_jigsaw.pos;
            let target_jigsaw_pos = (
                source_jigsaw_pos.0 + dx,
                source_jigsaw_pos.1 + dy,
                source_jigsaw_pos.2 + dz,
            );
            let source_jigsaw_local_y = source_jigsaw_pos.1 - source_box_y;
            let mut base_height: Option<i32> = None;

            // `poolAliasLookup.lookup(sourceJigsaw.pool())`。
            // `poolAliasLookup.lookup(sourceJigsaw.pool())`.
            let pool_name = self.aliases.lookup(&source_jigsaw.pool);
            let Some(target_pool) = self.pools.get(pool_name) else {
                continue 'sources;
            };
            if target_pool.size() == 0 && pool_name != "minecraft:empty" {
                continue 'sources;
            }
            let Some(fallback_pool) = self.pools.get(target_pool.fallback()) else {
                continue 'sources;
            };
            if fallback_pool.size() == 0 && target_pool.fallback() != "minecraft:empty" {
                continue 'sources;
            }

            let attach_inside_source = source_bb.contains(
                target_jigsaw_pos.0,
                target_jigsaw_pos.1,
                target_jigsaw_pos.2,
            );
            let children_free = if attach_inside_source {
                source_free
                    .get_or_insert_with(|| {
                        Rc::new(RefCell::new(FreeSpace::source(Box4::from_bounds(
                            source_bb,
                        ))))
                    })
                    .clone()
            } else {
                context_free.clone()
            };

            let mut target_pieces: Vec<PoolElement> = Vec::new();
            if depth != self.config.max_depth {
                target_pieces.extend(target_pool.shuffled_elements(self.random));
            }
            target_pieces.extend(fallback_pool.shuffled_elements(self.random));
            let placement_priority = source_jigsaw.placement_priority;

            for target_element in &target_pieces {
                if *target_element == PoolElement::Empty {
                    break;
                }
                for target_rotation in Rotation::get_shuffled(self.random) {
                    let target_jigsaws =
                        target_element.shuffled_jigsaws((0, 0, 0), target_rotation, self.random)?;
                    let hack_box = target_element.bounding_box((0, 0, 0), target_rotation)?;
                    let expand_to = if self.config.use_expansion_hack
                        && (hack_box.max_y - hack_box.min_y + 1) <= 16
                    {
                        self.expansion_size(&target_jigsaws, &hack_box)?
                    } else {
                        0
                    };

                    for target_jigsaw in &target_jigsaws {
                        if !can_attach(source_jigsaw, target_jigsaw) {
                            continue;
                        }
                        let local = target_jigsaw.pos;
                        let raw_box_pos = (
                            target_jigsaw_pos.0 - local.0,
                            target_jigsaw_pos.1 - local.1,
                            target_jigsaw_pos.2 - local.2,
                        );
                        let raw_bb = target_element.bounding_box(raw_box_pos, target_rotation)?;
                        let raw_target_y = raw_bb.min_y;
                        let target_projection = target_element.projection();
                        let target_rigid = target_projection == Projection::Rigid;
                        let target_jigsaw_local_y = local.1;
                        let delta_y = source_jigsaw_local_y - target_jigsaw_local_y
                            + source_jigsaw.front.step_y();

                        let target_box_y = if source_rigid && target_rigid {
                            source_box_y + delta_y
                        } else {
                            let base = self.free_height(&mut base_height, source_jigsaw_pos)?;
                            base - target_jigsaw_local_y
                        };
                        let y_offset = target_box_y - raw_target_y;
                        let mut target_bb = BoundingBox {
                            min_x: raw_bb.min_x,
                            min_y: raw_bb.min_y + y_offset,
                            min_z: raw_bb.min_z,
                            max_x: raw_bb.max_x,
                            max_y: raw_bb.max_y + y_offset,
                            max_z: raw_bb.max_z,
                        };
                        let target_box_position =
                            (raw_box_pos.0, raw_box_pos.1 + y_offset, raw_box_pos.2);
                        if expand_to > 0 {
                            let new_size = (expand_to + 1).max(target_bb.max_y - target_bb.min_y);
                            target_bb.max_y = target_bb.max_y.max(target_bb.min_y + new_size);
                        }

                        let probe = Box4::from_bounds(target_bb).deflate_quarter();
                        if children_free.borrow().join_is_not_empty(&probe) {
                            continue;
                        }
                        children_free
                            .borrow_mut()
                            .subtract(Box4::from_bounds(target_bb));
                        let target_ground_level_delta = if target_rigid {
                            source_gld - delta_y
                        } else {
                            target_element.ground_level_delta()
                        };
                        let junction_y = if source_rigid {
                            source_box_y + source_jigsaw_local_y
                        } else if target_rigid {
                            target_box_y + target_jigsaw_local_y
                        } else {
                            let base = self.free_height(&mut base_height, source_jigsaw_pos)?;
                            base + delta_y / 2
                        };
                        self.pieces[piece_index].junctions.push(Junction {
                            source_x: target_jigsaw_pos.0,
                            source_ground_y: junction_y - source_jigsaw_local_y + source_gld,
                            source_z: target_jigsaw_pos.2,
                            delta_y,
                            dest_projection: target_projection,
                        });
                        let new_index = self.pieces.len();
                        self.pieces.push(Piece {
                            element: target_element.clone(),
                            position: target_box_position,
                            rotation: target_rotation,
                            bounding_box: target_bb,
                            ground_level_delta: target_ground_level_delta,
                            junctions: vec![Junction {
                                source_x: source_jigsaw_pos.0,
                                source_ground_y: junction_y - target_jigsaw_local_y
                                    + target_ground_level_delta,
                                source_z: source_jigsaw_pos.2,
                                delta_y: -delta_y,
                                dest_projection: source_projection,
                            }],
                            liquid_settings: self.liquid_settings,
                        });
                        // 官方 `depth + 1 <= maxDepth`。
                        // Vanilla `depth + 1 <= maxDepth`.
                        if depth < self.config.max_depth {
                            self.placing.add(
                                PieceState {
                                    piece_index: new_index,
                                    free: children_free.clone(),
                                    depth: depth + 1,
                                },
                                placement_priority,
                            );
                        }
                        continue 'sources;
                    }
                }
            }
        }
        Ok(())
    }

    /// `getFirstFreeHeight`（懒算，缓存在 `base_height`）。
    fn free_height(
        &self,
        base_height: &mut Option<i32>,
        pos: (i32, i32, i32),
    ) -> Result<i32, String> {
        if let Some(value) = base_height {
            return Ok(*value);
        }
        let value = self.generator.first_occupied_height(pos.0, pos.2)? + 1;
        *base_height = Some(value);
        Ok(value)
    }

    /// `doExpansionHack` 里的 `expandTo`：`hackBox` 内的拼图方块对应子 pool 的最大 YSpan。
    fn expansion_size(
        &self,
        target_jigsaws: &[JigsawBlock],
        hack_box: &BoundingBox,
    ) -> Result<i32, String> {
        let mut max = 0;
        for jigsaw in target_jigsaws {
            let (dx, dy, dz) = jigsaw.front.step();
            let neighbour = (jigsaw.pos.0 + dx, jigsaw.pos.1 + dy, jigsaw.pos.2 + dz);
            if !hack_box.contains(neighbour.0, neighbour.1, neighbour.2) {
                continue;
            }
            let size = match self.pools.get(self.aliases.lookup(&jigsaw.pool)) {
                Some(pool) => {
                    let own = pool.max_size()?;
                    let fallback = self
                        .pools
                        .get(pool.fallback())
                        .map(|pool| pool.max_size())
                        .transpose()?
                        .unwrap_or(0);
                    own.max(fallback)
                }
                None => 0,
            };
            max = max.max(size);
        }
        Ok(max)
    }
}

#[cfg(test)]
mod tests {
    use super::PriorityQueue;

    /// `SequencedPriorityIterator`：高优先级先出、同优先级 FIFO、优先级可以交替入队。
    ///
    /// `SequencedPriorityIterator`: highest priority first, FIFO within a priority, and
    /// priorities may be interleaved on insert.
    #[test]
    fn priority_queue_matches_sequenced_priority_iterator() {
        let mut queue = PriorityQueue::default();
        queue.add("a0", 0);
        queue.add("b5", 5);
        queue.add("a1", 0);
        queue.add("c2", 2);
        queue.add("b6", 5);
        // 负优先级也要能排到最后。
        // Negative priorities sort last too.
        queue.add("d-3", -3);

        let mut order = Vec::new();
        while let Some(value) = queue.next() {
            order.push(value);
        }
        assert_eq!(order, vec!["b5", "b6", "c2", "a0", "a1", "d-3"]);
    }

    /// 空队列返回 `None`；取完后也返回 `None`。
    ///
    /// An empty queue yields `None`, and so does an exhausted one.
    #[test]
    fn priority_queue_empties() {
        let mut queue = PriorityQueue::default();
        assert!(queue.next().is_none());
        queue.add(1, 0);
        assert_eq!(queue.next(), Some(1));
        assert!(queue.next().is_none());
    }
}
