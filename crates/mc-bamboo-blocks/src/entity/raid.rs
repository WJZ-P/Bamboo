//! 袭击数据：`Raids` / `Raid` 的存档结构和不碰世界的纯逻辑。
//!
//! 官方 `net.minecraft.world.entity.raid.Raid` / `Raids`。这里只放 `raids.dat`
//! 的字段、按难度和不祥等级算波次的表，以及状态机里那些只读写自身字段的判定；
//! 刷怪、发包、读写世界那部分留给调用方，见模块注释。字段名和顺序照
//! `Raid.MAP_CODEC` / `Raids.CODEC`。
//!
//! Raid data: the `raids.dat` shape of vanilla `Raids` / `Raid`, the wave tables
//! keyed by difficulty and omen level, and the state-machine predicates that only
//! touch their own fields. Spawning, packets and world access stay with the caller.
//! Field names and order follow `Raid.MAP_CODEC` / `Raids.CODEC`.

use mc_bamboo_core::BlockPos;
use mc_bamboo_nbt::Tag;

use crate::archive::tag;

/// 难度 ID（官方 `Difficulty.ordinal` 的编码，独立于 crate 里其它难度表示）。
///
/// Difficulty ids (vanilla `Difficulty.ordinal`, kept independent of other
/// difficulty representations in the crate).
pub const DIFFICULTY_PEACEFUL: i32 = 0;
pub const DIFFICULTY_EASY: i32 = 1;
pub const DIFFICULTY_NORMAL: i32 = 2;
pub const DIFFICULTY_HARD: i32 = 3;

/// `Raid.DEFAULT_PRE_RAID_TICKS`：每波之前的冷却，也是构造器里 `raidCooldownTicks` 的初值。
///
/// `Raid.DEFAULT_PRE_RAID_TICKS`: pre-wave cooldown and the constructor's initial
/// `raidCooldownTicks`.
pub const DEFAULT_PRE_RAID_TICKS: i32 = 300;
/// `Raid.RAID_TIMEOUT_TICKS`：袭击活跃时间上限。
pub const RAID_TIMEOUT_TICKS: i64 = 48000;
/// `Raid.POST_RAID_TICK_LIMIT`：所有袭击者死亡后转胜利前的等待刻数。
pub const POST_RAID_TICK_LIMIT: i32 = 40;
/// `Raid.DEFAULT_MAX_RAID_OMEN_LEVEL` / `getMaxRaidOmenLevel()`。
pub const MAX_RAID_OMEN_LEVEL: i32 = 5;
/// `Raid.MAX_CELEBRATION_TICKS`：胜利 / 失败后血条停留的刻数。
pub const MAX_CELEBRATION_TICKS: i32 = 600;
/// `Raid.HERO_OF_THE_VILLAGE_DURATION`：胜利奖励的时长。
pub const HERO_OF_THE_VILLAGE_DURATION: i32 = 48000;
/// `Raid.VALID_RAID_RADIUS_SQR`：`ServerLevel.getRaidAt` 的查找半径平方。
pub const VALID_RAID_RADIUS_SQR: i32 = 9216;
/// `Raid.RAID_REMOVAL_THRESHOLD_SQR`：`updateRaiders` 里移除离队袭击者的距离平方。
pub const RAID_REMOVAL_THRESHOLD_SQR: f64 = 12544.0;
/// `Raid.MAX_NO_ACTION_TIME`：`Raids.canJoinRaid` / `updateRaiders` 的阈值。
pub const MAX_NO_ACTION_TIME: i32 = 2400;
/// `Raid.OUTSIDE_RAID_BOUNDS_TIMEOUT`：离村外的容忍刻数。
pub const OUTSIDE_RAID_BOUNDS_TIMEOUT: i32 = 30;
/// `Raids.tick` 里强制 `setDirty` 的周期。
pub const RAID_SAVE_INTERVAL: i32 = 200;

/// `Raid.RaidStatus`，序列化名照 `getSerializedName`。
///
/// Vanilla `Raid.RaidStatus` with its `getSerializedName` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RaidStatus {
    #[default]
    Ongoing,
    Victory,
    Loss,
    Stopped,
}

impl RaidStatus {
    /// `getSerializedName()`。
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Ongoing => "ongoing",
            Self::Victory => "victory",
            Self::Loss => "loss",
            Self::Stopped => "stopped",
        }
    }

    /// 按序列化名解析，坏值返回 `None`（不 panic）。
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "ongoing" => Some(Self::Ongoing),
            "victory" => Some(Self::Victory),
            "loss" => Some(Self::Loss),
            "stopped" => Some(Self::Stopped),
            _ => None,
        }
    }
}

/// `Raid.RaiderType`，枚举顺序照官方 `VALUES`（spawnGroup 遍历顺序）。
///
/// Vanilla `Raid.RaiderType` in its `VALUES` order (the spawnGroup iteration order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaiderType {
    Vindicator,
    Evoker,
    Pillager,
    Witch,
    Ravager,
}

impl RaiderType {
    /// 全部类型，按 `VALUES` 顺序。
    pub const VALUES: [Self; 5] = [
        Self::Vindicator,
        Self::Evoker,
        Self::Pillager,
        Self::Witch,
        Self::Ravager,
    ];

