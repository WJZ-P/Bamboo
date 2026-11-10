//! 地上的物品，官方 `ItemEntity`。
//!
//! 构造时的朝向和初速度取实体自己的随机源，不取 `level.random`。调用方如果随后
//! 盖掉速度，那两次 `nextDouble` 仍然要先取掉。重力、摩擦、流体上浮和每 4 tick
//! 跳过移动都按官方的顺序。
//!
//! A ground item, vanilla `ItemEntity`.
//!
//! Facing and the initial velocity come from the entity's own random source, not
//! `level.random`. Callers that overwrite the velocity still draw those two doubles first.

use std::collections::BTreeMap;

use mc_bamboo_core::{Axis, BlockPos, Direction, Vec3, mth};
use mc_bamboo_data::{
    BlockAncestor, blocks,
    registries::{self, fluid},
    tags::block_tags,
};
use mc_bamboo_protocol::{
    components::{ComponentPatch, ComponentValue, cursor::HolderSet, defaults},
    item::ItemStack,
};

use crate::{
    entity::{Entity, EntityKind, GroundItem},
    fluid::Fluid,
    shape::Aabb,
    state::{BlockState, FluidState, SupportType},
    world::World,
};

/// `ItemEntity.LIFETIME`。
const LIFETIME: i32 = 6000;
/// `ItemEntity.INFINITE_PICKUP_DELAY`。
const INFINITE_PICKUP_DELAY: i32 = 32767;
/// `ItemEntity.INFINITE_LIFETIME`。
const INFINITE_LIFETIME: i32 = -32768;
/// `ItemEntity.DEFAULT_HEALTH`。
const DEFAULT_HEALTH: i32 = 5;
/// `ItemEntity.EYE_HEIGHT`。
const EYE_HEIGHT: f64 = 0.2125;
/// `ItemEntity` 的默认重力。
const GRAVITY: f64 = 0.04;
/// `Entity.getAirDrag` 的默认值。
const AIR_DRAG: f32 = 0.98;
/// 着地且几乎不动时，`(tickCount + id) % 4 != 0` 的那一拍不移动。
const REST_TICKS: i32 = 4;
/// 水平速度平方不超过这个数才算“几乎不动”。官方写的是 `1.0E-5F`。
const REST_SPEED: f64 = 1.0e-5_f32 as f64;
/// `merge` 写死的上限，不是物品自己的最大堆叠数。
const MERGE_LIMIT: i32 = 64;
/// `ServerExplosion.StackCollector.tryMerge` 传给 `ItemEntity.merge` 的上限。
///
/// The cap `ServerExplosion.StackCollector.tryMerge` passes to `ItemEntity.merge`.
const EXPLOSION_MERGE_LIMIT: i32 = 16;
/// 物品宽高都是 0.25，`popResource` 用高度的一半。
const HALF_ITEM_HEIGHT: f64 = 0.125;
/// `Containers.dropItemStack` 里三个 `triangle` 的展开宽度，官方的字面量。
const DROP_SPREAD: f64 = 0.11485000171139836;
/// 火和灵魂火的伤害。`FireBlock` 是 1，`SoulFireBlock` 是 2。
const FIRE_DAMAGE: f32 = 1.0;
const SOUL_FIRE_DAMAGE: f32 = 2.0;
/// `lavaHurt` 的伤害。
const LAVA_DAMAGE: f32 = 4.0;
/// `CactusBlock.entityInside` 的伤害。
const CACTUS_DAMAGE: f32 = 1.0;
/// 岩浆里点燃 15 秒，火里点燃 8 秒。
const LAVA_FIRE_SECONDS: f32 = 15.0;
const FIRE_SECONDS: f32 = 8.0;

/// 这一拍流体扫描的结果。高度是流体顶面减去实体盒子底，和 `getFluidHeight` 一样。
struct FluidScan {
    water: f64,
    lava: f64,
}

impl GroundItem {
    /// 还没取构造随机数。`roll_velocity` 为真时构造函数还会取两次水平速度。
    ///
    /// Random draws have not happened yet. When `roll_velocity` is set, the constructor
    /// also draws the two horizontal speeds.
    pub fn new(stack: ItemStack, roll_velocity: bool) -> Self {
        Self {
            stack,
            age: 0,
            pickup_delay: 0,
            health: DEFAULT_HEALTH,
            bob_offs: 0.0,
            roll_velocity,
            target: None,
        }
    }

    /// `setDefaultPickUpDelay`。
    pub fn set_default_pickup_delay(&mut self) {
        self.pickup_delay = 10;
    }

    /// `setPickUpDelay`。
    pub fn set_pickup_delay(&mut self, delay: i32) {
        self.pickup_delay = delay;
    }
}

impl Entity {
    /// 位置构造函数：`new ItemEntity(level, x, y, z, stack)`。
    ///
    /// 速度的两次 `nextDouble` 在 [`Entities::spawn`](super::Entities::spawn) 分配 ID、
    /// 重设随机源之后才取。
    ///
    /// The two velocity `nextDouble`s are drawn in [`Entities::spawn`](super::Entities::spawn),
    /// after the id is assigned and the random source is reseeded.
    pub fn ground_item(position: Vec3, stack: ItemStack) -> Self {
        Self::new(EntityKind::Item(GroundItem::new(stack, true)), position)
    }

    /// 带速度的构造函数：`new ItemEntity(level, x, y, z, stack, dx, dy, dz)`。
    ///
    /// 不取速度随机数，朝向和 `bobOffs` 仍然取。
    ///
    /// Does not draw the velocity. Facing and `bobOffs` are still drawn.
    pub fn ground_item_with_delta(position: Vec3, stack: ItemStack, delta: Vec3) -> Self {
        let mut entity = Self::new(EntityKind::Item(GroundItem::new(stack, false)), position);
        entity.delta = delta;
        entity
    }

    /// 实体随机源已经按 ID 设好之后，补上构造函数里的取数。
    ///
    /// Draws the constructor's random values once the entity random source has been seeded from the id.
    pub(crate) fn finish_item_random(&mut self) {
        let EntityKind::Item(item) = &self.kind else {
            return;
        };
        let roll_velocity = item.roll_velocity;
        // `bobOffs = nextFloat() * (float)(Math.PI * 2)`，然后 `setYRot(nextFloat() * 360)`。
        // `bobOffs = nextFloat() * (float)(Math.PI * 2)`, then `setYRot(nextFloat() * 360)`.
        let full_turn = (std::f64::consts::PI * 2.0) as f32;
        let bob = self.random.next_float() * full_turn;
        self.y_rot = self.random.next_float() * 360.0;
        if roll_velocity {
            let dx = self.random.next_double() * 0.2 - 0.1;
            let dz = self.random.next_double() * 0.2 - 0.1;
            self.delta = Vec3::new(dx, 0.2, dz);
        }
        if let EntityKind::Item(item) = &mut self.kind {
            item.bob_offs = bob;
        }
    }

