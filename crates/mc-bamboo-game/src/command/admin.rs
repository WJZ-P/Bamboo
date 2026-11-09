//! 服务器管理类命令：OP、封禁、白名单、关服、空闲超时、转移和版本。
//!
//! 命令只改世界线程 `Global` 上的权威名单。改动登记在 `Region.admin_changes`，
//! tick 末尾统一发布给连接层写盘；世界线程自己不碰 IO。
//!
//! Server administration commands: ops, bans, the whitelist, shutdown, idle timeout,
//! transfer and version. Commands only edit the authoritative lists on the world thread's
//! `Global`. Changes are recorded on `Region.admin_changes` and published for disk writes
//! at the end of the tick; the world thread never does IO.

use mc_bamboo_command::{
    EntitySelector, GameProfileInput, Message, ParsedCommand, Value, permission,
};
use mc_bamboo_protocol::{
    packet::play::PlayRequest,
    text::{Component, TranslateArg},
};
use uuid::{Builder, Uuid};

use crate::{
    admin::{AdminChanges, BanEntry, BanListEntry, LoginRefusal, NameAndId},
    command::{
        feedback::{self, CommandFail, fail},
        select,
    },
    global::Global,
    inbound::PlayerId,
    region::Region,
};

/// 官方 `op-permission-level` 的默认值，`PermissionLevel.OWNER`。
const OP_LEVEL: u8 = crate::admin::AdminState::OP_LEVEL;
/// 官方 `ClientboundTransferPacket` 的默认端口。
const DEFAULT_PORT: i32 = 25565;
/// 官方 `multiplayer.disconnect.idling`。
const IDLING: &str = "multiplayer.disconnect.idling";

/// 取走这一 tick 登记过的名单变更。
pub(crate) fn take_changes(region: &mut Region) -> AdminChanges {
    std::mem::take(&mut region.admin_changes)
}

/// `/whitelist reload` 读盘回来之后按官方 `kickUnlistedPlayers` 踢人。
pub(crate) fn after_reload(global: &mut Global, region: &mut Region) {
    kick_unlisted(global, region);
}

/// 官方 `PlayerList.canPlayerLogin` 产出的断开原因，世界线程复查时用。
pub(crate) fn refusal_message(refusal: &LoginRefusal) -> Component {
    use crate::admin::RefusalArg;
    let text = refusal.text();
    let args: Vec<TranslateArg> = text
        .args
        .iter()
        .map(|arg| match arg {
            RefusalArg::Text(value) => TranslateArg::from(value.clone()),
            RefusalArg::Translate(key) => Component::translate(*key, []).into(),
        })
        .collect();
    let mut line = Component::translate(text.key, args);
    if let Some((key, expires)) = &text.expiration {
        line = line.extra(Component::translate(
            *key,
            [TranslateArg::from(expires.clone())],
        ));
    }
    line
}

/// 官方 `ServerPlayer.resetLastActionTime` 对应的 18 个处理器。
/// Bamboo 已经解码并且能对应上的都在这里；其余见 README。
pub(crate) fn note_action(region: &mut Region, player: PlayerId, request: &PlayRequest) {
    let active = matches!(
        request,
        PlayRequest::Chat { .. }
            | PlayRequest::ChatCommand { .. }
            | PlayRequest::PlayerInput { .. }
            | PlayRequest::PlayerCommand { .. }
            | PlayRequest::Punch
            | PlayRequest::SetCarriedItem { .. }
            | PlayRequest::PlayerAction { .. }
            | PlayRequest::UseItemOn { .. }
            | PlayRequest::UseItem { .. }
            | PlayRequest::ContainerClick { .. }
            | PlayRequest::ContainerButtonClick { .. }
            | PlayRequest::SignUpdate { .. }
            | PlayRequest::ClientCommand { .. }
    ) || moved(region, player, request);
    if active && region.last_action_millis.contains_key(&player) {
        region
            .last_action_millis
            .insert(player, region.now_millis());
    }
}

