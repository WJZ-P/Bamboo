//! 鹦鹉螺 / 僵尸鹦鹉螺：官方 `AbstractNautilus`、`Nautilus`、`ZombieNautilus` 的
//! 构造事实、状态、音效分派、生成规则和坐骑相关判定。
//!
//! 大脑在 [`super::nautilus_ai`]；驯服、上鞍、物品栏和骑乘复用 [`super::tame`] 与
//! [`super::equine::inventory`] 那两条已有链路。
//!
//! Nautiluses / zombie nautiluses: construction facts, state, sound dispatch, spawn
//! rules and mount predicates from vanilla `AbstractNautilus`, `Nautilus` and
//! `ZombieNautilus`. The brain lives in [`super::nautilus_ai`]; taming, saddling, the
//! inventory and riding reuse the existing [`super::tame`] and
//! [`super::equine::inventory`] paths.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{blocks, items, registries::attribute, tags::item_tags};
use mc_bamboo_protocol::{item::ItemStack, packet::play::EquipmentSlot};

use super::{Attributes, Malus, Mob, MobEvent, MobType, attributes::Attribute, living, motion};
use crate::{
    entity::{Entity, ctx::EntityCtx},
    state::item_is_tagged,
    world::World,
};
use mc_bamboo_data::PathType;

/// 是不是两种鹦鹉螺之一。
///
/// Whether this is either nautilus kind.
pub const fn is_nautilus(kind: MobType) -> bool {
    matches!(kind, MobType::Nautilus | MobType::ZombieNautilus)
}

/// `SpawnPlacements`：两种都是 `IN_WATER`。
///
/// Both register as `IN_WATER` in `SpawnPlacements`.
pub const fn spawns_in_water(kind: MobType) -> bool {
    is_nautilus(kind)
}

/// `EntityTypes.NAUTILUS` / `ZOMBIE_NAUTILUS`：成年 0.875×0.95、眼高 0.2751，
/// 幼年整体乘 0.5（幼年没有单独的眼高，沿用成年值）。
///
/// `EntityTypes.NAUTILUS` / `ZOMBIE_NAUTILUS`: 0.875 by 0.95 with eye height 0.2751
/// as adults; babies scale both by 0.5 and keep the adult eye height.
pub const fn dimensions(baby: bool) -> (f32, f32, f32) {
    if baby {
        (0.4375, 0.475, 0.2751)
    } else {
        (0.875, 0.95, 0.2751)
    }
}

/// `AbstractNautilus.createAttributes`：15 血、移速 1.0、攻击 3、击退抗性 0.3；
/// 僵尸鹦鹉螺把移速改成 1.1。其余值都是 `Animal.createAnimalAttributes()` 的。
///
/// `AbstractNautilus.createAttributes`: 15 health, 1.0 speed, 3 attack and 0.3
/// knockback resistance; the zombie nautilus overrides speed with 1.1. All other
/// values come from `Animal.createAnimalAttributes()`.
pub fn attributes(kind: MobType) -> Attributes {
    let mut table = Attributes::cow();
    table.set_base(attribute::MAX_HEALTH, 15.0);
    table.set_base(
        attribute::MOVEMENT_SPEED,
        if kind == MobType::ZombieNautilus {
            1.100_000_023_841_858
        } else {
            1.0
        },
    );
    if let Some(entry) = table
        .entries
        .iter_mut()
        .find(|entry| entry.id == attribute::KNOCKBACK_RESISTANCE)
    {
        entry.base = 0.300_000_011_920_928_96;
    }
    if let Some(entry) = table
        .entries
        .iter_mut()
        .find(|entry| entry.id == attribute::ATTACK_DAMAGE)
    {
        entry.base = 3.0;
    } else {
        table
            .entries
            .push(Attribute::new(attribute::ATTACK_DAMAGE, 3.0));
    }
    table
}

/// `AbstractNautilus` 的构造函数把 `WATER` 的寻路代价设成 0。
///
/// The `AbstractNautilus` constructor sets the `WATER` pathfinding malus to zero.
pub fn malus() -> Malus {
    let mut malus = Malus::default();
    malus.set(PathType::Water, 0.0);
    malus
}

