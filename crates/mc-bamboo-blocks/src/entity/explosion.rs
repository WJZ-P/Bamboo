//! 爆炸：官方 `ServerExplosion`。TNT 只走和方块有关的那部分，生物爆炸（苦力怕）还伤生物和玩家。
//!
//! Explosions (`ServerExplosion`). TNT runs the block half only; mob explosions (creepers)
//! also hurt mobs and players.
//!
//! TNT 的爆炸，官方 `ServerExplosion` 里和方块有关的那部分。
//!
//! 只走 TNT 这一条路：不伤生物、不点火。被炸掉的方块按战利品表掉物品，不给经验。
//! 破坏哪些方块、按什么顺序破坏都要和官方一样，因为顺序决定世界随机源怎么用，
//! 进而决定连锁点燃的 TNT 拿到什么引信。
//!
//! The block half of `ServerExplosion`, TNT only. Mobs are not hurt and no fire is
//! started. Broken blocks drop through their loot tables, without experience.
//! Which blocks break, and in which order, matches vanilla because that order decides
//! how `level.random` is consumed and therefore which fuse a chained TNT gets.

use std::collections::HashSet;

use mc_bamboo_core::{BlockPos, Vec3, mth};
use mc_bamboo_data::BlockAncestor;
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    entity::{
        EntityKind, EntityPublic, KindPublic,
        ctx::PlayerView,
        mob::{DamageKind, DamageSource},
    },
    shape::{Aabb, CollisionContext},
    state::{BlockState, FluidState},
    world::World,
};

