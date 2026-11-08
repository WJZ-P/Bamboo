//! 官方 `levelgen/synth` 下的全部噪声：`GradientNoise`、`PerlinNoise`、`SmearedPerlinNoise`、
//! `SimplexNoise`、`NoiseStack`、`NormalNoise`、`LegacyFbmInitializer` 和 `BlendedNoise` 的 fbm。
//!
//! 每种噪声都有两条求值路径，官方的两条路径算出来的最后一位不同，都要移植：
//!
//! * [`Noise::get`] 是官方的 `Noise.get(x, y, z)`，密度函数单点求值走这条；
//! * [`Noise::add_to_point`] 是官方的 `Noise.addToVolume` 在一个方块坐标上的贡献，
//!   成批求值走这条。它把梯度点积拆成「xz 的部分加 y 的部分」，和 `get` 的
//!   `gradDot` 结合顺序不同；坐标也是 `方块坐标 * (外层系数 * 频率)`，
//!   而 `get` 是 `(方块坐标 * 外层系数) * 频率`。
//!   `interpolated` 密度函数在单点求值时读的是这条路径，所以两条都用得到。

use crate::{
    interval::Interval,
    math::{floor, lerp3, smoothstep},
    random::Random,
};

/// `GradientNoise.GRADIENT`：16 个梯度，后四个是前面的重复。
const GRADIENT: [[i32; 3]; 16] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
    [1, 1, 0],
    [0, -1, 1],
    [-1, 1, 0],
    [0, -1, -1],
];
/// `GradientNoise.ROUND_OFF`。
const ROUND_OFF: f64 = 3.3554432e7;
/// `GradientNoise.HALF_ROUND_OFF`，官方是 `Math.nextDown(1.6777216E7)`。
const HALF_ROUND_OFF: f64 = 16_777_215.999_999_998;
/// `PerlinNoise.STANDARD_DEVIATION`。
pub const PERLIN_STANDARD_DEVIATION: f64 = 0.270_224_783_124_521_1;
/// `SmearedPerlinNoise.SHIFT_UP_EPSILON`，官方是 float 字面量，用在 double 运算里。
const SHIFT_UP_EPSILON: f64 = 1.0e-7_f32 as f64;

/// `GradientNoise.wrap`：坐标太大时折回来，`DEBUG_ENABLE_FARLANDS` 关着。
fn wrap(x: f64) -> f64 {
    if (-HALF_ROUND_OFF..HALF_ROUND_OFF).contains(&x) {
        x
    } else {
        x - (x / ROUND_OFF + 0.5).floor() * ROUND_OFF
    }
}

/// `GradientNoise` 的共同部分：三个偶然偏移和 256 个置换。
#[derive(Debug, Clone)]
struct GradientNoise {
    perms: [u8; 256],
    offset_x: f64,
    offset_y: f64,
    offset_z: f64,
}

impl GradientNoise {
    /// `GradientNoise(random, noiseOffsetScale)`：先取三个 double，再洗一遍置换表。
    fn new(random: &Random, noise_offset_scale: f64) -> Self {
        let offset_x = random.next_double() * noise_offset_scale;
        let offset_y = random.next_double() * noise_offset_scale;
        let offset_z = random.next_double() * noise_offset_scale;
        let mut perms = [0u8; 256];
        for (index, slot) in perms.iter_mut().enumerate() {
            *slot = index as u8;
        }
        for index in 0..256usize {
            let offset = random.next_int(256 - index as i32) as usize;
            perms.swap(index, index + offset);
        }
        Self {
            perms,
            offset_x,
            offset_y,
            offset_z,
        }
    }

    /// `permute(x)`。
    fn permute(&self, x: i32) -> i32 {
        i32::from(self.perms[(x & 0xFF) as usize])
    }

    /// `permuteToGrad(x)`。
    fn permute_to_grad(&self, x: i32) -> [i32; 3] {
        GRADIENT[(self.permute(x) & 15) as usize]
    }
}

/// `GradientNoise.Gradient.dot(float, float, float)`，也就是 `gradDot`。
fn grad_dot(gradient: [i32; 3], x: f32, y: f32, z: f32) -> f32 {
    gradient[0] as f32 * x + gradient[1] as f32 * y + gradient[2] as f32 * z
}

/// `GradientNoise.Gradient.dotXz(float, float)`。
fn grad_dot_xz(gradient: [i32; 3], x: f32, z: f32) -> f32 {
    gradient[0] as f32 * x + gradient[2] as f32 * z
}

/// `GradientNoise.Gradient.dot(double, double, double)`。
fn grad_dot_f64(gradient: [i32; 3], x: f64, y: f64, z: f64) -> f64 {
    f64::from(gradient[0]) * x + f64::from(gradient[1]) * y + f64::from(gradient[2]) * z
}

/// 一个格子八个角的梯度，按 `sampleAndLerp` 的下标顺序。
struct CornerGradients {
    gradients: [[i32; 3]; 8],
}

impl GradientNoise {
    /// `sampleAndLerp` 和 `addToVolume` 共用的那八次置换。
    fn corners(&self, cell: (i32, i32, i32)) -> CornerGradients {
        let (x, y, z) = cell;
        let x0 = self.permute(x);
        let x1 = self.permute(x.wrapping_add(1));
        let xy00 = self.permute(x0.wrapping_add(y));
        let xy01 = self.permute(x0.wrapping_add(y).wrapping_add(1));
        let xy10 = self.permute(x1.wrapping_add(y));
        let xy11 = self.permute(x1.wrapping_add(y).wrapping_add(1));
        let z1 = z.wrapping_add(1);
        CornerGradients {
            gradients: [
                self.permute_to_grad(xy00.wrapping_add(z)),
                self.permute_to_grad(xy10.wrapping_add(z)),
                self.permute_to_grad(xy01.wrapping_add(z)),
                self.permute_to_grad(xy11.wrapping_add(z)),
                self.permute_to_grad(xy00.wrapping_add(z1)),
                self.permute_to_grad(xy10.wrapping_add(z1)),
                self.permute_to_grad(xy01.wrapping_add(z1)),
                self.permute_to_grad(xy11.wrapping_add(z1)),
            ],
        }
    }
}

