//! 官方 `net.minecraft.world.level.border.WorldBorder` 的状态和数学。
//!
//! 边界按维度各有一份，这里只放纯逻辑：状态、距离、范围判断、夹取、碰撞形状和渐变推进，
//! 不认识网络、存档文件，也不认识世界线程。改变状态的七个方法返回 [`BorderEvent`]，
//! 由游戏层照 `BorderChangeListener` 翻译成对应的包。
//!
//! State and maths of vanilla `net.minecraft.world.level.border.WorldBorder`.
//!
//! Each dimension owns one border. This module is pure logic: state, distance, bounds
//! checks, clamping, the collision shape and lerp progression. It knows nothing about the
//! network, save files or the world thread. The seven mutators return a [`BorderEvent`] that
//! the game layer turns into a packet, mirroring `BorderChangeListener`.

use mc_bamboo_core::{Aabb, BlockPos, ChunkPos, Vec3};

/// `WorldBorder.MAX_SIZE`，官方是 `5.999997E7F` 这个 float 提升成 double。
///
/// `WorldBorder.MAX_SIZE`: the float `5.999997E7F` widened to a double.
pub const MAX_SIZE: f64 = 59_999_968.0;
/// `WorldBorder.MAX_CENTER_COORDINATE`，`2.9999984E7`。
pub const MAX_CENTER_COORDINATE: f64 = 29_999_984.0;
/// `WorldBorder.absoluteMaxSize` 的初值，`MinecraftServer.getAbsoluteMaxWorldSize()`。
///
/// The initial `absoluteMaxSize`, from `MinecraftServer.getAbsoluteMaxWorldSize()`.
pub const ABSOLUTE_MAX_SIZE: i32 = 29_999_984;
/// `1.0E-5F` 这个 float 提升成 double，官方在 `isWithinBounds(AABB)` 和
/// `clampVec3ToBound` 里都用它。
///
/// The float `1.0E-5F` widened to a double; vanilla uses it in
/// `isWithinBounds(AABB)` and `clampVec3ToBound`.
const EDGE: f64 = 1.0e-5_f32 as f64;
/// `Shapes.INFINITY` 的有限替身。碰撞解算只看边界墙的那一侧，用足够大的有限值
/// 和无穷大的结果逐位相同，还避开了 `joinIsNotEmpty` 里 `inf - inf` 的 NaN。
///
/// A finite stand-in for `Shapes.INFINITY`. Collision resolution only looks at the
/// wall's inner side, so a large finite bound gives the same result while avoiding
/// the `inf - inf` NaN inside `joinIsNotEmpty`.
const OUTSIDE: f64 = 1.0e15;

/// 官方 `BorderStatus`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStatus {
    Stationary,
    Shrinking,
    Growing,
}

/// `BorderChangeListener` 的七个回调。两个伤害相关的回调官方不发包，但状态变更照样记账。
///
/// The seven `BorderChangeListener` callbacks. Vanilla sends no packet for the two
/// damage-related ones, but the state change is still reported.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BorderEvent {
    /// `onSetSize`。
    SetSize { size: f64 },
    /// `onLerpSize`。
    LerpSize {
        from: f64,
        to: f64,
        ticks: i64,
        game_time: i64,
    },
    /// `onSetCenter`。
    SetCenter { x: f64, z: f64 },
    /// `onSetWarningTime`。
    SetWarningTime { time: i32 },
    /// `onSetWarningBlocks`。
    SetWarningBlocks { blocks: i32 },
    /// `onSetDamagePerBlock`，官方的监听器什么都不做。
    ///
    /// `onSetDamagePerBlock`: the vanilla listener does nothing.
    SetDamagePerBlock { amount: f64 },
    /// `onSetSafeZone`，官方的监听器什么都不做。
    ///
    /// `onSetSafeZone`: the vanilla listener does nothing.
    SetSafeZone { zone: f64 },
}

/// 官方 `WorldBorder.Settings` 的九个字段，名字和它的 `Codec` 一一对应。
///
/// The nine fields of vanilla `WorldBorder.Settings`, named after its `Codec` keys.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderSettings {
    /// `center_x`。
    pub center_x: f64,
    /// `center_z`。
    pub center_z: f64,
    /// `damage_per_block`。
    pub damage_per_block: f64,
    /// `safe_zone`。
    pub safe_zone: f64,
    /// `warning_blocks`。
    pub warning_blocks: i32,
    /// `warning_time`。
    pub warning_time: i32,
    /// `size`。
    pub size: f64,
    /// `lerp_time`。
    pub lerp_time: i64,
    /// `lerp_target`。
    pub lerp_target: f64,
}

