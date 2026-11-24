//! 僵尸村民（官方 `ZombieVillager`）：构造、`finalizeSpawn`、金苹果治疗、
//! 每刻的转化进度和完成后的数据搬运。
//!
//! `VillagerData` / `Offers` / `Gossips` / `Xp` 复用 [`super::villager::Villager`]
//! 的字段；八卦和交易都还没求值，存档原文照搬。
//!
//! Zombie villagers (vanilla `ZombieVillager`): construction, `finalizeSpawn`, the
//! golden-apple cure, per-tick conversion progress and the state carried over at
//! completion. `VillagerData` / `Offers` / `Gossips` / `Xp` reuse the villager state;
//! gossips and offers are kept verbatim.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{items, registries::mob_effect, villager as facts};
use mc_bamboo_nbt::Tag;

use super::{
    Mob, MobEvent, MobType,
    effects::EffectInstance,
    interact::{MobInteractRequest, MobInteraction},
    living,
    spawn::{SpawnGroup, SpawnReason},
    villager::Villager,
};
use crate::{
    InteractionResult,
    archive::tag::{self, put},
    entity::{
        Entity,
        ctx::{BlockView, EntityCtx},
    },
    state::BlockState,
    world::World,
};

/// `VILLAGER_CONVERSION_WAIT_MIN` / `WAIT_MAX`：`nextInt(2401) + 3600`。
///
/// `VILLAGER_CONVERSION_WAIT_MIN` / `WAIT_MAX`: `nextInt(2401) + 3600`.
pub const CONVERSION_WAIT_MIN: i32 = 3600;
pub const CONVERSION_WAIT_SPAN: i32 = 2401;

/// `SPECIAL_BLOCK_RADIUS` 和 `MAX_SPECIAL_BLOCKS_COUNT`。
///
/// `SPECIAL_BLOCK_RADIUS` and `MAX_SPECIAL_BLOCKS_COUNT`.
pub const SPECIAL_BLOCK_RADIUS: i32 = 4;
pub const MAX_SPECIAL_BLOCKS_COUNT: i32 = 14;

/// 官方世界事件号：村民感染 1026、治疗完成 1027。
///
/// Official level events: villager infected 1026, cured 1027.
pub const ZOMBIE_INFECTED_EVENT: i32 = 1026;
pub const ZOMBIE_VILLAGER_CURED_EVENT: i32 = 1027;

/// 僵尸村民的治疗状态。`converting` 是 `DATA_CONVERTING_ID`，存档写 `ConversionTime`；
/// `ConversionPlayer` 留给八卦系统，现在只原样往返。
///
/// The zombie villager cure state. `converting` is `DATA_CONVERTING_ID`, saved as
/// `ConversionTime`; `ConversionPlayer` is kept for the future gossip system.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ZombieVillagerState {
    pub converting: bool,
    pub conversion_time: i32,
    /// `conversionStarter`：发起治疗的玩家 UUID。
    pub conversion_player: Option<[u8; 16]>,
}

impl ZombieVillagerState {
    /// `startConverting`：只记状态，效果与事件由调用方处理。
    ///
    /// `startConverting`: state only; the caller applies effects and the entity event.
    pub fn start(&mut self, player: Option<[u8; 16]>, time: i32) {
        self.converting = true;
        self.conversion_time = time;
        self.conversion_player = player;
    }

    /// `addAdditionalSaveData` 的六个僵尸村民键。
    ///
    /// The six zombie-villager keys of `addAdditionalSaveData`.
    pub fn write_nbt(&self, villager: &Villager, out: &mut Vec<(String, Tag)>) {
        put(out, "VillagerData", villager.data_tag());
        put(
            out,
            "VillagerDataFinalized",
            Tag::Byte(i8::from(villager.finalized)),
        );
        if let Some(offers) = &villager.offers {
            put(out, "Offers", offers.clone());
        }
        if let Some(gossips) = &villager.gossips {
            put(out, "Gossips", gossips.clone());
        }
        put(
            out,
            "ConversionTime",
            Tag::Int(if self.converting {
                self.conversion_time
            } else {
                -1
            }),
        );
        if let Some(player) = self.conversion_player {
            put(out, "ConversionPlayer", super::nbt::uuid_tag(&player));
        }
        put(out, "Xp", Tag::Int(villager.xp));
    }

    /// `readAdditionalSaveData`：`VillagerDataFinalized` 或 `VillagerData` 存在时
    /// finalized 为真；`ConversionTime != -1` 才进入转化状态。
    ///
    /// `readAdditionalSaveData`: either `VillagerDataFinalized` or a present
    /// `VillagerData` finalizes the data; only a non-`-1` `ConversionTime` converts.
    pub fn read_nbt(&mut self, villager: &mut Villager, source: &Tag) {
        let data = tag::field(source, "VillagerData");
        let finalized = tag::bool_or(source, "VillagerDataFinalized", false);
        if finalized || data.is_some() {
            villager.finalized = true;
            if let Some(data) = data {
                villager.read_data(data);
            } else {
                initialize_data(villager, 0);
            }
        }
        villager.offers = tag::field(source, "Offers").cloned();
        villager.gossips = tag::field(source, "Gossips").cloned();
        let time = tag::int_or(source, "ConversionTime", -1);
        if time == -1 {
            self.converting = false;
            self.conversion_time = -1;
        } else {
            self.start(super::nbt::uuid_field(source, "ConversionPlayer"), time);
        }
        villager.xp = tag::int_or(source, "Xp", 0);
        self.conversion_player =
            super::nbt::uuid_field(source, "ConversionPlayer").or(self.conversion_player.take());
    }

