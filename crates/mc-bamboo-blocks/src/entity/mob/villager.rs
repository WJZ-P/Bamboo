//! 村民的同步数据和存档：`VillagerData`、`VillagerDataFinalized`、`DataUnhappyCounter`、
//! 食物 / 经验 / 补货时刻和八卦。
//!
//! 期 2 只有骨架：交易（`Offers`）和背包（`Inventory`）原样保留不解释，八卦也只保留
//! 原文；求值在期 5 / 期 6。
//!
//! Villager synced data and saving: `VillagerData`, `VillagerDataFinalized`, the
//! unhappy counter, food / experience / restock times and gossips. Phase 2 keeps
//! `Offers` and `Inventory` verbatim; trading and gossip semantics come later.

use std::collections::BTreeMap;

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{items, poi as poi_data, tags::item_tags, villager as facts};
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{
    components::{archive::slots as item_slots, value::GlobalPos},
    item::ItemStack,
};

use super::{
    brain::{Brain, memory::MemoryType},
    navigation::Navigation,
    spawn::{self, SpawnReason},
};
use crate::{
    archive::tag::{self, put},
    behaviour::{
        common::item_of,
        misc::{COMPOST_MAX_LEVEL, COMPOST_READY, add_layer_state, extract_produce},
    },
    container::item_max,
    entity::{Entity, EntityKind, EntityUid},
    level::Effect,
    state::{LEVEL_COMPOSTER, item_is_tagged},
    world::World,
};

/// 幼年村民 `NearestBedSensor` 的重试缓存（放进 `Villager` 保持 `Sensor` 16 字节、`Brain` 952 字节）。
///
/// Baby villager `NearestBedSensor` retry cache (stored on `Villager` so `Sensor` stays
/// 16 bytes and `Brain` stays 952 bytes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NearestBedCache {
    pub batch_cache: BTreeMap<BlockPos, i64>,
    pub tried_count: i32,
    pub last_update: i64,
}

/// `AbstractVillager.inventory`：8 格 `SimpleContainer`，空背包不分配堆内存。
///
/// `AbstractVillager.inventory`: an 8-slot `SimpleContainer` with lazy heap allocation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VillagerInventory {
    slots: Option<Box<[ItemStack; 8]>>,
}

impl VillagerInventory {
    /// 槽位数（`AbstractVillager` 固定为 8）。
    pub const SIZE: usize = 8;

    /// 读取第 `index` 格物品（越界或空背包返回空堆）。
    pub fn get(&self, index: usize) -> ItemStack {
        self.slots
            .as_deref()
            .and_then(|s| s.get(index))
            .cloned()
            .unwrap_or_else(ItemStack::empty)
    }

    /// 写入第 `index` 格物品。
    pub fn set(&mut self, index: usize, stack: ItemStack) {
        if index >= Self::SIZE {
            return;
        }
        if stack.is_empty() {
            if let Some(slots) = self.slots.as_deref_mut() {
                slots[index] = ItemStack::empty();
            }
            return;
        }
        let slots = self
            .slots
            .get_or_insert_with(|| Box::new(std::array::from_fn(|_| ItemStack::empty())));
        slots[index] = stack;
    }

    /// 所有槽是否都为空。
    pub fn is_empty(&self) -> bool {
        self.slots
            .as_deref()
            .is_none_or(|slots| slots.iter().all(ItemStack::is_empty))
    }

    /// `SimpleContainer.canAddItem(ItemStack)`。
    pub fn can_add_item(&self, stack: &ItemStack) -> bool {
        if stack.is_empty() {
            return false;
        }
        let Some(slots) = self.slots.as_deref() else {
            return true;
        };
        let max = item_max(stack).min(64);
        for slot in slots {
            if slot.is_empty() || (same_item_components(slot, stack) && slot.count < max) {
                return true;
            }
        }
        false
    }

    /// `SimpleContainer.addItem(ItemStack)`：先合入已有同类槽，再放进空槽，返回剩余部分。
    pub fn add_item(&mut self, stack: ItemStack) -> ItemStack {
        if stack.is_empty() {
            return ItemStack::empty();
        }
        let mut leftover = stack;
        let max = item_max(&leftover).min(64);
        let slots = self
            .slots
            .get_or_insert_with(|| Box::new(std::array::from_fn(|_| ItemStack::empty())));
        // `moveItemToOccupiedSlotsWithSameType`
        for slot in slots.iter_mut() {
            if same_item_components(slot, &leftover) && slot.count < max {
                let take = leftover.count.min(max - slot.count);
                if take > 0 {
                    slot.count += take;
                    leftover.count -= take;
                    if leftover.count <= 0 {
                        return ItemStack::empty();
                    }
                }
            }
        }
        // `moveItemToEmptySlots`
        for slot in slots.iter_mut() {
            if slot.is_empty() {
                let take = leftover.count.min(max);
                let mut placed = leftover.clone();
                placed.count = take;
                *slot = placed;
                leftover.count -= take;
                if leftover.count <= 0 {
                    return ItemStack::empty();
                }
            }
        }
        leftover
    }

