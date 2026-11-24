//! 玩家的弓和弩：`BowItem` / `CrossbowItem` 的使用、松手和装填。
//!
//! 官方把「正在使用物品」放在 `LivingEntity.useItem` 里，松手时 `ItemStack.releaseUsing`。
//! 这里和吃东西一样存一个相对的剩余刻数，拆岛 / 并岛不需要平移时钟。
//!
//! 弹药查找、无限附魔和创造模式的数量规则照 `ProjectileWeaponItem.draw` /
//! `useAmmo`；箭实体本身由 `mc_bamboo_blocks` 的 `Entity::arrow_from_shot` 构造。
//!
//! Player bows and crossbows: use, release and loading, following vanilla
//! `BowItem` / `CrossbowItem` and `ProjectileWeaponItem`. The remaining use
//! ticks are relative, so island splits and merges need no clock rebasing.

use mc_bamboo_blocks::{
    World,
    entity::arrow::{ArrowShot, Pickup, TridentShot},
};
use mc_bamboo_core::{Hand, Vec3, mth};
use mc_bamboo_data::{items, registries::data_component_type as kinds, sounds, tags::item_tags};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, PatchEntry, defaults, stack::ItemStackTemplate},
    item::ItemStack,
    packet::play::GameMode,
};

use crate::{inbound::PlayerId, player::Player, region::Region};

/// `BowItem.MAX_DRAW_DURATION` / `ProjectileWeaponItem` 的 `useDuration`：72000。
///
/// `BowItem.getUseDuration` is 72000 for both weapons.
const USE_DURATION: i32 = 72_000;

/// 正在拉弓 / 装填。
///
/// One weapon being drawn or loaded; `remaining` is a relative countdown.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WeaponUse {
    pub hand: Hand,
    pub item: i32,
    pub remaining: i32,
    /// 已用刻数里跨过 20% / 50% 音效的标记，对应 `CrossbowItem` 的两个字段。
    ///
    /// The two sound flags of `CrossbowItem`.
    pub start_sound: bool,
    pub mid_sound: bool,
}

impl WeaponUse {
    fn new(hand: Hand, item: i32) -> Self {
        Self {
            hand,
            item,
            remaining: USE_DURATION,
            start_sound: false,
            mid_sound: false,
        }
    }

    /// `getTicksUsingItem`。
    ///
    /// Vanilla's `getTicksUsingItem`.
    fn time_held(&self) -> i32 {
        USE_DURATION - self.remaining
    }
}

/// `Item.use` 的一半：弓和弩开始使用，或者弩已经装好时直接射出。
/// 返回真表示这一下被武器消耗，不再走放置 / 方块交互。
///
/// Returns true when the weapon consumed the action, so placement and block
/// interaction are skipped.
pub(crate) fn start(region: &mut Region, id: PlayerId, hand: Hand) -> bool {
    let Some(player) = region.players.get(&id) else {
        return false;
    };
    let stack = player.inventory.held(hand).clone();
    if stack.is_empty() {
        return false;
    }
    let creative = player.game_mode == GameMode::Creative;
    let item = stack.item;
    match item {
        // `TridentItem.use`：快坏的物品和「有激流但不在水 / 雨里」都直接失败。
        // `TridentItem.use`: a nearly-broken item, or riptide outside water/rain, fails.
        items::TRIDENT => {
            if mc_bamboo_blocks::durability::next_damage_will_break(&stack) {
                return false;
            }
            if trident_spin_strength(&stack) > 0.0
                && !crate::player_environment::is_in_water_or_rain(region, id)
            {
                return false;
            }
            if let Some(player) = region.players.get_mut(&id)
                && player.weapon_use.is_none()
                && player.use_item.is_none()
            {
                player.weapon_use = Some(WeaponUse::new(hand, item));
                player.sync.metadata_dirty = true;
            }
            true
        }
        items::BOW => {
            if !creative
                && let Some(player) = region.players.get(&id)
                && find_projectile(player).is_none()
            {
                return false;
            }
            if let Some(player) = region.players.get_mut(&id)
                && player.weapon_use.is_none()
                && player.use_item.is_none()
            {
                player.weapon_use = Some(WeaponUse::new(hand, item));
                player.sync.metadata_dirty = true;
            }
            true
        }
        items::CROSSBOW => {
            if is_charged(&stack) {
                shoot_crossbow(region, id, hand);
                return true;
            }
            if let Some(player) = region.players.get(&id)
                && find_projectile(player).is_none()
            {
                return false;
            }
            if let Some(player) = region.players.get_mut(&id)
                && player.weapon_use.is_none()
                && player.use_item.is_none()
            {
                player.weapon_use = Some(WeaponUse::new(hand, item));
                player.sync.metadata_dirty = true;
            }
            true
        }
        _ => false,
    }
}

