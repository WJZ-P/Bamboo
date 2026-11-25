//! 记忆：官方 `MemoryModuleType`、`MemorySlot` 和 `Brain` 里读写记忆的那几个方法。
//!
//! 每种记忆一个槽，槽里有没有值、剩几刻过期都照 `MemorySlot`：清空时过期时间回到
//! `Long.MAX_VALUE`，`getTimeUntilExpiry` 对空槽也返回它。没有登记的记忆读作不存在，
//! 写进去也不生效，和官方 `getMemorySlotIfPresent` 返回 `null` 一样。
//!
//! Memories: vanilla `MemoryModuleType`, `MemorySlot` and the memory accessors of
//! `Brain`. A cleared slot's time-to-live goes back to `Long.MAX_VALUE`, which
//! `getTimeUntilExpiry` also reports for empty slots. Unregistered memories read as
//! absent and ignore writes, like vanilla's `getMemorySlotIfPresent` returning `null`.

use std::sync::Arc;

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_protocol::components::value::GlobalPos;

use super::super::{DamageSource, MobType};
use crate::entity::EntityUid;

/// `MemorySlot.NEVER_EXPIRE`。
const NEVER_EXPIRE: i64 = i64::MAX;

/// Bamboo 建模的记忆种类，名字是官方注册名。
///
/// The memory types Bamboo models, named after the vanilla registry ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MemoryType {
    WalkTarget,
    LookTarget,
    Path,
    CantReachWalkTargetSince,
    NearestLivingEntities,
    NearestVisibleLivingEntities,
    NearestVisibleAdult,
    NearestPlayers,
    NearestVisiblePlayer,
    NearestVisibleAttackablePlayer,
    NearestVisibleWantedItem,
    HurtBy,
    HurtByEntity,
    TemptingPlayer,
    TemptationCooldownTicks,
    GazeCooldownTicks,
    IsPanicking,
    BreedTarget,
    DangerDetectedRecently,
    LongJumpCooldownTicks,
    LongJumpMidJump,
    RamCooldownTicks,
    RamTarget,
    NearestVisibleAttackablePlayers,
    /// 青蛙的记忆。`is_in_water` / `is_pregnant` 带 `Unit.CODEC`（进存档），
    /// `nearest_attackable` / `attack_target` / `unreachable_tongue_targets` 没编解码器
    /// （不进存档）。追加在末尾，不改既有种类的下标和 `filled()` 顺序。
    ///
    /// The frog's memories. `is_in_water` / `is_pregnant` carry a `Unit.CODEC` (saved);
    /// `nearest_attackable` / `attack_target` / `unreachable_tongue_targets` have no codec
    /// (not saved). Appended last so the existing types keep their indices and `filled()` order.
    IsInWater,
    IsPregnant,
    NearestAttackable,
    AttackTarget,
    /// `FrogAttackablesSensor.requires()`（`UNREACHABLE_TONGUE_TARGETS`）：官方用
    /// `register("unreachable_tongue_targets")`，没有编解码器，不进存档。
    ///
    /// `FrogAttackablesSensor.requires()` (`UNREACHABLE_TONGUE_TARGETS`): vanilla uses
    /// `register("unreachable_tongue_targets")` with no codec, so it is not saved.
    UnreachableTongueTargets,
    /// 美西螈的记忆：装死倒计时（`register("play_dead_ticks", Codec.INT)`，进存档）、
    /// 捕猎冷却（`register("has_hunting_cooldown", Unit.CODEC)`，进存档）和近战冷却
    /// （`register("attack_cooling_down", Unit.CODEC)`）。追加在末尾，不改既有下标。
    ///
    /// The axolotl's memories: the play-dead countdown (`register("play_dead_ticks",
    /// Codec.INT)`, saved), the hunting cooldown (`register("has_hunting_cooldown",
    /// Unit.CODEC)`, saved) and the melee cooldown (`register("attack_cooling_down",
    /// Unit.CODEC)`). Appended last so the existing indices stay put.
    PlayDeadTicks,
    HuntingCooldown,
    AttackCoolingDown,
    /// 村民的记忆：追加在末尾，保持既有种类下标和 `filled()` 顺序不变（总数 49 <= 64，
    /// 仍落在 `u64` 位掩码里）。带编解码器的进存档：`home` / `job_site` /
    /// `potential_job_site` / `meeting_point`（`GlobalPos.CODEC`）、`last_worked_at_poi` /
    /// `last_slept` / `last_woken`（`Codec.LONG`）、`golem_detected_recently`（`Codec.BOOL`）、
    /// `item_pickup_cooldown_ticks`（`Codec.INT`）；其余不进存档。
    ///
    /// Villager memories appended last so existing indices and `filled()` order stay put
    /// (49 <= 64, fitting `u64` masks). Those with a codec are saved: `home` / `job_site` /
    /// `potential_job_site` / `meeting_point` (`GlobalPos.CODEC`), `last_worked_at_poi` /
    /// `last_slept` / `last_woken` (`Codec.LONG`), `golem_detected_recently` (`Codec.BOOL`),
    /// `item_pickup_cooldown_ticks` (`Codec.INT`); the rest are not saved.
    Home,
    JobSite,
    PotentialJobSite,
    MeetingPoint,
    SecondaryJobSite,
    VisibleVillagerBabies,
    InteractionTarget,
    DoorsToClose,
    NearestBed,
    NearestHostile,
    HidingPlace,
    HeardBellTime,
    LastWorkedAtPoi,
    GolemDetectedRecently,
    LastSlept,
    LastWoken,
    ItemPickupCooldownTicks,
    /// 鹦鹉螺的 `charge_cooldown_ticks`（`Codec.INT`）和
    /// `attack_target_cooldown`（`Codec.INT`）。追加在末尾，不改既有下标。
    ///
    /// The nautilus's `charge_cooldown_ticks` and `attack_target_cooldown`.
    ChargeCooldownTicks,
    AttackTargetCooldown,
    /// 铜傀儡的 `transport_items_cooldown_ticks`（`register(name)` 无编解码器，
    /// 不进存档）。追加在末尾，不改既有下标。
    ///
    /// The copper golem's `transport_items_cooldown_ticks` (registered without a
    /// codec, so it is not saved). Appended last to keep existing indices.
    TransportItemsCooldownTicks,
    /// `TransportItemsBetweenContainers` 的 `VISITED_BLOCK_POSITIONS` /
    /// `UNREACHABLE_TRANSPORT_BLOCK_POSITIONS`：官方 `Set<GlobalPos>`，
    /// 都带 `GlobalPos.CODEC.listOf()` 编解码器，进存档。追加在末尾。
    ///
    /// The transport behaviour's visited / unreachable position sets, both saved.
    VisitedBlockPositions,
    UnreachableTransportBlockPositions,
    /// 嗅探兽的记忆。带编解码器的进存档：`sniffer_explored_positions`
    /// （`GlobalPos.CODEC.listOf()`）和 `sniff_cooldown`（`Unit.CODEC`）；其余
    /// （`sniffer_sniffing_target` / `sniffer_digging` / `sniffer_happy`）用
    /// `register("...")` 无编解码器，不进存档。追加在末尾，不改既有下标。
    ///
    /// The sniffer's memories. Those with a codec are saved: `sniffer_explored_positions`
    /// (`GlobalPos.CODEC.listOf()`) and `sniff_cooldown` (`Unit.CODEC`); the rest
    /// (`sniffer_sniffing_target` / `sniffer_digging` / `sniffer_happy`) are registered
    /// without a codec and are not saved. Appended last so the existing indices stay put.
    SnifferExploredPositions,
    SnifferSniffingTarget,
    SnifferDigging,
    SnifferHappy,
    SniffCooldown,
}

