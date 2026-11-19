//! 饥饿的其余消耗来源：移动、疾跑、跳跃、游泳、攻击、挖掘。
//!
//! 官方 `ServerPlayer.checkMovementStatistics` / `jumpFromGround` /
//! `Player.attack` / `ServerPlayerGameMode` 的破坏成功路径。
//! 全是纯函数：调用方按官方条件决定是否调用、把结果加进
//! `FoodData.exhaustion`（`Player.causeFoodExhaustion` 在创造模式跳过）。
//!
//! The remaining hunger-exhaustion sources. Pure functions; the caller
//! applies them to `FoodData.exhaustion` and skips creative mode.

/// `FoodData.addExhaustion` 的上限。
pub const MAX_EXHAUSTION: f32 = 40.0;

/// `Player.causeFoodExhaustion`：创造模式跳过，写入 `pending_exhaustion`
/// 由 [`flush`] 在 tick 末尾落到 `FoodData`。
pub(crate) fn add(player: &mut crate::player::Player, amount: f32) {
    if player.game_mode != mc_bamboo_protocol::packet::play::GameMode::Creative {
        player.pending_exhaustion += amount;
    }
}

/// `&Player` 借用下的等价入口（攻击路径）。
pub(crate) fn add_in(
    region: &mut crate::region::Region,
    id: crate::inbound::PlayerId,
    amount: f32,
) {
    if let Some(player) = region.players.get_mut(&id) {
        add(player, amount);
    }
}

/// 把这一刻累计的消耗写进 `FoodData.exhaustion`（夹到 40）。
pub(crate) fn flush(region: &mut crate::region::Region) {
    for player in region.players.values_mut() {
        let amount = std::mem::take(&mut player.pending_exhaustion);
        if amount != 0.0 {
            player.health.food.exhaustion =
                (player.health.food.exhaustion + amount).min(MAX_EXHAUSTION);
        }
    }
}

/// `didNotMove(dx, dy, dz)`：三个方向都小于 `1e-7` 的平方阈值时整段跳过。
/// 官方用 `dx*dx + dy*dy + dz*dz < 9.0E-8`。
fn did_not_move(x: f64, y: f64, z: f64) -> bool {
    x * x + y * y + z * z < 9.0e-8
}

/// 官方 `Math.round(dist * 100.0f)`：Java 的 round 是 `floor(x + 0.5)`。
fn cm(dx: f64, dy: f64, dz: f64) -> i32 {
    let dist = (dx * dx + dy * dy + dz * dz).sqrt() as f32 * 100.0;
    (dist + 0.5).floor() as i32
}

/// `ServerPlayer.checkMovementStatistics(dx, dy, dz)` 的消耗量。
/// 返回 `None` 表示这段不消耗；`Some(amount)` 交给 [`add`]。
///
/// The exhaustion amount from `checkMovementStatistics`; `None` when the
/// movement check takes no exhaustion branch.
#[allow(
    clippy::too_many_arguments,
    reason = "the official movement check branches on all of these movement states"
)]
pub fn movement(
    dx: f64,
    dy: f64,
    dz: f64,
    swimming: bool,
    eye_in_water: bool,
    in_water: bool,
    on_climbable: bool,
    on_ground: bool,
    sprinting: bool,
    fall_flying: bool,
) -> Option<f32> {
    if did_not_move(dx, dy, dz) {
        return None;
    }
    if swimming || eye_in_water {
        let centimeters = cm(dx, dy, dz);
        return (centimeters > 0).then_some(0.01 * centimeters as f32 * 0.01);
    }
    if in_water {
        let centimeters = cm(dx, 0.0, dz);
        return (centimeters > 0).then_some(0.01 * centimeters as f32 * 0.01);
    }
    if on_climbable {
        // 官方只发统计，不加饥饿消耗。
        return (dy > 0.0).then_some(0.0);
    }
    if on_ground {
        let centimeters = cm(dx, 0.0, dz);
        if centimeters <= 0 {
            return None;
        }
        // 疾跑 `0.1 * cm * 0.01`；走路 / 潜行是 `0.0 * cm * 0.01`。
        let factor = if sprinting { 0.1 } else { 0.0 };
        return Some(factor * centimeters as f32 * 0.01);
    }
    if fall_flying {
        let centimeters = cm(dx, dy, dz);
        return Some(0.0 * centimeters as f32 * 0.01);
    }
    None
}

/// `ServerPlayer.jumpFromGround`：疾跑 0.2，否则 0.05。
pub const fn jump(sprinting: bool) -> f32 {
    if sprinting { 0.2 } else { 0.05 }
}

/// `Player.attack` 命中实体：`causeFoodExhaustion(0.1)`。
pub const ATTACK: f32 = 0.1;

/// `ServerPlayerGameMode` 破坏方块成功：`causeFoodExhaustion(0.005)`。
pub const MINE: f32 = 0.005;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn still_players_take_no_exhaustion() {
        assert_eq!(
            movement(0.0, 0.0, 0.0, false, false, false, false, true, true, false),
            None
        );
        assert_eq!(
            movement(
                1.0e-9, 0.0, 0.0, false, false, false, false, true, true, false
            ),
            None
        );
    }

    #[test]
    fn sprint_walk_swim_and_jump_match_the_official_coefficients() {
        // 疾跑 1 格 = 100 cm -> 0.1 * 100 * 0.01 = 0.1
        let sprint = movement(1.0, 0.0, 0.0, false, false, false, false, true, true, false);
        assert!(sprint.is_some_and(|value| (value - 0.1).abs() < 1.0e-6));
        // 走路不消耗。
        assert_eq!(
            movement(
                1.0, 0.0, 0.0, false, false, false, false, true, false, false
            ),
            Some(0.0)
        );
        // 游泳 1 格 = 0.01 * 100 * 0.01 = 0.01
        assert_eq!(
            movement(
                1.0, 0.0, 0.0, true, false, false, false, false, false, false
            ),
            Some(0.01)
        );
        // 改坏确认：疾跑系数写成走路会变成 0。
        assert_ne!(
            movement(1.0, 0.0, 0.0, false, false, false, false, true, true, false),
            Some(0.0)
        );
        assert_eq!(jump(true), 0.2);
        assert_eq!(jump(false), 0.05);
        assert_eq!(ATTACK, 0.1);
        assert_eq!(MINE, 0.005);
    }
}
