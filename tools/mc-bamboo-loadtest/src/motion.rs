//! 假玩家的移动轨迹。每 50 ms 给一个位置，速度按「格/秒」换算。

use std::f64::consts::TAU;

use crate::wire::{SPAWN, WANDER_RADIUS};

/// 移动方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    Stand,
    Walk,
    Fly,
    Wander,
}

impl Pattern {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "stand" => Ok(Self::Stand),
            "walk" => Ok(Self::Walk),
            "fly" => Ok(Self::Fly),
            "wander" => Ok(Self::Wander),
            other => Err(format!(
                "unknown pattern '{other}', expected stand|walk|fly|wander"
            )),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Stand => "stand",
            Self::Walk => "walk",
            Self::Fly => "fly",
            Self::Wander => "wander",
        }
    }

    /// 默认速度，格/秒。`fly` 是鞘翅加烟花的速度估计。
    pub const fn default_speed(self) -> f64 {
        match self {
            Self::Stand => 0.0,
            Self::Walk | Self::Wander => 4.3,
            Self::Fly => 33.0,
        }
    }

    /// 是否需要开启创造模式的飞行能力。
    pub const fn needs_flying(self) -> bool {
        matches!(self, Self::Fly)
    }
}

/// 起始布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Cluster,
    Spread,
}

impl Layout {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "cluster" => Ok(Self::Cluster),
            "spread" => Ok(Self::Spread),
            other => Err(format!("unknown layout '{other}', expected cluster|spread")),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Cluster => "cluster",
            Self::Spread => "spread",
        }
    }
}

/// 一名假玩家的位置序列。
#[derive(Debug, Clone)]
pub struct Motion {
    pattern: Pattern,
    speed: f64,
    /// 每一步的水平方向，单位向量。
    direction: (f64, f64),
    /// 出生点附近的随机游走圆心（cluster）或者出发点（spread）。
    center: (f64, f64),
    position: (f64, f64, f64),
    wander_angle: f64,
    rng: u64,
}

impl Motion {
    /// `index` 用来把 spread 的玩家均匀撒到一圈不同的方向。
    pub fn new(pattern: Pattern, layout: Layout, index: usize, bots: usize, speed: f64) -> Self {
        let angle = TAU * index as f64 / bots.max(1) as f64;
        let direction = (angle.cos(), angle.sin());
        let center = match layout {
            Layout::Cluster => (SPAWN.0, SPAWN.2),
            // spread：起点在出生点，方向彼此不同，越走越远。
            Layout::Spread => (SPAWN.0, SPAWN.2),
        };
        Self {
            pattern,
            speed,
            direction,
            center,
            position: SPAWN,
            wander_angle: angle,
            rng: 0x9e37_79b9_7f4a_7c15_u64 ^ (index as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9),
        }
    }

    /// 当前位置。测试用。
    #[cfg(test)]
    pub const fn position(&self) -> (f64, f64, f64) {
        self.position
    }

    /// 收到一次传送之后把轨迹对齐到服务端给的位置，避免被拉回。
    pub fn snap_to(&mut self, position: (f64, f64, f64)) {
        self.position = position;
        self.center = (position.0, position.2);
    }

    /// 前进一个客户端 tick（50 ms），返回新位置。
    pub fn step(&mut self, dt: f64) -> (f64, f64, f64) {
        match self.pattern {
            Pattern::Stand => {}
            Pattern::Walk | Pattern::Fly => {
                let distance = self.speed * dt;
                self.position.0 += self.direction.0 * distance;
                self.position.2 += self.direction.1 * distance;
                // 飞行时保持一个固定的高度，不制造竖直方向的碰撞。
                if self.pattern == Pattern::Fly {
                    self.position.1 = SPAWN.1 + 8.0;
                }
            }
            Pattern::Wander => {
                // 每步轻微改向；超出半径时把方向拉回圆心。
                self.wander_angle += (self.next_f64() - 0.5) * 0.5;
                let dx = self.position.0 - self.center.0;
                let dz = self.position.2 - self.center.1;
                let radius = (dx * dx + dz * dz).sqrt();
                if radius > WANDER_RADIUS {
                    self.wander_angle = (-dz).atan2(-dx) + (self.next_f64() - 0.5) * 0.5;
                }
                let distance = self.speed * dt;
                self.position.0 += self.wander_angle.cos() * distance;
                self.position.2 += self.wander_angle.sin() * distance;
            }
        }
        self.position
    }

    /// 一个不需要额外依赖的确定性伪随机数，只用来挑游走方向。
    fn next_f64(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1_u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stand_does_not_move() {
        let mut motion = Motion::new(Pattern::Stand, Layout::Cluster, 0, 1, 4.3);
        for _ in 0..100 {
            assert_eq!(motion.step(0.05), SPAWN);
        }
    }

    #[test]
    fn one_tick_of_walking_matches_the_requested_speed() {
        let mut motion = Motion::new(Pattern::Walk, Layout::Spread, 0, 1, 4.3);
        let before = motion.position();
        let after = motion.step(0.05);
        let distance = (after.0 - before.0).hypot(after.2 - before.2);
        assert!((distance - 0.215).abs() < 1e-9, "distance {distance}");
    }

    #[test]
    fn one_tick_of_flying_stays_under_the_speed_check() {
        let mut motion = Motion::new(Pattern::Fly, Layout::Spread, 3, 20, 33.0);
        // 第一包要从服务端给的出生点跳到飞行高度，这一步也不超过 10 格。
        let first = motion.step(0.05);
        assert!((first.1 - SPAWN.1 - 8.0).abs() < 1e-9);
        for _ in 0..20 {
            let before = motion.position();
            let after = motion.step(0.05);
            let distance = (after.0 - before.0).hypot(after.2 - before.2);
            assert!((distance - 1.65).abs() < 1e-9, "distance {distance}");
            assert!(distance < 10.0);
        }
    }

    #[test]
    fn spread_uses_different_directions() {
        let mut a = Motion::new(Pattern::Walk, Layout::Spread, 0, 4, 4.3);
        let mut b = Motion::new(Pattern::Walk, Layout::Spread, 2, 4, 4.3);
        a.step(1.0);
        b.step(1.0);
        assert!((a.position().0 - b.position().0).abs() > 4.0);
    }

    #[test]
    fn wander_stays_near_the_spawn_point() {
        let mut motion = Motion::new(Pattern::Wander, Layout::Cluster, 7, 200, 4.3);
        for _ in 0..20_000 {
            let position = motion.step(0.05);
            let distance = (position.0 - SPAWN.0).hypot(position.2 - SPAWN.2);
            assert!(distance < WANDER_RADIUS + 1.0, "wandered {distance}");
        }
    }

    #[test]
    fn snap_to_follows_a_server_teleport() {
        let mut motion = Motion::new(Pattern::Walk, Layout::Spread, 0, 1, 4.3);
        motion.snap_to((100.5, -60.0, -20.5));
        assert_eq!(motion.position(), (100.5, -60.0, -20.5));
        let after = motion.step(0.05);
        assert!(after.0 > 100.5);
    }
}
