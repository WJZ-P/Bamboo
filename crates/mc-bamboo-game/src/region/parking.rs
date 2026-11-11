//! 区块停放表：任务 A 的「磁盘」。
//!
//! 卸载时把 region 交还的整个 [`RegionColumn`] 原样停放在这里，不经过 NBT 编码；
//! 下次加载同一个位置时整份取回来。上层内容（容器、计划刻、方块实体、红石记忆、
//! 实体）都还在盒子里，所以一局游戏之内玩家走出视距再走回来，箱子里的东西不会丢。
//!
//! 真实文件、Anvil、玩家数据、`level.dat` 和上层内容的 NBT 序列化是任务 B 的事。
//! `is_pristine` 的盒子不停放，直接丢掉，下次用生成器模板重新造一个。

use std::collections::BTreeMap;

use mc_bamboo_core::ChunkPos;

use super::column::RegionColumn;

/// 按区块坐标停放整个盒子。
#[derive(Debug, Default)]
pub(crate) struct Parking {
    columns: BTreeMap<ChunkPos, Box<RegionColumn>>,
}

// 提交 5 把 Director 接进 `World::tick` 后才会有正式的调用方。
#[allow(dead_code)]
impl Parking {
    /// 停放一个盒子。同一坐标后来的覆盖先来的（设计稿 5.5：丢旧快照）。
    pub(crate) fn park(&mut self, pos: ChunkPos, column: Box<RegionColumn>) {
        self.columns.insert(pos, column);
    }

    /// 取出一个盒子；没停放过返回 `None`，调用方走生成器模板。
    pub(crate) fn take(&mut self, pos: ChunkPos) -> Option<Box<RegionColumn>> {
        self.columns.remove(&pos)
    }

    /// 有没有这个位置的盒子，不取走。
    pub(crate) fn contains(&self, pos: ChunkPos) -> bool {
        self.columns.contains_key(&pos)
    }

    pub(crate) fn len(&self) -> usize {
        self.columns.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    /// 关服时把还停放的盒子全取出来（任务 B 落盘用；任务 A 只在测试里查数量）。
    pub(crate) fn take_all(&mut self) -> BTreeMap<ChunkPos, Box<RegionColumn>> {
        std::mem::take(&mut self.columns)
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_blocks::entity::{Entity, EntityKind, PrimedTnt};
    use mc_bamboo_core::Vec3;

    use super::*;
    use crate::{region::Region, world::tests::Harness};

    /// 造一个带内容的盒子：一个方块改动、一个实体，都不是原样。
    fn box_with_content(region: &mut Region, pos: ChunkPos) -> Box<RegionColumn> {
        region.chunks.set_block(
            mc_bamboo_core::BlockPos::new(pos.x * 16 + 2, -60, pos.z * 16 + 2),
            mc_bamboo_data::blocks::STONE,
        );
        region.entities.spawn(Entity::new(
            EntityKind::Tnt(PrimedTnt::new()),
            Vec3::new(f64::from(pos.x * 16 + 3), -59.0, f64::from(pos.z * 16 + 3)),
        ));
        region.chunks.finish_light_updates();
        region
            .take_columns(&[pos].into_iter().collect())
            .expect("前提满足")
            .remove(&pos)
            .expect("区块盒子在")
    }

    #[test]
    fn parked_boxes_keep_all_upper_layer_content() {
        let mut server = Harness::new(2);
        let pos = ChunkPos::new(0, 0);
        let column = box_with_content(server.region(), pos);
        let mut parking = Parking::default();
        assert!(parking.is_empty());
        parking.park(pos, column);
        assert_eq!(parking.len(), 1);
        assert!(parking.contains(pos));

        let restored = parking.take(pos).expect("停放过");
        assert!(parking.is_empty());
        assert!(!parking.contains(pos));
        server
            .region()
            .insert_column(pos, restored)
            .expect("前提满足");
        let region = server.region();
        assert_eq!(
            region
                .chunks
                .block(mc_bamboo_core::BlockPos::new(2, -60, 2)),
            Some(mc_bamboo_data::blocks::STONE),
            "方块改动要跟着盒子回来"
        );
        assert_eq!(region.entities.len(), 1, "实体也要跟着盒子回来");
    }

    #[test]
    fn parking_a_new_snapshot_drops_the_old_one() {
        let mut server = Harness::new(2);
        let pos = ChunkPos::new(0, 0);
        let mut parking = Parking::default();
        parking.park(pos, box_with_content(server.region(), pos));
        parking.park(pos, box_with_content(server.region(), pos));
        assert_eq!(parking.len(), 1, "同一坐标后来的覆盖先来的");
        let all = parking.take_all();
        assert_eq!(all.len(), 1);
        assert!(parking.is_empty());
    }
}
