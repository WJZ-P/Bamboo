//! 每名玩家的进度运行时状态、监听表、可见性和发包，官方 `PlayerAdvancements`。
//!
//! 树是全服共享的只读数据（`mc-bamboo-advancements`）；这里的状态挂在 `Player` 上，
//! 随玩家在岛之间搬动。触发器判定在 [`crate::advancement_triggers`]，奖励与公告在
//! [`crate::advancement_rewards`]，命令在 [`crate::command::advancement`]。
//!
//! Per-player advancement state, listener registration, visibility and packets.
//! The shared tree is read-only; all mutable state moves with the player.

use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
use chrono::TimeZone;

use mc_bamboo_advancements::{
    AdvancementTree, Frame,
    progress_file::{AdvancementProgress, ProgressFile},
};
use mc_bamboo_protocol::packet::play::{
    ProgressEntry, SeenAdvancements, SelectAdvancementsTab, UpdateAdvancements,
};

use crate::{chat, inbound::PlayerId, links::Links, player::Player, region::Region};

/// 触发器类型 → `(进度下标, 判据名, 条件)` 的监听表。
/// 和官方 `activeTriggers` 一样只留还没完成的判据。
#[derive(Debug, Clone, Default)]
pub(crate) struct Listeners {
    /// 键是 `minecraft:trigger_type` 注册表里的完整名字。
    pub by_trigger: BTreeMap<String, Vec<(usize, String)>>,
}

/// 一名玩家的进度状态。官方 `PlayerAdvancements` 的可变部分。
#[derive(Debug, Clone)]
pub(crate) struct PlayerAdvancements {
    /// 进度下标 → 进度状态。官方是 LinkedHashMap，这里按 id 排序保证三种岛模式一致。
    pub progress: BTreeMap<usize, AdvancementProgress>,
    /// 当前客户端能看见的进度下标。
    pub visible: BTreeSet<usize>,
    /// 进度发生变化、下次 flush 要发给客户端的进度。
    pub progress_changed: BTreeSet<usize>,
    /// 要重新算可见性的根。
    pub roots_to_update: BTreeSet<usize>,
    pub listeners: Listeners,
    /// 官方 `isFirstPacket`：进服时第一包带 `reset=true`。
    pub first_packet: bool,
    /// 官方 `lastSelectedTab`。
    pub selected_tab: Option<usize>,
    /// 已完成但还没发过公告的显示进度；奖励在 `award` 当场发。
    pub known_recipes: BTreeSet<String>,
    /// 首个玩家 tick 的无条件进度是否已经处理过。
    pub automatic_done: bool,
}

/// 判据完成时刻。官方用 `Instant.now()`（墙上时钟）；Bamboo 用同一个钟表，
/// 但在 `cfg(test)` 下固定成一个常量，让「同种子字节一致」和三种岛模式的对拍可复现。
fn now() -> chrono::DateTime<chrono::Local> {
    #[cfg(test)]
    {
        chrono::Local
            .timestamp_opt(1_700_000_000, 0)
            .single()
            .unwrap_or_else(chrono::Local::now)
    }
    #[cfg(not(test))]
    {
        chrono::Local::now()
    }
}

impl Default for PlayerAdvancements {
    fn default() -> Self {
        Self {
            progress: BTreeMap::new(),
            visible: BTreeSet::new(),
            progress_changed: BTreeSet::new(),
            roots_to_update: BTreeSet::new(),
            listeners: Listeners::default(),
            first_packet: true,
            selected_tab: None,
            known_recipes: BTreeSet::new(),
            automatic_done: false,
        }
    }
}

impl PlayerAdvancements {
    /// 从官方格式的进度档案构造。未知进度、未知判据照官方忽略 / 按定义补齐。
    pub(crate) fn from_file(tree: &AdvancementTree, text: Option<&str>) -> Self {
        let mut state = Self::default();
        let file = text
            .and_then(|text| ProgressFile::decode(text).ok())
            .unwrap_or_default();
        for (id, mut progress) in file.progress {
            let Some(index) = tree.index(id.as_str()) else {
                // 官方打一条 warn 后跳过，这里同样略过。
                continue;
            };
            progress.update(&tree.nodes[index].advancement.requirements);
            state.progress.insert(index, progress);
            state.progress_changed.insert(index);
            state.roots_to_update.insert(tree.root_of(index));
        }
        state.rebuild_listeners(tree);
        // 首次包的可见性由 flush 里的 evaluateVisibility 从每个根算起。
        for &root in &tree.roots {
            state.roots_to_update.insert(root);
        }
        state
    }

