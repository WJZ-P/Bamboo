//! 盔甲架：官方 `ArmorStand`，`LivingEntity` 一支、没有 AI。
//!
//! 状态收在 [`ArmorStand`] 里，别的生物不读。尺寸有普通 / 小型 / 标记三种，
//! 同步数据是 `DATA_CLIENT_FLAGS` 一个字节加 6 个姿势旋转。存档键见
//! [`write_nbt`] / [`read_nbt`]，受伤的两段式摇晃和破坏见
//! [`super::living::hurt_server`] 的盔甲架分支。
//!
//! The armour stand, vanilla `ArmorStand`: a `LivingEntity` with no AI. Its
//! state lives in [`ArmorStand`]. There are normal, small and marker sizes;
//! the synced data is one `DATA_CLIENT_FLAGS` byte plus six pose rotations.
//! The two-hit wobble and breaking logic lives in the armour-stand branch of
//! [`super::living::hurt_server`].

use mc_bamboo_core::Vec3;
use mc_bamboo_data::{items, tags};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use crate::{
    InteractionResult,
    archive::tag::{self, put},
    entity::{
        Entity,
        ctx::{Command, EntityCtx},
    },
    game_event::{GameEvent, dispatch::EventContext},
};

use super::{
    DamageKind, DamageSource, Mob, MobEvent,
    interact::{MobInteractRequest, MobInteraction},
    living,
};

/// 一个姿势绕三根轴的旋转（度），官方 `net.minecraft.core.Rotations`。
///
/// A pose's rotation around the three axes in degrees, vanilla `Rotations`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotations {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Rotations {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// `Rotations.CODEC`：`[x, y, z]` 三个浮点的列表。
    ///
    /// `Rotations.CODEC`: a list of three floats `[x, y, z]`.
    pub fn to_tag(self) -> Tag {
        Tag::List(vec![
            Tag::Float(self.x),
            Tag::Float(self.y),
            Tag::Float(self.z),
        ])
    }

    /// 读 `Rotations.CODEC`；元素不是浮点或数量不对时按缺省值处理。
    ///
    /// Reads `Rotations.CODEC`; a malformed list falls back to the default.
    pub fn from_tag(tag: &Tag, default: Self) -> Self {
        let Tag::List(values) = tag else {
            return default;
        };
        let mut floats = [default.x, default.y, default.z];
        for (slot, value) in values.iter().take(3).enumerate() {
            if let Tag::Float(value) = value {
                floats[slot] = *value;
            }
        }
        Self::new(floats[0], floats[1], floats[2])
    }
}

/// `DATA_CLIENT_FLAGS` 的位。
pub const CLIENT_FLAG_SMALL: u8 = 1;
pub const CLIENT_FLAG_SHOW_ARMS: u8 = 4;
pub const CLIENT_FLAG_NO_BASEPLATE: u8 = 8;
pub const CLIENT_FLAG_MARKER: u8 = 16;

/// `DisabledSlots` 里「取出」和「放入」两段的位偏移。
pub const DISABLE_TAKING_OFFSET: i32 = 8;
pub const DISABLE_PUTTING_OFFSET: i32 = 16;

pub const DEFAULT_HEAD_POSE: Rotations = Rotations::new(0.0, 0.0, 0.0);
pub const DEFAULT_BODY_POSE: Rotations = Rotations::new(0.0, 0.0, 0.0);
pub const DEFAULT_LEFT_ARM_POSE: Rotations = Rotations::new(-10.0, 0.0, -10.0);
pub const DEFAULT_RIGHT_ARM_POSE: Rotations = Rotations::new(-15.0, 0.0, 10.0);
pub const DEFAULT_LEFT_LEG_POSE: Rotations = Rotations::new(-1.0, 0.0, -1.0);
pub const DEFAULT_RIGHT_LEG_POSE: Rotations = Rotations::new(1.0, 0.0, 1.0);

/// 普通 / 小型 / 标记三种尺寸和眼高。
pub const NORMAL_DIMENSIONS: (f32, f32, f32) = (0.5, 1.975, 1.7775);
/// `BABY_DIMENSIONS`：`dims.scale(0.5).withEyeHeight(0.9875)`。
pub const BABY_DIMENSIONS: (f32, f32, f32) = (0.25, 0.9875, 0.9875);
/// `MARKER_DIMENSIONS`：`EntityDimensions.fixed(0, 0)`。
pub const MARKER_DIMENSIONS: (f32, f32, f32) = (0.0, 0.0, 0.0);

