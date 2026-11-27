//! 玩家的环境伤害：摔落、溺水、着火 / 岩浆 / 火焰方块、窒息（卡在方块里）、
//! 虚空、仙人掌 / 甜浆果 / 岩浆块、细雪冻伤。
//!
//! 官方入口：`Entity.baseTick`（`lavaHurt`、`checkInsideBlocks`、
//! `remainingFireTicks % 20`）、`LivingEntity.baseTick`（`decreaseAirSupply`、
//! `isInWall`、`y < minY-64`）、`Entity.causeFallDamage` →
//! `LivingEntity.calculateFallDamage` / `calculateFallPower`。
//! 伤害统一走 [`crate::player_hurt::hurt_player`]，不复制减伤 / 死亡 / 发包链路。
//!
//! 假世界只跑这几段独立入口（见 `MobOracle.playEnv` 的注释）；完整
//! `ServerPlayer.tick()` 需要连接层和区块票，和本模块比较的字段无关。
//!
//! Player environment damage. Everything routes through the shared hurt path.
//! All new state is a relative countdown, never an island-clock instant.

use mc_bamboo_blocks::{
    BlockState,
    entity::mob::{DamageKind, DamageSource},
};
use mc_bamboo_core::{Aabb, BlockPos, Vec3};
use mc_bamboo_data::{
    blocks,
    registries::{attribute, mob_effect},
    tags::fluid_tags,
};

use crate::{inbound::PlayerId, player::Player, region::Region};

/// 官方 `Entity.MAX_AIR_SUPPLY` / `Player.getMaxAirSupply()`。
pub const MAX_AIR: i32 = 300;
/// `shouldTakeDrowningDamage()` 的阈值：`air <= -20`。
const DROWN_AT: i32 = -20;
/// 官方 `Entity.getFireImmuneTicks()`：玩家是 0，没着火时 `remainingFireTicks`
/// 被设成 `-0`。这里只用一个整数表示剩余刻数。
const FIRE_TICKS_PER_SECOND: i32 = 20;
/// `Player.getFireImmuneTicks()`：没着火时 `remainingFireTicks` 是 -20。
const FIRE_IMMUNE_TICKS: i32 = 20;
/// 细雪每刻涨的冻结值上限，官方 `Entity.TICKS_FROZEN_MAX` = 140。
const MAX_FROZEN: i32 = 140;
/// 官方 `LivingEntity.calculateFallPower` 的 `1.0E-6` 容差。
const FALL_EPSILON: f64 = 1.0e-6;

/// 玩家身上按实体存的那部分环境状态。全是相对倒计时 / 累计量。
///
/// Per-player environment state. All relative counters, never absolute time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Environment {
    /// `Entity.remainingFireTicks`：正数表示着火，0 表示没着火。
    pub remaining_fire_ticks: i32,
    /// `Entity.airSupply`。
    pub air_supply: i32,
    /// `Entity.ticksFrozen`（细雪）。
    pub ticks_frozen: i32,
    /// 这一刻眼睛是否在细雪里（`inPowderSnow`），下一刻清零。
    pub in_powder_snow: bool,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            remaining_fire_ticks: 0,
            air_supply: MAX_AIR,
            ticks_frozen: 0,
            in_powder_snow: false,
        }
    }
}

impl Environment {
    /// 官方 `PlayerData`：Fire / Air / TicksFrozen。
    pub fn save(&self, data: &mut mc_bamboo_storage::player_data::PlayerData) {
        data.fire = self
            .remaining_fire_ticks
            .clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        data.air = self.air_supply.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        data.ticks_frozen = self.ticks_frozen;
    }

    pub fn restore(&mut self, data: &mc_bamboo_storage::player_data::PlayerData) {
        self.remaining_fire_ticks = i32::from(data.fire);
        self.air_supply = i32::from(data.air);
        self.ticks_frozen = data.ticks_frozen;
        self.in_powder_snow = false;
    }
}

