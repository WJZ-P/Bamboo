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

use mc_bamboo_protocol::item::ItemStack;

use super::{Entity, EntityUid, ctx::EntityCtx};

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

/// 船的一刻。`Entity::tick` 在 `common_tick` 之后调它。完整物理在后续提交里补。
///
/// One boat tick, called after `common_tick`. The full physics lands in a later
/// commit; for now this advances the timers the bytecode decrements before the
/// movement step.
pub fn tick(me: &mut Entity, _ctx: &mut dyn EntityCtx) {
    let super::EntityKind::Boat(boat) = &mut me.kind else {
        return;
    };
    boat.old_status = boat.status;
    if boat.hurt_time > 0 {
        boat.hurt_time -= 1;
    }
    if boat.damage > 0.0 {
        boat.damage -= 1.0;
    }
}