/// 官方 `handlePlayerKnownMovement`：只有已知位移平方大于 1e-5 才重置。
fn moved(region: &Region, player: PlayerId, request: &PlayRequest) -> bool {
    let PlayRequest::MovePlayer {
        position: Some(position),
        ..
    } = request
    else {
        return false;
    };
    let Some(current) = region.players.get(&player) else {
        return false;
    };
    let now = current.position();
    let delta =
        mc_bamboo_core::Vec3::new(position.x - now.x, position.y - now.y, position.z - now.z);
    delta.length_sqr() > 9.999_999_747_378_752E-6_f64
}

/// 官方 `ServerGamePacketListenerImpl.tick`：超过设定分钟数才踢。
pub(crate) fn tick_idle(global: &mut Global, region: &mut Region) {
    let minutes = global.admin.idle_timeout_minutes;
    if minutes <= 0 {
        return;
    }
    let limit = i64::from(minutes).saturating_mul(60_000);
    let now = region.now_millis();
    let idle: Vec<PlayerId> = region
        .last_action_millis
        .iter()
        .filter(|(_, last)| now.saturating_sub(**last) > limit)
        .map(|(id, _)| *id)
        .collect();
    let reason = Component::translate(IDLING, []);
    for id in idle {
        region.links.kick(id, &reason);
        region.last_action_millis.remove(&id);
    }
}

/// 官方 `GameProfileArgument.getGameProfiles`：`@` 选在线玩家，名字解析成 `(UUID, 名字)`。
///
/// 在线模式没有 Mojang 查询，不在线的名字按官方「查不到」报
/// `argument.player.unknown`；离线模式用 `UUIDUtil.createOfflinePlayerUUID`。
fn game_profiles(
    global: &Global,
    region: &Region,
    source: PlayerId,
    parsed: &ParsedCommand,
    name: &str,
) -> Result<Vec<NameAndId>, CommandFail> {
    let input = parsed
        .get(name)
        .and_then(Value::as_game_profile)
        .ok_or_else(|| fail("command.failed"))?;
    match input {
        GameProfileInput::Selector(selector) => {
            let level = level_of(region, source);
            let ids = game_profile_players(region, source, level, selector)?;
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(player) = region.players.get(&id) {
                    out.push(NameAndId {
                        uuid: player.uuid,
                        name: player.name.clone(),
                    });
                }
            }
            Ok(out)
        }
        GameProfileInput::Name(name) => {
            if let Some(player) = region
                .players
                .values()
                .find(|player| player.name.eq_ignore_ascii_case(name))
            {
                return Ok(vec![NameAndId {
                    uuid: player.uuid,
                    name: player.name.clone(),
                }]);
            }
            // 名单里记着这个人的 UUID 时按官方的档案缓存直接认出来。
            if region.online_mode
                && let Some(entry) = global
                    .admin
                    .whitelist
                    .values()
                    .find(|entry| entry.name.eq_ignore_ascii_case(name))
            {
                return Ok(vec![entry.clone()]);
            }
            Ok(vec![offline_name(region, name)?])
        }
    }
}

/// `GameProfileArgument.SelectorResult`：空结果报 `argument.entity.notfound.player`。
fn game_profile_players(
    region: &Region,
    source: PlayerId,
    level: u8,
    selector: &EntitySelector,
) -> Result<Vec<PlayerId>, CommandFail> {
    let ids = select::players(region, source, level, selector)
        .map_err(|_| fail("argument.entity.notfound.player"))?;
    Ok(ids)
}

/// 官方 `UUIDUtil.createOfflinePlayerUUID`：`OfflinePlayer:<name>` 的 MD5，version 3。
fn offline_uuid(name: &str) -> Uuid {
    use md5::{Digest, Md5};
    let digest = Md5::digest(format!("OfflinePlayer:{name}"));
    Builder::from_md5_bytes(digest.into()).into_uuid()
}