/// `Entity.igniteForTicks`：只在现有剩余刻数更短时延长。
///
/// `Entity.igniteForTicks`: only extends a shorter remaining burn.
pub(crate) fn ignite_for_ticks(region: &mut Region, id: PlayerId, ticks: i32) {
    if let Some(player) = region.players.get_mut(&id) {
        ignite_player(player, ticks);
    }
}

/// `Entity.igniteForTicks` 落在一名玩家上：剩余刻数更短才延长，然后 `clearFreeze`。
pub(crate) fn ignite_player(player: &mut Player, ticks: i32) {
    if player.environment.remaining_fire_ticks < ticks {
        set_player_fire_ticks(player, ticks);
    }
    if player.environment.ticks_frozen != 0 {
        player.environment.ticks_frozen = 0;
    }
    player.sync.metadata_dirty = true;
}

/// `Player.setRemainingFireTicks`：能力无敌的玩家最多保留 1 刻。
///
/// `Player.setRemainingFireTicks`: ability invulnerability caps the timer at one tick.
pub(crate) fn set_player_fire_ticks(player: &mut Player, ticks: i32) {
    player.environment.remaining_fire_ticks =
        if player.game_mode == mc_bamboo_protocol::packet::play::GameMode::Creative {
            ticks.min(1)
        } else {
            ticks
        };
}

/// `Entity.clearFire`。
#[allow(
    dead_code,
    reason = "used by rain and milk in the environment/consumption paths"
)]
pub(crate) fn clear_fire(region: &mut Region, id: PlayerId) {
    if let Some(player) = region.players.get_mut(&id)
        && player.environment.remaining_fire_ticks > 0
    {
        player.environment.remaining_fire_ticks = 0;
        player.sync.metadata_dirty = true;
    }
}

/// 玩家有没有着火（实体数据位 0 用）。
pub(crate) fn is_on_fire(player: &Player) -> bool {
    player.environment.remaining_fire_ticks > 0
}

/// `LivingEntity.calculateFallDamage`：
/// `floor((fallDistance + 1e-6 - getAttributeValue(SAFE_FALL_DISTANCE)) * getAttributeValue(FALL_DAMAGE_MULTIPLIER))`。
/// 跳跃提升效果已经通过 `minecraft:effect.jump_boost` 修饰符加在 `SAFE_FALL_DISTANCE` 上。
///
/// Vanilla `calculateFallDamage`, reading `SAFE_FALL_DISTANCE` (which already
/// includes Jump Boost) and `FALL_DAMAGE_MULTIPLIER` from the attribute map.
pub(crate) fn fall_damage(player: &Player, fall_distance: f64) -> i32 {
    let safe = player.attributes.value(attribute::SAFE_FALL_DISTANCE);
    let power = fall_distance + FALL_EPSILON - safe;
    mc_bamboo_core::mth::floor(power * player.attributes.value(attribute::FALL_DAMAGE_MULTIPLIER))
        .max(0)
}

/// `Entity.causeFallDamage` 的落地分支：伤害 > 0 才走完整受伤链路。
/// 水 / 缓降 / 漂浮在移动校验里已经把 `fall_distance` 清零。
///
/// Landing branch of `Entity.causeFallDamage`; a positive amount goes through
/// the full hurt path.
pub(crate) fn on_land(region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    let distance = player.motion.fall_distance;
    let position = player.position();
    let amount = fall_damage(player, distance);
    let alive = player.health.health > 0.0;
    // `ServerPlayer.resetFallDistance`：还活着且记下了起跳位置时触发距离判据。
    // Bamboo 没有 `startingToFallPosition`，落点 y 加上下落距离就是起点 y。
    if alive {
        crate::advancement_triggers::record(
            region,
            id,
            "minecraft:fall_from_height",
            serde_json::json!({
                "start_y": position.y + distance,
                "distance": {"y": distance},
                "player_y": position.y,
            }),
        );
    }
    if amount > 0 {
        // `getFallDamageSound` 在 `hurt` 之前播。
        crate::player_hurt::play_fall_sound(region, id, amount, position);
        hurt(region, id, DamageKind::Fall, amount as f32);
    }
    if let Some(player) = region.players.get_mut(&id) {
        player.motion.fall_distance = 0.0;
    }
}

