//! `EuclideanGameEventListenerRegistry`：每个区段一份监听者列表，按岛存放。
//!
//! 官方每个区块的每个区段一个 `ArrayList`，按登记先后遍历；这里用
//! `(区块, sectionY)` 做键，拆岛 / 合岛按区块搬行。

use std::collections::{BTreeMap, BTreeSet, HashMap};

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
    /// 实体监听者当前登记在哪个区段，对应官方 `DynamicGameEventListener.lastSection`。
    /// 用 `BTreeMap` 让每刻的同步按 uid 升序，精确与并行驱动逐位一致。
    ///
    /// Which section each entity listener is registered in, the counterpart of vanilla
    /// `DynamicGameEventListener.lastSection`. A `BTreeMap` keeps the per-tick sync in
    /// ascending uid order so precise and parallel drives agree bit for bit.
    entity_sections: BTreeMap<EntityUid, (ChunkPos, i32)>,
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

    /// `DynamicGameEventListener.move`：区段没变就不动，变了就注销旧的、登记新的。
    ///
    /// `DynamicGameEventListener.move`: a no-op when the section is unchanged;
    /// otherwise unregister the old one and register the new one.
    pub fn sync_entity(&mut self, uid: EntityUid, chunk: ChunkPos, section_y: i32) {
        let key = (chunk, section_y);
        if self.entity_sections.get(&uid).copied() == Some(key) {
            return;
        }
        let listener = ListenerRef::Entity(uid);
        if let Some((old_chunk, old_y)) = self.entity_sections.insert(uid, key) {
            self.unregister(old_chunk, old_y, listener);
        }
        self.register(chunk, section_y, listener);
    }

    /// `DynamicGameEventListener.remove`：按记录的区段注销。
    ///
    /// `DynamicGameEventListener.remove`: unregister from the recorded section.
    pub fn remove_entity(&mut self, uid: EntityUid) {
        if let Some((chunk, y)) = self.entity_sections.remove(&uid) {
            self.unregister(chunk, y, ListenerRef::Entity(uid));
        }
    }

    /// 当前跟踪的实体监听者 uid，按升序。
    ///
    /// The tracked entity-listener uids, in ascending order.
    pub fn entity_listener_uids(&self) -> Vec<EntityUid> {
        self.entity_sections.keys().copied().collect()
    }

    /// 某实体监听者当前登记的区段。
    ///
    /// The section an entity listener is currently registered in.
    pub fn entity_section(&self, uid: EntityUid) -> Option<(ChunkPos, i32)> {
        self.entity_sections.get(&uid).copied()
    }

    /// 区块卸载：丢掉这一列的全部区段。
    pub fn remove_chunk(&mut self, chunk: ChunkPos) {
        self.sections.retain(|(c, _), _| *c != chunk);
        self.entity_sections.retain(|_, (c, _)| *c != chunk);
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
        self.entity_sections.retain(|uid, key| {
            if chunks.contains(&key.0) {
                moved.entity_sections.insert(*uid, *key);
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
        self.entity_sections.extend(other.entity_sections);
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
        for (uid, (chunk, y)) in &self.entity_sections {
            if !chunks.contains(chunk) {
                return Err(format!("entity listener {uid} outside island"));
            }
            if !self
                .sections
                .get(&(*chunk, *y))
                .is_some_and(|list| list.contains(&ListenerRef::Entity(*uid)))
            {
                return Err(format!("entity listener {uid} missing from its section"));
            }
        }
        Ok(())
    }

    /// 登记表是否为空。
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty() && self.entity_sections.is_empty()
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

    #[test]
    fn entity_listener_moves_across_sections_and_survives_island_changes() {
        let a = ChunkPos::new(0, 0);
        let b = ChunkPos::new(1, 0);
        let uid = 7;
        let mut t = GameEventListeners::default();
        t.sync_entity(uid, a, 4);
        assert_eq!(t.entity_section(uid), Some((a, 4)));
        assert_eq!(t.section(a, 4), vec![ListenerRef::Entity(uid)]);
        // 同区段是幂等的 `move`。
        t.sync_entity(uid, a, 4);
        assert_eq!(t.section(a, 4), vec![ListenerRef::Entity(uid)]);
        // 跨区段：旧区段注销、新区段登记。
        t.sync_entity(uid, a, 5);
        assert!(t.section(a, 4).is_empty());
        assert_eq!(t.section(a, 5), vec![ListenerRef::Entity(uid)]);
        assert_eq!(t.entity_section(uid), Some((a, 5)));
        // 拆岛把实体记录按区块一起搬走。
        let moved = t.split_off(&[a].into_iter().collect());
        assert!(t.is_empty());
        assert_eq!(moved.entity_section(uid), Some((a, 5)));
        assert!(moved.check_invariants(&[a].into_iter().collect()).is_ok());
        // 合岛并进另一张表的实体记录，uid 仍升序。
        let mut other = GameEventListeners::default();
        other.sync_entity(9, b, 2);
        let mut joined = GameEventListeners::default();
        joined.absorb(moved);
        joined.absorb(other);
        assert_eq!(joined.entity_listener_uids(), vec![7, 9]);
        assert!(
            joined
                .check_invariants(&[a, b].into_iter().collect())
                .is_ok()
        );
        // 移除与区块卸载都清掉记录。
        joined.remove_entity(7);
        assert_eq!(joined.entity_section(7), None);
        assert!(joined.section(a, 5).is_empty());
        joined.remove_chunk(b);
        assert_eq!(joined.entity_section(9), None);
        assert!(joined.is_empty());
    }
}
