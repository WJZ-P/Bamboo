//! 生成期刷怪接进 Director：`ChunkStatus.SPAWN` 的 `ChunkGenerator.spawnOriginalMobs`。
//!
//! Director 的计算线程在 `spawn` 阶段还拿不到最终方块和光照（它们在世界线程完工组装时
//! 才合成），所以这一步在组装好交付盒子之后、同一个世界线程上跑：读这一柱的方块、
//! 高度图和生成期光照，按 `NoiseBasedChunkGenerator.spawnOriginalMobs` 建两个随机源，
//! 调 [`spawn_mobs_for_chunk_generation`]，生成的生物放进盒子的实体表，随区块交付和存盘。
//! 一个区块大约只取一两个随机数，开销很小。
//!
//! World-generation spawning in the Director: `ChunkGenerator.spawnOriginalMobs` of
//! `ChunkStatus.SPAWN`. Compute threads do not have the final blocks and light at the
//! `spawn` stage (the world thread assembles them on completion), so this runs on the
//! world thread right after assembly: it reads the column's blocks, heightmap and
//! generation light, builds the two randoms of `spawnOriginalMobs`, calls
//! [`spawn_mobs_for_chunk_generation`] and puts the mobs in the box's entity table, so
//! they are delivered and saved with the chunk. A chunk draws one or two numbers on
//! average, so the cost is small.

use mc_bamboo_blocks::{
    Aabb, BlockEntities, BlockEvents, BlockState, BlockTicks, EntityStore, EntityTables, Level,
    LevelRandom, LevelTables, RedstoneMemory, World,
    entity::{
        JavaRandom,
        mob::natural_gen::{GenSpawn, spawn_mobs_for_chunk_generation},
    },
};
use mc_bamboo_core::{BlockPos, ChunkPos, LightLayer, sign::SignContent};
use mc_bamboo_data::spawning::{self, MobCategory};
use mc_bamboo_world::{MIN_Y, WORLD_HEIGHT};
use mc_bamboo_worldgen::{
    chunk,
    random::{Random, WorldgenRandom},
};
use tracing::warn;

use crate::{region::RegionColumn, world_options::WorldGenConfig, worldgen::with_generator};

/// `spawnOriginalMobs`：超平坦（`FlatLevelSource`）什么都不做；噪声世界的三种预设都没有
/// `disable_mob_generation`。`spawn_mobs` 是游戏规则 `SPAWN_MOBS`。
///
/// `spawnOriginalMobs`: superflat (`FlatLevelSource`) does nothing; none of the three
/// noise presets sets `disable_mob_generation`. `spawn_mobs` is the `SPAWN_MOBS` rule.
pub(crate) fn spawn_original_mobs(
    config: &WorldGenConfig,
    pos: ChunkPos,
    column: &mut RegionColumn,
    spawn_mobs: bool,
) {
    if !spawn_mobs || config.level_type.settings_id().is_none() {
        return;
    }
    let rows = with_generator(config, |generator| {
        Ok(spawn_in_column(generator, config.seed, pos, column))
    });
    match rows {
        Ok(rows) => column.add_generated_entities(rows),
        Err(error) => warn!(%error, x = pos.x, z = pos.z, "world-generation spawning skipped"),
    }
}

/// 在一柱上跑完 `spawnMobsForChunkGeneration`，返回实体行。
///
/// Run `spawnMobsForChunkGeneration` on one column and return the entity rows.
fn spawn_in_column(
    generator: &chunk::Generator<'static>,
    seed: i64,
    pos: ChunkPos,
    column: &RegionColumn,
) -> Vec<mc_bamboo_blocks::entity::EntityRow> {
    let min_x = pos.x * 16;
    let min_z = pos.z * 16;
    let max_y = MIN_Y + WORLD_HEIGHT - 1;
    // `sourcePos = center.getWorldPosition().atY(maxY)`；刷怪表是全分辨率群系
    // （`Vec3.atCenterOf` 再取整，回到同一格），概率走 quart 群系。
    // `sourcePos` is the chunk origin at the top; the table reads the full-resolution
    // biome, the probability the quart biome.
    let table = spawning::for_biome_id(generator.region_biome(min_x, max_y, min_z) as i32);
    let probability =
        spawning::for_biome_id(
            generator.region_noise_biome(min_x >> 2, max_y >> 2, min_z >> 2) as i32,
        )
        .creature_spawn_probability;
    let decoration = WorldgenRandom::new(Random::Legacy(mc_bamboo_blocks::LegacyRandom::new(0)))
        .set_decoration_seed(seed, min_x, min_z);
    let random = mc_bamboo_blocks::LegacyRandom::new(decoration);
    let region_random = generator.region_random_at(pos.x, pos.z);
    let mut level = GenSpawnLevel::new(generator, seed, pos, column);
    let mut ticks = BlockTicks::new();
    let mut world = World::new(&mut level, &mut ticks);
    spawn_mobs_for_chunk_generation(
        &mut world,
        &GenSpawn {
            chunk: pos,
            mobs: table.mobs_to_spawn(MobCategory::Creature),
            probability,
            random: &random,
            region_random: &region_random,
        },
    )
    .into_iter()
    .map(|entity| entity.split())
    .collect()
}