/// 官方 `NameAndId.createOffline`，名字不在线时用它。
fn offline_name(region: &Region, name: &str) -> Result<NameAndId, CommandFail> {
    if region.online_mode {
        // 没有 Mojang 查询，走官方 `UserNameToIdResolver` 查不到的分支。
        Err(fail("argument.player.unknown"))
    } else {
        Ok(NameAndId {
            uuid: offline_uuid(name),
            name: name.to_owned(),
        })
    }
}

/// 官方 `PlayerList.op/deop` 之后的 `sendPlayerPermissionLevel`：
/// entity_event 24+等级，再发这名玩家用得了的整棵命令树。
fn apply_online_permission(region: &mut Region, uuid: Uuid, level: u8, op: bool) {
    let targets: Vec<PlayerId> = region
        .players
        .iter()
        .filter(|(_, player)| player.uuid == uuid)
        .map(|(id, _)| *id)
        .collect();
    for id in targets {
        if let Some(player) = region.players.get_mut(&id) {
            player.permission_level = level;
            player.is_op = op;
        }
        crate::command::send_session(&mut region.links, id, level, id.0);
    }
}

fn level_of(region: &Region, id: PlayerId) -> u8 {
    region
        .players
        .get(&id)
        .map(|player| player.permission_level)
        .unwrap_or(permission::ALL)
}

fn player_name(region: &Region, id: PlayerId) -> String {
    region
        .players
        .get(&id)
        .map(|player| player.name.clone())
        .unwrap_or_default()
}

/// 官方 `BanPlayerCommands` / `PardonCommand` 的 reason 组件。
/// 有签名消息时和 `/say` 一样用客户端签过的文本。
fn reason_message(entry: &BanEntry) -> Component {
    match &entry.reason {
        Some(reason) => Component::text(reason.clone()),
        None => Component::translate("multiplayer.disconnect.banned.reason.default", []),
    }
}

fn render(region: &Region, source: PlayerId, message: &Message) -> Result<String, CommandFail> {
    if message.parts.is_empty() {
        return Ok(message.text.clone());
    }
    let chars: Vec<char> = message.text.chars().collect();
    let mut text = String::new();
    let mut cursor = 0;
    let level = level_of(region, source);
    for part in &message.parts {
        let start = part.start.min(chars.len());
        if start > cursor {
            text.extend(chars[cursor..start].iter().copied());
        }
        let ids = select::mention(region, source, level, &part.selector)?;
        let names: Vec<String> = ids.iter().map(|id| player_name(region, *id)).collect();
        text.push_str(&names.join(", "));
        cursor = part.end.min(chars.len());
    }
    if cursor < chars.len() {
        text.extend(chars[cursor..].iter().copied());
    }
    Ok(text)
}

fn required_text<'a>(parsed: &'a ParsedCommand, name: &str) -> Result<&'a str, CommandFail> {
    parsed
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| fail("command.failed"))
}

fn required_int(parsed: &ParsedCommand, name: &str) -> Result<i32, CommandFail> {
    parsed
        .get(name)
        .and_then(Value::as_integer)
        .ok_or_else(|| fail("command.failed"))
}

/// 封禁项：官方 `BanListEntry(NameAndId, null, source, null, reason)`。
fn new_ban(region: &Region, source: PlayerId, reason: Option<String>) -> BanEntry {
    BanEntry {
        created: crate::admin::ban_date(region.now_millis()),
        source: player_name(region, source),
        expires: None,
        expires_millis: None,
        reason,
    }
}

/// 官方 `MinecraftServer.kickUnlistedPlayers`。
fn kick_unlisted(global: &Global, region: &mut Region) {
    if !global.admin.kicks_unlisted() {
        return;
    }
    let reason = Component::translate("multiplayer.disconnect.not_whitelisted", []);
    let ids: Vec<PlayerId> = region
        .players
        .iter()
        .filter(|(_, player)| {
            !global.admin.whitelist.contains_key(&player.uuid)
                && !global.admin.ops.contains_key(&player.uuid)
        })
        .map(|(id, _)| *id)
        .collect();
    for id in ids {
        region.links.kick(id, &reason);
    }
}

