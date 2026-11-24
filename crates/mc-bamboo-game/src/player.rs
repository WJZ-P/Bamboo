//! 玩家在世界里的状态。
//!
//! 每个领域的状态放在各自模块的类型里，这里只把它们组合起来。

use mc_bamboo_blocks::entity::{
    JavaRandom,
    ctx::PlayerView,
    mob::{attributes::Attributes, effects::EffectMap, enchantment_effects::ActiveLocationEffects},
};
use mc_bamboo_core::{Aabb, BlockPos, ChunkPos, Hand, Vec3};
use mc_bamboo_protocol::{
    item::ItemStack,
    packet::play::{ClientInformation, EquipmentSlot, GameMode, MODEL_PART_HAT},
};
use uuid::Uuid;

use crate::{
    chat::SpamThrottlers,
    chunk_tracking::{ChunkTracker, MIN_VIEW_DISTANCE},
    container::EnderChest,
    entity_sync::EntitySync,
    experience::Experience,
    inbound::PlayerId,
    interaction::{BlockPrediction, mining::Mining},
    inventory::Inventory,
    menu::station::InventoryCursor,
    movement::Teleport,
    player_grid::PlayerGrid,
    player_health::PlayerHealth,
    player_move::Motion,
    secure_chat::PlayerChat,
    world_state::RespawnData,
};

const STANDING_EYE_HEIGHT: f64 = 1.62;
const CROUCHING_EYE_HEIGHT: f64 = 1.27;
const STANDING_HEIGHT: f64 = 1.8;
const CROUCHING_HEIGHT: f64 = 1.5;
/// 碰撞盒宽度的一半。
const HALF_WIDTH: f64 = 0.3;

