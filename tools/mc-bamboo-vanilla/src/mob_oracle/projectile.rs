use std::collections::BTreeSet;

use mc_bamboo_blocks::entity::{KindPublic, mob::MobType};
use mc_bamboo_game::{ReplayPlayerSetup, ReplayPlayers};
use mc_bamboo_protocol::{
    frame::packet_body,
    item::ItemStack,
    packet::play::{EquipmentSlot, RemoveEntities},
};

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ArrowPickupCase {
    count: i32,
    pickup: u8,
    grounded: bool,
    shake: u8,
    creative: bool,
    full_inventory: i32,
    removed: bool,
    arrows: i32,
    remaining: i32,
}

impl Report {
    pub(super) fn replay_arrow_pickup(&mut self, case: &ArrowPickupCase) {
        use mc_bamboo_blocks::entity::arrow::Pickup;
        self.cases += 1;
        let pickup = match case.pickup {
            0 => Pickup::Disallowed,
            1 => Pickup::Allowed,
            2 => Pickup::CreativeOnly,
            value => panic!("unrecognized pickup mode {value}"),
        };
        let actual = mc_bamboo_game::replay_arrow_pickup(
            case.count,
            pickup,
            case.grounded,
            case.shake,
            case.creative,
            (case.full_inventory >= 0).then_some(case.full_inventory),
        );
        let expected = (case.removed, case.arrows, case.remaining);
        if actual != expected {
            self.mismatch(format!(
                "arrow pickup {case:?}: official {expected:?}, bamboo {actual:?}"
            ));
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProjectileCase {
    name: String,
    difficulty: i32,
    wall: bool,
    skeleton: bool,
    first_id: i32,
    player_id: i32,
    player_x: f64,
    blocks: Vec<[i32; 4]>,
    frames: Vec<ProjectileFrame>,
    player_states: Vec<[i64; 5]>,
    player_packets: Vec<Vec<String>>,
    shots: usize,
    embeds: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectileFrame {
    arrows: Vec<ArrowFrame>,
    packets: Vec<String>,
    shooter: Option<[i64; 8]>,
    shooter_seed: Option<i64>,
    aggressive: Option<bool>,
    drawing: Option<bool>,
    player_thermal: Option<[i32; 2]>,
    targets: Option<[i32; 2]>,
    effect_target: Option<[i32; 2]>,
    player_motion: Option<[i64; 3]>,
    /// 本刻的熄火音效：x/y/z 的原始位、音量、音高、声源序号。
    ///
    /// Extinguish sounds this tick: raw x/y/z bits, volume, pitch, and source ordinal.
    extinguished: Option<Vec<[i64; 6]>>,
    /// 骷髅一支的射手：实体 ID、类型、index 16 的布尔、冻结刻数、血量位、主手物品。
    ///
    /// The skeleton-family shooter: id, type, the index-16 boolean, frozen ticks, health
    /// bits and main-hand item.
    kin: Option<[i64; 6]>,
    /// 本刻的 `levelEvent`：事件号、坐标、数据。
    ///
    /// This tick's `levelEvent` calls: event, position and data.
    level_events: Option<Vec<[i32; 5]>>,
    /// 玩家身上的效果：效果 ID、时长、等级（官方一侧的玩家不 tick）。
    ///
    /// Effects on the player: id, duration, amplifier (the official player is not ticked).
    player_effects: Option<Vec<[i32; 3]>>,
    shearing: Option<Shearing>,
}

/// 一次剪刀右键：是否消耗动作，以及掉出来的物品实体（物品、数量、位置和速度的原始位）。
/// 官方一侧另外输出剪刀的损耗 `damage`，这里不读。
///
/// One shears right-click: whether it consumed the action and the dropped item entities
/// (item, count, raw position and velocity bits). The official `damage` field is not read.
#[derive(Debug, Deserialize)]
struct Shearing {
    consumed: bool,
    drops: Vec<[i64; 8]>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ArrowFrame {
    id: i32,
    motion: [i64; 8],
    ground: bool,
    shake: u8,
    seed: i64,
    fire: Option<i32>,
}

impl ProjectileCase {
    fn flaming(&self) -> bool {
        self.name.starts_with("arrow_flame_")
    }

    fn piercing(&self) -> bool {
        self.name.starts_with("arrow_piercing_")
    }

    fn power_punch(&self) -> bool {
        self.name == "arrow_power_punch_hit"
    }

    fn effect(&self) -> Option<i32> {
        use mc_bamboo_data::registries::mob_effect;
        match self.name.as_str() {
            "arrow_tipped_hit" => Some(mob_effect::POISON),
            "arrow_spectral_hit" => Some(mob_effect::GLOWING),
            _ => None,
        }
    }

    fn fades(&self) -> bool {
        self.name == "arrow_tipped_fade"
    }

    /// 弹药带的效果；褪色场景用毒箭，但不放靶子。
    ///
    /// The ammo's effect; the fade case uses a poison arrow without a target.
    fn ammo_effect(&self) -> Option<i32> {
        if self.fades() {
            Some(mc_bamboo_data::registries::mob_effect::POISON)
        } else {
            self.effect()
        }
    }

    fn configure_effect_arrow(&self, data: &mut mc_bamboo_blocks::entity::arrow::Arrow) {
        use mc_bamboo_blocks::entity::arrow::ArrowVariant;
        use mc_bamboo_data::{
            items,
            registries::{data_component_type, mob_effect, potion},
        };
        use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};

        match self.ammo_effect() {
            Some(mob_effect::GLOWING) => {
                data.pickup_item = ItemStack::new(items::SPECTRAL_ARROW, 1);
                data.variant = ArrowVariant::of_ammo(&data.pickup_item);
            }
            Some(mob_effect::POISON) => {
                let mut patch = ComponentPatch::new();
                patch.put(
                    data_component_type::POTION_CONTENTS,
                    PatchEntry::Set(ComponentValue::PotionContents {
                        potion: Some(potion::POISON),
                        custom_color: None,
                        custom_effects: Vec::new(),
                        custom_name: None,
                    }),
                );
                let (added, removed) = patch.to_opaque();
                data.pickup_item = ItemStack {
                    added,
                    removed,
                    ..ItemStack::new(items::TIPPED_ARROW, 1)
                };
            }
            _ => {}
        }
    }

    /// 射手的种类，按场景名前缀。
    ///
    /// The shooter kind, from the scene name prefix.
    fn shooter_kind(&self) -> MobType {
        if self.name.starts_with("stray_") {
            MobType::Stray
        } else if self.name.starts_with("bogged_") {
            MobType::Bogged
        } else {
            MobType::Skeleton
        }
    }

    fn shears(&self) -> bool {
        self.name == "bogged_shear"
    }

    /// 玩家无敌、不会被当成目标的场景。
    ///
    /// Scenes whose player is invulnerable and never targeted.
    fn passive(&self) -> bool {
        self.shears() || self.name == "skeleton_freeze"
    }

    fn overlaps_flight(&self) -> bool {
        self.name == "arrow_block_overlaps_flight"
    }

    fn starts_without_gravity(&self) -> bool {
        self.name == "arrow_no_gravity"
    }

    fn water(&self) -> bool {
        self.name.starts_with("arrow_water_")
    }

    fn water_tail(&self) -> bool {
        self.name == "arrow_water_after_pierce"
    }

    fn level(&self) -> MobLevel {
        MobLevel {
            natural_spawn: None,
            blocks: self
                .blocks
                .iter()
                .map(|&[x, y, z, state]| (BlockPos::new(x, y, z), BlockState(state as u32)))
                .collect(),
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: self.difficulty,
                ..ZombieFacts::default()
            },
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            entities: Entities::with_next_id(self.first_id),
            players: Vec::new(),
            game: Some(ReplayPlayers::new(
                self.difficulty,
                &[ReplayPlayerSetup {
                    id: self.player_id,
                    uuid: (1 << 64) | 2,
                    name: "ArrowOracle".to_owned(),
                    position: Vec3::new(self.player_x, 64.0, 0.5),
                    health: 20.0,
                    seed: mc_bamboo_blocks::entity::ENTITY_RANDOM_SEED,
                }],
            )),
            wrong_was_hurt: false,
        }
    }

    fn spawn(&self, world: &mut World<'_>, mutate: bool, break_pierce: bool) {
        if self.skeleton {
            let mut skeleton = world.prepare_entity(Entity::by_mob_type(
                self.shooter_kind(),
                Vec3::new(0.5, 64.0, 0.5),
            ));
            skeleton.random = JavaRandom::new(42);
            skeleton.y_rot = 0.0;
            let mob = skeleton.kind.mob_mut().unwrap();
            mob.uuid = 1_u128.to_be_bytes();
            mob.y_head_rot = 0.0;
            mob.y_body_rot = 0.0;
            if !mutate {
                mob.equipment.set(
                    EquipmentSlot::MainHand,
                    ItemStack::new(mc_bamboo_data::items::BOW, 1),
                );
            }
            world.add_prepared_entity(skeleton);
        } else {
            let y = if self.water() {
                65.25
            } else if self.piercing() {
                65.0
            } else {
                65.5
            };
            let mut arrow = world.prepare_entity(Entity::arrow(Vec3::new(0.5, y, 0.5)));
            if let EntityKind::Arrow(data) = &mut arrow.kind {
                data.no_gravity = self.starts_without_gravity();
                self.configure_effect_arrow(data);
                if self.power_punch() {
                    use mc_bamboo_protocol::components::{
                        ComponentPatch, ComponentValue, PatchEntry,
                    };
                    let registry = mc_bamboo_data::registries::SYNCED_REGISTRIES
                        .iter()
                        .find(|registry| registry.name == "minecraft:enchantment")
                        .unwrap();
                    let levels = ["minecraft:power", "minecraft:punch"]
                        .into_iter()
                        .zip([3, 2])
                        .map(|(name, level)| {
                            (
                                registry
                                    .entries
                                    .iter()
                                    .position(|entry| *entry == name)
                                    .unwrap() as i32,
                                level,
                            )
                        })
                        .collect();
                    let mut patch = ComponentPatch::new();
                    patch.put(
                        mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
                        PatchEntry::Set(ComponentValue::Enchantments(levels)),
                    );
                    let (added, removed) = patch.to_opaque();
                    data.weapon = Some(ItemStack {
                        added,
                        removed,
                        ..ItemStack::new(mc_bamboo_data::items::BOW, 1)
                    });
                }
                if self.piercing() || self.water_tail() {
                    data.pierce = i8::from(!break_pierce);
                    data.base_damage = 0.5;
                }
            }
            if !self.overlaps_flight() {
                let power = if self.water() {
                    7.0
                } else if self.piercing() {
                    6.0
                } else {
                    1.6
                };
                arrow.shoot_arrow(
                    Vec3::new(1.0, if mutate { 0.25 } else { 0.0 }, 0.0),
                    power,
                    0.0,
                );
            }
            world.add_prepared_entity(arrow);
            if self.effect().is_some() {
                let mut target = Entity::by_mob_type(MobType::Cow, Vec3::new(6.5, 64.0, 0.5));
                target.with_mob(|_, mob| mob.no_ai = true);
                world.spawn_entity(target);
            }
            if self.piercing() {
                for x in [3.5, 5.5] {
                    let mut target = Entity::by_mob_type(MobType::Cow, Vec3::new(x, 64.0, 0.5));
                    target.with_mob(|_, mob| mob.no_ai = true);
                    world.spawn_entity(target);
                }
            }
        }
    }
}

impl Report {
    pub(super) fn replay_projectile(&mut self, case: &ProjectileCase) {
        self.cases += 1;
        let mut level = case.level();
        if self.break_test.as_deref() == Some("arrow_water") {
            // 改坏确认：拿掉水面，熄火音效和火焰刻都应该对不上。
            // Break self-test: drop the water so the extinguish sound and fire ticks must differ.
            level.blocks.remove(&BlockPos::new(3, 65, 0));
            level.blocks.remove(&BlockPos::new(4, 65, 0));
        }
        level.game.as_mut().unwrap().set_arrow_target(
            case.player_id,
            case.name.ends_with("_rejected") || case.passive(),
            case.flaming().then_some([37, 80]),
        );
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        case.spawn(
            &mut World::new(&mut level, &mut ticks),
            self.break_test.as_deref() == Some("projectile"),
            self.break_test.as_deref() == Some("arrow_piercing"),
        );
        let mut tracking = EntityTracking::default();
        let mut previous = BTreeSet::new();
        let mut shots = 0;
        let mut embeds = 0;
        let mut packet_count = 0;
        let mut player_effects = std::collections::BTreeMap::new();
        let extinguish_sound = i32::from(mc_bamboo_data::sounds::sound_event(
            "minecraft:entity.generic.extinguish_fire",
        ));
        for (tick, expected) in case.frames.iter().enumerate() {
            let mut extinguished = Vec::new();
            let mut level_events = Vec::new();
            if tick != 0 {
                if (case.flaming() || case.water()) && tick == 1 {
                    level
                        .entities
                        .with_mut(case.first_id, |arrow| arrow.remaining_fire_ticks = 200);
                }
                if case.wall && tick == 12 {
                    level.blocks.remove(&BlockPos::new(7, 65, 0));
                }
                if case.overlaps_flight() && tick == 2 {
                    level.blocks.insert(
                        BlockPos::new(0, 65, 0),
                        BlockState(mc_bamboo_data::blocks::STONE),
                    );
                }
                if case.starts_without_gravity() && tick == 10 {
                    level.entities.with_mut(case.first_id, |entity| {
                        if let EntityKind::Arrow(data) = &mut entity.kind {
                            data.no_gravity = false;
                        }
                    });
                }
                if case.name == "arrow_despawn" && tick == 6 {
                    level.entities.with_mut(case.first_id, |entity| {
                        if let EntityKind::Arrow(data) = &mut entity.kind {
                            data.life = 1198;
                        }
                    });
                }
                if case.fades() && tick == 6 {
                    level.entities.with_mut(case.first_id, |entity| {
                        if let EntityKind::Arrow(data) = &mut entity.kind {
                            data.in_ground_time = 597;
                        }
                    });
                }
                level.game.as_mut().unwrap().tick_cooldowns();
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                if case.shears() && (tick == 3 || tick == 6) {
                    let ours = shear_shooter(&mut world, case);
                    self.check_shearing(&case.name, tick, expected.shearing.as_ref(), &ours);
                }
                world.tick_entities();
                let effects = world.drain_effects();
                for effect in &effects {
                    match effect {
                        mc_bamboo_blocks::Effect::LevelEvent {
                            event, pos, data, ..
                        } => level_events.push([*event, pos.x, pos.y, pos.z, *data]),
                        mc_bamboo_blocks::Effect::PlayerEffect {
                            effect,
                            duration,
                            amplifier,
                            ..
                        } => {
                            // 官方一侧的玩家不 tick，同一种效果再来一次不会更长，保留第一次的。
                            // The official player is not ticked, so a repeat never lasts longer.
                            player_effects
                                .entry(*effect)
                                .or_insert([*effect, *duration, *amplifier]);
                        }
                        _ => {}
                    }
                }
                extinguished.extend(effects.iter().filter_map(|effect| {
                    let mc_bamboo_blocks::Effect::Sound {
                        sound,
                        source,
                        x,
                        y,
                        z,
                        volume,
                        pitch,
                        ..
                    } = effect
                    else {
                        return None;
                    };
                    (*sound == extinguish_sound).then(|| {
                        [
                            x.to_bits() as i64,
                            y.to_bits() as i64,
                            z.to_bits() as i64,
                            i64::from(volume.to_bits()),
                            i64::from(pitch.to_bits()),
                            i64::from(source.id()),
                        ]
                    })
                }));
            }
            if let Some(thermal) = expected.player_thermal {
                let actual = level
                    .game
                    .as_ref()
                    .unwrap()
                    .arrow_target_thermal(case.player_id);
                if actual != thermal {
                    self.mismatch(format!("{} tick {tick} player fire/frozen: official {thermal:?}, bamboo {actual:?}", case.name));
                }
            }
            if let Some(theirs) = expected.player_motion {
                let delta = level
                    .game
                    .as_ref()
                    .unwrap()
                    .arrow_target_motion(case.player_id);
                let ours = [delta.x, delta.y, delta.z].map(|component| component.to_bits() as i64);
                if ours != theirs {
                    self.mismatch(format!(
                        "{} tick {tick} Punch velocity: official {:?}, bamboo {delta:?}",
                        case.name,
                        theirs.map(|value| f64::from_bits(value as u64))
                    ));
                }
            }
            if let Some(theirs) = expected.effect_target {
                let entity = level.entities.get(case.first_id + 1).unwrap();
                let mob = entity.kind.mob().unwrap();
                let effect = mob.effects.get(case.effect().unwrap());
                let ours = [
                    mob.health.to_bits() as i32,
                    effect.map_or(-1, |effect| effect.duration),
                ];
                if ours != theirs {
                    self.mismatch(format!("{} tick {tick} target health bits/effect duration: official {theirs:?}, bamboo {ours:?}", case.name));
                }
            }
            if let Some(targets) = expected.targets {
                let actual = [case.first_id + 1, case.first_id + 2].map(|id| {
                    level
                        .entities
                        .get(id)
                        .unwrap()
                        .kind
                        .mob()
                        .unwrap()
                        .health
                        .to_bits() as i32
                });
                if actual != targets {
                    self.mismatch(format!("{} tick {tick} target health bits: official {targets:?}, bamboo {actual:?}", case.name));
                }
            }
            // 箭自己的实体事件（药水褪色）在 tick 里广播，排在移除和配对包前面。
            // An arrow's own entity events (the potion puff) are broadcast during
            // its tick, ahead of removal and pairing packets.
            let arrow_events: Vec<_> = level
                .entities
                .take_mob_events()
                .into_iter()
                .filter(|(uid, _)| {
                    level
                        .entities
                        .public_at(*uid)
                        .is_some_and(|public| matches!(public.kind, KindPublic::Arrow(_)))
                })
                .collect();
            let mut packets = event_packets(&arrow_events, || 0);
            let active: BTreeSet<_> = level
                .entities
                .ids()
                .into_iter()
                .filter(|&id| {
                    level
                        .entities
                        .public_at(id)
                        .is_some_and(|public| matches!(public.kind, KindPublic::Arrow(_)))
                })
                .collect();
            for &uid in previous.difference(&active) {
                packets.push(packet_body(&RemoveEntities { entity_ids: &[uid] }));
            }
            let spawns = level.entities.take_spawn_snapshots();
            for public in spawns {
                if !matches!(public.kind, KindPublic::Arrow(_)) {
                    continue;
                }
                let snapshot = Snapshot::of(&public);
                packets.extend(
                    tracking
                        .tracked_for_oracle(&snapshot)
                        .pairing_packets(&snapshot),
                );
                shots += 1;
            }
            if tick != 0 {
                for &uid in &active {
                    let snapshot = Snapshot::of(level.entities.public_at(uid).unwrap());
                    packets.extend(tracking.tracked_for_oracle(&snapshot).step(
                        &snapshot,
                        snapshot.needs_sync,
                        snapshot.sync_velocity,
                    ));
                }
            }
            let arrows: Vec<_> = active
                .iter()
                .map(|&id| {
                    let entity = level.entities.get(id).unwrap();
                    let EntityKind::Arrow(data) = &entity.kind else {
                        unreachable!()
                    };
                    ArrowFrame {
                        id,
                        motion: motion(&entity, data.pitch),
                        ground: data.in_ground,
                        shake: data.shake_time,
                        seed: entity.random.state(),
                        fire: case.water().then_some(entity.remaining_fire_ticks),
                    }
                })
                .collect();
            embeds += arrows.iter().filter(|arrow| arrow.ground).count();
            let label = format!("{} tick {tick}", case.name);
            self.check_arrow_frames(&label, &expected.arrows, &arrows);
            if let Some(expected_effects) = &expected.extinguished
                && *expected_effects != extinguished
            {
                self.mismatch(format!(
                    "{label} extinguish sounds: official {expected_effects:?}, bamboo {extinguished:?}"
                ));
            }
            if let Some(theirs) = &expected.level_events
                && *theirs != level_events
            {
                self.mismatch(format!(
                    "{label} level events: official {theirs:?}, bamboo {level_events:?}"
                ));
            }
            if let Some(theirs) = &expected.player_effects {
                let ours: Vec<_> = player_effects.values().copied().collect();
                if *theirs != ours {
                    self.mismatch(format!(
                        "{label} player effects: official {theirs:?}, bamboo {ours:?}"
                    ));
                }
            }
            if let Some(expected_motion) = expected.shooter {
                // 转化之后射手换成新生成的那只：取表里编号最大的生物。
                // After a conversion the shooter is the new mob: the highest mob id.
                let shooter_id = shooter_id(&level).unwrap_or(case.first_id);
                let shooter = level.entities.get(shooter_id).unwrap();
                let mob = shooter.kind.mob().unwrap();
                if let Some(theirs) = expected.kin {
                    let ours = kin_state(shooter_id, mob);
                    if ours != theirs {
                        self.mismatch(format!(
                            "{label} shooter id/type/flag/frozen/health/hand: official {theirs:?}, bamboo {ours:?}"
                        ));
                    }
                }
                self.check_projectile_motion(
                    &label,
                    "shooter",
                    &expected_motion,
                    &motion(&shooter, mob.x_rot),
                );
                let ours = (
                    shooter.random.state(),
                    mob.aggressive,
                    mob.using_item_flags != 0,
                );
                let theirs = (
                    expected.shooter_seed.unwrap(),
                    expected.aggressive.unwrap(),
                    expected.drawing.unwrap(),
                );
                if ours != theirs {
                    self.mismatch(format!("{label} shooter seed/aggressive/drawing: official {theirs:?}, bamboo {ours:?}"));
                }
            }
            compare_packet_list(
                &mut self.mismatches,
                &self.show,
                &case.name,
                tick,
                &packets,
                &expected.packets,
            );
            let player = level.game.as_mut().unwrap();
            let state = player.state(case.player_id);
            if state != case.player_states[tick] {
                self.mismatch(format!(
                    "{label} player: official {:?}, bamboo {state:?}",
                    case.player_states[tick]
                ));
            }
            let player_packets: Vec<_> = player
                .take_packets(case.player_id)
                .into_iter()
                .filter(|body| matches!(body.first(), Some(43 | 67 | 68 | 69 | 106)))
                .collect();
            compare_packet_list(
                &mut self.mismatches,
                &self.show,
                &format!("{} player", case.name),
                tick,
                &player_packets,
                &case.player_packets[tick],
            );
            packet_count += packets.len() + player_packets.len();
            previous = active;
            self.steps += 1;
        }
        if (shots, embeds) != (case.shots, case.embeds) {
            self.mismatch(format!(
                "{} coverage: official {} shots / {} embedded ticks, bamboo {shots} / {embeds}",
                case.name, case.shots, case.embeds
            ));
        }
        let health = f32::from_bits(level.game.as_ref().unwrap().state(case.player_id)[0] as u32);
        println!(
            "{}: {} ticks, {shots} arrows, {embeds} embedded ticks, {packet_count} compared packets, final player health {health}",
            case.name,
            case.frames.len()
        );
    }

    fn check_shearing(
        &mut self,
        name: &str,
        tick: usize,
        expected: Option<&Shearing>,
        ours: &(bool, Vec<[i64; 8]>),
    ) {
        let theirs = expected.map(|shearing| (shearing.consumed, shearing.drops.clone()));
        if theirs.as_ref() != Some(ours) {
            self.mismatch(format!(
                "{name} tick {tick} shearing consumed/drops: official {theirs:?}, bamboo {ours:?}"
            ));
        }
    }

    fn check_arrow_frames(&mut self, label: &str, expected: &[ArrowFrame], actual: &[ArrowFrame]) {
        if expected.len() != actual.len() {
            self.mismatch(format!(
                "{label}: official {} arrows, bamboo {}",
                expected.len(),
                actual.len()
            ));
        }
        for (theirs, ours) in expected.iter().zip(actual) {
            if (
                theirs.id,
                theirs.ground,
                theirs.shake,
                theirs.seed,
                theirs.fire,
            ) != (ours.id, ours.ground, ours.shake, ours.seed, ours.fire)
            {
                self.mismatch(format!("{label} arrow id/ground/shake/seed/fire: official ({}, {}, {}, {}, {:?}), bamboo ({}, {}, {}, {}, {:?})",
                    theirs.id, theirs.ground, theirs.shake, theirs.seed, theirs.fire, ours.id, ours.ground, ours.shake, ours.seed, ours.fire));
            }
            self.check_projectile_motion(label, "arrow", &theirs.motion, &ours.motion);
        }
    }

    pub(super) fn check_projectile_motion(
        &mut self,
        label: &str,
        entity: &str,
        expected: &[i64; 8],
        actual: &[i64; 8],
    ) {
        let names = ["x", "y", "z", "dx", "dy", "dz", "yaw", "pitch"];
        if let Some(i) = (0..8).find(|&i| expected[i] != actual[i]) {
            let value = |bits| {
                if i < 6 {
                    f64::from_bits(bits as u64)
                } else {
                    f64::from(f32::from_bits(bits as u32))
                }
            };
            self.mismatch(format!(
                "{label} {entity} {}: official {}, bamboo {}",
                names[i],
                value(expected[i]),
                value(actual[i])
            ));
        }
    }
}

/// 表里编号最大的活生物。
///
/// The highest id among the live mobs in the table.
fn shooter_id(level: &MobLevel) -> Option<i32> {
    level.entities.ids().into_iter().rfind(|&id| {
        level
            .entities
            .public_at(id)
            .is_some_and(|public| !public.removed && matches!(public.kind, KindPublic::Mob(_)))
    })
}

fn kin_state(id: i32, mob: &mc_bamboo_blocks::entity::mob::Mob) -> [i64; 6] {
    let flag = match mob.mob_type {
        MobType::Skeleton => mob.freeze_conversion.converting,
        MobType::Bogged => mob.bogged_sheared,
        _ => false,
    };
    let hand = mob.equipment.get(EquipmentSlot::MainHand);
    [
        i64::from(id),
        i64::from(mob.mob_type.entity_type()),
        i64::from(flag),
        i64::from(mob.ticks_frozen),
        i64::from(mob.health.to_bits() as i32),
        i64::from(if hand.is_empty() { 0 } else { hand.item }),
    ]
}

/// 玩家拿剪刀右键射手，返回是否消耗动作和这次新掉出来的物品实体。
///
/// The player right-clicks the shooter with shears; returns whether the action was
/// consumed and the item entities this dropped.
fn shear_shooter(world: &mut World<'_>, case: &ProjectileCase) -> (bool, Vec<[i64; 8]>) {
    use mc_bamboo_blocks::entity::mob::interact::MobInteractRequest;
    let before: BTreeSet<_> = world.entity_ids().into_iter().collect();
    let request = MobInteractRequest {
        player_uid: case.player_id,
        player_uuid: ((1_u128 << 64) | 2).to_be_bytes(),
        hand: mc_bamboo_core::Hand::Main,
        held: ItemStack::new(mc_bamboo_data::items::SHEARS, 1),
        creative: false,
        sneaking: false,
        position: Vec3::new(case.player_x, 64.0, 0.5),
    };
    let outcome = world.interact_mob(case.first_id, &request);
    let drops = world
        .entity_ids()
        .into_iter()
        .filter(|id| !before.contains(id))
        .filter_map(|id| {
            let entity = world.entity(id)?;
            let EntityKind::Item(item) = &entity.kind else {
                return None;
            };
            let (p, v) = (entity.position, entity.delta);
            Some([
                i64::from(item.stack.item),
                i64::from(item.stack.count),
                p.x.to_bits() as i64,
                p.y.to_bits() as i64,
                p.z.to_bits() as i64,
                v.x.to_bits() as i64,
                v.y.to_bits() as i64,
                v.z.to_bits() as i64,
            ])
        })
        .collect();
    (outcome.result.consumes_action(), drops)
}

pub(super) fn motion(entity: &Entity, pitch: f32) -> [i64; 8] {
    let p = entity.position;
    let v = entity.delta;
    [
        p.x.to_bits() as i64,
        p.y.to_bits() as i64,
        p.z.to_bits() as i64,
        v.x.to_bits() as i64,
        v.y.to_bits() as i64,
        v.z.to_bits() as i64,
        i64::from(entity.y_rot.to_bits() as i32),
        i64::from(pitch.to_bits() as i32),
    ]
}
