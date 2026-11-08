//! 玩家在世界里的状态。
//!
//! 每个领域的状态放在各自模块的类型里，这里只把它们组合起来。

use mc_bamboo_core::{Aabb, ChunkPos, Vec3};
use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode, MODEL_PART_HAT};
use uuid::Uuid;

use crate::{
    chat::SpamThrottlers,
    chunk_tracking::{ChunkTracker, MIN_VIEW_DISTANCE},
    container::EnderChest,
    entity_sync::EntitySync,
    inbound::PlayerId,
    interaction::{BlockPrediction, mining::Mining},
    inventory::Inventory,
    movement::Teleport,
    player_grid::PlayerGrid,
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
    /// 实体位置，也就是脚底。只能经 [`Player::set_position`] 改，见 [`PlayerGrid`]。
    ///
    /// Entity position, the feet. Changed only through [`Player::set_position`]; see [`PlayerGrid`].
    position: Vec3,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    /// 每 tick 由派生状态的阶段写成 `yaw`。
    pub(crate) head_yaw: f32,
    pub(crate) on_ground: bool,
    /// 最近一次收到的客户端设置。
    pub(crate) client: ClientInformation,
    pub(crate) teleport: Teleport,
    pub(crate) chunks: ChunkTracker,
    pub(crate) sync: EntitySync,
    pub(crate) prediction: BlockPrediction,
    /// 正在挖哪个方块、挖了多久，官方 `ServerPlayerGameMode` 的那几个字段。
    pub(crate) mining: Mining,
    pub(crate) inventory: Inventory,
    /// 官方 `containerCounter`，每打开一次菜单加一，100 回绕到 1。
    pub(crate) container_counter: i32,
    /// 这名玩家自己的末影箱，每个玩家一份。
    pub(crate) ender_chest: EnderChest,
    pub(crate) spam: SpamThrottlers,
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
        Self {
            id,
            uuid,
            name,
            game_mode,
            position,
            yaw: 0.0,
            pitch: 0.0,
            head_yaw: 0.0,
            on_ground: false,
            sync: EntitySync::new(position, &client),
            client,
            teleport: Teleport::default(),
            chunks: ChunkTracker::default(),
            prediction: BlockPrediction::default(),
            mining: Mining::default(),
            inventory: Inventory::default(),
            container_counter: 0,
            ender_chest: EnderChest::default(),
            spam: SpamThrottlers::default(),
        }
    }

    /// 实体位置，也就是脚底。
    ///
    /// Entity position, the feet.
    pub(crate) fn position(&self) -> Vec3 {
        self.position
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
