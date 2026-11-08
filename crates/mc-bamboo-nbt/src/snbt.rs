//! SNBT：文本形式的 NBT。
//!
//! 解析部分复述官方 `SnbtGrammar`（packrat 语法）和 `TagParser`：数字的后缀、进制、
//! 下划线和范围检查，字符串的转义，`true`/`false` 与 `bool()`/`uuid()` 内建，数组字面量，
//! 重复键后者覆盖前者，结束后还有内容算错。
//!
//! 打印部分有两套：紧凑形式对标 `StringTagVisitor`（`CompoundTag.toString()` 用它），
//! 带缩进的漂亮形式对标 `SnbtPrinterTagVisitor`（`/data get` 用它）。

use crate::{MAX_DEPTH, Tag};

/// SNBT 解析失败。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} at position {position}")]
pub struct SnbtError {
    /// 出错位置的字符下标（不是字节下标）。
    pub position: usize,
    /// 出错原因，英文，和项目里其他错误信息一致。
    pub message: String,
}

/// 按官方 `TagParser.parseFully` 解析一段 SNBT，必须正好消费完输入。
pub fn parse_snbt(input: &str) -> Result<Tag, SnbtError> {
    let characters: Vec<char> = input.chars().collect();
    let mut parser = Parser::new(&characters);
    let tag = parser.parse_literal()?;
    parser.skip_whitespace();
    if parser.pos < parser.input.len() {
        return Err(parser.error("trailing data"));
    }
    Ok(tag)
}

/// 按官方 `TagParser.parseCompoundFully` 解析，根节点必须是复合标签。
pub fn parse_snbt_compound(input: &str) -> Result<Vec<(String, Tag)>, SnbtError> {
    match parse_snbt(input)? {
        Tag::Compound(entries) => Ok(entries),
        other => Err(SnbtError {
            position: 0,
            message: format!("expected a compound tag, found {}", tag_name(&other)),
        }),
    }
}

