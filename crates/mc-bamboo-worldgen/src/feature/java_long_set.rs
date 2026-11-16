//! `it.unimi.dsi.fastutil.longs.LongOpenHashSet` 的迭代顺序复刻。
//!
//! `ChunkAccess.structureReferences` 是 `HashMap<Structure, LongOpenHashSet>`；
//! `StructureManager.startsForStructure` 按 `LongOpenHashSet` 的迭代序反查起点，
//! `applyBiomeDecoration` 对同一个结构的这些起点依次 `placeInChunk`，共用一份
//! `WorldgenRandom`。起点顺序因此决定后续所有取数，必须逐个照官方放。
//!
//! 字节码核对（fastutil 8.5.18，`javap -c -p LongOpenHashSet` / `HashCommon`）：
//! * 默认构造 `new LongOpenHashSet()` 先按 `expected=16` 调
//!   `arraySize(16, 0.75f) = 32`，所以 `n = 32`、`f = 0.75f`、
//!   `maxFill = min(ceil(n*f), n-1) = 24`；
//! * `key` 数组长度 `n+1`，下标 `n` 是键 0 的专槽（`containsNull`），
//!   其余下标 `0..n` 用 0 表示空；
//! * `add(k)`：`k == 0` 走专槽，否则 `pos = (int)HashCommon.mix(k) & mask`，
//!   占用就 `(pos+1) & mask` 线性探测，找到相等键就返回 false；
//! * 插入后 `size >= maxFill` 时 `rehash(arraySize(size+1, f))`：
//!   `arraySize = max(2, nextPowerOfTwo(ceil(expected/f)))`，
//!   `rehash` **从旧表高位向低位**扫，把每个非零键按同样的探测插到新表；
//! * 迭代器（`SetIterator.nextLong`）先出键 0（如果 `containsNull`），再按
//!   `pos = n` 递减扫 `key[pos]`，跳过 0；`wrapped` 只在 `remove` 后出现，
//!   引用集合只做 add，不会有。
//!
//! Iteration order of fastutil's `LongOpenHashSet`, which vanilla uses for
//! structure references. The order decides structure placement and therefore
//! the shared feature random stream. Insertion history (including rehash
//! timing and linear probing) is reproduced exactly.

/// `HashCommon.mix(long)`：乘 phi 常数后两次无符号右移异或。
///
/// `HashCommon.mix(long)`: multiply by the phi constant then two unsigned-shift xors.
fn mix(value: i64) -> i64 {
    let x = value.wrapping_mul(-7_046_029_254_386_353_131_i64);
    let y = x ^ (x as u64 >> 32) as i64;
    y ^ (y as u64 >> 16) as i64
}

/// `HashCommon.nextPowerOfTwo(long)`：
/// `1L << (64 - Long.numberOfLeadingZeros(x - 1))`，输入不超过 `1 << 30`。
fn next_power_of_two(value: i64) -> usize {
    if value <= 1 {
        return 1;
    }
    let shift = 64 - ((value - 1) as u64).leading_zeros();
    (1_u64 << shift) as usize
}

/// `HashCommon.arraySize(expected, f)`。
fn array_size(expected: usize, f: f64) -> usize {
    let want = next_power_of_two((expected as f64 / f).ceil() as i64);
    want.max(2)
}

/// `HashCommon.maxFill(n, f)`。
fn max_fill(n: usize, f: f64) -> usize {
    ((n as f64 * f).ceil() as usize).min(n - 1)
}

/// 一个只做 add 的 `LongOpenHashSet`，`to_vec()` 给出官方迭代器顺序。
///
/// An insert-only `LongOpenHashSet`; `to_vec()` returns the official iteration order.
#[derive(Debug, Clone)]
pub struct JavaLongSet {
    key: Vec<i64>,
    mask: usize,
    contains_null: bool,
    n: usize,
    max_fill: usize,
    size: usize,
}