/// `AbstractNautilus` 的 `SmoothSwimmingMoveControl(85, 10, 0.011F, 0.0F, true)`。
///
/// `AbstractNautilus`'s `SmoothSwimmingMoveControl(85, 10, 0.011F, 0.0F, true)`.
pub(super) const MOVE_PARAMS: super::smooth_swim::MoveParams = super::smooth_swim::MoveParams {
    max_turn_x: 85.0,
    max_turn_y: 10.0,
    in_water_speed: 0.011,
    outside_water_speed: 0.0,
    apply_gravity: true,
};

/// `AbstractNautilus.travelInWater`：`moveRelative(getSpeed(), input)`、一次
/// `move(SELF, deltaMovement)`，再整体乘 0.9（没有下沉项）。
///
/// `AbstractNautilus.travelInWater`: `moveRelative(getSpeed(), input)`, one
/// `move(SELF, deltaMovement)`, then scale everything by 0.9, with no sink term.
pub fn travel_in_water(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx, input: Vec3) {
    living::move_relative(me, mob.speed, input);
    let movement = me.delta;
    motion::move_living(me, mob, ctx, movement);
    me.delta = me.delta.scale(0.9);
}

/// `AbstractNautilus` 自己那几个字段。
///
/// The own fields of `AbstractNautilus`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NautilusState {
    /// `dashCooldown`：冲刺冷却（40 刻）。
    ///
    /// `dashCooldown`: the 40-tick dash cooldown.
    pub dash_cooldown: i32,
    /// `playerJumpPendingScale`：客户端跳跃包留下的力度。
    ///
    /// `playerJumpPendingScale`: the strength left by the client jump packet.
    pub player_jump_pending_scale: f32,
    /// `DASH` 布尔元数据（index 20）。
    ///
    /// The `DASH` boolean metadata (index 20).
    pub dashing: bool,
    /// `ZombieNautilus.DATA_VARIANT_ID`（index 21）的注册表 ID。
    ///
    /// The registry id of `ZombieNautilus.DATA_VARIANT_ID` (index 21).
    pub variant: i32,
}

/// 僵尸鹦鹉螺的两种变种，`zombie_nautilus_variant` 注册表按名字排序。
///
/// The two `zombie_nautilus_variant` entries, sorted by registry name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variant {
    #[default]
    Temperate,
    Warm,
}

impl Variant {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Temperate => "minecraft:temperate",
            Self::Warm => "minecraft:warm",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Temperate, Self::Warm]
            .into_iter()
            .find(|variant| variant.name() == name)
    }

    pub const fn registry_id(self) -> i32 {
        match self {
            Self::Temperate => 0,
            Self::Warm => 1,
        }
    }

    pub fn from_id(id: i32) -> Self {
        match id {
            1 => Self::Warm,
            _ => Self::Temperate,
        }
    }
}

/// `finalizeSpawn` 按群系标签选变种：只有 `#spawns_coral_variant_zombie_nautilus`
/// （暖水海洋）是 warm，其余 temperate。
///
/// `finalizeSpawn` selects the variant from the biome: only
/// `#spawns_coral_variant_zombie_nautilus` (warm ocean) yields warm.
pub const CORAL_VARIANT_BIOME_TAG: &str = "minecraft:spawns_coral_variant_zombie_nautilus";

/// `AbstractNautilus.isFood`：没驯服或幼年用 `#nautilus_food`，驯服的成年用
/// `#nautilus_taming_items`。
///
/// `AbstractNautilus.isFood`: untamed or baby nautiluses eat `#nautilus_food`;
/// tame adults take `#nautilus_taming_items`.
pub fn is_food(mob: &Mob, item: i32) -> bool {
    if !mob.tame.is_tame() || mob.is_baby() {
        item_is_tagged(item, item_tags::NAUTILUS_FOOD)
    } else {
        item_is_tagged(item, item_tags::NAUTILUS_TAMING_ITEMS)
    }
}

