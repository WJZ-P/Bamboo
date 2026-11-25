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
    /// 掠夺者场景：世界随机源的状态（装填完成音效的音高从它取）。
    ///
    /// Pillager scenes: the level random state (the loading-end pitch draws from it).
    level_seed: Option<i64>,
    /// 掠夺者场景：弩里的箭数、耐久损耗、已用刻数、`noActionTime`。
    ///
    /// Pillager scenes: loaded arrows, crossbow damage, ticks used and `noActionTime`.
    crossbow: Option<[i64; 4]>,
    /// 本刻的全部音效：音效 ID、声源序号、坐标原始位、音量位、音高位。
    ///
    /// All sounds this tick: id, source ordinal, raw x/y/z, volume, pitch.
    sounds: Option<Vec<[i64; 7]>>,
    /// 忠诚返回逐刻状态：dealtDamage / noPhysics / 返回计数 / inGroundTime。
    /// 三叉戟已被丢弃时是 `null`。
    ///
    /// Per-tick loyalty state: dealtDamage / noPhysics / return count /
    /// inGroundTime; `null` once the trident has been removed.
    trident: Option<Option<[i64; 4]>>,
    /// 这一刻新掉出来的物品实体：物品 ID、数量、x/y/z 坐标原始位。
    ///
    /// Item entities dropped this tick: item id, count and raw x/y/z bits.
    drops: Option<Vec<[i64; 5]>>,
    /// 场景主人逐刻状态：`[是否存活, 是否旁观]`。
    ///
    /// The scene owner's per-tick state: `[alive, spectator]`.
    owner: Option<[i64; 2]>,
    /// 引雷召唤的闪电：x/y/z 坐标原始位、`cause` 玩家 ID（无则 -1）。
    ///
    /// Channeling-summoned bolts: raw x/y/z bits and the cause player id
    /// (-1 when none).
    lightning: Option<Vec<[i64; 4]>>,
    /// 引雷受害者（牛）的血量位（仅 `post_attack` 场景有）。
    ///
    /// The channeling victim (cow) health bits (only post_attack scenes).
    victim: Option<i64>,
    /// 溺尸场景：怪物三叉戟的 pickup 枚举序号（DISALLOWED=0，玩家捡不到）。三叉戟被移除时为 -1。
    ///
    /// Drowned scenes: the mob trident's pickup ordinal (DISALLOWED=0, player cannot
    /// pick it up); -1 once the trident has been removed.
    pickup: Option<i32>,
    /// 激流逐刻：`[自旋剩余刻数, 生物标志位字节]`（标志位含使用物品位 0/1 与自旋位 4）。
    ///
    /// Riptide tick state: `[spin ticks left, living flags byte]` (the byte has
    /// the using-item bits 0/1 and the spin-attack bit 4).
    spin: Option<[i64; 2]>,
    /// 激流逐刻：`ITEM_USED` 累计值 `[物品 ID, 值]`。
    ///
    /// Riptide cumulative `ITEM_USED` stats as `[item id, value]`.
    item_used: Option<Vec<[i64; 2]>>,
    /// 激流自旋攻击受害者（牛）的血量位。
    ///
    /// The riptide spin-attack victim (cow) health bits.
    spin_victim: Option<i64>,
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
        if self.name.starts_with("pillager_") {
            MobType::Pillager
        } else if self.name.starts_with("stray_") {
            MobType::Stray
        } else if self.name.starts_with("bogged_") {
            MobType::Bogged
        } else if self.name.starts_with("parched_") {
            MobType::Parched
        } else {
            MobType::Skeleton
        }
    }

    /// 射手主手的武器：骷髅一支是弓，掠夺者是弩（多重射击场景带多重射击和穿透）。
    ///
    /// The shooter's weapon: a bow for the skeleton family, a crossbow for pillagers
    /// (with Multishot and Piercing in the multishot scene).
    fn shooter_weapon(&self) -> ItemStack {
        use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};
        if self.shooter_kind() != MobType::Pillager {
            return ItemStack::new(mc_bamboo_data::items::BOW, 1);
        }
        let crossbow = ItemStack::new(mc_bamboo_data::items::CROSSBOW, 1);
        if self.name != "pillager_crossbow_multishot" {
            return crossbow;
        }
        let registry = mc_bamboo_data::registries::SYNCED_REGISTRIES
            .iter()
            .find(|registry| registry.name == "minecraft:enchantment")
            .unwrap();
        let levels = ["minecraft:multishot", "minecraft:piercing"]
            .into_iter()
            .map(|name| {
                let id = registry.entries.iter().position(|entry| *entry == name);
                (id.unwrap() as i32, 1)
            })
            .collect();
        let mut patch = ComponentPatch::new();
        patch.put(
            mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
            PatchEntry::Set(ComponentValue::Enchantments(levels)),
        );
        let (added, removed) = patch.to_opaque();
        ItemStack {
            added,
            removed,
            ..crossbow
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

    /// 三叉戟逐刻场景：飞行、插地、忠诚返回、引雷、溺尸远程。
    ///
    /// Trident tick-by-tick scenes: flight, sticking, loyalty return,
    /// channeling and the drowned ranged attack.
    fn trident(&self) -> bool {
        self.name.starts_with("trident_")
    }

    /// 忠诚等级（忠诚 / 主人死亡 / 旁观场景）。
    ///
    /// Loyalty level for the return / dead-owner / spectator scenes.
    fn loyalty(&self) -> u8 {
        if self.name.starts_with("trident_loyalty")
            || self.name.starts_with("trident_owner")
            || self.name.starts_with("trident_spectator")
        {
            3
        } else {
            0
        }
    }

    /// 场上预置的 `dealtDamage`：忠诚三叉戟从「已命中」态开始返回。
    ///
    /// A pre-set `dealtDamage`: a returning loyal trident starts post-hit.
    fn dealt_damage(&self) -> bool {
        self.name == "trident_loyalty"
            || self.name.starts_with("trident_owner")
            || self.name.starts_with("trident_spectator")
    }

    /// 主人死亡：`LivingEntity.isAlive()` 为假，忠诚三叉戟判定主人不可接受并掉落。
    ///
    /// Dead owner: `LivingEntity.isAlive()` is false, so a loyal trident drops.
    fn owner_dead(&self) -> bool {
        self.name == "trident_owner_dead"
    }

    /// 主人旁观：`isAcceptibleReturnOwner()` 为假，忠诚三叉戟也掉落。
    ///
    /// Spectator owner: `isAcceptibleReturnOwner()` is false, so it drops too.
    fn owner_spectator(&self) -> bool {
        self.name == "trident_spectator"
    }

    /// 主人下线：`getOwner()` 解析成 null，tick 外层短路，不掉落也不返回。
    ///
    /// Offline owner: `getOwner()` is null and the outer guard short-circuits,
    /// so the trident neither drops nor returns.
    fn owner_offline(&self) -> bool {
        self.name == "trident_owner_offline"
    }

    /// 引雷场景雷雨 + 天空可见（避雷针场景还要求避雷针，由方块表给）。
    ///
    /// Channeling scenes run in a thunderstorm with open sky (the rod scene
    /// also needs a lightning rod, supplied by the block table).
    fn thundering(&self) -> bool {
        self.name.starts_with("trident_channeling")
    }

    fn open_sky(&self) -> bool {
        self.name.starts_with("trident_channeling")
    }

    fn channeling(&self) -> bool {
        self.name.starts_with("trident_channeling")
    }

    fn channeling_mob(&self) -> bool {
        self.name == "trident_channeling_mob" || self.name == "trident_channeling_covered"
    }

    /// 受害者头顶被盖住：`can_see_sky` 只有牛脚下这个坐标看不见天，逐刻确认
    /// `channeling_strike` 取的正是 `BlockPos::containing(victim)`。
    ///
    /// Covered victim: `can_see_sky` is false only at the cow's feet, tick by
    /// tick confirming `channeling_strike` samples `BlockPos::containing(victim)`.
    fn covered_sky(&self) -> bool {
        self.name == "trident_channeling_covered"
    }

    /// 溺尸远程投掷（普通 / 困难）：怪物三叉戟 pickup=Disallowed，玩家捡不到。
    ///
    /// Drowned ranged throw (normal / hard): the mob trident has pickup=Disallowed, so
    /// the player cannot pick it up.
    fn drowned(&self) -> bool {
        self.name.starts_with("trident_drowned")
    }

    /// 激流 / 自旋逐刻场景（没有投射物，只有玩家动作与包）。
    ///
    /// Riptide / auto-spin tick scenes (no projectile, only player actions
    /// and packets).
    fn riptide(&self) -> bool {
        self.name.starts_with("trident_riptide")
    }

    /// 不放牛：自旋计时自然走到 0，覆盖清标志位的那一帧。
    ///
    /// No cow: the spin timer runs out naturally, covering the flag-clear frame.
    fn riptide_no_hit(&self) -> bool {
        self.name.ends_with("_no_hit")
    }

    fn level(&self) -> MobLevel {
        MobLevel {
            natural_spawn: None,
            biome: None,
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
                raining: self.thundering(),
                thundering: self.thundering(),
                can_see_sky: self.open_sky(),
                covered_sky_pos: self.covered_sky().then(|| BlockPos::new(6, 64, 0)),
                ..ZombieFacts::default()
            },
            redstone: mc_bamboo_blocks::RedstoneMemory::default(),
            block_entities: BlockEntities::default(),
            block_events: BlockEvents::default(),
            game_event_listeners: Default::default(),
            warden_warnings: Default::default(),
            darkness_ticks: 0,
            poi: mc_bamboo_world::poi::PoiStore::new(),
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
                    creative: false,
                }],
            )),
            inhabited: Default::default(),
            mob_teams: Default::default(),
            ignore_bed_monsters: false,
            short_hunger: false,
            wrong_was_hurt: false,
            explosion_break: 0,
            shoulder: Default::default(),
        }
    }

    fn spawn(
        &self,
        world: &mut World<'_>,
        mutate: bool,
        break_pierce: bool,
        break_return: bool,
        break_drowned: bool,
        break_parched: bool,
    ) {
        if self.skeleton {
            // 改坏确认：把焦骸换成普通骷髅，箭上的 600 刻虚弱立刻消失。
            // Breakage check: replace the parched with a plain skeleton so its
            // 600-tick weakness arrow disappears.
            let shooter_kind = if break_parched && self.shooter_kind() == MobType::Parched {
                MobType::Skeleton
            } else {
                self.shooter_kind()
            };
            let mut skeleton =
                world.prepare_entity(Entity::by_mob_type(shooter_kind, Vec3::new(0.5, 64.0, 0.5)));
            skeleton.random = JavaRandom::new(42);
            skeleton.y_rot = 0.0;
            let mob = skeleton.kind.mob_mut().unwrap();
            mob.uuid = 1_u128.to_be_bytes();
            mob.y_head_rot = 0.0;
            mob.y_body_rot = 0.0;
            if !mutate {
                mob.equipment
                    .set(EquipmentSlot::MainHand, self.shooter_weapon());
            }
            world.add_prepared_entity(skeleton);
        } else if self.drowned() {
            // 溺尸远程投掷：用生产 `perform_ranged_attack` 共用的 `aimed_trident_shot`
            // 构造怪物三叉戟，与官方 `OracleDrowned.performRangedAttack` 的初态对齐
            // （射手 (0.5,64,0.5)、eye_y=64+1.74f、目标=场景玩家 (6.5,64,0.5)、
            // getY(1/3)、dy+horizontal*0.2、power 1.6、uncertainty=14-4*difficulty、
            // pickup=Disallowed）。官方把三叉戟放进 `world.spawned` 后按 FIRST_ID 收进
            // arrows；这里 `spawn_trident_shot` 走生产 prepare/shoot 路径，作为实体表里
            // 第一个 prepare 的实体，随机源种子与官方 `entitySeeds.nextLong()` 一致。
            // owner=FIRST_ID+1 与官方溺尸（getId()=FIRST_ID+1）一致 → 生成包
            // data=owner id，也让 `me_owner` 能解析出真实生物以触发难度缩放。
            //
            // Drowned ranged throw: build the mob trident with the same
            // `aimed_trident_shot` the production goal uses, matching the official
            // `OracleDrowned.performRangedAttack` initial state. As the first
            // prepared entity its random seed lines up with the official
            // `entitySeeds.nextLong()`; owner=FIRST_ID+1 makes the add-entity
            // packet's data (owner id) match the official phantom shooter and lets
            // `me_owner` resolve a real Mob for difficulty damage scaling.
            use mc_bamboo_blocks::entity::mob::drowned::aimed_trident_shot;
            let shooter = Entity::by_mob_type(MobType::Drowned, Vec3::new(0.5, 64.0, 0.5));
            let eye_y = shooter.position.y + f64::from(shooter.kind.mob().unwrap().eye_height);
            // 改坏确认 trident_drowned：把难度拨错一档，uncertainty / 散布全变，
            // 逐刻初速与后续位置必须报不一致。
            // Break self-test: shift the difficulty so uncertainty / spread change;
            // the per-tick initial velocity and downstream motion must diverge.
            let difficulty = if break_drowned {
                self.difficulty + 1
            } else {
                self.difficulty
            };
            let shot = aimed_trident_shot(
                Vec3::new(0.5, 64.0, 0.5),
                eye_y,
                Vec3::new(self.player_x, 64.0, 0.5),
                1.8_f32,
                difficulty,
                ItemStack::new(mc_bamboo_data::items::TRIDENT, 1),
                // 主人是真实溺尸（官方 getOwner() 解析成非玩家 LivingEntity），
                // id 与 Java `setId(FIRST_ID + 1)` 对齐；生成包的 data 也取这个值。
                // The owner is a real drowned (vanilla getOwner() resolves to a
                // non-player LivingEntity); the id matches Java's setId and is
                // also the add-entity packet's data.
                self.first_id + 1,
                1_u128.to_be_bytes(),
            );
            world.spawn_trident_shot(shot);
            // 官方幽灵溺尸虽不进 tracked/tick，但 `getOwner()` 是非玩家 LivingEntity，
            // 困难难度伤害按 `scalesWithDifficulty()` ×1.5。Bamboo 生产代码只有当
            // `me_owner` 能把主人解析成真实生物时才缩放（`scale_when_non_player`），
            // 这里补一只 no_ai、放在飞行路径外的溺尸作为主人，使难度缩放按官方生效。
            // 它是 Mob 生成快照，逐刻包只从 Arrow 快照构造，不会多出发包；no_ai
            // 且远离三叉戟，不会被当成命中目标或产生额外音效。
            //
            // Vanilla's phantom drowned is not tracked/ticked, but getOwner() is a
            // non-player LivingEntity, so HARD scales damage ×1.5. Bamboo's production
            // scales only when me_owner resolves a real Mob (scale_when_non_player),
            // so add an inert, off-path drowned as the owner. Mob spawn snapshots
            // never become oracle packets (only Arrow ones do); no_ai and far from
            // the trident, it is neither a hit target nor a source of extra sounds.
            let mut drowned = Entity::by_mob_type(MobType::Drowned, Vec3::new(0.5, 64.0, -8.0));
            drowned.with_mob(|_, mob| mob.no_ai = true);
            world.spawn_entity(drowned);
        } else if self.trident() {
            // 三叉戟逐刻对拍：从显式初态出发，和官方一样走 `shoot` 再逐刻
            // `AbstractArrow.tick`。扔出初态已由 `shot_trident` 单独对拍，这里只比
            // 飞行 / 插地 / 忠诚 / 引雷的逐刻行为。
            // Trident tick parity: start from an explicit state, `shoot` like the
            // official side, then tick `AbstractArrow.tick` frame by frame. The
            // throw's initial state is already covered by `shot_trident`; here we
            // compare only the per-tick flight / stick / loyalty / channeling.
            use mc_bamboo_blocks::entity::arrow::{Pickup, TridentShot};
            use mc_bamboo_protocol::components::{ComponentPatch, ComponentValue, PatchEntry};
            let loyalty = self.loyalty();
            // 改坏确认：忠诚等级驱动返回加速度（0.05*L），改掉它逐刻位置/速度
            // 与 ID_LOYALTY 元数据都应报不一致。
            // Break self-test: the loyalty level drives the return acceleration
            // (0.05*L); corrupting it must break the per-tick motion and metadata.
            let loyalty = if break_return && loyalty > 1 {
                loyalty - 1
            } else {
                loyalty
            };
            let mut item = ItemStack::new(mc_bamboo_data::items::TRIDENT, 1);
            let mut levels: Vec<(i32, i32)> = Vec::new();
            if loyalty > 0 || self.channeling() {
                let registry = mc_bamboo_data::registries::SYNCED_REGISTRIES
                    .iter()
                    .find(|registry| registry.name == "minecraft:enchantment")
                    .unwrap();
                if loyalty > 0 {
                    let id = registry
                        .entries
                        .iter()
                        .position(|entry| *entry == "minecraft:loyalty")
                        .unwrap() as i32;
                    levels.push((id, i32::from(loyalty)));
                }
                if self.channeling() {
                    // `channeling.json` 的 post_attack / hit_block 效果要物品堆带引雷。
                    // `channeling.json`'s post_attack / hit_block needs the enchantment.
                    let id = registry
                        .entries
                        .iter()
                        .position(|entry| *entry == "minecraft:channeling")
                        .unwrap() as i32;
                    levels.push((id, 1));
                }
                let mut patch = ComponentPatch::new();
                patch.put(
                    mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
                    PatchEntry::Set(ComponentValue::Enchantments(levels)),
                );
                let (added, removed) = patch.to_opaque();
                item.added = added;
                item.removed = removed;
            }
            let owner_uuid = ((1_u128 << 64) | 2).to_be_bytes();
            // 下线主人：物品堆仍带忠诚、dealtDamage 仍预置，但 `owner`/`owner_uuid`
            // 都为空，`trident_tick` 解析不到主人 → 外层短路，既不掉也不返回，
            // 走正常飞行；对应 Java `put(Projectile,"owner",null)`。
            // Offline owner: the stack still has loyalty and dealtDamage is set,
            // but both owner fields are null, so `trident_tick` resolves no
            // owner and short-circuits — normal flight, no drop, no return.
            // 引雷场景也要主人：闪电的 `cause` 是投掷者（ServerPlayer）。
            // Channeling also needs the owner: the bolt's `cause` is the thrower.
            let has_owner = (loyalty > 0 || self.channeling()) && !self.owner_offline();
            let shot = TridentShot {
                position: Vec3::new(0.5, 65.5, 0.5),
                direction: Vec3::new(1.0, 0.0, 0.0),
                power: 1.6,
                uncertainty: 0.0,
                source_movement: None,
                owner: has_owner.then_some(self.player_id),
                owner_uuid: has_owner.then_some(owner_uuid),
                item,
                pickup: if loyalty > 0 {
                    Pickup::Allowed
                } else {
                    Pickup::Disallowed
                },
            };
            let mut arrow = world.prepare_entity(Entity::configured_trident(&shot));
            if self.dealt_damage()
                && let EntityKind::Arrow(data) = &mut arrow.kind
            {
                data.dealt_damage = true;
            }
            arrow.shoot_arrow(Vec3::new(1.0, 0.0, 0.0), 1.6, 0.0);
            world.add_prepared_entity(arrow);
            if self.channeling_mob() {
                // 引雷 post_attack 的受害者：静止的牛，闪电落在它自己的位置。
                // The channeling post_attack victim: a still cow; the bolt lands
                // on its own position.
                let mut cow = Entity::by_mob_type(MobType::Cow, Vec3::new(6.5, 64.0, 0.5));
                cow.with_mob(|_, mob| mob.no_ai = true);
                world.spawn_entity(cow);
            }
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
    /// 激流逐刻对拍：玩家蓄力松手、自旋攻击撞牛、生物标志位 / `ITEM_USED`
    /// 同步包。官方一侧的 `playRiptide` 逐刻驱动同样的服务端效果。
    ///
    /// Riptide tick parity: charge and release, spin-attack contact damage,
    /// living flags / `ITEM_USED` sync packets. The official `playRiptide`
    /// drives the same server-side effects tick by tick.
    fn replay_riptide(&mut self, case: &ProjectileCase) {
        self.cases += 1;
        let mut level = case.level();
        {
            let game = level.game.as_mut().unwrap();
            game.set_raining(true);
            // `TridentItem.use` 的门槛也要水 / 雨，先注入再开始使用。
            // `TridentItem.use` also needs water / rain; inject it first.
            let mut item = ItemStack::new(mc_bamboo_data::items::TRIDENT, 1);
            let registry = mc_bamboo_data::registries::SYNCED_REGISTRIES
                .iter()
                .find(|registry| registry.name == "minecraft:enchantment")
                .unwrap();
            let id = registry
                .entries
                .iter()
                .position(|entry| *entry == "minecraft:riptide")
                .unwrap() as i32;
            // 改坏确认 `trident_riptide`：把激流等级改成 0，松手就变成普通投掷，
            // 自旋计时、标志位、音效和受害者血量都必须报不一致。
            // Break self-test `trident_riptide`: clear riptide so release
            // becomes an ordinary throw; spin ticks, flags, sounds and victim
            // health must all diverge.
            let level_value = if self.break_test.as_deref() == Some("trident_riptide") {
                0
            } else {
                1
            };
            let mut patch = mc_bamboo_protocol::components::ComponentPatch::new();
            patch.put(
                mc_bamboo_data::registries::data_component_type::ENCHANTMENTS,
                mc_bamboo_protocol::components::PatchEntry::Set(
                    mc_bamboo_protocol::components::ComponentValue::Enchantments(vec![(
                        id,
                        level_value,
                    )]),
                ),
            );
            let (added, removed) = patch.to_opaque();
            item.added = added;
            item.removed = removed;
            assert!(game.start_use(case.player_id, item));
            if !case.riptide_no_hit() {
                // 牛放在玩家身边，落在自旋扫掠盒里；`Player.attack` 的伤害
                // 走正式入口，官方一侧的牛在 (7.0,64,0.5)。
                // A cow beside the player, inside the spin sweep box.
                // `Player.attack` runs the production damage path.
                let mut cow = Entity::by_mob_type(MobType::Cow, Vec3::new(7.0, 64.0, 0.5));
                cow.with_mob(|_, mob| mob.no_ai = true);
                game.add_spin_target(cow);
            }
        }
        let mut victim_uid = None;
        // 自旋目标在区域实体表里是第一个实体，id 由 `Entities::spawn` 分配。
        // The spin target is the first entity in the region store.
        if !case.riptide_no_hit() {
            victim_uid = level.game.as_ref().unwrap().first_mob_uid();
        }
        for (tick, expected) in case.frames.iter().enumerate() {
            let mut sounds: Vec<[i64; 7]> = Vec::new();
            if tick > 0 {
                let game = level.game.as_mut().unwrap();
                if tick == 1 {
                    game.release(case.player_id, 10);
                } else {
                    game.spin_tick();
                }
                game.request_stats(case.player_id);
                // 自旋扫掠期间区域实体表的伤害落在世界实体上；这里把区域的
                // 生物表当作受害者，音效由 `send_effects` 直接发给玩家连接。
                // Damage lands in the region entity table; sounds are sent
                // straight to the player's capture sink.
                let effects = game.take_sound_effects();
                sounds.extend(effects.iter().filter_map(|effect| {
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
                    Some([
                        i64::from(*sound),
                        i64::from(source.id()),
                        x.to_bits() as i64,
                        y.to_bits() as i64,
                        z.to_bits() as i64,
                        i64::from(volume.to_bits()),
                        i64::from(pitch.to_bits()),
                    ])
                }));
            }
            let game = level.game.as_ref().unwrap();
            let label = format!("{} tick {tick}", case.name);
            if let Some(theirs) = expected.spin {
                let ours = game.spin_state(case.player_id);
                if ours != theirs {
                    self.mismatch(format!(
                        "{label} spin ticks/living flags: official {theirs:?}, bamboo {ours:?}"
                    ));
                }
            }
            if let Some(theirs) = &expected.item_used {
                // 官方的 `ServerStatsCounter` 里零值统计项也算一个条目；
                // Bamboo 的映射只在真正 award 过之后才有条目，tick0 补一个零值。
                // The official `ServerStatsCounter` holds a zero-valued entry;
                // Bamboo's map only has entries after an award, so add the
                // tick-0 zero baseline.
                let mut ours: Vec<[i64; 2]> = game
                    .item_used_stats(case.player_id)
                    .into_iter()
                    .map(|(item, value)| [i64::from(item), i64::from(value)])
                    .collect();
                if ours.is_empty() {
                    ours.push([i64::from(mc_bamboo_data::items::TRIDENT), 0]);
                }
                if *theirs != ours {
                    self.mismatch(format!(
                        "{label} ITEM_USED stats: official {theirs:?}, bamboo {ours:?}"
                    ));
                }
            }
            if let Some(theirs) = expected.spin_victim {
                let ours = victim_uid.and_then(|uid| game.spin_target_health(uid));
                if Some(theirs) != ours {
                    self.mismatch(format!(
                        "{label} spin victim health: official {theirs}, bamboo {ours:?}"
                    ));
                }
            }
            if let Some(theirs) = &expected.sounds
                && *theirs != sounds
            {
                self.mismatch(format!(
                    "{label} sounds: official {theirs:?}, bamboo {sounds:?}"
                ));
            }
            let game = level.game.as_mut().unwrap();
            let state = game.state(case.player_id);
            if state != case.player_states[tick] {
                self.mismatch(format!(
                    "{label} player: official {:?}, bamboo {state:?}",
                    case.player_states[tick]
                ));
            }
            // `startUsingItem` / 松手 / 自旋清标志这些当刻的脏元数据由
            // `sync_once` 在 tick0 与每刻末尾发出，和官方 `sendChanges`
            // 的时序一致。`award_stats` 的条目顺序是官方 HashMap 的哈希序，
            // 累计值由 `itemUsed` 字段精确比对，这里不比它的字节。
            // Dirty metadata from `startUsingItem`, release and the spin-flag
            // clear is sent by `sync_once` on tick 0 and at each tick end,
            // matching the official `sendChanges` timing. `award_stats`
            // entries follow the official HashMap order, so their cumulative
            // values are compared exactly by `itemUsed` instead of bytes.
            if tick <= 1 {
                // tick>=2 的 `spin_tick` 已经跑过一轮同步。
                // `spin_tick` on tick>=2 already ran a sync round.
                game.sync_once();
            }
            let player_packets: Vec<_> = game
                .take_packets(case.player_id)
                .into_iter()
                .filter(|body| matches!(body.first(), Some(101 | 43 | 67 | 68 | 69 | 106)))
                .collect();
            compare_packet_list(
                &mut self.mismatches,
                &self.show,
                &format!("{} player", case.name),
                tick,
                &player_packets,
                &case.player_packets[tick],
            );
            self.steps += 1;
        }
        let health = f32::from_bits(level.game.as_ref().unwrap().state(case.player_id)[0] as u32);
        println!(
            "{}: {} ticks, riptide spin parity, final player health {health}",
            case.name,
            case.frames.len()
        );
    }
}

impl Report {
    pub(super) fn replay_projectile(&mut self, case: &ProjectileCase) {
        if case.riptide() {
            self.replay_riptide(case);
            return;
        }
        self.cases += 1;
        let mut level = case.level();
        // 改坏确认 `trident_sky_sample`：把「头顶被盖住」的坐标挪开一格，
        // `channeling_strike` 取的采样点就会看见天、错误地召出闪电，逐刻的
        // lightning / sounds 必须报不一致。
        // Break self-test `trident_sky_sample`: move the covered coordinate
        // one block away. `channeling_strike`'s sample then sees the sky and
        // wrongly summons a bolt, so lightning / sounds must diverge.
        if self.break_test.as_deref() == Some("trident_sky_sample") && case.covered_sky() {
            level.facts.covered_sky_pos = Some(BlockPos::new(7, 64, 0));
        }
        // 改坏确认 `trident_channeling`：关掉露天条件，引雷就不该召唤闪电、
        // 也不该播雷声；逐刻的 lightning / sounds 必须报不一致。
        // Break self-test `trident_channeling`: turn off open sky so channeling
        // summons no bolt and plays no thunder; lightning / sounds must diverge.
        if self.break_test.as_deref() == Some("trident_channeling") {
            level.facts.can_see_sky = false;
        }
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
        // 主人不可接受的两条路径：死亡 / 旁观，逐刻行为和掉落物都要对上官方。
        // The two unacceptable-owner paths: dead / spectator; both the per-tick
        // behavior and the dropped item must match vanilla.
        // 改坏确认 `trident_owner`：故意不把主人设成死亡 / 旁观，三叉戟就会错误地
        // 飞回而不是掉落，逐刻状态、掉落物、主人状态和包都必须报不一致。
        // Break self-test `trident_owner`: leave the owner acceptable, so the
        // trident wrongly returns instead of dropping; state, drops, owner and
        // packets must all diverge.
        let break_owner = self.break_test.as_deref() == Some("trident_owner");
        if case.owner_dead() && !break_owner {
            level.game.as_mut().unwrap().set_dead(case.player_id);
        }
        if case.owner_spectator() && !break_owner {
            level.game.as_mut().unwrap().set_spectator(case.player_id);
        }
        let mut ticks = BlockTicks::default();
        ticks.set_time(1000);
        case.spawn(
            &mut World::new(&mut level, &mut ticks),
            self.break_test.as_deref() == Some("projectile"),
            self.break_test.as_deref() == Some("arrow_piercing"),
            self.break_test.as_deref() == Some("trident_return"),
            self.break_test.as_deref() == Some("trident_drowned"),
            self.break_test.as_deref() == Some("parched_arrow"),
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
            let mut sounds: Vec<[i64; 7]> = Vec::new();
            let mut drops: Vec<[i64; 5]> = Vec::new();
            let mut bolts: Vec<[i64; 4]> = Vec::new();
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
                sounds.extend(effects.iter().filter_map(|effect| {
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
                    Some([
                        i64::from(*sound),
                        i64::from(source.id()),
                        x.to_bits() as i64,
                        y.to_bits() as i64,
                        z.to_bits() as i64,
                        i64::from(volume.to_bits()),
                        i64::from(pitch.to_bits()),
                    ])
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
            // 官方 `FakeLevel` 把召唤的闪电放进 `world.spawned`、当刻就 `clear()`，
            // 既不 tick 也不跟踪；这里同样把闪电从回放世界移除，避免它后续的
            // 伤害 / 点燃 / 音效进入逐刻对拍（闪电实体自身的逐刻行为由
            // `blocks/entity/lightning.rs` 的单测覆盖）。召唤事件本身仍由
            // `lightning`（坐标 + cause）与 `sounds`（雷声）严格对拍。
            // The official `FakeLevel` puts summoned bolts into `world.spawned`
            // and clears them the same tick, never ticking or tracking them.
            // Remove them from the replay world here too, so their downstream
            // damage / ignition / sounds stay out of the tick parity (the bolt's
            // own tick behavior is covered by `lightning.rs` unit tests). The
            // summon event itself is still strictly compared via `lightning`
            // (position + cause) and `sounds` (thunder).
            if case.channeling() {
                let bolt_ids: Vec<_> = level
                    .entities
                    .ids()
                    .into_iter()
                    .filter(|&id| {
                        level
                            .entities
                            .public_at(id)
                            .is_some_and(|p| matches!(p.kind, KindPublic::LightningBolt(_)))
                    })
                    .collect();
                for id in bolt_ids {
                    level.entities.remove(id);
                }
            }
            // 这一刻新掉出来的物品实体：物品 ID、数量、生成坐标原始位。忠诚三叉戟
            // 主人不可接受时掉出的拾取物就在这里（带 0.1f 抬高后的 y）。
            // Item entities spawned this tick: item id, count, raw spawn position
            // bits. A loyal trident's drop for an unacceptable owner lands here
            // (with the 0.1f raised y).
            for public in &spawns {
                if let KindPublic::Item { stack, .. } = &public.kind {
                    drops.push([
                        i64::from(stack.item),
                        i64::from(stack.count),
                        public.position.x.to_bits() as i64,
                        public.position.y.to_bits() as i64,
                        public.position.z.to_bits() as i64,
                    ]);
                }
                if let KindPublic::LightningBolt(bolt) = &public.kind {
                    // 引雷闪电：坐标原始位 + `cause` 玩家 ID（无则 -1）。
                    // Channeling bolt: raw position bits + cause player id (-1 if none).
                    bolts.push([
                        public.position.x.to_bits() as i64,
                        public.position.y.to_bits() as i64,
                        public.position.z.to_bits() as i64,
                        bolt.cause.map_or(-1, i64::from),
                    ]);
                }
            }
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
                let mut ours = level.game.as_ref().unwrap().effects(case.player_id);
                if ours.is_empty() && !player_effects.is_empty() {
                    ours = player_effects.values().copied().collect();
                }
                if *theirs != ours {
                    self.mismatch(format!(
                        "{label} player effects: official {theirs:?}, bamboo {ours:?}"
                    ));
                }
            }
            if self.break_test.as_deref() == Some("trident_sound")
                && case.trident()
                && let Some(first) = sounds.first_mut()
            {
                // 改坏确认：把落地音效的 ID 改掉，逐刻对拍必须报不一致。
                // Break self-test: corrupt the land sound id so the tick parity
                // must report a mismatch.
                first[0] = -1;
            }
            if let Some(theirs) = &expected.sounds
                && *theirs != sounds
            {
                self.mismatch(format!(
                    "{label} sounds: official {theirs:?}, bamboo {sounds:?}"
                ));
            }
            if let Some(theirs) = expected.trident {
                let ours =
                    level
                        .entities
                        .get(case.first_id)
                        .and_then(|entity| match &entity.kind {
                            EntityKind::Arrow(data) => Some([
                                i64::from(data.dealt_damage),
                                i64::from(data.no_physics),
                                i64::from(data.client_side_return_trident_tick_count),
                                i64::from(data.in_ground_time),
                            ]),
                            _ => None,
                        });
                if theirs != ours {
                    self.mismatch(format!(
                        "{label} trident state: official {theirs:?}, bamboo {ours:?}"
                    ));
                }
            }
            if let Some(theirs) = expected.pickup {
                // 怪物三叉戟 pickup 恒为 Disallowed(0)；玩家碰到也捡不起来。三叉戟
                // 被丢弃 / 移除时官方给 -1。
                // A mob trident's pickup is always Disallowed(0); the official side
                // emits -1 once the trident has been removed.
                let ours = level
                    .entities
                    .get(case.first_id)
                    .and_then(|entity| match &entity.kind {
                        EntityKind::Arrow(data) => Some(data.pickup as i32),
                        _ => None,
                    })
                    .unwrap_or(-1);
                if theirs != ours {
                    self.mismatch(format!("{label} pickup: official {theirs}, bamboo {ours}"));
                }
            }
            if let Some(theirs) = &expected.drops
                && *theirs != drops
            {
                self.mismatch(format!(
                    "{label} drops: official {theirs:?}, bamboo {drops:?}"
                ));
            }
            if let Some(theirs) = expected.owner {
                let ours = level
                    .game
                    .as_ref()
                    .unwrap()
                    .views()
                    .iter()
                    .find(|view| view.uid == case.player_id)
                    .map(|view| [i64::from(view.alive), i64::from(view.spectator)]);
                if Some(theirs) != ours {
                    self.mismatch(format!(
                        "{label} owner: official {theirs:?}, bamboo {ours:?}"
                    ));
                }
            }
            if let Some(theirs) = &expected.lightning
                && *theirs != bolts
            {
                self.mismatch(format!(
                    "{label} lightning: official {theirs:?}, bamboo {bolts:?}"
                ));
            }
            if let Some(theirs) = expected.victim {
                // 受害牛在 `first_id + 1`（和 effect_target 场景一致）。
                // The victim cow is at `first_id + 1` (as in the effect_target scenes).
                let ours = level.entities.get(case.first_id + 1).and_then(|entity| {
                    entity
                        .kind
                        .mob()
                        .map(|mob| i64::from(mob.health.to_bits() as i32))
                });
                if Some(theirs) != ours {
                    self.mismatch(format!(
                        "{label} victim health: official {theirs}, bamboo {ours:?}"
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
                if let Some(theirs) = expected.crossbow {
                    use mc_bamboo_blocks::entity::mob::crossbow;
                    let held = mob.equipment.get(EquipmentSlot::MainHand);
                    let ours = [
                        crossbow::charged_count(held) as i64,
                        i64::from(mc_bamboo_blocks::durability::damage_of(held).unwrap_or(0)),
                        i64::from(crossbow::ticks_using(mob)),
                        i64::from(mob.no_action_time),
                    ];
                    if ours != theirs {
                        self.mismatch(format!(
                            "{label} crossbow loaded/damage/ticks used/noActionTime: official {theirs:?}, bamboo {ours:?}"
                        ));
                    }
                }
                if let Some(theirs) = expected.level_seed {
                    let ours = level.random.source().seed();
                    if ours != theirs {
                        self.mismatch(format!(
                            "{label} level random: official {theirs}, bamboo {ours}"
                        ));
                    }
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
            // `ServerPlayer.doTick` 在实体阶段之后按血量快照补 `set_health`。
            if tick != 0 {
                player.sync_health();
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
        MobType::Pillager => mob.crossbow.charging,
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
        player_height: 1.8,
        sneaking: false,
        difficulty: 2,
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
