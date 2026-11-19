//! 生物：官方 `LivingEntity` → `Mob` → `PathfinderMob` → `AgeableMob` → `Animal` → `Cow`。
//!
//! 这一轮只有牛。状态都在 [`Mob`] 里，按官方的类分组；一刻的流程在 [`living`]，
//! 移动和方块效果在 [`motion`]，目标在 [`goals`]，寻路在 [`path`] 和 [`navigation`]。
//!
//! tick 期间 [`Mob`] 被从 `Entity.kind` 里暂时取出来，和 `Entity` 本身分开借用；
//! 实体的尺寸因此存在 [`Entity::dimensions`] 上，不从 `kind` 推。
//!
//! Mobs: vanilla `LivingEntity` through `Cow`. Only cows this round. During a tick the
//! [`Mob`] is moved out of `Entity.kind` so it can be borrowed apart from the entity;
//! sizes therefore live on [`Entity::dimensions`].

pub mod attributes;
pub mod control;
pub mod damage;
pub mod goals;
pub mod interact;
pub mod living;
pub mod motion;
pub mod navigation;
pub mod nbt;
pub mod path;
pub mod spawn;
pub(crate) mod timing;

use mc_bamboo_core::{BlockPos, Vec3};

use self::{
    attributes::Attributes,
    control::{BodyRotationControl, JumpControl, LookControl, MoveControl},
    goals::GoalSelector,
    navigation::Navigation,
    path::Malus,
};
use super::{EntityUid, JavaRandom};

/// 生物种类。现在只有牛。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobType {
    Cow,
}

impl MobType {
    /// 注册表 ID。
    pub const fn entity_type(self) -> i32 {
        match self {
            Self::Cow => mc_bamboo_data::registries::entity_type::COW,
        }
    }

    /// 成年和幼年的（宽，高，眼高）。
    pub const fn dimensions(self, baby: bool) -> (f32, f32, f32) {
        match (self, baby) {
            (Self::Cow, false) => (0.9, 1.4, 1.3),
            // `Cow.BABY_DIMENSIONS`：`scalable(0.45F, 0.7F).withEyeHeight(0.69F)`。
            (Self::Cow, true) => (0.45, 0.7, 0.69),
        }
    }
}

/// 牛的花纹，`cow_variant` 注册表。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CowVariant {
    #[default]
    Temperate,
    Warm,
    Cold,
}

impl CowVariant {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Temperate => "minecraft:temperate",
            Self::Warm => "minecraft:warm",
            Self::Cold => "minecraft:cold",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Temperate, Self::Warm, Self::Cold]
            .into_iter()
            .find(|variant| variant.name() == name)
    }

    /// 注册表里的下标（`cow_variant` 按名字排序：cold、temperate、warm），同步元数据用。
    pub const fn registry_id(self) -> i32 {
        match self {
            Self::Cold => 0,
            Self::Temperate => 1,
            Self::Warm => 2,
        }
    }
}

/// 牛的叫声，`cow_sound_variant` 注册表。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CowSoundVariant {
    #[default]
    Classic,
    Moody,
}

impl CowSoundVariant {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Classic => "minecraft:classic",
            Self::Moody => "minecraft:moody",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Classic, Self::Moody]
            .into_iter()
            .find(|variant| variant.name() == name)
    }

    /// 注册表里的下标（`cow_sound_variant` 按名字排序），同步元数据用。
    pub const fn registry_id(self) -> i32 {
        match self {
            Self::Classic => 0,
            Self::Moody => 1,
        }
    }

    /// `CowSoundVariant.ambientSound`。
    pub const fn ambient(self) -> &'static str {
        match self {
            Self::Classic => "minecraft:entity.cow.ambient",
            Self::Moody => "minecraft:entity.cow_moody.ambient",
        }
    }

    /// `CowSoundVariant.hurtSound`。
    pub const fn hurt(self) -> &'static str {
        match self {
            Self::Classic => "minecraft:entity.cow.hurt",
            Self::Moody => "minecraft:entity.cow_moody.hurt",
        }
    }

    /// `CowSoundVariant.deathSound`。
    pub const fn death(self) -> &'static str {
        match self {
            Self::Classic => "minecraft:entity.cow.death",
            Self::Moody => "minecraft:entity.cow_moody.death",
        }
    }

    /// `CowSoundVariant.stepSound`。
    pub const fn step(self) -> &'static str {
        match self {
            Self::Classic => "minecraft:entity.cow.step",
            Self::Moody => "minecraft:entity.cow_moody.step",
        }
    }
}