    /// 写回官方格式的进度档案。只含有过进度的条目；配方书集合也一起写进玩家数据
    /// （第二期配方书包的接口），这里挂在存档会话上。
    pub(crate) fn to_file(&self, tree: &AdvancementTree) -> String {
        let mut file = ProgressFile::new();
        for (&index, progress) in &self.progress {
            if progress.has_progress() {
                file.progress
                    .insert(tree.nodes[index].id.to_owned(), progress.clone());
            }
        }
        file.encode()
    }

    /// 取（或新建）一个进度的状态，照官方 `getOrStartProgress`。
    fn get_or_start(&mut self, tree: &AdvancementTree, index: usize) -> &mut AdvancementProgress {
        self.progress
            .entry(index)
            .or_insert_with(|| AdvancementProgress::start(&tree.nodes[index].advancement))
    }

    /// 官方 `award`。返回是否发生变化；完成时由调用方发奖励和公告。
    pub(crate) fn award(&mut self, tree: &AdvancementTree, index: usize, criterion: &str) -> bool {
        let was_done = self.get_or_start(tree, index).is_done();
        let now = now();
        let changed = self.get_or_start(tree, index).grant(criterion, now);
        if changed {
            let done = self.progress[&index].is_done();
            self.unregister_listeners(tree, index);
            self.progress_changed.insert(index);
            if !was_done && done {
                self.roots_to_update.insert(tree.root_of(index));
            }
        }
        changed
    }

    /// 官方 `revoke`。返回是否发生变化。
    pub(crate) fn revoke(&mut self, tree: &AdvancementTree, index: usize, criterion: &str) -> bool {
        let was_done = self.get_or_start(tree, index).is_done();
        let changed = self.get_or_start(tree, index).revoke(criterion);
        if changed {
            let done = self.progress[&index].is_done();
            self.register_listeners(tree, index);
            self.progress_changed.insert(index);
            if was_done && !done {
                self.roots_to_update.insert(tree.root_of(index));
            }
        }
        changed
    }

    /// 官方 `registerListeners`：已完成的进度不再监听，只留没完成的判据。
    fn register_listeners(&mut self, tree: &AdvancementTree, index: usize) {
        let progress = self.get_or_start(tree, index);
        let done = progress.is_done();
        let pending: Vec<(String, String)> = tree.nodes[index]
            .advancement
            .criteria
            .iter()
            .filter(|(name, _)| !done && !progress.is_criterion_done(name))
            .map(|(name, criterion)| (criterion.trigger.clone(), name.clone()))
            .collect();
        for (trigger, name) in pending {
            self.listeners
                .by_trigger
                .entry(trigger)
                .or_default()
                .push((index, name));
        }
    }

    /// 官方 `unregisterListeners`：删掉这个进度下已完成判据（或整个进度已完成）的监听。
    fn unregister_listeners(&mut self, tree: &AdvancementTree, index: usize) {
        let progress = self.get_or_start(tree, index);
        let done = progress.is_done();
        let finished: Vec<(String, String)> = tree.nodes[index]
            .advancement
            .criteria
            .iter()
            .filter(|(name, _)| done || progress.is_criterion_done(name))
            .map(|(name, criterion)| (criterion.trigger.clone(), name.clone()))
            .collect();
        for (trigger, name) in finished {
            if let Some(list) = self.listeners.by_trigger.get_mut(&trigger) {
                list.retain(|(other, criterion_name)| *other != index || criterion_name != &name);
            }
        }
        self.listeners.by_trigger.retain(|_, list| !list.is_empty());
    }

    /// 官方 `reload` 之后和读档之后重建整张监听表。
    pub(crate) fn rebuild_listeners(&mut self, tree: &AdvancementTree) {
        self.listeners = Listeners::default();
        // `registerListeners` 对所有节点都要跑，不只显示的。
        for index in 0..tree.nodes.len() {
            self.register_listeners(tree, index);
        }
    }

    /// 官方 `checkForAutomaticTriggers`：没有判据的进度自动完成并给奖励。
    pub(crate) fn check_automatic(&mut self, tree: &AdvancementTree) -> Vec<usize> {
        let mut completed = Vec::new();
        if self.automatic_done {
            return completed;
        }
        self.automatic_done = true;
        for index in 0..tree.nodes.len() {
            if tree.nodes[index].advancement.criteria.is_empty() {
                self.get_or_start(tree, index);
                self.progress_changed.insert(index);
                self.roots_to_update.insert(tree.root_of(index));
                completed.push(index);
            }
        }
        completed
    }

