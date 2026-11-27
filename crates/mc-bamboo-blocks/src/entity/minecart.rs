//! 矿车实体，官方 `AbstractMinecart` 及它的几个子类，运动走 `OldMinecartBehavior`。
//!
//! 实验特性 `minecart_improvements` 的 `NewMinecartBehavior` 不在范围里。
//!
//! Minecart entities, vanilla `AbstractMinecart` and its subclasses, moved by
//! `OldMinecartBehavior`. The experimental `NewMinecartBehavior` is out of scope.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{BlockAncestor, blocks, items, registries::entity_type, tags::block_tags};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    BlockState,
    entity::{
        Entity, EntityKind, EntityUid, PrimedTnt,
        ctx::{BlockView, Command, EntityCtx},
    },
    state::{POWERED, RAIL_SHAPE, RailShape},
};

/// 矿车的种类；注册表实体类型 ID 决定它，物品放置时反过来映射。
///
/// The minecart variant; the registry entity type decides it, and placement
/// maps an item back to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinecartKind {
    Rideable,
    Chest,
    Furnace,
    Tnt,
    Hopper,
    CommandBlock,
}

impl MinecartKind {
    /// 注册表实体类型 ID。
    ///
    /// The registry entity type id.
    pub const fn entity_type(self) -> i32 {
        match self {
            Self::Rideable => entity_type::MINECART,
            Self::Chest => entity_type::CHEST_MINECART,
            Self::Furnace => entity_type::FURNACE_MINECART,
            Self::Tnt => entity_type::TNT_MINECART,
            Self::Hopper => entity_type::HOPPER_MINECART,
            Self::CommandBlock => entity_type::COMMAND_BLOCK_MINECART,
        }
    }

    /// 放置它的物品。
    ///
    /// The item that places it.
    pub const fn item(self) -> i32 {
        match self {
            Self::Rideable => items::MINECART,
            Self::Chest => items::CHEST_MINECART,
            Self::Furnace => items::FURNACE_MINECART,
            Self::Tnt => items::TNT_MINECART,
            Self::Hopper => items::HOPPER_MINECART,
            Self::CommandBlock => items::COMMAND_BLOCK_MINECART,
        }
    }

    /// 按注册表实体类型 ID 认矿车；不是矿车返回 `None`。
    ///
    /// Resolve a minecart kind from a registry entity type id.
    pub const fn from_entity_type(id: i32) -> Option<Self> {
        Some(match id {
            entity_type::MINECART => Self::Rideable,
            entity_type::CHEST_MINECART => Self::Chest,
            entity_type::FURNACE_MINECART => Self::Furnace,
            entity_type::TNT_MINECART => Self::Tnt,
            entity_type::HOPPER_MINECART => Self::Hopper,
            entity_type::COMMAND_BLOCK_MINECART => Self::CommandBlock,
            _ => return None,
        })
    }

    /// 按物品认矿车；不是矿车物品返回 `None`。
    ///
    /// Resolve a minecart kind from an item id.
    pub const fn from_item(item: i32) -> Option<Self> {
        Some(match item {
            items::MINECART => Self::Rideable,
            items::CHEST_MINECART => Self::Chest,
            items::FURNACE_MINECART => Self::Furnace,
            items::TNT_MINECART => Self::Tnt,
            items::HOPPER_MINECART => Self::Hopper,
            items::COMMAND_BLOCK_MINECART => Self::CommandBlock,
            _ => return None,
        })
    }

    /// `getDefaultDisplayOffset`：运输矿车 8、漏斗矿车 1，其余 6。
    ///
    /// `getDefaultDisplayOffset`: 8 for a chest cart, 1 for a hopper, 6 otherwise.
    pub const fn default_display_offset(self) -> i32 {
        match self {
            Self::Chest => 8,
            Self::Hopper => 1,
            _ => 6,
        }
    }

    /// `isRideable`：只有普通矿车能坐。
    ///
    /// `isRideable`: only the plain minecart is rideable.
    pub const fn is_rideable(self) -> bool {
        matches!(self, Self::Rideable)
    }

    /// `isFurnace`。
    pub const fn is_furnace(self) -> bool {
        matches!(self, Self::Furnace)
    }

    /// 容器格数：运输矿车 27、漏斗矿车 5，其余没有容器。
    ///
    /// Container size: 27 for a chest cart, 5 for a hopper, none otherwise.
    pub const fn container_size(self) -> usize {
        match self {
            Self::Chest => 27,
            Self::Hopper => 5,
            _ => 0,
        }
    }
}

