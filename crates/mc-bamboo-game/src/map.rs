//! 地图数据、索引与持久化：对应官方 `MapIndex`、`MapItemSavedData`、
//! `MapBanner`、`MapFrame`、`MapDecoration`、`HoldingPlayer` 与 `MapPatch`。
//!
//! 26.3 文件布局（以字节码为准）：
//! - `MapIndex.TYPE`：`minecraft:maps/last_id` → `data/minecraft/maps/last_id.dat`
//! - `MapItemSavedData.type(MapId)`：`minecraft:maps/<id>` → `data/minecraft/maps/<id>.dat`
//!
//! Map data, index, and persistence matching vanilla 26.3 (`MapIndex`,
//! `MapItemSavedData`, `MapBanner`, `MapFrame`, `HoldingPlayer`, `MapPatch`).

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use mc_bamboo_blocks::{
    BlockState, InteractionResult, SupportType, World as BlockWorld,
    entity::{KindPublic, hanging::ItemFramePublic},
};
use mc_bamboo_core::{BlockPos, ChunkPos, Direction, Hand};
use mc_bamboo_data::{
    block_of_state, blocks,
    component_registries::registry_names::MAP_DECORATION_TYPE,
    items,
    registries::{
        block_entity_type as block_entities, data_component_type as kinds, fluid as fluid_reg,
    },
    sounds, state_fluid, state_map_color,
    tags::block_tags,
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry, defaults, text::checked_component},
    item::ItemStack,
    packet::play::{GameMode, MapDecoration, MapItemData, MapPatch},
    text::Component,
};
use mc_bamboo_world::{MIN_Y, store::ChunkStore};

use crate::{
    global::Global,
    global_port::{GlobalEffect, GlobalPort},
    inbound::PlayerId,
    region::Region,
};

/// 地图宽 / 高（像素数），官方 `MapItemSavedData.MAP_SIZE`。
pub const MAP_SIZE: usize = 128;
/// 地图半宽，官方 `MapItemSavedData.HALF_MAP_SIZE`。
pub const HALF_MAP_SIZE: i32 = 64;
/// 颜色数组字节数（128 × 128）。
pub const COLORS_LEN: usize = MAP_SIZE * MAP_SIZE;
/// 最大缩放等级，官方 `MapItemSavedData.MAX_SCALE`。
pub const MAX_SCALE: i8 = 4;
/// 追踪标记数量上限，官方 `MapItemSavedData.TRACKED_DECORATION_LIMIT`。
pub const TRACKED_DECORATION_LIMIT: i32 = 256;
/// `MapIndex.NO_MAP_ID`。
pub const NO_MAP_ID: i32 = -1;

/// 官方 `MapColor` 常量 ID（只列逻辑里直接引用的）。
pub mod map_color {
    pub const NONE: u8 = 0;
    pub const DIRT: u8 = 10;
    pub const STONE: u8 = 11;
    pub const WATER: u8 = 12;
}

/// 官方 `MapColor$Brightness`，编码在压缩颜色字节的低 2 位。
///
/// Official `MapColor.Brightness`, packed into the low 2 bits of a map pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Brightness {
    Low = 0,
    Normal = 1,
    High = 2,
    Lowest = 3,
}

/// 官方 `MapColor.getPackedId(Brightness)`：`(color_id << 2) | (brightness & 3)`。
///
/// Official `MapColor.getPackedId(Brightness)`.
#[inline]
pub const fn packed_color(color_id: u8, brightness: Brightness) -> u8 {
    (color_id << 2) | (brightness as u8 & 3)
}

/// 官方 `BuiltInRegistries.MAP_DECORATION_TYPE` 常用注册表 ID。
pub mod decoration_type {
    pub const PLAYER: i32 = 0;
    pub const FRAME: i32 = 1;
    pub const RED_MARKER: i32 = 2;
    pub const BLUE_MARKER: i32 = 3;
    pub const TARGET_X: i32 = 4;
    pub const TARGET_POINT: i32 = 5;
    pub const PLAYER_OFF_MAP: i32 = 6;
    pub const PLAYER_OFF_LIMITS: i32 = 7;
    pub const BANNER_WHITE: i32 = 10;
}

/// 官方 `MapDecorationType.trackCount()`（按 26.3 `MapDecorationTypes` 字节码）。
///
/// Official `MapDecorationType.trackCount()` from 26.3 `MapDecorationTypes`.
pub const fn decoration_tracks_count(kind: i32) -> bool {
    matches!(kind, 0..=3 | 6..=7 | 10..=25)
}

/// 按注册名（带或不带 `minecraft:`）查 `minecraft:map_decoration_type` ID。
///
/// Look up a `minecraft:map_decoration_type` registry ID by name.
pub fn decoration_id_from_name(name: &str) -> Option<i32> {
    let full = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    MAP_DECORATION_TYPE
        .iter()
        .position(|&entry| entry == full)
        .map(|idx| idx as i32)
}

const DYE_COLOR_NAMES: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// `DyeColor` 序号（0..=15）→ 官方序列化名。
pub fn dye_color_name(color: u8) -> &'static str {
    DYE_COLOR_NAMES
        .get(usize::from(color))
        .copied()
        .unwrap_or("white")
}

/// 官方 `DyeColor.CODEC` 序列化名 → 序号（0..=15）。
pub fn dye_color_from_name(name: &str) -> Option<u8> {
    DYE_COLOR_NAMES
        .iter()
        .position(|&entry| entry == name)
        .map(|idx| idx as u8)
}

/// 从方块状态提取旗帜底色（`DyeColor` 序号 0..=15）；不是旗帜方块返回 `None`。
///
/// Extract the banner base `DyeColor` ordinal (0..=15) from a block state, or
/// `None` if the block is not a standing or wall banner.
pub fn banner_color_from_state(state: u32) -> Option<u8> {
    let block = block_of_state(state)?;
    if block.block_entity_type != Some(block_entities::BANNER) {
        return None;
    }
    let name = block.name.strip_prefix("minecraft:").unwrap_or(block.name);
    let color = name
        .strip_suffix("_wall_banner")
        .or_else(|| name.strip_suffix("_banner"))?;
    dye_color_from_name(color)
}

/// 官方 `MapBanner.fromWorld(BlockGetter, BlockPos)`：读取世界中的旗帜方块底色与自定义名。
///
/// Official `MapBanner.fromWorld`: read the banner block's base color and
/// optional custom name from world chunks and block-entity item components.
pub fn banner_from_world(
    chunks: &ChunkStore,
    block_items: &BTreeMap<BlockPos, ComponentPatch>,
    pos: BlockPos,
) -> Option<MapBanner> {
    let state = chunks.block(pos)?;
    let color = banner_color_from_state(state)?;
    let name = block_items
        .get(&pos)
        .and_then(|patch| match patch.get(kinds::CUSTOM_NAME) {
            Some(ComponentValue::CustomName(name)) => Some(name.clone()),
            _ => None,
        });
    Some(MapBanner { pos, color, name })
}

/// 地图上的旗帜标记，对应官方 `MapBanner`。
///
/// A banner marker stored on a map, corresponding to vanilla `MapBanner`.
#[derive(Debug, Clone, PartialEq)]
pub struct MapBanner {
    pub pos: BlockPos,
    /// `DyeColor` 序号（0 = white ..= 15 = black）。
    pub color: u8,
    pub name: Option<Component>,
}

impl MapBanner {
    /// 官方 `MapBanner.getId()`：`"banner-x,y,z"`。
    pub fn id(&self) -> String {
        format!("banner-{},{},{}", self.pos.x, self.pos.y, self.pos.z)
    }

    /// 官方 `MapBanner.getDecoration()`：`BANNER_WHITE + color`。
    pub fn decoration_kind(&self) -> i32 {
        decoration_type::BANNER_WHITE + i32::from(self.color.min(15))
    }

    /// 编成官方 `MapBanner.CODEC` 的复合标签。
    pub fn to_tag(&self) -> Tag {
        let mut entries = vec![
            (
                "pos".to_owned(),
                Tag::IntArray(vec![self.pos.x, self.pos.y, self.pos.z]),
            ),
            (
                "color".to_owned(),
                Tag::String(dye_color_name(self.color).to_owned()),
            ),
        ];
        if let Some(name) = &self.name {
            entries.push(("name".to_owned(), name.tag().clone()));
        }
        Tag::Compound(entries)
    }

    /// 从官方 `MapBanner.CODEC` 的复合标签解码。
    pub fn from_tag(tag: &Tag) -> Option<Self> {
        let pos = get_block_pos(tag, "pos")?;
        let color = match get(tag, "color") {
            Some(Tag::String(s)) => dye_color_from_name(s).unwrap_or(0),
            _ => 0,
        };
        let name = get(tag, "name").and_then(checked_component);
        Some(Self { pos, color, name })
    }
}

/// 地图上的物品展示框标记，对应官方 `MapFrame`。
///
/// An item frame marker stored on a map, corresponding to vanilla `MapFrame`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapFrame {
    pub pos: BlockPos,
    pub rotation: i32,
    pub entity_id: i32,
}

impl MapFrame {
    /// 官方 `MapFrame.frameId(BlockPos)`：`"frame-x,y,z"`。
    pub fn frame_id(pos: BlockPos) -> String {
        format!("frame-{},{},{}", pos.x, pos.y, pos.z)
    }

    /// 官方 `MapFrame.getId()`。
    pub fn id(&self) -> String {
        Self::frame_id(self.pos)
    }

    /// 编成官方 `MapFrame.CODEC` 的复合标签。
    pub fn to_tag(&self) -> Tag {
        Tag::Compound(vec![
            (
                "pos".to_owned(),
                Tag::IntArray(vec![self.pos.x, self.pos.y, self.pos.z]),
            ),
            ("rotation".to_owned(), Tag::Int(self.rotation)),
            ("entity_id".to_owned(), Tag::Int(self.entity_id)),
        ])
    }

    /// 从官方 `MapFrame.CODEC` 的复合标签解码。
    pub fn from_tag(tag: &Tag) -> Option<Self> {
        let pos = get_block_pos(tag, "pos")?;
        let rotation = get_int(tag, "rotation")?;
        let entity_id = get_int(tag, "entity_id")?;
        Some(Self {
            pos,
            rotation,
            entity_id,
        })
    }
}

/// 官方 `MapItemSavedData.getFrameKey(int)`：`"frame-" + entityId`。
pub fn frame_decoration_key(entity_id: i32) -> String {
    format!("frame-{entity_id}")
}

/// 持有所有权的颜色脏区补丁，用于构造 [`MapItemData`]。
///
/// Owned color update patch used to build [`MapItemData`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedMapPatch {
    pub start_x: u8,
    pub start_y: u8,
    pub width: u8,
    pub height: u8,
    pub colors: Vec<u8>,
}

/// 持有所有权的一份地图更新包内容，对应官方 `ClientboundMapItemDataPacket`。
///
/// Owned map update payload corresponding to `ClientboundMapItemDataPacket`.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnedMapUpdate {
    pub map_id: i32,
    pub scale: i8,
    pub locked: bool,
    pub decorations: Option<Vec<MapDecoration>>,
    pub color_patch: Option<OwnedMapPatch>,
}

impl OwnedMapUpdate {
    /// 转为可编码的协议包视图。
    ///
    /// Borrow as a protocol [`MapItemData`] packet.
    pub fn as_packet(&self) -> MapItemData<'_> {
        MapItemData {
            map_id: self.map_id,
            scale: self.scale,
            locked: self.locked,
            decorations: self.decorations.as_deref(),
            color_patch: self.color_patch.as_ref().map(|p| MapPatch {
                start_x: p.start_x,
                start_y: p.start_y,
                width: p.width,
                height: p.height,
                colors: &p.colors,
            }),
        }
    }
}

/// 一名正在持有 / 观察这张地图的玩家同步状态，对应官方 `MapItemSavedData$HoldingPlayer`。
///
/// Per-player dirty tracking state, corresponding to vanilla
/// `MapItemSavedData.HoldingPlayer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldingPlayer {
    pub player: PlayerId,
    dirty_data: bool,
    min_dirty_x: u8,
    min_dirty_y: u8,
    max_dirty_x: u8,
    max_dirty_y: u8,
    dirty_decorations: bool,
    tick: i32,
    pub step: i32,
}

impl HoldingPlayer {
    pub fn new(player: PlayerId) -> Self {
        Self {
            player,
            dirty_data: true,
            min_dirty_x: 0,
            min_dirty_y: 0,
            max_dirty_x: 127,
            max_dirty_y: 127,
            dirty_decorations: true,
            tick: 0,
            step: 0,
        }
    }

    pub fn mark_colors_dirty(&mut self, x: u8, y: u8) {
        if self.dirty_data {
            self.min_dirty_x = self.min_dirty_x.min(x);
            self.min_dirty_y = self.min_dirty_y.min(y);
            self.max_dirty_x = self.max_dirty_x.max(x);
            self.max_dirty_y = self.max_dirty_y.max(y);
        } else {
            self.dirty_data = true;
            self.min_dirty_x = x;
            self.min_dirty_y = y;
            self.max_dirty_x = x;
            self.max_dirty_y = y;
        }
    }

    pub fn mark_decorations_dirty(&mut self) {
        self.dirty_decorations = true;
    }

