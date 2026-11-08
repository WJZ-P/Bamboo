//! 官方的 `Identifier`：带命名空间的注册名，命令里到处都要读它。

use crate::{ParseError, reader::StringReader};

/// 一个注册名。总是带命名空间，没有写前缀时补 `minecraft`。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identifier {
    namespace: String,
    path: String,
}

impl Identifier {
    /// 按官方规则解析一个完整字符串，不做长度检查。判断是不是合法注册名时用 `parse` 而不是读命令。
    pub fn parse(raw: &str) -> Option<Self> {
        let (namespace, path) = match raw.split_once(':') {
            Some(("", path)) => ("minecraft", path),
            Some((namespace, path)) => (namespace, path),
            None => ("minecraft", raw),
        };
        if !is_valid_namespace(namespace) || !is_valid_path(path) {
            return None;
        }
        Some(Self {
            namespace: namespace.to_owned(),
            path: path.to_owned(),
        })
    }

    /// 读一段可以被注册名使用的字符。官方 `Identifier.readGreedy`。
    pub fn read(reader: &mut StringReader) -> Result<Self, ParseError> {
        let start = reader.cursor();
        let raw = read_greedy(reader);
        Self::parse(&raw).ok_or_else(|| {
            reader.set_cursor(start);
            ParseError::new("argument.id.invalid")
        })
    }

    /// 和 [`read`](Self::read) 一样，但不接受空字符串。物品组件名用这个。
    pub fn read_non_empty(reader: &mut StringReader) -> Result<Self, ParseError> {
        let start = reader.cursor();
        let raw = read_greedy(reader);
        if raw.is_empty() {
            return Err(ParseError::new("argument.id.invalid"));
        }
        Self::parse(&raw).ok_or_else(|| {
            reader.set_cursor(start);
            ParseError::new("argument.id.invalid")
        })
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// 注册表里用的名字，比如 `minecraft:stone`。
    pub fn to_registry_name(&self) -> String {
        format!("{}:{}", self.namespace, self.path)
    }
}

fn read_greedy(reader: &mut StringReader) -> String {
    let start = reader.cursor();
    while reader.can_read() && is_allowed_in_identifier(reader.peek()) {
        reader.skip();
    }
    reader.slice(start, reader.cursor())
}

/// 官方 `Identifier.isAllowedInIdentifier`。
fn is_allowed_in_identifier(character: char) -> bool {
    character.is_ascii_lowercase()
        || character.is_ascii_digit()
        || matches!(character, '_' | ':' | '/' | '.' | '-')
}

fn is_valid_namespace(namespace: &str) -> bool {
    namespace != ".."
        && namespace.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '_' | '-' | '.')
        })
}

fn is_valid_path(path: &str) -> bool {
    path.chars().all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '_' | '-' | '.' | '/')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(input: &str) -> Result<Identifier, String> {
        let mut reader = StringReader::new(input);
        let result = Identifier::read(&mut reader).map_err(|error| error.key.to_owned());
        assert_eq!(reader.cursor(), input.chars().count());
        result
    }

    #[test]
    fn fills_in_the_default_namespace() {
        assert_eq!(read("stone").unwrap().to_registry_name(), "minecraft:stone");
        assert_eq!(
            read(":stone").unwrap().to_registry_name(),
            "minecraft:stone"
        );
        assert_eq!(
            read("bamboo:test/path").unwrap().to_registry_name(),
            "bamboo:test/path"
        );
        assert_eq!(read("minecraft:").unwrap().to_registry_name(), "minecraft:");
    }

    #[test]
    fn uppercase_is_not_consumed() {
        // 官方 `readGreedy` 只读小写字符，遇到大写就停下，结果是一个空的路径。
        let mut reader = StringReader::new("Stone");
        assert_eq!(
            Identifier::read(&mut reader).unwrap().to_registry_name(),
            "minecraft:"
        );
        assert_eq!(reader.cursor(), 0);
    }

    #[test]
    fn invalid_identifiers_rewind() {
        let mut reader = StringReader::new("..:stone");
        assert_eq!(
            Identifier::read(&mut reader).unwrap_err().key,
            "argument.id.invalid"
        );
        assert_eq!(reader.cursor(), 0);
        assert!(Identifier::parse("stone#tag").is_none());
        assert!(Identifier::parse("..:stone").is_none());
        assert!(Identifier::parse("a:b:c").is_none());
    }

    #[test]
    fn stops_at_characters_that_are_not_allowed() {
        let mut reader = StringReader::new("stone[foo=bar]");
        assert_eq!(
            Identifier::read(&mut reader).unwrap().to_registry_name(),
            "minecraft:stone"
        );
        assert_eq!(reader.peek(), '[');
    }
}
