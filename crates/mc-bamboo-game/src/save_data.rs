//! `level.dat` 的运行时字段、`data/minecraft/game_rules.dat` 和
//! `dimensions/minecraft/overworld/data/minecraft/world_border.dat` 的编解码。
//!
//! 这一层只做「运行时状态 ⇄ NBT 复合标签」的翻译，不碰世界目录（读写在
//! [`SaveSession`](crate::save_session::SaveSession) 和 `mc-bamboo-storage`）。
//! 格式照 26.3 官方 codec：
//! - `GameRuleMap.CODEC`（`Codec.dispatchedMap`）：键是带命名空间的规则标识符，
//!   布尔规则是 NBT `Byte`，整数规则是 `Int`；只写默认特性开着的规则。
//! - `WorldBorder.Settings.CODEC`（`RecordCodecBuilder`）：九个必填字段，
//!   双精度是 `Double`、整数是 `Int`、`lerp_time` 是 `Long`。
//! - `PrimaryLevelData.createTag`：`GameType` / `spawn` / `difficulty_settings`。
//!
//! Translation between runtime state and the NBT compounds for `level.dat`,
//! `data/minecraft/game_rules.dat` and the overworld `world_border.dat`.
//! This layer only maps state to NBT; file IO lives in `SaveSession` and
//! `mc-bamboo-storage`. Layout follows the 26.3 official codecs.

use mc_bamboo_blocks::world_border::BorderSettings;
use mc_bamboo_core::BlockPos;
use mc_bamboo_nbt::Tag;
use mc_bamboo_storage::level_dat::LevelData;

use crate::{
    gamerules::{GameRuleSet, Stored},
    player_save::game_mode_from_id,
    world_state::{Difficulty, RespawnData, WorldState},
};

