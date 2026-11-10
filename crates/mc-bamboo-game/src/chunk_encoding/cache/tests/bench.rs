//! 量一量发区块的开销：N 个改过的区块发给 M 名玩家，改动前每次现编，现在走缓存。
//!
//! 每次发送都和 `send_chunks` 一样把区块包写进这名玩家的出站缓冲。
//!
//! ```text
//! cargo test --release -p mc-bamboo-game send_cost -- --ignored --nocapture
//! ```

use std::{
    hint::black_box,
    mem::size_of,
    sync::Arc,
    time::{Duration, Instant},
};

use mc_bamboo_protocol::{frame::append_frame, packet::play::LevelChunkWithLight};

use super::*;
use crate::{chunk_tracking::ChunkView, outgoing::SharedFrames};

/// 每项量这么多次，取中位数。
const RUNS: usize = 15;
/// 地面上面第一格。
const GROUND: i32 = -60;
/// 改过的区块：以原点为中心 9×9 个，每个里面盖一间小屋。
const AREA_RADIUS: i32 = 4;
const PLAYER_COUNTS: [usize; 4] = [1, 2, 10, 50];

#[test]
#[ignore = "基准测试，用 release 手动跑"]
fn send_cost() {
    // 模板要在盖房子之前编：`encode_template` 编的是 (0, 0)，盖完以后那里也有房子。
    let template_body = encode_template(&ChunkStore::flat());
    let start = Instant::now();
    let (store, chunks) = built_up_area();
    println!(
        "built {} edited chunks in {:.0} ms",
        chunks.len(),
        start.elapsed().as_secs_f64() * 1000.0
    );
    report_bodies(&store, &template_body, &chunks);
    for players in PLAYER_COUNTS {
        report_sends(&store, &template_body, &chunks, players);
    }
    report_memory(&store, &template_body, &chunks);
    report_sweep(&store);
}

/// 每个区块里一间小屋：木板墙、玻璃窗、石头屋顶、火把、箱子和写了字的告示牌，
/// 方块、高度图、方块实体和两种光照都改到了。
fn build_house(store: &mut ChunkStore, chunk: ChunkPos) {
    let corner = BlockPos::new(chunk.x * 16 + 3, GROUND, chunk.z * 16 + 3);
    for dx in 0..10 {
        for dz in 0..10 {
            let wall = dx == 0 || dx == 9 || dz == 0 || dz == 9;
            for dy in 0..4 {
                let window = dy == 2 && (dx == 4 || dz == 4);
                let state = match (wall, window) {
                    (false, _) => continue,
                    (true, true) => blocks::GLASS,
                    (true, false) => blocks::OAK_PLANKS,
                };
                store.set_block(corner.offset(dx, dy, dz), state);
            }
            store.set_block(corner.offset(dx, 4, dz), blocks::STONE);
        }
    }
    store.set_block(corner.offset(5, 0, 5), blocks::TORCH);
    store.set_block(corner.offset(1, 0, 1), blocks::CHEST);
    let sign = corner.offset(4, 1, -1);
    store.set_block(sign, blocks::OAK_SIGN);
    let mut content = SignContent::default();
    content.front.messages[0] = format!("house {} {}", chunk.x, chunk.z);
    store.set_sign(sign, content);
}

fn built_up_area() -> (ChunkStore, Vec<ChunkPos>) {
    let mut store = ChunkStore::flat();
    let range = -AREA_RADIUS..=AREA_RADIUS;
    let chunks: Vec<ChunkPos> = range
        .clone()
        .flat_map(|x| range.clone().map(move |z| ChunkPos::new(x, z)))
        .collect();
    for &chunk in &chunks {
        build_house(&mut store, chunk);
    }
    store.finish_light_updates();
    (store, chunks)
}

/// 改动前的 `chunk_body`：没改过的区块共用模板，改过的每次重新编码。
fn uncached_body<'a>(store: &ChunkStore, template_body: &'a [u8], pos: ChunkPos) -> Cow<'a, [u8]> {
    if store.is_template(pos) {
        Cow::Borrowed(template_body)
    } else {
        Cow::Owned(encode_chunk(store, pos))
    }
}

fn write_chunk(buffer: &mut Vec<u8>, pos: ChunkPos, body: &[u8]) {
    let packet = LevelChunkWithLight {
        x: pos.x,
        z: pos.z,
        body,
    };
    append_frame(buffer, &packet).expect("区块包放得进一帧");
}

/// 每名玩家一个出站缓冲。量之前清空但留着容量：缓冲变长和缺页两边一样多，会盖过要比的差别。
fn clear(buffers: &mut [Vec<u8>]) {
    for buffer in buffers {
        buffer.clear();
    }
}

/// 把每个区块发给每名玩家，不用缓存。
fn send_uncached(
    buffers: &mut [Vec<u8>],
    store: &ChunkStore,
    template_body: &[u8],
    chunks: &[ChunkPos],
) -> Duration {
    clear(buffers);
    let start = Instant::now();
    for buffer in buffers.iter_mut() {
        for &pos in chunks {
            write_chunk(buffer, pos, &uncached_body(store, template_body, pos));
        }
    }
    let elapsed = start.elapsed();
    black_box(buffers);
    elapsed
}