/// 世界里的一名玩家。
pub(crate) struct Player {
    pub(crate) id: PlayerId,
    pub(crate) uuid: Uuid,
    pub(crate) name: String,
    pub(crate) game_mode: GameMode,
    /// 命令权限等级，0 到 4，来自 `ops.json`。
    ///
    /// Command permission level, 0 through 4, from `ops.json`.
    pub(crate) permission_level: u8,
    /// 是否列在 `ops.json` 里。和权限等级分开，等级 0 的 OP 也算 OP。
    ///
    /// Whether the player is listed in `ops.json`. Separate from the level: a level-0 op still counts.
    pub(crate) is_op: bool,
    /// 实体位置，也就是脚底。只能经 [`Player::set_position`] 改，见 [`PlayerGrid`]。
    ///
    /// Entity position, the feet. Changed only through [`Player::set_position`]; see [`PlayerGrid`].
    position: Vec3,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    /// 每 tick 由派生状态的阶段写成 `yaw`。
    pub(crate) head_yaw: f32,
    pub(crate) on_ground: bool,
    /// 移动校验用的服务端状态，对应 `ServerGamePacketListenerImpl` 里跟位置有关的字段。
    ///
    /// Server-side movement state, the position fields of `ServerGamePacketListenerImpl`.
    pub(crate) motion: Motion,
    /// 最近一次收到的客户端设置。
    pub(crate) client: ClientInformation,
    pub(crate) teleport: Teleport,
    pub(crate) chunks: ChunkTracker,
    pub(crate) sync: EntitySync,
    pub(crate) prediction: BlockPrediction,
    /// 正在挖哪个方块、挖了多久，官方 `ServerPlayerGameMode` 的那几个字段。
    pub(crate) mining: Mining,
    pub(crate) inventory: Inventory,
    /// 没有方块菜单时，玩家物品栏里的 2×2 合成。光标和远程槽都在这里。
    ///
    /// The 2×2 crafting grid while no block menu is open. The cursor and remote slots live here.
    pub(crate) inventory_cursor: InventoryCursor,
    /// 官方 `containerCounter`，每打开一次菜单加一，100 回绕到 1。
    pub(crate) container_counter: i32,
    /// 这名玩家自己的末影箱，每个玩家一份。
    pub(crate) ender_chest: EnderChest,
    pub(crate) spam: SpamThrottlers,
    /// 丢出物品、铁砧损坏和附魔换种子用的 `this.random`。种子由玩家 ID 决定。
    ///
    /// `this.random` for thrown items, anvil damage and enchantment seeds.
    pub(crate) random: JavaRandom,
    /// 官方 `Player.enchantmentSeed`，进服时从 `this.random` 取。
    ///
    /// Vanilla `Player.enchantmentSeed`, drawn from `this.random` on join.
    pub(crate) enchantment_seed: i32,
    /// 官方 `Player` 的经验等级、进度和总经验。
    ///
    /// Vanilla `Player`'s experience level, progress and total.
    pub(crate) experience: Experience,
    /// 官方 `Player.takeXpDelay`，每刻减 1，为 0 才能碰到经验球。
    ///
    /// Vanilla `Player.takeXpDelay`: decremented each tick; an orb is taken only at zero.
    pub(crate) take_xp_delay: i32,
    /// 官方 `Player.DATA_SCORE_ID`（存档里的 `Score`），`giveExperiencePoints` 也加它。
    ///
    /// Vanilla `Player.DATA_SCORE_ID` (the saved `Score`); `giveExperiencePoints` adds to it too.
    pub(crate) score: i32,
    /// 官方 `ServerPlayer.lastSentExp`，构造时是 `-99999999`；总经验变了才发包。
    ///
    /// Vanilla `ServerPlayer.lastSentExp`, `-99999999` on construction. A packet
    /// goes out only when the total changes.
    pub(crate) last_sent_exp: i32,
    /// 创造模式把物品拖出界面的 `TickThrottler(20, 1480)`。
    ///
    /// Creative drags out of the screen use `TickThrottler(20, 1480)`.
    pub(crate) drop_spam: i32,
    /// 公钥、消息链和已读窗口。没有会话时按无签名处理。
    ///
    /// Public key, message chain, and the seen-message window. Without a session, chat stays unsigned.
    pub(crate) chat: PlayerChat,
    /// 官方 `Player.sleepCounter` / `SLEEPING_POS_ID`：睡在床上时非空。
    ///
    /// Official `Player.sleepCounter` / `SLEEPING_POS_ID`: set while in bed.
    pub(crate) sleep: Option<crate::sleep::Sleeping>,
    /// 官方 `ServerPlayer.respawnConfig`：`/spawnpoint` 设的玩家重生点。
    ///
    /// Official `ServerPlayer.respawnConfig`: the player's spawn set by `/spawnpoint`.
    pub(crate) respawn: Option<RespawnData>,
    /// 进服 / 重生时等搜索覆盖的区块就绪，还没发最终位置包。等待期间不跑物理、
    /// 不受伤、不处理移动包（官方 `PrepareSpawnTask` 在进服前把区块准备好）。
    ///
    /// Join / respawn is waiting for the search-covered chunks before sending
    /// the final position packet. No physics, hurt or movement while waiting
    /// (vanilla `PrepareSpawnTask` prepares chunks before the player enters).
    pub(crate) spawn_pending: Option<crate::spawn_landing::PendingLanding>,
    /// 这名玩家自己的入站队列（岛的实施计划第 6 节第 4 步）。世界线程每刻取一次，
    /// 和主控队列、别的玩家的队列按序号归并；跟着玩家走。
    ///
    /// This player's own inbound queue (island plan, section 6, step 4). The world
    /// drains it each tick and merges it with the master queue and the other
    /// players' queues by number; it moves with the player.
    pub(crate) inbox: crate::inbound::PlayerInbox,
    /// 官方 `ServerPlayer` 的六个 `lastRecorded*` 字段（计分板的只读准则），顺序见
    /// [`tick_criteria`](crate::scoreboard::tick_criteria)。新玩家是构造函数的初值。
    ///
    /// The six official `ServerPlayer.lastRecorded*` fields (the scoreboard's
    /// read-only criteria), ordered as in
    /// [`tick_criteria`](crate::scoreboard::tick_criteria). A new player starts at
    /// the constructor defaults.
    pub(crate) recorded_criteria: [i32; 6],
    /// 官方 `Player` / `LivingEntity` 的生命、受伤冷却和饥饿状态。
    /// 全都是相对倒计时，随 `IslandPart::Players` 整行搬，不需要时钟平移。
    ///
    /// Health, hurt cooldowns and hunger. All relative countdowns, moved with
    /// the whole player row and needing no clock rebasing.
    pub(crate) health: PlayerHealth,
    /// `LivingEntity.activeEffects`：和生物层共用同一份表，只存相对剩余刻数。
    ///
    /// `LivingEntity.activeEffects`: the same shared map as mobs, with
    /// relative countdowns only.
    pub(crate) effects: EffectMap,
    /// `LivingEntity.attributes`：玩家属性表，基础值来自 `Player.createAttributes`。
    /// 每刻由 [`crate::player_attributes::rebuild`] 从装备 / 效果 / 游戏模式重算修饰符。
    ///
    /// The player attribute map. Every tick
    /// [`crate::player_attributes::rebuild`] recomputes its modifiers from
    /// equipment, effects and game mode.
    pub(crate) attributes: Attributes,
    /// `LivingEntity.activeLocationDependentEnchantments`：`location_changed` 效果的
    /// 激活状态，以及灵魂疾行临时修饰符的当前等级（0 表示没挂）。
    ///
    /// `LivingEntity.activeLocationDependentEnchantments`: active state of each
    /// `location_changed` effect, plus the level of the soul-speed transient
    /// modifiers (0 means none).
    pub(crate) location_effects: ActiveLocationEffects,
    pub(crate) soul_speed_level: i32,
    /// `LivingEntity.lastPos`：整数方块坐标跨格才重跑 `runLocationChangedEffects`。
    ///
    /// `LivingEntity.lastPos`: `runLocationChangedEffects` re-runs only when
    /// the integer block position changes.
    pub(crate) last_location_pos: Option<BlockPos>,
    /// 上一刻靴子槽的物品；变化时先停掉旧的 location 效果再立即重跑。
    ///
    /// Last tick's boots stack; on change the old location effects stop and
    /// the current ones run immediately.
    pub(crate) last_location_boots: ItemStack,
    /// `Entity` 的环境状态：着火、氧气、细雪冻结。全是相对倒计时。
    ///
    /// `Entity` environment state: fire, air, powder-snow freeze. All relative counters.
    pub(crate) environment: crate::player_environment::Environment,
    /// `LivingEntity.useItem` / `useItemRemaining`：正在吃东西 / 喝东西。
    pub(crate) use_item: Option<crate::player_consume::UsingItem>,
    /// `LivingEntity.useItem` 里的弓和弩。
    ///
    /// A bow or crossbow in `LivingEntity.useItem`.
    pub(crate) weapon_use: Option<crate::player_weapon::WeaponUse>,
    /// `LivingEntity` 的自旋攻击状态（激流三叉戟）。
    ///
    /// The `LivingEntity` auto-spin attack state (a riptide trident).
    pub(crate) auto_spin_attack: crate::player_auto_spin::AutoSpinAttack,
    /// 官方 `Player.attackStrengthTicker`：`aiStep` 每刻加一，攻击 / 挥空 / 换物品清零。
    /// 相对计数，随玩家整行搬，不需要时钟平移。
    ///
    /// Vanilla `Player.attackStrengthTicker`: incremented every `aiStep` tick and
    /// reset on attack, air swing or item swap. A relative counter needing no rebasing.
    pub(crate) attack_strength_ticker: i32,
    /// 官方 `Player.lastItemInMainHand`：主手物品变了才检查要不要重置冷却。
    ///
    /// Vanilla `Player.lastItemInMainHand`: the previous main-hand stack, used to
    /// detect the item swap that resets the attack cooldown.
    pub(crate) last_main_hand_item: ItemStack,
    pub(crate) item_use_stats: crate::item_use_stats::ItemUseStats,
    /// `Player.cooldowns`：右键使用后的物品冷却。
    pub(crate) cooldowns: crate::item_cooldowns::ItemCooldowns,
    /// `ServerPlayer.enderPearls`：这名玩家扔出、还在飞的珍珠实体号。
    pub(crate) ender_pearls: std::collections::BTreeSet<i32>,
    /// `saveEnderPearls` 的 NBT 快照；`runtime_player_data` 是 `&Player` 的纯函数，
    /// 每刻由 `ender_pearl::tick` 从实体表刷新。
    pub(crate) saved_ender_pearls: Vec<mc_bamboo_nbt::Tag>,
    /// 这一刻累计的饥饿消耗，`player_exhaustion::flush` 在 tick 末尾写入。
    pub(crate) pending_exhaustion: f32,
    /// `ServerPlayer.lastSentHealth` / `lastSentFood` / `lastFoodSaturationZero` 的合并快照。
    /// 官方构造时是 -1e8 / -99999999 / true，进服第一刻必发一包 `set_health`。
    ///
    /// The combined official "last sent health" snapshot. The constructor values
    /// force one `set_health` on the first tick.
    pub(crate) health_sync: crate::player_hurt::HealthSync,
    /// `CombatTracker.inCombat`：第一次被非玩家生物打伤时置真，1b 的死亡消息要用。
    pub(crate) in_combat: bool,
    /// `CombatTracker.entries`：`getDeathMessage` 要按它们找最显著的一次摔落。
    /// 不存绝对时刻；条目的清空时机照官方 `recheckStatus`。
    ///
    /// `CombatTracker.entries`: `getDeathMessage` picks the most significant
    /// fall from them. No absolute instants; cleared like `recheckStatus`.
    pub(crate) combat_entries: Vec<crate::player_death::CombatEntry>,
    /// 距最后一次 `recordDamage` 的刻数（相对倒计时，官方用 tickCount 差）。
    pub(crate) combat_idle_ticks: i32,
    /// 官方 `LivingEntity.lastHurtByPlayer` / `lastHurtByMob`（击杀归属）。
    /// 显示组件在受伤那一刻就定下来；`memory` / `age` 都是相对倒计时，
    /// 岛拆合时整行跟着 `Players` 搬，不需要时钟平移。
    ///
    /// Vanilla `lastHurtByPlayer` / `lastHurtByMob` (kill credit). The display
    /// component is captured at hurt time; `memory` / `age` are relative
    /// countdowns so island splits and merges need no rebasing.
    pub(crate) hurt_by_player: Option<crate::player_death::KillCredit>,
    pub(crate) hurt_by_mob: Option<crate::player_death::KillCredit>,
    /// `ServerPlayer.dead`：一次死亡流程正在跑，重生之前不再触发第二次死亡。
    /// 与 `health.health <= 0` 分开存，因为 `restoreFrom` 会把血量设满。
    ///
    /// `ServerPlayer.dead`: a death is in progress; no second death until
    /// respawn. Separate from zero health because `restoreFrom` refills it.
    pub(crate) dead: bool,
    /// `ServerPlayer.lastDeathLocation`：死亡时写下，`Player.readAdditionalSaveData`
    /// 和 `restoreFrom` 双向保留；重生包的 `CommonPlayerSpawnInfo` 也读它。
    ///
    /// Vanilla `lastDeathLocation`, written at death, saved and restored.
    pub(crate) last_death_location: Option<BlockPos>,
    /// 实体 tick 数（`ServerPlayer.tickCount`），官方和平回血用它的模。
    /// 存相对计数而不是岛时钟的绝对时刻，拆岛 / 并岛无需平移。
    ///
    /// Entity tick count, used modulo 20/10 by peaceful regeneration. Relative,
    /// so island splits and merges need no rebasing.
    pub(crate) entity_ticks: i32,
    /// `Stats.CUSTOM.get(Stats.TIME_SINCE_REST)`：没睡觉的每一刻 +1，开始睡觉清零。
    /// 幻翼自定义刷怪器读它决定要不要在玩家头顶刷怪。
    ///
    /// `Stats.CUSTOM.get(Stats.TIME_SINCE_REST)`: +1 on every non-sleeping tick,
    /// reset on sleep start. The phantom custom spawner reads it.
    pub(crate) time_since_rest: i32,
    /// `lastHurtByMob` / `lastHurtMob` / `lastDamageSource`，狼保护主人时读。
    ///
    /// Combat memory read by tamed wolves guarding their owner.
    pub(crate) combat_memory: crate::player_combat_memory::CombatMemory,
    /// `ServerPlayer.respawnConfig` 之外，死亡后一律等待客户端发来的
    /// `PERFORM_RESPAWN`：`immediate_respawn` 只影响客户端要不要显示死亡界面，
    /// 服务端不自己重生，客户端不发就一直是死的。
    ///
    /// After death, always wait for the client's `PERFORM_RESPAWN`:
    /// `immediate_respawn` only tells the client whether to show the death
    /// screen. The server never respawns on its own; without a client packet
    /// the player stays dead.
    pub(crate) awaiting_respawn: bool,
    /// 骑乘状态：骑着的载具和载具移动校验用的几个位置。
    ///
    /// Riding state: the vehicle and the positions its movement is checked against.
    pub(crate) riding: crate::riding::Riding,
    /// 官方 `ServerPlayer.getAdvancements()` 的可变状态，跟着玩家在岛之间搬动。
    pub(crate) advancements: crate::advancements::PlayerAdvancements,
}

