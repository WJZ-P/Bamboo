//! 矿车实体，官方 `AbstractMinecart` 及它的几个子类，运动走 `OldMinecartBehavior`。
//!
//! 实验特性 `minecart_improvements` 的 `NewMinecartBehavior` 不在范围里。
//!
//! Minecart entities, vanilla `AbstractMinecart` and its subclasses, moved by
//! `OldMinecartBehavior`. The experimental `NewMinecartBehavior` is out of scope.

use mc_bamboo_core::{BlockPos, ChunkPos, Vec3, mth};
use mc_bamboo_data::{BlockAncestor, blocks, items, registries::entity_type, tags::block_tags};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{components::text::checked_component, item::ItemStack, text::Component};

use crate::{
    Aabb, BlockState, World,
    archive::tag,
    behaviour::machines::spawner::BaseSpawner,
    entity::{
        Entity, EntityKind, EntityUid, KindPublic, PrimedTnt,
        ctx::{BlockView, Command, EntityCtx},
        mob::{MobType, living},
    },
    game_event::{GameEvent, dispatch::EventContext},
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
    Spawner,
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
            Self::Spawner => entity_type::SPAWNER_MINECART,
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
            Self::Spawner => items::MINECART,
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
            entity_type::SPAWNER_MINECART => Self::Spawner,
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
    /// 实体的可选自定义名。 / The entity's optional custom name.
    pub custom_name: Option<Tag>,
    /// 是否显示自定义名。 / Whether the name is visible.
    pub custom_name_visible: bool,
    /// 最近一次设定的俯仰，旧矿车行为会在一刻结束前归零。
    /// The current pitch, which the old behavior resets before the tick ends.
    pub pitch: f32,
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
    /// 命令方块矿车独有的状态。 / State held by command block carts.
    pub command_block: Option<CommandBlock>,
    pub(crate) spawner: Option<Box<BaseSpawner>>,
}

