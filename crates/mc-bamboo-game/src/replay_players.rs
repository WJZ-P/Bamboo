//! 对拍用：生物对拍的假世界借一个真的 [`Region`] 来放玩家，生物打玩家时走
//! 和精确驱动同一个正式入口（[`crate::player_hurt::hurt_player_by_mob`]）。
//!
//! Oracle helper: the mob oracle's fake world keeps its players in a real
//! [`Region`], so mob hits run the same production entry as the precise driver.

use std::sync::{Arc, Mutex};

use mc_bamboo_blocks::{
    PlayerHurtNow,
    entity::{EntityPublic, EntityUid, ctx::PlayerView, mob::DamageSource},
};
use mc_bamboo_core::Vec3;
use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};

use crate::{inbound::PlayerId, player::Player, region::Region};

/// 一名对拍玩家的起点。
#[derive(Debug, Clone)]
pub struct ReplayPlayerSetup {
    pub id: i32,
    pub uuid: u128,
    pub name: String,
    pub position: Vec3,
    pub health: f32,
    /// 玩家实体随机源的初始种子（`JavaRandom::new`）。
    pub seed: i64,
    /// 创造模式（否则生存）。
    pub creative: bool,
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl crate::inbound::PlayerSink for Capture {
    fn send(&mut self, frames: crate::outgoing::Outgoing) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extend(frames.to_raw_bytes());
        true
    }

    fn close(&mut self) {}
}

/// 放对拍玩家的区域和每名玩家收到的包。
pub struct ReplayPlayers {
    region: Region,
    captures: Vec<(PlayerId, Capture)>,
}

impl ReplayPlayers {
    /// 建一个只放玩家的区域。`difficulty` 是难度 ID。
    pub fn new(difficulty: i32, players: &[ReplayPlayerSetup]) -> Self {
        let mut region = Region::new(2, 20, false);
        region.global.difficulty = match difficulty {
            0 => crate::world_state::Difficulty::Peaceful,
            1 => crate::world_state::Difficulty::Easy,
            3 => crate::world_state::Difficulty::Hard,
            _ => crate::world_state::Difficulty::Normal,
        };
        let mut captures = Vec::new();
        for setup in players {
            let id = PlayerId(setup.id);
            let mut player = Player::new(
                id,
                uuid::Uuid::from_u128(setup.uuid),
                setup.name.clone(),
                if setup.creative {
                    GameMode::Creative
                } else {
                    GameMode::Survival
                },
                ClientInformation::default(),
                setup.position,
            );
            player.health.health = setup.health;
            player.health_sync = crate::player_hurt::HealthSync::of(&player);
            player.random = mc_bamboo_blocks::entity::JavaRandom::new(setup.seed);
            let capture = Capture::default();
            region.links.add(id, Box::new(capture.clone()));
            region.insert_player(player);
            captures.push((id, capture));
        }
        Self { region, captures }
    }

    /// 生物读到的玩家视图，和正式驱动的 `for_each_player` 同一个构造。
    pub fn views(&self) -> Vec<PlayerView> {
        self.region
            .players
            .iter()
            .map(|(id, player)| player.view(*id))
            .collect()
    }

    /// 生物打一名玩家。`attacker` 是打人的生物这一刻的公开数据。
    pub fn hurt(
        &mut self,
        target: EntityUid,
        source: DamageSource,
        amount: f32,
        attacker: Option<&EntityPublic>,
    ) -> PlayerHurtNow {
        let mut parts = self.region.hurt_parts();
        parts.attacker = attacker;
        let (hurt, death_drops) =
            crate::player_hurt::hurt_player_by_mob(&mut parts, PlayerId(target), source, amount);
        PlayerHurtNow { hurt, death_drops }
    }

    /// 生物给玩家点火。
    pub fn ignite(&mut self, target: EntityUid, ticks: i32) {
        if let Some(player) = self.region.players.get_mut(&PlayerId(target)) {
            crate::player_environment::ignite_player(player, ticks);
        }
    }