/// 盔甲架的专属状态。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArmorStand {
    /// `DATA_CLIENT_FLAGS`：小型、手臂、无底座、标记。
    pub client_flags: u8,
    pub head_pose: Rotations,
    pub body_pose: Rotations,
    pub left_arm_pose: Rotations,
    pub right_arm_pose: Rotations,
    pub left_leg_pose: Rotations,
    pub right_leg_pose: Rotations,
    /// 官方 `ArmorStand.invisible` 私有字段：`setInvisible` 同时把它和 `Entity` 的
    /// 共享标志隐身位（index 0 bit 5）设成同一个值，`Invisible` 存档键读写它。
    /// Bamboo 以它为唯一状态源，同步时推导共享标志位。
    ///
    /// Vanilla `ArmorStand.invisible` field: `setInvisible` sets it and the shared
    /// invisible flag (index 0 bit 5) together, and the `Invisible` save key reads
    /// and writes it. Bamboo keeps it as the single source and derives the flag.
    pub invisible: bool,
    /// `disabledSlots`。
    pub disabled_slots: i32,
    /// `lastHit`：上次被敲的岛时钟绝对时刻。
    pub last_hit: i64,
}

impl Default for ArmorStand {
    fn default() -> Self {
        Self {
            client_flags: 0,
            head_pose: DEFAULT_HEAD_POSE,
            body_pose: DEFAULT_BODY_POSE,
            left_arm_pose: DEFAULT_LEFT_ARM_POSE,
            right_arm_pose: DEFAULT_RIGHT_ARM_POSE,
            left_leg_pose: DEFAULT_LEFT_LEG_POSE,
            right_leg_pose: DEFAULT_RIGHT_LEG_POSE,
            invisible: false,
            disabled_slots: 0,
            last_hit: 0,
        }
    }
}

impl ArmorStand {
    pub const fn is_small(&self) -> bool {
        self.client_flags & CLIENT_FLAG_SMALL != 0
    }

    pub const fn show_arms(&self) -> bool {
        self.client_flags & CLIENT_FLAG_SHOW_ARMS != 0
    }

    pub const fn show_base_plate(&self) -> bool {
        self.client_flags & CLIENT_FLAG_NO_BASEPLATE == 0
    }

    pub const fn is_marker(&self) -> bool {
        self.client_flags & CLIENT_FLAG_MARKER != 0
    }

    /// `hasPhysics()`：不是标记、没有 `NoGravity`。
    pub const fn has_physics(&self, no_gravity: bool) -> bool {
        !self.is_marker() && !no_gravity
    }

    /// `getDimensionsMarker(isMarker)`：标记、小型、普通的三种尺寸。
    pub const fn dimensions(&self) -> (f32, f32, f32) {
        if self.is_marker() {
            MARKER_DIMENSIONS
        } else if self.is_small() {
            BABY_DIMENSIONS
        } else {
            NORMAL_DIMENSIONS
        }
    }

    /// `isDisabled(slot)`：`disabledSlots & (1 << slot.getFilterBit(0))`，手槽还要看手臂。
    pub fn is_disabled(&self, slot_id: i32, slot_is_hand: bool) -> bool {
        let bit = slot_id;
        self.disabled_slots & (1 << bit) != 0 || (slot_is_hand && !self.show_arms())
    }

    /// `setBit`。
    fn set_flag(&mut self, mask: u8, enabled: bool) {
        if enabled {
            self.client_flags |= mask;
        } else {
            self.client_flags &= !mask;
        }
    }

    pub fn set_small(&mut self, small: bool) {
        self.set_flag(CLIENT_FLAG_SMALL, small);
    }

    pub fn set_show_arms(&mut self, arms: bool) {
        self.set_flag(CLIENT_FLAG_SHOW_ARMS, arms);
    }

    pub fn set_no_base_plate(&mut self, no_base_plate: bool) {
        self.set_flag(CLIENT_FLAG_NO_BASEPLATE, no_base_plate);
    }

    pub fn set_marker(&mut self, marker: bool) {
        self.set_flag(CLIENT_FLAG_MARKER, marker);
    }

    /// `addAdditionalSaveData`：六个布尔、`DisabledSlots` 和 `Pose`。
    pub fn write_nbt(&self, mob: &Mob, out: &mut Vec<(String, Tag)>) {
        put(out, "Invisible", Tag::Byte(i8::from(self.invisible)));
        put(out, "Small", Tag::Byte(i8::from(self.is_small())));
        put(out, "ShowArms", Tag::Byte(i8::from(self.show_arms())));
        put(out, "DisabledSlots", Tag::Int(self.disabled_slots));
        put(
            out,
            "NoBasePlate",
            Tag::Byte(i8::from(!self.show_base_plate())),
        );
        if self.is_marker() {
            put(out, "Marker", Tag::Byte(1));
        }
        let pose = Tag::Compound(vec![
            ("Head".to_owned(), self.head_pose.to_tag()),
            ("Body".to_owned(), self.body_pose.to_tag()),
            ("LeftArm".to_owned(), self.left_arm_pose.to_tag()),
            ("RightArm".to_owned(), self.right_arm_pose.to_tag()),
            ("LeftLeg".to_owned(), self.left_leg_pose.to_tag()),
            ("RightLeg".to_owned(), self.right_leg_pose.to_tag()),
        ]);
        put(out, "Pose", pose);
        let _ = mob;
    }

