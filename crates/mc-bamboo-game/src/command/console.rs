//! 服务器控制台这个命令来源（`CommandSourceStack` 的服务器来源）和它的分发。
//!
//! 官方 `DedicatedServer` 的控制台线程读 stdin，把每行排队，主 tick 线程
//! `handleConsoleInputs` 再执行；命令来源是 `createCommandSourceStack()`：
//! 名字 `Server`、权限 4、`entity == null`（不是世界里的实体）、反馈打日志。
//! Bamboo 里读 stdin 在连接层，命令经 [`Inbound::ConsoleCommand`] 交给世界线程，
//! 世界线程从不读 stdin。
//!
//! The server-console command source (`CommandSourceStack` server source) and its
//! dispatch. The official `DedicatedServer` console thread reads stdin and the main
//! tick thread runs each line with a source named `Server`, permission 4, no entity,
//! and feedback to the log. Bamboo reads stdin in the connection layer and hands
//! commands to the world thread via [`Inbound::ConsoleCommand`]; the world thread
//! never reads stdin.

use mc_bamboo_command::permission;

use crate::{
    command::{commands, feedback},
    global::Global,
    inbound::PlayerId,
    region::Region,
};

/// 控制台来源的显示名，官方 `createCommandSourceStack` 的 `Component.literal("Server")`。
///
/// The console source's display name: official `Component.literal("Server")`.
pub(crate) const NAME: &str = "Server";

/// 控制台来源的权限，官方 `LevelBasedPermissionSet.OWNER` = 4。
///
/// The console source's permission level: official `LevelBasedPermissionSet.OWNER` = 4.
pub(crate) const PERMISSION: u8 = permission::OWNERS;

/// 控制台来源的占位 `PlayerId`。真实玩家 ID 从 1 起递增，永远取不到这个值；
/// 它不会出现在 `region.players` 里（官方控制台来源 `entity == null`）。
///
/// Sentinel `PlayerId` for the console source. Real player ids start at 1 and increase,
/// so they never reach this value; it is absent from `region.players` because the
/// official console source has `entity == null`.
pub(crate) const SOURCE: PlayerId = PlayerId(i32::MIN);

/// 这个来源是不是服务器控制台。
///
/// Whether a source is the server console.
pub(crate) fn is_console(id: PlayerId) -> bool {
    id == SOURCE
}

/// 官方 `DedicatedServer.handleConsoleInputs` → `Commands.performPrefixedCommand`：
/// 去掉一个可选的前导 `/`，按权限 4 解析，把结果交给玩家命令共用的动作表执行。
/// 控制台命令不走签名聊天（那是玩家聊天命令才有的）。
///
/// Official `DedicatedServer.handleConsoleInputs` → `Commands.performPrefixedCommand`:
/// strip one optional leading `/`, parse at permission 4, and run the result through the
/// same action table player commands use. Console commands skip signed-chat handling.
pub(crate) fn on_console_command(global: &mut Global, region: &mut Region, line: &str) {
    let command = line.strip_prefix('/').unwrap_or(line);
    match commands().graph.parse(command, PERMISSION) {
        Ok(parsed) => {
            let action = commands()
                .actions
                .get(parsed.action)
                .and_then(|action| *action);
            if let Some(action) = action {
                if let Err(failure) = action(global, region, SOURCE, &parsed) {
                    feedback::report(region, SOURCE, command, &failure);
                }
            } else {
                feedback::report(region, SOURCE, command, &feedback::fail("command.failed"));
            }
        }
        Err(failure) => feedback::report(
            region,
            SOURCE,
            command,
            &feedback::fail_parse(command, &failure.error, failure.cursor),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use mc_bamboo_world::flat;
    use uuid::Uuid;

    use super::*;
    use crate::{
        inbound::{PlayerId, PlayerSink},
        outgoing::Outgoing,
        player::Player,
    };

    #[derive(Default, Clone)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl PlayerSink for Sink {
        fn send(&mut self, frames: Outgoing) -> bool {
            self.0
                .lock()
                .expect("测试输出锁不该中毒")
                .extend(frames.to_raw_bytes());
            true
        }

        fn close(&mut self) {}
    }

    /// 一名等级 4 的在线 OP，能收到控制台命令广播的 `chat.type.admin`。
    ///
    /// One level-4 online op that can receive the console command's `chat.type.admin`.
    fn world_with_op() -> (Global, Region, Sink) {
        let mut region = Region::new(2, 20, false);
        region.send_command_feedback = true;
        region.broadcast_console_to_ops = true;
        let id = PlayerId(1);
        let mut player = Player::new(
            id,
            Uuid::from_u128(1),
            "Mochi".to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            mc_bamboo_core::Vec3::new(8.5, f64::from(flat::surface_y() + 1), 8.5),
        );
        player.permission_level = 4;
        player.is_op = true;
        region.insert_player(player);
        let sink = Sink::default();
        region.links.add(id, Box::new(sink.clone()));
        (Global::default(), region, sink)
    }

    /// `chat.type.admin` 和来源名都写进 `SystemChat` 的 NBT，直接找这两段文本。
    ///
    /// `chat.type.admin` and the source name appear in the `SystemChat` NBT bytes.
    fn broadcasted(sink: &Sink) -> bool {
        let bytes = sink.0.lock().expect("测试输出锁不该中毒").clone();
        let text = String::from_utf8_lossy(&bytes);
        text.contains("chat.type.admin") && text.contains(NAME)
    }

    #[test]
    fn console_stop_requests_shutdown_at_owner_permission() {
        let (mut global, mut region, _sink) = world_with_op();
        // `/stop` 要等级 4：命令能到达 stop 节点，说明控制台按等级 4 解析。
        // `/stop` needs level 4: reaching the node proves the console parses at level 4.
        on_console_command(&mut global, &mut region, "stop");
        assert!(global.admin.stop_requested);
    }

    #[test]
    fn a_leading_slash_is_optional() {
        let (mut global, mut region, _sink) = world_with_op();
        on_console_command(&mut global, &mut region, "/stop");
        assert!(global.admin.stop_requested);
    }

    #[test]
    fn an_unknown_command_does_not_request_shutdown() {
        let (mut global, mut region, _sink) = world_with_op();
        on_console_command(&mut global, &mut region, "definitely-not-a-command");
        assert!(!global.admin.stop_requested);
    }

    #[test]
    fn console_feedback_broadcasts_to_ops_when_enabled() {
        let (mut global, mut region, sink) = world_with_op();
        region.broadcast_console_to_ops = true;
        on_console_command(&mut global, &mut region, "stop");
        region.links.flush();
        assert!(broadcasted(&sink));
    }

    #[test]
    fn console_feedback_does_not_broadcast_when_disabled() {
        let (mut global, mut region, sink) = world_with_op();
        region.broadcast_console_to_ops = false;
        on_console_command(&mut global, &mut region, "stop");
        region.links.flush();
        assert!(!broadcasted(&sink));
    }
}
