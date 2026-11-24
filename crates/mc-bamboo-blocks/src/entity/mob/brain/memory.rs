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

use super::super::DamageSource;
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
}

impl MemoryType {
    /// 记忆种类的个数，槽表的长度。
    ///
    /// The number of memory types: the length of the slot table.
    pub const COUNT: usize = 32;

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
        )
    }

    const fn index(self) -> usize {
        self as usize
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
///
/// `NearestVisibleLivingEntities`: nearby living entities by distance; the visibility
/// test runs lazily the first time each one is asked about and is then cached. The uid
/// list is a shared slice so `NearestLivingEntities` and this memory share one allocation.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NearestVisible {
    pub entities: Arc<[EntityUid]>,
    /// 和 `entities` 对齐的缓存，`None` 是还没算过。
    ///
    /// A cache aligned with `entities`; `None` means not tested yet.
    pub visible: Vec<Option<bool>>,
}

impl NearestVisible {
    pub fn new(entities: Arc<[EntityUid]>) -> Self {
        let visible = vec![None; entities.len()];
        Self { entities, visible }
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
}

impl MemoryValue {
    /// `Brain.isEmptyCollection`：空列表当作没有值。
    ///
    /// `Brain.isEmptyCollection`: an empty list counts as no value.
    fn is_empty_collection(&self) -> bool {
        matches!(self, Self::Entities(list) if list.is_empty())
    }
}

/// `MemorySlot`。
///
/// `MemorySlot`.
#[derive(Debug, Clone, PartialEq)]
struct Slot {
    registered: bool,
    value: Option<MemoryValue>,
    ttl: i64,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            registered: false,
            value: None,
            ttl: NEVER_EXPIRE,
        }
    }
}

/// 一只生物的全部记忆槽。
///
/// One mob's memory slots.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Memories {
    slots: [Slot; MemoryType::COUNT],
}

impl Memories {
    /// `Brain.registerMemory`。
    ///
    /// `Brain.registerMemory`.
    pub fn register(&mut self, kind: MemoryType) {
        self.slots[kind.index()].registered = true;
    }

    pub fn is_registered(&self, kind: MemoryType) -> bool {
        self.slots[kind.index()].registered
    }

    /// `Brain.forgetOutdatedMemories` → `MemorySlot.tick`。
    ///
    /// `Brain.forgetOutdatedMemories` → `MemorySlot.tick`.
    pub fn tick(&mut self) {
        for slot in &mut self.slots {
            if slot.value.is_some() && slot.ttl != NEVER_EXPIRE {
                if slot.ttl <= 0 {
                    slot.value = None;
                    slot.ttl = NEVER_EXPIRE;
                } else {
                    slot.ttl -= 1;
                }
            }
        }
    }

    /// `Brain.checkMemory`。
    ///
    /// `Brain.checkMemory`.
    pub fn check(&self, kind: MemoryType, status: MemoryStatus) -> bool {
        let slot = &self.slots[kind.index()];
        slot.registered
            && match status {
                MemoryStatus::Registered => true,
                MemoryStatus::ValuePresent => slot.value.is_some(),
                MemoryStatus::ValueAbsent => slot.value.is_none(),
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
        let slot = &self.slots[kind.index()];
        if slot.registered {
            slot.value.as_ref()
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, kind: MemoryType) -> Option<&mut MemoryValue> {
        let slot = &mut self.slots[kind.index()];
        if slot.registered {
            slot.value.as_mut()
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
        let slot = &mut self.slots[kind.index()];
        if !slot.registered {
            return;
        }
        if value.is_empty_collection() {
            slot.value = None;
            slot.ttl = NEVER_EXPIRE;
        } else {
            slot.value = Some(value);
            slot.ttl = ttl;
        }
    }

    /// `Brain.eraseMemory`。
    ///
    /// `Brain.eraseMemory`.
    pub fn erase(&mut self, kind: MemoryType) {
        let slot = &mut self.slots[kind.index()];
        if slot.registered {
            slot.value = None;
            slot.ttl = NEVER_EXPIRE;
        }
    }

    /// `Brain.clearMemories`：全部槽清空，登记保留。
    ///
    /// `Brain.clearMemories`: every slot emptied, registrations kept.
    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            slot.value = None;
            slot.ttl = NEVER_EXPIRE;
        }
    }

    /// `Brain.getTimeUntilExpiry`：空槽也返回 `Long.MAX_VALUE`。
    ///
    /// `Brain.getTimeUntilExpiry`; empty slots report `Long.MAX_VALUE` as well.
    pub fn time_until_expiry(&self, kind: MemoryType) -> i64 {
        self.slots[kind.index()].ttl
    }

    /// 有值的槽：（种类，值，过期时间；不过期时为 `None`）。存档和对拍用。
    ///
    /// The filled slots as (type, value, time to live or `None`), for saving and oracles.
    pub fn filled(&self) -> impl Iterator<Item = (MemoryType, &MemoryValue, Option<i64>)> {
        MemoryType::ALL.into_iter().filter_map(|kind| {
            let slot = &self.slots[kind.index()];
            let value = slot.value.as_ref()?;
            Some((kind, value, (slot.ttl != NEVER_EXPIRE).then_some(slot.ttl)))
        })
    }

    pub fn int(&self, kind: MemoryType) -> Option<i32> {
        match self.get(kind) {
            Some(MemoryValue::Int(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn entity(&self, kind: MemoryType) -> Option<EntityUid> {
        match self.get(kind) {
            Some(MemoryValue::Entity(uid)) => Some(*uid),
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
}