/// 一刻度环境判定：`Entity.baseTick` / `LivingEntity.baseTick` 的等价拆分。
/// 不跑移动物理 / `ServerPlayer.doTick` 的其它阶段（对拍两侧同样不跑）。
///
/// One environment tick: the equivalent split of the two official base ticks.
pub(crate) fn tick_environment(region: &mut Region) {
    let ids: Vec<PlayerId> = region.players.keys().copied().collect();
    tick_environment_players(region, &ids);
}

/// 只推进给定玩家（对拍假世界里的观察者不参与）。
///
/// Advances only the given players (an oracle observer does not take part).
pub(crate) fn tick_environment_players(region: &mut Region, ids: &[PlayerId]) {
    for &id in ids {
        tick_player(region, id);
    }
}

fn tick_player(region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if player.dead || player.spawn_pending.is_some() {
        // 官方 `Entity.applyEffectsFromBlocks` 即使死了也把没点着的
        // `remainingFireTicks` 设成 `-getFireImmuneTicks()`。
        if let Some(player) = region.players.get_mut(&id)
            && player.environment.remaining_fire_ticks == 0
        {
            player.environment.remaining_fire_ticks = -FIRE_IMMUNE_TICKS;
        }
        return;
    }
    let creative = player.game_mode == mc_bamboo_protocol::packet::play::GameMode::Creative;
    let position = player.position();
    let fire = player.environment.remaining_fire_ticks;
    let air = player.environment.air_supply;
    let frozen = player.environment.ticks_frozen;
    let eye_y = position.y + player.eye_height();
    let eye = BlockPos::containing(Vec3::new(position.x, eye_y, position.z));
    let feet = BlockPos::containing(position);
    let below = feet.offset(0, -1, 0);
    let view = EnvironmentView {
        chunks: &region.chunks,
    };
    let eye_state = view.state(eye);
    let below_state = view.state(below);
    let body = view.body_states(position, eye_y);
    // `LivingEntity.isInWall`：眼睛那一小片（宽 = 体型宽 * 0.8，上下各 5e-7）
    // 和会让人窒息的方块碰撞形状相交才算。整盒扫会把站在方块旁边的玩家误伤。
    // Vanilla `isInWall`: only a thin slice at eye height intersects the
    // suffocating block's collision shapes.
    let in_wall = {
        let width = 0.6_f64 * 0.8;
        let slice = Aabb::new(
            position.x - width / 2.0,
            eye_y - 0.5e-6,
            position.z - width / 2.0,
            position.x + width / 2.0,
            eye_y + 0.5e-6,
            position.z + width / 2.0,
        );
        let min = BlockPos::containing(Vec3::new(slice.min_x, slice.min_y, slice.min_z));
        let max = BlockPos::containing(Vec3::new(slice.max_x, slice.max_y, slice.max_z));
        let mut found = false;
        'outer: for x in min.x..=max.x {
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    let pos = BlockPos::new(x, y, z);
                    let state = view.state(pos);
                    if state.is_air() || !state.is_suffocating() {
                        continue;
                    }
                    let boxes = mc_bamboo_blocks::shape::collision_boxes(
                        state,
                        pos,
                        mc_bamboo_blocks::CollisionContext::Empty,
                    );
                    if boxes.iter().any(|part| part.at(pos).intersects(slice)) {
                        found = true;
                        break 'outer;
                    }
                }
            }
        }
        found
    };
    let in_fire = body
        .iter()
        .any(|(state, _)| state.is(blocks::FIRE) || state.is(blocks::SOUL_FIRE));
    let in_lava = body
        .iter()
        .any(|(state, _)| state.fluid().is_tagged(fluid_tags::LAVA));
    let in_water_eye = eye_state.fluid().is_tagged(fluid_tags::WATER);
    let bubble = eye_state.is(blocks::BUBBLE_COLUMN);
    let in_cactus = body.iter().any(|(state, _)| state.is(blocks::CACTUS));
    let in_berry = body
        .iter()
        .any(|(state, _)| state.is(blocks::SWEET_BERRY_BUSH));
    let in_powder = body.iter().any(|(state, _)| state.is(blocks::POWDER_SNOW));
    let on_magma = player.on_ground && below_state.is(blocks::MAGMA_BLOCK);

    // `Entity.baseTick`：着火每 20 刻 1 点 `on_fire`，岩浆里不重复扣。
    if fire > 0 && fire % FIRE_TICKS_PER_SECOND == 0 && !in_lava {
        hurt(region, id, DamageKind::OnFire, 1.0);
    }
    // `Entity.baseTick` 先把剩余刻数减 1（后面 `applyEffectsFromBlocks`
    // 的火焰 / 岩浆可能重新点燃，`gained` 由那时比较得出）。
    let fire_before_blocks = fire.saturating_sub(1).max(0);
    if let Some(player) = region.players.get_mut(&id) {
        player.environment.remaining_fire_ticks = fire_before_blocks;
    }
    // 虚空：`minY - 64`，Bamboo 的超平坦下界是 -64。
    if position.y < -128.0 {
        hurt(region, id, DamageKind::OutOfWorld, 4.0);
    }
    // `Entity.applyEffectsFromBlocks` 的 `InsideBlockEffectApplier`：
    // 火焰 / 岩浆点燃并扣血，发生在 `baseTick` 的减刻之后。
    if in_lava {
        hurt(region, id, DamageKind::Lava, 4.0);
        ignite_for_ticks(region, id, 15 * FIRE_TICKS_PER_SECOND);
    }
    if in_fire {
        hurt(region, id, DamageKind::InFire, 1.0);
        ignite_for_ticks(region, id, 8 * FIRE_TICKS_PER_SECOND);
    }
    // `LivingEntity.baseTick`：眼睛在水里且不是气泡柱时减氧气；
    // `MobEffectUtil.hasWaterBreathing` 包含水下呼吸 / 潮涌能量 / 鹦鹉螺之息；
    // `LivingEntity.decreaseAirSupply` 读 `Attributes.OXYGEN_BONUS`（水下呼吸附魔等），
    // 概率 `1/(bonus+1)` 减氧，取玩家实体随机源。
    let (oxygen_bonus, water_breathing, refill_air, roll) = {
        let Some(player) = region.players.get_mut(&id) else {
            return;
        };
        let bonus = player.attributes.value(attribute::OXYGEN_BONUS);
        let has_wb = player.effects.has(mob_effect::WATER_BREATHING);
        let has_conduit = player.effects.has(mob_effect::CONDUIT_POWER);
        let has_nautilus = player.effects.has(mob_effect::BREATH_OF_THE_NAUTILUS);
        let water_breathing = has_wb || has_conduit || has_nautilus;
        // `MobEffectUtil.shouldEffectsRefillAirsupply`：只有鹦鹉螺之息时停氧但不补氧。
        // `MobEffectUtil.shouldEffectsRefillAirsupply`: Breath of the Nautilus alone pauses without refilling.
        let refill_air = !has_nautilus || has_wb || has_conduit;
        let roll = if in_water_eye && !bubble && !water_breathing && !creative && bonus > 0.0 {
            player.random.next_double()
        } else {
            0.0
        };
        (bonus, water_breathing, refill_air, roll)
    };
    let skip_air_loss = oxygen_bonus > 0.0 && roll >= 1.0 / (oxygen_bonus + 1.0);
    let air = if in_water_eye && !bubble && !water_breathing && !creative {
        if skip_air_loss { air } else { air - 1 }
    } else if refill_air {
        (air + 4).min(MAX_AIR)
    } else {
        air
    };
    let drown = air <= DROWN_AT;
    let air = if drown { 0 } else { air };
    if drown {
        // `broadcastEntityEvent(this, 67)`：溺水气泡。
        region.links.send(
            id,
            &mc_bamboo_protocol::packet::play::EntityEvent {
                entity_id: id.0,
                event: 67,
            },
        );
    }
    let frozen = if in_powder {
        (frozen + 1).min(MAX_FROZEN)
    } else {
        (frozen - 2).max(0)
    };
    if let Some(player) = region.players.get_mut(&id) {
        // `Entity.baseTick` 末尾：没在火里、这一刻也没重新点燃时设成
        // `-getFireImmuneTicks()`（玩家是 -20）；点燃过的保留新值。
        let burning = player.environment.remaining_fire_ticks;
        let gained = burning > fire_before_blocks;
        if burning <= 0 && !gained {
            player.environment.remaining_fire_ticks = -FIRE_IMMUNE_TICKS;
        }
        player.environment.air_supply = air;
        player.environment.ticks_frozen = frozen;
        player.environment.in_powder_snow = in_powder;
    }
    if drown {
        // `damageSources().drown()` 2 点。
        hurt(region, id, DamageKind::Drown, 2.0);
    }
    // 细雪每 40 刻 1 点冻伤；和平难度不掉血。
    if in_powder && frozen >= 40 && frozen % 40 == 0 && region.global.difficulty as i32 != 0 {
        hurt(region, id, DamageKind::Freeze, 1.0);
    }
    // 卡墙 / 仙人掌 / 甜浆果 / 岩浆块：都在 `Entity.checkInsideBlocks` 里。
    if in_wall {
        // `ServerPlayer.onInsideBlock`：每刻在方块里都触发 ENTER_BLOCK。
        let block = mc_bamboo_data::blocks::BLOCKS
            .get(eye_state.0 as usize)
            .map(|block| block.name)
            .unwrap_or("minecraft:air");
        crate::advancement_triggers::record(
            region,
            id,
            "minecraft:enter_block",
            serde_json::json!({"block": block, "blocks": block}),
        );
        hurt(region, id, DamageKind::InWall, 1.0);
    }
    if in_cactus {
        hurt(region, id, DamageKind::Cactus, 1.0);
    }
    if in_berry {
        hurt(region, id, DamageKind::SweetBerryBush, 1.0);
    }
    if on_magma {
        hurt(region, id, DamageKind::HotFloor, 1.0);
    }
}

