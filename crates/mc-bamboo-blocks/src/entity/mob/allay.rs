//! 悦灵（`Allay`）：`PathfinderMob` + `InventoryCarrier` + `VibrationSystem`。
//!
//! 官方 `Allay` 的构造、属性、飞行控制、交互、心跳 / 复制冷却、捡物和存档集中在这里；
//! 大脑结构表在 [`super::allay_ai`]，游戏层的物品栏落地走 [`super::interact`] 的
//! `MobInteraction`。
//!
//! The allay (`Allay`): a `PathfinderMob` + `InventoryCarrier` + `VibrationSystem`.
//! The brain lives in [`super::allay_ai`]; inventory changes are applied by the game
//! layer from `MobInteraction`.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{
    blocks, items,
    registries::{attribute, data_component_type as kinds, entity_type as ids},
    tags::item_tags,
};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{components::defaults, item::ItemStack, packet::play::EquipmentSlot};

use super::{
    Mob, MobEvent, MobType,
    attributes::Attributes,
    interact::{MobInteractRequest, MobInteraction},
};
use crate::{
    BlockGameRules, InteractionResult,
    entity::{
        Entity, EntityUid,
        ctx::{Command, EntityCtx},
    },
    game_event::{
        GameEvent,
        dispatch::EntityListener,
        vibration::{VibrationData, VibrationUser},
    },
    state::item_is_tagged,
};

/// `Allay` 构造时连续定义的两个布尔元数据。`PathfinderMob` 的最后一个是
/// `DATA_MOB_FLAGS_ID`(15)，所以这两个是 16 / 17（已在字节码的类初始化里核对）。
pub const DATA_DANCING: u8 = 16;
pub const DATA_CAN_DUPLICATE: u8 = 17;

/// `DEFAULT_DUPLICATION_COOLDOWN` / `DUPLICATION_COOLDOWN_TICKS`：复制后两只各 6000 刻。
pub const DUPLICATION_COOLDOWN: i64 = 6000;
/// `AllayAi.TIME_TO_FORGET_NOTEBLOCK`。
pub const NOTEBLOCK_COOLDOWN: i32 = 600;
/// `GoAndGiveItemsToTarget` 的 `cooldownDuration`。
pub const ITEM_PICKUP_COOLDOWN: i32 = 60;
/// `VibrationUser.VIBRATION_EVENT_LISTENER_RANGE`。
pub const LISTENER_RADIUS: i32 = 16;

/// 对拍改坏确认的三种生产代码篡改。只有 `mob-oracle --break-self-test` 会设置，
/// 其它驱动恒为 [`Tamper::None`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tamper {
    None,
    /// 送物品只看喜欢的玩家、不看音符盒（`shouldDepositItemsAtLikedNoteblock` 恒假）。
    DepositPlayerOnly,
    /// 复制冷却不减（`updateDuplicationCooldown` 空转）。
    NoDuplicationCooldown,
    /// 停舞判定反转（`shouldStopDancing` 取反）。
    InvertStopDancing,
    /// `SetEntityLookTargetSometimes` 的任意活体变体改回只找玩家（`allay_look_cow`
    /// 应报出差异）。
    LookPlayerOnly,
}

thread_local! {
    static ORACLE_TAMPER: std::cell::Cell<Tamper> = const { std::cell::Cell::new(Tamper::None) };
}

/// 由 `mob-oracle` 按 `--break-self-test` 设置，其它驱动不调用。
pub fn set_oracle_tamper(name: &str) {
    let tamper = match name {
        "allay_deposit_player" => Tamper::DepositPlayerOnly,
        "allay_no_dup_cooldown" => Tamper::NoDuplicationCooldown,
        "allay_dance_inverted" => Tamper::InvertStopDancing,
        "allay_look_player_only" => Tamper::LookPlayerOnly,
        _ => Tamper::None,
    };
    ORACLE_TAMPER.with(|cell| cell.set(tamper));
}

pub(crate) fn oracle_tamper() -> Tamper {
    ORACLE_TAMPER.with(|cell| cell.get())
}