fn tag_name(tag: &Tag) -> &'static str {
    match tag {
        Tag::Byte(_) => "TAG_Byte",
        Tag::Short(_) => "TAG_Short",
        Tag::Int(_) => "TAG_Int",
        Tag::Long(_) => "TAG_Long",
        Tag::Float(_) => "TAG_Float",
        Tag::Double(_) => "TAG_Double",
        Tag::ByteArray(_) => "TAG_Byte_Array",
        Tag::String(_) => "TAG_String",
        Tag::List(_) => "TAG_List",
        Tag::Compound(_) => "TAG_Compound",
        Tag::IntArray(_) => "TAG_Int_Array",
        Tag::LongArray(_) => "TAG_Long_Array",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sign {
    Plus,
    Minus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntBase {
    Binary,
    Decimal,
    Hex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignedPrefix {
    Signed,
    Unsigned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeSuffix {
    Float,
    Double,
    Byte,
    Short,
    Int,
    Long,
}

/// 官方的 `IntegerSuffix`：可选的有无符号前缀和可选的类型字母。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IntegerSuffix {
    signed: Option<SignedPrefix>,
    kind: Option<TypeSuffix>,
}

const NO_SUFFIX: IntegerSuffix = IntegerSuffix {
    signed: None,
    kind: None,
};

/// 一个还没定下类型的整数字面量，数组元素要按数组的前缀重新定类型。
#[derive(Debug, Clone)]
struct IntegerLiteral {
    sign: Option<Sign>,
    base: IntBase,
    digits: String,
    suffix: IntegerSuffix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArrayKind {
    Byte,
    Int,
    Long,
}

struct Parser<'a> {
    input: &'a [char],
    pos: usize,
    depth: u32,
}

impl<'a> Parser<'a> {
    fn new(input: &'a [char]) -> Self {
        Self {
            input,
            pos: 0,
            depth: 0,
        }
    }

    fn error(&self, message: impl Into<String>) -> SnbtError {
        SnbtError {
            position: self.pos,
            message: message.into(),
        }
    }

    fn peek_raw(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    /// 官方 `StringReader.skipWhitespace`：按 `Character.isWhitespace` 跳过空白。
    fn skip_whitespace(&mut self) {
        while let Some(character) = self.peek_raw() {
            if !is_java_whitespace(character) {
                break;
            }
            self.pos += 1;
        }
    }

    /// 每个终结符都会先跳过空白，所以标点之间的空格都允许。
    fn try_char(&mut self, expected: char) -> bool {
        self.skip_whitespace();
        if self.peek_raw() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn try_any_char(&mut self, first: char, second: char) -> Option<char> {
        self.skip_whitespace();
        match self.peek_raw() {
            Some(character) if character == first || character == second => {
                self.pos += 1;
                Some(character)
            }
            _ => None,
        }
    }

    fn expect_char(&mut self, expected: char) -> Result<(), SnbtError> {
        if self.try_char(expected) {
            Ok(())
        } else {
            Err(self.error(format!("expected '{expected}'")))
        }
    }

    fn enter_depth(&mut self) -> Result<(), SnbtError> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error(format!("SNBT is nested deeper than {MAX_DEPTH}")));
        }
        self.depth += 1;
        Ok(())
    }

    /// 官方 `literal` 规则：按首字符决定走哪个分支。
    fn parse_literal(&mut self) -> Result<Tag, SnbtError> {
        self.skip_whitespace();
        let first = match self.peek_raw() {
            Some(character) => character,
            None => return Err(self.error("unexpected end of input")),
        };
        if can_start_number(first) {
            let start = self.pos;
            if let Some(tag) = self.try_float() {
                return Ok(tag);
            }
            self.pos = start;
            if let Some(tag) = self.try_integer() {
                return Ok(tag);
            }
            self.pos = start;
            return Err(self.error("could not parse number"));
        }
        match first {
            '"' | '\'' => {
                self.pos += 1;
                let contents = self.parse_quoted_contents(first)?;
                self.expect_char(first)?;
                Ok(Tag::String(contents))
            }
            '{' => {
                self.pos += 1;
                self.parse_map()
            }
            '[' => {
                self.pos += 1;
                self.parse_list()
            }
            _ => self.parse_unquoted_or_builtin(),
        }
    }

    /// 官方 `float_literal`：四个分支依次尝试，用不上就整体失败。
    fn try_float(&mut self) -> Option<Tag> {
        let start = self.pos;
        let sign = self.try_sign();
        let after_sign = self.pos;

        // 整数部分 '.' [小数部分] [指数] [类型字母]
        if let Some(whole) = self.try_decimal_run()
            && self.try_char('.')
        {
            let fraction = self.try_decimal_run();
            let exponent = self.try_exponent();
            let suffix = self.try_float_suffix();
            return build_float(sign, Some(&whole), fraction.as_deref(), exponent, suffix);
        }

        // '.' 小数部分 [指数] [类型字母]
        self.pos = after_sign;
        if self.try_char('.') {
            let fraction = match self.try_decimal_run() {
                Some(fraction) => fraction,
                None => {
                    self.pos = start;
                    return None;
                }
            };
            let exponent = self.try_exponent();
            let suffix = self.try_float_suffix();
            return build_float(sign, None, Some(&fraction), exponent, suffix);
        }

        // 整数部分 指数 [类型字母]
        self.pos = after_sign;
        if let Some(whole) = self.try_decimal_run()
            && let Some(exponent) = self.try_exponent()
        {
            let suffix = self.try_float_suffix();
            return build_float(sign, Some(&whole), None, Some(exponent), suffix);
        }

        // 整数部分 [指数] 类型字母
        self.pos = after_sign;
        if let Some(whole) = self.try_decimal_run() {
            let exponent = self.try_exponent();
            if let Some(suffix) = self.try_float_suffix() {
                return build_float(sign, Some(&whole), None, exponent, Some(suffix));
            }
        }

        self.pos = start;
        None
    }

    fn try_sign(&mut self) -> Option<Sign> {
        match self.try_any_char('+', '-') {
            Some('+') => Some(Sign::Plus),
            Some(_) => Some(Sign::Minus),
            None => None,
        }
    }

    /// 官方 `float_exponent_part`：`e`/`E` 加可选符号再加十进制数字。
    fn try_exponent(&mut self) -> Option<(Sign, String)> {
        let start = self.pos;
        self.try_any_char('e', 'E')?;
        let sign = self.try_sign().unwrap_or(Sign::Plus);
        match self.try_decimal_run() {
            Some(digits) => Some((sign, digits)),
            None => {
                self.pos = start;
                None
            }
        }
    }

    /// 官方 `float_type_suffix`：`f`/`F` 是单精度，`d`/`D` 是双精度。
    fn try_float_suffix(&mut self) -> Option<TypeSuffix> {
        match self.try_any_char('f', 'F') {
            Some(_) => Some(TypeSuffix::Float),
            None => self.try_any_char('d', 'D').map(|_| TypeSuffix::Double),
        }
    }

    /// 官方 `NumberRunParseRule`：贪婪读一段允许的字符，首尾不能是下划线。
    fn try_run(&mut self, accepted: fn(char) -> bool) -> Option<String> {
        self.skip_whitespace();
        let start = self.pos;
        while let Some(character) = self.peek_raw() {
            if !accepted(character) {
                break;
            }
            self.pos += 1;
        }
        if self.pos == start || self.input[start] == '_' || self.input[self.pos - 1] == '_' {
            self.pos = start;
            None
        } else {
            Some(self.input[start..self.pos].iter().collect())
        }
    }

    fn try_decimal_run(&mut self) -> Option<String> {
        self.try_run(|character| character.is_ascii_digit() || character == '_')
    }

    fn try_hex_run(&mut self) -> Option<String> {
        self.try_run(|character| character.is_ascii_hexdigit() || character == '_')
    }

    fn try_binary_run(&mut self) -> Option<String> {
        self.try_run(|character| matches!(character, '0' | '1' | '_'))
    }

    fn try_integer(&mut self) -> Option<Tag> {
        let start = self.pos;
        let literal = self.try_integer_literal()?;
        let tag = create_integer(literal.sign, literal.base, &literal.digits, literal.suffix);
        if tag.is_none() {
            self.pos = start;
        }
        tag
    }

    /// 官方 `integer_literal`：可选符号、进制数字、可选后缀。
    fn try_integer_literal(&mut self) -> Option<IntegerLiteral> {
        let start = self.pos;
        let sign = self.try_sign();
        let after_sign = self.pos;

        let (base, digits) = if self.try_char('0') {
            if self.try_any_char('x', 'X').is_some() {
                // `0x` 后面有 cut：读不到十六进制数字就是错误，不再回退。
                let digits = match self.try_hex_run() {
                    Some(digits) => digits,
                    None => {
                        self.pos = start;
                        return None;
                    }
                };
                (IntBase::Hex, digits)
            } else {
                let before_binary = self.pos;
                if self.try_any_char('b', 'B').is_some() {
                    if let Some(digits) = self.try_binary_run() {
                        (IntBase::Binary, digits)
                    } else {
                        self.pos = before_binary;
                        if self.try_decimal_run().is_some() {
                            self.pos = start;
                            return None;
                        }
                        (IntBase::Decimal, String::from("0"))
                    }
                } else {
                    if self.try_decimal_run().is_some() {
                        self.pos = start;
                        return None;
                    }
                    (IntBase::Decimal, String::from("0"))
                }
            }
        } else {
            self.pos = after_sign;
            match self.try_decimal_run() {
                Some(digits) => (IntBase::Decimal, digits),
                None => {
                    self.pos = start;
                    return None;
                }
            }
        };

        let suffix = self.try_integer_suffix().unwrap_or(NO_SUFFIX);
        Some(IntegerLiteral {
            sign,
            base,
            digits,
            suffix,
        })
    }

    /// 官方 `integer_suffix`：`ub`/`us`/`ui`/`ul`、`sb`/`ss`/`si`/`sl`，以及单个字母。
    fn try_integer_suffix(&mut self) -> Option<IntegerSuffix> {
        let start = self.pos;

        if self.try_any_char('u', 'U').is_some() {
            let after_u = self.pos;
            for (letters, kind) in [
                (('b', 'B'), TypeSuffix::Byte),
                (('s', 'S'), TypeSuffix::Short),
                (('i', 'I'), TypeSuffix::Int),
                (('l', 'L'), TypeSuffix::Long),
            ] {
                self.pos = after_u;
                if self.try_any_char(letters.0, letters.1).is_some() {
                    return Some(IntegerSuffix {
                        signed: Some(SignedPrefix::Unsigned),
                        kind: Some(kind),
                    });
                }
            }
            self.pos = start;
        }

        if self.try_any_char('s', 'S').is_some() {
            let after_s = self.pos;
            for (letters, kind) in [
                (('b', 'B'), TypeSuffix::Byte),
                (('s', 'S'), TypeSuffix::Short),
                (('i', 'I'), TypeSuffix::Int),
                (('l', 'L'), TypeSuffix::Long),
            ] {
                self.pos = after_s;
                if self.try_any_char(letters.0, letters.1).is_some() {
                    return Some(IntegerSuffix {
                        signed: Some(SignedPrefix::Signed),
                        kind: Some(kind),
                    });
                }
            }
            self.pos = after_s;
            return Some(IntegerSuffix {
                signed: None,
                kind: Some(TypeSuffix::Short),
            });
        }

        for (letters, kind) in [
            (('b', 'B'), TypeSuffix::Byte),
            (('s', 'S'), TypeSuffix::Short),
            (('i', 'I'), TypeSuffix::Int),
            (('l', 'L'), TypeSuffix::Long),
        ] {
            self.pos = start;
            if self.try_any_char(letters.0, letters.1).is_some() {
                return Some(IntegerSuffix {
                    signed: None,
                    kind: Some(kind),
                });
            }
        }

        self.pos = start;
        None
    }

    /// 官方 `quoted_string_literal` 的内容部分：一直读到同种引号为止。
    fn parse_quoted_contents(&mut self, quote: char) -> Result<String, SnbtError> {
        let mut contents = String::new();
        loop {
            let character = match self.peek_raw() {
                Some(character) => character,
                None => return Err(self.error("unterminated string")),
            };
            if character == quote {
                return Ok(contents);
            }
            if character == '\\' {
                self.pos += 1;
                contents.push_str(&self.parse_escape()?);
            } else {
                contents.push(character);
                self.pos += 1;
            }
        }
    }

    /// 官方 `string_escape_sequence`：返回转义展开后的内容。
    fn parse_escape(&mut self) -> Result<String, SnbtError> {
        // 转义序列的终结符同样先跳过空白。
        self.skip_whitespace();
        let character = match self.peek_raw() {
            Some(character) => character,
            None => return Err(self.error("unexpected end of input")),
        };
        self.pos += 1;
        match character {
            'b' => Ok(String::from("\u{8}")),
            's' => Ok(String::from(" ")),
            't' => Ok(String::from("\t")),
            'n' => Ok(String::from("\n")),
            'f' => Ok(String::from("\u{c}")),
            'r' => Ok(String::from("\r")),
            '\\' => Ok(String::from("\\")),
            '\'' => Ok(String::from("'")),
            '"' => Ok(String::from("\"")),
            'x' => self.parse_hex_escape(2),
            'u' => self.parse_hex_escape(4),
            'U' => self.parse_hex_escape(8),
            'N' => Err(self.error("unicode name escapes are not supported")),
            other => Err(self.error(format!("unknown escape '\\{other}'"))),
        }
    }

    fn parse_hex_escape(&mut self, length: usize) -> Result<String, SnbtError> {
        let mut value: u32 = 0;
        for _ in 0..length {
            let digit = match self.peek_raw().and_then(|character| character.to_digit(16)) {
                Some(digit) => digit,
                None => {
                    return Err(self.error(format!("expected {length} hexadecimal digits")));
                }
            };
            value = value * 16 + digit;
            self.pos += 1;
        }
        if value > 0x10ffff {
            return Err(self.error(format!("invalid code point U+{value:08X}")));
        }

        // Java 的字符串允许半个代理项；Rust 的 String 不允许。接连的两个代理项转义
        // 还原成一个增补字符，落单的只能报错。
        if (0xd800..=0xdbff).contains(&value) {
            let after_high = self.pos;
            self.skip_whitespace();
            if self.peek_raw() == Some('\\') {
                self.pos += 1;
                self.skip_whitespace();
                if self.peek_raw() == Some('u') {
                    self.pos += 1;
                    if let Some(low) = self.try_hex_escape_value(4)
                        && (0xdc00..=0xdfff).contains(&low)
                    {
                        let code_point = 0x10000 + ((value - 0xd800) << 10) + (low - 0xdc00);
                        return Ok(String::from(
                            char::from_u32(code_point).unwrap_or('\u{fffd}'),
                        ));
                    }
                }
            }
            self.pos = after_high;
            return Err(self.error("unpaired surrogate code point"));
        }
        if (0xdc00..=0xdfff).contains(&value) {
            return Err(self.error("unpaired surrogate code point"));
        }
        match char::from_u32(value) {
            Some(character) => Ok(String::from(character)),
            None => Err(self.error(format!("invalid code point U+{value:08X}"))),
        }
    }

    fn try_hex_escape_value(&mut self, length: usize) -> Option<u32> {
        let mut value: u32 = 0;
        for _ in 0..length {
            let digit = self.peek_raw()?.to_digit(16)?;
            value = value * 16 + digit;
            self.pos += 1;
        }
        Some(value)
    }

    /// 官方 `map_literal`。
    fn parse_map(&mut self) -> Result<Tag, SnbtError> {
        self.enter_depth()?;
        let mut entries: Vec<(String, Tag)> = Vec::new();
        self.skip_whitespace();
        if self.peek_raw() != Some('}') {
            loop {
                let key = self.parse_map_key()?;
                if key.is_empty() {
                    return Err(self.error("map key cannot be empty"));
                }
                self.expect_char(':')?;
                let value = self.parse_literal()?;
                match entries.iter_mut().find(|(existing, _)| *existing == key) {
                    Some(entry) => entry.1 = value,
                    None => entries.push((key, value)),
                }
                if self.try_char(',') {
                    self.skip_whitespace();
                    if self.peek_raw() == Some('}') {
                        break;
                    }
                    continue;
                }
                break;
            }
        }
        self.expect_char('}')?;
        self.depth -= 1;
        Ok(Tag::Compound(entries))
    }

    /// 官方 `map_key`：带引号的字符串或不带引号的字符串。
    fn parse_map_key(&mut self) -> Result<String, SnbtError> {
        self.skip_whitespace();
        match self.peek_raw() {
            Some(quote @ ('"' | '\'')) => {
                self.pos += 1;
                let contents = self.parse_quoted_contents(quote)?;
                self.expect_char(quote)?;
                Ok(contents)
            }
            _ => self.read_unquoted(),
        }
    }

    /// 官方 `list_literal`：先试数组前缀，不行再当普通列表。
    fn parse_list(&mut self) -> Result<Tag, SnbtError> {
        self.enter_depth()?;
        let start = self.pos;
        if let Some(kind) = self.try_array_prefix()
            && self.try_char(';')
        {
            let entries = self.parse_integer_literals()?;
            self.expect_char(']')?;
            let tag = self.build_array(kind, entries)?;
            self.depth -= 1;
            return Ok(tag);
        }
        self.pos = start;
        let entries = self.parse_list_entries()?;
        self.expect_char(']')?;
        self.depth -= 1;
        Ok(Tag::List(entries))
    }

    fn try_array_prefix(&mut self) -> Option<ArrayKind> {
        match self.try_any_char('B', 'B') {
            Some(_) => Some(ArrayKind::Byte),
            None => match self.try_any_char('L', 'L') {
                Some(_) => Some(ArrayKind::Long),
                None => self.try_any_char('I', 'I').map(|_| ArrayKind::Int),
            },
        }
    }

    /// 数组元素必须是整数字面量；空数组也算合法（官方 `[I;]` 能解析）。
    fn parse_integer_literals(&mut self) -> Result<Vec<IntegerLiteral>, SnbtError> {
        let mut entries = Vec::new();
        self.skip_whitespace();
        if self.peek_raw() == Some(']') {
            return Ok(entries);
        }
        loop {
            match self.try_integer_literal() {
                Some(literal) => entries.push(literal),
                None => return Err(self.error("expected an integer literal")),
            }
            if self.try_char(',') {
                self.skip_whitespace();
                if self.peek_raw() == Some(']') {
                    break;
                }
                continue;
            }
            break;
        }
        Ok(entries)
    }

    fn parse_list_entries(&mut self) -> Result<Vec<Tag>, SnbtError> {
        let mut entries = Vec::new();
        self.skip_whitespace();
        if self.peek_raw() == Some(']') {
            return Ok(entries);
        }
        loop {
            entries.push(self.parse_literal()?);
            if self.try_char(',') {
                self.skip_whitespace();
                if self.peek_raw() == Some(']') {
                    break;
                }
                continue;
            }
            break;
        }
        Ok(entries)
    }

    /// 官方 `ArrayPrefix.create`：定下元素类型，再按目标数组收窄。
    fn build_array(&self, kind: ArrayKind, entries: Vec<IntegerLiteral>) -> Result<Tag, SnbtError> {
        let (default_type, allowed): (TypeSuffix, &[TypeSuffix]) = match kind {
            ArrayKind::Byte => (TypeSuffix::Byte, &[TypeSuffix::Byte]),
            ArrayKind::Int => (
                TypeSuffix::Int,
                &[TypeSuffix::Int, TypeSuffix::Byte, TypeSuffix::Short],
            ),
            ArrayKind::Long => (
                TypeSuffix::Long,
                &[
                    TypeSuffix::Long,
                    TypeSuffix::Byte,
                    TypeSuffix::Short,
                    TypeSuffix::Int,
                ],
            ),
        };

        let mut bytes = Vec::new();
        let mut ints = Vec::new();
        let mut longs = Vec::new();
        for entry in entries {
            let actual = entry.suffix.kind.unwrap_or(default_type);
            if !allowed.contains(&actual) {
                return Err(self.error("invalid array element type"));
            }
            let value = create_integer(
                entry.sign,
                entry.base,
                &entry.digits,
                IntegerSuffix {
                    signed: entry.suffix.signed,
                    kind: Some(actual),
                },
            )
            .ok_or_else(|| self.error("array element is out of range"))?;
            match (kind, value) {
                (ArrayKind::Byte, Tag::Byte(value)) => bytes.push(value),
                (ArrayKind::Int, Tag::Byte(value)) => ints.push(i32::from(value)),
                (ArrayKind::Int, Tag::Short(value)) => ints.push(i32::from(value)),
                (ArrayKind::Int, Tag::Int(value)) => ints.push(value),
                (ArrayKind::Long, Tag::Byte(value)) => longs.push(i64::from(value)),
                (ArrayKind::Long, Tag::Short(value)) => longs.push(i64::from(value)),
                (ArrayKind::Long, Tag::Int(value)) => longs.push(i64::from(value)),
                (ArrayKind::Long, Tag::Long(value)) => longs.push(value),
                _ => return Err(self.error("invalid array element type")),
            }
        }

        Ok(match kind {
            ArrayKind::Byte => Tag::ByteArray(bytes),
            ArrayKind::Int => Tag::IntArray(ints),
            ArrayKind::Long => Tag::LongArray(longs),
        })
    }

    /// 官方 `unquoted_string_or_builtin`。
    fn parse_unquoted_or_builtin(&mut self) -> Result<Tag, SnbtError> {
        let contents = self.read_unquoted()?;
        let starts_with_number = contents.chars().next().is_some_and(can_start_number);
        if starts_with_number {
            return Err(self.error("unquoted string cannot start with a number"));
        }

        let start = self.pos;
        if self.try_char('(') {
            let arguments = self.parse_arguments()?;
            self.expect_char(')')?;
            return self.run_builtin(&contents, arguments);
        }
        self.pos = start;

        if contents.eq_ignore_ascii_case("true") {
            Ok(Tag::Byte(1))
        } else if contents.eq_ignore_ascii_case("false") {
            Ok(Tag::Byte(0))
        } else {
            Ok(Tag::String(contents))
        }
    }

    fn parse_arguments(&mut self) -> Result<Vec<Tag>, SnbtError> {
        let mut arguments = Vec::new();
        self.skip_whitespace();
        if self.peek_raw() == Some(')') {
            return Ok(arguments);
        }
        loop {
            arguments.push(self.parse_literal()?);
            if self.try_char(',') {
                self.skip_whitespace();
                if self.peek_raw() == Some(')') {
                    break;
                }
                continue;
            }
            break;
        }
        Ok(arguments)
    }

    /// 官方 `SnbtOperations.BUILTIN_OPERATIONS`：只认 `bool/1` 和 `uuid/1`。
    fn run_builtin(&self, name: &str, arguments: Vec<Tag>) -> Result<Tag, SnbtError> {
        match (name, arguments.as_slice()) {
            ("bool", [argument]) => {
                let value = match number_of(argument) {
                    Some(value) => value != 0.0,
                    None => return Err(self.error("expected a number")),
                };
                Ok(Tag::Byte(i8::from(value)))
            }
            ("uuid", [Tag::String(value)]) => match java_uuid_to_int_array(value) {
                Some(values) => Ok(Tag::IntArray(values.to_vec())),
                None => Err(self.error("expected a valid UUID string")),
            },
            ("uuid", [_]) => Err(self.error("expected a string UUID")),
            _ => Err(self.error(format!("no such operation {name}/{}", arguments.len()))),
        }
    }

    /// 官方 `UnquotedStringParseRule`：至少一个 `[0-9A-Za-z_.+-]`。
    fn read_unquoted(&mut self) -> Result<String, SnbtError> {
        self.skip_whitespace();
        let start = self.pos;
        while let Some(character) = self.peek_raw() {
            if !is_unquoted_string_char(character) {
                break;
            }
            self.pos += 1;
        }
        if self.pos == start {
            return Err(self.error("expected an unquoted string"));
        }
        Ok(self.input[start..self.pos].iter().collect())
    }
}

/// 官方 `canStartNumber`：数字和小数点、正负号才可能是数字的开头。
fn can_start_number(character: char) -> bool {
    matches!(character, '+' | '-' | '.' | '0'..='9')
}

/// 官方 Brigadier `StringReader.isAllowedInUnquotedString`。
fn is_unquoted_string_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '+')
}

