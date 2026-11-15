//! 废弃传送门（`RuinedPortalStructure` / `RuinedPortalPiece`）的结构件布局。
//!
//! 官方 `findGenerationPoint` 先在多个 `Setup` 之间按权重抽一个（`setups.size() > 1` 时
//! 一次 `nextFloat()` 加累减），再抽「空腔」、模板（5% 概率巨型）、`Rotation`、`Mirror`，
//! 然后按模板包围盒的中心算出 `getBaseHeight` 的 `surfaceY`，最后 `findSuitableY` 从
//! `surfaceY` 起逐格向下找「四角里至少三个角在这一层是不透明方块」的位置。这一步要
//! `getBaseColumn` 的整列方块（`mc-bamboo-worldgen::chunk::Generator::base_column`）。
//! 结构件本身是 `TemplateStructurePiece` 的子类：包围盒用 rotation / mirror / pivot 算，
//! 存档多写 `Rotation` / `Mirror` / `VerticalPlacement` / `Properties` 四个键。
//! 写方块（`postProcess` 的地表铺地狱岩、藤蔓、`spreadNetherrack`）属放置里程碑。
//!
//! Layout of the ruined portal pieces. Vanilla `findGenerationPoint` first picks one `Setup`
//! by weight (one `nextFloat()` plus repeated subtraction when `setups.size() > 1`), then draws
//! the air pocket, the template (5% chance of a giant one), the `Rotation` and the `Mirror`,
//! computes `surfaceY` from `getBaseHeight` at the template box centre, and finally
//! `findSuitableY` walks down from `surfaceY` looking for a level where at least three of the
//! four corners are opaque. That step needs the whole `getBaseColumn`
//! (`mc-bamboo-worldgen::chunk::Generator::base_column`). The piece itself extends
//! `TemplateStructurePiece`: the box comes from rotation / mirror / pivot, and the save data
//! adds `Rotation` / `Mirror` / `VerticalPlacement` / `Properties`. Block placement (the
//! `postProcess` netherrack spreading, vines, `spreadNetherrack`) is the placement milestone.

use serde_json::Value;

use crate::{
    biome_temperature::BiomeClimate,
    chunk::Generator,
    math::java_floor_div,
    structure::{
        pieces::{Piece, make_random},
        template::StructureTemplate,
        templated::{rotation_name, template_bounds},
    },
};

/// `RuinedPortalStructure.PROBABILITY_OF_GIANT_PORTAL`。
///
/// `RuinedPortalStructure.PROBABILITY_OF_GIANT_PORTAL`.
const PROBABILITY_OF_GIANT_PORTAL: f32 = 0.05;
/// `RuinedPortalStructure.MIN_Y_INDEX`：`minY = heightAccessor.getMinY() + 15`。
///
/// `RuinedPortalStructure.MIN_Y_INDEX`: `minY = heightAccessor.getMinY() + 15`.
const MIN_Y_INDEX: i32 = 15;

/// `STRUCTURE_LOCATION_PORTALS`，顺序和官方常量一致。
///
/// `STRUCTURE_LOCATION_PORTALS` in vanilla constant order.
const PORTALS: [&str; 10] = [
    "minecraft:ruined_portal/portal_1",
    "minecraft:ruined_portal/portal_2",
    "minecraft:ruined_portal/portal_3",
    "minecraft:ruined_portal/portal_4",
    "minecraft:ruined_portal/portal_5",
    "minecraft:ruined_portal/portal_6",
    "minecraft:ruined_portal/portal_7",
    "minecraft:ruined_portal/portal_8",
    "minecraft:ruined_portal/portal_9",
    "minecraft:ruined_portal/portal_10",
];
/// `STRUCTURE_LOCATION_GIANT_PORTALS`。
///
/// `STRUCTURE_LOCATION_GIANT_PORTALS`.
const GIANT_PORTALS: [&str; 3] = [
    "minecraft:ruined_portal/giant_portal_1",
    "minecraft:ruined_portal/giant_portal_2",
    "minecraft:ruined_portal/giant_portal_3",
];

