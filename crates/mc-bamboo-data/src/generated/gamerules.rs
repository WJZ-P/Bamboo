// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。

/// 一条游戏规则。下标是 `minecraft:game_rule` 的注册表 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameRuleDef {
    pub name: &'static str,
    pub identifier: &'static str,
    pub category: &'static str,
    pub kind: GameRuleKind,
    pub default_bool: bool,
    pub default_int: i32,
    pub min: i32,
    pub max: i32,
    /// `GameRule.isEnabled(FeatureFlags.DEFAULT_FLAGS)`。
    pub enabled: bool,
    pub features: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameRuleKind {
    Bool,
    Int,
}

pub const ADVANCE_TIME: usize = 0;
pub const ADVANCE_WEATHER: usize = 1;
pub const ALLOW_ENTERING_NETHER_USING_PORTALS: usize = 2;
pub const BLOCK_DROPS: usize = 3;
pub const BLOCK_EXPLOSION_DROP_DECAY: usize = 4;
pub const COMMAND_BLOCKS_WORK: usize = 5;
pub const COMMAND_BLOCK_OUTPUT: usize = 6;
pub const DROWNING_DAMAGE: usize = 7;
pub const ELYTRA_MOVEMENT_CHECK: usize = 8;
pub const ENDER_PEARLS_VANISH_ON_DEATH: usize = 9;
pub const ENTITY_DROPS: usize = 10;
pub const FALL_DAMAGE: usize = 11;
pub const FIRE_DAMAGE: usize = 12;
pub const FIRE_SPREAD_RADIUS_AROUND_PLAYER: usize = 13;
pub const FORGIVE_DEAD_PLAYERS: usize = 14;
pub const FREEZE_DAMAGE: usize = 15;
pub const GLOBAL_SOUND_EVENTS: usize = 16;
pub const IMMEDIATE_RESPAWN: usize = 17;
pub const KEEP_INVENTORY: usize = 18;
pub const LAVA_SOURCE_CONVERSION: usize = 19;
pub const LIMITED_CRAFTING: usize = 20;
pub const LOCATOR_BAR: usize = 21;
pub const LOG_ADMIN_COMMANDS: usize = 22;
pub const MAX_BLOCK_MODIFICATIONS: usize = 23;
pub const MAX_COMMAND_FORKS: usize = 24;
pub const MAX_COMMAND_SEQUENCE_LENGTH: usize = 25;
pub const MAX_ENTITY_CRAMMING: usize = 26;
pub const MAX_MINECART_SPEED: usize = 27;
pub const MAX_SNOW_ACCUMULATION_HEIGHT: usize = 28;
pub const MOB_DROPS: usize = 29;
pub const MOB_EXPLOSION_DROP_DECAY: usize = 30;
pub const MOB_GRIEFING: usize = 31;
pub const NATURAL_HEALTH_REGENERATION: usize = 32;
pub const PLAYER_MOVEMENT_CHECK: usize = 33;
pub const PLAYERS_NETHER_PORTAL_CREATIVE_DELAY: usize = 34;
pub const PLAYERS_NETHER_PORTAL_DEFAULT_DELAY: usize = 35;
pub const PLAYERS_SLEEPING_PERCENTAGE: usize = 36;
pub const PROJECTILES_CAN_BREAK_BLOCKS: usize = 37;
pub const PVP: usize = 38;
pub const RAIDS: usize = 39;
pub const RANDOM_TICK_SPEED: usize = 40;
pub const REDUCED_DEBUG_INFO: usize = 41;
pub const RESPAWN_RADIUS: usize = 42;
pub const SEND_COMMAND_FEEDBACK: usize = 43;
pub const SHOW_ADVANCEMENT_MESSAGES: usize = 44;
pub const SHOW_DEATH_MESSAGES: usize = 45;
pub const SPAWNER_BLOCKS_WORK: usize = 46;
pub const SPAWN_MOBS: usize = 47;
pub const SPAWN_MONSTERS: usize = 48;
pub const SPAWN_PATROLS: usize = 49;
pub const SPAWN_PHANTOMS: usize = 50;
pub const SPAWN_WANDERING_TRADERS: usize = 51;
pub const SPAWN_WARDENS: usize = 52;
pub const SPECTATORS_GENERATE_CHUNKS: usize = 53;
pub const SPREAD_VINES: usize = 54;
pub const TNT_EXPLODES: usize = 55;
pub const TNT_EXPLOSION_DROP_DECAY: usize = 56;
pub const UNIVERSAL_ANGER: usize = 57;
pub const WATER_SOURCE_CONVERSION: usize = 58;

