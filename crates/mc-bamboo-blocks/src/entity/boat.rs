//! 船、竹筏和它们的运输船版本，对应官方 `AbstractBoat` / `Boat` / `Raft` /
//! `AbstractChestBoat` / `ChestBoat` / `ChestRaft`。
//!
//! 船不是生物：没有 `LivingEntity` 的生命值和属性，伤害靠 `VehicleEntity` 的同步数据
//! （受伤时间、受伤方向、伤害值）累计。乘客名单和生物一样存在实体上，玩家 ID 和生物 ID
//! 混在一张表里。这里只放数据结构和类型推导，逐刻的物理在 [`super::boat`] 的 tick 函数里。
//!
//! Boats, rafts and their chest variants, vanilla `AbstractBoat` / `Boat` / `Raft` /
//! `AbstractChestBoat` / `ChestBoat` / `ChestRaft`. A boat is not a mob: it has no
//! `LivingEntity` health, and damage accumulates through `VehicleEntity`'s synced data
//! (hurt time, hurt direction, damage). Its passenger list is stored like a mob's, mixing
//! player and mob ids. This module holds the data and type lookups; the per-tick physics
//! lives in the tick function.

use mc_bamboo_core::{BlockPos, Direction, Vec3, mth};
use mc_bamboo_data::{blocks, tags::fluid_tags};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    shape::{Aabb, CollisionContext},
    state::{DRAG, FluidState, SupportType},
};

use super::{
    Entity, EntityPublic, EntityUid, KindPublic,
    ctx::{BlockView, Command, EntityCtx},
    item::{apply_current, flow_vector, friction},
};

/// 桨每次前进的角度，官方 `PADDLE_SPEED`（π/8 的 f32，位模式与字面量 `0.3926991f` 相同）。
///
/// How far a paddle advances per tick, vanilla `PADDLE_SPEED` (π/8 as f32, the
/// same bits as the literal `0.3926991f`).
pub const PADDLE_SPEED: f32 = std::f32::consts::FRAC_PI_8;
/// 划桨音效的阈值角，官方 `PADDLE_SOUND_TIME`（π/4）。
///
/// The paddle-sound threshold angle, vanilla `PADDLE_SOUND_TIME` (π/4).
pub const PADDLE_SOUND_TIME: f64 = 0.785_398_185_253_143_3;
/// 失控多少刻后把乘客甩下去，官方 `TIME_TO_EJECT`。
///
/// Ticks out of control before ejecting the passengers, vanilla `TIME_TO_EJECT`.
pub const TIME_TO_EJECT: f32 = 60.0;
/// 气泡柱的气泡时间，官方 `BUBBLE_TIME`。
///
/// The bubble-column bubble time, vanilla `BUBBLE_TIME`.
pub const BUBBLE_TIME: i32 = 60;
/// 船的默认重力加速度，官方 `AbstractBoat.getDefaultGravity()`。
///
/// The default gravity, vanilla `AbstractBoat.getDefaultGravity()`.
pub const DEFAULT_GRAVITY: f64 = 0.04;

/// 船在水里、水下、流动水下、陆上、空中的状态，官方 `AbstractBoat.Status`。
///
/// The boat's fluid / ground status, vanilla `AbstractBoat.Status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    /// 浮在水面，`IN_WATER`。
    InWater,
    /// 完全没入水源，`UNDER_WATER`。
    UnderWater,
    /// 没入流动水，`UNDER_FLOWING_WATER`。
    UnderFlowingWater,
    /// 在地上，`ON_LAND`。
    OnLand,
    /// 在空中，`IN_AIR`。
    #[default]
    InAir,
}

/// 运输船多出来的状态：27 格物品栏和战利品表。
///
/// The extra chest-boat state: a 27-slot inventory and a loot table.
#[derive(Debug, Clone, PartialEq)]
pub struct ChestBoat {
    /// `itemStacks`：27 格，全部以 `ItemStack.EMPTY` 起步。
    pub items: Vec<ItemStack>,
    /// `lootTable`。
    pub loot_table: Option<String>,
    /// `lootTableSeed`。
    pub loot_table_seed: i64,
}

impl Default for ChestBoat {
    fn default() -> Self {
        Self {
            items: vec![ItemStack::empty(); 27],
            loot_table: None,
            loot_table_seed: 0,
        }
    }
}

/// 船的完整状态（串行 tick 里用的那份，见 [`super::Entity`] 的说明）。
///
/// The full boat state used in the serial tick, mirroring the `Mob` pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct Boat {
    /// 注册表实体类型 ID，决定木种、是不是竹筏、是不是运输船。
    pub entity_type: i32,
    /// 乘客名单（玩家 ID 和生物 ID 混在一起，按上车顺序）。
    pub passengers: Vec<EntityUid>,
    /// `VehicleEntity.DATA_ID_HURT`。
    pub hurt_time: i32,
    /// `VehicleEntity.DATA_ID_HURTDIR`。
    pub hurt_dir: i32,
    /// `VehicleEntity.DATA_ID_DAMAGE`。
    pub damage: f32,
    /// `DATA_ID_PADDLE_LEFT`。
    pub paddle_left: bool,
    /// `DATA_ID_PADDLE_RIGHT`。
    pub paddle_right: bool,
    /// `DATA_ID_BUBBLE_TIME`。
    pub bubble_time: i32,
    /// `paddlePositions`：左右桨的动画角度。
    pub paddle_positions: [f32; 2],
    /// `outOfControlTicks`。
    pub out_of_control_ticks: f32,
    /// `deltaRotation`。
    pub delta_rotation: f32,
    pub input_left: bool,
    pub input_right: bool,
    pub input_up: bool,
    pub input_down: bool,
    /// `waterLevel`。
    pub water_level: f64,
    /// `landFriction`。
    pub land_friction: f32,
    pub status: Status,
    pub old_status: Status,
    /// `lastYd`。
    pub last_yd: f64,
    pub is_above_bubble_column: bool,
    pub bubble_column_direction_is_down: bool,
    pub bubble_multiplier: f32,
    pub bubble_angle: f32,
    pub bubble_angle_o: f32,
    /// 运输船的物品栏；普通船是 `None`。
    pub chest: Option<ChestBoat>,
}