impl BorderSettings {
    /// 官方 `WorldBorder.Settings.DEFAULT`（26.3 的 `warning_time` 是 300 刻）。
    ///
    /// Vanilla `WorldBorder.Settings.DEFAULT` (26.3 uses 300 ticks for `warning_time`).
    pub const DEFAULT: Self = Self {
        center_x: 0.0,
        center_z: 0.0,
        damage_per_block: 0.2,
        safe_zone: 5.0,
        warning_blocks: 5,
        warning_time: 300,
        size: MAX_SIZE,
        lerp_time: 0,
        lerp_target: 0.0,
    };
}

/// `WorldBorder.BorderExtent` 的两种实现。
///
/// The two `WorldBorder.BorderExtent` implementations.
#[derive(Debug, Clone, PartialEq)]
enum Extent {
    /// `StaticBorderExtent`。盒子在中心或绝对最大值变化时重算。
    ///
    /// `StaticBorderExtent`. The box is rebuilt when the center or the absolute
    /// maximum changes.
    Static {
        size: f64,
        min_x: f64,
        min_z: f64,
        max_x: f64,
        max_z: f64,
    },
    /// `MovingBorderExtent`。
    Moving {
        from: f64,
        to: f64,
        lerp_begin: i64,
        lerp_end: i64,
        lerp_duration: f64,
        lerp_progress: i64,
        size: f64,
        previous_size: f64,
    },
}

/// 官方 `WorldBorder`。游戏层持有权威副本，方块逻辑和实体逻辑各读一份快照。
///
/// Vanilla `WorldBorder`. The game layer owns the authoritative copy; block and
/// entity logic each read a snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldBorder {
    settings: BorderSettings,
    initialized: bool,
    damage_per_block: f64,
    safe_zone: f64,
    warning_time: i32,
    warning_blocks: i32,
    center_x: f64,
    center_z: f64,
    absolute_max_size: i32,
    extent: Extent,
}

impl Default for WorldBorder {
    /// 官方 `new WorldBorder()` 之后、`getWorldBorder()` 第一次调用时的
    /// `applyInitialSettings(gameTime)` 已经跑完，开服即默认边界。
    ///
    /// Vanilla `new WorldBorder()` after the first `getWorldBorder()` has run
    /// `applyInitialSettings(gameTime)`: the default border is live at startup.
    fn default() -> Self {
        let mut border = Self::from_settings(BorderSettings::DEFAULT);
        border.apply_initial_settings(0);
        border
    }
}

impl WorldBorder {
    /// 官方 `WorldBorder(Settings)`：只把存档里的设置存起来，字段还是构造器初值，
    /// 等 [`apply_initial_settings`](Self::apply_initial_settings) 再套上去。
    ///
    /// Vanilla `WorldBorder(Settings)`: store the saved settings, leaving the fields
    /// at their constructor defaults until [`apply_initial_settings`](Self::apply_initial_settings).
    pub fn from_settings(settings: BorderSettings) -> Self {
        Self {
            settings,
            initialized: false,
            damage_per_block: 0.2,
            safe_zone: 5.0,
            warning_time: 15,
            warning_blocks: 5,
            center_x: 0.0,
            center_z: 0.0,
            absolute_max_size: ABSOLUTE_MAX_SIZE,
            extent: Extent::Static {
                size: MAX_SIZE,
                min_x: 0.0,
                min_z: 0.0,
                max_x: 0.0,
                max_z: 0.0,
            },
        }
    }

    /// 从存档恢复：官方 `CODEC.xmap(WorldBorder::new, Settings::new)` 先造出待初始化的
    /// 边界，再由 `ServerLevel.getWorldBorder()` 的 `applyInitialSettings(gameTime)` 套用设置。
    /// 读盘时没有在线玩家，返回的事件被丢掉；字段和官方的 `Settings` 编解码一一对应。
    ///
    /// Restore from a save: vanilla `CODEC.xmap(WorldBorder::new, Settings::new)` builds a
    /// pending border, then `ServerLevel.getWorldBorder()` applies the settings with the
    /// current game time. Nobody is online during load, so the events are dropped. The
    /// fields match vanilla's `Settings` codec one for one.
    pub fn restore(settings: BorderSettings, game_time: i64) -> Self {
        let mut border = Self::from_settings(settings);
        border.apply_initial_settings(game_time);
        border
    }

    /// 导出成官方 `WorldBorder.Settings` 的九个字段，也就是 `Settings(WorldBorder)`。
    ///
    /// Export the nine fields of vanilla `WorldBorder.Settings`, i.e. `Settings(WorldBorder)`.
    pub fn export_settings(&self) -> BorderSettings {
        BorderSettings {
            center_x: self.center_x,
            center_z: self.center_z,
            damage_per_block: self.damage_per_block,
            safe_zone: self.safe_zone,
            warning_blocks: self.warning_blocks,
            warning_time: self.warning_time,
            size: self.extent.size(),
            lerp_time: self.extent.lerp_time(),
            lerp_target: self.extent.lerp_target(),
        }
    }