impl Minecart {
    /// 按种类新建一辆矿车。
    ///
    /// Build a fresh minecart of the given kind.
    pub fn new(kind: MinecartKind) -> Self {
        Self {
            entity_type: kind.entity_type(),
            custom_name: None,
            custom_name_visible: false,
            pitch: 0.0,
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
            command_block: (kind == MinecartKind::CommandBlock).then(CommandBlock::default),
            spawner: (kind == MinecartKind::Spawner).then(Box::default),
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
        if matches!(self.kind(), MinecartKind::CommandBlock) {
            items::MINECART
        } else {
            self.kind().item()
        }
    }

    /// `VehicleEntity.destroy` 给掉落的载具物品写上实体名字。
    ///
    /// `VehicleEntity.destroy` copies the entity name onto the dropped vehicle item.
    pub fn drop_stack(&self) -> ItemStack {
        super::hanging::item_with_custom_name(self.drop_item(), self.custom_name.as_ref())
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

/// 命令方块矿车的 `BaseCommandBlock` 和激活冷却。
///
/// A command cart's `BaseCommandBlock` and rail activation cooldown.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandBlock {
    /// 待执行命令。 / The command to run.
    pub command: String,
    /// 是否接收命令反馈。 / Whether the command source accepts feedback.
    pub track_output: bool,
    /// 成功执行的分支数。 / Number of successful command branches.
    pub success_count: i32,
    /// 最后一条命令反馈。 / Most recent command feedback.
    pub last_output: Option<Component>,
    /// 上次执行的世界刻号，-1 表示无记录。 / Last execution game time, or -1.
    pub last_execution: i64,
    /// 是否记录世界刻号。 / Whether to remember the execution game time.
    pub update_last_execution: bool,
    last_activated: i32,
    synced_command: String,
    synced_last_output: Option<Tag>,
}

impl Default for CommandBlock {
    fn default() -> Self {
        Self {
            command: String::new(),
            track_output: true,
            success_count: 0,
            last_output: None,
            last_execution: -1,
            update_last_execution: true,
            last_activated: 0,
            synced_command: String::new(),
            synced_last_output: None,
        }
    }
}

impl CommandBlock {
    /// 更新命令并重置成功次数。 / Update the command and reset the success count.
    pub fn set_command(&mut self, command: String) {
        self.command = command;
        self.success_count = 0;
    }

    /// `MinecartCommandBase.onUpdated`，刷新客户端编辑器读到的值。
    ///
    /// Refresh the values visible to the client editor.
    pub fn update_synced_data(&mut self) {
        self.synced_command.clone_from(&self.command);
        self.synced_last_output = self.last_output.as_ref().map(|output| output.tag().clone());
    }

    pub(crate) fn read(raw: &Tag) -> Self {
        let track_output = tag::bool_or(raw, "TrackOutput", true);
        let update_last_execution = tag::bool_or(raw, "UpdateLastExecution", true);
        let mut command = Self {
            command: tag::field(raw, "Command")
                .and_then(|value| match value {
                    Tag::String(command) => Some(command.clone()),
                    _ => None,
                })
                .unwrap_or_default(),
            success_count: tag::int_or(raw, "SuccessCount", 0),
            track_output,
            last_output: if track_output {
                tag::field(raw, "LastOutput").and_then(checked_component)
            } else {
                None
            },
            update_last_execution,
            last_execution: if update_last_execution {
                tag::long_or(raw, "LastExecution", -1)
            } else {
                -1
            },
            ..Self::default()
        };
        command.update_synced_data();
        command
    }

    pub(crate) fn write(&self, out: &mut Vec<(String, Tag)>) {
        tag::put(out, "Command", Tag::String(self.command.clone()));
        tag::put(out, "SuccessCount", Tag::Int(self.success_count));
        tag::put(out, "TrackOutput", Tag::Byte(i8::from(self.track_output)));
        tag::put(
            out,
            "UpdateLastExecution",
            Tag::Byte(i8::from(self.update_last_execution)),
        );
        if self.track_output
            && let Some(output) = &self.last_output
        {
            tag::put(out, "LastOutput", output.tag().clone());
        }
        if self.update_last_execution && self.last_execution != -1 {
            tag::put(out, "LastExecution", Tag::Long(self.last_execution));
        }
    }
}

/// 矿车经过激活铁轨时交给游戏层的执行来源。
///
/// The command source handed to the game layer as the cart crosses the rail.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CommandActivation {
    /// 发起命令的矿车。 / The cart issuing the command.
    pub entity: EntityUid,
    /// 激活时的位置。 / The position at activation.
    pub position: Vec3,
    /// 激活时的水平朝向。 / The yaw at activation.
    pub yaw: f32,
    /// 激活时的俯仰。 / The pitch at activation.
    pub pitch: f32,
    /// 激活所在的世界刻。 / Game time of the activation.
    pub game_time: i64,
}

/// 矿车公开的那一半，给同步和存档读。
///
/// The public half of a minecart, read by sync and storage.
#[derive(Debug, Clone, PartialEq)]
pub struct MinecartPublic {
    pub entity_type: i32,
    /// 实体的可选自定义名。 / The entity's optional custom name.
    pub custom_name: Option<Tag>,
    /// 是否显示自定义名。 / Whether the name is visible.
    pub custom_name_visible: bool,
    /// 矿车的俯仰。 / The cart's pitch.
    pub pitch: f32,
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
    /// 命令编辑器同步的内容。 / Command editor metadata.
    pub command: String,
    /// 命令编辑器同步的最后输出。 / Last output synced to the editor.
    pub command_output: Option<Tag>,
    /// 比较器读取的成功次数。 / Success count for comparators.
    pub success_count: Option<i32>,
}

impl MinecartPublic {
    /// 从完整状态抄出公开的一半。
    ///
    /// Copy the public half out of the full state.
    pub fn of(cart: &Minecart) -> Self {
        Self {
            entity_type: cart.entity_type,
            custom_name: cart.custom_name.clone(),
            custom_name_visible: cart.custom_name_visible,
            pitch: cart.pitch,
            passengers: cart.passengers.clone(),
            hurt_time: cart.hurt_time,
            hurt_dir: cart.hurt_dir,
            damage: cart.damage,
            custom_display_block: cart.custom_display_block.map(|state| state.0 as i32),
            display_offset: cart.display_offset,
            has_fuel: cart.kind().is_furnace() && cart.fuel > 0,
            flipped: cart.flipped,
            command: cart
                .command_block
                .as_ref()
                .map_or_else(String::new, |command| command.synced_command.clone()),
            command_output: cart
                .command_block
                .as_ref()
                .and_then(|command| command.synced_last_output.clone()),
            success_count: cart
                .command_block
                .as_ref()
                .map(|command| command.success_count),
        }
    }
}

/// 矿车注册时的乘客附着点，村民和流浪商人另走零向量。
///
/// The registered passenger anchor; villagers and wandering traders use zero.
pub fn passenger_attachment_point() -> Vec3 {
    Vec3::new(0.0, 0.1875, 0.0)
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
fn move_along_track(me: &mut Entity, cart: &mut Minecart, ctx: &mut dyn EntityCtx) {
    let view = ctx.blocks();
    let pos = current_rail_pos(view, me.position);
    let state = view.block_state(pos);
    me.fall_distance = 0.0;
    let x0 = me.position.x;
    let y0 = me.position.y;
    let z0 = me.position.z;
    let start = get_pos(view, x0, y0, z0);
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
    move_minecart(me, cart, ctx, movement);
    let view = ctx.blocks();

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
        get_pos(view, me.position.x, me.position.y, me.position.z),
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
            me.delta = start_from_conductor(view, pos, shape, v);
        }
    }
}