impl Player {
    pub(crate) fn new(
        id: PlayerId,
        uuid: Uuid,
        name: String,
        game_mode: GameMode,
        client: ClientInformation,
        position: Vec3,
    ) -> Self {
        let random = crate::drops::player_random(id);
        // 官方 `Player` 构造函数把 `enchantmentSeed` 设为 0；只有读档时
        // `XpSeed == 0` 才从 `this.random` 抽一次（见 player_save 的恢复函数）。
        //
        // Official `Player`'s constructor leaves `enchantmentSeed` at 0; only
        // a load with `XpSeed == 0` draws once from `this.random`
        // (see [`restore_experience_seed`]).
        let enchantment_seed = 0;
        let mut player = Self {
            id,
            uuid,
            name,
            game_mode,
            permission_level: 0,
            is_op: false,
            position,
            yaw: 0.0,
            pitch: 0.0,
            head_yaw: 0.0,
            on_ground: false,
            motion: Motion::new(position),
            sync: EntitySync::new(position, &client),
            client,
            teleport: Teleport::default(),
            chunks: ChunkTracker::default(),
            prediction: BlockPrediction::default(),
            mining: Mining::default(),
            inventory: Inventory::default(),
            inventory_cursor: InventoryCursor::new(),
            container_counter: 0,
            ender_chest: EnderChest::default(),
            spam: SpamThrottlers::default(),
            random,
            enchantment_seed,
            experience: Experience::default(),
            take_xp_delay: 0,
            score: 0,
            last_sent_exp: -99_999_999,
            drop_spam: 0,
            chat: PlayerChat::default(),
            sleep: None,
            respawn: None,
            spawn_pending: None,
            inbox: crate::inbound::PlayerInbox::default(),
            recorded_criteria: crate::scoreboard::UNRECORDED_CRITERIA,
            health: PlayerHealth::default(),
            effects: EffectMap::new(),
            attributes: Attributes::player(),
            location_effects: ActiveLocationEffects::default(),
            soul_speed_level: 0,
            last_location_pos: None,
            last_location_boots: ItemStack::empty(),
            environment: crate::player_environment::Environment::default(),
            use_item: None,
            weapon_use: None,
            auto_spin_attack: crate::player_auto_spin::AutoSpinAttack::default(),
            attack_strength_ticker: 0,
            last_main_hand_item: ItemStack::empty(),
            item_use_stats: Default::default(),
            cooldowns: crate::item_cooldowns::ItemCooldowns::default(),
            ender_pearls: std::collections::BTreeSet::new(),
            saved_ender_pearls: Vec::new(),
            pending_exhaustion: 0.0,
            health_sync: crate::player_hurt::HealthSync::INITIAL,
            in_combat: false,
            combat_entries: Vec::new(),
            combat_idle_ticks: 0,
            hurt_by_player: None,
            hurt_by_mob: None,
            dead: false,
            last_death_location: None,
            entity_ticks: 0,
            time_since_rest: 0,
            combat_memory: Default::default(),
            awaiting_respawn: false,
            riding: crate::riding::Riding::default(),
            advancements: crate::advancements::PlayerAdvancements::default(),
        };
        crate::player_attributes::rebuild_player(&mut player, false);
        player
    }

