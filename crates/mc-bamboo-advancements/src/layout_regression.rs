//! 两组由对拍工具抓到的出错兄弟顺序的回归测试：固定输入、断言官方坐标。
//!
//! 这两组顺序在修复 `apportion` 的 defaultAncestor 返回值之前分别让
//! `minecraft:husbandry/uh_oh` 与 `minecraft:adventure/heart_transplanter`
//! 的 y 比官方偏小。坐标是官方 26.3 的 `TreeNodePosition` 算出来的，
//! 用 f64 存成 JSON 数字，测试里按 f32 比较（和在线对拍一致）。
//!
//! Regression tests for two captured failing sibling orders: fixed input,
//! asserting the official coordinates. Before the defaultAncestor fix these
//! orders placed `husbandry/uh_oh` and `adventure/heart_transplanter` too high.

use crate::{AdvancementTree, TreeOrder};

/// 官方顺序摘要 bafbba63da32，出错节点 minecraft:husbandry/uh_oh（ours 1, official 1.25）。
const UH_OH_ORDER: &[(&str, &[&str])] = &[
    (
        "minecraft:adventure/kill_a_mob",
        &[
            "minecraft:adventure/shoot_arrow",
            "minecraft:adventure/throw_trident",
            "minecraft:adventure/spear_many_mobs",
            "minecraft:adventure/totem_of_undying",
            "minecraft:adventure/kill_all_mobs",
            "minecraft:adventure/kill_mob_near_sculk_catalyst",
        ],
    ),
    (
        "minecraft:adventure/minecraft_trials_edition",
        &[
            "minecraft:adventure/under_lock_and_key",
            "minecraft:adventure/blowback",
            "minecraft:adventure/lighten_up",
            "minecraft:adventure/who_needs_rockets",
            "minecraft:adventure/overoverkill",
        ],
    ),
    (
        "minecraft:adventure/ol_betsy",
        &[
            "minecraft:adventure/two_birds_one_arrow",
            "minecraft:adventure/whos_the_pillager_now",
            "minecraft:adventure/arbalistic",
        ],
    ),
    (
        "minecraft:adventure/root",
        &[
            "minecraft:adventure/honey_block_slide",
            "minecraft:adventure/salvage_sherd",
            "minecraft:adventure/crafters_crafting_crafters",
            "minecraft:adventure/lightning_rod_with_villager_no_fire",
            "minecraft:adventure/use_lodestone",
            "minecraft:adventure/brush_armadillo",
            "minecraft:adventure/trim_with_any_armor_pattern",
            "minecraft:adventure/kill_a_mob",
            "minecraft:adventure/trade",
            "minecraft:adventure/spyglass_at_parrot",
            "minecraft:adventure/read_power_of_chiseled_bookshelf",
            "minecraft:adventure/ol_betsy",
            "minecraft:adventure/avoid_vibration",
            "minecraft:adventure/voluntary_exile",
            "minecraft:adventure/minecraft_trials_edition",
            "minecraft:adventure/fall_from_world_height",
            "minecraft:adventure/heart_transplanter",
            "minecraft:adventure/sleep_in_bed",
        ],
    ),
    (
        "minecraft:adventure/salvage_sherd",
        &["minecraft:adventure/craft_decorated_pot_using_only_sherds"],
    ),
    (
        "minecraft:adventure/shoot_arrow",
        &[
            "minecraft:adventure/sniper_duel",
            "minecraft:adventure/bullseye",
        ],
    ),
    (
        "minecraft:adventure/sleep_in_bed",
        &[
            "minecraft:adventure/adventuring_time",
            "minecraft:adventure/play_jukebox_in_meadows",
            "minecraft:adventure/walk_on_powder_snow_with_leather_boots",
        ],
    ),
    (
        "minecraft:adventure/spyglass_at_ghast",
        &["minecraft:adventure/spyglass_at_dragon"],
    ),
    (
        "minecraft:adventure/spyglass_at_parrot",
        &["minecraft:adventure/spyglass_at_ghast"],
    ),
    (
        "minecraft:adventure/throw_trident",
        &["minecraft:adventure/very_very_frightening"],
    ),
    (
        "minecraft:adventure/trade",
        &[
            "minecraft:adventure/summon_iron_golem",
            "minecraft:adventure/trade_at_world_height",
        ],
    ),
    (
        "minecraft:adventure/trim_with_any_armor_pattern",
        &["minecraft:adventure/trim_with_all_exclusive_armor_patterns"],
    ),
    (
        "minecraft:adventure/under_lock_and_key",
        &["minecraft:adventure/revaulting"],
    ),
    (
        "minecraft:adventure/voluntary_exile",
        &["minecraft:adventure/hero_of_the_village"],
    ),
    (
        "minecraft:end/enter_end_gateway",
        &["minecraft:end/find_end_city"],
    ),
    (
        "minecraft:end/find_end_city",
        &["minecraft:end/levitate", "minecraft:end/elytra"],
    ),
    (
        "minecraft:end/kill_dragon",
        &[
            "minecraft:end/respawn_dragon",
            "minecraft:end/dragon_breath",
            "minecraft:end/dragon_egg",
            "minecraft:end/enter_end_gateway",
        ],
    ),
    ("minecraft:end/root", &["minecraft:end/kill_dragon"]),
    (
        "minecraft:husbandry/allay_deliver_item_to_player",
        &["minecraft:husbandry/allay_deliver_cake_to_note_block"],
    ),
    (
        "minecraft:husbandry/axolotl_in_a_bucket",
        &["minecraft:husbandry/kill_axolotl_target"],
    ),
    (
        "minecraft:husbandry/breed_an_animal",
        &["minecraft:husbandry/bred_all_animals"],
    ),
    (
        "minecraft:husbandry/feed_snifflet",
        &["minecraft:husbandry/plant_any_sniffer_seed"],
    ),
    (
        "minecraft:husbandry/fishy_business",
        &["minecraft:husbandry/tactical_fishing"],
    ),
    (
        "minecraft:husbandry/leash_all_frog_variants",
        &["minecraft:husbandry/froglights"],
    ),
    (
        "minecraft:husbandry/obtain_sniffer_egg",
        &["minecraft:husbandry/feed_snifflet"],
    ),
    (
        "minecraft:husbandry/plant_seed",
        &[
            "minecraft:husbandry/obtain_netherite_hoe",
            "minecraft:husbandry/balanced_diet",
        ],
    ),
    (
        "minecraft:husbandry/root",
        &[
            "minecraft:husbandry/breed_an_animal",
            "minecraft:husbandry/uh_oh",
            "minecraft:husbandry/tame_an_animal",
            "minecraft:husbandry/place_dried_ghast_in_water",
            "minecraft:husbandry/make_a_sign_glow",
            "minecraft:husbandry/safely_harvest_honey",
            "minecraft:husbandry/ride_a_boat_with_a_goat",
            "minecraft:husbandry/plant_seed",
            "minecraft:husbandry/tadpole_in_a_bucket",
            "minecraft:husbandry/fishy_business",
            "minecraft:husbandry/obtain_sniffer_egg",
            "minecraft:husbandry/allay_deliver_item_to_player",
            "minecraft:husbandry/silk_touch_nest",
        ],
    ),
    (
        "minecraft:husbandry/safely_harvest_honey",
        &["minecraft:husbandry/wax_on"],
    ),
    (
        "minecraft:husbandry/tactical_fishing",
        &["minecraft:husbandry/axolotl_in_a_bucket"],
    ),
    (
        "minecraft:husbandry/tadpole_in_a_bucket",
        &["minecraft:husbandry/leash_all_frog_variants"],
    ),
    (
        "minecraft:husbandry/tame_an_animal",
        &[
            "minecraft:husbandry/repair_wolf_armor",
            "minecraft:husbandry/complete_catalogue",
            "minecraft:husbandry/remove_wolf_armor",
            "minecraft:husbandry/whole_pack",
        ],
    ),
    (
        "minecraft:husbandry/wax_on",
        &["minecraft:husbandry/wax_off"],
    ),
    (
        "minecraft:nether/all_potions",
        &["minecraft:nether/all_effects"],
    ),
    (
        "minecraft:nether/brew_potion",
        &["minecraft:nether/all_potions"],
    ),
    (
        "minecraft:nether/create_beacon",
        &["minecraft:nether/create_full_beacon"],
    ),
    (
        "minecraft:nether/find_bastion",
        &["minecraft:nether/loot_bastion"],
    ),
    (
        "minecraft:nether/find_fortress",
        &[
            "minecraft:nether/get_wither_skull",
            "minecraft:nether/obtain_blaze_rod",
        ],
    ),
    (
        "minecraft:nether/get_wither_skull",
        &["minecraft:nether/summon_wither"],
    ),
    (
        "minecraft:nether/obtain_ancient_debris",
        &["minecraft:nether/netherite_armor"],
    ),
    (
        "minecraft:nether/obtain_blaze_rod",
        &["minecraft:nether/brew_potion"],
    ),
    (
        "minecraft:nether/obtain_crying_obsidian",
        &["minecraft:nether/charge_respawn_anchor"],
    ),
    (
        "minecraft:nether/return_to_sender",
        &["minecraft:nether/uneasy_alliance"],
    ),
    (
        "minecraft:nether/ride_strider",
        &[
            "minecraft:nether/explore_nether",
            "minecraft:nether/ride_strider_in_overworld_lava",
        ],
    ),
    (
        "minecraft:nether/root",
        &[
            "minecraft:nether/ride_strider",
            "minecraft:nether/find_fortress",
            "minecraft:nether/obtain_ancient_debris",
            "minecraft:nether/find_bastion",
            "minecraft:nether/fast_travel",
            "minecraft:nether/return_to_sender",
            "minecraft:nether/distract_piglin",
            "minecraft:nether/obtain_crying_obsidian",
        ],
    ),
    (
        "minecraft:nether/summon_wither",
        &["minecraft:nether/create_beacon"],
    ),
    (
        "minecraft:story/enter_the_nether",
        &[
            "minecraft:story/cure_zombie_villager",
            "minecraft:story/follow_ender_eye",
        ],
    ),
    (
        "minecraft:story/follow_ender_eye",
        &["minecraft:story/enter_the_end"],
    ),
    (
        "minecraft:story/form_obsidian",
        &["minecraft:story/enter_the_nether"],
    ),
    (
        "minecraft:story/iron_tools",
        &["minecraft:story/mine_diamond"],
    ),
    (
        "minecraft:story/lava_bucket",
        &["minecraft:story/form_obsidian"],
    ),
    (
        "minecraft:story/mine_diamond",
        &["minecraft:story/shiny_gear", "minecraft:story/enchant_item"],
    ),
    (
        "minecraft:story/mine_stone",
        &["minecraft:story/upgrade_tools"],
    ),
    (
        "minecraft:story/obtain_armor",
        &["minecraft:story/deflect_arrow"],
    ),
    ("minecraft:story/root", &["minecraft:story/mine_stone"]),
    (
        "minecraft:story/smelt_iron",
        &[
            "minecraft:story/obtain_armor",
            "minecraft:story/iron_tools",
            "minecraft:story/lava_bucket",
        ],
    ),
    (
        "minecraft:story/upgrade_tools",
        &["minecraft:story/smelt_iron"],
    ),
];

