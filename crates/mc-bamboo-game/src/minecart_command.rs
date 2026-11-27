//! 命令方块矿车的编辑与激活，执行共用世界的命令树。
//! Command minecart editing and activation through the shared command dispatcher.

use std::cell::RefCell;

use mc_bamboo_blocks::entity::{EntityKind, EntityUid, minecart::CommandActivation};
use mc_bamboo_command::permission;
use mc_bamboo_protocol::text::Component;

use crate::{
    command::{self, Action, feedback, source},
    global::Global,
    global_port::GlobalPort,
    inbound::PlayerId,
    region::Region,
};

struct Output {
    id: PlayerId,
    track_output: bool,
    last: Option<Component>,
    successes: i32,
}

thread_local! {
    static OUTPUT: RefCell<Option<Output>> = const { RefCell::new(None) };
}

/// 检查来源是不是正在执行的命令矿车。 / Whether the source is the running cart.
pub(crate) fn tracks_output(id: PlayerId) -> Option<bool> {
    OUTPUT.with(|output| {
        output
            .borrow()
            .as_ref()
            .filter(|output| output.id == id)
            .map(|output| output.track_output)
    })
}

/// 拦截命令来源的系统消息。 / Accept system messages sent to the cart source.
pub(crate) fn capture_output(id: PlayerId, message: &Component) -> bool {
    OUTPUT.with(|output| {
        let mut output = output.borrow_mut();
        let Some(output) = output.as_mut().filter(|output| output.id == id) else {
            return false;
        };
        if output.track_output {
            let stamp = chrono::Local::now().format("%H:%M:%S");
            output.last = Some(Component::text(format!("[{stamp}] ")).extra(message.clone()));
        }
        true
    })
}

/// 统计命令执行成功的分支。 / Count successful command branches.
pub(crate) fn record_success(id: PlayerId, line: &str) {
    if line.split_whitespace().next() == Some("execute") {
        return;
    }
    OUTPUT.with(|output| {
        if let Some(output) = output
            .borrow_mut()
            .as_mut()
            .filter(|output| output.id == id)
        {
            output.successes = output.successes.saturating_add(1);
        }
    });
}

/// 命令来源按矿车实体显示名字。 / The command source uses the cart's display name.
pub(crate) fn display_name(region: &Region, id: PlayerId) -> Component {
    crate::vehicle_display_name::styled(
        &region.global.scoreboard,
        id.0,
        "minecraft:command_block_minecart",
        Component::translate("entity.minecraft.command_block_minecart", []),
    )
}

pub(crate) fn on_set(
    region: &mut Region,
    player: PlayerId,
    uid: EntityUid,
    command: String,
    track_output: bool,
) {
    let Some(player_data) = region.players.get(&player) else {
        return;
    };
    if !crate::command_block::player_can_use(player_data) {
        let message = Component::translate("advMode.notAllowed", []);
        crate::chat::send_system_message(region, player, &message, false);
        return;
    }
    let enabled = region.global.command_blocks_work;
    let updated = region.entities.with_mut(uid, |entity| {
        let EntityKind::Minecart(cart) = &mut entity.kind else {
            return false;
        };
        let Some(base) = &mut cart.command_block else {
            return false;
        };
        if entity.removed {
            return false;
        }
        base.set_command(command.clone());
        base.track_output = track_output;
        if !track_output {
            base.last_output = None;
        }
        if enabled {
            base.update_synced_data();
        }
        true
    });
    if updated != Some(true) {
        return;
    }
    if let Some(entity) = region.entities.public_at(uid) {
        region.dirty.mark(entity.chunk_position());
    }
    if !command.is_empty() {
        let key = if enabled {
            "advMode.setCommand.success"
        } else {
            "advMode.setCommand.disabled"
        };
        let message = Component::translate(key, [command.into()]);
        crate::chat::send_system_message(region, player, &message, false);
    }
}

pub(crate) fn perform(global: &mut Global, region: &mut Region, activation: CommandActivation) {
    let enabled = region.global.command_blocks_work;
    let Some((line, track_output)) = region
        .entities
        .with_mut(activation.entity, |entity| {
            let EntityKind::Minecart(cart) = &mut entity.kind else {
                return None;
            };
            let base = cart.command_block.as_mut()?;
            if entity.removed || base.last_execution == activation.game_time {
                return None;
            }
            if base.command.eq_ignore_ascii_case("Searge") {
                base.last_output = Some(Component::text("#itzlipofutzli"));
                base.success_count = 1;
                return None;
            }
            base.success_count = 0;
            if !enabled || base.command.is_empty() {
                base.last_execution = if base.update_last_execution {
                    activation.game_time
                } else {
                    -1
                };
                return None;
            }
            base.last_output = None;
            Some((base.command.clone(), base.track_output))
        })
        .flatten()
    else {
        return;
    };
    let id = PlayerId(activation.entity);
    let source = source::Source {
        position: activation.position,
        yaw: activation.yaw,
        pitch: 0.0,
        entity: Some(source::Entity::World(activation.entity)),
    };
    let command = line.strip_prefix('/').unwrap_or(&line);
    let previous = OUTPUT.replace(Some(Output {
        id,
        track_output,
        last: None,
        successes: 0,
    }));
    source::with_override(id, source, || run(global, region, id, command));
    let Some(output) = OUTPUT.replace(previous) else {
        return;
    };
    region.entities.with_mut(activation.entity, |entity| {
        if let EntityKind::Minecart(cart) = &mut entity.kind
            && let Some(base) = &mut cart.command_block
        {
            base.success_count = base.success_count.saturating_add(output.successes);
            if let Some(last) = output.last {
                base.last_output = Some(last);
                base.update_synced_data();
            }
            base.last_execution = if base.update_last_execution {
                activation.game_time
            } else {
                -1
            };
        }
    });
    if let Some(entity) = region.entities.public_at(activation.entity) {
        region.dirty.mark(entity.chunk_position());
    }
    global.publish(region);
}

fn run(global: &mut Global, region: &mut Region, id: PlayerId, line: &str) {
    let parsed = match command::commands()
        .graph
        .parse(line, permission::GAMEMASTERS)
    {
        Ok(parsed) => parsed,
        Err(error) => {
            let failure = feedback::fail_parse(line, &error.error, error.cursor);
            feedback::report(region, id, line, &failure);
            return;
        }
    };
    let result = match command::commands()
        .actions
        .get(parsed.action)
        .and_then(|action| *action)
    {
        Some(Action::Island(action)) => {
            action(&mut GlobalPort::Direct(global), region, id, &parsed)
        }
        Some(Action::Global(action)) => action(global, region, id, &parsed),
        None => Err(feedback::fail("command.failed")),
    };
    match result {
        Ok(()) => record_success(id, line),
        Err(failure) => feedback::report(region, id, line, &failure),
    }
}

#[cfg(feature = "oracle-samples")]
pub use oracle::{CommandMinecartSample, replay_command_minecart};
#[cfg(feature = "oracle-samples")]
mod oracle;
