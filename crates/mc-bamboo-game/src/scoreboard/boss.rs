//! 自定义 Boss 栏。全服一份，不写存档。
//!
//! 官方 `CustomBossEvents` + `CustomBossEvent` + `ServerBossEvent`：每根栏记住一组玩家的
//! UUID，玩家进服时补发 add 包，离开时只清掉在线集合（UUID 集合保留，下次进服还会补发）。
//!
//! Custom boss bars. One set for the whole server, never saved.
//!
//! Official `CustomBossEvents` + `CustomBossEvent` + `ServerBossEvent`: each bar remembers
//! a set of player UUIDs. Joining sends the add packet again, while leaving only clears the
//! online set (the UUID set survives, so rejoining still receives the add packet).

use std::collections::{BTreeMap, BTreeSet};

use mc_bamboo_protocol::{
    packet::play::{BossAction, BossEvent},
    text::Component,
};
use uuid::Uuid;

use crate::{inbound::PlayerId, links::encode_frames, region::Region};

/// 官方 `CustomBossEvent` 的初值：白色、progress 样式、0/100、可见。
///
/// Official `CustomBossEvent` defaults: white, progress overlay, 0/100, visible.
const DEFAULT_MAX: i32 = 100;

/// Boss 栏颜色，id 是官方 `BossBarColor` 的枚举顺序。
///
/// A boss-bar color. The id is the ordinal of the official `BossBarColor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BossColor {
    Pink,
    Blue,
    Red,
    Green,
    Yellow,
    Purple,
    White,
}

impl BossColor {
    pub(crate) const ALL: [Self; 7] = [
        Self::Pink,
        Self::Blue,
        Self::Red,
        Self::Green,
        Self::Yellow,
        Self::Purple,
        Self::White,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Pink => "pink",
            Self::Blue => "blue",
            Self::Red => "red",
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Purple => "purple",
            Self::White => "white",
        }
    }

    pub(crate) fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|color| color.name() == name)
    }

    const fn id(self) -> i32 {
        self as i32
    }
}

/// Boss 栏的分段样式，id 是官方 `BossBarOverlay` 的枚举顺序。
///
/// A boss-bar overlay. The id is the ordinal of the official `BossBarOverlay`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BossStyle {
    Progress,
    Notched6,
    Notched10,
    Notched12,
    Notched20,
}

impl BossStyle {
    pub(crate) const ALL: [Self; 5] = [
        Self::Progress,
        Self::Notched6,
        Self::Notched10,
        Self::Notched12,
        Self::Notched20,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Progress => "progress",
            Self::Notched6 => "notched_6",
            Self::Notched10 => "notched_10",
            Self::Notched12 => "notched_12",
            Self::Notched20 => "notched_20",
        }
    }

    pub(crate) fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|style| style.name() == name)
    }

    const fn id(self) -> i32 {
        self as i32
    }
}

/// 一根 Boss 栏。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BossBar {
    pub uuid: Uuid,
    pub id: String,
    pub name: Component,
    pub color: BossColor,
    pub style: BossStyle,
    pub value: i32,
    pub max: i32,
    pub visible: bool,
    /// 官方 `CustomBossEvent.players`：UUID 集合，离线也留着。
    ///
    /// Official `CustomBossEvent.players`: UUIDs, kept while a player is offline.
    pub selected: BTreeSet<Uuid>,
    /// 官方 `ServerBossEvent.players`：当前在线且收到过 add 包的玩家。
    ///
    /// Official `ServerBossEvent.players`: online players that got the add packet.
    pub online: BTreeSet<PlayerId>,
}

impl BossBar {
    fn new(uuid: Uuid, id: String, name: Component) -> Self {
        Self {
            uuid,
            id,
            name,
            color: BossColor::White,
            style: BossStyle::Progress,
            value: 0,
            max: DEFAULT_MAX,
            visible: true,
            selected: BTreeSet::new(),
            online: BTreeSet::new(),
        }
    }

    /// 官方 `CustomBossEvent.setValue/setMax`：`value / max` 压到 `[0, 1]`。
    ///
    /// Official `CustomBossEvent.setValue/setMax`: `value / max` clamped to `[0, 1]`.
    fn progress(&self) -> f32 {
        if self.max <= 0 {
            return 0.0;
        }
        (self.value as f32 / self.max as f32).clamp(0.0, 1.0)
    }

    /// 官方 `CustomBossEvent.getDisplayName`。
    ///
    /// Official `CustomBossEvent.getDisplayName`.
    pub(crate) fn display_name(&self) -> Component {
        super::format::boss_name(&self.id, &self.name, self.color.id() as u8)
    }

    fn add_frame(&self) -> Vec<u8> {
        encode_frames(&BossEvent {
            id: self.uuid,
            action: BossAction::Add {
                name: &self.name,
                progress: self.progress(),
                color: self.color.id(),
                overlay: self.style.id(),
                flags: 0,
            },
        })
    }

    fn remove_frame(&self) -> Vec<u8> {
        encode_frames(&BossEvent {
            id: self.uuid,
            action: BossAction::Remove,
        })
    }

