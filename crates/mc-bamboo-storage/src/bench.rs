//! 区块 NBT 编解码的耗时和字节数，供区块流水线估预算。
//!
//! 解码拆成三段分别计时：解压、NBT 解析成 `Tag`、`from_parts` 构造。
//! 超平坦区块和一个「内容丰富」的合成区块各量一次。
//!
//! ```text
//! cargo test --release -p mc-bamboo-storage chunk_nbt_cost -- --ignored --nocapture
//! ```

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use mc_bamboo_core::{BlockPos, ChunkPos, sign::SignContent};
use mc_bamboo_data::blocks;
use mc_bamboo_nbt::{Accounter, Compression, decompress, write_compressed};
use mc_bamboo_world::{column::ChunkColumn, store::ChunkStore};

use crate::chunk_nbt::{ChunkEncodeExtra, decode, decode_bytes, encode_bytes};

/// 每项量这么多次，取中位数。
const RUNS: usize = 15;

#[test]
#[ignore = "基准测试，用 release 手动跑"]
fn chunk_nbt_cost() {
    for (label, column) in [("flat", flat_column()), ("rich", rich_column(2026))] {
        let options = ChunkEncodeExtra {
            chunk_pos: ChunkPos::new(3, -9),
            last_update: 1234,
            inhabited_time: 99,
            ..Default::default()
        };
        let plain = encode_bytes(&column, &options).expect("编码成功");
        let packed =
            write_compressed("", &decode_root(&plain), Compression::Zlib).expect("压缩成功");
        println!(
            "{label}: plain {} bytes, zlib {} bytes ({:.1}%)",
            plain.len(),
            packed.len(),
            packed.len() as f64 / plain.len() as f64 * 100.0
        );

        let encode = median(RUNS, || {
            black_box(encode_bytes(&column, &options).expect("编码成功"));
        });
        let decode_time = median(RUNS, || {
            black_box(decode_bytes(&plain).expect("解码成功"));
        });
        let inflate = median(RUNS, || {
            black_box(decompress(&packed, Compression::Zlib).expect("解压成功"));
        });
        let parse = median(RUNS, || {
            black_box(decode_root(&plain));
        });
        // 解码的后半段（`decode` 里除 NBT 解析以外的部分，主要是 `from_parts`）：
        // 复用已经解析好的 Tag，避免把解析算两遍。
        let root = decode_root(&plain);
        let from_parts = median(RUNS, || {
            black_box(decode(&root).expect("解码成功"));
        });
        println!(
            "{label}: encode {encode:.3?}, decode {decode_time:.3?} \
             (inflate {inflate:.3?}, NBT parse {parse:.3?}, assemble {from_parts:.3?})"
        );
    }
}

/// 读出一个带名字的根标签，只计 NBT 解析这一段。
fn decode_root(bytes: &[u8]) -> mc_bamboo_nbt::Tag {
    let (_, entries) = mc_bamboo_nbt::read_named(bytes, Accounter::unlimited()).expect("解析成功");
    mc_bamboo_nbt::Tag::Compound(entries)
}

fn median(runs: usize, mut work: impl FnMut()) -> Duration {
    let mut samples: Vec<Duration> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            work();
            start.elapsed()
        })
        .collect();
    samples.sort_unstable();
    samples[runs / 2]
}

fn flat_column() -> Box<ChunkColumn> {
    let mut store = ChunkStore::flat();
    store.finish_light_updates();
    store.take_column(ChunkPos::new(0, 0)).expect("光照算完了")
}

fn rich_column(seed: u64) -> Box<ChunkColumn> {
    let mut store = ChunkStore::flat();
    let mut state = seed | 1;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for index in 0..3000 {
        let x = (next() % 16) as i32;
        let z = (next() % 16) as i32;
        let y = -64 + (next() % 100) as i32;
        let place = [
            blocks::STONE,
            blocks::DIRT,
            blocks::GLOWSTONE,
            blocks::OAK_SIGN,
            blocks::GLASS,
            blocks::OAK_PLANKS,
        ][index % 6];
        store.set_block(BlockPos::new(x, y, z), place);
    }
    store.set_block(BlockPos::new(7, -60, 9), blocks::OAK_SIGN);
    let mut content = SignContent::BLANK;
    content.front.messages[0] = "竹 bamboo".to_owned();
    store.set_sign(BlockPos::new(7, -60, 9), content);
    store.finish_light_updates();
    store.take_column(ChunkPos::new(0, 0)).expect("光照算完了")
}