    /// `spawnsPerWaveBeforeBonus`，八波（索引 0..7）。
    ///
    /// The `spawnsPerWaveBeforeBonus` table, eight waves (index 0..7).
    pub const fn spawns_per_wave(self) -> &'static [i32; 8] {
        match self {
            Self::Vindicator => &[0, 0, 2, 0, 1, 4, 2, 5],
            Self::Evoker => &[0, 0, 0, 0, 0, 1, 1, 2],
            Self::Pillager => &[0, 4, 3, 3, 4, 4, 4, 2],
            Self::Witch => &[0, 0, 0, 0, 3, 0, 0, 1],
            Self::Ravager => &[0, 0, 0, 1, 0, 1, 0, 2],
        }
    }

    /// `entityType` 的注册表 ID。
    pub const fn entity_type(self) -> i32 {
        use mc_bamboo_data::registries::entity_type as ids;
        match self {
            Self::Vindicator => ids::VINDICATOR,
            Self::Evoker => ids::EVOKER,
            Self::Pillager => ids::PILLAGER,
            Self::Witch => ids::WITCH,
            Self::Ravager => ids::RAVAGER,
        }
    }
}

/// `Raid.getNumGroups(Difficulty)`：和平 0、简单 3、普通 5、困难 7；坏值按和平。
///
/// Vanilla `Raid.getNumGroups`: peaceful 0, easy 3, normal 5, hard 7; bad values
/// fall back to peaceful so untrusted difficulty cannot panic.
pub const fn num_groups(difficulty: i32) -> i32 {
    match difficulty {
        DIFFICULTY_PEACEFUL => 0,
        DIFFICULTY_EASY => 3,
        DIFFICULTY_NORMAL => 5,
        DIFFICULTY_HARD => 7,
        _ => 0,
    }
}

/// 一场袭击的存档字段，外加几个不进存档、但状态机每刻要用的运行时字段。
/// 袭击者按实体 uid 引用（官方 `groupRaiderMap` / `groupToLeaderMap` 里存的是
/// `Raider` 实例，这里按 id 存，世界线程按 uid 查实体）。这些字段官方不写进
/// `raids.dat`，读盘后为空。
///
/// One raid's saved fields plus a few non-saved runtime fields. Raiders are
/// referenced by entity uid (vanilla stores `Raider` instances in its maps;
/// here we keep ids and the world thread looks them up). None of these are
/// written to `raids.dat`, so they start empty after a load.
#[derive(Debug, Clone, PartialEq)]
pub struct Raid {
    pub started: bool,
    pub active: bool,
    pub ticks_active: i64,
    pub raid_omen_level: i32,
    pub groups_spawned: i32,
    pub cooldown_ticks: i32,
    pub post_raid_ticks: i32,
    pub total_health: f32,
    pub group_count: i32,
    pub status: RaidStatus,
    pub center: BlockPos,
    /// `heroesOfTheVillage`：UUID 的 16 字节大端表示，去重。
    pub heroes_of_the_village: Vec<[u8; 16]>,
    /// `waveSpawnPos`：官方不存档，读盘后为空。
    pub wave_spawn_pos: Option<BlockPos>,
    /// `celebrationTicks`：官方不存档，读盘后为 0。
    pub celebration_ticks: i32,
    /// `groupRaiderMap`：每波的袭击者实体 uid。
    pub raiders: std::collections::BTreeMap<i32, Vec<i32>>,
    /// `groupToLeaderMap`：每波队长实体 uid。
    pub leaders: std::collections::BTreeMap<i32, i32>,
    /// `Raid.random` 的状态（Java Random 的 LCG）。官方构造时 `RandomSource.create()`，
    /// 读盘时重建，所以不存档。对拍场景里预设 `waveSpawnPos` 能绕过位置随机。
    ///
    /// State of `Raid.random` (a Java-Random LCG). Vanilla creates it fresh in
    /// the constructor and rebuilds it on load, so it is not saved. Scenes can
    /// preset `waveSpawnPos` to bypass spawn-position randomness.
    pub rng_state: u64,
}

impl Raid {
    /// `Raid(center, difficulty)`：active=true、冷却 300、血条进度 0、状态 ONGOING。
    ///
    /// Vanilla `Raid(center, difficulty)`: active, 300-tick cooldown, empty bar,
    /// ONGOING status.
    pub fn new(center: BlockPos, difficulty: i32) -> Self {
        Self {
            started: false,
            active: true,
            ticks_active: 0,
            raid_omen_level: 0,
            groups_spawned: 0,
            cooldown_ticks: DEFAULT_PRE_RAID_TICKS,
            post_raid_ticks: 0,
            total_health: 0.0,
            group_count: num_groups(difficulty),
            status: RaidStatus::Ongoing,
            center,
            heroes_of_the_village: Vec::new(),
            wave_spawn_pos: None,
            celebration_ticks: 0,
            raiders: std::collections::BTreeMap::new(),
            leaders: std::collections::BTreeMap::new(),
            rng_state: random_seed(center),
        }
    }

    /// `addWaveMob`：把一名袭击者记进该波名册，可选累加总血量。名册按 uid 去重
    /// （官方按 UUID 替换同名实例）。
    ///
    /// Vanilla `addWaveMob`: record a raider under its wave, optionally adding
    /// its health. The roster is deduplicated by uid.
    pub fn add_wave_mob(&mut self, wave: i32, uid: i32, health: f32, add_health: bool) {
        let members = self.raiders.entry(wave).or_default();
        if !members.contains(&uid) {
            members.push(uid);
        }
        if add_health {
            self.total_health += health;
        }
    }

    /// `removeFromRaid`：从该波名册移除，可选扣回总血量，返回是否真的移除。
    ///
    /// Vanilla `removeFromRaid`: drop a raider from its wave, optionally
    /// subtracting its health, and report whether it was present.
    pub fn remove_raider(&mut self, wave: i32, uid: i32, health: f32, add_health: bool) -> bool {
        let Some(members) = self.raiders.get_mut(&wave) else {
            return false;
        };
        let Some(index) = members.iter().position(|&member| member == uid) else {
            return false;
        };
        members.remove(index);
        if add_health {
            self.total_health -= health;
        }
        true
    }

