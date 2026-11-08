// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。

/// 合成余料的组件补丁拼在一起，位置见 [`ENTRIES`] 的最后两列。
pub static PATCHES: &[u8] = b"";

/// `(物品 ID, 余料物品, 数量, 补丁起点, 补丁终点)`，按物品 ID 排好。
pub static ENTRIES: &[(i32, i32, i32, u32, u32)] = &[
    (1129, 1128, 1, 0, 0),
    (1130, 1128, 1, 0, 0),
    (1134, 1128, 1, 0, 0),
    (1441, 1270, 1, 0, 0),
    (1533, 1270, 1, 0, 0),
];

/// 这个物品的 `getCraftingRemainder`。没有余料时是 `None`。
pub fn of(item: i32) -> Option<(i32, i32, &'static [u8])> {
    let index = ENTRIES.binary_search_by_key(&item, |entry| entry.0).ok()?;
    let entry = ENTRIES[index];
    Some((entry.1, entry.2, &PATCHES[entry.3 as usize..entry.4 as usize]))
}