/// `AbstractMinecart.comeOffTrack`：出轨后的普通物理。
///
/// `AbstractMinecart.comeOffTrack`: ordinary physics off the rails.
fn come_off_track(me: &mut Entity, cart: &Minecart, ctx: &mut dyn EntityCtx) {
    let max = max_speed(me, cart);
    let v = me.delta;
    me.delta = Vec3::new(v.x.clamp(-max, max), v.y, v.z.clamp(-max, max));
    if me.on_ground {
        me.delta = me.delta.scale(0.5);
    }
    let movement = me.delta;
    move_minecart(me, cart, ctx, movement);
    if !me.on_ground {
        me.delta = me.delta.scale(0.95);
    }
}

fn move_minecart(me: &mut Entity, cart: &Minecart, ctx: &mut dyn EntityCtx, movement: Vec3) {
    let view = ctx.blocks();
    let multiplier = me.stuck_speed_multiplier;
    let swept = if multiplier.length_sqr() > 1.0e-7 {
        movement.multiply(multiplier.x, multiplier.y, multiplier.z)
    } else {
        movement
    };
    let bounds = me
        .bounding_box()
        .expanded_towards(swept.x, swept.y, swept.z)
        .inflate(1.0e-7, 1.0e-7, 1.0e-7);
    let root = living::root_vehicle(me.id, me.vehicle, ctx);
    let mut colliders = Vec::new();
    ctx.for_each_other_in(bounds, &mut |other| {
        let pushable = match &other.kind {
            KindPublic::Mob(mob) => {
                mob.is_alive()
                    && mob.mob_type.is_pushable()
                    && view.chunk_ticks_entities(ChunkPos::containing(
                        other.position.x,
                        other.position.z,
                    ))
                    && !living::is_climbing(
                        mob.mob_type,
                        mob.spider_flags != 0,
                        other.block_position(),
                        view,
                    )
            }
            KindPublic::Boat(_) | KindPublic::Minecart(_) => true,
            _ => false,
        };
        if pushable && !other.removed && living::root_vehicle(other.uid, other.vehicle, ctx) != root
        {
            colliders.push(other.bounding_box());
        }
    });
    view.for_each_player(&mut |player| {
        let bounds_player = player_bounds(player);
        if player.alive
            && !player.spectator
            && bounds.intersects(bounds_player)
            && !cart.passengers.contains(&player.uid)
        {
            colliders.push(bounds_player);
        }
    });
    me.move_entity_with_colliders(ctx, movement, &colliders);
}

