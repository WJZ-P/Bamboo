// 这个文件由 mc-bamboo-vanilla 生成，不要手动修改。

/// 连续枚举的常量个数，ID 就是 0 到这个数减一。
pub mod counts {
    pub const ATTRIBUTE_DISPLAY_TYPE: i32 = 3;
    pub const ATTRIBUTE_OPERATION: i32 = 3;
    pub const AXOLOTL_VARIANT: i32 = 5;
    pub const DYE_COLOR: i32 = 16;
    pub const EQUIPMENT_SLOT: i32 = 8;
    pub const EQUIPMENT_SLOT_GROUP: i32 = 11;
    pub const FIREWORK_SHAPE: i32 = 5;
    pub const FOX_VARIANT: i32 = 2;
    pub const HORSE_VARIANT: i32 = 7;
    pub const ITEM_USE_ANIMATION: i32 = 12;
    pub const LLAMA_VARIANT: i32 = 4;
    pub const MAP_POST_PROCESSING: i32 = 2;
    pub const MOOSHROOM_VARIANT: i32 = 2;
    pub const PARROT_VARIANT: i32 = 5;
    pub const RARITY: i32 = 4;
    pub const SALMON_SIZE: i32 = 3;
    pub const SWING_ANIMATION_TYPE: i32 = 3;
}

/// 稀疏枚举的 ID，按官方的声明顺序。官方的默认值都是第一个声明的常量。
pub mod sparse {
    pub const RABBIT_VARIANT: &[i32] = &[0, 1, 2, 3, 4, 5, 99];
    pub const TROPICAL_FISH_PATTERN: &[i32] = &[0, 256, 512, 768, 1024, 1280, 1, 257, 513, 769, 1025, 1281];
}

/// 枚举的序列化名字，下标就是网络上的 ID。存档编解码器按名字读写。
pub mod names {
    pub const ATTRIBUTE_DISPLAY_TYPE: &[&str] = &["default", "hidden", "override"];
    pub const ATTRIBUTE_OPERATION: &[&str] = &["add_value", "add_multiplied_base", "add_multiplied_total"];
    pub const AXOLOTL_VARIANT: &[&str] = &["lucy", "wild", "gold", "cyan", "blue"];
    pub const DYE_COLOR: &[&str] = &["white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black"];
    pub const EQUIPMENT_SLOT: &[&str] = &["mainhand", "feet", "legs", "chest", "head", "offhand", "body", "saddle"];
    pub const EQUIPMENT_SLOT_GROUP: &[&str] = &["any", "mainhand", "offhand", "hand", "feet", "legs", "chest", "head", "armor", "body", "saddle"];
    pub const FIREWORK_SHAPE: &[&str] = &["small_ball", "large_ball", "star", "creeper", "burst"];
    pub const FOX_VARIANT: &[&str] = &["red", "snow"];
    pub const HORSE_VARIANT: &[&str] = &["white", "creamy", "chestnut", "brown", "black", "gray", "dark_brown"];
    pub const ITEM_USE_ANIMATION: &[&str] = &["none", "eat", "drink", "block", "bow", "trident", "crossbow", "spyglass", "toot_horn", "brush", "bundle", "spear"];
    pub const LLAMA_VARIANT: &[&str] = &["creamy", "white", "brown", "gray"];
    pub const MOOSHROOM_VARIANT: &[&str] = &["red", "brown"];
    pub const PARROT_VARIANT: &[&str] = &["red_blue", "blue", "green", "yellow_blue", "gray"];
    pub const RABBIT_VARIANT: &[&str] = &["brown", "white", "black", "white_splotched", "gold", "salt", "evil"];
    pub const RARITY: &[&str] = &["common", "uncommon", "rare", "epic"];
    pub const SALMON_SIZE: &[&str] = &["small", "medium", "large"];
    pub const SWING_ANIMATION_TYPE: &[&str] = &["none", "whack", "stab"];
    pub const TROPICAL_FISH_PATTERN: &[&str] = &["kob", "sunstreak", "snooper", "dasher", "brinely", "spotty", "flopper", "stripey", "glitter", "blockfish", "betty", "clayfish"];
}
