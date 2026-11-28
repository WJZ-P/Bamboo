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
    merchant::{self, MerchantOffer},
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
        misc::{COMPOST_MAX_LEVEL, COMPOST_READY, extract_produce},
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
    /// `Villager.lastRestockCheckDay`：`shouldRestock` 每刻写，判断跨天。不存档。
    ///
    /// `Villager.lastRestockCheckDay`, rewritten every `shouldRestock` tick; not saved.
    pub last_restock_check_day: i64,
    pub last_gossip_decay: i64,
    pub restocks_today: i32,
    /// `Gossips` 原文。期 2 不解析，读进来原样写回。
    pub gossips: Option<Tag>,
    /// `AbstractVillager.offers`：`None` 表示还没生成（官方 `offers == null`）。
    pub offers: Option<Vec<MerchantOffer>>,
    /// `AbstractVillager.inventory`：8 格物品栏。
    ///
    /// `AbstractVillager.inventory`: 8-slot villager inventory.
    pub inventory: VillagerInventory,
    /// `LivingEntity.isSleeping()`：睡在床上时为真。
    ///
    /// `LivingEntity.isSleeping()`: true while resting in a bed.
    pub sleeping: bool,
    /// `LivingEntity.SLEEPING_POS_ID`：睡在床上时的床头坐标。
    ///
    /// `LivingEntity.SLEEPING_POS_ID`: the bed head position while sleeping.
    pub sleeping_pos: Option<BlockPos>,
    /// `AbstractVillager.tradingPlayer`：正在交易的玩家的实体 ID（`PlayerId.0`，也是
    /// `PlayerView.uid`）。`None` 对应官方 `tradingPlayer == null`。不存档。
    ///
    /// `AbstractVillager.tradingPlayer`: the trading player's entity id (`PlayerId.0`,
    /// also `PlayerView.uid`). `None` means vanilla's `tradingPlayer == null`. Not saved.
    pub trading_player: Option<i32>,
    /// `Villager.lastTradedPlayer`：最近一次交易的玩家，供声望事件使用（期 6 接）。不存档。
    ///
    /// `Villager.lastTradedPlayer`: the most recent trading player for reputation events
    /// (wired in phase 6). Not saved.
    pub last_traded_player: Option<i32>,
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
            last_restock_check_day: 0,
            last_gossip_decay: 0,
            restocks_today: 0,
            gossips: None,
            offers: None,
            inventory: VillagerInventory::default(),
            sleeping: false,
            sleeping_pos: None,
            trading_player: None,
            last_traded_player: None,
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

    /// `AbstractVillager.setTradingPlayer(Player)` / `Villager.setTradingPlayer(Player)`。
    /// 26.3 的 `stopTrading()` 除清空字段外没有别的副作用，非交易态转空时等价于直接赋值。
    ///
    /// `AbstractVillager.setTradingPlayer(Player)` / `Villager.setTradingPlayer(Player)`.
    /// In 26.3 `stopTrading()` only clears the field, so a non-null→null transition is a
    /// plain assignment.
    pub fn set_trading_player(&mut self, player: Option<i32>) {
        self.trading_player = player;
    }

    /// `AbstractVillager.stopTrading()`：清空交易玩家。
    ///
    /// `AbstractVillager.stopTrading()`: clears the trading player.
    pub fn stop_trading(&mut self) {
        self.set_trading_player(None);
    }

    /// `AbstractVillager.isTrading()`。
    pub const fn is_trading(&self) -> bool {
        self.trading_player.is_some()
    }

    /// `AbstractVillager.getTradingPlayer()`。
    pub const fn trading_player(&self) -> Option<i32> {
        self.trading_player
    }

    /// `AbstractVillager.stillValid(Player)` 里「交易玩家相同」的那一项；存活与距离由调用方判。
    ///
    /// The "same trading player" part of `AbstractVillager.stillValid(Player)`; liveness and
    /// distance are checked by the caller.
    pub const fn still_valid_trading(&self, player: i32) -> bool {
        matches!(self.trading_player, Some(id) if id == player)
    }

    /// `Villager.shouldRestock(ServerLevel)`：跨天或长时间没补货时重置当天计数，
    /// 再返回「今天还能补且需要补」。`day` 是 OVERWORLD_DAY 的天数（`clockTicks / 24000`）。
    ///
    /// `Villager.shouldRestock(ServerLevel)`: on a new day or after a long overdue gap,
    /// reset the daily counter first, then report whether a restock is allowed and needed.
    /// `day` is the OVERWORLD_DAY day number (`clockTicks / 24000`).
    pub fn should_restock(&mut self, game_time: i64, day: i64) -> bool {
        let long_overdue = game_time > self.last_restock + 12_000;
        let new_day = self.last_restock_check_day > 0 && day > self.last_restock_check_day;
        self.last_restock_check_day = day;
        if long_overdue || new_day {
            self.last_restock = game_time;
            self.reset_number_of_restocks();
        }
        allowed_to_restock(self, game_time) && needs_to_restock(self)
    }

    /// `Villager.restock()`：先按使用次数给每笔交易折算需求，再清零使用次数。
    /// 有交易玩家时按官方重算特殊价格（声望期 6，等价 `resetSpecialPrices`）。
    ///
    /// `Villager.restock()`: fold each offer's uses into demand, then clear the uses.
    /// With a trading player it recomputes special prices (gossip is phase 6, so this is
    /// equivalent to `resetSpecialPrices`).
    pub fn restock(&mut self, game_time: i64) {
        self.update_demand();
        for offer in self.offers.iter_mut().flatten() {
            offer.uses = 0;
        }
        if self.trading_player.is_some() {
            self.reset_special_prices();
        }
        self.last_restock = game_time;
        self.restocks_today += 1;
    }

    /// `Villager.updateDemand()`：遍历全部交易调 `MerchantOffer.updateDemand()`。
    fn update_demand(&mut self) {
        for offer in self.offers.iter_mut().flatten() {
            update_offer_demand(offer);
        }
    }

    /// `Villager.resetNumberOfRestocks()`。
    fn reset_number_of_restocks(&mut self) {
        self.catch_up_demand();
        self.restocks_today = 0;
    }

    /// `Villager.catchUpDemand()`：把没补到的那几次的需求补上。
    fn catch_up_demand(&mut self) {
        let misses = (2 - self.restocks_today).max(0);
        if misses > 0 {
            for offer in self.offers.iter_mut().flatten() {
                offer.uses = 0;
            }
        }
        for _ in 0..misses {
            self.update_demand();
        }
        if self.trading_player.is_some() {
            self.reset_special_prices();
        }
    }

    /// `Villager.resetSpecialPrices()`（声望期 6 之前的 `updateSpecialPrices` 结果）。
    ///
    /// `Villager.resetSpecialPrices()` (the phase-6-free result of `updateSpecialPrices`).
    pub fn reset_special_prices(&mut self) {
        for offer in self.offers.iter_mut().flatten() {
            offer.special_price_diff = 0;
        }
    }

    /// `Villager.updateSpecialPrices(Player)`：声望恒 0（期 6），英雄效果按
    /// `0.3 + 0.0625 * amplifier` 打折。
    ///
    /// `Villager.updateSpecialPrices(Player)`: reputation is always 0 (phase 6); the hero
    /// effect discounts by `0.3 + 0.0625 * amplifier`.
    pub fn update_special_prices(&mut self, hero_amplifier: Option<i32>) {
        self.reset_special_prices();
        let factor = hero_amplifier.map_or(0.0, |level| 0.3 + 0.0625 * f64::from(level));
        if factor <= 0.0 {
            return;
        }
        for offer in self.offers.iter_mut().flatten() {
            let discount = (factor * f64::from(offer.base_cost_a.count))
                .floor()
                .max(1.0) as i32;
            offer.special_price_diff -= discount;
        }
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

    /// `AbstractVillager.addAdditionalSaveData`：官方只在 `offers != null` 时写
    /// `Offers`，背包按 `SimpleContainer` 存成列表。流浪商人和村民共用这一层。
    ///
    /// `AbstractVillager.addAdditionalSaveData`: writes `Offers` only when non-null,
    /// and the inventory as a `SimpleContainer` list. Villagers and wandering traders
    /// share this layer.
    pub fn write_abstract_nbt(&self, out: &mut Vec<(String, Tag)>) {
        if let Some(offers) = &self.offers
            && let Ok(encoded) = merchant::encode_offers(offers)
        {
            put(out, "Offers", encoded.to_tag());
        }
        put(out, "Inventory", self.inventory.to_nbt());
    }

    /// `Villager.addAdditionalSaveData` 自己的八个键。
    ///
    /// The eight keys of `Villager.addAdditionalSaveData` itself.
    pub fn write_villager_nbt(&self, out: &mut Vec<(String, Tag)>) {
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

    /// `AbstractVillager.readAdditionalSaveData`：`Offers` 与 `Inventory` 两层共用。
    ///
    /// `AbstractVillager.readAdditionalSaveData`: the shared `Offers` / `Inventory` layer.
    pub fn read_abstract_nbt(&mut self, tag: &Tag) {
        self.offers =
            tag::field(tag, "Offers").and_then(|offers| merchant::decode_offers(offers).ok());
        self.inventory.read_nbt(tag::field(tag, "Inventory"));
    }

    /// `Villager.readAdditionalSaveData`：有 `VillagerData`（或已经标记 finalized）时
    /// finalized 置真。
    pub fn read_villager_nbt(&mut self, tag: &Tag) {
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
    }
}

/// `MerchantOffer.updateDemand()`：`demand += 2 * uses - maxUses`。
///
/// `MerchantOffer.updateDemand()`: `demand += 2 * uses - maxUses`.
fn update_offer_demand(offer: &mut MerchantOffer) {
    offer.demand += 2 * offer.uses - offer.max_uses;
}

/// `Villager.allowedToRestock()`。
fn allowed_to_restock(v: &Villager, now: i64) -> bool {
    v.restocks_today == 0 || (v.restocks_today < 2 && now > v.last_restock + 2400)
}

/// `Villager.needsToRestock()`：任一交易 `uses > 0`。
fn needs_to_restock(v: &Villager) -> bool {
    v.offers
        .as_ref()
        .is_some_and(|offers| offers.iter().any(|offer| offer.uses > 0))
}

/// 一次交易完成后的状态转移结果：要撒多少经验、是否升级。
///
/// The state transition after one trade: how much experience to award, and whether the
/// villager levelled up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TradeOutcome {
    pub orb_amount: i32,
    pub leveled_up: bool,
}

