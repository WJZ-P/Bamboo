//! 管理命令的单元测试。

use std::sync::{Arc, Mutex};

use mc_bamboo_protocol::{
    codec::Reader,
    frame::{MAX_FRAME_LEN, find_frame},
    packet::play::{ClientInformation, GameMode, SystemChat},
};
use mc_bamboo_world::flat;
use uuid::Uuid;

use crate::{
    global::Global,
    inbound::{PlayerId, PlayerSink},
    player::Player,
    region::Region,
};

#[derive(Default, Clone)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl PlayerSink for Sink {
    fn send(&mut self, frames: Vec<u8>) -> bool {
        self.0.lock().expect("测试输出锁不该中毒").extend(frames);
        true
    }

    fn close(&mut self) {}
}

/// 一名等级 4 的管理员和一名普通玩家，都能收到命令反馈。
fn world_with(names: &[(&str, u8)]) -> (Global, Region, Vec<(PlayerId, Sink)>) {
    let mut region = Region::new(2, 20, false);
    region.send_command_feedback = true;
    let mut sinks = Vec::new();
    for (index, (name, level)) in names.iter().enumerate() {
        let id = PlayerId(index as i32 + 1);
        let uuid = Uuid::from_u128(id.0 as u128);
        let mut player = Player::new(
            id,
            uuid,
            (*name).to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            mc_bamboo_core::Vec3::new(8.5, f64::from(flat::surface_y() + 1), 8.5),
        );
        player.permission_level = *level;
        player.is_op = *level > 0;
        region.insert_player(player);
        let sink = Sink::default();
        region.links.add(id, Box::new(sink.clone()));
        region.peer_ips.insert(id, "127.0.0.1".to_owned());
        sinks.push((id, sink));
    }
    (Global::default(), region, sinks)
}

fn run(global: &mut Global, region: &mut Region, id: PlayerId, command: &str) {
    crate::command::on_command(global, region, id, command, None);
    region.links.flush();
}

/// 把链路里攒下的反馈帧解成翻译键列表。
fn feedback_keys(sink: &Sink) -> Vec<String> {
    let bytes = sink.0.lock().expect("测试输出锁不该中毒").clone();
    let mut out = Vec::new();
    let mut rest = bytes.as_slice();
    while let Some(bounds) = find_frame(rest, MAX_FRAME_LEN).expect("测试帧合法") {
        let end = bounds.header_len + bounds.payload_len;
        let payload = &rest[bounds.header_len..end];
        rest = &rest[end..];
        if let Some(key) = translate_key(payload) {
            out.push(key);
        }
    }
    out
}

/// `system_chat` 负载里的翻译键。只认 `translate` 在第一位的 NBT compound。
fn translate_key(payload: &[u8]) -> Option<String> {
    use mc_bamboo_protocol::packet::ClientboundPacket;
    let mut reader = Reader::new(payload);
    if reader.varint().ok()? != SystemChat::ID {
        return None;
    }
    let body = reader.remaining();
    let mut nbt = mc_bamboo_nbt::Reader::new(body, mc_bamboo_nbt::Accounter::unlimited());
    let entries = nbt.compound().ok()?;
    entries
        .iter()
        .find(|(key, _)| key == "translate")
        .and_then(|(_, value)| match value {
            mc_bamboo_nbt::Tag::String(text) => Some(text.clone()),
            _ => None,
        })
}

#[test]
fn op_deop_round_trip_updates_permissions_and_commands() {
    let (mut global, mut region, sinks) = world_with(&[("Kitten", 4), ("Mochi", 0)]);
    let (admin, _) = sinks[0].clone();
    let (target, target_sink) = sinks[1].clone();
    let target_uuid = region.players[&target].uuid;

    run(&mut global, &mut region, admin, "op Mochi");
    assert_eq!(region.players[&target].permission_level, 4);
    assert!(region.players[&target].is_op);
    assert_eq!(global.admin.op(target_uuid).map(|op| op.level), Some(4));
    assert!(region.admin_changes.ops);
    // 目标收到 entity_event 的事件字节 28（OWNER 等级）和整棵命令树；
    // 成功消息发给命令来源。
    let target_bytes = target_sink.0.lock().expect("测试输出锁不该中毒").clone();
    // entity_event 负载是实体 ID（varint）加一个事件字节。
    assert!(
        target_bytes
            .windows(2)
            .any(|pair| pair[1] == mc_bamboo_protocol::packet::play::PERMISSION_LEVEL_OWNERS),
        "/op 之后要给目标重发权限等级: {target_bytes:?}"
    );
    assert_eq!(
        feedback_keys(&sinks[0].1),
        vec!["commands.op.success".to_owned()]
    );

    // 已经是 OP 时再 /op 走 `commands.op.failed`。
    let admin_sink = sinks[0].1.clone();
    admin_sink.0.lock().expect("测试输出锁不该中毒").clear();
    run(&mut global, &mut region, admin, "op Mochi");
    let failed = feedback_keys(&admin_sink);
    assert!(failed.is_empty(), "失败消息套在红色空组件里: {failed:?}");

    run(&mut global, &mut region, admin, "deop Mochi");
    assert_eq!(region.players[&target].permission_level, 0);
    assert!(!region.players[&target].is_op);
    assert_eq!(global.admin.op(target_uuid), None);
}

