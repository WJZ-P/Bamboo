//! 生物：官方 `LivingEntity` → `Mob`，再分出 `Animal` → `Cow` 和 `Monster` → `Zombie`。
//!
//! 所有种类共用一个 [`Mob`] 状态，按官方的类分组；每种生物的事实集中在
//! [`MobType`] 的一张表里，牛专属的字段只在 `Cow` 分支读写。一刻的流程在 [`living`]，
//! 移动和方块效果在 [`motion`]，目标在 [`goals`]，寻路在 [`path`] 和 [`navigation`]。
//!
//! tick 期间 [`Mob`] 被从 `Entity.kind` 里暂时取出来，和 `Entity` 本身分开借用；
//! 实体的尺寸因此存在 [`Entity::dimensions`] 上，不从 `kind` 推。
//!
//! Mobs: vanilla `LivingEntity` through `Cow` and `Zombie`. All types share one
//! [`Mob`] state with a [`MobType`] fact table. During a tick the [`Mob`] is moved
//! out of `Entity.kind` so it can be borrowed apart from the entity; sizes
//! therefore live on [`Entity::dimensions`].

pub mod attributes;
pub mod control;
pub mod damage;
pub mod effects;
pub mod equine;
pub mod equipment;
pub mod goals;
pub mod interact;
pub mod living;
pub mod motion;
pub mod navigation;
pub mod nbt;
pub mod path;
pub mod skeleton;
mod skeleton_drops;
pub mod spawn;
pub(crate) mod timing;

use mc_bamboo_core::{BlockPos, Vec3};

use self::{
    attributes::Attributes,
    control::{BodyRotationControl, JumpControl, LookControl, MoveControl},
    effects::EffectMap,
    goals::GoalSelector,
    navigation::Navigation,
    path::Malus,
};
use super::{EntityUid, JavaRandom};

/// 生物种类。牛走 `Animal` 一支，僵尸和尸壳走 `Monster` → `Zombie` 一支。
///
/// Mob kinds. Cows use the `Animal` branch; zombies and husks the
/// `Monster` → `Zombie` branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MobType {
    #[default]
    Cow,
    Zombie,
    Husk,
    Skeleton,
    Horse,
    Donkey,
    Mule,
    Llama,
}

impl MobType {
    /// 注册表 ID。
    pub const fn entity_type(self) -> i32 {
        use mc_bamboo_data::registries::entity_type as ids;
        match self {
            Self::Cow => ids::COW,
            Self::Zombie => ids::ZOMBIE,
            Self::Husk => ids::HUSK,
            Self::Skeleton => ids::SKELETON,
            Self::Horse => ids::HORSE,
            Self::Donkey => ids::DONKEY,
            Self::Mule => ids::MULE,
            Self::Llama => ids::LLAMA,
        }
    }

