//! 战利品表的随机序列，逐位移植官方的 `RandomSequence` 和 `RandomSequences`。
//!
//! 一张表如果写了 `random_sequence`，官方就不用 `Level.random`，而是按
//! （世界种子，序列名）从 `XoroshiroRandom` 里取数。同一个序列名在同一局游戏里
//! 共用一条随机流，取数的次数会带到下一次，所以这里也要按名字缓存实例。
//!
//! 序列名会先经 MD5 搅成 128 位，再和种子异或。这是官方 `seedFromHashOf`
//! 的做法，必须一模一样，否则同一种子下第一个 `nextLong` 就对不上。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use md5::{Digest, Md5};
use mc_bamboo_blocks::random::{upgrade_seed_to_128bit_unmixed, Seed128, XoroshiroRandom};

/// `RandomSupport.seedFromHashOf`：序列名 UTF-8 的 MD5，前 8 字节是低位，
/// 后 8 字节是高位，都按大端读成 long。
pub fn seed_from_hash_of(key: &str) -> Seed128 {
    let digest = Md5::digest(key.as_bytes());
    let read_long = |start: usize| {
        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(&digest[start..start + 8]);
        i64::from_be_bytes(bytes)
    };
    Seed128 {
        seed_lo: read_long(0),
        seed_hi: read_long(8),
    }
}

/// 一条按名字取数的随机序列。`create` 对应官方的静态构造方法。
pub struct RandomSequence {
    source: XoroshiroRandom,
}

impl RandomSequence {
    /// `RandomSequence.create(seed, sequence)`：先升级到 128 位，有序列名就异或
    /// 名字的哈希，最后 `mixed()` 再交给 Xoroshiro。
    pub fn create(seed: i64, sequence: Option<&str>) -> Self {
        let mut seed128 = upgrade_seed_to_128bit_unmixed(seed);
        if let Some(key) = sequence {
            seed128 = seed128.xor(seed_from_hash_of(key));
        }
        Self {
            source: XoroshiroRandom::from_seed128(seed128.mixed()),
        }
    }

    pub fn source(&self) -> &XoroshiroRandom {
        &self.source
    }
}

/// 一局游戏的序列缓存，对应官方的 `RandomSequences`。
///
/// 官方每个序列名只在第一次用时创建，之后同一个表的所有调用共用同一条流。
/// 缓存里放 `Rc`，调用方拿到的引用活得比 `borrow` 长，但状态仍然只有一份。
pub struct RandomSequences {
    world_seed: i64,
    cache: RefCell<HashMap<String, Rc<XoroshiroRandom>>>,
}

impl RandomSequences {
    pub fn new(world_seed: i64) -> Self {
        Self {
            world_seed,
            cache: RefCell::new(HashMap::new()),
        }
    }

    pub fn world_seed(&self) -> i64 {
        self.world_seed
    }

    /// `RandomSequences.get(key)`：没有就按默认参数（含世界种子和序列名）创建。
    pub fn get(&self, key: &str) -> Rc<XoroshiroRandom> {
        if let Some(source) = self.cache.borrow().get(key) {
            return Rc::clone(source);
        }
        let source = Rc::new(RandomSequence::create(self.world_seed, Some(key)).source);
        self.cache
            .borrow_mut()
            .insert(key.to_owned(), Rc::clone(&source));
        source
    }

    /// 已经创建过的序列数量，测试用。
    pub fn len(&self) -> usize {
        self.cache.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.borrow().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_blocks::random::RandomSource;

    /// 期望值来自官方 `RandomSequence.create(424242L, "minecraft:blocks/stone")`。
    #[test]
    fn sequence_matches_official() {
        let sequence = RandomSequence::create(424_242, Some("minecraft:blocks/stone"));
        assert_eq!(sequence.source().next_long(), -7_528_366_805_404_169_439);
    }

    #[test]
    fn name_hash_matches_official() {
        let hash = seed_from_hash_of("minecraft:blocks/stone");
        // `77c5c643ddbe0f28ee760458118d8b9d`，按大端读成两个 long。
        assert_eq!(hash.seed_lo, 8_630_522_255_710_752_552);
        assert_eq!(hash.seed_hi, -1_263_817_869_132_723_299);
    }

    #[test]
    fn cache_shares_one_stream_per_name() {
        // 用一条独立创建的序列当对照：同一个名字取两次，第二次必须接着第一次的状态走。
        let control = RandomSequence::create(42, Some("minecraft:blocks/stone"));
        let first_value = control.source().next_long();
        let second_value = control.source().next_long();

        let sequences = RandomSequences::new(42);
        let first = sequences.get("minecraft:blocks/stone");
        assert_eq!(first.next_long(), first_value);
        let second = sequences.get("minecraft:blocks/stone");
        assert_eq!(second.next_long(), second_value);
        assert_eq!(sequences.len(), 1);
        sequences.get("minecraft:blocks/dirt");
        assert_eq!(sequences.len(), 2);
    }
}
