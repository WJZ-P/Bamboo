//! 缓存给出的字节必须和当场重新编码的一模一样。

use mc_bamboo_core::{
    BlockPos,
    sign::{DyeColor, SignContent},
};
use mc_bamboo_data::{block_of_state, blocks};

use super::*;
use crate::{
    chunk_encoding::{chunk_body, encode_template},
    world::tests::Harness,
};

mod bench;

/// xorshift64*，和 `world/tests/fuzz.rs` 里的一样，结果完全由种子决定。
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, limit: u64) -> u64 {
        (self.next() >> 11) % limit
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<T: Copy>(&mut self, values: &[T]) -> T {
        values[self.below(values.len() as u64) as usize]
    }

    /// `start..end` 里的一个数。
    fn range(&mut self, start: i32, end: i32) -> i32 {
        start + self.below((end - start) as u64) as i32
    }
}

/// 随机放的方块：挡光的、透光的、发光的、流体、树叶，还有箱子和各种告示牌。
const STATES: [u32; 15] = [
    blocks::AIR,
    blocks::AIR,
    blocks::STONE,
    blocks::GLASS,
    blocks::GLOWSTONE,
    blocks::TORCH,
    blocks::WATER,
    blocks::LAVA,
    blocks::OAK_LEAVES,
    blocks::CHEST,
    blocks::OAK_SIGN,
    // 同一个告示牌转了个方向：方块实体和字都留着。
    blocks::OAK_SIGN + 4,
    blocks::OAK_WALL_SIGN,
    blocks::OAK_HANGING_SIGN,
    blocks::SEA_LANTERN,
];
const COLORS: [DyeColor; 3] = [DyeColor::Black, DyeColor::Red, DyeColor::LightBlue];
/// 中间 3×3 个区块里改方块，比较时再往外一圈：光照和光照段都会漏到隔壁。
const EDIT_RADIUS: i32 = 1;
const CHECK_RADIUS: i32 = 2;
const STEPS: usize = 400;
/// 每一步之后随机比较几个区块。
const CHECKS_PER_STEP: usize = 4;

/// 一次随机运行里缓存起了多少作用。
#[derive(Debug, Default)]
struct Counts {
    /// 直接用了缓存。
    hits: usize,
    /// 有缓存，但区块已经变了，重新编码。
    refreshed: usize,
}

fn random_pos(rng: &mut Rng) -> BlockPos {
    let low = -16 * EDIT_RADIUS;
    let high = 16 * (EDIT_RADIUS + 1);
    let x = rng.range(low, high);
    let z = rng.range(low, high);
    // 多数在地面附近。半空中的方块让空的区块段有了方块，周围几列的光照段跟着建起来，拆掉后又被丢掉。
    let y = match rng.below(10) {
        0..=5 => rng.range(-64, -48),
        6..=8 => rng.range(-48, 16),
        _ => rng.range(100, 320),
    };
    BlockPos::new(x, y, z)
}

fn random_content(rng: &mut Rng) -> SignContent {
    let mut content = SignContent::default();
    for message in &mut content.front.messages {
        if rng.chance(50) {
            *message = format!("line {}", rng.below(1000));
        }
    }
    content.back.messages[0] = format!("back {}", rng.below(10));
    content.front.color = rng.pick(&COLORS);
    content.back.glowing = rng.chance(50);
    content.waxed = rng.chance(20);
    content
}

fn is_sign(state: u32) -> bool {
    block_of_state(state).is_some_and(|block| block.name.ends_with("_sign"))
}

fn random_chunk(rng: &mut Rng) -> ChunkPos {
    let x = rng.range(-CHECK_RADIUS, CHECK_RADIUS + 1);
    let z = rng.range(-CHECK_RADIUS, CHECK_RADIUS + 1);
    ChunkPos::new(x, z)
}

/// 一次随机运行：在同一个世界里随机改、随机取，每次取到的都和重新编码的比较。
struct RandomRun {
    seed: u64,
    rng: Rng,
    store: ChunkStore,
    template_body: Vec<u8>,
    cache: ChunkCache,
    /// 放过告示牌的位置，后来可能被别的方块占了。
    signs: Vec<BlockPos>,
    counts: Counts,
}

impl RandomRun {
    fn new(seed: u64) -> Self {
        let store = ChunkStore::flat();
        Self {
            seed,
            rng: Rng::new(seed),
            template_body: encode_template(&store),
            store,
            cache: ChunkCache::default(),
            signs: Vec::new(),
            counts: Counts::default(),
        }
    }