    /// `SimpleContainer.countItem(Item)`。
    pub fn count_item(&self, item: i32) -> i32 {
        let Some(slots) = self.slots.as_deref() else {
            return 0;
        };
        slots
            .iter()
            .filter(|s| !s.is_empty() && s.item == item)
            .map(|s| s.count)
            .sum()
    }

    /// `SimpleContainer.removeItemType(Item, int)`：按槽序倒扣，返回实际扣出的物品堆。
    pub fn remove_item_type(&mut self, item: i32, mut count: i32) -> ItemStack {
        let mut removed = 0;
        if let Some(slots) = self.slots.as_deref_mut() {
            for slot in slots.iter_mut() {
                if count <= 0 {
                    break;
                }
                if !slot.is_empty() && slot.item == item {
                    let take = slot.count.min(count);
                    slot.count -= take;
                    count -= take;
                    removed += take;
                    if slot.count <= 0 {
                        *slot = ItemStack::empty();
                    }
                }
            }
        }
        if removed > 0 {
            ItemStack::new(item, removed)
        } else {
            ItemStack::empty()
        }
    }

    /// 从第 `index` 格减去 `amount` 个物品。
    pub fn shrink_slot(&mut self, index: usize, amount: i32) {
        if let Some(slots) = self.slots.as_deref_mut()
            && let Some(slot) = slots.get_mut(index)
        {
            slot.count -= amount;
            if slot.count <= 0 {
                *slot = ItemStack::empty();
            }
        }
    }

    /// `Villager.hasFarmSeeds()`。
    pub fn has_farm_seeds(&self) -> bool {
        self.slots.as_deref().is_some_and(|slots| {
            slots.iter().any(|s| {
                !s.is_empty() && item_is_tagged(s.item, item_tags::VILLAGER_PLANTABLE_SEEDS)
            })
        })
    }

    /// `Villager.countFoodPointsInInventory()`。
    pub fn count_food_points(&self) -> i32 {
        let Some(slots) = self.slots.as_deref() else {
            return 0;
        };
        slots
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.count * villager_food_nutrition(s.item))
            .sum()
    }

    /// 编成官方 `Inventory` NBT 列表（`SimpleContainer.storeAsItemList`）；空背包写空列表。
    pub fn to_nbt(&self) -> Tag {
        let mut list = Vec::new();
        if let Some(slots) = self.slots.as_deref() {
            for stack in slots {
                if stack.is_empty() {
                    continue;
                }
                if let Ok(tag) = item_slots::encode_item(stack) {
                    list.push(tag);
                }
            }
        }
        Tag::List(list)
    }

    /// 从官方 `Inventory` NBT 列表读回（`SimpleContainer.fromItemList`）。
    pub fn read_nbt(&mut self, tag: Option<&Tag>) {
        self.slots = None;
        let Some(Tag::List(entries)) = tag else {
            return;
        };
        for entry in entries {
            if let Ok(stack) = item_slots::decode_item(entry)
                && !stack.is_empty()
            {
                let _ = self.add_item(stack);
            }
        }
    }
}

fn same_item_components(a: &ItemStack, b: &ItemStack) -> bool {
    !a.is_empty()
        && !b.is_empty()
        && a.item == b.item
        && a.added == b.added
        && a.removed == b.removed
}

/// `DataComponents.VILLAGER_FOOD` 对应的营养值（面包 4，胡萝卜/马铃薯/甜菜根 1）。
///
/// Nutrition value from `DataComponents.VILLAGER_FOOD` (bread 4; carrot/potato/beetroot 1).
pub const fn villager_food_nutrition(item: i32) -> i32 {
    match item {
        items::BREAD => 4,
        items::CARROT | items::POTATO | items::BEETROOT => 1,
        _ => 0,
    }
}

/// 村民自己的状态。默认 `(plains, none, 1)`，`VillagerDataFinalized` 缺省假。
///
/// The villager's own state; defaults to `(plains, none, 1)` and unfinalized.
#[derive(Debug, Clone, PartialEq)]
pub struct Villager {
    /// `VillagerData.type` 的注册表 ID。
    pub kind: i32,
    /// `VillagerData.profession` 的注册表 ID。
    pub profession: i32,
    /// `VillagerData.level`，构造时夹到至少 1。
    pub level: i32,
    pub finalized: bool,
    /// `AbstractVillager.DATA_UNHAPPY_COUNTER`。
    pub unhappy: i32,
    /// `Villager.foodLevel`，存档里是 BYTE。
    pub food_level: i8,
    pub xp: i32,
    pub last_restock: i64,
    pub last_gossip_decay: i64,
    pub restocks_today: i32,
    /// `Gossips` 原文。期 2 不解析，读进来原样写回。
    pub gossips: Option<Tag>,
    /// `Offers` 原文。交易在期 5。
    pub offers: Option<Tag>,
    /// `AbstractVillager.inventory`：8 格物品栏。
    ///
    /// `AbstractVillager.inventory`: 8-slot villager inventory.
    pub inventory: VillagerInventory,
    /// `LivingEntity.isSleeping()`：睡在床上时为真（睡觉行为第 4 期接入）。
    ///
    /// `LivingEntity.isSleeping()`: true while resting in a bed (sleep behaviour lands in phase 4).
    pub sleeping: bool,
    /// 当前是否正在与玩家交易（交易系统第 5 期接入；村民降频豁免看它）。
    ///
    /// True while trading with a player (trading lands in phase 5; villager throttling
    /// exempts trading villagers).
    pub trading: bool,
    /// `NearestBedSensor`（仅幼年村民使用）的批次缓存，按需装箱。
    ///
    /// Lazily boxed batch cache for baby villagers' `NearestBedSensor`.
    pub nearest_bed_cache: Option<Box<NearestBedCache>>,
}