/// 官方 `Character.isWhitespace`。和 Rust 的 `char::is_whitespace` 不完全一样：
/// 不换行空格（U+00A0）在 Java 里不算空白，而 U+001C..=U+001F 算。
fn is_java_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{9}'..='\u{d}'
            | '\u{1c}'..='\u{20}'
            | '\u{1680}'
            | '\u{2000}'..='\u{2006}'
            | '\u{2008}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

fn push_cleaned(out: &mut String, contents: &str) {
    if contents.contains('_') {
        out.extend(contents.chars().filter(|character| *character != '_'));
    } else {
        out.push_str(contents);
    }
}

/// 官方 `createFloat`：把各段拼回一个 Java 能解析的数字串，再按后缀选精度。
fn build_float(
    sign: Option<Sign>,
    whole: Option<&str>,
    fraction: Option<&str>,
    exponent: Option<(Sign, String)>,
    suffix: Option<TypeSuffix>,
) -> Option<Tag> {
    let mut text = String::new();
    if sign == Some(Sign::Minus) {
        text.push('-');
    }
    if let Some(whole) = whole {
        push_cleaned(&mut text, whole);
    }
    if let Some(fraction) = fraction {
        text.push('.');
        push_cleaned(&mut text, fraction);
    }
    if let Some((exponent_sign, digits)) = exponent {
        text.push('e');
        if exponent_sign == Sign::Minus {
            text.push('-');
        }
        push_cleaned(&mut text, &digits);
    }

    match suffix {
        Some(TypeSuffix::Float) => {
            let value: f32 = text.parse().ok()?;
            value.is_finite().then_some(Tag::Float(value))
        }
        Some(TypeSuffix::Double) | None => {
            let value: f64 = text.parse().ok()?;
            value.is_finite().then_some(Tag::Double(value))
        }
        Some(TypeSuffix::Byte | TypeSuffix::Short | TypeSuffix::Int | TypeSuffix::Long) => None,
    }
}

