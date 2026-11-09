//! 和 Brigadier 的 `StringReader` 行为一致的字符读取器。
//!
//! 游标按字符计数。官方按 UTF-16 代码单元计数，命令里的非 BMP 字符会差一位，
//! 这是目前已知的差异，命令参数本身几乎不会用到。

use crate::ParseError;

/// 从字符串里按官方规则读取参数。
#[derive(Debug, Clone)]
pub struct StringReader {
    input: Vec<char>,
    cursor: usize,
}

impl StringReader {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            cursor: 0,
        }
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor;
    }

    pub fn total_length(&self) -> usize {
        self.input.len()
    }

    pub fn can_read(&self) -> bool {
        self.cursor < self.input.len()
    }

    pub fn can_read_n(&self, length: usize) -> bool {
        self.cursor + length <= self.input.len()
    }

    pub fn peek(&self) -> char {
        self.input[self.cursor]
    }

    /// 往后第 `offset` 个字符。调用前要自己确认 `can_read_n(offset + 1)`。
    pub fn peek_at(&self, offset: usize) -> char {
        self.input[self.cursor + offset]
    }

    /// 读一个字符并前进。官方 `StringReader.read`。
    pub fn read(&mut self) -> char {
        let character = self.peek();
        self.skip();
        character
    }

    pub fn skip(&mut self) {
        self.cursor += 1;
    }

    /// `start..end` 之间的原文，和官方的 `getString().substring` 一样。
    pub fn slice(&self, start: usize, end: usize) -> String {
        self.input[start..end].iter().collect()
    }

    /// 只跳过空格，和官方的 `skipWhitespace` 一样。
    pub fn skip_whitespace(&mut self) {
        while self.can_read() && self.peek() == ' ' {
            self.skip();
        }
    }

    /// 剩下还没有读的部分。
    pub fn remaining(&self) -> String {
        self.input[self.cursor..].iter().collect()
    }

    pub fn remaining_length(&self) -> usize {
        self.input.len() - self.cursor
    }

    /// 整个输入。官方的 `getString`。
    pub fn string(&self) -> String {
        self.input.iter().collect()
    }

    pub fn read_unquoted_string(&mut self) -> String {
        let start = self.cursor;
        while self.can_read() && is_allowed_in_unquoted_string(self.peek()) {
            self.skip();
        }
        self.input[start..self.cursor].iter().collect()
    }

    /// 有引号时读带引号的字符串，否则读不带引号的。输入结束时返回空字符串。
    pub fn read_string(&mut self) -> Result<String, ParseError> {
        if !self.can_read() {
            return Ok(String::new());
        }
        match self.peek() {
            '"' | '\'' => self.read_quoted_string(),
            _ => Ok(self.read_unquoted_string()),
        }
    }

    pub fn read_quoted_string(&mut self) -> Result<String, ParseError> {
        if !self.can_read() {
            return Ok(String::new());
        }
        let quote = self.peek();
        if quote != '"' && quote != '\'' {
            return Err(ParseError::text(
                "parsing.quote.expected.start",
                quote.to_string(),
            ));
        }
        self.skip();
        let mut result = String::new();
        let mut escaped = false;
        while self.can_read() {
            let next = self.peek();
            if escaped {
                if next == quote || next == '\\' {
                    result.push(next);
                    self.skip();
                    escaped = false;
                } else {
                    self.cursor -= 1;
                    return Err(ParseError::text("parsing.quote.escape", next.to_string()));
                }
            } else if next == '\\' {
                self.skip();
                escaped = true;
            } else if next == quote {
                self.skip();
                return Ok(result);
            } else {
                result.push(next);
                self.skip();
            }
        }
        Err(ParseError::new("parsing.quote.expected.end"))
    }

    pub fn read_boolean(&mut self) -> Result<bool, ParseError> {
        let value = self.read_string()?;
        if value.is_empty() {
            return Err(ParseError::new("parsing.bool.expected"));
        }
        match value.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(ParseError::text("parsing.bool.invalid", value)),
        }
    }

    pub fn read_int(&mut self) -> Result<i32, ParseError> {
        let (start, raw) = self.read_number();
        if raw.is_empty() {
            return Err(ParseError::new("parsing.int.expected"));
        }
        raw.parse().map_err(|_| {
            self.cursor = start;
            ParseError::text("parsing.int.invalid", raw)
        })
    }

    pub fn read_long(&mut self) -> Result<i64, ParseError> {
        let (start, raw) = self.read_number();
        if raw.is_empty() {
            return Err(ParseError::new("parsing.long.expected"));
        }
        raw.parse().map_err(|_| {
            self.cursor = start;
            ParseError::text("parsing.long.invalid", raw)
        })
    }

    pub fn read_double(&mut self) -> Result<f64, ParseError> {
        let (start, raw) = self.read_number();
        if raw.is_empty() {
            return Err(ParseError::new("parsing.double.expected"));
        }
        parse_java_double(&raw).ok_or_else(|| {
            self.cursor = start;
            ParseError::text("parsing.double.invalid", raw)
        })
    }

    pub fn read_float(&mut self) -> Result<f32, ParseError> {
        let (start, raw) = self.read_number();
        if raw.is_empty() {
            return Err(ParseError::new("parsing.float.expected"));
        }
        parse_java_double(&raw)
            .map(|value| value as f32)
            .ok_or_else(|| {
                self.cursor = start;
                ParseError::text("parsing.float.invalid", raw)
            })
    }

    /// 读下一个字符，不是 `expected` 时抛 `parsing.expected`。
    pub fn expect(&mut self, expected: char) -> Result<(), ParseError> {
        if !self.can_read() || self.peek() != expected {
            return Err(ParseError::text("parsing.expected", expected.to_string()));
        }
        self.skip();
        Ok(())
    }

    /// 读一段数字字符，返回起点和原文。没有读到时原文是空的。
    fn read_number(&mut self) -> (usize, String) {
        let start = self.cursor;
        while self.can_read() && is_allowed_number(self.peek()) {
            self.skip();
        }
        (start, self.input[start..self.cursor].iter().collect())
    }
}