    fn update_progress_frame(&self) -> Vec<u8> {
        encode_frames(&BossEvent {
            id: self.uuid,
            action: BossAction::UpdateProgress(self.progress()),
        })
    }

    fn update_name_frame(&self) -> Vec<u8> {
        encode_frames(&BossEvent {
            id: self.uuid,
            action: BossAction::UpdateName(&self.name),
        })
    }

    fn update_style_frame(&self) -> Vec<u8> {
        encode_frames(&BossEvent {
            id: self.uuid,
            action: BossAction::UpdateStyle {
                color: self.color.id(),
                overlay: self.style.id(),
            },
        })
    }
}

/// 全服的自定义 Boss 栏。下标是官方 `Identifier` 的字符串形式。
///
/// All custom boss bars. Keys are the string form of the official `Identifier`.
#[derive(Debug, Default)]
pub(crate) struct BossBars {
    bars: BTreeMap<String, BossBar>,
    /// 新栏 UUID 只要求互不重复，且和存档无关。
    ///
    /// New bar UUIDs only have to be distinct; they are not persisted.
    next_uuid: u64,
}

impl BossBars {
    fn fresh_uuid(&mut self) -> Uuid {
        self.next_uuid = self.next_uuid.wrapping_add(1);
        Uuid::from_u128(u128::from(self.next_uuid) | 0x6261_6d62_6f6f_0000_0000_0000_0000_0000)
    }

    pub(crate) fn ids(&self) -> Vec<String> {
        self.bars.keys().cloned().collect()
    }

    pub(crate) fn bar(&self, id: &str) -> Option<&BossBar> {
        self.bars.get(id)
    }

    /// 官方 `CustomBossEvent.getDisplayName`，列表和消息里都用这个。
    ///
    /// Official `CustomBossEvent.getDisplayName`, used by lists and messages.
    pub(crate) fn display_name(&self, id: &str) -> Option<Component> {
        self.bars.get(id).map(BossBar::display_name)
    }

    /// 官方 `getEvents`：按 id 排序的显示名列表。
    ///
    /// Official `getEvents`: display names sorted by id.
    pub(crate) fn entries(&self) -> Vec<Component> {
        self.bars.values().map(BossBar::display_name).collect()
    }

    pub(crate) fn value(&self, id: &str) -> Option<i32> {
        self.bars.get(id).map(|bar| bar.value)
    }

    pub(crate) fn max(&self, id: &str) -> Option<i32> {
        self.bars.get(id).map(|bar| bar.max)
    }

    pub(crate) fn visible(&self, id: &str) -> Option<bool> {
        self.bars.get(id).map(|bar| bar.visible)
    }

    pub(crate) fn color(&self, id: &str) -> Option<BossColor> {
        self.bars.get(id).map(|bar| bar.color)
    }

    pub(crate) fn style(&self, id: &str) -> Option<BossStyle> {
        self.bars.get(id).map(|bar| bar.style)
    }

    pub(crate) fn online_count(&self, id: &str) -> Option<usize> {
        self.bars.get(id).map(|bar| bar.online.len())
    }

    fn bar_mut(&mut self, id: &str) -> Option<&mut BossBar> {
        self.bars.get_mut(id)
    }

    /// 官方 `CustomBossEvents.create`。重复 id 返回 `false`。
    ///
    /// Official `CustomBossEvents.create`. A duplicate id returns `false`.
    pub(crate) fn create(&mut self, id: String, name: Component) -> bool {
        if self.bars.contains_key(&id) {
            return false;
        }
        let uuid = self.fresh_uuid();
        self.bars.insert(id.clone(), BossBar::new(uuid, id, name));
        true
    }

    /// 官方 `BossBarCommands.removeBar`：先 `removeAllPlayers` 再删掉。
    ///
    /// Official `BossBarCommands.removeBar`: `removeAllPlayers` first, then remove.
    pub(crate) fn remove(&mut self, id: &str, region: &mut Region) -> bool {
        let Some(mut bar) = self.bars.remove(id) else {
            return false;
        };
        self.remove_all(&mut bar, region);
        true
    }

    fn remove_all(&self, bar: &mut BossBar, region: &mut Region) {
        if !bar.visible {
            bar.online.clear();
            return;
        }
        let frame = bar.remove_frame();
        for player in std::mem::take(&mut bar.online) {
            region.links.send_frames(player, &frame);
        }
    }

    /// 官方 `CustomBossEvents.onPlayerConnect`。
    ///
    /// Official `CustomBossEvents.onPlayerConnect`.
    pub(crate) fn on_join(&mut self, region: &mut Region, id: PlayerId, uuid: Uuid) {
        for bar in self.bars.values_mut() {
            if bar.selected.contains(&uuid) {
                bar.online.insert(id);
                if bar.visible && region.links.is_open(id) {
                    let frame = bar.add_frame();
                    region.links.send_frames(id, &frame);
                }
            }
        }
    }

    /// 官方 `CustomBossEvents.onPlayerDisconnect`：不发包，只清在线集合。
    ///
    /// Official `CustomBossEvents.onPlayerDisconnect`: no packets, just clear online.
    pub(crate) fn on_leave(&mut self, id: PlayerId) {
        for bar in self.bars.values_mut() {
            bar.online.remove(&id);
        }
    }

