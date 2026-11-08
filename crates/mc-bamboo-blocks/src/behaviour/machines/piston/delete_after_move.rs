//! 官方 `PistonBaseBlock.moveBlocks` 里的 `deleteAfterMove` 表。
//!
//! 官方用的是 `java.util.HashMap<BlockPos, BlockState>`。剩下的方块要按它的遍历顺序先写成
//! 空气、再做形状更新，邻居更新的先后会影响红石，所以顺序必须和官方一样，不能按插入顺序。
//!
//! 照 Java 8 的 `HashMap`：键的哈希是 `Vec3i.hashCode()`，再扰动成
//! `h ^ (h >>> 16)`；容量 16 起步，负载因子 0.75，元素数超过 `容量 * 3 / 4` 翻倍；
//! 扩容把每个桶按 `hash & 旧容量` 拆成两半，两半都保持桶里原来的先后；删除不缩容。
//! 遍历是桶号从小到大，同一个桶里按放进去的先后。
//!
//! 一个桶里已经有 8 个结点、再放第 9 个时官方会尝试树化：容量不到 64 就先扩容一次。
//! 推动上限是 12 个方块，这里照做这次扩容；真正的树化还要容量至少 64，推动结构凑不出来，
//! 所以树化之后的红黑树遍历顺序没有实现。

use mc_bamboo_core::BlockPos;

use crate::state::BlockState;

/// Java `HashMap` 第一次 `put` 时的容量。
const INITIAL_CAPACITY: usize = 16;
/// `DEFAULT_LOAD_FACTOR` 的 3/4。
const LOAD_NUMERATOR: usize = 3;
const LOAD_DENOMINATOR: usize = 4;
/// `TREEIFY_THRESHOLD`。官方 `putVal` 追加结点时 `binCount` 是桶里原有结点数减一，
/// `binCount >= TREEIFY_THRESHOLD - 1` 就是原有结点数不少于 8：放第 9 个时尝试树化。
const TREEIFY_THRESHOLD: usize = 8;
/// `MIN_TREEIFY_CAPACITY`：容量不到它就只扩容，树化不起来。
const MIN_TREEIFY_CAPACITY: usize = 64;

/// `deleteAfterMove`。键是方块的位置，值是 `put` 时的状态。
#[derive(Default)]
pub(super) struct DeleteAfterMove {
    /// 桶。空的时候对应 Java 还没有建表，第一次 `put` 才建。
    buckets: Vec<Vec<Entry>>,
    len: usize,
}

struct Entry {
    hash: u32,
    pos: BlockPos,
    state: BlockState,
}

impl DeleteAfterMove {
    /// `HashMap.put`：键已在表里时只换值，结点留在原来的位置。
    pub(super) fn put(&mut self, pos: BlockPos, state: BlockState) {
        if self.buckets.is_empty() {
            self.buckets.resize_with(INITIAL_CAPACITY, Vec::new);
        }
        let hash = spread(pos);
        let index = bucket_index(hash, self.buckets.len());
        let existing = {
            let bucket = &mut self.buckets[index];
            if let Some(entry) = bucket.iter_mut().find(|entry| entry.pos == pos) {
                entry.state = state;
                // 官方没有插入新结点，后面的扩容判断都跳过。
                return;
            }
            let existing = bucket.len();
            bucket.push(Entry { hash, pos, state });
            existing
        };

        // 放进去的是第 9 个或更晚的结点：官方 `treeifyBin`，容量不到 64 只扩容。
        if existing >= TREEIFY_THRESHOLD && self.buckets.len() < MIN_TREEIFY_CAPACITY {
            self.resize();
        }
        self.len += 1;
        if self.len * LOAD_DENOMINATOR > self.buckets.len() * LOAD_NUMERATOR {
            self.resize();
        }
    }

