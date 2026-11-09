// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。

use crate::{KnownPack, SyncedRegistry};

/// 客户端需要认识的官方内容包。
pub const KNOWN_PACKS: &[KnownPack] = &[
    KnownPack { namespace: "minecraft", id: "core", version: "26.3" },
];

/// 启用的功能开关。
pub const ENABLED_FEATURES: &[&str] = &["minecraft:vanilla"];

/// 配置阶段要同步给客户端的注册表，顺序和官方一致。
pub const SYNCED_REGISTRIES: &[SyncedRegistry] = &[
    SyncedRegistry { name: "minecraft:worldgen/biome", entries: &["minecraft:badlands", "minecraft:bamboo_jungle", "minecraft:basalt_deltas", "minecraft:beach", "minecraft:birch_forest", "minecraft:cherry_grove", "minecraft:cold_ocean", "minecraft:crimson_forest", "minecraft:dappled_forest", "minecraft:dark_forest", "minecraft:deep_cold_ocean", "minecraft:deep_dark", "minecraft:deep_frozen_ocean", "minecraft:deep_lukewarm_ocean", "minecraft:deep_ocean", "minecraft:desert", "minecraft:dripstone_caves", "minecraft:end_barrens", "minecraft:end_highlands", "minecraft:end_midlands", "minecraft:eroded_badlands", "minecraft:flower_forest", "minecraft:forest", "minecraft:frozen_ocean", "minecraft:frozen_peaks", "minecraft:frozen_river", "minecraft:grove", "minecraft:ice_spikes", "minecraft:jagged_peaks", "minecraft:jungle", "minecraft:lukewarm_ocean", "minecraft:lush_caves", "minecraft:mangrove_swamp", "minecraft:meadow", "minecraft:mushroom_fields", "minecraft:nether_wastes", "minecraft:ocean", "minecraft:old_growth_birch_forest", "minecraft:old_growth_pine_taiga", "minecraft:old_growth_spruce_taiga", "minecraft:pale_garden", "minecraft:plains", "minecraft:river", "minecraft:savanna", "minecraft:savanna_plateau", "minecraft:small_end_islands", "minecraft:snowy_beach", "minecraft:snowy_plains", "minecraft:snowy_slopes", "minecraft:snowy_taiga", "minecraft:soul_sand_valley", "minecraft:sparse_jungle", "minecraft:stony_peaks", "minecraft:stony_shore", "minecraft:sulfur_caves", "minecraft:sunflower_plains", "minecraft:swamp", "minecraft:taiga", "minecraft:the_end", "minecraft:the_void", "minecraft:warm_ocean", "minecraft:warped_forest", "minecraft:windswept_forest", "minecraft:windswept_gravelly_hills", "minecraft:windswept_hills", "minecraft:windswept_savanna", "minecraft:wooded_badlands"] },
    SyncedRegistry { name: "minecraft:chat_type", entries: &["minecraft:chat", "minecraft:emote_command", "minecraft:msg_command_incoming", "minecraft:msg_command_outgoing", "minecraft:say_command", "minecraft:team_msg_command_incoming", "minecraft:team_msg_command_outgoing"] },
    SyncedRegistry { name: "minecraft:trim_pattern", entries: &["minecraft:bolt", "minecraft:coast", "minecraft:dune", "minecraft:eye", "minecraft:flow", "minecraft:host", "minecraft:raiser", "minecraft:rib", "minecraft:sentry", "minecraft:shaper", "minecraft:silence", "minecraft:snout", "minecraft:spire", "minecraft:tide", "minecraft:vex", "minecraft:ward", "minecraft:wayfinder", "minecraft:wild"] },
    SyncedRegistry { name: "minecraft:trim_material", entries: &["minecraft:amethyst", "minecraft:copper", "minecraft:diamond", "minecraft:emerald", "minecraft:gold", "minecraft:iron", "minecraft:lapis", "minecraft:netherite", "minecraft:quartz", "minecraft:redstone", "minecraft:resin"] },
    SyncedRegistry { name: "minecraft:wolf_variant", entries: &["minecraft:ashen", "minecraft:black", "minecraft:chestnut", "minecraft:pale", "minecraft:rusty", "minecraft:snowy", "minecraft:spotted", "minecraft:striped", "minecraft:woods"] },
    SyncedRegistry { name: "minecraft:wolf_sound_variant", entries: &["minecraft:angry", "minecraft:big", "minecraft:classic", "minecraft:cute", "minecraft:grumpy", "minecraft:puglin", "minecraft:sad"] },
    SyncedRegistry { name: "minecraft:pig_variant", entries: &["minecraft:cold", "minecraft:temperate", "minecraft:warm"] },
    SyncedRegistry { name: "minecraft:pig_sound_variant", entries: &["minecraft:big", "minecraft:classic", "minecraft:mini"] },
    SyncedRegistry { name: "minecraft:frog_variant", entries: &["minecraft:cold", "minecraft:temperate", "minecraft:warm"] },
    SyncedRegistry { name: "minecraft:cat_variant", entries: &["minecraft:all_black", "minecraft:black", "minecraft:british_shorthair", "minecraft:calico", "minecraft:jellie", "minecraft:persian", "minecraft:ragdoll", "minecraft:red", "minecraft:siamese", "minecraft:tabby", "minecraft:white"] },
    SyncedRegistry { name: "minecraft:cat_sound_variant", entries: &["minecraft:classic", "minecraft:royal"] },
    SyncedRegistry { name: "minecraft:cow_sound_variant", entries: &["minecraft:classic", "minecraft:moody"] },
    SyncedRegistry { name: "minecraft:cow_variant", entries: &["minecraft:cold", "minecraft:temperate", "minecraft:warm"] },
    SyncedRegistry { name: "minecraft:chicken_sound_variant", entries: &["minecraft:classic", "minecraft:picky"] },
    SyncedRegistry { name: "minecraft:chicken_variant", entries: &["minecraft:cold", "minecraft:temperate", "minecraft:warm"] },
    SyncedRegistry { name: "minecraft:zombie_nautilus_variant", entries: &["minecraft:temperate", "minecraft:warm"] },
    SyncedRegistry { name: "minecraft:painting_variant", entries: &["minecraft:alban", "minecraft:aztec", "minecraft:aztec2", "minecraft:backyard", "minecraft:baroque", "minecraft:bomb", "minecraft:bouquet", "minecraft:burning_skull", "minecraft:bust", "minecraft:cavebird", "minecraft:changing", "minecraft:cotan", "minecraft:courbet", "minecraft:creebet", "minecraft:dennis", "minecraft:donkey_kong", "minecraft:earth", "minecraft:endboss", "minecraft:fern", "minecraft:fighters", "minecraft:finding", "minecraft:fire", "minecraft:graham", "minecraft:humble", "minecraft:kebab", "minecraft:lowmist", "minecraft:match", "minecraft:meditative", "minecraft:orb", "minecraft:owlemons", "minecraft:passage", "minecraft:pigscene", "minecraft:plant", "minecraft:pointer", "minecraft:pond", "minecraft:pool", "minecraft:prairie_ride", "minecraft:sea", "minecraft:skeleton", "minecraft:skull_and_roses", "minecraft:stage", "minecraft:sunflowers", "minecraft:sunset", "minecraft:tides", "minecraft:unpacked", "minecraft:void", "minecraft:wanderer", "minecraft:wasteland", "minecraft:water", "minecraft:wind", "minecraft:wither"] },
    SyncedRegistry { name: "minecraft:sulfur_cube_archetype", entries: &["minecraft:bouncy", "minecraft:explosive", "minecraft:fast_flat", "minecraft:fast_sliding", "minecraft:high_resistance", "minecraft:hot", "minecraft:light", "minecraft:regular", "minecraft:slow_bouncy", "minecraft:slow_flat", "minecraft:slow_sliding", "minecraft:sticky"] },
    SyncedRegistry { name: "minecraft:dimension_type", entries: &["minecraft:overworld", "minecraft:overworld_caves", "minecraft:the_end", "minecraft:the_nether"] },
    SyncedRegistry { name: "minecraft:damage_type", entries: &["minecraft:arrow", "minecraft:bad_respawn_point", "minecraft:cactus", "minecraft:campfire", "minecraft:cramming", "minecraft:dragon_breath", "minecraft:drown", "minecraft:dry_out", "minecraft:ender_pearl", "minecraft:explosion", "minecraft:fall", "minecraft:falling_anvil", "minecraft:falling_block", "minecraft:falling_stalactite", "minecraft:fireball", "minecraft:fireworks", "minecraft:fly_into_wall", "minecraft:freeze", "minecraft:generic", "minecraft:generic_kill", "minecraft:hot_floor", "minecraft:in_fire", "minecraft:in_wall", "minecraft:indirect_magic", "minecraft:lava", "minecraft:lightning_bolt", "minecraft:mace_smash", "minecraft:magic", "minecraft:mob_attack", "minecraft:mob_attack_no_aggro", "minecraft:mob_projectile", "minecraft:on_fire", "minecraft:out_of_world", "minecraft:outside_border", "minecraft:player_attack", "minecraft:player_explosion", "minecraft:sonic_boom", "minecraft:spear", "minecraft:spit", "minecraft:stalagmite", "minecraft:starve", "minecraft:sting", "minecraft:sulfur_cube_hot", "minecraft:sweet_berry_bush", "minecraft:thorns", "minecraft:thrown", "minecraft:trident", "minecraft:unattributed_fireball", "minecraft:wind_charge", "minecraft:wither", "minecraft:wither_skull"] },
    SyncedRegistry { name: "minecraft:banner_pattern", entries: &["minecraft:base", "minecraft:border", "minecraft:bricks", "minecraft:circle", "minecraft:creeper", "minecraft:cross", "minecraft:curly_border", "minecraft:diagonal_left", "minecraft:diagonal_right", "minecraft:diagonal_up_left", "minecraft:diagonal_up_right", "minecraft:flow", "minecraft:flower", "minecraft:globe", "minecraft:gradient", "minecraft:gradient_up", "minecraft:guster", "minecraft:half_horizontal", "minecraft:half_horizontal_bottom", "minecraft:half_vertical", "minecraft:half_vertical_right", "minecraft:mojang", "minecraft:piglin", "minecraft:rhombus", "minecraft:skull", "minecraft:small_stripes", "minecraft:square_bottom_left", "minecraft:square_bottom_right", "minecraft:square_top_left", "minecraft:square_top_right", "minecraft:straight_cross", "minecraft:stripe_bottom", "minecraft:stripe_center", "minecraft:stripe_downleft", "minecraft:stripe_downright", "minecraft:stripe_left", "minecraft:stripe_middle", "minecraft:stripe_right", "minecraft:stripe_top", "minecraft:triangle_bottom", "minecraft:triangle_top", "minecraft:triangles_bottom", "minecraft:triangles_top"] },
    SyncedRegistry { name: "minecraft:enchantment", entries: &["minecraft:aqua_affinity", "minecraft:bane_of_arthropods", "minecraft:binding_curse", "minecraft:blast_protection", "minecraft:breach", "minecraft:channeling", "minecraft:density", "minecraft:depth_strider", "minecraft:efficiency", "minecraft:feather_falling", "minecraft:fire_aspect", "minecraft:fire_protection", "minecraft:flame", "minecraft:fortune", "minecraft:frost_walker", "minecraft:impaling", "minecraft:infinity", "minecraft:knockback", "minecraft:looting", "minecraft:loyalty", "minecraft:luck_of_the_sea", "minecraft:lunge", "minecraft:lure", "minecraft:mending", "minecraft:multishot", "minecraft:piercing", "minecraft:power", "minecraft:projectile_protection", "minecraft:protection", "minecraft:punch", "minecraft:quick_charge", "minecraft:respiration", "minecraft:riptide", "minecraft:sharpness", "minecraft:silk_touch", "minecraft:smite", "minecraft:soul_speed", "minecraft:sweeping_edge", "minecraft:swift_sneak", "minecraft:thorns", "minecraft:unbreaking", "minecraft:vanishing_curse", "minecraft:wind_burst"] },
    SyncedRegistry { name: "minecraft:jukebox_song", entries: &["minecraft:11", "minecraft:13", "minecraft:5", "minecraft:blocks", "minecraft:bounce", "minecraft:cat", "minecraft:chirp", "minecraft:creator", "minecraft:creator_music_box", "minecraft:far", "minecraft:lava_chicken", "minecraft:mall", "minecraft:mellohi", "minecraft:otherside", "minecraft:pigstep", "minecraft:precipice", "minecraft:relic", "minecraft:stal", "minecraft:strad", "minecraft:tears", "minecraft:wait", "minecraft:ward"] },
    SyncedRegistry { name: "minecraft:instrument", entries: &["minecraft:admire_goat_horn", "minecraft:call_goat_horn", "minecraft:dream_goat_horn", "minecraft:feel_goat_horn", "minecraft:ponder_goat_horn", "minecraft:seek_goat_horn", "minecraft:sing_goat_horn", "minecraft:yearn_goat_horn"] },
    SyncedRegistry { name: "minecraft:test_environment", entries: &["minecraft:default"] },
    SyncedRegistry { name: "minecraft:test_instance", entries: &["minecraft:always_pass"] },
    SyncedRegistry { name: "minecraft:dialog", entries: &["minecraft:custom_options", "minecraft:quick_actions", "minecraft:server_links"] },
    SyncedRegistry { name: "minecraft:world_clock", entries: &["minecraft:overworld", "minecraft:the_end"] },
    SyncedRegistry { name: "minecraft:timeline", entries: &["minecraft:day", "minecraft:early_game", "minecraft:moon", "minecraft:villager_schedule"] },
    SyncedRegistry { name: "minecraft:decorated_pot_pattern", entries: &["minecraft:angler", "minecraft:archer", "minecraft:arms_up", "minecraft:blade", "minecraft:brewer", "minecraft:burn", "minecraft:danger", "minecraft:explorer", "minecraft:flow", "minecraft:friend", "minecraft:guster", "minecraft:heart", "minecraft:heartbreak", "minecraft:howl", "minecraft:miner", "minecraft:mourner", "minecraft:plenty", "minecraft:prize", "minecraft:scrape", "minecraft:sheaf", "minecraft:shelter", "minecraft:skull", "minecraft:snort"] },
    SyncedRegistry { name: "minecraft:block_transformer", entries: &["minecraft:axe", "minecraft:hoe", "minecraft:shovel"] },
    SyncedRegistry { name: "minecraft:worldgen/block_state_provider", entries: &["minecraft:cave_vines_body", "minecraft:cave_vines_head", "minecraft:flower_flower_forest", "minecraft:flower_meadow", "minecraft:flower_plain", "minecraft:mangrove_propagule", "minecraft:podzol_beneath_tree", "minecraft:soil_beneath_tree"] },
];