pub static RULES: &[GameRuleDef] = &[
    GameRuleDef { name: "advance_time", identifier: "minecraft:advance_time", category: "minecraft:updates", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "advance_weather", identifier: "minecraft:advance_weather", category: "minecraft:updates", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "allow_entering_nether_using_portals", identifier: "minecraft:allow_entering_nether_using_portals", category: "minecraft:misc", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "block_drops", identifier: "minecraft:block_drops", category: "minecraft:drops", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "block_explosion_drop_decay", identifier: "minecraft:block_explosion_drop_decay", category: "minecraft:drops", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "command_blocks_work", identifier: "minecraft:command_blocks_work", category: "minecraft:misc", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "command_block_output", identifier: "minecraft:command_block_output", category: "minecraft:chat", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "drowning_damage", identifier: "minecraft:drowning_damage", category: "minecraft:player", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "elytra_movement_check", identifier: "minecraft:elytra_movement_check", category: "minecraft:player", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "ender_pearls_vanish_on_death", identifier: "minecraft:ender_pearls_vanish_on_death", category: "minecraft:player", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "entity_drops", identifier: "minecraft:entity_drops", category: "minecraft:drops", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "fall_damage", identifier: "minecraft:fall_damage", category: "minecraft:player", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "fire_damage", identifier: "minecraft:fire_damage", category: "minecraft:player", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "fire_spread_radius_around_player", identifier: "minecraft:fire_spread_radius_around_player", category: "minecraft:updates", kind: GameRuleKind::Int, default_bool: false, default_int: 128, min: -1, max: 2147483647, enabled: true, features: &[] },
    GameRuleDef { name: "forgive_dead_players", identifier: "minecraft:forgive_dead_players", category: "minecraft:mobs", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "freeze_damage", identifier: "minecraft:freeze_damage", category: "minecraft:player", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "global_sound_events", identifier: "minecraft:global_sound_events", category: "minecraft:misc", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "immediate_respawn", identifier: "minecraft:immediate_respawn", category: "minecraft:player", kind: GameRuleKind::Bool, default_bool: false, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "keep_inventory", identifier: "minecraft:keep_inventory", category: "minecraft:player", kind: GameRuleKind::Bool, default_bool: false, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "lava_source_conversion", identifier: "minecraft:lava_source_conversion", category: "minecraft:updates", kind: GameRuleKind::Bool, default_bool: false, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "limited_crafting", identifier: "minecraft:limited_crafting", category: "minecraft:player", kind: GameRuleKind::Bool, default_bool: false, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "locator_bar", identifier: "minecraft:locator_bar", category: "minecraft:player", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "log_admin_commands", identifier: "minecraft:log_admin_commands", category: "minecraft:chat", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "max_block_modifications", identifier: "minecraft:max_block_modifications", category: "minecraft:misc", kind: GameRuleKind::Int, default_bool: false, default_int: 32768, min: 1, max: 2147483647, enabled: true, features: &[] },
    GameRuleDef { name: "max_command_forks", identifier: "minecraft:max_command_forks", category: "minecraft:misc", kind: GameRuleKind::Int, default_bool: false, default_int: 65536, min: 0, max: 2147483647, enabled: true, features: &[] },
    GameRuleDef { name: "max_command_sequence_length", identifier: "minecraft:max_command_sequence_length", category: "minecraft:misc", kind: GameRuleKind::Int, default_bool: false, default_int: 65536, min: 0, max: 2147483647, enabled: true, features: &[] },
    GameRuleDef { name: "max_entity_cramming", identifier: "minecraft:max_entity_cramming", category: "minecraft:mobs", kind: GameRuleKind::Int, default_bool: false, default_int: 24, min: 0, max: 2147483647, enabled: true, features: &[] },
    GameRuleDef { name: "max_minecart_speed", identifier: "minecraft:max_minecart_speed", category: "minecraft:misc", kind: GameRuleKind::Int, default_bool: false, default_int: 8, min: 1, max: 1000, enabled: false, features: &["minecraft:minecart_improvements"] },
    GameRuleDef { name: "max_snow_accumulation_height", identifier: "minecraft:max_snow_accumulation_height", category: "minecraft:updates", kind: GameRuleKind::Int, default_bool: false, default_int: 1, min: 0, max: 8, enabled: true, features: &[] },
    GameRuleDef { name: "mob_drops", identifier: "minecraft:mob_drops", category: "minecraft:drops", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "mob_explosion_drop_decay", identifier: "minecraft:mob_explosion_drop_decay", category: "minecraft:drops", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "mob_griefing", identifier: "minecraft:mob_griefing", category: "minecraft:mobs", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "natural_health_regeneration", identifier: "minecraft:natural_health_regeneration", category: "minecraft:player", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "player_movement_check", identifier: "minecraft:player_movement_check", category: "minecraft:player", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "players_nether_portal_creative_delay", identifier: "minecraft:players_nether_portal_creative_delay", category: "minecraft:player", kind: GameRuleKind::Int, default_bool: false, default_int: 0, min: 0, max: 2147483647, enabled: true, features: &[] },
    GameRuleDef { name: "players_nether_portal_default_delay", identifier: "minecraft:players_nether_portal_default_delay", category: "minecraft:player", kind: GameRuleKind::Int, default_bool: false, default_int: 80, min: 0, max: 2147483647, enabled: true, features: &[] },
    GameRuleDef { name: "players_sleeping_percentage", identifier: "minecraft:players_sleeping_percentage", category: "minecraft:player", kind: GameRuleKind::Int, default_bool: false, default_int: 100, min: 0, max: 2147483647, enabled: true, features: &[] },
    GameRuleDef { name: "projectiles_can_break_blocks", identifier: "minecraft:projectiles_can_break_blocks", category: "minecraft:drops", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "pvp", identifier: "minecraft:pvp", category: "minecraft:player", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "raids", identifier: "minecraft:raids", category: "minecraft:mobs", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "random_tick_speed", identifier: "minecraft:random_tick_speed", category: "minecraft:updates", kind: GameRuleKind::Int, default_bool: false, default_int: 3, min: 0, max: 2147483647, enabled: true, features: &[] },
    GameRuleDef { name: "reduced_debug_info", identifier: "minecraft:reduced_debug_info", category: "minecraft:misc", kind: GameRuleKind::Bool, default_bool: false, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "respawn_radius", identifier: "minecraft:respawn_radius", category: "minecraft:player", kind: GameRuleKind::Int, default_bool: false, default_int: 10, min: 0, max: 2147483647, enabled: true, features: &[] },
    GameRuleDef { name: "send_command_feedback", identifier: "minecraft:send_command_feedback", category: "minecraft:chat", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "show_advancement_messages", identifier: "minecraft:show_advancement_messages", category: "minecraft:chat", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "show_death_messages", identifier: "minecraft:show_death_messages", category: "minecraft:chat", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "spawner_blocks_work", identifier: "minecraft:spawner_blocks_work", category: "minecraft:misc", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "spawn_mobs", identifier: "minecraft:spawn_mobs", category: "minecraft:spawning", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "spawn_monsters", identifier: "minecraft:spawn_monsters", category: "minecraft:spawning", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "spawn_patrols", identifier: "minecraft:spawn_patrols", category: "minecraft:spawning", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "spawn_phantoms", identifier: "minecraft:spawn_phantoms", category: "minecraft:spawning", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "spawn_wandering_traders", identifier: "minecraft:spawn_wandering_traders", category: "minecraft:spawning", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "spawn_wardens", identifier: "minecraft:spawn_wardens", category: "minecraft:spawning", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "spectators_generate_chunks", identifier: "minecraft:spectators_generate_chunks", category: "minecraft:player", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "spread_vines", identifier: "minecraft:spread_vines", category: "minecraft:updates", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "tnt_explodes", identifier: "minecraft:tnt_explodes", category: "minecraft:misc", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "tnt_explosion_drop_decay", identifier: "minecraft:tnt_explosion_drop_decay", category: "minecraft:drops", kind: GameRuleKind::Bool, default_bool: false, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "universal_anger", identifier: "minecraft:universal_anger", category: "minecraft:mobs", kind: GameRuleKind::Bool, default_bool: false, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
    GameRuleDef { name: "water_source_conversion", identifier: "minecraft:water_source_conversion", category: "minecraft:updates", kind: GameRuleKind::Bool, default_bool: true, default_int: 0, min: 0, max: 0, enabled: true, features: &[] },
];

/// 短名或 `minecraft:` 全名。没注册的返回 `None`。
pub fn find(name: &str) -> Option<usize> {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    RULES.iter().position(|rule| rule.name == name)
}