impl Default for Villager {
    fn default() -> Self {
        Self {
            kind: facts::type_id(facts::DEFAULT_TYPE).unwrap_or(0),
            profession: facts::profession_id(facts::NONE_PROFESSION).unwrap_or(0),
            level: 1,
            finalized: false,
            unhappy: 0,
            food_level: 0,
            xp: 0,
            last_restock: 0,
            last_gossip_decay: 0,
            restocks_today: 0,
            gossips: None,
            offers: None,
            inventory: VillagerInventory::default(),
            sleeping: false,
            trading: false,
            nearest_bed_cache: None,
        }
    }
}

/// 村民的紧凑公开状态，放进 [`crate::entity::KindPublic`] 里：第 4–6 期别的村民
/// （繁殖、职业分配、铁傀儡生成）和并行驱动的只读视图只读这一份，不用翻整张
/// `MobPublic`，也不会把十几个村民字段摊进每种生物的公开快照。
///
/// 期 2 只有骨架：现在只填已经拿得到的职业和经验。其余字段是第 3 期接入大脑 / POI /
/// 睡觉 / 繁殖逻辑后再填的占位，缺省值表示「还没有」。
///
/// Compact public villager state, carried in [`crate::entity::KindPublic`]: phases 4–6
/// cross-villager reads (breeding, profession assignment, golem spawning) and the
/// parallel driver's read-only view read just this, without scanning the whole
/// `MobPublic` or scattering a dozen villager fields onto every mob's public snapshot.
///
/// Phase 2 is a skeleton: only the already-available profession and experience are
/// filled. The rest are placeholders for the phase-3 brain / POI / sleep / breeding
/// wiring; their default means "not present yet".
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VillagerPublic {
    /// `VillagerData.profession` 的注册表 ID。
    ///
    /// The `VillagerData.profession` registry id.
    pub profession: i32,
    /// `Villager.getVillagerXp()`：交易经验。
    ///
    /// `Villager.getVillagerXp()`: trading experience.
    pub experience: i32,
    /// `MemoryModuleType.JOB_SITE`（`GlobalPos`）。还没有村民大脑，恒 `None`。
    ///
    /// `MemoryModuleType.JOB_SITE` (`GlobalPos`); `None` until villagers have a brain.
    pub job_site: Option<GlobalPos>,
    /// `MemoryModuleType.POTENTIAL_JOB_SITE`（`GlobalPos`），恒 `None`。
    ///
    /// `MemoryModuleType.POTENTIAL_JOB_SITE` (`GlobalPos`); always `None` for now.
    pub potential_job_site: Option<GlobalPos>,
    /// 当前路径（`MemoryModuleType.PATH`）的首 / 尾节点，恒 `None`。
    ///
    /// The first / last node of the current path; always `None` for now.
    pub path_front: Option<BlockPos>,
    pub path_back: Option<BlockPos>,
    /// `LivingEntity.isSleeping()`，恒假。
    ///
    /// `LivingEntity.isSleeping()`; always false for now.
    pub sleeping: bool,
    /// `MemoryModuleType.LAST_SLEPT`（绝对刻号），恒 `None`。
    ///
    /// `MemoryModuleType.LAST_SLEPT` (absolute game time); always `None` for now.
    pub last_slept: Option<i64>,
    /// `Villager.canBreed()` 的可繁殖位，恒假。
    ///
    /// The `Villager.canBreed()` breedable bit; always false for now.
    pub breedable: bool,
}

impl VillagerPublic {
    /// 从当前村民状态、大脑与导航拍一份公开快照。
    ///
    /// Snapshots the public villager state from the villager, brain, and navigation.
    pub fn of(villager: &Villager, brain: Option<&Brain>, navigation: &Navigation) -> Self {
        let (job_site, potential_job_site, last_slept) = match brain {
            Some(brain) => (
                brain.memories.global_pos(MemoryType::JobSite).cloned(),
                brain
                    .memories
                    .global_pos(MemoryType::PotentialJobSite)
                    .cloned(),
                brain.memories.long(MemoryType::LastSlept),
            ),
            None => (None, None, None),
        };
        let (path_front, path_back) = match navigation.path.as_ref() {
            Some(path) if !path.nodes.is_empty() => (
                Some(path.nodes[0].pos()),
                path.end_node().map(|node| node.pos()),
            ),
            _ => (None, None),
        };
        Self {
            profession: villager.profession,
            experience: villager.xp,
            job_site,
            potential_job_site,
            path_front,
            path_back,
            sleeping: villager.sleeping,
            last_slept,
            breedable: false,
        }
    }
}