impl MemoryType {
    /// 记忆种类的个数，槽表的长度。
    ///
    /// The number of memory types: the length of the slot table.
    pub const COUNT: usize = 59;

    /// 全部种类，按枚举顺序。
    ///
    /// Every type, in enum order.
    pub const ALL: [Self; Self::COUNT] = [
        Self::WalkTarget,
        Self::LookTarget,
        Self::Path,
        Self::CantReachWalkTargetSince,
        Self::NearestLivingEntities,
        Self::NearestVisibleLivingEntities,
        Self::NearestVisibleAdult,
        Self::NearestPlayers,
        Self::NearestVisiblePlayer,
        Self::NearestVisibleAttackablePlayer,
        Self::NearestVisibleWantedItem,
        Self::HurtBy,
        Self::HurtByEntity,
        Self::TemptingPlayer,
        Self::TemptationCooldownTicks,
        Self::GazeCooldownTicks,
        Self::IsPanicking,
        Self::BreedTarget,
        Self::DangerDetectedRecently,
        Self::LongJumpCooldownTicks,
        Self::LongJumpMidJump,
        Self::RamCooldownTicks,
        Self::RamTarget,
        Self::NearestVisibleAttackablePlayers,
        Self::IsInWater,
        Self::IsPregnant,
        Self::NearestAttackable,
        Self::AttackTarget,
        Self::UnreachableTongueTargets,
        Self::PlayDeadTicks,
        Self::HuntingCooldown,
        Self::AttackCoolingDown,
        Self::Home,
        Self::JobSite,
        Self::PotentialJobSite,
        Self::MeetingPoint,
        Self::SecondaryJobSite,
        Self::VisibleVillagerBabies,
        Self::InteractionTarget,
        Self::DoorsToClose,
        Self::NearestBed,
        Self::NearestHostile,
        Self::HidingPlace,
        Self::HeardBellTime,
        Self::LastWorkedAtPoi,
        Self::GolemDetectedRecently,
        Self::LastSlept,
        Self::LastWoken,
        Self::ItemPickupCooldownTicks,
        Self::ChargeCooldownTicks,
        Self::AttackTargetCooldown,
        Self::TransportItemsCooldownTicks,
        Self::VisitedBlockPositions,
        Self::UnreachableTransportBlockPositions,
        Self::SnifferExploredPositions,
        Self::SnifferSniffingTarget,
        Self::SnifferDigging,
        Self::SnifferHappy,
        Self::SniffCooldown,
    ];

