//! `VaultBlockEntity` / `VaultConfig` / `VaultServerData` / `VaultSharedData` /
//! `VaultState` 的服务端状态机。
//!
//! 官方把一个宝库分成方块状态里的 [`VaultState`]、两份数据（`ServerData` 只在服务端、
//! `SharedData` 会同步）和一份配置。这里照 26.3 的字节码把逐刻逻辑、插钥匙解锁、
//! 奖励弹出和存档收在纯逻辑里，世界副作用放进 [`VaultContext`]，既能挂到方块实体的
//! ticker 上，也能逐刻对拍。
//!
//! Server-side state machine of the vault. Vanilla splits it into a [`VaultState`]
//! (block state), two data records (`ServerData` server-only, `SharedData` synced)
//! and a config. Per-tick logic, key insertion, reward ejection and persistence
//! live here; world side effects go through [`VaultContext`].

use mc_bamboo_core::BlockPos;
use mc_bamboo_nbt::Tag;
use mc_bamboo_protocol::{components::archive::slots, item::ItemStack};

use crate::{
    archive::tag::{self, field, put},
    state::VaultState,
};

/// 官方 `VaultConfig.DEFAULT`。
pub const DEFAULT_LOOT_TABLE: &str = "minecraft:chests/trial_chambers/reward";
pub const DEFAULT_ACTIVATION_RANGE: f64 = 4.0;
pub const DEFAULT_DEACTIVATION_RANGE: f64 = 4.5;
/// `VaultSharedData.connectedParticlesRange` 的初值取 `VaultConfig.DEFAULT`。
pub const DEFAULT_PARTICLES_RANGE: f64 = 4.5;
/// `Server.tick` 里展示物品轮换 / 状态更新的周期（刻）。
const DISPLAY_CYCLE_TICK_RATE: i64 = 20;
/// `Server.unlock` 里解锁动画的等待（刻）。
const UNLOCKING_DELAY_TICKS: i64 = 14;
/// `Server.playInsertFailSound` 的节流（刻）。
const INSERT_FAIL_SOUND_BUFFER_TICKS: i64 = 15;
/// `addToRewardedPlayers` 的容量上限。
const MAX_REWARD_PLAYERS: usize = 128;

/// `LevelEvent` 里宝库用的编号。
pub mod level_event {
    /// 进入 `ACTIVE`。
    pub const ACTIVATE: i32 = 3015;
    /// 进入 `INACTIVE`。
    pub const DEACTIVATE: i32 = 3016;
    /// 弹出一件奖励。
    pub const EJECT_ITEM: i32 = 3017;
}

/// 状态转移 / 交互要发的声音（由 [`VaultContext`] 映射到具体音效 ID）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultSound {
    InsertItem,
    InsertItemFail,
    RejectRewardedPlayer,
    OpenShutter,
    CloseShutter,
}

/// `VaultConfig`：五个字段，缺省取 [`DEFAULT`] 那份。
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub loot_table: String,
    pub activation_range: f64,
    pub deactivation_range: f64,
    pub key_item: ItemStack,
    pub override_loot_table_to_display: Option<String>,
}

/// `VaultConfig.DEFAULT`。
pub fn default_config() -> Config {
    Config {
        loot_table: DEFAULT_LOOT_TABLE.to_owned(),
        activation_range: DEFAULT_ACTIVATION_RANGE,
        deactivation_range: DEFAULT_DEACTIVATION_RANGE,
        key_item: ItemStack::new(trial_key_item(), 1),
        override_loot_table_to_display: None,
    }
}

/// `Items.TRIAL_KEY` 的注册表 ID；数据没加载时退到 `AIR`（空堆）。
fn trial_key_item() -> i32 {
    mc_bamboo_data::items::TRIAL_KEY
}

