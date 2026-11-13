//! 坐标参数：世界坐标 `x y z`、相对坐标 `~ ~ ~`、局部坐标 `^ ^ ^`。

use crate::{
    ParseError, Value,
    reader::StringReader,
    value::{Coordinates, Rotation, WorldCoordinate},
};

const RELATIVE_PREFIX: char = '~';
const LOCAL_PREFIX: char = '^';

/// 官方 `WorldCoordinate.parseDouble`。
fn parse_world_double(
    reader: &mut StringReader,
    center: bool,
) -> Result<WorldCoordinate, ParseError> {
    if reader.can_read() && reader.peek() == LOCAL_PREFIX {
        return Err(ParseError::new("argument.pos.mixed"));
    }
    if !reader.can_read() {
        return Err(ParseError::new("argument.pos.missing.double"));
    }
    let relative = read_relative_prefix(reader);
    let start = reader.cursor();
    let mut value = if reader.can_read() && reader.peek() != ' ' {
        reader.read_double()?
    } else {
        0.0
    };
    let number = reader.slice(start, reader.cursor());
    if relative && number.is_empty() {
        return Ok(WorldCoordinate {
            relative: true,
            value: 0.0,
        });
    }
    if !number.contains('.') && !relative && center {
        value += 0.5;
    }
    Ok(WorldCoordinate { relative, value })
}

/// 官方 `WorldCoordinate.parseInt`。
fn parse_world_int(reader: &mut StringReader) -> Result<WorldCoordinate, ParseError> {
    if reader.can_read() && reader.peek() == LOCAL_PREFIX {
        return Err(ParseError::new("argument.pos.mixed"));
    }
    if !reader.can_read() {
        return Err(ParseError::new("argument.pos.missing.int"));
    }
    let relative = read_relative_prefix(reader);
    let value = if reader.can_read() && reader.peek() != ' ' {
        if relative {
            reader.read_double()?
        } else {
            f64::from(reader.read_int()?)
        }
    } else {
        0.0
    };
    Ok(WorldCoordinate { relative, value })
}

/// 读 `~`，返回是不是相对坐标。官方 `WorldCoordinate.isRelative`。
fn read_relative_prefix(reader: &mut StringReader) -> bool {
    if reader.peek() == RELATIVE_PREFIX {
        reader.skip();
        true
    } else {
        false
    }
}

fn parse_world_coordinates(
    reader: &mut StringReader,
    center_correct: bool,
    integer: bool,
) -> Result<Coordinates, ParseError> {
    let start = reader.cursor();
    let parse = |reader: &mut StringReader, center: bool| {
        if integer {
            parse_world_int(reader)
        } else {
            parse_world_double(reader, center)
        }
    };
    let x = parse(reader, center_correct)?;
    if reader.can_read() && reader.peek() == ' ' {
        reader.skip();
        let y = parse(reader, false)?;
        if reader.can_read() && reader.peek() == ' ' {
            reader.skip();
            let z = parse(reader, center_correct)?;
            return Ok(Coordinates::World { x, y, z });
        }
    }
    reader.set_cursor(start);
    Err(ParseError::new("argument.pos3d.incomplete"))
}

/// 官方 `LocalCoordinates.parse`。
fn parse_local(reader: &mut StringReader) -> Result<Coordinates, ParseError> {
    let start = reader.cursor();
    let left = parse_local_axis(reader, start)?;
    if reader.can_read() && reader.peek() == ' ' {
        reader.skip();
        let up = parse_local_axis(reader, start)?;
        if reader.can_read() && reader.peek() == ' ' {
            reader.skip();
            let forwards = parse_local_axis(reader, start)?;
            return Ok(Coordinates::Local { left, up, forwards });
        }
    }
    reader.set_cursor(start);
    Err(ParseError::new("argument.pos3d.incomplete"))
}

fn parse_local_axis(reader: &mut StringReader, start: usize) -> Result<f64, ParseError> {
    if !reader.can_read() {
        return Err(ParseError::new("argument.pos.missing.double"));
    }
    if reader.peek() != LOCAL_PREFIX {
        reader.set_cursor(start);
        return Err(ParseError::new("argument.pos.mixed"));
    }
    reader.skip();
    if reader.can_read() && reader.peek() != ' ' {
        reader.read_double()
    } else {
        Ok(0.0)
    }
}

/// `Vec3Argument.vec3()`。
pub fn parse_vec3(reader: &mut StringReader) -> Result<Value, ParseError> {
    if reader.can_read() && reader.peek() == LOCAL_PREFIX {
        parse_local(reader).map(Value::Vec3)
    } else {
        parse_world_coordinates(reader, true, false).map(Value::Vec3)
    }
}

/// `Vec3Argument.vec3(false)`。
pub fn parse_vec3_without_centering(reader: &mut StringReader) -> Result<Value, ParseError> {
    if reader.can_read() && reader.peek() == LOCAL_PREFIX {
        parse_local(reader).map(Value::Vec3)
    } else {
        parse_world_coordinates(reader, false, false).map(Value::Vec3)
    }
}