    /// 实体位置，也就是脚底。
    ///
    /// Entity position, the feet.
    pub(crate) fn position(&self) -> Vec3 {
        self.position
    }

    /// 官方 `ServerPlayer.giveExperiencePoints`（0 是空操作，改动强制下一刻发包）。
    ///
    /// Official `ServerPlayer.giveExperiencePoints`: zero is a no-op; changes
    /// force a packet on the next tick.
    pub(crate) fn give_experience_points(&mut self, points: i32) {
        if points == 0 {
            return;
        }
        self.experience.give_points(points, &mut self.score);
        self.last_sent_exp = -1;
    }

    /// 官方 `ServerPlayer.giveExperienceLevels`。
    pub(crate) fn give_experience_levels(&mut self, levels: i32) {
        if levels == 0 {
            return;
        }
        self.experience.give_levels(levels);
        self.last_sent_exp = -1;
    }

    /// 官方 `ServerPlayer.setExperiencePoints`。
    pub(crate) fn set_experience_points(&mut self, points: i32) -> bool {
        let changed = self.experience.set_points(points);
        if changed {
            self.last_sent_exp = -1;
        }
        changed
    }

    /// 官方 `ServerPlayer.setExperienceLevels`。
    pub(crate) fn set_experience_levels(&mut self, level: i32) {
        if level == self.experience.level {
            return;
        }
        self.experience.set_level(level);
        self.last_sent_exp = -1;
    }

