//! `/worldborder`。
//!
//! 语法、权限、反馈和失败消息照官方 `WorldBorderCommand`：整棵树要等级 2，
//! `get` 只回给执行者（不广播），其余成功消息广播给其他 OP。改边界时按
//! `PlayerList.addWorldborderListener` 的监听器发对应的包。
//!
//! `/worldborder`.
//!
//! Grammar, permission, success and failure text follow vanilla `WorldBorderCommand`:
//! the whole tree requires level 2, `get` replies only to the caller (no broadcast),
//! and every other success is broadcast to the other ops. Border changes emit the
//! packets from `PlayerList.addWorldborderListener`.

use mc_bamboo_blocks::{BorderEvent, MAX_CENTER_COORDINATE, MAX_SIZE};
use mc_bamboo_command::{ParsedCommand, Value};
use mc_bamboo_core::Vec3;
use mc_bamboo_protocol::{
    packet::play::{
        SetBorderCenter, SetBorderLerpSize, SetBorderSize, SetBorderWarningDelay,
        SetBorderWarningDistance,
    },
    text::Component,
};

use crate::{
    command::feedback::{self, CommandFail, fail, fail_with},
    global::Global,
    inbound::PlayerId,
    links::encode_frames,
    region::Region,
};

/// 官方失败消息里的 `5.999997E7F`，是 float 而不是 double。
///
/// The `5.999997E7F` in vanilla's failure message: a float, not a double.
const MAX_SIZE_FLOAT: f32 = 59_999_968.0;

pub(crate) fn add(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let distance = double_arg(parsed, "distance")?;
    let target = global.world_border().size() + distance;
    set_size(global, region, id, target, 0)
}

pub(crate) fn add_time(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let distance = double_arg(parsed, "distance")?;
    let time = time_arg(parsed, "time")?;
    let target = global.world_border().size() + distance;
    let ticks = global.world_border().lerp_time() + i64::from(time);
    set_size(global, region, id, target, ticks)
}

pub(crate) fn set(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let distance = double_arg(parsed, "distance")?;
    set_size(global, region, id, distance, 0)
}

pub(crate) fn set_time(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let distance = double_arg(parsed, "distance")?;
    let ticks = i64::from(time_arg(parsed, "time")?);
    set_size(global, region, id, distance, ticks)
}

/// 官方 `WorldBorderCommand.setSize`。
fn set_size(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    distance: f64,
    ticks: i64,
) -> Result<(), CommandFail> {
    let current = global.world_border().size();
    if current == distance {
        return Err(fail("commands.worldborder.set.failed.nochange"));
    }
    if distance < 1.0 {
        return Err(fail("commands.worldborder.set.failed.small"));
    }
    if distance > MAX_SIZE {
        return Err(fail_with(
            "commands.worldborder.set.failed.big",
            [MAX_SIZE_FLOAT.into()],
        ));
    }
    let formatted = format_fixed(distance, 1);
    if ticks > 0 {
        let game_time = global.time().0;
        let event = global
            .world_border_mut()
            .lerp_size_between(current, distance, ticks, game_time);
        apply(global, region, event);
        let seconds = format_seconds(ticks);
        let key = if distance > current {
            "commands.worldborder.set.grow"
        } else {
            "commands.worldborder.set.shrink"
        };
        feedback::success(
            region,
            id,
            Component::translate(key, [formatted.into(), seconds.into()]),
            true,
        );
    } else {
        let event = global.world_border_mut().set_size(distance);
        apply(global, region, event);
        feedback::success(
            region,
            id,
            Component::translate("commands.worldborder.set.immediate", [formatted.into()]),
            true,
        );
    }
    Ok(())
}

pub(crate) fn center(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let Some([x, z]) = parsed.get("pos").and_then(Value::as_vec2) else {
        return Err(fail("command.failed"));
    };
    let source = source_position(region, id)?;
    // 官方 `Vec2Argument.getVec2` 把两个 double 收成 float，再当 double 用。
    //
    // Vanilla `Vec2Argument.getVec2` narrows both doubles to floats, then widens them.
    let (x, z) = (
        f64::from(x.get(source.x) as f32),
        f64::from(z.get(source.z) as f32),
    );
    let border = global.world_border();
    if border.center_x() == x && border.center_z() == z {
        return Err(fail("commands.worldborder.center.failed"));
    }
    if x.abs() > MAX_CENTER_COORDINATE || z.abs() > MAX_CENTER_COORDINATE {
        return Err(fail_with(
            "commands.worldborder.set.failed.far",
            [MAX_CENTER_COORDINATE.into()],
        ));
    }
    let event = global.world_border_mut().set_center(x, z);
    apply(global, region, event);
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.worldborder.center.success",
            [format_fixed(x, 2).into(), format_fixed(z, 2).into()],
        ),
        true,
    );
    Ok(())
}

