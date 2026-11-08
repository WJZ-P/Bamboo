//! 世界的一块区域：区块、里面的玩家，以及这些玩家的出站链路。
//!
//! 这里的方法拿不到 [`Global`](crate::global::Global)，全服的事情由
//! [`World`](crate::World) 串起来。现在只有一个 `Region`，将来有多个时，
//! 玩家和它的链路一起搬家。

use std::collections::BTreeMap;

use mc_bamboo_blocks::{BlockTicks, RedstoneMemory};
use mc_bamboo_core::Vec3;
use mc_bamboo_data::registries::{attribute, dimension_type, world_clock};
use mc_bamboo_protocol::packet::play::{
    AttributeModifier, AttributeOperation, AttributeSnapshot, EntityData, GameMode,
    LevelChunksLoadStart, Login, PlayerAbilities, SetEntityData, SetTime, UpdateAttributes,
};
use mc_bamboo_world::{flat, store::ChunkStore};

use crate::{
    chunk_encoding::encode_chunk,
    chunk_tracking,
    global::GameTime,
    inbound::{JoinRequest, PlayerId},
    interaction::{BlockChanges, level::RegionLevel},
    links::Links,
    movement,
    player::Player,
};

const DIMENSION: &str = "minecraft:overworld";
/// 出生点在 (0, 0) 区块的中心。
const SPAWN_X: f64 = 8.5;
const SPAWN_Z: f64 = 8.5;
/// 正午。世界时钟的流速是 0，所以时间一直停在这里。
const NOON_TICKS: i64 = 6000;

const ABILITIES: PlayerAbilities = PlayerAbilities {
    invulnerable: true,
    flying: false,
    can_fly: true,
    instant_build: true,
    flying_speed: 0.05,
    walking_speed: 0.1,
};

/// 创造模式的触及距离加成完全由服务端下发，客户端代码里没有这两个数。
const CREATIVE_ATTRIBUTES: [AttributeSnapshot<'static>; 2] = [
    AttributeSnapshot {
        attribute: attribute::BLOCK_INTERACTION_RANGE,
        base: 4.5,
        modifiers: &[AttributeModifier {
            id: "minecraft:creative_mode_block_range",
            amount: 0.5,
            operation: AttributeOperation::AddValue,
        }],
    },
    AttributeSnapshot {
        attribute: attribute::ENTITY_INTERACTION_RANGE,
        base: 3.0,
        modifiers: &[AttributeModifier {
            id: "minecraft:creative_mode_entity_range",
            amount: 2.0,
            operation: AttributeOperation::AddValue,
        }],
    },
];

/// 调度用的时钟。和 [`GameTime`] 同步增长，但不是一回事，不能互相比较。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RegionTick(u64);

impl RegionTick {
    /// 从 `earlier` 到现在过了多少 tick。
    pub(crate) fn since(self, earlier: Self) -> u64 {
        self.0.saturating_sub(earlier.0)
    }
}

/// 世界的一块区域。`players` 和 `links` 是两个字段，遍历玩家时可以写链路。
pub(crate) struct Region {
    /// 已经进入世界的玩家。被踢的玩家留在这里，直到连接层的 `Leave` 到达。
    pub(crate) players: BTreeMap<PlayerId, Player>,
    pub(crate) links: Links,
    pub(crate) chunks: ChunkStore,
    /// 没改过的区块共用的编码结果。
    pub(crate) template_body: Vec<u8>,
    /// 这一 tick 改过的方块，等广播阶段发出去。
    pub(crate) block_changes: BlockChanges,
    /// 排队的计划刻。
    pub(crate) block_ticks: BlockTicks,
    /// 比较器的输出和红石火把最近的开关。
    redstone: RedstoneMemory,
    pub(crate) tick: RegionTick,
    /// 服务端视距，已经 clamp 到合法范围。
    pub(crate) view_distance: i32,
    max_players: i32,
}

// 世界线程以后要能把 `Region` 交给线程池，所以它必须能在线程之间移动。
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<Region>();
};

impl Region {
    pub(crate) fn new(view_distance: i32, max_players: i32) -> Self {
        let chunks = ChunkStore::flat();
        let template_body = encode_chunk(chunks.template());
        Self {
            players: BTreeMap::new(),
            links: Links::default(),
            chunks,
            template_body,
            block_changes: BlockChanges::default(),
            block_ticks: BlockTicks::new(),
            redstone: RedstoneMemory::default(),
            tick: RegionTick::default(),
            view_distance,
            max_players,
        }
    }