fn player_bounds(player: &super::ctx::PlayerView) -> Aabb {
    let half = f64::from(player.width) * 0.5;
    let p = player.position;
    Aabb::new(
        p.x - half,
        p.y,
        p.z - half,
        p.x + half,
        p.y + f64::from(player.height),
        p.z + half,
    )
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
    let mut ejected = Vec::new();
    let mut command_activation = None;
    if on_rails {
        move_along_track(me, &mut cart, ctx);
        if state.is(blocks::ACTIVATOR_RAIL) {
            let powered = state.try_get(POWERED).unwrap_or(false);
            if powered && cart.kind().is_rideable() {
                ejected = std::mem::take(&mut cart.passengers);
            }
            if powered
                && let Some(command) = &mut cart.command_block
                && me.tick_count.wrapping_sub(command.last_activated) >= 4
            {
                command.last_activated = me.tick_count;
                command_activation = Some(CommandActivation {
                    entity: me.id,
                    position: me.position,
                    yaw: me.y_rot,
                    pitch: cart.pitch,
                    game_time: ctx.blocks().game_time(),
                });
            }
            activate_minecart(&mut cart, powered);
        }
    } else {
        come_off_track(me, &cart, ctx);
    }
    let detectors = detector_rails(ctx.blocks(), me);
    me.apply_effects_from_blocks(ctx.blocks());
    cart.pitch = 0.0;

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
    if let Some(activation) = command_activation {
        ctx.emit(Command::ActivateMinecartCommand(activation), me);
    }
    // `DetectorRailBlock.checkPressed`：归还 `EntityKind::Minecart` 后再触发，
    // 这样 `check_detector_rail` 能认出正在 tick 的矿车自己。
    // Trigger `DetectorRailBlock.checkPressed` after restoring `EntityKind::Minecart`
    // so `check_detector_rail` recognizes the ticking cart itself.
    for at in detectors {
        ctx.emit(Command::MinecartDetectorRail { pos: at }, me);
    }
    if !ejected.is_empty() {
        ctx.emit(
            Command::MinecartEject {
                passengers: ejected,
            },
            me,
        );
    }
    ctx.emit(Command::MinecartPushAndPickup, me);
    tick_furnace(me);
    tick_tnt(me, ctx);
    // `MinecartHopper.tick` → `tryConsumeItems`：活着的、启用的漏斗矿车吸一次。
    // `MinecartHopper.tick` → `tryConsumeItems`: a live, enabled hopper cart sucks once.
    let hopper_ready = matches!(&me.kind, EntityKind::Minecart(cart)
        if cart.kind() == MinecartKind::Hopper && cart.enabled && !me.removed);
    if hopper_ready {
        ctx.emit(Command::MinecartHopperSuck, me);
    }
    if matches!(&me.kind, EntityKind::Minecart(cart) if cart.spawner.is_some()) {
        ctx.emit(Command::TickMinecartSpawner, me);
    }
}

pub(super) fn tick_spawner(me: &mut Entity, world: &mut World) {
    let position = me.block_position();
    if let EntityKind::Minecart(cart) = &mut me.kind
        && let Some(spawner) = &mut cart.spawner
    {
        spawner.server_tick(world, position, Some(me.id));
    }
}