    /// `getTotalRaidersAlive`：官方数的是袭击名册里所有波的袭击者数（死亡 /
    /// 离队的由 `updateRaiders` 每 20 刻清掉）。
    ///
    /// Vanilla `getTotalRaidersAlive`: the number of raiders across all wave
    /// rosters; dead / detached ones are culled by `updateRaiders` every 20 ticks.
    pub fn total_raiders(&self) -> i32 {
        self.raiders
            .values()
            .map(|members| members.len() as i32)
            .sum()
    }

    /// `Random.nextInt(bits)` 的一次 LCG 取数。
    pub fn next_bits(&mut self, bits: u32) -> i32 {
        self.rng_state =
            self.rng_state.wrapping_mul(0x5DEE_CE66D).wrapping_add(0xB) & ((1u64 << 48) - 1);
        (self.rng_state >> (48 - bits)) as i32
    }

    /// `Random.nextInt(bound)`。袭击里只用到很小的 bound（2、3），省去官方
    /// 的溢出回绕重试也不会改变取数。
    pub fn next_int(&mut self, bound: i32) -> i32 {
        if bound <= 0 {
            return 0;
        }
        if (bound & (bound - 1)) == 0 {
            ((i64::from(bound) * i64::from(self.next_bits(31))) >> 31) as i32
        } else {
            self.next_bits(31) % bound
        }
    }

    /// `Random.nextFloat()`。
    pub fn next_float(&mut self) -> f32 {
        self.next_bits(24) as f32 / ((1 << 24) as f32)
    }

    /// `isOver()`：胜利或失败。
    pub const fn is_over(&self) -> bool {
        matches!(self.status, RaidStatus::Victory | RaidStatus::Loss)
    }

    /// `isStopped()`。
    pub const fn is_stopped(&self) -> bool {
        matches!(self.status, RaidStatus::Stopped)
    }

    /// `isVictory()`。
    pub const fn is_victory(&self) -> bool {
        matches!(self.status, RaidStatus::Victory)
    }

    /// `hasFirstWaveSpawned()`：`groupsSpawned > 0`。
    pub const fn has_first_wave_spawned(&self) -> bool {
        self.groups_spawned > 0
    }

    /// `isFinalWave()`：`groupsSpawned == numGroups`。
    pub const fn is_final_wave(&self) -> bool {
        self.groups_spawned == self.group_count
    }

    /// `hasBonusWave()`：`raidOmenLevel > 1`。
    pub const fn has_bonus_wave(&self) -> bool {
        self.raid_omen_level > 1
    }

    /// `hasSpawnedBonusWave()`：`groupsSpawned > numGroups`。
    pub const fn has_spawned_bonus_wave(&self) -> bool {
        self.groups_spawned > self.group_count
    }

    /// `shouldSpawnBonusGroup()`：最后一波、场上清空、且有奖励波。
    pub const fn should_spawn_bonus_group(&self, raiders_alive: i32) -> bool {
        self.is_final_wave() && raiders_alive == 0 && self.has_bonus_wave()
    }

    /// `hasMoreWaves()`：有奖励波时看它刷没刷，否则看是否还没到最后一波。
    pub const fn has_more_waves(&self) -> bool {
        if self.has_bonus_wave() {
            !self.has_spawned_bonus_wave()
        } else {
            !self.is_final_wave()
        }
    }

    /// `isBetweenWaves()`：首波已刷、场上清空、冷却为正。
    pub const fn is_between_waves(&self, raiders_alive: i32) -> bool {
        self.has_first_wave_spawned() && raiders_alive == 0 && self.cooldown_ticks > 0
    }

    /// `shouldSpawnGroup()`：冷却到 0、还有波次、且场上清空。
    pub const fn should_spawn_group(&self, raiders_alive: i32) -> bool {
        self.cooldown_ticks == 0
            && (self.groups_spawned < self.group_count
                || self.should_spawn_bonus_group(raiders_alive))
            && raiders_alive == 0
    }

    /// `absorbRaidOmen(player)`：`None` 表示玩家没有 `RAID_OMEN`（返回 false）；
    /// 否则把 `amplifier + 1` 加进等级并夹到 `[0, 5]`。
    ///
    /// Vanilla `absorbRaidOmen`: `None` means no `RAID_OMEN` effect (returns false);
    /// otherwise add `amplifier + 1` and clamp to `[0, 5]`.
    pub fn absorb_raid_omen(&mut self, raid_omen_amplifier: Option<i32>) -> bool {
        let Some(amplifier) = raid_omen_amplifier else {
            return false;
        };
        self.raid_omen_level = (self.raid_omen_level + amplifier + 1).clamp(0, MAX_RAID_OMEN_LEVEL);
        true
    }

    /// `getEnchantOdds()`：不祥等级 2/3/4/5 对应 0.1/0.25/0.5/0.75，其余 0。
    pub const fn enchant_odds(&self) -> f32 {
        match self.raid_omen_level {
            2 => 0.1,
            3 => 0.25,
            4 => 0.5,
            5 => 0.75,
            _ => 0.0,
        }
    }

    /// `updateBossbar()` 的进度：`clamp(livingHealth / totalHealth, 0, 1)`；
    /// 分母为 0 时按 0，避免除零。
    pub fn bossbar_progress(&self, living_health: f32) -> f32 {
        if self.total_health <= 0.0 {
            return 0.0;
        }
        (living_health / self.total_health).clamp(0.0, 1.0)
    }

    /// `addHeroOfTheVillage(entity)`：按 UUID 去重。
    pub fn add_hero_of_the_village(&mut self, uuid: [u8; 16]) {
        if !self.heroes_of_the_village.contains(&uuid) {
            self.heroes_of_the_village.push(uuid);
        }
    }

