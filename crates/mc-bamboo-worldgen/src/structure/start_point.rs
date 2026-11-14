//! 四个代码生成结构的 `findGenerationPoint` 前置判定与「这个起点是否真的生成」的纯函数。
//!
//! 逐行照官方 26.3 翻译：`Structure.findValidGenerationPoint` =
//! `findGenerationPoint(context).filter(context::isValidBiome)`，`Structure.generate` 再检查
//! 结构件非空（这四个结构只要 stub 存在就非空）。所以「是否真的生成」= stub 存在且起点
//! 单点群系在 `structure.biomes()` 里。`isValidBiome` 走 `biomeResolver.getNoiseBiome` 的
//! 单点路径，quart 坐标含起点的 Y，这点对矿井/要塞的负 Y 起点尤其重要。
//!
//! 每个结构的 findGenerationPoint 专用判定：
//! * 矿井（普通 / 恶地）：恒有 stub，起点 = (middleX, 50+yOffset, minZ)；yOffset 来自已
//!   移植的 `pieces::mineshaft` 布局，恶地额外按结构件包围盒中心取
//!   `getBaseHeight(WORLD_SURFACE_WG)` 作为 surfaceHeight。
//! * 要塞：恒有 stub，起点 = `chunkPos.getWorldPosition()` = (minX, 0, minZ)。
//! * 海底神殿：`getBiomesWithin` 周边海洋判定 + `couldValidBiomeExistOnTopOfChunkCenter` +
//!   `OCEAN_FLOOR_WG` 的 `getFirstOccupiedHeight` 作为起点 Y。
//! * 林地府邸：`couldValidBiomeExistInTerrainColumn(blockX7, blockZ7)` + 四角最低高度 +
//!   `startPos.getY() < 60`。
//!
//! Pure functions for the four code-built structures' `findGenerationPoint` gates and
//! "whether this start really generates". `findValidGenerationPoint` =
//! `findGenerationPoint(context).filter(context::isValidBiome)`; `Structure.generate` also
//! checks pieces non-empty (always true when a stub exists for these four). So the decision
//! is: stub present AND the start's single-point biome is in `structure.biomes()`.
//! `isValidBiome` uses the single-point `biomeResolver.getNoiseBiome` path, whose quart
//! coords include the stub's Y — this matters for the negative-Y mineshaft/stronghold starts.

use std::collections::HashSet;

use crate::{
    chunk::Generator,
    math::java_floor_div,
    structure::{
        pieces::{self, union, x_span, z_span},
        rings::{BiomeResolver, resolve_allowed_biomes},
        validity::{corner_heights, is_valid_biome_at, lowest_y},
    },
};

const SEA_LEVEL: i32 = 63;
const SURROUND_RADIUS: i32 = 29;

/// `QuartPos.fromBlock(block)` = 向下取整的 `block / 4`（算术右移，负数也向下取整）。
fn quart(block: i32) -> i32 {
    java_floor_div(block, 4)
}

/// 一条查询的坐标与种子，几个纯函数共用，避免函数参数过多。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    pub seed: i64,
    pub cx: i32,
    pub cz: i32,
}

/// 四种结构（矿井分普通 / 恶地两种）的入口标记。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointKind {
    Mineshaft,
    MineshaftMesa,
    Stronghold,
    OceanMonument,
    Mansion,
}

impl PointKind {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "minecraft:mineshaft" => Some(Self::Mineshaft),
            "minecraft:mineshaft_mesa" => Some(Self::MineshaftMesa),
            "minecraft:stronghold" => Some(Self::Stronghold),
            "minecraft:monument" => Some(Self::OceanMonument),
            "minecraft:mansion" => Some(Self::Mansion),
            _ => None,
        }
    }
}

/// `Rotation.getRandom` 的序号（0=NONE, 1=CLOCKWISE_90, 2=CLOCKWISE_180,
/// 3=COUNTERCLOCKWISE_90），来自 `makeRandom(seed, chunkPos).nextInt(4)`。
fn rotation_index(site: Site) -> i32 {
    pieces::make_random(site.seed, site.cx, site.cz).next_int(4)
}

/// 每个 (seed, cx, cz) 只算一次的地形/群系量，与具体结构无关。
pub struct SiteGates {
    /// 中心 + 四角 WORLD_SURFACE_WG 高度 `getFirstOccupiedHeight`。
    pub heights: [i32; 5],
    /// OCEAN_FLOOR_WG 的 `getFirstOccupiedHeight(centerX, centerZ)`。
    pub ocean_floor: i32,
    /// `getBaseHeight(centerX, centerZ, WORLD_SURFACE_WG)` = heights[0] + 1。
    pub base: i32,
    /// `getBiomesWithin` 周边海洋判定（所有群系都在 REQUIRED_OCEAN_MONUMENT_SURROUNDING）。
    pub surrounding: bool,
    /// 林地府邸起点四角最低高度。
    pub lowest: i32,
    pub below_60: bool,
}