impl Config {
    /// `VaultConfig.CODEC` 的宽松解码 + `validate`（激活范围不得大于失效范围）。
    pub fn read(tag: Option<&Tag>) -> Config {
        let default = default_config();
        let Some(Tag::Compound(values)) = tag else {
            return default;
        };
        let get = |key: &str| {
            values
                .iter()
                .rev()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value)
        };
        let loot_table = get("loot_table")
            .and_then(identifier)
            .unwrap_or(default.loot_table.clone());
        let activation_range = get("activation_range")
            .and_then(|t| tag::java_f64(t).ok())
            .unwrap_or(default.activation_range);
        let deactivation_range = get("deactivation_range")
            .and_then(|t| tag::java_f64(t).ok())
            .unwrap_or(default.deactivation_range);
        // 有 `config` 复合标签但缺 `key_item` 时官方解码到 EMPTY（不是默认钥匙）。
        let key_item = match get("key_item") {
            Some(value) => slots::decode_item(value).unwrap_or_else(|_| ItemStack::empty()),
            None => ItemStack::empty(),
        };
        let override_loot_table_to_display =
            get("override_loot_table_to_display").and_then(identifier);
        if activation_range > deactivation_range {
            return default;
        }
        Config {
            loot_table,
            activation_range,
            deactivation_range,
            key_item,
            override_loot_table_to_display,
        }
    }

    /// `VaultConfig.CODEC` 的重新编码：等于默认值的字段省略。
    fn to_tag(&self) -> Tag {
        let default = default_config();
        let mut out = Vec::new();
        if self.loot_table != default.loot_table {
            put(&mut out, "loot_table", Tag::String(self.loot_table.clone()));
        }
        if self.activation_range.to_bits() != default.activation_range.to_bits() {
            put(
                &mut out,
                "activation_range",
                Tag::Double(self.activation_range),
            );
        }
        if self.deactivation_range.to_bits() != default.deactivation_range.to_bits() {
            put(
                &mut out,
                "deactivation_range",
                Tag::Double(self.deactivation_range),
            );
        }
        if !self.key_item.is_empty()
            && let Ok(encoded) = slots::encode_item(&self.key_item)
        {
            put(&mut out, "key_item", encoded);
        }
        if let Some(name) = &self.override_loot_table_to_display {
            put(
                &mut out,
                "override_loot_table_to_display",
                Tag::String(name.clone()),
            );
        }
        Tag::Compound(out)
    }
}

/// `VaultServerData`：`rewardedPlayers` 有序（超过 128 逐出最老）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ServerData {
    pub rewarded_players: Vec<[u8; 16]>,
    pub state_updating_resumes_at: i64,
    /// 待弹出的物品；`popNextItemToEject` 从**尾部**取。
    pub items_to_eject: Vec<ItemStack>,
    pub total_ejections_needed: i32,
    /// 不存档：插钥匙失败声音的节流时刻。
    pub last_insert_fail_timestamp: i64,
}

impl ServerData {
    pub fn has_rewarded_player(&self, uuid: [u8; 16]) -> bool {
        self.rewarded_players.contains(&uuid)
    }

    pub fn add_to_rewarded_players(&mut self, uuid: [u8; 16]) {
        if self.rewarded_players.contains(&uuid) {
            return;
        }
        self.rewarded_players.push(uuid);
        if self.rewarded_players.len() > MAX_REWARD_PLAYERS {
            self.rewarded_players.remove(0);
        }
    }

    pub fn pause_state_updating_until(&mut self, time: i64) {
        self.state_updating_resumes_at = time;
    }

    pub fn set_items_to_eject(&mut self, items: Vec<ItemStack>) {
        self.items_to_eject = items;
        self.total_ejections_needed = self.items_to_eject.len() as i32;
    }

    /// 下一个要弹出的物品（列表尾部，空则 `EMPTY`）。
    pub fn get_next_item_to_eject(&self) -> ItemStack {
        self.items_to_eject
            .last()
            .cloned()
            .unwrap_or_else(ItemStack::empty)
    }

    /// 从尾部弹出一件。
    pub fn pop_next_item_to_eject(&mut self) -> ItemStack {
        self.items_to_eject.pop().unwrap_or_else(ItemStack::empty)
    }

    pub fn mark_ejection_finished(&mut self) {
        self.total_ejections_needed = 0;
    }

    /// `VaultServerData.ejectionProgress`。
    pub fn ejection_progress(&self) -> f32 {
        if self.total_ejections_needed == 1 {
            return 1.0;
        }
        let size = self.items_to_eject.len() as f32;
        let total = self.total_ejections_needed as f32;
        1.0 - inverse_lerp(size, 1.0, total)
    }

