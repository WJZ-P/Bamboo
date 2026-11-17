//! 服务器控制台输入线程。
//!
//! 官方 `DedicatedServer.initServer` 起一条 "Server console handler" 守护线程，
//! 用 `BufferedReader` 读 `System.in`，每行 `handleConsoleInput` 排队，主 tick 线程
//! `handleConsoleInputs` 再执行。Bamboo 照做：连接层这里读 stdin，把每行作为
//! [`Inbound::ConsoleCommand`] 交给世界线程；世界线程从不读 stdin。
//!
//! The dedicated-server console input thread. The official `DedicatedServer.initServer`
//! starts a daemon "Server console handler" that reads `System.in` and queues each line;
//! the main tick thread runs them. Bamboo reads stdin in the connection layer and sends
//! each line to the world thread as [`Inbound::ConsoleCommand`]; the world thread never
//! reads stdin.

use std::io;

use mc_bamboo_game::{Inbound, InboundSender};
use tracing::warn;

/// 起一条读 stdin 的守护线程。读到 EOF、世界线程结束或出错时自行退出，永不 join。
/// 官方的控制台线程同样是 daemon，进程退出时随主线程一起结束。
///
/// Start a daemon thread reading stdin. It exits on EOF, when the world thread is gone,
/// or on error, and is never joined. The official console thread is likewise a daemon
/// that ends with the process.
pub(crate) fn spawn(inbox: InboundSender) {
    if let Err(error) = std::thread::Builder::new()
        .name("bamboo-console".to_owned())
        .spawn(move || run(inbox))
    {
        warn!(%error, "could not start the console input thread");
    }
}

fn run(inbox: InboundSender) {
    let stdin = io::stdin();
    loop {
        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => return,
            Ok(_) => {
                if !handle_line(&inbox, &line) {
                    return;
                }
            }
            Err(error) => {
                warn!(%error, "failed to read console input");
                return;
            }
        }
    }
}

/// 把一行控制台输入变成一条世界线程消息；返回 `false` 表示世界线程已结束。
/// 空行跳过，行尾的 `\r` / `\n` 去掉（官方 `BufferedReader.readLine` 也不带换行）。
///
/// Turn one console line into a world-thread message; `false` means the world thread
/// has ended. Empty lines are skipped and trailing `\r` / `\n` are removed (official
/// `BufferedReader.readLine` also strips the terminator).
fn handle_line(inbox: &InboundSender, line: &str) -> bool {
    let command = line.trim_end_matches(['\r', '\n']);
    if command.is_empty() {
        return true;
    }
    inbox
        .send(Inbound::ConsoleCommand(command.to_owned()))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_line_becomes_a_console_message() {
        let (sender, receiver) = mc_bamboo_game::inbound_channel();
        assert!(handle_line(&sender, "stop\n"));
        assert!(matches!(
            receiver.recv().unwrap().message,
            Inbound::ConsoleCommand(command) if command == "stop"
        ));
    }

    #[test]
    fn crlf_and_blank_lines_are_handled_like_readline() {
        let (sender, receiver) = mc_bamboo_game::inbound_channel();
        assert!(handle_line(&sender, "\r\n"));
        assert!(handle_line(&sender, "  \n"));
        assert!(handle_line(&sender, "say hi\r\n"));
        assert!(matches!(
            receiver.recv().unwrap().message,
            Inbound::ConsoleCommand(command) if command == "  "
        ));
        assert!(matches!(
            receiver.recv().unwrap().message,
            Inbound::ConsoleCommand(command) if command == "say hi"
        ));
        // 只有上面两条消息，没有空行产生的。
        // Only the two messages above; blank lines produce nothing.
        assert!(receiver.try_recv().is_none());
    }

    #[test]
    fn a_closed_world_inbox_stops_the_reader() {
        let (sender, receiver) = mc_bamboo_game::inbound_channel();
        drop(receiver);
        assert!(!handle_line(&sender, "stop\n"));
    }
}