/// 一个矿车的全部状态，字段照 `AbstractMinecart` / `VehicleEntity` 和各子类。
///
/// The full state of one minecart, mirroring `AbstractMinecart` / `VehicleEntity`
/// and the subclasses.
#[derive(Debug, Clone, PartialEq)]
pub struct Minecart {
    /// 注册表实体类型 ID，同时决定 [`MinecartKind`]。
    ///
    /// The registry entity type id; also determines the [`MinecartKind`].
    pub entity_type: i32,
    /// 乘客列表，骑乘层读它。
    ///
    /// The passenger list, read by the riding layer.
    pub passengers: Vec<EntityUid>,
    /// `DATA_ID_HURT`，默认 0。
    pub hurt_time: i32,
    /// `DATA_ID_HURTDIR`，默认 1。
    pub hurt_dir: i32,
    /// `DATA_ID_DAMAGE`，默认 0。
    pub damage: f32,
    /// `DATA_ID_CUSTOM_DISPLAY_BLOCK`，默认空。
    pub custom_display_block: Option<BlockState>,
    /// `DATA_ID_DISPLAY_OFFSET`。
    pub display_offset: i32,
    /// `onRails`。
    pub on_rails: bool,
    /// `flipped`。
    pub flipped: bool,
    /// `fuse`，默认 -1（未点燃）。
    pub fuse: i32,
    /// `explosion_power`，默认 4。
    pub explosion_power: f32,
    /// `explosion_speed_factor`，默认 1。
    pub explosion_speed_factor: f32,
    /// `fuel`，默认 0。
    pub fuel: i32,
    /// `push`，默认零向量。
    pub push: Vec3,
    /// 漏斗矿车的 `enabled`，默认 true。
    pub enabled: bool,
    /// 容器矿车的格子：运输矿车 27 格、漏斗矿车 5 格，其余为空。
    ///
    /// Container slots: 27 for a chest cart, 5 for a hopper cart, none otherwise.
    pub items: Vec<ItemStack>,
    /// `ContainerEntity.lootTable`。
    pub loot_table: Option<String>,
    /// `ContainerEntity.lootTableSeed`。
    pub loot_table_seed: i64,
    /// `MinecartHopper.consumedItemThisFrame`：每刻最多吸一次。
    pub consumed_item_this_frame: bool,
    /// 游戏层每刻同步进来的第一玩家乘客 `ServerPlayer.getLastClientMoveIntent()`，
    /// 瞬态字段、不存档。
    ///
    /// Transient `ServerPlayer.getLastClientMoveIntent()` of the first player
    /// passenger, synced by the game layer each tick and not saved.
    pub rider_intent: Vec3,
}

impl Minecart {
    /// 按种类新建一辆矿车。
    ///
    /// Build a fresh minecart of the given kind.
    pub fn new(kind: MinecartKind) -> Self {
        Self {
            entity_type: kind.entity_type(),
            passengers: Vec::new(),
            hurt_time: 0,
            hurt_dir: 1,
            damage: 0.0,
            custom_display_block: None,
            display_offset: kind.default_display_offset(),
            on_rails: false,
            flipped: false,
            fuse: -1,
            explosion_power: 4.0,
            explosion_speed_factor: 1.0,
            fuel: 0,
            push: Vec3::default(),
            enabled: true,
            items: vec![ItemStack::empty(); kind.container_size()],
            loot_table: None,
            loot_table_seed: 0,
            consumed_item_this_frame: false,
            rider_intent: Vec3::default(),
        }
    }

    /// 这辆车有没有容器（运输 / 漏斗矿车）。
    ///
    /// Whether this cart has a container (chest / hopper cart).
    pub const fn is_container(&self) -> bool {
        matches!(self.kind(), MinecartKind::Chest | MinecartKind::Hopper)
    }

    /// 这辆车的种类。
    ///
    /// This cart's kind.
    pub const fn kind(&self) -> MinecartKind {
        match MinecartKind::from_entity_type(self.entity_type) {
            Some(kind) => kind,
            None => MinecartKind::Rideable,
        }
    }

    /// 掉落时生成的物品。
    ///
    /// The item dropped when destroyed.
    pub const fn drop_item(&self) -> i32 {
        self.kind().item()
    }

    /// `MinecartFurnace.addFuel`：燃料物品、未超上限时加 3600 tick，并把推力
    /// 记成「矿车位置减玩家位置」的水平向量。
    ///
    /// `MinecartFurnace.addFuel`: with a valid fuel item and room left, add 3600
    /// ticks and set the push to the horizontal vector from the player to the cart.
    pub fn add_fuel(&mut self, from: Vec3, position: Vec3, fuel_item: bool) -> bool {
        if !fuel_item || self.fuel + 3600 > 32000 {
            return false;
        }
        self.fuel += 3600;
        if self.fuel > 0 {
            self.push = position.subtract(from).multiply(1.0, 0.0, 1.0);
        }
        true
    }
}

/// 矿车公开的那一半，给同步和存档读。
///
/// The public half of a minecart, read by sync and storage.
#[derive(Debug, Clone, PartialEq)]
pub struct MinecartPublic {
    pub entity_type: i32,
    pub passengers: Vec<EntityUid>,
    pub hurt_time: i32,
    pub hurt_dir: i32,
    pub damage: f32,
    /// `DATA_ID_CUSTOM_DISPLAY_BLOCK`，方块状态 ID。
    pub custom_display_block: Option<i32>,
    pub display_offset: i32,
    /// `MinecartFurnace.DATA_ID_FUEL`。
    pub has_fuel: bool,
    /// `AbstractMinecart.flipped`：`OldMinecartBehavior.getMotionDirection` 下车方向用。
    pub flipped: bool,
}

