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
use mc_bamboo_data::{BlockAncestor, blocks};
use mc_bamboo_protocol::item::ItemStack;

use crate::{
    Effect,
    entity::{
        EntityKind, EntityPublic, EntityUid, KindPublic,
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
/// 改坏确认的取值；正式构建里恒为 0，连 `Level` 上的钩子都不存在。
///
/// The break-test value; always 0 in production, where the `Level` hook is absent.
#[cfg(feature = "oracle-samples")]
fn explosion_break(world: &World<'_>) -> u8 {
    world.level().explosion_break()
}

#[cfg(not(feature = "oracle-samples"))]
const fn explosion_break(_world: &World<'_>) -> u8 {
    0
}

impl World<'_> {
    /// `PrimedTnt.explode` 走到 `ServerLevel.explode` 和 `ServerExplosion.explode`：
    /// 破坏方块、连锁点燃 TNT、伤害掉落物，还伤生物和玩家，并把爆炸包发给范围内玩家。
    ///
    /// `center` 是爆炸中心，`radius` 是爆炸强度。`source` 是点燃的 TNT 这一刻的公开
    /// 数据（死亡消息用它拿 `entity.minecraft.tnt` 显示名）；`owner` 是点燃它的活体，
    /// 有 owner 时伤害来源是 `player_explosion`（`direct` 指 TNT、`causing` 指 owner），
    /// 没有时是 `explosion`（`direct` 指 TNT、`causing` 为空）。
    ///
    /// `center` is the explosion centre and `radius` is its power. `source` is the primed
    /// TNT's public data (the death message reads its `entity.minecraft.tnt` name); `owner`
    /// is the living entity that lit it. With an owner the damage source is `player_explosion`
    /// (`direct` is the TNT, `causing` the owner); without one it is `explosion` (`direct` is
    /// the TNT, no `causing`).
    pub fn explode_tnt(
        &mut self,
        center: Vec3,
        radius: f32,
        source: Box<EntityPublic>,
        owner: Option<EntityUid>,
    ) {
        // 对拍的改坏确认钩子，正式构建里恒为 0（`Level::explosion_break`）。
        let brk = explosion_break(self);
        // 位 1：丢掉 `PrimedTnt.getY(0.0625)` 的抬升（TNT 高 0.98）。
        let center = if brk & 2 != 0 {
            Vec3::new(center.x, center.y - f64::from(0.98_f32) * 0.0625, center.z)
        } else {
            center
        };
        let mut broken = self.calculate_exploded_positions(center, radius, None);
        let block_count = broken.len() as i32;
        // `hurtEntities` 在打乱方块之前，而且不取 `level.random`。
        // `hurtEntities` runs before the block list is shuffled and does not draw `level.random`.
        crate::entity::item::hurt_items_by_explosion(self, center, radius);
        let mut damage = tnt_blast_source(&source, owner);
        damage.position = Some(center);
        // 位 0：跳过 `hurtEntities`，牛不掉血、玩家包里没有击退向量。
        let hit_players = if brk & 1 != 0 {
            Vec::new()
        } else {
            hurt_living_by_explosion(self, Some(&source), damage, center, radius)
        };
        self.interact_with_blocks(&mut broken, None);
        self.push_effect(Effect::Explode {
            center,
            radius,
            block_count,
            hit_players,
            // TNT 总是破坏方块，`isSmall` 只看半径。
            // TNT always interacts with blocks, so `isSmall` only looks at the radius.
            is_small: radius < 2.0,
            play_sound: true,
        });
    }

    /// `ServerLevel.explode(source, ..., ExplosionInteraction.MOB)`：来源是一个生物。
    /// 射线之后先伤实体；`mobGriefing` 关着时方块不动（`BlockInteraction.KEEP`），
    /// 开着时按 `mobExplosionDropDecay` 决定掉落带不带衰减。爆炸包发给范围内玩家。
    ///
    /// `ServerLevel.explode(source, ..., ExplosionInteraction.MOB)` for a mob source.
    /// Entities are hurt after the rays; blocks stay under disabled `mobGriefing`
    /// (`BlockInteraction.KEEP`), otherwise `mobExplosionDropDecay` picks decayed drops.
    /// The blast packet is sent to players in range.
    pub(crate) fn explode_mob(&mut self, source: &EntityPublic, center: Vec3, radius: f32) {
        let rules = self.block_game_rules();
        let mut broken = self.calculate_exploded_positions(center, radius, None);
        let block_count = broken.len() as i32;
        crate::entity::item::hurt_items_by_explosion(self, center, radius);
        let damage = blast_source(source);
        let hit_players = hurt_living_by_explosion(self, Some(source), damage, center, radius);
        let interacts = rules.mob_griefing;
        if interacts {
            let decay = rules.mob_explosion_drop_decay.then_some(radius);
            self.interact_with_blocks(&mut broken, decay);
        }
        let silent = matches!(&source.kind, KindPublic::Mob(mob) if mob.silent);
        self.push_effect(Effect::Explode {
            center,
            radius,
            block_count,
            hit_players,
            is_small: radius < 2.0 || !interacts,
            play_sound: !silent,
        });
    }

    /// `RespawnAnchorBlock.explode` 走到 `ServerLevel.explode(..., BLOCK)`：强度 5、带火，
    /// 伤害类型 `bad_respawn_point`。`anchor` 是被移除的重生锚那格，近水时按水的抗性算。
    ///
    /// `RespawnAnchorBlock.explode` lands in `ServerLevel.explode(..., BLOCK)`: power 5,
    /// fire, `bad_respawn_point` damage. `anchor` is the removed block position; its
    /// resistance becomes water's when the anchor sat next to water.
    pub(crate) fn explode_respawn_anchor(&mut self, center: Vec3, anchor: BlockPos) {
        // `removeBlock(pos, false)`：先移除重生锚，再算水和爆炸。
        self.remove_block(anchor);
        let rules = self.block_game_rules();
        let water_resistance = self.respawn_anchor_near_water(anchor).then_some((
            anchor,
            BlockState(blocks::WATER).block().explosion_resistance,
        ));
        let mut broken = self.calculate_exploded_positions(center, 5.0, water_resistance);
        let block_count = broken.len() as i32;
        crate::entity::item::hurt_items_by_explosion(self, center, 5.0);
        let mut damage = DamageSource::environment(DamageKind::BadRespawnPoint);
        damage.position = Some(center);
        let hit_players = hurt_living_by_explosion(self, None, damage, center, 5.0);
        let decay = rules.block_explosion_drop_decay.then_some(5.0);
        self.interact_with_blocks(&mut broken, decay);
        self.create_fire(&broken);
        self.push_effect(Effect::Explode {
            center,
            radius: 5.0,
            block_count,
            hit_players,
            is_small: false,
            play_sound: true,
        });
    }

    /// `RespawnAnchorBlock.explode` 的 `inWater`：四个水平邻居之一「会流的水」，
    /// 或者上方是水（`FluidTags.WATER`）。
    fn respawn_anchor_near_water(&self, anchor: BlockPos) -> bool {
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            if self.water_would_flow(anchor.offset(dx, 0, dz)) {
                return true;
            }
        }
        self.block_state(anchor.above()).fluid().is_water()
    }

    /// `RespawnAnchorBlock.isWaterThatWouldFlow`：源头水，或高度至少 2 且下方不是水的流水。
    fn water_would_flow(&self, pos: BlockPos) -> bool {
        let fluid = self.block_state(pos).fluid();
        if !fluid.is_water() {
            return false;
        }
        if fluid.is_source() {
            return true;
        }
        if fluid.amount() < 2 {
            return false;
        }
        !self.block_state(pos.below()).fluid().is_water()
    }

    /// `ServerExplosion.createFire`：每个被炸的格 1/3 概率，格子是空气且下方是完整渲染
    /// 方块时就放火。
    fn create_fire(&mut self, broken: &[BlockPos]) {
        for pos in broken {
            if self.random().next_int(3) != 0 {
                continue;
            }
            if !self.block_state(*pos).is_air() {
                continue;
            }
            if !self.block_state(pos.below()).is_solid_render() {
                continue;
            }
            self.set_block_and_update(*pos, BlockState(blocks::FIRE));
        }
    }

    /// `ServerExplosion.interactWithBlocks`：打乱、逐格破坏、最后一起弹出掉落。
    /// `decay` 是 `DESTROY_WITH_DECAY` 下交给战利品表的爆炸半径。打乱就地发生，
    /// 后面的 `createFire` 按打乱后的顺序放火（官方用的是同一个列表）。
    ///
    /// `ServerExplosion.interactWithBlocks`: shuffle, break each block, then pop the drops.
    /// `decay` is the radius handed to loot tables under `DESTROY_WITH_DECAY`. The shuffle
    /// is in place, so `createFire` later walks the same shuffled order.
    fn interact_with_blocks(&mut self, broken: &mut [BlockPos], decay: Option<f32>) {
        // `Util.shuffle(toBlow, level.random)`。
        self.random().shuffle(broken);
        // `interactWithBlocks`：先收齐掉落，整圈方块处理完才 `popResource`。
        // `interactWithBlocks` collects drops, then `popResource`s them after every block.
        let mut drops = Vec::new();
        for pos in broken.iter().copied() {
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
    /// `water_resistance` 是重生锚爆炸的水特判：被移除的那一格在近水时按水的抗性算
    /// （`RespawnAnchorBlock$1.getBlockExplosionResistance`）。
    ///
    /// 每条线开头从世界随机源取一个数，所以这个函数的调用次数和顺序都影响后续的随机数。
    fn calculate_exploded_positions(
        &mut self,
        center: Vec3,
        radius: f32,
        water_resistance: Option<(BlockPos, f32)>,
    ) -> Vec<BlockPos> {
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
                        let overridden = water_resistance
                            .filter(|(anchor, _)| *anchor == pos)
                            .map(|(_, resistance)| resistance);
                        if overridden.is_some() || !(state.is_air() && fluid.is_empty()) {
                            let resistance = overridden.unwrap_or_else(|| {
                                let resistance = state.block().explosion_resistance;
                                resistance.max(fluid_resistance(fluid))
                            });
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

/// `Explosion.getDefaultDamageSource(level, tnt)`：
/// `DamageSources.explosion(tnt, tnt.getIndirectSourceEntity())`。有 owner 时两个来源
/// 实体都在，是 `player_explosion`（`direct` 指 TNT、`causing` 指 owner）；没有时是
/// `explosion`（`direct` 指 TNT、`causing` 为空）。
///
/// `Explosion.getDefaultDamageSource(level, tnt)`, i.e.
/// `DamageSources.explosion(tnt, tnt.getIndirectSourceEntity())`. With an owner both
/// source entities exist and the kind is `player_explosion` (`direct` is the TNT,
/// `causing` the owner); otherwise `explosion` (`direct` is the TNT, no `causing`).
fn tnt_blast_source(source: &EntityPublic, owner: Option<EntityUid>) -> DamageSource {
    let mut damage = DamageSource::environment(if owner.is_some() {
        DamageKind::PlayerExplosion
    } else {
        DamageKind::Explosion
    });
    damage.direct = Some(source.uid);
    if let Some(owner) = owner {
        damage.attacker = Some(owner);
        damage.attacker_is_player = true;
    }
    damage
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
/// 生物再按爆炸击退抗性被推开，玩家记下自己那份击退向量（`getHitPlayers`）。
/// 掉落物由 [`crate::entity::item::hurt_items_by_explosion`] 处理。
/// 官方按实体分区的遍历顺序走一遍；这里先生物（uid 升序）后玩家。
/// 返回被击退的玩家 `(uid, 击退向量)`，供爆炸包逐玩家带一份。
///
/// The mob and player part of `ServerExplosion.hurtEntities`: damage from distance and
/// exposure, plus knockback for mobs scaled by explosion knockback resistance; players
/// record their own knockback (`getHitPlayers`). Items go through
/// [`crate::entity::item::hurt_items_by_explosion`]. Vanilla walks entity sections once;
/// this goes mobs (ascending uid) then players. Returns the hit players as
/// `(uid, knockback)` for the per-player blast packet.
fn hurt_living_by_explosion(
    world: &mut World,
    source: Option<&EntityPublic>,
    damage: DamageSource,
    center: Vec3,
    radius: f32,
) -> Vec<(EntityUid, Vec3)> {
    if radius < 1.0e-5 {
        return Vec::new();
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
    let mut hit_players = Vec::new();
    for player in players {
        if let Some((amount, knock)) = player_blast(world, &player, center, double_radius, area) {
            world.hurt_player_from_mob(player.uid, damage, amount, source);
            // 创造且飞行中的玩家照官方不进 `getHitPlayers`（伤害和 push 照给）。
            if !(player.creative && player.flying) {
                hit_players.push((player.uid, knock));
            }
        }
    }
    hit_players
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

/// 一名玩家在爆炸里受的伤害和击退向量。旁观者和死人不算（`EntitySelector.NO_SPECTATORS`）。
/// 击退向量就是 `getHitPlayers` 里那份，客户端按 `ClientboundExplodePacket` 推开玩家。
///
/// The damage and knockback one player takes. Spectators and the dead are skipped; the
/// knockback is the `getHitPlayers` entry the client applies from `ClientboundExplodePacket`.
fn player_blast(
    world: &World,
    player: &PlayerView,
    center: Vec3,
    double_radius: f64,
    area: Aabb,
) -> Option<(f32, Vec3)> {
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
    // 爆炸击退抗性和生物一样，读 `EXPLOSION_KNOCKBACK_RESISTANCE`（爆炸保护、盔甲修饰符）。
    let resistance = player.explosion_knockback_resistance;
    let push = (1.0 - dist) * f64::from(exposure) * (1.0 - resistance);
    let knock = player
        .eye_position()
        .subtract(center)
        .normalize()
        .scale(push);
    Some((blast_damage(dist, exposure, double_radius), knock))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockTicks, testing::TestLevel};

    /// 站在 `(2, 64, 0)` 的生存玩家视图，可改击退抗性和飞行状态。
    fn player(uid: i32, resistance: f64, creative: bool, flying: bool) -> PlayerView {
        let mut view = crate::entity::mob::throttle::test_player(uid, Vec3::new(2.0, 64.0, 0.0));
        view.explosion_knockback_resistance = resistance;
        view.creative = creative;
        view.flying = flying;
        view
    }

    /// 爆炸击退按 `1 - EXPLOSION_KNOCKBACK_RESISTANCE` 缩放：抗性 0.5 的玩家
    /// 击退只有抗性 0 的一半。两个玩家站在同一格，曝光度和方向完全一致。
    #[test]
    fn blast_knockback_scales_with_explosion_resistance() {
        let mut level = TestLevel::new();
        level.players.push(player(0, 0.0, false, false));
        level.players.push(player(1, 0.5, false, false));
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        let damage = DamageSource::environment(DamageKind::Explosion);
        let hit =
            hurt_living_by_explosion(&mut world, None, damage, Vec3::new(0.0, 64.0, 0.0), 4.0);
        let knock = |uid: i32| {
            hit.iter()
                .find(|(u, _)| *u == uid)
                .map(|(_, k)| k.length())
                .expect("the player is knocked back")
        };
        let full = knock(0);
        let half = knock(1);
        assert!(full > 0.0);
        assert!(
            (half - full * 0.5).abs() < 1.0e-6,
            "half resistance halves the knockback: {half} vs {full}"
        );
    }

    /// 创造且飞行中的玩家照官方不进 `getHitPlayers`，但同格子的普通玩家照常进。
    #[test]
    fn creative_flying_player_is_left_out_of_hit_players() {
        let mut level = TestLevel::new();
        level.players.push(player(0, 0.0, false, false));
        level.players.push(player(1, 0.0, true, true));
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        let damage = DamageSource::environment(DamageKind::Explosion);
        let hit =
            hurt_living_by_explosion(&mut world, None, damage, Vec3::new(0.0, 64.0, 0.0), 4.0);
        assert_eq!(hit.len(), 1);
        assert_eq!(
            hit[0].0, 0,
            "the creative flying player is left out of getHitPlayers"
        );
    }
}

#[cfg(test)]
mod tnt_tests {
    use super::*;
    use crate::{
        BlockTicks,
        entity::{Entity, mob::MobType},
        testing::TestLevel,
    };

    /// `tnt_blast_source`：有 owner 是 `player_explosion`（`direct` 指 TNT、`attacker` 指 owner），
    /// 没有 owner 是 `explosion`（只有 `direct`）。
    #[test]
    fn tnt_blast_source_sets_direct_and_owner() {
        let tnt = EntityPublic {
            uid: 7,
            kind: KindPublic::Tnt { fuse: 0 },
            position: Vec3::default(),
            delta: Vec3::default(),
            on_ground: false,
            yaw: 0.0,
            removed: false,
            precise: false,
            vehicle: None,
        };
        let plain = tnt_blast_source(&tnt, None);
        assert_eq!(plain.kind, DamageKind::Explosion);
        assert_eq!(plain.direct, Some(7));
        assert_eq!(plain.attacker, None);
        let owned = tnt_blast_source(&tnt, Some(3));
        assert_eq!(owned.kind, DamageKind::PlayerExplosion);
        assert_eq!(owned.direct, Some(7));
        assert_eq!(owned.attacker, Some(3));
        assert!(owned.attacker_is_player);
    }

    /// TNT 也伤生物：爆炸范围内的牛掉血，并被推开（速度变了）。
    #[test]
    fn tnt_hurts_a_nearby_mob() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::new();
        let mut world = World::new(&mut level, &mut ticks);
        let id = world.spawn_entity(Entity::by_mob_type(MobType::Cow, Vec3::new(2.0, 64.0, 0.0)));
        let before = world.entity(id).expect("the cow is in the world");
        let before_health = before.kind.mob().expect("a cow is a mob").health;
        world.explode_tnt(
            Vec3::new(0.0, 64.0, 0.0),
            4.0,
            Box::new(EntityPublic {
                uid: 0,
                kind: KindPublic::Tnt { fuse: 0 },
                position: Vec3::new(0.0, 64.0, 0.0),
                delta: Vec3::default(),
                on_ground: false,
                yaw: 0.0,
                removed: false,
                precise: false,
                vehicle: None,
            }),
            None,
        );
        let after = world.entity(id).expect("the cow survives");
        let after_mob = after.kind.mob().expect("a cow is a mob");
        assert!(
            after_mob.health < before_health,
            "the TNT blast should hurt the cow: {before_health} -> {}",
            after_mob.health
        );
        assert_ne!(
            after.delta, before.delta,
            "the blast should knock the cow back"
        );
    }
}