    /// 官方 `applyInitialSettings`：只跑一次，按顺序套用设置并发对应的事件。
    ///
    /// Vanilla `applyInitialSettings`: runs once, applying the settings in order and
    /// emitting the corresponding events.
    pub fn apply_initial_settings(&mut self, game_time: i64) -> Vec<BorderEvent> {
        if self.initialized {
            return Vec::new();
        }
        let settings = self.settings;
        let mut events = vec![
            self.set_center(settings.center_x, settings.center_z),
            self.set_damage_per_block(settings.damage_per_block),
            self.set_safe_zone(settings.safe_zone),
            self.set_warning_blocks(settings.warning_blocks),
            self.set_warning_time(settings.warning_time),
        ];
        if settings.lerp_time > 0 {
            events.push(self.lerp_size_between(
                settings.size,
                settings.lerp_target,
                settings.lerp_time,
                game_time,
            ));
        } else {
            events.push(self.set_size(settings.size));
        }
        self.initialized = true;
        events
    }

    /// 官方 `tick`，推进一拍渐变；静态边界什么也不做。
    ///
    /// Vanilla `tick`: advance the lerp by one tick; a static border does nothing.
    pub fn tick(&mut self) {
        let finished = match self.extent {
            Extent::Static { .. } => None,
            Extent::Moving {
                from,
                to,
                lerp_duration,
                ref mut lerp_progress,
                ref mut size,
                ref mut previous_size,
                ..
            } => {
                *lerp_progress -= 1;
                *previous_size = *size;
                let progress = (lerp_duration - *lerp_progress as f64) / lerp_duration;
                *size = if progress < 1.0 {
                    lerp(progress, from, to)
                } else {
                    to
                };
                (*lerp_progress <= 0).then_some(to)
            }
        };
        if let Some(to) = finished {
            self.extent =
                Self::static_box(self.center_x, self.center_z, self.absolute_max_size, to);
        }
    }

    /// `StaticBorderExtent.updateBox` 算出的四个边界。
    fn static_box(center_x: f64, center_z: f64, absolute_max_size: i32, size: f64) -> Extent {
        let max = f64::from(absolute_max_size);
        Extent::Static {
            size,
            min_x: clamp(center_x - size / 2.0, -max, max),
            min_z: clamp(center_z - size / 2.0, -max, max),
            max_x: clamp(center_x + size / 2.0, -max, max),
            max_z: clamp(center_z + size / 2.0, -max, max),
        }
    }

    /// `Mth.lerp(delta, previousSize, size)`：渐变边界在 `deltaPartialTick` 时的尺寸。
    ///
    /// `Mth.lerp(delta, previousSize, size)`: a moving border's size at a partial tick.
    fn lerped_size(&self, delta: f32) -> f64 {
        match self.extent {
            Extent::Static { size, .. } => size,
            Extent::Moving {
                size,
                previous_size,
                ..
            } => lerp(f64::from(delta), previous_size, size),
        }
    }

    /// `getMinX(deltaPartialTick)`，渐变边界按上一刻和这一刻的尺寸插值。
    ///
    /// `getMinX(deltaPartialTick)`: a moving border interpolates between its previous
    /// and current sizes.
    fn min_x_at(&self, delta: f32) -> f64 {
        let size = self.lerped_size(delta) / 2.0;
        clamp(
            self.center_x - size,
            -f64::from(self.absolute_max_size),
            f64::from(self.absolute_max_size),
        )
    }

    /// `getMaxX(deltaPartialTick)`。
    fn max_x_at(&self, delta: f32) -> f64 {
        let size = self.lerped_size(delta) / 2.0;
        clamp(
            self.center_x + size,
            -f64::from(self.absolute_max_size),
            f64::from(self.absolute_max_size),
        )
    }

    /// `getMinZ(deltaPartialTick)`。
    fn min_z_at(&self, delta: f32) -> f64 {
        let size = self.lerped_size(delta) / 2.0;
        clamp(
            self.center_z - size,
            -f64::from(self.absolute_max_size),
            f64::from(self.absolute_max_size),
        )
    }

    /// `getMaxZ(deltaPartialTick)`。
    fn max_z_at(&self, delta: f32) -> f64 {
        let size = self.lerped_size(delta) / 2.0;
        clamp(
            self.center_z + size,
            -f64::from(self.absolute_max_size),
            f64::from(self.absolute_max_size),
        )
    }

    pub fn min_x(&self) -> f64 {
        self.min_x_at(0.0)
    }

    pub fn max_x(&self) -> f64 {
        self.max_x_at(0.0)
    }

    pub fn min_z(&self) -> f64 {
        self.min_z_at(0.0)
    }

    pub fn max_z(&self) -> f64 {
        self.max_z_at(0.0)
    }

    pub const fn center_x(&self) -> f64 {
        self.center_x
    }