    /// 存档 / 命令里的官方名字。
    pub const fn name(self) -> &'static str {
        match self {
            Self::Cow => "minecraft:cow",
            Self::Zombie => "minecraft:zombie",
            Self::Husk => "minecraft:husk",
            Self::Skeleton => "minecraft:skeleton",
            Self::Horse => "minecraft:horse",
            Self::Donkey => "minecraft:donkey",
            Self::Mule => "minecraft:mule",
            Self::Llama => "minecraft:llama",
        }
    }

    pub fn by_name(name: &str) -> Option<Self> {
        [
            Self::Cow,
            Self::Zombie,
            Self::Husk,
            Self::Skeleton,
            Self::Horse,
            Self::Donkey,
            Self::Mule,
            Self::Llama,
        ]
        .into_iter()
        .find(|kind| kind.name() == name)
    }

    pub const fn is_equine(self) -> bool {
        matches!(self, Self::Horse | Self::Donkey | Self::Mule | Self::Llama)
    }

    /// 成年和幼年的（宽，高，眼高）。
    pub const fn dimensions(self, baby: bool) -> (f32, f32, f32) {
        match (self, baby) {
            (Self::Skeleton, _) => {
                let facts = &mc_bamboo_data::entity_types::FACTS[self.entity_type() as usize];
                (
                    facts.width,
                    facts.height,
                    mc_bamboo_data::entity_types::SKELETON_EYE_HEIGHT,
                )
            }
            (Self::Horse | Self::Donkey | Self::Mule | Self::Llama, _) => {
                equine::dimensions(self, baby)
            }
            (Self::Cow, false) => (0.9, 1.4, 1.3),
            // `Cow.BABY_DIMENSIONS`：`scalable(0.45F, 0.7F).withEyeHeight(0.69F)`。
            (Self::Cow, true) => (0.45, 0.7, 0.69),
            (Self::Zombie | Self::Husk, false) => (0.6, 1.95, 1.74),
            // `Zombie.BABY_DIMENSIONS`：`scalable(0.49F, 0.98F).withEyeHeight(0.775F)`。
            (Self::Zombie | Self::Husk, true) => (0.49, 0.98, 0.775),
        }
    }

    /// 是不是 `Monster` 一支（决定消失规则、经验和战利品规则）。
    pub const fn is_monster(self) -> bool {
        matches!(self, Self::Zombie | Self::Husk | Self::Skeleton)
    }

    /// `EntityType.isAllowedInPeaceful()`：牛为真，敌对生物为假。
    pub const fn allowed_in_peaceful(self) -> bool {
        !self.is_monster()
    }

    /// `EntityType.getCategory()` 的消失距离。牛是 `CREATURE`（32），
    /// 僵尸是 `MONSTER`（128 / 32）。
    pub const fn despawn_distance(self) -> i32 {
        if self.is_monster() { 128 } else { 32 }
    }

    /// `Mob.removeWhenFarAway`：牛为假，`Monster` 为真。
    pub const fn removes_when_far_away(self) -> bool {
        self.is_monster()
    }

    /// `Mob.xpReward`：牛由 `Animal.getBaseExperienceReward` 抽，僵尸固定 5。
    pub const fn xp_reward(self) -> i32 {
        if self.is_monster() { 5 } else { 0 }
    }

    /// `LivingEntity.dropAllDeathLoot` 用的战利品表。
    pub const fn loot_table(self) -> &'static str {
        match self {
            Self::Cow => "minecraft:entities/cow",
            Self::Zombie => "minecraft:entities/zombie",
            Self::Husk => "minecraft:entities/husk",
            Self::Skeleton => "minecraft:entities/skeleton",
            Self::Horse => "minecraft:entities/horse",
            Self::Donkey => "minecraft:entities/donkey",
            Self::Mule => "minecraft:entities/mule",
            Self::Llama => "minecraft:entities/llama",
        }
    }

    /// `EntityTypeTags.BURN_IN_DAYLIGHT` 加 `Zombie.isSunSensitive()`：
    /// 尸壳不烧，僵尸烧。
    pub const fn burns_in_daylight(self) -> bool {
        matches!(self, Self::Zombie | Self::Skeleton)
    }

    /// `Mob.getAmbientSoundInterval()`：牛覆写成 120，僵尸用缺省 80。
    pub const fn ambient_sound_interval(self) -> i32 {
        if self.is_equine() {
            400
        } else if self.is_monster() {
            80
        } else {
            120
        }
    }

    /// `Mob.getSoundVolume()`。牛 0.4，僵尸 1.0（`Monster` 用 `getSoundVolume` 缺省）。
    pub const fn sound_volume(self) -> f32 {
        if self.is_equine() {
            0.8
        } else if self.is_monster() {
            1.0
        } else {
            0.4
        }
    }

    /// 四种音效按官方类分派，僵尸和尸壳各有自己的事件名。
    pub const fn ambient_sound(self) -> Option<&'static str> {
        match self {
            Self::Cow => None,
            Self::Zombie => Some("minecraft:entity.zombie.ambient"),
            Self::Husk => Some("minecraft:entity.husk.ambient"),
            Self::Skeleton => Some("minecraft:entity.skeleton.ambient"),
            Self::Horse => Some("minecraft:entity.horse.ambient"),
            Self::Donkey => Some("minecraft:entity.donkey.ambient"),
            Self::Mule => Some("minecraft:entity.mule.ambient"),
            Self::Llama => Some("minecraft:entity.llama.ambient"),
        }
    }

    pub const fn hurt_sound(self) -> Option<&'static str> {
        match self {
            Self::Cow => None,
            Self::Zombie => Some("minecraft:entity.zombie.hurt"),
            Self::Husk => Some("minecraft:entity.husk.hurt"),
            Self::Skeleton => Some("minecraft:entity.skeleton.hurt"),
            Self::Horse => Some("minecraft:entity.horse.hurt"),
            Self::Donkey => Some("minecraft:entity.donkey.hurt"),
            Self::Mule => Some("minecraft:entity.mule.hurt"),
            Self::Llama => Some("minecraft:entity.llama.hurt"),
        }
    }

    pub const fn death_sound(self) -> Option<&'static str> {
        match self {
            Self::Cow => None,
            Self::Zombie => Some("minecraft:entity.zombie.death"),
            Self::Husk => Some("minecraft:entity.husk.death"),
            Self::Skeleton => Some("minecraft:entity.skeleton.death"),
            Self::Horse => Some("minecraft:entity.horse.death"),
            Self::Donkey => Some("minecraft:entity.donkey.death"),
            Self::Mule => Some("minecraft:entity.mule.death"),
            Self::Llama => Some("minecraft:entity.llama.death"),
        }
    }

    pub const fn step_sound(self) -> Option<&'static str> {
        match self {
            Self::Cow => None,
            Self::Zombie => Some("minecraft:entity.zombie.step"),
            Self::Husk => Some("minecraft:entity.husk.step"),
            Self::Skeleton => Some("minecraft:entity.skeleton.step"),
            Self::Horse => Some("minecraft:entity.horse.step"),
            Self::Donkey => Some("minecraft:entity.horse.step"),
            Self::Mule => Some("minecraft:entity.horse.step"),
            Self::Llama => Some("minecraft:entity.llama.step"),
        }
    }

    /// `SpawnEggItem` 的物品 ID；牛刷怪蛋已有，僵尸/尸壳的 ID 也在数据表里。
    pub const fn spawn_egg(self) -> i32 {
        use mc_bamboo_data::items;
        match self {
            Self::Cow => items::COW_SPAWN_EGG,
            Self::Zombie => items::ZOMBIE_SPAWN_EGG,
            Self::Husk => items::HUSK_SPAWN_EGG,
            Self::Skeleton => items::SKELETON_SPAWN_EGG,
            Self::Horse => items::HORSE_SPAWN_EGG,
            Self::Donkey => items::DONKEY_SPAWN_EGG,
            Self::Mule => items::MULE_SPAWN_EGG,
            Self::Llama => items::LLAMA_SPAWN_EGG,
        }
    }

    /// `/summon` 成功反馈里的翻译键。
    pub const fn translation_key(self) -> &'static str {
        match self {
            Self::Cow => "entity.minecraft.cow",
            Self::Zombie => "entity.minecraft.zombie",
            Self::Husk => "entity.minecraft.husk",
            Self::Skeleton => "entity.minecraft.skeleton",
            Self::Horse => "entity.minecraft.horse",
            Self::Donkey => "entity.minecraft.donkey",
            Self::Mule => "entity.minecraft.mule",
            Self::Llama => "entity.minecraft.llama",
        }
    }

    /// 幼年是不是用 `Zombie.DATA_BABY_ID`（布尔元数据），而不是牛的 `age < 0`。
    pub const fn uses_baby_metadata(self) -> bool {
        matches!(self, Self::Zombie | Self::Husk)
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
/// 官方 `DeathMessageType`，决定 `CombatTracker.getDeathMessage` 走哪条分支。
///
/// Vanilla `DeathMessageType`: which branch `CombatTracker.getDeathMessage` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathMessageType {
    Default,
    FallVariants,
    IntentionalGameDesign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    Wither,
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
            Self::Wither => "minecraft:wither",
            Self::Explosion => "minecraft:explosion",
        }
    }

    /// 官方 `DamageType.deathMessageType()`：26.3 的数据文件里只有
    /// `minecraft:fall` / `minecraft:ender_pearl` 是 `FALL_VARIANTS`，
    /// `minecraft:bad_respawn_point` 是 `INTENTIONAL_GAME_DESIGN`。
    ///
    /// Vanilla `DamageType.deathMessageType()`: only fall damage and ender
    /// pearl use `FALL_VARIANTS`; bad respawn point uses the linked variant.
    pub const fn death_message_type(self) -> DeathMessageType {
        match self {
            Self::Fall => DeathMessageType::FallVariants,
            _ => DeathMessageType::Default,
        }
    }

    /// `DamageTypeTags.IS_FALL` / `ALWAYS_MOST_SIGNIFICANT_FALL`。
    pub const fn is_fall(self) -> bool {
        matches!(self, Self::Fall)
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
            Self::Wither => "wither",
            Self::Explosion => "explosion",
        }
    }

    /// 按官方短名找一种伤害。未知名字返回 `None`，调用方按解析失败处理。
    ///
    /// Finds a kind by its short official name. Unknown names return `None`.
    pub fn by_name(name: &str) -> Option<Self> {
        let all: [DamageKind; 23] = [
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
            Self::Wither,
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
        ) || matches!(self, Self::Wither)
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
                | Self::Wither
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

    /// `#bypasses_resistance`：26.3 里只有 `out_of_world` / `generic_kill`。
    pub const fn bypasses_resistance(self) -> bool {
        matches!(self, Self::OutOfWorld | Self::GenericKill)
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
    pub projectile_motion: Option<Vec3>,
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
    /// 火矢命中时，在受伤判定前尝试点燃目标的刻数。
    ///
    /// If nonzero, a flaming arrow tries to ignite the target before damage.
    pub arrow_ignite_ticks: i32,
    pub(super) looting_level: i32,
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
            projectile_motion: None,
            raw_position: None,
            sprint_yaw: None,
            attacker_uuid: None,
            scale_when_non_player: false,
            weapon: None,
            arrow_ignite_ticks: 0,
            looting_level: 0,
        }
    }

    /// LivingEntity.dealDefaultKnockback：弹射物取速度反向，其余取攻击点。
    ///
    /// LivingEntity.dealDefaultKnockback uses reversed projectile motion or the source position.
    pub fn knockback_direction(&self, target: Vec3) -> Option<(f64, f64)> {
        if let Some(delta) = self.projectile_motion {
            Some((-delta.x, -delta.z))
        } else {
            self.position.map(|at| (at.x - target.x, at.z - target.z))
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

    /// 记录致因玩家的 UUID 和命中时的主手抢夺。
    ///
    /// Capture the causing player's UUID and their main-hand Looting at impact.
    pub(crate) fn with_player_mob_attack(mut self, player: PlayerMobAttack) -> Self {
        self.attacker_uuid = Some(player.uuid);
        self.looting_level = player.looting;
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
            projectile_motion: None,
            raw_position: None,
            sprint_yaw: None,
            attacker_uuid: None,
            scale_when_non_player: false,
            weapon: None,
            arrow_ignite_ticks: 0,
            looting_level: 0,
        }
    }

    /// `DamageSource.scalesWithDifficulty()`：`always` 恒缩放，
    /// `when_caused_by_living_non_player` 只在造成者是非玩家生物时缩放。
    pub const fn scales_with_difficulty(self) -> bool {
        self.kind.scales_with_difficulty() || self.scale_when_non_player
    }
}