pub mod biome {
    pub const TOTAL: u32 = 67;
    /// 条目名，下标就是注册表 ID。
    pub static NAMES: &[&str] = &["minecraft:badlands", "minecraft:bamboo_jungle", "minecraft:basalt_deltas", "minecraft:beach", "minecraft:birch_forest", "minecraft:cherry_grove", "minecraft:cold_ocean", "minecraft:crimson_forest", "minecraft:dappled_forest", "minecraft:dark_forest", "minecraft:deep_cold_ocean", "minecraft:deep_dark", "minecraft:deep_frozen_ocean", "minecraft:deep_lukewarm_ocean", "minecraft:deep_ocean", "minecraft:desert", "minecraft:dripstone_caves", "minecraft:end_barrens", "minecraft:end_highlands", "minecraft:end_midlands", "minecraft:eroded_badlands", "minecraft:flower_forest", "minecraft:forest", "minecraft:frozen_ocean", "minecraft:frozen_peaks", "minecraft:frozen_river", "minecraft:grove", "minecraft:ice_spikes", "minecraft:jagged_peaks", "minecraft:jungle", "minecraft:lukewarm_ocean", "minecraft:lush_caves", "minecraft:mangrove_swamp", "minecraft:meadow", "minecraft:mushroom_fields", "minecraft:nether_wastes", "minecraft:ocean", "minecraft:old_growth_birch_forest", "minecraft:old_growth_pine_taiga", "minecraft:old_growth_spruce_taiga", "minecraft:pale_garden", "minecraft:plains", "minecraft:river", "minecraft:savanna", "minecraft:savanna_plateau", "minecraft:small_end_islands", "minecraft:snowy_beach", "minecraft:snowy_plains", "minecraft:snowy_slopes", "minecraft:snowy_taiga", "minecraft:soul_sand_valley", "minecraft:sparse_jungle", "minecraft:stony_peaks", "minecraft:stony_shore", "minecraft:sulfur_caves", "minecraft:sunflower_plains", "minecraft:swamp", "minecraft:taiga", "minecraft:the_end", "minecraft:the_void", "minecraft:warm_ocean", "minecraft:warped_forest", "minecraft:windswept_forest", "minecraft:windswept_gravelly_hills", "minecraft:windswept_hills", "minecraft:windswept_savanna", "minecraft:wooded_badlands"];
    pub const BADLANDS: i32 = 0;
    pub const BAMBOO_JUNGLE: i32 = 1;
    pub const BASALT_DELTAS: i32 = 2;
    pub const BEACH: i32 = 3;
    pub const BIRCH_FOREST: i32 = 4;
    pub const CHERRY_GROVE: i32 = 5;
    pub const COLD_OCEAN: i32 = 6;
    pub const CRIMSON_FOREST: i32 = 7;
    pub const DAPPLED_FOREST: i32 = 8;
    pub const DARK_FOREST: i32 = 9;
    pub const DEEP_COLD_OCEAN: i32 = 10;
    pub const DEEP_DARK: i32 = 11;
    pub const DEEP_FROZEN_OCEAN: i32 = 12;
    pub const DEEP_LUKEWARM_OCEAN: i32 = 13;
    pub const DEEP_OCEAN: i32 = 14;
    pub const DESERT: i32 = 15;
    pub const DRIPSTONE_CAVES: i32 = 16;
    pub const END_BARRENS: i32 = 17;
    pub const END_HIGHLANDS: i32 = 18;
    pub const END_MIDLANDS: i32 = 19;
    pub const ERODED_BADLANDS: i32 = 20;
    pub const FLOWER_FOREST: i32 = 21;
    pub const FOREST: i32 = 22;
    pub const FROZEN_OCEAN: i32 = 23;
    pub const FROZEN_PEAKS: i32 = 24;
    pub const FROZEN_RIVER: i32 = 25;
    pub const GROVE: i32 = 26;
    pub const ICE_SPIKES: i32 = 27;
    pub const JAGGED_PEAKS: i32 = 28;
    pub const JUNGLE: i32 = 29;
    pub const LUKEWARM_OCEAN: i32 = 30;
    pub const LUSH_CAVES: i32 = 31;
    pub const MANGROVE_SWAMP: i32 = 32;
    pub const MEADOW: i32 = 33;
    pub const MUSHROOM_FIELDS: i32 = 34;
    pub const NETHER_WASTES: i32 = 35;
    pub const OCEAN: i32 = 36;
    pub const OLD_GROWTH_BIRCH_FOREST: i32 = 37;
    pub const OLD_GROWTH_PINE_TAIGA: i32 = 38;
    pub const OLD_GROWTH_SPRUCE_TAIGA: i32 = 39;
    pub const PALE_GARDEN: i32 = 40;
    pub const PLAINS: i32 = 41;
    pub const RIVER: i32 = 42;
    pub const SAVANNA: i32 = 43;
    pub const SAVANNA_PLATEAU: i32 = 44;
    pub const SMALL_END_ISLANDS: i32 = 45;
    pub const SNOWY_BEACH: i32 = 46;
    pub const SNOWY_PLAINS: i32 = 47;
    pub const SNOWY_SLOPES: i32 = 48;
    pub const SNOWY_TAIGA: i32 = 49;
    pub const SOUL_SAND_VALLEY: i32 = 50;
    pub const SPARSE_JUNGLE: i32 = 51;
    pub const STONY_PEAKS: i32 = 52;
    pub const STONY_SHORE: i32 = 53;
    pub const SULFUR_CAVES: i32 = 54;
    pub const SUNFLOWER_PLAINS: i32 = 55;
    pub const SWAMP: i32 = 56;
    pub const TAIGA: i32 = 57;
    pub const THE_END: i32 = 58;
    pub const THE_VOID: i32 = 59;
    pub const WARM_OCEAN: i32 = 60;
    pub const WARPED_FOREST: i32 = 61;
    pub const WINDSWEPT_FOREST: i32 = 62;
    pub const WINDSWEPT_GRAVELLY_HILLS: i32 = 63;
    pub const WINDSWEPT_HILLS: i32 = 64;
    pub const WINDSWEPT_SAVANNA: i32 = 65;
    pub const WOODED_BADLANDS: i32 = 66;
}

