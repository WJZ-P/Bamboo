//! 末影之眼实体，官方 `net.minecraft.world.entity.projectile.EyeOfEnder`。
//!
//! 由 `EnderEyeItem.use` 在玩家腰部高度（`player.getY(0.5)`）生成，朝最近要塞方向飞
//! 80 刻；第 81 刻（`++life > 80`）播放 `entity.ender_eye.death`，按构造时掷出的
//! `surviveAfterDeath`（`random.nextInt(5) > 0`）掉成 `ItemEntity` 或触发 2003 世界
//! 事件碎裂。`DATA_ITEM_STACK`（index 8）同步物品堆，存档写 `"Item"` 键。
//!
//! Eye of ender entity, vanilla `EyeOfEnder`. Spawned by `EnderEyeItem.use` at
//! `player.getY(0.5)`, flying toward the nearest stronghold for 80 ticks; on tick
//! 81 (`++life > 80`) it plays `entity.ender_eye.death`, discards itself, and
//! either spawns an `ItemEntity` (`random.nextInt(5) > 0`) or emits level event
//! 2003. Syncs `DATA_ITEM_STACK` at index 8 and saves the `"Item"` tag.

use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{items, sounds};
use mc_bamboo_protocol::item::ItemStack;

use super::{
    Entity, EntityKind, EntityUid, JavaRandom,
    ctx::{Command, EntityCtx},
};
use crate::{World, container::copy_with_count};

/// `EyeOfEnder.TOO_FAR_DISTANCE`：超过 12 格水平距离时只飞向 12 格外、高 8 格的点。
///
/// `EyeOfEnder.TOO_FAR_DISTANCE`: targets farther than 12 blocks horizontally are
/// clamped to 12 blocks away and 8 blocks higher.
pub const TOO_FAR_DISTANCE: f64 = 12.0;
/// `EyeOfEnder.TOO_FAR_SIGNAL_HEIGHT`。
pub const TOO_FAR_SIGNAL_HEIGHT: f64 = 8.0;
/// 寿命阈值：`++life > 80`（第 81 刻死亡）。
///
/// Lifetime threshold: `++life > 80` (dies on tick 81).
pub const MAX_LIFE: i32 = 80;
/// 碎裂世界事件号。
///
/// Level event emitted when the eye shatters.
pub const SHATTER_LEVEL_EVENT: i32 = 2003;
/// 死亡音效。
const DEATH_SOUND: &str = "minecraft:entity.ender_eye.death";

/// `EyeOfEnder` 自己的状态。
///
/// State owned by an `EyeOfEnder`.
#[derive(Debug, Clone, PartialEq)]
pub struct EyeOfEnder {
    /// `Entity.<init>` 从实体随机源抽出的 UUID（2 次 `nextLong`）。
    pub uuid: [u8; 16],
    /// `DATA_ITEM_STACK`（index 8），默认 `minecraft:ender_eye` × 1。
    pub item: ItemStack,
    /// `EyeOfEnder.target`；由 `signalTo` 设好，存档不保存。
    pub target: Option<Vec3>,
    /// `EyeOfEnder.life`；每刻加一，超过 80 时死亡。
    pub life: i32,
    /// `EyeOfEnder.surviveAfterDeath`；`signalTo` 时掷 `random.nextInt(5) > 0`。
    pub survive_after_death: bool,
}

/// 公开视图：同步和存档需要的 UUID 与物品堆。
///
/// Public view: UUID and item stack used by tracking and archiving.
#[derive(Debug, Clone, PartialEq)]
pub struct EyeOfEnderPublic {
    pub uuid: [u8; 16],
    pub item: ItemStack,
}

impl Default for EyeOfEnder {
    fn default() -> Self {
        Self::new(default_item())
    }
}

impl EyeOfEnder {
    /// 新建一枚末影之眼；空堆会照 `setItem` 回退成默认 `minecraft:ender_eye`。
    ///
    /// Create a new eye of ender; an empty stack falls back to `minecraft:ender_eye`
    /// as in `EyeOfEnder.setItem`.
    pub fn new(stack: ItemStack) -> Self {
        Self {
            uuid: [0; 16],
            item: normalize_item(stack),
            target: None,
            life: 0,
            survive_after_death: false,
        }
    }

    /// `EyeOfEnder.setItem`：空则用默认 `ENDER_EYE`，否则 `copyWithCount(1)`。
    pub fn set_item(&mut self, stack: ItemStack) {
        self.item = normalize_item(stack);
    }

