// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。

/// 一个高度图类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeightmapType {
    /// 官方的序列化名，比如 `MOTION_BLOCKING_NO_LEAVES`。
    pub name: &'static str,
    /// 网络上的键，也是官方 `Types` 的声明顺序。
    pub id: i32,
    /// `sendToClient`：区块包带上这一种。
    pub send_to_client: bool,
    /// `keepAfterWorldgen`：区块里一直留着这一种。
    pub keep_after_worldgen: bool,
}

/// `WORLD_SURFACE_WG`。
pub const WORLD_SURFACE_WG: HeightmapType = HeightmapType {
    name: "WORLD_SURFACE_WG",
    id: 0,
    send_to_client: false,
    keep_after_worldgen: false,
};

/// `WORLD_SURFACE`。
pub const WORLD_SURFACE: HeightmapType = HeightmapType {
    name: "WORLD_SURFACE",
    id: 1,
    send_to_client: true,
    keep_after_worldgen: true,
};

/// `OCEAN_FLOOR_WG`。
pub const OCEAN_FLOOR_WG: HeightmapType = HeightmapType {
    name: "OCEAN_FLOOR_WG",
    id: 2,
    send_to_client: false,
    keep_after_worldgen: false,
};

/// `OCEAN_FLOOR`。
pub const OCEAN_FLOOR: HeightmapType = HeightmapType {
    name: "OCEAN_FLOOR",
    id: 3,
    send_to_client: false,
    keep_after_worldgen: true,
};

/// `MOTION_BLOCKING`。
pub const MOTION_BLOCKING: HeightmapType = HeightmapType {
    name: "MOTION_BLOCKING",
    id: 4,
    send_to_client: true,
    keep_after_worldgen: true,
};

/// `MOTION_BLOCKING_NO_LEAVES`。
pub const MOTION_BLOCKING_NO_LEAVES: HeightmapType = HeightmapType {
    name: "MOTION_BLOCKING_NO_LEAVES",
    id: 5,
    send_to_client: true,
    keep_after_worldgen: true,
};

/// 全部类型，顺序和官方 `Types.values()` 相同。
pub static TYPES: &[HeightmapType] = &[WORLD_SURFACE_WG, WORLD_SURFACE, OCEAN_FLOOR_WG, OCEAN_FLOOR, MOTION_BLOCKING, MOTION_BLOCKING_NO_LEAVES];
