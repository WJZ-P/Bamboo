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
    indirect: bool,
    memory_ticks: i32,
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
pub(super) struct Drop {
    item: i32,
    count: i32,
    damage: i32,
    xp: i32,
}

pub(super) fn player_view() -> PlayerView {
    PlayerView {
        shoulder_left: None,
        shoulder_right: None,
        uid: 1025,
        yaw: 0.0,
        pitch: 0.0,
        uuid: Some(((1_u128 << 64) | 2).to_be_bytes()),
        position: Vec3::new(2.5, 64.0, 0.5),
        delta: Vec3::default(),
        width: 0.6,
        height: 1.8,
        eye_height: 1.62,
        health: 20.0,
        look_direction: Vec3::default(),
        gaze_disguise: false,
        alive: true,
        spectator: false,
        game_mode: 0,
        mining_fatigue: None,
        creative: false,
        flying: false,
        explosion_knockback_resistance: 0.0,
        sneaking: false,
        sprinting: false,
        swimming: false,
        invisible: false,
        armor_cover: 0.0,
        main_hand: items::DIAMOND_SWORD,
        off_hand: 0,
        team: None,
        combat: Default::default(),
        effects: Default::default(),
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

pub(super) fn snapshot(public: mc_bamboo_blocks::entity::EntityPublic) -> Option<Drop> {
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
            raid: None,
            natural_spawn: None,
            biome: None,
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
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(1),
            players: vec![player],
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            containers: Default::default(),
            container_open_counts: Default::default(),
            explosion_break: 0,
            shoulder: Default::default(),
        };
        let mut ticks = BlockTicks::default();
        let mut world = World::new(&mut level, &mut ticks);
        let mut entity = world.prepare_entity(Entity::by_mob_type(
            MobType::Skeleton,
            Vec3::new(0.5, 64.0, 0.5),
        ));
        entity.random = JavaRandom::new(case.seed);
        entity.with_mob(|_, mob| {
            if case.memory_ticks > 0 {
                mob.last_hurt_by_player_ref = player.uuid;
                mob.last_hurt_by_player_memory_time = case.memory_ticks;
            }
            for gear in &case.gear {
                let slot = SLOTS[gear.slot];
                mob.equipment.set(slot, gear_stack(gear));
                mob.equipment.set_drop_chance(slot, gear.chance);
            }
        });
        let uid = world.add_prepared_entity(entity);
        let mut source = if case.player_kill {
            DamageSource::player(player.uid, player.position, false)
        } else {
            DamageSource::environment(DamageKind::Fall)
        };
        if case.indirect {
            source.kind = DamageKind::Arrow;
            source.direct = Some(2048);
            source.projectile_motion = Some(Vec3::new(1.0, 0.0, 0.0));
        }
        if self.break_test.as_deref() == Some("skeleton_death_credit") {
            if case.indirect {
                source.attacker_is_player = false;
            }
            if case.memory_ticks > 0 {
                world.with_entity_mut(uid, &mut |entity| {
                    if let Some(mob) = entity.kind.mob_mut() {
                        mob.last_hurt_by_player_memory_time = 0;
                    }
                });
            }
        }
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
        // 官方一侧只列六个人形槽；身体和鞍槽骷髅用不到。
        // Vanilla lists the six humanoid slots; skeletons never use body or saddle.
        let remaining: Vec<i32> = SLOTS[..6]
            .iter()
            .map(|slot| mob.kind.mob().unwrap().equipment.get(*slot).item)
            .collect();
        let spawned: Vec<_> = level
            .entities
            .take_spawn_snapshots()
            .into_iter()
            .filter_map(snapshot)
            .collect();
        let expected = (true, true, case.mob_seed, case.level_seed);
        let label = format!(
            "skeleton_death seed {} looting {} indirect {} memory {}",
            case.seed, case.looting, case.indirect, case.memory_ticks
        );
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