    fn create_patch(&self, colors: &[u8; COLORS_LEN]) -> OwnedMapPatch {
        let start_x = self.min_dirty_x;
        let start_y = self.min_dirty_y;
        let width = self.max_dirty_x - self.min_dirty_x + 1;
        let height = self.max_dirty_y - self.min_dirty_y + 1;
        let w = usize::from(width);
        let h = usize::from(height);
        let sx = usize::from(start_x);
        let sy = usize::from(start_y);
        let mut buf = vec![0u8; w * h];
        for dx in 0..w {
            for dy in 0..h {
                buf[dx + dy * w] = colors[(sx + dx) + (sy + dy) * MAP_SIZE];
            }
        }
        OwnedMapPatch {
            start_x,
            start_y,
            width,
            height,
            colors: buf,
        }
    }

    fn next_update_packet(
        &mut self,
        map_id: i32,
        scale: i8,
        locked: bool,
        colors: &[u8; COLORS_LEN],
        decorations: &[(String, MapDecoration)],
    ) -> Option<OwnedMapUpdate> {
        let color_patch = if self.dirty_data {
            self.dirty_data = false;
            Some(self.create_patch(colors))
        } else {
            None
        };
        let decs = if self.dirty_decorations {
            let current = self.tick;
            self.tick = current.wrapping_add(1);
            if current % 5 == 0 {
                self.dirty_decorations = false;
                Some(decorations.iter().map(|(_, d)| d.clone()).collect())
            } else {
                None
            }
        } else {
            None
        };
        if decs.is_some() || color_patch.is_some() {
            Some(OwnedMapUpdate {
                map_id,
                scale,
                locked,
                decorations: decs,
                color_patch,
            })
        } else {
            None
        }
    }
}

/// 一张地图的完整服务端数据，对应官方 `MapItemSavedData`。
///
/// Full server-side state for one map, corresponding to vanilla `MapItemSavedData`.
#[derive(Debug, Clone)]
pub struct MapItemSavedData {
    pub center_x: i32,
    pub center_z: i32,
    pub dimension: String,
    pub tracking_position: bool,
    pub unlimited_tracking: bool,
    pub scale: i8,
    pub colors: [u8; COLORS_LEN],
    pub locked: bool,
    pub dirty: bool,
    pub(crate) carried_by: Vec<HoldingPlayer>,
    pub(crate) banner_markers: BTreeMap<String, MapBanner>,
    /// 保持插入顺序，与官方 `LinkedHashMap<String, MapDecoration>` 一致。
    /// Preserves insertion order, matching vanilla's `LinkedHashMap`.
    pub(crate) decorations: Vec<(String, MapDecoration)>,
    pub(crate) frame_markers: BTreeMap<String, MapFrame>,
    pub(crate) tracked_decoration_count: i32,
}

impl MapItemSavedData {
    fn new_raw(
        center_x: i32,
        center_z: i32,
        scale: i8,
        tracking_position: bool,
        unlimited_tracking: bool,
        locked: bool,
        dimension: String,
    ) -> Self {
        Self {
            center_x,
            center_z,
            dimension,
            tracking_position,
            unlimited_tracking,
            scale,
            colors: [0u8; COLORS_LEN],
            locked,
            dirty: true,
            carried_by: Vec::new(),
            banner_markers: BTreeMap::new(),
            decorations: Vec::new(),
            frame_markers: BTreeMap::new(),
            tracked_decoration_count: 0,
        }
    }

    /// 官方 `MapItemSavedData.createFresh(x, z, scale, trackingPosition, unlimitedTracking, dimension)`。
    ///
    /// Official `MapItemSavedData.createFresh`: snaps `(x, z)` onto the scale grid.
    pub fn create_fresh(
        x: f64,
        z: f64,
        scale: i8,
        tracking_position: bool,
        unlimited_tracking: bool,
        dimension: impl Into<String>,
    ) -> Self {
        let scale = scale.clamp(0, MAX_SCALE);
        let size = 128 * (1 << scale);
        let grid_x = ((x + 64.0) / f64::from(size)).floor() as i32;
        let grid_z = ((z + 64.0) / f64::from(size)).floor() as i32;
        let center_x = grid_x * size + size / 2 - 64;
        let center_z = grid_z * size + size / 2 - 64;
        Self::new_raw(
            center_x,
            center_z,
            scale,
            tracking_position,
            unlimited_tracking,
            false,
            dimension.into(),
        )
    }

    /// 官方 `MapItemSavedData.createForClient(scale, locked, dimension)`。
    pub fn create_for_client(scale: i8, locked: bool, dimension: impl Into<String>) -> Self {
        Self::new_raw(
            0,
            0,
            scale.clamp(0, MAX_SCALE),
            false,
            false,
            locked,
            dimension.into(),
        )
    }

    /// 官方 `MapItemSavedData.scaled()`：比例 +1（上限 4），重新对齐网格并清空颜色。
    ///
    /// Official `MapItemSavedData.scaled()`: increment scale (capped at 4) on a fresh grid.
    pub fn scaled(&self) -> Self {
        Self::create_fresh(
            f64::from(self.center_x),
            f64::from(self.center_z),
            (self.scale + 1).clamp(0, MAX_SCALE),
            self.tracking_position,
            self.unlimited_tracking,
            self.dimension.clone(),
        )
    }

    /// 官方 `MapItemSavedData.locked()`：复制像素、旗帜与标记，设 `locked = true`，
    /// 不复制 `frameMarkers` 与 `carriedBy`。
    ///
    /// Official `MapItemSavedData.locked()`: copy colors, banners and decorations
    /// with `locked = true`; `frameMarkers` and `carriedBy` are left empty.
    pub fn locked(&self) -> Self {
        let mut copy = Self::new_raw(
            self.center_x,
            self.center_z,
            self.scale,
            self.tracking_position,
            self.unlimited_tracking,
            true,
            self.dimension.clone(),
        );
        copy.banner_markers = self.banner_markers.clone();
        copy.decorations = self.decorations.clone();
        copy.tracked_decoration_count = self.tracked_decoration_count;
        copy.colors = self.colors;
        copy
    }

    /// 官方 `MapItemSavedData.updateColor(x, y, color)`：值变化时写入并标脏，返回是否改变。
    pub fn update_color(&mut self, x: usize, y: usize, color: u8) -> bool {
        let idx = x + y * MAP_SIZE;
        if self.colors[idx] != color {
            self.set_color(x, y, color);
            true
        } else {
            false
        }
    }

    /// 官方 `MapItemSavedData.setColor(x, y, color)`。
    pub fn set_color(&mut self, x: usize, y: usize, color: u8) {
        self.colors[x + y * MAP_SIZE] = color;
        self.set_colors_dirty(x as u8, y as u8);
    }

    fn set_colors_dirty(&mut self, x: u8, y: u8) {
        self.dirty = true;
        for holder in &mut self.carried_by {
            holder.mark_colors_dirty(x, y);
        }
    }

    fn set_decorations_dirty(&mut self) {
        for holder in &mut self.carried_by {
            holder.mark_decorations_dirty();
        }
    }

    /// 读取玩家当前的采样步数（未登记时为 0）。
    ///
    /// Current sampling step for this player (0 if not yet tracked).
    pub(crate) fn holding_player_step(&self, player: PlayerId) -> i32 {
        self.carried_by
            .iter()
            .find(|h| h.player == player)
            .map_or(0, |h| h.step)
    }

    /// 官方 `MapItemSavedData.getHoldingPlayer(player)`。
    pub fn holding_player_mut(&mut self, player: PlayerId) -> &mut HoldingPlayer {
        if let Some(pos) = self.carried_by.iter().position(|h| h.player == player) {
            return &mut self.carried_by[pos];
        }
        self.carried_by.push(HoldingPlayer::new(player));
        self.carried_by.last_mut().expect("just pushed")
    }

    /// 确保玩家已在 `carried_by` 中。
    pub fn ensure_holding_player(&mut self, player: PlayerId) {
        if !self.carried_by.iter().any(|h| h.player == player) {
            self.carried_by.push(HoldingPlayer::new(player));
        }
    }

    /// 官方 `MapItemSavedData.getUpdatePacket(mapId, player)`。
    pub fn get_update_packet(&mut self, map_id: i32, player: PlayerId) -> Option<OwnedMapUpdate> {
        let pos = self.carried_by.iter().position(|h| h.player == player)?;
        let scale = self.scale;
        let locked = self.locked;
        self.carried_by[pos].next_update_packet(
            map_id,
            scale,
            locked,
            &self.colors,
            &self.decorations,
        )
    }

    /// 官方 `MapItemSavedData.isTrackedCountOverLimit(limit)`：`trackedDecorationCount > limit`。
    pub fn is_tracked_count_over_limit(&self, limit: i32) -> bool {
        self.tracked_decoration_count > limit
    }

    /// 官方 `MapItemSavedData.removedFromFrame(pos, entityId)`。
    pub fn removed_from_frame(&mut self, pos: BlockPos, entity_id: i32) {
        self.remove_decoration(&frame_decoration_key(entity_id));
        self.frame_markers.remove(&MapFrame::frame_id(pos));
        self.dirty = true;
    }

    /// 当前所有标记（按插入顺序）。
    pub fn decorations(&self) -> impl Iterator<Item = &MapDecoration> {
        self.decorations.iter().map(|(_, d)| d)
    }

    /// 当前展示框标记列表。
    pub fn frame_markers(&self) -> &BTreeMap<String, MapFrame> {
        &self.frame_markers
    }

    /// 当前旗帜标记列表（官方 `MapItemSavedData.getBanners()`）。
    pub fn banner_markers(&self) -> &BTreeMap<String, MapBanner> {
        &self.banner_markers
    }

    /// 官方 `MapItemSavedData.toggleBanner(LevelAccessor, BlockPos)`：
    /// 在地图范围内对同一位置、颜色、自定义名的旗帜做开关切换。
    ///
    /// 注意照 26.3 字节码：此处以 `(pos.x + 0.5, pos.z + 0.5)` 计算标记坐标（与读档构造不同），
    /// 且不检查 `self.locked`。
    ///
    /// Official `MapItemSavedData.toggleBanner`: add or remove a banner marker
    /// within map bounds. Uses `(pos.x + 0.5, pos.z + 0.5)` and does not check
    /// `self.locked`, matching 26.3 bytecode.
    pub fn toggle_banner(
        &mut self,
        pos: BlockPos,
        banner: Option<MapBanner>,
        game_time: Option<i64>,
    ) -> bool {
        let d_x = f64::from(pos.x) + 0.5;
        let d_z = f64::from(pos.z) + 0.5;
        let scale_factor = 1 << self.scale;
        let rel_x = (d_x - f64::from(self.center_x)) / f64::from(scale_factor);
        let rel_z = (d_z - f64::from(self.center_z)) / f64::from(scale_factor);
        if !(-63.0..=63.0).contains(&rel_x) || !(-63.0..=63.0).contains(&rel_z) {
            return false;
        }
        let Some(banner) = banner else {
            return false;
        };
        let id = banner.id();
        if self.banner_markers.get(&id) == Some(&banner) {
            self.banner_markers.remove(&id);
            self.remove_decoration(&id);
            self.dirty = true;
            return true;
        }
        if !self.is_tracked_count_over_limit(TRACKED_DECORATION_LIMIT) {
            let kind = banner.decoration_kind();
            let name = banner.name.clone();
            self.banner_markers.insert(id.clone(), banner);
            self.add_decoration(kind, game_time, id, d_x, d_z, 180.0, name);
            self.dirty = true;
            return true;
        }
        false
    }

