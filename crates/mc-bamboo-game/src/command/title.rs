//! `/title`：清屏、重置、标题、副标题、动作栏和显示时间。
//!
//! 官方 `TitleCommand` 对每个分支发一个包给所有目标，然后按「只有一个目标 / 多个目标」
//! 给命令来源发一条成功消息并广播给其他 OP。`times` 的三个参数都是 `TimeArgument`。
//!
//! `/title`. Each branch sends one packet to every target, then reports success to
//! the source (single or multiple wording) and broadcasts it to other operators.

use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_protocol::{
    packet::play::{
        ClearTitles, SetActionBarText, SetSubtitleText, SetTitleText, SetTitlesAnimation,
    },
    text::Component,
};

use crate::{
    command::{
        feedback::{self, CommandFail, fail},
        select, text,
    },
    global::Global,
    inbound::PlayerId,
    region::Region,
};

/// 一条分支的反馈翻译键。官方 `CommandResponseTracker.Messages` 各有一对。
#[derive(Debug, Clone, Copy)]
struct MessageKeys {
    single: &'static str,
    multiple: &'static str,
}

const CLEAR: MessageKeys = MessageKeys {
    single: "commands.title.cleared.single",
    multiple: "commands.title.cleared.multiple",
};
const RESET: MessageKeys = MessageKeys {
    single: "commands.title.reset.single",
    multiple: "commands.title.reset.multiple",
};
const SHOW_TITLE: MessageKeys = MessageKeys {
    single: "commands.title.show.title.single",
    multiple: "commands.title.show.title.multiple",
};
const SHOW_SUBTITLE: MessageKeys = MessageKeys {
    single: "commands.title.show.subtitle.single",
    multiple: "commands.title.show.subtitle.multiple",
};
const SHOW_ACTIONBAR: MessageKeys = MessageKeys {
    single: "commands.title.show.actionbar.single",
    multiple: "commands.title.show.actionbar.multiple",
};
const TIMES: MessageKeys = MessageKeys {
    single: "commands.title.times.single",
    multiple: "commands.title.times.multiple",
};

/// 命令树里写死的分支名，执行时靠 `parsed.nodes` 里的字面量节点认出来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Clear,
    Reset,
    Title,
    Subtitle,
    Actionbar,
    Times,
}

pub(crate) fn run(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
    action: Action,
) -> Result<(), CommandFail> {
    let selector = parsed
        .get("targets")
        .and_then(Value::as_entity)
        .ok_or_else(|| fail("command.failed"))?;
    let targets = select::players(region, id, text::level_of(region, id), selector)?;
    let keys = match action {
        Action::Clear => {
            let packet = ClearTitles { reset_times: false };
            for target in &targets {
                region.links.send(*target, &packet);
            }
            CLEAR
        }
        Action::Reset => {
            let packet = ClearTitles { reset_times: true };
            for target in &targets {
                region.links.send(*target, &packet);
            }
            RESET
        }
        Action::Times => {
            let packet = SetTitlesAnimation {
                fade_in: required_time(parsed, "fadeIn")?,
                stay: required_time(parsed, "stay")?,
                fade_out: required_time(parsed, "fadeOut")?,
            };
            for target in &targets {
                region.links.send(*target, &packet);
            }
            TIMES
        }
        Action::Title | Action::Subtitle | Action::Actionbar => {
            // 官方 `showTitle` 对每个目标重新 `ComponentUtils.resolve` 一次。
            let raw = text::component(parsed, "title")?;
            let keys = match action {
                Action::Title => SHOW_TITLE,
                Action::Subtitle => SHOW_SUBTITLE,
                _ => SHOW_ACTIONBAR,
            };
            for target in &targets {
                let uuid = region
                    .players
                    .get(target)
                    .map(|player| player.uuid)
                    .unwrap_or_default();
                let resolved = text::resolve(global, region, id, *target, uuid, raw.tag(), 0);
                match action {
                    Action::Title => region
                        .links
                        .send(*target, &SetTitleText { text: &resolved }),
                    Action::Subtitle => region
                        .links
                        .send(*target, &SetSubtitleText { text: &resolved }),
                    _ => region
                        .links
                        .send(*target, &SetActionBarText { text: &resolved }),
                }
            }
            keys
        }
    };
    report(region, id, &targets, keys);
    Ok(())
}

fn report(region: &mut Region, source: PlayerId, targets: &[PlayerId], keys: MessageKeys) {
    let message = if let [only] = targets {
        let name = region
            .players
            .get(only)
            .map(|player| player.name.clone())
            .unwrap_or_default();
        Component::translate(keys.single, [Component::text(name).into()])
    } else {
        Component::translate(keys.multiple, [(targets.len() as i32).into()])
    };
    feedback::success(region, source, message, true);
}

fn required_time(parsed: &ParsedCommand, name: &str) -> Result<i32, CommandFail> {
    parsed
        .get(name)
        .and_then(Value::as_time)
        .ok_or_else(|| fail("command.failed"))
}

/// 每个可执行节点对应一个函数。命令树里分支名写死，这里只转发动作。
macro_rules! action {
    ($name:ident, $action:expr) => {
        pub(crate) fn $name(
            global: &mut Global,
            region: &mut Region,
            id: PlayerId,
            parsed: &ParsedCommand,
        ) -> Result<(), CommandFail> {
            run(global, region, id, parsed, $action)
        }
    };
}

action!(clear, Action::Clear);
action!(reset, Action::Reset);
action!(show_title, Action::Title);
action!(show_subtitle, Action::Subtitle);
action!(show_actionbar, Action::Actionbar);
action!(run_times, Action::Times);