/// `RuinedPortalPiece.VerticalPlacement`，字符串是 `CODEC` 写进 NBT 的名字。
///
/// `RuinedPortalPiece.VerticalPlacement`; the strings are the names its `CODEC` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalPlacement {
    OnLandSurface,
    PartlyBuried,
    OnOceanFloor,
    InMountain,
    Underground,
    InNether,
}

impl VerticalPlacement {
    /// 从 NBT 的字符串读回 `VerticalPlacement`。
    ///
    /// Read `VerticalPlacement` back from its NBT string.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "on_land_surface" => Self::OnLandSurface,
            "partly_buried" => Self::PartlyBuried,
            "on_ocean_floor" => Self::OnOceanFloor,
            "in_mountain" => Self::InMountain,
            "underground" => Self::Underground,
            "in_nether" => Self::InNether,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::OnLandSurface => "on_land_surface",
            Self::PartlyBuried => "partly_buried",
            Self::OnOceanFloor => "on_ocean_floor",
            Self::InMountain => "in_mountain",
            Self::Underground => "underground",
            Self::InNether => "in_nether",
        }
    }

    /// `RuinedPortalPiece.getHeightMapType(placement)`：只有 `ON_OCEAN_FLOOR` 用
    /// `OCEAN_FLOOR_WG`，其余都是 `WORLD_SURFACE_WG`。
    ///
    /// `RuinedPortalPiece.getHeightMapType(placement)`: only `ON_OCEAN_FLOOR` uses
    /// `OCEAN_FLOOR_WG`, everything else uses `WORLD_SURFACE_WG`.
    fn ocean_floor(self) -> bool {
        self == Self::OnOceanFloor
    }

    /// `Heightmap.Types.isOpaque()`：`WORLD_SURFACE_WG` 是 `NOT_AIR`，
    /// `OCEAN_FLOOR_WG` 是 `MATERIAL_MOTION_BLOCKING`（`BLOCKS_MOTION_IN_HEIGHTMAP`）。
    ///
    /// `Heightmap.Types.isOpaque()`: `NOT_AIR` for `WORLD_SURFACE_WG` and
    /// `MATERIAL_MOTION_BLOCKING` (`BLOCKS_MOTION_IN_HEIGHTMAP`) for `OCEAN_FLOOR_WG`.
    fn opaque(self, state: u32) -> bool {
        if self.ocean_floor() {
            mc_bamboo_blocks::BlockState(state)
                .is_tagged(mc_bamboo_data::tags::block_tags::BLOCKS_MOTION_IN_HEIGHTMAP)
        } else {
            state != mc_bamboo_data::blocks::AIR
        }
    }
}

/// 一个 `RuinedPortalStructure.Setup`。
///
/// One `RuinedPortalStructure.Setup`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setup {
    pub placement: VerticalPlacement,
    pub air_pocket_probability: f32,
    pub mossiness: f32,
    pub overgrown: bool,
    pub vines: bool,
    pub can_be_cold: bool,
    pub replace_with_blackstone: bool,
    pub weight: f32,
}

impl Setup {
    fn parse(value: &Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("setup must be an object")?;
        let placement = object
            .get("placement")
            .and_then(Value::as_str)
            .and_then(VerticalPlacement::parse)
            .ok_or("setup has no valid placement")?;
        let float = |key: &str| -> Result<f32, String> {
            object
                .get(key)
                .and_then(Value::as_f64)
                .map(|value| value as f32)
                .ok_or_else(|| format!("setup has no {key}"))
        };
        let boolean = |key: &str| -> Result<bool, String> {
            object
                .get(key)
                .and_then(Value::as_bool)
                .ok_or_else(|| format!("setup has no {key}"))
        };
        Ok(Self {
            placement,
            air_pocket_probability: float("air_pocket_probability")?,
            mossiness: float("mossiness")?,
            overgrown: boolean("overgrown")?,
            vines: boolean("vines")?,
            can_be_cold: boolean("can_be_cold")?,
            replace_with_blackstone: boolean("replace_with_blackstone")?,
            weight: float("weight")?,
        })
    }
}

