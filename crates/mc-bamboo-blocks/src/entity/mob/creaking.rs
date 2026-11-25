//! `Creaking`：和嘎枝之心绑定的敌对生物。
//!
//! 这里只落地官方构造函数、专属状态和 `home_pos` 存档。看着它时不能动、
//! 攻击和树脂连线等逐刻行为由后续阶段接入。
//!
//! `Creaking`: the hostile bound to a creaking heart. This module ports the
//! constructor, state and `home_pos` key; freeze, attack and resin effects are
//! wired in later stages.

use mc_bamboo_core::BlockPos;
use mc_bamboo_data::{entity_types, registries::attribute};

use super::{Mob, attributes::Attributes};

/// `Creaking.createAttributes()`：怪物属性上覆盖五个基础值。
pub(super) fn attributes() -> Attributes {
    let mut attributes = Attributes::monster();
    attributes.set_base(attribute::MAX_HEALTH, entity_types::CREAKING_MAX_HEALTH);
    attributes.set_base(
        attribute::MOVEMENT_SPEED,
        entity_types::CREAKING_MOVEMENT_SPEED,
    );
    attributes.set_base(
        attribute::ATTACK_DAMAGE,
        entity_types::CREAKING_ATTACK_DAMAGE,
    );
    attributes.set_base(attribute::FOLLOW_RANGE, entity_types::CREAKING_FOLLOW_RANGE);
    attributes.set_base(attribute::STEP_HEIGHT, entity_types::CREAKING_STEP_HEIGHT);
    attributes
}

/// `Creaking` 的运行时状态。四个同步项都按官方缺省值构造。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CreakingState {
    /// `CAN_MOVE`，缺省真。
    pub can_move: bool,
    /// `IS_ACTIVE`，缺省假。
    pub active: bool,
    /// `IS_TEARING_DOWN`，缺省假。
    pub tearing_down: bool,
    /// `HOME_POS`；没有绑定心时是空。
    pub home_pos: Option<BlockPos>,
}

/// 官方 `defineSynchedData` 的缺省值；`Default` 给别的生物用 false 填满。
impl CreakingState {
    pub fn new() -> Self {
        Self {
            can_move: true,
            ..Self::default()
        }
    }
}

/// `setTransient(homePos)`：绑定心。
pub fn bind(mob: &mut Mob, home: BlockPos) {
    mob.creaking.home_pos = Some(home);
}

/// `isHeartBound()`。
pub fn is_heart_bound(mob: &Mob) -> bool {
    mob.creaking.home_pos.is_some()
}

/// `home_pos` 的 NBT 形状是三个 int 的数组。
pub fn home_tag(home: BlockPos) -> mc_bamboo_nbt::Tag {
    mc_bamboo_nbt::Tag::IntArray(vec![home.x, home.y, home.z])
}

/// 坏数据一律不绑定，不让客户端 / 命令传来的值触发 panic。
pub fn home_from_tag(tag: &mc_bamboo_nbt::Tag) -> Option<BlockPos> {
    let mc_bamboo_nbt::Tag::IntArray(values) = tag else {
        return None;
    };
    (values.len() == 3).then(|| BlockPos::new(values[0], values[1], values[2]))
}

/// 逐刻对拍用的状态行。
impl Mob {
    pub fn describe_creaking(&self) -> String {
        let state = &self.creaking;
        format!(
            "can_move={} active={} tearing_down={} home={:?}",
            state.can_move, state.active, state.tearing_down, state.home_pos
        )
    }
}
