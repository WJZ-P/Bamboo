//! 官方 `StructurePlacement` 的移植：`random_spread` 与 `concentric_rings` 的放置判定。
//!
//! 这里只做「给定种子和区块坐标，哪些区块是结构起点的候选」这一步。结构件的生成
//! （`Structure.generate`）、群系有效性检查、terrain 里的 `Beardifier` 都在别的里程碑。
//! `createStructures` 用 `placement.isStructureChunk(state, x, z)` 当第一道门，本模块
//! 复刻的就是这个方法以及它依赖的随机源取数次数和顺序。
//!
//! Port of vanilla `StructurePlacement`: placement checks for `random_spread` and
//! `concentric_rings`. This covers only "which chunks are structure-start candidates";
//! structure-piece generation, biome validity checks and the terrain `Beardifier` are
//! separate milestones. `createStructures` gates on `isStructureChunk`, reproduced here.

use mc_bamboo_blocks::LegacyRandom;
use serde_json::Value;

use crate::random::{Random, WorldgenRandom};

/// `AbstractSpreadingStructurePlacement.FrequencyReductionMethod`（26.3 的四个值）。
///
/// The four values of `AbstractSpreadingStructurePlacement.FrequencyReductionMethod` in 26.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrequencyReductionMethod {
    Default,
    LegacyType1,
    LegacyType2,
    LegacyType3,
}

impl FrequencyReductionMethod {
    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "default" => Ok(Self::Default),
            "legacy_type_1" => Ok(Self::LegacyType1),
            "legacy_type_2" => Ok(Self::LegacyType2),
            "legacy_type_3" => Ok(Self::LegacyType3),
            _ => Err(format!("unknown frequency_reduction_method {name}")),
        }
    }
}

/// `RandomSpreadType`：`linear` 取一次 `nextInt`，`triangular` 取两次求平均。
///
/// `RandomSpreadType`: `linear` draws one `nextInt`, `triangular` averages two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadType {
    Linear,
    Triangular,
}

impl SpreadType {
    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "linear" => Ok(Self::Linear),
            "triangular" => Ok(Self::Triangular),
            _ => Err(format!("unknown spread_type {name}")),
        }
    }

    /// `RandomSpreadType.evaluate(random, limit)`，两次调用共用同一个随机源。
    ///
    /// `RandomSpreadType.evaluate(random, limit)`; successive calls share the random source.
    fn evaluate(&self, random: &WorldgenRandom, limit: i32) -> i32 {
        match self {
            Self::Linear => random.next_int(limit),
            Self::Triangular => (random.next_int(limit) + random.next_int(limit)) / 2,
        }
    }
}

/// `AbstractSpreadingStructurePlacement.ExclusionZone`：附近 `chunk_count` 格内不能有
/// `other_set` 的结构起点。
///
/// `AbstractSpreadingStructurePlacement.ExclusionZone`: forbids a start when `other_set`
/// has a start within `chunk_count` chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExclusionZone {
    pub other_set: String,
    pub chunk_count: i32,
}

impl ExclusionZone {
    fn parse(value: &Value) -> Result<Self, String> {
        let other_set = value
            .get("other_set")
            .and_then(Value::as_str)
            .ok_or("exclusion_zone missing other_set")?
            .to_owned();
        let chunk_count = value
            .get("chunk_count")
            .and_then(Value::as_i64)
            .ok_or("exclusion_zone missing chunk_count")? as i32;
        Ok(Self {
            other_set,
            chunk_count,
        })
    }
}

/// `AbstractSpreadingStructurePlacement` 里两种放置共用的字段。
///
/// Fields shared by both spreading placements in `AbstractSpreadingStructurePlacement`.
#[derive(Debug, Clone, PartialEq)]
pub struct Common {
    pub locate_offset: [i32; 3],
    pub frequency: f32,
    pub method: FrequencyReductionMethod,
    pub salt: i32,
    pub exclusion_zone: Option<ExclusionZone>,
}

