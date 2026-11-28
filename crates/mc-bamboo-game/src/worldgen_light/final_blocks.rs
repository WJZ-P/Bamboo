//! 共享生成期光照使用的局部 FEATURES 后方块层。
//!
//! Share the local post-FEATURES block layers used by generation light.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, OnceLock, Weak},
};

use crate::{
    world_options::WorldGenConfig,
    worldgen::GeneratedChunk,
    worldgen_features::{BlockDelta, FeatureCacheKey, feature_cache_key, fold_light_blocks},
};

pub(super) type Layers = Arc<Vec<Vec<u32>>>;
type Key = (FeatureCacheKey, usize, u16);
type Slot = Arc<OnceLock<Layers>>;

struct Entry {
    slot: Slot,
    // 条目在表内时不能重用这个地址，但不必保留地形本体。
    // Keep the address from being reused while cached without retaining the terrain.
    _terrain: Weak<GeneratedChunk>,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<Key, Entry>,
    order: VecDeque<Key>,
}

// 标准 384×256 方块层，128 份约 49 MiB；先插入的条目先淘汰。
// At 384×256 blocks, 128 sets of layers are about 49 MiB, evicted in FIFO order.
const CAPACITY: usize = 128;
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

pub(crate) fn clear() {
    if let Some(cache) = CACHE.get()
        && let Ok(mut cache) = cache.lock()
    {
        let old = std::mem::take(&mut *cache);
        drop(cache);
        drop(old);
    }
}

pub(super) fn get(
    config: &WorldGenConfig,
    x: i32,
    z: i32,
    terrain: &Arc<GeneratedChunk>,
    sources: [&[BlockDelta]; 9],
) -> Layers {
    crate::worldgen_cache::touch();
    // 同一格的不同视图若仅相差空来源，可共享同一份方块层。
    // Views of the same cell share layers when they differ only by empty sources.
    let mask = sources.iter().enumerate().fold(0, |mask, (slot, writes)| {
        mask | (u16::from(!writes.is_empty()) << slot)
    });
    let key = (
        feature_cache_key(config, x, z),
        Arc::as_ptr(terrain) as usize,
        mask,
    );
    let compute = || Arc::new(fold_light_blocks(&terrain.blocks, sources));
    let Ok(mut cache) = CACHE.get_or_init(Mutex::default).lock() else {
        return compute();
    };
    let (slot, evicted) = cache.slot(&key, terrain);
    drop(cache);
    drop(evicted);
    Arc::clone(slot.get_or_init(compute))
}

impl Cache {
    fn slot(&mut self, key: &Key, terrain: &Arc<GeneratedChunk>) -> (Slot, Option<Entry>) {
        if let Some(entry) = self.entries.get(key) {
            return (Arc::clone(&entry.slot), None);
        }
        let slot = Arc::new(OnceLock::new());
        let evicted = if self.entries.len() == CAPACITY {
            self.order
                .pop_front()
                .and_then(|first| self.entries.remove(&first))
        } else {
            None
        };
        self.order.push_back(key.clone());
        self.entries.insert(
            key.clone(),
            Entry {
                slot: Arc::clone(&slot),
                _terrain: Arc::downgrade(terrain),
            },
        );
        (slot, evicted)
    }
}
