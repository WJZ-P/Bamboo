//! 全世界共用的游戏规则。
//!
//! 权威副本在 [`Global`](crate::global::Global) 上，不写存档。重启之后回到官方默认值。
//! 方块逻辑读的是 [`Region`](crate::region::Region) 上的快照，改规则时当场同步。
//!
//! Game rules shared by the whole server.
//!
//! The authoritative copy lives on [`Global`](crate::global::Global) and is not saved.
//! A restart restores the official defaults. Block logic reads the snapshot on
//! [`Region`](crate::region::Region), which is updated as soon as a rule changes.

use mc_bamboo_blocks::BlockGameRules;
use mc_bamboo_command::{
    StringReader,
    argument::primitives::{parse_bool, parse_integer},
    permission,
};
use mc_bamboo_data::gamerules::{self, GameRuleKind, RULES};
use mc_bamboo_protocol::{
    packet::play::{GameRuleValues, SystemChat},
    text::Component,
};
use tracing::warn;

use crate::{inbound::PlayerId, links::encode_frames, region::Region};

/// 一次赋值的结果。
///
/// What one assignment did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Assignment {
    /// 写进去了。文本是官方 `serialize` 的结果。
    Updated(String),
    /// 值和现在一样。命令要报 `commands.gamerule.not_set`。
    Unchanged(String),
    /// 文本解不开，或者后面还有字符。官方静默忽略。
    Invalid,
    /// 规则没进默认特性的集合。`GameRules.set` 会警告并且不改值。
    ///
    /// 包处理仍然广播解析出来的值，和 `setGameRuleValue` 一样。
    ///
    /// The rule is outside the default feature set. `GameRules.set` warns and does not
    /// store it. Packet handling still broadcasts the parsed value, like `setGameRuleValue`.
    Disabled(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Stored {
    Bool(bool),
    Int(i32),
}

/// 全部已注册的游戏规则，下标和生成表一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GameRuleSet {
    values: Vec<Stored>,
}

impl Default for GameRuleSet {
    fn default() -> Self {
        Self::vanilla()
    }
}

impl GameRuleSet {
    /// `new GameRules(FeatureFlags.DEFAULT_FLAGS)`。
    pub(crate) fn vanilla() -> Self {
        let values = RULES
            .iter()
            .map(|rule| match rule.kind {
                GameRuleKind::Bool => Stored::Bool(rule.default_bool),
                GameRuleKind::Int => Stored::Int(rule.default_int),
            })
            .collect();
        Self { values }
    }

    pub(crate) fn bool(&self, index: usize) -> bool {
        match self.values[index] {
            Stored::Bool(value) => value,
            Stored::Int(_) => false,
        }
    }

    pub(crate) fn int(&self, index: usize) -> i32 {
        match self.values[index] {
            Stored::Int(value) => value,
            Stored::Bool(_) => 0,
        }
    }

    /// 方块代码用的那几条。
    ///
    /// The rules block code reads.
    pub(crate) fn block_snapshot(&self) -> BlockGameRules {
        BlockGameRules {
            random_tick_speed: self.int(gamerules::RANDOM_TICK_SPEED),
            fire_spread_radius_around_player: self.int(gamerules::FIRE_SPREAD_RADIUS_AROUND_PLAYER),
            tnt_explodes: self.bool(gamerules::TNT_EXPLODES),
            block_drops: self.bool(gamerules::BLOCK_DROPS),
            entity_drops: self.bool(gamerules::ENTITY_DROPS),
            water_source_conversion: self.bool(gamerules::WATER_SOURCE_CONVERSION),
            lava_source_conversion: self.bool(gamerules::LAVA_SOURCE_CONVERSION),
            spread_vines: self.bool(gamerules::SPREAD_VINES),
            max_block_modifications: self.int(gamerules::MAX_BLOCK_MODIFICATIONS),
            max_snow_accumulation_height: self.int(gamerules::MAX_SNOW_ACCUMULATION_HEIGHT),
            spawn_mobs: self.bool(gamerules::SPAWN_MOBS),
        }
    }

    /// 把方块快照和命令反馈用的布尔写到区域上。
    ///
    /// Copies the block snapshot and the command-feedback booleans onto the region.
    pub(crate) fn publish(&self, region: &mut Region) {
        region.block_rules = self.block_snapshot();
        region.send_command_feedback = self.bool(gamerules::SEND_COMMAND_FEEDBACK);
        region.log_admin_commands = self.bool(gamerules::LOG_ADMIN_COMMANDS);
        region.command_blocks_work = self.bool(gamerules::COMMAND_BLOCKS_WORK);
    }