/// 每刻推进使用状态：倒计时和弩的装填。
///
/// `LivingEntity.updateUsingItem` plus `CrossbowItem.onUseTick`.
pub(crate) fn tick(region: &mut Region) {
    let ids: Vec<PlayerId> = region.players.keys().copied().collect();
    for id in ids {
        let Some(player) = region.players.get(&id) else {
            continue;
        };
        let Some(using) = player.weapon_use.as_ref() else {
            continue;
        };
        let (hand, item, time_held) = (using.hand, using.item, using.time_held());
        if player.inventory.held(hand).item != item {
            // 手里的武器换了，官方 `updateUsingItem` 会停掉。
            //
            // The held weapon changed; vanilla `updateUsingItem` stops.
            if let Some(player) = region.players.get_mut(&id) {
                player.weapon_use = None;
                player.sync.metadata_dirty = true;
            }
            continue;
        }
        if item == items::CROSSBOW {
            charge_tick(region, id, hand, time_held);
        }
        let Some(player) = region.players.get(&id) else {
            continue;
        };
        let Some(using) = player.weapon_use.as_ref() else {
            continue;
        };
        if using.remaining > 0
            && let Some(player) = region.players.get_mut(&id)
            && let Some(using) = player.weapon_use.as_mut()
        {
            using.remaining -= 1;
        }
    }
}

/// `CrossbowItem.onUseTick`：20% / 50% 的装填音效，满了把弹药装进组件。
///
/// `CrossbowItem.onUseTick`: the 20% / 50% sounds, and loading the ammo into
/// the component at 100%.
fn charge_tick(region: &mut Region, id: PlayerId, hand: Hand, time_held: i32) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    let stack = player.inventory.held(hand).clone();
    let charge = charge_duration(&stack);
    let percent = time_held as f32 / charge as f32;
    if percent < 0.2
        && let Some(player) = region.players.get_mut(&id)
        && let Some(using) = player.weapon_use.as_mut()
    {
        using.start_sound = false;
        using.mid_sound = false;
    }
    let (mut play_start, mut play_mid) = (false, false);
    if let Some(player) = region.players.get_mut(&id)
        && let Some(using) = player.weapon_use.as_mut()
    {
        if percent >= 0.2 && !using.start_sound {
            using.start_sound = true;
            play_start = true;
        }
        if percent >= 0.5 && !using.mid_sound {
            using.mid_sound = true;
            play_mid = true;
        }
    }
    let position = player_position(region, id);
    let (start_sound, mid_sound) = charging_sounds(&stack);
    if play_start {
        play_sound(region, position, start_sound, 0.5, 1.0);
    }
    if play_mid && let Some(name) = mid_sound {
        play_sound(region, position, name, 0.5, 1.0);
    }
    if percent >= 1.0 && !is_charged(&stack) && load_projectiles(region, id, hand) {
        let pitch = 1.0 / (level_random_float(region) * 0.5 + 1.0) + 0.2;
        play_sound(
            region,
            position,
            "minecraft:item.crossbow.loading_end",
            1.0,
            pitch,
        );
    }
}

/// `ProjectileWeaponItem.tryLoadProjectiles`。
///
/// Vanilla's `tryLoadProjectiles`.
fn load_projectiles(region: &mut Region, id: PlayerId, hand: Hand) -> bool {
    let Some(player) = region.players.get_mut(&id) else {
        return false;
    };
    let stack = player.inventory.held(hand).clone();
    let Some(ammo) = find_projectile(player) else {
        return false;
    };
    let (drawn, consumed) = draw_ammo(player, &stack, ammo);
    if drawn.is_empty() {
        return false;
    }
    let templates: Vec<ItemStackTemplate> = drawn.iter().filter_map(item_stack_template).collect();
    if templates.is_empty() {
        return false;
    }
    let slot = player.inventory.hand_slot(hand);
    let held = &mut player.inventory.slots[slot];
    set_component(
        held,
        kinds::CHARGED_PROJECTILES,
        ComponentValue::ChargedProjectiles(templates),
    );
    sync_slots(region, id, consumed.into_iter().chain([slot]));
    true
}

/// `Player.releaseUsingItem` 对弓和弩的处理。弩在这一步不发射，只停止装填。
///
/// `Player.releaseUsingItem` for bows and crossbows. A crossbow does not fire
/// here; it only stops loading.
pub(crate) fn release(region: &mut Region, id: PlayerId) {
    // 弩在停用前额外跑一次 onUseTick：刚好到充满阈值时松手仍然可以装填。
    // Crossbows run one more onUseTick before stopping, so release at the threshold can load.
    if let Some(player) = region.players.get(&id)
        && let Some(using) = player.weapon_use.as_ref()
        && using.item == items::CROSSBOW
        && player.inventory.held(using.hand).item == using.item
    {
        charge_tick(region, id, using.hand, using.time_held());
    }
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let Some(using) = player.weapon_use.take() else {
        return;
    };
    player.sync.metadata_dirty = true;
    let held_item = player.inventory.held(using.hand).item;
    let time_held = using.time_held();
    let hand = using.hand;
    if using.item == items::TRIDENT {
        if held_item == items::TRIDENT {
            release_trident(region, id, hand, time_held);
        }
        return;
    }
    if using.item != items::BOW {
        return;
    }
    if held_item != items::BOW {
        return;
    }
    let power = get_power_for_time(time_held);
    if power < 0.1 {
        return;
    }
    release_bow(region, id, hand, power);
}