impl MinecartPublic {
    /// 从完整状态抄出公开的一半。
    ///
    /// Copy the public half out of the full state.
    pub fn of(cart: &Minecart) -> Self {
        Self {
            entity_type: cart.entity_type,
            passengers: cart.passengers.clone(),
            hurt_time: cart.hurt_time,
            hurt_dir: cart.hurt_dir,
            damage: cart.damage,
            custom_display_block: cart.custom_display_block.map(|state| state.0 as i32),
            display_offset: cart.display_offset,
            has_fuel: cart.kind().is_furnace() && cart.fuel > 0,
            flipped: cart.flipped,
        }
    }
}

/// `Entity.getPassengerAttachmentPoint` 的缺省值：矿车没有覆写（村民 / 流浪商人
/// 走 `LOWERED_PASSENGER_ATTACHMENT` 零向量），玩家乘客落在车体高度上。
///
/// The default `Entity.getPassengerAttachmentPoint`: minecarts do not override it
/// (villagers / wandering traders get the lowered zero vector), so a player
/// passenger sits at the cart's height.
pub fn passenger_attachment_point() -> Vec3 {
    let height = mc_bamboo_data::entity_types::FACTS
        .get(entity_type::MINECART as usize)
        .map_or(0.7_f32, |facts| facts.height);
    Vec3::new(0.0, f64::from(height), 0.0)
}

/// `OldMinecartBehavior` 的四个斜坡形状每刻加的速度；水里再乘 0.2。
///
/// The per-tick slope push of `OldMinecartBehavior`, times 0.2 in water.
const SLOPE_FACTOR: f64 = 0.0078125;

/// `AbstractMinecart.exits`：形状两端的出口向量（x, y, z），含斜坡的 y。
///
/// `AbstractMinecart.exits`: the two exit vectors (x, y, z), including slope y.
fn exits(shape: RailShape) -> ([i32; 3], [i32; 3]) {
    use RailShape::*;
    match shape {
        NorthSouth => ([0, 0, -1], [0, 0, 1]),
        EastWest => ([-1, 0, 0], [1, 0, 0]),
        AscendingEast => ([-1, -1, 0], [1, 0, 0]),
        AscendingWest => ([-1, 0, 0], [1, -1, 0]),
        AscendingNorth => ([0, 0, -1], [0, -1, 1]),
        AscendingSouth => ([0, -1, -1], [0, 0, 1]),
        SouthEast => ([0, 0, 1], [1, 0, 0]),
        SouthWest => ([0, 0, 1], [-1, 0, 0]),
        NorthWest => ([0, 0, -1], [-1, 0, 0]),
        NorthEast => ([0, 0, -1], [1, 0, 0]),
    }
}

/// `BaseRailBlock.isRail`。
fn is_rail(state: BlockState) -> bool {
    state.is_tagged(block_tags::RAILS) && state.is_a(BlockAncestor::BaseRailBlock)
}

fn horizontal_distance(v: Vec3) -> f64 {
    (v.x * v.x + v.z * v.z).sqrt()
}

fn floor_i(value: f64) -> i32 {
    value.floor() as i32
}

/// `Mth.wrapDegrees`。
fn wrap_degrees(value: f32) -> f32 {
    let mut result = value % 360.0;
    if result >= 180.0 {
        result -= 360.0;
    }
    if result < -180.0 {
        result += 360.0;
    }
    result
}

/// `getCurrentBlockPosOrRailBelow`：脚下还有铁轨时用那一格。
///
/// `getCurrentBlockPosOrRailBelow`: use the cell below when it is a rail.
fn current_rail_pos(ctx: &dyn BlockView, position: Vec3) -> BlockPos {
    let mut pos = BlockPos::containing(position);
    if ctx.block_state(pos.below()).is_tagged(block_tags::RAILS) {
        pos = pos.below();
    }
    pos
}

/// `OldMinecartBehavior.getPos`：把坐标吸附到轨道上；没有铁轨时返回 `None`。
///
/// `OldMinecartBehavior.getPos`: snap a position onto the rail; `None` off rails.
fn get_pos(ctx: &dyn BlockView, x: f64, y: f64, z: f64) -> Option<Vec3> {
    let ix = floor_i(x);
    let mut iy = floor_i(y);
    let iz = floor_i(z);
    if ctx
        .block_state(BlockPos::new(ix, iy - 1, iz))
        .is_tagged(block_tags::RAILS)
    {
        iy -= 1;
    }
    let state = ctx.block_state(BlockPos::new(ix, iy, iz));
    if !is_rail(state) {
        return None;
    }
    let shape = state.get(RAIL_SHAPE);
    let (first, second) = exits(shape);
    let p1 = Vec3::new(
        f64::from(ix) + 0.5 + f64::from(first[0]) * 0.5,
        f64::from(iy) + 0.0625 + f64::from(first[1]) * 0.5,
        f64::from(iz) + 0.5 + f64::from(first[2]) * 0.5,
    );
    let p2 = Vec3::new(
        f64::from(ix) + 0.5 + f64::from(second[0]) * 0.5,
        f64::from(iy) + 0.0625 + f64::from(second[1]) * 0.5,
        f64::from(iz) + 0.5 + f64::from(second[2]) * 0.5,
    );
    let dx = p2.x - p1.x;
    let dy = (p2.y - p1.y) * 2.0;
    let dz = p2.z - p1.z;
    let t = if dx == 0.0 {
        z - f64::from(iz)
    } else if dz == 0.0 {
        x - f64::from(ix)
    } else {
        ((x - p1.x) * dx + (z - p1.z) * dz) * 2.0
    };
    let mut ry = p1.y + dy * t;
    if dy < 0.0 {
        ry += 1.0;
    } else if dy > 0.0 {
        ry += 0.5;
    }
    Some(Vec3::new(p1.x + dx * t, ry, p1.z + dz * t))
}