    pub const fn center_z(&self) -> f64 {
        self.center_z
    }

    pub fn size(&self) -> f64 {
        self.extent.size()
    }

    pub fn lerp_time(&self) -> i64 {
        self.extent.lerp_time()
    }

    pub fn lerp_target(&self) -> f64 {
        self.extent.lerp_target()
    }

    pub fn lerp_speed(&self) -> f64 {
        self.extent.lerp_speed()
    }

    pub fn status(&self) -> BorderStatus {
        self.extent.status()
    }

    pub const fn safe_zone(&self) -> f64 {
        self.safe_zone
    }

    pub const fn damage_per_block(&self) -> f64 {
        self.damage_per_block
    }

    pub const fn warning_time(&self) -> i32 {
        self.warning_time
    }

    pub const fn warning_blocks(&self) -> i32 {
        self.warning_blocks
    }

    pub const fn absolute_max_size(&self) -> i32 {
        self.absolute_max_size
    }

    /// 官方 `setCenter`：先改中心、重算静态盒子，再通知监听器。
    ///
    /// Vanilla `setCenter`: update the center, rebuild a static box, then notify.
    pub fn set_center(&mut self, x: f64, z: f64) -> BorderEvent {
        self.center_x = x;
        self.center_z = z;
        self.on_center_change();
        BorderEvent::SetCenter { x, z }
    }

    /// 官方 `setSize`。
    pub fn set_size(&mut self, size: f64) -> BorderEvent {
        self.extent = Self::static_box(self.center_x, self.center_z, self.absolute_max_size, size);
        BorderEvent::SetSize { size }
    }

    /// 官方 `lerpSizeBetween`。`from == to` 时官方直接换成静态边界，但事件仍是 `onLerpSize`。
    ///
    /// Vanilla `lerpSizeBetween`. When `from == to` it still fires `onLerpSize` even
    /// though the extent becomes static.
    pub fn lerp_size_between(
        &mut self,
        from: f64,
        to: f64,
        ticks: i64,
        game_time: i64,
    ) -> BorderEvent {
        if from == to {
            self.extent =
                Self::static_box(self.center_x, self.center_z, self.absolute_max_size, to);
        } else {
            let duration = ticks as f64;
            let size = lerp(0.0, from, to);
            self.extent = Extent::Moving {
                from,
                to,
                lerp_begin: game_time,
                lerp_end: game_time.wrapping_add(ticks),
                lerp_duration: duration,
                lerp_progress: ticks,
                size,
                previous_size: size,
            };
        }
        BorderEvent::LerpSize {
            from,
            to,
            ticks,
            game_time,
        }
    }

    /// 官方 `setDamagePerBlock`。
    pub fn set_damage_per_block(&mut self, amount: f64) -> BorderEvent {
        self.damage_per_block = amount;
        BorderEvent::SetDamagePerBlock { amount }
    }

    /// 官方 `setSafeZone`。
    pub fn set_safe_zone(&mut self, zone: f64) -> BorderEvent {
        self.safe_zone = zone;
        BorderEvent::SetSafeZone { zone }
    }

    /// 官方 `setWarningTime`。
    pub fn set_warning_time(&mut self, time: i32) -> BorderEvent {
        self.warning_time = time;
        BorderEvent::SetWarningTime { time }
    }

    /// 官方 `setWarningBlocks`。
    pub fn set_warning_blocks(&mut self, blocks: i32) -> BorderEvent {
        self.warning_blocks = blocks;
        BorderEvent::SetWarningBlocks { blocks }
    }

    /// 官方 `setAbsoluteMaxSize`，没有监听器。
    ///
    /// Vanilla `setAbsoluteMaxSize`: no listener.
    pub fn set_absolute_max_size(&mut self, max: i32) {
        self.absolute_max_size = max;
        self.on_center_change();
    }

    /// `BorderExtent.onCenterChange` / `onAbsoluteMaxSizeChange`：只有静态边界重算盒子。
    ///
    /// `BorderExtent.onCenterChange` / `onAbsoluteMaxSizeChange`: only a static
    /// extent rebuilds its box.
    fn on_center_change(&mut self) {
        if let Extent::Static { size, .. } = self.extent {
            self.extent =
                Self::static_box(self.center_x, self.center_z, self.absolute_max_size, size);
        }
    }

    /// `isWithinBounds(double, double, double)`：左边含、右边不含。
    ///
    /// `isWithinBounds(double, double, double)`: the lower edge is inclusive, the
    /// upper edge exclusive.
    pub fn is_within_bounds(&self, x: f64, z: f64, margin: f64) -> bool {
        x >= self.min_x() - margin
            && x < self.max_x() + margin
            && z >= self.min_z() - margin
            && z < self.max_z() + margin
    }