    /// 官方注册名（不带命名空间）。
    ///
    /// The vanilla registry path (without the namespace).
    pub const fn name(self) -> &'static str {
        match self {
            Self::WalkTarget => "walk_target",
            Self::LookTarget => "look_target",
            Self::Path => "path",
            Self::CantReachWalkTargetSince => "cant_reach_walk_target_since",
            Self::NearestLivingEntities => "mobs",
            Self::NearestVisibleLivingEntities => "visible_mobs",
            Self::NearestVisibleAdult => "nearest_visible_adult",
            Self::NearestPlayers => "nearest_players",
            Self::NearestVisiblePlayer => "nearest_visible_player",
            Self::NearestVisibleAttackablePlayer => "nearest_visible_targetable_player",
            Self::NearestVisibleWantedItem => "nearest_visible_wanted_item",
            Self::HurtBy => "hurt_by",
            Self::HurtByEntity => "hurt_by_entity",
            Self::TemptingPlayer => "tempting_player",
            Self::TemptationCooldownTicks => "temptation_cooldown_ticks",
            Self::GazeCooldownTicks => "gaze_cooldown_ticks",
            Self::IsPanicking => "is_panicking",
            Self::BreedTarget => "breed_target",
            Self::DangerDetectedRecently => "danger_detected_recently",
            Self::LongJumpCooldownTicks => "long_jump_cooling_down",
            Self::LongJumpMidJump => "long_jump_mid_jump",
            Self::RamCooldownTicks => "ram_cooldown_ticks",
            Self::RamTarget => "ram_target",
            Self::NearestVisibleAttackablePlayers => "nearest_visible_targetable_players",
            Self::IsInWater => "is_in_water",
            Self::IsPregnant => "is_pregnant",
            Self::NearestAttackable => "nearest_attackable",
            Self::AttackTarget => "attack_target",
            Self::UnreachableTongueTargets => "unreachable_tongue_targets",
            Self::PlayDeadTicks => "play_dead_ticks",
            Self::HuntingCooldown => "has_hunting_cooldown",
            Self::AttackCoolingDown => "attack_cooling_down",
            Self::Home => "home",
            Self::JobSite => "job_site",
            Self::PotentialJobSite => "potential_job_site",
            Self::MeetingPoint => "meeting_point",
            Self::SecondaryJobSite => "secondary_job_site",
            Self::VisibleVillagerBabies => "visible_villager_babies",
            Self::InteractionTarget => "interaction_target",
            Self::DoorsToClose => "doors_to_close",
            Self::NearestBed => "nearest_bed",
            Self::NearestHostile => "nearest_hostile",
            Self::HidingPlace => "hiding_place",
            Self::HeardBellTime => "heard_bell_time",
            Self::LastWorkedAtPoi => "last_worked_at_poi",
            Self::GolemDetectedRecently => "golem_detected_recently",
            Self::LastSlept => "last_slept",
            Self::LastWoken => "last_woken",
            Self::ItemPickupCooldownTicks => "item_pickup_cooldown_ticks",
            Self::ChargeCooldownTicks => "charge_cooldown_ticks",
            Self::AttackTargetCooldown => "attack_target_cooldown",
            Self::TransportItemsCooldownTicks => "transport_items_cooldown_ticks",
            Self::VisitedBlockPositions => "visited_block_positions",
            Self::UnreachableTransportBlockPositions => "unreachable_transport_block_positions",
            Self::SnifferExploredPositions => "sniffer_explored_positions",
            Self::SnifferSniffingTarget => "sniffer_sniffing_target",
            Self::SnifferDigging => "sniffer_digging",
            Self::SnifferHappy => "sniffer_happy",
            Self::SniffCooldown => "sniff_cooldown",
        }
    }

    /// 按注册名（可带 `minecraft:`）找种类。
    ///
    /// Finds a type by registry name, with or without `minecraft:`.
    pub fn by_name(name: &str) -> Option<Self> {
        let path = name.strip_prefix("minecraft:").unwrap_or(name);
        Self::ALL.into_iter().find(|kind| kind.name() == path)
    }

    /// `MemoryModuleType.canSerialize()`：注册时带了编解码器的才进存档。
    ///
    /// `MemoryModuleType.canSerialize()`: only types registered with a codec are saved.
    pub const fn serializable(self) -> bool {
        matches!(
            self,
            Self::TemptationCooldownTicks
                | Self::GazeCooldownTicks
                | Self::IsPanicking
                | Self::DangerDetectedRecently
                | Self::LongJumpCooldownTicks
                | Self::RamCooldownTicks
                | Self::IsInWater
                | Self::IsPregnant
                | Self::PlayDeadTicks
                | Self::HuntingCooldown
                | Self::Home
                | Self::JobSite
                | Self::PotentialJobSite
                | Self::MeetingPoint
                | Self::LastWorkedAtPoi
                | Self::GolemDetectedRecently
                | Self::LastSlept
                | Self::LastWoken
                | Self::ItemPickupCooldownTicks
                | Self::SnifferExploredPositions
                | Self::SniffCooldown
                | Self::VisitedBlockPositions
                | Self::UnreachableTransportBlockPositions
        )
    }

    const fn index(self) -> usize {
        self as usize
    }

    /// 这个记忆在 `u64` 位掩码里的位，给行为入口条件预编译用。
    ///
    /// This memory's bit in a `u64` mask, for precompiled behaviour preconditions.
    pub const fn bit(self) -> u64 {
        1_u64 << (self as usize)
    }
}

/// `MemoryStatus`。
///
/// `MemoryStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryStatus {
    ValuePresent,
    ValueAbsent,
    Registered,
}

impl MemoryStatus {
    /// 官方枚举名的蛇形写法，对拍时两边一致。
    ///
    /// The snake_case form of the vanilla enum name; both sides print the same.
    pub const fn name(self) -> &'static str {
        match self {
            Self::ValuePresent => "value_present",
            Self::ValueAbsent => "value_absent",
            Self::Registered => "registered",
        }
    }
}

