//! `level.dat` 的运行时字段、`data/minecraft/game_rules.dat`、
//! `data/minecraft/raids.dat` 和
//! `dimensions/minecraft/overworld/data/minecraft/world_border.dat` 的编解码。
//!
//! 这一层只做「运行时状态 ⇄ NBT 复合标签」的翻译，不碰世界目录（读写在
//! [`SaveSession`](crate::save_session::SaveSession) 和 `mc-bamboo-storage`）。
//! 格式照 26.3 官方 codec：
//! - `GameRuleMap.CODEC`（`Codec.dispatchedMap`）：键是带命名空间的规则标识符，
//!   布尔规则是 NBT `Byte`，整数规则是 `Int`；只写默认特性开着的规则。
//! - `WorldBorder.Settings.CODEC`（`RecordCodecBuilder`）：九个必填字段，
//!   双精度是 `Double`、整数是 `Int`、`lerp_time` 是 `Long`。
//! - `PackedClockStates.CODEC`：`minecraft:world_clocks` 的注册名 → `ClockState`。
//! - `Raids.CODEC`：`raids.dat` 的 `raids` / `next_id` / `tick`。
//! - `PrimaryLevelData.createTag`：`GameType` / `spawn` / `difficulty_settings`。
//!
//! Translation between runtime state and the NBT compounds for `level.dat`,
//! `data/minecraft/game_rules.dat`, `data/minecraft/world_clocks.dat`,
//! `data/minecraft/raids.dat` and the overworld `world_border.dat`.
//! This layer only maps state to NBT; file IO lives in `SaveSession` and
//! `mc-bamboo-storage`. Layout follows the 26.3 official codecs.

use mc_bamboo_blocks::{entity::raid::Raids, world_border::BorderSettings};
use mc_bamboo_core::BlockPos;
use mc_bamboo_nbt::Tag;
use mc_bamboo_storage::level_dat::LevelData;

