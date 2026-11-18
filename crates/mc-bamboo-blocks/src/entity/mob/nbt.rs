//! 生物的存档 NBT。这一轮先只接通入口：牛照旧按不认识的实体整条原样保留，
//! 写盘入口报错（游戏里还生不出牛，走不到）。
//!
//! Mob archive NBT. Only the entry points are wired this round: cows on disk stay
//! preserved verbatim, and writing reports an error (no cow can exist in game yet).

use mc_bamboo_nbt::Tag;

use crate::{
    archive::tag::{Error, Result},
    entity::EntityRow,
};

/// 写一个生物。
pub fn write_mob(_row: &EntityRow) -> Result<Tag> {
    Err(Error::new("mob archive is not wired yet"))
}

/// 读一个生物；不是这一轮支持的生物时返回 `None`，交回调用方照旧处理。
pub fn read_mob(_tag: &Tag, _id: &str, _uid: i32) -> Result<Option<EntityRow>> {
    Ok(None)
}