    pub fn read(tag: Option<&Tag>) -> ServerData {
        let mut data = ServerData::default();
        let Some(Tag::Compound(values)) = tag else {
            return data;
        };
        let get = |key: &str| {
            values
                .iter()
                .rev()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value)
        };
        if let Some(Tag::List(items)) = get("rewarded_players") {
            for item in items {
                if let Some(uuid) = uuid_from_tag(item) {
                    data.rewarded_players.push(uuid);
                }
            }
        }
        if let Some(value) = get("state_updating_resumes_at") {
            data.state_updating_resumes_at = tag::java_i64(value).unwrap_or(0);
        }
        if let Some(Tag::List(items)) = get("items_to_eject") {
            for item in items {
                if let Ok(stack) = slots::decode_item(item) {
                    data.items_to_eject.push(stack);
                }
            }
        }
        // `VaultServerData.set` 不搬 `totalEjectionsNeeded`，load 后恒为 0。
        data
    }

    fn to_tag(&self) -> Tag {
        let mut out = Vec::new();
        if !self.rewarded_players.is_empty() {
            let list: Vec<Tag> = self
                .rewarded_players
                .iter()
                .map(|uuid| uuid_to_tag(*uuid))
                .collect();
            put(&mut out, "rewarded_players", Tag::List(list));
        }
        if self.state_updating_resumes_at != 0 {
            put(
                &mut out,
                "state_updating_resumes_at",
                Tag::Long(self.state_updating_resumes_at),
            );
        }
        if !self.items_to_eject.is_empty() {
            let list: Vec<Tag> = self
                .items_to_eject
                .iter()
                .filter_map(|stack| slots::encode_item(stack).ok())
                .collect();
            put(&mut out, "items_to_eject", Tag::List(list));
        }
        Tag::Compound(out)
    }
}

/// `VaultSharedData`：展示物品、已连接（范围内且未领奖）玩家、粒子范围。
/// `connected_players` 保序（`UUIDUtil.CODEC_LINKED_SET` 读入时保持列表顺序）。
#[derive(Debug, Clone, PartialEq)]
pub struct SharedData {
    pub display_item: ItemStack,
    pub connected_players: Vec<[u8; 16]>,
    pub connected_particles_range: f64,
}

impl Default for SharedData {
    fn default() -> Self {
        Self {
            display_item: ItemStack::empty(),
            connected_players: Vec::new(),
            connected_particles_range: DEFAULT_PARTICLES_RANGE,
        }
    }
}

impl SharedData {
    pub fn has_display_item(&self) -> bool {
        !self.display_item.is_empty()
    }

    /// `ItemStack.matches` 相同就不动，否则替换。
    pub fn set_display_item(&mut self, stack: ItemStack) {
        if self.display_item == stack {
            return;
        }
        self.display_item = stack;
    }

    pub fn has_connected_players(&self) -> bool {
        !self.connected_players.is_empty()
    }

    /// `updateConnectedPlayersWithinRange`：只留范围内且未领奖的玩家，保序去重。
    pub fn update_connected_players(
        &mut self,
        detected: Vec<[u8; 16]>,
        rewarded: &[[u8; 16]],
    ) -> bool {
        let mut next: Vec<[u8; 16]> = Vec::new();
        for uuid in detected {
            if !rewarded.contains(&uuid) && !next.contains(&uuid) {
                next.push(uuid);
            }
        }
        let changed = next != self.connected_players;
        self.connected_players = next;
        changed
    }

    pub fn read(tag: Option<&Tag>) -> SharedData {
        let mut data = SharedData::default();
        let Some(Tag::Compound(values)) = tag else {
            return data;
        };
        let get = |key: &str| {
            values
                .iter()
                .rev()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value)
        };
        if let Some(value) = get("display_item")
            && let Ok(stack) = slots::decode_item(value)
        {
            data.display_item = stack;
        }
        if let Some(Tag::List(items)) = get("connected_players") {
            for item in items {
                if let Some(uuid) = uuid_from_tag(item)
                    && !data.connected_players.contains(&uuid)
                {
                    data.connected_players.push(uuid);
                }
            }
        }
        if let Some(value) = get("connected_particles_range") {
            data.connected_particles_range =
                tag::java_f64(value).unwrap_or(DEFAULT_PARTICLES_RANGE);
        }
        data
    }

    fn to_tag(&self) -> Tag {
        let mut out = Vec::new();
        if !self.display_item.is_empty()
            && let Ok(encoded) = slots::encode_item(&self.display_item)
        {
            put(&mut out, "display_item", encoded);
        }
        if !self.connected_players.is_empty() {
            let list: Vec<Tag> = self
                .connected_players
                .iter()
                .map(|uuid| uuid_to_tag(*uuid))
                .collect();
            put(&mut out, "connected_players", Tag::List(list));
        }
        if self.connected_particles_range.to_bits() != DEFAULT_PARTICLES_RANGE.to_bits() {
            put(
                &mut out,
                "connected_particles_range",
                Tag::Double(self.connected_particles_range),
            );
        }
        Tag::Compound(out)
    }
}

/// 一个宝库：`VaultState` 在方块状态里，三份数据在这里。
#[derive(Debug, Clone, PartialEq)]
pub struct Vault {
    pub state: VaultState,
    pub config: Config,
    pub server: ServerData,
    pub shared: SharedData,
}