/// `TridentItem.THROW_THRESHOLD_TIME`：蓄力不足 10 刻不扔。
const THROW_THRESHOLD_TIME: i32 = 10;
/// `TridentItem.PROJECTILE_SHOOT_POWER`。
const TRIDENT_SHOOT_POWER: f32 = 2.5;
/// `TridentItem.releaseUsing` 里 `startAutoSpinAttack(20, 8.0f, stack)`。
const AUTO_SPIN_ATTACK_TICKS: i32 = 20;
const AUTO_SPIN_ATTACK_DAMAGE: f32 = 8.0;

/// `EnchantmentHelper.getTridentSpinAttackStrength`：riptide 的
/// `trident_spin_attack_strength` 是 linear base 1.5 / +0.75，即 1.5 + 0.75×(L−1)。
///
/// `EnchantmentHelper.getTridentSpinAttackStrength`: riptide's
/// `trident_spin_attack_strength` is linear 1.5 / +0.75, i.e. 1.5 + 0.75×(L−1).
fn trident_spin_strength(stack: &ItemStack) -> f32 {
    let level = enchantment_level(stack, "minecraft:riptide");
    if level <= 0 {
        0.0
    } else {
        1.5 + 0.75 * (level - 1) as f32
    }
}

/// `EnchantmentHelper.pickHighestLevel(stack, TRIDENT_SOUND).orElse(TRIDENT_THROW)`：
/// 激流每级一个音效，没有激流时是普通投掷音。
///
/// `EnchantmentHelper.pickHighestLevel(stack, TRIDENT_SOUND).orElse(TRIDENT_THROW)`:
/// one sound per riptide level, else the ordinary throw sound.
fn trident_sound(stack: &ItemStack) -> &'static str {
    match enchantment_level(stack, "minecraft:riptide").clamp(0, 3) {
        1 => "minecraft:item.trident.riptide_1",
        2 => "minecraft:item.trident.riptide_2",
        3 => "minecraft:item.trident.riptide_3",
        _ => "minecraft:item.trident.throw",
    }
}

/// `TridentItem.releaseUsing`：蓄力门槛、激流 / 水雨与乘客判定、耐久、统计和音效，
/// 然后按有没有激流分投掷实体或自旋攻击。玩家位移由客户端上报，服务端不模拟。
///
/// `TridentItem.releaseUsing`: the draw threshold, the riptide water/rain and
/// passenger gates, durability, stat and sound, then either throws the entity or
/// starts the auto-spin attack. Player motion is client-authoritative and not
/// simulated here.
fn release_trident(region: &mut Region, id: PlayerId, hand: Hand, time_held: i32) {
    if time_held < THROW_THRESHOLD_TIME {
        return;
    }
    let (stack, creative, is_passenger) = {
        let Some(player) = region.players.get(&id) else {
            return;
        };
        (
            player.inventory.held(hand).clone(),
            player.game_mode == GameMode::Creative,
            player.riding.is_passenger(),
        )
    };
    let spin = trident_spin_strength(&stack);
    // 激流：必须在水或雨里，且不能是乘客。
    // Riptide: only usable in water or rain, and never as a passenger.
    if spin > 0.0 && (!crate::player_environment::is_in_water_or_rain(region, id) || is_passenger) {
        return;
    }
    // `ItemStack.nextDamageWillBreak()`。
    if mc_bamboo_blocks::durability::next_damage_will_break(&stack) {
        return;
    }
    let sound = trident_sound(&stack);
    // `player.awardStat(Stats.ITEM_USED.get(this))`。
    crate::item_use_stats::award(region, id, items::TRIDENT);
    // `stack.hurtWithoutBreaking(1, player)`：丢出 / 自旋前先扣耐久（不会坏）。
    hurt_trident(region, id, hand);
    // `consumeAndReturn` 的副本要带上刚扣的耐久，所以重读手里的堆。
    let weapon = region.players[&id].inventory.held(hand).clone();
    if spin == 0.0 {
        throw_trident(region, id, hand, &weapon, creative, sound);
    } else {
        riptide(region, id, &weapon, sound);
    }
}

/// `ItemStack.hurtWithoutBreaking(1, player)` 的手上物品版本。
fn hurt_trident(region: &mut Region, id: PlayerId, hand: Hand) {
    crate::durability::run(region, id, hand, |region| {
        let (mut level, ticks) = region.block_level();
        let world = World::new(&mut level, ticks);
        mc_bamboo_blocks::durability::hurt_hand_without_breaking(&world, hand, 1);
    });
}