impl Common {
    fn parse(value: &Value) -> Result<Self, String> {
        let locate_offset = match value.get("locate_offset") {
            None => [0, 0, 0],
            Some(v) => {
                let array = v.as_array().ok_or("locate_offset must be an array")?;
                if array.len() != 3 {
                    return Err("locate_offset must have 3 components".to_owned());
                }
                let mut out = [0_i32; 3];
                for (slot, item) in out.iter_mut().zip(array) {
                    *slot = item.as_i64().ok_or("locate_offset must be integers")? as i32;
                }
                out
            }
        };
        let frequency = value
            .get("frequency")
            .and_then(Value::as_f64)
            .unwrap_or(1.0) as f32;
        let method = value
            .get("frequency_reduction_method")
            .and_then(Value::as_str)
            .map(FrequencyReductionMethod::parse)
            .transpose()?
            .unwrap_or(FrequencyReductionMethod::Default);
        let salt = value
            .get("salt")
            .and_then(Value::as_i64)
            .ok_or("placement missing salt")? as i32;
        let exclusion_zone = value
            .get("exclusion_zone")
            .map(ExclusionZone::parse)
            .transpose()?;
        Ok(Self {
            locate_offset,
            frequency,
            method,
            salt,
            exclusion_zone,
        })
    }

    /// `applyAdditionalChunkRestrictions(sourceX, sourceZ, levelSeed)`。
    ///
    /// `frequency == 1` 时官方直接返回 true，不取随机数；否则按方法跑对应的 reducer。
    /// 所有 reducer 都用的是 `new WorldgenRandom(new LegacyRandomSource(0L))`。
    ///
    /// `applyAdditionalChunkRestrictions(sourceX, sourceZ, levelSeed)`.
    /// With `frequency == 1` vanilla returns true without drawing random numbers; otherwise
    /// the matching reducer runs. Every reducer uses `new WorldgenRandom(new
    /// LegacyRandomSource(0L))`.
    pub fn apply_additional_chunk_restrictions(&self, seed: i64, x: i32, z: i32) -> bool {
        if !(self.frequency < 1.0) {
            return true;
        }
        let random = WorldgenRandom::new(Random::Legacy(LegacyRandom::new(0)));
        match self.method {
            FrequencyReductionMethod::Default => {
                // `probabilityReducer` 把 salt 当 x、sourceX 当 z、sourceZ 当 blend，
                // 与另外三个方法的参数直觉不同，照抄 26.3 源码。
                // `probabilityReducer` passes salt as x, sourceX as z, sourceZ as blend,
                // unlike the other three; copied verbatim from the 26.3 sources.
                random.set_large_feature_with_salt(seed, self.salt, x, z);
                random.next_float() < self.frequency
            }
            FrequencyReductionMethod::LegacyType1 => {
                // `legacyPillagerOutpostReducer`：`cx ^ cz<<4 ^ seed` 里 cx/cz 是 int。
                // `legacyPillagerOutpostReducer`: cx/cz in `cx ^ cz<<4 ^ seed` are ints.
                let cx = x >> 4;
                let cz = z >> 4;
                let mixed = i64::from(cx ^ (cz << 4));
                random.set_seed(mixed ^ seed);
                random.next_int_unbounded();
                let bound = (1.0_f32 / self.frequency) as i32;
                random.next_int(bound) == 0
            }
            FrequencyReductionMethod::LegacyType2 => {
                // `legacyArbitrarySaltProbabilityReducer`：忽略配置的 salt，用硬编码常量。
                // `legacyArbitrarySaltProbabilityReducer`: ignores the configured salt.
                random.set_large_feature_with_salt(seed, x, z, 10_387_320);
                random.next_float() < self.frequency
            }
            FrequencyReductionMethod::LegacyType3 => {
                random.set_large_feature_seed(seed, x, z);
                // 官方把 float 概率提升成 double 再比：nextDouble() < probability。
                // Vanilla promotes the float probability to double: nextDouble() < probability.
                random.next_double() < f64::from(self.frequency)
            }
        }
    }
}

/// `RandomSpreadStructurePlacement`。
#[derive(Debug, Clone, PartialEq)]
pub struct RandomSpread {
    pub common: Common,
    pub spacing: i32,
    pub separation: i32,
    pub spread_type: SpreadType,
}

