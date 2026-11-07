//! 天空光计算。
//!
//! 所有非空气方块都当作不透明：每一列最高的方块以上是满亮度，以下全黑。
//! 这只是区块发送时的初始数据，方块改动之后客户端自己重算。透明方块和光的扩散以后再做。

use crate::{MIN_Y, SECTION_COUNT, SECTION_SIZE, chunk::Chunk};

/// 一个区块段的光照数据大小：4096 个位置，每个 4 位。
pub const LIGHT_LAYER_BYTES: usize = 2048;
/// 光照数据比区块段多两层：世界底部下面和顶部上面各一层。
pub const LIGHT_SECTION_COUNT: usize = SECTION_COUNT + 2;

const FULL_LIGHT: u8 = 15;

/// 一个区块段的天空光。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkyLight {
    /// 全黑。
    Dark,
    /// 全亮。
    Full,
    /// 明暗混合。每字节两个位置，低 4 位在前。
    Partial(Box<[u8; LIGHT_LAYER_BYTES]>),
}

/// 计算一个区块每层的天空光，从世界底部下面一层开始。
pub fn sky_light(chunk: &Chunk) -> Vec<SkyLight> {
    let mut surfaces = [0; 256];
    for (index, surface) in surfaces.iter_mut().enumerate() {
        *surface = chunk.surface_y(index % 16, index / 16);
    }
    let lowest = surfaces.iter().copied().min().unwrap_or(MIN_Y);
    let highest = surfaces.iter().copied().max().unwrap_or(MIN_Y);

    (0..LIGHT_SECTION_COUNT as i32)
        .map(|layer| {
            let bottom = MIN_Y + (layer - 1) * SECTION_SIZE;
            let top = bottom + SECTION_SIZE;
            if bottom >= highest {
                SkyLight::Full
            } else if top <= lowest {
                SkyLight::Dark
            } else {
                SkyLight::Partial(Box::new(partial_layer(bottom, &surfaces)))
            }
        })
        .collect()
}

fn partial_layer(bottom: i32, surfaces: &[i32; 256]) -> [u8; LIGHT_LAYER_BYTES] {
    let mut layer = [0; LIGHT_LAYER_BYTES];
    for local_y in 0..SECTION_SIZE {
        for (column, &surface) in surfaces.iter().enumerate() {
            if bottom + local_y >= surface {
                let index = (local_y as usize) << 8 | column;
                layer[index / 2] |= FULL_LIGHT << (index % 2 * 4);
            }
        }
    }
    layer
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::blocks;

    use super::*;

    fn chunk_with_surface_at(y: i32) -> Chunk {
        let mut chunk = Chunk::empty(0);
        for z in 0..16 {
            for x in 0..16 {
                chunk.set_block(x, y - 1, z, blocks::STONE);
            }
        }
        chunk
    }

    #[test]
    fn has_a_layer_below_and_above_the_world() {
        assert_eq!(sky_light(&Chunk::empty(0)).len(), LIGHT_SECTION_COUNT);
    }

    #[test]
    fn everything_above_the_surface_is_bright() {
        let light = sky_light(&chunk_with_surface_at(-60));

        assert_eq!(light[0], SkyLight::Dark);
        assert!(matches!(light[1], SkyLight::Partial(_)));
        assert!(light[2..].iter().all(|layer| *layer == SkyLight::Full));
    }

    #[test]
    fn partial_layer_is_dark_below_the_surface_and_bright_above() {
        let light = sky_light(&chunk_with_surface_at(-60));
        let SkyLight::Partial(layer) = &light[1] else {
            panic!("地表所在的一层应该明暗混合");
        };

        // 这一层底部是 -64，所以前 4 层全黑，第 5 层开始全亮。
        assert!(layer[..512].iter().all(|&byte| byte == 0));
        assert!(layer[512..].iter().all(|&byte| byte == 0xff));
    }

    #[test]
    fn surface_on_a_section_boundary_needs_no_partial_layer() {
        let light = sky_light(&chunk_with_surface_at(-48));

        assert_eq!(light[0], SkyLight::Dark);
        assert_eq!(light[1], SkyLight::Dark);
        assert_eq!(light[2], SkyLight::Full);
    }
}