/// 官方 `IntegerLiteral.create`：按有无符号和类型把数字串解析成标签。
fn create_integer(
    sign: Option<Sign>,
    base: IntBase,
    digits: &str,
    suffix: IntegerSuffix,
) -> Option<Tag> {
    let signed = suffix.signed.unwrap_or(match base {
        IntBase::Binary | IntBase::Hex => SignedPrefix::Unsigned,
        IntBase::Decimal => SignedPrefix::Signed,
    });
    if signed == SignedPrefix::Unsigned && sign == Some(Sign::Minus) {
        return None;
    }

    let mut text = String::new();
    if sign == Some(Sign::Minus) {
        text.push('-');
    }
    push_cleaned(&mut text, digits);

    let radix = match base {
        IntBase::Binary => 2,
        IntBase::Decimal => 10,
        IntBase::Hex => 16,
    };
    let kind = suffix.kind.unwrap_or(TypeSuffix::Int);
    match (signed, kind) {
        (SignedPrefix::Signed, TypeSuffix::Byte) => {
            i8::from_str_radix(&text, radix).ok().map(Tag::Byte)
        }
        (SignedPrefix::Signed, TypeSuffix::Short) => {
            i16::from_str_radix(&text, radix).ok().map(Tag::Short)
        }
        (SignedPrefix::Signed, TypeSuffix::Int) => {
            i32::from_str_radix(&text, radix).ok().map(Tag::Int)
        }
        (SignedPrefix::Signed, TypeSuffix::Long) => {
            i64::from_str_radix(&text, radix).ok().map(Tag::Long)
        }
        (SignedPrefix::Unsigned, TypeSuffix::Byte) => u8::from_str_radix(&text, radix)
            .ok()
            .map(|value| Tag::Byte(value as i8)),
        (SignedPrefix::Unsigned, TypeSuffix::Short) => u16::from_str_radix(&text, radix)
            .ok()
            .map(|value| Tag::Short(value as i16)),
        (SignedPrefix::Unsigned, TypeSuffix::Int) => u32::from_str_radix(&text, radix)
            .ok()
            .map(|value| Tag::Int(value as i32)),
        (SignedPrefix::Unsigned, TypeSuffix::Long) => u64::from_str_radix(&text, radix)
            .ok()
            .map(|value| Tag::Long(value as i64)),
        (_, TypeSuffix::Float | TypeSuffix::Double) => None,
    }
}