    fn run(mut self) -> Counts {
        for _ in 0..STEPS {
            self.change();
            // 偶尔丢掉一些条目，像玩家走开那样。
            if self.rng.chance(5) {
                let rng = &mut self.rng;
                self.cache.retain(|_| rng.chance(50));
            }
            for _ in 0..CHECKS_PER_STEP {
                let pos = random_chunk(&mut self.rng);
                self.check(pos);
            }
        }
        self.store.finish_light_updates();
        for x in -CHECK_RADIUS..=CHECK_RADIUS {
            for z in -CHECK_RADIUS..=CHECK_RADIUS {
                self.check(ChunkPos::new(x, z));
            }
        }
        self.counts
    }

    /// 随机改一下世界：放方块、改告示牌，或者算一点攒着的光照，有时一轮只算到一半。
    fn change(&mut self) {
        let rng = &mut self.rng;
        match rng.below(10) {
            0..=6 => {
                let pos = random_pos(rng);
                let state = rng.pick(&STATES);
                if self.store.set_block(pos, state) && is_sign(state) {
                    self.signs.push(pos);
                }
            }
            7 if !self.signs.is_empty() => {
                let pos = self.signs[rng.below(self.signs.len() as u64) as usize];
                self.store.set_sign(pos, random_content(rng));
            }
            _ => {
                let budget = 1 + rng.below(4000) as usize;
                self.store.run_light_updates(budget);
            }
        }
    }

    /// 从缓存取一次，和重新编码的结果比较。
    fn check(&mut self, pos: ChunkPos) {
        if self.cache.is_fresh(pos, self.store.revision(pos)) {
            self.counts.hits += 1;
        } else if self.cache.bodies.contains_key(&pos) {
            self.counts.refreshed += 1;
        }
        let body = chunk_body(&mut self.cache, &self.store, &self.template_body, pos);
        let fresh = encode_chunk(&self.store, pos);
        assert!(*body == *fresh, "seed {}, chunk {pos:?}", self.seed);
    }
}

#[test]
fn cached_bodies_match_fresh_encodings_across_random_edits() {
    for seed in 1..=3 {
        let counts = RandomRun::new(seed).run();
        // 两种情况都要真的发生过，不然这个测试什么也没证明。
        assert!(counts.hits > 200, "seed {seed}: {counts:?}");
        assert!(counts.refreshed > 100, "seed {seed}: {counts:?}");
    }
}

#[test]
fn unchanged_chunks_are_served_from_the_cache() {
    let mut store = ChunkStore::flat();
    let mut cache = ChunkCache::default();
    let pos = BlockPos::new(1, -60, 1);
    let chunk = pos.chunk();
    store.set_block(pos, blocks::STONE);
    cache.body(&store, chunk);

    // 改掉缓存里的一个字节：取到改过的字节，说明没有重新编码。
    cache.bodies.get_mut(&chunk).expect("刚存进去").body[0] ^= 0xff;
    assert_ne!(*cache.body(&store, chunk), *encode_chunk(&store, chunk));

    store.set_block(pos, blocks::GLASS);
    assert_eq!(*cache.body(&store, chunk), *encode_chunk(&store, chunk));
}

#[test]
fn sign_text_reaches_the_cached_chunk() {
    let mut store = ChunkStore::flat();
    let mut cache = ChunkCache::default();
    let pos = BlockPos::new(3, -60, 3);
    store.set_block(pos, blocks::OAK_SIGN);
    let blank = cache.body(&store, pos.chunk()).into_owned();

    let mut content = SignContent::default();
    content.front.messages[0] = "hello".to_owned();
    store.set_sign(pos, content);

    let written = cache.body(&store, pos.chunk()).into_owned();
    assert_ne!(written, blank);
    assert_eq!(written, encode_chunk(&store, pos.chunk()));
}