    /// 官方 `Player.onEnchantmentPerformed`：扣等级并从玩家随机源换新种子，
    /// 返回新种子（菜单的 `enchantmentSeed` DataSlot 要用）。
    ///
    /// Official `Player.onEnchantmentPerformed`: subtracts levels, draws a new
    /// seed from the player's random source, and returns it for the menu's
    /// `enchantmentSeed` DataSlot.
    pub(crate) fn on_enchantment_performed(&mut self, levels: i32) -> i32 {
        self.experience.level = self.experience.level.wrapping_sub(levels);
        if self.experience.level < 0 {
            self.experience.level = 0;
            self.experience.progress = 0.0;
            self.experience.total = 0;
        }
        self.enchantment_seed = self.random.next_int();
        self.last_sent_exp = -1;
        self.enchantment_seed
    }

    /// 改位置，网格跟着更新。广播靠网格找附近的玩家，绕过这里改位置会让玩家漏收包。
    ///
    /// Moves the player and updates the grid. Broadcasts find nearby players through the grid,
    /// so changing the position any other way would make the player miss packets.
    pub(crate) fn set_position(&mut self, grid: &mut PlayerGrid, position: Vec3) {
        self.position = position;
        grid.place(self.id, position);
    }

    /// 玩家所在的区块。
    pub(crate) fn chunk_pos(&self) -> ChunkPos {
        ChunkPos::containing(self.position.x, self.position.z)
    }