/// 官方 `NbtOps.getNumberValue` 认得的所有数字标签。
fn number_of(tag: &Tag) -> Option<f64> {
    match tag {
        Tag::Byte(value) => Some(f64::from(*value)),
        Tag::Short(value) => Some(f64::from(*value)),
        Tag::Int(value) => Some(f64::from(*value)),
        Tag::Long(value) => Some(*value as f64),
        Tag::Float(value) => Some(f64::from(*value)),
        Tag::Double(value) => Some(*value),
        _ => None,
    }
}

/// 官方 `UUID.fromString`：五段十六进制，每段解析后左移十六位再拼，超出的高位丢弃。
fn java_uuid_to_int_array(input: &str) -> Option<[i32; 4]> {
    let mut parts: Vec<&str> = input.split('-').collect();
    while parts.last() == Some(&"") {
        parts.pop();
    }
    if parts.len() != 5 {
        return None;
    }

    let most_high = java_decode_hex_part(parts[0])?;
    let most_mid = java_decode_hex_part(parts[1])?;
    let most_low = java_decode_hex_part(parts[2])?;
    let least_high = java_decode_hex_part(parts[3])?;
    let least_low = java_decode_hex_part(parts[4])?;

    let most = ((most_high << 16) | most_mid) << 16 | most_low;
    let least = (least_high << 16) | least_low;
    Some([
        (most >> 32) as i32,
        most as i32,
        (least >> 32) as i32,
        least as i32,
    ])
}

/// 官方 `Long.decode("0x" + part)`，只处理这里固定的十六进制前缀。
fn java_decode_hex_part(part: &str) -> Option<i64> {
    let (negative, digits) = match part.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, part),
    };
    let digits = digits.strip_prefix('+').unwrap_or(digits);
    if digits.is_empty() {
        return None;
    }
    let value = i64::from_str_radix(digits, 16).ok()?;
    Some(if negative { -value } else { value })
}

/// 从浮点数的科学计数法表示还原 Java `Double.toString` / `Float.toString` 的格式。
///
/// Java 的规则：绝对值在 `[1e-3, 1e7)` 内用普通小数（至少一位小数），否则用
/// `d.dddE±e`，指数不带 `+`。
fn java_number_from_scientific(scientific: &str) -> String {
    let (mantissa, exponent) = match scientific.split_once('e') {
        Some(parts) => parts,
        None => return scientific.to_owned(),
    };
    let (negative, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, mantissa),
    };
    let digits: String = mantissa
        .chars()
        .filter(|character| *character != '.')
        .collect();
    let exponent: i32 = match exponent.parse() {
        Ok(exponent) => exponent,
        Err(_) => return scientific.to_owned(),
    };

    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if (-3..7).contains(&exponent) {
        if exponent >= 0 {
            let whole_length = exponent as usize + 1;
            for index in 0..whole_length {
                out.push(
                    digits
                        .as_bytes()
                        .get(index)
                        .map_or('0', |byte| *byte as char),
                );
            }
            out.push('.');
            if whole_length < digits.len() {
                out.push_str(&digits[whole_length..]);
            } else {
                out.push('0');
            }
        } else {
            out.push_str("0.");
            for _ in 0..(-exponent - 1) {
                out.push('0');
            }
            out.push_str(&digits);
        }
    } else {
        out.push_str(&digits[..1]);
        out.push('.');
        if digits.len() == 1 {
            out.push('0');
        } else {
            out.push_str(&digits[1..]);
        }
        out.push('E');
        out.push_str(&exponent.to_string());
    }
    out
}

/// Java `Double.toString`：最短往返数字加 Java 的排版规则。
pub fn java_double_to_string(value: f64) -> String {
    if value.is_nan() {
        return String::from("NaN");
    }
    if value.is_infinite() {
        return if value > 0.0 {
            String::from("Infinity")
        } else {
            String::from("-Infinity")
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            String::from("-0.0")
        } else {
            String::from("0.0")
        };
    }
    java_number_from_scientific(&format!("{value:e}"))
}

/// Java `Float.toString`。
pub fn java_float_to_string(value: f32) -> String {
    if value.is_nan() {
        return String::from("NaN");
    }
    if value.is_infinite() {
        return if value > 0.0 {
            String::from("Infinity")
        } else {
            String::from("-Infinity")
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            String::from("-0.0")
        } else {
            String::from("0.0")
        };
    }
    java_number_from_scientific(&format!("{value:e}"))
}

impl Tag {
    /// 官方 `StringTagVisitor` 的紧凑形式，也就是 `Tag.toString()`。
    pub fn to_snbt(&self) -> String {
        let mut out = String::new();
        self.write_snbt(&mut out);
        out
    }