impl Boat {
    /// `AbstractBoat.<init>` / `AbstractChestBoat.<init>`：桨位置归零，`blocksBuilding` 为真。
    pub fn new(entity_type: i32) -> Self {
        Self {
            entity_type,
            passengers: Vec::new(),
            hurt_time: 0,
            hurt_dir: 1,
            damage: 0.0,
            paddle_left: false,
            paddle_right: false,
            bubble_time: 0,
            paddle_positions: [0.0; 2],
            out_of_control_ticks: 0.0,
            delta_rotation: 0.0,
            input_left: false,
            input_right: false,
            input_up: false,
            input_down: false,
            water_level: 0.0,
            land_friction: 0.0,
            status: Status::InAir,
            old_status: Status::InAir,
            last_yd: 0.0,
            is_above_bubble_column: false,
            bubble_column_direction_is_down: false,
            bubble_multiplier: 0.0,
            bubble_angle: 0.0,
            bubble_angle_o: 0.0,
            chest: is_chest(entity_type).then(ChestBoat::default),
        }
    }

    /// 是不是运输船（`AbstractChestBoat`）。
    pub const fn is_chest(&self) -> bool {
        is_chest(self.entity_type)
    }

    /// 官方 `getDropItem()`：破坏后掉落的物品 ID。
    pub fn drop_item(&self) -> i32 {
        drop_item(self.entity_type)
    }
}

/// 别人读得到的那一半（同步和别的实体读），字段和 [`Boat`] 一一对应。
///
/// The public snapshot other entities and the sync layer read.
#[derive(Debug, Clone, PartialEq)]
pub struct BoatPublic {
    pub entity_type: i32,
    pub passengers: Vec<EntityUid>,
    pub hurt_time: i32,
    pub hurt_dir: i32,
    pub damage: f32,
    pub paddle_left: bool,
    pub paddle_right: bool,
    pub bubble_time: i32,
}

impl BoatPublic {
    /// 从完整状态取一份公开快照。
    pub fn of(boat: &Boat) -> Self {
        Self {
            entity_type: boat.entity_type,
            passengers: boat.passengers.clone(),
            hurt_time: boat.hurt_time,
            hurt_dir: boat.hurt_dir,
            damage: boat.damage,
            paddle_left: boat.paddle_left,
            paddle_right: boat.paddle_right,
            bubble_time: boat.bubble_time,
        }
    }
}

/// 按注册表实体类型判断是不是运输船。
pub const fn is_chest(entity_type: i32) -> bool {
    use mc_bamboo_data::registries::entity_type as ids;
    matches!(
        entity_type,
        ids::ACACIA_CHEST_BOAT
            | ids::BAMBOO_CHEST_RAFT
            | ids::BIRCH_CHEST_BOAT
            | ids::CHERRY_CHEST_BOAT
            | ids::DARK_OAK_CHEST_BOAT
            | ids::JUNGLE_CHEST_BOAT
            | ids::MANGROVE_CHEST_BOAT
            | ids::OAK_CHEST_BOAT
            | ids::PALE_OAK_CHEST_BOAT
            | ids::POPLAR_CHEST_BOAT
            | ids::SPRUCE_CHEST_BOAT
    )
}

/// 这个注册表实体类型是不是船（含竹筏和运输船）。
pub const fn is_boat_entity_type(entity_type: i32) -> bool {
    use mc_bamboo_data::registries::entity_type as ids;
    matches!(
        entity_type,
        ids::ACACIA_BOAT
            | ids::ACACIA_CHEST_BOAT
            | ids::BAMBOO_RAFT
            | ids::BAMBOO_CHEST_RAFT
            | ids::BIRCH_BOAT
            | ids::BIRCH_CHEST_BOAT
            | ids::CHERRY_BOAT
            | ids::CHERRY_CHEST_BOAT
            | ids::DARK_OAK_BOAT
            | ids::DARK_OAK_CHEST_BOAT
            | ids::JUNGLE_BOAT
            | ids::JUNGLE_CHEST_BOAT
            | ids::MANGROVE_BOAT
            | ids::MANGROVE_CHEST_BOAT
            | ids::OAK_BOAT
            | ids::OAK_CHEST_BOAT
            | ids::PALE_OAK_BOAT
            | ids::PALE_OAK_CHEST_BOAT
            | ids::POPLAR_BOAT
            | ids::POPLAR_CHEST_BOAT
            | ids::SPRUCE_BOAT
            | ids::SPRUCE_CHEST_BOAT
    )
}

/// 按注册表实体类型查破坏后掉落的物品 ID（官方 `BoatItem` 构造时给的 `Supplier`）。
pub const fn drop_item(entity_type: i32) -> i32 {
    use mc_bamboo_data::items;
    use mc_bamboo_data::registries::entity_type as ids;
    match entity_type {
        ids::ACACIA_BOAT => items::ACACIA_BOAT,
        ids::ACACIA_CHEST_BOAT => items::ACACIA_CHEST_BOAT,
        ids::BAMBOO_RAFT => items::BAMBOO_RAFT,
        ids::BAMBOO_CHEST_RAFT => items::BAMBOO_CHEST_RAFT,
        ids::BIRCH_BOAT => items::BIRCH_BOAT,
        ids::BIRCH_CHEST_BOAT => items::BIRCH_CHEST_BOAT,
        ids::CHERRY_BOAT => items::CHERRY_BOAT,
        ids::CHERRY_CHEST_BOAT => items::CHERRY_CHEST_BOAT,
        ids::DARK_OAK_BOAT => items::DARK_OAK_BOAT,
        ids::DARK_OAK_CHEST_BOAT => items::DARK_OAK_CHEST_BOAT,
        ids::JUNGLE_BOAT => items::JUNGLE_BOAT,
        ids::JUNGLE_CHEST_BOAT => items::JUNGLE_CHEST_BOAT,
        ids::MANGROVE_BOAT => items::MANGROVE_BOAT,
        ids::MANGROVE_CHEST_BOAT => items::MANGROVE_CHEST_BOAT,
        ids::OAK_BOAT => items::OAK_BOAT,
        ids::OAK_CHEST_BOAT => items::OAK_CHEST_BOAT,
        ids::PALE_OAK_BOAT => items::PALE_OAK_BOAT,
        ids::PALE_OAK_CHEST_BOAT => items::PALE_OAK_CHEST_BOAT,
        ids::POPLAR_BOAT => items::POPLAR_BOAT,
        ids::POPLAR_CHEST_BOAT => items::POPLAR_CHEST_BOAT,
        ids::SPRUCE_BOAT => items::SPRUCE_BOAT,
        ids::SPRUCE_CHEST_BOAT => items::SPRUCE_CHEST_BOAT,
        // 非船的注册表类型不会走到这里；给一个明确的空值兜底。
        _ => items::AIR,
    }
}

