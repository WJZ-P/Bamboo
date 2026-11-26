//! 不祥物品生成器，官方 `OminousItemSpawner`。
//!
//! 不祥试炼刷怪笼在玩家或已记录生物的上方生成它；到点掉下一件物品，或者朝下射出一枚
//! 投射物。`item` 是同步的 `DATA_ITEM`（ITEM_STACK，index 8），`spawn_item_after_ticks`
//! 进存档。客户端的粒子只在客户端跑，这里不做。
//!
//! The ominous item spawner, vanilla `OminousItemSpawner`. An ominous trial
//! spawner places one above a player or a tracked mob; when its timer expires it
//! drops an item or shoots a projectile downwards. `item` is the synced
//! `DATA_ITEM` slot, and `spawn_item_after_ticks` is saved. Client particles are
//! not part of the server side.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::items;
use mc_bamboo_protocol::item::ItemStack;

use super::{
    Entity, EntityKind,
    ctx::{Command, EntityCtx},
};
use crate::World;

/// `SPAWN_ITEM_DELAY_MIN` / `SPAWN_ITEM_DELAY_MAX`：`create` 里的闭区间。
pub const SPAWN_ITEM_DELAY_MIN: i32 = 60;
pub const SPAWN_ITEM_DELAY_MAX: i32 = 120;
/// `TICKS_BEFORE_ABOUT_TO_SPAWN_SOUND`：到点前 36 刻放预警音。
pub const TICKS_BEFORE_ABOUT_TO_SPAWN_SOUND: i64 = 36;
/// `spawnItem` 里的世界事件号。
pub const SPAWN_ITEM_LEVEL_EVENT: i32 = 3021;
/// 预警音。
const ABOUT_TO_SPAWN_SOUND: &str = "minecraft:block.trial_spawner.about_to_spawn_item";

/// `OminousItemSpawner` 自己的状态。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OminousItemSpawner {
    /// `DATA_ITEM`；掉完东西后清成空堆。
    pub item: ItemStack,
    /// `spawnItemAfterTicks`，进存档。
    pub spawn_item_after_ticks: i64,
}

impl OminousItemSpawner {
    /// `create(level, stack)`：`level.random.nextIntBetweenInclusive(60, 120)`。
    /// 世界随机源由调用方传进来（官方用的是 `level.getRandom()`）。
    pub fn new(stack: ItemStack, level_random: &crate::LegacyRandom) -> Self {
        Self {
            item: stack,
            spawn_item_after_ticks: i64::from(
                level_random.next_int_between_inclusive(SPAWN_ITEM_DELAY_MIN, SPAWN_ITEM_DELAY_MAX),
            ),
        }
    }
}

/// 造一个生成器实体；`spawn_item_after_ticks` 由调用方按世界随机源填好。
pub fn entity(position: Vec3, state: OminousItemSpawner) -> Entity {
    Entity::new(EntityKind::OminousItemSpawner(Box::new(state)), position)
}

/// `getItem().getItem() instanceof ProjectileItem`：不祥试炼刷怪笼的战利品表里
/// 可能出现的、实现了官方 `ProjectileItem` 的物品。其余物品按 `ItemEntity` 掉下。
///
/// Item ids implementing vanilla's `ProjectileItem` among the ominous trial
/// spawner's loot table entries.
pub const fn is_projectile_item(item: i32) -> bool {
    matches!(
        item,
        items::ARROW
            | items::TIPPED_ARROW
            | items::SPECTRAL_ARROW
            | items::FIRE_CHARGE
            | items::WIND_CHARGE
            | items::SPLASH_POTION
            | items::LINGERING_POTION
            | items::ENDER_PEARL
            | items::EGG
            | items::SNOWBALL
            | items::EXPERIENCE_BOTTLE
    )
}

/// `OminousItemSpawner.tickServer`：算阶段只取不随落地变化的事实，落地阶段写世界。
///
/// `tickCount == spawnItemAfterTicks - 36` 放预警音；`tickCount >= spawnItemAfterTicks`
/// 掉物品或投射物、发 3021 世界事件和 ENTITY_PLACE 游戏事件，然后 `kill`（Bamboo 里
/// 用 `discard()`）。
pub fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let EntityKind::OminousItemSpawner(spawner) = &me.kind else {
        return;
    };
    let now = i64::from(me.tick_count);
    let about_to_spawn = now == spawner.spawn_item_after_ticks - TICKS_BEFORE_ABOUT_TO_SPAWN_SOUND;
    let due = now >= spawner.spawn_item_after_ticks;
    if !about_to_spawn && !due {
        return;
    }
    let pos = me.block_position();
    let position = me.position;
    let EntityKind::OminousItemSpawner(spawner) = &mut me.kind else {
        return;
    };
    let stack = if due {
        std::mem::take(&mut spawner.item)
    } else {
        ItemStack::empty()
    };
    let facts = SpawnerTick {
        about_to_spawn,
        due,
        pos,
        position,
        stack,
    };
    // 官方 `spawnItem` 里 `setItem(EMPTY)`；同步的那一半在算阶段就清掉。
    if due {
        me.discard();
    }
    ctx.emit(Command::OminousItemSpawnerTick(facts), me);
}

