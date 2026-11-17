//! 废弃传送门的 `RuinedPortalPiece.postProcess`：放模板（含 `BlockAgeProcessor` /
//! `BlackstoneReplaceProcessor` / `LavaSubmergedBlockProcessor` / `ProtectedBlockProcessor` /
//! `RuleProcessor`），然后 `spreadNetherrack`、`addNetherrackDripColumnsBelowPortal`，
//! 最后按需补藤蔓和树叶。
//!
//! Vanilla `RuinedPortalPiece.postProcess`: place the template (with `BlockAgeProcessor`,
//! `BlackstoneReplaceProcessor`, `LavaSubmergedBlockProcessor`, `ProtectedBlockProcessor`
//! and `RuleProcessor`), then `spreadNetherrack` and
//! `addNetherrackDripColumnsBelowPortal`, and finally the optional vines and leaves.

use mc_bamboo_core::Direction;
use mc_bamboo_data::{blocks, facts, state_facts};

use crate::{
    feature::world::FeatureWorld,
    random::WorldgenRandom,
    structure::{
        pieces::{Box, Piece},
        template::StructureTemplate,
        template_place::{
            BlockWrite, IGNORE_STRUCTURE_AND_AIR, IGNORE_STRUCTURE_BLOCK, InputTest,
            LiquidSettings, PieceOutcome, PlaceSettings, Processor, Rule, heightmap_type, in_tag,
            template_box, template_piece_post_process,
        },
        templated::ruined_portal::{Properties, VerticalPlacement},
    },
};

/// `RuinedPortalPiece.PROBABILITY_OF_GOLD_GONE`。
///
/// `RuinedPortalPiece.PROBABILITY_OF_GOLD_GONE`.
const GOLD_GONE: f32 = 0.3;
/// `RuinedPortalPiece.PROBABILITY_OF_MAGMA_INSTEAD_OF_NETHERRACK`。
///
/// `RuinedPortalPiece.PROBABILITY_OF_MAGMA_INSTEAD_OF_NETHERRACK`.
const MAGMA_INSTEAD_OF_NETHERRACK: f32 = 0.07;
/// `RuinedPortalPiece.PROBABILITY_OF_MAGMA_INSTEAD_OF_LAVA`。
///
/// `RuinedPortalPiece.PROBABILITY_OF_MAGMA_INSTEAD_OF_LAVA`.
const MAGMA_INSTEAD_OF_LAVA: f32 = 0.2;

/// `spreadNetherrack` 的距离概率表。
///
/// The distance probability table of `spreadNetherrack`.
const NETHERRACK_PROBABILITY_BY_DISTANCE: [f32; 14] = [
    1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.9, 0.9, 0.8, 0.7, 0.6, 0.4, 0.2,
];

/// `RuinedPortalPiece.makeSettings(...)`。
///
/// `RuinedPortalPiece.makeSettings(...)`.
fn settings(
    rotation: i32,
    mirror: i32,
    pivot: (i32, i32),
    placement: VerticalPlacement,
    properties: Properties,
) -> PlaceSettings {
    let blocks = mc_bamboo_data::blocks::AIR;
    let ignore = if properties.air_pocket {
        IGNORE_STRUCTURE_BLOCK
    } else {
        IGNORE_STRUCTURE_AND_AIR
    };
    let mut rules = vec![Rule {
        // `getBlockReplaceRule(Blocks.GOLD_BLOCK, 0.3F, Blocks.AIR)`。
        // `getBlockReplaceRule(Blocks.GOLD_BLOCK, 0.3F, Blocks.AIR)`.
        input: InputTest::RandomBlock("minecraft:gold_block", GOLD_GONE),
        output: blocks,
        loot_table: None,
    }];
    rules.push(lava_rule(placement, properties));
    if !properties.cold {
        rules.push(Rule {
            // `getBlockReplaceRule(Blocks.NETHERRACK, 0.07F, Blocks.MAGMA_BLOCK)`。
            // `getBlockReplaceRule(Blocks.NETHERRACK, 0.07F, Blocks.MAGMA_BLOCK)`.
            input: InputTest::RandomBlock("minecraft:netherrack", MAGMA_INSTEAD_OF_NETHERRACK),
            output: block_state("minecraft:magma_block"),
            loot_table: None,
        });
    }
    let mut processors = vec![
        Processor::Ignore(ignore),
        Processor::Rule(rules),
        Processor::Age(properties.mossiness),
        Processor::Protected,
        Processor::LavaSubmerged,
    ];
    if properties.replace_with_blackstone {
        processors.push(Processor::Blackstone);
    }
    PlaceSettings::new(mirror, rotation, pivot)
        .with_liquid(LiquidSettings::ApplyWaterlogging)
        .with_processors(processors)
}

