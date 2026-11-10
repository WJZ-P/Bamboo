//! 改过的区块的包体缓存，以及所有区块的完整帧缓存。
//!
//! 一个区块包的内容只取决于这个区块和它那一列光照，[`ChunkStore::revision`] 在它们每次被写时都会换新值。
//! 包体缓存记下编码时的版本号，版本号没变就直接用上次的字节，和重新编码一个字节都不差；变了就当场重新编码。
//! 帧缓存按同样的版本号判断新鲜：区块变了就重做帧，多名玩家拿到的还是同一份帧。
//!
//! 内存有两道限制：
//!
//! - 每 tick 发完区块后，丢掉谁的跟踪范围（包括还没发的 `pending`）里都没有的区块，
//!   所以条目数不超过所有玩家跟踪范围的并集里改过的区块数；
//! - 包体条目加起来不超过 [`MAX_BYTES`]，满了就不再收新的，多出来的区块照旧每次现编。
//!   帧缓存的条目由同一个清理动作按坐标丢掉，所以条目数也跟着跟踪范围走。

use std::{borrow::Cow, collections::HashMap, sync::Arc};

use mc_bamboo_core::ChunkPos;
use mc_bamboo_world::store::{ChunkRevision, ChunkStore};

use super::encode_chunk;
use crate::outgoing::SharedFrames;

/// 包体缓存最多占这么多字节。超平坦世界里改过的区块编出来 7 到 10 KB，够放七千到九千个，
/// 比一名视距 32 的玩家跟踪的 3725 个区块多一倍。
const MAX_BYTES: usize = 64 << 20;

/// 区块的编码结果，世界线程独占，不用锁。
#[derive(Debug)]
pub(crate) struct ChunkCache {
    bodies: HashMap<ChunkPos, CachedBody>,
    /// 按坐标缓存的完整帧，包头里有坐标，所以每个位置的帧都不同。
    frames: HashMap<ChunkPos, CachedFrame>,
    /// 所有包体条目的字节数。
    bytes: usize,
    max_bytes: usize,
}

#[derive(Debug)]
struct CachedBody {
    /// 编码时区块的版本号。
    revision: ChunkRevision,
    body: Box<[u8]>,
}

#[derive(Debug)]
struct CachedFrame {
    revision: ChunkRevision,
    frames: Arc<SharedFrames>,
}

impl Default for ChunkCache {
    fn default() -> Self {
        Self::with_limit(MAX_BYTES)
    }
}

impl ChunkCache {
    fn with_limit(max_bytes: usize) -> Self {
        Self {
            bodies: HashMap::new(),
            frames: HashMap::new(),
            bytes: 0,
            max_bytes,
        }
    }

    /// 这个区块此刻的包体。版本号没变时用缓存，不然重新编码；放得下的话存起来，替换旧的。
    pub(crate) fn body(&mut self, store: &ChunkStore, pos: ChunkPos) -> Cow<'_, [u8]> {
        let revision = store.revision(pos);
        if !self.is_fresh(pos, revision) {
            self.remove(pos);
            let body = encode_chunk(store, pos);
            if self.bytes + body.len() > self.max_bytes {
                return Cow::Owned(body);
            }
            self.insert(pos, revision, body);
        }
        Cow::Borrowed(&self.bodies[&pos].body)
    }

    /// 缓存里的帧条目数，测试用。
    #[cfg(test)]
    pub(crate) fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// 有没有缓存着空帧，测试用。
    #[cfg(test)]
    pub(crate) fn has_empty_frame(&self) -> bool {
        self.frames
            .values()
            .any(|entry| entry.frames.raw().is_empty())
    }

    /// 这个区块此刻的完整帧。版本号没变时直接借出缓存里的那份。
    pub(crate) fn frame(&self, store: &ChunkStore, pos: ChunkPos) -> Option<Arc<SharedFrames>> {
        let revision = store.revision(pos);
        self.frames
            .get(&pos)
            .and_then(|entry| (entry.revision == revision).then(|| Arc::clone(&entry.frames)))
    }

    /// 放进刚做好的帧，替换这个坐标的旧帧。
    pub(crate) fn insert_frame(
        &mut self,
        store: &ChunkStore,
        pos: ChunkPos,
        raw: Vec<u8>,
    ) -> Arc<SharedFrames> {
        let frames = Arc::new(SharedFrames::new(raw));
        self.frames.insert(
            pos,
            CachedFrame {
                revision: store.revision(pos),
                frames: Arc::clone(&frames),
            },
        );
        frames
    }

    fn is_fresh(&self, pos: ChunkPos, revision: ChunkRevision) -> bool {
        self.bodies
            .get(&pos)
            .is_some_and(|entry| entry.revision == revision)
    }

    fn insert(&mut self, pos: ChunkPos, revision: ChunkRevision, body: Vec<u8>) {
        let body = body.into_boxed_slice();
        self.bytes += body.len();
        self.bodies.insert(pos, CachedBody { revision, body });
    }

    fn remove(&mut self, pos: ChunkPos) {
        if let Some(entry) = self.bodies.remove(&pos) {
            self.bytes -= entry.body.len();
        }
    }

    /// 只留下 `keep` 选中的区块，包体和帧一起清理。
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(ChunkPos) -> bool) {
        let bytes = &mut self.bytes;
        self.bodies.retain(|&pos, entry| {
            let kept = keep(pos);
            if !kept {
                *bytes -= entry.body.len();
            }
            kept
        });
        self.frames.retain(|&pos, _| keep(pos));
    }
}

#[cfg(test)]
mod tests;