pub mod chat_type {
    pub const TOTAL: u32 = 7;
    /// 条目名，下标就是注册表 ID。
    pub static NAMES: &[&str] = &["minecraft:chat", "minecraft:emote_command", "minecraft:msg_command_incoming", "minecraft:msg_command_outgoing", "minecraft:say_command", "minecraft:team_msg_command_incoming", "minecraft:team_msg_command_outgoing"];
    pub const CHAT: i32 = 0;
    pub const EMOTE_COMMAND: i32 = 1;
    pub const MSG_COMMAND_INCOMING: i32 = 2;
    pub const MSG_COMMAND_OUTGOING: i32 = 3;
    pub const SAY_COMMAND: i32 = 4;
    pub const TEAM_MSG_COMMAND_INCOMING: i32 = 5;
    pub const TEAM_MSG_COMMAND_OUTGOING: i32 = 6;
}

pub mod dimension_type {
    pub const TOTAL: u32 = 4;
    /// 条目名，下标就是注册表 ID。
    pub static NAMES: &[&str] = &["minecraft:overworld", "minecraft:overworld_caves", "minecraft:the_end", "minecraft:the_nether"];
    pub const OVERWORLD: i32 = 0;
    pub const OVERWORLD_CAVES: i32 = 1;
    pub const THE_END: i32 = 2;
    pub const THE_NETHER: i32 = 3;
}

pub mod world_clock {
    pub const TOTAL: u32 = 2;
    /// 条目名，下标就是注册表 ID。
    pub static NAMES: &[&str] = &["minecraft:overworld", "minecraft:the_end"];
    pub const OVERWORLD: i32 = 0;
    pub const THE_END: i32 = 1;
}

pub mod particle_type {
    pub const TOTAL: u32 = 128;
    /// 条目名，下标就是注册表 ID。
    pub static NAMES: &[&str] = &["minecraft:angry_villager", "minecraft:block", "minecraft:block_marker", "minecraft:bubble", "minecraft:sulfur_bubbles", "minecraft:noxious_gas", "minecraft:noxious_gas_cloud", "minecraft:geyser", "minecraft:geyser_base", "minecraft:geyser_poof", "minecraft:geyser_plume", "minecraft:cloud", "minecraft:copper_fire_flame", "minecraft:crit", "minecraft:damage_indicator", "minecraft:dragon_breath", "minecraft:dripping_lava", "minecraft:falling_lava", "minecraft:landing_lava", "minecraft:dripping_water", "minecraft:falling_water", "minecraft:dust", "minecraft:dust_color_transition", "minecraft:effect", "minecraft:elder_guardian", "minecraft:enchanted_hit", "minecraft:enchant", "minecraft:end_rod", "minecraft:entity_effect", "minecraft:explosion_emitter", "minecraft:explosion", "minecraft:gust", "minecraft:small_gust", "minecraft:gust_emitter_large", "minecraft:gust_emitter_small", "minecraft:sonic_boom", "minecraft:falling_dust", "minecraft:firework", "minecraft:fishing", "minecraft:flame", "minecraft:infested", "minecraft:cherry_leaves", "minecraft:pale_oak_leaves", "minecraft:red_poplar_leaves", "minecraft:orange_poplar_leaves", "minecraft:yellow_poplar_leaves", "minecraft:tinted_leaves", "minecraft:sculk_soul", "minecraft:sculk_charge", "minecraft:sculk_charge_pop", "minecraft:soul_fire_flame", "minecraft:soul", "minecraft:flash", "minecraft:happy_villager", "minecraft:composter", "minecraft:heart", "minecraft:instant_effect", "minecraft:item", "minecraft:vibration", "minecraft:trail", "minecraft:pause_mob_growth", "minecraft:reset_mob_growth", "minecraft:item_slime", "minecraft:item_cobweb", "minecraft:item_snowball", "minecraft:large_smoke", "minecraft:lava", "minecraft:mycelium", "minecraft:note", "minecraft:poof", "minecraft:portal", "minecraft:rain", "minecraft:smoke", "minecraft:white_smoke", "minecraft:sneeze", "minecraft:spit", "minecraft:squid_ink", "minecraft:sweep_attack", "minecraft:totem_of_undying", "minecraft:underwater", "minecraft:splash", "minecraft:witch", "minecraft:bubble_pop", "minecraft:current_down", "minecraft:bubble_column_up", "minecraft:nautilus", "minecraft:dolphin", "minecraft:campfire_cosy_smoke", "minecraft:campfire_signal_smoke", "minecraft:dripping_honey", "minecraft:falling_honey", "minecraft:landing_honey", "minecraft:falling_nectar", "minecraft:falling_spore_blossom", "minecraft:ash", "minecraft:crimson_spore", "minecraft:warped_spore", "minecraft:spore_blossom_air", "minecraft:dripping_obsidian_tear", "minecraft:falling_obsidian_tear", "minecraft:landing_obsidian_tear", "minecraft:reverse_portal", "minecraft:white_ash", "minecraft:small_flame", "minecraft:snowflake", "minecraft:dripping_dripstone_lava", "minecraft:falling_dripstone_lava", "minecraft:dripping_dripstone_water", "minecraft:falling_dripstone_water", "minecraft:glow_squid_ink", "minecraft:glow", "minecraft:wax_on", "minecraft:wax_off", "minecraft:electric_spark", "minecraft:scrape", "minecraft:shriek", "minecraft:egg_crack", "minecraft:dust_plume", "minecraft:trial_spawner_detection", "minecraft:trial_spawner_detection_ominous", "minecraft:vault_connection", "minecraft:dust_pillar", "minecraft:ominous_spawning", "minecraft:raid_omen", "minecraft:trial_omen", "minecraft:block_crumble", "minecraft:firefly", "minecraft:sulfur_cube_goo"];
    pub const ANGRY_VILLAGER: i32 = 0;
    pub const BLOCK: i32 = 1;
    pub const BLOCK_MARKER: i32 = 2;
    pub const BUBBLE: i32 = 3;
    pub const SULFUR_BUBBLES: i32 = 4;
    pub const NOXIOUS_GAS: i32 = 5;
    pub const NOXIOUS_GAS_CLOUD: i32 = 6;
    pub const GEYSER: i32 = 7;
    pub const GEYSER_BASE: i32 = 8;
    pub const GEYSER_POOF: i32 = 9;
    pub const GEYSER_PLUME: i32 = 10;
    pub const CLOUD: i32 = 11;
    pub const COPPER_FIRE_FLAME: i32 = 12;
    pub const CRIT: i32 = 13;
    pub const DAMAGE_INDICATOR: i32 = 14;
    pub const DRAGON_BREATH: i32 = 15;
    pub const DRIPPING_LAVA: i32 = 16;
    pub const FALLING_LAVA: i32 = 17;
    pub const LANDING_LAVA: i32 = 18;
    pub const DRIPPING_WATER: i32 = 19;
    pub const FALLING_WATER: i32 = 20;
    pub const DUST: i32 = 21;
    pub const DUST_COLOR_TRANSITION: i32 = 22;
    pub const EFFECT: i32 = 23;
    pub const ELDER_GUARDIAN: i32 = 24;
    pub const ENCHANTED_HIT: i32 = 25;
    pub const ENCHANT: i32 = 26;
    pub const END_ROD: i32 = 27;
    pub const ENTITY_EFFECT: i32 = 28;
    pub const EXPLOSION_EMITTER: i32 = 29;
    pub const EXPLOSION: i32 = 30;
    pub const GUST: i32 = 31;
    pub const SMALL_GUST: i32 = 32;
    pub const GUST_EMITTER_LARGE: i32 = 33;
    pub const GUST_EMITTER_SMALL: i32 = 34;
    pub const SONIC_BOOM: i32 = 35;
    pub const FALLING_DUST: i32 = 36;
    pub const FIREWORK: i32 = 37;
    pub const FISHING: i32 = 38;
    pub const FLAME: i32 = 39;
    pub const INFESTED: i32 = 40;
    pub const CHERRY_LEAVES: i32 = 41;
    pub const PALE_OAK_LEAVES: i32 = 42;
    pub const RED_POPLAR_LEAVES: i32 = 43;
    pub const ORANGE_POPLAR_LEAVES: i32 = 44;
    pub const YELLOW_POPLAR_LEAVES: i32 = 45;
    pub const TINTED_LEAVES: i32 = 46;
    pub const SCULK_SOUL: i32 = 47;
    pub const SCULK_CHARGE: i32 = 48;
    pub const SCULK_CHARGE_POP: i32 = 49;
    pub const SOUL_FIRE_FLAME: i32 = 50;
    pub const SOUL: i32 = 51;
    pub const FLASH: i32 = 52;
    pub const HAPPY_VILLAGER: i32 = 53;
    pub const COMPOSTER: i32 = 54;
    pub const HEART: i32 = 55;
    pub const INSTANT_EFFECT: i32 = 56;
    pub const ITEM: i32 = 57;
    pub const VIBRATION: i32 = 58;
    pub const TRAIL: i32 = 59;
    pub const PAUSE_MOB_GROWTH: i32 = 60;
    pub const RESET_MOB_GROWTH: i32 = 61;
    pub const ITEM_SLIME: i32 = 62;
    pub const ITEM_COBWEB: i32 = 63;
    pub const ITEM_SNOWBALL: i32 = 64;
    pub const LARGE_SMOKE: i32 = 65;
    pub const LAVA: i32 = 66;
    pub const MYCELIUM: i32 = 67;
    pub const NOTE: i32 = 68;
    pub const POOF: i32 = 69;
    pub const PORTAL: i32 = 70;
    pub const RAIN: i32 = 71;
    pub const SMOKE: i32 = 72;
    pub const WHITE_SMOKE: i32 = 73;
    pub const SNEEZE: i32 = 74;
    pub const SPIT: i32 = 75;
    pub const SQUID_INK: i32 = 76;
    pub const SWEEP_ATTACK: i32 = 77;
    pub const TOTEM_OF_UNDYING: i32 = 78;
    pub const UNDERWATER: i32 = 79;
    pub const SPLASH: i32 = 80;
    pub const WITCH: i32 = 81;
    pub const BUBBLE_POP: i32 = 82;
    pub const CURRENT_DOWN: i32 = 83;
    pub const BUBBLE_COLUMN_UP: i32 = 84;
    pub const NAUTILUS: i32 = 85;
    pub const DOLPHIN: i32 = 86;
    pub const CAMPFIRE_COSY_SMOKE: i32 = 87;
    pub const CAMPFIRE_SIGNAL_SMOKE: i32 = 88;
    pub const DRIPPING_HONEY: i32 = 89;
    pub const FALLING_HONEY: i32 = 90;
    pub const LANDING_HONEY: i32 = 91;
    pub const FALLING_NECTAR: i32 = 92;
    pub const FALLING_SPORE_BLOSSOM: i32 = 93;
    pub const ASH: i32 = 94;
    pub const CRIMSON_SPORE: i32 = 95;
    pub const WARPED_SPORE: i32 = 96;
    pub const SPORE_BLOSSOM_AIR: i32 = 97;
    pub const DRIPPING_OBSIDIAN_TEAR: i32 = 98;
    pub const FALLING_OBSIDIAN_TEAR: i32 = 99;
    pub const LANDING_OBSIDIAN_TEAR: i32 = 100;
    pub const REVERSE_PORTAL: i32 = 101;
    pub const WHITE_ASH: i32 = 102;
    pub const SMALL_FLAME: i32 = 103;
    pub const SNOWFLAKE: i32 = 104;
    pub const DRIPPING_DRIPSTONE_LAVA: i32 = 105;
    pub const FALLING_DRIPSTONE_LAVA: i32 = 106;
    pub const DRIPPING_DRIPSTONE_WATER: i32 = 107;
    pub const FALLING_DRIPSTONE_WATER: i32 = 108;
    pub const GLOW_SQUID_INK: i32 = 109;
    pub const GLOW: i32 = 110;
    pub const WAX_ON: i32 = 111;
    pub const WAX_OFF: i32 = 112;
    pub const ELECTRIC_SPARK: i32 = 113;
    pub const SCRAPE: i32 = 114;
    pub const SHRIEK: i32 = 115;
    pub const EGG_CRACK: i32 = 116;
    pub const DUST_PLUME: i32 = 117;
    pub const TRIAL_SPAWNER_DETECTION: i32 = 118;
    pub const TRIAL_SPAWNER_DETECTION_OMINOUS: i32 = 119;
    pub const VAULT_CONNECTION: i32 = 120;
    pub const DUST_PILLAR: i32 = 121;
    pub const OMINOUS_SPAWNING: i32 = 122;
    pub const RAID_OMEN: i32 = 123;
    pub const TRIAL_OMEN: i32 = 124;
    pub const BLOCK_CRUMBLE: i32 = 125;
    pub const FIREFLY: i32 = 126;
    pub const SULFUR_CUBE_GOO: i32 = 127;
}

