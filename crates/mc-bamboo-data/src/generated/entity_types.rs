// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。

/// 一个实体类型的跟踪参数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityTypeFacts {
    /// 官方的注册表名。
    pub name: &'static str,
    /// `clientTrackingRange`，单位是区块。
    pub client_tracking_range: i32,
    /// `UpdateInterval.periodic` 的周期，-1 表示 `UpdateInterval.NEVER`。
    pub update_interval: i32,
    /// `trackDeltas`：速度变了要不要给跟踪的玩家发速度包。
    pub track_deltas: bool,
    /// `EntityDimensions.width`。
    pub width: f32,
    /// `EntityDimensions.height`。
    pub height: f32,
}

/// 全部实体类型，下标就是注册表 ID，和 [`crate::registries::entity_type`] 的常量一致。
pub static FACTS: &[EntityTypeFacts] = &[
    EntityTypeFacts { name: "minecraft:acacia_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:acacia_chest_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:allay", client_tracking_range: 8, update_interval: 2, track_deltas: true, width: 0.35, height: 0.6 },
    EntityTypeFacts { name: "minecraft:area_effect_cloud", client_tracking_range: 10, update_interval: -1, track_deltas: true, width: 6.0, height: 0.5 },
    EntityTypeFacts { name: "minecraft:armadillo", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.7, height: 0.65 },
    EntityTypeFacts { name: "minecraft:armor_stand", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.5, height: 1.975 },
    EntityTypeFacts { name: "minecraft:arrow", client_tracking_range: 4, update_interval: 20, track_deltas: true, width: 0.5, height: 0.5 },
    EntityTypeFacts { name: "minecraft:axolotl", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.75, height: 0.42 },
    EntityTypeFacts { name: "minecraft:bamboo_chest_raft", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:bamboo_raft", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:bat", client_tracking_range: 5, update_interval: 3, track_deltas: false, width: 0.5, height: 0.9 },
    EntityTypeFacts { name: "minecraft:bee", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.55, height: 0.5 },
    EntityTypeFacts { name: "minecraft:birch_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:birch_chest_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:blaze", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.8 },
    EntityTypeFacts { name: "minecraft:block_display", client_tracking_range: 10, update_interval: 1, track_deltas: true, width: 0.0, height: 0.0 },
    EntityTypeFacts { name: "minecraft:bogged", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.99 },
    EntityTypeFacts { name: "minecraft:breeze", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.6, height: 1.77 },
    EntityTypeFacts { name: "minecraft:breeze_wind_charge", client_tracking_range: 4, update_interval: 10, track_deltas: true, width: 0.3125, height: 0.3125 },
    EntityTypeFacts { name: "minecraft:camel", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.7, height: 2.375 },
    EntityTypeFacts { name: "minecraft:camel_husk", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.7, height: 2.375 },
    EntityTypeFacts { name: "minecraft:cat", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 0.7 },
    EntityTypeFacts { name: "minecraft:cave_spider", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.7, height: 0.5 },
    EntityTypeFacts { name: "minecraft:cherry_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:cherry_chest_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:chest_minecart", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.98, height: 0.7 },
    EntityTypeFacts { name: "minecraft:chicken", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.4, height: 0.7 },
    EntityTypeFacts { name: "minecraft:cod", client_tracking_range: 4, update_interval: 3, track_deltas: true, width: 0.5, height: 0.3 },
    EntityTypeFacts { name: "minecraft:copper_golem", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.49, height: 0.98 },
    EntityTypeFacts { name: "minecraft:command_block_minecart", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.98, height: 0.7 },
    EntityTypeFacts { name: "minecraft:cow", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.9, height: 1.4 },
    EntityTypeFacts { name: "minecraft:creaking", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.9, height: 2.7 },
    EntityTypeFacts { name: "minecraft:creeper", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.7 },
    EntityTypeFacts { name: "minecraft:cushion", client_tracking_range: 10, update_interval: -1, track_deltas: false, width: 1.0, height: 0.25 },
    EntityTypeFacts { name: "minecraft:dark_oak_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:dark_oak_chest_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:dolphin", client_tracking_range: 5, update_interval: 3, track_deltas: true, width: 0.9, height: 0.6 },
    EntityTypeFacts { name: "minecraft:donkey", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.3964844, height: 1.5 },
    EntityTypeFacts { name: "minecraft:dragon_fireball", client_tracking_range: 4, update_interval: 10, track_deltas: true, width: 1.0, height: 1.0 },
    EntityTypeFacts { name: "minecraft:drowned", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.95 },
    EntityTypeFacts { name: "minecraft:egg", client_tracking_range: 4, update_interval: 10, track_deltas: true, width: 0.25, height: 0.25 },
    EntityTypeFacts { name: "minecraft:elder_guardian", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.9975, height: 1.9975 },
    EntityTypeFacts { name: "minecraft:enderman", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 2.9 },
    EntityTypeFacts { name: "minecraft:endermite", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.4, height: 0.3 },
    EntityTypeFacts { name: "minecraft:ender_dragon", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 16.0, height: 8.0 },
    EntityTypeFacts { name: "minecraft:ender_pearl", client_tracking_range: 4, update_interval: 10, track_deltas: true, width: 0.25, height: 0.25 },
    EntityTypeFacts { name: "minecraft:end_crystal", client_tracking_range: 16, update_interval: -1, track_deltas: false, width: 2.0, height: 2.0 },
    EntityTypeFacts { name: "minecraft:evoker", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.95 },
    EntityTypeFacts { name: "minecraft:evoker_fangs", client_tracking_range: 6, update_interval: 2, track_deltas: false, width: 0.5, height: 0.8 },
    EntityTypeFacts { name: "minecraft:experience_bottle", client_tracking_range: 4, update_interval: 10, track_deltas: true, width: 0.25, height: 0.25 },
    EntityTypeFacts { name: "minecraft:experience_orb", client_tracking_range: 6, update_interval: 20, track_deltas: true, width: 0.5, height: 0.5 },
    EntityTypeFacts { name: "minecraft:eye_of_ender", client_tracking_range: 4, update_interval: 4, track_deltas: true, width: 0.25, height: 0.25 },
    EntityTypeFacts { name: "minecraft:falling_block", client_tracking_range: 10, update_interval: 20, track_deltas: true, width: 0.98, height: 0.98 },
    EntityTypeFacts { name: "minecraft:fireball", client_tracking_range: 4, update_interval: 10, track_deltas: true, width: 1.0, height: 1.0 },
    EntityTypeFacts { name: "minecraft:firework_rocket", client_tracking_range: 4, update_interval: 10, track_deltas: true, width: 0.25, height: 0.25 },
    EntityTypeFacts { name: "minecraft:fox", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 0.7 },
    EntityTypeFacts { name: "minecraft:frog", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.5, height: 0.5 },
    EntityTypeFacts { name: "minecraft:furnace_minecart", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.98, height: 0.7 },
    EntityTypeFacts { name: "minecraft:ghast", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 4.0, height: 4.0 },
    EntityTypeFacts { name: "minecraft:happy_ghast", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 4.0, height: 4.0 },
    EntityTypeFacts { name: "minecraft:giant", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 3.6, height: 12.0 },
    EntityTypeFacts { name: "minecraft:glow_item_frame", client_tracking_range: 10, update_interval: -1, track_deltas: false, width: 0.5, height: 0.5 },
    EntityTypeFacts { name: "minecraft:glow_squid", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.8, height: 0.8 },
    EntityTypeFacts { name: "minecraft:goat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.9, height: 1.3 },
    EntityTypeFacts { name: "minecraft:guardian", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.85, height: 0.85 },
    EntityTypeFacts { name: "minecraft:hoglin", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 1.3964844, height: 1.4 },
    EntityTypeFacts { name: "minecraft:hopper_minecart", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.98, height: 0.7 },
    EntityTypeFacts { name: "minecraft:horse", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.3964844, height: 1.6 },
    EntityTypeFacts { name: "minecraft:husk", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.95 },
    EntityTypeFacts { name: "minecraft:illusioner", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.95 },
    EntityTypeFacts { name: "minecraft:interaction", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.0, height: 0.0 },
    EntityTypeFacts { name: "minecraft:iron_golem", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.4, height: 2.7 },
    EntityTypeFacts { name: "minecraft:item", client_tracking_range: 6, update_interval: 20, track_deltas: true, width: 0.25, height: 0.25 },
    EntityTypeFacts { name: "minecraft:item_display", client_tracking_range: 10, update_interval: 1, track_deltas: true, width: 0.0, height: 0.0 },
    EntityTypeFacts { name: "minecraft:item_frame", client_tracking_range: 10, update_interval: -1, track_deltas: false, width: 0.5, height: 0.5 },
    EntityTypeFacts { name: "minecraft:jungle_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:jungle_chest_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:leash_knot", client_tracking_range: 10, update_interval: -1, track_deltas: false, width: 0.375, height: 0.5 },
    EntityTypeFacts { name: "minecraft:lightning_bolt", client_tracking_range: 16, update_interval: -1, track_deltas: true, width: 0.0, height: 0.0 },
    EntityTypeFacts { name: "minecraft:llama", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.9, height: 1.87 },
    EntityTypeFacts { name: "minecraft:llama_spit", client_tracking_range: 4, update_interval: 10, track_deltas: false, width: 0.25, height: 0.25 },
    EntityTypeFacts { name: "minecraft:magma_cube", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.52, height: 0.52 },
    EntityTypeFacts { name: "minecraft:mangrove_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:mangrove_chest_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:mannequin", client_tracking_range: 32, update_interval: 2, track_deltas: true, width: 0.6, height: 1.8 },
    EntityTypeFacts { name: "minecraft:marker", client_tracking_range: 0, update_interval: 3, track_deltas: true, width: 0.0, height: 0.0 },
    EntityTypeFacts { name: "minecraft:minecart", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.98, height: 0.7 },
    EntityTypeFacts { name: "minecraft:mooshroom", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.9, height: 1.4 },
    EntityTypeFacts { name: "minecraft:mule", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 1.3964844, height: 1.6 },
    EntityTypeFacts { name: "minecraft:nautilus", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.875, height: 0.95 },
    EntityTypeFacts { name: "minecraft:oak_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:oak_chest_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:ocelot", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.6, height: 0.7 },
    EntityTypeFacts { name: "minecraft:ominous_item_spawner", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.25, height: 0.25 },
    EntityTypeFacts { name: "minecraft:painting", client_tracking_range: 10, update_interval: -1, track_deltas: false, width: 0.5, height: 0.5 },
    EntityTypeFacts { name: "minecraft:pale_oak_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:pale_oak_chest_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:panda", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.3, height: 1.25 },
    EntityTypeFacts { name: "minecraft:parched", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.99 },
    EntityTypeFacts { name: "minecraft:parrot", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.5, height: 0.9 },
    EntityTypeFacts { name: "minecraft:phantom", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.9, height: 0.5 },
    EntityTypeFacts { name: "minecraft:pig", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.9, height: 0.9 },
    EntityTypeFacts { name: "minecraft:piglin", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.95 },
    EntityTypeFacts { name: "minecraft:piglin_brute", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.95 },
    EntityTypeFacts { name: "minecraft:pillager", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.95 },
    EntityTypeFacts { name: "minecraft:polar_bear", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.4, height: 1.4 },
    EntityTypeFacts { name: "minecraft:poplar_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:poplar_chest_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:splash_potion", client_tracking_range: 4, update_interval: 10, track_deltas: true, width: 0.25, height: 0.25 },
    EntityTypeFacts { name: "minecraft:lingering_potion", client_tracking_range: 4, update_interval: 10, track_deltas: true, width: 0.25, height: 0.25 },
    EntityTypeFacts { name: "minecraft:pufferfish", client_tracking_range: 4, update_interval: 3, track_deltas: true, width: 0.7, height: 0.7 },
    EntityTypeFacts { name: "minecraft:rabbit", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.49, height: 0.6 },
    EntityTypeFacts { name: "minecraft:ravager", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.95, height: 2.2 },
    EntityTypeFacts { name: "minecraft:salmon", client_tracking_range: 4, update_interval: 3, track_deltas: true, width: 0.7, height: 0.4 },
    EntityTypeFacts { name: "minecraft:sheep", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.9, height: 1.3 },
    EntityTypeFacts { name: "minecraft:shulker", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.0, height: 1.0 },
    EntityTypeFacts { name: "minecraft:shulker_bullet", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.3125, height: 0.3125 },
    EntityTypeFacts { name: "minecraft:silverfish", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.4, height: 0.3 },
    EntityTypeFacts { name: "minecraft:skeleton", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.99 },
    EntityTypeFacts { name: "minecraft:skeleton_horse", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.3964844, height: 1.6 },
    EntityTypeFacts { name: "minecraft:slime", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.52, height: 0.52 },
    EntityTypeFacts { name: "minecraft:small_fireball", client_tracking_range: 4, update_interval: 10, track_deltas: true, width: 0.3125, height: 0.3125 },
    EntityTypeFacts { name: "minecraft:sniffer", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.9, height: 1.75 },
    EntityTypeFacts { name: "minecraft:snowball", client_tracking_range: 4, update_interval: 10, track_deltas: true, width: 0.25, height: 0.25 },
    EntityTypeFacts { name: "minecraft:snow_golem", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.7, height: 1.9 },
    EntityTypeFacts { name: "minecraft:spawner_minecart", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.98, height: 0.7 },
    EntityTypeFacts { name: "minecraft:spectral_arrow", client_tracking_range: 4, update_interval: 20, track_deltas: true, width: 0.5, height: 0.5 },
    EntityTypeFacts { name: "minecraft:spider", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 1.4, height: 0.9 },
    EntityTypeFacts { name: "minecraft:spruce_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:spruce_chest_boat", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.375, height: 0.5625 },
    EntityTypeFacts { name: "minecraft:squid", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.8, height: 0.8 },
    EntityTypeFacts { name: "minecraft:stray", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.99 },
    EntityTypeFacts { name: "minecraft:strider", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.9, height: 1.7 },
    EntityTypeFacts { name: "minecraft:sulfur_cube", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.49, height: 0.49 },
    EntityTypeFacts { name: "minecraft:tadpole", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.4, height: 0.3 },
    EntityTypeFacts { name: "minecraft:text_display", client_tracking_range: 10, update_interval: 1, track_deltas: true, width: 0.0, height: 0.0 },
    EntityTypeFacts { name: "minecraft:tnt", client_tracking_range: 10, update_interval: 10, track_deltas: true, width: 0.98, height: 0.98 },
    EntityTypeFacts { name: "minecraft:tnt_minecart", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.98, height: 0.7 },
    EntityTypeFacts { name: "minecraft:trader_llama", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.9, height: 1.87 },
    EntityTypeFacts { name: "minecraft:trident", client_tracking_range: 4, update_interval: 20, track_deltas: true, width: 0.5, height: 0.5 },
    EntityTypeFacts { name: "minecraft:tropical_fish", client_tracking_range: 4, update_interval: 3, track_deltas: true, width: 0.5, height: 0.4 },
    EntityTypeFacts { name: "minecraft:turtle", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.2, height: 0.4 },
    EntityTypeFacts { name: "minecraft:vex", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.4, height: 0.8 },
    EntityTypeFacts { name: "minecraft:villager", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.6, height: 1.95 },
    EntityTypeFacts { name: "minecraft:vindicator", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.95 },
    EntityTypeFacts { name: "minecraft:wandering_trader", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.6, height: 1.95 },
    EntityTypeFacts { name: "minecraft:warden", client_tracking_range: 16, update_interval: 3, track_deltas: true, width: 0.9, height: 2.9 },
    EntityTypeFacts { name: "minecraft:wind_charge", client_tracking_range: 4, update_interval: 10, track_deltas: true, width: 0.3125, height: 0.3125 },
    EntityTypeFacts { name: "minecraft:witch", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.95 },
    EntityTypeFacts { name: "minecraft:wither", client_tracking_range: 10, update_interval: 3, track_deltas: false, width: 0.9, height: 3.5 },
    EntityTypeFacts { name: "minecraft:wither_skeleton", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.7, height: 2.4 },
    EntityTypeFacts { name: "minecraft:wither_skull", client_tracking_range: 4, update_interval: 10, track_deltas: true, width: 0.3125, height: 0.3125 },
    EntityTypeFacts { name: "minecraft:wolf", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.6, height: 0.85 },
    EntityTypeFacts { name: "minecraft:zoglin", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 1.3964844, height: 1.4 },
    EntityTypeFacts { name: "minecraft:zombie", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.95 },
    EntityTypeFacts { name: "minecraft:zombie_horse", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 1.3964844, height: 1.6 },
    EntityTypeFacts { name: "minecraft:zombie_nautilus", client_tracking_range: 10, update_interval: 3, track_deltas: true, width: 0.875, height: 0.95 },
    EntityTypeFacts { name: "minecraft:zombie_villager", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.95 },
    EntityTypeFacts { name: "minecraft:zombified_piglin", client_tracking_range: 8, update_interval: 3, track_deltas: true, width: 0.6, height: 1.95 },
    EntityTypeFacts { name: "minecraft:player", client_tracking_range: 32, update_interval: 2, track_deltas: false, width: 0.6, height: 1.8 },
    EntityTypeFacts { name: "minecraft:fishing_bobber", client_tracking_range: 4, update_interval: 5, track_deltas: true, width: 0.25, height: 0.25 },
];

pub const ARROW_FLAGS: u8 = 8;
pub const ARROW_PIERCE: u8 = 9;
pub const ARROW_IN_GROUND: u8 = 10;
pub const ARROW_COLOR: u8 = 11;
/// `ThrownTrident.DATA_ID_LOYALTY`：BYTE，继承 `AbstractArrow` 之后的第一项自有元数据。
///
/// `ThrownTrident.DATA_ID_LOYALTY`: BYTE, the first accessor after `AbstractArrow`.
pub const TRIDENT_LOYALTY: u8 = 11;
/// `ThrownTrident.DATA_ID_FOIL`：BOOLEAN，是否有附魔光效。
///
/// `ThrownTrident.DATA_ID_FOIL`: BOOLEAN, whether it has the enchantment glint.
pub const TRIDENT_FOIL: u8 = 12;
pub const SKELETON_EYE_HEIGHT: f32 = 1.74;
pub const LLAMA_EYE_HEIGHT: f32 = 1.7765;
pub const MULE_EYE_HEIGHT: f32 = 1.52;
pub const DONKEY_EYE_HEIGHT: f32 = 1.425;
pub const HORSE_EYE_HEIGHT: f32 = 1.52;
pub const SKELETON_HORSE_MAX_HEALTH: f64 = 15.0;
pub const SKELETON_HORSE_MOVEMENT_SPEED: f64 = 0.20000000298023224;
pub const ZOMBIE_HORSE_MAX_HEALTH: f64 = 25.0;
pub const SKELETON_HORSE_BABY_WIDTH: f32 = 0.97753906;
pub const SKELETON_HORSE_BABY_HEIGHT: f32 = 1.12;
pub const SKELETON_HORSE_BABY_EYE_HEIGHT: f32 = 1.064;
pub const SKELETON_HORSE_EYE_HEIGHT: f32 = 1.52;
pub const ZOMBIE_HORSE_BABY_WIDTH: f32 = 0.97753906;
pub const ZOMBIE_HORSE_BABY_HEIGHT: f32 = 1.12;
pub const ZOMBIE_HORSE_BABY_EYE_HEIGHT: f32 = 1.064;
pub const ZOMBIE_HORSE_EYE_HEIGHT: f32 = 1.52;
pub const HORSE_FLAGS: u8 = 18;
pub const HORSE_VARIANT: u8 = 19;
pub const HORSE_CHEST: u8 = 19;
pub const LLAMA_STRENGTH: u8 = 20;
pub const LLAMA_VARIANT: u8 = 21;
pub const RABBIT_EYE_HEIGHT: f32 = 0.59;
pub const RABBIT_BABY_WIDTH: f32 = 0.24;
pub const RABBIT_BABY_HEIGHT: f32 = 0.4;
pub const RABBIT_BABY_EYE_HEIGHT: f32 = 0.39;
pub const RABBIT_VARIANT: u8 = 18;
pub const RABBIT_MAX_HEALTH: f64 = 3.0;
pub const RABBIT_MOVEMENT_SPEED: f64 = 0.30000001192092896;
pub const RABBIT_ATTACK_DAMAGE: f64 = 3.0;
pub const FOX_EYE_HEIGHT: f32 = 0.4;
pub const FOX_BABY_WIDTH: f32 = 0.36;
pub const FOX_BABY_HEIGHT: f32 = 0.42000002;
pub const FOX_BABY_EYE_HEIGHT: f32 = 0.34375;
pub const FOX_TRUSTED_0: u8 = 20;
pub const FOX_TRUSTED_1: u8 = 21;
pub const FOX_TYPE: u8 = 18;
pub const FOX_FLAGS: u8 = 19;
pub const FOX_MAX_HEALTH: f64 = 10.0;
pub const FOX_MOVEMENT_SPEED: f64 = 0.30000001192092896;
pub const FOX_ATTACK_DAMAGE: f64 = 2.0;
pub const FOX_SAFE_FALL_DISTANCE: f64 = 5.0;
pub const FOX_FOLLOW_RANGE: f64 = 32.0;
pub const WOLF_EYE_HEIGHT: f32 = 0.68;
pub const WOLF_BABY_WIDTH: f32 = 0.3;
pub const WOLF_BABY_HEIGHT: f32 = 0.425;
pub const WOLF_BABY_EYE_HEIGHT: f32 = 0.34375;
pub const TAME_FLAGS: u8 = 18;
pub const TAME_OWNER: u8 = 19;
pub const WOLF_INTERESTED: u8 = 20;
pub const WOLF_COLLAR: u8 = 21;
pub const WOLF_ANGER_END_TIME: u8 = 22;
pub const WOLF_VARIANT: u8 = 23;
pub const WOLF_SOUND_VARIANT: u8 = 24;
pub const WOLF_MAX_HEALTH: f64 = 8.0;
pub const WOLF_MOVEMENT_SPEED: f64 = 0.30000001192092896;
pub const WOLF_ATTACK_DAMAGE: f64 = 4.0;
pub const IRON_GOLEM_EYE_HEIGHT: f32 = 2.295;
pub const SNOW_GOLEM_EYE_HEIGHT: f32 = 1.7;
pub const GOLEM_CRACKINESS_LOW: f32 = 0.75;
pub const GOLEM_CRACKINESS_MEDIUM: f32 = 0.5;
pub const GOLEM_CRACKINESS_HIGH: f32 = 0.25;
pub const IRON_GOLEM_MAX_HEALTH: f64 = 100.0;
pub const IRON_GOLEM_MOVEMENT_SPEED: f64 = 0.25;
pub const IRON_GOLEM_KNOCKBACK_RESISTANCE: f64 = 1.0;
pub const IRON_GOLEM_ATTACK_DAMAGE: f64 = 15.0;
pub const IRON_GOLEM_STEP_HEIGHT: f64 = 1.0;
pub const SNOW_GOLEM_MAX_HEALTH: f64 = 4.0;
pub const SNOW_GOLEM_MOVEMENT_SPEED: f64 = 0.20000000298023224;
pub const PIG_BOOST_TIME: u8 = 18;
pub const PIG_VARIANT: u8 = 19;
pub const PIG_SOUND_VARIANT: u8 = 20;
pub const SHEEP_WOOL: u8 = 18;
pub const POLAR_BEAR_STANDING: u8 = 18;
pub const PANDA_UNHAPPY: u8 = 18;
pub const PANDA_SNEEZE: u8 = 19;
pub const PANDA_EAT: u8 = 20;
pub const PANDA_MAIN_GENE: u8 = 21;
pub const PANDA_HIDDEN_GENE: u8 = 22;
pub const PANDA_FLAGS: u8 = 23;
pub const OCELOT_TRUSTING: u8 = 18;
pub const IRON_GOLEM_FLAGS: u8 = 16;
pub const SNOW_GOLEM_PUMPKIN: u8 = 16;
pub const CAT_VARIANT: u8 = 20;
pub const CAT_IS_LYING: u8 = 21;
pub const CAT_RELAX_STATE_ONE: u8 = 22;
pub const CAT_COLLAR: u8 = 23;
pub const CAT_SOUND_VARIANT: u8 = 24;
pub const PARROT_VARIANT: u8 = 20;
pub const GLOW_SQUID_DARK_TICKS: u8 = 18;
pub const BAT_FLAGS: u8 = 16;
pub const PHANTOM_SIZE: u8 = 16;
pub const GUARDIAN_MOVING: u8 = 16;
pub const GUARDIAN_ATTACK_TARGET: u8 = 17;
pub const NAUTILUS_DASH: u8 = 20;
pub const ZOMBIE_NAUTILUS_VARIANT: u8 = 21;
pub const FISH_FROM_BUCKET: u8 = 16;
pub const SALMON_VARIANT: u8 = 17;
pub const TROPICAL_FISH_VARIANT: u8 = 17;
pub const PUFFERFISH_PUFF_STATE: u8 = 17;
pub const CHICKEN_VARIANT: u8 = 18;
pub const CHICKEN_SOUND_VARIANT: u8 = 19;
pub const ARMADILLO_STATE: u8 = 18;
pub const GOAT_SCREAMING: u8 = 18;
pub const GOAT_LEFT_HORN: u8 = 19;
pub const GOAT_RIGHT_HORN: u8 = 20;
pub const FROG_VARIANT: u8 = 18;
pub const FROG_TONGUE_TARGET: u8 = 19;
pub const TURTLE_HAS_EGG: u8 = 18;
pub const TURTLE_LAYING_EGG: u8 = 19;
pub const DOLPHIN_GOT_FISH: u8 = 18;
pub const DOLPHIN_MOISTNESS: u8 = 19;
pub const AXOLOTL_VARIANT: u8 = 18;
pub const AXOLOTL_PLAYING_DEAD: u8 = 19;
pub const AXOLOTL_FROM_BUCKET: u8 = 20;
pub const SILVERFISH_MAX_HEALTH: f64 = 8.0;
pub const SILVERFISH_MOVEMENT_SPEED: f64 = 0.25;
pub const SILVERFISH_ATTACK_DAMAGE: f64 = 1.0;
pub const ENDERMITE_MAX_HEALTH: f64 = 8.0;
pub const ENDERMITE_MOVEMENT_SPEED: f64 = 0.25;
pub const ENDERMITE_ATTACK_DAMAGE: f64 = 2.0;
pub const ENDERMITE_MAX_LIFE: i32 = 2400;
pub const ENDERMITE_XP_REWARD: i32 = 3;
pub const PARROT_SOUND_MAP_SIZE: i32 = 42;
pub const BEE_FLAGS: u8 = 18;
pub const BEE_ANGER_END_TIME: u8 = 19;
pub const BEE_EYE_HEIGHT: f32 = 0.3;
pub const BEE_FLAP_DEGREES_PER_TICK: f32 = 120.32113;
pub const BEE_MAX_HEALTH: f64 = 10.0;
pub const BEE_FLYING_SPEED: f64 = 0.6000000238418579;
pub const BEE_MOVEMENT_SPEED: f64 = 0.30000001192092896;
pub const BEE_ATTACK_DAMAGE: f64 = 2.0;
pub const BEE_TICKS_PER_FLAP: i32 = 2;
pub const BEE_ANGER_MIN_SECONDS: i32 = 400;
pub const BEE_ANGER_MAX_SECONDS: i32 = 780;
pub const BEEHIVE_MAX_OCCUPANTS: i32 = 3;
pub const BEEHIVE_MIN_TICKS_NECTARLESS: i32 = 600;
pub const BEEHIVE_MIN_TICKS_NECTAR: i32 = 2400;
pub const BEEHIVE_REENTER_TICKS: i32 = 400;
pub const TROPICAL_FISH_COMMON_VARIANTS: &[i32] = &[117506305, 117899265, 185008129, 117441793, 118161664, 65536, 50726144, 67764993, 234882305, 67110144, 117441025, 16778497, 101253888, 50660352, 918529, 235340288, 918273, 67108865, 917504, 459008, 67699456, 67371009];
pub const PIG_VARIANT_SERIALIZER: i32 = 28;
pub const PIG_SOUND_VARIANT_SERIALIZER: i32 = 29;
pub const CHICKEN_VARIANT_SERIALIZER: i32 = 30;
pub const CHICKEN_SOUND_VARIANT_SERIALIZER: i32 = 31;
pub const ARMADILLO_STATE_SERIALIZER: i32 = 36;
pub const FROG_VARIANT_SERIALIZER: i32 = 27;
pub const FROG_TONGUE_TARGET_SERIALIZER: i32 = 19;
pub const ZOMBIE_NAUTILUS_VARIANT_SERIALIZER: i32 = 32;
pub const CUBE_MOB_SIZE: u8 = 18;
pub const SLIME_EYE_HEIGHT: f32 = 0.325;
pub const MOOSHROOM_TYPE: u8 = 18;
/// `SuspiciousEffectHolder`：(物品 ID, [(效果 ID, 刻数)])，按物品 ID 升序。
pub static STEW_FLOWERS: &[(i32, &[(i32, i32)])] = &[
    (298, &[(22, 7)]),
    (299, &[(22, 7)]),
    (300, &[(14, 220)]),
    (301, &[(8, 140)]),
    (302, &[(15, 100)]),
    (303, &[(22, 7)]),
    (304, &[(11, 60)]),
    (305, &[(14, 220)]),
    (306, &[(17, 140)]),
    (307, &[(17, 140)]),
    (308, &[(17, 140)]),
    (309, &[(17, 140)]),
    (310, &[(9, 140)]),
    (311, &[(7, 100)]),
    (312, &[(18, 220)]),
    (313, &[(19, 140)]),
    (314, &[(15, 100)]),
];
pub const CREEPER_IGNITED: u8 = 18;
pub const CREEPER_POWERED: u8 = 17;
pub const CREEPER_SWELL_DIR: u8 = 16;
pub const ENDERMAN_CARRY_STATE: u8 = 16;
pub const ENDERMAN_CREEPY: u8 = 17;
pub const ENDERMAN_STARED_AT: u8 = 18;
pub const SPIDER_FLAGS: u8 = 16;
pub const CAVE_SPIDER_EYE_HEIGHT: f32 = 0.45;
pub const CREEPER_EYE_HEIGHT: f32 = 1.445;
pub const ENDERMAN_EYE_HEIGHT: f32 = 2.55;
pub const ENDERMITE_EYE_HEIGHT: f32 = 0.13;
pub const SILVERFISH_EYE_HEIGHT: f32 = 0.13;
pub const SPIDER_EYE_HEIGHT: f32 = 0.65;