fn hurt(region: &mut Region, id: PlayerId, kind: DamageKind, amount: f32) {
    crate::player_hurt::hurt_player(
        region,
        id,
        DamageSource::environment(kind),
        amount,
        &mut None,
    );
}

/// 玩家碰撞盒里那几格的方块状态，只用来判「在里面」。
/// 和官方 `Entity.checkInsideBlocks` 的 Aabb 收缩一致：`0.001` 内缩，
/// 遍历盒覆盖的每个方块坐标。
/// `Entity.isInWaterOrRain()`：身体接触水，或者脚下 / 头顶那一格在下 RAIN。
/// 激流三叉戟的 `use` / `releaseUsing` 用它判断能不能用；放在通用层供别处复用。
///
/// `Entity.isInWaterOrRain()`: the body touches water, or the feet / bounding-box
/// top is being rained on. Used by riptide's `use` / `releaseUsing` gates.
pub(crate) fn is_in_water_or_rain(region: &mut Region, id: PlayerId) -> bool {
    let Some(player) = region.players.get(&id) else {
        return false;
    };
    let position = player.position();
    let eye_y = position.y + player.eye_height();
    let in_water = {
        let view = EnvironmentView {
            chunks: &region.chunks,
        };
        view.body_states(position, eye_y)
            .iter()
            .any(|(state, _)| state.fluid().is_tagged(fluid_tags::WATER))
    };
    if in_water {
        return true;
    }
    // `Entity.isInRain`：`isRainingAt(blockPosition) || isRainingAt(containing(x, bb.maxY, z))`。
    let feet = BlockPos::containing(position);
    let top = BlockPos::containing(Vec3::new(position.x, position.y + 1.8, position.z));
    let (mut level, ticks) = region.block_level();
    let world = mc_bamboo_blocks::World::new(&mut level, ticks);
    world.is_raining_at(feet) || world.is_raining_at(top)
}

