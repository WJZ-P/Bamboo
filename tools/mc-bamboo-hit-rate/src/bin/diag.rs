//! 场景诊断：检查时钟输入到底有没有让机器动起来。

use mc_bamboo_blocks::{Level, state::POWERED};
use mc_bamboo_data::blocks;
use mc_bamboo_hit_rate::{machines, world::Input};

fn main() {
    let mut m = machines::repeater_clock();
    println!("input={:?}", m.input);
    let Input::Clock { pos, .. } = m.input.clone() else { panic!() };
    for now in 1..=16 {
        m.step(now, false);
        if now % 4 == 0 || now == 9 {
            let mut row = Vec::new();
            for x in -1..=8 {
                let p = mc_bamboo_core::BlockPos::new(x, 64, 0);
                let s = m.level.block_state(p);
                let tag = if s.is(blocks::REDSTONE_BLOCK) {
                    "R"
                } else if s.is(blocks::REDSTONE_WIRE) {
                    if s.get(mc_bamboo_blocks::state::POWER) > 0 { "W" } else { "w" }
                } else if s.is(blocks::REPEATER) {
                    if s.get(POWERED) { "P" } else { "p" }
                } else if s.is(blocks::REDSTONE_LAMP) {
                    "L"
                } else if s.is(blocks::AIR) {
                    "."
                } else {
                    "?"
                };
                row.push(tag);
            }
            let pending: Vec<_> = m.ticks.pending().collect();
            println!("tick {now}: source={:?} row={} pending={pending:?}", m.level.block_state(pos).0, row.concat());
        }
    }
}