impl RandomSpread {
    fn parse(value: &Value) -> Result<Self, String> {
        let common = Common::parse(value)?;
        let spacing = value
            .get("spacing")
            .and_then(Value::as_i64)
            .ok_or("random_spread missing spacing")? as i32;
        let separation = value
            .get("separation")
            .and_then(Value::as_i64)
            .ok_or("random_spread missing separation")? as i32;
        let spread_type = value
            .get("spread_type")
            .and_then(Value::as_str)
            .map(SpreadType::parse)
            .transpose()?
            .unwrap_or(SpreadType::Linear);
        Ok(Self {
            common,
            spacing,
            separation,
            spread_type,
        })
    }

    /// `RandomSpreadStructurePlacement.getPotentialStructureChunk(seed, sourceX, sourceZ)`。
    ///
    /// 用 `setLargeFeatureWithSalt(seed, floorDiv(x,spacing), floorDiv(z,spacing), salt)`
    /// 播种，再按 `spread_type` 依次求 spreadX / spreadZ，两次取数顺序不能交换。
    ///
    /// `RandomSpreadStructurePlacement.getPotentialStructureChunk(seed, sourceX, sourceZ)`.
    /// Seeds via `setLargeFeatureWithSalt(seed, floorDiv(x,spacing), floorDiv(z,spacing),
    /// salt)`, then evaluates spreadX / spreadZ in order; the two draws must not be swapped.
    pub fn get_potential_structure_chunk(
        &self,
        seed: i64,
        source_x: i32,
        source_z: i32,
    ) -> (i32, i32) {
        let grid_x = source_x.div_euclid(self.spacing);
        let grid_z = source_z.div_euclid(self.spacing);
        let random = WorldgenRandom::new(Random::Legacy(LegacyRandom::new(0)));
        random.set_large_feature_with_salt(seed, grid_x, grid_z, self.common.salt);
        let limit = self.spacing - self.separation;
        let spread_x = self.spread_type.evaluate(&random, limit);
        let spread_z = self.spread_type.evaluate(&random, limit);
        (
            grid_x * self.spacing + spread_x,
            grid_z * self.spacing + spread_z,
        )
    }

    /// `RandomSpreadStructurePlacement.isPlacementChunk`。
    pub fn is_placement_chunk(&self, seed: i64, x: i32, z: i32) -> bool {
        let (px, pz) = self.get_potential_structure_chunk(seed, x, z);
        px == x && pz == z
    }
}

/// `ConcentricRingsStructurePlacement`（要塞）。环位置的确定性几何在本文件，
/// `findBiomeHorizontal` 的群系吸附还没移植（见 `docs/worldgen-structures-plan.md`）。
///
/// `ConcentricRingsStructurePlacement` (strongholds). This file holds the deterministic
/// ring geometry; the `findBiomeHorizontal` biome snapping is not ported yet.
#[derive(Debug, Clone, PartialEq)]
pub struct ConcentricRings {
    pub common: Common,
    pub distance: i32,
    pub spread: i32,
    pub count: i32,
    pub preferred_biomes: String,
}

impl ConcentricRings {
    fn parse(value: &Value) -> Result<Self, String> {
        let common = Common::parse(value)?;
        let distance = value
            .get("distance")
            .and_then(Value::as_i64)
            .ok_or("concentric_rings missing distance")? as i32;
        let spread = value
            .get("spread")
            .and_then(Value::as_i64)
            .ok_or("concentric_rings missing spread")? as i32;
        let count = value
            .get("count")
            .and_then(Value::as_i64)
            .ok_or("concentric_rings missing count")? as i32;
        let preferred_biomes = value
            .get("preferred_biomes")
            .and_then(Value::as_str)
            .ok_or("concentric_rings missing preferred_biomes")?
            .to_owned();
        Ok(Self {
            common,
            distance,
            spread,
            count,
            preferred_biomes,
        })
    }

