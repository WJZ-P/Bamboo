//! 钓鱼逐刻对拍：五个场景覆盖抛竿飞行落水、完整咬钩与战利品/经验球/耐久/统计结算、
//! 钩住牛并拉拽（实体事件 31 与 5 点耐久）、以及露天与非露天水域 5×4×5 判定和宝藏掉落差异。
//! 附带三处改坏确认：`fishing_gravity`、`fishing_open_water`、`fishing_pull`（均改生产代码）。
//!
//! Tick-by-tick fishing oracle: five scenarios covering cast flight & water landing,
//! full bite/retrieve/loot/XP/durability/stat flow, hooking & pulling a cow (entity event 31
//! and 5 durability), and open-water vs enclosed 5×4×5 treasure eligibility, with three
//! production-code break-self-tests.

use mc_bamboo_blocks::{
    BlockState, BlockTicks, Effect, LevelRandom, World,
    entity::{
        Entity, EntityKind,
        ctx::{PlayerCombat, PlayerView, TargetEffects},
        fishing_hook::{
            FishHookState, FishingHookCast, caught_item_velocity, set_oracle_break_gravity,
            set_oracle_break_open_water, set_oracle_break_pull, should_stop_fishing,
        },
        mob::MobType,
    },
};
use mc_bamboo_core::{BlockPos, Vec3};
use mc_bamboo_data::{blocks, items, registries::entity_type};
use mc_bamboo_loot::{EntityView, LevelView, LootParams, LootStack, LootTable, Position};
use serde::Deserialize;

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FishingCase {
    pub(super) name: String,
    pub(super) first_id: i32,
    pub(super) player_id: i32,
    pub(super) player_pos: [f64; 3],
    pub(super) player_yaw: f32,
    pub(super) player_pitch: f32,
    pub(super) lure_level: i32,
    pub(super) luck_level: i32,
    pub(super) cow_pos: Option<[f64; 3]>,
    pub(super) blocks: Vec<[i32; 4]>,
    /// 每刻：`[x,y,z,dx,dy,dz,yRot,xRot,state,onGround,inWater,removed,hookedData,biting,openWater,nibble,timeUntilLured,timeUntilHooked,hookSeed,levelSeed,particleCount]`。
    pub(super) frames: Vec<[i64; 21]>,
    /// 每刻发出的粒子类型 ID 列表（按顺序）。
    pub(super) frame_particles: Vec<Vec<i32>>,
    /// 收竿后校验。
    pub(super) retrieve: Option<FishingRetrieveExpected>,
    /// 露天 / 非露天宝藏对照：在相同序列上连续抽取 30 次 `gameplay/fishing` 的 `[itemId, count]`。
    pub(super) extra_loot: Vec<[i32; 2]>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FishingRetrieveExpected {
    pub(super) rod_damage: i32,
    pub(super) entity_event_31: bool,
    pub(super) cow_delta: Option<[i64; 3]>,
    /// `[itemId, count, vxBits, vyBits, vzBits]`。
    pub(super) caught_items: Vec<[i64; 5]>,
    /// `[xpValue, xBits, yBits, zBits]`。
    pub(super) xp_orbs: Vec<[i64; 4]>,
    pub(super) fish_caught: i32,
    pub(super) level_seed: i64,
}

/// 宝藏表里独有的 6 种物品（`gameplay/fishing/treasure`）；
/// 用于确认露天场景确实抽出过宝藏、非露天场景 0 次宝藏。
fn is_treasure_only_item(item_id: i32) -> bool {
    matches!(
        item_id,
        items::NAME_TAG
            | items::SADDLE
            | items::BOW
            | items::BOOK
            | items::ENCHANTED_BOOK
            | items::NAUTILUS_SHELL
    )
}

struct OracleFishingLevelView<'a> {
    level: &'a MobLevel,
}

impl LevelView for OracleFishingLevelView<'_> {
    fn is_loaded(&self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        Some(true)
    }

    fn block_state(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        Some(
            self.level
                .blocks
                .get(&BlockPos::new(x, y, z))
                .copied()
                .unwrap_or(BlockState(blocks::AIR)),
        )
    }

    fn biome(&self, _x: i32, _y: i32, _z: i32) -> Option<i32> {
        Some(0)
    }

    fn can_see_sky(&self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        Some(true)
    }

    fn is_raining(&self) -> Option<bool> {
        Some(false)
    }

    fn is_thundering(&self) -> Option<bool> {
        Some(false)
    }

    fn day_time(&self) -> Option<i64> {
        None
    }

    fn game_time(&self) -> Option<i64> {
        None
    }

    fn dimension(&self) -> Option<&'static str> {
        Some("minecraft:overworld")
    }
}