/// 船物品 ID → 注册表实体类型 ID（官方 `BoatItem` 构造时给的 `EntityType`）。
/// 非船物品返回 `None`。
///
/// Boat item id to registry entity type (the `EntityType` a `BoatItem` carries).
/// `None` for non-boat items.
pub const fn entity_type_for_item(item: i32) -> Option<i32> {
    use mc_bamboo_data::items;
    use mc_bamboo_data::registries::entity_type as ids;
    Some(match item {
        items::ACACIA_BOAT => ids::ACACIA_BOAT,
        items::ACACIA_CHEST_BOAT => ids::ACACIA_CHEST_BOAT,
        items::BAMBOO_RAFT => ids::BAMBOO_RAFT,
        items::BAMBOO_CHEST_RAFT => ids::BAMBOO_CHEST_RAFT,
        items::BIRCH_BOAT => ids::BIRCH_BOAT,
        items::BIRCH_CHEST_BOAT => ids::BIRCH_CHEST_BOAT,
        items::CHERRY_BOAT => ids::CHERRY_BOAT,
        items::CHERRY_CHEST_BOAT => ids::CHERRY_CHEST_BOAT,
        items::DARK_OAK_BOAT => ids::DARK_OAK_BOAT,
        items::DARK_OAK_CHEST_BOAT => ids::DARK_OAK_CHEST_BOAT,
        items::JUNGLE_BOAT => ids::JUNGLE_BOAT,
        items::JUNGLE_CHEST_BOAT => ids::JUNGLE_CHEST_BOAT,
        items::MANGROVE_BOAT => ids::MANGROVE_BOAT,
        items::MANGROVE_CHEST_BOAT => ids::MANGROVE_CHEST_BOAT,
        items::OAK_BOAT => ids::OAK_BOAT,
        items::OAK_CHEST_BOAT => ids::OAK_CHEST_BOAT,
        items::PALE_OAK_BOAT => ids::PALE_OAK_BOAT,
        items::PALE_OAK_CHEST_BOAT => ids::PALE_OAK_CHEST_BOAT,
        items::POPLAR_BOAT => ids::POPLAR_BOAT,
        items::POPLAR_CHEST_BOAT => ids::POPLAR_CHEST_BOAT,
        items::SPRUCE_BOAT => ids::SPRUCE_BOAT,
        items::SPRUCE_CHEST_BOAT => ids::SPRUCE_CHEST_BOAT,
        _ => return None,
    })
}

/// 这个船实体类型是不是竹筏（`Raft` / `ChestRaft`，骑乘高度不同）。
///
/// Whether this boat entity type is a raft (`Raft` / `ChestRaft`), whose ride
/// height differs.
pub const fn is_raft(entity_type: i32) -> bool {
    use mc_bamboo_data::registries::entity_type as ids;
    matches!(entity_type, ids::BAMBOO_RAFT | ids::BAMBOO_CHEST_RAFT)
}

/// 官方 `AbstractBoat.getPassengerAttachmentPoint`：单乘客用 `getSinglePassengerXOffset()`
/// （普通船 0、运输船 0.15），两名乘客时按下标取 0.2 / -0.6，动物乘客再各加 0.2。
/// 高度是 `rideHeight(dimensions)`（船 `height/3`、筏 `height*0.8888889`），点
/// `(0, rideHeight, z)` 绕 `-yRot * 0.017453292f` 旋转。玩家不是动物。
///
/// Vanilla `AbstractBoat.getPassengerAttachmentPoint`: a single passenger uses
/// `getSinglePassengerXOffset()` (0, or 0.15 for chest boats); with two
/// passengers the index picks 0.2 / -0.6, plus 0.2 for an animal passenger.
/// The height is `rideHeight(dimensions)` (boat `height/3`, raft
/// `height*0.8888889`); the point `(0, rideHeight, z)` rotates by
/// `-yRot * 0.017453292f`. A player is not an animal.
pub fn passenger_attachment_point(
    entity_type: i32,
    yaw: f32,
    index: usize,
    passenger_count: usize,
    passenger_is_animal: bool,
) -> Vec3 {
    let mut z = if is_chest(entity_type) {
        0.15_f32
    } else {
        0.0_f32
    };
    if passenger_count > 1 {
        z = if index == 0 { 0.2_f32 } else { -0.6_f32 };
    }
    if passenger_is_animal {
        z += 0.2_f32;
    }
    let height = mc_bamboo_data::entity_types::FACTS
        .get(entity_type as usize)
        .map_or(0.5625_f32, |facts| facts.height);
    let ride_height = if is_raft(entity_type) {
        height * 0.888_888_9_f32
    } else {
        height / 3.0_f32
    };
    let angle = -yaw * 0.017_453_292_f32;
    let sin = f64::from(mth::sin(f64::from(angle)));
    let cos = f64::from(mth::cos(f64::from(angle)));
    // `Vec3(0, rideHeight, z).yRot(angle)`：x' = z*sin，z' = z*cos。
    Vec3::new(
        f64::from(z) * sin,
        f64::from(ride_height),
        f64::from(z) * cos,
    )
}