/// 悦灵自己的状态：每个 `Mob` 都带一份，字段只在 `Allay` 分支读写。
#[derive(Debug, Clone, PartialEq)]
pub struct Allay {
    /// `SimpleContainer(1)`，存档键 `Inventory`。
    pub inventory: Vec<ItemStack>,
    /// `DATA_DANCING` 元数据。
    pub dancing: bool,
    /// `DATA_CAN_DUPLICATE` 元数据（复制冷却为 0 时为真）。
    pub can_duplicate: bool,
    /// `jukeboxPos`：正在跟着跳舞的唱片机。
    pub jukebox_pos: Option<BlockPos>,
    /// `duplicationCooldown`，存档键 `DuplicationCooldown`。
    pub duplication_cooldown: i64,
    /// 心跳：每 10 刻回 1 点血（`Allay.aiStep`）。
    pub heal_phase: i32,
}

impl Default for Allay {
    fn default() -> Self {
        Self {
            inventory: vec![ItemStack::empty()],
            dancing: false,
            can_duplicate: true,
            jukebox_pos: None,
            duplication_cooldown: 0,
            heal_phase: 0,
        }
    }
}

impl Allay {
    /// `hasItemInHand()`：主手有没有物品。
    pub fn has_item_in_hand(&self, mob: &Mob) -> bool {
        !mob.equipment.get(EquipmentSlot::MainHand).is_empty()
    }

    /// `getInventory().isEmpty()`。
    pub fn has_item_to_throw(&self) -> bool {
        !self.inventory.iter().all(ItemStack::is_empty)
    }

    /// `SimpleContainer.canAddItem`：一格背包，同类物品还可合并。
    pub fn can_add_item(&self, stack: &ItemStack) -> bool {
        if stack.is_empty() {
            return false;
        }
        let slot = self
            .inventory
            .first()
            .cloned()
            .unwrap_or_else(ItemStack::empty);
        slot.is_empty()
            || (slot.item == stack.item
                && slot.is_same_item_same_components(stack)
                && slot.count < stack.max_stack_size().min(64))
    }

    /// `SimpleContainer.addItem`：返回没放进去的那一部分。
    pub fn add_item(&mut self, stack: ItemStack) -> ItemStack {
        if stack.is_empty() || !self.can_add_item(&stack) {
            return stack;
        }
        let max = stack.max_stack_size().min(64);
        let slot = &mut self.inventory[0];
        if slot.is_empty() {
            let mut placed = stack.clone();
            let take = stack.count.min(max);
            placed.count = take;
            *slot = placed;
            let mut leftover = stack;
            leftover.count -= take;
            return if leftover.count <= 0 {
                ItemStack::empty()
            } else {
                leftover
            };
        }
        let take = stack.count.min(max - slot.count);
        slot.count += take;
        let mut leftover = stack;
        leftover.count -= take;
        if leftover.count <= 0 {
            ItemStack::empty()
        } else {
            leftover
        }
    }

    /// `SimpleContainer.removeItem(0, 1)`。
    pub fn remove_one(&mut self) -> ItemStack {
        let Some(slot) = self.inventory.first_mut() else {
            return ItemStack::empty();
        };
        if slot.is_empty() {
            return ItemStack::empty();
        }
        let mut taken = slot.clone();
        taken.count = 1;
        slot.count -= 1;
        if slot.count <= 0 {
            *slot = ItemStack::empty();
        }
        taken
    }

    /// `SimpleContainer.removeAllItems()`。
    pub fn remove_all(&mut self) -> Vec<ItemStack> {
        let mut out = Vec::new();
        for slot in &mut self.inventory {
            if !slot.is_empty() {
                out.push(std::mem::replace(slot, ItemStack::empty()));
            }
        }
        out
    }
}