impl Villager {
    /// `VillagerData.withProfession` / `Villager.setVillagerData`：
    /// 职业变化时按官方清空 `offers`（下次 `getOffers()` 重新生成）。
    ///
    /// `VillagerData.withProfession` / `Villager.setVillagerData`: clears `offers`
    /// whenever the profession changes, matching vanilla.
    pub fn set_profession(&mut self, profession: i32) {
        if self.profession != profession {
            self.offers = None;
        }
        self.profession = profession;
    }

    /// `VillagerData.withLevel`：等级夹在 `MIN_VILLAGER_LEVEL..=MAX_VILLAGER_LEVEL`。
    ///
    /// `VillagerData.withLevel`: clamps level to `MIN_VILLAGER_LEVEL..=MAX_VILLAGER_LEVEL`.
    pub fn with_level(&mut self, level: i32) {
        self.level = level.clamp(facts::MIN_VILLAGER_LEVEL, facts::MAX_VILLAGER_LEVEL);
    }

    /// `VillagerData.canLevelUp(level)`。
    pub const fn can_level_up(level: i32) -> bool {
        level >= facts::MIN_VILLAGER_LEVEL && level < facts::MAX_VILLAGER_LEVEL
    }

    /// `VillagerData.getMinXpPerLevel(level)`。
    pub fn min_xp_per_level(level: i32) -> i32 {
        if Self::can_level_up(level) {
            facts::NEXT_LEVEL_XP_THRESHOLDS[(level - 1) as usize]
        } else {
            0
        }
    }

    /// `VillagerData.getMaxXpPerLevel(level)`。
    pub fn max_xp_per_level(level: i32) -> i32 {
        if Self::can_level_up(level) {
            facts::NEXT_LEVEL_XP_THRESHOLDS[level as usize]
        } else {
            0
        }
    }

    /// `Villager.shouldIncreaseLevel()`。
    pub fn should_increase_level(&self) -> bool {
        Self::can_level_up(self.level) && self.xp >= Self::max_xp_per_level(self.level)
    }

    /// `Villager.shouldRestock(ServerLevel)` 占位接口（第 5 期接真实交易表和补货）。
    ///
    /// Placeholder for `Villager.shouldRestock(ServerLevel)` (wired to real trades in Phase 5).
    pub fn should_restock(&mut self, _game_time: i64) -> bool {
        false
    }

    /// `Villager.restock()` 占位接口（第 5 期接真实交易表和补货）。
    ///
    /// Placeholder for `Villager.restock()` (wired to real trades in Phase 5).
    pub fn restock(&mut self, game_time: i64) {
        self.last_restock = game_time;
        self.restocks_today += 1;
    }

    /// `VillagerData` 的完整默认值。
    pub fn is_default_data(&self) -> bool {
        self.kind == facts::type_id(facts::DEFAULT_TYPE).unwrap_or(0)
            && self.profession == facts::profession_id(facts::NONE_PROFESSION).unwrap_or(0)
            && self.level == 1
    }
    /// `VillagerData.CODEC`：`type` / `profession` 是注册表名字串，`level` 是整数。
    ///
    /// `VillagerData.CODEC`: the type and profession are registry name strings and
    /// the level is an int.
    pub fn data_tag(&self) -> Tag {
        let kind = facts::type_name(self.kind).unwrap_or(facts::DEFAULT_TYPE);
        let profession =
            facts::profession(self.profession).map_or(facts::NONE_PROFESSION, |entry| entry.name);
        Tag::Compound(vec![
            ("type".to_owned(), Tag::String(kind.to_owned())),
            ("profession".to_owned(), Tag::String(profession.to_owned())),
            ("level".to_owned(), Tag::Int(self.level)),
        ])
    }

    /// 读 `VillagerData`；缺的字段按官方的 `orElseGet` 缺省，等级至少 1。
    pub fn read_data(&mut self, value: &Tag) {
        if let Some(kind) = tag::string_or(value, "type").and_then(facts::type_id) {
            self.kind = kind;
        }
        if let Some(profession) = tag::string_or(value, "profession").and_then(facts::profession_id)
        {
            self.profession = profession;
        }
        if let Some(level) = tag::field(value, "level").and_then(|value| match value {
            Tag::Int(level) => Some(*level),
            _ => None,
        }) {
            self.level = level.max(facts::MIN_VILLAGER_LEVEL);
        }
    }

    /// `Villager.hasExcessFood()`。
    pub fn has_excess_food(&self) -> bool {
        self.inventory.count_food_points() >= 24
    }

    /// `Villager.wantsMoreFood()`。
    pub fn wants_more_food(&self) -> bool {
        self.inventory.count_food_points() < 12
    }