/// 船的一刻，`AbstractBoat.tick` 的服务端路径：状态判定、漂浮物理、移动、气泡柱、
/// 水流和桨动画。玩家控制时位置由客户端 `move_vehicle` 决定，服务端只清速度。
///
/// One boat tick, the server path of `AbstractBoat.tick`: status, buoyancy,
/// movement, bubble columns, currents and paddle animation. A player-controlled
/// boat has its position set by the client `move_vehicle` packet, so the server
/// only clears its velocity.
pub fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let super::EntityKind::Boat(boat_box) = &me.kind else {
        return;
    };
    // 把状态复制到局部：物理里要同时读方块视图、改位置和这份状态，绕开借用纠缠。
    let mut boat = (**boat_box).clone();
    let view = ctx.blocks();

    boat.old_status = boat.status;
    boat.status = get_status(&mut boat, me, view);
    if matches!(boat.status, Status::UnderWater | Status::UnderFlowingWater) {
        boat.out_of_control_ticks += 1.0;
    } else {
        boat.out_of_control_ticks = 0.0;
    }
    // 服务端 `outOfControlTicks >= 60` 时的 `ejectPassengers()` 由骑乘层接入。
    if boat.hurt_time > 0 {
        boat.hurt_time -= 1;
    }
    if boat.damage > 0.0 {
        boat.damage -= 1.0;
    }

    // 服务端 `isLocalInstanceAuthoritative()` = `!isClientAuthoritative()`，后者
    // 只在控制者是玩家时为真。无人 / 动物控制的船由服务端跑 `floatBoat` + `move`。
    if !first_passenger_is_player(&boat) {
        boat.paddle_left = false;
        boat.paddle_right = false;
        float_boat(&mut boat, me, view);
        me.move_entity(view, me.delta);
    } else {
        me.delta = Vec3::default();
        boat.delta_rotation = 0.0;
    }

    me.apply_effects_from_blocks(view);
    me.apply_effects_from_blocks(view);
    detect_bubble_column(&mut boat, me, view);
    apply_water_current(me, view);
    // 官方 `Entity.baseTick` 的岩浆 / 火：每刻调一次 `hurtServer`
    // （岩浆 4.0、火 1.0），伤害累计到阈值就破坏并掉物品。
    let env_amount = environment_damage(me, view);
    tick_bubble_column(&mut boat, me, ctx);

    for index in 0..2 {
        let paddling = if index == 0 {
            boat.paddle_left
        } else {
            boat.paddle_right
        };
        if paddling {
            let before = f64::from(boat.paddle_positions[index] % 6.283_185_5_f32);
            let after = f64::from((boat.paddle_positions[index] + PADDLE_SPEED) % 6.283_185_5_f32);
            if before <= PADDLE_SOUND_TIME && after >= PADDLE_SOUND_TIME {
                let _ = me.random.next_float();
            }
            boat.paddle_positions[index] += PADDLE_SPEED;
        } else {
            boat.paddle_positions[index] = 0.0;
        }
    }

    // 推动实体、把动物拉上船由骑乘层接入。
    if env_amount > 0.0 {
        hurt_from_environment(&mut boat, me, env_amount, ctx);
    }

    if let super::EntityKind::Boat(boat_box) = &mut me.kind {
        **boat_box = boat;
    }
}

/// `getStatus`：`isUnderwater` → `checkInWater` → 地面摩擦 → `IN_AIR`。
fn get_status(boat: &mut Boat, me: &Entity, view: &dyn BlockView) -> Status {
    if let Some(status) = is_underwater(me, view) {
        boat.water_level = me.bounding_box().max_y;
        return status;
    }
    if check_in_water(boat, me, view) {
        return Status::InWater;
    }
    let ground = get_ground_friction(me, view);
    if ground > 0.0 {
        boat.land_friction = ground;
        return Status::OnLand;
    }
    Status::InAir
}

/// `isUnderwater`：AABB 顶面之上还有水源格 → `UNDER_WATER`；有流动水 →
/// `UNDER_FLOWING_WATER`；否则返回 `None`。
fn is_underwater(me: &Entity, view: &dyn BlockView) -> Option<Status> {
    let bb = me.bounding_box();
    let ceiling = bb.max_y + 0.001;
    let (x0, x1) = (mth::floor(bb.min_x), mth::ceil(bb.max_x));
    let (y0, y1) = (mth::floor(bb.max_y), mth::ceil(ceiling));
    let (z0, z1) = (mth::floor(bb.min_z), mth::ceil(bb.max_z));
    let mut source = false;
    for x in x0..x1 {
        for y in y0..y1 {
            for z in z0..z1 {
                let pos = BlockPos::new(x, y, z);
                let fluid = view.fluid_state(pos);
                if !fluid.is_tagged(fluid_tags::WATER) {
                    continue;
                }
                let top = (y as f32) + view.fluid_height(fluid, pos);
                if ceiling < f64::from(top) {
                    if fluid.is_source() {
                        source = true;
                    } else {
                        return Some(Status::UnderFlowingWater);
                    }
                }
            }
        }
    }
    source.then_some(Status::UnderWater)
}

/// `checkInWater`：扫 AABB 里的水，把 `waterLevel` 记成最高水面，返回船底是否浸在水里。
fn check_in_water(boat: &mut Boat, me: &Entity, view: &dyn BlockView) -> bool {
    let bb = me.bounding_box();
    let (x0, x1) = (mth::floor(bb.min_x), mth::ceil(bb.max_x));
    let (y0, y1) = (mth::floor(bb.min_y), mth::ceil(bb.min_y + 0.001));
    let (z0, z1) = (mth::floor(bb.min_z), mth::ceil(bb.max_z));
    let mut found = false;
    boat.water_level = -f64::MAX;
    for x in x0..x1 {
        for y in y0..y1 {
            for z in z0..z1 {
                let pos = BlockPos::new(x, y, z);
                let fluid = view.fluid_state(pos);
                if !fluid.is_tagged(fluid_tags::WATER) {
                    continue;
                }
                let top = (y as f32) + view.fluid_height(fluid, pos);
                boat.water_level = boat.water_level.max(f64::from(top));
                if bb.min_y < f64::from(top) {
                    found = true;
                }
            }
        }
    }
    found
}