    /// `isWithinBounds(BlockPos)` / `isWithinBounds(Vec3)`。
    pub fn is_within_bounds_pos(&self, pos: BlockPos) -> bool {
        self.is_within_bounds(f64::from(pos.x), f64::from(pos.z), 0.0)
    }

    /// `isWithinBounds(ChunkPos)`：两个对角都在界内。
    ///
    /// `isWithinBounds(ChunkPos)`: both opposite corners must be inside.
    pub fn is_within_bounds_chunk(&self, pos: ChunkPos) -> bool {
        let (min_x, min_z) = (f64::from(pos.x * 16), f64::from(pos.z * 16));
        let (max_x, max_z) = (min_x + 15.0, min_z + 15.0);
        self.is_within_bounds(min_x, min_z, 0.0) && self.is_within_bounds(max_x, max_z, 0.0)
    }

    /// `isWithinBounds(AABB)`：`maxX`/`maxZ` 各减 `1.0E-5F`。
    ///
    /// `isWithinBounds(AABB)`: `maxX`/`maxZ` are each reduced by `1.0E-5F`.
    pub fn is_within_bounds_aabb(&self, box_: Aabb) -> bool {
        self.is_within_bounds(box_.min_x, box_.min_z, 0.0)
            && self.is_within_bounds(box_.max_x - EDGE, box_.max_z - EDGE, 0.0)
    }

    /// `clampVec3ToBound`：水平夹进边界，右边减 `1.0E-5F`，竖直不动。
    ///
    /// `clampVec3ToBound`: clamp the horizontal coordinates, subtracting `1.0E-5F`
    /// from the upper edge and leaving Y alone.
    pub fn clamp_vec3(&self, position: Vec3) -> Vec3 {
        Vec3::new(
            clamp(position.x, self.min_x(), self.max_x() - EDGE),
            position.y,
            clamp(position.z, self.min_z(), self.max_z() - EDGE),
        )
    }

    /// `clampToBounds`：`BlockPos.containing(clampVec3ToBound(...))`。
    pub fn clamp_to_bounds(&self, position: Vec3) -> BlockPos {
        BlockPos::containing(self.clamp_vec3(position))
    }

    /// `getDistanceToBorder(x, z)`：四个方向里最近的那个，可以是负数。
    ///
    /// `getDistanceToBorder(x, z)`: the nearest of the four sides; may be negative.
    pub fn distance_to_border(&self, x: f64, z: f64) -> f64 {
        let from_north = z - self.min_z();
        let from_south = self.max_z() - z;
        let from_west = x - self.min_x();
        let from_east = self.max_x() - x;
        from_west.min(from_east).min(from_north).min(from_south)
    }

    /// `isInsideCloseToBorder(Entity, AABB)`。
    pub fn is_inside_close_to_border(&self, x: f64, z: f64, box_: Aabb) -> bool {
        let bb_max = (box_.max_x - box_.min_x)
            .abs()
            .max((box_.max_z - box_.min_z).abs())
            .max(1.0);
        self.distance_to_border(x, z) < bb_max * 2.0 && self.is_within_bounds(x, z, bb_max)
    }

    /// `getCollisionShape()`：`Shapes.join(INFINITY, box(floor(minX), -inf, floor(minZ),
    /// ceil(maxX), +inf, ceil(maxZ)), ONLY_FIRST)`。官方合并结果在两根水平轴上各有三个
    /// 体素格，中心那一格是空的；`VoxelShape.collide` 靠这张 3×3 网格的格线找第一堵墙。
    /// 所以这里按网格切成八个互不相交的单格盒子，而不是把相邻的墙合并成大盒子——
    /// 合并后少了一条格线，碰撞位移会和官方不同。
    ///
    /// 内盒退化成零宽时 `Shapes.create` 返回空形状，合并结果就是整个 INFINITY，
    /// 这里同样只给一个覆盖全空间的盒子（它不挡任何位移，和官方一致）。
    ///
    /// `getCollisionShape()`: `Shapes.join(INFINITY, box(...), ONLY_FIRST)`. The joined
    /// shape has three voxels on each horizontal axis with the centre voxel empty, and
    /// `VoxelShape.collide` finds the first wall using those grid lines. This therefore
    /// returns the eight disjoint single-cell boxes of that 3x3 grid, rather than merging
    /// neighbouring walls into big boxes: a merged wall loses a grid line and resolves a
    /// different movement. When the inner box has zero width, `Shapes.create` returns an
    /// empty shape and the join is the whole of INFINITY, so a single full-space box is
    /// returned (it blocks nothing, matching vanilla).
    pub fn collision_shape(&self) -> Vec<Aabb> {
        let min_x = self.min_x().floor();
        let min_z = self.min_z().floor();
        let max_x = self.max_x().ceil();
        let max_z = self.max_z().ceil();
        if max_x - min_x < 1.0e-7 || max_z - min_z < 1.0e-7 {
            return vec![Aabb::new(
                -OUTSIDE, -OUTSIDE, -OUTSIDE, OUTSIDE, OUTSIDE, OUTSIDE,
            )];
        }
        let xs = [-OUTSIDE, min_x, max_x, OUTSIDE];
        let zs = [-OUTSIDE, min_z, max_z, OUTSIDE];
        let mut boxes = Vec::with_capacity(8);
        for i in 0..3 {
            for j in 0..3 {
                if i == 1 && j == 1 {
                    continue;
                }
                boxes.push(Aabb::new(
                    xs[i],
                    -OUTSIDE,
                    zs[j],
                    xs[i + 1],
                    OUTSIDE,
                    zs[j + 1],
                ));
            }
        }
        boxes
    }