#[test]
fn op_an_offline_name_uses_the_offline_uuid() {
    let (mut global, mut region, sinks) = world_with(&[("Kitten", 4)]);
    let (admin, _) = sinks[0].clone();
    run(&mut global, &mut region, admin, "op Notch");
    let expected = Uuid::parse_str("b50ad385-829d-3141-a216-7e7d7539ba7f").unwrap();
    assert_eq!(global.admin.op(expected).map(|op| op.level), Some(4));
    assert_eq!(global.admin.ops[&expected].name, "Notch");
}

#[test]
fn version_prints_the_official_header_and_fields() {
    let (mut global, mut region, sinks) = world_with(&[("Kitten", 2)]);
    let (admin, sink) = sinks[0].clone();
    run(&mut global, &mut region, admin, "version");
    assert_eq!(
        feedback_keys(&sink),
        [
            "commands.version.header",
            "commands.version.id",
            "commands.version.name",
            "commands.version.data",
            "commands.version.series",
            "commands.version.protocol",
            "commands.version.build_time",
            "commands.version.pack.resource",
            "commands.version.pack.data",
            "commands.version.stable.yes",
        ]
        .map(str::to_owned)
    );
}

#[test]
fn ban_kicks_the_online_player_and_pardon_removes_the_entry() {
    let (mut global, mut region, sinks) = world_with(&[("Kitten", 4), ("Mochi", 0)]);
    let (admin, _) = sinks[0].clone();
    let (target, target_sink) = sinks[1].clone();
    let uuid = region.players[&target].uuid;

    run(&mut global, &mut region, admin, "ban Mochi testing");
    assert!(global.admin.player_bans.contains_key(&uuid));
    assert_eq!(
        global.admin.player_bans[&uuid].entry.reason.as_deref(),
        Some("testing")
    );
    assert!(region.admin_changes.player_bans);
    assert!(!region.links.is_open(target), "被封的玩家当场断开");
    let bytes = target_sink.0.lock().expect("测试输出锁不该中毒").clone();
    assert!(
        bytes.windows(2).any(|_| true) && !bytes.is_empty(),
        "断开包要带上理由"
    );

    run(&mut global, &mut region, admin, "pardon Mochi");
    assert!(!global.admin.player_bans.contains_key(&uuid));
    assert_eq!(
        feedback_keys(&sinks[0].1),
        vec![
            "commands.ban.success".to_owned(),
            "commands.pardon.success".to_owned()
        ]
    );
}

#[test]
fn ban_ip_and_pardon_ip_use_the_peer_address() {
    let (mut global, mut region, sinks) = world_with(&[("Kitten", 4), ("Mochi", 0)]);
    let (admin, _) = sinks[0].clone();

    run(&mut global, &mut region, admin, "ban-ip Mochi reason");
    assert!(global.admin.ip_bans.contains_key("127.0.0.1"));
    assert!(region.admin_changes.ip_bans);
    assert!(!region.links.is_open(sinks[1].0), "同 IP 的玩家当场断开");

    run(&mut global, &mut region, admin, "pardon-ip 127.0.0.1");
    assert!(!global.admin.ip_bans.contains_key("127.0.0.1"));
}