pub(crate) fn damage_amount(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let amount = float_arg(parsed, "damagePerBlock")?;
    if global.world_border().damage_per_block() == f64::from(amount) {
        return Err(fail("commands.worldborder.damage.amount.failed"));
    }
    let event = global
        .world_border_mut()
        .set_damage_per_block(f64::from(amount));
    apply(global, region, event);
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.worldborder.damage.amount.success",
            [format_fixed(f64::from(amount), 2).into()],
        ),
        true,
    );
    Ok(())
}

pub(crate) fn damage_buffer(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let distance = float_arg(parsed, "distance")?;
    if global.world_border().safe_zone() == f64::from(distance) {
        return Err(fail("commands.worldborder.damage.buffer.failed"));
    }
    let event = global.world_border_mut().set_safe_zone(f64::from(distance));
    apply(global, region, event);
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.worldborder.damage.buffer.success",
            [format_fixed(f64::from(distance), 2).into()],
        ),
        true,
    );
    Ok(())
}

pub(crate) fn get(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    _parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let size = global.world_border().size();
    feedback::success(
        region,
        id,
        Component::translate("commands.worldborder.get", [format_fixed(size, 0).into()]),
        false,
    );
    Ok(())
}

pub(crate) fn warning_distance(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let distance = int_arg(parsed, "distance")?;
    if global.world_border().warning_blocks() == distance {
        return Err(fail("commands.worldborder.warning.distance.failed"));
    }
    let event = global.world_border_mut().set_warning_blocks(distance);
    apply(global, region, event);
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.worldborder.warning.distance.success",
            [distance.into()],
        ),
        true,
    );
    Ok(())
}

pub(crate) fn warning_time(
    global: &mut Global,
    region: &mut Region,
    id: PlayerId,
    parsed: &ParsedCommand,
) -> Result<(), CommandFail> {
    let ticks = time_arg(parsed, "time")?;
    if global.world_border().warning_time() == ticks {
        return Err(fail("commands.worldborder.warning.time.failed"));
    }
    let event = global.world_border_mut().set_warning_time(ticks);
    apply(global, region, event);
    feedback::success(
        region,
        id,
        Component::translate(
            "commands.worldborder.warning.time.success",
            [format_seconds(i64::from(ticks)).into()],
        ),
        true,
    );
    Ok(())
}

/// 官方 `formatTicksToSeconds`：`String.format(Locale.ROOT, "%.2f", ticks / 20.0)`。
///
/// Vanilla `formatTicksToSeconds`: `String.format(Locale.ROOT, "%.2f", ticks / 20.0)`.
fn format_seconds(ticks: i64) -> String {
    format_fixed(ticks as f64 / 20.0, 2)
}

/// 官方 `String.format(Locale.ROOT, "%.Nf", value)`。
///
/// Rust 的 `{:.N}` 是 ties-to-even，Java 的 `Formatter` 是 HALF_UP，而且舍入的是
/// double 的最短十进制表示：例如 `2.675` 的二进制值略小于 2.675，但最短表示是
/// `"2.675"`，Java 给 `"2.68"`，Rust 给 `"2.67"`；`1.25` 的 `%.1f` 官方是 `"1.3"`。
/// 这里照 Java 的做法把最短十进制字符串按 HALF_UP 舍入。
///
/// Vanilla `String.format(Locale.ROOT, "%.Nf", value)`. Rust's `{:.N}` rounds
/// ties to even, while Java's `Formatter` uses HALF_UP on the double's shortest
/// decimal representation: `2.675` prints `"2.68"` in Java but `"2.67"` in Rust,
/// and `1.25` with `%.1f` is `"1.3"`. Round the shortest decimal string HALF_UP.
fn format_fixed(value: f64, digits: usize) -> String {
    let text = format!("{value}");
    let (negative, unsigned) = text
        .strip_prefix('-')
        .map_or((false, text.as_str()), |rest| (true, rest));
    let (int_part, frac_part) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let mut fraction: Vec<u8> = frac_part.bytes().collect();
    while fraction.len() <= digits {
        fraction.push(b'0');
    }
    let round_up = fraction[digits] >= b'5';
    let mut all: Vec<u8> = int_part
        .bytes()
        .chain(fraction[..digits].iter().copied())
        .collect();
    if round_up {
        let mut carry = true;
        for digit in all.iter_mut().rev() {
            if *digit == b'9' {
                *digit = b'0';
            } else {
                *digit += 1;
                carry = false;
                break;
            }
        }
        if carry {
            all.insert(0, b'1');
        }
    }
    let split = all.len() - digits;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(std::str::from_utf8(&all[..split]).expect("ASCII digits"));
    if digits > 0 {
        out.push('.');
        out.push_str(std::str::from_utf8(&all[split..]).expect("ASCII digits"));
    }
    out
}