    fn write_snbt(&self, out: &mut String) {
        match self {
            Self::Byte(value) => {
                out.push_str(&value.to_string());
                out.push('b');
            }
            Self::Short(value) => {
                out.push_str(&value.to_string());
                out.push('s');
            }
            Self::Int(value) => out.push_str(&value.to_string()),
            Self::Long(value) => {
                out.push_str(&value.to_string());
                out.push('L');
            }
            Self::Float(value) => {
                out.push_str(&java_float_to_string(*value));
                out.push('f');
            }
            Self::Double(value) => {
                out.push_str(&java_double_to_string(*value));
                out.push('d');
            }
            Self::ByteArray(values) => {
                out.push_str("[B;");
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    out.push_str(&value.to_string());
                    out.push('B');
                }
                out.push(']');
            }
            Self::String(value) => out.push_str(&quote_and_escape(value)),
            Self::List(values) => {
                out.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    value.write_snbt(out);
                }
                out.push(']');
            }
            Self::Compound(entries) => {
                out.push('{');
                for (index, (key, value)) in sorted_entries(entries).enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    write_key_escape(key, out);
                    out.push(':');
                    value.write_snbt(out);
                }
                out.push('}');
            }
            Self::IntArray(values) => {
                out.push_str("[I;");
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    out.push_str(&value.to_string());
                }
                out.push(']');
            }
            Self::LongArray(values) => {
                out.push_str("[L;");
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    out.push_str(&value.to_string());
                    out.push('L');
                }
                out.push(']');
            }
        }
    }

    /// 官方 `SnbtPrinterTagVisitor`：四空格缩进，Minecraft 存档目录里几个固定的层级
    /// 不换行，键序也按 `KEY_ORDER` 排。
    pub fn to_pretty_snbt(&self) -> String {
        let mut out = String::new();
        let mut path = Vec::new();
        write_pretty_snbt(self, "    ", 0, &mut path, &mut out);
        out
    }
}

/// 官方的键序表：路径 -> 先排的键。
const KEY_ORDER: &[(&str, &[&str])] = &[
    (
        "{}",
        &[
            "DataVersion",
            "author",
            "size",
            "data",
            "entities",
            "palette",
            "palettes",
        ],
    ),
    ("{}.data.[].{}", &["pos", "state", "nbt"]),
    ("{}.entities.[].{}", &["blockPos", "pos"]),
];

/// 官方 `NO_INDENTATION`：这些路径下的列表和复合标签不换行。
const NO_INDENTATION: &[&str] = &[
    "{}.size.[]",
    "{}.data.[].{}",
    "{}.palette.[].{}",
    "{}.entities.[].{}",
];

fn path_string(path: &[String]) -> String {
    path.join(".")
}

fn write_pretty_snbt(
    tag: &Tag,
    indentation: &str,
    depth: usize,
    path: &mut Vec<String>,
    out: &mut String,
) {
    match tag {
        Tag::String(_)
        | Tag::Byte(_)
        | Tag::Short(_)
        | Tag::Int(_)
        | Tag::Long(_)
        | Tag::Float(_)
        | Tag::Double(_) => tag.write_snbt(out),
        Tag::ByteArray(values) => {
            out.push_str("[B;");
            for (index, value) in values.iter().enumerate() {
                out.push(' ');
                out.push_str(&value.to_string());
                out.push('B');
                if index != values.len() - 1 {
                    out.push(',');
                }
            }
            out.push(']');
        }
        Tag::IntArray(values) => {
            out.push_str("[I;");
            for (index, value) in values.iter().enumerate() {
                out.push(' ');
                out.push_str(&value.to_string());
                if index != values.len() - 1 {
                    out.push(',');
                }
            }
            out.push(']');
        }
        Tag::LongArray(values) => {
            out.push_str("[L;");
            for (index, value) in values.iter().enumerate() {
                out.push(' ');
                out.push_str(&value.to_string());
                out.push('L');
                if index != values.len() - 1 {
                    out.push(',');
                }
            }
            out.push(']');
        }
        Tag::List(values) => {
            if values.is_empty() {
                out.push_str("[]");
                return;
            }
            path.push(String::from("[]"));
            let indentation = if NO_INDENTATION.contains(&path_string(path).as_str()) {
                ""
            } else {
                indentation
            };
            out.push('[');
            if !indentation.is_empty() {
                out.push('\n');
            }
            for (index, value) in values.iter().enumerate() {
                for _ in 0..depth + 1 {
                    out.push_str(indentation);
                }
                write_pretty_snbt(value, indentation, depth + 1, path, out);
                if index != values.len() - 1 {
                    out.push(',');
                    out.push_str(if indentation.is_empty() { " " } else { "\n" });
                }
            }
            if !indentation.is_empty() {
                out.push('\n');
                for _ in 0..depth {
                    out.push_str(indentation);
                }
            }
            out.push(']');
            path.pop();
        }
        Tag::Compound(entries) => {
            if entries.is_empty() {
                out.push_str("{}");
                return;
            }
            path.push(String::from("{}"));
            let current_path = path_string(path);
            let indentation = if NO_INDENTATION.contains(&current_path.as_str()) {
                ""
            } else {
                indentation
            };
            let keys = pretty_keys(&current_path, entries);
            out.push('{');
            if !indentation.is_empty() {
                out.push('\n');
            }
            for (index, key) in keys.iter().enumerate() {
                let value = entries
                    .iter()
                    .find(|(name, _)| name.as_str() == key.as_str())
                    .map(|(_, value)| value);
                let value = match value {
                    Some(value) => value,
                    None => continue,
                };
                for _ in 0..depth + 1 {
                    out.push_str(indentation);
                }
                out.push_str(&handle_escape_pretty(key));
                out.push_str(": ");
                path.push(key.to_string());
                write_pretty_snbt(value, indentation, depth + 1, path, out);
                path.pop();
                if index != keys.len() - 1 {
                    out.push(',');
                    out.push_str(if indentation.is_empty() { " " } else { "\n" });
                }
            }
            if !indentation.is_empty() {
                out.push('\n');
                for _ in 0..depth {
                    out.push_str(indentation);
                }
            }
            out.push('}');
            path.pop();
        }
    }
}

/// 官方 `SnbtPrinterTagVisitor.getKeys`：先按 `KEY_ORDER`，剩下的排序。
fn pretty_keys<'a>(path: &str, entries: &'a [(String, Tag)]) -> Vec<&'a String> {
    let mut remaining: Vec<&String> = entries.iter().map(|(key, _)| key).collect();
    remaining.sort_by(|left, right| compare_utf16(left, right));
    let mut keys = Vec::new();
    if let Some((_, order)) = KEY_ORDER.iter().find(|(name, _)| *name == path) {
        for wanted in *order {
            if let Some(index) = remaining.iter().position(|key| key.as_str() == *wanted) {
                keys.push(remaining.remove(index));
            }
        }
    }
    keys.extend(remaining);
    keys
}

/// 官方 `StringTagVisitor` 和 `SnbtPrinterTagVisitor` 都按 Java 字符串比较排序，
/// 也就是按 UTF-16 码元比较。
fn compare_utf16(left: &str, right: &str) -> std::cmp::Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

