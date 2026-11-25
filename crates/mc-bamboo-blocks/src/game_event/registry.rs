//! `EuclideanGameEventListenerRegistry`：每个区段一份监听者列表，按岛存放。
//!
//! 官方每个区块的每个区段一个 `ArrayList`，按登记先后遍历；这里用
//! `(区块, sectionY)` 做键，拆岛 / 合岛按区块搬行。

use std::collections::{BTreeSet, HashMap};

use mc_bamboo_core::{BlockPos, ChunkPos};

use crate::entity::EntityUid;

/// 监听者是谁：方块实体按位置，实体按 uid。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListenerRef {
    /// 方块实体（幽匿感测体等）。
    BlockEntity(BlockPos),
    /// 实体（监守者、悦灵等）。
    Entity(EntityUid),
}

/// 按岛的监听者登记表。
#[derive(Clone, Debug, Default)]
pub struct GameEventListeners {
    sections: HashMap<(ChunkPos, i32), Vec<ListenerRef>>,
}

impl GameEventListeners {
    /// `register`：追加到这一区段列表末尾，已登记的不重复加。
    pub fn register(&mut self, chunk: ChunkPos, section_y: i32, listener: ListenerRef) {
        let list = self.sections.entry((chunk, section_y)).or_default();
        if !list.contains(&listener) {
            list.push(listener);
        }
    }

    /// `unregister`：列表空了就删掉这一区段（官方的 `onEmptyAction`）。
    pub fn unregister(&mut self, chunk: ChunkPos, section_y: i32, listener: ListenerRef) {
        let key = (chunk, section_y);
        if let Some(list) = self.sections.get_mut(&key) {
            list.retain(|l| *l != listener);
            if list.is_empty() {
                self.sections.remove(&key);
            }
        }
    }

    /// 这一区段的监听者，按登记先后。返回副本：投递期间登记表可能变。
    pub fn section(&self, chunk: ChunkPos, section_y: i32) -> Vec<ListenerRef> {
        self.sections
            .get(&(chunk, section_y))
            .cloned()
            .unwrap_or_default()
    }

    /// 这一区段当前是否还登记着 `listener`（投递中被注销的要跳过）。
    pub fn contains(&self, chunk: ChunkPos, section_y: i32, listener: ListenerRef) -> bool {
        self.sections
            .get(&(chunk, section_y))
            .is_some_and(|l| l.contains(&listener))
    }

    /// 区块卸载：丢掉这一列的全部区段。
    pub fn remove_chunk(&mut self, chunk: ChunkPos) {
        self.sections.retain(|(c, _), _| *c != chunk);
    }

    /// 拆岛：把这些区块的行搬到新表。
    pub fn split_off(&mut self, chunks: &BTreeSet<ChunkPos>) -> Self {
        let mut moved = Self::default();
        self.sections.retain(|key, list| {
            if chunks.contains(&key.0) {
                moved.sections.insert(*key, std::mem::take(list));
                false
            } else {
                true
            }
        });
        moved
    }

    /// 合岛：把另一张表的行并进来。两边区块不相交。
    pub fn absorb(&mut self, other: Self) {
        self.sections.extend(other.sections);
    }

    /// 不变量：没有空列表，每行的区块都在 `chunks` 里。
    pub fn check_invariants(&self, chunks: &BTreeSet<ChunkPos>) -> Result<(), String> {
        for ((chunk, y), list) in &self.sections {
            if list.is_empty() {
                return Err(format!("empty listener section {chunk:?} y={y}"));
            }
            if !chunks.contains(chunk) {
                return Err(format!("listener section {chunk:?} y={y} outside island"));
            }
        }
        Ok(())
    }

    /// 登记表是否为空。
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_order_and_split() {
        let c = ChunkPos::new(0, 0);
        let mut t = GameEventListeners::default();
        let a = ListenerRef::BlockEntity(BlockPos::new(1, 2, 3));
        let b = ListenerRef::BlockEntity(BlockPos::new(4, 2, 3));
        t.register(c, 0, a);
        t.register(c, 0, b);
        t.register(c, 0, a);
        assert_eq!(t.section(c, 0), vec![a, b]);
        let moved = t.split_off(&[c].into_iter().collect());
        assert!(t.is_empty());
        t.absorb(moved);
        assert!(t.check_invariants(&[c].into_iter().collect()).is_ok());
        t.unregister(c, 0, a);
        t.unregister(c, 0, b);
        assert!(t.is_empty());
    }
}
