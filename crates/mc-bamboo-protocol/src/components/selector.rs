//! 官方 `EntitySelectorParser`，只判断这段字符串能不能编过。
//!
//! `EntitySelector.COMPILABLE_CODEC` 用 `allowSelectors = true`，而且要求把整段字符串吃完。
//! 选项的限制（只能写一次、正反不能混用、`@s` 不能排序）和官方的 `Option` 谓词一致。
//!
//! `EntitySelectorParser`, only as a yes-or-no check.
//! `EntitySelector.COMPILABLE_CODEC` allows selectors and requires the whole string
//! to be consumed. Option limits match the official predicates.

use mc_bamboo_data::component_registries::registry_names::ENTITY_TYPE;

use super::syntax::{Reader, read_bounds, take_compound};

/// 整段文本是不是一个合法的实体选择器或玩家名。
pub(crate) fn parse(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut reader = Reader::new(&chars);
    let mut parser = Parser::default();
    parser.parse(&mut reader) && !reader.can_read()
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Limit {
    #[default]
    None,
    Single,
    Multiple,
}

#[derive(Default)]
struct Parser {
    current_entity: bool,
    distance: bool,
    level: bool,
    x: bool,
    y: bool,
    z: bool,
    dx: bool,
    dy: bool,
    dz: bool,
    rot_x: bool,
    rot_y: bool,
    limit: bool,
    sort: bool,
    scores: bool,
    advancements: bool,
    name: Limit,
    gamemode: Limit,
    team: Limit,
    kind: Limit,
    type_tags: Vec<String>,
}

impl Parser {
    fn parse(&mut self, reader: &mut Reader<'_>) -> bool {
        if reader.peek() == Some('@') {
            reader.skip();
            self.selector(reader)
        } else {
            self.name_or_uuid(reader)
        }
    }

    fn selector(&mut self, reader: &mut Reader<'_>) -> bool {
        let kind = match reader.peek() {
            Some(kind @ ('a' | 'e' | 'n' | 'p' | 'r' | 's')) => {
                reader.skip();
                kind
            }
            _ => return false,
        };
        self.current_entity = kind == 's';
        if reader.peek() != Some('[') {
            return true;
        }
        reader.skip();
        self.options(reader)
    }

    fn name_or_uuid(&mut self, reader: &mut Reader<'_>) -> bool {
        let Ok(name) = reader.read_string() else {
            return false;
        };
        if uuid(&name) {
            return true;
        }
        let length = name.encode_utf16().count();
        (1..=16).contains(&length)
    }

    fn options(&mut self, reader: &mut Reader<'_>) -> bool {
        while reader.can_read() && reader.peek() != Some(']') {
            reader.skip_whitespace();
            let start = reader.pos();
            let Ok(key) = reader.read_string() else {
                return false;
            };
            reader.skip_whitespace();
            if reader.expect('=').is_err() {
                reader.set_pos(start);
                return false;
            }
            reader.skip_whitespace();
            if !self.option(reader, &key) {
                return false;
            }
            reader.skip_whitespace();
            if !reader.can_read() {
                return false;
            }
            if reader.peek() == Some(',') {
                reader.skip();
                continue;
            }
            if reader.peek() != Some(']') {
                return false;
            }
            break;
        }
        if reader.peek() == Some(']') {
            reader.skip();
            true
        } else {
            false
        }
    }

    fn option(&mut self, reader: &mut Reader<'_>, key: &str) -> bool {
        match key {
            "name" => invertible(reader, &mut self.name, |reader| {
                reader.read_string().is_ok()
            }),
            "distance" => once(&mut self.distance, || non_negative(reader, false)),
            "level" => once(&mut self.level, || non_negative(reader, true)),
            "x" => once(&mut self.x, || reader.read_double().is_ok()),
            "y" => once(&mut self.y, || reader.read_double().is_ok()),
            "z" => once(&mut self.z, || reader.read_double().is_ok()),
            "dx" => once(&mut self.dx, || reader.read_double().is_ok()),
            "dy" => once(&mut self.dy, || reader.read_double().is_ok()),
            "dz" => once(&mut self.dz, || reader.read_double().is_ok()),
            "x_rotation" => once(&mut self.rot_x, || read_bounds(reader, false).is_ok()),
            "y_rotation" => once(&mut self.rot_y, || read_bounds(reader, false).is_ok()),
            "limit" => {
                if self.current_entity || self.limit {
                    return false;
                }
                let Ok(count) = reader.read_int() else {
                    return false;
                };
                if count < 1 {
                    return false;
                }
                self.limit = true;
                true
            }
            "sort" => {
                if self.current_entity || self.sort {
                    return false;
                }
                let name = reader.read_unquoted();
                if !matches!(
                    name.as_str(),
                    "nearest" | "furthest" | "random" | "arbitrary"
                ) {
                    return false;
                }
                self.sort = true;
                true
            }
            "gamemode" => invertible(reader, &mut self.gamemode, |reader| {
                let name = reader.read_unquoted();
                matches!(
                    name.as_str(),
                    "survival" | "creative" | "adventure" | "spectator"
                )
            }),
            "team" => invertible(reader, &mut self.team, |reader| {
                let _name = reader.read_unquoted();
                true
            }),
            "type" => self.entity_type(reader),
            "tag" => {
                let _tag = reader.read_unquoted();
                true
            }
            "nbt" => take_compound(reader),
            "scores" => once(&mut self.scores, || score_map(reader)),
            "advancements" => once(&mut self.advancements, || advancement_map(reader)),
            "predicate" => {
                let _inverted = inverted(reader);
                reader.read_identifier().is_ok()
            }
            _ => false,
        }
    }

    fn entity_type(&mut self, reader: &mut Reader<'_>) -> bool {
        let inverted = inverted(reader);
        let tag = reader.peek() == Some('#');
        if tag {
            reader.skip();
            reader.skip_whitespace();
        }
        if tag {
            if self.kind == Limit::Single {
                return false;
            }
            let Ok(id) = reader.read_identifier() else {
                return false;
            };
            if self.type_tags.iter().any(|seen| seen == &id) {
                return false;
            }
            self.type_tags.push(id);
            self.kind = Limit::Multiple;
            return true;
        }
        let allowed = !matches!(
            (self.kind, inverted),
            (Limit::Single, _) | (Limit::Multiple, false)
        );
        if !allowed {
            return false;
        }
        let Ok(id) = reader.read_identifier() else {
            return false;
        };
        if !ENTITY_TYPE.contains(&id.as_str()) {
            return false;
        }
        self.kind = if inverted {
            Limit::Multiple
        } else {
            Limit::Single
        };
        true
    }
}

fn once(flag: &mut bool, parse: impl FnOnce() -> bool) -> bool {
    if *flag || !parse() {
        return false;
    }
    *flag = true;
    true
}

fn invertible(
    reader: &mut Reader<'_>,
    state: &mut Limit,
    parse: impl FnOnce(&mut Reader<'_>) -> bool,
) -> bool {
    let inverted = inverted(reader);
    let allowed = !matches!(
        (*state, inverted),
        (Limit::Single, _) | (Limit::Multiple, false)
    );
    if !allowed || !parse(reader) {
        return false;
    }
    *state = if inverted {
        Limit::Multiple
    } else {
        Limit::Single
    };
    true
}

fn inverted(reader: &mut Reader<'_>) -> bool {
    reader.skip_whitespace();
    if reader.peek() == Some('!') {
        reader.skip();
        reader.skip_whitespace();
        true
    } else {
        false
    }
}

fn non_negative(reader: &mut Reader<'_>, integer: bool) -> bool {
    let Ok((min, max)) = read_bounds(reader, integer) else {
        return false;
    };
    min.is_none_or(|value| value >= 0.0) && max.is_none_or(|value| value >= 0.0)
}

fn score_map(reader: &mut Reader<'_>) -> bool {
    if reader.expect('{').is_err() {
        return false;
    }
    reader.skip_whitespace();
    while reader.can_read() && reader.peek() != Some('}') {
        reader.skip_whitespace();
        let _name = reader.read_unquoted();
        reader.skip_whitespace();
        if reader.expect('=').is_err() || read_bounds(reader, true).is_err() {
            return false;
        }
        reader.skip_whitespace();
        if reader.peek() == Some(',') {
            reader.skip();
        }
    }
    reader.expect('}').is_ok()
}

fn advancement_map(reader: &mut Reader<'_>) -> bool {
    if reader.expect('{').is_err() {
        return false;
    }
    reader.skip_whitespace();
    while reader.can_read() && reader.peek() != Some('}') {
        reader.skip_whitespace();
        if reader.read_identifier().is_err() {
            return false;
        }
        reader.skip_whitespace();
        if reader.expect('=').is_err() {
            return false;
        }
        reader.skip_whitespace();
        if reader.peek() == Some('{') {
            if !criterion_map(reader) {
                return false;
            }
        } else if reader.read_bool().is_err() {
            return false;
        }
        reader.skip_whitespace();
        if reader.peek() == Some(',') {
            reader.skip();
        }
    }
    reader.expect('}').is_ok()
}

fn criterion_map(reader: &mut Reader<'_>) -> bool {
    if reader.expect('{').is_err() {
        return false;
    }
    reader.skip_whitespace();
    while reader.can_read() && reader.peek() != Some('}') {
        reader.skip_whitespace();
        let _name = reader.read_unquoted();
        reader.skip_whitespace();
        if reader.expect('=').is_err() {
            return false;
        }
        reader.skip_whitespace();
        if reader.read_bool().is_err() {
            return false;
        }
        reader.skip_whitespace();
        if reader.peek() == Some(',') {
            reader.skip();
        }
    }
    reader.expect('}').is_ok()
}

fn uuid(text: &str) -> bool {
    uuid::Uuid::parse_str(text).is_ok()
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn selectors_match_the_official_grammar() {
        assert!(parse("@a"));
        assert!(parse("@e[type=minecraft:pig,limit=2,sort=nearest]"));
        assert!(parse("@s"));
        assert!(parse("Notch"));
        assert!(!parse("@s[limit=1]"));
        assert!(!parse("@a[type=no_such_mob]"));
        assert!(!parse("@a["));
        assert!(!parse("@a extra"));
        assert!(!parse(""));
    }
}