/// 改完边界：发布新快照，再照 `BorderChangeListener` 广播对应的包。
///
/// After a border change: publish the snapshot, then broadcast the packet that
/// `BorderChangeListener` would send.
fn apply(global: &mut Global, region: &mut Region, event: BorderEvent) {
    global.publish_border(region);
    let border = global.world_border();
    let frames = match event {
        BorderEvent::SetSize { .. } => encode_frames(&SetBorderSize {
            size: border.lerp_target(),
        }),
        BorderEvent::LerpSize { .. } => encode_frames(&SetBorderLerpSize {
            old_size: border.size(),
            new_size: border.lerp_target(),
            lerp_time: border.lerp_time(),
        }),
        BorderEvent::SetCenter { .. } => encode_frames(&SetBorderCenter {
            center_x: border.center_x(),
            center_z: border.center_z(),
        }),
        BorderEvent::SetWarningTime { .. } => encode_frames(&SetBorderWarningDelay {
            warning_delay: border.warning_time(),
        }),
        BorderEvent::SetWarningBlocks { .. } => encode_frames(&SetBorderWarningDistance {
            warning_blocks: border.warning_blocks(),
        }),
        // 官方这两个监听器什么都不做。
        //
        // Vanilla's listeners for these two do nothing.
        BorderEvent::SetDamagePerBlock { .. } | BorderEvent::SetSafeZone { .. } => return,
    };
    region.links.broadcast(&frames);
}

/// 执行者的脚底，`Vec2Argument.getVec2` 解相对坐标时要它。
///
/// The caller's feet, used by `Vec2Argument.getVec2` to resolve relative coordinates.
fn source_position(region: &Region, id: PlayerId) -> Result<Vec3, CommandFail> {
    region
        .players
        .get(&id)
        .map(|player| player.position())
        .ok_or_else(|| fail("permissions.requires.player"))
}

fn double_arg(parsed: &ParsedCommand, name: &str) -> Result<f64, CommandFail> {
    parsed
        .get(name)
        .and_then(Value::as_double)
        .ok_or_else(|| fail("command.failed"))
}

fn float_arg(parsed: &ParsedCommand, name: &str) -> Result<f32, CommandFail> {
    parsed
        .get(name)
        .and_then(Value::as_float)
        .ok_or_else(|| fail("command.failed"))
}

fn int_arg(parsed: &ParsedCommand, name: &str) -> Result<i32, CommandFail> {
    parsed
        .get(name)
        .and_then(Value::as_integer)
        .ok_or_else(|| fail("command.failed"))
}

fn time_arg(parsed: &ParsedCommand, name: &str) -> Result<i32, CommandFail> {
    parsed
        .get(name)
        .and_then(Value::as_time)
        .ok_or_else(|| fail("command.failed"))
}

#[cfg(test)]
mod tests {
    use mc_bamboo_protocol::packet::play::{ClientInformation, GameMode};
    use uuid::Uuid;

    use crate::{command, global::Global, inbound::PlayerId, player::Player, region::Region};

    use super::*;

    fn world() -> (Global, Region, PlayerId) {
        let global = Global::default();
        let mut region = Region::new(8, 20, false);
        let id = PlayerId(1);
        let mut player = Player::new(
            id,
            Uuid::nil(),
            "Op".to_owned(),
            GameMode::Creative,
            ClientInformation::default(),
            Vec3::new(8.5, 64.0, 8.5),
        );
        player.permission_level = 2;
        player.is_op = true;
        region.insert_player(player);
        global.publish_border(&mut region);
        (global, region, id)
    }

    fn run(global: &mut Global, region: &mut Region, id: PlayerId, command: &str) {
        command::on_command(global, region, id, command, None);
    }