/// `AbstractNautilus.usePlayerItem`：桶装鱼交回一个水桶。
///
/// `AbstractNautilus.usePlayerItem`: bucketed fish hand back a water bucket.
pub fn bucket_replacement(item: i32) -> Option<i32> {
    item_is_tagged(item, item_tags::NAUTILUS_BUCKET_FOOD).then_some(items::WATER_BUCKET)
}

/// `AbstractNautilus.checkNautilusSpawnRules`：y 在 `[sea - 25, sea - 5]` 之间、
/// 脚下是水、上方是水方块。
///
/// `AbstractNautilus.checkNautilusSpawnRules`: y within `[sea - 25, sea - 5]`,
/// water below and a water block above.
pub fn check_spawn_rules(world: &World, pos: BlockPos, sea_level: i32) -> bool {
    pos.y >= sea_level - 25
        && pos.y <= sea_level - 5
        && world
            .fluid_state(pos.below())
            .is_tagged(mc_bamboo_data::tags::fluid_tags::WATER)
        && world.block_state(pos.above()).is(blocks::WATER)
}

/// `AbstractNautilus.canUseSlot`：鞍和身体两格都要活着、成年、已驯服。
///
/// `AbstractNautilus.canUseSlot`: saddle and body both require a live, adult,
/// tame nautilus.
pub fn can_use_slot(mob: &Mob, slot: EquipmentSlot) -> bool {
    if matches!(slot, EquipmentSlot::Saddle | EquipmentSlot::Body) {
        return !mob.is_dead_or_dying() && !mob.is_baby() && mob.tame.is_tame();
    }
    super::equine::inventory::saddle_usable(mob)
}

/// `AbstractNautilus.getRiddenSpeed`：水里 0.0325×移速，陆上 0.02×移速。
///
/// `AbstractNautilus.getRiddenSpeed`: 0.0325 times the speed in water, 0.02 on land.
pub fn ridden_speed(mob: &Mob, in_water: bool) -> f32 {
    let scale = if in_water { 0.0325 } else { 0.02 };
    scale * mob.attributes.value(attribute::MOVEMENT_SPEED) as f32
}

/// `AbstractNautilus.executeRidersJump`：朝玩家视线方向加速，40 刻冷却、置 DASH。
///
/// `AbstractNautilus.executeRidersJump`: accelerate along the player's look vector,
/// start the 40-tick cooldown and set DASH.
pub fn execute_riders_jump(me: &mut Entity, mob: &mut Mob, scale: f32, pitch: f32, yaw: f32) {
    let strength = (if me.in_water { 1.2 } else { 0.5 })
        * scale
        * mob.attributes.value(attribute::MOVEMENT_SPEED) as f32;
    let look = Vec3::view_vector(yaw, pitch);
    me.delta = me.delta.add(look.scale(f64::from(strength)));
    mob.nautilus.dash_cooldown = 40;
    mob.nautilus.dashing = true;
    mob.needs_sync = true;
    living::play_sound(me, mob, dash_sound(mob.mob_type, me.in_water), 1.0, 1.0);
}

/// `AbstractNautilus.tick` 里的冲刺冷却与气泡（气泡是客户端粒子，这里只取随机数
/// 保持实体随机源一致）。
///
/// The dash cooldown and bubbles in `AbstractNautilus.tick`: bubbles are client
/// particles, so only the entity random draws are reproduced here.
pub fn tick(me: &mut Entity, mob: &mut Mob) {
    if mob.nautilus.dashing && mob.nautilus.dash_cooldown < 35 {
        mob.nautilus.dashing = false;
    }
    if mob.nautilus.dash_cooldown > 0 {
        mob.nautilus.dash_cooldown -= 1;
        if mob.nautilus.dash_cooldown == 0 {
            living::make_sound(me, mob, dash_ready_sound(mob.mob_type, me.in_water));
        }
    }
    if me.in_water {
        spawn_bubbles(me);
    }
}