    /// 官方 `MapItemSavedData.checkBanners(BlockGetter, int, int)`：
    /// 核对落在 `(x, z)` 柱上的旗帜标记；若世界里的旗帜已被破坏或颜色/名称改变则移除标记。
    ///
    /// Official `MapItemSavedData.checkBanners`: verify banner markers at column
    /// `(x, z)` and remove any whose block no longer matches.
    pub fn check_banners(
        &mut self,
        x: i32,
        z: i32,
        mut from_world: impl FnMut(BlockPos) -> Option<MapBanner>,
    ) {
        if self.banner_markers.is_empty() {
            return;
        }
        let stale: Vec<String> = self
            .banner_markers
            .iter()
            .filter_map(|(id, banner)| {
                if banner.pos.x == x
                    && banner.pos.z == z
                    && from_world(banner.pos).as_ref() != Some(banner)
                {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();
        for id in stale {
            self.banner_markers.remove(&id);
            self.remove_decoration(&id);
            self.dirty = true;
        }
    }

    /// 官方 `MapItemSavedData.removeDecoration(id)`。
    pub fn remove_decoration(&mut self, key: &str) {
        if let Some(pos) = self.decorations.iter().position(|(k, _)| k == key) {
            let (_, removed) = self.decorations.remove(pos);
            if decoration_tracks_count(removed.kind) {
                self.tracked_decoration_count -= 1;
            }
        }
        self.set_decorations_dirty();
    }

    /// 官方 `MapItemSavedData.addDecoration(...)`。
    #[allow(clippy::too_many_arguments)]
    pub fn add_decoration(
        &mut self,
        kind: i32,
        game_time: Option<i64>,
        key: String,
        world_x: f64,
        world_z: f64,
        rot: f64,
        name: Option<Component>,
    ) {
        let scale_factor = 1 << self.scale;
        let dx = (world_x - f64::from(self.center_x)) as f32 / scale_factor as f32;
        let dz = (world_z - f64::from(self.center_z)) as f32 / scale_factor as f32;
        let Some((resolved_kind, map_x, map_y, map_rot)) =
            self.calculate_decoration_location_and_type(kind, game_time, rot, dx, dz)
        else {
            self.remove_decoration(&key);
            return;
        };
        let next = MapDecoration {
            kind: resolved_kind,
            x: map_x,
            y: map_y,
            rot: map_rot & 15,
            name,
        };
        let prev = if let Some((_, existing)) = self.decorations.iter_mut().find(|(k, _)| k == &key)
        {
            Some(std::mem::replace(existing, next.clone()))
        } else {
            self.decorations.push((key, next.clone()));
            None
        };
        if prev.as_ref() != Some(&next) {
            if let Some(old) = prev
                && decoration_tracks_count(old.kind)
            {
                self.tracked_decoration_count -= 1;
            }
            if decoration_tracks_count(next.kind) {
                self.tracked_decoration_count += 1;
            }
            self.set_decorations_dirty();
        }
    }

    fn calculate_decoration_location_and_type(
        &self,
        kind: i32,
        game_time: Option<i64>,
        rot: f64,
        dx: f32,
        dz: f32,
    ) -> Option<(i32, i8, i8, i8)> {
        let map_x = clamp_map_coordinate(dx);
        let map_y = clamp_map_coordinate(dz);
        if kind == decoration_type::PLAYER {
            let (player_kind, player_rot) = if is_inside_map(dx, dz) {
                (kind, self.calculate_rotation(game_time, rot))
            } else {
                let outside = self.decoration_type_for_player_outside_map(dx, dz)?;
                (outside, self.calculate_rotation(game_time, rot))
            };
            return Some((player_kind, map_x, map_y, player_rot));
        }
        if is_inside_map(dx, dz) || self.unlimited_tracking {
            Some((kind, map_x, map_y, self.calculate_rotation(game_time, rot)))
        } else {
            None
        }
    }

    fn decoration_type_for_player_outside_map(&self, dx: f32, dz: f32) -> Option<i32> {
        if dx.abs() < 320.0 && dz.abs() < 320.0 {
            Some(decoration_type::PLAYER_OFF_MAP)
        } else if self.unlimited_tracking {
            Some(decoration_type::PLAYER_OFF_LIMITS)
        } else {
            None
        }
    }

    fn calculate_rotation(&self, game_time: Option<i64>, rot: f64) -> i8 {
        if self.dimension == "minecraft:the_nether"
            && let Some(time) = game_time
        {
            let i = (time / 10) as i32;
            let hash = i
                .wrapping_mul(i)
                .wrapping_mul(34_187_121)
                .wrapping_add(i.wrapping_mul(121));
            return ((hash >> 15) & 15) as i8;
        }
        let adjusted = if rot < 0.0 { rot - 8.0 } else { rot + 8.0 };
        ((adjusted * 16.0 / 360.0) as i32) as i8
    }

    /// 编成官方 `MapItemSavedData.CODEC` 的 `data` 复合标签。
    ///
    /// Encode to the official `MapItemSavedData.CODEC` compound tag.
    pub fn to_tag(&self) -> Tag {
        let colors = self.colors.iter().map(|&b| b as i8).collect();
        let banners = self
            .banner_markers
            .values()
            .map(MapBanner::to_tag)
            .collect();
        let frames = self.frame_markers.values().map(MapFrame::to_tag).collect();
        Tag::Compound(vec![
            ("dimension".to_owned(), Tag::String(self.dimension.clone())),
            ("xCenter".to_owned(), Tag::Int(self.center_x)),
            ("zCenter".to_owned(), Tag::Int(self.center_z)),
            ("scale".to_owned(), Tag::Byte(self.scale)),
            ("colors".to_owned(), Tag::ByteArray(colors)),
            (
                "trackingPosition".to_owned(),
                Tag::Byte(i8::from(self.tracking_position)),
            ),
            (
                "unlimitedTracking".to_owned(),
                Tag::Byte(i8::from(self.unlimited_tracking)),
            ),
            ("locked".to_owned(), Tag::Byte(i8::from(self.locked))),
            ("banners".to_owned(), Tag::List(banners)),
            ("frames".to_owned(), Tag::List(frames)),
        ])
    }

    /// 从官方 `MapItemSavedData.CODEC` 的 `data` 复合标签解码。
    ///
    /// Decode from the official `MapItemSavedData.CODEC` compound tag.
    pub fn from_tag(tag: &Tag) -> Option<Self> {
        let dimension = get_string(tag, "dimension")?.to_owned();
        let center_x = get_int(tag, "xCenter")?;
        let center_z = get_int(tag, "zCenter")?;
        let scale = match get(tag, "scale") {
            Some(Tag::Byte(v)) => (*v).clamp(0, MAX_SCALE),
            Some(_) => return None,
            None => 0,
        };
        let colors_slice = match get(tag, "colors") {
            Some(Tag::ByteArray(bytes)) => bytes,
            _ => return None,
        };
        let tracking_position = match get(tag, "trackingPosition") {
            Some(Tag::Byte(v)) => *v != 0,
            Some(_) => return None,
            None => true,
        };
        let unlimited_tracking = match get(tag, "unlimitedTracking") {
            Some(Tag::Byte(v)) => *v != 0,
            Some(_) => return None,
            None => false,
        };
        let locked = match get(tag, "locked") {
            Some(Tag::Byte(v)) => *v != 0,
            Some(_) => return None,
            None => false,
        };
        let banners = match get(tag, "banners") {
            Some(Tag::List(list)) => {
                let mut parsed = Vec::with_capacity(list.len());
                for item in list {
                    parsed.push(MapBanner::from_tag(item)?);
                }
                parsed
            }
            Some(_) => return None,
            None => Vec::new(),
        };
        let frames = match get(tag, "frames") {
            Some(Tag::List(list)) => {
                let mut parsed = Vec::with_capacity(list.len());
                for item in list {
                    parsed.push(MapFrame::from_tag(item)?);
                }
                parsed
            }
            Some(_) => return None,
            None => Vec::new(),
        };

        let mut data = Self::new_raw(
            center_x,
            center_z,
            scale,
            tracking_position,
            unlimited_tracking,
            locked,
            dimension,
        );
        data.dirty = false;
        if colors_slice.len() == COLORS_LEN {
            for (dst, &src) in data.colors.iter_mut().zip(colors_slice.iter()) {
                *dst = src as u8;
            }
        }
        for banner in banners {
            let id = banner.id();
            let kind = banner.decoration_kind();
            let bx = f64::from(banner.pos.x);
            let bz = f64::from(banner.pos.z);
            let name = banner.name.clone();
            data.banner_markers.insert(id.clone(), banner);
            data.add_decoration(kind, None, id, bx, bz, 180.0, name);
        }
        for frame in frames {
            let id = frame.id();
            let key = frame_decoration_key(frame.entity_id);
            let fx = f64::from(frame.pos.x);
            let fz = f64::from(frame.pos.z);
            let rot = f64::from(frame.rotation);
            data.frame_markers.insert(id, frame);
            data.add_decoration(decoration_type::FRAME, None, key, fx, fz, rot, None);
        }
        data.dirty = false;
        Some(data)
    }

    /// 官方 `MapItemSavedData.tickCarriedBy(player, stack, null)` 的服务端等价：
    /// 登记持有者，并在开启位置追踪、同维度时刷新玩家标记。
    ///
    /// Server-side equivalent of `tickCarriedBy(player, stack, null)`: register
    /// the holder and refresh the player marker when position tracking is on.
    #[allow(clippy::too_many_arguments)]
    pub fn tick_carried_by_player(
        &mut self,
        player: PlayerId,
        name: &str,
        x: f64,
        z: f64,
        rot: f64,
        game_time: i64,
        dimension: &str,
    ) {
        self.ensure_holding_player(player);
        if self.tracking_position && dimension == self.dimension {
            self.add_decoration(
                decoration_type::PLAYER,
                Some(game_time),
                name.to_owned(),
                x,
                z,
                rot,
                None,
            );
        }
    }

    /// 官方 `tickCarriedBy` 的物品展示框分支：仅在 `tracking_position` 开启时登记 / 刷新展示框标记。
    ///
    /// Item-frame branch of `tickCarriedBy`: register / refresh the frame marker
    /// when `tracking_position` is enabled.
    pub fn set_frame_marker(
        &mut self,
        pos: BlockPos,
        rotation: i32,
        entity_id: i32,
        game_time: Option<i64>,
    ) {
        if !self.tracking_position {
            return;
        }
        let id = MapFrame::frame_id(pos);
        if let Some(existing) = self.frame_markers.get(&id)
            && existing.entity_id != entity_id
        {
            let old_key = frame_decoration_key(existing.entity_id);
            self.remove_decoration(&old_key);
        }
        let key = frame_decoration_key(entity_id);
        self.add_decoration(
            decoration_type::FRAME,
            game_time,
            key,
            f64::from(pos.x),
            f64::from(pos.z),
            f64::from(rotation),
            None,
        );
        let frame = MapFrame {
            pos,
            rotation,
            entity_id,
        };
        let previous = self.frame_markers.insert(id, frame.clone());
        if previous.as_ref() != Some(&frame) {
            self.dirty = true;
        }
    }

    /// 去掉已加载区块内不再存在的展示框标记（框被移除，或框内不再是这张地图）。
    /// 未加载区块里的展示框标记照官方保留。
    ///
    /// Drop frame markers in loaded chunks whose entity is no longer framing this
    /// map, preserving markers in unloaded chunks as vanilla does.
    pub fn reconcile_frame_markers(&mut self, live: &BTreeSet<i32>, chunks: &ChunkStore) {
        let stale: Vec<(BlockPos, i32)> = self
            .frame_markers
            .values()
            .filter(|f| {
                chunks.is_loaded(ChunkPos::new(f.pos.x >> 4, f.pos.z >> 4))
                    && !live.contains(&f.entity_id)
            })
            .map(|f| (f.pos, f.entity_id))
            .collect();
        for (pos, entity_id) in stale {
            self.removed_from_frame(pos, entity_id);
        }
    }

    /// 清掉不再持有 / 观察这张地图的玩家，顺带去掉他们的玩家标记。
    ///
    /// Drop holders that no longer carry or view this map, removing their markers.
    pub fn retain_carried_by(
        &mut self,
        keep: &BTreeSet<PlayerId>,
        name_of: &BTreeMap<PlayerId, String>,
    ) {
        let mut removed = Vec::new();
        self.carried_by.retain(|holder| {
            if keep.contains(&holder.player) {
                true
            } else {
                removed.push(holder.player);
                false
            }
        });
        for player in removed {
            if let Some(name) = name_of.get(&player) {
                self.remove_decoration(name);
            }
        }
    }
}

fn is_inside_map(dx: f32, dz: f32) -> bool {
    (-63.0..=63.0).contains(&dx) && (-63.0..=63.0).contains(&dz)
}

fn clamp_map_coordinate(value: f32) -> i8 {
    if value <= -63.0 {
        -128
    } else if value >= 63.0 {
        127
    } else {
        ((f64::from(value * 2.0) + 0.5) as i32) as i8
    }
}

/// 官方 `MapIndex`：全局递增的地图 ID 分配器，存在 `data/minecraft/maps/last_id.dat`。
///
/// Official `MapIndex`: monotonically increasing map ID allocator stored in
/// `data/minecraft/maps/last_id.dat`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapIndex {
    pub last_map_id: i32,
    pub dirty: bool,
}

impl Default for MapIndex {
    fn default() -> Self {
        Self {
            last_map_id: NO_MAP_ID,
            dirty: false,
        }
    }
}

impl MapIndex {
    pub fn new(last_map_id: i32) -> Self {
        Self {
            last_map_id,
            dirty: false,
        }
    }

    /// 官方 `MapIndex.getNextMapId()`：`++lastMapId` 并标脏。
    pub fn next_map_id(&mut self) -> i32 {
        self.last_map_id = self.last_map_id.wrapping_add(1);
        self.dirty = true;
        self.last_map_id
    }

    /// 编成官方 `MapIndex.CODEC` 的 `data` 复合标签。
    pub fn to_tag(&self) -> Tag {
        Tag::Compound(vec![("map".to_owned(), Tag::Int(self.last_map_id))])
    }

    /// 从官方 `MapIndex.CODEC` 的 `data` 复合标签解码（缺字段用 `-1`）。
    pub fn from_tag(tag: &Tag) -> Self {
        let last_map_id = get_int(tag, "map").unwrap_or(NO_MAP_ID);
        Self {
            last_map_id,
            dirty: false,
        }
    }
}

/// 地图索引与所有已加载地图的集合（全服一份，存在 [`Global`] 上，每张地图按 `Arc` 独立写时复制）。
///
/// Server-wide container holding [`MapIndex`] and all loaded [`MapItemSavedData`]
/// entries (lives on [`Global`], with per-map copy-on-write via `Arc`).
#[derive(Debug, Clone, Default)]
pub struct MapStore {
    pub index: MapIndex,
    pub maps: BTreeMap<i32, Arc<MapItemSavedData>>,
}

impl MapStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.maps.is_empty()
    }

    pub fn get(&self, id: i32) -> Option<&MapItemSavedData> {
        self.maps.get(&id).map(Arc::as_ref)
    }

    pub fn get_mut(&mut self, id: i32) -> Option<&mut MapItemSavedData> {
        self.maps.get_mut(&id).map(Arc::make_mut)
    }

    /// 分配一个新地图 ID（`ServerLevel.getFreeMapId`）。
    pub fn next_map_id(&mut self) -> i32 {
        self.index.next_map_id()
    }