pub mod attribute {
    pub const TOTAL: u32 = 40;
    /// 条目名，下标就是注册表 ID。
    pub static NAMES: &[&str] = &["minecraft:air_drag_modifier", "minecraft:armor", "minecraft:armor_toughness", "minecraft:attack_damage", "minecraft:attack_knockback", "minecraft:attack_speed", "minecraft:below_name_distance", "minecraft:block_break_speed", "minecraft:block_interaction_range", "minecraft:bounciness", "minecraft:burning_time", "minecraft:camera_distance", "minecraft:explosion_knockback_resistance", "minecraft:entity_interaction_range", "minecraft:fall_damage_multiplier", "minecraft:flying_speed", "minecraft:follow_range", "minecraft:friction_modifier", "minecraft:gravity", "minecraft:jump_strength", "minecraft:knockback_resistance", "minecraft:luck", "minecraft:max_absorption", "minecraft:max_health", "minecraft:mining_efficiency", "minecraft:movement_efficiency", "minecraft:movement_speed", "minecraft:name_tag_distance", "minecraft:oxygen_bonus", "minecraft:safe_fall_distance", "minecraft:scale", "minecraft:sneaking_speed", "minecraft:spawn_reinforcements", "minecraft:step_height", "minecraft:submerged_mining_speed", "minecraft:sweeping_damage_ratio", "minecraft:tempt_range", "minecraft:water_movement_efficiency", "minecraft:waypoint_transmit_range", "minecraft:waypoint_receive_range"];
    pub const AIR_DRAG_MODIFIER: i32 = 0;
    pub const ARMOR: i32 = 1;
    pub const ARMOR_TOUGHNESS: i32 = 2;
    pub const ATTACK_DAMAGE: i32 = 3;
    pub const ATTACK_KNOCKBACK: i32 = 4;
    pub const ATTACK_SPEED: i32 = 5;
    pub const BELOW_NAME_DISTANCE: i32 = 6;
    pub const BLOCK_BREAK_SPEED: i32 = 7;
    pub const BLOCK_INTERACTION_RANGE: i32 = 8;
    pub const BOUNCINESS: i32 = 9;
    pub const BURNING_TIME: i32 = 10;
    pub const CAMERA_DISTANCE: i32 = 11;
    pub const EXPLOSION_KNOCKBACK_RESISTANCE: i32 = 12;
    pub const ENTITY_INTERACTION_RANGE: i32 = 13;
    pub const FALL_DAMAGE_MULTIPLIER: i32 = 14;
    pub const FLYING_SPEED: i32 = 15;
    pub const FOLLOW_RANGE: i32 = 16;
    pub const FRICTION_MODIFIER: i32 = 17;
    pub const GRAVITY: i32 = 18;
    pub const JUMP_STRENGTH: i32 = 19;
    pub const KNOCKBACK_RESISTANCE: i32 = 20;
    pub const LUCK: i32 = 21;
    pub const MAX_ABSORPTION: i32 = 22;
    pub const MAX_HEALTH: i32 = 23;
    pub const MINING_EFFICIENCY: i32 = 24;
    pub const MOVEMENT_EFFICIENCY: i32 = 25;
    pub const MOVEMENT_SPEED: i32 = 26;
    pub const NAME_TAG_DISTANCE: i32 = 27;
    pub const OXYGEN_BONUS: i32 = 28;
    pub const SAFE_FALL_DISTANCE: i32 = 29;
    pub const SCALE: i32 = 30;
    pub const SNEAKING_SPEED: i32 = 31;
    pub const SPAWN_REINFORCEMENTS: i32 = 32;
    pub const STEP_HEIGHT: i32 = 33;
    pub const SUBMERGED_MINING_SPEED: i32 = 34;
    pub const SWEEPING_DAMAGE_RATIO: i32 = 35;
    pub const TEMPT_RANGE: i32 = 36;
    pub const WATER_MOVEMENT_EFFICIENCY: i32 = 37;
    pub const WAYPOINT_TRANSMIT_RANGE: i32 = 38;
    pub const WAYPOINT_RECEIVE_RANGE: i32 = 39;
}

pub mod block_entity_type {
    pub const TOTAL: u32 = 49;
    /// 条目名，下标就是注册表 ID。
    pub static NAMES: &[&str] = &["minecraft:furnace", "minecraft:chest", "minecraft:trapped_chest", "minecraft:ender_chest", "minecraft:jukebox", "minecraft:dispenser", "minecraft:dropper", "minecraft:sign", "minecraft:hanging_sign", "minecraft:mob_spawner", "minecraft:creaking_heart", "minecraft:piston", "minecraft:brewing_stand", "minecraft:enchanting_table", "minecraft:end_portal", "minecraft:beacon", "minecraft:skull", "minecraft:daylight_detector", "minecraft:hopper", "minecraft:comparator", "minecraft:banner", "minecraft:structure_block", "minecraft:end_gateway", "minecraft:command_block", "minecraft:shulker_box", "minecraft:conduit", "minecraft:barrel", "minecraft:smoker", "minecraft:blast_furnace", "minecraft:lectern", "minecraft:bell", "minecraft:jigsaw", "minecraft:campfire", "minecraft:beehive", "minecraft:sculk_sensor", "minecraft:calibrated_sculk_sensor", "minecraft:sculk_catalyst", "minecraft:sculk_shrieker", "minecraft:chiseled_bookshelf", "minecraft:shelf", "minecraft:brushable_block", "minecraft:decorated_pot", "minecraft:crafter", "minecraft:trial_spawner", "minecraft:vault", "minecraft:test_block", "minecraft:test_instance_block", "minecraft:copper_golem_statue", "minecraft:potent_sulfur"];
    pub const FURNACE: i32 = 0;
    pub const CHEST: i32 = 1;
    pub const TRAPPED_CHEST: i32 = 2;
    pub const ENDER_CHEST: i32 = 3;
    pub const JUKEBOX: i32 = 4;
    pub const DISPENSER: i32 = 5;
    pub const DROPPER: i32 = 6;
    pub const SIGN: i32 = 7;
    pub const HANGING_SIGN: i32 = 8;
    pub const MOB_SPAWNER: i32 = 9;
    pub const CREAKING_HEART: i32 = 10;
    pub const PISTON: i32 = 11;
    pub const BREWING_STAND: i32 = 12;
    pub const ENCHANTING_TABLE: i32 = 13;
    pub const END_PORTAL: i32 = 14;
    pub const BEACON: i32 = 15;
    pub const SKULL: i32 = 16;
    pub const DAYLIGHT_DETECTOR: i32 = 17;
    pub const HOPPER: i32 = 18;
    pub const COMPARATOR: i32 = 19;
    pub const BANNER: i32 = 20;
    pub const STRUCTURE_BLOCK: i32 = 21;
    pub const END_GATEWAY: i32 = 22;
    pub const COMMAND_BLOCK: i32 = 23;
    pub const SHULKER_BOX: i32 = 24;
    pub const CONDUIT: i32 = 25;
    pub const BARREL: i32 = 26;
    pub const SMOKER: i32 = 27;
    pub const BLAST_FURNACE: i32 = 28;
    pub const LECTERN: i32 = 29;
    pub const BELL: i32 = 30;
    pub const JIGSAW: i32 = 31;
    pub const CAMPFIRE: i32 = 32;
    pub const BEEHIVE: i32 = 33;
    pub const SCULK_SENSOR: i32 = 34;
    pub const CALIBRATED_SCULK_SENSOR: i32 = 35;
    pub const SCULK_CATALYST: i32 = 36;
    pub const SCULK_SHRIEKER: i32 = 37;
    pub const CHISELED_BOOKSHELF: i32 = 38;
    pub const SHELF: i32 = 39;
    pub const BRUSHABLE_BLOCK: i32 = 40;
    pub const DECORATED_POT: i32 = 41;
    pub const CRAFTER: i32 = 42;
    pub const TRIAL_SPAWNER: i32 = 43;
    pub const VAULT: i32 = 44;
    pub const TEST_BLOCK: i32 = 45;
    pub const TEST_INSTANCE_BLOCK: i32 = 46;
    pub const COPPER_GOLEM_STATUE: i32 = 47;
    pub const POTENT_SULFUR: i32 = 48;
}