/// `getLavaProcessorRule(verticalPlacement, properties)`。
///
/// `getLavaProcessorRule(verticalPlacement, properties)`.
fn lava_rule(placement: VerticalPlacement, properties: Properties) -> Rule {
    if placement == VerticalPlacement::OnOceanFloor {
        Rule {
            input: InputTest::Block("minecraft:lava"),
            output: block_state("minecraft:magma_block"),
            loot_table: None,
        }
    } else if properties.cold {
        Rule {
            input: InputTest::Block("minecraft:lava"),
            output: block_state("minecraft:netherrack"),
            loot_table: None,
        }
    } else {
        Rule {
            input: InputTest::RandomBlock("minecraft:lava", MAGMA_INSTEAD_OF_LAVA),
            output: block_state("minecraft:magma_block"),
            loot_table: None,
        }
    }
}

/// `RuinedPortalPiece.postProcess` 的方块部分。
///
/// `chunk_bb` 是 `placeInChunk` 传下来的那个盒子：官方在结构件中心落在盒内时
/// **就地** `chunkBB.encapsulate(boundingBox)`，同一个区块里排在后面的结构件看到的是
/// 扩大后的盒子，所以这里收可变引用。`random` 是 `postProcess` 的随机源。
///
/// The block part of `RuinedPortalPiece.postProcess`. `chunk_bb` is the box handed down
/// by `placeInChunk`: when the piece centre is inside it vanilla calls
/// `chunkBB.encapsulate(boundingBox)` **in place**, so later pieces of the same chunk see
/// the grown box; hence the mutable reference. `random` is the `postProcess` random.
pub fn ruined_portal(
    world: &mut FeatureWorld<'_>,
    piece: &mut Piece,
    chunk_bb: &mut Box,
    random: &WorldgenRandom,
) -> Result<PieceOutcome, String> {
    let template_name = piece
        .get_str("Template")
        .ok_or("ruined portal piece has no Template")?
        .to_owned();
    let rotation = crate::structure::templated::rotation_ordinal(
        piece
            .get_str("Rotation")
            .ok_or("ruined portal piece has no Rotation")?,
    )?;
    let mirror = mirror_ordinal(
        piece
            .get_str("Mirror")
            .ok_or("ruined portal piece has no Mirror")?,
    )?;
    let placement = VerticalPlacement::parse(
        piece
            .get_str("VerticalPlacement")
            .ok_or("ruined portal piece has no VerticalPlacement")?,
    )
    .ok_or("bad VerticalPlacement")?;
    let properties = Properties::parse(
        piece
            .get_str("Properties")
            .ok_or("ruined portal piece has no Properties")?,
    )?;
    let template_position = (
        extra_int(piece, "TPX").ok_or("ruined portal piece has no TPX")?,
        extra_int(piece, "TPY").ok_or("ruined portal piece has no TPY")?,
        extra_int(piece, "TPZ").ok_or("ruined portal piece has no TPZ")?,
    );
    let template = StructureTemplate::load(&template_name)?;
    let pivot = (template.size.0 / 2, template.size.2 / 2);
    let settings = settings(rotation, mirror, pivot, placement, properties);

    // `if (chunkBB.isInside(boundingBox.getCenter()))`：只有结构件中心落在本区块内才放，
    // 并且把结构件的完整盒子并进 `chunkBB`（官方就地修改这个盒）。
    // `if (chunkBB.isInside(boundingBox.getCenter()))`: only place when the piece centre is
    // inside the chunk box, and merge the full piece box into `chunkBB` (vanilla mutates it).
    let bounding_box = template_box(&template, &settings, template_position);
    if !inside(chunk_bb, box_center(&bounding_box)) {
        return Ok(PieceOutcome {
            placed: false,
            writes: Vec::new(),
            block_entities: Vec::new(),
            bounding_box: piece.bb,
            template_position,
        });
    }
    encapsulate(chunk_bb, &bounding_box);
    // `handleDataMarker` 是空的。
    // `handleDataMarker` is empty.
    let mut outcome = template_piece_post_process(
        world,
        &template,
        template_position,
        &settings,
        *chunk_bb,
        random,
        &mut |_, _, _| {},
    );
    // `this.boundingBox` 在 `super.postProcess` 里被重算成模板盒。
    // `this.boundingBox` is recomputed as the template box inside `super.postProcess`.
    piece.bb = outcome.bounding_box;

    let mut portal = Portal {
        world,
        random,
        writes: std::mem::take(&mut outcome.writes),
        bb: outcome.bounding_box,
        placement,
        properties,
    };
    portal.spread_netherrack();
    portal.add_netherrack_drip_columns_below_portal();
    if properties.vines || properties.overgrown {
        // `BlockPos.betweenClosedStream(this.getBoundingBox())` 的顺序：x 变得最快，
        // 然后 y，z 最慢。
        // `BlockPos.betweenClosedStream(box)` order: x fastest, then y, z slowest.
        let bb = portal.bb;
        for z in bb[2]..=bb[5] {
            for y in bb[1]..=bb[4] {
                for x in bb[0]..=bb[3] {
                    if properties.vines {
                        portal.maybe_add_vines((x, y, z));
                    }
                    if properties.overgrown {
                        portal.maybe_add_leaves_above((x, y, z));
                    }
                }
            }
        }
    }
    outcome.writes = portal.writes;
    Ok(outcome)
}