    /// 写入或替换一份地图数据（`ServerLevel.setMapData`）。
    pub fn insert(&mut self, id: i32, mut data: MapItemSavedData) {
        data.dirty = true;
        self.maps.insert(id, Arc::new(data));
    }

    /// 玩家离线时从所有地图的 `carried_by` 与玩家标记中移除，仅在命中的地图上触发写时复制。
    ///
    /// Remove a departing player from `carried_by` and player decorations across
    /// all maps, touching only maps that reference them.
    pub(crate) fn on_player_leave(&mut self, player: PlayerId, name: Option<&str>) {
        let affected: Vec<i32> = self
            .maps
            .iter()
            .filter_map(|(&id, data)| {
                let has_holder = data.carried_by.iter().any(|h| h.player == player);
                let has_dec = name.is_some_and(|n| data.decorations.iter().any(|(k, _)| k == n));
                (has_holder || has_dec).then_some(id)
            })
            .collect();
        for id in affected {
            if let Some(data) = self.get_mut(id) {
                data.carried_by.retain(|h| h.player != player);
                if let Some(name) = name {
                    data.remove_decoration(name);
                }
            }
        }
    }
}

/// 构造一件带 `minecraft:map_id` 组件的已填充地图（数量 1）。
///
/// Build a single `filled_map` stack carrying `minecraft:map_id`.
pub fn make_filled_map(map_id: i32) -> ItemStack {
    let mut patch = ComponentPatch::new();
    patch.put(
        kinds::MAP_ID,
        PatchEntry::Set(ComponentValue::MapId(map_id)),
    );
    let (added, removed) = patch.to_opaque();
    ItemStack {
        item: items::FILLED_MAP,
        count: 1,
        added,
        removed,
    }
}

/// 读取物品堆上的 `minecraft:map_id` 组件值。
///
/// Read the `minecraft:map_id` component from an item stack, if present.
pub fn stack_map_id(stack: &ItemStack) -> Option<i32> {
    if stack.is_empty() {
        return None;
    }
    let patch = ComponentPatch::from_opaque(stack).ok()?;
    match defaults::resolve(stack.item, &patch, kinds::MAP_ID) {
        Some(ComponentValue::MapId(id)) => Some(*id),
        _ => None,
    }
}

/// 官方 `MapItem.create(level, x, z, scale, trackingPosition, unlimitedTracking)`：
/// 按比例网格对齐中心、分配新 `map_id`、存入地图表并返回带 `map_id` 的 `filled_map`。
///
/// Official `MapItem.create`: snap center onto the scale grid, allocate a new
/// `map_id`, store the fresh `MapItemSavedData`, and return the `filled_map`.
pub fn create_map(
    maps: &mut MapStore,
    x: i32,
    z: i32,
    scale: i8,
    tracking_position: bool,
    unlimited_tracking: bool,
    dimension: &str,
) -> ItemStack {
    let data = MapItemSavedData::create_fresh(
        f64::from(x),
        f64::from(z),
        scale,
        tracking_position,
        unlimited_tracking,
        dimension,
    );
    let id = maps.next_map_id();
    maps.insert(id, data);
    make_filled_map(id)
}

/// 官方 `EmptyMapItem.use`：手持空地图（`minecraft:map`）右键时创建已填充地图。
///
/// 顺序照 26.3 字节码：`consume(1, player)` → `awardStat(ITEM_USED)` → 播
/// `ui.cartography_table.take_result`（音源 `PLAYERS`）→ `MapItem.create` →
/// 手里变空则就地替换为已填充地图，否则 `Inventory.add`（满则 `drop`）。
///
/// Official `EmptyMapItem.use`: consume one empty map (non-creative), award
/// `ITEM_USED`, play `ui.cartography_table.take_result`, create the filled map,
/// and replace the empty hand or add/drop into inventory.
pub(crate) fn use_empty_map(
    region: &mut Region,
    maps: Option<&mut MapStore>,
    id: PlayerId,
    hand: Hand,
) -> bool {
    let Some((held, creative, pos)) = region.players.get(&id).map(|player| {
        (
            player.inventory.held(hand).clone(),
            player.game_mode == GameMode::Creative,
            player.position(),
        )
    }) else {
        return false;
    };
    if held.item != items::MAP {
        return false;
    }
    if region
        .players
        .get(&id)
        .is_some_and(|p| crate::ender_pearl::on_cooldown(p, &held))
    {
        return false;
    }

    // 1. `stack.consume(1, player)`
    let hand_now_empty = if let Some(player) = region.players.get_mut(&id) {
        let slot = player.inventory.hand_slot(hand);
        if !creative {
            let stack = &mut player.inventory.slots[slot];
            stack.count -= 1;
            if stack.count <= 0 {
                *stack = ItemStack::empty();
            }
        }
        player.inventory.slots[slot].is_empty()
    } else {
        return false;
    };

    // 2. `player.awardStat(Stats.ITEM_USED.get(this))`
    crate::item_use_stats::award(region, id, items::MAP);

    // 3. `serverLevel.playSound(null, player, UI_CARTOGRAPHY_TABLE_TAKE_RESULT, player.getSoundSource(), 1.0F, 1.0F)`
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = BlockWorld::new(&mut level, ticks);
        world.play_sound(
            pos,
            sounds::sound_event("minecraft:ui.cartography_table.take_result"),
            sounds::PLAYERS,
            1.0,
            1.0,
            false,
        );
        world.drain_effects()
    };
    crate::interaction::send_effects(region, None, &effects);

    // 4. `MapItem.create(serverLevel, player.getBlockX(), player.getBlockZ(), 0, true, false)`
    let block_x = pos.x.floor() as i32;
    let block_z = pos.z.floor() as i32;
    let store = match maps {
        Some(store) => store,
        None => Arc::make_mut(&mut region.global.maps),
    };
    let filled = create_map(
        store,
        block_x,
        block_z,
        0,
        true,
        false,
        crate::region::DIMENSION,
    );

    // 5. 手里空了就地放进手里，否则进背包 / 丢地上。
    if hand_now_empty {
        if let Some(player) = region.players.get_mut(&id) {
            let slot = player.inventory.hand_slot(hand);
            player.inventory.slots[slot] = filled;
            crate::player_weapon::sync_slots(region, id, [slot]);
        }
        crate::advancement_triggers::inventory_changed(region, id);
    } else {
        // 先同步手里因消耗减少的槽位（生存多于 1 张时）。
        if !creative
            && let Some(slot) = region.players.get(&id).map(|p| p.inventory.hand_slot(hand))
        {
            crate::player_weapon::sync_slots(region, id, [slot]);
        }
        crate::inventory::give_or_drop(region, id, filled);
    }

    if let Some(player) = region.players.get_mut(&id) {
        crate::ender_pearl::apply_cooldown(player, &held);
    }
    true
}

/// 官方 `MapItem.onCraftedPostProcess`：移除 `MAP_POST_PROCESSING` 组件，
/// 并在服务端地图表里按 `LOCK`（0）/ `SCALE`（1）分配新 `map_id` 与对应的锁定 / 放大数据。
///
/// Official `MapItem.onCraftedPostProcess`: strip `MAP_POST_PROCESSING` and,
/// using the server [`MapStore`], allocate a new `map_id` with `locked()` or
/// `scaled()` data.
pub fn on_crafted_post_process(stack: &mut ItemStack, maps: &mut MapStore) {
    if stack.is_empty() || stack.item != items::FILLED_MAP {
        return;
    }
    let Ok(mut patch) = ComponentPatch::from_opaque(stack) else {
        return;
    };
    let Some(ComponentValue::MapPostProcessing(kind)) =
        defaults::resolve(stack.item, &patch, kinds::MAP_POST_PROCESSING).cloned()
    else {
        return;
    };
    if defaults::of_item(stack.item)
        .get(kinds::MAP_POST_PROCESSING)
        .is_some()
    {
        patch.put(kinds::MAP_POST_PROCESSING, PatchEntry::Removed);
    } else {
        patch.remove(kinds::MAP_POST_PROCESSING);
    }
    let (added, removed) = patch.to_opaque();
    stack.added = added;
    stack.removed = removed;
    let Some(old_id) = stack_map_id(stack) else {
        return;
    };
    let next_data = match kind {
        0 => maps.get(old_id).map(MapItemSavedData::locked),
        1 => maps.get(old_id).map(MapItemSavedData::scaled),
        _ => None,
    };
    if let Some(data) = next_data {
        let new_id = maps.next_map_id();
        maps.insert(new_id, data);
        if defaults::of_item(stack.item).get(kinds::MAP_ID) == Some(&ComponentValue::MapId(new_id))
        {
            patch.remove(kinds::MAP_ID);
        } else {
            patch.put(
                kinds::MAP_ID,
                PatchEntry::Set(ComponentValue::MapId(new_id)),
            );
        }
        let (added, removed) = patch.to_opaque();
        stack.added = added;
        stack.removed = removed;
    }
}

/// 官方 `MapItem.useOn(UseOnContext)`：手持已填充地图右键旗帜时切换旗帜标记。
///
/// 若目标方块不是 `#minecraft:banners` 返回 `None`；是旗帜时照 26.3 字节码：
/// 找不到地图数据也返回 `Success`，找到数据时 `toggleBanner` 返回 `false` 才返回 `Fail`。
///
/// Official `MapItem.useOn`: toggle a banner marker when right-clicking a block
/// in `#minecraft:banners` with a `filled_map`. Returns `None` for non-banners.
pub(crate) fn use_map_on_banner(
    region: &mut Region,
    maps: Option<&mut MapStore>,
    pos: BlockPos,
    held: &ItemStack,
) -> Option<InteractionResult> {
    if held.item != items::FILLED_MAP {
        return None;
    }
    let state = region.chunks.block(pos)?;
    if !BlockState(state).is_tagged(block_tags::BANNERS) {
        return None;
    }
    let banner = banner_from_world(&region.chunks, &region.block_items, pos);
    let game_time = region.tick.game_time();
    let store = match maps {
        Some(store) => store,
        None => Arc::make_mut(&mut region.global.maps),
    };
    if let Some(map_id) = stack_map_id(held)
        && let Some(data) = store.get_mut(map_id)
        && !data.toggle_banner(pos, banner, Some(game_time))
    {
        return Some(InteractionResult::Fail);
    }
    Some(InteractionResult::Success)
}

/// 官方 `MapItem.getCorrectStateForFluidBlock`：含流体且顶面不是完整支撑面时改用流体方块本身的颜色。
///
/// Official `MapItem.getCorrectStateForFluidBlock`.
#[inline]
fn correct_state_for_fluid_block(state: u32) -> u32 {
    let fluid = state_fluid(state);
    if fluid.fluid != fluid_reg::EMPTY
        && !BlockState(state).is_face_sturdy(Direction::Up, SupportType::Full)
    {
        fluid.legacy_block
    } else {
        state
    }
}

/// 官方 `MapItem.update(level, entity, data)` 的逐列采样算法（主世界 `hasCeiling = false`）。
///
/// 每次调用使该玩家的 `HoldingPlayer.step` 加 1，只更新 `(x & 15) == (step & 15)` 或
/// 上一列刚发生颜色变化的列；逐列算 `scale × scale` 区域内方块众数（平票取最先出现）
/// 与水深 / 坡度亮度，并写回脏颜色。
///
/// Official `MapItem.update` column-sampling algorithm for non-ceiling dimensions.
pub fn update_colors(
    data: &mut MapItemSavedData,
    player: PlayerId,
    player_x: f64,
    player_z: f64,
    dimension: &str,
    chunks: &ChunkStore,
) {
    update_colors_with_banner_lookup(data, player, player_x, player_z, dimension, chunks, |_| {
        None
    });
}

/// 同 [`update_colors`]，并在官方 `MapItem.update` 对应位置调用 `checkBanners`。
///
/// Like [`update_colors`], also running `checkBanners` at the exact column-scan
/// point from vanilla `MapItem.update`.
pub fn update_colors_with_banner_lookup(
    data: &mut MapItemSavedData,
    player: PlayerId,
    player_x: f64,
    player_z: f64,
    dimension: &str,
    chunks: &ChunkStore,
    mut banner_at: impl FnMut(BlockPos) -> Option<MapBanner>,
) {
    if dimension != data.dimension {
        return;
    }
    let step = {
        let holder = data.holding_player_mut(player);
        holder.step = holder.step.wrapping_add(1);
        holder.step
    };
    let center_x = data.center_x;
    let center_z = data.center_z;
    let scale_shift = data.scale;
    sample_colors_with_step(
        center_x,
        center_z,
        scale_shift,
        step,
        player_x,
        player_z,
        chunks,
        data,
        |data, bx, bz| data.check_banners(bx, bz, &mut banner_at),
        |data, x, z, color| data.update_color(x, z, color),
    );
}