    /// 官方 `CustomBossEvent.setPlayers`。返回是否真的有人进/出。
    ///
    /// Official `CustomBossEvent.setPlayers`. Returns whether anyone was added or removed.
    pub(crate) fn set_players(
        &mut self,
        id: &str,
        targets: &[(PlayerId, Uuid)],
        region: &mut Region,
    ) -> Option<bool> {
        let wanted: BTreeSet<Uuid> = targets.iter().map(|(_, uuid)| *uuid).collect();
        let bar = self.bars.get_mut(id)?;
        let mut changed = false;
        let gone: Vec<PlayerId> = bar
            .online
            .iter()
            .copied()
            .filter(|player| {
                region
                    .players
                    .get(player)
                    .is_some_and(|entry| !wanted.contains(&entry.uuid))
            })
            .collect();
        let remove_frame = bar.visible.then(|| bar.remove_frame());
        for player in gone {
            bar.online.remove(&player);
            if let Some(frame) = &remove_frame {
                region.links.send_frames(player, frame);
            }
            changed = true;
        }
        let add_frame = bar.visible.then(|| bar.add_frame());
        for (player, uuid) in targets {
            if !region.players.contains_key(player) {
                continue;
            }
            if bar.online.insert(*player) {
                if let Some(frame) = &add_frame {
                    region.links.send_frames(*player, frame);
                }
                changed = true;
            }
            bar.selected.insert(*uuid);
        }
        // 官方 `setPlayers` 把离线的旧 UUID 也移出集合，只看 UUID 集合有没有变。
        //
        // Official `setPlayers` also removes stale offline UUIDs; any change counts.
        if bar.selected != wanted {
            bar.selected = wanted;
            changed = true;
        }
        Some(changed)
    }

    /// 在线玩家名，按 `ComponentUtils.formatList(Collection<String>)` 排序涂绿。
    ///
    /// Online player names, sorted and colored green as in
    /// `ComponentUtils.formatList(Collection<String>)`.
    pub(crate) fn online_names(&self, id: &str, region: &Region) -> Option<Vec<String>> {
        let bar = self.bars.get(id)?;
        let mut names: Vec<String> = bar
            .online
            .iter()
            .filter_map(|player| region.players.get(player).map(|entry| entry.name.clone()))
            .collect();
        names.sort();
        Some(names)
    }

    pub(crate) fn set_visible(
        &mut self,
        id: &str,
        visible: bool,
        region: &mut Region,
    ) -> Option<bool> {
        let bar = self.bar_mut(id)?;
        if bar.visible == visible {
            return Some(false);
        }
        bar.visible = visible;
        let frame = if visible {
            bar.add_frame()
        } else {
            bar.remove_frame()
        };
        for player in &bar.online {
            region.links.send_frames(*player, &frame);
        }
        Some(true)
    }

    pub(crate) fn set_value(&mut self, id: &str, value: i32, region: &mut Region) -> Option<bool> {
        let bar = self.bar_mut(id)?;
        if bar.value == value {
            return Some(false);
        }
        bar.value = value;
        Self::broadcast(bar, region, BossBar::update_progress_frame);
        Some(true)
    }

    pub(crate) fn set_max(&mut self, id: &str, max: i32, region: &mut Region) -> Option<bool> {
        let bar = self.bar_mut(id)?;
        if bar.max == max {
            return Some(false);
        }
        bar.max = max;
        Self::broadcast(bar, region, BossBar::update_progress_frame);
        Some(true)
    }

    pub(crate) fn set_color(
        &mut self,
        id: &str,
        color: BossColor,
        region: &mut Region,
    ) -> Option<bool> {
        let bar = self.bar_mut(id)?;
        if bar.color == color {
            return Some(false);
        }
        bar.color = color;
        Self::broadcast(bar, region, BossBar::update_style_frame);
        Some(true)
    }

    pub(crate) fn set_style(
        &mut self,
        id: &str,
        style: BossStyle,
        region: &mut Region,
    ) -> Option<bool> {
        let bar = self.bar_mut(id)?;
        if bar.style == style {
            return Some(false);
        }
        bar.style = style;
        Self::broadcast(bar, region, BossBar::update_style_frame);
        Some(true)
    }

    pub(crate) fn set_name(
        &mut self,
        id: &str,
        name: Component,
        region: &mut Region,
    ) -> Option<bool> {
        let bar = self.bar_mut(id)?;
        if bar.name == name {
            return Some(false);
        }
        bar.name = name;
        Self::broadcast(bar, region, BossBar::update_name_frame);
        Some(true)
    }

    /// 官方 `ServerBossEvent.broadcast`：不可见时一个包都不发。
    ///
    /// Official `ServerBossEvent.broadcast`: nothing is sent while hidden.
    fn broadcast(bar: &BossBar, region: &mut Region, frame: fn(&BossBar) -> Vec<u8>) {
        if !bar.visible {
            return;
        }
        let bytes = frame(bar);
        for player in &bar.online {
            region.links.send_frames(*player, &bytes);
        }
    }
}
