//! 玩家在世界里的状态。
//!
//! 每个领域的状态放在各自模块的类型里，这里只把它们组合起来。

use mc_bamboo_blocks::entity::{JavaRandom, ctx::PlayerView};
use mc_bamboo_core::{Aabb, ChunkPos, Hand, Vec3};
use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode, MODEL_PART_HAT};
use uuid::Uuid;

use crate::{
    chat::SpamThrottlers,
    chunk_tracking::{ChunkTracker, MIN_VIEW_DISTANCE},
    container::EnderChest,
    entity_sync::EntitySync,
    experience::Experience,
    inbound::PlayerId,
    interaction::{BlockPrediction, mining::Mining},
    inventory::Inventory,
    menu::station::InventoryCursor,
    movement::Teleport,
    player_grid::PlayerGrid,
    player_move::Motion,
    secure_chat::PlayerChat,
    world_state::RespawnData,
};

const STANDING_EYE_HEIGHT: f64 = 1.62;
const CROUCHING_EYE_HEIGHT: f64 = 1.27;
const STANDING_HEIGHT: f64 = 1.8;
const CROUCHING_HEIGHT: f64 = 1.5;
/// 碰撞盒宽度的一半。
const HALF_WIDTH: f64 = 0.3;

/// 世界里的一名玩家。
pub(crate) struct Player {
    pub(crate) id: PlayerId,
    pub(crate) uuid: Uuid,
    pub(crate) name: String,
    pub(crate) game_mode: GameMode,
    /// 命令权限等级，0 到 4，来自 `ops.json`。
    ///
    /// Command permission level, 0 through 4, from `ops.json`.
    pub(crate) permission_level: u8,
    /// 是否列在 `ops.json` 里。和权限等级分开，等级 0 的 OP 也算 OP。
    ///
    /// Whether the player is listed in `ops.json`. Separate from the level: a level-0 op still counts.
    pub(crate) is_op: bool,
    /// 实体位置，也就是脚底。只能经 [`Player::set_position`] 改，见 [`PlayerGrid`]。
    ///
    /// Entity position, the feet. Changed only through [`Player::set_position`]; see [`PlayerGrid`].
    position: Vec3,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    /// 每 tick 由派生状态的阶段写成 `yaw`。
    pub(crate) head_yaw: f32,
    pub(crate) on_ground: bool,
    /// 移动校验用的服务端状态，对应 `ServerGamePacketListenerImpl` 里跟位置有关的字段。
    ///
    /// Server-side movement state, the position fields of `ServerGamePacketListenerImpl`.
    pub(crate) motion: Motion,
    /// 最近一次收到的客户端设置。
    pub(crate) client: ClientInformation,
    pub(crate) teleport: Teleport,
    pub(crate) chunks: ChunkTracker,
    pub(crate) sync: EntitySync,
    pub(crate) prediction: BlockPrediction,
    /// 正在挖哪个方块、挖了多久，官方 `ServerPlayerGameMode` 的那几个字段。
    pub(crate) mining: Mining,
    pub(crate) inventory: Inventory,
    /// 没有方块菜单时，玩家物品栏里的 2×2 合成。光标和远程槽都在这里。
    ///
    /// The 2×2 crafting grid while no block menu is open. The cursor and remote slots live here.
    pub(crate) inventory_cursor: InventoryCursor,
    /// 官方 `containerCounter`，每打开一次菜单加一，100 回绕到 1。
    pub(crate) container_counter: i32,
    /// 这名玩家自己的末影箱，每个玩家一份。
    pub(crate) ender_chest: EnderChest,
    pub(crate) spam: SpamThrottlers,
    /// 丢出物品、铁砧损坏和附魔换种子用的 `this.random`。种子由玩家 ID 决定。
    ///
    /// `this.random` for thrown items, anvil damage and enchantment seeds.
    pub(crate) random: JavaRandom,
    /// 官方 `Player.enchantmentSeed`，进服时从 `this.random` 取。
    ///
    /// Vanilla `Player.enchantmentSeed`, drawn from `this.random` on join.
    pub(crate) enchantment_seed: i32,
    /// 官方 `Player` 的经验等级、进度和总经验。
    ///
    /// Vanilla `Player`'s experience level, progress and total.
    pub(crate) experience: Experience,
    /// 官方 `Player.takeXpDelay`，每刻减 1，为 0 才能碰到经验球。
    ///
    /// Vanilla `Player.takeXpDelay`: decremented each tick; an orb is taken only at zero.
    pub(crate) take_xp_delay: i32,
    /// 官方 `Player.DATA_SCORE_ID`（存档里的 `Score`），`giveExperiencePoints` 也加它。
    ///
    /// Vanilla `Player.DATA_SCORE_ID` (the saved `Score`); `giveExperiencePoints` adds to it too.
    pub(crate) score: i32,
    /// 官方 `ServerPlayer.lastSentExp`，构造时是 `-99999999`；总经验变了才发包。
    ///
    /// Vanilla `ServerPlayer.lastSentExp`, `-99999999` on construction. A packet
    /// goes out only when the total changes.
    pub(crate) last_sent_exp: i32,
    /// 创造模式把物品拖出界面的 `TickThrottler(20, 1480)`。
    ///
    /// Creative drags out of the screen use `TickThrottler(20, 1480)`.
    pub(crate) drop_spam: i32,
    /// 公钥、消息链和已读窗口。没有会话时按无签名处理。
    ///
    /// Public key, message chain, and the seen-message window. Without a session, chat stays unsigned.
    pub(crate) chat: PlayerChat,
    /// 官方 `ServerPlayer.respawnConfig`。没有死亡重生，只存着。
    ///
    /// Official `ServerPlayer.respawnConfig`. Stored only; there is no death respawn.
    pub(crate) respawn: Option<RespawnData>,
    /// 这名玩家自己的入站队列（岛的实施计划第 6 节第 4 步）。世界线程每刻取一次，
    /// 和主控队列、别的玩家的队列按序号归并；跟着玩家走。
    ///
    /// This player's own inbound queue (island plan, section 6, step 4). The world
    /// drains it each tick and merges it with the master queue and the other
    /// players' queues by number; it moves with the player.
    pub(crate) inbox: crate::inbound::PlayerInbox,
    /// 官方 `ServerPlayer` 的六个 `lastRecorded*` 字段（计分板的只读准则），顺序见
    /// [`tick_criteria`](crate::scoreboard::tick_criteria)。新玩家是构造函数的初值。
    ///
    /// The six official `ServerPlayer.lastRecorded*` fields (the scoreboard's
    /// read-only criteria), ordered as in
    /// [`tick_criteria`](crate::scoreboard::tick_criteria). A new player starts at
    /// the constructor defaults.
    pub(crate) recorded_criteria: [i32; 6],
}