    /// `removeWhenFarAway`：转化中或有交易经验时不按距离消失。
    ///
    /// `removeWhenFarAway`: no distance despawn while converting or with villager XP.
    pub const fn removes_when_far_away(&self, xp: i32) -> bool {
        !self.converting && xp == 0
    }
}

/// `initializeZombieVillagerData(random)`：`createDefaultVillagerData()` 再按
/// `VILLAGER_PROFESSION` 注册表大小抽一个职业。读档缺 `VillagerData` 时也走这一支。
///
/// `initializeZombieVillagerData(random)`: the default villager data with one
/// uniformly random profession. Missing saved data takes the same path.
pub fn initialize_data(villager: &mut Villager, roll: i32) {
    villager.kind = facts::type_id(facts::DEFAULT_TYPE).unwrap_or(0);
    villager.profession = facts::profession_id(facts::NONE_PROFESSION).unwrap_or(0);
    villager.level = facts::MIN_VILLAGER_LEVEL;
    let count = facts::PROFESSIONS.len() as i32;
    if count > 0 {
        villager.profession = roll.rem_euclid(count);
    }
}

/// 构造期的随机职业取实体自己的随机源：`DefaultedRegistry.getRandom` 是
/// `nextInt(size)`。
///
/// The constructor draws the profession from the entity's own random:
/// `DefaultedRegistry.getRandom` is one `nextInt(size)`.
pub fn randomize_profession(villager: &mut Villager, random: &mut crate::entity::JavaRandom) {
    let count = facts::PROFESSIONS.len() as i32;
    let roll = if count > 0 {
        random.next_int_bounded(count)
    } else {
        0
    };
    initialize_data(villager, roll);
}

/// `ZombieVillager.finalizeSpawn`：未 finalized 时按群系选品种，再走
/// `Zombie.finalizeSpawn`（幼年、鸡骑士、装备和属性）。
///
/// `ZombieVillager.finalizeSpawn`: pick the biome type when unfinalized, then run
/// `Zombie.finalizeSpawn` (baby, chicken jockey, equipment and attributes).
pub fn finalize_spawn(
    world: &mut World<'_>,
    entity: &mut Entity,
    reason: SpawnReason,
    group: &mut Option<SpawnGroup>,
) {
    let biome = world.biome_id(entity.block_position());
    entity.with_mob(|_, mob| {
        if !mob.villager.finalized {
            mob.villager.kind = facts::type_for_biome(biome);
            mob.villager.finalized = true;
        }
    });
    // `ZombieVillager.finalizeSpawn` 调的是 `Zombie.finalizeSpawn`，不是
    // `Villager` 的 `AgeableMob` 两层。
    //
    // `ZombieVillager.finalizeSpawn` calls `Zombie.finalizeSpawn`, not the villager's
    // `AgeableMob` layers.
    let kind = MobType::ZombieVillager;
    super::zombie_spawn::finalize_spawn(world, kind, entity, reason, group);
}

/// `ZombieVillager.mobInteract`：主手金苹果且身上有虚弱才开始转化。
/// 物品消耗与服务端效果都在 `startConverting` 里落地，玩家背包由调用方改。
///
/// `ZombieVillager.mobInteract`: a main-hand golden apple plus weakness starts the
/// conversion. The caller updates the player's inventory.
pub fn interact(
    mob: &mut Mob,
    req: &MobInteractRequest,
    difficulty: i32,
    random: &mut crate::entity::JavaRandom,
) -> Option<MobInteraction> {
    if mob.mob_type != MobType::ZombieVillager || req.hand != mc_bamboo_core::Hand::Main {
        return None;
    }
    if req.held.item != items::GOLDEN_APPLE {
        return None;
    }
    if !mob.effects.has(mob_effect::WEAKNESS) {
        // 有金苹果但没有虚弱：官方返回 `CONSUME`，不消耗物品。
        // A golden apple without weakness returns `CONSUME` and consumes nothing.
        return Some(MobInteraction {
            result: InteractionResult::Consume,
            ..MobInteraction::pass()
        });
    }
    let time = CONVERSION_WAIT_MIN + random.next_int_bounded(CONVERSION_WAIT_SPAN);
    start_converting(mob, Some(req.player_uuid), time, difficulty);
    Some(MobInteraction {
        result: InteractionResult::SuccessServer,
        shrink_held: !req.creative,
        ..MobInteraction::pass()
    })
}

