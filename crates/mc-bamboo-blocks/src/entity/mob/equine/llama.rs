//! 羊驼自己的随机化：力量（`setRandomStrength`）、毛色，以及繁殖时的继承。
//!
//! Llama-only randomization: strength, colour and their inheritance.

use super::{EquineState, JavaRandom, LegacyRandom};

/// `Llama.finalizeSpawn`：先力量（4% 概率上限是 5）再毛色。
///
/// `Llama.finalizeSpawn`: strength first (a 4% chance of a cap of 5), then colour.
pub(super) fn finalize_spawn(state: &mut EquineState, random: &LegacyRandom) {
    let max = if random.next_float() < 0.04 { 5 } else { 3 };
    state.strength = 1 + random.next_int(max);
    state.variant = random.next_int(4);
}

/// `Llama.getBreedOffspring`：力量在父母较大值内抽，3% 再加一，毛色随机取一方。
///
/// Strength drawn under the stronger parent plus a 3% bonus; colour from either parent.
pub(super) fn inherit(
    child: &mut EquineState,
    a: &EquineState,
    b: &EquineState,
    random: &mut JavaRandom,
) {
    let base = 1 + random.next_int_bounded(a.strength.max(b.strength).max(1));
    let bonus = i32::from(random.next_float() < 0.03);
    child.strength = (base + bonus).min(5);
    child.variant = if random.next_boolean() {
        a.variant
    } else {
        b.variant
    };
}
