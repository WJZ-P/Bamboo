//! Brigadier 的 `StringReader` 里文本组件用得到的那一部分。
//!
//! 游标按 UTF-16 码元走，和 Java 的 `String.charAt` 一样。BMP 里一个码元就是一个标量。
//! 代理项对不成标量，读出来是空，调用方停下。选择器里出现增补平面字符就整段拒绝。
//!
//! The slice of Brigadier's `StringReader` that chat components need.
//! The cursor counts UTF-16 code units, like Java's `String.charAt`. In the BMP one
//! unit is one scalar. A surrogate is not a scalar, so the read comes back empty and
//! the caller stops. A selector that contains a supplementary character is rejected.

use crate::components::cursor::parse_identifier;

/// 读一段选择器、NBT 路径或方块坐标。游标的单位是 UTF-16 码元。
pub(crate) struct Reader {
    units: Vec<u16>,
    pos: usize,
}

impl Reader {
    pub(crate) fn new(text: &str) -> Self {
        Self {
            units: text.encode_utf16().collect(),
            pos: 0,
        }
    }

    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    pub(crate) fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    pub(crate) fn can_read(&self) -> bool {
        self.pos < self.units.len()
    }

    /// 当前码元。代理项对不成一个标量时返回 `None`，调用方要停，不能空转。
    ///
    /// The current code unit. `None` when a surrogate does not form a scalar.
    /// Callers stop instead of spinning.
    pub(crate) fn peek(&self) -> Option<char> {
        self.unit_at(self.pos)
    }

    pub(crate) fn peek_at(&self, offset: usize) -> Option<char> {
        self.unit_at(self.pos + offset)
    }

    fn unit_at(&self, index: usize) -> Option<char> {
        char::from_u32(u32::from(*self.units.get(index)?))
    }

    pub(crate) fn skip(&mut self) {
        if self.pos < self.units.len() {
            self.pos += 1;
        }
    }

    pub(crate) fn copy(&self, start: usize, end: usize) -> String {
        String::from_utf16(&self.units[start..end]).unwrap_or_default()
    }

    /// 官方 `StringReader.skipWhitespace`，按 `Character.isWhitespace`。
    pub(crate) fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(java_whitespace) {
            self.skip();
        }
    }

    pub(crate) fn expect(&mut self, expected: char) -> Result<(), ()> {
        if self.peek() == Some(expected) {
            self.skip();
            Ok(())
        } else {
            Err(())
        }
    }

    /// 官方 `readString`：引号字符串，或者无引号的标识片段。
    pub(crate) fn read_string(&mut self) -> Result<String, ()> {
        match self.peek() {
            Some('"' | '\'') => self.read_quoted(),
            _ => Ok(self.read_unquoted()),
        }
    }

    pub(crate) fn read_unquoted(&mut self) -> String {
        let start = self.pos;
        while self.peek().is_some_and(unquoted) {
            self.skip();
        }
        self.copy(start, self.pos)
    }

    /// 官方 `readInt`。溢出或没有数字都是错。
    pub(crate) fn read_int(&mut self) -> Result<i32, ()> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.skip();
        }
        let digits = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.skip();
        }
        if self.pos == digits {
            self.pos = start;
            return Err(());
        }
        self.copy(start, self.pos).parse().map_err(|_| {
            self.pos = start;
        })
    }

    /// 官方 `readDouble`：减号、数字和小数点，再交给 `Double.parseDouble`。
    ///
    /// Brigadier `readDouble`: a minus, digits and dots, then `Double.parseDouble`.
    pub(crate) fn read_double(&mut self) -> Result<f64, ()> {
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_digit() || c == '.' || c == '-')
        {
            self.skip();
        }
        if self.pos == start {
            return Err(());
        }
        let text = self.copy(start, self.pos);
        text.parse().map_err(|_| {
            self.pos = start;
        })
    }

    /// 官方 `readBoolean`，只有 `true` 和 `false`。
    pub(crate) fn read_bool(&mut self) -> Result<bool, ()> {
        if self.starts_with("true") {
            self.pos += 4;
            Ok(true)
        } else if self.starts_with("false") {
            self.pos += 5;
            Ok(false)
        } else {
            Err(())
        }
    }

    /// 官方 `Identifier.read`，读完再按 `Identifier.parse` 校验。
    pub(crate) fn read_identifier(&mut self) -> Result<String, ()> {
        let start = self.pos;
        while self.peek().is_some_and(identifier_char) {
            self.skip();
        }
        if self.pos == start {
            return Err(());
        }
        let raw = self.copy(start, self.pos);
        parse_identifier(&raw).map_err(|_| ())
    }

    fn read_quoted(&mut self) -> Result<String, ()> {
        let quote = self.peek().ok_or(())?;
        self.skip();
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or(())?;
            self.skip();
            if c == '\\' {
                let escaped = self.peek().ok_or(())?;
                self.skip();
                if escaped != quote && escaped != '\\' {
                    return Err(());
                }
                out.push(escaped);
            } else if c == quote {
                return Ok(out);
            } else {
                out.push(c);
            }
        }
    }

    fn starts_with(&self, text: &str) -> bool {
        let prefix: Vec<u16> = text.encode_utf16().collect();
        self.units[self.pos..].starts_with(&prefix)
    }
}