// ---------------------------------------------------------------------------
// /op /deop
// ---------------------------------------------------------------------------

pub(crate) fn op(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let targets = game_profiles(global, region, id, parsed, "targets")?;
    let mut changed = 0;
    for target in targets {
        if global.admin.ops.contains_key(&target.uuid) {
            continue;
        }
        global.admin.op_player(target.clone());
        let level = global.admin.op(target.uuid).map_or(OP_LEVEL, |op| op.level);
        apply_online_permission(region, target.uuid, level, true);
        feedback::success(
            region,
            id,
            Component::translate("commands.op.success", [Component::text(target.name).into()]),
            true,
        );
        changed += 1;
    }
    if changed == 0 {
        return Err(fail("commands.op.failed"));
    }
    region.admin_changes.ops = true;
    Ok(())
}

pub(crate) fn deop(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let targets = game_profiles(global, region, id, parsed, "targets")?;
    let mut changed = 0;
    for target in targets {
        if !global.admin.deop_player(target.uuid) {
            continue;
        }
        apply_online_permission(region, target.uuid, 0, false);
        feedback::success(
            region,
            id,
            Component::translate(
                "commands.deop.success",
                [Component::text(target.name).into()],
            ),
            true,
        );
        changed += 1;
    }
    if changed == 0 {
        return Err(fail("commands.deop.failed"));
    }
    region.admin_changes.ops = true;
    kick_unlisted(global, region);
    Ok(())
}

// ---------------------------------------------------------------------------
// /ban /ban-ip /banlist /pardon /pardon-ip
// ---------------------------------------------------------------------------

pub(crate) fn ban(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let targets = game_profiles(global, region, id, parsed, "targets")?;
    let reason = optional_reason(region, id, parsed)?;
    let mut changed = 0;
    for target in targets {
        if global.admin.player_bans.contains_key(&target.uuid) {
            continue;
        }
        let entry = new_ban(region, id, reason.clone());
        global.admin.player_bans.insert(
            target.uuid,
            BanListEntry {
                name: target.name.clone(),
                entry: entry.clone(),
            },
        );
        feedback::success(
            region,
            id,
            Component::translate(
                "commands.ban.success",
                [
                    Component::text(target.name.clone()).into(),
                    reason_message(&entry).into(),
                ],
            ),
            true,
        );
        kick_by_uuid(region, target.uuid, "multiplayer.disconnect.banned");
        changed += 1;
    }
    if changed == 0 {
        return Err(fail("commands.ban.failed"));
    }
    region.admin_changes.player_bans = true;
    Ok(())
}

