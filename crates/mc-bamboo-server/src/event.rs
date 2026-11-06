//! 服务端对外广播的事件。

/// 一名在线玩家的公开信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerInfo {
    pub name: String,
    /// 带连字符的 UUID 字符串。
    pub uuid: String,
}

/// 服务端运行中发生的事件，界面可以订阅它们来刷新显示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerEvent {
    PlayerJoined(PlayerInfo),
    PlayerLeft(PlayerInfo),
}