    /// `readAdditionalSaveData`：六个布尔、`DisabledSlots` 和 `Pose`。
    pub fn read_nbt(&mut self, tag: &Tag) {
        self.invisible = tag::bool_or(tag, "Invisible", false);
        self.set_small(tag::bool_or(tag, "Small", false));
        self.set_show_arms(tag::bool_or(tag, "ShowArms", false));
        self.disabled_slots = tag::int_or(tag, "DisabledSlots", 0);
        self.set_no_base_plate(tag::bool_or(tag, "NoBasePlate", false));
        self.set_marker(tag::bool_or(tag, "Marker", false));
        if let Some(pose) = tag::field(tag, "Pose") {
            let entries = tag::entries(pose).unwrap_or_default();
            let read = |key: &str, default: Rotations| {
                entries
                    .iter()
                    .find(|(name, _)| name == key)
                    .map_or(default, |(_, value)| Rotations::from_tag(value, default))
            };
            self.head_pose = read("Head", DEFAULT_HEAD_POSE);
            self.body_pose = read("Body", DEFAULT_BODY_POSE);
            self.left_arm_pose = read("LeftArm", DEFAULT_LEFT_ARM_POSE);
            self.right_arm_pose = read("RightArm", DEFAULT_RIGHT_ARM_POSE);
            self.left_leg_pose = read("LeftLeg", DEFAULT_LEFT_LEG_POSE);
            self.right_leg_pose = read("RightLeg", DEFAULT_RIGHT_LEG_POSE);
        }
    }
}

/// 官方 `ArmorStand.interact(Player, InteractionHand, Vec3)`：按物品或点击高度选槽位，
/// 放上 / 取下 / 交换装备；`DisabledSlots`、标记和无手臂的手槽会挡住交互。
///
/// `ArmorStand.interact(Player, InteractionHand, Vec3)`: pick a slot from the item or
/// the click height, then put on / take off / swap equipment; `DisabledSlots`, the
/// marker flag and hand slots without arms block it.
pub fn interact(mob: &mut Mob, req: &MobInteractRequest) -> MobInteraction {
    // `isMarker() || stack.is(NAME_TAG)` → `LivingEntity.interact`，不做装备交互。
    // `isMarker() || stack.is(NAME_TAG)` → `LivingEntity.interact`, no equipment change.
    if mob.armor_stand.is_marker() || req.held.item == items::NAME_TAG {
        return MobInteraction::pass();
    }
    // `player.isSpectator()` → `SUCCESS`（不改装备、不消耗物品）。
    // `player.isSpectator()` → `SUCCESS` (no equipment change, no item use).
    if req.spectator {
        let mut out = MobInteraction::pass();
        out.result = InteractionResult::Success;
        return out;
    }
    let held = &req.held;
    let item_slot = super::equipment::equippable_slot(held);
    let empty_hand = held.is_empty();
    let target = if empty_hand {
        let clicked = clicked_slot(mob, req.hit_pos);
        // `isDisabled(clicked)` 为真时退回物品槽（空手时是主手）。
        // When `isDisabled(clicked)` is true vanilla falls back to the item slot.
        if mob
            .armor_stand
            .is_disabled(slot_bit(clicked), is_hand(clicked))
        {
            item_slot
        } else {
            clicked
        }
    } else {
        if mob
            .armor_stand
            .is_disabled(slot_bit(item_slot), is_hand(item_slot))
            || (is_hand(item_slot) && !mob.armor_stand.show_arms())
        {
            let mut out = MobInteraction::pass();
            out.result = InteractionResult::Fail;
            return out;
        }
        item_slot
    };
    // 空手时只有槽里有东西才交换（`hasItemInSlot`）。
    // With an empty hand vanilla only swaps when the slot holds something.
    if empty_hand && !has_item(mob, target) {
        return MobInteraction::pass();
    }
    let mut out = MobInteraction::pass();
    if swap_item(mob, target, held, req.creative, &mut out) {
        out.result = InteractionResult::SuccessServer;
        mob.needs_sync = true;
        return out;
    }
    MobInteraction::pass()
}

const fn slot_bit(slot: EquipmentSlot) -> i32 {
    slot as i32
}

const fn is_hand(slot: EquipmentSlot) -> bool {
    matches!(slot, EquipmentSlot::MainHand | EquipmentSlot::OffHand)
}

fn has_item(mob: &Mob, slot: EquipmentSlot) -> bool {
    !mob.equipment.get(slot).is_empty()
}

