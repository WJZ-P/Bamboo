//! Java 8 `HashMap<String, ?>` 的遍历顺序。
//!
//! 官方 `CompoundTag` 用 `HashMap`，`write` 走 `keySet()`，不是插入顺序。
//! 组件 `Codec` 把字段按声明顺序 `put` 进一张新表，再按桶序写出。
//! 这里复刻那张表，好让重新编码的 NBT 和官方逐字节相同。
//!
//! Java 8 `HashMap<String, ?>` iteration order.
//!
//! `CompoundTag` stores entries in a `HashMap`, and `write` follows `keySet()`,
//! not insertion order. A component `Codec` `put`s fields in declaration order
//! into a fresh map and then writes bucket order. This table reproduces that
//! so a re-encoded compound matches the official bytes.

use mc_bamboo_nbt::Tag;

/// `HashMap` 第一次 `put` 时的容量。
const INITIAL_CAPACITY: usize = 16;
/// `DEFAULT_LOAD_FACTOR` 的 3/4。
const LOAD_NUMERATOR: usize = 3;
const LOAD_DENOMINATOR: usize = 4;
/// `TREEIFY_THRESHOLD`。放进第 9 个结点时尝试树化。
const TREEIFY_THRESHOLD: usize = 8;
/// `MIN_TREEIFY_CAPACITY`：容量不到它就只扩容。
const MIN_TREEIFY_CAPACITY: usize = 64;

/// 按插入顺序放进 Java `HashMap` 之后的遍历下标。
///
/// 键已经在表里时只换值，位置不动。容量至少 64 且某个桶超过 8 个结点时，
/// 官方会改成红黑树；组件表凑不出那种情况，这里仍按链表顺序走。
///
/// Indexes in Java `HashMap` iteration order after inserting `keys` in order.
///
/// An existing key only replaces the value and stays put. A bin that grows past
/// eight nodes at capacity 64 or more becomes a red-black tree. Component maps
/// do not reach that, so this still walks the linked list.
pub(crate) fn iteration_order(keys: &[String]) -> Vec<usize> {
    let mut map = StringMap::default();
    for (index, key) in keys.iter().enumerate() {
        map.put(key, index);
    }
    map.iter().collect()
}

/// 把复合标签的键按「当前顺序就是 `put` 顺序」重排成 `HashMap` 的遍历顺序。
///
/// 嵌套的复合标签同样处理。列表保持原顺序，因为 `ListTag` 是 `ArrayList`。
///
/// Reorder a compound as if its current key order were the `put` order.
///
/// Nested compounds are reordered the same way. Lists stay put because `ListTag`
/// is an `ArrayList`.
pub(crate) fn hash_order(tag: Tag) -> Tag {
    match tag {
        Tag::Compound(entries) => {
            let entries: Vec<(String, Tag)> = entries
                .into_iter()
                .map(|(key, value)| (key, hash_order(value)))
                .collect();
            let order = iteration_order(
                &entries
                    .iter()
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>(),
            );
            Tag::Compound(
                order
                    .into_iter()
                    .map(|index| entries[index].clone())
                    .collect(),
            )
        }
        Tag::List(items) => Tag::List(items.into_iter().map(hash_order).collect()),
        other => other,
    }
}

#[derive(Default)]
struct StringMap {
    buckets: Vec<Vec<Node>>,
    len: usize,
}

struct Node {
    hash: u32,
    key: String,
    index: usize,
}

impl StringMap {
    fn put(&mut self, key: &str, index: usize) {
        if self.buckets.is_empty() {
            self.buckets.resize_with(INITIAL_CAPACITY, Vec::new);
        }
        let hash = spread(key);
        let bucket_index = hash as usize & (self.buckets.len() - 1);
        let existing = {
            let bucket = &mut self.buckets[bucket_index];
            if let Some(node) = bucket.iter_mut().find(|node| node.key == key) {
                node.index = index;
                return;
            }
            let existing = bucket.len();
            bucket.push(Node {
                hash,
                key: key.to_owned(),
                index,
            });
            existing
        };
        if existing >= TREEIFY_THRESHOLD && self.buckets.len() < MIN_TREEIFY_CAPACITY {
            self.resize();
        }
        self.len += 1;
        if self.len * LOAD_DENOMINATOR > self.buckets.len() * LOAD_NUMERATOR {
            self.resize();
        }
    }

    fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.buckets.iter().flatten().map(|node| node.index)
    }

    fn resize(&mut self) {
        let old_capacity = self.buckets.len();
        let new_capacity = old_capacity * 2;
        let old = std::mem::take(&mut self.buckets);
        self.buckets = (0..new_capacity).map(|_| Vec::new()).collect();
        for bucket in old {
            for node in bucket {
                let low = node.hash as usize & (old_capacity - 1);
                let index = if node.hash as usize & old_capacity == 0 {
                    low
                } else {
                    low + old_capacity
                };
                self.buckets[index].push(node);
            }
        }
    }
}

/// `String.hashCode()` 再做 `HashMap.hash` 的扰动 `h ^ (h >>> 16)`。
///
/// `String.hashCode()` then `HashMap.hash`, which is `h ^ (h >>> 16)`.
fn spread(key: &str) -> u32 {
    let mut hash: i32 = 0;
    for unit in key.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    let hash = hash as u32;
    hash ^ (hash >> 16)
}

#[cfg(test)]
mod tests {
    use super::iteration_order;

    /// 答案来自真的 `java.util.HashMap`（Java 26）。
    #[test]
    fn string_keys_follow_java_hash_map() {
        let keys = ["items", "count", "components", "predicates"].map(str::to_owned);
        let order = iteration_order(&keys);
        let ordered: Vec<&str> = order.iter().map(|&index| keys[index].as_str()).collect();
        assert_eq!(ordered, ["components", "predicates", "count", "items"]);
    }
}