/// `getWaterLevelAbove`：从 AABB 顶面逐层向上找第一层水面没到顶的水位。
fn get_water_level_above(boat: &Boat, me: &Entity, view: &dyn BlockView) -> f32 {
    let bb = me.bounding_box();
    let (x0, x1) = (mth::floor(bb.min_x), mth::ceil(bb.max_x));
    let (z0, z1) = (mth::floor(bb.min_z), mth::ceil(bb.max_z));
    let y0 = mth::floor(bb.max_y);
    let y1 = mth::ceil(bb.max_y - boat.last_yd);
    for y in y0..y1 {
        let mut max_height = 0.0f32;
        'columns: for x in x0..x1 {
            for z in z0..z1 {
                let pos = BlockPos::new(x, y, z);
                let fluid = view.fluid_state(pos);
                if fluid.is_tagged(fluid_tags::WATER) {
                    max_height = max_height.max(view.fluid_height(fluid, pos));
                }
                if max_height >= 1.0 {
                    break 'columns;
                }
            }
        }
        if max_height < 1.0 {
            return (y as f32) + max_height;
        }
    }
    (y1 + 1) as f32
}

/// `getGroundFriction`：底部薄片碰到的方块摩擦均值；无碰撞块时是 NaN，
/// `getStatus` 里 `NaN > 0` 为假，与官方一致。
fn get_ground_friction(me: &Entity, view: &dyn BlockView) -> f32 {
    let bb = me.bounding_box();
    // 官方底部薄片 `(minX, minY-0.001, minZ, maxX, minY, maxZ)`。
    let thin = Aabb::new(
        bb.min_x,
        bb.min_y - 0.001,
        bb.min_z,
        bb.max_x,
        bb.min_y,
        bb.max_z,
    );
    // 逐格扫描的范围比薄片大一圈；外圈边界由下面的角落 / 外圈规则排除。
    let (x0, x1) = (mth::floor(thin.min_x) - 1, mth::ceil(thin.max_x) + 1);
    let (y0, y1) = (mth::floor(thin.min_y) - 1, mth::ceil(thin.max_y) + 1);
    let (z0, z1) = (mth::floor(thin.min_z) - 1, mth::ceil(thin.max_z) + 1);
    // `state.getCollisionShape(level, pos)` 走空上下文。
    let context = CollisionContext::Empty;
    let mut sum = 0.0f32;
    let mut count = 0i32;
    for x in x0..x1 {
        let edge_x = x == x0 || x == x1 - 1;
        for z in z0..z1 {
            let edge_z = z == z0 || z == z1 - 1;
            // x / z 都在外圈时整列跳过。
            let corners = i32::from(edge_x) + i32::from(edge_z);
            if corners == 2 {
                continue;
            }
            for y in y0..y1 {
                // 有一个方向在外圈时，y 的外圈两层也跳过。
                if corners > 0 && (y == y0 || y == y1 - 1) {
                    continue;
                }
                let pos = BlockPos::new(x, y, z);
                let state = view.block_state(pos);
                if state.is(blocks::LILY_PAD) {
                    continue;
                }
                // `Shapes.joinIsNotEmpty(state.getCollisionShape(...).move(pos),
                // shape, AND)`：把这一格的碰撞形状平移到世界坐标后与薄片求交。
                let boxes = crate::shape::block_collision_boxes(
                    state,
                    pos,
                    context,
                    view.moving_piston_shape(pos).as_deref(),
                    view.shulker_box_shape(pos),
                );
                let meets = boxes.iter().any(|bounds| {
                    bounds
                        .moved(f64::from(pos.x), f64::from(pos.y), f64::from(pos.z))
                        .intersects(thin)
                });
                if !meets {
                    continue;
                }
                sum += friction(state);
                count += 1;
            }
        }
    }
    // 官方是 `f10 / (float)i11`：没有碰撞块时 0/0 = NaN，`getStatus` 里
    // `NaN > 0` 为假，正好等价于 IN_AIR。
    sum / count as f32
}

/// `floatBoat`：按状态算这一刻的加速度和阻力，必要时把船直接抬到水面。
fn float_boat(boat: &mut Boat, me: &mut Entity, view: &dyn BlockView) {
    let mut gravity = -DEFAULT_GRAVITY;
    let mut lift = 0.0f64;
    // 官方初值 `0.05f` 在走完状态分支前从不会被读：每个分支都会覆盖它。
    // Vanilla's `0.05f` initial value is never read: every status branch overwrites it.
    let drag: f32;
    let bb = me.bounding_box();
    let height = f64::from(me.dimensions.1);
    if boat.old_status == Status::InAir
        && boat.status != Status::InAir
        && boat.status != Status::OnLand
    {
        boat.water_level = me.y_at(1.0);
        let above = get_water_level_above(boat, me, view);
        let target = f64::from(above - me.dimensions.1) + 0.101;
        let dy = target - me.position.y;
        let moved = Aabb::new(
            bb.min_x,
            bb.min_y + dy,
            bb.min_z,
            bb.max_x,
            bb.max_y + dy,
            bb.max_z,
        );
        if view
            .block_collisions(moved, me.collision_context())
            .is_empty()
        {
            me.position = Vec3::new(me.position.x, target, me.position.z);
            me.delta = me.delta.multiply(1.0, 0.0, 1.0);
            boat.last_yd = 0.0;
        }
        boat.status = Status::InWater;
        return;
    }
    match boat.status {
        Status::InWater => {
            lift = (boat.water_level - me.position.y) / height;
            drag = 0.9;
        }
        Status::UnderFlowingWater => {
            gravity = -0.0007;
            drag = 0.9;
        }
        Status::UnderWater => {
            lift = 0.009_999_999_776_482_582;
            drag = 0.45;
        }
        Status::InAir => {
            drag = 0.9;
        }
        Status::OnLand => {
            drag = boat.land_friction;
            if first_passenger_is_player(boat) {
                boat.land_friction /= 2.0;
            }
        }
    }
    me.delta = Vec3::new(
        me.delta.x * f64::from(drag),
        me.delta.y + gravity,
        me.delta.z * f64::from(drag),
    );
    boat.delta_rotation *= drag;
    if lift > 0.0 {
        let y = (me.delta.y + lift * (DEFAULT_GRAVITY / 0.65)) * 0.75;
        me.delta = Vec3::new(me.delta.x, y, me.delta.z);
    }
}