impl Default for JavaLongSet {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaLongSet {
    /// 默认构造：`new LongOpenHashSet()`。
    pub fn new() -> Self {
        let n = array_size(16, 0.75);
        Self {
            key: vec![0; n + 1],
            mask: n - 1,
            contains_null: false,
            n,
            max_fill: max_fill(n, 0.75),
            size: 0,
        }
    }

    /// `add(long)`，返回是否真的插入（重复键返回 false）。
    pub fn insert(&mut self, value: i64) -> bool {
        if value == 0 {
            if self.contains_null {
                return false;
            }
            self.contains_null = true;
        } else {
            let mut pos = (mix(value) as usize) & self.mask;
            loop {
                let current = self.key[pos];
                if current == 0 {
                    break;
                }
                if current == value {
                    return false;
                }
                pos = (pos + 1) & self.mask;
            }
            self.key[pos] = value;
        }
        // 官方字节码 96-111 在 `size++` 之后、比较时压栈的是 **自增前的 size**
        // （`dup_x1` 留下的旧值）；`if_icmplt` 跳过 rehash 的条件是
        // `oldSize < maxFill`。也就是第 `maxFill+1` 个元素插入时才扩容，
        // 而 `arraySize(size+1, f)` 用的是自增后的新 size。
        // Vanilla compares the *pre-increment* size against maxFill: rehash
        // happens on the (maxFill+1)-th element, and arraySize uses the new size.
        let old_size = self.size;
        self.size += 1;
        if old_size >= self.max_fill {
            let new_n = array_size(self.size + 1, 0.75);
            self.rehash(new_n);
        }
        true
    }

    /// `rehash(newN)`：从旧表的高位 `n` 往低位 1 扫，按新掩码重新探测插入。
    fn rehash(&mut self, new_n: usize) {
        let old = std::mem::replace(&mut self.key, vec![0; new_n + 1]);
        let new_mask = new_n - 1;
        // 旧表的非 null 键在 `0..n-1`（下标 n 是键 0 专槽）。从 `n-1` 递减到
        // **0**（含），跳过空槽；漏掉下标 0 会在扩容时丢一个键。
        // Non-null keys live at `0..n-1` (index n is the null slot). Walk down to
        // and including 0; missing index 0 loses a key on every rehash.
        let mut i = self.n;
        while i > 0 {
            i -= 1;
            let value = old[i];
            if value == 0 {
                continue;
            }
            let mut pos = (mix(value) as usize) & new_mask;
            while self.key[pos] != 0 {
                pos = (pos + 1) & new_mask;
            }
            self.key[pos] = value;
        }
        self.n = new_n;
        self.mask = new_mask;
        self.max_fill = max_fill(new_n, 0.75);
    }

