//! Play 状态：把玩家放进世界，并维持连接。
//!
//! 现在还没有世界循环，所以这里只做三件事：发送进入世界需要的包、发送区块、收发心跳。
//! 移动、方块交互和多人同步要等后面的阶段。

use std::time::Duration;

use mc_bamboo_data::registries::{dimension_type, world_clock};
use mc_bamboo_protocol::{
    frame::append_frame,
    packet::play::{
        AddPlayerInfo, ChunkBatchFinished, ChunkBatchStart, Disconnect, GameMode, KeepAlive,
        LevelChunkWithLight, LevelChunksLoadStart, Login, PlayRequest, PlayerAbilities,
        PlayerPosition, SetChunkCacheCenter, SetTime,
    },
};
use mc_bamboo_world::flat;
use tokio::time::{Instant, MissedTickBehavior, interval_at, timeout};
use tokio_util::sync::CancellationToken;

use super::{Connection, ConnectionError};
use crate::{players::PlayerSession, server::Shared};

const DIMENSION: &str = "minecraft:overworld";
const GAME_MODE: GameMode = GameMode::Creative;
const SPAWN_TELEPORT_ID: i32 = 1;
/// 出生点在 (0, 0) 区块的中心。
const SPAWN_X: f64 = 8.5;
const SPAWN_Z: f64 = 8.5;
const SPAWN_CHUNK: (i32, i32) = (0, 0);
/// 正午。现在没有世界循环，时间暂时静止。
const NOON_TICKS: i64 = 6000;

/// 一次写入多少个区块。官方客户端会根据批次调整接收速度。
const CHUNKS_PER_BATCH: usize = 64;
/// 发送全部区块的时限，防止不读数据的客户端一直占着连接。
const CHUNK_SEND_TIMEOUT: Duration = Duration::from_secs(60);
/// 关闭时等待客户端断开的时限，不能让服务端的关闭流程被一个客户端拖住。
const SHUTDOWN_LINGER: Duration = Duration::from_secs(1);

pub(super) async fn serve(
    mut connection: Connection,
    session: &PlayerSession,
    shared: &Shared,
    cancel: CancellationToken,
) -> Result<(), ConnectionError> {
    send_join_packets(&mut connection, session, shared).await?;

    tokio::select! {
        () = cancel.cancelled() => return disconnect(&mut connection, "服务器已关闭").await,
        sent = timeout(CHUNK_SEND_TIMEOUT, send_chunks(&mut connection, shared)) => match sent {
            Ok(result) => result?,
            Err(_) => return Err(ConnectionError::ChunkSendTimeout),
        },
    }

    let period = shared.config.keep_alive_interval;
    let mut keep_alive = interval_at(Instant::now() + period, period);
    keep_alive.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut pending_keep_alive = None;
    let mut next_keep_alive_id = 1;

    loop {
        tokio::select! {
            () = cancel.cancelled() => return disconnect(&mut connection, "服务器已关闭").await,
            _ = keep_alive.tick() => {
                if pending_keep_alive.is_some() {
                    disconnect(&mut connection, "连接超时").await?;
                    return Err(ConnectionError::KeepAliveTimeout);
                }
                let mut frame = Vec::new();
                append_frame(&mut frame, &KeepAlive { id: next_keep_alive_id })?;
                connection.write(&frame).await?;
                pending_keep_alive = Some(next_keep_alive_id);
                next_keep_alive_id += 1;
            }
            frame = connection.next_frame() => {
                let Some(payload) = frame? else {
                    return Ok(());
                };
                match PlayRequest::decode(&payload)? {
                    PlayRequest::KeepAlive(id) if pending_keep_alive == Some(id) => {
                        pending_keep_alive = None;
                    }
                    PlayRequest::KeepAlive(_) => return Err(ConnectionError::UnexpectedPacket),
                    // 传送确认和移动之类的包，要等世界循环做好才用得上。
                    PlayRequest::AcceptTeleportation(_) | PlayRequest::Other(_) => {}
                }
            }
        }
    }
}