/// 一种流体的跟踪器，`EntityFluidInteraction.Tracker`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FluidTracker {
    /// 流体注册表 ID（静止和流动的水是两种）。
    pub fluid: i32,
    pub height: f64,
    pub eyes_inside: bool,
}

/// 伤害的种类，`minecraft:damage_type` 注册表里的一项。
///
/// A kind of damage: one entry of the `minecraft:damage_type` registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageKind {
    Generic,
    GenericKill,
    InWall,
    Cramming,
    Drown,
    OnFire,
    InFire,
    Lava,
    HotFloor,
    Cactus,
    SweetBerryBush,
    Fall,
    Freeze,
    OutOfWorld,
    PlayerAttack,
    MobAttack,
    MobAttackNoAggro,
    Arrow,
    Starve,
    Thorns,
    Magic,
    Explosion,
}

impl DamageKind {
    /// 注册名。
    pub const fn name(self) -> &'static str {
        match self {
            Self::Generic => "minecraft:generic",
            Self::GenericKill => "minecraft:generic_kill",
            Self::InWall => "minecraft:in_wall",
            Self::Cramming => "minecraft:cramming",
            Self::Drown => "minecraft:drown",
            Self::OnFire => "minecraft:on_fire",
            Self::InFire => "minecraft:in_fire",
            Self::Lava => "minecraft:lava",
            Self::HotFloor => "minecraft:hot_floor",
            Self::Cactus => "minecraft:cactus",
            Self::SweetBerryBush => "minecraft:sweet_berry_bush",
            Self::Fall => "minecraft:fall",
            Self::Freeze => "minecraft:freeze",
            Self::OutOfWorld => "minecraft:out_of_world",
            Self::PlayerAttack => "minecraft:player_attack",
            Self::MobAttack => "minecraft:mob_attack",
            Self::MobAttackNoAggro => "minecraft:mob_attack_no_aggro",
            Self::Arrow => "minecraft:arrow",
            Self::Starve => "minecraft:starve",
            Self::Thorns => "minecraft:thorns",
            Self::Magic => "minecraft:magic",
            Self::Explosion => "minecraft:explosion",
        }
    }

    /// 官方 `DamageType.msgId()`，数据文件里的 `message_id`：死亡消息
    /// `death.attack.<msgId>` 的后缀，和注册名不是一回事。
    ///
    /// Official `DamageType.msgId()` from the data file: the suffix of
    /// `death.attack.<msgId>`, which is not the registry name.
    pub const fn message_id(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::GenericKill => "genericKill",
            Self::InWall => "inWall",
            Self::Cramming => "cramming",
            Self::Drown => "drown",
            Self::OnFire => "onFire",
            Self::InFire => "inFire",
            Self::Lava => "lava",
            Self::HotFloor => "hotFloor",
            Self::Cactus => "cactus",
            Self::SweetBerryBush => "sweetBerryBush",
            Self::Fall => "fall",
            Self::Freeze => "freeze",
            Self::OutOfWorld => "outOfWorld",
            Self::PlayerAttack => "player",
            Self::MobAttack | Self::MobAttackNoAggro => "mob",
            Self::Arrow => "arrow",
            Self::Starve => "starve",
            Self::Thorns => "thorns",
            Self::Magic => "magic",
            Self::Explosion => "explosion",
        }
    }

    /// 按官方短名找一种伤害。未知名字返回 `None`，调用方按解析失败处理。
    ///
    /// Finds a kind by its short official name. Unknown names return `None`.
    pub fn by_name(name: &str) -> Option<Self> {
        let all: [DamageKind; 22] = [
            Self::Generic,
            Self::GenericKill,
            Self::InWall,
            Self::Cramming,
            Self::Drown,
            Self::OnFire,
            Self::InFire,
            Self::Lava,
            Self::HotFloor,
            Self::Cactus,
            Self::SweetBerryBush,
            Self::Fall,
            Self::Freeze,
            Self::OutOfWorld,
            Self::PlayerAttack,
            Self::MobAttack,
            Self::MobAttackNoAggro,
            Self::Arrow,
            Self::Starve,
            Self::Thorns,
            Self::Magic,
            Self::Explosion,
        ];
        all.into_iter()
            .find(|kind| kind.name().strip_prefix("minecraft:") == Some(name))
    }

    /// `#no_knockback`。
    pub const fn no_knockback(self) -> bool {
        !matches!(
            self,
            Self::PlayerAttack
                | Self::MobAttack
                | Self::MobAttackNoAggro
                | Self::Arrow
                | Self::Thorns
        )
    }

    /// `#bypasses_armor`。
    pub const fn bypasses_armor(self) -> bool {
        matches!(
            self,
            Self::OnFire
                | Self::InWall
                | Self::Cramming
                | Self::Drown
                | Self::Generic
                | Self::Fall
                | Self::Freeze
                | Self::OutOfWorld
                | Self::GenericKill
        )
    }

    /// `#bypasses_effects`：抗性效果和保护附魔都不生效。
    pub const fn bypasses_effects(self) -> bool {
        matches!(self, Self::OutOfWorld | Self::GenericKill)
    }

    /// `#bypasses_enchantments`：保护附魔不生效。
    pub const fn bypasses_enchantments(self) -> bool {
        matches!(self, Self::OutOfWorld | Self::GenericKill)
    }

    /// `#is_fire`。
    pub const fn is_fire(self) -> bool {
        matches!(
            self,
            Self::InFire | Self::OnFire | Self::Lava | Self::HotFloor
        )
    }

    /// `#no_impact`。
    pub const fn no_impact(self) -> bool {
        matches!(self, Self::Drown)
    }

    /// `#bypasses_invulnerability`。
    pub const fn bypasses_invulnerability(self) -> bool {
        matches!(self, Self::OutOfWorld | Self::GenericKill)
    }

    /// `#bypasses_cooldown`：26.3 的数据包里这个标签是空的。
    pub const fn bypasses_cooldown(self) -> bool {
        let _ = self;
        false
    }

    /// `DamageSource.scalesWithDifficulty()`：只有 `always` 缩放，以及
    /// `when_caused_by_living_non_player` 且造成者是非玩家生物时缩放。
    /// 玩家的 `causing_entity` 类型由 [`DamageSource::scale_when_non_player`] 给出。
    ///
    /// `DamageSource.scalesWithDifficulty()`: only `always`, or
    /// `when_caused_by_living_non_player` with a non-player living cause.
    pub const fn scales_with_difficulty(self) -> bool {
        matches!(self, Self::Explosion)
    }

    /// `DamageType.exhaustion`：受伤后累加的饥饿消耗。
    pub const fn exhaustion(self) -> f32 {
        const TENTH: f32 = 0.1;
        match self {
            Self::PlayerAttack
            | Self::MobAttack
            | Self::MobAttackNoAggro
            | Self::Arrow
            | Self::Thorns
            | Self::Explosion
            | Self::Cactus
            | Self::InFire
            | Self::Lava
            | Self::HotFloor
            | Self::SweetBerryBush => TENTH,
            _ => 0.0,
        }
    }

    /// `#panic_causes`。
    pub const fn panic_causes(self) -> bool {
        matches!(
            self,
            Self::PlayerAttack
                | Self::MobAttack
                | Self::MobAttackNoAggro
                | Self::Arrow
                | Self::Thorns
                | Self::Cactus
                | Self::Freeze
                | Self::HotFloor
                | Self::InFire
                | Self::Lava
                | Self::OnFire
        )
    }
}