/// 复算一个区块的地形/群系量。`surrounding` 走单点 `BiomeResolver`（`getBiomesWithin`
/// 用的是 `biomeSource.createResolver(climateSampler)` 的单点路径）。
pub fn site_gates(
    generator: &Generator,
    resolver: &mut BiomeResolver,
    site: Site,
) -> Result<SiteGates, String> {
    let center_x = site.cx * 16 + 8;
    let center_z = site.cz * 16 + 8;
    let min_x = site.cx * 16;
    let min_z = site.cz * 16;
    let block_x7 = site.cx * 16 + 7;
    let block_z7 = site.cz * 16 + 7;
    let offset_x9 = site.cx * 16 + 9;
    let offset_z9 = site.cz * 16 + 9;

    let heights = [
        generator.first_occupied_height(center_x, center_z)?,
        generator.first_occupied_height(min_x, min_z)?,
        generator.first_occupied_height(min_x, min_z + 16)?,
        generator.first_occupied_height(min_x + 16, min_z)?,
        generator.first_occupied_height(min_x + 16, min_z + 16)?,
    ];
    let ocean_floor = generator.first_occupied_height_ocean_floor(center_x, center_z)?;
    let base = heights[0] + 1;

    // 林地府邸四角：rotation 决定 (±5, ±5)。
    let (offset_x, offset_z) = match rotation_index(site) {
        1 => (-5, 5),
        2 => (-5, -5),
        3 => (5, -5),
        _ => (5, 5),
    };
    let corners = corner_heights(generator, block_x7, block_z7, offset_x, offset_z)?;
    let lowest = lowest_y(corners);
    let below_60 = lowest < 60;

    let surrounding_tag = resolve_allowed_biomes("#minecraft:required_ocean_monument_surrounding")?;
    let surrounding = surrounding_ok(resolver, offset_x9, offset_z9, &surrounding_tag);

    Ok(SiteGates {
        heights,
        ocean_floor,
        base,
        surrounding,
        lowest,
        below_60,
    })
}

/// `BiomeResolver.getBiomesWithin(offsetX, seaLevel, offsetZ, 29)` 的「集合里每个群系都在
/// 标签里」判定。逐格单点 `getNoiseBiome`，顺序与官方一致（最外层 z、中层 x、内层 y）。
/// 官方先取完整集合再逐项判，结果等价于「任一格群系不在标签里即失败」，可短路。
fn surrounding_ok(
    resolver: &mut BiomeResolver,
    offset_x: i32,
    offset_z: i32,
    tag: &HashSet<String>,
) -> bool {
    let x0 = quart(offset_x - SURROUND_RADIUS);
    let x1 = quart(offset_x + SURROUND_RADIUS);
    let y0 = quart(SEA_LEVEL - SURROUND_RADIUS);
    let y1 = quart(SEA_LEVEL + SURROUND_RADIUS);
    let z0 = quart(offset_z - SURROUND_RADIUS);
    let z1 = quart(offset_z + SURROUND_RADIUS);
    for z in z0..=z1 {
        for x in x0..=x1 {
            for y in y0..=y1 {
                let name = resolver.biome_at(x, y, z);
                if !tag.contains(&name) {
                    // 发现一个不在标签里的群系：官方取完整集合再判，结果一样，可短路。
                    // One out-of-tag biome makes the whole check fail; short-circuit is equivalent.
                    return false;
                }
            }
        }
    }
    true
}

/// `GenerationContext.couldStructureExistInColumn(blockX, blockZ, minY-1, maxY)`：
/// 一列 quart Y 里有没有一个合法群系（成批 `createResolverForChunk` 路径）。
fn column_has_valid_biome_at(
    generator: &Generator,
    block_x: i32,
    block_z: i32,
    allowed: &HashSet<String>,
) -> Result<bool, String> {
    let min_y = generator.settings().min_y;
    let height = generator.settings().height;
    let names = generator.column_biome_names(
        quart(block_x),
        quart(block_z),
        quart(min_y - 1),
        quart(min_y + height - 1),
    )?;
    Ok(names.iter().any(|name| allowed.contains(name)))
}