/// 发送进入世界需要的所有包。顺序和官方的 `PlayerList.placeNewPlayer` 一致。
async fn send_join_packets(
    connection: &mut Connection,
    session: &PlayerSession,
    shared: &Shared,
) -> Result<(), ConnectionError> {
    let view_distance = i32::from(shared.config.view_distance);
    let mut frames = Vec::new();

    append_frame(
        &mut frames,
        &Login {
            entity_id: session.entity_id,
            dimensions: &[DIMENSION],
            max_players: shared.config.max_players as i32,
            view_distance,
            simulation_distance: view_distance,
            dimension_type: dimension_type::OVERWORLD,
            dimension: DIMENSION,
            hashed_seed: 0,
            game_mode: GAME_MODE,
            is_flat: true,
            sea_level: flat::SEA_LEVEL,
            online_mode: false,
        },
    )?;
    append_frame(
        &mut frames,
        &PlayerAbilities {
            invulnerable: true,
            flying: false,
            can_fly: true,
            instant_build: true,
            flying_speed: 0.05,
            walking_speed: 0.1,
        },
    )?;
    append_frame(
        &mut frames,
        &PlayerPosition {
            teleport_id: SPAWN_TELEPORT_ID,
            x: SPAWN_X,
            y: f64::from(flat::surface_y()),
            z: SPAWN_Z,
            yaw: 0.0,
            pitch: 0.0,
        },
    )?;
    append_frame(
        &mut frames,
        &AddPlayerInfo {
            uuid: session.uuid,
            name: &session.name,
            game_mode: GAME_MODE,
        },
    )?;
    append_frame(
        &mut frames,
        &SetTime {
            game_time: 0,
            clock: world_clock::OVERWORLD,
            clock_ticks: NOON_TICKS,
            clock_rate: 0.0,
        },
    )?;
    append_frame(&mut frames, &LevelChunksLoadStart)?;
    append_frame(
        &mut frames,
        &SetChunkCacheCenter {
            x: SPAWN_CHUNK.0,
            z: SPAWN_CHUNK.1,
        },
    )?;

    connection.write(&frames).await?;
    Ok(())
}

/// 从出生点向外发送视距内的所有区块，近的先发，玩家能尽快看到脚下的地面。
async fn send_chunks(connection: &mut Connection, shared: &Shared) -> Result<(), ConnectionError> {
    let positions = chunks_around(SPAWN_CHUNK, i32::from(shared.config.view_distance));
    let mut frames = Vec::new();

    for batch in positions.chunks(CHUNKS_PER_BATCH) {
        frames.clear();
        append_frame(&mut frames, &ChunkBatchStart)?;
        for &(x, z) in batch {
            append_frame(
                &mut frames,
                &LevelChunkWithLight {
                    x,
                    z,
                    body: &shared.flat_chunk_body,
                },
            )?;
        }
        append_frame(
            &mut frames,
            &ChunkBatchFinished {
                batch_size: batch.len() as i32,
            },
        )?;
        connection.write(&frames).await?;
    }
    Ok(())
}

/// 列出以 `center` 为中心、`radius` 圈以内的区块，按距离从近到远排序。
fn chunks_around(center: (i32, i32), radius: i32) -> Vec<(i32, i32)> {
    let mut positions: Vec<(i32, i32)> = (-radius..=radius)
        .flat_map(|dz| (-radius..=radius).map(move |dx| (dx, dz)))
        .collect();
    positions.sort_by_key(|&(dx, dz)| dx * dx + dz * dz);
    positions
        .into_iter()
        .map(|(dx, dz)| (center.0 + dx, center.1 + dz))
        .collect()
}

/// 告诉客户端断开的原因，再关闭连接。
async fn disconnect(connection: &mut Connection, reason: &str) -> Result<(), ConnectionError> {
    let mut frame = Vec::new();
    append_frame(&mut frame, &Disconnect { reason })?;
    connection.write(&frame).await?;
    // 超时说明客户端不配合，这时直接放弃就好。
    let _ = timeout(SHUTDOWN_LINGER, connection.finish()).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_every_chunk_in_range_nearest_first() {
        let positions = chunks_around((2, -3), 1);

        assert_eq!(positions.len(), 9);
        assert_eq!(positions[0], (2, -3));
        assert!(positions.contains(&(1, -4)));
        assert!(positions.contains(&(3, -2)));
        assert_eq!(
            positions
                .last()
                .map(|&(x, z)| (x - 2).abs() + (z + 3).abs()),
            Some(2)
        );
    }
}