/// 按给定采样步数 `step` 对一块区域的地图颜色逐列采样，变化像素通过 `write_pixel` 写出，
/// 并在每个已采样的 `(bx, bz)` 上调用 `on_column`（对应官方 `MapItem.update` 中的 `checkBanners`）。
///
/// Sample map columns for a given `step`, reporting each pixel to `write_pixel`
/// (which returns whether the pixel color changed) and invoking `on_column` at
/// each sampled `(bx, bz)` (matching vanilla `MapItem.update`'s `checkBanners`).
#[allow(clippy::too_many_arguments)]
fn sample_colors_with_step<S>(
    center_x: i32,
    center_z: i32,
    scale_shift: i8,
    step: i32,
    player_x: f64,
    player_z: f64,
    chunks: &ChunkStore,
    state_ctx: &mut S,
    mut on_column: impl FnMut(&mut S, i32, i32),
    mut write_pixel: impl FnMut(&mut S, usize, usize, u8) -> bool,
) {
    let scale: i32 = 1 << scale_shift;
    // Java `Mth.floor(x - centerX) / scale + 64`：整数除法向零截断，与 Rust `/` 一致。
    let px = (player_x - f64::from(center_x)).floor() as i32 / scale + HALF_MAP_SIZE;
    let pz = (player_z - f64::from(center_z)).floor() as i32 / scale + HALF_MAP_SIZE;
    let radius = 128 / scale;

    let mut changed = false;
    let scale_sq = scale * scale;
    let edge_limit = (radius - 2) * (radius - 2);
    let radius_sq = radius * radius;

    for x in (px - radius + 1)..(px + radius) {
        if (x & 15) != (step & 15) && !changed {
            continue;
        }
        changed = false;
        let mut prev_height: f64 = 0.0;

        for z in (pz - radius - 1)..(pz + radius) {
            if !(0..128).contains(&x) || !(-1..128).contains(&z) {
                continue;
            }
            let dx_p = x - px;
            let dz_p = z - pz;
            let dist_sq = dx_p * dx_p + dz_p * dz_p;
            let dither_edge = dist_sq > edge_limit;

            let world_x = (center_x / scale + x - HALF_MAP_SIZE) * scale;
            let world_z = (center_z / scale + z - HALF_MAP_SIZE) * scale;
            let chunk_pos = ChunkPos::new(world_x >> 4, world_z >> 4);
            if !chunks.is_loaded(chunk_pos) {
                continue;
            }

            let mut water_depth: i32 = 0;
            let mut avg_height: f64 = 0.0;
            // 复现 Guava `LinkedHashMultiset` + `Multisets.copyHighestCountFirst`：
            // 记下每种 MapColor（0..=63）的频次和首次出现顺序，平票取最先出现的颜色。
            let mut counts = [0u32; 64];
            let mut order = [0u8; 64];
            let mut order_len = 0usize;

            for ox in 0..scale {
                for oz in 0..scale {
                    let bx = world_x + ox;
                    let bz = world_z + oz;
                    let Some(mut y) = chunks.world_surface_y(BlockPos::new(bx, MIN_Y, bz)) else {
                        continue;
                    };
                    let mut state: u32;
                    if y > MIN_Y {
                        loop {
                            y -= 1;
                            state = chunks
                                .block(BlockPos::new(bx, y, bz))
                                .unwrap_or(blocks::AIR);
                            if state_map_color(state) != map_color::NONE || y <= MIN_Y {
                                break;
                            }
                        }
                        if y > MIN_Y && state_fluid(state).fluid != fluid_reg::EMPTY {
                            let mut y2 = y - 1;
                            loop {
                                let state2 = chunks
                                    .block(BlockPos::new(bx, y2, bz))
                                    .unwrap_or(blocks::AIR);
                                y2 -= 1;
                                water_depth += 1;
                                if y2 <= MIN_Y || state_fluid(state2).fluid == fluid_reg::EMPTY {
                                    break;
                                }
                            }
                            state = correct_state_for_fluid_block(state);
                        }
                    } else {
                        state = blocks::BEDROCK;
                    }

                    on_column(state_ctx, bx, bz);
                    avg_height += f64::from(y) / f64::from(scale_sq);
                    let color_id = state_map_color(state) & 63;
                    let idx = usize::from(color_id);
                    if counts[idx] == 0 {
                        order[order_len] = color_id;
                        order_len += 1;
                    }
                    counts[idx] += 1;
                }
            }

            water_depth /= scale_sq;
            let mut best_color = map_color::NONE;
            let mut best_count = 0u32;
            for &color_id in &order[..order_len] {
                let cnt = counts[usize::from(color_id)];
                if cnt > best_count {
                    best_count = cnt;
                    best_color = color_id;
                }
            }

            let brightness = if best_color == map_color::WATER {
                let d = f64::from(water_depth) * 0.1 + f64::from((x + z) & 1) * 0.2;
                if d < 0.5 {
                    Brightness::High
                } else if d > 0.9 {
                    Brightness::Low
                } else {
                    Brightness::Normal
                }
            } else {
                let d = (avg_height - prev_height) * 4.0 / f64::from(scale + 4)
                    + (f64::from((x + z) & 1) - 0.5) * 0.4;
                if d > 0.6 {
                    Brightness::High
                } else if d < -0.6 {
                    Brightness::Low
                } else {
                    Brightness::Normal
                }
            };
            prev_height = avg_height;

            if z < 0 || dist_sq >= radius_sq || (dither_edge && ((x + z) & 1) == 0) {
                continue;
            }
            changed |= write_pixel(
                state_ctx,
                x as usize,
                z as usize,
                packed_color(best_color, brightness),
            );
        }
    }
}

/// 玩家物品栏是否持有指定 `map_id` 的已填充地图（对应官方 `MapItemSavedData.mapMatcher`）。
fn player_holds_map(player: &crate::player::Player, map_id: i32) -> bool {
    player
        .inventory
        .slots
        .iter()
        .skip(5)
        .any(|stack| stack.item == items::FILLED_MAP && stack_map_id(stack) == Some(map_id))
}

/// 一名玩家在本刻持有的地图快照，作为 [`MapTickEffect`] 的一部分交回主控。
///
/// One player's carried-map snapshot handed to the master inside [`MapTickEffect`].
pub(crate) struct PlayerMapTick {
    pub(crate) id: PlayerId,
    pub(crate) name: String,
    pub(crate) x: f64,
    pub(crate) z: f64,
    pub(crate) yaw: f64,
    pub(crate) maps: Vec<i32>,
}

/// 一张地图在本刻采样出的像素颜色增量。
///
/// Per-map pixel color changes sampled during one island tick.
pub(crate) struct MapColorDelta {
    pub(crate) map_id: i32,
    pub(crate) pixels: Vec<(u8, u8, u8)>,
}

/// 岛在一刻里算好的地图同步增量，经 [`GlobalPort`] 交给主控写权威 [`MapStore`] 并发包。
///
/// Per-tick map sync delta computed on an island and applied to the authoritative
/// [`MapStore`] on the master through [`GlobalPort`].
pub(crate) struct MapTickEffect {
    pub(crate) game_time: i64,
    pub(crate) dimension: &'static str,
    pub(crate) stale_frames: Vec<(i32, BlockPos, i32)>,
    pub(crate) stale_banners: Vec<(i32, MapBanner)>,
    pub(crate) framed_map_ids: BTreeSet<i32>,
    pub(crate) island_players: BTreeMap<PlayerId, String>,
    pub(crate) player_ticks: Vec<PlayerMapTick>,
    pub(crate) step_updates: Vec<(PlayerId, i32, i32)>,
    pub(crate) color_updates: Vec<MapColorDelta>,
}

/// 物品展示框在 `ServerEntity.tickCount % 10 == 0` 时产生的广播请求，经 [`GlobalPort`]
/// 交给主控写展示框标记并给本岛玩家发地图包。
///
/// Framed-map 10-tick broadcast request sent through [`GlobalPort`] to update
/// the frame marker on [`Global`] and emit map packets to this island's players.
pub(crate) struct FramedMapTickEffect {
    pub(crate) map_id: i32,
    pub(crate) entity_id: i32,
    pub(crate) pos: BlockPos,
    pub(crate) rotation: i32,
    pub(crate) game_time: i64,
    pub(crate) island_players: Vec<PlayerId>,
}

/// 构造一条展示框地图广播效果；岛上无玩家或框内不是带 `map_id` 的地图时返回 `None`。
///
/// Build a [`FramedMapTickEffect`], or `None` when no players are on this island
/// or the frame does not hold a filled map with a `map_id`.
pub(crate) fn build_framed_map_tick(
    players: &BTreeMap<PlayerId, crate::player::Player>,
    entity_id: i32,
    frame: &ItemFramePublic,
    game_time: i64,
) -> Option<FramedMapTickEffect> {
    if players.is_empty() || frame.item.item != items::FILLED_MAP {
        return None;
    }
    let map_id = stack_map_id(&frame.item)?;
    Some(FramedMapTickEffect {
        map_id,
        entity_id,
        pos: frame.pos,
        rotation: frame.direction.data_2d() * 90,
        game_time,
        island_players: players.keys().copied().collect(),
    })
}

/// 主控侧应用展示框 10 刻广播：登记展示框标记、向发起岛上的每名玩家发出脏补丁 / 标记包。
/// 不驱逐别的岛上的在线持有者。
///
/// Master-side handler for the 10-tick framed-map broadcast: update the frame
/// marker on the authoritative [`MapStore`] and send updates to the island's
/// players without evicting online holders on other islands.
pub(crate) fn apply_framed_map_tick(
    global: &mut Global,
    region: &mut Region,
    effect: FramedMapTickEffect,
) {
    if effect.island_players.is_empty() || global.maps().get(effect.map_id).is_none() {
        return;
    }
    let mut outgoing = Vec::new();
    {
        let online_ids: BTreeSet<PlayerId> = effect
            .island_players
            .iter()
            .copied()
            .filter(|id| region.players.contains_key(id))
            .collect();
        let retain_online = |player: PlayerId, global_online: &BTreeSet<PlayerId>| {
            online_ids.contains(&player) || global_online.contains(&player)
        };
        let all_online: BTreeSet<PlayerId> = region
            .players
            .keys()
            .copied()
            .filter(|&id| global.is_online(id))
            .collect();
        // 先收集当前仍在线的全服玩家集合（避免借用冲突）：单岛 / 裸 Region 单测里
        // `roster` 可能为空，所以与 `region.players` 并集判定在线。
        let mut live_holders: BTreeSet<PlayerId> = all_online;
        if let Some(existing) = global.maps().get(effect.map_id) {
            for h in &existing.carried_by {
                if global.is_online(h.player) {
                    live_holders.insert(h.player);
                }
            }
        }
        let Some(data) = global.maps_mut().get_mut(effect.map_id) else {
            return;
        };
        for &pid in &effect.island_players {
            let Some(player) = region.players.get(&pid) else {
                continue;
            };
            data.ensure_holding_player(player.id);
            if !player_holds_map(player, effect.map_id) {
                data.remove_decoration(&player.name);
            }
            // `frame != null` 时官方 `tickCarriedBy` 只清掉已离开世界的持有者，不踢非持有观察者。
            data.carried_by
                .retain(|h| retain_online(h.player, &live_holders));
            data.set_frame_marker(
                effect.pos,
                effect.rotation,
                effect.entity_id,
                Some(effect.game_time),
            );
            if let Some(update) = data.get_update_packet(effect.map_id, player.id) {
                outgoing.push((player.id, update));
            }
        }
    }
    for (player, update) in &outgoing {
        let packet = update.as_packet();
        region.links.send(*player, &packet);
    }
    global.publish(region);
}