    /// 生物 / 喷溅药水给玩家挂一个非瞬时状态效果。
    ///
    /// Applies a non-instant status effect to a replay player.
    pub fn give_effect(
        &mut self,
        target: EntityUid,
        instance: mc_bamboo_blocks::entity::mob::effects::EffectInstance,
    ) {
        crate::player_effects::give(&mut self.region, PlayerId(target), instance);
    }

    /// 喷溅药水按距离缩放给玩家回血。
    ///
    /// Heals a replay player by a distance-scaled amount.
    pub fn heal(&mut self, target: EntityUid, amount: f32) {
        if let Some(player) = self.region.players.get_mut(&PlayerId(target))
            && player.health.health > 0.0
        {
            let max = crate::player_effects::max_health(player);
            player.health.health = (player.health.health + amount).min(max);
        }
    }

    /// 末影珍珠的传送：`ServerPlayer.teleport` 成功后的位置、速度清零和下落距离清零。
    ///
    /// An ender pearl teleport: the position plus the zeroed velocity and fall distance
    /// of a successful `ServerPlayer.teleport`.
    pub fn teleport(&mut self, target: EntityUid, position: Vec3) {
        let Region {
            players,
            player_grid,
            ..
        } = &mut self.region;
        let Some(player) = players.get_mut(&PlayerId(target)) else {
            return;
        };
        player.motion.delta = Vec3::default();
        player.motion.fall_distance = 0.0;
        player.on_ground = false;
        player.set_position(player_grid, position);
    }

    /// 对拍里玩家此刻的位置。
    ///
    /// The replay player's current position.
    pub fn position(&self, target: EntityUid) -> Vec3 {
        self.region
            .players
            .get(&PlayerId(target))
            .map_or(Vec3::default(), crate::player::Player::position)
    }

    /// 设玩家的朝向（`yaw`/`pitch`），末影螨 `snapTo` 主人时读它。
    ///
    /// Set the player's facing, read when an endermite snaps to its owner.
    pub fn set_rotation(&mut self, target: EntityUid, yaw: f32, pitch: f32) {
        if let Some(player) = self.region.players.get_mut(&PlayerId(target)) {
            player.yaw = yaw;
            player.pitch = pitch;
        }
    }

    /// 玩家当前生效的效果列表：`[效果 ID, 剩余时长, 等级]`（按 ID 升序）。
    ///
    /// Active effects on a replay player as `[effect_id, duration, amplifier]` sorted by id.
    pub fn effects(&self, target: EntityUid) -> Vec<[i32; 3]> {
        self.region
            .players
            .get(&PlayerId(target))
            .map_or_else(Vec::new, |player| {
                player
                    .effects
                    .entries()
                    .iter()
                    .map(|effect| [effect.effect, effect.duration, effect.amplifier])
                    .collect()
            })
    }

    /// 玩家阶段的 `set_health`（官方 `ServerPlayer.doTick`）：血量快照变了就补一包。
    /// 活着的玩家被生物打中时不当场发，等这里。
    pub fn sync_health(&mut self) {
        let mut pending = Vec::new();
        for (id, player) in &mut self.region.players {
            if player.dead {
                continue;
            }
            let now = crate::player_hurt::HealthSync::of(player);
            if now != player.health_sync {
                player.health_sync = now;
                pending.push((
                    *id,
                    mc_bamboo_protocol::packet::play::SetHealth {
                        health: now.health,
                        food: now.food,
                        saturation: player.health.food.saturation,
                    },
                ));
            }
        }
        for (id, packet) in pending {
            self.region.links.send(id, &packet);
        }
    }

    /// 每刻推进受伤冷却和受伤动画（`ServerPlayer.tick` / `LivingEntity.baseTick` 的那两项）。
    pub fn tick_cooldowns(&mut self) {
        for player in self.region.players.values_mut() {
            player.health.tick_cooldowns();
        }
    }

