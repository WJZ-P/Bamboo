//! 玩家弓、弩和发射器射箭的初态对拍：箭的位置、速度、朝向、暴击、穿透、火焰、
//! 拾取方式和物品栏。飞行本身由 `projectile` 场景逐刻比。
//!
//! Compares the initial state of player bow, crossbow and dispenser shots: arrow
//! position, velocity, rotation, crit, piercing, fire, pickup and the inventory.
//! Tick-by-tick flight is covered by the projectile scenes.

use mc_bamboo_blocks::entity::FIRST_ENTITY_ID as FIRST_ID;
use mc_bamboo_data::items;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ShotCase {
    name: String,
    hold: i32,
    yaw: i32,
    pitch: i32,
    level_seed: i64,
    arrows: Vec<ShotArrow>,
    ammo: i32,
    ammo_item: i32,
    damage: i32,
    player_seed: i64,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ShotArrow {
    x: i64,
    y: i64,
    z: i64,
    dx: i64,
    dy: i64,
    dz: i64,
    yaw: i32,
    pitch: i32,
    crit: bool,
    pierce: i32,
    fire: i32,
    pickup: u8,
    item: i32,
    count: i32,
}

type PlayerShot = (i32, i32, i32, Vec<(String, i32)>, Option<i32>, bool);

impl ShotCase {
    fn player(&self) -> Option<PlayerShot> {
        let plain = || Vec::new();
        let scene = match self.name.as_str() {
            "shot_bow_short_draw" | "shot_bow_half_draw" | "shot_bow_moving" => {
                (items::BOW, items::ARROW, 5, plain(), None, false)
            }
            "shot_bow_enchanted" => (
                items::BOW,
                items::ARROW,
                5,
                ["power", "punch", "flame", "unbreaking"]
                    .into_iter()
                    .zip([3, 2, 1, 2])
                    .map(|(name, level)| (format!("minecraft:{name}"), level))
                    .collect(),
                None,
                false,
            ),
            "shot_bow_full_draw" => (items::BOW, items::ARROW, 5, plain(), None, false),
            "shot_bow_infinity" => (
                items::BOW,
                items::ARROW,
                5,
                vec![("minecraft:infinity".to_owned(), 1)],
                None,
                false,
            ),
            "shot_bow_tipped" => (
                items::BOW,
                items::TIPPED_ARROW,
                5,
                plain(),
                Some(mc_bamboo_data::registries::potion::POISON),
                false,
            ),
            "shot_bow_creative" => (items::BOW, items::ARROW, 5, plain(), None, true),
            "shot_crossbow_load" => (items::CROSSBOW, items::ARROW, 5, plain(), None, false),
            "shot_crossbow_multishot" | "shot_crossbow_angled_multishot" => (
                items::CROSSBOW,
                items::ARROW,
                5,
                vec![("minecraft:multishot".to_owned(), 1)],
                None,
                false,
            ),
            "shot_crossbow_quick_charge" => (
                items::CROSSBOW,
                items::ARROW,
                5,
                vec![("minecraft:quick_charge".to_owned(), 2)],
                None,
                false,
            ),
            "shot_crossbow_piercing" => (
                items::CROSSBOW,
                items::ARROW,
                5,
                vec![("minecraft:piercing".to_owned(), 3)],
                None,
                false,
            ),
            "shot_trident" | "shot_trident_moving" => {
                (items::TRIDENT, items::ARROW, 5, plain(), None, false)
            }
            "shot_trident_creative" => (items::TRIDENT, items::ARROW, 5, plain(), None, true),
            "shot_dispenser_arrow" | "shot_dispenser_arrow_up" => return None,
            other => panic!("unrecognized shot scene {other}"),
        };
        Some(scene)
    }

    fn replay_player(&self, report: &mut Report, scene: PlayerShot) {
        let (weapon, ammo_item, ammo_count, enchantments, potion, creative) = scene;
        let result = mc_bamboo_game::replay_shot(&mc_bamboo_game::ReplayShot {
            weapon,
            ammo_item,
            ammo_count,
            hold_ticks: self.hold,
            yaw: self.yaw,
            pitch: self.pitch,
            movement: (self.yaw != 0).then_some(Vec3::new(0.125, -0.25, 0.0625)),
            enchantments,
            potion,
            creative,
        });
        self.compare_arrows(report, &result.arrows);
        if (result.ammo, result.ammo_item, result.weapon_damage)
            != (self.ammo, self.ammo_item, self.damage)
        {
            report.mismatch(format!(
                "{} inventory: official ({}, {}, {}), bamboo ({}, {}, {})",
                self.name,
                self.ammo,
                self.ammo_item,
                self.damage,
                result.ammo,
                result.ammo_item,
                result.weapon_damage
            ));
        }
        if result.level_seed != self.level_seed {
            report.mismatch(format!(
                "{} level random: official {}, bamboo {}",
                self.name, self.level_seed, result.level_seed
            ));
        }
        if result.player_seed != self.player_seed {
            report.mismatch(format!(
                "{} player random: official {}, bamboo {}",
                self.name, self.player_seed, result.player_seed
            ));
        }
    }

    fn replay_dispenser(&self, report: &mut Report) {
        let mut level = MobLevel {
            natural_spawn: None,
            biome: None,
            blocks: HashMap::new(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts::default(),
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(FIRST_ID),
            players: Vec::new(),
            game: None,
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            explosion_break: 0,
        };
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        {
            let mut world = World::new(&mut level, &mut ticks);
            let left = mc_bamboo_blocks::dispense::dispense_arrow_at(
                &mut world,
                BlockPos::new(0, 64, 0),
                if self.name.ends_with("_up") {
                    mc_bamboo_core::Direction::Up
                } else {
                    mc_bamboo_core::Direction::East
                },
                ItemStack::new(items::ARROW, 1),
            );
            let same_item = left.count == 0 || left.item == self.ammo_item;
            if left.count != self.ammo || !same_item {
                report.mismatch(format!(
                    "{} dispenser stack: official ({}, {}), bamboo ({}, {})",
                    self.name, self.ammo, self.ammo_item, left.count, left.item
                ));
            }
        }
        let region = &level.entities;
        let mut arrows = Vec::new();
        for uid in region.ids() {
            let Some(entity) = region.public_at(uid) else {
                continue;
            };
            let mc_bamboo_blocks::entity::KindPublic::Arrow(public) = entity.kind else {
                continue;
            };
            let Some(state) = region.get(uid) else {
                continue;
            };
            let mc_bamboo_blocks::entity::EntityKind::Arrow(arrow) = &state.kind else {
                continue;
            };
            arrows.push(mc_bamboo_game::ReplayShotArrow {
                motion: [
                    entity.position.x.to_bits() as i64,
                    entity.position.y.to_bits() as i64,
                    entity.position.z.to_bits() as i64,
                    entity.delta.x.to_bits() as i64,
                    entity.delta.y.to_bits() as i64,
                    entity.delta.z.to_bits() as i64,
                ],
                yaw: entity.yaw.to_bits() as i32,
                pitch: public.pitch.to_bits() as i32,
                critical: public.critical,
                pierce: i32::from(public.pierce),
                fire: state.remaining_fire_ticks,
                pickup: match public.pickup {
                    mc_bamboo_blocks::entity::arrow::Pickup::Disallowed => 0,
                    mc_bamboo_blocks::entity::arrow::Pickup::Allowed => 1,
                    mc_bamboo_blocks::entity::arrow::Pickup::CreativeOnly => 2,
                },
                item: arrow.pickup_item.item,
                count: arrow.pickup_item.count,
            });
        }
        self.compare_arrows(report, &arrows);
    }

    fn compare_arrows(&self, report: &mut Report, arrows: &[mc_bamboo_game::ReplayShotArrow]) {
        if arrows.len() != self.arrows.len() {
            report.mismatch(format!(
                "{} arrow count: official {}, bamboo {}",
                self.name,
                self.arrows.len(),
                arrows.len()
            ));
        }
        for (theirs, ours) in self.arrows.iter().zip(arrows) {
            let expected = ShotArrow {
                x: theirs.x,
                y: theirs.y,
                z: theirs.z,
                dx: theirs.dx,
                dy: theirs.dy,
                dz: theirs.dz,
                yaw: theirs.yaw,
                pitch: theirs.pitch,
                crit: theirs.crit,
                pierce: theirs.pierce,
                fire: theirs.fire,
                pickup: theirs.pickup,
                item: theirs.item,
                count: theirs.count,
            };
            let actual = ShotArrow {
                x: ours.motion[0],
                y: ours.motion[1],
                z: ours.motion[2],
                dx: ours.motion[3],
                dy: ours.motion[4],
                dz: ours.motion[5],
                yaw: ours.yaw,
                pitch: ours.pitch,
                crit: ours.critical,
                pierce: ours.pierce,
                fire: ours.fire,
                pickup: ours.pickup,
                item: ours.item,
                count: ours.count,
            };
            if expected != actual {
                let names = [
                    "x", "y", "z", "dx", "dy", "dz", "yaw", "pitch", "crit", "pierce", "fire",
                    "pickup", "item", "count",
                ];
                let mut diffs = Vec::new();
                for (index, name) in names.iter().enumerate() {
                    let same = match index {
                        0..=5 => expected.motion()[index] == actual.motion()[index],
                        6 => theirs.yaw == ours.yaw,
                        7 => theirs.pitch == ours.pitch,
                        8 => theirs.crit == ours.critical,
                        9 => theirs.pierce == ours.pierce,
                        10 => theirs.fire == ours.fire,
                        11 => theirs.pickup == ours.pickup,
                        12 => theirs.item == ours.item,
                        _ => theirs.count == ours.count,
                    };
                    if !same {
                        diffs.push(format!(
                            "{name}: official {} bamboo {}",
                            describe_shot(&expected, index),
                            describe_shot(&actual, index)
                        ));
                    }
                }
                report.mismatch(format!("{} arrow: {}", self.name, diffs.join("; ")));
            }
        }
    }
}

impl ShotArrow {
    fn motion(&self) -> [i64; 6] {
        [self.x, self.y, self.z, self.dx, self.dy, self.dz]
    }
}

fn describe_shot(arrow: &ShotArrow, index: usize) -> String {
    match index {
        0..=5 => {
            let bits = arrow.motion()[index];
            f64::from_bits(bits as u64).to_string()
        }
        6 => f32::from_bits(arrow.yaw as u32).to_string(),
        7 => f32::from_bits(arrow.pitch as u32).to_string(),
        8 => arrow.crit.to_string(),
        9 => arrow.pierce.to_string(),
        10 => arrow.fire.to_string(),
        11 => arrow.pickup.to_string(),
        12 => arrow.item.to_string(),
        _ => arrow.count.to_string(),
    }
}

impl Report {
    pub(super) fn replay_shot(&mut self, case: &ShotCase) {
        self.cases += 1;
        self.steps += 1;
        match case.player() {
            Some(scene) => case.replay_player(self, scene),
            None => case.replay_dispenser(self),
        }
    }
}
