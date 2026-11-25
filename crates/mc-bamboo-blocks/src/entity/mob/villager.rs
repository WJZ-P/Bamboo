//! 村民的同步数据和存档：`VillagerData`、`VillagerDataFinalized`、`DataUnhappyCounter`、
//! 食物 / 经验 / 补货时刻和八卦。
//!
//! 期 2 只有骨架：交易（`Offers`）和背包（`Inventory`）原样保留不解释，八卦也只保留
//! 原文；求值在期 5 / 期 6。
//!
//! Villager synced data and saving: `VillagerData`, `VillagerDataFinalized`, the
//! unhappy counter, food / experience / restock times and gossips. Phase 2 keeps
//! `Offers` and `Inventory` verbatim; trading and gossip semantics come later.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::villager as facts;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::components::value::GlobalPos;
use mc_bamboo_protocol::merchant::{self, MerchantOffer};

use super::{
    brain::{Brain, memory::MemoryType},
    navigation::Navigation,
    spawn::{self, SpawnReason},
};
use crate::{
    archive::tag::{self, put},
    entity::Entity,
    world::World,
};

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
    /// `Inventory` 原文。拾取物品在期 3。
    pub inventory: Option<Tag>,
    /// `LivingEntity.isSleeping()`：睡在床上时为真（睡觉行为第 4 期接入）。
    ///
    /// `LivingEntity.isSleeping()`: true while resting in a bed (sleep behaviour lands in phase 4).
    pub sleeping: bool,
    /// 当前是否正在与玩家交易（交易系统第 5 期接入；村民降频豁免看它）。
    ///
    /// True while trading with a player (trading lands in phase 5; villager throttling
    /// exempts trading villagers).
    pub trading: bool,
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
            inventory: None,
            sleeping: false,
            trading: false,
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

    /// `Villager.addAdditionalSaveData` 的 8 个键，加上 `AbstractVillager` 的 `Offers` /
    /// `Inventory`。官方只在 `offers != null` 时写 `Offers`。
    ///
    /// The eight `Villager.addAdditionalSaveData` keys plus `AbstractVillager`'s
    /// `Offers` / `Inventory`. Vanilla writes `Offers` only when it is non-null.
    pub fn write_nbt(&self, out: &mut Vec<(String, Tag)>) {
        if let Some(offers) = &self.offers
            && let Ok(encoded) = merchant::encode_offers(offers)
        {
            put(out, "Offers", encoded.to_tag());
        }
        if let Some(inventory) = &self.inventory {
            put(out, "Inventory", inventory.clone());
        }
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
    /// finalized 置真。`Offers` / `Inventory` 原样保留。
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
        self.offers =
            tag::field(tag, "Offers").and_then(|offers| merchant::decode_offers(offers).ok());
        self.inventory = tag::field(tag, "Inventory").cloned();
    }
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
            villager.offers = Some(Vec::new());
            villager.inventory = Some(Tag::List(Vec::new()));
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
            assert!(mob.villager.inventory.is_some());
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