    pub fn ground(&self) -> Option<&GroundItem> {
        match &self.kind {
            EntityKind::Item(item) => Some(item),
            _ => None,
        }
    }

    pub fn ground_mut(&mut self) -> Option<&mut GroundItem> {
        match &mut self.kind {
            EntityKind::Item(item) => Some(item),
            _ => None,
        }
    }

    /// `ItemEntity.tick`。调用方已经跑过 `commonTick`（记下旧位置、`tickCount++`）。
    ///
    /// `ItemEntity.tick`. The caller has already run `commonTick` (old position and `tickCount++`).
    pub(crate) fn tick_item(&mut self, world: &mut World) {
        let empty = self.ground().is_some_and(|item| item.stack.is_empty());
        if empty {
            self.discard();
            return;
        }
        let scan = self.item_base_tick(world);
        // 这一拍里被岩浆或火丢掉之后，官方仍会把摩擦、年龄和第二次流体扫描做完。
        // After lava or fire discards the item, vanilla still applies drag, age and the second fluid scan.
        if let Some(item) = self.ground_mut()
            && item.pickup_delay > 0
            && item.pickup_delay != INFINITE_PICKUP_DELAY
        {
            item.pickup_delay -= 1;
        }
        // 官方在移动前把 `xo/yo/zo` 改成当前位置，盖掉 `commonTick` 记下的旧位置。
        // Vanilla overwrites `xo/yo/zo` with the current position before moving.
        self.old_position = self.position;
        // `0.1F` 先是 float，比较时扩成 double。
        // `0.1F` is a float and widens to double in the comparison.
        let in_water = self.in_water && scan.water > f64::from(0.1_f32);
        // `isInLava()` 在 `baseTick` 末尾把 `firstTick` 清掉之后才为真。
        // `isInLava()` becomes true only after `baseTick` clears `firstTick`.
        let in_lava = scan.lava > f64::from(0.1_f32);
        if in_water {
            // `setFluidMovement(0.99F)`：先是 float，再扩成 double，不是 double 字面量 0.99。
            // `setFluidMovement(0.99F)` widens a float. It is not the double literal 0.99.
            self.set_fluid_movement(f64::from(0.99_f32));
        } else if in_lava {
            self.set_fluid_movement(f64::from(0.95_f32));
        } else {
            self.delta.y -= GRAVITY;
        }

        let no_physics = !block_space_free(world, self, self.bounding_box().deflated(1.0e-7));
        if no_physics {
            self.move_towards_closest_space(world);
        }
        let horizontal = self.delta.x * self.delta.x + self.delta.z * self.delta.z;
        let resting = self.on_ground
            && horizontal <= REST_SPEED
            && (self.tick_count + self.id).rem_euclid(REST_TICKS) != 0;
        if resting {
            if !no_physics {
                self.apply_effects_from_blocks(world);
                self.item_hazards(world);
            }
        } else {
            let movement = self.delta;
            self.move_entity(world, movement);
            if !no_physics {
                self.apply_effects_from_blocks(world);
                self.item_hazards(world);
            }
            let mut ground = AIR_DRAG;
            if self.on_ground {
                let below = world.block_state(self.on_pos(world, 0.999999));
                ground *= friction(below);
            }
            self.delta =
                self.delta
                    .multiply(f64::from(ground), f64::from(AIR_DRAG), f64::from(ground));
            if self.on_ground && self.delta.y < 0.0 {
                self.delta.y *= -0.5;
            }
        }
        self.merge_with_neighbours(world);
        if let Some(item) = self.ground_mut()
            && item.age != INFINITE_LIFETIME
        {
            item.age += 1;
        }
        // 移动之后再扫一次流体：溅水的随机数和水流推力都在这里。
        // A second fluid scan after the move: splash rolls and the current live here.
        let _ = self.update_item_fluid(world);
        let expired = self.ground().is_some_and(|item| item.age >= LIFETIME);
        if expired {
            self.discard();
        }
    }

    /// `baseTick` 里物品会碰到的那一段：流体、着火、掉出世界。
    ///
    /// The part of `baseTick` an item actually reaches: fluid, fire, and falling out of the world.
    fn item_base_tick(&mut self, world: &mut World) -> FluidScan {
        let first = self.first_tick;
        let scan = self.update_item_fluid(world);
        if self.remaining_fire_ticks > 0 {
            if self.item_fire_immune() {
                self.remaining_fire_ticks = 0;
            } else {
                let in_lava = !first && scan.lava > 0.0;
                if self.remaining_fire_ticks % 20 == 0 && !in_lava {
                    self.hurt_item(DamageKind::OnFire, 1.0);
                }
                if !self.removed {
                    self.remaining_fire_ticks -= 1;
                }
            }
        }
        if self.removed {
            return scan;
        }
        if !first && scan.lava > 0.0 {
            self.fall_distance *= 0.5;
        }
        if self.position.y < f64::from(world.min_y() - 64) {
            self.discard();
        }
        self.first_tick = false;
        scan
    }

    /// `setFluidMovement`。`y < 0.06F` 的比较发生在 float 上。
    ///
    /// `setFluidMovement`. The `y < 0.06F` test happens on a float.
    fn set_fluid_movement(&mut self, multiplier: f64) {
        let y = self.delta.y;
        // `5.0E-4F` 先按 float 取值，再加到 double 的 y 上。
        // `5.0E-4F` is a float, then added to the double y.
        let lift = if (y as f32) < 0.06 {
            f64::from(5.0e-4_f32)
        } else {
            0.0
        };
        self.delta = Vec3::new(
            self.delta.x * multiplier,
            y + lift,
            self.delta.z * multiplier,
        );
    }

    /// `moveTowardsClosestSpace`。卡在实心方块里时朝最近的空面推一把，并取一次 `nextFloat`。
    ///
    /// `moveTowardsClosestSpace`. A nudge toward the nearest open face, plus one `nextFloat`.
    fn move_towards_closest_space(&mut self, world: &World) {
        let y = (self.bounding_box().min_y + self.bounding_box().max_y) / 2.0;
        let pos = BlockPos::containing(Vec3::new(self.position.x, y, self.position.z));
        let delta = Vec3::new(
            self.position.x - f64::from(pos.x),
            y - f64::from(pos.y),
            self.position.z - f64::from(pos.z),
        );
        let order = [
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
            Direction::Up,
        ];
        let mut closest = Direction::Up;
        let mut closest_distance = f64::MAX;
        for direction in order {
            let neighbor = pos.relative(direction);
            if world.block_state(neighbor).is_collision_shape_full_block() {
                continue;
            }
            let component = match direction.axis() {
                Axis::X => delta.x,
                Axis::Y => delta.y,
                Axis::Z => delta.z,
            };
            let oriented = if direction.is_positive() {
                1.0 - component
            } else {
                component
            };
            if oriented < closest_distance {
                closest_distance = oriented;
                closest = direction;
            }
        }
        let speed = self.random.next_float() * 0.2 + 0.1;
        let step = if closest.is_positive() { 1.0 } else { -1.0 };
        let scaled = self.delta.scale(0.75);
        let pushed = f64::from(step * speed);
        self.delta = match closest.axis() {
            Axis::X => Vec3::new(pushed, scaled.y, scaled.z),
            Axis::Y => Vec3::new(scaled.x, pushed, scaled.z),
            Axis::Z => Vec3::new(scaled.x, scaled.y, pushed),
        };
    }