/// 生成期区域的只读世界：中心柱的方块、光照和群系；没有实体、玩家和方块实体。
/// 中心之外的格子当空气（`spawn` 只要求邻居到 `biomes`，官方那边读到的也多半是空的）。
///
/// The read-only world of a generation region: the center column's blocks, light and
/// biomes; no entities, players or block entities. Cells outside the center read as
/// air (`spawn` only needs neighbors at `biomes`, which are mostly empty in vanilla too).
struct GenSpawnLevel<'a> {
    generator: &'a chunk::Generator<'static>,
    pos: ChunkPos,
    column: &'a RegionColumn,
    block_entities: BlockEntities,
    block_events: BlockEvents,
    redstone: RedstoneMemory,
    entities: EntityStore,
    random: LevelRandom,
}

impl<'a> GenSpawnLevel<'a> {
    /// 实体种子源按世界种子和区块坐标派生，每个区块生成的 UUID 各不相同、可重放。
    /// 官方实体的随机源不带种子，这里的值不影响任何对拍项。
    ///
    /// The entity seed source derives from the world seed and chunk, so UUIDs differ per
    /// chunk and replay identically. Vanilla's entity random is unseeded, so no compared
    /// value depends on this.
    fn new(
        generator: &'a chunk::Generator<'static>,
        seed: i64,
        pos: ChunkPos,
        column: &'a RegionColumn,
    ) -> Self {
        let mut entities = EntityStore::new();
        let chunk_seed = seed
            ^ i64::from(pos.x).wrapping_mul(0x2F0F_1F25_37C5_4E1B)
            ^ i64::from(pos.z).wrapping_mul(0x5851_F42D_4C95_7F2D);
        entities.set_entity_seed_source(JavaRandom::new(chunk_seed));
        Self {
            generator,
            pos,
            column,
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            redstone: RedstoneMemory::default(),
            entities,
            random: LevelRandom::new(seed),
        }
    }

    fn in_center(&self, pos: BlockPos) -> bool {
        pos.x >> 4 == self.pos.x
            && pos.z >> 4 == self.pos.z
            && (MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&pos.y)
    }
}

impl LevelTables for GenSpawnLevel<'_> {
    fn block_entities_table(&self) -> &BlockEntities {
        &self.block_entities
    }

    fn block_entities_table_mut(&mut self) -> &mut BlockEntities {
        &mut self.block_entities
    }

    fn block_events_table(&self) -> &BlockEvents {
        &self.block_events
    }

    fn block_events_table_mut(&mut self) -> &mut BlockEvents {
        &mut self.block_events
    }

    fn redstone_table(&self) -> &RedstoneMemory {
        &self.redstone
    }

    fn redstone_table_mut(&mut self) -> &mut RedstoneMemory {
        &mut self.redstone
    }
}

impl EntityTables for GenSpawnLevel<'_> {
    fn entities_table(&self) -> &EntityStore {
        &self.entities
    }

    fn entities_table_mut(&mut self) -> &mut EntityStore {
        &mut self.entities
    }
}