/// 官方 `getClickedSlot(Vec3)`：按点击高度（除以尺寸缩放）挑一个已装备的槽。
///
/// `getClickedSlot(Vec3)`: pick an equipped slot by the scaled click height.
fn clicked_slot(mob: &Mob, hit_pos: Option<Vec3>) -> EquipmentSlot {
    let small = mob.armor_stand.is_small();
    let scale = if small { 0.5_f64 } else { 1.0 };
    let y = hit_pos.map_or(0.0, |at| at.y) / scale;
    let span = |base: f64, small_extra: f64, normal_extra: f64| {
        (base, base + if small { small_extra } else { normal_extra })
    };
    let (lo, hi) = span(0.1, 0.8, 0.45);
    if y >= lo && y < hi && has_item(mob, EquipmentSlot::Feet) {
        return EquipmentSlot::Feet;
    }
    let (lo, hi) = (
        0.9 + if small { 0.3 } else { 0.0 },
        0.9 + if small { 1.0 } else { 0.7 },
    );
    if y >= lo && y < hi && has_item(mob, EquipmentSlot::Chest) {
        return EquipmentSlot::Chest;
    }
    let (lo, hi) = span(0.4, 1.0, 0.8);
    if y >= lo && y < hi && has_item(mob, EquipmentSlot::Legs) {
        return EquipmentSlot::Legs;
    }
    if y >= 1.6 && has_item(mob, EquipmentSlot::Head) {
        return EquipmentSlot::Head;
    }
    if !has_item(mob, EquipmentSlot::MainHand) && has_item(mob, EquipmentSlot::OffHand) {
        EquipmentSlot::OffHand
    } else {
        EquipmentSlot::MainHand
    }
}

/// 官方 `ArmorStand.swapItem`：取出/放入禁用位、创造复制、多件拆分、整堆交换。
///
/// `ArmorStand.swapItem`: taking/putting disable bits, creative copy, splitting a
/// stack and swapping the whole held stack.
fn swap_item(
    mob: &mut Mob,
    slot: EquipmentSlot,
    held: &ItemStack,
    creative: bool,
    out: &mut MobInteraction,
) -> bool {
    let bit = slot_bit(slot);
    let in_slot = mob.equipment.get(slot).clone();
    if !in_slot.is_empty()
        && mob.armor_stand.disabled_slots & (1 << (bit + DISABLE_TAKING_OFFSET)) != 0
    {
        return false;
    }
    if in_slot.is_empty()
        && mob.armor_stand.disabled_slots & (1 << (bit + DISABLE_PUTTING_OFFSET)) != 0
    {
        return false;
    }
    if creative && in_slot.is_empty() && !held.is_empty() {
        let mut copy = held.clone();
        copy.count = 1;
        mob.equipment.set(slot, copy);
        return true;
    }
    if !held.is_empty() && held.count > 1 && in_slot.is_empty() {
        let mut copy = held.clone();
        copy.count = 1;
        mob.equipment.set(slot, copy);
        out.shrink_held = true;
        return true;
    }
    mob.equipment.set(slot, held.clone());
    if in_slot.is_empty() {
        out.shrink_held = true;
    } else {
        out.replacement = Some(in_slot);
    }
    true
}

/// 官方 `ArmorStand.hurtServer`：两段式摇晃、创造一下移除、火 / 爆炸 / 箭的特殊处理，
/// 破坏后掉盔甲架物品和全部装备，发破坏音效与粒子。
///
/// `ArmorStand.hurtServer`: the two-hit wobble, one-hit creative removal, the fire /
/// explosion / arrow cases, and the item and equipment drops on breaking.
pub fn hurt_server(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    source: DamageSource,
    _amount: f32,
) -> bool {
    if me.removed {
        return false;
    }
    // `mobGriefing` 关着且 `DamageSource.getEntity()` 是 `Mob` 时完全不受伤：
    // 照字节码查来源实体的类型，不拿伤害类型的难度缩放标志近似。
    // With `mobGriefing` off and `DamageSource.getEntity()` a `Mob`, no hurt at
    // all: look up the source entity's kind rather than a scaling flag.
    let source_is_mob = source
        .attacker
        .and_then(|uid| ctx.other(uid))
        .is_some_and(|public| matches!(public.kind, crate::entity::KindPublic::Mob(_)));
    if !ctx.blocks().block_game_rules().mob_griefing && source_is_mob {
        return false;
    }
    if source.kind.bypasses_invulnerability() {
        kill(me);
        return false;
    }
    if living::is_invulnerable_to(me, mob, &source)
        || mob.armor_stand.invisible
        || mob.armor_stand.is_marker()
    {
        return false;
    }
    if damage_in_tag(source.kind, "minecraft:is_explosion") {
        broken_by_anything(me, mob, ctx);
        kill(me);
        return false;
    }
    if damage_in_tag(source.kind, "minecraft:ignites_armor_stands") {
        if me.remaining_fire_ticks > 0 {
            cause_damage(me, mob, ctx, &source, 0.15);
        } else {
            me.remaining_fire_ticks = 100; // `igniteForSeconds(5)`。
        }
        return false;
    }
    if damage_in_tag(source.kind, "minecraft:burns_armor_stands") && mob.health > 0.5 {
        cause_damage(me, mob, ctx, &source, 4.0);
        return false;
    }
    let can_break = damage_in_tag(source.kind, "minecraft:can_break_armor_stand");
    let always_kill = damage_in_tag(source.kind, "minecraft:always_kills_armor_stands");
    if !can_break && !always_kill {
        return false;
    }
    // `Player.getAbilities().mayBuild` 为假（冒险 / 旁观）时不受伤。
    // A player without `mayBuild` (adventure / spectator) cannot break it.
    if source.attacker_is_player && !source.attacker_may_build {
        return false;
    }
    if source.creative_player {
        living::play_sound(me, mob, "minecraft:entity.armor_stand.break", 1.0, 1.0);
        breaking_particles(me, mob);
        kill(me);
        return true;
    }
    let now = ctx.blocks().game_time();
    if now - mob.armor_stand.last_hit <= WOBBLE_TIME || always_kill {
        broken_by_player(me, mob, ctx);
        breaking_particles(me, mob);
        kill(me);
    } else {
        // 第一下：摇晃（实体事件 32）、ENTITY_DAMAGE、记下时刻。
        mob.events.push(MobEvent::EntityEvent { event: 32 });
        emit_damage_event(me, ctx, &source);
        mob.armor_stand.last_hit = now;
    }
    true
}

