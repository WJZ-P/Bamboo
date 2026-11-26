//! 矿车实体，官方 `AbstractMinecart` 及它的几个子类，运动走 `OldMinecartBehavior`。
//!
//! 实验特性 `minecart_improvements` 的 `NewMinecartBehavior` 不在范围里。
//!
//! Minecart entities, vanilla `AbstractMinecart` and its subclasses, moved by
//! `OldMinecartBehavior`. The experimental `NewMinecartBehavior` is out of scope.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{BlockAncestor, blocks, items, registries::entity_type, tags::block_tags};

use crate::{
    BlockState,
    entity::{
        Entity, EntityKind, EntityUid, PrimedTnt,
        ctx::{BlockView, EntityCtx},
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
        }
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
    pub display_offset: i32,
    /// `MinecartFurnace.DATA_ID_FUEL`。
    pub has_fuel: bool,
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
            display_offset: cart.display_offset,
            has_fuel: cart.fuel > 0,
        }
    }
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

/// `AbstractMinecart.applyNaturalSlowdown`：y 清零，有乘客 0.997、无 0.96，水中再 0.95。
///
/// `AbstractMinecart.applyNaturalSlowdown`: zero y, 0.997 with a passenger or
/// 0.96 without, times 0.95 in water.
fn apply_natural_slowdown(me: &Entity, cart: &Minecart, v: Vec3) -> Vec3 {
    let factor = if cart.passengers.is_empty() {
        0.96
    } else {
        0.997
    };
    let mut result = v.multiply(factor, 0.0, factor);
    if me.in_water {
        result = result.scale(0.95);
    }
    result
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
    let brake = is_powered_rail && !powered;
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
}

/// `activateMinecart`：激活铁轨通电时各子类的反应。
///
/// `activateMinecart`: each subclass's response to a powered activator rail.
fn activate_minecart(cart: &mut Minecart, active: bool) {
    if !active {
        return;
    }
    match cart.kind() {
        MinecartKind::Rideable => {
            cart.passengers.clear();
            if cart.hurt_time == 0 {
                cart.hurt_dir = -cart.hurt_dir;
                cart.hurt_time = 10;
                cart.damage = 50.0;
            }
        }
        MinecartKind::Tnt => {
            if cart.fuse < 0 {
                cart.fuse = 80;
            }
        }
        MinecartKind::Hopper => {
            cart.enabled = false;
        }
        _ => {}
    }
}