use crate::{
    gamerules::{GameRuleSet, Stored},
    global::Clock,
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

/// 把 `game_rules.dat` 的 `data` 复合标签套回游戏规则。
///
/// 官方 `GameRuleMap.CODEC` 是 `Registry.byNameCodec().dispatchedMap(...)`，
/// 整份 map 一次解码；`SavedDataStorage.readSavedData` 用 `resultOrPartial`，
/// 解码失败返回 null，`computeIfAbsent` 用 `constructor()` 造全默认的 map。
/// 所以任何一条坏条目都不能「只忽略这一条」：整份解码失败，全部退回默认值。
/// 这里先把整份解成一张临时的值表，全部条目都合法才一次性写进 `rules`。
///
/// Apply a `game_rules.dat` `data` compound back onto the game rules. Vanilla
/// decodes the whole dispatched map at once and falls back to the all-default
/// constructor if it fails, so a single bad entry invalidates the entire file.
pub(crate) fn apply_game_rules(rules: &mut GameRuleSet, tag: &Tag) {
    let Tag::Compound(entries) = tag else {
        return;
    };
    let mut parsed = Vec::with_capacity(entries.len());
    for (name, value) in entries {
        // 官方 codec 只认带命名空间的、启用的规则；布尔是 Byte、整数是 Int，
        // 整数还要在规则自己的 [min, max] 里。任一条不合格就整份作废。
        // The official codec only accepts enabled namespaced rules, with Byte
        // for booleans and Int (in range) for integers; reject the whole file
        // on any mismatch instead of dropping the offending entry.
        let stored = match value {
            Tag::Byte(v) => Stored::Bool(*v != 0),
            Tag::Int(v) => Stored::Int(*v),
            _ => return,
        };
        let Some(index) = mc_bamboo_data::gamerules::find(name) else {
            return;
        };
        let rule = &mc_bamboo_data::gamerules::RULES[index];
        let valid = match (rule.kind, stored) {
            (mc_bamboo_data::gamerules::GameRuleKind::Bool, Stored::Bool(_)) => true,
            (mc_bamboo_data::gamerules::GameRuleKind::Int, Stored::Int(v)) => {
                (rule.min..=rule.max).contains(&v)
            }
            _ => false,
        };
        if !rule.enabled || !valid {
            return;
        }
        parsed.push((index, stored));
    }
    for (index, stored) in parsed {
        rules.set_saved(index, stored);
    }
}

/// 把两份世界时钟编成 `world_clocks.dat` 的 `data` 复合标签。
/// 官方 `PackedClockStates.CODEC` 是 `Codec.unboundedMap(WorldClock.CODEC, ClockState.CODEC)`，
/// 键是注册名字符串；`ClockState.CODEC` 必填 `total_ticks`，其余三项是可选字段。
///
/// Encode both world clocks into the `data` compound of `world_clocks.dat`.
/// Vanilla uses `Codec.unboundedMap(WorldClock.CODEC, ClockState.CODEC)` with
/// registry-name keys; `ClockState.CODEC` requires `total_ticks` and makes the
/// other three fields optional.
pub(crate) fn world_clocks_tag(global: &crate::global::Global) -> Tag {
    let entries = global
        .clocks()
        .into_iter()
        .map(|(name, clock)| {
            (
                name.to_owned(),
                Tag::Compound(vec![
                    ("total_ticks".to_owned(), Tag::Long(clock.total_ticks)),
                    ("partial_tick".to_owned(), Tag::Float(clock.partial_tick)),
                    ("rate".to_owned(), Tag::Float(clock.rate)),
                    ("paused".to_owned(), Tag::Byte(i8::from(clock.paused))),
                ]),
            )
        })
        .collect();
    Tag::Compound(entries)
}

/// 解出 `world_clocks.dat`；任一条坏数据都让整份作废（官方 `resultOrPartial` +
/// `computeIfAbsent` 的默认构造），返回空表时不改运行时的时钟。
/// 未知注册名按官方 `unboundedMap` 由键 codec 拒绝，同样整份作废。
///
/// Decode `world_clocks.dat`; any bad entry invalidates the whole file (vanilla's
/// `resultOrPartial` plus the default constructor). Unknown registry names are
/// rejected by the official key codec, so they invalidate it too.
pub(crate) fn parse_world_clocks(tag: &Tag) -> Option<Vec<(&'static str, Clock)>> {
    let Tag::Compound(entries) = tag else {
        return None;
    };
    let mut parsed = Vec::with_capacity(entries.len());
    for (name, value) in entries {
        let clock_name = match name.as_str() {
            "minecraft:overworld" => "minecraft:overworld",
            "minecraft:the_end" => "minecraft:the_end",
            _ => return None,
        };
        let total_ticks = get_long(value, "total_ticks")?;
        let partial_tick = match get(value, "partial_tick") {
            Some(Tag::Float(v)) if v.is_finite() => *v,
            Some(_) => return None,
            None => 0.0,
        };
        // `ExtraCodecs.POSITIVE_FLOAT` 是 (0, Float.MAX] 的闭区间检查，NaN 也过不了。
        // `ExtraCodecs.POSITIVE_FLOAT` checks (0, MAX]; NaN fails it too.
        let rate = match get(value, "rate") {
            Some(Tag::Float(v)) if *v > 0.0 && v.is_finite() => *v,
            Some(_) => return None,
            None => 1.0,
        };
        let paused = match get(value, "paused") {
            Some(Tag::Byte(v)) => *v != 0,
            Some(_) => return None,
            None => false,
        };
        parsed.push((
            clock_name,
            Clock {
                total_ticks,
                partial_tick,
                rate,
                paused,
            },
        ));
    }
    Some(parsed)
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

/// 把运行时状态写回 `LevelData`，逐字段对应官方
/// `PrimaryLevelData.setTagData`（`createTag` 的私有实现）读取的运行时字段。
/// `world_dir` 是这一局真正打开的世界目录，末段就是官方 `level-name`
/// （`DedicatedServerProperties.levelName` → `LevelSettings.levelName()`）。
///
/// Copy runtime state into `LevelData`, field by field, matching what vanilla's
/// `PrimaryLevelData.setTagData` reads. The last component of `world_dir` is the
/// official `level-name` (`DedicatedServerProperties.levelName` →
/// `LevelSettings.levelName()`).
pub(crate) fn sync_level_data(
    level: &mut LevelData,
    state: &WorldState,
    world_dir: &std::path::Path,
    time: i64,
    last_played: i64,
) {
    // `settings.levelName()`：世界目录名，Bamboo 由 `level-name` 推出来。
    if let Some(name) = world_dir.file_name().and_then(|name| name.to_str()) {
        level.level_name = name.to_owned();
    }
    // `settings.gameType()`：新玩家进服时的默认游戏模式。
    level.game_type = state.default_game_mode as i32;
    // `respawnData`：`GlobalPos` 由 `Level.OVERWORLD` 和出生点组成，yaw/pitch 是运行时值。
    level.spawn.dimension = mc_bamboo_storage::level_dat::OVERWORLD.to_owned();
    level.spawn.x = state.spawn.pos.x;
    level.spawn.y = state.spawn.pos.y;
    level.spawn.z = state.spawn.pos.z;
    level.spawn.yaw = state.spawn.yaw;
    level.spawn.pitch = state.spawn.pitch;
    // `settings.difficultySettings()`：hardcore 没有运行时状态，按官方
    // `createNewWorldData` 的常量 false 写；locked 走 `/difficulty` 的运行态。
    level.difficulty.difficulty = state.difficulty.name().to_owned();
    level.difficulty.hardcore = false;
    level.difficulty.locked = state.difficulty_locked;
    // `gameTime` / `Util.getEpochMillis()`。
    level.time = time;
    level.last_played = last_played;
    // `settings.allowCommands()`：官方专用服务端只在 DEMO 下为 true，
    // `createNewWorldData` 传常量 false；Bamboo 没有运行时开关，写真值 false。
    level.allow_commands = false;
    // `MinecraftServer.loadLevel` 会 `setModdedInfo("vanilla", false)`：
    // Bamboo 是专用服务端，服务器品牌只有 vanilla，也没被 mod 过。
    level.server_brands = vec!["vanilla".to_owned()];
    level.was_modded = false;
    level.removed_features.clear();
    // `initialized` 一开局就 true（官方 parse 缺字段时也是 true）；世界已经建好了。
    level.initialized = true;
    // `versionHistory`：官方每次写都把当前 DataVersion 追加到末项不同的情况。
    // 只支持一个版本，所以就是 [DATA_VERSION]。
    level.version_history = vec![mc_bamboo_data::version::DATA_VERSION];
    level.data_version = mc_bamboo_data::version::DATA_VERSION;
    level.version = Default::default();
    level.format_version = mc_bamboo_storage::level_dat::LEVEL_FORMAT_VERSION;
    // `WorldDataConfiguration`：DataPackConfig.DEFAULT 与 DEFAULT_FLAGS。
    // 等于默认对象时 `lenientOptionalFieldOf` 不写 `DataPacks` / `enabled_features`。
    level.data_packs = None;
    level.enabled_features = vec!["minecraft:vanilla".to_owned()];
    // `createTag(null)` 走的是 `singlePlayerUUID`，专用服务端是 null。
    level.singleplayer_uuid = None;
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

/// 把各岛的袭击汇总成一份 `raids.dat` 的 `data` 复合标签。
///
/// 官方一个维度一份 `Raids`，Bamboo 每岛一份，存盘时按袭击 id 汇总。各岛的
/// `next_id` 可能撞号，所以重新分配全局 id：所有表的 `next_id` 最大值起步，
/// 按 (岛编号, 原 id) 排序后逐场分配。`tick` 取所有表里的最大值。
///
/// Roll every island's raids into one `raids.dat` `data` compound. Bamboo has
/// one `Raids` per island, so ids are reassigned globally (starting at the
/// largest `next_id`, ordered by island and id) and `tick` is the maximum.
pub(crate) fn raids_tag(islands: &[(u32, Raids)]) -> Tag {
    let next_id = islands
        .iter()
        .map(|(_, raids)| raids.next_id)
        .max()
        .unwrap_or(1);
    let tick = islands
        .iter()
        .map(|(_, raids)| raids.tick)
        .max()
        .unwrap_or(0);
    let mut out = Raids::new();
    out.next_id = next_id;
    out.tick = tick;
    for (_, raids) in islands {
        for raid in raids.raids.values() {
            let id = out.unique_id();
            out.raids.insert(id, raid.clone());
        }
    }
    out.to_tag()
}

/// 启动时从 `data/` 读到的 saved data，供 `World::build` 采用。
/// Saved-data compounds loaded from `data/` at startup for `World::build`.
#[derive(Debug, Default)]
pub(crate) struct StartupData {
    pub(crate) game_rules: Option<Tag>,
    pub(crate) world_border: Option<Tag>,
    pub(crate) world_clocks: Option<Tag>,
    /// `raids.dat` 的 `data` 复合标签。官方覆盖整个维度，Bamboo 在家岛加载。
    pub(crate) raids: Raids,
    /// 每个命名空间一份 `command_storage.dat` 的 `data` 复合标签。
    pub(crate) command_storage: Vec<(String, Tag)>,
}

impl StartupData {
    /// 把游戏规则、边界和世界时钟套到 `Global`/`Region` 上。边界的 `restore`
    /// 要传存档里的游戏时间，和官方 `applyInitialSettings(getGameTime())` 一致。
    ///
    /// Apply game rules, border and world clocks to `Global`/`Region`; `restore`
    /// takes the saved game time, like vanilla's
    /// `applyInitialSettings(getGameTime())`.
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
        if let Some(clocks) = self.world_clocks.as_ref().and_then(parse_world_clocks) {
            for (name, state) in clocks {
                global.restore_clock(name, state);
            }
        }
        for (namespace, tag) in &self.command_storage {
            global.command_storage.load_namespace(namespace, tag);
        }
        // `raids.dat` 是维度级的：只有家岛在启动时加载，别的岛从它拆出去。
        // `raids.dat` is dimension-wide: only the home island loads it at
        // startup; other islands get their raids from a later split.
        region.raids = self.raids.clone();
        global.publish(region);
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

    /// 官方 `SavedDataStorage` 读坏整份 saved data 就用默认构造；坏一条不能
    /// 只忽略那一条、把同文件里其它好条目套上去。
    #[test]
    fn a_bad_game_rule_entry_invalidates_the_whole_file() {
        let mut rules = GameRuleSet::vanilla();
        let index = gamerules::find("minecraft:random_tick_speed").unwrap();
        assert!(matches!(
            rules.assign(index, "7"),
            crate::gamerules::Assignment::Updated(_)
        ));
        let mut entries = match game_rules_tag(&rules) {
            Tag::Compound(entries) => entries,
            _ => unreachable!("game rules tag is a compound"),
        };
        // 一条坏条目：未知的规则名。
        entries.push(("minecraft:not_a_rule".to_owned(), Tag::Byte(1)));

        let mut restored = GameRuleSet::vanilla();
        apply_game_rules(&mut restored, &Tag::Compound(entries));
        assert_eq!(
            restored,
            GameRuleSet::vanilla(),
            "整份解码失败，全部退回默认值"
        );

        // 类型不符 / 越界 / 非复合标签也一律整份作废。
        for bad in [
            Tag::Compound(vec![(
                "minecraft:random_tick_speed".to_owned(),
                Tag::String("7".to_owned()),
            )]),
            Tag::Compound(vec![(
                "minecraft:random_tick_speed".to_owned(),
                Tag::Int(-1),
            )]),
            Tag::List(Vec::new()),
        ] {
            let mut restored = GameRuleSet::vanilla();
            apply_game_rules(&mut restored, &bad);
            assert_eq!(restored, GameRuleSet::vanilla(), "坏档 {bad:?}");
        }
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

    /// `world_clocks.dat` 把两份时钟的四个字段都存在，重新启动时逐字段套回。
    /// The four fields of both clocks round-trip through `world_clocks.dat`.
    #[test]
    fn world_clocks_round_trip_all_four_fields() {
        let mut global = crate::global::Global::default();
        let overworld = global.clock_named_mut("minecraft:overworld").unwrap();
        overworld.total_ticks = 42_000;
        overworld.partial_tick = 0.25;
        overworld.rate = 2.5;
        overworld.paused = false;
        let end = global.clock_named_mut("minecraft:the_end").unwrap();
        end.total_ticks = 123;
        end.paused = true;
        let tag = world_clocks_tag(&global);
        let parsed = parse_world_clocks(&tag).expect("合法的世界时钟存档");
        assert_eq!(parsed.len(), 2);

        let mut restored = crate::global::Global::default();
        for (name, state) in parsed {
            restored.restore_clock(name, state);
        }
        assert_eq!(restored.clocks(), global.clocks());

        // 官方 `ClockState.CODEC` 的三个可选字段：缺了就补默认值。
        let minimal = Tag::Compound(vec![(
            "minecraft:overworld".to_owned(),
            Tag::Compound(vec![("total_ticks".to_owned(), Tag::Long(7))]),
        )]);
        let parsed = parse_world_clocks(&minimal).expect("只带必填字段也合法");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].1.total_ticks, 7);
        assert_eq!(parsed[0].1.partial_tick, 0.0);
        assert_eq!(parsed[0].1.rate, 1.0);
        assert!(!parsed[0].1.paused);
    }

    /// 官方 `SavedDataStorage` 读坏整份 saved data 就用默认构造；一条坏的时钟
    /// 不能只忽略那一条、把同文件里其它好条目套上去。
    /// A bad entry invalidates the whole file, as official `resultOrPartial`
    /// plus the default constructor do.
    #[test]
    fn a_bad_world_clock_entry_invalidates_the_whole_file() {
        let good = (
            "minecraft:overworld".to_owned(),
            Tag::Compound(vec![("total_ticks".to_owned(), Tag::Long(9))]),
        );
        let bad = [
            // 必填字段缺失 / 类型错。
            Tag::Compound(vec![
                good.clone(),
                (
                    "minecraft:the_end".to_owned(),
                    Tag::Compound(vec![("rate".to_owned(), Tag::Float(1.0))]),
                ),
            ]),
            Tag::Compound(vec![(
                "minecraft:overworld".to_owned(),
                Tag::Compound(vec![("total_ticks".to_owned(), Tag::Int(1))]),
            )]),
            // 未知注册名（官方键 codec 会拒绝）。
            Tag::Compound(vec![
                good.clone(),
                (
                    "minecraft:not_a_clock".to_owned(),
                    Tag::Compound(vec![("total_ticks".to_owned(), Tag::Long(1))]),
                ),
            ]),
            // rate 必须严格大于 0；paused / partial_tick 类型不对也坏档。
            Tag::Compound(vec![(
                "minecraft:overworld".to_owned(),
                Tag::Compound(vec![
                    ("total_ticks".to_owned(), Tag::Long(1)),
                    ("rate".to_owned(), Tag::Float(0.0)),
                ]),
            )]),
            Tag::Compound(vec![(
                "minecraft:overworld".to_owned(),
                Tag::Compound(vec![
                    ("total_ticks".to_owned(), Tag::Long(1)),
                    ("paused".to_owned(), Tag::Int(1)),
                ]),
            )]),
            Tag::Compound(vec![(
                "minecraft:overworld".to_owned(),
                Tag::Compound(vec![
                    ("total_ticks".to_owned(), Tag::Long(1)),
                    ("partial_tick".to_owned(), Tag::Float(f32::NAN)),
                ]),
            )]),
            Tag::List(Vec::new()),
        ];
        for tag in bad {
            assert!(parse_world_clocks(&tag).is_none(), "坏档 {tag:?}");
        }
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
        sync_level_data(
            &mut level,
            &state,
            std::path::Path::new("/srv/my-world"),
            4242,
            999,
        );
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

    /// `setTagData` 的字段都来自运行时的真值，或官方专用服务端的默认值；
    /// 读入的旧值不会原样带回来。
    #[test]
    fn level_dat_fields_come_from_runtime_state_or_official_defaults() {
        use mc_bamboo_storage::level_dat::{DataPacks, LEVEL_FORMAT_VERSION, OVERWORLD};

        let mut level = LevelData {
            level_name: "stale name".to_owned(),
            server_brands: vec!["modded".to_owned()],
            was_modded: true,
            removed_features: vec!["minecraft:trade_rebalance".to_owned()],
            version_history: vec![100, 200],
            data_version: 1,
            game_type: 3,
            allow_commands: true,
            initialized: false,
            data_packs: Some(DataPacks {
                enabled: vec!["bamboo:pack".to_owned()],
                disabled: Vec::new(),
            }),
            enabled_features: vec!["bamboo:feature".to_owned()],
            singleplayer_uuid: Some([1, 2, 3, 4]),
            spawn: mc_bamboo_storage::level_dat::SpawnData {
                dimension: "minecraft:the_end".to_owned(),
                ..Default::default()
            },
            difficulty: mc_bamboo_storage::level_dat::DifficultySettings {
                difficulty: "peaceful".to_owned(),
                hardcore: true,
                locked: false,
            },
            ..Default::default()
        };
        let state = WorldState::default();
        sync_level_data(
            &mut level,
            &state,
            std::path::Path::new("/srv/survival"),
            77,
            888,
        );

        assert_eq!(level.level_name, "survival", "LevelName 来自 level-name");
        assert_eq!(level.game_type, 1, "GameType 来自运行时默认游戏模式");
        assert_eq!(level.spawn.dimension, OVERWORLD);
        assert!(!level.difficulty.hardcore, "hardcore 按官方常量 false");
        assert_eq!(level.time, 77);
        assert_eq!(level.last_played, 888);
        assert!(!level.allow_commands, "allowCommands 按官方常量 false");
        assert!(level.initialized);
        assert_eq!(level.server_brands, vec!["vanilla".to_owned()]);
        assert!(!level.was_modded);
        assert!(level.removed_features.is_empty());
        assert_eq!(
            level.version_history,
            vec![mc_bamboo_data::version::DATA_VERSION]
        );
        assert_eq!(level.data_version, mc_bamboo_data::version::DATA_VERSION);
        assert_eq!(level.format_version, LEVEL_FORMAT_VERSION);
        assert!(level.data_packs.is_none(), "DataPacks 是官方默认，不写键");
        assert_eq!(level.enabled_features, vec!["minecraft:vanilla".to_owned()]);
        assert!(level.singleplayer_uuid.is_none());
    }

    /// 两个岛的袭击汇总成一份 raids.dat：id 从各表 next_id 的最大值起重新分配，
    /// tick 取最大值，读回后字段完整（运行时字段按官方清空）。
    #[test]
    fn raids_tag_rolls_up_islands_without_id_collisions() {
        use mc_bamboo_blocks::entity::raid::{Raid, RaidStatus};
        let mut first = Raids::new();
        first.next_id = 4;
        first.tick = 120;
        let mut ongoing = Raid::new(BlockPos::new(0, 64, 0), 2);
        ongoing.started = true;
        ongoing.raid_omen_level = 3;
        first.raids.insert(1, ongoing.clone());
        let mut victory = Raid::new(BlockPos::new(100, 70, 0), 3);
        victory.status = RaidStatus::Victory;
        first.raids.insert(3, victory.clone());

        let mut second = Raids::new();
        second.next_id = 9;
        second.tick = 300;
        second.raids.insert(3, ongoing.clone());

        let tag = raids_tag(&[(0, first), (1, second)]);
        let decoded = Raids::from_tag(&tag);
        assert_eq!(decoded.next_id, 12, "三场从 9 起分配，下一次用 12");
        assert_eq!(decoded.tick, 300);
        assert_eq!(decoded.raids.len(), 3, "三场袭击各占一个不冲突的 id");
        let ids: Vec<i32> = decoded.raids.keys().copied().collect();
        assert_eq!(ids, [9, 10, 11], "从最大 next_id 起重新分配");
        // 按 (岛, 原 id) 顺序：家岛的 ongoing、victory，然后另一岛的 ongoing。
        assert_eq!(decoded.raids[&9].center, ongoing.center);
        assert_eq!(decoded.raids[&9].raid_omen_level, 3);
        assert_eq!(decoded.raids[&10].status, RaidStatus::Victory);
        assert_eq!(decoded.raids[&10].center, BlockPos::new(100, 70, 0));
        assert!(decoded.raids[&9].raiders.is_empty(), "运行时名册不存档");
    }

    /// 没有岛时仍写出合法的空 raids.dat，字段形状照官方。
    #[test]
    fn raids_tag_without_islands_is_a_valid_empty_table() {
        assert_eq!(raids_tag(&[]), Raids::new().to_tag());
    }
}