/// 命中生物时取的玩家信息；弹射物也按致因玩家当前持有物计算抢夺。
///
/// Player data at a mob hit; projectiles also use their owner's current main hand.
#[derive(Debug, Clone, Copy)]
pub struct PlayerMobAttack {
    pub uuid: [u8; 16],
    pub looting: i32,
}

/// 掉落表区分致死来源和最近伤害过它的玩家。
///
/// Death loot distinguishes the fatal source from the last player who hurt the mob.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobDeathLoot {
    pub victim: MobType,
    pub on_fire: bool,
    pub source: DamageSource,
    pub recent_player: Option<[u8; 16]>,
    pub recent_player_id: Option<EntityUid>,
}

impl MobDeathLoot {
    pub(super) fn new(me: &super::Entity, mob: &Mob, source: DamageSource) -> Self {
        let credited = mob.last_hurt_by_player_memory_time > 0;
        Self {
            victim: mob.mob_type,
            on_fire: me.remaining_fire_ticks > 0,
            source,
            recent_player: mob.last_hurt_by_player_ref.filter(|_| credited),
            recent_player_id: mob.last_hurt_by_player.filter(|_| credited),
        }
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
    /// `activeEffects`。只存相对剩余刻数，整行随实体搬，不需要时钟平移。
    ///
    /// `activeEffects`. Relative countdowns only; moved with the entity row.
    pub effects: EffectMap,
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
    // ---- Monster（牛恒为缺省值，不参与它的 tick）----
    /// `Mob.target`：当前攻击目标的 uid。玩家不在实体表里，按 uid 在玩家表找。
    pub target: Option<EntityUid>,
    /// `Mob.setAggressive`：`ZombieAttackGoal` 抬手时置真，同步进 `MobFlags` 的第 4 位。
    pub aggressive: bool,
    pub using_item_flags: u8,
    /// `Zombie.canBreakDoors`；`finalizeSpawn` 按 0.1 概率置真。
    pub can_break_doors: bool,
    /// `BreakDoorGoal` 的 240 刻进度，随实体行搬岛。
    pub door_break_time: i32,
    pub door_break_progress: i32,
    pub door_break_pos: Option<BlockPos>,
    /// 目标选择器。牛是空的，僵尸/尸壳按官方优先级注册。
    pub target_goals: GoalSelector,
    /// `Zombie.DATA_BABY_ID`：僵尸用布尔元数据，不用 `age < 0`。
    pub zombie_baby: bool,
    /// `Zombie.DATA_SPECIAL_TYPE_ID`（僵尸村民类型，本期恒 0）。
    pub zombie_special_type: i32,
    /// 六个人形装备槽、掉落几率和上次同步出去的装备（通用生物层，牛的全空）。
    pub equipment: equipment::MobEquipment,
    /// `Zombie.DATA_DROWNED_CONVERSION_ID`（转化后放，元数据恒 false）。
    pub drowned_converting: bool,
    /// `MoveToBlockGoal.nextStartTick`：海龟蛋目标的倒计时。
    pub turtle_egg_next_start_tick: i32,
    /// `MoveToBlockGoal.tryTicks` / `RemoveBlockGoal.ticksSinceReachedGoal`。
    pub turtle_egg_try_ticks: i32,
    pub turtle_egg_ticks_since_reached: i32,
    pub turtle_egg_pos: Option<BlockPos>,
    /// `MoveToBlockGoal.maxStayTicks` / `reachedTarget`。
    pub turtle_egg_max_stay: i32,
    pub turtle_egg_reached: bool,
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
    pub equine: Option<Box<equine::EquineState>>,
}

/// 要发给客户端的一次生物事件。
#[derive(Debug, Clone, PartialEq)]
pub enum MobEvent {
    /// `LivingEntity.handleEquipmentChanges`：`sendToTrackingPlayers` 一个 `set_equipment`，
    /// 变了的槽按 ordinal。
    Equipment(
        Vec<(
            mc_bamboo_protocol::packet::play::EquipmentSlot,
            mc_bamboo_protocol::item::ItemStack,
        )>,
    ),
    /// 别的 `broadcastEntityEvent`：装备坏掉（47–52）、主副手互换（55）。
    EntityEvent { event: u8 },
    /// `broadcastDamageEvent`：受了一次完整的伤害。
    Damage {
        kind: DamageKind,
        attacker: Option<EntityUid>,
        direct: Option<EntityUid>,
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
        /// `Monster.getSoundSource()` 是 HOSTILE，牛是 NEUTRAL。
        hostile: bool,
    },
}

impl Mob {
    /// `EntityType.create`：构造函数链。随机数（UUID 两个 `nextLong`、`yRot` 一个
    /// `nextFloat`）由调用方在实体拿到 ID 之后取，见 [`super::Entity::cow`]。
    pub fn cow() -> Self {
        Self::new(MobType::Cow)
    }