/// 非激流：扔出 `ThrownTrident` 实体，生存模式消耗一个，创造模式不消耗且不可捡。
///
/// Non-riptide: throw a `ThrownTrident`, consume one in survival, keep it in
/// creative and mark it `CREATIVE_ONLY`.
fn throw_trident(
    region: &mut Region,
    id: PlayerId,
    hand: Hand,
    weapon: &ItemStack,
    creative: bool,
    sound: &str,
) {
    let Some(position) = player_eye(region, id) else {
        return;
    };
    let (yaw, pitch) = player_rotation(region, id);
    let (owner, owner_uuid, slot) = {
        let Some(player) = region.players.get(&id) else {
            return;
        };
        (
            Some(player.id.0),
            Some(*player.uuid.as_bytes()),
            player.inventory.hand_slot(hand),
        )
    };
    let source_movement = player_movement(region, id);
    // `ItemStack.consumeAndReturn(1, living)`：生存模式手里减一个，创造模式不减。
    if !creative {
        if let Some(player) = region.players.get_mut(&id) {
            let held = &mut player.inventory.slots[slot];
            held.count -= 1;
            if held.count <= 0 {
                *held = ItemStack::empty();
            }
        }
        sync_slots(region, id, [slot]);
    }
    let pickup = if creative {
        Pickup::CreativeOnly
    } else {
        Pickup::Allowed
    };
    let shot = TridentShot {
        position,
        direction: shoot_from_rotation(yaw, pitch, 0.0),
        power: TRIDENT_SHOOT_POWER,
        uncertainty: 1.0,
        source_movement,
        owner,
        owner_uuid,
        item: copy_with_count(weapon, 1),
        pickup,
    };
    {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        world.spawn_trident_shot(shot);
    }
    // `level.playSound(null, player, tridentSound, PLAYERS, 1, 1)`。
    play_sound(region, player_position(region, id), sound, 1.0, 1.0);
}

/// 激流：服务端只做官方服务端那一侧的事 —— 自旋攻击计时、生物标志位、碰实体
/// 伤害、音效和统计；推出 / 起跳的位移是客户端预测的，服务端不模拟。
///
/// Riptide: the server only does its own side — the spin-attack timer, the
/// living flag, contact damage, sound and stat. The push / hop motion is
/// client-predicted and not simulated here.
fn riptide(region: &mut Region, id: PlayerId, weapon: &ItemStack, sound: &str) {
    crate::player_auto_spin::start(
        region,
        id,
        AUTO_SPIN_ATTACK_TICKS,
        AUTO_SPIN_ATTACK_DAMAGE,
        weapon.clone(),
    );
    play_sound(region, player_position(region, id), sound, 1.0, 1.0);
}

/// `BowItem.releaseUsing` 里的射击部分。
///
/// The shooting half of `BowItem.releaseUsing`.
fn release_bow(region: &mut Region, id: PlayerId, hand: Hand, power: f32) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let weapon = player.inventory.held(hand).clone();
    let Some(ammo) = find_projectile(player) else {
        return;
    };
    let (drawn, consumed) = draw_ammo(player, &weapon, ammo);
    let Some(projectile) = drawn.into_iter().next() else {
        return;
    };
    let Some(position) = player_eye(region, id) else {
        return;
    };
    let (yaw, pitch) = player_rotation(region, id);
    let dir = shoot_from_rotation(yaw, pitch, 0.0);
    let mut shot = prepare_shot(
        &region.players[&id],
        position,
        dir,
        power * 3.0,
        projectile,
        weapon,
        power == 1.0,
    );
    shot.source_movement = player_movement(region, id);
    spawn_arrow(region, shot);
    hurt_weapon(region, id, hand);
    sync_slots(region, id, consumed);
    let pitch = 1.0 / (level_random_float(region) * 0.4 + 1.2) + power * 0.5;
    play_sound(
        region,
        player_position(region, id),
        "minecraft:entity.arrow.shoot",
        1.0,
        pitch,
    );
    crate::item_use_stats::award(region, id, items::BOW);
}

/// `CrossbowItem.performShooting`：组件里的弹药逐个射出。
///
/// `CrossbowItem.performShooting`: fires the charged projectiles one by one.
fn shoot_crossbow(region: &mut Region, id: PlayerId, hand: Hand) {
    // 官方 `ShotCrossbowTrigger` 在弩射出时触发，条件里的 `item` 是弩本身。
    crate::advancement_triggers::record(
        region,
        id,
        "minecraft:shot_crossbow",
        serde_json::json!({"item": "minecraft:crossbow"}),
    );
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let weapon = player.inventory.held(hand).clone();
    let Ok(patch) = ComponentPatch::from_opaque(&weapon) else {
        return;
    };
    let Some(ComponentValue::ChargedProjectiles(charged)) =
        defaults::resolve(weapon.item, &patch, kinds::CHARGED_PROJECTILES)
    else {
        return;
    };
    if charged.is_empty() {
        return;
    }
    let projectiles: Vec<ItemStack> = charged.iter().filter_map(item_stack).collect();
    if projectiles.is_empty() || projectiles.iter().any(|stack| !supported_projectile(stack)) {
        return;
    }
    let slot = player.inventory.hand_slot(hand);
    clear_component(
        &mut player.inventory.slots[slot],
        kinds::CHARGED_PROJECTILES,
    );
    let cleared = player.inventory.slots[slot].clone();
    let Some(position) = player_eye(region, id) else {
        return;
    };
    let player = &region.players[&id];
    let (yaw, pitch) = (player.head_yaw, player.pitch);
    let spread = projectile_spread(&weapon);
    let angle_step = if projectiles.len() == 1 {
        0.0
    } else {
        2.0 * spread / (projectiles.len() - 1) as f32
    };
    let angle_offset = (projectiles.len().saturating_sub(1) % 2) as f32 * angle_step / 2.0;
    let mut direction = 1.0_f32;
    for (index, projectile) in projectiles.into_iter().enumerate() {
        let angle = angle_offset + direction * (index.div_ceil(2) as f32) * angle_step;
        direction = -direction;
        let dir = if angle == 0.0 {
            view_vector(pitch, yaw)
        } else {
            rotated_view_vector(pitch, yaw, angle)
        };
        let shot = prepare_shot(
            &region.players[&id],
            position,
            dir,
            3.15,
            projectile,
            region.players[&id].inventory.held(hand).clone(),
            true,
        );
        spawn_arrow(region, shot);
        let pitch = shot_pitch(region, id, index);
        play_sound(
            region,
            player_position(region, id),
            "minecraft:item.crossbow.shoot",
            1.0,
            pitch,
        );
        hurt_weapon(region, id, hand);
        if region
            .players
            .get(&id)
            .is_some_and(|player| player.inventory.held(hand).is_empty())
        {
            break;
        }
    }
    if region.players[&id].inventory.slots[slot].matches(&cleared) {
        sync_slots(region, id, [slot]);
    }
    crate::item_use_stats::award(region, id, items::CROSSBOW);
}