/// 矿井起点 Y = 50 + yOffset = 起始房间（第 0 个结构件）平移后的 `bb[1]`。恶地要先按
/// 结构件包围盒中心取 `getBaseHeight(WORLD_SURFACE_WG)` 作为 surfaceHeight 喂给布局，
/// 才能拿到正确的 yOffset。
fn mineshaft_stub_y(
    kind: PointKind,
    site: Site,
    generator: &Generator,
    gates: &SiteGates,
) -> Result<i32, String> {
    let mesa = kind == PointKind::MineshaftMesa;
    let surface_height = if mesa {
        // 包围盒 X/Z 中心与 surfaceHeight 无关（yOffset 只影响 Y），先用任意值生成一次。
        let first = pieces::mineshaft::generate(site.seed, site.cx, site.cz, true, SEA_LEVEL + 1);
        let boxes: Vec<[i32; 6]> = first.iter().map(|p| p.bb).collect();
        let bb = union(&boxes);
        let center_x = bb[0] + x_span(&bb) / 2;
        let center_z = bb[2] + z_span(&bb) / 2;
        // getBaseHeight(WORLD_SURFACE_WG) = getFirstOccupiedHeight + 1。
        generator.first_occupied_height(center_x, center_z)? + 1
    } else {
        gates.base
    };
    let result = pieces::mineshaft::generate(site.seed, site.cx, site.cz, mesa, surface_height);
    Ok(result[0].bb[1])
}

/// `findGenerationPoint` 的产出：stub 是否存在，以及 stub 位置。`col`/`col7` 已由调用方
/// 用该结构的 `allowed` 集算好（两者都依赖结构的 `validBiome`）。
fn find_generation_point(
    kind: PointKind,
    site: Site,
    generator: &Generator,
    gates: &SiteGates,
    col: bool,
    col7: bool,
) -> Result<(bool, (i32, i32, i32)), String> {
    let center_x = site.cx * 16 + 8;
    let center_z = site.cz * 16 + 8;
    let min_x = site.cx * 16;
    let min_z = site.cz * 16;
    let block_x7 = site.cx * 16 + 7;
    let block_z7 = site.cz * 16 + 7;
    match kind {
        PointKind::Mineshaft | PointKind::MineshaftMesa => {
            // 恒有 stub，起点 Y 来自已移植的矿井布局（含 moveBelowSeaLevel / 恶地 getBaseHeight）。
            let stub_y = mineshaft_stub_y(kind, site, generator, gates)?;
            Ok((true, (center_x, stub_y, min_z)))
        }
        PointKind::Stronghold => Ok((true, (min_x, 0, min_z))),
        PointKind::OceanMonument => {
            // 周边海洋 + 整列判定都过才有 stub。
            Ok((
                gates.surrounding && col,
                (center_x, gates.ocean_floor, center_z),
            ))
        }
        PointKind::Mansion => {
            // 整列判定 + startPos.getY() < 60 门槛。
            Ok((col7 && !gates.below_60, (block_x7, gates.lowest, block_z7)))
        }
    }
}

/// 单结构的复算结果，逐字段与 Java oracle 的 R 行比较。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartPointResult {
    pub valid: bool,
    pub stub: (i32, i32, i32),
    pub col: bool,
    pub col7: bool,
    pub pv: bool,
}

/// 复算「这个起点是否真的生成」：stub 存在且 `isValidBiome(stub)`。
pub fn start_point(
    kind: PointKind,
    site: Site,
    generator: &Generator,
    resolver: &mut BiomeResolver,
    allowed: &HashSet<String>,
    gates: &SiteGates,
) -> Result<StartPointResult, String> {
    let center_x = site.cx * 16 + 8;
    let center_z = site.cz * 16 + 8;
    let block_x7 = site.cx * 16 + 7;
    let block_z7 = site.cz * 16 + 7;
    let col = column_has_valid_biome_at(generator, center_x, center_z, allowed)?;
    let col7 = column_has_valid_biome_at(generator, block_x7, block_z7, allowed)?;
    let (present, stub) = find_generation_point(kind, site, generator, gates, col, col7)?;
    // isValidBiome(stub) 用起点 quart 坐标（含 Y）的单点群系。
    let stub_biome_valid = is_valid_biome_at(resolver, stub.0, stub.1, stub.2, allowed);
    // R 行的 pv = isValidBiome((centerX, h[0], centerZ))，与 stub 判定分开。
    let pv = is_valid_biome_at(resolver, center_x, gates.heights[0], center_z, allowed);
    let valid = present && stub_biome_valid;
    Ok(StartPointResult {
        valid,
        stub: if valid { stub } else { (0, 0, 0) },
        col,
        col7,
        pv,
    })
}