    /// `Villager.wantsToPickUp(ServerLevel, ItemStack)`。
    ///
    /// `Villager.wantsToPickUp(ServerLevel, ItemStack)`.
    pub fn wants_to_pick_up(&self, stack: &ItemStack) -> bool {
        if stack.is_empty() {
            return false;
        }
        let wanted = item_is_tagged(stack.item, item_tags::VILLAGER_PICKS_UP)
            || villager_food_nutrition(stack.item) > 0
            || profession_requests_item(self.profession, stack.item);
        wanted && self.inventory.can_add_item(stack)
    }

    /// `Villager.addAdditionalSaveData` 的 8 个键，加上 `AbstractVillager` 的 `Offers` /
    /// `Inventory`。官方只在 `offers != null` 时写 `Offers`，非空背包才写 `Inventory`。
    ///
    /// The eight `Villager.addAdditionalSaveData` keys plus `AbstractVillager`'s
    /// `Offers` / `Inventory`. Vanilla writes `Offers` only when non-null and `Inventory`
    /// only when non-empty.
    pub fn write_nbt(&self, out: &mut Vec<(String, Tag)>) {
        if let Some(offers) = &self.offers {
            put(out, "Offers", offers.clone());
        }
        put(out, "Inventory", self.inventory.to_nbt());
        put(out, "VillagerData", self.data_tag());
        put(
            out,
            "VillagerDataFinalized",
            Tag::Byte(i8::from(self.finalized)),
        );
        put(out, "FoodLevel", Tag::Byte(self.food_level));
        put(
            out,
            "Gossips",
            self.gossips.clone().unwrap_or(Tag::List(Vec::new())),
        );
        put(out, "Xp", Tag::Int(self.xp));
        put(out, "LastRestock", Tag::Long(self.last_restock));
        put(out, "LastGossipDecay", Tag::Long(self.last_gossip_decay));
        put(out, "RestocksToday", Tag::Int(self.restocks_today));
    }

    /// `Villager.readAdditionalSaveData`：有 `VillagerData`（或已经标记 finalized）时
    /// finalized 置真。`Offers` 原样保留，`Inventory` 解入 8 格物品栏。
    pub fn read_nbt(&mut self, tag: &Tag) {
        let data = tag::field(tag, "VillagerData");
        let finalized = tag::bool_or(tag, "VillagerDataFinalized", false);
        if finalized || data.is_some() {
            self.finalized = true;
            if let Some(data) = data {
                self.read_data(data);
            }
        }
        self.food_level = tag::byte_or(tag, "FoodLevel", 0);
        self.gossips = tag::field(tag, "Gossips").cloned();
        self.xp = tag::int_or(tag, "Xp", 0);
        self.last_restock = tag::long_or(tag, "LastRestock", 0);
        self.last_gossip_decay = tag::long_or(tag, "LastGossipDecay", 0);
        self.restocks_today = tag::int_or(tag, "RestocksToday", 0);
        self.offers = tag::field(tag, "Offers").cloned();
        self.inventory.read_nbt(tag::field(tag, "Inventory"));
    }
}

/// `VillagerProfession.requestedItems().contains(item)`。
pub fn profession_requests_item(profession: i32, item: i32) -> bool {
    let Some(entry) = facts::profession(profession) else {
        return false;
    };
    let Some(item_info) = item_of(item) else {
        return false;
    };
    entry.requested_items.contains(&item_info.name)
}

/// `VillagerProfession.workSound()`：13 个有工作站的职业对应的工作音效名。
///
/// `VillagerProfession.workSound()`: work sound id for each of the 13 working professions.
pub fn work_sound(profession: i32) -> Option<&'static str> {
    let entry = facts::profession(profession)?;
    match entry.name {
        "minecraft:armorer" => Some("minecraft:entity.villager.work_armorer"),
        "minecraft:butcher" => Some("minecraft:entity.villager.work_butcher"),
        "minecraft:cartographer" => Some("minecraft:entity.villager.work_cartographer"),
        "minecraft:cleric" => Some("minecraft:entity.villager.work_cleric"),
        "minecraft:farmer" => Some("minecraft:entity.villager.work_farmer"),
        "minecraft:fisherman" => Some("minecraft:entity.villager.work_fisherman"),
        "minecraft:fletcher" => Some("minecraft:entity.villager.work_fletcher"),
        "minecraft:leatherworker" => Some("minecraft:entity.villager.work_leatherworker"),
        "minecraft:librarian" => Some("minecraft:entity.villager.work_librarian"),
        "minecraft:mason" => Some("minecraft:entity.villager.work_mason"),
        "minecraft:shepherd" => Some("minecraft:entity.villager.work_shepherd"),
        "minecraft:toolsmith" => Some("minecraft:entity.villager.work_toolsmith"),
        "minecraft:weaponsmith" => Some("minecraft:entity.villager.work_weaponsmith"),
        _ => None,
    }
}