/// 把每个区块发给每名玩家，用 `cache`。传进来的缓存是空的，就是第一名玩家现编、后面的玩家直接用。
fn send_cached(
    buffers: &mut [Vec<u8>],
    cache: &mut ChunkCache,
    store: &ChunkStore,
    template_body: &[u8],
    chunks: &[ChunkPos],
) -> Duration {
    clear(buffers);
    let start = Instant::now();
    for buffer in buffers.iter_mut() {
        for &pos in chunks {
            write_chunk(buffer, pos, &chunk_body(cache, store, template_body, pos));
        }
    }
    let elapsed = start.elapsed();
    black_box(buffers);
    elapsed
}

/// 只算拿到区块内容的时间：改动前每次现编，现在查到缓存就直接用。
fn report_bodies(store: &ChunkStore, template_body: &[u8], chunks: &[ChunkPos]) {
    let encode = median(|| {
        let start = Instant::now();
        for &pos in chunks {
            black_box(encode_chunk(store, pos));
        }
        start.elapsed()
    });
    let mut cache = ChunkCache::default();
    for &pos in chunks {
        cache.body(store, pos);
    }
    let hit = median(|| {
        let start = Instant::now();
        for &pos in chunks {
            black_box(chunk_body(&mut cache, store, template_body, pos));
        }
        start.elapsed()
    });
    let per_chunk = |duration: Duration| duration.as_secs_f64() * 1e6 / chunks.len() as f64;
    println!(
        "body only: encode {:.2} us/chunk, cache hit {:.3} us/chunk",
        per_chunk(encode),
        per_chunk(hit)
    );
}

fn report_sends(store: &ChunkStore, template_body: &[u8], chunks: &[ChunkPos], players: usize) {
    let mut buffers = vec![Vec::new(); players];
    let before = median(|| send_uncached(&mut buffers, store, template_body, chunks));
    let cold = median(|| {
        let mut cache = ChunkCache::default();
        send_cached(&mut buffers, &mut cache, store, template_body, chunks)
    });
    let mut warm_cache = ChunkCache::default();
    for &pos in chunks {
        warm_cache.body(store, pos);
    }
    let warm = median(|| send_cached(&mut buffers, &mut warm_cache, store, template_body, chunks));
    let sends = (players * chunks.len()) as f64;
    let line = |duration: Duration| {
        let millis = duration.as_secs_f64() * 1000.0;
        format!("{millis:>8.3} ms {:>6.2} us/send", millis * 1000.0 / sends)
    };
    println!(
        "{} chunks to {players:>2} players  before {}   cold cache {}   warm cache {}   x{:.1} / x{:.1}",
        chunks.len(),
        line(before),
        line(cold),
        line(warm),
        before.as_secs_f64() / cold.as_secs_f64(),
        before.as_secs_f64() / warm.as_secs_f64(),
    );
}

/// 每个缓存条目占多少内存：编码出来的字节，加上表里的键、版本号和指针。
fn report_memory(store: &ChunkStore, template_body: &[u8], chunks: &[ChunkPos]) {
    let mut cache = ChunkCache::default();
    for &pos in chunks {
        cache.body(store, pos);
    }
    let mut one_block = ChunkStore::flat();
    one_block.set_block(BlockPos::new(1, GROUND, 1), blocks::STONE);
    println!(
        "memory: {} bytes of bodies for {} chunks, {} bytes per house chunk \
         ({} for a chunk with one block changed, template {}), plus {} bytes per table slot",
        cache.bytes,
        cache.bodies.len(),
        cache.bytes / cache.bodies.len(),
        encode_chunk(&one_block, ChunkPos::new(0, 0)).len(),
        template_body.len(),
        size_of::<(ChunkPos, CachedBody)>(),
    );
}

/// 每 tick 丢掉没人跟踪的条目要多久。最坏的情况：每个条目都要看到最后一名玩家才找到。
fn report_sweep(store: &ChunkStore) {
    let revision = store.revision(ChunkPos::new(0, 0));
    let body: Box<[u8]> = vec![0; 8 * 1024].into_boxed_slice();
    let mut cache = ChunkCache::default();
    for x in 0..40 {
        for z in 0..25 {
            let entry = CachedFrame {
                revision,
                frames: Arc::new(SharedFrames::new(body.to_vec())),
            };
            cache.frames.insert(ChunkPos::new(x, z), entry);
        }
    }
    let far = ChunkView {
        center: ChunkPos::new(10_000, 10_000),
        view_distance: 32,
    };
    let mut views = vec![far; 49];
    views.push(ChunkView {
        center: ChunkPos::new(20, 12),
        view_distance: 32,
    });
    let sweep = median(|| {
        let start = Instant::now();
        cache.retain(|pos| views.iter().any(|view| view.contains(pos)));
        start.elapsed()
    });
    assert_eq!(cache.frames.len(), 1000);
    println!(
        "sweep: {:.3} ms for {} entries against {} views",
        sweep.as_secs_f64() * 1000.0,
        cache.frames.len(),
        views.len()
    );
}

fn median(mut measure: impl FnMut() -> Duration) -> Duration {
    let mut times: Vec<Duration> = (0..RUNS).map(|_| measure()).collect();
    times.sort_unstable();
    times[RUNS / 2]
}