    /// `ChunkGeneratorStructureState.generateRingPositions` 里的确定性几何：算出每个
    /// 环位置的初始 `(chunkX, chunkZ)`。
    ///
    /// **这不是最终的要塞起点**：官方还要用 `BiomeSource.findBiomeHorizontal` 把每个点
    /// 吸附到 `preferred_biomes`，那一步依赖 uncached 气候采样器，本轮没移植。因此本
    /// 函数的返回值只能当作几何草稿，不能用来判定 `isStructureChunk`，要塞起点属于
    /// 已知限制。
    ///
    /// The deterministic geometry of `ChunkGeneratorStructureState.generateRingPositions`:
    /// the initial `(chunkX, chunkZ)` of each ring position.
    ///
    /// **These are not final stronghold starts**: vanilla further snaps each point to
    /// `preferred_biomes` via `BiomeSource.findBiomeHorizontal`, which needs the uncached
    /// climate sampler and is not ported this round. Treat the result as a geometry draft,
    /// never as `isStructureChunk` input; stronghold starts are a known limitation.
    pub fn ring_positions_geometry(&self, seed: i64) -> Vec<(i32, i32)> {
        // `RandomSource.create().setSeed(concentricRingsSeed)`：普通世界里
        // concentricRingsSeed == levelSeed，且 create() 返回的是 LegacyRandomSource。
        // `RandomSource.create().setSeed(concentricRingsSeed)`: for normal worlds
        // concentricRingsSeed == levelSeed and create() returns a LegacyRandomSource.
        let random = LegacyRandom::new(0);
        random.set_seed(seed);
        let mut angle = random.next_double() * std::f64::consts::PI * 2.0;
        let mut positions = Vec::with_capacity(self.count.max(0) as usize);
        let mut circle = 0_i32;
        let mut position_in_circle = 0_i32;
        let mut spread = self.spread.max(1);
        for i in 0..self.count {
            let dist = 4.0 * f64::from(self.distance)
                + f64::from(self.distance * circle) * 6.0
                + (random.next_double() - 0.5) * f64::from(self.distance) * 2.5;
            // `Math.round(double)` = `floor(a + 0.5)`，不是 Rust 的「远离零取整」。
            // `Math.round(double)` = `floor(a + 0.5)`, not Rust's round-half-away-from-zero.
            let initial_x = (angle.cos() * dist + 0.5).floor() as i32;
            let initial_z = (angle.sin() * dist + 0.5).floor() as i32;
            positions.push((initial_x, initial_z));
            // `random.fork()` 消费一次 nextLong() 当作 findBiomeHorizontal 的搜索种子。
            // 即便本轮不跑吸附，这一步也推进了 random 的状态，后面的角度/距离都依赖它。
            // `random.fork()` consumes one nextLong() as the findBiomeHorizontal seed.
            // Even without snapping this advances `random`, so later angles/distances depend on it.
            let _biome_search_seed = random.next_long();
            angle += (std::f64::consts::PI * 2.0) / f64::from(spread);
            position_in_circle += 1;
            if position_in_circle == spread {
                circle += 1;
                position_in_circle = 0;
                spread += 2 * spread / (circle + 1);
                spread = spread.min(self.count - i);
                angle += random.next_double() * std::f64::consts::PI * 2.0;
            }
        }
        positions
    }
}

/// 官方 `StructurePlacement` 分发的子集：`random_spread` 与 `concentric_rings`。
///
/// The subset of vanilla `StructurePlacement` dispatch: `random_spread` and
/// `concentric_rings`.
#[derive(Debug, Clone, PartialEq)]
pub enum Placement {
    RandomSpread(RandomSpread),
    ConcentricRings(ConcentricRings),
}

impl Placement {
    /// 从 `structure_set` 的 `placement` 对象解析；缺省字段按官方 codec 的默认值补。
    ///
    /// Parse a `structure_set` `placement` object; missing fields take the vanilla codec
    /// defaults.
    pub fn parse(value: &Value) -> Result<Self, String> {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or("placement missing type")?;
        match kind {
            "minecraft:random_spread" => Ok(Self::RandomSpread(RandomSpread::parse(value)?)),
            "minecraft:concentric_rings" => {
                Ok(Self::ConcentricRings(ConcentricRings::parse(value)?))
            }
            _ => Err(format!("unknown placement type {kind}")),
        }
    }