/// `AbstractMinecart.getMaxSpeed`（转发 behavior）：动力矿车再乘 0.5 / 0.75。
///
/// `AbstractMinecart.getMaxSpeed` (delegated to the behavior): a furnace cart
/// multiplies it by 0.5 on land or 0.75 in water.
fn max_speed(me: &Entity, cart: &Minecart) -> f64 {
    let base = if me.in_water { 0.2 } else { 0.4 };
    if cart.kind().is_furnace() {
        base * if me.in_water { 0.75 } else { 0.5 }
    } else {
        base
    }
}

/// `MinecartFurnace.calculateNewPushAlong`：把推力按当前速度方向重新投影，长度不变。
///
/// `MinecartFurnace.calculateNewPushAlong`: reproject the push along the current
/// velocity, keeping its length.
fn calculate_new_push_along(push: Vec3, v: Vec3) -> Vec3 {
    let push_horizontal = push.x * push.x + push.z * push.z;
    let motion_horizontal = v.x * v.x + v.z * v.z;
    if push_horizontal > 1.0e-4 && motion_horizontal > 0.001 {
        let len_sqr = v.length_sqr();
        if len_sqr != 0.0 {
            let dot = push.x * v.x + push.y * v.y + push.z * v.z;
            // `Vec3.projectedOn`：参数是速度，返回 `v * (push·v) / |v|²`。
            // `Vec3.projectedOn`: the argument is the velocity; it returns
            // `v * (push·v) / |v|²`.
            let projected = v.scale(dot).scale(1.0 / len_sqr);
            return projected.normalize().scale(push.length());
        }
    }
    push
}

/// `AbstractMinecartContainer.applyNaturalSlowdown`：容器越空减速越小（惯性越大），
/// `factor = 0.98 + (15 - 红石信号) * 0.001`，有战利品表时不加这一项。
///
/// `AbstractMinecartContainer.applyNaturalSlowdown`: `factor = 0.98 +
/// (15 - redstone signal) * 0.001`, skipped when a loot table is set.
fn container_slowdown(me: &Entity, cart: &Minecart, v: Vec3) -> Vec3 {
    let mut factor = 0.98_f32;
    if cart.loot_table.is_none() {
        let signal = crate::container::redstone_signal(cart.items.iter(), cart.items.len());
        factor += (15 - signal) as f32 * 0.001;
    }
    if me.in_water {
        factor *= 0.95;
    }
    v.multiply(f64::from(factor), 0.0, f64::from(factor))
}

/// `AbstractMinecart.applyNaturalSlowdown`：容器矿车走填充度版，动力矿车先按推力折算，
/// 其余走通用的 y 清零、有乘客 0.997 / 无 0.96、水中再 0.95。
///
/// `AbstractMinecart.applyNaturalSlowdown`: container carts use the fill-based
/// factor, furnace carts fold in their push, the rest use the generic slowdown.
fn apply_natural_slowdown(me: &Entity, cart: &mut Minecart, v: Vec3) -> Vec3 {
    if cart.is_container() {
        return container_slowdown(me, cart, v);
    }
    let base = if cart.kind().is_furnace() {
        if cart.push.length_sqr() > 1.0e-7 {
            cart.push = calculate_new_push_along(cart.push, v);
            let mut result = v.multiply(0.8, 0.0, 0.8).add(cart.push);
            if me.in_water {
                result = result.scale(0.1);
            }
            result
        } else {
            v.multiply(0.98, 0.0, 0.98)
        }
    } else {
        v
    };
    let factor = if cart.passengers.is_empty() {
        0.96
    } else {
        0.997
    };
    let mut result = base.multiply(factor, 0.0, factor);
    if me.in_water {
        result = result.scale(0.95);
    }
    result
}

/// `DetectorRailBlock.getSearchBB` 里的格子：碰撞盒和 `(pos+0.2 .. pos+0.8)` 相交的
/// 未点亮探测铁轨。
///
/// Cells of `DetectorRailBlock.getSearchBB`: unpowered detector rails whose
/// `(pos+0.2 .. pos+0.8)` box intersects the cart's bounding box.
fn detector_rails(blocks: &dyn BlockView, me: &Entity) -> Vec<BlockPos> {
    let half = f64::from(me.dimensions.0) / 2.0;
    let min = Vec3::new(me.position.x - half, me.position.y, me.position.z - half);
    let max = Vec3::new(
        me.position.x + half,
        me.position.y + f64::from(me.dimensions.1),
        me.position.z + half,
    );
    let mut found = Vec::new();
    for y in floor_i(min.y)..=floor_i(max.y) {
        for z in floor_i(min.z)..=floor_i(max.z) {
            for x in floor_i(min.x)..=floor_i(max.x) {
                let state = blocks.block_state(BlockPos::new(x, y, z));
                if !state.is(blocks::DETECTOR_RAIL) || state.try_get(POWERED).unwrap_or(false) {
                    continue;
                }
                let lo_x = f64::from(x) + 0.2;
                let hi_x = f64::from(x) + 0.8;
                let lo_y = f64::from(y);
                let hi_y = f64::from(y) + 0.8;
                let lo_z = f64::from(z) + 0.2;
                let hi_z = f64::from(z) + 0.8;
                if min.x < hi_x
                    && max.x > lo_x
                    && min.y < hi_y
                    && max.y > lo_y
                    && min.z < hi_z
                    && max.z > lo_z
                {
                    found.push(BlockPos::new(x, y, z));
                }
            }
        }
    }
    found
}