/// `spawnBubbles`：一次 `nextFloat` 判概率，命中再取四次随机数。服务端只留取数。
///
/// `spawnBubbles`: one `nextFloat` for the probability and four more draws on a hit.
/// The server keeps only the draws, not the particle packet.
fn spawn_bubbles(me: &mut Entity) {
    let _speed = me.delta.length();
    let probability = (_speed * 2.0).clamp(0.150_000_005_960_464_48, 1.0);
    if me.random.next_float() as f64 >= probability {
        return;
    }
    let _spread = me.random.next_double() * 0.8 * (1.0 + _speed);
    let _ = me.random.next_float();
    let _ = me.random.next_float();
    let _ = me.random.next_float();
}

/// `AbstractNautilus.getDashSound` / `getDashReadySound` 的两套音效。
///
/// The water / land dash and dash-ready sounds of `AbstractNautilus`.
pub fn dash_sound(kind: MobType, under_water: bool) -> &'static str {
    let zombie = kind == MobType::ZombieNautilus;
    match (zombie, under_water) {
        (true, true) => "minecraft:entity.zombie_nautilus.dash",
        (true, false) => "minecraft:entity.zombie_nautilus.dash_land",
        (false, true) => "minecraft:entity.nautilus.dash",
        (false, false) => "minecraft:entity.nautilus.dash_land",
    }
}

/// 冲刺就绪的音效。
///
/// The dash-ready sound.
pub fn dash_ready_sound(kind: MobType, under_water: bool) -> &'static str {
    let zombie = kind == MobType::ZombieNautilus;
    match (zombie, under_water) {
        (true, true) => "minecraft:entity.zombie_nautilus.dash_ready",
        (true, false) => "minecraft:entity.zombie_nautilus.dash_ready_land",
        (false, true) => "minecraft:entity.nautilus.dash_ready",
        (false, false) => "minecraft:entity.nautilus.dash_ready_land",
    }
}

/// `getAmbientSound`：幼年 / 成年、水下 / 岸上各一套（僵尸版不分幼年）。
///
/// `getAmbientSound`: baby / adult and water / land variants; the zombie version
/// has no baby sounds.
pub fn ambient_voice(kind: MobType, baby: bool, under_water: bool) -> Option<&'static str> {
    if kind == MobType::ZombieNautilus {
        return Some(if under_water {
            "minecraft:entity.zombie_nautilus.ambient"
        } else {
            "minecraft:entity.zombie_nautilus.ambient_land"
        });
    }
    Some(match (baby, under_water) {
        (true, true) => "minecraft:entity.baby_nautilus.ambient",
        (true, false) => "minecraft:entity.baby_nautilus.ambient_land",
        (false, true) => "minecraft:entity.nautilus.ambient",
        (false, false) => "minecraft:entity.nautilus.ambient_land",
    })
}

/// `getHurtSound` / `getDeathSound`，`eat` 与 `swim` 也在这里按类型分派。
///
/// `getHurtSound` / `getDeathSound`, plus the eating and swim sounds.
pub fn voice(kind: MobType, baby: bool, under_water: bool, which: Voice) -> &'static str {
    if kind == MobType::ZombieNautilus {
        return match (under_water, which) {
            (true, Voice::Hurt) => "minecraft:entity.zombie_nautilus.hurt",
            (false, Voice::Hurt) => "minecraft:entity.zombie_nautilus.hurt_land",
            (true, Voice::Death) => "minecraft:entity.zombie_nautilus.death",
            (false, Voice::Death) => "minecraft:entity.zombie_nautilus.death_land",
            (_, Voice::Eat) => "minecraft:entity.zombie_nautilus.eat",
            (_, Voice::Swim) => "minecraft:entity.zombie_nautilus.swim",
            (_, Voice::Ambient) => "minecraft:entity.zombie_nautilus.ambient",
        };
    }
    match (baby, under_water, which) {
        (true, true, Voice::Hurt) => "minecraft:entity.baby_nautilus.hurt",
        (true, false, Voice::Hurt) => "minecraft:entity.baby_nautilus.hurt_land",
        (true, true, Voice::Death) => "minecraft:entity.baby_nautilus.death",
        (true, false, Voice::Death) => "minecraft:entity.baby_nautilus.death_land",
        (true, _, Voice::Eat) => "minecraft:entity.baby_nautilus.eat",
        (true, _, Voice::Swim) => "minecraft:entity.baby_nautilus.swim",
        (false, true, Voice::Hurt) => "minecraft:entity.nautilus.hurt",
        (false, false, Voice::Hurt) => "minecraft:entity.nautilus.hurt_land",
        (false, true, Voice::Death) => "minecraft:entity.nautilus.death",
        (false, false, Voice::Death) => "minecraft:entity.nautilus.death_land",
        (false, _, Voice::Eat) => "minecraft:entity.nautilus.eat",
        (false, _, Voice::Swim) => "minecraft:entity.nautilus.swim",
        (_, _, Voice::Ambient) => "minecraft:entity.nautilus.ambient",
    }
}