/// 落地 [`crate::entity::ctx::Command::VillagerPickUpItem`]（`InventoryCarrier.pickUpItem`）。
///
/// Applies [`crate::entity::ctx::Command::VillagerPickUpItem`] (`InventoryCarrier.pickUpItem`).
pub(crate) fn apply_pick_up(world: &mut World, item: EntityUid, me: &mut Entity) {
    let Some(entity) = world.entity(item) else {
        return;
    };
    if entity.removed {
        return;
    }
    let EntityKind::Item(ground) = &entity.kind else {
        return;
    };
    if ground.stack.is_empty() || ground.pickup_delay > 0 {
        return;
    }
    let stack = ground.stack.clone();
    let mut leftover: Option<ItemStack> = None;
    me.with_mob(|_, mob| {
        if mob.villager.wants_to_pick_up(&stack) {
            leftover = Some(mob.villager.inventory.add_item(stack.clone()));
        }
    });
    let Some(remaining) = leftover else {
        return;
    };
    world.with_entity_mut(item, &mut |entity| {
        if remaining.is_empty() {
            entity.discard();
        } else if let EntityKind::Item(ground) = &mut entity.kind {
            ground.stack.count = remaining.count;
        }
    });
}

/// 落地 [`crate::entity::ctx::Command::VillagerCompost`]（`WorkAtComposter.compostItems`）。
///
/// Applies [`crate::entity::ctx::Command::VillagerCompost`] (`WorkAtComposter.compostItems`).
pub(crate) fn apply_compost(world: &mut World, pos: BlockPos, me: &mut Entity) {
    const COMPOSTABLE_ITEMS: [i32; 2] = [items::WHEAT_SEEDS, items::BEETROOT_SEEDS];
    let mut initial_state = world.block_state(pos);
    if !initial_state.is_a(mc_bamboo_data::BlockAncestor::ComposterBlock) {
        return;
    }
    if initial_state.get(LEVEL_COMPOSTER) == COMPOST_READY {
        initial_state = extract_produce(initial_state, world, pos);
    }
    let mut remaining = 20_i32;
    let mut seen = [0_i32; 2];
    let mut state = initial_state;
    for slot_idx in (0..VillagerInventory::SIZE).rev() {
        if remaining <= 0 {
            break;
        }
        let stack = me
            .kind
            .mob()
            .map_or_else(ItemStack::empty, |m| m.villager.inventory.get(slot_idx));
        let Some(kind_idx) = COMPOSTABLE_ITEMS.iter().position(|&id| id == stack.item) else {
            continue;
        };
        let count = stack.count;
        let total = seen[kind_idx] + count;
        seen[kind_idx] = total;
        let to_insert = (total - 10).min(remaining).min(count);
        if to_insert <= 0 {
            continue;
        }
        remaining -= to_insert;
        let layers = item_of(stack.item).and_then(|info| info.compostable);
        let Some(layers) = layers else {
            continue;
        };
        for _ in 0..to_insert {
            if state.get(LEVEL_COMPOSTER) < COMPOST_MAX_LEVEL {
                state = add_layer_state(state, world, pos, layers);
                me.with_mob(|_, mob| mob.villager.inventory.shrink_slot(slot_idx, 1));
            }
            if state.get(LEVEL_COMPOSTER) == COMPOST_MAX_LEVEL {
                world.push_effect(Effect::ComposterFill {
                    pos,
                    success: state != initial_state,
                });
                return;
            }
        }
    }
    world.push_effect(Effect::ComposterFill {
        pos,
        success: state != initial_state,
    });
}

/// 把职业的 `held_job_site` 列表转成 POI 类型位掩码（`1 << kind`）。
///
/// Converts a profession's `held_job_site` names into a POI type bitmask (`1 << kind`).
pub fn held_job_site_mask(profession: i32) -> u32 {
    let Some(entry) = facts::profession(profession) else {
        return 0;
    };
    poi_names_to_mask(entry.held_job_site)
}

/// 把职业的 `acquirable_job_site` 列表转成 POI 类型位掩码（`1 << kind`）。
///
/// Converts a profession's `acquirable_job_site` names into a POI type bitmask (`1 << kind`).
pub fn acquirable_job_site_mask(profession: i32) -> u32 {
    let Some(entry) = facts::profession(profession) else {
        return 0;
    };
    poi_names_to_mask(entry.acquirable_job_site)
}

/// 遍历 `VILLAGER_PROFESSION` 注册表找第一个 `held_job_site` 包含 `poi_kind` 的职业 ID，
/// 对应 `AssignProfessionFromJobSite`。
///
/// Finds the first profession in registry order whose `held_job_site` matches `poi_kind`,
/// matching `AssignProfessionFromJobSite`.
pub fn profession_for_poi(poi_kind: u8) -> Option<i32> {
    let poi_name = poi_data::POI_TYPES.get(usize::from(poi_kind))?.name;
    facts::PROFESSIONS
        .iter()
        .position(|entry| {
            entry
                .held_job_site
                .iter()
                .any(|name| name.strip_prefix("minecraft:").unwrap_or(name) == poi_name)
        })
        .map(|idx| idx as i32)
}

/// `PoiType.validRange()`，未知类型回退 1。
///
/// `PoiType.validRange()`, falling back to 1 for unknown ids.
pub fn poi_valid_range(poi_kind: u8) -> i32 {
    poi_data::POI_TYPES
        .get(usize::from(poi_kind))
        .map_or(1, |entry| entry.valid_range)
}