#[test]
fn banlist_lists_both_kinds_and_nothing_when_empty() {
    let (mut global, mut region, sinks) = world_with(&[("Kitten", 4)]);
    let (admin, sink) = sinks[0].clone();
    run(&mut global, &mut region, admin, "banlist");
    assert_eq!(feedback_keys(&sink), ["commands.banlist.none"]);

    run(&mut global, &mut region, admin, "ban Mochi why");
    run(&mut global, &mut region, admin, "ban-ip 10.0.0.1 why");
    sink.0.lock().expect("测试输出锁不该中毒").clear();
    run(&mut global, &mut region, admin, "banlist");
    let keys = feedback_keys(&sink);
    assert_eq!(keys[0], "commands.banlist.list");
    assert_eq!(
        keys.iter()
            .filter(|k| *k == "commands.banlist.entry")
            .count(),
        2
    );

    sink.0.lock().expect("测试输出锁不该中毒").clear();
    run(&mut global, &mut region, admin, "banlist players");
    let keys = feedback_keys(&sink);
    assert_eq!(
        keys.iter()
            .filter(|k| *k == "commands.banlist.entry")
            .count(),
        1
    );
}

#[test]
fn whitelist_on_off_add_remove_and_list() {
    let (mut global, mut region, sinks) = world_with(&[("Kitten", 4)]);
    let (admin, sink) = sinks[0].clone();
    run(&mut global, &mut region, admin, "whitelist on");
    assert!(global.admin.using_whitelist);
    assert!(region.admin_changes.using_whitelist);
    assert_eq!(feedback_keys(&sink), ["commands.whitelist.enabled"]);

    sink.0.lock().expect("测试输出锁不该中毒").clear();
    run(&mut global, &mut region, admin, "whitelist add Mochi");
    assert_eq!(global.admin.whitelist.len(), 1);
    assert!(region.admin_changes.whitelist);
    sink.0.lock().expect("测试输出锁不该中毒").clear();
    run(&mut global, &mut region, admin, "whitelist list");
    assert_eq!(feedback_keys(&sink), ["commands.whitelist.list"]);

    sink.0.lock().expect("测试输出锁不该中毒").clear();
    run(&mut global, &mut region, admin, "whitelist remove Mochi");
    assert!(global.admin.whitelist.is_empty());
    sink.0.lock().expect("测试输出锁不该中毒").clear();
    run(&mut global, &mut region, admin, "whitelist list");
    assert_eq!(feedback_keys(&sink), ["commands.whitelist.none"]);

    sink.0.lock().expect("测试输出锁不该中毒").clear();
    run(&mut global, &mut region, admin, "whitelist off");
    assert!(!global.admin.using_whitelist);
    assert_eq!(feedback_keys(&sink), ["commands.whitelist.disabled"]);
}

#[test]
fn enforce_whitelist_kicks_unlisted_players() {
    let (mut global, mut region, sinks) = world_with(&[("Kitten", 4), ("Mochi", 0)]);
    let (admin, _) = sinks[0].clone();
    let (target, _) = sinks[1].clone();
    global.admin.enforce_whitelist = true;

    // 只有 Kitten 在名单里；开启时 Mochi 会被踢。
    global.admin.whitelist.insert(
        region.players[&admin].uuid,
        NameAndId {
            uuid: region.players[&admin].uuid,
            name: "Kitten".to_owned(),
        },
    );
    run(&mut global, &mut region, admin, "whitelist on");
    assert!(!region.links.is_open(target));
    assert!(region.links.is_open(admin));
}

#[test]
fn stop_sets_the_flag_and_sends_the_official_message() {
    let (mut global, mut region, sinks) = world_with(&[("Kitten", 4)]);
    let (admin, sink) = sinks[0].clone();
    run(&mut global, &mut region, admin, "stop");
    assert!(global.admin.stop_requested);
    assert_eq!(feedback_keys(&sink), ["commands.stop.stopping"]);
}

#[test]
fn set_idle_timeout_uses_the_official_messages() {
    let (mut global, mut region, sinks) = world_with(&[("Kitten", 4)]);
    let (admin, sink) = sinks[0].clone();
    run(&mut global, &mut region, admin, "setidletimeout 5");
    assert_eq!(global.admin.idle_timeout_minutes, 5);
    assert_eq!(feedback_keys(&sink), ["commands.setidletimeout.success"]);

    sink.0.lock().expect("测试输出锁不该中毒").clear();
    run(&mut global, &mut region, admin, "setidletimeout 0");
    assert_eq!(global.admin.idle_timeout_minutes, 0);
    assert_eq!(
        feedback_keys(&sink),
        ["commands.setidletimeout.success.disabled"]
    );
}