    /// `getDefaultNumSpawns(type, wave, bonus)`：奖励波取 `numGroups` 那一列，
    /// 否则取 `wave` 列；越界列按 0，坏数据不会 panic。
    pub fn default_num_spawns(&self, raider: RaiderType, wave: i32, bonus: bool) -> i32 {
        let table = raider.spawns_per_wave();
        let index = if bonus { self.group_count } else { wave };
        usize::try_from(index)
            .ok()
            .and_then(|i| table.get(i))
            .copied()
            .unwrap_or(0)
    }

    /// `getPotentialBonusSpawns(type, random, wave, difficulty, isBonusWave)`。
    /// `roll` 是官方 `RandomSource.nextInt(bound)`：注入它既让函数保持纯、可测，
    /// 也精确复刻官方的取数次数与顺序（逐刻对拍用）。
    ///
    /// Vanilla `getPotentialBonusSpawns`. `roll` stands for `RandomSource.nextInt`;
    /// injecting it keeps the function pure and reproduces the exact draw order.
    pub fn potential_bonus_spawns<F>(
        raider: RaiderType,
        wave: i32,
        difficulty: i32,
        bonus: bool,
        roll: &mut F,
    ) -> i32
    where
        F: FnMut(i32) -> i32,
    {
        let easy = difficulty == DIFFICULTY_EASY;
        let normal = difficulty == DIFFICULTY_NORMAL;
        let amount = match raider {
            RaiderType::Vindicator | RaiderType::Pillager => {
                if easy {
                    // 官方 easy 分支先 `nextInt(2)`，为 0 时直接返回 0（少一次取数）。
                    // The easy branch draws `nextInt(2)` first; a zero returns early.
                    let first = roll(2);
                    if first == 0 {
                        return 0;
                    }
                    first
                } else if normal {
                    1
                } else {
                    2
                }
            }
            RaiderType::Witch => {
                if !easy && wave > 2 && wave != 4 {
                    1
                } else {
                    return 0;
                }
            }
            RaiderType::Ravager => i32::from(!easy && bonus),
            RaiderType::Evoker => return 0,
        };
        if amount > 0 { roll(amount + 1) } else { 0 }
    }

    /// 编成 `RaidWithId` 的一条（含 `id`），字段顺序照 `RaidWithId.CODEC` +
    /// `Raid.MAP_CODEC`。
    pub fn to_tag(&self, id: i32) -> Tag {
        Tag::Compound(vec![
            ("id".to_owned(), Tag::Int(id)),
            ("started".to_owned(), Tag::Byte(i8::from(self.started))),
            ("active".to_owned(), Tag::Byte(i8::from(self.active))),
            ("ticks_active".to_owned(), Tag::Long(self.ticks_active)),
            ("raid_omen_level".to_owned(), Tag::Int(self.raid_omen_level)),
            ("groups_spawned".to_owned(), Tag::Int(self.groups_spawned)),
            ("cooldown_ticks".to_owned(), Tag::Int(self.cooldown_ticks)),
            ("post_raid_ticks".to_owned(), Tag::Int(self.post_raid_ticks)),
            ("total_health".to_owned(), Tag::Float(self.total_health)),
            ("group_count".to_owned(), Tag::Int(self.group_count)),
            (
                "status".to_owned(),
                Tag::String(self.status.serialized_name().to_owned()),
            ),
            ("center".to_owned(), block_pos_tag(self.center)),
            (
                "heroes_of_the_village".to_owned(),
                Tag::List(self.heroes_of_the_village.iter().map(uuid_tag).collect()),
            ),
        ])
    }

    /// 从一条 `RaidWithId` 解码；缺字段 / 类型不对返回 `None`（整条作废）。
    pub fn from_tag(raw: &Tag) -> Option<(i32, Self)> {
        let id = tag::int_or(raw, "id", -1);
        let status = RaidStatus::from_name(tag::string_or(raw, "status")?)?;
        let center = block_pos_from(raw, "center")?;
        let heroes = match tag::field(raw, "heroes_of_the_village") {
            Some(Tag::List(list)) => list.iter().filter_map(uuid_from).collect(),
            _ => return None,
        };
        Some((
            id,
            Self {
                started: tag::bool_or(raw, "started", false),
                active: tag::bool_or(raw, "active", true),
                ticks_active: tag::long_or(raw, "ticks_active", 0),
                raid_omen_level: tag::int_or(raw, "raid_omen_level", 0),
                groups_spawned: tag::int_or(raw, "groups_spawned", 0),
                cooldown_ticks: tag::int_or(raw, "cooldown_ticks", DEFAULT_PRE_RAID_TICKS),
                post_raid_ticks: tag::int_or(raw, "post_raid_ticks", 0),
                total_health: tag::float_or(raw, "total_health", 0.0),
                group_count: tag::int_or(raw, "group_count", 0),
                status,
                center,
                heroes_of_the_village: heroes,
                wave_spawn_pos: None,
                celebration_ticks: 0,
                raiders: std::collections::BTreeMap::new(),
                leaders: std::collections::BTreeMap::new(),
                rng_state: random_seed(center),
            },
        ))
    }
}

/// `Raids`（`raids.dat` 的 SavedData）：按 id 存的袭击表、`nextId`、每刻计数。
///
/// Vanilla `Raids` SavedData: raids keyed by id, `nextId`, and a per-tick counter.
#[derive(Debug, Clone, PartialEq)]
pub struct Raids {
    pub raids: std::collections::BTreeMap<i32, Raid>,
    pub next_id: i32,
    pub tick: i32,
}

impl Default for Raids {
    /// `Raids()`：`nextId` 初值为 1。
    fn default() -> Self {
        Self {
            raids: std::collections::BTreeMap::new(),
            next_id: 1,
            tick: 0,
        }
    }
}