/// 半空中的一块石头让周围 3×3 列都存上光照段，拆掉以后这些段在一轮光照结束时被丢掉。
/// 隔壁那列的光照值一格都没变，`light_update` 不会提它，区块包里却少了这几段。
#[test]
fn light_sections_dropped_next_door_refresh_the_neighbor() {
    let mut store = ChunkStore::flat();
    let template_body = encode_template(&store);
    let mut cache = ChunkCache::default();
    let stone = BlockPos::new(8, 200, 8);
    let neighbor = ChunkPos::new(1, 0);

    store.set_block(stone, blocks::STONE);
    store.finish_light_updates();
    let with_sections = chunk_body(&mut cache, &store, &template_body, neighbor).into_owned();

    store.set_block(stone, blocks::AIR);
    store.finish_light_updates();

    let fresh = encode_chunk(&store, neighbor);
    assert_ne!(fresh, with_sections);
    assert_eq!(
        *chunk_body(&mut cache, &store, &template_body, neighbor),
        *fresh
    );
}

#[test]
fn a_full_cache_still_returns_the_current_bytes() {
    let mut store = ChunkStore::flat();
    let first = BlockPos::new(1, -60, 1);
    let second = BlockPos::new(17, -60, 1);
    store.set_block(first, blocks::STONE);
    store.set_block(second, blocks::STONE);
    let size = encode_chunk(&store, first.chunk()).len();
    // 只放得下一个区块。
    let mut cache = ChunkCache::with_limit(size);

    cache.body(&store, first.chunk());
    let overflow = cache.body(&store, second.chunk());
    assert!(matches!(overflow, Cow::Owned(_)));
    assert_eq!(*overflow, *encode_chunk(&store, second.chunk()));
    assert_eq!((cache.bodies.len(), cache.bytes), (1, size));

    // 变大以后放不下了，旧的条目也不能留着。
    store.set_block(first, blocks::OAK_SIGN);
    store.set_sign(first, random_content(&mut Rng::new(7)));
    let grown = cache.body(&store, first.chunk());
    assert_eq!(*grown, *encode_chunk(&store, first.chunk()));
    assert_eq!((cache.bodies.len(), cache.bytes), (0, 0));
}

#[test]
fn retain_frees_the_bytes_of_dropped_entries() {
    let mut store = ChunkStore::flat();
    let kept = BlockPos::new(1, -60, 1);
    let dropped = BlockPos::new(17, -60, 1);
    store.set_block(kept, blocks::STONE);
    store.set_block(dropped, blocks::CHEST);
    let mut cache = ChunkCache::default();
    cache.body(&store, kept.chunk());
    cache.body(&store, dropped.chunk());

    cache.retain(|pos| pos == kept.chunk());

    assert_eq!(cache.bodies.len(), 1);
    assert_eq!(cache.bytes, encode_chunk(&store, kept.chunk()).len());
}

#[test]
fn the_cache_forgets_chunks_nobody_tracks() {
    let mut server = Harness::new(2);
    let edited = BlockPos::new(1, -60, 1);
    server.ensure_loaded(&[edited.chunk()]);
    server.region().chunks.set_block(edited, blocks::STONE);
    let client = server.join_and_settle("Kitten");
    assert!(
        server
            .region()
            .chunk_cache
            .bodies
            .contains_key(&edited.chunk())
    );
    assert!(
        server
            .region()
            .chunk_cache
            .frames
            .contains_key(&edited.chunk())
    );

    server.move_to(&client, 1000.0, 1000.0);
    // 新视图里的区块也要真的加载：已加载模式下没交付的位置编不出包体，
    // 缓存就到不了「没人跟踪的都清掉」这一步。
    let center = ChunkPos::containing(1000.0, 1000.0);
    let view: Vec<ChunkPos> = (center.x - 3..=center.x + 3)
        .flat_map(|x| (center.z - 3..=center.z + 3).map(move |z| ChunkPos::new(x, z)))
        .collect();
    server.ensure_loaded(&view);
    server.tick();

    // 无限模式里新视图共用模板帧，缓存全空；已加载模式里新视图每个区块
    // 各编一份包体，所以条数正好是这片视图的区块数。
    let expected_bodies = if server.world_region().chunks.is_loaded_mode() {
        view.len()
    } else {
        0
    };
    assert_eq!(server.region().chunk_cache.bodies.len(), expected_bodies);
    // 旧视图的坐标一个都不留。
    assert!(
        !server
            .region()
            .chunk_cache
            .bodies
            .contains_key(&edited.chunk())
    );
    assert!(
        server
            .region()
            .chunk_cache
            .bodies
            .keys()
            .all(|pos| view.contains(pos)),
        "留下的都是当前视图里的包体"
    );
    assert!(
        !server
            .region()
            .chunk_cache
            .frames
            .contains_key(&edited.chunk())
    );
}