/// `isRedstoneConductor`。
fn is_redstone_conductor(ctx: &dyn BlockView, pos: BlockPos) -> bool {
    ctx.block_state(pos).is_redstone_conductor()
}

/// `poweredRail` 通电时静止矿车沿红石导体方向启动。
///
/// A powered rail starts a still cart toward a neighbouring redstone conductor.
fn start_from_conductor(ctx: &dyn BlockView, pos: BlockPos, shape: RailShape, v: Vec3) -> Vec3 {
    let (dx, dz) = if shape == RailShape::EastWest {
        if is_redstone_conductor(ctx, pos.west()) {
            (0.02, v.z)
        } else if is_redstone_conductor(ctx, pos.east()) {
            (-0.02, v.z)
        } else {
            return v;
        }
    } else if shape == RailShape::NorthSouth {
        if is_redstone_conductor(ctx, pos.north()) {
            (v.x, 0.02)
        } else if is_redstone_conductor(ctx, pos.south()) {
            (v.x, -0.02)
        } else {
            return v;
        }
    } else {
        return v;
    };
    Vec3::new(dx, v.y, dz)
}

/// `OldMinecartBehavior.moveAlongTrack`。
fn move_along_track(me: &mut Entity, cart: &mut Minecart, ctx: &dyn BlockView) {
    let pos = current_rail_pos(ctx, me.position);
    let state = ctx.block_state(pos);
    me.fall_distance = 0.0;
    let x0 = me.position.x;
    let y0 = me.position.y;
    let z0 = me.position.z;
    let start = get_pos(ctx, x0, y0, z0);
    let mut rail_y = f64::from(pos.y);
    let is_powered_rail = state.is(blocks::POWERED_RAIL);
    let powered = is_powered_rail && state.try_get(POWERED).unwrap_or(false);
    let mut brake = is_powered_rail && !powered;
    let shape = state.get(RAIL_SHAPE);

    let mut slope = SLOPE_FACTOR;
    if me.in_water {
        slope *= 0.2;
    }
    let on_slope = match shape {
        RailShape::AscendingEast => {
            me.delta.x -= slope;
            true
        }
        RailShape::AscendingWest => {
            me.delta.x += slope;
            true
        }
        RailShape::AscendingNorth => {
            me.delta.z += slope;
            true
        }
        RailShape::AscendingSouth => {
            me.delta.z -= slope;
            true
        }
        _ => false,
    };
    if on_slope {
        rail_y += 1.0;
    }

    let (first, second) = exits(shape);
    let mut dx = f64::from(second[0] - first[0]);
    let mut dz = f64::from(second[2] - first[2]);
    let len = (dx * dx + dz * dz).sqrt();
    if me.delta.x * dx + me.delta.z * dz < 0.0 {
        dx = -dx;
        dz = -dz;
    }
    let speed = 2.0_f64.min(horizontal_distance(me.delta));
    me.delta = Vec3::new(speed * dx / len, me.delta.y, speed * dz / len);

    // `OldMinecartBehavior.moveAlongTrack`：第一乘客是玩家且有按键移动意图时，
    // 水平速度平方小于 0.01 就加 `intent * 0.001` 并取消未通电动力铁轨刹车。
    //
    // `OldMinecartBehavior.moveAlongTrack`: when the first passenger is a player
    // with a non-zero move intent and horizontal speed² < 0.01, nudge the cart by
    // `intent * 0.001` and suppress the unpowered-rail brake.
    if !cart.passengers.is_empty() && cart.rider_intent.length_sqr() > 0.0 {
        let intent = cart.rider_intent;
        let normalized = intent.normalize();
        let speed_sqr = me.delta.x * me.delta.x + me.delta.z * me.delta.z;
        if normalized.length_sqr() > 0.0 && speed_sqr < 0.01 {
            me.delta = me
                .delta
                .add(Vec3::new(intent.x * 0.001, 0.0, intent.z * 0.001));
            brake = false;
        }
    }

    // 未通电的动力铁轨刹车。
    if brake {
        let speed = horizontal_distance(me.delta);
        if speed < 0.03 {
            me.delta = Vec3::default();
        } else {
            me.delta = me.delta.multiply(0.5, 0.0, 0.5);
        }
    }

    let first_x = f64::from(pos.x) + 0.5 + f64::from(first[0]) * 0.5;
    let first_z = f64::from(pos.z) + 0.5 + f64::from(first[2]) * 0.5;
    let second_x = f64::from(pos.x) + 0.5 + f64::from(second[0]) * 0.5;
    let second_z = f64::from(pos.z) + 0.5 + f64::from(second[2]) * 0.5;
    let track_dx = second_x - first_x;
    let track_dz = second_z - first_z;
    let t = if track_dx == 0.0 {
        z0 - f64::from(pos.z)
    } else if track_dz == 0.0 {
        x0 - f64::from(pos.x)
    } else {
        ((x0 - first_x) * track_dx + (z0 - first_z) * track_dz) * 2.0
    };
    me.position = Vec3::new(first_x + track_dx * t, rail_y, first_z + track_dz * t);

    let factor = if cart.passengers.is_empty() {
        1.0
    } else {
        0.75
    };
    let max = max_speed(me, cart);
    let v = me.delta;
    let movement = Vec3::new(
        (factor * v.x).clamp(-max, max),
        0.0,
        (factor * v.z).clamp(-max, max),
    );
    me.move_entity(ctx, movement);

    if first[1] != 0
        && floor_i(me.position.x) - pos.x == first[0]
        && floor_i(me.position.z) - pos.z == first[2]
    {
        me.position.y += f64::from(first[1]);
    } else if second[1] != 0
        && floor_i(me.position.x) - pos.x == second[0]
        && floor_i(me.position.z) - pos.z == second[2]
    {
        me.position.y += f64::from(second[1]);
    }
    me.delta = apply_natural_slowdown(me, cart, me.delta);

    if let (Some(snapped), Some(start)) = (
        get_pos(ctx, me.position.x, me.position.y, me.position.z),
        start,
    ) {
        let extra = (start.y - snapped.y) * 0.05;
        let v = me.delta;
        let speed = horizontal_distance(v);
        if speed > 0.0 {
            let ratio = (speed + extra) / speed;
            me.delta = v.multiply(ratio, 1.0, ratio);
        }
        me.position.y = snapped.y;
    }

    let ix = floor_i(me.position.x);
    let iz = floor_i(me.position.z);
    if ix != pos.x || iz != pos.z {
        let v = me.delta;
        let speed = horizontal_distance(v);
        me.delta = Vec3::new(
            speed * f64::from(ix - pos.x),
            v.y,
            speed * f64::from(iz - pos.z),
        );
    }

    if powered {
        let v = me.delta;
        let speed = horizontal_distance(v);
        if speed > 0.01 {
            // 官方是 `Vec3.add(x, 0, z)`：y 上的 `-0.0 + 0.0` 会变成正零。
            // Vanilla uses `Vec3.add(x, 0, z)`: `-0.0 + 0.0` becomes a positive zero.
            me.delta = v.add(Vec3::new(v.x / speed * 0.06, 0.0, v.z / speed * 0.06));
        } else {
            me.delta = start_from_conductor(ctx, pos, shape, v);
        }
    }
}

