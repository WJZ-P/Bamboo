//! 解析错误，对应官方 `CommandSyntaxException` 的翻译键和参数。
//!
//! 官方在解析时抛出异常，异常里带一个可翻译的 `Component`。这里只保存键和参数，
//! 由网络层翻成 `Component` 发给客户端，所以这个 crate 不依赖聊天和协议。

/// 翻译参数。官方异常里允许放数字、字符串和布尔值。
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorArg {
    Text(String),
    /// 整数和长整数。翻译时按十进制输出。
    Int(i64),
    /// 单精度和双精度浮点数。翻译时按 Java 的 `Float`/`Double.toString` 输出。
    Float(f64),
    Bool(bool),
}

impl From<&str> for ErrorArg {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for ErrorArg {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&String> for ErrorArg {
    fn from(value: &String) -> Self {
        Self::Text(value.clone())
    }
}

macro_rules! int_args {
    ($($kind:ty),+) => {
        $(impl From<$kind> for ErrorArg {
            fn from(value: $kind) -> Self {
                Self::Int(value.into())
            }
        })+
    };
}

int_args!(i8, i16, i32, i64, u8, u16, u32);

impl From<f32> for ErrorArg {
    fn from(value: f32) -> Self {
        Self::Float(f64::from(value))
    }
}

impl From<f64> for ErrorArg {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<bool> for ErrorArg {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

/// 一条解析错误：翻译键和按顺序排列的参数。
///
/// 键和官方 `CommandSyntaxException.BUILT_IN_EXCEPTIONS` 以及各种动态异常相同，
/// 客户端和语言文件都认识。
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub key: &'static str,
    pub args: Vec<ErrorArg>,
}

impl ParseError {
    /// 没有参数的错误，比如 `command.unknown.command`。
    pub fn new(key: &'static str) -> Self {
        Self {
            key,
            args: Vec::new(),
        }
    }

    /// 只有一个文本参数的错误，比如 `parsing.int.invalid`。
    pub fn text(key: &'static str, text: impl Into<String>) -> Self {
        Self {
            key,
            args: vec![ErrorArg::Text(text.into())],
        }
    }

    /// 按顺序带上参数的错误。
    pub fn with_args<const N: usize>(key: &'static str, args: [ErrorArg; N]) -> Self {
        Self {
            key,
            args: args.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_convert_from_the_java_shapes() {
        assert_eq!(ErrorArg::from(3_i32), ErrorArg::Int(3));
        assert_eq!(ErrorArg::from(3_u32), ErrorArg::Int(3));
        assert_eq!(ErrorArg::from(1.5_f32), ErrorArg::Float(1.5));
        assert_eq!(ErrorArg::from("stone"), ErrorArg::Text("stone".into()));
        assert_eq!(ErrorArg::from(true), ErrorArg::Bool(true));
    }
}