/// bafbba63da32 的全部官方显示坐标 (id, x, y)。
const UH_OH_POSITIONS: &[(&str, f64, f64)] = &[
    ("minecraft:adventure/adventuring_time", 2.0, 25.5),
    ("minecraft:adventure/arbalistic", 2.0, 18.5),
    ("minecraft:adventure/avoid_vibration", 1.0, 18.5),
    ("minecraft:adventure/blowback", 2.0, 21.5),
    ("minecraft:adventure/brush_armadillo", 1.0, 5.0),
    ("minecraft:adventure/bullseye", 3.0, 7.5),
    (
        "minecraft:adventure/craft_decorated_pot_using_only_sherds",
        2.0,
        1.0,
    ),
    ("minecraft:adventure/crafters_crafting_crafters", 1.0, 2.0),
    ("minecraft:adventure/fall_from_world_height", 1.0, 23.833334),
    ("minecraft:adventure/heart_transplanter", 1.0, 25.166666),
    ("minecraft:adventure/hero_of_the_village", 2.0, 19.5),
    ("minecraft:adventure/honey_block_slide", 1.0, 0.0),
    ("minecraft:adventure/kill_a_mob", 1.0, 9.75),
    ("minecraft:adventure/kill_all_mobs", 2.0, 11.5),
    (
        "minecraft:adventure/kill_mob_near_sculk_catalyst",
        2.0,
        12.5,
    ),
    ("minecraft:adventure/lighten_up", 2.0, 22.5),
    (
        "minecraft:adventure/lightning_rod_with_villager_no_fire",
        1.0,
        3.0,
    ),
    ("minecraft:adventure/minecraft_trials_edition", 1.0, 22.5),
    ("minecraft:adventure/ol_betsy", 1.0, 17.5),
    ("minecraft:adventure/overoverkill", 2.0, 24.5),
    ("minecraft:adventure/play_jukebox_in_meadows", 2.0, 26.5),
    (
        "minecraft:adventure/read_power_of_chiseled_bookshelf",
        1.0,
        16.5,
    ),
    ("minecraft:adventure/revaulting", 3.0, 20.5),
    ("minecraft:adventure/root", 0.0, 13.25),
    ("minecraft:adventure/salvage_sherd", 1.0, 1.0),
    ("minecraft:adventure/shoot_arrow", 2.0, 7.0),
    ("minecraft:adventure/sleep_in_bed", 1.0, 26.5),
    ("minecraft:adventure/sniper_duel", 3.0, 6.5),
    ("minecraft:adventure/spear_many_mobs", 2.0, 9.5),
    ("minecraft:adventure/spyglass_at_dragon", 3.0, 15.5),
    ("minecraft:adventure/spyglass_at_ghast", 2.0, 15.5),
    ("minecraft:adventure/spyglass_at_parrot", 1.0, 15.5),
    ("minecraft:adventure/summon_iron_golem", 2.0, 13.5),
    ("minecraft:adventure/throw_trident", 2.0, 8.5),
    ("minecraft:adventure/totem_of_undying", 2.0, 10.5),
    ("minecraft:adventure/trade", 1.0, 14.0),
    ("minecraft:adventure/trade_at_world_height", 2.0, 14.5),
    (
        "minecraft:adventure/trim_with_all_exclusive_armor_patterns",
        2.0,
        6.0,
    ),
    ("minecraft:adventure/trim_with_any_armor_pattern", 1.0, 6.0),
    ("minecraft:adventure/two_birds_one_arrow", 2.0, 16.5),
    ("minecraft:adventure/under_lock_and_key", 2.0, 20.5),
    ("minecraft:adventure/use_lodestone", 1.0, 4.0),
    ("minecraft:adventure/very_very_frightening", 3.0, 8.5),
    ("minecraft:adventure/voluntary_exile", 1.0, 19.5),
    (
        "minecraft:adventure/walk_on_powder_snow_with_leather_boots",
        2.0,
        27.5,
    ),
    ("minecraft:adventure/who_needs_rockets", 2.0, 23.5),
    ("minecraft:adventure/whos_the_pillager_now", 2.0, 17.5),
    ("minecraft:end/dragon_breath", 2.0, 1.0),
    ("minecraft:end/dragon_egg", 2.0, 2.0),
    ("minecraft:end/elytra", 4.0, 3.5),
    ("minecraft:end/enter_end_gateway", 2.0, 3.0),
    ("minecraft:end/find_end_city", 3.0, 3.0),
    ("minecraft:end/kill_dragon", 1.0, 1.5),
    ("minecraft:end/levitate", 4.0, 2.5),
    ("minecraft:end/respawn_dragon", 2.0, 0.0),
    ("minecraft:end/root", 0.0, 1.5),
    (
        "minecraft:husbandry/allay_deliver_cake_to_note_block",
        2.0,
        12.0,
    ),
    (
        "minecraft:husbandry/allay_deliver_item_to_player",
        1.0,
        12.0,
    ),
    ("minecraft:husbandry/axolotl_in_a_bucket", 3.0, 10.0),
    ("minecraft:husbandry/balanced_diet", 2.0, 8.0),
    ("minecraft:husbandry/bred_all_animals", 2.0, 0.0),
    ("minecraft:husbandry/breed_an_animal", 1.0, 0.0),
    ("minecraft:husbandry/complete_catalogue", 2.0, 2.0),
    ("minecraft:husbandry/feed_snifflet", 2.0, 11.0),
    ("minecraft:husbandry/fishy_business", 1.0, 10.0),
    ("minecraft:husbandry/froglights", 3.0, 9.0),
    ("minecraft:husbandry/kill_axolotl_target", 4.0, 10.0),
    ("minecraft:husbandry/leash_all_frog_variants", 2.0, 9.0),
    ("minecraft:husbandry/make_a_sign_glow", 1.0, 4.5),
    ("minecraft:husbandry/obtain_netherite_hoe", 2.0, 7.0),
    ("minecraft:husbandry/obtain_sniffer_egg", 1.0, 11.0),
    ("minecraft:husbandry/place_dried_ghast_in_water", 1.0, 3.5),
    ("minecraft:husbandry/plant_any_sniffer_seed", 3.0, 11.0),
    ("minecraft:husbandry/plant_seed", 1.0, 7.5),
    ("minecraft:husbandry/remove_wolf_armor", 2.0, 3.0),
    ("minecraft:husbandry/repair_wolf_armor", 2.0, 1.0),
    ("minecraft:husbandry/ride_a_boat_with_a_goat", 1.0, 6.5),
    ("minecraft:husbandry/root", 0.0, 6.5),
    ("minecraft:husbandry/safely_harvest_honey", 1.0, 5.5),
    ("minecraft:husbandry/silk_touch_nest", 1.0, 13.0),
    ("minecraft:husbandry/tactical_fishing", 2.0, 10.0),
    ("minecraft:husbandry/tadpole_in_a_bucket", 1.0, 9.0),
    ("minecraft:husbandry/tame_an_animal", 1.0, 2.5),
    ("minecraft:husbandry/uh_oh", 1.0, 1.25),
    ("minecraft:husbandry/wax_off", 3.0, 5.5),
    ("minecraft:husbandry/wax_on", 2.0, 5.5),
    ("minecraft:husbandry/whole_pack", 2.0, 4.0),
    ("minecraft:nether/all_effects", 5.0, 3.0),
    ("minecraft:nether/all_potions", 4.0, 3.0),
    ("minecraft:nether/brew_potion", 3.0, 3.0),
    ("minecraft:nether/charge_respawn_anchor", 2.0, 9.0),
    ("minecraft:nether/create_beacon", 4.0, 2.0),
    ("minecraft:nether/create_full_beacon", 5.0, 2.0),
    ("minecraft:nether/distract_piglin", 1.0, 8.0),
    ("minecraft:nether/explore_nether", 2.0, 0.0),
    ("minecraft:nether/fast_travel", 1.0, 6.0),
    ("minecraft:nether/find_bastion", 1.0, 5.0),
    ("minecraft:nether/find_fortress", 1.0, 2.5),
    ("minecraft:nether/get_wither_skull", 2.0, 2.0),
    ("minecraft:nether/loot_bastion", 2.0, 5.0),
    ("minecraft:nether/netherite_armor", 2.0, 4.0),
    ("minecraft:nether/obtain_ancient_debris", 1.0, 4.0),
    ("minecraft:nether/obtain_blaze_rod", 2.0, 3.0),
    ("minecraft:nether/obtain_crying_obsidian", 1.0, 9.0),
    ("minecraft:nether/return_to_sender", 1.0, 7.0),
    ("minecraft:nether/ride_strider", 1.0, 0.5),
    ("minecraft:nether/ride_strider_in_overworld_lava", 2.0, 1.0),
    ("minecraft:nether/root", 0.0, 4.75),
    ("minecraft:nether/summon_wither", 3.0, 2.0),
    ("minecraft:nether/uneasy_alliance", 2.0, 7.0),
    ("minecraft:story/cure_zombie_villager", 7.0, 2.0),
    ("minecraft:story/deflect_arrow", 5.0, 0.0),
    ("minecraft:story/enchant_item", 6.0, 1.5),
    ("minecraft:story/enter_the_end", 8.0, 3.0),
    ("minecraft:story/enter_the_nether", 6.0, 2.5),
    ("minecraft:story/follow_ender_eye", 7.0, 3.0),
    ("minecraft:story/form_obsidian", 5.0, 2.5),
    ("minecraft:story/iron_tools", 4.0, 1.0),
    ("minecraft:story/lava_bucket", 4.0, 2.5),
    ("minecraft:story/mine_diamond", 5.0, 1.0),
    ("minecraft:story/mine_stone", 1.0, 1.25),
    ("minecraft:story/obtain_armor", 4.0, 0.0),
    ("minecraft:story/root", 0.0, 1.25),
    ("minecraft:story/shiny_gear", 6.0, 0.5),
    ("minecraft:story/smelt_iron", 3.0, 1.25),
    ("minecraft:story/upgrade_tools", 2.0, 1.25),
];