fn hurt_weapon(region: &mut Region, id: PlayerId, hand: Hand) {
    crate::durability::run(region, id, hand, |region| {
        let (mut level, ticks) = region.block_level();
        let world = World::new(&mut level, ticks);
        mc_bamboo_blocks::durability::hurt_hand(&world, hand, 1);
    });
}

fn prepare_shot(
    shooter: &Player,
    position: Vec3,
    direction: Vec3,
    power: f32,
    ammo: ItemStack,
    weapon: ItemStack,
    critical: bool,
) -> ArrowShot {
    let pickup = if is_intangible(&ammo) {
        Pickup::CreativeOnly
    } else {
        Pickup::Allowed
    };
    ArrowShot {
        position,
        direction,
        power,
        uncertainty: 1.0,
        source_movement: None,
        owner: Some(shooter.id.0),
        owner_uuid: Some(*shooter.uuid.as_bytes()),
        ammo,
        weapon: Some(weapon),
        critical,
        pickup,
    }
}

fn spawn_arrow(region: &mut Region, shot: ArrowShot) {
    let shooter = shot.owner.map(crate::inbound::PlayerId);
    {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        world.spawn_arrow_shot(shot);
    }
    if let Some(shooter) = shooter {
        // 官方 `UsingItemTrigger`：玩家正在用弓 / 弩时每刻触发，箭射出这一刻也报一次。
        crate::advancement_triggers::record(
            region,
            shooter,
            "minecraft:using_item",
            serde_json::json!({"item": "minecraft:bow"}),
        );
    }
}

/// 找到这份武器能用的弹药：副手、主手，再按 `Inventory` 下标扫背包。
///
/// `Player.getProjectile`: off hand, main hand, then the inventory indices.
fn find_projectile(player: &Player) -> Option<(usize, ItemStack)> {
    let off = player.inventory.hand_slot(Hand::Off);
    let main = player.inventory.hand_slot(Hand::Main);
    for slot in [off, main] {
        let stack = &player.inventory.slots[slot];
        if supported_projectile(stack) {
            return Some((slot, stack.clone()));
        }
    }
    for index in 0..41 {
        let slot = container_slot(index);
        let stack = &player.inventory.slots[slot];
        if supported_projectile(stack) {
            return Some((slot, stack.clone()));
        }
    }
    // 创造模式没有实体弹药时 `getProjectile` 给一支普通箭。
    //
    // Creative mode hands out a plain arrow when no ammo exists.
    if player.game_mode == GameMode::Creative {
        return Some((usize::MAX, ItemStack::new(items::ARROW, 1)));
    }
    None
}

/// `Inventory` 的容器下标换算成菜单槽位。
///
/// Container index to menu slot.
fn container_slot(index: usize) -> usize {
    match index {
        0..=8 => crate::inventory::HOTBAR_START + index,
        9..=35 => index,
        36..=39 => 5 + (index - 36),
        _ => crate::inventory::OFF_HAND,
    }
}

fn supported_projectile(stack: &ItemStack) -> bool {
    !stack.is_empty() && mc_bamboo_blocks::state::item_is_tagged(stack.item, item_tags::ARROWS)
}

/// `ProjectileWeaponItem.draw` + `useAmmo`：返回这一发要用的弹药物品，并扣数量。
///
/// `ProjectileWeaponItem.draw` and `useAmmo`: the stacks to fire, with the real
/// ammo deducted.
fn draw_ammo(
    player: &mut Player,
    weapon: &ItemStack,
    ammo: (usize, ItemStack),
) -> (Vec<ItemStack>, Option<usize>) {
    let count = projectile_count(weapon).max(1);
    let (slot, projectile) = ammo;
    let mut drawn = Vec::new();
    let mut consumed = None;
    for index in 0..count {
        let source = projectile.clone();
        let force_infinite = index > 0;
        let used = use_ammo(player, weapon, source, slot, force_infinite);
        if !used.is_empty() {
            if !force_infinite
                && slot != usize::MAX
                && player.inventory.slots[slot].count != projectile.count
            {
                consumed = Some(slot);
            }
            drawn.push(used);
        }
    }
    (drawn, consumed)
}

