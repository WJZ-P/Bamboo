use mc_bamboo_blocks::entity::mob::MobType;
use mc_bamboo_data::registries::{attribute, data_component_type};
use mc_bamboo_loot::{LootStack, mob_spawn_enchantments};
use mc_bamboo_protocol::{
    components::{ComponentValue, PatchEntry},
    item::ItemStack,
    packet::play::EquipmentSlot,
};

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SkeletonSpawnCase {
    seed: i64,
    egg: bool,
    difficulty: i32,
    clock: i64,
    inhabited_time: i64,
    halloween: bool,
    head_drop_bits: i32,
    left_handed: bool,
    can_pick_up_loot: bool,
    follow_bonus: i64,
    yaw: Option<i32>,
    equipment: Vec<Equipment>,
    level_seed: i64,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct Equipment {
    item: i32,
    enchantments: Vec<(i32, i32)>,
}

#[derive(Default)]
pub(super) struct Coverage {
    summons: usize,
    eggs: usize,
    armored: usize,
    enchanted: usize,
    pumpkins: usize,
    lanterns: usize,
    helmets: usize,
}

impl Coverage {
    fn record(&mut self, case: &SkeletonSpawnCase) {
        if case.egg {
            self.eggs += 1;
        } else {
            self.summons += 1;
        }
        use mc_bamboo_data::items;
        let is_armor = |item| {
            !matches!(
                item,
                items::AIR | items::CARVED_PUMPKIN | items::JACK_O_LANTERN
            )
        };
        self.armored += usize::from(
            case.equipment
                .iter()
                .skip(1)
                .any(|stack| is_armor(stack.item)),
        );
        self.enchanted += usize::from(
            case.equipment
                .iter()
                .any(|stack| !stack.enchantments.is_empty()),
        );
        if let Some(head) = case.equipment.last() {
            self.pumpkins += usize::from(head.item == items::CARVED_PUMPKIN);
            self.lanterns += usize::from(head.item == items::JACK_O_LANTERN);
            self.helmets += usize::from(is_armor(head.item));
        }
    }

    pub(super) fn print(&self) {
        if self.eggs + self.summons != 0 {
            println!(
                "skeleton_spawn: {} summons, {} eggs, {} armored, {} enchanted",
                self.summons, self.eggs, self.armored, self.enchanted
            );
            if self.pumpkins + self.lanterns != 0 {
                println!(
                    "skeleton_halloween: {} pumpkins, {} lanterns, {} ordinary helmets",
                    self.pumpkins, self.lanterns, self.helmets
                );
            }
        }
    }
}

pub(super) fn enchant_equipment(
    stack: &mut ItemStack,
    special: f32,
    random: &dyn mc_bamboo_blocks::random::RandomSource,
) {
    let levels = mob_spawn_enchantments(&LootStack::new(stack.item, stack.count), special, random);
    if !levels.is_empty() {
        let mut patch = stack.patch();
        patch.put(
            data_component_type::ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(levels)),
        );
        (stack.added, stack.removed) = patch.to_opaque();
    }
}

/// 陷阱骑士对拍用的装备字符串：`item@id:{name@level,...}`，和 Java 夹具同格式。
pub(super) fn equipment_string(stack: &ItemStack) -> String {
    let mut levels: Vec<String> = match stack.patch().get(data_component_type::ENCHANTMENTS) {
        Some(ComponentValue::Enchantments(levels)) => levels
            .iter()
            .map(|&(id, level)| {
                format!(
                    "{}@{level}",
                    mc_bamboo_loot::enchantment_name(id).unwrap_or("?")
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    levels.sort();
    let item = if stack.is_empty() { 0 } else { stack.item };
    format!("{}:{{{}}}", item, levels.join(","))
}

fn equipment(stack: &ItemStack) -> Equipment {
    let mut enchantments = match stack.patch().get(data_component_type::ENCHANTMENTS) {
        Some(ComponentValue::Enchantments(levels)) => levels.clone(),
        _ => Vec::new(),
    };
    enchantments.sort_unstable();
    Equipment {
        item: if stack.is_empty() { 0 } else { stack.item },
        enchantments,
    }
}

impl Report {
    pub(super) fn replay_skeleton_spawn(&mut self, case: &SkeletonSpawnCase) {
        self.cases += 1;
        self.skeleton_spawns.record(case);
        let mut level = MobLevel {
            natural_spawn: None,
            biome: None,
            blocks: HashMap::new(),
            random: LevelRandom::new(case.seed),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                halloween: case.halloween
                    && self.break_test.as_deref() != Some("skeleton_halloween"),
                clock: case.clock,
                inhabited_time: case.inhabited_time,
                difficulty: case.difficulty,
                ..ZombieFacts::default()
            },
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            game_event_listeners: Default::default(),
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(1),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
        };
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        if self.break_test.as_deref() == Some("skeleton_spawn") {
            world.random().next_float();
        }
        let pos = BlockPos::new(0, 64, 0);
        let id = if case.egg {
            spawn::spawn_egg(&mut world, MobType::Skeleton, pos, true, false, None)
        } else {
            spawn::summon_mob(
                &mut world,
                MobType::Skeleton,
                Vec3::new(0.5, 64.0, 0.5),
                None,
            )
            .unwrap()
        };
        let entity = world.entity(id).unwrap();
        // 裸召唤的初始朝向不取世界随机源，测试不对齐私有构造种子。
        // The initial yaw of a bare summon is independent of the world RNG;
        // the fixtures do not align the private constructor seeds.
        let yaw = case.egg.then(|| entity.y_rot.to_bits() as i32);
        let mob = entity.kind.mob().unwrap();
        let mut attributes = mob.attributes.clone();
        let bonus = attributes
            .instance(attribute::FOLLOW_RANGE)
            .and_then(|range| range.modifier("minecraft:random_spawn_bonus"))
            .map(|modifier| modifier.amount.to_bits() as i64);
        let actual = (
            mob.left_handed,
            mob.can_pick_up_loot,
            bonus,
            yaw,
            world.random().seed(),
        );
        let expected = (
            case.left_handed,
            case.can_pick_up_loot,
            Some(case.follow_bonus),
            case.yaw,
            case.level_seed,
        );
        let label = format!("skeleton spawn seed {} egg {}", case.seed, case.egg);
        if actual != expected {
            self.mismatch(format!(
                "{label} (left-handed, loot, follow bonus, yaw, world RNG): official {expected:?}, bamboo {actual:?}"
            ));
        }
        let actual = [
            EquipmentSlot::MainHand,
            EquipmentSlot::Feet,
            EquipmentSlot::Legs,
            EquipmentSlot::Chest,
            EquipmentSlot::Head,
        ]
        .map(|slot| equipment(mob.equipment.get(slot)));
        if actual.as_slice() != case.equipment {
            self.mismatch(format!(
                "{label} equipment: official {:?}, bamboo {actual:?}",
                case.equipment
            ));
        }
        let head_drop = mob.equipment.drop_chance(EquipmentSlot::Head).to_bits() as i32;
        if head_drop != case.head_drop_bits {
            self.mismatch(format!(
                "{label} head drop bits: official {}, bamboo {head_drop}",
                case.head_drop_bits
            ));
        }
    }
}