    /// `EyeOfEnder.signalTo(Vec3)`：定目标点、清零 `life`，并掷 `random.nextInt(5) > 0`。
    ///
    /// `EyeOfEnder.signalTo(Vec3)`: set the flight target, reset `life`, and roll
    /// `random.nextInt(5) > 0`.
    pub fn signal_to(&mut self, position: Vec3, target: Vec3, random: &mut JavaRandom) {
        let delta = target.subtract(position);
        let dist = horizontal_distance(delta);
        self.target = Some(if dist > TOO_FAR_DISTANCE {
            position.add(Vec3::new(
                delta.x / dist * TOO_FAR_DISTANCE,
                TOO_FAR_SIGNAL_HEIGHT,
                delta.z / dist * TOO_FAR_DISTANCE,
            ))
        } else {
            target
        });
        self.life = 0;
        self.survive_after_death = random.next_int_bounded(5) > 0;
    }

    /// 供 `EntityPublic` 使用的公开视图。
    pub fn public(&self) -> EyeOfEnderPublic {
        EyeOfEnderPublic {
            uuid: self.uuid,
            item: self.item.clone(),
        }
    }
}

/// `EyeOfEnder.getDefaultItem()`。
pub fn default_item() -> ItemStack {
    ItemStack::new(items::ENDER_EYE, 1)
}

fn normalize_item(stack: ItemStack) -> ItemStack {
    if stack.is_empty() {
        default_item()
    } else {
        copy_with_count(&stack, 1)
    }
}

fn horizontal_distance(v: Vec3) -> f64 {
    (v.x * v.x + v.z * v.z).sqrt()
}

/// 造一个末影之眼实体；尚未调用 `signalTo`。
pub fn entity(position: Vec3, stack: ItemStack) -> Entity {
    Entity::new(
        EntityKind::EyeOfEnder(Box::new(EyeOfEnder::new(stack))),
        position,
    )
}

impl World<'_> {
    /// 构造一枚末影之眼、设物品、`signalTo(target)`，再放进世界。
    ///
    /// 随机数顺序照官方：`prepare_entity` 先给实体换种并按 `Entity.<init>` 抽 UUID，
    /// 再用同一个实体随机源跑 `signalTo` 的 `nextInt(5)`，最后入表。
    ///
    /// Construct an `EyeOfEnder`, set its item, call `signalTo(target)`, and add
    /// it to the world. Random draws match vanilla: `prepare_entity` seeds the
    /// entity and draws its UUID, then `signalTo` draws `nextInt(5)`.
    pub fn spawn_eye_of_ender(
        &mut self,
        position: Vec3,
        item: ItemStack,
        target: Vec3,
    ) -> EntityUid {
        let mut me = self.prepare_entity(entity(position, item));
        let EntityKind::EyeOfEnder(eye) = &mut me.kind else {
            unreachable!("eye of ender entity has matching kind");
        };
        eye.signal_to(position, target, &mut me.random);
        self.add_prepared_entity(me)
    }
}

/// `EyeOfEnder.updateDeltaMovement(Vec3 delta, Vec3 newPos, Vec3 target)`。
///
/// 浮点运算顺序逐指令对照 26.3 字节码。
///
/// Floating-point operation order matches the 26.3 bytecode instruction for instruction.
pub fn update_delta_movement(delta: Vec3, new_pos: Vec3, target: Vec3) -> Vec3 {
    let to_target = Vec3::new(target.x - new_pos.x, 0.0, target.z - new_pos.z);
    let horiz_dist = (to_target.x * to_target.x + to_target.z * to_target.z).sqrt();
    let delta_horiz = horizontal_distance(delta);
    // `Mth.lerp(0.0025, delta.horizontalDistance(), horizDist)` = start + 0.0025 * (end - start).
    let mut speed = delta_horiz + 0.0025 * (horiz_dist - delta_horiz);
    let mut dy = delta.y;
    if horiz_dist < 1.0 {
        speed *= 0.8;
        dy *= 0.8;
    }
    let y_dir = if new_pos.y - delta.y < target.y {
        1.0
    } else {
        -1.0
    };
    let scale = speed / horiz_dist;
    Vec3::new(
        to_target.x * scale,
        dy + (y_dir - dy) * 0.015,
        to_target.z * scale,
    )
}

/// 末影之眼寿命到点时的落地事实。
///
/// World effects emitted when an eye of ender expires.
#[derive(Debug, Clone, PartialEq)]
pub struct EyeOfEnderFinish {
    pub position: Vec3,
    pub block_pos: BlockPos,
    pub item: ItemStack,
    pub survive_after_death: bool,
}