impl Raids {
    pub fn new() -> Self {
        Self::default()
    }

    /// `getUniqueId()`：返回当前值再自增。
    pub fn unique_id(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// 拆岛：把中心落在 `inside` 区块里的袭击搬到返回的新表，其余留在本表。
    /// 一场袭击整个跟着它的中心走；两岛共享 `next_id` 起点，全局计数在存盘
    /// 汇总时统一（各岛运行时只看自己的表，id 暂时重复不影响查找）。
    ///
    /// Island split: move raids whose center is `inside` into the returned table,
    /// keeping the rest. A raid moves whole with its center; both islands share
    /// the `next_id` start, and the save rolls every island into one global
    /// counter. Each island only reads its own map at runtime.
    pub fn split_off(&mut self, inside: impl Fn(&BlockPos) -> bool) -> Self {
        let mut moved = std::collections::BTreeMap::new();
        let mut staying = std::collections::BTreeMap::new();
        for (id, raid) in std::mem::take(&mut self.raids) {
            if inside(&raid.center) {
                moved.insert(id, raid);
            } else {
                staying.insert(id, raid);
            }
        }
        self.raids = staying;
        Self {
            raids: moved,
            next_id: self.next_id,
            tick: self.tick,
        }
    }

    /// 合岛：把另一张表的袭击并进来，给它们重新分配不冲突的 id。别的全局计数
    /// （`next_id` / `tick`）以吸收方为准，存盘汇总时再统一。
    ///
    /// Island merge: absorb another table's raids, reassigning collision-free ids.
    /// The absorber's global counters win; the save unifies them.
    pub fn absorb(&mut self, other: Self) {
        for raid in other.raids.into_values() {
            let id = self.unique_id();
            self.raids.insert(id, raid);
        }
    }

    /// `getNearbyRaid(pos, radiusSqr)`：active 且中心距离平方小于半径的最近一场，
    /// 返回它的 id。半径参数本身就是平方值（官方 `ServerLevel.getRaidAt` 传
    /// `VALID_RAID_RADIUS_SQR`）。
    pub fn nearby_raid(&self, center: BlockPos, radius_sqr: f64) -> Option<i32> {
        let mut best: Option<(i32, f64)> = None;
        for (&id, raid) in &self.raids {
            if !raid.active {
                continue;
            }
            let dx = f64::from(raid.center.x) - f64::from(center.x);
            let dy = f64::from(raid.center.y) - f64::from(center.y);
            let dz = f64::from(raid.center.z) - f64::from(center.z);
            let dist = dx * dx + dy * dy + dz * dz;
            if dist < radius_sqr && best.is_none_or(|(_, d)| dist < d) {
                best = Some((id, dist));
            }
        }
        best.map(|(id, _)| id)
    }

    /// 编成 `raids.dat` 的 `data` 复合标签，字段顺序照 `Raids.CODEC`。
    pub fn to_tag(&self) -> Tag {
        let list = self
            .raids
            .iter()
            .map(|(&id, raid)| raid.to_tag(id))
            .collect();
        Tag::Compound(vec![
            ("raids".to_owned(), Tag::List(list)),
            ("next_id".to_owned(), Tag::Int(self.next_id)),
            ("tick".to_owned(), Tag::Int(self.tick)),
        ])
    }

    /// 从 `raids.dat` 的 `data` 复合标签解码。官方 `Raids.load` 用
    /// `resultOrPartial(...).orElseGet(Raids::new)`，所以任一条坏数据都让整份
    /// 退回默认构造；这里照这个语义：结构不对、或某条解不出来，就返回默认值。
    pub fn from_tag(raw: &Tag) -> Self {
        let Tag::Compound(_) = raw else {
            return Self::default();
        };
        let Some(Tag::List(list)) = tag::field(raw, "raids") else {
            return Self::default();
        };
        let mut raids = std::collections::BTreeMap::new();
        for entry in list {
            let Some((id, raid)) = Raid::from_tag(entry) else {
                return Self::default();
            };
            raids.insert(id, raid);
        }
        // `next_id` / `tick` 是必填 INT，缺了按官方整份作废。
        let (Some(Tag::Int(next_id)), Some(Tag::Int(tick))) =
            (tag::field(raw, "next_id"), tag::field(raw, "tick"))
        else {
            return Self::default();
        };
        Self {
            raids,
            next_id: *next_id,
            tick: *tick,
        }
    }
}

/// `Raid.random` 的初始种子：官方用 `RandomSource.create()`（每次都不同），
/// 这里按中心派生一个确定值，逐刻场景可复现；官方不存档这个随机源。
fn random_seed(center: BlockPos) -> u64 {
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
    seed ^= (center.x as u64).wrapping_mul(0x100_0000_01B3);
    seed ^= (center.y as u64).wrapping_mul(0x9E37_79B1);
    seed ^= (center.z as u64).wrapping_mul(0x85EB_CA6B);
    (seed ^ 0x5DEE_CE66D) & ((1u64 << 48) - 1)
}

/// `BlockPos.CODEC` 的 NBT：三个 int 的 `IntArray`。
fn block_pos_tag(pos: BlockPos) -> Tag {
    Tag::IntArray(vec![pos.x, pos.y, pos.z])
}

fn block_pos_from(raw: &Tag, key: &str) -> Option<BlockPos> {
    let Tag::IntArray(words) = tag::field(raw, key)? else {
        return None;
    };
    let [x, y, z] = words.as_slice() else {
        return None;
    };
    Some(BlockPos::new(*x, *y, *z))
}

/// `UUIDUtil.CODEC` 的 NBT：四个 int 的 `IntArray`（每个 int 是 4 个大端字节）。
fn uuid_tag(bytes: &[u8; 16]) -> Tag {
    let mut words = [0i32; 4];
    for (index, word) in words.iter_mut().enumerate() {
        let mut value = 0u32;
        for &byte in &bytes[index * 4..index * 4 + 4] {
            value = value << 8 | u32::from(byte);
        }
        *word = value as i32;
    }
    Tag::IntArray(words.to_vec())
}

fn uuid_from(raw: &Tag) -> Option<[u8; 16]> {
    let Tag::IntArray(words) = raw else {
        return None;
    };
    let [a, b, c, d] = words.as_slice() else {
        return None;
    };
    let mut bytes = [0u8; 16];
    for (output, word) in bytes.chunks_mut(4).zip([*a, *b, *c, *d]) {
        output.copy_from_slice(&(word as u32).to_be_bytes());
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_raid(center: BlockPos, status: RaidStatus) -> Raid {
        let mut raid = Raid::new(center, DIFFICULTY_HARD);
        raid.started = true;
        raid.active = false;
        raid.ticks_active = 12345;
        raid.raid_omen_level = 3;
        raid.groups_spawned = 4;
        raid.cooldown_ticks = 42;
        raid.post_raid_ticks = 7;
        raid.total_health = 123.5;
        raid.status = status;
        raid.heroes_of_the_village = vec![[1u8; 16], [0xABu8; 16]];
        raid.wave_spawn_pos = Some(BlockPos::new(9, 8, 7));
        raid.celebration_ticks = 55;
        raid
    }

    /// 默认 Raids 的 nextId 是 1，往返不变。
    #[test]
    fn default_raids_round_trip() {
        let raids = Raids::new();
        assert_eq!(raids.next_id, 1);
        assert_eq!(raids.tick, 0);
        assert!(raids.raids.is_empty());
        assert_eq!(Raids::from_tag(&raids.to_tag()), raids);
    }

    /// 顶层字段名和类型照 Raids.CODEC。
    #[test]
    fn top_level_tag_shape() {
        let Tag::Compound(entries) = Raids::new().to_tag() else {
            panic!("compound");
        };
        let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["raids", "next_id", "tick"]);
        assert!(matches!(entries[0].1, Tag::List(_)));
        assert!(matches!(entries[1].1, Tag::Int(1)));
        assert!(matches!(entries[2].1, Tag::Int(0)));
    }

    /// 单条字段名 / 顺序 / 类型照 RaidWithId.CODEC + MAP_CODEC。
    #[test]
    fn raid_tag_shape() {
        let tag = sample_raid(BlockPos::new(1, 2, 3), RaidStatus::Victory).to_tag(9);
        let Tag::Compound(entries) = tag else {
            panic!("compound");
        };
        let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            [
                "id",
                "started",
                "active",
                "ticks_active",
                "raid_omen_level",
                "groups_spawned",
                "cooldown_ticks",
                "post_raid_ticks",
                "total_health",
                "group_count",
                "status",
                "center",
                "heroes_of_the_village"
            ]
        );
        assert!(matches!(entries[1].1, Tag::Byte(1)));
        assert!(matches!(entries[2].1, Tag::Byte(0)));
        assert!(matches!(entries[3].1, Tag::Long(12345)));
        assert!(matches!(entries[8].1, Tag::Float(_)));
        assert!(matches!(&entries[10].1, Tag::String(s) if s == "victory"));
        assert!(matches!(&entries[11].1, Tag::IntArray(v) if v == &[1, 2, 3]));
        assert!(matches!(&entries[12].1, Tag::List(v) if v.len() == 2));
    }