/// 官方顺序摘要 c15f3b068c59，出错节点 minecraft:adventure/heart_transplanter（ours 8.25, official 8.5）。
const HEART_ORDER: &[(&str, &[&str])] = &[
    (
        "minecraft:adventure/kill_a_mob",
        &[
            "minecraft:adventure/kill_mob_near_sculk_catalyst",
            "minecraft:adventure/totem_of_undying",
            "minecraft:adventure/spear_many_mobs",
            "minecraft:adventure/throw_trident",
            "minecraft:adventure/kill_all_mobs",
            "minecraft:adventure/shoot_arrow",
        ],
    ),
    (
        "minecraft:adventure/minecraft_trials_edition",
        &[
            "minecraft:adventure/lighten_up",
            "minecraft:adventure/overoverkill",
            "minecraft:adventure/blowback",
            "minecraft:adventure/who_needs_rockets",
            "minecraft:adventure/under_lock_and_key",
        ],
    ),
    (
        "minecraft:adventure/ol_betsy",
        &[
            "minecraft:adventure/arbalistic",
            "minecraft:adventure/whos_the_pillager_now",
            "minecraft:adventure/two_birds_one_arrow",
        ],
    ),
    (
        "minecraft:adventure/root",
        &[
            "minecraft:adventure/fall_from_world_height",
            "minecraft:adventure/use_lodestone",
            "minecraft:adventure/ol_betsy",
            "minecraft:adventure/kill_a_mob",
            "minecraft:adventure/heart_transplanter",
            "minecraft:adventure/spyglass_at_parrot",
            "minecraft:adventure/minecraft_trials_edition",
            "minecraft:adventure/lightning_rod_with_villager_no_fire",
            "minecraft:adventure/brush_armadillo",
            "minecraft:adventure/avoid_vibration",
            "minecraft:adventure/salvage_sherd",
            "minecraft:adventure/read_power_of_chiseled_bookshelf",
            "minecraft:adventure/voluntary_exile",
            "minecraft:adventure/honey_block_slide",
            "minecraft:adventure/crafters_crafting_crafters",
            "minecraft:adventure/trim_with_any_armor_pattern",
            "minecraft:adventure/sleep_in_bed",
            "minecraft:adventure/trade",
        ],
    ),
    (
        "minecraft:adventure/salvage_sherd",
        &["minecraft:adventure/craft_decorated_pot_using_only_sherds"],
    ),
    (
        "minecraft:adventure/shoot_arrow",
        &[
            "minecraft:adventure/bullseye",
            "minecraft:adventure/sniper_duel",
        ],
    ),
    (
        "minecraft:adventure/sleep_in_bed",
        &[
            "minecraft:adventure/walk_on_powder_snow_with_leather_boots",
            "minecraft:adventure/adventuring_time",
            "minecraft:adventure/play_jukebox_in_meadows",
        ],
    ),
    (
        "minecraft:adventure/spyglass_at_ghast",
        &["minecraft:adventure/spyglass_at_dragon"],
    ),
    (
        "minecraft:adventure/spyglass_at_parrot",
        &["minecraft:adventure/spyglass_at_ghast"],
    ),
    (
        "minecraft:adventure/throw_trident",
        &["minecraft:adventure/very_very_frightening"],
    ),
    (
        "minecraft:adventure/trade",
        &[
            "minecraft:adventure/summon_iron_golem",
            "minecraft:adventure/trade_at_world_height",
        ],
    ),
    (
        "minecraft:adventure/trim_with_any_armor_pattern",
        &["minecraft:adventure/trim_with_all_exclusive_armor_patterns"],
    ),
    (
        "minecraft:adventure/under_lock_and_key",
        &["minecraft:adventure/revaulting"],
    ),
    (
        "minecraft:adventure/voluntary_exile",
        &["minecraft:adventure/hero_of_the_village"],
    ),
    (
        "minecraft:end/enter_end_gateway",
        &["minecraft:end/find_end_city"],
    ),
    (
        "minecraft:end/find_end_city",
        &["minecraft:end/elytra", "minecraft:end/levitate"],
    ),
    (
        "minecraft:end/kill_dragon",
        &[
            "minecraft:end/dragon_breath",
            "minecraft:end/respawn_dragon",
            "minecraft:end/dragon_egg",
            "minecraft:end/enter_end_gateway",
        ],
    ),
    ("minecraft:end/root", &["minecraft:end/kill_dragon"]),
    (
        "minecraft:husbandry/allay_deliver_item_to_player",
        &["minecraft:husbandry/allay_deliver_cake_to_note_block"],
    ),
    (
        "minecraft:husbandry/axolotl_in_a_bucket",
        &["minecraft:husbandry/kill_axolotl_target"],
    ),
    (
        "minecraft:husbandry/breed_an_animal",
        &["minecraft:husbandry/bred_all_animals"],
    ),
    (
        "minecraft:husbandry/feed_snifflet",
        &["minecraft:husbandry/plant_any_sniffer_seed"],
    ),
    (
        "minecraft:husbandry/fishy_business",
        &["minecraft:husbandry/tactical_fishing"],
    ),
    (
        "minecraft:husbandry/leash_all_frog_variants",
        &["minecraft:husbandry/froglights"],
    ),
    (
        "minecraft:husbandry/obtain_sniffer_egg",
        &["minecraft:husbandry/feed_snifflet"],
    ),
    (
        "minecraft:husbandry/plant_seed",
        &[
            "minecraft:husbandry/balanced_diet",
            "minecraft:husbandry/obtain_netherite_hoe",
        ],
    ),
    (
        "minecraft:husbandry/root",
        &[
            "minecraft:husbandry/silk_touch_nest",
            "minecraft:husbandry/plant_seed",
            "minecraft:husbandry/place_dried_ghast_in_water",
            "minecraft:husbandry/allay_deliver_item_to_player",
            "minecraft:husbandry/obtain_sniffer_egg",
            "minecraft:husbandry/fishy_business",
            "minecraft:husbandry/uh_oh",
            "minecraft:husbandry/breed_an_animal",
            "minecraft:husbandry/tadpole_in_a_bucket",
            "minecraft:husbandry/safely_harvest_honey",
            "minecraft:husbandry/ride_a_boat_with_a_goat",
            "minecraft:husbandry/tame_an_animal",
            "minecraft:husbandry/make_a_sign_glow",
        ],
    ),
    (
        "minecraft:husbandry/safely_harvest_honey",
        &["minecraft:husbandry/wax_on"],
    ),
    (
        "minecraft:husbandry/tactical_fishing",
        &["minecraft:husbandry/axolotl_in_a_bucket"],
    ),
    (
        "minecraft:husbandry/tadpole_in_a_bucket",
        &["minecraft:husbandry/leash_all_frog_variants"],
    ),
    (
        "minecraft:husbandry/tame_an_animal",
        &[
            "minecraft:husbandry/repair_wolf_armor",
            "minecraft:husbandry/whole_pack",
            "minecraft:husbandry/remove_wolf_armor",
            "minecraft:husbandry/complete_catalogue",
        ],
    ),
    (
        "minecraft:husbandry/wax_on",
        &["minecraft:husbandry/wax_off"],
    ),
    (
        "minecraft:nether/all_potions",
        &["minecraft:nether/all_effects"],
    ),
    (
        "minecraft:nether/brew_potion",
        &["minecraft:nether/all_potions"],
    ),
    (
        "minecraft:nether/create_beacon",
        &["minecraft:nether/create_full_beacon"],
    ),
    (
        "minecraft:nether/find_bastion",
        &["minecraft:nether/loot_bastion"],
    ),
    (
        "minecraft:nether/find_fortress",
        &[
            "minecraft:nether/get_wither_skull",
            "minecraft:nether/obtain_blaze_rod",
        ],
    ),
    (
        "minecraft:nether/get_wither_skull",
        &["minecraft:nether/summon_wither"],
    ),
    (
        "minecraft:nether/obtain_ancient_debris",
        &["minecraft:nether/netherite_armor"],
    ),
    (
        "minecraft:nether/obtain_blaze_rod",
        &["minecraft:nether/brew_potion"],
    ),
    (
        "minecraft:nether/obtain_crying_obsidian",
        &["minecraft:nether/charge_respawn_anchor"],
    ),
    (
        "minecraft:nether/return_to_sender",
        &["minecraft:nether/uneasy_alliance"],
    ),
    (
        "minecraft:nether/ride_strider",
        &[
            "minecraft:nether/ride_strider_in_overworld_lava",
            "minecraft:nether/explore_nether",
        ],
    ),
    (
        "minecraft:nether/root",
        &[
            "minecraft:nether/ride_strider",
            "minecraft:nether/distract_piglin",
            "minecraft:nether/find_bastion",
            "minecraft:nether/obtain_crying_obsidian",
            "minecraft:nether/obtain_ancient_debris",
            "minecraft:nether/return_to_sender",
            "minecraft:nether/fast_travel",
            "minecraft:nether/find_fortress",
        ],
    ),
    (
        "minecraft:nether/summon_wither",
        &["minecraft:nether/create_beacon"],
    ),
    (
        "minecraft:story/enter_the_nether",
        &[
            "minecraft:story/cure_zombie_villager",
            "minecraft:story/follow_ender_eye",
        ],
    ),
    (
        "minecraft:story/follow_ender_eye",
        &["minecraft:story/enter_the_end"],
    ),
    (
        "minecraft:story/form_obsidian",
        &["minecraft:story/enter_the_nether"],
    ),
    (
        "minecraft:story/iron_tools",
        &["minecraft:story/mine_diamond"],
    ),
    (
        "minecraft:story/lava_bucket",
        &["minecraft:story/form_obsidian"],
    ),
    (
        "minecraft:story/mine_diamond",
        &["minecraft:story/shiny_gear", "minecraft:story/enchant_item"],
    ),
    (
        "minecraft:story/mine_stone",
        &["minecraft:story/upgrade_tools"],
    ),
    (
        "minecraft:story/obtain_armor",
        &["minecraft:story/deflect_arrow"],
    ),
    ("minecraft:story/root", &["minecraft:story/mine_stone"]),
    (
        "minecraft:story/smelt_iron",
        &[
            "minecraft:story/obtain_armor",
            "minecraft:story/lava_bucket",
            "minecraft:story/iron_tools",
        ],
    ),
    (
        "minecraft:story/upgrade_tools",
        &["minecraft:story/smelt_iron"],
    ),
];