    /// `HashMap.remove`：只把结点从桶里摘掉，容量不变。
    pub(super) fn remove(&mut self, pos: BlockPos) {
        if self.buckets.is_empty() {
            return;
        }
        let hash = spread(pos);
        let index = bucket_index(hash, self.buckets.len());
        let bucket = &mut self.buckets[index];
        if let Some(position) = bucket.iter().position(|entry| entry.pos == pos) {
            bucket.remove(position);
            self.len -= 1;
        }
    }

    /// `HashMap.keySet()`、`entrySet()` 的遍历顺序。
    pub(super) fn iter(&self) -> impl Iterator<Item = (BlockPos, BlockState)> + '_ {
        self.buckets
            .iter()
            .flatten()
            .map(|entry| (entry.pos, entry.state))
    }

    /// `HashMap.resize`：容量翻倍，每个桶按 `hash & 旧容量` 拆成下标不变和下标加旧容量两半，
    /// 两半内部都保持原来的先后。
    fn resize(&mut self) {
        let old_capacity = self.buckets.len();
        let new_capacity = old_capacity * 2;
        let old = std::mem::take(&mut self.buckets);
        self.buckets = (0..new_capacity).map(|_| Vec::new()).collect();
        for bucket in old {
            for entry in bucket {
                let low = entry.hash as usize & (old_capacity - 1);
                let index = if entry.hash as usize & old_capacity == 0 {
                    low
                } else {
                    low + old_capacity
                };
                self.buckets[index].push(entry);
            }
        }
    }
}