/// `AbstractMinecart.comeOffTrack`：出轨后的普通物理。
///
/// `AbstractMinecart.comeOffTrack`: ordinary physics off the rails.
fn come_off_track(me: &mut Entity, cart: &Minecart, ctx: &dyn BlockView) {
    let max = max_speed(me, cart);
    let v = me.delta;
    me.delta = Vec3::new(v.x.clamp(-max, max), v.y, v.z.clamp(-max, max));
    if me.on_ground {
        me.delta = me.delta.scale(0.5);
    }
    let movement = me.delta;
    me.move_entity(ctx, movement);
    if !me.on_ground {
        me.delta = me.delta.scale(0.95);
    }
}

/// `AbstractMinecart.tick`：`OldMinecartBehavior.tick` 的逐刻顺序。
///
/// `AbstractMinecart.tick`: the per-tick order of `OldMinecartBehavior.tick`.
pub(crate) fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let mut cart = match std::mem::replace(&mut me.kind, EntityKind::Tnt(PrimedTnt::new())) {
        EntityKind::Minecart(cart) => cart,
        other => {
            me.kind = other;
            return;
        }
    };
    if cart.hurt_time > 0 {
        cart.hurt_time -= 1;
    }
    if cart.damage > 0.0 {
        cart.damage -= 1.0;
    }
    // `MinecartHopper.tick` 每刻开头清掉上一刻的「已吸过」标记。
    // `MinecartHopper.tick` clears the per-tick "already sucked" flag.
    cart.consumed_item_this_frame = false;

    let prev_y_rot = me.y_rot;
    let gravity = if me.in_water { 0.005 } else { 0.04 };
    me.delta = me.delta.add(Vec3::new(0.0, -gravity, 0.0));
    let blocks = ctx.blocks();
    let pos = current_rail_pos(blocks, me.position);
    let state = blocks.block_state(pos);
    let on_rails = is_rail(state);
    cart.on_rails = on_rails;
    if on_rails {
        move_along_track(me, &mut cart, ctx.blocks());
        if state.is(blocks::ACTIVATOR_RAIL) {
            activate_minecart(&mut cart, state.try_get(POWERED).unwrap_or(false));
        }
    } else {
        come_off_track(me, &cart, ctx.blocks());
    }
    let detectors = detector_rails(ctx.blocks(), me);
    me.apply_effects_from_blocks(ctx.blocks());

    let dx = me.old_position.x - me.position.x;
    let dz = me.old_position.z - me.position.z;
    if dx * dx + dz * dz > 0.001 {
        me.y_rot = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) as f32;
        if cart.flipped {
            me.y_rot += 180.0;
        }
    }
    let turn = wrap_degrees(me.y_rot - prev_y_rot);
    if turn < -170.0 || turn > 170.0 {
        me.y_rot += 180.0;
        cart.flipped = !cart.flipped;
    }
    me.y_rot %= 360.0;
    me.set_requires_precise_position(me.horizontal_collision || me.vertical_collision);
    me.first_tick = false;
    me.kind = EntityKind::Minecart(cart);
    // `DetectorRailBlock.checkPressed`：归还 `EntityKind::Minecart` 后再触发，
    // 这样 `check_detector_rail` 能认出正在 tick 的矿车自己。
    // Trigger `DetectorRailBlock.checkPressed` after restoring `EntityKind::Minecart`
    // so `check_detector_rail` recognizes the ticking cart itself.
    for at in detectors {
        ctx.emit(Command::MinecartDetectorRail { pos: at }, me);
    }
    tick_furnace(me);
    tick_tnt(me, ctx);
    // `MinecartHopper.tick` → `tryConsumeItems`：活着的、启用的漏斗矿车吸一次。
    // `MinecartHopper.tick` → `tryConsumeItems`: a live, enabled hopper cart sucks once.
    let hopper_ready = matches!(&me.kind, EntityKind::Minecart(cart)
        if cart.kind() == MinecartKind::Hopper && cart.enabled && !me.removed);
    if hopper_ready {
        ctx.emit(Command::MinecartHopperSuck, me);
    }
}