    /// 多场袭击、各状态、heroes、UUID 全部往返；非存档字段读盘后归零 / 清空。
    #[test]
    fn multiple_raids_round_trip() {
        let mut raids = Raids::new();
        raids.next_id = 40;
        raids.tick = 555;
        raids.raids.insert(
            1,
            sample_raid(BlockPos::new(-5, 64, 20), RaidStatus::Ongoing),
        );
        raids.raids.insert(
            7,
            sample_raid(BlockPos::new(100, 70, -300), RaidStatus::Loss),
        );
        let decoded = Raids::from_tag(&raids.to_tag());
        assert_eq!(decoded.next_id, 40);
        assert_eq!(decoded.tick, 555);
        assert_eq!(decoded.raids.len(), 2);
        for (id, original) in &raids.raids {
            let got = &decoded.raids[id];
            assert_eq!(got.status, original.status);
            assert_eq!(got.center, original.center);
            assert_eq!(got.ticks_active, original.ticks_active);
            assert_eq!(got.total_health, original.total_health);
            assert_eq!(got.heroes_of_the_village, original.heroes_of_the_village);
            // 非存档字段读盘后重置。
            assert_eq!(got.wave_spawn_pos, None);
            assert_eq!(got.celebration_ticks, 0);
        }
    }

    /// 坏结构 / 坏状态 / 缺必填字段都整份退回默认值。
    #[test]
    fn bad_tags_fall_back_to_default() {
        assert_eq!(Raids::from_tag(&Tag::Int(3)), Raids::default());
        // 缺 next_id / tick。
        assert_eq!(
            Raids::from_tag(&Tag::Compound(vec![(
                "raids".to_owned(),
                Tag::List(Vec::new())
            )])),
            Raids::default()
        );
        // 一条坏 status 让整份作废。
        let mut bad = sample_raid(BlockPos::new(0, 0, 0), RaidStatus::Ongoing).to_tag(1);
        if let Tag::Compound(entries) = &mut bad
            && let Some((_, Tag::String(s))) = entries.iter_mut().find(|(k, _)| k == "status")
        {
            *s = "nonsense".to_owned();
        }
        let raids = Raids::from_tag(&Tag::Compound(vec![
            ("raids".to_owned(), Tag::List(vec![bad])),
            ("next_id".to_owned(), Tag::Int(2)),
            ("tick".to_owned(), Tag::Int(0)),
        ]));
        assert_eq!(raids, Raids::default());
    }

