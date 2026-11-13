//! 睡着的方块实体关注的格子和实体盒子。
//!
//! 闲置漏斗睡到「它读到的东西变了」：自己、朝向那一格、上方一格的方块和容器内容，
//! 加上吸取盒里的物品实体。睡下时在这里登记，外部写入报告位置，这里查出关注者叫醒。
//! 没有登记时两个入口都是一次 `is_empty` 分支。
//!
//! Cells and entity boxes watched by sleeping block entities. An idle hopper sleeps until
//! something it reads changes. Writers report positions; the watchers are woken.

use std::collections::{HashMap, HashSet};

use mc_bamboo_core::BlockPos;

use crate::shape::Aabb;

/// 一个方块实体睡下时要关注的东西。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Watch {
    /// 方块或容器内容一变就要叫醒的格子。
    pub cells: Vec<BlockPos>,
    /// 有实体在里面出现、移动或换了物品就要叫醒的盒子。
    pub boxes: Vec<Aabb>,
}

/// 关注表：格子 → 关注者，16³ 段 → (关注者, 盒子)，再加关注者 → 登记内容的反查。
#[derive(Debug, Default)]
pub(crate) struct Watchers {
    cells: HashMap<BlockPos, Vec<BlockPos>>,
    sections: HashMap<(i32, i32, i32), Vec<(BlockPos, Aabb)>>,
    by_watcher: HashMap<BlockPos, Watch>,
    /// 段集合有过变化，实体表那份要重新同步。
    sections_changed: bool,
}

impl Watchers {
    pub(crate) fn is_empty(&self) -> bool {
        self.by_watcher.is_empty()
    }

    /// 登记一个关注者；它已有的登记先撤掉。
    pub(crate) fn watch(&mut self, watcher: BlockPos, watch: Watch) {
        self.unwatch(watcher);
        for &cell in &watch.cells {
            let list = self.cells.entry(cell).or_default();
            if !list.contains(&watcher) {
                list.push(watcher);
            }
        }
        for &area in &watch.boxes {
            for key in sections_of(area) {
                self.sections.entry(key).or_default().push((watcher, area));
                self.sections_changed = true;
            }
        }
        self.by_watcher.insert(watcher, watch);
    }

    /// 撤掉一个关注者的全部登记。
    pub(crate) fn unwatch(&mut self, watcher: BlockPos) {
        let Some(watch) = self.by_watcher.remove(&watcher) else {
            return;
        };
        for cell in watch.cells {
            if let Some(list) = self.cells.get_mut(&cell) {
                list.retain(|&other| other != watcher);
                if list.is_empty() {
                    self.cells.remove(&cell);
                }
            }
        }
        for area in watch.boxes {
            for key in sections_of(area) {
                if let Some(list) = self.sections.get_mut(&key) {
                    list.retain(|&(other, _)| other != watcher);
                    if list.is_empty() {
                        self.sections.remove(&key);
                        self.sections_changed = true;
                    }
                }
            }
        }
    }

    /// 关注这一格的方块实体，按登记顺序。
    pub(crate) fn cell_watchers(&self, pos: BlockPos) -> Option<&[BlockPos]> {
        self.cells.get(&pos).map(Vec::as_slice)
    }

    /// 盒子和 `area` 相交的关注者，按位置排序去重。
    pub(crate) fn box_watchers(&self, area: Aabb, out: &mut Vec<BlockPos>) {
        for key in sections_of(area) {
            if let Some(list) = self.sections.get(&key) {
                for &(watcher, watched) in list {
                    if watched.intersects(area) {
                        out.push(watcher);
                    }
                }
            }
        }
    }

    /// 段集合变过的话交出现在的段，给实体表同步；没变过返回 `None`。
    pub(crate) fn take_changed_sections(&mut self) -> Option<HashSet<(i32, i32, i32)>> {
        if !std::mem::take(&mut self.sections_changed) {
            return None;
        }
        Some(self.sections.keys().copied().collect())
    }
}

/// 一个盒子摸到的 16³ 段，和实体表空间索引同一套键。
pub(crate) fn sections_of(area: Aabb) -> impl Iterator<Item = (i32, i32, i32)> {
    let (first, last) = crate::entity::store::section_range(&area);
    (first.0..=last.0).flat_map(move |x| {
        (first.1..=last.1).flat_map(move |y| (first.2..=last.2).map(move |z| (x, y, z)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(x: f64) -> Aabb {
        Aabb::new(x, 64.0, 0.0, x + 1.0, 65.0, 1.0)
    }

    #[test]
    fn watching_and_unwatching_leaves_no_trace() {
        let mut watchers = Watchers::default();
        let hopper = BlockPos::new(0, 64, 0);
        watchers.watch(
            hopper,
            Watch {
                cells: vec![hopper, hopper.above()],
                boxes: vec![unit(15.5)],
            },
        );
        assert_eq!(watchers.cell_watchers(hopper.above()), Some(&[hopper][..]));
        let mut found = Vec::new();
        watchers.box_watchers(unit(16.0), &mut found);
        assert_eq!(found, [hopper], "跨段的盒子在另一段也查得到");
        found.clear();
        watchers.box_watchers(unit(15.8), &mut found);
        assert_eq!(found, [hopper, hopper], "两段各报一次，调用方去重");
        found.clear();
        watchers.box_watchers(unit(20.0), &mut found);
        assert!(found.is_empty(), "同段但不相交的不算");
        assert_eq!(watchers.take_changed_sections().map(|s| s.len()), Some(2));
        watchers.unwatch(hopper);
        assert!(watchers.is_empty());
        assert!(watchers.cell_watchers(hopper).is_none());
        assert_eq!(watchers.take_changed_sections().map(|s| s.len()), Some(0));
        assert!(watchers.take_changed_sections().is_none());
    }
}
