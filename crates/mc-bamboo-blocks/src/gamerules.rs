//! 方块逻辑要读的那几条游戏规则。
//!
//! 权威副本在世界线程的 `Global` 上。这里只放方块代码用得到的快照，默认值和
//! `mc-bamboo-data` 里导出的官方表一致。测试和 oracle 不改规则时行为与原来相同。
//!
//! The game rules block logic reads.
//!
//! The authoritative copy lives on the world thread's `Global`. This is only the snapshot
//! block code needs. Defaults match the official table exported into `mc-bamboo-data`.
//! Tests and the oracle keep the old behaviour when they leave the rules alone.

use mc_bamboo_data::gamerules::{self, GameRuleKind, RULES};

/// 方块和流体用到的游戏规则。
///
/// Game rules read by blocks and fluids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockGameRules {
    /// `GameRules.RANDOM_TICK_SPEED`。`ServerLevel.tickChunk` 的第二个参数。
    pub random_tick_speed: i32,
    /// `GameRules.FIRE_SPREAD_RADIUS_AROUND_PLAYER`。`-1` 表示不限距离。
    ///
    /// `-1` means the distance is unlimited.
    pub fire_spread_radius_around_player: i32,
    /// `GameRules.TNT_EXPLODES`。`TntBlock.prime` 和 `wasExploded` 先看它。
    pub tnt_explodes: bool,
    /// `GameRules.BLOCK_DROPS`。`Block.dropResources` 在生成掉落物之前看它。
    ///
    /// `Block.dropResources` checks this before spawning drops.
    pub block_drops: bool,
    /// `WaterFluid.canConvertToSource` 读的 `GameRules.WATER_SOURCE_CONVERSION`。
    pub water_source_conversion: bool,
    /// `LavaFluid.canConvertToSource` 读的 `GameRules.LAVA_SOURCE_CONVERSION`。
    pub lava_source_conversion: bool,
    /// `VineBlock.randomTick` 开头的 `GameRules.SPREAD_VINES`。
    pub spread_vines: bool,
    /// `GameRules.MAX_BLOCK_MODIFICATIONS`。`FillCommand` 用它限制体积。
    pub max_block_modifications: i32,
    /// `GameRules.MAX_SNOW_ACCUMULATION_HEIGHT`。`ServerLevel.tickPrecipitation` 在下雨时读它。
    ///
    /// `ServerLevel.tickPrecipitation` reads this while it is raining.
    pub max_snow_accumulation_height: i32,
}

impl BlockGameRules {
    /// 官方 `new GameRules(FeatureFlags.DEFAULT_FLAGS)` 里这些规则的默认值。
    ///
    /// Defaults from `new GameRules(FeatureFlags.DEFAULT_FLAGS)`.
    pub fn vanilla() -> Self {
        Self {
            random_tick_speed: int_default(gamerules::RANDOM_TICK_SPEED),
            fire_spread_radius_around_player: int_default(
                gamerules::FIRE_SPREAD_RADIUS_AROUND_PLAYER,
            ),
            tnt_explodes: bool_default(gamerules::TNT_EXPLODES),
            block_drops: bool_default(gamerules::BLOCK_DROPS),
            water_source_conversion: bool_default(gamerules::WATER_SOURCE_CONVERSION),
            lava_source_conversion: bool_default(gamerules::LAVA_SOURCE_CONVERSION),
            spread_vines: bool_default(gamerules::SPREAD_VINES),
            max_block_modifications: int_default(gamerules::MAX_BLOCK_MODIFICATIONS),
            max_snow_accumulation_height: int_default(gamerules::MAX_SNOW_ACCUMULATION_HEIGHT),
        }
    }
}

fn bool_default(index: usize) -> bool {
    let rule = &RULES[index];
    debug_assert!(rule.kind == GameRuleKind::Bool);
    rule.default_bool
}

fn int_default(index: usize) -> i32 {
    let rule = &RULES[index];
    debug_assert!(rule.kind == GameRuleKind::Int);
    rule.default_int
}

/// `Block.dropResources` 会不会生成掉落物。
///
/// 创造模式的 `preventsBlockDrops`（`instabuild`）另外挡住掉落，和这条规则是两回事。
/// 物品实体还没接上，调用方只留下这个判断。
///
/// Whether `Block.dropResources` would spawn drops.
///
/// Creative `preventsBlockDrops` (`instabuild`) is a separate gate. Item entities are not
/// wired yet; callers keep this check in place.
pub fn block_drops_allowed(rules: BlockGameRules, prevents_block_drops: bool) -> bool {
    rules.block_drops && !prevents_block_drops
}