pub(super) fn push_and_pickup(me: &mut Entity, world: &mut World) {
    let EntityKind::Minecart(cart) = &me.kind else {
        return;
    };
    let fast =
        cart.kind().is_rideable() && me.delta.x * me.delta.x + me.delta.z * me.delta.z >= 0.01;
    let bounds = me
        .bounding_box()
        .inflate(f64::from(0.2_f32), 0.0, f64::from(0.2_f32));
    let team = world
        .level()
        .entity_team(&crate::archive::entity_uuid(me.id).to_be_bytes());
    let mut candidates = Vec::new();
    world
        .level()
        .for_each_entity_in_section_order(bounds, &mut |other| {
            if other.uid != me.id
                && !other.removed
                && (matches!(other.kind, KindPublic::Minecart(_))
                    || fast && matches!(other.kind, KindPublic::Mob(_) | KindPublic::Boat(_)))
            {
                candidates.push(other.uid);
            }
        });
    let root = root_vehicle(me, world);
    for uid in candidates {
        let Some(mut other) = world.level_mut().take_entity(uid) else {
            continue;
        };
        let mounted = collide_with_entity(me, &mut other, fast, team, root, world);
        world.level_mut().put_entity(other);
        if mounted {
            world.game_event(
                GameEvent::EntityMount,
                me.position,
                EventContext {
                    source_entity: Some(uid),
                    ..EventContext::default()
                },
            );
        }
    }
    if fast {
        push_players(me, bounds, team, world);
    }
}

fn collide_with_entity(
    me: &mut Entity,
    other: &mut Entity,
    fast: bool,
    team: Option<u32>,
    root: EntityUid,
    world: &World,
) -> bool {
    let EntityKind::Minecart(cart) = &mut me.kind else {
        return false;
    };
    let other_cart = matches!(other.kind, EntityKind::Minecart(_));
    if fast {
        let uuid = other.kind.mob().map_or_else(
            || crate::archive::entity_uuid(other.id).to_be_bytes(),
            |mob| mob.uuid,
        );
        if !can_push_teams(team, world.level().entity_team(&uuid), world) {
            return false;
        }
        if other.kind.mob().is_some() {
            let pushable = world
                .level()
                .chunk_ticks_entities(ChunkPos::containing(other.position.x, other.position.z))
                && other.with_mob(|entity, mob| {
                    mob.health > 0.0
                        && mob.mob_type.is_pushable()
                        && !living::on_climbable(entity, mob, world)
                }) == Some(true);
            if !pushable {
                return false;
            }
        }
        if !other_cart
            && !matches!(other.kind.mob(), Some(mob) if mob.mob_type == MobType::IronGolem)
            && cart.passengers.is_empty()
            && other.vehicle.is_none()
        {
            if other.boarding_cooldown <= 0 && other.id != root {
                other.vehicle = Some(me.id);
                super::riding::add_passenger(&mut cart.passengers, other.id, false);
                return true;
            }
            return false;
        }
    }
    if other_cart {
        push_minecart(other, me);
    } else if root_vehicle(other, world) != root {
        let other_is_vehicle = match &other.kind {
            EntityKind::Boat(boat) => {
                if me.position.y > other.position.y {
                    return false;
                }
                !boat.passengers.is_empty()
            }
            EntityKind::Mob(mob) => {
                if mob.no_physics || living::is_sleeping_for_push(mob) {
                    return false;
                }
                mob.is_vehicle()
            }
            _ => return false,
        };
        if let Some((dx, dz)) = living::push_amount(other.position, me.position) {
            let push = Vec3::new(dx, 0.0, dz);
            if !other_is_vehicle {
                other.delta = other.delta.add(push.scale(-1.0));
                other.with_mob(|_, mob| mob.needs_sync = true);
            }
            if cart.passengers.is_empty() {
                me.delta = me.delta.add(push);
            }
        }
    }
    false
}

fn root_vehicle(entity: &Entity, world: &World) -> EntityUid {
    let mut root = entity.id;
    let mut current = entity.vehicle;
    for _ in 0..64 {
        let Some(uid) = current else { break };
        root = uid;
        current = world
            .level()
            .entity_public(uid)
            .and_then(|vehicle| vehicle.vehicle);
    }
    root
}

