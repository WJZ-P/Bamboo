//! 僵尸白天着火的端到端覆盖：真实服务端、真实世界时钟和天气，客户端只看线上的包。
//!
//! 每个用例召唤一只 `NoAI` 的僵尸（不走动、不追人，但官方 `Mob.aiStep` 照样跑
//! `burnUndead`），用 `/time set` 和 `/weather` 控制环境，从 `set_entity_data` 的
//! 共享标志位（`0x01` 着火）、`entity_event`、`remove_entities` 和 `set_equipment`
//! 判断它有没有着火、烧死、头盔掉耐久。
//!
//! End-to-end coverage of zombies burning in daylight on a real server with the real
//! clock and weather, judged only from the packets the client receives.

mod common;

use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use common::{
    TestServer,
    player::{Answers, Player, offline_uuid},
    wire::{Frame, SetEquipment, chat_command},
};
use mc_bamboo_data::{
    items,
    packets::play::clientbound,
    registries::{data_component_type, entity_type},
};
use mc_bamboo_protocol::{
    codec::Reader,
    components::{ComponentValue, PatchEntry},
};
use mc_bamboo_server::ServerConfig;
use tokio::time::sleep;
use uuid::Uuid;

/// 着火、扣血、死亡动画和移除要好几十刻，点火本身每刻只有约 4% 的几率；给足余量。
const BURN_DEADLINE: Duration = Duration::from_secs(40);
/// 不该着火的用例观察这么久（约 200 刻）。白天露天时 200 刻都不点火的几率约 3e-4。
const QUIET: Duration = Duration::from_secs(10);
/// `Entity.FLAG_ONFIRE`。
const ON_FIRE: u8 = 0x01;
/// `EntityEvent.DEATH`。
const DEATH_EVENT: u8 = 3;
/// 协议里 `EquipmentSlot.HEAD` 的 ordinal。
const HEAD: u8 = 5;

/// 临时目录里放一份 `ops.json`，让玩家可以用 `/summon` / `/time` / `/weather`。
struct Lists(PathBuf);

