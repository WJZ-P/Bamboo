//! 服务器计分板和队伍。全世界共用一份，放在 [`Global`](crate::global::Global) 上。
//!
//! 不写存档。目标只有被显示出来才跟踪，包的顺序按 `ServerScoreboard`。
//!
//! The server scoreboard and teams. One copy is shared by the whole server and lives on
//! [`Global`](crate::global::Global).
//!
//! Nothing is saved. An objective is tracked only while it is displayed, and packet order
//! follows `ServerScoreboard`.

mod boss;
mod format;

use std::collections::{BTreeMap, BTreeSet};

use mc_bamboo_protocol::{
    packet::play::{
        NumberFormat, ResetScore, SetDisplayObjective, SetObjective, SetPlayerTeam, SetScore,
        TeamAction, TeamParameters,
    },
    text::Component,
};

use crate::{links::encode_frames, region::Region};

#[allow(unused_imports)]
pub(crate) use boss::{BossBars, BossColor, BossStyle};
#[allow(unused_imports)]
pub(crate) use format::{boss_name, format_green_names, format_list, objective_name, team_name};

/// 一次改分数时的记分板名字。`display` 只有在线玩家才有。
///
/// The scoreboard name for one score change. `display` is set only for an online player.
pub(crate) struct Holder {
    pub name: String,
    pub display: Option<Component>,
}

/// 运算写不进去，或者除数是 0，或者结果放不进 `int`。
///
/// A score write was rejected, the divisor was 0, or the result does not fit in an `int`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScoreFailure {
    ReadOnly,
    DivideByZero,
    Overflow,
}

#[derive(Debug, Clone, PartialEq)]
struct Objective {
    name: String,
    criteria: String,
    read_only: bool,
    display: Component,
    /// 0 是整数，1 是心。
    ///
    /// 0 is integer and 1 is hearts.
    render: i32,
    auto_update: bool,
    format: Option<NumberFormat>,
}