pub mod entity_type {
    pub const TOTAL: u32 = 161;
    /// 条目名，下标就是注册表 ID。
    pub static NAMES: &[&str] = &["minecraft:acacia_boat", "minecraft:acacia_chest_boat", "minecraft:allay", "minecraft:area_effect_cloud", "minecraft:armadillo", "minecraft:armor_stand", "minecraft:arrow", "minecraft:axolotl", "minecraft:bamboo_chest_raft", "minecraft:bamboo_raft", "minecraft:bat", "minecraft:bee", "minecraft:birch_boat", "minecraft:birch_chest_boat", "minecraft:blaze", "minecraft:block_display", "minecraft:bogged", "minecraft:breeze", "minecraft:breeze_wind_charge", "minecraft:camel", "minecraft:camel_husk", "minecraft:cat", "minecraft:cave_spider", "minecraft:cherry_boat", "minecraft:cherry_chest_boat", "minecraft:chest_minecart", "minecraft:chicken", "minecraft:cod", "minecraft:copper_golem", "minecraft:command_block_minecart", "minecraft:cow", "minecraft:creaking", "minecraft:creeper", "minecraft:cushion", "minecraft:dark_oak_boat", "minecraft:dark_oak_chest_boat", "minecraft:dolphin", "minecraft:donkey", "minecraft:dragon_fireball", "minecraft:drowned", "minecraft:egg", "minecraft:elder_guardian", "minecraft:enderman", "minecraft:endermite", "minecraft:ender_dragon", "minecraft:ender_pearl", "minecraft:end_crystal", "minecraft:evoker", "minecraft:evoker_fangs", "minecraft:experience_bottle", "minecraft:experience_orb", "minecraft:eye_of_ender", "minecraft:falling_block", "minecraft:fireball", "minecraft:firework_rocket", "minecraft:fox", "minecraft:frog", "minecraft:furnace_minecart", "minecraft:ghast", "minecraft:happy_ghast", "minecraft:giant", "minecraft:glow_item_frame", "minecraft:glow_squid", "minecraft:goat", "minecraft:guardian", "minecraft:hoglin", "minecraft:hopper_minecart", "minecraft:horse", "minecraft:husk", "minecraft:illusioner", "minecraft:interaction", "minecraft:iron_golem", "minecraft:item", "minecraft:item_display", "minecraft:item_frame", "minecraft:jungle_boat", "minecraft:jungle_chest_boat", "minecraft:leash_knot", "minecraft:lightning_bolt", "minecraft:llama", "minecraft:llama_spit", "minecraft:magma_cube", "minecraft:mangrove_boat", "minecraft:mangrove_chest_boat", "minecraft:mannequin", "minecraft:marker", "minecraft:minecart", "minecraft:mooshroom", "minecraft:mule", "minecraft:nautilus", "minecraft:oak_boat", "minecraft:oak_chest_boat", "minecraft:ocelot", "minecraft:ominous_item_spawner", "minecraft:painting", "minecraft:pale_oak_boat", "minecraft:pale_oak_chest_boat", "minecraft:panda", "minecraft:parched", "minecraft:parrot", "minecraft:phantom", "minecraft:pig", "minecraft:piglin", "minecraft:piglin_brute", "minecraft:pillager", "minecraft:polar_bear", "minecraft:poplar_boat", "minecraft:poplar_chest_boat", "minecraft:splash_potion", "minecraft:lingering_potion", "minecraft:pufferfish", "minecraft:rabbit", "minecraft:ravager", "minecraft:salmon", "minecraft:sheep", "minecraft:shulker", "minecraft:shulker_bullet", "minecraft:silverfish", "minecraft:skeleton", "minecraft:skeleton_horse", "minecraft:slime", "minecraft:small_fireball", "minecraft:sniffer", "minecraft:snowball", "minecraft:snow_golem", "minecraft:spawner_minecart", "minecraft:spectral_arrow", "minecraft:spider", "minecraft:spruce_boat", "minecraft:spruce_chest_boat", "minecraft:squid", "minecraft:stray", "minecraft:strider", "minecraft:sulfur_cube", "minecraft:tadpole", "minecraft:text_display", "minecraft:tnt", "minecraft:tnt_minecart", "minecraft:trader_llama", "minecraft:trident", "minecraft:tropical_fish", "minecraft:turtle", "minecraft:vex", "minecraft:villager", "minecraft:vindicator", "minecraft:wandering_trader", "minecraft:warden", "minecraft:wind_charge", "minecraft:witch", "minecraft:wither", "minecraft:wither_skeleton", "minecraft:wither_skull", "minecraft:wolf", "minecraft:zoglin", "minecraft:zombie", "minecraft:zombie_horse", "minecraft:zombie_nautilus", "minecraft:zombie_villager", "minecraft:zombified_piglin", "minecraft:player", "minecraft:fishing_bobber"];
    pub const ACACIA_BOAT: i32 = 0;
    pub const ACACIA_CHEST_BOAT: i32 = 1;
    pub const ALLAY: i32 = 2;
    pub const AREA_EFFECT_CLOUD: i32 = 3;
    pub const ARMADILLO: i32 = 4;
    pub const ARMOR_STAND: i32 = 5;
    pub const ARROW: i32 = 6;
    pub const AXOLOTL: i32 = 7;
    pub const BAMBOO_CHEST_RAFT: i32 = 8;
    pub const BAMBOO_RAFT: i32 = 9;
    pub const BAT: i32 = 10;
    pub const BEE: i32 = 11;
    pub const BIRCH_BOAT: i32 = 12;
    pub const BIRCH_CHEST_BOAT: i32 = 13;
    pub const BLAZE: i32 = 14;
    pub const BLOCK_DISPLAY: i32 = 15;
    pub const BOGGED: i32 = 16;
    pub const BREEZE: i32 = 17;
    pub const BREEZE_WIND_CHARGE: i32 = 18;
    pub const CAMEL: i32 = 19;
    pub const CAMEL_HUSK: i32 = 20;
    pub const CAT: i32 = 21;
    pub const CAVE_SPIDER: i32 = 22;
    pub const CHERRY_BOAT: i32 = 23;
    pub const CHERRY_CHEST_BOAT: i32 = 24;
    pub const CHEST_MINECART: i32 = 25;
    pub const CHICKEN: i32 = 26;
    pub const COD: i32 = 27;
    pub const COPPER_GOLEM: i32 = 28;
    pub const COMMAND_BLOCK_MINECART: i32 = 29;
    pub const COW: i32 = 30;
    pub const CREAKING: i32 = 31;
    pub const CREEPER: i32 = 32;
    pub const CUSHION: i32 = 33;
    pub const DARK_OAK_BOAT: i32 = 34;
    pub const DARK_OAK_CHEST_BOAT: i32 = 35;
    pub const DOLPHIN: i32 = 36;
    pub const DONKEY: i32 = 37;
    pub const DRAGON_FIREBALL: i32 = 38;
    pub const DROWNED: i32 = 39;
    pub const EGG: i32 = 40;
    pub const ELDER_GUARDIAN: i32 = 41;
    pub const ENDERMAN: i32 = 42;
    pub const ENDERMITE: i32 = 43;
    pub const ENDER_DRAGON: i32 = 44;
    pub const ENDER_PEARL: i32 = 45;
    pub const END_CRYSTAL: i32 = 46;
    pub const EVOKER: i32 = 47;
    pub const EVOKER_FANGS: i32 = 48;
    pub const EXPERIENCE_BOTTLE: i32 = 49;
    pub const EXPERIENCE_ORB: i32 = 50;
    pub const EYE_OF_ENDER: i32 = 51;
    pub const FALLING_BLOCK: i32 = 52;
    pub const FIREBALL: i32 = 53;
    pub const FIREWORK_ROCKET: i32 = 54;
    pub const FOX: i32 = 55;
    pub const FROG: i32 = 56;
    pub const FURNACE_MINECART: i32 = 57;
    pub const GHAST: i32 = 58;
    pub const HAPPY_GHAST: i32 = 59;
    pub const GIANT: i32 = 60;
    pub const GLOW_ITEM_FRAME: i32 = 61;
    pub const GLOW_SQUID: i32 = 62;
    pub const GOAT: i32 = 63;
    pub const GUARDIAN: i32 = 64;
    pub const HOGLIN: i32 = 65;
    pub const HOPPER_MINECART: i32 = 66;
    pub const HORSE: i32 = 67;
    pub const HUSK: i32 = 68;
    pub const ILLUSIONER: i32 = 69;
    pub const INTERACTION: i32 = 70;
    pub const IRON_GOLEM: i32 = 71;
    pub const ITEM: i32 = 72;
    pub const ITEM_DISPLAY: i32 = 73;
    pub const ITEM_FRAME: i32 = 74;
    pub const JUNGLE_BOAT: i32 = 75;
    pub const JUNGLE_CHEST_BOAT: i32 = 76;
    pub const LEASH_KNOT: i32 = 77;
    pub const LIGHTNING_BOLT: i32 = 78;
    pub const LLAMA: i32 = 79;
    pub const LLAMA_SPIT: i32 = 80;
    pub const MAGMA_CUBE: i32 = 81;
    pub const MANGROVE_BOAT: i32 = 82;
    pub const MANGROVE_CHEST_BOAT: i32 = 83;
    pub const MANNEQUIN: i32 = 84;
    pub const MARKER: i32 = 85;
    pub const MINECART: i32 = 86;
    pub const MOOSHROOM: i32 = 87;
    pub const MULE: i32 = 88;
    pub const NAUTILUS: i32 = 89;
    pub const OAK_BOAT: i32 = 90;
    pub const OAK_CHEST_BOAT: i32 = 91;
    pub const OCELOT: i32 = 92;
    pub const OMINOUS_ITEM_SPAWNER: i32 = 93;
    pub const PAINTING: i32 = 94;
    pub const PALE_OAK_BOAT: i32 = 95;
    pub const PALE_OAK_CHEST_BOAT: i32 = 96;
    pub const PANDA: i32 = 97;
    pub const PARCHED: i32 = 98;
    pub const PARROT: i32 = 99;
    pub const PHANTOM: i32 = 100;
    pub const PIG: i32 = 101;
    pub const PIGLIN: i32 = 102;
    pub const PIGLIN_BRUTE: i32 = 103;
    pub const PILLAGER: i32 = 104;
    pub const POLAR_BEAR: i32 = 105;
    pub const POPLAR_BOAT: i32 = 106;
    pub const POPLAR_CHEST_BOAT: i32 = 107;
    pub const SPLASH_POTION: i32 = 108;
    pub const LINGERING_POTION: i32 = 109;
    pub const PUFFERFISH: i32 = 110;
    pub const RABBIT: i32 = 111;
    pub const RAVAGER: i32 = 112;
    pub const SALMON: i32 = 113;
    pub const SHEEP: i32 = 114;
    pub const SHULKER: i32 = 115;
    pub const SHULKER_BULLET: i32 = 116;
    pub const SILVERFISH: i32 = 117;
    pub const SKELETON: i32 = 118;
    pub const SKELETON_HORSE: i32 = 119;
    pub const SLIME: i32 = 120;
    pub const SMALL_FIREBALL: i32 = 121;
    pub const SNIFFER: i32 = 122;
    pub const SNOWBALL: i32 = 123;
    pub const SNOW_GOLEM: i32 = 124;
    pub const SPAWNER_MINECART: i32 = 125;
    pub const SPECTRAL_ARROW: i32 = 126;
    pub const SPIDER: i32 = 127;
    pub const SPRUCE_BOAT: i32 = 128;
    pub const SPRUCE_CHEST_BOAT: i32 = 129;
    pub const SQUID: i32 = 130;
    pub const STRAY: i32 = 131;
    pub const STRIDER: i32 = 132;
    pub const SULFUR_CUBE: i32 = 133;
    pub const TADPOLE: i32 = 134;
    pub const TEXT_DISPLAY: i32 = 135;
    pub const TNT: i32 = 136;
    pub const TNT_MINECART: i32 = 137;
    pub const TRADER_LLAMA: i32 = 138;
    pub const TRIDENT: i32 = 139;
    pub const TROPICAL_FISH: i32 = 140;
    pub const TURTLE: i32 = 141;
    pub const VEX: i32 = 142;
    pub const VILLAGER: i32 = 143;
    pub const VINDICATOR: i32 = 144;
    pub const WANDERING_TRADER: i32 = 145;
    pub const WARDEN: i32 = 146;
    pub const WIND_CHARGE: i32 = 147;
    pub const WITCH: i32 = 148;
    pub const WITHER: i32 = 149;
    pub const WITHER_SKELETON: i32 = 150;
    pub const WITHER_SKULL: i32 = 151;
    pub const WOLF: i32 = 152;
    pub const ZOGLIN: i32 = 153;
    pub const ZOMBIE: i32 = 154;
    pub const ZOMBIE_HORSE: i32 = 155;
    pub const ZOMBIE_NAUTILUS: i32 = 156;
    pub const ZOMBIE_VILLAGER: i32 = 157;
    pub const ZOMBIFIED_PIGLIN: i32 = 158;
    pub const PLAYER: i32 = 159;
    pub const FISHING_BOBBER: i32 = 160;
}