/// 一次伤害的来源，`DamageSource`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamageSource {
    pub kind: DamageKind,
    /// `getEntity()`：造成伤害的实体，也是 `DamageEvent` 的 `source_cause`。
    pub attacker: Option<EntityUid>,
    /// `getDirectEntity()`：直接打到目标的实体，是 `DamageEvent` 的 `source_direct`。
    pub direct: Option<EntityUid>,
    /// 攻击者是不是玩家；玩家的伤害会让牛记住它，死时掉经验。
    pub attacker_is_player: bool,
    /// 攻击者是不是创造模式的玩家（`isCreativePlayer`）。
    pub creative_player: bool,
    /// `getSourcePosition()`：显式位置，或者回退到攻击者位置。击退方向读它。
    pub position: Option<Vec3>,
    /// `sourcePositionRaw()`：只有 `at <location>` 才有值，`DamageEvent` 读它。
    pub raw_position: Option<Vec3>,
    /// 冲刺攻击时玩家的朝向，`causeExtraKnockback` 的方向用它。
    pub sprint_yaw: Option<f32>,
    /// 攻击者的 UUID，存进 `last_hurt_by_*`。
    pub attacker_uuid: Option<[u8; 16]>,
    /// 造成伤害的是非玩家生物：`when_caused_by_living_non_player` 缩放要读它。
    pub scale_when_non_player: bool,
    /// 主手武器带来的附魔（这一期只读破甲之外的效果，见 `damage::protection`）。
    /// 空表示没有武器。
    pub weapon: Option<u16>,
}