    /// 这名玩家实际的视距：客户端请求的值，但不超过服务端视距，这样区块不会落到客户端范围外。
    pub(crate) fn view_distance(&self, server_view_distance: i32) -> i32 {
        i32::from(self.client.view_distance).clamp(MIN_VIEW_DISTANCE, server_view_distance)
    }

    pub(crate) fn shows_hat(&self) -> bool {
        shows_hat(&self.client)
    }

    /// 四格盔甲里非空的比例，官方 `getArmorCoverPercentage()`。
    pub(crate) fn armor_cover(&self) -> f32 {
        let empty = mc_bamboo_protocol::item::ItemStack::empty();
        let equipment = self.equipment_stacks(&empty);
        PlayerHealth::armor_cover(&equipment)
    }

    /// 六个装备槽，顺序是主手、副手、靴子、护腿、胸甲、头盔。
    /// `fallback` 只在槽位表缺格时用，正常路径不会用到。
    pub(crate) fn equipment_stacks<'a>(&'a self, fallback: &'a ItemStack) -> [&'a ItemStack; 6] {
        let get = |slot| self.inventory.equipment(slot).unwrap_or(fallback);
        [
            get(EquipmentSlot::MainHand),
            get(EquipmentSlot::OffHand),
            get(EquipmentSlot::Feet),
            get(EquipmentSlot::Legs),
            get(EquipmentSlot::Chest),
            get(EquipmentSlot::Head),
        ]
    }

    /// 眼睛离脚底多高。
    pub(crate) fn eye_height(&self) -> f64 {
        if self.sync.is_crouching() {
            CROUCHING_EYE_HEIGHT
        } else {
            STANDING_EYE_HEIGHT
        }
    }

    /// 实体逻辑看到的玩家视图，对应官方 `Level.players()` 里的一个 `Player`。
    /// 这里不筛选旁观者或死亡状态：`getNearestPlayer` 的谓词、
    /// `TargetingConditions` 和 `ExperienceOrb.followNearbyPlayer` 各自按官方条件过滤。
    ///
    /// The player view seen by entity logic, one entry of `Level.players()`.
    /// Spectators and dead players are not filtered here: `getNearestPlayer`'s
    /// predicate, `TargetingConditions` and `ExperienceOrb.followNearbyPlayer`
    /// each apply the official conditions themselves.
    pub(crate) fn view(&self, id: PlayerId) -> PlayerView {
        let main = self.inventory.held(Hand::Main);
        let off = self.inventory.held(Hand::Off);
        PlayerView {
            uid: id.0,
            uuid: Some(*self.uuid.as_bytes()),
            position: self.position,
            delta: self.motion.delta,
            eye_height: self.eye_height() as f32,
            health: self.health.health,
            look_direction: Vec3::view_vector(self.yaw, self.pitch),
            gaze_disguise: self
                .inventory
                .equipment(EquipmentSlot::Head)
                .is_some_and(|stack| {
                    !stack.is_empty()
                        && mc_bamboo_blocks::state::item_is_tagged(
                            stack.item,
                            mc_bamboo_data::tags::item_tags::GAZE_DISGUISE_EQUIPMENT,
                        )
                }),
            // 隐身仍按默认假处理；`armor_cover` 按四格盔甲里非空的比例算。
            // Invisibility stays false; armor cover follows the worn slots.
            alive: !self.health.is_dead_or_dying(),
            spectator: self.game_mode == GameMode::Spectator,
            creative: self.game_mode == GameMode::Creative,
            game_mode: self.game_mode as i32,
            mining_fatigue: self
                .effects
                .get(mc_bamboo_data::registries::mob_effect::MINING_FATIGUE)
                .map(|instance| (instance.amplifier, instance.duration)),
            sneaking: self.sync.is_crouching(),
            sprinting: self.sync.is_sprinting(),
            swimming: self.motion.swimming,
            invisible: false,
            armor_cover: self.armor_cover(),
            main_hand: if main.is_empty() { 0 } else { main.item },
            off_hand: if off.is_empty() { 0 } else { off.item },
            team: None,
            yaw: self.yaw,
            pitch: self.pitch,
            combat: self.combat_memory.view(self.entity_ticks),
            effects: mc_bamboo_blocks::entity::ctx::TargetEffects::from_map(&self.effects),
        }
    }

    /// [`view`](Self::view) 加上按名字查到的队伍。
    pub(crate) fn view_in(
        &self,
        id: PlayerId,
        board: &crate::scoreboard::Scoreboard,
    ) -> PlayerView {
        PlayerView {
            team: board.team_index_of(&self.name),
            ..self.view(id)
        }
    }

    /// 碰撞盒，世界坐标。
    pub(crate) fn bounding_box(&self) -> Aabb {
        let height = if self.sync.is_crouching() {
            CROUCHING_HEIGHT
        } else {
            STANDING_HEIGHT
        };
        let Vec3 { x, y, z } = self.position;
        Aabb::new(
            x - HALF_WIDTH,
            y,
            z - HALF_WIDTH,
            x + HALF_WIDTH,
            y + height,
            z + HALF_WIDTH,
        )
    }
}