/// 声音用途。
///
/// Which sound to dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    Ambient,
    Hurt,
    Death,
    Eat,
    Swim,
}

/// `AbstractNautilus.getEquipSound`：鞍位是水下 / 岸上两套专用音效。
///
/// `AbstractNautilus.getEquipSound`: saddles use one of two dedicated sounds.
pub const fn saddle_equip_sound(under_water: bool) -> &'static str {
    if under_water {
        "minecraft:item.nautilus_saddle_underwater_equip"
    } else {
        "minecraft:item.nautilus_saddle_equip"
    }
}

/// `AbstractNautilus.tryToTame`：1/3 概率，事件 7 / 6，最后出吃食声。
///
/// `AbstractNautilus.tryToTame`: one in three, entity event 7 / 6, then the
/// eating sound.
pub fn try_to_tame(me: &mut Entity, mob: &mut Mob, player_uuid: [u8; 16]) {
    if me.random.next_int_bounded(3) == 0 {
        super::tame::tame(mob, player_uuid);
        mob.navigation.stop();
        mob.events.push(MobEvent::EntityEvent { event: 7 });
        mob.events.push(MobEvent::Tamed {
            player: player_uuid,
        });
    } else {
        mob.events.push(MobEvent::EntityEvent { event: 6 });
    }
    mob.events.push(MobEvent::Sound {
        sound: voice(mob.mob_type, mob.is_baby(), me.in_water, Voice::Eat),
        volume: 1.0,
        pitch: 1.0,
        position: me.position,
        hostile: false,
        seed: None,
    });
}

