//! 共享生成期光照使用的完整 FEATURES 后方块层。
//!
//! Share the complete post-FEATURES block layers used by generation light.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, OnceLock, Weak},
};

use crate::{
    world_options::WorldGenConfig,
    worldgen::GeneratedChunk,
    worldgen_features::{FeatureCacheKey, feature_block_layers, feature_cache_key},
};

pub(super) type Layers = Arc<Vec<Vec<u32>>>;
type Key = (FeatureCacheKey, usize);
type Slot = Arc<OnceLock<Result<Layers, String>>>;

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
) -> Result<Layers, String> {
    crate::worldgen_cache::touch();
    let key = (
        feature_cache_key(config, x, z),
        Arc::as_ptr(terrain) as usize,
    );
    let compute = || feature_block_layers(config, x, z, &terrain.blocks).map(Arc::new);
    let Ok(mut cache) = CACHE.get_or_init(Mutex::default).lock() else {
        return compute();
    };
    let (slot, evicted) = cache.slot(&key, terrain);
    drop(cache);
    drop(evicted);
    let result = slot.get_or_init(compute);
    if result.is_err()
        && let Some(cache) = CACHE.get()
        && let Ok(mut cache) = cache.lock()
        && cache
            .entries
            .get(&key)
            .is_some_and(|entry| Arc::ptr_eq(&entry.slot, &slot))
    {
        cache.entries.remove(&key);
        cache.order.retain(|other| other != &key);
    }
    result.clone()
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