/// `ServerExplosion.calculateExplodedPositions` 的网格边长，只有最外面一层格子发出射线。
const RAY_GRID: i32 = 16;
/// 射线每走一格消耗的爆炸强度，官方写的是 `0.22500001F`。
const POWER_PER_STEP: f32 = 0.225_000_01;
/// 射线每一步走多远，官方的 `0.3F`。
const STEP: f32 = 0.3;
/// 方块抗性之外再加的抗性，官方的 `0.3F`。
const RESISTANCE_BONUS: f32 = 0.3;
/// 每点抗性消耗多少爆炸强度，官方的 `0.3F`。
const RESISTANCE_COST: f32 = 0.3;
impl World<'_> {
    /// `PrimedTnt.explode` 走到 `ServerLevel.explode` 和 `ServerExplosion.explode`，
    /// 这里只保留破坏方块和连锁点燃 TNT。
    ///
    /// `center` 是爆炸中心，`radius` 是爆炸强度。不伤生物，也不给经验。
    ///
    /// 官方把 `entity.generic.explode` 和爆炸粒子放进 `ClientboundExplodePacket`，不是 `playSound`。
    /// 协议里还没有这个包，所以这里不另发一条声音，避免和 `playSeededSound` 对不上。
    ///
    /// `center` is the explosion centre and `radius` is its power. Mobs are not hurt and no
    /// experience is dropped.
    ///
    /// Vanilla puts `entity.generic.explode` and the explosion particles on
    /// `ClientboundExplodePacket`, not `playSound`. That packet is not in the protocol yet,
    /// so this does not emit a sound that would disagree with `playSeededSound`.
    pub fn explode_tnt(&mut self, center: Vec3, radius: f32) {
        let broken = self.calculate_exploded_positions(center, radius);
        // `hurtEntities` 在打乱方块之前，而且不取 `level.random`。
        // `hurtEntities` runs before the block list is shuffled and does not draw `level.random`.
        crate::entity::item::hurt_items_by_explosion(self, center, radius);
        self.interact_with_blocks(broken, None);
    }

    /// `ServerLevel.explode(source, ..., ExplosionInteraction.MOB)`：来源是一个生物。
    /// 射线之后先伤实体；`mobGriefing` 关着时方块不动（`BlockInteraction.KEEP`），
    /// 开着时按 `mobExplosionDropDecay` 决定掉落带不带衰减。
    /// `ClientboundExplodePacket`（声音、粒子、玩家击退）协议里还没有，不发。
    ///
    /// `ServerLevel.explode(source, ..., ExplosionInteraction.MOB)` for a mob source.
    /// Entities are hurt after the rays; blocks stay under disabled `mobGriefing`
    /// (`BlockInteraction.KEEP`), otherwise `mobExplosionDropDecay` picks decayed drops.
    /// `ClientboundExplodePacket` (sound, particles, player knockback) is not in the
    /// protocol yet and is not sent.
    pub(crate) fn explode_mob(&mut self, source: &EntityPublic, center: Vec3, radius: f32) {
        let rules = self.block_game_rules();
        let broken = self.calculate_exploded_positions(center, radius);
        crate::entity::item::hurt_items_by_explosion(self, center, radius);
        hurt_living_by_explosion(self, source, center, radius);
        if rules.mob_griefing {
            let decay = rules.mob_explosion_drop_decay.then_some(radius);
            self.interact_with_blocks(broken, decay);
        }
    }

    /// `ServerExplosion.interactWithBlocks`：打乱、逐格破坏、最后一起弹出掉落。
    /// `decay` 是 `DESTROY_WITH_DECAY` 下交给战利品表的爆炸半径。
    ///
    /// `ServerExplosion.interactWithBlocks`: shuffle, break each block, then pop the drops.
    /// `decay` is the radius handed to loot tables under `DESTROY_WITH_DECAY`.
    fn interact_with_blocks(&mut self, mut broken: Vec<BlockPos>, decay: Option<f32>) {
        // `Util.shuffle(toBlow, level.random)`。
        self.random().shuffle(&mut broken);
        // `interactWithBlocks`：先收齐掉落，整圈方块处理完才 `popResource`。
        // `interactWithBlocks` collects drops, then `popResource`s them after every block.
        let mut drops = Vec::new();
        for pos in broken {
            let state = self.block_state(pos);
            if state.is_air() {
                continue;
            }
            // `TntBlock.dropFromExplosion` 返回假，别的方块默认掉落。
            // `TntBlock.dropFromExplosion` is false. Every other block drops by default.
            let was_tnt = state.is_a(BlockAncestor::TntBlock);
            if !was_tnt {
                let stacks = match decay {
                    Some(radius) => self.explosion_block_drops_decayed(state, pos, radius),
                    None => self.explosion_block_drops(state, pos),
                };
                for stack in stacks {
                    collect_drop(&mut drops, pos, stack);
                }
            }
            // 26.3 `BlockBehaviour.onExplosionHit` 先调 `spawnAfterBreak`：工具是空手，
            // 虫蚀方块会刷出蠹虫；之后才是 `getDrops` 和 `setBlockAndUpdate`。
            //
            // 26.3 `BlockBehaviour.onExplosionHit` calls `spawnAfterBreak` first with
            // an empty tool: infested blocks spawn a silverfish here.
            if !was_tnt {
                self.spawn_after_break(state, pos, &ItemStack::empty());
            }
            // `setBlockAndUpdate` 在 `wasExploded` 之前。容器的 `dropContents` 就在这一步取数。
            // `setBlockAndUpdate` runs before `wasExploded`. Container `dropContents` draws here.
            self.set_block_and_update(pos, BlockState::AIR);
            if was_tnt {
                self.spawn_short_fuse_tnt(pos);
            }
        }
        for (pos, stack) in drops {
            self.pop_resource(pos, stack);
        }
    }

    /// `ServerExplosion.calculateExplodedPositions`：从 16 立方网格最外层每一格朝外射一条线，
    /// 线上剩多少爆炸强度就炸掉沿途的方块。
    ///
    /// 每条线开头从世界随机源取一个数，所以这个函数的调用次数和顺序都影响后续的随机数。
    fn calculate_exploded_positions(&mut self, center: Vec3, radius: f32) -> Vec<BlockPos> {
        let mut blown = PositionSet::default();
        for xx in 0..RAY_GRID {
            for yy in 0..RAY_GRID {
                for zz in 0..RAY_GRID {
                    if xx != 0
                        && xx != RAY_GRID - 1
                        && yy != 0
                        && yy != RAY_GRID - 1
                        && zz != 0
                        && zz != RAY_GRID - 1
                    {
                        continue;
                    }
                    // 官方的坐标和长度都是 `float` 算完才变成 `double`。
                    let xd = f64::from(xx as f32 / 15.0 * 2.0 - 1.0);
                    let yd = f64::from(yy as f32 / 15.0 * 2.0 - 1.0);
                    let zd = f64::from(zz as f32 / 15.0 * 2.0 - 1.0);
                    let length = (xd * xd + yd * yd + zd * zd).sqrt();
                    let (xd, yd, zd) = (xd / length, yd / length, zd / length);
                    let mut remaining = radius * (0.7 + self.random().next_float() * 0.6);
                    let (mut xp, mut yp, mut zp) = (center.x, center.y, center.z);
                    while remaining > 0.0 {
                        let pos = BlockPos::new(mth::floor(xp), mth::floor(yp), mth::floor(zp));
                        let state = self.block_state(pos);
                        let fluid = state.fluid();
                        if !self.is_in_world_bounds(pos) {
                            break;
                        }
                        if !(state.is_air() && fluid.is_empty()) {
                            let resistance = state.block().explosion_resistance;
                            let resistance = resistance.max(fluid_resistance(fluid));
                            remaining -= (resistance + RESISTANCE_BONUS) * RESISTANCE_COST;
                        }
                        // `ExplosionDamageCalculator.shouldBlockExplode` 恒为真。
                        if remaining > 0.0 {
                            blown.insert(pos);
                        }
                        let step = f64::from(STEP);
                        xp += xd * step;
                        yp += yd * step;
                        zp += zd * step;
                        remaining -= POWER_PER_STEP;
                    }
                }
            }
        }
        blown.into_java_order()
    }
}