impl Vault {
    /// `VaultBlockEntity.loadAdditional`：从存档条目还原；`state` 来自方块状态。
    pub fn read(tag: &Tag, state: VaultState) -> Vault {
        Vault {
            state,
            config: Config::read(field(tag, "config")),
            server: ServerData::read(field(tag, "server_data")),
            shared: SharedData::read(field(tag, "shared_data")),
        }
    }

    /// `VaultBlockEntity.saveAdditional`：三份 codec 写出的键。
    pub fn additional(&self) -> Vec<(String, Tag)> {
        vec![
            ("config".to_owned(), self.config.to_tag()),
            ("shared_data".to_owned(), self.shared.to_tag()),
            ("server_data".to_owned(), self.server.to_tag()),
        ]
    }

    /// `VaultBlockEntity.Server.tick`。
    pub fn tick<C: VaultContext>(&mut self, pos: BlockPos, ominous: bool, ctx: &mut C) {
        let now = ctx.game_time();
        // `shouldCycleDisplayItem`: `gameTime % 20 == 0 && state == ACTIVE`。
        if now % DISPLAY_CYCLE_TICK_RATE == 0 && self.state == VaultState::Active {
            self.cycle_display_item(pos, ctx);
        }
        // 状态更新被 `stateUpdatingResumesAt` 门控。
        if now >= self.server.state_updating_resumes_at {
            let next = self.tick_and_get_next(pos, ctx);
            if next != self.state {
                self.set_vault_state(pos, next, ominous, ctx);
            }
        }
    }

    /// `VaultState.tickAndGetNext`。
    fn tick_and_get_next<C: VaultContext>(&mut self, pos: BlockPos, ctx: &mut C) -> VaultState {
        let now = ctx.game_time();
        match self.state {
            VaultState::Inactive => {
                self.update_state_for_connected_players(pos, self.config.activation_range, ctx)
            }
            VaultState::Active => {
                self.update_state_for_connected_players(pos, self.config.deactivation_range, ctx)
            }
            VaultState::Unlocking => {
                self.server
                    .pause_state_updating_until(now.saturating_add(DISPLAY_CYCLE_TICK_RATE));
                VaultState::Ejecting
            }
            VaultState::Ejecting => {
                if self.server.items_to_eject.is_empty() {
                    self.server.mark_ejection_finished();
                    self.update_state_for_connected_players(
                        pos,
                        self.config.deactivation_range,
                        ctx,
                    )
                } else {
                    let progress = self.server.ejection_progress();
                    let stack = self.server.pop_next_item_to_eject();
                    ctx.eject_item(pos, &stack, progress);
                    self.shared
                        .set_display_item(self.server.get_next_item_to_eject());
                    self.server
                        .pause_state_updating_until(now.saturating_add(DISPLAY_CYCLE_TICK_RATE));
                    VaultState::Ejecting
                }
            }
        }
    }

    /// `VaultState.updateStateForConnectedPlayers`：`pause(gameTime+20)` 后按是否有
    /// 已连接玩家决定 `ACTIVE` / `INACTIVE`。
    fn update_state_for_connected_players<C: VaultContext>(
        &mut self,
        pos: BlockPos,
        range: f64,
        ctx: &mut C,
    ) -> VaultState {
        let detected = ctx.connected_players(pos, range, &self.server.rewarded_players);
        self.shared
            .update_connected_players(detected, &self.server.rewarded_players);
        self.server
            .pause_state_updating_until(ctx.game_time().saturating_add(DISPLAY_CYCLE_TICK_RATE));
        if self.shared.has_connected_players() {
            VaultState::Active
        } else {
            VaultState::Inactive
        }
    }

    /// `Server.cycleDisplayItemFromLootTable`：`canEjectReward` 假则清空展示物品，
    /// 否则从表里随机取一件。
    fn cycle_display_item<C: VaultContext>(&mut self, pos: BlockPos, ctx: &mut C) {
        if !self.can_eject_reward() {
            self.shared.set_display_item(ItemStack::empty());
            return;
        }
        let table = self
            .config
            .override_loot_table_to_display
            .as_deref()
            .unwrap_or(&self.config.loot_table);
        let item = ctx.random_display_item(table, pos);
        self.shared.set_display_item(item);
    }

    /// `canEjectReward`：`keyItem` 非空且状态不是 `INACTIVE`。
    fn can_eject_reward(&self) -> bool {
        !self.config.key_item.is_empty() && self.state != VaultState::Inactive
    }