/// `AbstractVillager.notifyTrade` → `Villager.rewardTradeXp` → `increaseMerchantCareer`
/// 的状态转移部分。`orb_base` 是调用方按村民**自己的**随机源算好的 `3 + nextInt(4)`；
/// `new_offers` 是新等级的交易（调用方持有 loot 表，blocks 不依赖它），只在升级时追加。
/// 经验球是否真撒由 `reward_exp` 与调用方决定；声望事件（期 6）在这里跳过。
///
/// The state-transition part of `AbstractVillager.notifyTrade` → `Villager.rewardTradeXp`
/// → `increaseMerchantCareer`. `orb_base` is `3 + nextInt(4)` drawn by the caller from the
/// villager's **own** random; `new_offers` are the new tier's trades (the caller owns the
/// loot tables, which blocks does not depend on) appended only on a level-up. Whether xp
/// orbs are spawned is `reward_exp` and the caller's business; the reputation event is
/// phase 6 and skipped here.
pub fn notify_trade(
    mob: &mut super::Mob,
    offer_xp: i32,
    orb_base: i32,
    new_offers: impl FnOnce() -> Vec<MerchantOffer>,
) -> TradeOutcome {
    let mut orb_amount = orb_base;
    // `AbstractVillager.notifyTrade`：`ambientSoundTime = -getAmbientSoundInterval()`，
    // 压住交易后的环境音，音高随机源才和官方同步。
    // `AbstractVillager.notifyTrade`: `ambientSoundTime = -getAmbientSoundInterval()`
    // suppresses the post-trade ambient sound so the pitch RNG stays aligned.
    mob.ambient_sound_time = -mob.mob_type.ambient_sound_interval();
    mob.villager.xp += offer_xp;
    mob.villager.last_traded_player = mob.villager.trading_player;
    let leveled_up = mob.villager.should_increase_level();
    if leveled_up {
        // 只在升级时才求值新交易，避免每次交易都推进 loot 随机序列。
        // Only evaluate the new offers on a level-up so loot RNG stays in sync.
        increase_merchant_career(mob, new_offers());
        // `rewardTradeXp` 升级后挂 200 刻、0 级的再生效果。
        // `rewardTradeXp` adds a 200-tick, amplifier-0 regeneration effect on level-up.
        super::living::add_effect(
            mob,
            super::effects::EffectInstance::new(
                mc_bamboo_data::registries::mob_effect::REGENERATION,
                200,
            ),
        );
        orb_amount += 5;
    }
    TradeOutcome {
        orb_amount,
        leveled_up,
    }
}