impl DamageSource {
    /// 没有攻击者的环境伤害。
    pub const fn environment(kind: DamageKind) -> Self {
        Self {
            kind,
            attacker: None,
            direct: None,
            attacker_is_player: false,
            creative_player: false,
            position: None,
            raw_position: None,
            sprint_yaw: None,
            attacker_uuid: None,
            scale_when_non_player: false,
            weapon: None,
        }
    }

    /// 带上攻击者的 UUID。
    pub const fn with_attacker_uuid(mut self, uuid: [u8; 16]) -> Self {
        self.attacker_uuid = Some(uuid);
        self
    }

    /// 覆盖来源位置（`/damage ... at <location>`）。
    pub const fn with_position(mut self, position: Vec3) -> Self {
        self.position = Some(position);
        self.raw_position = Some(position);
        self
    }

    /// 记录造成伤害的实体种类，供难度缩放判断。
    pub const fn with_non_player_cause(mut self, non_player: bool) -> Self {
        self.scale_when_non_player = non_player;
        self
    }

    /// 记录主手武器的物品 ID（没有武器时是 `None`）。
    pub const fn with_weapon(mut self, item: u16) -> Self {
        self.weapon = Some(item);
        self
    }

    /// `damageSources().playerAttack(player)`。
    pub const fn player(uid: EntityUid, position: Vec3, creative: bool) -> Self {
        Self {
            kind: DamageKind::PlayerAttack,
            attacker: Some(uid),
            direct: Some(uid),
            attacker_is_player: true,
            creative_player: creative,
            position: Some(position),
            raw_position: None,
            sprint_yaw: None,
            attacker_uuid: None,
            scale_when_non_player: false,
            weapon: None,
        }
    }

    /// `DamageSource.scalesWithDifficulty()`：`always` 恒缩放，
    /// `when_caused_by_living_non_player` 只在造成者是非玩家生物时缩放。
    pub const fn scales_with_difficulty(self) -> bool {
        self.kind.scales_with_difficulty() || self.scale_when_non_player
    }
}