impl Player {
    pub(crate) fn new(
        id: PlayerId,
        uuid: Uuid,
        name: String,
        game_mode: GameMode,
        client: ClientInformation,
        position: Vec3,
    ) -> Self {
        let random = crate::drops::player_random(id);
        // 官方 `Player` 构造函数把 `enchantmentSeed` 设为 0；只有读档时
        // `XpSeed == 0` 才从 `this.random` 抽一次（见 player_save 的恢复函数）。
        //
        // Official `Player`'s constructor leaves `enchantmentSeed` at 0; only
        // a load with `XpSeed == 0` draws once from `this.random`
        // (see [`restore_experience_seed`]).
        let enchantment_seed = 0;
        Self {
            id,
            uuid,
            name,
            game_mode,
            permission_level: 0,
            is_op: false,
            position,
            yaw: 0.0,
            pitch: 0.0,
            head_yaw: 0.0,
            on_ground: false,
            motion: Motion::new(position),
            sync: EntitySync::new(position, &client),
            client,
            teleport: Teleport::default(),
            chunks: ChunkTracker::default(),
            prediction: BlockPrediction::default(),
            mining: Mining::default(),
            inventory: Inventory::default(),
            inventory_cursor: InventoryCursor::new(),
            container_counter: 0,
            ender_chest: EnderChest::default(),
            spam: SpamThrottlers::default(),
            random,
            enchantment_seed,
            experience: Experience::default(),
            take_xp_delay: 0,
            score: 0,
            last_sent_exp: -99_999_999,
            drop_spam: 0,
            chat: PlayerChat::default(),
            respawn: None,
            inbox: crate::inbound::PlayerInbox::default(),
            recorded_criteria: crate::scoreboard::UNRECORDED_CRITERIA,
        }
    }