/// `Allay.createAttributes()`：20 血、0.1 飞行速度、0.1 移速、2 攻击。
pub(super) fn attributes() -> Attributes {
    // `Mob.createMobAttributes()` 里没有 `FLYING_SPEED` / `ATTACK_DAMAGE` 条目，
    // 官方的 `.add(...)` 把它们追加进去。
    let mut table = Attributes::monster();
    table.set_base(attribute::MAX_HEALTH, 20.0);
    table.set_base(attribute::MOVEMENT_SPEED, f64::from(0.1_f32));
    table.set_base(attribute::ATTACK_DAMAGE, 2.0);
    if table.get(attribute::FLYING_SPEED).is_none() {
        table.entries.push(super::attributes::Attribute::new(
            attribute::FLYING_SPEED,
            f64::from(0.1_f32),
        ));
    }
    table
}

/// 构造函数：`FlyingPathNavigation`（打不开门、能浮水、路径长 48）、
/// `setCanPickUpLoot(true)` 和两个动态游戏事件监听者。
pub(super) fn configure(mob: &mut Mob) {
    mob.navigation.flags.fly = true;
    mob.navigation.flags.can_float = true;
    mob.navigation.flags.can_open_doors = false;
    mob.navigation.required_path_length = 48.0;
    mob.can_pick_up_loot = true;
    mob.entity_listener = Some(EntityListener {
        radius: LISTENER_RADIUS,
        delivery_mode: crate::game_event::dispatch::DeliveryMode::ByDistance,
        vibration: VibrationData::default(),
    });
    mob.allay = Allay::default();
}

/// `Allay.travel`：`travelFlying(input, getSpeed())`，无重力、无地面摩擦。
pub(super) fn travel(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, input: Vec3) {
    let (speed, drag) = if me.in_water {
        (0.02_f32, f64::from(0.8_f32))
    } else if super::living::is_in_lava(me, mob) {
        (0.02_f32, 0.5)
    } else {
        (mob.speed, f64::from(0.91_f32))
    };
    super::living::move_relative(me, speed, input);
    let movement = me.delta;
    super::motion::move_living(me, mob, ctx, movement);
    me.delta = me.delta.scale(drag);
}

/// `Allay.getAmbientSound()`：手上有物品时用另一条环境音。
pub fn ambient_sound(mob: &Mob) -> &'static str {
    if mob.allay.has_item_in_hand(mob) {
        "minecraft:entity.allay.ambient_with_item"
    } else {
        "minecraft:entity.allay.ambient_without_item"
    }
}

/// `Allay.aiStep`：每 10 刻回 1 血、每 20 刻检查是否该停舞、复制冷却每刻减一。
pub(super) fn ai_step(me: &Entity, mob: &mut Mob, ctx: &dyn EntityCtx) {
    let tick = me.tick_count;
    if tick % 10 == 0 && super::living::is_alive(me, mob) {
        let max = mob.attributes.value(attribute::MAX_HEALTH) as f32;
        mob.health = (mob.health + 1.0).clamp(0.0, max);
    }
    let stop_dancing = mob.allay.dancing && should_stop_dancing(me, mob, ctx);
    if stop_dancing && tick % 20 == 0 {
        set_dancing(mob, false);
        mob.allay.jukebox_pos = None;
    }
    tick_duplication_cooldown(mob);
}

/// `Allay.updateDuplicationCooldown()`：服务端每刻减一。
pub fn tick_duplication_cooldown(mob: &mut Mob) {
    if oracle_tamper() == Tamper::NoDuplicationCooldown {
        return;
    }
    if mob.allay.duplication_cooldown > 0 {
        set_duplication_cooldown(mob, mob.allay.duplication_cooldown - 1);
    }
}

/// `Allay.shouldStopDancing()`：唱片机还在半径 10 内且那一格仍是唱片机就继续跳。
fn should_stop_dancing(me: &Entity, mob: &Mob, ctx: &dyn EntityCtx) -> bool {
    let Some(pos) = mob.allay.jukebox_pos else {
        return true;
    };
    let center = Vec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    );
    let radius = f64::from(GameEvent::JukeboxPlay.notification_radius());
    if center.distance_to_sqr(me.position) >= radius * radius {
        return true;
    }
    let stop = !ctx.blocks().block_state(pos).is(blocks::JUKEBOX);
    if oracle_tamper() == Tamper::InvertStopDancing {
        !stop
    } else {
        stop
    }
}

