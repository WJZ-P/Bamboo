//! 数字、字符串、时间、游戏模式这些简单参数。

use crate::{ErrorArg, ParseError, Value, reader::StringReader, value::GameMode};

/// 官方 `StringArgumentType.StringType`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringKind {
    /// 一个单词，遇到空格或不允许的字符就停。
    SingleWord,
    /// 可以带引号。
    Quotable,
    /// 吞掉剩下的全部内容。
    Greedy,
}

pub fn parse_bool(reader: &mut StringReader) -> Result<Value, ParseError> {
    reader.read_boolean().map(Value::Bool)
}

pub fn parse_integer(reader: &mut StringReader, min: i32, max: i32) -> Result<Value, ParseError> {
    let value = reader.read_int()?;
    check_integer_range(value, min, max)?;
    Ok(Value::Integer(value))
}

fn check_integer_range(value: i32, min: i32, max: i32) -> Result<(), ParseError> {
    if value < min {
        return Err(ParseError::with_args(
            "argument.integer.low",
            [ErrorArg::Int(min.into()), ErrorArg::Int(value.into())],
        ));
    }
    if value > max {
        return Err(ParseError::with_args(
            "argument.integer.high",
            [ErrorArg::Int(max.into()), ErrorArg::Int(value.into())],
        ));
    }
    Ok(())
}

pub fn parse_long(reader: &mut StringReader, min: i64, max: i64) -> Result<Value, ParseError> {
    let value = reader.read_long()?;
    if value < min {
        return Err(ParseError::with_args(
            "argument.long.low",
            [ErrorArg::Int(min), ErrorArg::Int(value)],
        ));
    }
    if value > max {
        return Err(ParseError::with_args(
            "argument.long.high",
            [ErrorArg::Int(max), ErrorArg::Int(value)],
        ));
    }
    Ok(Value::Long(value))
}

pub fn parse_float(reader: &mut StringReader, min: f32, max: f32) -> Result<Value, ParseError> {
    let value = reader.read_float()?;
    if value < min {
        return Err(ParseError::with_args(
            "argument.float.low",
            [
                ErrorArg::Float(f64::from(min)),
                ErrorArg::Float(f64::from(value)),
            ],
        ));
    }
    if value > max {
        return Err(ParseError::with_args(
            "argument.float.high",
            [
                ErrorArg::Float(f64::from(max)),
                ErrorArg::Float(f64::from(value)),
            ],
        ));
    }
    Ok(Value::Float(value))
}

pub fn parse_double(reader: &mut StringReader, min: f64, max: f64) -> Result<Value, ParseError> {
    let value = reader.read_double()?;
    if value < min {
        return Err(ParseError::with_args(
            "argument.double.low",
            [ErrorArg::Float(min), ErrorArg::Float(value)],
        ));
    }
    if value > max {
        return Err(ParseError::with_args(
            "argument.double.high",
            [ErrorArg::Float(max), ErrorArg::Float(value)],
        ));
    }
    Ok(Value::Double(value))
}

pub fn parse_string(reader: &mut StringReader, kind: StringKind) -> Result<Value, ParseError> {
    match kind {
        StringKind::SingleWord => Ok(Value::String(reader.read_unquoted_string())),
        StringKind::Quotable => reader.read_string().map(Value::String),
        StringKind::Greedy => {
            let text = reader.remaining();
            reader.set_cursor(reader.total_length());
            Ok(Value::String(text))
        }
    }
}

/// 官方 `TimeArgument`。`d` 是一天，`s` 是一秒，`t` 是游戏刻。
pub fn parse_time(reader: &mut StringReader, minimum: i32) -> Result<Value, ParseError> {
    let value = reader.read_float()?;
    let unit = reader.read_unquoted_string();
    let factor = match unit.as_str() {
        "d" => 24_000,
        "s" => 20,
        "t" | "" => 1,
        _ => return Err(ParseError::new("argument.time.invalid_unit")),
    };
    // 官方 `Math.round(float)` 等于 `floor(x + 0.5f)`，不是四舍五入到远离零。
    let ticks = (value * factor as f32 + 0.5).floor() as i32;
    if ticks < minimum {
        return Err(ParseError::with_args(
            "argument.time.tick_count_too_low",
            [ErrorArg::Int(minimum.into()), ErrorArg::Int(ticks.into())],
        ));
    }
    Ok(Value::Time(ticks))
}