    /// 按类型构造。牛的属性/目标/导航一条不改，其它类型只换这三张表。
    pub fn new(mob_type: MobType) -> Self {
        let attributes = match mob_type {
            MobType::Cow => Attributes::cow(),
            MobType::Skeleton => Attributes::skeleton(),
            MobType::Zombie | MobType::Husk => Attributes::zombie(),
            kind => equine::attributes(kind),
        };
        let follow_range = attributes.base(mc_bamboo_data::registries::attribute::FOLLOW_RANGE);
        let mut navigation = Navigation::new(follow_range);
        let goals = match mob_type {
            MobType::Cow => GoalSelector::cow(&mut navigation),
            MobType::Skeleton => GoalSelector::skeleton(),
            MobType::Zombie | MobType::Husk => GoalSelector::zombie(&mut navigation),
            kind => equine::goals(kind, &mut navigation),
        };
        let target_goals = match mob_type {
            MobType::Skeleton => GoalSelector::skeleton_targets(),
            MobType::Zombie | MobType::Husk => GoalSelector::zombie_targets(),
            MobType::Cow => GoalSelector::default(),
            kind => equine::targets(kind),
        };
        let health = attributes.value(mc_bamboo_data::registries::attribute::MAX_HEALTH) as f32;
        Self {
            mob_type,
            uuid: [0; 16],
            x_rot: 0.0,
            y_rot_o: 0.0,
            x_rot_o: 0.0,
            eye_height: mob_type.dimensions(false).2,
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
            effects: EffectMap::new(),
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
            malus: if mob_type.is_monster() {
                Malus::monster()
            } else {
                Malus::animal()
            },
            persistence_required: false,
            left_handed: false,
            no_ai: false,
            can_pick_up_loot: false,
            home: None,
            loot_table_seed: 0,
            target: None,
            aggressive: false,
            using_item_flags: 0,
            can_break_doors: false,
            door_break_time: 0,
            door_break_progress: -1,
            door_break_pos: None,
            target_goals,
            zombie_baby: false,
            zombie_special_type: 0,
            equipment: equipment::MobEquipment::default(),
            drowned_converting: false,
            turtle_egg_next_start_tick: 0,
            turtle_egg_try_ticks: 0,
            turtle_egg_ticks_since_reached: 0,
            turtle_egg_pos: None,
            turtle_egg_max_stay: 0,
            turtle_egg_reached: false,
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
            equine: mob_type
                .is_equine()
                .then(|| Box::new(equine::EquineState::default())),
        }
    }