/// `Allay.setDancing`：服务端不开 AI 或正在恐慌时不能开始跳。
pub fn set_dancing(mob: &mut Mob, dancing: bool) {
    if mob.no_ai || (dancing && mob.is_panicking()) {
        return;
    }
    mob.allay.dancing = dancing;
}

/// `Allay.setJukeboxPlaying`。
pub fn set_jukebox_playing(mob: &mut Mob, pos: BlockPos, playing: bool) {
    if playing {
        if !mob.allay.dancing {
            mob.allay.jukebox_pos = Some(pos);
            set_dancing(mob, true);
        }
    } else if mob.allay.jukebox_pos == Some(pos) || mob.allay.jukebox_pos.is_none() {
        mob.allay.jukebox_pos = None;
        set_dancing(mob, false);
    }
}

/// `Allay.setDuplicationCooldown`：同步 `DATA_CAN_DUPLICATE`。
pub fn set_duplication_cooldown(mob: &mut Mob, cooldown: i64) {
    mob.allay.duplication_cooldown = cooldown.max(0);
    mob.allay.can_duplicate = mob.allay.duplication_cooldown == 0;
}

/// `Allay.mobInteract`：复制、递物、取回三支。丢出背包 / 复制出的新实体由调用方落地。
pub(super) fn interact(
    me: &Entity,
    mob: &mut Mob,
    req: &MobInteractRequest,
) -> Option<MobInteraction> {
    if mob.mob_type != MobType::Allay {
        return None;
    }
    let held = mob.equipment.get(EquipmentSlot::MainHand).clone();
    // `duplicates_allays` + 跳舞 + 冷却结束：复制。
    if mob.allay.dancing
        && !req.held.is_empty()
        && item_is_tagged(req.held.item, item_tags::DUPLICATES_ALLAYS)
        && mob.allay.can_duplicate
    {
        set_duplication_cooldown(mob, DUPLICATION_COOLDOWN);
        super::living::play_sound(me, mob, "minecraft:block.amethyst_block.chime", 2.0, 1.0);
        entity_event_18(mob);
        return Some(MobInteraction {
            result: InteractionResult::Success,
            shrink_held: !req.creative,
            duplicate_allay: true,
            ..MobInteraction::pass()
        });
    }
    // 空手 + 手里没东西：递一个物品，记下喜欢的玩家。
    if held.is_empty() && !req.held.is_empty() {
        let mut one = req.held.clone();
        one.count = 1;
        mob.equipment.set(EquipmentSlot::MainHand, one);
        super::living::play_sound(me, mob, "minecraft:entity.allay.item_given", 2.0, 1.0);
        if let Some(brain) = mob.brain.as_mut() {
            brain.memories.set(
                super::brain::memory::MemoryType::LikedPlayer,
                super::brain::memory::MemoryValue::Uuid(req.player_uuid),
            );
        }
        return Some(MobInteraction {
            result: InteractionResult::Success,
            shrink_held: !req.creative,
            entity_event: Some(18),
            ..MobInteraction::pass()
        });
    }
    // 主手空手 + 悦灵手里有东西：取回并丢出背包里的全部物品。
    if !held.is_empty() && req.hand == mc_bamboo_core::Hand::Main && req.held.is_empty() {
        mob.equipment
            .set(EquipmentSlot::MainHand, ItemStack::empty());
        super::living::play_sound(me, mob, "minecraft:entity.allay.item_taken", 2.0, 1.0);
        let thrown = mob.allay.remove_all();
        if let Some(brain) = mob.brain.as_mut() {
            brain
                .memories
                .erase(super::brain::memory::MemoryType::LikedPlayer);
        }
        return Some(MobInteraction {
            result: InteractionResult::Success,
            replacement: Some(held),
            thrown_stacks: thrown,
            ..MobInteraction::pass()
        });
    }
    // 悦灵不是 `Animal`：`PathfinderMob.mobInteract` 恒 PASS。
    Some(MobInteraction::pass())
}