/// c15f3b068c59 的全部官方显示坐标 (id, x, y)。
const HEART_POSITIONS: &[(&str, f64, f64)] = &[
    ("minecraft:adventure/adventuring_time", 2.0, 24.5),
    ("minecraft:adventure/arbalistic", 2.0, 1.0),
    ("minecraft:adventure/avoid_vibration", 1.0, 16.5),
    ("minecraft:adventure/blowback", 2.0, 13.5),
    ("minecraft:adventure/brush_armadillo", 1.0, 15.5),
    ("minecraft:adventure/bullseye", 3.0, 8.5),
    (
        "minecraft:adventure/craft_decorated_pot_using_only_sherds",
        2.0,
        17.5,
    ),
    ("minecraft:adventure/crafters_crafting_crafters", 1.0, 21.5),
    ("minecraft:adventure/fall_from_world_height", 1.0, 0.0),
    ("minecraft:adventure/heart_transplanter", 1.0, 8.5),
    ("minecraft:adventure/hero_of_the_village", 2.0, 19.5),
    ("minecraft:adventure/honey_block_slide", 1.0, 20.5),
    ("minecraft:adventure/kill_a_mob", 1.0, 6.5),
    ("minecraft:adventure/kill_all_mobs", 2.0, 8.0),
    ("minecraft:adventure/kill_mob_near_sculk_catalyst", 2.0, 4.0),
    ("minecraft:adventure/lighten_up", 2.0, 11.5),
    (
        "minecraft:adventure/lightning_rod_with_villager_no_fire",
        1.0,
        14.5,
    ),
    ("minecraft:adventure/minecraft_trials_edition", 1.0, 13.5),
    ("minecraft:adventure/ol_betsy", 1.0, 2.0),
    ("minecraft:adventure/overoverkill", 2.0, 12.5),
    ("minecraft:adventure/play_jukebox_in_meadows", 2.0, 25.5),
    (
        "minecraft:adventure/read_power_of_chiseled_bookshelf",
        1.0,
        18.5,
    ),
    ("minecraft:adventure/revaulting", 3.0, 15.5),
    ("minecraft:adventure/root", 0.0, 13.5),
    ("minecraft:adventure/salvage_sherd", 1.0, 17.5),
    ("minecraft:adventure/shoot_arrow", 2.0, 9.0),
    ("minecraft:adventure/sleep_in_bed", 1.0, 24.5),
    ("minecraft:adventure/sniper_duel", 3.0, 9.5),
    ("minecraft:adventure/spear_many_mobs", 2.0, 6.0),
    ("minecraft:adventure/spyglass_at_dragon", 3.0, 10.5),
    ("minecraft:adventure/spyglass_at_ghast", 2.0, 10.5),
    ("minecraft:adventure/spyglass_at_parrot", 1.0, 10.5),
    ("minecraft:adventure/summon_iron_golem", 2.0, 26.5),
    ("minecraft:adventure/throw_trident", 2.0, 7.0),
    ("minecraft:adventure/totem_of_undying", 2.0, 5.0),
    ("minecraft:adventure/trade", 1.0, 27.0),
    ("minecraft:adventure/trade_at_world_height", 2.0, 27.5),
    (
        "minecraft:adventure/trim_with_all_exclusive_armor_patterns",
        2.0,
        22.5,
    ),
    ("minecraft:adventure/trim_with_any_armor_pattern", 1.0, 22.5),
    ("minecraft:adventure/two_birds_one_arrow", 2.0, 3.0),
    ("minecraft:adventure/under_lock_and_key", 2.0, 15.5),
    ("minecraft:adventure/use_lodestone", 1.0, 1.0),
    ("minecraft:adventure/very_very_frightening", 3.0, 7.0),
    ("minecraft:adventure/voluntary_exile", 1.0, 19.5),
    (
        "minecraft:adventure/walk_on_powder_snow_with_leather_boots",
        2.0,
        23.5,
    ),
    ("minecraft:adventure/who_needs_rockets", 2.0, 14.5),
    ("minecraft:adventure/whos_the_pillager_now", 2.0, 2.0),
    ("minecraft:end/dragon_breath", 2.0, 0.0),
    ("minecraft:end/dragon_egg", 2.0, 2.0),
    ("minecraft:end/elytra", 4.0, 2.5),
    ("minecraft:end/enter_end_gateway", 2.0, 3.0),
    ("minecraft:end/find_end_city", 3.0, 3.0),
    ("minecraft:end/kill_dragon", 1.0, 1.5),
    ("minecraft:end/levitate", 4.0, 3.5),
    ("minecraft:end/respawn_dragon", 2.0, 1.0),
    ("minecraft:end/root", 0.0, 1.5),
    (
        "minecraft:husbandry/allay_deliver_cake_to_note_block",
        2.0,
        3.0,
    ),
    ("minecraft:husbandry/allay_deliver_item_to_player", 1.0, 3.0),
    ("minecraft:husbandry/axolotl_in_a_bucket", 3.0, 5.0),
    ("minecraft:husbandry/balanced_diet", 2.0, 0.5),
    ("minecraft:husbandry/bred_all_animals", 2.0, 7.0),
    ("minecraft:husbandry/breed_an_animal", 1.0, 7.0),
    ("minecraft:husbandry/complete_catalogue", 2.0, 13.0),
    ("minecraft:husbandry/feed_snifflet", 2.0, 4.0),
    ("minecraft:husbandry/fishy_business", 1.0, 5.0),
    ("minecraft:husbandry/froglights", 3.0, 8.0),
    ("minecraft:husbandry/kill_axolotl_target", 4.0, 5.0),
    ("minecraft:husbandry/leash_all_frog_variants", 2.0, 8.0),
    ("minecraft:husbandry/make_a_sign_glow", 1.0, 12.5),
    ("minecraft:husbandry/obtain_netherite_hoe", 2.0, 1.5),
    ("minecraft:husbandry/obtain_sniffer_egg", 1.0, 4.0),
    ("minecraft:husbandry/place_dried_ghast_in_water", 1.0, 2.0),
    ("minecraft:husbandry/plant_any_sniffer_seed", 3.0, 4.0),
    ("minecraft:husbandry/plant_seed", 1.0, 1.0),
    ("minecraft:husbandry/remove_wolf_armor", 2.0, 12.0),
    ("minecraft:husbandry/repair_wolf_armor", 2.0, 10.0),
    ("minecraft:husbandry/ride_a_boat_with_a_goat", 1.0, 10.25),
    ("minecraft:husbandry/root", 0.0, 6.25),
    ("minecraft:husbandry/safely_harvest_honey", 1.0, 9.0),
    ("minecraft:husbandry/silk_touch_nest", 1.0, 0.0),
    ("minecraft:husbandry/tactical_fishing", 2.0, 5.0),
    ("minecraft:husbandry/tadpole_in_a_bucket", 1.0, 8.0),
    ("minecraft:husbandry/tame_an_animal", 1.0, 11.5),
    ("minecraft:husbandry/uh_oh", 1.0, 6.0),
    ("minecraft:husbandry/wax_off", 3.0, 9.0),
    ("minecraft:husbandry/wax_on", 2.0, 9.0),
    ("minecraft:husbandry/whole_pack", 2.0, 11.0),
    ("minecraft:nether/all_effects", 5.0, 8.0),
    ("minecraft:nether/all_potions", 4.0, 8.0),
    ("minecraft:nether/brew_potion", 3.0, 8.0),
    ("minecraft:nether/charge_respawn_anchor", 2.0, 3.5),
    ("minecraft:nether/create_beacon", 4.0, 7.0),
    ("minecraft:nether/create_full_beacon", 5.0, 7.0),
    ("minecraft:nether/distract_piglin", 1.0, 1.5),
    ("minecraft:nether/explore_nether", 2.0, 1.0),
    ("minecraft:nether/fast_travel", 1.0, 6.5),
    ("minecraft:nether/find_bastion", 1.0, 2.5),
    ("minecraft:nether/find_fortress", 1.0, 7.5),
    ("minecraft:nether/get_wither_skull", 2.0, 7.0),
    ("minecraft:nether/loot_bastion", 2.0, 2.5),
    ("minecraft:nether/netherite_armor", 2.0, 4.5),
    ("minecraft:nether/obtain_ancient_debris", 1.0, 4.5),
    ("minecraft:nether/obtain_blaze_rod", 2.0, 8.0),
    ("minecraft:nether/obtain_crying_obsidian", 1.0, 3.5),
    ("minecraft:nether/return_to_sender", 1.0, 5.5),
    ("minecraft:nether/ride_strider", 1.0, 0.5),
    ("minecraft:nether/ride_strider_in_overworld_lava", 2.0, 0.0),
    ("minecraft:nether/root", 0.0, 4.0),
    ("minecraft:nether/summon_wither", 3.0, 7.0),
    ("minecraft:nether/uneasy_alliance", 2.0, 5.5),
    ("minecraft:story/cure_zombie_villager", 7.0, 0.5),
    ("minecraft:story/deflect_arrow", 5.0, 0.0),
    ("minecraft:story/enchant_item", 6.0, 3.0),
    ("minecraft:story/enter_the_end", 8.0, 1.5),
    ("minecraft:story/enter_the_nether", 6.0, 1.0),
    ("minecraft:story/follow_ender_eye", 7.0, 1.5),
    ("minecraft:story/form_obsidian", 5.0, 1.0),
    ("minecraft:story/iron_tools", 4.0, 2.5),
    ("minecraft:story/lava_bucket", 4.0, 1.0),
    ("minecraft:story/mine_diamond", 5.0, 2.5),
    ("minecraft:story/mine_stone", 1.0, 1.25),
    ("minecraft:story/obtain_armor", 4.0, 0.0),
    ("minecraft:story/root", 0.0, 1.25),
    ("minecraft:story/shiny_gear", 6.0, 2.0),
    ("minecraft:story/smelt_iron", 3.0, 1.25),
    ("minecraft:story/upgrade_tools", 2.0, 1.25),
];