    pub fn is_baby(&self) -> bool {
        if self.mob_type.uses_baby_metadata() {
            self.zombie_baby
        } else {
            self.age < 0
        }
    }

    /// `Zombie.setBaby`：换元数据、尺寸和 `+0.5` 速度修饰符。
    pub fn set_zombie_baby(&mut self, baby: bool) {
        self.zombie_baby = baby;
        // `Zombie.onSyncedDataUpdated` 会 `refreshDimensions()`，眼高随
        // `Zombie.BABY_DIMENSIONS` 变成 0.775（成年是 1.74）。
        self.eye_height = self.mob_type.dimensions(baby).2;
        if let Some(speed) = self
            .attributes
            .instance(mc_bamboo_data::registries::attribute::MOVEMENT_SPEED)
        {
            speed.remove_modifier("minecraft:baby");
            if baby {
                speed.modifiers.push(attributes::Modifier {
                    id: "minecraft:baby".to_owned(),
                    amount: 0.5,
                    operation: attributes::Operation::AddMultipliedBase,
                    permanent: false,
                });
            }
        }
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
    ///
    /// Shift the absolute timestamps by `delta` when islands merge.
    pub fn rebase(&mut self, delta: i64) {
        self.last_damage_stamp = self.last_damage_stamp.saturating_add(delta);
        self.navigation.rebase(delta);
        // `MeleeAttackGoal.lastCanUseCheck` 也是 `gameTime`，牛没有这个目标，平移到它身上是空操作。
        // `MeleeAttackGoal.lastCanUseCheck` is `gameTime` as well; cows have no such goal,
        // so rebasing it is a no-op for them.
        self.goals.rebase_clock(delta);
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
    pub can_parent_equine: bool,
    /// 马科要同步的元数据；其余生物是 None。
    ///
    /// The synced equine metadata; None for other mobs.
    pub equine: Option<equine::EquinePublic>,
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
    /// `Mob.DATA_MOB_FLAGS_ID` 的攻击性位；牛恒为假。
    pub aggressive: bool,
    pub using_item_flags: u8,
    /// `Zombie.DATA_BABY_ID`（布尔元数据），牛不用。
    pub zombie_baby: bool,
    /// `Zombie.DATA_SPECIAL_TYPE_ID`。
    pub zombie_special_type: i32,
    /// `Zombie.DATA_DROWNED_CONVERSION_ID`。
    pub drowned_converting: bool,
    /// 非空的装备槽，按 ordinal；配对时的 `set_equipment` 用它。
    pub equipment: Vec<(
        mc_bamboo_protocol::packet::play::EquipmentSlot,
        mc_bamboo_protocol::item::ItemStack,
    )>,
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
            can_parent_equine: equine::can_parent(mob),
            equine: mob.equine.as_deref().map(equine::EquinePublic::of),
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
            aggressive: mob.aggressive,
            using_item_flags: mob.using_item_flags,
            zombie_baby: mob.zombie_baby,
            zombie_special_type: mob.zombie_special_type,
            drowned_converting: mob.drowned_converting,
            equipment: mob
                .equipment
                .iter()
                .filter(|(_, stack)| !stack.is_empty())
                .map(|(slot, stack)| (slot, stack.clone()))
                .collect(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::Entity;
    use mc_bamboo_data::registries::attribute;

    /// 官方 `Zombie.createAttributes` 的四个覆盖值和元数据 index 都从字节码导出。
    #[test]
    fn zombie_attributes_and_metadata_match_the_official_values() {
        let zombie = Mob::new(MobType::Zombie);
        let cow = Mob::new(MobType::Cow);
        assert_eq!(zombie.attributes.value(attribute::MAX_HEALTH), 20.0);
        assert_eq!(zombie.attributes.value(attribute::FOLLOW_RANGE), 35.0);
        assert_eq!(
            zombie.attributes.value(attribute::MOVEMENT_SPEED),
            f64::from(0.23_f32)
        );
        assert_eq!(zombie.attributes.value(attribute::ATTACK_DAMAGE), 3.0);
        assert_eq!(zombie.attributes.value(attribute::ARMOR), 2.0);
        assert_eq!(cow.attributes.value(attribute::MAX_HEALTH), 10.0);
        assert_eq!(MobType::Zombie.dimensions(false), (0.6, 1.95, 1.74));
        assert_eq!(MobType::Zombie.dimensions(true), (0.49, 0.98, 0.775));
        assert_eq!(zombie.target_goals.goals.len(), 5);
        assert_eq!(zombie.goals.goals.len(), 7);
        assert_eq!(cow.target_goals.goals.len(), 0);
        assert!(!MobType::Husk.burns_in_daylight());
        assert!(MobType::Zombie.burns_in_daylight());
    }

    /// 幼年僵尸用布尔元数据并换速度修饰符；牛仍按 `age < 0`。
    #[test]
    fn baby_metadata_is_type_specific() {
        let mut zombie = Mob::new(MobType::Zombie);
        assert!(!zombie.is_baby());
        zombie.set_zombie_baby(true);
        assert!(zombie.is_baby());
        assert_eq!(
            zombie.attributes.value(attribute::MOVEMENT_SPEED),
            f64::from(0.23_f32) * 1.5
        );
        let mut cow = Mob::new(MobType::Cow);
        assert!(!cow.is_baby());
        cow.age = -24000;
        assert!(cow.is_baby());
        assert!(MobPublic::of(&cow, 0).baby);
    }

    /// 存档往返保留僵尸的 `IsBaby` / `CanBreakDoors`，牛的键不串类型。
    #[test]
    fn zombie_nbt_round_trip_keeps_monster_fields() {
        let mut entity = Entity::zombie(Vec3::new(0.5, 64.0, 0.5));
        entity.with_mob(|_, mob| {
            mob.set_zombie_baby(true);
            mob.can_break_doors = true;
        });
        let row = entity.split();
        let tag = nbt::write_mob(&row).expect("write zombie");
        let mc_bamboo_nbt::Tag::Compound(entries) = &tag else {
            panic!("mob NBT is a compound");
        };
        let has = |key: &str| entries.iter().any(|(name, _)| name == key);
        assert!(has("IsBaby"));
        assert!(has("CanBreakDoors"));
        assert!(!has("Age"));
        assert!(!has("variant"));
        let loaded = nbt::read_mob(&tag, "minecraft:zombie", 7)
            .expect("read zombie")
            .expect("modelled");
        let mut rebuilt = loaded.into_entity();
        rebuilt.with_mob(|_, mob| {
            assert!(mob.is_baby());
            assert!(mob.can_break_doors);
            assert_eq!(mob.mob_type, MobType::Zombie);
        });
    }
}
