//! 世界生成端到端：区块变成可运行时做完工后处理，玩家收到的区块包里栅栏已经连好；
//! 处理后表清空，退出重进、重启服务端以后不再重复处理。
//!
//! 种子 42 的沼泽小屋在区块 (-17,22)。官方 `postProcessGeneration` 会把小屋的四根橡木栅栏
//! 连起来（`feature-merge` / `post-process` 对拍里列出的格子）。
//!
//! End-to-end world generation: post-processing runs when a chunk becomes
//! ticking, so the chunk packet already carries connected fences; the table is
//! then empty and a rejoin or restart does not process it again.

mod common;

use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use common::{
    TestServer,
    player::{Answers, Player},
    wire::{LevelChunk, PlayerPosition, Pos, SetChunkCacheCenter, SystemChat, chat_command},
};
use mc_bamboo_data::blocks;
use mc_bamboo_game::world_options::{LevelType, WorldGenConfig};
use mc_bamboo_server::ServerConfig;
use tokio::time::sleep;
use uuid::Uuid;

/// 沼泽小屋所在的区块。
const HUT_CHUNK: (i32, i32) = (-17, 22);
/// 小屋的一根栅栏：官方后处理后只连东边（`east=true`，其余 `false`）。
const HUT_FENCE: Pos = Pos::new(-271, 66, 353);
/// 普通世界生成很慢（调试构建、结构、3×3 地物），等区块的上限放宽。
const GENERATION_DEADLINE: Duration = Duration::from_secs(600);

/// 橡木栅栏的状态 ID：属性按 `east, north, south, waterlogged, west` 排，布尔值 `true`
/// 在前。缺省状态（全 `false`）是最后一个，只连东边的状态比它小 16。
/// Oak fence state ids: properties `east, north, south, waterlogged, west`, `true`
/// first. The default (all `false`) is the last one; east-only is 16 below it.
const FENCE_EAST_ONLY: u32 = blocks::OAK_FENCE - 16;

/// 每个测试一份临时目录：世界目录和名单文件（给测试玩家 op，好用 `/tp`、`/setblock`）。
struct Dir(PathBuf);