    /// `Server.tryInsertKey`：玩家拿钥匙右键。返回有没有成功解锁。
    pub fn try_insert_key<C: VaultContext>(
        &mut self,
        pos: BlockPos,
        ominous: bool,
        player: [u8; 16],
        key: &ItemStack,
        ctx: &mut C,
    ) -> bool {
        if !self.can_eject_reward() {
            return false;
        }
        if !valid_key(&self.config.key_item, key) {
            self.play_insert_fail_sound(pos, VaultSound::InsertItemFail, ctx);
            return false;
        }
        if self.server.has_rewarded_player(player) {
            self.play_insert_fail_sound(pos, VaultSound::RejectRewardedPlayer, ctx);
            return false;
        }
        let items = ctx.resolve_reward(pos, player, key);
        if items.is_empty() {
            return false;
        }
        ctx.consume_key(player, self.config.key_item.count);
        self.unlock(pos, ominous, items, ctx);
        self.server.add_to_rewarded_players(player);
        // 解锁后立刻按失效范围刷新已连接玩家（把刚领奖的玩家排除）。
        let detected = ctx.connected_players(
            pos,
            self.config.deactivation_range,
            &self.server.rewarded_players,
        );
        self.shared
            .update_connected_players(detected, &self.server.rewarded_players);
        true
    }

    /// `Server.unlock`：填 `itemsToEject`、设展示物品、`pause(+14)`、切到 `UNLOCKING`。
    fn unlock<C: VaultContext>(
        &mut self,
        pos: BlockPos,
        ominous: bool,
        items: Vec<ItemStack>,
        ctx: &mut C,
    ) {
        self.server.set_items_to_eject(items);
        self.shared
            .set_display_item(self.server.get_next_item_to_eject());
        self.server
            .pause_state_updating_until(ctx.game_time().saturating_add(UNLOCKING_DELAY_TICKS));
        self.set_vault_state(pos, VaultState::Unlocking, ominous, ctx);
    }

    /// `Server.setVaultState`：写方块状态，再跑 `old.onExit` / `new.onEnter`。
    fn set_vault_state<C: VaultContext>(
        &mut self,
        pos: BlockPos,
        to: VaultState,
        ominous: bool,
        ctx: &mut C,
    ) {
        let from = self.state;
        ctx.set_block_state(to);
        self.state = to;
        // `onExit`。
        if from == VaultState::Ejecting {
            ctx.sound(pos, VaultSound::CloseShutter);
        }
        // `onEnter`。
        match to {
            VaultState::Inactive => {
                self.shared.set_display_item(ItemStack::empty());
                ctx.level_event(level_event::DEACTIVATE, pos, i32::from(ominous));
            }
            VaultState::Active => {
                if !self.shared.has_display_item() {
                    self.cycle_display_item(pos, ctx);
                }
                ctx.level_event(level_event::ACTIVATE, pos, i32::from(ominous));
            }
            VaultState::Unlocking => ctx.sound(pos, VaultSound::InsertItem),
            VaultState::Ejecting => ctx.sound(pos, VaultSound::OpenShutter),
        }
    }

    fn play_insert_fail_sound<C: VaultContext>(
        &mut self,
        pos: BlockPos,
        sound: VaultSound,
        ctx: &mut C,
    ) {
        let now = ctx.game_time();
        if now
            >= self
                .server
                .last_insert_fail_timestamp
                .saturating_add(INSERT_FAIL_SOUND_BUFFER_TICKS)
        {
            ctx.sound(pos, sound);
            self.server.last_insert_fail_timestamp = now;
        }
    }
}

/// `Mth.inverseLerp`：`(value - min) / (max - min)`。
fn inverse_lerp(value: f32, min: f32, max: f32) -> f32 {
    (value - min) / (max - min)
}

/// `ItemStack.isSameItemSameComponents`：物品 ID、组件补丁相同，且数量够。
fn valid_key(want: &ItemStack, have: &ItemStack) -> bool {
    want.item == have.item
        && want.added == have.added
        && want.removed == have.removed
        && have.count >= want.count
}

fn identifier(tag: &Tag) -> Option<String> {
    match tag {
        Tag::String(text) => mc_bamboo_protocol::components::cursor::parse_identifier(text).ok(),
        _ => None,
    }
}