impl FishingCase {
    fn level(&self) -> MobLevel {
        let mut map = HashMap::new();
        for &[x, y, z, state] in &self.blocks {
            map.insert(BlockPos::new(x, y, z), BlockState(state as u32));
        }
        let pos = Vec3::new(self.player_pos[0], self.player_pos[1], self.player_pos[2]);
        let player_view = PlayerView {
            uid: self.player_id,
            uuid: Some([0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 9]),
            position: pos,
            delta: Vec3::default(),
            width: 0.6,
            height: 1.8,
            eye_height: 1.62,
            health: 20.0,
            look_direction: Vec3::new(0.0, 0.0, 1.0),
            gaze_disguise: false,
            alive: true,
            spectator: false,
            creative: false,
            flying: false,
            explosion_knockback_resistance: 0.0,
            game_mode: 0,
            mining_fatigue: None,
            sneaking: false,
            sprinting: false,
            swimming: false,
            invisible: false,
            armor_cover: 0.0,
            main_hand: items::FISHING_ROD,
            off_hand: 0,
            team: None,
            yaw: self.player_yaw,
            pitch: self.player_pitch,
            combat: PlayerCombat::default(),
            effects: TargetEffects::default(),
            shoulder_left: None,
            shoulder_right: None,
        };
        MobLevel {
            blocks: map,
            random: LevelRandom::new(1),
            loot_sequences: mc_bamboo_loot::RandomSequences::new(0),
            looting: Vec::new(),
            rules: mc_bamboo_blocks::BlockGameRules::vanilla(),
            facts: ZombieFacts {
                difficulty: 2,
                can_see_sky: true,
                ..ZombieFacts::default()
            },
            natural_spawn: None,
            biome: None,
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
            entities: Entities::with_next_id(self.first_id),
            players: vec![player_view],
            game: None,
            wrong_was_hurt: false,
            inhabited: HashMap::new(),
            mob_teams: HashMap::new(),
            short_hunger: false,
            ignore_bed_monsters: false,
            explosion_break: 0,
            containers: Default::default(),
            container_open_counts: Default::default(),
            shoulder: mc_bamboo_blocks::entity::shoulder::ShoulderState::empty(),
            raid: None,
        }
    }
}

impl Report {
    pub(super) fn replay_fishing(&mut self, case: &FishingCase) {
        self.cases += 1;
        set_oracle_break_gravity(self.break_test.as_deref() == Some("fishing_gravity"));
        set_oracle_break_open_water(self.break_test.as_deref() == Some("fishing_open_water"));
        set_oracle_break_pull(self.break_test.as_deref() == Some("fishing_pull"));

        let mut level = case.level();
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);

        let cow_id = if let Some([cx, cy, cz]) = case.cow_pos {
            let mut world = World::new(&mut level, &mut ticks);
            let mut cow = Entity::by_mob_type(MobType::Cow, Vec3::new(cx, cy, cz));
            if let Some(mob) = cow.kind.mob_mut() {
                mob.no_ai = true;
            }
            Some(world.spawn_entity(cow))
        } else {
            None
        };

        // Tick 0: 抛竿（先掷 `FISHING_BOBBER_THROW` 音高随机数，再生成浮漂）。
        let owner_pos = Vec3::new(case.player_pos[0], case.player_pos[1], case.player_pos[2]);
        let _throw_pitch = 0.4_f32 / (level.random.source().next_float() * 0.4_f32 + 0.8_f32);
        let cast = FishingHookCast {
            owner: case.player_id,
            owner_uuid: [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 9],
            owner_position: owner_pos,
            owner_eye_y: owner_pos.y + f64::from(1.62_f32),
            owner_yaw: case.player_yaw,
            owner_pitch: case.player_pitch,
            luck: case.luck_level,
            lure_speed: case.lure_level * 100,
        };
        let hook_id = {
            let mut world = World::new(&mut level, &mut ticks);
            world.spawn_fishing_hook_cast(&cast)
        };
        let _ = level.entities.take_spawn_snapshots();
        let mut last_hook: Option<Entity> = None;

        for (tick, expected) in case.frames.iter().enumerate() {
            let mut tick_particles: Vec<i32> = Vec::new();
            if tick > 0 {
                ticks.set_time(1000 + tick as i64);
                let mut world = World::new(&mut level, &mut ticks);
                if let Some(kept) = world.tick_one_entity_kept(hook_id) {
                    last_hook = Some(kept);
                }
                for effect in world.drain_effects() {
                    if let Effect::Particles { particle, .. } = effect {
                        tick_particles.push(particle);
                    }
                }
            }
            let entity = level.entities.get(hook_id).or(last_hook.clone());
            let ours = snapshot_fishing_frame(
                entity.as_ref(),
                level.random.source().seed(),
                tick_particles.len(),
            );
            if &ours != expected {
                self.mismatch(format!(
                    "{} tick {tick}: official {expected:?}, bamboo {ours:?}",
                    case.name
                ));
            }
            if let Some(expected_particles) = case.frame_particles.get(tick)
                && &tick_particles != expected_particles
            {
                self.mismatch(format!(
                    "{} tick {tick} particles: official {expected_particles:?}, bamboo {tick_particles:?}",
                    case.name
                ));
            }
            self.steps += 1;
        }

