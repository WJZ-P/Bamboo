//! `gamemode` 和 `ServerPlayer.setGameMode`。
//!
//! 只认官方 26.3 的四个全名：`survival`、`creative`、`adventure`、`spectator`。
//! 没有权限等级，任何玩家都能改自己的模式。不发 `commands.gamemode.success.self`。
//!
//! `gamemode` and `ServerPlayer.setGameMode`.
//!
//! Only the four full names vanilla 26.3 accepts: `survival`, `creative`, `adventure`, `spectator`.
//! There are no permission levels, so any player can change their own mode. `commands.gamemode.success.self` is not sent.

use mc_bamboo_data::registries::{attribute, attribute_defaults};
use mc_bamboo_protocol::packet::play::{
    AttributeSnapshot, ChangeGameMode, GameMode, PlayerAbilities, PlayerInfoEntry,
    PlayerInfoUpdate, UpdateAttributes,
};

use crate::{
    global::Global,
    inbound::PlayerId,
    links::encode_frames,
    region::{self, Region},
};

/// 和进服时那份能力包一样的速度。官方默认就是这两个数。
///
/// The same speeds as the join abilities packet. These are vanilla's defaults.
const FLYING_SPEED: f32 = 0.05;
const WALKING_SPEED: f32 = 0.1;

const PLAIN_ATTRIBUTES: [AttributeSnapshot<'static>; 2] = [
    AttributeSnapshot {
        attribute: attribute::BLOCK_INTERACTION_RANGE,
        base: attribute_defaults::BLOCK_INTERACTION_RANGE,
        modifiers: &[],
    },
    AttributeSnapshot {
        attribute: attribute::ENTITY_INTERACTION_RANGE,
        base: attribute_defaults::ENTITY_INTERACTION_RANGE,
        modifiers: &[],
    },
];

/// 认出 `gamemode <模式>` 就处理并返回真。别的命令返回假，交给原来的聊天处理。
///
/// Handles `gamemode <mode>` and returns true. Any other command returns false for the existing chat path.
pub(crate) fn try_command(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    command: &str,
) -> bool {
    let mut words = command.split_whitespace();
    let Some(name) = words.next() else {
        return false;
    };
    if name != "gamemode" {
        return false;
    }
    let Some(mode) = words.next().and_then(game_mode_by_name) else {
        return false;
    };
    if words.next().is_some() {
        return false;
    }
    set_game_mode(global, region, id, mode);
    true
}

/// `GameType.byName`：序列化名字，区分大小写。
fn game_mode_by_name(name: &str) -> Option<GameMode> {
    match name {
        "survival" => Some(GameMode::Survival),
        "creative" => Some(GameMode::Creative),
        "adventure" => Some(GameMode::Adventure),
        "spectator" => Some(GameMode::Spectator),
        _ => None,
    }
}

/// `ServerPlayer.setGameMode`。模式没变时什么包都不发。
///
/// `ServerPlayer.setGameMode`. An unchanged mode sends nothing.
pub(crate) fn set_game_mode(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    mode: GameMode,
) {
    let Some(player) = region.players.get_mut(&id) else {
        return;
    };
    if player.game_mode == mode {
        return;
    }
    let flying = flying_after_change(player.sync.flying, player.on_ground, mode);
    player.game_mode = mode;
    player.sync.flying = flying;
    global.set_game_mode(id, mode);

    let abilities = abilities(mode, flying);
    region.links.send(id, &abilities);
    broadcast_mode(region, id, mode);
    region.links.send(id, &ChangeGameMode { mode });
    // `setGameMode` 在游戏事件之后再调一次 `onUpdateAbilities`。
    //
    // `setGameMode` calls `onUpdateAbilities` again after the game event.
    region.links.send(id, &abilities);
    send_attributes(region, id, mode);
}

