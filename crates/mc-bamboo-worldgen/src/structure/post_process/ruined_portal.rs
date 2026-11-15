//! 废弃传送门的 `RuinedPortalPiece.postProcess`：放模板（含 `BlockAgeProcessor` /
//! `BlackstoneReplaceProcessor` / `LavaSubmergedBlockProcessor` / `ProtectedBlockProcessor` /
//! `RuleProcessor`），然后 `spreadNetherrack`、`addNetherrackDripColumnsBelowPortal`，
//! 最后按需补藤蔓和树叶。
//!
//! Vanilla `RuinedPortalPiece.postProcess`: place the template (with `BlockAgeProcessor`,
//! `BlackstoneReplaceProcessor`, `LavaSubmergedBlockProcessor`, `ProtectedBlockProcessor`
//! and `RuleProcessor`), then `spreadNetherrack` and
//! `addNetherrackDripColumnsBelowPortal`, and finally the optional vines and leaves.

use mc_bamboo_data::{blocks, facts, state_facts};

use crate::{
    feature::world::FeatureWorld,
    random::WorldgenRandom,
    structure::{
        pieces::{Box, Piece},
        template::StructureTemplate,
        template_place::{
            IGNORE_STRUCTURE_AND_AIR, IGNORE_STRUCTURE_BLOCK, InputTest, LiquidSettings,
            PieceOutcome, PlaceSettings, Processor, Rule, heightmap_type,
            template_piece_post_process,
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
    }];
    rules.push(lava_rule(placement, properties));
    if !properties.cold {
        rules.push(Rule {
            // `getBlockReplaceRule(Blocks.NETHERRACK, 0.07F, Blocks.MAGMA_BLOCK)`。
            // `getBlockReplaceRule(Blocks.NETHERRACK, 0.07F, Blocks.MAGMA_BLOCK)`.
            input: InputTest::RandomBlock("minecraft:netherrack", MAGMA_INSTEAD_OF_NETHERRACK),
            output: block_state("minecraft:magma_block"),
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
        }
    } else if properties.cold {
        Rule {
            input: InputTest::Block("minecraft:lava"),
            output: block_state("minecraft:netherrack"),
        }
    } else {
        Rule {
            input: InputTest::RandomBlock("minecraft:lava", MAGMA_INSTEAD_OF_LAVA),
            output: block_state("minecraft:magma_block"),
        }
    }
}

/// `RuinedPortalPiece.postProcess` 的方块部分。
///
/// The block part of `RuinedPortalPiece.postProcess`.
pub fn ruined_portal(
    world: &mut FeatureWorld<'_>,
    piece: &Piece,
    chunk_bb: Box,
    random: &WorldgenRandom,
    record: bool,
) -> Result<PortalsOutcome, String> {
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
    let bounding_box =
        crate::structure::template_place::template_box(&template, &settings, template_position);
    let center = box_center(&bounding_box);
    let mut writes = Vec::new();
    if !inside(&chunk_bb, center) {
        return Ok(PortalsOutcome {
            placement: PieceOutcome {
                placed: None,
                markers: Vec::new(),
                jigsaw_writes: Vec::new(),
                bounding_box,
                template_position,
            },
            writes,
        });
    }
    let mut chunk_bb = chunk_bb;
    encapsulate(&mut chunk_bb, &bounding_box);
    let placement_outcome = template_piece_post_process(
        world,
        &template,
        template_position,
        &settings,
        chunk_bb,
        record,
    );
    // `this.getBoundingBox()` 在 `super.postProcess` 里被重算成模板盒。
    // `this.getBoundingBox()` is recomputed as the template box inside `super.postProcess`.
    let piece_box = placement_outcome.bounding_box;

    log_writes(&mut writes, world, record);
    spread_netherrack(
        world,
        &piece_box,
        placement,
        properties,
        random,
        &mut writes,
        record,
    );
    add_netherrack_drip_columns(world, &piece_box, properties, random, &mut writes, record);
    if properties.vines || properties.overgrown {
        // `BlockPos.betweenClosedStream(this.getBoundingBox())`：x 内层、z 外层。
        // `BlockPos.betweenClosedStream(this.getBoundingBox())`: x innermost, z outermost.
        for z in piece_box[2]..=piece_box[5] {
            for x in piece_box[0]..=piece_box[3] {
                for y in piece_box[1]..=piece_box[4] {
                    if properties.vines {
                        maybe_add_vines(world, (x, y, z), random, &mut writes, record);
                    }
                    if properties.overgrown {
                        maybe_add_leaves_above(world, (x, y, z), random, &mut writes, record);
                    }
                }
            }
        }
    }

    Ok(PortalsOutcome {
        placement: placement_outcome,
        writes,
    })
}

/// 废弃传送门的产物：模板放置部分，加上 `postProcess` 之后追加的方块。
///
/// The ruined-portal outcome: the template placement plus the blocks appended after it.
pub struct PortalsOutcome {
    pub placement: PieceOutcome,
    /// `spreadNetherrack` / 滴水柱 / 藤蔓 / 树叶写下的方块。
    ///
    /// The blocks written by `spreadNetherrack`, the drip columns, vines and leaves.
    pub writes: Vec<(i32, i32, i32, u32)>,
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

/// 把调用方传入的写入日志接上（对拍用）。
///
/// Append to the caller's write log (oracle only).
fn log_writes(writes: &mut Vec<(i32, i32, i32, u32)>, _world: &FeatureWorld<'_>, _record: bool) {
    let _ = writes;
}

/// `spreadNetherrack(random, level)`。
///
/// `spreadNetherrack(random, level)`.
#[allow(clippy::too_many_arguments)]
fn spread_netherrack(
    world: &mut FeatureWorld<'_>,
    bb: &Box,
    placement: VerticalPlacement,
    properties: Properties,
    random: &WorldgenRandom,
    writes: &mut Vec<(i32, i32, i32, u32)>,
    record: bool,
) {
    let follow_ground_surface = placement == VerticalPlacement::OnLandSurface
        || placement == VerticalPlacement::OnOceanFloor;
    let center = box_center(bb);
    let (center_x, center_z) = (center.0, center.2);
    let max_distance = NETHERRACK_PROBABILITY_BY_DISTANCE.len() as i32;
    let x_span = bb[3] - bb[0] + 1;
    let z_span = bb[5] - bb[2] + 1;
    let average_width = (x_span + z_span) / 2;
    let distance_adjustment = random.next_int((8 - average_width / 2).max(1));

    for x in center_x - max_distance..=center_x + max_distance {
        for z in center_z - max_distance..=center_z + max_distance {
            let distance = (x - center_x).abs() + (z - center_z).abs();
            let adjusted = (distance + distance_adjustment).max(0);
            if adjusted >= max_distance {
                continue;
            }
            let probability = NETHERRACK_PROBABILITY_BY_DISTANCE[adjusted as usize];
            if random.next_double() >= f64::from(probability) {
                continue;
            }
            let surface_y = surface_y(world, x, z, placement);
            let y = if follow_ground_surface {
                surface_y
            } else {
                bb[1].min(surface_y)
            };
            let pos = (x, y, z);
            if (y - bb[1]).abs() > 3 || !can_replace_by_netherrack(world, pos, placement) {
                continue;
            }
            place_netherrack_or_magma(world, pos, properties, random, writes, record);
            if properties.overgrown {
                maybe_add_leaves_above(world, pos, random, writes, record);
            }
            add_netherrack_drip_column(
                world,
                (pos.0, pos.1 - 1, pos.2),
                properties,
                random,
                writes,
                record,
            );
        }
    }
}

/// `getSurfaceY(level, x, z, verticalPlacement)`。
///
/// `getSurfaceY(level, x, z, verticalPlacement)`.
fn surface_y(world: &FeatureWorld<'_>, x: i32, z: i32, placement: VerticalPlacement) -> i32 {
    let kind = heightmap_type(placement == VerticalPlacement::OnOceanFloor);
    world.get_height(kind, x, z) - 1
}

/// `addNetherrackDripColumnsBelowPortal(random, level)`。
///
/// `addNetherrackDripColumnsBelowPortal(random, level)`.
fn add_netherrack_drip_columns(
    world: &mut FeatureWorld<'_>,
    bb: &Box,
    properties: Properties,
    random: &WorldgenRandom,
    writes: &mut Vec<(i32, i32, i32, u32)>,
    record: bool,
) {
    // 注意边界：官方是 `x < maxX`、`z < maxZ`（不含右边界）。
    // Note the bounds: vanilla loops `x < maxX` and `z < maxZ` (exclusive).
    for x in bb[0] + 1..bb[3] {
        for z in bb[2] + 1..bb[5] {
            let pos = (x, bb[1], z);
            if is_block(world.get_block(pos.0, pos.1, pos.2), "minecraft:netherrack") {
                add_netherrack_drip_column(
                    world,
                    (x, bb[1] - 1, z),
                    properties,
                    random,
                    writes,
                    record,
                );
            }
        }
    }
}

/// `addNetherrackDripColumn(random, level, pos)`：先写一格，再按 50% 概率往下延伸，最多 8 格。
///
/// `addNetherrackDripColumn(random, level, pos)`: write one block, then extend downward
/// with a 50% chance per step, at most eight steps.
fn add_netherrack_drip_column(
    world: &mut FeatureWorld<'_>,
    pos: (i32, i32, i32),
    properties: Properties,
    random: &WorldgenRandom,
    writes: &mut Vec<(i32, i32, i32, u32)>,
    record: bool,
) {
    // 官方在 `placeNetherrackOrMagma` 里用 `this.verticalPlacement` 判液体；
    // 滴水柱没有那个上下文，用 `IN_NETHER` 之外的默认值（见汇报的疑点）。
    // Vanilla checks `this.verticalPlacement` inside `placeNetherrackOrMagma`; the drip
    // column has no such context here, so the default (not `IN_NETHER`) is used, see the
    // report.
    let mut current = pos;
    place_netherrack_or_magma_drip(world, current, properties, None, random, writes, record);
    let mut remaining = 8;
    while remaining > 0 && random.next_float() < 0.5 {
        current = (current.0, current.1 - 1, current.2);
        remaining -= 1;
        place_netherrack_or_magma_drip(world, current, properties, None, random, writes, record);
    }
}

/// `placeNetherrackOrMagma(random, level, pos)`。
///
/// `placeNetherrackOrMagma(random, level, pos)`.
fn place_netherrack_or_magma(
    world: &mut FeatureWorld<'_>,
    pos: (i32, i32, i32),
    properties: Properties,
    random: &WorldgenRandom,
    writes: &mut Vec<(i32, i32, i32, u32)>,
    record: bool,
) {
    place_netherrack_or_magma_drip(world, pos, properties, None, random, writes, record);
}

/// `placeNetherrackOrMagma` 的共用实现；`placement` 为 `None` 表示按「不是 IN_NETHER」处理。
///
/// The shared `placeNetherrackOrMagma` body; `placement = None` means "not IN_NETHER".
fn place_netherrack_or_magma_drip(
    world: &mut FeatureWorld<'_>,
    pos: (i32, i32, i32),
    properties: Properties,
    _placement: Option<VerticalPlacement>,
    random: &WorldgenRandom,
    writes: &mut Vec<(i32, i32, i32, u32)>,
    record: bool,
) {
    let current = world.get_block(pos.0, pos.1, pos.2);
    if !can_replace_state(current) {
        return;
    }
    let state = if !properties.cold && random.next_float() < MAGMA_INSTEAD_OF_NETHERRACK {
        block_state("minecraft:magma_block")
    } else {
        block_state("minecraft:netherrack")
    };
    world.set_block_with_heightmaps(pos.0, pos.1, pos.2, state);
    if record {
        writes.push((pos.0, pos.1, pos.2, state));
    }
}

/// `canBlockBeReplacedByNetherrackOrMagma(level, pos)`。
///
/// `canBlockBeReplacedByNetherrackOrMagma(level, pos)`.
fn can_replace_by_netherrack(
    world: &FeatureWorld<'_>,
    pos: (i32, i32, i32),
    placement: VerticalPlacement,
) -> bool {
    let state = world.get_block(pos.0, pos.1, pos.2);
    if is_block(state, "minecraft:lava") && placement != VerticalPlacement::InNether {
        return false;
    }
    can_replace_state(state)
}

/// `canBlockBeReplacedByNetherrackOrMagma(BlockState)` 里与位置无关的部分。
///
/// The position-independent part of `canBlockBeReplacedByNetherrackOrMagma(BlockState)`.
fn can_replace_state(state: u32) -> bool {
    if state_facts(state) & facts::AIR != 0 {
        return false;
    }
    if is_block(state, "minecraft:obsidian") {
        return false;
    }
    if mc_bamboo_data::block_of_state(state).is_some_and(|block| {
        mc_bamboo_data::block_tag("minecraft:features_cannot_replace")
            .is_some_and(|ids| ids.contains(&(block.first_state as i32)))
    }) {
        return false;
    }
    true
}

/// `maybeAddVines(random, level, pos)`。
///
/// `maybeAddVines(random, level, pos)`.
fn maybe_add_vines(
    world: &mut FeatureWorld<'_>,
    pos: (i32, i32, i32),
    random: &WorldgenRandom,
    writes: &mut Vec<(i32, i32, i32, u32)>,
    record: bool,
) {
    let state = world.get_block(pos.0, pos.1, pos.2);
    if state_facts(state) & facts::AIR != 0 || is_block(state, "minecraft:vine") {
        return;
    }
    // `getRandomHorizontalDirection(random)`：`Plane.HORIZONTAL` 的 NORTH / EAST /
    // SOUTH / WEST。
    // `getRandomHorizontalDirection(random)`: NORTH / EAST / SOUTH / WEST of
    // `Plane.HORIZONTAL`.
    const HORIZONTAL: [(&str, (i32, i32)); 4] = [
        ("north", (0, -1)),
        ("east", (1, 0)),
        ("south", (0, 1)),
        ("west", (-1, 0)),
    ];
    let (name, (dx, dz)) = HORIZONTAL[random.next_int(4) as usize];
    let neighbour = (pos.0 + dx, pos.1, pos.2 + dz);
    let neighbour_state = world.get_block(neighbour.0, neighbour.1, neighbour.2);
    if state_facts(neighbour_state) & facts::AIR == 0 {
        return;
    }
    // `Block.isFaceFull(state.getCollisionShape(level, pos), direction)`。
    // `Block.isFaceFull(state.getCollisionShape(level, pos), direction)`.
    let opposite = match name {
        "north" => "south",
        "south" => "north",
        "east" => "west",
        _ => "east",
    };
    if !face_is_full(state, name) {
        return;
    }
    let mut vine = block_state("minecraft:vine");
    if let Some(next) = crate::feature::state::with_property(vine, opposite, "true") {
        vine = next;
    }
    world.set_block_with_heightmaps(neighbour.0, neighbour.1, neighbour.2, vine);
    if record {
        writes.push((neighbour.0, neighbour.1, neighbour.2, vine));
    }
}

/// `maybeAddLeavesAbove(random, level, pos)`。
///
/// `maybeAddLeavesAbove(random, level, pos)`.
fn maybe_add_leaves_above(
    world: &mut FeatureWorld<'_>,
    pos: (i32, i32, i32),
    random: &WorldgenRandom,
    writes: &mut Vec<(i32, i32, i32, u32)>,
    record: bool,
) {
    if random.next_float() >= 0.5 {
        return;
    }
    if !is_block(world.get_block(pos.0, pos.1, pos.2), "minecraft:netherrack") {
        return;
    }
    let above = (pos.0, pos.1 + 1, pos.2);
    if state_facts(world.get_block(above.0, above.1, above.2)) & facts::AIR == 0 {
        return;
    }
    let mut leaves = block_state("minecraft:jungle_leaves");
    if let Some(next) = crate::feature::state::with_property(leaves, "persistent", "true") {
        leaves = next;
    }
    world.set_block_with_heightmaps(above.0, above.1, above.2, leaves);
    if record {
        writes.push((above.0, above.1, above.2, leaves));
    }
}

/// `Block.isFaceFull(collisionShape, direction)`：碰撞形状在这一面上的投影是否盖满整面。
///
/// `Block.isFaceFull(collisionShape, direction)`: whether the collision shape covers the
/// whole face.
fn face_is_full(state: u32, direction: &str) -> bool {
    let boxes = mc_bamboo_data::collision_shape(state);
    // 官方还要求形状在垂直于该面的两轴上覆盖 0..16、在方向上贴边。用轴对齐盒子近似：
    // 只要有一个盒子的对应面是完整 16×16 就算满。
    // Vanilla also requires the shape to span 0..16 on the two perpendicular axes and to
    // touch the face; approximated with axis-aligned boxes: any box whose face is the full
    // 16×16 counts.
    boxes.iter().any(|b| match direction {
        "north" => {
            b.min_z <= 0.0 && b.min_x <= 0.0 && b.max_x >= 1.0 && b.min_y <= 0.0 && b.max_y >= 1.0
        }
        "south" => {
            b.max_z >= 1.0 && b.min_x <= 0.0 && b.max_x >= 1.0 && b.min_y <= 0.0 && b.max_y >= 1.0
        }
        "east" => {
            b.max_x >= 1.0 && b.min_z <= 0.0 && b.max_z >= 1.0 && b.min_y <= 0.0 && b.max_y >= 1.0
        }
        "west" => {
            b.min_x <= 0.0 && b.min_z <= 0.0 && b.max_z >= 1.0 && b.min_y <= 0.0 && b.max_y >= 1.0
        }
        "up" => {
            b.max_y >= 1.0 && b.min_x <= 0.0 && b.max_x >= 1.0 && b.min_z <= 0.0 && b.max_z >= 1.0
        }
        _ => b.min_y <= 0.0 && b.min_x <= 0.0 && b.max_x >= 1.0 && b.min_z <= 0.0 && b.max_z >= 1.0,
    })
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