pub fn parse_game_mode(reader: &mut StringReader) -> Result<Value, ParseError> {
    let name = reader.read_unquoted_string();
    GameMode::by_name(&name)
        .map(Value::GameMode)
        .ok_or_else(|| ParseError::text("argument.gamemode.invalid", name))
}

/// 官方 `MinMaxBounds.Ints.fromReader` 的文本形式：`5`、`5..`、`..5`、`5..10`。
pub fn parse_int_range(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    if !reader.can_read() {
        return Err(ParseError::new("argument.range.empty"));
    }
    let rest = reader.remaining();
    let end = rest
        .find(|c: char| c != '.' && !c.is_ascii_digit() && c != '-')
        .unwrap_or(rest.len());
    let text = &rest[..end];
    if text.is_empty() {
        return Err(ParseError::new("argument.range.empty"));
    }
    let parse = |part: &str| -> Result<Option<i32>, ParseError> {
        if part.is_empty() {
            return Ok(None);
        }
        part.parse::<i32>()
            .map(Some)
            .map_err(|_| ParseError::new("argument.range.ints"))
    };
    let (min, max) = match text.split_once("..") {
        Some((min, max)) => (parse(min)?, parse(max)?),
        None => {
            let value = parse(text)?;
            (value, value)
        }
    };
    if let (Some(min), Some(max)) = (min, max)
        && min > max
    {
        return Err(ParseError::new("argument.range.swapped"));
    }
    if min.is_none() && max.is_none() {
        return Err(ParseError::new("argument.range.empty"));
    }
    reader.set_cursor(start + end);
    Ok(Value::IntBounds { min, max })
}

/// 官方 `SlotArgument`：物品栏槽位的名字，不在这里查表。
pub fn parse_slot(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    let name = reader.read_unquoted_string();
    if name.is_empty() {
        reader.set_cursor(start);
        return Err(ParseError::new("argument.slot.invalid"));
    }
    Ok(Value::String(name))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_ranges_use_the_official_keys() {
        let mut reader = StringReader::new("0");
        let error = parse_integer(&mut reader, 1, 10).unwrap_err();
        assert_eq!(error.key, "argument.integer.low");
        assert_eq!(error.args, [ErrorArg::Int(1), ErrorArg::Int(0)]);

        let mut reader = StringReader::new("11");
        let error = parse_integer(&mut reader, 1, 10).unwrap_err();
        assert_eq!(error.key, "argument.integer.high");
        assert_eq!(error.args, [ErrorArg::Int(10), ErrorArg::Int(11)]);
    }

    #[test]
    fn time_units_scale_ticks() {
        assert_eq!(
            parse_time(&mut StringReader::new("1d"), 0).unwrap(),
            Value::Time(24_000)
        );
        assert_eq!(
            parse_time(&mut StringReader::new("1.5s"), 0).unwrap(),
            Value::Time(30)
        );
        assert_eq!(
            parse_time(&mut StringReader::new("7"), 0).unwrap(),
            Value::Time(7)
        );
        assert_eq!(
            parse_time(&mut StringReader::new("-1"), 0).unwrap_err().key,
            "argument.time.tick_count_too_low"
        );
        assert_eq!(
            parse_time(&mut StringReader::new("1x"), 0).unwrap_err().key,
            "argument.time.invalid_unit"
        );
    }

    #[test]
    fn time_rounds_like_java() {
        // Math.round(0.5f) == 1，Math.round(-2.5f) == -2。
        assert_eq!(
            parse_time(&mut StringReader::new("0.5"), i32::MIN).unwrap(),
            Value::Time(1)
        );
        assert_eq!(
            parse_time(&mut StringReader::new("-2.5"), i32::MIN).unwrap(),
            Value::Time(-2)
        );
    }

    #[test]
    fn game_modes_are_case_sensitive() {
        assert_eq!(
            parse_game_mode(&mut StringReader::new("spectator")).unwrap(),
            Value::GameMode(GameMode::Spectator)
        );
        let error = parse_game_mode(&mut StringReader::new("Spectator")).unwrap_err();
        assert_eq!(error.key, "argument.gamemode.invalid");
        assert_eq!(error.args, [ErrorArg::Text("Spectator".into())]);
    }

    #[test]
    fn greedy_strings_take_everything() {
        let mut reader = StringReader::new("hello @a world");
        assert_eq!(
            parse_string(&mut reader, StringKind::Greedy).unwrap(),
            Value::String("hello @a world".into())
        );
        assert!(!reader.can_read());
    }
}