    /// `SetIterator` 的顺序：键 0 专槽先出，然后从 `n-1` 递减扫。
    pub fn to_vec(&self) -> Vec<i64> {
        let mut out = Vec::with_capacity(self.size);
        if self.contains_null {
            out.push(0);
        }
        let mut pos = self.n;
        while pos > 0 {
            pos -= 1;
            let value = self.key[pos];
            if value != 0 {
                out.push(value);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_capacity_and_load_factor_match_fastutil() {
        let set = JavaLongSet::new();
        assert_eq!(set.n, 32);
        assert_eq!(set.max_fill, 24);
        assert_eq!(set.key.len(), 33);
    }

    #[test]
    fn zero_key_comes_first() {
        let mut set = JavaLongSet::new();
        set.insert(7);
        set.insert(0);
        set.insert(9);
        assert_eq!(set.to_vec().first(), Some(&0));
    }

    #[test]
    fn duplicate_insert_does_not_change_order_or_size() {
        let mut set = JavaLongSet::new();
        assert!(set.insert(5));
        assert!(!set.insert(5));
        assert_eq!(set.to_vec(), vec![5]);
        assert_eq!(set.size, 1);
    }

    #[test]
    fn matches_official_iteration_orders() {
        // 用 `java DumpOrder.java` 在官方 fastutil 8.5.18 上实测的几组。
        let cases: &[(&[i64], &[i64])] = &[
            (
                &[-68_719_476_739, -73_014_444_032],
                &[-73_014_444_032, -68_719_476_739],
            ),
            (
                &[-73_014_444_032, -68_719_476_739],
                &[-73_014_444_032, -68_719_476_739],
            ),
            (&[0, 1, 2, 3, 4], &[0, 1, 3, 4, 2]),
            (
                &[68_719_476_736, 4_294_967_296, 1, 0],
                &[0, 4_294_967_296, 68_719_476_736, 1],
            ),
        ];
        for (input, expected) in cases {
            let mut set = JavaLongSet::new();
            for &value in *input {
                set.insert(value);
            }
            assert_eq!(set.to_vec(), *expected, "input {input:?}");
        }
    }
}

/// 运行时回放官方 `JavaSetOracle --longset --cases 3000` 的输出，逐条比迭代序。
/// 数据文件由 `long-set` 子命令生成，单元测试读同一份文本保证两条路径一致。
#[cfg(test)]
mod oracle_replay {
    use super::JavaLongSet;

    #[test]
    fn matches_three_thousand_fastutil_cases() {
        // 与 `long-set` 子命令共享同一份官方输出（3000 组、含键 0 / 负数 /
        // 掩码冲突 / 跨扩容边界 / 重复键）。
        let text = include_str!("java_long_set_cases.txt");
        let mut lines = text.lines();
        let mut cases = 0_usize;
        while let Some(line) = lines.next() {
            if !line.starts_with("CASE ") {
                continue;
            }
            let case = line.to_owned();
            let ops = lines
                .next()
                .unwrap_or("")
                .strip_prefix("OPS ")
                .unwrap_or("");
            let order = lines
                .next()
                .unwrap_or("")
                .strip_prefix("ORDER ")
                .unwrap_or("");
            let mut set = JavaLongSet::new();
            for value in ops.split(';').filter(|s| !s.is_empty()) {
                set.insert(value.parse::<i64>().unwrap());
            }
            let expected: Vec<i64> = order
                .split(';')
                .filter(|s| !s.is_empty())
                .map(|value| value.parse::<i64>().unwrap())
                .collect();
            let actual = set.to_vec();
            let first = actual
                .iter()
                .zip(expected.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(actual.len().min(expected.len()));
            assert_eq!(
                actual,
                expected,
                "{case}: first difference at {first} ({:?} vs {:?})",
                actual.get(first),
                expected.get(first)
            );
            cases += 1;
        }
        assert_eq!(cases, 3000);
    }

    /// 故意改坏一处：把 `to_vec` 的前两个输出交换，回放断言必须失败
    /// （防止「两边都空 / 比较器没跑」的假绿）。
    #[test]
    fn swapping_two_orders_breaks_the_oracle_replay() {
        let text = include_str!("java_long_set_cases.txt");
        let mut lines = text.lines();
        let mut found = false;
        while let Some(line) = lines.next() {
            if !line.starts_with("CASE ") {
                continue;
            }
            let ops = lines
                .next()
                .unwrap_or("")
                .strip_prefix("OPS ")
                .unwrap_or("");
            let order = lines
                .next()
                .unwrap_or("")
                .strip_prefix("ORDER ")
                .unwrap_or("");
            let mut expected: Vec<i64> = order
                .split(';')
                .filter(|s| !s.is_empty())
                .map(|value| value.parse::<i64>().unwrap())
                .collect();
            if expected.len() < 2 {
                continue;
            }
            expected.swap(0, 1);
            let mut set = JavaLongSet::new();
            for value in ops.split(';').filter(|s| !s.is_empty()) {
                set.insert(value.parse::<i64>().unwrap());
            }
            assert_ne!(set.to_vec(), expected, "the comparator stayed green");
            found = true;
            break;
        }
        assert!(found);
    }
}