/// `startConverting`：记发起者 / 时长、同步转化位、移除虚弱、加力量
/// （等级 `min(difficulty.id - 1, 0)`）、发实体事件 16。
///
/// `startConverting`: records the starter and duration, sets the converting bit,
/// removes weakness, adds strength (`min(difficulty.id - 1, 0)`) and broadcasts
/// entity event 16.
pub fn start_converting(mob: &mut Mob, player: Option<[u8; 16]>, time: i32, difficulty: i32) {
    mob.zombie_villager.start(player, time);
    let _ = living::remove_effect(mob, mob_effect::WEAKNESS);
    let amplifier = (difficulty - 1).min(0);
    let _ = living::add_effect(
        mob,
        EffectInstance::with_amplifier(mob_effect::STRENGTH, time, amplifier),
    );
    mob.events.push(MobEvent::EntityEvent { event: 16 });
    mob.needs_sync = true;
}

/// 每刻的转化进度：`nextFloat() < 0.01` 时扫 `x/y/z ∈ [floor-4, floor+4)`，
/// 最多 14 个 `#speeds_up_zombie_villager_curing` 方块；每个命中方块抽一次
/// `nextFloat() < 0.3`，命中就多减一刻。
///
/// Per-tick conversion progress: with `nextFloat() < 0.01`, scan the 9×9×9 box
/// around the mob for up to 14 tagged blocks. Each tagged block draws one
/// `nextFloat() < 0.3` and adds one extra tick of progress.
pub fn conversion_progress(me: &mut Entity, view: &dyn BlockView) -> i32 {
    let mut progress = 1;
    if me.random.next_float() >= 0.01 {
        return progress;
    }
    let radius = SPECIAL_BLOCK_RADIUS;
    let base = BlockPos::new(
        me.position.x as i32,
        me.position.y as i32,
        me.position.z as i32,
    );
    let mut found = 0;
    'outer: for x in (base.x - radius)..(base.x + radius) {
        for y in (base.y - radius)..(base.y + radius) {
            for z in (base.z - radius)..(base.z + radius) {
                if found >= MAX_SPECIAL_BLOCKS_COUNT {
                    break 'outer;
                }
                let state = view.block_state(BlockPos::new(x, y, z));
                if is_curing_block(state) {
                    if me.random.next_float() < 0.3 {
                        progress += 1;
                    }
                    found += 1;
                }
            }
        }
    }
    progress
}

/// `#speeds_up_zombie_villager_curing`：铁栏杆和床。
///
/// `#speeds_up_zombie_villager_curing`: iron bars and beds.
fn is_curing_block(state: BlockState) -> bool {
    state.is_tagged(mc_bamboo_data::tags::block_tags::SPEEDS_UP_ZOMBIE_VILLAGER_CURING)
}

/// `ZombieVillager.tick`：转化中先扣进度，到点发转化命令并丢弃自己。
///
/// `ZombieVillager.tick`: subtract the progress while converting, then emit the
/// conversion command and discard this mob when the timer runs out.
pub fn tick(me: &mut Entity, mob: &mut Mob, ctx: &mut dyn EntityCtx) {
    if mob.mob_type != MobType::ZombieVillager
        || !mob.zombie_villager.converting
        || !living::is_alive(me, mob)
    {
        return;
    }
    let progress = conversion_progress(me, ctx.blocks());
    mob.zombie_villager.conversion_time -= progress;
    if mob.zombie_villager.conversion_time > 0 {
        return;
    }
    ctx.emit(
        crate::entity::ctx::Command::ConvertMob {
            from: Box::new(mob.clone()),
            to: MobType::Villager,
            level_event: ZOMBIE_VILLAGER_CURED_EVENT,
            zombie_attributes: false,
        },
        me,
    );
    me.discard();
}

/// 完成转化后给新村民挂 200 刻反胃并重建大脑（`Villager.refreshBrain`）。
/// 八卦系统还没做：声望只通过 `ConversionPlayer` 留接口。
///
/// After the conversion the new villager gets 200 ticks of nausea and a fresh
/// brain (`Villager.refreshBrain`). Gossip is not modelled, so reputation only
/// keeps the `ConversionPlayer` hook.
pub fn after_cure(me: &mut Entity, mob: &mut Mob) {
    let _ = living::add_effect(mob, EffectInstance::new(mob_effect::NAUSEA, 200));
    super::villager_ai::refresh_brain_offline(me, mob);
}

/// `ZombieVillager.getVoicePitch()`：幼年基础音高 2.0，成年 1.0。
///
/// `ZombieVillager.getVoicePitch()`: base pitch 2.0 for babies, 1.0 for adults.
pub fn voice_pitch(me: &mut Entity, mob: &Mob) -> f32 {
    let first = me.random.next_float();
    let second = me.random.next_float();
    (first - second) * 0.2 + if mob.is_baby() { 2.0 } else { 1.0 }
}

/// 构造时给一只僵尸村民随机职业。
pub fn configure_new(mob: &mut Mob, random: &mut crate::entity::JavaRandom) {
    randomize_profession(&mut mob.villager, random);
}

/// 治疗完成 / 感染时搬运数据（两边共用 `Villager` 字段）。
///
/// State carried over on cure / infection (both sides share the `Villager` fields).
pub fn copy_villager_state(from: &Mob, to: &mut Mob) {
    to.villager = from.villager.clone();
}