/// 用固定兄弟顺序建树，逐项比较全部显示节点的坐标和官方一致。
fn assert_layout(label: &str, order: &[(&str, &[&str])], positions: &[(&str, f64, f64)]) {
    let mut tree = AdvancementTree::parse(mc_bamboo_data::ADVANCEMENTS_JSON, TreeOrder::ById, None)
        .expect("内嵌进度树必须合法");
    let mut child_order: Vec<Vec<String>> = vec![Vec::new(); tree.nodes.len()];
    for &(id, children) in order {
        let index = tree.index(id).unwrap_or_else(|| panic!("缺进度 {id}"));
        child_order[index] = children.iter().map(|child| (*child).to_owned()).collect();
    }
    tree.apply_order(TreeOrder::External, Some(&child_order))
        .expect("外部兄弟顺序必须合法");
    assert_eq!(
        positions.len(),
        tree.displayed.len(),
        "{label}: 显示节点数不同"
    );
    for &(id, x, y) in positions {
        let index = tree.index(id).unwrap_or_else(|| panic!("缺进度 {id}"));
        assert_eq!(
            tree.nodes[index].x.to_bits(),
            (x as f32).to_bits(),
            "{label} {id}: x 不同"
        );
        assert_eq!(
            tree.nodes[index].y.to_bits(),
            (y as f32).to_bits(),
            "{label} {id}: y 不同"
        );
    }
}

#[test]
fn husbandry_uh_oh_order_matches_vanilla() {
    assert_layout("bafbba63da32", UH_OH_ORDER, UH_OH_POSITIONS);
}

#[test]
fn adventure_heart_transplanter_order_matches_vanilla() {
    assert_layout("c15f3b068c59", HEART_ORDER, HEART_POSITIONS);
}