/// 一个生物的全部状态。
#[derive(Debug, Clone, PartialEq)]
pub struct Mob {
    pub mob_type: MobType,
    pub uuid: [u8; 16],
    // ---- Entity 里普通实体没有的部分 ----
    pub x_rot: f32,
    pub y_rot_o: f32,
    pub x_rot_o: f32,
    pub eye_height: f32,
    /// `Entity.invulnerableTime`：临时无敌，不是受伤冷却。
    pub invulnerable_time: i32,
    pub minor_horizontal_collision: bool,
    pub fluid_trackers: Vec<FluidTracker>,
    pub was_eye_in_water: bool,
    pub in_powder_snow: bool,
    pub was_in_powder_snow: bool,
    pub ticks_frozen: i32,
    pub air_supply: i32,
    pub move_dist: f32,
    pub fly_dist: f32,
    pub next_step: f32,
    pub crystal_sound_intensity: f32,
    pub last_crystal_sound_tick: i32,
    pub silent: bool,
    pub no_gravity: bool,
    pub invulnerable: bool,
    /// `CustomName`：文本组件的 NBT 形式（字符串或复合标签），同步时原样转成组件。
    ///
    /// `CustomName` in its NBT form (a string or a compound), sent as is.
    pub custom_name: Option<mc_bamboo_nbt::Tag>,
    pub custom_name_visible: bool,
    pub glowing: bool,
    pub portal_cooldown: i32,
    pub tags: Vec<String>,
    /// 存档里官方有、这里不建模的键（效果、装备、拴绳、`data` 等），读进来原样写回。
    ///
    /// Vanilla keys that are not modelled here (effects, equipment, leash, `data`),
    /// kept verbatim from disk and written back.
    pub passthrough: Vec<(String, mc_bamboo_nbt::Tag)>,
    // ---- LivingEntity ----
    pub attributes: Attributes,
    pub health: f32,
    pub dead: bool,
    pub hurt_time: i32,
    pub hurt_duration: i32,
    pub death_time: i32,
    /// `damageCooldownTime`：受伤后 20 刻内的减伤冷却。
    pub damage_cooldown: i32,
    pub last_hurt: f32,
    pub last_damage_source: Option<DamageSource>,
    /// 绝对时刻（岛的时钟），合并时要平移。
    pub last_damage_stamp: i64,
    pub last_hurt_by_mob: Option<EntityUid>,
    /// `lastHurtByMob` 的 UUID（`EntityReference`），存档用。读盘后只有 UUID、没有 uid。
    ///
    /// The UUID of `lastHurtByMob`, for saving. After a load only the UUID is known.
    pub last_hurt_by_mob_ref: Option<[u8; 16]>,
    pub last_hurt_by_mob_timestamp: i32,
    pub last_hurt_by_player: Option<EntityUid>,
    /// `lastHurtByPlayer` 的 UUID，存档用。
    pub last_hurt_by_player_ref: Option<[u8; 16]>,
    pub last_hurt_by_player_memory_time: i32,
    /// `currentImpulseContextResetGraceTime`。牛不会被风弹推，只为存档往返。
    pub impulse_grace_time: i32,
    pub no_action_time: i32,
    pub speed: f32,
    pub xxa: f32,
    pub yya: f32,
    pub zza: f32,
    pub jumping: bool,
    pub no_jump_delay: i32,
    pub absorption: f32,
    pub y_head_rot: f32,
    pub y_head_rot_o: f32,
    pub y_body_rot: f32,
    pub y_body_rot_o: f32,
    pub last_pos: Option<BlockPos>,
    pub last_climbable_pos: Option<BlockPos>,
    pub fall_flying: bool,
    // ---- Mob ----
    pub ambient_sound_time: i32,
    pub look: LookControl,
    pub mover: MoveControl,
    pub jump: JumpControl,
    pub body: BodyRotationControl,
    pub navigation: Navigation,
    pub goals: GoalSelector,
    /// `Sensing` 的视线缓存，每刻清空。
    pub seen: Vec<(EntityUid, bool)>,
    pub malus: Malus,
    pub persistence_required: bool,
    pub left_handed: bool,
    pub no_ai: bool,
    pub can_pick_up_loot: bool,
    pub home: Option<(BlockPos, i32)>,
    pub loot_table_seed: i64,
    // ---- AgeableMob / Animal / Cow ----
    pub age: i32,
    pub forced_age: i32,
    pub forced_age_timer: i32,
    pub age_lock_particle_timer: i32,
    pub age_locked: bool,
    pub in_love: i32,
    pub love_cause: Option<[u8; 16]>,
    pub variant: CowVariant,
    pub sound_variant: CowSoundVariant,
    /// 这一刻要广播的实体事件（受伤、死亡），游戏层同步时取走。
    pub events: Vec<MobEvent>,
    /// `hurtMarked`：速度要强制同步一次。
    pub hurt_marked: bool,
    /// 这一刻 `BreedGoal` 决定生小牛；`living::tick` 在把 `kind` 放回实体之后落地。
    pub pending_breed: Option<EntityUid>,
}