/// `onAboveBubbleColumn`：船所在格子里有气泡柱时记下方向并起泡。
fn detect_bubble_column(boat: &mut Boat, me: &Entity, view: &dyn BlockView) {
    let bb = me.bounding_box();
    let (x0, x1) = (mth::floor(bb.min_x), mth::ceil(bb.max_x));
    let (y0, y1) = (mth::floor(bb.min_y), mth::ceil(bb.max_y));
    let (z0, z1) = (mth::floor(bb.min_z), mth::ceil(bb.max_z));
    for x in x0..x1 {
        for y in y0..y1 {
            for z in z0..z1 {
                let state = view.block_state(BlockPos::new(x, y, z));
                if state.is(blocks::BUBBLE_COLUMN) {
                    boat.is_above_bubble_column = true;
                    boat.bubble_column_direction_is_down = state.get(DRAG);
                    if boat.bubble_time == 0 {
                        boat.bubble_time = BUBBLE_TIME;
                    }
                }
            }
        }
    }
}

/// `tickBubbleColumn`：气泡时间到点后施加向上 / 向下的气泡冲量。
fn tick_bubble_column(boat: &mut Boat, me: &mut Entity, ctx: &mut dyn EntityCtx) {
    if !boat.is_above_bubble_column {
        boat.bubble_time = 0;
    }
    if boat.bubble_time > 0 {
        boat.bubble_time -= 1;
        let elapsed = BUBBLE_TIME - boat.bubble_time - 1;
        if elapsed > 0 && boat.bubble_time == 0 {
            boat.bubble_time = 0;
            let down = boat.bubble_column_direction_is_down;
            if down {
                // `ejectPassengers()`：清乘客名单，把被甩的乘客交给游戏层。
                let ejected = std::mem::take(&mut boat.passengers);
                ctx.emit(Command::BoatBubbleEvent { event: 71, ejected }, me);
            } else {
                ctx.emit(
                    Command::BoatBubbleEvent {
                        event: 72,
                        ejected: Vec::new(),
                    },
                    me,
                );
            }
            handle_bubble_column_effect(me, down);
        }
        // 字节码只在 `bubbleTime > 0` 这一刻把 `isAboveBubbleColumn` 置回 false。
        boat.is_above_bubble_column = false;
    }
}

/// `handleBubbleColumnEffect`。
fn handle_bubble_column_effect(me: &mut Entity, down: bool) {
    if down {
        me.delta = me.delta.add(Vec3::new(0.0, -0.7, 0.0));
    } else {
        me.delta = Vec3::new(me.delta.x, 0.6, me.delta.z);
    }
}

/// `Entity.updateFluidInteraction` 的水流推动部分：扫 AABB 内每个水格，累加
/// `flow_vector`，用累积的水面高度（Java `Math.max`）缩放不足 0.4 的推力，
/// 最后 `apply_current`。船没有流体跟踪器，`in_water` 用累积水面高度 > 0 判定。
///
/// The water-current half of `Entity.updateFluidInteraction`: scan water cells,
/// accumulate `flow_vector`, scale by the accumulated (Java max) fluid height
/// below 0.4, and apply the current.
fn apply_water_current(me: &mut Entity, view: &dyn BlockView) {
    let bounds = me.bounding_box().deflated(0.001);
    let x0 = mth::floor(bounds.min_x);
    let x1 = mth::ceil(bounds.max_x) - 1;
    let y0 = mth::floor(bounds.min_y);
    let y1 = mth::ceil(bounds.max_y) - 1;
    let z0 = mth::floor(bounds.min_z);
    let z1 = mth::ceil(bounds.max_z) - 1;
    // `hasFluidAndLoaded`：任何一个相关区块没加载就整段跳过。
    if !fluid_area_loaded(view, x0 - 1, y0, z0 - 1, x1 + 1, z1 + 1) {
        return;
    }
    let entity_min_y = me.position.y;
    let block_x = mth::floor(me.position.x);
    let block_z = mth::floor(me.position.z);
    // `Entity.getEyeHeight()`：`dimensions.height * 0.85F`。
    let eye = me.position.y + f64::from(me.dimensions.1 * 0.85_f32);
    let mut eyes_inside = false;
    let mut current = Vec3::default();
    let mut height = 0.0f64;
    let mut count = 0i32;
    for x in x0..=x1 {
        for y in y0..=y1 {
            for z in z0..=z1 {
                let pos = BlockPos::new(x, y, z);
                let fluid = view.fluid_state(pos);
                if fluid.is_empty() || !fluid.is_tagged(fluid_tags::WATER) {
                    continue;
                }
                let fy = f64::from(y);
                let top = fy + f64::from(view.fluid_height(fluid, pos));
                if top < bounds.min_y {
                    continue;
                }
                // `Entity.updateFluidInteraction` 的 eye tracker：眼睛落在这一格
                // 水面下方时记 `eyes_inside`，供 `isEyeInFluid(WATER)` 判定。
                if x == block_x && z == block_z && eye >= fy {
                    let camera = fy + f64::from(camera_height(view, fluid, pos));
                    if eye <= camera {
                        eyes_inside = true;
                    }
                }
                height = mth::java_max(top - entity_min_y, height);
                let mut flow = flow_vector(view, pos, fluid);
                if height < 0.4 {
                    flow = flow.scale(height);
                }
                current = current.add(flow);
                count += 1;
            }
        }
    }
    // `isEyeInFluid(WATER)` 供 `canAddPassenger` 的 `!isEyeInFluid(WATER)` 门控。
    me.in_water = eyes_inside;
    // `isPushedByFluid()` 为真且 `in_water`（水面高度 > 0）才施力。
    if height > 0.0 && count > 0 {
        apply_current(me, &current, 0.014);
    }
}