impl Level for GenSpawnLevel<'_> {
    fn block_state(&self, pos: BlockPos) -> BlockState {
        if !(MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&pos.y) {
            return BlockState(mc_bamboo_data::blocks::VOID_AIR);
        }
        if !self.in_center(pos) {
            return BlockState(mc_bamboo_data::blocks::AIR);
        }
        let chunk = self.column.chunk_column().chunk();
        BlockState(chunk.block((pos.x & 15) as usize, pos.y, (pos.z & 15) as usize))
    }

    fn write_block(&mut self, _pos: BlockPos, _state: BlockState, _flags: u32) -> bool {
        false
    }

    fn is_unobstructed(&self, _boxes: &[Aabb]) -> bool {
        true
    }

    fn min_y(&self) -> i32 {
        MIN_Y
    }

    fn max_y(&self) -> i32 {
        MIN_Y + WORLD_HEIGHT - 1
    }

    fn random(&self) -> &LevelRandom {
        &self.random
    }

    fn is_randomly_ticking_section(&self, _chunk: ChunkPos, _section_y: i32) -> bool {
        false
    }

    fn any_player_close_enough_to(&self, _pos: BlockPos, _max_distance: i32) -> bool {
        false
    }

    /// 中心柱读生成期光照；外面按没有光照数据算（天空 15，方块 0）。
    ///
    /// The center reads generation light; outside counts as no light data.
    fn brightness(&self, layer: LightLayer, pos: BlockPos) -> i32 {
        if self.in_center(pos) {
            return i32::from(self.column.chunk_column().light_value(layer, pos));
        }
        match layer {
            LightLayer::Sky => 15,
            LightLayer::Block => 0,
        }
    }

    /// `WorldGenRegion.getBiome`：八角插值后夹进区块高度的 quart 群系。
    ///
    /// `WorldGenRegion.getBiome`: the zoomed quart biome clamped into the chunk height.
    fn biome_id(&self, pos: BlockPos) -> i32 {
        self.generator.region_biome(pos.x, pos.y, pos.z) as i32
    }

    fn sign(&self, _pos: BlockPos) -> Option<SignContent> {
        None
    }

    fn set_sign(&mut self, _pos: BlockPos, _content: SignContent) -> bool {
        false
    }

    fn note_block_sound(&self, _pos: BlockPos) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::registries::entity_type;

    use super::*;
    use crate::world_options::LevelType;

    /// 模板柱（草方块地面、已算好光照）放在真实主世界群系上：一批区块里能刷出牛，
    /// 每头牛都落在自己的区块、站在草上；同一区块重跑结果相同。
    ///
    /// Template columns (grass, baked light) over real overworld biomes: some cows
    /// spawn, each inside its own chunk on the grass, and reruns are identical.
    #[test]
    fn cows_spawn_inside_their_chunk_and_replay_identically() {
        let config = WorldGenConfig {
            seed: 1234,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        let mut cows = 0;
        for x in 0..24 {
            for z in 0..24 {
                // 隔开取样，避开大片海洋。
                // Spread the samples to step over large oceans.
                let pos = ChunkPos::new(x * 9, z * 9);
                let column = RegionColumn::from_template();
                let rows = with_generator(&config, |generator| {
                    Ok(spawn_in_column(generator, config.seed, pos, &column))
                })
                .unwrap();
                let again = with_generator(&config, |generator| {
                    Ok(spawn_in_column(generator, config.seed, pos, &column))
                })
                .unwrap();
                assert_eq!(rows.len(), again.len());
                for (row, replay) in rows.iter().zip(&again) {
                    assert_eq!(row.public.position, replay.public.position);
                    // 生成期刷怪现在会放所有建模了的动物，不只是牛。
                    // Generation spawning now places every modelled animal, not only cows.
                    let modelled = [
                        entity_type::COW,
                        entity_type::PIG,
                        entity_type::SHEEP,
                        entity_type::CHICKEN,
                        entity_type::RABBIT,
                        entity_type::FOX,
                        entity_type::WOLF,
                        entity_type::HORSE,
                        entity_type::DONKEY,
                        entity_type::MULE,
                        entity_type::LLAMA,
                        entity_type::ARMADILLO,
                        entity_type::GOAT,
                    ];
                    assert!(modelled.contains(&row.public.kind.entity_type()));
                    assert_eq!(
                        row.public.kind.entity_type(),
                        replay.public.kind.entity_type()
                    );
                    let at = row.public.position;
                    assert_eq!(BlockPos::containing(at).chunk(), pos, "生物要落在本区块");
                    if row.public.kind.entity_type() == entity_type::COW {
                        cows += 1;
                    }
                }
            }
        }
        assert!(cows > 0, "576 个分散的区块里应该刷出牛");
    }

    /// `ChunkAccess.getNoiseBiome` 把 quart y 夹进区块高度：顶上之外的查询等于最顶一层。
    ///
    /// `ChunkAccess.getNoiseBiome` clamps the quart y into the chunk: queries above the
    /// top equal the topmost layer.
    #[test]
    fn region_biomes_clamp_the_quart_height() {
        let config = WorldGenConfig {
            seed: 99,
            level_type: LevelType::Normal,
            ..WorldGenConfig::default()
        };
        with_generator(&config, |generator| {
            for (x, z) in [(0, 0), (37, -12), (-80, 64)] {
                let top = generator.region_noise_biome(x, 79, z);
                assert_eq!(generator.region_noise_biome(x, 80, z), top);
                assert_eq!(generator.region_noise_biome(x, 400, z), top);
                let bottom = generator.region_noise_biome(x, -16, z);
                assert_eq!(generator.region_noise_biome(x, -40, z), bottom);
            }
            Ok(())
        })
        .unwrap();
    }
}