/// 官方 `MinMaxBounds.Bounds.fromReader`。两端可以缺一边，`5..1` 在这里也合法。
///
/// `MinMaxBounds.Bounds.fromReader`. Either end may be absent, and `5..1` is legal here.
pub(crate) fn read_bounds(
    reader: &mut Reader,
    integer: bool,
) -> Result<(Option<f64>, Option<f64>), ()> {
    if !reader.can_read() {
        return Err(());
    }
    let min = read_bound_number(reader)?;
    let max = if reader.peek() == Some('.') && reader.peek_at(1) == Some('.') {
        reader.skip();
        reader.skip();
        read_bound_number(reader)?
    } else {
        min.clone()
    };
    if min.is_none() && max.is_none() {
        return Err(());
    }
    let parse = |text: String| -> Result<f64, ()> {
        if integer {
            text.parse::<i32>()
                .map(|value| value as f64)
                .map_err(|_| ())
        } else {
            text.parse::<f64>().map_err(|_| ())
        }
    };
    Ok((min.map(parse).transpose()?, max.map(parse).transpose()?))
}

fn read_bound_number(reader: &mut Reader) -> Result<Option<String>, ()> {
    let start = reader.pos();
    while let Some(c) = reader.peek() {
        let number = c.is_ascii_digit() || c == '-';
        let dot = c == '.' && reader.peek_at(1) != Some('.');
        if number || dot {
            reader.skip();
        } else {
            break;
        }
    }
    if reader.pos() == start {
        return Ok(None);
    }
    Ok(Some(reader.copy(start, reader.pos())))
}

/// 从当前位置读一个 SNBT 复合标签，成功就把游标移过它。
///
/// Read one SNBT compound and leave the cursor just past it.
pub(crate) fn take_compound(reader: &mut Reader) -> bool {
    if reader.peek() != Some('{') {
        return false;
    }
    let start = reader.pos();
    let Some(end) = compound_end(&reader.units[start..]) else {
        return false;
    };
    let Ok(text) = String::from_utf16(&reader.units[start..start + end]) else {
        return false;
    };
    if mc_bamboo_nbt::parse_snbt(&text).is_err() {
        return false;
    }
    reader.set_pos(start + end);
    true
}

fn compound_end(units: &[u16]) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote = None;
    let mut escaped = false;
    for (index, unit) in units.iter().copied().enumerate() {
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if unit == u16::from(b'\\') {
                escaped = true;
            } else if unit == open {
                quote = None;
            }
            continue;
        }
        match unit {
            unit if unit == u16::from(b'"') || unit == u16::from(b'\'') => quote = Some(unit),
            unit if unit == u16::from(b'{') => depth += 1,
            unit if unit == u16::from(b'}') => {
                depth -= 1;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// 官方 `BlockPosArgument.blockPos`。整段字符串都要被吃掉。
///
/// `BlockPosArgument.blockPos`. The whole string has to be consumed.
pub(crate) fn block_pos(text: &str) -> bool {
    let mut reader = Reader::new(text);
    let ok = if reader.peek() == Some('^') {
        local_pos(&mut reader)
    } else {
        world_pos(&mut reader)
    };
    ok && !reader.can_read()
}

fn world_pos(reader: &mut Reader) -> bool {
    world_coord(reader)
        && reader.expect(' ').is_ok()
        && world_coord(reader)
        && reader.expect(' ').is_ok()
        && world_coord(reader)
}

fn world_coord(reader: &mut Reader) -> bool {
    if reader.peek() == Some('^') || !reader.can_read() {
        return false;
    }
    let relative = reader.peek() == Some('~');
    if relative {
        reader.skip();
    }
    if reader.can_read() && reader.peek() != Some(' ') {
        if relative {
            reader.read_double().is_ok()
        } else {
            reader.read_int().is_ok()
        }
    } else {
        true
    }
}

fn local_pos(reader: &mut Reader) -> bool {
    local_coord(reader)
        && reader.expect(' ').is_ok()
        && local_coord(reader)
        && reader.expect(' ').is_ok()
        && local_coord(reader)
}

fn local_coord(reader: &mut Reader) -> bool {
    if reader.peek() != Some('^') {
        return false;
    }
    reader.skip();
    if reader.can_read() && reader.peek() != Some(' ') {
        reader.read_double().is_ok()
    } else {
        true
    }
}

fn java_whitespace(c: char) -> bool {
    matches!(
        c,
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

fn unquoted(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+')
}

fn identifier_char(c: char) -> bool {
    matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '/' | '.' | ':')
}