/// 按 `PlayerId` 升序列出本岛的玩家，和官方 `ServerLevel.players()` 的进服顺序一致：
/// 实体 ID 单调递增，进服时追加到表尾，离开时删掉。
///
/// Lists this island's players by ascending `PlayerId`, matching the join order
/// of `ServerLevel.players()`: entity ids grow monotonically, joins append and
/// leaves remove.
pub(crate) fn player_views(
    players: &std::collections::BTreeMap<PlayerId, Player>,
    board: &crate::scoreboard::Scoreboard,
) -> Vec<PlayerView> {
    players
        .iter()
        .map(|(id, player)| player.view_in(*id, board))
        .collect()
}

/// 客户端设置里帽子层是不是打开的。
pub(crate) fn shows_hat(client: &ClientInformation) -> bool {
    client.model_customisation & MODEL_PART_HAT != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player_requesting(view_distance: i8) -> Player {
        let client = ClientInformation {
            view_distance,
            ..ClientInformation::default()
        };
        Player::new(
            PlayerId(1),
            Uuid::nil(),
            "Kitten".to_owned(),
            GameMode::Creative,
            client,
            Vec3::default(),
        )
    }

    #[test]
    fn requested_view_distance_stays_between_two_and_the_server_value() {
        assert_eq!(player_requesting(12).view_distance(8), 8);
        assert_eq!(player_requesting(5).view_distance(8), 5);
        assert_eq!(player_requesting(2).view_distance(8), 2);
        assert_eq!(player_requesting(1).view_distance(8), 2);
        assert_eq!(player_requesting(0).view_distance(8), 2);
        assert_eq!(player_requesting(-5).view_distance(8), 2);
        assert_eq!(player_requesting(i8::MAX).view_distance(32), 32);
        assert_eq!(player_requesting(i8::MIN).view_distance(2), 2);
    }

    #[test]
    fn chunk_position_rounds_down() {
        let mut player = player_requesting(2);

        let mut grid = PlayerGrid::default();
        player.set_position(&mut grid, Vec3::new(-0.5, 500.0, 16.0));
        assert_eq!(player.chunk_pos(), ChunkPos::new(-1, 1));

        player.set_position(&mut grid, Vec3::new(3.0e7, 0.0, -3.0e7));
        assert_eq!(player.chunk_pos(), ChunkPos::new(1_875_000, -1_875_000));
    }

    #[test]
    fn hat_follows_the_model_customisation_bit() {
        let mut player = player_requesting(2);

        assert!(!player.shows_hat());
        player.client.model_customisation = 0x7f;
        assert!(player.shows_hat());
        player.client.model_customisation = 0x3f;
        assert!(!player.shows_hat());
    }
}