/// `ExplosionDamageCalculator.getEntityDamageAmount`。
fn blast_damage(dist: f64, exposure: f32, double_radius: f64) -> f32 {
    let power = (1.0 - dist) * f64::from(exposure);
    ((power * power + power) / 2.0 * 7.0 * double_radius + 1.0) as f32
}

/// `DamageSources.explosion(direct, causing)`：两个来源实体都有时是 `player_explosion`。
///
/// `DamageSources.explosion(direct, causing)`: `player_explosion` when both exist.
fn blast_source(source: &EntityPublic) -> DamageSource {
    let mut damage = DamageSource::environment(DamageKind::PlayerExplosion);
    damage.attacker = Some(source.uid);
    damage.direct = Some(source.uid);
    damage.position = Some(source.position);
    if let KindPublic::Mob(mob) = &source.kind {
        damage.attacker_uuid = Some(mob.uuid);
    }
    damage
}

/// `ServerExplosion.hurtEntities` 里生物和玩家的那部分：按距离和曝光度算伤害，
/// 生物再按爆炸击退抗性被推开。掉落物由 [`crate::entity::item::hurt_items_by_explosion`] 处理。
/// 官方按实体分区的遍历顺序走一遍；这里先生物（uid 升序）后玩家。
///
/// The mob and player part of `ServerExplosion.hurtEntities`: damage from distance and
/// exposure, plus knockback for mobs scaled by explosion knockback resistance. Vanilla
/// walks entity sections once; this goes mobs (ascending uid) then players.
fn hurt_living_by_explosion(world: &mut World, source: &EntityPublic, center: Vec3, radius: f32) {
    if radius < 1.0e-5 {
        return;
    }
    let double_radius = f64::from(radius * 2.0);
    let floor = |value: f64| f64::from(mth::floor(value));
    let area = Aabb::new(
        floor(center.x - double_radius - 1.0),
        floor(center.y - double_radius - 1.0),
        floor(center.z - double_radius - 1.0),
        floor(center.x + double_radius + 1.0),
        floor(center.y + double_radius + 1.0),
        floor(center.z + double_radius + 1.0),
    );
    let damage = blast_source(source);
    for id in world.entity_ids() {
        if let Some((amount, knock)) = mob_blast(world, id, center, double_radius, area) {
            world.hurt_mob(id, damage, amount);
            world.with_entity_mut(id, &mut |entity| {
                entity.delta = entity.delta.add(knock);
                // `Entity.push(Vec3)` 置 `needsSync`：速度要同步出去。
                // `Entity.push(Vec3)` sets `needsSync` so the velocity is sent.
                if let Some(mob) = entity.kind.mob_mut() {
                    mob.hurt_marked = true;
                }
            });
        }
    }
    let mut players = Vec::new();
    world
        .level()
        .for_each_player(&mut |player| players.push(*player));
    for player in players {
        if let Some(amount) = player_blast(world, &player, center, double_radius, area) {
            world.hurt_player_from_mob(player.uid, damage, amount, Some(source));
        }
    }
}

/// 一头生物在爆炸里受的伤害和被推开的速度；不在范围里返回 `None`。
///
/// The damage and push one mob takes from a blast, or `None` when out of range.
fn mob_blast(
    world: &World,
    id: i32,
    center: Vec3,
    double_radius: f64,
    area: Aabb,
) -> Option<(f32, Vec3)> {
    let entity = world.entity(id)?;
    let EntityKind::Mob(mob) = &entity.kind else {
        return None;
    };
    let bounds = entity.bounding_box();
    if entity.removed || !bounds.intersects(area) {
        return None;
    }
    let dist = entity.position.distance_to_sqr(center).sqrt() / double_radius;
    if dist > 1.0 {
        return None;
    }
    let eye = Vec3::new(
        entity.position.x,
        entity.position.y + f64::from(mob.eye_height),
        entity.position.z,
    );
    let exposure =
        crate::entity::item::seen_percent_of(world, center, bounds, entity.collision_context());
    let resistance = mob
        .attributes
        .value(mc_bamboo_data::registries::attribute::EXPLOSION_KNOCKBACK_RESISTANCE);
    let push = (1.0 - dist) * f64::from(exposure) * (1.0 - resistance);
    let knock = eye.subtract(center).normalize().scale(push);
    Some((blast_damage(dist, exposure, double_radius), knock))
}