pub(crate) struct EnvironmentView<'a> {
    chunks: &'a mc_bamboo_world::store::ChunkStore,
}

impl<'a> EnvironmentView<'a> {
    pub(crate) const fn new(chunks: &'a mc_bamboo_world::store::ChunkStore) -> Self {
        Self { chunks }
    }

    fn state(&self, pos: BlockPos) -> BlockState {
        BlockState(self.chunks.block(pos).unwrap_or(blocks::VOID_AIR))
    }

    pub(crate) fn body_states(&self, position: Vec3, eye_y: f64) -> Vec<(BlockState, BlockPos)> {
        let half = 0.3_f64 - 0.001;
        let bounds = Aabb::new(
            position.x - half,
            position.y + 0.001,
            position.z - half,
            position.x + half,
            (position.y + 1.8_f64)
                .min(eye_y + 0.2)
                .max(position.y + 0.001),
            position.z + half,
        );
        let min = BlockPos::containing(Vec3::new(bounds.min_x, bounds.min_y, bounds.min_z));
        let max = BlockPos::containing(Vec3::new(bounds.max_x, bounds.max_y, bounds.max_z));
        let mut out = Vec::new();
        for x in min.x..=max.x {
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    let pos = BlockPos::new(x, y, z);
                    let state = self.state(pos);
                    if !state.is_air() {
                        out.push((state, pos));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    fn region_with_player() -> (Region, PlayerId) {
        let mut region = Region::new(2, 20, false);
        let id = PlayerId(1);
        let player = Player::new(
            id,
            Uuid::nil(),
            "Test".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        region.insert_player(player);
        (region, id)
    }

    #[test]
    fn ignite_extends_but_clear_fire_and_save_round_trip_keep_relative_ticks() {
        let (mut region, id) = region_with_player();
        ignite_for_ticks(&mut region, id, 160);
        assert_eq!(region.players[&id].environment.remaining_fire_ticks, 160);
        ignite_for_ticks(&mut region, id, 80);
        assert_eq!(region.players[&id].environment.remaining_fire_ticks, 160);
        assert!(is_on_fire(&region.players[&id]));
        clear_fire(&mut region, id);
        assert!(!is_on_fire(&region.players[&id]));
        // 存档往返：Fire / Air / TicksFrozen 都是相对量，拆岛不用平移。
        region.players.get_mut(&id).unwrap().environment = Environment {
            remaining_fire_ticks: 200,
            air_supply: 137,
            ticks_frozen: 77,
            in_powder_snow: true,
        };
        let mut data = mc_bamboo_storage::player_data::PlayerData::default();
        region.players[&id].environment.save(&mut data);
        let mut restored = Environment::default();
        restored.restore(&data);
        assert_eq!(restored.remaining_fire_ticks, 200);
        assert_eq!(restored.air_supply, 137);
        assert_eq!(restored.ticks_frozen, 77);
        assert!(!restored.in_powder_snow);
    }

    #[test]
    fn fall_damage_follows_the_official_formula() {
        let (_, id) = region_with_player();
        let player = Player::new(
            id,
            Uuid::nil(),
            "Test".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        // `floor(d + 1e-6 - 3)`：4 格 1 点，3 格 0 点，0 格负无穷取 0。
        assert_eq!(fall_damage(&player, 4.0), 1);
        assert_eq!(fall_damage(&player, 3.0), 0);
        assert_eq!(fall_damage(&player, 0.0), 0);
        // 改坏确认：跳跃提升 I 把安全距离抬到 5，4 格不再掉血。
        let mut boosted = Player::new(
            id,
            Uuid::nil(),
            "Boosted".to_owned(),
            GameMode::Survival,
            ClientInformation::default(),
            Vec3::new(0.5, 64.0, 0.5),
        );
        boosted.effects.add(
            mc_bamboo_blocks::entity::mob::effects::EffectInstance::with_amplifier(
                mob_effect::JUMP_BOOST,
                600,
                1,
            ),
        );
        crate::player_attributes::rebuild_player(&mut boosted, false);
        assert_eq!(fall_damage(&boosted, 4.0), 0);
        assert_ne!(fall_damage(&boosted, 4.0), fall_damage(&player, 4.0));
    }
}