impl GradientNoise {
    /// `PerlinNoise.sampleAndLerp`。`original_relative_y` 只用来算 y 方向的插值系数。
    fn sample_and_lerp(
        &self,
        cell: (i32, i32, i32),
        relative: (f32, f32, f32),
        original_relative_y: f32,
    ) -> f32 {
        let corners = self.corners(cell);
        let alpha = (
            smoothstep(relative.0),
            smoothstep(original_relative_y),
            smoothstep(relative.2),
        );
        lerp3(alpha, corners.dots(relative))
    }
}

impl CornerGradients {
    /// `PerlinNoise.sampleAndLerp` 的八个点积。
    fn dots(&self, relative: (f32, f32, f32)) -> [f32; 8] {
        let (rx, ry, rz) = relative;
        let (rx1, ry1, rz1) = (rx - 1.0, ry - 1.0, rz - 1.0);
        let grads = &self.gradients;
        [
            grad_dot(grads[0], rx, ry, rz),
            grad_dot(grads[1], rx1, ry, rz),
            grad_dot(grads[2], rx, ry1, rz),
            grad_dot(grads[3], rx1, ry1, rz),
            grad_dot(grads[4], rx, ry, rz1),
            grad_dot(grads[5], rx1, ry, rz1),
            grad_dot(grads[6], rx, ry1, rz1),
            grad_dot(grads[7], rx1, ry1, rz1),
        ]
    }

    /// `addToVolume` 的八个点积：xz 的部分和 y 的部分分开算，和 [`Self::dots`] 的结合顺序不同。
    fn dots_split(&self, relative_xz: (f32, f32), relative_y: f32) -> [f32; 8] {
        let (rx, rz) = relative_xz;
        let (rx1, rz1) = (rx - 1.0, rz - 1.0);
        let ry = relative_y;
        let ry1 = relative_y - 1.0;
        let grads = &self.gradients;
        [
            grad_dot_xz(grads[0], rx, rz) + grads[0][1] as f32 * ry,
            grad_dot_xz(grads[1], rx1, rz) + grads[1][1] as f32 * ry,
            grad_dot_xz(grads[2], rx, rz) + grads[2][1] as f32 * ry1,
            grad_dot_xz(grads[3], rx1, rz) + grads[3][1] as f32 * ry1,
            grad_dot_xz(grads[4], rx, rz1) + grads[4][1] as f32 * ry,
            grad_dot_xz(grads[5], rx1, rz1) + grads[5][1] as f32 * ry,
            grad_dot_xz(grads[6], rx, rz1) + grads[6][1] as f32 * ry1,
            grad_dot_xz(grads[7], rx1, rz1) + grads[7][1] as f32 * ry1,
        ]
    }
}

/// `PerlinNoise`。
#[derive(Debug, Clone)]
pub struct PerlinNoise {
    base: GradientNoise,
}

impl PerlinNoise {
    /// `PerlinNoise(random)`：偏移的系数是 256。
    pub fn new(random: &Random) -> Self {
        Self {
            base: GradientNoise::new(random, 256.0),
        }
    }

    /// `PerlinNoise.get(x, y, z)`。
    fn get(&self, x: f64, y: f64, z: f64) -> f32 {
        let x = wrap(x) + self.base.offset_x;
        let y = wrap(y) + self.base.offset_y;
        let z = wrap(z) + self.base.offset_z;
        let cell = (floor(x), floor(y), floor(z));
        let relative = (
            (x - f64::from(cell.0)) as f32,
            (y - f64::from(cell.1)) as f32,
            (z - f64::from(cell.2)) as f32,
        );
        self.base.sample_and_lerp(cell, relative, relative.1)
    }

    /// `PerlinNoise.addToVolume` 在一个方块坐标上的贡献。
    fn add_to_point(
        &self,
        block: (i32, i32, i32),
        xz_scale: f64,
        y_scale: f64,
        amplitude: f32,
    ) -> f32 {
        let z = wrap(f64::from(block.2) * xz_scale) + self.base.offset_z;
        let floor_z = floor(z);
        let relative_z = (z - f64::from(floor_z)) as f32;
        let x = wrap(f64::from(block.0) * xz_scale) + self.base.offset_x;
        let floor_x = floor(x);
        let relative_x = (x - f64::from(floor_x)) as f32;
        let y = wrap(f64::from(block.1) * y_scale) + self.base.offset_y;
        let floor_y = floor(y);
        let relative_y = (y - f64::from(floor_y)) as f32;

        let corners = self.base.corners((floor_x, floor_y, floor_z));
        let alpha = (
            smoothstep(relative_x),
            smoothstep(relative_y),
            smoothstep(relative_z),
        );
        amplitude
            * lerp3(
                alpha,
                corners.dots_split((relative_x, relative_z), relative_y),
            )
    }
}

/// `SmearedPerlinNoise`：y 方向按 `fudgeYScale` 分段抹平。
#[derive(Debug, Clone)]
pub struct SmearedPerlinNoise {
    base: GradientNoise,
    fudge_y_scale: f64,
}