    /// getNumGroups 四档难度。
    #[test]
    fn num_groups_matches_vanilla() {
        assert_eq!(num_groups(DIFFICULTY_PEACEFUL), 0);
        assert_eq!(num_groups(DIFFICULTY_EASY), 3);
        assert_eq!(num_groups(DIFFICULTY_NORMAL), 5);
        assert_eq!(num_groups(DIFFICULTY_HARD), 7);
        assert_eq!(num_groups(99), 0);
    }

    /// RaiderType 波次表照官方枚举。
    #[test]
    fn wave_tables_match_vanilla() {
        assert_eq!(
            RaiderType::Vindicator.spawns_per_wave(),
            &[0, 0, 2, 0, 1, 4, 2, 5]
        );
        assert_eq!(
            RaiderType::Evoker.spawns_per_wave(),
            &[0, 0, 0, 0, 0, 1, 1, 2]
        );
        assert_eq!(
            RaiderType::Pillager.spawns_per_wave(),
            &[0, 4, 3, 3, 4, 4, 4, 2]
        );
        assert_eq!(
            RaiderType::Witch.spawns_per_wave(),
            &[0, 0, 0, 0, 3, 0, 0, 1]
        );
        assert_eq!(
            RaiderType::Ravager.spawns_per_wave(),
            &[0, 0, 0, 1, 0, 1, 0, 2]
        );
    }

    /// getDefaultNumSpawns：普通波取 wave 列，奖励波取 numGroups 列。
    #[test]
    fn default_num_spawns_follows_wave_or_bonus() {
        let raid = Raid::new(BlockPos::new(0, 0, 0), DIFFICULTY_NORMAL);
        assert_eq!(raid.group_count, 5);
        assert_eq!(raid.default_num_spawns(RaiderType::Pillager, 1, false), 4);
        assert_eq!(raid.default_num_spawns(RaiderType::Vindicator, 2, false), 2);
        // 奖励波用 group_count=5 这一列。
        assert_eq!(
            raid.default_num_spawns(RaiderType::Vindicator, 9, true),
            RaiderType::Vindicator.spawns_per_wave()[5]
        );
        // 越界列不 panic。
        assert_eq!(raid.default_num_spawns(RaiderType::Witch, 99, false), 0);
    }

    /// absorbRaidOmen 累加 amplifier+1 并夹到 5，无效果返回 false。
    #[test]
    fn absorb_raid_omen_clamps() {
        let mut raid = Raid::new(BlockPos::new(0, 0, 0), DIFFICULTY_NORMAL);
        assert!(!raid.absorb_raid_omen(None));
        assert_eq!(raid.raid_omen_level, 0);
        assert!(raid.absorb_raid_omen(Some(0)));
        assert_eq!(raid.raid_omen_level, 1);
        assert!(raid.absorb_raid_omen(Some(3)));
        assert_eq!(raid.raid_omen_level, 5);
        assert!(raid.absorb_raid_omen(Some(4)));
        assert_eq!(raid.raid_omen_level, 5);
    }

    /// getEnchantOdds 四档。
    #[test]
    fn enchant_odds_match_vanilla() {
        for (level, odds) in [(0, 0.0), (1, 0.0), (2, 0.1), (3, 0.25), (4, 0.5), (5, 0.75)] {
            let mut raid = Raid::new(BlockPos::new(0, 0, 0), DIFFICULTY_NORMAL);
            raid.raid_omen_level = level;
            assert_eq!(raid.enchant_odds(), odds);
        }
    }

    /// 波次判定：最后一波、奖励波、hasMoreWaves、shouldSpawnGroup。
    #[test]
    fn wave_predicates_match_vanilla() {
        let mut raid = Raid::new(BlockPos::new(0, 0, 0), DIFFICULTY_NORMAL);
        assert_eq!(raid.group_count, 5);
        assert!(!raid.has_first_wave_spawned());
        assert!(!raid.is_final_wave());
        assert!(raid.has_more_waves());
        raid.groups_spawned = 5;
        assert!(raid.is_final_wave());
        assert!(!raid.has_more_waves());
        assert!(!raid.has_bonus_wave());
        assert!(!raid.should_spawn_bonus_group(0));
        raid.raid_omen_level = 2;
        assert!(raid.has_bonus_wave());
        assert!(raid.should_spawn_bonus_group(0));
        assert!(!raid.should_spawn_bonus_group(1));
        assert!(raid.has_more_waves());
        raid.groups_spawned = 6;
        assert!(raid.has_spawned_bonus_wave());
        assert!(!raid.has_more_waves());
        // shouldSpawnGroup 要冷却到 0 且场上清空。
        raid.groups_spawned = 0;
        raid.cooldown_ticks = 300;
        assert!(!raid.should_spawn_group(0));
        raid.cooldown_ticks = 0;
        assert!(raid.should_spawn_group(0));
        assert!(!raid.should_spawn_group(1));
    }