    /// 实体位置，也就是脚底。
    ///
    /// Entity position, the feet.
    pub(crate) fn position(&self) -> Vec3 {
        self.position
    }

    /// 官方 `ServerPlayer.giveExperiencePoints`（0 是空操作，改动强制下一刻发包）。
    ///
    /// Official `ServerPlayer.giveExperiencePoints`: zero is a no-op; changes
    /// force a packet on the next tick.
    pub(crate) fn give_experience_points(&mut self, points: i32) {
        if points == 0 {
            return;
        }
        self.experience.give_points(points, &mut self.score);
        self.last_sent_exp = -1;
    }

    /// 官方 `ServerPlayer.giveExperienceLevels`。
    pub(crate) fn give_experience_levels(&mut self, levels: i32) {
        if levels == 0 {
            return;
        }
        self.experience.give_levels(levels);
        self.last_sent_exp = -1;
    }

    /// 官方 `ServerPlayer.setExperiencePoints`。
    pub(crate) fn set_experience_points(&mut self, points: i32) -> bool {
        let changed = self.experience.set_points(points);
        if changed {
            self.last_sent_exp = -1;
        }
        changed
    }

    /// 官方 `ServerPlayer.setExperienceLevels`。
    pub(crate) fn set_experience_levels(&mut self, level: i32) {
        if level == self.experience.level {
            return;
        }
        self.experience.set_level(level);
        self.last_sent_exp = -1;
    }

    /// 官方 `Player.onEnchantmentPerformed`：扣等级并从玩家随机源换新种子。
    ///
    /// Official `Player.onEnchantmentPerformed`: subtracts levels and draws
    /// a new seed from the player's random source.
    #[cfg_attr(not(feature = "oracle-samples"), allow(dead_code))]
    pub(crate) fn on_enchantment_performed(&mut self, levels: i32) {
        self.experience.level = self.experience.level.wrapping_sub(levels);
        if self.experience.level < 0 {
            self.experience.level = 0;
            self.experience.progress = 0.0;
            self.experience.total = 0;
        }
        self.enchantment_seed = self.random.next_int();
        self.last_sent_exp = -1;
    }

    /// 改位置，网格跟着更新。广播靠网格找附近的玩家，绕过这里改位置会让玩家漏收包。
    ///
    /// Moves the player and updates the grid. Broadcasts find nearby players through the grid,
    /// so changing the position any other way would make the player miss packets.
    pub(crate) fn set_position(&mut self, grid: &mut PlayerGrid, position: Vec3) {
        self.position = position;
        grid.place(self.id, position);
    }

    /// 玩家所在的区块。
    pub(crate) fn chunk_pos(&self) -> ChunkPos {
        ChunkPos::containing(self.position.x, self.position.z)
    }

    /// 这名玩家实际的视距：客户端请求的值，但不超过服务端视距，这样区块不会落到客户端范围外。
    pub(crate) fn view_distance(&self, server_view_distance: i32) -> i32 {
        i32::from(self.client.view_distance).clamp(MIN_VIEW_DISTANCE, server_view_distance)
    }

    pub(crate) fn shows_hat(&self) -> bool {
        shows_hat(&self.client)
    }

    /// 眼睛离脚底多高。
    pub(crate) fn eye_height(&self) -> f64 {
        if self.sync.is_crouching() {
            CROUCHING_EYE_HEIGHT
        } else {
            STANDING_EYE_HEIGHT
        }
    }