/// `PositionTracker`：一个方块位置，或者跟着一个实体（玩家或生物）。
///
/// `PositionTracker`: a block position, or one following an entity (player or mob).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PositionTracker {
    /// `BlockPosTracker`：方块坐标和它的中心（或者构造时给的精确点）。
    ///
    /// `BlockPosTracker`: the block and its centre (or the exact point it was built from).
    Block { pos: BlockPos, center: Vec3 },
    /// `EntityTracker(entity, trackEyeHeight, targetEyeHeight)`。
    ///
    /// `EntityTracker(entity, trackEyeHeight, targetEyeHeight)`.
    Entity {
        uid: EntityUid,
        track_eye: bool,
        target_eye: bool,
    },
    /// 跟着 `ItemEntity` 的 `EntityTracker`：官方的 tracker 直接持有实体对象，掉落物被
    /// 捡走、行离开实体表之后仍能报出最后的位置、`isVisibleBy` 也恒为真。这里存下
    /// 写入记忆时的坐标当作离表后的快照；实体还在时用表里的实时坐标。
    ///
    /// An `EntityTracker` following an `ItemEntity`: vanilla holds the entity object,
    /// so a picked-up item still reports its last position and is always visible.
    /// The stored position is the fallback once its row leaves the table; while it
    /// remains, the live table position is used.
    Item {
        uid: EntityUid,
        pos: Vec3,
        track_eye: bool,
        target_eye: bool,
    },
}

impl PositionTracker {
    /// `new BlockPosTracker(Vec3)`：方块取包含它的那一格，中心就是这个点。
    ///
    /// `new BlockPosTracker(Vec3)`: the containing block, centred on the point itself.
    pub fn at_point(point: Vec3) -> Self {
        Self::Block {
            pos: BlockPos::containing(point),
            center: point,
        }
    }

    /// `new BlockPosTracker(BlockPos)`：中心是 `Vec3.atCenterOf`。
    ///
    /// `new BlockPosTracker(BlockPos)`: centred with `Vec3.atCenterOf`.
    pub fn at_block(pos: BlockPos) -> Self {
        Self::Block {
            pos,
            center: Vec3::new(
                f64::from(pos.x) + 0.5,
                f64::from(pos.y) + 0.5,
                f64::from(pos.z) + 0.5,
            ),
        }
    }
}

/// `WalkTarget`。
///
/// `WalkTarget`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WalkTarget {
    pub target: PositionTracker,
    pub speed: f32,
    pub close_enough: i32,
}

impl WalkTarget {
    /// `new WalkTarget(Vec3, speed, closeEnough)`：落到包含这个点的方块上。
    ///
    /// `new WalkTarget(Vec3, speed, closeEnough)`: snapped to the containing block.
    pub fn to_point(point: Vec3, speed: f32, close_enough: i32) -> Self {
        Self {
            target: PositionTracker::at_block(BlockPos::containing(point)),
            speed,
            close_enough,
        }
    }
}

/// `NearestVisibleLivingEntities`：附近的活体按距离排好，视线判定第一次问到时才算，
/// 算过的记下来。实体表用共享切片，`NearestLivingEntities` 和这条记忆共用同一份数据。
/// 每项另存一份实体种类，供村民传感器按类型预筛，不用逐项再回查实体表。
///
/// `NearestVisibleLivingEntities`: nearby living entities by distance; the visibility
/// test runs lazily the first time each one is asked about and is then cached. The uid
/// list is a shared slice so `NearestLivingEntities` and this memory share one
/// allocation. Each entry also carries its mob type so villager sensors can prefilter
/// by type without looking the entity up again.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NearestVisible {
    pub entities: Arc<[EntityUid]>,
    /// 和 `entities` 对齐的实体种类；玩家 / 非生物为 `None`。
    ///
    /// Mob type aligned with `entities`; `None` for players and non-mob entities.
    pub mob_types: Vec<Option<MobType>>,
    /// 列表里有没有敌对类型。`VillagerHostilesSensor` 没有敌对类型时可直接擦记忆，
    /// 不必逐项测距离。当前用 `MobType::is_enemy` 作保守超集：官方精确集合
    /// `ACCEPTABLE_DISTANCE_FROM_HOSTILES` 的多数灾厄 / 僵尸类生物期 3 才建模，
    /// 超集只会少做早退、不会误擦；精确的判定走 [`Self::has_type`]。
    ///
    /// Whether the list contains a hostile type. `VillagerHostilesSensor` can erase
    /// its memory at once when there is none, without a per-entry distance test.
    /// Currently a conservative superset via `MobType::is_enemy`: most of vanilla's
    /// exact `ACCEPTABLE_DISTANCE_FROM_HOSTILES` set is modelled only in phase 3, and
    /// a superset only skips the early-out (never wrongly erases). Precise checks
    /// go through [`Self::has_type`].
    pub has_hostile_type: bool,
    /// 和 `entities` 对齐的视线缓存，`None` 是还没算过。
    ///
    /// A visibility cache aligned with `entities`; `None` means not tested yet.
    pub visible: Vec<Option<bool>>,
}