    /// 官方 `AdvancementVisibilityEvaluator.evaluateVisibility`。
    /// 从根开始的栈里最多向上看两级：完成 → SHOW，无显示 → HIDE，hidden → HIDE，
    /// 否则 NO_CHANGE；本节点或子树有完成的就可见。
    fn evaluate_visibility(
        &mut self,
        tree: &AdvancementTree,
        index: usize,
        stack: &mut Vec<VisibilityRule>,
        changed: &mut BTreeSet<usize>,
        removed: &mut BTreeSet<usize>,
    ) -> bool {
        let done = self
            .progress
            .get(&index)
            .is_some_and(AdvancementProgress::is_done);
        let rule = match tree.nodes[index].advancement.display.as_ref() {
            None => VisibilityRule::Hide,
            Some(_) if done => VisibilityRule::Show,
            Some(display) if display.hidden => VisibilityRule::Hide,
            Some(_) => VisibilityRule::NoChange,
        };
        stack.push(rule);
        let mut any_done = done;
        for &child in &tree.nodes[index].children {
            any_done |= self.evaluate_visibility(tree, child, stack, changed, removed);
        }
        let visible = any_done || unfinished_visible(stack);
        stack.pop();
        if visible {
            if self.visible.insert(index) && self.progress.contains_key(&index) {
                changed.insert(index);
            }
        } else if self.visible.remove(&index) {
            removed.insert(index);
        }
        any_done
    }

    /// 官方 `flushDirty`：把这一刻的变化编成一个 `UpdateAdvancements` 包。
    /// 返回要发给这名玩家的包内容；没有变化时不发包。
    pub(crate) fn flush(&mut self, tree: &AdvancementTree, links: &mut Links, id: PlayerId) {
        if !self.first_packet && self.roots_to_update.is_empty() && self.progress_changed.is_empty()
        {
            self.first_packet = false;
            return;
        }
        let mut changed = BTreeSet::new();
        let mut removed = BTreeSet::new();
        let roots: Vec<usize> = self.roots_to_update.iter().copied().collect();
        self.roots_to_update.clear();
        for root in roots {
            let mut stack = vec![
                VisibilityRule::NoChange,
                VisibilityRule::NoChange,
                VisibilityRule::NoChange,
            ];
            let root = tree.root_of(root);
            self.evaluate_visibility(tree, root, &mut stack, &mut changed, &mut removed);
        }
        for &index in &self.progress_changed {
            if self.visible.contains(&index) {
                changed.insert(index);
            }
        }
        self.progress_changed.clear();
        if changed.is_empty() && self.visible.is_empty() && removed.is_empty() {
            self.first_packet = false;
            return;
        }
        // added：首包发全部当前可见的显示节点，之后只发新变可见的。
        let added: Vec<usize> = if self.first_packet {
            tree.displayed
                .iter()
                .copied()
                .filter(|&index| self.visible.contains(&index))
                .collect()
        } else {
            changed
                .iter()
                .copied()
                .filter(|&index| {
                    tree.nodes[index].advancement.display.is_some() && self.visible.contains(&index)
                })
                .collect()
        };
        let mut definitions = Vec::with_capacity(added.len());
        let positioned: Vec<mc_bamboo_protocol::packet::play::PositionedAdvancement<'_>> = added
            .iter()
            .map(|&index| {
                definitions.push(tree.nodes[index].advancement.write_network_bytes());
                mc_bamboo_protocol::packet::play::PositionedAdvancement {
                    id: tree.nodes[index].id,
                    definition: &[],
                    x: tree.nodes[index].x,
                    y: tree.nodes[index].y,
                }
            })
            .collect();
        let positioned: Vec<_> = positioned
            .into_iter()
            .enumerate()
            .map(|(i, mut item)| {
                item.definition = &definitions[i];
                item
            })
            .collect();
        let removed_ids: Vec<&str> = removed.iter().map(|&index| tree.nodes[index].id).collect();
        let mut progress_bytes = Vec::with_capacity(changed.len());
        let progress_entries: Vec<ProgressEntry<'_>> = changed
            .iter()
            .filter(|&&index| self.visible.contains(&index))
            .map(|&index| {
                progress_bytes.push(self.progress[&index].write_network_bytes());
                ProgressEntry {
                    id: tree.nodes[index].id,
                    progress: &[],
                }
            })
            .collect();
        let progress_entries: Vec<_> = progress_entries
            .into_iter()
            .enumerate()
            .map(|(i, mut entry)| {
                entry.progress = &progress_bytes[i];
                entry
            })
            .collect();
        if positioned.is_empty() && removed_ids.is_empty() && progress_entries.is_empty() {
            self.first_packet = false;
            return;
        }
        links.send(
            id,
            &UpdateAdvancements {
                reset: self.first_packet,
                added: &positioned,
                removed: &removed_ids,
                progress: &progress_entries,
                show_advancements: true,
            },
        );
        self.first_packet = false;
    }