fn optional_reason(
    region: &Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<Option<String>, CommandFail> {
    match parsed.get("reason").and_then(Value::as_message) {
        Some(message) => Ok(Some(render(region, id, message)?)),
        None => Ok(None),
    }
}

fn kick_by_uuid(region: &mut Region, uuid: Uuid, key: &'static str) {
    let reason = Component::translate(key, []);
    let ids: Vec<PlayerId> = region
        .players
        .iter()
        .filter(|(_, player)| player.uuid == uuid)
        .map(|(id, _)| *id)
        .collect();
    for id in ids {
        region.links.kick(id, &reason);
    }
}

/// 官方 `InetAddresses.isInetAddress` 的 IPv4/IPv6 判定，Bamboo 用标准库解析。
fn is_inet_address(text: &str) -> bool {
    text.parse::<std::net::IpAddr>().is_ok()
}

/// `ban-ip` 的 target 可以是 IP，也可以是在线玩家的名字。
fn resolve_ip(region: &Region, target: &str) -> Result<String, CommandFail> {
    if is_inet_address(target) {
        return Ok(target.to_owned());
    }
    region
        .players
        .iter()
        .find(|(_, player)| player.name.eq_ignore_ascii_case(target))
        .and_then(|(id, _)| region.peer_ip(*id))
        .map(str::to_owned)
        .ok_or_else(|| fail("commands.banip.invalid"))
}

pub(crate) fn ban_ip(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let target = required_text(parsed, "target")?;
    let ip = resolve_ip(region, target)?;
    if global.admin.ip_bans.contains_key(&ip) {
        return Err(fail("commands.banip.failed"));
    }
    let reason = optional_reason(region, id, parsed)?;
    let entry = new_ban(region, id, reason);
    global.admin.ip_bans.insert(
        ip.clone(),
        BanListEntry {
            name: ip.clone(),
            entry: entry.clone(),
        },
    );
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.banip.success",
            [
                TranslateArg::from(ip.clone()),
                reason_message(&entry).into(),
            ],
        ),
        true,
    );
    region.admin_changes.ip_bans = true;
    let affected: Vec<PlayerId> = region
        .players
        .iter()
        .filter(|(id, _)| region.peer_ip(**id) == Some(ip.as_str()))
        .map(|(player_id, _)| *player_id)
        .collect();
    if !affected.is_empty() {
        let names = join_names(region, &affected);
        feedback::success(
            region,
            id,
            Component::translate(
                "commands.banip.info",
                [
                    TranslateArg::from(affected.len() as i32),
                    Component::text(names).into(),
                ],
            ),
            true,
        );
    }
    let reason = Component::translate("multiplayer.disconnect.ip_banned", []);
    for target in affected {
        region.links.kick(target, &reason);
    }
    Ok(())
}