    /// `isMergable` 为真、并且到了合并周期时，把周围同类物品并进来。
    ///
    /// When `isMergable` and the merge period has elapsed, pull in matching neighbours.
    fn merge_with_neighbours(&mut self, world: &mut World) {
        if !self.is_mergable() {
            return;
        }
        let moved = mth::floor(self.old_position.x) != mth::floor(self.position.x)
            || mth::floor(self.old_position.y) != mth::floor(self.position.y)
            || mth::floor(self.old_position.z) != mth::floor(self.position.z);
        let rate = if moved { 2 } else { 40 };
        if self.tick_count % rate != 0 {
            return;
        }
        let area = self.bounding_box().inflate(0.5, 0.0, 0.5);
        let others = item_ids_in(world, area, self.id);
        for other_id in others {
            if self.removed || !self.is_mergable() {
                break;
            }
            self.try_merge(world, other_id);
        }
    }

    fn is_mergable(&self) -> bool {
        let Some(item) = self.ground() else {
            return false;
        };
        !self.removed
            && item.pickup_delay != INFINITE_PICKUP_DELAY
            && item.age != INFINITE_LIFETIME
            && item.age < LIFETIME
            && item.stack.count < item.stack.max_stack_size()
    }

    /// `tryToMerge`。数量少的并进数量多的；一样多时并进对方。
    ///
    /// `tryToMerge`. The smaller stack merges into the larger one; equal counts merge into the other.
    fn try_merge(&mut self, world: &mut World, other_id: i32) {
        let Some(other) = world.entity(other_id) else {
            return;
        };
        if other.removed || !other.is_mergable() {
            return;
        }
        let (Some(mine), Some(theirs)) = (self.ground(), other.ground()) else {
            return;
        };
        if mine.target != theirs.target || !stacks_can_merge(&mine.stack, &theirs.stack) {
            return;
        }
        let merge_into_other = theirs.stack.count >= mine.stack.count;
        if merge_into_other {
            let mut to_stack = theirs.stack.clone();
            let mut from_stack = mine.stack.clone();
            let to_delay = theirs.pickup_delay;
            let to_age = theirs.age;
            let from_delay = mine.pickup_delay;
            let from_age = mine.age;
            merge_stacks(&mut to_stack, &mut from_stack);
            let delay = to_delay.max(from_delay);
            let age = to_age.min(from_age);
            world.with_entity_mut(other_id, &mut |other| {
                if let Some(other) = other.ground_mut() {
                    other.stack = to_stack.clone();
                    other.pickup_delay = delay;
                    other.age = age;
                }
            });
            if let Some(mine) = self.ground_mut() {
                mine.stack = from_stack.clone();
                if from_stack.is_empty() {
                    self.discard();
                }
            }
        } else {
            let mut to_stack = mine.stack.clone();
            let mut from_stack = theirs.stack.clone();
            let delay = mine.pickup_delay.max(theirs.pickup_delay);
            let age = mine.age.min(theirs.age);
            merge_stacks(&mut to_stack, &mut from_stack);
            if let Some(mine) = self.ground_mut() {
                mine.stack = to_stack;
                mine.pickup_delay = delay;
                mine.age = age;
            }
            let empty = from_stack.is_empty();
            world.with_entity_mut(other_id, &mut |other| {
                if let Some(item) = other.ground_mut() {
                    item.stack = from_stack.clone();
                }
                if empty {
                    other.discard();
                }
            });
        }
    }

