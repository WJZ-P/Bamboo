//! 超平坦的群系地物列表和推迟到装饰阶段的地层。
//!
//! Superflat biome feature selection and layers deferred until decoration.

use super::{
    FeatureChunks,
    provider::WorldGenerationContext,
    registry::Registry,
    world::{FeatureWorld, HeightmapKind, is_heightmap_opaque},
};
use mc_bamboo_data::{blocks, registries};

const LAKES: [&str; 2] = [
    "minecraft:lake_lava_underground",
    "minecraft:lake_lava_surface",
];

pub const SEA_LEVEL: i32 = -63;

pub struct FlatDecoration<'a> {
    pub biome: u32,
    pub features: bool,
    pub lakes: bool,
    pub layers: &'a [(u32, i32)],
}

impl FlatDecoration<'_> {
    pub fn generation(&self, input: &FeatureChunks) -> WorldGenerationContext {
        WorldGenerationContext {
            min_y: input.min_y.max(0),
            height: input.height.min(384),
            sea_level: SEA_LEVEL,
        }
    }

    fn names_at<'a>(&self, registry: &'a Registry, step: usize) -> &'a [String] {
        let is_void =
            registries::biome::NAMES.get(self.biome as usize) == Some(&"minecraft:the_void");
        let void_layers = self
            .layers
            .iter()
            .all(|&(state, count)| state == blocks::AIR || count == 0);
        if self.features && (!void_layers || is_void) && !matches!(step, 3 | 4) {
            registry.biome_features(self.biome, step)
        } else {
            &[]
        }
    }

    /// 固定群系只有一个序列，FeatureSorter 的拓扑序就是该序列（每个阶段分别编号）。
    ///
    /// A fixed biome supplies a single chain to FeatureSorter, numbered separately per step.
    pub(super) fn features_at(&self, registry: &Registry, step: usize) -> Result<Vec<u32>, String> {
        if step == 1 && self.lakes {
            LAKES
                .iter()
                .map(|name| {
                    registry
                        .placed_id(name)
                        .ok_or_else(|| format!("missing {name}"))
                })
                .collect()
        } else {
            self.names_at(registry, step)
                .iter()
                .map(|name| {
                    registry
                        .placed_id(name)
                        .ok_or_else(|| format!("missing {name}"))
                })
                .collect()
        }
    }

    pub(crate) fn has_feature(&self, registry: &Registry, id: u32) -> bool {
        (0..super::DECORATION_STEPS).any(|step| {
            if step == 1 && self.lakes {
                LAKES
                    .iter()
                    .any(|name| registry.placed_id(name) == Some(id))
            } else {
                self.names_at(registry, step)
                    .iter()
                    .any(|name| registry.placed_id(name) == Some(id))
            }
        })
    }

    /// FillLayerFeature 在最后一个步骤逐格补上非 MOTION_BLOCKING 地层，不覆盖现有方块。
    ///
    /// FillLayerFeature restores non-MOTION_BLOCKING layers last, without replacing occupied cells.
    pub(super) fn fill_layers(&self, world: &mut FeatureWorld<'_>, origin: (i32, i32, i32)) {
        let (x, mut y, z) = origin;
        for &(state, thickness) in self.layers {
            if state != blocks::AIR && !terrain_layer(state) {
                for layer in y..y + thickness {
                    for dx in 0..16 {
                        for dz in 0..16 {
                            if world.is_air(x + dx, layer, z + dz) {
                                world.set_block_with_heightmaps(x + dx, layer, z + dz, state);
                            }
                        }
                    }
                }
            }
            y += thickness;
        }
    }
}

pub fn terrain_layer(state: u32) -> bool {
    is_heightmap_opaque(state, HeightmapKind::MotionBlocking)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_decoration_distinguishes_lakes_and_void_biome() {
        let data = crate::Worldgen::load().unwrap();
        let registry = Registry::parse(data.raw()).unwrap();
        let biome = registry.biome_id("minecraft:plains").unwrap();
        let mut flat = FlatDecoration {
            biome,
            features: true,
            lakes: true,
            layers: &[],
        };
        assert_eq!(
            flat.features_at(&registry, 1).unwrap(),
            LAKES.map(|name| registry.placed_id(name).unwrap())
        );
        assert!(flat.features_at(&registry, 9).unwrap().is_empty());
        flat.biome = registry.biome_id("minecraft:the_void").unwrap();
        assert_eq!(
            flat.features_at(&registry, 10).unwrap(),
            [registry.placed_id("minecraft:void_start_platform").unwrap()]
        );
        flat.features = false;
        assert!(flat.features_at(&registry, 10).unwrap().is_empty());
        assert!(!terrain_layer(blocks::AIR));
        assert!(terrain_layer(blocks::STONE));
    }
}
