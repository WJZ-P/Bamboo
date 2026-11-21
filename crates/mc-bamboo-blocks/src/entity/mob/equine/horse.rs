//! 马自己的随机化：花色（`Variant`）和斑纹（`Markings`），速度与跳跃的生成公式。
//!
//! Horse-only randomization: coat and markings, and the speed and jump formulas.

use super::{EquineState, JavaRandom, LegacyRandom};

const COATS: i32 = 7;
const MARKINGS: i32 = 5;

/// `Horse.finalizeSpawn`：`Util.getRandom` 抽毛色和斑纹。
///
/// `Horse.finalizeSpawn`: draw the coat and markings.
pub(super) fn finalize_spawn(state: &mut EquineState, random: &LegacyRandom) {
    let coat = random.next_int(COATS);
    let markings = random.next_int(MARKINGS);
    state.variant = coat | (markings << 8);
}

/// `AbstractHorse.generateSpeed`。
pub(super) fn speed(mut next: impl FnMut() -> f64) -> f64 {
    (f64::from(0.45_f32) + next() * 0.3 + next() * 0.3 + next() * 0.3) * 0.25
}

/// `AbstractHorse.generateJumpStrength`。
pub(super) fn jump(mut next: impl FnMut() -> f64) -> f64 {
    f64::from(0.4_f32) + next() * 0.2 + next() * 0.2 + next() * 0.2
}

/// `Horse.getBreedOffspring` 的花色：`nextInt(9)` 选毛色来源，`nextInt(5)` 选斑纹来源。
///
/// The foal's variant: `nextInt(9)` picks the coat source, `nextInt(5)` the markings source.
pub(super) fn offspring_variant(a: i32, b: i32, random: &mut JavaRandom) -> i32 {
    let coat = match random.next_int_bounded(9) {
        0..4 => (a & 255) % COATS,
        4..8 => (b & 255) % COATS,
        _ => random.next_int_bounded(COATS),
    };
    let markings = match random.next_int_bounded(5) {
        0..2 => ((a >> 8) & 255) % MARKINGS,
        2..4 => ((b >> 8) & 255) % MARKINGS,
        _ => random.next_int_bounded(MARKINGS),
    };
    coat | (markings << 8)
}