/// `UUIDUtil.CODEC`：`IntArray`（4 个 i32）↔ 16 字节。
fn uuid_to_tag(uuid: [u8; 16]) -> Tag {
    let mut parts = Vec::with_capacity(4);
    for chunk in uuid.chunks(4) {
        parts.push(i32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Tag::IntArray(parts)
}

fn uuid_from_tag(tag: &Tag) -> Option<[u8; 16]> {
    let Tag::IntArray(parts) = tag else {
        return None;
    };
    if parts.len() != 4 {
        return None;
    }
    let mut uuid = [0_u8; 16];
    for (index, part) in parts.iter().enumerate() {
        uuid[index * 4..index * 4 + 4].copy_from_slice(&part.to_be_bytes());
    }
    Some(uuid)
}

/// 宝库的世界副作用。方块层只跑状态机，需要游戏层数据的（战利品表、玩家探测、
/// 声音、消耗钥匙）由调用方落地；缺省实现让纯逻辑单测不依赖世界。
pub trait VaultContext {
    fn game_time(&self) -> i64;
    /// `PlayerDetector.detect`（`INCLUDING_CREATIVE_PLAYERS`、`lineOfSight=false`）。
    fn connected_players(&self, pos: BlockPos, range: f64, rewarded: &[[u8; 16]]) -> Vec<[u8; 16]>;
    /// `resolveItemsToEject`：VAULT 战利品表（含玩家 luck / this_entity / tool）。
    fn resolve_reward(&self, pos: BlockPos, player: [u8; 16], key: &ItemStack) -> Vec<ItemStack>;
    /// `getRandomDisplayItemFromLootTable`：用世界随机源从表里取一件。
    fn random_display_item(&mut self, loot_table: &str, pos: BlockPos) -> ItemStack;
    /// `ejectResultItem`：`spawnItem` + `levelEvent(3017)` + 弹出声音。
    fn eject_item(&mut self, pos: BlockPos, stack: &ItemStack, progress: f32);
    fn level_event(&mut self, event: i32, pos: BlockPos, data: i32);
    fn sound(&mut self, pos: BlockPos, sound: VaultSound);
    fn set_block_state(&mut self, state: VaultState);
    /// 消耗玩家手里的钥匙（`ItemStack.consume`）。
    fn consume_key(&mut self, player: [u8; 16], count: i32);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_item() -> ItemStack {
        ItemStack::new(trial_key_item(), 1)
    }

    fn loot_item(id: i32) -> ItemStack {
        ItemStack::new(id, 1)
    }

    #[derive(Default)]
    struct TestCtx {
        time: i64,
        players: Vec<[u8; 16]>,
        states: Vec<VaultState>,
        events: Vec<(i32, i32)>,
        sounds: Vec<VaultSound>,
        ejected: Vec<(ItemStack, f32)>,
        display: ItemStack,
        consumed: Vec<([u8; 16], i32)>,
    }

    impl VaultContext for TestCtx {
        fn game_time(&self) -> i64 {
            self.time
        }
        fn connected_players(
            &self,
            _pos: BlockPos,
            _range: f64,
            _rewarded: &[[u8; 16]],
        ) -> Vec<[u8; 16]> {
            self.players.clone()
        }
        fn resolve_reward(
            &self,
            _pos: BlockPos,
            _player: [u8; 16],
            _key: &ItemStack,
        ) -> Vec<ItemStack> {
            vec![loot_item(900), loot_item(901)]
        }
        fn random_display_item(&mut self, _loot_table: &str, _pos: BlockPos) -> ItemStack {
            self.display.clone()
        }
        fn eject_item(&mut self, _pos: BlockPos, stack: &ItemStack, progress: f32) {
            self.ejected.push((stack.clone(), progress));
        }
        fn level_event(&mut self, event: i32, _pos: BlockPos, data: i32) {
            self.events.push((event, data));
        }
        fn sound(&mut self, _pos: BlockPos, sound: VaultSound) {
            self.sounds.push(sound);
        }
        fn set_block_state(&mut self, state: VaultState) {
            self.states.push(state);
        }
        fn consume_key(&mut self, player: [u8; 16], count: i32) {
            self.consumed.push((player, count));
        }
    }

    fn active_vault() -> (Vault, TestCtx) {
        let mut vault = Vault {
            state: VaultState::Inactive,
            config: default_config(),
            server: ServerData::default(),
            shared: SharedData::default(),
        };
        let player = [7_u8; 16];
        let mut ctx = TestCtx {
            time: 100,
            players: vec![player],
            display: loot_item(42),
            ..TestCtx::default()
        };
        // INACTIVE -> ACTIVE（探测到玩家）。
        vault.tick(BlockPos::new(0, 64, 0), false, &mut ctx);
        assert_eq!(vault.state, VaultState::Active);
        (vault, ctx)
    }

    #[test]
    fn inactive_activates_when_a_player_is_near() {
        let (vault, ctx) = active_vault();
        assert_eq!(vault.shared.connected_players.len(), 1);
        // ACTIVE.onEnter 发了 3015 且轮换出展示物品。
        assert!(ctx.events.contains(&(level_event::ACTIVATE, 0)));
        assert_eq!(vault.shared.display_item, loot_item(42));
    }

    #[test]
    fn inactive_stays_inactive_without_players() {
        let mut vault = Vault {
            state: VaultState::Inactive,
            config: default_config(),
            server: ServerData::default(),
            shared: SharedData::default(),
        };
        let mut ctx = TestCtx {
            time: 100,
            ..TestCtx::default()
        };
        vault.tick(BlockPos::new(0, 64, 0), false, &mut ctx);
        assert_eq!(vault.state, VaultState::Inactive);
        assert!(ctx.states.is_empty());
    }

    #[test]
    fn inserting_a_key_unlocks_then_ejects_then_deactivates() {
        let (mut vault, mut ctx) = active_vault();
        let player = [7_u8; 16];
        let pos = BlockPos::new(0, 64, 0);
        assert!(vault.try_insert_key(pos, false, player, &key_item(), &mut ctx));
        assert_eq!(vault.state, VaultState::Unlocking);
        assert_eq!(ctx.consumed, vec![(player, 1)]);
        assert_eq!(vault.server.items_to_eject.len(), 2);
        assert_eq!(vault.server.total_ejections_needed, 2);
        assert!(vault.server.has_rewarded_player(player));
        // 进入 UNLOCKING 会等 14 刻才 EJECTING。
        ctx.time += UNLOCKING_DELAY_TICKS;
        vault.tick(pos, false, &mut ctx);
        assert_eq!(vault.state, VaultState::Ejecting);
        // 每次 EJECTING 弹一件（从尾部），间隔 20 刻。
        for step in 0..3 {
            ctx.time += DISPLAY_CYCLE_TICK_RATE;
            vault.tick(pos, false, &mut ctx);
            if step < 2 {
                assert_eq!(vault.state, VaultState::Ejecting);
            }
        }
        assert_eq!(ctx.ejected.len(), 2);
        // 第二件先弹（尾部），进度递增。
        assert_eq!(ctx.ejected[0].0, loot_item(901));
        assert_eq!(ctx.ejected[1].0, loot_item(900));
        assert!(ctx.ejected[0].1 < ctx.ejected[1].1);
        assert!(ctx.events.contains(&(level_event::DEACTIVATE, 0)));
    }

    #[test]
    fn wrong_key_and_rewarded_player_are_rejected() {
        let (mut vault, mut ctx) = active_vault();
        let pos = BlockPos::new(0, 64, 0);
        // 错误钥匙：失败声音，不改状态、不领奖。
        assert!(!vault.try_insert_key(pos, false, [7; 16], &loot_item(5), &mut ctx));
        assert_eq!(ctx.sounds, vec![VaultSound::InsertItemFail]);
        assert_eq!(vault.state, VaultState::Active);
        // 正确钥匙领奖。
        assert!(vault.try_insert_key(pos, false, [7; 16], &key_item(), &mut ctx));
        // 同一玩家再插：直接 return（不解锁），过了节流窗口才发拒绝声音。
        ctx.time += INSERT_FAIL_SOUND_BUFFER_TICKS + 1;
        assert!(!vault.try_insert_key(pos, false, [7; 16], &key_item(), &mut ctx));
        assert!(ctx.sounds.contains(&VaultSound::RejectRewardedPlayer));
    }

    #[test]
    fn ejection_progress_matches_vanilla_formula() {
        let mut server = ServerData::default();
        server.set_items_to_eject(vec![loot_item(1)]);
        assert_eq!(server.ejection_progress(), 1.0);
        server.set_items_to_eject(vec![loot_item(1), loot_item(2)]);
        assert_eq!(server.ejection_progress(), 0.0);
        server.pop_next_item_to_eject();
        assert_eq!(server.ejection_progress(), 1.0);
    }

    #[test]
    fn archive_round_trip_keeps_only_codec_keys() {
        let player = [3_u8; 16];
        let mut vault = Vault {
            state: VaultState::Ejecting,
            config: default_config(),
            server: ServerData::default(),
            shared: SharedData::default(),
        };
        vault.server.add_to_rewarded_players(player);
        vault.server.state_updating_resumes_at = 1234;
        vault.server.set_items_to_eject(vec![loot_item(55)]);
        vault.shared.display_item = loot_item(66);
        vault.shared.connected_players.push([9; 16]);
        let tag = Tag::Compound(vault.additional());
        let back = Vault::read(&tag, VaultState::Ejecting);
        assert_eq!(back.config, vault.config);
        assert_eq!(back.server.rewarded_players, vec![player]);
        assert_eq!(back.server.state_updating_resumes_at, 1234);
        assert_eq!(back.server.items_to_eject, vec![loot_item(55)]);
        // `totalEjectionsNeeded` 不进存档。
        assert_eq!(back.server.total_ejections_needed, 0);
        assert_eq!(back.shared.display_item, loot_item(66));
        assert!(back.shared.connected_players.contains(&[9; 16]));
    }

    #[test]
    fn config_validate_rejects_inverted_ranges() {
        let tag = Tag::Compound(vec![
            ("activation_range".to_owned(), Tag::Double(9.0)),
            ("deactivation_range".to_owned(), Tag::Double(2.0)),
        ]);
        assert_eq!(Config::read(Some(&tag)), default_config());
    }

    /// 新的结构化 `Vault` 存档与 `passive.rs` 里已和官方对拍过的 codec 逐值一致。
    /// `config` / `shared_data` / `server_data` 三个键都要对上，否则挂接后
    /// chunk-content 的 `vault-*` 用例会退化。
    #[test]
    fn archive_matches_the_passive_codec() {
        use crate::{
            BlockState,
            archive::{PassiveBlockEntity, PassiveKind},
        };
        let cases: Vec<Tag> = vec![
            // 裸条目：三个 codec 都缺省。
            Tag::Compound(Vec::new()),
            // chunk-content 的 vault-full：非默认范围、uuid 列表、待弹出物品等。
            Tag::Compound(vec![
                (
                    "config".to_owned(),
                    Tag::Compound(vec![
                        (
                            "loot_table".to_owned(),
                            Tag::String("minecraft:chests/trial_chambers/reward".to_owned()),
                        ),
                        ("activation_range".to_owned(), Tag::Double(3.0)),
                        ("deactivation_range".to_owned(), Tag::Double(4.5)),
                    ]),
                ),
                (
                    "shared_data".to_owned(),
                    Tag::Compound(vec![(
                        "connected_players".to_owned(),
                        Tag::List(vec![Tag::IntArray(vec![1, 2, 3, 4])]),
                    )]),
                ),
                (
                    "server_data".to_owned(),
                    Tag::Compound(vec![
                        ("items_to_eject".to_owned(), Tag::List(Vec::new())),
                        ("total_ejections_needed".to_owned(), Tag::Int(7)),
                        ("state_updating_resumes_at".to_owned(), Tag::Long(99)),
                    ]),
                ),
            ]),
            // 非默认钥匙、粒子范围、奖励玩家、待弹出物品。
            Tag::Compound(vec![
                (
                    "config".to_owned(),
                    Tag::Compound(vec![
                        (
                            "loot_table".to_owned(),
                            Tag::String("minecraft:chests/end_city_treasure".to_owned()),
                        ),
                        (
                            "override_loot_table_to_display".to_owned(),
                            Tag::String("minecraft:chests/simple_dungeon".to_owned()),
                        ),
                    ]),
                ),
                (
                    "shared_data".to_owned(),
                    Tag::Compound(vec![
                        (
                            "display_item".to_owned(),
                            Tag::Compound(vec![
                                ("id".to_owned(), Tag::String("minecraft:diamond".to_owned())),
                                ("count".to_owned(), Tag::Int(3)),
                            ]),
                        ),
                        ("connected_particles_range".to_owned(), Tag::Double(8.0)),
                    ]),
                ),
                (
                    "server_data".to_owned(),
                    Tag::Compound(vec![
                        (
                            "rewarded_players".to_owned(),
                            Tag::List(vec![Tag::IntArray(vec![9, 8, 7, 6])]),
                        ),
                        (
                            "items_to_eject".to_owned(),
                            Tag::List(vec![Tag::Compound(vec![
                                ("id".to_owned(), Tag::String("minecraft:emerald".to_owned())),
                                ("count".to_owned(), Tag::Int(2)),
                            ])]),
                        ),
                    ]),
                ),
            ]),
        ];
        for tag in cases {
            let via_vault = Vault::read(&tag, VaultState::Active).additional();
            let via_passive = PassiveBlockEntity::read(
                PassiveKind::Vault,
                BlockPos::new(0, 0, 0),
                BlockState::AIR,
                &tag,
            )
            .unwrap()
            .additional();
            assert_eq!(via_vault, via_passive, "tag = {tag:?}");
        }
    }
}
