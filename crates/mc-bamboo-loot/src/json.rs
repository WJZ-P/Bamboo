//! 解析数据包 JSON 时用的小工具。
//!
//! 官方的 codec 在加载时就校验过数据，这里的数据来自生成器，正常情况下不会坏。
//! 所有函数返回 [`LootError::Invalid`] 而不是 panic，测试里能把坏数据报出来。

use serde_json::{Map, Value};

use crate::{LootError, Result};

pub(crate) fn invalid(message: impl Into<String>) -> LootError {
    LootError::Invalid(message.into())
}

/// 取 JSON 对象，不是对象时报错。
pub(crate) fn object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| invalid(format!("{what} must be an object")))
}

/// 可选的字段。
pub(crate) fn field<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    object.get(name).filter(|value| !value.is_null())
}

/// 必填字段。
pub(crate) fn required<'a>(
    object: &'a Map<String, Value>,
    name: &str,
    what: &str,
) -> Result<&'a Value> {
    field(object, name).ok_or_else(|| invalid(format!("{what} is missing field `{name}`")))
}

pub(crate) fn string<'a>(value: &'a Value, what: &str) -> Result<&'a str> {
    value
        .as_str()
        .ok_or_else(|| invalid(format!("{what} must be a string")))
}

pub(crate) fn opt_string<'a>(
    object: &'a Map<String, Value>,
    name: &str,
    what: &str,
) -> Result<Option<&'a str>> {
    field(object, name).map(|value| string(value, what)).transpose()
}

/// 只接受字符串的数组，单个字符串也当成只有一项的数组。
///
/// 官方的 `HolderSet` 和几个列表 codec 都放宽成这种写法。
pub(crate) fn string_or_list<'a>(value: &'a Value, what: &str) -> Result<Vec<&'a str>> {
    match value {
        Value::String(name) => Ok(vec![name.as_str()]),
        Value::Array(values) => values
            .iter()
            .map(|value| string(value, what))
            .collect::<Result<Vec<_>>>(),
        _ => Err(invalid(format!("{what} must be a string or a string list"))),
    }
}

/// 字符串数组，返回自己拥有的 `String`，用在要长期保存的字段上。
pub(crate) fn string_list(value: &Value, what: &str) -> Result<Vec<String>> {
    array(value, what)?
        .iter()
        .map(|value| string(value, what).map(str::to_owned))
        .collect()
}

/// 可选的字符串数组字段。
pub(crate) fn opt_string_list(
    object: &Map<String, Value>,
    name: &str,
    what: &str,
) -> Result<Option<Vec<String>>> {
    field(object, name)
        .map(|value| string_list(value, what))
        .transpose()
}

pub(crate) fn array<'a>(value: &'a Value, what: &str) -> Result<&'a [Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| invalid(format!("{what} must be an array")))
}

pub(crate) fn opt_array<'a>(
    object: &'a Map<String, Value>,
    name: &str,
    what: &str,
) -> Result<Option<&'a [Value]>> {
    field(object, name)
        .map(|value| array(value, what))
        .transpose()
}

pub(crate) fn i32_value(value: &Value, what: &str) -> Result<i32> {
    value
        .as_i64()
        .and_then(|number| i32::try_from(number).ok())
        .ok_or_else(|| invalid(format!("{what} must be an int")))
}

pub(crate) fn opt_i32(
    object: &Map<String, Value>,
    name: &str,
    what: &str,
    default: i32,
) -> Result<i32> {
    match field(object, name) {
        Some(value) => i32_value(value, what),
        None => Ok(default),
    }
}

/// 官方的 codec 读 float：`Float.parseFloat` 的十进制最短表示先读成 f64 再收窄，
/// 对 `Float.toString` 写出的值结果相同。
pub(crate) fn f32_value(value: &Value, what: &str) -> Result<f32> {
    value
        .as_f64()
        .map(|number| number as f32)
        .ok_or_else(|| invalid(format!("{what} must be a float")))
}

pub(crate) fn opt_bool(
    object: &Map<String, Value>,
    name: &str,
    what: &str,
    default: bool,
) -> Result<bool> {
    match field(object, name) {
        Some(value) => value
            .as_bool()
            .ok_or_else(|| invalid(format!("{what} must be a bool"))),
        None => Ok(default),
    }
}

pub(crate) fn type_name<'a>(object: &'a Map<String, Value>, what: &str) -> Result<&'a str> {
    string(required(object, "type", what)?, "type")
}

/// 去掉 `#` 前缀，返回（名字，是不是标签）。
pub(crate) fn holder_name(name: &str) -> (&str, bool) {
    match name.strip_prefix('#') {
        Some(tag) => (tag, true),
        None => (name, false),
    }
}
