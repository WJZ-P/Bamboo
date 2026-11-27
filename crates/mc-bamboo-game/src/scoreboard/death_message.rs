//! 玩家死亡消息按队伍的 `deathMessageVisibility` 分发。
//!
//! 官方 `ServerPlayer.die`：死者没有队伍或者规则是 `always` 时走
//! `PlayerList.broadcastSystemMessage`；`hideForOtherTeams` 走 `broadcastSystemToTeam`
//! （同队、不含死者自己）；`hideForOwnTeam` 走 `broadcastSystemToAllExceptTeam`（不在
//! 这一队的人）；`never` 谁都不发。
//!
//! Player death messages routed by the team's `deathMessageVisibility`, as in official
//! `ServerPlayer.die`: `PlayerList.broadcastSystemMessage` without a team or with
//! `always`; `broadcastSystemToTeam` (teammates, not the victim) for
//! `hideForOtherTeams`; `broadcastSystemToAllExceptTeam` (everyone outside the team) for
//! `hideForOwnTeam`; nobody for `never`.

use std::{collections::BTreeMap, sync::Arc};

use super::{DeathAudience, Scoreboard};
use crate::{inbound::PlayerId, links::Links, player::Player, region::Region};

/// 把一条死亡消息发给该看到它的玩家。
///
/// Send one death message to the players who should see it.
pub(crate) fn broadcast_death_message(
    players: &BTreeMap<PlayerId, Player>,
    links: &mut Links,
    board: &Scoreboard,
    frames: &[u8],
    victim: PlayerId,
    victim_name: &str,
) {
    let audience = board.death_audience(victim_name);
    if audience == DeathAudience::Everyone {
        crate::chat::broadcast_system_frames(players, links, frames, None);
        return;
    }
    send_to_audience(players, links, board, frames, victim, &audience);
    // 不在主控手里的岛上的接收者：那个岛落地时照同样的条件自己发。
    //
    // Receivers on islands that are away: each island sends under the same rule when
    // it lands.
    links.fan_out(|| {
        let frames: Arc<[u8]> = frames.into();
        Arc::new(move |region: &mut Region| {
            let board = Arc::clone(&region.global.scoreboard);
            send_to_audience(
                &region.players,
                &mut region.links,
                &board,
                &frames,
                victim,
                &audience,
            );
        })
    });
}

/// 官方 `ServerPlayer.sendSystemMessage`：聊天可见性是 `HIDDEN` 的玩家不收。
///
/// Official `ServerPlayer.sendSystemMessage`: a player with HIDDEN chat is skipped.
fn send_to_audience(
    players: &BTreeMap<PlayerId, Player>,
    links: &mut Links,
    board: &Scoreboard,
    frames: &[u8],
    victim: PlayerId,
    audience: &DeathAudience,
) {
    let ids: Vec<PlayerId> = players
        .iter()
        .filter(|(id, player)| {
            sees(board, audience, **id == victim, &player.name)
                && crate::chat::accepts_system_messages(player.client.chat_visibility, false)
        })
        .map(|(id, _)| *id)
        .collect();
    for id in ids {
        links.send_frames(id, frames);
    }
}

fn sees(board: &Scoreboard, audience: &DeathAudience, is_victim: bool, name: &str) -> bool {
    match audience {
        DeathAudience::Everyone => true,
        DeathAudience::Team(team) => !is_victim && board.team_of(name) == Some(team),
        DeathAudience::OtherTeams(team) => board.team_of(name) != Some(team),
        DeathAudience::Nobody => false,
    }
}