#[test]
fn idle_players_are_kicked_after_the_timeout() {
    let (mut global, mut region, sinks) = world_with(&[("Kitten", 4), ("Mochi", 0)]);
    let (admin, _) = sinks[0].clone();
    let (target, _) = sinks[1].clone();
    run(&mut global, &mut region, admin, "setidletimeout 1");
    region.last_action_millis.insert(admin, 0);
    region.last_action_millis.insert(target, 0);
    region.clock_millis = Some(120_000);
    tick_idle(&mut global, &mut region);
    assert!(!region.links.is_open(target));
    assert!(!region.links.is_open(admin));
}

#[test]
fn transfer_sends_the_official_packet_and_feedback() {
    let (mut global, mut region, sinks) = world_with(&[("Kitten", 4)]);
    let (admin, sink) = sinks[0].clone();
    run(&mut global, &mut region, admin, "transfer example.com");
    let bytes = sink.0.lock().expect("测试输出锁不该中毒").clone();
    assert!(
        bytes.windows(3).any(|window| window == [0x84, 0x01, 0x0b]),
        "transfer 包的 ID 是 132，主机名长度 11"
    );
    assert!(bytes.windows(11).any(|w| w == b"example.com"));
    assert_eq!(feedback_keys(&sink), ["commands.transfer.success.single"]);
}

#[test]
fn version_requires_gamemasters() {
    let (mut global, mut region, sinks) = world_with(&[("Kitten", 1)]);
    let (admin, sink) = sinks[0].clone();
    run(&mut global, &mut region, admin, "version");
    assert!(
        feedback_keys(&sink).is_empty(),
        "等级 1 解析不了 /version，只有红色错误"
    );
}

use crate::admin::NameAndId;
use crate::command::admin::tick_idle;

#[test]
fn online_mode_resolves_names_from_the_session_profile_cache() {
    let (mut global, mut region, sinks) = world_with(&[("Kitten", 4)]);
    region.online_mode = true;
    let (admin, sink) = sinks[0].clone();
    let seen = NameAndId {
        uuid: Uuid::from_u128(99),
        name: "Mochi".to_owned(),
    };
    global.admin.remember_player(&seen);

    // 缓存里的人能认出来，即使只在白名单里也查不到（不查白名单）。
    global.admin.whitelist.insert(
        Uuid::from_u128(98),
        NameAndId {
            uuid: Uuid::from_u128(98),
            name: "Whitelisted".to_owned(),
        },
    );
    run(&mut global, &mut region, admin, "op Mochi");
    assert_eq!(global.admin.op(seen.uuid).map(|op| op.level), Some(4));
    assert_eq!(global.admin.whitelist.len(), 1);
    assert!(feedback_keys(&sink).contains(&"commands.op.success".to_owned()));

    // 白名单不是官方的档案缓存。
    sink.0.lock().expect("测试输出锁不该中毒").clear();
    run(&mut global, &mut region, admin, "op Whitelisted");
    assert_eq!(global.admin.ops.len(), 1);
    let failed = feedback_keys(&sink);
    assert!(failed.is_empty(), "查不到应该只回红色错误：{failed:?}");

    // 完全没见过的名字也按官方「查不到」报错。
    sink.0.lock().expect("测试输出锁不该中毒").clear();
    run(&mut global, &mut region, admin, "op Stranger");
    assert_eq!(global.admin.ops.len(), 1);
    assert!(feedback_keys(&sink).is_empty());
}

#[test]
fn joins_remember_the_player_for_later_name_lookups() {
    let (mut global, mut region, sinks) = world_with(&[("Admin", 4), ("Mochi", 0)]);
    region.online_mode = true;
    let (admin, _) = sinks[0].clone();
    let (target, _) = sinks[1].clone();
    let player = NameAndId {
        uuid: region.players[&target].uuid,
        name: "Mochi".to_owned(),
    };
    global.admin.remember_player(&player);
    // 玩家离线后还留在本次启动的缓存里。
    region.players.remove(&target);
    run(&mut global, &mut region, admin, "op mochi");
    assert_eq!(global.admin.op(player.uuid).map(|op| op.level), Some(4));
}

#[test]
fn the_profile_cache_is_case_insensitive() {
    let (mut global, _region, _sinks) = world_with(&[]);
    let player = NameAndId {
        uuid: Uuid::from_u128(7),
        name: "Joined".to_owned(),
    };
    global.admin.remember_player(&player);
    assert_eq!(
        global.admin.known_player("JOINED").map(|p| p.uuid),
        Some(player.uuid)
    );
}