/// `ProjectileWeaponItem.useAmmo`。
///
/// Vanilla's `useAmmo`.
fn use_ammo(
    player: &mut Player,
    weapon: &ItemStack,
    projectile: ItemStack,
    slot: usize,
    force_infinite: bool,
) -> ItemStack {
    let creative = player.game_mode == GameMode::Creative;
    let ammo_to_use = if !force_infinite && !creative {
        ammo_use(weapon, &projectile)
    } else {
        0
    };
    if ammo_to_use > projectile.count {
        return ItemStack::empty();
    }
    if ammo_to_use == 0 {
        let mut copy = copy_with_count(&projectile, 1);
        set_component(
            &mut copy,
            kinds::INTANGIBLE_PROJECTILE,
            ComponentValue::IntangibleProjectile,
        );
        return copy;
    }
    let used = copy_with_count(&projectile, ammo_to_use);
    if slot != usize::MAX {
        let held = &mut player.inventory.slots[slot];
        held.count -= ammo_to_use;
        if held.count <= 0 {
            *held = ItemStack::empty();
        }
    }
    used
}

/// `EnchantmentHelper.processAmmoUse`：无限附魔只对普通箭把消耗降到 0。
///
/// `EnchantmentHelper.processAmmoUse`: Infinity only zeroes plain arrows.
fn ammo_use(weapon: &ItemStack, projectile: &ItemStack) -> i32 {
    if projectile.item == items::ARROW && enchantment_level(weapon, "minecraft:infinity") > 0 {
        0
    } else {
        1
    }
}

/// `EnchantmentHelper.processProjectileCount`：多重射击每级 +2。
///
/// `processProjectileCount`: Multishot adds 2 per level.
fn projectile_count(weapon: &ItemStack) -> usize {
    let multishot = enchantment_level(weapon, "minecraft:multishot").max(0) as usize;
    (1 + multishot * 2).max(1)
}

/// `EnchantmentHelper.processProjectileSpread`：多重射击每级 +10 度。
///
/// `processProjectileSpread`: Multishot adds 10 degrees per level.
fn projectile_spread(weapon: &ItemStack) -> f32 {
    enchantment_level(weapon, "minecraft:multishot").max(0) as f32 * 10.0
}

/// 按拉弓刻数计算蓄力。
///
/// Computes the draw strength using `BowItem.getPowerForTime`.
pub(crate) fn get_power_for_time(time_held: i32) -> f32 {
    let mut power = time_held as f32 / 20.0;
    power = (power * power + power * 2.0) / 3.0;
    if power > 1.0 { 1.0 } else { power }
}

/// `CrossbowItem.getChargeDuration`：1.25 秒，快速装填每级减 0.25 秒。
///
/// `CrossbowItem.getChargeDuration`: 1.25 seconds, minus 0.25 per Quick Charge level.
fn charge_duration(crossbow: &ItemStack) -> i32 {
    let quick = enchantment_level(crossbow, "minecraft:quick_charge").max(0) as f32;
    let seconds = 1.25 + (-0.25 * quick);
    mth::floor(f64::from(seconds) * 20.0)
}

fn is_charged(stack: &ItemStack) -> bool {
    matches!(
        defaults::resolve(stack.item, &stack.patch(), kinds::CHARGED_PROJECTILES),
        Some(ComponentValue::ChargedProjectiles(projectiles)) if !projectiles.is_empty()
    )
}

fn is_intangible(stack: &ItemStack) -> bool {
    matches!(
        defaults::resolve(stack.item, &stack.patch(), kinds::INTANGIBLE_PROJECTILE),
        Some(ComponentValue::IntangibleProjectile)
    )
}

/// `CrossbowItem.getChargingSounds`：快速装填换开始音效。
///
/// `getChargingSounds`: Quick Charge swaps the start sound.
fn charging_sounds(stack: &ItemStack) -> (&'static str, Option<&'static str>) {
    let quick = enchantment_level(stack, "minecraft:quick_charge");
    let start = match quick {
        0 => "minecraft:item.crossbow.loading_start",
        1 => "minecraft:item.crossbow.quick_charge_1",
        2 => "minecraft:item.crossbow.quick_charge_2",
        _ => "minecraft:item.crossbow.quick_charge_3",
    };
    (
        start,
        (quick == 0).then_some("minecraft:item.crossbow.loading_middle"),
    )
}

/// `CrossbowItem.getShotPitch`：第一发 1.0，其余按奇偶取随机音高。
///
/// `getShotPitch`: 1.0 for the first shot, a random pitch afterwards.
fn shot_pitch(region: &mut Region, id: PlayerId, index: usize) -> f32 {
    if index == 0 {
        return 1.0;
    }
    let high = index % 2 == 1;
    let range = if high { 0.63 } else { 0.43 };
    let Some(player) = region.players.get_mut(&id) else {
        return 1.0;
    };
    1.0 / (player.random.next_float() * 0.5 + 1.8) + range
}