/// `Allay.wantsToPickUp`：手上必须有「模板」物品，mobGriefing 开、背包放得下、
/// 且掉落物和主手同种（药水成分也一样）才捡。
pub(super) fn wants_to_pick_up(mob: &Mob, stack: &ItemStack, rules: &BlockGameRules) -> bool {
    let held = mob.equipment.get(EquipmentSlot::MainHand);
    if held.is_empty() {
        return false;
    }
    if !rules.mob_griefing || stack.is_empty() {
        return false;
    }
    mob.allay.can_add_item(stack) && allay_considers_item_equal(held, stack)
}

/// `Allay.allayConsidersItemEqual`：`ItemStack.isSameItem` 且药水成分相同。
fn allay_considers_item_equal(a: &ItemStack, b: &ItemStack) -> bool {
    a.is_same_item(b) && !has_non_matching_potion(a, b)
}

/// `Allay.hasNonMatchingPotion`：两边 `potion_contents`（含物品默认值）不一样。
fn has_non_matching_potion(a: &ItemStack, b: &ItemStack) -> bool {
    let patch_a = a.patch();
    let patch_b = b.patch();
    defaults::resolve(a.item, &patch_a, kinds::POTION_CONTENTS)
        != defaults::resolve(b.item, &patch_b, kinds::POTION_CONTENTS)
}

/// `Allay.customServerAiStep`：大脑、每刻活动重选、振动系统。
pub(super) fn custom_server_ai_step(
    me: &mut Entity,
    mob: &mut Mob,
    ctx: &mut dyn EntityCtx,
    decision_gap: i64,
) {
    let now = ctx.blocks().game_time();
    // 官方 `Allay.tick` 在服务端先 `PathfinderMob.tick`（里面跑 `customServerAiStep`：
    // 大脑 + 活动重选），之后才 `VibrationSystem.Ticker.tick`，最后恐慌停舞。
    // `Allay.tick` on the server runs `PathfinderMob.tick` (brain + activity) first,
    // then `VibrationSystem.Ticker.tick`, then the panic stop.
    if let Some(mut brain) = mob.brain.take() {
        let replaced = brain.tick_by(me, mob, ctx, decision_gap.max(1));
        mob.brain = Some(Box::new(replaced.unwrap_or(*brain)));
    }
    if let Some(brain) = mob.brain.as_mut() {
        super::allay_ai::update_activity(brain);
    }
    tick_vibration(me, mob, ctx, now);
    // `Allay.tick` 还在恐慌时立刻停舞。
    if mob.is_panicking() {
        set_dancing(mob, false);
    }
}

/// `Allay.tick` → `VibrationSystem.Ticker.tick`：监听点在眼睛，到达时回调 User。
fn tick_vibration(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, now: i64) {
    let eye = Vec3::new(
        me.position.x,
        me.position.y + f64::from(mob.eye_height),
        me.position.z,
    );
    let mut data = mob
        .entity_listener
        .map(|listener| listener.vibration)
        .unwrap_or_default();
    {
        let mut user = AllayVibrationUser {
            me: &mut *me,
            mob,
            ctx: &*ctx,
        };
        crate::game_event::vibration::tick(&mut data, &mut user, now, Some(eye), false);
    }
    if let Some(listener) = mob.entity_listener.as_mut() {
        listener.vibration = data;
    }
}

/// `Allay$VibrationUser.canReceiveVibration` 的静态部分：不开 AI、不是音符盒事件、
/// 或已记下的音符盒仍在冷却期且事件与那一格不符时拒绝。
pub fn can_receive_vibration(mob: &Mob, event: GameEvent, source: BlockPos) -> bool {
    if mob.no_ai || event != GameEvent::NoteBlockPlay {
        return false;
    }
    let Some(brain) = mob.brain.as_ref() else {
        return true;
    };
    match brain
        .memories
        .global_pos(super::brain::memory::MemoryType::LikedNoteblockPosition)
    {
        None => true,
        Some(gp) => gp.dimension == "minecraft:overworld" && gp.pos == source,
    }
}

