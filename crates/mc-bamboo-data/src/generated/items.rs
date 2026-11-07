// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。

use crate::{Item, ItemKind};

/// 各物品的注册表 ID。
pub const AIR: i32 = 0;
pub const STONE: i32 = 1;
pub const GRANITE: i32 = 2;
pub const POLISHED_GRANITE: i32 = 3;
pub const DIORITE: i32 = 4;
pub const POLISHED_DIORITE: i32 = 5;
pub const ANDESITE: i32 = 6;
pub const POLISHED_ANDESITE: i32 = 7;
pub const DEEPSLATE: i32 = 8;
pub const COBBLED_DEEPSLATE: i32 = 9;
pub const POLISHED_DEEPSLATE: i32 = 10;
pub const CALCITE: i32 = 11;
pub const TUFF: i32 = 12;
pub const TUFF_SLAB: i32 = 13;
pub const TUFF_STAIRS: i32 = 14;
pub const TUFF_WALL: i32 = 15;
pub const CHISELED_TUFF: i32 = 16;
pub const POLISHED_TUFF: i32 = 17;
pub const POLISHED_TUFF_SLAB: i32 = 18;
pub const POLISHED_TUFF_STAIRS: i32 = 19;
pub const POLISHED_TUFF_WALL: i32 = 20;
pub const TUFF_BRICKS: i32 = 21;
pub const TUFF_BRICK_SLAB: i32 = 22;
pub const TUFF_BRICK_STAIRS: i32 = 23;
pub const TUFF_BRICK_WALL: i32 = 24;
pub const CHISELED_TUFF_BRICKS: i32 = 25;
pub const SULFUR: i32 = 26;
pub const POTENT_SULFUR: i32 = 27;
pub const SULFUR_SLAB: i32 = 28;
pub const SULFUR_STAIRS: i32 = 29;
pub const SULFUR_WALL: i32 = 30;
pub const POLISHED_SULFUR: i32 = 31;
pub const POLISHED_SULFUR_SLAB: i32 = 32;
pub const POLISHED_SULFUR_STAIRS: i32 = 33;
pub const POLISHED_SULFUR_WALL: i32 = 34;
pub const SULFUR_BRICKS: i32 = 35;
pub const SULFUR_BRICK_SLAB: i32 = 36;
pub const SULFUR_BRICK_STAIRS: i32 = 37;
pub const SULFUR_BRICK_WALL: i32 = 38;
pub const CHISELED_SULFUR: i32 = 39;
pub const CINNABAR: i32 = 40;
pub const CINNABAR_SLAB: i32 = 41;
pub const CINNABAR_STAIRS: i32 = 42;
pub const CINNABAR_WALL: i32 = 43;
pub const POLISHED_CINNABAR: i32 = 44;
pub const POLISHED_CINNABAR_SLAB: i32 = 45;
pub const POLISHED_CINNABAR_STAIRS: i32 = 46;
pub const POLISHED_CINNABAR_WALL: i32 = 47;
pub const CINNABAR_BRICKS: i32 = 48;
pub const CINNABAR_BRICK_SLAB: i32 = 49;
pub const CINNABAR_BRICK_STAIRS: i32 = 50;
pub const CINNABAR_BRICK_WALL: i32 = 51;
pub const CHISELED_CINNABAR: i32 = 52;
pub const DRIPSTONE_BLOCK: i32 = 53;
pub const GRASS_BLOCK: i32 = 54;
pub const DIRT: i32 = 55;
pub const COARSE_DIRT: i32 = 56;
pub const PODZOL: i32 = 57;
pub const ROOTED_DIRT: i32 = 58;
pub const MUD: i32 = 59;
pub const CRIMSON_NYLIUM: i32 = 60;
pub const WARPED_NYLIUM: i32 = 61;
pub const COBBLESTONE: i32 = 62;
pub const OAK_PLANKS: i32 = 63;
pub const SPRUCE_PLANKS: i32 = 64;
pub const BIRCH_PLANKS: i32 = 65;
pub const JUNGLE_PLANKS: i32 = 66;
pub const ACACIA_PLANKS: i32 = 67;
pub const CHERRY_PLANKS: i32 = 68;
pub const DARK_OAK_PLANKS: i32 = 69;
pub const PALE_OAK_PLANKS: i32 = 70;
pub const MANGROVE_PLANKS: i32 = 71;
pub const POPLAR_PLANKS: i32 = 72;
pub const BAMBOO_PLANKS: i32 = 73;
pub const CRIMSON_PLANKS: i32 = 74;
pub const WARPED_PLANKS: i32 = 75;
pub const BAMBOO_MOSAIC: i32 = 76;
pub const OAK_SAPLING: i32 = 77;
pub const SPRUCE_SAPLING: i32 = 78;
pub const BIRCH_SAPLING: i32 = 79;
pub const JUNGLE_SAPLING: i32 = 80;
pub const ACACIA_SAPLING: i32 = 81;
pub const CHERRY_SAPLING: i32 = 82;
pub const DARK_OAK_SAPLING: i32 = 83;
pub const PALE_OAK_SAPLING: i32 = 84;
pub const MANGROVE_PROPAGULE: i32 = 85;
pub const POPLAR_SAPLING: i32 = 86;
pub const BEDROCK: i32 = 87;
pub const SAND: i32 = 88;
pub const SUSPICIOUS_SAND: i32 = 89;
pub const SUSPICIOUS_GRAVEL: i32 = 90;
pub const RED_SAND: i32 = 91;
pub const GRAVEL: i32 = 92;
pub const COAL_ORE: i32 = 93;
pub const DEEPSLATE_COAL_ORE: i32 = 94;
pub const IRON_ORE: i32 = 95;
pub const DEEPSLATE_IRON_ORE: i32 = 96;
pub const COPPER_ORE: i32 = 97;
pub const DEEPSLATE_COPPER_ORE: i32 = 98;
pub const GOLD_ORE: i32 = 99;
pub const DEEPSLATE_GOLD_ORE: i32 = 100;
pub const REDSTONE_ORE: i32 = 101;
pub const DEEPSLATE_REDSTONE_ORE: i32 = 102;
pub const EMERALD_ORE: i32 = 103;
pub const DEEPSLATE_EMERALD_ORE: i32 = 104;
pub const LAPIS_ORE: i32 = 105;
pub const DEEPSLATE_LAPIS_ORE: i32 = 106;
pub const DIAMOND_ORE: i32 = 107;
pub const DEEPSLATE_DIAMOND_ORE: i32 = 108;
pub const NETHER_GOLD_ORE: i32 = 109;
pub const NETHER_QUARTZ_ORE: i32 = 110;
pub const ANCIENT_DEBRIS: i32 = 111;
pub const COAL_BLOCK: i32 = 112;
pub const RAW_IRON_BLOCK: i32 = 113;
pub const RAW_COPPER_BLOCK: i32 = 114;
pub const RAW_GOLD_BLOCK: i32 = 115;
pub const HEAVY_CORE: i32 = 116;
pub const AMETHYST_BLOCK: i32 = 117;
pub const BUDDING_AMETHYST: i32 = 118;
pub const IRON_BLOCK: i32 = 119;
pub const COPPER_BLOCK: i32 = 120;
pub const EXPOSED_COPPER: i32 = 121;
pub const WEATHERED_COPPER: i32 = 122;
pub const OXIDIZED_COPPER: i32 = 123;
pub const WAXED_COPPER_BLOCK: i32 = 124;
pub const WAXED_EXPOSED_COPPER: i32 = 125;
pub const WAXED_WEATHERED_COPPER: i32 = 126;
pub const WAXED_OXIDIZED_COPPER: i32 = 127;
pub const GOLD_BLOCK: i32 = 128;
pub const DIAMOND_BLOCK: i32 = 129;
pub const NETHERITE_BLOCK: i32 = 130;
pub const CHISELED_COPPER: i32 = 131;
pub const EXPOSED_CHISELED_COPPER: i32 = 132;
pub const WEATHERED_CHISELED_COPPER: i32 = 133;
pub const OXIDIZED_CHISELED_COPPER: i32 = 134;
pub const WAXED_CHISELED_COPPER: i32 = 135;
pub const WAXED_EXPOSED_CHISELED_COPPER: i32 = 136;
pub const WAXED_WEATHERED_CHISELED_COPPER: i32 = 137;
pub const WAXED_OXIDIZED_CHISELED_COPPER: i32 = 138;
pub const CUT_COPPER: i32 = 139;
pub const EXPOSED_CUT_COPPER: i32 = 140;
pub const WEATHERED_CUT_COPPER: i32 = 141;
pub const OXIDIZED_CUT_COPPER: i32 = 142;
pub const WAXED_CUT_COPPER: i32 = 143;
pub const WAXED_EXPOSED_CUT_COPPER: i32 = 144;
pub const WAXED_WEATHERED_CUT_COPPER: i32 = 145;
pub const WAXED_OXIDIZED_CUT_COPPER: i32 = 146;
pub const CUT_COPPER_STAIRS: i32 = 147;
pub const EXPOSED_CUT_COPPER_STAIRS: i32 = 148;
pub const WEATHERED_CUT_COPPER_STAIRS: i32 = 149;
pub const OXIDIZED_CUT_COPPER_STAIRS: i32 = 150;
pub const WAXED_CUT_COPPER_STAIRS: i32 = 151;
pub const WAXED_EXPOSED_CUT_COPPER_STAIRS: i32 = 152;
pub const WAXED_WEATHERED_CUT_COPPER_STAIRS: i32 = 153;
pub const WAXED_OXIDIZED_CUT_COPPER_STAIRS: i32 = 154;
pub const CUT_COPPER_SLAB: i32 = 155;
pub const EXPOSED_CUT_COPPER_SLAB: i32 = 156;
pub const WEATHERED_CUT_COPPER_SLAB: i32 = 157;
pub const OXIDIZED_CUT_COPPER_SLAB: i32 = 158;
pub const WAXED_CUT_COPPER_SLAB: i32 = 159;
pub const WAXED_EXPOSED_CUT_COPPER_SLAB: i32 = 160;
pub const WAXED_WEATHERED_CUT_COPPER_SLAB: i32 = 161;
pub const WAXED_OXIDIZED_CUT_COPPER_SLAB: i32 = 162;
pub const OAK_LOG: i32 = 163;
pub const SPRUCE_LOG: i32 = 164;
pub const BIRCH_LOG: i32 = 165;
pub const JUNGLE_LOG: i32 = 166;
pub const ACACIA_LOG: i32 = 167;
pub const CHERRY_LOG: i32 = 168;
pub const PALE_OAK_LOG: i32 = 169;
pub const DARK_OAK_LOG: i32 = 170;
pub const MANGROVE_LOG: i32 = 171;
pub const POPLAR_LOG: i32 = 172;
pub const MANGROVE_ROOTS: i32 = 173;
pub const MUDDY_MANGROVE_ROOTS: i32 = 174;
pub const CRIMSON_STEM: i32 = 175;
pub const WARPED_STEM: i32 = 176;
pub const BAMBOO_BLOCK: i32 = 177;
pub const STRIPPED_OAK_LOG: i32 = 178;
pub const STRIPPED_SPRUCE_LOG: i32 = 179;
pub const STRIPPED_BIRCH_LOG: i32 = 180;
pub const STRIPPED_JUNGLE_LOG: i32 = 181;
pub const STRIPPED_ACACIA_LOG: i32 = 182;
pub const STRIPPED_CHERRY_LOG: i32 = 183;
pub const STRIPPED_DARK_OAK_LOG: i32 = 184;
pub const STRIPPED_PALE_OAK_LOG: i32 = 185;
pub const STRIPPED_MANGROVE_LOG: i32 = 186;
pub const STRIPPED_POPLAR_LOG: i32 = 187;
pub const STRIPPED_CRIMSON_STEM: i32 = 188;
pub const STRIPPED_WARPED_STEM: i32 = 189;
pub const STRIPPED_OAK_WOOD: i32 = 190;
pub const STRIPPED_SPRUCE_WOOD: i32 = 191;
pub const STRIPPED_BIRCH_WOOD: i32 = 192;
pub const STRIPPED_JUNGLE_WOOD: i32 = 193;
pub const STRIPPED_ACACIA_WOOD: i32 = 194;
pub const STRIPPED_CHERRY_WOOD: i32 = 195;
pub const STRIPPED_DARK_OAK_WOOD: i32 = 196;
pub const STRIPPED_PALE_OAK_WOOD: i32 = 197;
pub const STRIPPED_MANGROVE_WOOD: i32 = 198;
pub const STRIPPED_POPLAR_WOOD: i32 = 199;
pub const STRIPPED_CRIMSON_HYPHAE: i32 = 200;
pub const STRIPPED_WARPED_HYPHAE: i32 = 201;
pub const STRIPPED_BAMBOO_BLOCK: i32 = 202;
pub const OAK_WOOD: i32 = 203;
pub const SPRUCE_WOOD: i32 = 204;
pub const BIRCH_WOOD: i32 = 205;
pub const JUNGLE_WOOD: i32 = 206;
pub const ACACIA_WOOD: i32 = 207;
pub const CHERRY_WOOD: i32 = 208;
pub const PALE_OAK_WOOD: i32 = 209;
pub const DARK_OAK_WOOD: i32 = 210;
pub const MANGROVE_WOOD: i32 = 211;
pub const POPLAR_WOOD: i32 = 212;
pub const CRIMSON_HYPHAE: i32 = 213;
pub const WARPED_HYPHAE: i32 = 214;
pub const OAK_LEAVES: i32 = 215;
pub const SPRUCE_LEAVES: i32 = 216;
pub const BIRCH_LEAVES: i32 = 217;
pub const JUNGLE_LEAVES: i32 = 218;
pub const ACACIA_LEAVES: i32 = 219;
pub const CHERRY_LEAVES: i32 = 220;
pub const DARK_OAK_LEAVES: i32 = 221;
pub const PALE_OAK_LEAVES: i32 = 222;
pub const MANGROVE_LEAVES: i32 = 223;
pub const RED_POPLAR_LEAVES: i32 = 224;
pub const ORANGE_POPLAR_LEAVES: i32 = 225;
pub const YELLOW_POPLAR_LEAVES: i32 = 226;
pub const AZALEA_LEAVES: i32 = 227;
pub const FLOWERING_AZALEA_LEAVES: i32 = 228;
pub const SPONGE: i32 = 229;
pub const WET_SPONGE: i32 = 230;
pub const GLASS: i32 = 231;
pub const TINTED_GLASS: i32 = 232;
pub const LAPIS_BLOCK: i32 = 233;
pub const SANDSTONE: i32 = 234;
pub const CHISELED_SANDSTONE: i32 = 235;
pub const CUT_SANDSTONE: i32 = 236;
pub const COBWEB: i32 = 237;
pub const SHORT_GRASS: i32 = 238;
pub const FERN: i32 = 239;
pub const BUSH: i32 = 240;
pub const RED_SHRUB: i32 = 241;
pub const AZALEA: i32 = 242;
pub const FLOWERING_AZALEA: i32 = 243;
pub const DEAD_BUSH: i32 = 244;
pub const FIREFLY_BUSH: i32 = 245;
pub const SHORT_DRY_GRASS: i32 = 246;
pub const TALL_DRY_GRASS: i32 = 247;
pub const SEAGRASS: i32 = 248;
pub const SEA_PICKLE: i32 = 249;
pub const WHITE_WOOL: i32 = 250;
pub const ORANGE_WOOL: i32 = 251;
pub const MAGENTA_WOOL: i32 = 252;
pub const LIGHT_BLUE_WOOL: i32 = 253;
pub const YELLOW_WOOL: i32 = 254;
pub const LIME_WOOL: i32 = 255;
pub const PINK_WOOL: i32 = 256;
pub const GRAY_WOOL: i32 = 257;
pub const LIGHT_GRAY_WOOL: i32 = 258;
pub const CYAN_WOOL: i32 = 259;
pub const PURPLE_WOOL: i32 = 260;
pub const BLUE_WOOL: i32 = 261;
pub const BROWN_WOOL: i32 = 262;
pub const GREEN_WOOL: i32 = 263;
pub const RED_WOOL: i32 = 264;
pub const BLACK_WOOL: i32 = 265;
pub const WHITE_WOOL_STAIRS: i32 = 266;
pub const ORANGE_WOOL_STAIRS: i32 = 267;
pub const MAGENTA_WOOL_STAIRS: i32 = 268;
pub const LIGHT_BLUE_WOOL_STAIRS: i32 = 269;
pub const YELLOW_WOOL_STAIRS: i32 = 270;
pub const LIME_WOOL_STAIRS: i32 = 271;
pub const PINK_WOOL_STAIRS: i32 = 272;
pub const GRAY_WOOL_STAIRS: i32 = 273;
pub const LIGHT_GRAY_WOOL_STAIRS: i32 = 274;
pub const CYAN_WOOL_STAIRS: i32 = 275;
pub const PURPLE_WOOL_STAIRS: i32 = 276;
pub const BLUE_WOOL_STAIRS: i32 = 277;
pub const BROWN_WOOL_STAIRS: i32 = 278;
pub const GREEN_WOOL_STAIRS: i32 = 279;
pub const RED_WOOL_STAIRS: i32 = 280;
pub const BLACK_WOOL_STAIRS: i32 = 281;
pub const WHITE_WOOL_SLAB: i32 = 282;
pub const ORANGE_WOOL_SLAB: i32 = 283;
pub const MAGENTA_WOOL_SLAB: i32 = 284;
pub const LIGHT_BLUE_WOOL_SLAB: i32 = 285;
pub const YELLOW_WOOL_SLAB: i32 = 286;
pub const LIME_WOOL_SLAB: i32 = 287;
pub const PINK_WOOL_SLAB: i32 = 288;
pub const GRAY_WOOL_SLAB: i32 = 289;
pub const LIGHT_GRAY_WOOL_SLAB: i32 = 290;
pub const CYAN_WOOL_SLAB: i32 = 291;
pub const PURPLE_WOOL_SLAB: i32 = 292;
pub const BLUE_WOOL_SLAB: i32 = 293;
pub const BROWN_WOOL_SLAB: i32 = 294;
pub const GREEN_WOOL_SLAB: i32 = 295;
pub const RED_WOOL_SLAB: i32 = 296;
pub const BLACK_WOOL_SLAB: i32 = 297;
pub const DANDELION: i32 = 298;
pub const GOLDEN_DANDELION: i32 = 299;
pub const OPEN_EYEBLOSSOM: i32 = 300;
pub const CLOSED_EYEBLOSSOM: i32 = 301;
pub const POPPY: i32 = 302;
pub const BLUE_ORCHID: i32 = 303;
pub const ALLIUM: i32 = 304;
pub const AZURE_BLUET: i32 = 305;
pub const RED_TULIP: i32 = 306;
pub const ORANGE_TULIP: i32 = 307;
pub const WHITE_TULIP: i32 = 308;
pub const PINK_TULIP: i32 = 309;
pub const OXEYE_DAISY: i32 = 310;
pub const CORNFLOWER: i32 = 311;
pub const LILY_OF_THE_VALLEY: i32 = 312;
pub const WITHER_ROSE: i32 = 313;
pub const TORCHFLOWER: i32 = 314;
pub const PITCHER_PLANT: i32 = 315;
pub const SPORE_BLOSSOM: i32 = 316;
pub const BROWN_MUSHROOM: i32 = 317;
pub const RED_MUSHROOM: i32 = 318;
pub const SHELF_MUSHROOM: i32 = 319;
pub const CRIMSON_FUNGUS: i32 = 320;
pub const WARPED_FUNGUS: i32 = 321;
pub const CRIMSON_ROOTS: i32 = 322;
pub const WARPED_ROOTS: i32 = 323;
pub const NETHER_SPROUTS: i32 = 324;
pub const WEEPING_VINES: i32 = 325;
pub const TWISTING_VINES: i32 = 326;
pub const SUGAR_CANE: i32 = 327;
pub const KELP: i32 = 328;
pub const PINK_PETALS: i32 = 329;
pub const WILDFLOWERS: i32 = 330;
pub const LEAF_LITTER: i32 = 331;
pub const MOSS_CARPET: i32 = 332;
pub const MOSS_BLOCK: i32 = 333;
pub const PALE_MOSS_CARPET: i32 = 334;
pub const PALE_HANGING_MOSS: i32 = 335;
pub const PALE_MOSS_BLOCK: i32 = 336;
pub const HANGING_ROOTS: i32 = 337;
pub const BIG_DRIPLEAF: i32 = 338;
pub const SMALL_DRIPLEAF: i32 = 339;
pub const BAMBOO: i32 = 340;
pub const OAK_SLAB: i32 = 341;
pub const SPRUCE_SLAB: i32 = 342;
pub const BIRCH_SLAB: i32 = 343;
pub const JUNGLE_SLAB: i32 = 344;
pub const ACACIA_SLAB: i32 = 345;
pub const CHERRY_SLAB: i32 = 346;
pub const DARK_OAK_SLAB: i32 = 347;
pub const PALE_OAK_SLAB: i32 = 348;
pub const MANGROVE_SLAB: i32 = 349;
pub const POPLAR_SLAB: i32 = 350;
pub const BAMBOO_SLAB: i32 = 351;
pub const BAMBOO_MOSAIC_SLAB: i32 = 352;
pub const CRIMSON_SLAB: i32 = 353;
pub const WARPED_SLAB: i32 = 354;
pub const STONE_SLAB: i32 = 355;
pub const SMOOTH_STONE_SLAB: i32 = 356;
pub const SANDSTONE_SLAB: i32 = 357;
pub const CUT_SANDSTONE_SLAB: i32 = 358;
pub const PETRIFIED_OAK_SLAB: i32 = 359;
pub const COBBLESTONE_SLAB: i32 = 360;
pub const BRICK_SLAB: i32 = 361;
pub const STONE_BRICK_SLAB: i32 = 362;
pub const MUD_BRICK_SLAB: i32 = 363;
pub const NETHER_BRICK_SLAB: i32 = 364;
pub const QUARTZ_SLAB: i32 = 365;
pub const RED_SANDSTONE_SLAB: i32 = 366;
pub const CUT_RED_SANDSTONE_SLAB: i32 = 367;
pub const PURPUR_SLAB: i32 = 368;
pub const PRISMARINE_SLAB: i32 = 369;
pub const PRISMARINE_BRICK_SLAB: i32 = 370;
pub const DARK_PRISMARINE_SLAB: i32 = 371;
pub const SMOOTH_QUARTZ: i32 = 372;
pub const SMOOTH_RED_SANDSTONE: i32 = 373;
pub const SMOOTH_SANDSTONE: i32 = 374;
pub const SMOOTH_STONE: i32 = 375;
pub const BRICKS: i32 = 376;
pub const ACACIA_SHELF: i32 = 377;
pub const BAMBOO_SHELF: i32 = 378;
pub const BIRCH_SHELF: i32 = 379;
pub const CHERRY_SHELF: i32 = 380;
pub const CRIMSON_SHELF: i32 = 381;
pub const DARK_OAK_SHELF: i32 = 382;
pub const JUNGLE_SHELF: i32 = 383;
pub const MANGROVE_SHELF: i32 = 384;
pub const POPLAR_SHELF: i32 = 385;
pub const OAK_SHELF: i32 = 386;
pub const PALE_OAK_SHELF: i32 = 387;
pub const SPRUCE_SHELF: i32 = 388;
pub const WARPED_SHELF: i32 = 389;
pub const BOOKSHELF: i32 = 390;
pub const CHISELED_BOOKSHELF: i32 = 391;
pub const DECORATED_POT: i32 = 392;
pub const MOSSY_COBBLESTONE: i32 = 393;
pub const OBSIDIAN: i32 = 394;
pub const TORCH: i32 = 395;
pub const END_ROD: i32 = 396;
pub const CHORUS_PLANT: i32 = 397;
pub const CHORUS_FLOWER: i32 = 398;
pub const PURPUR_BLOCK: i32 = 399;
pub const PURPUR_PILLAR: i32 = 400;
pub const PURPUR_STAIRS: i32 = 401;
pub const SPAWNER: i32 = 402;
pub const CREAKING_HEART: i32 = 403;
pub const CHEST: i32 = 404;
pub const CRAFTING_TABLE: i32 = 405;
pub const FARMLAND: i32 = 406;
pub const FURNACE: i32 = 407;
pub const LADDER: i32 = 408;
pub const COBBLESTONE_STAIRS: i32 = 409;
pub const SNOW: i32 = 410;
pub const ICE: i32 = 411;
pub const SNOW_BLOCK: i32 = 412;
pub const CACTUS: i32 = 413;
pub const CACTUS_FLOWER: i32 = 414;
pub const CLAY: i32 = 415;
pub const JUKEBOX: i32 = 416;
pub const OAK_FENCE: i32 = 417;
pub const SPRUCE_FENCE: i32 = 418;
pub const BIRCH_FENCE: i32 = 419;
pub const JUNGLE_FENCE: i32 = 420;
pub const ACACIA_FENCE: i32 = 421;
pub const CHERRY_FENCE: i32 = 422;
pub const DARK_OAK_FENCE: i32 = 423;
pub const PALE_OAK_FENCE: i32 = 424;
pub const MANGROVE_FENCE: i32 = 425;
pub const POPLAR_FENCE: i32 = 426;
pub const BAMBOO_FENCE: i32 = 427;
pub const CRIMSON_FENCE: i32 = 428;
pub const WARPED_FENCE: i32 = 429;
pub const PUMPKIN: i32 = 430;
pub const CARVED_PUMPKIN: i32 = 431;
pub const JACK_O_LANTERN: i32 = 432;
pub const NETHERRACK: i32 = 433;
pub const SOUL_SAND: i32 = 434;
pub const SOUL_SOIL: i32 = 435;
pub const BASALT: i32 = 436;
pub const POLISHED_BASALT: i32 = 437;
pub const SMOOTH_BASALT: i32 = 438;
pub const SOUL_TORCH: i32 = 439;
pub const COPPER_TORCH: i32 = 440;
pub const GLOWSTONE: i32 = 441;
pub const INFESTED_STONE: i32 = 442;
pub const INFESTED_COBBLESTONE: i32 = 443;
pub const INFESTED_STONE_BRICKS: i32 = 444;
pub const INFESTED_MOSSY_STONE_BRICKS: i32 = 445;
pub const INFESTED_CRACKED_STONE_BRICKS: i32 = 446;
pub const INFESTED_CHISELED_STONE_BRICKS: i32 = 447;
pub const INFESTED_DEEPSLATE: i32 = 448;
pub const STONE_BRICKS: i32 = 449;
pub const MOSSY_STONE_BRICKS: i32 = 450;
pub const CRACKED_STONE_BRICKS: i32 = 451;
pub const CHISELED_STONE_BRICKS: i32 = 452;
pub const PACKED_MUD: i32 = 453;
pub const MUD_BRICKS: i32 = 454;
pub const DEEPSLATE_BRICKS: i32 = 455;
pub const CRACKED_DEEPSLATE_BRICKS: i32 = 456;
pub const DEEPSLATE_TILES: i32 = 457;
pub const CRACKED_DEEPSLATE_TILES: i32 = 458;
pub const CHISELED_DEEPSLATE: i32 = 459;
pub const REINFORCED_DEEPSLATE: i32 = 460;
pub const BROWN_MUSHROOM_BLOCK: i32 = 461;
pub const RED_MUSHROOM_BLOCK: i32 = 462;
pub const MUSHROOM_STEM: i32 = 463;
pub const IRON_BARS: i32 = 464;
pub const COPPER_BARS: i32 = 465;
pub const EXPOSED_COPPER_BARS: i32 = 466;
pub const WEATHERED_COPPER_BARS: i32 = 467;
pub const OXIDIZED_COPPER_BARS: i32 = 468;
pub const WAXED_COPPER_BARS: i32 = 469;
pub const WAXED_EXPOSED_COPPER_BARS: i32 = 470;
pub const WAXED_WEATHERED_COPPER_BARS: i32 = 471;
pub const WAXED_OXIDIZED_COPPER_BARS: i32 = 472;
pub const IRON_CHAIN: i32 = 473;
pub const COPPER_CHAIN: i32 = 474;
pub const EXPOSED_COPPER_CHAIN: i32 = 475;
pub const WEATHERED_COPPER_CHAIN: i32 = 476;
pub const OXIDIZED_COPPER_CHAIN: i32 = 477;
pub const WAXED_COPPER_CHAIN: i32 = 478;
pub const WAXED_EXPOSED_COPPER_CHAIN: i32 = 479;
pub const WAXED_WEATHERED_COPPER_CHAIN: i32 = 480;
pub const WAXED_OXIDIZED_COPPER_CHAIN: i32 = 481;
pub const GLASS_PANE: i32 = 482;
pub const MELON: i32 = 483;
pub const VINE: i32 = 484;
pub const GLOW_LICHEN: i32 = 485;
pub const RESIN_CLUMP: i32 = 486;
pub const RESIN_BLOCK: i32 = 487;
pub const RESIN_BRICKS: i32 = 488;
pub const RESIN_BRICK_STAIRS: i32 = 489;
pub const RESIN_BRICK_SLAB: i32 = 490;
pub const RESIN_BRICK_WALL: i32 = 491;
pub const CHISELED_RESIN_BRICKS: i32 = 492;
pub const BRICK_STAIRS: i32 = 493;
pub const STONE_BRICK_STAIRS: i32 = 494;
pub const MUD_BRICK_STAIRS: i32 = 495;
pub const MYCELIUM: i32 = 496;
pub const LILY_PAD: i32 = 497;
pub const NETHER_BRICKS: i32 = 498;
pub const CRACKED_NETHER_BRICKS: i32 = 499;
pub const CHISELED_NETHER_BRICKS: i32 = 500;
pub const NETHER_BRICK_FENCE: i32 = 501;
pub const NETHER_BRICK_STAIRS: i32 = 502;
pub const SCULK: i32 = 503;
pub const SCULK_VEIN: i32 = 504;
pub const SCULK_CATALYST: i32 = 505;
pub const SCULK_SHRIEKER: i32 = 506;
pub const ENCHANTING_TABLE: i32 = 507;
pub const END_PORTAL_FRAME: i32 = 508;
pub const END_STONE: i32 = 509;
pub const END_STONE_BRICKS: i32 = 510;
pub const DRAGON_EGG: i32 = 511;
pub const SANDSTONE_STAIRS: i32 = 512;
pub const ENDER_CHEST: i32 = 513;
pub const EMERALD_BLOCK: i32 = 514;
pub const OAK_STAIRS: i32 = 515;
pub const SPRUCE_STAIRS: i32 = 516;
pub const BIRCH_STAIRS: i32 = 517;
pub const JUNGLE_STAIRS: i32 = 518;
pub const ACACIA_STAIRS: i32 = 519;
pub const CHERRY_STAIRS: i32 = 520;
pub const DARK_OAK_STAIRS: i32 = 521;
pub const PALE_OAK_STAIRS: i32 = 522;
pub const MANGROVE_STAIRS: i32 = 523;
pub const POPLAR_STAIRS: i32 = 524;
pub const BAMBOO_STAIRS: i32 = 525;
pub const BAMBOO_MOSAIC_STAIRS: i32 = 526;
pub const CRIMSON_STAIRS: i32 = 527;
pub const WARPED_STAIRS: i32 = 528;
pub const COMMAND_BLOCK: i32 = 529;
pub const BEACON: i32 = 530;
pub const COBBLESTONE_WALL: i32 = 531;
pub const MOSSY_COBBLESTONE_WALL: i32 = 532;
pub const BRICK_WALL: i32 = 533;
pub const PRISMARINE_WALL: i32 = 534;
pub const RED_SANDSTONE_WALL: i32 = 535;
pub const MOSSY_STONE_BRICK_WALL: i32 = 536;
pub const GRANITE_WALL: i32 = 537;
pub const STONE_BRICK_WALL: i32 = 538;
pub const MUD_BRICK_WALL: i32 = 539;
pub const NETHER_BRICK_WALL: i32 = 540;
pub const ANDESITE_WALL: i32 = 541;
pub const RED_NETHER_BRICK_WALL: i32 = 542;
pub const SANDSTONE_WALL: i32 = 543;
pub const END_STONE_BRICK_WALL: i32 = 544;
pub const DIORITE_WALL: i32 = 545;
pub const BLACKSTONE_WALL: i32 = 546;
pub const POLISHED_BLACKSTONE_WALL: i32 = 547;
pub const POLISHED_BLACKSTONE_BRICK_WALL: i32 = 548;
pub const COBBLED_DEEPSLATE_WALL: i32 = 549;
pub const POLISHED_DEEPSLATE_WALL: i32 = 550;
pub const DEEPSLATE_BRICK_WALL: i32 = 551;
pub const DEEPSLATE_TILE_WALL: i32 = 552;
pub const ANVIL: i32 = 553;
pub const CHIPPED_ANVIL: i32 = 554;
pub const DAMAGED_ANVIL: i32 = 555;
pub const CHISELED_QUARTZ_BLOCK: i32 = 556;
pub const QUARTZ_BLOCK: i32 = 557;
pub const QUARTZ_BRICKS: i32 = 558;
pub const QUARTZ_PILLAR: i32 = 559;
pub const QUARTZ_STAIRS: i32 = 560;
pub const WHITE_TERRACOTTA: i32 = 561;
pub const ORANGE_TERRACOTTA: i32 = 562;
pub const MAGENTA_TERRACOTTA: i32 = 563;
pub const LIGHT_BLUE_TERRACOTTA: i32 = 564;
pub const YELLOW_TERRACOTTA: i32 = 565;
pub const LIME_TERRACOTTA: i32 = 566;
pub const PINK_TERRACOTTA: i32 = 567;
pub const GRAY_TERRACOTTA: i32 = 568;
pub const LIGHT_GRAY_TERRACOTTA: i32 = 569;
pub const CYAN_TERRACOTTA: i32 = 570;
pub const PURPLE_TERRACOTTA: i32 = 571;
pub const BLUE_TERRACOTTA: i32 = 572;
pub const BROWN_TERRACOTTA: i32 = 573;
pub const GREEN_TERRACOTTA: i32 = 574;
pub const RED_TERRACOTTA: i32 = 575;
pub const BLACK_TERRACOTTA: i32 = 576;
pub const BARRIER: i32 = 577;
pub const LIGHT: i32 = 578;
pub const HAY_BLOCK: i32 = 579;
pub const WHITE_CARPET: i32 = 580;
pub const ORANGE_CARPET: i32 = 581;
pub const MAGENTA_CARPET: i32 = 582;
pub const LIGHT_BLUE_CARPET: i32 = 583;
pub const YELLOW_CARPET: i32 = 584;
pub const LIME_CARPET: i32 = 585;
pub const PINK_CARPET: i32 = 586;
pub const GRAY_CARPET: i32 = 587;
pub const LIGHT_GRAY_CARPET: i32 = 588;
pub const CYAN_CARPET: i32 = 589;
pub const PURPLE_CARPET: i32 = 590;
pub const BLUE_CARPET: i32 = 591;
pub const BROWN_CARPET: i32 = 592;
pub const GREEN_CARPET: i32 = 593;
pub const RED_CARPET: i32 = 594;
pub const BLACK_CARPET: i32 = 595;
pub const TERRACOTTA: i32 = 596;
pub const PACKED_ICE: i32 = 597;
pub const DIRT_PATH: i32 = 598;
pub const SUNFLOWER: i32 = 599;
pub const LILAC: i32 = 600;
pub const ROSE_BUSH: i32 = 601;
pub const PEONY: i32 = 602;
pub const TALL_GRASS: i32 = 603;
pub const LARGE_FERN: i32 = 604;
pub const WHITE_STAINED_GLASS: i32 = 605;
pub const ORANGE_STAINED_GLASS: i32 = 606;
pub const MAGENTA_STAINED_GLASS: i32 = 607;
pub const LIGHT_BLUE_STAINED_GLASS: i32 = 608;
pub const YELLOW_STAINED_GLASS: i32 = 609;
pub const LIME_STAINED_GLASS: i32 = 610;
pub const PINK_STAINED_GLASS: i32 = 611;
pub const GRAY_STAINED_GLASS: i32 = 612;
pub const LIGHT_GRAY_STAINED_GLASS: i32 = 613;
pub const CYAN_STAINED_GLASS: i32 = 614;
pub const PURPLE_STAINED_GLASS: i32 = 615;
pub const BLUE_STAINED_GLASS: i32 = 616;
pub const BROWN_STAINED_GLASS: i32 = 617;
pub const GREEN_STAINED_GLASS: i32 = 618;
pub const RED_STAINED_GLASS: i32 = 619;
pub const BLACK_STAINED_GLASS: i32 = 620;
pub const WHITE_STAINED_GLASS_PANE: i32 = 621;
pub const ORANGE_STAINED_GLASS_PANE: i32 = 622;
pub const MAGENTA_STAINED_GLASS_PANE: i32 = 623;
pub const LIGHT_BLUE_STAINED_GLASS_PANE: i32 = 624;
pub const YELLOW_STAINED_GLASS_PANE: i32 = 625;
pub const LIME_STAINED_GLASS_PANE: i32 = 626;
pub const PINK_STAINED_GLASS_PANE: i32 = 627;
pub const GRAY_STAINED_GLASS_PANE: i32 = 628;
pub const LIGHT_GRAY_STAINED_GLASS_PANE: i32 = 629;
pub const CYAN_STAINED_GLASS_PANE: i32 = 630;
pub const PURPLE_STAINED_GLASS_PANE: i32 = 631;
pub const BLUE_STAINED_GLASS_PANE: i32 = 632;
pub const BROWN_STAINED_GLASS_PANE: i32 = 633;
pub const GREEN_STAINED_GLASS_PANE: i32 = 634;
pub const RED_STAINED_GLASS_PANE: i32 = 635;
pub const BLACK_STAINED_GLASS_PANE: i32 = 636;
pub const PRISMARINE: i32 = 637;
pub const PRISMARINE_BRICKS: i32 = 638;
pub const DARK_PRISMARINE: i32 = 639;
pub const PRISMARINE_STAIRS: i32 = 640;
pub const PRISMARINE_BRICK_STAIRS: i32 = 641;
pub const DARK_PRISMARINE_STAIRS: i32 = 642;
pub const SEA_LANTERN: i32 = 643;
pub const RED_SANDSTONE: i32 = 644;
pub const CHISELED_RED_SANDSTONE: i32 = 645;
pub const CUT_RED_SANDSTONE: i32 = 646;
pub const RED_SANDSTONE_STAIRS: i32 = 647;
pub const REPEATING_COMMAND_BLOCK: i32 = 648;
pub const CHAIN_COMMAND_BLOCK: i32 = 649;
pub const MAGMA_BLOCK: i32 = 650;
pub const NETHER_WART_BLOCK: i32 = 651;
pub const WARPED_WART_BLOCK: i32 = 652;
pub const RED_NETHER_BRICKS: i32 = 653;
pub const BONE_BLOCK: i32 = 654;
pub const STRUCTURE_VOID: i32 = 655;
pub const SHULKER_BOX: i32 = 656;
pub const WHITE_SHULKER_BOX: i32 = 657;
pub const ORANGE_SHULKER_BOX: i32 = 658;
pub const MAGENTA_SHULKER_BOX: i32 = 659;
pub const LIGHT_BLUE_SHULKER_BOX: i32 = 660;
pub const YELLOW_SHULKER_BOX: i32 = 661;
pub const LIME_SHULKER_BOX: i32 = 662;
pub const PINK_SHULKER_BOX: i32 = 663;
pub const GRAY_SHULKER_BOX: i32 = 664;
pub const LIGHT_GRAY_SHULKER_BOX: i32 = 665;
pub const CYAN_SHULKER_BOX: i32 = 666;
pub const PURPLE_SHULKER_BOX: i32 = 667;
pub const BLUE_SHULKER_BOX: i32 = 668;
pub const BROWN_SHULKER_BOX: i32 = 669;
pub const GREEN_SHULKER_BOX: i32 = 670;
pub const RED_SHULKER_BOX: i32 = 671;
pub const BLACK_SHULKER_BOX: i32 = 672;
pub const WHITE_GLAZED_TERRACOTTA: i32 = 673;
pub const ORANGE_GLAZED_TERRACOTTA: i32 = 674;
pub const MAGENTA_GLAZED_TERRACOTTA: i32 = 675;
pub const LIGHT_BLUE_GLAZED_TERRACOTTA: i32 = 676;
pub const YELLOW_GLAZED_TERRACOTTA: i32 = 677;
pub const LIME_GLAZED_TERRACOTTA: i32 = 678;
pub const PINK_GLAZED_TERRACOTTA: i32 = 679;
pub const GRAY_GLAZED_TERRACOTTA: i32 = 680;
pub const LIGHT_GRAY_GLAZED_TERRACOTTA: i32 = 681;
pub const CYAN_GLAZED_TERRACOTTA: i32 = 682;
pub const PURPLE_GLAZED_TERRACOTTA: i32 = 683;
pub const BLUE_GLAZED_TERRACOTTA: i32 = 684;
pub const BROWN_GLAZED_TERRACOTTA: i32 = 685;
pub const GREEN_GLAZED_TERRACOTTA: i32 = 686;
pub const RED_GLAZED_TERRACOTTA: i32 = 687;
pub const BLACK_GLAZED_TERRACOTTA: i32 = 688;
pub const WHITE_CONCRETE: i32 = 689;
pub const ORANGE_CONCRETE: i32 = 690;
pub const MAGENTA_CONCRETE: i32 = 691;
pub const LIGHT_BLUE_CONCRETE: i32 = 692;
pub const YELLOW_CONCRETE: i32 = 693;
pub const LIME_CONCRETE: i32 = 694;
pub const PINK_CONCRETE: i32 = 695;
pub const GRAY_CONCRETE: i32 = 696;
pub const LIGHT_GRAY_CONCRETE: i32 = 697;
pub const CYAN_CONCRETE: i32 = 698;
pub const PURPLE_CONCRETE: i32 = 699;
pub const BLUE_CONCRETE: i32 = 700;
pub const BROWN_CONCRETE: i32 = 701;
pub const GREEN_CONCRETE: i32 = 702;
pub const RED_CONCRETE: i32 = 703;
pub const BLACK_CONCRETE: i32 = 704;
pub const WHITE_CONCRETE_STAIRS: i32 = 705;
pub const ORANGE_CONCRETE_STAIRS: i32 = 706;
pub const MAGENTA_CONCRETE_STAIRS: i32 = 707;
pub const LIGHT_BLUE_CONCRETE_STAIRS: i32 = 708;
pub const YELLOW_CONCRETE_STAIRS: i32 = 709;
pub const LIME_CONCRETE_STAIRS: i32 = 710;
pub const PINK_CONCRETE_STAIRS: i32 = 711;
pub const GRAY_CONCRETE_STAIRS: i32 = 712;
pub const LIGHT_GRAY_CONCRETE_STAIRS: i32 = 713;
pub const CYAN_CONCRETE_STAIRS: i32 = 714;
pub const PURPLE_CONCRETE_STAIRS: i32 = 715;
pub const BLUE_CONCRETE_STAIRS: i32 = 716;
pub const BROWN_CONCRETE_STAIRS: i32 = 717;
pub const GREEN_CONCRETE_STAIRS: i32 = 718;
pub const RED_CONCRETE_STAIRS: i32 = 719;
pub const BLACK_CONCRETE_STAIRS: i32 = 720;
pub const WHITE_CONCRETE_SLAB: i32 = 721;
pub const ORANGE_CONCRETE_SLAB: i32 = 722;
pub const MAGENTA_CONCRETE_SLAB: i32 = 723;
pub const LIGHT_BLUE_CONCRETE_SLAB: i32 = 724;
pub const YELLOW_CONCRETE_SLAB: i32 = 725;
pub const LIME_CONCRETE_SLAB: i32 = 726;
pub const PINK_CONCRETE_SLAB: i32 = 727;
pub const GRAY_CONCRETE_SLAB: i32 = 728;
pub const LIGHT_GRAY_CONCRETE_SLAB: i32 = 729;
pub const CYAN_CONCRETE_SLAB: i32 = 730;
pub const PURPLE_CONCRETE_SLAB: i32 = 731;
pub const BLUE_CONCRETE_SLAB: i32 = 732;
pub const BROWN_CONCRETE_SLAB: i32 = 733;
pub const GREEN_CONCRETE_SLAB: i32 = 734;
pub const RED_CONCRETE_SLAB: i32 = 735;
pub const BLACK_CONCRETE_SLAB: i32 = 736;
pub const WHITE_CONCRETE_POWDER: i32 = 737;
pub const ORANGE_CONCRETE_POWDER: i32 = 738;
pub const MAGENTA_CONCRETE_POWDER: i32 = 739;
pub const LIGHT_BLUE_CONCRETE_POWDER: i32 = 740;
pub const YELLOW_CONCRETE_POWDER: i32 = 741;
pub const LIME_CONCRETE_POWDER: i32 = 742;
pub const PINK_CONCRETE_POWDER: i32 = 743;
pub const GRAY_CONCRETE_POWDER: i32 = 744;
pub const LIGHT_GRAY_CONCRETE_POWDER: i32 = 745;
pub const CYAN_CONCRETE_POWDER: i32 = 746;
pub const PURPLE_CONCRETE_POWDER: i32 = 747;
pub const BLUE_CONCRETE_POWDER: i32 = 748;
pub const BROWN_CONCRETE_POWDER: i32 = 749;
pub const GREEN_CONCRETE_POWDER: i32 = 750;
pub const RED_CONCRETE_POWDER: i32 = 751;
pub const BLACK_CONCRETE_POWDER: i32 = 752;
pub const TURTLE_EGG: i32 = 753;
pub const SNIFFER_EGG: i32 = 754;
pub const DRIED_GHAST: i32 = 755;
pub const DEAD_TUBE_CORAL_BLOCK: i32 = 756;
pub const DEAD_BRAIN_CORAL_BLOCK: i32 = 757;
pub const DEAD_BUBBLE_CORAL_BLOCK: i32 = 758;
pub const DEAD_FIRE_CORAL_BLOCK: i32 = 759;
pub const DEAD_HORN_CORAL_BLOCK: i32 = 760;
pub const TUBE_CORAL_BLOCK: i32 = 761;
pub const BRAIN_CORAL_BLOCK: i32 = 762;
pub const BUBBLE_CORAL_BLOCK: i32 = 763;
pub const FIRE_CORAL_BLOCK: i32 = 764;
pub const HORN_CORAL_BLOCK: i32 = 765;
pub const TUBE_CORAL: i32 = 766;
pub const BRAIN_CORAL: i32 = 767;
pub const BUBBLE_CORAL: i32 = 768;
pub const FIRE_CORAL: i32 = 769;
pub const HORN_CORAL: i32 = 770;
pub const DEAD_BRAIN_CORAL: i32 = 771;
pub const DEAD_BUBBLE_CORAL: i32 = 772;
pub const DEAD_FIRE_CORAL: i32 = 773;
pub const DEAD_HORN_CORAL: i32 = 774;
pub const DEAD_TUBE_CORAL: i32 = 775;
pub const TUBE_CORAL_FAN: i32 = 776;
pub const BRAIN_CORAL_FAN: i32 = 777;
pub const BUBBLE_CORAL_FAN: i32 = 778;
pub const FIRE_CORAL_FAN: i32 = 779;
pub const HORN_CORAL_FAN: i32 = 780;
pub const DEAD_TUBE_CORAL_FAN: i32 = 781;
pub const DEAD_BRAIN_CORAL_FAN: i32 = 782;
pub const DEAD_BUBBLE_CORAL_FAN: i32 = 783;
pub const DEAD_FIRE_CORAL_FAN: i32 = 784;
pub const DEAD_HORN_CORAL_FAN: i32 = 785;
pub const BLUE_ICE: i32 = 786;
pub const CONDUIT: i32 = 787;
pub const POLISHED_GRANITE_STAIRS: i32 = 788;
pub const SMOOTH_RED_SANDSTONE_STAIRS: i32 = 789;
pub const MOSSY_STONE_BRICK_STAIRS: i32 = 790;
pub const POLISHED_DIORITE_STAIRS: i32 = 791;
pub const MOSSY_COBBLESTONE_STAIRS: i32 = 792;
pub const END_STONE_BRICK_STAIRS: i32 = 793;
pub const STONE_STAIRS: i32 = 794;
pub const SMOOTH_SANDSTONE_STAIRS: i32 = 795;
pub const SMOOTH_QUARTZ_STAIRS: i32 = 796;
pub const GRANITE_STAIRS: i32 = 797;
pub const ANDESITE_STAIRS: i32 = 798;
pub const RED_NETHER_BRICK_STAIRS: i32 = 799;
pub const POLISHED_ANDESITE_STAIRS: i32 = 800;
pub const DIORITE_STAIRS: i32 = 801;
pub const COBBLED_DEEPSLATE_STAIRS: i32 = 802;
pub const POLISHED_DEEPSLATE_STAIRS: i32 = 803;
pub const DEEPSLATE_BRICK_STAIRS: i32 = 804;
pub const DEEPSLATE_TILE_STAIRS: i32 = 805;
pub const POLISHED_GRANITE_SLAB: i32 = 806;
pub const SMOOTH_RED_SANDSTONE_SLAB: i32 = 807;
pub const MOSSY_STONE_BRICK_SLAB: i32 = 808;
pub const POLISHED_DIORITE_SLAB: i32 = 809;
pub const MOSSY_COBBLESTONE_SLAB: i32 = 810;
pub const END_STONE_BRICK_SLAB: i32 = 811;
pub const SMOOTH_SANDSTONE_SLAB: i32 = 812;
pub const SMOOTH_QUARTZ_SLAB: i32 = 813;
pub const GRANITE_SLAB: i32 = 814;
pub const ANDESITE_SLAB: i32 = 815;
pub const RED_NETHER_BRICK_SLAB: i32 = 816;
pub const POLISHED_ANDESITE_SLAB: i32 = 817;
pub const DIORITE_SLAB: i32 = 818;
pub const COBBLED_DEEPSLATE_SLAB: i32 = 819;
pub const POLISHED_DEEPSLATE_SLAB: i32 = 820;
pub const DEEPSLATE_BRICK_SLAB: i32 = 821;
pub const DEEPSLATE_TILE_SLAB: i32 = 822;
pub const SCAFFOLDING: i32 = 823;
pub const REDSTONE: i32 = 824;
pub const REDSTONE_TORCH: i32 = 825;
pub const REDSTONE_BLOCK: i32 = 826;
pub const REPEATER: i32 = 827;
pub const COMPARATOR: i32 = 828;
pub const PISTON: i32 = 829;
pub const STICKY_PISTON: i32 = 830;
pub const SLIME_BLOCK: i32 = 831;
pub const HONEY_BLOCK: i32 = 832;
pub const OBSERVER: i32 = 833;
pub const HOPPER: i32 = 834;
pub const DISPENSER: i32 = 835;
pub const DROPPER: i32 = 836;
pub const LECTERN: i32 = 837;
pub const TARGET: i32 = 838;
pub const LEVER: i32 = 839;
pub const LIGHTNING_ROD: i32 = 840;
pub const EXPOSED_LIGHTNING_ROD: i32 = 841;
pub const WEATHERED_LIGHTNING_ROD: i32 = 842;
pub const OXIDIZED_LIGHTNING_ROD: i32 = 843;
pub const WAXED_LIGHTNING_ROD: i32 = 844;
pub const WAXED_EXPOSED_LIGHTNING_ROD: i32 = 845;
pub const WAXED_WEATHERED_LIGHTNING_ROD: i32 = 846;
pub const WAXED_OXIDIZED_LIGHTNING_ROD: i32 = 847;
pub const DAYLIGHT_DETECTOR: i32 = 848;
pub const SCULK_SENSOR: i32 = 849;
pub const CALIBRATED_SCULK_SENSOR: i32 = 850;
pub const TRIPWIRE_HOOK: i32 = 851;
pub const TRAPPED_CHEST: i32 = 852;
pub const TNT: i32 = 853;
pub const REDSTONE_LAMP: i32 = 854;
pub const NOTE_BLOCK: i32 = 855;
pub const STONE_BUTTON: i32 = 856;
pub const POLISHED_BLACKSTONE_BUTTON: i32 = 857;
pub const OAK_BUTTON: i32 = 858;
pub const SPRUCE_BUTTON: i32 = 859;
pub const BIRCH_BUTTON: i32 = 860;
pub const JUNGLE_BUTTON: i32 = 861;
pub const ACACIA_BUTTON: i32 = 862;
pub const CHERRY_BUTTON: i32 = 863;
pub const DARK_OAK_BUTTON: i32 = 864;
pub const PALE_OAK_BUTTON: i32 = 865;
pub const MANGROVE_BUTTON: i32 = 866;
pub const POPLAR_BUTTON: i32 = 867;
pub const BAMBOO_BUTTON: i32 = 868;
pub const CRIMSON_BUTTON: i32 = 869;
pub const WARPED_BUTTON: i32 = 870;
pub const STONE_PRESSURE_PLATE: i32 = 871;
pub const POLISHED_BLACKSTONE_PRESSURE_PLATE: i32 = 872;
pub const LIGHT_WEIGHTED_PRESSURE_PLATE: i32 = 873;
pub const HEAVY_WEIGHTED_PRESSURE_PLATE: i32 = 874;
pub const OAK_PRESSURE_PLATE: i32 = 875;
pub const SPRUCE_PRESSURE_PLATE: i32 = 876;
pub const BIRCH_PRESSURE_PLATE: i32 = 877;
pub const JUNGLE_PRESSURE_PLATE: i32 = 878;
pub const ACACIA_PRESSURE_PLATE: i32 = 879;
pub const CHERRY_PRESSURE_PLATE: i32 = 880;
pub const DARK_OAK_PRESSURE_PLATE: i32 = 881;
pub const PALE_OAK_PRESSURE_PLATE: i32 = 882;
pub const MANGROVE_PRESSURE_PLATE: i32 = 883;
pub const POPLAR_PRESSURE_PLATE: i32 = 884;
pub const BAMBOO_PRESSURE_PLATE: i32 = 885;
pub const CRIMSON_PRESSURE_PLATE: i32 = 886;
pub const WARPED_PRESSURE_PLATE: i32 = 887;
pub const IRON_DOOR: i32 = 888;
pub const OAK_DOOR: i32 = 889;
pub const SPRUCE_DOOR: i32 = 890;
pub const BIRCH_DOOR: i32 = 891;
pub const JUNGLE_DOOR: i32 = 892;
pub const ACACIA_DOOR: i32 = 893;
pub const CHERRY_DOOR: i32 = 894;
pub const DARK_OAK_DOOR: i32 = 895;
pub const PALE_OAK_DOOR: i32 = 896;
pub const MANGROVE_DOOR: i32 = 897;
pub const POPLAR_DOOR: i32 = 898;
pub const BAMBOO_DOOR: i32 = 899;
pub const CRIMSON_DOOR: i32 = 900;
pub const WARPED_DOOR: i32 = 901;
pub const COPPER_DOOR: i32 = 902;
pub const EXPOSED_COPPER_DOOR: i32 = 903;
pub const WEATHERED_COPPER_DOOR: i32 = 904;
pub const OXIDIZED_COPPER_DOOR: i32 = 905;
pub const WAXED_COPPER_DOOR: i32 = 906;
pub const WAXED_EXPOSED_COPPER_DOOR: i32 = 907;
pub const WAXED_WEATHERED_COPPER_DOOR: i32 = 908;
pub const WAXED_OXIDIZED_COPPER_DOOR: i32 = 909;
pub const IRON_TRAPDOOR: i32 = 910;
pub const OAK_TRAPDOOR: i32 = 911;
pub const SPRUCE_TRAPDOOR: i32 = 912;
pub const BIRCH_TRAPDOOR: i32 = 913;
pub const JUNGLE_TRAPDOOR: i32 = 914;
pub const ACACIA_TRAPDOOR: i32 = 915;
pub const CHERRY_TRAPDOOR: i32 = 916;
pub const DARK_OAK_TRAPDOOR: i32 = 917;
pub const PALE_OAK_TRAPDOOR: i32 = 918;
pub const MANGROVE_TRAPDOOR: i32 = 919;
pub const POPLAR_TRAPDOOR: i32 = 920;
pub const BAMBOO_TRAPDOOR: i32 = 921;
pub const CRIMSON_TRAPDOOR: i32 = 922;
pub const WARPED_TRAPDOOR: i32 = 923;
pub const COPPER_TRAPDOOR: i32 = 924;
pub const EXPOSED_COPPER_TRAPDOOR: i32 = 925;
pub const WEATHERED_COPPER_TRAPDOOR: i32 = 926;
pub const OXIDIZED_COPPER_TRAPDOOR: i32 = 927;
pub const WAXED_COPPER_TRAPDOOR: i32 = 928;
pub const WAXED_EXPOSED_COPPER_TRAPDOOR: i32 = 929;
pub const WAXED_WEATHERED_COPPER_TRAPDOOR: i32 = 930;
pub const WAXED_OXIDIZED_COPPER_TRAPDOOR: i32 = 931;
pub const OAK_FENCE_GATE: i32 = 932;
pub const SPRUCE_FENCE_GATE: i32 = 933;
pub const BIRCH_FENCE_GATE: i32 = 934;
pub const JUNGLE_FENCE_GATE: i32 = 935;
pub const ACACIA_FENCE_GATE: i32 = 936;
pub const CHERRY_FENCE_GATE: i32 = 937;
pub const DARK_OAK_FENCE_GATE: i32 = 938;
pub const PALE_OAK_FENCE_GATE: i32 = 939;
pub const MANGROVE_FENCE_GATE: i32 = 940;
pub const POPLAR_FENCE_GATE: i32 = 941;
pub const BAMBOO_FENCE_GATE: i32 = 942;
pub const CRIMSON_FENCE_GATE: i32 = 943;
pub const WARPED_FENCE_GATE: i32 = 944;
pub const POWERED_RAIL: i32 = 945;
pub const DETECTOR_RAIL: i32 = 946;
pub const RAIL: i32 = 947;
pub const ACTIVATOR_RAIL: i32 = 948;
pub const SADDLE: i32 = 949;
pub const WHITE_HARNESS: i32 = 950;
pub const ORANGE_HARNESS: i32 = 951;
pub const MAGENTA_HARNESS: i32 = 952;
pub const LIGHT_BLUE_HARNESS: i32 = 953;
pub const YELLOW_HARNESS: i32 = 954;
pub const LIME_HARNESS: i32 = 955;
pub const PINK_HARNESS: i32 = 956;
pub const GRAY_HARNESS: i32 = 957;
pub const LIGHT_GRAY_HARNESS: i32 = 958;
pub const CYAN_HARNESS: i32 = 959;
pub const PURPLE_HARNESS: i32 = 960;
pub const BLUE_HARNESS: i32 = 961;
pub const BROWN_HARNESS: i32 = 962;
pub const GREEN_HARNESS: i32 = 963;
pub const RED_HARNESS: i32 = 964;
pub const BLACK_HARNESS: i32 = 965;
pub const MINECART: i32 = 966;
pub const CHEST_MINECART: i32 = 967;
pub const FURNACE_MINECART: i32 = 968;
pub const TNT_MINECART: i32 = 969;
pub const HOPPER_MINECART: i32 = 970;
pub const CARROT_ON_A_STICK: i32 = 971;
pub const WARPED_FUNGUS_ON_A_STICK: i32 = 972;
pub const PHANTOM_MEMBRANE: i32 = 973;
pub const ELYTRA: i32 = 974;
pub const OAK_BOAT: i32 = 975;
pub const OAK_CHEST_BOAT: i32 = 976;
pub const SPRUCE_BOAT: i32 = 977;
pub const SPRUCE_CHEST_BOAT: i32 = 978;
pub const BIRCH_BOAT: i32 = 979;
pub const BIRCH_CHEST_BOAT: i32 = 980;
pub const JUNGLE_BOAT: i32 = 981;
pub const JUNGLE_CHEST_BOAT: i32 = 982;
pub const ACACIA_BOAT: i32 = 983;
pub const ACACIA_CHEST_BOAT: i32 = 984;
pub const CHERRY_BOAT: i32 = 985;
pub const CHERRY_CHEST_BOAT: i32 = 986;
pub const DARK_OAK_BOAT: i32 = 987;
pub const DARK_OAK_CHEST_BOAT: i32 = 988;
pub const PALE_OAK_BOAT: i32 = 989;
pub const PALE_OAK_CHEST_BOAT: i32 = 990;
pub const MANGROVE_BOAT: i32 = 991;
pub const MANGROVE_CHEST_BOAT: i32 = 992;
pub const POPLAR_BOAT: i32 = 993;
pub const POPLAR_CHEST_BOAT: i32 = 994;
pub const BAMBOO_RAFT: i32 = 995;
pub const BAMBOO_CHEST_RAFT: i32 = 996;
pub const STRUCTURE_BLOCK: i32 = 997;
pub const JIGSAW: i32 = 998;
pub const TEST_BLOCK: i32 = 999;
pub const TEST_INSTANCE_BLOCK: i32 = 1000;
pub const TURTLE_HELMET: i32 = 1001;
pub const TURTLE_SCUTE: i32 = 1002;
pub const ARMADILLO_SCUTE: i32 = 1003;
pub const WOLF_ARMOR: i32 = 1004;
pub const FLINT_AND_STEEL: i32 = 1005;
pub const BOWL: i32 = 1006;
pub const APPLE: i32 = 1007;
pub const BOW: i32 = 1008;
pub const ARROW: i32 = 1009;
pub const COAL: i32 = 1010;
pub const CHARCOAL: i32 = 1011;
pub const DIAMOND: i32 = 1012;
pub const EMERALD: i32 = 1013;
pub const LAPIS_LAZULI: i32 = 1014;
pub const QUARTZ: i32 = 1015;
pub const AMETHYST_SHARD: i32 = 1016;
pub const RAW_IRON: i32 = 1017;
pub const IRON_INGOT: i32 = 1018;
pub const RAW_COPPER: i32 = 1019;
pub const COPPER_INGOT: i32 = 1020;
pub const RAW_GOLD: i32 = 1021;
pub const GOLD_INGOT: i32 = 1022;
pub const NETHERITE_INGOT: i32 = 1023;
pub const NETHERITE_SCRAP: i32 = 1024;
pub const WOODEN_SWORD: i32 = 1025;
pub const WOODEN_SHOVEL: i32 = 1026;
pub const WOODEN_PICKAXE: i32 = 1027;
pub const WOODEN_AXE: i32 = 1028;
pub const WOODEN_HOE: i32 = 1029;
pub const COPPER_SWORD: i32 = 1030;
pub const COPPER_SHOVEL: i32 = 1031;
pub const COPPER_PICKAXE: i32 = 1032;
pub const COPPER_AXE: i32 = 1033;
pub const COPPER_HOE: i32 = 1034;
pub const STONE_SWORD: i32 = 1035;
pub const STONE_SHOVEL: i32 = 1036;
pub const STONE_PICKAXE: i32 = 1037;
pub const STONE_AXE: i32 = 1038;
pub const STONE_HOE: i32 = 1039;
pub const GOLDEN_SWORD: i32 = 1040;
pub const GOLDEN_SHOVEL: i32 = 1041;
pub const GOLDEN_PICKAXE: i32 = 1042;
pub const GOLDEN_AXE: i32 = 1043;
pub const GOLDEN_HOE: i32 = 1044;
pub const IRON_SWORD: i32 = 1045;
pub const IRON_SHOVEL: i32 = 1046;
pub const IRON_PICKAXE: i32 = 1047;
pub const IRON_AXE: i32 = 1048;
pub const IRON_HOE: i32 = 1049;
pub const DIAMOND_SWORD: i32 = 1050;
pub const DIAMOND_SHOVEL: i32 = 1051;
pub const DIAMOND_PICKAXE: i32 = 1052;
pub const DIAMOND_AXE: i32 = 1053;
pub const DIAMOND_HOE: i32 = 1054;
pub const NETHERITE_SWORD: i32 = 1055;
pub const NETHERITE_SHOVEL: i32 = 1056;
pub const NETHERITE_PICKAXE: i32 = 1057;
pub const NETHERITE_AXE: i32 = 1058;
pub const NETHERITE_HOE: i32 = 1059;
pub const STICK: i32 = 1060;
pub const MUSHROOM_STEW: i32 = 1061;
pub const STRING: i32 = 1062;
pub const FEATHER: i32 = 1063;
pub const GUNPOWDER: i32 = 1064;
pub const WHEAT_SEEDS: i32 = 1065;
pub const WHEAT: i32 = 1066;
pub const BREAD: i32 = 1067;
pub const LEATHER_HELMET: i32 = 1068;
pub const LEATHER_CHESTPLATE: i32 = 1069;
pub const LEATHER_LEGGINGS: i32 = 1070;
pub const LEATHER_BOOTS: i32 = 1071;
pub const COPPER_HELMET: i32 = 1072;
pub const COPPER_CHESTPLATE: i32 = 1073;
pub const COPPER_LEGGINGS: i32 = 1074;
pub const COPPER_BOOTS: i32 = 1075;
pub const CHAINMAIL_HELMET: i32 = 1076;
pub const CHAINMAIL_CHESTPLATE: i32 = 1077;
pub const CHAINMAIL_LEGGINGS: i32 = 1078;
pub const CHAINMAIL_BOOTS: i32 = 1079;
pub const IRON_HELMET: i32 = 1080;
pub const IRON_CHESTPLATE: i32 = 1081;
pub const IRON_LEGGINGS: i32 = 1082;
pub const IRON_BOOTS: i32 = 1083;
pub const DIAMOND_HELMET: i32 = 1084;
pub const DIAMOND_CHESTPLATE: i32 = 1085;
pub const DIAMOND_LEGGINGS: i32 = 1086;
pub const DIAMOND_BOOTS: i32 = 1087;
pub const GOLDEN_HELMET: i32 = 1088;
pub const GOLDEN_CHESTPLATE: i32 = 1089;
pub const GOLDEN_LEGGINGS: i32 = 1090;
pub const GOLDEN_BOOTS: i32 = 1091;
pub const NETHERITE_HELMET: i32 = 1092;
pub const NETHERITE_CHESTPLATE: i32 = 1093;
pub const NETHERITE_LEGGINGS: i32 = 1094;
pub const NETHERITE_BOOTS: i32 = 1095;
pub const FLINT: i32 = 1096;
pub const PORKCHOP: i32 = 1097;
pub const COOKED_PORKCHOP: i32 = 1098;
pub const PAINTING: i32 = 1099;
pub const GOLDEN_APPLE: i32 = 1100;
pub const ENCHANTED_GOLDEN_APPLE: i32 = 1101;
pub const OAK_SIGN: i32 = 1102;
pub const SPRUCE_SIGN: i32 = 1103;
pub const BIRCH_SIGN: i32 = 1104;
pub const JUNGLE_SIGN: i32 = 1105;
pub const ACACIA_SIGN: i32 = 1106;
pub const CHERRY_SIGN: i32 = 1107;
pub const DARK_OAK_SIGN: i32 = 1108;
pub const PALE_OAK_SIGN: i32 = 1109;
pub const MANGROVE_SIGN: i32 = 1110;
pub const POPLAR_SIGN: i32 = 1111;
pub const BAMBOO_SIGN: i32 = 1112;
pub const CRIMSON_SIGN: i32 = 1113;
pub const WARPED_SIGN: i32 = 1114;
pub const OAK_HANGING_SIGN: i32 = 1115;
pub const SPRUCE_HANGING_SIGN: i32 = 1116;
pub const BIRCH_HANGING_SIGN: i32 = 1117;
pub const JUNGLE_HANGING_SIGN: i32 = 1118;
pub const ACACIA_HANGING_SIGN: i32 = 1119;
pub const CHERRY_HANGING_SIGN: i32 = 1120;
pub const DARK_OAK_HANGING_SIGN: i32 = 1121;
pub const PALE_OAK_HANGING_SIGN: i32 = 1122;
pub const MANGROVE_HANGING_SIGN: i32 = 1123;
pub const POPLAR_HANGING_SIGN: i32 = 1124;
pub const BAMBOO_HANGING_SIGN: i32 = 1125;
pub const CRIMSON_HANGING_SIGN: i32 = 1126;
pub const WARPED_HANGING_SIGN: i32 = 1127;
pub const BUCKET: i32 = 1128;
pub const WATER_BUCKET: i32 = 1129;
pub const LAVA_BUCKET: i32 = 1130;
pub const POWDER_SNOW_BUCKET: i32 = 1131;
pub const SNOWBALL: i32 = 1132;
pub const LEATHER: i32 = 1133;
pub const MILK_BUCKET: i32 = 1134;
pub const PUFFERFISH_BUCKET: i32 = 1135;
pub const SALMON_BUCKET: i32 = 1136;
pub const COD_BUCKET: i32 = 1137;
pub const TROPICAL_FISH_BUCKET: i32 = 1138;
pub const AXOLOTL_BUCKET: i32 = 1139;
pub const SULFUR_CUBE_BUCKET: i32 = 1140;
pub const TADPOLE_BUCKET: i32 = 1141;
pub const BRICK: i32 = 1142;
pub const CLAY_BALL: i32 = 1143;
pub const DRIED_KELP_BLOCK: i32 = 1144;
pub const PAPER: i32 = 1145;
pub const BOOK: i32 = 1146;
pub const SLIME_BALL: i32 = 1147;
pub const EGG: i32 = 1148;
pub const BLUE_EGG: i32 = 1149;
pub const BROWN_EGG: i32 = 1150;
pub const COMPASS: i32 = 1151;
pub const RECOVERY_COMPASS: i32 = 1152;
pub const BUNDLE: i32 = 1153;
pub const WHITE_BUNDLE: i32 = 1154;
pub const ORANGE_BUNDLE: i32 = 1155;
pub const MAGENTA_BUNDLE: i32 = 1156;
pub const LIGHT_BLUE_BUNDLE: i32 = 1157;
pub const YELLOW_BUNDLE: i32 = 1158;
pub const LIME_BUNDLE: i32 = 1159;
pub const PINK_BUNDLE: i32 = 1160;
pub const GRAY_BUNDLE: i32 = 1161;
pub const LIGHT_GRAY_BUNDLE: i32 = 1162;
pub const CYAN_BUNDLE: i32 = 1163;
pub const PURPLE_BUNDLE: i32 = 1164;
pub const BLUE_BUNDLE: i32 = 1165;
pub const BROWN_BUNDLE: i32 = 1166;
pub const GREEN_BUNDLE: i32 = 1167;
pub const RED_BUNDLE: i32 = 1168;
pub const BLACK_BUNDLE: i32 = 1169;
pub const WHITE_CUSHION: i32 = 1170;
pub const ORANGE_CUSHION: i32 = 1171;
pub const MAGENTA_CUSHION: i32 = 1172;
pub const LIGHT_BLUE_CUSHION: i32 = 1173;
pub const YELLOW_CUSHION: i32 = 1174;
pub const LIME_CUSHION: i32 = 1175;
pub const PINK_CUSHION: i32 = 1176;
pub const GRAY_CUSHION: i32 = 1177;
pub const LIGHT_GRAY_CUSHION: i32 = 1178;
pub const CYAN_CUSHION: i32 = 1179;
pub const PURPLE_CUSHION: i32 = 1180;
pub const BLUE_CUSHION: i32 = 1181;
pub const BROWN_CUSHION: i32 = 1182;
pub const GREEN_CUSHION: i32 = 1183;
pub const RED_CUSHION: i32 = 1184;
pub const BLACK_CUSHION: i32 = 1185;
pub const FISHING_ROD: i32 = 1186;
pub const CLOCK: i32 = 1187;
pub const SPYGLASS: i32 = 1188;
pub const GLOWSTONE_DUST: i32 = 1189;
pub const COD: i32 = 1190;
pub const SALMON: i32 = 1191;
pub const TROPICAL_FISH: i32 = 1192;
pub const PUFFERFISH: i32 = 1193;
pub const COOKED_COD: i32 = 1194;
pub const COOKED_SALMON: i32 = 1195;
pub const INK_SAC: i32 = 1196;
pub const GLOW_INK_SAC: i32 = 1197;
pub const COCOA_BEANS: i32 = 1198;
pub const WHITE_DYE: i32 = 1199;
pub const ORANGE_DYE: i32 = 1200;
pub const MAGENTA_DYE: i32 = 1201;
pub const LIGHT_BLUE_DYE: i32 = 1202;
pub const YELLOW_DYE: i32 = 1203;
pub const LIME_DYE: i32 = 1204;
pub const PINK_DYE: i32 = 1205;
pub const GRAY_DYE: i32 = 1206;
pub const LIGHT_GRAY_DYE: i32 = 1207;
pub const CYAN_DYE: i32 = 1208;
pub const PURPLE_DYE: i32 = 1209;
pub const BLUE_DYE: i32 = 1210;
pub const BROWN_DYE: i32 = 1211;
pub const GREEN_DYE: i32 = 1212;
pub const RED_DYE: i32 = 1213;
pub const BLACK_DYE: i32 = 1214;
pub const BONE_MEAL: i32 = 1215;
pub const BONE: i32 = 1216;
pub const SUGAR: i32 = 1217;
pub const CAKE: i32 = 1218;
pub const WHITE_BED: i32 = 1219;
pub const ORANGE_BED: i32 = 1220;
pub const MAGENTA_BED: i32 = 1221;
pub const LIGHT_BLUE_BED: i32 = 1222;
pub const YELLOW_BED: i32 = 1223;
pub const LIME_BED: i32 = 1224;
pub const PINK_BED: i32 = 1225;
pub const GRAY_BED: i32 = 1226;
pub const LIGHT_GRAY_BED: i32 = 1227;
pub const CYAN_BED: i32 = 1228;
pub const PURPLE_BED: i32 = 1229;
pub const BLUE_BED: i32 = 1230;
pub const BROWN_BED: i32 = 1231;
pub const GREEN_BED: i32 = 1232;
pub const RED_BED: i32 = 1233;
pub const BLACK_BED: i32 = 1234;
pub const STRAW_BED: i32 = 1235;
pub const COOKIE: i32 = 1236;
pub const CRAFTER: i32 = 1237;
pub const FILLED_MAP: i32 = 1238;
pub const OCEAN_MONUMENT_MAP: i32 = 1239;
pub const WOODLAND_MANSION_MAP: i32 = 1240;
pub const BURIED_TRIAL_CHAMBERS_MAP: i32 = 1241;
pub const JUNGLE_PYRAMID_MAP: i32 = 1242;
pub const SWAMP_HUT_MAP: i32 = 1243;
pub const DESERT_VILLAGE_MAP: i32 = 1244;
pub const PLAINS_VILLAGE_MAP: i32 = 1245;
pub const SAVANNA_VILLAGE_MAP: i32 = 1246;
pub const SNOWY_VILLAGE_MAP: i32 = 1247;
pub const TAIGA_VILLAGE_MAP: i32 = 1248;
pub const BURIED_TREASURE_MAP: i32 = 1249;
pub const BURIED_ANCIENT_CITY_MAP: i32 = 1250;
pub const BURIED_MINESHAFT_MAP: i32 = 1251;
pub const DESERT_PYRAMID_MAP: i32 = 1252;
pub const ABANDONED_CAMP_MAP: i32 = 1253;
pub const WARM_OCEAN_RUINS_MAP: i32 = 1254;
pub const SHEARS: i32 = 1255;
pub const MELON_SLICE: i32 = 1256;
pub const DRIED_KELP: i32 = 1257;
pub const PUMPKIN_SEEDS: i32 = 1258;
pub const MELON_SEEDS: i32 = 1259;
pub const BEEF: i32 = 1260;
pub const COOKED_BEEF: i32 = 1261;
pub const CHICKEN: i32 = 1262;
pub const COOKED_CHICKEN: i32 = 1263;
pub const ROTTEN_FLESH: i32 = 1264;
pub const ENDER_PEARL: i32 = 1265;
pub const BLAZE_ROD: i32 = 1266;
pub const GHAST_TEAR: i32 = 1267;
pub const GOLD_NUGGET: i32 = 1268;
pub const NETHER_WART: i32 = 1269;
pub const GLASS_BOTTLE: i32 = 1270;
pub const POTION: i32 = 1271;
pub const SPIDER_EYE: i32 = 1272;
pub const FERMENTED_SPIDER_EYE: i32 = 1273;
pub const BLAZE_POWDER: i32 = 1274;
pub const MAGMA_CREAM: i32 = 1275;
pub const BREWING_STAND: i32 = 1276;
pub const CAULDRON: i32 = 1277;
pub const ENDER_EYE: i32 = 1278;
pub const GLISTERING_MELON_SLICE: i32 = 1279;
pub const CHICKEN_SPAWN_EGG: i32 = 1280;
pub const COW_SPAWN_EGG: i32 = 1281;
pub const PIG_SPAWN_EGG: i32 = 1282;
pub const SHEEP_SPAWN_EGG: i32 = 1283;
pub const CAMEL_SPAWN_EGG: i32 = 1284;
pub const DONKEY_SPAWN_EGG: i32 = 1285;
pub const HORSE_SPAWN_EGG: i32 = 1286;
pub const MULE_SPAWN_EGG: i32 = 1287;
pub const CAT_SPAWN_EGG: i32 = 1288;
pub const PARROT_SPAWN_EGG: i32 = 1289;
pub const WOLF_SPAWN_EGG: i32 = 1290;
pub const ARMADILLO_SPAWN_EGG: i32 = 1291;
pub const BAT_SPAWN_EGG: i32 = 1292;
pub const BEE_SPAWN_EGG: i32 = 1293;
pub const FOX_SPAWN_EGG: i32 = 1294;
pub const GOAT_SPAWN_EGG: i32 = 1295;
pub const LLAMA_SPAWN_EGG: i32 = 1296;
pub const OCELOT_SPAWN_EGG: i32 = 1297;
pub const PANDA_SPAWN_EGG: i32 = 1298;
pub const POLAR_BEAR_SPAWN_EGG: i32 = 1299;
pub const RABBIT_SPAWN_EGG: i32 = 1300;
pub const AXOLOTL_SPAWN_EGG: i32 = 1301;
pub const COD_SPAWN_EGG: i32 = 1302;
pub const DOLPHIN_SPAWN_EGG: i32 = 1303;
pub const FROG_SPAWN_EGG: i32 = 1304;
pub const GLOW_SQUID_SPAWN_EGG: i32 = 1305;
pub const NAUTILUS_SPAWN_EGG: i32 = 1306;
pub const PUFFERFISH_SPAWN_EGG: i32 = 1307;
pub const SALMON_SPAWN_EGG: i32 = 1308;
pub const SQUID_SPAWN_EGG: i32 = 1309;
pub const TADPOLE_SPAWN_EGG: i32 = 1310;
pub const TROPICAL_FISH_SPAWN_EGG: i32 = 1311;
pub const TURTLE_SPAWN_EGG: i32 = 1312;
pub const ALLAY_SPAWN_EGG: i32 = 1313;
pub const MOOSHROOM_SPAWN_EGG: i32 = 1314;
pub const SNIFFER_SPAWN_EGG: i32 = 1315;
pub const SULFUR_CUBE_SPAWN_EGG: i32 = 1316;
pub const COPPER_GOLEM_SPAWN_EGG: i32 = 1317;
pub const IRON_GOLEM_SPAWN_EGG: i32 = 1318;
pub const SNOW_GOLEM_SPAWN_EGG: i32 = 1319;
pub const TRADER_LLAMA_SPAWN_EGG: i32 = 1320;
pub const VILLAGER_SPAWN_EGG: i32 = 1321;
pub const WANDERING_TRADER_SPAWN_EGG: i32 = 1322;
pub const BOGGED_SPAWN_EGG: i32 = 1323;
pub const CAMEL_HUSK_SPAWN_EGG: i32 = 1324;
pub const DROWNED_SPAWN_EGG: i32 = 1325;
pub const HUSK_SPAWN_EGG: i32 = 1326;
pub const PARCHED_SPAWN_EGG: i32 = 1327;
pub const SKELETON_SPAWN_EGG: i32 = 1328;
pub const SKELETON_HORSE_SPAWN_EGG: i32 = 1329;
pub const STRAY_SPAWN_EGG: i32 = 1330;
pub const WITHER_SPAWN_EGG: i32 = 1331;
pub const WITHER_SKELETON_SPAWN_EGG: i32 = 1332;
pub const ZOMBIE_SPAWN_EGG: i32 = 1333;
pub const ZOMBIE_HORSE_SPAWN_EGG: i32 = 1334;
pub const ZOMBIE_NAUTILUS_SPAWN_EGG: i32 = 1335;
pub const ZOMBIE_VILLAGER_SPAWN_EGG: i32 = 1336;
pub const CAVE_SPIDER_SPAWN_EGG: i32 = 1337;
pub const SPIDER_SPAWN_EGG: i32 = 1338;
pub const BREEZE_SPAWN_EGG: i32 = 1339;
pub const CREAKING_SPAWN_EGG: i32 = 1340;
pub const CREEPER_SPAWN_EGG: i32 = 1341;
pub const ELDER_GUARDIAN_SPAWN_EGG: i32 = 1342;
pub const GUARDIAN_SPAWN_EGG: i32 = 1343;
pub const PHANTOM_SPAWN_EGG: i32 = 1344;
pub const SILVERFISH_SPAWN_EGG: i32 = 1345;
pub const SLIME_SPAWN_EGG: i32 = 1346;
pub const WARDEN_SPAWN_EGG: i32 = 1347;
pub const WITCH_SPAWN_EGG: i32 = 1348;
pub const EVOKER_SPAWN_EGG: i32 = 1349;
pub const PILLAGER_SPAWN_EGG: i32 = 1350;
pub const RAVAGER_SPAWN_EGG: i32 = 1351;
pub const VINDICATOR_SPAWN_EGG: i32 = 1352;
pub const VEX_SPAWN_EGG: i32 = 1353;
pub const BLAZE_SPAWN_EGG: i32 = 1354;
pub const GHAST_SPAWN_EGG: i32 = 1355;
pub const HAPPY_GHAST_SPAWN_EGG: i32 = 1356;
pub const HOGLIN_SPAWN_EGG: i32 = 1357;
pub const MAGMA_CUBE_SPAWN_EGG: i32 = 1358;
pub const PIGLIN_SPAWN_EGG: i32 = 1359;
pub const PIGLIN_BRUTE_SPAWN_EGG: i32 = 1360;
pub const STRIDER_SPAWN_EGG: i32 = 1361;
pub const ZOGLIN_SPAWN_EGG: i32 = 1362;
pub const ZOMBIFIED_PIGLIN_SPAWN_EGG: i32 = 1363;
pub const ENDER_DRAGON_SPAWN_EGG: i32 = 1364;
pub const ENDERMAN_SPAWN_EGG: i32 = 1365;
pub const ENDERMITE_SPAWN_EGG: i32 = 1366;
pub const SHULKER_SPAWN_EGG: i32 = 1367;
pub const EXPERIENCE_BOTTLE: i32 = 1368;
pub const FIRE_CHARGE: i32 = 1369;
pub const WIND_CHARGE: i32 = 1370;
pub const WRITABLE_BOOK: i32 = 1371;
pub const WRITTEN_BOOK: i32 = 1372;
pub const BREEZE_ROD: i32 = 1373;
pub const MACE: i32 = 1374;
pub const ITEM_FRAME: i32 = 1375;
pub const GLOW_ITEM_FRAME: i32 = 1376;
pub const FLOWER_POT: i32 = 1377;
pub const CARROT: i32 = 1378;
pub const POTATO: i32 = 1379;
pub const BAKED_POTATO: i32 = 1380;
pub const POISONOUS_POTATO: i32 = 1381;
pub const MAP: i32 = 1382;
pub const GOLDEN_CARROT: i32 = 1383;
pub const SKELETON_SKULL: i32 = 1384;
pub const WITHER_SKELETON_SKULL: i32 = 1385;
pub const PLAYER_HEAD: i32 = 1386;
pub const ZOMBIE_HEAD: i32 = 1387;
pub const CREEPER_HEAD: i32 = 1388;
pub const DRAGON_HEAD: i32 = 1389;
pub const PIGLIN_HEAD: i32 = 1390;
pub const NETHER_STAR: i32 = 1391;
pub const PUMPKIN_PIE: i32 = 1392;
pub const FIREWORK_ROCKET: i32 = 1393;
pub const FIREWORK_STAR: i32 = 1394;
pub const ENCHANTED_BOOK: i32 = 1395;
pub const NETHER_BRICK: i32 = 1396;
pub const RESIN_BRICK: i32 = 1397;
pub const PRISMARINE_SHARD: i32 = 1398;
pub const PRISMARINE_CRYSTALS: i32 = 1399;
pub const RABBIT: i32 = 1400;
pub const COOKED_RABBIT: i32 = 1401;
pub const RABBIT_STEW: i32 = 1402;
pub const RABBIT_FOOT: i32 = 1403;
pub const RABBIT_HIDE: i32 = 1404;
pub const ARMOR_STAND: i32 = 1405;
pub const COPPER_HORSE_ARMOR: i32 = 1406;
pub const IRON_HORSE_ARMOR: i32 = 1407;
pub const GOLDEN_HORSE_ARMOR: i32 = 1408;
pub const DIAMOND_HORSE_ARMOR: i32 = 1409;
pub const NETHERITE_HORSE_ARMOR: i32 = 1410;
pub const LEATHER_HORSE_ARMOR: i32 = 1411;
pub const LEAD: i32 = 1412;
pub const NAME_TAG: i32 = 1413;
pub const COMMAND_BLOCK_MINECART: i32 = 1414;
pub const MUTTON: i32 = 1415;
pub const COOKED_MUTTON: i32 = 1416;
pub const WHITE_BANNER: i32 = 1417;
pub const ORANGE_BANNER: i32 = 1418;
pub const MAGENTA_BANNER: i32 = 1419;
pub const LIGHT_BLUE_BANNER: i32 = 1420;
pub const YELLOW_BANNER: i32 = 1421;
pub const LIME_BANNER: i32 = 1422;
pub const PINK_BANNER: i32 = 1423;
pub const GRAY_BANNER: i32 = 1424;
pub const LIGHT_GRAY_BANNER: i32 = 1425;
pub const CYAN_BANNER: i32 = 1426;
pub const PURPLE_BANNER: i32 = 1427;
pub const BLUE_BANNER: i32 = 1428;
pub const BROWN_BANNER: i32 = 1429;
pub const GREEN_BANNER: i32 = 1430;
pub const RED_BANNER: i32 = 1431;
pub const BLACK_BANNER: i32 = 1432;
pub const END_CRYSTAL: i32 = 1433;
pub const CHORUS_FRUIT: i32 = 1434;
pub const POPPED_CHORUS_FRUIT: i32 = 1435;
pub const TORCHFLOWER_SEEDS: i32 = 1436;
pub const PITCHER_POD: i32 = 1437;
pub const BEETROOT: i32 = 1438;
pub const BEETROOT_SEEDS: i32 = 1439;
pub const BEETROOT_SOUP: i32 = 1440;
pub const DRAGON_BREATH: i32 = 1441;
pub const SPLASH_POTION: i32 = 1442;
pub const SPECTRAL_ARROW: i32 = 1443;
pub const TIPPED_ARROW: i32 = 1444;
pub const LINGERING_POTION: i32 = 1445;
pub const SHIELD: i32 = 1446;
pub const WOODEN_SPEAR: i32 = 1447;
pub const STONE_SPEAR: i32 = 1448;
pub const COPPER_SPEAR: i32 = 1449;
pub const IRON_SPEAR: i32 = 1450;
pub const GOLDEN_SPEAR: i32 = 1451;
pub const DIAMOND_SPEAR: i32 = 1452;
pub const NETHERITE_SPEAR: i32 = 1453;
pub const TOTEM_OF_UNDYING: i32 = 1454;
pub const SHULKER_SHELL: i32 = 1455;
pub const IRON_NUGGET: i32 = 1456;
pub const COPPER_NUGGET: i32 = 1457;
pub const KNOWLEDGE_BOOK: i32 = 1458;
pub const DEBUG_STICK: i32 = 1459;
pub const MUSIC_DISC_13: i32 = 1460;
pub const MUSIC_DISC_CAT: i32 = 1461;
pub const MUSIC_DISC_BLOCKS: i32 = 1462;
pub const MUSIC_DISC_BOUNCE: i32 = 1463;
pub const MUSIC_DISC_CHIRP: i32 = 1464;
pub const MUSIC_DISC_CREATOR: i32 = 1465;
pub const MUSIC_DISC_CREATOR_MUSIC_BOX: i32 = 1466;
pub const MUSIC_DISC_FAR: i32 = 1467;
pub const MUSIC_DISC_LAVA_CHICKEN: i32 = 1468;
pub const MUSIC_DISC_MALL: i32 = 1469;
pub const MUSIC_DISC_MELLOHI: i32 = 1470;
pub const MUSIC_DISC_STAL: i32 = 1471;
pub const MUSIC_DISC_STRAD: i32 = 1472;
pub const MUSIC_DISC_WARD: i32 = 1473;
pub const MUSIC_DISC_11: i32 = 1474;
pub const MUSIC_DISC_WAIT: i32 = 1475;
pub const MUSIC_DISC_OTHERSIDE: i32 = 1476;
pub const MUSIC_DISC_RELIC: i32 = 1477;
pub const MUSIC_DISC_5: i32 = 1478;
pub const MUSIC_DISC_PIGSTEP: i32 = 1479;
pub const MUSIC_DISC_PRECIPICE: i32 = 1480;
pub const MUSIC_DISC_TEARS: i32 = 1481;
pub const DISC_FRAGMENT_5: i32 = 1482;
pub const TRIDENT: i32 = 1483;
pub const NAUTILUS_SHELL: i32 = 1484;
pub const IRON_NAUTILUS_ARMOR: i32 = 1485;
pub const GOLDEN_NAUTILUS_ARMOR: i32 = 1486;
pub const DIAMOND_NAUTILUS_ARMOR: i32 = 1487;
pub const NETHERITE_NAUTILUS_ARMOR: i32 = 1488;
pub const COPPER_NAUTILUS_ARMOR: i32 = 1489;
pub const HEART_OF_THE_SEA: i32 = 1490;
pub const CROSSBOW: i32 = 1491;
pub const SUSPICIOUS_STEW: i32 = 1492;
pub const LOOM: i32 = 1493;
pub const FLOWER_BANNER_PATTERN: i32 = 1494;
pub const CREEPER_BANNER_PATTERN: i32 = 1495;
pub const SKULL_BANNER_PATTERN: i32 = 1496;
pub const MOJANG_BANNER_PATTERN: i32 = 1497;
pub const GLOBE_BANNER_PATTERN: i32 = 1498;
pub const PIGLIN_BANNER_PATTERN: i32 = 1499;
pub const FLOW_BANNER_PATTERN: i32 = 1500;
pub const GUSTER_BANNER_PATTERN: i32 = 1501;
pub const FIELD_MASONED_BANNER_PATTERN: i32 = 1502;
pub const BORDURE_INDENTED_BANNER_PATTERN: i32 = 1503;
pub const GOAT_HORN: i32 = 1504;
pub const COMPOSTER: i32 = 1505;
pub const BARREL: i32 = 1506;
pub const SMOKER: i32 = 1507;
pub const BLAST_FURNACE: i32 = 1508;
pub const CARTOGRAPHY_TABLE: i32 = 1509;
pub const FLETCHING_TABLE: i32 = 1510;
pub const GRINDSTONE: i32 = 1511;
pub const SMITHING_TABLE: i32 = 1512;
pub const STONECUTTER: i32 = 1513;
pub const BELL: i32 = 1514;
pub const LANTERN: i32 = 1515;
pub const SOUL_LANTERN: i32 = 1516;
pub const COPPER_LANTERN: i32 = 1517;
pub const EXPOSED_COPPER_LANTERN: i32 = 1518;
pub const WEATHERED_COPPER_LANTERN: i32 = 1519;
pub const OXIDIZED_COPPER_LANTERN: i32 = 1520;
pub const WAXED_COPPER_LANTERN: i32 = 1521;
pub const WAXED_EXPOSED_COPPER_LANTERN: i32 = 1522;
pub const WAXED_WEATHERED_COPPER_LANTERN: i32 = 1523;
pub const WAXED_OXIDIZED_COPPER_LANTERN: i32 = 1524;
pub const SWEET_BERRIES: i32 = 1525;
pub const GLOW_BERRIES: i32 = 1526;
pub const CAMPFIRE: i32 = 1527;
pub const SOUL_CAMPFIRE: i32 = 1528;
pub const SHROOMLIGHT: i32 = 1529;
pub const HONEYCOMB: i32 = 1530;
pub const BEE_NEST: i32 = 1531;
pub const BEEHIVE: i32 = 1532;
pub const HONEY_BOTTLE: i32 = 1533;
pub const HONEYCOMB_BLOCK: i32 = 1534;
pub const LODESTONE: i32 = 1535;
pub const CRYING_OBSIDIAN: i32 = 1536;
pub const BLACKSTONE: i32 = 1537;
pub const BLACKSTONE_SLAB: i32 = 1538;
pub const BLACKSTONE_STAIRS: i32 = 1539;
pub const GILDED_BLACKSTONE: i32 = 1540;
pub const POLISHED_BLACKSTONE: i32 = 1541;
pub const POLISHED_BLACKSTONE_SLAB: i32 = 1542;
pub const POLISHED_BLACKSTONE_STAIRS: i32 = 1543;
pub const CHISELED_POLISHED_BLACKSTONE: i32 = 1544;
pub const POLISHED_BLACKSTONE_BRICKS: i32 = 1545;
pub const POLISHED_BLACKSTONE_BRICK_SLAB: i32 = 1546;
pub const POLISHED_BLACKSTONE_BRICK_STAIRS: i32 = 1547;
pub const CRACKED_POLISHED_BLACKSTONE_BRICKS: i32 = 1548;
pub const RESPAWN_ANCHOR: i32 = 1549;
pub const CANDLE: i32 = 1550;
pub const WHITE_CANDLE: i32 = 1551;
pub const ORANGE_CANDLE: i32 = 1552;
pub const MAGENTA_CANDLE: i32 = 1553;
pub const LIGHT_BLUE_CANDLE: i32 = 1554;
pub const YELLOW_CANDLE: i32 = 1555;
pub const LIME_CANDLE: i32 = 1556;
pub const PINK_CANDLE: i32 = 1557;
pub const GRAY_CANDLE: i32 = 1558;
pub const LIGHT_GRAY_CANDLE: i32 = 1559;
pub const CYAN_CANDLE: i32 = 1560;
pub const PURPLE_CANDLE: i32 = 1561;
pub const BLUE_CANDLE: i32 = 1562;
pub const BROWN_CANDLE: i32 = 1563;
pub const GREEN_CANDLE: i32 = 1564;
pub const RED_CANDLE: i32 = 1565;
pub const BLACK_CANDLE: i32 = 1566;
pub const SMALL_AMETHYST_BUD: i32 = 1567;
pub const MEDIUM_AMETHYST_BUD: i32 = 1568;
pub const LARGE_AMETHYST_BUD: i32 = 1569;
pub const AMETHYST_CLUSTER: i32 = 1570;
pub const POINTED_DRIPSTONE: i32 = 1571;
pub const SULFUR_SPIKE: i32 = 1572;
pub const OCHRE_FROGLIGHT: i32 = 1573;
pub const VERDANT_FROGLIGHT: i32 = 1574;
pub const PEARLESCENT_FROGLIGHT: i32 = 1575;
pub const FROGSPAWN: i32 = 1576;
pub const ECHO_SHARD: i32 = 1577;
pub const BRUSH: i32 = 1578;
pub const NETHERITE_UPGRADE_SMITHING_TEMPLATE: i32 = 1579;
pub const SENTRY_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1580;
pub const DUNE_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1581;
pub const COAST_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1582;
pub const WILD_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1583;
pub const WARD_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1584;
pub const EYE_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1585;
pub const VEX_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1586;
pub const TIDE_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1587;
pub const SNOUT_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1588;
pub const RIB_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1589;
pub const SPIRE_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1590;
pub const WAYFINDER_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1591;
pub const SHAPER_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1592;
pub const SILENCE_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1593;
pub const RAISER_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1594;
pub const HOST_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1595;
pub const FLOW_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1596;
pub const BOLT_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = 1597;
pub const ANGLER_POTTERY_SHERD: i32 = 1598;
pub const ARCHER_POTTERY_SHERD: i32 = 1599;
pub const ARMS_UP_POTTERY_SHERD: i32 = 1600;
pub const BLADE_POTTERY_SHERD: i32 = 1601;
pub const BREWER_POTTERY_SHERD: i32 = 1602;
pub const BURN_POTTERY_SHERD: i32 = 1603;
pub const DANGER_POTTERY_SHERD: i32 = 1604;
pub const EXPLORER_POTTERY_SHERD: i32 = 1605;
pub const FLOW_POTTERY_SHERD: i32 = 1606;
pub const FRIEND_POTTERY_SHERD: i32 = 1607;
pub const GUSTER_POTTERY_SHERD: i32 = 1608;
pub const HEART_POTTERY_SHERD: i32 = 1609;
pub const HEARTBREAK_POTTERY_SHERD: i32 = 1610;
pub const HOWL_POTTERY_SHERD: i32 = 1611;
pub const MINER_POTTERY_SHERD: i32 = 1612;
pub const MOURNER_POTTERY_SHERD: i32 = 1613;
pub const PLENTY_POTTERY_SHERD: i32 = 1614;
pub const PRIZE_POTTERY_SHERD: i32 = 1615;
pub const SCRAPE_POTTERY_SHERD: i32 = 1616;
pub const SHEAF_POTTERY_SHERD: i32 = 1617;
pub const SHELTER_POTTERY_SHERD: i32 = 1618;
pub const SKULL_POTTERY_SHERD: i32 = 1619;
pub const SNORT_POTTERY_SHERD: i32 = 1620;
pub const COPPER_GRATE: i32 = 1621;
pub const EXPOSED_COPPER_GRATE: i32 = 1622;
pub const WEATHERED_COPPER_GRATE: i32 = 1623;
pub const OXIDIZED_COPPER_GRATE: i32 = 1624;
pub const WAXED_COPPER_GRATE: i32 = 1625;
pub const WAXED_EXPOSED_COPPER_GRATE: i32 = 1626;
pub const WAXED_WEATHERED_COPPER_GRATE: i32 = 1627;
pub const WAXED_OXIDIZED_COPPER_GRATE: i32 = 1628;
pub const COPPER_BULB: i32 = 1629;
pub const EXPOSED_COPPER_BULB: i32 = 1630;
pub const WEATHERED_COPPER_BULB: i32 = 1631;
pub const OXIDIZED_COPPER_BULB: i32 = 1632;
pub const WAXED_COPPER_BULB: i32 = 1633;
pub const WAXED_EXPOSED_COPPER_BULB: i32 = 1634;
pub const WAXED_WEATHERED_COPPER_BULB: i32 = 1635;
pub const WAXED_OXIDIZED_COPPER_BULB: i32 = 1636;
pub const COPPER_CHEST: i32 = 1637;
pub const EXPOSED_COPPER_CHEST: i32 = 1638;
pub const WEATHERED_COPPER_CHEST: i32 = 1639;
pub const OXIDIZED_COPPER_CHEST: i32 = 1640;
pub const WAXED_COPPER_CHEST: i32 = 1641;
pub const WAXED_EXPOSED_COPPER_CHEST: i32 = 1642;
pub const WAXED_WEATHERED_COPPER_CHEST: i32 = 1643;
pub const WAXED_OXIDIZED_COPPER_CHEST: i32 = 1644;
pub const COPPER_GOLEM_STATUE: i32 = 1645;
pub const EXPOSED_COPPER_GOLEM_STATUE: i32 = 1646;
pub const WEATHERED_COPPER_GOLEM_STATUE: i32 = 1647;
pub const OXIDIZED_COPPER_GOLEM_STATUE: i32 = 1648;
pub const WAXED_COPPER_GOLEM_STATUE: i32 = 1649;
pub const WAXED_EXPOSED_COPPER_GOLEM_STATUE: i32 = 1650;
pub const WAXED_WEATHERED_COPPER_GOLEM_STATUE: i32 = 1651;
pub const WAXED_OXIDIZED_COPPER_GOLEM_STATUE: i32 = 1652;
pub const TRIAL_SPAWNER: i32 = 1653;
pub const TRIAL_KEY: i32 = 1654;
pub const OMINOUS_TRIAL_KEY: i32 = 1655;
pub const VAULT: i32 = 1656;
pub const OMINOUS_BOTTLE: i32 = 1657;