fn can_push_teams(team: Option<u32>, other: Option<u32>, world: &World) -> bool {
    let rule = team.map_or(0, |index| world.level().team_collision_rule(index));
    let other_rule = other.map_or(0, |index| world.level().team_collision_rule(index));
    let allied = team.is_some() && team == other;
    [rule, other_rule].into_iter().all(|rule| match rule {
        1 => false,
        2 => allied,
        3 => !allied,
        _ => true,
    })
}

fn push_players(me: &mut Entity, bounds: Aabb, team: Option<u32>, world: &mut World) {
    let EntityKind::Minecart(cart) = &me.kind else {
        return;
    };
    let mut players = Vec::new();
    BlockView::for_each_player(world, &mut |player| {
        if player.alive
            && !player.spectator
            && player_bounds(player).intersects(bounds)
            && !cart.passengers.contains(&player.uid)
            && can_push_teams(team, player.team, world)
        {
            players.push(*player);
        }
    });
    for player in players {
        if let Some((dx, dz)) = living::push_amount(player.position, me.position) {
            let push = Vec3::new(dx, 0.0, dz);
            if cart.passengers.is_empty() {
                me.delta = me.delta.add(push);
            }
            world.push_player(player.uid, push.scale(-1.0));
        }
    }
}

/// `AbstractMinecart.push` 从碰撞对方发起，按被调用的矿车朝向门控。
///
/// `AbstractMinecart.push` runs on the contacted cart and checks that cart's heading.
pub(super) fn push_minecart(cart: &mut Entity, other: &mut Entity) {
    let EntityKind::Minecart(state) = &cart.kind else {
        return;
    };
    if state.passengers.contains(&other.id) || other.kind.mob().is_some_and(|mob| mob.no_physics) {
        return;
    }
    let offset = Vec3::new(
        other.position.x - cart.position.x,
        0.0,
        other.position.z - cart.position.z,
    );
    let dist_sq = offset.length_sqr();
    if dist_sq < f64::from(1.0e-4_f32) {
        return;
    }
    let distance = dist_sq.sqrt();
    let normal = Vec3::new(offset.x / distance, 0.0, offset.z / distance);
    let scale = (1.0 / distance).min(1.0);
    let push = normal.scale(scale).scale(f64::from(0.1_f32)).scale(0.5);
    let EntityKind::Minecart(other_state) = &other.kind else {
        cart.delta = cart.delta.add(Vec3::new(-push.x, 0.0, -push.z));
        other.delta = other.delta.add(Vec3::new(push.x / 4.0, 0.0, push.z / 4.0));
        other.with_mob(|_, mob| mob.needs_sync = true);
        return;
    };
    let yaw = f64::from(cart.y_rot * 0.017_453_292_f32);
    let facing = Vec3::new(f64::from(mth::cos(yaw)), 0.0, f64::from(mth::sin(yaw))).normalize();
    if (normal.x * facing.x + normal.z * facing.z).abs() < f64::from(0.8_f32) {
        return;
    }
    let a = cart.delta;
    let b = other.delta;
    match (state.kind().is_furnace(), other_state.kind().is_furnace()) {
        (true, false) => {
            cart.delta = a.multiply(0.95, 1.0, 0.95);
            other.delta = b
                .multiply(0.2, 1.0, 0.2)
                .add(Vec3::new(a.x + push.x, 0.0, a.z + push.z));
        }
        (false, true) => {
            cart.delta = a
                .multiply(0.2, 1.0, 0.2)
                .add(Vec3::new(b.x - push.x, 0.0, b.z - push.z));
            other.delta = b.multiply(0.95, 1.0, 0.95);
        }
        _ => {
            let x = (a.x + b.x) / 2.0;
            let z = (a.z + b.z) / 2.0;
            cart.delta = a
                .multiply(0.2, 1.0, 0.2)
                .add(Vec3::new(x - push.x, 0.0, z - push.z));
            other.delta = b
                .multiply(0.2, 1.0, 0.2)
                .add(Vec3::new(x + push.x, 0.0, z + push.z));
        }
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