    /// 仙人掌、火和岩浆。`noPhysics` 时官方的 `isAffectedByBlocks` 为假，这里也不做。
    ///
    /// Cactus, fire, and lava. Vanilla skips these while `noPhysics` is set.
    fn item_hazards(&mut self, world: &mut World) {
        if self.removed {
            return;
        }
        let bounds = self.bounding_box().deflated(1.0e-5);
        let min = BlockPos::new(
            mth::floor(bounds.min_x),
            mth::floor(bounds.min_y),
            mth::floor(bounds.min_z),
        );
        let max = BlockPos::new(
            mth::floor(bounds.max_x),
            mth::floor(bounds.max_y),
            mth::floor(bounds.max_z),
        );
        let mut lava = false;
        let mut fires: Vec<(BlockPos, f32)> = Vec::new();
        let mut cactus = false;
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                for x in min.x..=max.x {
                    let pos = BlockPos::new(x, y, z);
                    let state = world.block_state(pos);
                    if state.is(blocks::CACTUS) {
                        cactus = true;
                    } else if state.is(blocks::SOUL_FIRE) {
                        fires.push((pos, SOUL_FIRE_DAMAGE));
                    } else if state.is(blocks::FIRE) || state.is_a(BlockAncestor::BaseFireBlock) {
                        fires.push((pos, FIRE_DAMAGE));
                    }
                    let fluid_state = state.fluid();
                    if fluid_state.is_tagged(mc_bamboo_data::tags::fluid_tags::LAVA)
                        && fluid_box(world, pos, fluid_state)
                            .is_some_and(|fluid| bounds.intersects(fluid))
                    {
                        lava = true;
                    }
                }
            }
        }
        if lava {
            self.lava_hurt(world);
        }
        if self.removed {
            return;
        }
        for (_, damage) in fires {
            self.fire_ignite();
            if self.removed {
                return;
            }
            self.hurt_item(DamageKind::InFire, damage);
            if self.removed {
                return;
            }
        }
        if cactus {
            self.hurt_item(DamageKind::Cactus, CACTUS_DAMAGE);
        }
        if !self.removed {
            crate::hopper::suck_touching_item(world, self);
        }
    }

    /// `BaseFireBlock.fireIgnite`。物品不是 `ServerPlayer`，不会额外取 `level.random`。
    ///
    /// `BaseFireBlock.fireIgnite`. An item is not a `ServerPlayer`, so this draws no `level.random`.
    fn fire_ignite(&mut self) {
        if self.item_fire_immune() {
            return;
        }
        if self.remaining_fire_ticks < 0 {
            self.remaining_fire_ticks += 1;
        }
        if self.remaining_fire_ticks >= 0 {
            self.ignite_for_seconds(FIRE_SECONDS);
        }
    }

    /// `Entity.lavaIgnite` 然后 `lavaHurt`。
    ///
    /// `Entity.lavaIgnite` followed by `lavaHurt`.
    fn lava_hurt(&mut self, _world: &mut World) {
        if self.item_fire_immune() {
            return;
        }
        self.ignite_for_seconds(LAVA_FIRE_SECONDS);
        let hurt = self.hurt_item(DamageKind::Lava, LAVA_DAMAGE);
        let health = self.ground().map(|item| item.health).unwrap_or(0);
        // 音效的音高取一次 `nextFloat`。这里不播声音，数还是要取。
        // The burn sound draws one `nextFloat` for its pitch. The roll stays even though the sound does not play.
        if hurt && (health <= 0 || self.tick_count % 10 == 0) {
            let _pitch = self.random.next_float();
        }
    }

    fn ignite_for_seconds(&mut self, seconds: f32) {
        let ticks = mth::floor(f64::from(seconds * 20.0));
        if self.remaining_fire_ticks < ticks {
            self.remaining_fire_ticks = ticks;
        }
    }

    /// `ItemEntity.hurtServer`。没有生物伤害来源，`MOB_GRIEFING` 那条用不上。
    ///
    /// `ItemEntity.hurtServer`. No mob can be the source, so the `MOB_GRIEFING` check does not apply.
    fn hurt_item(&mut self, kind: DamageKind, damage: f32) -> bool {
        let Some(item) = self.ground() else {
            return false;
        };
        if !can_be_hurt_by(&item.stack, kind) {
            return false;
        }
        let health = item.health;
        // `(int)(health - damage)`：先按 float 相减，再向零截断。
        // `(int)(health - damage)`: subtract as float, then truncate toward zero.
        let next = (health as f32 - damage) as i32;
        if let Some(item) = self.ground_mut() {
            item.health = next;
            if next <= 0 {
                self.discard();
            }
        }
        true
    }

    /// `ItemEntity.fireImmune`：物品扛得住 `inFire`，或者实体类型本身免疫（物品不是）。
    ///
    /// `ItemEntity.fireImmune`: the stack resists `inFire`. The item entity type itself is not immune.
    fn item_fire_immune(&self) -> bool {
        self.ground()
            .is_some_and(|item| !can_be_hurt_by(&item.stack, DamageKind::InFire))
    }

    /// `EntityFluidInteraction.update` 加上进水时的溅水随机数。
    ///
    /// `EntityFluidInteraction.update`, plus the splash rolls when the item enters water.
    fn update_item_fluid(&mut self, world: &World) -> FluidScan {
        let was_touching = self.in_water;
        let scan = scan_fluid(world, self);
        if scan.heights.water > 0.0 {
            self.fall_distance = 0.0;
            if !was_touching && !self.first_tick {
                splash(self);
            }
        }
        self.in_water = scan.heights.water > 0.0;
        if scan.heights.water > 0.0 {
            apply_current(self, &scan.water_flow, 0.014);
        }
        if scan.heights.lava > 0.0 {
            // 主世界的岩浆不是 `FAST_LAVA`。
            // Overworld lava is not `FAST_LAVA`.
            apply_current(self, &scan.lava_flow, 0.0023333333333333335);
        }
        scan.heights
    }
}

struct FluidDetail {
    heights: FluidScan,
    water_flow: Vec3,
    lava_flow: Vec3,
}

/// 伤害种类，对应物品能不能被这一种 `DamageType` 打掉。
///
/// Which `DamageType` is being applied, for `ItemStack.canBeHurtBy`.
#[derive(Clone, Copy)]
enum DamageKind {
    InFire,
    OnFire,
    Lava,
    Cactus,
    Explosion,
}

impl DamageKind {
    fn name(self) -> &'static str {
        match self {
            Self::InFire => "minecraft:in_fire",
            Self::OnFire => "minecraft:on_fire",
            Self::Lava => "minecraft:lava",
            Self::Cactus => "minecraft:cactus",
            Self::Explosion => "minecraft:explosion",
        }
    }
}

/// `ItemStack.canBeHurtBy`：带 `damage_resistant` 且类型集合包含这个伤害时不受伤。
///
/// `ItemStack.canBeHurtBy`: a `damage_resistant` set that contains the damage type blocks it.
fn can_be_hurt_by(stack: &ItemStack, kind: DamageKind) -> bool {
    let Ok(patch) = ComponentPatch::from_opaque(stack) else {
        return true;
    };
    let Some(ComponentValue::DamageResistant(set)) = defaults::resolve(
        stack.item,
        &patch,
        registries::data_component_type::DAMAGE_RESISTANT,
    ) else {
        return true;
    };
    !holder_contains(set, damage_type_id(kind.name()))
}

fn holder_contains(set: &HolderSet, id: i32) -> bool {
    match set {
        HolderSet::Ids(ids) => ids.contains(&id),
        HolderSet::Tag(name) => {
            let name = name.trim_start_matches('#');
            damage_tag(name).contains(&id)
        }
    }
}

fn damage_type_id(name: &str) -> i32 {
    registries::SYNCED_REGISTRIES
        .iter()
        .find(|registry| registry.name == "minecraft:damage_type")
        .and_then(|registry| registry.entries.iter().position(|entry| *entry == name))
        .unwrap_or(0) as i32
}

fn damage_tag(name: &str) -> &'static [i32] {
    mc_bamboo_data::tags::TAG_GROUPS
        .iter()
        .find(|group| group.registry == "minecraft:damage_type")
        .and_then(|group| group.tags.iter().find(|tag| tag.name == name))
        .map(|tag| tag.ids)
        .unwrap_or(&[])
}

fn friction(state: BlockState) -> f32 {
    if state.is(blocks::ICE) || state.is(blocks::PACKED_ICE) || state.is(blocks::FROSTED_ICE) {
        0.98
    } else if state.is(blocks::BLUE_ICE) {
        0.989
    } else if state.is(blocks::SLIME_BLOCK) {
        0.8
    } else {
        0.6
    }
}

fn block_space_free(world: &World, entity: &Entity, bounds: Aabb) -> bool {
    world
        .block_collisions(bounds, entity.collision_context())
        .is_empty()
}

/// `areMergable`：加起来不超过对方的最大堆叠，而且物品和组件都相同。
///
/// `areMergable`: the combined count fits in the other stack, and the item plus components match.
fn stacks_can_merge(this_stack: &ItemStack, other_stack: &ItemStack) -> bool {
    if other_stack.count + this_stack.count > other_stack.max_stack_size() {
        return false;
    }
    this_stack.is_same_item_same_components(other_stack)
}