pub mod data_component_type {
    pub const TOTAL: u32 = 122;
    /// 条目名，下标就是注册表 ID。
    pub static NAMES: &[&str] = &["minecraft:custom_data", "minecraft:max_stack_size", "minecraft:max_damage", "minecraft:damage", "minecraft:unbreakable", "minecraft:use_effects", "minecraft:custom_name", "minecraft:minimum_attack_charge", "minecraft:damage_type", "minecraft:item_name", "minecraft:item_model", "minecraft:lore", "minecraft:rarity", "minecraft:enchantments", "minecraft:can_place_on", "minecraft:can_break", "minecraft:attribute_modifiers", "minecraft:custom_model_data", "minecraft:tooltip_display", "minecraft:repair_cost", "minecraft:creative_slot_lock", "minecraft:enchantment_glint_override", "minecraft:intangible_projectile", "minecraft:food", "minecraft:consumable", "minecraft:use_remainder", "minecraft:use_cooldown", "minecraft:damage_resistant", "minecraft:tool", "minecraft:weapon", "minecraft:attack_range", "minecraft:enchantable", "minecraft:equippable", "minecraft:repairable", "minecraft:glider", "minecraft:tooltip_style", "minecraft:death_protection", "minecraft:blocks_attacks", "minecraft:piercing_weapon", "minecraft:kinetic_weapon", "minecraft:attack_animation", "minecraft:interact_animation", "minecraft:additional_trade_cost", "minecraft:block_transformer", "minecraft:villager_food", "minecraft:stored_enchantments", "minecraft:dye", "minecraft:dyed_color", "minecraft:map_id", "minecraft:map_decorations", "minecraft:map_post_processing", "minecraft:charged_projectiles", "minecraft:bundle_contents", "minecraft:potion_contents", "minecraft:potion_duration_scale", "minecraft:suspicious_stew_effects", "minecraft:writable_book_content", "minecraft:written_book_content", "minecraft:trim", "minecraft:debug_stick_state", "minecraft:entity_data", "minecraft:bucket_entity_data", "minecraft:block_entity_data", "minecraft:instrument", "minecraft:provides_trim_material", "minecraft:ominous_bottle_amplifier", "minecraft:jukebox_playable", "minecraft:provides_banner_patterns", "minecraft:recipes", "minecraft:lodestone_tracker", "minecraft:firework_explosion", "minecraft:fireworks", "minecraft:profile", "minecraft:note_block_sound", "minecraft:banner_patterns", "minecraft:base_color", "minecraft:pot_decorations", "minecraft:container", "minecraft:block_state", "minecraft:bees", "minecraft:sulfur_cube_content", "minecraft:lock", "minecraft:container_loot", "minecraft:break_sound", "minecraft:compostable", "minecraft:cooking_fuel", "minecraft:brewing_fuel", "minecraft:mob_visibility", "minecraft:villager/variant", "minecraft:wolf/variant", "minecraft:wolf/sound_variant", "minecraft:wolf/collar", "minecraft:fox/variant", "minecraft:salmon/size", "minecraft:parrot/variant", "minecraft:tropical_fish/pattern", "minecraft:tropical_fish/base_color", "minecraft:tropical_fish/pattern_color", "minecraft:mooshroom/variant", "minecraft:rabbit/variant", "minecraft:pig/variant", "minecraft:pig/sound_variant", "minecraft:cow/variant", "minecraft:cow/sound_variant", "minecraft:chicken/variant", "minecraft:chicken/sound_variant", "minecraft:zombie_nautilus/variant", "minecraft:frog/variant", "minecraft:horse/variant", "minecraft:painting/variant", "minecraft:llama/variant", "minecraft:axolotl/variant", "minecraft:cat/variant", "minecraft:cat/sound_variant", "minecraft:cat/collar", "minecraft:sheep/color", "minecraft:shulker/color", "minecraft:provides_pottery_pattern", "minecraft:sign_text_front", "minecraft:sign_text_back", "minecraft:waxed", "minecraft:cushion/color"];
    pub const CUSTOM_DATA: i32 = 0;
    pub const MAX_STACK_SIZE: i32 = 1;
    pub const MAX_DAMAGE: i32 = 2;
    pub const DAMAGE: i32 = 3;
    pub const UNBREAKABLE: i32 = 4;
    pub const USE_EFFECTS: i32 = 5;
    pub const CUSTOM_NAME: i32 = 6;
    pub const MINIMUM_ATTACK_CHARGE: i32 = 7;
    pub const DAMAGE_TYPE: i32 = 8;
    pub const ITEM_NAME: i32 = 9;
    pub const ITEM_MODEL: i32 = 10;
    pub const LORE: i32 = 11;
    pub const RARITY: i32 = 12;
    pub const ENCHANTMENTS: i32 = 13;
    pub const CAN_PLACE_ON: i32 = 14;
    pub const CAN_BREAK: i32 = 15;
    pub const ATTRIBUTE_MODIFIERS: i32 = 16;
    pub const CUSTOM_MODEL_DATA: i32 = 17;
    pub const TOOLTIP_DISPLAY: i32 = 18;
    pub const REPAIR_COST: i32 = 19;
    pub const CREATIVE_SLOT_LOCK: i32 = 20;
    pub const ENCHANTMENT_GLINT_OVERRIDE: i32 = 21;
    pub const INTANGIBLE_PROJECTILE: i32 = 22;
    pub const FOOD: i32 = 23;
    pub const CONSUMABLE: i32 = 24;
    pub const USE_REMAINDER: i32 = 25;
    pub const USE_COOLDOWN: i32 = 26;
    pub const DAMAGE_RESISTANT: i32 = 27;
    pub const TOOL: i32 = 28;
    pub const WEAPON: i32 = 29;
    pub const ATTACK_RANGE: i32 = 30;
    pub const ENCHANTABLE: i32 = 31;
    pub const EQUIPPABLE: i32 = 32;
    pub const REPAIRABLE: i32 = 33;
    pub const GLIDER: i32 = 34;
    pub const TOOLTIP_STYLE: i32 = 35;
    pub const DEATH_PROTECTION: i32 = 36;
    pub const BLOCKS_ATTACKS: i32 = 37;
    pub const PIERCING_WEAPON: i32 = 38;
    pub const KINETIC_WEAPON: i32 = 39;
    pub const ATTACK_ANIMATION: i32 = 40;
    pub const INTERACT_ANIMATION: i32 = 41;
    pub const ADDITIONAL_TRADE_COST: i32 = 42;
    pub const BLOCK_TRANSFORMER: i32 = 43;
    pub const VILLAGER_FOOD: i32 = 44;
    pub const STORED_ENCHANTMENTS: i32 = 45;
    pub const DYE: i32 = 46;
    pub const DYED_COLOR: i32 = 47;
    pub const MAP_ID: i32 = 48;
    pub const MAP_DECORATIONS: i32 = 49;
    pub const MAP_POST_PROCESSING: i32 = 50;
    pub const CHARGED_PROJECTILES: i32 = 51;
    pub const BUNDLE_CONTENTS: i32 = 52;
    pub const POTION_CONTENTS: i32 = 53;
    pub const POTION_DURATION_SCALE: i32 = 54;
    pub const SUSPICIOUS_STEW_EFFECTS: i32 = 55;
    pub const WRITABLE_BOOK_CONTENT: i32 = 56;
    pub const WRITTEN_BOOK_CONTENT: i32 = 57;
    pub const TRIM: i32 = 58;
    pub const DEBUG_STICK_STATE: i32 = 59;
    pub const ENTITY_DATA: i32 = 60;
    pub const BUCKET_ENTITY_DATA: i32 = 61;
    pub const BLOCK_ENTITY_DATA: i32 = 62;
    pub const INSTRUMENT: i32 = 63;
    pub const PROVIDES_TRIM_MATERIAL: i32 = 64;
    pub const OMINOUS_BOTTLE_AMPLIFIER: i32 = 65;
    pub const JUKEBOX_PLAYABLE: i32 = 66;
    pub const PROVIDES_BANNER_PATTERNS: i32 = 67;
    pub const RECIPES: i32 = 68;
    pub const LODESTONE_TRACKER: i32 = 69;
    pub const FIREWORK_EXPLOSION: i32 = 70;
    pub const FIREWORKS: i32 = 71;
    pub const PROFILE: i32 = 72;
    pub const NOTE_BLOCK_SOUND: i32 = 73;
    pub const BANNER_PATTERNS: i32 = 74;
    pub const BASE_COLOR: i32 = 75;
    pub const POT_DECORATIONS: i32 = 76;
    pub const CONTAINER: i32 = 77;
    pub const BLOCK_STATE: i32 = 78;
    pub const BEES: i32 = 79;
    pub const SULFUR_CUBE_CONTENT: i32 = 80;
    pub const LOCK: i32 = 81;
    pub const CONTAINER_LOOT: i32 = 82;
    pub const BREAK_SOUND: i32 = 83;
    pub const COMPOSTABLE: i32 = 84;
    pub const COOKING_FUEL: i32 = 85;
    pub const BREWING_FUEL: i32 = 86;
    pub const MOB_VISIBILITY: i32 = 87;
    pub const VILLAGER_VARIANT: i32 = 88;
    pub const WOLF_VARIANT: i32 = 89;
    pub const WOLF_SOUND_VARIANT: i32 = 90;
    pub const WOLF_COLLAR: i32 = 91;
    pub const FOX_VARIANT: i32 = 92;
    pub const SALMON_SIZE: i32 = 93;
    pub const PARROT_VARIANT: i32 = 94;
    pub const TROPICAL_FISH_PATTERN: i32 = 95;
    pub const TROPICAL_FISH_BASE_COLOR: i32 = 96;
    pub const TROPICAL_FISH_PATTERN_COLOR: i32 = 97;
    pub const MOOSHROOM_VARIANT: i32 = 98;
    pub const RABBIT_VARIANT: i32 = 99;
    pub const PIG_VARIANT: i32 = 100;
    pub const PIG_SOUND_VARIANT: i32 = 101;
    pub const COW_VARIANT: i32 = 102;
    pub const COW_SOUND_VARIANT: i32 = 103;
    pub const CHICKEN_VARIANT: i32 = 104;
    pub const CHICKEN_SOUND_VARIANT: i32 = 105;
    pub const ZOMBIE_NAUTILUS_VARIANT: i32 = 106;
    pub const FROG_VARIANT: i32 = 107;
    pub const HORSE_VARIANT: i32 = 108;
    pub const PAINTING_VARIANT: i32 = 109;
    pub const LLAMA_VARIANT: i32 = 110;
    pub const AXOLOTL_VARIANT: i32 = 111;
    pub const CAT_VARIANT: i32 = 112;
    pub const CAT_SOUND_VARIANT: i32 = 113;
    pub const CAT_COLLAR: i32 = 114;
    pub const SHEEP_COLOR: i32 = 115;
    pub const SHULKER_COLOR: i32 = 116;
    pub const PROVIDES_POTTERY_PATTERN: i32 = 117;
    pub const SIGN_TEXT_FRONT: i32 = 118;
    pub const SIGN_TEXT_BACK: i32 = 119;
    pub const WAXED: i32 = 120;
    pub const CUSHION_COLOR: i32 = 121;
}

