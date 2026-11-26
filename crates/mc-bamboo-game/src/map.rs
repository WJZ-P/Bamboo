//! 地图数据、索引与持久化：对应官方 `MapIndex`、`MapItemSavedData`、
//! `MapBanner`、`MapFrame`、`MapDecoration`、`HoldingPlayer` 与 `MapPatch`。
//!
//! 26.3 文件布局（以字节码为准）：
//! - `MapIndex.TYPE`：`minecraft:maps/last_id` → `data/minecraft/maps/last_id.dat`
//! - `MapItemSavedData.type(MapId)`：`minecraft:maps/<id>` → `data/minecraft/maps/<id>.dat`
//!
//! Map data, index, and persistence matching vanilla 26.3 (`MapIndex`,
//! `MapItemSavedData`, `MapBanner`, `MapFrame`, `HoldingPlayer`, `MapPatch`).

use std::collections::BTreeMap;

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::component_registries::registry_names::MAP_DECORATION_TYPE;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::text::checked_component,
    packet::play::{MapDecoration, MapItemData, MapPatch},
    text::Component,
};

use crate::inbound::PlayerId;

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

/// 地图索引与所有已加载地图的集合。
///
/// Container holding the [`MapIndex`] and all loaded [`MapItemSavedData`] entries.
#[derive(Debug, Clone, Default)]
pub struct MapStore {
    pub index: MapIndex,
    pub maps: BTreeMap<i32, MapItemSavedData>,
}

impl MapStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: i32) -> Option<&MapItemSavedData> {
        self.maps.get(&id)
    }

    pub fn get_mut(&mut self, id: i32) -> Option<&mut MapItemSavedData> {
        self.maps.get_mut(&id)
    }

    /// 分配一个新地图 ID（`ServerLevel.getFreeMapId`）。
    pub fn next_map_id(&mut self) -> i32 {
        self.index.next_map_id()
    }

    /// 写入或替换一份地图数据（`ServerLevel.setMapData`）。
    pub fn insert(&mut self, id: i32, mut data: MapItemSavedData) {
        data.dirty = true;
        self.maps.insert(id, data);
    }

    /// 拆岛时把中心落在目标岛的地图搬过去，两边各自继承当前 `last_map_id`。
    pub(crate) fn split_off(&mut self, mut inside: impl FnMut(&BlockPos) -> bool) -> Self {
        let mut out = Self {
            index: MapIndex::new(self.index.last_map_id),
            maps: BTreeMap::new(),
        };
        let moved: Vec<i32> = self
            .maps
            .iter()
            .filter_map(|(&id, data)| {
                let center = BlockPos::new(data.center_x, 0, data.center_z);
                inside(&center).then_some(id)
            })
            .collect();
        for id in moved {
            if let Some(data) = self.maps.remove(&id) {
                out.maps.insert(id, data);
            }
        }
        out
    }

    /// 合岛时吸收另一座岛的地图表。
    pub(crate) fn absorb(&mut self, other: Self) {
        if other.index.last_map_id > self.index.last_map_id {
            self.index.last_map_id = other.index.last_map_id;
        }
        self.index.dirty |= other.index.dirty;
        for (id, data) in other.maps {
            self.maps.insert(id, data);
        }
    }

    /// 存盘时把各停放岛的 `MapStore` 汇总成一份。
    pub(crate) fn merge_for_save<'a>(stores: impl IntoIterator<Item = &'a Self>) -> Self {
        let mut merged = Self::default();
        for store in stores {
            if store.index.last_map_id > merged.index.last_map_id {
                merged.index.last_map_id = store.index.last_map_id;
            }
            merged.index.dirty |= store.index.dirty;
            for (&id, data) in &store.maps {
                merged.maps.insert(id, data.clone());
            }
        }
        merged
    }
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
}