/// 官方 `CompoundTag` 排序后的条目。重复键在官方 `HashMap` 里不可能出现，这里保持原样。
fn sorted_entries(entries: &[(String, Tag)]) -> impl Iterator<Item = (&String, &Tag)> {
    let mut sorted: Vec<(&String, &Tag)> = entries.iter().map(|(key, tag)| (key, tag)).collect();
    sorted.sort_by(|left, right| compare_utf16(left.0, right.0));
    sorted.into_iter()
}

/// 官方 `StringTagVisitor.handleKeyEscape`：`[A-Za-z._]+[A-Za-z0-9._+-]*`
/// 且不是 `true`/`false`（不分大小写）的键不加引号。
fn write_key_escape(key: &str, out: &mut String) {
    if !key.eq_ignore_ascii_case("true") && !key.eq_ignore_ascii_case("false") && is_simple_key(key)
    {
        out.push_str(key);
    } else {
        out.push_str(&quote_and_escape(key));
    }
}

fn is_simple_key(key: &str) -> bool {
    let mut characters = key.chars();
    match characters.next() {
        Some(first) if first.is_ascii_alphabetic() || matches!(first, '.' | '_') => {}
        _ => return false,
    }
    characters.all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '+' | '-')
    })
}

/// 官方 `SnbtPrinterTagVisitor.handleEscapePretty`：`[A-Za-z0-9._+-]+` 全匹配才不加引号。
fn handle_escape_pretty(key: &str) -> String {
    let simple = !key.is_empty()
        && key.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '+' | '-')
        });
    if simple {
        key.to_owned()
    } else {
        quote_and_escape(key)
    }
}

/// 官方 `SnbtGrammar.escapeControlCharacters`，返回要跟在反斜杠后的文本。
fn escape_control_character(character: char) -> Option<String> {
    match character {
        '\u{8}' => Some(String::from("b")),
        '\t' => Some(String::from("t")),
        '\n' => Some(String::from("n")),
        '\u{c}' => Some(String::from("f")),
        '\r' => Some(String::from("r")),
        character if character < ' ' => Some(format!("x{:02X}", character as u32)),
        _ => None,
    }
}