/// 单个 POI 名字转位掩码。
///
/// Single POI name to bitmask.
pub fn poi_name_mask(name: &str) -> u32 {
    poi_data::poi_type_id(name).map_or(0, |id| 1_u32 << id)
}

fn poi_names_to_mask(names: &[&str]) -> u32 {
    let mut mask = 0_u32;
    for name in names {
        if let Some(id) = poi_data::poi_type_id(name) {
            mask |= 1_u32 << id;
        }
    }
    mask
}

/// `Villager.finalizeSpawn`：繁殖时职业清空，未 finalized 时按群系选品种，然后走
/// `AbstractVillager` → `AgeableMob` → `Mob`。
///
/// (`AbstractVillager` 在没有组数据时新建 `new AgeableMobGroupData(false)`：村民不会
/// 因为组计数变成幼年。)
///
/// `Villager.finalizeSpawn`: breeding clears the profession, an unfinalized villager
/// takes its type from the biome, then the `AbstractVillager` -> `AgeableMob` -> `Mob`
/// layers run. A fresh `AgeableMobGroupData(false)` means no baby roll.
pub(crate) fn finalize_spawn(
    world: &mut World,
    entity: &mut Entity,
    reason: SpawnReason,
    group: &mut Option<spawn::SpawnGroup>,
) {
    let biome = world.biome_id(entity.block_position());
    let random = world.random();
    entity.with_mob(|_, mob| {
        if reason == SpawnReason::Breeding {
            mob.villager.profession = facts::profession_id(facts::NONE_PROFESSION).unwrap_or(0);
        }
        // `VillagerDataHolder.finalizeVillagerType`：只看 finalized，没有随机数。
        if !mob.villager.finalized {
            mob.villager.kind = facts::type_for_biome(biome);
            mob.villager.finalized = true;
        }
        spawn::finalize_villager_ageable(random, mob, group);
    });
    // 按当前世界时钟落地初始日程活动（与官方构造函数末尾 `registerBrainGoals` 一致，不抽随机数）。
    // Apply the initial schedule activity from the world clock (matches `registerBrainGoals`
    // at the end of the vanilla constructor; draws no RNG).
    let game_time = world.game_time();
    entity.with_mob(|_, mob| {
        super::villager_ai::register_brain_goals(mob, world, game_time);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::mob::{MobType, nbt};
    use crate::{BlockTicks, World, testing::TestLevel};
    use mc_bamboo_core::Vec3;

    /// 成年和幼年的尺寸、眼高、属性都来自官方字节码。
    #[test]
    fn dimensions_attributes_and_spawn_rules_match_vanilla() {
        let villager = super::super::Mob::new(MobType::Villager);
        assert_eq!(
            villager
                .attributes
                .value(mc_bamboo_data::registries::attribute::MAX_HEALTH),
            20.0
        );
        assert_eq!(
            villager
                .attributes
                .value(mc_bamboo_data::registries::attribute::FOLLOW_RANGE),
            16.0
        );
        assert_eq!(
            villager
                .attributes
                .value(mc_bamboo_data::registries::attribute::MOVEMENT_SPEED),
            0.5
        );
        assert_eq!(MobType::Villager.dimensions(false), (0.6, 1.95, 1.62));
        assert_eq!(MobType::Villager.dimensions(true), (0.49, 0.98, 0.63));
        assert!(MobType::Villager.is_ageable());
        assert!(!MobType::Villager.is_animal());
        assert!(!MobType::Villager.removes_when_far_away());
        assert!(villager.can_pick_up_loot);
        assert!(villager.navigation.flags.can_open_doors);
        assert!(villager.navigation.flags.can_float);
        assert_eq!(villager.navigation.required_path_length, 48.0);
        assert_eq!(
            MobType::Villager.mob_category(),
            mc_bamboo_data::spawning::MobCategory::Misc
        );
    }

    /// `/summon minecraft:villager`：测试世界是平原，品种取平原且 finalized。
    #[test]
    fn summon_takes_the_biome_type_and_finalizes() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let uid = spawn::summon_mob(
            &mut world,
            MobType::Villager,
            Vec3::new(0.5, 64.0, 0.5),
            None,
        )
        .expect("村民可以召唤");
        world.with_entity_mut(uid, &mut |entity| {
            entity.with_mob(|_, mob| {
                assert_eq!(
                    mob.villager.kind,
                    facts::type_id("minecraft:plains").unwrap()
                );
                assert!(mob.villager.finalized);
                assert_eq!(
                    mob.villager.profession,
                    facts::profession_id("minecraft:none").unwrap()
                );
                assert_eq!(mob.villager.level, 1);
            });
        });
    }

    /// 存档的 8 个键和原样保留的 `Offers` / `Inventory` 能读回。
    #[test]
    fn nbt_round_trip_keeps_villager_keys() {
        let mut entity = Entity::by_mob_type(MobType::Villager, Vec3::default());
        entity.with_mob(|_, mob| {
            let villager = &mut mob.villager;
            villager.kind = facts::type_id("minecraft:taiga").unwrap();
            villager.profession = facts::profession_id("minecraft:farmer").unwrap();
            villager.level = 3;
            villager.finalized = true;
            villager.unhappy = 7;
            villager.food_level = 4;
            villager.xp = 20;
            villager.last_restock = 1234;
            villager.last_gossip_decay = 5678;
            villager.restocks_today = 2;
            villager.gossips = Some(Tag::List(Vec::new()));
            villager.offers = Some(Tag::Compound(vec![(
                "Recipes".to_owned(),
                Tag::List(Vec::new()),
            )]));
            villager.inventory.add_item(ItemStack::new(items::WHEAT, 3));
        });
        let tag = nbt::write_mob(&entity.split()).expect("写村民");
        let mc_bamboo_nbt::Tag::Compound(entries) = &tag else {
            panic!("生物存档是复合标签");
        };
        for key in [
            "VillagerData",
            "VillagerDataFinalized",
            "FoodLevel",
            "Gossips",
            "Xp",
            "LastRestock",
            "LastGossipDecay",
            "RestocksToday",
            "Offers",
            "Inventory",
        ] {
            assert!(entries.iter().any(|(name, _)| name == key), "缺 {key}");
        }
        let loaded = nbt::read_mob(&tag, "minecraft:villager", 3)
            .expect("读村民")
            .expect("已建模");
        let mut entity = loaded.into_entity();
        entity.with_mob(|_, mob| {
            assert_eq!(
                mob.villager.kind,
                facts::type_id("minecraft:taiga").unwrap()
            );
            assert_eq!(
                mob.villager.profession,
                facts::profession_id("minecraft:farmer").unwrap()
            );
            assert_eq!(mob.villager.level, 3);
            assert!(mob.villager.finalized);
            assert_eq!(mob.villager.food_level, 4);
            assert_eq!(mob.villager.xp, 20);
            assert_eq!(mob.villager.last_restock, 1234);
            assert_eq!(mob.villager.last_gossip_decay, 5678);
            assert_eq!(mob.villager.restocks_today, 2);
            assert!(mob.villager.offers.is_some());
            assert_eq!(mob.villager.inventory.count_item(items::WHEAT), 3);
        });
    }

    /// `VillagerData` 缺字段时按官方缺省补 plains / none / 1。
    #[test]
    fn missing_villager_data_fields_take_vanilla_defaults() {
        let mut villager = Villager::default();
        villager.read_data(&Tag::Compound(vec![(
            "type".to_owned(),
            Tag::String("minecraft:snow".to_owned()),
        )]));
        assert_eq!(villager.kind, facts::type_id("minecraft:snow").unwrap());
        assert_eq!(
            villager.profession,
            facts::profession_id("minecraft:none").unwrap()
        );
        assert_eq!(villager.level, 1);
        villager.read_data(&Tag::Compound(vec![("level".to_owned(), Tag::Int(0))]));
        assert_eq!(villager.level, 1);
    }

    /// 村民默认尺寸是成年的，构造时即带大脑和日程。
    #[test]
    fn villager_has_brain_and_adult_dimensions() {
        let villager = super::super::Mob::new(MobType::Villager);
        assert!(villager.brain.is_some());
        assert_eq!(villager.dimensions(), (0.6, 1.95));
    }

    /// `VillagerPublic` 挂在 `KindPublic::Mob` 下，只给村民装箱，非村民为 `None`，
    /// 同步大脑中的工作站、上次睡觉刻和睡眠标志。
    #[test]
    fn villager_public_reads_brain_and_state() {
        let farmer = facts::profession_id("minecraft:farmer").unwrap();
        let job = GlobalPos {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(10, 64, 20),
        };
        let mut entity = Entity::by_mob_type(MobType::Villager, Vec3::new(1.0, 64.0, 2.0));
        entity.with_mob(|_, mob| {
            mob.villager.profession = farmer;
            mob.villager.xp = 42;
            mob.villager.sleeping = true;
            let brain = mob.brain.as_mut().expect("villager has brain");
            brain.memories.set(
                MemoryType::JobSite,
                super::super::brain::memory::MemoryValue::GlobalPos(job.clone()),
            );
            brain.memories.set(
                MemoryType::LastSlept,
                super::super::brain::memory::MemoryValue::Long(900),
            );
        });
        let row = entity.split();
        let public = row
            .public
            .kind
            .villager()
            .expect("村民必须带 VillagerPublic");
        assert_eq!(public.profession, farmer);
        assert_eq!(public.experience, 42);
        assert_eq!(public.job_site, Some(job));
        assert_eq!(public.potential_job_site, None);
        assert_eq!(public.path_front, None);
        assert_eq!(public.path_back, None);
        assert!(public.sleeping);
        assert_eq!(public.last_slept, Some(900));
        assert!(!public.breedable);

        let cow = Entity::by_mob_type(MobType::Cow, Vec3::new(1.0, 64.0, 2.0)).split();
        assert!(cow.public.kind.villager().is_none());
    }
}