fn join_names(region: &Region, ids: &[PlayerId]) -> String {
    ids.iter()
        .map(|id| player_name(region, *id))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn banlist_all(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    show_banlist(global, region, id, BanListKind::All)
}

pub(crate) fn banlist_ips(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    show_banlist(global, region, id, BanListKind::Ips)
}

pub(crate) fn banlist_players(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    show_banlist(global, region, id, BanListKind::Players)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BanListKind {
    All,
    Ips,
    Players,
}

/// 官方 `BanListCommands.showList`。空名单只发 `commands.banlist.none`，
/// 否则先发计数，再逐条发 `commands.banlist.entry`。
fn show_banlist(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    kind: BanListKind,
) -> Result<(), CommandFail> {
    let mut entries: Vec<(Component, String, Component)> = Vec::new();
    if kind != BanListKind::Ips {
        for ban in global.admin.player_bans.values() {
            entries.push((
                Component::text(ban.name.clone()),
                ban.entry.source.clone(),
                reason_message(&ban.entry),
            ));
        }
    }
    if kind != BanListKind::Players {
        for ban in global.admin.ip_bans.values() {
            entries.push((
                Component::text(ban.name.clone()),
                ban.entry.source.clone(),
                reason_message(&ban.entry),
            ));
        }
    }
    if entries.is_empty() {
        feedback::success(
            region,
            id,
            Component::translate("commands.banlist.none", []),
            false,
        );
    } else {
        feedback::success(
            region,
            id,
            Component::translate(
                "commands.banlist.list",
                [TranslateArg::from(entries.len() as i32)],
            ),
            false,
        );
        for (display, source, reason) in entries {
            feedback::success(
                region,
                id,
                Component::translate(
                    "commands.banlist.entry",
                    [display.into(), TranslateArg::from(source), reason.into()],
                ),
                false,
            );
        }
    }
    Ok(())
}

pub(crate) fn pardon(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let targets = game_profiles(global, region, id, parsed, "targets")?;
    let mut changed = 0;
    for target in targets {
        if global.admin.player_bans.remove(&target.uuid).is_none() {
            continue;
        }
        feedback::success(
            region,
            id,
            Component::translate(
                "commands.pardon.success",
                [Component::text(target.name).into()],
            ),
            true,
        );
        changed += 1;
    }
    if changed == 0 {
        return Err(fail("commands.pardon.failed"));
    }
    region.admin_changes.player_bans = true;
    Ok(())
}

pub(crate) fn pardon_ip(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let target = required_text(parsed, "target")?;
    if !is_inet_address(target) {
        return Err(fail("commands.pardonip.invalid"));
    }
    if global.admin.ip_bans.remove(target).is_none() {
        return Err(fail("commands.pardonip.failed"));
    }
    feedback::success(
        region,
        id,
        Component::translate("commands.pardonip.success", [TranslateArg::from(target)]),
        true,
    );
    region.admin_changes.ip_bans = true;
    Ok(())
}

// ---------------------------------------------------------------------------
// /whitelist
// ---------------------------------------------------------------------------

pub(crate) fn whitelist_enable(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    if global.admin.using_whitelist {
        return Err(fail("commands.whitelist.alreadyOn"));
    }
    global.admin.using_whitelist = true;
    region.admin_changes.using_whitelist = true;
    feedback::success(
        region,
        id,
        Component::translate("commands.whitelist.enabled", []),
        true,
    );
    kick_unlisted(global, region);
    Ok(())
}

pub(crate) fn whitelist_disable(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    if !global.admin.using_whitelist {
        return Err(fail("commands.whitelist.alreadyOff"));
    }
    global.admin.using_whitelist = false;
    region.admin_changes.using_whitelist = true;
    feedback::success(
        region,
        id,
        Component::translate("commands.whitelist.disabled", []),
        true,
    );
    Ok(())
}

pub(crate) fn whitelist_add(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let targets = game_profiles(global, region, id, parsed, "targets")?;
    let mut changed = 0;
    for target in targets {
        if global.admin.whitelist.contains_key(&target.uuid) {
            continue;
        }
        global.admin.whitelist.insert(target.uuid, target.clone());
        feedback::success(
            region,
            id,
            Component::translate(
                "commands.whitelist.add.success",
                [Component::text(target.name).into()],
            ),
            true,
        );
        changed += 1;
    }
    if changed == 0 {
        return Err(fail("commands.whitelist.add.failed"));
    }
    region.admin_changes.whitelist = true;
    Ok(())
}

pub(crate) fn whitelist_remove(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let targets = game_profiles(global, region, id, parsed, "targets")?;
    let mut changed = 0;
    for target in targets {
        if global.admin.whitelist.remove(&target.uuid).is_none() {
            continue;
        }
        feedback::success(
            region,
            id,
            Component::translate(
                "commands.whitelist.remove.success",
                [Component::text(target.name).into()],
            ),
            true,
        );
        changed += 1;
    }
    if changed == 0 {
        return Err(fail("commands.whitelist.remove.failed"));
    }
    region.admin_changes.whitelist = true;
    kick_unlisted(global, region);
    Ok(())
}

pub(crate) fn whitelist_list(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    let names: Vec<String> = global
        .admin
        .whitelist
        .values()
        .map(|entry| entry.name.clone())
        .collect();
    if names.is_empty() {
        feedback::success(
            region,
            id,
            Component::translate("commands.whitelist.none", []),
            false,
        );
    } else {
        feedback::success(
            region,
            id,
            Component::translate(
                "commands.whitelist.list",
                [
                    TranslateArg::from(names.len() as i32),
                    TranslateArg::from(names.join(",")),
                ],
            ),
            false,
        );
    }
    Ok(())
}

pub(crate) fn whitelist_reload(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    feedback::success(
        region,
        id,
        Component::translate("commands.whitelist.reloaded", []),
        true,
    );
    if let Some(sender) = &global.admin_messages {
        let _ = sender.send(crate::admin::AdminMessage::ReloadWhitelist);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// /stop /setidletimeout /transfer /version
// ---------------------------------------------------------------------------

pub(crate) fn stop(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    feedback::success(
        region,
        id,
        Component::translate("commands.stop.stopping", []),
        true,
    );
    global.admin.stop_requested = true;
    Ok(())
}

pub(crate) fn set_idle_timeout(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let minutes = required_int(parsed, "minutes")?;
    global.admin.idle_timeout_minutes = minutes;
    let message = if minutes > 0 {
        Component::translate(
            "commands.setidletimeout.success",
            [TranslateArg::from(minutes)],
        )
    } else {
        Component::translate("commands.setidletimeout.success.disabled", [])
    };
    feedback::success(region, id, message, true);
    Ok(())
}

pub(crate) fn transfer(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = global;
    let hostname = required_text(parsed, "hostname")?.to_owned();
    let port = parsed
        .get("port")
        .and_then(Value::as_integer)
        .unwrap_or(DEFAULT_PORT);
    let targets: Vec<PlayerId> = match parsed.get("players").and_then(Value::as_entity) {
        Some(selector) => select::players(region, id, level_of(region, id), selector)?,
        None => vec![id],
    };
    if targets.is_empty() {
        return Err(fail("commands.transfer.error.no_players"));
    }
    let packet = mc_bamboo_protocol::packet::play::Transfer {
        host: &hostname,
        port,
    };
    let frames = crate::links::encode_frames(&packet);
    for target in &targets {
        region.links.send_frames(*target, &frames);
    }
    let message = if let [only] = targets.as_slice() {
        Component::translate(
            "commands.transfer.success.single",
            [
                Component::text(player_name(region, *only)).into(),
                TranslateArg::from(hostname),
                TranslateArg::from(port),
            ],
        )
    } else {
        Component::translate(
            "commands.transfer.success.multiple",
            [
                TranslateArg::from(targets.len() as i32),
                TranslateArg::from(hostname),
                TranslateArg::from(port),
            ],
        )
    };
    feedback::success(region, id, message, true);
    Ok(())
}

pub(crate) fn version(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = (global, parsed);
    for line in version_lines() {
        feedback::send_system(region, id, &line);
    }
    Ok(())
}

/// 官方 `VersionCommand.dumpVersion` 的行序。版本信息来自 `mc-bamboo-data` 的常量，
/// 其余字段是 26.3 官方 `version.json` 里的固定值。
fn version_lines() -> Vec<Component> {
    use crate::admin::{BUILD_TIME, PACK_DATA, PACK_RESOURCE, SERIES};
    use mc_bamboo_data::version::{DATA_VERSION, GAME_VERSION, PROTOCOL_VERSION};
    let mut lines = vec![Component::translate("commands.version.header", [])];
    lines.push(Component::translate(
        "commands.version.id",
        [TranslateArg::from(GAME_VERSION)],
    ));
    lines.push(Component::translate(
        "commands.version.name",
        [TranslateArg::from(GAME_VERSION)],
    ));
    lines.push(Component::translate(
        "commands.version.data",
        [TranslateArg::from(DATA_VERSION)],
    ));
    lines.push(Component::translate(
        "commands.version.series",
        [TranslateArg::from(SERIES)],
    ));
    lines.push(Component::translate(
        "commands.version.protocol",
        [
            TranslateArg::from(PROTOCOL_VERSION),
            TranslateArg::from(format!("0x{PROTOCOL_VERSION:x}")),
        ],
    ));
    lines.push(Component::translate(
        "commands.version.build_time",
        [Component::text(BUILD_TIME).into()],
    ));
    lines.push(Component::translate(
        "commands.version.pack.resource",
        [TranslateArg::from(PACK_RESOURCE)],
    ));
    lines.push(Component::translate(
        "commands.version.pack.data",
        [TranslateArg::from(PACK_DATA)],
    ));
    lines.push(Component::translate("commands.version.stable.yes", []));
    lines
}

#[cfg(test)]
mod tests;