    /// `GameRule.serialize`：布尔是 `true`/`false`，整数是十进制。
    pub(crate) fn serialize(&self, index: usize) -> String {
        match self.values[index] {
            Stored::Bool(value) => value.to_string(),
            Stored::Int(value) => value.to_string(),
        }
    }

    /// 默认特性下 `availableRules()` 里的规则，按注册表顺序。
    ///
    /// 官方的 `HashMap` 顺序不稳定。客户端用这张表替换转圈，键是规则的标识符。
    ///
    /// Enabled rules from `availableRules()`, in registry order.
    ///
    /// The official `HashMap` order is not stable. The client replaces its spinner with this
    /// list. The key is the rule identifier.
    pub(crate) fn enabled_values(&self) -> Vec<(&'static str, String)> {
        RULES
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.enabled)
            .map(|(index, rule)| (rule.identifier, self.serialize(index)))
            .collect()
    }

    /// `GameRule.deserialize`。整段必须被吃掉。命令和 `set_game_rule` 都走这条。
    ///
    /// `GameRule.deserialize`. The whole text must be consumed. Both the command and
    /// `set_game_rule` use this.
    pub(crate) fn assign(&mut self, index: usize, text: &str) -> Assignment {
        let Some(parsed) = parse_value(index, text) else {
            return Assignment::Invalid;
        };
        let serialized = match parsed {
            Stored::Bool(value) => value.to_string(),
            Stored::Int(value) => value.to_string(),
        };
        if !RULES[index].enabled {
            return Assignment::Disabled(serialized);
        }
        if self.values[index] == parsed {
            return Assignment::Unchanged(serialized);
        }
        self.values[index] = parsed;
        Assignment::Updated(serialized)
    }
}

fn parse_value(index: usize, text: &str) -> Option<Stored> {
    let rule = &RULES[index];
    let mut reader = StringReader::new(text);
    let stored = match rule.kind {
        GameRuleKind::Bool => Stored::Bool(parse_bool(&mut reader).ok()?.as_bool()?),
        GameRuleKind::Int => Stored::Int(
            parse_integer(&mut reader, rule.min, rule.max)
                .ok()?
                .as_integer()?,
        ),
    };
    // 尾随字符时官方返回带部分值的 `DataResult.error`，但 `Error.result()` 恒为空。
    // `setGameRuleValue` 只看 `result()`，所以前缀不会被采用。
    //
    // Trailing characters make `DataResult.error` with a partial value, but
    // `Error.result()` is always empty. `setGameRuleValue` only reads `result()`,
    // so the prefix is not applied.
    if reader.can_read() {
        return None;
    }
    Some(stored)
}

/// `sendGameRuleValues`。没有权限时警告，不发包。
///
/// `sendGameRuleValues`. Without permission this warns and sends nothing.
pub(crate) fn on_request_values(global: &crate::global::Global, region: &mut Region, id: PlayerId) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if player.permission_level < permission::GAMEMASTERS {
        warn_denied(&player.name, "request game rule values");
        return;
    }
    let owned = global.rules().enabled_values();
    let entries: Vec<(&str, &str)> = owned
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    region.links.send(id, &GameRuleValues { entries: &entries });
}

/// `handleSetGameRule`。解不开的值，以及带尾随字符的值，都静默忽略。
/// 未启用的规则不写入，但仍然广播解析后的文本。
///
/// `handleSetGameRule`. A value that does not parse, including one with trailing
/// characters, is ignored. A disabled rule is not stored, but the parsed text is still
/// broadcast.
pub(crate) fn on_set_packet(
    global: &mut crate::global::Global,
    region: &mut Region,
    id: PlayerId,
    entries: Vec<(String, String)>,
) {
    let Some(player) = region.players.get(&id) else {
        return;
    };
    if player.permission_level < permission::GAMEMASTERS {
        warn_denied(&player.name, "set game rule values");
        return;
    }
    let name = player.name.clone();
    for (identifier, value) in entries {
        let Some(index) = gamerules::find(&identifier) else {
            warn!(
                player = %name,
                rule = %identifier,
                "received request to set unknown game rule"
            );
            continue;
        };
        let short = RULES[index].name;
        match global.rules_mut().assign(index, &value) {
            Assignment::Updated(text) => {
                global.rules().publish(region);
                broadcast_set(region, short, &text);
            }
            Assignment::Unchanged(text) | Assignment::Disabled(text) => {
                broadcast_set(region, short, &text);
            }
            Assignment::Invalid => {}
        }
    }
}