/// 官方 `StringTag.quoteAndEscape`：优先用单引号，字符串里有单引号才用双引号。
pub fn quote_and_escape(input: &str) -> String {
    let mut body = String::new();
    let mut quote = None;
    for character in input.chars() {
        match character {
            '\\' => body.push_str("\\\\"),
            '"' | '\'' => {
                if quote.is_none() {
                    quote = Some(if character == '"' { '\'' } else { '"' });
                }
                if quote == Some(character) {
                    body.push('\\');
                }
                body.push(character);
            }
            other => match escape_control_character(other) {
                Some(escaped) => {
                    body.push('\\');
                    body.push_str(&escaped);
                }
                None => body.push(other),
            },
        }
    }
    let quote = quote.unwrap_or('"');
    let mut out = String::with_capacity(body.len() + 2);
    out.push(quote);
    out.push_str(&body);
    out.push(quote);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Tag {
        match parse_snbt(input) {
            Ok(tag) => tag,
            Err(error) => panic!("failed to parse {input:?}: {error}"),
        }
    }

    #[test]
    fn parses_integers() {
        assert_eq!(parse("0"), Tag::Int(0));
        assert_eq!(parse("123"), Tag::Int(123));
        assert_eq!(parse("-123"), Tag::Int(-123));
        assert_eq!(parse("+123"), Tag::Int(123));
        assert_eq!(parse("1_000"), Tag::Int(1000));
        assert_eq!(parse("1b"), Tag::Byte(1));
        assert_eq!(parse("1s"), Tag::Short(1));
        assert_eq!(parse("1i"), Tag::Int(1));
        assert_eq!(parse("1L"), Tag::Long(1));
        assert_eq!(parse("255ub"), Tag::Byte(-1));
        assert_eq!(parse("65535us"), Tag::Short(-1));
        assert_eq!(parse("4294967295ui"), Tag::Int(-1));
        assert_eq!(parse("18446744073709551615ul"), Tag::Long(-1));
        assert_eq!(parse("-128sb"), Tag::Byte(-128));
        assert_eq!(parse("0xFF"), Tag::Int(255));
        assert_eq!(parse("0xFFFFFFFF"), Tag::Int(-1));
        assert_eq!(parse("0b1010"), Tag::Int(10));
        assert_eq!(parse("0xFFFFus"), Tag::Short(-1));
        assert_eq!(parse("0xFFub"), Tag::Byte(-1));
        assert_eq!(parse("0b"), Tag::Byte(0));
    }

    #[test]
    fn parses_floats() {
        assert_eq!(parse("1.0"), Tag::Double(1.0));
        assert_eq!(parse("1."), Tag::Double(1.0));
        assert_eq!(parse(".5"), Tag::Double(0.5));
        assert_eq!(parse("-0.5"), Tag::Double(-0.5));
        assert_eq!(parse("1e5"), Tag::Double(100000.0));
        assert_eq!(parse("1.5e-2"), Tag::Double(0.015));
        assert_eq!(parse("1.5f"), Tag::Float(1.5));
        assert_eq!(parse("1.5d"), Tag::Double(1.5));
        assert_eq!(parse("1f"), Tag::Float(1.0));
        assert_eq!(parse("1_0.0"), Tag::Double(10.0));
        assert_eq!(parse("1 . 5"), Tag::Double(1.5));
    }

    #[test]
    fn parses_strings() {
        assert_eq!(parse("\"hello\""), Tag::String(String::from("hello")));
        assert_eq!(parse("'hello'"), Tag::String(String::from("hello")));
        assert_eq!(parse("\"a\\nb\""), Tag::String(String::from("a\nb")));
        assert_eq!(parse("\"a\\x41b\""), Tag::String(String::from("aAb")));
        assert_eq!(parse("\"\\u00e9\""), Tag::String(String::from("é")));
        assert_eq!(
            parse("\"\\U0001F600\""),
            Tag::String(String::from("\u{1f600}"))
        );
        assert_eq!(
            parse("\"\\uD83D\\uDE00\""),
            Tag::String(String::from("\u{1f600}"))
        );
        assert_eq!(
            parse("plain_text-1.5+"),
            Tag::String(String::from("plain_text-1.5+"))
        );
        assert_eq!(parse("\"it's\""), Tag::String(String::from("it's")));
        assert_eq!(
            parse("'say \"hi\"'"),
            Tag::String(String::from("say \"hi\""))
        );
    }

    #[test]
    fn parses_builtins() {
        assert_eq!(parse("true"), Tag::Byte(1));
        assert_eq!(parse("FALSE"), Tag::Byte(0));
        assert_eq!(parse("bool(1b)"), Tag::Byte(1));
        assert_eq!(parse("bool(0.0)"), Tag::Byte(0));
        assert_eq!(
            parse("uuid(\"1-1-1-1-1\")"),
            Tag::IntArray(vec![1, 65537, 0, 65537])
        );
        assert!(parse_snbt("bool(\"x\")").is_err());
        assert!(parse_snbt("uuid(1)").is_err());
        assert!(parse_snbt("nope(1)").is_err());
    }

    #[test]
    fn parses_containers() {
        assert_eq!(parse("{}"), Tag::Compound(Vec::new()));
        assert_eq!(parse("[]"), Tag::List(Vec::new()));
        assert_eq!(
            parse("{a:1b,b:[1,2],c:{d:\"e\"}}"),
            Tag::Compound(vec![
                (String::from("a"), Tag::Byte(1)),
                (String::from("b"), Tag::List(vec![Tag::Int(1), Tag::Int(2)])),
                (
                    String::from("c"),
                    Tag::Compound(vec![(String::from("d"), Tag::String(String::from("e")))])
                ),
            ])
        );
        assert_eq!(
            parse("{a:1,a:2}"),
            Tag::Compound(vec![(String::from("a"), Tag::Int(2))])
        );
        assert_eq!(parse("{a:1,}"), parse("{a:1}"));
        assert_eq!(parse("[1,2,]"), parse("[1,2]"));
        assert_eq!(parse("[B;1b,2b]"), Tag::ByteArray(vec![1, 2]));
        assert_eq!(parse("[I;]"), Tag::IntArray(Vec::new()));
        assert_eq!(parse("[I;1,2b,3s]"), Tag::IntArray(vec![1, 2, 3]));
        assert_eq!(parse("[L;1]"), Tag::LongArray(vec![1]));
        assert_eq!(
            parse("[I]"),
            Tag::List(vec![Tag::String(String::from("I"))])
        );
        assert_eq!(parse("[B;0xFFub]"), Tag::ByteArray(vec![-1]));
        // `b` 本身是十六进制数字，所以 `0xFFb` 读的是 0xFFB，装不进 byte。
        assert!(parse_snbt("[B;0xFFb]").is_err());
    }

    #[test]
    fn rejects_invalid_input() {
        for input in [
            "",
            "00",
            "01",
            "0x",
            "0xZB",
            "1.0L",
            "1.0x",
            "9e999",
            "\"abc",
            "{",
            "{a:}",
            "{\"\":1}",
            "{:1}",
            "{a:1} extra",
            "[1,,2]",
            "[B;300]",
            "[B;1i]",
            "[I;1L]",
            "[L;1.0]",
            "-1ub",
            "256ub",
            "65536us",
            "\"\\q\"",
            "\"\\uD800\"",
            "()",
        ] {
            assert!(parse_snbt(input).is_err(), "expected {input:?} to fail");
        }
        assert!(parse_snbt_compound("[1]").is_err());
    }

    #[test]
    fn prints_compact() {
        assert_eq!(Tag::Int(5).to_snbt(), "5");
        assert_eq!(Tag::Byte(5).to_snbt(), "5b");
        assert_eq!(Tag::Long(5).to_snbt(), "5L");
        assert_eq!(Tag::Float(1.5).to_snbt(), "1.5f");
        assert_eq!(Tag::Double(1.0).to_snbt(), "1.0d");
        assert_eq!(Tag::ByteArray(vec![1, -1]).to_snbt(), "[B;1B,-1B]");
        assert_eq!(Tag::IntArray(vec![1, 2]).to_snbt(), "[I;1,2]");
        assert_eq!(Tag::LongArray(vec![1]).to_snbt(), "[L;1L]");
        assert_eq!(
            Tag::Compound(vec![
                (String::from("b"), Tag::Int(1)),
                (String::from("a"), Tag::String(String::from("x"))),
            ])
            .to_snbt(),
            "{a:\"x\",b:1}"
        );
        assert_eq!(Tag::String(String::from("a b")).to_snbt(), "\"a b\"");
        assert_eq!(Tag::String(String::from("it's")).to_snbt(), "\"it's\"");
        assert_eq!(
            Tag::Compound(vec![(String::from("true"), Tag::Int(1))]).to_snbt(),
            "{\"true\":1}"
        );
    }

    #[test]
    fn prints_pretty() {
        let tag = Tag::Compound(vec![
            (String::from("DataVersion"), Tag::Int(3953)),
            (
                String::from("data"),
                Tag::List(vec![Tag::Compound(vec![
                    (String::from("state"), Tag::Int(0)),
                    (
                        String::from("pos"),
                        Tag::List(vec![Tag::Int(1), Tag::Int(2)]),
                    ),
                ])]),
            ),
        ]);
        assert_eq!(
            tag.to_pretty_snbt(),
            "{\n    DataVersion: 3953,\n    data: [\n        {pos: [1, 2], state: 0}\n    ]\n}"
        );
        assert_eq!(Tag::List(Vec::new()).to_pretty_snbt(), "[]");
        assert_eq!(Tag::ByteArray(vec![1, 2]).to_pretty_snbt(), "[B; 1B, 2B]");
    }

    #[test]
    fn java_float_formatting() {
        assert_eq!(java_double_to_string(1.0), "1.0");
        assert_eq!(java_double_to_string(0.1), "0.1");
        assert_eq!(java_double_to_string(100.0), "100.0");
        assert_eq!(java_double_to_string(9999999.0), "9999999.0");
        assert_eq!(java_double_to_string(1e7), "1.0E7");
        assert_eq!(java_double_to_string(1e-3), "0.001");
        assert_eq!(java_double_to_string(1e-4), "1.0E-4");
        assert_eq!(java_double_to_string(123456789.0), "1.23456789E8");
        assert_eq!(java_double_to_string(-0.0), "-0.0");
        assert_eq!(java_double_to_string(1.0 / 3.0), "0.3333333333333333");
        assert_eq!(java_double_to_string(f64::NAN), "NaN");
        assert_eq!(java_double_to_string(f64::INFINITY), "Infinity");
        assert_eq!(java_float_to_string(1e10), "1.0E10");
        assert_eq!(java_float_to_string(0.001), "0.001");
    }

    #[test]
    fn round_trips_through_snbt() {
        let tag = Tag::Compound(vec![
            (String::from("byte"), Tag::Byte(-1)),
            (String::from("short"), Tag::Short(-2)),
            (String::from("int"), Tag::Int(-3)),
            (String::from("long"), Tag::Long(-4)),
            (String::from("float"), Tag::Float(0.25)),
            (String::from("double"), Tag::Double(0.25)),
            (String::from("bytes"), Tag::ByteArray(vec![0, 127, -128])),
            (String::from("string"), Tag::String(String::from("a\nb\"c"))),
            (
                String::from("list"),
                Tag::List(vec![Tag::Int(1), Tag::String(String::from("two"))]),
            ),
            (String::from("empty"), Tag::List(Vec::new())),
            (String::from("ints"), Tag::IntArray(vec![1, -2])),
            (String::from("longs"), Tag::LongArray(vec![1, -2])),
        ]);
        // 复合标签的键序由打印器排好，解析回来保持文本顺序，所以比较再打印一遍的结果。
        let text = tag.to_snbt();
        assert_eq!(parse(&text).to_snbt(), text);
        let pretty = tag.to_pretty_snbt();
        assert_eq!(parse(&pretty).to_snbt(), text);
    }
}