/// `getGameTime() - lastHit <= 5` 里那个阈值。
const WOBBLE_TIME: i64 = 5;

fn kill(me: &mut Entity) {
    me.discard();
}

fn emit_damage_event(me: &mut Entity, ctx: &mut dyn EntityCtx, source: &DamageSource) {
    ctx.emit(
        Command::GameEvent {
            event: GameEvent::EntityDamage,
            position: me.position,
            context: EventContext {
                source_entity: source.attacker,
                ..Default::default()
            },
        },
        me,
    );
}

/// `brokenByPlayer`：掉一个带 `CustomName` 的盔甲架物品，再走 `brokenByAnything`。
fn broken_by_player(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    // `brokenByPlayer`：掉一个带 `CustomName` 的盔甲架物品，再走 `brokenByAnything`。
    ctx.emit(
        Command::DropItem {
            stack: broken_stand_item(mob.custom_name.as_ref()),
        },
        me,
    );
    broken_by_anything(me, mob, ctx);
}

/// `Block.popResource(level, blockPosition(), new ItemStack(Items.ARMOR_STAND))`：
/// 有自定义名时 `set(CUSTOM_NAME, getCustomName())`，组件值就是文本标签的网络编码。
///
/// `Block.popResource(..., new ItemStack(Items.ARMOR_STAND))`: with a custom name,
/// `set(CUSTOM_NAME, ...)`; the component value is the text tag's network encoding.
fn broken_stand_item(custom_name: Option<&Tag>) -> ItemStack {
    let mut stack = ItemStack::new(items::ARMOR_STAND, 1);
    if let Some(name) = custom_name {
        let mut bytes = Vec::new();
        name.write_network(&mut bytes);
        stack.added.push((
            mc_bamboo_data::registries::data_component_type::CUSTOM_NAME,
            bytes,
        ));
    }
    stack
}

/// `brokenByAnything`：破坏音效、空战利品表（无掉落）、掉全部装备并清空槽位。
fn broken_by_anything(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    living::play_sound(me, mob, "minecraft:entity.armor_stand.break", 1.0, 1.0);
    // `dropAllDeathLoot` 走 `minecraft:entities/armor_stand` 空表，没有掉落。
    let drops: Vec<ItemStack> = mob
        .equipment
        .iter()
        .filter(|(_, stack)| !stack.is_empty())
        .map(|(_, stack)| stack.clone())
        .collect();
    for stack in drops {
        ctx.emit(Command::DropItem { stack }, me);
    }
    for slot in [
        EquipmentSlot::MainHand,
        EquipmentSlot::OffHand,
        EquipmentSlot::Feet,
        EquipmentSlot::Legs,
        EquipmentSlot::Chest,
        EquipmentSlot::Head,
        EquipmentSlot::Body,
        EquipmentSlot::Saddle,
    ] {
        mob.equipment.set(slot, ItemStack::empty());
    }
}

/// `causeDamage`：先扣血，血量降到 0.5 以下就按 `brokenByAnything` 破坏。
fn cause_damage(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    source: &DamageSource,
    amount: f32,
) {
    let health = mob.health - amount;
    if health <= 0.5 {
        broken_by_anything(me, mob, ctx);
        kill(me);
    } else {
        mob.health = health;
        emit_damage_event(me, ctx, source);
    }
}

/// `showBreakingParticles`：橡木木板方块粒子。
fn breaking_particles(me: &Entity, mob: &mut Mob) {
    // `BlockParticleOption(ParticleTypes.BLOCK, Blocks.OAK_PLANKS.defaultBlockState())`：
    // 参数是橡木木板的默认方块状态 ID。
    // `BlockParticleOption(..., OAK_PLANKS.defaultBlockState())`: the argument is
    // the oak planks default block-state id.
    let oak_planks = crate::BlockState(mc_bamboo_data::blocks::OAK_PLANKS)
        .default_state()
        .0;
    mob.events.push(MobEvent::Particles {
        particle: mc_bamboo_data::registries::particle_type::BLOCK,
        item: Some(i32::try_from(oak_planks).unwrap_or(0)),
        position: Vec3::new(
            me.position.x,
            me.position.y + f64::from(me.dimensions.1) * (2.0 / 3.0),
            me.position.z,
        ),
        offset: Vec3::new(
            f64::from(me.dimensions.0) / 4.0,
            f64::from(me.dimensions.1) / 4.0,
            f64::from(me.dimensions.0) / 4.0,
        ),
        speed: 0.05,
        count: 10,
    });
}