impl SmearedPerlinNoise {
    pub fn new(random: &Random, fudge_y_scale: f64) -> Self {
        Self {
            base: GradientNoise::new(random, 256.0),
            fudge_y_scale,
        }
    }

    /// `SmearedPerlinNoise.range(fudgeYScale)`。
    pub fn range_of(fudge_y_scale: f64) -> Interval {
        Interval::of_symmetric((fudge_y_scale.abs() + 2.0) as f32)
    }

    /// `SmearedPerlinNoise.computeFudgeY`。
    fn compute_fudge_y(&self, original_y: f64, relative_y: f64) -> f64 {
        let fudge_limit = if original_y >= 0.0 && original_y < relative_y {
            original_y
        } else {
            relative_y
        };
        f64::from(floor(fudge_limit / self.fudge_y_scale + SHIFT_UP_EPSILON)) * self.fudge_y_scale
    }

    /// `SmearedPerlinNoise.get(x, y, z)`。
    fn get(&self, raw_x: f64, raw_y: f64, raw_z: f64) -> f32 {
        let x = wrap(raw_x) + self.base.offset_x;
        let y = wrap(raw_y) + self.base.offset_y;
        let z = wrap(raw_z) + self.base.offset_z;
        let cell = (floor(x), floor(y), floor(z));
        let relative_x = (x - f64::from(cell.0)) as f32;
        let relative_y = y - f64::from(cell.1);
        let relative_z = (z - f64::from(cell.2)) as f32;
        let fudged_relative_y = (relative_y - self.compute_fudge_y(raw_y, relative_y)) as f32;
        self.base.sample_and_lerp(
            cell,
            (relative_x, fudged_relative_y, relative_z),
            relative_y as f32,
        )
    }

    /// `SmearedPerlinNoise.addToVolume` 在一个方块坐标上的贡献。
    fn add_to_point(
        &self,
        block: (i32, i32, i32),
        xz_scale: f64,
        y_scale: f64,
        amplitude: f32,
    ) -> f32 {
        let z = wrap(f64::from(block.2) * xz_scale) + self.base.offset_z;
        let floor_z = floor(z);
        let relative_z = (z - f64::from(floor_z)) as f32;
        let x = wrap(f64::from(block.0) * xz_scale) + self.base.offset_x;
        let floor_x = floor(x);
        let relative_x = (x - f64::from(floor_x)) as f32;
        let original_y = f64::from(block.1) * y_scale;
        let y = wrap(original_y) + self.base.offset_y;
        let floor_y = floor(y);
        let relative_y = y - f64::from(floor_y);
        let fudged_relative_y = (relative_y - self.compute_fudge_y(original_y, relative_y)) as f32;

        let corners = self.base.corners((floor_x, floor_y, floor_z));
        let alpha = (
            smoothstep(relative_x),
            smoothstep(relative_y as f32),
            smoothstep(relative_z),
        );
        amplitude
            * lerp3(
                alpha,
                corners.dots_split((relative_x, relative_z), fudged_relative_y),
            )
    }
}

/// `SimplexNoise`。
#[derive(Debug, Clone)]
pub struct SimplexNoise {
    base: GradientNoise,
}

/// `SimplexNoise.F2`。官方是 `0.5 * (Math.sqrt(3.0) - 1.0)`，开方按 IEEE 精确舍入，两边相同。
fn simplex_f2() -> f64 {
    0.5 * (3.0_f64.sqrt() - 1.0)
}

/// `SimplexNoise.G2`。
fn simplex_g2() -> f64 {
    (3.0 - 3.0_f64.sqrt()) / 6.0
}

impl SimplexNoise {
    /// `SimplexNoise(random, discardNoiseOffset)`：丢掉偏移时系数是 0，但还是取三个 double。
    pub fn new(random: &Random, discard_noise_offset: bool) -> Self {
        Self {
            base: GradientNoise::new(random, if discard_noise_offset { 0.0 } else { 256.0 }),
        }
    }

    /// `SimplexNoise.getCornerNoise3D`。
    fn corner_noise(&self, index: i32, x: f64, y: f64, z: f64, base: f64) -> f64 {
        let mut t0 = base - x * x - y * y - z * z;
        if t0 < 0.0 {
            0.0
        } else {
            t0 *= t0;
            t0 * t0 * grad_dot_f64(GRADIENT[index as usize], x, y, z)
        }
    }

    /// `SimplexNoise.get(x, y)`。
    pub fn get2(&self, raw_x: f64, raw_y: f64) -> f32 {
        let g2 = simplex_g2();
        let xin = raw_x + self.base.offset_x;
        let yin = raw_y + self.base.offset_y;
        let s = (xin + yin) * simplex_f2();
        let i = floor(xin + s);
        let j = floor(yin + s);
        let t = f64::from(i.wrapping_add(j)) * g2;
        let x0 = xin - (f64::from(i) - t);
        let y0 = yin - (f64::from(j) - t);
        let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
        let x1 = x0 - f64::from(i1) + g2;
        let y1 = y0 - f64::from(j1) + g2;
        let x2 = x0 - 1.0 + 2.0 * g2;
        let y2 = y0 - 1.0 + 2.0 * g2;
        let ii = i & 0xFF;
        let jj = j & 0xFF;
        let permute = |value: i32| self.base.permute(value);
        let gi0 = permute(ii.wrapping_add(permute(jj))) % 12;
        let gi1 = permute(
            ii.wrapping_add(i1)
                .wrapping_add(permute(jj.wrapping_add(j1))),
        ) % 12;
        let gi2 = permute(ii.wrapping_add(1).wrapping_add(permute(jj.wrapping_add(1)))) % 12;
        let n0 = self.corner_noise(gi0, x0, y0, 0.0, 0.5);
        let n1 = self.corner_noise(gi1, x1, y1, 0.0, 0.5);
        let n2 = self.corner_noise(gi2, x2, y2, 0.0, 0.5);
        (70.0 * (n0 + n1 + n2)) as f32
    }