    /// isBetweenWaves / bossbar 进度 / hero 去重 / getUniqueId / nearbyRaid。
    #[test]
    fn misc_pure_helpers() {
        let mut raid = Raid::new(BlockPos::new(0, 0, 0), DIFFICULTY_NORMAL);
        assert!(!raid.is_between_waves(0));
        raid.groups_spawned = 1;
        assert!(raid.is_between_waves(0));
        assert!(!raid.is_between_waves(2));
        raid.total_health = 100.0;
        assert_eq!(raid.bossbar_progress(25.0), 0.25);
        assert_eq!(raid.bossbar_progress(500.0), 1.0);
        assert_eq!(raid.bossbar_progress(-5.0), 0.0);
        raid.total_health = 0.0;
        assert_eq!(raid.bossbar_progress(10.0), 0.0);
        raid.add_hero_of_the_village([3u8; 16]);
        raid.add_hero_of_the_village([3u8; 16]);
        assert_eq!(raid.heroes_of_the_village.len(), 1);

        let mut raids = Raids::new();
        assert_eq!(raids.unique_id(), 1);
        assert_eq!(raids.unique_id(), 2);
        assert_eq!(raids.next_id, 3);
        let mut a = Raid::new(BlockPos::new(0, 64, 0), DIFFICULTY_NORMAL);
        let mut b = Raid::new(BlockPos::new(10, 64, 0), DIFFICULTY_NORMAL);
        raids.raids.insert(5, a.clone());
        raids.raids.insert(6, b.clone());
        assert_eq!(raids.nearby_raid(BlockPos::new(1, 64, 0), 100.0), Some(5));
        a.active = false;
        raids.raids.insert(5, a);
        assert_eq!(raids.nearby_raid(BlockPos::new(1, 64, 0), 100.0), Some(6));
        b.active = false;
        raids.raids.insert(6, b);
        assert_eq!(raids.nearby_raid(BlockPos::new(1, 64, 0), 100.0), None);
    }

    /// 用一串预设的 `nextInt` 结果驱动 `potential_bonus_spawns`，同时记下每次
    /// 取数的 bound，好核对官方的取数次数与顺序。
    fn run_bonus(
        raider: RaiderType,
        wave: i32,
        difficulty: i32,
        bonus: bool,
        values: Vec<i32>,
    ) -> (i32, Vec<i32>) {
        let mut bounds = Vec::new();
        let mut values = values.into_iter();
        let mut roll = |bound: i32| {
            bounds.push(bound);
            values.next().unwrap_or(0)
        };
        let result = Raid::potential_bonus_spawns(raider, wave, difficulty, bonus, &mut roll);
        (result, bounds)
    }

    /// 袭击名册的加减与总血量记账。
    #[test]
    fn raider_roster_tracks_health() {
        let mut raid = Raid::new(BlockPos::new(0, 0, 0), DIFFICULTY_NORMAL);
        raid.add_wave_mob(1, 10, 24.0, true);
        raid.add_wave_mob(1, 11, 24.0, true);
        raid.add_wave_mob(2, 12, 100.0, true);
        // 读盘恢复走 `addWaveMob(..., false)`：同 uid 不重复记账、不重复加血。
        raid.add_wave_mob(1, 10, 24.0, false);
        assert_eq!(raid.total_raiders(), 3);
        assert_eq!(raid.total_health, 148.0);
        assert!(raid.remove_raider(1, 11, 24.0, true));
        assert_eq!(raid.total_raiders(), 2);
        assert_eq!(raid.total_health, 124.0);
        // 不在名册 / 波次不存在都返回假，不改血量。
        assert!(!raid.remove_raider(1, 999, 24.0, true));
        assert!(!raid.remove_raider(9, 12, 100.0, true));
        assert_eq!(raid.total_health, 124.0);
        // 读盘后名册清空（运行时字段不存档）。
        let decoded = Raids::from_tag(
            &Raids {
                raids: std::collections::BTreeMap::from([(1, raid.clone())]),
                ..Raids::new()
            }
            .to_tag(),
        );
        assert!(decoded.raids[&1].raiders.is_empty());
        assert!(decoded.raids[&1].leaders.is_empty());
    }

    /// potential_bonus_spawns 的分支、结果与取数顺序照字节码。
    #[test]
    fn bonus_spawns_branches_and_draw_order() {
        // 普通：一次 roll(2)；困难：一次 roll(3)。
        assert_eq!(
            run_bonus(RaiderType::Pillager, 3, DIFFICULTY_NORMAL, false, vec![1]),
            (1, vec![2])
        );
        assert_eq!(
            run_bonus(RaiderType::Vindicator, 3, DIFFICULTY_HARD, false, vec![2]),
            (2, vec![3])
        );
        // 简单：先 roll(2)，为 0 直接返回（一次取数）；为 1 再 roll(2)（两次）。
        assert_eq!(
            run_bonus(RaiderType::Pillager, 3, DIFFICULTY_EASY, false, vec![0]),
            (0, vec![2])
        );
        assert_eq!(
            run_bonus(RaiderType::Pillager, 3, DIFFICULTY_EASY, false, vec![1, 1]),
            (1, vec![2, 2])
        );
        // 女巫命中：一次 roll(2)；不命中 / 简单：不取数。
        assert_eq!(
            run_bonus(RaiderType::Witch, 3, DIFFICULTY_NORMAL, false, vec![1]),
            (1, vec![2])
        );
        assert_eq!(
            run_bonus(RaiderType::Witch, 4, DIFFICULTY_NORMAL, false, vec![]),
            (0, vec![])
        );
        assert_eq!(
            run_bonus(RaiderType::Witch, 3, DIFFICULTY_EASY, false, vec![]),
            (0, vec![])
        );
        // 劫掠兽：非简单且是奖励波才一次 roll(2)；唤魔者直接 0。
        assert_eq!(
            run_bonus(RaiderType::Ravager, 5, DIFFICULTY_HARD, true, vec![1]),
            (1, vec![2])
        );
        assert_eq!(
            run_bonus(RaiderType::Ravager, 5, DIFFICULTY_HARD, false, vec![]),
            (0, vec![])
        );
        assert_eq!(
            run_bonus(RaiderType::Evoker, 5, DIFFICULTY_HARD, false, vec![]),
            (0, vec![])
        );
    }
}