/// `super.postProcess` 之后那几步共用的状态：世界、随机源、写入日志和结构件自己的字段。
///
/// The state shared by the steps after `super.postProcess`: the world, the random, the
/// write log and the piece's own fields.
struct Portal<'w, 'a, 'r> {
    world: &'w mut FeatureWorld<'a>,
    random: &'r WorldgenRandom,
    writes: Vec<BlockWrite>,
    /// `this.boundingBox`。
    bb: Box,
    placement: VerticalPlacement,
    properties: Properties,
}

impl Portal<'_, '_, '_> {
    fn get(&self, pos: (i32, i32, i32)) -> u32 {
        self.world.get_block(pos.0, pos.1, pos.2)
    }

    /// `level.setBlockAndUpdate(pos, state)`。
    ///
    /// `level.setBlockAndUpdate(pos, state)`.
    fn set(&mut self, pos: (i32, i32, i32), state: u32) {
        if self
            .world
            .set_block_with_heightmaps(pos.0, pos.1, pos.2, state)
        {
            self.writes.push((pos.0, pos.1, pos.2, state));
        }
    }

    /// `spreadNetherrack(random, level)`。
    ///
    /// `spreadNetherrack(random, level)`.
    fn spread_netherrack(&mut self) {
        let follow_ground_surface = self.placement == VerticalPlacement::OnLandSurface
            || self.placement == VerticalPlacement::OnOceanFloor;
        let bb = self.bb;
        let center = box_center(&bb);
        let (center_x, center_z) = (center.0, center.2);
        let max_distance = NETHERRACK_PROBABILITY_BY_DISTANCE.len() as i32;
        let x_span = bb[3] - bb[0] + 1;
        let z_span = bb[5] - bb[2] + 1;
        let average_width = (x_span + z_span) / 2;
        let distance_adjustment = self.random.next_int((8 - average_width / 2).max(1));

        for x in center_x - max_distance..=center_x + max_distance {
            for z in center_z - max_distance..=center_z + max_distance {
                let distance = (x - center_x).abs() + (z - center_z).abs();
                let adjusted = (distance + distance_adjustment).max(0);
                if adjusted >= max_distance {
                    continue;
                }
                let probability = NETHERRACK_PROBABILITY_BY_DISTANCE[adjusted as usize];
                if self.random.next_double() >= f64::from(probability) {
                    continue;
                }
                let surface_y = self.surface_y(x, z);
                let y = if follow_ground_surface {
                    surface_y
                } else {
                    bb[1].min(surface_y)
                };
                let pos = (x, y, z);
                if (y - bb[1]).abs() > 3 || !self.can_be_replaced(self.get(pos)) {
                    continue;
                }
                self.place_netherrack_or_magma(pos);
                if self.properties.overgrown {
                    self.maybe_add_leaves_above(pos);
                }
                self.add_netherrack_drip_column((pos.0, pos.1 - 1, pos.2));
            }
        }
    }

    /// `getSurfaceY(level, x, z, verticalPlacement)`。
    ///
    /// `getSurfaceY(level, x, z, verticalPlacement)`.
    fn surface_y(&self, x: i32, z: i32) -> i32 {
        let kind = heightmap_type(self.placement == VerticalPlacement::OnOceanFloor);
        self.world.get_height(kind, x, z) - 1
    }

    /// `addNetherrackDripColumnsBelowPortal(random, level)`。
    ///
    /// `addNetherrackDripColumnsBelowPortal(random, level)`.
    fn add_netherrack_drip_columns_below_portal(&mut self) {
        let bb = self.bb;
        // 注意边界：官方是 `x < maxX`、`z < maxZ`（两头都不含）。
        // Note the bounds: vanilla loops `minX + 1 .. maxX` and `minZ + 1 .. maxZ`, both
        // exclusive.
        for x in bb[0] + 1..bb[3] {
            for z in bb[2] + 1..bb[5] {
                if is_block(self.get((x, bb[1], z)), "minecraft:netherrack") {
                    self.add_netherrack_drip_column((x, bb[1] - 1, z));
                }
            }
        }
    }

    /// `addNetherrackDripColumn(random, level, pos)`：先写一格，再按 50% 概率往下延伸，
    /// 最多 8 格。
    ///
    /// `addNetherrackDripColumn(random, level, pos)`: write one block, then extend downward
    /// with a 50% chance per step, at most eight steps.
    fn add_netherrack_drip_column(&mut self, pos: (i32, i32, i32)) {
        let mut current = pos;
        self.place_netherrack_or_magma(current);
        let mut remaining = 8;
        while remaining > 0 && self.random.next_float() < 0.5 {
            current = (current.0, current.1 - 1, current.2);
            remaining -= 1;
            self.place_netherrack_or_magma(current);
        }
    }

    /// `placeNetherrackOrMagma(random, level, pos)`。
    ///
    /// `placeNetherrackOrMagma(random, level, pos)`.
    fn place_netherrack_or_magma(&mut self, pos: (i32, i32, i32)) {
        if !self.can_be_replaced(self.get(pos)) {
            return;
        }
        let state =
            if !self.properties.cold && self.random.next_float() < MAGMA_INSTEAD_OF_NETHERRACK {
                block_state("minecraft:magma_block")
            } else {
                block_state("minecraft:netherrack")
            };
        self.set(pos, state);
    }

    /// `canBlockBeReplacedByNetherrackOrMagma(state)`。注意第一条是 `is(Blocks.AIR)`，
    /// 不是 `isAir()`：洞穴空气可以被替换。
    ///
    /// `canBlockBeReplacedByNetherrackOrMagma(state)`. The first test is `is(Blocks.AIR)`,
    /// not `isAir()`: cave air may be replaced.
    fn can_be_replaced(&self, state: u32) -> bool {
        state != blocks::AIR
            && !is_block(state, "minecraft:obsidian")
            && !in_tag(state, "minecraft:features_cannot_replace")
            && (self.placement == VerticalPlacement::InNether || !is_block(state, "minecraft:lava"))
    }

    /// `maybeAddVines(random, level, pos)`。
    ///
    /// `maybeAddVines(random, level, pos)`.
    fn maybe_add_vines(&mut self, pos: (i32, i32, i32)) {
        let state = self.get(pos);
        if state_facts(state) & facts::AIR != 0 || is_block(state, "minecraft:vine") {
            return;
        }
        // `getRandomHorizontalDirection(random)`：`Plane.HORIZONTAL` 的 NORTH / EAST /
        // SOUTH / WEST。
        // `getRandomHorizontalDirection(random)`: NORTH / EAST / SOUTH / WEST of
        // `Plane.HORIZONTAL`.
        const HORIZONTAL: [(Direction, &str, (i32, i32)); 4] = [
            (Direction::North, "south", (0, -1)),
            (Direction::East, "west", (1, 0)),
            (Direction::South, "north", (0, 1)),
            (Direction::West, "east", (-1, 0)),
        ];
        let (direction, opposite, (dx, dz)) = HORIZONTAL[self.random.next_int(4) as usize];
        let neighbour = (pos.0 + dx, pos.1, pos.2 + dz);
        if state_facts(self.get(neighbour)) & facts::AIR == 0 {
            return;
        }
        // `Block.isFaceFull(state.getCollisionShape(level, pos), direction)`。
        // `Block.isFaceFull(state.getCollisionShape(level, pos), direction)`.
        if !mc_bamboo_blocks::shape::is_face_full(mc_bamboo_data::collision_shape(state), direction)
        {
            return;
        }
        // `VineBlock.getPropertyForFace(direction.getOpposite())`。
        // `VineBlock.getPropertyForFace(direction.getOpposite())`.
        let mut vine = block_state("minecraft:vine");
        if let Some(next) = crate::feature::state::with_property(vine, opposite, "true") {
            vine = next;
        }
        self.set(neighbour, vine);
    }

    /// `maybeAddLeavesAbove(random, level, pos)`：先取 `nextFloat()`，再看方块。
    ///
    /// `maybeAddLeavesAbove(random, level, pos)`: the `nextFloat()` comes first, the block
    /// checks after.
    fn maybe_add_leaves_above(&mut self, pos: (i32, i32, i32)) {
        if self.random.next_float() >= 0.5 {
            return;
        }
        if !is_block(self.get(pos), "minecraft:netherrack") {
            return;
        }
        let above = (pos.0, pos.1 + 1, pos.2);
        if state_facts(self.get(above)) & facts::AIR == 0 {
            return;
        }
        let mut leaves = block_state("minecraft:jungle_leaves");
        if let Some(next) = crate::feature::state::with_property(leaves, "persistent", "true") {
            leaves = next;
        }
        self.set(above, leaves);
    }
}