    /// `SimplexNoise.get(x, y, z)`。
    fn get(&self, raw_x: f64, raw_y: f64, raw_z: f64) -> f32 {
        const F3: f64 = 1.0 / 3.0;
        const G3: f64 = 1.0 / 6.0;
        let xin = raw_x + self.base.offset_x;
        let yin = raw_y + self.base.offset_y;
        let zin = raw_z + self.base.offset_z;
        let s = (xin + yin + zin) * F3;
        let i = floor(xin + s);
        let j = floor(yin + s);
        let k = floor(zin + s);
        let t = f64::from(i.wrapping_add(j).wrapping_add(k)) * G3;
        let x0 = xin - (f64::from(i) - t);
        let y0 = yin - (f64::from(j) - t);
        let z0 = zin - (f64::from(k) - t);
        let (first, second) = simplex_corner_order(x0, y0, z0);
        let x1 = x0 - f64::from(first[0]) + G3;
        let y1 = y0 - f64::from(first[1]) + G3;
        let z1 = z0 - f64::from(first[2]) + G3;
        let x2 = x0 - f64::from(second[0]) + F3;
        let y2 = y0 - f64::from(second[1]) + F3;
        let z2 = z0 - f64::from(second[2]) + F3;
        let x3 = x0 - 1.0 + 0.5;
        let y3 = y0 - 1.0 + 0.5;
        let z3 = z0 - 1.0 + 0.5;
        let ii = i & 0xFF;
        let jj = j & 0xFF;
        let kk = k & 0xFF;
        let permute = |value: i32| self.base.permute(value);
        let corner = |offset: [i32; 3]| {
            permute(
                ii.wrapping_add(offset[0]).wrapping_add(permute(
                    jj.wrapping_add(offset[1])
                        .wrapping_add(permute(kk.wrapping_add(offset[2]))),
                )),
            ) % 12
        };
        let gi0 = corner([0, 0, 0]);
        let gi1 = corner(first);
        let gi2 = corner(second);
        let gi3 = corner([1, 1, 1]);
        let n0 = self.corner_noise(gi0, x0, y0, z0, 0.6);
        let n1 = self.corner_noise(gi1, x1, y1, z1, 0.6);
        let n2 = self.corner_noise(gi2, x2, y2, z2, 0.6);
        let n3 = self.corner_noise(gi3, x3, y3, z3, 0.6);
        (32.0 * (n0 + n1 + n2 + n3)) as f32
    }
}

/// `SimplexNoise.get` 里按 `x0`、`y0`、`z0` 的大小关系挑两个角的那一串 if。
fn simplex_corner_order(x0: f64, y0: f64, z0: f64) -> ([i32; 3], [i32; 3]) {
    if x0 >= y0 {
        if y0 >= z0 {
            ([1, 0, 0], [1, 1, 0])
        } else if x0 >= z0 {
            ([1, 0, 0], [1, 0, 1])
        } else {
            ([0, 0, 1], [1, 0, 1])
        }
    } else if y0 < z0 {
        ([0, 0, 1], [0, 1, 1])
    } else if x0 < z0 {
        ([0, 1, 0], [0, 1, 1])
    } else {
        ([0, 1, 0], [1, 1, 0])
    }
}

/// `NoiseStack.Layer`。
#[derive(Debug, Clone)]
struct Layer {
    noise: Noise,
    frequency: f64,
    amplitude: f32,
}

/// `NoiseStack`：一串带频率和振幅的噪声，求值时按顺序累加。
///
/// 官方的 `Perlin` 和 `SmearedPerlin` 两个子类只是把方法复制了一遍，求值完全相同，这里不分。
#[derive(Debug, Clone)]
pub struct NoiseStack {
    layers: Vec<Layer>,
    range: Interval,
}

/// `NoiseStack.Builder`。
#[derive(Debug, Default)]
pub struct NoiseStackBuilder {
    layers: Vec<Layer>,
}

impl NoiseStackBuilder {
    pub fn add(&mut self, noise: Noise, frequency: f64, amplitude: f32) -> &mut Self {
        self.layers.push(Layer {
            noise,
            frequency,
            amplitude,
        });
        self
    }

    /// `addStack(stack, frequency, amplitude)`：把另一叠的层摊平进来。
    pub fn add_stack(&mut self, stack: &NoiseStack, frequency: f64, amplitude: f32) -> &mut Self {
        for layer in &stack.layers {
            self.layers.push(Layer {
                noise: layer.noise.clone(),
                frequency: layer.frequency * frequency,
                amplitude: layer.amplitude * amplitude,
            });
        }
        self
    }

    pub fn build(self) -> NoiseStack {
        NoiseStack::new(self.layers)
    }
}

impl NoiseStack {
    pub fn builder() -> NoiseStackBuilder {
        NoiseStackBuilder::default()
    }

    fn new(layers: Vec<Layer>) -> Self {
        let mut range = Interval::of_exact(0.0);
        for layer in &layers {
            let layer_range =
                Interval::mul_of(layer.noise.range(), Interval::of_exact(layer.amplitude));
            range = Interval::add_of(range, layer_range);
        }
        Self { layers, range }
    }

    pub fn range(&self) -> Interval {
        self.range
    }

    /// `getLayer(index)`，测试用。
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    fn get(&self, x: f64, y: f64, z: f64) -> f32 {
        let mut value = 0.0_f32;
        for layer in &self.layers {
            let frequency = layer.frequency;
            value += layer.amplitude * layer.noise.get(x * frequency, y * frequency, z * frequency);
        }
        value
    }