        // 收竿结算比较。
        if let Some(expected) = &case.retrieve {
            let Some(mut hook_entity) = level.entities.take(hook_id) else {
                self.mismatch(format!(
                    "{} retrieve: hook missing before retrieve",
                    case.name
                ));
                set_oracle_break_gravity(false);
                set_oracle_break_open_water(false);
                set_oracle_break_pull(false);
                return;
            };
            let owner_view = level.players[0];
            let mut dmg = 0;
            let mut event_31 = false;
            let mut caught_items: Vec<[i64; 5]> = Vec::new();
            let mut xp_orbs: Vec<[i64; 4]> = Vec::new();
            let mut fish_caught = 0;
            let mut hook_open_water = true;
            let mut hook_pos = hook_entity.position;
            let mut hook_luck = 0;

            if !hook_entity.removed && !should_stop_fishing(hook_entity.position, &owner_view) {
                hook_pos = hook_entity.position;
                let on_ground = hook_entity.on_ground;
                let (hooked_in, nibble, open_water, luck) = match &hook_entity.kind {
                    EntityKind::FishingHook(hook) => {
                        (hook.hooked_in, hook.nibble, hook.open_water, hook.luck)
                    }
                    _ => (None, 0, false, 0),
                };
                hook_open_water = open_water;
                hook_luck = luck;

                if let Some(target_uid) = hooked_in {
                    {
                        let mut world = World::new(&mut level, &mut ticks);
                        world.pull_hooked_entity(hook_pos, owner_pos, target_uid);
                    }
                    event_31 = true;
                    dmg = 5;
                } else if nibble > 0 {
                    let stacks = roll_fishing_loot(&level, hook_pos, open_water, luck as f32);
                    let vel = caught_item_velocity(owner_pos, hook_pos);
                    let orb_pos = Vec3::new(owner_pos.x, owner_pos.y + 0.5, owner_pos.z + 0.5);
                    for stack in stacks {
                        caught_items.push([
                            i64::from(stack.item),
                            i64::from(stack.count),
                            vel.x.to_bits() as i64,
                            vel.y.to_bits() as i64,
                            vel.z.to_bits() as i64,
                        ]);
                        let xp = hook_entity.random.next_int_bounded(6) + 1;
                        xp_orbs.push([
                            i64::from(xp),
                            orb_pos.x.to_bits() as i64,
                            orb_pos.y.to_bits() as i64,
                            orb_pos.z.to_bits() as i64,
                        ]);
                        if mc_bamboo_data::item_tag("minecraft:fishes")
                            .is_some_and(|ids| ids.contains(&stack.item))
                        {
                            fish_caught += 1;
                        }
                    }
                    dmg = 1;
                }
                if on_ground {
                    dmg = 2;
                }
            }
            // `FishingRodItem.use` 收竿音高取一次 `level.random.nextFloat()`。
            let _retrieve_pitch =
                0.4_f32 / (level.random.source().next_float() * 0.4_f32 + 0.8_f32);
            let cow_delta = cow_id.and_then(|cid| {
                level.entities.get(cid).map(|cow| {
                    [
                        cow.delta.x.to_bits() as i64,
                        cow.delta.y.to_bits() as i64,
                        cow.delta.z.to_bits() as i64,
                    ]
                })
            });
            let after_seed = level.random.source().seed();

            if dmg != expected.rod_damage {
                self.mismatch(format!(
                    "{} retrieve rodDamage: official {}, bamboo {dmg}",
                    case.name, expected.rod_damage
                ));
            }
            if event_31 != expected.entity_event_31 {
                self.mismatch(format!(
                    "{} retrieve event31: official {}, bamboo {event_31}",
                    case.name, expected.entity_event_31
                ));
            }
            if cow_delta != expected.cow_delta {
                self.mismatch(format!(
                    "{} retrieve cowDelta: official {:?}, bamboo {:?}",
                    case.name, expected.cow_delta, cow_delta
                ));
            }
            if caught_items != expected.caught_items {
                self.mismatch(format!(
                    "{} retrieve caughtItems: official {:?}, bamboo {:?}",
                    case.name, expected.caught_items, caught_items
                ));
            }
            if xp_orbs != expected.xp_orbs {
                self.mismatch(format!(
                    "{} retrieve xpOrbs: official {:?}, bamboo {:?}",
                    case.name, expected.xp_orbs, xp_orbs
                ));
            }
            if fish_caught != expected.fish_caught {
                self.mismatch(format!(
                    "{} retrieve fishCaught: official {}, bamboo {fish_caught}",
                    case.name, expected.fish_caught
                ));
            }
            if after_seed != expected.level_seed {
                self.mismatch(format!(
                    "{} retrieve levelSeed: official {}, bamboo {after_seed}",
                    case.name, expected.level_seed
                ));
            }

            // 露天 / 非露天对照：连续抽 30 次战利品表并比宝藏是否出现。
            if !case.extra_loot.is_empty() {
                let mut ours_extra: Vec<[i32; 2]> = Vec::new();
                for _ in 0..case.extra_loot.len() {
                    let stacks =
                        roll_fishing_loot(&level, hook_pos, hook_open_water, hook_luck as f32);
                    if let Some(first) = stacks.first() {
                        ours_extra.push([first.item, first.count]);
                    } else {
                        ours_extra.push([0, 0]);
                    }
                }
                if ours_extra != case.extra_loot {
                    self.mismatch(format!(
                        "{} extraLoot: official {:?}, bamboo {:?}",
                        case.name, case.extra_loot, ours_extra
                    ));
                }
                let treasure_count = ours_extra
                    .iter()
                    .filter(|[id, _]| is_treasure_only_item(*id))
                    .count();
                if case.name == "fishing_open_water_treasure" && treasure_count == 0 {
                    self.mismatch(format!(
                        "{} expected at least one treasure in open water, got 0",
                        case.name
                    ));
                }
                if case.name == "fishing_enclosed_no_treasure" && treasure_count > 0 {
                    self.mismatch(format!(
                        "{} expected 0 treasure in enclosed water, got {treasure_count}",
                        case.name
                    ));
                }
            }
            self.steps += 1;
        }