/// 从 `structure` 条目的原始 JSON 解析 `setups`，顺序保持数组原样（权重累加顺序有意义）。
///
/// Parse `setups` from a `structure` entry's raw JSON, keeping the array order (the weight
/// accumulation order matters).
pub fn setups(raw: &Value) -> Result<Vec<Setup>, String> {
    raw.get("setups")
        .and_then(Value::as_array)
        .ok_or_else(|| "ruined portal has no setups".to_owned())?
        .iter()
        .map(Setup::parse)
        .collect()
}

/// `RuinedPortalPiece.Properties`：结构件存档里的六项。
///
/// `RuinedPortalPiece.Properties`: the six fields in the piece's save data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Properties {
    pub cold: bool,
    pub mossiness: f32,
    pub air_pocket: bool,
    pub overgrown: bool,
    pub vines: bool,
    pub replace_with_blackstone: bool,
}

impl Properties {
    /// 解析 `canonical` 拼出来的字符串（对拍与读档共用同一种写法）。
    ///
    /// Parse the string `canonical` writes (shared by the oracle and save loading).
    pub fn parse(text: &str) -> Result<Self, String> {
        let body = text
            .trim()
            .strip_prefix('{')
            .and_then(|rest| rest.strip_suffix('}'))
            .ok_or_else(|| format!("bad portal properties {text}"))?;
        let mut cold = false;
        let mut mossiness = 0.0;
        let mut air_pocket = false;
        let mut overgrown = false;
        let mut vines = false;
        let mut replace_with_blackstone = false;
        for entry in body.split(';') {
            let (key, value) = entry
                .split_once('=')
                .ok_or_else(|| format!("bad portal property {entry}"))?;
            match key {
                "cold" => cold = value == "1",
                "mossiness" => {
                    mossiness = value
                        .parse()
                        .map_err(|_| format!("bad mossiness {value}"))?
                }
                "air_pocket" => air_pocket = value == "1",
                "overgrown" => overgrown = value == "1",
                "vines" => vines = value == "1",
                "replace_with_blackstone" => replace_with_blackstone = value == "1",
                other => return Err(format!("unknown portal property {other}")),
            }
        }
        Ok(Self {
            cold,
            mossiness,
            air_pocket,
            overgrown,
            vines,
            replace_with_blackstone,
        })
    }

    /// 按 `Properties.CODEC` 的字段名拼成和 Java oracle 一致的标准串（键按字母序）。
    ///
    /// Format into the canonical string the Java oracle produces for `Properties.CODEC`
    /// (keys sorted alphabetically).
    pub fn canonical(self) -> String {
        format!(
            "{{air_pocket={};cold={};mossiness={};overgrown={};replace_with_blackstone={};\
             vines={}}}",
            u8::from(self.air_pocket),
            u8::from(self.cold),
            java_float(self.mossiness),
            u8::from(self.overgrown),
            u8::from(self.replace_with_blackstone),
            u8::from(self.vines),
        )
    }
}

/// `Float.toString(float)` 的写法：整数值补 `.0`，其余用最短往返十进制。
///
/// `Float.toString(float)` format: integral values gain a `.0`, the rest use the shortest
/// round-trip decimal.
fn java_float(value: f32) -> String {
    let text = format!("{value}");
    if text.contains('.') || text.contains('e') || text.contains("inf") {
        text
    } else {
        format!("{text}.0")
    }
}

/// `Mirror` 的序号：`NONE` / `LEFT_RIGHT` / `FRONT_BACK`。
///
/// `Mirror` ordinals: `NONE` / `LEFT_RIGHT` / `FRONT_BACK`.
const MIRROR_FRONT_BACK: i32 = 2;