/// `EnchantmentHelper.getPiercingCount` 之外，武器上某个附魔的等级。
///
/// One enchantment's level on a weapon.
fn enchantment_level(stack: &ItemStack, enchantment: &str) -> i32 {
    mc_bamboo_blocks::entity::arrow::enchantment_level(stack, enchantment)
}

fn item_stack_template(stack: &ItemStack) -> Option<ItemStackTemplate> {
    let components = ComponentPatch::from_opaque(stack).ok()?;
    Some(ItemStackTemplate {
        item: stack.item,
        count: stack.count,
        components,
    })
}

fn item_stack(template: &ItemStackTemplate) -> Option<ItemStack> {
    let (added, removed) = template.components.to_opaque();
    Some(ItemStack {
        item: template.item,
        count: template.count,
        added,
        removed,
    })
}

fn copy_with_count(stack: &ItemStack, count: i32) -> ItemStack {
    mc_bamboo_blocks::container::copy_with_count(stack, count)
}

fn set_component(stack: &mut ItemStack, kind: i32, value: ComponentValue) {
    let mut patch = ComponentPatch::from_opaque(stack).unwrap_or_default();
    patch.put(kind, PatchEntry::Set(value));
    let (added, removed) = patch.to_opaque();
    stack.added = added;
    stack.removed = removed;
}

fn clear_component(stack: &mut ItemStack, kind: i32) {
    let mut patch = ComponentPatch::from_opaque(stack).unwrap_or_default();
    patch.remove(kind);
    let (added, removed) = patch.to_opaque();
    stack.added = added;
    stack.removed = removed;
}

/// `Entity.calculateViewVector`，全部按 `f32` 算。
///
/// `Entity.calculateViewVector`, all in `f32`.
fn view_vector(x_rot: f32, y_rot: f32) -> Vec3 {
    let x = x_rot * (std::f32::consts::PI / 180.0);
    let y = -y_rot * (std::f32::consts::PI / 180.0);
    let y_cos = mth::cos(f64::from(y));
    let y_sin = mth::sin(f64::from(y));
    let x_cos = mth::cos(f64::from(x));
    let x_sin = mth::sin(f64::from(x));
    Vec3::new(
        f64::from(y_sin * x_cos),
        f64::from(-x_sin),
        f64::from(y_cos * x_cos),
    )
}

/// `Projectile.shootFromRotation` 的方向部分。`y_offset` 是弓的 0 度。
///
/// The direction half of `Projectile.shootFromRotation`.
pub(crate) fn shoot_from_rotation(y_rot: f32, x_rot: f32, y_offset: f32) -> Vec3 {
    let xd = -mth::sin(f64::from(y_rot * (std::f32::consts::PI / 180.0)))
        * mth::cos(f64::from(x_rot * (std::f32::consts::PI / 180.0)));
    let yd = -mth::sin(f64::from(
        (x_rot + y_offset) * (std::f32::consts::PI / 180.0),
    ));
    let zd = mth::cos(f64::from(y_rot * (std::f32::consts::PI / 180.0)))
        * mth::cos(f64::from(x_rot * (std::f32::consts::PI / 180.0)));
    Vec3::new(f64::from(xd), f64::from(yd), f64::from(zd))
}

/// 玩家弩按 `CrossbowItem.shootProjectile` 绕视角的上方向转动。
///
/// Vanilla's untargeted `CrossbowItem.shootProjectile` rotates about the view's up vector.
fn rotated_view_vector(pitch: f32, yaw: f32, angle: f32) -> Vec3 {
    let view = view_vector(pitch, yaw);
    let axis = view_vector(pitch - 90.0, yaw);
    let half = f64::from(angle * (std::f32::consts::PI / 180.0)) * 0.5;
    let s = half.sin();
    let x = (axis.x * s) as f32;
    let y = (axis.y * s) as f32;
    let z = (axis.z * s) as f32;
    let w = (1.0 - s * s).sqrt() as f32;
    transform_quaternion(view, x, y, z, w)
}

/// JOML 默认关闭融合乘加；各系数和向量分量都按单精度舍入。
///
/// JOML defaults to unfused multiply-add; every coefficient and vector component is single precision.
fn transform_quaternion(v: Vec3, x: f32, y: f32, z: f32, w: f32) -> Vec3 {
    let (xx, yy, zz, ww) = (x * x, y * y, z * z, w * w);
    let (xy, xz, yz, xw, yw, zw) = (x * y, x * z, y * z, x * w, y * w, z * w);
    let inv = 1.0 / (xx + yy + zz + ww);
    let (vx, vy, vz) = (v.x as f32, v.y as f32, v.z as f32);
    Vec3::new(
        f64::from(
            (xx - yy - zz + ww) * inv * vx
                + (2.0 * (xy - zw) * inv * vy + 2.0 * (xz + yw) * inv * vz),
        ),
        f64::from(
            2.0 * (xy + zw) * inv * vx
                + ((yy - xx - zz + ww) * inv * vy + 2.0 * (yz - xw) * inv * vz),
        ),
        f64::from(
            2.0 * (xz - yw) * inv * vx
                + (2.0 * (yz + xw) * inv * vy + (zz - xx - yy + ww) * inv * vz),
        ),
    )
}