/// `ItemEntity.merge`。上限是 64，多出来的留在 `from`。
///
/// `ItemEntity.merge`. The cap is 64; anything past it stays on `from`.
fn merge_stacks(to_stack: &mut ItemStack, from_stack: &mut ItemStack) {
    move_into(to_stack, from_stack, MERGE_LIMIT);
}

/// `ItemEntity.merge`，上限是爆炸收集堆的 16。合并不了时两边都不动。
///
/// `ItemEntity.merge` with the explosion collector's cap of 16. Nothing moves when the stacks
/// cannot merge.
pub(crate) fn merge_explosion(to_stack: &mut ItemStack, from_stack: &mut ItemStack) {
    if !stacks_can_merge(to_stack, from_stack) {
        return;
    }
    move_into(to_stack, from_stack, EXPLOSION_MERGE_LIMIT);
}

fn move_into(to_stack: &mut ItemStack, from_stack: &mut ItemStack, limit: i32) {
    let room = (to_stack.max_stack_size().min(limit) - to_stack.count).max(0);
    let moved = room.min(from_stack.count);
    if moved == 0 {
        return;
    }
    to_stack.count += moved;
    from_stack.count -= moved;
    if from_stack.count <= 0 {
        *from_stack = ItemStack::empty();
    }
}

fn item_ids_in(world: &World, area: Aabb, except: i32) -> Vec<i32> {
    world
        .entity_ids()
        .into_iter()
        .filter(|id| *id != except)
        .filter(|id| {
            world.entity(*id).is_some_and(|entity| {
                entity.ground().is_some()
                    && !entity.removed
                    && entity.bounding_box().intersects(area)
            })
        })
        .collect()
}

fn splash(entity: &mut Entity) {
    // 音高两个 `nextFloat`，然后两组粒子循环。宽 0.25 时每组 6 次。
    // Two `nextFloat`s for the pitch, then the two particle loops. Width 0.25 means six each.
    let _ = entity.random.next_float();
    let _ = entity.random.next_float();
    let width = entity.kind.dimensions().0;
    let limit = 1.0 + width * 20.0;
    let mut index = 0;
    while (index as f32) < limit {
        let _ = entity.random.next_double();
        let _ = entity.random.next_double();
        let _ = entity.random.next_double();
        index += 1;
    }
    index = 0;
    while (index as f32) < limit {
        let _ = entity.random.next_double();
        let _ = entity.random.next_double();
        index += 1;
    }
}

fn scan_fluid(world: &World, entity: &Entity) -> FluidDetail {
    let empty = FluidDetail {
        heights: FluidScan {
            water: 0.0,
            lava: 0.0,
        },
        water_flow: Vec3::default(),
        lava_flow: Vec3::default(),
    };
    let Some(bounds) = fluid_interaction_box(entity) else {
        return empty;
    };
    let x0 = mth::floor(bounds.min_x);
    let y0 = mth::floor(bounds.min_y);
    let z0 = mth::floor(bounds.min_z);
    let x1 = mth::ceil(bounds.max_x) - 1;
    let y1 = mth::ceil(bounds.max_y) - 1;
    let z1 = mth::ceil(bounds.max_z) - 1;
    if !fluid_area_loaded(world, x0 - 1, y0, z0 - 1, x1 + 1, y1, z1 + 1) {
        return empty;
    }
    let entity_y = entity.bounding_box().min_y;
    let mut heights: BTreeMap<i32, f64> = BTreeMap::new();
    let mut water_flow = Vec3::default();
    let mut lava_flow = Vec3::default();
    let mut water_current_height = 0.0_f64;
    let mut lava_current_height = 0.0_f64;
    let mut water_count = 0;
    let mut lava_count = 0;
    for x in x0..=x1 {
        for y in y0..=y1 {
            for z in z0..=z1 {
                let pos = BlockPos::new(x, y, z);
                let fluid = world.fluid_state(pos);
                if fluid.is_empty() {
                    continue;
                }
                let top = f64::from(pos.y) + f64::from(crate::fluid::height(fluid, world, pos));
                if top < bounds.min_y {
                    continue;
                }
                let kind = fluid.fluid();
                let submerged = top - entity_y;
                let tracker = heights.entry(kind).or_insert(0.0);
                *tracker = tracker.max(submerged);
                let tracker_height = *tracker;
                let flow = flow_vector(world, pos, fluid);
                if fluid.is_tagged(mc_bamboo_data::tags::fluid_tags::WATER) {
                    water_current_height = water_current_height.max(tracker_height);
                    let scaled = scale_flow(flow, water_current_height);
                    water_flow = water_flow.add(scaled);
                    water_count += 1;
                } else if fluid.is_tagged(mc_bamboo_data::tags::fluid_tags::LAVA) {
                    lava_current_height = lava_current_height.max(tracker_height);
                    let scaled = scale_flow(flow, lava_current_height);
                    lava_flow = lava_flow.add(scaled);
                    lava_count += 1;
                }
            }
        }
    }
    let water = tag_height(&heights, true);
    let lava = tag_height(&heights, false);
    FluidDetail {
        heights: FluidScan { water, lava },
        water_flow: if water_count == 0 {
            Vec3::default()
        } else {
            water_flow
        },
        lava_flow: if lava_count == 0 {
            Vec3::default()
        } else {
            lava_flow
        },
    }
}

fn tag_height(heights: &BTreeMap<i32, f64>, water: bool) -> f64 {
    heights
        .iter()
        .filter(|(kind, _)| {
            let state_is_water = **kind == fluid::WATER || **kind == fluid::FLOWING_WATER;
            if water {
                state_is_water
            } else {
                !state_is_water && **kind != fluid::EMPTY
            }
        })
        .map(|(_, height)| *height)
        .fold(0.0, f64::max)
}

fn scale_flow(flow: Vec3, height: f64) -> Vec3 {
    if height < 0.4 {
        flow.scale(height)
    } else {
        flow
    }
}

fn apply_current(entity: &mut Entity, accumulated: &Vec3, scale: f64) {
    if accumulated.length_sqr() < f64::from(1.0e-5_f32) {
        return;
    }
    let mut impulse = accumulated.normalize().scale(scale);
    let slow = entity.delta.x.abs() < 0.003 && entity.delta.z.abs() < 0.003;
    if slow && impulse.length() < 0.0045000000000000005 {
        impulse = impulse.normalize().scale(0.0045000000000000005);
    }
    entity.delta = entity.delta.add(impulse);
}

fn fluid_interaction_box(entity: &Entity) -> Option<Aabb> {
    Some(entity.bounding_box().deflated(0.001))
}