/// `MinecartFurnace.tick`：燃料递减，用完清空推力，冒烟粒子消耗一次实体随机数。
///
/// `MinecartFurnace.tick`: burn fuel, clear the push when empty, and draw one
/// entity random value for the smoke particle.
fn tick_furnace(me: &mut Entity) {
    let is_furnace = matches!(&me.kind, EntityKind::Minecart(cart) if cart.kind().is_furnace());
    if !is_furnace {
        return;
    }
    let mut has_fuel = false;
    if let EntityKind::Minecart(cart) = &mut me.kind {
        if cart.fuel > 0 {
            cart.fuel -= 1;
        }
        if cart.fuel <= 0 {
            cart.push = Vec3::default();
        }
        has_fuel = cart.fuel > 0;
    }
    // 官方 `hasFuel && random.nextInt(4) == 0` 时冒烟；粒子不在范围里，
    // 但实体随机数的这次取值要照取。
    // Vanilla draws when `hasFuel && random.nextInt(4) == 0`; the particle is
    // out of scope but the entity random draw must still happen.
    if has_fuel {
        let _ = me.random.next_int_bounded(4);
    }
}

/// `MinecartTNT.tick`：引信递减、归零爆炸，以及撞击时的爆炸。
///
/// `MinecartTNT.tick`: count down the fuse, explode at zero, and on a hard impact.
fn tick_tnt(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let is_tnt = matches!(&me.kind, EntityKind::Minecart(cart) if cart.kind() == MinecartKind::Tnt);
    if !is_tnt {
        return;
    }
    let speed_sqr = me.delta.x * me.delta.x + me.delta.z * me.delta.z;
    let fuse = match &me.kind {
        EntityKind::Minecart(cart) => cart.fuse,
        _ => -1,
    };
    let mut explode = false;
    if fuse > 0 {
        if let EntityKind::Minecart(cart) = &mut me.kind {
            cart.fuse -= 1;
        }
    } else if fuse == 0 {
        explode = true;
    }
    if me.horizontal_collision && speed_sqr >= 0.01 {
        explode = true;
    }
    if !explode {
        return;
    }
    let (power, factor) = match &me.kind {
        EntityKind::Minecart(cart) => (cart.explosion_power, cart.explosion_speed_factor),
        _ => (4.0, 1.0),
    };
    // 官方：`explosionPowerBase + explosionSpeedFactor * random.nextDouble() * 1.5
    //        * min(sqrt(speed²), 5)`，结果转成 float。
    // Vanilla: `explosionPowerBase + explosionSpeedFactor * random.nextDouble() * 1.5
    //          * min(sqrt(speed²), 5)`, as a float.
    let radius = (f64::from(power)
        + f64::from(factor) * me.random.next_double() * 1.5 * speed_sqr.sqrt().min(5.0))
        as f32;
    let source = Box::new(me.clone().split().public);
    me.discard();
    ctx.emit(
        Command::MinecartExplode {
            center: me.position,
            radius,
            source,
            owner: None,
        },
        me,
    );
}