/// 从一个复合标签里按键取值。
/// Look up a key in a compound tag.
fn get<'a>(tag: &'a Tag, key: &str) -> Option<&'a Tag> {
    match tag {
        Tag::Compound(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn get_int(tag: &Tag, key: &str) -> Option<i32> {
    match get(tag, key) {
        Some(Tag::Int(v)) => Some(*v),
        _ => None,
    }
}

fn get_long(tag: &Tag, key: &str) -> Option<i64> {
    match get(tag, key) {
        Some(Tag::Long(v)) => Some(*v),
        _ => None,
    }
}

fn get_double(tag: &Tag, key: &str) -> Option<f64> {
    match get(tag, key) {
        Some(Tag::Double(v)) => Some(*v),
        _ => None,
    }
}

/// 把当前游戏规则编成 `game_rules.dat` 的 `data` 复合标签。
/// Encode the current game rules into the `data` compound of `game_rules.dat`.
pub(crate) fn game_rules_tag(rules: &GameRuleSet) -> Tag {
    let entries = rules
        .saved()
        .into_iter()
        .map(|(name, value)| {
            let value = match value {
                Stored::Bool(v) => Tag::Byte(i8::from(v)),
                Stored::Int(v) => Tag::Int(v),
            };
            (name.to_owned(), value)
        })
        .collect();
    Tag::Compound(entries)
}

/// 把 `game_rules.dat` 的 `data` 复合标签套回游戏规则。坏键、类型不符、越界、
/// 没启用的规则都整条忽略，和官方 `SavedDataStorage` 读坏退回默认一致。
/// Apply a `game_rules.dat` `data` compound back onto the game rules. Bad keys,
/// wrong types, out-of-range and disabled rules are ignored, like vanilla.
pub(crate) fn apply_game_rules(rules: &mut GameRuleSet, tag: &Tag) {
    let Tag::Compound(entries) = tag else {
        return;
    };
    for (name, value) in entries {
        let stored = match value {
            Tag::Byte(v) => Stored::Bool(*v != 0),
            Tag::Int(v) => Stored::Int(*v),
            _ => continue,
        };
        rules.apply_saved(name, stored);
    }
}

/// 把世界边界设置编成 `world_border.dat` 的 `data` 复合标签（九个必填字段）。
/// Encode border settings into the `data` compound (nine required fields).
pub(crate) fn world_border_tag(s: &BorderSettings) -> Tag {
    Tag::Compound(vec![
        ("center_x".to_owned(), Tag::Double(s.center_x)),
        ("center_z".to_owned(), Tag::Double(s.center_z)),
        (
            "damage_per_block".to_owned(),
            Tag::Double(s.damage_per_block),
        ),
        ("safe_zone".to_owned(), Tag::Double(s.safe_zone)),
        ("warning_blocks".to_owned(), Tag::Int(s.warning_blocks)),
        ("warning_time".to_owned(), Tag::Int(s.warning_time)),
        ("size".to_owned(), Tag::Double(s.size)),
        ("lerp_time".to_owned(), Tag::Long(s.lerp_time)),
        ("lerp_target".to_owned(), Tag::Double(s.lerp_target)),
    ])
}

/// 解出 `world_border.dat` 的九个字段；缺一或类型不对返回 `None`（调用方用默认）。
/// Decode the nine fields; return `None` (use defaults) if any is missing.
pub(crate) fn parse_world_border(tag: &Tag) -> Option<BorderSettings> {
    Some(BorderSettings {
        center_x: get_double(tag, "center_x")?,
        center_z: get_double(tag, "center_z")?,
        damage_per_block: get_double(tag, "damage_per_block")?,
        safe_zone: get_double(tag, "safe_zone")?,
        warning_blocks: get_int(tag, "warning_blocks")?,
        warning_time: get_int(tag, "warning_time")?,
        size: get_double(tag, "size")?,
        lerp_time: get_long(tag, "lerp_time")?,
        lerp_target: get_double(tag, "lerp_target")?,
    })
}

/// 把运行时世界设置写回 `LevelData`（`PrimaryLevelData.createTag` 里会变的那几项）。
/// `hardcore` 和 `spawn.dimension` 运行时没建模，原样保留读入的值。
/// Copy runtime world settings back into `LevelData`. `hardcore` and
/// `spawn.dimension` are not modelled at runtime, so they keep the loaded value.
pub(crate) fn sync_level_data(
    level: &mut LevelData,
    state: &WorldState,
    time: i64,
    last_played: i64,
) {
    level.game_type = state.default_game_mode as i32;
    level.spawn.x = state.spawn.pos.x;
    level.spawn.y = state.spawn.pos.y;
    level.spawn.z = state.spawn.pos.z;
    level.spawn.yaw = state.spawn.yaw;
    level.spawn.pitch = state.spawn.pitch;
    level.difficulty.difficulty = state.difficulty.name().to_owned();
    level.difficulty.locked = state.difficulty_locked;
    level.time = time;
    level.last_played = last_played;
}

/// 把启动读到的 `level.dat` 套回运行时世界设置（进服序列和命令都从这里读）。
/// Apply the loaded `level.dat` back onto runtime world settings at startup.
pub(crate) fn apply_level_data(state: &mut WorldState, level: &LevelData) {
    state.default_game_mode = game_mode_from_id(level.game_type);
    state.spawn = RespawnData::of(
        BlockPos::new(level.spawn.x, level.spawn.y, level.spawn.z),
        level.spawn.yaw,
        level.spawn.pitch,
    );
    if let Some(difficulty) = Difficulty::ALL
        .iter()
        .copied()
        .find(|d| d.name() == level.difficulty.difficulty)
    {
        state.difficulty = difficulty;
    }
    state.difficulty_locked = level.difficulty.locked;
}

/// `level.dat` 里读出来的世界设置和两份 saved data，供 `World::build` 采用。
/// World settings and the two saved-data compounds loaded at startup.
#[derive(Debug, Default)]
pub(crate) struct StartupData {
    pub(crate) game_rules: Option<Tag>,
    pub(crate) world_border: Option<Tag>,
}

impl StartupData {
    /// 把游戏规则/边界套到 `Global`/`Region` 上。边界的 `restore` 要传存档里的游戏时间。
    /// Apply game rules and border to `Global`/`Region`; `restore` takes the saved
    /// game time as vanilla does.
    pub(crate) fn apply(
        &self,
        global: &mut crate::global::Global,
        region: &mut crate::region::Region,
        game_time: i64,
    ) {
        if let Some(tag) = &self.game_rules {
            apply_game_rules(global.rules_mut(), tag);
        }
        if let Some(settings) = self.world_border.as_ref().and_then(parse_world_border) {
            *global.world_border_mut() =
                mc_bamboo_blocks::world_border::WorldBorder::restore(settings, game_time);
        }
        global.rules().publish(region);
        global.publish_border(region);
    }
}

#[cfg(test)]
mod tests {
    use mc_bamboo_data::gamerules;

    use super::*;

    #[test]
    fn game_rules_round_trip_keeps_an_edited_rule() {
        let mut rules = GameRuleSet::vanilla();
        let index = gamerules::find("minecraft:random_tick_speed").unwrap();
        assert!(matches!(
            rules.assign(index, "7"),
            crate::gamerules::Assignment::Updated(_)
        ));
        let tag = game_rules_tag(&rules);
        assert!(matches!(
            get(&tag, "minecraft:random_tick_speed"),
            Some(Tag::Int(7))
        ));
        // 布尔规则写成 Byte（本版本的规则名是 fire_damage）。
        assert!(matches!(
            get(&tag, "minecraft:fire_damage"),
            Some(Tag::Byte(_))
        ));

        let mut restored = GameRuleSet::vanilla();
        apply_game_rules(&mut restored, &tag);
        assert_eq!(restored.int(index), 7);
    }

    #[test]
    fn world_border_round_trips_all_nine_fields() {
        let settings = BorderSettings {
            center_x: 12.5,
            center_z: -8.0,
            damage_per_block: 0.3,
            safe_zone: 4.0,
            warning_blocks: 7,
            warning_time: 42,
            size: 123.0,
            lerp_time: 99,
            lerp_target: 200.0,
        };
        let tag = world_border_tag(&settings);
        assert_eq!(parse_world_border(&tag), Some(settings));
        // 少一个字段就当坏档退回默认。
        let broken = Tag::Compound(vec![("center_x".to_owned(), Tag::Double(0.0))]);
        assert!(parse_world_border(&broken).is_none());
    }

    #[test]
    fn level_runtime_values_round_trip() {
        let mut level = LevelData::default();
        let state = WorldState {
            default_game_mode: mc_bamboo_protocol::packet::play::GameMode::Survival,
            difficulty: Difficulty::Hard,
            difficulty_locked: true,
            spawn: RespawnData::of(BlockPos::new(100, 70, -200), 45.0, 20.0),
            ..Default::default()
        };
        sync_level_data(&mut level, &state, 4242, 999);
        assert_eq!(level.game_type, 0);
        assert_eq!(level.time, 4242);
        assert_eq!(level.difficulty.difficulty, "hard");
        assert!(level.difficulty.locked);

        let mut applied = WorldState::default();
        apply_level_data(&mut applied, &level);
        assert_eq!(
            applied.default_game_mode,
            mc_bamboo_protocol::packet::play::GameMode::Survival
        );
        assert_eq!(applied.difficulty, Difficulty::Hard);
        assert!(applied.difficulty_locked);
        assert_eq!(applied.spawn.pos, BlockPos::new(100, 70, -200));
    }
}