fn fluid_area_loaded(world: &World, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) -> bool {
    let mut x = x0;
    while x <= x1 {
        let mut z = z0;
        while z <= z1 {
            if !world.is_loaded(BlockPos::new(x, y0, z))
                || !world.is_loaded(BlockPos::new(x, y1, z))
            {
                return false;
            }
            z += 16;
        }
        x += 16;
    }
    true
}

/// `FlowingFluid.getFlow`。
///
/// `FlowingFluid.getFlow`.
fn flow_vector(world: &World, pos: BlockPos, fluid: FluidState) -> Vec3 {
    let Some(kind) = Fluid::of(fluid) else {
        return Vec3::default();
    };
    let own = fluid.own_height();
    let mut flow_x = 0.0;
    let mut flow_z = 0.0;
    for direction in Direction::HORIZONTAL {
        let neighbor = pos.relative(direction);
        let neighbour_fluid = world.fluid_state(neighbor);
        if !affects_flow(kind, neighbour_fluid) {
            continue;
        }
        let mut neighbor_height = neighbour_fluid.own_height();
        let mut distance = 0.0_f32;
        if neighbor_height == 0.0 {
            if !world
                .block_state(neighbor)
                .is_tagged(block_tags::BLOCKS_FLUID_FLOW)
            {
                let below = neighbor.below();
                let below_fluid = world.fluid_state(below);
                if affects_flow(kind, below_fluid) {
                    neighbor_height = below_fluid.own_height();
                    if neighbor_height > 0.0 {
                        distance = own - (neighbor_height - 0.8888889);
                    }
                }
            }
        } else if neighbor_height > 0.0 {
            distance = own - neighbor_height;
        }
        if distance != 0.0 {
            let (step_x, _, step_z) = direction.normal();
            flow_x += f64::from(step_x) * f64::from(distance);
            flow_z += f64::from(step_z) * f64::from(distance);
        }
    }
    let mut flow = Vec3::new(flow_x, 0.0, flow_z);
    if fluid.is_falling() {
        for direction in Direction::HORIZONTAL {
            let neighbor = pos.relative(direction);
            if solid_face(world, kind, neighbor, direction)
                || solid_face(world, kind, neighbor.above(), direction)
            {
                flow = flow.normalize().add(Vec3::new(0.0, -6.0, 0.0));
                break;
            }
        }
    }
    flow.normalize()
}

fn affects_flow(kind: Fluid, neighbour: FluidState) -> bool {
    neighbour.is_empty() || Fluid::of(neighbour) == Some(kind)
}

fn solid_face(world: &World, kind: Fluid, pos: BlockPos, direction: Direction) -> bool {
    if Fluid::of(world.fluid_state(pos)) == Some(kind) {
        return false;
    }
    if direction == Direction::Up {
        return true;
    }
    let state = world.block_state(pos);
    if state.is_a(BlockAncestor::IceBlock) {
        return false;
    }
    world.is_face_sturdy(pos, direction, SupportType::Full)
}

fn fluid_box(world: &World, pos: BlockPos, fluid: FluidState) -> Option<Aabb> {
    if fluid.is_empty() {
        return None;
    }
    let height = f64::from(crate::fluid::height(fluid, world, pos));
    if height <= 0.0 {
        return None;
    }
    Some(Aabb::new(
        f64::from(pos.x),
        f64::from(pos.y),
        f64::from(pos.z),
        f64::from(pos.x + 1),
        f64::from(pos.y) + height,
        f64::from(pos.z + 1),
    ))
}