    pub(crate) fn advance_tick(&mut self) {
        self.tick.0 += 1;
    }

    /// 方块逻辑要的世界和计划刻队列。它们借用不同的字段，所以要一起拿。
    pub(crate) fn block_level(&mut self) -> (RegionLevel<'_>, &mut BlockTicks) {
        let level = RegionLevel {
            chunks: &mut self.chunks,
            changes: &mut self.block_changes,
            players: &self.players,
            redstone: &mut self.redstone,
        };
        (level, &mut self.block_ticks)
    }

    /// 进服第 1 到 3 步：`login`、`player_abilities`、`player_position`。
    pub(crate) fn add_player(&mut self, request: JoinRequest, game_mode: GameMode) {
        let JoinRequest {
            id,
            uuid,
            name,
            client,
            sink,
        } = request;
        self.links.add(id, sink);

        let login = Login {
            entity_id: id.0,
            dimensions: &[DIMENSION],
            max_players: self.max_players,
            view_distance: self.view_distance,
            simulation_distance: self.view_distance,
            dimension_type: dimension_type::OVERWORLD,
            dimension: DIMENSION,
            hashed_seed: 0,
            game_mode,
            is_flat: true,
            sea_level: flat::SEA_LEVEL,
            online_mode: false,
        };
        self.links.send(id, &login);
        self.links.send(id, &ABILITIES);

        let spawn = Vec3::new(SPAWN_X, f64::from(flat::surface_y()), SPAWN_Z);
        let mut player = Player::new(id, uuid, name, game_mode, client, spawn);
        movement::teleport(&mut player, &mut self.links, self.tick, spawn);
        self.players.insert(id, player);
    }

    /// 进服第 6 步：只有服务端会告诉客户端自己的皮肤外层、主手和触及距离。
    pub(crate) fn send_own_entity_data(&mut self, id: PlayerId) {
        let Some(player) = self.players.get(&id) else {
            return;
        };
        let items = [
            EntityData::MainHand(player.client.main_hand),
            EntityData::ModelCustomisation(player.client.model_customisation),
        ];
        self.links.send(
            id,
            &SetEntityData {
                entity_id: id.0,
                items: &items,
            },
        );
        self.links.send(
            id,
            &UpdateAttributes {
                entity_id: id.0,
                attributes: &CREATIVE_ATTRIBUTES,
            },
        );
    }

    /// 进服第 8 到 10 步：`set_time`、`game_event` 13、`set_chunk_cache_center`。
    /// 跟踪范围里的区块都进 `pending`，这一 tick 末尾发第一批。
    pub(crate) fn start_loading_chunks(&mut self, id: PlayerId, time: GameTime) {
        let time = SetTime {
            game_time: time.0,
            clock: world_clock::OVERWORLD,
            clock_ticks: NOON_TICKS,
            clock_rate: 0.0,
        };
        self.links.send(id, &time);
        // 客户端收不到它就永远停在加载画面。
        self.links.send(id, &LevelChunksLoadStart);
        if let Some(player) = self.players.get_mut(&id) {
            chunk_tracking::update_player_view(player, &mut self.links, self.view_distance);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_clock_measures_elapsed_ticks() {
        let mut region = Region::new(2, 20);
        let start = region.tick;

        for _ in 0..21 {
            region.advance_tick();
        }

        assert_eq!(region.tick.since(start), 21);
        assert_eq!(start.since(region.tick), 0);
    }

    #[test]
    fn creative_attributes_match_the_official_bytes() {
        let mut frames = Vec::new();
        mc_bamboo_protocol::packet::ClientboundPacket::write_body(
            &UpdateAttributes {
                entity_id: 1,
                attributes: &CREATIVE_ATTRIBUTES,
            },
            &mut frames,
        );

        let mut expected = vec![0x01, 0x02, 0x08];
        expected.extend_from_slice(&4.5f64.to_be_bytes());
        expected.extend_from_slice(&[0x01, 0x23]);
        expected.extend_from_slice(b"minecraft:creative_mode_block_range");
        expected.extend_from_slice(&0.5f64.to_be_bytes());
        expected.extend_from_slice(&[0x00, 0x0d]);
        expected.extend_from_slice(&3.0f64.to_be_bytes());
        expected.extend_from_slice(&[0x01, 0x24]);
        expected.extend_from_slice(b"minecraft:creative_mode_entity_range");
        expected.extend_from_slice(&2.0f64.to_be_bytes());
        expected.push(0x00);
        assert_eq!(expected.len(), 113);
        assert_eq!(frames, expected);
    }
}