/// `Mirror.LEGACY_CODEC` 写进 NBT 的字符串。
///
/// The string `Mirror.LEGACY_CODEC` writes to NBT.
fn mirror_name(mirror: i32) -> &'static str {
    match mirror {
        1 => "LEFT_RIGHT",
        MIRROR_FRONT_BACK => "FRONT_BACK",
        _ => "NONE",
    }
}

/// `RuinedPortalPiece` 的构造参数，避免函数参数过多。
///
/// Constructor parameters of one `RuinedPortalPiece`, to keep the arity small.
struct PortalPieceSpec<'a> {
    template_name: &'a str,
    template_size: (i32, i32, i32),
    position: (i32, i32, i32),
    rotation: i32,
    mirror: i32,
    pivot: (i32, i32),
    placement: VerticalPlacement,
    properties: Properties,
}

/// `RuinedPortalPiece` 的一个结构件。
///
/// One `RuinedPortalPiece`.
fn portal_piece(spec: PortalPieceSpec<'_>) -> Piece {
    let PortalPieceSpec {
        template_name,
        template_size,
        position,
        rotation,
        mirror,
        pivot,
        placement,
        properties,
    } = spec;
    let bb = template_bounds(template_size, rotation, mirror, pivot, position);
    let mut piece = Piece::plain("minecraft:rupo", bb, 0);
    // `TemplateStructurePiece` 构造器里的 `setOrientation(Direction.NORTH)`。
    // `setOrientation(Direction.NORTH)` in the `TemplateStructurePiece` constructor.
    piece.o = 2;
    piece.rot = rotation;
    // `getMirror()` 没有被 `TemplateStructurePiece` 覆写，仍是构造器设的 `NONE`。
    // `getMirror()` is not overridden by `TemplateStructurePiece`, so it stays `NONE`.
    piece.mirror = 0;
    piece.put_str("Template", template_name);
    piece.put_int("TPX", position.0);
    piece.put_int("TPY", position.1);
    piece.put_int("TPZ", position.2);
    piece.put_str("Rotation", rotation_name(rotation));
    piece.put_str("Mirror", mirror_name(mirror));
    piece.put_str("VerticalPlacement", placement.name());
    piece.put_str("Properties", &properties.canonical());
    piece
}

/// `sample(random, limit)`：`limit == 0` 恒 false、`limit == 1` 恒 true（都不取随机数），
/// 否则 `nextFloat() < limit`。
///
/// `sample(random, limit)`: `limit == 0` is false and `limit == 1` is true (neither draws a
/// number), otherwise `nextFloat() < limit`.
fn sample(random: &crate::random::WorldgenRandom, limit: f32) -> bool {
    if limit == 0.0 {
        false
    } else if limit == 1.0 {
        true
    } else {
        random.next_float() < limit
    }
}

/// `getRandomWithinInterval(random, minPreferred, max)`。
///
/// `getRandomWithinInterval(random, minPreferred, max)`.
fn random_within_interval(
    random: &crate::random::WorldgenRandom,
    min_preferred: i32,
    max: i32,
) -> i32 {
    if min_preferred < max {
        random.next_int_between_inclusive(min_preferred, max)
    } else {
        max
    }
}