/// `EyeOfEnder.tick`，在 `commonTick` 之后运行。
///
/// `EyeOfEnder.tick`, running after `commonTick`.
pub fn tick(me: &mut Entity, ctx: &mut dyn EntityCtx) {
    let new_pos = me.position.add(me.delta);
    let target = match &me.kind {
        EntityKind::EyeOfEnder(eye) => eye.target,
        _ => return,
    };
    if let Some(target) = target {
        me.delta = update_delta_movement(me.delta, new_pos, target);
    }
    me.position = new_pos;
    let block_pos = me.block_position();
    let finish = {
        let EntityKind::EyeOfEnder(eye) = &mut me.kind else {
            return;
        };
        eye.life += 1;
        if eye.life > MAX_LIFE {
            Some(EyeOfEnderFinish {
                position: me.position,
                block_pos,
                item: eye.item.clone(),
                survive_after_death: eye.survive_after_death,
            })
        } else {
            None
        }
    };
    if let Some(facts) = finish {
        me.discard();
        ctx.emit(Command::EyeOfEnderFinish(facts), me);
    }
}

/// [`Command::EyeOfEnderFinish`] 落地：播放死亡音效，按 `survive_after_death` 掉物品或发 2003 事件。
///
/// Apply [`Command::EyeOfEnderFinish`]: play death sound, then spawn an `ItemEntity`
/// or emit level event 2003 depending on `survive_after_death`.
pub fn apply_finish(_me: &mut Entity, world: &mut World<'_>, facts: EyeOfEnderFinish) {
    world.play_sound(
        facts.position,
        sounds::sound_event(DEATH_SOUND),
        sounds::NEUTRAL,
        1.0,
        1.0,
        false,
    );
    if facts.survive_after_death {
        world.spawn_entity(Entity::ground_item(facts.position, facts.item));
    } else {
        world.level_event(SHATTER_LEVEL_EVENT, facts.block_pos, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, Effect, testing::TestLevel};

    #[test]
    fn signal_to_clamps_far_targets_and_keeps_near_targets() {
        let start = Vec3::new(0.0, 65.0, 0.0);
        let mut eye = EyeOfEnder::default();
        let mut rng = JavaRandom::new(42);
        eye.signal_to(start, Vec3::new(30.0, 20.0, 40.0), &mut rng);
        let target = eye.target.expect("target set");
        assert!((target.x - 7.2).abs() < 1e-12);
        assert!((target.y - 73.0).abs() < 1e-12);
        assert!((target.z - 9.6).abs() < 1e-12);
        assert_eq!(eye.life, 0);

        // 水平距离恰好 <= 12 时直接飞向目标点本身。
        let near = Vec3::new(3.0, 30.0, 4.0);
        eye.signal_to(start, near, &mut rng);
        assert_eq!(eye.target, Some(near));
    }

    #[test]
    fn expires_on_tick_81_and_either_drops_or_shatters() {
        // 先找两个不同结果（存活 / 碎裂）的种子并逐刻跑 81 刻。
        for want_survive in [true, false] {
            let mut level = TestLevel::new();
            let mut ticks = BlockTicks::new();
            let uid = {
                let mut world = World::new(&mut level, &mut ticks);
                let mut me = world.prepare_entity(entity(
                    Vec3::new(0.5, 65.0, 0.5),
                    ItemStack::new(items::ENDER_EYE, 5),
                ));
                let EntityKind::EyeOfEnder(eye) = &mut me.kind else {
                    unreachable!();
                };
                eye.signal_to(
                    Vec3::new(0.5, 65.0, 0.5),
                    Vec3::new(100.0, 0.0, 0.0),
                    &mut me.random,
                );
                eye.survive_after_death = want_survive;
                assert_eq!(eye.item.count, 1, "setItem copies with count 1");
                world.add_prepared_entity(me)
            };

            let mut finish_effects = Vec::new();
            for step in 1..=81 {
                let mut world = World::new(&mut level, &mut ticks);
                world.tick_entities();
                let effects = world.drain_effects();
                if step < 81 {
                    assert!(
                        level.entities().get(uid).is_some(),
                        "eye must still be alive at tick {step}"
                    );
                    assert!(effects.is_empty(), "no finish effects before tick 81");
                } else {
                    assert!(
                        level.entities().get(uid).is_none(),
                        "eye must be discarded on tick 81"
                    );
                    finish_effects = effects;
                }
            }

            let death_sound = i32::from(sounds::sound_event(DEATH_SOUND));
            assert!(
                finish_effects
                    .iter()
                    .any(|e| matches!(e, Effect::Sound { sound, .. } if *sound == death_sound)),
                "death sound must play on tick 81"
            );
            let items: Vec<_> = level
                .entities()
                .iter_public()
                .filter(|e| matches!(e.kind, crate::entity::KindPublic::Item { .. }))
                .collect();
            let shatters = finish_effects
                .iter()
                .filter(|e| matches!(e, Effect::LevelEvent { event: 2003, .. }))
                .count();
            if want_survive {
                assert_eq!(items.len(), 1);
                assert_eq!(shatters, 0);
            } else {
                assert_eq!(items.len(), 0);
                assert_eq!(shatters, 1);
            }
        }
    }
}