/// 一次生成器 tick 的落地事实。
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnerTick {
    /// `TRIAL_SPAWNER_ABOUT_TO_SPAWN_ITEM` 的那一刻。
    pub about_to_spawn: bool,
    /// 到点掉东西的那一刻。
    pub due: bool,
    pub pos: BlockPos,
    pub position: Vec3,
    /// 到点那一刻取出来的物品堆；空堆表示什么都不生成。
    pub stack: ItemStack,
}

/// [`Command::OminousItemSpawnerTick`] 的落地：预警音、掉物 / 投射物、3021 和游戏事件。
pub fn apply(_me: &mut Entity, world: &mut World<'_>, facts: SpawnerTick) {
    if facts.about_to_spawn {
        // `playSound(null, blockPosition, ABOUT_TO_SPAWN_ITEM, NEUTRAL)`：默认音量 / 音高 1。
        world.play_sound(
            crate::world::block_center_public(facts.pos),
            mc_bamboo_data::sounds::sound_event(ABOUT_TO_SPAWN_SOUND),
            mc_bamboo_data::sounds::NEUTRAL,
            1.0,
            1.0,
            false,
        );
    }
    if !facts.due || facts.stack.is_empty() {
        return;
    }
    if is_projectile_item(facts.stack.item) {
        spawn_projectile(world, &facts);
    } else {
        // `new ItemEntity(level, getX, getY, getZ, stack)`：不抬高、拾取延迟缺省 0。
        world.spawn_entity(Entity::ground_item(facts.position, facts.stack));
    }
    world.level_event(SPAWN_ITEM_LEVEL_EVENT, facts.pos, 1);
    world.game_event(
        crate::game_event::GameEvent::EntityPlace,
        facts.position,
        crate::game_event::dispatch::EventContext::default(),
    );
}