/// `RuinedPortalStructure.findSuitableY`。
///
/// `RuinedPortalStructure.findSuitableY`.
#[allow(clippy::too_many_arguments)]
fn find_suitable_y(
    generator: &Generator,
    random: &crate::random::WorldgenRandom,
    placement: VerticalPlacement,
    air_pocket: bool,
    surface_y_at_center: i32,
    y_span: i32,
    bb: &[i32; 6],
) -> Result<i32, String> {
    let min_y = generator.settings().min_y + MIN_Y_INDEX;
    let new_y = match placement {
        VerticalPlacement::InNether => {
            if air_pocket {
                random.next_int_between_inclusive(32, 100)
            } else if random.next_float() < 0.5 {
                random.next_int_between_inclusive(27, 29)
            } else {
                random.next_int_between_inclusive(29, 100)
            }
        }
        VerticalPlacement::InMountain => {
            let max_y = surface_y_at_center - y_span;
            random_within_interval(random, 70, max_y)
        }
        VerticalPlacement::Underground => {
            let max_y = surface_y_at_center - y_span;
            random_within_interval(random, min_y, max_y)
        }
        VerticalPlacement::PartlyBuried => {
            surface_y_at_center - y_span + random.next_int_between_inclusive(2, 8)
        }
        _ => surface_y_at_center,
    };

    // 四个底角各取一整列，顺序照官方的 `ImmutableList`。
    // One full column per bottom corner, in the vanilla `ImmutableList` order.
    let corners = [
        (bb[0], bb[2]),
        (bb[3], bb[2]),
        (bb[0], bb[5]),
        (bb[3], bb[5]),
    ];
    let mut columns = Vec::with_capacity(4);
    for (x, z) in corners {
        columns.push(generator.base_column(x, z)?);
    }

    let mut projected_y = new_y;
    while projected_y > min_y {
        let mut corners_on_solid_ground = 0;
        for column in &columns {
            let index = projected_y - generator.settings().min_y;
            // `NoiseColumn.getBlock` 越界返回空气，这里用 `0` 个索引之外的值同样判成不透明与否。
            // `NoiseColumn.getBlock` yields air out of range; out-of-bounds indices are treated
            // the same way (never opaque).
            let state = usize::try_from(index)
                .ok()
                .and_then(|index| column.get(index).copied());
            let opaque = state.is_some_and(|state| placement.opaque(state));
            if opaque {
                corners_on_solid_ground += 1;
                if corners_on_solid_ground == 3 {
                    return Ok(projected_y);
                }
            }
        }
        projected_y -= 1;
    }
    Ok(projected_y)
}

/// `RuinedPortalStructure.findGenerationPoint` 的完整产出：起点和结构件列表。
///
/// 起点是 `GenerationStub.position()`——`(chunkMinX, projectedY, chunkMinZ)`，
/// `findValidGenerationPoint` 的 `isValidBiome` 就判这个点，不是包围盒角。
///
/// The full output of `RuinedPortalStructure.findGenerationPoint`: the start and the piece
/// list. The start is `GenerationStub.position()` — `(chunkMinX, projectedY, chunkMinZ)` —
/// and `findValidGenerationPoint`'s `isValidBiome` tests exactly that point, not a box corner.
#[derive(Debug, Clone, PartialEq)]
pub struct PortalStart {
    pub origin: (i32, i32, i32),
    pub pieces: Vec<Piece>,
}