    #[test]
    fn set_and_center_change_the_border_and_publish_the_snapshot() {
        let (mut global, mut region, id) = world();
        run(&mut global, &mut region, id, "worldborder set 100");
        assert_eq!(global.world_border().size(), 100.0);
        assert_eq!(region.world_border.size(), 100.0);
        run(
            &mut global,
            &mut region,
            id,
            "worldborder center 10.5 -20.5",
        );
        assert_eq!(global.world_border().center_x(), 10.5);
        assert_eq!(global.world_border().center_z(), -20.5);
        assert_eq!(region.world_border.center_x(), 10.5);
    }

    #[test]
    fn add_and_set_with_time_start_a_lerp() {
        let (mut global, mut region, id) = world();
        run(&mut global, &mut region, id, "worldborder set 100");
        run(&mut global, &mut region, id, "worldborder add 50 20s");
        assert_eq!(global.world_border().size(), 100.0);
        assert_eq!(global.world_border().lerp_target(), 150.0);
        // `add` 的时间是「剩余渐变刻 + 新时间」。
        //
        // `add` adds the new time to the remaining lerp ticks.
        assert_eq!(global.world_border().lerp_time(), 400);
        run(&mut global, &mut region, id, "worldborder set 20 5s");
        // `set` 从当前尺寸（渐变还没推进，仍是 100）开始新的渐变。
        //
        // `set` starts the new lerp from the current size (still 100; the lerp has
        // not ticked yet).
        assert_eq!(global.world_border().size(), 100.0);
        assert_eq!(global.world_border().lerp_target(), 20.0);
        assert_eq!(global.world_border().lerp_time(), 100);
    }

    #[test]
    fn damage_and_warning_subcommands_update_the_rest_of_the_state() {
        let (mut global, mut region, id) = world();
        run(&mut global, &mut region, id, "worldborder damage amount 2");
        assert_eq!(global.world_border().damage_per_block(), 2.0);
        run(&mut global, &mut region, id, "worldborder damage buffer 9");
        assert_eq!(global.world_border().safe_zone(), 9.0);
        run(&mut global, &mut region, id, "worldborder warning time 10s");
        assert_eq!(global.world_border().warning_time(), 200);
        run(
            &mut global,
            &mut region,
            id,
            "worldborder warning distance 12",
        );
        assert_eq!(global.world_border().warning_blocks(), 12);
        run(&mut global, &mut region, id, "worldborder get");
    }

    /// 和 JVM 里 `String.format(Locale.ROOT, "%.Nf", value)` 的结果逐字对照过。
    #[test]
    fn fixed_format_matches_java_half_up() {
        let cases: &[(f64, usize, &str)] = &[
            (1.25, 1, "1.3"),
            (1.35, 1, "1.4"),
            (2.675, 2, "2.68"),
            (0.125, 2, "0.13"),
            (100.05, 1, "100.1"),
            (1_000_000.0, 1, "1000000.0"),
            (9.99, 1, "10.0"),
            (-0.0, 2, "-0.00"),
            (59_999_968.0, 0, "59999968"),
            (59_999_968.5, 0, "59999969"),
            (1e-7, 2, "0.00"),
        ];
        for &(value, digits, expected) in cases {
            assert_eq!(format_fixed(value, digits), expected, "{value} %.{digits}f");
        }
        assert_eq!(format_seconds(100), "5.00");
    }

    #[test]
    fn no_change_and_out_of_range_are_rejected() {
        let (mut global, mut region, id) = world();
        run(&mut global, &mut region, id, "worldborder set 59999968");
        assert_eq!(global.world_border().size(), MAX_SIZE);
        run(&mut global, &mut region, id, "worldborder set 0");
        assert_eq!(global.world_border().size(), MAX_SIZE);
        run(&mut global, &mut region, id, "worldborder set 59999969");
        assert_eq!(global.world_border().size(), MAX_SIZE);
        // 官方把 float 参数提升成 double 再存，所以 `0.2` 不是「没变化」：
        // 它变成 0.20000000298023224。同一个 float 再写一次才是没变化。
        //
        // Vanilla widens the float argument to a double before storing it, so `0.2`
        // is not "unchanged": it becomes 0.20000000298023224. Only repeating the
        // same float counts as unchanged.
        run(&mut global, &mut region, id, "worldborder damage amount 2");
        assert_eq!(global.world_border().damage_per_block(), 2.0);
        run(&mut global, &mut region, id, "worldborder damage amount 2");
        assert_eq!(global.world_border().damage_per_block(), 2.0);
        run(
            &mut global,
            &mut region,
            id,
            "worldborder center 30000000 0",
        );
        assert_eq!(global.world_border().center_x(), 0.0);
    }
}