/// 官方 `Vec2Argument.vec2()`：两个水平坐标，绝对整数都居中，不支持 `^` 局部坐标。
///
/// Vanilla `Vec2Argument.vec2()`: two horizontal coordinates, centring absolute
/// integers; local `^` coordinates are not accepted by `parse`.
pub fn parse_vec2(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    if !reader.can_read() {
        return Err(ParseError::new("argument.pos2d.incomplete"));
    }
    let x = parse_world_double(reader, true)?;
    if reader.can_read() && reader.peek() == ' ' {
        reader.skip();
        let z = parse_world_double(reader, true)?;
        return Ok(Value::Vec2([x, z]));
    }
    reader.set_cursor(start);
    Err(ParseError::new("argument.pos2d.incomplete"))
}

/// `BlockPosArgument.blockPos()`。
pub fn parse_block_pos(reader: &mut StringReader) -> Result<Value, ParseError> {
    if reader.can_read() && reader.peek() == LOCAL_PREFIX {
        parse_local(reader).map(Value::BlockPos)
    } else {
        parse_world_coordinates(reader, false, true).map(Value::BlockPos)
    }
}

/// `RotationArgument.rotation()`。第一个数是水平角，第二个数是俯仰角。
pub fn parse_rotation(reader: &mut StringReader) -> Result<Value, ParseError> {
    let start = reader.cursor();
    if !reader.can_read() {
        return Err(ParseError::new("argument.rotation.incomplete"));
    }
    let yaw = parse_world_double(reader, false)?;
    if reader.can_read() && reader.peek() == ' ' {
        reader.skip();
        let pitch = parse_world_double(reader, false)?;
        return Ok(Value::Rotation(Rotation { yaw, pitch }));
    }
    reader.set_cursor(start);
    Err(ParseError::new("argument.rotation.incomplete"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bamboo_core::Vec3;

    fn vec3(input: &str) -> Coordinates {
        match parse_vec3(&mut StringReader::new(input)).unwrap() {
            Value::Vec3(coordinates) => coordinates,
            other => panic!("不是坐标：{other:?}"),
        }
    }

    #[test]
    fn centers_whole_world_coordinates() {
        assert_eq!(
            vec3("1 2 3").position(Vec3::default(), 0.0, 0.0),
            Vec3::new(1.5, 2.0, 3.5)
        );
        assert_eq!(
            vec3("1.0 2 3.0").position(Vec3::default(), 0.0, 0.0),
            Vec3::new(1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn relative_coordinates_use_the_source() {
        let coordinates = vec3("~ ~1 ~-2.5");
        assert!(coordinates.is_x_relative());
        assert_eq!(
            coordinates.position(Vec3::new(10.0, 20.0, 30.0), 0.0, 0.0),
            Vec3::new(10.0, 21.0, 27.5)
        );
    }

    #[test]
    fn local_coordinates_need_three_carets() {
        let coordinates = vec3("^ ^ ^2");
        // 官方的三角函数是查表的，坐标会有 1e-7 量级的误差。
        let position = coordinates.position(Vec3::new(10.0, 20.0, 30.0), 0.0, 0.0);
        assert!((position.x - 10.0).abs() < 1.0e-6);
        assert!((position.y - 20.0).abs() < 1.0e-6);
        assert!((position.z - 32.0).abs() < 1.0e-6);
        let mut reader = StringReader::new("~ ^ ^");
        let error = parse_vec3(&mut reader).unwrap_err();
        assert_eq!(error.key, "argument.pos.mixed");
        // 官方在第二个坐标处直接抛出，游标停在 `^` 上。
        assert_eq!(reader.cursor(), 2);
    }

    #[test]
    fn incomplete_coordinates_reset_to_the_start() {
        let mut reader = StringReader::new("1 2");
        let error = parse_vec3(&mut reader).unwrap_err();
        assert_eq!(error.key, "argument.pos3d.incomplete");
        assert_eq!(reader.cursor(), 0);
    }

    #[test]
    fn block_positions_parse_ints() {
        let mut reader = StringReader::new("1 2 3");
        let value = parse_block_pos(&mut reader).unwrap();
        assert_eq!(
            value.as_block_pos(),
            Some(Coordinates::absolute(1.0, 2.0, 3.0))
        );
        assert_eq!(
            parse_block_pos(&mut StringReader::new("1.5 2 3"))
                .unwrap_err()
                .key,
            "parsing.int.invalid"
        );
    }

    #[test]
    fn rotations_use_two_coordinates() {
        let mut reader = StringReader::new("~90 -45");
        let value = parse_rotation(&mut reader).unwrap();
        assert_eq!(
            value.as_rotation().unwrap().resolve(10.0, 20.0),
            (100.0, -45.0)
        );
        assert_eq!(
            parse_rotation(&mut StringReader::new("90"))
                .unwrap_err()
                .key,
            "argument.rotation.incomplete"
        );
    }
}