pub(crate) fn player_position(region: &Region, id: PlayerId) -> Option<Vec3> {
    let player = region.players.get(&id)?;
    Some(player.position())
}

/// `getEyeY() - 0.1` 的箭生成位置。
///
/// The spawn position, `getEyeY() - 0.1F`.
pub(crate) fn player_eye(region: &Region, id: PlayerId) -> Option<Vec3> {
    let player = region.players.get(&id)?;
    let position = player.position();
    // 官方 `getEyeHeight` 是 `float`，加到 double 的 y 上再减 `0.1F`。
    //
    // Vanilla's `getEyeHeight` is a `float`, widened into the double y before
    // subtracting `0.1F`.
    Some(Vec3::new(
        position.x,
        position.y + f64::from(player.eye_height() as f32) - f64::from(0.1_f32),
        position.z,
    ))
}

pub(crate) fn player_rotation(region: &Region, id: PlayerId) -> (f32, f32) {
    region
        .players
        .get(&id)
        .map_or((0.0, 0.0), |player| (player.yaw, player.pitch))
}

/// `ServerPlayer.lastKnownClientMovement`。
///
/// `ServerPlayer.lastKnownClientMovement`.
pub(crate) fn player_movement(region: &Region, id: PlayerId) -> Option<Vec3> {
    region.players.get(&id).map(|player| {
        let movement = player.motion.known_movement;
        Vec3::new(
            movement.x,
            if player.on_ground { 0.0 } else { movement.y },
            movement.z,
        )
    })
}

pub(crate) fn level_random_float(region: &mut Region) -> f32 {
    let (mut level, ticks) = region.block_level();
    let world = World::new(&mut level, ticks);
    world.random().next_float()
}

/// 未开菜单时同步变动的玩家物品栏槽位；否则由菜单同步。
///
/// Sync changed player inventory slots unless an open menu will sync them.
pub(crate) fn sync_slots(
    region: &mut Region,
    id: PlayerId,
    slots: impl IntoIterator<Item = usize>,
) {
    if region.menus.contains_key(&id) {
        return;
    }
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    for slot in slots {
        crate::inventory::send_slot(player, &mut region.links, slot);
    }
}

/// `level.playSound(null, x, y, z, sound, PLAYERS, volume, pitch)`：发给附近所有玩家，包括射手自己。
///
/// 临时 `World` 只把声音记进自己的效果列表，丢掉之前要取出来发出去，否则玩家听不到。
///
/// `level.playSound(null, x, y, z, sound, PLAYERS, volume, pitch)`: sent to every nearby
/// player, the shooter included.
///
/// The temporary `World` only records the sound in its effect list, so the effects must
/// be drained and sent before it is dropped, or no player hears it.
pub(crate) fn play_sound(
    region: &mut Region,
    position: Option<Vec3>,
    name: &str,
    volume: f32,
    pitch: f32,
) {
    let Some(position) = position else {
        return;
    };
    let effects = {
        let (mut level, ticks) = region.block_level();
        let mut world = World::new(&mut level, ticks);
        world.play_sound(
            position,
            sounds::sound_event(name),
            sounds::PLAYERS,
            volume,
            pitch,
            false,
        );
        world.drain_effects()
    };
    crate::interaction::send_effects(region, None, &effects);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_data::registries::{SYNCED_REGISTRIES, data_component_type};
    use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};

    fn enchanted(name: &str, level: i32) -> ItemStack {
        let mut stack = ItemStack::new(items::TRIDENT, 1);
        let id = SYNCED_REGISTRIES
            .iter()
            .find(|registry| registry.name == "minecraft:enchantment")
            .unwrap()
            .entries
            .iter()
            .position(|entry| *entry == name)
            .unwrap() as i32;
        let mut patch = ComponentPatch::new();
        patch.put(
            data_component_type::ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(vec![(id, level)])),
        );
        let (added, removed) = patch.to_opaque();
        stack.added = added;
        stack.removed = removed;
        stack
    }

    #[test]
    fn riptide_strength_and_sound_follow_the_26_3_effect_data() {
        let plain = ItemStack::new(items::TRIDENT, 1);
        assert_eq!(trident_spin_strength(&plain), 0.0);
        assert_eq!(trident_sound(&plain), "minecraft:item.trident.throw");
        for (level, strength, sound) in [
            (1, 1.5, "minecraft:item.trident.riptide_1"),
            (2, 2.25, "minecraft:item.trident.riptide_2"),
            (3, 3.0, "minecraft:item.trident.riptide_3"),
        ] {
            let stack = enchanted("minecraft:riptide", level);
            assert_eq!(trident_spin_strength(&stack), strength);
            assert_eq!(trident_sound(&stack), sound);
        }
    }
}