/// 用官方序列化过的文本覆盖规则。没列出的规则保持默认值。
///
/// 名字必须是已启用的规则，值必须能被官方参数类型整段吃掉。不影响方块的规则只做校验。
///
/// Overrides rules with officially serialized text. Rules that are absent keep their defaults.
///
/// The name must be an enabled rule, and the value must be consumed by the official argument
/// type. Rules that do not affect blocks are only validated.
pub fn apply_text(rules: &mut BlockGameRules, name: &str, value: &str) -> Result<(), String> {
    let index = gamerules::find(name).ok_or_else(|| format!("unknown game rule {name}"))?;
    let rule = &RULES[index];
    if !rule.enabled {
        return Err(format!("game rule {name} is not enabled"));
    }
    match rule.kind {
        GameRuleKind::Bool => {
            let parsed = parse_bool(value)?;
            assign_bool(rules, rule.name, parsed);
        }
        GameRuleKind::Int => {
            let parsed = parse_int(value, rule.min, rule.max)?;
            assign_int(rules, rule.name, parsed);
        }
    }
    Ok(())
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("invalid boolean {value}")),
    }
}

fn parse_int(value: &str, min: i32, max: i32) -> Result<i32, String> {
    let parsed: i32 = value
        .parse()
        .map_err(|_| format!("invalid integer {value}"))?;
    if parsed < min || parsed > max {
        return Err(format!("integer {parsed} is outside {min}..={max}"));
    }
    Ok(parsed)
}

fn assign_bool(rules: &mut BlockGameRules, name: &str, value: bool) {
    match name {
        "tnt_explodes" => rules.tnt_explodes = value,
        "block_drops" => rules.block_drops = value,
        "water_source_conversion" => rules.water_source_conversion = value,
        "lava_source_conversion" => rules.lava_source_conversion = value,
        "spread_vines" => rules.spread_vines = value,
        _ => {}
    }
}

fn assign_int(rules: &mut BlockGameRules, name: &str, value: i32) {
    match name {
        "random_tick_speed" => rules.random_tick_speed = value,
        "fire_spread_radius_around_player" => rules.fire_spread_radius_around_player = value,
        "max_block_modifications" => rules.max_block_modifications = value,
        "max_snow_accumulation_height" => rules.max_snow_accumulation_height = value,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_matches_the_generated_defaults() {
        let rules = BlockGameRules::vanilla();
        assert_eq!(
            rules.random_tick_speed,
            crate::random_tick::DEFAULT_RANDOM_TICK_SPEED
        );
        assert_eq!(
            rules.random_tick_speed,
            RULES[gamerules::RANDOM_TICK_SPEED].default_int
        );
        assert_eq!(
            rules.fire_spread_radius_around_player,
            RULES[gamerules::FIRE_SPREAD_RADIUS_AROUND_PLAYER].default_int
        );
        assert_eq!(
            rules.tnt_explodes,
            RULES[gamerules::TNT_EXPLODES].default_bool
        );
        assert_eq!(
            rules.block_drops,
            RULES[gamerules::BLOCK_DROPS].default_bool
        );
        assert_eq!(
            rules.water_source_conversion,
            RULES[gamerules::WATER_SOURCE_CONVERSION].default_bool
        );
        assert_eq!(
            rules.lava_source_conversion,
            RULES[gamerules::LAVA_SOURCE_CONVERSION].default_bool
        );
        assert_eq!(
            rules.spread_vines,
            RULES[gamerules::SPREAD_VINES].default_bool
        );
        assert_eq!(
            rules.max_block_modifications,
            RULES[gamerules::MAX_BLOCK_MODIFICATIONS].default_int
        );
        assert_eq!(
            rules.max_snow_accumulation_height,
            RULES[gamerules::MAX_SNOW_ACCUMULATION_HEIGHT].default_int
        );
    }

    #[test]
    fn text_overrides_reject_unknown_and_out_of_range_values() {
        let mut rules = BlockGameRules::vanilla();
        apply_text(&mut rules, "tnt_explodes", "false").unwrap();
        assert!(!rules.tnt_explodes);
        assert!(apply_text(&mut rules, "no_such_rule", "true").is_err());
        assert!(apply_text(&mut rules, "random_tick_speed", "-1").is_err());
        assert!(apply_text(&mut rules, "tnt_explodes", "TRUE").is_err());
        assert!(apply_text(&mut rules, "max_minecart_speed", "8").is_err());
    }
}