    /// 实体逻辑看到的玩家视图，对应官方 `Level.players()` 里的一个 `Player`。
    /// 这里不筛选旁观者或死亡状态：`getNearestPlayer` 的谓词、
    /// `TargetingConditions` 和 `ExperienceOrb.followNearbyPlayer` 各自按官方条件过滤。
    ///
    /// The player view seen by entity logic, one entry of `Level.players()`.
    /// Spectators and dead players are not filtered here: `getNearestPlayer`'s
    /// predicate, `TargetingConditions` and `ExperienceOrb.followNearbyPlayer`
    /// each apply the official conditions themselves.
    pub(crate) fn view(&self, id: PlayerId) -> PlayerView {
        let main = self.inventory.held(Hand::Main);
        let off = self.inventory.held(Hand::Off);
        PlayerView {
            uid: id.0,
            position: self.position,
            eye_height: self.eye_height() as f32,
            // Bamboo 还没有玩家生命和药水效果。
            // Bamboo has no player health or potion effects yet.
            alive: true,
            spectator: self.game_mode == GameMode::Spectator,
            creative: self.game_mode == GameMode::Creative,
            sneaking: self.sync.is_crouching(),
            invisible: false,
            armor_cover: 0.0,
            main_hand: if main.is_empty() { 0 } else { main.item },
            off_hand: if off.is_empty() { 0 } else { off.item },
        }
    }

    /// 碰撞盒，世界坐标。
    pub(crate) fn bounding_box(&self) -> Aabb {
        let height = if self.sync.is_crouching() {
            CROUCHING_HEIGHT
        } else {
            STANDING_HEIGHT
        };
        let Vec3 { x, y, z } = self.position;
        Aabb::new(
            x - HALF_WIDTH,
            y,
            z - HALF_WIDTH,
            x + HALF_WIDTH,
            y + height,
            z + HALF_WIDTH,
        )
    }
}

/// 按 `PlayerId` 升序列出本岛的玩家，和官方 `ServerLevel.players()` 的进服顺序一致：
/// 实体 ID 单调递增，进服时追加到表尾，离开时删掉。
///
/// Lists this island's players by ascending `PlayerId`, matching the join order
/// of `ServerLevel.players()`: entity ids grow monotonically, joins append and
/// leaves remove.
pub(crate) fn player_views(
    players: &std::collections::BTreeMap<PlayerId, Player>,
) -> Vec<PlayerView> {
    players
        .iter()
        .map(|(id, player)| player.view(*id))
        .collect()
}

/// 客户端设置里帽子层是不是打开的。
pub(crate) fn shows_hat(client: &ClientInformation) -> bool {
    client.model_customisation & MODEL_PART_HAT != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player_requesting(view_distance: i8) -> Player {
        let client = ClientInformation {
            view_distance,
            ..ClientInformation::default()
        };
        Player::new(
            PlayerId(1),
            Uuid::nil(),
            "Kitten".to_owned(),
            GameMode::Creative,
            client,
            Vec3::default(),
        )
    }

    #[test]
    fn requested_view_distance_stays_between_two_and_the_server_value() {
        assert_eq!(player_requesting(12).view_distance(8), 8);
        assert_eq!(player_requesting(5).view_distance(8), 5);
        assert_eq!(player_requesting(2).view_distance(8), 2);
        assert_eq!(player_requesting(1).view_distance(8), 2);
        assert_eq!(player_requesting(0).view_distance(8), 2);
        assert_eq!(player_requesting(-5).view_distance(8), 2);
        assert_eq!(player_requesting(i8::MAX).view_distance(32), 32);
        assert_eq!(player_requesting(i8::MIN).view_distance(2), 2);
    }

    #[test]
    fn chunk_position_rounds_down() {
        let mut player = player_requesting(2);

        let mut grid = PlayerGrid::default();
        player.set_position(&mut grid, Vec3::new(-0.5, 500.0, 16.0));
        assert_eq!(player.chunk_pos(), ChunkPos::new(-1, 1));

        player.set_position(&mut grid, Vec3::new(3.0e7, 0.0, -3.0e7));
        assert_eq!(player.chunk_pos(), ChunkPos::new(1_875_000, -1_875_000));
    }

    #[test]
    fn hat_follows_the_model_customisation_bit() {
        let mut player = player_requesting(2);

        assert!(!player.shows_hat());
        player.client.model_customisation = 0x7f;
        assert!(player.shows_hat());
        player.client.model_customisation = 0x3f;
        assert!(!player.shows_hat());
    }
}