impl Dir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("bamboo-worldgen-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("临时目录应该能建");
        let uuid = common::player::offline_uuid("Surveyor");
        fs::write(
            path.join("ops.json"),
            format!(r#"[{{"uuid":"{uuid}","name":"Surveyor","level":4}}]"#),
        )
        .expect("名单文件应该能写");
        Self(path)
    }

    fn config(&self) -> ServerConfig {
        ServerConfig {
            view_distance: 2,
            list_directory: self.0.clone(),
            world_dir: Some(self.0.join("world")),
            world_gen: WorldGenConfig {
                seed: 42,
                level_type: LevelType::Normal,
                generate_structures: true,
                ..WorldGenConfig::default()
            },
            ..ServerConfig::default()
        }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// 进服、传送到小屋，等到小屋区块的区块包。
async fn visit_hut(server: &TestServer) -> (Player, LevelChunk) {
    let mut player = server.spawn_player("Surveyor", 2, Answers::default()).await;
    // 进服时等出生区块之前的传送会被出生落点盖掉，先等出生点那个区块到了再传送。
    // A teleport before the spawn chunks arrive is overridden by the spawn
    // placement, so wait for the spawn chunk first.
    let spawn = player.expect::<SetChunkCacheCenter>(|_| true).await;
    wait_for_chunk(&player, (spawn.x, spawn.z)).await;
    player.send(&chat_command("tp -267.5 70 355.5")).await;
    // 假客户端按自己记的位置发移动，要先改成落点，不然服务端会当成移动过快拉回去。
    // The fake client reports its stored position; update it to the landing spot
    // or the server treats the next move as too fast and pulls the player back.
    let landed = player
        .expect::<PlayerPosition>(|packet| packet.x < -200.0)
        .await;
    player.position = (landed.x, landed.y, landed.z);
    let chunk = wait_for_chunk(&player, HUT_CHUNK).await;
    (player, chunk)
}

/// 普通世界生成慢，按区块包逐个轮询，不用公共的 10 秒期限。
async fn wait_for_chunk(player: &Player, at: (i32, i32)) -> LevelChunk {
    let deadline = Instant::now() + GENERATION_DEADLINE;
    loop {
        if let Some(chunk) = player
            .frames()
            .iter()
            .rev()
            .filter(|frame| frame.is::<LevelChunk>())
            .map(|frame| frame.decode::<LevelChunk>())
            .find(|chunk| (chunk.x, chunk.z) == at)
        {
            return chunk;
        }
        if Instant::now() >= deadline {
            let frames = player.frames();
            let chunks: Vec<(i32, i32)> = frames
                .iter()
                .filter(|frame| frame.is::<LevelChunk>())
                .map(|frame| {
                    let chunk = frame.decode::<LevelChunk>();
                    (chunk.x, chunk.z)
                })
                .collect();
            let chat: Vec<String> = frames
                .iter()
                .filter(|frame| frame.is::<SystemChat>())
                .map(|frame| {
                    format!(
                        "{:?}",
                        frame.decode::<SystemChat>().content.translate_keys()
                    )
                })
                .collect();
            panic!(
                "never received chunk {at:?}; got {} frames, chunks {chunks:?}, chat {chat:?}",
                frames.len()
            );
        }
        sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn chunk_packets_carry_post_processed_fences_and_the_table_is_consumed_once() {
    let dir = Dir::new();

    // 1. 区块包里的栅栏已经连好：发包之前做完了后处理。
    let mut server = TestServer::start(dir.config()).await;
    let (mut player, chunk) = visit_hut(&server).await;
    assert_eq!(
        chunk.state_at(HUT_FENCE),
        FENCE_EAST_ONLY,
        "the hut fence must be connected before the chunk is sent"
    );

    // 2. 用 `strict` 把这根栅栏换成不连的缺省状态（`strict` 不做形状更新）。表要是
    // 没清空，下次变成可运行时它又会被连回去。
    player
        .send(&chat_command(
            "setblock -271 66 353 minecraft:oak_fence strict",
        ))
        .await;
    let reply = player
        .expect::<SystemChat>(|packet| {
            packet
                .content
                .translate_keys()
                .iter()
                .any(|key| key.starts_with("commands.setblock"))
        })
        .await;
    assert!(
        reply
            .content
            .translate_keys()
            .contains(&"commands.setblock.success"),
        "setblock failed: {:?}",
        reply.content.translate_keys()
    );
    // 方块变化可能和同段别的改动合成 `section_blocks_update`，按客户端视角回放来等。
    // The change may ride a `section_blocks_update`; wait on the replayed view.
    let deadline = Instant::now() + Duration::from_secs(30);
    while player.block_at(HUT_FENCE) != Some(blocks::OAK_FENCE) {
        assert!(
            Instant::now() < deadline,
            "setblock never reached the client"
        );
        sleep(Duration::from_millis(50)).await;
    }
    drop(player);

    // 3. 退出重进：区块不管还在内存里还是已经收回存盘，都不再处理。
    let (player, chunk) = visit_hut(&server).await;
    assert_eq!(chunk.state_at(HUT_FENCE), blocks::OAK_FENCE);
    drop(player);

    // 4. 重启服务端：存档里的表是空的，区块读回来也不再处理。
    server.shutdown_within(GENERATION_DEADLINE).await;
    let mut server = TestServer::start(dir.config()).await;
    let (player, chunk) = visit_hut(&server).await;
    assert_eq!(
        chunk.state_at(HUT_FENCE),
        blocks::OAK_FENCE,
        "a saved chunk must not be post-processed again"
    );
    drop(player);
    server.shutdown_within(GENERATION_DEADLINE).await;
}