/// `BoundingBox.getCenter()`。
///
/// `BoundingBox.getCenter()`.
fn box_center(bb: &Box) -> (i32, i32, i32) {
    (
        bb[0] + (bb[3] - bb[0] + 1) / 2,
        bb[1] + (bb[4] - bb[1] + 1) / 2,
        bb[2] + (bb[5] - bb[2] + 1) / 2,
    )
}

fn inside(bb: &Box, pos: (i32, i32, i32)) -> bool {
    pos.0 >= bb[0]
        && pos.0 <= bb[3]
        && pos.1 >= bb[1]
        && pos.1 <= bb[4]
        && pos.2 >= bb[2]
        && pos.2 <= bb[5]
}

/// `BoundingBox.encapsulate(other)`。
///
/// `BoundingBox.encapsulate(other)`.
fn encapsulate(bb: &mut Box, other: &Box) {
    bb[0] = bb[0].min(other[0]);
    bb[1] = bb[1].min(other[1]);
    bb[2] = bb[2].min(other[2]);
    bb[3] = bb[3].max(other[3]);
    bb[4] = bb[4].max(other[4]);
    bb[5] = bb[5].max(other[5]);
}

fn block_state(name: &str) -> u32 {
    mc_bamboo_data::block_by_name(name)
        .map(|block| block.default_state)
        .unwrap_or(blocks::AIR)
}

fn is_block(state: u32, name: &str) -> bool {
    mc_bamboo_data::block_of_state(state).is_some_and(|block| block.name == name)
}

fn mirror_ordinal(name: &str) -> Result<i32, String> {
    Ok(match name {
        "NONE" => 0,
        "LEFT_RIGHT" => 1,
        "FRONT_BACK" => 2,
        other => return Err(format!("bad mirror {other}")),
    })
}

fn extra_int(piece: &Piece, key: &str) -> Option<i32> {
    match piece.extras.get(key) {
        Some(crate::structure::pieces::Extra::Int(value)) => Some(*value),
        _ => None,
    }
}