/// `activateMinecart`：激活铁轨各子类的反应。漏斗矿车通电关、未通电开，
/// 其余子类只在通电时生效。
///
/// `activateMinecart`: each subclass's response to an activator rail. A hopper
/// cart sets `enabled = !active` on every pass; the others react only when active.
fn activate_minecart(cart: &mut Minecart, active: bool) {
    match cart.kind() {
        MinecartKind::Hopper => {
            let next = !active;
            if cart.enabled != next {
                cart.enabled = next;
            }
        }
        MinecartKind::Rideable if active => {
            cart.passengers.clear();
            if cart.hurt_time == 0 {
                cart.hurt_dir = -cart.hurt_dir;
                cart.hurt_time = 10;
                cart.damage = 50.0;
            }
        }
        MinecartKind::Tnt if active && cart.fuse < 0 => {
            cart.fuse = 80;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn furnace_fuel_is_capped_and_sets_a_horizontal_push() {
        let mut cart = Minecart::new(MinecartKind::Furnace);
        let at = Vec3::new(0.5, 64.0625, 0.5);
        let player = Vec3::new(0.5, 64.0, -1.5);
        assert!(cart.add_fuel(player, at, true));
        assert_eq!(cart.fuel, 3600);
        assert_eq!(cart.push, Vec3::new(0.0, 0.0, 2.0));
        // 非燃料物品不动。
        assert!(!cart.add_fuel(player, at, false));
        assert_eq!(cart.fuel, 3600);
        // 超过 32000 的上限。
        cart.fuel = 30000;
        assert!(!cart.add_fuel(player, at, true));
        assert_eq!(cart.fuel, 30000);
    }

    #[test]
    fn push_reprojects_along_the_motion_without_changing_length() {
        // 推力 (1,0,1) 投影到速度 (1,0,0) 后方向转到 +x，长度仍是 sqrt(2)。
        let push = Vec3::new(1.0, 0.0, 1.0);
        let motion = Vec3::new(1.0, 0.0, 0.0);
        let projected = calculate_new_push_along(push, motion);
        assert!((projected.x - 2.0_f64.sqrt()).abs() < 1.0e-9);
        assert!(projected.y.abs() < 1.0e-9);
        assert!(projected.z.abs() < 1.0e-9);
        assert!((projected.length() - push.length()).abs() < 1.0e-9);
        // 速度太短时保留原推力。
        assert_eq!(calculate_new_push_along(push, Vec3::default()), push);
    }

    #[test]
    fn rider_intent_nudges_a_slow_cart_and_cancels_unpowered_rail_brake() {
        use crate::{
            BlockTicks, World,
            state::{POWERED, RAIL_SHAPE, RailShape},
            testing::TestLevel,
        };

        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        for z in -2..=4 {
            level.set(BlockPos::new(0, 63, z), BlockState(blocks::STONE));
            level.set(
                BlockPos::new(0, 64, z),
                BlockState(blocks::POWERED_RAIL)
                    .with(RAIL_SHAPE, RailShape::NorthSouth)
                    .with(POWERED, false),
            );
        }
        let mut entity = Entity::minecart(MinecartKind::Rideable, Vec3::new(0.5, 64.0625, 0.5));
        if let EntityKind::Minecart(cart) = &mut entity.kind {
            cart.passengers.push(1);
            cart.rider_intent = Vec3::new(0.0, 0.0, 1.0);
        }
        let id = {
            let mut world = World::new(&mut level, &mut ticks);
            world.spawn_entity(entity)
        };
        let mut world = World::new(&mut level, &mut ticks);
        world.tick_entities();
        let after = world.entity(id).expect("minecart alive");
        assert!(
            after.delta.z > 0.0,
            "rider intent must push a still cart even on an unpowered powered rail"
        );
    }

    #[test]
    fn detector_rail_powers_reports_container_signal_and_resets_after_20_ticks() {
        use crate::{
            BlockTicks, World,
            state::{POWERED, RAIL_SHAPE, RailShape},
            testing::TestLevel,
        };

        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let detector = BlockPos::new(0, 64, 0);
        level.set(detector.below(), BlockState(blocks::STONE));
        level.set(
            detector,
            BlockState(blocks::DETECTOR_RAIL).with(RAIL_SHAPE, RailShape::NorthSouth),
        );
        let mut cart = Entity::minecart(MinecartKind::Chest, Vec3::new(0.5, 64.0625, 0.5));
        if let EntityKind::Minecart(data) = &mut cart.kind {
            // 27 格装满一半以上应给出非零比较器信号。
            data.items[0] = ItemStack::new(items::STONE, 64);
        }
        let id = {
            let mut world = World::new(&mut level, &mut ticks);
            world.spawn_entity(cart)
        };
        {
            let mut world = World::new(&mut level, &mut ticks);
            world.tick_entities();
            let state = world.block_state(detector);
            assert!(state.get(POWERED), "detector rail powers under a cart");
            assert_eq!(state.own_signal(&world, detector), 15);
            assert!(
                state.analog_output_signal(&world, detector, mc_bamboo_core::Direction::North) > 0,
                "comparator reads container cart fill on a powered detector rail"
            );
            // 把矿车移走后，等 20 刻计划刻到期自动熄灭。
            world.with_entity_mut(id, &mut |entity| entity.discard());
        }
        for step in 1..=20 {
            crate::run_scheduled_ticks(&mut level, &mut ticks, step);
        }
        let world = World::new(&mut level, &mut ticks);
        assert!(
            !world.block_state(detector).get(POWERED),
            "detector rail turns off after 20 ticks with no cart"
        );
    }
}