/// 每刻的玩家地图同步（岛侧）：在本地区块上核销失效展示框、逐列采样未锁定地图的颜色增量，
/// 并把结果做成 [`GlobalEffect::MapTick`] 经 [`GlobalPort`] 交给主控写权威 [`MapStore`] 并发包。
/// 单岛齐步（`PortMode::Direct`）下当场应用并发包，与官方逐位一致；多岛分离下交回主控时合并。
///
/// Island-side per-tick map sync: detect stale frame markers in local chunks,
/// sample unlocked held-map color deltas locally, and submit a
/// [`GlobalEffect::MapTick`] through [`GlobalPort`].
pub(crate) fn tick(port: &mut GlobalPort<'_>, region: &mut Region) {
    if region.global.maps.is_empty() {
        return;
    }
    let game_time = region.tick.game_time();
    let dimension = crate::region::DIMENSION;

    // 1. 收集本岛仍装着地图的展示框，找出本岛已加载区块中已移除/已取出的展示框标记。
    let mut framed_maps: BTreeMap<i32, BTreeSet<i32>> = BTreeMap::new();
    for public in region.entities.iter_public() {
        if public.removed {
            continue;
        }
        let KindPublic::ItemFrame(frame) = &public.kind else {
            continue;
        };
        if frame.item.item != items::FILLED_MAP {
            continue;
        }
        if let Some(map_id) = stack_map_id(&frame.item) {
            framed_maps.entry(map_id).or_default().insert(public.uid);
        }
    }
    let mut stale_frames: Vec<(i32, BlockPos, i32)> = Vec::new();
    let empty_ids = BTreeSet::new();
    for (&map_id, data) in &region.global.maps.maps {
        let live = framed_maps.get(&map_id).unwrap_or(&empty_ids);
        for frame in data.frame_markers.values() {
            if region
                .chunks
                .is_loaded(ChunkPos::new(frame.pos.x >> 4, frame.pos.z >> 4))
                && !live.contains(&frame.entity_id)
            {
                stale_frames.push((map_id, frame.pos, frame.entity_id));
            }
        }
    }

    if region.players.is_empty() {
        if !stale_frames.is_empty() {
            port.submit(
                region,
                GlobalEffect::MapTick(MapTickEffect {
                    game_time,
                    dimension,
                    stale_frames,
                    stale_banners: Vec::new(),
                    framed_map_ids: framed_maps.into_keys().collect(),
                    island_players: BTreeMap::new(),
                    player_ticks: Vec::new(),
                    step_updates: Vec::new(),
                    color_updates: Vec::new(),
                }),
            );
        }
        return;
    }

    let mut island_players: BTreeMap<PlayerId, String> = BTreeMap::new();
    let mut player_ticks: Vec<PlayerMapTick> = Vec::new();
    let mut hand_samples: Vec<(PlayerId, f64, f64, Vec<i32>)> = Vec::new();
    let mut any_carried = false;

    for player in region.players.values() {
        let id = player.id;
        let pos = player.position();
        let main_hand = player.inventory.hand_slot(Hand::Main);
        let off_hand = player.inventory.hand_slot(Hand::Off);
        let mut maps = Vec::new();
        let mut hand_maps = Vec::new();
        // 官方 `Inventory.items`（36 格）+ `EntityEquipment`（盔甲 / 副手）对应菜单槽 5..=45，
        // 不含 2×2 合成格的 0..=4；`inventoryTick` 只挂在 `MapItem`（`filled_map`）上。
        for (slot, stack) in player.inventory.slots.iter().enumerate().skip(5) {
            if stack.item == items::FILLED_MAP
                && let Some(map_id) = stack_map_id(stack)
            {
                maps.push(map_id);
                if slot == main_hand || slot == off_hand {
                    hand_maps.push(map_id);
                }
            }
        }
        maps.sort_unstable();
        maps.dedup();
        if !maps.is_empty() {
            any_carried = true;
        }
        island_players.insert(id, player.name.clone());
        if !hand_maps.is_empty() {
            hand_samples.push((id, pos.x, pos.z, hand_maps));
        }
        player_ticks.push(PlayerMapTick {
            id,
            name: player.name.clone(),
            x: pos.x,
            z: pos.z,
            yaw: f64::from(player.yaw),
            maps,
        });
    }

    // 快速跳过：没有失效展示框、没人携带地图、且没有地图追踪到本岛玩家时，不产生任何效果。
    if stale_frames.is_empty() && !any_carried {
        let any_tracked_here = region.global.maps.maps.values().any(|data| {
            data.carried_by
                .iter()
                .any(|h| island_players.contains_key(&h.player))
                || data
                    .decorations
                    .iter()
                    .any(|(k, _)| island_players.values().any(|name| name == k))
        });
        if !any_tracked_here {
            return;
        }
    }

    // 2. 在本岛区块上跑逐列颜色采样与 `checkBanners`，产出增量像素、失效旗帜与步数更新。
    //    同一刻多名玩家或主副手同时拿同一张图时共用本地 colors 缓冲，保证与单岛原地修改逐位一致。
    let mut scratch_colors: BTreeMap<i32, [u8; COLORS_LEN]> = BTreeMap::new();
    let mut steps: BTreeMap<(PlayerId, i32), i32> = BTreeMap::new();
    let mut color_diffs: BTreeMap<i32, Vec<(u8, u8, u8)>> = BTreeMap::new();
    let mut stale_banners: Vec<(i32, MapBanner)> = Vec::new();
    let mut seen_stale_banners: BTreeSet<(i32, String)> = BTreeSet::new();

    for (pid, x, z, hand_maps) in &hand_samples {
        for &map_id in hand_maps {
            let Some(data) = region.global.maps.get(map_id) else {
                continue;
            };
            if data.locked || dimension != data.dimension {
                continue;
            }
            let colors = scratch_colors.entry(map_id).or_insert(data.colors);
            let step_entry = steps
                .entry((*pid, map_id))
                .or_insert_with(|| data.holding_player_step(*pid));
            *step_entry = step_entry.wrapping_add(1);
            let step = *step_entry;
            let diffs = color_diffs.entry(map_id).or_default();
            let has_banners = !data.banner_markers.is_empty();
            sample_colors_with_step(
                data.center_x,
                data.center_z,
                data.scale,
                step,
                *x,
                *z,
                &region.chunks,
                &mut (),
                |_, bx, bz| {
                    if !has_banners {
                        return;
                    }
                    for (id, banner) in &data.banner_markers {
                        if banner.pos.x == bx
                            && banner.pos.z == bz
                            && !seen_stale_banners.contains(&(map_id, id.clone()))
                            && banner_from_world(&region.chunks, &region.block_items, banner.pos)
                                .as_ref()
                                != Some(banner)
                        {
                            seen_stale_banners.insert((map_id, id.clone()));
                            stale_banners.push((map_id, banner.clone()));
                        }
                    }
                },
                |_, px, pz, color| {
                    let idx = px + pz * MAP_SIZE;
                    if colors[idx] != color {
                        colors[idx] = color;
                        diffs.push((px as u8, pz as u8, color));
                        true
                    } else {
                        false
                    }
                },
            );
        }
    }

    let step_updates: Vec<(PlayerId, i32, i32)> = steps
        .into_iter()
        .map(|((player, map_id), step)| (player, map_id, step))
        .collect();
    let color_updates: Vec<MapColorDelta> = color_diffs
        .into_iter()
        .filter(|(_, pixels)| !pixels.is_empty())
        .map(|(map_id, pixels)| MapColorDelta { map_id, pixels })
        .collect();

    port.submit(
        region,
        GlobalEffect::MapTick(MapTickEffect {
            game_time,
            dimension,
            stale_frames,
            stale_banners,
            framed_map_ids: framed_maps.into_keys().collect(),
            island_players,
            player_ticks,
            step_updates,
            color_updates,
        }),
    );
}