/// Brigadier 允许出现在数字里的字符：数字、小数点和负号。
pub fn is_allowed_number(character: char) -> bool {
    character.is_ascii_digit() || character == '.' || character == '-'
}

/// Brigadier 允许出现在不带引号字符串里的字符。
pub fn is_allowed_in_unquoted_string(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '+')
}

/// 按 Java `Double.parseDouble` 的规则解析 Brigadier 能读出来的数字。
///
/// Brigadier 只允许数字、小数点和负号，所以这里处理不了指数、十六进制、
/// 后缀和 `Infinity` 这类 Java 形式。不接受时返回 `None`。
pub(crate) fn parse_java_double(raw: &str) -> Option<f64> {
    let digits = raw.strip_prefix('-').unwrap_or(raw);
    if digits.is_empty() || digits.matches('.').count() > 1 {
        return None;
    }
    let (whole, fraction) = match digits.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (digits, None),
    };
    if whole.is_empty() && fraction.is_none_or(str::is_empty) {
        return None;
    }
    let mut text = String::with_capacity(raw.len() + 2);
    if raw.starts_with('-') {
        text.push('-');
    }
    text.push_str(if whole.is_empty() { "0" } else { whole });
    if let Some(fraction) = fraction {
        text.push('.');
        text.push_str(if fraction.is_empty() { "0" } else { fraction });
    }
    text.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorArg;

    fn reader(input: &str) -> StringReader {
        StringReader::new(input)
    }

    #[test]
    fn reads_brigadier_unquoted_strings() {
        let mut input = reader("foo_bar-1.5+baz qux");
        assert_eq!(input.read_unquoted_string(), "foo_bar-1.5+baz");
        assert_eq!(input.peek(), ' ');
        input.skip_whitespace();
        assert_eq!(input.read_unquoted_string(), "qux");
        assert!(!input.can_read());
    }

    #[test]
    fn reads_single_and_double_quoted_strings() {
        let mut input = reader(r#""hello world" 'it\'s'"#);
        assert_eq!(input.read_string().unwrap(), "hello world");
        input.skip_whitespace();
        assert_eq!(input.read_string().unwrap(), "it's");

        assert_eq!(reader("").read_string().unwrap(), "");
        assert_eq!(reader("abc").read_string().unwrap(), "abc");
    }

    #[test]
    fn rejects_bad_quotes_like_brigadier() {
        assert_eq!(
            reader(r#""unterminated"#)
                .read_quoted_string()
                .unwrap_err()
                .key,
            "parsing.quote.expected.end"
        );
        let error = reader(r#""bad\q""#).read_quoted_string().unwrap_err();
        assert_eq!(error.key, "parsing.quote.escape");
        assert_eq!(error.args, [ErrorArg::Text("q".into())]);
        assert_eq!(
            reader("bad").read_quoted_string().unwrap_err().key,
            "parsing.quote.expected.start"
        );
    }

    #[test]
    fn reads_numbers_exactly_like_brigadier() {
        let mut input = reader("-12.5 -3 4. .5 5.. -");
        assert_eq!(input.read_double().unwrap(), -12.5);
        input.skip_whitespace();
        assert_eq!(input.read_double().unwrap(), -3.0);
        input.skip_whitespace();
        assert_eq!(input.read_double().unwrap(), 4.0);
        input.skip_whitespace();
        assert_eq!(input.read_double().unwrap(), 0.5);
        input.skip_whitespace();
        let error = input.read_double().unwrap_err();
        assert_eq!(error.key, "parsing.double.invalid");
        assert_eq!(error.args, [ErrorArg::Text("5..".into())]);
        // 失败后游标回到数字开头。
        assert_eq!(input.cursor(), 15);
        assert_eq!(
            input.read_int().unwrap_err().args,
            [ErrorArg::Text("5..".into())]
        );
        assert_eq!(input.read_int().unwrap_err().key, "parsing.int.invalid");
    }

    #[test]
    fn reads_integer_and_long_bounds() {
        assert_eq!(reader("2147483647").read_int().unwrap(), i32::MAX);
        assert_eq!(
            reader("2147483648").read_int().unwrap_err().key,
            "parsing.int.invalid"
        );
        assert_eq!(
            reader("x").read_int().unwrap_err().key,
            "parsing.int.expected"
        );
        assert_eq!(reader("9223372036854775807").read_long().unwrap(), i64::MAX);
        assert_eq!(
            reader("1.5").read_long().unwrap_err().key,
            "parsing.long.invalid"
        );
        assert_eq!(
            reader("abc").read_double().unwrap_err().key,
            "parsing.double.expected"
        );
        let mut number = reader("1e3");
        assert_eq!(number.read_double().unwrap(), 1.0);
        assert_eq!(number.cursor(), 1);
        assert_eq!(number.remaining(), "e3");
    }

    #[test]
    fn reads_booleans_like_brigadier() {
        assert!(reader("true").read_boolean().unwrap());
        assert!(!reader("false").read_boolean().unwrap());
        assert_eq!(
            reader("TRUE").read_boolean().unwrap_err().key,
            "parsing.bool.invalid"
        );
        assert_eq!(
            reader("").read_boolean().unwrap_err().key,
            "parsing.bool.expected"
        );
    }

    #[test]
    fn expect_reports_the_symbol() {
        let mut input = reader("]");
        assert_eq!(
            input.expect(',').unwrap_err().args,
            [ErrorArg::Text(",".into())]
        );
        input.expect(']').unwrap();
        assert_eq!(input.expect(']').unwrap_err().key, "parsing.expected");
    }
}