    fn get2(&self, x: f64, y: f64) -> f32 {
        let mut value = 0.0_f32;
        for layer in &self.layers {
            let frequency = layer.frequency;
            value += layer.amplitude * layer.noise.get2(x * frequency, y * frequency);
        }
        value
    }

    fn add_to_point(
        &self,
        block: (i32, i32, i32),
        xz_scale: f64,
        y_scale: f64,
        amplitude: f32,
    ) -> f32 {
        let mut value = 0.0_f32;
        for layer in &self.layers {
            let frequency = layer.frequency;
            value += layer.noise.add_to_point(
                block,
                xz_scale * frequency,
                y_scale * frequency,
                amplitude * layer.amplitude,
            );
        }
        value
    }
}

/// 官方 `Noise` 接口的全部实现。
#[derive(Debug, Clone)]
pub enum Noise {
    Perlin(PerlinNoise),
    Smeared(SmearedPerlinNoise),
    Simplex(SimplexNoise),
    Stack(NoiseStack),
}

impl Noise {
    /// `Noise.range()`。
    pub fn range(&self) -> Interval {
        match self {
            Self::Perlin(_) | Self::Simplex(_) => Interval::of_symmetric(2.0),
            Self::Smeared(noise) => SmearedPerlinNoise::range_of(noise.fudge_y_scale),
            Self::Stack(stack) => stack.range(),
        }
    }

    /// `Noise.get(x, y, z)`。
    pub fn get(&self, x: f64, y: f64, z: f64) -> f32 {
        match self {
            Self::Perlin(noise) => noise.get(x, y, z),
            Self::Smeared(noise) => noise.get(x, y, z),
            Self::Simplex(noise) => noise.get(x, y, z),
            Self::Stack(stack) => stack.get(x, y, z),
        }
    }

    /// `Noise.get(x, y)`：官方的二维重载，`PerlinNoise` 会再折一次坐标。
    pub fn get2(&self, x: f64, y: f64) -> f32 {
        match self {
            Self::Perlin(noise) => noise.get(wrap(x), 0.0, wrap(y)),
            Self::Smeared(noise) => noise.get(wrap(x), 0.0, wrap(y)),
            Self::Simplex(noise) => noise.get2(x, y),
            Self::Stack(stack) => stack.get2(x, y),
        }
    }

    /// `Noise.addToVolume` 在一个方块坐标上的贡献。
    ///
    /// `SimplexNoise` 没有重写 `addToVolume`，走接口的默认实现，所以直接按 `get` 算，坐标也不折。
    pub fn add_to_point(
        &self,
        block: (i32, i32, i32),
        xz_scale: f64,
        y_scale: f64,
        amplitude: f32,
    ) -> f32 {
        match self {
            Self::Perlin(noise) => noise.add_to_point(block, xz_scale, y_scale, amplitude),
            Self::Smeared(noise) => noise.add_to_point(block, xz_scale, y_scale, amplitude),
            Self::Simplex(noise) => {
                amplitude
                    * noise.get(
                        f64::from(block.0) * xz_scale,
                        f64::from(block.1) * y_scale,
                        f64::from(block.2) * xz_scale,
                    )
            }
            Self::Stack(stack) => stack.add_to_point(block, xz_scale, y_scale, amplitude),
        }
    }
}

/// `NormalNoise.Normalization`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Normalization {
    Disabled,
    Enabled,
    /// 官方标了 `@Deprecated`，`normalize: "legacy"` 时用。
    Legacy,
}

/// `NormalNoise.Parameters`：数据包 `worldgen/noise` 里的内容。
#[derive(Debug, Clone, PartialEq)]
pub struct NormalNoiseParameters {
    pub base_amplitude: f64,
    pub base_octave: i32,
    pub octave_count: i32,
    pub normalize: Normalization,
    pub amplitude_modifiers: Vec<f64>,
}

/// `NormalNoise.OctaveInfo`。
#[derive(Debug, Clone, Copy)]
struct OctaveInfo {
    octave_index: i32,
    frequency: f64,
    amplitude: f64,
}

impl OctaveInfo {
    fn seed(self) -> String {
        format!("octave_{}", self.octave_index)
    }

    fn abs_amplitude(self) -> f64 {
        self.amplitude.abs()
    }
}

/// `NormalNoise`：参数加上算好的归一化系数，`create` 时才真的取随机数造噪声。
#[derive(Debug, Clone)]
pub struct NormalNoise {
    parameters: NormalNoiseParameters,
    octaves: Vec<OctaveInfo>,
    normalization_factor: f64,
    range: Interval,
}

/// `NormalNoise.TARGET_DEVIATION`。
const TARGET_DEVIATION: f64 = 0.333_333_333_333_333_3;
/// `NormalNoise.INPUT_FACTOR`：第二叠噪声的频率要乘这个数。
const INPUT_FACTOR: f64 = 1.018_126_888_217_522_7;

impl NormalNoise {
    /// `new NormalNoise(parameters)`。
    pub fn new(parameters: NormalNoiseParameters) -> Self {
        let octaves = build_octaves(
            parameters.base_octave,
            parameters.base_amplitude,
            parameters.octave_count,
            parameters.normalize != Normalization::Disabled,
            &parameters.amplitude_modifiers,
        );
        let mut target_amplitude: f64 = octaves.iter().map(|octave| octave.abs_amplitude()).sum();
        let mut normalization_factor = compute_normalization_factor(target_amplitude, &octaves);
        if parameters.normalize == Normalization::Legacy && normalization_factor != 0.0 {
            let parity = compute_parity_normalization_factor(
                parameters.base_amplitude,
                parameters.octave_count,
                &parameters.amplitude_modifiers,
            );
            target_amplitude *= parity / normalization_factor;
            normalization_factor = parity;
        }
        let range = Interval::of_symmetric((target_amplitude * TARGET_DEVIATION * 6.0) as f32);
        Self {
            parameters,
            octaves,
            normalization_factor,
            range,
        }
    }