impl Objective {
    fn formatted(&self) -> Component {
        objective_name(&self.name, &self.display)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Score {
    value: i32,
    locked: bool,
    display: Option<Component>,
    format: Option<NumberFormat>,
}

impl Score {
    fn new() -> Self {
        Self {
            value: 0,
            locked: true,
            display: None,
            format: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Team {
    name: String,
    display: Component,
    prefix: Component,
    suffix: Component,
    friendly_fire: bool,
    see_invisibles: bool,
    visibility: i32,
    death_messages: i32,
    collision: i32,
    color: Option<u8>,
    members: Vec<String>,
}

impl Team {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            display: Component::text(name),
            prefix: Component::text(""),
            suffix: Component::text(""),
            friendly_fire: true,
            see_invisibles: true,
            visibility: 0,
            death_messages: 0,
            collision: 0,
            color: None,
            members: Vec::new(),
        }
    }

    fn formatted(&self) -> Component {
        team_name(&self.name, &self.display, self.color)
    }

    fn options(&self) -> u8 {
        u8::from(self.friendly_fire) | (u8::from(self.see_invisibles) << 1)
    }

    fn parameters(&self) -> TeamParameters<'_> {
        TeamParameters {
            display: &self.display,
            prefix: &self.prefix,
            suffix: &self.suffix,
            visibility: self.visibility,
            collision: self.collision,
            color: self.color.map(i32::from),
            options: self.options(),
        }
    }
}

/// 官方 `ServerScoreboard`。重启后是空的。
///
/// Official `ServerScoreboard`. A restart leaves it empty.
#[derive(Debug, Default)]
pub(crate) struct Scoreboard {
    objectives: Vec<Objective>,
    /// 玩家名到各目标的分数。没有分数的玩家不留在这里。
    ///
    /// Player name to scores. A player with no scores is not kept here.
    scores: BTreeMap<String, BTreeMap<String, Score>>,
    tracked: BTreeSet<String>,
    display: [Option<String>; 19],
    teams: Vec<Team>,
    membership: BTreeMap<String, String>,
    /// 官方 `ServerPlayer` 的六个 `lastRecorded*` 字段。缺省就是构造函数的初值。
    ///
    /// The six official `ServerPlayer.lastRecorded*` fields. Absence means the constructor default.
    recorded: BTreeMap<String, [i32; 6]>,
}

impl Scoreboard {
    pub(crate) fn objective_exists(&self, name: &str) -> bool {
        self.objective(name).is_some()
    }

    pub(crate) fn is_read_only(&self, name: &str) -> bool {
        self.objective(name)
            .is_some_and(|objective| objective.read_only)
    }

    /// 有没有启用「显示自动更新」的目标。分数写入时用它决定要不要带显示名。
    ///
    /// Whether any objective has display auto-update enabled. Score writes use it
    /// to decide whether a display component should be attached.
    pub(crate) fn auto_updates(&self, _player: &str) -> bool {
        self.objectives
            .iter()
            .any(|objective| objective.auto_update)
    }

    pub(crate) fn is_trigger(&self, name: &str) -> bool {
        self.objective(name)
            .is_some_and(|objective| objective.criteria == "trigger")
    }

    pub(crate) fn objective_count(&self) -> usize {
        self.objectives.len()
    }

    pub(crate) fn objective_entries(&self) -> Vec<Component> {
        self.objectives.iter().map(Objective::formatted).collect()
    }

    pub(crate) fn formatted_objective(&self, name: &str) -> Option<Component> {
        self.objective(name).map(Objective::formatted)
    }

    pub(crate) fn raw_objective_display(&self, name: &str) -> Option<&Component> {
        self.objective(name).map(|objective| &objective.display)
    }

    pub(crate) fn display_at(&self, slot: u8) -> Option<&str> {
        self.display
            .get(slot as usize)
            .and_then(|slot| slot.as_deref())
    }

    pub(crate) fn score_value(&self, player: &str, objective: &str) -> Option<i32> {
        self.scores
            .get(player)
            .and_then(|scores| scores.get(objective))
            .map(|score| score.value)
    }

    pub(crate) fn score_locked(&self, player: &str, objective: &str) -> Option<bool> {
        self.scores
            .get(player)
            .and_then(|scores| scores.get(objective))
            .map(|score| score.locked)
    }

    pub(crate) fn tracked_players(&self) -> Vec<String> {
        self.scores.keys().cloned().collect()
    }

    pub(crate) fn player_score_lines(&self, player: &str) -> Vec<(Component, i32)> {
        let Some(scores) = self.scores.get(player) else {
            return Vec::new();
        };
        scores
            .iter()
            .filter_map(|(objective, score)| {
                self.formatted_objective(objective)
                    .map(|name| (name, score.value))
            })
            .collect()
    }

    /// 选择器的 `scores`。没有这个目标，或者这个玩家还没有分数，都不算匹配。
    ///
    /// A selector `scores` check. A missing objective or a missing score does not match.
    pub(crate) fn score_in(
        &self,
        player: &str,
        objective: &str,
        contains: impl Fn(i32) -> bool,
    ) -> bool {
        self.score_value(player, objective).is_some_and(contains)
            && self.objective_exists(objective)
    }

    pub(crate) fn team_exists(&self, name: &str) -> bool {
        self.team(name).is_some()
    }

    pub(crate) fn team_of(&self, player: &str) -> Option<&str> {
        self.membership.get(player).map(String::as_str)
    }

    pub(crate) fn team_count(&self) -> usize {
        self.teams.len()
    }

    pub(crate) fn team_entries(&self) -> Vec<Component> {
        self.teams.iter().map(Team::formatted).collect()
    }

    pub(crate) fn formatted_team(&self, name: &str) -> Option<Component> {
        self.team(name).map(Team::formatted)
    }

    pub(crate) fn team_display(&self, name: &str) -> Option<&Component> {
        self.team(name).map(|team| &team.display)
    }

    pub(crate) fn team_color(&self, name: &str) -> Option<Option<u8>> {
        self.team(name).map(|team| team.color)
    }

    pub(crate) fn team_flag(&self, name: &str, friendly_fire: bool) -> Option<bool> {
        self.team(name).map(|team| {
            if friendly_fire {
                team.friendly_fire
            } else {
                team.see_invisibles
            }
        })
    }

    pub(crate) fn team_visibility(&self, name: &str, death: bool) -> Option<i32> {
        self.team(name).map(|team| {
            if death {
                team.death_messages
            } else {
                team.visibility
            }
        })
    }

    pub(crate) fn team_collision(&self, name: &str) -> Option<i32> {
        self.team(name).map(|team| team.collision)
    }

    pub(crate) fn team_members(&self, name: &str) -> Option<&[String]> {
        self.team(name).map(|team| team.members.as_slice())
    }

    /// 没有队伍时就是纯文本，和现在的聊天、加入消息字节一致。
    ///
    /// Without a team this is plain text, so current chat and join packets stay the same.
    pub(crate) fn format_player_name(&self, name: &str) -> Component {
        let plain = Component::text(name);
        let Some(team_name) = self.team_of(name) else {
            return plain;
        };
        let Some(team) = self.team(team_name) else {
            return plain;
        };
        format::format_name(team.color, &team.prefix, &team.suffix, &plain)
    }

    pub(crate) fn add_objective(
        &mut self,
        name: &str,
        criteria: &str,
        read_only: bool,
        hearts: bool,
        display: Component,
    ) -> bool {
        if self.objective_exists(name) {
            return false;
        }
        self.objectives.push(Objective {
            name: name.to_owned(),
            criteria: criteria.to_owned(),
            read_only,
            display,
            render: i32::from(hearts),
            auto_update: false,
            format: None,
        });
        true
    }

    pub(crate) fn remove_objective(
        &mut self,
        name: &str,
        region: &mut Region,
    ) -> Option<Component> {
        let formatted = self.formatted_objective(name)?;
        for slot in 0..self.display.len() {
            if self.display[slot].as_deref() == Some(name) {
                self.set_display(slot as u8, None, region);
            }
        }
        for scores in self.scores.values_mut() {
            scores.remove(name);
        }
        self.scores.retain(|_, scores| !scores.is_empty());
        self.objectives.retain(|objective| objective.name != name);
        if self.tracked.contains(name) {
            self.stop_tracking(name, region);
        }
        Some(formatted)
    }

    pub(crate) fn set_objective_display(
        &mut self,
        name: &str,
        display: Component,
        region: &mut Region,
    ) -> bool {
        let Some(objective) = self
            .objectives
            .iter_mut()
            .find(|objective| objective.name == name)
        else {
            return false;
        };
        if objective.display == display {
            return false;
        }
        objective.display = display;
        let changed = objective.clone();
        if self.tracked.contains(name) {
            send_objective(region, &changed, 2);
        }
        true
    }

    pub(crate) fn set_render(&mut self, name: &str, hearts: bool, region: &mut Region) -> bool {
        let render = i32::from(hearts);
        let Some(objective) = self
            .objectives
            .iter_mut()
            .find(|objective| objective.name == name)
        else {
            return false;
        };
        if objective.render == render {
            return false;
        }
        objective.render = render;
        let changed = objective.clone();
        if self.tracked.contains(name) {
            send_objective(region, &changed, 2);
        }
        true
    }

    pub(crate) fn set_auto_update(
        &mut self,
        name: &str,
        enabled: bool,
        region: &mut Region,
    ) -> bool {
        let Some(objective) = self
            .objectives
            .iter_mut()
            .find(|objective| objective.name == name)
        else {
            return false;
        };
        if objective.auto_update == enabled {
            return false;
        }
        objective.auto_update = enabled;
        let changed = objective.clone();
        if self.tracked.contains(name) {
            send_objective(region, &changed, 2);
        }
        true
    }

    pub(crate) fn set_objective_format(
        &mut self,
        name: &str,
        format: Option<NumberFormat>,
        region: &mut Region,
    ) {
        let Some(objective) = self
            .objectives
            .iter_mut()
            .find(|objective| objective.name == name)
        else {
            return;
        };
        objective.format = format;
        let changed = objective.clone();
        if self.tracked.contains(name) {
            send_objective(region, &changed, 2);
        }
    }

    pub(crate) fn set_display(&mut self, slot: u8, next: Option<&str>, region: &mut Region) {
        let Some(current) = self.display.get_mut(slot as usize) else {
            return;
        };
        let old = current.clone();
        *current = next.map(str::to_owned);
        if old.as_deref() != next
            && let Some(old_name) = old
        {
            if self.slot_count(&old_name) > 0 {
                send_slot(region, slot, next.unwrap_or(""));
            } else {
                self.stop_tracking(&old_name, region);
            }
        }
        if let Some(next) = next {
            if self.tracked.contains(next) {
                send_slot(region, slot, next);
            } else {
                self.start_tracking(next, region);
            }
        }
    }

    pub(crate) fn set_score(
        &mut self,
        holder: &Holder,
        objective: &str,
        value: i32,
        region: &mut Region,
    ) {
        let _ = self.write_score(holder, objective, value, false, region);
    }

    pub(crate) fn add_score(
        &mut self,
        holder: &Holder,
        objective: &str,
        amount: i32,
        region: &mut Region,
    ) -> i32 {
        let fresh = self.ensure_score(&holder.name, objective);
        let current = self.score_value(&holder.name, objective).unwrap_or(0);
        let value = current.wrapping_add(amount);
        let _ = self.write_with_fresh(holder, objective, value, fresh, false, region);
        value
    }

    pub(crate) fn write_forced(
        &mut self,
        holder: &Holder,
        objective: &str,
        value: i32,
        region: &mut Region,
    ) {
        let _ = self.write_score(holder, objective, value, true, region);
    }

    pub(crate) fn unlock(&mut self, holder: &Holder, objective: &str, region: &mut Region) -> bool {
        let fresh = self.ensure_score(&holder.name, objective);
        let Some(score) = self.score_mut(&holder.name, objective) else {
            return false;
        };
        if !score.locked {
            return false;
        }
        score.locked = false;
        if fresh {
            self.send_score_now(&holder.name, objective, region);
        }
        true
    }

    pub(crate) fn set_score_display(
        &mut self,
        holder: &Holder,
        objective: &str,
        display: Option<Component>,
        region: &mut Region,
    ) {
        let fresh = self.ensure_score(&holder.name, objective);
        let Some(score) = self.score_mut(&holder.name, objective) else {
            return;
        };
        if !fresh && score.display == display {
            return;
        }
        score.display = display;
        self.send_score_now(&holder.name, objective, region);
    }

    pub(crate) fn set_score_format(
        &mut self,
        holder: &Holder,
        objective: &str,
        format: Option<NumberFormat>,
        region: &mut Region,
    ) {
        self.ensure_score(&holder.name, objective);
        if let Some(score) = self.score_mut(&holder.name, objective) {
            score.format = format;
        }
        self.send_score_now(&holder.name, objective, region);
    }

    pub(crate) fn reset_score(&mut self, player: &str, objective: &str, region: &mut Region) {
        let Some(scores) = self.scores.get_mut(player) else {
            return;
        };
        let removed = scores.remove(objective).is_some();
        if scores.is_empty() {
            self.scores.remove(player);
            if !self.tracked.is_empty() {
                send_reset(region, player, None);
            }
        } else if removed && self.tracked.contains(objective) {
            send_reset(region, player, Some(objective));
        }
    }

    pub(crate) fn reset_player(&mut self, player: &str, region: &mut Region) {
        if self.scores.remove(player).is_some() && !self.tracked.is_empty() {
            send_reset(region, player, None);
        }
    }

    pub(crate) fn operate(
        &mut self,
        target: &Holder,
        target_objective: &str,
        operation: mc_bamboo_command::ScoreOperation,
        source: &Holder,
        source_objective: &str,
        region: &mut Region,
    ) -> Result<i32, ScoreFailure> {
        if operation == mc_bamboo_command::ScoreOperation::Swap {
            self.swap(target, target_objective, source, source_objective, region)?;
            return Ok(self
                .score_value(&target.name, target_objective)
                .unwrap_or(0));
        }
        let fresh = self.ensure_score(&target.name, target_objective);
        let right = self.opening_value(&source.name, source_objective);
        let left = self
            .score_value(&target.name, target_objective)
            .unwrap_or(0);
        let value = apply_operation(operation, left, right)?;
        self.write_with_fresh(target, target_objective, value, fresh, false, region)?;
        Ok(value)
    }

    /// `><` 两边各有自己的 `requiresSync`。新造出来的来源即使写回原值也要发包。
    ///
    /// Each side of `><` has its own `requiresSync`. A source score created for the swap
    /// sends a packet even when the stored value does not change.
    pub(crate) fn swap(
        &mut self,
        target: &Holder,
        target_objective: &str,
        source: &Holder,
        source_objective: &str,
        region: &mut Region,
    ) -> Result<(), ScoreFailure> {
        let left_fresh = self.ensure_score(&target.name, target_objective);
        let right_fresh = self.ensure_score(&source.name, source_objective);
        let left = self
            .score_value(&target.name, target_objective)
            .unwrap_or(0);
        let right = self
            .score_value(&source.name, source_objective)
            .unwrap_or(0);
        self.write_with_fresh(target, target_objective, right, left_fresh, false, region)?;
        self.write_with_fresh(source, source_objective, left, right_fresh, false, region)?;
        Ok(())
    }

    pub(crate) fn create_team(
        &mut self,
        name: &str,
        display: Component,
        region: &mut Region,
    ) -> bool {
        if self.team_exists(name) {
            return false;
        }
        let team = Team::new(name);
        send_team_add(region, &team);
        self.teams.push(team);
        self.set_team_display(name, display, region);
        true
    }

    pub(crate) fn delete_team(&mut self, name: &str, region: &mut Region) -> Option<Component> {
        let formatted = self.formatted_team(name)?;
        let members = self
            .team(name)
            .map(|team| team.members.clone())
            .unwrap_or_default();
        for member in members {
            self.membership.remove(&member);
        }
        self.teams.retain(|team| team.name != name);
        region.links.broadcast(&encode_frames(&SetPlayerTeam {
            name,
            action: TeamAction::Remove,
        }));
        Some(formatted)
    }

    pub(crate) fn empty_team(&mut self, name: &str, region: &mut Region) -> Option<Vec<String>> {
        let members = self.team(name)?.members.clone();
        for member in &members {
            self.leave(member, region);
        }
        Some(members)
    }

    pub(crate) fn join(&mut self, player: &str, team_name: &str, region: &mut Region) -> bool {
        if !self.team_exists(team_name) {
            return false;
        }
        if self.membership.contains_key(player) {
            self.leave(player, region);
        }
        let Some(team) = self.teams.iter_mut().find(|team| team.name == team_name) else {
            return false;
        };
        if team.members.iter().any(|member| member == player) {
            return false;
        }
        team.members.push(player.to_owned());
        self.membership
            .insert(player.to_owned(), team_name.to_owned());
        let players = [player.to_owned()];
        region.links.broadcast(&encode_frames(&SetPlayerTeam {
            name: team_name,
            action: TeamAction::Join { players: &players },
        }));
        true
    }

    pub(crate) fn leave(&mut self, player: &str, region: &mut Region) -> bool {
        let Some(team_name) = self.membership.remove(player) else {
            return false;
        };
        if let Some(team) = self.teams.iter_mut().find(|team| team.name == team_name) {
            team.members.retain(|member| member != player);
        }
        let players = [player.to_owned()];
        region.links.broadcast(&encode_frames(&SetPlayerTeam {
            name: &team_name,
            action: TeamAction::Leave { players: &players },
        }));
        true
    }

    pub(crate) fn set_team_display(&mut self, name: &str, display: Component, region: &mut Region) {
        let Some(team) = self.teams.iter_mut().find(|team| team.name == name) else {
            return;
        };
        team.display = display;
        let team = team.clone();
        send_team_change(region, &team);
    }

    pub(crate) fn set_team_prefix(&mut self, name: &str, prefix: Component, region: &mut Region) {
        self.update_team(name, region, |team| team.prefix = prefix);
    }

    pub(crate) fn set_team_suffix(&mut self, name: &str, suffix: Component, region: &mut Region) {
        self.update_team(name, region, |team| team.suffix = suffix);
    }

    pub(crate) fn set_team_color(&mut self, name: &str, color: Option<u8>, region: &mut Region) {
        self.update_team(name, region, |team| team.color = color);
    }

    pub(crate) fn set_team_friendly_fire(
        &mut self,
        name: &str,
        allowed: bool,
        region: &mut Region,
    ) {
        self.update_team(name, region, |team| team.friendly_fire = allowed);
    }

    pub(crate) fn set_team_see_invisibles(
        &mut self,
        name: &str,
        allowed: bool,
        region: &mut Region,
    ) {
        self.update_team(name, region, |team| team.see_invisibles = allowed);
    }

    pub(crate) fn set_team_visibility(&mut self, name: &str, visibility: i32, region: &mut Region) {
        self.update_team(name, region, |team| team.visibility = visibility);
    }

    pub(crate) fn set_team_death(&mut self, name: &str, visibility: i32, region: &mut Region) {
        self.update_team(name, region, |team| team.death_messages = visibility);
    }

    pub(crate) fn set_team_collision(&mut self, name: &str, collision: i32, region: &mut Region) {
        self.update_team(name, region, |team| team.collision = collision);
    }

    /// 进服时发给这名玩家的队伍和已显示目标。空计分板一个包都不加。
    ///
    /// Teams and displayed objectives sent to a player who is joining. An empty scoreboard
    /// adds no packets.
    pub(crate) fn join_frames(&self) -> Vec<u8> {
        let mut frames = Vec::new();
        for team in &self.teams {
            frames.extend(encode_frames(&SetPlayerTeam {
                name: &team.name,
                action: TeamAction::Add {
                    parameters: team.parameters(),
                    players: &team.members,
                },
            }));
        }
        let mut sent = BTreeSet::new();
        for objective in &self.display {
            let Some(name) = objective else {
                continue;
            };
            if !sent.insert(name.clone()) {
                continue;
            }
            if let Some(objective) = self.objective(name) {
                append_start(&mut frames, self, objective);
            }
        }
        frames
    }

    /// 玩家断开时什么都不广播。
    ///
    /// 官方 `ServerLevel$EntityCallbacks.onDestroyed` 调的是 `Scoreboard.entityRemoved`，
    /// 它第一句就是「是 `Player` 就 return」；`ServerScoreboard.onPlayerRemoved` 只由
    /// `resetAllPlayerScores` / `resetSinglePlayerScore` 触发，也就是 `/scoreboard players reset`。
    /// 所以别人侧边栏上这名离线玩家的分数还留着。分数本身当然也不删。
    ///
    /// Nothing is broadcast when a player disconnects.
    ///
    /// Official `ServerLevel$EntityCallbacks.onDestroyed` calls `Scoreboard.entityRemoved`,
    /// which returns immediately for a `Player`; `ServerScoreboard.onPlayerRemoved` is only
    /// reached from `resetAllPlayerScores` / `resetSinglePlayerScore`, i.e.
    /// `/scoreboard players reset`. Other clients keep the offline player's scores.
    pub(crate) fn on_leave(&mut self, name: &str) {
        self.forget_criteria(name);
    }

    /// 官方 `ServerPlayer.doTick` 里的六个只读准则。Bamboo 没有生命、饥饿、氧气、盔甲和
    /// 经验系统，值就取玩家的初值；顺序和官方一样是 health、food、air、armor、xp、level。
    ///
    /// The six read-only criteria from official `ServerPlayer.doTick`. Bamboo has no health,
    /// hunger, air, armor or experience system, so each uses the player's default value;
    /// the order matches official: health, food, air, armor, xp, level.
    pub(crate) fn tick_criteria(&mut self, region: &mut Region) {
        const VALUES: [(&str, i32); 6] = [
            ("health", 20),
            ("food", 20),
            ("air", 300),
            ("armor", 0),
            ("xp", 0),
            ("level", 0),
        ];
        let names: Vec<String> = region
            .players
            .values()
            .map(|player| player.name.clone())
            .collect();
        for name in names {
            // 官方每个字段的初值都不是这些默认值（`Float.MIN_VALUE` 和 `Integer.MIN_VALUE`），
            // 所以进服后的第一刻每个准则都会写一次，之后值没变就不写。
            //
            // Official fields start at `Float.MIN_VALUE` / `Integer.MIN_VALUE`, so every
            // criterion is written once on the first tick and never again while unchanged.
            let recorded = self.recorded.entry(name.clone()).or_insert([i32::MIN; 6]);
            let changed: Vec<(usize, i32)> = VALUES
                .iter()
                .enumerate()
                .filter(|(index, (_, value))| recorded[*index] != *value)
                .map(|(index, (_, value))| (index, *value))
                .collect();
            if changed.is_empty() {
                continue;
            }
            for (index, value) in &changed {
                recorded[*index] = *value;
            }
            let holder = Holder {
                name: name.clone(),
                display: Some(self.format_player_name(&name)),
            };
            for (index, value) in changed {
                let criteria = VALUES[index].0;
                let objectives: Vec<String> = self
                    .objectives
                    .iter()
                    .filter(|objective| objective.criteria == criteria)
                    .map(|objective| objective.name.clone())
                    .collect();
                for objective in objectives {
                    self.write_forced(&holder, &objective, value, region);
                }
            }
        }
    }

    fn forget_criteria(&mut self, name: &str) {
        // 官方新 `ServerPlayer` 实例的六个 `lastRecorded*` 字段都回到构造函数初值。
        //
        // A new official `ServerPlayer` resets all six `lastRecorded*` fields.
        self.recorded.remove(name);
    }

    fn update_team(&mut self, name: &str, region: &mut Region, change: impl FnOnce(&mut Team)) {
        let Some(team) = self.teams.iter_mut().find(|team| team.name == name) else {
            return;
        };
        change(team);
        let team = team.clone();
        send_team_change(region, &team);
    }

    fn objective(&self, name: &str) -> Option<&Objective> {
        self.objectives
            .iter()
            .find(|objective| objective.name == name)
    }

    fn team(&self, name: &str) -> Option<&Team> {
        self.teams.iter().find(|team| team.name == name)
    }

    fn slot_count(&self, name: &str) -> usize {
        self.display
            .iter()
            .filter(|slot| slot.as_deref() == Some(name))
            .count()
    }

    fn ensure_score(&mut self, player: &str, objective: &str) -> bool {
        let scores = self.scores.entry(player.to_owned()).or_default();
        if scores.contains_key(objective) {
            false
        } else {
            scores.insert(objective.to_owned(), Score::new());
            true
        }
    }

    fn opening_value(&mut self, player: &str, objective: &str) -> i32 {
        self.ensure_score(player, objective);
        self.score_value(player, objective).unwrap_or(0)
    }

    fn score_mut(&mut self, player: &str, objective: &str) -> Option<&mut Score> {
        self.scores
            .get_mut(player)
            .and_then(|scores| scores.get_mut(objective))
    }

    fn write_score(
        &mut self,
        holder: &Holder,
        objective: &str,
        value: i32,
        force: bool,
        region: &mut Region,
    ) -> Result<(), ScoreFailure> {
        let fresh = self.ensure_score(&holder.name, objective);
        self.write_with_fresh(holder, objective, value, fresh, force, region)
    }

    fn write_with_fresh(
        &mut self,
        holder: &Holder,
        objective: &str,
        value: i32,
        fresh: bool,
        force: bool,
        region: &mut Region,
    ) -> Result<(), ScoreFailure> {
        let read_only = self.is_read_only(objective);
        if read_only && !force {
            return Err(ScoreFailure::ReadOnly);
        }
        let auto = self
            .objective(objective)
            .is_some_and(|objective| objective.auto_update);
        let Some(score) = self.score_mut(&holder.name, objective) else {
            return Ok(());
        };
        let mut changed = fresh;
        if auto
            && let Some(display) = &holder.display
            && score.display.as_ref() != Some(display)
        {
            score.display = Some(display.clone());
            changed = true;
        }
        if score.value != value {
            score.value = value;
            changed = true;
        }
        if changed {
            self.send_score_now(&holder.name, objective, region);
        }
        Ok(())
    }

    fn send_score_now(&self, player: &str, objective: &str, region: &mut Region) {
        if !self.tracked.contains(objective) {
            return;
        }
        let Some(score) = self
            .scores
            .get(player)
            .and_then(|scores| scores.get(objective))
        else {
            return;
        };
        region.links.broadcast(&encode_frames(&SetScore {
            owner: player,
            objective,
            score: score.value,
            display: score.display.as_ref(),
            number_format: score.format.as_ref(),
        }));
    }

    fn start_tracking(&mut self, name: &str, region: &mut Region) {
        if self.tracked.contains(name) {
            return;
        }
        let Some(objective) = self.objective(name).cloned() else {
            return;
        };
        let mut frames = Vec::new();
        append_start(&mut frames, self, &objective);
        self.tracked.insert(name.to_owned());
        region.links.broadcast(&frames);
    }

    fn stop_tracking(&mut self, name: &str, region: &mut Region) {
        if !self.tracked.remove(name) {
            return;
        }
        let mut frames = encode_frames(&SetObjective {
            name,
            method: 1,
            display: &Component::text(""),
            render_type: 0,
            number_format: None,
        });
        for (slot, shown) in self.display.iter().enumerate() {
            if shown.as_deref() == Some(name) {
                frames.extend(encode_frames(&SetDisplayObjective {
                    slot: slot as i32,
                    objective: name,
                }));
            }
        }
        region.links.broadcast(&frames);
    }
}

fn append_start(frames: &mut Vec<u8>, board: &Scoreboard, objective: &Objective) {
    frames.extend(encode_frames(&SetObjective {
        name: &objective.name,
        method: 0,
        display: &objective.display,
        render_type: objective.render,
        number_format: objective.format.as_ref(),
    }));
    for (slot, shown) in board.display.iter().enumerate() {
        if shown.as_deref() == Some(objective.name.as_str()) {
            frames.extend(encode_frames(&SetDisplayObjective {
                slot: slot as i32,
                objective: &objective.name,
            }));
        }
    }
    for (player, scores) in &board.scores {
        if let Some(score) = scores.get(&objective.name) {
            frames.extend(encode_frames(&SetScore {
                owner: player,
                objective: &objective.name,
                score: score.value,
                display: score.display.as_ref(),
                number_format: score.format.as_ref(),
            }));
        }
    }
}

fn send_objective(region: &mut Region, objective: &Objective, method: u8) {
    region.links.broadcast(&encode_frames(&SetObjective {
        name: &objective.name,
        method,
        display: &objective.display,
        render_type: objective.render,
        number_format: objective.format.as_ref(),
    }));
}

fn send_slot(region: &mut Region, slot: u8, objective: &str) {
    region.links.broadcast(&encode_frames(&SetDisplayObjective {
        slot: i32::from(slot),
        objective,
    }));
}

fn send_reset(region: &mut Region, owner: &str, objective: Option<&str>) {
    region
        .links
        .broadcast(&encode_frames(&ResetScore { owner, objective }));
}

fn send_team_add(region: &mut Region, team: &Team) {
    region.links.broadcast(&encode_frames(&SetPlayerTeam {
        name: &team.name,
        action: TeamAction::Add {
            parameters: team.parameters(),
            players: &team.members,
        },
    }));
}

fn send_team_change(region: &mut Region, team: &Team) {
    region.links.broadcast(&encode_frames(&SetPlayerTeam {
        name: &team.name,
        action: TeamAction::Change {
            parameters: team.parameters(),
        },
    }));
}

fn apply_operation(
    operation: mc_bamboo_command::ScoreOperation,
    left: i32,
    right: i32,
) -> Result<i32, ScoreFailure> {
    Ok(match operation {
        mc_bamboo_command::ScoreOperation::Assign => right,
        mc_bamboo_command::ScoreOperation::Add => left.wrapping_add(right),
        mc_bamboo_command::ScoreOperation::Subtract => left.wrapping_sub(right),
        mc_bamboo_command::ScoreOperation::Multiply => left.wrapping_mul(right),
        mc_bamboo_command::ScoreOperation::Divide => floor_div(left, right)?,
        mc_bamboo_command::ScoreOperation::Modulo => floor_mod(left, right)?,
        mc_bamboo_command::ScoreOperation::Min => left.min(right),
        mc_bamboo_command::ScoreOperation::Max => left.max(right),
        mc_bamboo_command::ScoreOperation::Swap => right,
    })
}

fn floor_div(left: i32, right: i32) -> Result<i32, ScoreFailure> {
    if right == 0 {
        return Err(ScoreFailure::DivideByZero);
    }
    if left == i32::MIN && right == -1 {
        return Err(ScoreFailure::Overflow);
    }
    let (left, right) = (i64::from(left), i64::from(right));
    let quotient = left / right;
    let remainder = left % right;
    let divided = if remainder != 0 && (left < 0) != (right < 0) {
        quotient - 1
    } else {
        quotient
    };
    Ok(divided as i32)
}

fn floor_mod(left: i32, right: i32) -> Result<i32, ScoreFailure> {
    let divided = floor_div(left, right)?;
    Ok(left.wrapping_sub(divided.wrapping_mul(right)))
}
