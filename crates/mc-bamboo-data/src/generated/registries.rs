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

pub mod dimension_type {
    pub const TOTAL: u32 = 4;
    pub const OVERWORLD: i32 = 0;
    pub const OVERWORLD_CAVES: i32 = 1;
    pub const THE_END: i32 = 2;
    pub const THE_NETHER: i32 = 3;
}

pub mod world_clock {
    pub const TOTAL: u32 = 2;
    pub const OVERWORLD: i32 = 0;
    pub const THE_END: i32 = 1;
}