impl NearestVisible {
    /// `mob_types` 与 `entities` 等长；玩家 / 非生物用 `None`。
    ///
    /// `mob_types` is aligned with `entities`; `None` for players / non-mob entities.
    pub fn new(entities: Arc<[EntityUid]>, mob_types: Vec<Option<MobType>>) -> Self {
        let len = entities.len();
        let has_hostile_type = mob_types.iter().flatten().any(|ty| ty.is_enemy());
        let visible = vec![None; len];
        Self {
            entities,
            mob_types,
            has_hostile_type,
            visible,
        }
    }

    /// 按实体类型预筛，保持列表顺序，返回匹配项的 (下标, uid)。未来的
    /// `VillagerBabiesSensor` 先用它筛出村民，再逐项读实时幼年状态。
    ///
    /// Prefilters by mob type, keeping list order, returning `(index, uid)` for each
    /// match. A future `VillagerBabiesSensor` uses it to pick villagers before reading
    /// each one's live baby status.
    pub fn prefilter(&self, accept: impl Fn(Option<MobType>) -> bool) -> Vec<(usize, EntityUid)> {
        self.entities
            .iter()
            .copied()
            .enumerate()
            .filter(|(index, _)| accept(self.mob_types.get(*index).copied().flatten()))
            .collect()
    }

    /// 列表里有没有满足类型条件的实体（[`Self::has_hostile_type`] 的通用形式）。
    ///
    /// Whether any entry satisfies the type predicate (the general form of
    /// [`Self::has_hostile_type`]).
    pub fn has_type(&self, accept: impl Fn(Option<MobType>) -> bool) -> bool {
        self.mob_types.iter().any(|ty| accept(*ty))
    }
}

/// 一个记忆的值。官方每种记忆有自己的 Java 类型，这里按形状合成一个枚举。
///
/// A memory value. Vanilla gives each memory its own Java type; here they share one
/// enum by shape.
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryValue {
    Unit,
    Bool(bool),
    Int(i32),
    Long(i64),
    Vec3(Vec3),
    Entity(EntityUid),
    Entities(Arc<[EntityUid]>),
    Visible(NearestVisible),
    Walk(WalkTarget),
    Look(PositionTracker),
    Damage(DamageSource),
    /// `GlobalPos`（`home` / `job_site` / `potential_job_site` / `meeting_point` / `hiding_place`）。
    GlobalPos(GlobalPos),
    /// `List<GlobalPos>`（`secondary_job_site`）。
    GlobalPosList(Vec<GlobalPos>),
    /// `BlockPos`（`nearest_bed`）。
    BlockPos(BlockPos),
    /// `NEAREST_VISIBLE_WANTED_ITEM`：官方记忆里直接持有 `ItemEntity` 对象，实体被捡走
    /// 之后 `GoToWantedItem` 仍能用它最后的位置继续判断。这里把 uid 和当时的坐标一起
    /// 存下来；对拍打印时仍只输出 uid。
    ///
    /// `NEAREST_VISIBLE_WANTED_ITEM`: vanilla stores the `ItemEntity` itself, so
    /// `GoToWantedItem` can still use its last position after the item is picked up.
    /// Keep the uid plus its position; the oracle dump still prints just the uid.
    WantedItem {
        uid: EntityUid,
        pos: Vec3,
    },
}

impl MemoryValue {
    /// `Brain.isEmptyCollection`：空列表当作没有值。
    ///
    /// `Brain.isEmptyCollection`: an empty list counts as no value.
    fn is_empty_collection(&self) -> bool {
        matches!(self, Self::Entities(list) if list.is_empty())
            || matches!(self, Self::GlobalPosList(list) if list.is_empty())
    }
}

/// `MemorySlot`。值用 `Box` 装着：官方 `MemorySlot` 里值是对象引用，这里
/// `MemoryValue` 有 184 字节，装进槽里会让 32 个槽白白占 6400 字节。装箱后
/// 空槽只有一个空指针，大对象只在真正写入记忆时才分配。
///
/// `MemorySlot`. The value is boxed: vanilla holds an object reference, while
/// `MemoryValue` is 184 bytes and embedding it would waste 6400 bytes on 32 slots.
/// An empty slot is now just a null pointer; the big object is allocated only when a
/// memory is actually set.
#[derive(Debug, Clone, PartialEq)]
struct Slot {
    value: Option<Box<MemoryValue>>,
    ttl: i64,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            value: None,
            ttl: NEVER_EXPIRE,
        }
    }
}

/// 一个记忆种类在位掩码里的位。`MemoryType::COUNT` 是 32，正好落进 `u64`。
///
/// The bit of a memory type in the masks. `MemoryType::COUNT` is 32, fitting a `u64`.
const fn bit(kind: MemoryType) -> u64 {
    kind.bit()
}

/// 记忆状态的三组位掩码，给行为入口条件做纯位运算判定。
///
/// The three bit masks of memory state, for bit-only behaviour precondition checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryMasks {
    /// 有值的记忆。
    pub present: u64,
    /// 登记过但没有值的记忆。
    pub absent: u64,
    /// 登记过的记忆。
    pub registered: u64,
}

/// 一只生物的全部记忆槽。
///
/// One mob's memory slots.
#[derive(Debug, Clone, PartialEq)]
pub struct Memories {
    slots: [Slot; MemoryType::COUNT],
    /// 登记过的记忆位。
    registered: u64,
    /// 有值的记忆位（`registered` 的子集）。
    present: u64,
    /// 有值且会过期（`ttl != NEVER_EXPIRE`）的位，`tick` 只遍历这些。
    expiring: u64,
}

