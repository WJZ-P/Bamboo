//! 蜜蜂：`Bee` 的元数据字节、巢 / 花的位置、愤怒和授粉计数。
//!
//! 属性、尺寸、元数据 index 和音效都由生成器从官方导出。飞行导航与移动控制
//! 复用鹦鹉那一份；目标表在 [`super::bee_goals`]。
//!
//! Bees: the `Bee` flag byte, hive/flower positions, anger and pollination counters.
//! Attributes, dimensions, metadata indices and sounds come from the generator; flying
//! navigation and movement control are shared with parrots.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::entity_types;
use mc_bamboo_nbt::Tag;

use super::{Attributes, Mob, MobType};
use crate::archive::tag::{self};

/// `DATA_FLAGS_ID` 的位。
pub const FLAG_ROLL: u8 = 2;
pub const FLAG_HAS_STUNG: u8 = 4;
pub const FLAG_HAS_NECTAR: u8 = 8;

/// `Bee` 的私有状态。
#[derive(Debug, Clone, PartialEq)]
pub struct Bee {
    /// 元数据字节；只用到第 2、4、8 位。
    pub flags: u8,
    /// `DATA_ANGER_END_TIME`，绝对时刻；-1 表示没在生气。
    pub anger_end_time: i64,
    pub time_since_sting: i32,
    pub ticks_without_nectar: i32,
    pub stay_out_of_hive: i32,
    pub crops_grown: i32,
    pub remaining_cooldown_before_locating_new_hive: i32,
    pub remaining_cooldown_before_locating_new_flower: i32,
    pub saved_flower_pos: Option<BlockPos>,
    pub hive_pos: Option<BlockPos>,
    pub under_water_ticks: i32,
    /// 正在授粉（`BeePollinateGoal` 的状态）。
    pub pollinating: bool,
    /// 上一次头部翻滚动画的量（客户端表现，存档不写）。
    pub roll_amount: f32,
    pub roll_amount_o: f32,
}

impl Default for Bee {
    fn default() -> Self {
        Self {
            flags: 0,
            anger_end_time: -1,
            time_since_sting: 0,
            ticks_without_nectar: 0,
            stay_out_of_hive: 0,
            crops_grown: 0,
            remaining_cooldown_before_locating_new_hive: 0,
            // `Mth.nextInt(random, 20, 60)` 的初值由 `Mob::new` 在实体随机源上取。
            remaining_cooldown_before_locating_new_flower: 0,
            saved_flower_pos: None,
            hive_pos: None,
            under_water_ticks: 0,
            pollinating: false,
            roll_amount: 0.0,
            roll_amount_o: 0.0,
        }
    }
}