fn broadcast_set(region: &mut Region, name: &str, value: &str) {
    // 普通系统消息，不包 `chat.type.admin`，也不看 `send_command_feedback`。
    // 每个 `isOp` 都收到，包括改规则的那个人。
    //
    // A plain system message. It is not wrapped in `chat.type.admin` and it ignores
    // `send_command_feedback`. Every `isOp` receives it, including the player who set it.
    let content = Component::translate("commands.gamerule.set", [name.into(), value.into()]);
    let frames = encode_frames(&SystemChat {
        content: &content,
        overlay: false,
    });
    let ops: Vec<PlayerId> = region
        .players
        .iter()
        .filter(|(_, player)| player.is_op)
        .map(|(id, _)| *id)
        .collect();
    for id in ops {
        region.links.send_frames(id, &frames);
    }
}

/// 没有权限时的警告。官方只记日志，不踢人。
///
/// The warning for a player without permission. Vanilla logs it and does not kick.
pub(crate) fn warn_denied(name: &str, action: &str) {
    warn!(
        player = name,
        "tried to {action} without required permissions"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_enabled_rule_round_trips_and_rejects_bad_text() {
        let mut rules = GameRuleSet::vanilla();
        for (index, rule) in RULES.iter().enumerate() {
            let serialized = rules.serialize(index);
            if !rule.enabled {
                assert!(
                    matches!(rules.assign(index, &serialized), Assignment::Disabled(_)),
                    "{}",
                    rule.name
                );
                continue;
            }
            assert_eq!(
                rules.assign(index, &serialized),
                Assignment::Unchanged(serialized.clone()),
                "{}",
                rule.name
            );
            match rule.kind {
                GameRuleKind::Bool => {
                    let flipped = if rules.bool(index) { "false" } else { "true" };
                    assert!(matches!(
                        rules.assign(index, flipped),
                        Assignment::Updated(_)
                    ));
                    assert!(matches!(rules.assign(index, "yes"), Assignment::Invalid));
                    assert!(matches!(rules.assign(index, "true "), Assignment::Invalid));
                    rules.assign(index, &serialized);
                }
                GameRuleKind::Int => {
                    if rule.min > i32::MIN {
                        assert!(matches!(
                            rules.assign(index, &(rule.min - 1).to_string()),
                            Assignment::Invalid
                        ));
                    }
                    if rule.max < i32::MAX {
                        assert!(matches!(
                            rules.assign(index, &(rule.max + 1).to_string()),
                            Assignment::Invalid
                        ));
                    }
                    assert!(matches!(rules.assign(index, "1.5"), Assignment::Invalid));
                    assert!(matches!(rules.assign(index, "1 "), Assignment::Invalid));
                    let next = if rules.int(index) == rule.min {
                        rule.min + 1
                    } else {
                        rule.min
                    };
                    assert!(matches!(
                        rules.assign(index, &next.to_string()),
                        Assignment::Updated(_)
                    ));
                    rules.assign(index, &serialized);
                }
            }
            assert_eq!(rules.serialize(index), serialized);
        }
    }

    #[test]
    fn disabled_rules_parse_without_changing_the_stored_value() {
        let mut rules = GameRuleSet::vanilla();
        let before = rules.serialize(gamerules::MAX_MINECART_SPEED);
        assert!(matches!(
            rules.assign(gamerules::MAX_MINECART_SPEED, "9"),
            Assignment::Disabled(_)
        ));
        assert_eq!(rules.serialize(gamerules::MAX_MINECART_SPEED), before);
        assert!(
            !rules
                .enabled_values()
                .iter()
                .any(|(name, _)| *name == "minecraft:max_minecart_speed")
        );
    }

    #[test]
    fn trailing_characters_are_ignored_even_when_a_prefix_parses() {
        let mut rules = GameRuleSet::vanilla();
        assert!(matches!(
            rules.assign(gamerules::RANDOM_TICK_SPEED, "8 leftover"),
            Assignment::Invalid
        ));
        assert_eq!(rules.int(gamerules::RANDOM_TICK_SPEED), 3);
        assert!(matches!(
            rules.assign(gamerules::TNT_EXPLODES, "false!"),
            Assignment::Invalid
        ));
        assert!(rules.bool(gamerules::TNT_EXPLODES));
    }
}
