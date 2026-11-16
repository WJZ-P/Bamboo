//! `LakeFeature`（岩浆湖）。
//!
//! 照 `LakeFeature.place`：原点先偏移 `(-8, -4, -8)`，在 16×8×16 的格子里按
//! `random.nextInt(4) + 4` 个椭球做布尔遮罩；再沿三个方向各扫一遍：先校验边缘格
//! （`can_place_feature` 与上方流体判定），再填流体/空气，最后铺屏障层。
//! 主世界导出的两个湖（`lake_lava_underground`、`lake_lava_surface`）的流体都是岩浆；
//! 另有 `sulfur_pool` 的内联水湖（群系 `sulfur_caves` 不结冰）。官方末段
//! 「水做的湖在表面结冰」（`Biome.shouldFreeze`）在这批数据里不写方块，但
//! `LakeFeature.place` 仍返回 true，`SequenceFeature` 才能继续放 `potent_sulfur`。
//!
//! `LakeFeature.place`: offset the origin by `(-8, -4, -8)`, fill a 16×8×16 boolean mask
//! from `random.nextInt(4) + 4` ellipsoids, then sweep the grid three times: validate the
//! edge cells (`can_place_feature` plus the liquid check above), write fluid/air, and lay
//! the barrier shell. Both overworld lakes use lava; `sulfur_pool`'s inline water lake is
//! in a biome that never freezes. The freeze branch writes nothing for 26.3 data, but
//! `place` still returns true so `SequenceFeature` continues to the `potent_sulfur` step.

use mc_bamboo_data::{facts, registries, state_facts, state_fluid};
use serde_json::Value;

use crate::random::WorldgenRandom;

use super::{block_predicate::BlockPredicate, state_provider::StateProvider, world::FeatureWorld};

/// 遮罩的尺寸：16×8×16。
const SIZE_X: usize = 16;
const SIZE_Y: usize = 8;
const SIZE_Z: usize = 16;

/// `LakeFeature` 的配置。
#[derive(Debug, Clone)]
pub struct LakeFeature {
    pub fluid: StateProvider,
    pub barrier: StateProvider,
    pub can_place_feature: BlockPredicate,
    pub can_replace_with_air_or_fluid: BlockPredicate,
    pub can_replace_with_barrier: BlockPredicate,
}

impl LakeFeature {
    pub fn parse(config: &Value) -> Result<Self, String> {
        let object = config.as_object().ok_or("lake feature must be an object")?;
        let field = |name: &str| -> Result<&Value, String> {
            object
                .get(name)
                .ok_or_else(|| format!("lake has no {name}"))
        };
        Ok(Self {
            fluid: StateProvider::parse(field("fluid")?)
                .ok_or("lake fluid state provider is unported")?,
            barrier: StateProvider::parse(field("barrier")?)
                .ok_or("lake barrier state provider is unported")?,
            can_place_feature: BlockPredicate::parse(field("can_place_feature")?),
            can_replace_with_air_or_fluid: BlockPredicate::parse(field(
                "can_replace_with_air_or_fluid",
            )?),
            can_replace_with_barrier: BlockPredicate::parse(field("can_replace_with_barrier")?),
        })
    }

    /// 配置里的谓词与状态提供者是否都已移植。
    ///
    /// Whether every predicate and state provider in the config is ported.
    pub fn supported(&self) -> bool {
        self.fluid.supported()
            && self.barrier.supported()
            && self.can_place_feature.is_supported()
            && self.can_replace_with_air_or_fluid.is_supported()
            && self.can_replace_with_barrier.is_supported()
    }
}

/// 遮罩下标：`(x * 16 + z) * 8 + y`，和官方一致。
///
/// The mask index `(x * 16 + z) * 8 + y`, matching the official layout.
fn index(x: usize, y: usize, z: usize) -> usize {
    (x * SIZE_Z + z) * SIZE_Y + y
}

