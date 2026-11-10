//! 诊断：活塞门乐观口径下键同效果不同的原因。

use mc_bamboo_blocks::recording::Record;
use mc_bamboo_hit_rate::{machines, world::Machine};

fn main() {
    let (records, _) = Machine::compare(machines::piston_door, 1000);
    let mut seen: std::collections::HashMap<u128, &Record> = Default::default();
    let mut causes: std::collections::BTreeMap<String, usize> = Default::default();
    for r in &records {
        if !r.cacheable(false) {
            continue;
        }
        let key = r.exact_key(false);
        if let Some(previous) = seen.get(&key) {
            if previous.effect_key() != r.effect_key() {
                let cause = if previous.writes.len() != r.writes.len() {
                    format!(
                        "write count {} vs {}",
                        previous.writes.len(),
                        r.writes.len()
                    )
                } else {
                    let index = previous
                        .writes
                        .iter()
                        .zip(&r.writes)
                        .position(|(a, b)| a != b)
                        .unwrap_or(usize::MAX);
                    format!(
                        "write[{index}] {:?} vs {:?}",
                        previous.writes.get(index),
                        r.writes.get(index)
                    )
                };
                *causes.entry(cause).or_default() += 1;
            }
        } else {
            seen.insert(key, r);
        }
    }
    for (cause, count) in causes.iter().take(10) {
        println!("{count}: {cause}");
    }
}