/// `spawnProjectile`：朝 `DOWN` 射出投射物。`DispenseConfig` 的方向步长是
/// `(0,-1,0)`，`shoot` 的散布和速度取自新投射物自己的随机源。
fn spawn_projectile(world: &mut World<'_>, facts: &SpawnerTick) {
    let stack = &facts.stack;
    // `DispenseConfig.DEFAULT` 是 (power 1.1, uncertainty 6.0)；子类只改这几项。
    let (power, uncertainty, override_event) = match stack.item {
        items::WIND_CHARGE => (1.0_f32, 6.666_666_5_f32, Some(1051)),
        items::FIRE_CHARGE => (1.0, 6.666_666_5, Some(1018)),
        items::SPLASH_POTION | items::LINGERING_POTION | items::EXPERIENCE_BOTTLE => {
            (1.375, 3.0, None)
        }
        _ => (1.1, 6.0, None),
    };
    if let Some(event) = override_event {
        world.level_event(event, facts.pos, 0);
    }
    let direction = Vec3::new(0.0, -1.0, 0.0);
    match stack.item {
        // 箭、火焰弹、风弹这些不是 `ThrowableItemProjectile` 的种类：直接按
        // `Projectile.shoot` 构造并射出。
        items::ARROW | items::TIPPED_ARROW | items::SPECTRAL_ARROW => {
            let shot = super::arrow::ArrowShot {
                position: facts.position,
                direction,
                power,
                uncertainty,
                source_movement: None,
                owner: None,
                owner_uuid: None,
                ammo: stack.clone(),
                weapon: None,
                critical: false,
                pickup: super::arrow::Pickup::Allowed,
            };
            world.spawn_arrow_shot(shot);
        }
        items::SPLASH_POTION | items::LINGERING_POTION => {
            let shot = super::splash_potion::SplashShot {
                position: facts.position,
                direction,
                power,
                uncertainty,
                source_movement: None,
                owner: None,
                owner_uuid: None,
                item: stack.clone(),
            };
            world.spawn_splash_shot(&shot);
        }
        items::WIND_CHARGE => {
            let shot = super::wind_charge::WindChargeShot {
                position: facts.position,
                direction,
                power,
                uncertainty,
                variant: super::wind_charge::WindChargeVariant::Player,
                owner: None,
                owner_uuid: None,
            };
            world.spawn_wind_charge_shot(&shot);
        }
        items::EGG | items::SNOWBALL | items::ENDER_PEARL => {
            let projectile = match stack.item {
                items::SNOWBALL => super::thrown_egg::ThrowableKind::Snowball,
                items::ENDER_PEARL => super::thrown_egg::ThrowableKind::EnderPearl,
                _ => super::thrown_egg::ThrowableKind::Egg,
            };
            let shot = super::thrown_egg::EggShot {
                position: facts.position,
                direction,
                power,
                uncertainty,
                projectile,
                source_movement: None,
                owner: None,
                owner_uuid: None,
                item: stack.clone(),
            };
            world.spawn_egg_shot(&shot);
        }
        // 火焰弹（`SmallFireball`）和附魔之瓶（`ThrownExperienceBottle`）尚未建模，
        // 官方在这里分别射出这两枚投射物，Bamboo 按普通掉落物处理（README 有记录）。
        _ => {
            world.spawn_entity(Entity::ground_item(facts.position, stack.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BlockTicks, Effect,
        entity::{EntityKind, ctx::PreciseCtx},
        testing::TestLevel,
    };
    use mc_bamboo_data::items;

    fn spawner(delay: i64, stack: ItemStack) -> Entity {
        let mut entity = entity(
            Vec3::new(0.5, 70.0, 0.5),
            OminousItemSpawner {
                item: stack,
                spawn_item_after_ticks: delay,
            },
        );
        entity.id = 9;
        entity
    }

    /// 逐刻驱动一个生成器，返回每一刻的（预警音、世界事件、掉出的物品堆）。
    fn run(delay: i64, stack: ItemStack, ticks: i32) -> Vec<(bool, Vec<i32>, Vec<ItemStack>)> {
        let mut out = Vec::new();
        for _ in 0..ticks {
            let mut level = TestLevel::new();
            let mut scheduler = BlockTicks::new();
            let mut world = World::new(&mut level, &mut scheduler);
            let mut me = spawner(delay, stack.clone());
            me.tick_count = out.len() as i32;
            let mut ctx = PreciseCtx::new(&mut world, me.id);
            super::tick(&mut me, &mut ctx);
            let world = ctx.into_world();
            let effects = world.drain_effects();
            let mut sounds = false;
            let mut events = Vec::new();
            for effect in effects {
                match effect {
                    Effect::Sound { sound, .. } => {
                        sounds |= sound
                            == i32::from(mc_bamboo_data::sounds::sound_event(ABOUT_TO_SPAWN_SOUND));
                    }
                    Effect::LevelEvent { event, .. } => events.push(event),
                    _ => {}
                }
            }
            let mut items = Vec::new();
            for spawned in world.live_entities() {
                if let crate::entity::EntityKind::Item(item) = &spawned.kind {
                    items.push(item.stack.clone());
                }
            }
            out.push((sounds, events, items));
        }
        out
    }

    /// 60 刻后掉一件普通物品：第 24 刻预警音，第 60 刻 3021 事件和物品，然后消失。
    #[test]
    fn drops_an_item_at_the_scheduled_tick() {
        let stack = ItemStack::new(items::STONE, 3);
        let frames = run(60, stack.clone(), 61);
        assert!(frames[24].0, "the warning sounds 36 ticks before");
        assert_eq!(frames[59], (false, vec![], vec![]));
        assert_eq!(
            frames[60],
            (false, vec![SPAWN_ITEM_LEVEL_EVENT], vec![stack.clone()])
        );
        assert_eq!(frames[60].1.len(), 1);
        let mut unused = spawner(60, stack);
        // 到点之后 `DATA_ITEM` 清空、实体标记成要移除。
        let mut level = TestLevel::new();
        let mut scheduler = BlockTicks::new();
        let mut world = World::new(&mut level, &mut scheduler);
        for _ in 0..60 {
            unused.common_tick();
            let mut ctx = PreciseCtx::new(&mut world, unused.id);
            super::tick(&mut unused, &mut ctx);
        }
        assert!(unused.removed);
        let EntityKind::OminousItemSpawner(state) = &unused.kind else {
            panic!("still a spawner");
        };
        assert!(state.item.is_empty());
    }

    /// 空堆到点不生成东西、不发 3021，官方 `spawnItem` 开头就返回。
    #[test]
    fn an_empty_stack_drops_nothing() {
        let frames = run(60, ItemStack::empty(), 61);
        assert_eq!(frames[60], (false, vec![], vec![]));
    }
}