/// 要发给客户端的一次生物事件。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MobEvent {
    /// `broadcastDamageEvent`：受了一次完整的伤害。
    Damage {
        kind: DamageKind,
        attacker: Option<EntityUid>,
    },
    /// `broadcastEntityEvent(this, 3)`：死亡。
    Death,
    /// `broadcastEntityEvent(this, 60)`：死亡动画结束，冒烟。
    Poof,
    /// `broadcastEntityEvent(this, 18)`：开始求爱 / 繁殖时的爱心。
    Hearts,
    /// `Player.playSound(COW_MILK, 1, 1)`：音源是玩家，不是中立生物。
    MilkSound { position: Vec3 },
    /// `Entity.playSound`：在实体当时的位置放一个音效（已经排除了静音的情况）。
    ///
    /// `Entity.playSound` at the entity's position at the time (silent mobs excluded).
    Sound {
        sound: &'static str,
        volume: f32,
        pitch: f32,
        position: Vec3,
    },
}

impl Mob {
    /// `EntityType.create`：构造函数链。随机数（UUID 两个 `nextLong`、`yRot` 一个
    /// `nextFloat`）由调用方在实体拿到 ID 之后取，见 [`super::Entity::cow`]。
    pub fn cow() -> Self {
        let attributes = Attributes::cow();
        let follow_range = attributes.base(mc_bamboo_data::registries::attribute::FOLLOW_RANGE);
        let mut navigation = Navigation::new(follow_range);
        let goals = GoalSelector::cow(&mut navigation);
        let health = attributes.value(mc_bamboo_data::registries::attribute::MAX_HEALTH) as f32;
        Self {
            mob_type: MobType::Cow,
            uuid: [0; 16],
            x_rot: 0.0,
            y_rot_o: 0.0,
            x_rot_o: 0.0,
            eye_height: MobType::Cow.dimensions(false).2,
            invulnerable_time: 0,
            minor_horizontal_collision: false,
            fluid_trackers: Vec::new(),
            was_eye_in_water: false,
            in_powder_snow: false,
            was_in_powder_snow: false,
            ticks_frozen: 0,
            air_supply: 300,
            move_dist: 0.0,
            fly_dist: 0.0,
            next_step: 1.0,
            crystal_sound_intensity: 0.0,
            last_crystal_sound_tick: 0,
            silent: false,
            no_gravity: false,
            invulnerable: false,
            custom_name: None,
            custom_name_visible: false,
            glowing: false,
            portal_cooldown: 0,
            tags: Vec::new(),
            passthrough: Vec::new(),
            attributes,
            health,
            dead: false,
            hurt_time: 0,
            hurt_duration: 0,
            death_time: 0,
            damage_cooldown: 0,
            last_hurt: 0.0,
            last_damage_source: None,
            last_damage_stamp: 0,
            last_hurt_by_mob: None,
            last_hurt_by_mob_ref: None,
            last_hurt_by_mob_timestamp: 0,
            last_hurt_by_player: None,
            last_hurt_by_player_ref: None,
            last_hurt_by_player_memory_time: 0,
            impulse_grace_time: 0,
            no_action_time: 0,
            speed: 0.0,
            xxa: 0.0,
            yya: 0.0,
            zza: 0.0,
            jumping: false,
            no_jump_delay: 0,
            absorption: 0.0,
            y_head_rot: 0.0,
            y_head_rot_o: 0.0,
            y_body_rot: 0.0,
            y_body_rot_o: 0.0,
            last_pos: None,
            last_climbable_pos: None,
            fall_flying: false,
            ambient_sound_time: 0,
            look: LookControl::default(),
            mover: MoveControl::default(),
            jump: JumpControl::default(),
            body: BodyRotationControl::default(),
            navigation,
            goals,
            seen: Vec::new(),
            malus: Malus::animal(),
            persistence_required: false,
            left_handed: false,
            no_ai: false,
            can_pick_up_loot: false,
            home: None,
            loot_table_seed: 0,
            age: 0,
            forced_age: 0,
            forced_age_timer: 0,
            age_lock_particle_timer: 0,
            age_locked: false,
            in_love: 0,
            love_cause: None,
            variant: CowVariant::Temperate,
            sound_variant: CowSoundVariant::Classic,
            events: Vec::new(),
            hurt_marked: false,
            pending_breed: None,
        }
    }

