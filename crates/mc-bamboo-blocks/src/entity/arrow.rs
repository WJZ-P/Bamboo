use mc_bamboo_core::{Aabb, Axis, BlockPos, Direction, Vec3, mth};
use mc_bamboo_data::{items as item, sounds};
use mc_bamboo_protocol::item::ItemStack;

use super::ctx::{BlockView, Command, EntityCtx, PlayerView};
use super::mob::{DamageKind, DamageSource};
use super::{Entity, EntityKind, EntityUid, KindPublic};
use crate::{BlockHit, BlockState, CollisionContext, World};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Pickup {
    #[default]
    Disallowed,
    Allowed,
    CreativeOnly,
}

/// 箭的两种官方类：`Arrow` 和 `SpectralArrow`。
///
/// 两者共用 `AbstractArrow` 的飞行、命中和持久化，只有命中后的效果和实体类型不同。
///
/// The two official classes: `Arrow` and `SpectralArrow`. They share
/// `AbstractArrow` flight, impacts and persistence; only the post-hit effect
/// and the entity type differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArrowVariant {
    #[default]
    Normal,
    /// `SpectralArrow`，`duration` 是发光刻数（默认 200）。
    ///
    /// `SpectralArrow`; `duration` is the glowing tick count (default 200).
    Spectral { duration: i32 },
    /// `ThrownTrident`。官方继承 `AbstractArrow`，所以这里复用箭的全部飞行、命中和插地；
    /// 只把实体类型、伤害、反弹和元数据换掉。
    ///
    /// `ThrownTrident`. Vanilla extends `AbstractArrow`, so it reuses the whole
    /// flight, impact and in-ground logic; only the entity type, damage,
    /// deflection and metadata differ.
    Trident,
}

impl ArrowVariant {
    /// 注册表里的实体类型。
    ///
    /// The registry entity type.
    pub const fn entity_type(self) -> i32 {
        match self {
            Self::Normal => mc_bamboo_data::registries::entity_type::ARROW,
            Self::Spectral { .. } => mc_bamboo_data::registries::entity_type::SPECTRAL_ARROW,
            Self::Trident => mc_bamboo_data::registries::entity_type::TRIDENT,
        }
    }

    /// 是不是三叉戟。
    ///
    /// Whether this is a thrown trident.
    pub const fn is_trident(self) -> bool {
        matches!(self, Self::Trident)
    }