/// 主控侧应用一张岛交回的 [`MapTickEffect`]：核销失效展示框、刷新玩家标记、合并颜色增量、
/// 仅在本岛玩家范围内清理不再持有地图的观察者（不动别的岛上的在线玩家），最后向本岛玩家发包。
///
/// Master-side handler for [`MapTickEffect`]: remove stale frame markers, update
/// player markers, merge color deltas, evict non-carriers only among this
/// island's players (leaving online holders on other islands intact), and emit
/// map packets to this island's players.
pub(crate) fn apply_map_tick(global: &mut Global, region: &mut Region, effect: MapTickEffect) {
    let mut carriers: BTreeMap<i32, BTreeSet<PlayerId>> = BTreeMap::new();
    for p in &effect.player_ticks {
        for &map_id in &p.maps {
            carriers.entry(map_id).or_default().insert(p.id);
        }
    }

    // 1. 核销该岛已加载区块里消失的展示框标记。
    for (map_id, pos, entity_id) in effect.stale_frames {
        if let Some(data) = global.maps_mut().get_mut(map_id) {
            data.removed_from_frame(pos, entity_id);
        }
    }

    // 2. 刷新本岛玩家标记与手持采样步数 / 颜色增量。
    for p in &effect.player_ticks {
        for &map_id in &p.maps {
            if let Some(data) = global.maps_mut().get_mut(map_id) {
                data.tick_carried_by_player(
                    p.id,
                    &p.name,
                    p.x,
                    p.z,
                    p.yaw,
                    effect.game_time,
                    effect.dimension,
                );
            }
        }
    }
    for &(player, map_id, step) in &effect.step_updates {
        if let Some(data) = global.maps_mut().get_mut(map_id) {
            data.holding_player_mut(player).step = step;
        }
    }
    for delta in &effect.color_updates {
        if let Some(data) = global.maps_mut().get_mut(delta.map_id)
            && !data.locked
        {
            for &(x, y, color) in &delta.pixels {
                data.update_color(usize::from(x), usize::from(y), color);
            }
        }
    }
    for (map_id, expected) in effect.stale_banners {
        if let Some(data) = global.maps_mut().get_mut(map_id)
            && !data.locked
        {
            let id = expected.id();
            if data.banner_markers.get(&id) == Some(&expected) {
                data.banner_markers.remove(&id);
                data.remove_decoration(&id);
                data.dirty = true;
            }
        }
    }

    // 3. 清理本岛不再持有地图的玩家（以及已离线玩家），绝不碰其他岛上仍在线的玩家。
    //    先只读筛出真的需要清理的地图，避免对无关地图做写时复制。
    let to_clean: Vec<i32> = global
        .maps()
        .maps
        .iter()
        .filter_map(|(&map_id, data)| {
            let map_carriers = carriers.get(&map_id);
            let needs_clean = data.carried_by.iter().any(|h| {
                let local = effect.island_players.contains_key(&h.player);
                if local {
                    !map_carriers.is_some_and(|c| c.contains(&h.player))
                } else {
                    !global.is_online(h.player)
                }
            }) || data.decorations.iter().any(|(k, _)| {
                effect
                    .island_players
                    .iter()
                    .any(|(pid, name)| name == k && !map_carriers.is_some_and(|c| c.contains(pid)))
            });
            needs_clean.then_some(map_id)
        })
        .collect();

    let no_carriers = BTreeSet::new();
    for map_id in to_clean {
        let map_carriers = carriers.get(&map_id).unwrap_or(&no_carriers);
        let framed_here = effect.framed_map_ids.contains(&map_id);
        let remote_online: BTreeSet<PlayerId> = global
            .maps()
            .get(map_id)
            .map(|data| {
                data.carried_by
                    .iter()
                    .map(|h| h.player)
                    .filter(|&pid| {
                        !effect.island_players.contains_key(&pid) && global.is_online(pid)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let Some(data) = global.maps_mut().get_mut(map_id) else {
            continue;
        };
        if map_carriers.is_empty() && framed_here {
            data.carried_by.retain(|h| {
                effect.island_players.contains_key(&h.player) || remote_online.contains(&h.player)
            });
            for name in effect.island_players.values() {
                data.remove_decoration(name);
            }
        } else {
            let mut removed_local = Vec::new();
            data.carried_by.retain(|h| {
                if effect.island_players.contains_key(&h.player) {
                    if map_carriers.contains(&h.player) {
                        true
                    } else {
                        removed_local.push(h.player);
                        false
                    }
                } else {
                    remote_online.contains(&h.player)
                }
            });
            for pid in removed_local {
                if let Some(name) = effect.island_players.get(&pid) {
                    data.remove_decoration(name);
                }
            }
        }
    }

    // 4. 给本岛每名持有地图的玩家取一份脏补丁 / 标记包并发出（`ServerPlayer.synchronizeSpecialItemUpdates`）。
    let mut targets: BTreeSet<(PlayerId, i32)> = BTreeSet::new();
    for p in &effect.player_ticks {
        for &map_id in &p.maps {
            targets.insert((p.id, map_id));
        }
    }
    let mut outgoing = Vec::new();
    for (player, map_id) in targets {
        if let Some(data) = global.maps_mut().get_mut(map_id)
            && let Some(update) = data.get_update_packet(map_id, player)
        {
            outgoing.push((player, update));
        }
    }
    for (player, update) in &outgoing {
        let packet = update.as_packet();
        region.links.send(*player, &packet);
    }
    global.publish(region);
}

fn get<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    match tag {
        Tag::Compound(entries) => entries.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn get_int(tag: &Tag, key: &str) -> Option<i32> {
    match get(tag, key) {
        Some(Tag::Int(v)) => Some(*v),
        _ => None,
    }
}

fn get_string<'a>(tag: &'a Tag, key: &str) -> Option<&'a str> {
    match get(tag, key) {
        Some(Tag::String(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn get_block_pos(tag: &Tag, key: &str) -> Option<BlockPos> {
    match get(tag, key) {
        Some(Tag::IntArray(coords)) if coords.len() == 3 => {
            Some(BlockPos::new(coords[0], coords[1], coords[2]))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_fresh_snaps_center_to_official_scale_grid() {
        // scale 0: size = 128; x in [-64, 63] -> grid 0 -> center 0
        let d0 = MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, "minecraft:overworld");
        assert_eq!((d0.center_x, d0.center_z), (0, 0));

        let d0_edge =
            MapItemSavedData::create_fresh(63.0, -64.0, 0, true, false, "minecraft:overworld");
        assert_eq!((d0_edge.center_x, d0_edge.center_z), (0, 0));

        let d0_next =
            MapItemSavedData::create_fresh(64.0, -65.0, 0, true, false, "minecraft:overworld");
        assert_eq!((d0_next.center_x, d0_next.center_z), (128, -128));

        // scale 1: size = 256; x = 0 -> grid 0 -> center 64
        let d1 = MapItemSavedData::create_fresh(0.0, 0.0, 1, true, false, "minecraft:overworld");
        assert_eq!((d1.center_x, d1.center_z), (64, 64));

        // scale 4: size = 2048; x = 0 -> grid 0 -> center 960
        let d4 = MapItemSavedData::create_fresh(0.0, 0.0, 4, true, false, "minecraft:overworld");
        assert_eq!((d4.center_x, d4.center_z), (960, 960));
    }

    #[test]
    fn map_index_starts_at_zero_and_round_trips() {
        let mut idx = MapIndex::default();
        assert_eq!(idx.last_map_id, -1);
        assert_eq!(idx.next_map_id(), 0);
        assert_eq!(idx.next_map_id(), 1);
        let tag = idx.to_tag();
        let restored = MapIndex::from_tag(&tag);
        assert_eq!(restored.last_map_id, 1);
    }

    #[test]
    fn map_data_round_trips_colors_banners_and_frames() {
        let mut data =
            MapItemSavedData::create_fresh(100.0, -200.0, 2, true, true, "minecraft:overworld");
        data.set_color(10, 20, packed_color(map_color::WATER, Brightness::High));
        data.set_color(127, 127, packed_color(map_color::STONE, Brightness::Low));
        let banner = MapBanner {
            pos: BlockPos::new(100, 64, -200),
            color: 14, // red
            name: Some(Component::text("Base")),
        };
        data.banner_markers.insert(banner.id(), banner.clone());
        let frame = MapFrame {
            pos: BlockPos::new(110, 65, -190),
            rotation: 90,
            entity_id: 42,
        };
        data.frame_markers.insert(frame.id(), frame.clone());

        let locked = data.locked();
        assert!(locked.locked);
        assert_eq!(locked.colors[10 + 20 * 128], data.colors[10 + 20 * 128]);
        assert!(locked.frame_markers.is_empty());

        let tag = data.to_tag();
        let restored = MapItemSavedData::from_tag(&tag).expect("decode map data");
        assert_eq!(restored.dimension, "minecraft:overworld");
        assert_eq!(restored.center_x, data.center_x);
        assert_eq!(restored.center_z, data.center_z);
        assert_eq!(restored.scale, 2);
        assert!(restored.tracking_position);
        assert!(restored.unlimited_tracking);
        assert!(!restored.locked);
        assert_eq!(restored.colors, data.colors);
        assert_eq!(restored.banner_markers.len(), 1);
        assert_eq!(restored.frame_markers.len(), 1);
        assert_eq!(restored.decorations.len(), 2);
        assert_eq!(restored.tracked_decoration_count, 2);
    }

    #[test]
    fn holding_player_batches_dirty_rect_and_throttles_decorations() {
        let mut data =
            MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, "minecraft:overworld");
        let p = PlayerId(1);
        data.ensure_holding_player(p);
        // 首次调用发全图 128x128 与初始标记（tick=0, 0 % 5 == 0）。
        let first = data.get_update_packet(0, p).expect("initial packet");
        let patch = first.color_patch.expect("initial patch");
        assert_eq!(
            (patch.start_x, patch.start_y, patch.width, patch.height),
            (0, 0, 128, 128)
        );
        assert!(first.decorations.is_some());
        // 没改动时不再发包。
        assert!(data.get_update_packet(0, p).is_none());
        // 改两个像素，补丁应精确包住这两个像素的最小外接矩形。
        data.update_color(5, 10, 44);
        data.update_color(8, 12, 48);
        let second = data.get_update_packet(0, p).expect("color patch");
        let patch = second.color_patch.expect("dirty rect");
        assert_eq!(
            (patch.start_x, patch.start_y, patch.width, patch.height),
            (5, 10, 4, 3)
        );
        assert!(second.decorations.is_none());
        assert_eq!(patch.colors[0], 44);
        assert_eq!(patch.colors[3 + 2 * 4], 48);
        // 标记脏后 tick=1..=4 节流不发，tick=5 才发。
        data.add_decoration(
            decoration_type::PLAYER,
            None,
            "Alex".into(),
            0.0,
            0.0,
            0.0,
            None,
        );
        for _ in 1..5 {
            assert!(data.get_update_packet(0, p).is_none());
        }
        let dec_pkt = data.get_update_packet(0, p).expect("decorations on tick 5");
        assert!(dec_pkt.color_patch.is_none());
        assert_eq!(dec_pkt.decorations.unwrap().len(), 1);
    }

    #[test]
    fn use_empty_map_survival_single_multi_and_creative() {
        use std::sync::{Arc, Mutex};

        use mc_bamboo_core::Vec3;
        use mc_bamboo_data::{items, packets::play::clientbound};
        use mc_bamboo_protocol::{
            codec::Reader,
            frame::{MAX_FRAME_LEN, find_frame},
            packet::play::ClientInformation,
        };
        use uuid::Uuid;

        use crate::{inbound::PlayerSink, outgoing::Outgoing, player::Player};

        #[derive(Default, Clone)]
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl PlayerSink for Sink {
            fn send(&mut self, frames: Outgoing) -> bool {
                self.0.lock().unwrap().extend(frames.to_raw_bytes());
                true
            }
            fn close(&mut self) {}
        }

        let mut region = crate::region::Region::new(2, 20, false);
        let p_id = PlayerId(1);
        let player = Player::new(
            p_id,
            Uuid::from_u128(1),
            "Alex".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(10.5, 64.0, -20.5),
        );
        region.insert_player(player);
        let sink = Sink::default();
        region.links.add(p_id, Box::new(sink.clone()));

        // 1. 生存手里只有 1 张空地图 → 原槽位变成 filled_map(id=0)，记录统计并播放音效。
        region.players.get_mut(&p_id).unwrap().inventory.slots[36] = ItemStack::new(items::MAP, 1);
        assert!(use_empty_map(&mut region, None, p_id, Hand::Main));
        region.links.flush();
        let held = &region.players[&p_id].inventory.slots[36];
        assert_eq!(held.item, items::FILLED_MAP);
        assert_eq!(held.count, 1);
        assert_eq!(stack_map_id(held), Some(0));
        assert_eq!(region.global.maps.index.last_map_id, 0);
        let data0 = region.global.maps.get(0).expect("map 0 created");
        assert_eq!((data0.center_x, data0.center_z, data0.scale), (0, 0, 0));
        assert!(data0.tracking_position);
        assert!(!data0.unlimited_tracking);
        let dirty_stats = region
            .players
            .get_mut(&p_id)
            .unwrap()
            .item_use_stats
            .take_dirty();
        assert_eq!(dirty_stats, vec![(items::MAP, 1)]);

        let raw = std::mem::take(&mut *sink.0.lock().unwrap());
        let mut rest = raw.as_slice();
        let mut saw_sound = false;
        while let Some(bounds) = find_frame(rest, MAX_FRAME_LEN).unwrap() {
            let end = bounds.header_len + bounds.payload_len;
            let mut r = Reader::new(&rest[bounds.header_len..end]);
            if r.varint().unwrap() == clientbound::SOUND {
                saw_sound = true;
            }
            rest = &rest[end..];
        }
        assert!(saw_sound, "should play cartography take_result sound");

        // 2. 生存手里有 3 张空地图 → 扣 1 张剩 2 张，新地图（id=1）放进背包空位。
        region.players.get_mut(&p_id).unwrap().inventory.slots[36] = ItemStack::new(items::MAP, 3);
        assert!(use_empty_map(&mut region, None, p_id, Hand::Main));
        assert_eq!(region.players[&p_id].inventory.slots[36].item, items::MAP);
        assert_eq!(region.players[&p_id].inventory.slots[36].count, 2);
        let found = region.players[&p_id]
            .inventory
            .slots
            .iter()
            .find(|s| s.item == items::FILLED_MAP && stack_map_id(s) == Some(1));
        assert!(
            found.is_some(),
            "filled map id=1 should be added to inventory"
        );

        // 3. 创造模式 → 不扣空地图，仍创建新地图（id=2）并放入背包。
        region.players.get_mut(&p_id).unwrap().game_mode = GameMode::Creative;
        region.players.get_mut(&p_id).unwrap().inventory.slots[36] = ItemStack::new(items::MAP, 1);
        assert!(use_empty_map(&mut region, None, p_id, Hand::Main));
        assert_eq!(region.players[&p_id].inventory.slots[36].item, items::MAP);
        assert_eq!(region.players[&p_id].inventory.slots[36].count, 1);
        let found2 = region.players[&p_id]
            .inventory
            .slots
            .iter()
            .find(|s| s.item == items::FILLED_MAP && stack_map_id(s) == Some(2));
        assert!(
            found2.is_some(),
            "creative should keep empty map and add filled map"
        );
    }

    #[test]
    fn update_colors_samples_terrain_slopes_water_depth_and_fluids() {
        use mc_bamboo_blocks::{
            BlockState,
            state::{SLAB_TYPE, SlabType, WATERLOGGED},
        };

        // 构造平坦草地测试区 + 斜坡 + 浅水/深水 + 含水半砖。
        let mut chunks = ChunkStore::flat();
        // 在 (0..=3, *, -2..=4) 铺平到 y=64 石头。
        for bx in 0..=3 {
            for bz in -2..=4 {
                chunks.set_block(BlockPos::new(bx, 64, bz), blocks::STONE);
            }
        }
        // 上坡：(0, 65..=66, 1) 石头。
        chunks.set_block(BlockPos::new(0, 65, 1), blocks::STONE);
        chunks.set_block(BlockPos::new(0, 66, 1), blocks::STONE);

        // 浅水：(1, 64, 1) 1 格水 → waterDepth = 1 → d = 0.1 + ((x+z)&1)*0.2 < 0.5 → High。
        chunks.set_block(BlockPos::new(1, 64, 1), blocks::WATER);
        // 深水：(2, 54..=64, 1) 11 格水 → waterDepth = 11 → d >= 1.1 > 0.9 → Low。
        for y in 54..=64 {
            chunks.set_block(BlockPos::new(2, y, 1), blocks::WATER);
        }
        // 含水半砖（顶面不完整支撑面）：(3, 64, 1) 用下置含水石半砖，地图应按水着色。
        let waterlogged_slab = BlockState(blocks::STONE_SLAB)
            .with(SLAB_TYPE, SlabType::Bottom)
            .with(WATERLOGGED, true)
            .0;
        chunks.set_block(BlockPos::new(3, 64, 1), waterlogged_slab);

        let mut data =
            MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, "minecraft:overworld");
        let p = PlayerId(1);
        // 跑 16 次覆盖全部 `(x & 15)` 列。
        for _ in 0..16 {
            update_colors(&mut data, p, 0.5, 0.5, "minecraft:overworld", &chunks);
        }

        // pixel = world + 64
        let px = |wx: i32| (wx + 64) as usize;
        let pz = |wz: i32| (wz + 64) as usize;
        let color_at = |wx: i32, wz: i32| data.colors[px(wx) + pz(wz) * 128];

        // (0, 0): 平坦石头（z=-1 也是 y=64）→ STONE + Normal。
        assert_eq!(
            color_at(0, 0),
            packed_color(map_color::STONE, Brightness::Normal)
        );
        // (0, 1): 上坡 +2 格 → d = 2*4/5 + 0.2 = 1.8 > 0.6 → STONE + High。
        assert_eq!(
            color_at(0, 1),
            packed_color(map_color::STONE, Brightness::High)
        );
        // (0, 2): 下坡 -2 格 → d = -2*4/5 - 0.2 = -1.8 < -0.6 → STONE + Low。
        assert_eq!(
            color_at(0, 2),
            packed_color(map_color::STONE, Brightness::Low)
        );
        // (1, 1): 浅水 → WATER + High。
        assert_eq!(
            color_at(1, 1),
            packed_color(map_color::WATER, Brightness::High)
        );
        // (2, 1): 深水 → WATER + Low。
        assert_eq!(
            color_at(2, 1),
            packed_color(map_color::WATER, Brightness::Low)
        );
        // (3, 1): 含水下置石半砖 → getCorrectStateForFluidBlock 返回水 → WATER + High。
        assert_eq!(
            color_at(3, 1),
            packed_color(map_color::WATER, Brightness::High)
        );
    }

    fn tick_local(region: &mut Region) {
        let mut global = Global::default();
        global.set_maps((*region.global.maps).clone());
        let mut port = GlobalPort::Direct(&mut global);
        tick(&mut port, region);
        region.global.maps = Arc::new(global.maps().clone());
    }

    #[test]
    fn tick_syncs_held_map_full_patch_marker_and_cleans_up_on_drop() {
        use std::sync::{Arc, Mutex};

        use mc_bamboo_core::Vec3;
        use mc_bamboo_data::{items, packets::play::clientbound};
        use mc_bamboo_protocol::{
            codec::Reader,
            frame::{MAX_FRAME_LEN, find_frame},
            packet::play::ClientInformation,
        };
        use uuid::Uuid;

        use crate::{inbound::PlayerSink, outgoing::Outgoing, player::Player};

        #[derive(Default, Clone)]
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl PlayerSink for Sink {
            fn send(&mut self, frames: Outgoing) -> bool {
                self.0.lock().unwrap().extend(frames.to_raw_bytes());
                true
            }
            fn close(&mut self) {}
        }

        #[derive(Debug, PartialEq, Eq)]
        struct Decoded {
            map_id: i32,
            scale: i8,
            locked: bool,
            decorations: i32,
            patch_width: u8,
        }

        fn decode(raw: &[u8]) -> Vec<Decoded> {
            let mut out = Vec::new();
            let mut rest = raw;
            while let Some(bounds) = find_frame(rest, MAX_FRAME_LEN).unwrap() {
                let end = bounds.header_len + bounds.payload_len;
                let mut r = Reader::new(&rest[bounds.header_len..end]);
                if r.varint().unwrap() == clientbound::MAP_ITEM_DATA {
                    let map_id = r.varint().unwrap();
                    let scale = r.i8().unwrap();
                    let locked = r.bool().unwrap();
                    let decorations = if r.bool().unwrap() {
                        let count = r.varint().unwrap();
                        for _ in 0..count {
                            let _kind = r.varint().unwrap();
                            let _x = r.i8().unwrap();
                            let _y = r.i8().unwrap();
                            let _rot = r.i8().unwrap();
                            assert!(!r.bool().unwrap(), "markers have no name component");
                        }
                        count
                    } else {
                        0
                    };
                    let patch_width = r.u8().unwrap();
                    if patch_width > 0 {
                        let _height = r.u8().unwrap();
                        let _start_x = r.u8().unwrap();
                        let _start_y = r.u8().unwrap();
                        let len = r.varint().unwrap();
                        let _ = r.bytes(len.max(0) as usize).unwrap();
                    }
                    out.push(Decoded {
                        map_id,
                        scale,
                        locked,
                        decorations,
                        patch_width,
                    });
                }
                rest = &rest[end..];
            }
            out
        }

        let mut region = crate::region::Region::new(2, 20, false);
        let p_id = PlayerId(1);
        let player = Player::new(
            p_id,
            Uuid::from_u128(1),
            "Alex".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        region.insert_player(player);
        let sink = Sink::default();
        region.links.add(p_id, Box::new(sink.clone()));
        Arc::make_mut(&mut region.global.maps).insert(
            0,
            MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, "minecraft:overworld"),
        );
        // 主手（选中格 36）放一张带 map_id=0 的已填充地图。
        region.players.get_mut(&p_id).unwrap().inventory.slots[36] = make_filled_map(0);

        tick_local(&mut region);
        region.links.flush();
        let first = decode(&std::mem::take(&mut *sink.0.lock().unwrap()));
        assert_eq!(
            first,
            vec![Decoded {
                map_id: 0,
                scale: 0,
                locked: false,
                decorations: 1,
                patch_width: 128,
            }],
            "first tick sends a full 128-wide patch and the player marker"
        );
        // 玩家标记已加入地图数据。
        assert_eq!(region.global.maps.get(0).unwrap().decorations().count(), 1);

        // 地图被拿走：不再发包，玩家标记与持有者都被清掉。
        region.players.get_mut(&p_id).unwrap().inventory.slots[36] = ItemStack::empty();
        tick_local(&mut region);
        region.links.flush();
        assert!(decode(&std::mem::take(&mut *sink.0.lock().unwrap())).is_empty());
        assert_eq!(region.global.maps.get(0).unwrap().decorations().count(), 0);
        assert!(region.global.maps.get(0).unwrap().carried_by.is_empty());

        // 空地图（无 map_id）不会触发任何同步。
        region.players.get_mut(&p_id).unwrap().inventory.slots[36] = ItemStack::new(items::MAP, 1);
        tick_local(&mut region);
        region.links.flush();
        assert!(decode(&std::mem::take(&mut *sink.0.lock().unwrap())).is_empty());
    }

    #[test]
    fn tick_broadcasts_framed_map_marker_on_the_ten_tick_cadence() {
        use std::sync::{Arc, Mutex};

        use mc_bamboo_blocks::entity::{Entity, EntityKind};
        use mc_bamboo_core::{Direction, Vec3};
        use mc_bamboo_data::packets::play::clientbound;
        use mc_bamboo_protocol::{
            codec::Reader,
            frame::{MAX_FRAME_LEN, find_frame},
            packet::play::ClientInformation,
        };
        use uuid::Uuid;

        use crate::{inbound::PlayerSink, outgoing::Outgoing, player::Player};

        #[derive(Default, Clone)]
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl PlayerSink for Sink {
            fn send(&mut self, frames: Outgoing) -> bool {
                self.0.lock().unwrap().extend(frames.to_raw_bytes());
                true
            }
            fn close(&mut self) {}
        }

        let mut region = crate::region::Region::new(2, 20, false);
        // 故意选一个不是 10 的倍数的 game_time，验证展示框按自身 `ServerEntity.tickCount`
        // （从 0 开始，每 10 刻一次）而不是 `game_time % 10` 广播。
        region.start_clock(crate::global::GameTime(7));
        let p_id = PlayerId(1);
        let player = Player::new(
            p_id,
            Uuid::from_u128(1),
            "Alex".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        region.insert_player(player);
        let sink = Sink::default();
        region.links.add(p_id, Box::new(sink.clone()));
        Arc::make_mut(&mut region.global.maps).insert(
            0,
            MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, "minecraft:overworld"),
        );

        // 一个装着带 map_id=0 地图的物品展示框，玩家手里没有地图。
        let mut frame = Entity::item_frame(BlockPos::new(0, 64, 0), Direction::South, false);
        if let EntityKind::ItemFrame(f) = &mut frame.kind {
            f.item = make_filled_map(0);
        }
        region.entities.spawn(frame);

        let drain_map_packets = |sink: &Sink| -> Vec<(usize, u8)> {
            let raw = std::mem::take(&mut *sink.0.lock().unwrap());
            let mut rest = raw.as_slice();
            let mut out = Vec::new();
            while let Some(bounds) = find_frame(rest, MAX_FRAME_LEN).unwrap() {
                let end = bounds.header_len + bounds.payload_len;
                let mut r = Reader::new(&rest[bounds.header_len..end]);
                if r.varint().unwrap() == clientbound::MAP_ITEM_DATA {
                    let _map_id = r.varint().unwrap();
                    let _scale = r.i8().unwrap();
                    let _locked = r.bool().unwrap();
                    let mut frame_markers = 0usize;
                    if r.bool().unwrap() {
                        let count = r.varint().unwrap();
                        for _ in 0..count {
                            let kind = r.varint().unwrap();
                            let _x = r.i8().unwrap();
                            let _y = r.i8().unwrap();
                            let _rot = r.i8().unwrap();
                            assert!(!r.bool().unwrap());
                            if kind == decoration_type::FRAME {
                                frame_markers += 1;
                            }
                        }
                    }
                    let patch_width = r.u8().unwrap();
                    out.push((frame_markers, patch_width));
                }
                rest = &rest[end..];
            }
            out
        };

        // 第 0 刻（ServerEntity.tickCount == 0，即使 game_time == 7）：立刻广播全图 + 展示框标记。
        tick_local(&mut region);
        crate::entity_tracking::sync(&mut region);
        region.links.flush();
        assert_eq!(drain_map_packets(&sink), vec![(1, 128)]);
        assert_eq!(region.global.maps.get(0).unwrap().frame_markers().len(), 1);

        // 第 1..=9 刻（包括 game_time == 10）：即使地图颜色变脏，展示框也还没到第 10 刻，不发包。
        Arc::make_mut(&mut region.global.maps)
            .get_mut(0)
            .unwrap()
            .update_color(1, 1, 44);
        for _ in 1..10 {
            region.tick = region.tick.plus(1);
            tick_local(&mut region);
            crate::entity_tracking::sync(&mut region);
            region.links.flush();
            assert!(
                drain_map_packets(&sink).is_empty(),
                "item frame must wait until its own ServerEntity.tickCount % 10 == 0"
            );
        }

        // 第 10 刻（ServerEntity.tickCount == 10，game_time == 17）：发出 1×1 增量补丁，而非重发 128×128。
        region.tick = region.tick.plus(1);
        tick_local(&mut region);
        crate::entity_tracking::sync(&mut region);
        region.links.flush();
        assert_eq!(drain_map_packets(&sink), vec![(0, 1)]);
    }

    #[test]
    fn locked_map_skips_color_sampling_during_tick() {
        use mc_bamboo_core::Vec3;
        use mc_bamboo_protocol::packet::play::ClientInformation;
        use uuid::Uuid;

        use crate::player::Player;

        let mut region = crate::region::Region::new(2, 20, false);
        let p_id = PlayerId(1);
        let player = Player::new(
            p_id,
            Uuid::from_u128(1),
            "Alex".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        region.insert_player(player);
        let locked =
            MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, "minecraft:overworld")
                .locked();
        Arc::make_mut(&mut region.global.maps).insert(0, locked);
        region.players.get_mut(&p_id).unwrap().inventory.slots[36] = make_filled_map(0);

        for _ in 0..16 {
            tick_local(&mut region);
        }
        assert!(
            region
                .global
                .maps
                .get(0)
                .unwrap()
                .colors
                .iter()
                .all(|&b| b == 0),
            "locked map must not update colors while held"
        );
    }

    #[test]
    fn toggle_banner_use_on_and_check_banners_on_break_or_replace() {
        use mc_bamboo_core::Vec3;
        use mc_bamboo_protocol::packet::play::ClientInformation;
        use uuid::Uuid;

        use crate::{interaction::UseItemOn, player::Player};

        let mut region = crate::region::Region::new(2, 20, false);
        let p_id = PlayerId(1);
        let player = Player::new(
            p_id,
            Uuid::from_u128(1),
            "Alex".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        region.insert_player(player);
        Arc::make_mut(&mut region.global.maps).insert(
            0,
            MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, "minecraft:overworld"),
        );
        region.players.get_mut(&p_id).unwrap().inventory.slots[36] = make_filled_map(0);

        // 放一面带自定义名的红色站立旗帜，右键地图加上旗帜标记。
        let banner_pos = BlockPos::new(2, 64, 1);
        region.chunks.set_block(banner_pos, blocks::RED_BANNER);
        let mut patch = ComponentPatch::new();
        patch.put(
            kinds::CUSTOM_NAME,
            PatchEntry::Set(ComponentValue::CustomName(Component::text("Outpost"))),
        );
        region.block_items.insert(banner_pos, patch);

        let click_banner = UseItemOn {
            hand: Hand::Main,
            pos: banner_pos,
            face: Direction::Up,
            cursor: (0.5, 0.5, 0.5),
            inside: false,
            sequence: 1,
        };
        crate::interaction::on_use_item_on(&mut region, p_id, click_banner);
        {
            let data = region.global.maps.get(0).unwrap();
            assert_eq!(data.banner_markers().len(), 1);
            let stored = data.banner_markers().values().next().unwrap();
            assert_eq!(stored.pos, banner_pos);
            assert_eq!(stored.color, 14); // red
            assert_eq!(stored.name, Some(Component::text("Outpost")));
            let dec = data.decorations().next().unwrap();
            assert_eq!(dec.kind, decoration_type::BANNER_WHITE + 14);
            // toggleBanner 用 pos + 0.5: (2.5 - 0)*2 + 0.5 = 5.5 -> 5; (1.5 - 0)*2 + 0.5 = 3.5 -> 3
            assert_eq!((dec.x, dec.y, dec.rot), (5, 3, 8));
            assert_eq!(dec.name, Some(Component::text("Outpost")));
        }

        // 再右键同一面旗帜 → 取消标记。
        crate::interaction::on_use_item_on(
            &mut region,
            p_id,
            UseItemOn {
                sequence: 2,
                ..click_banner
            },
        );
        assert!(
            region
                .global
                .maps
                .get(0)
                .unwrap()
                .banner_markers()
                .is_empty()
        );
        assert_eq!(region.global.maps.get(0).unwrap().decorations().count(), 0);

        // 第三次右键重新加上，再换成蓝色墙旗帜 → 跑 16 刻采样后 checkBanners 必须清掉旧红旗标记。
        crate::interaction::on_use_item_on(
            &mut region,
            p_id,
            UseItemOn {
                sequence: 3,
                ..click_banner
            },
        );
        assert_eq!(region.global.maps.get(0).unwrap().banner_markers().len(), 1);
        region
            .chunks
            .set_block(banner_pos, blocks::BLUE_WALL_BANNER);
        for _ in 0..16 {
            tick_local(&mut region);
        }
        assert!(
            region
                .global
                .maps
                .get(0)
                .unwrap()
                .banner_markers()
                .is_empty(),
            "replacing banner color must trigger checkBanners removal"
        );

        // 对蓝色墙旗帜右键可重新登记（验证墙旗帜颜色提取），打掉后 checkBanners 再次移除。
        crate::interaction::on_use_item_on(
            &mut region,
            p_id,
            UseItemOn {
                sequence: 4,
                ..click_banner
            },
        );
        let stored_blue = region
            .global
            .maps
            .get(0)
            .unwrap()
            .banner_markers()
            .values()
            .next()
            .unwrap();
        assert_eq!(stored_blue.color, 11); // blue
        region.chunks.set_block(banner_pos, blocks::AIR);
        region.block_items.remove(&banner_pos);
        for _ in 0..16 {
            tick_local(&mut region);
        }
        assert!(
            region
                .global
                .maps
                .get(0)
                .unwrap()
                .banner_markers()
                .is_empty(),
            "breaking banner must trigger checkBanners removal"
        );
    }

    #[test]
    fn toggle_banner_checks_bounds_tracked_limit_and_allows_locked_maps() {
        let mut data =
            MapItemSavedData::create_fresh(0.0, 0.0, 0, true, false, "minecraft:overworld");
        // 越界位置（rel_x = 63.5 > 63.0）拒绝。
        let outside = BlockPos::new(63, 64, 0);
        assert!(!data.toggle_banner(
            outside,
            Some(MapBanner {
                pos: outside,
                color: 0,
                name: None,
            }),
            None,
        ));
        // 边界内（rel_x = -63.0 和 rel_x = 62.5）接受；空旗帜拒绝。
        let inside = BlockPos::new(62, 64, 0);
        assert!(!data.toggle_banner(inside, None, None));
        assert!(data.toggle_banner(
            inside,
            Some(MapBanner {
                pos: inside,
                color: 0,
                name: None,
            }),
            None,
        ));
        // 锁定地图仍允许切换旗帜（26.3 字节码 toggleBanner 不检查 locked）。
        let mut locked = data.locked();
        assert!(locked.toggle_banner(
            inside,
            Some(MapBanner {
                pos: inside,
                color: 0,
                name: None,
            }),
            None,
        ));
        assert!(locked.banner_markers().is_empty());

        // trackedDecorationCount > 256 时拒绝新增，但仍允许取消已有标记。
        data.tracked_decoration_count = TRACKED_DECORATION_LIMIT + 1;
        let other = BlockPos::new(1, 64, 1);
        assert!(!data.toggle_banner(
            other,
            Some(MapBanner {
                pos: other,
                color: 1,
                name: None,
            }),
            None,
        ));
        assert!(data.toggle_banner(
            inside,
            Some(MapBanner {
                pos: inside,
                color: 0,
                name: None,
            }),
            None,
        ));
    }
}