    pub fn range(&self) -> Interval {
        self.range
    }

    pub fn parameters(&self) -> &NormalNoiseParameters {
        &self.parameters
    }

    /// `NormalNoise.create(random)`：两叠柏林噪声，第二叠的频率差一点。
    pub fn create(&self, random: &Random) -> Noise {
        let first_random = random.fork_positional();
        let second_random = random.fork_positional();
        let mut stack = NoiseStack::builder();
        for octave in &self.octaves {
            let seed = octave.seed();
            let first = PerlinNoise::new(&first_random.from_hash_of(&seed));
            let second = PerlinNoise::new(&second_random.from_hash_of(&seed));
            let value_factor = (self.normalization_factor * octave.amplitude) as f32;
            stack.add(Noise::Perlin(first), octave.frequency, value_factor);
            stack.add(
                Noise::Perlin(second),
                octave.frequency * INPUT_FACTOR,
                value_factor,
            );
        }
        Noise::Stack(stack.build())
    }

    /// `NormalNoise.createForLegacyNetherBiome(random)`：下界温度和植被噪声走这条老路。
    pub fn create_for_legacy_nether_biome(&self, random: &Random) -> Noise {
        let modifiers = if self.parameters.amplitude_modifiers.is_empty() {
            vec![1.0; self.parameters.octave_count.max(0) as usize]
        } else {
            self.parameters.amplitude_modifiers.clone()
        };
        let first = create_legacy_fbm(random, self.parameters.base_octave, &modifiers);
        let second = create_legacy_fbm(random, self.parameters.base_octave, &modifiers);
        let value_factor = (self.normalization_factor * self.parameters.base_amplitude) as f32;
        let mut stack = NoiseStack::builder();
        stack.add_stack(&first, 1.0, value_factor);
        stack.add_stack(&second, INPUT_FACTOR, value_factor);
        Noise::Stack(stack.build())
    }
}

/// `NormalNoise.buildOctaves`。
fn build_octaves(
    base_octave: i32,
    base_amplitude: f64,
    octave_count: i32,
    normalize: bool,
    amplitude_modifiers: &[f64],
) -> Vec<OctaveInfo> {
    let mut frequency = 2.0_f64.powf(f64::from(base_octave));
    let mut amplitude = base_amplitude;
    if normalize {
        amplitude *= 0.5_f64.powf(f64::from(-(octave_count - 1)))
            / (0.5_f64.powf(f64::from(-octave_count)) - 1.0);
    }
    let mut octaves = Vec::with_capacity(octave_count.max(0) as usize);
    for index in 0..octave_count {
        let modifier = amplitude_modifier(amplitude_modifiers, index as usize);
        if modifier != 0.0 {
            octaves.push(OctaveInfo {
                octave_index: base_octave + index,
                frequency,
                amplitude: amplitude * modifier,
            });
        }
        frequency *= 2.0;
        amplitude *= 0.5;
    }
    octaves
}

/// `NormalNoise.getAmplitudeModifier`。
fn amplitude_modifier(modifiers: &[f64], index: usize) -> f64 {
    if modifiers.is_empty() {
        1.0
    } else {
        modifiers[index]
    }
}

/// `NormalNoise.computeNormalizationFactor`。
fn compute_normalization_factor(target_amplitude: f64, octaves: &[OctaveInfo]) -> f64 {
    let input_deviation = estimate_deviation(octaves);
    if input_deviation == 0.0 {
        return 0.0;
    }
    let input_sum_deviation = input_deviation * 2.0_f64.sqrt();
    let target_deviation = target_amplitude * TARGET_DEVIATION;
    target_deviation / input_sum_deviation
}

/// `NormalNoise.estimateDeviation`。
fn estimate_deviation(octaves: &[OctaveInfo]) -> f64 {
    let mut variance = 0.0;
    for octave in octaves {
        let layer_deviation = PERLIN_STANDARD_DEVIATION * octave.abs_amplitude();
        variance += layer_deviation * layer_deviation;
    }
    variance.sqrt()
}

/// `NormalNoise.computeParityNormalizationFactor`。
fn compute_parity_normalization_factor(
    base_amplitude: f64,
    octave_count: i32,
    modifiers: &[f64],
) -> f64 {
    let mut min_octave = i32::MAX;
    let mut max_octave = i32::MIN;
    for index in 0..octave_count {
        if amplitude_modifier(modifiers, index as usize) != 0.0 {
            min_octave = min_octave.min(index);
            max_octave = max_octave.max(index);
        }
    }
    base_amplitude * 0.5 * TARGET_DEVIATION
        / parity_expected_deviation(max_octave.wrapping_sub(min_octave))
}

/// `NormalNoise.parityExpectedDeviation`。
fn parity_expected_deviation(octave_span: i32) -> f64 {
    0.1 * (1.0 + 1.0 / f64::from(octave_span.wrapping_add(1)))
}