    /// 按弹药物品决定箭的种类。`SpectralArrowItem.createArrow` 造 `SpectralArrow`。
    ///
    /// The variant chosen by the ammo item; `SpectralArrowItem.createArrow`
    /// builds a `SpectralArrow`.
    pub fn of_ammo(ammo: &ItemStack) -> Self {
        if ammo.item == item::SPECTRAL_ARROW {
            Self::Spectral { duration: 200 }
        } else {
            Self::Normal
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowPublic {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub pitch: f32,
    pub critical: bool,
    pub pierce: i8,
    pub in_ground: bool,
    pub shake_time: u8,
    pub pickup: Pickup,
    pub on_fire: bool,
    pub no_gravity: bool,
    /// `AbstractArrow.ID_FLAGS` 的 noPhysics 位（忠诚返回中的三叉戟）。
    ///
    /// The noPhysics bit of `AbstractArrow.ID_FLAGS` (a returning loyal trident).
    pub no_physics: bool,
    pub needs_sync: bool,
    pub sync_velocity: bool,
    pub variant: ArrowVariant,
    pub color: i32,
    /// `ThrownTrident.ID_LOYALTY`：忠诚等级（0 表示没有）。
    ///
    /// `ThrownTrident.ID_LOYALTY`: the loyalty level (0 means none).
    pub loyalty: u8,
    /// `ThrownTrident.ID_FOIL`：附魔光效。
    ///
    /// `ThrownTrident.ID_FOIL`: the enchantment glint.
    pub foil: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arrow {
    pub uuid: [u8; 16],
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    pub pitch: f32,
    pub no_gravity: bool,
    pub base_damage: f64,
    pub critical: bool,
    pub pierce: i8,
    pub in_ground: bool,
    pub shake_time: u8,
    pub life: i32,
    pub pickup: Pickup,
    pub pickup_item: ItemStack,
    pub weapon: Option<ItemStack>,
    pub last_state: Option<BlockState>,
    pub sound: u16,
    pub left_owner: bool,
    pub variant: ArrowVariant,
    /// `Projectile.leftOwnerChecked`：同一刻内只查一次，每刻末尾由 `Projectile.tick` 清掉。
    ///
    /// `Projectile.leftOwnerChecked`: at most one check per tick, cleared by
    /// `Projectile.tick` at the end of every tick.
    left_owner_checked: bool,
    pub has_been_shot: bool,
    needs_sync: bool,
    sync_velocity: bool,
    pierced: Vec<EntityUid>,
    /// `AbstractArrow.inGroundTime`：连续插地的刻数，不进存档。
    ///
    /// `AbstractArrow.inGroundTime`: consecutive ticks in the ground; not saved.
    pub in_ground_time: i32,
    /// 这一刻 `Arrow.tick` 广播了实体事件 0（药水褪色的粒子），等同步层取走。
    ///
    /// `Arrow.tick` broadcast entity event 0 (the potion puff) this tick; the
    /// sync layer takes it.
    potion_puff: bool,
    /// `ThrownTrident.dealtDamage`：命中过实体后置真，之后不再命中实体。
    ///
    /// `ThrownTrident.dealtDamage`: set after an entity hit; no more entity hits.
    pub dealt_damage: bool,
    /// `ThrownTrident.ID_LOYALTY`：从物品算出的忠诚等级（0..=127）。
    ///
    /// `ThrownTrident.ID_LOYALTY`: loyalty derived from the item (0..=127).
    pub loyalty: u8,
    /// `ThrownTrident.ID_FOIL`：附魔光效。
    ///
    /// `ThrownTrident.ID_FOIL`: the enchantment glint.
    pub foil: bool,
    /// `ThrownTrident` 返回主人时的 `noPhysics`，存档不写。
    ///
    /// `ThrownTrident`'s `noPhysics` while returning to its owner; not saved.
    pub no_physics: bool,
    /// `ThrownTrident.clientSideReturnTridentTickCount`：返回计数，第一刻播返回音效。
    ///
    /// `ThrownTrident.clientSideReturnTridentTickCount`: counts return ticks and
    /// gates the first return sound; not saved.
    pub client_side_return_trident_tick_count: i32,
}

impl Default for Arrow {
    fn default() -> Self {
        Self {
            uuid: [0; 16],
            owner: None,
            owner_uuid: None,
            pitch: 0.0,
            no_gravity: false,
            base_damage: 2.0,
            critical: false,
            pierce: 0,
            in_ground: false,
            shake_time: 0,
            life: 0,
            pickup: Pickup::Disallowed,
            pickup_item: ItemStack::new(item::ARROW, 1),
            weapon: None,
            last_state: None,
            sound: arrow_hit_sound(),
            left_owner: false,
            left_owner_checked: false,
            has_been_shot: false,
            needs_sync: false,
            sync_velocity: false,
            pierced: Vec::new(),
            variant: ArrowVariant::Normal,
            in_ground_time: 0,
            potion_puff: false,
            dealt_damage: false,
            loyalty: 0,
            foil: false,
            no_physics: false,
            client_side_return_trident_tick_count: 0,
        }
    }
}

impl Arrow {
    pub(crate) fn public(&self, remaining_fire_ticks: i32) -> ArrowPublic {
        ArrowPublic {
            uuid: self.uuid,
            owner: self.owner,
            pitch: self.pitch,
            critical: self.critical,
            pierce: self.pierce,
            in_ground: self.in_ground,
            shake_time: self.shake_time,
            pickup: self.pickup,
            on_fire: remaining_fire_ticks > 0,
            no_gravity: self.no_gravity,
            no_physics: self.no_physics,
            needs_sync: self.needs_sync,
            sync_velocity: self.sync_velocity,
            variant: self.variant,
            color: potion_color(self),
            loyalty: self.loyalty,
            foil: self.foil,
        }
    }
}

pub(crate) fn arrow_hit_sound() -> u16 {
    sounds::sound_event("minecraft:entity.arrow.hit")
}

/// `ThrownTrident.getDefaultHitGroundSoundEvent()`。
pub(crate) fn trident_hit_ground_sound() -> u16 {
    sounds::sound_event("minecraft:item.trident.hit_ground")
}

/// `ThrownTrident.onHitEntity` 的命中音效。
pub(crate) fn trident_hit_sound() -> u16 {
    sounds::sound_event("minecraft:item.trident.hit")
}

/// `ItemStack.hasFoil()`：解析好的 `enchantments` 组件非空。
///
/// `ItemStack.hasFoil()`: the resolved `enchantments` component is non-empty.
pub(crate) fn has_foil(stack: &ItemStack) -> bool {
    use mc_bamboo_data::registries::data_component_type::ENCHANTMENTS;
    use mc_bamboo_protocol::components::{ComponentValue, defaults};

    matches!(
        defaults::resolve(stack.item, &stack.patch(), ENCHANTMENTS),
        Some(ComponentValue::Enchantments(levels)) if !levels.is_empty()
    )
}

/// `ThrownTrident.getLoyaltyFromItem`：`Mth.clamp(trident_return_acceleration, 0, 127)`，
/// 26.3 的 `loyalty` 效果组件值就是附魔等级，所以直接取等级。
///
/// `ThrownTrident.getLoyaltyFromItem`: clamp of `trident_return_acceleration` to
/// 0..=127; the 26.3 loyalty effect value equals the enchantment level.
pub fn loyalty_from_item(stack: &ItemStack) -> u8 {
    enchantment_level(stack, "minecraft:loyalty").clamp(0, 127) as u8
}

/// `TridentItem.BASE_DAMAGE`，也是 `ThrownTrident.onHitEntity` 的基础伤害。
pub const TRIDENT_BASE_DAMAGE: f32 = 8.0;

impl Entity {
    pub fn arrow(position: Vec3) -> Self {
        Self::new(EntityKind::Arrow(Box::default()), position)
    }

    /// 一支刚造出来、还没射出的三叉戟。
    ///
    /// A freshly constructed trident, not yet thrown.
    pub fn trident(position: Vec3) -> Self {
        let arrow = Arrow {
            variant: ArrowVariant::Trident,
            ..Arrow::default()
        };
        Self::new(EntityKind::Arrow(Box::new(arrow)), position)
    }

    /// 一次扔三叉戟的输入：玩家松手和溺尸远程攻击都走这里。
    ///
    /// The inputs to one trident throw; both a player's release and a drowned's
    /// ranged attack use it.
    pub fn trident_from_shot(shot: TridentShot) -> Self {
        let mut entity = Self::configured_trident(&shot);
        entity.shoot_arrow_with_movement(
            shot.direction,
            shot.power,
            shot.uncertainty,
            shot.source_movement,
        );
        entity
    }

    /// 只设字段、不射。三叉戟的附魔从物品堆读，物品堆同时是拾取物和 `getWeaponItem()`。
    ///
    /// Configure only. A trident reads its enchantments from the stack, which is
    /// both the pickup item and `getWeaponItem()`.
    pub fn configured_trident(shot: &TridentShot) -> Self {
        let mut entity = Self::trident(shot.position);
        let EntityKind::Arrow(arrow) = &mut entity.kind else {
            unreachable!()
        };
        arrow.pickup = shot.pickup;
        arrow.pickup_item = shot.item.clone();
        arrow.weapon = None;
        arrow.owner = shot.owner;
        arrow.owner_uuid = shot.owner_uuid;
        arrow.base_damage = f64::from(TRIDENT_BASE_DAMAGE);
        arrow.loyalty = loyalty_from_item(&shot.item);
        arrow.foil = has_foil(&shot.item);
        arrow.sound = trident_hit_ground_sound();
        entity
    }

    /// 一次射箭的输入。玩家弓、弩和发射器都走这里，区别只在弹药、武器和拾取方式。
    ///
    /// One shot's inputs. The player bow, crossbow and dispenser all use this;
    /// only ammo, weapon and pickup mode differ.
    pub fn arrow_from_shot(shot: ArrowShot) -> Self {
        let mut entity = Self::configured_arrow(&shot);
        entity.shoot_arrow_with_movement(
            shot.direction,
            shot.power,
            shot.uncertainty,
            shot.source_movement,
        );
        entity
    }

    /// 只设字段、不射。调用方先用 [`EntityStore::prepare`] 拿种子和 UUID，再射。
    ///
    /// Configure only. The caller lets [`EntityStore::prepare`] draw the seed and
    /// UUID first, then shoots.
    pub fn configured_arrow(shot: &ArrowShot) -> Self {
        let mut entity = Self::arrow(shot.position);
        let EntityKind::Arrow(arrow) = &mut entity.kind else {
            unreachable!()
        };
        arrow.variant = ArrowVariant::of_ammo(&shot.ammo);
        arrow.pickup = shot.pickup;
        arrow.pickup_item = shot.ammo.clone();
        arrow.owner = shot.owner;
        arrow.owner_uuid = shot.owner_uuid;
        arrow.weapon = shot.weapon.clone();
        arrow.base_damage = 2.0;
        if let Some(weapon) = &arrow.weapon {
            if weapon.item == mc_bamboo_data::items::CROSSBOW {
                arrow.sound = mc_bamboo_data::sounds::sound_event("minecraft:item.crossbow.hit");
            }
            arrow.pierce = enchantment_level(weapon, "minecraft:piercing") as i8;
            if enchantment_level(weapon, "minecraft:flame") > 0 {
                entity.remaining_fire_ticks = 2000;
            }
        }
        arrow.critical = shot.critical;
        entity
    }

    /// Projectile.shoot：先归一化，再按 XYZ 顺序抽散布，最后乘速度。
    ///
    /// Projectile.shoot normalizes first, draws spread in XYZ order, then scales.
    pub fn shoot_arrow(&mut self, direction: Vec3, power: f32, uncertainty: f32) {
        self.shoot_arrow_with_movement(direction, power, uncertainty, None);
    }

    /// `Projectile.shootFromRotation` 的最后一步：再加上射手的已知移动。
    ///
    /// The last step of `Projectile.shootFromRotation`: add the shooter's known movement.
    pub fn shoot_arrow_with_movement(
        &mut self,
        direction: Vec3,
        power: f32,
        uncertainty: f32,
        source_movement: Option<Vec3>,
    ) {
        let EntityKind::Arrow(arrow) = &mut self.kind else {
            return;
        };
        let spread = 0.0172275 * f64::from(uncertainty);
        self.delta = direction
            .normalize()
            .add(Vec3::new(
                self.random.triangle(0.0, spread),
                self.random.triangle(0.0, spread),
                self.random.triangle(0.0, spread),
            ))
            .scale(f64::from(power));
        let (yaw, pitch) = rotation(self.delta);
        self.y_rot = yaw;
        arrow.pitch = pitch;
        if let Some(movement) = source_movement {
            self.delta = self.delta.add(movement);
        }
        arrow.life = 0;
        arrow.needs_sync = true;
    }
}

/// [`Entity::arrow_from_shot`] 的输入。
///
/// The inputs to [`Entity::arrow_from_shot`].
#[derive(Debug, Clone, PartialEq)]
pub struct ArrowShot {
    pub position: Vec3,
    /// 射手视线方向，`shootFromRotation` 的 `(x, y, z)`；发射器传朝向的步长。
    ///
    /// The shooter's view vector; the dispenser passes the facing step.
    pub direction: Vec3,
    pub power: f32,
    pub uncertainty: f32,
    /// `ServerPlayer.lastKnownClientMovement`；发射器为 `None`。
    ///
    /// `ServerPlayer.lastKnownClientMovement`; `None` for a dispenser.
    pub source_movement: Option<Vec3>,
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    /// 弹药物品，原样写进 `pickup`（含药水组件）。
    ///
    /// The ammo stack, stored as-is for pickup (including potion components).
    pub ammo: ItemStack,
    /// 弓或弩；发射器为 `None`。
    ///
    /// The bow or crossbow; `None` for a dispenser.
    pub weapon: Option<ItemStack>,
    pub critical: bool,
    pub pickup: Pickup,
}

/// [`Entity::trident_from_shot`] 的输入。
///
/// The inputs to [`Entity::trident_from_shot`].
#[derive(Debug, Clone, PartialEq)]
pub struct TridentShot {
    pub position: Vec3,
    pub direction: Vec3,
    pub power: f32,
    pub uncertainty: f32,
    pub source_movement: Option<Vec3>,
    pub owner: Option<EntityUid>,
    pub owner_uuid: Option<[u8; 16]>,
    /// 扔出的三叉戟物品堆（带附魔组件）；既是拾取物也是附魔来源。
    ///
    /// The thrown trident stack (with enchantments); both the pickup item and
    /// the source of its enchantment effects.
    pub item: ItemStack,
    /// 玩家扔出是 `Allowed` / `CreativeOnly`，怪物扔出是 `Disallowed`。
    ///
    /// `Allowed`/`CreativeOnly` for players, `Disallowed` for mobs.
    pub pickup: Pickup,
}

pub(super) fn rotation(movement: Vec3) -> (f32, f32) {
    let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
    let deg = |r| (r * f64::from(180.0_f32 / std::f32::consts::PI)) as f32;
    (
        deg(mth::atan2(movement.x, movement.z)),
        deg(mth::atan2(movement.y, horizontal)),
    )
}

pub(super) fn lerp_rotation(from: f32, to: f32) -> f32 {
    let mut from = from;
    while to - from < -180.0 {
        from -= 360.0;
    }
    while to - from >= 180.0 {
        from += 360.0;
    }
    from + 0.2 * (to - from)
}

fn at_point(pos: Vec3, radius: f64) -> Aabb {
    Aabb::new(pos.x, pos.y, pos.z, pos.x, pos.y, pos.z).inflated(radius)
}

pub(crate) fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    // 三叉戟的忠诚返回在 `super.tick()` 之前，可能改速度；这之后取 movement
    // 才和官方 `AbstractArrow.tick()` 开头读到的 `getDeltaMovement()` 一致。
    // A loyal trident's return runs before `super.tick()` and may change the
    // velocity, so `movement` is read afterwards to match vanilla's read.
    if trident_tick(me, ctx) {
        return;
    }
    let movement = me.delta;
    if !tick_or_check_ground(me, ctx) {
        fly(me, ctx, movement);
    }
}

/// `getOwner()` 解析出来的、忠诚返回要用到的主人信息。
///
/// The owner facts a returning trident needs, resolved from `getOwner()`.
struct ReturnOwner {
    eye: Vec3,
    width: f64,
    alive: bool,
    spectator: bool,
    is_player: bool,
}

fn return_owner(ctx: &dyn EntityCtx, uid: EntityUid) -> Option<ReturnOwner> {
    if let Some(player) = ctx.player(uid) {
        return Some(ReturnOwner {
            eye: Vec3::new(
                player.position.x,
                player.position.y + f64::from(player.eye_height),
                player.position.z,
            ),
            width: 0.6,
            alive: player.alive,
            spectator: player.spectator,
            is_player: true,
        });
    }
    let public = ctx.other(uid)?;
    let (width, height) = public.kind.dimensions();
    Some(ReturnOwner {
        eye: Vec3::new(
            public.position.x,
            public.position.y + f64::from(height) * 0.85,
            public.position.z,
        ),
        width: f64::from(width),
        alive: !public.removed,
        spectator: false,
        is_player: false,
    })
}

/// `ThrownTrident.tick` 里 `super.tick()` 之前的那一半：插地超过 4 刻置
/// `dealtDamage`，然后按忠诚度飞回主人。返回 `true` 表示官方那句
/// `discard(); return;`（非玩家主人已经很近），这一刻不再跑 `AbstractArrow.tick()`。
///
/// The half of `ThrownTrident.tick` before `super.tick()`: mark `dealtDamage`
/// after four ground ticks, then fly back to the owner by loyalty. Returns
/// `true` for vanilla's `discard(); return;` (a close non-player owner), which
/// skips the rest of `AbstractArrow.tick()` this tick.
fn trident_tick(me: &mut Entity, ctx: &mut dyn EntityCtx) -> bool {
    {
        let EntityKind::Arrow(arrow) = &mut me.kind else {
            return false;
        };
        if !arrow.variant.is_trident() {
            return false;
        }
        if arrow.in_ground_time > 4 {
            arrow.dealt_damage = true;
        }
    }
    let (loyalty, dealt_damage, no_physics, pickup, owner, owner_uuid, return_ticks, pickup_item) = {
        let EntityKind::Arrow(arrow) = &me.kind else {
            return false;
        };
        (
            arrow.loyalty,
            arrow.dealt_damage,
            arrow.no_physics,
            arrow.pickup,
            arrow.owner,
            arrow.owner_uuid,
            arrow.client_side_return_trident_tick_count,
            arrow.pickup_item.clone(),
        )
    };
    // `loyalty > 0 && (dealtDamage || isNoPhysics()) && owner != null`：只有命中过
    // （或已在返回途中置了 noPhysics）才飞回；刚投出、还没命中时继续正常飞行。
    // `loyalty > 0 && (dealtDamage || isNoPhysics()) && owner != null`: fly back
    // only after a hit, or once noPhysics marks the return as already in progress.
    if loyalty == 0 || (!dealt_damage && !no_physics) {
        return false;
    }
    let owner_uid = owner.or_else(|| owner_uuid.and_then(|uuid| resolve_owner(ctx, uuid)));
    let Some(uid) = owner_uid else {
        return false;
    };
    let Some(owner) = return_owner(ctx, uid) else {
        return false;
    };
    // `!isAcceptibleReturnOwner()`：主人死了或旁观 -> 掉成物品并丢弃。
    // `!isAcceptibleReturnOwner()`: owner dead or spectating -> drop and discard.
    if !owner.alive || (owner.is_player && owner.spectator) {
        if pickup == Pickup::Allowed {
            // 官方 `spawnAtLocation(getPickupItem(), 0.1f)`：掉落物抬高 0.1 格。
            // Vanilla `spawnAtLocation(getPickupItem(), 0.1f)`: drop 0.1 above.
            ctx.emit(Command::DropTridentItem { stack: pickup_item }, me);
        }
        me.removed = true;
        return false;
    }
    // 非玩家主人：进到 `bbWidth + 1` 以内就丢弃并提前返回。
    // Non-player owner: within `bbWidth + 1` -> discard and return.
    if !owner.is_player && me.position.distance_to_sqr(owner.eye).sqrt() < owner.width + 1.0 {
        me.removed = true;
        return true;
    }
    // 返回主人：`setPosRaw` 的 y 偏移、`delta * 0.95 + toOwner.normalize() * 0.05*L`。
    // Return: the `setPosRaw` y offset and `delta * 0.95 + normalize(toOwner) * 0.05*L`.
    let to_owner = Vec3::new(
        owner.eye.x - me.position.x,
        owner.eye.y - me.position.y,
        owner.eye.z - me.position.z,
    );
    let loyalty_f = f64::from(loyalty);
    let new_y = me.position.y + to_owner.y * 0.015 * loyalty_f;
    let speed = 0.05 * loyalty_f;
    let delta = me.delta.scale(0.95).add(to_owner.normalize().scale(speed));
    me.position = Vec3::new(me.position.x, new_y, me.position.z);
    me.delta = delta;
    if let EntityKind::Arrow(arrow) = &mut me.kind {
        arrow.no_physics = true;
        arrow.needs_sync = true;
        arrow.client_side_return_trident_tick_count = return_ticks.saturating_add(1);
    }
    if return_ticks == 0 {
        ctx.emit(
            Command::TridentReturnSound {
                position: me.position,
            },
            me,
        );
    }
    false
}

fn tick_or_check_ground(me: &mut Entity, ctx: &mut dyn EntityCtx) -> bool {
    let blocks = ctx.blocks();
    let position = me.position;
    let block_pos = me.block_position();
    let current_state = blocks.block_state(block_pos);
    let collision = me.collision_context();
    let surrounding = at_point(position, 0.06);
    let top = BlockPos::containing(Vec3::new(position.x, me.bounding_box().max_y, position.z));
    let EntityKind::Arrow(arrow) = &mut me.kind else {
        return true;
    };
    arrow.needs_sync = false;
    arrow.sync_velocity = false;
    let nearby_blocks = blocks.block_collisions(surrounding, collision);
    if nearby_blocks
        .iter()
        .filter(|(pos, _)| *pos == block_pos)
        .any(|(_, shapes)| {
            shapes
                .iter()
                .any(|bb| bb.contains(position.x, position.y, position.z))
        })
    {
        me.delta = Vec3::default();
        // `setInGround(true)` 只在值变化时触发 `onSyncedDataUpdated`，那里才设震动。
        // `setInGround(true)` only fires `onSyncedDataUpdated` on a real change, which
        // is where the shake is set.
        let was_in_ground = arrow.in_ground;
        arrow.in_ground = true;
        if !was_in_ground && !me.first_tick && arrow.shake_time == 0 {
            arrow.shake_time = 7;
        }
    }
    arrow.shake_time = arrow.shake_time.saturating_sub(1);
    if me.in_water || blocks.is_raining_at(block_pos) || blocks.is_raining_at(top) {
        me.remaining_fire_ticks = me.remaining_fire_ticks.min(0);
    }
    if arrow.in_ground {
        if arrow.last_state != Some(current_state) && nearby_blocks.is_empty() {
            arrow.in_ground = false;
            me.delta = Vec3::new(
                me.delta.x * f64::from(me.random.next_float() * 0.2),
                me.delta.y * f64::from(me.random.next_float() * 0.2),
                me.delta.z * f64::from(me.random.next_float() * 0.2),
            );
            arrow.life = 0;
        } else {
            arrow.life += 1;
            // `ThrownTrident.tickDespawn`：可捡回且带忠诚的三叉戟不会到时消失。
            // `ThrownTrident.tickDespawn`: an allowed-pickup trident with loyalty
            // never despawns on the age timer.
            let loyalty_returns =
                arrow.variant.is_trident() && arrow.pickup == Pickup::Allowed && arrow.loyalty > 0;
            me.removed = arrow.life >= 1200 && !loyalty_returns;
        }
        arrow.in_ground_time += 1;
        fade_exposed_potion(arrow);
        if !me.removed
            && let Some(pitch) = projectile_travel_effects(me, blocks, position)
        {
            ctx.emit(Command::ProjectileExtinguished { pitch }, me);
        }
        true
    } else {
        arrow.in_ground_time = 0;
        false
    }
}

/// `Arrow.tick` 服务端那半：药水箭连续插地 600 刻后广播事件 0，拾取物换成普通箭。
/// 官方在 `super.tick()` 之后判，这里放在插地分支末尾；这一刻落地（`inGroundTime`
/// 是 0）或被拔出（不再插地）都不触发，所以只有插地分支需要判。
///
/// The server half of `Arrow.tick`: after 600 consecutive ticks in the ground a
/// tipped arrow broadcasts event 0 and its pickup becomes a plain arrow. Vanilla
/// checks after `super.tick()`; only the in-ground branch can satisfy it, since a
/// landing tick has `inGroundTime == 0` and a dislodged arrow is no longer in ground.
fn fade_exposed_potion(arrow: &mut Arrow) {
    if arrow.in_ground
        && arrow.variant == ArrowVariant::Normal
        && arrow.in_ground_time >= 600
        && has_potion_contents(arrow)
    {
        arrow.potion_puff = true;
        arrow.pickup_item = ItemStack::new(item::ARROW, 1);
    }
}

/// 取走这一刻的药水褪色事件。
///
/// Take this tick's potion puff event.
pub(crate) fn take_potion_puff(arrow: &mut Arrow) -> bool {
    std::mem::take(&mut arrow.potion_puff)
}

/// `!getPotionContents().equals(PotionContents.EMPTY)`。
fn has_potion_contents(arrow: &Arrow) -> bool {
    use mc_bamboo_data::registries::data_component_type::POTION_CONTENTS;
    use mc_bamboo_protocol::components::{ComponentValue, defaults};

    let patch = arrow.pickup_item.patch();
    matches!(
        defaults::resolve(arrow.pickup_item.item, &patch, POTION_CONTENTS),
        Some(ComponentValue::PotionContents {
            potion,
            custom_color,
            custom_effects,
            custom_name,
        }) if potion.is_some()
            || custom_color.is_some()
            || !custom_effects.is_empty()
            || custom_name.is_some()
    )
}

fn fly(me: &mut Entity, ctx: &mut dyn EntityCtx, movement: Vec3) {
    let blocks = ctx.blocks();
    let position = me.position;
    let collision = me.collision_context();
    // `AbstractArrow.tick()` 里 `flag = !isNoPhysics()`：noPhysics 时跳过
    // clip / 命中 / 碰撞与重力，但仍要按「取负速度」更新朝向、
    // `setPos(pos + delta)`，再照常跑 0.99 空气阻力与 `Projectile.tick()`
    // 收尾（`finish_flight` 里对 noPhysics 跳过重力）。忠诚返回的三叉戟走这里。
    // `AbstractArrow.tick()`'s `flag = !isNoPhysics()`: with no physics the
    // clip / hit / collision / gravity sections are skipped, but rotation
    // (from the negated delta) and `setPos(pos + delta)` still happen, followed
    // by the usual 0.99 air drag and `Projectile.tick()` tail (`finish_flight`
    // skips gravity for noPhysics). A returning loyal trident takes this path.
    if matches!(&me.kind, EntityKind::Arrow(arrow) if arrow.no_physics) {
        let (yaw, pitch) = rotation(Vec3::new(-movement.x, movement.y, -movement.z));
        me.y_rot = lerp_rotation(me.y_rot, yaw);
        me.position = position.add(movement);
        if let EntityKind::Arrow(arrow) = &mut me.kind {
            arrow.pitch = lerp_rotation(arrow.pitch, pitch);
        }
        finish_flight(me, blocks);
        return;
    }
    let EntityKind::Arrow(arrow) = &mut me.kind else {
        return;
    };
    if me.in_water {
        // `AbstractArrow.getWaterInertia()` 是 0.6，`ThrownTrident` 覆盖成 0.99。
        // `AbstractArrow.getWaterInertia()` is 0.6; `ThrownTrident` overrides it to 0.99.
        let inertia = if arrow.variant.is_trident() {
            0.99_f32
        } else {
            0.6_f32
        };
        me.delta = me.delta.scale(f64::from(inertia));
    }
    let (yaw, pitch) = rotation(movement);
    me.y_rot = lerp_rotation(me.y_rot, yaw);
    arrow.pitch = lerp_rotation(arrow.pitch, pitch);
    let owner = arrow
        .owner
        .or_else(|| arrow.owner_uuid.and_then(|uuid| resolve_owner(ctx, uuid)));
    let area = sweep_area(me);
    let mut owner_in_range = owner
        .and_then(|uid| ctx.other(uid))
        .is_some_and(|entity| entity.bounding_box().intersects(area));
    blocks.for_each_player(&mut |p| {
        if p.alive && !p.spectator && player_box(p).intersects(area) {
            owner_in_range |= owner == Some(p.uid);
        }
    });
    let EntityKind::Arrow(arrow) = &mut me.kind else {
        unreachable!()
    };
    arrow.owner = owner;
    // `Projectile.checkLeftOwner` 只在第一次调用时更新，之后 `leftOwnerChecked` 挡住。
    // `Projectile.checkLeftOwner` only updates on its first call; `leftOwnerChecked`
    // blocks every later one within the same tick.
    if !arrow.left_owner && !arrow.left_owner_checked {
        arrow.left_owner = !owner_in_range;
        arrow.left_owner_checked = true;
    }
    let end = position.add(movement);
    let block_hit = clip_including_border(blocks, position, end, collision);
    let end = block_hit.map_or(end, |hit| hit.location);
    let hits = find_targets(me, ctx, end);
    if hits.is_empty() && block_hit.is_none() {
        me.position = end;
        if let Some(pitch) = projectile_travel_effects(me, blocks, position) {
            ctx.emit(Command::ProjectileExtinguished { pitch }, me);
        }
        finish_flight(me, ctx.blocks());
    } else {
        ctx.emit(
            Command::ArrowImpact {
                block: block_hit,
                hits,
                end,
            },
            me,
        );
    }
}

fn sweep_area(me: &Entity) -> Aabb {
    me.bounding_box()
        .expanded_towards(me.delta.x, me.delta.y, me.delta.z)
        .inflated(1.0)
}

fn find_targets(me: &Entity, ctx: &dyn EntityCtx, end: Vec3) -> Vec<ArrowEntityHit> {
    let EntityKind::Arrow(arrow) = &me.kind else {
        return Vec::new();
    };
    // `ThrownTrident.findHitEntity`：命中过一次实体（或插地太久）后不再查实体。
    // `ThrownTrident.findHitEntity`: after one entity hit (or long in-ground) it
    // stops looking for entities.
    if arrow.variant.is_trident() && arrow.dealt_damage {
        return Vec::new();
    }
    let blocks = ctx.blocks();
    let area = sweep_area(me);
    let position = me.position;
    let collision = me.collision_context();
    let mut hits = Vec::new();
    let margin = f64::from(((me.tick_count - 2) as f32 / 20.0).clamp(0.0, 0.3));
    // `ThrownTrident.findHitEntities` 把单次 `findHitEntity` 包成单元素集合，
    // 后者走 `ProjectileUtil.getEntityHitResult(..., computeMargin)` —— 总是直接
    // 裁剪按 margin 膨胀后的包围盒。普通箭走 `findHitEntities` 的
    // `getManyEntityHitResult`（先精确盒、未命中才膨胀盒）。三叉戟命中后 REVERSE
    // 反弹且留在场上，膨胀盒接触点会逐刻可见，必须按官方各自路径区分。
    //
    // `ThrownTrident.findHitEntities` wraps the single `findHitEntity`, which
    // runs `ProjectileUtil.getEntityHitResult(..., computeMargin)` — it always
    // clips the margin-inflated box. Ordinary arrows use `findHitEntities`'s
    // `getManyEntityHitResult` (exact box first, inflated box only as a fallback).
    // A deflected trident stays in the world, so its inflated contact is visible
    // tick by tick; the two paths must match vanilla separately.
    let trident = arrow.variant.is_trident();
    let mut consider = |uid, feet, bb: Aabb, player| {
        if (arrow.left_owner || Some(uid) != arrow.owner) && !arrow.pierced.contains(&uid) {
            let contact = if trident {
                clip_entity(bb.inflated(margin), position, end)
            } else {
                clip_entity_with_margin(blocks, bb, position, end, collision, margin)
            };
            if let Some(contact) = contact {
                hits.push((
                    position.distance_to_sqr(feet),
                    ArrowEntityHit {
                        uid,
                        contact,
                        player,
                    },
                ));
            }
        }
    };
    ctx.for_each_other_in(area, &mut |other| {
        if !other.removed && matches!(&other.kind, KindPublic::Mob(mob) if mob.health > 0.0) {
            consider(other.uid, other.position, other.bounding_box(), false);
        }
    });
    blocks.for_each_player(&mut |player| {
        let bb = player_box(player);
        if player.alive && !player.spectator && bb.intersects(area) {
            consider(player.uid, player.position, bb, true);
        }
    });
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
    hits.into_iter().map(|(_, hit)| hit).collect()
}

/// `ProjectileUtil.getManyEntityHitResult`：普通箭 / 穿刺箭先裁剪精确包围盒，
/// 未命中时才用 `computeMargin` 膨胀的包围盒，并检查「接触点→盒心」之间有无方块
/// 遮挡（结果取膨胀盒的外侧接触点）。`AbstractArrow.findHitEntities` 走这里。
///
/// Vanilla `ProjectileUtil.getManyEntityHitResult` (ordinary / piercing arrows):
/// clip the exact box first, then fall back to the `computeMargin`-inflated box,
/// checking for a block between the outside contact and the box center; the
/// result is the inflated box's outside contact. `AbstractArrow.findHitEntities`
/// uses this path.
fn clip_entity_with_margin(
    view: &dyn BlockView,
    bb: Aabb,
    from: Vec3,
    to: Vec3,
    context: CollisionContext,
    margin: f64,
) -> Option<Vec3> {
    clip_entity(bb, from, to).or_else(|| {
        if margin == 0.0 {
            return None;
        }
        let outside = clip_entity(bb.inflated(margin), from, to)?;
        let center = bb.center();
        let inside = clip_including_border(view, outside, center, context)
            .map_or(center, |hit| hit.location);
        clip_entity(bb, outside, inside).map(|_| outside)
    })
}

pub(crate) fn clip_including_border(
    view: &dyn BlockView,
    from: Vec3,
    to: Vec3,
    context: CollisionContext,
) -> Option<BlockHit> {
    let hit = view.clip_collider(from, to, context);
    let end = hit.map_or(to, |hit| hit.location);
    let border = view.world_border();
    if !border.is_within_bounds(from.x, from.z, 0.0) || border.is_within_bounds(end.x, end.z, 0.0) {
        return hit;
    }
    let delta = end.subtract(from);
    let mut face = Direction::North;
    let mut highest = f32::from_bits(1);
    for direction in Direction::ALL {
        let (x, y, z) = direction.normal();
        let dot = delta.x as f32 * x as f32 + delta.y as f32 * y as f32 + delta.z as f32 * z as f32;
        if dot > highest {
            highest = dot;
            face = direction;
        }
    }
    let location = border.clamp_vec3(end);
    Some(BlockHit {
        pos: BlockPos::containing(location),
        face,
        location,
        inside: false,
    })
}

pub(crate) fn resolve_owner(ctx: &dyn EntityCtx, uuid: [u8; 16]) -> Option<EntityUid> {
    let mut player = None;
    ctx.blocks().for_each_player(&mut |view| {
        if view.uuid == Some(uuid) {
            player = Some(view.uid);
        }
    });
    player.or_else(|| {
        ctx.entity_uids().into_iter().find(|&uid| {
            ctx.other(uid).is_some_and(
                |entity| matches!(entity.kind, KindPublic::Mob(mob) if mob.uuid == uuid),
            )
        })
    })
}

pub(crate) fn player_box(player: &PlayerView) -> Aabb {
    let p = player.position;
    let half = f64::from(0.6_f32) / 2.0;
    Aabb::new(
        p.x - half,
        p.y,
        p.z - half,
        p.x + half,
        p.y + f64::from(if player.sneaking { 1.5_f32 } else { 1.8_f32 }),
        p.z + half,
    )
}

// AABB.clip 只接受线段内部的入射面交点，不把盒内起点当成命中。
// AABB.clip only accepts entering faces strictly within the segment.
pub(crate) fn clip_entity(bb: Aabb, from: Vec3, to: Vec3) -> Option<Vec3> {
    let start = [from.x, from.y, from.z];
    let d = to.subtract(from);
    let steps = [d.x, d.y, d.z];
    let axes = [Axis::X, Axis::Y, Axis::Z];
    let mut best = 1.0;
    for (i, axis) in axes.into_iter().enumerate() {
        let plane = if steps[i] > 1.0e-7 {
            bb.min(axis)
        } else if steps[i] < -1.0e-7 {
            bb.max(axis)
        } else {
            continue;
        };
        let t = (plane - start[i]) / steps[i];
        if t > 0.0
            && t < best
            && (1..=2).all(|n| {
                let j = (i + n) % 3;
                let value = start[j] + t * steps[j];
                bb.min(axes[j]) - 1.0e-7 < value && value < bb.max(axes[j]) + 1.0e-7
            })
        {
            best = t;
        }
    }
    (best < 1.0).then(|| from.add(d.scale(best)))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowEntityHit {
    uid: EntityUid,
    contact: Vec3,
    player: bool,
}

pub(super) fn impact(
    me: &mut Entity,
    world: &mut World<'_>,
    mut hits: Vec<ArrowEntityHit>,
    block: Option<BlockHit>,
    end: Vec3,
) {
    if let EntityKind::Arrow(arrow) = &mut me.kind {
        arrow.needs_sync = true;
    }
    while !me.removed {
        let from = me.position;
        me.position = hits.first().map_or(end, |hit| hit.contact);
        if let Some(pitch) = projectile_travel_effects(me, world, from) {
            play_extinguished_sound(me, world, pitch);
        }
        if me.removed {
            break;
        }
        if hits.is_empty() {
            if let Some(hit) = block {
                hit_block(me, world, hit);
            }
            break;
        }
        // 官方 `hitTargetsOrDeflectSelf` 只在箭死掉或目标主动偏转时提前返回；
        // 拒伤的反弹发生在 `onHitEntity` 内部，不会中断这一批。
        // Vanilla `hitTargetsOrDeflectSelf` stops only when the arrow dies or the
        // target deflects it; the rejection bounce happens inside `onHitEntity` and
        // does not abort the batch.
        if !hits.into_iter().all(|hit| hit_entity(me, world, hit)) {
            break;
        }
        // 官方只在还有穿透次数时重查剩余线段；没有穿透的箭这一批处理完就停。
        // Vanilla rescans only while piercing is left; a non-piercing arrow stops
        // after this batch.
        if !arrow_pierces(me) {
            break;
        }
        // 这一批命中后，沿原先的方块裁剪终点重查剩余线段。
        // After this batch, query the remaining segment against the original block-clip endpoint.
        let ctx = super::ctx::PreciseCtx::new(world, me.id);
        hits = find_targets(me, &ctx, end);
    }
    finish_flight(me, world);
}

pub(super) fn projectile_travel_effects(
    me: &mut Entity,
    blocks: &dyn BlockView,
    from: Vec3,
) -> Option<f32> {
    let burning = me.remaining_fire_ticks > 0;
    me.apply_effects_along(blocks, from);
    // Entity.playEntityOnFireExtinguishedSound 使用本实体的随机源。
    // Entity.playEntityOnFireExtinguishedSound uses the entity's own random stream.
    (burning && me.remaining_fire_ticks <= 0)
        .then(|| 1.6 + (me.random.next_float() - me.random.next_float()) * 0.4)
}

pub(super) fn play_extinguished_sound(me: &Entity, world: &mut World<'_>, pitch: f32) {
    world.play_sound(
        me.position,
        sounds::sound_event("minecraft:entity.generic.extinguish_fire"),
        sounds::NEUTRAL,
        0.7,
        pitch,
        false,
    );
}

/// `Entity.deflection`：目标是 `deflects_projectiles`（现在只有旋风人）时按
/// `ProjectileDeflection.REVERSE` 弹回（速度 × -0.5、偏航 +170..190），放
/// `BREEZE_DEFLECT` 音，不造成伤害。`Breeze.deflection` 对风弹返回 NONE，但箭和三叉戟
/// 不是风弹，走这一支。
fn deflect_off_target(me: &mut Entity, world: &mut World<'_>, uid: EntityUid) -> bool {
    let deflects = world
        .entity(uid)
        .is_some_and(|entity| {
            entity
                .kind
                .mob()
                .is_some_and(|mob| mob.mob_type.deflects_projectiles())
        });
    if !deflects {
        return false;
    }
    world.play_sound(
        me.position,
        sounds::sound_event("minecraft:entity.breeze.deflect"),
        sounds::HOSTILE,
        1.0,
        1.0,
        false,
    );
    me.delta = me.delta.multiply(-0.5, -0.5, -0.5);
    me.y_rot += 170.0 + me.random.next_float() * 20.0;
    if let EntityKind::Arrow(arrow) = &mut me.kind {
        arrow.needs_sync = true;
    }
    true
}

fn hit_entity(me: &mut Entity, world: &mut World<'_>, hit: ArrowEntityHit) -> bool {
    if deflect_off_target(me, world, hit.uid) {
        return false;
    }
    let EntityKind::Arrow(arrow) = &me.kind else {
        return false;
    };
    if arrow.variant.is_trident() {
        return hit_entity_trident(me, world, hit);
    }
    let EntityKind::Arrow(arrow) = &mut me.kind else {
        return false;
    };
    if arrow.pierce > 0 {
        if arrow.pierced.len() > arrow.pierce as usize {
            me.removed = true;
            return false;
        }
        arrow.pierced.push(hit.uid);
    }
    let speed = me.delta.length() as f32;
    let mut damage = (f64::from(speed) * impact_damage(arrow))
        .clamp(0.0, f64::from(i32::MAX))
        .ceil() as i32;
    if arrow.critical {
        damage = damage.saturating_add(me.random.next_int_bounded(damage / 2 + 2));
    }
    let owner = arrow.owner.and_then(|uid| world.level().entity_public(uid));
    let owner_player = arrow.owner.and_then(|uid| {
        let mut owner = None;
        world.level().for_each_player(&mut |p| {
            if p.uid == uid {
                owner = Some(*p);
            }
        });
        owner
    });
    let cause = owner
        .as_ref()
        .map(|p| p.uid)
        .or(owner_player.map(|p| p.uid));
    let source = DamageSource {
        attacker: cause.or(Some(me.id)),
        direct: Some(me.id),
        attacker_is_player: owner_player.is_some(),
        creative_player: owner_player.is_some_and(|p| p.creative),
        position: Some(me.position),
        projectile_motion: Some(me.delta),
        arrow_ignite_ticks: if me.remaining_fire_ticks > 0 { 100 } else { 0 },
        arrow_punch: arrow
            .weapon
            .as_ref()
            .map_or(0, |weapon| enchantment_level(weapon, "minecraft:punch")),
        attacker_uuid: arrow.owner_uuid,
        scale_when_non_player: owner
            .as_ref()
            .is_some_and(|p| matches!(p.kind, KindPublic::Mob(_))),
        ..DamageSource::environment(DamageKind::Arrow)
    };
    let accepted = if hit.player {
        world.hurt_player_from_mob(hit.uid, source, damage as f32, owner.as_ref())
    } else {
        hurt_mob(world, hit.uid, source, damage as f32)
    };
    if accepted {
        play_hit_sound(me, world);
        // 官方 `LivingEntity` 目标上的 `doKnockback` 和 `doPostHurtEffects` 都在
        // 受伤成功之后；玩家目标只做药水效果（玩家击退不走实体推力）。
        //
        // Vanilla runs `doKnockback` and `doPostHurtEffects` on a living
        // target after a successful hurt. Player targets apply Punch in the hurt adapter.
        if !hit.player {
            apply_punch_knockback(world, hit.uid, &source);
        }
        apply_post_hurt_effects(me, world, hit.uid, hit.player);
        // `AbstractArrow.onHitEntity`：穿透箭攒下的击杀列表，或者普通箭
        // 打死这一个目标时，给射手触发 KILLED_BY_ARROW。
        let after = {
            let EntityKind::Arrow(after) = &me.kind else {
                return false;
            };
            (
                after.owner_uuid,
                after
                    .weapon
                    .as_ref()
                    .and_then(|stack| mc_bamboo_data::items::ITEMS.get(stack.item as usize))
                    .map(|item| item.name)
                    .unwrap_or("minecraft:bow"),
                {
                    let mut names = [""; 5];
                    let uids = if after.pierced.is_empty() {
                        vec![hit.uid]
                    } else {
                        after.pierced.clone()
                    };
                    for (index, uid) in uids.iter().take(5).enumerate() {
                        names[index] = world
                            .entity(*uid)
                            .and_then(|entity| entity.kind.mob().map(|mob| mob.mob_type.name()))
                            .unwrap_or("minecraft:player");
                    }
                    names
                },
            )
        };
        let killed = !hit.player
            && world
                .entity(hit.uid)
                .and_then(|entity| entity.kind.mob().map(|mob| mob.health))
                .is_none_or(|health| health <= 0.0);
        if killed {
            world.push_effect(crate::level::Effect::KilledByArrow {
                shooter: owner_player.and_then(|p| p.uuid).or(after.0),
                victims: after.2,
                weapon: after.1,
            });
        }
        if arrow_pierces(me) {
            return true;
        }
        me.removed = true;
    } else {
        me.delta = me.delta.scale(-0.2 * 0.5);
        me.y_rot += 170.0 + me.random.next_float() * 20.0;
        if me.delta.length_sqr() < 1.0e-7 {
            if let EntityKind::Arrow(arrow) = &me.kind
                && arrow.pickup == Pickup::Allowed
            {
                world.spawn_at_location(
                    me.position.add(Vec3::new(0.0, f64::from(0.1_f32), 0.0)),
                    arrow.pickup_item.clone(),
                );
            }
            me.removed = true;
        }
    }
    // 拒伤造成的反弹发生在 onHitEntity 内，不是目标主动偏转；箭还活着就继续处理这一批。
    // Reversal after rejected damage happens within onHitEntity, not as a target
    // deflection; keep walking the batch while the arrow is alive.
    !me.removed
}

/// `ThrownTrident.onHitEntity`：固定 8 点伤害加穿刺，命中后置 `dealtDamage`，
/// 命中音效加 `ProjectileDeflection.REVERSE` 反弹，实体留在世界里等主人捡回。
///
/// `ThrownTrident.onHitEntity`: a flat 8 damage plus impaling, sets `dealtDamage`,
/// plays the hit sound, deflects with `REVERSE` and stays in the world to be
/// picked up.
fn hit_entity_trident(me: &mut Entity, world: &mut World<'_>, hit: ArrowEntityHit) -> bool {
    let damage = {
        let EntityKind::Arrow(arrow) = &me.kind else {
            return false;
        };
        let aquatic = !hit.player && victim_is_aquatic(world, hit.uid);
        trident_damage(arrow, aquatic)
    };
    let owner = me_owner(me, world);
    let owner_player = me_owner_player(me, world);
    let cause = owner
        .as_ref()
        .map(|p| p.uid)
        .or(owner_player.map(|p| p.uid));
    let mut source = DamageSource {
        attacker: cause.or(Some(me.id)),
        direct: Some(me.id),
        attacker_is_player: owner_player.is_some(),
        creative_player: owner_player.is_some_and(|p| p.creative),
        position: Some(me.position),
        projectile_motion: Some(me.delta),
        attacker_uuid: me_owner_uuid(me),
        scale_when_non_player: owner
            .as_ref()
            .is_some_and(|p| matches!(p.kind, KindPublic::Mob(_))),
        ..DamageSource::environment(DamageKind::Trident)
    };
    // `Zombie.killedEntity`：溺尸扔出的三叉戟命中村民时，官方 `source.getEntity()`
    // 是那只溺尸，同样按难度转化。箭本体正在 tick，主人的随机源在实体表里，
    // 所以这一次的 `nextBoolean()` 要就地改主人的随机源。
    //
    // `Zombie.killedEntity`: for a drowned's trident, vanilla's
    // `source.getEntity()` is the drowned, so the same conversion applies. The
    // owner's random lives in the entity store; draw there.
    if !hit.player {
        let zombie_owner = owner.as_ref().is_some_and(
            |public| matches!(&public.kind, KindPublic::Mob(mob) if mob.mob_type.is_zombie()),
        );
        let mut lethal = false;
        world.level().with_entity_public(hit.uid, &mut |public| {
            lethal = matches!(&public.kind, KindPublic::Mob(mob)
                if mob.mob_type == super::mob::MobType::Villager
                    && mob.health > 0.0
                    && mob.health <= damage);
        });
        let lethal_villager = lethal;
        if zombie_owner && lethal_villager {
            source = source.with_zombie_attacker();
            source.attacker_type = owner.as_ref().map(|public| public.kind.entity_type());
            let difficulty = world.difficulty_id();
            if difficulty == 2 {
                let mut roll = false;
                if let Some(uid) = owner.as_ref().map(|public| public.uid) {
                    world.with_entity_mut(uid, &mut |owner| {
                        roll = owner.random.next_boolean();
                    });
                }
                source = source.with_villager_conversion_roll(roll);
            }
        }
    }
    let accepted = if hit.player {
        world.hurt_player_from_mob(hit.uid, source, damage, owner.as_ref())
    } else {
        hurt_mob(world, hit.uid, source, damage)
    };
    if accepted {
        if !hit.player {
            apply_punch_knockback(world, hit.uid, &source);
        }
        // `channeling` 的 `post_attack`：伤害成功之后才召唤闪电。
        // `channeling`'s `post_attack`: summon only after a successful hurt.
        let (stack, owner_uid) = {
            let EntityKind::Arrow(arrow) = &me.kind else {
                unreachable!()
            };
            (arrow.pickup_item.clone(), arrow.owner)
        };
        let victim = victim_position(world, hit);
        channeling_strike(
            world,
            &stack,
            victim,
            BlockPos::containing(victim),
            owner_uid,
            false,
            BlockState(0),
        );
    }
    if let EntityKind::Arrow(arrow) = &mut me.kind {
        arrow.dealt_damage = true;
        arrow.needs_sync = true;
    }
    // `projectileReceivesSideEffectsOnHit`：命中音效加 REVERSE 反弹，无论伤害是否生效。
    // `projectileReceivesSideEffectsOnHit`: hit sound plus REVERSE deflection,
    // regardless of whether the damage landed.
    world.play_sound(
        me.position,
        trident_hit_sound(),
        sounds::NEUTRAL,
        1.0,
        1.0,
        false,
    );
    let rotation = 170.0 + me.random.next_float() * 20.0;
    // REVERSE 用 `Vec3(0.02, 0.2, 0.02)` 乘 -0.5：速度按 (-0.01, -0.1, -0.01) 缩放。
    // REVERSE uses `Vec3(0.02, 0.2, 0.02)` times -0.5: scale velocity by (-0.01, -0.1, -0.01).
    me.delta = Vec3::new(me.delta.x * -0.01, me.delta.y * -0.1, me.delta.z * -0.01);
    me.y_rot += rotation;
    !me.removed
}

/// `ThrownTrident.onHitEntity` 的伤害：固定 8 点，加 `impaling` 对水生目标每级 2.5。
///
/// The damage of `ThrownTrident.onHitEntity`: a flat 8 plus `impaling`'s 2.5
/// per level against aquatic targets.
fn trident_damage(arrow: &Arrow, victim_aquatic: bool) -> f32 {
    let mut damage = TRIDENT_BASE_DAMAGE;
    let level = enchantment_level(&arrow.pickup_item, "minecraft:impaling");
    if level > 0 && victim_aquatic {
        // 26.3 `impaling` 的 `minecraft:damage`：linear base 2.5 / +2.5，即每级 2.5。
        // 26.3 `impaling` `minecraft:damage`: linear 2.5 / +2.5, i.e. 2.5 per level.
        damage += 2.5 * level as f32;
    }
    damage
}

/// 目标是不是 `#minecraft:aquatic`（穿刺加成只对水生生物生效）。
///
/// Whether the victim is in `#minecraft:aquatic` (impaling only helps against aquatic mobs).
fn victim_is_aquatic(world: &World<'_>, uid: EntityUid) -> bool {
    world
        .entity(uid)
        .and_then(|entity| entity.kind.mob().map(|mob| mob.mob_type))
        .is_some_and(|mob_type| mob_type.in_entity_tag("minecraft:aquatic"))
}

/// 命中目标的位置（`summon_entity` / `can_see_sky` 用的是目标实体自己的位置，
/// 不是射线接触点）；查不到时退回接触点。
///
/// The victim entity's own position (used by `summon_entity` / `can_see_sky`,
/// not the ray contact point); falls back to the contact point.
fn victim_position(world: &World<'_>, hit: ArrowEntityHit) -> Vec3 {
    if !hit.player
        && let Some(entity) = world.entity(hit.uid)
    {
        return entity.position;
    }
    if hit.player {
        let mut found = None;
        world.level().for_each_player(&mut |player| {
            if player.uid == hit.uid {
                found = Some(player.position);
            }
        });
        if let Some(position) = found {
            return position;
        }
    }
    hit.contact
}

/// `channeling` 的 `post_attack` / `hit_block` 效果：雷暴、露天（命中方块还要
/// 是 `#minecraft:lightning_rods`）时召唤一道闪电并播 `item.trident.thunder`。
/// 闪电的 `cause` 是投掷者。
///
/// The `channeling` `post_attack` / `hit_block` effect: during a thunderstorm,
/// with open sky (and a `#minecraft:lightning_rods` block for the block hit),
/// summon a lightning bolt and play `item.trident.thunder`. The bolt's `cause`
/// is the thrower.
fn channeling_strike(
    world: &mut World<'_>,
    stack: &ItemStack,
    position: Vec3,
    sky_pos: BlockPos,
    cause: Option<i32>,
    require_rod: bool,
    state: BlockState,
) {
    if enchantment_level(stack, "minecraft:channeling") <= 0 {
        return;
    }
    if !world.is_thundering() {
        return;
    }
    if require_rod && !state.is_tagged(mc_bamboo_data::tags::block_tags::LIGHTNING_RODS) {
        return;
    }
    if !world.level().can_see_sky(sky_pos) {
        return;
    }
    let mut bolt = crate::entity::lightning::entity(position, false);
    if let EntityKind::LightningBolt(data) = &mut bolt.kind {
        data.cause = cause;
    }
    world.spawn_entity(bolt);
    world.play_sound(
        position,
        sounds::sound_event("minecraft:item.trident.thunder"),
        sounds::NEUTRAL,
        5.0,
        1.0,
        false,
    );
}

fn me_owner(me: &Entity, world: &World<'_>) -> Option<crate::entity::EntityPublic> {
    let EntityKind::Arrow(arrow) = &me.kind else {
        return None;
    };
    arrow.owner.and_then(|uid| world.level().entity_public(uid))
}

fn me_owner_player(me: &Entity, world: &World<'_>) -> Option<super::ctx::PlayerView> {
    let EntityKind::Arrow(arrow) = &me.kind else {
        return None;
    };
    let uid = arrow.owner?;
    let mut owner = None;
    world.level().for_each_player(&mut |p| {
        if p.uid == uid {
            owner = Some(*p);
        }
    });
    owner
}

fn me_owner_uuid(me: &Entity) -> Option<[u8; 16]> {
    match &me.kind {
        EntityKind::Arrow(arrow) => arrow.owner_uuid,
        _ => None,
    }
}

/// `AbstractArrow.doKnockback`：冲击附魔在受伤成功后再推一次。
///
/// `AbstractArrow.doKnockback`: the Punch enchantment pushes again after a
/// successful hurt.
fn apply_punch_knockback(world: &mut World<'_>, target: EntityUid, source: &DamageSource) {
    if source.arrow_punch <= 0 {
        return;
    }
    world.with_entity_mut(target, &mut |entity| {
        let Some(mob) = entity.kind.mob() else {
            return;
        };
        let resistance = mob
            .attributes
            .value(mc_bamboo_data::registries::attribute::KNOCKBACK_RESISTANCE);
        if let Some(push) = source.arrow_push(resistance) {
            entity.delta = entity.delta.add(push);
        }
    });
}

fn apply_post_hurt_effects(me: &Entity, world: &mut World<'_>, target: EntityUid, player: bool) {
    let EntityKind::Arrow(arrow) = &me.kind else {
        return;
    };
    let effects = match arrow.variant {
        ArrowVariant::Spectral { duration } => {
            vec![crate::entity::mob::effects::EffectInstance::new(
                mc_bamboo_data::registries::mob_effect::GLOWING,
                duration,
            )]
        }
        ArrowVariant::Normal => potion_effect_instances(&arrow.pickup_item),
        // 三叉戟没有药水 / 光箭效果。
        // A trident has no potion or spectral effects.
        ArrowVariant::Trident => Vec::new(),
    };
    for effect in effects {
        if crate::entity::mob::effects::is_instantaneous(effect.effect) {
            // 官方 `LivingEntity.addEffect` 对瞬间效果当场结算。
            //
            // Vanilla `LivingEntity.addEffect` settles instantaneous effects at once.
            if player {
                world.instant_player_effect(target, effect.effect, effect.amplifier);
            } else {
                world.apply_mob_instant_effect(target, effect.effect, effect.amplifier);
            }
        } else if player {
            world.give_player_effect(target, effect);
        } else {
            world.add_mob_effect(target, effect);
        }
    }
}

/// `PotionContents.forEachEffect`：先基础药水，再自定义效果，时长按组件缩放。
///
/// `PotionContents.forEachEffect`: the base potion first, then the custom
/// effects, with durations scaled by the component.
/// 按 `PotionContents.getColor` 取自定义色，或按等级对可见效果求加权平均。
///
/// `PotionContents.getColor`: custom colour or a weighted mean of visible effects.
fn potion_color(arrow: &Arrow) -> i32 {
    if arrow.variant != ArrowVariant::Normal || arrow.pickup_item.added.is_empty() {
        return -1;
    }
    potion_color_of(&arrow.pickup_item)
}

/// `PotionContents.getColor()`：自定义色，或按等级对可见效果求加权平均。喷溅药水和药水箭
/// 共用。
///
/// `PotionContents.getColor()`: the custom colour, or a weighted mean of the visible
/// effects. Splash potions and tipped arrows share it.
pub(crate) fn potion_color_of(stack: &mc_bamboo_protocol::item::ItemStack) -> i32 {
    use mc_bamboo_data::registries::data_component_type::POTION_CONTENTS;
    use mc_bamboo_protocol::components::{ComponentValue, defaults};

    let patch = stack.patch();
    let Some(ComponentValue::PotionContents {
        potion,
        custom_color,
        custom_effects,
        custom_name,
    }) = defaults::resolve(stack.item, &patch, POTION_CONTENTS)
    else {
        return -1;
    };
    if let Some(color) = custom_color {
        return *color;
    }
    if potion.is_none() && custom_effects.is_empty() && custom_name.is_none() {
        return -1;
    }
    let mut totals = [0; 3];
    let mut weight = 0;
    for effect in potion_effect_instances(stack)
        .into_iter()
        .filter(|effect| effect.visible)
    {
        let color = mc_bamboo_data::potions::EFFECT_COLORS
            .get(effect.effect as usize)
            .copied()
            .unwrap_or_default();
        let amount = effect.amplifier + 1;
        for (sum, shift) in totals.iter_mut().zip([16, 8, 0]) {
            *sum += amount * (color >> shift & 0xff);
        }
        weight += amount;
    }
    if weight == 0 {
        return -13_083_194;
    }
    let [r, g, b] = totals.map(|sum| sum / weight);
    (0xff00_0000_u32 as i32) | (r << 16) | (g << 8) | b
}

/// `Potion.hasInstantEffects()`：基础药水里有没有瞬时效果。
///
/// `Potion.hasInstantEffects()`: whether the base potion has an instantaneous effect.
pub(crate) fn potion_has_instant(stack: &mc_bamboo_protocol::item::ItemStack) -> bool {
    use mc_bamboo_data::registries::data_component_type::POTION_CONTENTS;
    use mc_bamboo_protocol::components::{ComponentValue, defaults};

    let patch = stack.patch();
    let Some(ComponentValue::PotionContents {
        potion: Some(potion),
        ..
    }) = defaults::resolve(stack.item, &patch, POTION_CONTENTS)
    else {
        return false;
    };
    mc_bamboo_data::potions::effects(*potion).is_some_and(|effects| {
        effects.iter().any(|&(effect, _, _)| {
            effect == mc_bamboo_data::registries::mob_effect::INSTANT_HEALTH
                || effect == mc_bamboo_data::registries::mob_effect::INSTANT_DAMAGE
        })
    })
}

/// `PotionContents.getColor()`：没有药水组件时返回 `BASE_POTION_COLOR`（-13083194），
/// 供喷溅药水的世界事件使用。
///
/// `PotionContents.getColor()`: returns `BASE_POTION_COLOR` (-13083194) when empty,
/// for splash potion level events.
pub(crate) fn potion_contents_color(stack: &mc_bamboo_protocol::item::ItemStack) -> i32 {
    let color = potion_color_of(stack);
    if color == -1 { -13_083_194 } else { color }
}

/// `PotionContents.getAllEffects()` 与 `POTION_DURATION_SCALE`：返回未缩放的原始效果和
/// 持续时间倍率，供 `ThrownSplashPotion.onHitAsPotion` 一次缩放并四舍五入。
///
/// `PotionContents.getAllEffects()` plus `POTION_DURATION_SCALE`: returns the unscaled
/// effects and the duration scale so `ThrownSplashPotion.onHitAsPotion` scales once.
pub(crate) fn potion_raw_effects_and_scale(
    stack: &mc_bamboo_protocol::item::ItemStack,
) -> (Vec<crate::entity::mob::effects::EffectInstance>, f32) {
    use crate::entity::mob::effects::EffectInstance;
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::components::{ComponentValue, defaults};

    let patch = stack.patch();
    let Some(ComponentValue::PotionContents {
        potion,
        custom_effects,
        ..
    }) = defaults::resolve(stack.item, &patch, kinds::POTION_CONTENTS)
    else {
        return (Vec::new(), 1.0);
    };
    let scale = match defaults::resolve(stack.item, &patch, kinds::POTION_DURATION_SCALE) {
        Some(ComponentValue::PotionDurationScale(scale)) => *scale,
        _ => 1.0,
    };
    let mut out = Vec::new();
    if let Some(potion) = potion
        && let Some(effects) = mc_bamboo_data::potions::effects(*potion)
    {
        for &(effect, duration, amplifier) in effects {
            out.push(EffectInstance::with_amplifier(effect, duration, amplifier));
        }
    }
    for instance in custom_effects {
        let details = &instance.details;
        let mut effect = EffectInstance::with_flags(
            instance.effect,
            details.duration,
            details.amplifier,
            details.ambient,
            details.show_particles,
        );
        effect.show_icon = details.show_icon;
        out.push(effect);
    }
    (out, scale)
}

pub(crate) fn potion_effect_instances(
    stack: &mc_bamboo_protocol::item::ItemStack,
) -> Vec<crate::entity::mob::effects::EffectInstance> {
    use crate::entity::mob::effects::EffectInstance;
    use mc_bamboo_data::registries::data_component_type as kinds;
    use mc_bamboo_protocol::components::{ComponentValue, defaults};

    let patch = stack.patch();
    let Some(ComponentValue::PotionContents {
        potion,
        custom_effects,
        ..
    }) = defaults::resolve(stack.item, &patch, kinds::POTION_CONTENTS)
    else {
        return Vec::new();
    };
    let scale = match defaults::resolve(stack.item, &patch, kinds::POTION_DURATION_SCALE) {
        Some(ComponentValue::PotionDurationScale(scale)) => *scale,
        _ => 1.0,
    };
    let mut out = Vec::new();
    if let Some(potion) = potion
        && let Some(effects) = mc_bamboo_data::potions::effects(*potion)
    {
        for &(effect, duration, amplifier) in effects {
            out.push(EffectInstance::with_amplifier(
                effect,
                scaled_duration(duration, scale),
                amplifier,
            ));
        }
    }
    for instance in custom_effects {
        let details = &instance.details;
        let mut effect = EffectInstance::with_flags(
            instance.effect,
            scaled_duration(details.duration, scale),
            details.amplifier,
            details.ambient,
            details.show_particles,
        );
        effect.show_icon = details.show_icon;
        out.push(effect);
    }
    out
}

/// `MobEffectInstance.withScaledDuration`：无限时长和 0 不缩放，其余至少 1 刻。
///
/// `MobEffectInstance.withScaledDuration`: infinite and zero durations stay,
/// everything else gets at least one tick.
fn scaled_duration(duration: i32, scale: f32) -> i32 {
    if duration <= 0 {
        duration
    } else {
        (((duration as f32) * scale) as i32).max(1)
    }
}

fn impact_damage(arrow: &Arrow) -> f64 {
    let Some(weapon) = &arrow.weapon else {
        return arrow.base_damage;
    };
    let mut damage = arrow.base_damage as f32;
    let level = enchantment_level(weapon, "minecraft:power");
    if level > 0 {
        damage += 1.0 + (level - 1) as f32 * 0.5;
    }
    f64::from(damage)
}

/// 武器上某个附魔的等级；没有 `enchantments` 组件时是 0。
///
/// One enchantment's level on a stack; 0 without an `enchantments` component.
pub fn enchantment_level(stack: &ItemStack, enchantment: &str) -> i32 {
    use mc_bamboo_data::registries::{SYNCED_REGISTRIES, data_component_type::ENCHANTMENTS};
    use mc_bamboo_protocol::components::{ComponentValue, defaults};

    let patch = stack.patch();
    let Some(ComponentValue::Enchantments(levels)) =
        defaults::resolve(stack.item, &patch, ENCHANTMENTS)
    else {
        return 0;
    };
    let Some(id) = SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == "minecraft:enchantment")
        .and_then(|registry| {
            registry
                .entries
                .iter()
                .position(|name| *name == enchantment)
        })
    else {
        return 0;
    };
    levels
        .iter()
        .find_map(|&(key, level)| (key == id as i32).then_some(level))
        .unwrap_or(0)
}

fn hurt_mob(world: &mut World<'_>, target: EntityUid, source: DamageSource, damage: f32) -> bool {
    let burning = source.arrow_ignite_ticks > 0;
    let mut previous_fire = 0;
    if burning {
        world.with_entity_mut(target, &mut |entity| {
            previous_fire = entity.remaining_fire_ticks;
            entity.remaining_fire_ticks = previous_fire.max(source.arrow_ignite_ticks);
            if let Some(mob) = entity.kind.mob_mut() {
                mob.ticks_frozen = 0;
            }
        });
    }
    let accepted = world.hurt_mob(target, source, damage);
    if burning && !accepted {
        world.with_entity_mut(target, &mut |entity| {
            entity.remaining_fire_ticks = previous_fire
        });
    }
    accepted
}

fn arrow_pierces(me: &Entity) -> bool {
    matches!(&me.kind, EntityKind::Arrow(arrow) if arrow.pierce > 0)
}

fn hit_block(me: &mut Entity, world: &mut World<'_>, hit: BlockHit) {
    let EntityKind::Arrow(arrow) = &mut me.kind else {
        return;
    };
    arrow.last_state = Some(world.block_state(hit.pos));
    let sign = |x: f64| if x == 0.0 { x } else { x.signum() };
    let offset = Vec3::new(sign(me.delta.x), sign(me.delta.y), sign(me.delta.z));
    me.position = me.position.subtract(offset.scale(f64::from(0.05_f32)));
    me.delta = Vec3::default();
    arrow.in_ground = true;
    arrow.shake_time = 7;
    arrow.critical = false;
    arrow.pierce = 0;
    arrow.pierced.clear();
    if let EntityKind::Arrow(arrow) = &mut me.kind
        && !arrow.variant.is_trident()
    {
        arrow.sound = arrow_hit_sound();
    }
    // `channeling` 的 `hit_block`：命中的方块是避雷针、雷暴且露天时召唤闪电。
    // 官方 `AbstractArrow.onHitBlock` 先跑 `hitBlockEnchantmentEffects`（引雷），
    // 再播落地音效，所以这里也必须在 `play_hit_sound` 之前，音效顺序才对得上。
    // `channeling`'s `hit_block`: a lightning-rod block in a thunderstorm under
    // open sky summons a bolt. Vanilla `AbstractArrow.onHitBlock` runs
    // `hitBlockEnchantmentEffects` before the ground sound, so this must come
    // ahead of `play_hit_sound` for the sound order to match.
    if let EntityKind::Arrow(arrow) = &me.kind
        && arrow.variant.is_trident()
    {
        let stack = arrow.pickup_item.clone();
        let owner_uid = arrow.owner;
        let state = world.block_state(hit.pos);
        channeling_strike(world, &stack, hit.location, hit.pos, owner_uid, true, state);
    }
    play_hit_sound(me, world);
    // `Projectile.onHit` 的 BLOCK 分支：`gameEvent(PROJECTILE_LAND, 命中格心,
    // Context.of(projectile, 命中格的状态))`。
    world.game_event(
        crate::game_event::GameEvent::ProjectileLand,
        crate::world::block_center_public(hit.pos),
        crate::game_event::dispatch::EventContext {
            affected_state: Some(world.block_state(hit.pos)),
            ..Default::default()
        },
    );
}

fn play_hit_sound(me: &mut Entity, world: &mut World<'_>) {
    if let EntityKind::Arrow(arrow) = &me.kind {
        let pitch = 1.2 / (me.random.next_float() * 0.2 + 0.9);
        world.play_sound(me.position, arrow.sound, sounds::NEUTRAL, 1.0, pitch, false);
    }
}

fn finish_flight(me: &mut Entity, blocks: &dyn BlockView) {
    if !me.in_water {
        me.delta = me.delta.scale(f64::from(0.99_f32));
    }
    if matches!(&me.kind, EntityKind::Arrow(arrow)
        if !arrow.in_ground && !arrow.no_gravity && !arrow.no_physics)
    {
        // Entity.applyGravity 把整支向量加上 (0, -g, 0)，也会规范化反弹留下的 -0。
        // `AbstractArrow.tick()` 对 noPhysics 跳过重力（忠诚返回中的三叉戟）。
        // Entity.applyGravity adds (0, -g, 0) to the whole vector, normalizing -0
        // after a reversal. `AbstractArrow.tick()` skips gravity for noPhysics.
        me.delta = me.delta.add(Vec3::new(0.0, -0.05, 0.0));
    }
    let in_lava = me.update_projectile_fluid(blocks);
    // Entity.baseTick 的 on_fire 伤害会让 Projectile.markHurt，即使没有损血。
    // Entity.baseTick's on-fire pulse calls Projectile.markHurt even without health damage.
    let sync_velocity =
        me.remaining_fire_ticks > 0 && me.remaining_fire_ticks % 20 == 0 && !in_lava;
    if me.remaining_fire_ticks > 0 {
        me.remaining_fire_ticks -= 1;
    }
    if me.position.y < f64::from(blocks.min_y() - 64) {
        me.removed = true;
    }
    me.first_tick = false;
    if let EntityKind::Arrow(arrow) = &mut me.kind {
        arrow.has_been_shot = true;
        arrow.sync_velocity = sync_velocity;
        // `Projectile.tick` 在每刻末尾把 `leftOwnerChecked` 清回 false，
        // 所以下一飞行的刻还会再判一次，直到真的离开主人。
        // `Projectile.tick` clears `leftOwnerChecked` at the end of each tick,
        // so the next flying tick checks again until the arrow really left its owner.
        arrow.left_owner_checked = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_crossing_water_extinguishes_before_ending_in_dry_air() {
        let mut level = crate::testing::TestLevel::new();
        level.set(
            BlockPos::new(2, 65, 0),
            BlockState(mc_bamboo_data::blocks::WATER),
        );
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let mut arrow = Entity::arrow(Vec3::new(0.5, 65.25, 0.5));
        arrow.delta = Vec3::new(5.0, 0.0, 0.0);
        arrow.remaining_fire_ticks = 200;
        let id = world.spawn_entity(arrow);
        let mut expected = world.entity(id).unwrap().random;
        expected.next_float();
        expected.next_float();
        world.tick_entities();
        let arrow = world.entity(id).unwrap();
        assert_eq!(arrow.position, Vec3::new(5.5, 65.25, 0.5));
        assert_eq!(arrow.remaining_fire_ticks, 0);
        assert_eq!(arrow.random.state(), expected.state());
        assert!(!arrow.in_water);
    }

    #[test]
    fn piercing_rescans_and_applies_blocks_in_the_remaining_segment() {
        use crate::entity::mob::MobType;
        let mut level = crate::testing::TestLevel::new();
        level.set(
            BlockPos::new(5, 65, 0),
            BlockState(mc_bamboo_data::blocks::COBWEB),
        );
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let targets = [2.0, 4.0].map(|x| {
            world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(x, 64.0, 0.5)))
        });
        let mut arrow = Entity::arrow(Vec3::new(0.5, 65.0, 0.5));
        let EntityKind::Arrow(data) = &mut arrow.kind else {
            unreachable!()
        };
        data.pierce = 1;
        data.base_damage = 0.5;
        arrow.delta = Vec3::new(6.0, 0.0, 0.0);
        let end = arrow.position.add(arrow.delta);
        // 首批候选只给第一头牛；后续扫描应发现第二头，并穿过其后的蛛网。
        // Supply only the first cow in the initial batch. The rescan should find
        // the second one and apply the web behind it.
        impact(
            &mut arrow,
            &mut world,
            vec![ArrowEntityHit {
                uid: targets[0],
                contact: Vec3::new(1.7, 65.0, 0.5),
                player: false,
            }],
            None,
            end,
        );
        assert_eq!(
            targets.map(|id| world.entity(id).unwrap().kind.mob().unwrap().health),
            [7.0, 7.0]
        );
        assert_eq!(arrow.position, end);
        assert_eq!(arrow.stuck_speed_multiplier.x, 0.25);
    }

    #[test]
    fn burning_arrow_forces_velocity_sync_on_the_fire_damage_pulse() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let mut arrow = Entity::arrow(Vec3::new(0.5, 65.0, 0.5));
        arrow.remaining_fire_ticks = 20;
        let id = world.spawn_entity(arrow);
        for expected in [true, false] {
            world.tick_entities();
            let entity = world.entity(id).unwrap();
            assert!(
                matches!(&entity.kind, EntityKind::Arrow(data) if data.sync_velocity == expected)
            );
        }
    }

    #[test]
    fn piercing_arrow_keeps_hitting_after_rejection_until_it_reaches_the_limit() {
        use crate::entity::mob::MobType;
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let mut arrow = Entity::arrow(Vec3::new(0.5, 65.0, 0.5));
        if let EntityKind::Arrow(data) = &mut arrow.kind {
            data.pierce = 1;
            data.base_damage = 0.5;
        }
        arrow.shoot_arrow(Vec3::new(1.0, 0.0, 0.0), 6.0, 0.0);
        let projectile = world.spawn_entity(arrow);
        let targets = [2.0, 3.5, 5.5].map(|x| {
            let mut target = Entity::by_mob_type(MobType::Cow, Vec3::new(x, 64.0, 0.5));
            target.with_mob(|_, mob| {
                mob.no_ai = true;
                mob.invulnerable = x == 2.0;
            });
            world.spawn_entity(target)
        });
        world.tick_entities();
        let health = targets.map(|id| world.entity(id).unwrap().kind.mob().unwrap().health);
        assert_eq!(health, [10.0, 9.0, 10.0]);
        assert!(world.entity(projectile).is_none());
    }

    #[test]
    fn non_piercing_arrow_keeps_the_batch_after_a_rejected_hit() {
        use crate::entity::mob::MobType;
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let targets = [2.0, 4.0].map(|x| {
            let mut target = Entity::by_mob_type(MobType::Cow, Vec3::new(x, 64.0, 0.5));
            target.with_mob(|_, mob| {
                mob.no_ai = true;
                mob.invulnerable = true;
            });
            world.spawn_entity(target)
        });
        let mut arrow = Entity::arrow(Vec3::new(0.5, 65.0, 0.5));
        if let EntityKind::Arrow(data) = &mut arrow.kind {
            data.base_damage = 0.5;
        }
        arrow.delta = Vec3::new(6.0, 0.0, 0.0);
        let end = arrow.position.add(arrow.delta);
        // 两个目标都拒伤；每次都把速度反向。两次反弹后 X 速度回到正值。
        // Both targets reject damage; each rejection reverses the motion, so the X
        // speed is positive again only after both hits were processed.
        impact(
            &mut arrow,
            &mut world,
            vec![
                ArrowEntityHit {
                    uid: targets[0],
                    contact: Vec3::new(1.7, 65.0, 0.5),
                    player: false,
                },
                ArrowEntityHit {
                    uid: targets[1],
                    contact: Vec3::new(3.7, 65.0, 0.5),
                    player: false,
                },
            ],
            None,
            end,
        );
        assert!(arrow.delta.x > 0.0, "delta was {:?}", arrow.delta);
        assert!(!arrow.removed);
    }

    #[test]
    fn no_gravity_keeps_air_drag_and_does_not_add_vertical_speed() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut arrow = Entity::arrow(Vec3::new(0.5, 64.5, 0.5));
        let EntityKind::Arrow(data) = &mut arrow.kind else {
            unreachable!()
        };
        data.no_gravity = true;
        arrow.delta = Vec3::new(1.0, 0.0, 0.0);
        let mut world = World::new(&mut level, &mut ticks);
        let uid = world.spawn_entity(arrow);
        world.tick_entities();
        let arrow = world.entity(uid).unwrap();
        assert_eq!(arrow.position, Vec3::new(1.5, 64.5, 0.5));
        assert_eq!(arrow.delta, Vec3::new(f64::from(0.99_f32), 0.0, 0.0));
    }