/// 判定「这一格不是遮罩内，但六邻里有遮罩内」的边缘格。
///
/// 官方展开的六个条件是 x±1、z±1，再加上同列的 `y+1` 与 `y-1`
/// （最后一个是 `grid[(xx * 16 + zz) * 8 + (yy - 1)]`，注意不是 `zz - 1`）。
///
/// Whether a cell outside the mask touches the mask on one of its six faces. The official
/// expansion checks x±1, z±1 and the same column's `y+1` / `y-1`; note the last one is
/// `grid[(xx * 16 + zz) * 8 + (yy - 1)]`, not `zz - 1`.
fn is_edge(mask: &[bool; SIZE_X * SIZE_Y * SIZE_Z], x: usize, y: usize, z: usize) -> bool {
    (x < SIZE_X - 1 && mask[index(x + 1, y, z)])
        || (x > 0 && mask[index(x - 1, y, z)])
        || (z < SIZE_Z - 1 && mask[index(x, y, z + 1)])
        || (z > 0 && mask[index(x, y, z - 1)])
        || (y < SIZE_Y - 1 && mask[index(x, y + 1, z)])
        || (y > 0 && mask[index(x, y - 1, z)])
}

/// `LakeFeature.place`。
///
/// `LakeFeature.place`.
pub fn place(
    lake: &LakeFeature,
    world: &mut FeatureWorld<'_>,
    random: &WorldgenRandom,
    origin: (i32, i32, i32),
) -> bool {
    if origin.1 <= world.min_y + 4 {
        return false;
    }
    let (ox, oy, oz) = (origin.0 - 8, origin.1 - 4, origin.2 - 8);
    let mut mask = [false; SIZE_X * SIZE_Y * SIZE_Z];
    let spots = random.next_int(4) + 4;

    for _ in 0..spots {
        let xr = random.next_double() * 6.0 + 3.0;
        let yr = random.next_double() * 4.0 + 2.0;
        let zr = random.next_double() * 6.0 + 3.0;
        let xp = random.next_double() * (16.0 - xr - 2.0) + 1.0 + xr / 2.0;
        let yp = random.next_double() * (8.0 - yr - 4.0) + 2.0 + yr / 2.0;
        let zp = random.next_double() * (16.0 - zr - 2.0) + 1.0 + zr / 2.0;

        for xx in 1..15 {
            for zz in 1..15 {
                for yy in 1..7 {
                    let xd = (f64::from(xx as i32) - xp) / (xr / 2.0);
                    let yd = (f64::from(yy as i32) - yp) / (yr / 2.0);
                    let zd = (f64::from(zz as i32) - zp) / (zr / 2.0);
                    if xd * xd + yd * yd + zd * zd < 1.0 {
                        mask[index(xx, yy, zz)] = true;
                    }
                }
            }
        }
    }

    // 官方的 `fluid.getState(level, random, origin)` 在原点取一次，会消费随机数。
    // The official `fluid.getState(level, random, origin)` draws once at the origin.
    let origin_pos = (ox, oy, oz);
    let Some(fluid) = lake.fluid.get_optional_state(world, random, origin_pos) else {
        return false;
    };

    for xx in 0..SIZE_X {
        for zz in 0..SIZE_Z {
            for yy in 0..SIZE_Y {
                if mask[index(xx, yy, zz)] || !is_edge(&mask, xx, yy, zz) {
                    continue;
                }
                let pos = (ox + xx as i32, oy + yy as i32, oz + zz as i32);
                let state = world.get_block(pos.0, pos.1, pos.2);
                if yy >= 4 && is_liquid(state) {
                    return false;
                }
                if yy < 4 && state_facts(state) & facts::SOLID == 0 && !same_state(state, fluid) {
                    return false;
                }
                if !lake.can_place_feature.test(world, pos) {
                    return false;
                }
            }
        }
    }

    let cave_air = mc_bamboo_data::blocks::CAVE_AIR;
    for xx in 0..SIZE_X {
        for zz in 0..SIZE_Z {
            for yy in 0..SIZE_Y {
                if !mask[index(xx, yy, zz)] {
                    continue;
                }
                let pos = (ox + xx as i32, oy + yy as i32, oz + zz as i32);
                if lake.can_replace_with_air_or_fluid.test(world, pos) {
                    let place_air = yy >= 4;
                    world.set_block_with_heightmaps(
                        pos.0,
                        pos.1,
                        pos.2,
                        if place_air { cave_air } else { fluid },
                    );
                }
            }
        }
    }

    let barrier = lake.barrier.get_optional_state(world, random, origin_pos);
    if let Some(barrier) = barrier
        && state_facts(barrier) & facts::AIR == 0
    {
        for xx in 0..SIZE_X {
            for zz in 0..SIZE_Z {
                for yy in 0..SIZE_Y {
                    if mask[index(xx, yy, zz)] || !is_edge(&mask, xx, yy, zz) {
                        continue;
                    }
                    // 官方条件：`yy < 4 || random.nextInt(2) != 0` 才继续。
                    // `yy > 0` 时短路先不取随机数，和官方 `||` 的求值顺序一致。
                    // Official: continue when `yy < 4 || random.nextInt(2) != 0`; short-circuit
                    // keeps the draw order identical (no draw when `yy < 4`).
                    if yy >= 4 && random.next_int(2) == 0 {
                        continue;
                    }
                    let pos = (ox + xx as i32, oy + yy as i32, oz + zz as i32);
                    let state = world.get_block(pos.0, pos.1, pos.2);
                    if state_facts(state) & facts::SOLID != 0
                        && lake.can_replace_with_barrier.test(world, pos)
                    {
                        world.set_block_with_heightmaps(pos.0, pos.1, pos.2, barrier);
                    }
                }
            }
        }
    }

    // 官方 `LakeFeature.place`：水湖在表面按 `Biome.shouldFreeze` 铺冰，然后
    // **返回 true**。26.3 唯一的水湖是 `sulfur_pool`（内联，群系 `sulfur_caves`
    // 温度足够高，不结冰），所以这里没有结冰方块要写；但必须返回 true，否则
    // `SequenceFeature` 会在第一个子项后短路，`sulfur_pool` 的第二步
    // `simple_block`（放湿润的强效硫磺）整条被跳过。
    // Official `LakeFeature.place` freezes a water lake's surface and then
    // **returns true**. 26.3's only water lake is `sulfur_pool` in the warm
    // `sulfur_caves` biome (no ice), but the return value matters: `SequenceFeature`
    // short-circuits on false, which used to skip the `potent_sulfur` step.
    if state_fluid(fluid).fluid == registries::fluid::WATER {
        // `sulfur_caves` 的 `shouldFreeze` 恒 false；其它群系没有水湖配置。
        // `shouldFreeze` is false for `sulfur_caves`; no other biome has a water lake.
        return true;
    }

    true
}

/// `BlockState.liquid()`：这一格有没有流体（`!getFluidState().isEmpty()`）。
///
/// `BlockState.liquid()`: whether the block has a non-empty fluid state.
fn is_liquid(state: u32) -> bool {
    state_fluid(state).fluid != registries::fluid::EMPTY
}

fn same_state(a: u32, b: u32) -> bool {
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_overworld_lakes() {
        let root: Value =
            serde_json::from_str(mc_bamboo_data::worldgen::WORLDGEN).expect("worldgen json");
        let features = root
            .get("feature")
            .and_then(Value::as_object)
            .expect("feature registry");
        for name in ["minecraft:lake_lava", "minecraft:lake_lava_underground"] {
            let Some(config) = features.get(name) else {
                continue;
            };
            let lake = LakeFeature::parse(config).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(lake.supported(), "{name}");
        }
    }
}
