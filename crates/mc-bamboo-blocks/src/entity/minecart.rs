//! 矿车实体，官方 `AbstractMinecart` 及它的几个子类，运动走 `OldMinecartBehavior`。
//!
//! 实验特性 `minecart_improvements` 的 `NewMinecartBehavior` 不在范围里。
//!
//! Minecart entities, vanilla `AbstractMinecart` and its subclasses, moved by
//! `OldMinecartBehavior`. The experimental `NewMinecartBehavior` is out of scope.

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{items, registries::entity_type};

use crate::{
    BlockState,
    entity::{Entity, EntityKind, EntityUid, ctx::EntityCtx},
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

/// `AbstractMinecart.tick` 的计时部分；铁轨运动在后续步骤里接进来。
///
/// The timer part of `AbstractMinecart.tick`; track movement is wired in later.
pub(crate) fn tick(me: &mut Entity, _ctx: &mut dyn EntityCtx) {
    if let EntityKind::Minecart(cart) = &mut me.kind {
        if cart.hurt_time > 0 {
            cart.hurt_time -= 1;
        }
        if cart.damage > 0.0 {
            cart.damage -= 1.0;
        }
    }
    me.first_tick = false;
}