/// `FlowingFluid.getFlow` 用的相机高度：水源且上方方块顶面结实时为 1.0，
/// 否则是流体自身高度。
fn camera_height(view: &dyn BlockView, fluid: FluidState, pos: BlockPos) -> f32 {
    if fluid.is_source()
        && view
            .block_state(pos.above())
            .is_face_sturdy(Direction::Down, SupportType::Full)
    {
        1.0
    } else {
        view.fluid_height(fluid, pos)
    }
}

/// `Entity.updateFluidInteraction` 的加载检查：按区块坐标逐格看。
fn fluid_area_loaded(view: &dyn BlockView, x0: i32, y0: i32, z0: i32, x1: i32, z1: i32) -> bool {
    for chunk_z in (z0 >> 4)..=(z1 >> 4) {
        for chunk_x in (x0 >> 4)..=(x1 >> 4) {
            let probe = BlockPos::new(
                chunk_x << 4,
                y0.max(view.min_y()).min(view.max_y()),
                chunk_z << 4,
            );
            if !view.is_loaded(probe) {
                return false;
            }
        }
    }
    true
}

/// `AbstractBoat.positionRider` 末尾的 `clampRotation` / `calculatePassengerBodyYRot`：
/// 动物且满员时身体朝向是船朝向 + 90/270（按乘客 ID 奇偶），否则就是船朝向。
/// 生物乘客把 `yRot`/`yBodyRot` 设成它并 `clampHeadRotationToBody`；其它乘客
/// （玩家）把与船朝向的差限制在 ±105°。
///
/// `AbstractBoat`'s `clampRotation` / `calculatePassengerBodyYRot`: animal
/// passengers at full capacity face the boat yaw plus 90/270; mobs take that
/// body yaw, other passengers are clamped to ±105° around it.
pub fn clamp_passenger_rotation(passenger: &mut Entity, vehicle: &EntityPublic) {
    let KindPublic::Boat(boat) = &vehicle.kind else {
        return;
    };
    let max = if is_chest(boat.entity_type) { 1 } else { 2 };
    let animal = passenger
        .kind
        .mob()
        .is_some_and(|mob| mob.mob_type.is_animal());
    let body_y_rot = if animal && boat.passengers.len() == max {
        let offset = if passenger.id.rem_euclid(2) == 0 {
            90.0
        } else {
            270.0
        };
        vehicle.yaw + offset
    } else {
        vehicle.yaw
    };
    if passenger.kind.mob().is_some() {
        // `AbstractBoat.clampRotation` 的 Mob 分支：`setYRot(bodyYRot)` 后
        // `Mob.clampHeadRotationToBody()`（字节码 1831，limit = getMaxHeadYRot() = 75）。
        passenger.y_rot = body_y_rot;
        if let Some(mob) = passenger.kind.mob_mut() {
            mob.y_body_rot = body_y_rot;
            let limit = 75.0_f32;
            let head = mob.y_head_rot;
            let delta = mth::wrap_degrees(mob.y_body_rot - head);
            let clamped = mth::clamp_f32(mth::wrap_degrees(mob.y_body_rot - head), -limit, limit);
            mob.y_head_rot = head + delta - clamped;
        }
    } else {
        // `AbstractBoat.clampRotation` 非 Mob 分支：`clamp(diff, -105, 105)`。
        let diff = mth::wrap_degrees(passenger.y_rot - body_y_rot);
        let clamped = mth::clamp_f32(diff, -105.0, 105.0);
        passenger.y_rot = passenger.y_rot + clamped - diff;
    }
}

/// 第一乘客是不是玩家：玩家 ID 从 1 起，实体 ID 从 `FIRST_ENTITY_ID` 起。
fn first_passenger_is_player(boat: &Boat) -> bool {
    boat.passengers
        .first()
        .is_some_and(|&uid| uid < super::FIRST_ENTITY_ID)
}

/// `Entity.baseTick`：船 AABB 里有岩浆 → 岩浆伤害 4.0；还剩着火刻数 → 火伤害 1.0。
///
/// `Entity.baseTick`: lava touching the boat's AABB deals 4.0, remaining fire
/// ticks deal 1.0.
fn environment_damage(me: &Entity, view: &dyn BlockView) -> f32 {
    let bb = me.bounding_box();
    let mut in_lava = false;
    for x in mth::floor(bb.min_x)..mth::ceil(bb.max_x) {
        for y in mth::floor(bb.min_y)..mth::ceil(bb.max_y) {
            for z in mth::floor(bb.min_z)..mth::ceil(bb.max_z) {
                if view
                    .fluid_state(BlockPos::new(x, y, z))
                    .is_tagged(fluid_tags::LAVA)
                {
                    in_lava = true;
                }
            }
        }
    }
    // `Entity.lavaHurt()`：`hurtServer(damageSources.lava(), 4.0F)`，每刻都来。
    if in_lava {
        return 4.0;
    }
    // `Entity.baseTick`：`remainingFireTicks % 20 == 0 && !isInLava()` 时
    // `hurtServer(damageSources.onFire(), 1.0F)`；其余刻只递减火刻数。
    if me.remaining_fire_ticks > 0 && me.remaining_fire_ticks % 20 == 0 {
        return 1.0;
    }
    0.0
}