/// `Allay$VibrationUser`：半径 16、眼睛位置、只认 `#allay_can_listen` 的事件。
struct AllayVibrationUser<'a> {
    me: &'a mut Entity,
    mob: &'a mut Mob,
    ctx: &'a dyn EntityCtx,
}

impl VibrationUser for AllayVibrationUser<'_> {
    fn radius(&self) -> i32 {
        LISTENER_RADIUS
    }

    fn position(&self) -> Option<Vec3> {
        Some(Vec3::new(
            self.me.position.x,
            self.me.position.y + f64::from(self.mob.eye_height),
            self.me.position.z,
        ))
    }

    fn can_receive(&self, event: GameEvent, source: BlockPos) -> bool {
        can_receive_vibration(self.mob, event, source)
    }

    fn on_receive(
        &mut self,
        event: GameEvent,
        source: BlockPos,
        _source_player: Option<EntityUid>,
        _distance: f32,
    ) {
        if event == GameEvent::NoteBlockPlay {
            super::allay_ai::hear_noteblock(self.mob, self.ctx, source);
        }
    }

    fn send_particles(&mut self, _from: Vec3, _to: Vec3, _travel: i32) -> usize {
        0
    }
}

/// `Allay.addAdditionalSaveData`：`Inventory` / `listener` / `DuplicationCooldown`。
pub fn write_nbt(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    let items: Vec<Tag> = mob
        .allay
        .inventory
        .iter()
        .filter(|stack| !stack.is_empty())
        .filter_map(|stack| mc_bamboo_protocol::components::archive::slots::encode_item(stack).ok())
        .collect();
    crate::archive::tag::put(out, "Inventory", Tag::List(items));
    let listener = mob
        .entity_listener
        .map(|state| state.vibration)
        .unwrap_or_default();
    crate::archive::tag::put(out, "listener", Tag::Compound(listener.write_codec()));
    crate::archive::tag::put(
        out,
        "DuplicationCooldown",
        Tag::Long(mob.allay.duplication_cooldown),
    );
}

/// `Allay.readAdditionalSaveData`：`DuplicationCooldown` 走 `getIntOr(key, 0)`。
pub fn read_nbt(mob: &mut Mob, tag: &Tag) {
    if let Some(Tag::List(items)) = crate::archive::tag::field(tag, "Inventory") {
        let mut inventory = vec![ItemStack::empty()];
        for (index, item) in items.iter().enumerate().take(1) {
            if let Ok(stack) = mc_bamboo_protocol::components::archive::slots::decode_item(item) {
                inventory[index] = stack;
            }
        }
        mob.allay.inventory = inventory;
    }
    if let Some(listener) = crate::archive::tag::field(tag, "listener") {
        let data = crate::archive::passive::read_listener_for_vibration(listener);
        if let Some(state) = mob.entity_listener.as_mut() {
            state.vibration = data;
        }
    }
    let cooldown = i64::from(crate::archive::tag::int_or(tag, "DuplicationCooldown", 0));
    set_duplication_cooldown(mob, cooldown);
    mob.can_pick_up_loot = true;
}

/// `Allay.dropEquipment`：背包和主手都掉出来。
pub(super) fn drop_equipment(mob: &mut Mob) -> Vec<ItemStack> {
    let mut out = mob.allay.remove_all();
    let held = mob.equipment.get(EquipmentSlot::MainHand).clone();
    if !held.is_empty() {
        out.push(held);
        mob.equipment
            .set(EquipmentSlot::MainHand, ItemStack::empty());
    }
    out
}

/// `Allay.handleEntityEvent(18)`：三颗心。服务端只发实体事件。
fn entity_event_18(mob: &mut Mob) {
    mob.events.push(MobEvent::EntityEvent { event: 18 });
}

/// `Allay` 投掷时交给游戏层的命令（起点在眼高 - 0.2）。
pub(super) fn throw_command(stack: ItemStack, position: Vec3, delta: Vec3) -> Command {
    Command::AllayThrow {
        stack,
        position,
        delta,
    }
}