impl World<'_> {
    /// `Block.popResource`。位置的三次 `nextDouble` 无条件取，然后才看 `BLOCK_DROPS`
    /// 和物品是不是空的：私有 `popResource(level, supplier, stack)` 只在
    /// `ServerLevel && !stack.isEmpty() && gameRules.get(BLOCK_DROPS)` 时调 `supplier.get()`，
    /// 也就是物品实体的构造（速度两个 `nextDouble`）、拾取延迟和 `addFreshEntity`。
    ///
    /// `Block.popResource`. Three position `nextDouble`s are always drawn. The private
    /// `popResource(level, supplier, stack)` only calls `supplier.get()` (the ItemEntity
    /// constructor, its two velocity doubles, pickup delay and `addFreshEntity`) when
    /// `ServerLevel && !stack.isEmpty() && gameRules.get(BLOCK_DROPS)`.
    pub fn pop_resource(&mut self, pos: BlockPos, stack: ItemStack) {
        let x = f64::from(pos.x) + 0.5 + self.random().mth_next_double(-0.25, 0.25);
        let y =
            f64::from(pos.y) + 0.5 + self.random().mth_next_double(-0.25, 0.25) - HALF_ITEM_HEIGHT;
        let z = f64::from(pos.z) + 0.5 + self.random().mth_next_double(-0.25, 0.25);
        if self.block_game_rules().block_drops {
            self.spawn_dropped_item(Vec3::new(x, y, z), stack, true, 10);
        }
    }

    /// `Block.popResourceFromFace`。位置和速度的随机数都无条件按面取，
    /// 但只有 `BLOCK_DROPS` 为真且物品非空时才生成实体。
    ///
    /// `Block.popResourceFromFace`. All position and velocity draws happen for the face,
    /// but the entity only spawns when `BLOCK_DROPS` is on and the stack is non-empty.
    pub fn pop_resource_from_face(&mut self, pos: BlockPos, face: Direction, stack: ItemStack) {
        let (step_x, step_y, step_z) = face.normal();
        let random_x = if step_x == 0 {
            Some(self.random().mth_next_double(-0.25, 0.25))
        } else {
            None
        };
        let random_y = if step_y == 0 {
            Some(self.random().mth_next_double(-0.25, 0.25))
        } else {
            None
        };
        let random_z = if step_z == 0 {
            Some(self.random().mth_next_double(-0.25, 0.25))
        } else {
            None
        };
        let delta_x = if step_x == 0 {
            self.random().mth_next_double(-0.1, 0.1)
        } else {
            f64::from(step_x) * 0.1
        };
        let delta_y = if step_y == 0 {
            self.random().mth_next_double(0.0, 0.1)
        } else {
            f64::from(step_y) * 0.1 + 0.1
        };
        let delta_z = if step_z == 0 {
            self.random().mth_next_double(-0.1, 0.1)
        } else {
            f64::from(step_z) * 0.1
        };
        let x = f64::from(pos.x)
            + 0.5
            + random_x.unwrap_or(f64::from(step_x) * (0.5 + HALF_ITEM_HEIGHT));
        let y = f64::from(pos.y)
            + 0.5
            + random_y.unwrap_or(f64::from(step_y) * (0.5 + HALF_ITEM_HEIGHT))
            - HALF_ITEM_HEIGHT;
        let z = f64::from(pos.z)
            + 0.5
            + random_z.unwrap_or(f64::from(step_z) * (0.5 + HALF_ITEM_HEIGHT));
        if self.block_game_rules().block_drops {
            let id = self.spawn_dropped_item(Vec3::new(x, y, z), stack, false, 10);
            self.with_entity_mut(id, &mut |entity| {
                entity.delta = Vec3::new(delta_x, delta_y, delta_z);
            });
        }
    }

    /// `Containers.dropItemStack`。空堆只取掉落点的三个 double。
    ///
    /// `Containers.dropItemStack`. An empty stack only draws the three doubles for the drop point.
    pub fn drop_container_stack(&mut self, x: f64, y: f64, z: f64, mut stack: ItemStack) {
        let width = 0.25_f64;
        let center = 1.0 - width;
        let half = width / 2.0;
        let xo = x.floor() + self.random().next_double() * center + half;
        let yo = y.floor() + self.random().next_double() * center;
        let zo = z.floor() + self.random().next_double() * center + half;
        while !stack.is_empty() {
            let amount = (self.random().next_int(21) + 10).min(stack.count);
            let piece = split_stack(&mut stack, amount);
            let id = self.spawn_dropped_item(Vec3::new(xo, yo, zo), piece, true, 0);
            let delta = Vec3::new(
                self.random().triangle(0.0, DROP_SPREAD),
                self.random().triangle(0.2, DROP_SPREAD),
                self.random().triangle(0.0, DROP_SPREAD),
            );
            self.with_entity_mut(id, &mut |entity| entity.delta = delta);
        }
    }

    /// `DefaultDispenseItemBehavior.spawnItem`，速度随机数由调用方提前取好。
    ///
    /// 官方先算好落点，再取一个 `nextDouble` 和三个 `triangle` 的六个 double，
    /// 然后才 `addFreshEntity`。分两步是为了让注册行为保持原来的取数顺序，
    /// 物品实体仍按这个速度生成。
    ///
    /// `DefaultDispenseItemBehavior.spawnItem` with velocity draws already taken by the caller.
    ///
    /// Vanilla computes the spawn point, draws one `nextDouble` and the six doubles of three
    /// `triangle`s, and only then calls `addFreshEntity`. Splitting the draws from the spawn
    /// keeps the official order for behaviours that draw them without creating an entity.
    pub fn spawn_dispensed_item_with_velocity(
        &mut self,
        stack: ItemStack,
        direction: Direction,
        position: Vec3,
        velocity: Vec3,
    ) {
        if stack.is_empty() {
            return;
        }
        let mut spawn = position;
        spawn.y -= if direction.axis() == Axis::Y {
            0.125
        } else {
            0.15625
        };
        let id = self.spawn_dropped_item(spawn, stack, true, 0);
        self.with_entity_mut(id, &mut |entity| entity.delta = velocity);
    }

    /// `Entity.spawnAtLocation`：用物品自己的随机速度，拾取延迟 10。
    ///
    /// `Entity.spawnAtLocation`: the new item's own random velocity, pickup delay 10.
    pub fn spawn_at_location(&mut self, position: Vec3, stack: ItemStack) -> Option<i32> {
        if stack.is_empty() {
            return None;
        }
        Some(self.spawn_dropped_item(position, stack, true, 10))
    }

    fn spawn_dropped_item(
        &mut self,
        position: Vec3,
        stack: ItemStack,
        roll_velocity: bool,
        pickup_delay: i32,
    ) -> i32 {
        if stack.is_empty() {
            return 0;
        }
        let entity = if roll_velocity {
            Entity::ground_item(position, stack)
        } else {
            Entity::ground_item_with_delta(position, stack, Vec3::default())
        };
        let id = self.spawn_entity(entity);
        if pickup_delay != 0 {
            self.with_entity_mut(id, &mut |entity| {
                if let Some(item) = entity.ground_mut() {
                    item.set_pickup_delay(pickup_delay);
                }
            });
        }
        id
    }
}

/// 爆炸打到物品。`ItemEntity.ignoreExplosion` 在 TNT（会破坏方块）时不忽略。
///
/// Explosion damage against items. `ItemEntity.ignoreExplosion` does not ignore a TNT blast.
pub(crate) fn hurt_items_by_explosion(world: &mut World, center: Vec3, radius: f32) {
    // `hurtEntities` 在半径小于 `1.0E-5F` 时整段跳过。
    // `hurtEntities` skips the whole pass when the radius is below `1.0E-5F`.
    if radius < 1.0e-5 {
        return;
    }
    let double_radius = f64::from(radius * 2.0);
    let x0 = f64::from(mth::floor(center.x - double_radius - 1.0));
    let y0 = f64::from(mth::floor(center.y - double_radius - 1.0));
    let z0 = f64::from(mth::floor(center.z - double_radius - 1.0));
    let x1 = f64::from(mth::floor(center.x + double_radius + 1.0));
    let y1 = f64::from(mth::floor(center.y + double_radius + 1.0));
    let z1 = f64::from(mth::floor(center.z + double_radius + 1.0));
    let area = Aabb::new(x0, y0, z0, x1, y1, z1);
    let ids = world.entity_ids();
    for id in ids {
        let prepared = {
            let Some(entity) = world.entity(id) else {
                continue;
            };
            if entity.ground().is_none()
                || entity.removed
                || !entity.bounding_box().intersects(area)
            {
                continue;
            }
            // 距离用脚底，击退方向的 y 用眼睛。
            // Distance uses the feet. The knockback direction uses the eye on y.
            let dist = entity.position.distance_to_sqr(center).sqrt() / double_radius;
            if dist > 1.0 {
                continue;
            }
            let exposure = seen_percent(world, center, &entity);
            let power = (1.0 - dist) * f64::from(exposure);
            let amount = ((power * power + power) / 2.0 * 7.0 * double_radius + 1.0) as f32;
            let eye_y = entity.position.y + EYE_HEIGHT;
            let direction = Vec3::new(
                entity.position.x - center.x,
                eye_y - center.y,
                entity.position.z - center.z,
            )
            .normalize();
            Some((amount, direction.scale(power)))
        };
        let Some((amount, knock)) = prepared else {
            continue;
        };
        world.with_entity_mut(id, &mut |entity| {
            // 免疫的物品 `hurtServer` 返回 false，击退仍然加上。
            // An immune stack makes `hurtServer` return false. The knockback is still applied.
            entity.hurt_item(DamageKind::Explosion, amount);
            entity.delta = entity.delta.add(knock);
        });
    }
}