impl Default for Memories {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| Slot::default()),
            registered: 0,
            present: 0,
            expiring: 0,
        }
    }
}

impl Memories {
    /// `Brain.registerMemory`。
    ///
    /// `Brain.registerMemory`.
    pub fn register(&mut self, kind: MemoryType) {
        self.registered |= bit(kind);
    }

    /// 一次登记整张模板算好的记忆集合（构造大脑时省掉逐条 `register`）。
    ///
    /// Registers a whole precomputed memory set at once (avoids per-type `register`
    /// calls when building a brain).
    pub fn register_mask(&mut self, mask: u64) {
        self.registered |= mask;
    }

    /// 已登记记忆的位掩码（模板构造时读出来，供实例复用）。
    ///
    /// The registered-memory bit mask (read when building a template, reused by instances).
    pub fn registered_mask(&self) -> u64 {
        self.registered
    }

    pub fn is_registered(&self, kind: MemoryType) -> bool {
        self.registered & bit(kind) != 0
    }

    /// `Brain.checkMemory`：用位掩码判定，不碰槽里的值。
    ///
    /// `Brain.checkMemory`: a bit-mask test that never touches the slot values.
    pub fn check(&self, kind: MemoryType, status: MemoryStatus) -> bool {
        let b = bit(kind);
        if self.registered & b == 0 {
            return false;
        }
        match status {
            MemoryStatus::Registered => true,
            MemoryStatus::ValuePresent => self.present & b != 0,
            MemoryStatus::ValueAbsent => self.present & b == 0,
        }
    }

    /// 给行为入口条件用的三组掩码。
    ///
    /// The three masks a behaviour checks its entry conditions against.
    pub fn masks(&self) -> MemoryMasks {
        MemoryMasks {
            present: self.present,
            absent: self.registered & !self.present,
            registered: self.registered,
        }
    }

    /// `Brain.forgetOutdatedMemories` → `MemorySlot.tick`：只遍历「有值且会过期」的位，
    /// 不再每刻扫全部 32 个槽。
    ///
    /// `Brain.forgetOutdatedMemories` → `MemorySlot.tick`: iterate only the filled slots
    /// with a finite expiry, instead of scanning all 32 slots every tick.
    pub fn tick(&mut self) {
        self.tick_by(1);
    }

    /// 一次按 `gap` 刻推进记忆过期（降频后大脑跳刻用，`gap <= 1` 等价于单刻 [`Self::tick`]）。
    ///
    /// 逐刻语义下 `ttl == 0` 在第 1 刻清空、`ttl == k` 在第 `k + 1` 刻清空，
    /// 所以连续跑 `step` 刻等价于：`ttl < step` 清空，否则 `ttl -= step`。
    ///
    /// Advances memory expiry by `gap` ticks at once (for throttled brain ticks;
    /// `gap <= 1` matches single-tick [`Self::tick`]). Under per-tick rules a slot with
    /// `ttl == k` clears on tick `k + 1`, so `step` ticks clear iff `ttl < step`,
    /// otherwise subtract `step`.
    pub fn tick_by(&mut self, gap: i64) {
        let step = gap.max(1);
        let mut bits = self.expiring;
        while bits != 0 {
            let index = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            let b = 1_u64 << index;
            let slot = &mut self.slots[index];
            let cleared = if slot.ttl < step {
                slot.value = None;
                slot.ttl = NEVER_EXPIRE;
                true
            } else {
                slot.ttl -= step;
                false
            };
            if cleared {
                self.present &= !b;
                self.expiring &= !b;
            }
        }
    }

    /// `Brain.hasMemoryValue`。
    ///
    /// `Brain.hasMemoryValue`.
    pub fn has(&self, kind: MemoryType) -> bool {
        self.check(kind, MemoryStatus::ValuePresent)
    }

