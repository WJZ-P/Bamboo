//! `SpellcasterIllager` 的共享状态：施法计时、当前法术，以及 `SpellcasterUseSpellGoal`
//! 基类共用的两个计时字段。唤魔者和幻术师共用。
//!
//! 施法状态同步走 `DATA_SPELL_CASTING_ID`（字节，等于当前法术的 `id`）；`isCastingSpell`
//! 在服务端只看 `spellCastingTickCount > 0`。
//!
//! The shared `SpellcasterIllager` state: the casting timer, the current spell and the
//! two timers shared by the `SpellcasterUseSpellGoal` base. Evokers and illusioners
//! share it. Casting state syncs through `DATA_SPELL_CASTING_ID` (a byte equal to the
//! spell's `id`); `isCastingSpell` on the server only looks at `spellCastingTickCount > 0`.

use crate::entity::mob::Mob;

/// `SpellcasterIllager.IllagerSpell`：六种法术，`id` 就是同步字节的值，颜色只在客户端用。
///
/// `SpellcasterIllager.IllagerSpell`: the six spells; `id` is the synced byte and the
/// colour is client-side only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IllagerSpell {
    #[default]
    None,
    SummonVex,
    Fangs,
    Wololo,
    Disappear,
    Blindness,
}

impl IllagerSpell {
    /// 同步字节的值，照官方枚举的 `id`。
    ///
    /// The synced byte, matching the vanilla `id`.
    pub const fn id(self) -> i32 {
        match self {
            Self::None => 0,
            Self::SummonVex => 1,
            Self::Fangs => 2,
            Self::Wololo => 3,
            Self::Disappear => 4,
            Self::Blindness => 5,
        }
    }

    /// `IllagerSpell.byId`：坏值退回 `NONE`（官方 `ByIdMap` 的越界策略）。
    ///
    /// `IllagerSpell.byId`: bad values fall back to `NONE`.
    pub const fn from_id(id: i32) -> Self {
        match id {
            1 => Self::SummonVex,
            2 => Self::Fangs,
            3 => Self::Wololo,
            4 => Self::Disappear,
            5 => Self::Blindness,
            _ => Self::None,
        }
    }
}

/// `SpellcasterIllager` 自己的状态。`spell_casting_tick_count` 存档键是 `SpellTicks`，
/// `current_spell` 不存档（读盘后清空）。
///
/// The `SpellcasterIllager` state. `spell_casting_tick_count` saves as `SpellTicks`;
/// `current_spell` is not saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpellcasterState {
    /// `spellCastingTickCount`。
    pub spell_casting_tick_count: i32,
    /// `currentSpell`。
    pub current_spell: IllagerSpell,
}

impl SpellcasterState {
    /// `isCastingSpell()` 的服务端分支。
    ///
    /// The server branch of `isCastingSpell()`.
    pub const fn is_casting_spell(&self) -> bool {
        self.spell_casting_tick_count > 0
    }

    /// `setIsCastingSpell(spell)`：记下法术并同步 `DATA_SPELL_CASTING_ID`。
    ///
    /// `setIsCastingSpell(spell)`: record the spell and sync `DATA_SPELL_CASTING_ID`.
    pub fn set_spell(&mut self, spell: IllagerSpell) {
        self.current_spell = spell;
    }
}

/// `SpellcasterUseSpellGoal` 基类共用的两个字段。每个施法目标各存一份。
///
/// The two fields shared by `SpellcasterUseSpellGoal`; each casting goal keeps its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpellcasterUseSpell {
    /// `attackWarmupDelay`。
    pub attack_warmup_delay: i32,
    /// `nextAttackTickCount`。
    pub next_attack_tick_count: i32,
}

/// `SpellcasterIllager.customServerAiStep` 在 super 之后的那一句：施法计时递减。
/// `aiStep` 里调用，每个施法生物（唤魔者 / 幻术师）都走这里。
///
/// The part of `SpellcasterIllager.customServerAiStep` after `super`: count the
/// casting timer down. Called from `aiStep` for every spellcaster.
pub(super) fn server_ai_step(mob: &mut Mob) {
    if mob.spellcaster.spell_casting_tick_count > 0 {
        mob.spellcaster.spell_casting_tick_count -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spell_ids_and_round_trip() {
        assert_eq!(IllagerSpell::None.id(), 0);
        assert_eq!(IllagerSpell::SummonVex.id(), 1);
        assert_eq!(IllagerSpell::Fangs.id(), 2);
        assert_eq!(IllagerSpell::Wololo.id(), 3);
        assert_eq!(IllagerSpell::Disappear.id(), 4);
        assert_eq!(IllagerSpell::Blindness.id(), 5);
        for spell in [
            IllagerSpell::None,
            IllagerSpell::SummonVex,
            IllagerSpell::Fangs,
            IllagerSpell::Wololo,
            IllagerSpell::Disappear,
            IllagerSpell::Blindness,
        ] {
            assert_eq!(IllagerSpell::from_id(spell.id()), spell);
        }
        assert_eq!(IllagerSpell::from_id(-1), IllagerSpell::None);
        assert_eq!(IllagerSpell::from_id(99), IllagerSpell::None);
    }

    #[test]
    fn casting_is_only_while_the_timer_is_positive() {
        let mut state = SpellcasterState::default();
        assert!(!state.is_casting_spell());
        state.spell_casting_tick_count = 1;
        assert!(state.is_casting_spell());
        state.spell_casting_tick_count = 0;
        assert!(!state.is_casting_spell());
    }
}