/// `VehicleEntity.hurtServer` 的环境伤害路径：累计伤害、翻转方向，破坏时丢弃实体
/// 并掉出船物品和运输船箱内的东西（`Command::DropItem` 在命令落地时生成实体）。
///
/// The environmental half of `VehicleEntity.hurtServer`: accumulate damage and
/// flip the direction; on destruction discard and drop the boat item plus the
/// chest contents via `Command::DropItem`.
fn hurt_from_environment(boat: &mut Boat, me: &mut Entity, amount: f32, ctx: &mut dyn EntityCtx) {
    boat.hurt_dir = -boat.hurt_dir;
    boat.hurt_time = 10;
    boat.damage += amount * 10.0;
    if boat.damage <= 40.0 {
        return;
    }
    let mut stacks = vec![ItemStack::new(boat.drop_item(), 1)];
    if let Some(chest) = &boat.chest {
        stacks.extend(
            chest
                .items
                .iter()
                .filter(|stack| !stack.is_empty())
                .cloned(),
        );
    }
    me.discard();
    for stack in stacks {
        ctx.emit(Command::DropItem { stack }, me);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BlockGameRules, BlockHit,
        shape::CollisionContext,
        state::{BlockState, FluidState, SupportType},
    };
    use mc_bamboo_core::{Direction, LightLayer};
    use mc_bamboo_data::registries::entity_type as ids;

    /// 空世界：没有方块、流体、漏斗，全部区块都加载。`floatBoat` 只在
    /// 「空中→水/陆」分支用方块视图，这里四个场景都把 `old_status` 设成
    /// 和 `status` 相同，不会走到那个分支。
    struct EmptyView;

    impl BlockView for EmptyView {
        fn block_state(&self, _: BlockPos) -> BlockState {
            BlockState::AIR
        }
        fn moving_piston_shape(&self, _: BlockPos) -> Option<Vec<Aabb>> {
            None
        }
        fn shulker_box_shape(&self, _: BlockPos) -> Option<Aabb> {
            None
        }
        fn fluid_state(&self, _: BlockPos) -> FluidState {
            BlockState::AIR.fluid()
        }
        fn block_collisions(&self, _: Aabb, _: CollisionContext) -> Vec<(BlockPos, Vec<Aabb>)> {
            Vec::new()
        }
        fn find_supporting_block(&self, _: Vec3, _: Aabb, _: CollisionContext) -> Option<BlockPos> {
            None
        }
        fn clip_fall_damage_resetting(&self, _: Vec3, _: Vec3) -> bool {
            false
        }
        fn clip_collider_source_water(
            &self,
            _: Vec3,
            _: Vec3,
            _: CollisionContext,
        ) -> Option<BlockHit> {
            None
        }
        fn clip_collider(&self, _: Vec3, _: Vec3, _: CollisionContext) -> Option<BlockHit> {
            None
        }
        fn is_face_sturdy(&self, _: BlockPos, _: Direction, _: SupportType) -> bool {
            false
        }
        fn is_loaded(&self, _: BlockPos) -> bool {
            true
        }
        fn is_in_world_bounds(&self, _: BlockPos) -> bool {
            true
        }
        fn min_y(&self) -> i32 {
            -64
        }
        fn max_y(&self) -> i32 {
            320
        }
        fn brightness(&self, _: LightLayer, _: BlockPos) -> i32 {
            15
        }
        fn block_game_rules(&self) -> BlockGameRules {
            BlockGameRules::vanilla()
        }
        fn fluid_height(&self, _: FluidState, _: BlockPos) -> f32 {
            0.0
        }
        fn any_hopper_in(&self, _: BlockPos, _: BlockPos) -> bool {
            false
        }
    }

    fn scenario(status: Status) -> (Entity, Boat) {
        let mut me = Entity::boat(ids::OAK_BOAT, Vec3::new(0.0, 64.0, 0.0));
        me.delta = Vec3::new(1.0, 0.5, -1.0);
        let mut boat = Boat::new(ids::OAK_BOAT);
        boat.status = status;
        boat.old_status = status;
        boat.delta_rotation = 2.0;
        boat.water_level = 64.5;
        (me, boat)
    }

    #[test]
    fn in_air_uses_0_9_drag_and_gravity() {
        let (mut me, mut boat) = scenario(Status::InAir);
        float_boat(&mut boat, &mut me, &EmptyView);
        assert_eq!(me.delta.x, f64::from(0.9_f32));
        assert_eq!(me.delta.y, 0.5 - 0.04);
        assert_eq!(me.delta.z, -f64::from(0.9_f32));
        assert_eq!(boat.delta_rotation, 2.0 * 0.9_f32);
    }

    #[test]
    fn under_water_uses_0_45_drag_and_0_01_lift() {
        let (mut me, mut boat) = scenario(Status::UnderWater);
        float_boat(&mut boat, &mut me, &EmptyView);
        let expected_y = (0.5 - 0.04 + 0.009_999_999_776_482_582_f64 * (0.04 / 0.65)) * 0.75;
        assert_eq!(me.delta.x, f64::from(0.45_f32));
        assert_eq!(me.delta.y, expected_y);
        assert_eq!(me.delta.z, -f64::from(0.45_f32));
        assert_eq!(boat.delta_rotation, 2.0 * 0.45_f32);
    }

    #[test]
    fn under_flowing_water_uses_0_9_drag_and_small_gravity() {
        let (mut me, mut boat) = scenario(Status::UnderFlowingWater);
        float_boat(&mut boat, &mut me, &EmptyView);
        assert_eq!(me.delta.x, f64::from(0.9_f32));
        assert_eq!(me.delta.y, 0.5 - 0.0007);
        assert_eq!(me.delta.z, -f64::from(0.9_f32));
        assert_eq!(boat.delta_rotation, 2.0 * 0.9_f32);
    }

    #[test]
    fn in_water_uses_0_9_drag_and_water_level_lift() {
        let (mut me, mut boat) = scenario(Status::InWater);
        float_boat(&mut boat, &mut me, &EmptyView);
        let lift = (boat.water_level - me.position.y) / f64::from(me.dimensions.1);
        let expected_y = (0.5 - 0.04 + lift * (0.04 / 0.65)) * 0.75;
        assert_eq!(me.delta.x, f64::from(0.9_f32));
        assert_eq!(me.delta.y, expected_y);
        assert_eq!(me.delta.z, -f64::from(0.9_f32));
        assert_eq!(boat.delta_rotation, 2.0 * 0.9_f32);
    }
}