impl Lists {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("bamboo-zombie-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("临时目录应该能建");
        let op = offline_uuid("Keeper");
        fs::write(
            path.join("ops.json"),
            format!(r#"[{{"uuid":"{op}","name":"Keeper","level":4}}]"#),
        )
        .expect("名单文件应该能写");
        Self(path)
    }

    fn config(&self) -> ServerConfig {
        ServerConfig {
            view_distance: 2,
            list_directory: self.0.clone(),
            ..ServerConfig::default()
        }
    }
}

impl Drop for Lists {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// 进服、等区块，再按顺序执行 `setup` 里的命令。
async fn enter(server: &TestServer, setup: &[&str]) -> Player {
    let player = server.spawn_player("Keeper", 2, Answers::default()).await;
    player.wait_for_chunks((0, 0), 2).await;
    player.settle().await;
    for command in setup {
        player.send(&chat_command(command)).await;
    }
    player.settle().await;
    player
}

/// `add_entity` 里第一只僵尸的网络 ID。
fn zombie_id(frames: &[Frame]) -> Option<i32> {
    frames
        .iter()
        .filter(|frame| frame.id == clientbound::ADD_ENTITY)
        .find_map(|frame| {
            let mut reader = Reader::new(&frame.body);
            let id = reader.varint().ok()?;
            reader.uuid().ok()?;
            (reader.varint().ok()? == entity_type::ZOMBIE).then_some(id)
        })
}

/// `set_entity_data` 里这个实体的共享标志位（index 0），没有这一项时是 `None`。
///
/// 只认识共享标志位前面可能出现的几种序列化器；遇到别的就停（共享标志位总是排在
/// 最前面，`SynchedEntityData` 按 index 升序写脏项）。
fn shared_flags(frame: &Frame, entity: i32) -> Option<u8> {
    if frame.id != clientbound::SET_ENTITY_DATA {
        return None;
    }
    let mut reader = Reader::new(&frame.body);
    if reader.varint().ok()? != entity {
        return None;
    }
    let index = reader.u8().ok()?;
    if index != 0 {
        return None;
    }
    let serializer = reader.varint().ok()?;
    (serializer == 0).then(|| reader.u8().ok()).flatten()
}

fn is_death_event(frame: &Frame, entity: i32) -> bool {
    if frame.id != clientbound::ENTITY_EVENT {
        return false;
    }
    let mut reader = Reader::new(&frame.body);
    matches!(
        (reader.i32(), reader.u8()),
        (Ok(id), Ok(DEATH_EVENT)) if id == entity
    )
}

fn is_removed(frame: &Frame, entity: i32) -> bool {
    if frame.id != clientbound::REMOVE_ENTITIES {
        return false;
    }
    let mut reader = Reader::new(&frame.body);
    let Ok(count) = reader.varint() else {
        return false;
    };
    (0..count).any(|_| reader.varint().ok() == Some(entity))
}

/// 共享标志位里出现过着火位。
fn caught_fire(frames: &[Frame], entity: i32) -> bool {
    frames
        .iter()
        .filter_map(|frame| shared_flags(frame, entity))
        .any(|flags| flags & ON_FIRE != 0)
}

/// 头盔的 `damage` 组件出现过大于 0 的值。
fn helmet_damaged(frames: &[Frame], entity: i32) -> bool {
    frames
        .iter()
        .filter(|frame| frame.is::<SetEquipment>())
        .map(Frame::decode::<SetEquipment>)
        .filter(|packet| packet.entity == entity)
        .any(|packet| {
            matches!(
                packet.patch(HEAD).and_then(|patch| patch.entry(data_component_type::DAMAGE)),
                Some(PatchEntry::Set(ComponentValue::Damage(damage))) if *damage > 0
            )
        })
}

/// 反复看收到的包，直到 `check` 成立；超过 `deadline` 让测试失败。
async fn wait_for(
    player: &Player,
    what: &str,
    deadline: Duration,
    check: impl Fn(&[Frame]) -> bool,
) {
    let until = Instant::now() + deadline;
    loop {
        if check(&player.frames()) {
            return;
        }
        assert!(Instant::now() < until, "{what} 没有在 {deadline:?} 内发生");
        sleep(Duration::from_millis(100)).await;
    }
}

/// 召唤之后等到僵尸出现在客户端，返回它的网络 ID。
async fn summon(player: &Player, nbt: &str) -> i32 {
    player
        .send(&chat_command(&format!(
            "summon minecraft:zombie 4.5 -60 4.5 {{NoAI:1b{nbt}}}"
        )))
        .await;
    wait_for(player, "僵尸出现", Duration::from_secs(10), |frames| {
        zombie_id(frames).is_some()
    })
    .await;
    zombie_id(&player.frames()).expect("刚等到")
}

/// 白天、晴天、露天：僵尸着火、掉血，最后烧死并被移除。
#[tokio::test]
async fn a_zombie_in_open_daylight_burns_to_death() {
    let lists = Lists::new();
    let server = TestServer::start(lists.config()).await;
    let player = enter(&server, &["time set 1000", "weather clear"]).await;
    let zombie = summon(&player, ",Health:2f").await;
    wait_for(&player, "僵尸着火", BURN_DEADLINE, |frames| {
        caught_fire(frames, zombie)
    })
    .await;
    wait_for(&player, "僵尸烧死", BURN_DEADLINE, |frames| {
        frames.iter().any(|frame| is_death_event(frame, zombie))
    })
    .await;
    wait_for(&player, "僵尸被移除", BURN_DEADLINE, |frames| {
        frames.iter().any(|frame| is_removed(frame, zombie))
    })
    .await;
}

/// 观察一段时间，僵尸既没着火也没死。
async fn assert_stays_unburnt(player: &Player, zombie: i32, why: &str) {
    sleep(QUIET).await;
    player.settle().await;
    let frames = player.frames();
    assert!(!caught_fire(&frames, zombie), "{why}：僵尸不该着火");
    assert!(
        !frames.iter().any(|frame| is_death_event(frame, zombie)),
        "{why}：僵尸不该死"
    );
}

/// 午夜：天黑，`MONSTERS_BURN` 也是假，不着火。
#[tokio::test]
async fn a_zombie_at_night_does_not_burn() {
    let lists = Lists::new();
    let server = TestServer::start(lists.config()).await;
    let player = enter(&server, &["time set 18000", "weather clear"]).await;
    let zombie = summon(&player, "").await;
    assert_stays_unburnt(&player, zombie, "夜里").await;
}

/// 白天，但头顶有一块石头：看不见天空，不着火。
#[tokio::test]
async fn a_zombie_under_a_roof_does_not_burn() {
    let lists = Lists::new();
    let server = TestServer::start(lists.config()).await;
    let player = enter(
        &server,
        &[
            "time set 1000",
            "weather clear",
            "setblock 4 -57 4 minecraft:stone",
        ],
    )
    .await;
    let zombie = summon(&player, "").await;
    assert_stays_unburnt(&player, zombie, "屋顶下").await;
}

/// 白天下雨：`isInWaterOrRain`，不着火。
#[tokio::test]
async fn a_zombie_in_the_rain_does_not_burn() {
    let lists = Lists::new();
    let server = TestServer::start(lists.config()).await;
    let player = enter(&server, &["time set 1000", "weather rain"]).await;
    let zombie = summon(&player, "").await;
    assert_stays_unburnt(&player, zombie, "下雨").await;
}

/// 白天戴铁头盔：头盔替它挡太阳、掉耐久，僵尸本身不着火。
#[tokio::test]
async fn a_helmet_keeps_a_zombie_from_burning_and_wears_out() {
    let lists = Lists::new();
    let server = TestServer::start(lists.config()).await;
    let player = enter(&server, &["time set 1000", "weather clear"]).await;
    let zombie = summon(
        &player,
        r#",equipment:{head:{id:"minecraft:iron_helmet",count:1}}"#,
    )
    .await;
    // 配对时头盔就在 `set_equipment` 里。
    let paired = player
        .frames()
        .iter()
        .filter(|frame| frame.is::<SetEquipment>())
        .map(Frame::decode::<SetEquipment>)
        .any(|packet| {
            packet.entity == zombie
                && packet
                    .slot(HEAD)
                    .flatten()
                    .is_some_and(|stack| stack.item == items::IRON_HELMET)
        });
    assert!(paired, "配对包里应该有头盔");
    wait_for(&player, "头盔掉耐久", BURN_DEADLINE, |frames| {
        helmet_damaged(frames, zombie)
    })
    .await;
    assert!(
        !caught_fire(&player.frames(), zombie),
        "戴头盔的僵尸不该着火"
    );
}