    #[test]
    fn embedded_arrow_is_removed_at_age_1200() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let pos = BlockPos::new(0, 64, 0);
        let state = BlockState(mc_bamboo_data::blocks::STONE);
        level.set(pos, state);
        let mut arrow = Entity::arrow(Vec3::new(0.5, 64.5, 0.5));
        let EntityKind::Arrow(data) = &mut arrow.kind else {
            unreachable!()
        };
        data.in_ground = true;
        data.last_state = Some(state);
        data.life = 1198;
        let mut world = World::new(&mut level, &mut ticks);
        let uid = world.spawn_entity(arrow);
        world.tick_entities();
        assert!(
            matches!(&world.entity(uid).unwrap().kind, EntityKind::Arrow(data) if data.life == 1199)
        );
        world.tick_entities();
        assert!(world.entity(uid).is_none());
    }

    #[test]
    fn spectral_arrows_glow_the_target() {
        use crate::entity::mob::MobType;
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let target =
            world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(3.0, 64.0, 0.5)));
        let arrow = Entity::arrow_from_shot(ArrowShot {
            position: Vec3::new(0.5, 65.0, 0.5),
            direction: Vec3::new(1.0, 0.0, 0.0),
            power: 6.0,
            uncertainty: 0.0,
            source_movement: None,
            owner: None,
            owner_uuid: None,
            ammo: ItemStack::new(item::SPECTRAL_ARROW, 1),
            weapon: None,
            critical: false,
            pickup: Pickup::Allowed,
        });
        world.spawn_entity(arrow);
        world.tick_entities();
        let entity = world.entity(target).unwrap();
        let mob = entity.kind.mob().unwrap();
        assert!(
            mob.effects
                .has(mc_bamboo_data::registries::mob_effect::GLOWING)
        );
        assert_eq!(
            mob.effects
                .get(mc_bamboo_data::registries::mob_effect::GLOWING)
                .map(|effect| effect.duration),
            Some(200)
        );
    }

    #[test]
    fn a_tipped_arrow_applies_its_potion_effects() {
        use crate::entity::mob::MobType;
        use mc_bamboo_data::registries::mob_effect;

        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let target =
            world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(3.0, 64.0, 0.5)));
        let ammo = poison_arrow_stack();
        let arrow = Entity::arrow_from_shot(ArrowShot {
            position: Vec3::new(0.5, 65.0, 0.5),
            direction: Vec3::new(1.0, 0.0, 0.0),
            power: 6.0,
            uncertainty: 0.0,
            source_movement: None,
            owner: None,
            owner_uuid: None,
            ammo,
            weapon: None,
            critical: false,
            pickup: Pickup::Allowed,
        });
        world.spawn_entity(arrow);
        world.tick_entities();
        let entity = world.entity(target).unwrap();
        let mob = entity.kind.mob().unwrap();
        assert!(mob.effects.has(mob_effect::POISON));
        assert_eq!(
            mob.effects
                .get(mob_effect::POISON)
                .map(|effect| effect.duration),
            Some(112),
            "药水箭的 `potion_duration_scale` 是 0.125，45 秒的毒变成 112 刻"
        );
    }

    fn poison_arrow_stack() -> ItemStack {
        use mc_bamboo_data::registries::data_component_type as kinds;
        use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};

        let mut ammo = ItemStack::new(item::TIPPED_ARROW, 1);
        let mut patch = ComponentPatch::new();
        patch.put(
            kinds::POTION_CONTENTS,
            PatchEntry::Set(ComponentValue::PotionContents {
                potion: Some(mc_bamboo_data::registries::potion::POISON),
                custom_color: None,
                custom_effects: Vec::new(),
                custom_name: None,
            }),
        );
        let (added, removed) = patch.to_opaque();
        ammo.added = added;
        ammo.removed = removed;
        ammo
    }

    /// `Arrow.tick`：插地第 600 刻广播事件 0，拾取物变成普通箭，颜色回到 -1。
    #[test]
    fn a_tipped_arrow_fades_after_600_ticks_in_the_ground() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        level.set(
            BlockPos::new(2, 64, 0),
            BlockState(mc_bamboo_data::blocks::STONE),
        );
        let mut world = World::new(&mut level, &mut ticks);
        let mut arrow = Entity::arrow(Vec3::new(2.5, 64.5, 0.5));
        if let EntityKind::Arrow(state) = &mut arrow.kind {
            state.pickup_item = poison_arrow_stack();
        }
        let uid = world.spawn_entity(arrow);
        let state = |world: &World<'_>| match &world.entity(uid).unwrap().kind {
            EntityKind::Arrow(arrow) => {
                (arrow.in_ground_time, arrow.potion_puff, potion_color(arrow))
            }
            _ => unreachable!(),
        };
        for _ in 0..599 {
            world.tick_entities();
        }
        let (time, puff, color) = state(&world);
        assert_eq!((time, puff), (599, false));
        assert_ne!(color, -1);
        world.tick_entities();
        assert_eq!(state(&world), (600, true, -1));
        let entity = world.entity(uid).unwrap();
        let EntityKind::Arrow(arrow) = &entity.kind else {
            unreachable!()
        };
        assert_eq!(arrow.pickup_item, ItemStack::new(item::ARROW, 1));
    }

    #[test]
    fn punch_pushes_the_target_along_the_arrow() {
        use crate::entity::mob::MobType;
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let target =
            world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(3.0, 64.0, 0.5)));
        let mut weapon = ItemStack::new(item::BOW, 1);
        let mut patch = mc_bamboo_protocol::components::ComponentPatch::new();
        patch.put(
            mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
            mc_bamboo_protocol::components::PatchEntry::Set(
                mc_bamboo_protocol::components::ComponentValue::Enchantments(vec![(
                    enchantment_id("minecraft:punch"),
                    1,
                )]),
            ),
        );
        let (added, removed) = patch.to_opaque();
        weapon.added = added;
        weapon.removed = removed;
        let arrow = Entity::arrow_from_shot(ArrowShot {
            position: Vec3::new(0.5, 65.0, 0.5),
            direction: Vec3::new(1.0, 0.0, 0.0),
            power: 6.0,
            uncertainty: 0.0,
            source_movement: None,
            owner: None,
            owner_uuid: None,
            ammo: ItemStack::new(item::ARROW, 1),
            weapon: Some(weapon),
            critical: false,
            pickup: Pickup::Allowed,
        });
        world.spawn_entity(arrow);
        world.tick_entities();
        let entity = world.entity(target).unwrap();
        assert!(
            entity.delta.x > 0.0,
            "冲击附魔把目标沿箭的方向推出去，got {:?}",
            entity.delta
        );
    }

    fn enchantment_id(name: &str) -> i32 {
        mc_bamboo_data::registries::SYNCED_REGISTRIES
            .iter()
            .find(|registry| registry.name == "minecraft:enchantment")
            .and_then(|registry| registry.entries.iter().position(|entry| *entry == name))
            .expect("enchantment is registered") as i32
    }

    #[test]
    fn entity_clip_excludes_inside_starts_and_endpoint_contacts() {
        let bb = Aabb::new(0., 0., 0., 1., 1., 1.);
        let start = Vec3::new(-1., 0.5, 0.5);
        let face = Vec3::new(0., 0.5, 0.5);
        assert_eq!(clip_entity(bb, start, Vec3::new(2., 0.5, 0.5)), Some(face));
        assert_eq!(clip_entity(bb, start, face), None);
        assert_eq!(clip_entity(bb, bb.center(), start), None);
    }

    #[test]
    fn arrow_embeds_and_detaches_after_its_block_is_removed() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let block = BlockPos::new(2, 64, 0);
        level.set(block, BlockState(mc_bamboo_data::blocks::STONE));
        let mut arrow = Entity::arrow(Vec3::new(0.5, 64.5, 0.5));
        arrow.delta = Vec3::new(3.0, 0.0, 0.0);
        let uid;
        {
            let mut world = World::new(&mut level, &mut ticks);
            uid = world.spawn_entity(arrow);
            world.tick_entities();
            let arrow = world.entity(uid).unwrap();
            assert_eq!(arrow.position.x, 2.0 - f64::from(0.05_f32));
            assert_eq!(arrow.delta, Vec3::default());
            assert!(
                matches!(&arrow.kind, EntityKind::Arrow(a) if a.in_ground && a.shake_time == 7)
            );

            let row = arrow.split();
            let tag = crate::archive::entity::write_entity(&row).unwrap();
            let restored = crate::archive::entity::read_entity(&tag, uid)
                .unwrap()
                .unwrap();
            let KindPublic::Arrow(mut original) = row.public.kind else {
                panic!("expected arrow")
            };
            original.needs_sync = false;
            assert_eq!(KindPublic::Arrow(original), restored.public.kind);
            let super::super::KindState::Arrow(mut saved) = row.state.kind else {
                panic!("expected arrow state")
            };
            saved.needs_sync = false;
            // `leftOwnerChecked` 不写进存档，读回来从 false 开始。
            // `leftOwnerChecked` is not saved, so a reloaded arrow starts from false.
            saved.left_owner_checked = false;
            assert_eq!(super::super::KindState::Arrow(saved), restored.state.kind);
        }
        level.set(block, BlockState(mc_bamboo_data::blocks::AIR));
        let mut world = World::new(&mut level, &mut ticks);
        world.tick_entities();
        let detached = world.entity(uid).unwrap();
        assert!(matches!(&detached.kind, EntityKind::Arrow(a) if !a.in_ground && a.life == 0));
        world.tick_entities();
        assert_eq!(world.entity(uid).unwrap().delta.y, -0.05);
    }

    #[test]
    fn a_block_placed_around_an_arrow_only_starts_shaking_once() {
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let uid = world.spawn_entity(Entity::arrow(Vec3::new(0.5, 64.5, 0.5)));
        world.tick_entities();
        world.set_block(
            BlockPos::new(0, 64, 0),
            BlockState(mc_bamboo_data::blocks::STONE),
            2,
        );
        for expected in [6, 5, 4, 3, 2, 1, 0, 0, 0] {
            world.tick_entities();
            let arrow = world.entity(uid).unwrap();
            assert!(
                matches!(&arrow.kind, EntityKind::Arrow(a) if a.in_ground && a.shake_time == expected)
            );
        }
    }

    #[test]
    fn arrow_shoot_uses_xyz_triangular_spread() {
        let mut arrow = Entity::arrow(Vec3::default());
        let mut random = arrow.random;
        let spread = 0.0172275 * 6.0;
        let expected = Vec3::new(
            random.triangle(0., spread),
            random.triangle(0., spread),
            1. + random.triangle(0., spread),
        )
        .scale(f64::from(1.6_f32));
        arrow.shoot_arrow(Vec3::new(0., 0., 2.), 1.6, 6.);
        assert_eq!(arrow.delta, expected);
        assert_eq!(arrow.random, random);
    }

    #[test]
    fn left_owner_is_rechecked_on_every_flying_tick() {
        use crate::entity::mob::MobType;
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let owner =
            world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(0.5, 64.0, 0.5)));
        let mut arrow = Entity::arrow(Vec3::new(0.5, 65.0, 0.5));
        if let EntityKind::Arrow(data) = &mut arrow.kind {
            data.owner = Some(owner);
        }
        arrow.delta = Vec3::new(5.0, 0.0, 0.0);
        let uid = world.spawn_entity(arrow);
        let left_owner = |world: &World<'_>| {
            let entity = world.entity(uid).unwrap();
            let EntityKind::Arrow(data) = &entity.kind else {
                panic!("expected arrow")
            };
            data.left_owner
        };
        world.tick_entities();
        assert!(!left_owner(&world));
        world.tick_entities();
        assert!(left_owner(&world));
    }

    fn enchanted_trident(enchantment: &str, level: i32) -> ItemStack {
        use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};
        let mut stack = ItemStack::new(item::TRIDENT, 1);
        let mut patch = ComponentPatch::new();
        patch.put(
            mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(vec![(
                enchantment_id(enchantment),
                level,
            )])),
        );
        let (added, removed) = patch.to_opaque();
        stack.added = added;
        stack.removed = removed;
        stack
    }

    fn trident_shot(item: ItemStack) -> Entity {
        Entity::trident_from_shot(TridentShot {
            position: Vec3::new(0.5, 65.0, 0.5),
            direction: Vec3::new(1.0, 0.0, 0.0),
            power: 2.5,
            uncertainty: 0.0,
            source_movement: None,
            owner: None,
            owner_uuid: None,
            item,
            pickup: Pickup::Allowed,
        })
    }

    fn arrow_of(entity: &Entity) -> &Arrow {
        match &entity.kind {
            EntityKind::Arrow(arrow) => arrow,
            _ => panic!("expected an arrow/trident"),
        }
    }

    #[test]
    fn a_trident_is_an_arrow_variant_with_its_own_entity_type() {
        use mc_bamboo_data::registries::entity_type;
        let trident = Entity::trident(Vec3::default());
        assert_eq!(trident.kind.entity_type(), entity_type::TRIDENT);
        assert_eq!(arrow_of(&trident).variant, ArrowVariant::Trident);
        assert_eq!(trident.dimensions, (0.5, 0.5));
    }

    #[test]
    fn a_trident_reads_loyalty_and_foil_from_its_item() {
        let plain = trident_shot(ItemStack::new(item::TRIDENT, 1));
        assert_eq!(arrow_of(&plain).loyalty, 0);
        assert!(!arrow_of(&plain).foil);
        assert_eq!(arrow_of(&plain).base_damage, 8.0);
        assert_eq!(arrow_of(&plain).sound, trident_hit_ground_sound());

        let loyal = trident_shot(enchanted_trident("minecraft:loyalty", 3));
        assert_eq!(arrow_of(&loyal).loyalty, 3);
        assert!(arrow_of(&loyal).foil, "an enchanted trident has the glint");
        assert_eq!(arrow_of(&loyal).pickup_item.item, item::TRIDENT);
        assert!(arrow_of(&loyal).weapon.is_none());
    }

    #[test]
    fn trident_damage_is_flat_eight_plus_impaling_on_aquatic_targets() {
        let plain_entity = trident_shot(ItemStack::new(item::TRIDENT, 1));
        let plain = arrow_of(&plain_entity);
        assert_eq!(trident_damage(plain, false), 8.0);
        assert_eq!(trident_damage(plain, true), 8.0);

        let impaling_entity = trident_shot(enchanted_trident("minecraft:impaling", 3));
        let impaling = arrow_of(&impaling_entity);
        assert_eq!(trident_damage(impaling, false), 8.0);
        assert_eq!(trident_damage(impaling, true), 8.0 + 2.5 * 3.0);
    }

    #[test]
    fn only_aquatic_mobs_get_the_impaling_bonus() {
        use crate::entity::mob::MobType;
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let cow = world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::default()));
        let squid = world.spawn_entity(Entity::by_mob_type(MobType::Squid, Vec3::default()));
        assert!(!victim_is_aquatic(&world, cow));
        assert!(victim_is_aquatic(&world, squid));
    }

    #[test]
    fn a_trident_hit_keeps_the_entity_and_sets_dealt_damage() {
        use crate::entity::mob::MobType;
        let mut level = crate::testing::TestLevel::new();
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let target =
            world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(2.0, 64.0, 0.5)));
        let mut trident = trident_shot(ItemStack::new(item::TRIDENT, 1));
        trident.delta = Vec3::new(3.0, 0.0, 0.0);
        let end = trident.position.add(trident.delta);
        impact(
            &mut trident,
            &mut world,
            vec![ArrowEntityHit {
                uid: target,
                contact: Vec3::new(1.7, 65.0, 0.5),
                player: false,
            }],
            None,
            end,
        );
        // 8 点固定伤害；牛 10 点血剩 2 点。
        // Flat eight damage; a cow at 10 health drops to 2.
        assert_eq!(
            world.entity(target).unwrap().kind.mob().unwrap().health,
            2.0
        );
        assert!(arrow_of(&trident).dealt_damage);
        assert!(
            !trident.removed,
            "a thrown trident is not removed on an entity hit"
        );
    }

    #[test]
    fn a_trident_uses_a_higher_water_inertia_than_an_arrow() {
        let mut level = crate::testing::TestLevel::new();
        level.set(
            BlockPos::new(1, 65, 0),
            BlockState(mc_bamboo_data::blocks::WATER),
        );
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let mut trident = trident_shot(ItemStack::new(item::TRIDENT, 1));
        trident.delta = Vec3::new(1.0, 0.0, 0.0);
        trident.in_water = true;
        let mut arrow = Entity::arrow(Vec3::new(0.5, 65.0, 0.5));
        arrow.delta = Vec3::new(1.0, 0.0, 0.0);
        arrow.in_water = true;
        let trident_id = world.spawn_entity(trident);
        let arrow_id = world.spawn_entity(arrow);
        world.tick_entities();
        // 两者落地后的公共缩放相同，所以 x 分量之比就是 `fly()` 的水中惯性之比：0.99 / 0.6。
        // Both share the same post-flight scaling, so the x ratio is the fly()
        // water-inertia ratio: 0.99 / 0.6.
        let trident_x = world.entity(trident_id).unwrap().delta.x;
        let arrow_x = world.entity(arrow_id).unwrap().delta.x;
        let ratio = trident_x / arrow_x;
        assert!(
            (ratio - 0.99_f32 as f64 / 0.6_f32 as f64).abs() < 1.0e-6,
            "ratio was {ratio}"
        );
    }

    #[test]
    fn a_trident_survives_a_save_round_trip_with_dealt_damage() {
        let mut trident = trident_shot(enchanted_trident("minecraft:loyalty", 2));
        if let EntityKind::Arrow(arrow) = &mut trident.kind {
            arrow.dealt_damage = true;
        }
        trident.id = 77;
        let row = trident.split();
        let tag = crate::archive::entity::write_entity(&row).unwrap();
        let restored = crate::archive::entity::read_entity(&tag, 77)
            .unwrap()
            .unwrap();
        let arrow = match &restored.public.kind {
            crate::entity::KindPublic::Arrow(a) => a,
            _ => panic!("expected arrow"),
        };
        assert_eq!(arrow.variant, ArrowVariant::Trident);
        assert_eq!(arrow.loyalty, 2);
        assert!(arrow.foil);
        let state = match &restored.state.kind {
            crate::entity::KindState::Arrow(a) => a,
            _ => panic!("expected arrow state"),
        };
        assert!(state.dealt_damage);
    }

    /// `dealt_damage` 建模「是否已经命中过」：官方忠诚返回只在命中后触发。
    /// `dealt_damage` models whether the trident has already hit: vanilla only
    /// returns a loyal trident after it has dealt damage.
    fn loyal_trident(owner_uid: i32, position: Vec3, loyalty: u8, dealt_damage: bool) -> Entity {
        let mut entity = Entity::trident(position);
        if let EntityKind::Arrow(arrow) = &mut entity.kind {
            arrow.loyalty = loyalty;
            arrow.owner = Some(owner_uid);
            arrow.owner_uuid = Some([1; 16]);
            arrow.pickup = Pickup::Allowed;
            arrow.dealt_damage = dealt_damage;
        }
        entity
    }

    fn return_player(uid: i32, position: Vec3, alive: bool) -> crate::entity::ctx::PlayerView {
        crate::entity::ctx::PlayerView {
            shoulder_left: None,
            shoulder_right: None,
            uid,
            uuid: Some([1; 16]),
            position,
            delta: Vec3::default(),
            width: 0.6,
            height: 1.8,
            eye_height: 1.62,
            health: 20.0,
            look_direction: Vec3::default(),
            gaze_disguise: false,
            alive,
            spectator: false,
            game_mode: 0,
            mining_fatigue: None,
            creative: false,
            flying: false,
            explosion_knockback_resistance: 0.0,
            sneaking: false,
            sprinting: false,
            swimming: false,
            invisible: false,
            armor_cover: 0.0,
            main_hand: 0,
            off_hand: 0,
            team: None,
            combat: Default::default(),
            effects: Default::default(),
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    /// 忠诚三叉戟第一刻朝主人加速、置 noPhysics、记返回计数并播返回音效。
    #[test]
    fn a_loyal_trident_accelerates_toward_its_owner_and_plays_the_return_sound() {
        let mut level = crate::testing::TestLevel::new();
        let owner = return_player(7, Vec3::new(3.5, 64.0, 0.5), true);
        level.players.push(owner);
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let id = world.spawn_entity(loyal_trident(7, Vec3::new(0.5, 65.0, 0.5), 3, true));
        world.tick_entities();
        {
            let trident = world.entity(id).expect("still returning");
            let EntityKind::Arrow(arrow) = &trident.kind else {
                panic!("expected a trident");
            };
            assert!(arrow.no_physics);
            assert_eq!(arrow.client_side_return_trident_tick_count, 1);
            // `delta = old * 0.95 + normalize(toOwner) * 0.05*L`（初速为 0），
            // 之后 `super.tick()` 仍施加 0.99 空气阻力（noPhysics 也照施）。
            // `delta = old*0.95 + normalize(toOwner)*0.05*L` (initial speed 0),
            // then `super.tick()` still applies the 0.99 air drag even with noPhysics.
            let to_owner = Vec3::new(3.0, (64.0 + 1.62) - 65.0, 0.0);
            let expected = to_owner
                .normalize()
                .scale(0.05 * 3.0)
                .scale(f64::from(0.99_f32));
            assert!(
                (trident.delta.x - expected.x).abs() < 1.0e-9
                    && (trident.delta.y - expected.y).abs() < 1.0e-9,
                "delta accelerates toward the owner: {:?} vs {:?}",
                trident.delta,
                expected
            );
            // `setPosRaw` 的 y 偏移加上 super.tick 的位移，都让 y 前进。
            assert!(trident.position.y > 65.0);
        }
        let effects = world.drain_effects();
        let sound = i32::from(sounds::sound_event("minecraft:item.trident.return"));
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, crate::level::Effect::Sound { sound: s, .. } if *s == sound)),
            "the return sound plays on the first returning tick"
        );
    }

    /// 主人死了：掉落成物品并丢弃，不再返回。
    #[test]
    fn a_loyal_trident_drops_and_discards_when_its_owner_is_dead() {
        let mut level = crate::testing::TestLevel::new();
        let owner = return_player(7, Vec3::new(3.5, 64.0, 0.5), false);
        level.players.push(owner);
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let id = world.spawn_entity(loyal_trident(7, Vec3::new(0.5, 65.0, 0.5), 3, true));
        world.tick_entities();
        assert!(world.entity(id).is_none(), "the trident is discarded");
        let mut items = 0;
        let area = Aabb::new(-1.0, 63.0, -1.0, 5.0, 67.0, 2.0);
        world
            .level()
            .for_each_entity_in_section_order(area, &mut |public| {
                if matches!(public.kind, crate::entity::KindPublic::Item { .. }) {
                    items += 1;
                }
            });
        assert_eq!(items, 1, "the pickup item is dropped");
    }

    /// 第二刻返回音效不再播，计数继续加。
    #[test]
    fn the_return_sound_plays_only_once() {
        let mut level = crate::testing::TestLevel::new();
        let owner = return_player(7, Vec3::new(3.5, 64.0, 0.5), true);
        level.players.push(owner);
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let id = world.spawn_entity(loyal_trident(7, Vec3::new(0.5, 65.0, 0.5), 3, true));
        world.tick_entities();
        let _ = world.drain_effects();
        world.tick_entities();
        let trident = world.entity(id).expect("still returning");
        let EntityKind::Arrow(arrow) = &trident.kind else {
            panic!("expected a trident");
        };
        assert_eq!(arrow.client_side_return_trident_tick_count, 2);
        let sound = i32::from(sounds::sound_event("minecraft:item.trident.return"));
        let effects = world.drain_effects();
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, crate::level::Effect::Sound { sound: s, .. } if *s == sound)),
            "no return sound on the second tick"
        );
    }

    /// 刚投出、还没命中的忠诚三叉戟不会立刻飞回：官方 `dealtDamage || isNoPhysics()`
    /// 在命中前为假，这一刻只做正常飞行（不置 noPhysics、不播返回音效）。
    /// A freshly thrown loyal trident does not return before it hits: vanilla's
    /// `dealtDamage || isNoPhysics()` is false before a hit, so it just flies.
    #[test]
    fn a_loyal_trident_keeps_flying_until_it_hits() {
        let mut level = crate::testing::TestLevel::new();
        let owner = return_player(7, Vec3::new(3.5, 64.0, 0.5), true);
        level.players.push(owner);
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let id = world.spawn_entity(loyal_trident(7, Vec3::new(0.5, 65.0, 0.5), 3, false));
        world.tick_entities();
        {
            let trident = world.entity(id).expect("still flying");
            let EntityKind::Arrow(arrow) = &trident.kind else {
                panic!("expected a trident");
            };
            assert!(!arrow.no_physics, "no return before a hit");
            assert_eq!(arrow.client_side_return_trident_tick_count, 0);
        }
        let sound = i32::from(sounds::sound_event("minecraft:item.trident.return"));
        let effects = world.drain_effects();
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, crate::level::Effect::Sound { sound: s, .. } if *s == sound)),
            "no return sound before a hit"
        );
    }

    fn channeling_trident(with_channeling: bool) -> Entity {
        let mut entity = Entity::trident(Vec3::new(0.5, 65.0, 0.5));
        if let EntityKind::Arrow(arrow) = &mut entity.kind {
            arrow.pickup_item = if with_channeling {
                enchanted_trident("minecraft:channeling", 1)
            } else {
                ItemStack::new(item::TRIDENT, 1)
            };
            arrow.owner = Some(1 << 24);
        }
        entity.delta = Vec3::new(1.0, 0.0, 0.0);
        entity
    }

    /// 命中一头牛，返回（闪电实体数、有没有播雷声、闪电的 cause）。
    fn channeling_hit(thundering: bool, with_channeling: bool) -> (usize, bool, Option<i32>) {
        let mut level = crate::testing::TestLevel::new();
        if thundering {
            level.weather = crate::weather::BlockWeather {
                rain_level: 1.0,
                thunder_level: 1.0,
            };
        }
        let mut ticks = crate::BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let target = world.spawn_entity(Entity::by_mob_type(
            crate::entity::mob::MobType::Cow,
            Vec3::new(2.0, 64.0, 0.5),
        ));
        let mut trident = channeling_trident(with_channeling);
        let end = trident.position.add(trident.delta);
        impact(
            &mut trident,
            &mut world,
            vec![ArrowEntityHit {
                uid: target,
                contact: Vec3::new(1.7, 65.0, 0.5),
                player: false,
            }],
            None,
            end,
        );
        let effects = world.drain_effects();
        let thunder = i32::from(sounds::sound_event("minecraft:item.trident.thunder"));
        let has_sound = effects
            .iter()
            .any(|effect| matches!(effect, crate::level::Effect::Sound { sound, .. } if *sound == thunder));
        let area = Aabb::new(-1.0, 63.0, -1.0, 5.0, 67.0, 2.0);
        let mut bolts = 0;
        let mut bolt_uid = None;
        world
            .level()
            .for_each_entity_in_section_order(area, &mut |public| {
                if matches!(public.kind, crate::entity::KindPublic::LightningBolt(_)) {
                    bolts += 1;
                    bolt_uid = Some(public.uid);
                }
            });
        let cause = bolt_uid.and_then(|uid| {
            world.entity(uid).and_then(|entity| match &entity.kind {
                EntityKind::LightningBolt(data) => data.cause,
                _ => None,
            })
        });
        (bolts, has_sound, cause)
    }

    #[test]
    fn channeling_summons_a_caused_bolt_in_a_thunderstorm() {
        let (bolts, sound, cause) = channeling_hit(true, true);
        assert_eq!(bolts, 1, "one lightning bolt is summoned");
        assert!(sound, "the thunder sound plays");
        assert_eq!(cause, Some(1 << 24), "the bolt's cause is the thrower");
    }

    #[test]
    fn channeling_needs_the_enchantment_and_the_thunderstorm() {
        assert_eq!(channeling_hit(true, false).0, 0, "no enchantment");
        assert_eq!(channeling_hit(false, true).0, 0, "no thunderstorm");
        assert_eq!(channeling_hit(false, false).0, 0, "neither");
    }
}