    /// `Entity.collectCollidersIgnoringWorldBorder` 里加边界形状的那一半：只有实体离边界
    /// 够近时才把碰撞形状算进碰撞体，`source` 是实体自己的位置。
    ///
    /// The world-border half of `Entity.collectCollidersIgnoringWorldBorder`: the
    /// collision shape joins the colliders only when the source is close enough.
    pub fn collision_colliders(&self, source_x: f64, source_z: f64, box_: Aabb) -> Vec<Aabb> {
        if self.is_inside_close_to_border(source_x, source_z, box_) {
            self.collision_shape()
        } else {
            Vec::new()
        }
    }

    /// `CollisionGetter.noBorderCollision`：离边界够远时恒真；够近时看边界形状
    /// 和这个盒子有没有正体积重叠。
    ///
    /// `CollisionGetter.noBorderCollision`: always true when far from the border;
    /// otherwise true only when the border shape does not overlap this box.
    pub fn no_border_collision(&self, source_x: f64, source_z: f64, box_: Aabb) -> bool {
        if !self.is_inside_close_to_border(source_x, source_z, box_) {
            return true;
        }
        !self
            .collision_shape()
            .iter()
            .any(|wall| overlaps(*wall, box_))
    }
}

/// 两个 AABB 的正体积重叠，也就是 `Shapes.joinIsNotEmpty(..., AND)`。
///
/// A positive-volume overlap between two AABBs, i.e. `Shapes.joinIsNotEmpty(..., AND)`.
fn overlaps(first: Aabb, second: Aabb) -> bool {
    first.min_x < second.max_x
        && first.max_x > second.min_x
        && first.min_y < second.max_y
        && first.max_y > second.min_y
        && first.min_z < second.max_z
        && first.max_z > second.min_z
}

impl Extent {
    fn size(&self) -> f64 {
        match *self {
            Self::Static { size, .. } | Self::Moving { size, .. } => size,
        }
    }

    fn lerp_time(&self) -> i64 {
        match *self {
            Self::Static { .. } => 0,
            Self::Moving { lerp_progress, .. } => lerp_progress,
        }
    }

    fn lerp_target(&self) -> f64 {
        match *self {
            Self::Static { size, .. } => size,
            Self::Moving { to, .. } => to,
        }
    }

    fn lerp_speed(&self) -> f64 {
        match *self {
            Self::Static { .. } => 0.0,
            Self::Moving {
                from,
                to,
                lerp_begin,
                lerp_end,
                ..
            } => (from - to).abs() / (lerp_end - lerp_begin) as f64,
        }
    }

    fn status(&self) -> BorderStatus {
        match *self {
            Self::Static { .. } => BorderStatus::Stationary,
            Self::Moving { from, to, .. } => {
                if to < from {
                    BorderStatus::Shrinking
                } else {
                    BorderStatus::Growing
                }
            }
        }
    }
}

/// `Mth.lerp(double, double, double)`。
fn lerp(alpha: f64, start: f64, end: f64) -> f64 {
    start + alpha * (end - start)
}