    /// `Brain.getMemory`：没有登记的种类官方会抛异常，这里读作没有。
    ///
    /// `Brain.getMemory`; vanilla throws for unregistered types, here they read as empty.
    pub fn get(&self, kind: MemoryType) -> Option<&MemoryValue> {
        if self.registered & bit(kind) != 0 {
            self.slots[kind.index()].value.as_deref()
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, kind: MemoryType) -> Option<&mut MemoryValue> {
        if self.registered & bit(kind) != 0 {
            self.slots[kind.index()].value.as_deref_mut()
        } else {
            None
        }
    }

    /// `Brain.setMemory(type, value)`：不过期；空列表等于清除。
    ///
    /// `Brain.setMemory(type, value)`: no expiry; an empty list clears the slot.
    pub fn set(&mut self, kind: MemoryType, value: MemoryValue) {
        self.set_with_ttl(kind, value, NEVER_EXPIRE);
    }

    /// `Brain.setMemory(type, Optional)`。
    ///
    /// `Brain.setMemory(type, Optional)`.
    pub fn set_or_erase(&mut self, kind: MemoryType, value: Option<MemoryValue>) {
        match value {
            Some(value) => self.set(kind, value),
            None => self.erase(kind),
        }
    }

    /// `Brain.setMemoryWithExpiry`。
    ///
    /// `Brain.setMemoryWithExpiry`.
    pub fn set_with_ttl(&mut self, kind: MemoryType, value: MemoryValue, ttl: i64) {
        let b = bit(kind);
        if self.registered & b == 0 {
            return;
        }
        let empty = value.is_empty_collection();
        {
            let slot = &mut self.slots[kind.index()];
            if empty {
                slot.value = None;
                slot.ttl = NEVER_EXPIRE;
            } else {
                slot.value = Some(Box::new(value));
                slot.ttl = ttl;
            }
        }
        if empty {
            self.present &= !b;
            self.expiring &= !b;
        } else {
            self.present |= b;
            if ttl == NEVER_EXPIRE {
                self.expiring &= !b;
            } else {
                self.expiring |= b;
            }
        }
    }

    /// `Brain.eraseMemory`。
    ///
    /// `Brain.eraseMemory`.
    pub fn erase(&mut self, kind: MemoryType) {
        let b = bit(kind);
        if self.registered & b == 0 {
            return;
        }
        let slot = &mut self.slots[kind.index()];
        slot.value = None;
        slot.ttl = NEVER_EXPIRE;
        self.present &= !b;
        self.expiring &= !b;
    }

    /// `Brain.clearMemories`：全部槽清空，登记保留。
    ///
    /// `Brain.clearMemories`: every slot emptied, registrations kept.
    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            slot.value = None;
            slot.ttl = NEVER_EXPIRE;
        }
        self.present = 0;
        self.expiring = 0;
    }

    /// `Brain.getTimeUntilExpiry`：空槽也返回 `Long.MAX_VALUE`。
    ///
    /// `Brain.getTimeUntilExpiry`; empty slots report `Long.MAX_VALUE` as well.
    pub fn time_until_expiry(&self, kind: MemoryType) -> i64 {
        let slot = &self.slots[kind.index()];
        if slot.value.is_some() {
            slot.ttl
        } else {
            NEVER_EXPIRE
        }
    }

    /// 有值的槽：（种类，值，过期时间；不过期时为 `None`）。存档和对拍用。
    ///
    /// The filled slots as (type, value, time to live or `None`), for saving and oracles.
    pub fn filled(&self) -> impl Iterator<Item = (MemoryType, &MemoryValue, Option<i64>)> + '_ {
        let mut bits = self.present;
        std::iter::from_fn(move || {
            while bits != 0 {
                let index = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let slot = &self.slots[index];
                if let Some(value) = slot.value.as_deref() {
                    return Some((
                        MemoryType::ALL[index],
                        value,
                        (slot.ttl != NEVER_EXPIRE).then_some(slot.ttl),
                    ));
                }
            }
            None
        })
    }

    pub fn int(&self, kind: MemoryType) -> Option<i32> {
        match self.get(kind) {
            Some(MemoryValue::Int(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn long(&self, kind: MemoryType) -> Option<i64> {
        match self.get(kind) {
            Some(MemoryValue::Long(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn bool(&self, kind: MemoryType) -> Option<bool> {
        match self.get(kind) {
            Some(MemoryValue::Bool(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn global_pos(&self, kind: MemoryType) -> Option<&GlobalPos> {
        match self.get(kind) {
            Some(MemoryValue::GlobalPos(pos)) => Some(pos),
            _ => None,
        }
    }

    /// `List<GlobalPos>`（`sniffer_explored_positions`）。
    pub fn global_pos_list(&self, kind: MemoryType) -> Option<&[GlobalPos]> {
        match self.get(kind) {
            Some(MemoryValue::GlobalPosList(list)) => Some(list.as_slice()),
            _ => None,
        }
    }

    pub fn block_pos(&self, kind: MemoryType) -> Option<BlockPos> {
        match self.get(kind) {
            Some(MemoryValue::BlockPos(pos)) => Some(*pos),
            _ => None,
        }
    }

    pub fn entity(&self, kind: MemoryType) -> Option<EntityUid> {
        match self.get(kind) {
            Some(MemoryValue::Entity(uid)) => Some(*uid),
            Some(MemoryValue::WantedItem { uid, .. }) => Some(*uid),
            _ => None,
        }
    }

    /// `NEAREST_VISIBLE_WANTED_ITEM` 持有的 `ItemEntity` 快照：表里的实时坐标优先，
    /// 实体已经离表时用记忆里的最后坐标。
    ///
    /// The `ItemEntity` held by `NEAREST_VISIBLE_WANTED_ITEM`: prefer the live table
    /// position, falling back to the stored last position once it leaves the table.
    pub fn wanted_item(&self) -> Option<(EntityUid, Vec3)> {
        match self.get(MemoryType::NearestVisibleWantedItem) {
            Some(MemoryValue::WantedItem { uid, pos }) => Some((*uid, *pos)),
            Some(MemoryValue::Entity(uid)) => Some((*uid, Vec3::default())),
            _ => None,
        }
    }

    pub fn walk_target(&self) -> Option<WalkTarget> {
        match self.get(MemoryType::WalkTarget) {
            Some(MemoryValue::Walk(target)) => Some(*target),
            _ => None,
        }
    }

    pub fn look_target(&self) -> Option<PositionTracker> {
        match self.get(MemoryType::LookTarget) {
            Some(MemoryValue::Look(target)) => Some(*target),
            _ => None,
        }
    }

    pub fn entities(&self, kind: MemoryType) -> Option<&[EntityUid]> {
        match self.get(kind) {
            Some(MemoryValue::Entities(list)) => Some(list.as_ref()),
            _ => None,
        }
    }

    pub fn damage(&self, kind: MemoryType) -> Option<DamageSource> {
        match self.get(kind) {
            Some(MemoryValue::Damage(source)) => Some(*source),
            _ => None,
        }
    }

    pub fn vec3(&self, kind: MemoryType) -> Option<Vec3> {
        match self.get(kind) {
            Some(MemoryValue::Vec3(point)) => Some(*point),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 有过期时间的记忆照 `MemorySlot.tick`：先减到 0，再过一刻才清掉，清掉后回到永不过期。
    ///
    /// Expiring memories follow `MemorySlot.tick`: count down to 0, clear one tick later,
    /// then report never-expire again.
    #[test]
    fn expiring_memory_counts_down_then_clears() {
        let mut memories = Memories::default();
        memories.register(MemoryType::DangerDetectedRecently);
        memories.set_with_ttl(
            MemoryType::DangerDetectedRecently,
            MemoryValue::Bool(true),
            2,
        );
        memories.tick();
        assert_eq!(
            memories.time_until_expiry(MemoryType::DangerDetectedRecently),
            1
        );
        memories.tick();
        assert_eq!(
            memories.time_until_expiry(MemoryType::DangerDetectedRecently),
            0
        );
        assert!(memories.has(MemoryType::DangerDetectedRecently));
        memories.tick();
        assert!(!memories.has(MemoryType::DangerDetectedRecently));
        assert_eq!(
            memories.time_until_expiry(MemoryType::DangerDetectedRecently),
            i64::MAX
        );
    }

    /// 没登记的记忆读不到、写不进；空列表等于清除。
    ///
    /// Unregistered memories ignore writes; an empty list clears.
    #[test]
    fn unregistered_and_empty_lists_read_as_absent() {
        let mut memories = Memories::default();
        memories.set(MemoryType::WalkTarget, MemoryValue::Unit);
        assert!(!memories.check(MemoryType::WalkTarget, MemoryStatus::ValueAbsent));
        memories.register(MemoryType::NearestLivingEntities);
        memories.set(
            MemoryType::NearestLivingEntities,
            MemoryValue::Entities(Arc::from([3])),
        );
        assert!(memories.has(MemoryType::NearestLivingEntities));
        memories.set(
            MemoryType::NearestLivingEntities,
            MemoryValue::Entities(Arc::default()),
        );
        assert!(!memories.has(MemoryType::NearestLivingEntities));
    }

    /// 青蛙新增的记忆追加在枚举末尾，既有种类下标不变；`is_in_water` / `is_pregnant`
    /// 带 `Unit.CODEC` 进存档，另三个不进存档（官方 `MemoryModuleType.canSerialize`）。
    ///
    /// The frog memories append at the end so existing indices stay put; `is_in_water` /
    /// `is_pregnant` carry a `Unit.CODEC` and are saved, the other three are not (vanilla
    /// `MemoryModuleType.canSerialize`).
    #[test]
    fn frog_memories_append_and_serialize_like_vanilla() {
        assert_eq!(MemoryType::ALL.len(), MemoryType::COUNT);
        assert_eq!(MemoryType::IsInWater.name(), "is_in_water");
        assert_eq!(MemoryType::IsPregnant.name(), "is_pregnant");
        assert_eq!(MemoryType::NearestAttackable.name(), "nearest_attackable");
        assert_eq!(MemoryType::AttackTarget.name(), "attack_target");
        assert_eq!(
            MemoryType::UnreachableTongueTargets.name(),
            "unreachable_tongue_targets"
        );
        assert!(MemoryType::IsInWater.serializable());
        assert!(MemoryType::IsPregnant.serializable());
        assert!(!MemoryType::NearestAttackable.serializable());
        assert!(!MemoryType::AttackTarget.serializable());
        assert!(!MemoryType::UnreachableTongueTargets.serializable());
        assert_eq!(
            MemoryType::by_name("minecraft:attack_target"),
            Some(MemoryType::AttackTarget)
        );
        assert_eq!(
            MemoryType::by_name("minecraft:unreachable_tongue_targets"),
            Some(MemoryType::UnreachableTongueTargets)
        );
    }

    /// 可见列表按类型预筛、保持顺序；敌对类型位是保守超集。
    ///
    /// The visible list prefilters by type in order; the hostile bit is a conservative
    /// superset.
    #[test]
    fn nearest_visible_prefilters_by_type_and_tracks_hostiles() {
        let entities: Arc<[EntityUid]> = Arc::from([10, 11, 12]);
        let mob_types = vec![None, Some(MobType::Cow), Some(MobType::Zombie)];
        let visible = NearestVisible::new(entities, mob_types);
        assert!(visible.has_hostile_type);
        assert_eq!(
            visible.prefilter(|ty| ty == Some(MobType::Cow)),
            vec![(1, 11)]
        );
        assert_eq!(
            visible.prefilter(|ty| ty.is_some_and(|m| m.is_enemy())),
            vec![(2, 12)]
        );
        assert!(
            visible
                .prefilter(|ty| ty == Some(MobType::Villager))
                .is_empty()
        );
        assert!(visible.has_type(|ty| ty == Some(MobType::Zombie)));
        assert!(!visible.has_type(|ty| ty == Some(MobType::Villager)));

        // 只有玩家和非敌对生物时，敌对类型位为假。
        // Only players and non-hostile mobs: the hostile bit is false.
        let benign = NearestVisible::new(Arc::from([20, 21]), vec![None, Some(MobType::Cow)]);
        assert!(!benign.has_hostile_type);
    }
}
