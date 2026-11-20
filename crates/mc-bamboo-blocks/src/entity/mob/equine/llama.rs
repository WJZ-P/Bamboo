use super::{EquineState, JavaRandom, LegacyRandom};

pub(super) fn finalize_spawn(state: &mut EquineState, random: &LegacyRandom) {
    let max = if random.next_float() < 0.04 { 5 } else { 3 };
    state.strength = 1 + random.next_int(max);
    state.variant = random.next_int(4);
}

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
