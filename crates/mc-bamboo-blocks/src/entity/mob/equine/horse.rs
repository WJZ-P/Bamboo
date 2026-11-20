use super::{EquineState, JavaRandom, LegacyRandom};

const COATS: i32 = 7;
const MARKINGS: i32 = 5;

pub(super) fn finalize_spawn(state: &mut EquineState, random: &LegacyRandom) {
    let coat = random.next_int(COATS);
    let markings = random.next_int(MARKINGS);
    state.variant = coat | (markings << 8);
}

pub(super) fn speed(mut next: impl FnMut() -> f64) -> f64 {
    (f64::from(0.45_f32) + next() * 0.3 + next() * 0.3 + next() * 0.3) * 0.25
}

pub(super) fn jump(mut next: impl FnMut() -> f64) -> f64 {
    f64::from(0.4_f32) + next() * 0.2 + next() * 0.2 + next() * 0.2
}

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
