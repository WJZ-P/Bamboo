//! `save-all` / `save-on` / `save-off`，照官方 `SaveAllCommand`、`SaveOnCommand`、
//! `SaveOffCommand`。三个命令都要求 `LEVEL_OWNERS`（4）。
//!
//! 命令动作不碰存档：把 [`SaveCommand`] 压进 `Global`，由世界线程在 tick 里消费。
//! 权限等级和 `commands.save.saving` 照官方；`commands.save.success` /
//! `commands.save.failed` 由世界线程在保存真正完成后按 IO 栅栏回执发，
//! 写盘失败绝不报成功（F9 / 官方 `SaveAllCommand` 的 `ERROR_FAILED`）。
//!
//! `save-all` / `save-on` / `save-off`, mirroring the official commands. All need
//! `LEVEL_OWNERS` (4). Actions push a [`SaveCommand`] into `Global` for the world
//! thread; they never touch the disk. Permissions and `commands.save.saving`
//! follow vanilla; the world thread sends `commands.save.success` /
//! `commands.save.failed` from the IO barrier ack after the save really
//! completes — a failed write never reports success (F9).

use mc_bamboo_command::ParsedCommand;
use mc_bamboo_protocol::text::Component;

use crate::{
    command::feedback::{self, CommandFail},
    global::Global,
    inbound::PlayerId,
    region::Region,
    save_session::SaveCommand,
};

/// `save-all`：照官方先发 `commands.save.saving`；`commands.save.success` 由世界线程
/// 在快照都交给 IO 之后再发，所以这里只压请求，不提前报成功。
pub(crate) fn save_all(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    do_save_all(global, region, id, false)
}

/// `save-all flush`：多一道 IO 排空栅栏。
pub(crate) fn save_all_flush(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    do_save_all(global, region, id, true)
}

fn do_save_all(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    flush: bool,
) -> Result<(), CommandFail> {
    feedback::success(
        region,
        id,
        Component::translate("commands.save.saving", []),
        false,
    );
    global
        .save_requests
        .push(SaveCommand::SaveAll { flush, source: id });
    Ok(())
}

/// `save-on`：官方 `setAutoSave(true)` 返回 false 时抛 `commands.save.alreadyOn`。
pub(crate) fn save_on(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    if global.save_auto {
        return Err(feedback::fail("commands.save.alreadyOn"));
    }
    global.save_auto = true;
    global.save_requests.push(SaveCommand::SaveOn);
    feedback::success(
        region,
        id,
        Component::translate("commands.save.enabled", []),
        true,
    );
    Ok(())
}

/// `save-off`：官方 `setAutoSave(false)` 返回 false 时抛 `commands.save.alreadyOff`。
pub(crate) fn save_off(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let _ = parsed;
    if !global.save_auto {
        return Err(feedback::fail("commands.save.alreadyOff"));
    }
    global.save_auto = false;
    global.save_requests.push(SaveCommand::SaveOff);
    feedback::success(
        region,
        id,
        Component::translate("commands.save.disabled", []),
        true,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use mc_bamboo_core::Vec3;
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use super::*;
    use crate::{
        global::Global, inbound::PlayerSink, outgoing::Outgoing, player::Player, region::Region,
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

    fn setup() -> (Global, Region, PlayerId) {
        let mut region = Region::new(2, 20, false);
        region.send_command_feedback = true;
        let id = PlayerId(1);
        let mut player = Player::new(
            id,
            Uuid::from_u128(1),
            "Admin".to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            Vec3::new(8.5, 0.0, 8.5),
        );
        player.permission_level = 4;
        player.is_op = true;
        region.insert_player(player);
        region.links.add(id, Box::new(Sink::default()));
        (Global::default(), region, id)
    }

    #[test]
    fn owner_permission_is_required() {
        let graph = &crate::command::commands().graph;
        assert!(graph.parse("save-all", 4).is_ok());
        assert!(graph.parse("save-all flush", 4).is_ok());
        assert!(graph.parse("save-on", 4).is_ok());
        assert!(graph.parse("save-off", 4).is_ok());
        assert!(graph.parse("save-all", 3).is_err());
        assert!(graph.parse("save-on", 3).is_err());
    }

    #[test]
    fn save_on_off_toggle_and_reject_repeats() {
        let (mut global, mut region, id) = setup();
        crate::command::on_command(&mut global, &mut region, id, "save-off", None);
        assert!(!global.save_auto);
        assert_eq!(global.save_requests, vec![SaveCommand::SaveOff]);
        // 再关一次：官方抛 alreadyOff，不产生新请求。
        crate::command::on_command(&mut global, &mut region, id, "save-off", None);
        assert_eq!(global.save_requests.len(), 1);
        crate::command::on_command(&mut global, &mut region, id, "save-on", None);
        assert!(global.save_auto);
        assert_eq!(
            global.save_requests,
            vec![SaveCommand::SaveOff, SaveCommand::SaveOn]
        );
    }

    #[test]
    fn save_all_and_flush_push_requests() {
        let (mut global, mut region, id) = setup();
        crate::command::on_command(&mut global, &mut region, id, "save-all", None);
        crate::command::on_command(&mut global, &mut region, id, "save-all flush", None);
        assert_eq!(
            global.save_requests,
            vec![
                SaveCommand::SaveAll {
                    flush: false,
                    source: id
                },
                SaveCommand::SaveAll {
                    flush: true,
                    source: id
                }
            ]
        );
    }
}