pub mod fluid {
    pub const TOTAL: u32 = 5;
    /// 条目名，下标就是注册表 ID。
    pub static NAMES: &[&str] = &["minecraft:empty", "minecraft:flowing_water", "minecraft:water", "minecraft:flowing_lava", "minecraft:lava"];
    pub const EMPTY: i32 = 0;
    pub const FLOWING_WATER: i32 = 1;
    pub const WATER: i32 = 2;
    pub const FLOWING_LAVA: i32 = 3;
    pub const LAVA: i32 = 4;
}

pub mod potion {
    pub const TOTAL: u32 = 46;
    /// 条目名，下标就是注册表 ID。
    pub static NAMES: &[&str] = &["minecraft:water", "minecraft:mundane", "minecraft:thick", "minecraft:awkward", "minecraft:night_vision", "minecraft:long_night_vision", "minecraft:invisibility", "minecraft:long_invisibility", "minecraft:leaping", "minecraft:long_leaping", "minecraft:strong_leaping", "minecraft:fire_resistance", "minecraft:long_fire_resistance", "minecraft:swiftness", "minecraft:long_swiftness", "minecraft:strong_swiftness", "minecraft:slowness", "minecraft:long_slowness", "minecraft:strong_slowness", "minecraft:turtle_master", "minecraft:long_turtle_master", "minecraft:strong_turtle_master", "minecraft:water_breathing", "minecraft:long_water_breathing", "minecraft:healing", "minecraft:strong_healing", "minecraft:harming", "minecraft:strong_harming", "minecraft:poison", "minecraft:long_poison", "minecraft:strong_poison", "minecraft:regeneration", "minecraft:long_regeneration", "minecraft:strong_regeneration", "minecraft:strength", "minecraft:long_strength", "minecraft:strong_strength", "minecraft:weakness", "minecraft:long_weakness", "minecraft:luck", "minecraft:slow_falling", "minecraft:long_slow_falling", "minecraft:wind_charged", "minecraft:weaving", "minecraft:oozing", "minecraft:infested"];
    pub const WATER: i32 = 0;
    pub const MUNDANE: i32 = 1;
    pub const THICK: i32 = 2;
    pub const AWKWARD: i32 = 3;
    pub const NIGHT_VISION: i32 = 4;
    pub const LONG_NIGHT_VISION: i32 = 5;
    pub const INVISIBILITY: i32 = 6;
    pub const LONG_INVISIBILITY: i32 = 7;
    pub const LEAPING: i32 = 8;
    pub const LONG_LEAPING: i32 = 9;
    pub const STRONG_LEAPING: i32 = 10;
    pub const FIRE_RESISTANCE: i32 = 11;
    pub const LONG_FIRE_RESISTANCE: i32 = 12;
    pub const SWIFTNESS: i32 = 13;
    pub const LONG_SWIFTNESS: i32 = 14;
    pub const STRONG_SWIFTNESS: i32 = 15;
    pub const SLOWNESS: i32 = 16;
    pub const LONG_SLOWNESS: i32 = 17;
    pub const STRONG_SLOWNESS: i32 = 18;
    pub const TURTLE_MASTER: i32 = 19;
    pub const LONG_TURTLE_MASTER: i32 = 20;
    pub const STRONG_TURTLE_MASTER: i32 = 21;
    pub const WATER_BREATHING: i32 = 22;
    pub const LONG_WATER_BREATHING: i32 = 23;
    pub const HEALING: i32 = 24;
    pub const STRONG_HEALING: i32 = 25;
    pub const HARMING: i32 = 26;
    pub const STRONG_HARMING: i32 = 27;
    pub const POISON: i32 = 28;
    pub const LONG_POISON: i32 = 29;
    pub const STRONG_POISON: i32 = 30;
    pub const REGENERATION: i32 = 31;
    pub const LONG_REGENERATION: i32 = 32;
    pub const STRONG_REGENERATION: i32 = 33;
    pub const STRENGTH: i32 = 34;
    pub const LONG_STRENGTH: i32 = 35;
    pub const STRONG_STRENGTH: i32 = 36;
    pub const WEAKNESS: i32 = 37;
    pub const LONG_WEAKNESS: i32 = 38;
    pub const LUCK: i32 = 39;
    pub const SLOW_FALLING: i32 = 40;
    pub const LONG_SLOW_FALLING: i32 = 41;
    pub const WIND_CHARGED: i32 = 42;
    pub const WEAVING: i32 = 43;
    pub const OOZING: i32 = 44;
    pub const INFESTED: i32 = 45;
}

pub mod mob_effect {
    pub const TOTAL: u32 = 40;
    /// 条目名，下标就是注册表 ID。
    pub static NAMES: &[&str] = &["minecraft:speed", "minecraft:slowness", "minecraft:haste", "minecraft:mining_fatigue", "minecraft:strength", "minecraft:instant_health", "minecraft:instant_damage", "minecraft:jump_boost", "minecraft:nausea", "minecraft:regeneration", "minecraft:resistance", "minecraft:fire_resistance", "minecraft:water_breathing", "minecraft:invisibility", "minecraft:blindness", "minecraft:night_vision", "minecraft:hunger", "minecraft:weakness", "minecraft:poison", "minecraft:wither", "minecraft:health_boost", "minecraft:absorption", "minecraft:saturation", "minecraft:glowing", "minecraft:levitation", "minecraft:luck", "minecraft:unluck", "minecraft:slow_falling", "minecraft:conduit_power", "minecraft:dolphins_grace", "minecraft:bad_omen", "minecraft:hero_of_the_village", "minecraft:darkness", "minecraft:trial_omen", "minecraft:raid_omen", "minecraft:wind_charged", "minecraft:weaving", "minecraft:oozing", "minecraft:infested", "minecraft:breath_of_the_nautilus"];
    pub const SPEED: i32 = 0;
    pub const SLOWNESS: i32 = 1;
    pub const HASTE: i32 = 2;
    pub const MINING_FATIGUE: i32 = 3;
    pub const STRENGTH: i32 = 4;
    pub const INSTANT_HEALTH: i32 = 5;
    pub const INSTANT_DAMAGE: i32 = 6;
    pub const JUMP_BOOST: i32 = 7;
    pub const NAUSEA: i32 = 8;
    pub const REGENERATION: i32 = 9;
    pub const RESISTANCE: i32 = 10;
    pub const FIRE_RESISTANCE: i32 = 11;
    pub const WATER_BREATHING: i32 = 12;
    pub const INVISIBILITY: i32 = 13;
    pub const BLINDNESS: i32 = 14;
    pub const NIGHT_VISION: i32 = 15;
    pub const HUNGER: i32 = 16;
    pub const WEAKNESS: i32 = 17;
    pub const POISON: i32 = 18;
    pub const WITHER: i32 = 19;
    pub const HEALTH_BOOST: i32 = 20;
    pub const ABSORPTION: i32 = 21;
    pub const SATURATION: i32 = 22;
    pub const GLOWING: i32 = 23;
    pub const LEVITATION: i32 = 24;
    pub const LUCK: i32 = 25;
    pub const UNLUCK: i32 = 26;
    pub const SLOW_FALLING: i32 = 27;
    pub const CONDUIT_POWER: i32 = 28;
    pub const DOLPHINS_GRACE: i32 = 29;
    pub const BAD_OMEN: i32 = 30;
    pub const HERO_OF_THE_VILLAGE: i32 = 31;
    pub const DARKNESS: i32 = 32;
    pub const TRIAL_OMEN: i32 = 33;
    pub const RAID_OMEN: i32 = 34;
    pub const WIND_CHARGED: i32 = 35;
    pub const WEAVING: i32 = 36;
    pub const OOZING: i32 = 37;
    pub const INFESTED: i32 = 38;
    pub const BREATH_OF_THE_NAUTILUS: i32 = 39;
}