/// `AbstractNautilus.mobInteract`：幼年交给 `Animal`，潜行开物品栏，拿食物
/// 未驯服时驯服、受伤时喂 2 点血，最后上鞍 / 铠甲或让玩家骑上去。
///
/// `AbstractNautilus.mobInteract`: babies defer to `Animal`, sneaking opens the
/// inventory, food tames or heals, and the remaining stacks equip or mount.
pub fn interact(
    me: &mut Entity,
    mob: &mut Mob,
    req: &super::interact::MobInteractRequest,
) -> super::interact::MobInteraction {
    use super::interact::MobInteraction;

    // `AbstractNautilus.interact` 先标成持久。
    // `AbstractNautilus.interact` marks the mob persistent first.
    mob.persistence_required = true;
    if mob.is_baby() {
        return super::interact::animal_interact(me, mob, req);
    }
    if mob.tame.is_tame() && req.sneaking {
        return MobInteraction {
            open_inventory: super::equine::inventory::can_open(mob),
            result: crate::InteractionResult::Success,
            ..MobInteraction::pass()
        };
    }
    if !req.held.is_empty() {
        if !mob.tame.is_tame() && is_food(mob, req.held.item) {
            let replacement = bucket_replacement(req.held.item).map(|item| ItemStack::new(item, 1));
            try_to_tame(me, mob, req.player_uuid);
            return MobInteraction {
                result: crate::InteractionResult::SuccessServer,
                shrink_held: !req.creative && replacement.is_none(),
                replacement,
                tamed: mob.tame.is_tame(),
                ..MobInteraction::pass()
            };
        }
        if is_food(mob, req.held.item)
            && mob.health < mob.attributes.value(attribute::MAX_HEALTH) as f32
        {
            let replacement = bucket_replacement(req.held.item).map(|item| ItemStack::new(item, 1));
            mob.health = (mob.health + 2.0).min(mob.attributes.value(attribute::MAX_HEALTH) as f32);
            return MobInteraction {
                result: crate::InteractionResult::Success,
                shrink_held: !req.creative && replacement.is_none(),
                replacement,
                ..MobInteraction::pass()
            };
        }
        // `ItemStack.interactLivingEntity`：可装备组件接上鞍 / 铠甲。
        // `ItemStack.interactLivingEntity`: the equippable component fits saddles / armor.
        if let Some(equippable) = super::equine::equippable(&req.held)
            && super::equine::equip_on_target(me, mob, &equippable, &req.held)
        {
            return MobInteraction {
                result: crate::InteractionResult::Success,
                shrink_held: !req.creative,
                ..MobInteraction::pass()
            };
        }
    }
    if mob.tame.is_tame() && !req.sneaking && !is_food(mob, req.held.item) {
        return MobInteraction {
            result: crate::InteractionResult::Success,
            ride: true,
            ..MobInteraction::pass()
        };
    }
    super::interact::animal_interact(me, mob, req)
}

/// `AbstractNautilus.checkRestriction`：已驯服、没被骑、没拴绳时把家限制在
/// 当前位置；成年没鞍是 32 格，其余 16 格。
///
/// `AbstractNautilus.checkRestriction`: a tame, unridden, unleashed nautilus keeps
/// a home at its current position: 32 blocks for unsaddled adults, 16 otherwise.
pub fn check_restriction(me: &Entity, mob: &mut Mob) {
    if mob.is_vehicle() || !mob.tame.is_tame() {
        return;
    }
    let saddled = !mob.equipment.get(EquipmentSlot::Saddle).is_empty();
    let radius = if !mob.is_baby() && !saddled { 32 } else { 16 };
    if let Some((home, home_radius)) = mob.home {
        let delta = home.corner().subtract(me.block_position().corner());
        let within = delta.x * delta.x + delta.y * delta.y + delta.z * delta.z
            < f64::from(radius + 8) * f64::from(radius + 8);
        if within && home_radius == radius {
            return;
        }
    }
    mob.home = Some((me.block_position(), radius));
}

/// 空的物品栏（鹦鹉螺没有箱子格）：`getInventoryColumns()` 为 0。
///
/// An empty inventory: `getInventoryColumns()` is zero for nautiluses.
pub fn inventory_columns(_mob: &Mob) -> i32 {
    0
}

/// `NautilusInventoryMenu` 的两格只收鞍和能穿在身上的鹦鹉螺铠甲。
///
/// The two slots of `NautilusInventoryMenu` accept a saddle or body armor.
pub fn inventory_accepts(mob: &Mob, slot: EquipmentSlot, stack: &ItemStack) -> bool {
    inventory_accepts_with_flag(
        mob.mob_type,
        can_use_slot(mob, EquipmentSlot::Saddle),
        slot,
        stack,
    )
}

