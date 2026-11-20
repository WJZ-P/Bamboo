use mc_bamboo_blocks::entity::{
    KindPublic,
    mob::{DamageKind, MobType, equipment::SLOTS},
};
use mc_bamboo_data::{items, registries::data_component_type as kinds};
use mc_bamboo_protocol::components::{ComponentValue, PatchEntry, defaults};

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DeathCase {
    seed: i64,
    gear: Vec<Gear>,
    player_kill: bool,
    looting: i32,
    drops: bool,
    spawned: Vec<Drop>,
    remaining: Vec<i32>,
    mob_seed: i64,
    level_seed: i64,
}

#[derive(Debug, Deserialize)]
struct Gear {
    slot: usize,
    item: i32,
    chance: f32,
    damage: i32,
    vanishing: bool,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct Drop {
    item: i32,
    count: i32,
    damage: i32,
    xp: i32,
}

fn player_view() -> PlayerView {
    PlayerView {
        uid: 1025,
        uuid: None,
        position: Vec3::new(2.5, 64.0, 0.5),
        eye_height: 1.62,
        alive: true,
        spectator: false,
        creative: false,
        sneaking: false,
        invisible: false,
        armor_cover: 0.0,
        main_hand: items::DIAMOND_SWORD,
        off_hand: 0,
    }
}

fn gear_stack(gear: &Gear) -> ItemStack {
    let mut stack = ItemStack::new(gear.item, 1);
    let mut patch = stack.patch();
    if gear.damage > 0 {
        patch.put(
            kinds::DAMAGE,
            PatchEntry::Set(ComponentValue::Damage(gear.damage)),
        );
    }
    if gear.vanishing {
        let id = mc_bamboo_loot::enchantment_id("minecraft:vanishing_curse").unwrap();
        patch.put(
            kinds::ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(vec![(id, 1)])),
        );
    }
    (stack.added, stack.removed) = patch.to_opaque();
    stack
}

fn snapshot(public: mc_bamboo_blocks::entity::EntityPublic) -> Option<Drop> {
    match public.kind {
        KindPublic::Item { stack, .. } => {
            let damage = match defaults::resolve(stack.item, &stack.patch(), kinds::DAMAGE) {
                Some(ComponentValue::Damage(damage)) => *damage,
                _ => 0,
            };
            Some(Drop {
                item: stack.item,
                count: stack.count,
                damage,
                xp: 0,
            })
        }
        KindPublic::ExperienceOrb { value } => Some(Drop {
            item: 0,
            count: 0,
            damage: 0,
            xp: value,
        }),
        _ => None,
    }
}

impl Report {
    pub(super) fn replay_skeleton_death(&mut self, case: &DeathCase) {
        self.cases += 1;
        let mut rules = mc_bamboo_blocks::BlockGameRules::vanilla();
        rules.mob_drops = case.drops;
        let player = player_view();
        let mut level = MobLevel {
            blocks: HashMap::new(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: vec![case.looting],
            rules,
            facts: ZombieFacts {
                difficulty: 2,
                ..ZombieFacts::default()
            },
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(1),
            players: vec![player],
            game: None,
            wrong_was_hurt: false,
        };
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let mut entity = world.prepare_entity(Entity::by_mob_type(
            MobType::Skeleton,
            Vec3::new(0.5, 64.0, 0.5),
        ));
        entity.random = JavaRandom::new(case.seed);
        entity.with_mob(|_, mob| {
            for gear in &case.gear {
                let slot = SLOTS[gear.slot];
                mob.equipment.set(slot, gear_stack(gear));
                mob.equipment.set_drop_chance(slot, gear.chance);
            }
        });
        let uid = world.add_prepared_entity(entity);
        let source = if case.player_kill {
            let mut weapon = ItemStack::new(items::DIAMOND_SWORD, 1);
            if case.looting > 0 {
                let id = mc_bamboo_loot::enchantment_id("minecraft:looting").unwrap();
                let mut patch = weapon.patch();
                patch.put(
                    kinds::ENCHANTMENTS,
                    PatchEntry::Set(ComponentValue::Enchantments(vec![(id, case.looting)])),
                );
                (weapon.added, weapon.removed) = patch.to_opaque();
            }
            DamageSource::player(player.uid, player.position, false).with_looting_from(&weapon)
        } else {
            DamageSource::environment(DamageKind::Fall)
        };
        if self.break_test.as_deref() == Some("skeleton_death") {
            world.with_entity_mut(uid, &mut |entity| {
                entity.random.next_float();
            });
        }
        let hit = world.hurt_mob(uid, source, 100.0);
        let mob = world.entity(uid).unwrap();
        let state = (
            hit,
            mob.kind.mob().unwrap().dead,
            mob.random.state(),
            world.random().seed(),
        );
        let remaining = SLOTS.map(|slot| mob.kind.mob().unwrap().equipment.get(slot).item);
        let spawned: Vec<_> = level
            .entities
            .take_spawn_snapshots()
            .into_iter()
            .filter_map(snapshot)
            .collect();
        let expected = (true, true, case.mob_seed, case.level_seed);
        let label = format!("skeleton_death seed {} looting {}", case.seed, case.looting);
        if state != expected {
            self.mismatch(format!(
                "{label} hit/dead/mob RNG/world RNG: official {expected:?}, bamboo {state:?}"
            ));
        }
        if spawned != case.spawned {
            self.mismatch(format!(
                "{label} drops: official {:?}, bamboo {spawned:?}",
                case.spawned
            ));
        }
        if remaining.as_slice() != case.remaining {
            self.mismatch(format!(
                "{label} remaining equipment: official {:?}, bamboo {remaining:?}",
                case.remaining
            ));
        }
    }
}