/// `Vec3i.hashCode()`：`(y + z * 31) * 31 + x`，整数按 32 位回绕，再做 `HashMap.hash` 的扰动。
fn spread(pos: BlockPos) -> u32 {
    let hash = pos
        .y
        .wrapping_add(pos.z.wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(pos.x);
    let hash = hash as u32;
    hash ^ (hash >> 16)
}

fn bucket_index(hash: u32, capacity: usize) -> usize {
    hash as usize & (capacity - 1)
}

#[cfg(test)]
mod tests {
    use super::DeleteAfterMove;
    use crate::state::BlockState;

    fn pos(x: i32, y: i32, z: i32) -> mc_bamboo_core::BlockPos {
        mc_bamboo_core::BlockPos::new(x, y, z)
    }

    /// 状态只用来核对值有没有跟着位置走，随便取个非零的。
    fn state() -> BlockState {
        BlockState(1)
    }

    fn order(table: &DeleteAfterMove) -> Vec<mc_bamboo_core::BlockPos> {
        table.iter().map(|(pos, _)| pos).collect()
    }

    /// 按顺序 `put` 这些位置再取遍历顺序。答案来自真的 `java.util.HashMap`。
    fn put_all(positions: &[(i32, i32, i32)]) -> Vec<mc_bamboo_core::BlockPos> {
        let mut table = DeleteAfterMove::default();
        for &(x, y, z) in positions {
            table.put(pos(x, y, z), state());
        }
        order(&table)
    }

    fn positions(positions: &[(i32, i32, i32)]) -> Vec<mc_bamboo_core::BlockPos> {
        positions.iter().map(|&(x, y, z)| pos(x, y, z)).collect()
    }

    /// 12 个方块的一排：哈希就是 x，桶号一样从小到大。
    #[test]
    fn line_of_blocks() {
        let line: Vec<(i32, i32, i32)> = (0..12).map(|x| (x, 0, 0)).collect();
        assert_eq!(put_all(&line), positions(&line));
    }

    /// 反过来放，遍历还是桶号从小到大，不是放进去的顺序。
    #[test]
    fn buckets_run_from_low_to_high() {
        let reversed: Vec<(i32, i32, i32)> = (0..12).rev().map(|x| (x, 0, 0)).collect();
        let line: Vec<(i32, i32, i32)> = (0..12).map(|x| (x, 0, 0)).collect();
        assert_eq!(put_all(&reversed), positions(&line));
        assert_eq!(
            put_all(&[(5, 0, 0), (2, 0, 0), (9, 0, 0)]),
            positions(&[(2, 0, 0), (5, 0, 0), (9, 0, 0)])
        );
    }

    /// 同一个桶里按放进去的先后；扩容拆桶以后还保持这个先后。
    #[test]
    fn one_bucket_keeps_insertion_order() {
        assert_eq!(
            put_all(&[(16, 0, 0), (0, 0, 0), (32, 0, 0)]),
            positions(&[(16, 0, 0), (0, 0, 0), (32, 0, 0)])
        );
    }

    /// 负坐标和大坐标，中间位被扰动进低位。
    #[test]
    fn negative_and_large_coordinates() {
        let negatives = [
            (-1, -64, -1),
            (-2, -64, -1),
            (-17, -63, -2),
            (-33, -62, -3),
            (-5, 0, -7),
        ];
        assert_eq!(put_all(&negatives), positions(&negatives));
        assert_eq!(
            put_all(&[
                (1_000_000, 200, -300),
                (999_999, 199, -301),
                (1_000_064, 200, -300),
                (1_000_128, 200, -300),
                (-1_000_000, -200, 300),
                (131_071, 131_071, 131_071),
            ]),
            positions(&[
                (999_999, 199, -301),
                (-1_000_000, -200, 300),
                (1_000_000, 200, -300),
                (1_000_064, 200, -300),
                (1_000_128, 200, -300),
                (131_071, 131_071, 131_071),
            ])
        );
    }

    /// 删掉的结点不再出现，剩下的先后不变。
    #[test]
    fn removed_entries_disappear() {
        let mut table = DeleteAfterMove::default();
        for &(x, y, z) in &[
            (3, 4, 5),
            (3, 4, 6),
            (3, 5, 5),
            (4, 4, 5),
            (100, 4, 5),
            (0, 0, 0),
        ] {
            table.put(pos(x, y, z), state());
        }
        table.remove(pos(3, 5, 5));
        table.remove(pos(0, 0, 0));
        assert_eq!(
            order(&table),
            positions(&[(3, 4, 5), (3, 4, 6), (4, 4, 5), (100, 4, 5)])
        );
    }

    /// 同一个桶里有 8 个时官方不扩容，按放进去的先后；放第 9 个时 `treeifyBin` 扩容一次，
    /// 桶按第 5 位拆开。两组答案都来自真的 `java.util.HashMap`。
    #[test]
    fn a_crowded_bucket_resizes_on_its_ninth_entry() {
        let eight: Vec<(i32, i32, i32)> = (0..8).map(|i| (1 + 16 * i, 0, 0)).collect();
        assert_eq!(put_all(&eight), positions(&eight));

        let nine: Vec<(i32, i32, i32)> = (0..9).map(|i| (1 + 16 * i, 0, 0)).collect();
        let xs = [1, 33, 65, 97, 129, 17, 49, 81, 113];
        let split: Vec<(i32, i32, i32)> = xs.iter().map(|&x| (x, 0, 0)).collect();
        assert_eq!(put_all(&nine), positions(&split));
    }

    /// 同一个桶里到 9 个，官方会扩容（树化前的 `treeifyBin`），顺序跟着拆桶走。
    #[test]
    fn treeify_resize_keeps_order() {
        let colliding: Vec<(i32, i32, i32)> = (0..9).map(|i| (7, -9, 13 + i * 64)).collect();
        let mut table = DeleteAfterMove::default();
        for &(x, y, z) in &colliding {
            table.put(pos(x, y, z), state());
        }
        table.remove(pos(7, -9, 77));
        assert_eq!(
            order(&table),
            positions(&[
                (7, -9, 397),
                (7, -9, 333),
                (7, -9, 525),
                (7, -9, 461),
                (7, -9, 13),
                (7, -9, 205),
                (7, -9, 269),
                (7, -9, 141),
            ])
        );
    }
}