/// `GameType.updatePlayerAbilities`，接着是落地时取消飞行。
///
/// `GameType.updatePlayerAbilities`, then the ground check that stops flying.
fn flying_after_change(was_flying: bool, on_ground: bool, mode: GameMode) -> bool {
    let mut flying = match mode {
        GameMode::Creative => was_flying,
        GameMode::Spectator => true,
        GameMode::Survival | GameMode::Adventure => false,
    };
    // 官方用 `isInRangeOfGround`。这里只有 `on_ground`。
    //
    // Vanilla uses `isInRangeOfGround`. Only `on_ground` is available here.
    if flying && mode != GameMode::Spectator && on_ground {
        flying = false;
    }
    flying
}

fn abilities(mode: GameMode, flying: bool) -> PlayerAbilities {
    let creative = mode == GameMode::Creative;
    let spectator = mode == GameMode::Spectator;
    PlayerAbilities {
        invulnerable: creative || spectator,
        flying,
        can_fly: creative || spectator,
        instant_build: creative,
        flying_speed: FLYING_SPEED,
        walking_speed: WALKING_SPEED,
    }
}

fn broadcast_mode(region: &mut Region, id: PlayerId, mode: GameMode) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    let entry = PlayerInfoEntry {
        uuid: player.uuid,
        name: "",
        properties: &[],
        game_mode: mode,
        listed: true,
        latency: 0,
        display_name: None,
        list_order: 0,
        show_hat: player.shows_hat(),
    };
    let frames = encode_frames(&PlayerInfoUpdate {
        actions: PlayerInfoUpdate::UPDATE_GAME_MODE,
        entries: &[entry],
    });
    region.links.broadcast(&frames);
}

fn send_attributes(region: &mut Region, id: PlayerId, mode: GameMode) {
    let creative = mode == GameMode::Creative;
    let attributes = if creative {
        region::CREATIVE_ATTRIBUTES.as_slice()
    } else {
        PLAIN_ATTRIBUTES.as_slice()
    };
    let Some(player) = region.players.get(&id) else {
        return;
    };
    region.links.send(
        id,
        &UpdateAttributes {
            entity_id: player.id.0,
            attributes,
        },
    );
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::packets::play::clientbound;
    use mc_bamboo_protocol::packet::play::PlayRequest;

    use super::*;
    use crate::world::tests::Harness;

    #[test]
    fn gamemode_switches_and_the_same_mode_sends_nothing() {
        let mut server = Harness::new(2);
        let client = server.join_and_settle("kitten");
        client.take_frames();

        server.request(
            &client,
            PlayRequest::ChatCommand {
                command: "gamemode survival".to_owned(),
            },
        );
        server.tick();
        let frames = client.take_frames();
        let events: Vec<_> = frames
            .iter()
            .filter(|frame| frame.id == clientbound::GAME_EVENT)
            .collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].body, [3, 0, 0, 0, 0]);
        assert_eq!(
            frames
                .iter()
                .filter(|frame| frame.id == clientbound::PLAYER_ABILITIES)
                .count(),
            2
        );
        assert!(
            frames
                .iter()
                .any(|frame| frame.id == clientbound::PLAYER_INFO_UPDATE)
        );
        assert_eq!(server.player(&client).game_mode, GameMode::Survival);

        server.request(
            &client,
            PlayRequest::ChatCommand {
                command: "gamemode survival".to_owned(),
            },
        );
        server.tick();
        assert!(
            client
                .take_frames()
                .iter()
                .all(|frame| frame.id != clientbound::GAME_EVENT)
        );

        server.request(
            &client,
            PlayRequest::ChatCommand {
                command: "gamemode creative".to_owned(),
            },
        );
        server.tick();
        let mut creative = vec![3];
        creative.extend_from_slice(&1.0f32.to_be_bytes());
        let frames = client.take_frames();
        assert!(frames.iter().any(|frame| frame.body == creative));
        assert_eq!(server.player(&client).game_mode, GameMode::Creative);
    }
}