    /// `ElderGuardian.customServerAiStep`：给挂上挖掘疲劳的玩家发
    /// `GUARDIAN_ELDER_EFFECT` 游戏事件包（静音时参数 0、否则 1）。
    ///
    /// `ElderGuardian.customServerAiStep`: send the `GUARDIAN_ELDER_EFFECT`
    /// game-event packet (param 0 when silent, else 1).
    pub fn guardian_elder_effect(&mut self, target: EntityUid, silent: bool) {
        use mc_bamboo_protocol::packet::play::GameEvent;
        let packet = GameEvent {
            event: GameEvent::GUARDIAN_ELDER_EFFECT,
            param: if silent { 0.0 } else { 1.0 },
        };
        self.region
            .links
            .send(crate::inbound::PlayerId(target), &packet);
    }

    /// 对拍场景设玩家游戏模式（0 生存 / 1 创造 / 2 冒险 / 3 旁观）。
    /// `ReplayPlayerSetup` 只带 creative 布尔，其余模式用这个补上。
    ///
    /// Set the game mode for a replay player (0 survival / 1 creative /
    /// 2 adventure / 3 spectator); `ReplayPlayerSetup` only carries a bool.
    pub fn set_game_mode(&mut self, target: EntityUid, game_mode: i32) {
        let mode = match game_mode {
            1 => GameMode::Creative,
            2 => GameMode::Adventure,
            3 => GameMode::Spectator,
            _ => GameMode::Survival,
        };
        if let Some(player) = self.region.players.get_mut(&PlayerId(target)) {
            player.game_mode = mode;
        }
    }

    /// 给弹射物定点场景设无敌、火焰和冰冻初值。
    ///
    /// Set invulnerability, fire and freezing in a projectile fixture.
    pub fn set_arrow_target(
        &mut self,
        target: EntityUid,
        invulnerable: bool,
        thermal: Option<[i32; 2]>,
    ) {
        let player = self.region.players.get_mut(&PlayerId(target)).unwrap();
        if invulnerable {
            player.game_mode = GameMode::Creative;
        }
        if let Some([fire, frozen]) = thermal {
            player.environment.remaining_fire_ticks = fire;
            player.environment.ticks_frozen = frozen;
        }
    }

    /// 火焰和冰冻计时独立于血量快照。
    ///
    /// Returns the fire timer and freeze timer separately from the health snapshot.
    pub fn arrow_target_thermal(&self, target: EntityUid) -> [i32; 2] {
        let env = &self.region.players[&PlayerId(target)].environment;
        [env.remaining_fire_ticks, env.ticks_frozen]
    }

    /// 比较箭命中的附魔推力单独使用的玩家速度。
    ///
    /// Player velocity for comparison of the arrow's enchanted push.
    pub fn arrow_target_motion(&self, target: EntityUid) -> Vec3 {
        self.region.players[&PlayerId(target)].motion.delta
    }

    /// 逐刻比对的玩家状态：血量位、受伤冷却、受伤动画、是否死亡、着火刻数。
    pub fn state(&self, target: EntityUid) -> [i64; 5] {
        self.region
            .players
            .get(&PlayerId(target))
            .map_or([0; 5], |player| {
                [
                    i64::from(player.health.health.to_bits()),
                    i64::from(player.health.damage_cooldown),
                    i64::from(player.health.hurt_time),
                    i64::from(player.dead),
                    i64::from(player.environment.remaining_fire_ticks),
                ]
            })
    }

    /// 取走这名玩家这一刻收到的全部包，每项是「包 ID + 包体」。
    pub fn take_packets(&mut self, target: EntityUid) -> Vec<Vec<u8>> {
        self.region.links.flush();
        let Some((_, capture)) = self.captures.iter().find(|(id, _)| id.0 == target) else {
            return Vec::new();
        };
        let bytes = std::mem::take(
            &mut *capture
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        let mut rest = bytes.as_slice();
        let mut out = Vec::new();
        while let Ok(Some(bounds)) =
            mc_bamboo_protocol::frame::find_frame(rest, mc_bamboo_protocol::frame::MAX_FRAME_LEN)
        {
            let end = bounds.header_len + bounds.payload_len;
            out.push(rest[bounds.header_len..end].to_vec());
            rest = &rest[end..];
        }
        out
    }
}
