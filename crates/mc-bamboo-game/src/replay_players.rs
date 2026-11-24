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
use mc_bamboo_core::{Hand, Vec3};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{ClientInformation, GameMode},
};

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

    /// 把主人设成「已死亡」：血量归零（`LivingEntity.isAlive()` 为假、忠诚三叉戟
    /// 判定主人不可接受），并置 `ServerPlayer.dead`（`state()` 的死亡位、`sync_health`
    /// 跳过死人）。对拍里对应官方 `player.setHealth(0)`。
    ///
    /// Mark the owner dead: zero health (so `LivingEntity.isAlive()` is false and
    /// a loyal trident drops) plus `ServerPlayer.dead` (the `state()` dead bit and
    /// `sync_health` skipping dead players). Mirrors official `player.setHealth(0)`.
    pub fn set_dead(&mut self, target: EntityUid) {
        if let Some(player) = self.region.players.get_mut(&PlayerId(target)) {
            player.health.health = 0.0;
            player.health_sync = crate::player_hurt::HealthSync::of(player);
            player.dead = true;
        }
    }

    /// 把主人设成旁观者（`ServerPlayerGameMode.gameModeForPlayer`），忠诚三叉戟
    /// 判定主人不可接受；直接改字段以避开 `setGameMode` 发包。
    ///
    /// Make the owner a spectator so a loyal trident rejects them; set the field
    /// directly to avoid the packets `setGameMode` would send.
    pub fn set_spectator(&mut self, target: EntityUid) {
        if let Some(player) = self.region.players.get_mut(&PlayerId(target)) {
            player.game_mode = GameMode::Spectator;
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

    /// 注入下雨（激流 `releaseUsing` 的 `isInWaterOrRain()` 门槛）。
    ///
    /// Inject rain for riptide's `isInWaterOrRain()` release gate.
    pub fn set_raining(&mut self, raining: bool) {
        let level = if raining { 1.0 } else { 0.0 };
        self.region.global.weather.rain_level = level;
        self.region.global.weather.o_rain_level = level;
        self.region.global.weather.raining = raining;
        self.region.global.block_weather.rain_level = level;
    }

    /// 给主手装上物品并开始使用（`TridentItem.use` 的注册部分）。
    ///
    /// Equip the main hand and start using it (the registration half of
    /// `TridentItem.use`).
    pub fn start_use(&mut self, target: EntityUid, held: ItemStack) -> bool {
        let id = PlayerId(target);
        {
            let Some(player) = self.region.players.get_mut(&id) else {
                return false;
            };
            let slot = player.inventory.hand_slot(Hand::Main);
            player.inventory.slots[slot] = held;
        }
        crate::player_weapon::start(&mut self.region, id, Hand::Main)
    }

    /// 蓄力 `held` 刻后松手（`LivingEntity.useItemRemaining` 直接置成
    /// `72000 - held`），走正式的 `release` 入口。
    ///
    /// Release after `held` ticks (set `LivingEntity.useItemRemaining` to
    /// `72000 - held`) through the production `release` entry.
    pub fn release(&mut self, target: EntityUid, held: i32) {
        let id = PlayerId(target);
        if let Some(player) = self.region.players.get_mut(&id)
            && let Some(using) = player.weapon_use.as_mut()
        {
            using.remaining = 72_000 - held;
        }
        // 和正式服务端一样，先按装备重建属性表（手里三叉戟的攻击速度修饰符）。
        // Rebuild the attribute table from equipment first, like the server tick.
        crate::player_attributes::rebuild(&mut self.region);
        crate::player_weapon::release(&mut self.region, id);
    }

    /// 只跑一轮玩家间元数据同步（`startUsingItem` / 松手当刻的发包）。
    ///
    /// Run just one round of player metadata sync (the `startUsingItem` /
    /// release tick sends its packet on that same tick).
    pub fn sync_once(&mut self) {
        crate::entity_sync::update_derived_state(&mut self.region);
        crate::entity_sync::sync_entities(&mut self.region);
    }

    /// 用物品逐刻状态、自旋攻击和玩家间同步包的一轮官方 tick 顺序。
    ///
    /// One vanilla-tick slice for using an item, the auto-spin attack and
    /// player-to-player sync packets, in the official order.
    pub fn spin_tick(&mut self) {
        // 官方对拍一侧不跑 `ServerPlayer.doTick` 的 `Player.aiStep`，
        // `attackStrengthTicker` 恒为 0；这里也不推进它。
        // The official oracle side does not run `Player.aiStep`, so
        // `attackStrengthTicker` stays 0; do not advance it here either.
        // 正式服务端每刻先重建属性表再攻击（`world.rs` 的 `player_attributes::rebuild`），
        // 攻击冷却要读手里三叉戟的攻击速度修饰符。
        // The server rebuilds the attribute table before attacking each tick; the
        // attack cooldown reads the held trident's attack-speed modifier.
        crate::player_attributes::rebuild(&mut self.region);
        crate::player_auto_spin::tick(&mut self.region);
        crate::player_weapon::tick(&mut self.region);
        crate::entity_sync::update_derived_state(&mut self.region);
        crate::entity_sync::sync_entities(&mut self.region);
    }

    /// 客户端 `RequestStats` 之后 `ServerStatsCounter.sendStats` 发的累计值。
    ///
    /// The cumulative stats `ServerStatsCounter.sendStats` sends after a
    /// client `RequestStats`.
    pub fn request_stats(&mut self, target: EntityUid) {
        crate::chat::on_client_command(
            &mut self.region,
            PlayerId(target),
            mc_bamboo_protocol::packet::play::ClientCommandKind::RequestStats,
        );
    }

    /// 逐刻的 `ITEM_USED` 统计累计值（物品 ID 升序）。
    ///
    /// Per-tick cumulative `ITEM_USED` stat values, sorted by item id.
    pub fn item_used_stats(&self, target: EntityUid) -> Vec<(i32, i32)> {
        self.region
            .players
            .get(&PlayerId(target))
            .map_or_else(Vec::new, |player| player.item_use_stats.values())
    }

    /// 自旋攻击状态和生物标志位：`[剩余刻数, 生物标志位字节]`。
    ///
    /// Auto-spin state and living flags: `[ticks left, living flags byte]`.
    pub fn spin_state(&self, target: EntityUid) -> [i64; 2] {
        self.region
            .players
            .get(&PlayerId(target))
            .map_or([0; 2], |player| {
                // `EntitySync::Metadata::of`：使用物品位 0/1 加自旋攻击位 4。
                // `EntitySync::Metadata::of`: the using-item bits (0/1) plus
                // the auto-spin-attack bit (4).
                let using = player
                    .weapon_use
                    .as_ref()
                    .map_or(0, |active| 1 | (u8::from(active.hand == Hand::Off) << 1));
                [
                    i64::from(player.auto_spin_attack.ticks),
                    i64::from(using | crate::player_auto_spin::living_flag(player)),
                ]
            })
    }

    /// 把生物放到区域实体表里，让 `Player.attack` 能解析出自旋攻击的目标。
    ///
    /// Put a mob into the region's entity table so `Player.attack` can resolve
    /// it as the spin-attack target.
    pub fn add_spin_target(&mut self, entity: mc_bamboo_blocks::entity::Entity) -> i32 {
        self.region.entities.spawn(entity)
    }

    /// 区域实体表里第一只生物的 ID（对拍里就是自旋攻击的牛）。
    ///
    /// The first mob's id in the region entity table (the spin target here).
    pub fn first_mob_uid(&self) -> Option<EntityUid> {
        self.region.entities.ids().into_iter().next()
    }

    /// 自旋攻击目标此刻的血量位。
    ///
    /// The spin-attack target's health bits this tick.
    pub fn spin_target_health(&self, uid: EntityUid) -> Option<i64> {
        self.region
            .entities
            .public_at(uid)
            .map(|public| match &public.kind {
                mc_bamboo_blocks::entity::KindPublic::Mob(mob) => {
                    i64::from(mob.health.to_bits() as i32)
                }
                _ => 0,
            })
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

    /// 这一刻区域里产生的音效效果（自旋攻击命中 / 激流松手）。
    ///
    /// Sound effects produced in the region this tick (spin hit / riptide
    /// release).
    pub fn take_sound_effects(&mut self) -> Vec<mc_bamboo_blocks::Effect> {
        std::mem::take(&mut self.region.sound_effects)
            .into_iter()
            .collect()
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