        set_oracle_break_gravity(false);
        set_oracle_break_open_water(false);
        set_oracle_break_pull(false);
        println!("{}: {} ticks", case.name, case.frames.len());
    }
}

fn roll_fishing_loot(
    level: &MobLevel,
    hook_pos: Vec3,
    open_water: bool,
    luck: f32,
) -> Vec<LootStack> {
    let table = LootTable::from_name("minecraft:gameplay/fishing").expect("gameplay/fishing");
    let origin = Position::new(hook_pos.x, hook_pos.y, hook_pos.z);
    let view = OracleFishingLevelView { level };
    let mut hook_view = EntityView::new(entity_type::FISHING_BOBBER);
    hook_view.is_fishing_hook = true;
    hook_view.in_open_water = open_water;
    hook_view.position = Some(origin);
    let mut params = LootParams::new(luck).with_origin(origin);
    params.level = Some(&view);
    params.tool = Some(LootStack::new(items::FISHING_ROD, 1));
    params.this_entity = Some(hook_view);
    table
        .get_random_items_with_seed(
            &params,
            0,
            Some(&level.loot_sequences),
            level.random.source(),
        )
        .expect("evaluate gameplay/fishing")
}

fn snapshot_fishing_frame(
    entity: Option<&Entity>,
    level_seed: i64,
    particle_count: usize,
) -> [i64; 21] {
    let Some(entity) = entity else {
        return [0; 21];
    };
    let (pitch, state, hooked_data, biting, open_water, nibble, lured, hooked) = match &entity.kind
    {
        EntityKind::FishingHook(hook) => (
            hook.pitch,
            match hook.state {
                FishHookState::Flying => 0_i64,
                FishHookState::HookedInEntity => 1_i64,
                FishHookState::Bobbing => 2_i64,
            },
            i64::from(hook.hooked_in.map_or(0, |id| id + 1)),
            i64::from(hook.biting),
            i64::from(hook.open_water),
            i64::from(hook.nibble),
            i64::from(hook.time_until_lured),
            i64::from(hook.time_until_hooked),
        ),
        _ => (0.0, -1, 0, 0, 0, 0, 0, 0),
    };
    [
        entity.position.x.to_bits() as i64,
        entity.position.y.to_bits() as i64,
        entity.position.z.to_bits() as i64,
        entity.delta.x.to_bits() as i64,
        entity.delta.y.to_bits() as i64,
        entity.delta.z.to_bits() as i64,
        i64::from(entity.y_rot.to_bits() as i32),
        i64::from(pitch.to_bits() as i32),
        state,
        i64::from(entity.on_ground),
        i64::from(entity.in_water),
        i64::from(entity.removed),
        hooked_data,
        biting,
        open_water,
        nibble,
        lured,
        hooked,
        entity.random.state(),
        level_seed,
        particle_count as i64,
    ]
}
