//! 村民的场景：生成时的群系品种、存档往返和死亡掉落。
//!
//! 期 2 的村民没有大脑，生成场景只比较 `VillagerData` / finalized / 尺寸和血量；
//! 构造期的传感器延迟取数不在这一期，所以不比随机源。
//!
//! Villager scenes: the biome type at spawn, save round trips and death drops. Phase 2
//! has no villager brain, so the spawn scene compares `VillagerData` / finalized /
//! dimensions / health and not the random source.

use super::*;
use mc_bamboo_blocks::entity::mob::MobType;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VillagerSpawnCase {
    biome: String,
    kind: String,
    profession: String,
    level: i32,
    finalized: bool,
    dimensions: [f32; 2],
    health: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VillagerSaveCase {
    name: String,
    nbt: String,
    reloaded: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VillagerDeathCase {
    by_player: bool,
    removed: bool,
    items: Vec<String>,
    experience: i32,
}

/// 村民自己的键，加上原样保留的 `Offers` / `Inventory` 和通用的 `Brain`。
const SAVE_KEYS: &[&str] = &[
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
    "Brain",
];

impl Report {
    pub(super) fn replay_villager_spawn(&mut self, case: &VillagerSpawnCase) {
        self.cases += 1;
        let Some(biome) = mc_bamboo_data::registries::biome::by_name(&case.biome) else {
            self.mismatch(format!("{}: 官方群系不认识", case.biome));
            return;
        };
        let level = MobLevel {
            blocks: std::collections::HashMap::new(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            biome: Some(biome),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: 2,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(1 << 24),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
        };
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let mut level = level;
        let mut world = World::new(&mut level, &mut ticks);
        let uid = match mc_bamboo_blocks::entity::mob::spawn::summon_mob(
            &mut world,
            MobType::Villager,
            Vec3::new(0.5, 64.0, 0.5),
            None,
        ) {
            Ok(uid) => uid,
            Err(error) => {
                self.mismatch(format!("{}: summon failed: {error:?}", case.biome));
                return;
            }
        };
        if self.break_test.as_deref() == Some("villager_spawn") {
            // 改坏确认：把品种固定成平原，群系场景应当立刻报不一致。
            world.with_entity_mut(uid, &mut |entity| {
                entity.with_mob(|_, mob| {
                    mob.villager.kind =
                        mc_bamboo_data::villager::type_id("minecraft:plains").unwrap_or(0);
                });
            });
        }
        let Some(entity) = world.entity(uid) else {
            self.mismatch(format!("{}: summon vanished", case.biome));
            return;
        };
        let Some(mob) = entity.kind.mob() else {
            self.mismatch(format!("{}: summon not a mob", case.biome));
            return;
        };
        let kind = mc_bamboo_data::villager::type_name(mob.villager.kind).unwrap_or("");
        let profession = mc_bamboo_data::villager::profession(mob.villager.profession)
            .map_or("", |entry| entry.name);
        let dimensions = mob.dimensions();
        let actual = (
            kind,
            profession,
            mob.villager.level,
            mob.villager.finalized,
            [dimensions.0, dimensions.1],
            mob.health,
        );
        let expected = (
            case.kind.as_str(),
            case.profession.as_str(),
            case.level,
            case.finalized,
            case.dimensions,
            case.health,
        );
        if actual != expected {
            self.mismatch(format!(
                "villager spawn {}: official {expected:?}, bamboo {actual:?}",
                case.biome
            ));
        }
    }

    pub(super) fn replay_villager_save(&mut self, case: &VillagerSaveCase) {
        use mc_bamboo_blocks::entity::mob::nbt;
        self.cases += 1;
        let official = match mc_bamboo_nbt::parse_snbt(&case.nbt) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{}: SNBT does not parse: {error:?}", case.name));
                return;
            }
        };
        let mut entity = Entity::by_mob_type(MobType::Villager, Vec3::default());
        if let Err(error) = nbt::load_into(&mut entity, &official) {
            self.mismatch(format!("{}: load failed: {error:?}", case.name));
            return;
        }
        if self.break_test.as_deref() == Some("villager_save") {
            // 改坏确认：等级加一，存档对拍应当报 `VillagerData` 不一致。
            entity.with_mob(|_, mob| mob.villager.level += 1);
        }
        let ours = match nbt::write_mob(&entity.split()) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!("{}: save failed: {error:?}", case.name));
                return;
            }
        };
        let reloaded = match mc_bamboo_nbt::parse_snbt(&case.reloaded) {
            Ok(tag) => tag,
            Err(error) => {
                self.mismatch(format!(
                    "{}: reloaded SNBT does not parse: {error:?}",
                    case.name
                ));
                return;
            }
        };
        for key in SAVE_KEYS {
            let expected = field(&reloaded, key).map(canonical);
            let actual = field(&ours, key).map(canonical);
            if expected != actual {
                self.mismatch(format!(
                    "{} {key}: official {expected:?}, bamboo {actual:?}",
                    case.name
                ));
            }
        }
    }

    pub(super) fn replay_villager_death(&mut self, case: &VillagerDeathCase) {
        self.cases += 1;
        let mut level = MobLevel {
            blocks: std::collections::HashMap::new(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            biome: None,
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: 2,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(1 << 24),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
        };
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        let mut source =
            DamageSource::environment(mc_bamboo_blocks::entity::mob::DamageKind::Generic);
        if case.by_player {
            source =
                DamageSource::environment(mc_bamboo_blocks::entity::mob::DamageKind::PlayerAttack);
            source.attacker_is_player = true;
        }
        let id;
        {
            let mut world = World::new(&mut level, &mut ticks);
            id = world.spawn_entity(Entity::by_mob_type(
                MobType::Villager,
                Vec3::new(0.5, 64.0, 0.5),
            ));
            world.hurt_mob(id, source, 100.0);
        }
        for tick in 0..25 {
            ticks.set_time(1001 + tick);
            let mut world = World::new(&mut level, &mut ticks);
            world.tick_entities();
        }
        let removed = level.entities.public_at(id).is_none();
        if removed != case.removed {
            self.mismatch(format!(
                "villager death by_player {}: removed official {}, bamboo {removed}",
                case.by_player, case.removed
            ));
        }
        let mut items: Vec<String> = level
            .entities
            .iter()
            .filter_map(|entity| {
                entity
                    .ground()
                    .map(|ground| format!("minecraft:itemx{}", ground.stack.count))
            })
            .collect();
        items.sort();
        if items != case.items {
            self.mismatch(format!(
                "villager death by_player {}: items official {:?}, bamboo {items:?}",
                case.by_player, case.items
            ));
        }
        let experience: i32 = level
            .entities
            .iter()
            .filter_map(|entity| match &entity.kind {
                mc_bamboo_blocks::entity::EntityKind::ExperienceOrb(orb) => Some(orb.value),
                _ => None,
            })
            .sum();
        if experience != case.experience {
            self.mismatch(format!(
                "villager death by_player {}: experience official {}, bamboo {experience}",
                case.by_player, case.experience
            ));
        }
    }
}

/// 复合标签的键按名字排序，便于和官方的哈希表顺序比较。
fn canonical(tag: &mc_bamboo_nbt::Tag) -> mc_bamboo_nbt::Tag {
    use mc_bamboo_nbt::Tag;
    match tag {
        Tag::Compound(entries) => {
            let mut sorted: Vec<_> = entries
                .iter()
                .map(|(name, value)| (name.clone(), canonical(value)))
                .collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            Tag::Compound(sorted)
        }
        Tag::List(items) => Tag::List(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

fn field<'a>(tag: &'a mc_bamboo_nbt::Tag, key: &str) -> Option<&'a mc_bamboo_nbt::Tag> {
    match tag {
        mc_bamboo_nbt::Tag::Compound(entries) => entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value),
        _ => None,
    }
}