/// 全部物品，下标就是注册表 ID。
pub const ITEMS: &[Item] = &[
    Item { name: "minecraft:air", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone", kind: ItemKind::Block, block: Some(1), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:granite", kind: ItemKind::Block, block: Some(2), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_granite", kind: ItemKind::Block, block: Some(3), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diorite", kind: ItemKind::Block, block: Some(4), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_diorite", kind: ItemKind::Block, block: Some(5), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:andesite", kind: ItemKind::Block, block: Some(6), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_andesite", kind: ItemKind::Block, block: Some(7), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate", kind: ItemKind::Block, block: Some(1241), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cobbled_deepslate", kind: ItemKind::Block, block: Some(1242), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_deepslate", kind: ItemKind::Block, block: Some(1246), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:calcite", kind: ItemKind::Block, block: Some(1115), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tuff", kind: ItemKind::Block, block: Some(1074), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tuff_slab", kind: ItemKind::Block, block: Some(1075), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tuff_stairs", kind: ItemKind::Block, block: Some(1076), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tuff_wall", kind: ItemKind::Block, block: Some(1077), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chiseled_tuff", kind: ItemKind::Block, block: Some(1082), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_tuff", kind: ItemKind::Block, block: Some(1078), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_tuff_slab", kind: ItemKind::Block, block: Some(1079), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_tuff_stairs", kind: ItemKind::Block, block: Some(1080), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_tuff_wall", kind: ItemKind::Block, block: Some(1081), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tuff_bricks", kind: ItemKind::Block, block: Some(1083), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tuff_brick_slab", kind: ItemKind::Block, block: Some(1084), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tuff_brick_stairs", kind: ItemKind::Block, block: Some(1085), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tuff_brick_wall", kind: ItemKind::Block, block: Some(1086), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chiseled_tuff_bricks", kind: ItemKind::Block, block: Some(1087), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sulfur", kind: ItemKind::Block, block: Some(1088), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:potent_sulfur", kind: ItemKind::Block, block: Some(1089), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sulfur_slab", kind: ItemKind::Block, block: Some(1090), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sulfur_stairs", kind: ItemKind::Block, block: Some(1091), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sulfur_wall", kind: ItemKind::Block, block: Some(1092), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_sulfur", kind: ItemKind::Block, block: Some(1093), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_sulfur_slab", kind: ItemKind::Block, block: Some(1094), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_sulfur_stairs", kind: ItemKind::Block, block: Some(1095), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_sulfur_wall", kind: ItemKind::Block, block: Some(1096), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sulfur_bricks", kind: ItemKind::Block, block: Some(1097), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sulfur_brick_slab", kind: ItemKind::Block, block: Some(1098), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sulfur_brick_stairs", kind: ItemKind::Block, block: Some(1099), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sulfur_brick_wall", kind: ItemKind::Block, block: Some(1100), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chiseled_sulfur", kind: ItemKind::Block, block: Some(1101), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cinnabar", kind: ItemKind::Block, block: Some(1102), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cinnabar_slab", kind: ItemKind::Block, block: Some(1103), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cinnabar_stairs", kind: ItemKind::Block, block: Some(1104), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cinnabar_wall", kind: ItemKind::Block, block: Some(1105), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_cinnabar", kind: ItemKind::Block, block: Some(1106), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_cinnabar_slab", kind: ItemKind::Block, block: Some(1107), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_cinnabar_stairs", kind: ItemKind::Block, block: Some(1108), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_cinnabar_wall", kind: ItemKind::Block, block: Some(1109), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cinnabar_bricks", kind: ItemKind::Block, block: Some(1110), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cinnabar_brick_slab", kind: ItemKind::Block, block: Some(1111), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cinnabar_brick_stairs", kind: ItemKind::Block, block: Some(1112), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cinnabar_brick_wall", kind: ItemKind::Block, block: Some(1113), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chiseled_cinnabar", kind: ItemKind::Block, block: Some(1114), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dripstone_block", kind: ItemKind::Block, block: Some(1222), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:grass_block", kind: ItemKind::Block, block: Some(8), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dirt", kind: ItemKind::Block, block: Some(9), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:coarse_dirt", kind: ItemKind::Block, block: Some(10), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:podzol", kind: ItemKind::Block, block: Some(11), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:rooted_dirt", kind: ItemKind::Block, block: Some(1239), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mud", kind: ItemKind::Block, block: Some(1240), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_nylium", kind: ItemKind::Block, block: Some(965), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_nylium", kind: ItemKind::Block, block: Some(956), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cobblestone", kind: ItemKind::Block, block: Some(12), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_planks", kind: ItemKind::Block, block: Some(13), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_planks", kind: ItemKind::Block, block: Some(14), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_planks", kind: ItemKind::Block, block: Some(15), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_planks", kind: ItemKind::Block, block: Some(16), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_planks", kind: ItemKind::Block, block: Some(17), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_planks", kind: ItemKind::Block, block: Some(18), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_planks", kind: ItemKind::Block, block: Some(19), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_planks", kind: ItemKind::Block, block: Some(21), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_planks", kind: ItemKind::Block, block: Some(22), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_planks", kind: ItemKind::Block, block: Some(23), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_planks", kind: ItemKind::Block, block: Some(24), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_planks", kind: ItemKind::Block, block: Some(973), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_planks", kind: ItemKind::Block, block: Some(974), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_mosaic", kind: ItemKind::Block, block: Some(25), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_sapling", kind: ItemKind::Block, block: Some(26), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:spruce_sapling", kind: ItemKind::Block, block: Some(27), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:birch_sapling", kind: ItemKind::Block, block: Some(28), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:jungle_sapling", kind: ItemKind::Block, block: Some(29), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:acacia_sapling", kind: ItemKind::Block, block: Some(30), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:cherry_sapling", kind: ItemKind::Block, block: Some(31), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_sapling", kind: ItemKind::Block, block: Some(32), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_sapling", kind: ItemKind::Block, block: Some(33), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:mangrove_propagule", kind: ItemKind::Block, block: Some(34), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:poplar_sapling", kind: ItemKind::Block, block: Some(35), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:bedrock", kind: ItemKind::Block, block: Some(36), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sand", kind: ItemKind::Block, block: Some(39), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:suspicious_sand", kind: ItemKind::Block, block: Some(40), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:suspicious_gravel", kind: ItemKind::Block, block: Some(43), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_sand", kind: ItemKind::Block, block: Some(41), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gravel", kind: ItemKind::Block, block: Some(42), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:coal_ore", kind: ItemKind::Block, block: Some(48), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_coal_ore", kind: ItemKind::Block, block: Some(49), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_ore", kind: ItemKind::Block, block: Some(46), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_iron_ore", kind: ItemKind::Block, block: Some(47), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_ore", kind: ItemKind::Block, block: Some(1132), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_copper_ore", kind: ItemKind::Block, block: Some(1133), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gold_ore", kind: ItemKind::Block, block: Some(44), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_gold_ore", kind: ItemKind::Block, block: Some(45), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:redstone_ore", kind: ItemKind::Block, block: Some(320), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_redstone_ore", kind: ItemKind::Block, block: Some(321), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:emerald_ore", kind: ItemKind::Block, block: Some(449), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_emerald_ore", kind: ItemKind::Block, block: Some(450), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lapis_ore", kind: ItemKind::Block, block: Some(111), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_lapis_ore", kind: ItemKind::Block, block: Some(112), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond_ore", kind: ItemKind::Block, block: Some(247), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_diamond_ore", kind: ItemKind::Block, block: Some(248), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:nether_gold_ore", kind: ItemKind::Block, block: Some(50), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:nether_quartz_ore", kind: ItemKind::Block, block: Some(529), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ancient_debris", kind: ItemKind::Block, block: Some(1006), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:coal_block", kind: ItemKind::Block, block: Some(609), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:raw_iron_block", kind: ItemKind::Block, block: Some(1263), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:raw_copper_block", kind: ItemKind::Block, block: Some(1264), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:raw_gold_block", kind: ItemKind::Block, block: Some(1265), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:heavy_core", kind: ItemKind::Block, block: Some(1277), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:amethyst_block", kind: ItemKind::Block, block: Some(1068), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:budding_amethyst", kind: ItemKind::Block, block: Some(1069), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_block", kind: ItemKind::Block, block: Some(218), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_block", kind: ItemKind::Block, block: Some(1124), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_copper", kind: ItemKind::Block, block: Some(1125), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_copper", kind: ItemKind::Block, block: Some(1126), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_copper", kind: ItemKind::Block, block: Some(1127), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_copper_block", kind: ItemKind::Block, block: Some(1128), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_copper", kind: ItemKind::Block, block: Some(1129), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_copper", kind: ItemKind::Block, block: Some(1130), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_copper", kind: ItemKind::Block, block: Some(1131), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gold_block", kind: ItemKind::Block, block: Some(217), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond_block", kind: ItemKind::Block, block: Some(249), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_block", kind: ItemKind::Block, block: Some(1005), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chiseled_copper", kind: ItemKind::Block, block: Some(1142), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_chiseled_copper", kind: ItemKind::Block, block: Some(1143), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_chiseled_copper", kind: ItemKind::Block, block: Some(1144), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_chiseled_copper", kind: ItemKind::Block, block: Some(1145), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_chiseled_copper", kind: ItemKind::Block, block: Some(1146), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_chiseled_copper", kind: ItemKind::Block, block: Some(1147), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_chiseled_copper", kind: ItemKind::Block, block: Some(1148), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_chiseled_copper", kind: ItemKind::Block, block: Some(1149), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cut_copper", kind: ItemKind::Block, block: Some(1134), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_cut_copper", kind: ItemKind::Block, block: Some(1135), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_cut_copper", kind: ItemKind::Block, block: Some(1136), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_cut_copper", kind: ItemKind::Block, block: Some(1137), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_cut_copper", kind: ItemKind::Block, block: Some(1138), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_cut_copper", kind: ItemKind::Block, block: Some(1139), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_cut_copper", kind: ItemKind::Block, block: Some(1140), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_cut_copper", kind: ItemKind::Block, block: Some(1141), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cut_copper_stairs", kind: ItemKind::Block, block: Some(1150), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_cut_copper_stairs", kind: ItemKind::Block, block: Some(1151), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_cut_copper_stairs", kind: ItemKind::Block, block: Some(1152), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_cut_copper_stairs", kind: ItemKind::Block, block: Some(1153), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_cut_copper_stairs", kind: ItemKind::Block, block: Some(1154), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_cut_copper_stairs", kind: ItemKind::Block, block: Some(1155), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_cut_copper_stairs", kind: ItemKind::Block, block: Some(1156), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_cut_copper_stairs", kind: ItemKind::Block, block: Some(1157), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cut_copper_slab", kind: ItemKind::Block, block: Some(1158), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_cut_copper_slab", kind: ItemKind::Block, block: Some(1159), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_cut_copper_slab", kind: ItemKind::Block, block: Some(1160), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_cut_copper_slab", kind: ItemKind::Block, block: Some(1161), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_cut_copper_slab", kind: ItemKind::Block, block: Some(1162), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_cut_copper_slab", kind: ItemKind::Block, block: Some(1163), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_cut_copper_slab", kind: ItemKind::Block, block: Some(1164), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_cut_copper_slab", kind: ItemKind::Block, block: Some(1165), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_log", kind: ItemKind::Block, block: Some(51), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_log", kind: ItemKind::Block, block: Some(52), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_log", kind: ItemKind::Block, block: Some(53), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_log", kind: ItemKind::Block, block: Some(54), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_log", kind: ItemKind::Block, block: Some(55), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_log", kind: ItemKind::Block, block: Some(56), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_log", kind: ItemKind::Block, block: Some(58), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_log", kind: ItemKind::Block, block: Some(57), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_log", kind: ItemKind::Block, block: Some(59), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_log", kind: ItemKind::Block, block: Some(60), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_roots", kind: ItemKind::Block, block: Some(61), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:muddy_mangrove_roots", kind: ItemKind::Block, block: Some(62), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_stem", kind: ItemKind::Block, block: Some(961), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_stem", kind: ItemKind::Block, block: Some(952), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_block", kind: ItemKind::Block, block: Some(63), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_oak_log", kind: ItemKind::Block, block: Some(71), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_spruce_log", kind: ItemKind::Block, block: Some(64), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_birch_log", kind: ItemKind::Block, block: Some(65), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_jungle_log", kind: ItemKind::Block, block: Some(66), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_acacia_log", kind: ItemKind::Block, block: Some(67), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_cherry_log", kind: ItemKind::Block, block: Some(68), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_dark_oak_log", kind: ItemKind::Block, block: Some(69), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_pale_oak_log", kind: ItemKind::Block, block: Some(70), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_mangrove_log", kind: ItemKind::Block, block: Some(72), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_poplar_log", kind: ItemKind::Block, block: Some(73), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_crimson_stem", kind: ItemKind::Block, block: Some(962), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_warped_stem", kind: ItemKind::Block, block: Some(953), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_oak_wood", kind: ItemKind::Block, block: Some(84), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_spruce_wood", kind: ItemKind::Block, block: Some(85), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_birch_wood", kind: ItemKind::Block, block: Some(86), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_jungle_wood", kind: ItemKind::Block, block: Some(87), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_acacia_wood", kind: ItemKind::Block, block: Some(88), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_cherry_wood", kind: ItemKind::Block, block: Some(89), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_dark_oak_wood", kind: ItemKind::Block, block: Some(90), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_pale_oak_wood", kind: ItemKind::Block, block: Some(91), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_mangrove_wood", kind: ItemKind::Block, block: Some(92), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_poplar_wood", kind: ItemKind::Block, block: Some(93), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_crimson_hyphae", kind: ItemKind::Block, block: Some(964), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_warped_hyphae", kind: ItemKind::Block, block: Some(955), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stripped_bamboo_block", kind: ItemKind::Block, block: Some(74), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_wood", kind: ItemKind::Block, block: Some(75), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_wood", kind: ItemKind::Block, block: Some(76), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_wood", kind: ItemKind::Block, block: Some(77), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_wood", kind: ItemKind::Block, block: Some(78), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_wood", kind: ItemKind::Block, block: Some(79), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_wood", kind: ItemKind::Block, block: Some(80), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_wood", kind: ItemKind::Block, block: Some(20), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_wood", kind: ItemKind::Block, block: Some(81), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_wood", kind: ItemKind::Block, block: Some(82), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_wood", kind: ItemKind::Block, block: Some(83), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_hyphae", kind: ItemKind::Block, block: Some(963), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_hyphae", kind: ItemKind::Block, block: Some(954), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_leaves", kind: ItemKind::Block, block: Some(94), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:spruce_leaves", kind: ItemKind::Block, block: Some(95), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:birch_leaves", kind: ItemKind::Block, block: Some(96), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:jungle_leaves", kind: ItemKind::Block, block: Some(97), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:acacia_leaves", kind: ItemKind::Block, block: Some(98), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:cherry_leaves", kind: ItemKind::Block, block: Some(99), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_leaves", kind: ItemKind::Block, block: Some(100), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_leaves", kind: ItemKind::Block, block: Some(101), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:mangrove_leaves", kind: ItemKind::Block, block: Some(102), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:red_poplar_leaves", kind: ItemKind::Block, block: Some(103), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:orange_poplar_leaves", kind: ItemKind::Block, block: Some(104), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:yellow_poplar_leaves", kind: ItemKind::Block, block: Some(105), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:azalea_leaves", kind: ItemKind::Block, block: Some(106), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:flowering_azalea_leaves", kind: ItemKind::Block, block: Some(107), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:sponge", kind: ItemKind::Block, block: Some(108), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wet_sponge", kind: ItemKind::Block, block: Some(109), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:glass", kind: ItemKind::Block, block: Some(110), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tinted_glass", kind: ItemKind::Block, block: Some(1116), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lapis_block", kind: ItemKind::Block, block: Some(113), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sandstone", kind: ItemKind::Block, block: Some(115), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chiseled_sandstone", kind: ItemKind::Block, block: Some(116), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cut_sandstone", kind: ItemKind::Block, block: Some(117), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cobweb", kind: ItemKind::Block, block: Some(139), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:short_grass", kind: ItemKind::Block, block: Some(140), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:fern", kind: ItemKind::Block, block: Some(141), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:bush", kind: ItemKind::Block, block: Some(143), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:red_shrub", kind: ItemKind::Block, block: Some(144), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:azalea", kind: ItemKind::Block, block: Some(1228), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:flowering_azalea", kind: ItemKind::Block, block: Some(1229), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:dead_bush", kind: ItemKind::Block, block: Some(142), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:firefly_bush", kind: ItemKind::Block, block: Some(1285), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:short_dry_grass", kind: ItemKind::Block, block: Some(145), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:tall_dry_grass", kind: ItemKind::Block, block: Some(146), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:seagrass", kind: ItemKind::Block, block: Some(147), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:sea_pickle", kind: ItemKind::Block, block: Some(878), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:white_wool", kind: ItemKind::Block, block: Some(151), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_wool", kind: ItemKind::Block, block: Some(152), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_wool", kind: ItemKind::Block, block: Some(153), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_wool", kind: ItemKind::Block, block: Some(154), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_wool", kind: ItemKind::Block, block: Some(155), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_wool", kind: ItemKind::Block, block: Some(156), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_wool", kind: ItemKind::Block, block: Some(157), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_wool", kind: ItemKind::Block, block: Some(158), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_wool", kind: ItemKind::Block, block: Some(159), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_wool", kind: ItemKind::Block, block: Some(160), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_wool", kind: ItemKind::Block, block: Some(161), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_wool", kind: ItemKind::Block, block: Some(162), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_wool", kind: ItemKind::Block, block: Some(163), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_wool", kind: ItemKind::Block, block: Some(164), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_wool", kind: ItemKind::Block, block: Some(165), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_wool", kind: ItemKind::Block, block: Some(166), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_wool_stairs", kind: ItemKind::Block, block: Some(167), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_wool_stairs", kind: ItemKind::Block, block: Some(168), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_wool_stairs", kind: ItemKind::Block, block: Some(169), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_wool_stairs", kind: ItemKind::Block, block: Some(170), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_wool_stairs", kind: ItemKind::Block, block: Some(171), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_wool_stairs", kind: ItemKind::Block, block: Some(172), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_wool_stairs", kind: ItemKind::Block, block: Some(173), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_wool_stairs", kind: ItemKind::Block, block: Some(174), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_wool_stairs", kind: ItemKind::Block, block: Some(175), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_wool_stairs", kind: ItemKind::Block, block: Some(176), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_wool_stairs", kind: ItemKind::Block, block: Some(177), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_wool_stairs", kind: ItemKind::Block, block: Some(178), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_wool_stairs", kind: ItemKind::Block, block: Some(179), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_wool_stairs", kind: ItemKind::Block, block: Some(180), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_wool_stairs", kind: ItemKind::Block, block: Some(181), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_wool_stairs", kind: ItemKind::Block, block: Some(182), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_wool_slab", kind: ItemKind::Block, block: Some(183), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_wool_slab", kind: ItemKind::Block, block: Some(184), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_wool_slab", kind: ItemKind::Block, block: Some(185), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_wool_slab", kind: ItemKind::Block, block: Some(186), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_wool_slab", kind: ItemKind::Block, block: Some(187), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_wool_slab", kind: ItemKind::Block, block: Some(188), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_wool_slab", kind: ItemKind::Block, block: Some(189), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_wool_slab", kind: ItemKind::Block, block: Some(190), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_wool_slab", kind: ItemKind::Block, block: Some(191), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_wool_slab", kind: ItemKind::Block, block: Some(192), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_wool_slab", kind: ItemKind::Block, block: Some(193), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_wool_slab", kind: ItemKind::Block, block: Some(194), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_wool_slab", kind: ItemKind::Block, block: Some(195), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_wool_slab", kind: ItemKind::Block, block: Some(196), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_wool_slab", kind: ItemKind::Block, block: Some(197), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_wool_slab", kind: ItemKind::Block, block: Some(198), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dandelion", kind: ItemKind::Block, block: Some(200), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:golden_dandelion", kind: ItemKind::Block, block: Some(201), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:open_eyeblossom", kind: ItemKind::Block, block: Some(1281), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:closed_eyeblossom", kind: ItemKind::Block, block: Some(1282), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:poppy", kind: ItemKind::Block, block: Some(203), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:blue_orchid", kind: ItemKind::Block, block: Some(204), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:allium", kind: ItemKind::Block, block: Some(205), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:azure_bluet", kind: ItemKind::Block, block: Some(206), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:red_tulip", kind: ItemKind::Block, block: Some(207), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:orange_tulip", kind: ItemKind::Block, block: Some(208), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:white_tulip", kind: ItemKind::Block, block: Some(209), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:pink_tulip", kind: ItemKind::Block, block: Some(210), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:oxeye_daisy", kind: ItemKind::Block, block: Some(211), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:cornflower", kind: ItemKind::Block, block: Some(212), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:lily_of_the_valley", kind: ItemKind::Block, block: Some(214), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:wither_rose", kind: ItemKind::Block, block: Some(213), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:torchflower", kind: ItemKind::Block, block: Some(202), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:pitcher_plant", kind: ItemKind::Block, block: Some(722), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:spore_blossom", kind: ItemKind::Block, block: Some(1227), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:brown_mushroom", kind: ItemKind::Block, block: Some(215), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:red_mushroom", kind: ItemKind::Block, block: Some(216), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:shelf_mushroom", kind: ItemKind::Block, block: Some(447), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:crimson_fungus", kind: ItemKind::Block, block: Some(966), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:warped_fungus", kind: ItemKind::Block, block: Some(957), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:crimson_roots", kind: ItemKind::Block, block: Some(972), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:warped_roots", kind: ItemKind::Block, block: Some(959), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:nether_sprouts", kind: ItemKind::Block, block: Some(960), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:weeping_vines", kind: ItemKind::Block, block: Some(968), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:twisting_vines", kind: ItemKind::Block, block: Some(970), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:sugar_cane", kind: ItemKind::Block, block: Some(331), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:kelp", kind: ItemKind::Block, block: Some(832), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:pink_petals", kind: ItemKind::Block, block: Some(1231), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:wildflowers", kind: ItemKind::Block, block: Some(1232), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:leaf_litter", kind: ItemKind::Block, block: Some(1233), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:moss_carpet", kind: ItemKind::Block, block: Some(1230), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:moss_block", kind: ItemKind::Block, block: Some(1234), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:pale_moss_carpet", kind: ItemKind::Block, block: Some(1279), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:pale_hanging_moss", kind: ItemKind::Block, block: Some(1280), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:pale_moss_block", kind: ItemKind::Block, block: Some(1278), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:hanging_roots", kind: ItemKind::Block, block: Some(1238), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:big_dripleaf", kind: ItemKind::Block, block: Some(1235), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:small_dripleaf", kind: ItemKind::DoubleHigh, block: Some(1237), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:bamboo", kind: ItemKind::Block, block: Some(882), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_slab", kind: ItemKind::Block, block: Some(653), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_slab", kind: ItemKind::Block, block: Some(654), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_slab", kind: ItemKind::Block, block: Some(655), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_slab", kind: ItemKind::Block, block: Some(656), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_slab", kind: ItemKind::Block, block: Some(657), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_slab", kind: ItemKind::Block, block: Some(658), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_slab", kind: ItemKind::Block, block: Some(659), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_slab", kind: ItemKind::Block, block: Some(660), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_slab", kind: ItemKind::Block, block: Some(661), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_slab", kind: ItemKind::Block, block: Some(662), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_slab", kind: ItemKind::Block, block: Some(663), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_mosaic_slab", kind: ItemKind::Block, block: Some(664), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_slab", kind: ItemKind::Block, block: Some(975), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_slab", kind: ItemKind::Block, block: Some(976), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone_slab", kind: ItemKind::Block, block: Some(665), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:smooth_stone_slab", kind: ItemKind::Block, block: Some(678), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sandstone_slab", kind: ItemKind::Block, block: Some(666), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cut_sandstone_slab", kind: ItemKind::Block, block: Some(667), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:petrified_oak_slab", kind: ItemKind::Block, block: Some(668), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cobblestone_slab", kind: ItemKind::Block, block: Some(669), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brick_slab", kind: ItemKind::Block, block: Some(670), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone_brick_slab", kind: ItemKind::Block, block: Some(671), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mud_brick_slab", kind: ItemKind::Block, block: Some(672), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:nether_brick_slab", kind: ItemKind::Block, block: Some(673), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:quartz_slab", kind: ItemKind::Block, block: Some(674), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_sandstone_slab", kind: ItemKind::Block, block: Some(675), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cut_red_sandstone_slab", kind: ItemKind::Block, block: Some(676), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purpur_slab", kind: ItemKind::Block, block: Some(716), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:prismarine_slab", kind: ItemKind::Block, block: Some(587), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:prismarine_brick_slab", kind: ItemKind::Block, block: Some(588), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_prismarine_slab", kind: ItemKind::Block, block: Some(589), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:smooth_quartz", kind: ItemKind::Block, block: Some(680), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:smooth_red_sandstone", kind: ItemKind::Block, block: Some(681), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:smooth_sandstone", kind: ItemKind::Block, block: Some(679), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:smooth_stone", kind: ItemKind::Block, block: Some(677), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bricks", kind: ItemKind::Block, block: Some(219), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_shelf", kind: ItemKind::Block, block: Some(223), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_shelf", kind: ItemKind::Block, block: Some(224), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_shelf", kind: ItemKind::Block, block: Some(225), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_shelf", kind: ItemKind::Block, block: Some(226), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_shelf", kind: ItemKind::Block, block: Some(227), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_shelf", kind: ItemKind::Block, block: Some(228), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_shelf", kind: ItemKind::Block, block: Some(229), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_shelf", kind: ItemKind::Block, block: Some(230), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_shelf", kind: ItemKind::Block, block: Some(233), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_shelf", kind: ItemKind::Block, block: Some(231), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_shelf", kind: ItemKind::Block, block: Some(232), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_shelf", kind: ItemKind::Block, block: Some(234), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_shelf", kind: ItemKind::Block, block: Some(235), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bookshelf", kind: ItemKind::Block, block: Some(221), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chiseled_bookshelf", kind: ItemKind::Block, block: Some(222), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:decorated_pot", kind: ItemKind::Block, block: Some(1273), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mossy_cobblestone", kind: ItemKind::Block, block: Some(236), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:obsidian", kind: ItemKind::Block, block: Some(237), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:torch", kind: ItemKind::StandingAndWall, block: Some(238), wall_block: Some(239), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:end_rod", kind: ItemKind::Block, block: Some(712), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chorus_plant", kind: ItemKind::Block, block: Some(713), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chorus_flower", kind: ItemKind::Block, block: Some(714), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purpur_block", kind: ItemKind::Block, block: Some(715), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purpur_pillar", kind: ItemKind::Block, block: Some(717), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purpur_stairs", kind: ItemKind::Block, block: Some(718), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spawner", kind: ItemKind::Block, block: Some(242), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:creaking_heart", kind: ItemKind::Block, block: Some(243), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chest", kind: ItemKind::Block, block: Some(245), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crafting_table", kind: ItemKind::Block, block: Some(250), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:farmland", kind: ItemKind::Block, block: Some(252), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:furnace", kind: ItemKind::Block, block: Some(253), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ladder", kind: ItemKind::Block, block: Some(266), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cobblestone_stairs", kind: ItemKind::Block, block: Some(268), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:snow", kind: ItemKind::Block, block: Some(325), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ice", kind: ItemKind::Block, block: Some(326), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:snow_block", kind: ItemKind::Block, block: Some(327), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cactus", kind: ItemKind::Block, block: Some(328), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:cactus_flower", kind: ItemKind::Block, block: Some(329), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:clay", kind: ItemKind::Block, block: Some(330), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jukebox", kind: ItemKind::Block, block: Some(332), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_fence", kind: ItemKind::Block, block: Some(333), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_fence", kind: ItemKind::Block, block: Some(692), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_fence", kind: ItemKind::Block, block: Some(693), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_fence", kind: ItemKind::Block, block: Some(694), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_fence", kind: ItemKind::Block, block: Some(695), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_fence", kind: ItemKind::Block, block: Some(696), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_fence", kind: ItemKind::Block, block: Some(697), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_fence", kind: ItemKind::Block, block: Some(698), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_fence", kind: ItemKind::Block, block: Some(699), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_fence", kind: ItemKind::Block, block: Some(700), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_fence", kind: ItemKind::Block, block: Some(701), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_fence", kind: ItemKind::Block, block: Some(979), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_fence", kind: ItemKind::Block, block: Some(980), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pumpkin", kind: ItemKind::Block, block: Some(410), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:carved_pumpkin", kind: ItemKind::Block, block: Some(345), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:jack_o_lantern", kind: ItemKind::Block, block: Some(346), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherrack", kind: ItemKind::Block, block: Some(334), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:soul_sand", kind: ItemKind::Block, block: Some(335), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:soul_soil", kind: ItemKind::Block, block: Some(336), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:basalt", kind: ItemKind::Block, block: Some(337), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_basalt", kind: ItemKind::Block, block: Some(338), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:smooth_basalt", kind: ItemKind::Block, block: Some(1262), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:soul_torch", kind: ItemKind::StandingAndWall, block: Some(339), wall_block: Some(340), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_torch", kind: ItemKind::StandingAndWall, block: Some(341), wall_block: Some(342), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:glowstone", kind: ItemKind::Block, block: Some(343), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:infested_stone", kind: ItemKind::Block, block: Some(382), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:infested_cobblestone", kind: ItemKind::Block, block: Some(383), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:infested_stone_bricks", kind: ItemKind::Block, block: Some(384), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:infested_mossy_stone_bricks", kind: ItemKind::Block, block: Some(385), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:infested_cracked_stone_bricks", kind: ItemKind::Block, block: Some(386), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:infested_chiseled_stone_bricks", kind: ItemKind::Block, block: Some(387), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:infested_deepslate", kind: ItemKind::Block, block: Some(1261), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone_bricks", kind: ItemKind::Block, block: Some(376), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mossy_stone_bricks", kind: ItemKind::Block, block: Some(377), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cracked_stone_bricks", kind: ItemKind::Block, block: Some(378), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chiseled_stone_bricks", kind: ItemKind::Block, block: Some(379), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:packed_mud", kind: ItemKind::Block, block: Some(380), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mud_bricks", kind: ItemKind::Block, block: Some(381), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_bricks", kind: ItemKind::Block, block: Some(1254), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cracked_deepslate_bricks", kind: ItemKind::Block, block: Some(1259), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_tiles", kind: ItemKind::Block, block: Some(1250), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cracked_deepslate_tiles", kind: ItemKind::Block, block: Some(1260), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chiseled_deepslate", kind: ItemKind::Block, block: Some(1258), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:reinforced_deepslate", kind: ItemKind::Block, block: Some(1272), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_mushroom_block", kind: ItemKind::Block, block: Some(388), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:red_mushroom_block", kind: ItemKind::Block, block: Some(389), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:mushroom_stem", kind: ItemKind::Block, block: Some(390), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:iron_bars", kind: ItemKind::Block, block: Some(391), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_bars", kind: ItemKind::Block, block: Some(392), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_copper_bars", kind: ItemKind::Block, block: Some(393), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_copper_bars", kind: ItemKind::Block, block: Some(394), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_copper_bars", kind: ItemKind::Block, block: Some(395), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_copper_bars", kind: ItemKind::Block, block: Some(396), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_copper_bars", kind: ItemKind::Block, block: Some(397), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_copper_bars", kind: ItemKind::Block, block: Some(398), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_copper_bars", kind: ItemKind::Block, block: Some(399), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_chain", kind: ItemKind::Block, block: Some(400), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_chain", kind: ItemKind::Block, block: Some(401), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_copper_chain", kind: ItemKind::Block, block: Some(402), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_copper_chain", kind: ItemKind::Block, block: Some(403), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_copper_chain", kind: ItemKind::Block, block: Some(404), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_copper_chain", kind: ItemKind::Block, block: Some(405), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_copper_chain", kind: ItemKind::Block, block: Some(406), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_copper_chain", kind: ItemKind::Block, block: Some(407), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_copper_chain", kind: ItemKind::Block, block: Some(408), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:glass_pane", kind: ItemKind::Block, block: Some(409), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:melon", kind: ItemKind::Block, block: Some(411), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:vine", kind: ItemKind::Block, block: Some(416), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:glow_lichen", kind: ItemKind::Block, block: Some(417), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:resin_clump", kind: ItemKind::Block, block: Some(418), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:resin_block", kind: ItemKind::Block, block: Some(425), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:resin_bricks", kind: ItemKind::Block, block: Some(426), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:resin_brick_stairs", kind: ItemKind::Block, block: Some(427), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:resin_brick_slab", kind: ItemKind::Block, block: Some(428), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:resin_brick_wall", kind: ItemKind::Block, block: Some(429), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chiseled_resin_bricks", kind: ItemKind::Block, block: Some(430), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brick_stairs", kind: ItemKind::Block, block: Some(420), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone_brick_stairs", kind: ItemKind::Block, block: Some(421), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mud_brick_stairs", kind: ItemKind::Block, block: Some(422), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mycelium", kind: ItemKind::Block, block: Some(423), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lily_pad", kind: ItemKind::PlaceOnWater, block: Some(424), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:nether_bricks", kind: ItemKind::Block, block: Some(431), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cracked_nether_bricks", kind: ItemKind::Block, block: Some(1032), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chiseled_nether_bricks", kind: ItemKind::Block, block: Some(1031), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:nether_brick_fence", kind: ItemKind::Block, block: Some(432), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:nether_brick_stairs", kind: ItemKind::Block, block: Some(433), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sculk", kind: ItemKind::Block, block: Some(1120), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sculk_vein", kind: ItemKind::Block, block: Some(1121), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sculk_catalyst", kind: ItemKind::Block, block: Some(1122), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sculk_shrieker", kind: ItemKind::Block, block: Some(1123), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:enchanting_table", kind: ItemKind::Block, block: Some(435), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:end_portal_frame", kind: ItemKind::Block, block: Some(442), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:end_stone", kind: ItemKind::Block, block: Some(443), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:end_stone_bricks", kind: ItemKind::Block, block: Some(719), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dragon_egg", kind: ItemKind::Block, block: Some(444), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sandstone_stairs", kind: ItemKind::Block, block: Some(448), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ender_chest", kind: ItemKind::Block, block: Some(451), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:emerald_block", kind: ItemKind::Block, block: Some(454), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_stairs", kind: ItemKind::Block, block: Some(244), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_stairs", kind: ItemKind::Block, block: Some(455), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_stairs", kind: ItemKind::Block, block: Some(456), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_stairs", kind: ItemKind::Block, block: Some(457), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_stairs", kind: ItemKind::Block, block: Some(569), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_stairs", kind: ItemKind::Block, block: Some(570), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_stairs", kind: ItemKind::Block, block: Some(571), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_stairs", kind: ItemKind::Block, block: Some(572), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_stairs", kind: ItemKind::Block, block: Some(573), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_stairs", kind: ItemKind::Block, block: Some(574), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_stairs", kind: ItemKind::Block, block: Some(575), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_mosaic_stairs", kind: ItemKind::Block, block: Some(576), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_stairs", kind: ItemKind::Block, block: Some(985), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_stairs", kind: ItemKind::Block, block: Some(986), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:command_block", kind: ItemKind::GameMaster, block: Some(458), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:beacon", kind: ItemKind::Block, block: Some(459), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cobblestone_wall", kind: ItemKind::Block, block: Some(460), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mossy_cobblestone_wall", kind: ItemKind::Block, block: Some(461), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brick_wall", kind: ItemKind::Block, block: Some(914), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:prismarine_wall", kind: ItemKind::Block, block: Some(915), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_sandstone_wall", kind: ItemKind::Block, block: Some(916), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mossy_stone_brick_wall", kind: ItemKind::Block, block: Some(917), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:granite_wall", kind: ItemKind::Block, block: Some(918), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone_brick_wall", kind: ItemKind::Block, block: Some(919), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mud_brick_wall", kind: ItemKind::Block, block: Some(920), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:nether_brick_wall", kind: ItemKind::Block, block: Some(921), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:andesite_wall", kind: ItemKind::Block, block: Some(922), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_nether_brick_wall", kind: ItemKind::Block, block: Some(923), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sandstone_wall", kind: ItemKind::Block, block: Some(924), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:end_stone_brick_wall", kind: ItemKind::Block, block: Some(925), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diorite_wall", kind: ItemKind::Block, block: Some(926), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blackstone_wall", kind: ItemKind::Block, block: Some(1016), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_blackstone_wall", kind: ItemKind::Block, block: Some(1030), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_blackstone_brick_wall", kind: ItemKind::Block, block: Some(1024), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cobbled_deepslate_wall", kind: ItemKind::Block, block: Some(1245), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_deepslate_wall", kind: ItemKind::Block, block: Some(1249), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_brick_wall", kind: ItemKind::Block, block: Some(1257), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_tile_wall", kind: ItemKind::Block, block: Some(1253), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:anvil", kind: ItemKind::Block, block: Some(520), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chipped_anvil", kind: ItemKind::Block, block: Some(521), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:damaged_anvil", kind: ItemKind::Block, block: Some(522), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chiseled_quartz_block", kind: ItemKind::Block, block: Some(532), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:quartz_block", kind: ItemKind::Block, block: Some(531), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:quartz_bricks", kind: ItemKind::Block, block: Some(1033), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:quartz_pillar", kind: ItemKind::Block, block: Some(533), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:quartz_stairs", kind: ItemKind::Block, block: Some(534), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_terracotta", kind: ItemKind::Block, block: Some(537), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_terracotta", kind: ItemKind::Block, block: Some(538), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_terracotta", kind: ItemKind::Block, block: Some(539), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_terracotta", kind: ItemKind::Block, block: Some(540), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_terracotta", kind: ItemKind::Block, block: Some(541), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_terracotta", kind: ItemKind::Block, block: Some(542), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_terracotta", kind: ItemKind::Block, block: Some(543), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_terracotta", kind: ItemKind::Block, block: Some(544), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_terracotta", kind: ItemKind::Block, block: Some(545), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_terracotta", kind: ItemKind::Block, block: Some(546), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_terracotta", kind: ItemKind::Block, block: Some(547), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_terracotta", kind: ItemKind::Block, block: Some(548), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_terracotta", kind: ItemKind::Block, block: Some(549), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_terracotta", kind: ItemKind::Block, block: Some(550), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_terracotta", kind: ItemKind::Block, block: Some(551), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_terracotta", kind: ItemKind::Block, block: Some(552), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:barrier", kind: ItemKind::Block, block: Some(578), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light", kind: ItemKind::Block, block: Some(579), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:hay_block", kind: ItemKind::Block, block: Some(591), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:white_carpet", kind: ItemKind::Block, block: Some(592), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_carpet", kind: ItemKind::Block, block: Some(593), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_carpet", kind: ItemKind::Block, block: Some(594), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_carpet", kind: ItemKind::Block, block: Some(595), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_carpet", kind: ItemKind::Block, block: Some(596), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_carpet", kind: ItemKind::Block, block: Some(597), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_carpet", kind: ItemKind::Block, block: Some(598), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_carpet", kind: ItemKind::Block, block: Some(599), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_carpet", kind: ItemKind::Block, block: Some(600), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_carpet", kind: ItemKind::Block, block: Some(601), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_carpet", kind: ItemKind::Block, block: Some(602), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_carpet", kind: ItemKind::Block, block: Some(603), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_carpet", kind: ItemKind::Block, block: Some(604), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_carpet", kind: ItemKind::Block, block: Some(605), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_carpet", kind: ItemKind::Block, block: Some(606), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_carpet", kind: ItemKind::Block, block: Some(607), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:terracotta", kind: ItemKind::Block, block: Some(608), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:packed_ice", kind: ItemKind::Block, block: Some(610), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dirt_path", kind: ItemKind::Block, block: Some(724), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sunflower", kind: ItemKind::DoubleHigh, block: Some(611), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:lilac", kind: ItemKind::DoubleHigh, block: Some(612), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:rose_bush", kind: ItemKind::DoubleHigh, block: Some(613), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:peony", kind: ItemKind::DoubleHigh, block: Some(614), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:tall_grass", kind: ItemKind::DoubleHigh, block: Some(615), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:large_fern", kind: ItemKind::DoubleHigh, block: Some(616), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:white_stained_glass", kind: ItemKind::Block, block: Some(349), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_stained_glass", kind: ItemKind::Block, block: Some(350), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_stained_glass", kind: ItemKind::Block, block: Some(351), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_stained_glass", kind: ItemKind::Block, block: Some(352), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_stained_glass", kind: ItemKind::Block, block: Some(353), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_stained_glass", kind: ItemKind::Block, block: Some(354), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_stained_glass", kind: ItemKind::Block, block: Some(355), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_stained_glass", kind: ItemKind::Block, block: Some(356), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_stained_glass", kind: ItemKind::Block, block: Some(357), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_stained_glass", kind: ItemKind::Block, block: Some(358), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_stained_glass", kind: ItemKind::Block, block: Some(359), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_stained_glass", kind: ItemKind::Block, block: Some(360), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_stained_glass", kind: ItemKind::Block, block: Some(361), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_stained_glass", kind: ItemKind::Block, block: Some(362), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_stained_glass", kind: ItemKind::Block, block: Some(363), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_stained_glass", kind: ItemKind::Block, block: Some(364), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_stained_glass_pane", kind: ItemKind::Block, block: Some(553), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_stained_glass_pane", kind: ItemKind::Block, block: Some(554), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_stained_glass_pane", kind: ItemKind::Block, block: Some(555), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_stained_glass_pane", kind: ItemKind::Block, block: Some(556), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_stained_glass_pane", kind: ItemKind::Block, block: Some(557), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_stained_glass_pane", kind: ItemKind::Block, block: Some(558), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_stained_glass_pane", kind: ItemKind::Block, block: Some(559), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_stained_glass_pane", kind: ItemKind::Block, block: Some(560), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_stained_glass_pane", kind: ItemKind::Block, block: Some(561), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_stained_glass_pane", kind: ItemKind::Block, block: Some(562), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_stained_glass_pane", kind: ItemKind::Block, block: Some(563), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_stained_glass_pane", kind: ItemKind::Block, block: Some(564), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_stained_glass_pane", kind: ItemKind::Block, block: Some(565), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_stained_glass_pane", kind: ItemKind::Block, block: Some(566), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_stained_glass_pane", kind: ItemKind::Block, block: Some(567), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_stained_glass_pane", kind: ItemKind::Block, block: Some(568), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:prismarine", kind: ItemKind::Block, block: Some(581), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:prismarine_bricks", kind: ItemKind::Block, block: Some(582), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_prismarine", kind: ItemKind::Block, block: Some(583), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:prismarine_stairs", kind: ItemKind::Block, block: Some(584), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:prismarine_brick_stairs", kind: ItemKind::Block, block: Some(585), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_prismarine_stairs", kind: ItemKind::Block, block: Some(586), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sea_lantern", kind: ItemKind::Block, block: Some(590), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_sandstone", kind: ItemKind::Block, block: Some(649), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chiseled_red_sandstone", kind: ItemKind::Block, block: Some(650), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cut_red_sandstone", kind: ItemKind::Block, block: Some(651), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_sandstone_stairs", kind: ItemKind::Block, block: Some(652), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:repeating_command_block", kind: ItemKind::GameMaster, block: Some(726), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chain_command_block", kind: ItemKind::GameMaster, block: Some(727), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magma_block", kind: ItemKind::Block, block: Some(729), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:nether_wart_block", kind: ItemKind::Block, block: Some(730), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:warped_wart_block", kind: ItemKind::Block, block: Some(958), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:red_nether_bricks", kind: ItemKind::Block, block: Some(731), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bone_block", kind: ItemKind::Block, block: Some(732), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:structure_void", kind: ItemKind::Block, block: Some(733), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:shulker_box", kind: ItemKind::Block, block: Some(735), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_shulker_box", kind: ItemKind::Block, block: Some(736), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_shulker_box", kind: ItemKind::Block, block: Some(737), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_shulker_box", kind: ItemKind::Block, block: Some(738), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_shulker_box", kind: ItemKind::Block, block: Some(739), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_shulker_box", kind: ItemKind::Block, block: Some(740), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_shulker_box", kind: ItemKind::Block, block: Some(741), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_shulker_box", kind: ItemKind::Block, block: Some(742), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_shulker_box", kind: ItemKind::Block, block: Some(743), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_shulker_box", kind: ItemKind::Block, block: Some(744), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_shulker_box", kind: ItemKind::Block, block: Some(745), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_shulker_box", kind: ItemKind::Block, block: Some(746), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_shulker_box", kind: ItemKind::Block, block: Some(747), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_shulker_box", kind: ItemKind::Block, block: Some(748), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_shulker_box", kind: ItemKind::Block, block: Some(749), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_shulker_box", kind: ItemKind::Block, block: Some(750), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_shulker_box", kind: ItemKind::Block, block: Some(751), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_glazed_terracotta", kind: ItemKind::Block, block: Some(752), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_glazed_terracotta", kind: ItemKind::Block, block: Some(753), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_glazed_terracotta", kind: ItemKind::Block, block: Some(754), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_glazed_terracotta", kind: ItemKind::Block, block: Some(755), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_glazed_terracotta", kind: ItemKind::Block, block: Some(756), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_glazed_terracotta", kind: ItemKind::Block, block: Some(757), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_glazed_terracotta", kind: ItemKind::Block, block: Some(758), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_glazed_terracotta", kind: ItemKind::Block, block: Some(759), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_glazed_terracotta", kind: ItemKind::Block, block: Some(760), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_glazed_terracotta", kind: ItemKind::Block, block: Some(761), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_glazed_terracotta", kind: ItemKind::Block, block: Some(762), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_glazed_terracotta", kind: ItemKind::Block, block: Some(763), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_glazed_terracotta", kind: ItemKind::Block, block: Some(764), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_glazed_terracotta", kind: ItemKind::Block, block: Some(765), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_glazed_terracotta", kind: ItemKind::Block, block: Some(766), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_glazed_terracotta", kind: ItemKind::Block, block: Some(767), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_concrete", kind: ItemKind::Block, block: Some(768), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_concrete", kind: ItemKind::Block, block: Some(769), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_concrete", kind: ItemKind::Block, block: Some(770), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_concrete", kind: ItemKind::Block, block: Some(771), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_concrete", kind: ItemKind::Block, block: Some(772), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_concrete", kind: ItemKind::Block, block: Some(773), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_concrete", kind: ItemKind::Block, block: Some(774), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_concrete", kind: ItemKind::Block, block: Some(775), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_concrete", kind: ItemKind::Block, block: Some(776), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_concrete", kind: ItemKind::Block, block: Some(777), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_concrete", kind: ItemKind::Block, block: Some(778), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_concrete", kind: ItemKind::Block, block: Some(779), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_concrete", kind: ItemKind::Block, block: Some(780), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_concrete", kind: ItemKind::Block, block: Some(781), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_concrete", kind: ItemKind::Block, block: Some(782), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_concrete", kind: ItemKind::Block, block: Some(783), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_concrete_stairs", kind: ItemKind::Block, block: Some(784), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_concrete_stairs", kind: ItemKind::Block, block: Some(785), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_concrete_stairs", kind: ItemKind::Block, block: Some(786), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_concrete_stairs", kind: ItemKind::Block, block: Some(787), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_concrete_stairs", kind: ItemKind::Block, block: Some(788), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_concrete_stairs", kind: ItemKind::Block, block: Some(789), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_concrete_stairs", kind: ItemKind::Block, block: Some(790), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_concrete_stairs", kind: ItemKind::Block, block: Some(791), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_concrete_stairs", kind: ItemKind::Block, block: Some(792), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_concrete_stairs", kind: ItemKind::Block, block: Some(793), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_concrete_stairs", kind: ItemKind::Block, block: Some(794), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_concrete_stairs", kind: ItemKind::Block, block: Some(795), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_concrete_stairs", kind: ItemKind::Block, block: Some(796), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_concrete_stairs", kind: ItemKind::Block, block: Some(797), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_concrete_stairs", kind: ItemKind::Block, block: Some(798), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_concrete_stairs", kind: ItemKind::Block, block: Some(799), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_concrete_slab", kind: ItemKind::Block, block: Some(800), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_concrete_slab", kind: ItemKind::Block, block: Some(801), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_concrete_slab", kind: ItemKind::Block, block: Some(802), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_concrete_slab", kind: ItemKind::Block, block: Some(803), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_concrete_slab", kind: ItemKind::Block, block: Some(804), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_concrete_slab", kind: ItemKind::Block, block: Some(805), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_concrete_slab", kind: ItemKind::Block, block: Some(806), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_concrete_slab", kind: ItemKind::Block, block: Some(807), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_concrete_slab", kind: ItemKind::Block, block: Some(808), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_concrete_slab", kind: ItemKind::Block, block: Some(809), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_concrete_slab", kind: ItemKind::Block, block: Some(810), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_concrete_slab", kind: ItemKind::Block, block: Some(811), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_concrete_slab", kind: ItemKind::Block, block: Some(812), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_concrete_slab", kind: ItemKind::Block, block: Some(813), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_concrete_slab", kind: ItemKind::Block, block: Some(814), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_concrete_slab", kind: ItemKind::Block, block: Some(815), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_concrete_powder", kind: ItemKind::Block, block: Some(816), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_concrete_powder", kind: ItemKind::Block, block: Some(817), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_concrete_powder", kind: ItemKind::Block, block: Some(818), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_concrete_powder", kind: ItemKind::Block, block: Some(819), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_concrete_powder", kind: ItemKind::Block, block: Some(820), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_concrete_powder", kind: ItemKind::Block, block: Some(821), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_concrete_powder", kind: ItemKind::Block, block: Some(822), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_concrete_powder", kind: ItemKind::Block, block: Some(823), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_concrete_powder", kind: ItemKind::Block, block: Some(824), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_concrete_powder", kind: ItemKind::Block, block: Some(825), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_concrete_powder", kind: ItemKind::Block, block: Some(826), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_concrete_powder", kind: ItemKind::Block, block: Some(827), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_concrete_powder", kind: ItemKind::Block, block: Some(828), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_concrete_powder", kind: ItemKind::Block, block: Some(829), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_concrete_powder", kind: ItemKind::Block, block: Some(830), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_concrete_powder", kind: ItemKind::Block, block: Some(831), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:turtle_egg", kind: ItemKind::Block, block: Some(835), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sniffer_egg", kind: ItemKind::Block, block: Some(836), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dried_ghast", kind: ItemKind::Block, block: Some(837), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_tube_coral_block", kind: ItemKind::Block, block: Some(838), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_brain_coral_block", kind: ItemKind::Block, block: Some(839), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_bubble_coral_block", kind: ItemKind::Block, block: Some(840), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_fire_coral_block", kind: ItemKind::Block, block: Some(841), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_horn_coral_block", kind: ItemKind::Block, block: Some(842), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tube_coral_block", kind: ItemKind::Block, block: Some(843), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brain_coral_block", kind: ItemKind::Block, block: Some(844), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bubble_coral_block", kind: ItemKind::Block, block: Some(845), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:fire_coral_block", kind: ItemKind::Block, block: Some(846), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:horn_coral_block", kind: ItemKind::Block, block: Some(847), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tube_coral", kind: ItemKind::Block, block: Some(853), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brain_coral", kind: ItemKind::Block, block: Some(854), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bubble_coral", kind: ItemKind::Block, block: Some(855), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:fire_coral", kind: ItemKind::Block, block: Some(856), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:horn_coral", kind: ItemKind::Block, block: Some(857), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_brain_coral", kind: ItemKind::Block, block: Some(849), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_bubble_coral", kind: ItemKind::Block, block: Some(850), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_fire_coral", kind: ItemKind::Block, block: Some(851), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_horn_coral", kind: ItemKind::Block, block: Some(852), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_tube_coral", kind: ItemKind::Block, block: Some(848), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tube_coral_fan", kind: ItemKind::StandingAndWall, block: Some(863), wall_block: Some(873), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brain_coral_fan", kind: ItemKind::StandingAndWall, block: Some(864), wall_block: Some(874), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bubble_coral_fan", kind: ItemKind::StandingAndWall, block: Some(865), wall_block: Some(875), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:fire_coral_fan", kind: ItemKind::StandingAndWall, block: Some(866), wall_block: Some(876), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:horn_coral_fan", kind: ItemKind::StandingAndWall, block: Some(867), wall_block: Some(877), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_tube_coral_fan", kind: ItemKind::StandingAndWall, block: Some(858), wall_block: Some(868), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_brain_coral_fan", kind: ItemKind::StandingAndWall, block: Some(859), wall_block: Some(869), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_bubble_coral_fan", kind: ItemKind::StandingAndWall, block: Some(860), wall_block: Some(870), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_fire_coral_fan", kind: ItemKind::StandingAndWall, block: Some(861), wall_block: Some(871), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dead_horn_coral_fan", kind: ItemKind::StandingAndWall, block: Some(862), wall_block: Some(872), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_ice", kind: ItemKind::Block, block: Some(879), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:conduit", kind: ItemKind::Block, block: Some(880), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_granite_stairs", kind: ItemKind::Block, block: Some(887), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:smooth_red_sandstone_stairs", kind: ItemKind::Block, block: Some(888), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mossy_stone_brick_stairs", kind: ItemKind::Block, block: Some(889), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_diorite_stairs", kind: ItemKind::Block, block: Some(890), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mossy_cobblestone_stairs", kind: ItemKind::Block, block: Some(891), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:end_stone_brick_stairs", kind: ItemKind::Block, block: Some(892), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone_stairs", kind: ItemKind::Block, block: Some(893), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:smooth_sandstone_stairs", kind: ItemKind::Block, block: Some(894), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:smooth_quartz_stairs", kind: ItemKind::Block, block: Some(895), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:granite_stairs", kind: ItemKind::Block, block: Some(896), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:andesite_stairs", kind: ItemKind::Block, block: Some(897), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_nether_brick_stairs", kind: ItemKind::Block, block: Some(898), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_andesite_stairs", kind: ItemKind::Block, block: Some(899), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diorite_stairs", kind: ItemKind::Block, block: Some(900), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cobbled_deepslate_stairs", kind: ItemKind::Block, block: Some(1243), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_deepslate_stairs", kind: ItemKind::Block, block: Some(1247), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_brick_stairs", kind: ItemKind::Block, block: Some(1255), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_tile_stairs", kind: ItemKind::Block, block: Some(1251), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_granite_slab", kind: ItemKind::Block, block: Some(901), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:smooth_red_sandstone_slab", kind: ItemKind::Block, block: Some(902), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mossy_stone_brick_slab", kind: ItemKind::Block, block: Some(903), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_diorite_slab", kind: ItemKind::Block, block: Some(904), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mossy_cobblestone_slab", kind: ItemKind::Block, block: Some(905), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:end_stone_brick_slab", kind: ItemKind::Block, block: Some(906), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:smooth_sandstone_slab", kind: ItemKind::Block, block: Some(907), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:smooth_quartz_slab", kind: ItemKind::Block, block: Some(908), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:granite_slab", kind: ItemKind::Block, block: Some(909), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:andesite_slab", kind: ItemKind::Block, block: Some(910), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_nether_brick_slab", kind: ItemKind::Block, block: Some(911), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_andesite_slab", kind: ItemKind::Block, block: Some(912), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diorite_slab", kind: ItemKind::Block, block: Some(913), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cobbled_deepslate_slab", kind: ItemKind::Block, block: Some(1244), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_deepslate_slab", kind: ItemKind::Block, block: Some(1248), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_brick_slab", kind: ItemKind::Block, block: Some(1256), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:deepslate_tile_slab", kind: ItemKind::Block, block: Some(1252), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:scaffolding", kind: ItemKind::Scaffolding, block: Some(927), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:redstone", kind: ItemKind::Block, block: Some(246), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:redstone_torch", kind: ItemKind::StandingAndWall, block: Some(322), wall_block: Some(323), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:redstone_block", kind: ItemKind::Block, block: Some(528), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:repeater", kind: ItemKind::Block, block: Some(348), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:comparator", kind: ItemKind::Block, block: Some(526), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:piston", kind: ItemKind::Block, block: Some(149), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sticky_piston", kind: ItemKind::Block, block: Some(138), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:slime_block", kind: ItemKind::Block, block: Some(577), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:honey_block", kind: ItemKind::Block, block: Some(1003), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:observer", kind: ItemKind::Block, block: Some(734), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:hopper", kind: ItemKind::Block, block: Some(530), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dispenser", kind: ItemKind::Block, block: Some(114), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dropper", kind: ItemKind::Block, block: Some(536), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lectern", kind: ItemKind::Block, block: Some(935), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:target", kind: ItemKind::Block, block: Some(1000), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lever", kind: ItemKind::Block, block: Some(306), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lightning_rod", kind: ItemKind::Block, block: Some(1214), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_lightning_rod", kind: ItemKind::Block, block: Some(1215), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_lightning_rod", kind: ItemKind::Block, block: Some(1216), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_lightning_rod", kind: ItemKind::Block, block: Some(1217), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_lightning_rod", kind: ItemKind::Block, block: Some(1218), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_lightning_rod", kind: ItemKind::Block, block: Some(1219), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_lightning_rod", kind: ItemKind::Block, block: Some(1220), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_lightning_rod", kind: ItemKind::Block, block: Some(1221), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:daylight_detector", kind: ItemKind::Block, block: Some(527), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sculk_sensor", kind: ItemKind::Block, block: Some(1118), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:calibrated_sculk_sensor", kind: ItemKind::Block, block: Some(1119), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tripwire_hook", kind: ItemKind::Block, block: Some(452), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:trapped_chest", kind: ItemKind::Block, block: Some(523), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tnt", kind: ItemKind::Block, block: Some(220), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:redstone_lamp", kind: ItemKind::Block, block: Some(445), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:note_block", kind: ItemKind::Block, block: Some(118), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone_button", kind: ItemKind::Block, block: Some(324), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_blackstone_button", kind: ItemKind::Block, block: Some(1029), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_button", kind: ItemKind::Block, block: Some(495), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_button", kind: ItemKind::Block, block: Some(496), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_button", kind: ItemKind::Block, block: Some(497), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_button", kind: ItemKind::Block, block: Some(498), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_button", kind: ItemKind::Block, block: Some(499), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_button", kind: ItemKind::Block, block: Some(500), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_button", kind: ItemKind::Block, block: Some(501), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_button", kind: ItemKind::Block, block: Some(502), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_button", kind: ItemKind::Block, block: Some(503), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_button", kind: ItemKind::Block, block: Some(504), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_button", kind: ItemKind::Block, block: Some(505), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_button", kind: ItemKind::Block, block: Some(987), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_button", kind: ItemKind::Block, block: Some(988), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone_pressure_plate", kind: ItemKind::Block, block: Some(307), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_blackstone_pressure_plate", kind: ItemKind::Block, block: Some(1028), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_weighted_pressure_plate", kind: ItemKind::Block, block: Some(524), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:heavy_weighted_pressure_plate", kind: ItemKind::Block, block: Some(525), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_pressure_plate", kind: ItemKind::Block, block: Some(309), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_pressure_plate", kind: ItemKind::Block, block: Some(310), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_pressure_plate", kind: ItemKind::Block, block: Some(311), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_pressure_plate", kind: ItemKind::Block, block: Some(312), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_pressure_plate", kind: ItemKind::Block, block: Some(313), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_pressure_plate", kind: ItemKind::Block, block: Some(314), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_pressure_plate", kind: ItemKind::Block, block: Some(315), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_pressure_plate", kind: ItemKind::Block, block: Some(316), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_pressure_plate", kind: ItemKind::Block, block: Some(317), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_pressure_plate", kind: ItemKind::Block, block: Some(318), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_pressure_plate", kind: ItemKind::Block, block: Some(319), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_pressure_plate", kind: ItemKind::Block, block: Some(977), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_pressure_plate", kind: ItemKind::Block, block: Some(978), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_door", kind: ItemKind::DoubleHigh, block: Some(308), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_door", kind: ItemKind::DoubleHigh, block: Some(265), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_door", kind: ItemKind::DoubleHigh, block: Some(702), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_door", kind: ItemKind::DoubleHigh, block: Some(703), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_door", kind: ItemKind::DoubleHigh, block: Some(704), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_door", kind: ItemKind::DoubleHigh, block: Some(705), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_door", kind: ItemKind::DoubleHigh, block: Some(706), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_door", kind: ItemKind::DoubleHigh, block: Some(707), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_door", kind: ItemKind::DoubleHigh, block: Some(708), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_door", kind: ItemKind::DoubleHigh, block: Some(709), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_door", kind: ItemKind::DoubleHigh, block: Some(710), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_door", kind: ItemKind::DoubleHigh, block: Some(711), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_door", kind: ItemKind::DoubleHigh, block: Some(989), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_door", kind: ItemKind::DoubleHigh, block: Some(990), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_door", kind: ItemKind::DoubleHigh, block: Some(1166), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_copper_door", kind: ItemKind::DoubleHigh, block: Some(1167), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_copper_door", kind: ItemKind::DoubleHigh, block: Some(1168), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_copper_door", kind: ItemKind::DoubleHigh, block: Some(1169), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_copper_door", kind: ItemKind::DoubleHigh, block: Some(1170), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_copper_door", kind: ItemKind::DoubleHigh, block: Some(1171), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_copper_door", kind: ItemKind::DoubleHigh, block: Some(1172), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_copper_door", kind: ItemKind::DoubleHigh, block: Some(1173), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_trapdoor", kind: ItemKind::Block, block: Some(580), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_trapdoor", kind: ItemKind::Block, block: Some(365), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_trapdoor", kind: ItemKind::Block, block: Some(366), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_trapdoor", kind: ItemKind::Block, block: Some(367), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_trapdoor", kind: ItemKind::Block, block: Some(368), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_trapdoor", kind: ItemKind::Block, block: Some(369), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_trapdoor", kind: ItemKind::Block, block: Some(370), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_trapdoor", kind: ItemKind::Block, block: Some(371), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_trapdoor", kind: ItemKind::Block, block: Some(372), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_trapdoor", kind: ItemKind::Block, block: Some(373), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_trapdoor", kind: ItemKind::Block, block: Some(374), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_trapdoor", kind: ItemKind::Block, block: Some(375), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_trapdoor", kind: ItemKind::Block, block: Some(981), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_trapdoor", kind: ItemKind::Block, block: Some(982), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_trapdoor", kind: ItemKind::Block, block: Some(1174), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_copper_trapdoor", kind: ItemKind::Block, block: Some(1175), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_copper_trapdoor", kind: ItemKind::Block, block: Some(1176), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_copper_trapdoor", kind: ItemKind::Block, block: Some(1177), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_copper_trapdoor", kind: ItemKind::Block, block: Some(1178), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_copper_trapdoor", kind: ItemKind::Block, block: Some(1179), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_copper_trapdoor", kind: ItemKind::Block, block: Some(1180), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_copper_trapdoor", kind: ItemKind::Block, block: Some(1181), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_fence_gate", kind: ItemKind::Block, block: Some(419), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_fence_gate", kind: ItemKind::Block, block: Some(682), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_fence_gate", kind: ItemKind::Block, block: Some(683), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_fence_gate", kind: ItemKind::Block, block: Some(684), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_fence_gate", kind: ItemKind::Block, block: Some(685), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_fence_gate", kind: ItemKind::Block, block: Some(686), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_fence_gate", kind: ItemKind::Block, block: Some(687), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_fence_gate", kind: ItemKind::Block, block: Some(688), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_fence_gate", kind: ItemKind::Block, block: Some(689), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_fence_gate", kind: ItemKind::Block, block: Some(690), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_fence_gate", kind: ItemKind::Block, block: Some(691), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_fence_gate", kind: ItemKind::Block, block: Some(983), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_fence_gate", kind: ItemKind::Block, block: Some(984), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:powered_rail", kind: ItemKind::Block, block: Some(136), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:detector_rail", kind: ItemKind::Block, block: Some(137), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:rail", kind: ItemKind::Block, block: Some(267), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:activator_rail", kind: ItemKind::Block, block: Some(535), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:saddle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_harness", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:minecart", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chest_minecart", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:furnace_minecart", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tnt_minecart", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:hopper_minecart", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:carrot_on_a_stick", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_fungus_on_a_stick", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:phantom_membrane", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:elytra", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_chest_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_chest_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_chest_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_chest_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_chest_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_chest_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_chest_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_chest_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_chest_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_chest_boat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_raft", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_chest_raft", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:structure_block", kind: ItemKind::GameMaster, block: Some(995), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jigsaw", kind: ItemKind::GameMaster, block: Some(996), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:test_block", kind: ItemKind::GameMaster, block: Some(997), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:test_instance_block", kind: ItemKind::GameMaster, block: Some(998), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:turtle_helmet", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:turtle_scute", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:armadillo_scute", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wolf_armor", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:flint_and_steel", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bowl", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:apple", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:bow", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:arrow", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:coal", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:charcoal", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:emerald", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lapis_lazuli", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:quartz", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:amethyst_shard", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:raw_iron", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_ingot", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:raw_copper", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_ingot", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:raw_gold", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gold_ingot", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_ingot", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_scrap", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wooden_sword", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: false, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wooden_shovel", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wooden_pickaxe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wooden_axe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wooden_hoe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_sword", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: false, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_shovel", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_pickaxe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_axe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_hoe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone_sword", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: false, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone_shovel", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone_pickaxe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone_axe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone_hoe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:golden_sword", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: false, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:golden_shovel", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:golden_pickaxe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:golden_axe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:golden_hoe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_sword", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: false, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_shovel", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_pickaxe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_axe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_hoe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond_sword", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: false, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond_shovel", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond_pickaxe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond_axe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond_hoe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_sword", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: false, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_shovel", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_pickaxe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_axe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_hoe", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stick", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mushroom_stew", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:string", kind: ItemKind::Block, block: Some(453), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:feather", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gunpowder", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wheat_seeds", kind: ItemKind::Block, block: Some(251), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:wheat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:bread", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:leather_helmet", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:leather_chestplate", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:leather_leggings", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:leather_boots", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_helmet", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_chestplate", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_leggings", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_boots", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chainmail_helmet", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chainmail_chestplate", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chainmail_leggings", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chainmail_boots", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_helmet", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_chestplate", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_leggings", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_boots", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond_helmet", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond_chestplate", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond_leggings", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond_boots", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:golden_helmet", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:golden_chestplate", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:golden_leggings", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:golden_boots", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_helmet", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_chestplate", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_leggings", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_boots", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:flint", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:porkchop", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cooked_porkchop", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:painting", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:golden_apple", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:enchanted_golden_apple", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_sign", kind: ItemKind::StandingAndWall, block: Some(254), wall_block: Some(269), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_sign", kind: ItemKind::StandingAndWall, block: Some(255), wall_block: Some(270), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_sign", kind: ItemKind::StandingAndWall, block: Some(256), wall_block: Some(271), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_sign", kind: ItemKind::StandingAndWall, block: Some(259), wall_block: Some(274), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_sign", kind: ItemKind::StandingAndWall, block: Some(257), wall_block: Some(272), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_sign", kind: ItemKind::StandingAndWall, block: Some(258), wall_block: Some(273), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_sign", kind: ItemKind::StandingAndWall, block: Some(260), wall_block: Some(275), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_sign", kind: ItemKind::StandingAndWall, block: Some(261), wall_block: Some(276), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_sign", kind: ItemKind::StandingAndWall, block: Some(262), wall_block: Some(277), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_sign", kind: ItemKind::StandingAndWall, block: Some(263), wall_block: Some(278), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_sign", kind: ItemKind::StandingAndWall, block: Some(264), wall_block: Some(279), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_sign", kind: ItemKind::StandingAndWall, block: Some(991), wall_block: Some(993), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_sign", kind: ItemKind::StandingAndWall, block: Some(992), wall_block: Some(994), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oak_hanging_sign", kind: ItemKind::HangingSign, block: Some(280), wall_block: Some(293), attachment: Some(1), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spruce_hanging_sign", kind: ItemKind::HangingSign, block: Some(281), wall_block: Some(294), attachment: Some(1), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:birch_hanging_sign", kind: ItemKind::HangingSign, block: Some(282), wall_block: Some(295), attachment: Some(1), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_hanging_sign", kind: ItemKind::HangingSign, block: Some(285), wall_block: Some(298), attachment: Some(1), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:acacia_hanging_sign", kind: ItemKind::HangingSign, block: Some(283), wall_block: Some(296), attachment: Some(1), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cherry_hanging_sign", kind: ItemKind::HangingSign, block: Some(284), wall_block: Some(297), attachment: Some(1), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dark_oak_hanging_sign", kind: ItemKind::HangingSign, block: Some(286), wall_block: Some(299), attachment: Some(1), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pale_oak_hanging_sign", kind: ItemKind::HangingSign, block: Some(287), wall_block: Some(300), attachment: Some(1), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mangrove_hanging_sign", kind: ItemKind::HangingSign, block: Some(290), wall_block: Some(301), attachment: Some(1), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:poplar_hanging_sign", kind: ItemKind::HangingSign, block: Some(291), wall_block: Some(302), attachment: Some(1), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bamboo_hanging_sign", kind: ItemKind::HangingSign, block: Some(292), wall_block: Some(305), attachment: Some(1), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crimson_hanging_sign", kind: ItemKind::HangingSign, block: Some(288), wall_block: Some(303), attachment: Some(1), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warped_hanging_sign", kind: ItemKind::HangingSign, block: Some(289), wall_block: Some(304), attachment: Some(1), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bucket", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:water_bucket", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lava_bucket", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:powder_snow_bucket", kind: ItemKind::SolidBucket, block: Some(1117), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:snowball", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:leather", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:milk_bucket", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pufferfish_bucket", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:salmon_bucket", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cod_bucket", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tropical_fish_bucket", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:axolotl_bucket", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sulfur_cube_bucket", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tadpole_bucket", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brick", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:clay_ball", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dried_kelp_block", kind: ItemKind::Block, block: Some(834), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:paper", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:book", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:slime_ball", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:compass", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:recovery_compass", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_bundle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_cushion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:fishing_rod", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:clock", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spyglass", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:glowstone_dust", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cod", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:salmon", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tropical_fish", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pufferfish", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cooked_cod", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cooked_salmon", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ink_sac", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:glow_ink_sac", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cocoa_beans", kind: ItemKind::Block, block: Some(446), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:white_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_dye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bone_meal", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bone", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sugar", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cake", kind: ItemKind::Block, block: Some(347), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:white_bed", kind: ItemKind::Block, block: Some(119), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_bed", kind: ItemKind::Block, block: Some(120), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_bed", kind: ItemKind::Block, block: Some(121), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_bed", kind: ItemKind::Block, block: Some(122), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_bed", kind: ItemKind::Block, block: Some(123), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_bed", kind: ItemKind::Block, block: Some(124), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_bed", kind: ItemKind::Block, block: Some(125), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_bed", kind: ItemKind::Block, block: Some(126), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_bed", kind: ItemKind::Block, block: Some(127), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_bed", kind: ItemKind::Block, block: Some(128), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_bed", kind: ItemKind::Block, block: Some(129), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_bed", kind: ItemKind::Block, block: Some(130), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_bed", kind: ItemKind::Block, block: Some(131), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_bed", kind: ItemKind::Block, block: Some(132), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_bed", kind: ItemKind::Block, block: Some(133), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_bed", kind: ItemKind::Block, block: Some(134), wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:straw_bed", kind: ItemKind::Block, block: Some(135), wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:cookie", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:crafter", kind: ItemKind::Block, block: Some(1274), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:filled_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ocean_monument_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:woodland_mansion_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:buried_trial_chambers_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:jungle_pyramid_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:swamp_hut_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:desert_village_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:plains_village_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:savanna_village_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:snowy_village_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:taiga_village_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:buried_treasure_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:buried_ancient_city_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:buried_mineshaft_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:desert_pyramid_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:abandoned_camp_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warm_ocean_ruins_map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:shears", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:melon_slice", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:dried_kelp", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:pumpkin_seeds", kind: ItemKind::Block, block: Some(414), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:melon_seeds", kind: ItemKind::Block, block: Some(415), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:beef", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cooked_beef", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chicken", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cooked_chicken", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:rotten_flesh", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ender_pearl", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blaze_rod", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ghast_tear", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gold_nugget", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:nether_wart", kind: ItemKind::Block, block: Some(434), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:glass_bottle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:potion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spider_eye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:fermented_spider_eye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blaze_powder", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magma_cream", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brewing_stand", kind: ItemKind::Block, block: Some(436), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cauldron", kind: ItemKind::Block, block: Some(437), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ender_eye", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:glistering_melon_slice", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chicken_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cow_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pig_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sheep_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:camel_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:donkey_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:horse_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mule_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cat_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:parrot_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wolf_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:armadillo_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bat_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bee_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:fox_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:goat_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:llama_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ocelot_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:panda_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polar_bear_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:rabbit_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:axolotl_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cod_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dolphin_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:frog_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:glow_squid_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:nautilus_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pufferfish_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:salmon_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:squid_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tadpole_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tropical_fish_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:turtle_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:allay_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mooshroom_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sniffer_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sulfur_cube_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_golem_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_golem_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:snow_golem_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:trader_llama_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:villager_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wandering_trader_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bogged_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:camel_husk_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:drowned_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:husk_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:parched_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:skeleton_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:skeleton_horse_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stray_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wither_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wither_skeleton_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:zombie_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:zombie_horse_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:zombie_nautilus_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:zombie_villager_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cave_spider_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spider_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:breeze_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:creaking_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:creeper_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:elder_guardian_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:guardian_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:phantom_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:silverfish_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:slime_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:warden_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:witch_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:evoker_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pillager_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ravager_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:vindicator_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:vex_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blaze_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ghast_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:happy_ghast_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:hoglin_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magma_cube_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:piglin_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:piglin_brute_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:strider_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:zoglin_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:zombified_piglin_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ender_dragon_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:enderman_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:endermite_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:shulker_spawn_egg", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:experience_bottle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:fire_charge", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wind_charge", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:writable_book", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:written_book", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:breeze_rod", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mace", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: false, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:item_frame", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:glow_item_frame", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:flower_pot", kind: ItemKind::Block, block: Some(462), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:carrot", kind: ItemKind::Block, block: Some(493), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:potato", kind: ItemKind::Block, block: Some(494), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:baked_potato", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:poisonous_potato", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:map", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:golden_carrot", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:skeleton_skull", kind: ItemKind::StandingAndWall, block: Some(506), wall_block: Some(507), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wither_skeleton_skull", kind: ItemKind::StandingAndWall, block: Some(508), wall_block: Some(509), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:player_head", kind: ItemKind::PlayerHead, block: Some(512), wall_block: Some(513), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:zombie_head", kind: ItemKind::StandingAndWall, block: Some(510), wall_block: Some(511), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:creeper_head", kind: ItemKind::StandingAndWall, block: Some(514), wall_block: Some(515), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dragon_head", kind: ItemKind::StandingAndWall, block: Some(516), wall_block: Some(517), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:piglin_head", kind: ItemKind::StandingAndWall, block: Some(518), wall_block: Some(519), attachment: Some(0), max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:nether_star", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pumpkin_pie", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:firework_rocket", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:firework_star", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:enchanted_book", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:nether_brick", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:resin_brick", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:prismarine_shard", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:prismarine_crystals", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:rabbit", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cooked_rabbit", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:rabbit_stew", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:rabbit_foot", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:rabbit_hide", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:armor_stand", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_horse_armor", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_horse_armor", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:golden_horse_armor", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond_horse_armor", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_horse_armor", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:leather_horse_armor", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lead", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:name_tag", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:command_block_minecart", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mutton", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cooked_mutton", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_banner", kind: ItemKind::Banner, block: Some(617), wall_block: Some(633), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_banner", kind: ItemKind::Banner, block: Some(618), wall_block: Some(634), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_banner", kind: ItemKind::Banner, block: Some(619), wall_block: Some(635), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_banner", kind: ItemKind::Banner, block: Some(620), wall_block: Some(636), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_banner", kind: ItemKind::Banner, block: Some(621), wall_block: Some(637), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_banner", kind: ItemKind::Banner, block: Some(622), wall_block: Some(638), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_banner", kind: ItemKind::Banner, block: Some(623), wall_block: Some(639), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_banner", kind: ItemKind::Banner, block: Some(624), wall_block: Some(640), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_banner", kind: ItemKind::Banner, block: Some(625), wall_block: Some(641), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_banner", kind: ItemKind::Banner, block: Some(626), wall_block: Some(642), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_banner", kind: ItemKind::Banner, block: Some(627), wall_block: Some(643), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_banner", kind: ItemKind::Banner, block: Some(628), wall_block: Some(644), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_banner", kind: ItemKind::Banner, block: Some(629), wall_block: Some(645), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_banner", kind: ItemKind::Banner, block: Some(630), wall_block: Some(646), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_banner", kind: ItemKind::Banner, block: Some(631), wall_block: Some(647), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_banner", kind: ItemKind::Banner, block: Some(632), wall_block: Some(648), attachment: Some(0), max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:end_crystal", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chorus_fruit", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:popped_chorus_fruit", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:torchflower_seeds", kind: ItemKind::Block, block: Some(720), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:pitcher_pod", kind: ItemKind::Block, block: Some(721), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:beetroot", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:beetroot_seeds", kind: ItemKind::Block, block: Some(723), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:beetroot_soup", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dragon_breath", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:splash_potion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spectral_arrow", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tipped_arrow", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lingering_potion", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:shield", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wooden_spear", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stone_spear", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_spear", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_spear", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:golden_spear", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond_spear", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_spear", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:totem_of_undying", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:shulker_shell", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_nugget", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_nugget", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:knowledge_book", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:debug_stick", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: false, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:music_disc_13", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_cat", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_blocks", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_bounce", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_chirp", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_creator", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_creator_music_box", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_far", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_lava_chicken", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_mall", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_mellohi", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_stal", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_strad", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_ward", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_11", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_wait", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_otherside", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_relic", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_5", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_pigstep", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_precipice", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:music_disc_tears", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: true },
    Item { name: "minecraft:disc_fragment_5", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:trident", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: false, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:nautilus_shell", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:iron_nautilus_armor", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:golden_nautilus_armor", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:diamond_nautilus_armor", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_nautilus_armor", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_nautilus_armor", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:heart_of_the_sea", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crossbow", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:suspicious_stew", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:loom", kind: ItemKind::Block, block: Some(928), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:flower_banner_pattern", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:creeper_banner_pattern", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:skull_banner_pattern", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mojang_banner_pattern", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:globe_banner_pattern", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:piglin_banner_pattern", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:flow_banner_pattern", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:guster_banner_pattern", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:field_masoned_banner_pattern", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bordure_indented_banner_pattern", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:goat_horn", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:composter", kind: ItemKind::Block, block: Some(999), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:barrel", kind: ItemKind::Block, block: Some(929), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:smoker", kind: ItemKind::Block, block: Some(930), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blast_furnace", kind: ItemKind::Block, block: Some(931), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cartography_table", kind: ItemKind::Block, block: Some(932), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:fletching_table", kind: ItemKind::Block, block: Some(933), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:grindstone", kind: ItemKind::Block, block: Some(934), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:smithing_table", kind: ItemKind::Block, block: Some(936), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:stonecutter", kind: ItemKind::Block, block: Some(937), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bell", kind: ItemKind::Block, block: Some(938), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lantern", kind: ItemKind::Block, block: Some(939), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:soul_lantern", kind: ItemKind::Block, block: Some(940), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_lantern", kind: ItemKind::Block, block: Some(941), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_copper_lantern", kind: ItemKind::Block, block: Some(942), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_copper_lantern", kind: ItemKind::Block, block: Some(943), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_copper_lantern", kind: ItemKind::Block, block: Some(944), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_copper_lantern", kind: ItemKind::Block, block: Some(945), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_copper_lantern", kind: ItemKind::Block, block: Some(946), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_copper_lantern", kind: ItemKind::Block, block: Some(947), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_copper_lantern", kind: ItemKind::Block, block: Some(948), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sweet_berries", kind: ItemKind::Block, block: Some(951), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:glow_berries", kind: ItemKind::Block, block: Some(1225), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:campfire", kind: ItemKind::Block, block: Some(949), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:soul_campfire", kind: ItemKind::Block, block: Some(950), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:shroomlight", kind: ItemKind::Block, block: Some(967), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: true, jukebox_playable: false },
    Item { name: "minecraft:honeycomb", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bee_nest", kind: ItemKind::Block, block: Some(1001), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:beehive", kind: ItemKind::Block, block: Some(1002), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:honey_bottle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 16, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:honeycomb_block", kind: ItemKind::Block, block: Some(1004), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lodestone", kind: ItemKind::Block, block: Some(1013), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:crying_obsidian", kind: ItemKind::Block, block: Some(1007), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blackstone", kind: ItemKind::Block, block: Some(1014), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blackstone_slab", kind: ItemKind::Block, block: Some(1017), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blackstone_stairs", kind: ItemKind::Block, block: Some(1015), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gilded_blackstone", kind: ItemKind::Block, block: Some(1025), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_blackstone", kind: ItemKind::Block, block: Some(1018), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_blackstone_slab", kind: ItemKind::Block, block: Some(1027), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_blackstone_stairs", kind: ItemKind::Block, block: Some(1026), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:chiseled_polished_blackstone", kind: ItemKind::Block, block: Some(1021), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_blackstone_bricks", kind: ItemKind::Block, block: Some(1019), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_blackstone_brick_slab", kind: ItemKind::Block, block: Some(1022), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:polished_blackstone_brick_stairs", kind: ItemKind::Block, block: Some(1023), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cracked_polished_blackstone_bricks", kind: ItemKind::Block, block: Some(1020), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:respawn_anchor", kind: ItemKind::Block, block: Some(1008), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:candle", kind: ItemKind::Block, block: Some(1034), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:white_candle", kind: ItemKind::Block, block: Some(1035), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:orange_candle", kind: ItemKind::Block, block: Some(1036), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:magenta_candle", kind: ItemKind::Block, block: Some(1037), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_blue_candle", kind: ItemKind::Block, block: Some(1038), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:yellow_candle", kind: ItemKind::Block, block: Some(1039), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:lime_candle", kind: ItemKind::Block, block: Some(1040), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pink_candle", kind: ItemKind::Block, block: Some(1041), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:gray_candle", kind: ItemKind::Block, block: Some(1042), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:light_gray_candle", kind: ItemKind::Block, block: Some(1043), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:cyan_candle", kind: ItemKind::Block, block: Some(1044), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:purple_candle", kind: ItemKind::Block, block: Some(1045), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blue_candle", kind: ItemKind::Block, block: Some(1046), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brown_candle", kind: ItemKind::Block, block: Some(1047), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:green_candle", kind: ItemKind::Block, block: Some(1048), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:red_candle", kind: ItemKind::Block, block: Some(1049), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:black_candle", kind: ItemKind::Block, block: Some(1050), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:small_amethyst_bud", kind: ItemKind::Block, block: Some(1073), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:medium_amethyst_bud", kind: ItemKind::Block, block: Some(1072), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:large_amethyst_bud", kind: ItemKind::Block, block: Some(1071), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:amethyst_cluster", kind: ItemKind::Block, block: Some(1070), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pointed_dripstone", kind: ItemKind::Block, block: Some(1223), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sulfur_spike", kind: ItemKind::Block, block: Some(1224), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ochre_froglight", kind: ItemKind::Block, block: Some(1268), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:verdant_froglight", kind: ItemKind::Block, block: Some(1269), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:pearlescent_froglight", kind: ItemKind::Block, block: Some(1270), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:frogspawn", kind: ItemKind::PlaceOnWater, block: Some(1271), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:echo_shard", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brush", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 1, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:netherite_upgrade_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sentry_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:dune_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:coast_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wild_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ward_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:eye_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:vex_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:tide_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:snout_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:rib_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:spire_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:wayfinder_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:shaper_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:silence_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:raiser_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:host_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:flow_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:bolt_armor_trim_smithing_template", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:angler_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:archer_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:arms_up_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:blade_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:brewer_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:burn_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:danger_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:explorer_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:flow_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:friend_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:guster_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:heart_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:heartbreak_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:howl_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:miner_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:mourner_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:plenty_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:prize_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:scrape_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:sheaf_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:shelter_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:skull_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:snort_pottery_sherd", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_grate", kind: ItemKind::Block, block: Some(1182), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_copper_grate", kind: ItemKind::Block, block: Some(1183), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_copper_grate", kind: ItemKind::Block, block: Some(1184), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_copper_grate", kind: ItemKind::Block, block: Some(1185), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_copper_grate", kind: ItemKind::Block, block: Some(1186), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_copper_grate", kind: ItemKind::Block, block: Some(1187), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_copper_grate", kind: ItemKind::Block, block: Some(1188), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_copper_grate", kind: ItemKind::Block, block: Some(1189), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_bulb", kind: ItemKind::Block, block: Some(1190), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_copper_bulb", kind: ItemKind::Block, block: Some(1191), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_copper_bulb", kind: ItemKind::Block, block: Some(1192), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_copper_bulb", kind: ItemKind::Block, block: Some(1193), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_copper_bulb", kind: ItemKind::Block, block: Some(1194), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_copper_bulb", kind: ItemKind::Block, block: Some(1195), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_copper_bulb", kind: ItemKind::Block, block: Some(1196), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_copper_bulb", kind: ItemKind::Block, block: Some(1197), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_chest", kind: ItemKind::Block, block: Some(1198), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_copper_chest", kind: ItemKind::Block, block: Some(1199), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_copper_chest", kind: ItemKind::Block, block: Some(1200), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_copper_chest", kind: ItemKind::Block, block: Some(1201), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_copper_chest", kind: ItemKind::Block, block: Some(1202), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_copper_chest", kind: ItemKind::Block, block: Some(1203), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_copper_chest", kind: ItemKind::Block, block: Some(1204), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_copper_chest", kind: ItemKind::Block, block: Some(1205), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:copper_golem_statue", kind: ItemKind::Block, block: Some(1206), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:exposed_copper_golem_statue", kind: ItemKind::Block, block: Some(1207), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:weathered_copper_golem_statue", kind: ItemKind::Block, block: Some(1208), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:oxidized_copper_golem_statue", kind: ItemKind::Block, block: Some(1209), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_copper_golem_statue", kind: ItemKind::Block, block: Some(1210), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_exposed_copper_golem_statue", kind: ItemKind::Block, block: Some(1211), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_weathered_copper_golem_statue", kind: ItemKind::Block, block: Some(1212), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:waxed_oxidized_copper_golem_statue", kind: ItemKind::Block, block: Some(1213), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:trial_spawner", kind: ItemKind::Block, block: Some(1275), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:trial_key", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ominous_trial_key", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:vault", kind: ItemKind::Block, block: Some(1276), wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: false, compostable: false, jukebox_playable: false },
    Item { name: "minecraft:ominous_bottle", kind: ItemKind::Other, block: None, wall_block: None, attachment: None, max_stack_size: 64, can_destroy_blocks_in_creative: true, consumable: true, compostable: false, jukebox_playable: false },
];