/// `Villager.increaseMerchantCareer`：`withLevel(level+1)`（职业不变，不清 offers）+
/// `updateTrades`（把新等级交易追加进现有 offers）。末尾按官方重算特殊价格。
///
/// `Villager.increaseMerchantCareer`: `withLevel(level+1)` (profession unchanged, offers
/// kept) + `updateTrades` (append the new tier's offers). Recomputes special prices at the
/// end like vanilla.
fn increase_merchant_career(mob: &mut super::Mob, new_offers: Vec<MerchantOffer>) {
    mob.villager.with_level(mob.villager.level + 1);
    mob.villager
        .offers
        .get_or_insert_with(Vec::new)
        .extend(new_offers);
    if mob.villager.trading_player.is_some() {
        mob.villager.reset_special_prices();
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
/// 背包在 tick 里已经加了 `taken`。这里按地上那堆**现在**的数量结算：够就照扣（精确驱动
/// 当场落地，永远够，结果和以前逐刻相同）；并行驱动里别的生物先落地拿走了一部分或全部
/// 时，不够的那部分从自己的背包里退回去。
///
/// Applies [`crate::entity::ctx::Command::VillagerPickUpItem`] (`InventoryCarrier.pickUpItem`).
///
/// The tick already added `taken` to the inventory. This settles against the ground
/// stack as it is **now**: when it covers `taken` it is shrunk as before (the precise
/// driver commits at once, so it always does and the result is unchanged tick for
/// tick); when another mob committed first in the parallel driver and took some or all
/// of it, the uncovered part goes back out of this mob's inventory.
pub(crate) fn apply_pick_up(
    world: &mut World,
    item: EntityUid,
    taken: &ItemStack,
    me: &mut Entity,
) {
    let mut granted = 0;
    world.with_entity_mut(item, &mut |entity| {
        if entity.removed {
            return;
        }
        let EntityKind::Item(ground) = &mut entity.kind else {
            return;
        };
        granted = taken.count.min(ground.stack.count);
        let remaining = ground.stack.count - granted;
        if remaining <= 0 {
            entity.discard();
        } else {
            ground.stack.count = remaining;
        }
    });
    let excess = taken.count - granted;
    if excess <= 0 {
        return;
    }
    me.with_mob(|_, mob| {
        if mob.mob_type == super::MobType::Allay {
            if let Some(slot) = mob.allay.inventory.first_mut() {
                slot.count -= excess;
                if slot.count <= 0 {
                    *slot = ItemStack::empty();
                }
            }
        } else {
            let _ = mob.villager.inventory.remove_item_type(taken.item, excess);
        }
    });
}

/// 落地 [`crate::entity::ctx::Command::VillagerCompostExtract`]（满桶产出骨粉并清空）。
///
/// Applies [`crate::entity::ctx::Command::VillagerCompostExtract`] (empties a ready composter).
pub(crate) fn apply_compost_extract(world: &mut World, pos: BlockPos) {
    let state = world.block_state(pos);
    if state.is_a(mc_bamboo_data::BlockAncestor::ComposterBlock)
        && state.get(LEVEL_COMPOSTER) == COMPOST_READY
    {
        let _ = extract_produce(state, world, pos);
    }
}

/// 落地 [`crate::entity::ctx::Command::VillagerCompostFinish`]（写入新层数并发 `levelEvent(1500)`）。
///
/// Applies [`crate::entity::ctx::Command::VillagerCompostFinish`] (commits composter level and emits `levelEvent(1500)`).
pub(crate) fn apply_compost_finish(world: &mut World, pos: BlockPos, level: i32, success: bool) {
    let state = world.block_state(pos);
    if !state.is_a(mc_bamboo_data::BlockAncestor::ComposterBlock) {
        return;
    }
    if success {
        let clamped = level.clamp(0, COMPOST_MAX_LEVEL);
        let next = state.with(LEVEL_COMPOSTER, clamped);
        world.set_block_and_update(pos, next);
        if clamped == COMPOST_MAX_LEVEL {
            world.schedule_tick(pos, state.block(), 20);
        }
    }
    world.push_effect(Effect::ComposterFill { pos, success });
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

/// `Villager.POI_MEMORIES` 对应的 POI 类型掩码。
///
/// POI type bitmask for each entry of `Villager.POI_MEMORIES`.
pub fn poi_memory_mask(profession: i32, memory: MemoryType) -> u32 {
    match memory {
        MemoryType::Home => poi_name_mask("home"),
        MemoryType::JobSite => held_job_site_mask(profession),
        MemoryType::PotentialJobSite => {
            // `PoiType.ALL_ACQUIRABLE_JOBS`：与 `none` 职业的可获取工作站掩码一致。
            let none = facts::profession_id(facts::NONE_PROFESSION).unwrap_or(0);
            acquirable_job_site_mask(none)
        }
        MemoryType::MeetingPoint => poi_name_mask("meeting"),
        _ => 0,
    }
}

/// `Villager.releasePoi(MemoryModuleType)`：从指定记忆表查 `GlobalPos`，若该处仍是对应
/// POI 类型则归还一张票。
///
/// `Villager.releasePoi(MemoryModuleType)`: releases one ticket at the remembered
/// `GlobalPos` when the POI at that position still matches `POI_MEMORIES`.
pub fn release_poi_from_memories(
    me: &mut Entity,
    profession: i32,
    memories: &super::brain::memory::Memories,
    ctx: &mut dyn crate::entity::ctx::EntityCtx,
    memory: MemoryType,
) {
    let mask = poi_memory_mask(profession, memory);
    if mask == 0 {
        return;
    }
    let Some(gp) = memories.global_pos(memory) else {
        return;
    };
    if gp.dimension != "minecraft:overworld" {
        return;
    }
    if ctx
        .blocks()
        .poi_type_at(gp.pos)
        .is_some_and(|kind| (mask & (1_u32 << kind)) != 0)
    {
        ctx.emit(crate::entity::ctx::Command::PoiRelease { pos: gp.pos }, me);
    }
}

/// `Villager.releasePoi`：当 `mob.brain` 仍在 `mob` 上时使用。
///
/// `Villager.releasePoi` when `mob.brain` is attached to `mob`.
pub fn release_poi(
    me: &mut Entity,
    mob: &super::Mob,
    ctx: &mut dyn crate::entity::ctx::EntityCtx,
    memory: MemoryType,
) {
    let Some(brain) = mob.brain.as_deref() else {
        return;
    };
    release_poi_from_memories(me, mob.villager.profession, &brain.memories, ctx, memory);
}

/// `Villager.releaseAllPois()`：依次归还 `HOME`、`JOB_SITE`、`POTENTIAL_JOB_SITE`、
/// `MEETING_POINT` 四个槽位的 POI 票。
///
/// `Villager.releaseAllPois()`: releases tickets for `HOME`, `JOB_SITE`,
/// `POTENTIAL_JOB_SITE`, and `MEETING_POINT`.
pub fn release_all_pois(
    me: &mut Entity,
    mob: &super::Mob,
    ctx: &mut dyn crate::entity::ctx::EntityCtx,
) {
    for memory in [
        MemoryType::Home,
        MemoryType::JobSite,
        MemoryType::PotentialJobSite,
        MemoryType::MeetingPoint,
    ] {
        release_poi(me, mob, ctx, memory);
    }
}

/// `AbstractBedBlock.getSleepHeight`：床的轮廓最高 Y（普通床为 `9/16 = 0.5625`）。
///
/// `AbstractBedBlock.getSleepHeight`: maximum Y of the bed outline (`0.5625` for beds).
fn bed_sleep_height(blocks: &dyn crate::entity::ctx::BlockView, pos: BlockPos) -> Option<f64> {
    let state = blocks.block_state(pos);
    if !state.is_tagged(mc_bamboo_data::tags::block_tags::BEDS) {
        return None;
    }
    let boxes = crate::shape::outline_boxes(state, pos);
    boxes
        .iter()
        .map(|b| b.max_y)
        .fold(None, |acc: Option<f64>, y| {
            Some(acc.map_or(y, |m| m.max(y)))
        })
}

/// `LivingEntity.checkBedExists()`。
///
/// `LivingEntity.checkBedExists()`.
pub fn check_bed_exists(mob: &super::Mob, blocks: &dyn crate::entity::ctx::BlockView) -> bool {
    mob.villager
        .sleeping_pos
        .is_some_and(|pos| bed_sleep_height(blocks, pos).is_some())
}

/// `LivingEntity.startSleeping(BlockPos)`：贴床定位、置 `OCCUPIED=true`、切 `SLEEPING` 姿势
/// 与 `0.2 x 0.2` 碰撞箱、清零速度并标同步。
///
/// `LivingEntity.startSleeping(BlockPos)`: snaps to the bed, sets `OCCUPIED=true`, switches
/// to sleeping pose and `0.2 x 0.2` dimensions, zeroes velocity, and marks sync.
pub fn start_sleeping(
    me: &mut Entity,
    mob: &mut super::Mob,
    ctx: &mut dyn crate::entity::ctx::EntityCtx,
    pos: BlockPos,
) -> bool {
    let Some(sleep_height) = bed_sleep_height(ctx.blocks(), pos) else {
        return false;
    };
    me.position = mc_bamboo_core::Vec3::new(
        pos.x as f64 + 0.5,
        pos.y as f64 + sleep_height + 0.125,
        pos.z as f64 + 0.5,
    );
    ctx.emit(
        crate::entity::ctx::Command::SetBedOccupied {
            pos,
            occupied: true,
        },
        me,
    );
    mob.villager.sleeping_pos = Some(pos);
    mob.villager.sleeping = true;
    me.dimensions = (0.2, 0.2);
    mob.eye_height = 0.2;
    me.delta = mc_bamboo_core::Vec3::default();
    mob.needs_sync = true;
    true
}

/// `Villager.stopSleeping()` / `LivingEntity.stopSleeping()`：清床 `OCCUPIED`、按
/// `AbstractBedBlock.findStandUpPosition` 找站起位置并面向床、恢复碰撞箱并写 `LAST_WOKEN`。
///
/// `Villager.stopSleeping()` / `LivingEntity.stopSleeping()`: clears bed `OCCUPIED`,
/// moves to `AbstractBedBlock.findStandUpPosition` facing the bed, restores dimensions,
/// and records `LAST_WOKEN`.
pub fn stop_sleeping(
    me: &mut Entity,
    mob: &mut super::Mob,
    memories: Option<&mut super::brain::memory::Memories>,
    ctx: &mut dyn crate::entity::ctx::EntityCtx,
) {
    let Some(pos) = mob.villager.sleeping_pos.take() else {
        mob.villager.sleeping = false;
        return;
    };
    mob.villager.sleeping = false;
    let (width, height, eye) = mob.mob_type.dimensions(mob.is_baby());
    me.dimensions = (width, height);
    mob.eye_height = eye;

    let state = ctx.blocks().block_state(pos);
    if state.is_tagged(mc_bamboo_data::tags::block_tags::BEDS) {
        let facing = state.get(crate::state::HORIZONTAL_FACING);
        ctx.emit(
            crate::entity::ctx::Command::SetBedOccupied {
                pos,
                occupied: false,
            },
            me,
        );
        let stand =
            find_stand_up_position(ctx.blocks(), pos, facing, me.y_rot).unwrap_or_else(|| {
                mc_bamboo_core::Vec3::new(
                    pos.x as f64 + 0.5,
                    (pos.y + 1) as f64 + 0.1,
                    pos.z as f64 + 0.5,
                )
            });
        let bed_bottom =
            mc_bamboo_core::Vec3::new(pos.x as f64 + 0.5, pos.y as f64, pos.z as f64 + 0.5);
        let dir = bed_bottom.subtract(stand).normalize();
        // `57.2957763671875` 是字节码常量 `(double)(180.0F / (float)Math.PI)`。
        let mut yaw = mc_bamboo_core::mth::atan2(dir.z, dir.x) * 57.295_776_367_187_5 - 90.0;
        yaw %= 360.0;
        if yaw >= 180.0 {
            yaw -= 360.0;
        }
        if yaw < -180.0 {
            yaw += 360.0;
        }
        me.position = stand;
        me.y_rot = yaw as f32;
        mob.x_rot = 0.0;
    }

    let now = ctx.blocks().game_time();
    if let Some(mem) = memories {
        mem.set(
            MemoryType::LastWoken,
            super::brain::memory::MemoryValue::Long(now),
        );
    } else if let Some(brain) = mob.brain.as_mut() {
        brain.memories.set(
            MemoryType::LastWoken,
            super::brain::memory::MemoryValue::Long(now),
        );
    }
}

/// `AbstractBedBlock.findStandUpPosition`（村民默认尺寸 `0.6 x 1.95`）。
fn find_stand_up_position(
    blocks: &dyn crate::entity::ctx::BlockView,
    pos: BlockPos,
    bed_facing: mc_bamboo_core::Direction,
    y_rot: f32,
) -> Option<mc_bamboo_core::Vec3> {
    let cw = bed_facing.clockwise();
    let preferred = if direction_is_facing_angle(cw, y_rot) {
        cw.opposite()
    } else {
        cw
    };
    let is_bunk = blocks
        .block_state(pos.below())
        .is_tagged(mc_bamboo_data::tags::block_tags::BEDS);
    if is_bunk {
        let surround = bed_surround_offsets(bed_facing, preferred);
        let above = bed_above_offsets(bed_facing);
        for check_dangerous in [true, false] {
            if let Some(v) = find_stand_up_at_offsets(blocks, pos, &surround, check_dangerous) {
                return Some(v);
            }
            if let Some(v) =
                find_stand_up_at_offsets(blocks, pos.below(), &surround, check_dangerous)
            {
                return Some(v);
            }
            if let Some(v) = find_stand_up_at_offsets(blocks, pos, &above, check_dangerous) {
                return Some(v);
            }
        }
        return None;
    }
    let surround = bed_surround_offsets(bed_facing, preferred);
    let above = bed_above_offsets(bed_facing);
    for check_dangerous in [true, false] {
        if let Some(v) = find_stand_up_at_offsets(blocks, pos, &surround, check_dangerous) {
            return Some(v);
        }
        if let Some(v) = find_stand_up_at_offsets(blocks, pos, &above, check_dangerous) {
            return Some(v);
        }
    }
    None
}

fn dir_step_xz(dir: mc_bamboo_core::Direction) -> (i32, i32) {
    match dir {
        mc_bamboo_core::Direction::North => (0, -1),
        mc_bamboo_core::Direction::South => (0, 1),
        mc_bamboo_core::Direction::West => (-1, 0),
        mc_bamboo_core::Direction::East => (1, 0),
        _ => (0, 0),
    }
}

fn direction_is_facing_angle(dir: mc_bamboo_core::Direction, degrees: f32) -> bool {
    let radians = f64::from(degrees * (std::f32::consts::PI / 180.0));
    let sin = -mc_bamboo_core::mth::sin(radians);
    let cos = mc_bamboo_core::mth::cos(radians);
    let (sx, sz) = dir_step_xz(dir);
    (sx as f32) * sin + (sz as f32) * cos > 0.0
}

fn bed_surround_offsets(
    facing: mc_bamboo_core::Direction,
    side: mc_bamboo_core::Direction,
) -> [(i32, i32); 10] {
    let (fx, fz) = dir_step_xz(facing);
    let (sx, sz) = dir_step_xz(side);
    [
        (sx, sz),
        (sx - fx, sz - fz),
        (sx - 2 * fx, sz - 2 * fz),
        (-2 * fx, -2 * fz),
        (-sx - 2 * fx, -sz - 2 * fz),
        (-sx - fx, -sz - fz),
        (-sx, -sz),
        (-sx + fx, -sz + fz),
        (fx, fz),
        (sx + fx, sz + fz),
    ]
}

fn bed_above_offsets(facing: mc_bamboo_core::Direction) -> [(i32, i32); 2] {
    let (fx, fz) = dir_step_xz(facing);
    [(0, 0), (-fx, -fz)]
}

fn find_stand_up_at_offsets(
    blocks: &dyn crate::entity::ctx::BlockView,
    origin: BlockPos,
    offsets: &[(i32, i32)],
    check_dangerous: bool,
) -> Option<mc_bamboo_core::Vec3> {
    for &(dx, dz) in offsets {
        let pos = BlockPos::new(origin.x + dx, origin.y, origin.z + dz);
        if let Some(v) = find_safe_dismount_location(blocks, pos, check_dangerous) {
            return Some(v);
        }
    }
    None
}

/// `DismountHelper.findSafeDismountLocation`（对应村民成人默认碰撞箱 `0.6 x 1.95`）。
fn find_safe_dismount_location(
    blocks: &dyn crate::entity::ctx::BlockView,
    pos: BlockPos,
    check_dangerous: bool,
) -> Option<mc_bamboo_core::Vec3> {
    if check_dangerous && is_block_dangerous(blocks.block_state(pos)) {
        return None;
    }
    let floor = block_floor_height(blocks, pos);
    if floor.is_infinite() || floor >= 1.0 {
        return None;
    }
    if check_dangerous && floor <= 0.0 && is_block_dangerous(blocks.block_state(pos.below())) {
        return None;
    }
    let candidate =
        mc_bamboo_core::Vec3::new(pos.x as f64 + 0.5, pos.y as f64 + floor, pos.z as f64 + 0.5);
    let aabb = mc_bamboo_core::Aabb::new(
        candidate.x - 0.3,
        candidate.y,
        candidate.z - 0.3,
        candidate.x + 0.3,
        candidate.y + 1.95,
        candidate.z + 0.3,
    );
    if has_block_collision(blocks, aabb) {
        return None;
    }
    Some(candidate)
}

fn is_block_dangerous(state: crate::state::BlockState) -> bool {
    use mc_bamboo_data::{blocks, tags::block_tags};
    state.is_tagged(block_tags::FIRE)
        || state.is(blocks::MAGMA_BLOCK)
        || state.is(blocks::CAMPFIRE)
        || state.is(blocks::SOUL_CAMPFIRE)
        || state.is(blocks::LAVA)
        || state.is(blocks::SWEET_BERRY_BUSH)
        || state.is(blocks::CACTUS)
        || state.is(blocks::WITHER_ROSE)
        || state.is(blocks::POWDER_SNOW)
}

fn non_climbable_max_y(blocks: &dyn crate::entity::ctx::BlockView, pos: BlockPos) -> Option<f64> {
    let state = blocks.block_state(pos);
    if state.is_tagged(mc_bamboo_data::tags::block_tags::CLIMBABLE) {
        return None;
    }
    if state.is_tagged(mc_bamboo_data::tags::block_tags::TRAPDOORS) && state.get(crate::state::OPEN)
    {
        return None;
    }
    let boxes = crate::shape::collision_boxes(state, pos, crate::shape::CollisionContext::Empty);
    boxes
        .iter()
        .map(|b| b.max_y)
        .fold(None, |acc: Option<f64>, y| {
            Some(acc.map_or(y, |m| m.max(y)))
        })
}

fn block_floor_height(blocks: &dyn crate::entity::ctx::BlockView, pos: BlockPos) -> f64 {
    if let Some(y) = non_climbable_max_y(blocks, pos) {
        return y;
    }
    if let Some(y) = non_climbable_max_y(blocks, pos.below()) {
        return y - 1.0;
    }
    f64::NEG_INFINITY
}

fn has_block_collision(
    blocks: &dyn crate::entity::ctx::BlockView,
    aabb: mc_bamboo_core::Aabb,
) -> bool {
    let min_x = mc_bamboo_core::mth::floor(aabb.min_x - 1.0e-7) - 1;
    let max_x = mc_bamboo_core::mth::floor(aabb.max_x + 1.0e-7) + 1;
    let min_y = mc_bamboo_core::mth::floor(aabb.min_y - 1.0e-7) - 1;
    let max_y = mc_bamboo_core::mth::floor(aabb.max_y + 1.0e-7) + 1;
    let min_z = mc_bamboo_core::mth::floor(aabb.min_z - 1.0e-7) - 1;
    let max_z = mc_bamboo_core::mth::floor(aabb.max_z + 1.0e-7) + 1;
    let ctx = crate::shape::CollisionContext::Empty;
    for x in min_x..=max_x {
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                let pos = BlockPos::new(x, y, z);
                let state = blocks.block_state(pos);
                if state.is_air() {
                    continue;
                }
                for b in crate::shape::collision_boxes(state, pos, ctx).iter() {
                    let world_box = b.moved(x as f64, y as f64, z as f64);
                    if world_box.intersects(aabb) {
                        return true;
                    }
                }
            }
        }
    }
    false
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
    use mc_bamboo_protocol::merchant::ItemCost;

    fn sample_offer(uses: i32) -> MerchantOffer {
        MerchantOffer {
            base_cost_a: ItemCost::new(mc_bamboo_data::items::EMERALD, 1),
            cost_b: None,
            result: ItemStack::new(mc_bamboo_data::items::APPLE, 1),
            uses,
            max_uses: 12,
            reward_exp: true,
            special_price_diff: 0,
            demand: 0,
            price_multiplier: 0.0,
            xp: 1,
        }
    }

    /// `MerchantOffer.updateDemand` 的公式来自字节码：`demand += 2*uses - maxUses`。
    #[test]
    fn update_offer_demand_follows_the_vanilla_formula() {
        let mut offer = sample_offer(3);
        update_offer_demand(&mut offer);
        assert_eq!(offer.demand, 2 * 3 - 12);
    }

    /// 补货跨天重置计数，且只有「今天还能补」和「有使用过」同时成立才补。
    #[test]
    fn restock_only_fires_when_needed_and_allowed() {
        let mut v = Villager {
            offers: Some(vec![sample_offer(1)]),
            ..Villager::default()
        };
        // 第一次：需要补（uses>0）且允许（restocks_today==0）。
        assert!(v.should_restock(1000, 0));
        v.restock(1000);
        assert_eq!(v.restocks_today, 1);
        assert_eq!(v.offers.as_ref().unwrap()[0].uses, 0);
        // 刚补完还没冷却，不再补。
        assert!(!v.should_restock(2000, 0));
        // 跨天重置当天计数：先让检查日变成正数，再跨到第二天。
        v.last_restock_check_day = 1;
        assert!(!v.should_restock(3000, 2));
        assert_eq!(v.restocks_today, 0);
    }

    /// `Villager.restock()` 先把全部使用次数折进需求，再清零使用次数，并记补货时刻。
    #[test]
    fn restock_folds_uses_into_demand_then_clears() {
        let mut v = Villager {
            offers: Some(vec![sample_offer(3)]),
            ..Villager::default()
        };
        v.offers.as_mut().unwrap()[0].demand = 5;
        v.restock(777);
        let offer = &v.offers.as_ref().unwrap()[0];
        assert_eq!(offer.demand, 5 + 2 * 3 - 12);
        assert_eq!(offer.uses, 0);
        assert_eq!(v.last_restock, 777);
        assert_eq!(v.restocks_today, 1);
    }

    /// `notify_trade` 升级：xp 累加、等级 +1、追加新等级交易、经验球 +5、挂 200 刻再生。
    #[test]
    fn notify_trade_levels_up_and_adds_regeneration() {
        let mut mob = super::super::Mob::new(MobType::Villager);
        mob.villager.offers = Some(vec![sample_offer(0)]);
        mob.villager.level = 1;
        mob.villager.xp = 9; // 9 + offer.xp(2) = 11 >= 10 → 升级
        let outcome = notify_trade(&mut mob, 2, 3, || vec![sample_offer(0), sample_offer(0)]);
        assert!(outcome.leveled_up);
        assert_eq!(outcome.orb_amount, 3 + 5);
        assert_eq!(mob.villager.level, 2);
        assert_eq!(mob.villager.xp, 11);
        assert_eq!(mob.villager.offers.as_ref().unwrap().len(), 3);
        let regen = mob
            .effects
            .get(mc_bamboo_data::registries::mob_effect::REGENERATION)
            .expect("升级后应有再生效果");
        assert_eq!(regen.duration, 200);
        assert_eq!(regen.amplifier, 0);
    }

    /// 没到阈值时 `notify_trade` 只加 xp 和经验球数量，不升级、不追加交易、不挂效果。
    #[test]
    fn notify_trade_without_level_up_leaves_offers_alone() {
        let mut mob = super::super::Mob::new(MobType::Villager);
        mob.villager.offers = Some(vec![sample_offer(0)]);
        mob.villager.level = 1;
        mob.villager.xp = 0;
        let outcome = notify_trade(&mut mob, 2, 4, || vec![sample_offer(0)]);
        assert!(!outcome.leveled_up);
        assert_eq!(outcome.orb_amount, 4);
        assert_eq!(mob.villager.level, 1);
        assert_eq!(mob.villager.xp, 2);
        assert_eq!(mob.villager.offers.as_ref().unwrap().len(), 1);
        assert!(
            mob.effects
                .get(mc_bamboo_data::registries::mob_effect::REGENERATION)
                .is_none()
        );
    }

    /// 交易状态方法：`setTradingPlayer` / `isTrading` / `stopTrading` / `stillValid`。
    #[test]
    fn trading_player_state_matches_vanilla() {
        let mut v = Villager::default();
        assert!(!v.is_trading());
        assert_eq!(v.trading_player(), None);
        v.set_trading_player(Some(42));
        assert!(v.is_trading());
        assert_eq!(v.trading_player(), Some(42));
        assert!(v.still_valid_trading(42));
        assert!(!v.still_valid_trading(7));
        v.stop_trading();
        assert!(!v.is_trading());
        assert_eq!(v.trading_player(), None);
    }

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
            villager.offers = Some(Vec::new());
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

    /// 村民入睡置床 `OCCUPIED`、缩碰撞箱与 `sleeping_pos` 存档往返，醒来清占用并写 `LAST_WOKEN`。
    #[test]
    fn villager_sleep_and_wake_updates_bed_dimensions_and_nbt() {
        use crate::level::LevelEntities;
        use crate::state::{BED_PART, BedPart, BlockState, HORIZONTAL_FACING, OCCUPIED};
        use mc_bamboo_core::Direction;
        use mc_bamboo_data::blocks;

        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        ticks.set_time(200);
        let mut world = World::new(&mut level, &mut ticks);
        for x in -4..=4 {
            for z in -4..=4 {
                world.set_block(BlockPos::new(x, 63, z), BlockState(blocks::STONE), 2);
            }
        }
        let head = BlockPos::new(1, 64, 0);
        let foot = BlockPos::new(1, 64, 1);
        world.set_block(
            head,
            BlockState(blocks::RED_BED)
                .with(HORIZONTAL_FACING, Direction::North)
                .with(BED_PART, BedPart::Head)
                .with(OCCUPIED, false),
            2,
        );
        world.set_block(
            foot,
            BlockState(blocks::RED_BED)
                .with(HORIZONTAL_FACING, Direction::North)
                .with(BED_PART, BedPart::Foot)
                .with(OCCUPIED, false),
            2,
        );
        let uid = spawn::summon_mob(
            &mut world,
            MobType::Villager,
            Vec3::new(0.5, 64.0, 0.5),
            None,
        )
        .unwrap();

        drop(world);
        let mut me = level.take_entity(uid).unwrap();
        let mut world = World::new(&mut level, &mut ticks);
        let mut ctx = crate::entity::ctx::PreciseCtx::new(&mut world, uid);
        let slept = me
            .with_mob(|me, mob| start_sleeping(me, mob, &mut ctx, head))
            .unwrap();
        assert!(slept);
        assert_eq!(me.dimensions, (0.2, 0.2));
        assert!((me.position.y - 64.6875).abs() < 1.0e-6);
        let mob = me.kind.mob().unwrap();
        assert!(mob.villager.sleeping);
        assert_eq!(mob.villager.sleeping_pos, Some(head));
        assert!(world.block_state(head).get(OCCUPIED));

        // 存档往返保留 `sleeping_pos` 与睡眠尺寸。
        let saved = nbt::write_mob(&me.clone().split()).unwrap();
        let reloaded = nbt::read_mob(&saved, "minecraft:villager", 7)
            .unwrap()
            .unwrap();
        let reloaded_mob = match &reloaded.state.kind {
            crate::entity::KindState::Mob(m) => m,
            _ => panic!("expected mob"),
        };
        assert!(reloaded_mob.villager.sleeping);
        assert_eq!(reloaded_mob.villager.sleeping_pos, Some(head));
        assert_eq!(reloaded_mob.dimensions(), (0.2, 0.2));

        // 起床恢复尺寸、清床占用并记录 `LAST_WOKEN`。
        let mut ctx = crate::entity::ctx::PreciseCtx::new(&mut world, uid);
        me.with_mob(|me, mob| stop_sleeping(me, mob, None, &mut ctx));
        assert_eq!(me.dimensions, (0.6, 1.95));
        let mob = me.kind.mob().unwrap();
        assert!(!mob.villager.sleeping);
        assert_eq!(mob.villager.sleeping_pos, None);
        assert_eq!(
            mob.brain
                .as_deref()
                .and_then(|b| b.memories.long(MemoryType::LastWoken)),
            Some(200)
        );
        assert!(!world.block_state(head).get(OCCUPIED));
    }

    /// 敲钟通知 32 格内活着的村民写 `HEARD_BELL_TIME`，32 格外不通知。
    #[test]
    fn bell_ring_notifies_villagers_within_32_blocks() {
        let mut level = TestLevel::new();
        let mut ticks = BlockTicks::default();
        ticks.set_time(555);
        let mut world = World::new(&mut level, &mut ticks);
        let near = spawn::summon_mob(
            &mut world,
            MobType::Villager,
            Vec3::new(10.5, 64.0, 0.5),
            None,
        )
        .unwrap();
        let far = spawn::summon_mob(
            &mut world,
            MobType::Villager,
            Vec3::new(40.5, 64.0, 0.5),
            None,
        )
        .unwrap();
        crate::behaviour::attached::bell::ring(&mut world, BlockPos::new(0, 64, 0));
        let near_heard = world.entity(near).and_then(|e| {
            e.kind
                .mob()
                .and_then(|m| m.brain.as_deref())
                .and_then(|b| b.memories.long(MemoryType::HeardBellTime))
        });
        let far_heard = world.entity(far).and_then(|e| {
            e.kind
                .mob()
                .and_then(|m| m.brain.as_deref())
                .and_then(|b| b.memories.long(MemoryType::HeardBellTime))
        });
        assert_eq!(near_heard, Some(555));
        assert_eq!(far_heard, None);
    }

    /// 村民死亡和僵尸转化都会归还持有的 POI 票数。
    #[test]
    fn villager_death_and_conversion_release_all_poi_tickets() {
        use crate::entity::mob::{DamageKind, DamageSource, brain::memory::MemoryValue, living};
        use crate::level::LevelEntities;
        use crate::state::BlockState;
        use mc_bamboo_data::blocks;
        use mc_bamboo_protocol::components::value::GlobalPos;

        for convert in [false, true] {
            let mut level = TestLevel::new();
            let composter = BlockPos::new(3, 64, 0);
            let farmer_poi = mc_bamboo_data::poi::poi_type_id("farmer").unwrap();

            let mut ticks = BlockTicks::default();
            let mut world = World::new(&mut level, &mut ticks);
            world.set_block(composter, BlockState(blocks::COMPOSTER), 2);
            assert!(world.poi_take(composter, 1_u32 << farmer_poi));
            let uid = spawn::summon_mob(
                &mut world,
                MobType::Villager,
                Vec3::new(0.5, 64.0, 0.5),
                None,
            )
            .unwrap();
            let farmer_prof = facts::profession_id("minecraft:farmer").unwrap();
            world.with_entity_mut(uid, &mut |entity| {
                entity.with_mob(|_, mob| {
                    mob.villager.set_profession(farmer_prof);
                    if let Some(brain) = mob.brain.as_mut() {
                        brain.memories.set(
                            MemoryType::JobSite,
                            MemoryValue::GlobalPos(GlobalPos {
                                dimension: "minecraft:overworld".to_owned(),
                                pos: composter,
                            }),
                        );
                    }
                });
            });

            let source = if convert {
                DamageSource::environment(DamageKind::MobAttack)
                    .with_zombie_attacker()
                    .with_villager_conversion_roll(false)
            } else {
                DamageSource::environment(DamageKind::GenericKill)
            };
            drop(world);
            let mut me = level.take_entity(uid).unwrap();
            let mut world = World::new(&mut level, &mut ticks);
            let mut ctx = crate::entity::ctx::PreciseCtx::new(&mut world, uid);
            me.with_mob(|me, mob| {
                living::hurt_server(me, mob, &mut ctx, source, 100.0);
            });
            drop(world);
            assert_eq!(
                level.poi.free_tickets(composter),
                1,
                "convert={convert} should release job site ticket"
            );
        }
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
