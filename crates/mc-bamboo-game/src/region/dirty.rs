//! 「上次保存之后改过没有」的标记，对应官方 `ChunkAccess.unsaved`。
//!
//! 官方 `ChunkMap.saveChunkIfNeeded` 只看 `isUnsaved()`；`markUnsaved` 在
//! `LevelChunk.setBlockState`、`setLightCorrect`、方块实体 `setChanged` 等写路径
//! 置位，`ChunkMap.save` 用 `tryMarkSaved()` 清掉。Bamboo 照这个分工：
//!
//! - 下层（方块、光照）由 `ChunkStore` 版本号对比，不用脏集合。
//! - 上层（容器、物品组件、命令方块、讲台、方块实体、计划刻、红石记忆、实体）
//!   在可变访问入口调 `Dirty::mark` 把区块放进集合。宁多勿少，多存一次没有错。
//! - 每刻只碰这个集合，开销和集合大小有关，和已加载区块数无关。
//!
//! `content_fingerprints` 留作 `cfg(test)` 的校验器：判定「没改过」时用快照编码
//! 和上次保存的字节复核，证明改动标记没有漏。
//!
//! Vanilla-style per-chunk dirty marks. Only the dirty set is scanned each tick;
//! never the whole loaded-chunk table. The content fingerprint is a test-only
//! verifier that re-encodes clean chunks and compares with the last saved bytes.

use std::collections::BTreeSet;

use mc_bamboo_core::ChunkPos;

/// 按区块记「改过、还没写成功」的集合。
#[derive(Debug, Default)]
pub(crate) struct Dirty {
    pending: BTreeSet<ChunkPos>,
    /// 测试模式下，上次写出的区块 NBT；复核「没改过」的判断有没有漏标脏。
    #[cfg(test)]
    last_bytes: std::collections::BTreeMap<ChunkPos, mc_bamboo_nbt::Tag>,
}

impl Dirty {
    pub(crate) fn mark(&mut self, pos: ChunkPos) {
        self.pending.insert(pos);
    }

    /// 这一刻有上层内容改动的区块。
    pub(crate) fn pending_contains(&self, pos: ChunkPos) -> bool {
        self.pending.contains(&pos)
    }

    pub(crate) fn take(&mut self) -> BTreeSet<ChunkPos> {
        std::mem::take(&mut self.pending)
    }

    /// 测试用：记下这次写出的 NBT，供 `verify_clean` 比较。
    #[cfg(test)]
    #[allow(dead_code, reason = "端到端 / 差分测试用快照字节复核改动标记")]
    pub(crate) fn note_bytes(&mut self, pos: ChunkPos, chunk: mc_bamboo_nbt::Tag) {
        self.last_bytes.insert(pos, chunk);
    }

    /// 测试模式：判定「没改过」的区块必须真的和上次写出的字节相同。
    /// 计划刻的 `t` 是相对延迟，每刻都变，比较时抹掉它。
    #[cfg(test)]
    pub(crate) fn verify_clean(&self, region: &crate::region::Region, pos: ChunkPos) {
        use mc_bamboo_nbt::Tag;

        if self.pending.contains(&pos) {
            return;
        }
        let Some(before) = self.last_bytes.get(&pos) else {
            return;
        };
        let region_column = region.chunks.column_clone(pos);
        let Some(column) = region_column else {
            return;
        };
        let after = mc_bamboo_storage::chunk_nbt::encode(
            &column,
            &mc_bamboo_storage::chunk_nbt::ChunkEncodeExtra {
                chunk_pos: pos,
                ..Default::default()
            },
        )
        .expect("测试里的快照必须能编码");
        let normalize = |tag: &Tag| {
            let Tag::Compound(entries) = tag else {
                return tag.clone();
            };
            Tag::Compound(
                entries
                    .iter()
                    .map(|(key, value)| {
                        if (key == "block_ticks" || key == "fluid_ticks")
                            && let Tag::List(list) = value
                        {
                            let list = list
                                .iter()
                                .map(|item| {
                                    let Tag::Compound(fields) = item else {
                                        return item.clone();
                                    };
                                    Tag::Compound(
                                        fields
                                            .iter()
                                            .map(|(field, value)| {
                                                if field == "t" {
                                                    (field.clone(), Tag::Int(0))
                                                } else {
                                                    (field.clone(), value.clone())
                                                }
                                            })
                                            .collect(),
                                    )
                                })
                                .collect();
                            (key.clone(), Tag::List(list))
                        } else {
                            (key.clone(), value.clone())
                        }
                    })
                    .collect(),
            )
        };
        assert_eq!(
            normalize(&after),
            normalize(before),
            "区块 ({}, {}) 被判成没改过，但只读快照变了（漏标脏）",
            pos.x,
            pos.z
        );
    }
}