/// `RuinedPortalStructure.findGenerationPoint` → `generatePieces` 的结构件列表。
///
/// 需要 `generator` 的整列方块原语（`findSuitableY`）、`resolver` 的单点群系（`isCold`）
/// 和 `raw` 里的 `setups`。
///
/// The piece list of `RuinedPortalStructure.findGenerationPoint` → `generatePieces`.
/// Needs the generator's full-column primitive (`findSuitableY`), the resolver's single-point
/// biome (`isCold`) and `setups` from `raw`.
pub fn generate(
    generator: &Generator,
    resolver: &mut crate::structure::rings::BiomeResolver,
    seed: i64,
    cx: i32,
    cz: i32,
    raw: &Value,
) -> Result<PortalStart, String> {
    let all = setups(raw)?;
    if all.is_empty() {
        return Err("ruined portal has an empty setups list".to_owned());
    }
    let random = make_random(seed, cx, cz);
    let chosen = if all.len() > 1 {
        let mut total = 0.0f32;
        for setup in &all {
            total += setup.weight;
        }
        let mut pick = random.next_float();
        let mut chosen = None;
        for setup in &all {
            pick -= setup.weight / total;
            if pick < 0.0 {
                chosen = Some(setup);
                break;
            }
        }
        *chosen.ok_or("ruined portal picked no setup")?
    } else {
        all[0]
    };

    let air_pocket = sample(&random, chosen.air_pocket_probability);
    let template_name = if random.next_float() < PROBABILITY_OF_GIANT_PORTAL {
        GIANT_PORTALS[random.next_int(GIANT_PORTALS.len() as i32) as usize]
    } else {
        PORTALS[random.next_int(PORTALS.len() as i32) as usize]
    };
    let template = StructureTemplate::load(template_name)?;
    let rotation = random.next_int(4);
    let mirror = if random.next_float() < 0.5 {
        0
    } else {
        MIRROR_FRONT_BACK
    };
    let pivot = (
        java_floor_div(template.size.0, 2),
        java_floor_div(template.size.2, 2),
    );
    let base = (cx * 16, 0, cz * 16);
    let bb = template_bounds(template.size, rotation, mirror, pivot, base);
    let center = (
        bb[0] + (bb[3] - bb[0] + 1) / 2,
        bb[1] + (bb[4] - bb[1] + 1) / 2,
        bb[2] + (bb[5] - bb[2] + 1) / 2,
    );
    // `getBaseHeight(...) - 1`，而 `getBaseHeight` = `getFirstOccupiedHeight + 1`
    // （整列全空气时是 `minY`），所以这里直接就是 `first_occupied_height`。
    // `getBaseHeight(...) - 1`, and `getBaseHeight` = `getFirstOccupiedHeight + 1` (or `minY`
    // for an all-air column), so it is exactly `first_occupied_height`.
    let surface_y = if chosen.placement.ocean_floor() {
        generator.first_occupied_height_ocean_floor(center.0, center.2)?
    } else {
        generator.first_occupied_height(center.0, center.2)?
    };
    let projected_y = find_suitable_y(
        generator,
        &random,
        chosen.placement,
        air_pocket,
        surface_y,
        bb[4] - bb[1] + 1,
        &bb,
    )?;
    let origin = (base.0, projected_y, base.2);

    // `isCold(origin, biomeResolver.getNoiseBiome(quart(origin)), seaLevel)`。
    // `isCold(origin, biomeResolver.getNoiseBiome(quart(origin)), seaLevel)`.
    let cold = chosen.can_be_cold && {
        let biome = resolver.biome_at(
            java_floor_div(origin.0, 4),
            java_floor_div(origin.1, 4),
            java_floor_div(origin.2, 4),
        );
        cold_biome(&biome, origin, generator.settings().sea_level)?
    };
    let properties = Properties {
        cold,
        mossiness: chosen.mossiness,
        air_pocket,
        overgrown: chosen.overgrown,
        vines: chosen.vines,
        replace_with_blackstone: chosen.replace_with_blackstone,
    };
    Ok(PortalStart {
        origin,
        pieces: vec![portal_piece(PortalPieceSpec {
            template_name,
            template_size: template.size,
            position: origin,
            rotation,
            mirror,
            pivot,
            placement: chosen.placement,
            properties,
        })],
    })
}

/// `isCold(pos, biome, seaLevel)`：群系在这个位置够不够冷得下雪。
///
/// `isCold(pos, biome, seaLevel)`: whether the biome is cold enough to snow at that position.
fn cold_biome(biome: &str, pos: (i32, i32, i32), sea_level: i32) -> Result<bool, String> {
    let id = mc_bamboo_data::BIOME_CLIMATES
        .iter()
        .position(|entry| entry.name == biome)
        .ok_or_else(|| format!("unknown biome {biome}"))?;
    Ok(BiomeClimate::of(id as i32).cold_enough_to_snow(pos.0, pos.1, pos.2, sea_level))
}