    /// 官方 `setSelectedTab`：只接受有显示的根；变化时发一包。
    pub(crate) fn select_tab(
        &mut self,
        tree: &AdvancementTree,
        links: &mut Links,
        id: PlayerId,
        tab: Option<usize>,
    ) {
        let accepted = tab.filter(|&index| tree.nodes[index].advancement.is_root());
        if accepted == self.selected_tab {
            return;
        }
        self.selected_tab = accepted;
        links.send(
            id,
            &SelectAdvancementsTab {
                tab: accepted.map(|index| tree.nodes[index].id),
            },
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisibilityRule {
    Hide,
    Show,
    NoChange,
}

/// 官方 `evaluateVisiblityForUnfinishedNode`：从栈顶（当前节点）往上最多看两级。
fn unfinished_visible(stack: &[VisibilityRule]) -> bool {
    for &rule in stack.iter().rev().take(3) {
        match rule {
            VisibilityRule::Show => return true,
            VisibilityRule::Hide => return false,
            VisibilityRule::NoChange => {}
        }
    }
    false
}

/// 客户端打开 / 关闭进度界面。
pub(crate) fn on_seen_advancements(region: &mut Region, id: PlayerId, seen: &SeenAdvancements) {
    let tree = AdvancementTree::global();
    let tab = match seen {
        SeenAdvancements::OpenedTab(id) => tree.index(id.as_str()),
        SeenAdvancements::ClosedScreen => None,
    };
    if let Some(player) = region.players.get_mut(&id) {
        let mut state = std::mem::take(&mut player.advancements);
        state.select_tab(tree, &mut region.links, id, tab);
        player.advancements = state;
    }
}

/// 玩家 tick 里跑一次：无条件进度、触发器 flush。官方 `ServerPlayer.doTick`
/// 每刻调 TICK 触发器，LOCATION 每 20 刻一次。
pub(crate) fn tick(region: &mut Region) {
    let tree = AdvancementTree::global();
    let ids: Vec<PlayerId> = region.players.keys().copied().collect();
    let every_20 = region.tick.game_time() % 20 == 0;
    for id in ids {
        // 无条件进度与触发器各自只短时间借用玩家表。
        let automatic = region
            .players
            .get_mut(&id)
            .map(|player| player.advancements.check_automatic(tree))
            .unwrap_or_default();
        for index in automatic {
            crate::advancement_rewards::grant(region, id, index);
        }
        crate::advancement_triggers::trigger_tick(region, id);
        if every_20 {
            crate::advancement_triggers::trigger_location(region, id);
        }
        let Some(player) = region.players.get_mut(&id) else {
            continue;
        };
        let mut state = std::mem::take(&mut player.advancements);
        state.flush(tree, &mut region.links, id);
        player.advancements = state;
    }
}

/// 进服时从磁盘读出进度档案。连接层在 JoinRequest 里带进来。
pub(crate) fn join_state(text: Option<&str>) -> PlayerAdvancements {
    PlayerAdvancements::from_file(AdvancementTree::global(), text)
}

/// 存盘 / 退出时写进度档案。世界线程只编码，磁盘 IO 在连接层 / 存档会话里做。
pub(crate) fn encode_save(player: &Player) -> String {
    player.advancements.to_file(AdvancementTree::global())
}

/// 公告：官方 `AdvancementType.createAnnouncement` + `PlayerList.broadcastSystemMessage`。
/// 规则 `minecraft:show_advancement_messages` 关掉时不发。
pub(crate) fn announce(region: &mut Region, player_id: PlayerId, index: usize) {
    let tree = AdvancementTree::global();
    let Some(display) = &tree.nodes[index].advancement.display else {
        return;
    };
    if !display.announce_to_chat
        || !region
            .global
            .rules
            .bool(mc_bamboo_data::gamerules::SHOW_ADVANCEMENT_MESSAGES)
    {
        return;
    }
    let Some(player) = region.players.get(&player_id) else {
        return;
    };
    let name = player.name.clone();
    let title = display.title.clone();
    let content = mc_bamboo_protocol::text::Component::translate(
        display.frame.announcement_key(),
        [name.into(), title.into()],
    );
    let mut frames = Vec::new();
    mc_bamboo_protocol::frame::append_frame(
        &mut frames,
        &mc_bamboo_protocol::packet::play::SystemChat {
            content: &content,
            overlay: false,
        },
    )
    .ok();
    chat::broadcast_system_message(region, &frames, None);
    let _ = Frame::Task;
}