/// `LegacyFbmInitializer.createForLegacyNetherBiome`。
fn create_legacy_fbm(random: &Random, first_octave: i32, amplitudes: &[f64]) -> NoiseStack {
    let octaves = amplitudes.len() as i32;
    let zero_octave_index = -first_octave;
    let mut noise_levels: Vec<Option<PerlinNoise>> = vec![None; octaves.max(0) as usize];
    // 官方先造出 0 号八度，用不上也已经取过数了。
    let zero_octave = PerlinNoise::new(random);
    if zero_octave_index >= 0
        && zero_octave_index < octaves
        && amplitudes[zero_octave_index as usize] != 0.0
    {
        noise_levels[zero_octave_index as usize] = Some(zero_octave);
    }
    for index in (0..zero_octave_index).rev() {
        if index < octaves && amplitudes[index as usize] != 0.0 {
            noise_levels[index as usize] = Some(PerlinNoise::new(random));
        } else {
            // `skipOctave`：官方取掉同样多的数，再丢掉。
            random.consume_count(262);
        }
    }

    assert!(
        zero_octave_index >= octaves - 1,
        "positive octaves are temporarily disabled"
    );

    let mut factor = 2.0_f64.powf(f64::from(-zero_octave_index));
    let mut value_factor =
        2.0_f64.powf(f64::from(octaves - 1)) / (2.0_f64.powf(f64::from(octaves)) - 1.0);
    let mut stack = NoiseStack::builder();
    for (index, noise) in noise_levels.into_iter().enumerate() {
        if let Some(noise) = noise {
            stack.add(
                Noise::Perlin(noise),
                factor,
                (value_factor * amplitudes[index]) as f32,
            );
        }
        factor *= 2.0;
        value_factor /= 2.0;
    }
    stack.build()
}

/// `BlendedNoise`：`old_blended_noise` 密度函数背后的三叠抹平柏林噪声。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendedNoiseParameters {
    pub xz_scale: f64,
    pub y_scale: f64,
    pub xz_factor: f64,
    pub y_factor: f64,
    pub smear_scale_multiplier: f64,
}

/// `BlendedNoise.BASE_SCALE`。
const BLENDED_BASE_SCALE: f64 = 684.412;
/// `BlendedNoise.LIMIT_FACTOR`，官方是 float 字面量。
const BLENDED_LIMIT_FACTOR: f64 = 0.99998474_f32 as f64;
/// `BlendedNoise.MAIN_FACTOR`。
const BLENDED_MAIN_FACTOR: f64 = 12.75;
/// `BlendedNoise.NOISE_SEED`。
pub const BLENDED_NOISE_SEED: &str = "minecraft:terrain";

/// `BlendedNoise.FbmSet`。
#[derive(Debug, Clone)]
pub struct BlendedFbmSet {
    pub min_limit: NoiseStack,
    pub max_limit: NoiseStack,
    pub main: NoiseStack,
}

impl BlendedNoiseParameters {
    fn xz_multiplier(self) -> f64 {
        BLENDED_BASE_SCALE * self.xz_scale
    }

    fn y_multiplier(self) -> f64 {
        BLENDED_BASE_SCALE * self.y_scale
    }

    /// `createFbmSet(random)`：三叠噪声按这个顺序取数。
    pub fn create_fbm_set(self, random: &Random) -> BlendedFbmSet {
        let limit_smear_scale_y = self.y_multiplier() * self.smear_scale_multiplier;
        let main_smear_scale_y = limit_smear_scale_y / self.y_factor;
        BlendedFbmSet {
            min_limit: create_fbm(random, -15, limit_smear_scale_y, BLENDED_LIMIT_FACTOR),
            max_limit: create_fbm(random, -15, limit_smear_scale_y, BLENDED_LIMIT_FACTOR),
            main: create_fbm(random, -7, main_smear_scale_y, BLENDED_MAIN_FACTOR),
        }
    }

    /// 噪声坐标的三个系数：两叠 limit 用前两个，main 用后面除过的那两个。
    pub fn multipliers(self) -> (f64, f64, f64, f64) {
        let xz = self.xz_multiplier();
        let y = self.y_multiplier();
        (xz, y, xz / self.xz_factor, y / self.y_factor)
    }

    /// `BlendedNoise.range()`：官方单独算一遍，不看真的造出来的那几叠。
    pub fn range(self) -> Interval {
        compute_fbm_range(
            -15,
            self.y_multiplier() * self.smear_scale_multiplier,
            BLENDED_LIMIT_FACTOR,
        )
    }
}

/// `BlendedNoise.createFbm`。
fn create_fbm(
    random: &Random,
    first_octave: i32,
    smear_scale_y: f64,
    value_factor: f64,
) -> NoiseStack {
    assert!(first_octave <= 0, "firstOctave>0");
    let octaves = -first_octave + 1;
    let mut factor = 1.0_f64;
    let mut value_factor = value_factor / (2.0_f64.powf(f64::from(octaves)) - 1.0);
    let mut stack = NoiseStack::builder();
    for _ in (0..octaves).rev() {
        stack.add(
            Noise::Smeared(SmearedPerlinNoise::new(random, smear_scale_y * factor)),
            factor,
            value_factor as f32,
        );
        factor /= 2.0;
        value_factor *= 2.0;
    }
    stack.build()
}

