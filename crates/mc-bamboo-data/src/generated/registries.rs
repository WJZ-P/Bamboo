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

pub mod chat_type {
    pub const TOTAL: u32 = 7;
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

pub mod attribute {
    pub const TOTAL: u32 = 40;
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
    pub const EMPTY: i32 = 0;
    pub const FLOWING_WATER: i32 = 1;
    pub const WATER: i32 = 2;
    pub const FLOWING_LAVA: i32 = 3;
    pub const LAVA: i32 = 4;
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