/// 伤害类型在不在某个 `minecraft:damage_type` 标签里。
fn damage_in_tag(kind: DamageKind, name: &str) -> bool {
    let Some(id) = mc_bamboo_data::damage_type_id(kind.name()) else {
        return false;
    };
    tags::TAG_GROUPS
        .iter()
        .flat_map(|group| group.tags.iter())
        .find(|entry| entry.name == name)
        .is_some_and(|entry| entry.ids.contains(&id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        archive::tag,
        entity::{
            Entity,
            mob::{MobType, nbt},
        },
    };
    use mc_bamboo_core::{Hand, Vec3};
    use mc_bamboo_data::items;

    #[test]
    fn dimensions_follow_the_client_flags() {
        let mut entity = Entity::by_mob_type(MobType::ArmorStand, Vec3::default());
        assert_eq!(entity.kind.dimensions(), (0.5, 1.975));
        entity.with_mob(|_, mob| mob.armor_stand.set_small(true));
        assert_eq!(entity.kind.dimensions(), (0.25, 0.9875));
        entity.with_mob(|_, mob| mob.armor_stand.set_marker(true));
        assert_eq!(entity.kind.dimensions(), (0.0, 0.0));
    }

    #[test]
    fn armour_stand_state_survives_a_save_and_load() {
        let mut entity = Entity::by_mob_type(MobType::ArmorStand, Vec3::new(0.5, 62.0, 0.5));
        entity.with_mob(|_, mob| {
            let stand = &mut mob.armor_stand;
            stand.set_small(true);
            stand.set_show_arms(true);
            stand.set_no_base_plate(true);
            stand.set_marker(true);
            stand.invisible = true;
            stand.disabled_slots = 0x00ff_ffff;
            stand.head_pose = Rotations::new(1.0, 2.0, 3.0);
            stand.body_pose = Rotations::new(-4.0, -5.0, -6.0);
            stand.left_arm_pose = Rotations::new(7.0, 8.0, 9.0);
            stand.right_arm_pose = Rotations::new(10.0, 11.0, 12.0);
            stand.left_leg_pose = Rotations::new(13.0, 14.0, 15.0);
            stand.right_leg_pose = Rotations::new(16.0, 17.0, 18.0);
        });
        let saved = nbt::write_mob(&entity.split()).expect("an armour stand saves");
        assert_eq!(tag::byte_or(&saved, "Invisible", 0), 1);
        assert_eq!(tag::byte_or(&saved, "Small", 0), 1);
        assert_eq!(tag::byte_or(&saved, "ShowArms", 0), 1);
        assert_eq!(tag::byte_or(&saved, "NoBasePlate", 0), 1);
        assert_eq!(tag::byte_or(&saved, "Marker", 0), 1);
        assert_eq!(tag::int_or(&saved, "DisabledSlots", 0), 0x00ff_ffff);
        let pose = tag::field(&saved, "Pose").expect("Pose is written");
        let entries = tag::entries(pose).expect("Pose is a compound");
        let head = entries
            .iter()
            .find(|(key, _)| key == "Head")
            .map(|(_, value)| value)
            .expect("Head pose is written");
        assert_eq!(
            Rotations::from_tag(head, DEFAULT_HEAD_POSE),
            Rotations::new(1.0, 2.0, 3.0)
        );

        let row = nbt::read_mob(&saved, "minecraft:armor_stand", 7)
            .expect("the tag loads")
            .expect("armour stands are modelled");
        let entity = row.into_entity();
        let mob = entity.kind.mob().expect("a mob");
        assert!(mob.no_ai);
        assert_eq!(
            mob.armor_stand.client_flags,
            CLIENT_FLAG_SMALL
                | CLIENT_FLAG_SHOW_ARMS
                | CLIENT_FLAG_NO_BASEPLATE
                | CLIENT_FLAG_MARKER
        );
        assert!(mob.armor_stand.invisible);
        assert_eq!(mob.armor_stand.disabled_slots, 0x00ff_ffff);
        assert_eq!(mob.armor_stand.head_pose, Rotations::new(1.0, 2.0, 3.0));
        assert_eq!(mob.armor_stand.body_pose, Rotations::new(-4.0, -5.0, -6.0));
        assert_eq!(mob.armor_stand.left_arm_pose, Rotations::new(7.0, 8.0, 9.0));
        assert_eq!(
            mob.armor_stand.right_arm_pose,
            Rotations::new(10.0, 11.0, 12.0)
        );
        assert_eq!(
            mob.armor_stand.left_leg_pose,
            Rotations::new(13.0, 14.0, 15.0)
        );
        assert_eq!(
            mob.armor_stand.right_leg_pose,
            Rotations::new(16.0, 17.0, 18.0)
        );
        // 标记盔甲架的碰撞盒是 (0, 0)。
        assert_eq!(entity.dimensions, (0.0, 0.0));
    }

    fn interact_request(
        held: ItemStack,
        creative: bool,
        hit_pos: Option<Vec3>,
    ) -> MobInteractRequest {
        MobInteractRequest {
            player_uid: 1,
            player_uuid: [0; 16],
            hand: Hand::Main,
            held,
            creative,
            position: Vec3::default(),
            player_height: 1.8,
            sneaking: false,
            difficulty: 2,
            hit_pos,
            spectator: false,
        }
    }

    fn armour_stand() -> Entity {
        Entity::by_mob_type(MobType::ArmorStand, Vec3::default())
    }

    #[test]
    fn wearing_a_helmet_goes_to_the_head_slot() {
        let mut entity = armour_stand();
        let req = interact_request(ItemStack::new(items::DIAMOND_HELMET, 1), false, None);
        let outcome = entity
            .with_mob(|_, mob| interact(mob, &req))
            .expect("a mob");
        assert_eq!(outcome.result, InteractionResult::SuccessServer);
        assert!(outcome.shrink_held);
        let mob = entity.kind.mob().expect("a mob");
        assert_eq!(
            mob.equipment.get(EquipmentSlot::Head).item,
            items::DIAMOND_HELMET
        );
        assert!(mob.needs_sync);
    }

    #[test]
    fn taking_off_with_an_empty_hand_returns_the_stack() {
        let mut entity = armour_stand();
        entity.with_mob(|_, mob| {
            mob.equipment.set(
                EquipmentSlot::Head,
                ItemStack::new(items::DIAMOND_HELMET, 1),
            );
        });
        // 头部高度（相对实体）选中 HEAD 槽。
        let req = interact_request(ItemStack::empty(), false, Some(Vec3::new(0.0, 1.7, 0.0)));
        let outcome = entity
            .with_mob(|_, mob| interact(mob, &req))
            .expect("a mob");
        assert_eq!(outcome.result, InteractionResult::SuccessServer);
        assert_eq!(
            outcome.replacement.as_ref().map(|stack| stack.item),
            Some(items::DIAMOND_HELMET)
        );
        let mob = entity.kind.mob().expect("a mob");
        assert!(mob.equipment.get(EquipmentSlot::Head).is_empty());
    }

    #[test]
    fn a_marker_cannot_be_interacted_with() {
        let mut entity = armour_stand();
        entity.with_mob(|_, mob| mob.armor_stand.set_marker(true));
        let req = interact_request(ItemStack::new(items::DIAMOND_HELMET, 1), false, None);
        let outcome = entity
            .with_mob(|_, mob| interact(mob, &req))
            .expect("a mob");
        assert_eq!(outcome.result, InteractionResult::Pass);
    }

    #[test]
    fn a_hand_slot_without_arms_fails() {
        let mut entity = armour_stand();
        // 木棍没有 `equippable` 组件，落到 MAINHAND（手槽），无手臂时失败。
        let req = interact_request(ItemStack::new(items::STICK, 1), false, None);
        let outcome = entity
            .with_mob(|_, mob| interact(mob, &req))
            .expect("a mob");
        assert_eq!(outcome.result, InteractionResult::Fail);
    }

    #[test]
    fn disabled_slots_block_wearing() {
        let mut entity = armour_stand();
        // HEAD 的 `getFilterBit(0)` = 5。
        entity.with_mob(|_, mob| mob.armor_stand.disabled_slots |= 1 << 5);
        let req = interact_request(ItemStack::new(items::DIAMOND_HELMET, 1), false, None);
        let outcome = entity
            .with_mob(|_, mob| interact(mob, &req))
            .expect("a mob");
        assert_eq!(outcome.result, InteractionResult::Fail);
    }

    #[test]
    fn creative_copies_one_without_consuming() {
        let mut entity = armour_stand();
        let req = interact_request(ItemStack::new(items::DIAMOND_HELMET, 3), true, None);
        let outcome = entity
            .with_mob(|_, mob| interact(mob, &req))
            .expect("a mob");
        assert_eq!(outcome.result, InteractionResult::SuccessServer);
        assert!(!outcome.shrink_held);
        assert!(outcome.replacement.is_none());
        let mob = entity.kind.mob().expect("a mob");
        assert_eq!(mob.equipment.get(EquipmentSlot::Head).count, 1);
    }

    #[test]
    fn hurt_damage_tags_match_the_official_registry() {
        assert!(damage_in_tag(
            DamageKind::PlayerAttack,
            "minecraft:can_break_armor_stand"
        ));
        assert!(damage_in_tag(
            DamageKind::Arrow,
            "minecraft:always_kills_armor_stands"
        ));
        assert!(damage_in_tag(
            DamageKind::Explosion,
            "minecraft:is_explosion"
        ));
        assert!(!damage_in_tag(
            DamageKind::PlayerAttack,
            "minecraft:always_kills_armor_stands"
        ));
    }

    #[test]
    fn the_click_height_picks_the_equipped_slot() {
        let cases = [
            (0.3_f64, EquipmentSlot::Feet),
            (0.7, EquipmentSlot::Legs),
            (1.1, EquipmentSlot::Chest),
            (1.8, EquipmentSlot::Head),
        ];
        for (y, slot) in cases {
            let mut entity = armour_stand();
            entity.with_mob(|_, mob| {
                mob.equipment
                    .set(slot, ItemStack::new(items::DIAMOND_HELMET, 1));
            });
            let req = interact_request(ItemStack::empty(), false, Some(Vec3::new(0.0, y, 0.0)));
            let outcome = entity
                .with_mob(|_, mob| interact(mob, &req))
                .expect("a mob");
            assert_eq!(
                outcome.replacement.as_ref().map(|stack| stack.item),
                Some(items::DIAMOND_HELMET),
                "slot {slot:?} at y={y}"
            );
            assert!(
                entity
                    .kind
                    .mob()
                    .expect("a mob")
                    .equipment
                    .get(slot)
                    .is_empty()
            );
        }
    }

    #[test]
    fn the_broken_stand_item_carries_the_custom_name() {
        let name = Tag::String("Hall Monitor".to_owned());
        let stack = broken_stand_item(Some(&name));
        assert_eq!(stack.item, items::ARMOR_STAND);
        assert_eq!(stack.count, 1);
        assert_eq!(stack.added.len(), 1);
        assert_eq!(
            stack.added[0].0,
            mc_bamboo_data::registries::data_component_type::CUSTOM_NAME
        );
        let mut expected = Vec::new();
        name.write_network(&mut expected);
        assert_eq!(stack.added[0].1, expected);
        assert!(broken_stand_item(None).added.is_empty());
    }

    #[test]
    fn a_spectator_gets_success_without_changing_equipment() {
        let mut entity = armour_stand();
        let mut req = interact_request(ItemStack::new(items::DIAMOND_HELMET, 1), false, None);
        req.spectator = true;
        let outcome = entity
            .with_mob(|_, mob| interact(mob, &req))
            .expect("a mob");
        assert_eq!(outcome.result, InteractionResult::Success);
        assert!(
            entity
                .kind
                .mob()
                .expect("a mob")
                .equipment
                .get(EquipmentSlot::Head)
                .is_empty()
        );
    }

    #[test]
    fn a_name_tag_does_not_start_an_equipment_interaction() {
        let mut entity = armour_stand();
        let req = interact_request(ItemStack::new(items::NAME_TAG, 1), false, None);
        let outcome = entity
            .with_mob(|_, mob| interact(mob, &req))
            .expect("a mob");
        assert_eq!(outcome.result, InteractionResult::Pass);
    }

    #[test]
    fn a_player_without_may_build_cannot_break_a_stand() {
        use crate::{BlockTicks, World, testing::TestLevel};
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let uid =
            world.add_prepared_entity(Entity::by_mob_type(MobType::ArmorStand, Vec3::default()));
        let mut source = DamageSource::player(1, Vec3::default(), false);
        source.attacker_may_build = false;
        assert!(!world.hurt_mob(uid, source, 1.0));
        let source = DamageSource::player(1, Vec3::default(), false);
        assert!(world.hurt_mob(uid, source, 1.0));
    }

    #[test]
    fn an_armour_stand_save_skips_mob_and_ageable_keys() {
        let mut entity = Entity::by_mob_type(MobType::ArmorStand, Vec3::default());
        entity.with_mob(|_, mob| {
            mob.armor_stand.set_small(true);
            mob.armor_stand.disabled_slots = 0x00ff_ffff;
        });
        let saved = nbt::write_mob(&entity.split()).expect("an armour stand saves");
        for key in [
            "CanPickUpLoot",
            "PersistenceRequired",
            "LeftHanded",
            "NoAI",
            "drop_chances",
            "leash",
            "home_radius",
            "home_pos",
            "DeathLootTableSeed",
            "Age",
            "ForcedAge",
            "AgeLocked",
            "InLove",
            "LoveCause",
        ] {
            assert!(tag::field(&saved, key).is_none(), "unexpected {key}");
        }
        // 父类链 `LivingEntity` 无条件写 `Brain`；`ArmorStand` 自己写这些键。
        // `LivingEntity` always writes `Brain`; `ArmorStand` writes its own keys.
        for key in [
            "Brain",
            "Health",
            "Invisible",
            "Small",
            "ShowArms",
            "DisabledSlots",
            "NoBasePlate",
            "Pose",
        ] {
            assert!(tag::field(&saved, key).is_some(), "missing {key}");
        }
    }

    #[test]
    fn an_armour_stand_has_no_brain_and_uses_the_empty_loot_table() {
        assert!(super::super::new_brain(MobType::ArmorStand).is_none());
        assert_eq!(
            MobType::ArmorStand.loot_table(),
            "minecraft:entities/armor_stand"
        );
    }
}