/// `BlendedNoise.computeFbmRange`。
fn compute_fbm_range(first_octave: i32, smear_scale_y: f64, value_factor: f64) -> Interval {
    let octaves = -first_octave + 1;
    let mut factor = 1.0_f64;
    let mut value_factor = value_factor / (2.0_f64.powf(f64::from(octaves)) - 1.0);
    let mut range = Interval::of_exact(0.0);
    for _ in (0..octaves).rev() {
        let layer_range = Interval::mul_of(
            SmearedPerlinNoise::range_of(smear_scale_y * factor),
            Interval::of_exact(value_factor as f32),
        );
        range = Interval::add_of(range, layer_range);
        factor /= 2.0;
        value_factor *= 2.0;
    }
    range
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::XoroshiroRandom;

    fn xoroshiro(seed: i64) -> Random {
        Random::Xoroshiro(XoroshiroRandom::new(seed))
    }

    /// 置换表是 0..256 的一个排列，和官方洗牌的方式一致。
    #[test]
    fn permutation_table_is_a_permutation() {
        let noise = GradientNoise::new(&xoroshiro(1), 256.0);
        let mut sorted = noise.perms;
        sorted.sort_unstable();
        for (index, value) in sorted.iter().enumerate() {
            assert_eq!(usize::from(*value), index);
        }
        assert!(noise.offset_x >= 0.0 && noise.offset_x < 256.0);
    }

    /// 丢掉偏移时三个偏移都是 0，但随机数还是取了。
    #[test]
    fn simplex_can_discard_the_offset() {
        let random = xoroshiro(5);
        let noise = SimplexNoise::new(&random, true);
        assert_eq!(noise.base.offset_x, 0.0);
        assert_eq!(noise.base.offset_y, 0.0);
        assert_eq!(noise.base.offset_z, 0.0);
        // 和不丢偏移的那一个取同样多的数。
        let twin = xoroshiro(5);
        let _ = SimplexNoise::new(&twin, false);
        assert_eq!(random.next_long(), twin.next_long());
    }

    /// `wrap` 只在坐标很大时才折回来。
    #[test]
    fn wrap_only_folds_huge_coordinates() {
        assert_eq!(wrap(0.0), 0.0);
        assert_eq!(wrap(1234.5), 1234.5);
        assert_eq!(wrap(-1234.5), -1234.5);
        assert_eq!(
            wrap(1.0e9),
            1.0e9 - (1.0e9 / ROUND_OFF + 0.5).floor() * ROUND_OFF
        );
        assert!(wrap(1.0e9).abs() <= ROUND_OFF);
    }

    /// 柏林噪声在整数格点上是 0：八个角的点积都带着为零的相对坐标。
    #[test]
    fn perlin_is_zero_on_lattice_points() {
        let noise = PerlinNoise::new(&xoroshiro(3));
        let offset = (
            -noise.base.offset_x,
            -noise.base.offset_y,
            -noise.base.offset_z,
        );
        assert_eq!(noise.get(offset.0, offset.1, offset.2), 0.0);
    }

    /// 两条路径算的是同一个数学函数，只有最后几位可能不同。
    #[test]
    fn both_paths_agree_up_to_rounding() {
        let noise = Noise::Perlin(PerlinNoise::new(&xoroshiro(11)));
        for block in [(0, 0, 0), (13, -7, 29), (-100, 60, 400)] {
            let point = noise.get(
                f64::from(block.0) * 0.5,
                f64::from(block.1) * 0.25,
                f64::from(block.2) * 0.5,
            );
            let volume = noise.add_to_point(block, 0.5, 0.25, 1.0);
            assert!(
                (point - volume).abs() < 1.0e-5,
                "{point} vs {volume} at {block:?}"
            );
        }
    }

    /// 官方 `NormalNoise` 的归一化：振幅全是 1 时系数只看八度数。
    #[test]
    fn normal_noise_builds_one_stack_per_octave() {
        let parameters = NormalNoiseParameters {
            base_amplitude: 1.0,
            base_octave: -3,
            octave_count: 2,
            normalize: Normalization::Enabled,
            amplitude_modifiers: Vec::new(),
        };
        let noise = NormalNoise::new(parameters);
        assert_eq!(noise.octaves.len(), 2);
        assert_eq!(noise.octaves[0].octave_index, -3);
        assert_eq!(noise.octaves[1].octave_index, -2);
        assert_eq!(noise.octaves[0].frequency, 0.125);
        let Noise::Stack(stack) = noise.create(&xoroshiro(2)) else {
            panic!("create 给的是一叠噪声");
        };
        // 每个八度两层：第二层的频率差一个 INPUT_FACTOR。
        assert_eq!(stack.layer_count(), 4);
    }

    /// 振幅是 0 的八度不造噪声，也不占层。
    #[test]
    fn zero_amplitudes_are_skipped() {
        let parameters = NormalNoiseParameters {
            base_amplitude: 1.0,
            base_octave: -5,
            octave_count: 3,
            normalize: Normalization::Enabled,
            amplitude_modifiers: vec![1.0, 0.0, 1.0],
        };
        let noise = NormalNoise::new(parameters);
        assert_eq!(noise.octaves.len(), 2);
        assert_eq!(noise.octaves[1].octave_index, -3);
    }

    /// `createFbm` 的层数是 `-firstOctave + 1`，频率从 1 往下减半。
    #[test]
    fn blended_noise_builds_sixteen_octaves() {
        let parameters = BlendedNoiseParameters {
            xz_scale: 0.25,
            y_scale: 0.125,
            xz_factor: 80.0,
            y_factor: 160.0,
            smear_scale_multiplier: 8.0,
        };
        let set = parameters.create_fbm_set(&xoroshiro(4));
        assert_eq!(set.min_limit.layer_count(), 16);
        assert_eq!(set.max_limit.layer_count(), 16);
        assert_eq!(set.main.layer_count(), 8);
        assert!(!parameters.range().is_nai());
    }

    /// 下界的老 fbm：两个八度，前面还要跳掉六个。
    #[test]
    fn legacy_nether_fbm_skips_the_unused_octaves() {
        let random = xoroshiro(6);
        let stack = create_legacy_fbm(&random, -7, &[1.0, 1.0]);
        assert_eq!(stack.layer_count(), 2);
        assert_eq!(stack.layers[0].frequency, 2.0_f64.powi(-7));
        assert_eq!(stack.layers[1].frequency, 2.0_f64.powi(-6));
    }
}