/// 游戏层菜单已经缓存了 `canUseSlot(SADDLE)`，用这份缓存判两格。
///
/// The game-layer menu caches `canUseSlot(SADDLE)`; use that for both slots.
pub fn inventory_accepts_with_flag(
    kind: MobType,
    saddle_usable: bool,
    slot: EquipmentSlot,
    stack: &ItemStack,
) -> bool {
    super::equine::inventory::accepts(kind, saddle_usable, slot, stack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::registries::entity_type as ids;

    /// 注册事实与属性：两个种类都能按名字和注册表 ID 找到，尺寸、属性照字节码。
    ///
    /// Registry facts and attributes: both kinds resolve by name and id, with the
    /// bytecode's dimensions and attributes.
    #[test]
    fn registry_facts_and_attributes() {
        assert_eq!(
            MobType::by_name("minecraft:nautilus"),
            Some(MobType::Nautilus)
        );
        assert_eq!(
            MobType::by_name("minecraft:zombie_nautilus"),
            Some(MobType::ZombieNautilus)
        );
        assert_eq!(MobType::Nautilus.entity_type(), ids::NAUTILUS);
        assert_eq!(MobType::ZombieNautilus.entity_type(), ids::ZOMBIE_NAUTILUS);
        assert_eq!(dimensions(false), (0.875, 0.95, 0.2751));
        assert_eq!(dimensions(true), (0.4375, 0.475, 0.2751));
        let adult = attributes(MobType::Nautilus);
        assert_eq!(adult.value(attribute::MAX_HEALTH), 15.0);
        assert_eq!(adult.value(attribute::MOVEMENT_SPEED), 1.0);
        assert_eq!(adult.value(attribute::ATTACK_DAMAGE), 3.0);
        assert_eq!(
            adult.value(attribute::KNOCKBACK_RESISTANCE),
            0.300_000_011_920_928_96
        );
        assert_eq!(
            attributes(MobType::ZombieNautilus).value(attribute::MOVEMENT_SPEED),
            1.100_000_023_841_858
        );
        assert_eq!(
            MobType::Nautilus.loot_table(),
            "minecraft:entities/nautilus"
        );
        assert_eq!(
            MobType::ZombieNautilus.loot_table(),
            "minecraft:entities/zombie_nautilus"
        );
        assert!(MobType::Nautilus.can_breathe_under_water());
        assert!(!MobType::Nautilus.is_monster());
        assert!(MobType::ZombieNautilus.is_monster());
        assert!(MobType::Nautilus.is_tamable());
        assert!(MobType::ZombieNautilus.is_tamable());
    }

    /// 变种按名字排序，食物标签与冲刺音效按字节码分派。
    ///
    /// Variants sort by name; food tags and dash sounds follow the bytecode.
    #[test]
    fn variants_food_and_sounds() {
        assert_eq!(Variant::default(), Variant::Temperate);
        assert_eq!(Variant::Warm.registry_id(), 1);
        assert_eq!(Variant::from_id(1), Variant::Warm);
        assert_eq!(Variant::from_name("minecraft:warm"), Some(Variant::Warm));
        assert_eq!(
            dash_sound(MobType::Nautilus, true),
            "minecraft:entity.nautilus.dash"
        );
        assert_eq!(
            dash_sound(MobType::ZombieNautilus, false),
            "minecraft:entity.zombie_nautilus.dash_land"
        );
        assert_eq!(
            ambient_voice(MobType::Nautilus, true, false),
            Some("minecraft:entity.baby_nautilus.ambient_land")
        );
        let mut mob = Mob::new(MobType::Nautilus);
        assert!(is_food(&mob, items::PUFFERFISH));
        assert!(!is_food(&mob, items::WHEAT));
        crate::entity::mob::tame::set_tame(&mut mob, true, false);
        assert!(is_food(&mob, items::PUFFERFISH));
        assert_eq!(
            bucket_replacement(items::COD_BUCKET),
            Some(items::WATER_BUCKET)
        );
        assert_eq!(bucket_replacement(items::COD), None);
    }

    /// 只有驯服、成年、活着的鹦鹉螺能上鞍和铠甲。
    ///
    /// Only a tame, adult, live nautilus can wear a saddle or armor.
    #[test]
    fn slots_require_a_tame_adult() {
        let mut mob = Mob::new(MobType::Nautilus);
        assert!(!can_use_slot(&mob, EquipmentSlot::Saddle));
        assert!(!can_use_slot(&mob, EquipmentSlot::Body));
        crate::entity::mob::tame::set_tame(&mut mob, true, false);
        assert!(can_use_slot(&mob, EquipmentSlot::Saddle));
        assert!(can_use_slot(&mob, EquipmentSlot::Body));
    }
}