/// 一名玩家在爆炸里受的伤害。旁观者和死人不算（`EntitySelector.NO_SPECTATORS`）。
/// 玩家的击退走 `ClientboundExplodePacket`，这里没有。
///
/// The damage one player takes. Spectators and the dead are skipped; player knockback
/// travels in `ClientboundExplodePacket`, which is not sent.
fn player_blast(
    world: &World,
    player: &PlayerView,
    center: Vec3,
    double_radius: f64,
    area: Aabb,
) -> Option<f32> {
    if player.spectator || !player.alive {
        return None;
    }
    let half = f64::from(0.6_f32) / 2.0;
    let height = f64::from(if player.sneaking { 1.5_f32 } else { 1.8_f32 });
    let feet = player.position;
    let bounds = Aabb::new(
        feet.x - half,
        feet.y,
        feet.z - half,
        feet.x + half,
        feet.y + height,
        feet.z + half,
    );
    if !bounds.intersects(area) {
        return None;
    }
    let dist = feet.distance_to_sqr(center).sqrt() / double_radius;
    if dist > 1.0 {
        return None;
    }
    let exposure =
        crate::entity::item::seen_percent_of(world, center, bounds, CollisionContext::Empty);
    Some(blast_damage(dist, exposure, double_radius))
}

/// `ServerExplosion.addOrAppendStack`：先按 16 的上限并进已有的堆，剩下的另起一堆。
///
/// `ServerExplosion.addOrAppendStack`: merge into an existing stack with a cap of 16, then keep
/// whatever is left as a new stack.
fn collect_drop(stacks: &mut Vec<(BlockPos, ItemStack)>, pos: BlockPos, mut stack: ItemStack) {
    for (_, existing) in stacks.iter_mut() {
        crate::entity::merge_explosion(existing, &mut stack);
        if stack.is_empty() {
            return;
        }
    }
    stacks.push((pos, stack));
}

/// `FluidState.getExplosionResistance()`。数值来自生成的流体表。
///
/// `FluidState.getExplosionResistance()`, from the generated fluid table.
fn fluid_resistance(fluid: FluidState) -> f32 {
    fluid.explosion_resistance()
}

/// 爆炸破坏的方块表，官方的 `HashSet<BlockPos>`。
///
/// 遍历顺序是官方 `HashMap` 的桶顺序：桶数从 16 开始翻倍到装得下为止，按桶下标升序，
/// 同一个桶里按加入顺序。桶里的元素多到 8 个又会树化，那时顺序就不一样了；TNT 的半径
/// 只有 4，一组爆炸远到不了那一步。
#[derive(Debug, Default)]
struct PositionSet {
    positions: Vec<BlockPos>,
    seen: HashSet<BlockPos>,
}

impl PositionSet {
    /// `HashSet.add`。
    fn insert(&mut self, pos: BlockPos) {
        if self.seen.insert(pos) {
            self.positions.push(pos);
        }
    }

    /// `new ObjectArrayList(set)`：按哈希表的遍历顺序排好。
    fn into_java_order(self) -> Vec<BlockPos> {
        let capacity = table_capacity(self.positions.len());
        let mut buckets = vec![Vec::new(); capacity];
        for pos in self.positions {
            buckets[block_pos_bucket(pos, capacity)].push(pos);
        }
        buckets.into_iter().flatten().collect()
    }
}

/// `HashMap` 装下这么多个元素后的桶数。
fn table_capacity(len: usize) -> usize {
    let mut capacity = 16;
    while len > capacity / 4 * 3 {
        capacity *= 2;
    }
    capacity
}

/// `BlockPos` 落在哪个桶里：`Vec3i.hashCode()`，再按 `HashMap.hash` 和桶数取下标。
fn block_pos_bucket(pos: BlockPos, capacity: usize) -> usize {
    let hash = pos
        .y
        .wrapping_add(pos.z.wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(pos.x);
    let spread = hash ^ ((hash as u32) >> 16) as i32;
    (spread as u32 & (capacity as u32 - 1)) as usize
}