    pub fn common(&self) -> &Common {
        match self {
            Self::RandomSpread(p) => &p.common,
            Self::ConcentricRings(p) => &p.common,
        }
    }

    /// 随机散布放置的参数，供对拍和上层逐集合计算潜在起点用。
    ///
    /// The random-spread parameters, used by the oracle and per-set potential-start math.
    pub fn as_random_spread(&self) -> Option<&RandomSpread> {
        match self {
            Self::RandomSpread(p) => Some(p),
            Self::ConcentricRings(_) => None,
        }
    }

    /// `applyAdditionalChunkRestrictions`（转发到共用字段）。
    pub fn apply_additional_chunk_restrictions(&self, seed: i64, x: i32, z: i32) -> bool {
        self.common()
            .apply_additional_chunk_restrictions(seed, x, z)
    }

    /// `RandomSpreadStructurePlacement.isPlacementChunk`。`concentric_rings` 的
    /// `isPlacementChunk` 需要 ring 位置（依赖群系吸附），本轮返回 false 并标为不支持。
    ///
    /// `RandomSpreadStructurePlacement.isPlacementChunk`. The `concentric_rings` variant
    /// needs ring positions (biome snapping), so this returns false as unsupported here.
    pub fn is_placement_chunk(&self, seed: i64, x: i32, z: i32) -> bool {
        match self {
            Self::RandomSpread(p) => p.is_placement_chunk(seed, x, z),
            Self::ConcentricRings(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(value: Value) -> Placement {
        Placement::parse(&value).expect("placement parses")
    }

    #[test]
    fn random_spread_defaults_match_vanilla_codec() {
        let p = parse(
            json!({"type":"minecraft:random_spread","salt":10387312,"separation":8,"spacing":34}),
        );
        match p {
            Placement::RandomSpread(p) => {
                assert_eq!(p.common.frequency, 1.0);
                assert_eq!(p.common.method, FrequencyReductionMethod::Default);
                assert_eq!(p.spread_type, SpreadType::Linear);
                assert_eq!(p.common.locate_offset, [0, 0, 0]);
                assert!(p.common.exclusion_zone.is_none());
                assert_eq!(p.common.salt, 10387312);
            }
            _ => panic!("expected random_spread"),
        }
    }

    #[test]
    fn frequency_one_never_draws_random() {
        let p =
            parse(json!({"type":"minecraft:random_spread","salt":5,"separation":8,"spacing":34}));
        // frequency == 1 恒 true，任意坐标都不该因种子而变。
        assert!(p.apply_additional_chunk_restrictions(123, 7, -9));
        assert!(p.apply_additional_chunk_restrictions(-1, 0, 0));
    }

    #[test]
    fn potential_chunk_is_deterministic_and_in_grid_cell() {
        let p = parse(
            json!({"type":"minecraft:random_spread","salt":10387312,"separation":8,"spacing":34}),
        );
        let p = p.as_random_spread().expect("random_spread");
        let first = p.get_potential_structure_chunk(42, 100, -100);
        assert_eq!(first, p.get_potential_structure_chunk(42, 100, -100));
        // potential 必须落在 source 所在的 spacing 网格内。
        let (px, pz) = first;
        assert!((0..34).contains(&(px - 100_i32.div_euclid(34) * 34)));
        assert!((0..34).contains(&(pz - (-100_i32).div_euclid(34) * 34)));
    }

    #[test]
    fn concentric_rings_geometry_is_deterministic() {
        let p = parse(json!({
            "type":"minecraft:concentric_rings","salt":0,"distance":32,
            "spread":3,"count":128,"preferred_biomes":"#minecraft:stronghold_biased_to"
        }));
        match p {
            Placement::ConcentricRings(p) => {
                assert_eq!(
                    p.ring_positions_geometry(12345),
                    p.ring_positions_geometry(12345)
                );
                assert_eq!(p.ring_positions_geometry(12345).len(), 128);
                // 吸附没做，这一轮 concentric_rings 不参与 isPlacementChunk。
                assert!(!Placement::ConcentricRings(p).is_placement_chunk(12345, 0, 0));
            }
            _ => panic!("expected concentric_rings"),
        }
    }
}
