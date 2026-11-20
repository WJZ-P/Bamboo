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
    entity::arrow::{ArrowShot, Pickup},
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
    let Some(player) = region.players.get_mut(&id) else {
        return false;
    };
    let stack = player.inventory.held(hand).clone();
    if stack.is_empty() {
        return false;
    }
    let creative = player.game_mode == GameMode::Creative;
    match stack.item {
        items::BOW => {
            if !creative && find_projectile(player, &stack).is_none() {
                return false;
            }
            player.weapon_use = Some(WeaponUse::new(hand, stack.item));
            player.sync.metadata_dirty = true;
            true
        }
        items::CROSSBOW => {
            if is_charged(&stack) {
                shoot_crossbow(region, id, hand);
                return true;
            }
            if find_projectile(player, &stack).is_none() {
                return false;
            }
            player.weapon_use = Some(WeaponUse::new(hand, stack.item));
            player.sync.metadata_dirty = true;
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
    let (start_sound, end_sound) = charging_sounds(&stack);
    if play_start && let Some(name) = start_sound {
        play_sound(region, position, name, 0.5, 1.0);
    }
    if play_mid {
        play_sound(
            region,
            position,
            "minecraft:item.crossbow.loading_middle",
            0.5,
            1.0,
        );
    }
    if percent >= 1.0
        && !is_charged(&stack)
        && load_projectiles(region, id, hand)
        && let Some(name) = end_sound
    {
        let pitch = 1.0 / (level_random_float(region) * 0.5 + 1.0) + 0.2;
        play_sound(region, position, name, 1.0, pitch);
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
    let Some(ammo) = find_projectile(player, &stack) else {
        return false;
    };
    let drawn = draw_ammo(player, &stack, ammo);
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
    true
}

/// `Player.releaseUsingItem` 对弓和弩的处理。弩在这一步不发射，只停止装填。
///
/// `Player.releaseUsingItem` for bows and crossbows. A crossbow does not fire
/// here; it only stops loading.
pub(crate) fn release(region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let Some(using) = player.weapon_use.take() else {
        return;
    };
    player.sync.metadata_dirty = true;
    if using.item != items::BOW {
        return;
    }
    if player.inventory.held(using.hand).item != items::BOW {
        return;
    }
    let time_held = using.time_held();
    let power = get_power_for_time(time_held);
    if power < 0.1 {
        return;
    }
    release_bow(region, id, using.hand, power);
}

/// `BowItem.releaseUsing` 里的射击部分。
///
/// The shooting half of `BowItem.releaseUsing`.
fn release_bow(region: &mut Region, id: PlayerId, hand: Hand, power: f32) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    let weapon = player.inventory.held(hand).clone();
    let Some(ammo) = find_projectile(player, &weapon) else {
        return;
    };
    let drawn = draw_ammo(player, &weapon, ammo);
    let Some(projectile) = drawn.into_iter().next() else {
        return;
    };
    let Some(position) = player_eye(region, id) else {
        return;
    };
    let (yaw, pitch) = player_rotation(region, id);
    let dir = shoot_from_rotation(pitch, yaw, 0.0);
    let movement = player_movement(region, id);
    spawn_arrow(
        region,
        id,
        position,
        dir,
        power * 3.0,
        1.0,
        projectile,
        Some(weapon),
        power == 1.0,
        movement,
    );
    let pitch = 1.0 / (level_random_float(region) * 0.4 + 1.2) + power * 0.5;
    play_sound(
        region,
        Some(position),
        "minecraft:entity.arrow.shoot",
        1.0,
        pitch,
    );
    // 耐久在箭发射之后扣；创造模式不扣。
    crate::durability::run(region, id, hand, |region| {
        let (mut level, ticks) = region.block_level();
        let world = World::new(&mut level, ticks);
        mc_bamboo_blocks::durability::hurt_hand(&world, hand, 1);
    });
}

/// `CrossbowItem.performShooting`：组件里的弹药逐个射出。
///
/// `CrossbowItem.performShooting`: fires the charged projectiles one by one.
fn shoot_crossbow(region: &mut Region, id: PlayerId, hand: Hand) {
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
    let slot = player.inventory.hand_slot(hand);
    clear_component(
        &mut player.inventory.slots[slot],
        kinds::CHARGED_PROJECTILES,
    );
    let Some(position) = player_eye(region, id) else {
        return;
    };
    let (yaw, pitch) = player_rotation(region, id);
    let spread = projectile_spread(&weapon);
    let angle_step = if projectiles.len() == 1 {
        0.0
    } else {
        2.0 * spread / (projectiles.len() - 1) as f32
    };
    let angle_offset = ((projectiles.len() - 1) % 2) as f32 * angle_step / 2.0;
    let mut direction = 1.0_f32;
    for (index, projectile) in projectiles.into_iter().enumerate() {
        let angle = angle_offset + direction * (index.div_ceil(2) as f32) * angle_step;
        direction = -direction;
        let dir = if angle == 0.0 {
            view_vector(pitch, yaw)
        } else {
            rotated_view_vector(pitch, yaw, angle)
        };
        spawn_arrow(
            region,
            id,
            position,
            dir,
            3.15,
            1.0,
            projectile,
            Some(weapon.clone()),
            true,
            player_movement(region, id),
        );
        let pitch = shot_pitch(region, id, index);
        play_sound(
            region,
            Some(position),
            "minecraft:item.crossbow.shoot",
            1.0,
            pitch,
        );
        crate::durability::run(region, id, hand, |region| {
            let (mut level, ticks) = region.block_level();
            let world = World::new(&mut level, ticks);
            mc_bamboo_blocks::durability::hurt_hand(&world, hand, 1);
        });
        if region
            .players
            .get(&id)
            .is_some_and(|player| player.inventory.held(hand).is_empty())
        {
            break;
        }
    }
}

/// 生成一支玩家的箭并放进世界。
///
/// Spawns one player arrow and adds it to the world.
#[allow(clippy::too_many_arguments)]
fn spawn_arrow(
    region: &mut Region,
    id: PlayerId,
    eye: Vec3,
    direction: Vec3,
    power: f32,
    uncertainty: f32,
    ammo: ItemStack,
    weapon: Option<ItemStack>,
    critical: bool,
    movement: Option<Vec3>,
) {
    let intangible = is_intangible(&ammo);
    let pickup = if intangible {
        Pickup::CreativeOnly
    } else {
        Pickup::Allowed
    };
    let uuid = region
        .players
        .get(&id)
        .map(|player| *player.uuid.as_bytes());
    let shot = ArrowShot {
        position: eye,
        direction,
        power,
        uncertainty,
        source_movement: movement,
        owner: Some(id.0),
        owner_uuid: uuid,
        ammo,
        weapon,
        critical,
        pickup,
    };
    let (mut level, ticks) = region.block_level();
    let mut world = World::new(&mut level, ticks);
    world.spawn_arrow_shot(shot);
}

/// 找到这份武器能用的弹药：副手、主手，再按 `Inventory` 下标扫背包。
///
/// `Player.getProjectile`: off hand, main hand, then the inventory indices.
fn find_projectile(player: &Player, _weapon: &ItemStack) -> Option<(usize, ItemStack)> {
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
fn draw_ammo(player: &mut Player, weapon: &ItemStack, ammo: (usize, ItemStack)) -> Vec<ItemStack> {
    let count = projectile_count(weapon).max(1);
    let (slot, projectile) = ammo;
    let mut drawn = Vec::new();
    for index in 0..count {
        let source = projectile.clone();
        let force_infinite = index > 0;
        let used = use_ammo(player, weapon, source, slot, force_infinite);
        if !used.is_empty() {
            drawn.push(used);
        }
    }
    drawn
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

/// `BowItem.getPowerForTime`。
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
fn charging_sounds(stack: &ItemStack) -> (Option<&'static str>, Option<&'static str>) {
    let quick = enchantment_level(stack, "minecraft:quick_charge");
    let start = match quick {
        0 => "minecraft:item.crossbow.loading_start",
        1 => "minecraft:item.crossbow.quick_charge_1",
        2 => "minecraft:item.crossbow.quick_charge_2",
        _ => "minecraft:item.crossbow.quick_charge_3",
    };
    (Some(start), Some("minecraft:item.crossbow.loading_end"))
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
fn shoot_from_rotation(y_rot: f32, x_rot: f32, y_offset: f32) -> Vec3 {
    let xd = -mth::sin(f64::from(y_rot * (std::f32::consts::PI / 180.0)))
        * mth::cos(f64::from(x_rot * (std::f32::consts::PI / 180.0)));
    let yd = -mth::sin(f64::from(
        (x_rot + y_offset) * (std::f32::consts::PI / 180.0),
    ));
    let zd = mth::cos(f64::from(y_rot * (std::f32::consts::PI / 180.0)))
        * mth::cos(f64::from(x_rot * (std::f32::consts::PI / 180.0)));
    Vec3::new(f64::from(xd), f64::from(yd), f64::from(zd))
}

/// `CrossbowItem.getProjectileShotVector`：绕射手的上方向轴转 `angle` 度。
///
/// `getProjectileShotVector`: rotate the view vector about the shooter's up
/// vector by `angle` degrees, using JOML's quaternion arithmetic.
fn rotated_view_vector(x_rot: f32, y_rot: f32, angle: f32) -> Vec3 {
    let view = view_vector(x_rot, y_rot);
    let mut axis = cross(view, Vec3::new(0.0, 1.0, 0.0));
    if axis.length_sqr() <= 1.0e-7 {
        axis = cross(view, view_vector(x_rot - 90.0, y_rot));
    }
    let right = rotate_axis_angle(view, axis, std::f32::consts::FRAC_PI_2);
    rotate_axis_angle(view, right, angle * (std::f32::consts::PI / 180.0))
}

/// `Vec3.cross`。
///
/// `Vec3.cross`.
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

/// JOML `Quaternionf.setAngleAxis` + `rotate(Vector3f)`，`angle` 是弧度。
///
/// JOML `Quaternionf.setAngleAxis` followed by `rotate(Vector3f)`; `angle` is
/// in radians.
fn rotate_axis_angle(vector: Vec3, axis: Vec3, angle: f32) -> Vec3 {
    let half = f64::from(angle * 0.5);
    let s = half.sin() as f32;
    let c = half.cos() as f32;
    let axis = if (axis.length_sqr() - 1.0).abs() > f64::EPSILON {
        axis.normalize()
    } else {
        axis
    };
    let (x, y, z) = (axis.x as f32 * s, axis.y as f32 * s, axis.z as f32 * s);
    let w = c;
    let (fx, fy, fz) = (vector.x as f32, vector.y as f32, vector.z as f32);
    let ix = w * fx + y * fz - z * fy;
    let iy = w * fy + z * fx - x * fz;
    let iz = w * fz + x * fy - y * fx;
    let iw = -x * fx - y * fy - z * fz;
    Vec3::new(
        f64::from(ix * w + iw * -x + iy * -z - iz * -y),
        f64::from(iy * w + iw * -y + iz * -x - ix * -z),
        f64::from(iz * w + iw * -z + ix * -y - iy * -x),
    )
}

fn player_position(region: &Region, id: PlayerId) -> Option<Vec3> {
    let player = region.players.get(&id)?;
    Some(player.position())
}

/// `getEyeY() - 0.1` 的箭生成位置。
///
/// The spawn position, `getEyeY() - 0.1`.
fn player_eye(region: &Region, id: PlayerId) -> Option<Vec3> {
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

fn player_rotation(region: &Region, id: PlayerId) -> (f32, f32) {
    region
        .players
        .get(&id)
        .map_or((0.0, 0.0), |player| (player.yaw, player.pitch))
}

/// `ServerPlayer.lastKnownClientMovement`。
///
/// `ServerPlayer.lastKnownClientMovement`.
fn player_movement(region: &Region, id: PlayerId) -> Option<Vec3> {
    region.players.get(&id).map(|player| player.motion.delta)
}

fn level_random_float(region: &mut Region) -> f32 {
    let (mut level, ticks) = region.block_level();
    let world = World::new(&mut level, ticks);
    world.random().next_float()
}

fn play_sound(region: &mut Region, position: Option<Vec3>, name: &str, volume: f32, pitch: f32) {
    let Some(position) = position else {
        return;
    };
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
}