pub mod banner_pattern {
    pub const TOTAL: u32 = 43;
    /// 条目名，下标就是注册表 ID。
    pub static NAMES: &[&str] = &["minecraft:base", "minecraft:border", "minecraft:bricks", "minecraft:circle", "minecraft:creeper", "minecraft:cross", "minecraft:curly_border", "minecraft:diagonal_left", "minecraft:diagonal_right", "minecraft:diagonal_up_left", "minecraft:diagonal_up_right", "minecraft:flow", "minecraft:flower", "minecraft:globe", "minecraft:gradient", "minecraft:gradient_up", "minecraft:guster", "minecraft:half_horizontal", "minecraft:half_horizontal_bottom", "minecraft:half_vertical", "minecraft:half_vertical_right", "minecraft:mojang", "minecraft:piglin", "minecraft:rhombus", "minecraft:skull", "minecraft:small_stripes", "minecraft:square_bottom_left", "minecraft:square_bottom_right", "minecraft:square_top_left", "minecraft:square_top_right", "minecraft:straight_cross", "minecraft:stripe_bottom", "minecraft:stripe_center", "minecraft:stripe_downleft", "minecraft:stripe_downright", "minecraft:stripe_left", "minecraft:stripe_middle", "minecraft:stripe_right", "minecraft:stripe_top", "minecraft:triangle_bottom", "minecraft:triangle_top", "minecraft:triangles_bottom", "minecraft:triangles_top"];
    pub const BASE: i32 = 0;
    pub const BORDER: i32 = 1;
    pub const BRICKS: i32 = 2;
    pub const CIRCLE: i32 = 3;
    pub const CREEPER: i32 = 4;
    pub const CROSS: i32 = 5;
    pub const CURLY_BORDER: i32 = 6;
    pub const DIAGONAL_LEFT: i32 = 7;
    pub const DIAGONAL_RIGHT: i32 = 8;
    pub const DIAGONAL_UP_LEFT: i32 = 9;
    pub const DIAGONAL_UP_RIGHT: i32 = 10;
    pub const FLOW: i32 = 11;
    pub const FLOWER: i32 = 12;
    pub const GLOBE: i32 = 13;
    pub const GRADIENT: i32 = 14;
    pub const GRADIENT_UP: i32 = 15;
    pub const GUSTER: i32 = 16;
    pub const HALF_HORIZONTAL: i32 = 17;
    pub const HALF_HORIZONTAL_BOTTOM: i32 = 18;
    pub const HALF_VERTICAL: i32 = 19;
    pub const HALF_VERTICAL_RIGHT: i32 = 20;
    pub const MOJANG: i32 = 21;
    pub const PIGLIN: i32 = 22;
    pub const RHOMBUS: i32 = 23;
    pub const SKULL: i32 = 24;
    pub const SMALL_STRIPES: i32 = 25;
    pub const SQUARE_BOTTOM_LEFT: i32 = 26;
    pub const SQUARE_BOTTOM_RIGHT: i32 = 27;
    pub const SQUARE_TOP_LEFT: i32 = 28;
    pub const SQUARE_TOP_RIGHT: i32 = 29;
    pub const STRAIGHT_CROSS: i32 = 30;
    pub const STRIPE_BOTTOM: i32 = 31;
    pub const STRIPE_CENTER: i32 = 32;
    pub const STRIPE_DOWNLEFT: i32 = 33;
    pub const STRIPE_DOWNRIGHT: i32 = 34;
    pub const STRIPE_LEFT: i32 = 35;
    pub const STRIPE_MIDDLE: i32 = 36;
    pub const STRIPE_RIGHT: i32 = 37;
    pub const STRIPE_TOP: i32 = 38;
    pub const TRIANGLE_BOTTOM: i32 = 39;
    pub const TRIANGLE_TOP: i32 = 40;
    pub const TRIANGLES_BOTTOM: i32 = 41;
    pub const TRIANGLES_TOP: i32 = 42;
}

pub mod menu {
    pub const TOTAL: u32 = 25;
    /// 条目名，下标就是注册表 ID。
    pub static NAMES: &[&str] = &["minecraft:generic_9x1", "minecraft:generic_9x2", "minecraft:generic_9x3", "minecraft:generic_9x4", "minecraft:generic_9x5", "minecraft:generic_9x6", "minecraft:generic_3x3", "minecraft:crafter_3x3", "minecraft:anvil", "minecraft:beacon", "minecraft:blast_furnace", "minecraft:brewing_stand", "minecraft:crafting", "minecraft:enchantment", "minecraft:furnace", "minecraft:grindstone", "minecraft:hopper", "minecraft:lectern", "minecraft:loom", "minecraft:merchant", "minecraft:shulker_box", "minecraft:smithing", "minecraft:smoker", "minecraft:cartography_table", "minecraft:stonecutter"];
    pub const GENERIC_9X1: i32 = 0;
    pub const GENERIC_9X2: i32 = 1;
    pub const GENERIC_9X3: i32 = 2;
    pub const GENERIC_9X4: i32 = 3;
    pub const GENERIC_9X5: i32 = 4;
    pub const GENERIC_9X6: i32 = 5;
    pub const GENERIC_3X3: i32 = 6;
    pub const CRAFTER_3X3: i32 = 7;
    pub const ANVIL: i32 = 8;
    pub const BEACON: i32 = 9;
    pub const BLAST_FURNACE: i32 = 10;
    pub const BREWING_STAND: i32 = 11;
    pub const CRAFTING: i32 = 12;
    pub const ENCHANTMENT: i32 = 13;
    pub const FURNACE: i32 = 14;
    pub const GRINDSTONE: i32 = 15;
    pub const HOPPER: i32 = 16;
    pub const LECTERN: i32 = 17;
    pub const LOOM: i32 = 18;
    pub const MERCHANT: i32 = 19;
    pub const SHULKER_BOX: i32 = 20;
    pub const SMITHING: i32 = 21;
    pub const SMOKER: i32 = 22;
    pub const CARTOGRAPHY_TABLE: i32 = 23;
    pub const STONECUTTER: i32 = 24;
}

/// 属性的默认值，没有任何状态效果和附魔时的起点。
pub mod attribute_defaults {
    pub const AIR_DRAG_MODIFIER: f64 = 1.0;
    pub const ARMOR: f64 = 0.0;
    pub const ARMOR_TOUGHNESS: f64 = 0.0;
    pub const ATTACK_DAMAGE: f64 = 2.0;
    pub const ATTACK_KNOCKBACK: f64 = 0.0;
    pub const ATTACK_SPEED: f64 = 4.0;
    pub const BELOW_NAME_DISTANCE: f64 = 10.0;
    pub const BLOCK_BREAK_SPEED: f64 = 1.0;
    pub const BLOCK_INTERACTION_RANGE: f64 = 4.5;
    pub const BOUNCINESS: f64 = 0.0;
    pub const BURNING_TIME: f64 = 1.0;
    pub const CAMERA_DISTANCE: f64 = 4.0;
    pub const ENTITY_INTERACTION_RANGE: f64 = 3.0;
    pub const EXPLOSION_KNOCKBACK_RESISTANCE: f64 = 0.0;
    pub const FALL_DAMAGE_MULTIPLIER: f64 = 1.0;
    pub const FLYING_SPEED: f64 = 0.4;
    pub const FOLLOW_RANGE: f64 = 32.0;
    pub const FRICTION_MODIFIER: f64 = 1.0;
    pub const GRAVITY: f64 = 0.08;
    pub const JUMP_STRENGTH: f64 = 0.41999998688697815;
    pub const KNOCKBACK_RESISTANCE: f64 = 0.0;
    pub const LUCK: f64 = 0.0;
    pub const MAX_ABSORPTION: f64 = 0.0;
    pub const MAX_HEALTH: f64 = 20.0;
    pub const MINING_EFFICIENCY: f64 = 0.0;
    pub const MOVEMENT_EFFICIENCY: f64 = 0.0;
    pub const MOVEMENT_SPEED: f64 = 0.7;
    pub const NAME_TAG_DISTANCE: f64 = 64.0;
    pub const OXYGEN_BONUS: f64 = 0.0;
    pub const SAFE_FALL_DISTANCE: f64 = 3.0;
    pub const SCALE: f64 = 1.0;
    pub const SNEAKING_SPEED: f64 = 0.3;
    pub const SPAWN_REINFORCEMENTS: f64 = 0.0;
    pub const STEP_HEIGHT: f64 = 0.6;
    pub const SUBMERGED_MINING_SPEED: f64 = 0.2;
    pub const SWEEPING_DAMAGE_RATIO: f64 = 0.0;
    pub const TEMPT_RANGE: f64 = 10.0;
    pub const WATER_MOVEMENT_EFFICIENCY: f64 = 0.0;
    pub const WAYPOINT_RECEIVE_RANGE: f64 = 0.0;
    pub const WAYPOINT_TRANSMIT_RANGE: f64 = 0.0;
}