/// `Mth.clamp(double, double, double)`：`value < min ? min : Math.min(value, max)`。
///
/// `Mth.clamp(double, double, double)`: `value < min ? min : Math.min(value, max)`.
fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if max < value {
        max
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn border(size: f64, center_x: f64, center_z: f64) -> WorldBorder {
        let mut border = WorldBorder::default();
        border.set_center(center_x, center_z);
        border.set_size(size);
        border
    }

    #[test]
    fn default_settings_match_the_official_codec() {
        let border = WorldBorder::default();
        // 官方 `Settings(WorldBorder)` 的 `lerpTarget` 来自 `StaticBorderExtent.getLerpTarget()`，
        // 静态边界返回自己的 size；`Settings.DEFAULT.lerpTarget` 的 0 从不使用（lerpTime 也是 0）。
        //
        // Vanilla `Settings(WorldBorder)` takes `lerpTarget` from
        // `StaticBorderExtent.getLerpTarget()`, which returns the size. The 0 in
        // `Settings.DEFAULT.lerpTarget` is never used (lerpTime is 0 too).
        let exported = border.export_settings();
        assert_eq!(exported.lerp_target, MAX_SIZE);
        assert_eq!(
            BorderSettings {
                lerp_target: 0.0,
                ..exported
            },
            BorderSettings::DEFAULT
        );
        assert_eq!(border.size(), MAX_SIZE);
        assert_eq!(border.warning_time(), 300);
        assert_eq!(border.warning_blocks(), 5);
        assert_eq!(border.safe_zone(), 5.0);
        assert_eq!(border.damage_per_block(), 0.2);
        assert_eq!(border.status(), BorderStatus::Stationary);
        assert_eq!(border.min_x(), -MAX_CENTER_COORDINATE);
        assert_eq!(border.max_x(), MAX_CENTER_COORDINATE);
    }

    #[test]
    fn bounds_are_inclusive_on_the_min_and_exclusive_on_the_max() {
        let border = border(10.0, 0.0, 0.0);
        assert!(border.is_within_bounds(-5.0, -5.0, 0.0));
        assert!(!border.is_within_bounds(5.0, 0.0, 0.0));
        assert!(border.is_within_bounds(4.999, 0.0, 0.0));
        assert!(!border.is_within_bounds(0.0, 5.0, 0.0));
        assert!(border.is_within_bounds(-5.0, 0.0, 0.1));
    }

    #[test]
    fn aabb_and_chunk_overloads_match_the_official_edges() {
        let narrow = border(20.0, 0.0, 0.0);
        let inside = Aabb::new(-9.0, 0.0, -9.0, 9.0, 2.0, 9.0);
        assert!(narrow.is_within_bounds_aabb(inside));
        let outside = Aabb::new(-11.0, 0.0, -9.0, 9.0, 2.0, 9.0);
        assert!(!narrow.is_within_bounds_aabb(outside));
        // 区块 (0,0) 的角是 0..15，边界要更大才装得下。
        //
        // Chunk (0,0) spans 0..15, so the border must be wider than the AABB above.
        let mut wide = WorldBorder::default();
        wide.set_size(100.0);
        assert!(wide.is_within_bounds_chunk(ChunkPos::new(0, 0)));
        assert!(!wide.is_within_bounds_chunk(ChunkPos::new(4, 0)));
    }

    #[test]
    fn distance_is_the_nearest_side_and_can_be_negative() {
        let border = border(100.0, 10.0, -20.0);
        assert_eq!(border.distance_to_border(10.0, -20.0), 50.0);
        assert_eq!(border.distance_to_border(-39.0, -20.0), 1.0);
        assert_eq!(border.distance_to_border(-41.0, -20.0), -1.0);
    }

    #[test]
    fn clamping_uses_the_exclusive_upper_edge() {
        let border = border(10.0, 0.0, 0.0);
        let clamped = border.clamp_vec3(Vec3::new(100.0, 64.0, -100.0));
        assert_eq!(clamped.x, 5.0 - EDGE);
        assert_eq!(clamped.z, -5.0);
        assert_eq!(clamped.y, 64.0);
        assert_eq!(
            border.clamp_to_bounds(Vec3::new(100.0, 0.0, 100.0)),
            BlockPos::new(4, 0, 4)
        );
    }

    #[test]
    fn absolute_max_size_clamps_the_box() {
        let mut border = border(MAX_SIZE * 2.0, 0.0, 0.0);
        assert_eq!(border.min_x(), -MAX_CENTER_COORDINATE);
        assert_eq!(border.max_x(), MAX_CENTER_COORDINATE);
        border.set_absolute_max_size(100);
        assert_eq!(border.min_x(), -100.0);
        assert_eq!(border.max_x(), 100.0);
        border.set_center(50.0, -50.0);
        assert_eq!(border.min_x(), -100.0);
        assert_eq!(border.max_z(), 100.0);
    }

    #[test]
    fn center_change_rebuilds_a_static_box_but_not_a_moving_one() {
        let mut border = border(10.0, 0.0, 0.0);
        border.set_center(100.0, 200.0);
        assert_eq!(border.min_x(), 95.0);
        assert_eq!(border.max_z(), 205.0);
        border.lerp_size_between(10.0, 20.0, 10, 0);
        let before = border.min_x();
        border.set_center(200.0, 400.0);
        // 官方 `MovingBorderExtent.onCenterChange` 什么也不做；minX 用的是新中心，
        // 所以尺寸那一项仍是 interpolate 出来的值。
        //
        // Vanilla `MovingBorderExtent.onCenterChange` is empty; minX uses the new center.
        assert_eq!(border.min_x() - before, 100.0);
    }

    #[test]
    fn lerp_progresses_one_tick_at_a_time_and_finishes_at_the_target() {
        let mut border = WorldBorder::default();
        border.set_size(100.0);
        border.lerp_size_between(100.0, 200.0, 10, 1000);
        assert_eq!(border.status(), BorderStatus::Growing);
        assert_eq!(border.lerp_time(), 10);
        assert_eq!(border.lerp_target(), 200.0);
        assert_eq!(border.size(), 100.0);
        for tick in 1..=10 {
            border.tick();
            assert_eq!(border.size(), 100.0 + 10.0 * f64::from(tick));
        }
        assert_eq!(border.size(), 200.0);
        assert_eq!(border.status(), BorderStatus::Stationary);
        assert_eq!(border.lerp_time(), 0);
        assert_eq!(border.lerp_target(), 200.0);
        assert_eq!(border.lerp_speed(), 0.0);
    }

    #[test]
    fn shrinking_uses_the_previous_size_for_the_box_and_reports_its_status() {
        let mut border = WorldBorder::default();
        border.set_size(200.0);
        border.lerp_size_between(200.0, 100.0, 4, 0);
        assert_eq!(border.status(), BorderStatus::Shrinking);
        assert_eq!(border.lerp_speed(), 25.0);
        assert_eq!(border.min_x(), -100.0);
        border.tick();
        assert_eq!(border.size(), 175.0);
        // `getMinX(0.0F)` 用 `previousSize`（200），不是这一刻的 175。
        //
        // `getMinX(0.0F)` uses `previousSize` (200), not the current 175.
        assert_eq!(border.min_x(), -100.0);
        assert_eq!(border.max_x(), 100.0);
    }

    #[test]
    fn a_zero_length_lerp_is_static_but_still_reports_the_event() {
        let mut border = WorldBorder::default();
        let event = border.lerp_size_between(50.0, 50.0, 10, 0);
        assert_eq!(border.status(), BorderStatus::Stationary);
        assert_eq!(border.size(), 50.0);
        assert_eq!(
            event,
            BorderEvent::LerpSize {
                from: 50.0,
                to: 50.0,
                ticks: 10,
                game_time: 0
            }
        );
    }

    #[test]
    fn settings_round_trip_restores_a_lerp_at_the_current_game_time() {
        let mut border = WorldBorder::default();
        border.set_center(12.5, -30.0);
        border.set_damage_per_block(3.0);
        border.set_safe_zone(7.0);
        border.set_warning_time(42);
        border.set_warning_blocks(9);
        border.lerp_size_between(100.0, 400.0, 20, 1000);
        border.tick();
        border.tick();
        let settings = border.export_settings();
        assert_eq!(settings.size, 130.0);
        assert_eq!(settings.lerp_time, 18);
        assert_eq!(settings.lerp_target, 400.0);

        let restored = WorldBorder::restore(settings, 5000);
        assert_eq!(restored.center_x(), 12.5);
        assert_eq!(restored.size(), 130.0);
        assert_eq!(restored.lerp_time(), 18);
        assert_eq!(restored.lerp_target(), 400.0);
        assert_eq!(restored.warning_time(), 42);
        assert_eq!(restored.status(), BorderStatus::Growing);
    }

    #[test]
    fn events_follow_the_official_listener_order() {
        let mut border = WorldBorder::from_settings(BorderSettings {
            center_x: 1.0,
            center_z: 2.0,
            damage_per_block: 1.5,
            safe_zone: 3.0,
            warning_blocks: 7,
            warning_time: 11,
            size: 100.0,
            lerp_time: 5,
            lerp_target: 200.0,
        });
        let events = border.apply_initial_settings(100);
        assert_eq!(events.len(), 6);
        assert_eq!(events[0], BorderEvent::SetCenter { x: 1.0, z: 2.0 });
        assert_eq!(events[1], BorderEvent::SetDamagePerBlock { amount: 1.5 });
        assert_eq!(events[2], BorderEvent::SetSafeZone { zone: 3.0 });
        assert_eq!(events[3], BorderEvent::SetWarningBlocks { blocks: 7 });
        assert_eq!(events[4], BorderEvent::SetWarningTime { time: 11 });
        assert_eq!(
            events[5],
            BorderEvent::LerpSize {
                from: 100.0,
                to: 200.0,
                ticks: 5,
                game_time: 100
            }
        );
        assert!(border.apply_initial_settings(200).is_empty());
    }

    #[test]
    fn the_border_shape_only_joins_colliders_near_the_edge() {
        let border = border(100.0, 0.0, 0.0);
        let box_ = Aabb::new(-1.0, 0.0, -1.0, 1.0, 2.0, 1.0);
        assert!(border.collision_colliders(0.0, 0.0, box_).is_empty());
        let near = border.collision_colliders(48.0, 0.0, box_);
        // 3×3 网格去掉中心那一格，剩下八格。
        //
        // The 3x3 grid minus its centre cell leaves eight boxes.
        assert_eq!(near.len(), 8);
        assert_eq!(near[0].max_x, border.min_x().floor());
        assert_eq!(near[8 - 1].min_x, border.max_x().ceil());
    }
}
