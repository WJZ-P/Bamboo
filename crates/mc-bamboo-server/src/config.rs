//! 服务端配置。

use std::time::Duration;

use tracing::warn;

const MIN_VIEW_DISTANCE: u8 = 2;
const MAX_VIEW_DISTANCE: u8 = 32;
/// 客户端这么久收不到任何字节就断开。没有别的流量时，心跳是唯一能让它收到字节的东西。
const CLIENT_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// 服务端运行参数。
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// 服务器列表里显示的介绍文字。
    pub motd: String,
    /// 同时在线的玩家上限。
    pub max_players: u32,
    /// 服务端视距，也是玩家视距的上限。合法范围是 2 到 32，超出时启动时会被修正。
    ///
    /// 这是客户端的渲染半径。实际发送的区块沿坐标轴多一圈，四角是圆的，
    /// 这样渲染范围里每个区块的邻居都在客户端手里。视距 8 时是 329 个区块。
    pub view_distance: u8,
    /// 连接进入游戏之前可以停留的总时长。
    pub pre_play_timeout: Duration,
    /// 同时处于进入游戏之前阶段的连接上限，超出的连接会被直接丢弃。
    pub max_pre_play_connections: usize,
    /// 发送心跳的间隔。上一次心跳到下一次发送时还没有回应，玩家就会被断开。
    ///
    /// 必须大于 0 且小于 30 秒，否则客户端会先超时，启动时会换成默认值。
    pub keep_alive_interval: Duration,
    /// 登录之后启用压缩的阈值，单位是包负载的字节数，和官方的 `network-compression-threshold` 相同。
    ///
    /// 小于 0 表示关闭压缩，0 表示每个包都压缩。默认 256。
    pub compression_threshold: i32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            motd: "A Bamboo Server".to_owned(),
            max_players: 20,
            view_distance: 8,
            pre_play_timeout: Duration::from_secs(30),
            max_pre_play_connections: 1024,
            keep_alive_interval: Duration::from_secs(15),
            compression_threshold: 256,
        }
    }
}

impl ServerConfig {
    /// 把配置修正到能运行的范围。世界线程和 `login` 包都用修正后的值。
    pub(crate) fn normalized(mut self) -> Self {
        self.view_distance = self
            .view_distance
            .clamp(MIN_VIEW_DISTANCE, MAX_VIEW_DISTANCE);

        let interval = self.keep_alive_interval;
        if interval.is_zero() || interval >= CLIENT_READ_TIMEOUT {
            let default = Self::default().keep_alive_interval;
            warn!(
                configured = ?interval,
                used = ?default,
                "keep-alive interval must be above zero and below {CLIENT_READ_TIMEOUT:?}"
            );
            self.keep_alive_interval = default;
        }

        // 所有负数都表示关闭压缩，统一成 -1，后面只比较这一个值。
        self.compression_threshold = self.compression_threshold.max(-1);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_view_distance(view_distance: u8) -> u8 {
        let config = ServerConfig {
            view_distance,
            ..ServerConfig::default()
        };
        config.normalized().view_distance
    }

    fn with_keep_alive(keep_alive_interval: Duration) -> Duration {
        let config = ServerConfig {
            keep_alive_interval,
            ..ServerConfig::default()
        };
        config.normalized().keep_alive_interval
    }

    fn with_compression(compression_threshold: i32) -> i32 {
        let config = ServerConfig {
            compression_threshold,
            ..ServerConfig::default()
        };
        config.normalized().compression_threshold
    }

    #[test]
    fn view_distance_is_clamped_like_the_official_server() {
        assert_eq!(with_view_distance(0), 2);
        assert_eq!(with_view_distance(1), 2);
        assert_eq!(with_view_distance(2), 2);
        assert_eq!(with_view_distance(32), 32);
        assert_eq!(with_view_distance(33), 32);
        assert_eq!(with_view_distance(u8::MAX), 32);
    }

    #[test]
    fn keep_alive_must_leave_room_before_the_client_times_out() {
        let default = ServerConfig::default().keep_alive_interval;

        assert_eq!(
            with_keep_alive(Duration::from_millis(100)),
            Duration::from_millis(100)
        );
        assert_eq!(
            with_keep_alive(Duration::from_millis(29_999)),
            Duration::from_millis(29_999)
        );
        assert_eq!(with_keep_alive(Duration::from_secs(30)), default);
        assert_eq!(with_keep_alive(Duration::from_secs(3600)), default);
    }

    #[test]
    fn keep_alive_cannot_be_zero() {
        // 周期为 0 时 tokio 的 interval 会 panic。
        assert_eq!(
            with_keep_alive(Duration::ZERO),
            ServerConfig::default().keep_alive_interval
        );
    }

    #[test]
    fn the_default_compression_threshold_matches_the_official_server() {
        assert_eq!(ServerConfig::default().compression_threshold, 256);
    }

    #[test]
    fn negative_compression_thresholds_all_mean_off() {
        // 官方只看阈值是不是非负，这里统一成 -1 方便比较。
        assert_eq!(with_compression(i32::MIN), -1);
        assert_eq!(with_compression(-256), -1);
        assert_eq!(with_compression(-1), -1);
        // 0 是合法的：每个包都压缩。
        assert_eq!(with_compression(0), 0);
        assert_eq!(with_compression(256), 256);
        assert_eq!(with_compression(i32::MAX), i32::MAX);
    }
}