    pub fn is_baby(&self) -> bool {
        self.age < 0
    }

    /// （宽，高）。
    pub fn dimensions(&self) -> (f32, f32) {
        let (width, height, _) = self.mob_type.dimensions(self.is_baby());
        (width, height)
    }

    /// `isDeadOrDying`。
    pub fn is_dead_or_dying(&self) -> bool {
        self.health <= 0.0 || self.dead
    }

    /// `PathfinderMob.isPanicking`：正在跑的目标里有恐慌。
    pub fn is_panicking(&self) -> bool {
        self.goals.is_panicking()
    }

    /// 岛合并时把绝对时刻平移 `delta`。
    pub fn rebase(&mut self, delta: i64) {
        self.last_damage_stamp = self.last_damage_stamp.saturating_add(delta);
        self.navigation.rebase(delta);
    }
}

/// 别的实体和追踪同步能读到的生物数据。
#[derive(Debug, Clone, PartialEq)]
pub struct MobPublic {
    pub mob_type: MobType,
    pub uuid: [u8; 16],
    /// 同步 `update_attributes` 用的属性表（只含已经实例化的条目）。
    ///
    /// The attribute table for `update_attributes` (instantiated entries only).
    pub attributes: Attributes,
    pub health: f32,
    pub baby: bool,
    /// `AgeableMob.AGE_LOCKED` 元数据。
    pub age_locked: bool,
    pub x_rot: f32,
    pub y_head_rot: f32,
    pub y_body_rot: f32,
    pub in_love: i32,
    pub age: i32,
    pub panicking: bool,
    pub no_ai: bool,
    pub silent: bool,
    pub no_gravity: bool,
    pub variant: CowVariant,
    pub sound_variant: CowSoundVariant,
    pub on_fire: bool,
    pub ticks_frozen: i32,
    pub air_supply: i32,
    pub custom_name: Option<mc_bamboo_nbt::Tag>,
    pub custom_name_visible: bool,
    pub glowing: bool,
    pub left_handed: bool,
    pub dying: bool,
}

impl MobPublic {
    pub fn of(mob: &Mob, remaining_fire_ticks: i32) -> Self {
        Self {
            mob_type: mob.mob_type,
            uuid: mob.uuid,
            attributes: mob.attributes.clone(),
            health: mob.health,
            baby: mob.is_baby(),
            age_locked: mob.age_locked,
            x_rot: mob.x_rot,
            y_head_rot: mob.y_head_rot,
            y_body_rot: mob.y_body_rot,
            in_love: mob.in_love,
            age: mob.age,
            panicking: mob.is_panicking(),
            no_ai: mob.no_ai,
            silent: mob.silent,
            no_gravity: mob.no_gravity,
            variant: mob.variant,
            sound_variant: mob.sound_variant,
            on_fire: remaining_fire_ticks > 0,
            ticks_frozen: mob.ticks_frozen,
            air_supply: mob.air_supply,
            custom_name: mob.custom_name.clone(),
            custom_name_visible: mob.custom_name_visible,
            glowing: mob.glowing,
            left_handed: mob.left_handed,
            dying: mob.is_dead_or_dying(),
        }
    }

    pub fn dimensions(&self) -> (f32, f32) {
        let (width, height, _) = self.mob_type.dimensions(self.baby);
        (width, height)
    }

    pub fn is_alive(&self) -> bool {
        self.health > 0.0
    }
}

/// `Mth.createInsecureUUID(random)`：两个 `nextLong`，按版本 4 打标。
pub fn insecure_uuid(random: &mut JavaRandom) -> [u8; 16] {
    let most = (random.next_long() & -61441) | 16384;
    let least = (random.next_long() & 4_611_686_018_427_387_903) | i64::MIN;
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&most.to_be_bytes());
    bytes[8..].copy_from_slice(&least.to_be_bytes());
    bytes
}