/// `ServerExplosion.getSeenPercent`：从物品盒子里取样，看有几条线没被方块挡住。
///
/// `ServerExplosion.getSeenPercent`: sample the item's box and count rays that miss blocks.
fn seen_percent(world: &World, center: Vec3, entity: &Entity) -> f32 {
    let bounds = entity.bounding_box();
    let span_x = bounds.max_x - bounds.min_x;
    let span_y = bounds.max_y - bounds.min_y;
    let span_z = bounds.max_z - bounds.min_z;
    let xs = 1.0 / (span_x * 2.0 + 1.0);
    let ys = 1.0 / (span_y * 2.0 + 1.0);
    let zs = 1.0 / (span_z * 2.0 + 1.0);
    if xs < 0.0 || ys < 0.0 || zs < 0.0 {
        return 0.0;
    }
    let x_offset = (1.0 - (1.0 / xs).floor() * xs) / 2.0;
    let z_offset = (1.0 - (1.0 / zs).floor() * zs) / 2.0;
    let mut hits = 0;
    let mut count = 0;
    let mut xx = 0.0;
    while xx <= 1.0 {
        let mut yy = 0.0;
        while yy <= 1.0 {
            let mut zz = 0.0;
            while zz <= 1.0 {
                let x = mth::lerp(xx, bounds.min_x, bounds.max_x);
                let y = mth::lerp(yy, bounds.min_y, bounds.max_y);
                let z = mth::lerp(zz, bounds.min_z, bounds.max_z);
                let from = Vec3::new(x + x_offset, y, z + z_offset);
                if world
                    .clip_collider(from, center, entity.collision_context())
                    .is_none()
                {
                    hits += 1;
                }
                count += 1;
                zz += zs;
            }
            yy += ys;
        }
        xx += xs;
    }
    if count == 0 {
        0.0
    } else {
        hits as f32 / count as f32
    }
}

fn split_stack(stack: &mut ItemStack, amount: i32) -> ItemStack {
    let taken = amount.min(stack.count).max(0);
    let mut piece = stack.clone();
    piece.count = taken;
    stack.count -= taken;
    if stack.count <= 0 {
        *stack = ItemStack::empty();
    }
    piece
}

/// 玩家捡起时用。延迟没到、或者指定了别人，就捡不起来。
///
/// Used when a player tries to pick the item up. A delay, or another owner's id, refuses it.
pub fn can_pick_up(entity: &Entity, player: &[u8; 16]) -> bool {
    let Some(item) = entity.ground() else {
        return false;
    };
    if entity.removed || item.pickup_delay != 0 || item.stack.is_empty() {
        return false;
    }
    item.target.is_none_or(|target| target == *player)
}

/// 捡起之后把剩下的数量写回去。空了就丢掉实体。返回这次动画用的原始数量。
///
/// Writes the leftover count back after a pickup. An empty stack discards the entity.
pub fn take_stack(entity: &mut Entity, stack: ItemStack) -> i32 {
    let original = entity.ground().map(|item| item.stack.count).unwrap_or(0);
    if let Some(item) = entity.ground_mut() {
        if stack.is_empty() {
            // 官方先 `setCount(0)`，空了再 `setCount(orgCount)`，物品种类不变。
            // Vanilla `setCount(0)`s, then `setCount(orgCount)` on the same stack. The item stays.
            item.stack.count = original;
            entity.discard();
        } else {
            item.stack = stack;
        }
    }
    original
}

/// 玩家丢出时盖掉构造速度。构造函数的两次 double 已经在 `spawn` 里取过了。
///
/// Overwrites the constructor velocity after a player throw. Those two doubles were already drawn in `spawn`.
pub fn set_throw_velocity(entity: &mut Entity, delta: Vec3) {
    entity.delta = delta;
    if let Some(item) = entity.ground_mut() {
        item.set_pickup_delay(40);
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_core::BlockPos;
    use mc_bamboo_data::items;
    use mc_bamboo_protocol::item::ItemStack;

    use crate::{BlockTicks, LegacyRandom, testing::TestLevel, world::World};

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    /// `Block.popResource`：三次位置 double 无条件取，规则为假时不生成实体。
    #[test]
    fn block_drops_off_still_draws_the_position_offsets() {
        let mut level = TestLevel::new();
        level.rules.block_drops = false;
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.pop_resource(POS, ItemStack::new(items::STONE, 1));
        assert!(world.entity_ids().is_empty());
        assert_eq!(
            world.random().seed(),
            seed_after(|random| {
                for _ in 0..3 {
                    random.next_double();
                }
            })
        );
    }

    /// `BLOCK_DROPS` 为真时生成实体；实体的两次速度 double 用自己的随机源，不动 `level.random`。
    #[test]
    fn block_drops_on_spawns_the_item() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.pop_resource(POS, ItemStack::new(items::STONE, 1));
        assert_eq!(world.entity_ids().len(), 1);
        assert_eq!(
            world.random().seed(),
            seed_after(|random| {
                for _ in 0..3 {
                    random.next_double();
                }
            })
        );
    }

    /// `popResourceFromFace` 的位置与速度随机数按面取，规则为假时只取数不生成实体。
    #[test]
    fn block_drops_off_still_draws_the_face_offsets() {
        use mc_bamboo_core::Direction;

        let mut level = TestLevel::new();
        level.rules.block_drops = false;
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.pop_resource_from_face(POS, Direction::Up, ItemStack::new(items::STONE, 1));
        assert!(world.entity_ids().is_empty());
        // 上面：X/Z 的位置、Y/Z 的速度各两次 `nextDouble`，X 的速度是常量 0。
        assert_eq!(
            world.random().seed(),
            seed_after(|random| {
                for _ in 0..4 {
                    random.next_double();
                }
            })
        );
    }

    #[test]
    fn block_drops_on_spawns_the_face_item() {
        use mc_bamboo_core::Direction;

        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.pop_resource_from_face(POS, Direction::Up, ItemStack::new(items::STONE, 1));
        assert_eq!(world.entity_ids().len(), 1);
        assert_eq!(
            world.random().seed(),
            seed_after(|random| {
                for _ in 0..4 {
                    random.next_double();
                }
            })
        );
    }

    /// 侧面：`step_x != 0` 让 X 的位置和速度都不取随机数，Y/Z 的位置和速度都取。
    #[test]
    fn a_horizontal_face_draws_its_own_offsets() {
        use mc_bamboo_core::Direction;

        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        world.pop_resource_from_face(POS, Direction::East, ItemStack::new(items::STONE, 1));
        assert_eq!(world.entity_ids().len(), 1);
        // Y、Z 的位置两次，Y、Z 的速度两次，合计四次。
        assert_eq!(
            world.random().seed(),
            seed_after(|random| {
                for _ in 0..4 {
                    random.next_double();
                }
            })
        );
    }

    fn seed_after(draws: impl FnOnce(&LegacyRandom)) -> i64 {
        let random = LegacyRandom::new(0);
        draws(&random);
        random.seed()
    }
}