impl Bee {
    pub fn has_flag(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    pub fn set_flag(&mut self, flag: u8, value: bool) {
        if value {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
    }

    pub fn has_nectar(&self) -> bool {
        self.has_flag(FLAG_HAS_NECTAR)
    }

    pub fn has_stung(&self) -> bool {
        self.has_flag(FLAG_HAS_STUNG)
    }

    pub fn is_rolling(&self) -> bool {
        self.has_flag(FLAG_ROLL)
    }
}

/// `EntityTypes.BEE`：`sized(0.55F, 0.5F)`，眼高由生成器给出。
pub(super) fn dimensions() -> (f32, f32, f32) {
    let facts = &entity_types::FACTS[MobType::Bee.entity_type() as usize];
    (facts.width, facts.height, entity_types::BEE_EYE_HEIGHT)
}

/// `Bee.createAttributes`：四个基础值全部来自生成器。
pub(super) fn attributes() -> Attributes {
    let mut table = Attributes::cow();
    table.set_base(
        mc_bamboo_data::registries::attribute::MAX_HEALTH,
        entity_types::BEE_MAX_HEALTH,
    );
    table.set_base(
        mc_bamboo_data::registries::attribute::MOVEMENT_SPEED,
        entity_types::BEE_MOVEMENT_SPEED,
    );
    table.entries.push(super::attributes::Attribute::new(
        mc_bamboo_data::registries::attribute::FLYING_SPEED,
        entity_types::BEE_FLYING_SPEED,
    ));
    table.entries.push(super::attributes::Attribute::new(
        mc_bamboo_data::registries::attribute::ATTACK_DAMAGE,
        entity_types::BEE_ATTACK_DAMAGE,
    ));
    table
}

/// 构造函数里 `FlyingPathNavigation` 和寻路代价，以及三个倒计时的初值抽取。
pub(super) fn configure(mob: &mut Mob) {
    use mc_bamboo_data::PathType;
    mob.navigation.flags.fly = true;
    mob.navigation.flags.can_float = false;
    mob.navigation.flags.can_open_doors = false;
    mob.navigation.required_path_length = 48.0;
    mob.malus.set(PathType::Fire, -1.0);
    mob.malus.set(PathType::Water, -1.0);
    mob.malus.set(PathType::WaterBorder, 16.0);
    mob.malus.set(PathType::Cocoa, -1.0);
    mob.malus.set(PathType::Fence, -1.0);
}

/// `Bee.addAdditionalSaveData` 追加的键。
pub fn write_nbt(mob: &Mob, out: &mut Vec<(String, Tag)>) {
    use crate::archive::tag::put;
    let bee = &mob.bee;
    if let Some(pos) = bee.hive_pos {
        put(out, "hive_pos", block_pos_tag(pos));
    }
    if let Some(pos) = bee.saved_flower_pos {
        put(out, "flower_pos", block_pos_tag(pos));
    }
    put(out, "HasNectar", Tag::Byte(i8::from(bee.has_nectar())));
    put(out, "HasStung", Tag::Byte(i8::from(bee.has_stung())));
    put(
        out,
        "TicksSincePollination",
        Tag::Int(bee.ticks_without_nectar),
    );
    put(out, "CannotEnterHiveTicks", Tag::Int(bee.stay_out_of_hive));
    put(out, "CropsGrownSincePollination", Tag::Int(bee.crops_grown));
    super::neutral::write_nbt(mob, out);
}

/// `Bee.readAdditionalSaveData`：缺键用构造器缺省值。
pub fn read_nbt(mob: &mut Mob, tag: &Tag) {
    let bee = &mut mob.bee;
    bee.set_flag(FLAG_HAS_NECTAR, tag::bool_or(tag, "HasNectar", false));
    bee.set_flag(FLAG_HAS_STUNG, tag::bool_or(tag, "HasStung", false));
    bee.ticks_without_nectar = tag::int_or(tag, "TicksSincePollination", 0);
    bee.stay_out_of_hive = tag::int_or(tag, "CannotEnterHiveTicks", 0);
    bee.crops_grown = tag::int_or(tag, "CropsGrownSincePollination", 0);
    bee.hive_pos = block_pos(tag, "hive_pos");
    bee.saved_flower_pos = block_pos(tag, "flower_pos");
    super::neutral::read_nbt(mob, tag);
}

fn block_pos_tag(pos: BlockPos) -> Tag {
    Tag::IntArray(vec![pos.x, pos.y, pos.z])
}

fn block_pos(tag: &Tag, key: &str) -> Option<BlockPos> {
    let raw = tag::int_array(tag, key);
    (raw.len() == 3).then(|| BlockPos::new(raw[0], raw[1], raw[2]))
}

/// `Occupant.createEntity` 的 `setBeeReleaseData`：年龄按在巢里待的刻数推进，
/// 求爱时间减掉同样的刻数，并把蜂巢坐标记到蜂身上。
pub fn set_release_data(mob: &mut Mob, hive: BlockPos, ticks_in_hive: i32) {
    mob.bee.hive_pos = Some(hive);
    if !mob.age_locked {
        let age = mob.age;
        mob.age = if age < 0 {
            (age.saturating_add(ticks_in_hive)).min(0)
        } else {
            (age.saturating_sub(ticks_in_hive)).max(0)
        };
    }
    mob.in_love = mob.in_love.saturating_sub(ticks_in_hive).max(0);
}

/// 蜂巢放出蜜蜂时有没有记住花。
pub fn has_saved_flower(mob: &Mob) -> bool {
    mob.bee.saved_flower_pos.is_some()
}

pub fn set_saved_flower(mob: &mut Mob, pos: Option<BlockPos>) {
    mob.bee.saved_flower_pos = pos;
}

pub fn set_stay_out_of_hive(mob: &mut Mob, ticks: i32) {
    mob.bee.stay_out_of_hive = ticks;
}

/// `Bee.dropOffNectar`：清掉花蜜、授粉计数归零。
pub fn drop_off_nectar(mob: &mut Mob) {
    mob.bee.set_flag(FLAG_HAS_NECTAR, false);
    mob.bee.crops_grown = 0;
    mob.bee.ticks_without_nectar = 0;
}

/// `Bee.aiStep` 里的三个倒计时、翻滚和巢有效性检查。
pub fn ai_step(mob: &mut Mob, tick_count: i32, in_water: bool, target_close: bool) {
    let bee = &mut mob.bee;
    if bee.stay_out_of_hive > 0 {
        bee.stay_out_of_hive -= 1;
    }
    if bee.remaining_cooldown_before_locating_new_hive > 0 {
        bee.remaining_cooldown_before_locating_new_hive -= 1;
    }
    if bee.remaining_cooldown_before_locating_new_flower > 0 {
        bee.remaining_cooldown_before_locating_new_flower -= 1;
    }
    bee.set_flag(
        FLAG_ROLL,
        mob.anger.is_angry(0) && !bee.has_stung() && target_close,
    );
    let _ = (tick_count, in_water);
}

/// `Bee.customServerAiStep`：溺水、蜇后死亡抽签、没蜜时的计时和愤怒更新。
pub fn custom_server_ai_step(
    me: &mut crate::entity::Entity,
    mob: &mut Mob,
    now: i64,
    difficulty: i32,
) {
    let _ = (difficulty, now);
    let had_stung = mob.bee.has_stung();
    let bee = &mut mob.bee;
    if me.in_water {
        bee.under_water_ticks += 1;
    } else {
        bee.under_water_ticks = 0;
    }
    if had_stung {
        bee.time_since_sting = bee.time_since_sting.saturating_add(1);
        let remaining = (1200 - bee.time_since_sting).clamp(1, 1200);
        if bee.time_since_sting % 5 == 0 && me.random.next_int_bounded(remaining) == 0 {
            mob.health = 0.0;
            mob.dead = true;
        }
    }
    if !bee.has_nectar() {
        bee.ticks_without_nectar = bee.ticks_without_nectar.saturating_add(1);
    }
    let _ = mob.anger.is_angry(now);
}