/// `EntityTypes.ALLAY` 的 id，供注册与对拍断言用。
pub const ENTITY_TYPE: i32 = ids::ALLAY;
/// 音符盒的物品 ID，供递物场景用。
pub const NOTE_BLOCK_ITEM: i32 = items::NOTE_BLOCK;

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};

    /// 官方构造事实和元数据缺省值。
    #[test]
    fn allay_construction_facts() {
        let mob = Mob::new(MobType::Allay);
        assert_eq!(MobType::Allay.entity_type(), ids::ALLAY);
        assert_eq!(MobType::Allay.dimensions(false), (0.35, 0.6, 0.36));
        assert_eq!(mob.attributes.base(attribute::MAX_HEALTH), 20.0);
        assert_eq!(
            mob.attributes.base(attribute::FLYING_SPEED),
            f64::from(0.1_f32)
        );
        assert!(mob.can_pick_up_loot);
        assert!(mob.navigation.flags.fly);
        assert!(mob.navigation.flags.can_float);
        assert!(!mob.navigation.flags.can_open_doors);
        assert_eq!(mob.navigation.required_path_length, 48.0);
        let listener = mob.entity_listener.expect("allay is a listener");
        assert_eq!(listener.radius, LISTENER_RADIUS);
        assert!(mob.allay.can_duplicate);
        assert!(!mob.allay.dancing);
    }

    /// 复制冷却从 6000 减到 0 才恢复 `DATA_CAN_DUPLICATE`。
    #[test]
    fn duplication_cooldown_controls_the_metadata() {
        let mut mob = Mob::new(MobType::Allay);
        set_duplication_cooldown(&mut mob, 3);
        assert!(!mob.allay.can_duplicate);
        tick_duplication_cooldown(&mut mob);
        assert_eq!(mob.allay.duplication_cooldown, 2);
        assert!(!mob.allay.can_duplicate);
        set_duplication_cooldown(&mut mob, 0);
        assert!(mob.allay.can_duplicate);
    }

    /// 存档往返保留背包、监听者和复制冷却。
    #[test]
    fn allay_nbt_round_trip() {
        let mut mob = Mob::new(MobType::Allay);
        mob.allay.add_item(ItemStack::new(items::AMETHYST_SHARD, 3));
        set_duplication_cooldown(&mut mob, 123);
        let mut out = Vec::new();
        write_nbt(&mob, &mut out);
        let tag = Tag::Compound(out);
        let mut rebuilt = Mob::new(MobType::Allay);
        read_nbt(&mut rebuilt, &tag);
        assert_eq!(rebuilt.allay.inventory[0].count, 3);
        assert_eq!(rebuilt.allay.duplication_cooldown, 123);
        assert!(!rebuilt.allay.can_duplicate);
    }

    /// `allayConsidersItemEqual` 只比物品和药水成分，不比其它组件：同种物品即便
    /// 其它组件不同也算相同，药水成分不一样才算不同。
    #[test]
    fn item_equality_only_compares_item_and_potion() {
        let plain = ItemStack::new(items::NOTE_BLOCK, 1);
        assert!(allay_considers_item_equal(&plain, &plain));
        assert!(!allay_considers_item_equal(
            &plain,
            &ItemStack::new(items::AMETHYST_SHARD, 1)
        ));

        // 同一个物品、一个显式设了药水成分一个没有 → 药水不同。
        let mut with_potion = ItemStack::new(items::NOTE_BLOCK, 1);
        let mut patch = ComponentPatch::new();
        patch.put(
            kinds::POTION_CONTENTS,
            PatchEntry::Set(ComponentValue::PotionContents {
                potion: Some(1),
                custom_color: None,
                custom_effects: Vec::new(),
                custom_name: None,
            }),
        );
        let (added, removed) = patch.to_opaque();
        with_potion.added = added;
        with_potion.removed = removed;
        assert!(!allay_considers_item_equal(&plain, &with_potion));
    }
}
